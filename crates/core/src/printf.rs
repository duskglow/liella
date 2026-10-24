//! Formatted output (spec §12.6; D-12.4, D-12.5, D-12.12): the `printf` format language and the
//! rendering of typed arguments, for byte output (`printf`) and wide output (`wprintf`).
//!
//! A format is [`parse`]d into literal runs and conversion [`Conv`]s, each conversion naming the
//! index of the argument it formats (and of its `*` width and precision), so that positional
//! (`%2$d`) and sequential formats are one model; [`Parsed::classes`] says how each argument is
//! read from a `va_list`. [`render`] formats the arguments into an [`Out`], whose unit is a byte
//! or a wide character: every conversion's body is produced in the output's units and then
//! padded, so width and precision count what the destination counts.
//!
//! Registered conversions (D-12.8) enter through two hooks: a [`Registry`] while parsing (the
//! modifiers, and the arguments a registered letter takes) and [`Env::custom`] while
//! rendering (the handler's output, which is the whole field).

#![allow(clippy::arithmetic_side_effects, clippy::too_many_lines)] // counts and offsets within the format and the body buffers

use crate::error::{Errno, Result};
use crate::mem::{InlineVec, TryVec};
use crate::num::float::{Float, Format, Rounding};
use crate::num::format::{self as nf, Case, Sink, Style};

/// Conversion flags.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Flags {
    /// `-`: left-justify.
    pub left: bool,
    /// `+`: always a sign.
    pub plus: bool,
    /// ` `: a space for a positive sign.
    pub space: bool,
    /// `#`: the alternate form.
    pub alt: bool,
    /// `0`: pad with zeros.
    pub zero: bool,
    /// `'`: group digits by the locale.
    pub group: bool,
    /// `I`: the locale's digits.
    pub i18n: bool,
}

/// `INT_MAX`, the largest width or precision a call may use (owner ruling R3).
const INT_MAX: usize = 0x7fff_ffff;

/// A field width or precision.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Count {
    /// Not given.
    #[default]
    None,
    /// Given in the format.
    Fixed(usize),
    /// From the argument with this index (`*`, `*m$`).
    Arg(usize),
}

/// A length modifier.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Len {
    /// None: `int`, `double`.
    #[default]
    None,
    /// `hh`.
    Char,
    /// `h`.
    Short,
    /// `l`.
    Long,
    /// `ll`.
    LongLong,
    /// `L`, `q`: `long double` (and `long long` for integer conversions).
    LongDouble,
    /// `j`.
    Max,
    /// `z`, `Z`.
    Size,
    /// `t`.
    Ptrdiff,
    /// `wN` (C23): an `N`-bit integer.
    Bits(u16),
    /// `wfN` (C23): the fastest type of at least `N` bits.
    Fast(u16),
}

/// One conversion specification.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Conv {
    /// The flags.
    pub flags: Flags,
    /// The field width.
    pub width: Count,
    /// The precision.
    pub prec: Count,
    /// The length modifier.
    pub len: Len,
    /// The bit of the registered modifier written before the length, 0 for none (D-12.8).
    pub user: u16,
    /// The conversion character.
    pub letter: u32,
    /// The index of the first argument it formats.
    pub arg: usize,
    /// How many arguments it formats, from `arg` on: 1, none for `%m`, or what a registered
    /// conversion's argument function asked for.
    pub nargs: usize,
    /// The letter has a registered handler (D-12.8).
    pub custom: bool,
    /// Where the specification starts in the format (its `%`) and ends (after the letter).
    pub span: (usize, usize),
}

/// A piece of a format.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Piece {
    /// Literal text: a range of the format.
    Text(usize, usize),
    /// A conversion.
    Conv(Conv),
    /// A specification that is not a conversion: copied as it is (a range of the format).
    Bad(usize, usize),
    /// The format ends inside a specification: rendering fails with `EINVAL` here, after the
    /// text before it.
    #[default]
    Truncated,
}

/// How an argument is read from a `va_list`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Class {
    /// An integer or a pointer (a general-purpose register slot). Arguments a format names but
    /// never uses are read as this.
    #[default]
    Int,
    /// A `double`.
    Double,
    /// A `long double`.
    LongDouble,
    /// A type added with `register_printf_type` (D-12.8): read by its own function into an
    /// object of `size` bytes.
    User {
        /// The type's number.
        ty: u32,
        /// The object's size, from the conversion's argument function.
        size: usize,
    },
}

/// The pieces and argument classes a format keeps inline (most formats have fewer).
const INLINE_PIECES: usize = 16;

/// The bytes of a converted number or a short text kept inline while it is built.
const INLINE_TEXT: usize = 128;

/// The units of one formatted field kept inline (longer fields move to the heap).
const INLINE_FIELD: usize = 256;

/// A number's text while it is built.
type Text = InlineVec<u8, INLINE_TEXT>;

/// A formatted field before padding.
pub type Field<U> = InlineVec<U, INLINE_FIELD>;

/// A parsed format.
#[derive(Debug)]
pub struct Parsed {
    /// The pieces in order.
    pub pieces: InlineVec<Piece, INLINE_PIECES>,
    /// The class of each argument, by index.
    pub classes: InlineVec<Class, INLINE_PIECES>,
    /// A width or precision written in the format exceeds `INT_MAX`: rendering fails with
    /// `EOVERFLOW` before anything is output (owner ruling R3, 2026-10-05).
    pub overflow: bool,
}

/// An argument's value as read.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Arg {
    /// An integer or a pointer: the register's 64 bits (narrower types are converted by the
    /// conversion that uses them).
    Int(u64),
    /// A `double`'s bits.
    Double(u64),
    /// A `long double`'s 80 bits, in the low bits.
    LongDouble(u128),
    /// A registered type's value: the address of the object its function filled.
    User(usize),
}

impl Default for Arg {
    fn default() -> Arg {
        Arg::Int(0)
    }
}

/// What registered conversions (`register_printf_specifier`, `register_printf_modifier`,
/// D-12.8) add to the format language while a format is parsed.
pub trait Registry {
    /// The longest registered modifier at the start of the units `at(0)`, `at(1)`, ... (0 past
    /// the end): its length in units and its bit.
    fn modifier(&self, at: &dyn Fn(usize) -> u32) -> Option<(usize, u16)>;
    /// Whether `conv`'s letter has a registered handler, and the arguments it takes; `first`
    /// is the index its first argument will have.
    fn conversion(&mut self, conv: &Conv, first: usize) -> Result<Registered>;
}

/// A letter's registration, as [`Registry::conversion`] reports it.
#[derive(Debug)]
pub enum Registered {
    /// No handler: the built-in conversion, if any.
    No,
    /// A handler, with the built-in rules for the arguments (no argument function, or it
    /// declined).
    Builtin,
    /// A handler, and the classes of the arguments its argument function asked for.
    Args(TryVec<Class>),
}

/// The format language without registrations.
pub struct Plain;

impl Registry for Plain {
    fn modifier(&self, _: &dyn Fn(usize) -> u32) -> Option<(usize, u16)> {
        None
    }

    fn conversion(&mut self, _: &Conv, _: usize) -> Result<Registered> {
        Ok(Registered::No)
    }
}

fn digit_value(c: u32) -> Option<usize> {
    (u32::from(b'0')..=u32::from(b'9')).contains(&c).then(|| usize::try_from(c - u32::from(b'0')).unwrap_or(0))
}

/// The conversion characters: integers, floating, characters and text, pointer, count, error
/// text.
fn is_conversion(c: u32) -> bool {
    u8::try_from(c).is_ok_and(|b| b"diouxXbBfFeEgGaAcCsSpnm".contains(&b))
}

/// The class of a built-in conversion's argument.
fn builtin_class(letter: u32, len: Len) -> Class {
    match u8::try_from(letter).unwrap_or(0) {
        b'f' | b'F' | b'e' | b'E' | b'g' | b'G' | b'a' | b'A' => {
            if len == Len::LongDouble { Class::LongDouble } else { Class::Double }
        }
        _ => Class::Int,
    }
}

/// `NL_ARGMAX`: the highest argument position a format may name (`%n$`, `*m$`); POSIX allows
/// `[1, NL_ARGMAX]`. A higher one is `EINVAL` (D-12.18: found by fuzzing, a position such as
/// `%100000000$s` made the parse record that many argument classes, gigabytes, before failing).
pub const NL_ARGMAX: usize = 4096;

/// Parse `fmt` (bytes or wide units). `reg` gives the registered modifiers and conversions
/// (D-12.8): a modifier is matched once, after the precision and before the length; a
/// registered letter takes the arguments its argument function asks for, in order from its
/// position.
pub fn parse<U: Copy + Into<u32>>(fmt: &[U], reg: &mut dyn Registry) -> Result<Parsed> {
    let mut p = Parsed::new();
    parse_into(fmt, reg, &mut p)?;
    Ok(p)
}

impl Parsed {
    /// An empty parse, to be filled by [`parse_into`].
    pub fn new() -> Parsed {
        Parsed { pieces: InlineVec::new(), classes: InlineVec::new(), overflow: false }
    }
}

impl Default for Parsed {
    fn default() -> Parsed {
        Parsed::new()
    }
}

/// [`parse`] into `p` (an empty parse), which the caller keeps in place: the parse is kept
/// inline and is not moved.
pub fn parse_into<U: Copy + Into<u32>>(fmt: &[U], reg: &mut dyn Registry, p: &mut Parsed) -> Result<()> {
    let at = |i: usize| fmt.get(i).map_or(0, |&u| u.into());
    let mut next = 0usize; // the next sequential argument
    let mut i = 0usize;
    let mut lit = 0usize;
    let pct = u32::from(b'%');
    // Read a decimal number at `j`: its value and the index after it.
    let number = |mut j: usize| -> (Option<usize>, usize) {
        let mut v: Option<usize> = None;
        while let Some(d) = digit_value(at(j)) {
            v = Some(v.unwrap_or(0).saturating_mul(10).saturating_add(d));
            j += 1;
        }
        (v, j)
    };
    let set_class = |idx: usize, c: Class, classes: &mut InlineVec<Class, INLINE_PIECES>| -> Result<()> {
        while classes.len() <= idx {
            classes.push(Class::Int)?;
        }
        if let Some(slot) = classes.get_mut(idx) {
            *slot = c;
        }
        Ok(())
    };
    while i < fmt.len() {
        if at(i) != pct {
            i += 1;
            continue;
        }
        if lit < i {
            p.pieces.push(Piece::Text(lit, i))?;
        }
        let start = i;
        i += 1;
        if at(i) == pct {
            p.pieces.push(Piece::Text(i, i + 1))?;
            i += 1;
            lit = i;
            continue;
        }
        let mut c = Conv { span: (start, start), ..Conv::default() };
        // `n$`: the argument's position.
        let mut positional = None;
        if let (Some(n), j) = number(i) {
            if at(j) == u32::from(b'$') && n > 0 {
                if n > NL_ARGMAX {
                    return Err(Errno::EINVAL);
                }
                positional = Some(n - 1);
                i = j + 1;
            }
        }
        // Flags.
        loop {
            match u8::try_from(at(i)).unwrap_or(0) {
                b'-' => c.flags.left = true,
                b'+' => c.flags.plus = true,
                b' ' => c.flags.space = true,
                b'#' => c.flags.alt = true,
                b'0' => c.flags.zero = true,
                b'\'' => c.flags.group = true,
                b'I' => c.flags.i18n = true,
                _ => break,
            }
            i += 1;
        }
        // A `*` count: positional (`*m$`) or the next argument.
        let star = |i: &mut usize, next: &mut usize| -> Result<Count> {
            if let (Some(m), j) = number(*i) {
                if at(j) == u32::from(b'$') && m > 0 {
                    if m > NL_ARGMAX {
                        return Err(Errno::EINVAL);
                    }
                    *i = j + 1;
                    return Ok(Count::Arg(m - 1));
                }
            }
            let idx = *next;
            *next += 1;
            Ok(Count::Arg(idx))
        };
        // Width.
        if at(i) == u32::from(b'*') {
            i += 1;
            c.width = star(&mut i, &mut next)?;
        } else if let (Some(w), j) = number(i) {
            c.width = Count::Fixed(w);
            p.overflow |= w > INT_MAX;
            i = j;
        }
        // Precision.
        if at(i) == u32::from(b'.') {
            i += 1;
            if at(i) == u32::from(b'*') {
                i += 1;
                c.prec = star(&mut i, &mut next)?;
            } else {
                let (v, j) = number(i);
                c.prec = Count::Fixed(v.unwrap_or(0));
                p.overflow |= v.is_some_and(|v| v > INT_MAX);
                i = j;
            }
        }
        // A registered modifier: the longest match.
        if let Some((n, bit)) = reg.modifier(&|k| at(i + k)) {
            i += n;
            c.user = bit;
        }
        // Length.
        let ch = |k: usize| u8::try_from(at(k)).unwrap_or(0);
        c.len = match ch(i) {
            b'h' if ch(i + 1) == b'h' => {
                i += 2;
                Len::Char
            }
            b'h' => {
                i += 1;
                Len::Short
            }
            b'l' if ch(i + 1) == b'l' => {
                i += 2;
                Len::LongLong
            }
            b'l' => {
                i += 1;
                Len::Long
            }
            b'L' | b'q' => {
                i += 1;
                Len::LongDouble
            }
            b'j' => {
                i += 1;
                Len::Max
            }
            b'z' | b'Z' => {
                i += 1;
                Len::Size
            }
            b't' => {
                i += 1;
                Len::Ptrdiff
            }
            b'w' => {
                let fast = ch(i + 1) == b'f';
                let from = if fast { i + 2 } else { i + 1 };
                match number(from) {
                    (Some(n), j) => {
                        i = j;
                        let n = u16::try_from(n).unwrap_or(0);
                        if fast { Len::Fast(n) } else { Len::Bits(n) }
                    }
                    _ => Len::None,
                }
            }
            _ => Len::None,
        };
        c.letter = at(i);
        if i >= fmt.len() {
            p.pieces.push(Piece::Truncated)?;
            lit = fmt.len();
            break;
        }
        // `%` after flags, width or precision prints `%` (the reference library ignores them).
        if c.letter == pct {
            p.pieces.push(Piece::Text(i, i + 1))?;
            i += 1;
            lit = i;
            continue;
        }
        let builtin = is_conversion(c.letter);
        let first = positional.unwrap_or(next);
        let registered = reg.conversion(&c, first)?;
        if !builtin && matches!(registered, Registered::No) {
            // Not a conversion: the text from `%` on is output as it is.
            let end = (i + 1).min(fmt.len());
            p.pieces.push(Piece::Bad(start, end))?;
            i = end;
            lit = i;
            continue;
        }
        i += 1;
        c.span = (start, i);
        c.custom = !matches!(registered, Registered::No);
        // The built-in rules: one argument, none for `%m` or a letter only a handler knows.
        let own = [builtin_class(c.letter, c.len)];
        let args: &[Class] = match &registered {
            Registered::Args(v) => v.as_slice(),
            _ if builtin && c.letter != u32::from(b'm') => &own,
            _ => &[],
        };
        c.arg = first;
        c.nargs = args.len();
        if positional.is_none() {
            next += c.nargs;
        }
        for (k, class) in args.iter().enumerate() {
            set_class(first + k, *class, &mut p.classes)?;
        }
        for count in [c.width, c.prec] {
            if let Count::Arg(a) = count {
                if p.classes.len() <= a {
                    set_class(a, Class::Int, &mut p.classes)?;
                }
            }
        }
        p.pieces.push(Piece::Conv(c))?;
        lit = i;
    }
    if lit < fmt.len() {
        p.pieces.push(Piece::Text(lit, fmt.len()))?;
    }
    Ok(())
}

