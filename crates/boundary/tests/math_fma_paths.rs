//! The `double` exp/log fast paths' two builds (MATH-F64-FMA-PATH, D-10.25): the FMA build and
//! the baseline must return the same results bit for bit, so a random sample goes through both
//! (forced by `selftest::set_math_fma`) and through the accurate path. Clause: s10_0_f_0 (the
//! functions are correctly rounded in every build).

// Sample generation is bit-level arithmetic on bounded values; indexing is over equal-length vectors.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing, clippy::as_conversions)]
// exp/log are Liella's own C entries (m::exp, m::log): keep LLVM from treating them as builtins.
#![no_builtins]

use liella_boundary::posix::s10_math_gen as m;
use liella_boundary::selftest;
use liella_math::real::Real;
use liella_math::status::Rounding;

/// xorshift64: a fixed, reproducible sample.
fn next(s: &mut u64) -> u64 {
    *s ^= *s << 13;
    *s ^= *s >> 7;
    *s ^= *s << 17;
    *s
}

fn both(f: extern "C" fn(f64) -> f64, exact: fn(f64) -> f64, xs: &[f64]) -> usize {
    assert!(selftest::set_math_fma(true));
    assert!(selftest::math_fma(), "set_math_fma(true) did not select the FMA build");
    let fused: Vec<u64> = xs.iter().map(|&x| f(x).to_bits()).collect();
    assert!(selftest::set_math_fma(false));
    assert!(!selftest::math_fma(), "set_math_fma(false) did not select the baseline build");
    let mut checked = 0;
    for (i, &x) in xs.iter().enumerate() {
        let plain = f(x).to_bits();
        assert_eq!(fused[i], plain, "x = {x:e} ({:#x}): FMA build {:#x}, baseline {plain:#x}", x.to_bits(), fused[i]);
        if i % 64 == 0 {
            assert_eq!(plain, exact(x).to_bits(), "x = {x:e}: not the correctly rounded result");
            checked += 1;
        }
    }
    checked
}

// s10_3_c_0, s10_0_g_0: every build of exp/log computes the same, correctly rounded result.
#[test]
fn exp_and_log_fma_build_matches_baseline_bit_for_bit() {
    // exp, log and exp10 (each export with an FMA build).
    if !selftest::set_math_fma(true) {
        eprintln!("SKIP: this CPU (or its OS) has no FMA; only the baseline build exists here");
        return;
    }
    let mut s = 0x9e37_79b9_7f4a_7c15u64;
    // exp: uniform over the fast domain, plus small magnitudes down to 2^-62 (both sides of TINY).
    let mut xs: Vec<f64> = (0..200_000).map(|_| (next(&mut s) >> 11) as f64 * 2f64.powi(-53) * 1420.0 - 710.0).collect();
    xs.extend((0..50_000).map(|_| {
        let b = next(&mut s);
        let mag = 2f64.powi(-((b % 62) as i32)) * (1.0 + (b >> 12) as f64 * 2f64.powi(-52));
        if b & 1 == 0 { mag } else { -mag }
    }));
    let n = both(m::exp, |x| Real::exp(x, Rounding::Nearest).0, &xs);
    // log: random positive bit patterns (every binade, subnormals included) and values near 1.
    let mut ls: Vec<f64> = (0..200_000).map(|_| f64::from_bits(next(&mut s) >> 1)).filter(|x| x.is_finite()).collect();
    ls.extend((0..50_000).map(|_| 1.0 + ((next(&mut s) >> 11) as f64 * 2f64.powi(-53) - 0.5) * 2f64.powi(-((next(&mut s) % 60) as i32))));
    let k = both(m::log, |x| Real::log(x, Rounding::Nearest).0, &ls);
    // exp10 (LIBM-F64-FAST-KERNELS): uniform over the fast domain, next to the multiples of
    // log10(2)/64 (tiny reduced arguments) and small magnitudes.
    let l = core::f64::consts::LOG10_2 / 64.0;
    let mut ts: Vec<f64> = (0..200_000).map(|_| (next(&mut s) >> 11) as f64 * 2f64.powi(-53) * 616.0 - 308.0).collect();
    ts.extend((0..50_000).map(|_| {
        let b = next(&mut s);
        let c = ((b % 130_000) as f64 - 65_000.0) * l;
        f64::from_bits(c.to_bits().wrapping_add((b >> 40) % 64).wrapping_sub(32))
    }).filter(|x| x.is_finite()));
    ts.extend((0..20_000).map(|_| 2f64.powi(-((next(&mut s) % 62) as i32)) * if next(&mut s) & 1 == 0 { 1.0 } else { -1.0 }));
    let j = both(m::exp10, |x| Real::exp10(x, Rounding::Nearest).0, &ts);
    assert!(n > 3000 && k > 3000 && j > 3000, "sample too small: {n} {k} {j}");
    // Leave the startup decision in force for any later test in this binary.
    assert!(selftest::set_math_fma(true));
}

