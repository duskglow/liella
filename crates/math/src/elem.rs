//! What the elementary functions of `float` and `double` share (spec §10.3, D-10.13): the formats
//! they are implemented for, special-value handling, and the final rounding.
//!
//! Each function computes `f(x)` in double-double arithmetic ([`crate::dd`]) to a relative error
//! of about 2^-100 and rounds that approximation once into the result format in the caller's
//! rounding direction. For `float` this is correctly rounded (the approximation is far closer
//! than any binary32 rounding boundary needs; the exhaustive sweeps check it); for `double` it is
//! correctly rounded except where the exact value lies within about 2^-100 of a rounding
//! boundary, which random inputs essentially never meet.

use crate::dd::{self, Dd, Residual};
use crate::float::{self, Exact, Float, Kind, Num};
use crate::status::{Flags, Rounding, Status};

/// The formats computed in double precision: `float` and `double` (every value is an `f64`).
pub trait Small: Float {
    /// The value as an `f64` (exact).
    fn widen(self) -> f64;
    /// An `f64` result in this format; used only where the value is already representable
    /// (the `double` fast paths call it only for `double`).
    fn narrow(v: f64) -> Self;
    /// `v` rounded to nearest (ties to even) into this format, for a `v` in its normal range.
    fn narrow_nearest(v: f64) -> Self;
}

impl Small for f32 {
    fn widen(self) -> f64 {
        f64::from(self)
    }
    fn narrow(v: f64) -> f32 {
        #[allow(clippy::as_conversions, clippy::cast_possible_truncation)] // callers pass representable values only
        let r = v as f32;
        r
    }
    fn narrow_nearest(v: f64) -> f32 {
        #[allow(clippy::as_conversions, clippy::cast_possible_truncation)] // rounds to nearest, ties to even
        let r = v as f32;
        r
    }
}

impl Small for f64 {
    fn widen(self) -> f64 {
        self
    }
    fn narrow(v: f64) -> f64 {
        v
    }
    fn narrow_nearest(v: f64) -> f64 {
        v
    }
}

/// Round an approximation `v · 2^scale`; `ERANGE` for an overflow to ∞ or an underflow to 0.
pub fn finish<F: Small>(v: Dd, scale: i32, res: Residual, mode: Rounding) -> (F, Status) {
    if let Some(r) = direct::<F>(v, scale, res, mode) {
        return r;
    }
    let (r, flags) = dd::round::<F>(v, scale, res, mode);
    (r, float::range_errno(r, Status::flags(flags)))
}

