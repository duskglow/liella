//! The elementary functions of the wide formats (spec §10.3, D-10.19): x87 `long double`
//! (`l`, `f64x`), and generic over any `Float` so that `_Float128` can follow on the same code.
//! Each function is evaluated in the 384-bit arithmetic of [`crate::mp`] (relative error near
//! 2^-370 for every kernel used) and rounded once into the format in the caller's direction, so
//! every result is correctly rounded in every mode unless the exact value lies within about
//! 2^-370 of a rounding boundary. That happens only for exact results, which are detected and
//! computed exactly (`exp2` and `log2` at powers of two, `exp10` and `log10` at powers of ten,
//! perfect cubes, the exact powers of `pow`), and for arguments so small or large that the
//! function differs from a representable value by less than its last place: those are rounded
//! by the side of the deviation ([`beside`]), as `elem::near` does for `float` and `double`.
//!
//! The special operands, NaNs, flags and `errno` follow the `float` and `double` forms (D-10.14).

#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::as_conversions)] // exponents of bounded range

use crate::elem;
use crate::float::{self, Exact, Float, Kind, Num};
use crate::fused;
use crate::mp::{self, Mp};
use crate::status::{Rounding, Status};
use crate::trig::elem_round_below;

/// The value of a finite or zero operand.
pub(crate) fn value(n: &Num) -> Mp {
    if n.kind == Kind::Zero { Mp::ZERO } else { Mp::from_u128(n.neg, n.sig, i64::from(n.exp)) }
}

/// The value as an `f64` estimate for range decisions (saturating).
fn est(n: &Num) -> f64 {
    value(n).to_f64()
}

/// Round an approximation (see [`mp::round`]).
fn approx<F: Float>(v: Mp, mode: Rounding) -> (F, Status) {
    mp::round::<F>(v, mode)
}

/// An exact result (at most 128 significant bits; zero gives +0).
fn exact<F: Float>(v: Mp, mode: Rounding) -> (F, Status) {
    if v.is_zero() {
        return (float::zero(false), Status::OK);
    }
    mp::round_exact::<F>(v, mode)
}

/// The exact value `v` (at most 120 significant bits, nonzero) moved by an amount far below its
/// last place, away from zero when `larger` and toward it otherwise: the result of a function
/// whose deviation from `v` the 384-bit evaluation cannot resolve or need not.
pub(crate) fn beside<F: Float>(v: Mp, larger: bool, mode: Rounding) -> (F, Status) {
    let sig = (u128::from(v.m[mp::LIMBS - 1]) << 64) | u128::from(v.m[mp::LIMBS - 2]);
    let exp = (v.exp - 128).clamp(-(1 << 30), 1 << 30) as i32;
    let sig = if larger { sig } else { sig - 1 };
    let (r, flags) = float::round::<F>(Exact { neg: v.neg, sig, exp, sticky: true }, mode);
    (r, float::range_errno(r, Status::flags(flags)))
}

/// `x(1 + δ)` for a tiny `δ` of known sign (`grows`: `|f(x)| > |x|`).
fn near_x<F: Float>(n: &Num, grows: bool, mode: Rounding) -> (F, Status) {
    beside(value(n), grows, mode)
}

/// `±1` moved by a tiny amount (`larger`: magnitude above 1).
pub(crate) fn near_one<F: Float>(neg: bool, larger: bool, mode: Rounding) -> (F, Status) {
    let one = Mp::from_i64(1);
    beside(if neg { one.neg() } else { one }, larger, mode)
}

/// `|x| < 2^-(PREC/2 + 2)`: `x(1 + c·x²)` (c of order 1) is within a quarter of `x`'s last place
/// of `x`.
fn cubic_tiny<F: Float>(n: &Num) -> bool {
    n.top() < -((F::PREC as i32) / 2 + 2)
}

/// `|x| < 2^-(PREC + 3)`: `1 + c·x` (c below 4) is within a quarter of the last place under 1.
fn linear_tiny<F: Float>(n: &Num) -> bool {
    n.top() < -(F::PREC as i32 + 3)
}

fn signed(v: Mp, neg: bool) -> Mp {
    if neg { v.neg() } else { v }
}

