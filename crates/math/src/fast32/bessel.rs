//! j0f, j1f, y0f, y1f from one `double` kernel each with an absolute error bound, turned into
//! a per-call margin for [`super::finish`]'s rounding test (LIBM-F32-BESSEL; D-10.49).

#![allow(clippy::as_conversions, clippy::arithmetic_side_effects, clippy::indexing_slicing)] // as in fast32.rs: bit fields, bounded exponents, piece indices below 11

use super::*;

// The Bessel functions J0, J1, Y0, Y1 (LIBM-F32-BESSEL). Pieces: [0, 2) a fit around 0 (J) or
// the series form Y_n = (2/π) ln(x) J_n + regular part (- 2/(πx) for Y1); [2, 24) eleven pieces
// [c - 1, c + 1), c = 3, 5, ..., 23, each one polynomial in t = x - c; [24, 2^19) Hankel's
// asymptotic form. Every core returns y with an ABSOLUTE bound e (|y - f| ≤ e): near a zero of
// the function a relative bound cannot hold for a polynomial in x, so [`bessel_finish`] turns
// e into the margin e/ulp(y) of this call. Near a zero that margin grows and the rounding test
// refuses: those operands go to the accurate path (no table of zeros is needed for that).

/// The switch from the pieces to Hankel's form (DLMF 10.17.3; the plant target "switch").
const BES_HANKEL: f64 = 24.0;
/// The end of the fast path: [`trig_core`]'s range.
const BES_MAX: f64 = 524_288.0;
/// The start of the fast path (the fits of the regular parts start there).
const BES_MIN: f64 = 9.536_743_164_062_5e-7; // 2^-20
const U53: f64 = 1.110_223_024_625_156_5e-16; // 2^-53
const TWO53: f64 = 9_007_199_254_740_992.0;
const TWO_OVER_PI: f64 = f64::from_bits(0x3fe4_5f30_6dc9_c883); // RN(2/π)
const INV_PI_D: f64 = f64::from_bits(0x3fd4_5f30_6dc9_c883); // RN(1/π)
/// The bounds, in units of 2^-53 (absolute unless stated), each ≥ the bound proved in
/// `bessel_bounds_hold` from the coefficients (Horner: γ_2n·Σ|a_i||t|^i plus the fit's error).
const BES_SMALL_E: f64 = 64.0; // J0 on [0, 2) (|J0| ≤ 1), and J1/x·|x| scaled by |x|
const BES_REG_E: f64 = 64.0; // the regular parts of Y0, Y1 on [2^-20, 2)
/// The pieces of [2, 24) per function (J0, J1, Y0, Y1); the first piece of Y0 and Y1 ([2, 4),
/// next to the logarithmic singularity at 0) takes [`BES_PIECE0_E`] instead: its fit levels at
/// 66 and 399 units (`fast32_y0.rs`, `fast32_y1.rs`), the others at ≤ 2.8.
const BES_PIECE_E: f64 = 128.0;
const BES_PIECE0_E: [f64; 4] = [128.0, 128.0, 192.0, 512.0];
const BES_LOG_E: f64 = 400.0; // (2/π) ln x: log_core 2^-44.79 relative plus two roundings, of |L|
const BES_HANKEL_E: f64 = 20.0; // Hankel's form, of the amplitude (see `hankel_core`)

const J0S: [u64; 9] = crate::coeffs::fast32_j0_small::FAST32_J0_SMALL;
const J1S: [u64; 9] = crate::coeffs::fast32_j1_small::FAST32_J1_SMALL;
const Y0R: [u64; 9] = crate::coeffs::fast32_y0_reg::FAST32_Y0_REG;
const Y1R: [u64; 9] = crate::coeffs::fast32_y1_reg::FAST32_Y1_REG;
/// The pieces of [2, 24): J0, J1 degree 13, Y0, Y1 degree 16 (their first piece converges
/// slowly next to the singularity at 0), one row per center 3, 5, ..., 23.
const J0P: [[u64; 14]; 11] = crate::coeffs::fast32_j0::FAST32_J0;
const J1P: [[u64; 14]; 11] = crate::coeffs::fast32_j1::FAST32_J1;
const Y0P: [[u64; 17]; 11] = crate::coeffs::fast32_y0::FAST32_Y0;
const Y1P: [[u64; 17]; 11] = crate::coeffs::fast32_y1::FAST32_Y1;

