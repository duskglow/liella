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
    let (case_a, b_worst, case_c) = case_bounds();
    eprintln!("log quick bound: A 2^{:.3}, B 2^{:.3}, C 2^{:.3}", case_a.log2(), b_worst.log2(), case_c.log2());
    for (v, n) in [(case_a, "A"), (b_worst, "B"), (case_c, "C")] {
        assert!(v <= RHO, "case {n} bound {v:e} above RHO");
    }
    // The multiplier is the least double with C·(1 − 2^54·RHO) > 1 + 2^-53 (exact in Mp).
    let u = two(-53);
    let one_u = Mp::from_f64(1.0).add(Mp::from_f64(u));
    let k = Mp::from_f64(1.0).sub(Mp::from_f64(RHO * two(54)));
    assert!(Mp::from_f64(C).mul(k).cmp_abs(one_u).is_gt(), "C below its bound");
    assert!(Mp::from_f64(f64::from_bits(C.to_bits() - 1)).mul(k).cmp_abs(one_u).is_le(), "C not the least");
}

/// `log`'s case bounds A, B, C (module doc), each term from its formula in `f64` rounded up by
/// `1 + 2^-40`, after checking the table rows they rely on.
fn case_bounds() -> (f64, f64, f64) {
    let grid = two(42);
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
    (case_a, b_worst, case_c)
}

/// `log2`, `log10` and `log1p` (D-10.66, module doc): the constants of the final product and the
/// bounds built on `log`'s cases.
#[test]
fn log_family_constants_and_bounds() {
    for (k, lnb, name) in [(INV_LN2, mp::ln2(), "1/ln 2"), (INV_LN10, mp::ln10(), "1/ln 10")] {
        let [k0, k1, k26, k26l] = k;
        let one = Mp::from_f64(1.0);
        // (k0 + k1)·ln b within 2^-105 of 1, and k26 + k26l within 2^-78·k of k.
        assert!(Mp::from_f64(k0).add(Mp::from_f64(k1)).mul(lnb).sub(one).cmp_abs(Mp::from_f64(two(-105))).is_le(), "{name}: k0 + k1 off");
        assert!(Mp::from_f64(k26).add(Mp::from_f64(k26l)).mul(lnb).sub(one).cmp_abs(Mp::from_f64(two(-78))).is_le(), "{name}: k26 + k26l off");
        assert_eq!(k26.to_bits(), k0.to_bits() & !LOW27, "{name}: k26 is not k0 cut to 26 bits");
        assert!(dd::abs(k26l) <= two(-25) * k0 && dd::abs(k1) <= two(-53) * k0, "{name}: low parts too large");
    }
    let (a, b, c) = case_bounds();
    let log_worst = a.max(b).max(c);
    let up = 1.0 + two(-40);
    // log2, log10: log's bound plus the SSE2 product's 2^-74 (the FMA build's is smaller).
    let logb = (log_worst * (1.0 + two(-74)) + two(-74)) * up;
    // log1p, small |x|: case A with |r| < 2^-8.
    let r = two(-8);
    let u = two(-53);
    let small = (r.powi(9) / 10.0 + 2.3 * u * r * r + 0.34 * u * r * r + two(-87)) / (1.0 - two(-8)) * up;
    // log1p, |x| >= 2^-8: log's cases (relative to ln yh, within 2^-44 of ln(1 + x)) plus the
    // correction's truncation, square and roundings over |ln(1 + x)| >= ln(1 + 2^-8).
    let d = u * (1.0 + two(-7));
    let corr = (d * two(-21) / (1.0 - two(-7)) + two(-106) + two(-104)) / (two(-8).ln_1p()) * up;
    let general = (log_worst * (1.0 + two(-44)) + corr) * up;
    eprintln!("log2/log10 bound 2^{:.3}; log1p small 2^{:.3}, general 2^{:.3}", logb.log2(), small.log2(), general.log2());
    for (v, n) in [(logb, "log2/log10"), (small, "log1p small"), (general, "log1p general")] {
        assert!(v <= RHO, "{n} bound {v:e} above RHO");
    }
}

