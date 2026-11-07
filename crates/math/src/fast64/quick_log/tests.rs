//! The quick `log` phase, SSE2 build (§10.2 log, D-10.3: an answer is the correctly rounded
//! result; D-10.51, D-10.60). The FMA build is checked through the boundary
//! (`crates/boundary/tests/log_quick.rs`), which may call it.

use super::*;
use crate::dd;
use crate::mp::{self, Mp};
use crate::real::Real;
use crate::status::Rounding;

/// xorshift64 stream.
fn bits(n: usize, seed: u64) -> impl Iterator<Item = u64> {
    let mut st = seed;
    (0..n).map(move |_| {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        st
    })
}

/// Uniform in `[lo, hi)`.
fn uniform(n: usize, seed: u64, lo: f64, hi: f64) -> impl Iterator<Item = f64> {
    bits(n, seed).map(move |b| lo + (hi - lo) * (f64::from_bits(0x3ff0_0000_0000_0000 | (b >> 12)) - 1.0))
}

/// Positive normal finite doubles with uniformly random bits (every binade alike).
fn spread(n: usize, seed: u64) -> impl Iterator<Item = f64> {
    bits(n, seed).map(|b| f64::from_bits(0x0010_0000_0000_0000 + (b >> 1) % (0x7ff0_0000_0000_0000 - 0x0010_0000_0000_0000)))
}

/// The correctly rounded `ln x`: today's tier (correctly rounded wherever it answers), else the
/// accurate path.
fn cr(x: f64) -> f64 {
    super::super::log_nearest(x).unwrap_or_else(|| Real::log(x, Rounding::Nearest).0)
}

fn two(e: i32) -> f64 {
    f64::powi(2.0, e)
}

/// Row `j`'s interval `[F_j, F_j + w_j)` (module doc).
fn interval(j: usize) -> (f64, f64) {
    let j = f64::from(u8::try_from(j).unwrap_or(0));
    if j < 76.0 { ((180.0 + j) / 256.0, two(-8)) } else { (1.0 + (j - 76.0) / 128.0, two(-7)) }
}

