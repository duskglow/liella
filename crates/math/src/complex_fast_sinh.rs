//! The quick tier of `csinh` and `csin` (spec §10.5; LIBM-CPLX-SINCOS, the structure of D-10.53):
//! `csinh(x + iy) = sinh x·cos y + i·cosh x·sin y`, and `csin(x + iy) = sin x·cosh y + i·cos x·sinh
//! y` is `csinh(y + ix)` with its parts swapped (no sign changes, so every rounding mode carries
//! over). Each part is a double-double with a proved bound, rounded by the `cexp` tier's test
//! ([`super::round`]: `near`, else `settle`). A part that test declines is answered as an overflow
//! or by the middle tier when it can be (below the quick tier's code); what remains, and every
//! operand outside the ranges below (zeros, infinities, NaNs, the axes, `|y| < 2^-30` or `>= 2^20`, `|x| < 2^-60`
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
//!   that is at most `2^-69·coth(2^-3) < 2^-65.9` (the cancellation; [`fast64::exp_eps`] stays
//!   below `2^-69.9·E` for the reduced range, so the computed bound is below `2^-66` of `S`).
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

use super::{fast32, fast64, kernel, middle_round, round, sin_or_cos, t, tier_any, trig, Dd, Part, PROD, WIDEN, Y_MAX, Y_MIN};
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

// ---------------------------------------------------------------------------------------------
// After a quick decline (LIBM-CPLX-SINCOS-4). Only the envelope's tier ([`csinh`], [`csin`]) runs
// this cold code; the in-place `*_nearest` entries still decline such a part (they raise inexact
// only), and the boundary's cold fallback then runs the envelope's tier, as for `cexp` (D-10.99).
//
// **Overflow.** A part whose every value within `eps` of `P·2^m` has magnitude `>= 2^1024`
// rounds, in every mode, to `±∞` or `±DBL_MAX` by its sign and the mode (C Annex F; the result
// overflows, so `FE_OVERFLOW` and `FE_INEXACT`, and `ERANGE` with an infinite part, as the route). The lower bound `(|P.hi| − |P.lo| −
// eps)·(1 − 2^-50)` absorbs the two roundings of its own evaluation. A part between `DBL_MAX` and
// `2^1024` still declines.
//
// **The middle tier** (the method of `cexp`'s, D-10.99): `P = H ⊗ T` near `2^-96`.
// * `T` = [`sin_or_cos`]. Its documented bound is `2^-100` absolute; next to a multiple of `π/2`
//   `|T|` can be `2^-62` (Muller 2016 §11.3), so the bound is used relative: the three
//   double-double additions of the reduction err by at most `2^-104.4` of their partial sums
//   (Joldes, Muller, Popescu, ACM TOMS 44(2), 2017), the first of which exceeds `|r|` by at most
//   `|n·P2| < 2^-34.5`; with `RN(n·P3)` (`2^-142`) and `n·δ` (`2^-143`): `|r̃ − r| <= 2^-102.8·|r|
//   + 2^-138.5`. The sine branch `r̃·S(z)` is within `2^-101` of `sin r̃` relative (`S >= 0.9`),
//   and `|sin r| >= 0.9·|r|` for `|r| <= π/4 + 2^-31`; the cosine branch, `>= 0.70`, is within
//   `2^-101.4` absolute. So `|T − t| <= 2^-99·|T.hi| + 2^-137` (`T_REL`, `T_ABS`; the tests check
//   it against the 384-bit arithmetic next to `kπ/2`).
// * `H` for `a >= 2^-3`: `e^a = V·2^s` and `e^-a = W·2^s'` from [`crate::wide_dd::exp_dd`], each
//   within `2^-100` relative (`|x| < 2^14`), `Q = W·2^(s'−s)` (exact; dropped above `40`, `e^-2a <
//   2^-115`); `cosh a = (V + Q)·2^(s−1)`, `sinh a = (V − Q)·2^(s−1)` by [`Dd::add`] (`2^-104.4` of
//   the result): relative to `sinh a` the inputs' errors grow by `coth a <= coth 2^-3 < 8.05`, so
//   `|H − h| <= 2^-96.8·|H|` for both.
// * `H` for `a < 2^-3` (units `2^0`): `sinh a = a·Σ z^k/(2k+1)!`, `cosh a = Σ z^k/(2k)!`, `z =
//   two_prod(a, a)` exact, by [`dd::horner`] with `cexp`'s coefficient tables, the signs of odd `k`
//   flipped (exact): every term positive, `z < 2^-6`, so the sums are within `2^-102` relative and
//   the product with `a` within `2^-101.5`.
// * `|H·T − h·t| <= eH·|t| + |H|·eT + 2^-102.6·A·B` (`A = |H.hi|`, `B = |T.hi|`) `<= 2^-96.3·A·B
//   + 2^-136.9·A`, so `eps = A·(B + 2^-40)·2^-96` ([`MID_EPS`], [`T_ABS`]) with room for its own
//   evaluation; [`super::middle_round`] decides or declines (also any overflow or underflow of the
//   scaled result). Every intermediate stays normal (`a >= 2^-60`, `|T| >= 2^-63`).

