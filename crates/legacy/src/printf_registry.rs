//! The printf registry (spec §12.11; D-12.8): conversion letters, modifiers and argument types
//! registered for the whole process, what they mean to a format (`struct printf_info`, the
//! `PA_*` argument types), and the two ready-made formatters, `printf_size` and `__printf_fp`.
//!
//! The registry is process-wide state, as the contract makes it (⚑ HIST): every `printf` in the
//! process sees it. It is kept in atomics; registrations are serialised by one lock and readers
//! take none, so formatting never waits for a registration. Registered letters are 0 to 255,
//! modifiers are at most sixteen (one bit each of `printf_info.user`) of at most
//! [`MODIFIER_MAX`] characters below 256, and registered types are numbered from `PA_LAST` (8)
//! to 255.

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering::{Acquire, Relaxed, Release}};

use liella::error::{Errno, KResult};
use liella::mem::TryVec;
use liella::num::float::{self, Float, Format, Rounding, Value};
use liella::num::format::{Case, Spec, Style};
use liella::printf::{self as pf, Call, Class, Conv, Count, Flags, Len, Unit};
use liella_abi::layout::struct_printf_info;

use crate::lock::SpinLock;

// ------------------------------------------------------------------------------ the PA_* types

/// `int`.
pub const PA_INT: i32 = 0;
/// `int`, used as a `char`.
pub const PA_CHAR: i32 = 1;
/// `wint_t`.
pub const PA_WCHAR: i32 = 2;
/// `const char *`.
pub const PA_STRING: i32 = 3;
/// `const wchar_t *`.
pub const PA_WSTRING: i32 = 4;
/// `void *`.
pub const PA_POINTER: i32 = 5;
/// `float` (passed as a `double`).
pub const PA_FLOAT: i32 = 6;
/// `double`.
pub const PA_DOUBLE: i32 = 7;
/// The first number `register_printf_type` gives.
pub const PA_LAST: i32 = 8;
/// `long long` (with `PA_INT`), `long double` (with `PA_DOUBLE`).
pub const PA_FLAG_LONG_LONG: i32 = 1 << 8;
/// `long`.
pub const PA_FLAG_LONG: i32 = 1 << 9;
/// `short`.
pub const PA_FLAG_SHORT: i32 = 1 << 10;
/// A pointer to the type.
pub const PA_FLAG_PTR: i32 = 1 << 11;

// ------------------------------------------------------------------------------ the registry

/// Letters a handler can be registered for.
const LETTERS: usize = 256;
/// Modifiers: one bit each of `printf_info.user`.
const MODIFIERS: usize = 16;
/// The longest modifier, in characters.
pub const MODIFIER_MAX: usize = 64;
/// Registered types: `PA_LAST` to 255.
const TYPES: usize = 248;

/// A letter's handler and argument function (addresses; 0 for none).
struct Letter {
    handler: AtomicUsize,
    arginfo: AtomicUsize,
}

/// A modifier's characters (`len` 0 until registered).
struct Modifier {
    len: AtomicUsize,
    text: [AtomicU8; MODIFIER_MAX],
}

static LETTER: [Letter; LETTERS] = [const { Letter { handler: AtomicUsize::new(0), arginfo: AtomicUsize::new(0) } }; LETTERS];
static MODIFIER: [Modifier; MODIFIERS] = [const { Modifier { len: AtomicUsize::new(0), text: [const { AtomicU8::new(0) }; MODIFIER_MAX] } }; MODIFIERS];
/// Modifiers registered (published after their text).
static MODIFIER_COUNT: AtomicUsize = AtomicUsize::new(0);
/// The functions of the registered types, by number less `PA_LAST`.
static TYPE: [AtomicUsize; TYPES] = [const { AtomicUsize::new(0) }; TYPES];
/// Types registered.
static TYPE_COUNT: AtomicUsize = AtomicUsize::new(0);
/// A handler or a modifier was ever registered: formats need the registry.
static ACTIVE: AtomicBool = AtomicBool::new(false);
/// Serialises registrations.
static LOCK: SpinLock = SpinLock::new();

