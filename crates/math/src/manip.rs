//! Exponent and sign manipulation, neighbours, NaN payloads and canonical encodings (spec
//! §10.2): `frexp`, `ldexp`/`scalbn`/`scalbln`, `ilogb`/`llogb`/`logb`, `significand`, `scalb`,
//! `nextafter`/`nexttoward`/`nextup`/`nextdown`, `copysign`/`fabs`, `getpayload`/`setpayload`/
//! `setpayloadsig`, `canonicalize`. All results are exact except where scaling leaves the range.

#![allow(clippy::as_conversions, clippy::arithmetic_side_effects)] // exponents and encodings of bounded width

use crate::float::{self, Exact, Float, Kind, Num};
use crate::status::{Flags, Rounding, Status};

/// The result for a NaN operand of a function that otherwise raises nothing: a quiet NaN, with
/// invalid for a signalling one (or an x87 encoding that is not a number).
fn nan_result<F: Float>(n: &Num) -> Option<(F, Status)> {
    match n.kind {
        Kind::QNan => Some((float::quiet(n), Status::OK)),
        Kind::SNan => Some((float::quiet(n), Status::INVALID)),
        Kind::Invalid => Some((float::default_nan(), Status::INVALID)),
        _ => None,
    }
}

/// `frexp`: a fraction in [0.5, 1) (with `x`'s sign) and the exponent; zero, ∞ and NaN give
/// themselves and exponent 0.
pub fn frexp<F: Float>(x: F) -> ((F, i32), Status) {
    let n = float::decode(x);
    if let Some((v, st)) = nan_result::<F>(&n) {
        return ((v, 0), st);
    }
    match n.kind {
        Kind::Finite => {
            let e = n.top() + 1;
            ((float::pack(n.neg, n.sig, n.exp - e), e), Status::OK)
        }
        _ => ((x, 0), Status::OK),
    }
}

/// `ldexp`/`scalbn`/`scalbln`: `x · 2^k`, rounded in `mode` when the result is subnormal.
/// Overflow, and an underflow to zero, report `ERANGE`.
pub fn scale<F: Float>(x: F, k: i64, mode: Rounding) -> (F, Status) {
    let n = float::decode(x);
    if let Some(r) = nan_result::<F>(&n) {
        return r;
    }
    if n.kind != Kind::Finite {
        return (x, Status::OK);
    }
    // Beyond twice the format's exponent span the result saturates either way.
    let span = i64::from(F::EMAX) * 2 + i64::from(F::PREC) + 2;
    let k = k.clamp(-span, span) as i32;
    let (v, flags) = float::round::<F>(Exact { neg: n.neg, sig: n.sig, exp: n.exp + k, sticky: false }, mode);
    (v, float::range_errno(v, Status::flags(flags)))
}

/// `FP_ILOGB0` and `FP_ILOGBNAN` (both `INT_MIN` on x86).
pub const FP_ILOGB0: i32 = i32::MIN;
/// `FP_ILOGBNAN`.
pub const FP_ILOGBNAN: i32 = i32::MIN;

/// The unbiased exponent as a 64-bit value: `ilogb` (`llogb` widens the limits). Zero and NaN give
/// `zero_nan`, ∞ gives `inf`, each with `FE_INVALID` and `EDOM`.
pub fn ilogb_with<F: Float>(x: F, zero_nan: i64, inf: i64) -> (i64, Status) {
    let n = float::decode(x);
    match n.kind {
        Kind::Finite => (i64::from(n.top()), Status::OK),
        Kind::Inf => (inf, Status::DOMAIN),
        _ => (zero_nan, Status::DOMAIN),
    }
}

/// `ilogb`.
pub fn ilogb<F: Float>(x: F) -> (i32, Status) {
    let (v, st) = ilogb_with(x, i64::from(FP_ILOGB0), i64::from(i32::MAX));
    (v as i32, st)
}

/// `llogb` (`FP_LLOGB0` = `FP_LLOGBNAN` = `LONG_MIN`).
pub fn llogb<F: Float>(x: F) -> (i64, Status) {
    ilogb_with(x, i64::MIN, i64::MAX)
}

