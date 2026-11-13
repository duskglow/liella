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
/// answers equal the route's; returns the answers per regime (nearest). The regimes shape the
/// hyperbolic argument, so `csin` (`swap`) takes the operand with its parts exchanged.
fn compare((f, route, nearest, swap): Fns, n: usize, seed: u64) -> [usize; 5] {
    let mut out = [0; 5];
    for (regime, o) in out.iter_mut().enumerate() {
        let mut st = seed ^ (regime as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        for _ in 0..n {
            let (h, t) = operand(&mut st, regime);
            let (x, y) = if swap { (t, h) } else { (h, t) };
            for mode in MODES {
                let Some(((a, b), st)) = f(x, y, mode) else { continue };
                let ((ra, rb), rs) = route(x, y, mode);
                assert_eq!((a.to_bits(), b.to_bits()), (ra.to_bits(), rb.to_bits()), "({x:e}, {y:e}) {mode:?}: tier ({a:e}, {b:e}) route ({ra:e}, {rb:e})");
                assert_eq!(st, rs, "({x:e}, {y:e}) {mode:?}: the status");
                if mode == Rounding::Nearest {
                    *o += 1;
                    // The in-place entry answers what its quick test decides and declines the rest
                    // (overflows and middle-tier parts go through the cold fallback).
                    if let Some(v) = nearest(x, y) {
                        assert_eq!((v.0.to_bits(), v.1.to_bits()), (a.to_bits(), b.to_bits()), "({x:e}, {y:e}): the in-place entry");
                    }
                }
            }
        }
    }
    out
}

type Fns = (fn(f64, f64, Rounding) -> Option<((f64, f64), Status)>, fn(f64, f64, Rounding) -> ((f64, f64), Status), fn(f64, f64) -> Option<(f64, f64)>, bool);

/// `csinh` and `csin`: the tier, the route, the in-place entry, whether the hyperbolic argument
/// is the imaginary part.
const FNS: [Fns; 2] = [(csinh, csinh_route::<f64>, csinh_nearest, false), (csin, csin_route::<f64>, csin_nearest, true)];

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
                for (f, route, _, swap) in FNS {
                    let (a, b) = if swap { (y, x) } else { (x, y) };
                    if let Some((v, st)) = f(a, b, mode) {
                        let (r, rs) = route(a, b, mode);
                        assert_eq!((v.0.to_bits(), v.1.to_bits()), (r.0.to_bits(), r.1.to_bits()), "({a:e}, {b:e}) {mode:?}");
                        assert_eq!(st, rs, "({a:e}, {b:e}) {mode:?}: the status");
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

/// Overflowing parts: `|x|` in `[709, 746]` (the results cross `2^1023` and `2^1024` there), trig
/// factors from `2^-20` to `1`, both signs, every mode: where the tier answers, its values and
/// status (overflow, inexact, `ERANGE`) are the route's; a value between `DBL_MAX` and `2^1024` in
/// nearest must not become `∞`, nor a finite one above `2^1023`.
#[test]
fn csinh_tier_answers_overflowing_parts_as_the_route() {
    // Clauses: s10_5_c_0, s10_0_f_0.
    let mut st = 0x0bad_cafe_u64;
    let (mut answered, mut overflowed, mut total) = (0, 0, 0);
    for i in 0..300 {
        let x = uniform(&mut st, 709.0, 746.0) * if i % 2 == 0 { 1.0 } else { -1.0 };
        let y = match i % 3 {
            0 => uniform(&mut st, -20.0, 0.0).exp2(),
            1 => -uniform(&mut st, 0.0, 1.5),
            _ => uniform(&mut st, 1.6, 3.1),
        };
        for mode in MODES {
            for (f, route, _, swap) in FNS {
                let (a, b) = if swap { (y, x) } else { (x, y) };
                total += 1;
                let Some((v, stat)) = f(a, b, mode) else { continue };
                let (r, rs) = route(a, b, mode);
                assert_eq!((v.0.to_bits(), v.1.to_bits()), (r.0.to_bits(), r.1.to_bits()), "({a:e}, {b:e}) {mode:?}");
                assert_eq!(stat, rs, "({a:e}, {b:e}) {mode:?}: the status");
                answered += 1;
                overflowed += usize::from(stat.flags.any(Flags::OVERFLOW));
            }
        }
    }
    assert!(answered * 100 >= total * 99, "answered {answered} of {total}");
    assert!(overflowed * 2 >= total, "overflows answered {overflowed} of {total}");
}

/// The middle tier against the 384-bit arithmetic over the sweep's regimes: the trig factor
/// within `2^-99·|T.hi| + 2^-137` (next to multiples of `π/2` too), the hyperbolic factors within
/// `2^-96.8` relative, each part within its `eps`; and its answers, forced for every part, are
/// the route's in every mode (a decline only for a result that is not a normal `double`).
#[test]
fn csinh_middle_tier_bounds_and_answers() {
    // Clauses: s10_5_c_0, s10_0_f_0.
    use crate::mp::{self, Mp};
    let of = |d: Dd| Mp::from_f64(d.hi).add(Mp::from_f64(d.lo));
    let (rel_t, abs_t, rel_h) = ((-99f64).exp2(), (-137f64).exp2(), (-96.8f64).exp2());
    let mut st = 0x5151_u64;
    let (mut answered, mut total, mut tight) = (0, 0, 0);
    for i in 0..1000 {
        let (h, t) = operand(&mut st, i % 5);
        let Some((sh, ch, s)) = mid_hyp(h.abs()) else { continue };
        let (rsh, rch) = mp::sinh_cosh(Mp::from_f64(h.abs()));
        for (v, r, name) in [(sh, rsh, "sinh"), (ch, rch, "cosh")] {
            let d = r.ldexp(-i64::from(s)).sub(of(v)).to_f64().abs();
            assert!(d <= rel_h * v.hi, "{name} {h:e}: error {d:e} above 2^-96.8 of {:e}", v.hi);
        }
        let (rs, rc) = mp::sin_cos_f64(t);
        let rsh = if h < 0.0 { rsh.neg() } else { rsh };
        for (part, hv, tv) in [(Part::Re, rsh, rc), (Part::Im, rch, rs)] {
            let v = sin_or_cos(t, part);
            let terr = tv.sub(of(v)).to_f64().abs();
            assert!(terr <= rel_t * v.hi.abs() + abs_t, "t = {t:e} {part:?}: trig error {terr:e} for {:e}", v.hi);
            let Some((p, eps, m)) = mid_part(h, t, part) else { continue };
            let exact = hv.mul(tv).ldexp(-i64::from(m));
            let err = exact.sub(of(p)).to_f64().abs();
            assert!(err <= eps, "({h:e}, {t:e}) {part:?}: error {err:e} above the bound {eps:e}");
            tight += usize::from(err > eps / 65536.0);
            for mode in MODES {
                let (r, _) = csinh_route::<f64>(h, t, mode);
                let want = if part == Part::Re { r.0 } else { r.1 };
                if !want.is_normal() {
                    continue;
                }
                total += 1;
                if let Some(got) = middle_round(p, eps, m, mode) {
                    assert_eq!(got.to_bits(), want.to_bits(), "({h:e}, {t:e}) {part:?} {mode:?}: middle {got:e} route {want:e}");
                    answered += 1;
                }
            }
        }
    }
    assert!(answered + total / 1000 >= total, "the middle tier answered {answered} of {total}");
    assert!(tight > 0, "every middle error far below its bound");
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
