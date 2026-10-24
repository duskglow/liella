//! Formatted input (spec §12.7; D-12.6, D-12.13): the `scanf` format language over byte input
//! (`scanf`, `fscanf`, `sscanf`) or wide input (`wscanf`, `fwscanf`, `swscanf`). Design record:
//! D-12.24.
//!
//! [`scan`] walks the format one directive at a time (white space, an ordinary unit, or a
//! conversion specification decoded on the spot into a `Spec`; no pre-pass, no allocation). Each
//! conversion is a small state machine over the [`Input`], which yields one unit at a time with
//! one unit of pushback: a field ends at the first unit that cannot continue it, and that unit is
//! given back. A field that ends while it is only the start of a number (`1e`, `0x` as a floating
//! number, `-`) is consumed and the conversion fails (spec §12.7, D-12.13). Values go to a
//! [`Sink`], which owns the caller's pointer arguments: the engine names an argument by its index,
//! so positional (`%2$d`) and sequential conversions are one model.
//!
//! Integer fields accumulate a saturating 64-bit magnitude, then fit it to the destination's
//! width: signed values out of range store the nearest bound, unsigned ones clamp the magnitude
//! and negate modulo 2^N (D-12.6). Floating fields are checked and normalised to ASCII here and
//! converted by the exact float engine (D-8.5), correctly rounded in the caller's rounding mode.
//! A `%[` set is compiled to a 256-bit map of the units below 256; a wide set naming higher units
//! is also tested against its text in the format. Text for a caller's buffer is stored in chunks
//! as it is converted, so a long field needs no buffer of its own size; `%m` text is collected.

use crate::ctype::{self, Class};
use crate::error::{Errno, Result};
use crate::locale::file::Cat;
use crate::locale::{Item, Locale};
use crate::mem::{InlineVec, TryVec};
use crate::num::float::{Format, Rounding};
use crate::num::parse::{parse_float, Dialect};
use crate::text::{self, Decoded, Encoding, MbState};

/// The symbol set a call came through (spec §12.7, ⚑ COMPAT).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Set {
    /// The plain names: `%a` before `s`, `S` or `[` allocates (the pre-C99 GNU meaning).
    #[default]
    Gnu,
    /// `__isoc99_*`: `%a` is a floating conversion.
    C99,
    /// `__isoc23_*`: also `%i` accepts a `0b` prefix.
    C23,
}

/// A unit of a format: a byte (the narrow functions) or a `wchar_t` (the wide ones). The input
/// has the same kind of unit as the format.
pub trait FmtUnit: Copy {
    /// True for `wchar_t` formats (and wide input).
    const WIDE: bool;
    /// The unit's value: a byte, or a code point.
    fn code(self) -> u32;
}

impl FmtUnit for u8 {
    const WIDE: bool = false;
    #[inline]
    fn code(self) -> u32 {
        u32::from(self)
    }
}

impl FmtUnit for i32 {
    const WIDE: bool = true;
    #[inline]
    fn code(self) -> u32 {
        // The bits as they are: a negative `wchar_t` is no character and matches nothing.
        u32::from_ne_bytes(self.to_ne_bytes())
    }
}

/// The text being scanned: a string, or a stream whose lock the caller holds.
pub trait Input {
    /// The next unit; `Ok(None)` at end of input; `Err` on a read or encoding error, which is
    /// an input failure like the end of input.
    fn next(&mut self) -> Result<Option<u32>>;
    /// Give back `unit`, the unit just read (one unit of pushback).
    fn back(&mut self, unit: u32);
}

/// The caller's objects, named by the pointer arguments.
pub trait Sink {
    /// The address held by pointer argument `n` (1-based); 0 when there is none.
    fn pointer(&mut self, n: usize) -> usize;
    /// Store the low `size` bytes of `value` (an integer of that size) at `addr`.
    fn int(&mut self, addr: usize, size: usize, value: u64);
    /// Store `data` at `addr` (text, or the bytes of a floating value).
    fn bytes(&mut self, addr: usize, data: &[u8]);
    /// Store wide characters (`wchar_t`) at `addr`.
    fn wide(&mut self, addr: usize, data: &[u32]);
    /// `%m`: a new allocation holding `data`, its address stored in the pointer at `addr`.
    fn alloc_bytes(&mut self, addr: usize, data: &[u8]) -> Result<()>;
    /// `%m` for wide characters.
    fn alloc_wide(&mut self, addr: usize, data: &[u32]) -> Result<()>;
    /// `%m` that failed: a null pointer stored at `addr`.
    fn null(&mut self, addr: usize);
}

/// What a scan needs from the caller's environment.
#[derive(Clone, Copy)]
pub struct Ctx<'a> {
    /// The calling thread's locale: white space, the radix and thousands separator, and the
    /// multibyte encoding of `%ls` (byte input) and `%s` (wide input).
    pub loc: &'a Locale,
    /// The locale's multibyte encoding.
    pub enc: Encoding,
    /// The rounding mode of floating conversions.
    pub rounding: Rounding,
    /// The symbol set.
    pub set: Set,
}

/// How a scan ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Outcome {
    /// The number of assignments made.
    Assigned(usize),
    /// An input failure (end of input, a read or encoding error) before the first conversion
    /// completed: `EOF`.
    InputFailure,
}

/// The result of [`scan`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Scanned {
    /// How it ended.
    pub outcome: Outcome,
    /// An error to report through `errno` (a read error, or `EILSEQ`), whatever the outcome.
    pub error: Option<Errno>,
}

// ------------------------------------------------------------------------------ the format

/// The unit at `i` of a format, `None` past its end.
fn at<F: FmtUnit>(fmt: &[F], i: usize) -> Option<u32> {
    fmt.get(i).map(|u| u.code())
}

