//! The `expm1`/`log1p`/`cbrt`/`sinh`/`cosh`/`tanh` fast tier against the accurate path (no spec
//! clause; D-10.13: where an entry answers, it is the accurate path's result bit for bit). Plain
//! build only: the FMA build's bit-identity is checked through the exports (`math_fma_paths`).
//! Each sweep also requires the tier to answer at least 99.9% of the time (a weaker bound or a
//! worse cbrt seed shows as a low rate), and `cbrt` must decline every exact cube.

use crate::real::Real;
use crate::status::Rounding;

/// xorshift64 uniform in `[lo, hi)`.
fn uniform(n: usize, seed: u64, lo: f64, hi: f64) -> impl Iterator<Item = f64> {
    let mut st = seed;
    (0..n).map(move |_| {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        lo + (hi - lo) * (f64::from_bits(0x3ff0_0000_0000_0000 | (st >> 12)) - 1.0)
    })
}

/// Signed magnitudes `±m·2^-e`, `e` uniform in `[0, emax)`, `m` in `[1, 2)`.
fn logscale(n: usize, seed: u64, emax: u64) -> impl Iterator<Item = f64> {
    let mut st = seed;
    (0..n).map(move |_| {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        let m = f64::from_bits(0x3ff0_0000_0000_0000 | (st >> 12));
        let e = i32::try_from(st % emax).unwrap_or(0);
        let v = m * f64::powi(2.0, -e);
        if st & 0x800 == 0 { v } else { -v }
    })
}

/// Checks `fast` against `exact` on `xs` and returns `(answered, total)`; every answer must be
/// the accurate result bit for bit.
fn check(name: &str, fast: fn(f64) -> Option<f64>, exact: fn(f64) -> f64, xs: impl Iterator<Item = f64>) -> (usize, usize) {
    let (mut got, mut n) = (0, 0);
    for x in xs {
        n += 1;
        if let Some(v) = fast(x) {
            got += 1;
            let e = exact(x);
            assert_eq!(v.to_bits(), e.to_bits(), "{name}({x:e} = {:#x}): fast {v:e}, accurate {e:e}", x.to_bits());
        }
    }
    (got, n)
}

fn rate(name: &str, (got, n): (usize, usize)) {
    assert!(n > 0 && got * 1000 >= n * 999, "{name}: the fast tier answered {got} of {n} (below 99.9%)");
}

/// The accurate path in round-to-nearest, as `fn(f64) -> f64`.
macro_rules! rn {
    ($f:ident) => {
        {
            let f: fn(f64) -> f64 = |x: f64| Real::$f(x, Rounding::Nearest).0;
            f
        }
    };
}

#[test]
fn expm1_tier_matches_the_accurate_path() {
    let e = rn!(expm1);
    rate("expm1 series", check("expm1", super::expm1_nearest, e, logscale(40_000, 0x1234_5678_9abc_def1, 60).map(|x| x * 0.0078)));
    rate("expm1 kernel", check("expm1", super::expm1_nearest, e, uniform(60_000, 0x9e37_79b9_7f4a_7c15, -36.9, 708.0)));
    rate("expm1 near 2^-6", check("expm1", super::expm1_nearest, e, uniform(40_000, 0x2545_f491_4f6c_dd1d, -0.2, 0.2)));
}

#[test]
fn log1p_tier_matches_the_accurate_path() {
    let e = rn!(log1p);
    rate("log1p series", check("log1p", super::log1p_nearest, e, logscale(40_000, 0x1234_5678_9abc_def1, 60).map(|x| x * 0.0039)));
    rate("log1p kernel", check("log1p", super::log1p_nearest, e, uniform(40_000, 0x9e37_79b9_7f4a_7c15, -0.999_999, 4.0)));
    rate("log1p near -1", check("log1p", super::log1p_nearest, e, logscale(20_000, 0x2545_f491_4f6c_dd1d, 50).map(|x| -1.0 + x.abs())));
    let big = logscale(40_000, 0x5151_aaaa_1234_9876, 900).map(|x| 1.0 / x.abs());
    rate("log1p large", check("log1p", super::log1p_nearest, e, big.filter(|x| *x < 1e288)));
}

#[test]
fn hyperbolic_tier_matches_the_accurate_path() {
    let rows: [(&str, fn(f64) -> Option<f64>, fn(f64) -> f64, f64); 3] = [
        ("sinh", super::sinh_nearest, rn!(sinh), 708.0),
        ("cosh", super::cosh_nearest, rn!(cosh), 708.0),
        ("tanh", super::tanh_nearest, rn!(tanh), 22.0),
    ];
    for (name, f, ex, top) in rows {
        rate(name, check(name, f, ex, logscale(40_000, 0x1234_5678_9abc_def1, 60).map(|x| x * 0.0078)));
        rate(name, check(name, f, ex, uniform(60_000, 0x9e37_79b9_7f4a_7c15, -top, top)));
        rate(name, check(name, f, ex, uniform(40_000, 0x2545_f491_4f6c_dd1d, -45.0, 45.0)));
    }
}

#[test]
fn cbrt_tier_matches_the_accurate_path_and_declines_exact_cubes() {
    let e = rn!(cbrt);
    // Every exponent residue: random significands over the whole normal range.
    let mut st = 0x0bad_cafe_dead_beefu64;
    let xs: Vec<f64> = (0..120_000)
        .map(|_| {
            st ^= st << 13;
            st ^= st >> 7;
            st ^= st << 17;
            f64::from_bits(st & 0xffef_ffff_ffff_ffff | 0x0010_0000_0000_0000)
        })
        .collect();
    rate("cbrt", check("cbrt", super::cbrt_nearest, e, xs.into_iter()));
    rate("cbrt [1, 8)", check("cbrt", super::cbrt_nearest, e, uniform(40_000, 0x9e37_79b9_7f4a_7c15, 1.0, 8.0)));
    // Exact cubes n³·2^(3k): declined (the accurate path raises no inexact for them).
    for n in (1u32..2000).chain((1u32..2000).map(|i| 131_071 - i)) {
        let c = f64::from(n).powi(3);
        for k in [-300, -7, -1, 0, 1, 5, 100] {
            let x = c * f64::powi(2.0, 3 * k);
            assert_eq!(super::cbrt_nearest(x), None, "cbrt({x:e}) is the exact cube of {n}·2^{k} but the tier answered");
            assert_eq!(super::cbrt_nearest(-x), None, "cbrt({:e}) is an exact cube but the tier answered", -x);
        }
    }
}