/// One function of the family: its quick parts, quick answer, tiers and correctly rounded
/// reference, and `ln b` to compare parts in Mp (`ln x = (zh + zl)·ln b`; 0 for `log1p`).
struct Fam {
    name: &'static str,
    parts: fn(f64) -> Option<(f64, f64)>,
    quick: fn(f64) -> Option<f64>,
    tiered: fn(f64) -> Option<f64>,
    cr: fn(f64) -> f64,
    lnb: f64,
}

const FAMS: [Fam; 3] = [
    Fam { name: "log2", parts: log2_quick_parts, quick: log2_quick, tiered: log2_tiered, cr: cr2, lnb: core::f64::consts::LN_2 },
    Fam { name: "log10", parts: log10_quick_parts, quick: log10_quick, tiered: log10_tiered, cr: cr10, lnb: core::f64::consts::LN_10 },
    Fam { name: "log1p", parts: log1p_quick_parts, quick: log1p_quick, tiered: log1p_tiered, cr: cr1p, lnb: 0.0 },
];

fn cr2(x: f64) -> f64 {
    super::super::log2_nearest(x).unwrap_or_else(|| Real::log2(x, Rounding::Nearest).0)
}
fn cr10(x: f64) -> f64 {
    super::super::log10_nearest(x).unwrap_or_else(|| Real::log10(x, Rounding::Nearest).0)
}
fn cr1p(x: f64) -> f64 {
    super::super::log1p_nearest(x).unwrap_or_else(|| Real::log1p(x, Rounding::Nearest).0)
}

/// `true` where the family member's quick phase must answer (module doc).
fn in_domain(f: &Fam, x: f64) -> bool {
    if f.lnb == 0.0 {
        dd::abs(x) >= two(-60) && x > -1.0 && x < two(960)
    } else {
        (f64::MIN_POSITIVE..f64::INFINITY).contains(&x)
    }
}

/// `|f(x) − (zh + zl)|` in Mp, scaled to `ln` for log2/log10, and the matching bound.
fn residual(f: &Fam, x: f64, zh: f64, zl: f64) -> (Mp, Mp) {
    let z = Mp::from_f64(zh).add(Mp::from_f64(zl));
    if f.lnb == 0.0 {
        (mp::ln1p(Mp::from_f64(x)).sub(z), Mp::from_f64(RHO * dd::abs(zh)))
    } else {
        let lnb = if f.name == "log2" { mp::ln2() } else { mp::ln10() };
        (mp::ln(Mp::from_f64(x)).sub(z.mul(lnb)), Mp::from_f64(RHO * dd::abs(zh) * f.lnb * (1.0 - two(-50))))
    }
}

/// Doubles in `(−1, 0)` with uniformly random bits (every binade alike).
fn spread_neg(n: usize, seed: u64) -> impl Iterator<Item = f64> {
    bits(n, seed).map(|b| -f64::from_bits((b >> 1) % 0x3ff0_0000_0000_0000))
}

/// The operands of the family's tests: `log`'s for log2/log10 (`scale` each); for log1p, both
/// sides of `2^-8`, `(−1, 1)`, `[1e-3, 1e3)`, every binade of each sign. The last `2·scale` are
/// the fallback-rate sample.
fn fam_operands(f: &Fam, scale: usize) -> Vec<f64> {
    if f.lnb != 0.0 {
        return operands(scale).collect();
    }
    uniform(scale, 0x2545_f491_4f6c_dd1d, -two(-8), two(-8))
        .chain(uniform(scale, 3, -two(-6), two(-6)))
        .chain(spread_neg(scale / 2, 21).chain(spread(scale / 2, 23)))
        .chain(uniform(scale, 5, -0.99, 1.0))
        .chain(uniform(scale, 7, 1e-3, 1e3))
        .collect()
}