/// Whether `u` is the ASCII character `c`.
fn is(u: Option<u32>, c: u8) -> bool {
    u == Some(u32::from(c))
}

/// `u` as a byte, `None` when it is none or above 255.
fn byte(u: Option<u32>) -> Option<u8> {
    u.and_then(|c| u8::try_from(c).ok())
}

/// The ASCII lower case of `u`; 0 when it is none or not ASCII.
fn lower(u: Option<u32>) -> u8 {
    byte(u).filter(u8::is_ascii).map_or(0, |b| b.to_ascii_lowercase())
}

/// The value of `u` as an ASCII digit of `base` (2 to 16).
fn digit(u: Option<u32>, base: u32) -> Option<u32> {
    char::from_u32(u?)?.to_digit(base)
}

/// A decimal number at `*i` of the format, advancing past it; `None` without a digit.
fn number<F: FmtUnit>(fmt: &[F], i: &mut usize) -> Option<usize> {
    let mut n: Option<usize> = None;
    while let Some(d) = digit(at(fmt, *i), 10) {
        let d = usize::try_from(d).unwrap_or(0);
        n = Some(n.unwrap_or(0).saturating_mul(10).saturating_add(d));
        *i = i.saturating_add(1);
    }
    n
}

/// A length modifier.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Len {
    /// `hh`.
    Char,
    /// `h`.
    Short,
    /// None given.
    Int,
    /// `l`.
    Long,
    /// `ll`, `q`, `L`.
    LongLong,
    /// `j`, `z`, `Z`, `t` (and `%p`).
    Max,
}

impl Len {
    /// The size in bytes of an integer destination (x86_64).
    fn int_size(self) -> usize {
        match self {
            Len::Char => 1,
            Len::Short => 2,
            Len::Int => 4,
            Len::Long | Len::LongLong | Len::Max => 8,
        }
    }

    /// The format of a floating destination and the bytes stored (`long double` is the x87
    /// 80-bit format; its padding is left alone).
    fn float(self) -> (Format, usize) {
        match self {
            Len::Long => (Format::F64, 8),
            Len::LongLong => (Format::F80, 10),
            Len::Char | Len::Short | Len::Int | Len::Max => (Format::F32, 4),
        }
    }
}

/// What a specification converts.
#[derive(Clone, Copy)]
enum Conv {
    /// An integer in `base` (0: by its prefix); `bin`: base 0 also takes a `0b` prefix.
    Int { base: u32, signed: bool, bin: bool },
    /// `a e f g` and their capitals.
    Float,
    /// `c`, `C`.
    Chars,
    /// `s`, `S`.
    Str,
    /// `[`: the set's text is `fmt[start..end]`.
    Set { neg: bool, start: usize, end: usize },
    /// `n`.
    Count,
    /// `%%`.
    Percent,
}

/// One conversion specification: `%[n$][*'][width][m][len]conv`.
#[derive(Clone, Copy)]
struct Spec {
    /// The positional argument (`%n$`).
    arg: Option<usize>,
    /// `*`: convert, assign nothing.
    skip: bool,
    /// `'`: accept the locale's thousands separator after a digit.
    group: bool,
    /// `None`: not given (or 0).
    width: Option<usize>,
    /// `m` (or the plain set's `%a` before `s`, `S`, `[`).
    alloc: bool,
    len: Len,
    /// A text conversion into `wchar_t` (`l`, `S`, `C`).
    wide: bool,
    conv: Conv,
}

/// Decode the specification after a `%` (`start` is the index after it): the specification and
/// the index after it, or `None` when it is not a valid one.
fn spec<F: FmtUnit>(fmt: &[F], start: usize, set: Set) -> Option<(Spec, usize)> {
    let mut s = Spec { arg: None, skip: false, group: false, width: None, alloc: false, len: Len::Int, wide: false, conv: Conv::Percent };
    let mut i = start;
    if is(at(fmt, i), b'%') {
        return Some((s, i.saturating_add(1)));
    }
    let mut j = i;
    let pos = number(fmt, &mut j).filter(|_| is(at(fmt, j), b'$'));
    if let Some(n) = pos {
        if n == 0 {
            return None;
        }
        s.arg = Some(n);
        i = j.saturating_add(1);
    }
    loop {
        match byte(at(fmt, i)) {
            Some(b'*') => s.skip = true,
            Some(b'\'') => s.group = true,
            _ => break,
        }
        i = i.saturating_add(1);
    }
    s.width = number(fmt, &mut i).filter(|&w| w > 0);
    if is(at(fmt, i), b'm') {
        s.alloc = true;
        i = i.saturating_add(1);
    }
    let (len, used) = match (byte(at(fmt, i)), byte(at(fmt, i.saturating_add(1)))) {
        (Some(b'h'), Some(b'h')) => (Len::Char, 2),
        (Some(b'h'), _) => (Len::Short, 1),
        (Some(b'l'), Some(b'l')) => (Len::LongLong, 2),
        (Some(b'l'), _) => (Len::Long, 1),
        (Some(b'q' | b'L'), _) => (Len::LongLong, 1),
        (Some(b'j' | b'z' | b'Z' | b't'), _) => (Len::Max, 1),
        _ => (Len::Int, 0),
    };
    s.len = len;
    i = i.saturating_add(used);
    let mut conv = byte(at(fmt, i))?;
    i = i.saturating_add(1);
    if conv == b'a' && set == Set::Gnu && matches!(byte(at(fmt, i)), Some(b's' | b'S' | b'[')) {
        s.alloc = true;
        conv = byte(at(fmt, i))?;
        i = i.saturating_add(1);
    }
    s.conv = match conv {
        b'd' => Conv::Int { base: 10, signed: true, bin: false },
        b'i' => Conv::Int { base: 0, signed: true, bin: set == Set::C23 },
        b'o' => Conv::Int { base: 8, signed: false, bin: false },
        b'u' => Conv::Int { base: 10, signed: false, bin: false },
        b'x' | b'X' => Conv::Int { base: 16, signed: false, bin: false },
        b'b' => Conv::Int { base: 2, signed: false, bin: false },
        b'p' => {
            s.len = Len::Max;
            Conv::Int { base: 16, signed: false, bin: false }
        }
        b'a' | b'A' | b'e' | b'E' | b'f' | b'F' | b'g' | b'G' => Conv::Float,
        b'c' => Conv::Chars,
        b'C' => {
            s.wide = true;
            Conv::Chars
        }
        b's' => Conv::Str,
        b'S' => {
            s.wide = true;
            Conv::Str
        }
        b'[' => {
            let (neg, set_start, end) = set_bounds(fmt, i)?;
            i = end.saturating_add(1);
            Conv::Set { neg, start: set_start, end }
        }
        b'n' => Conv::Count,
        _ => return None,
    };
    if matches!(s.conv, Conv::Chars | Conv::Str | Conv::Set { .. }) {
        s.wide |= s.len == Len::Long;
    } else if s.alloc {
        return None;
    }
    Some((s, i))
}

