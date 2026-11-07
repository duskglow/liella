//! The `double` sin, cos, tan, sincos quick phase (LIBM-F64-TRIG-QUICK, D-10.51, D-10.61; their
//! tier 1) in both builds through the boundary (the FMA build needs the startup dispatch, so its
//! tests live here; the SSE2 build's own tests are in `liella-math`'s `trig/quick/tests.rs`).
//! Clauses: s10_0_f_0 (D-10.3: every value the quick phase returns is the correctly rounded
//! result, so its two builds are bit-identical wherever both answer) and s10_3_c_r4 (exact
//! reduction for huge arguments: the integer Payne-Hanek path).

// Sample generation is bit-level arithmetic on bounded values.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing, clippy::as_conversions)]

use liella_boundary::selftest;
use liella_math::status::Rounding;

/// xorshift64 states from `seed`.
fn bits(n: usize, seed: u64) -> impl Iterator<Item = u64> {
    let mut st = seed;
    (0..n).map(move |_| {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        st
    })
}

/// Uniform over `[lo, hi)`.
fn uniform(n: usize, seed: u64, lo: f64, hi: f64) -> impl Iterator<Item = f64> {
    bits(n, seed).map(move |b| lo + (hi - lo) * (f64::from_bits(0x3ff0_0000_0000_0000 | (b >> 12)) - 1.0))
}

/// Uniform bit patterns over `[2^-30, 2^1024)`, either sign: every reduction path, mostly the
/// huge one (Payne-Hanek, `|x| >= 2^20`).
fn uniform_bits(n: usize, seed: u64) -> impl Iterator<Item = f64> {
    let lo = f64::powi(2.0, -30).to_bits();
    let span = f64::MAX.to_bits() + 1 - lo;
    bits(n, seed).map(move |b| {
        let x = f64::from_bits(lo + (b >> 1) % span);
        if b & 1 == 0 { x } else { -x }
    })
}

const NAMES: [&str; 3] = ["sin", "cos", "tan"];

/// The correctly rounded result (today's tier, else the accurate path).
fn cr(which: u8, x: f64) -> f64 {
    match which {
        0 => liella_math::trig::sin(x, Rounding::Nearest).0,
        1 => liella_math::trig::cos(x, Rounding::Nearest).0,
        _ => liella_math::trig::tan(x, Rounding::Nearest).0,
    }
}

/// Held while a test needs one build to stay selected (the choice is process-wide).
static BUILD: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Run `f` with the build `fma` forced, then restore the startup decision; `None` when FMA is
/// asked for and absent.
fn with_build<T>(fma: bool, f: impl FnOnce() -> T) -> Option<T> {
    let _held = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let had = selftest::math_fma();
    if !selftest::set_math_fma(fma) {
        eprintln!("SKIP: this CPU (or its OS) has no FMA; only the SSE2 build exists here");
        return None;
    }
    assert_eq!(selftest::math_fma(), fma, "set_math_fma({fma}) did not select that build");
    let v = f();
    assert!(selftest::set_math_fma(had));
    Some(v)
}

/// Check every answer of the selected build against the correctly rounded result; returns the
/// number of declines.
fn check(build: &str, which: u8, xs: impl Iterator<Item = f64>) -> usize {
    let mut miss = 0usize;
    for x in xs {
        match selftest::trig_quick(which, x) {
            Some(v) => {
                let e = cr(which, x);
                assert_eq!(v.to_bits(), e.to_bits(), "{build} quick {}({x:e} = {:#x}): {v:e}, correctly rounded {e:e}", NAMES[which as usize], x.to_bits());
            }
            None => miss += 1,
        }
    }
    miss
}

