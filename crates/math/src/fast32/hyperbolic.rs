//! The hyperbolic kernels (`sinh`, `cosh`, `tanh`, `asinh`, `acosh`, `atanh`) of the `float` fast paths
//! (`super`, moved out of fast32.rs whole: the shared core, constants and rounding test stay there).

use super::*;

// The hyperbolic family (LIBM-F32-HYPERBOLIC). No new polynomial or table: the kernels reuse
// expm1_core, exp_core, sqrt64 and the log tables, so there is no Remez fit to record. u = 2^-53.
#[cfg(test)]
pub(super) const SINH_BOUND: f64 = 12.2;
#[cfg(test)]
pub(super) const COSH_BOUND: f64 = 233.0;
#[cfg(test)]
pub(super) const TANH_BOUND: f64 = 11.2;
#[cfg(test)]
pub(super) const ASINH_BOUND: f64 = 11.5;
#[cfg(test)]
pub(super) const ACOSH_BOUND: f64 = 12.3;
#[cfg(test)]
pub(super) const ATANH_BOUND: f64 = 9.3;
#[cfg(test)]
pub(super) const LOG1P64_BOUND: f64 = 8.2;
/// Margins (≥ 1.5·bound + 1 ulp, checked by a test).
pub(super) const SINH_DELTA: u64 = 20;
pub(super) const COSH_DELTA: u64 = 512;
pub(super) const TANH_DELTA: u64 = 18;
pub(super) const ASINH_DELTA: u64 = 20;
pub(super) const ACOSH_DELTA: u64 = 20;
pub(super) const ATANH_DELTA: u64 = 16;
/// The small-|x| cut of the family: below 2^-26 the result is x or 1 plus far less than a
/// `float` ulp, which the rounding test refuses in the directed modes; the accurate path takes them.
pub(super) const HYP_MIN: f64 = 1.490_116_119_384_765_6e-8;
/// tanh a for a ≥ TANH_SAT lies in (1 - 2^-25, 1): see [`tanh_core`].
pub(super) const TANH_SAT: f64 = 9.5;

/// ln(1 + w) for a `double` w in [2^-26, 2^130], relative error ≤ 8.2u.
///
/// z = RN(1 + w) ≥ 1, z = 2^e·m, i, L = LOG_INV[i] and LOG_C as in [`log_core`]; ln(1 + w) =
/// e·ln 2 + LOG_C[i] + ln(1 + r) with r = L·(1 + w)/2^e - 1, and the series, e·LN2_HI (exact:
/// e ≤ 130, 44 + 8 bits), e·LN2_LO and the sums as in [`log1p_core`] (3.2u in all) but the
/// series one degree shorter than log1p_core's: ln(1 + r) = r + r²(-1/2 + r·D3 + ... + r^5·D7).
/// Its remainder, r^8/8 - r^9/9 + ..., is below |r|^8/8 for |r| < 2^-7 (|m·L - 1| < 2^-7). r:
/// * e = 0 (z in [1, 1.5), w < 0.5): r = L·w + (L - 1) without rounding z's error in: w = wh + wl
///   with wh its top 28 bits, so wh·L (52 bits) and L - 1 (24 bits, L in [2/3, 1]) are exact. For
///   i = 0, L = 1 and r = wh + wl = w exactly, r in [2^-26, 2^-7), y ≥ r(1 - r/2): the remainder
///   is ≤ 1.004·2^-49/8 = 2.01u relative (case A: 3.2u + 2.01u ≤ 5.3u). For i ≥ 1, w ≥ 2^-8,
///   so wh·L is on the 2^-59 grid, L - 1 on 2^-24, their sum below 2^-7: exact; wl·L rounds by
///   ≤ u·2^-28 and the last sum by u·|r|, |r| < 0.0040 ≤ 1.03|y| (|y| ≥ ln(1 + 2^-8)): 4.3u, and
///   the remainder 0.004^8/8 < 0.02u·|y| (case B: 4.4u).
/// * e ≥ 1 (|y| ≥ ln 1.5 > 0.405): z's rounding moves ln z by ≤ u absolute, ≤ 2.47u relative;
///   RN(m·L) by ≤ u (then the subtraction of 1 is exact, Sterbenz), ln(1 + r) by ≤ 1.004u, 2.48u
///   relative; the remainder ≤ 2^-56/8 absolute, 0.04u relative: 3.2u + 2.47u + 2.48u + 0.04u
///   ≤ 8.2u (case C).
///
/// The overall bound is case C's, so the r^8 term (D8) of log1p_core's series buys nothing here.
///
/// A caller's w with relative error η moves the result by η·w/((1 + w)·ln(1 + w)) ≤ η.
/// Every operand is a normal `double` (w ≥ 2^-26): only inexact is raised.
#[inline(always)]
pub(super) fn log1p64(w: f64) -> f64 {
    let z = 1.0 + w;
    let zb = z.to_bits();
    let e = (zb.wrapping_sub(0x3fe8_0000_0000_0000) as i64 >> 52) as i32;
    let m = f64::from_bits(zb.wrapping_sub((e as i64 as u64) << 52));
    let i = ((m.to_bits() >> 45) & 127) as usize;
    let l = LOG_INV[i];
    let r = if e == 0 {
        let wh = f64::from_bits(w.to_bits() & !0x1ff_ffff);
        (wh * l + (l - 1.0)) + (w - wh) * l
    } else {
        m * l - 1.0
    };
    let s = r * r * (-0.5 + r * (D3 + r * (D4 + r * (D5 + r * (D6 + r * D7)))));
    let ef = f64::from(e);
    (ef * LN2_HI + LOG_C[i]) + (r + (s + ef * LN2_LO))
}

