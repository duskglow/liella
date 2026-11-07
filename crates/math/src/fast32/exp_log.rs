//! The exponential and logarithm kernels (`exp`, `exp2`, `exp10`, `log`, `log2`, `log10`,
//! `log1p`, `expm1`, `cbrt`, `pow`) of the `float` fast paths
//! (`super`, moved out of fast32.rs whole: the shared core, constants and rounding test stay there).

use super::*;

/// e^x, 2^x or 10^x as a `double` y with |y/f - 1| ≤ 2^-45.16, for x in (`lo`, `hi`) and
/// `c` = 128·log2(base) rounded (128 exactly for exp2).
///
/// Reduction: z = RN(x·c) = k + r with k the nearest integer, |r| ≤ 1/2, and f = 2^(k>>7)·T[k&127]
/// ·2^(r/128) with T = EXP2_TAB. |z| < 128·104/ln 2 < 2^14.24 (exp; exp10 likewise; exp2 is exact),
/// so c's rounding and the product's together put z within 2^-52·2^14.24 = 2^-37.76 of x·128·log2
/// (base): a relative error of ≤ 2^-37.76·ln 2/128 = 2^-45.29 in f. r = z - k is exact (k is an
/// integer within 1/2 of z, on z's grid).
///
/// 2^(r/128) = e^u, u = r·ln 2/128, |u| ≤ 2^-8.53: p = (1 + E1·r) + r²·((E2 + E3·r) + r²·E4)
/// (Estrin: two short chains instead of one long one). Taylor remainder ≤ |u|^5/120·1.003 <
/// 2^-49.5; the evaluation ≤ 2^-52 (the last sum, near 1, and 1 + E1·r round once each; the r²
/// part is < 2^-18 and its errors < 2^-69); the coefficients' roundings < 2^-61. y = RN(S·p) with
/// S = T·2^(k>>7) (an exact exponent add: k>>7 is in [-152, 128], S is a normal `double`): the
/// table 2^-53, the product 2^-53. Total ≤ 2^-45.29 + 2^-49.5 + 2^-52 + 2^-52 < 2^-45.16.
#[inline(always)]
pub(super) fn exp_core(x: f32, lo: f64, hi: f64, c: f64) -> Option<f64> {
    let x = f64::from(x);
    if !(x > lo && x < hi) {
        return None;
    }
    Some(exp_z(x * c))
}

/// 2^(z/128) for |z| < 2^14.24: the reduction and polynomial of [`exp_core`] (whose proof
/// covers every z in that range, the rounding of z itself aside).
#[inline(always)]
pub(super) fn exp_z(z: f64) -> f64 {
    let kd = z + SHIFT;
    let k = kd.to_bits() as i32;
    let r = z - (kd - SHIFT);
    let r2 = r * r;
    let p = (1.0 + E1 * r) + r2 * ((E2 + E3 * r) + r2 * E4);
    let s = EXP2_BITS[(k & 127) as usize].wrapping_add((i64::from(k >> 7) as u64) << 52);
    f64::from_bits(s) * p
}

/// The smallest |x| the one-pass entries take: 2^-26, a normal `float` (below it y is within the
/// margin of 1 and the test would refuse anyway).
const ONEPASS_AMIN: u32 = 0x3280_0000;

