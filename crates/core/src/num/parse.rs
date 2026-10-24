//! Text to number (spec §8.1–§8.3; decisions D-8.1–D-8.3). Generic over the code unit, so the
//! wide forms are the same functions.
//!
//! Results are typed outcomes, not sentinels: the boundary turns them into `endptr`, `ERANGE`
//! and the C clamping rules.

#![allow(clippy::as_conversions, clippy::arithmetic_side_effects)] // digit arithmetic on bounded values

use crate::bytes::Unit;
use crate::num::bigint::Big;
use crate::num::float::{round, Float, Format, Rounded, Rounding, Value};

/// Which revision of the C grammar base 0 follows (D-8.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Dialect {
    /// C99–C17: `0x` hexadecimal, leading `0` octal.
    #[default]
    C99,
    /// C23 (`__isoc23_*`): also `0b` binary.
    C23,
}

/// What an integer parse found.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Status {
    /// A value in range.
    Ok,
    /// No digits: nothing consumed.
    NoDigits,
    /// Out of range: the value is clamped.
    Overflow,
    /// The base is not 0 or 2..=36 (`EINVAL`).
    InvalidBase,
}

/// An integer parse result.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParsedInt<V> {
    /// The value (clamped on overflow, 0 when nothing was parsed).
    pub value: V,
    /// Units consumed (0 when no digits).
    pub used: usize,
    /// Outcome.
    pub status: Status,
}

/// An unsigned parse also reports a leading minus sign, which the C contract applies modulo
/// 2^64 (D-8.2, applied by `liella-legacy`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParsedUnsigned {
    /// Magnitude and outcome.
    pub parsed: ParsedInt<u64>,
    /// A `-` preceded the digits.
    pub negated: bool,
}

/// `isspace` in the `"C"` locale.
pub fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

fn byte_at<T: Unit>(s: &[T], i: usize) -> u8 {
    s.get(i).and_then(|c| c.as_byte()).unwrap_or(0)
}

fn digit_value(b: u8) -> Option<u32> {
    match b {
        b'0'..=b'9' => Some(u32::from(b - b'0')),
        b'a'..=b'z' => Some(u32::from(b - b'a') + 10),
        b'A'..=b'Z' => Some(u32::from(b - b'A') + 10),
        _ => None,
    }
}

/// Common front end: white space, sign, base prefix. Returns (negative, digits start, base).
fn prefix<T: Unit>(s: &[T], base: u32, dialect: Dialect) -> (bool, usize, u32) {
    let mut i = 0usize;
    while is_space(byte_at(s, i)) && i < s.len() {
        i += 1;
    }
    let mut neg = false;
    match byte_at(s, i) {
        b'-' => {
            neg = true;
            i += 1;
        }
        b'+' => i += 1,
        _ => {}
    }
    let is_digit_in = |b: u8, r: u32| digit_value(b).is_some_and(|d| d < r);
    let lower = |b: u8| b.to_ascii_lowercase();
    if byte_at(s, i) == b'0' {
        let next = lower(byte_at(s, i + 1));
        if (base == 0 || base == 16) && next == b'x' && is_digit_in(byte_at(s, i + 2), 16) {
            return (neg, i + 2, 16);
        }
        if dialect == Dialect::C23 && (base == 0 || base == 2) && next == b'b' && is_digit_in(byte_at(s, i + 2), 2) {
            return (neg, i + 2, 2);
        }
        if base == 0 {
            return (neg, i, 8);
        }
    }
    (neg, i, if base == 0 { 10 } else { base })
}

/// Accumulate digits of `base` from `start` into a magnitude, saturating at `limit`.
fn digits<T: Unit>(s: &[T], start: usize, base: u32, limit: u64) -> (u64, usize, bool) {
    let mut v: u64 = 0;
    let mut over = false;
    let mut i = start;
    while let Some(d) = digit_value(byte_at(s, i)).filter(|&d| d < base) {
        if i >= s.len() {
            break;
        }
        match v.checked_mul(u64::from(base)).and_then(|x| x.checked_add(u64::from(d))) {
            Some(x) if x <= limit => v = x,
            _ => over = true,
        }
        i += 1;
    }
    (v, i, over)
}