/// `register_printf_specifier(spec, handler, arginfo)` (and `register_printf_function`): a
/// letter outside 0..=255 is `EINVAL`. A NULL handler removes the registration.
pub fn register_specifier(spec: i32, handler: usize, arginfo: usize) -> KResult<()> {
    let slot = usize::try_from(spec).ok().and_then(|s| LETTER.get(s)).ok_or(Errno::EINVAL)?;
    let _g = LOCK.lock();
    slot.arginfo.store(arginfo, Release);
    slot.handler.store(handler, Release);
    if handler != 0 {
        ACTIVE.store(true, Release);
    }
    Ok(())
}

/// `register_printf_modifier(text)` (`text` the wide characters before the terminator): the
/// modifier's bit. Empty, a character above 255 or longer than [`MODIFIER_MAX`] is `EINVAL`;
/// a seventeenth modifier is `ENOSPC`. The same text registered again gets a new bit, which
/// then wins (the longest, latest match is taken).
pub fn register_modifier(text: &[i32]) -> KResult<u16> {
    if text.is_empty() || text.len() > MODIFIER_MAX || text.iter().any(|&c| !(0..=255).contains(&c)) {
        return Err(Errno::EINVAL);
    }
    let _g = LOCK.lock();
    let n = MODIFIER_COUNT.load(Relaxed);
    let slot = MODIFIER.get(n).ok_or(Errno::ENOSPC)?;
    for (cell, &c) in slot.text.iter().zip(text) {
        cell.store(u8::try_from(c).unwrap_or(0), Relaxed);
    }
    slot.len.store(text.len(), Release);
    MODIFIER_COUNT.store(n.saturating_add(1), Release);
    ACTIVE.store(true, Release);
    Ok(1u16.checked_shl(u32::try_from(n).unwrap_or(0)).unwrap_or(0))
}

/// `register_printf_type(fn)`: the new type's number, from `PA_LAST`; `ENOSPC` past 255.
pub fn register_type(f: usize) -> KResult<i32> {
    let _g = LOCK.lock();
    let n = TYPE_COUNT.load(Relaxed);
    TYPE.get(n).ok_or(Errno::ENOSPC)?.store(f, Release);
    TYPE_COUNT.store(n.saturating_add(1), Release);
    i32::try_from(n).ok().and_then(|k| k.checked_add(PA_LAST)).ok_or(Errno::ENOSPC)
}

/// Whether anything was registered that formats must consult.
pub fn active() -> bool {
    ACTIVE.load(Acquire)
}

/// The handler and argument function registered for `letter`, if a handler is.
pub fn handler(letter: u32) -> Option<(usize, usize)> {
    let slot = usize::try_from(letter).ok().and_then(|l| LETTER.get(l))?;
    let h = slot.handler.load(Acquire);
    (h != 0).then(|| (h, slot.arginfo.load(Acquire)))
}

/// The longest registered modifier at the start of `at(0)`, `at(1)`, ...: its length and bit
/// (of equal ones, the latest registered).
pub fn modifier(at: &dyn Fn(usize) -> u32) -> Option<(usize, u16)> {
    let count = MODIFIER_COUNT.load(Acquire);
    let mut best: Option<(usize, u16)> = None;
    for (i, m) in MODIFIER.iter().enumerate().take(count) {
        let len = m.len.load(Acquire);
        let matches = m.text.iter().take(len).enumerate().all(|(k, c)| at(k) == u32::from(c.load(Relaxed)));
        if len > 0 && matches && best.is_none_or(|(l, _)| len >= l) {
            best = Some((len, 1u16.checked_shl(u32::try_from(i).unwrap_or(0)).unwrap_or(0)));
        }
    }
    best
}

/// The function that reads registered type `ty` from a `va_list` (0 if none is registered).
pub fn type_function(ty: u32) -> usize {
    let k = usize::try_from(ty).ok().and_then(|t| t.checked_sub(8)).filter(|&k| k < TYPE_COUNT.load(Acquire));
    k.and_then(|k| TYPE.get(k)).map_or(0, |f| f.load(Acquire))
}