/// e^x (`c` = [`INV_L`], `amax` = the bits of 87.33) or 2^x (`c` = 128, `amax` = the bits of 126)
/// rounded in the caller's rounding direction by one evaluation in `double` and one `cvtsd2ss`
/// (LIBM-F32-ONEPASS, D-10.63), or `None` for the caller's envelope.
///
/// It runs in the caller's MXCSR without reading it, so the proof holds in every rounding
/// direction and under FTZ and DAZ:
/// * Operands: 2^-26 ≤ |x| < `amax`, tested on the bits before any floating-point operation, so a
///   NaN, an infinity, a zero or a subnormal (whose conversion DAZ would read as 0 and which
///   raises denormal-operand) never reaches the arithmetic. f = base^x then lies in
///   [2^-125.99, 2^125.99] (87.33 < 126·ln 2): normal, far from overflow and underflow.
/// * Every intermediate is a normal `double` (z = x·c with |z| < 2^13.98, S = T·2^(k>>7) with
///   k>>7 in [-127, 126], p near 1), so the arithmetic raises inexact and nothing else, and FTZ
///   and DAZ change nothing. It raises inexact only for an inexact result: e^x is exact only at
///   x = 0 (refused above) and 2^x at the integers, where z = 128·x, r = 0, p = 1 and y = S are
///   all exact.
/// * Bound in any direction (each operation's error ≤ 2^-52 relative, RN's 2^-53 doubled): z's
///   error ≤ (2^-53 + 2^-52)|z| ≤ 2^-37.43 (0 for exp2), ≤ 2^-44.96 relative in f; k is the integer
///   z + SHIFT rounds to, so |r| < 1 (1/2 in RN) and r's own rounding is ≤ 2^-53 absolute
///   (2^-60.5 in f); |u| = |r|·ln 2/128 ≤ 2^-7.53, Taylor remainder ≤ |u|^5/120·1.01 ≤ 2^-44.54;
///   the evaluation ≤ 2·1.006·2^-52 + 2^-59 ≤ 2^-50.97 (the two sums near 1; every other term is
///   below 2^-15); T 2^-53, the product S·p 2^-52. Total ε ≤ 2.446·2^-45 = 2^-43.71: |y - f| <
///   ε·2^53/(1 - ε) < 627 ulp(y) ([`EXP_ONEPASS_DELTA`] = 1024 ≥ 1.5·627 + 1).
/// * Rounding: when y's low 28 significand bits are more than `delta` from 0 and from 2^28, no
///   `float` and no midpoint lies between y and f (as in [`super::round`]), so `cvtsd2ss` of y in
///   the caller's direction is f correctly rounded in that direction; y is not a `float`, so the
///   conversion raises inexact, and the result is normal: no overflow, no underflow, no errno.
#[inline(always)]
pub(super) fn exp_onepass(x: f32, c: f64, amax: u32) -> Option<f32> {
    if (x.to_bits() & 0x7fff_ffff).wrapping_sub(ONEPASS_AMIN) >= amax - ONEPASS_AMIN {
        return None;
    }
    let y = exp_z(f64::from(x) * c);
    if (y.to_bits().wrapping_add(EXP_ONEPASS_DELTA) & 0x0fff_ffff) <= 2 * EXP_ONEPASS_DELTA {
        return None;
    }
    Some(y as f32)
}