/// `logb`: the exponent as a floating value; `logb(±0)` is −∞ with `FE_DIVBYZERO` and no
/// `errno` (glibc's documented behaviour, `logb(3)`).
pub fn logb<F: Float>(x: F) -> (F, Status) {
    let n = float::decode(x);
    if let Some(r) = nan_result::<F>(&n) {
        return r;
    }
    match n.kind {
        Kind::Zero => (float::inf(true), Status::flags(Flags::DIVBYZERO)),
        Kind::Inf => (float::inf(false), Status::OK),
        _ => {
            let e = n.top();
            (float::pack(e < 0, u128::from(e.unsigned_abs()), 0), Status::OK)
        }
    }
}

/// `significand(x)` = `scalb(x, −ilogb(x))`: the significand in [1, 2). Zero, ∞ and NaN return
/// themselves; the flags they raise follow glibc's per format (observed,
/// `spec/behaviour/libm-exact.glibc.txt.gz`): for `float` and `double`, invalid and `EDOM` (from
/// `ilogb`, plus inexact for `float`'s ∞); for `long double`, division by zero for a zero and
/// nothing else beyond a signalling NaN's invalid.
pub fn significand<F: Float>(x: F) -> (F, Status) {
    let n = float::decode(x);
    let x87 = F::EXPLICIT;
    match n.kind {
        Kind::Finite => (float::pack(n.neg, n.sig, n.exp - n.top()), Status::OK),
        Kind::Zero if x87 => (x, Status::flags(Flags::DIVBYZERO)),
        Kind::Inf if x87 => (x, Status::OK),
        Kind::Inf if F::PREC == 24 => (x, Status::DOMAIN.and(Status::flags(Flags::INEXACT))),
        Kind::Zero | Kind::Inf => (x, Status::DOMAIN),
        _ if x87 => nan_result::<F>(&n).unwrap_or((x, Status::OK)),
        _ => {
            let (v, _) = nan_result::<F>(&n).unwrap_or((x, Status::OK));
            (v, Status::DOMAIN)
        }
    }
}

/// `scalb(x, y)`: `x · 2^y` for an integral `y`. A non-integral `y` is a domain error;
/// `scalb(0, +∞)` and `scalb(∞, −∞)` are invalid; `scalb(x, +∞)` is ±∞ and `scalb(x, −∞)` ±0.
pub fn scalb<F: Float>(x: F, y: F, mode: Rounding) -> (F, Status) {
    let (a, b) = (float::decode(x), float::decode(y));
    if let Some(r) = crate::fused::nan_operands::<F, F>(&[a, b]) {
        return r;
    }
    // A finite non-integral y is a domain error whatever x is (glibc raises inexact too).
    if b.kind == Kind::Finite && crate::round::integral(y, crate::round::Dir::Zero, false).0 != y {
        return (float::default_nan(), Status::DOMAIN.and(Status::flags(Flags::INEXACT)));
    }
    let dom = (float::default_nan(), Status::DOMAIN);
    match (a.kind, b.kind) {
        (Kind::Zero, Kind::Inf) if !b.neg => dom,
        (Kind::Inf, Kind::Inf) if b.neg => dom,
        (Kind::Zero | Kind::Inf, _) => (x, Status::OK),
        (_, Kind::Inf) if b.neg => (float::zero(a.neg), Status::OK),
        (_, Kind::Inf) => (float::inf(a.neg), Status::OK),
        // x itself, canonical (a pseudo-denormal comes back normalized, X87-NAN-RULE-AUDIT-2).
        (_, Kind::Zero) => (if a.kind == Kind::Finite { float::pack(a.neg, a.sig, a.exp) } else { x }, Status::OK),
        _ => {
            // Beyond 2^62 the scale saturates anyway.
            let k = if b.top() > 62 { i64::MAX } else { (b.sig >> (-b.exp).max(0) << b.exp.max(0)) as i64 };
            scale(x, if b.neg { -k } else { k }, mode)
        }
    }
}

