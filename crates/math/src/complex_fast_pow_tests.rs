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

/// No spec clause (the bounds the middle tier's strict test relies on, #5023): the table facts
/// of the proofs at `log_dd` and `atan_dd` hold, every constant covers its proof (a bound made
/// 2x too small fails), and sampled errors against the 384-bit `crate::mp` stay inside.
#[test]
fn cpow_middle_bounds_cover_their_proofs() {
    use crate::coeffs::tables as tb;
    use crate::exp_log;
    use crate::mp::{self, Mp};
    let u = f64::EPSILON / 2.0;
    let mpd = |v: Dd| Mp::from_f64(v.hi).add(Mp::from_f64(v.lo));
    let rel = |got: Mp, want: Mp| got.sub(want).abs().to_f64() / want.abs().to_f64();
    let rep = 1.001 * u * u;
    for (k, &cb) in tb::LOG_C.iter().enumerate() {
        let (c, i) = (f64::from_bits(cb), k as f64 - 37.0);
        for m in [(1.0 + (i - 0.5) / 128.0).max(core::f64::consts::FRAC_1_SQRT_2), (1.0 + (i + 0.5) / 128.0).min(core::f64::consts::SQRT_2)] {
            let r = dd::abs(m * c - 1.0) + 2.0 * u;
            assert!(r / (2.0 - r) <= (-8.4f64).exp2(), "LOG_C bucket {i}");
        }
        if tb::LOG_L[k][0] != 0 {
            assert!(rel(mpd(Dd::of(tb::LOG_L[k])), mp::ln(Mp::from_f64(c)).neg()) <= rep, "LOG_L {i}");
        }
    }
    for (j, &a) in tb::ATAN_J64.iter().enumerate().skip(1) {
        assert!(rel(mpd(Dd::of(a)), mp::atan(Mp::from_f64(j as f64 / 64.0))) <= rep, "ATAN_J64 {j}");
    }
    assert!(rel(mpd(Dd::of(tb::LN2)), mp::ln(Mp::from_f64(2.0))) <= rep);
    assert!(rel(mpd(Dd::of(tb::PI_2)), mp::atan(Mp::from_f64(1.0)).mul_u64(2)) <= rep);
    // The proofs' sums (units and steps as in the doc comments).
    let (a2, f2) = (3.0 * u * u / (1.0 - 4.0 * u), 2.0 * u * u / (1.0 - 2.0 * u));
    let (m1, m2, d2) = (3.0 * u * u * (1.0 + 2.0 * u), 8.0 * u * u * (1.0 + (-50f64).exp2()), 5.0 * u * u * (1.0 + (-48f64).exp2()));
    let eps_lr = (a2 * (2.0 + 1.0 / 128.0) + d2) + (a2 + 2.1 * u * (-50.4f64).exp2() / 7.0 + 0.02 * u * u) + m2;
    let log_bound = (2.04 * u * u + 1.035 * eps_lr + a2).max(2.0 * m1 + 3.0 * u * u + 2.02 * a2 + 0.017 * eps_lr);
    let eps_at = m2 + (d2 + m1 / 2.0 + f2) + a2 + 0.04 * u * u;
    let atan_bound = 2.0 * u * u + (2.0001 * u * u + eps_at + a2) + d2 + a2;
    assert!(exp_log::LOG_DD_REL >= log_bound && trig::ATAN_DD_REL >= atan_bound, "{} {}", log_bound / (u * u), atan_bound / (u * u));
    assert!(LR_REL >= exp_log::LOG_DD_REL && LR_ABS >= 0.5 * (-104.4f64).exp2() && LI_REL >= trig::ATAN_DD_REL + d2 + a2 + 2.0 * u * u);
    let mut st = 0x5851_f42d_4c95_7f2d_u64;
    for k in 0..4000 {
        let v = if k % 2 == 0 { uniform(&mut st, -30.0, 30.0).exp2() } else { 1.0 + uniform(&mut st, -0.02, 0.02) };
        let x = dd::two_sum(v, v * uniform(&mut st, -1.0, 1.0) * u);
        let want = mp::ln(mpd(x));
        assert!(want.is_zero() || rel(mpd(exp_log::log_dd(x)), want) <= exp_log::LOG_DD_REL, "log_dd {:e} {:e}", x.hi, x.lo);
        let t = Dd::from(if k % 2 == 0 { uniform(&mut st, 0.0, 4.0) } else { uniform(&mut st, -40.0, 70.0).exp2() });
        assert!(rel(mpd(trig::atan_dd(t)), mp::atan(mpd(t))) <= trig::ATAN_DD_REL, "atan_dd {:e}", t.hi);
    }
}

/// Operands with `|w·log z|` in band 0 `[2^-6, 1]`, 1 `[1, 16]`, 2 `[16, 512]`.
fn wlog_operand(st: &mut u64, band: usize) -> [f64; 4] {
    let (x, y) = (uniform(st, -3.0, 3.0), uniform(st, -3.0, 3.0));
    let l = (0.25 * (x * x + y * y).ln().powi(2) + y.atan2(x).powi(2)).sqrt();
    let (lo, hi) = [(-6.0, 0.0), (0.0, 4.0), (4.0, 9.0)][band];
    let (m, a) = (uniform(st, lo, hi).exp2() / l, uniform(st, -3.1, 3.1));
    [x, y, m * a.cos(), m * a.sin()]
}

/// The decline rates per `|w·log z|` band (#5022; a long-sweeps row): quick declines measured;
/// the middle tier run on every part, its declines measured (none expected) and expected from
/// its bound (`4·eps/ulp` averaged), which must be `2^-30` or less per part.
#[test]
#[ignore = "long sweep: run in release"]
fn cpow_decline_rates() {
    for band in 0..3 {
        let mut st = 0x2545_f491_4f6c_dd1d_u64 + band as u64;
        let (mut n, mut quick, mut mid, mut expect) = (0u64, 0u64, 0u64, 0f64);
        for _ in 0..1 << 21 {
            let [x, y, c, d] = wlog_operand(&mut st, band);
            let Some(([(re, er), (im, ei)], m)) = parts(x, y, c, d) else { continue };
            for (p, e, part) in [(re, er, Part::Re), (im, ei, Part::Im)] {
                n += 1;
                quick += u64::from(strict(p, e, m, Rounding::Nearest).is_none());
                let Some((q, eps, s)) = middle_part(x, y, c, d, part) else {
                    mid += 1;
                    continue;
                };
                mid += u64::from(strict(q, eps, s, Rounding::Nearest).is_none());
                expect += 4.0 * eps / (dd::abs(q.hi) * f64::EPSILON);
            }
        }
        let rate = expect / n as f64;
        println!("band {band}: parts {n} quick 2^{:.1} middle {mid} expected 2^{:.1}", (quick as f64 / n as f64).log2(), rate.log2());
        assert!(n > 1 << 20 && mid == 0 && rate <= (-30f64).exp2(), "band {band}");
    }
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
