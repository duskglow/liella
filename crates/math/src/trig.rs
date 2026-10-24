//! Trigonometric functions with exact argument reduction (spec §10.3) for `float` and `double`:
//! `sin`, `cos`, `tan`, `sincos`, `asin`, `acos`, `atan`, `atan2`. Computed in double-double
//! ([`crate::elem`]).
//!
//! * Reduction: `x = n·π/64 + r` with `|r| <= π/128`. Below 1500 by Cody–Waite with a four-part
//!   `π/64`; above by Payne–Hanek (multiply the significand by a 256-bit window of the bits of
//!   `2/π`, keep the integer part modulo 128 and 190 bits of fraction), so every finite argument
//!   is reduced exactly (the closest a `double` comes to a multiple of `π/2` costs about 61 of
//!   those bits). Then `sin x = sin(nπ/64)·cos r + cos(nπ/64)·sin r` with the table of
//!   `sin(kπ/64)` and the Taylor series of `sin r`, `cos r`.
//! * `atan t` for `0 <= t <= 1`: `atan(j/64) + atan((t − j/64)/(1 + t·j/64))` with the table of
//!   `atan(j/64)` and the odd series; `t > 1` through `π/2 − atan(1/t)`. `asin`, `acos` and
//!   `atan2` reduce to it without cancellation (`1 − x²` as the exact `(1 − x)(1 + x)`).
//!
//! The tables are `coeffs/tables.rs` (`cargo xtask tables`).

#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)] // table indices reduced to the tables' ranges; exponents and word positions of bounded range

use crate::arith;
use crate::coeffs::tables as t;
use crate::dd::{self, horner, two_sum, Dd, Residual};
use crate::elem::{self, Small};
use crate::exp_log::reduce;
use crate::float::{self, Float, Kind};
use crate::fused;
use crate::status::{Rounding, Status};

const SCALE_PI: f64 = f64::from_bits(t::SCALES[2]); // 64/π
/// Below this magnitude `f(x) = x(1 + c·x²)` (or `1 + c·x²`) is within a quarter of the last
/// place of `x` (of 1), so it rounds as `x` plus a tiny amount of the sign of `c·x³`; above it the
/// correction is far beyond the double-double error. 2^-30.
const TINY: f64 = 9.313_225_746_154_785e-10;

/// `w` bits `[from, from + 64)` of the 2/π expansion (bit 0 is the first after the point).
fn two_over_pi_word(from: usize) -> u64 {
    let (w, o) = (from / 64, from % 64);
    let a = t::TWO_OVER_PI.get(w).copied().unwrap_or(0);
    let b = t::TWO_OVER_PI.get(w + 1).copied().unwrap_or(0);
    if o == 0 { a } else { (a << o) | (b >> (64 - o)) }
}

/// Bits `[lo, lo + 64)` of a big-endian multi-word integer (`p[0]` most significant).
fn bits(p: &[u64], lo: usize) -> u64 {
    let n = p.len() * 64;
    let word = |bit: usize| -> u64 {
        // The 64-bit word whose least significant bit is `bit` (zeros beyond the ends).
        let (w, o) = (bit / 64, bit % 64);
        let at = |i: usize| if i < p.len() { p[p.len() - 1 - i] } else { 0 };
        if o == 0 { at(w) } else { (at(w) >> o) | (at(w + 1) << (64 - o)) }
    };
    if lo >= n { 0 } else { word(lo) }
}