/// Each member's `(zh, zl)` within `RHO·|zh|` of the exact value (Mp) over `5·6·10^4` operands,
/// declining exactly outside its domain; the largest relative error is printed.
#[test]
fn log_family_quick_error_within_bound() {
    for f in &FAMS {
        let mut worst = 0.0f64;
        for x in fam_operands(f, 60_000) {
            let parts = (f.parts)(x);
            assert_eq!(parts.is_some(), in_domain(f, x), "{} quick parts({x:e}) answered outside, or declined inside, its domain", f.name);
            let Some((zh, zl)) = parts else { continue };
            let (err, bound) = residual(f, x, zh, zl);
            assert!(err.cmp_abs(bound).is_le(), "{} quick parts({x:e}): error above the bound", f.name);
            worst = worst.max(dd::abs(err.to_f64()) / bound.to_f64());
        }
        eprintln!("quick {} SSE2: largest error {:.4} of the bound RHO", f.name, worst);
        assert!(worst > 0.0);
    }
}

/// Operands per member of the rounding test: `LIELLA_QUICK_LOG_N` (the 10^7 accuracy run), else
/// 10^6.
fn fam_count() -> usize {
    std::env::var("LIELLA_QUICK_LOG_N").ok().and_then(|s| s.parse::<usize>().ok()).filter(|&k| k >= 10).unwrap_or(1_000_000)
}

/// Every answer of each member's quick phase and tiers against the correctly rounded result, bit
/// for bit, and the fallback rate on the last two groups within the model (`C − 1 = 0.098%`,
/// up to twice that by where the result sits in its binade: asserted 0.03%-0.3%).
#[test]
fn log_family_quick_matches_correctly_rounded() {
    let n = fam_count() / 5;
    for f in &FAMS {
        let xs = fam_operands(f, n);
        let (mut miss, mut answered) = (0usize, 0usize);
        let rate_from = xs.len() - 2 * n;
        for (i, &x) in xs.iter().enumerate() {
            let want = (f.cr)(x);
            match (f.quick)(x) {
                Some(v) => {
                    assert_eq!(v.to_bits(), want.to_bits(), "quick {}({x:e} = {:#x}): {v:e}, correctly rounded {want:e}", f.name, x.to_bits());
                    answered += 1;
                }
                None if i >= rate_from => miss += 1,
                None => {}
            }
            if let Some(v) = (f.tiered)(x) {
                assert_eq!(v.to_bits(), want.to_bits(), "tiered {}({x:e}) misrounded", f.name);
            }
        }
        let m = 2 * n;
        eprintln!("quick {} SSE2: {answered} answered of {}, fallback {miss} of {m}", f.name, xs.len());
        assert!(miss * 10_000 >= m * 3 && miss * 1000 <= m * 3, "{} fallback rate {miss} of {m} outside 0.03%-0.3%", f.name);
    }
}

