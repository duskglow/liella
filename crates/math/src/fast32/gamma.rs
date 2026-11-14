//! tgammaf and lgammaf from `double` kernels and [`super::round`]'s rounding test
//! (LIBM-F32-GAMMA; rulings #2760, #2784, #2783, #2889).

#![allow(clippy::as_conversions, clippy::arithmetic_side_effects, clippy::indexing_slicing)] // as in fast32.rs: bit fields, bounded exponents, table indices below 128

use super::*;

/// Γ(5/2 + s) on [-1/2, 1/2], degree 14 (`crates/math/fits/fast32_tgamma.fit`: relative 2^-50.17 = 7.1u).
const TG: [u64; 15] = crate::coeffs::fast32_tgamma::FAST32_TGAMMA;
/// lnΓ(1 + z)/z on [-1/8, 1/8], degree 11 (`crates/math/fits/fast32_lgamma.fit`: relative 2^-49.77 = 9.4u).
const LG: [u64; 12] = crate::coeffs::fast32_lgamma::FAST32_LGAMMA;
/// The gamma kernels' domain: |x| ≥ 2^-20 (so every sum x + j below is exact), -42 < x < 36.
const GAMMA_MIN: f64 = 1.0 / 1_048_576.0;
/// Stirling's series from here (lgammaf only).
const STIRLING_X: f64 = 36.0;
/// RN(ln(2π)/2 - 1/2) (60-digit decimal arithmetic).
const HALF_LN_2PI_M_HALF: f64 = f64::from_bits(0x3fda_cfe3_90c9_7d69);
#[cfg(test)]
const GAMMA_BOUND: f64 = 55.1;
/// tgammaf: ε ≤ 55.1u ([`gamma_core`]).
pub(super) const TGAMMA_DELTA: u64 = 96;
/// lgammaf, in ulp of y: near 1 ≤ 12, near 2 ≤ 41.6, ln|Γ| with |y| ≥ 1 ≤ 30.5, Stirling ≤ 29.1 ([`lgamma_core`]).
#[cfg(test)]
const LGAMMA_BOUND: f64 = 41.6;
const LGAMMA_DELTA: u64 = 72;
/// lgammaf, ln|Γ| with 2^-8 ≤ |y| < 1: ≤ 7260 ulp of y (absolute error, near the negative zeros).
#[cfg(test)]
const LGAMMA_SMALL_BOUND: f64 = 7260.0;
const LGAMMA_SMALL_DELTA: u64 = 11_264;
/// lgammaf for -2^23 < x < -42 by reflection ([`lgamma_reflect`]): ≤ 8.2 ulp of y.
#[cfg(test)]
const LGAMMA_REFLECT_BOUND: f64 = 8.2;
/// Distinct from the other pieces' margins: [`lgamma_rest`] reads the piece from it.
const LGAMMA_REFLECT_DELTA: u64 = 16;

/// c[0] + s·(c[1] + s·(...)) by Horner.
#[inline(always)]
fn horner<const N: usize>(c: &[u64; N], s: f64) -> f64 {
    c.iter().rev().fold(0.0, |r, &k| r * s + f64::from_bits(k))
}

/// Γ(x) as a `double` with relative error ≤ 55.1u (u = 2^-53), for 2^-20 ≤ |x|, -42 < x < 36, x
/// not a negative integer (`None` outside).
///
/// n = the integer nearest x - 5/2 (SHIFT), t = x - n in [2, 3]: x is a `float` with |x| < 64, a
/// multiple of 2^-43 (|x| ≥ 2^-20), so t, s = t - 5/2 and every x + j and t + j below are exact.
/// n ≤ 0: Γ(x) = Γ(t)/(x (x + 1)···(t - 1)), -n ≤ 44 factors; n > 0: Γ(x) = Γ(t)·t (t + 1)···(x - 1),
/// n ≤ 34 factors. Γ(t) = P(s): the fit 7.1u; Horner on |s| ≤ 1/2 with P in [0.88, 2]: the step
/// errors weighted by |c_k s^k|·2k over |P| sum to < 3u, plus the last rounding: ≤ 4u. The product
/// and the final multiplication or division round once per factor: ≤ 44u. Total ≤ 55.1u (second
/// order included). A negative integer x gives t = 2 or 3 exactly and is refused (a pole).
#[inline(always)]
pub(super) fn gamma_core(xd: f64) -> Option<f64> {
    if !(crate::dd::abs(xd) >= GAMMA_MIN && xd > -42.0 && xd < 36.0) {
        return None;
    }
    let kd = (xd - 2.5) + SHIFT;
    let n = kd.to_bits() as i32;
    let t = xd - (kd - SHIFT);
    if xd < 0.0 && (t <= 2.0 || t >= 3.0) {
        return None;
    }
    let p = horner(&TG, t - 2.5);
    let mut q = 1.0;
    if n > 0 {
        for j in 0..n {
            q *= t + f64::from(j);
        }
        Some(p * q)
    } else {
        for j in 0..-n {
            q *= xd + f64::from(j);
        }
        Some(p / q)
    }
}

/// ln v for a positive normal `double` v in [2^-181, 2^181], absolute error ≤ (1.45 + 2|y|)u.
///
/// As [`log1p_core`]'s e ≠ 0 case: v = 2^e·m, r = RN(m·L) - 1 (the product ≤ 2^-53 absolute, the
/// subtraction exact): 1.01u; LOG_C ≤ 0.41u; e·LN2_HI exact (|e| ≤ 181, 8 bits by 44); the series
/// remainder and the small sums < 0.02u; the two last sums u·|hi| and u·|y|, |hi| ≤ |y| + 0.0081.
#[inline(always)]
fn ln64(v: f64) -> f64 {
    let vb = v.to_bits();
    let e = (vb.wrapping_sub(0x3fe8_0000_0000_0000) as i64 >> 52) as i32;
    let m = f64::from_bits(vb.wrapping_sub((e as i64 as u64) << 52));
    let i = ((m.to_bits() >> 45) & 127) as usize;
    let r = m * LOG_INV[i] - 1.0;
    let s = r * r * (-0.5 + r * (D3 + r * (D4 + r * (D5 + r * (D6 + r * (D7 + r * D8))))));
    let ef = f64::from(e);
    (ef * LN2_HI + LOG_C[i]) + (r + (s + ef * LN2_LO))
}

