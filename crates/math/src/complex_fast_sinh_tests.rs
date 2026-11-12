//! The `csinh`/`csin` quick tier against the 384-bit route (Clauses: s10_5_c_0, s10_0_f_0: where
//! the tier answers, it is the route's result bit for bit, in every mode) and the hyperbolic
//! factors against a double-double reference.

use super::*;
use crate::complex::{csin_route, csinh_route};

const MODES: [Rounding; 4] = [Rounding::Nearest, Rounding::Down, Rounding::Up, Rounding::Zero];

/// xorshift64 uniform in `[lo, hi)`.
fn uniform(st: &mut u64, lo: f64, hi: f64) -> f64 {
    *st ^= *st << 13;
    *st ^= *st >> 7;
    *st ^= *st << 17;
    lo + (hi - lo) * ((*st >> 11) as f64 * f64::from_bits(0x3ca0_0000_0000_0000))
}

/// One operand of a regime: 0 typical `[-3, 3]²`, 1 the series (`|x|` log-uniform in
/// `[2^-60, 2^-3]`), 2 the cancellation band `[2^-3, 1]`, 3 large `|x|` up to the overflow edge,
/// 4 `y` near a multiple of `π/2` (a small trig factor). The sign of `x` random.
fn operand(st: &mut u64, regime: usize) -> (f64, f64) {
    let s = if uniform(st, 0.0, 1.0) < 0.5 { -1.0 } else { 1.0 };
    let y = uniform(st, -3.0, 3.0);
    match regime {
        0 => (uniform(st, -3.0, 3.0), y),
        1 => (s * uniform(st, -60.0, -3.0).exp2(), y),
        2 => (s * uniform(st, 0.125, 1.0), y),
        3 => (s * uniform(st, 30.0, 746.0), uniform(st, -40.0, 40.0)),
        _ => (s * uniform(st, 0.0, 5.0), core::f64::consts::FRAC_PI_2 * uniform(st, 1.0, 9.0).floor() + uniform(st, -1e-9, 1e-9)),
    }
}