/// Exact results stay exact and are answered (`log2 2^k = k`, `log10 10^n = n`, `+0` at 1); the
/// hard cases (`10^-n`, `1 ± k·2^-53`, log1p's tiny and boundary operands, and a seeded sample's
/// operands whose unchecked `zh` is wrong, so the test cannot pass vacuously) are declined or
/// answered right, and the tiers answer them right.
#[test]
fn log_family_exact_and_hard_cases() {
    for k in -1022..=1023 {
        assert_eq!(log2_quick(two(k)).map(f64::to_bits), Some(f64::from(k).to_bits()), "log2(2^{k}) not answered exactly");
    }
    let mut p = 1.0f64;
    for n in 0..=22 {
        assert_eq!(log10_quick(p).map(f64::to_bits), Some(f64::from(n).to_bits()), "log10(1e{n}) not answered exactly");
        assert_eq!(log10_tiered(p).map(f64::to_bits), Some(f64::from(n).to_bits()), "tiered log10(1e{n}) not exact");
        p *= 10.0;
    }
    // The tiers' exact paths (flags: module doc): every power of two, and the table holds exactly
    // the 22 powers 10^1..10^22, each in its binade.
    for k in -1022..=1023 {
        assert_eq!(log2_tiered(two(k)).map(f64::to_bits), Some(f64::from(k).to_bits()), "tiered log2(2^{k}) not exact");
    }
    let held: Vec<f64> = POW10_BY_BINADE.iter().filter(|e| e.0 != 0).map(|e| e.1).collect();
    assert_eq!(held, (1..=22).map(f64::from).collect::<Vec<_>>(), "POW10_BY_BINADE does not hold 10^1..10^22");
    for (i, &(b, n)) in POW10_BY_BINADE.iter().enumerate() {
        if b != 0 {
            assert_eq!((b >> 52, f64::from_bits(b)), (1026 + u64::try_from(i).unwrap_or(0), format!("1e{n}").parse::<f64>().unwrap_or(0.0)), "POW10_BY_BINADE[{i}]");
        }
    }
    for x in [-2.0, -0.5, 0.0, -0.0, f64::INFINITY, two(-1074), -10.0, -100.0, 1e23] {
        assert_eq!(log2_tiered(x).map(f64::to_bits), super::super::log2_nearest(x).map(f64::to_bits), "tiered log2({x:e}) must take the tiers");
        assert_eq!(log10_tiered(x).map(f64::to_bits), super::super::log10_nearest(x).map(f64::to_bits), "tiered log10({x:e}) must take the tiers");
    }
    for x in [0.0, -0.0, -1.0, -2.0, two(-61), -two(-61), 5e-324, f64::INFINITY, f64::NAN, two(960)] {
        assert_eq!(log1p_quick_parts(x), None, "quick log1p must decline {x:e}");
    }
    for f in &FAMS {
        let mut hard: Vec<f64> = Vec::new();
        for k in 1..=64u32 {
            let d = f64::from(k) * f64::EPSILON;
            if f.lnb == 0.0 {
                let s = two(-i32::try_from(k).unwrap_or(0));
                hard.extend([d, -d, s, -s, two(-8) + d * two(-8), -two(-8) - d * two(-8), -1.0 + d, two(i32::try_from(k).unwrap_or(0) * 14)]);
            } else {
                hard.extend([1.0 + d, 1.0 - 0.5 * d, 0.1f64.powi(i32::try_from(k % 23).unwrap_or(0))]);
            }
        }
        // Misroundings of the unchecked zh lie where the rounding test declines; look near 1
        // (log2, log10) or below 2^-7 (log1p), where the error is closest to the bound, and
        // over [0.5, 2) and every binade.
        let near = if f.lnb == 0.0 { uniform(1_000_000, 0x2545_f491_4f6c_dd1d, -two(-7), two(-7)) } else { uniform(1_000_000, 0x2545_f491_4f6c_dd1d, 1.0 - two(-8), 1.0 + two(-7)) };
        let (mut wrong, mut misses, mut tier2) = (0usize, 0usize, 0usize);
        for x in near.chain(uniform(500_000, 0x9e37_79b9_7f4a_7c15, 0.5, 2.0)).chain(spread(500_000, 13)) {
            if let Some((zh, zl)) = (f.parts)(x) {
                if zh + zl * C != zh {
                    // A quick miss: the tiers must reach tier 2, which answers most of them.
                    tier2 += usize::from((f.tiered)(x).is_some());
                    misses += 1;
                    if zh != (f.cr)(x) {
                        wrong += 1;
                        hard.push(x);
                    }
                }
            }
        }
        assert!(wrong > 0, "{}: no misrounded unchecked quick result in the sample: the hard-case test is vacuous", f.name);
        assert!(tier2 * 2 > misses, "{}: tier 2 never answered a quick miss ({tier2} of {misses})", f.name);
        for x in hard {
            let want = (f.cr)(x);
            if let Some(v) = (f.quick)(x) {
                assert_eq!(v.to_bits(), want.to_bits(), "quick {}({x:e}) misrounded", f.name);
            }
            assert_eq!((f.tiered)(x).map(f64::to_bits).unwrap_or(want.to_bits()), want.to_bits(), "tiered {}({x:e}) misrounded", f.name);
        }
    }
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
        let parts = log_quick_parts(x);
        assert!(parts.is_some(), "quick log declined {x:e} (positive normal)");
        let (zh, zl) = parts.unwrap_or_default();
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