// ------------------------------------------------------------------------------ rendering

/// What rendering needs from the caller: argument memory, the locale, `errno`'s text.
pub trait Env {
    /// The text a non-NULL `%s` argument points at: up to its NUL, at most `max` bytes.
    fn text(&mut self, addr: usize, max: Option<usize>) -> &'static [u8];
    /// The wide string a non-NULL `%ls` argument points at: up to its NUL, at most `max`
    /// units.
    fn wide(&mut self, addr: usize, max: Option<usize>) -> &'static [i32];
    /// Store the count of units output so far for `%n` (`len` gives the pointee's size).
    fn store_count(&mut self, addr: usize, len: Len, count: usize);
    /// Append the text of `errno` as it was when the call began (`%m`).
    fn error_text(&mut self, out: &mut TryVec<u8>) -> Result<()>;
    /// The decimal point (`LC_NUMERIC`).
    fn radix(&self) -> &[u8];
    /// The thousands separator (`LC_NUMERIC`).
    fn thousands(&self) -> &[u8];
    /// The grouping (`LC_NUMERIC`): group sizes, the last repeating; 127 or 0 ends grouping.
    fn grouping(&self) -> &[u8];
    /// The locale's digit `d` (`I`), as multibyte text.
    fn digit(&self, d: u8) -> &[u8];
    /// Encode a wide character in the locale's multibyte encoding; `None` if it cannot be.
    fn encode(&self, wc: u32, out: &mut [u8; 16]) -> Option<usize>;
    /// Decode one multibyte character: the character and the bytes it took; `None` if the
    /// bytes are invalid or incomplete.
    fn decode(&self, text: &[u8]) -> Option<(u32, usize)>;
    /// The rounding mode of the floating conversions.
    fn rounding(&self) -> Rounding;
    /// Run a registered conversion's handler (D-12.8). The default declines, so the built-in
    /// conversion applies.
    fn custom(&mut self, call: &Call<'_>) -> Result<Custom> {
        let _ = call;
        Ok(Custom::Declined(TryVec::new()))
    }
}

/// A registered conversion as its handler sees it (D-12.8).
#[derive(Debug)]
pub struct Call<'a> {
    /// The specification.
    pub conv: &'a Conv,
    /// The field width (from the format or an argument).
    pub width: usize,
    /// The precision.
    pub prec: Option<usize>,
    /// Left-justified (`-`, or a negative `*` width).
    pub left: bool,
    /// The destination counts wide characters.
    pub wide: bool,
    /// Its arguments.
    pub args: &'a [Arg],
}

/// What a registered conversion's handler did: the units it output (bytes, or wide characters
/// for wide output), which are part of the result whatever happened next.
#[derive(Debug)]
pub enum Custom {
    /// It formatted the conversion.
    Done(TryVec<u32>),
    /// It declined: the built-in conversion follows, or the specification as written.
    Declined(TryVec<u32>),
    /// It failed: the call fails with this error.
    Failed(TryVec<u32>, Errno),
}

/// An output unit: a byte or a wide character.
pub trait Unit: Copy + Default + PartialEq + core::fmt::Debug + Into<u32> {
    /// The unit is a wide character.
    const WIDE: bool;
    /// An ASCII byte as a unit.
    fn ascii(b: u8) -> Self;
    /// A unit from its value (a byte keeps the low 8 bits).
    fn from_raw(u: u32) -> Self;
    /// Append multibyte `text` in this unit (decoded for wide output); `max` limits the units
    /// appended (whole characters only).
    fn from_text(env: &dyn Env, text: &[u8], max: Option<usize>, out: &mut Field<Self>) -> Result<()>;
    /// Append wide characters in this unit (encoded for byte output); `max` limits the units
    /// appended (whole characters only).
    fn from_wide(env: &dyn Env, wide: &[i32], max: Option<usize>, out: &mut Field<Self>) -> Result<()>;
    /// A character argument of `%c` (a byte) in this unit.
    fn from_byte(env: &dyn Env, b: u8, out: &mut Field<Self>) -> Result<()>;
}

impl Unit for u8 {
    const WIDE: bool = false;

    fn ascii(b: u8) -> u8 {
        b
    }

    fn from_raw(u: u32) -> u8 {
        u.to_le_bytes()[0]
    }

    fn from_text(_: &dyn Env, text: &[u8], max: Option<usize>, out: &mut Field<u8>) -> Result<()> {
        out.extend_from_slice(text.get(..max.unwrap_or(text.len()).min(text.len())).unwrap_or(&[]))
    }

    /// `%ls` into bytes: at most `max` bytes and no partial character (C17 7.21.6.1); nothing is
    /// converted once `max` bytes are written, so a character past the precision cannot fail
    /// the call (as observed: one that does not convert while room is left gives `EILSEQ`).
    fn from_wide(env: &dyn Env, wide: &[i32], max: Option<usize>, out: &mut Field<u8>) -> Result<()> {
        let mut n = 0usize;
        for &w in wide {
            if max.is_some_and(|m| n >= m) {
                break;
            }
            let mut b = [0u8; 16];
            let k = env.encode(u32::from_ne_bytes(w.to_ne_bytes()), &mut b).ok_or(Errno::EILSEQ)?;
            if max.is_some_and(|m| n + k > m) {
                break;
            }
            out.extend_from_slice(b.get(..k).unwrap_or(&[]))?;
            n += k;
        }
        Ok(())
    }

    fn from_byte(_: &dyn Env, b: u8, out: &mut Field<u8>) -> Result<()> {
        out.push(b)
    }
}

impl Unit for u32 {
    const WIDE: bool = true;

    fn ascii(b: u8) -> u32 {
        u32::from(b)
    }

    fn from_raw(u: u32) -> u32 {
        u
    }

    fn from_text(env: &dyn Env, text: &[u8], max: Option<usize>, out: &mut Field<u32>) -> Result<()> {
        let mut i = 0usize;
        let mut n = 0usize;
        while i < text.len() && !max.is_some_and(|m| n >= m) {
            let (c, k) = env.decode(text.get(i..).unwrap_or(&[])).ok_or(Errno::EILSEQ)?;
            out.push(c)?;
            i += k.max(1);
            n += 1;
        }
        Ok(())
    }

    fn from_wide(_: &dyn Env, wide: &[i32], max: Option<usize>, out: &mut Field<u32>) -> Result<()> {
        for &w in wide.iter().take(max.unwrap_or(usize::MAX)) {
            out.push(u32::from_ne_bytes(w.to_ne_bytes()))?;
        }
        Ok(())
    }

    fn from_byte(env: &dyn Env, b: u8, out: &mut Field<u32>) -> Result<()> {
        let c = env.decode(&[b]).map_or(u32::from(b), |(c, _)| c);
        out.push(c)
    }
}

/// Where rendered units go.
pub trait Out<U: Unit> {
    /// Append units.
    fn put(&mut self, s: &[U]);
}

/// A byte sink over a number's text for the formatters.
struct Bytes<'a>(&'a mut Text, Option<Errno>);

impl Sink for Bytes<'_> {
    fn put(&mut self, b: &[u8]) {
        if self.0.extend_from_slice(b).is_err() {
            self.1 = Some(Errno::ENOMEM);
        }
    }
}

/// The value of a width or precision: from the format or an argument; a negative width means
/// left-justified, a negative precision none.
fn count(c: Count, args: &[Arg], is_width: bool, left: &mut bool) -> Option<usize> {
    match c {
        Count::None => None,
        Count::Fixed(n) => Some(n),
        Count::Arg(i) => {
            let v = match args.get(i) {
                Some(Arg::Int(v)) => i32::from_ne_bytes(u32::try_from(v & 0xFFFF_FFFF).unwrap_or(0).to_ne_bytes()),
                _ => 0,
            };
            if v < 0 {
                if is_width {
                    *left = true;
                    Some(usize::try_from(v.unsigned_abs()).unwrap_or(0))
                } else {
                    None
                }
            } else {
                Some(usize::try_from(v).unwrap_or(0))
            }
        }
    }
}

fn int_arg(args: &[Arg], i: usize) -> u64 {
    match args.get(i) {
        Some(Arg::Int(v)) => *v,
        _ => 0,
    }
}

/// The width in bits of an integer conversion's type (`int` without a modifier).
fn int_bits(len: Len) -> u32 {
    match len {
        Len::Char => 8,
        Len::Short => 16,
        Len::None => 32,
        Len::Long | Len::LongLong | Len::LongDouble | Len::Max | Len::Size | Len::Ptrdiff => 64,
        Len::Bits(n) => u32::from(n).clamp(1, 64),
        Len::Fast(n) => {
            if n <= 8 {
                8
            } else {
                64
            }
        }
    }
}

/// The argument as a signed value of the conversion's width.
fn signed(v: u64, bits: u32) -> i128 {
    if bits >= 64 {
        return i128::from(i64::from_ne_bytes(v.to_ne_bytes()));
    }
    let shift = 64 - bits;
    i128::from(i64::from_ne_bytes((v << shift).to_ne_bytes()) >> shift)
}

/// The argument as an unsigned value of the conversion's width.
fn unsigned(v: u64, bits: u32) -> u64 {
    if bits >= 64 { v } else { v & ((1u64 << bits) - 1) }
}

/// Insert the locale's thousands separators into the run of ASCII digits `digits`.
fn group(env: &dyn Env, digits: &[u8], out: &mut Text) -> Result<()> {
    let sep = env.thousands();
    let g = env.grouping();
    if sep.is_empty() || g.first().is_none_or(|&x| x == 0 || x == 127) {
        return out.extend_from_slice(digits);
    }
    // Group boundaries from the right.
    let mut cuts: InlineVec<usize, 32> = InlineVec::new();
    let mut pos = digits.len();
    let mut gi = 0usize;
    let mut size = 0usize;
    loop {
        if let Some(&s) = g.get(gi) {
            if s == 127 || s == 0 && gi == 0 {
                break;
            }
            if s != 0 {
                size = usize::from(s);
            }
            gi += 1;
        }
        if size == 0 || pos <= size {
            break;
        }
        pos -= size;
        cuts.push(pos)?;
        if g.get(gi).is_some_and(|&s| s == 127) {
            break;
        }
    }
    let mut from = 0usize;
    for &cut in cuts.iter().rev() {
        out.extend_from_slice(digits.get(from..cut).unwrap_or(&[]))?;
        out.extend_from_slice(sep)?;
        from = cut;
    }
    out.extend_from_slice(digits.get(from..).unwrap_or(&[]))
}

/// Replace ASCII digits by the locale's (`I`).
fn localize_digits(env: &dyn Env, text: &[u8], out: &mut Text) -> Result<()> {
    for &b in text {
        if b.is_ascii_digit() {
            out.extend_from_slice(env.digit(b - b'0'))?;
        } else {
            out.push(b)?;
        }
    }
    Ok(())
}