/// ln x as a `double` y with |y/ln x - 1| ≤ 2^-51, for a positive finite `float` x.
///
/// x = 2^e·m with m in [0.75, 1.5) (as a `double` every `float`, subnormals included, is normal).
/// i = the top 7 fraction bits of m; c_i the centre of its interval (m in [1, 1.5): width 1/128;
/// m in [0.75, 1): width 1/256), except c = 1 for i = 0 (m in [1, 1 + 2^-7)) and i = 127
/// (m in [1 - 2^-8, 1)). LOG_INV[i] has 24 bits and m at most 24, so m·LOG_INV[i] is exact and
/// r = m·LOG_INV[i] - 1 is exact (Sterbenz), |r| < 2^-7 (i = 0), < 0.0040 otherwise.
/// ln x = e·ln 2 + LOG_C[i] + ln(1 + r), ln(1 + r) = r + s, s = r²(-1/2 + r/3) + r⁴(-1/4 + r/5 -
/// r²/6) (Estrin). y = ((e·LN2 + LOG_C[i]) + r) + s.
/// * Case A (e = 0, c = 1): the first two sums are exact (0 + 0 + r), y = RN(r + s): the Taylor
///   remainder |r|^7/7 ≤ 2^-44.8|r| (|r| < 2^-7), the evaluation of s ≤ 2^-51·r² and the last rounding
///   2^-53, over |y| ≥ 0.996|r|: ε ≤ 2^-44.79.
/// * Case B1 (e ≠ 0, |y| ≥ ln 2 - ln 1.5 = 0.28, |e·ln 2| ≤ 2.5|y|, |LOG_C| ≤ 1.5|y|): e·LN2 ≤
///   2^-52|e·ln 2|, LOG_C 2^-53|LOG_C|, three sums 2^-53·1.01|y| each, remainder |r|^7/7 < 2^-51.8 <
///   2^-50|y|: ε ≤ 17.5·2^-53 < 2^-48.8.
/// * Case B2 (e = 0, c ≠ 1, |r| ≤ 2^-8, |y| ≥ ln(1 + 2^-8), |LOG_C| ≤ 1.5|y|): LOG_C, two sums,
///   remainder 2^-58.8 < 2^-50.8|y|: ε < 2^-50.
#[inline(always)]
pub(super) fn log_core(x: f32) -> Option<f64> {
    log_core_from(x, 1)
}

/// [`log_core`] for `float`s whose bits are at least `lo` (1: every positive finite one;
/// 0x0080_0000: the normal ones, whose conversion to `double` raises nothing).
#[inline(always)]
pub(super) fn log_core_from(x: f32, lo: u32) -> Option<f64> {
    if x.to_bits().wrapping_sub(lo) >= 0x7f80_0000 - lo {
        return None; // ±0, negative, ∞, NaN: the accurate path's special cases
    }
    let xb = f64::from(x).to_bits();
    let e = (xb.wrapping_sub(0x3fe8_0000_0000_0000) as i64 >> 52) as i32;
    let m = f64::from_bits(xb.wrapping_sub((e as i64 as u64) << 52));
    let i = ((m.to_bits() >> 45) & 127) as usize;
    let r = m * LOG_INV[i] - 1.0;
    let r2 = r * r;
    let s = r2 * (-0.5 + r * D3) + (r2 * r2) * ((D4 + r * D5) + r2 * D6);
    let ef = f64::from(e);
    Some(((ef * LN2 + LOG_C[i]) + r) + s)
}