pub(crate) fn one<F: Float>(neg: bool) -> F {
    float::pack(neg, 1, 0)
}

// ------------------------------------------------------------------------------------------
// Exponentials and logarithms.
// ------------------------------------------------------------------------------------------

/// Special operands of the exponentials: NaN, ±∞ (−∞ gives `-1` or `+0`), ±0 (1, or `x`).
fn exp_special<F: Float>(x: F, neg_inf_is_minus_one: bool, zero_is_x: bool) -> Result<Num, (F, Status)> {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return Err(r);
    }
    match n.kind {
        Kind::Inf if n.neg => Err((if neg_inf_is_minus_one { one(true) } else { float::zero(false) }, Status::OK)),
        Kind::Inf => Err((x, Status::OK)),
        Kind::Zero if zero_is_x => Err((x, Status::OK)),
        Kind::Zero => Err((one(false), Status::OK)),
        _ => Ok(n),
    }
}

/// The magnitude of `v` beyond which `e^v` overflows (above the largest finite value) or
/// underflows completely (below half the smallest subnormal, 2^(1 − EMAX − PREC)) in the format:
/// (EMAX + PREC + 2)·ln 2, a factor 4 beyond that half. About 11401.6 for x87 `long double` (smallest
/// subnormal 2^-16445) and 11435.7 for `_Float128` (2^-16494, whose subnormals reach e^-11433:
/// the x87 cut-off would flush them).
fn exp_limit<F: Float>() -> f64 {
    f64::from(F::EMAX + F::PREC as i32 + 2) * core::f64::consts::LN_2
}

/// `e^v`, with arguments beyond ±[`exp_limit`] overflowing or underflowing completely.
fn exp_of<F: Float>(v: Mp, mode: Rounding) -> (F, Status) {
    let e = v.to_f64();
    if e > exp_limit::<F>() {
        return elem::overflow(false, mode);
    }
    if e < -exp_limit::<F>() {
        return elem::underflow(false, mode);
    }
    approx(mp::exp(v), mode)
}

/// Is a finite value an integer; and its value when it fits an `i64` below 2^40.
pub(crate) fn integer(n: &Num) -> (bool, Option<i64>) {
    if n.kind == Kind::Zero {
        return (true, Some(0));
    }
    if n.exp >= 0 {
        let small = n.top() < 40;
        return (true, small.then(|| ((n.sig << n.exp) as i64) * if n.neg { -1 } else { 1 }));
    }
    let f = n.exp.unsigned_abs();
    if f >= 128 || n.sig & ((1u128 << f) - 1) != 0 {
        return (false, None);
    }
    let v = n.sig >> f;
    (true, (v < 1 << 40).then(|| (v as i64) * if n.neg { -1 } else { 1 }))
}

/// `exp(x)` = e^x.
pub fn exp<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = match exp_special(x, false, false) {
        Ok(n) => n,
        Err(r) => return r,
    };
    if linear_tiny::<F>(&n) {
        return near_one(false, !n.neg, mode);
    }
    exp_of(value(&n), mode)
}

/// `exp2(x)` = 2^x; exact for integral `x`.
pub fn exp2<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = match exp_special(x, false, false) {
        Ok(n) => n,
        Err(r) => return r,
    };
    let e = est(&n);
    if e > 20_000.0 {
        return elem::overflow(false, mode);
    }
    if e < -20_000.0 {
        return elem::underflow(false, mode);
    }
    if let (true, Some(k)) = integer(&n) {
        return exact(Mp::from_i64(1).ldexp(k), mode);
    }
    if linear_tiny::<F>(&n) {
        return near_one(false, !n.neg, mode);
    }
    exp_of(value(&n).mul(mp::ln2()), mode)
}