// s10_0_f_0, s10_3_c_r4: the FMA build returns only correctly rounded values, on the cbench
// range, a wide range and the huge (Payne-Hanek) range, and answers at least 97.5% of
// uniform [-10, 10] (the fallback stays the cold path); sincos returns sin's and cos's answers.
#[test]
fn trig_quick_fma_build_rounds_correctly() {
    let Some(()) = with_build(true, || {
        for which in 0..3u8 {
            let n = 1_000_000;
            let miss = check("FMA", which, uniform(n, 1 + u64::from(which), -10.0, 10.0));
            eprintln!("quick {} FMA: fallback {miss} of {n} on uniform [-10, 10]", NAMES[which as usize]);
            assert!(miss * 40 < n, "FMA {} fallback {miss} of {n} not under 2.5%", NAMES[which as usize]);
            check("FMA", which, uniform(200_000, 11, -1e6, 1e6));
            check("FMA", which, uniform_bits(200_000, 13 + u64::from(which)));
        }
        let mut both = 0usize;
        for x in uniform(200_000, 17, -10.0, 10.0).chain(uniform_bits(50_000, 19)) {
            if let Some((s, c)) = selftest::sincos_quick(x) {
                both += 1;
                assert_eq!((Some(s.to_bits()), Some(c.to_bits())), (selftest::trig_quick(0, x).map(f64::to_bits), selftest::trig_quick(1, x).map(f64::to_bits)), "FMA quick sincos({x:e} = {:#x})", x.to_bits());
            }
        }
        assert!(both > 200_000, "only {both} sincos answers");
    }) else {
        return;
    };
}

// s10_0_f_0: both builds on one sample: wherever both answer, the bits agree (D-10.50: a build
// never changes a result).
#[test]
fn trig_quick_builds_agree() {
    let xs: Vec<f64> = uniform(100_000, 23, -10.0, 10.0).chain(uniform_bits(100_000, 29)).collect();
    for which in 0..3u8 {
        let run = |fma| with_build(fma, || xs.iter().map(|&x| selftest::trig_quick(which, x)).collect::<Vec<_>>());
        let (Some(plain), Some(fused)) = (run(false), run(true)) else { return };
        let mut both = 0usize;
        for (i, &x) in xs.iter().enumerate() {
            if let (Some(a), Some(b)) = (plain[i], fused[i]) {
                both += 1;
                assert_eq!(a.to_bits(), b.to_bits(), "{}({x:e} = {:#x}): SSE2 build {a:e}, FMA build {b:e}", NAMES[which as usize], x.to_bits());
            }
        }
        assert!(both > 190_000, "only {both} operands answered by both builds");
    }
}

/// The accuracy job (LIBM-F64-TRIG-QUICK, brief "10^8 operands"): `TRIG_QUICK_N` operands
/// (default 10^8) per function, distribution and build, over the threads available. Ignored in
/// the gate.
// s10_0_f_0, s10_3_c_r4: 0 mismatches against the correctly rounded sin, cos, tan in both builds.
#[test]
#[ignore = "accuracy job: run with --ignored (TRIG_QUICK_N operands per function, distribution and build)"]
fn trig_quick_accuracy_job() {
    let n: usize = std::env::var("TRIG_QUICK_N").ok().and_then(|s| s.parse().ok()).unwrap_or(100_000_000);
    let threads = std::thread::available_parallelism().map_or(4, usize::from);
    let per = n.div_ceil(threads);
    for fma in [false, true] {
        let name = if fma { "FMA" } else { "SSE2" };
        let Some(()) = with_build(fma, || {
            for which in 0..3u8 {
                for dist in 0..3u64 {
                    let miss: usize = std::thread::scope(|s| {
                        let hs: Vec<_> = (0..threads as u64)
                            .map(|t| {
                                s.spawn(move || {
                                    let seed = 0x9e37_79b9_7f4a_7c15 ^ (dist << 40) ^ (u64::from(which) << 48) ^ (t + 1);
                                    match dist {
                                        0 => check(name, which, uniform(per, seed, -10.0, 10.0)),
                                        1 => check(name, which, uniform(per, seed, -1048576.0, 1048576.0)),
                                        _ => check(name, which, uniform_bits(per, seed)),
                                    }
                                })
                            })
                            .collect();
                        hs.into_iter().map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e))).sum()
                    });
                    let total = per * threads;
                    let label = ["uniform [-10, 10]", "uniform [-2^20, 2^20]", "uniform bits over [2^-30, 2^1024)"][dist as usize];
                    eprintln!("quick {} {name}: {label}: {total} operands, 0 mismatches, fallback {miss} ({:.4}%)", NAMES[which as usize], miss as f64 * 100.0 / total as f64);
                }
            }
        }) else {
            continue;
        };
    }
}