/// `2^-96`: the middle part's bound per unit of `A·(B + 2^-40)`.
const MID_EPS: f64 = f64::from_bits(0x39f0_0000_0000_0000);
/// `2^-40`: the trig factor's absolute term `2^-136`, as a share of `2^-96`.
const T_ABS: f64 = f64::from_bits(0x3d70_0000_0000_0000);
/// `1 − 2^-50`: the overflow test's lower bound against its own roundings.
const LOWER: f64 = f64::from_bits(0x3fef_ffff_ffff_fffc);
/// Below this scale no part can reach `2^1024` (`|P| < 2^3`).
const OVF_M: i32 = 64;

/// A double-double negated (both words; a zero word stays `+0`).
const fn flip([hi, lo]: [u64; 2]) -> [u64; 2] {
    [hi ^ (1 << 63), if lo == 0 { 0 } else { lo ^ (1 << 63) }]
}

/// `c` with its odd-`k` entries negated.
const fn unsigned([c0, c1, c2, c3, c4, c5, c6, c7, c8, c9, c10, c11, c12, c13]: [[u64; 2]; 14]) -> [[u64; 2]; 14] {
    [c0, flip(c1), c2, flip(c3), c4, flip(c5), c6, flip(c7), c8, flip(c9), c10, flip(c11), c12, flip(c13)]
}

/// `1/(2k+1)!` and `1/(2k)!`, `k = 0..=13`, as double-doubles.
const SINH_DD: [[u64; 2]; 14] = unsigned(super::SIN_DD);
const COSH_DD: [[u64; 2]; 14] = unsigned(super::COS_DD);

/// `±∞` or `±DBL_MAX` by the sign and `mode` when every value within `eps` of `P·2^m` has
/// magnitude `>= 2^1024`, else `None`.
fn overflow(p: Dd, eps: f64, m: i32, mode: Rounding) -> Option<f64> {
    if m < OVF_M {
        return None;
    }
    let low = (dd::abs(p.hi) - dd::abs(p.lo) - eps) * LOWER;
    if low < dd::pow2(1024 - m) {
        return None;
    }
    let neg = p.hi < 0.0;
    let inf = matches!((mode, neg), (Rounding::Nearest, _) | (Rounding::Up, false) | (Rounding::Down, true));
    let v = if inf { f64::INFINITY } else { f64::MAX };
    Some(if neg { -v } else { v })
}

/// `sinh a` and `cosh a` for the middle tier in units of `2^s`, `(S, C, s)`, within `2^-96.8` of
/// each (see above), or `None` outside `2^-60 <= a <= 746`.
fn mid_hyp(a: f64) -> Option<(Dd, Dd, i32)> {
    if !(A_MIN..=A_MAX).contains(&a) {
        return None;
    }
    if a < SERIES_MAX {
        let z = dd::two_prod(a, a);
        return Some((Dd::from(a).mul(dd::horner(z, &SINH_DD, 8)), dd::horner(z, &COSH_DD, 9), 0));
    }
    let (v, s) = crate::wide_dd::exp_dd(&crate::float::decode(a))?;
    let q = if a <= Q_MAX {
        let (w, s2) = crate::wide_dd::exp_dd(&crate::float::decode(-a))?;
        w.scale(dd::pow2(s2 - s))
    } else {
        Dd { hi: 0.0, lo: 0.0 }
    };
    Some((v.add(q.neg()), v.add(q), s - 1))
}