/// `exp10(x)` = 10^x; exact for the integral `x` from 1 to 55 (5^x fits 128 bits: representable
/// up to 27 for x87 `long double` and 48 for `_Float128`, 5^48 < 2^113, and rounded exactly above).
pub fn exp10<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = match exp_special(x, false, false) {
        Ok(n) => n,
        Err(r) => return r,
    };
    let e = est(&n);
    if e > 6_000.0 {
        return elem::overflow(false, mode);
    }
    if e < -6_000.0 {
        return elem::underflow(false, mode);
    }
    if let (true, Some(k @ 1..=55)) = integer(&n) {
        let five = (0..k).fold(1u128, |p, _| p * 5);
        return exact(Mp::from_u128(false, five, k), mode);
    }
    if n.top() < -(F::PREC as i32 + 4) {
        return near_one(false, !n.neg, mode);
    }
    exp_of(value(&n).mul(mp::ln10()), mode)
}

/// `expm1(x)` = e^x − 1, accurate near 0.
pub fn expm1<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = match exp_special(x, true, true) {
        Ok(n) => n,
        Err(r) => return r,
    };
    let e = est(&n);
    if e > 11_400.0 {
        return elem::overflow(false, mode);
    }
    if e < -(F::PREC as f64) * 0.7 - 6.0 {
        // e^x below a quarter of the last place under 1: −1 plus a tiny positive amount.
        return near_one(true, false, mode);
    }
    if linear_tiny::<F>(&n) {
        // x + x²/2: above x.
        return near_x(&n, !n.neg, mode);
    }
    approx(mp::exp(value(&n)).sub(Mp::from_i64(1)), mode)
}

/// Special operands of the logarithms: NaN, negative (domain), ±0 (pole), +∞. The callers pass
/// `positive_nan` for x87 `long double` only: glibc's `logl`, `log2l` and `log10l` return a
/// positive domain NaN, its `logf128`, `log2f128` and `log10f128` the default negative one
/// (probe, D-10.19).
fn log_special<F: Float>(x: F, positive_nan: bool) -> Result<Num, (F, Status)> {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return Err(r);
    }
    match n.kind {
        Kind::Zero => Err(elem::pole(true)),
        _ if n.neg => Err(if positive_nan { elem::domain_pos() } else { elem::domain() }),
        Kind::Inf => Err((x, Status::OK)),
        _ => Ok(n),
    }
}

/// The odd part of a finite nonzero value and the exponent of 2: `|x| = odd · 2^e`.
fn odd_part(n: &Num) -> (u128, i64) {
    let tz = n.sig.trailing_zeros();
    (n.sig >> tz, i64::from(n.exp) + i64::from(tz))
}

/// `log(x)` = ln x. Its domain NaN is positive (glibc's `logl`, D-10.19).
pub fn log<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = match log_special(x, F::EXPLICIT) {
        Ok(n) => n,
        Err(r) => return r,
    };
    if odd_part(&n) == (1, 0) {
        return (float::zero(false), Status::OK);
    }
    approx(mp::ln(value(&n)), mode)
}

/// `log2(x)`; exact for powers of two. Its domain NaN is positive (glibc's `log2l`, D-10.19).
pub fn log2<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = match log_special(x, F::EXPLICIT) {
        Ok(n) => n,
        Err(r) => return r,
    };
    let (odd, e) = odd_part(&n);
    if odd == 1 {
        return exact(Mp::from_i64(e), mode);
    }
    approx(mp::ln(value(&n)).div(mp::ln2()), mode)
}

/// `log10(x)`; exact for the powers of ten the format holds.
pub fn log10<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = match log_special(x, F::EXPLICIT) {
        Ok(n) => n,
        Err(r) => return r,
    };
    let (odd, e) = odd_part(&n);
    let mut five = 1u128;
    for k in 0..=55i64 {
        if odd == five && e == k {
            return exact(Mp::from_i64(k), mode);
        }
        five = five.saturating_mul(5);
    }
    approx(mp::ln(value(&n)).div(mp::ln10()), mode)
}

/// `log1p(x)` = ln(1 + x), accurate near 0.
pub fn log1p<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = float::decode(x);
    if let Some((r, st)) = elem::nan_in(&n) {
        // glibc's `log1pf128` returns a NaN operand with its sign cleared (probe, D-10.19); the
        // x87 form keeps it.
        return (if F::EXPLICIT { r } else { float::with_sign(r, false) }, st);
    }
    match n.kind {
        Kind::Zero => return (x, Status::OK),
        Kind::Inf if !n.neg => return (x, Status::OK),
        Kind::Inf => return elem::domain(),
        _ => {}
    }
    let v = value(&n);
    let one = Mp::from_i64(1);
    if n.neg {
        match v.cmp_abs(one) {
            core::cmp::Ordering::Equal => return elem::pole(true),
            core::cmp::Ordering::Greater => return elem::domain(),
            core::cmp::Ordering::Less => {}
        }
    }
    if linear_tiny::<F>(&n) {
        // x − x²/2: below x.
        return near_x(&n, n.neg, mode);
    }
    approx(mp::ln1p(v), mode)
}