/// Hankel's coefficients for order ν: P takes (-1)^m a_2m, Q (-1)^m a_(2m+1), with a_0 = 1 and
/// a_(k+1) = a_k·(4ν² - (2k+1)²)/(8(k+1)) (DLMF 10.17.1), k < 22. Each a_k is within 2k·u of
/// its exact rational (checked in `hankel_coefficients_are_the_series`).
const fn hankel_coeffs(nu: u32) -> [[f64; 11]; 2] {
    let mu = (4 * nu * nu) as f64;
    let mut pq = [[0.0; 11]; 2];
    let mut a = 1.0;
    let mut k = 0;
    while k < 22 {
        pq[k % 2][k / 2] = if (k / 2) % 2 == 0 { a } else { -a };
        let j = (2 * k + 1) as f64;
        a = a * (mu - j * j) / (8.0 * (k + 1) as f64);
        k += 1;
    }
    pq
}
const HANKEL: [[[f64; 11]; 2]; 2] = [hankel_coeffs(0), hankel_coeffs(1)];

#[inline(always)]
fn horner(c: &[u64], t: f64) -> f64 {
    let n = c.len();
    let mut y = f64::from_bits(c[n - 1]);
    let mut i = n - 1;
    while i > 0 {
        i -= 1;
        y = y * t + f64::from_bits(c[i]);
    }
    y
}

#[inline(always)]
fn horner_f(c: &[f64; 11], t: f64) -> f64 {
    let mut y = c[10];
    let mut i = 10;
    while i > 0 {
        i -= 1;
        y = y * t + c[i];
    }
    y
}

/// Hankel's form for x in [24, 2^19): f = √(2/(πx))·(P cos ω ∓ ...), ω = x - νπ/2 - π/4, written
/// with s = sin x, c = cos x: J0 = A·(P(c + s) - Q(s - c)), Y0 = A·(P(s - c) + Q(c + s)), J1 =
/// A·(P(s - c) + Q(s + c)), Y1 = A·(Q(s - c) - P(s + c)), A = √(1/(πx)).
/// P and Q are truncated after a_20 and a_21: for real x and k ≥ ν - 1/2 the remainder is
/// bounded by the first neglected term (DLMF 10.17(iii)), |a_22|/24^22 < 2^-58 and less beyond
/// (`bessel_bounds_hold`).
/// Errors in units u = 2^-53: s, c 9.05 each (trig_core), c ± s ≤ 9.05·√2 + √2 < 14.2 (|c ± s| ≤
/// √2); |P - 1| < 2^-12 with ΔP < 1.1 (the last sum), |Q| < 0.016 with ΔQ < 0.1; the products and
/// the bracket's sum add < 1.42 + 1.42 + 0.03: the bracket ≤ 19.1, the truncation < 0.1. A: 1/x
/// (1), ·1/π (1 + the constant's 1), sqrt64 1.6: ≤ 4.6 relative, and the product 1, so
/// e ≤ (20·A + 6·|y|)·u.
#[inline(always)]
fn hankel_core(f: usize, x: f64) -> Option<(f64, f64)> {
    let (s, c) = trig_core(x as f32)?;
    let r = 1.0 / x;
    let z = r * r;
    let nu = f & 1;
    let p = horner_f(&HANKEL[nu][0], z);
    let q = r * horner_f(&HANKEL[nu][1], z);
    let (sum, dif) = (c + s, s - c);
    let b = match f {
        0 => p * sum - q * dif,
        1 | 2 => p * dif + q * sum,
        _ => q * dif - p * sum,
    };
    let a = sqrt64(INV_PI_D * r);
    let y = a * b;
    Some((y, (BES_HANKEL_E * a + 6.0 * y.abs()) * U53))
}

