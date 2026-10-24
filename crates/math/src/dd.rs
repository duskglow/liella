//! Double-double arithmetic: a value as the unevaluated sum `hi + lo` of two `f64` with
//! `|lo| <= ulp(hi)/2`, about 106 bits of precision. The elementary functions of `float` and
//! `double` (spec §10.3) compute in it and round once at the end (`elem`).
//!
//! The error-free transformations (Knuth's two-sum, Dekker's split and product, from the
//! literature on floating-point arithmetic) are exact only in round-to-nearest, which the
//! translation layer guarantees while a function runs (`mathret::elem`, D-10.13). They do not use
//! a fused multiply-add: baseline x86-64 has none, and calling `fma` would reach the software one.
//! Magnitudes stay below 2^995, where Dekker's split cannot overflow; callers scale.

#![allow(clippy::arithmetic_side_effects)] // exponents and integer parts of bounded range

use crate::float::{self, Exact, Float, Kind};
use crate::status::{Flags, Rounding};

/// A double-double value.
#[derive(Clone, Copy, PartialEq, Debug, Default)]
pub struct Dd {
    /// The leading part.
    pub hi: f64,
    /// The trailing part.
    pub lo: f64,
}

/// `a + b` exactly, as a normalized pair (Knuth).
#[inline]
pub fn two_sum(a: f64, b: f64) -> Dd {
    let s = a + b;
    let bb = s - a;
    let e = (a - (s - bb)) + (b - bb);
    Dd { hi: s, lo: e }
}

/// `a + b` exactly when `|a| >= |b|` (or `a` is zero).
#[inline]
pub fn fast_two_sum(a: f64, b: f64) -> Dd {
    let s = a + b;
    Dd { hi: s, lo: b - (s - a) }
}

/// Dekker's split of `a` into two halves of 26 bits each.
#[inline]
fn split(a: f64) -> (f64, f64) {
    const C: f64 = 134_217_729.0; // 2^27 + 1
    let t = C * a;
    let hi = t - (t - a);
    (hi, a - hi)
}

/// `a * b` exactly (Dekker).
#[inline]
pub fn two_prod(a: f64, b: f64) -> Dd {
    let p = a * b;
    let (ah, al) = split(a);
    let (bh, bl) = split(b);
    let e = ((ah * bh - p) + ah * bl + al * bh) + al * bl;
    Dd { hi: p, lo: e }
}

// Named methods, not the operator traits: chains read in evaluation order, and each name is a
// double-double operation of known cost.
#[allow(clippy::should_implement_trait)]
impl Dd {
    /// From a stored constant (two `f64` bit patterns).
    #[inline]
    pub const fn of(c: [u64; 2]) -> Dd {
        Dd { hi: f64::from_bits(c[0]), lo: f64::from_bits(c[1]) }
    }

    /// An `f64`.
    #[inline]
    pub const fn from(x: f64) -> Dd {
        Dd { hi: x, lo: 0.0 }
    }

    /// The nearest `f64`.
    #[inline]
    pub fn to_f64(self) -> f64 {
        self.hi + self.lo
    }

    /// Negation.
    #[inline]
    #[must_use]
    pub fn neg(self) -> Dd {
        Dd { hi: -self.hi, lo: -self.lo }
    }

    /// Sum (relative error about 2^-105 when no cancellation: the "accurate" addition).
    #[inline]
    #[must_use]
    pub fn add(self, o: Dd) -> Dd {
        let s = two_sum(self.hi, o.hi);
        let t = two_sum(self.lo, o.lo);
        let u = fast_two_sum(s.hi, s.lo + t.hi);
        fast_two_sum(u.hi, u.lo + t.lo)
    }

    /// Sum with an `f64`.
    #[inline]
    #[must_use]
    pub fn add_f64(self, b: f64) -> Dd {
        let s = two_sum(self.hi, b);
        fast_two_sum(s.hi, s.lo + self.lo)
    }

    /// Difference.
    #[inline]
    #[must_use]
    pub fn sub(self, o: Dd) -> Dd {
        self.add(o.neg())
    }