// ------------------------------------------------------------------------------------------
// Powers and roots.
// ------------------------------------------------------------------------------------------

/// `pow(x, y)` = x^y, with the special cases of C Annex F as the `double` form has them.
#[allow(clippy::too_many_lines)] // the Annex F table is long
pub fn pow<F: Float>(x: F, y: F, mode: Rounding) -> (F, Status) {
    let (a, b) = (float::decode(x), float::decode(y));
    let x_is_one = a.kind == Kind::Finite && !a.neg && odd_part(&a) == (1, 0);
    // x^±0 = 1 and 1^y = 1, even for a quiet NaN. A signalling x^±0 in the x87 format is its
    // quiet NaN with invalid, as IEEE 754 and glibc's `powl` have it (X87-NAN-RULE-AUDIT-2).
    let snan_x87 = F::PREC == 64 && a.kind == Kind::SNan;
    if (b.kind == Kind::Zero && !snan_x87) || x_is_one {
        let st = if a.kind == Kind::SNan || b.kind == Kind::SNan { Status::INVALID } else { Status::OK };
        return (one(false), st);
    }
    if let Some(r) = fused::nan_operands::<F, F>(&[a, b]) {
        return r;
    }
    let (y_int, y_odd) = if b.kind == Kind::Finite {
        let (int, _) = integer(&b);
        let odd = int && {
            // The units bit: bit −exp of sig.
            if b.exp > 0 { false } else { b.exp > -128 && (b.sig >> b.exp.unsigned_abs()) & 1 == 1 }
        };
        (int, odd)
    } else {
        (false, false)
    };
    let neg = a.neg && y_odd;
    let ax = Num { neg: false, ..a };
    // y = ±∞.
    if b.kind == Kind::Inf {
        let unit = ax.kind == Kind::Finite && odd_part(&ax) == (1, 0);
        let below = ax.kind == Kind::Zero || (ax.kind == Kind::Finite && ax.top() < 0);
        let r = if unit {
            one(false)
        } else if below == b.neg {
            float::inf(false)
        } else {
            float::zero(false)
        };
        return (r, Status::OK);
    }
    // x = ±0.
    if a.kind == Kind::Zero {
        return if b.neg { elem::pole(neg) } else { (float::zero(neg), Status::OK) };
    }
    // x = ±∞.
    if a.kind == Kind::Inf {
        return (if b.neg { float::zero(neg) } else { float::inf(neg) }, Status::OK);
    }
    if a.neg && !y_int {
        return elem::domain();
    }
    let vy = value(&b);
    let one_mp = Mp::from_i64(1);
    // The exactly computable cases.
    if vy == one_mp {
        return (x, Status::OK);
    }
    if vy == one_mp.neg() {
        let (v, st) = fused::div::<F, F>(one(false), x, mode);
        return (v, float::range_errno(v, st));
    }
    if vy == one_mp.ldexp(-1) {
        return fused::sqrt::<F, F>(float::with_sign(x, false), mode);
    }
    let (odd, e) = odd_part(&ax);
    if odd == 1 {
        // A power of two: 2^(e·y) is exact when e·y is an integer.
        let p = Mp::from_i64(e).mul(vy);
        if p.exp <= 40 && p.frac().is_zero() {
            let k = p.round_i64();
            return exact(signed(one_mp.ldexp(k), neg), mode);
        }
    }
    if let Some(r) = exact_power::<F>(&ax, &b, neg, mode) {
        return r;
    }
    // The general case: e^(y·ln|x|).
    let z = mp::ln(value(&ax)).mul(vy);
    let zf = z.to_f64();
    if zf > exp_limit::<F>() {
        return elem::overflow(neg, mode);
    }
    if zf < -exp_limit::<F>() {
        return elem::underflow(neg, mode);
    }
    if z.exp < -(F::PREC as i64 + 3) {
        // 1 + z beyond resolution: just beside ±1.
        return near_one(neg, !z.neg, mode);
    }
    approx(signed(mp::exp(z), neg), mode)
}

