//! Number to text (spec §8.4, §8.5; D-8.4, D-8.5, D-8.9): exact decimal expansion of binary
//! floating values, rounding at any digit position in any rounding mode, the `%e`/`%f`/`%g`/`%a`
//! styles, and integer formatting.
//!
//! Output goes to a [`Sink`], so callers decide whether bytes land in a bounded buffer, a
//! stream or a counter; the formatted length is always the full length.

#![allow(clippy::as_conversions, clippy::arithmetic_side_effects, clippy::indexing_slicing)] // digit buffers with checked lengths

use crate::num::bigint::Big;
use crate::num::float::{Float, Format, Rounding, Value};

/// Where formatted bytes go.
pub trait Sink {
    /// Append bytes.
    fn put(&mut self, bytes: &[u8]);
    /// Append `n` copies of `b`.
    fn repeat(&mut self, b: u8, n: usize) {
        let chunk = [b; 64];
        let mut left = n;
        while left > 0 {
            let k = left.min(chunk.len());
            self.put(&chunk[..k]);
            left -= k;
        }
    }
}

/// A sink writing into a bounded buffer while counting the full length (`snprintf`,
/// `strfrom*`).
pub struct Bounded<'a> {
    buf: &'a mut [u8],
    /// Bytes the full result has.
    pub len: usize,
}

impl<'a> Bounded<'a> {
    /// Write into `buf`.
    pub fn new(buf: &'a mut [u8]) -> Bounded<'a> {
        Bounded { buf, len: 0 }
    }

    /// Bytes actually stored (at most the buffer).
    pub fn stored(&self) -> usize {
        self.len.min(self.buf.len())
    }
}

impl Sink for Bounded<'_> {
    fn put(&mut self, bytes: &[u8]) {
        if let Some(room) = self.buf.get_mut(self.len..) {
            let k = room.len().min(bytes.len());
            room[..k].copy_from_slice(&bytes[..k]);
        }
        self.len = self.len.saturating_add(bytes.len());
    }
}

/// Most significant digits any exact expansion has (binary128 subnormals: about 11,500).
pub const MAX_DIGITS: usize = 12_288;

/// An exact or rounded decimal: `0.d1 d2 ... dn × 10^exp10`, digits as values 0–9, with
/// implicit zeros after `len`, in room for `N` digits ([`MAX_DIGITS`]: any binary value;
/// [`SMALL_DIGITS`]: the values [`exact_small`] takes). A truncated decimal
/// ([`exact_small_limited`]) has `sticky` set instead: the value lies strictly between the
/// stored digits and one unit more in the last stored place, so the stored digits are the
/// value's leading digits and the rest is known to be non-zero (enough for exact rounding at
/// any position before the last stored digit).
pub struct Decimal<const N: usize = MAX_DIGITS> {
    digits: [u8; N],
    len: usize,
    sticky: bool,
    /// Decimal exponent (position of the point relative to the first digit).
    pub exp10: i32,
}

impl<const N: usize> Decimal<N> {
    /// The stored digits (without implicit trailing zeros).
    pub fn digits(&self) -> &[u8] {
        self.digits.get(..self.len).unwrap_or(&[])
    }

    /// Digit `i` (0 beyond the stored ones).
    pub fn digit(&self, i: usize) -> u8 {
        if i < self.len { self.digits[i] } else { 0 }
    }

    /// True for zero.
    pub fn is_zero(&self) -> bool {
        self.len == 0
    }

    fn zero() -> Decimal<N> {
        Decimal { digits: [0; N], len: 0, sticky: false, exp10: 0 }
    }
}

/// The exact decimal expansion of `mant * 2^exp`.
///
/// D-8.11 (2): the value is split at the binary point. The integer part
/// is converted least significant chunk first by repeated division by 10^9
/// ([`Big::divrem_1e9`]); the `k`-bit fraction most significant chunk first, each
/// multiplication by 10^9 lifting the next nine digits above bit `k` ([`Big::split_above`]).
/// A `k`-bit fraction has exactly `k` decimal digits, so the loop ends; nothing is rounded.
pub fn exact(mant: u128, exp: i32) -> Decimal {
    let mut d = Decimal::zero();
    if mant == 0 {
        return d;
    }
    // Trailing zero bits only lengthen the fraction: drop them.
    let tz = mant.trailing_zeros();
    let (mant, exp) = (mant >> tz, exp.saturating_add(tz as i32));
    if exp >= 0 {
        let mut int = Big::from_u128(mant);
        int.shl(exp.unsigned_abs());
        put_integer(&mut d, &mut int);
    } else {
        let k = exp.unsigned_abs();
        let (int_part, frac_part) = if k >= 128 { (0, mant) } else { (mant >> k, mant & ((1u128 << k) - 1)) };
        if int_part != 0 {
            put_integer(&mut d, &mut Big::from_u128(int_part));
        }
        put_fraction(&mut d, &mut Big::from_u128(frac_part), k);
    }
    // Trailing zeros are implicit in an exact decimal.
    while d.len > 0 && d.digits[d.len - 1] == 0 {
        d.len -= 1;
    }
    d
}

/// The digits of the non-zero integer `int` into the empty `d` (consumed; sets `exp10`):
/// nine digits per division, written backwards from the end of the buffer, then moved to the
/// front.
fn put_integer(d: &mut Decimal, int: &mut Big) {
    let mut at = MAX_DIGITS;
    while !int.is_zero() && at >= 9 {
        let mut chunk = int.divrem_1e9();
        for slot in d.digits[at - 9..at].iter_mut().rev() {
            *slot = (chunk % 10) as u8;
            chunk /= 10;
        }
        at -= 9;
    }
    while at < MAX_DIGITS && d.digits[at] == 0 {
        at += 1;
    }
    let n = MAX_DIGITS - at;
    d.digits.copy_within(at.., 0);
    d.len = n;
    d.exp10 = n as i32;
}

/// Append the digits of `frac / 2^k` (`frac < 2^k`, consumed) after any integer digits in `d`.
/// While `d` is empty, leading zeros only lower `exp10`.
fn put_fraction(d: &mut Decimal, frac: &mut Big, k: u32) {
    while !frac.is_zero() {
        frac.mul_add_small(1_000_000_000, 0);
        let chunk = frac.split_above(k) as u32;
        if d.len == 0 && chunk == 0 {
            d.exp10 -= 9;
            continue;
        }
        let mut nine = [0u8; 9];
        let mut c = chunk;
        for slot in nine.iter_mut().rev() {
            *slot = (c % 10) as u8;
            c /= 10;
        }
        let mut from = 0;
        if d.len == 0 {
            from = nine.iter().position(|&x| x != 0).unwrap_or(0);
            d.exp10 -= from as i32;
        }
        let n = (9 - from).min(MAX_DIGITS - d.len);
        d.digits[d.len..d.len + n].copy_from_slice(&nine[from..from + n]);
        d.len += n;
        if d.len == MAX_DIGITS {
            // Beyond any binary format's expansion; keep the decimal honest if ever reached.
            d.sticky = !frac.is_zero() || nine[from + n..].iter().any(|&x| x != 0);
            break;
        }
    }
}

/// The room of the decimals [`exact_small`] makes: the integer digits of a value below 2^127
/// (39) and the at most 124 fraction digits of a value with 124 binary fraction bits.
pub const SMALL_DIGITS: usize = 176;

/// The exact decimal of `mant × 2^exp` when it fits [`SMALL_DIGITS`] cheaply: `mant` below
/// 2^64 (every `double` and x87 `long double` significand) and `exp` from -124 to 63, so that
/// the integer part is one `u128` and each fraction digit comes from one `u128`
/// multiplication (`frac × 10 < 2^128`). The digits are exact, as [`exact`]'s; this only
/// spares the 12 KiB buffer and the big arithmetic (the common `printf` arguments).
pub fn exact_small(mant: u128, exp: i32) -> Option<Decimal<SMALL_DIGITS>> {
    exact_small_limited(mant, exp, usize::MAX, usize::MAX)
}

/// [`exact_small`], but generating digits only while they can matter: it stops once at least
/// one significant digit is stored and either `sig` significant digits or `frac` digits after
/// the decimal point are generated, and marks the decimal `sticky` when non-zero digits
/// follow. The stored digits are then exactly the value's leading digits (never rounded), so
/// [`round_digits`] stays exact for any `keep` below the stored length (D-8.9; P9).
pub fn exact_small_limited(mant: u128, exp: i32, sig: usize, frac: usize) -> Option<Decimal<SMALL_DIGITS>> {
    let mut d = Decimal::<SMALL_DIGITS>::zero();
    exact_small_into(mant, exp, sig, frac, &mut d).then_some(d)
}

