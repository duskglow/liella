//! The `double` `pow` quick phase (LIBM-F64-POW-QUICK, D-10.51, D-10.62; `pow`'s tier 1) in its
//! FMA build through the boundary (the SSE2 build's own tests are in `liella-math`'s
//! `fast64/quick_pow/tests.rs`), and the `pow` entry over the tiers. Clause: s10_0_f_0 (§10.2
//! pow, D-10.3): every value the quick phase returns is the correctly rounded `x^y`, so its two
//! builds are bit-identical wherever both answer. `LIELLA_POW_SWEEP=<n>` sets the entry sweep's
//! operand count per set (default 50000; the 10^8 job passes 25000000).

// Sample generation is bit-level arithmetic on bounded values.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing, clippy::as_conversions)]
// pow is Liella's own C entry (m::pow): keep LLVM from treating it as a builtin.
#![no_builtins]

use liella_boundary::posix::s10_math_gen as m;
use liella_boundary::selftest;
use liella_math::real::Real;
use liella_math::status::Rounding;

/// Serialises the tests that switch the process-wide build.
static BUILD: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// xorshift64 over `[lo, hi)`.
fn uniform(n: usize, seed: u64, lo: f64, hi: f64) -> impl Iterator<Item = f64> {
    let mut st = seed;
    (0..n).map(move |_| {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        lo + (hi - lo) * (f64::from_bits(0x3ff0_0000_0000_0000 | (st >> 12)) - 1.0)
    })
}

/// The cbench box, x near 1 with large y, results near overflow and near underflow.
fn operands(n: usize, seed: u64) -> impl Iterator<Item = (f64, f64)> {
    let box_ = uniform(n, seed, 0.01, 10.0).zip(uniform(n, seed + 1, -10.0, 10.0));
    let near1 = uniform(n, seed + 2, 1.0 - 1.0 / 256.0, 1.0 + 1.0 / 128.0).zip(uniform(n, seed + 3, -3000.0, 3000.0));
    let big = uniform(n, seed + 4, 1.5, 10.0).zip(uniform(n, seed + 5, 0.0, 1e-3)).map(|(x, d)| (x, 707.0 / x.ln() * (1.0 - d)));
    let tiny = uniform(n, seed + 6, 1.5, 10.0).zip(uniform(n, seed + 7, 0.0, 1e-3)).map(|(x, d)| (x, -707.0 / x.ln() * (1.0 - d)));
    box_.chain(near1).chain(big).chain(tiny)
}

/// Runs `f` with the FMA build selected, or `None` (printed) on a CPU without FMA.
fn with_fma<T>(f: impl FnOnce() -> T) -> Option<T> {
    let _held = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !selftest::set_math_fma(true) {
        eprintln!("SKIP: this CPU has no FMA (or the OS does not save the AVX state)");
        return None;
    }
    assert!(selftest::math_fma(), "set_math_fma(true) did not select the FMA build");
    let out = f();
    assert!(selftest::set_math_fma(false));
    Some(out)
}

/// The FMA build's quick answers against the correctly rounded `pow` (the accurate path), bit for
/// bit, and its fallback on the cbench box (model `c − 1 ≈ 0.008%`, asserted at most 0.1%).
/// Clause: s10_0_f_0.
#[test]
fn pow_quick_fma_matches_correctly_rounded_pow() {
    let n = 50_000usize;
    let Some(miss) = with_fma(|| {
        let mut miss = 0usize;
        for (i, (x, y)) in operands(n, 0x2545_f491_4f6c_dd1d).enumerate() {
            match selftest::pow_quick(x, y) {
                Some(v) => {
                    let e = Real::pow(x, y, Rounding::Nearest).0;
                    assert_eq!(v.to_bits(), e.to_bits(), "FMA quick pow({x:e}, {y:e}): {v:e}, correctly rounded {e:e}");
                }
                None if i < n => miss += 1,
                None => {}
            }
        }
        miss
    }) else {
        return;
    };
    eprintln!("quick pow FMA: fallback {miss} of {n} on the cbench box");
    assert!(miss * 1000 <= n, "FMA fallback {miss} of {n} above 0.1%");
}

/// The `pow` entry (both builds; quick phase, tier 2, accurate path) against the correctly rounded
/// `pow`, bit for bit, over `4·LIELLA_POW_SWEEP` operands per build; exact cases keep their
/// exact results, and subnormal `x` is rounded correctly. Clause: s10_0_f_0.
#[test]
fn pow_entry_tiers_round_correctly() {
    let n = std::env::var("LIELLA_POW_SWEEP").ok().and_then(|s| s.parse().ok()).unwrap_or(50_000usize);
    let check = |build: &str| {
        for (x, y) in operands(n, 0x9e37_79b9_7f4a_7c15) {
            let e = Real::pow(x, y, Rounding::Nearest).0;
            assert_eq!(m::pow(x, y).to_bits(), e.to_bits(), "{build} pow({x:e}, {y:e}) entry misrounded");
        }
        for (x, y, want) in [(4.0, 0.5, 2.0), (4.0, 1.5, 8.0), (16.0, 0.25, 2.0), (3.0, 2.0, 9.0), (1.0 + f64::EPSILON, 2.0, 1.0 + 2.0 * f64::EPSILON)] {
            assert_eq!(m::pow(x, y).to_bits(), f64::to_bits(want), "{build} pow({x:e}, {y:e}) exact case");
        }
        // Subnormal x: outside the quick log's normal-x reduction, so the domain check must
        // route them past the quick phase.
        for (x, y) in [(5e-324, 0.3), (2.0e-310, 0.75), (2.0e-310, -0.01)] {
            let e = Real::pow(x, y, Rounding::Nearest).0;
            assert_eq!(m::pow(x, y).to_bits(), e.to_bits(), "{build} pow({x:e}, {y:e}) subnormal x misrounded");
        }
    };
    {
        let _held = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let had = selftest::math_fma();
        assert!(selftest::set_math_fma(false));
        check("SSE2");
        assert!(selftest::set_math_fma(had));
    }
    with_fma(|| check("FMA"));
}