/// How an argument of type `code` (with the object size an argument function gave) is read.
pub fn class_of(code: i32, size: i32) -> Class {
    if code < 0 || code & PA_FLAG_PTR != 0 {
        return Class::Int;
    }
    match code & 0xFF {
        PA_FLOAT => Class::Double,
        PA_DOUBLE if code & PA_FLAG_LONG_LONG != 0 => Class::LongDouble,
        PA_DOUBLE => Class::Double,
        t if t >= PA_LAST => Class::User { ty: u32::try_from(t).unwrap_or(0), size: usize::try_from(size).unwrap_or(0) },
        _ => Class::Int,
    }
}

/// The type `parse_printf_format` reports for a built-in conversion's argument (`None` for
/// `%m`). Integer conversions follow the length's `printf_info` bits: `hh` (and `w8`, `wf8`)
/// `PA_CHAR`, `h` (`w16`) `PA_INT|PA_FLAG_SHORT`, any with `is_long` (`l`, `ll`, `j`, `z`, `t`,
/// `w64`, `wf16` and up) `PA_INT|PA_FLAG_LONG`, others (`L`, `q`) `PA_INT`; `%c`, `%s` are
/// `PA_CHAR`, `PA_STRING` whatever the length.
pub fn builtin_type(c: &Conv) -> Option<i32> {
    let bits = len_bits(c.len);
    match u8::try_from(c.letter).unwrap_or(0) {
        b'd' | b'i' | b'o' | b'u' | b'x' | b'X' | b'b' | b'B' => Some(if bits & IS_CHAR != 0 {
            PA_CHAR
        } else if bits & SHORT != 0 {
            PA_INT | PA_FLAG_SHORT
        } else if bits & LONG != 0 {
            PA_INT | PA_FLAG_LONG
        } else {
            PA_INT
        }),
        b'f' | b'F' | b'e' | b'E' | b'g' | b'G' | b'a' | b'A' => Some(if bits & LONG_DOUBLE != 0 { PA_DOUBLE | PA_FLAG_LONG_LONG } else { PA_DOUBLE }),
        b'c' => Some(PA_CHAR),
        b'C' => Some(PA_WCHAR),
        b's' => Some(PA_STRING),
        b'S' => Some(PA_WSTRING),
        b'p' => Some(PA_POINTER),
        b'n' => Some(PA_INT | PA_FLAG_PTR),
        _ => None,
    }
}

// ------------------------------------------------------------------------------ printf_info

/// `is_long_double` (`L`, `q`, `ll`).
pub const LONG_DOUBLE: u16 = 1 << 0;
/// `is_short` (`h`).
pub const SHORT: u16 = 1 << 1;
/// `is_long` (`l`, `ll`, `j`, `z`, `t`).
pub const LONG: u16 = 1 << 2;
/// `alt` (`#`).
pub const ALT: u16 = 1 << 3;
/// `space`.
pub const SPACE: u16 = 1 << 4;
/// `left` (`-`).
pub const LEFT: u16 = 1 << 5;
/// `showsign` (`+`).
pub const SHOWSIGN: u16 = 1 << 6;
/// `group` (`'`).
pub const GROUP: u16 = 1 << 7;
/// `extra`.
pub const EXTRA: u16 = 1 << 8;
/// `is_char` (`hh`).
pub const IS_CHAR: u16 = 1 << 9;
/// `wide`: the destination is wide-oriented.
pub const WIDE: u16 = 1 << 10;
/// `i18n` (`I`).
pub const I18N: u16 = 1 << 11;

/// The length bits of `struct printf_info` for a length modifier (observed: `ll`, `w64` and
/// `wf16` and up set both `is_long_double` and `is_long`; `q` is `L`).
fn len_bits(len: Len) -> u16 {
    match len {
        Len::Char | Len::Bits(8) | Len::Fast(8) => IS_CHAR,
        Len::Short | Len::Bits(16) => SHORT,
        Len::Long | Len::Max | Len::Size | Len::Ptrdiff => LONG,
        Len::LongLong | Len::Bits(64) | Len::Fast(16 | 32 | 64) => LONG_DOUBLE | LONG,
        Len::LongDouble => LONG_DOUBLE,
        Len::None | Len::Bits(_) | Len::Fast(_) => 0,
    }
}