/// `strtol`/`strtoll`/`strtoimax` (all 64-bit on x86_64).
pub fn parse_i64<T: Unit>(s: &[T], base: u32, dialect: Dialect) -> ParsedInt<i64> {
    if base == 1 || base > 36 {
        return ParsedInt { value: 0, used: 0, status: Status::InvalidBase };
    }
    let (neg, start, b) = prefix(s, base, dialect);
    let limit = if neg { 1u64 << 63 } else { i64::MAX as u64 };
    let (mag, end, over) = digits(s, start, b, limit);
    if end == start {
        return ParsedInt { value: 0, used: 0, status: Status::NoDigits };
    }
    if over {
        return ParsedInt { value: if neg { i64::MIN } else { i64::MAX }, used: end, status: Status::Overflow };
    }
    let value = if neg { (mag as i64).wrapping_neg() } else { mag as i64 };
    ParsedInt { value, used: end, status: Status::Ok }
}

/// The common `strtol` shapes in one pass (STRTOIMAX): base 10 or 0, no leading white space, at
/// most 18 decimal digits (no overflow possible), or no digits at all (bash's `legal_number`
/// probes names). `head` is the start of the text, `whole` whether it is all of it. `Some((value,
/// used))` when `head` decides the result exactly as [`parse_i64`] would (`used` 0 = no digits);
/// `None` sends the call to [`parse_i64`] (white space, a `0` under base 0, other bases, long or
/// undecided digit runs).
pub fn parse_i64_short(head: &[u8], base: u32, whole: bool) -> Option<(i64, usize)> {
    if base != 10 && base != 0 {
        return None;
    }
    let (neg, start) = match head.first() {
        Some(b'-') => (true, 1),
        Some(b'+') => (false, 1),
        _ => (false, 0),
    };
    let Some(&c) = head.get(start) else {
        // The whole text is empty or a lone sign: no digits.
        return whole.then_some((0, 0));
    };
    if !c.is_ascii_digit() {
        // No digits: after a sign nothing is skipped; before one, white space is.
        if start == 0 && is_space(c) {
            return None;
        }
        return Some((0, 0));
    }
    if base == 0 && c == b'0' {
        return None;
    }
    let mut v: u64 = 0;
    let mut i = start;
    while let Some(&d) = head.get(i) {
        if !d.is_ascii_digit() {
            break;
        }
        if i - start == 18 {
            return None;
        }
        v = v * 10 + u64::from(d - b'0');
        i += 1;
    }
    if i == head.len() && !whole {
        return None;
    }
    let m = v as i64;
    Some((if neg { -m } else { m }, i))
}

/// `strtoul`/`strtoull`/`strtoumax`: the magnitude, plus whether a `-` preceded it.
pub fn parse_u64<T: Unit>(s: &[T], base: u32, dialect: Dialect) -> ParsedUnsigned {
    if base == 1 || base > 36 {
        return ParsedUnsigned { parsed: ParsedInt { value: 0, used: 0, status: Status::InvalidBase }, negated: false };
    }
    let (neg, start, b) = prefix(s, base, dialect);
    let (mag, end, over) = digits(s, start, b, u64::MAX);
    let parsed = if end == start {
        ParsedInt { value: 0, used: 0, status: Status::NoDigits }
    } else if over {
        ParsedInt { value: u64::MAX, used: end, status: Status::Overflow }
    } else {
        ParsedInt { value: mag, used: end, status: Status::Ok }
    };
    ParsedUnsigned { parsed, negated: neg && end != start }
}

/// Range outcome of a floating parse.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FloatRange {
    /// In range (possibly inexact).
    Ok,
    /// Overflow: ±HUGE_VAL or the largest finite value, per the rounding mode (`ERANGE`).
    Overflow,
    /// Underflow: a tiny inexact result, subnormal or zero (`ERANGE`).
    Underflow,
}

/// A floating parse result.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ParsedFloat {
    /// The value (+0 when nothing was parsed).
    pub value: Float,
    /// Units consumed (0 when no conversion).
    pub used: usize,
    /// Range outcome.
    pub range: FloatRange,
}

/// Significant decimal digits kept exactly; later nonzero digits only set the sticky bit. Large
/// enough that no rounding boundary of `fmt` can fall strictly between the kept prefix and the
/// prefix plus one unit in its last place (the longest exact decimal expansion of a midpoint is
/// about `(prec + 1 - emin + prec) * log10(5)` significant digits).
fn kept_digits(fmt: Format) -> usize {
    match fmt.prec {
        0..=24 => 160,
        25..=53 => 820,
        _ => 12_000,
    }
}

