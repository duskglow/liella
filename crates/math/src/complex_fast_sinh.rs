//! The quick tier of `csinh` and `csin` (spec §10.5; LIBM-CPLX-SINCOS, the structure of D-10.53):
//! `csinh(x + iy) = sinh x·cos y + i·cosh x·sin y`, and `csin(x + iy) = sin x·cosh y + i·cos x·sinh
//! y` is `csinh(y + ix)` with its parts swapped (no sign changes, so every rounding mode carries
//! over). Each part is a double-double with a proved bound, rounded by the `cexp` tier's test
//! ([`super::round`]: `near`, else `settle`). A part that test declines, and every operand outside
//! the ranges below (zeros, infinities, NaNs, the axes, `|y| < 2^-30` or `>= 2^20`, `|x| < 2^-60`
//! or `> 746`), takes the 384-bit route unchanged, so C Annex G's special values stay the route's.
//!
//! **Hyperbolic factors**, `a = |x|`, both in units of `2^(m−1)`:
//! * `e^a = E·2^m` from [`fast64::exp_kernel`] within `Ee` ([`fast64::exp_eps`], below
//!   `2^-69·|E.hi|`), as `cexp`; `2^-60 <= a <= 746`.
//! * `e^-a = Q·2^m`, `Q = R·2^-2m`, `R ≈ 1/E` as a double-double: `r0 = RN(1/E.hi)`, so `E.hi·r0 =
//!   1 + ε` with `|ε| <= 2^-53` on the grid `2^-106`; `1 − p.hi` (Sterbenz) and then `− p.lo` are
//!   exact (`p` = `two_prod(E.hi, r0)`), giving `−ε`; `d = −ε − RN(E.lo·r0)` (`2^-106` each);
//!   `R = Fast2Sum(r0, r0·d)`. The exact `1/E = r0/(1 − d')` differs by `r0·d'² <= 2^-104·r0` and
//!   the roundings: `|R − 1/E| <= 2^-103·R`. For the exact `e = e^a·2^-m`, `|1/E − 1/e| <=
//!   Ee/(E·e)`, so `|Q − e^-a·2^-m| <= (Ee·R.hi + 2^-102)·Q.hi·(1 + 2^-48)` (`eQ`). The scaling is
//!   exact for `a <= 40` (`m <= 58`); above, `e^-2a < 2^-115` and `Q` is dropped, `eQ = 2^-114·E.hi`.
//! * `cosh a = (E + Q)·2^(m−1)`: Fast2Sum of the heads (exact, `E.hi >= Q.hi`), the tails added
//!   (two roundings of terms below `2^-51·E.hi`), Fast2Sum: within `Ee + eQ + 2^-102·E.hi`.
//! * `sinh a = (E − Q)·2^(m−1)` for `a >= 2^-3`, the same steps and bound; relative to the result
//!   that is at most `2^-69·coth(2^-3) = 2^-66` (the cancellation).
//! * `sinh a` for `a < 2^-3` (`m = 0` there; scaled by `2^(1−m)` all the same) by the odd series
//!   `a + a·z/6 + a·z²·q(z)`, `z = a²` exact as `two_prod(a, a)`: `a·z` exact as `two_prod(a, z.hi)`
//!   plus `RN(a·z.lo)`, normalized, then [`Dd::mul`] by `1/6` as a double-double (`2^-102` of a term
//!   below `2^-8.5·a`); `q = 1/5! + zh·(1/7! + … + zh/13!)`, `zh = RN(z)`, and `a·RN(zh²)·q <=
//!   2^-18.9·a` within `7.1u` of its value (`zh²` `3u`, two products, `q` `2.1u`): `2^-69.1·a`. The
//!   omitted `a·z^7/15!·1.01 < 2^-82·a`; the low sum's two roundings `< 2^-71.7·a`. So the series is
//!   within `SERIES = 2^-68` of `sinh a`, relative to its head `>= a`.
//!
//! **Parts.** `P = H ⊗ T` as `cexp`'s parts, `T` the trig kernels' `cos y` or `sin y` (`2^-30 <= |y|
//! < 2^20`, bound [`trig::eps`]), `eps = (eH·B + A·eT + 2^-99·A·B)·(1 + 2^-48)` with `A = |H.hi|`,
//! `B = |T.hi|`; `cexp`'s second-order argument needs `eH <= 2^-60·A`, met above. Rounded and
//! scaled by `2^(m−1)` with [`super::round`]: only normal results of biased exponent `2..=2046`,
//! so nothing overflows or underflows in the tier (a product that stays finite while `cosh` or
//! `sinh` alone would overflow, `a` up to 746, is answered here). Every intermediate is normal
//! (`H >= 2^-59`, `|T| >= 2^-63`): only inexact can arise. Neither part is a `double` or a
//! midpoint: for nonzero algebraic `x`, `y`, `sinh x·cos y` and `cosh x·sin y` are rational
//! combinations of `e^(±x±iy)`, distinct algebraic exponents, so Lindemann–Weierstrass excludes a
//! rational value (as for `cexp`).

