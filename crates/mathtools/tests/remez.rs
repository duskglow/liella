//! The Remez tool against fits with answers known from the mathematics.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]
#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::indexing_slicing)]

use liella_mathtools::mp::{self, Big, Fmt, Round};
use liella_mathtools::remez::*;

fn rel(a: &Big, b: &Big) -> f64 {
    a.sub(b).div(b, 64, Round::Nearest).to_f64().abs()
}

fn alternates(fit: &Fit) -> bool {
    fit.extrema.windows(2).all(|w| w[0].1.is_neg() != w[1].1.is_neg())
}

/// Best linear approximation of the convex e^x on [0, 1]: by equioscillation at 0, xi, 1,
/// `c1 = e - 1`, `e^xi = c1`, `c0 = (1 + c1 (1 - xi)) / 2`, `E = (c1 (1 - xi) - 1) / 2`.
#[test]
fn degree_one_exp_closed_form() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    let f = |x: &Big, p: u32| mp::exp(x, p).value;
    let pb = Problem::new(&f, Parity::Neither, Big::zero(false), Big::one(), vec![0, 1], false);
    let fit = remez(&pb).unwrap();
    let one = Big::one();
    let c1 = mp::exp(&one, 300).value.sub(&one);
    let xi = mp::log(&c1, 300).value;
    let c0 = one.add(&c1.mul(&one.sub(&xi))).mul_pow2(-1);
    let e = c1.mul(&one.sub(&xi)).sub(&one).mul_pow2(-1).abs();
    assert!(rel(&fit.coeffs[0].1, &c0) < 1e-30, "c0 {}", fit.coeffs[0].1.to_decimal(40));
    assert!(rel(&fit.coeffs[1].1, &c1) < 1e-30, "c1 {}", fit.coeffs[1].1.to_decimal(40));
    assert!(rel(&fit.levelled, &e) < 1e-30, "E {}", fit.levelled.to_decimal(40));
    assert_eq!(fit.extrema.len(), 3);
    assert!(rel(&fit.extrema[1].0, &xi) < 1e-12, "xi {}", fit.extrema[1].0.to_decimal(20));
    assert!(alternates(&fit));
}

/// The best approximation of x^n on [-1, 1] by lower degrees leaves `T_n(x) / 2^(n-1)`, whose
/// maximum is `2^(1-n)`.
#[test]
fn chebyshev_minimal_deviation() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    for n in [2u32, 5, 6, 9] {
        let f = move |x: &Big, _p: u32| {
            let mut r = Big::one();
            for _ in 0..n {
                r = r.mul(x);
            }
            r
        };
        let pb = Problem::new(&f, Parity::Neither, Big::from_i64(-1), Big::one(), (0..n).collect(), false);
        let fit = remez(&pb).unwrap();
        assert!(rel(&fit.levelled, &Big::pow2(1 - i64::from(n))) < 1e-30, "n = {n}: {}", fit.levelled.to_decimal(30));
        assert_eq!(fit.extrema.len(), n as usize + 1);
        assert!(alternates(&fit));
    }
    // n = 6 explicitly: x^6 - T_6/32 = 1.5 x^4 - 0.5625 x^2 + 1/32.
    let f = |x: &Big, _p: u32| x.mul(x).mul(x).mul(x).mul(x).mul(x);
    let pb = Problem::new(&f, Parity::Neither, Big::from_i64(-1), Big::one(), (0..6).collect(), false);
    let fit = remez(&pb).unwrap();
    let want = [1.0 / 32.0, 0.0, -0.5625, 0.0, 1.5, 0.0];
    for (k, c) in &fit.coeffs {
        assert!((c.to_f64() - want[*k as usize]).abs() < 1e-30, "x^{k}: {}", c.to_decimal(30));
    }
}

#[test]
fn exp_degree_seven_equioscillates() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    let f = |x: &Big, p: u32| mp::exp(x, p).value;
    let a = Big::parse_rounded("-0.35", 300).unwrap();
    let pb = Problem::new(&f, Parity::Neither, a.clone(), a.neg(), (0..8).collect(), true);
    let fit = remez(&pb).unwrap();
    assert_eq!(fit.extrema.len(), 9);
    assert!(alternates(&fit));
    assert!(fit.spread < 1e-6);
    for (_, e) in &fit.extrema {
        assert!(rel(&e.abs(), &fit.levelled) < 2e-6);
    }
    // The error everywhere stays at the levelled error (coefficients kept to 113 bits).
    let q: Vec<(u32, Big)> =
        fit.coeffs.iter().map(|(k, c)| (*k, Big::from_bits(Fmt::F128, c.round_to(Fmt::F128, Round::Nearest).0))).collect();
    let around: Vec<Big> = fit.extrema.iter().map(|(x, _)| x.clone()).collect();
    let (m, n) = measure(&pb, &q, Fmt::F128, 2000, &around);
    assert!(n > 2000);
    let ratio = m.div(&fit.levelled, 64, Round::Nearest).to_f64();
    assert!((1.0 - 1e-5..1.0 + 1e-5).contains(&ratio), "{ratio}");
    eprintln!("exp degree 7 on [-0.35, 0.35]: levelled 2^{:.2}", log2_abs(&fit.levelled));
}