/// [`exact_small_limited`] built in `d` (which must be [`Decimal::zero`]): false when the value
/// is out of the small range. No result is moved (a move of the 200-byte decimal is a `memcpy`
/// call, about 150 instructions through the library's own copy).
pub fn exact_small_into(mant: u128, exp: i32, sig: usize, frac: usize, d: &mut Decimal<SMALL_DIGITS>) -> bool {
    if mant == 0 {
        return true;
    }
    if mant >> 64 != 0 || !(-124..=63).contains(&exp) {
        return false;
    }
    let frac_limit = frac;
    let sig = sig.max(1);
    let (int_part, mut frac, sh) = if exp >= 0 { (mant << exp, 0u128, 0u32) } else {
        let sh = (-exp) as u32;
        (mant >> sh, mant & ((1u128 << sh) - 1), sh)
    };
    let mut tmp = [0u8; 40];
    let mut k = 0usize;
    if int_part >> 64 == 0 {
        // The common case: 64-bit division (a u128 one is a library call per digit).
        let mut v = int_part as u64;
        while v != 0 {
            tmp[k] = (v % 10) as u8;
            v /= 10;
            k += 1;
        }
    } else {
        let mut v = int_part;
        while v != 0 {
            tmp[k] = (v % 10) as u8;
            v /= 10;
            k += 1;
        }
    }
    for i in (0..k).rev() {
        d.digits[d.len] = tmp[i];
        d.len += 1;
    }
    d.exp10 = k as i32;
    if d.len > sig {
        // More integer digits than needed: keep `sig`, the rest only as the sticky bit.
        d.sticky = d.digits[sig..d.len].iter().any(|&x| x != 0) || frac != 0;
        d.len = sig;
        frac = 0;
    }
    let mask = if sh == 0 { 0 } else { (1u128 << sh) - 1 };
    let mut nfrac = 0usize;
    // Leading zeros after the point (no significant digit yet): one digit at a time.
    while d.len == 0 && frac != 0 {
        frac *= 10;
        let digit = (frac >> sh) as u8;
        frac &= mask;
        nfrac += 1;
        if digit == 0 {
            d.exp10 -= 1;
        } else {
            d.digits[0] = digit;
            d.len = 1;
        }
    }
    // The rest in chunks: `frac × 10^c` still fits a `u128` (frac < 2^sh), and the integer part
    // of the product is the next `c` digits at once (one multiplication per chunk, not per digit).
    const POW10: [u128; 10] = [1, 10, 100, 1000, 10_000, 100_000, 1_000_000, 10_000_000, 100_000_000, 1_000_000_000];
    let chunk = if sh <= 98 { 9 } else if sh <= 114 { 4 } else { 1 };
    while frac != 0 && d.len > 0 && d.len < SMALL_DIGITS {
        if d.len >= sig || nfrac >= frac_limit {
            d.sticky = true;
            break;
        }
        let c = chunk.min(sig - d.len).min(frac_limit - nfrac).min(SMALL_DIGITS - d.len);
        frac *= POW10[c];
        let mut v = (frac >> sh) as u32;
        frac &= mask;
        nfrac += c;
        for i in (0..c).rev() {
            d.digits[d.len + i] = (v % 10) as u8;
            v /= 10;
        }
        d.len += c;
    }
    if !d.sticky {
        // Trailing zeros are implicit, but only in an exact decimal: a truncated one keeps
        // its stored length, which is what makes rounding before its last digit exact.
        while d.len > 0 && d.digits[d.len - 1] == 0 {
            d.len -= 1;
        }
    }
    true
}

/// Round `d` to keep `keep` digits (`keep` may be 0 or negative: everything rounds away or to
/// one unit at position `keep`). Returns the rounded decimal.
#[inline(always)]
pub fn round_digits<const N: usize>(d: &Decimal<N>, keep: i64, neg: bool, mode: Rounding) -> Decimal<N> {
    let mut r = Decimal::<N>::zero();
    round_digits_into(d, keep, neg, mode, &mut r);
    r
}

/// [`round_digits`] into `r`, which must be [`Decimal::zero`] (nothing is moved).
///
/// The discarded part is read as its first digit and whether anything after it is non-zero
/// (a stored digit or `d`'s sticky bit): the inputs [`Rounding::rounds_up`] needs. A carry past
/// the first digit leaves `1` one place higher. The result is exact (never sticky), trailing
/// zeros implicit. With `keep` past the stored digits of a sticky decimal the tail is taken as
/// below half (callers keep at least one stored digit past `keep`).
pub fn round_digits_into<const N: usize>(d: &Decimal<N>, keep: i64, neg: bool, mode: Rounding, r: &mut Decimal<N>) {
    let len = d.len;
    if len == 0 {
        return;
    }
    let (kept, next, rest) = match usize::try_from(keep) {
        Ok(k) if k < len => (k, d.digits[k], d.sticky || d.digits[k + 1..len].iter().any(|&x| x != 0)),
        Ok(_) => (len, 0, d.sticky),
        // Everything discarded, and the value is below a tenth of the unit at `keep`.
        Err(_) => (0, 0, true),
    };
    let odd = kept > 0 && i64::try_from(kept).is_ok_and(|k| k == keep) && d.digits[kept - 1] & 1 == 1;
    let up = mode.rounds_up(neg, odd, next > 5 || (next == 5 && rest), next == 5 && !rest, next != 0 || rest);
    if keep <= 0 {
        if up {
            r.digits[0] = 1;
            r.len = 1;
            r.exp10 = i32::try_from(i64::from(d.exp10) - keep + 1).unwrap_or(i32::MAX);
        }
        return;
    }
    r.exp10 = d.exp10;
    r.digits[..kept].copy_from_slice(&d.digits[..kept]);
    r.len = kept;
    if up {
        // One unit at position keep - 1 (past the stored digits only for a sticky decimal).
        let mut at = usize::try_from(keep).unwrap_or(N).min(N);
        r.digits[kept..at].fill(0);
        r.len = at;
        loop {
            if at == 0 {
                r.digits[0] = 1;
                r.len = 1;
                r.exp10 += 1;
                break;
            }
            at -= 1;
            if r.digits[at] == 9 {
                r.digits[at] = 0;
            } else {
                r.digits[at] += 1;
                break;
            }
        }
    }
    while r.len > 0 && r.digits[r.len - 1] == 0 {
        r.len -= 1;
    }
}

/// Case of letters in the output.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Case {
    /// `e`, `inf`, `0x`.
    Lower,
    /// `E`, `INF`, `0X`.
    Upper,
}

/// A floating conversion request (the `printf` subset `strfrom*` accepts, plus the flags
/// `printf` will need).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Spec {
    /// `e`, `f`, `g` or `a`.
    pub style: Style,
    /// Letter case.
    pub case: Case,
    /// Precision (`None`: the default, 6, or exact for `a`).
    pub precision: Option<usize>,
    /// `#`: keep the radix and (for `g`) trailing zeros.
    pub alternate: bool,
}

/// The four floating styles.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    /// `%e`.
    Exp,
    /// `%f`.
    Fixed,
    /// `%g`.
    General,
    /// `%a`.
    Hex,
}

/// Format `x` (a value of `fmt`) per `spec` into `out` (without sign padding or width; the sign
/// is written when negative, including `-nan` and `-0`). `radix` is the decimal point: the
/// locale's, which may be several bytes.
pub fn format_float(out: &mut dyn Sink, x: Float, fmt: Format, spec: Spec, radix: &[u8], mode: Rounding) {
    if x.neg {
        out.put(b"-");
    }
    let upper = spec.case == Case::Upper;
    match x.value {
        Value::Inf => out.put(if upper { b"INF" } else { b"inf" }),
        Value::Nan { .. } => out.put(if upper { b"NAN" } else { b"nan" }),
        _ if spec.style == Style::Hex => format_hex(out, x, fmt, spec, radix, mode),
        _ => format_decimal(out, x, spec, radix, mode),
    }
}