/// lnΓ(x) = ln|Γ(x)| as a `double` y and its margin (ulp of y), piecewise, for a finite x:
/// * |x - 1| ≤ 1/8: y = z·q(z), z = x - 1 exact. q: the fit 9.4u; Horner on |z| ≤ 1/8 with
///   |q| ≥ 0.47: the weighted step errors < 1.6u; the product u: ≤ 12u relative.
/// * |x - 2| ≤ 1/8: y = ln(1 + z) + z·q(z), z = x - 2 (exact, a `float`, |z| ≥ 2^-23): [`log1p_core`]
///   6.8u of |L| ≤ 2.8|y|, z·q 12u of |A| ≤ 1.8|y|, the sum u: ≤ 41.6u relative.
/// * x ≥ 36: Stirling, y = (x - 1/2)(ln x - 1) + (ln(2π)/2 - 1/2 + 1/(12x) - 1/(360x³) + 1/(1260x⁵)).
///   ln x by [`log_core`] (17.5u, case B1, ln x ≥ 3.58): ln x - 1 ≥ 2.58 carries ≤ 25.3u, x - 1/2
///   ≤ u, the product u: 27.3u of T ≤ y; the constants and the series < 0.43 against y ≥ 92 and
///   the last sum u: ≤ 28.4u; the omitted term 1/(1680x⁷) ≤ 2^-46.9 < 0.74u of y: ≤ 29.1u.
/// * otherwise ([`gamma_core`]'s domain): y = ln|G|, absolute error ≤ 55.1u·1.0001 + (1.45 + 2|y|)u
///   ≤ (56.7 + 2.1|y|)u, in ulp of y (|y| in [2^E, 2^(E+1))): ≤ 56.7·2^-(E+1) + 2.1: ≤ 30.5 for
///   |y| ≥ 1, ≤ 7260 for |y| ≥ 2^-8. Below 2^-8 (next to the negative zeros) the bound is too wide
///   for the rounding test: those operands and |x| < 2^-20 go to [`lgamma_edge`]; the poles are
///   refused.
///
/// (Tests only: the exports go through [`eval_lgamma`].) The third value says the margin is
/// [`lgamma_edge`]'s.
#[cfg(test)]
pub(super) fn lgamma_core(x: f32) -> Option<(f64, u64, bool)> {
    let (y, d, _, edge) = lgamma_core_signed(x)?;
    Some((y, d, edge))
}

/// [`lgamma_core`] and the sign of Γ(x) (1 or -1). Only the last branch sees x < 0, and there the
/// sign of [`gamma_core`]'s Γ(x) is exact: every factor x + j is exact and nonzero (a pole is
/// refused) and P > 0; its sign bit spread over the word and or-ed with 1 gives ±1. The other
/// branches have x > 0. So no operand is converted to an integer to find the parity of its integer
/// part (D-10.95), and the positive path computes no parity at all. The fourth value is true when
/// [`lgamma_edge`] answered (its [`Status::hw`] stays conservative, see there).
#[inline(always)]
fn lgamma_core_signed(x: f32) -> Option<(f64, u64, i32, bool)> {
    lgamma_core_pieces::<true>(x)
}

/// [`lgamma_core_signed`] (`EDGE`), or without [`lgamma_edge`]'s pieces (`!EDGE`: x < -42 still
/// by [`lgamma_reflect`], the rest of [`lgamma_edge`]'s operands `None`). [`lgamma_rest`] takes the
/// latter: it covers no refusal of those pieces, and recomputing them (the double-double kernel)
/// to find that out cost the cbench row beside the zero at -3.14382577 5 instructions per call
/// (LIBM-LGAMMAF-EDGE-3).
#[inline(always)]
fn lgamma_core_pieces<const EDGE: bool>(x: f32) -> Option<(f64, u64, i32, bool)> {
    let xd = f64::from(x);
    let z = xd - 1.0;
    if crate::dd::abs(z) <= 0.125 {
        return Some((z * horner(&LG, z), LGAMMA_DELTA, 1, false));
    }
    let z = xd - 2.0;
    if crate::dd::abs(z) <= 0.125 {
        return Some((log1p_core(z as f32)? + z * horner(&LG, z), LGAMMA_DELTA, 1, false));
    }
    if xd >= STIRLING_X {
        let lx = log_core(x)?;
        let w = 1.0 / xd;
        let w2 = w * w;
        let s = w * (1.0 / 12.0 + w2 * (-1.0 / 360.0 + w2 * (1.0 / 1260.0)));
        return Some(((xd - 0.5) * (lx - 1.0) + (HALF_LN_2PI_M_HALF + s), LGAMMA_DELTA, 1, false));
    }
    let Some(g) = gamma_core(xd) else {
        return if EDGE { lgamma_wide(xd) } else if xd < -42.0 { lgamma_reflect(xd) } else { None };
    };
    let y = ln64(crate::dd::abs(g));
    let a = crate::dd::abs(y);
    if a >= 1.0 {
        Some((y, LGAMMA_DELTA, (g.to_bits() as i64 >> 63) as i32 | 1, false))
    } else if a >= 1.0 / 256.0 {
        Some((y, LGAMMA_SMALL_DELTA, (g.to_bits() as i64 >> 63) as i32 | 1, false))
    } else if EDGE {
        lgamma_edge(xd)
    } else {
        None
    }
}

/// 2^-60, [`crate::fast_gamma::TINY`]: below it lnΓ(x) = -ln|x| to well within a `double` ulp.
const LGAMMA_TINY: f64 = f64::from_bits(0x3c30_0000_0000_0000);
/// 2^26: the largest margin [`lgamma_edge`] hands to [`super::round`] (whose test holds below 2^27).
const EDGE_MAX_MARGIN: f64 = 67_108_864.0;

/// lnΓ(x) where [`gamma_core`] refuses x: x < -42 by [`lgamma_reflect`] (its margin is not
/// [`lgamma_edge`]'s: the fourth value is false there), the rest by [`lgamma_edge`]. Its own
/// function, so that [`lgamma_edge`]'s other call (|y| < 2^-8, the cbench row's x = -3.14382577)
/// does not pass the test (the test and the inlined reflection inside [`lgamma_edge`] cost that
/// row 6 instructions per call, LIBM-LGAMMAF-EDGE-3).
#[cold]
#[inline(never)]
fn lgamma_wide(xd: f64) -> Option<(f64, u64, i32, bool)> {
    if xd < -42.0 {
        return lgamma_reflect(xd);
    }
    lgamma_edge(xd)
}