/// [`both`] for pow's two arguments.
fn both2(xs: &[(f64, f64)]) -> usize {
    assert!(selftest::set_math_fma(true));
    let fused: Vec<u64> = xs.iter().map(|&(x, y)| m::pow(x, y).to_bits()).collect();
    assert!(selftest::set_math_fma(false));
    let mut checked = 0;
    for (i, &(x, y)) in xs.iter().enumerate() {
        let plain = m::pow(x, y).to_bits();
        assert_eq!(fused[i], plain, "pow({x:e}, {y:e}): FMA build {:#x}, baseline {plain:#x}", fused[i]);
        if i % 64 == 0 {
            assert_eq!(plain, Real::pow(x, y, Rounding::Nearest).0.to_bits(), "pow({x:e}, {y:e}): not the correctly rounded result");
            checked += 1;
        }
    }
    checked
}

// s10_3_c_0, s10_0_g_0: exp2, log2, log10 and pow (LIBM-F64-EXPLOGPOW-TIER: each export with an
// FMA build) compute the same, correctly rounded result in both builds.
#[test]
fn exp2_logb_pow_fma_build_matches_baseline_bit_for_bit() {
    if !selftest::set_math_fma(true) {
        eprintln!("SKIP: this CPU (or its OS) has no FMA; only the baseline build exists here");
        return;
    }
    let mut s = 0x6a09_e667_f3bc_c908u64;
    let unit = |s: &mut u64| (next(s) >> 11) as f64 * 2f64.powi(-53);
    // exp2: uniform over the fast domain, next to the multiples of 1/64 (r tiny, r = 0), small
    // magnitudes down to 2^-62.
    let mut es: Vec<f64> = (0..150_000).map(|_| unit(&mut s) * 2044.0 - 1022.0).collect();
    es.extend((0..50_000).map(|_| {
        let b = next(&mut s);
        let c = ((b % 130_000) as f64 - 65_000.0) / 64.0;
        f64::from_bits(c.to_bits().wrapping_add((b >> 40) % 64).wrapping_sub(32))
    }).filter(|x| x.is_finite()));
    es.extend((0..20_000).map(|_| 2f64.powi(-((next(&mut s) % 62) as i32)) * (1.0 + unit(&mut s)) * if next(&mut s) & 1 == 0 { 1.0 } else { -1.0 }));
    let a = both(m::exp2, |x| Real::exp2(x, Rounding::Nearest).0, &es);
    // log2, log10: random positive bit patterns (every binade, subnormals included), values near
    // 1 and next to the powers of two and ten.
    let mut ls: Vec<f64> = (0..150_000).map(|_| f64::from_bits(next(&mut s) >> 1)).filter(|x| x.is_finite()).collect();
    ls.extend((0..50_000).map(|_| 1.0 + (unit(&mut s) - 0.5) * 2f64.powi(-((next(&mut s) % 60) as i32))));
    ls.extend((0..20_000).map(|_| {
        let b = next(&mut s);
        let p = if b & 1 == 0 { 2f64.powi((b % 2000) as i32 - 1000) } else { 10f64.powi((b % 600) as i32 - 300) };
        f64::from_bits(p.to_bits().wrapping_add((b >> 40) % 16).wrapping_sub(8))
    }).filter(|x| x.is_finite() && *x > 0.0));
    let b = both(m::log2, |x| Real::log2(x, Rounding::Nearest).0, &ls);
    let c = both(m::log10, |x| Real::log10(x, Rounding::Nearest).0, &ls);
    // pow: the bench ranges, wide x with y·ln x over the exp range, x near 1, negative x with an
    // odd integral y.
    let mut ps: Vec<(f64, f64)> = (0..100_000).map(|_| (0.01 + 9.99 * unit(&mut s), unit(&mut s) * 20.0 - 10.0)).collect();
    ps.extend((0..40_000).map(|_| {
        let x = f64::from_bits(0x0010_0000_0000_0000 + next(&mut s) % 0x7fd0_0000_0000_0000);
        (x, (unit(&mut s) * 1440.0 - 740.0) / x.ln())
    }));
    ps.extend((0..20_000).map(|_| (1.0 + (unit(&mut s) - 0.5) * 2f64.powi(-((next(&mut s) % 50) as i32)), unit(&mut s) * 2000.0 - 1000.0)));
    ps.extend((0..10_000).map(|_| (-(0.5 + unit(&mut s)), f64::from(2 * (next(&mut s) % 300) as i32 - 299))));
    let d = both2(&ps);
    assert!(a > 3000 && b > 3000 && c > 3000 && d > 2000, "sample too small: {a} {b} {c} {d}");
    // pow's refusals (its domain ladder: x positive, normal, finite, not 1; |y| >= 2^-200; y·2^11
    // not an integer): each reaches the accurate path in both builds.
    let (inf, nan, tiny) = (f64::INFINITY, f64::NAN, f64::MIN_POSITIVE / 4.0);
    let edge = [(-2.0, 0.3), (-0.5, 1.5), (-3.0, 3.0), (0.0, 0.7), (-0.0, -0.7), (inf, -0.3), (-inf, 0.3), (nan, 0.3), (2.0, nan), (1.0, 0.3), (tiny, 0.3), (3.0, 2.0), (16.0, 0.25), (2.0, 1e-300), (0.5, 1.0 / 2048.0)];
    for fma in [true, false] {
        assert!(selftest::set_math_fma(fma));
        for (x, y) in edge {
            let (got, want) = (m::pow(x, y), Real::pow(x, y, Rounding::Nearest).0);
            assert_eq!(got.to_bits(), want.to_bits(), "pow({x:e}, {y:e}) refusal, FMA build {fma}: {got:e}, accurate path {want:e}");
        }
    }
    assert!(selftest::set_math_fma(true));
}