/// `struct printf_info`: what a handler, an argument function and the two formatters receive.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Info {
    /// The precision, -1 for none.
    pub prec: i32,
    /// The field width.
    pub width: i32,
    /// The conversion letter.
    pub spec: u32,
    /// The flag bits ([`LONG_DOUBLE`] ... [`I18N`]).
    pub bits: u16,
    /// The registered modifier's bit.
    pub user: u16,
    /// The padding character: `0` for the `0` flag (not left-justified), else a space.
    pub pad: u32,
}

impl Info {
    /// The record for `conv` with the given width, precision and justification.
    fn of(conv: &Conv, width: usize, prec: Option<usize>, left: bool, wide: bool) -> Info {
        let f = conv.flags;
        let mut bits = len_bits(conv.len);
        for (on, bit) in [(f.alt, ALT), (f.space, SPACE), (left, LEFT), (f.plus, SHOWSIGN), (f.group, GROUP), (wide, WIDE), (f.i18n, I18N)] {
            if on {
                bits |= bit;
            }
        }
        Info {
            prec: prec.map_or(-1, |p| i32::try_from(p).unwrap_or(i32::MAX)),
            width: i32::try_from(width).unwrap_or(i32::MAX),
            spec: conv.letter,
            bits,
            user: conv.user,
            pad: u32::from(if f.zero && !left { b'0' } else { b' ' }),
        }
    }

    /// The record an argument function receives while the format is parsed: a `*` width is
    /// 0 and a `*` precision -1, as their arguments are not read yet.
    pub fn parsed(conv: &Conv, wide: bool) -> Info {
        let fixed = |c: Count| if let Count::Fixed(n) = c { Some(n) } else { None };
        Info::of(conv, fixed(conv.width).unwrap_or(0), fixed(conv.prec), conv.flags.left, wide)
    }

    /// The record a handler receives.
    pub fn call(call: &Call<'_>) -> Info {
        Info::of(call.conv, call.width, call.prec, call.left, call.wide)
    }

    /// Whether flag `bit` is set.
    pub fn has(&self, bit: u16) -> bool {
        self.bits & bit != 0
    }

    /// From the C layout.
    pub fn from_c(c: &struct_printf_info) -> Info {
        let [lo, hi] = c.is_long_double;
        Info {
            prec: c.prec,
            width: c.width,
            spec: u32::from_ne_bytes(c.spec.to_ne_bytes()),
            bits: u16::from_le_bytes([lo, hi]),
            user: c.user,
            pad: u32::from_ne_bytes(c.pad.to_ne_bytes()),
        }
    }

    /// In the C layout.
    pub fn to_c(&self) -> struct_printf_info {
        struct_printf_info {
            prec: self.prec,
            width: self.width,
            spec: i32::from_ne_bytes(self.spec.to_ne_bytes()),
            is_long_double: self.bits.to_le_bytes(),
            user: self.user,
            pad: i32::from_ne_bytes(self.pad.to_ne_bytes()),
        }
    }

    /// The sign, grouping and digit flags, as the floating formatter takes them.
    fn flags(&self) -> Flags {
        Flags { plus: self.has(SHOWSIGN), space: self.has(SPACE), alt: self.has(ALT), group: self.has(GROUP), i18n: self.has(I18N), ..Flags::default() }
    }
}

// ------------------------------------------------------------------------------ formatters

/// `printf_size_info`: the argument type for `printf_size`.
pub fn printf_size_type(info: &Info) -> i32 {
    if info.has(LONG_DOUBLE) { PA_DOUBLE | PA_FLAG_LONG_LONG } else { PA_DOUBLE }
}

/// Whether the positive finite `x` is at least `d`.
fn at_least(x: Float, d: u64) -> bool {
    match x.value {
        Value::Finite { mant, exp } if !x.neg => match u32::try_from(exp) {
            // `mant` has at most 64 bits, so a shift below 64 fits.
            Ok(e) => e >= 64 || mant.checked_shl(e).is_none_or(|v| v >= u128::from(d)),
            Err(_) => mant.checked_shr(exp.unsigned_abs()).unwrap_or(0) >= u128::from(d),
        },
        _ => false,
    }
}

