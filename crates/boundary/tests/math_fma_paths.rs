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