/// [`finish`] without the 256-bit route, for a result well inside `F`'s normal range; `None`
/// (the caller takes [`dd::round`]) for a zero, non-finite or near-overflow/underflow `v`, and
/// for an exact tie between two `F` values, which the residual alone decides.
///
/// Proof. Let `V = (hi + lo)·2^scale + ε`, `ε` the residual (an infinitesimal of its sign, or
/// 0). `two_sum` gives `s = RN(hi + lo)` and `t` with `hi + lo = s + t` exactly and `|t|` at most
/// half the spacing of `f64`s at `s`. For `float`, `r = RN₂₄(s)` (`s as f32`, exact in `f64`) and
/// `d = s − r` (exact: both lie on `s`'s `f64` grid and within one `float` spacing); for `double`
/// `r = s`, `d = 0`. Then `V/2^scale = r + d + t + ε`. `d` is 0 or a multiple of `s`'s `f64`
/// spacing `u` and `|t| <= u/2`, so `ρ = d + t + ε` has the sign of the first nonzero of `d`, `t`,
/// `ε`, and `|ρ|` is at most half the `F` spacing on `ρ`'s side of `r` (`h`, halved below a power
/// of two when `ρ` points toward zero). When the leading term's magnitude is exactly `h`,
/// `V` may be a tie: refused. Otherwise `|ρ| < h` and `r` is the nearest `F` value; `V` lies
/// strictly between `r` and its neighbour on `ρ`'s side, so the directed roundings give `r` or
/// that neighbour by the sign of `ρ` alone. The range check keeps `r`, its neighbours and their
/// scaled values normal and finite in `F`: scaling is exact and nothing underflows or overflows,
/// and the flags are inexact exactly when `ρ ≠ 0`, as `dd::round` gives.
#[allow(clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_sign_loss, clippy::arithmetic_side_effects, clippy::float_cmp)] // exponent fields of in-range values
fn direct<F: Small>(v: Dd, scale: i32, res: Residual, mode: Rounding) -> Option<(F, Status)> {
    let s = v.hi + v.lo;
    // Unbiased exponent of s (zero, subnormal, ∞ and NaN fall outside the window below).
    let e = ((s.to_bits() >> 52) & 0x7ff) as i32 - 1023;
    // Scaled: r, its neighbours and their images normal and finite in F. Unscaled also: half
    // an F spacing at s (2^(e − PREC), below for a power of two) a normal f64.
    let (lo_e, hi_e) = (2 - F::EMAX, F::EMAX - 1);
    let lo_u = lo_e.max(-1020 + F::PREC as i32);
    if e < lo_u || e > hi_e || e + scale < lo_e || e + scale > hi_e {
        return None;
    }
    let bb = s - v.hi;
    let t = (v.hi - (s - bb)) + (v.lo - bb);
    let r = F::narrow_nearest(s);
    let rw = r.widen();
    let d = s - rw;
    // Exponent of r (r may have rounded up to the next binade) and half its F spacing.
    let rb = rw.to_bits();
    let er = ((rb >> 52) & 0x7ff) as i32 - 1023;
    let pow2 = rb & ((1u64 << 52) - 1) == 0;
    let half = f64::from_bits(((er - F::PREC as i32 + 1023) as u64) << 52);
    // ρ's sign: the first nonzero of d, t and the residual; `lead` its leading term.
    let (sign, lead) = if d != 0.0 {
        (if d > 0.0 { 1 } else { -1 }, d)
    } else if t != 0.0 {
        (if t > 0.0 { 1 } else { -1 }, t)
    } else {
        (
            match res {
                Residual::Exact => 0,
                Residual::Above => 1,
                Residual::Below => -1,
            },
            0.0,
        )
    };
    let toward_zero = (sign < 0) != (rw < 0.0);
    let h = if pow2 && toward_zero { half * 0.5 } else { half };
    if lead == h || lead == -h {
        return None;
    }
    // Whether the result is r's neighbour on ρ's side rather than r.
    let step = match mode {
        Rounding::Nearest => false,
        Rounding::Up => sign > 0,
        Rounding::Down => sign < 0,
        Rounding::Zero => sign != 0 && toward_zero,
    };
    // That neighbour: one unit of the F encoding (sign and magnitude) toward or away from 0.
    let bits = r.to_bits();
    let mut out = if step { F::from_bits(if toward_zero { bits - 1 } else { bits + 1 }) } else { r };
    if scale != 0 {
        let k = u128::from(scale.unsigned_abs()) << (F::PREC - 1);
        let b = out.to_bits();
        out = F::from_bits(if scale > 0 { b + k } else { b - k });
    }
    let flags = if sign == 0 { Flags::NONE } else { Flags::INEXACT };
    Some((out, float::range_errno(out, Status::flags(flags))))
}

/// An exactly representable result (an `f64` that fits `F`).
pub fn exact<F: Small>(v: f64) -> (F, Status) {
    if v.is_infinite() {
        return (float::inf(v < 0.0), Status::OK);
    }
    finish(Dd::from(v), 0, Residual::Exact, Rounding::Nearest)
}

/// A NaN operand's result: the quieted NaN, invalid for a signalling one.
pub fn nan_in<F: Float>(n: &Num) -> Option<(F, Status)> {
    match n.kind {
        Kind::QNan => Some((float::quiet(n), Status::OK)),
        Kind::SNan => Some((float::quiet(n), Status::INVALID)),
        Kind::Invalid => Some((float::default_nan(), Status::INVALID)),
        _ => None,
    }
}

/// A domain error: the default NaN, invalid and `EDOM`.
pub fn domain<F: Float>() -> (F, Status) {
    (float::default_nan(), Status::DOMAIN)
}

/// A domain error with a positive quiet NaN (glibc's `asin`, `acos` and `log10`; the others give
/// the default NaN, which has the sign bit set).
pub fn domain_pos<F: Float>() -> (F, Status) {
    (float::nan(false, true, 0), Status::DOMAIN)
}

/// A pole: ±∞, division by zero and `ERANGE`.
pub fn pole<F: Float>(neg: bool) -> (F, Status) {
    (float::inf(neg), Status::POLE)
}

/// A result far beyond the format's range, as rounding it gives: ∞ or the largest finite
/// value, overflow and inexact, `ERANGE` for ∞.
pub fn overflow<F: Float>(neg: bool, mode: Rounding) -> (F, Status) {
    let (r, flags) = float::round::<F>(Exact { neg, sig: 1, exp: 1 << 20, sticky: false }, mode);
    (r, float::range_errno(r, Status::flags(flags)))
}