    /// Product.
    #[inline]
    #[must_use]
    pub fn mul(self, o: Dd) -> Dd {
        let p = two_prod(self.hi, o.hi);
        fast_two_sum(p.hi, p.lo + (self.hi * o.lo + self.lo * o.hi))
    }

    /// Product with an `f64`.
    #[inline]
    #[must_use]
    pub fn mul_f64(self, b: f64) -> Dd {
        let p = two_prod(self.hi, b);
        fast_two_sum(p.hi, p.lo + self.lo * b)
    }

    /// Product with a power of two (exact barring overflow and underflow).
    #[inline]
    #[must_use]
    pub fn scale(self, s: f64) -> Dd {
        Dd { hi: self.hi * s, lo: self.lo * s }
    }

    /// Quotient (two Newton-like correction steps of the long division).
    #[inline]
    #[must_use]
    pub fn div(self, o: Dd) -> Dd {
        let q1 = self.hi / o.hi;
        let r = self.sub(o.mul_f64(q1));
        let q2 = r.hi / o.hi;
        let r = r.sub(o.mul_f64(q2));
        let q3 = r.hi / o.hi;
        fast_two_sum(q1, q2).add_f64(q3)
    }

    /// Square.
    #[inline]
    #[must_use]
    pub fn sqr(self) -> Dd {
        self.mul(self)
    }
}

/// 1.5 · 2^52: adding and subtracting it rounds an `f64` of magnitude below 2^51 to an integer
/// (in round-to-nearest, ties to even). `core` has no `round` without the C library.
const ROUNDER: f64 = 6_755_399_441_055_744.0;

/// The integer nearest `x` (ties to even), for `|x| < 2^51`.
#[inline]
pub fn nearest_int(x: f64) -> f64 {
    (x + ROUNDER) - ROUNDER
}

/// An integral `f64` of magnitude below 2^31 as `i32`.
#[inline]
pub fn to_i32(x: f64) -> i32 {
    #[allow(clippy::as_conversions, clippy::cast_possible_truncation)] // integral and in range
    let v = x as i32;
    v
}

/// `2^k` for -1022 <= k <= 1023.
#[inline]
pub fn pow2(k: i32) -> f64 {
    #[allow(clippy::as_conversions, clippy::cast_sign_loss)] // biased exponent in 1..=2046
    let b = ((k + 1023) as u64) << 52;
    f64::from_bits(b)
}

/// `x · 2^k` for |k| <= 2044 (two exact multiplications, barring overflow and underflow).
#[inline]
pub fn ldexp(x: f64, k: i32) -> f64 {
    let h = k / 2;
    x * pow2(h.clamp(-1022, 1023)) * pow2((k - h).clamp(-1022, 1023))
}

/// `|x|`.
#[inline]
pub fn abs(x: f64) -> f64 {
    f64::from_bits(x.to_bits() & !(1u64 << 63))
}

/// The square root of a positive double-double of moderate magnitude (2^-1000 to 2^1000): an
/// inverse square root by Newton's iteration from a bit-level estimate, then one correction in
/// double-double (about 2^-104).
pub fn sqrt(x: Dd) -> Dd {
    if x.hi <= 0.0 {
        return Dd::from(0.0);
    }
    // Estimate 1/sqrt(x.hi) from the halved exponent, then four Newton steps (3.4% -> 2^-60).
    let mut y = f64::from_bits(0x5fe6_eb50_c7b5_37a9u64.wrapping_sub(x.hi.to_bits() >> 1));
    for _ in 0..4 {
        y *= 1.5 - 0.5 * x.hi * y * y;
    }
    let s = x.hi * y;
    let e = x.sub(two_prod(s, s));
    fast_two_sum(s, e.hi / (2.0 * s))
}

