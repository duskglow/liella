//! The `expm1`/`log1p`/`cbrt`/`sinh`/`cosh`/`tanh` fast tier against the accurate path (no spec
//! clause; D-10.13: where an entry answers, it is the accurate path's result bit for bit). Plain
//! build only: the FMA build's bit-identity is checked through the exports (`math_fma_paths`).
//! Each sweep also requires the tier to answer at least 99.9% of the time (a weaker bound or a
//! worse cbrt seed shows as a low rate), and `cbrt` must decline every exact cube.

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

/// Signed magnitudes `±m·2^-e`, `e` uniform in `[0, emax)`, `m` in `[1, 2)` (callers keep the scaled
/// magnitudes above the tier's floor `2^-60`, so the answer rate counts operands in the window).
fn logscale(n: usize, seed: u64, emax: u64) -> impl Iterator<Item = f64> {
    let mut st = seed;
    (0..n).map(move |_| {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        let m = f64::from_bits(0x3ff0_0000_0000_0000 | (st >> 12));
        let e = i32::try_from(st % emax).unwrap_or(0);
        let v = m * f64::powi(2.0, -e);
        if st & 0x800 == 0 { v } else { -v }
    })
}

/// Checks `fast` against `exact` on `xs` and returns `(answered, total)`; every answer must be
/// the accurate result bit for bit.
fn check(name: &str, fast: fn(f64) -> Option<f64>, exact: fn(f64) -> f64, xs: impl Iterator<Item = f64>) -> (usize, usize) {
    let (mut got, mut n) = (0, 0);
    for x in xs {
        n += 1;
        if let Some(v) = fast(x) {
            got += 1;
            let e = exact(x);
            assert_eq!(v.to_bits(), e.to_bits(), "{name}({x:e} = {:#x}): fast {v:e}, accurate {e:e}", x.to_bits());
        }
    }
    (got, n)
}

fn rate(name: &str, (got, n): (usize, usize)) {
    assert!(n > 0 && got * 1000 >= n * 999, "{name}: the fast tier answered {got} of {n} (below 99.9%)");
}

/// The accurate path in round-to-nearest, as `fn(f64) -> f64`.
macro_rules! rn {
    ($f:ident) => {
        {
            let f: fn(f64) -> f64 = |x: f64| Real::$f(x, Rounding::Nearest).0;
            f
        }
    };
}

#[test]
fn expm1_tier_matches_the_accurate_path() {
    let e = rn!(expm1);
    rate("expm1 series", check("expm1", super::expm1_nearest, e, logscale(40_000, 0x1234_5678_9abc_def1, 50).map(|x| x * 0.0078)));
    rate("expm1 kernel", check("expm1", super::expm1_nearest, e, uniform(60_000, 0x9e37_79b9_7f4a_7c15, -36.9, 708.0)));
    rate("expm1 near 2^-6", check("expm1", super::expm1_nearest, e, uniform(40_000, 0x2545_f491_4f6c_dd1d, -0.2, 0.2)));
}

#[test]
fn log1p_tier_matches_the_accurate_path() {
    let e = rn!(log1p);
    rate("log1p series", check("log1p", super::log1p_nearest, e, logscale(40_000, 0x1234_5678_9abc_def1, 50).map(|x| x * 0.0039)));
    rate("log1p kernel", check("log1p", super::log1p_nearest, e, uniform(40_000, 0x9e37_79b9_7f4a_7c15, -0.999_999, 4.0)));
    rate("log1p near -1", check("log1p", super::log1p_nearest, e, logscale(20_000, 0x2545_f491_4f6c_dd1d, 50).map(|x| -1.0 + x.abs())));
    let big = logscale(40_000, 0x5151_aaaa_1234_9876, 900).map(|x| 1.0 / x.abs());
    rate("log1p large", check("log1p", super::log1p_nearest, e, big.filter(|x| *x < 1e288)));
}