/// lnΓ(x) for the operands [`lgamma_core_signed`]'s `double` pieces refuse, as its (y, margin in
/// ulp of y, sign of Γ(x)), or `None` (the poles, x ≤ -16 off the poles, 0, NaN: the accurate
/// path; [`lgamma_wide`] takes x < -42 before it).
/// Cold: on the cbench row's 256 operands only x = -3.14382577 comes here (LIBM-F32-REST-NEXT).
/// * |y| < 2^-8 beside the negative zeros, and 2^-60 ≤ |x| < 2^-20: the `double` tier's
///   double-double kernel ([`crate::fast_gamma::lgamma_kernel`], x in its domain: |x| < 16, not a
///   negative integer), Y = Y.hi + Y.lo with |lnΓ(x) - Y| ≤ ε (its proved absolute bound: tables
///   2^-75 relative, D 2^-99, the log and the sums 2^-96). v = RN(Y.hi + Y.lo): |lnΓ(x) - v| ≤
///   ε + ulp(v)/2. With q = ε/ulp(v) (exact: ulp(v) a power of two, v ≥ 2^-126 so ulp(v) ≥ 2^-178
///   normal), the margin d = ⌊q⌋ + 2 > q + 1/2 is strict, as [`super::round`] requires; q ≥ 2^26 is
///   refused (round's test needs d < 2^27). No `float` x ≤ -16 other than a pole lies within the
///   |y| < 2^-8 window of a zero of lnΓ (the windows shrink like 1/n!), so the kernel's |x| < 16
///   loses no operand that could pass the test.
/// * 0 < |x| < 2^-60: lnΓ(x) = -ln|x| - γx + R, |R| ≤ 0.83x² (ζ(2)/2 bounds the series' terms, which
///   alternate and decrease). y = -ln64(|x|): |x| ≥ 2^-149 is in ln64's range, |y| in [41.5, 103.3],
///   |y| in [2^E, 2^(E+1)), E ≥ 5; ln64's (1.45 + 2|y|)u is ≤ 1.45·2^-(E+1) + 2 ≤ 2.03 ulp(y); the
///   dropped γx + R ≤ 2^-59.9 against ulp(y) ≥ 2^-47 adds < 0.001: ≤ 2.04 ulp, so LGAMMA_DELTA
///   (72) holds. Γ(x) has the sign of x.
///
/// Flags: inexact, as every piece ([`super::finish`]); [`eval_lgamma`] widens the hardware hint
/// [`Status::hw`] to [`Status::HW_ANY`] for these results, as the `double` tier does for the same
/// kernel, so the in-place envelope always writes MXCSR after a call that came here (no proof
/// that the kernel's intermediates stay normal is needed; the cost is on this cold path only).
#[cold]
#[inline(never)]
fn lgamma_edge(xd: f64) -> Option<(f64, u64, i32, bool)> {
    let a = crate::dd::abs(xd);
    if a < LGAMMA_TINY {
        if a == 0.0 {
            return None;
        }
        return Some((-ln64(a), LGAMMA_DELTA, if xd < 0.0 { -1 } else { 1 }, true));
    }
    if !crate::fast_gamma::domain(xd, false) {
        return None;
    }
    let (y, eps, sign) = crate::fast_gamma::lgamma_kernel(xd)?;
    let v = y.hi + y.lo;
    let e = v.to_bits() & 0x7ff0_0000_0000_0000;
    if e < 0x3810_0000_0000_0000 {
        return None;
    }
    let q = eps / (f64::from_bits(e) / 4_503_599_627_370_496.0);
    if q.is_nan() || q >= EDGE_MAX_MARGIN {
        return None;
    }
    // RANGE: 0 <= q < 2^26 (NaN and larger returned above).
    Some((v, q as u64 + 2, sign, true))
}

