//! The `double` `exp` quick phase (LIBM-F64-EXP-QUICK, D-10.51, D-10.54; `exp`'s tier 1 since LIBM-F64-EXP-QUICK-WIRE)
//! in both builds through the boundary (the FMA build needs the startup dispatch, so its tests live
//! here; the SSE2 build's own tests are in `liella-math`'s `fast64/quick/tests.rs`). Clause:
//! s10_0_f_0 (§10.2 exp, D-10.3): every value the quick phase returns is the correctly rounded
//! `e^x`, so its two builds are bit-identical wherever both answer.

// Sample generation is bit-level arithmetic on bounded values.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing, clippy::as_conversions)]
// exp is Liella's own C entry (m::exp): keep LLVM from treating it as a builtin.
#![no_builtins]

use liella_boundary::posix::s10_math_gen as m;
use liella_boundary::selftest;
use liella_math::fast64::quick::{RHO_FMA, RHO_SSE2};
use liella_math::mp::{self, Mp};
use liella_math::real::Real;
use liella_math::status::Rounding;

/// xorshift64 over `[lo, hi)`: a fixed, reproducible sample (the cbench operand generator's step).
fn uniform(n: usize, seed: u64, lo: f64, hi: f64) -> impl Iterator<Item = f64> {
    let mut st = seed;
    (0..n).map(move |_| {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        lo + (hi - lo) * (f64::from_bits(0x3ff0_0000_0000_0000 | (st >> 12)) - 1.0)
    })
}

/// Uniform bit patterns over the quick domain `2^-60 <= |x| < 708`, both signs.
fn uniform_bits(n: usize, seed: u64) -> impl Iterator<Item = f64> {
    let lo = f64::powi(2.0, -60).to_bits();
    let span = 708f64.to_bits() - lo;
    let mut st = seed;
    (0..n).map(move |_| {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        let x = f64::from_bits(lo + (st >> 1) % span);
        if st & 1 == 0 { x } else { -x }
    })
}

/// The correctly rounded `e^x` (tier 2, else the accurate path).
fn cr(x: f64) -> f64 {
    liella_math::fast64::exp_nearest(x).unwrap_or_else(|| Real::exp(x, Rounding::Nearest).0)
}

/// Held while a test needs one build to stay selected (the choice is process-wide).
static BUILD: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The wired entry (LIBM-F64-EXP-QUICK-WIRE, D-10.51) in the SSE2 build: `m::exp` on the operands
/// of a seeded sample whose quick result, rounded without the test, is wrong (found here, so the
/// test cannot pass vacuously) must return the correctly rounded `e^x` (tier 2 or the accurate
/// path decides them).
// s10_0_f_0: exp through its entry is correctly rounded where the quick phase would misround.
#[test]
fn exp_entry_rounds_quick_misroundings_right() {
    let _held = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let had = selftest::math_fma();
    assert!(selftest::set_math_fma(false));
    let mut wrong = 0usize;
    for x in uniform(400_000, 0x9e37_79b9_7f4a_7c15, -708.0, 708.0) {
        let Some((zh, zl, sc)) = selftest::exp_quick_parts(x) else { continue };
        let e = cr(x);
        if (zh + zl) * sc != e {
            wrong += 1;
            let v = m::exp(x);
            assert_eq!(v.to_bits(), e.to_bits(), "hard case exp({x:e} = {:#x}) through the entry: {v:e}, correctly rounded {e:e}", x.to_bits());
        }
    }
    assert!(selftest::set_math_fma(had));
    assert!(wrong >= 20, "only {wrong} raw misroundings found: the hard set is too small");
}

/// Run `f` with the FMA build forced, then restore the startup decision; `None` without FMA.
fn with_fma<T>(f: impl FnOnce() -> T) -> Option<T> {
    let _held = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !selftest::set_math_fma(true) {
        eprintln!("SKIP: this CPU (or its OS) has no FMA; only the SSE2 build exists here");
        return None;
    }
    assert!(selftest::math_fma(), "set_math_fma(true) did not select the FMA build");
    let v = f();
    assert!(selftest::set_math_fma(true));
    Some(v)
}