/// The neighbour of a finite or infinite non-NaN `x` toward +∞ (`up`) or −∞.
fn step<F: Float>(x: F, up: bool) -> F {
    let n = float::decode(x);
    // An x87 pseudo-denormal steps within its own encoding, as glibc's `nextupl` does: one unit of
    // the significand, the exponent field left at zero.
    let b = x.to_bits();
    if F::EXPLICIT && n.kind == Kind::Finite && b >> 64 & 0x7fff == 0 && b >> 63 & 1 == 1 {
        let away = up != n.neg;
        if !away || b & u128::from(u64::MAX) < u128::from(u64::MAX) {
            return F::from_bits(if away { b + 1 } else { b - 1 });
        }
    }
    match n.kind {
        Kind::Zero => float::pack(!up, 1, float::lsb_min::<F>()),
        Kind::Inf if n.neg == !up => x,
        Kind::Inf => float::max_finite(n.neg),
        _ => {
            // Away from zero adds one unit in the last place, toward zero removes one; with an
            // explicit leading bit (x87) the significand is renormalized by `pack`.
            let away = up != n.neg;
            let (sig, exp) = if away {
                (n.sig + 1, n.exp)
            } else if n.sig == float::lead_bit::<F>() && n.exp > float::lsb_min::<F>() {
                // At a power of two the step below is half as large.
                ((n.sig << 1) - 1, n.exp - 1)
            } else {
                (n.sig - 1, n.exp)
            };
            if sig >> F::PREC != 0 {
                if n.exp + 1 > F::EMAX - (F::PREC as i32 - 1) {
                    return float::inf(n.neg);
                }
                return float::pack(n.neg, sig >> 1, exp + 1);
            }
            float::pack(n.neg, sig, exp)
        }
    }
}

/// `nextup` (`up`) and `nextdown`: the adjacent value toward ±∞, raising nothing.
pub fn next<F: Float>(x: F, up: bool) -> (F, Status) {
    let n = float::decode(x);
    if let Some(r) = nan_result::<F>(&n) {
        return r;
    }
    (step(x, up), Status::OK)
}

/// The step of `nextafter`/`nexttoward` from a non-NaN `x` toward a non-NaN target that compares
/// to it as `dir`; `equal` is the result when they are equal (the target in `x`'s format, so
/// `nextafter(0, −0)` is −0). Overflow, and a subnormal or zero result, raise overflow or
/// underflow with inexact, and `ERANGE`, except from a zero `x` (glibc raises the flags only).
fn next_step<F: Float>(x: F, dir: core::cmp::Ordering, equal: F) -> (F, Status) {
    use core::cmp::Ordering;
    let r = match dir {
        Ordering::Equal => return (equal, Status::OK),
        Ordering::Greater => step(x, true),
        Ordering::Less => step(x, false),
    };
    let m = float::decode(r);
    let flags = match m.kind {
        Kind::Inf => Flags(Flags::OVERFLOW.0 | Flags::INEXACT.0),
        Kind::Zero => Flags(Flags::UNDERFLOW.0 | Flags::INEXACT.0),
        // A zero exponent field (an x87 pseudo-denormal included) is the subnormal range.
        Kind::Finite if m.top() < float::emin::<F>() || r.to_bits() >> float::frac_bits::<F>() & 0x7fff == 0 => Flags(Flags::UNDERFLOW.0 | Flags::INEXACT.0),
        _ => Flags::NONE,
    };
    let st = Status::flags(flags);
    (r, if float::decode(x).kind == Kind::Zero { st } else { st.range_errno() })
}

/// `nextafter(x, y)`: the neighbour of `x` toward `y`. NaN operands propagate (for `float` and
/// `double` glibc returns `y`'s payload first).
pub fn nextafter<F: Float>(x: F, y: F) -> (F, Status) {
    let (a, b) = (float::decode(x), float::decode(y));
    if let Some(r) = crate::fused::nan_operands_by::<F, F>(&[a, b], crate::fused::Pick::Order([1, 0, 2])) {
        return r;
    }
    match compare(&b, &a) {
        Some(dir) => next_step(x, dir, y),
        None => (float::default_nan(), Status::OK),
    }
}

