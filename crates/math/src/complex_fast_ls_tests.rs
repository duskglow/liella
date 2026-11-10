//! The `clog`/`csqrt` quick tiers (and `float`'s) against the 384-bit route (Clauses: s10_5_c_0,
//! s10_0_f_0: where a tier answers, it is the route's result bit for bit, in every mode).

use super::*;
use crate::complex::{clog_route, csqrt_route};

const MODES: [Rounding; 4] = [Rounding::Nearest, Rounding::Down, Rounding::Up, Rounding::Zero];

fn uniform(st: &mut u64, lo: f64, hi: f64) -> f64 {
    *st ^= *st << 13;
    *st ^= *st >> 7;
    *st ^= *st << 17;
    lo + (hi - lo) * (f64::from_bits(0x3ff0_0000_0000_0000 | (*st >> 12)) - 1.0)
}

fn sign(st: &mut u64, v: f64) -> f64 {
    if uniform(st, 0.0, 1.0) < 0.5 { -v } else { v }
}

/// `2^k·[1, 2)` with `k` uniform in `[lo, hi)`, random sign.
fn magnitude(st: &mut u64, lo: f64, hi: f64) -> f64 {
    let k = uniform(st, lo, hi) as i32;
    let m = uniform(st, 1.0, 2.0);
    sign(st, dd::pow2(k) * m)
}

/// The regimes: general `[-8, 8]²`; `|z|` within `2^-k` of 1 (`k` up to 55); near the axes
/// (`min/max` down to `2^-70`, beyond `atan2`'s `2^-60`); wide magnitudes (`2^±300`, within `2^50` of each other).
fn operand(st: &mut u64, regime: usize) -> (f64, f64) {
    match regime {
        0 => (uniform(st, -8.0, 8.0), uniform(st, -8.0, 8.0)),
        1 => {
            let th = uniform(st, -3.2, 3.2);
            let d = magnitude(st, -56.0, -1.0);
            let (s, c) = crate::mp::sin_cos_f64(th);
            ((1.0 + d) * c.to_f64(), (1.0 + d) * s.to_f64())
        }
        2 => {
            let a = uniform(st, -10.0, 10.0);
            let k = magnitude(st, -71.0, -1.0);
            if uniform(st, 0.0, 1.0) < 0.5 { (a, a * k) } else { (a * k, a) }
        }
        _ => {
            // Within 2^50 of each other (atan2's window needs min/max >= 2^-60).
            let x = magnitude(st, -300.0, 300.0);
            (x, x * magnitude(st, -50.0, 50.0))
        }
    }
}

fn hw_sqrt(v: f64) -> f64 {
    v.sqrt()
}

/// `n` operands per regime, four modes, both types, both functions: every answer equals the
/// route's (value bits and status), the in-place entries agree with Nearest; returns the
/// `double` answers (nearest) per regime, `[clog; 4]` then `[csqrt; 4]`.
fn compare(n: usize, seed: u64) -> [usize; 8] {
    let mut st = seed;
    let mut hits = [0usize; 8];
    for regime in 0..4 {
        for _ in 0..n {
            let (x, y) = operand(&mut st, regime);
            let (xf, yf) = (x as f32, y as f32);
            for mode in MODES {
                let nearest = mode == Rounding::Nearest;
                if let Some(r) = clog(x, y, mode) {
                    let w = clog_route(x, y, mode);
                    assert_eq!((x, y, mode, r.0 .0.to_bits(), r.0 .1.to_bits(), r.1), (x, y, mode, w.0 .0.to_bits(), w.0 .1.to_bits(), w.1), "clog quick tier differs");
                    if nearest {
                        if let Some(h) = hits.get_mut(regime) {
                            *h += 1;
                        }
                        assert_eq!(clog_nearest(x, y).map(|(a, b)| (a.to_bits(), b.to_bits())), Some((r.0 .0.to_bits(), r.0 .1.to_bits())));
                    }
                }
                if let Some(r) = csqrt(x, y, mode) {
                    let w = csqrt_route(x, y, mode);
                    assert_eq!((x, y, mode, r.0 .0.to_bits(), r.0 .1.to_bits(), r.1), (x, y, mode, w.0 .0.to_bits(), w.0 .1.to_bits(), w.1), "csqrt quick tier differs");
                    if nearest {
                        if let Some(h) = hits.get_mut(4 + regime) {
                            *h += 1;
                        }
                        assert_eq!(csqrt_nearest(x, y, hw_sqrt).map(|(a, b)| (a.to_bits(), b.to_bits())), Some((r.0 .0.to_bits(), r.0 .1.to_bits())));
                    }
                }
                if let Some(r) = fast32::clogf(xf, yf, mode) {
                    let w = clog_route(xf, yf, mode);
                    assert_eq!((xf, yf, mode, r.0 .0.to_bits(), r.0 .1.to_bits(), r.1), (xf, yf, mode, w.0 .0.to_bits(), w.0 .1.to_bits(), w.1), "clogf quick tier differs");
                    if nearest {
                        assert_eq!(fast32::clogf_nearest(xf, yf).map(|(a, b)| (a.to_bits(), b.to_bits())), Some((r.0 .0.to_bits(), r.0 .1.to_bits())));
                    }
                }
                if let Some(r) = fast32::csqrtf(xf, yf, mode, soft_sqrt) {
                    let w = csqrt_route(xf, yf, mode);
                    assert_eq!((xf, yf, mode, r.0 .0.to_bits(), r.0 .1.to_bits(), r.1), (xf, yf, mode, w.0 .0.to_bits(), w.0 .1.to_bits(), w.1), "csqrtf quick tier differs");
                    if nearest {
                        assert_eq!(fast32::csqrtf_nearest(xf, yf, hw_sqrt).map(|(a, b)| (a.to_bits(), b.to_bits())), Some((r.0 .0.to_bits(), r.0 .1.to_bits())));
                    }
                }
            }
        }
    }
    hits
}

