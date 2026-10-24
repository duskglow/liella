//! Numeric behaviours the ABI requires but the internal API rejects (spec §8; D-8.2, D-8.5):
//! the unsigned-negation rule of `strtoul`, `atoi`'s truncation, and the `ecvt` family.
//!
//! `ecvt`/`fcvt` details the spec leaves open are fixed here (D-8.10): fewer than one requested
//! digit gives one for `ecvt`; zero yields `max(n, 1)` zero digits with `decpt = 1`; a value
//! that `fcvt` rounds to zero is treated as zero; infinities and NaNs yield `"inf"`/`"nan"` with
//! `decpt = 0`; digits beyond the destination are dropped.

#![allow(clippy::as_conversions)] // int truncation is the contract of atoi

use liella::bytes::Unit;
use liella::error::{Errno, Result};
use liella::num::first_trailing_one;
use liella::num::format::{exact, format_float, round_digits, Bounded, Case, Decimal, Sink, Spec, Style};
use liella::num::parse::{parse_f64_short,parse_float, parse_i64, parse_i64_short, parse_u64, Dialect, FloatRange, ParsedUnsigned, Status};
use liella::num::{Float, Format, Rounding, Value};

use crate::cstring::Dest;
use crate::result_slots::Buf;
use crate::string_compat::Cursor;

/// The `strtoul` value: a negated magnitude wraps modulo 2^64 (D-8.2); overflow stays
/// `ULONG_MAX` whatever the sign.
pub fn strtoul_value(p: ParsedUnsigned) -> u64 {
    if p.negated && p.parsed.status == Status::Ok { p.parsed.value.wrapping_neg() } else { p.parsed.value }
}

/// `atoi`: `(int)strtol(s, NULL, 10)` (truncating; overflow is not reported).
pub fn atoi<T: Unit>(s: &[T]) -> i32 {
    parse_i64(s, 10, Dialect::C99).value as i32
}

/// What an `ecvt`-family call produced.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cvt {
    /// Digits written (excluding the terminator, which is also written).
    pub len: usize,
    /// Position of the decimal point relative to the first digit.
    pub decpt: i32,
    /// The sign bit.
    pub neg: bool,
}

fn write_text(out: &mut [u8], text: &[u8]) -> usize {
    let cap = out.len().saturating_sub(1);
    let n = text.len().min(cap);
    if let (Some(d), Some(s)) = (out.get_mut(..n), text.get(..n)) {
        d.copy_from_slice(s);
    }
    if let Some(t) = out.get_mut(n) {
        *t = 0;
    }
    n
}

fn write_digits(out: &mut [u8], r: &Decimal, count: usize) -> usize {
    let cap = out.len().saturating_sub(1);
    let n = count.min(cap);
    for (i, slot) in out.iter_mut().take(n).enumerate() {
        *slot = b'0'.saturating_add(r.digit(i));
    }
    if let Some(t) = out.get_mut(n) {
        *t = 0;
    }
    n
}

fn zero_digits(out: &mut [u8], n: usize) -> usize {
    let cap = out.len().saturating_sub(1);
    let k = n.max(1).min(cap);
    for slot in out.iter_mut().take(k) {
        *slot = b'0';
    }
    if let Some(t) = out.get_mut(k) {
        *t = 0;
    }
    k
}

fn special(x: Float, out: &mut [u8]) -> Option<Cvt> {
    match x.value {
        Value::Inf => Some(Cvt { len: write_text(out, b"inf"), decpt: 0, neg: x.neg }),
        Value::Nan { .. } => Some(Cvt { len: write_text(out, b"nan"), decpt: 0, neg: x.neg }),
        _ => None,
    }
}

/// `ecvt`/`qecvt`/`ecvt_r`: `ndigit` significant digits, correctly rounded in `mode`.
pub fn ecvt(x: Float, ndigit: i32, mode: Rounding, out: &mut [u8]) -> Cvt {
    if let Some(c) = special(x, out) {
        return c;
    }
    let n = usize::try_from(ndigit.max(1)).unwrap_or(1);
    match x.value {
        Value::Finite { mant, exp } => {
            let r = round_digits(&exact(mant, exp), n as i64, x.neg, mode);
            Cvt { len: write_digits(out, &r, n), decpt: r.exp10, neg: x.neg }
        }
        _ => Cvt { len: zero_digits(out, n), decpt: 1, neg: x.neg },
    }
}