/// Payne–Hanek: `x·64/π` for a positive finite `x >= 1500`, as `(n mod 128, f)` with the
/// fraction `f` in [−1/2, 1/2] as a double-double.
fn payne_hanek(x: f64) -> (i32, Dd) {
    let n = float::decode(x);
    let m = u64::try_from(n.sig).unwrap_or(0);
    let e = n.exp; // x = m·2^e
    // Bits of 2/π before bit i0 (1-based) add only multiples of 128 to x·64/π.
    let i0 = (e - 1).max(1);
    let start = usize::try_from(i0 - 1).unwrap_or(0);
    let b: [u64; 4] = core::array::from_fn(|i| two_over_pi_word(start + 64 * i));
    // P = m × B (big-endian, 5 words).
    let mut p = [0u64; 5];
    let mut carry: u128 = 0;
    for i in (0..4).rev() {
        let v = u128::from(m) * u128::from(b[i]) + carry;
        p[i + 1] = u64::try_from(v & u128::from(u64::MAX)).unwrap_or(0);
        carry = v >> 64;
    }
    p[0] = u64::try_from(carry).unwrap_or(0);
    // x·64/π ≈ P·2^s with s = e + 5 − (i0 + 255): the fraction has fb = −s bits.
    let fb = usize::try_from(i0 + 255 - e - 5).unwrap_or(249);
    let mut k = i32::try_from(bits(&p, fb) & 127).unwrap_or(0);
    let mut f = [bits(&p, fb - 64), bits(&p, fb - 128), bits(&p, fb - 192)];
    // Round to the nearest integer: a fraction of 1/2 or more becomes 1 − f below the next one.
    let neg = f[0] >> 63 == 1;
    if neg {
        k = (k + 1) & 127;
        // 2^192 − F (two's complement of the 192-bit fraction).
        let mut borrow = true;
        for w in f.iter_mut().rev() {
            *w = !*w;
            if borrow {
                let (v, c) = w.overflowing_add(1);
                *w = v;
                borrow = c;
            }
        }
    }
    // Normalize the 192-bit magnitude and keep its top 128 bits.
    let hi = (u128::from(f[0]) << 64) | u128::from(f[1]);
    let lz = if hi != 0 { hi.leading_zeros() } else { 128 + f[2].leading_zeros() };
    let v: u128 = match lz {
        0 => hi,
        1..=127 => (hi << lz) | (u128::from(f[2]) << lz >> 64),
        _ => u128::from(f[2]) << (lz - 128) << 64,
    };
    // v·2^(−128 − lz) as a double-double: the top 53 bits exactly, the rest rounded.
    let top = v >> 75;
    let rest = v & ((1u128 << 75) - 1);
    #[allow(clippy::as_conversions, clippy::cast_precision_loss)] // 53 bits exact; the rest rounds
    let (th, tl) = (top as f64, rest as f64);
    let scale = -(128 + i32::try_from(lz).unwrap_or(0));
    let fd = dd::fast_two_sum(dd::ldexp(th, 75 + scale), dd::ldexp(tl, scale));
    (k, if neg { fd.neg() } else { fd })
}

/// `x = n·π/64 + r`: `(n mod 128, r)` with `|r| <= π/128` (a little more for huge `x`), for a
/// finite `x`.
fn reduce_pi64(x: f64) -> (i32, Dd) {
    let ax = dd::abs(x);
    let (n, r) = if ax < 1500.0 {
        let kf = dd::nearest_int(ax * SCALE_PI);
        (dd::to_i32(kf) & 127, reduce(Dd::from(ax), kf, &t::PI64_CW))
    } else {
        let (n, f) = payne_hanek(ax);
        (n, f.mul(Dd::of(t::PI64)))
    };
    if x < 0.0 { ((128 - n) & 127, r.neg()) } else { (n, r) }
}

/// `(sin x, cos x)` for a finite `x`.
fn sin_cos(x: f64) -> (Dd, Dd) {
    let (n, r) = reduce_pi64(x);
    let u = r.sqr().neg();
    let sr = r.mul(horner(u, &t::INV_ODD_FACT[..8], 4));
    let cr = horner(u, &t::INV_EVEN_FACT[..8], 4);
    let s = Dd::of(t::SIN_K_PI64[usize::try_from(n).unwrap_or(0)]);
    let c = Dd::of(t::SIN_K_PI64[usize::try_from((n + 32) & 127).unwrap_or(0)]);
    (s.mul(cr).add(c.mul(sr)), c.mul(cr).sub(s.mul(sr)))
}

