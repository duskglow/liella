//! The `double` `erf`, `erfc` quick phase (LIBM-F64-SPECIAL-QUICK, D-10.84) through the C entries
//! in both builds (the FMA build needs the startup dispatch). Clause: s10_0_f_0 (§10.2, D-10.3):
//! every value the entries return is the correctly rounded result, so the two builds are
//! bit-identical. `LIELLA_ERF_N` (default 20000) sets the operands per region; the long accuracy
//! job runs it with a larger value.

// Sample generation is bit-level arithmetic on bounded values.
#![allow(clippy::arithmetic_side_effects, clippy::as_conversions)]
// erf, erfc are Liella's own C entries: keep LLVM from treating them as builtins.
#![no_builtins]

use liella_boundary::posix::s10_math_gen as m;
use liella_boundary::selftest;
use liella_math::status::Rounding;

/// Serialises the tests that switch the process-wide build.
static BUILD: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn per_region() -> usize {
    std::env::var("LIELLA_ERF_N").ok().and_then(|v| v.parse().ok()).unwrap_or(20_000)
}

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

/// The quick domain's binades (`2^-5` to 6, both signs), the slot edges around 1 (the bound's
/// worst slots and `erfc`'s `1 − erf` switch), and the hand-over regions (`2^-5`, 6).
fn operands(n: usize) -> Vec<f64> {
    let mut v: Vec<f64> = Vec::new();
    for (k, (lo, hi)) in [(0.03, 0.035), (0.03125, 0.5), (0.5, 1.0), (0.98, 1.02), (1.0, 2.0), (2.0, 6.0), (5.9, 6.1)]
        .into_iter()
        .enumerate()
    {
        v.extend(uniform(n, k as u64 + 1, lo, hi));
    }
    let neg: Vec<f64> = v.iter().map(|x| -x).collect();
    v.extend(neg);
    v
}

/// Both builds: every `erf` and `erfc` entry result equals the accurate path's rounded to
/// nearest. Clause: s10_0_f_0.
#[test]
fn erf_quick_both_builds_round_correctly() {
    let _held = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let had = selftest::math_fma();
    let xs = operands(per_region());
    for (build, fma) in [("SSE2", false), ("FMA", true)] {
        if !selftest::set_math_fma(fma) {
            eprintln!("SKIP {build}: this CPU (or its OS) has no FMA; only the SSE2 build exists here");
            continue;
        }
        for &x in &xs {
            let want = liella_math::special::erf(x, Rounding::Nearest).0;
            assert_eq!(m::erf(x).to_bits(), want.to_bits(), "{build} erf({x:e}) misrounded");
            let want = liella_math::special::erfc(x, Rounding::Nearest).0;
            assert_eq!(m::erfc(x).to_bits(), want.to_bits(), "{build} erfc({x:e}) misrounded");
        }
        eprintln!("{build}: {} operands checked", xs.len());
    }
    selftest::set_math_fma(had);
}