/// |x| as a `double` when HYP_MIN ≤ |x| < `hi` (NaN fails both tests).
#[inline(always)]
pub(super) fn hyp_arg(x: f32, hi: f64) -> Option<f64> {
    let a = crate::dd::abs(f64::from(x));
    if a >= HYP_MIN && a < hi { Some(a) } else { None }
}

/// sinh x, ε ≤ 12.2u, for 2^-26 ≤ |x| < 89: y = (E + E/(E + 1))/2, E = e^a - 1 by
/// [`expm1_core`] (9.1u). E/(E + 1) moves by 9.1u/(1 + E) ≤ 9.1u for E's error, plus the sum
/// E + 1 and the quotient (u each): 11.1u; the sum of the two positive terms adds u (12.1u, with
/// the second-order terms 12.2u); halving is exact (y ≥ 2^-27). The overflow range and the cut's
/// small operands are the accurate path's.
#[inline(always)]
pub(super) fn sinh_core(x: f32) -> Option<f64> {
    let a = hyp_arg(x, 89.0)?;
    let e = expm1_core(a as f32)?; // a is a `float` (exact)
    let y = (e + e / (e + 1.0)) * 0.5;
    Some(if x < 0.0 { -y } else { y })
}

/// cosh x, ε ≤ 2^-45.14 (< 233 ulp), for 2^-26 ≤ |x| < 89.5: y = (E + 1/E)/2, E = e^a by
/// [`exp_core`] (2^-45.16; its proof holds up to 104), 1/E adds u, the positive sum u, halving
/// exact. Results above FLT_MAX are refused by [`round`]: the accurate path's overflow.
#[inline(always)]
pub(super) fn cosh_core(x: f32) -> Option<f64> {
    let a = hyp_arg(x, 89.5)? as f32;
    let e = exp_core(a, -1.0, 89.5, INV_L)?;
    Some((e + 1.0 / e) * 0.5)
}

/// tanh x, ε ≤ 11.2u, for 2^-26 ≤ |x| < 9.5: y = E/(E + 2), E = e^(2a) - 1 by [`expm1_core`]
/// (2a exact, < 19; 9.1u). The quotient moves by 9.1u·2/(E + 2) ≤ 9.1u for E's error, plus the
/// sum and the division (u each): 11.1u, 11.2u with second-order terms.
/// For finite |x| ≥ 9.5: 1 - tanh a = 2/(e^(2a) + 1) < 2^-26.4, so tanh a and y = 1 - 2^-30 lie
/// strictly inside (1 - 2^-25, 1), between two adjacent rounding boundaries (1 - 2^-25 the
/// midpoint, 1 a `float`): they round alike in every direction, inexact (the rounding test
/// accepts y, whose low 28 bits are far from 0 and 2^28). ∞ and NaN go to the accurate path.
#[inline(always)]
pub(super) fn tanh_core(x: f32) -> Option<f64> {
    let a = hyp_arg(x, f64::INFINITY)?;
    let y = if a >= TANH_SAT {
        1.0 - 9.313_225_746_154_785e-10
    } else {
        let e = expm1_core((a + a) as f32)?;
        e / (e + 2.0)
    };
    Some(if x < 0.0 { -y } else { y })
}

/// asinh x, ε ≤ 11.5u, for finite |x| ≥ 2^-26: y = ln(1 + w), w = a + a²/(1 + √(1 + a²)).
/// a² is exact (24-bit a, ≥ 2^-52), 1 + a² rounds (u), [`sqrt64`] adds 2u and halves the rest
/// (2.5u), 1 + √ (3.5u: the sum's u and √'s share ≤ 1), the quotient q (4.5u); q < a so a + q
/// carries ≤ 4.5u/2 + u = 3.25u; [`log1p64`] passes that on (≤ 3.25u) and adds 8.2u.
#[inline(always)]
pub(super) fn asinh_core(x: f32) -> Option<f64> {
    asinh_core_with(x, sqrt64)
}