/// The constants the proof relies on, and the bound recomputed from them (module doc, cases A-C;
/// each term from its formula in `f64`, rounded up by the factor `1 + 2^-40`).
#[test]
fn log_quick_constants_and_bounds() {
    for (c, n) in [(C3, 3.0f64), (C4, -4.0), (C5, 5.0), (C6, -6.0), (C7, 7.0), (C8, -8.0), (C9, 9.0)] {
        assert_eq!(c.to_bits(), (1.0 / n).to_bits(), "C is not RN(1/{n})");
    }
    let grid = two(42);
    let ln2 = mp::ln2();
    assert_eq!((KH * grid).fract(), 0.0, "KH not a multiple of 2^-42");
    assert!(ln2.sub(Mp::from_f64(KH)).sub(Mp::from_f64(KL)).cmp_abs(Mp::from_f64(two(-96))).is_le(), "KH + KL not within 2^-96 of ln 2");
    let up = 1.0 + two(-40);
    let u = two(-53);
    let mut b_worst = 0.0f64;
    let mut a_max = 0.0f64;
    for j in 0..128 {
        let [c, b, lh, ll] = t::LOG_Q128[j].map(f64::from_bits);
        let (f, w) = interval(j);
        assert_eq!(c.to_bits() & ((1 << 45) - 1), 0, "c_{j} has more than 8 significant bits");
        assert!(Mp::from_f64(c).mul(Mp::from_f64(f)).sub(Mp::from_f64(1.0)).sub(Mp::from_f64(b)).to_f64() == 0.0, "b_{j} != c·F − 1");
        assert_eq!((lh * grid).fract(), 0.0, "Lh_{j} not a multiple of 2^-42");
        let l = mp::ln(Mp::from_f64(c));
        assert!(l.add(Mp::from_f64(lh)).add(Mp::from_f64(ll)).cmp_abs(Mp::from_f64(two(-96))).is_le(), "row {j}: −ln c not within 2^-96");
        // |r| over the interval (exact: both ends are products of short doubles).
        let a = dd::abs(c * f - 1.0).max(dd::abs(c * (f + w) - 1.0));
        assert!(a <= two(-7), "row {j}: |r| above 2^-7");
        if j == 75 || j == 76 {
            assert!(c == 1.0 && lh == 0.0 && ll == 0.0, "row {j} next to 1 must have c = 1");
            continue;
        }
        a_max = a_max.max(a);
        assert!(dd::abs(lh) >= 1.01 * a * (1.0 + a), "row {j}: Fast2Sum(th, vh) needs |Lh| >= |vh|");
        // Case B: |ln x| >= min over the interval's ends (it holds no 1).
        let ymin = dd::abs(f.ln()).min(dd::abs((f + w).ln())) / up;
        let abs_err = (a.powi(10) / 10.0 / (1.0 - a) + 2.3 * u * a.powi(3) + u * (0.34 * a.powi(3) + two(-42)) + two(-90)) * up;
        b_worst = b_worst.max(abs_err / ymin);
    }
    assert!(a_max <= two(-7) / 1.3, "|r| off the rows next to 1 above 2^-7.38");
    let r = two(-7);
    let case_a = (r.powi(9) / 10.0 + 2.3 * u * r * r + 0.34 * u * r * r + two(-87)) / (1.0 - two(-8)) * up;
    let case_c = ((a_max.powi(10) / 10.0 / (1.0 - a_max) + 2.3 * u * a_max.powi(3) + u * two(-20)) / 0.341 + two(-94)) * up;
    eprintln!("log quick bound: A 2^{:.3}, B 2^{:.3}, C 2^{:.3}", case_a.log2(), b_worst.log2(), case_c.log2());
    for (v, n) in [(case_a, "A"), (b_worst, "B"), (case_c, "C")] {
        assert!(v <= RHO, "case {n} bound {v:e} above RHO");
    }
    // The multiplier is the least double with C·(1 − 2^54·RHO) > 1 + 2^-53 (exact in Mp).
    let one_u = Mp::from_f64(1.0).add(Mp::from_f64(u));
    let k = Mp::from_f64(1.0).sub(Mp::from_f64(RHO * two(54)));
    assert!(Mp::from_f64(C).mul(k).cmp_abs(one_u).is_gt(), "C below its bound");
    assert!(Mp::from_f64(f64::from_bits(C.to_bits() - 1)).mul(k).cmp_abs(one_u).is_le(), "C not the least");
}

/// The operands of the error and rounding tests: near 1 (cases A, B), [0.5, 2), [1e-3, 1e3) and
/// every binade.
fn operands(scale: usize) -> impl Iterator<Item = f64> {
    uniform(scale, 0x2545_f491_4f6c_dd1d, 1.0 - two(-7), 1.0 + two(-6))
        .chain(uniform(scale, 3, 1.0 - two(-30), 1.0 + two(-30)))
        .chain(uniform(scale, 5, 0.5, 2.0))
        .chain(uniform(scale, 7, 1e-3, 1e3))
        .chain(spread(scale, 9))
}

/// The SSE2 build's `(zh, zl)` within `RHO·|zh|` of `ln x` (exact in Mp) over 3·10^5 operands,
/// and the largest relative error seen (printed; the stated bound is 2^-64).
#[test]
fn log_quick_error_within_bound() {
    let mut worst = 0.0f64;
    for x in operands(60_000) {
        let Some((zh, zl)) = log_quick_parts(x) else { panic!("quick log declined {x:e} (positive normal)") };
        let err = mp::ln(Mp::from_f64(x)).sub(Mp::from_f64(zh)).sub(Mp::from_f64(zl));
        assert!(err.cmp_abs(Mp::from_f64(RHO * dd::abs(zh))).is_le(), "quick log({x:e}): error {:e} above the bound", err.to_f64() / zh);
        worst = worst.max(dd::abs(err.to_f64() / zh));
    }
    eprintln!("quick log SSE2: largest relative error 2^{:.3} (bound 2^{:.3})", worst.log2(), RHO.log2());
    assert!(worst > 0.0);
}

