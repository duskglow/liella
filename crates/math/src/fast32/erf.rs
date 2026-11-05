//! erff and erfcf from one `double` kernel each and [`super::round`]'s rounding test
//! (LIBM-F32-ERF; rulings #2760, #2784, #2783, #2889).

#![allow(clippy::as_conversions, clippy::arithmetic_side_effects, clippy::indexing_slicing)] // as in fast32.rs: bit fields, bounded exponents, piece indices below 6

use super::*;

/// The proved bounds of [`erf_core`] and [`erfc_core`] in `double` ulp of y, and their margins
/// (≥ 1.5·bound + 1, checked by a test).
#[cfg(test)]
const ERF_BOUND: f64 = 26.0;
#[cfg(test)]
const ERFC_BOUND: f64 = 160.0;
pub(super) const ERF_DELTA: u64 = 40;
pub(super) const ERFC_DELTA: u64 = 248;
/// erf x = x·P(x²) on [-1, 1] (`crates/math/fits/fast32_erf.fit`; 3.67u after rounding).
const ERF_P: [u64; 11] = crate::coeffs::fast32_erf::FAST32_ERF;
/// erfcx = erfc(x)·e^(x²) on six pieces of [1, 37/4), each a degree-15 polynomial in t = x - c
/// (`crates/math/fits/fast32_erfcx<i>.fit`; ≤ 0.9u after rounding).
const ERFCX: [[u64; 16]; 6] = [
    crate::coeffs::fast32_erfcx0::FAST32_ERFCX0,
    crate::coeffs::fast32_erfcx1::FAST32_ERFCX1,
    crate::coeffs::fast32_erfcx2::FAST32_ERFCX2,
    crate::coeffs::fast32_erfcx3::FAST32_ERFCX3,
    crate::coeffs::fast32_erfcx4::FAST32_ERFCX4,
    crate::coeffs::fast32_erfcx5::FAST32_ERFCX5,
];
/// The pieces' lower ends (after the first, 1) and centres, as in the fit files (each fit's
/// interval is the piece's half-width around its centre; a test checks the centres).
const ERFCX_LO: [f64; 5] = [1.5, 2.25, 3.375, 5.0625, 7.593_75];
const ERFCX_MID: [f64; 6] = [1.25, 1.875, 2.8125, 4.218_75, 6.328_125, 8.421_875];
/// erfc x < 2^-25 for x ≥ ERF_SAT (erfc 4 = 2^-25.95): erf x then lies in (1 - 2^-25, 1), between
/// the midpoint below 1 and 1, an interval with no `float` rounding boundary inside. ERF_ONE =
/// 1 - 2^-30 lies there too, so it rounds as erf x in every direction and is inexact like it
/// (and [`round`] passes it: its low 28 bits are 0x0c00_0000, far from both ends). Likewise
/// erfc(-x) in (2 - 2^-25, 2) beside ERFC_TWO = 2 - 2^-29 (a `float` ulp below 2 is 2^-23).
const ERF_SAT: f64 = 4.0;
const ERF_ONE: f64 = 1.0 - 1.0 / 1_073_741_824.0;
const ERFC_TWO: f64 = 2.0 - 1.0 / 536_870_912.0;
/// erfc x is below 2^-126 from 9.194 on (erfc 9.1875 = 2^-125.8): subnormal results are the
/// accurate path's, refused before the kernel (above this, [`round`] would refuse them after it).
/// The last erfcx piece is fitted up to 37/4.
const ERFC_MAX: f64 = 9.1875;

/// erf v for |v| < 1 as v·P(v²), ε ≤ 7u: the fit 3.67u; s = RN(v²) moves P by under
/// s·|P'/P|·u < 0.4u; Horner in s ∈ [0, 1) with |c_k s^k| falling by over 3 per step from
/// c_0 = 1.128 (each partial sum within 0.36 of c_k) adds ≤ 1.6u; the product u: 6.67u.
#[inline(always)]
fn erf_small(v: f64) -> f64 {
    let s = v * v;
    let mut p = f64::from_bits(ERF_P[10]);
    for k in (0..10).rev() {
        p = p * s + f64::from_bits(ERF_P[k]);
    }
    v * p
}