/// The middle tier's part: `sinh h·cos t` (`Part::Re`) or `cosh h·sin t` (`Part::Im`) `≈ P·2^s`
/// within `eps`, `(P, eps, s)`, or `None`.
fn mid_part(h: f64, t: f64, part: Part) -> Option<(Dd, f64, i32)> {
    let (sh, ch, s) = mid_hyp(dd::abs(h))?;
    let f = match part {
        Part::Re if h < 0.0 => sh.neg(),
        Part::Re => sh,
        Part::Im => ch,
    };
    let v = sin_or_cos(t, part);
    let a = dd::abs(f.hi);
    Some((f.mul(v), a * (dd::abs(v.hi) + T_ABS) * MID_EPS, s))
}

/// A part the quick test declined (`(P, eps)` at scale `m`): the overflow value, else the middle
/// tier; `(value, overflowed)`, or `None` for the 384-bit route.
#[cold]
#[inline(never)]
fn rescue(p: (Dd, f64), m: i32, h: f64, t: f64, part: Part, mode: Rounding) -> Option<(f64, bool)> {
    if let Some(v) = overflow(p.0, p.1, m, mode) {
        return Some((v, true));
    }
    let (q, eps, s) = mid_part(h, t, part)?;
    Some((middle_round(q, eps, s, mode)?, false))
}

/// Both parts in `mode` for the envelope's tier: the quick test, then [`rescue`]; swapped for
/// csin (`swap`), with the status (overflow when a part overflowed).
#[inline(always)]
fn full(h: f64, t: f64, swap: bool, mode: Rounding) -> Option<((f64, f64), Status)> {
    let ([p0, p1], m) = parts(h, t)?;
    let (re, o0) = match round(p0.0, p0.1, m, mode) {
        Some(v) => (v, false),
        None => rescue(p0, m, h, t, Part::Re, mode)?,
    };
    let (im, o1) = match round(p1.0, p1.1, m, mode) {
        Some(v) => (v, false),
        None => rescue(p1, m, h, t, Part::Im, mode)?,
    };
    // An overflow raises its flags; `ERANGE` goes with an infinite part (a directed mode's
    // `±DBL_MAX` keeps `errno`), as the route reports.
    let st = match (o0 || o1, re.is_infinite() || im.is_infinite()) {
        (false, _) => Status::flags(Flags::INEXACT),
        (true, false) => Status::flags(Flags::OVERFLOW.union(Flags::INEXACT)),
        (true, true) => Status::flags(Flags::OVERFLOW.union(Flags::INEXACT)).overflow_errno(),
    };
    Some((if swap { (im, re) } else { (re, im) }, st))
}

/// `csinh(x + iy)` to nearest for the boundary's in-place entry (as `cexp_nearest`: each part's
/// test raises inexact itself, nothing else can arise; a decline goes to the cold fallback).
#[inline(always)]
pub fn csinh_nearest(x: f64, y: f64) -> Option<(f64, f64)> {
    both(x, y, false, Rounding::Nearest)
}

/// `csin(x + iy)` to nearest for the boundary's in-place entry: `csinh(y + ix)` swapped.
#[inline(always)]
pub fn csin_nearest(x: f64, y: f64) -> Option<(f64, f64)> {
    both(y, x, true, Rounding::Nearest)
}

/// `csinh(x + iy)` in `mode` by the quick tier and its middle tier, or `None`.
#[inline(always)]
pub fn csinh(x: f64, y: f64, mode: Rounding) -> Option<((f64, f64), Status)> {
    full(x, y, false, mode)
}

/// `csin(x + iy)` in `mode` by the quick tier and its middle tier, or `None`.
#[inline(always)]
pub fn csin(x: f64, y: f64, mode: Rounding) -> Option<((f64, f64), Status)> {
    full(y, x, true, mode)
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