/// `|x|^y` when it is a dyadic number of at most 128 significant bits (exactly representable or
/// not, halfway or not: it is then rounded exactly). `y = m/2^k` with `k <= 6` and `m > 1`: the
/// `2^k`-th root of `|x|` must be exact (an odd part of at most 113 bits is at most a 2^6-th
/// power), then the `m`-th power must fit. A negative `m` gives a dyadic result only for a power
/// of two, handled by the caller.
pub(crate) fn exact_power<F: Float>(ax: &Num, b: &Num, neg: bool, mode: Rounding) -> Option<(F, Status)> {
    if b.neg {
        return None;
    }
    let (ym, yk) = {
        let tz = b.sig.trailing_zeros();
        let (m, e) = (b.sig >> tz, i64::from(b.exp) + i64::from(tz));
        if e >= 0 {
            if e > 8 {
                return None;
            }
            (m << e, 0)
        } else {
            (m, -e)
        }
    };
    if yk > 6 || ym > 200 {
        return None;
    }
    let mut r = value(ax);
    for _ in 0..yk {
        r = mp::exact_sqrt(r)?;
    }
    // r = odd · 2^e exactly (at most 113 significant bits).
    let bits = r.sig_bits();
    let odd = {
        let top = (u128::from(r.m[mp::LIMBS - 1]) << 64) | u128::from(r.m[mp::LIMBS - 2]);
        if bits > 128 {
            return None;
        }
        top >> (128 - bits)
    };
    let e = r.exp - bits;
    let mut p: u128 = 1;
    for _ in 0..ym {
        p = p.checked_mul(odd)?;
    }
    let ex = e.checked_mul(ym as i64)?;
    Some(exact(Mp::from_u128(neg, p, ex), mode))
}

/// `cbrt(x)`, the real cube root; exact for perfect cubes.
pub fn cbrt<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return r;
    }
    if n.kind != Kind::Finite {
        return (x, Status::OK);
    }
    // |x| = m · 2^(3q) with m in [1, 8).
    let top = i64::from(n.top());
    let q = top.div_euclid(3);
    let m = value(&Num { neg: false, ..n }).ldexp(-3 * q);
    let mf = m.to_f64();
    let mut y = 1.5;
    for _ in 0..8 {
        y -= (y * y * y - mf) / (3.0 * y * y);
    }
    // Newton's iteration in 384 bits: 53, 106, 212, 424 bits.
    let mut r = Mp::from_f64(y);
    for _ in 0..4 {
        let r2 = r.sqr();
        r = r.sub(r2.mul(r).sub(m).div(r2.mul_u64(3)));
    }
    // A perfect cube: c³ = m exactly for the root rounded to 120 bits.
    let c = r.round_bits(120);
    if c.sqr().mul(c) == m {
        return exact(signed(c.ldexp(q), n.neg), mode);
    }
    approx(signed(r.ldexp(q), n.neg), mode)
}

// ------------------------------------------------------------------------------------------
// Trigonometric functions.
// ------------------------------------------------------------------------------------------

/// The special operands of `sin`, `cos`, `tan`: NaN, ±∞ (domain), ±0 (1 for `cos`, or `x`).
fn trig_special<F: Float>(x: F, zero_is_one: bool) -> Result<Num, (F, Status)> {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return Err(r);
    }
    match n.kind {
        Kind::Inf => Err(elem::domain()),
        Kind::Zero if zero_is_one => Err((one(false), Status::OK)),
        Kind::Zero => Err((x, Status::OK)),
        _ => Ok(n),
    }
}

fn sin_cos_of(n: &Num) -> (Mp, Mp) {
    mp::sin_cos_wide(n.neg, n.sig, i64::from(n.exp))
}