/// The bounds of a set after `%[` (`i` is the index after the `[`): negated, the start of its
/// text, and the index of its closing `]`; `None` when it is not closed. A `]` first (after any
/// `^`) is a member.
fn set_bounds<F: FmtUnit>(fmt: &[F], i: usize) -> Option<(bool, usize, usize)> {
    let neg = is(at(fmt, i), b'^');
    let start = if neg { i.saturating_add(1) } else { i };
    let mut k = if is(at(fmt, start), b']') { start.saturating_add(1) } else { start };
    loop {
        let u = at(fmt, k)?;
        if u == u32::from(b']') {
            return Some((neg, start, k));
        }
        k = k.saturating_add(1);
    }
}

/// Call `f(lo, hi)` for each member range of the set text `fmt[start..end]`: `a-z` is a range
/// when `a <= z`, a reversed one is its three characters, and a `-` first or last is a member.
fn set_items<F: FmtUnit>(fmt: &[F], start: usize, end: usize, mut f: impl FnMut(u32, u32)) {
    let mut k = start;
    while k < end {
        let Some(c) = at(fmt, k) else { break };
        let after = k.saturating_add(2);
        let range_end = at(fmt, after).filter(|_| after < end && is(at(fmt, k.saturating_add(1)), b'-'));
        if let Some(hi) = range_end {
            if c <= hi {
                f(c, hi);
            } else {
                f(c, c);
                f(u32::from(b'-'), u32::from(b'-'));
                f(hi, hi);
            }
            k = k.saturating_add(3);
            continue;
        }
        f(c, c);
        k = k.saturating_add(1);
    }
}

/// A compiled `%[` set.
struct ScanSet {
    /// Members below 256, one bit each.
    map: [u64; 4],
    neg: bool,
    start: usize,
    end: usize,
    /// A range reaches above 255 (wide formats only): such units are tested against the text.
    high: bool,
}

impl ScanSet {
    fn new<F: FmtUnit>(fmt: &[F], neg: bool, start: usize, end: usize) -> ScanSet {
        let mut map = [0u64; 4];
        let mut high = false;
        set_items(fmt, start, end, |lo, hi| {
            high |= hi > 255;
            let mut c = lo;
            while let Some(b) = u8::try_from(c).ok().filter(|_| c <= hi) {
                if let Some(word) = map.get_mut(usize::from(b.wrapping_shr(6))) {
                    *word |= 1u64.wrapping_shl(u32::from(b & 63));
                }
                c = c.saturating_add(1);
            }
        });
        ScanSet { map, neg, start, end, high }
    }

    fn contains<F: FmtUnit>(&self, fmt: &[F], u: u32) -> bool {
        let member = match u8::try_from(u) {
            Ok(b) => self.map.get(usize::from(b.wrapping_shr(6))).is_some_and(|w| w.wrapping_shr(u32::from(b & 63)) & 1 == 1),
            Err(_) if self.high => {
                let mut hit = false;
                set_items(fmt, self.start, self.end, |lo, hi| hit |= (lo..=hi).contains(&u));
                hit
            }
            Err(_) => false,
        };
        member != self.neg
    }
}

// ------------------------------------------------------------------------------- the input

/// The input with the scan's count of units consumed and its first error.
struct Reader<'a, I: Input + ?Sized> {
    input: &'a mut I,
    loc: &'a Locale,
    /// Wide input (white space is a wide class).
    wide: bool,
    /// Units consumed so far (`%n`).
    used: usize,
    /// The input ended or failed: no further reads.
    ended: bool,
    error: Option<Errno>,
}

impl<I: Input + ?Sized> Reader<'_, I> {
    fn get(&mut self) -> Option<u32> {
        if self.ended {
            return None;
        }
        match self.input.next() {
            Ok(Some(c)) => {
                self.used = self.used.saturating_add(1);
                Some(c)
            }
            Ok(None) => {
                self.ended = true;
                None
            }
            Err(e) => {
                self.ended = true;
                self.error = self.error.or(Some(e));
                None
            }
        }
    }

    fn unget(&mut self, c: Option<u32>) {
        if let Some(u) = c {
            self.input.back(u);
            self.used = self.used.saturating_sub(1);
        }
    }

    fn space(&self, c: u32) -> bool {
        is_space(c, self.wide, self.loc)
    }

    /// Skip white space; false at the end of input.
    fn skip_space(&mut self) -> bool {
        while let Some(c) = self.get() {
            if !self.space(c) {
                self.unget(Some(c));
                return true;
            }
        }
        false
    }
}

