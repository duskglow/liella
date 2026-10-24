//! Exact remainders (spec §10.2): `fmod` (quotient truncated), `remainder`/`drem` (quotient
//! rounded to nearest, ties to even) and `remquo` (`remainder` plus the low three bits of the
//! quotient with its sign, as glibc returns them). The remainder of two floating values is always
//! representable, so the results are exact.
//!
//! The quotient's binary expansion is produced a few bits at a time, `r = (r · 2^k) mod y`, with
//! `k` chosen so the shifted remainder fits 128 bits; the exponent difference of the operands can
//! be up to 32 766 bits (x87, binary128).

#![allow(clippy::as_conversions, clippy::arithmetic_side_effects)] // significands and exponents of bounded width

use crate::float::{self, Exact, Float, Kind, Num};
use crate::fused::{nan_operands_by, Pick};
use crate::status::{MathError, Rounding, Status};

/// `|x| mod |y|` for finite nonzero operands: the remainder's significand at the exponent of
/// `y`'s normalized least significant bit, and the low 64 bits of the truncated quotient. `None`
/// when `|x| < |y|` (the remainder is `x` and the quotient 0).
fn mod_core<F: Float>(a: &Num, b: &Num) -> Option<(u128, i32, u64)> {
    let prec = F::PREC as i32;
    // Normalize both significands to PREC bits.
    let norm = |n: &Num| {
        let shift = prec - (128 - n.sig.leading_zeros() as i32);
        (n.sig << shift, n.exp - shift)
    };
    let ((mx, ex), (my, ey)) = (norm(a), norm(b));
    if ex < ey || (ex == ey && mx < my) {
        return None;
    }
    let mut d = ex - ey;
    let mut r = mx % my;
    let mut q = (mx / my) as u64;
    let chunk = 127 - prec;
    while d > 0 {
        let k = d.min(chunk);
        let t = r << k;
        q = q.checked_shl(k as u32).unwrap_or(0).wrapping_add((t / my) as u64);
        r = t % my;
        d -= k;
    }
    Some((r, ey, q))
}

/// The special cases shared by the three functions: NaN operands, an infinite `x` or a zero `y`
/// (invalid), an infinite `y` or a zero `x` (the result is `x`). `errno` selects `EDOM` for the
/// invalid cases.
fn special<F: Float>(x: F, a: &Num, b: &Num, errno: bool, pick: Pick) -> Option<(F, Status)> {
    if let Some(r) = nan_operands_by::<F, F>(&[*a, *b], pick) {
        return Some(r);
    }
    let invalid = (float::default_nan(), Status::INVALID.with(if errno { MathError::Domain } else { MathError::None }));
    match (a.kind, b.kind) {
        (Kind::Inf, _) | (_, Kind::Zero) => Some(invalid),
        // x itself, canonical: a pseudo-denormal comes back normalized as glibc returns it
        // (X87-NAN-RULE-AUDIT-2).
        (Kind::Zero, _) | (_, Kind::Inf) => Some((if a.kind == Kind::Finite { float::pack(a.neg, a.sig, a.exp) } else { x }, Status::OK)),
        _ => None,
    }
}

/// An exact value with a sign, packed into `F`.
fn exact<F: Float>(neg: bool, sig: u128, exp: i32) -> F {
    float::round::<F>(Exact { neg, sig, exp, sticky: false }, Rounding::Nearest).0
}

/// `fmod(x, y)`: `x − n·y` with `n` the quotient truncated toward zero; the sign of `x`.
pub fn fmod<F: Float>(x: F, y: F) -> (F, Status) {
    let (a, b) = (float::decode(x), float::decode(y));
    if let Some(r) = special(x, &a, &b, true, Pick::Format) {
        return r;
    }
    match mod_core::<F>(&a, &b) {
        // |x| < |y|: x itself, canonically encoded (glibc canonicalizes an x87 pseudo-denormal).
        None => (exact(a.neg, a.sig, a.exp), Status::OK),
        Some((r, e, _)) => (exact(a.neg, r, e), Status::OK),
    }
}

/// A finite nonzero significand shifted to exactly `PREC` bits, with its exponent.
fn normalized<F: Float>(n: &Num) -> (u128, i32) {
    let shift = F::PREC as i32 - (128 - n.sig.leading_zeros() as i32);
    (n.sig << shift, n.exp - shift)
}