/// `x / d`, correctly rounded to `kind` in `mode` (as the floating division of that format).
fn divide(x: Float, d: u64, kind: Format, mode: Rounding) -> Float {
    match x.value {
        Value::Finite { mant, exp } if d != 0 => {
            // The significand at the top of 128 bits keeps more than enough quotient bits.
            let shift = mant.leading_zeros();
            let top = mant.checked_shl(shift).unwrap_or(0);
            let (q, r) = (top.checked_div(u128::from(d)).unwrap_or(0), top.checked_rem(u128::from(d)).unwrap_or(0));
            let e = exp.saturating_sub(i32::try_from(shift).unwrap_or(0));
            float::round(x.neg, q, e, r != 0, kind, mode).value
        }
        _ => x,
    }
}

/// Pad `body` to the width in `info`: after it when left-justified, else before it; a `0`
/// padding character goes after the sign (`prefix` units) of a finite value, and is a space
/// for an infinity or a NaN. Any other padding character is used as it is.
fn pad<U: Unit>(body: TryVec<U>, prefix: usize, info: &Info, finite: bool) -> KResult<TryVec<U>> {
    let width = usize::try_from(info.width).unwrap_or(0);
    let n = width.saturating_sub(body.len());
    if n == 0 {
        return Ok(body);
    }
    let zero = info.pad == u32::from(b'0');
    let fill = if zero && !finite { U::ascii(b' ') } else { U::from_raw(info.pad) };
    let mut out: TryVec<U> = TryVec::with_capacity(width)?;
    let split = if info.has(LEFT) {
        body.len()
    } else if zero && finite {
        prefix.min(body.len())
    } else {
        0
    };
    out.extend_from_slice(body.get(..split).unwrap_or(&[]))?;
    for _ in 0..n {
        out.push(fill)?;
    }
    out.extend_from_slice(body.get(split..).unwrap_or(&[]))?;
    Ok(out)
}

/// `printf_size`: `x` (of `kind`) with a unit prefix, as the destination's units. A lowercase
/// letter divides by 1024 with the units ` kmgtpezy`, an uppercase one by 1000 with
/// ` KMGTPEZY` (the space: no unit), while the value is at least the divisor, up to `y`/`Y`;
/// negative values are not divided. The number has the precision (3 by default) and the
/// sign, `#`, `'` and `I` flags, as `%f`; the width counts the unit too. An infinity or a NaN
/// is `inf`/`nan` without a unit, signed by the flags (a NaN never shows `-`).
pub fn printf_size<U: Unit>(env: &dyn pf::Env, info: &Info, x: Float, kind: Format) -> KResult<TryVec<U>> {
    let upper = u8::try_from(info.spec).is_ok_and(|b| b.is_ascii_uppercase());
    let (divisor, units): (u64, &[u8; 9]) = if upper { (1000, b" KMGTPEZY") } else { (1024, b" kmgtpezy") };
    let mut body: TryVec<U> = TryVec::new();
    match x.value {
        Value::Inf | Value::Nan { .. } => {
            let nan = matches!(x.value, Value::Nan { .. });
            let mut t: TryVec<u8> = TryVec::new();
            if x.neg && !nan {
                t.push(b'-')?;
            } else if info.has(SHOWSIGN) {
                t.push(b'+')?;
            } else if info.has(SPACE) {
                t.push(b' ')?;
            }
            t.extend_from_slice(if nan { b"nan" } else { b"inf" })?;
            push_text(env, &t, &mut body)?;
            pad(body, 0, info, false)
        }
        _ => {
            let mut v = x;
            let mut unit = 0usize;
            while unit < 8 && at_least(v, divisor) {
                v = divide(v, divisor, kind, env.rounding());
                unit = unit.saturating_add(1);
            }
            let precision = Some(usize::try_from(info.prec).unwrap_or(3));
            let t = pf::float_text(env, v, kind, Spec { style: Style::Fixed, case: Case::Lower, precision, alternate: info.has(ALT) }, info.flags())?;
            push_text(env, &t.text, &mut body)?;
            body.push(U::ascii(units.get(unit).copied().unwrap_or(b' ')))?;
            pad(body, t.prefix, info, true)
        }
    }
}