fn is_space(c: u32, wide: bool, loc: &Locale) -> bool {
    if wide {
        ctype::is_w(c, Class::SPACE, loc)
    } else {
        u8::try_from(c).is_ok_and(|b| ctype::classes(b, loc).intersects(Class::SPACE))
    }
}

/// The units a field may still take.
struct Field(usize);

impl Field {
    fn take<I: Input + ?Sized>(&mut self, r: &mut Reader<'_, I>) -> Option<u32> {
        if self.0 == 0 {
            return None;
        }
        let c = r.get()?;
        self.0 = self.0.saturating_sub(1);
        Some(c)
    }
}

/// How reading a field ended.
enum Got<T> {
    /// A complete field.
    Done(T),
    /// The input does not match (what was read of the field is consumed).
    Mismatch,
    /// End of input (or an error) before the field's first unit.
    Eof,
}

/// Whether `c` is the first unit of the separator `sep`.
fn starts(c: Option<u32>, sep: &[u32]) -> bool {
    sep.first().is_some_and(|&s| c == Some(s))
}

/// The rest of a separator whose first unit was read: `Some(the unit after it)`, or `None` when
/// the input does not continue it (the unit that does not is given back).
fn rest_of<I: Input + ?Sized>(r: &mut Reader<'_, I>, f: &mut Field, sep: &[u32]) -> Option<Option<u32>> {
    for &s in sep.get(1..).unwrap_or(&[]) {
        let c = f.take(r);
        if c != Some(s) {
            r.unget(c);
            return None;
        }
    }
    Some(f.take(r))
}

// ------------------------------------------------------------------------------ the fields

/// An integer field in `base` (0: by prefix): (negative, magnitude, overflowed).
fn read_int<I: Input + ?Sized>(r: &mut Reader<'_, I>, width: usize, base: u32, bin: bool, group: &[u32]) -> Got<(bool, u64, bool)> {
    let mut f = Field(width);
    let mut c = f.take(r);
    if c.is_none() {
        return Got::Eof;
    }
    let neg = is(c, b'-');
    if neg || is(c, b'+') {
        c = f.take(r);
    }
    let mut base = base;
    let mut digits = 0usize;
    if is(c, b'0') && matches!(base, 0 | 2 | 16) {
        // The `0` of a prefix is a digit: `0x` alone is 0.
        digits = 1;
        c = f.take(r);
        let l = lower(c);
        if l == b'x' && base != 2 {
            base = 16;
            c = f.take(r);
        } else if l == b'b' && (base == 2 || bin) {
            base = 2;
            c = f.take(r);
        } else if base == 0 {
            base = 8;
        }
    } else if base == 0 {
        base = 10;
    }
    let (mut mag, mut over) = (0u64, false);
    loop {
        if let Some(d) = digit(c, base) {
            match mag.checked_mul(u64::from(base)).and_then(|m| m.checked_add(u64::from(d))) {
                Some(m) => mag = m,
                None => over = true,
            }
            digits = digits.saturating_add(1);
            c = f.take(r);
        } else if digits > 0 && starts(c, group) {
            match rest_of(r, &mut f, group) {
                Some(next) => c = next,
                None => return Got::Mismatch,
            }
        } else {
            break;
        }
    }
    r.unget(c);
    if digits == 0 { Got::Mismatch } else { Got::Done((neg, mag, over)) }
}

/// The value an integer conversion stores in a `size`-byte object (its low bytes): signed out
/// of range is the nearest bound; unsigned clamps the magnitude, then a `-` negates modulo 2^N
/// (D-12.6).
fn int_value(neg: bool, mag: u64, over: bool, size: usize, signed: bool) -> u64 {
    let bits = u32::try_from(size.saturating_mul(8)).unwrap_or(64).clamp(8, 64);
    let umax = u64::MAX.wrapping_shr(64u32.saturating_sub(bits));
    if signed {
        let max = umax.wrapping_shr(1);
        let min_mag = max.saturating_add(1);
        if !neg {
            if over || mag > max { max } else { mag }
        } else if over || mag > min_mag {
            min_mag.wrapping_neg()
        } else {
            mag.wrapping_neg()
        }
    } else if over || mag > umax {
        umax
    } else if neg {
        mag.wrapping_neg() & umax
    } else {
        mag
    }
}

