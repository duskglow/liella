//! The quick `sin`, `cos`, `tan` phase, SSE2 build (§10.3 sin/cos/tan, D-10.3: an answer is the
//! correctly rounded result; D-10.51, D-10.61). The FMA build is checked through the product
//! (`crates/boundary/tests/trig_quick.rs`).

use super::plain::{assemble, cos_quick, reduce, series, sin_quick, sincos_quick, tan_quick};
use super::*;
use crate::dd;
use crate::mp::{self, Mp};
use crate::status::Rounding;

/// xorshift64 uniform in `[lo, hi)`.
fn uniform(n: usize, seed: u64, lo: f64, hi: f64) -> impl Iterator<Item = f64> {
    bits(n, seed).map(move |b| lo + (hi - lo) * (f64::from_bits(0x3ff0_0000_0000_0000 | (b >> 12)) - 1.0))
}

fn bits(n: usize, seed: u64) -> impl Iterator<Item = u64> {
    let mut st = seed;
    (0..n).map(move |_| {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        st
    })
}

/// Doubles with uniformly random bits in `[2^20, 2^1023]`, either sign (the huge path).
fn huge(n: usize, seed: u64) -> impl Iterator<Item = f64> {
    bits(n, seed).map(|b| f64::from_bits(((0x413 + (b >> 53) % (0x7fe - 0x413)) << 52) | (b & 0x800f_ffff_ffff_ffff)))
}

/// The correctly rounded results (the full path: today's tier, then the accurate path).
fn cr(which: usize, x: f64) -> f64 {
    match which {
        0 => crate::trig::sin(x, Rounding::Nearest).0,
        1 => crate::trig::cos(x, Rounding::Nearest).0,
        _ => crate::trig::tan(x, Rounding::Nearest).0,
    }
}

fn quick(which: usize, x: f64) -> Option<f64> {
    match which {
        0 => sin_quick(x),
        1 => cos_quick(x),
        _ => tan_quick(x),
    }
}

/// The bound's terms recomputed from the module doc (`S != 0` and `S = 0`), the multipliers'
/// conditions exactly, and the two ratios the terms rely on at their extreme (`n = 1`, `r = −π/128`).
#[test]
fn quick_constants_and_bounds() {
    let u = f64::powi(2.0, -53);
    let two = |e: i32| f64::powi(2.0, e);
    let sum_s = 2.0 * (-63.69f64).exp2() + (-63.66f64).exp2() + 6.8 * u * (-13.24f64).exp2() + (-66.2f64).exp2() + two(-70);
    let sum_0 = (-63.9f64).exp2() + 6.8 * u * (-13.24f64).exp2() + 3.0 * (-66.2f64).exp2();
    assert!(sum_s * 1.01 < RHO && sum_0 * 1.01 < RHO, "bound sums 2^{:.3}, 2^{:.3} against RHO 2^{:.3}", sum_s.log2(), sum_0.log2(), RHO.log2());
    // c·(1 − 2^54·rho) > 1 + 2^-53, exactly (dyadic values).
    for (c, rho) in [(C_Q, RHO), (super::C_TAN, two(-60))] {
        let lhs = Mp::from_f64(c).sub(Mp::from_f64(c).mul(Mp::from_f64(rho * two(54))));
        let rhs = Mp::from_f64(1.0).add(Mp::from_f64(u));
        assert!(lhs.cmp_abs(rhs).is_gt(), "multiplier {c:e} too small for rho {rho:e}");
    }
    assert!(2.0 * 0.875 * two(-61) + two(-99) < two(-60), "tan's bound");
    let (r, s, c, y) = (core::f64::consts::PI / 128.0, (core::f64::consts::PI / 64.0).sin(), (core::f64::consts::PI / 64.0).cos(), (core::f64::consts::PI / 128.0).sin());
    assert!(s <= 2.0 * y * (1.0 + 1e-12) && c * r <= y && y > (-5.35f64).exp2());
}