/// `n` operands per regime and mode for `f` (`csinh` or `csin`) against `route`: the tier's
/// answers equal the route's; returns the answers per regime (nearest).
fn compare((f, route, nearest): Fns, n: usize, seed: u64) -> [usize; 5] {
    let mut out = [0; 5];
    for (regime, o) in out.iter_mut().enumerate() {
        let mut st = seed ^ (regime as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        for _ in 0..n {
            let (x, y) = operand(&mut st, regime);
            for mode in MODES {
                let Some(((a, b), _)) = f(x, y, mode) else { continue };
                let ((ra, rb), rs) = route(x, y, mode);
                assert_eq!((a.to_bits(), b.to_bits()), (ra.to_bits(), rb.to_bits()), "({x:e}, {y:e}) {mode:?}: tier ({a:e}, {b:e}) route ({ra:e}, {rb:e})");
                assert_eq!(rs, Status::flags(Flags::INEXACT), "({x:e}, {y:e}) {mode:?}");
                if mode == Rounding::Nearest {
                    *o += 1;
                    assert_eq!(nearest(x, y), Some((a, b)), "({x:e}, {y:e}): the in-place entry");
                }
            }
        }
    }
    out
}

type Fns = (fn(f64, f64, Rounding) -> Option<((f64, f64), Status)>, fn(f64, f64, Rounding) -> ((f64, f64), Status), fn(f64, f64) -> Option<(f64, f64)>);

/// `csinh` and `csin`: the tier, the route, the in-place entry.
const FNS: [Fns; 2] = [(csinh, csinh_route::<f64>, csinh_nearest), (csin, csin_route::<f64>, csin_nearest)];

#[test]
fn csinh_quick_tier_matches_the_384_bit_route_in_every_mode() {
    for fns in FNS {
        let got = compare(fns, 120, 0x1234_5678);
        // The tier answers nearly all of the typical box and both hyperbolic bands (a decline is
        // a part within about 2^-66 of a rounding boundary, or an out-of-range result).
        assert!(got[0] >= 118 && got[1] >= 118 && got[2] >= 117, "answers per regime {got:?}");
    }
}

#[test]
fn csinhf_quick_tier_matches_the_route_in_every_mode() {
    let mut st = 0x5eed_u64;
    let mut answered = 0;
    for i in 0..4000 {
        let x = uniform(&mut st, -12.0, 12.0) as f32;
        let x = if i % 4 == 0 { x * 1e-5 } else { x };
        let y = uniform(&mut st, -6.0, 6.0) as f32;
        for mode in MODES {
            for (f, route) in [(fast32::csinhf as fn(f32, f32, Rounding) -> _, crate::complex::csinh_route::<f32> as fn(f32, f32, Rounding) -> _), (fast32::csinf, crate::complex::csin_route::<f32>)] {
                let Some((v, _)) = f(x, y, mode) else { continue };
                let (r, _) = route(x, y, mode);
                assert_eq!((v.0.to_bits(), v.1.to_bits()), (r.0.to_bits(), r.1.to_bits()), "({x:e}, {y:e}) {mode:?}");
                answered += 1;
            }
        }
    }
    assert!(answered >= 31_000, "answers {answered}");
}

/// The hard cases at the bands' edges: the series cut, `e^-a` dropped above 40, `cosh`/`sinh`
/// overflowing while the product stays finite, the trig range's ends, a tiny trig factor.
#[test]
fn csinh_quick_tier_hard_cases() {
    let up = |v: f64| f64::from_bits(v.to_bits() + 1);
    let dn = |v: f64| f64::from_bits(v.to_bits() - 1);
    let half_pi = core::f64::consts::FRAC_PI_2;
    let xs = [0.125, dn(0.125), up(0.125), 40.0, up(40.0), 2f64.powi(-60), 709.0, 711.0, 745.9, 1e-10, 0.7];
    let ys = [half_pi, 3.0 * half_pi, 2f64.powi(-30), dn(1_048_576.0), 1.0, 1e-3];
    for x in xs.into_iter().flat_map(|x| [x, -x]) {
        for y in ys.into_iter().flat_map(|y| [y, -y]) {
            for mode in MODES {
                for (k, (f, route, _)) in FNS.into_iter().enumerate() {
                    let (a, b) = if k == 1 { (y, x) } else { (x, y) };
                    if let Some((v, _)) = f(a, b, mode) {
                        let (r, _) = route(a, b, mode);
                        assert_eq!((v.0.to_bits(), v.1.to_bits()), (r.0.to_bits(), r.1.to_bits()), "({a:e}, {b:e}) {mode:?}");
                    }
                }
            }
        }
    }
    // sinh 710.6 and cosh 710.6 overflow (2.07e308); times cos and sin of π/4 they do not: the
    // tier answers both parts.
    let r = csinh(710.6, core::f64::consts::FRAC_PI_4, Rounding::Nearest);
    assert!(r.is_some(), "the finite product is answered by the tier");
    let Some(((re, im), _)) = r else { return };
    assert!(re.is_finite() && im.is_finite() && re > 1e308 && im > 1e308, "{re:e} {im:e}");
}

/// `sinh a`, `cosh a` within `2^-100`: the even and odd Taylor series of `a/2^j` (`a/2^j <= 0.7`,
/// the `cexp` middle tier's coefficients with `z = −(a/2^j)²`), doubled `j` times.
fn reference(a: f64) -> (Dd, Dd) {
    let mut j = 0;
    while a * dd::pow2(-j) > 0.7 {
        j += 1;
    }
    let r = a * dd::pow2(-j);
    let z = dd::two_prod(r, r).neg();
    let (mut s, mut c) = (Dd::from(r).mul(dd::horner(z, &super::super::SIN_DD, 8)), dd::horner(z, &super::super::COS_DD, 9));
    for _ in 0..j {
        (s, c) = (s.mul(c).scale(2.0), s.mul(s).add(c.mul(c)));
    }
    (s, c)
}

#[test]
fn hyperbolic_factors_are_within_their_bounds() {
    let mut st = 0xfeed_u64;
    for i in 0..30_000 {
        let a = match i % 3 {
            0 => uniform(&mut st, -60.0, -3.0).exp2(),
            1 => uniform(&mut st, 0.125, 2.0),
            _ => uniform(&mut st, 2.0, 30.0),
        };
        let h = hyp(a);
        assert!(h.is_some(), "{a:e} is in the range of hyp");
        let Some((s, es, c, ec, m)) = h else { continue };
        let (rs, rc) = reference(a);
        let sc = dd::pow2(1 - m);
        for (v, e, r, name) in [(s, es, rs.scale(sc), "sinh"), (c, ec, rc.scale(sc), "cosh")] {
            let d = dd::abs((v.hi - r.hi) + (v.lo - r.lo));
            assert!(d <= e, "{name} {a:e}: error {d:e} above the bound {e:e}");
            assert!(e <= f64::from_bits(0x3bd0_0000_0000_0000) * v.hi, "{name} {a:e}: bound {e:e} above 2^-66");
        }
    }
}

/// The sweep (`cargo test --release -p liella-math --lib -- --ignored csinh_quick_tier_sweep`):
/// the comparison above on 8 threads, 10^6 operands per function and regime in all four modes.
#[test]
#[ignore = "long sweep: cargo xtask long-sweeps runs it in release (nightly and release runs)"]
fn csinh_quick_tier_sweep() {
    // Clauses: s10_5_c_0, s10_0_f_0.
    for (k, fns) in FNS.into_iter().enumerate() {
        let threads: Vec<_> = (0..8u64).map(|i| std::thread::spawn(move || compare(fns, 125_000, 0xc0ff_ee00 ^ (i + 1).wrapping_mul(0xd1b5_4a32_d192_ed03)))).collect();
        let mut total = [0usize; 5];
        for h in threads {
            let r = h.join();
            assert!(r.is_ok(), "a sweep thread failed (its panic message is above)");
            for (t, v) in total.iter_mut().zip(r.unwrap_or_default()) {
                *t += v;
            }
        }
        let name = if k == 0 { "csinh" } else { "csin" };
        std::println!("{name} quick tier sweep: answers per regime (nearest) {total:?} of 1000000 each");
    }
}