/// `sincos` through its export, as one value pair of bits.
fn sincos_bits(x: f64) -> (u64, u64) {
    let (mut s, mut c) = (0.0, 0.0);
    m::sincos(x, Some(&mut s), Some(&mut c));
    (s.to_bits(), c.to_bits())
}

/// The trig sample (LIBM-F64-TRIG-TIER): every binade of the fast domain [2^-30, 2^20) and beyond
/// it on both sides, next to the cell edges k·π/64 ± π/128 and the multiples of π/2 (a few ulps
/// apart: reduced arguments near zero and cancellations), and both signs.
fn trig_sample(s: &mut u64) -> Vec<f64> {
    let sign = |b: u64, v: f64| if b & 1 == 0 { v } else { -v };
    let mut xs: Vec<f64> = (0..120_000).map(|_| {
        let b = next(s);
        let e = (b % 60) as i32 - 36; // 2^-36 .. 2^23
        sign(b >> 63, 2f64.powi(e) * (1.0 + (next(s) >> 12) as f64 * 2f64.powi(-52)))
    }).collect();
    let q = core::f64::consts::PI / 128.0;
    xs.extend((0..60_000).map(|_| {
        let b = next(s);
        let k = ((b % 4_000_000) + 1) as f64; // cell edges (odd k) and cell centres (k = 2j) up to ~10^5
        let c = k * q;
        sign(b >> 63, f64::from_bits(c.to_bits().wrapping_add((b >> 40) % 9).wrapping_sub(4)))
    }));
    xs.extend((0..20_000).map(|_| {
        let b = next(s);
        let c = ((b % 600_000) + 1) as f64 * core::f64::consts::FRAC_PI_2;
        sign(b >> 63, f64::from_bits(c.to_bits().wrapping_add((b >> 40) % 5).wrapping_sub(2)))
    }));
    xs
}