/// Every answer against the correctly rounded `log`, bit for bit, and the fallback rate on
/// uniform operands within the model (`C − 1 = 0.098%`: asserted 0.03%-0.3%).
#[test]
fn log_quick_matches_correctly_rounded_log() {
    let mut miss = 0usize;
    let n = 2_000_000usize;
    for (i, x) in operands(n / 5).enumerate() {
        match log_quick(x) {
            Some(v) => {
                let e = cr(x);
                assert_eq!(v.to_bits(), e.to_bits(), "quick log({x:e} = {:#x}): {v:e}, correctly rounded {e:e}", x.to_bits());
            }
            None if i >= n / 5 * 3 => miss += 1,
            None => {}
        }
    }
    let m = n / 5 * 2;
    eprintln!("quick log SSE2 fallback: {miss} of {m}");
    assert!(miss * 10_000 >= m * 3 && miss * 1000 <= m * 3, "fallback rate {miss} of {m} outside 0.03%-0.3%");
}

/// Outside the domain the quick phase declines; at 1 it returns +0; the tiers answer every
/// positive double as the correctly rounded log; and hard cases (`1 ± k·2^-53`, powers of two,
/// and the operands of a seeded sample whose unchecked quick result `zh` is wrong, so
/// the test cannot pass vacuously) are declined or answered right.
#[test]
fn log_quick_declines_or_rounds_right_on_hard_cases() {
    for x in [0.0, -0.0, -1.0, f64::INFINITY, f64::NAN, f64::MIN_POSITIVE / 2.0, 5e-324, -f64::MIN_POSITIVE] {
        assert_eq!(log_quick_parts(x), None, "quick log must decline {x:e}");
    }
    assert_eq!(log_quick(1.0).map(f64::to_bits), Some(0), "quick log(1) is +0");
    let mut hard: Vec<f64> = Vec::new();
    for k in 1..=64u32 {
        let d = f64::from(k) * f64::EPSILON;
        hard.extend([1.0 + d, 1.0 - 0.5 * d, two(i32::try_from(k).unwrap_or(0) * 16 - 520)]);
    }
    let mut wrong = 0usize;
    // Misroundings of the unchecked `zh` lie where `zl` is next to half an ulp, which is where
    // the rounding test declines: look there (about 0.1% of the sample). Away from 1 the error is
    // far below RHO and a misrounded `zh` is rare (none in 2·10^6 operands on one run), so the
    // sample also covers `c_j = 1` next to 1 (case A), where the error is closest to the bound.
    let near_one = uniform(1_000_000, 0x2545_f491_4f6c_dd1d, 1.0 - two(-8), 1.0 + two(-7));
    for x in uniform(1_000_000, 0x9e37_79b9_7f4a_7c15, 0.5, 2.0).chain(spread(1_000_000, 13)).chain(near_one) {
        if let Some((zh, zl)) = log_quick_parts(x) {
            if zh + zl * C != zh && zh != cr(x) {
                wrong += 1;
                hard.push(x);
            }
        }
    }
    assert!(wrong > 0, "no misrounded unchecked quick result in the sample: the hard-case test is vacuous");
    for x in hard {
        let want = cr(x);
        if let Some(v) = log_quick(x) {
            assert_eq!(v.to_bits(), want.to_bits(), "quick log({x:e}) misrounded");
        }
        assert_eq!(log_tiered(x).map(f64::to_bits).unwrap_or(want.to_bits()), want.to_bits(), "tiered log({x:e}) misrounded");
    }
}