#[test]
fn clog_csqrt_quick_tiers_match_the_384_bit_route_in_every_mode() {
    // Clauses: s10_5_c_0, s10_0_f_0. The tiers must also be reached where they apply.
    let n = 1500;
    let [c0, c1, _, c3, s0, s1, _, s3] = compare(n, 0x9e37_79b9_7f4a_7c15);
    assert!(c0 * 1000 >= n * 998 && s0 * 1000 >= n * 998, "general regime answered {c0}, {s0} of {n}");
    assert!(c1 * 10 >= n * 9 && s1 * 10 >= n * 9, "near |z| = 1 answered {c1}, {s1} of {n}");
    assert!(c3 * 1000 >= n * 998 && s3 * 1000 >= n * 998, "wide regime answered {c3}, {s3} of {n}");
}

#[test]
fn clog_quick_tier_answers_near_the_unit_circle() {
    // Clauses: s10_0_f_0. `|z|² − 1` near 2^-50 (x just below 1, y near 2^-26): the `log1p` path
    // with its exact products answers; `ln` of the rounded sum could not.
    let mut st = 0x5151_7e57_aa55_1234u64;
    let mut hit = 0;
    for _ in 0..2000 {
        let x = 1.0 - uniform(&mut st, 1.0, 1024.0).floor() * dd::pow2(-53);
        let y = uniform(&mut st, 1.0, 2.0) * dd::pow2(-26);
        if let Some(r) = clog(x, y, Rounding::Nearest) {
            hit += 1;
            let w = clog_route(x, y, Rounding::Nearest);
            assert_eq!((r.0 .0.to_bits(), r.0 .1.to_bits()), (w.0 .0.to_bits(), w.0 .1.to_bits()), "clog quick tier differs near |z| = 1");
        }
    }
    assert!(hit >= 1900, "clog near |z| = 1 answered only {hit} of 2000");
}

#[test]
fn csqrt_quick_tiers_decline_exact_roots() {
    // Clauses: s10_0_f_0. `csqrt(3 + 4i) = 2 + i` and its kin are exact: the route decides them.
    for (x, y) in [(3.0, 4.0), (-3.0, 4.0), (5.0, -12.0), (-7.0, -24.0), (0.75, 1.0)] {
        assert!(csqrt(x, y, Rounding::Nearest).is_none() && csqrt_nearest(x, y, hw_sqrt).is_none(), "exact root of ({x}, {y}) answered");
        assert!(fast32::csqrtf(x as f32, y as f32, Rounding::Up, soft_sqrt).is_none(), "exact float root of ({x}, {y}) answered");
    }
}

#[test]
fn rounding_test_declines_an_interval_holding_a_midpoint() {
    // Clauses: s10_0_f_0. To nearest, 1.5 + 2^-53 (the midpoint above 1.5) lies within eps =
    // 2^-70 of a value 2^-80 below it; in the directed modes, 1.5 itself lies within eps of
    // 1.5 + 2^-80. Either way the rounding is undecided and the test must decline.
    assert_eq!(round(Dd { hi: 1.5, lo: dd::pow2(-53) - dd::pow2(-80) }, dd::pow2(-70), 0, Rounding::Nearest), None, "the rounding test answered across a midpoint");
    for mode in [Rounding::Down, Rounding::Up, Rounding::Zero] {
        assert_eq!(round(Dd { hi: 1.5, lo: dd::pow2(-80) }, dd::pow2(-70), 0, mode), None, "the rounding test answered across a double ({mode:?})");
    }
}

#[test]
fn csqrt_branch_cut_sides() {
    // Clauses: s10_5_c_0. Just above and below the negative real axis the imaginary part takes
    // the sign of y (Annex G: csqrt(conj z) = conj csqrt z).
    for y in [1e-10, -1e-10, 3e-5, -3e-5] {
        let ((re, im), _) = csqrt(-4.0, y, Rounding::Nearest).unwrap_or(((f64::NAN, f64::NAN), Status::flags(Flags::INEXACT)));
        assert!(re > 0.0 && im.signum() == y.signum() && (im.abs() - 2.0).abs() < 1e-9, "csqrt(-4 + {y}i) = ({re}, {im}): wrong side of the cut");
    }
}

/// The 10^7-operand sweep per function (`cargo test --release -p liella-math --lib -- --ignored
/// clog_csqrt_quick_tier_sweep`): the comparison above on 8 threads, four modes.
#[test]
#[ignore = "long sweep, run as a job"]
fn clog_csqrt_quick_tier_sweep() {
    // Clauses: s10_5_c_0, s10_0_f_0.
    let threads: Vec<_> = (0..8u64).map(|i| std::thread::spawn(move || compare(312_500, 0x2545_f491_4f6c_dd1d ^ (i + 1).wrapping_mul(0xd1b5_4a32_d192_ed03)))).collect();
    let mut total = [0usize; 8];
    for h in threads {
        let r = h.join();
        assert!(r.is_ok(), "a sweep thread failed (its panic message is above)");
        for (t, v) in total.iter_mut().zip(r.unwrap_or_default()) {
            *t += v;
        }
    }
    std::println!("clog/csqrt quick tier sweep: 10^7 operands, double answers (nearest) clog per regime {:?}, csqrt {:?} of 2500000 each", total.get(..4), total.get(4..));
}
