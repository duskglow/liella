//! The quick `pow` phase, SSE2 build (§10.2 pow, D-10.3: an answer is the correctly rounded
//! result; D-10.51, D-10.62). The FMA build is checked through the boundary
//! (`crates/boundary/tests/pow_quick.rs`), which may call it.

use super::*;
use crate::fast64::quick::{RHO_FMA, RHO_SSE2};
use crate::mp::{self, Mp};
use crate::real::Real;
use crate::status::Rounding;

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

fn two(e: i32) -> f64 {
    f64::powi(2.0, e)
}

/// The correctly rounded `x^y`: today's tier (correctly rounded wherever it answers), else the
/// accurate path.
fn cr(x: f64, y: f64) -> f64 {
    super::super::pow_nearest(x, y).unwrap_or_else(|| Real::pow(x, y, Rounding::Nearest).0)
}

/// The operand sets: the cbench box (x in [0.01, 10], y in [-10, 10]); x near 1 with large y;
/// results near overflow; results near underflow (`y` from `±707/ln x`).
fn operands(n: usize) -> Vec<(f64, f64)> {
    let mut v: Vec<(f64, f64)> = uniform(n, 0x2545_f491_4f6c_dd1d, 0.01, 10.0).zip(uniform(n, 3, -10.0, 10.0)).collect();
    v.extend(uniform(n, 5, 1.0 - two(-8), 1.0 + two(-7)).zip(uniform(n, 7, -3000.0, 3000.0)));
    for (sign, seed) in [(1.0, 11u64), (-1.0, 13)] {
        v.extend(uniform(n, seed, 1.5, 10.0).zip(uniform(n, seed + 1, 0.0, 1e-3)).map(|(x, d)| (x, sign * 707.0 / x.ln() * (1.0 - d))));
    }
    v
}

/// `ln x` and `x^y` exactly enough (Mp).
fn mp_pow(x: f64, y: f64) -> Mp {
    mp::exp(mp::ln(Mp::from_f64(x)).mul(Mp::from_f64(y)))
}

/// The constants the proof relies on, the quick exp's bound recomputed for the folded reduction,
/// and the rounding test's `c` checked exactly at both ends of `|y·r3|` (module doc).
#[test]
fn pow_quick_constants_and_bounds() {
    assert_eq!(L10.to_bits(), (-1.0f64 / 10.0).to_bits(), "L10 is not RN(-1/10)");
    assert_eq!(KC, 2.75 * two(-53));
    let up = 1.0 + two(-40);
    let u = two(-53);
    let l = mp::ln2().div(Mp::from_f64(128.0));
    let l2 = -NL2;
    let cw = l.sub(Mp::from_f64(-NL1)).sub(Mp::from_f64(l2));
    // D-10.54's terms with |rh| <= R' and the reduction's roundings of the fold (module doc).
    let rh = core::f64::consts::LN_2 / 256.0 * (1.0 + two(-32)) * up + two(-61);
    let qmax = rh * rh * (0.5 + rh / 6.0 + rh * rh / 24.0 + rh * rh * rh / 120.0 + rh.powi(4) / 720.0) * (1.0 + 4.05 * u) * up;
    let prod = u * rh;
    let eval = 4.05 * u * qmax;
    let low = u * (qmax + two(-62)) * up;
    let rlrh = (two(-62) + two(-79)) * (rh.exp() - 1.0) * up;
    let tl_term = u * (qmax + two(-61)) * up;
    let trunc = rh.powi(7) / 5040.0 * rh.exp() * up;
    let red = (130_740.0 * cw.abs().to_f64() + 3.0 * u * (130_740.0 * dd::abs(l2) + two(-41)) + u * two(-61)) * rh.exp() * up;
    let small = two(-105) * rh.exp() + two(-104);
    let zmin = (-rh).exp() * (1.0 - two(-60)) / up;
    let fma = (eval + 3.0 * low + rlrh + tl_term + trunc + red + small) / zmin;
    let sse2 = (prod + eval + 4.0 * low + rlrh + tl_term + trunc + red + small) / zmin;
    assert!(fma <= RHO_FMA, "FMA bound {fma:e} above RHO_FMA");
    assert!(sse2 <= RHO_SSE2, "SSE2 bound {sse2:e} above RHO_SSE2");
    // c = RN(|y·r3|·A2 + C0) >= C0 + A2·|y·r3|·(1 − u) − u; rho = RHO_E + (KC'·|y·r3| + 2^-83·708)·(1 + 2^-38).
    let one = Mp::from_f64(1.0);
    let one_u = one.add(Mp::from_f64(u));
    let kc = Mp::from_f64(KC).mul(Mp::from_f64(1.0 + two(-49)));
    let grow = Mp::from_f64(1.0 + two(-38));
    for (c0, rho_e) in [(C0_SSE2, RHO_SSE2), (C0_FMA, RHO_FMA)] {
        for yr in [0.0, YR_MAX] {
            let c = Mp::from_f64(c0).add(Mp::from_f64(A2).mul(Mp::from_f64(yr)).mul(Mp::from_f64(1.0 - u))).sub(Mp::from_f64(u));
            let d = kc.mul(Mp::from_f64(yr)).add(Mp::from_f64(two(-83)).mul(Mp::from_f64(708.0))).mul(grow);
            let rho = Mp::from_f64(rho_e).add(d);
            assert!(rho.mul(Mp::from_f64(two(54))).cmp_abs(Mp::from_f64(two(-7))).is_le(), "2^54·rho above 2^-7 at |y·r3| = {yr:e}");
            let k = one.sub(rho.mul(Mp::from_f64(two(54))));
            assert!(c.mul(k).cmp_abs(one_u).is_gt(), "c below the bound at |y·r3| = {yr:e} (C0 {c0:e})");
        }
    }
}