/// ln(1 + x) as a `double` y with |y/ln(1 + x) - 1| ≤ 6.8·2^-53, for a finite `float` x > -1
/// with |x| ≥ 2^-26 (below that the result is x - x²/2 to within the rounding test's margin, and
/// the test would refuse almost every operand; the accurate path's `near` case takes them).
///
/// z = RN(1 + x) is exact unless x ≥ 2^51: for x in (-1, -1/2] by Sterbenz; otherwise x's last
/// bit is ≥ 2^-49 and its top below 2^52, so 1 + x spans at most 53 bits. z = 2^e·m, i, c and
/// LOG_INV/LOG_C as in [`log_core`]; ln(1 + r) = r + r²(-1/2 + r/3 - ... - r^6/8) (Horner, D3..D8)
/// and e·ln 2 = e·LN2_HI (exact, |e| ≤ 128, 44 bits) + e·LN2_LO, whose error terms (the series
/// remainder, LOG_C's and the sums' roundings, 3.2·2^-53 in all) are the base below, except r:
/// * e = 0 (z in [0.75, 1.5), so z is exact and x = z - 1): r = z·L - 1 = x·L + (L - 1) with
///   L = LOG_INV[i]; x·L is exact (24 by 24 bits), L - 1 is exact (L in [2/3, 4/3], 24 bits), so r
///   is RN of the exact value: 2^-53|r|. Case A (c = 1, L = 1): r = x exactly, as in log_core.
///   Case B (c ≠ 1, |y| ≥ ln(1 + 2^-8) and |r| < 0.0040): r's rounding adds ≤ 1.03·2^-53|y|,
///   4.3·2^-53 in total.
/// * e ≠ 0 (|y| ≥ ln(4/3) > 0.287): m has up to 53 bits, so m·L is rounded once (≤ 2^-53
///   absolute, |m·L - 1| < 2^-7) and the subtraction of 1 is exact (Sterbenz): ln(1 + r) moves by
///   ≤ 1.01·2^-53, ≤ 3.6·2^-53|y|; z's own rounding for x ≥ 2^51 is ≤ 2^-53 absolute against
///   |y| > 35 (≤ 2^-58|y|). With the base 3.2·2^-53: ≤ 6.8·2^-53.
pub(super) fn log1p_core(x: f32) -> Option<f64> {
    let xd = f64::from(x);
    // NaN, x ≤ -1, ∞ and |x| < 2^-26 (the zeros and subnormals included) go to the accurate path.
    if !(xd > -1.0 && xd <= f64::from(f32::MAX)) || xd.to_bits() & !(1 << 63) < 0x3e50_0000_0000_0000 {
        return None;
    }
    let z = 1.0 + xd;
    let zb = z.to_bits();
    let e = (zb.wrapping_sub(0x3fe8_0000_0000_0000) as i64 >> 52) as i32;
    let m = f64::from_bits(zb.wrapping_sub((e as i64 as u64) << 52));
    let i = ((m.to_bits() >> 45) & 127) as usize;
    let l = LOG_INV[i];
    let r = if e == 0 { xd * l + (l - 1.0) } else { m * l - 1.0 };
    let s = r * r * (-0.5 + r * (D3 + r * (D4 + r * (D5 + r * (D6 + r * (D7 + r * D8))))));
    let ef = f64::from(e);
    Some((ef * LN2_HI + LOG_C[i]) + (r + (s + ef * LN2_LO)))
}

/// e^x - 1 as a `double` y with |y/f - 1| ≤ 9.1·2^-53, for a `float` x in (-18, 89) with
/// |x| ≥ 2^-45 (below that the rounding test refuses every operand: y is within 12 ulp of x).
///
/// Reduction as for e^x in [`exp_core`]: x = k·ln2/128 + u, |u| ≤ 0.00271 with absolute error
/// ≤ 2^-61.4 (|k| < 2^15), and for k = 0, u = x exactly. E = 2^m·2^(j/128) (m = k >> 7, j = k & 127)
/// = 2^m·(T + T_lo) with T = EXP2_TAB[j], T_lo = EXP2_LO[j] (its rounding ≤ 2^-106·T); e^u = 1 + q,
/// q = u·h, h = 1 + u(1/2 + u(1/6 + u(1/24 + u/120))). f = (E - 1) + E·q is evaluated as
/// y = RN(2^m·T - 1) + 2^m·(T_lo + T·q), 2^m exact (m in [-26, 128]). With δ = 2^-53:
/// * k = 0: y = RN(u·h), T = 1, T_lo = 0, the rest exact: h's error ≤ 1.002δ, the product δ, the
///   Taylor remainder u^5/720 ≤ 0.9δ relative: ≤ 2.91δ.
/// * k ≠ 0: |x| ≥ ln2/256, so |u| ≤ |k|·ln2/256 and |E - 1| ≤ 2.003|f|, |E·q| ≤ 1.003|f|,
///   E·|u| ≤ 1.01|f| (the worst case is k = ±1). Terms: RN(2^m·T - 1) ≤ 2.003δ; u's error, scaled by
///   E/|f| ≤ 372: ≤ 1.07δ; h, u·h, T·q (1.002 + 1 + 1)δ·1.003; the sum T_lo + T·q 1.003δ; the
///   Taylor remainder 0.9δ·1.01; T_lo's rounding ≤ 2^-96; the last sum δ. Total ≤ 9.0δ, so 9.1δ.
///
/// Every intermediate is a normal `double` or zero (Horner keeps u's powers out), so only inexact
/// is raised in hardware.
pub(super) fn expm1_core(x: f32) -> Option<f64> {
    let x = f64::from(x);
    // NaN, x ≤ -18, x ≥ 89 and |x| < 2^-45 (the zeros and subnormals included) go to the accurate path.
    if !(x > -18.0 && x < 89.0) || x.to_bits() & !(1 << 63) < 0x3d20_0000_0000_0000 {
        return None;
    }
    let kd = x * INV_L + SHIFT;
    let k = kd.to_bits() as i32;
    let kd = kd - SHIFT;
    let u = (x - kd * L_HI) - kd * L_LO;
    let j = (k & 127) as usize;
    let t = EXP2_TAB[j];
    let p = f64::from_bits((((k >> 7) + 1023) as u64) << 52);
    let h = 1.0 + u * (0.5 + u * (C3 + u * (C4 + u * C5)));
    let hi = t * p - 1.0;
    Some(hi + p * (EXP2_LO[j] + t * (u * h)))
}

