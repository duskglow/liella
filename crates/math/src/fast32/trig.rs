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
///
/// The range test and the `double` operand come from x's bits (LIBM-F32-ONEPASS-REST, D-10.72): no
/// `cvtss2sd`, whose partial write would wait for the register's previous writer (D-10.67), and a
/// refused subnormal never reaches an FP instruction. The `float` sin, cos and tan exports take
/// [`trig_core_with`] and an in-place `cvtss2sd` from the boundary instead (D-10.79). |x| in [`TRIG_MIN`, `TRIG_MAX`) is a normal
/// `float` with biased exponent 101..=145, so its `double` is the same sign, the exponent rebased by
/// 1023 - 127 = 896 and the 23 fraction bits moved up 29 places: exactly `f64::from(x)`.
#[inline(always)]
pub(super) fn trig_core(x: f32) -> Option<(f64, f64)> {
    trig_core_with(x, widen_bits)
}

/// `f64::from(x)` built from x's bits, for a normal `float` with biased exponent 101..=145
/// ([`trig_core`]'s range): the exponent rebased by 896, the fraction moved up 29 places.
#[inline(always)]
fn widen_bits(x: f32) -> f64 {
    let b = x.to_bits();
    let ab = b & 0x7fff_ffff;
    f64::from_bits(((u64::from(ab) << 29) + (896 << 52)) | (u64::from(b ^ ab) << 32))
}

/// [`trig_core`] with the `double` operand from `widen` (LIBM-SINF-IVB-RATCHET-2, D-10.79): the
/// boundary passes one in-place `cvtss2sd` (`hot::fpregs::widen`; this crate forbids `unsafe`),
/// one instruction against [`widen_bits`]' six, with no false dependency either. `widen` is called
/// only after the range test, on a normal `float` with biased exponent 101..=145, and must return
/// exactly `f64::from(x)` there (the tests pass `f64::from`).
#[inline(always)]
pub(super) fn trig_core_with(x: f32, widen: impl Fn(f32) -> f64) -> Option<(f64, f64)> {
    // NaN, ±∞, |x| ≥ 2^19 and |x| < 2^-26 (the zeros and subnormals included): the accurate path.
    if !in_fast_range(x) {
        return None;
    }
    let (n, r) = reduce_fast(widen(x))?;
    Some(sin_cos_at(n, r))
}

/// [`trig_core_with`] that also takes 2^19 ≤ |x| < ∞ through [`reduce_large`] (LIBM-TINY-FASTPATHS-2,
/// its pending decision record): the sin, cos, tan and sincos entries. [`trig_core`] keeps the fast range alone for its
/// other users (cexpf, the Bessel functions), whose proofs state that range.
#[inline(always)]
pub(super) fn trig_core_wide_with(x: f32, widen: impl Fn(f32) -> f64) -> Option<(f64, f64)> {
    let (n, r) = if in_fast_range(x) { reduce_fast(widen(x))? } else { reduce_large(x.to_bits())? };
    Some(sin_cos_at(n, r))
}

/// [`trig_core_wide_with`] with the operand built from the bits (the envelope's [`super::eval`]).
#[inline(always)]
pub(super) fn trig_core_wide(x: f32) -> Option<(f64, f64)> {
    trig_core_wide_with(x, widen_bits)
}

/// 2^-26 ≤ |x| < 2^19, from the bits: one subtraction and one comparison.
#[inline(always)]
fn in_fast_range(x: f32) -> bool {
    const LO: u32 = 0x3280_0000; // TRIG_MIN = 2^-26 as `float` bits
    const HI: u32 = 0x4900_0000; // TRIG_MAX = 2^19
    const _: () = assert!(f32::from_bits(LO) as f64 == TRIG_MIN && f32::from_bits(HI) as f64 == TRIG_MAX);
    (x.to_bits() & 0x7fff_ffff).wrapping_sub(LO) < HI - LO
}

/// Cody–Waite on the fast range ([`trig_core`]'s proof): (n mod 128, r), or `None` for |r| < 2^-36.
#[inline(always)]
fn reduce_fast(xd: f64) -> Option<(usize, f64)> {
    let k = xd * INV_PI64 + SHIFT;
    let n = (k.to_bits() & 127) as usize; // n mod 128 (the low bits of 2^51 + n)
    let kf = k - SHIFT;
    let r = ((xd - kf * PI64_1) - kf * PI64_2) - kf * PI64_3;
    if r.abs() < TRIG_R_MIN {
        return None;
    }
    Some((n, r))
}