const MAX_KEPT: usize = 12_000;

fn result(r: Rounded, used: usize) -> ParsedFloat {
    let range = if r.overflow {
        FloatRange::Overflow
    } else if r.underflow {
        FloatRange::Underflow
    } else {
        FloatRange::Ok
    };
    ParsedFloat { value: r.value, used, range }
}

fn eq_ignore_case<T: Unit>(s: &[T], at: usize, word: &[u8]) -> bool {
    word.iter().enumerate().all(|(k, &w)| byte_at(s, at + k).to_ascii_lowercase() == w) && at + word.len() <= s.len()
}

/// Whether the radix `radix` (one or more units) starts at `i`.
fn radix_at<T: Unit>(s: &[T], i: usize, radix: &[T]) -> bool {
    !radix.is_empty() && s.get(i..).is_some_and(|r| r.starts_with(radix))
}

/// `strtod`-family parsing into `fmt`, correctly rounded in `mode` (spec §8.2). `radix` is the
/// locale's radix character (D-8.3), in the string's units: the multibyte decimal point for
/// byte strings (several bytes in some locales), the wide one for wide strings.
pub fn parse_float<T: Unit>(s: &[T], radix: &[T], fmt: Format, mode: Rounding, dialect: Dialect) -> ParsedFloat {
    let none = ParsedFloat { value: Float { neg: false, value: Value::Zero }, used: 0, range: FloatRange::Ok };
    let mut i = 0usize;
    while i < s.len() && is_space(byte_at(s, i)) {
        i += 1;
    }
    let mut neg = false;
    match byte_at(s, i) {
        b'-' => {
            neg = true;
            i += 1;
        }
        b'+' => i += 1,
        _ => {}
    }
    // Infinity and NaN.
    if eq_ignore_case(s, i, b"inf") {
        let used = if eq_ignore_case(s, i, b"infinity") { i + 8 } else { i + 3 };
        return ParsedFloat { value: Float { neg, value: Value::Inf }, used, range: FloatRange::Ok };
    }
    if eq_ignore_case(s, i, b"nan") {
        let mut used = i + 3;
        let mut payload = 0u128;
        if byte_at(s, used) == b'(' && used < s.len() {
            let start = used + 1;
            let mut j = start;
            while j < s.len() && (byte_at(s, j).is_ascii_alphanumeric() || byte_at(s, j) == b'_') {
                j += 1;
            }
            if byte_at(s, j) == b')' && j < s.len() {
                let seq = s.get(start..j).unwrap_or(&[]);
                let p = parse_u64(seq, 0, dialect);
                if p.parsed.used == seq.len() && p.parsed.status == Status::Ok {
                    payload = u128::from(p.parsed.value);
                }
                used = j + 1;
            }
        }
        return ParsedFloat { value: Float { neg, value: Value::Nan { payload, quiet: true } }, used, range: FloatRange::Ok };
    }
    // Hexadecimal.
    if byte_at(s, i) == b'0' && byte_at(s, i + 1).eq_ignore_ascii_case(&b'x') {
        let hex_start = i + 2;
        let has_digit = digit_value(byte_at(s, hex_start)).is_some_and(|d| d < 16) && hex_start < s.len()
            || (radix_at(s, hex_start, radix) && digit_value(byte_at(s, hex_start + radix.len())).is_some_and(|d| d < 16) && hex_start + radix.len() < s.len());
        if has_digit {
            return parse_hex(s, hex_start, neg, radix, fmt, mode);
        }
        // "0x" without digits: the "0" is the number.
        let r = round(neg, 0, 0, false, fmt, mode);
        return result(r, i + 1);
    }
    parse_decimal(s, i, neg, radix, fmt, mode).unwrap_or(none)
}

