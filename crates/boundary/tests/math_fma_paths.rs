//! The `double` exp/log fast paths' two builds (MATH-F64-FMA-PATH, D-10.22): the FMA build and
//! the baseline must return the same results bit for bit, so a random sample goes through both
//! (forced by `selftest::set_math_fma`) and through the accurate path. Clause: s10_0_f_0 (the
//! functions are correctly rounded in every build).

// Sample generation is bit-level arithmetic on bounded values; indexing is over equal-length vectors.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing, clippy::as_conversions)]

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

#[test]
fn exp_and_log_fma_build_matches_baseline_bit_for_bit() {
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
    assert!(n > 3000 && k > 3000, "sample too small: {n} {k}");
    // Leave the startup decision in force for any later test in this binary.
    assert!(selftest::set_math_fma(true));
}