/// (sin, cos)(nπ/64 + r) from the table and the polynomials ([`trig_core`]'s proof), n < 128.
#[inline(always)]
fn sin_cos_at(n: usize, r: f64) -> (f64, f64) {
    let z = r * r;
    let sr = r + r * z * (S3 + z * S5);
    let cm = z * (K2 + z * (K4 + z * K6));
    let s = f64::from_bits(t::SIN_K_PI64[n][0]);
    let c = f64::from_bits(t::SIN_K_PI64[(n + 32) & 127][0]);
    (s + (s * cm + c * sr), c + (c * cm - s * sr))
}

/// The first 256 bits of 2/π after the point, most significant first (the generated
/// `TWO_OVER_PI`'s first words, pinned by `trig_large_table_is_two_over_pi`).
pub(super) const TWO_OVER_PI_256: [u64; 4] =
    [0xa2f9_836e_4e44_1529, 0xfc27_57d1_f534_ddc0, 0xdb62_9599_3c43_9041, 0xfe51_63ab_debb_c561];

/// [`reduce_large`]'s table: entry i = ⌊2^K/π⌋ mod 2^128, K = i + 123, for the biased exponent
/// i + 146 (2^19 ≤ |x| < 2^128). ⌊2^K/π⌋ = ⌊2^(K-1)·(2/π)⌋, whose low 128 bits are the bits
/// K - 129 .. K - 2 of 2/π after the point (bit 0 first; those before the point are 0).
pub(super) const TRIG_LARGE: [u128; 109] = {
    let hi = ((TWO_OVER_PI_256[0] as u128) << 64) | TWO_OVER_PI_256[1] as u128;
    let lo = ((TWO_OVER_PI_256[2] as u128) << 64) | TWO_OVER_PI_256[3] as u128;
    let mut tab = [0u128; 109];
    let mut i = 0;
    while i < 109 {
        tab[i] = if i < 6 { hi >> (6 - i) } else if i == 6 { hi } else { (hi << (i - 6)) | (lo >> (134 - i)) };
        i += 1;
    }
    tab
};

/// RN(π/4·2^63), below 2^63.
const PI4_Q63: u64 = 0x6487_ed51_10b4_611a;

/// Payne–Hanek in fixed point for 2^19 ≤ |x| < ∞ (Payne and Hanek, "Radian reduction for
/// trigonometric functions", SIGNUM Newsletter 18(1), 1983; Muller, *Elementary Functions*, 3rd
/// ed., 2016, ch. 11): `b` is x's bits; (n mod 128, r) with x = n·π/64 + r, |r| ≤ π/128 (1 + 2^-52)
/// and |Δr| ≤ 1.01·2^-53|r|, or `None` (NaN, ±∞, |x| < 2^19, and |r| < 2^-35.3).
/// * x = m·2^(E-150), m the 24-bit significand, E the biased exponent 146..=254, so
///   x·(64/π)·2^121 = m·2^K/π with K = E - 23. Write 2^K/π = H·2^128 + T + δ, T = [`TRIG_LARGE`]
///   entry, 0 ≤ δ < 1: m·H·2^128 is a multiple of 128·2^121, so P = m·T mod 2^128 is
///   x·(64/π) mod 128 in units of 2^-121 less m·δ < 2^24 units (2^-97). A negative x negates P.
/// * n = ⌊P/2^121 + 1/2⌋ mod 128, and Q = P·2^7 mod 2^128 read as signed is f·2^128, f = x·64/π - n
///   in [-1/2, 1/2), its error below 2^31 units of 2^-128. |f| < 2^-31 (|Q| < 2^97) is refused;
///   otherwise the error is below 2^-66|f|.
/// * |Q| shifted left by s (its high word's leading zeros, 0..=30) has 64 significant bits in its
///   high word u (truncation < 2^-63 relative); ⌊u·[`PI4_Q63`]/2^64⌋ (constant 2^-63.6, truncation
///   2^-61 relative) is in [2^61, 2^63), converted once (2^-53), and r = that·2^(-67-s) with f's
///   sign, exactly (a power of two; |r| ≥ 2^-35.3, normal). In all ≤ 1.01·2^-53|r|, inside the
///   2·2^-53|r| of [`trig_core`]'s proof, whose bounds therefore hold unchanged; |r| ≥ 2^-35.3 >
///   2^-36 as there.
///
/// Integer operations and one `cvtsi2sd` (inexact only, where the result is inexact anyway): no
/// conversion of x, so no denormal-operand, and every `double` is normal.
#[inline(always)]
fn reduce_large(b: u32) -> Option<(usize, f64)> {
    let ab = b & 0x7fff_ffff;
    let i = (ab >> 23).wrapping_sub(146);
    if i >= 109 {
        return None;
    }
    let m = u128::from((ab & 0x007f_ffff) | 0x0080_0000);
    let p = m.wrapping_mul(TRIG_LARGE[i as usize]);
    let p = if b >> 31 != 0 { p.wrapping_neg() } else { p };
    let n = (p.wrapping_add(1 << 120) >> 121) as usize;
    let q = (p << 7) as i128;
    let a = q.unsigned_abs();
    let hi = (a >> 64) as u64;
    if hi < 1 << 33 {
        return None;
    }
    let s = hi.leading_zeros();
    let u = ((a << s) >> 64) as u64;
    let prod = ((u128::from(u) * u128::from(PI4_Q63)) >> 64) as i64;
    let scale = f64::from_bits(((956 - u64::from(s)) << 52) | (((q >> 64) as u64) & (1 << 63)));
    Some((n, prod as f64 * scale))
}