/// Append `text` to `body` in the destination's units.
fn push_text<U: Unit>(env: &dyn pf::Env, text: &[u8], body: &mut TryVec<U>) -> KResult<()> {
    let mut field = pf::Field::<U>::new();
    U::from_text(env, text, None, &mut field)?;
    body.extend_from_slice(&field)
}

/// `__printf_fp`: `x` (of `kind`) as the conversion `info.spec`, as the destination's units:
/// `f`/`F` fixed, `e`/`E` exponent, any other letter the shortest form (`%g`), its exponent
/// letter being the one two before it in the character set, as `e` is for `g` (observed: `a`
/// gives `_`); an uppercase letter gives `INF`/`NAN`. A negative precision is the default. The
/// padding is [`pad`]'s.
pub fn printf_fp<U: Unit>(env: &dyn pf::Env, info: &Info, x: Float, kind: Format) -> KResult<TryVec<U>> {
    let letter = u8::try_from(info.spec).unwrap_or(0);
    let style = match letter {
        b'f' | b'F' => Style::Fixed,
        b'e' | b'E' => Style::Exp,
        _ => Style::General,
    };
    let case = if letter.is_ascii_uppercase() { Case::Upper } else { Case::Lower };
    let precision = usize::try_from(info.prec).ok();
    let t = pf::float_text(env, x, kind, Spec { style, case, precision, alternate: info.has(ALT) }, info.flags())?;
    let mut body: TryVec<U> = TryVec::new();
    push_text(env, &t.text, &mut body)?;
    if style == Style::General && t.finite {
        let e = |u: &U| -> bool { matches!(u8::try_from((*u).into()), Ok(b'e' | b'E')) };
        if let Some(slot) = body.iter().rposition(e).and_then(|k| body.as_mut_slice().get_mut(k)) {
            *slot = U::from_raw(info.spec.wrapping_sub(2));
        }
    }
    pad(body, t.prefix, info, t.finite)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn f64(v: f64) -> Float {
        Float::unpack(u128::from(v.to_bits()), Format::F64)
    }

    #[test]
    fn types_and_classes() {
        assert_eq!(class_of(PA_DOUBLE | PA_FLAG_LONG_LONG, 0), Class::LongDouble);
        assert_eq!((class_of(PA_FLOAT, 0), class_of(PA_DOUBLE | PA_FLAG_PTR, 0)), (Class::Double, Class::Int));
        assert_eq!(class_of(9, 8), Class::User { ty: 9, size: 8 });
        assert_eq!((class_of(PA_STRING, 0), class_of(-7, 0)), (Class::Int, Class::Int));
        let conv = |letter: u8, len: Len| Conv { letter: u32::from(letter), len, ..Conv::default() };
        assert_eq!(builtin_type(&conv(b'i', Len::LongLong)), Some(PA_INT | PA_FLAG_LONG));
        assert_eq!(builtin_type(&conv(b'i', Len::LongDouble)), Some(PA_INT));
        assert_eq!(builtin_type(&conv(b'i', Len::Fast(16))), Some(PA_INT | PA_FLAG_LONG));
        assert_eq!(builtin_type(&conv(b'f', Len::LongDouble)), Some(PA_DOUBLE | PA_FLAG_LONG_LONG));
        assert_eq!((builtin_type(&conv(b'c', Len::Long)), builtin_type(&conv(b'm', Len::None))), (Some(PA_CHAR), None));
    }

    #[test]
    fn info_round_trips_through_the_c_layout() {
        let i = Info { prec: 3, width: -9, spec: u32::from(b'Q'), bits: LONG | LEFT | I18N, user: 4, pad: u32::from(b'0') };
        assert_eq!(Info::from_c(&i.to_c()), i);
    }

    #[test]
    fn scaling_divides_like_the_floating_unit() {
        let mut v = f64(1e300);
        for _ in 0..8 {
            v = divide(v, 1000, Format::F64, Rounding::Nearest);
        }
        let mut want = 1e300f64;
        for _ in 0..8 {
            want /= 1000.0;
        }
        assert_eq!(v, f64(want));
        assert!(at_least(f64(1024.0), 1024) && !at_least(f64(1023.75), 1024) && !at_least(f64(-2048.0), 1024));
        assert!(at_least(f64(1e300), 1000) && !at_least(f64(5e-300), 1));
    }
}