/// The SSE2 build's `lh + ll` within `KC·|r3|·(1 + 2^-50) + 2^-84·|lh|` of `ln x` (exact in Mp)
/// near 1 (case A), on `[0.5, 2)` (B, C) and over `[1e-3, 1e3)`; the largest error is printed.
#[test]
fn pow_log_error_within_bound() {
    let xs = uniform(60_000, 3, 1.0 - two(-8), 1.0 + two(-7)).chain(uniform(60_000, 5, 0.5, 2.0)).chain(uniform(60_000, 7, 1e-3, 1e3));
    let mut worst = 0.0f64;
    for x in xs {
        if x == 1.0 {
            continue;
        }
        let (lh, ll, r3) = pow_log_parts(x);
        let err = mp::ln(Mp::from_f64(x)).sub(Mp::from_f64(lh)).sub(Mp::from_f64(ll));
        let bound = KC * dd::abs(r3) * (1.0 + two(-50)) + two(-84) * dd::abs(lh);
        assert!(err.cmp_abs(Mp::from_f64(bound)).is_le(), "pow log({x:e}): error {:e} above {bound:e}", err.to_f64());
        worst = worst.max(dd::abs(err.to_f64()) / bound);
    }
    eprintln!("quick pow log SSE2: largest error / bound {worst:.3}");
    assert!(worst > 0.0);
}

/// The SSE2 build's `(zh + zl)·sc` within `rho·|zh|·sc` of `x^y` (exact in Mp), `rho` the
/// composed bound (module doc), over the operand sets.
#[test]
fn pow_quick_error_within_bound() {
    let mut worst = 0.0f64;
    for (x, y) in operands(5000) {
        if !pow_domain(x, y) {
            continue;
        }
        let Some((zh, zl, sc, _)) = pow_quick_parts(x, y) else { continue };
        let (lh, _, r3) = pow_log_parts(x);
        let rho = (RHO_SSE2 + (KC * (1.0 + two(-49)) * dd::abs(y * r3) + two(-83) * dd::abs(y * lh))) * (1.0 + two(-37));
        let err = mp_pow(x, y).mul(Mp::from_f64(1.0 / sc)).sub(Mp::from_f64(zh)).sub(Mp::from_f64(zl));
        assert!(err.cmp_abs(Mp::from_f64(rho * dd::abs(zh))).is_le(), "quick pow({x:e}, {y:e}): error {:e} above rho {rho:e}", err.to_f64() / zh);
        worst = worst.max(dd::abs(err.to_f64() / zh));
    }
    eprintln!("quick pow SSE2: largest relative error 2^{:.3}", worst.log2());
    assert!(worst > 0.0);
}

/// Every answer against the correctly rounded `pow`, bit for bit, over 4·10^5 operands; the
/// unchecked `zh·sc` is wrong on some of them (so the test is not vacuous) and every such operand
/// is declined; the fallback on the cbench box is within the model (SSE2 `c − 1 = 0.57%`:
/// asserted 0.1%-1.5%).
#[test]
fn pow_quick_matches_correctly_rounded_pow() {
    let n = 100_000usize;
    let (mut miss, mut wrong) = (0usize, 0usize);
    for (i, (x, y)) in operands(n).into_iter().enumerate() {
        let e = cr(x, y);
        if pow_domain(x, y) {
            if let Some((zh, _, sc, _)) = pow_quick_parts(x, y) {
                wrong += usize::from((zh * sc).to_bits() != e.to_bits());
            }
        }
        match pow_quick(x, y) {
            Some(v) => assert_eq!(v.to_bits(), e.to_bits(), "quick pow({x:e}, {y:e}): {v:e}, correctly rounded {e:e}"),
            None if i < n => miss += 1,
            None => {}
        }
    }
    eprintln!("quick pow SSE2: fallback {miss} of {n} on the cbench box; unchecked zh wrong on {wrong}");
    assert!(wrong > 0, "no operand with a wrong unchecked zh: the rounding test is not exercised");
    assert!(miss * 1000 >= n && miss * 1000 <= n * 15, "fallback {miss} of {n} outside 0.1%-1.5%");
}

/// Exact and special cases are refused before the quick phase: `x^y` a `double` or a midpoint
/// (`y·2^11` an integer, `x` a power of two), `x = 1`, `x <= 0`, NaN, infinities, subnormal `x`,
/// tiny `y`; and results past the exp domain decline.
#[test]
fn pow_quick_declines_exact_and_special_cases() {
    let exact = [(4.0, 0.5), (9.0, 0.5), (4.0, 1.5), (2.0, 0.75), (1.0 + two(-27), 2.0), (3.0, 2.0), (16.0, 0.25), (2.0, 10.5)];
    for (x, y) in exact {
        assert_eq!(pow_quick(x, y), None, "quick pow must refuse the exact case ({x:e}, {y:e})");
    }
    for (x, y) in [(1.0, 0.3), (0.0, 0.3), (-2.0, 0.3), (f64::NAN, 0.3), (2.0, f64::NAN), (f64::INFINITY, 0.3), (5e-324, 0.3), (2.0, 1e-70), (10.0, 400.3), (10.0, -400.3)] {
        assert_eq!(pow_quick(x, y), None, "quick pow must decline ({x:e}, {y:e})");
    }
    assert!(pow_quick(1.5, 0.3).is_some(), "quick pow declined an ordinary operand");
}