/// erfc a for 1 ≤ a < 37/4 as e^(-a²)·Q_i(a - c_i), ε ≤ 2^-45.74 (153u): a² is exact (a has 24
/// bits), so z = RN(-a²·INV_L) has |z| < 85.6·184.7 < 2^13.95: INV_L's rounding and z's give
/// 2^-39.05 + 2^-40 in z, 2^-45.99 in e^(-a²) after ln 2/128; with [`exp_core`]'s polynomial
/// (2^-49.5), evaluation (2^-52) and table and scaling (2·2^-53): 2^-45.83. t = a - c_i is
/// exact (a multiple of 2^-23 below 2). Q_i: the fit ≤ 0.9u, Horner (|t| ≤ 1.27, the terms
/// falling by over 2 per step) ≤ 2u, the product u. e^(-a²) ≥ 2^-123.5 and Q > 0.06: all normal.
#[inline(always)]
fn erfc_big(a: f64) -> f64 {
    let i = usize::from(a >= ERFCX_LO[0])
        + usize::from(a >= ERFCX_LO[1])
        + usize::from(a >= ERFCX_LO[2])
        + usize::from(a >= ERFCX_LO[3])
        + usize::from(a >= ERFCX_LO[4]);
    let (q, t) = (&ERFCX[i], a - ERFCX_MID[i]);
    let mut p = f64::from_bits(q[15]);
    for k in (0..15).rev() {
        p = p * t + f64::from_bits(q[k]);
    }
    exp_z(-(a * a) * INV_L) * p
}

/// erf x as a `double`, ≤ ERF_BOUND ulp: [`erf_small`] (7u) below 1; 1 - erfc a up to
/// ERF_SAT (erfc a ≤ 0.158 within 2^-45.74, over a result ≥ 0.84 rounded once: 24.6u); ±ERF_ONE
/// beyond (see ERF_SAT). Infinities and NaN go to the accurate path (exact ±1, NaN).
#[inline(always)]
pub(super) fn erf_core(x: f32) -> Option<f64> {
    let v = f64::from(x);
    let a = crate::dd::abs(v);
    if a < 1.0 {
        return Some(erf_small(v));
    }
    let y = if a < ERF_SAT {
        1.0 - erfc_big(a)
    } else if a < f64::INFINITY {
        ERF_ONE
    } else {
        return None;
    };
    Some(if v < 0.0 { -y } else { y })
}