/// Digits `from..to` of `d` as text (positions before the first digit or past the stored ones
/// are zeros), gathered in 64-byte stack chunks, one sink call each.
fn put_digits<const N: usize>(out: &mut dyn Sink, d: &Decimal<N>, from: i64, to: i64) {
    let mut chunk = [0u8; 64];
    let mut i = from;
    while i < to {
        let n = usize::try_from(to - i).unwrap_or(usize::MAX).min(chunk.len());
        for (j, slot) in chunk[..n].iter_mut().enumerate() {
            *slot = b'0' + usize::try_from(i + j as i64).map_or(0, |p| d.digit(p));
        }
        out.put(&chunk[..n]);
        i += n as i64;
    }
}

/// `%e`, `%f`, `%g` of a zero or finite value, without the sign: the scaled integer paths where
/// they apply (PRINTF-FLOAT, PRINTF-G), else the general decimal path.
fn format_decimal(out: &mut dyn Sink, x: Float, spec: Spec, radix: &[u8], mode: Rounding) {
    if let Value::Finite { mant, exp } = x.value {
        let p = spec.precision.unwrap_or(6);
        if spec.style == Style::Fixed && fixed_scaled(out, mant, exp, x.neg, p, spec.alternate, radix, mode) {
            return;
        }
        if spec.style == Style::General && general_scaled(out, mant, exp, x.neg, p, spec.alternate, spec.case, radix, mode) {
            return;
        }
    }
    decimal_general(out, x, spec, radix, mode);
}

/// The general decimal path (`%e`, `%f`, `%g` of any value, without the sign): the decimal
/// expansion, truncated where the style allows, then rounded and laid out by `decimal_styles`.
/// The scaled paths above must print exactly what this prints (the differential tests check it).
fn decimal_general(out: &mut dyn Sink, x: Float, spec: Spec, radix: &[u8], mode: Rounding) {
    // Digits the style can look at: rounding at `keep` reads digit `keep` and whether any
    // later one is non-zero, so one digit past the kept ones plus the sticky bit suffice.
    // %e keeps p+1 significant digits; %g keeps p (its %f branch at most p); %f keeps the
    // digits up to p after the point.
    let p = spec.precision.unwrap_or(6);
    let (sig, frac) = match spec.style {
        Style::Exp => (p.saturating_add(2), usize::MAX),
        Style::General => (p.max(1).saturating_add(1), usize::MAX),
        Style::Fixed => (usize::MAX, p.saturating_add(1)),
        Style::Hex => (usize::MAX, usize::MAX),
    };
    match x.value {
        Value::Finite { mant, exp } => {
            let mut d = Decimal::<SMALL_DIGITS>::zero();
            if exact_small_into(mant, exp, sig, frac, &mut d) {
                decimal_styles(out, &d, x.neg, spec, radix, mode);
            } else {
                decimal_styles(out, &exact(mant, exp), x.neg, spec, radix, mode);
            }
        }
        _ => decimal_styles(out, &Decimal::<SMALL_DIGITS>::zero(), x.neg, spec, radix, mode),
    }
}

/// `%.<p>f` of `mant × 2^exp` for `p` up to 19 by scaled integer arithmetic (PRINTF-FLOAT): with
/// `mant < 2^64` and `10^p < 2^64`, `mant × 10^p` is one exact `u128`, its integer part
/// `q = ⌊x × 10^p⌋` is the whole digit string and the bits shifted out decide the rounding exactly
/// (above, at or below half a unit). No decimal expansion, no digit array, one sink call. False
/// (nothing written) when the value or the rounded `q` falls outside that range; the general
/// path then formats it.
#[allow(clippy::too_many_arguments)]
fn fixed_scaled(out: &mut dyn Sink, mant: u128, exp: i32, neg: bool, p: usize, alt: bool, radix: &[u8], mode: Rounding) -> bool {
    const POW10: [u64; 20] = [
        1, 10, 100, 1_000, 10_000, 100_000, 1_000_000, 10_000_000, 100_000_000, 1_000_000_000, 10_000_000_000,
        100_000_000_000, 1_000_000_000_000, 10_000_000_000_000, 100_000_000_000_000, 1_000_000_000_000_000,
        10_000_000_000_000_000, 100_000_000_000_000_000, 1_000_000_000_000_000_000, 10_000_000_000_000_000_000,
    ];
    let Some(&scale) = POW10.get(p) else { return false };
    if mant >> 64 != 0 || radix.len() > 8 {
        return false;
    }
    let prod = mant * u128::from(scale); // < 2^64 × 2^64: exact
    let q = if exp >= 0 {
        // An integer: exact when the shift loses no bit.
        if exp > 127 || prod.leading_zeros() < exp as u32 {
            return false;
        }
        prod << exp
    } else {
        let sh = exp.unsigned_abs();
        // q, the remainder r and half a unit (2^(sh-1)) as u128; past 2^128 the value is below half.
        let (q, above, half) = match sh {
            1..=127 => {
                let r = prod & ((1u128 << sh) - 1);
                let h = 1u128 << (sh - 1);
                (prod >> sh, r > h, r == h)
            }
            128 => (0, prod > 1 << 127, prod == 1 << 127),
            _ => (0, false, false),
        };
        let inexact = q << sh.min(127) != prod || sh >= 128;
        let up = inexact && mode.rounds_up(neg, q & 1 == 1, above, half, true);
        q + u128::from(up)
    };
    let Ok(q) = u64::try_from(q) else { return false };
    // Text: integer digits (at least "0"), radix (when there are fraction digits or `#`), `p` digits.
    let (int, mut fr) = (q / scale, q % scale);
    let mut line = [0u8; 20 + 8 + 19];
    let mut tmp = [0u8; 20];
    let int_digits = u64_digits_into(&mut tmp, int);
    let mut n = int_digits.len();
    line[..n].copy_from_slice(int_digits);
    if p > 0 || alt {
        line[n..n + radix.len()].copy_from_slice(radix);
        n += radix.len();
    }
    for slot in line[n..n + p].iter_mut().rev() {
        *slot = b'0' + (fr % 10) as u8;
        fr /= 10;
    }
    n += p;
    out.put(&line[..n]);
    true
}

/// `⌊mant × 2^exp × 10^s⌋` and where the rest lies against half a unit (above, exactly half,
/// inexact), all exact in `u128`; `None` when an operand would not fit (the caller falls back).
///
/// Exactness: the value is `num / den` with `num = mant × 2^max(exp,0) × 10^max(s,0)` and
/// `den = 2^max(-exp,0) × 10^max(-s,0)`, both integers built with checked operations (any
/// overflow is `None`), so `q = num div den` and `r = num mod den` are exact and the discarded
/// tail is exactly `r / den` in `[0, 1)`: above half iff `2r > den` iff `r > den - r`, exactly half
/// iff `r = den - r`, inexact iff `r != 0`. (PRINTF-FLOAT measured two variants, both worse
/// overall on the printf %g rows: a shift-and-mask branch for power-of-two `den` - the u128
/// division is ~22 instructions here and the branch grew the inline code by 63 on the division
/// path - and a const 10^k table in place of `checked_pow`, +0.3..1.2%.)
fn scaled_quotient(mant: u128, exp: i32, s: i32) -> Option<(u128, bool, bool, bool)> {
    let pow10 = |k: u32| 10u128.checked_pow(k);
    let shl = |v: u128, k: u32| if k < 128 && v.leading_zeros() >= k { Some(v << k) } else { None };
    let mut num = mant;
    let mut den = 1u128;
    if s >= 0 {
        num = num.checked_mul(pow10(s.unsigned_abs())?)?;
    } else {
        den = pow10(s.unsigned_abs())?;
    }
    if exp >= 0 {
        num = shl(num, exp.unsigned_abs())?;
    } else {
        den = shl(den, exp.unsigned_abs())?;
    }
    let (q, r) = (num / den, num % den);
    let rest = den - r; // r < den: no overflow; r against half of den without halving it
    Some((q, r > rest, r == rest, r != 0))
}