/// Horner evaluation of `sum c[i] x^i` where the terms from index `split` up are small enough
/// to be evaluated in plain `f64` (with `x.hi`) and the leading ones in double-double.
#[inline]
pub fn horner(x: Dd, c: &[[u64; 2]], split: usize) -> Dd {
    let mut tail = 0.0f64;
    for k in c.iter().skip(split).rev() {
        tail = tail * x.hi + f64::from_bits(k[0]);
    }
    let mut acc = Dd::from(tail);
    for k in c.iter().take(split).rev() {
        acc = acc.mul(x).add(Dd::of(*k));
    }
    acc
}

/// How the true value relates to an approximation passed to [`round`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Residual {
    /// The approximation is the exact value.
    Exact,
    /// The true value is slightly above (or the sign is unknown: a nonzero residual far below
    /// the approximation's precision).
    Above,
    /// The true value is slightly below.
    Below,
}

/// Round `v · 2^scale` into `F` in `mode`: the result and the flags (inexact, overflow,
/// underflow). A residual other than `Exact` makes the result inexact and breaks the tie when
/// `v` itself is representable.
pub fn round<F: Float>(v: Dd, scale: i32, residual: Residual, mode: Rounding) -> (F, Flags) {
    let (a, b) = (float::decode(v.hi), float::decode(v.lo));
    if a.kind != Kind::Finite {
        // Zero, or an infinity/NaN from an overflowing intermediate: the caller's special cases
        // keep those out; a zero approximation is a zero (or a tiny value of known sign).
        let neg = v.hi.is_sign_negative();
        return match residual {
            Residual::Exact => (float::zero(neg), Flags::NONE),
            r => float::round::<F>(Exact { neg: r == Residual::Below, sig: 0, exp: 0, sticky: true }, mode),
        };
    }
    let mut e = crate::arith::add_exact(&a, &b, Rounding::Nearest);
    e.exp += scale;
    if residual != Residual::Exact {
        // Room below the approximation for the residual: at least PREC + 3 bits.
        let len = 128 - e.sig.leading_zeros();
        let shift = (F::PREC + 3).saturating_sub(len).min(127u32.saturating_sub(len));
        e.sig <<= shift;
        e.exp -= i32::try_from(shift).unwrap_or(0);
        let toward_zero = (residual == Residual::Below) != e.neg;
        if toward_zero && !e.sticky {
            e.sig -= 1;
        }
        e.sticky = true;
    }
    float::round::<F>(e, mode)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn error_free_transformations() {
        let a = 1.0 + f64::EPSILON;
        let p = two_prod(a, a);
        assert_eq!((p.hi, p.lo), (1.0 + 2.0 * f64::EPSILON, f64::EPSILON * f64::EPSILON));
        let s = two_sum(1.0, 1e-30);
        assert_eq!((s.hi, s.lo), (1.0, 1e-30));
        // (1/3) * 3 in double-double is 1 within 2^-104.
        let third = Dd::from(1.0).div(Dd::from(3.0));
        let back = third.mul_f64(3.0).sub(Dd::from(1.0));
        assert!(back.hi.abs() < 1e-31, "{back:?}");
        let r = sqrt(Dd::from(2.0));
        let sq = r.sqr().sub(Dd::from(2.0));
        assert!(sq.hi.abs() < 1e-31, "{sq:?}");
    }

    #[test]
    fn rounding_with_a_residual() {
        // 1 exactly, true value just below: down and toward zero give the predecessor.
        let one = Dd::from(1.0);
        assert_eq!(round::<f64>(one, 0, Residual::Below, Rounding::Down).0, 1.0 - f64::EPSILON / 2.0);
        assert_eq!(round::<f64>(one, 0, Residual::Below, Rounding::Nearest).0, 1.0);
        assert_eq!(round::<f64>(one, 0, Residual::Above, Rounding::Up).0, 1.0 + f64::EPSILON);
        assert_eq!(round::<f64>(one, 0, Residual::Exact, Rounding::Up), (1.0, Flags::NONE));
        assert_eq!(round::<f32>(Dd { hi: 1.0, lo: 1e-20 }, 3, Residual::Above, Rounding::Nearest), (8.0, Flags::INEXACT));
        assert_eq!(round::<f32>(Dd::from(1.0), -130, Residual::Above, Rounding::Nearest).1, Flags(0x30));
    }
}