/// A floating field, normalised to the ASCII text `strtod` reads (`.` for the radix, no
/// separators). `group` is accepted after a digit of a decimal integer part.
fn read_float<I: Input + ?Sized>(r: &mut Reader<'_, I>, width: usize, group: &[u32], radix: &[u32]) -> Result<Got<TryVec<u8>>> {
    let mut f = Field(width);
    let mut t = TryVec::new();
    let mut c = f.take(r);
    if c.is_none() {
        return Ok(Got::Eof);
    }
    if is(c, b'-') || is(c, b'+') {
        t.push(if is(c, b'-') { b'-' } else { b'+' })?;
        c = f.take(r);
    }
    match lower(c) {
        b'i' => return word(r, &mut f, c, t, b"infinity", &[3, 8]),
        b'n' => return word(r, &mut f, c, t, b"nan", &[3]),
        _ => {}
    }
    let mut base = 10;
    let mut digits = 0usize;
    if is(c, b'0') {
        t.push(b'0')?;
        digits = 1;
        c = f.take(r);
        if lower(c) == b'x' {
            // `0x` starts a hexadecimal number; the `0` is not one of its digits.
            base = 16;
            digits = 0;
            t.push(b'x')?;
            c = f.take(r);
        }
    }
    loop {
        if let Some(d) = digit(c, base) {
            t.push(hex_char(d))?;
            digits = digits.saturating_add(1);
            c = f.take(r);
        } else if base == 10 && digits > 0 && starts(c, group) && !starts(c, radix) {
            match rest_of(r, &mut f, group) {
                Some(next) => c = next,
                None => return Ok(Got::Mismatch),
            }
        } else {
            break;
        }
    }
    if starts(c, radix) {
        match rest_of(r, &mut f, radix) {
            Some(next) => c = next,
            None => return Ok(Got::Mismatch),
        }
        t.push(b'.')?;
        while let Some(d) = digit(c, base) {
            t.push(hex_char(d))?;
            digits = digits.saturating_add(1);
            c = f.take(r);
        }
    }
    if digits == 0 {
        r.unget(c);
        return Ok(Got::Mismatch);
    }
    let mark = if base == 16 { b'p' } else { b'e' };
    if lower(c) == mark {
        t.push(mark)?;
        c = f.take(r);
        if is(c, b'-') || is(c, b'+') {
            t.push(if is(c, b'-') { b'-' } else { b'+' })?;
            c = f.take(r);
        }
        let mut exp = 0usize;
        while let Some(d) = digit(c, 10) {
            t.push(hex_char(d))?;
            exp = exp.saturating_add(1);
            c = f.take(r);
        }
        if exp == 0 {
            r.unget(c);
            return Ok(Got::Mismatch);
        }
    }
    r.unget(c);
    Ok(Got::Done(t))
}

/// The ASCII digit for `d` (below 16).
fn hex_char(d: u32) -> u8 {
    char::from_digit(d, 16).and_then(|ch| u8::try_from(u32::from(ch)).ok()).unwrap_or(b'0')
}

/// `inf`/`infinity` or `nan` (case-insensitive) from `c` on: the longest prefix of `word` the
/// input gives, complete when its length is one of `complete`.
fn word<I: Input + ?Sized>(r: &mut Reader<'_, I>, f: &mut Field, mut c: Option<u32>, mut t: TryVec<u8>, word: &[u8], complete: &[usize]) -> Result<Got<TryVec<u8>>> {
    let mut n = 0usize;
    while let Some(&w) = word.get(n) {
        if lower(c) != w {
            break;
        }
        n = n.saturating_add(1);
        c = if n < word.len() { f.take(r) } else { None };
    }
    r.unget(c);
    if !complete.contains(&n) {
        return Ok(Got::Mismatch);
    }
    t.extend_from_slice(word.get(..3).unwrap_or(word))?;
    Ok(Got::Done(t))
}

/// Which units a text field takes.
enum Accept {
    /// `%c`: any.
    Any,
    /// `%s`: up to white space.
    NotSpace,
    /// `%[`: the set's members.
    Set(ScanSet),
}

/// Where converted text goes.
#[derive(Clone, Copy)]
enum Dest {
    /// Suppressed: nowhere.
    Discard,
    /// The caller's object at this address.
    At(usize),
    /// `%m`: a new allocation, its address stored in the pointer at this address.
    Alloc(usize),
}

/// Units buffered before a store to the caller's object.
const CHUNK: usize = 1024;

/// A text field converted to its destination's units: bytes for `%s`/`%c`/`%[` (wide input is
/// encoded), wide characters for `%ls`/`%lc`/`%l[` (byte input is decoded).
struct Out {
    dest: Dest,
    from_wide: bool,
    to_wide: bool,
    enc: Encoding,
    /// Bytes of a character not yet complete (byte input to a wide destination).
    state: MbState,
    bytes: TryVec<u8>,
    wides: TryVec<u32>,
    /// Bytes already stored at an [`Dest::At`] object.
    stored: usize,
}

/// What one input unit was to a text field.
enum Fed {
    /// It completed a character (what the width counts).
    Char,
    /// It began or continued a multibyte character.
    Partial,
    /// An invalid sequence, or a character the encoding cannot represent.
    Bad,
}

impl Out {
    fn new(dest: Dest, from_wide: bool, to_wide: bool, enc: Encoding) -> Out {
        Out { dest, from_wide, to_wide, enc, state: MbState::INITIAL, bytes: TryVec::new(), wides: TryVec::new(), stored: 0 }
    }

    fn feed(&mut self, sink: &mut dyn Sink, u: u32) -> Result<Fed> {
        match (self.from_wide, self.to_wide) {
            (false, false) | (true, true) => self.put(sink, u)?,
            (false, true) => match text::decode(&mut self.state, &[byte(Some(u)).unwrap_or(0)], self.enc) {
                Decoded::Char(c, _) | Decoded::Continuation(c) => self.put(sink, c)?,
                Decoded::Null => self.put(sink, 0)?,
                Decoded::Incomplete => return Ok(Fed::Partial),
                Decoded::Invalid => return Ok(Fed::Bad),
            },
            (true, false) => {
                let mut buf = [0u8; text::MAX_ENCODED];
                let Some(n) = text::encode(u, self.enc, &mut buf) else { return Ok(Fed::Bad) };
                for &b in buf.get(..n).unwrap_or(&[]) {
                    self.put(sink, u32::from(b))?;
                }
            }
        }
        Ok(Fed::Char)
    }

    /// A character is incomplete.
    fn pending(&self) -> bool {
        !self.state.is_initial()
    }

    fn put(&mut self, sink: &mut dyn Sink, u: u32) -> Result<()> {
        match self.dest {
            Dest::Discard => return Ok(()),
            Dest::At(_) | Dest::Alloc(_) if self.to_wide => self.wides.push(u)?,
            Dest::At(_) | Dest::Alloc(_) => self.bytes.push(byte(Some(u)).unwrap_or(0))?,
        }
        if matches!(self.dest, Dest::At(_)) && self.bytes.len().saturating_add(self.wides.len()) >= CHUNK {
            self.flush(sink);
        }
        Ok(())
    }