/// The special operands of `sin`, `cos`, `tan`: NaN, ±∞ (domain), ±0 (`zero`, or `x`).
fn trig_special<F: Small>(x: F, zero: Option<f64>) -> Option<(F, Status)> {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return Some(r);
    }
    match n.kind {
        Kind::Inf => Some(elem::domain()),
        Kind::Zero => Some(zero.map_or((x, Status::OK), elem::exact)),
        _ => None,
    }
}

/// A `double` result of the fast path ([`fast`]) as `F` (only called when `F` is `f64`).
fn from_fast<F: Small>((v, st): (f64, Status)) -> (F, Status) {
    (F::from_bits(u128::from(v.to_bits())), st)
}

/// `sin(x)`.
pub fn sin<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    if let Some(r) = trig_special(x, None) {
        return r;
    }
    if F::PREC == 53 {
        if let Some(r) = fast::sin(x.widen(), mode) {
            return from_fast(r);
        }
    }
    sin_accurate(x, mode)
}

/// `sin(x)` for a finite nonzero `x` by the double-double path (the fast path's fallback).
fn sin_accurate<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    let xf = x.widen();
    if dd::abs(xf) < TINY {
        return elem::near(xf, xf < 0.0, mode); // x − x³/6
    }
    elem::finish(sin_cos(xf).0, 0, Residual::Above, mode)
}

/// `cos(x)`.
pub fn cos<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    if let Some(r) = trig_special(x, Some(1.0)) {
        return r;
    }
    if F::PREC == 53 {
        if let Some(r) = fast::cos(x.widen(), mode) {
            return from_fast(r);
        }
    }
    cos_accurate(x, mode)
}

/// `cos(x)` for a finite nonzero `x` by the double-double path.
fn cos_accurate<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    let xf = x.widen();
    if dd::abs(xf) < TINY {
        return elem::near(1.0, false, mode); // 1 − x²/2
    }
    elem::finish(sin_cos(xf).1, 0, Residual::Above, mode)
}

/// `tan(x)`.
pub fn tan<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    if let Some(r) = trig_special(x, None) {
        return r;
    }
    if F::PREC == 53 {
        if let Some(r) = fast::tan(x.widen(), mode) {
            return from_fast(r);
        }
    }
    tan_accurate(x, mode)
}

/// `tan(x)` for a finite nonzero `x` by the double-double path.
fn tan_accurate<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    let xf = x.widen();
    if dd::abs(xf) < TINY {
        return elem::near(xf, xf > 0.0, mode); // x + x³/3
    }
    let (s, c) = sin_cos(xf);
    elem::finish(s.div(c), 0, Residual::Above, mode)
}

/// `sincos(x)`: `(sin x, cos x)` with the flags of both.
pub fn sincos<F: Small>(x: F, mode: Rounding) -> ((F, F), Status) {
    let (s, st) = sin(x, mode);
    let (c, ct) = cos(x, mode);
    ((s, c), st.and(ct))
}

/// `atan t` for a non-negative double-double `t` (finite).
pub(crate) fn atan_dd(t: Dd) -> Dd {
    if t.hi > 1.152_921_504_606_847e18 {
        // Above 2^60: π/2 − 1/t, the correction already far below the last place of π/2.
        return Dd::of(t::PI_2).add_f64(-1.0 / t.hi);
    }
    if t.hi > 1.0 {
        return Dd::of(t::PI_2).sub(atan_dd(Dd::from(1.0).div(t)));
    }
    let jf = dd::nearest_int(t.hi * 64.0);
    let c = jf / 64.0;
    // u = (t − c)/(1 + t·c), |u| <= 1/128 + a little.
    let u = t.add_f64(-c).div(t.mul_f64(c).add_f64(1.0));
    let at = u.mul(horner(u.sqr().neg(), &t::INV_ODD[..9], 4));
    Dd::of(t::ATAN_J64[usize::try_from(dd::to_i32(jf)).unwrap_or(0)]).add(at)
}

