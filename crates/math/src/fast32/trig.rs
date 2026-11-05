//! The trigonometric kernels (`sin`, `cos`, `tan`, `sincos`) of the `float` fast paths
//! (`super`, moved out of fast32.rs whole: the shared core, constants and rounding test stay there).

use super::*;

/// The trig fast range: 2^-26 ≤ |x| < 2^19 (n below 2^23.4, so the n·PI64_k products are exact).
pub(super) const TRIG_MIN: f64 = 1.490_116_119_384_765_6e-8;
pub(super) const TRIG_MAX: f64 = 524_288.0;
/// Below this |r| (for n ≠ 0) the reduction's absolute error would matter: the accurate path.
pub(super) const TRIG_R_MIN: f64 = 1.455_191_522_836_685_2e-11; // 2^-36
pub(super) const INV_PI64: f64 = f64::from_bits(t::SCALES[2]); // RN(64/π)
pub(super) const PI64_1: f64 = f64::from_bits(0x3fa9_21fb_5400_0000); // π/64 to 27 significant bits
pub(super) const PI64_2: f64 = f64::from_bits(0x3dc1_0b46_1100_0000); // the next 29 bits
pub(super) const PI64_3: f64 = f64::from_bits(0x3bf4_c4c6_628b_80dc); // RN(π/64 - PI64_1 - PI64_2)
/// sin, cos: ε ≤ 24.3·2^-53 (proof at [`trig_core`]), so < [`TRIG_BOUND`] = 24.4 ulp.
pub(super) const TRIG_DELTA: u64 = 40;
/// tan = s/c: both quotients' bounds and the division, ε ≤ 49.7·2^-53, so < [`TAN_BOUND`] = 50 ulp.
pub(super) const TAN_DELTA: u64 = 80;
#[cfg(test)]
pub(super) const TRIG_BOUND: f64 = 24.4;
#[cfg(test)]
pub(super) const TAN_BOUND: f64 = 50.0;
/// The minimax polynomials for sin and cos on [-0.0246, 0.0246] (`crates/math/fits/fast32_sin.fit`
/// and `fast32_cos.fit`, `cargo xtask remez`, D-10.3): sin odd 1..5 with the r coefficient fixed
/// at 1, relative error 2^-49.07 (15.24·2^-53); cos even 0..6 with the constant fixed at 1,
/// relative error 2^-64.59.
pub(super) const S3: f64 = f64::from_bits(crate::coeffs::fast32_sin::FAST32_SIN[1]);
pub(super) const S5: f64 = f64::from_bits(crate::coeffs::fast32_sin::FAST32_SIN[2]);
pub(super) const K2: f64 = f64::from_bits(crate::coeffs::fast32_cos::FAST32_COS[1]);
pub(super) const K4: f64 = f64::from_bits(crate::coeffs::fast32_cos::FAST32_COS[2]);
pub(super) const K6: f64 = f64::from_bits(crate::coeffs::fast32_cos::FAST32_COS[3]);

/// (sin x, cos x) as `double`s, each with |y/f - 1| ≤ 24.3·2^-53, for 2^-26 ≤ |x| < 2^19.
///
/// Reduction (Cody–Waite): x = n·π/64 + r, n = nearest(RN(x·64/π)), so |r| < 0.024544.
/// n·PI64_1 and n·PI64_2 are exact (27 and 29 bits, |n| < 2^24); x - n·PI64_1 is exact (both on
/// the 2^-31 grid, the difference below 2^-5); then two roundings ≤ 2^-53 of their results, the
/// product n·PI64_3 ≤ 2^-94 and the constants' tail ≤ 2^-95.8: |Δr| ≤ 2·2^-53|r| + 2^-93. n = 0
/// gives r = x exactly. |r| < 2^-36 with n ≠ 0 goes to the accurate path.
///
/// sin r = sr = r + r·z(S3 + z·S5), z = r² (|r| ≤ 0.024545 < 0.0246, the fitted range):
/// approximation error ≤ 15.25·2^-53|r| (the fit's 15.24 plus its coefficients' rounding, already
/// in the fit's measured figure); with Δr and the evaluation (the r·z·(...) part is < 10^-4|r|,
/// its roundings below 2^-63|r|; the final sum 2^-53), |Δsr| ≤ (3.01 + 15.25)·2^-53|r| =
/// 18.26·2^-53|r|. cos r - 1 = cm = z(K2 + z(K4 + z·K6)): approximation error ≤ 2^-64.5 < 0.01·2^-53
/// absolute, |cm| ≤ 3.02·10^-4, |Δcm| ≤ 8·2^-53|cm| + 0.01·2^-53. S = sin(nπ/64), C = cos(nπ/64)
/// are the `double` heads of the table (relative error 2^-53; sin 0, sin π, cos ±π/2 are exactly
/// 0 and ±1). sin x = S + (S·cm + C·sr); cos x is the same with (C, -S) for (S, C) (the angle
/// shifted by π/2).
/// * n ≢ 0 (mod 64): the angle is at least π/128 from a multiple of π, so |S| ≤ 2|y| and
///   |C·r| ≤ 1.0001|y|. The errors, in units of 2^-53|y|: S 2, C 1.0001, sr 18.26·1.0001, cm
///   0.005 + 0.02, the two products 1.002, the inner sum 1.02, the outer 1: ≤ 24.3.
/// * n ≡ 0 (mod 64): S = 0, C = ±1, y = ±sr: Δr ≤ (2 + 2^-5)·2^-53|r| (|r| ≥ 2^-36), so
///   ≤ 3.04 + 15.25 = 18.3.
///
/// No operand or intermediate is subnormal (|r| ≥ 2^-36 or r = x ≥ 2^-26, so z^4 ≥ 2^-304), and
/// |y| ≥ 2^-37.
#[inline(always)]
pub(super) fn trig_core(x: f32) -> Option<(f64, f64)> {
    let xd = f64::from(x);
    let ax = xd.abs();
    // NaN, ±∞, |x| ≥ 2^19 and |x| < 2^-26 (the zeros and subnormals included): the accurate path.
    if !(TRIG_MIN..TRIG_MAX).contains(&ax) {
        return None;
    }
    let k = xd * INV_PI64 + SHIFT;
    let n = (k.to_bits() & 127) as usize; // n mod 128 (the low bits of 2^51 + n)
    let kf = k - SHIFT;
    let r = ((xd - kf * PI64_1) - kf * PI64_2) - kf * PI64_3;
    if r.abs() < TRIG_R_MIN {
        return None;
    }
    let z = r * r;
    let sr = r + r * z * (S3 + z * S5);
    let cm = z * (K2 + z * (K4 + z * K6));
    let s = f64::from_bits(t::SIN_K_PI64[n][0]);
    let c = f64::from_bits(t::SIN_K_PI64[(n + 32) & 127][0]);
    Some((s + (s * cm + c * sr), c + (c * cm - s * sr)))
}

/// `sincosf`: one reduction for both results; each one that fails its rounding test is computed
/// by the accurate path, so the pair and the flags are those of `sinf` and `cosf`.
pub fn sincos(x: f32, mode: Rounding) -> ((f32, f32), Status) {
    let Some((ys, yc)) = trig_core(x) else {
        return crate::trig::sincos(x, mode);
    };
    let s = finish(x, ys, TRIG_DELTA, mode, "sinf").unwrap_or_else(|| crate::trig::sin(x, mode));
    let c = finish(x, yc, TRIG_DELTA, mode, "cosf").unwrap_or_else(|| crate::trig::cos(x, mode));
    ((s.0, c.0), s.1.and(c.1))
}