    /// Store what is buffered at the caller's object, after what was stored before.
    fn flush(&mut self, sink: &mut dyn Sink) {
        let Dest::At(addr) = self.dest else { return };
        let at = addr.wrapping_add(self.stored);
        if self.to_wide {
            if !self.wides.is_empty() {
                sink.wide(at, &self.wides);
                self.stored = self.stored.saturating_add(self.wides.len().saturating_mul(4));
                self.wides.clear();
            }
        } else if !self.bytes.is_empty() {
            sink.bytes(at, &self.bytes);
            self.stored = self.stored.saturating_add(self.bytes.len());
            self.bytes.clear();
        }
    }

    /// The field is complete: terminate it (`%s`, `%[`) and store it.
    fn finish(&mut self, sink: &mut dyn Sink, terminate: bool) -> Result<()> {
        if terminate {
            self.put(sink, 0)?;
        }
        match self.dest {
            Dest::Discard => Ok(()),
            Dest::At(_) => {
                self.flush(sink);
                Ok(())
            }
            Dest::Alloc(addr) if self.to_wide => sink.alloc_wide(addr, &self.wides),
            Dest::Alloc(addr) => sink.alloc_bytes(addr, &self.bytes),
        }
    }

    /// The conversion failed: what was converted stays stored (no terminator); `%m` stores NULL.
    fn fail(&mut self, sink: &mut dyn Sink) {
        match self.dest {
            Dest::Discard => {}
            Dest::At(_) => self.flush(sink),
            Dest::Alloc(addr) => sink.null(addr),
        }
    }
}

/// How a text field ended.
enum TextEnd {
    Done,
    Mismatch,
    Eof,
    /// An encoding error (`EILSEQ`).
    Bad,
}

/// A text field of at most `width` characters into `out`.
fn read_text<F: FmtUnit, I: Input + ?Sized>(r: &mut Reader<'_, I>, fmt: &[F], accept: &Accept, width: usize, out: &mut Out, sink: &mut dyn Sink) -> Result<TextEnd> {
    let mut left = width;
    let mut any = false;
    let mut eof = false;
    while left > 0 {
        let Some(u) = r.get() else {
            eof = true;
            break;
        };
        let member = match accept {
            Accept::Any => true,
            Accept::NotSpace => !r.space(u),
            Accept::Set(set) => set.contains(fmt, u),
        };
        if !member {
            r.unget(Some(u));
            break;
        }
        any = true;
        match out.feed(sink, u)? {
            Fed::Char => left = left.saturating_sub(1),
            Fed::Partial => {}
            Fed::Bad => return Ok(TextEnd::Bad),
        }
    }
    Ok(match (any, eof) {
        (false, true) => TextEnd::Eof,
        (false, false) => TextEnd::Mismatch,
        (true, _) if out.pending() => TextEnd::Bad,
        (true, _) => TextEnd::Done,
    })
}

// ------------------------------------------------------------------------------- the scan

/// `LC_NUMERIC` word items of a wide locale (`<langinfo.h>` order):
/// `_NL_NUMERIC_DECIMAL_POINT_WC` and `_NL_NUMERIC_THOUSANDS_SEP_WC`.
const WIDE_RADIX: usize = 3;
const WIDE_THOUSANDS: usize = 4;

/// A numeric item of the locale in the input's units: `narrow` as bytes, or the wide word item
/// `wide_item` as one wide character. Empty when there is none.
fn numeric(loc: &Locale, wide: bool, narrow: &[u8], wide_item: usize) -> InlineVec<u32, 16> {
    let mut v = InlineVec::default();
    if wide {
        let w = match loc.item(Cat::Numeric, wide_item) {
            Item::Word(w) => w,
            Item::Bytes(_) => 0,
        };
        if w != 0 && v.push(w).is_err() {
            return InlineVec::default();
        }
    } else {
        for &b in narrow {
            if v.push(u32::from(b)).is_err() {
                return InlineVec::default();
            }
        }
    }
    v
}

/// How the walk over the format stopped.
#[derive(PartialEq, Eq)]
enum Stop {
    /// The end of the format.
    Done,
    /// A matching failure (or an invalid specification, or an encoding error).
    Mismatch,
    /// An input failure.
    InputFailure,
}

/// The scan's counts.
struct Run {
    /// Assignments made.
    assigned: usize,
    /// A conversion completed (suppressed ones included; `%n` is none).
    converted: bool,
    /// Sequential arguments used.
    seq: usize,
}

impl Run {
    /// The destination address of a conversion: its positional argument, or the next one.
    fn arg(&mut self, sink: &mut dyn Sink, pos: Option<usize>) -> usize {
        let n = match pos {
            Some(n) => n,
            None => {
                self.seq = self.seq.saturating_add(1);
                self.seq
            }
        };
        sink.pointer(n)
    }

    fn done(&mut self, assigned: bool) {
        self.converted = true;
        if assigned {
            self.assigned = self.assigned.saturating_add(1);
        }
    }
}

/// Run `fmt` over `input`, storing through `sink` (spec §12.7). `Err` only when memory for a
/// field cannot be had (`ENOMEM`).
pub fn scan<F: FmtUnit, I: Input + ?Sized>(fmt: &[F], input: &mut I, sink: &mut dyn Sink, ctx: Ctx<'_>) -> Result<Scanned> {
    let mut r = Reader { input, loc: ctx.loc, wide: F::WIDE, used: 0, ended: false, error: None };
    let mut run = Run { assigned: 0, converted: false, seq: 0 };
    let stop = directives(fmt, &mut r, sink, &ctx, &mut run)?;
    let outcome = if stop == Stop::InputFailure && !run.converted { Outcome::InputFailure } else { Outcome::Assigned(run.assigned) };
    Ok(Scanned { outcome, error: r.error })
}