/// `%.<p>g` of `mant × 2^exp` for `p` up to 19 by scaled integer arithmetic (PRINTF-G): the decimal
/// exponent `X` comes from the binary one (corrected by one when the guess is off), the `p`
/// significant digits are one exact quotient `⌊x × 10^(p-1-X)⌋` rounded by the exact remainder, and
/// the style (`%f` for `-4 <= X < p`, else `%e`) and trailing-zero stripping (unless `#`) follow.
/// False (nothing written) outside that range; the general path then formats it.
///
/// Why this prints exactly what the general path prints (PRINTF-FLOAT proof):
/// 1. `X` is exact. Whatever the guess, a quotient is accepted only when
///    `10^(p-1) <= ⌊x × 10^(p-1-X)⌋ < 10^p`, i.e. `10^X <= x < 10^(X+1)` (the floor of a value
///    `>= 10^(p-1)`, an integer, is `>= 10^(p-1)`; the floor is `< 10^p` iff the value is): `X` is
///    `⌊log10 x⌋`, the decimal exponent of the first significant digit. The guess only decides
///    how many tries that takes (three misses -> fallback, never a wrong `X`).
/// 2. `q` is the first `p` significant digits, truncated, and `scaled_quotient` reports the
///    discarded tail exactly (above half / exactly half / non-zero, see there). These are the
///    inputs the general path's rounding at `p` significant digits reads (the next digit and
///    the sticky rest), so `Rounding::rounds_up` decides the same way in every mode.
/// 3. A carry to `10^p` is the significand `10^(p-1)` at `X + 1`: the exponent `%e` would print
///    after rounding, which is the `X` the C standard's `%g` style rule uses.
/// 4. Both layouts round at the same digit: `%f` with precision `p-1-X` keeps `X+1` integer
///    digits plus `p-1-X` fraction digits (for `X < 0`: `-X-1` zeros after "0." then `p` digits),
///    `p` significant digits in all; `%e` with precision `p-1` keeps `p`. So one rounded `q` gives
///    both, and the layout only places the radix (the locale's bytes), the zeros and the exponent
///    (sign, at least two digits). Trailing zeros of `q` go unless `#`; the radix goes with them
///    when no fraction digit is left, unless `#`.
/// 5. Range: `p <= 19` keeps `q < 10^19 < 2^64` (u64 digits); every product and shift is checked
///    (overflow -> fallback); zero, `p > 19` and radices over 8 bytes take the general path.
#[allow(clippy::too_many_arguments)]
fn general_scaled(out: &mut dyn Sink, mant: u128, exp: i32, neg: bool, p: usize, alt: bool, case: Case, radix: &[u8], mode: Rounding) -> bool {
    let p = p.max(1);
    if p > 19 || mant == 0 || radix.len() > 8 {
        return false;
    }
    let p32 = p as i32;
    let lo = 10u128.pow(p32 as u32 - 1);
    let hi = lo * 10;
    // floor(log10(2^b)) for the top bit b of x: X itself or one below it (|b| stays small here).
    let b = 127 - mant.leading_zeros() as i32 + exp;
    if !(-400..=400).contains(&b) {
        return false;
    }
    let mut x10 = (b * 78_913) >> 18;
    let mut found = None;
    for _ in 0..3 {
        let Some((q, above, half, inexact)) = scaled_quotient(mant, exp, p32 - 1 - x10) else { return false };
        if q >= hi {
            x10 += 1;
        } else if q < lo {
            x10 -= 1;
        } else {
            found = Some((q, above, half, inexact));
            break;
        }
    }
    let Some((q, above, half, inexact)) = found else { return false };
    let up = inexact && mode.rounds_up(neg, q & 1 == 1, above, half, true);
    let mut q = q + u128::from(up);
    if q == hi {
        // Rounding carried into a new digit: 10^p at X is 10^(p-1) at X + 1.
        q = lo;
        x10 += 1;
    }
    let Ok(q) = u64::try_from(q) else { return false };
    let mut tmp = [0u8; 20];
    let digits = u64_digits_into(&mut tmp, q); // exactly p digits, the first non-zero
    let keep = if alt { p } else { digits.iter().rposition(|&d| d != b'0').map_or(1, |i| i + 1) };
    let mut line = [0u8; 64];
    let mut n = 0usize;
    let mut push = |bytes: &[u8], line: &mut [u8; 64]| {
        for &c in bytes {
            if let Some(slot) = line.get_mut(n) {
                *slot = c;
            }
            n += 1;
        }
    };
    if x10 >= -4 && x10 < p32 {
        if x10 >= 0 {
            // Integer part: the first X + 1 digits; fraction: the kept digits after them.
            let int = x10 as usize + 1;
            push(&digits[..int], &mut line);
            if keep > int || alt {
                push(radix, &mut line);
            }
            if keep > int {
                push(&digits[int..keep], &mut line);
            }
        } else {
            push(b"0", &mut line);
            push(radix, &mut line);
            for _ in 0..(-x10 - 1) {
                push(b"0", &mut line);
            }
            push(&digits[..keep], &mut line);
        }
    } else {
        push(&digits[..1], &mut line);
        if keep > 1 || alt {
            push(radix, &mut line);
        }
        push(&digits[1..keep], &mut line);
        push(&[if case == Case::Upper { b'E' } else { b'e' }, if x10 < 0 { b'-' } else { b'+' }], &mut line);
        let mag = x10.unsigned_abs();
        let mut ebuf = [0u8; 20];
        let ed = u64_digits_into(&mut ebuf, u64::from(mag));
        if mag < 10 {
            push(b"0", &mut line);
        }
        push(ed, &mut line);
    }
    debug_assert!(n <= line.len(), "the %g line outgrew its buffer");
    out.put(line.get(..n).unwrap_or(&[]));
    true
}

/// The decimal digits of `v` ("0" for zero), at the end of `buf`.
fn u64_digits_into(buf: &mut [u8; 20], mut v: u64) -> &[u8] {
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (v % 10) as u8;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    &buf[i..]
}

/// `%e`, `%f` or `%g` of `exact_d` (exact, or truncated with its sticky bit past every digit
/// the style rounds at), without the sign. `%g` follows C: precision P (0 means 1) and X, the
/// exponent `%e` would print after rounding to P digits; `%f` with P-1-X fraction digits when
/// P > X >= -4, else `%e` with P-1; trailing zeros (and a bare radix) go unless `#`.
fn decimal_styles<const N: usize>(out: &mut dyn Sink, exact_d: &Decimal<N>, neg: bool, spec: Spec, radix: &[u8], mode: Rounding) {
    let p = spec.precision.unwrap_or(6);
    let alt = spec.alternate;
    match spec.style {
        Style::Exp | Style::Hex => sci(out, exact_d, neg, p, alt, spec.case, radix, mode, false),
        Style::Fixed => fixed(out, exact_d, neg, p, alt, radix, mode, false),
        Style::General => {
            let pp = p.max(1);
            let pp_i = i64::try_from(pp).unwrap_or(i64::MAX);
            let r = round_digits(exact_d, pp_i, neg, mode);
            let x = if r.is_zero() { 0 } else { i64::from(r.exp10) - 1 };
            if x < pp_i && x >= -4 {
                fixed(out, exact_d, neg, usize::try_from(pp_i - 1 - x).unwrap_or(0), alt, radix, mode, !alt);
            } else {
                sci(out, exact_d, neg, pp - 1, alt, spec.case, radix, mode, !alt);
            }
        }
    }
}

/// `%f` of `d` with `p` fraction digits; `strip` drops trailing fraction zeros and then a bare
/// radix (for `%g` without `#`).
#[allow(clippy::too_many_arguments)]
fn fixed<const N: usize>(out: &mut dyn Sink, d: &Decimal<N>, neg: bool, p: usize, alt: bool, radix: &[u8], mode: Rounding, strip: bool) {
    let p = i64::try_from(p).unwrap_or(i64::MAX / 2);
    let r = round_digits(d, i64::from(d.exp10) + p, neg, mode);
    // Digit index of the first fraction digit (before the first stored digit when below 1).
    let e = i64::from(r.exp10);
    if e > 0 {
        put_digits(out, &r, 0, e);
    } else {
        out.put(b"0");
    }
    let frac_len = if strip { (r.len as i64 - e).clamp(0, p) } else { p };
    if frac_len > 0 || alt {
        out.put(radix);
    }
    put_digits(out, &r, e, e + frac_len);
}

