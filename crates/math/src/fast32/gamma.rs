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
///   for the rounding test: refused, as are the poles and |x| < 2^-20.
#[inline(always)]
pub(super) fn lgamma_core(x: f32) -> Option<(f64, u64)> {
    let xd = f64::from(x);
    let z = xd - 1.0;
    if crate::dd::abs(z) <= 0.125 {
        return Some((z * horner(&LG, z), LGAMMA_DELTA));
    }
    let z = xd - 2.0;
    if crate::dd::abs(z) <= 0.125 {
        return Some((log1p_core(z as f32)? + z * horner(&LG, z), LGAMMA_DELTA));
    }
    if xd >= STIRLING_X {
        let lx = log_core(x)?;
        let w = 1.0 / xd;
        let w2 = w * w;
        let s = w * (1.0 / 12.0 + w2 * (-1.0 / 360.0 + w2 * (1.0 / 1260.0)));
        return Some(((xd - 0.5) * (lx - 1.0) + (HALF_LN_2PI_M_HALF + s), LGAMMA_DELTA));
    }
    let y = ln64(crate::dd::abs(gamma_core(xd)?));
    let a = crate::dd::abs(y);
    if a >= 1.0 {
        Some((y, LGAMMA_DELTA))
    } else if a >= 1.0 / 256.0 {
        Some((y, LGAMMA_SMALL_DELTA))
    } else {
        None
    }
}

/// `lgammaf` by the fast path with the sign of Γ(x) (`signgam`), or `None` (see [`eval`]). Γ(x) < 0
/// exactly for x in (-1, 0), (-3, -2), ...: x < 0 with an even integer part.
#[inline(always)]
pub fn eval_lgamma(x: f32, mode: Rounding) -> Option<((f32, Status), i32)> {
    let r = eval(Fast::Lgamma, x, mode)?;
    // RANGE: clamped to [-2^24, 0] (D-10.95): the compiler may convert before the `x < 0` test,
    // and an unclamped huge or NaN x would raise invalid in hardware. Every float <= -2^24 is an
    // even integer, as -2^24 is, so the parity read for x < 0 is unchanged.
    let n = x.max(-16_777_216.0).min(0.0); // max, not clamp: a NaN x becomes -2^24, never NaN

    Some((r, if x < 0.0 && (n as i32) & 1 == 0 { -1 } else { 1 }))
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
        for (d, b) in [(TGAMMA_DELTA, GAMMA_BOUND), (LGAMMA_DELTA, LGAMMA_BOUND), (LGAMMA_SMALL_DELTA, LGAMMA_SMALL_BOUND)] {
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
            if let Some((y, d)) = lgamma_core(x) {
                lg += 1;
                let f = crate::special::lgamma_accurate::<f64>(xd, Rounding::Nearest).0.0;
                let ulp = f64::from_bits(y.to_bits() & 0x7ff0_0000_0000_0000) / 4_503_599_627_370_496.0;
                let b = if d == LGAMMA_SMALL_DELTA { LGAMMA_SMALL_BOUND } else { LGAMMA_BOUND };
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
        for x in [1.0f32, 2.0, 0.0, -0.0, -1.0, -3.0, -64.5, f32::INFINITY, f32::NAN] {
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