/// Execute the format's directives in order.
fn directives<F: FmtUnit, I: Input + ?Sized>(fmt: &[F], r: &mut Reader<'_, I>, sink: &mut dyn Sink, ctx: &Ctx<'_>, run: &mut Run) -> Result<Stop> {
    let mut i = 0usize;
    while let Some(u) = at(fmt, i) {
        if is_space(u, F::WIDE, ctx.loc) {
            while at(fmt, i).is_some_and(|u| is_space(u, F::WIDE, ctx.loc)) {
                i = i.saturating_add(1);
            }
            r.skip_space();
            continue;
        }
        i = i.saturating_add(1);
        if u != u32::from(b'%') {
            match r.get() {
                Some(c) if c == u => continue,
                Some(c) => {
                    r.unget(Some(c));
                    return Ok(Stop::Mismatch);
                }
                None => return Ok(Stop::InputFailure),
            }
        }
        let Some((s, next)) = spec(fmt, i, ctx.set) else { return Ok(Stop::Mismatch) };
        i = next;
        if let Some(stop) = convert(fmt, &s, r, sink, ctx, run)? {
            return Ok(stop);
        }
    }
    Ok(Stop::Done)
}

/// Execute one conversion; `Some` when the scan stops.
fn convert<F: FmtUnit, I: Input + ?Sized>(fmt: &[F], s: &Spec, r: &mut Reader<'_, I>, sink: &mut dyn Sink, ctx: &Ctx<'_>, run: &mut Run) -> Result<Option<Stop>> {
    let width = s.width.unwrap_or(usize::MAX);
    match s.conv {
        Conv::Percent => {
            if !r.skip_space() {
                return Ok(Some(Stop::InputFailure));
            }
            let c = r.get();
            if is(c, b'%') {
                return Ok(None);
            }
            r.unget(c);
            Ok(Some(Stop::Mismatch))
        }
        Conv::Count => {
            if !s.skip {
                let addr = run.arg(sink, s.arg);
                sink.int(addr, s.len.int_size(), u64::try_from(r.used).unwrap_or(u64::MAX));
            }
            Ok(None)
        }
        Conv::Int { base, signed, bin } => {
            if !r.skip_space() {
                return Ok(Some(Stop::InputFailure));
            }
            let group = if s.group { numeric(ctx.loc, F::WIDE, ctx.loc.thousands_sep(), WIDE_THOUSANDS) } else { InlineVec::default() };
            match read_int(r, width, base, bin, group.as_slice()) {
                Got::Eof => Ok(Some(Stop::InputFailure)),
                Got::Mismatch => Ok(Some(Stop::Mismatch)),
                Got::Done((neg, mag, over)) => {
                    if !s.skip {
                        let size = s.len.int_size();
                        let addr = run.arg(sink, s.arg);
                        sink.int(addr, size, int_value(neg, mag, over, size, signed));
                    }
                    run.done(!s.skip);
                    Ok(None)
                }
            }
        }
        Conv::Float => {
            if !r.skip_space() {
                return Ok(Some(Stop::InputFailure));
            }
            let mut radix = numeric(ctx.loc, F::WIDE, ctx.loc.decimal_point(), WIDE_RADIX);
            if radix.is_empty() && radix.push(u32::from(b'.')).is_err() {
                return Ok(Some(Stop::Mismatch));
            }
            let group = if s.group { numeric(ctx.loc, F::WIDE, ctx.loc.thousands_sep(), WIDE_THOUSANDS) } else { InlineVec::default() };
            let text = match read_float(r, width, group.as_slice(), radix.as_slice())? {
                Got::Eof => return Ok(Some(Stop::InputFailure)),
                Got::Mismatch => return Ok(Some(Stop::Mismatch)),
                Got::Done(text) => text,
            };
            let (format, size) = s.len.float();
            let parsed = parse_float(text.as_slice(), b".".as_slice(), format, ctx.rounding, Dialect::C99);
            if parsed.used != text.len() {
                return Ok(Some(Stop::Mismatch));
            }
            if !s.skip {
                let bits = parsed.value.pack(format).to_le_bytes();
                let addr = run.arg(sink, s.arg);
                sink.bytes(addr, bits.get(..size).unwrap_or(&[]));
            }
            run.done(!s.skip);
            Ok(None)
        }
        Conv::Chars | Conv::Str | Conv::Set { .. } => text_conv(fmt, s, r, sink, ctx, run),
    }
}