/// lnΓ(x) for a `float` -2^23 < x < -42 off the poles, as (y, [`LGAMMA_REFLECT_DELTA`], sign of
/// Γ(x), false); `None` for the poles, x ≤ -2^23 (every such `float` is a pole), -∞ and NaN.
///
/// With X = -x: Γ(x)·Γ(X)·X = Γ(x)·Γ(1 - x) = π/sin πx (DLMF 5.5.3, 5.5.1), Γ(1 - x) > 0, so
/// lnΓ(x) = -ln(X·|sin πx|/π) - lnΓ(X) and sign Γ(x) = sign sin πx. No cancellation: lnΓ(X) ≥
/// lnΓ(42) > 114.03, while A = -ln(X·|sin πx|/π) lies in [-14.8, 9.47] (X·|sin πx| ≥ 2^-12, as the
/// distance of X to an integer is at least ulp(X) = 2^(e-23) ≥ 2^-18 and |sin πd| ≥ 2d), so
/// y < -104.5 and lnΓ(X) ≤ 1.0906·|y|.
/// * Reduction, exact: k = the integer nearest x/2 (|x/2| < 2^22), r = x - 2k in [-1, 1] (x and 2k
///   multiples of 2^-18 within 1), sin πx = sin πr, sign r = sign Γ(x); t = min(|r|, 1 - |r|) in
///   (0, 1/2] (exact), |sin πx| = sin πt; t = 0 is a pole. s = sin πt: t ≤ 1/4, g = 2t ≤ 1/2, s =
///   g·S(g²), S = sin(πg/2)/g; else g = 1 - 2t < 1/2, s = C(g²), C = cos(πg/2) (the Taylor
///   polynomials of the `float` Bessel band, [`super::bessel::LG_SIN`], their remainder below 2^-62
///   at |g| = 1/2; g² exact, g having at most 18 bits). Horner with S ≥ 1.41, C ≥ 0.70: the final
///   sum and product, the weighted inner steps and the coefficients' own rounding ≤ 3u relative,
///   the product g·S u: s within 4u relative.
/// * v = X·s·(1/π) within 4 + 1 + 1 + 0.5 = 6.5u relative (to second order); a = ln64(v) (v in
///   [2^-14, 2^22], ln64's range): |a - ln(X s/π)| ≤ 6.51u + (1.45 + 2·14.8)u ≤ 37.6u absolute.
/// * L = lnΓ(X) by Stirling as in [`lgamma_core_signed`], with ln X = ln64(X) (absolute (1.45 + 2 ln X)u,
///   ln X in [3.73, 16]): ln X - 1 within (1.45 + 2 ln X)/(ln X - 1) + 1 ≤ 4.26u relative (largest
///   at X = 42), X - 1/2 exact, the product u: T within 5.27u·T, T ≤ L; RN(ln(2π)/2 - 1/2) 0.25u,
///   its sum with the series 0.42u, the series' own rounding < 0.01u, the omitted 1/(1680X⁷) ≤
///   2.3·10^-15 < 21u absolute; the last sum u·L: L within 6.27u·L + 21.7u.
/// * y = -(a + L), rounded u·|y|. Total ≤ 6.27u·(|y| + 9.47) + 21.7u + 37.6u + u·|y|, that is
///   7.27u·|y| plus 118.7u; with u·|y| < ulp(y) and ulp(y) ≥ 2^-46 = 128u (|y| ≥ 64): ≤ 8.2 ulp(y)
///   ([`LGAMMA_REFLECT_BOUND`]; the margin 16 ≥ 1.5·8.2 + 1, as [`super::round`] wants).
///
/// Every intermediate is a normal `double` (s ≥ sin(π·2^-18), the Taylor terms ≥ 2^-34·10^-14),
/// so the hardware raises inexact only, as [`super::finish`] reports.
#[inline(always)]
fn lgamma_reflect(xd: f64) -> Option<(f64, u64, i32, bool)> {
    if xd.is_nan() || xd <= -8_388_608.0 {
        return None;
    }
    let r = xd - 2.0 * crate::dd::nearest_int(0.5 * xd);
    let ar = crate::dd::abs(r);
    let t = if ar > 0.5 { 1.0 - ar } else { ar };
    if t == 0.0 {
        return None;
    }
    let s = if t <= 0.25 {
        let g = 2.0 * t;
        g * super::bessel::horner_f10(&super::bessel::LG_SIN, g * g)
    } else {
        let g = 1.0 - 2.0 * t;
        super::bessel::horner_f10(&super::bessel::LG_COS, g * g)
    };
    let big = -xd;
    let a = ln64(big * s * super::bessel::INV_PI_D);
    let w = 1.0 / big;
    let w2 = w * w;
    let ser = w * (1.0 / 12.0 + w2 * (-1.0 / 360.0 + w2 * (1.0 / 1260.0)));
    let l = (big - 0.5) * (ln64(big) - 1.0) + (HALF_LN_2PI_M_HALF + ser);
    Some((-(a + l), LGAMMA_REFLECT_DELTA, (r.to_bits() as i64 >> 63) as i32 | 1, false))
}

// The operands whose last try the rounding test refuses in the pieces [`lgamma_rest`] covers,
// sorted by `float` bits of x: bits 32..63 x, 0..30 |F| for F the `float` nearest lnΓ(x), bit 31
// set when |lnΓ(x)| lies above |F|. Written by `lgammaf_table_is_the_refused_set` (the plant
// target "table").
include!("lgamma_tab.rs");

/// Whether [`lgamma_rest`] covers a refused answer of the piece with margin `d` (`edge`: from
/// [`lgamma_edge`]'s kernels): the reflection and 2^-8 ≤ |y| < 1. Not [`lgamma_edge`]'s own
/// refusals (|x| < 2^-20 and |y| < 2^-8: 608 operands, 594 of them below 2^-60; the accurate path,
/// as before).
#[inline(always)]
fn lgamma_tabled(d: u64, edge: bool) -> bool {
    !edge && (d == LGAMMA_SMALL_DELTA || d == LGAMMA_REFLECT_DELTA)
}

/// `lgammaf` by the fast path with the sign of Γ(x) (`signgam`), or `None` (see [`eval`]): the
/// value as `eval(Fast::Lgamma, ..)`, the sign from Γ(x) itself ([`lgamma_core_signed`]): the
/// first try ([`lgamma_first`]), then on any refusal the second tries ([`lgamma_rest`], from x and
/// `mode` alone). `lgammaf` itself ([`crate::special::lgamma`]) calls the two apart, so that its
/// refusal path makes one cold call.
#[inline(always)]
pub fn eval_lgamma(x: f32, mode: Rounding) -> Option<((f32, Status), i32)> {
    match lgamma_first(x, mode) {
        Some(r) => Some(r),
        None => lgamma_rest(x, mode),
    }
}

/// The first try: the pieces of [`lgamma_core_signed`] and one rounding test.
#[inline(always)]
pub fn lgamma_first(x: f32, mode: Rounding) -> Option<((f32, Status), i32)> {
    let (y, d, sign, edge) = lgamma_core_signed(x)?;
    let (v, st) = super::finish(x, y, d, mode, "lgammaf")?;
    Some(((v, if edge { st.hw(Status::HW_ANY) } else { st }), sign))
}

/// The second tries after [`lgamma_first`] refused x: its pieces again (y with margin `d`, `edge`
/// as [`lgamma_core_signed`]'s; `None` there, the poles and the other operands no piece takes,
/// stays `None`), then for 2^-8 ≤ |y| < 1 [`lgamma_edge`]'s double-double kernel; then, in the
/// pieces [`lgamma_tabled`] names, the table (a binary search by x's bits). A table entry's
/// value is rebuilt a 2^-28 relative step from F on its side, with y's sign (|y - lnΓ(x)| is far
/// below |y| in every covered piece), so every rounding mode and the inexact flag follow from one
/// more rounding test with margin 1. `None` otherwise (the accurate path). The hardware hint is
/// [`Status::HW_ANY`] (the kernel's intermediates are not tracked here; cold).
#[cold]
#[inline(never)]
pub fn lgamma_rest(x: f32, mode: Rounding) -> Option<((f32, Status), i32)> {
    let (y, d, sign, edge) = lgamma_core_pieces::<false>(x)?;
    if !lgamma_tabled(d, edge) {
        return None;
    }
    let st = Status::flags(Flags::INEXACT).hw(Status::HW_ANY);
    if d == LGAMMA_SMALL_DELTA {
        if let Some((y2, d2, _, _)) = lgamma_edge(f64::from(x)) {
            if let Some(v) = round(y2, d2, mode) {
                stats::record("lgammaf", true);
                return Some(((v, st), sign));
            }
        }
    }
    let i = LGT.binary_search_by(|e| ((e >> 32) as u32).cmp(&x.to_bits())).ok()?;
    let e = LGT[i];
    let f = f64::from(f32::from_bits(e as u32 & 0x7fff_ffff));
    let m = if e & 0x8000_0000 != 0 { f + f * 3.725_290_298_461_914e-9 } else { f - f * 3.725_290_298_461_914e-9 }; // 2^-28
    let v = round(if y < 0.0 { -m } else { m }, 1, mode)?;
    stats::record("lgammaf", true);
    Some(((v, st), sign))
}