/// [`asinh_core`] with the square root `sqrt`, which must meet [`sqrt64`]'s bound (≤ 2u) and
/// raise at most inexact on its operands (normal, in [1, 2^257]): a correctly rounded root
/// (`sqrtsd` in the default MXCSR, ≤ 0.5u) does both, so the proof above holds unchanged.
#[inline(always)]
pub(super) fn asinh_core_with(x: f32, sqrt: impl Fn(f64) -> f64) -> Option<f64> {
    // [`hyp_arg`]`(x, ∞)` from the bits (D-10.72, as `trig_core`): HYP_MIN = 2^-26 ≤ |x| < ∞ is a
    // normal `float`, whose `double` is the exponent rebased by 896 and the fraction moved up 29
    // places; no `cvtss2sd` (D-10.67), and a refused subnormal reaches no FP instruction.
    const LO: u32 = 0x3280_0000;
    const _: () = assert!(f32::from_bits(LO) as f64 == HYP_MIN);
    let ab = x.to_bits() & 0x7fff_ffff;
    if ab.wrapping_sub(LO) >= 0x7f80_0000 - LO {
        return None;
    }
    let a = f64::from_bits((u64::from(ab) << 29) + (896 << 52));
    let a2 = a * a;
    let y = log1p64(a + a2 / (1.0 + sqrt(1.0 + a2)));
    Some(if x < 0.0 { -y } else { y })
}

/// acosh x, ε ≤ 12.3u, for finite x > 1: y = ln(1 + w), w = t + √(t·(t + 2)), t = x - 1.
/// t is exact for x < 2^53 and within u above; t + 2 and the product round (u each), [`sqrt64`]
/// halves that and adds 2u (3u), the positive sum adds u: w carries ≤ 4u for x < 2^53 (above, the
/// error passed on is ≤ 5u/ln 2^53 < 0.14u); [`log1p64`] adds 8.2u: 12.2u, 12.3u with slack.
#[inline(always)]
pub(super) fn acosh_core(x: f32) -> Option<f64> {
    acosh_core_with(x, sqrt64)
}

/// [`acosh_core`] with the square root `sqrt` (as for [`asinh_core_with`]; operands in
/// [2^-22, 2^257]).
#[inline(always)]
pub(super) fn acosh_core_with(x: f32, sqrt: impl Fn(f64) -> f64) -> Option<f64> {
    // 1 < x < ∞ on the bits (a negative x has the top bit set): x is then a normal `float`, its
    // `double` built from the bits as in [`asinh_core_with`] (D-10.72; no `cvtss2sd`, D-10.67).
    let b = x.to_bits();
    if b.wrapping_sub(0x3f80_0001) >= 0x7f80_0000 - 0x3f80_0001 {
        return None;
    }
    let x = f64::from_bits((u64::from(b) << 29) + (896 << 52));
    let t = x - 1.0;
    Some(log1p64(t + sqrt(t * (t + 2.0))))
}

/// atanh x, ε ≤ 9.3u, for 2^-26 ≤ |x| < 1: y = ln(1 + w)/2, w = 2a/(1 - a); 1 - a is exact (a on
/// the 2^-49 grid) and the quotient rounds (u); [`log1p64`] adds 8.2u; halving is exact.
#[inline(always)]
pub(super) fn atanh_core(x: f32) -> Option<f64> {
    let a = hyp_arg(x, 1.0)?;
    let y = 0.5 * log1p64((a + a) / (1.0 - a));
    Some(if x < 0.0 { -y } else { y })
}

/// [`asinh_nearest`] with the square root `sqrt` (LIBM-F32-HYPERBOLIC-4): the boundary passes
/// `sqrtsd` (this crate forbids `unsafe` and so cannot issue it; the tests pass `f64::sqrt`).
/// `sqrt` must be correctly rounded to nearest (or at least meet [`sqrt64`]'s 2u bound) and
/// raise at most inexact on a normal positive operand; the result and flags are then those of
/// [`asinh_nearest`] wherever both answer (both are the correctly rounded result, inexact).
#[inline(always)]
pub fn asinh_nearest_sqrt(x: f32, sqrt: impl Fn(f64) -> f64) -> Option<f32> {
    round(asinh_core_with(x, sqrt)?, ASINH_DELTA, Rounding::Nearest)
}

/// [`asinh_nearest_sqrt`] for acosh.
#[inline(always)]
pub fn acosh_nearest_sqrt(x: f32, sqrt: impl Fn(f64) -> f64) -> Option<f32> {
    round(acosh_core_with(x, sqrt)?, ACOSH_DELTA, Rounding::Nearest)
}
