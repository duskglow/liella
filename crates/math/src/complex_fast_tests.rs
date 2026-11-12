//! The `cexp`/`cexpf` quick tier against the 384-bit route and against a 384-bit reference of
//! its bounds (Clauses: s10_5_c_0, s10_0_f_0: where the tier answers, it is the route's result
//! bit for bit, in every mode).

use super::*;
use crate::complex::cexp_route;
use crate::mp::{self, Mp};

const MODES: [Rounding; 4] = [Rounding::Nearest, Rounding::Down, Rounding::Up, Rounding::Zero];

/// xorshift64 uniform in `[lo, hi)`.
fn uniform(st: &mut u64, lo: f64, hi: f64) -> f64 {
    *st ^= *st << 13;
    *st ^= *st >> 7;
    *st ^= *st << 17;
    lo + (hi - lo) * (f64::from_bits(0x3ff0_0000_0000_0000 | (*st >> 12)) - 1.0)
}

/// `y` within a few ulp of `π/2 + kπ` (cos y tiny), `k < 2^18`.
fn near_half_pi(st: &mut u64) -> f64 {
    let k = uniform(st, 0.0, 262_144.0).floor();
    let base = (k + 0.5) * core::f64::consts::PI;
    // Half within 8 ulp (|cos y| near 2^-53·|y|: the absolute reduction bound makes the tier
    // decline, the route answers), half within 2^24 ulp (|cos y| down to about 2^-29·|y|).
    let w = if uniform(st, 0.0, 1.0) < 0.5 { 8.0 } else { 16_777_216.0 };
    let j = uniform(st, -w, w) as i64;
    f64::from_bits((base.to_bits() as i64 + j) as u64)
}

/// The regimes: general, cos y tiny, large |y| (beyond 2^20: the tier declines), x near the
/// overflow and the underflow thresholds.
fn operand(st: &mut u64, regime: usize) -> (f64, f64) {
    match regime {
        0 => (uniform(st, -30.0, 30.0), uniform(st, -12.0, 12.0)),
        1 => (uniform(st, -5.0, 5.0), near_half_pi(st)),
        2 => (uniform(st, -2.0, 2.0), uniform(st, -1.0e7, 1.0e7)),
        3 => (uniform(st, 700.0, 750.0), uniform(st, -4.0, 4.0)),
        _ => (uniform(st, -750.0, -700.0), uniform(st, -4.0, 4.0)),
    }
}

/// `n` operands per regime, all four modes, both types: every answer of the tier equals the
/// route's (value bits and status), the in-place entries agree with mode Nearest; returns how
/// many `double` operands (nearest) the tier answered in each regime.
/// Whether the quick rounding test (to nearest) declines a part of an operand inside the tier.
fn quick_declines(x: f64, y: f64) -> bool {
    parts(x, y).is_some_and(|([(re, er), (im, ei)], m)| round(re, er, m, Rounding::Nearest).is_none() || round(im, ei, m, Rounding::Nearest).is_none())
}

fn compare(n: usize, seed: u64) -> [usize; 5] {
    let mut st = seed;
    let mut hits = [0usize; 5];
    for (regime, hit) in hits.iter_mut().enumerate() {
        for _ in 0..n {
            let (x, y) = operand(&mut st, regime);
            for mode in MODES {
                if let Some(r) = cexp(x, y, mode) {
                    let w = cexp_route(x, y, mode);
                    assert_eq!((x, y, mode, r.0 .0.to_bits(), r.0 .1.to_bits(), r.1), (x, y, mode, w.0 .0.to_bits(), w.0 .1.to_bits(), w.1), "cexp quick tier differs");
                    if mode == Rounding::Nearest {
                        *hit += 1;
                        // In place: the same bits, or a decline where the quick test declines
                        // a part (the middle tier runs on the cold side, through `cexp`).
                        match cexp_nearest(x, y) {
                            Some((a, b)) => assert_eq!((a.to_bits(), b.to_bits()), (r.0 .0.to_bits(), r.0 .1.to_bits()), "cexp in place differs"),
                            None => {
                                assert!(quick_declines(x, y), "cexp in place declined ({x:e}, {y:e}) although the quick test decides both parts");
                                let c = cexp_nearest_middle(x, y).map(|(a, b)| (a.to_bits(), b.to_bits()));
                                assert_eq!(c, Some((r.0 .0.to_bits(), r.0 .1.to_bits())), "cexp's cold in-place attempt differs");
                            }
                        }
                    }
                }
                // float: the threshold regimes scaled by 1/8 (x in ±[87.5, 93.75), across
                // FLT_MAX's and FLT_MIN's exponents).
                let (xf, yf) = if regime >= 3 { ((x / 8.0) as f32, y as f32) } else { (x as f32, y as f32) };
                if let Some(r) = fast32::cexpf(xf, yf, mode) {
                    let w = cexp_route(xf, yf, mode);
                    assert_eq!((xf, yf, mode, r.0 .0.to_bits(), r.0 .1.to_bits(), r.1), (xf, yf, mode, w.0 .0.to_bits(), w.0 .1.to_bits(), w.1), "cexpf quick tier differs");
                    if mode == Rounding::Nearest {
                        assert_eq!(fast32::cexpf_nearest(xf, yf).map(|(a, b)| (a.to_bits(), b.to_bits())), Some((r.0 .0.to_bits(), r.0 .1.to_bits())));
                    }
                }
            }
        }
    }
    hits
}

