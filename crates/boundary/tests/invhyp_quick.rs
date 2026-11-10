//! The `double` `asinh`, `acosh`, `atanh` quick phase (LIBM-F64-INVHYP-QUICK, D-10.51, D-10.65)
//! in both builds through the boundary (the FMA build needs the startup dispatch), and the three
//! entries over the tiers. Clause: s10_0_f_0 (§10.2, D-10.3): every value the quick phase returns
//! is the correctly rounded result, so its two builds are bit-identical wherever both answer.
//! `LIELLA_INVHYP_N` (default 20000) sets the operands per region; the long accuracy job runs it
//! with a larger value.

// Sample generation is bit-level arithmetic on bounded values.
#![allow(clippy::arithmetic_side_effects, clippy::as_conversions)]
// asinh, acosh, atanh are Liella's own C entries: keep LLVM from treating them as builtins.
#![no_builtins]

use liella_boundary::posix::s10_math_gen as m;
use liella_boundary::selftest;
use liella_math::fast64::quick_log::{C_PAIR, RHO_PAIR};
use liella_math::real::Real;
use liella_math::status::Rounding;

/// Serialises the tests that switch the process-wide build.
static BUILD: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn per_region() -> usize {
    std::env::var("LIELLA_INVHYP_N").ok().and_then(|v| v.parse().ok()).unwrap_or(20_000)
}

/// xorshift64 in `[0, 1)`.
fn unit(n: usize, seed: u64) -> impl Iterator<Item = f64> {
    let mut st = seed;
    (0..n).map(move |_| {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        f64::from_bits(0x3ff0_0000_0000_0000 | (st >> 12)) - 1.0
    })
}

fn uniform(n: usize, seed: u64, lo: f64, hi: f64) -> impl Iterator<Item = f64> {
    unit(n, seed).map(move |u| lo + (hi - lo) * u)
}

/// Log-uniform over `[lo, hi)`: every binade sampled alike.
fn logu(n: usize, seed: u64, lo: f64, hi: f64) -> impl Iterator<Item = f64> {
    let (a, b) = (lo.log2(), hi.log2());
    unit(n, seed).map(move |u| (a + (b - a) * u).exp2().clamp(lo, hi))
}

fn two(e: i32) -> f64 {
    2f64.powi(e)
}

struct Fun {
    name: &'static str,
    quick: fn(f64) -> Option<f64>,
    entry: fn(f64) -> f64,
    cr: fn(f64) -> f64,
}

const FUNS: [Fun; 3] = [
    Fun { name: "asinh", quick: selftest::asinh_quick, entry: |x| m::asinh(x), cr: |x| Real::asinh(x, Rounding::Nearest).0 },
    Fun { name: "acosh", quick: selftest::acosh_quick, entry: |x| m::acosh(x), cr: |x| Real::acosh(x, Rounding::Nearest).0 },
    Fun { name: "atanh", quick: selftest::atanh_quick, entry: |x| m::atanh(x), cr: |x| Real::atanh(x, Rounding::Nearest).0 },
];

/// Each function's regions: next to its singular points (`acosh` near 1, `|x|` near 1 for
/// `atanh`), the regime boundaries (`1`, `2`, `2^32`) and the binades of the quick range.
fn operands(f: &str, n: usize) -> Vec<f64> {
    let mut v: Vec<f64> = Vec::new();
    match f {
        "asinh" => {
            v.extend(logu(n, 1, two(-200), two(-26)));
            v.extend(logu(n, 2, two(-26), two(-7)));
            v.extend(uniform(n, 3, two(-7), 1.0));
            v.extend(uniform(n, 4, 1.0, 4.0));
            v.extend(logu(n, 5, 4.0, two(32)));
            v.extend(logu(n, 6, two(31), two(1000)));
        }
        "acosh" => {
            v.extend(uniform(n, 7, 1.0, 1.0 + two(-30)));
            v.extend(logu(n, 8, two(-52), two(-7)).map(|d| 1.0 + d));
            v.extend(uniform(n, 9, 1.0, 2.0));
            v.extend(uniform(n, 10, 2.0, 8.0));
            v.extend(logu(n, 11, 8.0, two(32)));
            v.extend(logu(n, 12, two(31), two(1000)));
        }
        _ => {
            v.extend(logu(n, 13, two(-200), two(-7)));
            v.extend(uniform(n, 14, two(-7), 0.5));
            v.extend(uniform(n, 15, 0.5, 1.0));
            v.extend(logu(n, 16, two(-53), two(-7)).map(|d| 1.0 - d));
        }
    }
    let neg: Vec<f64> = if f == "acosh" { Vec::new() } else { v.iter().step_by(2).map(|x| -x).collect() };
    v.extend(neg);
    v
}