/// The common short `strtod` input in ONE pass, as a `double` and the units consumed: a decimal
/// number with at most 19 digits in all (`"938033"`, `" -42"`, `"3.14159"`, `"1.5e3"`), whose
/// value is `v × 10^e10` with both factors exact (WL-HOT-SEQ: gawk converts every field this way;
/// STRTOD-FAST: one scan serves integers and fractions, so the general parser is not preceded by a
/// failed pre-scan). `None` (nothing consumed that matters) sends the input to [`parse_float`]:
/// hexadecimal, `inf`/`nan`, no digits, more than 19 digits, `|e10| > 22`, or a value that would
/// need rounding outside `Rounding::Nearest`. The answer equals `parse_float`'s for `Format::F64`:
/// - `e10 == 0` (an integer): `v as f64` is exact for `v <= 2^53` in every mode, and correctly
///   rounded to nearest (ties to even) above it, so larger integers are taken in `Nearest` only;
/// - otherwise (`Nearest` only, `v <= 2^53`, `|e10| <= 22`): one IEEE multiplication or division of
///   two exact operands is correctly rounded (the classic exact fast path, as in `exact_f64`);
/// - neither overflows nor underflows, so `ERANGE` never applies; zero keeps its sign.
///
/// The number ends where `parse_float` ends it: the mantissa's digits with at most one radix
/// `radix` (the locale's, consumed even with no digit after it), then an exponent only when
/// `e`/`E` is followed by an optional sign and at least one digit.
pub fn parse_f64_short<T: Unit>(s: &[T], radix: &[T], mode: Rounding) -> Option<(f64, usize)> {
    // `byte_at` reads 0 past the end and for a unit that is not a byte: neither is a space, sign,
    // digit or exponent marker, so the scans below stop there without separate length checks.
    let mut i = 0usize;
    while is_space(byte_at(s, i)) {
        i += 1;
    }
    let neg = byte_at(s, i) == b'-';
    if neg || byte_at(s, i) == b'+' {
        i += 1;
    }
    // "0x"/"0X" starts a hexadecimal number.
    if byte_at(s, i) == b'0' && byte_at(s, i + 1) | 0x20 == b'x' {
        return None;
    }
    let mut v = 0u64;
    let mut ndig = 0usize;
    let mut e10 = 0i64;
    loop {
        let d = byte_at(s, i).wrapping_sub(b'0');
        if d > 9 {
            break;
        }
        if ndig == SMALL {
            return None;
        }
        v = v * 10 + u64::from(d);
        ndig += 1;
        i += 1;
    }
    if radix_at(s, i, radix) {
        i += radix.len();
        loop {
            let d = byte_at(s, i).wrapping_sub(b'0');
            if d > 9 {
                break;
            }
            if ndig == SMALL {
                return None;
            }
            v = v * 10 + u64::from(d);
            ndig += 1;
            e10 -= 1;
            i += 1;
        }
    }
    if ndig == 0 {
        return None;
    }
    let mut used = i;
    if byte_at(s, i) | 0x20 == b'e' {
        if let Some((e, end)) = exponent(s, i + 1) {
            e10 += e;
            used = end;
        }
    }
    let x = if e10 == 0 && (v <= 1 << 53 || mode == Rounding::Nearest) {
        v as f64
    } else {
        if mode != Rounding::Nearest || v > 1 << 53 {
            return None;
        }
        let p = *EXACT_POW10.get(usize::try_from(e10.unsigned_abs()).ok()?)?;
        if e10 > 0 {
            v as f64 * p
        } else {
            v as f64 / p
        }
    };
    Some((if neg { -x } else { x }, used))
}

fn parse_hex<T: Unit>(s: &[T], start: usize, neg: bool, radix: &[T], fmt: Format, mode: Rounding) -> ParsedFloat {
    let mut q: u128 = 0;
    let mut exp: i64 = 0;
    let mut sticky = false;
    let mut i = start;
    let mut seen_radix = false;
    loop {
        let b = byte_at(s, i);
        if i >= s.len() {
            break;
        }
        if !seen_radix && radix_at(s, i, radix) {
            seen_radix = true;
            i += radix.len();
            continue;
        }
        let Some(d) = digit_value(b).filter(|&d| d < 16) else { break };
        if q >> 120 == 0 {
            q = (q << 4) | u128::from(d);
            if seen_radix {
                exp -= 4;
            }
        } else {
            sticky |= d != 0;
            if !seen_radix {
                exp += 4;
            }
        }
        i += 1;
    }
    let mut used = i;
    // Optional binary exponent.
    if byte_at(s, i).eq_ignore_ascii_case(&b'p') && i < s.len() {
        if let Some((e, end)) = exponent(s, i + 1) {
            exp = exp.saturating_add(e);
            used = end;
        }
    }
    let exp = exp.clamp(-1_000_000, 1_000_000) as i32;
    result(round(neg, q, exp, sticky, fmt, mode), used)
}