/// `atan(x)`.
pub fn atan<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return r;
    }
    let xf = x.widen();
    match n.kind {
        Kind::Zero => return (x, Status::OK),
        Kind::Inf => return elem::finish(if n.neg { Dd::of(t::PI_2).neg() } else { Dd::of(t::PI_2) }, 0, Residual::Above, mode),
        _ => {}
    }
    if dd::abs(xf) < TINY {
        return elem::near(xf, xf < 0.0, mode); // x − x³/3
    }
    let a = atan_dd(Dd::from(dd::abs(xf)));
    elem::finish(if n.neg { a.neg() } else { a }, 0, Residual::Above, mode)
}

/// `asin(x)`.
pub fn asin<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return r;
    }
    let xf = x.widen();
    let a = dd::abs(xf);
    if n.kind == Kind::Zero {
        return (x, Status::OK);
    }
    if a > 1.0 || n.kind == Kind::Inf {
        return elem::domain_pos();
    }
    if a < TINY {
        return elem::near(xf, xf > 0.0, mode); // x + x³/6
    }
    // asin a = atan(a/√(1 − a²)), with 1 − a² = (1 − a)(1 + a) exactly.
    let d = two_sum(1.0, -a).mul(two_sum(1.0, a));
    let s = dd::sqrt(d);
    let r = if a == 1.0 {
        Dd::of(t::PI_2)
    } else if Dd::from(a).sub(s).hi <= 0.0 {
        atan_dd(Dd::from(a).div(s))
    } else {
        Dd::of(t::PI_2).sub(atan_dd(s.div(Dd::from(a))))
    };
    elem::finish(if n.neg { r.neg() } else { r }, 0, Residual::Above, mode)
}

/// `acos(x)`.
pub fn acos<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return r;
    }
    let xf = x.widen();
    if dd::abs(xf) > 1.0 || n.kind == Kind::Inf {
        return elem::domain_pos();
    }
    if xf == 1.0 {
        return elem::exact(0.0);
    }
    if xf == -1.0 {
        return elem::finish(Dd::of(t::PI), 0, Residual::Above, mode);
    }
    // acos x = 2·atan(√((1 − x)/(1 + x))).
    let q = two_sum(1.0, -xf).div(two_sum(1.0, xf));
    let r = atan_dd(dd::sqrt(q)).scale(2.0);
    elem::finish(r, 0, Residual::Above, mode)
}

/// `atan2(y, x)`: the angle of the point (x, y), with every signed-zero and infinite quadrant
/// of C Annex F.
pub fn atan2<F: Small>(y: F, x: F, mode: Rounding) -> (F, Status) {
    let (b, a) = (float::decode(y), float::decode(x));
    // NaN operands: x's payload first, as glibc returns it.
    if let Some(r) = fused::nan_operands_by::<F, F>(&[b, a], fused::Pick::Order([1, 0, 2])) {
        return r;
    }
    let pi = Dd::of(t::PI);
    let half = Dd::of(t::PI_2);
    let sign = |v: Dd| if b.neg { v.neg() } else { v };
    let done = |v: Dd| elem::finish::<F>(sign(v), 0, Residual::Above, mode);
    match (b.kind, a.kind) {
        (Kind::Zero, _) => {
            // ±0: x positive or +0 gives ±0, negative or −0 gives ±π.
            return if a.neg { done(pi) } else { (y, Status::OK) };
        }
        (Kind::Inf, Kind::Inf) => return done(if a.neg { pi.mul_f64(0.75) } else { Dd::of(t::PI_4) }),
        (Kind::Inf, _) | (_, Kind::Zero) => return done(half),
        (_, Kind::Inf) => {
            return if a.neg { done(pi) } else { (float::zero(b.neg), Status::OK) };
        }
        _ => {}
    }
    let (ay, ax) = (dd::abs(y.widen()), dd::abs(x.widen()));
    let (ny, nx) = (float::decode(ay), float::decode(ax));
    // Bring both near 1 by the same power of two; a ratio far outside the range is a limit.
    let shift = -nx.top().max(ny.top());
    let gap = ny.top() - nx.top();
    let theta = if gap > 120 {
        half
    } else if gap < -120 {
        // |y/x| below 2^-120: the angle is y/x minus a tiny amount (x > 0), or π minus it.
        if a.neg {
            pi
        } else {
            let q = arith::div_exact::<F>(&float::decode(y), &float::decode(x));
            let (v, flags) = elem_round_below::<F>(q, mode);
            return (v, float::range_errno(v, Status::flags(flags)));
        }
    } else {
        let (sy, sx) = (dd::ldexp(ay, shift), dd::ldexp(ax, shift));
        let t = if sy <= sx { atan_dd(Dd::from(sy).div(Dd::from(sx))) } else { half.sub(atan_dd(Dd::from(sx).div(Dd::from(sy)))) };
        if a.neg { pi.sub(t) } else { t }
    };
    done(theta)
}