/// A floating conversion's text before padding.
#[derive(Debug)]
pub struct FloatText {
    /// The sign, the digits with the locale's radix, grouping and digits, the exponent.
    pub text: InlineVec<u8, INLINE_TEXT>,
    /// The length of the prefix that zero padding goes after (the sign, `0x`).
    pub prefix: usize,
    /// The value is neither infinite nor a NaN.
    pub finite: bool,
}

/// Format `x` (a value of `kind`) as `spec` says, with the sign, grouping and locale-digit
/// `flags` (`+`, space, `'`, `I`), in the locale of `env`.
pub fn float_text(env: &dyn Env, x: Float, kind: Format, spec: nf::Spec, flags: Flags) -> Result<FloatText> {
    if flags.group || flags.i18n || spec.style == Style::Hex {
        return float_text_general(env, x, kind, spec, flags);
    }
    let mut text = Text::new();
    let (prefix, finite) = float_text_into(env, x, kind, spec, flags, &mut text)?;
    Ok(FloatText { text, prefix, finite })
}

/// [`float_text`] of the common case (no grouping, no locale digits, not hex) appended to
/// `text`: the sign and the formatter's output go straight in, without the intermediate
/// copies of the general path (`%e` p 6 spent a third of its instructions on them) and
/// without moving a result around (a move of the 150-byte text is a `memcpy` call). Returns
/// the prefix length and whether the value is finite. The formatter writes `-` itself.
pub fn float_text_into(env: &dyn Env, x: Float, kind: Format, spec: nf::Spec, flags: Flags, text: &mut InlineVec<u8, INLINE_TEXT>) -> Result<(usize, bool)> {
    debug_assert!(!flags.group && !flags.i18n && spec.style != Style::Hex);
    if !x.neg {
        if flags.plus {
            text.push(b'+')?;
        } else if flags.space {
            text.push(b' ')?;
        }
    }
    let prefix = text.len().saturating_add(usize::from(x.neg));
    let mut b = Bytes(text, None);
    nf::format_float(&mut b, x, kind, spec, env.radix(), env.rounding());
    if let Some(e) = b.1 {
        return Err(e);
    }
    let finite = !matches!(x.value, crate::num::float::Value::Inf | crate::num::float::Value::Nan { .. });
    Ok((prefix, finite))
}

/// [`float_text`] for every case: it formats into a scratch buffer, then rebuilds the text
/// with the sign, the grouping and the locale's digits.
pub fn float_text_general(env: &dyn Env, x: Float, kind: Format, spec: nf::Spec, flags: Flags) -> Result<FloatText> {
    let mut raw = Text::new();
    let mut radix = Text::new();
    radix.extend_from_slice(env.radix())?;
    let mut b = Bytes(&mut raw, None);
    nf::format_float(&mut b, x, kind, spec, &radix, env.rounding());
    if let Some(e) = b.1 {
        return Err(e);
    }
    let hex = spec.style == Style::Hex;
    let finite = !matches!(x.value, crate::num::float::Value::Inf | crate::num::float::Value::Nan { .. });
    let (neg, mag) = match raw.split_first() {
        Some((b'-', rest)) => (true, rest),
        _ => (false, &raw[..]),
    };
    let mut text = Text::new();
    if neg {
        text.push(b'-')?;
    } else if flags.plus {
        text.push(b'+')?;
    } else if flags.space {
        text.push(b' ')?;
    }
    if hex && finite {
        text.extend_from_slice(mag.get(..2).unwrap_or(&[]))?;
    }
    let prefix = text.len();
    let mag = if hex && finite { mag.get(2..).unwrap_or(&[]) } else { mag };
    let mut grouped = Text::new();
    if flags.group && !hex && finite {
        // The integer digits come first, up to the radix or the exponent.
        let int_end = mag.iter().position(|b| !b.is_ascii_digit()).unwrap_or(mag.len());
        group(env, mag.get(..int_end).unwrap_or(&[]), &mut grouped)?;
        grouped.extend_from_slice(mag.get(int_end..).unwrap_or(&[]))?;
    } else {
        grouped.extend_from_slice(mag)?;
    }
    if flags.i18n && !hex {
        localize_digits(env, &grouped, &mut text)?;
    } else {
        text.extend_from_slice(&grouped)?;
    }
    Ok(FloatText { text, prefix, finite })
}

/// Render `parsed` (of the format `fmt`) with `args` into `out`: the units output.
pub fn render<U: Unit, O: Out<U>>(fmt: &[U], parsed: &Parsed, args: &[Arg], env: &mut dyn Env, out: &mut O) -> Result<usize> {
    // A width or a precision beyond INT_MAX in the format fails the call with EOVERFLOW (owner
    // ruling R3, 2026-10-05: as the reference does, consistent with the rule for a result beyond
    // INT_MAX), before anything is output; `*` counts are `int`s and cannot exceed it.
    if parsed.overflow {
        return Err(Errno::EOVERFLOW);
    }
    let mut total = 0usize;
    let mut body: Field<U> = InlineVec::new();
    let emit = |s: &[U], out: &mut O, total: &mut usize| {
        out.put(s);
        *total += s.len();
    };
    for piece in parsed.pieces.iter() {
        match *piece {
            Piece::Text(a, b) | Piece::Bad(a, b) => emit(fmt.get(a..b).unwrap_or(&[]), out, &mut total),
            Piece::Truncated => return Err(Errno::EINVAL),
            Piece::Conv(c) => {
                body.clear();
                let mut left = c.flags.left;
                let width = count(c.width, args, true, &mut left).unwrap_or(0);
                let prec = count(c.prec, args, false, &mut left);
                // The prefix that zero padding goes after (sign, `0x`), and whether zero padding
                // applies.
                let mut prefix_len = 0usize;
                let mut zero_ok = c.flags.zero && !left;
                if c.custom {
                    // A registered handler's output is the whole field; what it wrote stays
                    // even when it declines or fails.
                    let call = Call { conv: &c, width, prec, left, wide: U::WIDE, args: args.get(c.arg..c.arg + c.nargs).unwrap_or(&[]) };
                    let outcome = env.custom(&call)?;
                    let (Custom::Done(written) | Custom::Declined(written) | Custom::Failed(written, _)) = &outcome;
                    for &u in written.iter() {
                        emit(&[U::from_raw(u)], out, &mut total);
                    }
                    match outcome {
                        Custom::Done(_) => continue,
                        Custom::Failed(_, e) => return Err(e),
                        Custom::Declined(_) if !is_conversion(c.letter) => {
                            emit(fmt.get(c.span.0..c.span.1).unwrap_or(&[]), out, &mut total);
                            continue;
                        }
                        Custom::Declined(_) => {}
                    }
                }
                let letter = u8::try_from(c.letter).unwrap_or(0);
                match letter {
                    b'd' | b'i' | b'u' | b'o' | b'x' | b'X' | b'b' | b'B' => {
                        let bits = int_bits(c.len);
                        let raw = int_arg(args, c.arg);
                        let (neg, mag) = if matches!(letter, b'd' | b'i') {
                            let v = signed(raw, bits);
                            (v < 0, u64::try_from(v.unsigned_abs()).unwrap_or(u64::MAX))
                        } else {
                            (false, unsigned(raw, bits))
                        };
                        let radix = match letter {
                            b'o' => 8,
                            b'x' | b'X' => 16,
                            b'b' | b'B' => 2,
                            _ => 10,
                        };
                        let case = if matches!(letter, b'X' | b'B') { Case::Upper } else { Case::Lower };
                        let mut dbuf = [0u8; 64];
                        let digits: &[u8] = if mag == 0 && prec == Some(0) { &[] } else { nf::u64_digits(&mut dbuf, mag, radix, case) };
                        // Precision: the minimum number of digits.
                        let mut min = prec.unwrap_or(0);
                        if letter == b'o' && c.flags.alt && digits.first() != Some(&b'0') {
                            min = min.max(digits.len() + 1);
                        }
                        let mut text: InlineVec<u8, 8> = InlineVec::new();
                        if neg {
                            text.push(b'-')?;
                        } else if matches!(letter, b'd' | b'i') && c.flags.plus {
                            text.push(b'+')?;
                        } else if matches!(letter, b'd' | b'i') && c.flags.space {
                            text.push(b' ')?;
                        }
                        if c.flags.alt && mag != 0 {
                            match letter {
                                b'x' => text.extend_from_slice(b"0x")?,
                                b'X' => text.extend_from_slice(b"0X")?,
                                b'b' => text.extend_from_slice(b"0b")?,
                                b'B' => text.extend_from_slice(b"0B")?,
                                _ => {}
                            }
                        }
                        prefix_len = text.len();
                        U::from_text(env, &text, None, &mut body)?;
                        for _ in digits.len()..min {
                            U::from_text(env, b"0", None, &mut body)?;
                        }
                        if (c.flags.group || c.flags.i18n) && radix == 10 {
                            let mut grouped = Text::new();
                            if c.flags.group {
                                group(env, digits, &mut grouped)?;
                            } else {
                                grouped.extend_from_slice(digits)?;
                            }
                            let mut local = Text::new();
                            if c.flags.i18n {
                                localize_digits(env, &grouped, &mut local)?;
                            } else {
                                local.extend_from_slice(&grouped)?;
                            }
                            U::from_text(env, &local, None, &mut body)?;
                        } else {
                            U::from_text(env, digits, None, &mut body)?;
                        }
                        if prec.is_some() {
                            zero_ok = false;
                        }
                    }
                    b'f' | b'F' | b'e' | b'E' | b'g' | b'G' | b'a' | b'A' => {
                        let (x, kind) = match args.get(c.arg) {
                            Some(Arg::Double(b)) => (Float::unpack(u128::from(*b), Format::F64), Format::F64),
                            Some(Arg::LongDouble(b)) => (Float::unpack(*b, Format::F80), Format::F80),
                            _ => (Float::unpack(0, Format::F64), Format::F64),
                        };
                        let style = match letter {
                            b'f' | b'F' => Style::Fixed,
                            b'e' | b'E' => Style::Exp,
                            b'g' | b'G' => Style::General,
                            _ => Style::Hex,
                        };
                        let case = if letter.is_ascii_uppercase() { Case::Upper } else { Case::Lower };
                        let t = float_text(env, x, kind, nf::Spec { style, case, precision: prec, alternate: c.flags.alt }, c.flags)?;
                        prefix_len = t.prefix;
                        if !t.finite {
                            zero_ok = false;
                        }
                        U::from_text(env, &t.text, None, &mut body)?;
                    }
                    b'c' | b'C' => {
                        zero_ok = false;
                        let v = int_arg(args, c.arg);
                        if letter == b'C' || c.len == Len::Long {
                            let w = [i32::from_ne_bytes(u32::try_from(v & 0xFFFF_FFFF).unwrap_or(0).to_ne_bytes())];
                            U::from_wide(env, &w, None, &mut body)?;
                        } else {
                            U::from_byte(env, u8::try_from(v & 0xFF).unwrap_or(0), &mut body)?;
                        }
                    }
                    b's' | b'S' => {
                        zero_ok = false;
                        let addr = usize::try_from(int_arg(args, c.arg)).unwrap_or(0);
                        if addr == 0 {
                            // `(null)` (GNU), unless the precision is too short to hold it.
                            if prec.is_none_or(|p| p >= 6) {
                                U::from_text(env, b"(null)", None, &mut body)?;
                            }
                        } else if letter == b'S' || c.len == Len::Long {
                            // A precision bounds the read in both cases: N bytes hold at most N
                            // characters, and the array need not be terminated (C17 7.21.6.1).
                            let wide = env.wide(addr, prec);
                            U::from_wide(env, wide, prec, &mut body)?;
                        } else {
                            let text = if core::mem::size_of::<U>() == 1 { env.text(addr, prec) } else { env.text(addr, None) };
                            U::from_text(env, text, prec, &mut body)?;
                        }
                    }
                    b'p' => {
                        zero_ok = false;
                        let v = int_arg(args, c.arg);
                        let mut text = Text::new();
                        if v == 0 {
                            text.extend_from_slice(b"(nil)")?;
                        } else {
                            if c.flags.plus {
                                text.push(b'+')?;
                            } else if c.flags.space {
                                text.push(b' ')?;
                            }
                            text.extend_from_slice(b"0x")?;
                            let mut b = Bytes(&mut text, None);
                            nf::format_u64(&mut b, v, 16, Case::Lower);
                            if let Some(e) = b.1 {
                                return Err(e);
                            }
                        }
                        U::from_text(env, &text, None, &mut body)?;
                    }
                    b'n' => {
                        let addr = usize::try_from(int_arg(args, c.arg)).unwrap_or(0);
                        env.store_count(addr, c.len, total);
                        continue;
                    }
                    b'm' => {
                        zero_ok = false;
                        let mut t: TryVec<u8> = TryVec::new();
                        env.error_text(&mut t)?;
                        U::from_text(env, &t, prec, &mut body)?;
                    }
                    _ => {
                        // Not reached: only conversions and registered letters are parsed as
                        // conversions, and a declined registered letter is output above.
                        emit(fmt.get(c.span.0..c.span.1).unwrap_or(&[]), out, &mut total);
                        continue;
                    }
                }
                // Padding.
                let pad = width.saturating_sub(body.len());
                if pad == 0 {
                    emit(&body, out, &mut total);
                } else if left {
                    emit(&body, out, &mut total);
                    for _ in 0..pad {
                        emit(&[U::ascii(b' ')], out, &mut total);
                    }
                } else if zero_ok {
                    let split = prefix_len.min(body.len());
                    emit(body.get(..split).unwrap_or(&[]), out, &mut total);
                    for _ in 0..pad {
                        emit(&[U::ascii(b'0')], out, &mut total);
                    }
                    emit(body.get(split..).unwrap_or(&[]), out, &mut total);
                } else {
                    for _ in 0..pad {
                        emit(&[U::ascii(b' ')], out, &mut total);
                    }
                    emit(&body, out, &mut total);
                }
            }
        }
    }
    Ok(total)
}