/// Every assembly within `RHO·|yh|` of the exact value (Mp), over the three reduction regimes;
/// the largest relative error seen is printed.
#[test]
fn quick_error_within_bound() {
    let mut worst = 0.0f64;
    let xs = uniform(20_000, 0x2545_f491_4f6c_dd1d, -10.0, 10.0).chain(uniform(10_000, 7, -1e6, 1e6)).chain(uniform(4000, 11, -1e-3, 1e-3)).chain(huge(10_000, 13));
    for x in xs {
        let Some((n, rh, rl)) = reduce(x) else { continue };
        let (ws, wc) = mp::sin_cos_f64(x);
        let p = series(rh, rl);
        for (i, want) in [(n, ws), (n + 32, wc)] {
            let (yh, yl, sh) = assemble(i, p);
            if sh == 0.0 && dd::abs(rh) < R26 {
                continue;
            }
            let err = want.sub(Mp::from_f64(yh)).sub(Mp::from_f64(yl));
            assert!(err.cmp_abs(Mp::from_f64(RHO * dd::abs(yh))).is_le(), "quick assembly at {x:e} ({:#x}), index {i}: error {:e} relative", x.to_bits(), err.to_f64() / yh);
            worst = worst.max(dd::abs(err.to_f64() / yh));
        }
    }
    eprintln!("quick sin/cos SSE2: largest relative error 2^{:.3} (bound 2^{:.3})", worst.log2(), RHO.log2());
    assert!(worst > 0.0);
}

/// Operands against the correctly rounded results: every answer bit for bit; on the cbench
/// operands (uniform [-10, 10]) the fallback within the model (`c − 1`: sin, cos 0.4%-1.2%,
/// tan 0.8%-2.5%).
#[test]
fn quick_matches_correctly_rounded_trig() {
    for which in 0..3 {
        for (set, xs) in [(0, uniform(1_000_000, [1, 2, 3][which], -10.0, 10.0).collect::<Vec<_>>()), (1, uniform(200_000, 5, -1e6, 1e6).collect()), (2, huge(200_000, 9).collect())] {
            let mut miss = 0usize;
            for &x in &xs {
                match quick(which, x) {
                    Some(v) => {
                        let e = cr(which, x);
                        assert_eq!(v.to_bits(), e.to_bits(), "quick {which} of {x:e} ({:#x}): {v:e}, correctly rounded {e:e}", x.to_bits());
                    }
                    None => miss += 1,
                }
            }
            let (lo, hi) = if which == 2 { (8, 25) } else { (4, 12) };
            if set == 0 {
                assert!(miss * 1000 >= xs.len() * lo && miss * 1000 <= xs.len() * hi, "function {which}: fallback {miss} of {}", xs.len());
            }
        }
    }
}

/// Hard cases: arguments next to multiples of `π/64` (zero or tiny table sine, the guard) and
/// next to `π/2` (tan's pole), and a huge double close to a multiple of `π/2` (Muller,
/// Handbook of Floating-Point Arithmetic, 2018, the `6381956970095103·2^797` case): the quick
/// phase declines or answers right; `sincos` agrees with `sin` and `cos`.
#[test]
fn quick_declines_or_rounds_right_on_hard_cases() {
    let mut xs = vec![6_381_956_970_095_103.0 * f64::powi(2.0, 797)];
    for k in 1..2000i32 {
        let m = f64::from(k) * core::f64::consts::FRAC_PI_2 / 32.0;
        let b = m.to_bits();
        xs.extend([f64::from_bits(b - 1), m, f64::from_bits(b + 1)]);
    }
    for x in xs.iter().flat_map(|&x| [x, -x]) {
        for which in 0..3 {
            if let Some(v) = quick(which, x) {
                assert_eq!(v.to_bits(), cr(which, x).to_bits(), "quick {which} of {x:e}");
            }
        }
        if let Some((s, c)) = sincos_quick(x) {
            assert_eq!((s.to_bits(), c.to_bits()), (cr(0, x).to_bits(), cr(1, x).to_bits()), "quick sincos of {x:e}");
        }
    }
}

/// Outside its domain (`|x| < 2^-26`, NaN, infinities) the quick phase declines.
#[test]
fn quick_domain() {
    for x in [0.0, -0.0, 1e-9, -1e-300, 5e-324, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(reduce(x).is_none(), "{x:e}");
    }
    assert!(reduce(1.5e-8).is_some() && reduce(1e300).is_some());
}