/// Round an exact quotient whose true value is slightly smaller in magnitude.
pub(crate) fn elem_round_below<F: Float>(q: float::Exact, mode: Rounding) -> (F, crate::status::Flags) {
    let mut e = q;
    let len = 128 - e.sig.leading_zeros();
    let shift = (F::PREC + 3).saturating_sub(len).min(127u32.saturating_sub(len));
    e.sig <<= shift;
    e.exp -= i32::try_from(shift).unwrap_or(0);
    if !e.sticky {
        e.sig -= 1;
    }
    e.sticky = true;
    float::round::<F>(e, mode)
}

/// The `double` fast path of `sin`, `cos`, `tan` (M6-6, Ziv's strategy). It returns `None` when
/// it cannot prove its result equals the double-double path's, which then runs unchanged.
///
/// * Reduction for `|x| < 2^20`: `k = nearest(x·64/π)`, `|k| < 2^24.35`; `π/64 = P1 + P2 + P3 + τ`
///   with `P1` of 27 bits, `P2` of 25 bits (so `k·P1`, `k·P2` are exact), `|τ| < 2^-119.1`.
///   `x − k·P1` is exact (both are multiples of `min(ulp x, 2^-31)` and the difference is below
///   2^-5.3), `− k·P2` by `two_sum` (exact), `− k·P3` rounded twice (each `<= 2^-91`, since
///   `|k·P3| < 2^-38`). Absolute error of `r = rh + rl`: `< 2^-94.8 + 2·2^-91 < 2^-89.9`;
///   `|r| <= π/128·(1 + 2^-40) < 2^-5.34`. `sin`, `cos` have slope `<= 1`, so this is absolute in
///   the result: `A = 2^-89`.
/// * `sin(nπ/64 + r) = S + C·rh + S·(cos r − 1) + C·(sin r − rh)` with `S`, `C` from
///   `SIN_K_PI64` (double-double, `<= 2^-106`). Taylor tails: `sin` to `r^9` (next term
///   `< 2^-84`), `cos` to `r^8` (`< 2^-75.2`, times `|S| <= 2|y|`). `C·rh`, `S·(−rh²/2)` and the
///   two leading sums are exact (Dekker `two_prod`, `two_sum`); the rest (`|·| < 2^-17.5`) is
///   summed in `double`: at most ten roundings of `2^-72` plus the tails' own relative `2^-51`
///   of `2^-18.5`, `< 2^-66.5` absolute. With `|y| >= sin(π/128) > 2^-5.35` when `S != 0`, and
///   every term relative to `|y| ≈ |r|` when `S = 0`, the relative error is `< 2^-62.3`. Bound
///   used: `B = 2^-61` (factor 2.4 for the `double` evaluation of `ε` itself and the
///   double-double path's own `2^-100`).
/// * Rounding test: `y = yh + yl` normalized; the nearest rounding boundary is `½ulp(yh) − |yl|`
///   away (`¼ulp` below a power of two) to nearest, `|yl|` directed. The result is `yh` (nearest)
///   or `elem::finish` of `y` (directed) only when that distance exceeds `ε = A + B·|yh|`.
/// * `|x| < 2^-30` (nearest only): `sin`, `tan` round to `x`, `cos` to 1, as `elem::near` gives.
///   The value is computed in hardware, so a caller's flush-to-zero would act on it (the
///   envelope switches it off).
mod fast {
    use super::{elem, t, Dd, Residual, Rounding, Status, SCALE_PI, TINY};
    use crate::dd;
    use crate::stats;
    use crate::status::Flags;