/// The remainder rounded to nearest and the quotient's low three bits (with the quotient's sign).
fn remainder_core<F: Float>(a: &Num, b: &Num) -> (F, i64) {
    let (ys, ye) = normalized::<F>(b);
    // r = |x| mod |y| at y's exponent, or |x| itself when |x| < |y|.
    let (rs, re, q) = mod_core::<F>(a, b).unwrap_or_else(|| {
        let (s, e) = normalized::<F>(a);
        (s, e, 0)
    });
    // 2r against |y|: r is below |y|, so its exponent is at most y's.
    let ord = match ye - re {
        0 => (rs << 1).cmp(&ys),
        1 => rs.cmp(&ys),
        _ => core::cmp::Ordering::Less,
    };
    let up = match ord {
        core::cmp::Ordering::Greater => true,
        core::cmp::Ordering::Equal => q & 1 == 1,
        core::cmp::Ordering::Less => false,
    };
    let bits = (q.wrapping_add(u64::from(up)) & 7) as i64;
    let quo = if a.neg != b.neg { -bits } else { bits };
    if !up {
        return (exact(a.neg, rs, re), quo);
    }
    // |y| − r, which crossed zero: the opposite sign of x.
    let (diff, e) = if re == ye { (ys - rs, ye) } else { ((ys << 1) - rs, re) };
    (exact(!a.neg, diff, e), quo)
}

/// `remainder(x, y)` (and `drem`): `x − n·y` with `n` the nearest integer to `x/y`, ties to even.
/// A zero result has `x`'s sign.
pub fn remainder<F: Float>(x: F, y: F) -> (F, Status) {
    let (a, b) = (float::decode(x), float::decode(y));
    if let Some(r) = special(x, &a, &b, true, if F::PREC == 53 { Pick::Order([1, 0, 2]) } else { Pick::Format }) {
        return r;
    }
    (remainder_core::<F>(&a, &b).0, Status::OK)
}

/// `remquo(x, y)`: `remainder` and the low three bits of the quotient, with its sign. The
/// quotient is `None` where glibc leaves the destination unwritten (NaN operands, an infinite `x`,
/// a zero `y`) and `errno` is never set (glibc's documented behaviour, `remquo(3)`).
pub fn remquo<F: Float>(x: F, y: F) -> ((F, Option<i32>), Status) {
    let (a, b) = (float::decode(x), float::decode(y));
    if let Some((v, st)) = special(x, &a, &b, false, Pick::Format)
        // remquol returns a pseudo-denormal x beside ±∞ as it is (probed, X87-NAN-RULE-AUDIT-2).
        .map(|(v, st)| (if b.kind == Kind::Inf && a.kind == Kind::Finite { x } else { v }, st))
    {
        let written = !(a.is_nan() || b.is_nan() || a.kind == Kind::Invalid || b.kind == Kind::Invalid || a.kind == Kind::Inf || b.kind == Kind::Zero);
        return ((v, written.then_some(0)), st);
    }
    let (v, q) = remainder_core::<F>(&a, &b);
    ((v, Some(q as i32)), Status::OK)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp, clippy::arithmetic_side_effects, clippy::as_conversions)]
mod tests {
    use super::*;

    #[test]
    fn fmod_agrees_with_the_hardware_route() {
        // Rust's `%` on f64 computes the exact fmod (it calls the C library's fmod, which is
        // exact by contract); compare over operands spanning many binades.
        let mut s = 0x0123_4567_89ab_cdefu64;
        for _ in 0..100_000 {
            s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let x = f64::from_bits((s & 0x800f_ffff_ffff_ffff) | ((0x380 + (s >> 54) % 0x100) << 52));
            s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
            let y = f64::from_bits((s & 0x800f_ffff_ffff_ffff) | ((0x380 + (s >> 54) % 0x100) << 52));
            assert_eq!(fmod(x, y).0.to_bits(), (x % y).to_bits(), "{x:e} {y:e}");
        }
        assert_eq!(fmod(f64::MAX, 5e-324).0, 0.0);
        assert_eq!(fmod(-5.5f32, 2.0).0, -1.5);
    }

    #[test]
    fn remainders() {
        assert_eq!(remainder(5.0f64, 2.0).0, 1.0);
        assert_eq!(remainder(7.0f64, 2.0).0, -1.0);
        assert_eq!(remainder(-2.5f64, 1.5).0, 0.5);
        assert_eq!(remquo(-1007.0f64, 1.0).0, (-0.0, Some(-7)));
        assert_eq!(remquo(1002.5f64, 1.0).0, (0.5, Some(2)));
        assert_eq!(remquo(3.0f64, -2.5).0, (0.5, Some(-1)));
        assert_eq!(remquo(1.0f64, 0.0).0 .1, None);
        assert_eq!(remainder(1.0f64, f64::INFINITY).0, 1.0);
        assert_eq!(remainder(f64::MAX, 5e-324).0, 0.0);
    }
}
