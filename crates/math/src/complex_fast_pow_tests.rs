//! The `cpow`/`cpowf` tiers against the 384-bit route (Clauses: s10_5_c_0, s10_0_f_0: where a
//! tier answers, it is the route's result bit for bit, in every mode, flags included).

#![allow(clippy::unwrap_used, clippy::float_cmp)]

use super::*;
use crate::complex::cpow_route;

const MODES: [Rounding; 4] = [Rounding::Nearest, Rounding::Down, Rounding::Up, Rounding::Zero];

fn uniform(st: &mut u64, lo: f64, hi: f64) -> f64 {
    *st ^= *st << 13;
    *st ^= *st >> 7;
    *st ^= *st << 17;
    lo + (hi - lo) * ((*st >> 11) as f64 * f64::from_bits(0x3ca0_0000_0000_0000))
}

/// Operands by band: 0 typical, 1 `|z|` near 1, 2 near the branch cut, 3 `|w|` in `[2^6, 2^12]`,
/// 4 real exponents (`d = 0`).
fn operand(st: &mut u64, band: usize) -> [f64; 4] {
    let mut u = |lo, hi| uniform(st, lo, hi);
    match band {
        0 => [u(-3.0, 3.0), u(-3.0, 3.0), u(-3.0, 3.0), u(-3.0, 3.0)],
        1 => {
            let (r, a) = (1.0 + u(-1e-6, 1e-6), u(-3.1, 3.1));
            [r * a.cos(), r * a.sin(), u(-3.0, 3.0), u(-3.0, 3.0)]
        }
        2 => [u(-4.0, -0.25), u(-1e-6, 1e-6), u(-3.0, 3.0), u(-3.0, 3.0)],
        3 => {
            let s = u(6.0, 12.0).exp2();
            [u(0.5, 1.5), u(-0.5, 0.5), s * u(-1.0, 1.0) / 64.0, s * u(-1.0, 1.0) / 64.0]
        }
        _ => [u(-3.0, 3.0), u(-3.0, 3.0), u(-6.0, 6.0), 0.0],
    }
}

/// Tier against route for `n` operands per band in every mode; returns `[answered, quick
/// declines, middle declines]` (nearest) and checks `float` alike.
fn compare(n: usize, seed: u64) -> [usize; 3] {
    let mut st = seed;
    let mut count = [0usize; 3];
    for band in 0..5 {
        for _ in 0..n {
            let [x, y, c, d] = operand(&mut st, band);
            for mode in MODES {
                if let Some(r) = cpow(x, y, c, d, mode) {
                    let w = cpow_route(x, y, c, d, mode);
                    assert_eq!((r.0 .0.to_bits(), r.0 .1.to_bits(), r.1), (w.0 .0.to_bits(), w.0 .1.to_bits(), w.1), "cpow {x:e} {y:e} {c:e} {d:e} {mode:?}");
                }
                let (xf, yf, cf, df) = (x as f32, y as f32, c as f32, d as f32);
                if let Some(r) = cpowf(xf, yf, cf, df, mode) {
                    let w = cpow_route(xf, yf, cf, df, mode);
                    assert_eq!((r.0 .0.to_bits(), r.0 .1.to_bits(), r.1), (w.0 .0.to_bits(), w.0 .1.to_bits(), w.1), "cpowf {xf:e} {yf:e} {cf:e} {df:e} {mode:?}");
                }
            }
            if let Some(([(re, er), (im, ei)], m)) = parts(x, y, c, d) {
                for (p, e, part) in [(re, er, Part::Re), (im, ei, Part::Im)] {
                    // The middle tier runs on every part (not only the quick declines): its
                    // bound must decide all of them (expected misses about 2^-44 per part).
                    if strict(p, e, m, Rounding::Nearest).is_some() {
                        count[0] += 1;
                    } else {
                        count[1] += 1;
                    }
                    if middle(x, y, c, d, part, Rounding::Nearest).is_none() {
                        count[2] += 1;
                    }
                }
            }
        }
    }
    count
}

#[test]
fn cpow_tiers_match_the_384_bit_route_in_every_mode() {
    let [answered, mid, declined] = compare(150, 0x9e37_79b9_7f4a_7c15);
    // Nearly every part is answered by one of the tiers; none reaches the route here.
    assert!(answered + mid > 1400 && declined == 0, "answered {answered} middle {mid} declined {declined}");
}

#[test]
fn cpow_tiers_decline_exact_parts() {
    // (3 + 4i)^1.5 = 2 + 11i and (3 + 4i)^2 = −7 + 24i: exact, so the route decides them.
    for (c, d) in [(1.5, 0.0), (2.0, 0.0), (0.5, 0.0)] {
        for mode in MODES {
            assert_eq!(cpow(3.0, 4.0, c, d, mode), None, "{c} {mode:?}");
            assert_eq!(cpowf(3.0, 4.0, c as f32, d as f32, mode), None, "{c} {mode:?}");
        }
    }
}