/// `sin(x)`.
pub fn sin<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = match trig_special(x, false) {
        Ok(n) => n,
        Err(r) => return r,
    };
    if cubic_tiny::<F>(&n) {
        return near_x(&n, false, mode); // x − x³/6
    }
    approx(sin_cos_of(&n).0, mode)
}

/// `cos(x)`.
pub fn cos<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = match trig_special(x, true) {
        Ok(n) => n,
        Err(r) => return r,
    };
    if cubic_tiny::<F>(&n) {
        return near_one(false, false, mode); // 1 − x²/2
    }
    approx(sin_cos_of(&n).1, mode)
}

/// `tan(x)`.
pub fn tan<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = match trig_special(x, false) {
        Ok(n) => n,
        Err(r) => return r,
    };
    if cubic_tiny::<F>(&n) {
        return near_x(&n, true, mode); // x + x³/3
    }
    let (s, c) = sin_cos_of(&n);
    approx(s.div(c), mode)
}

/// `sincos(x)`: `(sin x, cos x)` with the flags of both.
pub fn sincos<F: Float>(x: F, mode: Rounding) -> ((F, F), Status) {
    let (s, st) = sin(x, mode);
    let (c, ct) = cos(x, mode);
    ((s, c), st.and(ct))
}

/// `atan(x)`.
pub fn atan<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return r;
    }
    match n.kind {
        Kind::Zero => return (x, Status::OK),
        Kind::Inf => return approx(signed(mp::pi().ldexp(-1), n.neg), mode),
        _ => {}
    }
    if cubic_tiny::<F>(&n) {
        return near_x(&n, false, mode); // x − x³/3
    }
    approx(signed(mp::atan(value(&n).abs()), n.neg), mode)
}

/// `asin(x)`. Its domain NaN is positive for x87 `long double` (glibc's `asinl`) and the default
/// negative one for `_Float128` (glibc's `asinf128`, D-10.19).
pub fn asin<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return r;
    }
    if n.kind == Kind::Zero {
        return (x, Status::OK);
    }
    let a = value(&n).abs();
    let one = Mp::from_i64(1);
    if n.kind == Kind::Inf || a.cmp_abs(one).is_gt() {
        return arc_domain::<F>();
    }
    if cubic_tiny::<F>(&n) {
        return near_x(&n, true, mode); // x + x³/6
    }
    let r = if a == one {
        mp::pi().ldexp(-1)
    } else {
        // atan(a/√((1 − a)(1 + a))), the factors exact.
        mp::atan(a.div(one.sub(a).mul(one.add(a)).sqrt()))
    };
    approx(signed(r, n.neg), mode)
}

/// The domain NaN of `asin` and `acos`: positive for x87 `long double` (glibc's `asinl`, `acosl`),
/// the default negative one for `_Float128` (glibc's `asinf128`, `acosf128`; D-10.19).
fn arc_domain<F: Float>() -> (F, Status) {
    if F::EXPLICIT { elem::domain_pos() } else { elem::domain() }
}

/// `acos(x)`. A NaN operand comes back quieted, with its sign cleared for x87 `long double`
/// (glibc's `acosl`) and kept for `_Float128` (glibc's `acosf128`, D-10.19).
pub fn acos<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&Num { neg: n.neg && !F::EXPLICIT, ..n }) {
        return r;
    }
    let v = value(&n);
    let one = Mp::from_i64(1);
    if n.kind == Kind::Inf || v.cmp_abs(one).is_gt() {
        return arc_domain::<F>();
    }
    if v == one {
        return (float::zero(false), Status::OK);
    }
    if v == one.neg() {
        return approx(mp::pi(), mode);
    }
    // 2·atan(√((1 − x)/(1 + x))), the differences exact.
    approx(mp::atan(one.sub(v).div(one.add(v)).sqrt()).ldexp(1), mode)
}