/// A decimal exponent's digits after the marker: (value clamped to ±10^9, end).
fn exponent<T: Unit>(s: &[T], mut i: usize) -> Option<(i64, usize)> {
    let mut eneg = false;
    match byte_at(s, i) {
        b'-' => {
            eneg = true;
            i += 1;
        }
        b'+' => i += 1,
        _ => {}
    }
    let start = i;
    let mut e: i64 = 0;
    while i < s.len() && byte_at(s, i).is_ascii_digit() {
        e = (e * 10 + i64::from(byte_at(s, i) - b'0')).min(1_000_000_000);
        i += 1;
    }
    (i > start).then_some((if eneg { -e } else { e }, i))
}

/// The mantissa of a decimal number from `start`: whether it has a digit, where it ends, and
/// `dexp`, such that the value is 0.d1d2... × 10^dexp once the first nonzero digit is seen.
/// `keep` receives each significant digit with its index (leading zeros are not significant).
struct Mantissa {
    any_digit: bool,
    end: usize,
    dexp: i64,
    significant: usize,
}

fn scan_mantissa<T: Unit>(s: &[T], start: usize, radix: &[T], mut keep: impl FnMut(usize, u8)) -> Mantissa {
    let mut m = Mantissa { any_digit: false, end: start, dexp: 0, significant: 0 };
    let mut seen_radix = false;
    let mut i = start;
    while i < s.len() {
        let b = byte_at(s, i);
        if !seen_radix && radix_at(s, i, radix) {
            seen_radix = true;
            i += radix.len();
            continue;
        }
        if !b.is_ascii_digit() {
            break;
        }
        m.any_digit = true;
        let d = b - b'0';
        if m.significant == 0 && d == 0 {
            if seen_radix {
                m.dexp -= 1;
            }
        } else {
            if !seen_radix {
                m.dexp += 1;
            }
            keep(m.significant, d);
            m.significant += 1;
        }
        i += 1;
    }
    m.end = i;
    m
}

/// Significant digits a `u64` always holds.
const SMALL: usize = 19;

fn parse_decimal<T: Unit>(s: &[T], start: usize, neg: bool, radix: &[T], fmt: Format, mode: Rounding) -> Option<ParsedFloat> {
    // Most inputs have at most 19 significant digits: they are read into a u64, with no digit
    // buffer (the common case; the exact rounding below is the same either way).
    let mut small = 0u64;
    let m = scan_mantissa(s, start, radix, |k, d| {
        if k < SMALL {
            small = small * 10 + u64::from(d);
        }
    });
    if !m.any_digit {
        return None;
    }
    if m.significant <= SMALL {
        let (dexp, used) = exponent_part(s, &m);
        if let Some(r) = exact_f64(small, dexp - m.significant as i64, neg, fmt, mode) {
            return Some(result(r, used));
        }
        return Some(finish(dexp, used, Big::from_u128(u128::from(small)), m.significant, false, neg, fmt, mode));
    }
    let mut kept = [0u8; MAX_KEPT];
    let limit = kept_digits(fmt);
    let mut truncated = false;
    let m = scan_mantissa(s, start, radix, |k, d| {
        if k < limit {
            if let Some(slot) = kept.get_mut(k) {
                *slot = d;
            }
        } else {
            truncated |= d != 0;
        }
    });
    let nkept = m.significant.min(limit);
    // N = the kept digits.
    let mut n = Big::zero();
    for chunk in kept.get(..nkept).unwrap_or(&[]).chunks(19) {
        let mut v = 0u64;
        for &d in chunk {
            v = v * 10 + u64::from(d);
        }
        n.mul_add_small(10u64.pow(chunk.len() as u32), v);
    }
    let (dexp, used) = exponent_part(s, &m);
    Some(finish(dexp, used, n, nkept, truncated, neg, fmt, mode))
}

/// The decimal exponent after the mantissa folded into its `dexp`, and where the number ends.
fn exponent_part<T: Unit>(s: &[T], m: &Mantissa) -> (i64, usize) {
    let i = m.end;
    if byte_at(s, i).eq_ignore_ascii_case(&b'e') && i < s.len() {
        if let Some((e, end)) = exponent(s, i + 1) {
            return (m.dexp.saturating_add(e), end);
        }
    }
    (m.dexp, i)
}