#[test]
fn cexp_quick_tier_matches_the_384_bit_route_in_every_mode() {
    // Clauses: s10_5_c_0, s10_0_f_0.
    let n = 3000;
    let hits = compare(n, 0x9e37_79b9_7f4a_7c15);
    // The tier must be reached where it applies (a tier that always declines would pass the
    // comparison): nearly every general operand, a share of the cos-tiny ones; none beyond 2^20.
    assert!(hits[0] * 1000 >= n * 999, "general regime answered {} of {n}", hits[0]);
    assert!(hits[1] * 4 >= n, "cos-tiny regime answered {} of {n}", hits[1]);
    assert!(hits[3] > 0 && hits[4] > 0, "threshold regimes never answered: {hits:?}");
    let mut st = 7u64;
    for _ in 0..n {
        let (x, y) = (uniform(&mut st, -2.0, 2.0), uniform(&mut st, 1_048_576.0, 1.0e15));
        assert!(cexp(x, y, Rounding::Nearest).is_none() && cexp(x, -y, Rounding::Nearest).is_none(), "Payne-Hanek range must fall back");
    }
}

#[test]
fn cexp_quick_tier_bound_covers_the_error() {
    // Clauses: s10_0_f_0. Each part's `eps` against the 384-bit value of e^x·cos y, e^x·sin y
    // (scaled by 2^-m), including y next to π/2 + kπ, where the cosine's bound must stay relative.
    let mut st = 0x2545_f491_4f6c_dd1du64;
    let mut tight = 0usize;
    for i in 0..4000 {
        let (x, y) = if i % 2 == 0 { operand(&mut st, 0) } else { operand(&mut st, 1) };
        let Some((pp, m)) = parts(x, y) else { continue };
        let (s, c) = mp::sin_cos_f64(y);
        let e = mp::exp(Mp::from_f64(x)).ldexp(-i64::from(m));
        for ((p, eps), t) in pp.into_iter().zip([c, s]) {
            let err = e.mul(t).sub(Mp::from_f64(p.hi).add(Mp::from_f64(p.lo))).to_f64().abs();
            assert!(err <= eps, "x = {x:e}, y = {y:e}: error {err:e} above the bound {eps:e}");
            if err > eps / 64.0 {
                tight += 1;
            }
        }
    }
    // The bound is what decides: it is not loose by orders of magnitude everywhere.
    assert!(tight > 0, "every error far below its bound");
}

#[test]
fn cexpf_quick_tier_declines_near_a_rounding_boundary() {
    // Clauses: s10_0_f_0. Operands whose `double` part lies within the margin of a `float`
    // rounding boundary must go to the route: the online test is what keeps the tier correct.
    let mut st = 0x1234_5678_9abc_def1u64;
    let mut found = 0;
    for _ in 0..4_000_000 {
        let (x, y) = (uniform(&mut st, -20.0, 20.0) as f32, uniform(&mut st, -10.0, 10.0) as f32);
        let Some((re, im)) = fast32::cexp_core(x, y) else { continue };
        let close = |v: f64| {
            let low = v.to_bits() & 0x0fff_ffff;
            low <= fast32_delta() || low >= 0x1000_0000 - fast32_delta()
        };
        if close(re) || close(im) {
            found += 1;
            for mode in MODES {
                assert!(fast32::cexpf(x, y, mode).is_none(), "x = {x:e}, y = {y:e}: the tier answered next to a boundary");
            }
        }
    }
    assert!(found >= 5, "only {found} operands near a boundary");
}