/// ∛x as a `double` y with |y/∛x - 1| ≤ 5.4·2^-53, for a finite nonzero `float` x.
///
/// |x| = 2^e·m with e, m, i, r as in [`log_core`] (r = m·LOG_INV[i] - 1 exact, |r| < 2^-7; the
/// `double` of a subnormal `float` is normal). e = 3q + j, j in {0, 1, 2}. Then
/// ∛|x| = 2^q·CBRT_S[j]·CBRT_T[i]·(1 + r)^(1/3) exactly with the unrounded table values, since
/// m = (1 + r)/LOG_INV[i]. (1 + r)^(1/3) = 1 + r·g, g the binomial series to degree 6: the
/// remainder ≤ |C(1/3, 7)|·|r|^7·1.01 ≤ 0.019·2^-49 < 0.31δ (δ = 2^-53). g's coefficient and
/// Horner roundings are ≤ 2.2δ|g| and scaled by |r·g| < 2^-8.5: ≤ 0.01δ; the sum 1 + r·g δ. The
/// two table entries δ each (CBRT_S[0] = 1 exact), the products S·T and ·p δ each, 2^q exact.
/// Total ≤ 5.33δ, so 5.4δ. Every operation is on normal `double`s: only inexact in hardware.
pub(super) fn cbrt_core(x: f32) -> Option<f64> {
    let a = x.to_bits() & 0x7fff_ffff;
    if a.wrapping_sub(1) >= 0x7f7f_ffff {
        return None; // ±0, ±∞, NaN: the accurate path's special cases
    }
    let xb = f64::from(f32::from_bits(a)).to_bits();
    let e = (xb.wrapping_sub(0x3fe8_0000_0000_0000) as i64 >> 52) as i32;
    let m = f64::from_bits(xb.wrapping_sub((e as i64 as u64) << 52));
    let i = ((m.to_bits() >> 45) & 127) as usize;
    let r = m * LOG_INV[i] - 1.0;
    let p = 1.0 + r * (B1 + r * (B2 + r * (B3 + r * (B4 + r * (B5 + r * B6)))));
    // e in [-149, 128]: e + 150 = 3(q + 50) + j with a nonnegative dividend.
    let n = (e + 150) as u32;
    let (q, j) = ((n / 3) as i32 - 50, (n % 3) as usize);
    let y = CBRT_S[j] * CBRT_T[i] * p * f64::from_bits(((q + 1023) as u64) << 52);
    Some(if x < 0.0 { -y } else { y })
}