/// Powers of ten a `double` holds exactly.
const EXACT_POW10: [f64; 23] = [1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16, 1e17, 1e18, 1e19, 1e20, 1e21, 1e22];

/// `v × 10^e10` for a `double` rounded to nearest, when both operands are exact (v ≤ 2^53,
/// |e10| ≤ 22): one IEEE multiplication or division is then correctly rounded, the classic exact
/// fast path. The result is exactly representable, so `round` passes it through unchanged.
fn exact_f64(v: u64, e10: i64, neg: bool, fmt: Format, mode: Rounding) -> Option<Rounded> {
    if fmt != Format::F64 || mode != Rounding::Nearest || v > 1 << 53 || v == 0 {
        return None;
    }
    let p = *EXACT_POW10.get(usize::try_from(e10.unsigned_abs()).ok()?)?;
    let x = if e10 >= 0 { v as f64 * p } else { v as f64 / p };
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7ff) as i32;
    let frac = bits & ((1u64 << 52) - 1);
    let (mant, e2) = if exp == 0 { (frac, -1074) } else { (frac | 1u64 << 52, exp - 1075) };
    Some(round(neg, u128::from(mant), e2, false, fmt, mode))
}

/// The value `n` (the first `nkept` significant digits, `truncated` if nonzero digits were
/// dropped) × 10^(dexp − nkept), correctly rounded; the number ends at `used`.
#[allow(clippy::too_many_arguments)]
fn finish(dexp: i64, used: usize, mut n: Big, nkept: usize, truncated: bool, neg: bool, fmt: Format, mode: Rounding) -> ParsedFloat {
    if nkept == 0 {
        return result(round(neg, 0, 0, false, fmt, mode), used);
    }
    // Certain overflow and certain underflow, decided with margin from the format's range.
    let max10 = ((fmt.emax + 1) as f64 * core::f64::consts::LOG10_2) as i64 + 2;
    let min10 = ((fmt.emin() - fmt.prec as i32) as f64 * core::f64::consts::LOG10_2) as i64 - 2;
    if dexp - 1 > max10 {
        return result(round(neg, 1, fmt.emax + 2, false, fmt, mode), used);
    }
    if dexp < min10 {
        return result(round(neg, 0, 0, true, fmt, mode), used);
    }
    let e10 = dexp - nkept as i64;
    let r = if e10 >= 0 {
        n.mul_pow10(e10 as u32);
        let (top, shift, rest) = n.top128();
        round(neg, top, shift as i32, rest || truncated, fmt, mode)
    } else {
        let k = (-e10) as u32;
        let mut den = Big::from_u128(1);
        den.mul_pow5(k);
        // Align so that the quotient has prec+3 or prec+4 bits.
        let want = den.bit_len() as i64 + i64::from(fmt.prec) + 3 - n.bit_len() as i64;
        let mut exp2 = -(k as i64);
        if want >= 0 {
            n.shl(want as u32);
            exp2 -= want;
        } else {
            den.shl((-want) as u32);
            exp2 -= want;
        }
        let q = n.div_small_quotient(&den);
        let sticky = !n.is_zero() || truncated;
        round(neg, q, exp2 as i32, sticky, fmt, mode)
    };
    result(r, used)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, clippy::byte_char_slices)]
mod tests {
    use super::*;

    fn f64_of(s: &str) -> (f64, usize, FloatRange) {
        let p = parse_float(s.as_bytes(), b".", Format::F64, Rounding::Nearest, Dialect::C99);
        (f64::from_bits(p.value.pack(Format::F64) as u64), p.used, p.range)
    }

    #[test]
    fn decimal_matches_rust_parsing() {
        let samples = [
            "0", "1", "-1", "0.1", "1e23", "8.98846567431158e307", "2.2250738585072011e-308", "2.2250738585072014e-308",
            "4.9406564584124654e-324", "2.4703282292062327e-324", "2.4703282292062328e-324", "1.7976931348623157e308",
            "1.7976931348623158e308", "9007199254740993", "123456789012345678901234567890", "0.000000000000000000000000001",
            "3.14159265358979323846264338327950288419716939937510", "1e-400", "1e400", ".5", "5.", "00000.000001e6",
        ];
        for t in samples {
            let want: f64 = t.parse().unwrap();
            let (got, used, _) = f64_of(t);
            assert_eq!(got.to_bits(), want.to_bits(), "{t}");
            assert_eq!(used, t.len(), "{t}");
        }
    }

