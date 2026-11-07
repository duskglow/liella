//! The `double` `log` quick phase (LIBM-F64-LOG-QUICK, D-10.51, D-10.60; `log`'s tier 1) in its
//! FMA build through the boundary (the FMA build needs the startup dispatch, so its tests live
//! here; the SSE2 build's own tests are in `liella-math`'s `fast64/quick_log/tests.rs`), and the
//! `log` entry over the tiers. Clause: s10_0_f_0 (§10.2 log, D-10.3): every value the quick phase
//! returns is the correctly rounded `ln x`, so its two builds are bit-identical wherever both answer.

// Sample generation is bit-level arithmetic on bounded values.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing, clippy::as_conversions)]
// log is Liella's own C entry (m::log): keep LLVM from treating it as a builtin.
#![no_builtins]

use liella_boundary::posix::s10_math_gen as m;
use liella_boundary::selftest;
use liella_math::fast64::quick_log::RHO;
use liella_math::mp::{self, Mp};
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

/// Positive normal finite doubles with uniformly random bits.
fn spread(n: usize, seed: u64) -> impl Iterator<Item = f64> {
    let mut st = seed;
    (0..n).map(move |_| {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        f64::from_bits(0x0010_0000_0000_0000 + (st >> 1) % (0x7ff0_0000_0000_0000 - 0x0010_0000_0000_0000))
    })
}

/// The correctly rounded `ln x` from the accurate path alone.
fn cr(x: f64) -> f64 {
    Real::log(x, Rounding::Nearest).0
}

/// Run `f` with the FMA build forced, then restore the baseline; `None` without FMA.
fn with_fma<T>(f: impl FnOnce() -> T) -> Option<T> {
    let _held = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !selftest::set_math_fma(true) {
        eprintln!("SKIP: this CPU (or its OS) has no FMA; only the SSE2 build exists here");
        return None;
    }
    assert!(selftest::math_fma(), "set_math_fma(true) did not select the FMA build");
    let v = f();
    assert!(selftest::set_math_fma(false));
    Some(v)
}

fn operands(scale: usize) -> impl Iterator<Item = f64> {
    uniform(scale, 0x2545_f491_4f6c_dd1d, 1.0 - 2f64.powi(-7), 1.0 + 2f64.powi(-6))
        .chain(uniform(scale, 3, 1.0 - 2f64.powi(-30), 1.0 + 2f64.powi(-30)))
        .chain(uniform(scale, 5, 0.5, 2.0))
        .chain(uniform(scale, 7, 1e-3, 1e3))
        .chain(spread(scale, 9))
}

/// The FMA build's `(zh, zl)` within `RHO·|zh|` of `ln x` (exact in Mp).
#[test]
fn log_quick_fma_error_within_bound() {
    let worst = with_fma(|| {
        let mut worst = 0.0f64;
        for x in operands(20_000) {
            let (zh, zl) = selftest::log_quick_parts(x).unwrap_or_else(|| panic!("FMA quick log declined {x:e}"));
            let err = mp::ln(Mp::from_f64(x)).sub(Mp::from_f64(zh)).sub(Mp::from_f64(zl));
            assert!(err.cmp_abs(Mp::from_f64(RHO * zh.abs())).is_le(), "FMA quick log({x:e}): error {:e} above the bound", err.to_f64() / zh);
            worst = worst.max((err.to_f64() / zh).abs());
        }
        worst
    });
    if let Some(w) = worst {
        eprintln!("quick log FMA: largest relative error 2^{:.3} (bound 2^{:.3})", w.log2(), RHO.log2());
    }
}

/// Every answer of the selected build's quick phase and tiers is the correctly rounded `ln x`;
/// returns the quick phase's misses.
fn check(build: &str, xs: impl Iterator<Item = f64>) -> usize {
    let mut miss = 0;
    for x in xs {
        let want = cr(x).to_bits();
        match selftest::log_quick(x) {
            Some(v) => assert_eq!(v.to_bits(), want, "{build} quick log({x:e} = {:#x}) misrounded", x.to_bits()),
            None => miss += 1,
        }
        if let Some(v) = selftest::log_tiered(x) {
            assert_eq!(v.to_bits(), want, "{build} tiered log({x:e}) misrounded");
        }
        assert_eq!(m::log(x).to_bits(), want, "{build} log({x:e}) entry misrounded");
    }
    miss
}

/// Both builds through the entry: every answer correctly rounded; the FMA build's fallback rate
/// on 2·10^5 operands spread over the binades is below 0.3% (model `C − 1 = 0.098%`).
#[test]
fn log_quick_both_builds_round_correctly() {
    {
        let _held = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let had = selftest::math_fma();
        assert!(selftest::set_math_fma(false));
        check("SSE2", operands(40_000));
        assert!(selftest::set_math_fma(had));
    }
    if let Some(miss) = with_fma(|| check("FMA", operands(40_000))) {
        eprintln!("quick log FMA: {miss} misses of 200000");
        assert!(miss * 1000 <= 200_000 * 3, "FMA fallback {miss} of 200000 above 0.3%");
    }
}