/// A hyperbolic sweep: name, fast entry, accurate path, the end of the tier's range.
type Row = (&'static str, fn(f64) -> Option<f64>, fn(f64) -> f64, f64);

#[test]
fn hyperbolic_tier_matches_the_accurate_path() {
    let rows: [Row; 3] = [
        ("sinh", super::sinh_nearest, rn!(sinh), 708.0),
        ("cosh", super::cosh_nearest, rn!(cosh), 708.0),
        ("tanh", super::tanh_nearest, rn!(tanh), 22.0),
    ];
    for (name, f, ex, top) in rows {
        rate(name, check(name, f, ex, logscale(40_000, 0x1234_5678_9abc_def1, 50).map(|x| x * 0.0078)));
        rate(name, check(name, f, ex, uniform(60_000, 0x9e37_79b9_7f4a_7c15, -top, top)));
        rate(name, check(name, f, ex, uniform(40_000, 0x2545_f491_4f6c_dd1d, -top.min(45.0), top.min(45.0))));
    }
}

/// Hard cases for the kernel paths' rounding bounds: inputs whose exact result lies so close to a
/// rounding boundary (a midpoint between two `double`s) that the double-double before rounding
/// is on the wrong side of it. Found by a search of 10^8 to 1.6·10^9 uniform operands per range
/// (LIBM-F64-EXPM1-HYPER-CBRT-TIER-3) for those where the tier with its bound set to zero
/// disagreed with the accurate path; about one operand in 10^8 is such a case, so random sweeps
/// never test the bound. The tier must decline each (or answer the accurate result). `cbrt` has
/// none: 10^8 operands in [1, 8) gave no case (its second refinement leaves the double-double
/// much closer than the exp and log kernels do).
const HARD: [(&str, u64); 18] = [
    // expm1, the exp kernel with the −1 folded in: 264.65, 0.2545, 0.0915, 0.8283.
    ("expm1", 0x4070_8a59_f695_df1b),
    ("expm1", 0x3fd0_48ee_c037_0a2b),
    ("expm1", 0x3fb7_6a6a_31bc_1228),
    ("expm1", 0x3fea_818c_c3cf_8565),
    // log1p, the log kernel and `w.lo/w.hi`: 0.01256, 0.4485, −0.02829, −0.2955.
    ("log1p", 0x3f89_b8ac_178e_dd3c),
    ("log1p", 0x3fdc_b3c9_70f7_ee05),
    ("log1p", 0xbf9c_f904_74ec_20c0),
    ("log1p", 0xbfd2_e8fb_2f61_31b2),
    // sinh, e^a − e^-a by `Dd::sub`: 32.93.
    ("sinh", 0x4040_7710_584a_0713),
    // cosh, e^a + e^-a by `Dd::add`: 30.04, 2.767, 13.02.
    ("cosh", 0x403e_09b1_7a13_3df8),
    ("cosh", 0x4006_22ce_d2e2_19c3),
    ("cosh", 0x402a_0c34_a000_4664),
    // e^a/2 for a > 40 (sinh and cosh alike): 263.76, 154.02.
    ("cosh", 0x4070_7c26_6017_2b6f),
    ("cosh", 0x4063_40c1_aa18_661b),
    ("sinh", 0x4070_7c26_6017_2b6f),
    // tanh, the double-double quotient: 0.0513, 1.1404, 0.2321.
    ("tanh", 0x3faa_44ae_7c0c_9bd1),
    ("tanh", 0x3ff2_3eea_5dd3_fb12),
    ("tanh", 0x3fcd_b566_3998_0f39),
];

/// The hard cases above (no spec clause; D-10.13: an answer is the accurate path's result).
#[test]
fn tier_declines_or_rounds_right_on_hard_cases() {
    for (name, bits) in HARD {
        let x = f64::from_bits(bits);
        let (fast, exact): (fn(f64) -> Option<f64>, fn(f64) -> f64) = match name {
            "expm1" => (super::expm1_nearest, rn!(expm1)),
            "log1p" => (super::log1p_nearest, rn!(log1p)),
            "sinh" => (super::sinh_nearest, rn!(sinh)),
            "cosh" => (super::cosh_nearest, rn!(cosh)),
            "tanh" => (super::tanh_nearest, rn!(tanh)),
            _ => panic!("hard case for an unknown function {name}"),
        };
        let odd = matches!(name, "sinh" | "tanh");
        let signs: &[f64] = if odd { &[1.0, -1.0] } else { &[1.0] };
        for v in signs.iter().map(|s| s * x) {
            if let Some(got) = fast(v) {
                let e = exact(v);
                assert_eq!(got.to_bits(), e.to_bits(), "hard case {name}({v:e} = {:#x}): fast {got:e}, accurate {e:e}", v.to_bits());
            }
        }
    }
}

#[test]
fn cbrt_tier_matches_the_accurate_path_and_declines_exact_cubes() {
    let e = rn!(cbrt);
    // Every exponent residue: random significands over the whole normal range.
    let mut st = 0x0bad_cafe_dead_beefu64;
    let xs: Vec<f64> = (0..120_000)
        .map(|_| {
            st ^= st << 13;
            st ^= st >> 7;
            st ^= st << 17;
            f64::from_bits((st >> 12) | ((1 + st % 2046) << 52) | ((st & 1) << 63))
        })
        .collect();
    rate("cbrt", check("cbrt", super::cbrt_nearest, e, xs.into_iter()));
    rate("cbrt [1, 8)", check("cbrt", super::cbrt_nearest, e, uniform(40_000, 0x9e37_79b9_7f4a_7c15, 1.0, 8.0)));
    // Exact cubes n³·2^(3k): declined (the accurate path raises no inexact for them).
    for n in (1u32..2000).chain((1u32..2000).map(|i| 131_071 - i)) {
        let c = f64::from(n).powi(3);
        for k in [-300, -7, -1, 0, 1, 5, 100] {
            let x = c * f64::powi(2.0, 3 * k);
            assert_eq!(super::cbrt_nearest(x), None, "cbrt({x:e}) is the exact cube of {n}·2^{k} but the tier answered");
            assert_eq!(super::cbrt_nearest(-x), None, "cbrt({:e}) is an exact cube but the tier answered", -x);
        }
    }
}