// ------------------------------------------------------------------------------ simple formats

/// One conversion of a simple format (see [`simple`]), small so that a whole format's fit in
/// a few registers' worth of stack.
#[derive(Clone, Copy, Debug)]
struct SimpleConv {
    /// At most four digits; 0 for a `*` width ([`F_STAR`]).
    width: u16,
    /// The integer type's width in bits (`int_bits` of the length modifier).
    bits: u8,
    /// The conversion letter; `%` for `%%`.
    letter: u8,
    /// A floating conversion's precision (`.N`, at most two digits); [`NO_PREC`] for none.
    prec: u8,
    /// The flags, packed so the struct stays six bytes: [`F_LEFT`] ...
    flags: u8,
}

const F_LEFT: u8 = 1;
const F_ZERO: u8 = 2;
const F_ALT: u8 = 4;
const F_PLUS: u8 = 8;
const F_SPACE: u8 = 16;
/// Set on `e E f F g G`.
const F_FLOAT: u8 = 32;
/// Set on a `*` width: its `int` argument comes before the value.
const F_STAR: u8 = 64;

impl SimpleConv {
    fn has(self, f: u8) -> bool {
        self.flags & f != 0
    }
}

/// The bytes of an [`int_field`] buffer: the widest field it renders (a sign and 20 digits fit).
pub const INT_FIELD: usize = 32;

/// The whole field of a right-justified integer conversion (`d i u x X`, no [`F_LEFT`]) of the
/// argument word `raw`, `width` (at most [`INT_FIELD`]) wide, built backwards at the end of
/// `buf`: digits, then zero padding and the sign, or the sign and space padding. Returns where
/// the field starts. One buffer and one write per conversion (PRINTF-LAYERS); the same bytes as
/// the general path.
#[inline(always)]
fn int_field(c: SimpleConv, width: usize, raw: u64, buf: &mut [u8; INT_FIELD]) -> usize {
    let end = buf.len();
    let mut i = end;
    let mut put = |i: &mut usize, b: u8| {
        *i = i.wrapping_sub(1);
        if let Some(d) = buf.get_mut(*i) {
            *d = b;
        }
    };
    let neg = match c.letter {
        b'x' | b'X' => {
            let digits: &[u8; 16] = if c.letter == b'x' { b"0123456789abcdef" } else { b"0123456789ABCDEF" };
            let mut v = unsigned(raw, u32::from(c.bits));
            loop {
                put(&mut i, digits.get(usize::try_from(v & 0xF).unwrap_or(0)).copied().unwrap_or(b'0'));
                v >>= 4;
                if v == 0 {
                    break false;
                }
            }
        }
        _ => {
            let (mut v, neg) = if c.letter == b'u' {
                (unsigned(raw, u32::from(c.bits)), false)
            } else {
                let s = signed(raw, u32::from(c.bits));
                (u64::try_from(s.unsigned_abs()).unwrap_or(u64::MAX), s < 0)
            };
            // Two digits per division by 100, as `nf::u64_digits` (NEXT-REGRESS: one digit per
            // division cost ls -l / tar -tv about 1% of their instructions).
            while v >= 100 {
                let r = usize::try_from(v % 100).unwrap_or(0) * 2;
                v /= 100;
                put(&mut i, DIGIT_PAIRS.get(r + 1).copied().unwrap_or(b'0'));
                put(&mut i, DIGIT_PAIRS.get(r).copied().unwrap_or(b'0'));
            }
            if v >= 10 {
                let r = usize::try_from(v).unwrap_or(0) * 2;
                put(&mut i, DIGIT_PAIRS.get(r + 1).copied().unwrap_or(b'0'));
                put(&mut i, DIGIT_PAIRS.get(r).copied().unwrap_or(b'0'));
            } else {
                put(&mut i, b'0' + u8::try_from(v).unwrap_or(0));
            }
            neg
        }
    };
    let start = end.saturating_sub(width.min(end));
    // Padding is stored as one fill of its range, not a byte at a time.
    let fill = |buf: &mut [u8; INT_FIELD], from: usize, to: usize, b: u8| {
        if let Some(r) = buf.get_mut(from..to) {
            r.fill(b);
        }
    };
    if c.has(F_ZERO) {
        // Zeros up to the field's start, less the sign's byte, then the sign.
        let stop = start.saturating_add(usize::from(neg));
        if i > stop {
            fill(buf, stop, i, b'0');
            i = stop;
        }
        if neg {
            i = i.wrapping_sub(1);
            if let Some(d) = buf.get_mut(i) {
                *d = b'-';
            }
        }
    } else {
        if neg {
            i = i.wrapping_sub(1);
            if let Some(d) = buf.get_mut(i) {
                *d = b'-';
            }
        }
        if i > start {
            fill(buf, start, i, b' ');
            i = start;
        }
    }
    i
}

/// The decimal digit pairs `00` .. `99` of [`int_field`].
const DIGIT_PAIRS: &[u8; 200] = b"00010203040506070809101112131415161718192021222324252627282930313233343536373839404142434445464748495051525354555657585960616263646566676869707172737475767778798081828384858687888990919293949596979899";

/// A plan of one right-justified integer conversion of at most [`INT_FIELD`] bytes and no other
/// conversion ([`SimpleFormat::lone_int`]): its field is rendered whole by [`LoneInt::field`].
#[derive(Clone, Copy, Debug)]
pub struct LoneInt {
    /// Where the conversion starts in the format.
    pub at: usize,
    /// Where the text after it starts.
    pub after: usize,
    conv: SimpleConv,
}

impl LoneInt {
    /// The field for the argument word `raw` (read as [`render_simple`] reads it), at the end of `buf`.
    #[inline(always)]
    pub fn field(self, raw: u64, buf: &mut [u8; INT_FIELD]) -> &[u8] {
        let start = int_field(self.conv, usize::from(self.conv.width), raw, buf);
        buf.get(start..).unwrap_or(&[])
    }
}

/// [`SimpleConv::prec`] of a conversion without a precision.
const NO_PREC: u8 = u8::MAX;

/// [`SimpleConv::prec`] of a floating conversion with a `*` precision: its `int` comes after
/// the width's (if that is `*` too) and before the value; a negative one counts as none.
const PREC_STAR: u8 = u8::MAX - 1;

/// The conversion whose `%` is at `i` in a simple format and the offset after it; `Err(())`
/// when it is outside the subset.
#[allow(clippy::result_unit_err)]
fn simple_conv(fmt: &[u8], i: usize) -> core::result::Result<(SimpleConv, usize), ()> {
    let mut j = i + 1;
    if fmt.get(j) == Some(&b'%') {
        return Ok((SimpleConv { width: 0, bits: 0, letter: b'%', prec: NO_PREC, flags: 0 }, j + 1));
    }
    // The commonest conversions, with no flag, width, precision or length: no field scan.
    if let Some(&letter @ (b'd' | b'i' | b'u' | b'x' | b'X' | b's' | b'c')) = fmt.get(j) {
        return Ok((SimpleConv { width: 0, bits: 32, letter, prec: NO_PREC, flags: 0 }, j + 1));
    }
    if let (Some(b'l'), Some(&letter @ (b'd' | b'i' | b'u' | b'x' | b'X'))) = (fmt.get(j), fmt.get(j + 1)) {
        return Ok((SimpleConv { width: 0, bits: 64, letter, prec: NO_PREC, flags: 0 }, j + 2));
    }
    let mut flags = 0u8;
    loop {
        match fmt.get(j) {
            Some(b'-') => flags |= F_LEFT,
            Some(b'0') => flags |= F_ZERO,
            Some(b'#') => flags |= F_ALT,
            Some(b'+') => flags |= F_PLUS,
            Some(b' ') => flags |= F_SPACE,
            _ => break,
        }
        j += 1;
    }
    let mut width = 0u16;
    let mut digits = 0;
    while let Some(&d) = fmt.get(j).filter(|d| d.is_ascii_digit()) {
        // At most four digits: wider fields (and their INT_MAX rule) take the general path.
        digits += 1;
        if digits > 4 {
            return Err(());
        }
        width = width * 10 + u16::from(d - b'0');
        j += 1;
    }
    // Width then the letter, with no precision or length modifier (`%08x`): no further scan.
    if let Some(&letter @ (b'd' | b'i' | b'u' | b'x' | b'X' | b's' | b'c')) = fmt.get(j) {
        if flags & (F_ALT | F_PLUS | F_SPACE) == 0 {
            return Ok((SimpleConv { width, bits: 32, letter, prec: NO_PREC, flags }, j + 1));
        }
    }
    // A `*` width right after the flags (`%*s`, `%-*s`, `%*.*f`): parsed out of line, with no
    // precision or length scan here (SPRINTF-RENDER).
    if digits == 0 && fmt.get(j) == Some(&b'*') {
        return simple_star(fmt, j + 1, flags);
    }
    // A precision (floating conversions only): at most two digits, or `*`.
    let mut prec = NO_PREC;
    if fmt.get(j) == Some(&b'.') {
        j += 1;
        prec = simple_prec(fmt, &mut j)?;
    }
    let (bits, len) = match (fmt.get(j), fmt.get(j + 1)) {
        (Some(b'h'), Some(b'h')) => (8, 2),
        (Some(b'h'), _) => (16, 1),
        (Some(b'l'), Some(b'l')) => (64, 2),
        (Some(b'l' | b'j' | b'z' | b't'), _) => (64, 1),
        _ => (32, 0),
    };
    j += len;
    let plain = prec == NO_PREC && flags & (F_ALT | F_PLUS | F_SPACE) == 0;
    match fmt.get(j) {
        Some(&letter @ (b'd' | b'i' | b'u' | b'x' | b'X')) if plain => Ok((SimpleConv { width, bits, letter, prec, flags }, j + 1)),
        Some(&letter @ (b's' | b'c')) if len == 0 && plain => Ok((SimpleConv { width, bits, letter, prec, flags }, j + 1)),
        Some(&letter @ (b'e' | b'E' | b'f' | b'F' | b'g' | b'G')) if len == 0 => Ok((SimpleConv { width, bits, letter, prec, flags: flags | F_FLOAT }, j + 1)),
        // A `*` width right after the flags: parsed out of line, so the written-width
        // conversions above pay nothing for it.
        Some(b'*') if digits == 0 && len == 0 && prec == NO_PREC => simple_star(fmt, j + 1, flags),
        _ => Err(()),
    }
}

/// The rest of a simple conversion after its flags and a `*` width (`%*s`, ls -l's link count
/// column), `j` being just past the `*`: a length modifier for `d i u x X`, then the letter. Its
/// `int` is read before the value ([`F_STAR`]). No digits, precision or floating conversion
/// (`simple_float` takes the width from the plan): those take the general path.
#[cold]
#[inline(never)]
fn simple_star(fmt: &[u8], mut j: usize, flags: u8) -> core::result::Result<(SimpleConv, usize), ()> {
    let flags = flags | F_STAR;
    // The letter right after the `*` (ls -l's `%*s` / `%-*s` columns): no precision, length or
    // floating scan (SPRINTF-RENDER: those cost `%*s` about 40 instructions a parse).
    if fmt.get(j) != Some(&b'.') && flags & (F_ALT | F_PLUS | F_SPACE) == 0 {
        if let Some(&letter @ (b's' | b'c' | b'd' | b'i' | b'u' | b'x' | b'X')) = fmt.get(j) {
            return Ok((SimpleConv { width: 0, bits: 32, letter, prec: NO_PREC, flags }, j + 1));
        }
    }
    // A floating conversion (gawk's `"%*.*f"`): a precision of at most two digits or `*`, then
    // the letter; its `int`s and the value are read by `simple_float_star`.
    let mut prec = NO_PREC;
    if fmt.get(j) == Some(&b'.') {
        j += 1;
        prec = simple_prec(fmt, &mut j)?;
    }
    if let Some(&letter @ (b'e' | b'E' | b'f' | b'F' | b'g' | b'G')) = fmt.get(j) {
        return Ok((SimpleConv { width: 0, bits: 0, letter, prec, flags: flags | F_FLOAT }, j + 1));
    }
    if prec != NO_PREC || flags & (F_ALT | F_PLUS | F_SPACE) != 0 {
        return Err(());
    }
    let (bits, len) = match (fmt.get(j), fmt.get(j + 1)) {
        (Some(b'h'), Some(b'h')) => (8, 2),
        (Some(b'h'), _) => (16, 1),
        (Some(b'l'), Some(b'l')) => (64, 2),
        (Some(b'l' | b'j' | b'z' | b't'), _) => (64, 1),
        _ => (32, 0),
    };
    j += len;
    match fmt.get(j) {
        Some(&letter @ (b'd' | b'i' | b'u' | b'x' | b'X')) => Ok((SimpleConv { width: 0, bits, letter, prec: NO_PREC, flags }, j + 1)),
        Some(&letter @ (b's' | b'c')) if len == 0 => Ok((SimpleConv { width: 0, bits, letter, prec: NO_PREC, flags }, j + 1)),
        _ => Err(()),
    }
}