/// |x|^y as a `double` with |result/|x|^y - 1| ≤ 89·2^-53 (MATH-M6-F32-POWF), for a finite
/// nonzero `float` x, a finite nonzero `float` y and t = y·ln|x| in (-104, 89); the sign is the
/// caller's. No new tables: ln|x| comes from LOG_INV/LOG_C, e^t from EXP2_TAB. Its kernels are
/// its own (log1p's series D3..D8, expm1's L_HI/L_LO and C3..C5), not [`log_core`]'s and
/// [`exp_core`]'s shorter ones (MATH-M6-F32-KERNEL), whose 2^-45 bounds it does not inherit.
///
/// 1. ln|x| = H + lo, with e, i, r the reduction of [`log_core`] (r exact) and
///    s = r²·(-1/2 + r·(D3 + … + r·D8)), the series of [`log1p_core`]. a = RN(e·LN2_HI + LOG_C[i])
///    and H = RN(a + r) carry their rounding errors exactly (Fast2Sum: e·LN2_HI is exact and
///    either 0 or ≥ 0.69 > 0.41 ≥ |LOG_C| in magnitude; a is 0 or |a| ≥ 0.0058 > 0.0040 ≥ |r|).
///    lo = h_lo + (a_lo + (s + e·LN2_LO)). Errors relative to |ln x|: LOG_C's rounding
///    ≤ 2^-54|LOG_C| ≤ 2^-54·1.51|ln x| (|LOG_C| ≤ 1.51|ln x| in every case of the reduction); the
///    Taylor remainder and the evaluation of s ≤ 2^-58.6|r| ≤ 2^-58.6·1.004|ln x|; the sums in lo
///    (|lo| ≤ 2^-7.9|H|) and LN2_LO's products below 2^-60. In all δ_L ≤ 0.81·2^-53.
/// 2. t = y·(H + lo): H1 = H to 24 significant bits, H2 = H - H1 (exact); P = y·H1 is exact
///    (24 by 24 bits), Q = RN(y·RN(H2 + lo)) with |H2 + lo| ≤ 2^-7.8|H|, so its two roundings
///    add ≤ 2^-60|t|. t_hi = RN(P + Q), t_lo = (P - t_hi) + Q exactly (Fast2Sum, |Q| < |P|).
///    So |Δt| ≤ 0.83·2^-53|t| < 86.5·2^-53 for |t| < 104.
/// 3. e^t: k = round(128·t_hi/ln 2), |k| ≤ 19207 < 2^15 for |t| < 104. D = t_hi - k·L_HI is
///    exact (L_HI has 38 significant bits, so k·L_HI is exact and on the 2^-45 grid; when k ≠ 0,
///    |t_hi| > 2^-9 so t_hi is on the 2^-61 grid; |D| < 2^-8). u = D + (t_lo - k·L_LO): L_HI + L_LO
///    is within 2^-99.9 of ln 2/128 (≤ 2^-84.9 after ·k), |k·L_LO| < 2^-32 and |t_lo| ≤ 2^-46 round
///    below 2^-84, and the last sum's rounding ≤ 2^-53·2^-8.5: ≤ 2^-61 absolute added in all.
///    e^u = 1 + u·(1 + u·(1/2 + u·(C3 + u·(C4 + u·C5)))): Taylor remainder |u|^6/720·1.003 < 2^-60.7
///    (|u| < 0.00272), the coefficients' and Horner's roundings scaled by |u| ≤ 2^-8.5 (< 2^-60
///    in all); v = T + T·(u·h): T's rounding 2^-53, the last sum's 2^-53, the product T·(u·h)'s
///    ≤ 2^-53·0.0028; the scaling by 2^(k>>7) is exact. ≤ 1.02·2^-52 relative.
///
/// Total: e^|Δt| - 1 + 1.02·2^-52 + 2^-61 ≤ (0.84|t| + 2.06)·2^-53 < 89·2^-53 (|t| < 104).
pub(super) fn pow_core(ax: f64, yd: f64) -> Option<f64> {
    let xb = ax.to_bits();
    let e = (xb.wrapping_sub(0x3fe8_0000_0000_0000) as i64 >> 52) as i32;
    let m = f64::from_bits(xb.wrapping_sub((e as i64 as u64) << 52));
    let i = ((m.to_bits() >> 45) & 127) as usize;
    let r = m * LOG_INV[i] - 1.0;
    let s = r * r * (-0.5 + r * (D3 + r * (D4 + r * (D5 + r * (D6 + r * (D7 + r * D8))))));
    let ef = f64::from(e);
    let a0 = ef * LN2_HI;
    let a = a0 + LOG_C[i];
    let a_lo = (a0 - a) + LOG_C[i];
    let h = a + r;
    let h_lo = (a - h) + r;
    let lo = h_lo + (a_lo + (s + ef * LN2_LO));
    let h1 = f64::from_bits(h.to_bits() & 0xffff_ffff_e000_0000);
    let p = yd * h1;
    let q = yd * ((h - h1) + lo);
    let t = p + q;
    if !(t > -104.0 && t < 89.0) {
        return None; // overflow, underflow to 0 and NaN-free bounds: the accurate path
    }
    let t_lo = (p - t) + q;
    let kd = t * INV_L + SHIFT;
    let k = kd.to_bits() as i32;
    let kd = kd - SHIFT;
    let u = (t - kd * L_HI) + (t_lo - kd * L_LO);
    let tt = EXP2_TAB[(k & 127) as usize];
    let hh = 1.0 + u * (0.5 + u * (C3 + u * (C4 + u * C5)));
    let v = tt + tt * (u * hh);
    // k >> 7 is in [-151, 128]: 2^(k>>7) is a normal `double`.
    Some(v * f64::from_bits((((k >> 7) + 1023) as u64) << 52))
}