/// `fcvt`/`qfcvt`/`fcvt_r`: rounded to `ndigit` places after the point (negative values round
/// to the left of it); the digits run from the first significant one to that place.
pub fn fcvt(x: Float, ndigit: i32, mode: Rounding, out: &mut [u8]) -> Cvt {
    if let Some(c) = special(x, out) {
        return c;
    }
    let places = usize::try_from(ndigit.max(0)).unwrap_or(0);
    if let Value::Finite { mant, exp } = x.value {
        let d = exact(mant, exp);
        let r = round_digits(&d, i64::from(d.exp10).saturating_add(i64::from(ndigit)), x.neg, mode);
        if !r.is_zero() {
            let count = usize::try_from(i64::from(r.exp10).saturating_add(i64::from(ndigit))).unwrap_or(0);
            return Cvt { len: write_digits(out, &r, count), decpt: r.exp10, neg: x.neg };
        }
    }
    Cvt { len: zero_digits(out, places), decpt: 1, neg: x.neg }
}

/// `gcvt`/`qgcvt`: `printf("%.*g", ndigit, x)` (D-8.5: the same engine).
pub fn gcvt(out: &mut dyn Sink, x: Float, fmt: Format, ndigit: i32, mode: Rounding) {
    let p = usize::try_from(ndigit.max(0)).unwrap_or(0);
    format_float(out, x, fmt, Spec { style: Style::General, case: Case::Lower, precision: Some(p), alternate: false }, b".", mode);
}

// ------------------------------------------------------------------------------ C shapes (§8)

/// The rounding mode `fegetround` reports, from MXCSR.RC (the x87 control word is kept in step
/// by `fesetround`).
// ARCH(x86_64): decodes the MXCSR rounding field; FPCR.RMode on aarch64 (A-9)
pub fn rounding(mxcsr: u32) -> Rounding {
    match (mxcsr >> 13) & 3 {
        0 => Rounding::Nearest,
        1 => Rounding::Down,
        2 => Rounding::Up,
        _ => Rounding::Zero,
    }
}

/// Store the end position (`*endptr`) `used` units past the start at `addr`.
fn end_at<T>(end: Option<impl Cursor>, addr: usize, used: usize) {
    if let Some(mut e) = end {
        e.set(addr.saturating_add(used.saturating_mul(core::mem::size_of::<T>())));
    }
}

/// `strtol` and its siblings: the value and the `errno` to report. No digits: 0 and the end at
/// the start; overflow: the clamped value and `ERANGE`; an invalid base: 0 and `EINVAL`, the
/// end untouched.
pub fn strtol<T: Unit>(s: &[T], addr: usize, end: Option<impl Cursor>, base: i32, dialect: Dialect) -> (i64, Option<Errno>) {
    let Ok(b) = u32::try_from(base) else { return (0, Some(Errno::EINVAL)) };
    let p = parse_i64(s, b, dialect);
    match p.status {
        Status::InvalidBase => (0, Some(Errno::EINVAL)),
        Status::NoDigits => {
            end_at::<T>(end, addr, 0);
            (0, None)
        }
        Status::Overflow => {
            end_at::<T>(end, addr, p.used);
            (p.value, Some(Errno::ERANGE))
        }
        Status::Ok => {
            end_at::<T>(end, addr, p.used);
            (p.value, None)
        }
    }
}

/// [`strtol`] on narrow text when the first bytes decide it (STRTOIMAX): `head` is the start of
/// the text at `addr`, `whole` whether it is all of it. `None`: call [`strtol`] on the whole text.
/// The common shapes never set `errno`.
pub fn strtol_short(head: &[u8], addr: usize, end: Option<impl Cursor>, base: i32, whole: bool) -> Option<i64> {
    let (v, used) = parse_i64_short(head, u32::try_from(base).ok()?, whole)?;
    end_at::<u8>(end, addr, used);
    Some(v)
}

/// `strtoul` and its siblings, as [`strtol`]; a negated magnitude wraps (D-8.2).
pub fn strtoul<T: Unit>(s: &[T], addr: usize, end: Option<impl Cursor>, base: i32, dialect: Dialect) -> (u64, Option<Errno>) {
    let Ok(b) = u32::try_from(base) else { return (0, Some(Errno::EINVAL)) };
    let p = parse_u64(s, b, dialect);
    match p.parsed.status {
        Status::InvalidBase => (0, Some(Errno::EINVAL)),
        Status::NoDigits => {
            end_at::<T>(end, addr, 0);
            (0, None)
        }
        Status::Overflow => {
            end_at::<T>(end, addr, p.parsed.used);
            (u64::MAX, Some(Errno::ERANGE))
        }
        Status::Ok => {
            end_at::<T>(end, addr, p.parsed.used);
            (strtoul_value(p), None)
        }
    }
}