/// The precision of a simple conversion, `j` being just past its `.`: at most two digits
/// (none is 0), or [`PREC_STAR`] for a `*`; `Err(())` for three digits or more.
fn simple_prec(fmt: &[u8], j: &mut usize) -> core::result::Result<u8, ()> {
    if fmt.get(*j) == Some(&b'*') {
        *j += 1;
        return Ok(PREC_STAR);
    }
    let mut p = 0u8;
    let mut n = 0;
    while let Some(&d) = fmt.get(*j).filter(|d| d.is_ascii_digit()) {
        n += 1;
        if n > 2 {
            return Err(());
        }
        p = p * 10 + (d - b'0');
        *j += 1;
    }
    Ok(p)
}

/// The conversions (`%%` included) [`simple_into`] keeps; [`render_simple`] parses any further
/// ones again as it goes.
const SIMPLE_MAX: usize = 8;

/// [`SimpleFormat`]'s unused entries.
const NO_CONV: (SimpleConv, u8, u8) = (SimpleConv { width: 0, bits: 0, letter: b'%', prec: NO_PREC, flags: 0 }, 0, 0);

/// The literal-run byte of a plan entry whose run before the `%` is too long to store: the
/// renderer searches for that `%`.
const LIT_SCAN: u8 = u8::MAX;

/// Set in [`SimpleFormat::kinds`] when the format has more conversions than the plan keeps.
const F_MORE: u8 = 128;

/// A simple format's first conversions, parsed once by [`simple_into`] for [`render_simple`]:
/// each conversion and its length from the `%` (the offsets are found again by the scan for
/// `%` that copies the literal text anyway). 64 bytes: two vector stores to set up; a larger
/// plan, or one returned by value, costs more than the second parse it saves.
#[derive(Clone, Copy, Debug)]
pub struct SimpleFormat {
    convs: [(SimpleConv, u8, u8); SIMPLE_MAX],
    n: u8,
    /// [`F_FLOAT`] and [`F_STAR`] of every conversion (kept or not), OR-ed: a floating one makes
    /// [`render_simple`] need the locale's radix and the rounding mode from its `env`; a `*` width
    /// selects the instance that reads widths (so formats without one run the code they did before).
    kinds: u8,
}

impl SimpleFormat {
    /// An empty plan for [`simple_into`].
    pub const fn new() -> SimpleFormat {
        SimpleFormat { convs: [NO_CONV; SIMPLE_MAX], n: 0, kinds: 0 }
    }

    /// For the plan of a format that is literal text around ONE plain `%s` (no width, no flags,
    /// nothing else to convert; tar -tv's `" %s"`): where its `%s` starts and where the text
    /// after it starts. [`render_simple`] would write the text before, the string, the text
    /// after - the caller may write exactly those (TAR-TV, D-12.21).
    pub fn lone_s(&self) -> Option<(usize, usize)> {
        if self.n != 1 || self.kinds != 0 {
            return None;
        }
        let &(c, len, lit) = self.convs.first()?;
        (c.letter == b's' && c.width == 0 && c.flags == 0 && lit != LIT_SCAN).then(|| (usize::from(lit), usize::from(lit) + usize::from(len)))
    }

    /// The plan of text around one right-justified integer conversion (`d i u x X`, no `-`, a
    /// width of at most [`INT_FIELD`], no `*`) and nothing else to convert (`%02x`, `%jd`): the
    /// caller may write the text before, [`LoneInt::field`], the text after (PRINTF-LAYERS).
    pub fn lone_int(&self) -> Option<LoneInt> {
        if self.n != 1 || self.kinds != 0 {
            return None;
        }
        let &(c, len, lit) = self.convs.first()?;
        let ok = matches!(c.letter, b'd' | b'i' | b'u' | b'x' | b'X') && !c.has(F_LEFT) && usize::from(c.width) <= INT_FIELD && lit != LIT_SCAN;
        ok.then(|| LoneInt { at: usize::from(lit), after: usize::from(lit) + usize::from(len), conv: c })
    }

    /// For the plan of text around ONE plain `%c` (no width, no flags, nothing else to convert;
    /// gawk's `"*.*%c"`): where its `%c` starts and where the text after it starts. The caller
    /// may write the text before, the argument's low byte, the text after (SPRINTF-RENDER).
    pub fn lone_c(&self) -> Option<(usize, usize)> {
        if self.n != 1 || self.kinds != 0 {
            return None;
        }
        let &(c, len, lit) = self.convs.first()?;
        (c.letter == b'c' && c.width == 0 && c.flags == 0 && lit != LIT_SCAN).then(|| (usize::from(lit), usize::from(lit) + usize::from(len)))
    }

    /// Whether the plan has exactly one conversion and no `*` width or floating one: the only
    /// plans [`SimpleFormat::lone_s`], [`SimpleFormat::lone_int`] and [`SimpleFormat::lone_c`] accept.
    pub fn is_lone(&self) -> bool {
        self.n == 1 && self.kinds == 0
    }

    /// Whether the format has a floating conversion (its `env` must be a full one).
    pub fn has_float(&self) -> bool {
        self.kinds & F_FLOAT != 0
    }

    /// Whether the format has a `*` width (render it with `STAR` true).
    pub fn has_star(&self) -> bool {
        self.kinds & F_STAR != 0
    }
}

impl Default for SimpleFormat {
    fn default() -> SimpleFormat {
        SimpleFormat::new()
    }
}

/// The longest format a [`PlanCache`] keeps (tar's `"%s %s/%s %*s %-*s"` is 18 bytes).
pub const PLAN_CACHE_FMT: usize = 40;

/// One format a [`PlanCache`] keeps: its bytes, and its plan when it is simple.
#[derive(Clone, Copy, Debug)]
struct CachedPlan {
    /// The format's length; 0 for an unused entry (an empty format is never cached).
    len: u8,
    /// Whether the format is simple (`plan` is then what [`simple_into`] made of it).
    simple: bool,
    text: [u8; PLAN_CACHE_FMT],
    plan: SimpleFormat,
}

const NO_CACHED: CachedPlan = CachedPlan { len: 0, simple: false, text: [0; PLAN_CACHE_FMT], plan: SimpleFormat::new() };

/// What [`PlanCache::fill`] found out about a format.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Planned {
    /// Simple: its plan is in the caller's `plan`.
    Simple,
    /// Not simple: the general parse renders it.
    General,
    /// Not cached (empty or longer than [`PLAN_CACHE_FMT`]): the caller decides as usual.
    Unknown,
}

/// The last two formats a stream printed and what [`simple_into`] made of them (PRINTF-STAR,
/// D-12.21): a hit costs a length and byte compare instead of the parse (tar -tv prints two
/// formats per member). Keyed on the format's BYTES, never its address alone: a caller may
/// build formats in a buffer it rewrites. A cache, not state: two caches always compare equal.
#[derive(Clone, Copy, Debug)]
pub struct PlanCache {
    entries: [CachedPlan; 2],
    /// The entry the next miss replaces.
    next: u8,
}

impl PlanCache {
    /// An empty cache.
    pub const fn new() -> PlanCache {
        PlanCache { entries: [NO_CACHED; 2], next: 0 }
    }

    /// Whether `fmt` is simple, its plan (on [`Planned::Simple`]) written into `plan`, which
    /// must be a new one: from the cache, or parsed by [`simple_into`] and kept.
    pub fn fill(&mut self, fmt: &[u8], plan: &mut SimpleFormat) -> Planned {
        let len = fmt.len();
        if len == 0 || len > PLAN_CACHE_FMT {
            return Planned::Unknown;
        }
        // Length first (one compare rejects most other formats), then the bytes.
        for e in &self.entries {
            if usize::from(e.len) == len && e.text.get(..len).is_some_and(|t| same_short(t, fmt)) {
                if e.simple {
                    *plan = e.plan;
                    return Planned::Simple;
                }
                return Planned::General;
            }
        }
        let simple = simple_into(fmt, plan);
        let slot = usize::from(self.next & 1);
        if let Some(e) = self.entries.get_mut(slot) {
            e.len = u8::try_from(len).unwrap_or(0);
            e.simple = simple;
            if let Some(t) = e.text.get_mut(..len) {
                t.copy_from_slice(fmt);
            }
            e.plan = if simple { *plan } else { SimpleFormat::new() };
        }
        self.next ^= 1;
        if simple { Planned::Simple } else { Planned::General }
    }

    /// [`PlanCache::fill`] without the copy (TAR-TV): on a hit the kept plan itself is lent
    /// (`Some` exactly on [`Planned::Simple`]); a miss parses and keeps the format as `fill` does.
    pub fn route(&mut self, fmt: &[u8]) -> (Planned, Option<&SimpleFormat>) {
        let len = fmt.len();
        if len == 0 || len > PLAN_CACHE_FMT {
            return (Planned::Unknown, None);
        }
        let hit = self.entries.iter().position(|e| usize::from(e.len) == len && e.text.get(..len).is_some_and(|t| same_short(t, fmt)));
        let at = match hit {
            Some(k) => k,
            None => {
                let mut plan = SimpleFormat::new();
                self.fill(fmt, &mut plan);
                // `fill` kept it in the entry it replaced, then moved `next` on.
                usize::from((self.next ^ 1) & 1)
            }
        };
        match self.entries.get(at) {
            Some(e) if e.simple => (Planned::Simple, Some(&e.plan)),
            Some(_) => (Planned::General, None),
            None => (Planned::Unknown, None),
        }
    }
}

/// Whether `a` and `b` (the same length, 1..=[`PLAN_CACHE_FMT`]) hold the same bytes: words
/// with an overlapping last one, the tail first (formats sharing a prefix differ late), in
/// line rather than through `bcmp` (a call cost about as much as the compare of a short format).
#[inline(always)]
fn same_short(a: &[u8], b: &[u8]) -> bool {
    let n = a.len();
    if n != b.len() {
        return false;
    }
    if n >= 8 {
        let w = |s: &[u8], i: usize| u64::from_ne_bytes(s.get(i..i + 8).and_then(|x| x.try_into().ok()).unwrap_or([0; 8]));
        if w(a, n - 8) != w(b, n - 8) {
            return false;
        }
        let mut i = 0;
        while i + 8 < n {
            if w(a, i) != w(b, i) {
                return false;
            }
            i += 8;
        }
        return true;
    }
    if n >= 4 {
        let w = |s: &[u8], i: usize| u32::from_ne_bytes(s.get(i..i + 4).and_then(|x| x.try_into().ok()).unwrap_or([0; 4]));
        return w(a, n - 4) == w(b, n - 4) && w(a, 0) == w(b, 0);
    }
    a.iter().zip(b).all(|(x, y)| x == y)
}

impl Default for PlanCache {
    fn default() -> PlanCache {
        PlanCache::new()
    }
}

impl PartialEq for PlanCache {
    /// A cache is not state: it never makes two streams' records differ.
    fn eq(&self, _: &PlanCache) -> bool {
        true
    }
}

impl Eq for PlanCache {}

/// [`simple_into`] into a new plan.
pub fn simple(fmt: &[u8]) -> Option<SimpleFormat> {
    let mut plan = SimpleFormat::new();
    simple_into(fmt, &mut plan).then_some(plan)
}

/// Whether `fmt` is a simple format: literal text and only `%%` and conversions `d i u x X`
/// (with no length modifier or `hh h l ll j z t`), `s` and `c`, with no flags but `-` and `0` and a width of at most four
/// digits or `*` (no precision, no positions), and `e E f F g G` (written width only) of a `double` (also with `# + space` and a
/// precision of at most two digits), each at most 255 bytes long; if so, its first
/// [`SIMPLE_MAX`] conversions are in `plan` (a new one). [`render_simple`] formats exactly
/// these as [`render`] does; the rest goes through the general parse.
pub fn simple_into(fmt: &[u8], plan: &mut SimpleFormat) -> bool {
    let mut i = 0;
    while let Some(k) = fmt.get(i..).and_then(|r| r.iter().position(|&b| b == b'%')) {
        let Ok((conv, next)) = simple_conv(fmt, i + k) else { return false };
        // A run of repeated flags can make a conversion longer than its stored length allows.
        let Ok(len) = u8::try_from(next - (i + k)) else { return false };
        // A `*` precision reads an `int` too: the `STAR` instance renders it.
        plan.kinds |= conv.flags & (F_FLOAT | F_STAR) | if conv.prec == PREC_STAR { F_STAR } else { 0 };
        if let Some(slot) = plan.convs.get_mut(usize::from(plan.n)) {
            // The literal run before the `%` (fits in the entry's spare byte): the renderer
            // need not search for the `%` again.
            *slot = (conv, len, u8::try_from(k).ok().filter(|&r| r != LIT_SCAN).unwrap_or(LIT_SCAN));
            plan.n += 1;
        } else {
            plan.kinds |= F_MORE;
        }
        i = next;
    }
    true
}

