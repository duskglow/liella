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

/// 2/√π rounded to `double` (0x1.20dd750429b6dp+0).
const TWO_OVER_SQRT_PI: f64 = f64::from_bits(0x3ff2_0dd7_5042_9b6d);

/// Largest `|x|` bits taken by [`erf_tiny`]: 0x716fe1·2^-149 (n = 7434209), the last operand
/// whose result is subnormal in every mode (c·n = 8388606.56 < 2^23 - 1). From n = 0x716fe2
/// (c·n = 8388607.69) the result can round up to the normal 2^-126, which is not tiny after
/// rounding (no underflow), so those operands stay with the accurate path.
pub(super) const ERF_TINY_MAX: u32 = 0x0071_6fe1;

/// erff for `0 < |x| ≤ ERF_TINY_MAX` (LIBM-RETHINK-ERFC, D-10.107), whose subnormal results
/// [`super::round`] refuses. erf x = c·x·(1 - x²/3 + …) with c = 2/√π, the x³ term below 2^-250
/// relative; for x = n·2^-149 (n < 2^23) the result is c·n rounded to an integer, in units of
/// 2^-149. `y` = C·x (C = c to `double`) is within 2^-28.4 units of c·n (C's error 2^-53·n, the
/// product's rounding at most one `double` ulp, 2^-29), while c·n stays 2^-24.4 or more from every
/// half-integer and 2^-23.4 or more from every integer for n < 2^23: the convergents of 2c and c
/// below 2^23 (Khinchin, Continued Fractions, ch. II, best approximations; the decision record
/// lists them). So y rounds like erf x in every mode and is never a `float` itself: underflow and
/// inexact always, no `errno` (the result is never zero).
#[inline(always)]
pub(super) fn erf_tiny(x: f32, mode: Rounding) -> Option<(f32, Status)> {
    if (x.to_bits() & 0x7fff_ffff).wrapping_sub(1) >= ERF_TINY_MAX {
        return None;
    }
    let y = TWO_OVER_SQRT_PI * f64::from(x);
    // Round to nearest in hardware, then step for a directed mode (as `round`).
    let r = y as f32;
    let back = f64::from(r);
    let b = r.to_bits();
    let neg = y < 0.0;
    let bits = match mode {
        Rounding::Up if back < y => if neg { b - 1 } else { b + 1 },
        Rounding::Down if back > y => if neg { b + 1 } else { b - 1 },
        Rounding::Zero if (if neg { back < y } else { back > y }) => b - 1,
        _ => b,
    };
    let f = Flags::INEXACT.union(Flags::UNDERFLOW);
    Some((f32::from_bits(bits), Status::flags(f).hw(f.union(Status::HW_DENORMAL))))
}

/// erfcf for `0 < |x| < 2^-26` (LIBM-RETHINK-ERFC, D-10.107), which [`super::round`] refuses
/// (y lies within its margin of the `float` 1). erfc x = 1 - c·x·(1 - x²/3 + …) with
/// c·|x| < 2^-25: inside (1 - 2^-25, 1) for x > 0 and (1, 1 + 2^-24) for x < 0, open intervals
/// between rounding boundaries (the midpoints 1 - 2^-25 and 1 + 2^-24, the `float` 1). So the
/// result is 1 to nearest, 1 - 2^-24 down from below 1, 1 + 2^-23 up from above 1; inexact.
#[inline(always)]
pub(super) fn erfc_tiny(x: f32, mode: Rounding) -> Option<(f32, Status)> {
    let ax = x.to_bits() & 0x7fff_ffff;
    if ax.wrapping_sub(1) >= 0x3280_0000 - 1 {
        return None;
    }
    let bits = match (mode, x.is_sign_negative()) {
        (Rounding::Up, true) => 0x3f80_0001,
        (Rounding::Down | Rounding::Zero, false) => 0x3f7f_ffff,
        _ => 0x3f80_0000,
    };
    // The kernel ran on x before: denormal-operand from `f64::from` of a subnormal x.
    let hw = if ax < 0x0080_0000 { Flags::INEXACT.union(Status::HW_DENORMAL) } else { Flags::INEXACT };
    Some((f32::from_bits(bits), Status::flags(Flags::INEXACT).hw(hw)))
}

/// erff where [`super::eval`] refused: [`erf_tiny`]'s band, else the accurate path. Out of line
/// (LIBM-RETHINK-ERFC-3): the entry calls it from its refusal arm where it called the accurate
/// path, so a typical operand runs the same code as without the band. Not `#[cold]`: that
/// changed the entry's block layout (erfcf lost a duplicated rounding block on one piece, one
/// jump more per call there); inside `eval`, after the rounding test, the band cost a typical
/// call 5 to 8 instructions more (D-10.107).
#[inline(never)]
pub(super) fn erf_refused(x: f32, mode: Rounding) -> (f32, Status) {
    match erf_tiny(x, mode) {
        Some(r) => r,
        None => crate::special::erf(x, mode),
    }
}