    const P1: f64 = f64::from_bits(0x3fa9_21fb_5400_0000);
    const P2: f64 = f64::from_bits(0x3dc1_0b46_1000_0000);
    const P3: f64 = f64::from_bits(0x3c0a_6263_3145_c06e);
    const LIMIT: f64 = 1_048_576.0; // 2^20
    const A: f64 = f64::from_bits(0x3a60_0000_0000_0000); // 2^-89
    const B: f64 = f64::from_bits(0x3c20_0000_0000_0000); // 2^-61
    const MIN_NORMAL: f64 = f64::MIN_POSITIVE;
    const ONE_LESS: f64 = f64::from_bits(0x3fef_ffff_ffff_ffff); // 1 − 2^-53

    /// `x` (the nearest value of `sin x`, `tan x` for `|x| < 2^-30`), computed in hardware when it is
    /// subnormal: `x·(1 − 2^-53)` is inexact and below half an ulp from `x` there, so it rounds to `x`
    /// unless the caller's flush-to-zero (or denormals-are-zero) acts on it (#1077). `1 ± 2^-61` would
    /// round to 1 and make the multiplication exact, invisible to FTZ.
    #[inline(always)]
    fn tiny_x(x: f64) -> f64 {
        if dd::abs(x) < MIN_NORMAL { x * ONE_LESS } else { x }
    }
    // 1/k! with alternating signs.
    const S3: f64 = -1.0 / 6.0;
    const S5: f64 = 1.0 / 120.0;
    const S7: f64 = -1.0 / 5040.0;
    const S9: f64 = 1.0 / 362_880.0;
    const C4: f64 = 1.0 / 24.0;
    const C6: f64 = -1.0 / 720.0;
    const C8: f64 = 1.0 / 40_320.0;

    /// `x = k·π/64 + r`: `(k mod 128, r)` for `|x| < 2^20`.
    #[inline(always)]
    fn reduce(x: f64) -> (i32, Dd) {
        let kf = dd::nearest_int(x * SCALE_PI);
        let t1 = x - kf * P1;
        let s = dd::two_sum(t1, -(kf * P2));
        let rl = s.lo - kf * P3;
        (dd::to_i32(kf) & 127, dd::two_sum(s.hi, rl))
    }

    /// `sin(nπ/64 + r)` and its error bound.
    #[inline(always)]
    fn eval(n: i32, r: Dd) -> (Dd, f64) {
        let at = |i: i32| {
            let e = t::SIN_K_PI64[usize::try_from(i & 127).unwrap_or(0)];
            (f64::from_bits(e[0]), f64::from_bits(e[1]))
        };
        let ((sh, sl), (ch, cl)) = (at(n), at(n + 32));
        let (rh, rl) = (r.hi, r.lo);
        let zz = dd::two_prod(rh, rh);
        let z = zz.hi;
        let crh = -0.5 * z; // exact
        let crl = -0.5 * zz.lo - rh * rl + z * z * (C4 + z * (C6 + z * C8));
        let sr = rl - 0.5 * z * rl + rh * z * (S3 + z * (S5 + z * (S7 + z * S9)));
        let p = dd::two_prod(ch, rh);
        let q = dd::two_prod(sh, crh);
        let a = dd::two_sum(sh, p.hi);
        let b = dd::two_sum(a.hi, q.hi);
        let lo = a.lo + b.lo + p.lo + q.lo + sl + cl * rh + sh * crl + sl * crh + ch * sr + cl * sr;
        let y = dd::fast_two_sum(b.hi, lo);
        (y, A + B * dd::abs(y.hi))
    }