/// `nexttoward(x, y)`: `nextafter` toward a `long double` `y`, compared exactly (every format's
/// values are x87 values too).
pub fn nexttoward<F: Float>(x: F, y: crate::float::F80) -> (F, Status) {
    use crate::float::F80;
    let (a, b) = (float::decode(x), float::decode(y));
    let nan_a = a.is_nan() || a.kind == Kind::Invalid;
    if F::PREC == 64 && (nan_a || b.is_nan() || b.kind == Kind::Invalid) {
        // `nexttowardl` is `nextafterl`: both operands under the x87 unit's NaN rule, as glibc
        // has it (X87-NAN-RULE-AUDIT-2).
        return crate::fused::nan_operands::<F80, F>(&[a, b]).unwrap_or((float::default_nan(), Status::INVALID));
    }
    if nan_a {
        return nan_result::<F>(&a).unwrap_or((float::default_nan(), Status::OK));
    }
    if b.is_nan() || b.kind == Kind::Invalid {
        return crate::fused::nan_operands::<F80, F>(&[b]).unwrap_or((float::default_nan(), Status::OK));
    }
    let (wide, _) = float::convert::<F, F80>(x, Rounding::Nearest);
    let (equal, _) = float::convert::<F80, F>(y, Rounding::Nearest);
    // nexttowardl(x, x) returns y as it is, a pseudo-denormal too (probed, X87-NAN-RULE-AUDIT-2).
    let equal = if F::PREC == 64 { F::from_bits(y.to_bits()) } else { equal };
    match compare(&b, &float::decode(wide)) {
        Some(dir) => next_step(x, dir, equal),
        None => (float::default_nan(), Status::OK),
    }
}

/// The order of two non-NaN decoded values (−0 equals +0).
pub fn compare(a: &Num, b: &Num) -> Option<core::cmp::Ordering> {
    use core::cmp::Ordering;
    let mag = |n: &Num| -> (u8, i32, u128) {
        match n.kind {
            Kind::Zero => (0, 0, 0),
            Kind::Finite => {
                // Normalize to compare (top exponent, significand aligned to 128 bits).
                let top = n.top();
                (1, top, n.sig << n.sig.leading_zeros())
            }
            Kind::Inf => (2, 0, 0),
            _ => (3, 0, 0),
        }
    };
    if a.is_nan() || b.is_nan() {
        return None;
    }
    let (ma, mb) = (mag(a), mag(b));
    let neg_a = a.neg && ma.0 != 0;
    let neg_b = b.neg && mb.0 != 0;
    Some(match (neg_a, neg_b) {
        (false, true) => Ordering::Greater,
        (true, false) => Ordering::Less,
        (false, false) => ma.cmp(&mb),
        (true, true) => mb.cmp(&ma),
    })
}

/// `copysign`: `x`'s magnitude with `y`'s sign bit; exact, raising nothing even for a
/// signalling NaN.
pub fn copysign<F: Float>(x: F, y: F) -> F {
    float::with_sign(x, y.to_bits() & float::sign_bit::<F>() != 0)
}

/// `fabs`: the sign bit cleared, raising nothing.
pub fn fabs<F: Float>(x: F) -> F {
    float::with_sign(x, false)
}

/// `getpayload`: a NaN's payload as a non-negative integral value, or −1 for a non-NaN.
/// Raises nothing.
pub fn getpayload<F: Float>(x: F) -> F {
    let n = float::decode(x);
    if n.is_nan() { float::pack(false, n.sig, 0) } else { float::pack(true, 1, 0) }
}

/// `setpayload` (`signalling` false) and `setpayloadsig`: the NaN with payload `p`, if `p` is a
/// non-negative integer that fits the payload (and, for a signalling NaN, is nonzero); otherwise
/// `None` (the result is then +0 and the return value 1).
pub fn setpayload<F: Float>(p: F, signalling: bool) -> Option<F> {
    let n = float::decode(p);
    let payload = match n.kind {
        Kind::Zero if !n.neg => 0,
        Kind::Finite if !n.neg && n.exp >= 0 && n.top() < F::PREC as i32 - 2 => n.sig << n.exp,
        Kind::Finite if !n.neg && n.exp < 0 && n.sig.trailing_zeros() as i32 >= -n.exp => n.sig >> -n.exp,
        _ => return None,
    };
    if payload >= float::quiet_bit::<F>() || (signalling && payload == 0) {
        return None;
    }
    Some(float::nan(false, !signalling, payload))
}