    /// The exact fast path (at most 2^53, |exponent| ≤ 22) and the short-input path (at most 19
    /// digits) agree with Rust's correctly rounded parser, across and beyond their limits, and
    /// other rounding modes still take the exact path.
    #[test]
    fn fast_paths_match_rust_parsing() {
        let mut x: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..200_000 {
            let digits = 1 + next() % 20;
            let v = next() % 10u64.pow(digits.min(19) as u32);
            let e = (next() % 61) as i64 - 30;
            let t = format!("{v}e{e}");
            let want: f64 = t.parse().unwrap();
            let (got, used, _) = f64_of(&t);
            assert_eq!(got.to_bits(), want.to_bits(), "{t}");
            assert_eq!(used, t.len(), "{t}");
            let frac = format!("{}.{:0>5}", v / 100_000, v % 100_000);
            let want: f64 = frac.parse().unwrap();
            assert_eq!(f64_of(&frac).0.to_bits(), want.to_bits(), "{frac}");
        }
        // 0.3's nearest double is below 0.3, so rounding up differs: the fast path must not apply.
        let up = parse_float(b"0.3", b".", Format::F64, Rounding::Up, Dialect::C99);
        let near = parse_float(b"0.3", b".", Format::F64, Rounding::Nearest, Dialect::C99);
        assert_ne!(up.value, near.value, "Up is not Nearest for 0.3");
    }

    #[test]
    fn special_forms_and_prefixes() {
        assert_eq!(f64_of("  -inFinity!").1, 11);
        assert!(f64_of("nan(123)").0.is_nan());
        assert_eq!(f64_of("nan(12").1, 3, "an unterminated payload is not consumed");
        assert_eq!(f64_of("0x1p-1074").0, 5e-324);
        assert_eq!(f64_of("0x1.8p1").0, 3.0);
        assert_eq!(f64_of("0xg").1, 1, "0x without digits parses the 0");
        assert_eq!(f64_of("1e").1, 1, "an exponent without digits is not consumed");
        assert_eq!(f64_of("abc").1, 0);
        assert_eq!(f64_of("1e400").2, FloatRange::Overflow);
        assert_eq!(f64_of("1e-400").2, FloatRange::Underflow);
        assert_eq!(f64_of("4.9406564584124654e-324").2, FloatRange::Underflow);
    }

    #[test]
    fn integers() {
        let p = parse_i64(b"  -0x1F z".as_slice(), 0, Dialect::C99);
        assert_eq!((p.value, p.used, p.status), (-31, 7, Status::Ok));
        let p = parse_i64(b"0b101".as_slice(), 0, Dialect::C99);
        assert_eq!((p.value, p.used), (0, 1), "C99: 0b is not a prefix");
        let p = parse_i64(b"0b101".as_slice(), 0, Dialect::C23);
        assert_eq!((p.value, p.used), (5, 5));
        let p = parse_i64(b"9223372036854775808".as_slice(), 10, Dialect::C99);
        assert_eq!((p.value, p.status), (i64::MAX, Status::Overflow));
        let p = parse_i64(b"-9223372036854775808".as_slice(), 10, Dialect::C99);
        assert_eq!((p.value, p.status), (i64::MIN, Status::Ok));
        let u = parse_u64(b"-1".as_slice(), 10, Dialect::C99);
        assert_eq!((u.parsed.value, u.negated), (1, true));
        assert_eq!(parse_i64(b"+".as_slice(), 10, Dialect::C99).status, Status::NoDigits);
        assert_eq!(parse_i64(b"1".as_slice(), 37, Dialect::C99).status, Status::InvalidBase);
        assert_eq!(parse_i64(b"zz".as_slice(), 36, Dialect::C99).value, 35 * 36 + 35);
    }

    const MODES: [Rounding; 4] = [Rounding::Nearest, Rounding::Down, Rounding::Up, Rounding::Zero];