#[allow(clippy::too_many_arguments)]
fn sci<const N: usize>(out: &mut dyn Sink, d: &Decimal<N>, neg: bool, p: usize, alt: bool, case: Case, radix: &[u8], mode: Rounding, strip: bool) {
    // The usual precisions assemble the whole text on the stack and hand it over in one call
    // (the sink is a dyn call per piece otherwise: digit, radix, digits, `e`, sign, exponent).
    if p <= SCI_LINE_DIGITS && radix.len() <= 8 {
        let mut r = Decimal::<N>::zero();
        round_digits_into(d, p as i64 + 1, neg, mode, &mut r);
        let exp10 = if r.is_zero() { 0 } else { i64::from(r.exp10) - 1 };
        let mut line = [0u8; SCI_LINE];
        let mut n = 0usize;
        let mut push = |bytes: &[u8], line: &mut [u8; SCI_LINE]| {
            for &b in bytes {
                if let Some(slot) = line.get_mut(n) {
                    *slot = b;
                }
                n += 1;
            }
        };
        push(&[b'0' + r.digit(0)], &mut line);
        let mut frac_len = p;
        if strip {
            frac_len = (1..=p).rev().find(|&k| r.digit(k) != 0).unwrap_or(0);
        }
        if frac_len > 0 || alt {
            push(radix, &mut line);
        }
        for k in 1..=frac_len {
            push(&[b'0' + r.digit(k)], &mut line);
        }
        push(&[if case == Case::Upper { b'E' } else { b'e' }, if exp10 < 0 { b'-' } else { b'+' }], &mut line);
        let mag = exp10.unsigned_abs();
        let mut digits = [0u8; 64];
        let ds = u64_digits(&mut digits, mag, 10, Case::Lower);
        if mag < 10 {
            push(b"0", &mut line);
        }
        push(ds, &mut line);
        debug_assert!(n <= SCI_LINE, "the %e line outgrew its buffer");
        out.put(line.get(..n).unwrap_or(&[]));
        return;
    }
    sci_by_pieces(out, d, neg, p, alt, case, radix, mode, strip);
}

/// Longest `%e` text assembled on the stack: precision, radix (8 bytes), `e+ddddd`.
const SCI_LINE_DIGITS: usize = 40;
const SCI_LINE: usize = 1 + 8 + SCI_LINE_DIGITS + 2 + 5 + 8;

/// [`sci`] writing each piece to the sink as it goes (any precision); the reference the
/// buffered path must equal.
#[allow(clippy::too_many_arguments)]
fn sci_by_pieces<const N: usize>(out: &mut dyn Sink, d: &Decimal<N>, neg: bool, p: usize, alt: bool, case: Case, radix: &[u8], mode: Rounding, strip: bool) {
    let p = i64::try_from(p).unwrap_or(i64::MAX / 2);
    let r = round_digits(d, p + 1, neg, mode);
    let exp10 = if r.is_zero() { 0 } else { i64::from(r.exp10) - 1 };
    out.put(&[b'0' + r.digit(0)]);
    let frac_len = if strip { (r.len as i64 - 1).clamp(0, p) } else { p };
    if frac_len > 0 || alt {
        out.put(radix);
    }
    put_digits(out, &r, 1, 1 + frac_len);
    put_exponent(out, if case == Case::Upper { b'E' } else { b'e' }, exp10, 2);
}

/// The exponent part: `letter`, its sign, then at least `min_digits` decimal digits.
fn put_exponent(out: &mut dyn Sink, letter: u8, e: i64, min_digits: usize) {
    let mut buf = [0u8; 64];
    let ds = u64_digits(&mut buf, e.unsigned_abs(), 10, Case::Lower);
    out.put(&[letter, if e < 0 { b'-' } else { b'+' }]);
    if ds.len() < min_digits {
        out.repeat(b'0', min_digits - ds.len());
    }
    out.put(ds);
}

/// `%a` (D-8.9, probed): the significand in hexadecimal, the binary exponent in decimal.
/// binary32 prints as the binary64 it converts to; binary64 and binary128 lead with `1` (`0`
/// for a subnormal, at exponent `emin`); x87 prints its 64-bit significand nibble by nibble
/// (lead `8` to `f` when normal, subnormals at -16385). Without a precision: the shortest exact
/// text. With one: rounded at the last kept nibble in `mode`; a carry makes the lead `2`, and
/// on x87 a carry past `f` gives `1` with the exponent 4 higher.
fn format_hex(out: &mut dyn Sink, x: Float, fmt: Format, spec: Spec, radix: &[u8], mode: Rounding) {
    let fmt = if fmt == Format::F32 { Format::F64 } else { fmt };
    // The canonical encoding: normalized, or subnormal at the least exponent.
    let (mant, exp) = match Float::unpack(x.pack(fmt), fmt).value {
        Value::Finite { mant, exp } => (mant, exp),
        _ => (0, 0),
    };
    // Fraction bits after the lead digit, a whole number of nibbles in every format.
    let fb = if fmt.explicit_lead { fmt.prec - 4 } else { fmt.prec - 1 };
    let nibbles = (fb / 4) as usize;
    let (mut lead, mut frac, mut e) = if mant == 0 { (0u128, 0u128, 0i64) } else { (mant >> fb, mant & ((1u128 << fb) - 1), i64::from(exp) + i64::from(fb)) };
    let shown = match spec.precision {
        None => nibbles - (frac.trailing_zeros() / 4).min(fb / 4) as usize,
        Some(p) if p < nibbles => {
            let drop = 4 * (nibbles - p) as u32;
            let (mut kept, rem, half) = (frac >> drop, frac & ((1u128 << drop) - 1), 1u128 << (drop - 1));
            let odd = if p == 0 { lead & 1 == 1 } else { kept & 1 == 1 };
            if mode.rounds_up(x.neg, odd, rem > half, rem == half, rem != 0) {
                kept += 1;
                if kept >> (4 * p) != 0 {
                    kept = 0;
                    lead += 1;
                }
            }
            frac = kept << drop;
            if lead == 16 {
                lead = 1;
                e += 4;
            }
            p
        }
        Some(p) => p,
    };
    let upper = spec.case == Case::Upper;
    let hex: &[u8; 16] = if upper { b"0123456789ABCDEF" } else { b"0123456789abcdef" };
    out.put(if upper { b"0X" } else { b"0x" });
    out.put(&[hex[(lead & 15) as usize]]);
    if shown > 0 || spec.alternate {
        out.put(radix);
    }
    let real = shown.min(nibbles);
    let mut buf = [0u8; 32];
    for (i, slot) in buf[..real].iter_mut().enumerate() {
        *slot = hex[((frac >> (fb - 4 * (i as u32 + 1))) & 15) as usize];
    }
    out.put(&buf[..real]);
    out.repeat(b'0', shown - real);
    put_exponent(out, if upper { b'P' } else { b'p' }, e, 1);
}

/// Integer to text in `radix` 2..=36 (D-8.4).
pub fn format_u64(out: &mut dyn Sink, v: u64, radix: u32, case: Case) {
    let mut buf = [0u8; 64];
    out.put(u64_digits(&mut buf, v, radix, case));
}

/// The digits of `v` in `radix` (2 to 36), at the end of `buf`.
pub fn u64_digits(buf: &mut [u8; 64], v: u64, radix: u32, case: Case) -> &[u8] {
    let mut i = buf.len();
    let mut v = v;
    let digits: &[u8; 36] = if case == Case::Upper { b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ" } else { b"0123456789abcdefghijklmnopqrstuvwxyz" };
    match radix {
        // Constant divisors: two decimal digits per division by 100; powers of two by shifts.
        10 => {
            const PAIRS: &[u8; 200] = b"00010203040506070809101112131415161718192021222324252627282930313233343536373839404142434445464748495051525354555657585960616263646566676869707172737475767778798081828384858687888990919293949596979899";
            while v >= 100 {
                let r = (v % 100) as usize;
                v /= 100;
                i -= 2;
                buf[i] = PAIRS[2 * r];
                buf[i + 1] = PAIRS[2 * r + 1];
            }
            if v >= 10 {
                i -= 2;
                buf[i] = PAIRS[2 * v as usize];
                buf[i + 1] = PAIRS[2 * v as usize + 1];
            } else {
                i -= 1;
                buf[i] = b'0' + v as u8;
            }
            return &buf[i..];
        }
        2 | 8 | 16 => {
            let shift = radix.trailing_zeros();
            let mask = u64::from(radix) - 1;
            loop {
                i -= 1;
                buf[i] = digits[(v & mask) as usize];
                v >>= shift;
                if v == 0 {
                    break;
                }
            }
            return &buf[i..];
        }
        _ => {}
    }
    let r = u64::from(radix.clamp(2, 36));
    loop {
        i -= 1;
        let d = (v % r) as usize;
        let c = b"0123456789abcdefghijklmnopqrstuvwxyz"[d];
        buf[i] = if case == Case::Upper { c.to_ascii_uppercase() } else { c };
        v /= r;
        if v == 0 {
            break;
        }
    }
    &buf[i..]
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, clippy::byte_char_slices)]
mod tests {
    use super::*;

    struct S(Vec<u8>);
    impl Sink for S {
        fn put(&mut self, b: &[u8]) {
            self.0.extend_from_slice(b);
        }
    }