/// Every answer of the quick phase and of the entry is the correctly rounded result; returns the
/// quick phase's misses.
fn check(build: &str, f: &Fun, xs: &[f64]) -> usize {
    let mut miss = 0;
    for &x in xs {
        let want = (f.cr)(x).to_bits();
        match (f.quick)(x) {
            Some(v) => assert_eq!(v.to_bits(), want, "{build} quick {}({x:e} = {:#x}) misrounded", f.name, x.to_bits()),
            None => miss += 1,
        }
        assert_eq!((f.entry)(x).to_bits(), want, "{build} {}({x:e}) entry misrounded", f.name);
    }
    miss
}

/// Both builds: every answer correctly rounded, and the fallback rate below 0.6% (model
/// `C_PAIR − 1 = 0.196%`; a rate far above it means the pair is less accurate than its proof).
/// Clause: s10_0_f_0.
#[test]
fn invhyp_quick_both_builds_round_correctly() {
    let _held = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let had = selftest::math_fma();
    let n = per_region();
    for (build, fma) in [("SSE2", false), ("FMA", true)] {
        if !selftest::set_math_fma(fma) {
            eprintln!("SKIP {build}: this CPU (or its OS) has no FMA; only the SSE2 build exists here");
            continue;
        }
        for f in &FUNS {
            let xs = operands(f.name, n);
            let miss = check(build, f, &xs);
            eprintln!("{build} {}: {miss} misses of {}", f.name, xs.len());
            assert!(miss * 1000 <= xs.len() * 6, "{build} {} fallback {miss} of {} above 0.6%", f.name, xs.len());
            // The test's window is `C_PAIR − 1` wide whatever the actual error, so a rate far
            // below the model means the rounding test no longer decides (measured 0.12-0.21%).
            assert!(miss * 2000 >= xs.len(), "{build} {}: fallback {miss} of {} below 0.05%: rounding test inactive", f.name, xs.len());
        }
    }
    assert!(selftest::set_math_fma(had));
}

/// The quick phase declines outside its ranges (the accurate path keeps the special cases and
/// the flags there) and gives `acosh(1) = +0` exactly. Clause: s10_0_f_0.
#[test]
fn invhyp_quick_ranges() {
    let _held = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let had = selftest::math_fma();
    for fma in [false, true] {
        if !selftest::set_math_fma(fma) {
            continue;
        }
        assert_eq!(selftest::acosh_quick(1.0).map(f64::to_bits), Some(0));
        for x in [0.5, 1.0 - two(-53), 0.0, -1.0, f64::NAN, f64::INFINITY, two(1000)] {
            assert_eq!(selftest::acosh_quick(x), None, "acosh_quick({x:e}) answered");
        }
        for x in [0.0, -0.0, two(-201), -two(-201), f64::NAN, f64::INFINITY, -two(1000), f64::MIN_POSITIVE] {
            assert_eq!(selftest::asinh_quick(x), None, "asinh_quick({x:e}) answered");
        }
        for x in [1.0, -1.0, 0.0, two(-201), f64::NAN, 2.0, f64::NEG_INFINITY] {
            assert_eq!(selftest::atanh_quick(x), None, "atanh_quick({x:e}) answered");
        }
    }
    assert!(selftest::set_math_fma(had));
}

/// `C_PAIR` is the least double with `C_PAIR·(1 − 2^54·RHO_PAIR) > 1 + 2^-53` (exact integer
/// check, `RHO_PAIR = 2^-63`). Clause: s10_0_f_0.
#[test]
fn invhyp_quick_rounding_constant() {
    assert_eq!(RHO_PAIR.to_bits(), two(-63).to_bits());
    let k = u128::from(C_PAIR.to_bits() - 1f64.to_bits());
    // (1 + k·2^-52)(1 − 2^-9) > 1 + 2^-53, times 2^61.
    let holds = |k: u128| ((1u128 << 52) + k) * 511 > (1u128 << 61) + 256;
    assert!(holds(k) && !holds(k - 1), "C_PAIR is not the least valid multiplier");
}