/// `atan2(y, x)`: the angle of the point (x, y), with every signed-zero and infinite quadrant
/// of C Annex F.
pub fn atan2<F: Float>(y: F, x: F, mode: Rounding) -> (F, Status) {
    let (b, a) = (float::decode(y), float::decode(x));
    // NaN operands: x's payload first, as glibc returns it.
    if let Some(r) = fused::nan_operands_by::<F, F>(&[b, a], fused::Pick::Order([1, 0, 2])) {
        return r;
    }
    let pi = mp::pi();
    let done = |v: Mp| approx::<F>(signed(v, b.neg), mode);
    match (b.kind, a.kind) {
        (Kind::Zero, _) => {
            return if a.neg { done(pi) } else { (y, Status::OK) };
        }
        (Kind::Inf, Kind::Inf) => return done(if a.neg { pi.mul_u64(3).ldexp(-2) } else { pi.ldexp(-2) }),
        (Kind::Inf, _) | (_, Kind::Zero) => return done(pi.ldexp(-1)),
        (_, Kind::Inf) => {
            return if a.neg { done(pi) } else { (float::zero(b.neg), Status::OK) };
        }
        _ => {}
    }
    if !a.neg && b.top() - a.top() < -(F::PREC as i32 + 8) {
        // |y/x| tiny, x > 0: the angle is y/x less a negligible amount in magnitude.
        let q = crate::arith::div_exact::<F>(&b, &a);
        let (v, flags) = elem_round_below::<F>(q, mode);
        return (v, float::range_errno(v, Status::flags(flags)));
    }
    approx(mp::atan2(value(&b), value(&a)), mode)
}

// ------------------------------------------------------------------------------------------
// Hyperbolic functions.
// ------------------------------------------------------------------------------------------

/// NaN, ±0 and ±∞ of the odd functions that keep them (`sinh`, `asinh`, `tanh` but for ∞).
fn odd_special<F: Float>(x: F, inf_is_one: bool) -> Result<Num, (F, Status)> {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return Err(r);
    }
    match n.kind {
        Kind::Zero => Err((x, Status::OK)),
        Kind::Inf if inf_is_one => Err((one(n.neg), Status::OK)),
        Kind::Inf => Err((x, Status::OK)),
        _ => Ok(n),
    }
}

/// `sinh(x)`.
pub fn sinh<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = match odd_special(x, false) {
        Ok(n) => n,
        Err(r) => return r,
    };
    if cubic_tiny::<F>(&n) {
        return near_x(&n, true, mode); // x + x³/6
    }
    if crate::dd::abs(est(&n)) > 11_400.0 {
        return elem::overflow(n.neg, mode);
    }
    approx(signed(mp::sinh_cosh(value(&n).abs()).0, n.neg), mode)
}

/// `cosh(x)`.
pub fn cosh<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return r;
    }
    match n.kind {
        Kind::Zero => return (one(false), Status::OK),
        Kind::Inf => return (float::inf(false), Status::OK),
        _ => {}
    }
    if cubic_tiny::<F>(&n) {
        return near_one(false, true, mode); // 1 + x²/2
    }
    if crate::dd::abs(est(&n)) > 11_400.0 {
        return elem::overflow(false, mode);
    }
    approx(mp::sinh_cosh(value(&n).abs()).1, mode)
}

/// `tanh(x)`.
pub fn tanh<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = match odd_special(x, true) {
        Ok(n) => n,
        Err(r) => return r,
    };
    if cubic_tiny::<F>(&n) {
        return near_x(&n, false, mode); // x − x³/3
    }
    // 1 − tanh|x| = 2/(e^(2|x|) + 1) < 2^-(PREC + 2) beyond (PREC + 3)·ln2/2.
    if crate::dd::abs(est(&n)) > f64::from(F::PREC + 3) * 0.35 {
        return near_one(n.neg, false, mode);
    }
    let (s, c) = mp::sinh_cosh(value(&n).abs());
    approx(signed(s.div(c), n.neg), mode)
}

/// `asinh(x)` = ln(1 + w), `w = |x| + x²/(1 + √(1 + x²))`.
pub fn asinh<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = match odd_special(x, false) {
        Ok(n) => n,
        Err(r) => return r,
    };
    if cubic_tiny::<F>(&n) {
        return near_x(&n, false, mode); // x − x³/6
    }
    let a = value(&n).abs();
    let one = Mp::from_i64(1);
    let a2 = a.sqr();
    let w = a.add(a2.div(one.add(one.add(a2).sqrt())));
    approx(signed(mp::ln1p(w), n.neg), mode)
}