/// `atol`/`atoll`: `strtol(s, NULL, 10)` without error reporting.
pub fn atol<T: Unit>(s: &[T]) -> i64 {
    parse_i64(s, 10, Dialect::C99).value
}

/// The `strtod` family in format `fmt` with the locale's radix `radix` (D-8.3): the value's
/// bits and the `errno` to report (`ERANGE` on overflow and underflow).
pub fn strto_float<T: Unit>(s: &[T], addr: usize, end: Option<impl Cursor>, fmt: Format, mode: Rounding, radix: &[T]) -> (u128, Option<Errno>) {
    let p = parse_float(s, radix, fmt, mode, Dialect::C99);
    end_at::<T>(end, addr, p.used);
    (p.value.pack(fmt), (p.range != FloatRange::Ok).then_some(Errno::ERANGE))
}

/// `strtof`.
pub fn strtof<T: Unit>(s: &[T], addr: usize, end: Option<impl Cursor>, mode: Rounding, radix: &[T]) -> (f32, Option<Errno>) {
    let (b, e) = strto_float(s, addr, end, Format::F32, mode, radix);
    (f32::from_bits(b as u32), e)
}

/// `strtod`.
pub fn strtod<T: Unit>(s: &[T], addr: usize, end: Option<impl Cursor>, mode: Rounding, radix: &[T]) -> (f64, Option<Errno>) {
    // Short decimals first, in one pass: exact or one correctly rounded IEEE operation, no ERANGE
    // (WL-HOT-SEQ, STRTOD-FAST).
    if let Some((x, used)) = parse_f64_short(s, radix, mode) {
        end_at::<T>(end, addr, used);
        return (x, None);
    }
    let (b, e) = strto_float(s, addr, end, Format::F64, mode, radix);
    (f64::from_bits(b as u64), e)
}

/// `atof`: `strtod(s, NULL)`.
pub fn atof<T: Unit>(s: &[T], mode: Rounding, radix: &[T]) -> f64 {
    if let Some((x, _)) = parse_f64_short(s, radix, mode) {
        return x;
    }
    f64::from_bits(parse_float(s, radix, Format::F64, mode, Dialect::C99).value.pack(Format::F64) as u64)
}

/// A `float` argument as a value.
pub fn from_f32(x: f32) -> Float {
    Float::unpack(u128::from(x.to_bits()), Format::F32)
}

/// A `double` argument as a value.
pub fn from_f64(x: f64) -> Float {
    Float::unpack(u128::from(x.to_bits()), Format::F64)
}

/// The most digits `ecvt` and `fcvt` give for a `double` (`ndigit` is reduced to it: ecvt(3),
/// "a system-specific limit determined by the precision of a double"; observed 17).
pub const DOUBLE_DIGITS: i32 = 17;
/// The same for the `long double` forms (`qecvt`, `qfcvt` and their `_r` forms; observed 21).
pub const LONG_DOUBLE_DIGITS: i32 = 21;

/// Run an `ecvt`/`fcvt` conversion into a local buffer of `N` bytes, report the decimal point
/// and sign, and copy the digits into the process-wide result buffer `slot` (whose address is
/// returned). The caller reduces `ndigit` to [`DOUBLE_DIGITS`] or [`LONG_DOUBLE_DIGITS`].
pub fn cvt_static<const N: usize>(slot: &Buf<N>, x: Float, ndigit: i32, mode: Rounding, decpt: &mut i32, sign: &mut i32, fixed: bool) -> usize {
    let mut buf = [0u8; N];
    let c = if fixed { fcvt(x, ndigit, mode, &mut buf) } else { ecvt(x, ndigit, mode, &mut buf) };
    *decpt = c.decpt;
    *sign = i32::from(c.neg);
    slot.store(buf.get(..=c.len).unwrap_or(&buf));
    slot.addr()
}

/// `ecvt_r`/`fcvt_r` (and the `q` forms): digits into the caller's buffer; 0. `ndigit` is
/// reduced by the caller, as for [`cvt_static`].
pub fn cvt_r(x: Float, ndigit: i32, mode: Rounding, decpt: &mut i32, sign: &mut i32, out: &mut [u8], fixed: bool) -> i64 {
    let c = if fixed { fcvt(x, ndigit, mode, out) } else { ecvt(x, ndigit, mode, out) };
    *decpt = c.decpt;
    *sign = i32::from(c.neg);
    0
}

/// A [`Sink`] over a destination whose length grows with the output (`gcvt`).
struct DestSink<D> {
    d: D,
    len: usize,
    err: Option<Errno>,
}

