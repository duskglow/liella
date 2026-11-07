//! The quick `exp` phase, SSE2 build (§10.2 exp, D-10.3: an answer is the correctly rounded
//! result; D-10.51, D-10.54). The FMA build is checked through the boundary
//! (`crates/boundary/tests/exp_quick.rs`), which may call it.

use super::*;
use crate::dd;
use crate::mp::{self, Mp};
use crate::real::Real;
use crate::status::Rounding;

/// xorshift64 uniform in `[lo, hi)`.
fn uniform(n: usize, seed: u64, lo: f64, hi: f64) -> impl Iterator<Item = f64> {
    let mut st = seed;
    (0..n).map(move |_| {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        lo + (hi - lo) * (f64::from_bits(0x3ff0_0000_0000_0000 | (st >> 12)) - 1.0)
    })
}

/// The correctly rounded `e^x`: today's tier (correctly rounded wherever it answers), else the
/// accurate path.
fn cr(x: f64) -> f64 {
    super::super::exp_nearest(x).unwrap_or_else(|| Real::exp(x, Rounding::Nearest).0)
}

fn two(e: i32) -> f64 {
    f64::powi(2.0, e)
}

/// The constants the proof relies on, and the bound recomputed from them (module doc, terms
/// 1-6; each term computed in `f64` from its formula, rounded up by the factor `1 + 2^-40`).
#[test]
fn quick_constants_and_bounds() {
    for (c, n, name) in [(C3, 6.0f64, "C3"), (C4, 24.0, "C4"), (C5, 120.0, "C5"), (C6, 720.0, "C6")] {
        assert_eq!(c.to_bits(), (1.0 / n).to_bits(), "{name} is not RN(1/{n})");
    }
    let l = mp::ln2().div(Mp::from_f64(128.0));
    let (l1, l2) = (-NL1, -NL2);
    assert_eq!(l1.to_bits() & ((1 << 17) - 1), 0, "L1 has more than 36 bits");
    assert!(dd::abs(l2) <= two(-44), "L2 above 2^-44");
    let cw = l.sub(Mp::from_f64(l1)).sub(Mp::from_f64(l2));
    assert!(cw.cmp_abs(Mp::from_f64(two(-97))).is_le(), "L1 + L2 not within 2^-97 of ln2/128");
    let inv = Mp::from_f64(128.0).div(mp::ln2());
    assert!(inv.sub(Mp::from_f64(INV)).cmp_abs(Mp::from_f64(INV * two(-53))).is_le(), "INV is not RN(128/ln2)");
    for j in 0..128 {
        let th = f64::from_bits(t::EXP2_J128[j][0]);
        let tl = f64::from_bits(t::EXP2_J128[j][1]);
        let exact = mp::exp(Mp::from_f64(f64::from(u8::try_from(j).unwrap_or(0)) / 128.0).mul(mp::ln2()));
        let err = exact.sub(Mp::from_f64(th)).sub(Mp::from_f64(tl));
        assert!(err.cmp_abs(Mp::from_f64(th * two(-105))).is_le(), "EXP2_J128[{j}] not within 2^-105");
        assert!(dd::abs(tl) <= th * two(-53), "EXP2_J128[{j}]: low part above 2^-53·th");
    }
    // The terms, relative to th.
    let up = 1.0 + two(-40);
    let u = two(-53);
    let r = core::f64::consts::LN_2 / 256.0 * (1.0 + two(-34)) * up;
    let rh = r + two(-62);
    let qmax = rh * rh * (0.5 + rh / 6.0 + rh * rh / 24.0 + rh * rh * rh / 120.0 + rh.powi(4) / 720.0) * (1.0 + 4.05 * u) * up;
    let prod = u * rh;
    let eval = 4.05 * u * qmax;
    let low = u * (qmax + two(-62)) * up;
    let rlrh = (two(-62) + two(-79)) * (rh.exp() - 1.0) * up;
    let tl_term = u * (qmax + two(-61)) * up;
    let trunc = rh.powi(7) / 5040.0 * rh.exp() * up;
    // |kd| <= 130740: |kd|·|ln2/128 − L1 − L2| plus the two roundings forming rl (each u·2^-27).
    let red = (130_740.0 * cw.abs().to_f64() + 2.0 * u * 130_740.0 * dd::abs(l2) + u * two(-62)) * rh.exp() * up;
    let small = two(-105) * rh.exp() + two(-104);
    let zmin = (-rh).exp() * (1.0 - two(-60)) / up;
    let fma = (eval + 3.0 * low + rlrh + tl_term + trunc + red + small) / zmin;
    let sse2 = (prod + eval + 4.0 * low + rlrh + tl_term + trunc + red + small) / zmin;
    assert!(fma <= RHO_FMA, "FMA bound {fma:e} above RHO_FMA");
    assert!(sse2 <= RHO_SSE2, "SSE2 bound {sse2:e} above RHO_SSE2");
    // Each multiplier is the least double with c·(1 − 2^54·rho) > 1 + 2^-53 (exact in Mp).
    let one_u = Mp::from_f64(1.0).add(Mp::from_f64(u));
    for (c, rho) in [(C_SSE2, RHO_SSE2), (C_FMA, RHO_FMA)] {
        let k = Mp::from_f64(1.0).sub(Mp::from_f64(rho * two(54)));
        assert!(Mp::from_f64(c).mul(k).cmp_abs(one_u).is_gt(), "{c:e} below its bound");
        assert!(Mp::from_f64(f64::from_bits(c.to_bits() - 1)).mul(k).cmp_abs(one_u).is_le(), "{c:e} not the least");
    }
}

