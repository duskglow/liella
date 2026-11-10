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

// exp2 and exp10 on the same kernel (LIBM-F64-EXP-FAMILY-QUICK, D-10.64), SSE2 build. Clauses:
// §10.2 exp2/exp10, D-10.3 (an answer is the correctly rounded result).

/// The correctly rounded `2^x` / `10^x`: today's tier, else the accurate path.
fn cr2(x: f64) -> f64 {
    super::super::exp2_nearest(x).unwrap_or_else(|| Real::exp2(x, Rounding::Nearest).0)
}
fn cr10(x: f64) -> f64 {
    super::super::exp10_nearest(x).unwrap_or_else(|| Real::exp10(x, Rounding::Nearest).0)
}

type Parts = fn(f64) -> Option<(f64, f64, f64)>;
type Quick = fn(f64) -> Option<f64>;
/// `(name, parts, quick, correctly rounded, exact log of the base)`.
type Member = (&'static str, Parts, Quick, fn(f64) -> f64, Mp);
fn family() -> [Member; 2] {
    [("exp2", exp2_quick_parts, exp2_quick, cr2, mp::ln2()), ("exp10", exp10_quick_parts, exp10_quick, cr10, mp::ln10())]
}

/// The exp2/exp10 constants the proof relies on, and their bounds recomputed from them (module
/// doc, "exp2 and exp10"; terms in `f64` from their formulas, rounded up by `1 + 2^-40`).
#[test]
fn family_constants_and_bounds() {
    for (h, l, exact, name) in [(LN2H, LN2L, mp::ln2(), "ln2"), (LN10H, LN10L, mp::ln10(), "ln10")] {
        assert!(exact.sub(Mp::from_f64(h)).sub(Mp::from_f64(l)).cmp_abs(Mp::from_f64(h * two(-106))).is_le(), "{name} pair not within 2^-106");
        assert!(dd::abs(l) < two(-52), "{name} low part above 2^-52");
    }
    let log10_2 = mp::ln2().div(mp::ln10());
    let inv = Mp::from_f64(128.0).div(log10_2);
    assert!(inv.sub(Mp::from_f64(INV10)).cmp_abs(Mp::from_f64(INV10 * two(-53))).is_le(), "INV10 is not RN(128/log10 2)");
    let (e0, e1, e2) = (-NE0, -NE1, -NE2);
    assert_eq!(e0.to_bits() & ((1 << 21) - 1), 0, "E0 has more than 32 bits");
    assert_eq!(e1.to_bits() & ((1 << 17) - 1), 0, "E1 has more than 36 bits");
    assert_eq!(e2.to_bits() & ((1 << 17) - 1), 0, "E2 has more than 36 bits");
    let cw10 = log10_2.div(Mp::from_f64(128.0)).sub(Mp::from_f64(e0)).sub(Mp::from_f64(e1)).sub(Mp::from_f64(e2));
    assert!(cw10.cmp_abs(Mp::from_f64(two(-121))).is_le(), "E0 + E1 + E2 not within 2^-121 of log10(2)/128");
    assert!(130_537.0 * cw10.abs().to_f64() < two(-104), "exp10 reduction above 2^-104");
    let up = 1.0 + two(-40);
    let u = two(-53);
    let rh = core::f64::consts::LN_2 / 256.0 * (1.0 + two(-34)) * up + two(-62);
    let qmax = rh * rh * (0.5 + rh / 6.0 + rh * rh / 24.0 + rh * rh * rh / 120.0 + rh.powi(4) / 720.0) * (1.0 + 4.05 * u) * up;
    let er = rh.exp();
    let total = |rl: f64, red: f64, nlow: f64, prod: f64| {
        let low = u * (qmax + rl) * up;
        let rest = 4.05 * u * qmax + rl * (er - 1.0) * up + u * (qmax + two(-61)) * up + rh.powi(7) / 5040.0 * er * up;
        (prod + nlow * low + rest + red + two(-105) * er + two(-104)) / ((-rh).exp() * (1.0 - two(-60)) / up)
    };
    let sh = 0.001_176;
    assert!(Mp::from_f64(sh).cmp_abs(log10_2.div(Mp::from_f64(128.0)).mul(Mp::from_f64(0.5 + two(-35)))).is_gt(), "|sh| bound");
    let rl2s = two(-8) * dd::abs(LN2L) * (1.0 + 2.0 * u);
    let rl10s = (sh * dd::abs(LN10L) + two(-62) * 0.536 * LN10H) * (1.0 + 3.0 * u);
    let (rl2f, rl10f) = (rl2s + two(-62), rl10s + two(-62));
    let red2 = (two(-114) + 2.0 * u * rl2f + two(-120)) * er * up;
    let red10 = (two(-104) * 2.31 + sh * two(-106) + two(-63) * dd::abs(LN10L) + 3.0 * u * rl10f) * er * up;
    let prod = 2.0 * u * rh * er;
    for (name, fma, sse2) in [("exp2", total(rl2f, red2, 3.0, 0.0), total(rl2s, red2, 4.0, prod)), ("exp10", total(rl10f, red10, 3.0, 0.0), total(rl10s, red10, 4.0, prod))] {
        assert!(fma <= RHO_FAM_FMA, "{name} FMA bound {fma:e} above RHO_FAM_FMA");
        assert!(sse2 <= RHO_FAM_SSE2, "{name} SSE2 bound {sse2:e} above RHO_FAM_SSE2");
    }
    let one_u = Mp::from_f64(1.0).add(Mp::from_f64(u));
    for (c, rho) in [(C_FAM_SSE2, RHO_FAM_SSE2), (C_FAM_FMA, RHO_FAM_FMA)] {
        let k = Mp::from_f64(1.0).sub(Mp::from_f64(rho * two(54)));
        assert!(Mp::from_f64(c).mul(k).cmp_abs(one_u).is_gt(), "{c:e} below its bound");
        assert!(Mp::from_f64(f64::from_bits(c.to_bits() - 1)).mul(k).cmp_abs(one_u).is_le(), "{c:e} not the least");
    }
}

/// The SSE2 build's exp2/exp10 `(zh, zl)` within `RHO_FAM_SSE2·|zh|` of the exact value over the
/// scale (exact in Mp); the largest relative error seen is printed.
#[test]
fn family_error_within_bound() {
    for (name, parts, _, _, lnb) in family() {
        let top = if name == "exp2" { 1021.0 } else { 307.0 };
        let mut worst = 0.0f64;
        let xs = uniform(20_000, 0x2545_f491_4f6c_dd1d, -top, top).chain(uniform(20_000, 7, -1.0, 1.0)).chain(uniform(4000, 11, -1e-9, 1e-9));
        for x in xs {
            let Some((zh, zl, sc)) = parts(x) else { continue };
            let want = mp::exp(Mp::from_f64(x).mul(lnb)).mul(Mp::from_f64(1.0 / sc));
            let err = want.sub(Mp::from_f64(zh)).sub(Mp::from_f64(zl));
            assert!(err.cmp_abs(Mp::from_f64(RHO_FAM_SSE2 * dd::abs(zh))).is_le(), "quick {name}({x:e}): error {:e} above the bound", err.to_f64() / zh);
            worst = worst.max(dd::abs(err.to_f64() / zh));
        }
        eprintln!("quick {name} SSE2: largest relative error 2^{:.3} (bound 2^{:.3})", worst.log2(), RHO_FAM_SSE2.log2());
        assert!(worst > 0.0);
    }
}

/// 6·10^6 operands per function against the correctly rounded result: every answer bit for bit;
/// the fallback rate on uniform [-10, 10] within the model (0.8%-1.6%, expected 1.14%).
#[test]
fn family_matches_correctly_rounded() {
    for (name, _, quick, cr, _) in family() {
        let top = if name == "exp2" { 1021.0 } else { 307.0 };
        let sets: [(usize, u64, f64, f64); 3] = [(3_000_000, 1, -10.0, 10.0), (2_000_000, 2, -top, top), (1_000_000, 3, -0.01, 0.01)];
        for (n, seed, lo, hi) in sets {
            let mut miss = 0usize;
            for x in uniform(n, seed, lo, hi) {
                match quick(x) {
                    Some(v) => {
                        let e = cr(x);
                        assert_eq!(v.to_bits(), e.to_bits(), "quick {name}({x:e} = {:#x}): {v:e}, correctly rounded {e:e}", x.to_bits());
                    }
                    None => miss += 1,
                }
            }
            if seed == 1 {
                assert!(miss * 1000 >= n * 8 && miss * 1000 <= n * 16, "{name} fallback rate {miss} of {n} outside 0.8%-1.6%");
            }
        }
    }
}

/// Hard cases: operands of a seeded sample whose quick result rounded without the test is wrong
/// (so the test cannot pass vacuously), small `|x|`, and the exact cases (integers for exp2,
/// `1 ..= 23` for exp10), which the quick phase must decline.
#[test]
fn family_declines_or_rounds_right_on_hard_cases() {
    for (name, parts, quick, cr, _) in family() {
        let mut hard: Vec<f64> = Vec::new();
        for k in 1..=48u32 {
            let d = f64::from(k) * f64::EPSILON;
            hard.extend([0.5 * d, -0.5 * d, -0.25 * d]);
        }
        let mut wrong = 0usize;
        for x in uniform(400_000, 0x9e37_79b9_7f4a_7c15, -300.0, 300.0) {
            if let Some((zh, zl, sc)) = parts(x) {
                if (zh + zl) * sc != cr(x) {
                    wrong += 1;
                    hard.push(x);
                }
            }
        }
        assert!(wrong >= 20, "{name}: only {wrong} raw misroundings found: the hard set is too small");
        for x in hard {
            if let Some(v) = quick(x) {
                let e = cr(x);
                assert_eq!(v.to_bits(), e.to_bits(), "hard case {name}({x:e} = {:#x}): quick {v:e}, correctly rounded {e:e}", x.to_bits());
            }
        }
    }
    for n in -1020..=1020i32 {
        assert!(exp2_quick(f64::from(n)).is_none(), "quick exp2 answered the exact case {n}");
    }
    for n in 1..=23i32 {
        assert!(exp10_quick_parts(f64::from(n)).is_none(), "quick exp10 did not decline the exact case {n}");
    }
    for x in [24.0, 0.5, -1.0, -22.0, 22.5, 1.0 + f64::EPSILON] {
        assert!(exp10_quick_parts(x).is_some(), "quick exp10 declined {x:e}, not an exact case");
    }
}

/// Outside their domains the quick phases decline.
#[test]
fn family_domain() {
    let tiny = two(-60);
    for (name, parts, _, _, _) in family() {
        let top: f64 = if name == "exp2" { 1021.0 } else { 307.0 };
        for x in [0.0, -0.0, f64::from_bits(tiny.to_bits() - 1), top, -top, 1e300, f64::INFINITY, f64::NEG_INFINITY, f64::NAN, 5e-324] {
            assert!(parts(x).is_none(), "quick {name} answered outside its domain at {x:e}");
        }
        for x in [tiny, -tiny, f64::from_bits(top.to_bits() - 1), -f64::from_bits(top.to_bits() - 1)] {
            assert!(parts(x).is_some(), "quick {name} refused {x:e} inside its domain");
        }
    }
}

// expm1 (D-10.64, module doc "expm1"), SSE2 build; the FMA build through the boundary.

/// The correctly rounded `e^x − 1`: today's tier, else the accurate path.
fn crm1(x: f64) -> f64 {
    super::super::expm1_nearest(x).unwrap_or_else(|| Real::expm1(x, Rounding::Nearest).0)
}

/// The constants and both regions' bounds recomputed (module doc "expm1"; each term in `f64`,
/// rounded up by `1 + 2^-40`), and `C_M1` the least multiplier for `RHO_M1`.
#[test]
fn expm1_constants_and_bounds() {
    for (c, n) in [(C7, 5040.0f64), (C8, 40320.0), (C9, 362_880.0)] {
        assert_eq!(c.to_bits(), (1.0 / n).to_bits(), "C{n} is not RN(1/n!)");
    }
    assert_eq!(f64::from_bits(SMALL_M1), two(-5));
    assert_eq!(f64::from_bits(NEG37_BITS), -37.0);
    let up = 1.0 + two(-40);
    let u = two(-53);
    // Region A: exp's kernel (terms 2-6, as quick_constants_and_bounds) with the low part kept.
    let r = core::f64::consts::LN_2 / 256.0 * (1.0 + two(-34)) * up;
    let rh = r + two(-62);
    let qmax = rh * rh * (0.5 + rh / 6.0 + rh * rh / 24.0 + rh * rh * rh / 120.0 + rh.powi(4) / 720.0) * (1.0 + 4.05 * u) * up;
    let low = u * (qmax + two(-62)) * up;
    let rest = 4.05 * u * qmax + (two(-62) + two(-79)) * (rh.exp() - 1.0) * up + u * (qmax + two(-61)) * up + rh.powi(7) / 5040.0 * rh.exp() * up + 2f64.powf(-78.41) * up + two(-105) * rh.exp() + two(-104);
    let zmin = (-rh).exp() * (1.0 - two(-60)) / up;
    let x0 = two(-5);
    let amp = (1.0 / (1.0 - (-x0).exp())).max(1.0 / (x0.exp() - 1.0)) * up;
    for (name, k) in [("SSE2", (rest + 4.0 * low) / zmin), ("FMA", RHO_FMA)] {
        let a = (k * amp * (1.0 + k) + u * u * (1.0 + 1.01 * amp) * amp * up) * (1.0 + two(-51));
        assert!(a <= RHO_M1, "{name} table-region bound 2^{:.3} above RHO_M1", a.log2());
    }
    // Region B: the series.
    let pmax = (1.0 / 6.0 + x0 / 24.0 + x0 * x0 / 120.0 + x0.powi(3) / 720.0 + x0.powi(4) / 5040.0 + x0.powi(5) / 40320.0 + x0.powi(6) / 362_880.0) * up;
    let tail = 9.01 * u * x0 * x0 * pmax * up;
    let head = 2.01 * u * two(-25) * x0 * up;
    let trunc = x0.powi(9) / 3_628_800.0 * (1.0 + x0 / 11.0 * 1.01) * up;
    let sums = (u * (two(-25) * x0 + x0 * x0 * pmax) + u * (u * (1.0 + x0 / 2.0) + two(-25) * x0 + x0 * x0 * pmax)) * (1.0 + 2.0 * u) * up;
    let b = (tail + head + trunc + sums) / ((1.0 - (-x0).exp()) / x0) * (1.0 + two(-50));
    assert!(b <= RHO_M1, "series-region bound 2^{:.3} above RHO_M1", b.log2());
    let one_u = Mp::from_f64(1.0).add(Mp::from_f64(u));
    let k = Mp::from_f64(1.0).sub(Mp::from_f64(RHO_M1 * two(54)));
    assert!(Mp::from_f64(C_M1).mul(k).cmp_abs(one_u).is_gt(), "C_M1 below its bound");
    assert!(Mp::from_f64(f64::from_bits(C_M1.to_bits() - 1)).mul(k).cmp_abs(one_u).is_le(), "C_M1 not the least");
}

/// The SSE2 build's `(zh, zl)` within `RHO_M1·|zh|` of `e^x − 1` (exact in Mp) in both regions,
/// with operands on both sides of `2^-5`; the largest relative error printed.
#[test]
fn expm1_error_within_bound() {
    let edge = (0..4000u64).flat_map(|i| [f64::from_bits(SMALL_M1 - 2000 + i), -f64::from_bits(SMALL_M1 - 2000 + i)]);
    let xs = uniform(20_000, 0x2545_f491_4f6c_dd1d, -37.0, 708.0).chain(uniform(20_000, 7, -1.0, 1.0)).chain(uniform(20_000, 9, -0.04, 0.04)).chain(uniform(4000, 11, -1e-9, 1e-9)).chain(edge);
    let mut worst = 0.0f64;
    for x in xs {
        let Some((zh, zl)) = expm1_quick_parts(x) else { continue };
        let want = mp::exp(Mp::from_f64(x)).sub(Mp::from_f64(1.0));
        let err = want.sub(Mp::from_f64(zh)).sub(Mp::from_f64(zl));
        assert!(err.cmp_abs(Mp::from_f64(RHO_M1 * dd::abs(zh))).is_le(), "quick expm1({x:e}): error {:e} above the bound", err.to_f64() / zh);
        worst = worst.max(dd::abs(err.to_f64() / zh));
    }
    eprintln!("quick expm1 SSE2: largest relative error 2^{:.3} (bound 2^{:.3})", worst.log2(), RHO_M1.log2());
    assert!(worst > 0.0);
}

/// 4·10^6 operands against the correctly rounded `expm1`, every answer bit for bit (the low bits
/// of a small `x` included), the domain's edges refused, and the fallback rate on [-1, 1] within
/// the model (0.1%-1.2%).
#[test]
fn expm1_matches_correctly_rounded() {
    let sets: [(usize, u64, f64, f64); 4] = [(1_500_000, 1, -1.0, 1.0), (1_000_000, 2, -0.04, 0.04), (1_000_000, 3, -37.0, 708.0), (500_000, 4, -1e-6, 1e-6)];
    for (n, seed, lo, hi) in sets {
        let mut miss = 0usize;
        for x in uniform(n, seed, lo, hi) {
            match expm1_quick(x) {
                Some(v) => assert_eq!(v.to_bits(), crm1(x).to_bits(), "quick expm1({x:e} = {:#x}): {v:e}", x.to_bits()),
                None => miss += 1,
            }
        }
        eprintln!("quick expm1 SSE2 [{lo}, {hi}]: fallback {miss} of {n}");
        if seed == 1 {
            assert!(miss * 1000 >= n && miss * 1000 <= n * 12, "fallback rate {miss} of {n} outside 0.1%-1.2%");
        }
    }
    for x in [-37.0, -40.0, 708.0, two(-61), -two(-61), f64::NAN, f64::INFINITY] {
        assert!(expm1_quick_parts(x).is_none(), "quick expm1 took {x:e} outside its domain");
    }
    assert!(expm1_quick_parts(-f64::from_bits(NEG37_BITS - 1).abs()).is_some());
}