    /// The rounding test: `y` rounded in `mode` when every value within `eps` of it rounds alike.
    #[inline(always)]
    fn settle(y: Dd, eps: f64, mode: Rounding) -> Option<(f64, Status)> {
        let bits = y.hi.to_bits();
        let ex = (bits >> 52) & 0x7ff;
        if !(55..0x7fe).contains(&ex) {
            return None;
        }
        let half = f64::from_bits((ex - 53) << 52);
        let down = (y.lo < 0.0) != (y.hi < 0.0);
        let gap = if bits & ((1 << 52) - 1) == 0 && down { 0.5 * half } else { half };
        let ylo = dd::abs(y.lo);
        // A NaN or infinite bound fails too.
        if (gap - ylo).partial_cmp(&eps) != Some(core::cmp::Ordering::Greater) {
            return None;
        }
        match mode {
            Rounding::Nearest => Some((y.hi, Status::flags(Flags::INEXACT))),
            _ if ylo > eps => Some(elem::finish::<f64>(y, 0, Residual::Above, mode)),
            _ => None,
        }
    }

    /// `|x| < 2^-30` to nearest: `v` (computed in hardware from `x`), inexact, underflow when `v` is subnormal.
    #[inline(always)]
    fn tiny(x: f64, v: f64, mode: Rounding) -> Option<(f64, Status)> {
        if mode != Rounding::Nearest || dd::abs(x) == MIN_NORMAL {
            return None;
        }
        // Underflow only for a subnormal result (sin, tan of a subnormal x); cos rounds to 1.
        let f = if dd::abs(v) < MIN_NORMAL { Flags(Flags::INEXACT.0 | Flags::UNDERFLOW.0) } else { Flags::INEXACT };
        Some((v, Status::flags(f)))
    }

    /// One fast evaluation, counted for the fallback rate.
    fn counted(func: &'static str, r: Option<(f64, Status)>) -> Option<(f64, Status)> {
        stats::record(func, r.is_some());
        r
    }

    pub(super) fn sin(x: f64, mode: Rounding) -> Option<(f64, Status)> {
        let ax = dd::abs(x);
        if ax < TINY {
            return tiny(x, tiny_x(x), mode);
        }
        if ax >= LIMIT {
            return None;
        }
        let (n, r) = reduce(x);
        let (y, eps) = eval(n, r);
        counted("sin", settle(y, eps, mode))
    }

    pub(super) fn cos(x: f64, mode: Rounding) -> Option<(f64, Status)> {
        let ax = dd::abs(x);
        if ax < TINY {
            return tiny(x, 1.0 - x * x, mode); // rounds to 1
        }
        if ax >= LIMIT {
            return None;
        }
        let (n, r) = reduce(x);
        let (y, eps) = eval(n + 32, r);
        counted("cos", settle(y, eps, mode))
    }

    /// `tan = sin/cos` in double-double: relative error `<= εs/|s| + εc/|c| + 2^-100`.
    pub(super) fn tan(x: f64, mode: Rounding) -> Option<(f64, Status)> {
        let ax = dd::abs(x);
        if ax < TINY {
            return tiny(x, tiny_x(x), mode);
        }
        if ax >= LIMIT {
            return None;
        }
        let (n, r) = reduce(x);
        let ((s, es), (c, ec)) = (eval(n, r), eval(n + 32, r));
        let qh = s.hi / c.hi;
        let p = dd::two_prod(qh, c.hi);
        let ql = ((s.hi - p.hi - p.lo) + s.lo - qh * c.lo) / c.hi;
        let y = dd::fast_two_sum(qh, ql);
        let rel = es / dd::abs(s.hi) + ec / dd::abs(c.hi) + B * B;
        counted("tan", settle(y, 1.01 * rel * dd::abs(qh), mode))
    }

    #[cfg(test)]
    mod tests {
        use super::super::{cos_accurate, sin_accurate, tan_accurate};
        use super::*;