/// Render a [`simple`] format, `plan` being what [`simple_into`] made of `fmt`: each
/// conversion's argument is the next `next(is_double)` word (an integer or a pointer register slot, as
/// [`Class::Int`] reads it). The same output as [`render`] gives for it. Of `env`, only
/// [`Env::text`] is used.
pub fn render_simple<const FLOAT: bool, const STAR: bool, O: Out<u8>>(fmt: &[u8], plan: &SimpleFormat, next: &mut dyn FnMut() -> u64, next_double: &mut dyn FnMut() -> u64, env: &mut dyn Env, out: &mut O) -> Result<usize> {
    const SPACES: [u8; 16] = [b' '; 16];
    const ZEROS: [u8; 16] = [b'0'; 16];
    let mut total = 0usize;
    let put = |s: &[u8], out: &mut O, total: &mut usize| {
        if !s.is_empty() {
            out.put(s);
            *total += s.len();
        }
    };
    let pad = |n: usize, fill: &[u8; 16], out: &mut O, total: &mut usize| {
        let mut left = n;
        while left > 0 {
            let k = left.min(16);
            out.put(fill.get(..k).unwrap_or(&[]));
            *total += k;
            left -= k;
        }
    };
    let mut i = 0;
    let mut kept = plan.convs.get(..usize::from(plan.n)).unwrap_or(&[]).iter();
    let find = |i: usize| fmt.get(i..).and_then(|r| r.iter().position(|&b| b == b'%')).map(|k| i + k);
    loop {
        // The next conversion: one the plan kept (with its literal run, so no search for its
        // `%`), or past a full plan, found and parsed here.
        let (at, c, after) = if let Some(&(c, len, lit)) = kept.next() {
            let at = if lit != LIT_SCAN { i + usize::from(lit) } else { find(i).ok_or(Errno::EINVAL)? };
            (at, c, at + usize::from(len))
        } else {
            if plan.kinds & F_MORE == 0 {
                break;
            }
            let Some(at) = find(i) else { break };
            let (c, after) = simple_conv(fmt, at).map_err(|()| Errno::EINVAL)?;
            (at, c, after)
        };
        put(fmt.get(i..at).ok_or(Errno::EINVAL)?, out, &mut total);
        i = after;
        if c.letter == b'%' {
            put(b"%", out, &mut total);
            continue;
        }
        if FLOAT && c.has(F_FLOAT) {
            if c.has(F_STAR) || c.prec == PREC_STAR {
                // `*` width or precision (gawk's `"%*.*f"`): only the `STAR` instance reads
                // their `int`s; a plan without them never has such a conversion.
                if !STAR {
                    return Err(Errno::EINVAL);
                }
                total += simple_float_star(c, next, next_double, env, out)?;
                continue;
            }
            let precision = if c.prec == NO_PREC { None } else { Some(usize::from(c.prec)) };
            total += simple_float(c, usize::from(c.width), precision, next_double(), env, out)?;
            continue;
        }
        // A `*` width is the `int` before the value; only the `STAR` instance has this code, so
        // formats without one run exactly the written-width path.
        let mut c = c;
        let mut width = usize::from(c.width);
        if STAR && c.has(F_STAR) {
            let w = star_width(next);
            if w < 0 {
                c.flags |= F_LEFT;
            }
            width = usize::try_from(w.unsigned_abs()).unwrap_or(0);
        }
        let raw = next();
        // A string (ls -l's and tar -tv's `%*s` / `%-*s` columns) first: its text and the
        // padding, without the number checks and the digit buffer below (NEXT-REGRESS: those in
        // front of it cost tar -tv's format about 10% of render_simple's instructions).
        if c.letter == b's' {
            let addr = usize::try_from(raw).unwrap_or(0);
            let text: &[u8] = if addr == 0 { b"(null)" } else { env.text(addr, None) };
            let fill = width.saturating_sub(text.len());
            if fill == 0 {
                put(text, out, &mut total);
            } else if c.has(F_LEFT) {
                put(text, out, &mut total);
                pad(fill, &SPACES, out, &mut total);
            } else {
                pad(fill, &SPACES, out, &mut total);
                put(text, out, &mut total);
            }
            continue;
        }
        if matches!(c.letter, b'd' | b'i' | b'u' | b'x' | b'X') && !c.has(F_LEFT) && width <= INT_FIELD {
            // A right-justified integer: its whole field, one write.
            let mut fbuf = [0u8; INT_FIELD];
            let start = int_field(c, width, raw, &mut fbuf);
            put(fbuf.get(start..).unwrap_or(&[]), out, &mut total);
            continue;
        }
        if c.letter == b'c' {
            // One byte and its padding (gawk builds each float format with `"*.*%c"`): no digit
            // buffer (GAWK-PRINTF: setting it up cost this conversion about 28 instructions).
            let one = [u8::try_from(raw & 0xFF).unwrap_or(0)];
            let fill = width.saturating_sub(1);
            if fill == 0 {
                put(&one, out, &mut total);
            } else if c.has(F_LEFT) {
                put(&one, out, &mut total);
                pad(fill, &SPACES, out, &mut total);
            } else {
                pad(fill, &SPACES, out, &mut total);
                put(&one, out, &mut total);
            }
            continue;
        }
        let mut dbuf = [b'0'; 64];
        // A number's digits sit at the end of `dbuf` (`num` of them).
        let mut num = 0usize;
        // The text (other letters), the sign before a number (zero padding goes between), and
        // whether zeros pad.
        let (sign, text, zero_ok): (&[u8], &[u8], bool) = match c.letter {
            b'd' | b'i' => {
                let v = signed(raw, u32::from(c.bits));
                let mag = u64::try_from(v.unsigned_abs()).unwrap_or(u64::MAX);
                num = nf::u64_digits(&mut dbuf, mag, 10, Case::Lower).len();
                (if v < 0 { b"-" } else { b"" }, b"", c.has(F_ZERO))
            }
            b'x' | b'X' => {
                // Four bits a digit, straight from the table (the short digests of `%02x`).
                let digits: &[u8; 16] = if c.letter == b'x' { b"0123456789abcdef" } else { b"0123456789ABCDEF" };
                let mut v = unsigned(raw, u32::from(c.bits));
                let end = dbuf.len();
                loop {
                    num += 1;
                    if let Some(d) = dbuf.get_mut(end.wrapping_sub(num)) {
                        *d = digits.get(usize::try_from(v & 0xF).unwrap_or(0)).copied().unwrap_or(b'0');
                    }
                    v >>= 4;
                    if v == 0 {
                        break;
                    }
                }
                (b"", b"", c.has(F_ZERO))
            }
            b'u' => {
                num = nf::u64_digits(&mut dbuf, unsigned(raw, u32::from(c.bits)), 10, Case::Lower).len();
                (b"", b"", c.has(F_ZERO))
            }
            // A floating conversion in a plan rendered with `FLOAT` false: the caller broke the contract.
            _ => return Err(Errno::EINVAL),
        };
        if num != 0 && zero_ok && !c.has(F_LEFT) {
            // A zero-padded number (`%02x`): `dbuf` starts as all '0' and only the digits were
            // written, so the padding is already in place ahead of them. Only the sign (at the
            // field's start) is stored, and the whole field is one write.
            let start = dbuf.len().wrapping_sub(width.max(sign.len() + num));
            if start < dbuf.len() {
                if let (Some(&s), Some(b)) = (sign.first(), dbuf.get_mut(start)) {
                    *b = s;
                }
                put(dbuf.get(start..).unwrap_or(&[]), out, &mut total);
                continue;
            }
        }
        let body: &[u8] = if num != 0 { dbuf.get(dbuf.len() - num..).unwrap_or(&[]) } else { text };
        let len = sign.len() + body.len();
        let fill = width.saturating_sub(len);
        if fill == 0 {
            put(sign, out, &mut total);
            put(body, out, &mut total);
        } else if c.has(F_LEFT) {
            put(sign, out, &mut total);
            put(body, out, &mut total);
            pad(fill, &SPACES, out, &mut total);
        } else if zero_ok {
            put(sign, out, &mut total);
            pad(fill, &ZEROS, out, &mut total);
            put(body, out, &mut total);
        } else {
            pad(fill, &SPACES, out, &mut total);
            put(sign, out, &mut total);
            put(body, out, &mut total);
        }
    }
    put(fmt.get(i..).unwrap_or(&[]), out, &mut total);
    Ok(total)
}

/// A `*` width of a simple format: the next argument as an `int` (a negative one left-justifies
/// its magnitude, as `count` does on the general path).
fn star_width(next: &mut dyn FnMut() -> u64) -> i32 {
    i32::from_ne_bytes(u32::try_from(next() & 0xFFFF_FFFF).unwrap_or(0).to_ne_bytes())
}

/// One floating conversion of a simple format: the same text and padding as [`render`] gives
/// (`float_text`, then spaces or zeros after the sign up to the width). Apart from
/// [`render_simple`] so that the integer conversions' code stays small.
#[inline(never)]
fn simple_float<O: Out<u8>>(c: SimpleConv, width: usize, precision: Option<usize>, raw: u64, env: &mut dyn Env, out: &mut O) -> Result<usize> {
    let style = match c.letter {
        b'f' | b'F' => Style::Fixed,
        b'e' | b'E' => Style::Exp,
        _ => Style::General,
    };
    let case = if c.letter.is_ascii_uppercase() { Case::Upper } else { Case::Lower };
    let flags = Flags { plus: c.has(F_PLUS), space: c.has(F_SPACE), alt: c.has(F_ALT), ..Flags::default() };
    let x = Float::unpack(u128::from(raw), Format::F64);
    let mut ft = Text::new();
    // A simple format has neither grouping nor locale digits, and no `%a`: the direct form.
    let (prefix, finite) = float_text_into(env, x, Format::F64, nf::Spec { style, case, precision, alternate: c.has(F_ALT) }, flags, &mut ft)?;
    let text: &[u8] = &ft;
    let fill = width.saturating_sub(text.len());
    let mut total = text.len();
    if fill == 0 {
        out.put(text);
    } else if c.has(F_LEFT) {
        out.put(text);
        total += pad_with(b' ', fill, out);
    } else if c.has(F_ZERO) && finite {
        let (sign, body) = text.split_at(prefix.min(text.len()));
        out.put(sign);
        total += pad_with(b'0', fill, out);
        out.put(body);
    } else {
        total += pad_with(b' ', fill, out);
        out.put(text);
    }
    Ok(total)
}

/// A floating conversion of a simple format with a `*` width or precision: their `int`s
/// (width first), then the value, as the general path reads them. A negative width
/// left-justifies its magnitude; a negative precision counts as none. Any size is rendered
/// as [`render`] does (the same text and padding code); the caller checks the `int` total.
#[inline(never)]
fn simple_float_star<O: Out<u8>>(mut c: SimpleConv, next: &mut dyn FnMut() -> u64, next_double: &mut dyn FnMut() -> u64, env: &mut dyn Env, out: &mut O) -> Result<usize> {
    let mut width = usize::from(c.width);
    if c.has(F_STAR) {
        let w = star_width(next);
        if w < 0 {
            c.flags |= F_LEFT;
        }
        width = usize::try_from(w.unsigned_abs()).unwrap_or(usize::MAX);
    }
    let precision = match c.prec {
        NO_PREC => None,
        PREC_STAR => usize::try_from(star_width(next)).ok(),
        p => Some(usize::from(p)),
    };
    simple_float(c, width, precision, next_double(), env, out)
}

/// `n` copies of `b` into `out`, sixteen at a time; returns `n`.
fn pad_with<O: Out<u8>>(b: u8, n: usize, out: &mut O) -> usize {
    let fill = [b; 16];
    let mut left = n;
    while left > 0 {
        let k = left.min(16);
        out.put(fill.get(..k).unwrap_or(&[]));
        left -= k;
    }
    n
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::panic)]
mod simple_tests {
    use super::*;

    extern crate std;
    use std::vec::Vec;

    /// Strings at "addresses" 1, 2, 3.
    const TEXTS: [&[u8]; 3] = [b"", b"abc", b"a longer string"];

    struct TestEnv;