/// `tgammaf` by the fast path, or `None` (see [`eval`]).
#[inline(always)]
pub fn eval_tgamma(x: f32, mode: Rounding) -> Option<(f32, Status)> {
    eval(Fast::Tgamma, x, mode)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tests::{accurate, agree_in, exhaustive_by, MODES};
    use core::sync::atomic::{AtomicU64, Ordering};

    /// The gamma operands: a grid over [-44, 44] (the kernels' pieces and edges, the bench rows'
    /// [-10, 10]) and a stride over every encoding (Stirling's range, the tiny and huge ones).
    fn gamma_sample() -> impl Iterator<Item = f32> + Clone {
        let grid = (0..1u32 << 11).map(|i| -44.0 + 88.0 * (i as f32 + 0.37) / 2048.0);
        let edges = [0.875f32, 1.125, 1.875, 2.125, 36.0, -41.99, 35.99, 1.0, 2.0, -3.0, -2.457, 9.536_743e-7, -9.536_743e-7];
        let wide = (0u32..u32::MAX).step_by(1_048_573).map(f32::from_bits);
        grid.chain(edges).chain(wide)
    }

    /// The gamma kernels' margins cover their bounds (D-10.13 rounding test). (No spec clause.)
    #[test]
    fn gamma_margins() {
        for (d, b) in [(TGAMMA_DELTA, GAMMA_BOUND), (LGAMMA_DELTA, LGAMMA_BOUND), (LGAMMA_SMALL_DELTA, LGAMMA_SMALL_BOUND), (LGAMMA_REFLECT_DELTA, LGAMMA_REFLECT_BOUND)] {
            assert!(d as f64 >= 1.5 * b + 1.0, "margin {d} ulp for the bound {b} ulp");
        }
    }

    /// gamma_core (≤ 55.1u) and lgamma_core (its piece's bound in ulp of y) against the correctly
    /// rounded `double` accurate path (≤ 0.5 ulp more). (No spec clause.)
    #[test]
    fn gamma_cores_are_within_their_bounds() {
        let (mut tg, mut lg) = (0u32, 0u32);
        for x in gamma_sample() {
            let xd = f64::from(x);
            if let Some(y) = gamma_core(xd) {
                tg += 1;
                let f = crate::special::tgamma_accurate::<f64>(xd, Rounding::Nearest).0;
                let ulp = f64::from_bits(y.to_bits() & 0x7ff0_0000_0000_0000) / 4_503_599_627_370_496.0;
                assert!(crate::dd::abs(y - f) <= (GAMMA_BOUND + 0.5) * ulp, "gamma_core({x:e}): {} ulp", (y - f) / ulp);
            }
            if let Some((y, d, edge)) = lgamma_core(x) {
                lg += 1;
                let f = crate::special::lgamma_accurate::<f64>(xd, Rounding::Nearest).0.0;
                let ulp = f64::from_bits(y.to_bits() & 0x7ff0_0000_0000_0000) / 4_503_599_627_370_496.0;
                // lgamma_edge: its margin d with the reference's half ulp (d - 1 > q + 1/2 + 1/2).
                let b = match d {
                    _ if edge => d as f64 - 1.5,
                    LGAMMA_SMALL_DELTA => LGAMMA_SMALL_BOUND,
                    LGAMMA_REFLECT_DELTA => LGAMMA_REFLECT_BOUND,
                    _ => LGAMMA_BOUND,
                };
                assert!(crate::dd::abs(y - f) <= (b + 0.5) * ulp, "lgamma_core({x:e}): {} ulp", (y - f) / ulp);
            }
        }
        assert!(tg > 1500 && lg > 2500, "{tg} {lg}");
    }

    /// tgammaf and lgammaf (value, flags and signgam) against the accurate path on the sample in
    /// every mode. (No spec clause.)
    #[test]
    fn gamma_fast_paths_agree_on_a_sample() {
        assert!(agree_in(&[Fast::Tgamma, Fast::Lgamma], gamma_sample()) > 10_000);
        for m in MODES {
            for x in gamma_sample() {
                if let Some(r) = eval_lgamma(x, m) {
                    assert_eq!((x.to_bits(), r.1), (x.to_bits(), crate::special::lgamma_accurate::<f32>(x, m).1), "signgam");
                }
            }
        }
    }

    /// lgammaf beside the negative zeros of lnΓ (the 1024 encodings around the cbench row's
    /// x = -3.14382577 and around the zero near -2.4570) and for 0 < |x| < 2^-60 takes the fast path
    /// ([`lgamma_edge`]) and agrees with the accurate path (value, flags, signgam) in every mode.
    /// (No spec clause: LIBM-F32-REST-NEXT.)
    #[test]
    fn lgammaf_beside_the_negative_zeros_takes_the_fast_path() {
        let around = |c: f32| (-512i32..512).map(move |k| f32::from_bits(c.to_bits().wrapping_add_signed(k)));
        let tiny = [1.0e-30f32, -1.0e-30, 8.0e-19, -8.6e-19, f32::from_bits(1), f32::from_bits(0x8000_0001), f32::MIN_POSITIVE];
        let (mut edge, mut refused) = (0u32, 0u32);
        for x in around(-3.143_825_8).chain(around(-2.457_024_8)).chain(tiny) {
            edge += u32::from(matches!(lgamma_core(x), Some((_, _, true))));
            for m in MODES {
                let Some(r) = eval_lgamma(x, m) else {
                    refused += 1;
                    continue;
                };
                let want = crate::special::lgamma_accurate::<f32>(x, m);
                assert_eq!((x.to_bits(), m, r.0.0.to_bits(), r.0.1, r.1), (x.to_bits(), m, want.0.0.to_bits(), want.0.1, want.1), "lgammaf edge");
            }
        }
        assert!(edge > 200 && refused <= 4, "lgammaf edge path: {edge} operands taken, {refused} mode calls refused");
    }

    /// x < -42 operands: a stride over the encodings of (-2^23, -42), the operands one ulp from
    /// the integers -43, -100, -1000, -2^22 and -(2^23 - 1), and x = -42.5, -2^23 + 1/2.
    fn reflect_sample() -> impl Iterator<Item = f32> {
        let stride = (0xc228_0001u32..0xcb00_0000).step_by(31_481).map(f32::from_bits).filter(|x| *x != (*x as i64) as f32);
        let near = [-43.0f32, -100.0, -1000.0, -4_194_304.0, -8_388_607.0]
            .into_iter()
            .flat_map(|c| [c.to_bits() - 1, c.to_bits() + 1].map(f32::from_bits));
        stride.chain(near).chain([-42.5, -8_388_607.5])
    }

    /// The reflection below -42 is within [`LGAMMA_REFLECT_BOUND`] of the correctly rounded
    /// `double` accurate path (≤ 0.5 ulp more), with its sign of Γ(x), and refuses the poles, x ≤
    /// -2^23, -∞ and NaN. (No spec clause: LIBM-LGAMMAF-EDGE-2.)
    #[test]
    fn lgamma_reflection_is_within_its_bound() {
        let (mut n, mut worst) = (0u32, 0.0f64);
        for x in reflect_sample() {
            let xd = f64::from(x);
            let got = lgamma_core_signed(x);
            assert!(got.is_some(), "reflection refused {x:e}");
            let Some((y, d, sign, edge)) = got else { continue };
            assert_eq!((d, edge), (LGAMMA_REFLECT_DELTA, false), "{x:e}");
            let ((f, _), fs) = crate::special::lgamma_accurate::<f64>(xd, Rounding::Nearest);
            let ulp = f64::from_bits(y.to_bits() & 0x7ff0_0000_0000_0000) / 4_503_599_627_370_496.0;
            let e = crate::dd::abs(y - f) / ulp;
            assert!(e <= LGAMMA_REFLECT_BOUND + 0.5, "lgamma_reflect({x:e}): {e} ulp");
            assert_eq!((x.to_bits(), sign), (x.to_bits(), fs), "sign of Γ");
            worst = worst.max(e);
            n += 1;
        }
        std::println!("reflection: {n} operands, worst {worst:.3} ulp of y (bound {LGAMMA_REFLECT_BOUND})");
        assert!(n > 4_000, "{n}");
        for x in [-42.0f32, -43.0, -1.0e6, -8_388_608.0, -8_388_609.0, -1.0e30, f32::NEG_INFINITY, f32::NAN] {
            assert!(lgamma_core_signed(x).is_none(), "{x:e} is not a reflection operand");
        }
    }

    /// lgammaf below -42 takes the fast path for every operand of the sample (table entries
    /// included) and agrees with the accurate path (value, flags, signgam) in every mode. (No spec
    /// clause: LIBM-LGAMMAF-EDGE-2.)
    #[test]
    fn lgammaf_below_minus_42_takes_the_fast_path() {
        let xs = reflect_sample().step_by(16).chain(LGT.iter().map(|e| f32::from_bits((e >> 32) as u32)).filter(|x| *x < -42.0));
        let mut n = 0u32;
        for x in xs {
            for m in MODES {
                let got = eval_lgamma(x, m);
                assert!(got.is_some(), "lgammaf({x:e}) refused in {m:?}");
                let Some(r) = got else { continue };
                let want = crate::special::lgamma_accurate::<f32>(x, m);
                assert_eq!((x.to_bits(), m, r.0.0.to_bits(), r.0.1, r.1), (x.to_bits(), m, want.0.0.to_bits(), want.0.1, want.1), "lgammaf below -42");
                n += 1;
            }
        }
        assert!(n > 1_000, "{n}");
    }

    /// The operands of 2^-8 ≤ |y| < 1 that the rounding test refuses (margin [`LGAMMA_SMALL_DELTA`])
    /// in x ∈ [-2.6, -2.46] (beside the zero near -2.4570) take the second try ([`lgamma_edge`]
    /// through [`lgamma_rest`]) and agree with the accurate path (value, flags, signgam) in every
    /// mode; prints the first 64 of them. (No spec clause: LIBM-LGAMMAF-EDGE-2.)
    #[test]
    fn lgammaf_small_refusals_take_the_second_try() {
        let mut found = std::vec::Vec::new();
        for b in (-2.46f32).to_bits()..=(-2.6f32).to_bits() {
            let x = f32::from_bits(b);
            if let Some((y, LGAMMA_SMALL_DELTA, _, false)) = lgamma_core_signed(x) {
                if round(y, LGAMMA_SMALL_DELTA, Rounding::Nearest).is_none() {
                    found.push(x);
                }
            }
        }
        for &x in &found {
            for m in MODES {
                let got = eval_lgamma(x, m);
                assert!(got.is_some(), "lgammaf({x:e}) refused in {m:?}");
                let Some(r) = got else { continue };
                let want = crate::special::lgamma_accurate::<f32>(x, m);
                assert_eq!((x.to_bits(), m, r.0.0.to_bits(), r.0.1, r.1), (x.to_bits(), m, want.0.0.to_bits(), want.0.1, want.1), "second try");
            }
        }
        std::println!("small refusals: {} {:x?}", found.len(), found.iter().take(64).map(|x| x.to_bits()).collect::<std::vec::Vec<_>>());
        assert!(found.len() >= 8, "{} small refusals", found.len());
    }

    /// Every table entry is an operand whose last try the rounding test refuses, in a piece the
    /// table covers, and the fast path answers it as the accurate path does (value, flags,
    /// signgam) in every mode; the table is sorted and not empty. (No spec clause:
    /// LIBM-LGAMMAF-EDGE-2.)
    #[test]
    fn lgammaf_table_entries() {
        assert!(!LGT.is_empty() && LGT.windows(2).all(|w| (w[0] >> 32) < (w[1] >> 32)), "LGT sorted, not empty");
        for &e in LGT.iter() {
            let x = f32::from_bits((e >> 32) as u32);
            assert_eq!(lgammaf_last_try_refused(x), Some(true), "{x:e} is a refused operand");
            for m in MODES {
                let got = eval_lgamma(x, m);
                assert!(got.is_some(), "lgammaf({x:e}) refused in {m:?}");
                let Some(r) = got else { continue };
                let want = crate::special::lgamma_accurate::<f32>(x, m);
                assert_eq!((x.to_bits(), m, r.0.0.to_bits(), r.0.1, r.1), (x.to_bits(), m, want.0.0.to_bits(), want.0.1, want.1), "table entry");
            }
        }
    }

    /// For x the tries before the table make (as [`lgamma_rest`]): `None` when no piece answers
    /// or the piece is not [`lgamma_tabled`], else whether the rounding test refused the last try
    /// (the test does not depend on the mode).
    fn lgammaf_last_try_refused(x: f32) -> Option<bool> {
        let (y, d, _, edge) = lgamma_core_signed(x)?;
        if round(y, d, Rounding::Nearest).is_some() {
            return Some(false);
        }
        if !lgamma_tabled(d, edge) {
            return None;
        }
        if d == LGAMMA_SMALL_DELTA {
            if let Some((y2, d2, _, _)) = lgamma_edge(f64::from(x)) {
                return Some(round(y2, d2, Rounding::Nearest).is_none());
            }
        }
        Some(true)
    }

    /// The table is exactly the set of operands whose last try is refused (an exhaustive
    /// enumeration of every `float`). On a mismatch it prints the set and, with
    /// LIELLA_LGAMMAF_TABLE_OUT=<path>, writes the table file (lgamma_tab.rs) there, |F| and its side
    /// from the accurate path. (No spec clause: LIBM-LGAMMAF-EDGE-2.)
    #[test]
    #[ignore]
    fn lgammaf_table_is_the_refused_set() {
        let found = std::sync::Mutex::new(std::vec::Vec::new());
        exhaustive_by(|xs| {
            for &x in xs {
                if lgammaf_last_try_refused(x) == Some(true) {
                    found.lock().unwrap_or_else(std::sync::PoisonError::into_inner).push(x.to_bits());
                }
            }
            0
        });
        let mut found = found.into_inner().unwrap_or_else(std::sync::PoisonError::into_inner);
        found.sort_unstable();
        let have: std::vec::Vec<u32> = LGT.iter().map(|e| (e >> 32) as u32).collect();
        let ok = found == have;
        let below = found.iter().filter(|&&a| f32::from_bits(a) < -42.0).count();
        std::println!("lgammaf: {} refused ({below} below -42)", found.len());
        if !ok {
            let mut text = std::string::String::from(concat!(
                "// lgammaf's exception table (LIBM-LGAMMAF-EDGE-2), as written by `lgammaf_table_is_the_refused_set`\n",
                "// (the exhaustive enumeration of every `float`; LIELLA_LGAMMAF_TABLE_OUT=<this file>): bits 32..63 x,\n",
                "// 0..30 |F| for F the `float` nearest lnΓ(x), bit 31 set when |lnΓ(x)| lies above |F|; sorted by\n",
                "// the bits of x.\n\n"
            ));
            text += &std::format!("const LGT: [u64; {}] = [\n", found.len());
            for a in found {
                let x = f32::from_bits(a);
                let near = accurate(Fast::Lgamma, x, Rounding::Nearest).0;
                let above = accurate(Fast::Lgamma, x, Rounding::Zero).0 == near;
                text += &std::format!("    {:#018x}, // {x:e}\n", (u64::from(a) << 32) | (u64::from(above) << 31) | u64::from(near.to_bits() & 0x7fff_ffff));
            }
            text += "];\n";
            std::print!("{text}");
            if let Ok(p) = std::env::var("LIELLA_LGAMMAF_TABLE_OUT") {
                let w = std::fs::write(&p, &text);
                assert!(w.is_ok(), "{p}: {w:?}");
            }
        }
        assert!(ok, "lgammaf's table is not the refused set");
    }

    /// The tiered oracle's decided gamma answers equal the accurate path's in every mode on the
    /// sample. (No spec clause: ruling #2889's oracle.)
    #[test]
    fn gamma_oracle_agrees_with_the_accurate_path() {
        use crate::oracle32 as o;
        let mut decided = 0;
        for x in gamma_sample().step_by(3) {
            for (f, est) in [(Fast::Tgamma, o::tgamma_est(x)), (Fast::Lgamma, o::lgamma_est(x))] {
                for m in MODES {
                    if let Some(r) = o::decide(&est, m) {
                        decided += 1;
                        let want = accurate(f, x, m);
                        assert_eq!((f, m, x.to_bits(), r.to_bits(), Status::flags(Flags::INEXACT)), (f, m, x.to_bits(), want.0.to_bits(), want.1));
                    }
                }
            }
        }
        assert!(decided > 5_000, "{decided}");
    }

    /// The tier decides lgammaf at the integers n ≥ 3 and above 2^23 (where the fast path's
    /// Stirling piece runs up to overflow) and refuses its exact zeros and the poles: sending that
    /// range to mp put the exhaustive sweep past its time limit. (No spec clause: ruling #2889.)
    #[test]
    fn lgamma_oracle_decides_integers_and_large_operands() {
        use crate::oracle32 as o;
        for x in [3.0f32, 4.0, 15.0, 16.0, 17.0, 1000.0, 8_388_607.0, 8_388_608.0, 1.0e10, 1.0e30, 1.0e36] {
            for m in MODES {
                let r = o::decide(&o::lgamma_est(x), m);
                assert_eq!(r.map(f32::to_bits), Some(accurate(Fast::Lgamma, x, m).0.to_bits()), "lgamma oracle at {x:e} {m:?}");
            }
        }
        // Below -64 too since LIBM-LGAMMAF-EDGE-2 (the fast path's reflection takes x < -42).
        for x in [-64.5f32, -1000.25, -4_194_303.5, -8_388_607.5] {
            for m in MODES {
                let r = o::decide(&o::lgamma_est(x), m);
                assert_eq!(r.map(f32::to_bits), Some(accurate(Fast::Lgamma, x, m).0.to_bits()), "lgamma oracle at {x:e} {m:?}");
            }
        }
        for x in [1.0f32, 2.0, 0.0, -0.0, -1.0, -3.0, -65.0, -8_388_608.0, -1.0e30, f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
            assert!(matches!(o::lgamma_est(x), o::Est::Unknown), "lgamma oracle refuses {x:e}");
        }
    }

    /// `f` against the tiered oracle on `xs` (as erf's): one oracle call per operand the fast path
    /// takes in any mode; the number taken. Counts in `near_zero` the lgammaf operands refused
    /// only for |y| < 2^-8 (next to the negative zeros; they take the accurate path).
    fn agree_oracle(f: Fast, xs: impl Iterator<Item = f32>, tally: &crate::oracle32::Tally, near_zero: &AtomicU64) -> u64 {
        use crate::oracle32 as o;
        let mut taken = 0;
        for x in xs {
            let got = o::MODES.map(|m| eval(f, x, m));
            if got.iter().all(Option::is_none) {
                if f == Fast::Lgamma && lgamma_core(x).is_none() && gamma_core(f64::from(x)).is_some() && f64::from(x) < STIRLING_X {
                    near_zero.fetch_add(1, Ordering::Relaxed);
                }
                continue;
            }
            let est = if f == Fast::Tgamma { o::tgamma_est(x) } else { o::lgamma_est(x) };
            let want = o::oracle(est, tally, |m| accurate(f, x, m));
            for (i, g) in got.into_iter().enumerate() {
                if let Some(r) = g {
                    taken += 1;
                    let m = o::MODES[i];
                    assert_eq!((f, m, x.to_bits(), r.0.to_bits(), r.1), (f, m, x.to_bits(), want[i].0.to_bits(), want[i].1));
                }
            }
        }
        taken
    }

    /// `f` on every `float` operand in every mode against the tiered oracle; prints the calls
    /// taken, the oracle's mp rate and the near-zero refusals.
    fn exhaustive_oracle(f: Fast, name: &str) {
        let tally = crate::oracle32::Tally::default();
        let near_zero = AtomicU64::new(0);
        let taken = exhaustive_by(|xs| agree_oracle(f, xs.iter().copied(), &tally, &near_zero));
        let nz = near_zero.load(Ordering::Relaxed);
        std::println!("{name} fast path taken {taken} of {} calls; oracle: {}; near-zero refusals {nz} operands", 4u64 << 32, tally.summary());
    }

    /// tgammaf on every `float` operand in every mode, against the tiered oracle (whose mp tier is
    /// the accurate path). (No spec clause: LIBM-F32-GAMMA.)
    #[test]
    #[ignore]
    fn tgammaf_fast_path_agrees_exhaustively() {
        exhaustive_oracle(Fast::Tgamma, "tgammaf");
    }

    /// lgammaf on every `float` operand in every mode (as for tgammaf). (No spec clause.)
    #[test]
    #[ignore]
    fn lgammaf_fast_path_agrees_exhaustively() {
        exhaustive_oracle(Fast::Lgamma, "lgammaf");
    }

    /// Where a gamma sweep spends its time: the oracle check (as the exhaustive sweeps) over the
    /// 512 slices of one sign and exponent each, every `LIELLA_PROFILE_STRIDE`-th encoding
    /// (default 4099; 1 = in full), only the slices `LIELLA_PROFILE_SLICES=lo-hi` (slice = top 9
    /// bits, default all) and only `LIELLA_PROFILE_FN` (`lgammaf`, the default, or `tgammaf`).
    /// Prints per slice: operands checked, fast-path calls taken, oracle mp modes, near-zero
    /// refusals and the slice's wall time (a diagnostic, never asserted). (No spec clause.)
    #[test]
    #[ignore]
    fn gamma_sweep_profile() {
        let env = |k: &str| std::env::var(k).ok();
        let stride = env("LIELLA_PROFILE_STRIDE").and_then(|s| s.parse::<u32>().ok()).filter(|&k| k > 0).unwrap_or(4099);
        let (lo, hi) = env("LIELLA_PROFILE_SLICES")
            .and_then(|s| s.split_once('-').and_then(|(a, b)| Some((a.parse::<u32>().ok()?, b.parse::<u32>().ok()?))))
            .unwrap_or((0, 511));
        let f = if env("LIELLA_PROFILE_FN").as_deref() == Some("tgammaf") { Fast::Tgamma } else { Fast::Lgamma };
        let next = AtomicU64::new(u64::from(lo));
        let n = std::thread::available_parallelism().map_or(8, |n| n.get());
        let mut rows: std::vec::Vec<(u32, u64, u64, u64, u64, f64)> = std::thread::scope(|sc| {
            let hs: std::vec::Vec<_> = (0..n).map(|_| sc.spawn(|| {
                let mut out = std::vec::Vec::new();
                loop {
                    let s = next.fetch_add(1, Ordering::Relaxed) as u32;
                    if s > hi {
                        return out;
                    }
                    let tally = crate::oracle32::Tally::default();
                    let nz = AtomicU64::new(0);
                    let xs = ((s << 23)..=((s << 23) | 0x7f_ffff)).step_by(stride as usize).map(f32::from_bits);
                    let t0 = std::time::Instant::now();
                    let taken = agree_oracle(f, xs, &tally, &nz);
                    let secs = t0.elapsed().as_secs_f64();
                    let ops = (0x80_0000u64).div_ceil(u64::from(stride));
                    out.push((s, ops, taken, tally.mp_modes.load(Ordering::Relaxed), nz.load(Ordering::Relaxed), secs));
                }
            })).collect();
            hs.into_iter().flat_map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e))).collect()
        });
        rows.sort_by_key(|r| r.0);
        let total: f64 = rows.iter().map(|r| r.5).sum();
        std::println!("profile {f:?} stride {stride} slices {lo}-{hi}: {total:.2} s summed over slices");
        std::println!("slice  first-x        operands  taken  mp-modes  near-zero  secs  share");
        for (s, ops, taken, mp, nz, secs) in rows {
            if taken > 0 || mp > 0 || secs > total / 1000.0 {
                let x0 = f32::from_bits(s << 23);
                std::println!("{s:5}  {x0:<13e}  {ops:8}  {taken:6}  {mp:8}  {nz:9}  {secs:.3}  {:.3}", secs / total.max(1e-9));
            }
        }
    }
}