        type Fun = (fn(f64, Rounding) -> Option<(f64, Status)>, fn(f64, Rounding) -> (f64, Status), &'static str);
        const FUNS: [Fun; 3] = [(sin, sin_accurate::<f64>, "sin"), (cos, cos_accurate::<f64>, "cos"), (tan, tan_accurate::<f64>, "tan")];
        const MODES: [Rounding; 4] = [Rounding::Nearest, Rounding::Down, Rounding::Up, Rounding::Zero];

        /// Every fast result equals the double-double path's (value bits and status), in all four
        /// modes; returns the number of fallbacks.
        fn check(xs: &[f64]) -> [u64; 3] {
            let mut fb = [0u64; 3];
            for (i, &(f, acc, name)) in FUNS.iter().enumerate() {
                for &x in xs {
                    for mode in MODES {
                        match f(x, mode) {
                            Some((v, st)) => {
                                let (w, wt) = acc(x, mode);
                                assert_eq!((name, x, mode, v.to_bits(), st), (name, x, mode, w.to_bits(), wt));
                            }
                            None => fb[i] += 1,
                        }
                    }
                }
            }
            fb
        }

        fn random(n: usize, scale: f64) -> impl Iterator<Item = f64> {
            let mut s = 0x9e37_79b9_7f4a_7c15u64;
            (0..n).map(move |_| {
                s ^= s << 13;
                s ^= s >> 7;
                s ^= s << 17;
                #[allow(clippy::as_conversions, clippy::cast_precision_loss)]
                let u = (s >> 11) as f64 / 9_007_199_254_740_992.0;
                (2.0 * u - 1.0) * scale
            })
        }

        /// The table both paths share, checked independently of either: every entry
        /// `(S, C) = (sin kπ/64, cos kπ/64)` as double-double satisfies `S² + C² = 1` to `2^-100`
        /// (a 1-ulp change of any nonzero high word moves it by more than `2^-62`).
        #[test]
        fn trig_table_is_on_the_unit_circle() {
            // Clauses: s10_3_f_0.
            for k in 0..128i32 {
                let e = |i: i32| {
                    let v = t::SIN_K_PI64[usize::try_from(i & 127).unwrap_or(0)];
                    (f64::from_bits(v[0]), f64::from_bits(v[1]))
                };
                let ((sh, sl), (ch, cl)) = (e(k), e(k + 32));
                let (ps, pc) = (dd::two_prod(sh, sh), dd::two_prod(ch, ch));
                let u = dd::two_sum(ps.hi, pc.hi);
                let err = (u.hi - 1.0) + u.lo + ps.lo + pc.lo + 2.0 * (sh * sl + ch * cl);
                assert!(dd::abs(err) < f64::from_bits(0x39b0_0000_0000_0000), "entry {k}: S²+C²-1 = {err:e}");
            }
        }

        #[test]
        fn trig_fast_path_matches_the_accurate_path() {
            // Clauses: s10_3_f_0.
            let mut xs: Vec<f64> = random(40_000, 10.0).chain(random(10_000, LIMIT)).collect();
            // Near multiples of π/2 (the reduction's hard cases) and their neighbours.
            for k in [1.0, 2.0, 3.0, 7.0, 64.0, 355.0, 1e4, 6.0e5] {
                let m = k * core::f64::consts::FRAC_PI_2;
                for d in -2i64..=2 {
                    xs.push(f64::from_bits(m.to_bits().wrapping_add_signed(d)));
                }
            }
            // Tiny and subnormal operands, the boundaries 2^-30, 2^-1022 and 2^20.
            xs.extend([1e-310, -5e-324, 2.2e-308, -1e-12, TINY, -TINY, MIN_NORMAL, LIMIT * (1.0 - 1e-16), 1e-300]);
            let fb = check(&xs);
            let total = u64::try_from(xs.len() * 4).unwrap_or(1);
            for (i, &n) in fb.iter().enumerate() {
                // Ceiling (speed, not correctness): sin and cos at most one fallback in 50
                // evaluations, tan one in 25 (its bound is the sum of both, so about twice the
                // rate: measured 795 of 32196 on 2026-10-23).
                let ceiling = if i == 2 { 25 } else { 50 };
                eprintln!("{}: {n} fallbacks of {total}", FUNS[i].2);
                assert!(n * ceiling <= total, "{}: {n} fallbacks of {total}", FUNS[i].2);
            }
        }
    }
}