use super::{fast32, fast64, kernel, round, t, tier_any, trig, Dd, PROD, WIDEN, Y_MAX, Y_MIN};
use crate::dd;
use crate::float::Float;
use crate::status::{Flags, Rounding, Status};

/// `2^-60` and `746`: the range of `a` ([`fast64::reduce_exp`]).
const A_MIN: f64 = f64::from_bits(0x3c30_0000_0000_0000);
const A_MAX: f64 = 746.0;
/// `2^-3`: below, `sinh a` by the series.
const SERIES_MAX: f64 = 0.125;
/// Above `40`, `e^-a` is dropped (`e^-2a < 2^-115`).
const Q_MAX: f64 = 40.0;
/// `2^-68`: the series' relative bound; `2^-102`: the reciprocal's and the sums' roundings;
/// `2^-114`: the dropped `e^-2a`.
const SERIES: f64 = f64::from_bits(0x3bb0_0000_0000_0000);
const ROUNDS: f64 = f64::from_bits(0x3990_0000_0000_0000);
const E114: f64 = f64::from_bits(0x38d0_0000_0000_0000);
/// `1/6` as a double-double.
const SIXTH: Dd = Dd { hi: f64::from_bits(0x3fc5_5555_5555_5555), lo: f64::from_bits(0x3c65_5555_5555_5555) };
/// `1/(2k+1)!`, `k = 2..=6`.
const Q5: [f64; 5] = [1.0 / 120.0, 1.0 / 5040.0, 1.0 / 362_880.0, 1.0 / 39_916_800.0, 1.0 / 6_227_020_800.0];

/// `sinh a` by the series for `2^-60 <= a < 2^-3`, within `SERIES·|hi|` (module comment).
#[inline(always)]
fn series(a: f64) -> Dd {
    let z = dd::two_prod(a, a);
    let p = dd::two_prod(a, z.hi);
    let t1 = dd::fast_two_sum(p.hi, p.lo + a * z.lo).mul(SIXTH);
    let zh = z.hi;
    let q = Q5[0] + zh * (Q5[1] + zh * (Q5[2] + zh * (Q5[3] + zh * Q5[4])));
    let t2 = (a * (zh * zh)) * q;
    let u = dd::fast_two_sum(a, t1.hi);
    dd::fast_two_sum(u.hi, u.lo + (t1.lo + t2))
}

/// `sinh a` and `cosh a` (`a = |x|`) as double-doubles in units of `2^(m−1)` with their absolute
/// bounds in the same units, `(S, eS, C, eC, m)`, or `None` outside `2^-60 <= a <= 746`.
#[inline(always)]
pub(crate) fn hyp(a: f64) -> Option<(Dd, f64, Dd, f64, i32)> {
    if !(A_MIN..=A_MAX).contains(&a) {
        return None;
    }
    let kf = dd::nearest_int(a * f64::from_bits(t::SCALES[0]));
    let (e, m, tail) = fast64::exp_kernel(fast64::reduce_exp(a, kf), dd::to_i32(kf))?;
    let ee = fast64::exp_eps(e, tail);
    let (q, eq) = if a <= Q_MAX {
        let r0 = 1.0 / e.hi;
        let p = dd::two_prod(e.hi, r0);
        let d = ((1.0 - p.hi) - p.lo) - e.lo * r0;
        let r = dd::fast_two_sum(r0, r0 * d);
        let q = r.scale(dd::pow2(-2 * m));
        (q, (ee * r.hi + ROUNDS) * q.hi * WIDEN)
    } else {
        (Dd { hi: 0.0, lo: 0.0 }, E114 * e.hi)
    };
    let err = ee + eq + ROUNDS * e.hi;
    let c1 = dd::fast_two_sum(e.hi, q.hi);
    let c = dd::fast_two_sum(c1.hi, c1.lo + (e.lo + q.lo));
    if a < SERIES_MAX {
        let s = series(a).scale(dd::pow2(1 - m));
        return Some((s, SERIES * s.hi, c, err, m));
    }
    let s1 = dd::fast_two_sum(e.hi, -q.hi);
    Some((dd::fast_two_sum(s1.hi, s1.lo + (e.lo - q.lo)), err, c, err, m))
}