/// The `float` parts' margin in ulp of the `double` part.
fn fast32_delta() -> u64 {
    crate::fast32::CEXP_DELTA
}

/// The 10^7-operand sweep per type (`cargo test --release -p liella-math --lib -- --ignored
/// cexp_quick_tier_sweep`): the comparison above on 8 threads, 2·10^6 operands per regime.
#[test]
#[ignore = "long sweep: cargo xtask long-sweeps runs it in release (nightly and release runs)"]
fn cexp_quick_tier_sweep() {
    // Clauses: s10_5_c_0, s10_0_f_0.
    let threads: Vec<_> = (0..8u64).map(|i| std::thread::spawn(move || compare(250_000, 0x9e37_79b9_7f4a_7c15 ^ (i + 1).wrapping_mul(0xd1b5_4a32_d192_ed03)))).collect();
    let mut total = [0usize; 5];
    for h in threads {
        let r = h.join();
        assert!(r.is_ok(), "a sweep thread failed (its panic message is above)");
        let r = r.unwrap_or_default();
        for (t, v) in total.iter_mut().zip(r) {
            *t += v;
        }
    }
    std::println!("cexp quick tier sweep: 10^7 operands per type, double answers per regime (nearest) {total:?} of 2000000 each");
}

/// cbench_libm's 256 complex operands (its xorshift, seed and `[-3, 3]^2`): the tier declines one
/// only when it must, that is when a part's exact value (384 bits) lies within twice its bound of
/// a rounding boundary to nearest; any other decline sends the bench row to the route.
#[test]
fn cexp_quick_tier_declines_the_bench_box_only_when_forced() {
    // no spec clause (performance regression guard: the tier's answer rate on the measured operands).
    let mut r = 88_172_645_463_325_252u64;
    let mut next = || {
        r ^= r << 13;
        r ^= r >> 7;
        r ^= r << 17;
        (r >> 11) as f64 / 9_007_199_254_740_992.0
    };
    let mut declined = 0;
    for _ in 0..256 {
        let (x, y) = (-3.0 + 6.0 * next(), -3.0 + 6.0 * next());
        // The tier with its middle part (the boundary's in-place attempt and cold fallback).
        if cexp(x, y, Rounding::Nearest).is_some() {
            continue;
        }
        declined += 1;
        assert!(parts(x, y).is_some(), "({x:e}, {y:e}) is inside the tier's range");
        let (s, c) = mp::sin_cos_f64(y);
        // Distance of each exact part from the nearest midpoint of the `double` grid around it,
        // against the middle tier's bound (the last tier before the route).
        let forced = [(Part::Re, c), (Part::Im, s)].into_iter().any(|(part, t)| {
            let Some((_, eps, m)) = middle_part(x, y, part) else { return true };
            let v = mp::exp(Mp::from_f64(x)).ldexp(-i64::from(m)).mul(t);
            let r = v.to_f64();
            let half = (f64::from_bits(r.to_bits() + 1) - r).abs() / 2.0;
            // The midpoints `r ± half` are not `double`s: formed in 384 bits.
            let d = [-half, half].map(|h| v.sub(Mp::from_f64(r).add(Mp::from_f64(h))).to_f64().abs());
            std::println!(
                "declined ({x:e}, {y:e}) = ({:#x}, {:#x}): part {r:e} m={m} midpoint distance 2^{:.2} rel, bound 2^{:.2} rel",
                x.to_bits(),
                y.to_bits(),
                (d[0].min(d[1]) / r.abs()).log2(),
                (eps / r.abs()).log2()
            );
            d[0].min(d[1]) <= 2.0 * eps
        });
        assert!(forced, "the tier declined ({x:e}, {y:e}) although no part is within its bound of a rounding boundary");
    }
    std::println!("cexp quick tier: {declined} of 256 bench operands declined, each a hard case");
}

/// The operand of cbench_libm's set that the quick tier cannot decide (its real part lies
/// `2^-70.3` relative from a midpoint, the bound is `2^-66.3`).
const BENCH_HARD: (u64, u64) = (0xbff9_c127_e0e2_1690, 0xbffe_c14d_45bc_5e2d);