    fn f(x: f64, style: Style, p: Option<usize>) -> String {
        let mut s = S(Vec::new());
        let v = Float::unpack(u128::from(x.to_bits()), Format::F64);
        format_float(&mut s, v, Format::F64, Spec { style, case: Case::Lower, precision: p, alternate: false }, b".", Rounding::Nearest);
        String::from_utf8(s.0).unwrap()
    }

    #[test]
    fn decimal_styles_agree_with_rust_formatting() {
        let xs = [0.0, 1.0, 0.1, 2.5, 3.5, 1e-7, 123456.789, 1e300, 5e-324, 1.7976931348623157e308, 0.000123, 999.9995, 9.5, 0.5];
        for x in xs {
            for p in [0usize, 1, 3, 6, 17, 30] {
                assert_eq!(f(x, Style::Fixed, Some(p)), format!("{x:.p$}"), "{x} %.{p}f");
                // Rust's {:e} has no forced two-digit exponent; compare mantissa and exponent.
                let ours = f(x, Style::Exp, Some(p));
                let theirs = format!("{x:.p$e}");
                let (om, oe) = ours.split_once('e').unwrap();
                let (tm, te) = theirs.split_once('e').unwrap();
                assert_eq!(om, tm, "{x} %.{p}e");
                assert_eq!(oe.parse::<i32>().unwrap(), te.parse::<i32>().unwrap());
            }
        }
        assert_eq!(f(1e-5, Style::General, None), "1e-05");
        assert_eq!(f(123456.0, Style::General, None), "123456");
        assert_eq!(f(1234567.0, Style::General, None), "1.23457e+06");
        assert_eq!(f(0.0001, Style::General, None), "0.0001");
        assert_eq!(f(100.0, Style::General, Some(1)), "1e+02");
        assert_eq!(f(-0.0, Style::Fixed, Some(1)), "-0.0");
        assert_eq!(f(f64::NAN, Style::Fixed, None), "nan");
    }

    #[test]
    fn hex_style() {
        assert_eq!(f(1.0, Style::Hex, None), "0x1p+0");
        assert_eq!(f(0.5, Style::Hex, None), "0x1p-1");
        assert_eq!(f(-3.0, Style::Hex, None), "-0x1.8p+1");
        assert_eq!(f(5e-324, Style::Hex, None), "0x0.0000000000001p-1022");
        assert_eq!(f(0.0, Style::Hex, None), "0x0p+0");
        assert_eq!(f(1.0 + f64::EPSILON, Style::Hex, Some(1)), "0x1.0p+0");
        assert_eq!(f(1.96875, Style::Hex, Some(1)), "0x2.0p+0", "a carry into the leading digit makes it 2, as the reference library prints");
    }

    #[test]
    fn the_small_exact_decimal_equals_the_big_one() {
        // Every exponent the small path takes, and a spread of significands (a fixed
        // pseudo-random sequence, the extremes and powers of two).
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        for exp in -124..=63 {
            for k in 0..64 {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let mant = match k {
                    0 => 1,
                    1 => u64::MAX,
                    2 => 1 << 52,
                    3 => (1 << 53) - 1,
                    _ => x >> (k % 12),
                };
                let small = exact_small(u128::from(mant), exp).expect("in range");
                let big = exact(u128::from(mant), exp);
                assert_eq!((small.digits(), small.exp10), (big.digits(), big.exp10), "mant {mant} exp {exp}");
            }
        }
        assert!(exact_small(1, -125).is_none() && exact_small(1, 64).is_none() && exact_small(1 << 64, 0).is_none());
        assert!(exact_small(0, 5).is_some_and(|d| d.is_zero()));
    }

    fn xorshift(x: &mut u64) -> u64 {
        *x ^= *x << 13;
        *x ^= *x >> 7;
        *x ^= *x << 17;
        *x
    }

    /// Format `mant × 2^exp` through `format_float` (the truncated digits) and through the
    /// full exact expansion (the reference), in every rounding mode, every decimal style,
    /// precisions 0..=40 (and some huge ones) and both `#` settings; the outputs must be
    /// identical.
    fn check_against_full_expansion(neg: bool, mant: u128, exp: i32) {
        let full = exact(mant, exp);
        let x = Float { neg, value: if mant == 0 { Value::Zero } else { Value::Finite { mant, exp } } };
        for mode in [Rounding::Nearest, Rounding::Up, Rounding::Down, Rounding::Zero] {
            for style in [Style::Exp, Style::Fixed, Style::General] {
                for p in (0..=40).chain([60, 120, 200]) {
                    for alternate in [false, true] {
                        let spec = Spec { style, case: Case::Lower, precision: Some(p), alternate };
                        let mut ours = S(Vec::new());
                        format_float(&mut ours, x, Format::F64, spec, b".", mode);
                        let mut want = S(Vec::new());
                        if neg {
                            want.put(b"-");
                        }
                        decimal_styles(&mut want, &full, neg, spec, b".", mode);
                        assert_eq!(ours.0, want.0, "mant {mant} exp {exp} neg {neg} {mode:?} {style:?} p {p} alt {alternate}");
                    }
                }
            }
        }
    }