/// `[(sinh x·cos y, eps), (cosh x·sin y, eps)]` unscaled and the scale `m − 1`, or `None`.
#[inline(always)]
fn parts(x: f64, y: f64) -> Option<([(Dd, f64); 2], i32)> {
    if !(Y_MIN..Y_MAX).contains(&dd::abs(y)) {
        return None;
    }
    let (sh, es, ch, ec, m) = hyp(dd::abs(x))?;
    let sh = if x < 0.0 { sh.neg() } else { sh };
    let (n, r) = kernel::reduce(y);
    let p = kernel::poly(r);
    let (s, ss) = kernel::assemble(n, p);
    let (c, cs) = kernel::assemble(n + 32, p);
    let part = |h: Dd, eh: f64, v: Dd, et: f64| {
        let (a, b) = (dd::abs(h.hi), dd::abs(v.hi));
        let q = dd::two_prod(h.hi, v.hi);
        (dd::fast_two_sum(q.hi, q.lo + (h.hi * v.lo + h.lo * v.hi)), (eh * b + a * et + PROD * a * b) * WIDEN)
    };
    Some(([part(sh, es, c, trig::eps(cs, c)), part(ch, ec, s, trig::eps(ss, s))], m - 1))
}

/// Both parts in `mode`, those of `parts` swapped for csin (`swap`).
#[inline(always)]
fn both(x: f64, y: f64, swap: bool, mode: Rounding) -> Option<(f64, f64)> {
    let ([p0, p1], m) = parts(x, y)?;
    let ((re, er), (im, ei)) = if swap { (p1, p0) } else { (p0, p1) };
    Some((round(re, er, m, mode)?, round(im, ei, m, mode)?))
}

/// `csinh(x + iy)` to nearest for the boundary's in-place entry (as `cexp_nearest`: each part's
/// test raises inexact itself, nothing else can arise; a decline goes to the cold fallback).
#[inline(always)]
pub fn csinh_nearest(x: f64, y: f64) -> Option<(f64, f64)> {
    both(x, y, false,Rounding::Nearest)
}

/// `csin(x + iy)` to nearest for the boundary's in-place entry: `csinh(y + ix)` swapped.
#[inline(always)]
pub fn csin_nearest(x: f64, y: f64) -> Option<(f64, f64)> {
    both(y, x, true,Rounding::Nearest)
}

/// `csinh(x + iy)` in `mode` by the quick tier, or `None`.
#[inline(always)]
pub fn csinh(x: f64, y: f64, mode: Rounding) -> Option<((f64, f64), Status)> {
    Some((both(x, y, false,mode)?, Status::flags(Flags::INEXACT)))
}

/// `csin(x + iy)` in `mode` by the quick tier, or `None`.
#[inline(always)]
pub fn csin(x: f64, y: f64, mode: Rounding) -> Option<((f64, f64), Status)> {
    Some((both(y, x, true,mode)?, Status::flags(Flags::INEXACT)))
}

/// The `csinh` quick tier for any format.
#[inline(always)]
pub(crate) fn csinh_any<F: Float>(x: F, y: F, mode: Rounding) -> Option<((F, F), Status)> {
    tier_any(x, y, |a, b| csinh(a, b, mode), |a, b| fast32::csinhf(a, b, mode))
}

/// The `csin` quick tier for any format.
#[inline(always)]
pub(crate) fn csin_any<F: Float>(x: F, y: F, mode: Rounding) -> Option<((F, F), Status)> {
    tier_any(x, y, |a, b| csin(a, b, mode), |a, b| fast32::csinf(a, b, mode))
}

#[cfg(test)]
#[path = "complex_fast_sinh_tests.rs"]
mod tests;