/// erfcf where [`super::eval`] refused: [`erfc_tiny`]'s band, else the accurate path (as
/// [`erf_refused`]).
#[inline(never)]
pub(super) fn erfc_refused(x: f32, mode: Rounding) -> (f32, Status) {
    match erfc_tiny(x, mode) {
        Some(r) => r,
        None => crate::special::erfc(x, mode),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::tests::{accurate, agree_in, exhaustive_oracle_by, MODES};

    /// erff on every `float` operand in every mode, against the tiered oracle (whose mp tier is
    /// the accurate path): what the entry answers before the accurate path, the fast path and,
    /// where it refuses, the tiny operands' direct path. (No spec clause: LIBM-F32-ERF,
    /// LIBM-RETHINK-ERFC.)
    #[test]
    #[ignore]
    fn erff_fast_path_agrees_exhaustively() {
        exhaustive_oracle_by(Fast::Erf, "erff", crate::oracle32::erf_est, |x, m| eval(Fast::Erf, x, m).or_else(|| erf_tiny(x, m)));
    }

    /// erfcf on every `float` operand in every mode (as for erff). (No spec clause.)
    #[test]
    #[ignore]
    fn erfcf_fast_path_agrees_exhaustively() {
        exhaustive_oracle_by(Fast::Erfc, "erfcf", crate::oracle32::erfc_est, |x, m| eval(Fast::Erfc, x, m).or_else(|| erfc_tiny(x, m)));
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
    /// grid over [-10, 10], and each piece edge (1, the erfcx pieces, ERF_SAT, ERFC_MAX, the
    /// direct paths' band ends) ±8 floats on both sides; the tiny operands' dispatch to the direct
    /// paths. (Clause s10_0_f_0's Annex F reference for the special values, which must be refused
    /// or agree; LIBM-F32-ERF, LIBM-RETHINK-ERFC.)
    #[test]
    fn erf_fast_path_agrees_with_the_accurate_path() {
        let fs = [Fast::Erf, Fast::Erfc];
        let stride = (0..=u32::MAX).step_by(262_147).map(f32::from_bits);
        assert!(agree_in(&fs, stride) > 50_000);
        let grid = (0..1u32 << 12).map(|i| -10.0 + 20.0 * (i as f32) / 4096.0);
        assert!(agree_in(&fs, grid) > 30_000);
        let mut edges = std::vec::Vec::new();
        // Also the ends of the direct paths' bands (D-10.107): erf_tiny's (ERF_TINY_MAX + 1, the
        // first operand whose result can round up to the normal 2^-126) and erfc_tiny's 2^-26.
        let tiny_ends = [f32::from_bits(ERF_TINY_MAX + 1), f32::from_bits(0x3280_0000)];
        for e in [1.0f32, 1.5, 2.25, 3.375, 5.0625, 7.593_75, 4.0, 9.1875, 9.25].into_iter().chain(tiny_ends) {
            for d in 0..8u32 {
                for b in [e.to_bits() + d, e.to_bits() - 1 - d] {
                    edges.extend([f32::from_bits(b), -f32::from_bits(b)]);
                }
            }
        }
        assert!(agree_in(&fs, edges.iter().copied()) > 900);
        for x in [0.0f32, -0.0, f32::INFINITY, f32::NEG_INFINITY, f32::NAN] {
            assert!(MODES.into_iter().all(|m| eval(Fast::Erf, x, m).is_none() && eval(Fast::Erfc, x, m).is_none()), "{x:e} refused");
        }
        // Subnormal erff results and erfcf next to 1 (D-10.107): the rounding test refuses them
        // and the entry answers them from its refusal arm, by the direct paths inside their
        // bands, exactly. x = the subnormal edges above (erf_tiny's band end) and the hard
        // operands ±n·2^-149: the least subnormal and, on each side of a half-integer and of an
        // integer, the three n ≤ ERF_TINY_MAX whose c·n (c = 2/√π) lies closest to it (2^-24.4
        // below a half-integer at n = 145635, 2^-23.4 below an integer at 291270: the record's
        // convergents), where an error in C or in a directed step shows first. The direct paths
        // narrow the hardware flags (`Status::hw`) and the accurate path does not, so `hw` shows
        // which path the entry took.
        let hard = [1u32, 145_635, 436_905, 728_175, 291_270, 582_540, 873_810, 6_784_882, 7_076_152, 7_367_422, 6_639_247, 6_930_517, 7_221_787];
        let hard = hard.into_iter().flat_map(|n| [f32::from_bits(n), -f32::from_bits(n)]).map(|x| (x, true));
        for (x, is_hard) in hard.chain(edges.into_iter().filter(|x| x.is_subnormal()).map(|x| (x, false))) {
            for m in MODES {
                let paths = [(Fast::Erf, super::super::erf(x, m), erf_tiny(x, m)), (Fast::Erfc, super::super::erfc(x, m), erfc_tiny(x, m))];
                for (f, got, direct) in paths {
                    let (want, key) = (accurate(f, x, m), (f, m, x.to_bits()));
                    assert_eq!((key, got.0.to_bits(), got.1), (key, want.0.to_bits(), want.1), "the entry agrees with the accurate path");
                    assert!(direct.is_some() || !is_hard, "{key:?}: inside the direct path's band");
                    if let Some(d) = direct {
                        // Past erf_tiny's band end the result is normal and `eval` may answer.
                        assert!(eval(f, x, m).is_none(), "{key:?}: refused by the rounding test");
                        assert_ne!(want.1.hw, d.1.hw, "{key:?}: hw tells the paths apart");
                        assert_eq!((key, got.1.hw), (key, d.1.hw), "{key:?}: answered by the direct path");
                    }
                }
            }
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