/// J0 (f = 0), J1 (1), Y0 (2), Y1 (3) of a `float` x as (y, e) with |y - f| ≤ e, or `None`
/// (x outside [2^-20, 2^19) in magnitude, x ≤ 0 for Y, NaN). The bounds are at the constants.
#[inline(always)]
pub(super) fn bessel_core(f: usize, x: f32) -> Option<(f64, f64)> {
    let xd = f64::from(x);
    let ax = xd.abs();
    if !(BES_MIN..BES_MAX).contains(&ax) || (f >= 2 && xd < 0.0) {
        return None;
    }
    // J1 is odd, J0 even.
    let sign = if f == 1 && xd < 0.0 { -1.0 } else { 1.0 };
    let (y, e) = if ax < 2.0 {
        let z = ax * ax;
        match f {
            0 => {
                let y = horner(&J0S, z);
                (y, BES_SMALL_E * U53)
            }
            1 => (ax * horner(&J1S, z), BES_SMALL_E * U53 * ax),
            _ => {
                let l = TWO_OVER_PI * log_core(x)?;
                if f == 2 {
                    let j = horner(&J0S, z);
                    let y = l * j + horner(&Y0R, z);
                    // L's error times |J0| ≤ 1, J0's times |L|, the product's rounding, R0, the sum.
                    (y, ((BES_LOG_E + BES_SMALL_E + 1.0) * l.abs() + BES_REG_E + 2.0 * y.abs()) * U53)
                } else {
                    let j = ax * horner(&J1S, z);
                    let w = TWO_OVER_PI / ax;
                    let y = (l * j + ax * horner(&Y1R, z)) - w;
                    // w's two roundings and the final difference's share of it (|J1| ≤ x/2 ≤ x),
                    // L·J1 as for Y0 with J1's bound scaled by x, R1 (absolute fit error ≤ 1),
                    // the sums.
                    (y, (3.0 * w + (BES_LOG_E + BES_SMALL_E + 1.0) * (l * ax).abs() + BES_REG_E * ax + 1.0 + 2.0 * y.abs()) * U53)
                }
            }
        }
    } else if ax < BES_HANKEL {
        let i = ((ax - 2.0) * 0.5) as usize;
        let t = ax - (3 + 2 * i) as f64; // exact: ax is a `float` ≥ 2 (on the 2^-22 grid), |t| ≤ 1
        let c: &[u64] = match f {
            0 => &J0P[i],
            1 => &J1P[i],
            2 => &Y0P[i],
            _ => &Y1P[i],
        };
        (horner(c, t), if i == 0 { BES_PIECE0_E[f] } else { BES_PIECE_E } * U53)
    } else {
        hankel_core(f, ax)?
    };
    Some((sign * y, e))
}