#[test]
fn cexp_middle_tier_decides_the_bench_hard_case() {
    // Clauses: s10_0_f_0. The quick test declines the real part, the middle tier answers it,
    // and in every mode the answer is the 384-bit route's (value bits and status).
    let (x, y) = (f64::from_bits(BENCH_HARD.0), f64::from_bits(BENCH_HARD.1));
    let p = parts(x, y);
    assert!(p.is_some(), "the bench operand is inside the quick tier's range");
    let Some(([(re, er), _], m)) = p else { return };
    assert!(round(re, er, m, Rounding::Nearest).is_none(), "the quick test no longer declines the bench operand");
    for mode in MODES {
        let w = cexp_route(x, y, mode);
        let got = cexp(x, y, mode);
        assert!(got.is_some(), "the middle tier declined the bench operand in {mode:?}");
        let Some(((a, b), st)) = got else { continue };
        assert_eq!((a.to_bits(), b.to_bits(), st), (w.0 .0.to_bits(), w.0 .1.to_bits(), w.1), "cexp middle tier differs in {mode:?}");
    }
    // In place: the hot kernel declines the part (no spec clause, performance guard, ruling #4885:
    // it keeps no operand live for an out-of-line call), and the boundary's cold second attempt
    // answers it with the route's bits.
    assert_eq!(cexp_nearest(x, y), None, "the in-place kernel answered a quick-test decline");
    let c = cexp_nearest_middle(x, y).map(|(a, b)| (a.to_bits(), b.to_bits()));
    let w = cexp_route(x, y, Rounding::Nearest).0;
    assert_eq!(c, Some((w.0.to_bits(), w.1.to_bits())), "cexp's cold in-place attempt differs or declined");
}

#[test]
fn cexp_middle_tier_bound_covers_the_error() {
    // Clauses: s10_0_f_0. `sin_or_cos` within TRIG_ABS and each part within its `eps` of the
    // 384-bit values, over the quick tier's regimes, y next to π/2 + kπ and |y| up to 2^20.
    let mut st = 0x9e37_79b9_7f4a_7c15u64;
    let mut tight = 0usize;
    for i in 0..6000 {
        let (x, y) = match i % 3 {
            0 => operand(&mut st, i % 5),
            1 => (uniform(&mut st, -3.0, 3.0), near_half_pi(&mut st)),
            _ => (uniform(&mut st, -1.0, 1.0), uniform(&mut st, -1_048_575.0, 1_048_575.0)),
        };
        if parts(x, y).is_none() {
            continue;
        }
        let (s, c) = mp::sin_cos_f64(y);
        for (part, t) in [(Part::Re, c), (Part::Im, s)] {
            let d = sin_or_cos(y, part);
            let terr = t.sub(Mp::from_f64(d.hi).add(Mp::from_f64(d.lo))).to_f64().abs();
            assert!(terr <= TRIG_ABS, "y = {y:e}: trig error {terr:e} above the bound");
            let Some((p, eps, m)) = middle_part(x, y, part) else { continue };
            let e = mp::exp(Mp::from_f64(x)).ldexp(-i64::from(m));
            let err = e.mul(t).sub(Mp::from_f64(p.hi).add(Mp::from_f64(p.lo))).to_f64().abs();
            assert!(err <= eps, "x = {x:e}, y = {y:e}: middle error {err:e} above the bound {eps:e}");
            // Worst-case analysis against typical double-double errors near 2^-105: within 2^12.
            if err > eps / 4096.0 {
                tight += 1;
            }
        }
    }
    assert!(tight > 0, "every middle error far below its bound");
}

#[test]
fn cexp_middle_tier_declines_next_to_a_boundary() {
    // no spec clause (the middle tier's rounding test): an interval holding a midpoint declines
    // to nearest, one holding a `double` declines in the directed modes; clear of both it answers.
    let eps = MID_EPS;
    let midpoint = Dd { hi: 1.0, lo: f64::from_bits(0x3ca0_0000_0000_0000) };
    assert_eq!(middle_round(midpoint, eps, 0, Rounding::Nearest), None, "the middle tier answered next to a boundary");
    let on_double = Dd { hi: 1.0, lo: f64::from_bits(0x3af0_0000_0000_0000) };
    for mode in [Rounding::Down, Rounding::Up, Rounding::Zero] {
        assert_eq!(middle_round(on_double, eps * 1.0e10, 0, mode), None, "the middle tier answered next to a boundary in {mode:?}");
    }
    assert_eq!(middle_round(on_double, eps, 3, Rounding::Nearest), Some(8.0), "a clear part is answered and scaled");
}