/// `%c`, `%s`, `%[` and their wide and allocating forms.
fn text_conv<F: FmtUnit, I: Input + ?Sized>(fmt: &[F], s: &Spec, r: &mut Reader<'_, I>, sink: &mut dyn Sink, ctx: &Ctx<'_>, run: &mut Run) -> Result<Option<Stop>> {
    let (accept, terminate, width) = match s.conv {
        Conv::Set { neg, start, end } => (Accept::Set(ScanSet::new(fmt, neg, start, end)), true, s.width.unwrap_or(usize::MAX)),
        Conv::Str => (Accept::NotSpace, true, s.width.unwrap_or(usize::MAX)),
        _ => (Accept::Any, false, s.width.unwrap_or(1)),
    };
    let dest = match (s.skip, s.alloc) {
        (true, _) => Dest::Discard,
        (false, true) => Dest::Alloc(run.arg(sink, s.arg)),
        (false, false) => Dest::At(run.arg(sink, s.arg)),
    };
    let mut out = Out::new(dest, F::WIDE, s.wide, ctx.enc);
    if matches!(s.conv, Conv::Str) && !r.skip_space() {
        out.fail(sink);
        return Ok(Some(Stop::InputFailure));
    }
    match read_text(r, fmt, &accept, width, &mut out, sink)? {
        TextEnd::Done => {
            out.finish(sink, terminate)?;
            run.done(!s.skip);
            Ok(None)
        }
        TextEnd::Mismatch => {
            out.fail(sink);
            Ok(Some(Stop::Mismatch))
        }
        TextEnd::Eof => {
            out.fail(sink);
            Ok(Some(Stop::InputFailure))
        }
        TextEnd::Bad => {
            out.fail(sink);
            r.error = r.error.or(Some(Errno::EILSEQ));
            Ok(Some(Stop::Mismatch))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::arithmetic_side_effects)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    struct Str<'a> {
        s: &'a [u8],
        pos: usize,
    }

    impl Input for Str<'_> {
        fn next(&mut self) -> Result<Option<u32>> {
            let c = self.s.get(self.pos).map(|&b| u32::from(b));
            if c.is_some() {
                self.pos += 1;
            }
            Ok(c)
        }
        fn back(&mut self, _: u32) {
            self.pos -= 1;
        }
    }

    /// Arguments are slots of 16 bytes; pointer `n` is `n * 16`.
    #[derive(Default)]
    struct Slots(Vec<(usize, Vec<u8>)>);

    impl Sink for Slots {
        fn pointer(&mut self, n: usize) -> usize {
            n * 16
        }
        fn int(&mut self, addr: usize, size: usize, value: u64) {
            self.0.push((addr, value.to_le_bytes()[..size].to_vec()));
        }
        fn bytes(&mut self, addr: usize, data: &[u8]) {
            self.0.push((addr, data.to_vec()));
        }
        fn wide(&mut self, addr: usize, data: &[u32]) {
            self.0.push((addr, data.iter().flat_map(|w| w.to_le_bytes()).collect()));
        }
        fn alloc_bytes(&mut self, addr: usize, data: &[u8]) -> Result<()> {
            self.bytes(addr + 1, data);
            Ok(())
        }
        fn alloc_wide(&mut self, addr: usize, data: &[u32]) -> Result<()> {
            self.wide(addr + 1, data);
            Ok(())
        }
        fn null(&mut self, addr: usize) {
            self.0.push((addr + 2, Vec::new()));
        }
    }

    fn run(set: Set, input: &str, fmt: &str) -> (Outcome, usize, Vec<(usize, Vec<u8>)>) {
        let ctx = Ctx { loc: &Locale::C, enc: Encoding::Ascii, rounding: Rounding::Nearest, set };
        let mut inp = Str { s: input.as_bytes(), pos: 0 };
        let mut sink = Slots::default();
        let r = scan(fmt.as_bytes(), &mut inp, &mut sink, ctx).unwrap();
        (r.outcome, inp.pos, sink.0)
    }

    #[test]
    fn integers_follow_strtol_with_one_unit_of_pushback() {
        assert_eq!(run(Set::C99, "  12abc", "%d"), (Outcome::Assigned(1), 4, vec![(16, vec![12, 0, 0, 0])]));
        assert_eq!(run(Set::C99, "0xg", "%x"), (Outcome::Assigned(1), 2, vec![(16, vec![0, 0, 0, 0])]));
        assert_eq!(run(Set::C99, "0b101", "%i").1, 1);
        assert_eq!(run(Set::C23, "0b101", "%i").2, vec![(16, vec![5, 0, 0, 0])]);
        assert_eq!(run(Set::C99, "-", "%d"), (Outcome::Assigned(0), 1, vec![]));
        assert_eq!(run(Set::C99, "", "%d").0, Outcome::InputFailure);
        assert_eq!(run(Set::C99, "300", "%hhd").2, vec![(16, vec![0x7f])]);
        assert_eq!(run(Set::C99, "-1", "%u").2, vec![(16, vec![0xff; 4])]);
    }

    #[test]
    fn partial_numbers_are_consumed_and_fail() {
        assert_eq!(run(Set::C99, "1e+x", "%lf"), (Outcome::Assigned(0), 3, vec![]));
        assert_eq!(run(Set::C99, "infin", "%f"), (Outcome::Assigned(0), 5, vec![]));
        assert_eq!(run(Set::C99, "1.5", "%lf").2, vec![(16, 1.5f64.to_le_bytes().to_vec())]);
        assert_eq!(run(Set::C99, "0x1p3", "%f").2, vec![(16, 8.0f32.to_le_bytes().to_vec())]);
    }

    #[test]
    fn text_sets_allocation_and_counts() {
        assert_eq!(run(Set::C99, "abc-", "%[c-a]").2, vec![(16, b"a\0".to_vec())]);
        assert_eq!(run(Set::C99, "x", "%3c"), (Outcome::Assigned(1), 1, vec![(16, b"x".to_vec())]));
        assert_eq!(run(Set::Gnu, "abc", "%as %ms").2, vec![(17, b"abc\0".to_vec()), (34, vec![])]);
        assert_eq!(run(Set::C99, "1 2", "%d %n%d").2, vec![(16, vec![1, 0, 0, 0]), (32, vec![2, 0, 0, 0]), (48, vec![2, 0, 0, 0])]);
        assert_eq!(run(Set::C99, "4 9", "%2$d %1$d").2, vec![(32, vec![4, 0, 0, 0]), (16, vec![9, 0, 0, 0])]);
        assert_eq!(run(Set::C99, "", "%hhn %d").0, Outcome::InputFailure);
        assert_eq!(run(Set::C99, "5", "%*d %d").0, Outcome::Assigned(0));
    }
}