/// `sin_nearest` with the `double` operand from `widen` ([`trig_core_with`], D-10.79): the
/// boundary passes the in-place `cvtss2sd`. Where `widen` is exact on [`trig_core`]'s range, the
/// result and flags are those of `sin_nearest`.
#[inline(always)]
pub fn sin_nearest_widen(x: f32, widen: impl Fn(f32) -> f64) -> Option<f32> {
    round(trig_core_wide_with(x, widen)?.0, TRIG_DELTA, Rounding::Nearest)
}

/// [`sin_nearest_widen`] for cos.
#[inline(always)]
pub fn cos_nearest_widen(x: f32, widen: impl Fn(f32) -> f64) -> Option<f32> {
    round(trig_core_wide_with(x, widen)?.1, TRIG_DELTA, Rounding::Nearest)
}

/// [`sin_nearest_widen`] for tan = s/c.
#[inline(always)]
pub fn tan_nearest_widen(x: f32, widen: impl Fn(f32) -> f64) -> Option<f32> {
    let (s, c) = trig_core_wide_with(x, widen)?;
    round(s / c, TAN_DELTA, Rounding::Nearest)
}

/// [`sin_nearest_widen`] and [`cos_nearest_widen`] from one [`trig_core_with`] call (the
/// `sincosf` fast path, LIBM-SINCOS-SHARED, D-10.89): both results, each the one its own function
/// gives, or `None` when either fails its rounding test.
#[inline(always)]
pub fn sincos_nearest_widen(x: f32, widen: impl Fn(f32) -> f64) -> Option<(f32, f32)> {
    let (s, c) = trig_core_wide_with(x, widen)?;
    Some((round(s, TRIG_DELTA, Rounding::Nearest)?, round(c, TRIG_DELTA, Rounding::Nearest)?))
}

/// `sincosf`: one reduction for both results; each one that fails its rounding test is computed
/// by the accurate path, so the pair and the flags are those of `sinf` and `cosf`.
pub fn sincos(x: f32, mode: Rounding) -> ((f32, f32), Status) {
    let Some((ys, yc)) = trig_core_wide(x) else {
        return crate::trig::sincos(x, mode);
    };
    let s = finish(x, ys, TRIG_DELTA, mode, "sinf").unwrap_or_else(|| crate::trig::sin(x, mode));
    let c = finish(x, yc, TRIG_DELTA, mode, "cosf").unwrap_or_else(|| crate::trig::cos(x, mode));
    ((s.0, c.0), s.1.and(c.1))
}