/// The SSE2 build's `(zh, zl)` within `RHO_SSE2·|zh|` of `e^x/2^m` (exact in Mp), and the
/// largest relative error seen (printed; the stated bound is 2^-61.476).
#[test]
fn quick_error_within_bound() {
    let mut worst = 0.0f64;
    let xs = uniform(20_000, 0x2545_f491_4f6c_dd1d, -708.0, 708.0).chain(uniform(20_000, 7, -1.0, 1.0)).chain(uniform(4000, 11, -1e-9, 1e-9));
    for x in xs {
        let Some((zh, zl, sc)) = exp_quick_parts(x) else { continue };
        let want = mp::exp(Mp::from_f64(x)).mul(Mp::from_f64(1.0 / sc));
        let err = want.sub(Mp::from_f64(zh)).sub(Mp::from_f64(zl));
        assert!(err.cmp_abs(Mp::from_f64(RHO_SSE2 * dd::abs(zh))).is_le(), "quick exp({x:e}): error {:e} above the bound", err.to_f64() / zh);
        worst = worst.max(dd::abs(err.to_f64() / zh));
    }
    eprintln!("quick exp SSE2: largest relative error 2^{:.3} (bound 2^{:.3})", worst.log2(), RHO_SSE2.log2());
    assert!(worst > 0.0);
}

/// 10^7 operands against the correctly rounded `exp`: every answer bit for bit; the fallback rate
/// on the cbench operands (uniform [-10, 10]) within the model (0.4%-0.8%).
#[test]
fn quick_matches_correctly_rounded_exp() {
    let sets: [(usize, u64, f64, f64); 4] = [(4_000_000, 1, -10.0, 10.0), (4_000_000, 2, -708.0, 708.0), (1_000_000, 3, -0.01, 0.01), (1_000_000, 4, -745.2, 709.8)];
    for (n, seed, lo, hi) in sets {
        let mut miss = 0usize;
        for x in uniform(n, seed, lo, hi) {
            match exp_quick(x) {
                Some(v) => {
                    let e = cr(x);
                    assert_eq!(v.to_bits(), e.to_bits(), "quick exp({x:e} = {:#x}): {v:e}, correctly rounded {e:e}", x.to_bits());
                }
                None => miss += 1,
            }
        }
        if seed == 1 {
            assert!(miss * 1000 >= n * 4 && miss * 1000 <= n * 8, "fallback rate {miss} of {n} outside 0.4%-0.8%");
        }
    }
}

/// Hard cases: `e^(k·2^-53)` next to midpoints and doubles (as in `tests/fast64.rs`), and the
/// operands of a seeded sample whose quick result rounded without the test is wrong (found here,
/// so the test cannot pass vacuously): the quick phase must decline each or answer right.
#[test]
fn quick_declines_or_rounds_right_on_hard_cases() {
    let mut hard: Vec<f64> = Vec::new();
    for k in 1..=48u32 {
        let d = f64::from(k) * f64::EPSILON;
        hard.extend([0.5 * d, -0.5 * d, -0.25 * d]);
    }
    let mut wrong = 0usize;
    for x in uniform(400_000, 0x9e37_79b9_7f4a_7c15, -708.0, 708.0) {
        if let Some((zh, zl, sc)) = exp_quick_parts(x) {
            if (zh + zl) * sc != cr(x) {
                wrong += 1;
                hard.push(x);
            }
        }
    }
    assert!(wrong >= 20, "only {wrong} raw misroundings found: the hard set is too small");
    for x in hard {
        if let Some(v) = exp_quick(x) {
            let e = cr(x);
            assert_eq!(v.to_bits(), e.to_bits(), "hard case exp({x:e} = {:#x}): quick {v:e}, correctly rounded {e:e}", x.to_bits());
        }
    }
}

/// Outside `2^-60 <= |x| < 708` the quick phase declines (today's tier decides).
#[test]
fn quick_domain() {
    let tiny = two(-60);
    for x in [0.0, -0.0, f64::from_bits(tiny.to_bits() - 1), 708.0, -708.0, 1e300, f64::INFINITY, f64::NEG_INFINITY, f64::NAN, 5e-324] {
        assert!(exp_quick(x).is_none(), "quick exp answered outside its domain at {x:e}");
    }
    for x in [tiny, -tiny, f64::from_bits(708f64.to_bits() - 1), -f64::from_bits(708f64.to_bits() - 1)] {
        assert!(exp_quick_parts(x).is_some(), "quick exp refused {x:e} inside its domain");
    }
}