// s10_3_c_0, s10_0_g_0: sin, cos, tan and sincos (LIBM-F64-TRIG-TIER: each export with an FMA
// build) compute the same, correctly rounded result in both builds.
#[test]
fn trig_fma_build_matches_baseline_bit_for_bit() {
    if !selftest::set_math_fma(true) {
        eprintln!("SKIP: this CPU (or its OS) has no FMA; only the baseline build exists here");
        return;
    }
    let mut s = 0xbb67_ae85_84ca_a73bu64;
    let xs = trig_sample(&mut s);
    let a = both(m::sin, |x| Real::sin(x, Rounding::Nearest).0, &xs);
    let b = both(m::cos, |x| Real::cos(x, Rounding::Nearest).0, &xs);
    let c = both(m::tan, |x| Real::tan(x, Rounding::Nearest).0, &xs);
    assert!(a > 3000 && b > 3000 && c > 3000, "sample too small: {a} {b} {c}");
    let fused: Vec<(u64, u64)> = xs.iter().map(|&x| sincos_bits(x)).collect();
    assert!(selftest::set_math_fma(false));
    for (i, &x) in xs.iter().enumerate() {
        let plain = sincos_bits(x);
        assert_eq!(fused[i], plain, "sincos({x:e}): FMA build {:x?}, baseline {plain:x?}", fused[i]);
        if i % 64 == 0 {
            let ((sv, cv), _) = Real::sincos(x, Rounding::Nearest);
            assert_eq!(plain, (sv.to_bits(), cv.to_bits()), "sincos({x:e}): not the correctly rounded pair");
        }
    }
    assert!(selftest::set_math_fma(true));
}

// s10_3_c_0 (D-10.13), s10_0_g_0: erf and erfc (LIBM-SPECIAL-FAST-TIER-3's in-place paths) give
// the same correctly rounded result in both builds, and in the default environment raise inexact
// alone (and underflow for a subnormal result), as the accurate path does.
#[test]
fn erf_and_erfc_fma_build_matches_baseline_and_raises_inexact_alone() {
    use liella_boundary::posix::s10_math::{feclearexcept, fetestexcept};
    const FE_ALL: i32 = 0x3d;
    const FE_UNDERFLOW: i32 = 0x10;
    const FE_INEXACT: i32 = 0x20;
    if !selftest::set_math_fma(true) {
        eprintln!("SKIP: this CPU (or its OS) has no FMA; only the baseline build exists here");
        return;
    }
    let mut s = 0x6a09_e667_f3bc_c908u64;
    let unit = |s: &mut u64| (next(s) >> 11) as f64 * 2f64.powi(-53);
    // (−7, 7): every branch of both functions; [1, 27.6): erfc's e^(−x²)·Y branch and its end,
    // subnormal results included; magnitudes down to 2^-60 on both sides of 2^-55.
    let mut xs: Vec<f64> = (0..100_000).map(|_| unit(&mut s) * 14.0 - 7.0).collect();
    xs.extend((0..50_000).map(|_| 1.0 + unit(&mut s) * 26.6));
    xs.extend((0..20_000).map(|_| {
        let b = next(&mut s);
        let mag = 2f64.powi(-((b % 60) as i32)) * (1.0 + (b >> 12) as f64 * 2f64.powi(-52));
        if b & 1 == 0 { mag } else { -mag }
    }));
    let n = both(m::erf, |x| liella_math::special::erf(x, Rounding::Nearest).0, &xs);
    let k = both(m::erfc, |x| liella_math::special::erfc(x, Rounding::Nearest).0, &xs);
    assert!(n > 2000 && k > 2000, "sample too small: {n} {k}");
    for build in [true, false] {
        assert!(selftest::set_math_fma(build));
        for &x in xs.iter().step_by(7) {
            for (name, f) in [("erf", m::erf as extern "C" fn(f64) -> f64), ("erfc", m::erfc)] {
                feclearexcept(FE_ALL);
                let v = f(x);
                let raised = fetestexcept(FE_ALL);
                // Every sampled x is finite and nonzero, so every result is inexact; below 2^-1022
                // (zero included: erfc x for x above about 27.2) it is tiny, so underflow too.
                let want = FE_INEXACT | if v.abs() < f64::MIN_POSITIVE { FE_UNDERFLOW } else { 0 };
                assert_eq!(raised, want, "{name}({x:e}) = {v:e} (FMA build {build}): flags {raised:#x}, expected {want:#x}");
            }
        }
    }
    assert!(selftest::set_math_fma(true));
}