    impl Env for TestEnv {
        fn text(&mut self, addr: usize, max: Option<usize>) -> &'static [u8] {
            let t = TEXTS[addr - 1];
            &t[..max.unwrap_or(t.len()).min(t.len())]
        }
        fn wide(&mut self, _: usize, _: Option<usize>) -> &'static [i32] {
            &[]
        }
        fn store_count(&mut self, _: usize, _: Len, _: usize) {}
        fn error_text(&mut self, _: &mut TryVec<u8>) -> Result<()> {
            Ok(())
        }
        fn radix(&self) -> &[u8] {
            b"."
        }
        fn thousands(&self) -> &[u8] {
            b""
        }
        fn grouping(&self) -> &[u8] {
            b""
        }
        fn digit(&self, _: u8) -> &[u8] {
            b"0"
        }
        fn encode(&self, wc: u32, out: &mut [u8; 16]) -> Option<usize> {
            out[0] = u8::try_from(wc).ok()?;
            Some(1)
        }
        fn decode(&self, text: &[u8]) -> Option<(u32, usize)> {
            text.first().map(|&b| (u32::from(b), 1))
        }
        fn rounding(&self) -> Rounding {
            Rounding::Nearest
        }
    }

    impl Out<u8> for Vec<u8> {
        fn put(&mut self, s: &[u8]) {
            self.extend_from_slice(s);
        }
    }

    /// The direct `float_text` equals the general (scratch-buffer) one for every style, case,
    /// precision, `#`/`+`/space/group/i18n flag and value class.
    #[test]
    fn direct_float_text_equals_the_general_one() {
        use crate::num::float::Format;
        let env = TestEnv;
        let mut vals: Vec<(u64, Format)> = Vec::new();
        for v in [0.0f64, -0.0, 1.0, -1.0, 0.1, 2.5, 3.14159e-7, 123456.789, 1e300, 5e-324, 1.7976931348623157e308, 9.9999995, 999999.5, f64::INFINITY, f64::NEG_INFINITY, f64::NAN, -f64::NAN] {
            vals.push((v.to_bits(), Format::F64));
        }
        let mut n = 0usize;
        for (bits, fmt) in vals {
            let x = Float::unpack(u128::from(bits), fmt);
            for style in [Style::Exp, Style::Fixed, Style::General, Style::Hex] {
                for case in [Case::Lower, Case::Upper] {
                    for precision in [None, Some(0), Some(1), Some(6), Some(17), Some(40), Some(300)] {
                        for alternate in [false, true] {
                            for bits in 0..8u8 {
                                let flags = Flags { plus: bits & 1 != 0, space: bits & 2 != 0, group: bits & 4 != 0, ..Flags::default() };
                                let spec = nf::Spec { style, case, precision, alternate };
                                let a = float_text(&env, x, fmt, spec, flags).unwrap();
                                let b = float_text_general(&env, x, fmt, spec, flags).unwrap();
                                assert_eq!((&a.text[..], a.prefix, a.finite), (&b.text[..], b.prefix, b.finite), "{bits:#x} {style:?} {precision:?} {flags:?}");
                                n += 1;
                            }
                        }
                    }
                }
            }
        }
        assert!(n > 5000);
    }

    /// Every format of up to three tokens from the simple subset (and a few outside it, which
    /// [`simple`] must refuse) renders alike through both paths, for integers of every edge and
    /// for NULL and non-NULL strings.
    #[test]
    fn simple_formats_render_as_the_general_path_does() {
        let toks: [&[u8]; 41] = [
            b"ab", b"%%", b"%d", b"%5d", b"%-5d", b"%05d", b"%-05d", b"%ld", b"%i", b"%u", b"%x", b"%X", b"%08x", b"%lx", b"%lu", b"%s", b"%10s", b"%-10s", b"%05s", b"%c", b"%3c",
            b"%0c", b"%lld", b"%hhd", b"%hu", b"%zu", b"%jd", b"%td", b"%8llx", b"%hx", b"%12345d", b"%.2d", b"%*d", b"%ls", b"%Lf", b"%*s", b"%-*s", b"%0*d", b"%*lu",
            b"%*5d", b"%.*s",
        ];
        // `*` widths: zero, small, negative (left), -1, wide, and garbage above the `int`.
        let widths: [u64; 6] = [0, 3, 0xFFFF_FFFD, 0xFFFF_FFFF, 20, 0x1_0000_0005];
        let values: [u64; 9] = [0, 1, u64::MAX, 0xFFFF_FFFF_8000_0000, 0x7FFF_FFFF, 0x8000_0000, 255, 65, 0x1234_5678_9ABC_DEF0];
        let mut checked = 0usize;
        let mut formats: Vec<Vec<u8>> = std::vec![Vec::new()];
        let mut level = formats.clone();
        for _ in 0..3 {
            let mut next = Vec::new();
            for f in &level {
                for t in toks {
                    let mut g = f.clone();
                    g.extend_from_slice(t);
                    next.push(g);
                }
            }
            formats.extend(next.iter().cloned());
            level = next;
        }
        for fmt in &formats {
            let plan = simple(fmt);
            let s = plan.is_some();
            let outside = [&b"%12345d"[..], b"%.2d", b"%*5d", b"%.*s", b"%ls", b"%Lf"].iter().any(|t| fmt.windows(t.len()).any(|w| w == *t));
            assert_eq!(s, !outside, "{}", std::string::String::from_utf8_lossy(fmt));
            if !s {
                continue;
            }
            for (k, &v0) in values.iter().enumerate() {
                let mut parsed = Parsed::new();
                parse_into(fmt, &mut Plain, &mut parsed).unwrap();
                // The arguments: integers from the table; for a %s, an address 0..=3.
                let mut seq: Vec<u64> = Vec::new();
                let mut j = k;
                for p in parsed.pieces.iter() {
                    if let Piece::Conv(c) = p {
                        j += 1;
                        if matches!(c.width, Count::Arg(_)) {
                            seq.push(widths[j % widths.len()]);
                        }
                        seq.push(if c.letter == u32::from(b's') { v0.wrapping_add(u64::try_from(j).unwrap()) % 4 } else { values[j % values.len()] });
                    }
                }
                let args: Vec<Arg> = seq.iter().map(|&v| Arg::Int(v)).collect();
                let mut general = Vec::new();
                let n1 = render(fmt, &parsed, &args, &mut TestEnv, &mut general).unwrap();
                let mut quick = Vec::new();
                let mut it = seq.iter().copied();
                // The instance the boundary picks for this plan.
                let plan = plan.as_ref().unwrap();
                let n2 = if plan.has_star() {
                    render_simple::<false, true, _>(fmt, plan, &mut || it.next().unwrap(), &mut || unreachable!(), &mut TestEnv, &mut quick)
                } else {
                    render_simple::<false, false, _>(fmt, plan, &mut || it.next().unwrap(), &mut || unreachable!(), &mut TestEnv, &mut quick)
                }
                .unwrap();
                assert_eq!((n2, &quick), (n1, &general), "{} with {seq:x?}", std::string::String::from_utf8_lossy(fmt));
                checked += 1;
            }
        }
        assert!(checked > 100_000, "{checked}");
    }

    /// Floating conversions of the simple subset render alike through both paths: every letter,
    /// flag, width and precision, alone and among integers and strings, for values of every
    /// class (zero, negative zero, subnormal, huge, ties, carries, infinities, NaNs), and
    /// formats the subset must refuse stay refused.
    #[test]
    fn simple_floating_formats_render_as_the_general_path_does() {
        let values: [f64; 14] = [0.0, -0.0, 1.0, -2.5, 3.14159e-7, 0.5, 9.9999995, 123456.789, 1e300, 5e-324, 999999.5, f64::INFINITY, f64::NEG_INFINITY, f64::NAN];
        let mut fmts: Vec<Vec<u8>> = Vec::new();
        for flags in ["", "-", "0", "+", " ", "#", "+0", "-+", "#0", "- #", "+ "] {
            for width in ["", "1", "12", "30"] {
                for prec in ["", ".", ".0", ".1", ".6", ".17", ".40", ".99"] {
                    for letter in ["e", "E", "f", "F", "g", "G"] {
                        fmts.push(std::format!("%{flags}{width}{prec}{letter}").into_bytes());
                    }
                }
            }
        }
        for f in ["a%e|%d|%.3f", "%s %g %x %5.1f", "%d%d%d%d%d%d%e%f %.2e", "%%%e%%", "%c%G"] {
            fmts.push(f.as_bytes().to_vec());
        }
        // `%n` never: the fortified rule on where the format lies is checked on the general path only.
        let refused: [&[u8]; 15] = [b"%.100f", b"%+d", b"% d", b"%#x", b"%.3d", b"%Lf", b"%lf", b"%a", b"%*.100f", b"%.*d", b"%*.*s", b"%n", b"x%hhn", b"%d%ln", b"%1$d"];
        // A conversion longer than a plan entry records.
        assert!(simple(&[&b"%"[..], &[b'-'; 300], b"d"].concat()).is_none());
        for f in refused {
            assert!(simple(f).is_none(), "{}", std::string::String::from_utf8_lossy(f));
        }
        let mut checked = 0usize;
        for fmt in &fmts {
            let plan = simple(fmt).unwrap();
            assert!(plan.has_float());
            let mut parsed = Parsed::new();
            parse_into(fmt, &mut Plain, &mut parsed).unwrap();
            for k in 0..values.len() {
                let mut args: Vec<Arg> = Vec::new();
                let mut seq: Vec<(bool, u64)> = Vec::new();
                for (j, p) in parsed.pieces.iter().filter(|p| matches!(p, Piece::Conv(_))).enumerate() {
                    let Piece::Conv(c) = p else { continue };
                    let letter = u8::try_from(c.letter).unwrap();
                    if matches!(letter, b'e' | b'E' | b'f' | b'F' | b'g' | b'G') {
                        let bits = values[(k + j) % values.len()].to_bits();
                        args.push(Arg::Double(bits));
                        seq.push((true, bits));
                    } else {
                        let v = if letter == b's' { 2 } else { 77 + u64::try_from(j).unwrap() };
                        args.push(Arg::Int(v));
                        seq.push((false, v));
                    }
                }
                let mut general = Vec::new();
                let n1 = render(fmt, &parsed, &args, &mut TestEnv, &mut general).unwrap();
                let mut quick = Vec::new();
                let it = core::cell::RefCell::new(seq.iter().copied());
                let take = |want_double: bool| {
                    let (d, v) = it.borrow_mut().next().unwrap();
                    assert_eq!(d, want_double);
                    v
                };
                let n2 = render_simple::<true, true, _>(fmt, &plan, &mut || take(false), &mut || take(true), &mut TestEnv, &mut quick).unwrap();
                assert_eq!((n2, &quick), (n1, &general), "{}", std::string::String::from_utf8_lossy(fmt));
                checked += 1;
            }
        }
        assert!(checked > 25_000, "{checked}");
    }

    /// Floating conversions with a `*` width and/or precision (gawk's `"%*.*f"`) take the simple
    /// route and render as the general path does: negative widths left-justify, negative
    /// precisions count as none, garbage above the `int` is ignored.
    #[test]
    fn simple_star_floats_render_as_the_general_path_does() {
        let fmts: [&[u8]; 12] = [b"%*.*f", b"%.*f", b"%*f", b"%*.3f", b"%8.*e", b"%-*.*g", b"%0*.*f", b"%+*.*E", b"% #*.*G", b"x%d %*.*f|%s", b"%*.*f%*.*f", b"%*d %.*f"];
        let ints: [u64; 7] = [0, 3, 12, 0xFFFF_FFFF, 0xFFFF_FFF6, 0x1_0000_0002, 30];
        let values: [f64; 7] = [0.0, -2.5, 10.0 / 7.0, 123456.789, 1e300, f64::NAN, f64::NEG_INFINITY];
        let mut checked = 0usize;
        for fmt in fmts {
            let plan = simple(fmt).unwrap_or_else(|| panic!("{}", std::string::String::from_utf8_lossy(fmt)));
            assert!(plan.has_star() && plan.has_float());
            let mut parsed = Parsed::new();
            parse_into(fmt, &mut Plain, &mut parsed).unwrap();
            for k in 0..ints.len() * values.len() {
                let mut args: Vec<Arg> = Vec::new();
                let mut seq: Vec<(bool, u64)> = Vec::new();
                let mut n = k;
                for p in parsed.pieces.iter() {
                    let Piece::Conv(c) = p else { continue };
                    let mut int = |args: &mut Vec<Arg>, seq: &mut Vec<(bool, u64)>| {
                        let v = ints[n % ints.len()];
                        n += 1;
                        args.push(Arg::Int(v));
                        seq.push((false, v));
                    };
                    if matches!(c.width, Count::Arg(_)) {
                        int(&mut args, &mut seq);
                    }
                    if matches!(c.prec, Count::Arg(_)) {
                        int(&mut args, &mut seq);
                    }
                    let letter = u8::try_from(c.letter).unwrap();
                    if matches!(letter, b'e' | b'E' | b'f' | b'F' | b'g' | b'G') {
                        let bits = values[(k / ints.len() + n) % values.len()].to_bits();
                        args.push(Arg::Double(bits));
                        seq.push((true, bits));
                    } else {
                        let v = if letter == b's' { 2 } else { 77 };
                        args.push(Arg::Int(v));
                        seq.push((false, v));
                    }
                }
                let mut general = Vec::new();
                let n1 = render(fmt, &parsed, &args, &mut TestEnv, &mut general).unwrap();
                let mut quick = Vec::new();
                let it = core::cell::RefCell::new(seq.iter().copied());
                let take = |want_double: bool| {
                    let (d, v) = it.borrow_mut().next().unwrap();
                    assert_eq!(d, want_double);
                    v
                };
                let n2 = render_simple::<true, true, _>(fmt, &plan, &mut || take(false), &mut || take(true), &mut TestEnv, &mut quick).unwrap();
                assert_eq!((n2, &quick), (n1, &general), "{} with {seq:x?}", std::string::String::from_utf8_lossy(fmt));
                assert!(it.borrow_mut().next().is_none());
                checked += 1;
            }
        }
        assert_eq!(checked, 12 * 49);
    }

    /// A format with many conversions renders alike too.
    #[test]
    fn long_simple_formats_render_as_the_general_path_does() {
        let fmt: &[u8] = b"a%d %s|%05x%%%-3c %lu %hhd %2s %X %i %u end";
        let plan = simple(fmt).unwrap();
        assert_eq!(usize::from(plan.n), SIMPLE_MAX);
        let seq: [u64; 10] = [7, 2, 255, 65, u64::MAX, 0x1ff, 3, 0xabc, 0xFFFF_FFFF_FFFF_FFFB, 42];
        let mut parsed = Parsed::new();
        parse_into(fmt, &mut Plain, &mut parsed).unwrap();
        let args: Vec<Arg> = seq.iter().map(|&v| Arg::Int(v)).collect();
        let mut general = Vec::new();
        let n1 = render(fmt, &parsed, &args, &mut TestEnv, &mut general).unwrap();
        let mut quick = Vec::new();
        let mut it = seq.iter().copied();
        let n2 = render_simple::<false, false, _>(fmt, &plan, &mut || it.next().unwrap(), &mut || unreachable!(), &mut TestEnv, &mut quick).unwrap();
        assert_eq!((n2, &quick), (n1, &general), "{}", std::string::String::from_utf8_lossy(&general));
    }

    /// Literal runs around the longest one a plan entry stores (254 bytes; 255 and more are
    /// searched for again) render alike, before kept conversions and past a full plan.
    #[test]
    fn long_literal_runs_render_as_the_general_path_does() {
        for run in [0usize, 1, 253, 254, 255, 256, 300] {
            for convs in [1usize, SIMPLE_MAX, SIMPLE_MAX + 2] {
                let mut fmt = Vec::new();
                for k in 0..convs {
                    fmt.extend(core::iter::repeat_n(b'a' + u8::try_from(k % 26).unwrap(), run));
                    fmt.extend_from_slice(if k % 2 == 0 { b"%d" } else { b"%-3x" });
                }
                fmt.extend_from_slice(b"<tail>");
                let plan = simple(&fmt).unwrap();
                let seq: Vec<u64> = (0..convs).map(|k| u64::try_from(k).unwrap() * 37).collect();
                let mut parsed = Parsed::new();
                parse_into(&fmt, &mut Plain, &mut parsed).unwrap();
                let args: Vec<Arg> = seq.iter().map(|&v| Arg::Int(v)).collect();
                let mut general = Vec::new();
                let n1 = render(&fmt, &parsed, &args, &mut TestEnv, &mut general).unwrap();
                let mut quick = Vec::new();
                let mut it = seq.iter().copied();
                let n2 = render_simple::<false, false, _>(&fmt, &plan, &mut || it.next().unwrap(), &mut || unreachable!(), &mut TestEnv, &mut quick).unwrap();
                assert_eq!((n2, &quick), (n1, &general), "run {run}, {convs} conversions");
            }
        }
    }

    /// What [`PlanCache::fill`] says about `fmt` against a fresh [`simple`] parse of the same bytes.
    fn check_cached(cache: &mut PlanCache, fmt: &[u8]) {
        // The reference is `fill` on a copy; the cache itself goes on through `route`, which must
        // answer the same and lend the same plan (TAR-TV).
        let mut p = SimpleFormat::new();
        let mut copy = *cache;
        let r = copy.fill(fmt, &mut p);
        let (r2, kept) = cache.route(fmt);
        let name = std::string::String::from_utf8_lossy(fmt).into_owned();
        assert_eq!(r2, r, "route {name}");
        assert_eq!(kept.map(|k| std::format!("{k:?}")), (r == Planned::Simple).then(|| std::format!("{p:?}")), "route {name}");
        if fmt.is_empty() || fmt.len() > PLAN_CACHE_FMT {
            assert_eq!(r, Planned::Unknown, "{name}");
            return;
        }
        match simple(fmt) {
            Some(q) => {
                assert_eq!(r, Planned::Simple, "{name}");
                assert_eq!(std::format!("{p:?}"), std::format!("{q:?}"), "{name}");
            }
            None => assert_eq!(r, Planned::General, "{name}"),
        }
    }

    /// The plan cache (D-12.21) answers exactly what a fresh parse of the same bytes does, for
    /// misses and hits, formats that differ only in their bytes (same length), formats rewritten
    /// in place, and the length limit; three formats in turn evict each other.
    #[test]
    fn plan_cache_answers_as_a_fresh_parse_of_the_same_bytes() {
        let fmts: [&[u8]; 14] = [
            b"%s %s/%s %*s %-*s",
            b" %s",
            b"%5d",
            b"%5s",
            b"%.5",
            b"%-*d|%*s",
            b"%ls",
            b"%s %d %x\n",
            b"%f %10.3e",
            b"",
            b"%%",
            b"0123456789012345678901234567890123456%d",
            b"0123456789012345678901234567890123456%dx",
            b"%d %d %d %d %d %d %d %d %d %d %s",
        ];
        let mut cache = PlanCache::new();
        for _ in 0..3 {
            for f in fmts {
                check_cached(&mut cache, f);
                check_cached(&mut cache, f);
            }
            // Three alternating formats, then pairs in both orders.
            for (a, b) in [(0, 1), (2, 3), (3, 2), (4, 6), (1, 0)] {
                for _ in 0..3 {
                    check_cached(&mut cache, fmts[a]);
                    check_cached(&mut cache, fmts[b]);
                    check_cached(&mut cache, fmts[(a + b + 1) % fmts.len()]);
                }
            }
        }
        // One buffer rewritten between calls (a caller building its formats in place).
        let mut buf = *b"%-5d %s";
        check_cached(&mut cache, &buf);
        buf[1] = b'.';
        check_cached(&mut cache, &buf);
        buf[1] = b'*';
        check_cached(&mut cache, &buf);
        buf[6] = b'c';
        check_cached(&mut cache, &buf);
        buf[1] = b'-';
        check_cached(&mut cache, &buf);
    }

    /// `lone_s` (TAR-TV): exactly the plans of text around one plain `%s`, and the text before,
    /// the string and the text after are what `render_simple` writes for them.
    /// `lone_int` (PRINTF-LAYERS): exactly the plans of text around one right-justified integer
    /// of at most `INT_FIELD` bytes, and its pieces are what the general path writes.
    #[test]
    fn lone_int_pieces_are_what_the_general_path_writes() {
        let lone: [&[u8]; 12] = [b"%02x", b"%jd", b"%jx", b"%X", b"a%5d\n", b"%032u", b"%32hhd", b"%04X", b"%010ld|", b"%2i", b"%hx", b"%llu"];
        let not: [&[u8]; 9] = [b"%-5d", b"%33d", b"%d %d", b"%*d", b"%s", b"%c", b"%%%d", b"%d%%", b"%f"];
        let words: [u64; 10] = [0, 1, 9, 10, 0xab, 12345, u64::MAX, 1 << 63, 0x8000_0000, 0xffff_ff80];
        for fmt in lone {
            let name = std::string::String::from_utf8_lossy(fmt).into_owned();
            let Some(plan) = simple(fmt) else { continue };
            let li = plan.lone_int().unwrap_or_else(|| panic!("{name}"));
            for w in words {
                let mut general = Vec::new();
                let mut parsed = Parsed::new();
                parse_into(fmt, &mut Plain, &mut parsed).unwrap();
                render(fmt, &parsed, &[Arg::Int(w)], &mut TestEnv, &mut general).unwrap();
                let mut buf = [0u8; INT_FIELD];
                let mut lane = fmt[..li.at].to_vec();
                lane.extend_from_slice(li.field(w, &mut buf));
                lane.extend_from_slice(&fmt[li.after..]);
                assert_eq!(lane, general, "{name} {w:#x}");
            }
        }
        for fmt in not {
            if let Some(plan) = simple(fmt) {
                assert!(plan.lone_int().is_none(), "{}", std::string::String::from_utf8_lossy(fmt));
            }
        }
    }

    #[test]
    fn lone_s_pieces_are_what_render_simple_writes() {
        let lone: [&[u8]; 5] = [b" %s", b"%s", b"%s\n", b"ab%scd", b"0123456789012345678901234567890123456789%s|"];
        let not: [&[u8]; 10] = [b"%5s", b"%-s", b"%-5s", b"%s %s", b" %s%%", b"%%%s", b"%d", b"%c", b"%*s", b"%ls"];
        for fmt in lone {
            let name = std::string::String::from_utf8_lossy(fmt).into_owned();
            let plan = simple(fmt).unwrap();
            let (at, after) = plan.lone_s().unwrap_or_else(|| panic!("{name}"));
            for arg in 1..=TEXTS.len() {
                let mut quick = Vec::new();
                let n = render_simple::<false, false, _>(fmt, &plan, &mut || u64::try_from(arg).unwrap(), &mut || unreachable!(), &mut TestEnv, &mut quick).unwrap();
                let mut lane = fmt[..at].to_vec();
                lane.extend_from_slice(TestEnv.text(arg, None));
                lane.extend_from_slice(&fmt[after..]);
                assert_eq!((n, &quick), (lane.len(), &lane), "{name}");
            }
        }
        for fmt in not {
            if let Some(plan) = simple(fmt) {
                assert_eq!(plan.lone_s(), None, "{}", std::string::String::from_utf8_lossy(fmt));
            }
        }
    }

    #[test]
    fn lone_c_pieces_are_what_render_simple_writes() {
        let lone: [&[u8]; 4] = [b"*.*%c", b"%c", b"%c\n", b"ab%ccd"];
        let not: [&[u8]; 8] = [b"%5c", b"%-c", b"%-5c", b"%c%c", b" %c%%", b"%s", b"%*c", b"%d"];
        for fmt in lone {
            let name = std::string::String::from_utf8_lossy(fmt).into_owned();
            let plan = simple(fmt).unwrap();
            assert!(plan.is_lone(), "{name}");
            let (at, after) = plan.lone_c().unwrap_or_else(|| panic!("{name}"));
            for arg in [u64::from(b'f'), 0x1_0000_0167, u64::from(b'%')] {
                let mut quick = Vec::new();
                let n = render_simple::<false, false, _>(fmt, &plan, &mut || arg, &mut || unreachable!(), &mut TestEnv, &mut quick).unwrap();
                let mut lane = fmt[..at].to_vec();
                lane.push(u8::try_from(arg & 0xFF).unwrap());
                lane.extend_from_slice(&fmt[after..]);
                assert_eq!((n, &quick), (lane.len(), &lane), "{name}");
            }
        }
        for fmt in not {
            if let Some(plan) = simple(fmt) {
                assert_eq!(plan.lone_c(), None, "{}", std::string::String::from_utf8_lossy(fmt));
            }
        }
    }

    /// A `*` width right after the flags plans as the general parse reads it: the letter, the
    /// `-` flag and the `*` width kept; a written width before the `*` is not simple.
    #[test]
    fn star_width_letters_plan_as_the_general_parse() {
        for (fmt, letter, left) in [(&b"%*s"[..], b's', false), (b"%-*s", b's', true), (b"%*d", b'd', false), (b"%*c", b'c', false), (b"%-*x", b'x', true)] {
            let plan = simple(fmt).unwrap();
            let name = std::string::String::from_utf8_lossy(fmt).into_owned();
            assert!(plan.has_star() && !plan.is_lone(), "{name}");
            let (c, len, _) = plan.convs[0];
            assert_eq!((c.letter, c.has(F_LEFT), c.bits, c.prec, usize::from(len)), (letter, left, 32, NO_PREC, fmt.len()), "{name}");
        }
        for fmt in [&b"%5*s"[..], b"%*.2s", b"%#*x", b"%+*d"] {
            assert!(simple(fmt).is_none(), "{}", std::string::String::from_utf8_lossy(fmt));
        }
    }

    /// The cache's byte compare sees a difference at every position of every length it is used
    /// for, and equal bytes as equal.
    #[test]
    fn plan_cache_compare_sees_every_differing_byte() {
        let a: Vec<u8> = (0..PLAN_CACHE_FMT).map(|i| b'a' + u8::try_from(i % 26).unwrap()).collect();
        for n in 1..=PLAN_CACHE_FMT {
            assert!(same_short(&a[..n], &a[..n]), "{n}");
            for p in 0..n {
                let mut b = a[..n].to_vec();
                b[p] ^= 0x20;
                assert!(!same_short(&a[..n], &b), "{n} {p}");
            }
        }
    }

    /// The instance with no floating code refuses a plan that has a floating conversion
    /// rather than formatting it as something else.
    #[test]
    fn integer_only_rendering_refuses_a_floating_conversion() {
        let fmt: &[u8] = b"a%d %5.2f";
        let plan = simple(fmt).unwrap();
        assert!(plan.has_float());
        let mut out = Vec::new();
        let r = render_simple::<false, true, _>(fmt, &plan, &mut || 1, &mut || unreachable!(), &mut TestEnv, &mut out);
        assert_eq!(r.err(), Some(Errno::EINVAL));
    }
}