/// `acosh(x)` = ln(1 + t + √(t(2 + t))), `t = x − 1`.
pub fn acosh<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return r;
    }
    let v = value(&n);
    let one = Mp::from_i64(1);
    if n.neg || n.kind == Kind::Zero || (n.kind == Kind::Finite && v.cmp_abs(one).is_lt()) {
        return elem::domain();
    }
    if n.kind == Kind::Inf {
        return (x, Status::OK);
    }
    if v == one {
        return (float::zero(false), Status::OK);
    }
    let t = v.sub(one);
    let root = t.mul(Mp::from_i64(2).add(t)).sqrt();
    approx(mp::ln1p(t.add(root)), mode)
}

/// `atanh(x)` = ½·ln(1 + 2|x|/(1 − |x|)).
pub fn atanh<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    if float::decode(x).kind == Kind::Inf {
        return elem::domain();
    }
    let n = match odd_special(x, false) {
        Ok(n) => n,
        Err(r) => return r,
    };
    let a = value(&n).abs();
    let one = Mp::from_i64(1);
    match a.cmp_abs(one) {
        core::cmp::Ordering::Greater => return elem::domain(),
        core::cmp::Ordering::Equal => return elem::pole(n.neg),
        core::cmp::Ordering::Less => {}
    }
    if cubic_tiny::<F>(&n) {
        return near_x(&n, true, mode); // x + x³/3
    }
    let q = a.ldexp(1).div(one.sub(a));
    approx(signed(mp::ln1p(q).ldexp(-1), n.neg), mode)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::F80;

    fn f80(v: f64) -> F80 {
        float::convert::<f64, F80>(v, Rounding::Nearest).0
    }

    #[test]
    fn exact_cases_are_exact() {
        let n = Rounding::Nearest;
        assert_eq!(exp2(f80(-3.0), Rounding::Up), (f80(0.125), Status::OK));
        assert_eq!(exp10(f80(22.0), Rounding::Down), (f80(1e22), Status::OK));
        assert_eq!(log2(f80(1024.0), n), (f80(10.0), Status::OK));
        assert_eq!(log10(f80(1e20), Rounding::Zero), (f80(20.0), Status::OK));
        assert_eq!(pow(f80(9.0), f80(1.5), Rounding::Down), (f80(27.0), Status::OK));
        assert_eq!(pow(f80(-3.0), f80(3.0), Rounding::Up), (f80(-27.0), Status::OK));
        assert_eq!(pow(f80(16.0), f80(0.75), Rounding::Up), (f80(8.0), Status::OK));
        assert_eq!(cbrt(f80(-0.125), Rounding::Up), (f80(-0.5), Status::OK));
        assert_eq!(log(f80(1.0), Rounding::Down).0, f80(0.0));
        assert_eq!(acosh(f80(1.0), Rounding::Down).0, f80(0.0));
    }

    #[test]
    fn close_to_the_host() {
        let near = |a: F80, b: f64| {
            let (v, _) = float::convert::<F80, f64>(a, Rounding::Nearest);
            (v - b).abs() <= 4.0 * f64::EPSILON * b.abs()
        };
        for x in [0.3f64, -1.7, 2.5, 10.0, -0.01] {
            assert!(near(exp(f80(x), Rounding::Nearest).0, x.exp()), "exp {x}");
            assert!(near(sin(f80(x), Rounding::Nearest).0, x.sin()), "sin {x}");
            assert!(near(tan(f80(x), Rounding::Nearest).0, x.tan()), "tan {x}");
            assert!(near(sinh(f80(x), Rounding::Nearest).0, x.sinh()), "sinh {x}");
            assert!(near(cbrt(f80(x), Rounding::Nearest).0, x.cbrt()), "cbrt {x}");
            assert!(near(atan2(f80(x), f80(0.7), Rounding::Nearest).0, x.atan2(0.7)), "atan2 {x}");
            assert!(near(pow(f80(1.3), f80(x), Rounding::Nearest).0, 1.3f64.powf(x)), "pow {x}");
        }
        assert!(near(sin(f80(1e300), Rounding::Nearest).0, 1e300f64.sin()), "sin 1e300");
    }
}