/// erfc x as a `double`, ≤ ERFC_BOUND ulp: 1 - erf_small(x) for |x| < 1 (erfc x ≥ 0.157, the
/// 7u of erf x ≤ 0.843 become 37.6u, the sum u); erfc_big up to ERFC_MAX (153u, refused beyond:
/// subnormal results); 2 - erfc_big(-x) for x ≤ -1 (12.2u + 0.5u); ERFC_TWO from -ERF_SAT.
#[inline(always)]
pub(super) fn erfc_core(x: f32) -> Option<f64> {
    let v = f64::from(x);
    let a = crate::dd::abs(v);
    if a < 1.0 {
        return Some(1.0 - erf_small(v));
    }
    if v > 0.0 {
        return if a < ERFC_MAX { Some(erfc_big(a)) } else { None };
    }
    if a < ERF_SAT {
        Some(2.0 - erfc_big(a))
    } else if a < f64::INFINITY {
        Some(ERFC_TWO)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tests::{accurate, agree_in, exhaustive_by, MODES};

    /// `f` (erff or erfcf) against the tiered oracle on `xs`: one oracle call per operand the
    /// fast path takes in any mode, the accurate path only for the modes the oracle's
    /// double-double tier leaves undecided (ruling #2889); the number taken.
    fn agree_oracle(f: Fast, xs: impl Iterator<Item = f32>, tally: &crate::oracle32::Tally) -> u64 {
        use crate::oracle32 as o;
        let mut taken = 0;
        for x in xs {
            let got = o::MODES.map(|m| eval(f, x, m));
            if got.iter().all(Option::is_none) {
                continue;
            }
            let est = if f == Fast::Erf { o::erf_est(x) } else { o::erfc_est(x) };
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
    /// taken and how often the oracle needed the mp path.
    fn exhaustive_oracle(f: Fast, name: &str) {
        let tally = crate::oracle32::Tally::default();
        let taken = exhaustive_by(|xs| agree_oracle(f, xs.iter().copied(), &tally));
        std::println!("{name} fast path taken {taken} of {} calls; oracle: {}", 4u64 << 32, tally.summary());
    }

    /// erff on every `float` operand in every mode, against the tiered oracle (whose mp tier is
    /// the accurate path). (No spec clause: LIBM-F32-ERF.)
    #[test]
    #[ignore]
    fn erff_fast_path_agrees_exhaustively() {
        exhaustive_oracle(Fast::Erf, "erff");
    }

    /// erfcf on every `float` operand in every mode (as for erff). (No spec clause.)
    #[test]
    #[ignore]
    fn erfcf_fast_path_agrees_exhaustively() {
        exhaustive_oracle(Fast::Erfc, "erfcf");
    }

    /// The tiered oracle's decided answers equal the accurate path's in every mode: a stride over
    /// every encoding, a grid over [-10, 10] and the oracle's own seams (3, 4) ±8 floats. (No
    /// spec clause: ruling #2889's oracle.)
    #[test]
    fn erf_oracle_agrees_with_the_accurate_path() {
        use crate::oracle32 as o;
        let mut xs: std::vec::Vec<f32> = (0..=u32::MAX).step_by(1_048_583).map(f32::from_bits).collect();
        xs.extend((0..1u32 << 10).map(|i| -10.0 + 20.0 * (i as f32) / 1024.0));
        for e in [3.0f32, 4.0] {
            for d in 0..8u32 {
                xs.extend([e.to_bits() + d, e.to_bits() - 1 - d].map(|b| [f32::from_bits(b), -f32::from_bits(b)]).concat());
            }
        }
        let mut decided = 0;
        for &x in &xs {
            for (f, est) in [(Fast::Erf, o::erf_est(x)), (Fast::Erfc, o::erfc_est(x))] {
                for m in MODES {
                    if let Some(r) = o::decide(&est, m) {
                        decided += 1;
                        let want = accurate(f, x, m);
                        assert_eq!((f, m, x.to_bits(), r.to_bits(), Status::flags(Flags::INEXACT)), (f, m, x.to_bits(), want.0.to_bits(), want.1));
                    }
                }
            }
        }
        assert!(decided > 20_000, "{decided}");
    }

    /// erff and erfcf against the accurate path in every mode: a stride over every encoding, a
    /// grid over [-10, 10], and each piece edge (1, the erfcx pieces, ERF_SAT, ERFC_MAX) ±8
    /// floats on both sides. (Clause s10_0_f_0's Annex F reference for the special values, which
    /// must be refused or agree; LIBM-F32-ERF.)
    #[test]
    fn erf_fast_path_agrees_with_the_accurate_path() {
        let fs = [Fast::Erf, Fast::Erfc];
        let stride = (0..=u32::MAX).step_by(262_147).map(f32::from_bits);
        assert!(agree_in(&fs, stride) > 50_000);
        let grid = (0..1u32 << 12).map(|i| -10.0 + 20.0 * (i as f32) / 4096.0);
        assert!(agree_in(&fs, grid) > 30_000);
        let mut edges = std::vec::Vec::new();
        for e in [1.0f32, 1.5, 2.25, 3.375, 5.0625, 7.593_75, 4.0, 9.1875, 9.25] {
            for d in 0..8u32 {
                for b in [e.to_bits() + d, e.to_bits() - 1 - d] {
                    edges.extend([f32::from_bits(b), -f32::from_bits(b)]);
                }
            }
        }
        assert!(agree_in(&fs, edges.into_iter()) > 900);
        for x in [0.0f32, -0.0, f32::INFINITY, f32::NEG_INFINITY, f32::NAN, f32::from_bits(1)] {
            assert!(MODES.into_iter().all(|m| eval(Fast::Erf, x, m).is_none() && eval(Fast::Erfc, x, m).is_none()), "{x:e} refused");
        }
    }

    /// erf_core and erfc_core within ERF_BOUND and ERFC_BOUND against the correctly rounded
    /// `double` accurate path (≤ half an ulp, so a gap of bound - 1 ulp proves the bound), and the
    /// margins exceed the bounds by half and one ulp. (No spec clause: LIBM-F32-ERF.)
    #[test]
    fn erf_cores_are_within_their_bounds() {
        assert!(ERF_DELTA as f64 >= 1.5 * ERF_BOUND + 1.0 && ERFC_DELTA as f64 >= 1.5 * ERFC_BOUND + 1.0);
        // Each erfcx piece [lo, hi) is centred on its ERFCX_MID (the fits' shift).
        let ends = [1.0, ERFCX_LO[0], ERFCX_LO[1], ERFCX_LO[2], ERFCX_LO[3], ERFCX_LO[4], 37.0 / 4.0];
        for i in 0..6 {
            assert_eq!(ERFCX_MID[i], (ends[i] + ends[i + 1]) / 2.0, "erfcx piece {i} centre");
        }
        let ulp = |y: f64| f64::from_bits(y.to_bits() & 0x7ff0_0000_0000_0000) / 4_503_599_627_370_496.0;
        // Every 40009th `float` in [2^-30, 9.25), both signs; erf and erfc's saturation at ERF_SAT
        // and the refusal from ERFC_MAX are the agreement tests'.
        for b in (0x3080_0000u32..0x4114_0000).step_by(40_009).flat_map(|b| [b, b | 1 << 31]) {
            let x = f32::from_bits(b);
            let v = f64::from(x);
            if v.abs() < ERF_SAT {
                let (y, f) = (erf_core(x).unwrap_or(f64::NAN), crate::special::erf(v, Rounding::Nearest).0);
                assert!((y - f).abs() <= (ERF_BOUND - 1.0) * ulp(y), "erf_core({x:e}): {} ulp", (y - f) / ulp(y));
            }
            if v <= -ERF_SAT || v >= ERFC_MAX {
                continue;
            }
            let (y, f) = (erfc_core(x).unwrap_or(f64::NAN), crate::special::erfc(v, Rounding::Nearest).0);
            assert!((y - f).abs() <= (ERFC_BOUND - 1.0) * ulp(y), "erfc_core({x:e}): {} ulp", (y - f) / ulp(y));
        }
    }
}