#[test]
fn sin_over_x_even_powers_relative() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    let t = target("sin_over_x").unwrap();
    let f = |x: &Big, p: u32| (t.eval)(x, p);
    let b = mp::pi(300).mul_pow2(-2);
    let pb = Problem::new(&f, t.parity, b.neg(), b, vec![0, 2, 4, 6, 8], true);
    let fit = remez(&pb).unwrap();
    assert!(fit.folded);
    assert_eq!(fit.extrema.len(), 6);
    assert!(alternates(&fit));
    assert!(fit.spread < 1e-6);
    // Odd powers for the odd sin: folded, with the error limit near 0 sampled just off it.
    let t = target("sin").unwrap();
    let f = |x: &Big, p: u32| (t.eval)(x, p);
    let b = mp::pi(300).mul_pow2(-2);
    let pb = Problem::new(&f, t.parity, b.neg(), b, vec![1, 3, 5, 7, 9], true);
    let fit2 = remez(&pb).unwrap();
    assert!(fit2.folded && alternates(&fit2) && fit2.spread < 1e-6);
    // Same problem: sin(x)/x by even powers is sin by odd powers divided by x.
    assert!(rel(&fit.levelled, &fit2.levelled) < 1e-4, "{} {}", fit.levelled.to_decimal(10), fit2.levelled.to_decimal(10));
}

#[test]
fn fit_files() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    assert_eq!(parse_powers("even 0..10").unwrap(), vec![0, 2, 4, 6, 8, 10]);
    assert_eq!(parse_powers("odd 1..=7").unwrap(), vec![1, 3, 5, 7]);
    assert_eq!(parse_powers("1,3, 5").unwrap(), vec![1, 3, 5]);
    assert_eq!(parse_powers("0..3, 7").unwrap(), vec![0, 1, 2, 3, 7]);
    assert!(parse_powers("3..1").is_none() && parse_powers("x").is_none());
    assert_eq!(parse_expr("-pi/4", 53).unwrap().to_f64(), -std::f64::consts::FRAC_PI_4);
    assert_eq!(parse_expr("ln2/2", 53).unwrap().to_f64(), std::f64::consts::LN_2 / 2.0);
    assert_eq!(parse_expr("0x1.8p-3", 53).unwrap().to_f64(), 0.1875);
    assert_eq!(parse_expr("3*0.5", 53).unwrap().to_f64(), 1.5);
    assert_eq!(parse_expr("1e-3", 53).unwrap().to_f64(), 1e-3);

    let text = "# a small fit\nname = tiny_exp\ntarget = exp\ninterval = [-0.25, 0.25]  # comment\npowers = 0..4\n\
                fixed = 0:1\nerror = relative\nformat = f32\noutput = x/tiny.rs\npoints = 1000\n";
    let spec = parse_fit(text).unwrap();
    assert_eq!(spec.powers, vec![0, 1, 2, 3, 4]);
    let g1 = generate(&spec, "fits/tiny.fit").unwrap();
    let g2 = generate(&spec, "fits/tiny.fit").unwrap();
    assert_eq!(g1.source, g2.source, "deterministic output");
    assert!(g1.source.starts_with("// @generated by `cargo run --release -p liella-mathtools --bin remez -- fits/tiny.fit`."));
    assert!(g1.source.contains("pub(crate) const TINY_EXP: [u32; 5] = ["));
    assert!(g1.source.contains("0x3f800000, // x^0: 1.00000000e0"), "{}", g1.source);
    eprintln!("{}{}", g1.report, g1.source);

    assert!(parse_fit("name = a\nbogus = 1\n").unwrap_err().contains("unknown key"));
    assert!(parse_fit("name = a\nname = b\n").unwrap_err().contains("duplicate"));
    let bad = text.replace("target = exp", "target = nosuch");
    assert!(parse_fit(&bad).unwrap_err().contains("unknown target"));
}