    /// The buffered `%e` text equals the piece-by-piece one (the pre-P-E reference) for every
    /// precision (across the buffered limit), flag, case, rounding mode, radix length and
    /// exponent width: subnormal to huge, ties, carries into a new exponent, zero.
    #[test]
    fn buffered_exp_text_equals_the_piecewise_text() {
        let mut cases: Vec<(u128, i32)> = vec![
            (0, 0), (1, 0), (1, -1074), (5, -1), (25, -1), (125, -3), (99_999_995, 0), (9_999_995, -1), ((1 << 53) - 1, 971), ((1 << 53) - 1, -53),
            (u128::from(u64::MAX), -64), (u128::from(u64::MAX), 63), (0x8000_0000_0000_0001, -63), (1, -16445), (1 << 63, 16320), (3, -2), (9, -1), (1_000_005, 0),
            (7, 200), (7, -200), (123_456_789, -30), (1, 1),
            (10_000_000_000, 0), (1, -33), (1, 33), (1, 332), (1, -332), (1, 3), (1, -3), (1_000_000_000, 0), (1, -30), (1, 30), (1, 10), (1, -10),
        ];
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..40 {
            let m = (u128::from(xorshift(&mut x)) >> 11) | (1 << 52);
            cases.push((m, (xorshift(&mut x) % 400) as i32 - 330));
        }
        let radixes: [&[u8]; 4] = [b".", b",", b"\xd9\xab", b"123456789"];
        for &(mant, exp) in &cases {
            let d = exact(mant, exp);
            for mode in [Rounding::Nearest, Rounding::Up, Rounding::Down, Rounding::Zero] {
                for neg in [false, true] {
                    for p in (0..=45).chain([60, 200]) {
                        for (alt, strip) in [(false, false), (true, false), (false, true), (true, true)] {
                            for case in [Case::Lower, Case::Upper] {
                                for radix in radixes {
                                    let mut a = S(Vec::new());
                                    sci(&mut a, &d, neg, p, alt, case, radix, mode, strip);
                                    let mut b = S(Vec::new());
                                    sci_by_pieces(&mut b, &d, neg, p, alt, case, radix, mode, strip);
                                    assert_eq!(a.0, b.0, "mant {mant} exp {exp} neg {neg} {mode:?} p {p} alt {alt} strip {strip}");
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    /// The chunked fraction digits (9, 4 or 1 at a time by the shift) are the full expansion's
    /// leading digits for every limit, around the shifts where the chunk size changes, and
    /// the sticky bit says exactly whether non-zero digits follow.
    #[test]
    fn chunked_fraction_digits_are_the_leading_digits_of_the_full_expansion() {
        let mut cases: Vec<(u128, i32)> = Vec::new();
        let mut x = 0x2545_F491_4F6C_DD1Du64;
        for sh in [1, 2, 53, 57, 58, 59, 60, 63, 64, 65, 96, 97, 98, 99, 100, 112, 113, 114, 115, 116, 123, 124] {
            for m in [1u64, 3, 5, 0x10_0000_0000_0001, u64::MAX, 1 << 63, 999_999_999] {
                cases.push((u128::from(m), -sh));
            }
            for _ in 0..12 {
                cases.push((u128::from(xorshift(&mut x) >> (xorshift(&mut x) % 40)).max(1), -sh));
            }
        }
        for &(mant, exp) in &cases {
            let full = exact(mant, exp);
            for sig in [1usize, 2, 3, 8, 9, 10, 17, 18, 19, 30, 100, usize::MAX] {
                for frac in [0usize, 1, 6, 9, 10, 18, 19, 50, usize::MAX] {
                    let Some(d) = exact_small_limited(mant, exp, sig, frac) else { continue };
                    assert_eq!(d.exp10, full.exp10, "mant {mant} exp {exp} sig {sig} frac {frac}");
                    for i in 0..d.digits().len() {
                        assert_eq!(d.digit(i), full.digit(i), "mant {mant} exp {exp} sig {sig} frac {frac} digit {i}");
                    }
                    if d.sticky {
                        assert!(full.digits().len() > d.digits().len(), "mant {mant} exp {exp} sig {sig} frac {frac}: sticky without more digits");
                    } else {
                        assert_eq!(d.digits(), full.digits(), "mant {mant} exp {exp} sig {sig} frac {frac}");
                    }
                }
            }
        }
    }

    #[test]
    fn truncated_digits_format_as_the_full_expansion() {
        // Edge cases: ties at many positions ((2n+1) / 2^k, integers ending in 5), carries
        // (9.99.., 99.5), the smallest subnormal and largest double (the big path), the ends
        // of the small path's range, x87 significands (64 bits), zero.
        let mut cases: Vec<(u128, i32)> = vec![
            (1, -1074), (1, -124), (u128::from(u64::MAX), -124), (u128::from(u64::MAX), 63), (1, 63),
            ((1 << 53) - 1, 971), (1 << 52, -1074), (5, 0), (25, -1), (125, -3), (12345, 0), (1_000_005, 0),
            (199, -1), (19_999_999, -1), (99_999_995, 0), ((1 << 53) - 1, -53), ((1 << 53) - 1, 0), (3, -2),
            (u128::from(u64::MAX), -64), (0x8000_0000_0000_0001, -63), (0, 0),
        ];
        let mut r = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..200 {
            let k = (xorshift(&mut r) % 40) as i32 + 1;
            cases.push((u128::from((xorshift(&mut r) >> 30) | 1), -k));
        }
        for _ in 0..400 {
            // Random doubles: every magnitude the small path takes, a few it does not.
            let bits = xorshift(&mut r);
            let v = Float::unpack(u128::from(bits), Format::F64);
            if let Value::Finite { mant, exp } = v.value {
                if (-140..=80).contains(&exp) || bits % 32 == 0 {
                    cases.push((mant, exp));
                }
            }
        }
        for _ in 0..100 {
            let mant = xorshift(&mut r) | 1 << 63;
            cases.push((u128::from(mant), (xorshift(&mut r) % 200) as i32 - 140));
        }
        for (i, &(mant, exp)) in cases.iter().enumerate() {
            check_against_full_expansion(i % 2 == 1, mant, exp);
        }
    }

    #[test]
    fn scaled_fixed_equals_the_full_expansion() {
        // The scaled-integer %f path (p <= 19) against the untouched exact expansion: gawk's
        // i / 7 shapes, ties (2n+1) / 2^k at every precision, x87 significands next to 2^64,
        // integers next to the u64 limit, a multi-byte radix, all rounding modes, '#', sign.
        let mut cases: Vec<(u128, i32)> = Vec::new();
        for i in (0..1_000_000u32).step_by(4_999).chain([1, 6, 7, 8, 999_999]) {
            if let Value::Finite { mant, exp } = Float::unpack(u128::from((f64::from(i) / 7.0).to_bits()), Format::F64).value {
                cases.push((mant, exp));
            }
        }
        for k in 1..=24 {
            for n in [0u128, 1, 2, 7, 12_345] {
                cases.push((2 * n + 1, -k));
            }
        }
        let mut r = 0xD1B5_4A32_D192_ED03u64;
        for _ in 0..60 {
            cases.push((u128::from(xorshift(&mut r) | 1 << 63), (xorshift(&mut r) % 200) as i32 - 190));
            cases.push((u128::from(xorshift(&mut r) >> 11 | 1 << 52), (xorshift(&mut r) % 140) as i32 - 120));
        }
        cases.extend([(u128::from(u64::MAX), 0), (u128::from(u64::MAX), 1), (1, 63), (1, 64), (99_999_995, -1)]);
        scaled_cases_check(&cases, Style::Fixed, 0..=20);
    }

    #[test]
    fn scaled_general_equals_the_full_expansion() {
        // The scaled-integer %g path (p <= 19) against the untouched exact expansion: gawk's i / 7
        // (awk's "%.6g"), powers of ten and their neighbours (style switch at X = -4 and X = p,
        // carries like 9.9999995 -> 10), ties, random doubles over the whole e/f range, x87
        // significands; every precision 0..=20, all rounding modes, '#', sign, a 2-byte radix.
        let f64_case = |v: f64| match Float::unpack(u128::from(v.to_bits()), Format::F64).value {
            Value::Finite { mant, exp } => Some((mant, exp)),
            _ => None,
        };
        let mut cases: Vec<(u128, i32)> = Vec::new();
        for i in (0..1_000_000u32).step_by(9_973).chain([1, 6, 7, 8, 999_999]) {
            cases.extend(f64_case(f64::from(i) / 7.0));
        }
        for k in -25..=25 {
            let t = 10f64.powi(k);
            for v in [t, t * (1.0 - 1e-16), t * (1.0 + 2e-16), t * 0.99999995, t * 0.9999995, t * 0.95, t * 9.5, t * 0.5] {
                cases.extend(f64_case(v));
            }
        }
        for k in 1..=24 {
            for n in [0u128, 1, 7, 12_345, 999_999] {
                cases.push((2 * n + 1, -k));
            }
        }
        let mut r = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..80 {
            cases.push((u128::from(xorshift(&mut r) | 1 << 63), (xorshift(&mut r) % 260) as i32 - 190));
            cases.push((u128::from(xorshift(&mut r) >> 11 | 1 << 52), (xorshift(&mut r) % 300) as i32 - 200));
        }
        cases.extend([(u128::from(u64::MAX), 0), (u128::from(u64::MAX), 70), (1, 63), (1, 300), (1, -300), (99_999_995, -1)]);
        scaled_cases_check(&cases, Style::General, 0..=20);
    }

    fn scaled_cases_check(cases: &[(u128, i32)], style: Style, precisions: core::ops::RangeInclusive<usize>) {
        for &(mant, exp) in cases {
            let full = exact(mant, exp);
            for neg in [false, true] {
                let x = Float { neg, value: Value::Finite { mant, exp } };
                for mode in [Rounding::Nearest, Rounding::Up, Rounding::Down, Rounding::Zero] {
                    for p in precisions.clone() {
                        for (alternate, case, radix) in [(false, Case::Lower, &b"."[..]), (true, Case::Upper, &b"."[..]), (false, Case::Upper, &b"\xd9\xab"[..])] {
                            let spec = Spec { style, case, precision: Some(p), alternate };
                            let mut ours = S(Vec::new());
                            format_float(&mut ours, x, Format::F64, spec, radix, mode);
                            let mut want = S(Vec::new());
                            if neg {
                                want.put(b"-");
                            }
                            decimal_styles(&mut want, &full, neg, spec, radix, mode);
                            assert_eq!(ours.0, want.0, "mant {mant} exp {exp} neg {neg} {mode:?} p {p} alt {alternate}");
                        }
                    }
                }
            }
        }
    }

    /// One value of the %g differential run: doubles of every class (random bits, the scaled
    /// range, subnormals), exact ties at a digit position, values next to a rounding boundary
    /// at precision `p` (decimal strings ...5, ...4999.., ...5000..1, and their neighbouring
    /// doubles), powers of ten and their neighbours, the fixed/exponent switch (X = -5..-3 and
    /// X = p-2..p+1, including carries like 9.9999995e-5 -> 0.0001), and x87 significands.
    fn differential_value(r: &mut u64, kind: u64, p: usize) -> (Float, Format) {
        let f64v = |v: f64| Float::unpack(u128::from(v.to_bits()), Format::F64);
        let nudge = |v: f64, r: &mut u64| {
            let d = (xorshift(r) % 7) as i64 - 3;
            f64::from_bits((v.to_bits() as i64).wrapping_add(d) as u64)
        };
        let x = match kind % 8 {
            0 => Float::unpack(u128::from(xorshift(r)), Format::F64),
            1 => {
                let b = (xorshift(r) % 141) as i32 - 70;
                f64v((xorshift(r) >> 11 | 1 << 52) as f64 * 2f64.powi(b - 52))
            }
            2 => Float::unpack(u128::from(xorshift(r) & ((1 << 52) - 1)), Format::F64),
            3 => {
                // (2n+1) / 2^k: a 5 in the last decimal place, a tie when p cuts just before it.
                let n = xorshift(r) >> (12 + xorshift(r) % 52); // 2n+1 < 2^53: exact
                let k = (xorshift(r) % 70) as i32 + 1;
                f64v((2 * n + 1) as f64 * 2f64.powi(-k))
            }
            4 => {
                // Integers t·10^j + 5·10^(j-1) (ties for exp >= 0) while exact in a double.
                let j = (xorshift(r) % 12) as u32 + 1;
                let t = xorshift(r) % 10u64.pow(p.clamp(1, 17) as u32);
                let v = t.saturating_mul(10u64.pow(j)).saturating_add(5 * 10u64.pow(j - 1));
                f64v((v & ((1 << 53) - 1)) as f64)
            }
            5 | 6 => {
                let digits = p.clamp(1, 17);
                let lead = xorshift(r) % 9 + 1;
                let rest = if digits > 1 { xorshift(r) % 10u64.pow(digits as u32 - 1) } else { 0 };
                let tail = ["5", "49999999", "50000001", "4999999999999", "5000000000001", "9999999"][(xorshift(r) % 6) as usize];
                let pi = digits as i32;
                let x10 = if kind % 8 == 6 {
                    [-6, -5, -4, -3, pi - 2, pi - 1, pi, pi + 1][(xorshift(r) % 8) as usize]
                } else if xorshift(r) % 4 == 0 {
                    (xorshift(r) % 640) as i32 - 330
                } else {
                    (xorshift(r) % 66) as i32 - 25
                };
                let frac = if digits > 1 { format!("{rest:0w$}", w = digits - 1) } else { String::new() };
                let mut v: f64 = format!("{lead}.{frac}{tail}e{x10}").parse().unwrap();
                if xorshift(r) % 3 == 0 {
                    v = nudge(v, r);
                }
                if xorshift(r) % 5 == 0 {
                    v = format!("1e{}", (xorshift(r) % 632) as i32 - 323).parse().unwrap();
                    v = nudge(v, r);
                }
                f64v(v)
            }
            _ => {
                let mant = u128::from(xorshift(r) | 1 << 63);
                let exp = (xorshift(r) % 260) as i32 - 190;
                return (Float { neg: false, value: Value::Finite { mant, exp } }, Format::F80);
            }
        };
        (x, Format::F64)
    }

    /// `n` values, each formatted with `%.6g` and with one random %g spec (precision 0..=20,
    /// all four rounding modes, '#', case, a 2-byte radix, sign) by the scaled path and by the
    /// general path; every output must be byte-identical. Returns (formats, scaled hits).
    fn general_differential(n: u64, seed: u64) -> (u64, u64) {
        let mut r = seed;
        let (mut runs, mut hits) = (0u64, 0u64);
        let modes = [Rounding::Nearest, Rounding::Up, Rounding::Down, Rounding::Zero];
        for i in 0..n {
            let rp = (xorshift(&mut r) % 21) as usize;
            let (mut x, fmt) = differential_value(&mut r, i, if i % 2 == 0 { 6 } else { rp });
            x.neg = xorshift(&mut r) % 4 == 0;
            let Value::Finite { mant, exp } = x.value else { continue };
            let pick = xorshift(&mut r);
            let specs = [
                (Spec { style: Style::General, case: Case::Lower, precision: Some(6), alternate: false }, &b"."[..], Rounding::Nearest),
                (
                    Spec { style: Style::General, case: if pick & 1 == 0 { Case::Lower } else { Case::Upper }, precision: Some(rp), alternate: pick & 2 != 0 },
                    if pick & 4 == 0 { &b"."[..] } else { &b"\xd9\xab"[..] },
                    modes[(pick >> 3) as usize % 4],
                ),
            ];
            for (spec, radix, mode) in specs {
                runs += 1;
                let mut ours = S(Vec::new());
                if !general_scaled(&mut ours, mant, exp, x.neg, spec.precision.unwrap_or(6), spec.alternate, spec.case, radix, mode) {
                    assert!(ours.0.is_empty(), "the scaled path wrote before falling back");
                    continue;
                }
                hits += 1;
                let mut want = S(Vec::new());
                decimal_general(&mut want, x, spec, radix, mode);
                assert_eq!(
                    String::from_utf8_lossy(&ours.0),
                    String::from_utf8_lossy(&want.0),
                    "prec {} mant {mant} exp {exp} neg {} {mode:?} p {:?} alt {} {:?}",
                    fmt.prec,
                    x.neg,
                    spec.precision,
                    spec.alternate,
                    spec.case
                );
                // And the whole entry point (sign included) takes the same output.
                let mut whole = S(Vec::new());
                format_float(&mut whole, x, fmt, spec, radix, mode);
                let signed = [if x.neg { &b"-"[..] } else { &b""[..] }, &want.0[..]].concat();
                assert_eq!(whole.0, signed, "format_float, mant {mant} exp {exp}");
            }
        }
        (runs, hits)
    }

    #[test]
    fn scaled_general_matches_the_general_path_on_random_values() {
        let (runs, hits) = general_differential(40_000, 0x5DEE_CE66_D1CE_4E5Bu64);
        // The run must exercise the scaled path, not only its fallback.
        assert!(hits * 3 > runs, "scaled %g took {hits} of {runs}");
    }

    /// The PRINTF-FLOAT differential run: millions of values (LIELLA_PF_DIFF_N, default 4M,
    /// two formats each). Ignored in the suite (minutes unoptimised); run with --release
    /// --ignored --nocapture.
    #[test]
    #[ignore]
    fn scaled_general_matches_the_general_path_on_millions() {
        let n = std::env::var("LIELLA_PF_DIFF_N").ok().and_then(|v| v.parse().ok()).unwrap_or(4_000_000u64);
        let seed = std::env::var("LIELLA_PF_DIFF_SEED").ok().and_then(|v| v.parse().ok()).unwrap_or(0x9E37_79B9_7F4A_7C15u64);
        let (runs, hits) = general_differential(n, seed | 1);
        std::eprintln!("PF-DIFF: {n} values, {runs} formats, {hits} by the scaled path, all identical to the general path");
        assert!(hits * 3 > runs, "scaled %g took {hits} of {runs}");
    }

    #[test]
    fn random_doubles_agree_with_rust_formatting() {
        let mut r = 0x2545_F491_4F6C_DD1Du64;
        for i in 0..3000 {
            // Magnitudes from about 2^-70 to 2^70 (the small path's range).
            let e = (xorshift(&mut r) % 141) as i32 - 70;
            let x = (xorshift(&mut r) >> 11) as f64 * 2f64.powi(e - 53);
            let p = i % 41;
            assert_eq!(f(x, Style::Fixed, Some(p)), format!("{x:.p$}"), "{x:e} %.{p}f");
            let ours = f(x, Style::Exp, Some(p));
            let theirs = format!("{x:.p$e}");
            let (om, oe) = ours.split_once('e').unwrap();
            let (tm, te) = theirs.split_once('e').unwrap();
            assert_eq!(om, tm, "{x:e} %.{p}e");
            assert_eq!(oe.parse::<i32>().unwrap(), te.parse::<i32>().unwrap());
        }
    }

    #[test]
    fn integers_in_every_radix_agree_with_std() {
        let mut vals = vec![0u64, 1, 9, 10, 99, 100, 101, 999, 1000, u64::MAX, u64::MAX - 1, 1 << 63];
        let mut x = 0x2545_F491_4F6C_DD1Du64;
        for _ in 0..2000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            vals.push(x >> (x % 60));
        }
        for &v in &vals {
            for (radix, want) in [(10, format!("{v}")), (16, format!("{v:x}")), (8, format!("{v:o}")), (2, format!("{v:b}")), (36, String::new())] {
                let mut s = S(Vec::new());
                format_u64(&mut s, v, radix, Case::Lower);
                if radix != 36 {
                    assert_eq!(String::from_utf8(s.0).unwrap(), want, "{v} radix {radix}");
                }
            }
            let mut s = S(Vec::new());
            format_u64(&mut s, v, 16, Case::Upper);
            assert_eq!(String::from_utf8(s.0).unwrap(), format!("{v:X}"));
        }
    }
}