/// `canonicalize`: the canonical encoding of `x` (a signalling NaN is quieted, with invalid), or
/// `None` for an x87 encoding that represents no value (the destination is then left alone and
/// the return value is 1).
pub fn canonicalize<F: Float>(x: F) -> Option<(F, Status)> {
    let n = float::decode(x);
    match n.kind {
        Kind::Invalid => None,
        Kind::SNan => Some((float::quiet(&n), Status::INVALID)),
        Kind::Finite => Some((float::pack(n.neg, n.sig, n.exp), Status::OK)),
        _ => Some((x, Status::OK)),
    }
}

/// `nan(tag)`: the positive quiet NaN whose payload is the low bits of `payload` (the value of
/// the tag as `strtoull` reads it, 0 for a tag that is not a number).
pub fn nan_payload<F: Float>(payload: u128) -> F {
    float::nan(false, true, payload)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp, clippy::arithmetic_side_effects, clippy::as_conversions)]
mod tests {
    use super::*;
    use crate::float::F80;
    use crate::status::MathError;

    #[test]
    fn exponent_functions() {
        assert_eq!(frexp(8.0f64).0, (0.5, 4));
        assert_eq!(frexp(5e-324f64).0, (0.5, -1073));
        assert_eq!(scale(1.0f64, 1024, Rounding::Nearest), (f64::INFINITY, Status { flags: Flags(0x28), error: MathError::Range, hw: Status::HW_ANY }));
        assert_eq!(scale(1.5f64, -1074, Rounding::Nearest).0.to_bits(), 2);
        assert_eq!(scale(1.5f64, -1074, Rounding::Nearest).1, Status::flags(Flags(0x30)));
        assert_eq!(scale(1.0f32, i64::MIN, Rounding::Up).0.to_bits(), 1);
        assert_eq!(ilogb(0.0f64), (i32::MIN, Status::DOMAIN));
        assert_eq!(ilogb(3.0f32), (1, Status::OK));
        assert_eq!(logb(0.0f64), (f64::NEG_INFINITY, Status::flags(Flags::DIVBYZERO)));
        assert_eq!(logb(5e-324f64).0, -1074.0);
        assert_eq!(significand(-12.0f64).0, -1.5);
    }

    #[test]
    fn neighbours() {
        assert_eq!(next(0.0f64, true).0.to_bits(), 1);
        assert_eq!(next(1.0f64, false).0, 1.0 - f64::EPSILON / 2.0);
        assert_eq!(next(f64::MAX, true).0, f64::INFINITY);
        assert_eq!(next(-f64::MAX, true).0, -f64::MAX + 2f64.powi(971));
        assert_eq!(next(f64::INFINITY, true).0, f64::INFINITY);
        assert_eq!(next(f64::NEG_INFINITY, true).0, f64::MIN);
        assert_eq!(nextafter(f64::MIN_POSITIVE, 0.0).0.to_bits(), 0x000f_ffff_ffff_ffff);
        assert_eq!(nextafter(f64::MIN_POSITIVE, 0.0).1, Status { flags: Flags(0x30), error: MathError::Range, hw: Status::HW_ANY });
        // x87: the explicit leading bit survives a carry into the next binade.
        let below_two = F80((0x3fffu128 << 64) | u128::from(u64::MAX));
        assert_eq!(next(below_two, true).0, F80((0x4000u128 << 64) | (1u128 << 63)));
    }

    #[test]
    fn payloads() {
        assert_eq!(getpayload(f64::from_bits(0x7ff8_0001_2345_6789)), 4_886_718_345.0);
        assert_eq!(getpayload(1.0f64), -1.0);
        assert_eq!(setpayload(1.0f64, false).map(f64::to_bits), Some(0x7ff8_0000_0000_0001));
        assert_eq!(setpayload(0.0f64, true), None);
        assert_eq!(setpayload(-1.0f64, false), None);
        assert_eq!(setpayload(2f64.powi(51), false), None);
        let pd = F80((1u128 << 63) | 5);
        assert_eq!(canonicalize(pd), Some((F80((1u128 << 64) | (1u128 << 63) | 5), Status::OK)));
        assert_eq!(canonicalize(F80(0x3fffu128 << 64)), None);
    }
}