/// Check every answer of the selected build against the correctly rounded `e^x`; returns the
/// number of declines.
fn check(build: &str, xs: impl Iterator<Item = f64>) -> usize {
    let mut miss = 0usize;
    for x in xs {
        match selftest::exp_quick(x) {
            Some(v) => {
                let e = cr(x);
                assert_eq!(v.to_bits(), e.to_bits(), "{build} quick exp({x:e} = {:#x}): {v:e}, correctly rounded {e:e}", x.to_bits());
            }
            None => miss += 1,
        }
    }
    miss
}

// s10_0_f_0: the FMA build returns only correctly rounded values (so the same as the SSE2 build
// where both answer), and declines under 0.05% of the cbench operands (bound 2^-67.68, D-10.54).
#[test]
fn exp_quick_fma_build_rounds_correctly() {
    let sets: [(usize, u64, f64, f64); 4] = [(4_000_000, 1, -10.0, 10.0), (4_000_000, 2, -708.0, 708.0), (1_000_000, 3, -0.01, 0.01), (1_000_000, 4, -745.2, 709.8)];
    let Some(()) = with_fma(|| {
        for (n, seed, lo, hi) in sets {
            let miss = check("FMA", uniform(n, seed, lo, hi));
            if seed == 1 {
                eprintln!("quick exp FMA: fallback {miss} of {n} on uniform [-10, 10]");
                assert!(miss * 2000 < n, "FMA fallback rate {miss} of {n} not under 0.05%");
            }
        }
    }) else {
        return;
    };
    // Both builds on one sample: wherever both answer, the bits agree.
    let xs: Vec<f64> = uniform(200_000, 5, -708.0, 708.0).collect();
    let fused: Vec<Option<f64>> = with_fma(|| xs.iter().map(|&x| selftest::exp_quick(x)).collect()).unwrap_or_default();
    assert!(selftest::set_math_fma(false));
    let mut both = 0usize;
    for (i, &x) in xs.iter().enumerate() {
        if let (Some(a), Some(b)) = (fused[i], selftest::exp_quick(x)) {
            assert_eq!(a.to_bits(), b.to_bits(), "exp({x:e}): FMA build {a:e}, SSE2 build {b:e}");
            both += 1;
        }
    }
    assert!(selftest::set_math_fma(true));
    assert!(both > 190_000, "only {both} operands answered by both builds");
}

// s10_0_f_0: the FMA build's unrounded result is within its stated bound RHO_FMA (D-10.54).
#[test]
fn exp_quick_fma_error_within_bound() {
    let worst = with_fma(|| {
        let mut worst = 0.0f64;
        let xs = uniform(20_000, 0x2545_f491_4f6c_dd1d, -708.0, 708.0).chain(uniform(20_000, 7, -1.0, 1.0)).chain(uniform(4000, 11, -1e-9, 1e-9));
        for x in xs {
            let Some((zh, zl, sc)) = selftest::exp_quick_parts(x) else { continue };
            let want = mp::exp(Mp::from_f64(x)).mul(Mp::from_f64(1.0 / sc));
            let err = want.sub(Mp::from_f64(zh)).sub(Mp::from_f64(zl));
            assert!(err.cmp_abs(Mp::from_f64(RHO_FMA * zh.abs())).is_le(), "FMA quick exp({x:e}): error {:e} above the bound", err.to_f64() / zh);
            worst = worst.max((err.to_f64() / zh).abs());
        }
        worst
    });
    let Some(worst) = worst else { return };
    eprintln!("quick exp FMA: largest relative error 2^{:.3} (bound 2^{:.3})", worst.log2(), RHO_FMA.log2());
    assert!(worst > 0.0);
}