impl<D: Dest<u8>> Sink for DestSink<D> {
    fn put(&mut self, bytes: &[u8]) {
        let end = self.len.saturating_add(bytes.len());
        match self.d.get(end) {
            Ok(out) => {
                if let Some(dst) = out.get_mut(self.len..end) {
                    dst.copy_from_slice(bytes);
                }
            }
            Err(e) => self.err = Some(e),
        }
        self.len = end;
    }
}

/// `gcvt`/`qgcvt` into a caller buffer the contract says is large enough, terminated.
pub fn gcvt_into(d: impl Dest<u8>, x: Float, fmt: Format, ndigit: i32, mode: Rounding) -> Result<()> {
    let mut s = DestSink { d, len: 0, err: None };
    gcvt(&mut s, x, fmt, ndigit, mode);
    s.put(&[0]);
    s.err.map_or(Ok(()), Err)
}

/// The conversion of a `strfrom*` format: `%[.precision]` and one of `aAeEfFgG`, nothing else
/// (spec §8.4).
pub fn strfrom_spec(f: &[u8]) -> Option<Spec> {
    let rest = f.strip_prefix(b"%")?;
    let (precision, rest) = match rest.strip_prefix(b".") {
        Some(r) => {
            let n = r.iter().take_while(|b| b.is_ascii_digit()).count();
            let digits = r.get(..n)?;
            let p = digits.iter().try_fold(0usize, |acc, &d| acc.checked_mul(10)?.checked_add(usize::from(d.wrapping_sub(b'0'))))?;
            (Some(p), r.get(n..)?)
        }
        None => (None, rest),
    };
    let (&conv, tail) = rest.split_first()?;
    if !tail.is_empty() {
        return None;
    }
    let style = match conv.to_ascii_lowercase() {
        b'a' => Style::Hex,
        b'e' => Style::Exp,
        b'f' => Style::Fixed,
        b'g' => Style::General,
        _ => return None,
    };
    let case = if conv.is_ascii_uppercase() { Case::Upper } else { Case::Lower };
    Some(Spec { style, case, precision, alternate: false })
}

/// `strfromd` and its siblings: format into at most `out.len()` bytes (terminated when not
/// empty); the full length, or `EINVAL` for a format outside the restricted form.
pub fn strfrom(out: &mut [u8], f: &[u8], x: Float, fmt: Format, mode: Rounding) -> Result<i64> {
    let spec = strfrom_spec(f).ok_or(Errno::EINVAL)?;
    let room = out.len().saturating_sub(1);
    let len = {
        let mut b = Bounded::new(out.get_mut(..room).unwrap_or(&mut []));
        format_float(&mut b, x, fmt, spec, b".", mode);
        b.len
    };
    if let Some(t) = out.get_mut(len.min(room)) {
        *t = 0;
    }
    i64::try_from(len).map_err(|_| Errno::EOVERFLOW)
}

/// `abs`: `abs(INT_MIN)` is undefined in C; it wraps here.
pub fn abs32(n: i32) -> i32 {
    n.wrapping_abs()
}

/// `labs`/`llabs`/`imaxabs`.
pub fn abs64(n: i64) -> i64 {
    n.wrapping_abs()
}

/// `div`: quotient and remainder truncated toward zero; division by zero and `INT_MIN / -1`
/// are undefined in C and refused here (the boundary aborts with a diagnostic, S6).
pub fn div32(n: i32, d: i32) -> Result<(i32, i32)> {
    Ok((n.checked_div(d).ok_or(Errno::EDOM)?, n.checked_rem(d).ok_or(Errno::EDOM)?))
}

/// `ldiv`/`lldiv`/`imaxdiv`, as [`div32`].
pub fn div64(n: i64, d: i64) -> Result<(i64, i64)> {
    liella::num::div_rem(n, d).ok_or(Errno::EDOM)
}

/// `ffs`: 1-based index of the least significant set bit of the two's-complement value.
pub fn ffs32(n: i32) -> i32 {
    first_trailing_one(u32::from_ne_bytes(n.to_ne_bytes())) as i32
}

/// `ffsl`/`ffsll`.
pub fn ffs64(n: i64) -> i32 {
    first_trailing_one(u64::from_ne_bytes(n.to_ne_bytes())) as i32
}

/// `l64a` into `slot` (the calling thread's buffer, D-8.8); returns its address.
pub fn l64a_into(slot: &Buf<7>, n: i64) -> usize {
    let mut digits = [0u8; 6];
    let len = liella::num::l64a(n, &mut digits);
    let mut out = [0u8; 7];
    if let (Some(d), Some(s)) = (out.get_mut(..len), digits.get(..len)) {
        d.copy_from_slice(s);
    }
    slot.store(out.get(..=len).unwrap_or(&out));
    slot.addr()
}