/// A nonzero result far below the format's smallest subnormal: ±0 or the smallest subnormal,
/// underflow and inexact, `ERANGE` for 0.
pub fn underflow<F: Float>(neg: bool, mode: Rounding) -> (F, Status) {
    let (r, flags) = float::round::<F>(Exact { neg, sig: 0, exp: 0, sticky: true }, mode);
    (r, float::range_errno(r, Status::flags(flags)))
}

/// `x + δ` for a `δ` far below `x`'s last place, of the sign `above` gives: the result of
/// `f(x) = x(1 + O(x²))` for tiny `x` (`sin`, `tan`, `atan`, `expm1`, ...).
pub fn near<F: Small>(x: f64, above: bool, mode: Rounding) -> (F, Status) {
    finish(Dd::from(x), 0, if above { Residual::Above } else { Residual::Below }, mode)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp, clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_sign_loss, clippy::arithmetic_side_effects, clippy::cast_precision_loss)]
mod tests {
    use super::*;

    /// The 256-bit route alone (what `finish` did before the direct rounder).
    fn wide<F: Small>(v: Dd, scale: i32, res: Residual, mode: Rounding) -> (F, Status) {
        let (r, flags) = dd::round::<F>(v, scale, res, mode);
        (r, float::range_errno(r, Status::flags(flags)))
    }

    fn check<F: Small>(v: Dd, scale: i32, hits: &mut u32) {
        for res in [Residual::Exact, Residual::Above, Residual::Below] {
            for mode in [Rounding::Nearest, Rounding::Up, Rounding::Down, Rounding::Zero] {
                let (a, sa) = finish::<F>(v, scale, res, mode);
                let (b, sb) = wide::<F>(v, scale, res, mode);
                assert!(a.to_bits() == b.to_bits() && format!("{sa:?}") == format!("{sb:?}"), "{v:?} scale {scale} {res:?} {mode:?}: direct {:#x} {sa:?}, wide {:#x} {sb:?}", a.to_bits(), b.to_bits());
                if direct::<F>(v, scale, res, mode).is_some() {
                    *hits += 1;
                }
            }
        }
    }

    #[test]
    fn direct_rounding_matches_the_wide_route() {
        // no spec clause: the direct double-double rounder gives bit-for-bit the result and the
        // flags of the 256-bit route in every mode and residual, including ties, powers of two,
        // binade edges and scaled results near the format's limits.
        let mut s = 0x2545_f491_4f6c_dd1du64;
        let mut next = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            s
        };
        let mut hits = [0u32; 2];
        for i in 0..20000u32 {
            let r = next();
            let e = (r % 120) as i32 - 60;
            let hi = f64::from_bits((r >> 12) | (((1023 + e) as u64) << 52)) * if r & 1 == 0 { 1.0 } else { -1.0 };
            let ulp = f64::from_bits(((1023 + e - 52) as u64) << 52);
            let ulp32 = f64::from_bits(((1023 + e - 23) as u64) << 52);
            let hi32 = f64::from(hi as f32);
            let lo_r = next();
            let lo = match i % 8 {
                0 => 0.0,
                1 => ulp * 0.5,
                2 => -ulp * 0.5,
                3 => ulp * ((lo_r >> 11) as f64 / (1u64 << 53) as f64 - 0.5),
                4 => ulp * 2f64.powi(-((lo_r % 70) as i32)) * if lo_r & 2 == 0 { 1.0 } else { -1.0 },
                5 => ulp * 0.25,
                _ => -ulp * 0.25,
            };
            let sc = [0, 0, 0, 1, -1, 37, -200, 900, -900, 1000 - e, -1018 - e, 125 - e, -124 - e, 126 - e, -125 - e][(next() % 15) as usize];
            let p2 = f64::from_bits(((1023 + e) as u64) << 52) * hi.signum();
            for v in [Dd { hi, lo }, Dd { hi: p2, lo }, Dd { hi: p2, lo: lo * 0.5 }] {
                check::<f64>(v, sc, &mut hits[0]);
            }
            // float: ties and near-ties of the 24-bit grid, and powers of two.
            let t32 = match i % 4 {
                0 => hi32 + ulp32 * 0.5,
                1 => hi32 - ulp32 * 0.5,
                2 => hi32 + ulp32 * 0.25,
                _ => hi,
            };
            for v in [Dd { hi: t32, lo: 0.0 }, Dd { hi: t32, lo: ulp * 1e-3 }, Dd { hi: t32, lo: -ulp * 1e-3 }, Dd { hi: p2, lo }, Dd { hi: hi32, lo }] {
                check::<f32>(v, sc.clamp(-100, 100), &mut hits[1]);
            }
        }
        // The direct rounder must decide most of these (it refuses only ties and range edges).
        assert!(hits[0] > 200_000 && hits[1] > 200_000, "direct rounder used {hits:?} times");
    }
}