/// The accuracy job (LIBM-F64-EXP-QUICK, brief "10^8 per distribution"): `EXP_QUICK_N` operands
/// (default 10^8) per distribution and build, over the threads available. Ignored in the gate.
// s10_0_f_0: 0 mismatches against the correctly rounded exp in both builds.
#[test]
#[ignore = "accuracy job: run with --ignored (EXP_QUICK_N operands per distribution and build)"]
fn exp_quick_accuracy_job() {
    let n: usize = std::env::var("EXP_QUICK_N").ok().and_then(|s| s.parse().ok()).unwrap_or(100_000_000);
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let per = n.div_ceil(threads);
    let mut builds = vec![false];
    if selftest::set_math_fma(true) {
        builds.push(true);
    } else {
        eprintln!("SKIP FMA build: this CPU (or its OS) has no FMA");
    }
    for fma in builds {
        assert!(selftest::set_math_fma(fma));
        let name = if fma { "FMA" } else { "SSE2" };
        for dist in 0..3u64 {
            let miss: usize = std::thread::scope(|s| {
                let hs: Vec<_> = (0..threads as u64)
                    .map(|t| {
                        s.spawn(move || {
                            let seed = 0x9e37_79b9_7f4a_7c15 ^ (dist << 40) ^ (t + 1);
                            match dist {
                                0 => check(name, uniform(per, seed, -10.0, 10.0)),
                                1 => check(name, uniform_bits(per, seed)),
                                _ => check(name, uniform(per, seed, -745.2, 709.8)),
                            }
                        })
                    })
                    .collect();
                hs.into_iter().map(|h| h.join().unwrap_or(usize::MAX)).sum()
            });
            let total = per * threads;
            let label = ["uniform [-10, 10]", "uniform bits over the domain", "uniform [-745.2, 709.8]"][dist as usize];
            eprintln!("quick exp {name}: {label}: {total} operands, 0 mismatches, fallback {miss} ({:.4}%)", miss as f64 * 100.0 / total as f64);
        }
    }
    assert!(selftest::set_math_fma(true) || selftest::set_math_fma(false));
    eprintln!("bounds: FMA 2^{:.3}, SSE2 2^{:.3}", RHO_FMA.log2(), RHO_SSE2.log2());
}

/// The count loop (instructions per call by `perf stat -e instructions:u`, recipe in the
/// handoff): `EXP_QUICK_CALLS` calls over 4096 operands uniform in [-10, 10] (the cbench exp range),
/// `EXP_QUICK_BUILD` = `fma`, `sse2` or `none` (the loop alone, subtracted); a decline falls back to
/// `exp` itself, so the count includes the fallback rate. Ignored in the gate.
// no spec clause (measurement loop).
#[test]
#[ignore = "count loop: run with --ignored under perf stat"]
fn exp_quick_count_loop() {
    let calls: usize = std::env::var("EXP_QUICK_CALLS").ok().and_then(|s| s.parse().ok()).unwrap_or(0);
    let build = std::env::var("EXP_QUICK_BUILD").unwrap_or_default();
    let xs: Vec<f64> = uniform(4096, 0x9e37_79b9_7f4a_7c15, -10.0, 10.0).collect();
    match build.as_str() {
        "fma" => assert!(selftest::set_math_fma(true), "no FMA here"),
        "sse2" => assert!(selftest::set_math_fma(false)),
        _ => {}
    }
    // Decided once, outside the loop, so every mode's loop costs the same apart from the call.
    let none = build == "none";
    let mut acc = 0.0f64;
    let mut miss = 0usize;
    for i in 0..calls {
        let x = core::hint::black_box(xs[i & 4095]);
        acc += if none {
            x
        } else {
            selftest::exp_quick(x).unwrap_or_else(|| {
                miss += 1;
                m::exp(x)
            })
        };
    }
    core::hint::black_box(acc);
    eprintln!("count loop: build {build}, {calls} calls, {miss} fallbacks");
}