    /// Whether the short path's answer for `s` (if any) is parse_float's: value bits, length, and
    /// no ERANGE. Returns whether the short path answered.
    fn short_agrees(s: &[u8], radix: &[u8], mode: Rounding) -> bool {
        let Some(got) = parse_f64_short(s, radix, mode) else { return false };
        let p = parse_float(s, radix, Format::F64, mode, Dialect::C99);
        let want = (p.value.pack(Format::F64) as u64, p.used);
        assert_eq!((got.0.to_bits(), got.1), want, "{:?} radix {:?} {mode:?}", String::from_utf8_lossy(s), String::from_utf8_lossy(radix));
        assert_eq!(p.range, FloatRange::Ok, "{:?}", String::from_utf8_lossy(s));
        true
    }

    #[test]
    fn short_float_path_agrees_with_parse_float() {
        // WL-HOT-SEQ / STRTOD-FAST: whenever the one-pass path answers, it gives parse_float's value
        // (bits) and length in every rounding mode; what it must leave to parse_float returns None.
        let every_mode = ["938033", " \t-42", "+7", "-0", "0", "007", "9007199254740992", "123 456", "12\n", "5,5", "77abc", "12e0", "1.5e1", "1e", "1e+", "2E-x", "00x5", "5.", "5..5"];
        for s in every_mode {
            for mode in MODES {
                assert!(short_agrees(s.as_bytes(), b".", mode), "{s:?} {mode:?} not taken");
            }
        }
        let nearest_only = ["3.14159", "-2.5e-3", ".5", "+.25", "12.5", "12e3", "12E3", "9007199254740993", "9999999999999999999", "1234567890123456789", "1e22", "1e-22", "0.000", "-0.0", "-0e-5", "4.35", "0.1"];
        for s in nearest_only {
            assert!(short_agrees(s.as_bytes(), b".", Rounding::Nearest), "{s:?} not taken");
            for mode in &MODES[1..] {
                // Taken outside Nearest only when exact (then it must agree; checked inside).
                short_agrees(s.as_bytes(), b".", *mode);
            }
        }
        assert_eq!(parse_f64_short(b"0.1".as_slice(), b".", Rounding::Down), None, "needs rounding");
        assert_eq!(parse_f64_short(b"9007199254740993".as_slice(), b".", Rounding::Up), None, "needs rounding");
        let leave = ["12345678901234567890", "1.2345678901234567890", "0x1A", "0X1p3", "1e23", "1e-23", "", "-", " +", ".", "-.e5", "inf", "nan", "e5", "1e999999999999"];
        for s in leave {
            for mode in MODES {
                assert_eq!(parse_f64_short(s.as_bytes(), b".", mode), None, "{s:?} {mode:?}");
            }
        }
        // The locale's radix: "5,5" with radix "," is 5.5, with "." it is 5; a multibyte radix.
        assert_eq!(parse_f64_short(b"5,5".as_slice(), b",", Rounding::Nearest), Some((5.5, 3)));
        assert_eq!(parse_f64_short(b"5,5".as_slice(), b".", Rounding::Nearest), Some((5.0, 1)));
        assert_eq!(parse_f64_short(b"5.5".as_slice(), b",", Rounding::Nearest), Some((5.0, 1)));
        assert!(short_agrees("1\u{66b}25".as_bytes(), "\u{66b}".as_bytes(), Rounding::Nearest));
        assert!(short_agrees("1\u{66b}".as_bytes(), "\u{66b}".as_bytes(), Rounding::Nearest));
        assert!(short_agrees(b"5,5", b",", Rounding::Nearest));
    }

    #[test]
    fn short_float_path_agrees_on_generated_inputs() {
        // Deterministic random strings over the characters that steer the scan; every answer of
        // the short path is checked against parse_float in all four modes.
        let alphabet = b"0123456789000999..,eE+- \txX";
        let mut state = 0x9E37_79B9_7F4A_7C15u64;
        let mut taken = 0usize;
        for _ in 0..60_000 {
            state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let len = (state >> 59) as usize + 1;
            let mut s = [0u8; 32];
            let mut r = state;
            for slot in s.iter_mut().take(len) {
                r = r.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                *slot = alphabet[(r >> 33) as usize % alphabet.len()];
            }
            for mode in MODES {
                taken += usize::from(short_agrees(&s[..len], b".", mode));
                taken += usize::from(short_agrees(&s[..len], b",", mode));
            }
        }
        assert!(taken > 50_000, "the generator must exercise the short path ({taken})");
    }
}
