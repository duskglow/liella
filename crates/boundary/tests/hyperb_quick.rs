//! The `double` `sinh`, `cosh`, `tanh` quick phase (LIBM-F64-HYPERB-QUICK, D-10.70) in both
//! builds through the boundary (the FMA build needs the startup dispatch), and the three entries
//! over the tiers. Clause: s10_0_f_0 (§10.2, D-10.3): every value the quick phase returns is the
//! correctly rounded result, so its two builds are bit-identical wherever both answer.
//! `LIELLA_HYPERB_N` (default 10000) sets the operands per region; the long accuracy job runs it
//! with a larger value (four modes: `|x| < 2^-28`, around sinh's series switch `2^-5`, around
//! `ln(2^1024)`, and tanh's subnormal results near 0).

// Sample generation is bit-level arithmetic on bounded values.
#![allow(clippy::arithmetic_side_effects, clippy::as_conversions)]
// sinh, cosh, tanh are Liella's own C entries: keep LLVM from treating them as builtins.
#![no_builtins]

use liella_boundary::posix::s10_math_gen as m;
use liella_boundary::selftest;
use liella_math::real::Real;
use liella_math::status::Rounding;

/// Serialises the tests that switch the process-wide build.
static BUILD: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn per_region() -> usize {
    std::env::var("LIELLA_HYPERB_N").ok().and_then(|v| v.parse().ok()).unwrap_or(10_000)
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
    id: u8,
    entry: fn(f64) -> f64,
    cr: fn(f64) -> f64,
}

const FUNS: [Fun; 3] = [
    Fun { name: "sinh", id: 0, entry: |x| m::sinh(x), cr: |x| Real::sinh(x, Rounding::Nearest).0 },
    Fun { name: "cosh", id: 1, entry: |x| m::cosh(x), cr: |x| Real::cosh(x, Rounding::Nearest).0 },
    Fun { name: "tanh", id: 2, entry: |x| m::tanh(x), cr: |x| Real::tanh(x, Rounding::Nearest).0 },
];

/// The regions: tiny arguments, sinh's series switch (`2^-5`), the binades up to the `e^a/2`
/// switch (`23`) and tanh's quick limit (`22`), the bench's range, and the region around
/// `ln(2^1024)` where the quick phase hands over to tier 2 and the overflow thresholds.
fn operands(n: usize) -> Vec<f64> {
    let mut v: Vec<f64> = Vec::new();
    v.extend(logu(n, 1, two(-60), two(-28)));
    v.extend(logu(n, 2, two(-28), two(-6)));
    v.extend(uniform(n, 3, two(-6), two(-4)));
    v.extend(uniform(n, 4, two(-4), 1.0));
    v.extend(uniform(n, 5, 1.0, 10.0));
    v.extend(uniform(n, 6, 20.0, 25.0));
    v.extend(logu(n, 7, 25.0, 700.0));
    v.extend(uniform(n, 8, 705.0, 711.0));
    let neg: Vec<f64> = v.iter().step_by(2).map(|x| -x).collect();
    v.extend(neg);
    v
}

/// Every answer of the quick phase and of the entry is the correctly rounded result; returns the
/// quick phase's misses inside its domain.
fn check(build: &str, f: &Fun, xs: &[f64]) -> (usize, usize) {
    let (mut miss, mut inside) = (0, 0);
    let hi = if f.id == 2 { 22.0 } else { 708.0 };
    for &x in xs {
        let want = (f.cr)(x).to_bits();
        let got = selftest::hyp_quick(f.id, x);
        if let Some(v) = got {
            assert_eq!(v.to_bits(), want, "{build} quick {}({x:e} = {:#x}) misrounded", f.name, x.to_bits());
        }
        if x.abs() < hi {
            inside += 1;
            miss += usize::from(got.is_none());
        }
        assert_eq!((f.entry)(x).to_bits(), want, "{build} {}({x:e}) entry misrounded", f.name);
    }
    (miss, inside)
}

/// Both builds: every answer correctly rounded, and the fallback rate inside the quick domain
/// below 1.2% (model `C_M1 − 1 = 0.59%` where the low part is not far below half an ulp; a rate
/// far above it means a composition is less accurate than its proof). Clause: s10_0_f_0.
#[test]
fn hyperb_quick_both_builds_round_correctly() {
    let _held = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let had = selftest::math_fma();
    let xs = operands(per_region());
    for (build, fma) in [("SSE2", false), ("FMA", true)] {
        if !selftest::set_math_fma(fma) {
            eprintln!("SKIP {build}: this CPU (or its OS) has no FMA; only the SSE2 build exists here");
            continue;
        }
        for f in &FUNS {
            let (miss, inside) = check(build, f, &xs);
            eprintln!("{build} {}: {miss} misses of {inside}", f.name);
            assert!(miss * 1000 <= inside * 12, "{build} {} fallback {miss} of {inside} above 1.2%", f.name);
            // The window is `C_M1 − 1` wide whatever the actual error: a rate far below the model
            // means the rounding test no longer decides.
            assert!(miss * 5000 >= inside, "{build} {}: fallback {miss} of {inside} below 0.02%: rounding test inactive", f.name);
        }
    }
    assert!(selftest::set_math_fma(had));
}

/// The quick phase declines outside its domains (tier 2 and the accurate path keep the special
/// cases, the overflow thresholds and the flags there), and tanh's subnormal results near 0 come
/// out correctly rounded through the entry. Clause: s10_0_f_0.
#[test]
fn hyperb_quick_ranges() {
    let _held = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let had = selftest::math_fma();
    for fma in [false, true] {
        if !selftest::set_math_fma(fma) {
            continue;
        }
        for f in 0..3u8 {
            for x in [0.0, -0.0, two(-61), f64::NAN, f64::INFINITY, -f64::INFINITY, 708.0, 710.0, f64::MIN_POSITIVE] {
                assert_eq!(selftest::hyp_quick(f, x), None, "quick {f}({x:e}) answered");
            }
        }
        assert_eq!(selftest::hyp_quick(2, 22.5), None, "tanh quick answered above 22");
        for x in logu(per_region(), 9, two(-1074), two(-1020)) {
            for y in [x, -x] {
                let want = Real::tanh(y, Rounding::Nearest).0.to_bits();
                assert_eq!(m::tanh(y).to_bits(), want, "tanh({y:e}) entry misrounded");
            }
        }
    }
    assert!(selftest::set_math_fma(had));
}