/// `powf`'s fast path: x, y finite and nonzero, x > 0 or y an integer (the sign from its parity).
/// Every other operand (the C Annex F special cases, ±0, ±∞, NaN, x < 0 with y not an integer)
/// and every result the rounding test refuses (the exact powers among them) go to [`crate::pow`].
#[inline(always)]
pub fn eval_pow(x: f32, y: f32, mode: Rounding) -> Option<(f32, Status)> {
    let (xb, yb) = (x.to_bits(), y.to_bits());
    let (xa, ya) = (xb & 0x7fff_ffff, yb & 0x7fff_ffff);
    if xa.wrapping_sub(1) >= 0x7f7f_ffff || ya.wrapping_sub(1) >= 0x7f7f_ffff {
        return None; // ±0, ±∞, NaN
    }
    let mut neg = false;
    if xb >> 31 != 0 {
        // y must be an integer: |y| ≥ 2^23, or no fraction bits below its units bit.
        let ey = (ya >> 23) as i32 - 127;
        if ey < 0 {
            return None;
        }
        if ey < 23 {
            let sig = (ya & 0x7f_ffff) | 0x80_0000;
            let sh = 23 - ey as u32;
            if sig & ((1 << sh) - 1) != 0 {
                return None;
            }
            neg = (sig >> sh) & 1 == 1;
        }
    }
    let y0 = pow_core(f64::from(f32::from_bits(xa)), f64::from(y))?;
    let r = round(if neg { -y0 } else { y0 }, POW_DELTA, mode);
    stats::record("powf", r.is_some());
    let v = r?;
    // round's range is the normal `float`s and it refuses exact results: inexact only, as in
    // [`finish`]; denormal-operand from `f64::from` of a subnormal x or y.
    let denormal = x.is_subnormal() || y.is_subnormal();
    let hw = if denormal { Flags::INEXACT.union(Status::HW_DENORMAL) } else { Flags::INEXACT };
    Some((v, Status::flags(Flags::INEXACT).hw(hw)))
}

/// `powf`: the fast path, else the accurate path.
pub fn pow(x: f32, y: f32, mode: Rounding) -> (f32, Status) {
    match eval_pow(x, y, mode) {
        Some(r) => r,
        None => crate::pow::pow(x, y, mode),
    }
}