/// The rounding test for an absolute bound e: margin e/ulp(y) < e·2^53/|y| (ulp(y) > |y|·2^-53),
/// refused at or beyond 2^26 (the test's mask is 2^28; y near a zero lands here).
#[inline(always)]
pub(super) fn bessel_finish(x: f32, (y, e): (f64, f64), mode: Rounding, name: &'static str) -> Option<(f32, Status)> {
    let d = e * TWO53 / y.abs();
    if d.is_nan() || d >= 67_108_864.0 {
        stats::record(name, false);
        return None;
    }
    finish(x, y, d as u64 + 1, mode, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tests::{accurate, agree_in, exhaustive_oracle, MODES};

    /// The Bessel functions (not in the all-encodings stride test: their accurate path costs
    /// ~2.3e5 instructions a call, so their agreement tests sample, and the exhaustive sweeps
    /// use the tiered oracle).
    const BES: [Fast; 4] = [Fast::J0, Fast::J1, Fast::Y0, Fast::Y1];
    const BES_NAMES: [&str; 4] = ["j0f", "j1f", "y0f", "y1f"];

    /// Positive operands for the Bessel tests: a stride over [2^-20, 2^19) and dense runs at the
    /// cuts (2^-20, the piece borders 2, 4, ..., 24 with Hankel's switch, 2^19) and at the first
    /// zeros of J0, J1, Y0, Y1 (where the rounding test must refuse).
    fn bessel_operands(step: usize) -> std::vec::Vec<f32> {
        let mut xs: std::vec::Vec<f32> = (0x3580_0000u32..0x4900_0000).step_by(step).map(f32::from_bits).collect();
        let cuts = [0x3580_0000u32, 0x4900_0000, 0x4000_0000, 0x4080_0000, 0x40c0_0000, 0x4100_0000, 0x4120_0000, 0x4140_0000, 0x4160_0000, 0x4180_0000, 0x4190_0000, 0x41a0_0000, 0x41b0_0000, 0x41c0_0000];
        // The first two zeros of J0, J1, Y0, Y1 (DLMF Table 10.21.1).
        let zeros = [2.404_825_557_695_773f64, 5.520_078_110_286_311, 3.831_705_970_207_512, 7.015_586_669_815_619, 0.893_576_966_279_167_5, 3.957_678_419_314_858, 2.197_141_326_031_017, 5.429_681_040_794_135];
        for b in cuts.into_iter().chain(zeros.map(|z| (z as f32).to_bits())) {
            xs.extend((b - 0x100..b + 0x100).map(f32::from_bits));
        }
        xs
    }

    /// Each Bessel core's y is within its absolute bound e of the function, against the 384-bit
    /// series or Hankel sums of the accurate path rounded to double-double (their error far below
    /// 2^-100). (No spec clause: LIBM-F32-BESSEL, D-10.49.)
    #[test]
    fn bessel_cores_are_within_their_bounds() {
        let (mut seen, mut worst) = (0u32, [0f64; 4]);
        for x in bessel_operands(9973) {
            let want = crate::special::bessel01_dd(x);
            for f in 0..4 {
                if let Some((y, e)) = bessel_core(f, x) {
                    seen += 1;
                    let err = crate::dd::Dd::from(y).sub(want[f]).to_f64().abs();
                    assert!(err <= e, "{}({x:e}) = {y:e}: error {err:e} > bound {e:e}", BES_NAMES[f]);
                    worst[f] = worst[f].max(err / e);
                }
            }
        }
        std::println!("worst error / bound: {worst:?}");
        assert!(seen > 100_000, "{seen}");
    }

    /// The bound constants hold for the coefficients: Horner with a separate multiply and add
    /// rounds 2(n - 1) times, |error| ≤ γ_2(n-1)·Σ|a_i|·r^i (r the largest |t| or z), plus the fit's
    /// error as the Remez tool measured it (the generated files' headers, units of 2^-53).
    /// Hankel's series: the first neglected terms a_22/x^22, a_23/x^23 are below 2^-59 at x = 24
    /// (DLMF 10.17(iii): the remainder is bounded by the first neglected term). (No spec clause:
    /// LIBM-F32-BESSEL, D-10.49.)
    #[test]
    fn bessel_bounds_hold() {
        fn horner_bound(c: &[u64], r: f64) -> f64 {
            let s: f64 = c.iter().rev().fold(0.0, |acc, &a| acc * r + f64::from_bits(a).abs());
            let k = 2.0 * (c.len() - 1) as f64;
            k / (1.0 - k * U53) * s
        }
        // The small pieces on z = x² in [0, 4]: J0 relative (|J0| ≤ 1), J1/x relative to J1/x ≤ 1/2.
        assert!(horner_bound(&J0S, 4.0) + 2.56 + 1.2 <= BES_SMALL_E, "J0S {}", horner_bound(&J0S, 4.0));
        assert!(horner_bound(&J1S, 4.0) + 0.14 + 1.0 <= BES_SMALL_E, "J1S {}", horner_bound(&J1S, 4.0));
        assert!(horner_bound(&Y0R, 4.0) + 1.22 <= BES_REG_E, "Y0R {}", horner_bound(&Y0R, 4.0));
        assert!(horner_bound(&Y1R, 4.0) + 1.0 <= BES_REG_E, "Y1R {}", horner_bound(&Y1R, 4.0));
        // The pieces of [2, 24) on t in [-1, 1]: fit errors ≤ 2.8 (J0, J1), 66.27 and 398.55 on
        // Y0's and Y1's first piece, ≤ 1 on their others.
        for i in 0..11 {
            let fit = |f: usize| match (f, i) {
                (0 | 1, _) => 2.8,
                (2, 0) => 66.27,
                (3, 0) => 398.55,
                _ => 1.0,
            };
            for (f, c) in [(0, &J0P[i][..]), (1, &J1P[i][..]), (2, &Y0P[i][..]), (3, &Y1P[i][..])] {
                let b = if i == 0 { BES_PIECE0_E[f] } else { BES_PIECE_E };
                let h = horner_bound(c, 1.0) + fit(f) + 1.0;
                assert!(h <= b, "{} piece {i}: {h} > {b}", BES_NAMES[f]);
            }
        }
        for (nu, pq) in HANKEL.iter().enumerate() {
            // a_k by the recurrence once more, past the kept terms.
            let mu = (4 * nu * nu) as f64;
            let mut a = [1.0f64; 24];
            for k in 0..23 {
                let j = (2 * k + 1) as f64;
                a[k + 1] = a[k] * (mu - j * j) / (8.0 * (k + 1) as f64);
            }
            for k in 0..22 {
                let want = if (k / 2) % 2 == 0 { a[k] } else { -a[k] };
                assert_eq!(pq[k % 2][k / 2].to_bits(), want.to_bits(), "HANKEL[{nu}] a_{k}");
            }
            for k in [22, 23] {
                assert!(a[k].abs() / 24f64.powi(k as i32) < 2f64.powi(-58), "a_{k}(nu = {nu}) / 24^{k}");
            }
        }
    }

    /// The Bessel fast path against the accurate path in every mode on sampled operands (both
    /// signs; the Y functions refuse x ≤ 0), and its fallback rate on a grid over (0, 64]: the
    /// rounding test refuses only next to a zero or a rounding boundary. (No spec clause:
    /// LIBM-F32-BESSEL, D-10.49.)
    #[test]
    fn bessel_fast_path_agrees_with_the_accurate_path() {
        let xs = bessel_operands(0x1_0001);
        let both = xs.iter().flat_map(|&x| [x, -x]);
        let special = [0.0f32, -0.0, f32::INFINITY, f32::NEG_INFINITY, f32::NAN, 1e-30, -1e-30, 1e6, -1e6];
        assert!(agree_in(&BES, both.chain(special)) > 50_000);
        for (f, n) in BES.into_iter().zip(BES_NAMES) {
            let before = stats::counts(n);
            for i in 1..=1u32 << 16 {
                let _ = eval(f, 64.0 * i as f32 / 65_536.0, Rounding::Nearest);
            }
            let c = stats::counts(n).since(before);
            std::println!("{n}: fallback {c:?}");
            assert!(c.check_fallback(n, 1_000).is_ok(), "{n}: fallback rate {c:?}");
        }
    }

    /// The tiered oracle's estimates (`oracle32::bessel_est`) lie within their absolute bounds of
    /// the 384-bit values rounded to double-double (within 2^-105 relative), and the bounds are
    /// small enough to decide (below 2^-50·max(|y|, 1)): a stride over the fast path's range and
    /// dense runs at its cuts (the series/Hankel seam at 24 among them) and the first zeros. (No
    /// spec clause: ruling #2889's oracle, LIBM-F32-BESSEL.)
    #[test]
    fn bessel_oracle_is_within_its_bounds() {
        use crate::oracle32::{bessel_est, Est};
        let mut worst = [0.0f64; 4];
        for x in bessel_operands(1 << 15) {
            let want = crate::special::bessel01_dd(x);
            for f in 0..4 {
                let Est::Near { v, err } = bessel_est(f, x) else { panic!("{} at {x:e}: no estimate", BES_NAMES[f]) };
                let y = want[f];
                let d = v.sub(y).to_f64().abs();
                assert!(d <= err + 2.47e-32 * y.hi.abs(), "{} at {x:e}: |v - y| = {d:e} > {err:e}", BES_NAMES[f]); // 2^-105
                assert!(err < 8.9e-16 * y.hi.abs().max(1.0), "{} at {x:e}: bound {err:e} too wide", BES_NAMES[f]); // 2^-50
                worst[f] = worst[f].max(d / err);
            }
        }
        std::println!("bessel oracle: worst |v - y| / bound {worst:?}");
    }

    /// `exp_log::log_dd` stays within the oracle's `UL·(|ln| + 1)` with a factor 16 to spare, on
    /// x/2 over the series range [2^-21, 12). (No spec clause: ruling #2889's oracle.)
    #[test]
    fn bessel_oracle_log_bound() {
        use crate::mp::Mp;
        let (lo, hi) = (0x3580_0000u32, 0x41c0_0000u32);
        let mut worst = 0.0f64;
        for i in 0..4096u32 {
            let h = f64::from(f32::from_bits(lo + i * ((hi - lo) / 4096))) * 0.5;
            let got = crate::exp_log::log_dd(crate::dd::Dd::from(h));
            let want = crate::mp::ln(Mp::from_f64(h));
            let d = Mp::from_f64(got.hi).add(Mp::from_f64(got.lo)).sub(want).to_f64().abs();
            worst = worst.max(d / (want.to_f64().abs() + 1.0));
        }
        assert!(worst < crate::oracle32::bessel::UL / 16.0, "log_dd: {worst:e}");
        std::println!("log_dd: worst error / (|ln| + 1) = {worst:e}");
    }

    /// The tiered oracle's decided answers equal the accurate path's in every mode, both signs
    /// (Y: positive operands): a stride over the fast path's range and the cuts and first zeros
    /// ±8 floats. (No spec clause: ruling #2889's oracle, LIBM-F32-BESSEL.)
    #[test]
    fn bessel_oracle_agrees_with_the_accurate_path() {
        use crate::oracle32 as o;
        let mut pos: std::vec::Vec<f32> = (0x3580_0000u32..0x4900_0000).step_by(1 << 17).map(f32::from_bits).collect();
        let marks = [0x3580_0000u32, 0x4000_0000, 0x4080_0000, 0x4100_0000, 0x4180_0000, 0x41c0_0000, 0x48ff_ffff];
        let zeros = [2.404_825_557_695_773f32, 3.831_705_970_207_512, 0.893_576_966_279_167_5, 2.197_141_326_031_017];
        for b in marks.into_iter().chain(zeros.map(f32::to_bits)) {
            pos.extend((b - 8..b + 8).map(f32::from_bits));
        }
        let mut decided = 0u64;
        for &p in &pos {
            for x in [p, -p] {
                for (f, &fast) in BES.iter().enumerate() {
                    let est = o::bessel_est(f, x);
                    for m in MODES {
                        if let Some(r) = o::decide(&est, m) {
                            decided += 1;
                            let want = accurate(fast, x, m);
                            assert_eq!((fast, m, x.to_bits(), r.to_bits(), Status::flags(Flags::INEXACT)), (fast, m, x.to_bits(), want.0.to_bits(), want.1));
                        }
                    }
                }
            }
        }
        assert!(decided > 40_000, "{decided}");
    }

    /// j0f on every `float` operand in every mode against the tiered oracle (its mp tier the
    /// accurate path). (No spec clause: LIBM-F32-BESSEL, D-10.49.)
    #[test]
    #[ignore]
    fn j0f_fast_path_agrees_exhaustively() {
        exhaustive_oracle(Fast::J0, "j0f", |x| crate::oracle32::bessel_est(0, x));
    }

    /// j1f, as [`j0f_fast_path_agrees_exhaustively`]. (No spec clause: LIBM-F32-BESSEL, D-10.49.)
    #[test]
    #[ignore]
    fn j1f_fast_path_agrees_exhaustively() {
        exhaustive_oracle(Fast::J1, "j1f", |x| crate::oracle32::bessel_est(1, x));
    }

    /// y0f, as [`j0f_fast_path_agrees_exhaustively`]. (No spec clause: LIBM-F32-BESSEL, D-10.49.)
    #[test]
    #[ignore]
    fn y0f_fast_path_agrees_exhaustively() {
        exhaustive_oracle(Fast::Y0, "y0f", |x| crate::oracle32::bessel_est(2, x));
    }

    /// y1f, as [`j0f_fast_path_agrees_exhaustively`]. (No spec clause: LIBM-F32-BESSEL, D-10.49.)
    #[test]
    #[ignore]
    fn y1f_fast_path_agrees_exhaustively() {
        exhaustive_oracle(Fast::Y1, "y1f", |x| crate::oracle32::bessel_est(3, x));
    }
}
