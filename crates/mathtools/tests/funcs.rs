//! Elementary functions: identities at high precision, exact cases, special values, Rust std
//! as a coarse cross-check, and correct rounding through the Ziv loop.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]
#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::indexing_slicing)]

mod common;

use common::{abs_log2, f, rel_log2, v, Rng};
use liella_mathtools::mp::*;
use std::time::Instant;

const P: u32 = 200;

#[track_caller]
fn close(a: &Big, b: &Big, bits: u32, what: &str) {
    let r = rel_log2(a, b);
    assert!(r <= -f64::from(bits), "{what}: rel 2^{r} (a = {}, b = {})", a.to_decimal(30), b.to_decimal(30));
}

#[track_caller]
fn close_abs(a: &Big, b: &Big, scale_log2: i64, bits: u32, what: &str) {
    let r = abs_log2(a, b);
    assert!(r <= (scale_log2 - i64::from(bits)) as f64, "{what}: abs 2^{r}");
}

#[test]
fn exp_log_identities() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    let mut g = Rng(20);
    for _ in 0..1500 {
        let x = f(g.f64_exp(-40, 40)).abs();
        let l = v(log(&x, P));
        // exp turns the absolute error of l (|l| < 30) into relative error: 5 bits.
        close(&v(exp(&l, P)), &x, P - 8, "exp(log x)");
        let (a, b) = (f(g.range(-60.0, 60.0)), f(g.range(-60.0, 60.0)));
        close(&v(exp(&a.add(&b), P)), &v(exp(&a, P)).mul(&v(exp(&b, P))), P - 3, "exp(a+b)");
        let y = f(g.f64_exp(-40, 40)).abs();
        let (lx, ly) = (v(log(&x, P)), v(log(&y, P)));
        let scale = if lx.cmp_abs(&ly).is_ge() { lx.ilogb() } else { ly.ilogb() }.unwrap_or(0);
        close_abs(&v(log(&x.mul(&y), P)), &lx.add(&ly), scale, P - 3, "log(xy)");
        let t = f(g.range(-5.0, 5.0)).mul_pow2(-((g.next() % 60) as i64));
        close(&v(log1p(&v(expm1(&t, P)), P)), &t, P - 3, "log1p(expm1 t)");
        let big = f(g.range(1.0, 40.0) * if g.next() & 1 == 0 { 1.0 } else { -1.0 });
        close(&v(expm1(&big, P)), &v(exp(&big, P + 10)).sub(&Big::one()), P - 3, "expm1 vs exp - 1");
        let u = f(g.range(-1.0, 3.0));
        close(&v(log1p(&u, P)), &v(log(&u.add(&Big::one()), P + 10)), P - 3, "log1p vs log");
        // Other bases, through ln 2 and ln 10 at extra precision.
        let z = f(g.range(-300.0, 300.0));
        close(&v(exp2(&z, P)), &v(exp(&z.mul(&ln2(P + 30)), P + 10)), P - 3, "exp2");
        close(&v(exp10(&z, P)), &v(exp(&z.mul(&ln10(P + 30)), P + 10)), P - 3, "exp10");
        close(&v(log2(&x, P)), &v(log(&x, P + 10)).div(&ln2(P + 10), P + 10, Round::Nearest), P - 3, "log2");
        close(&v(log10(&x, P)), &v(log(&x, P + 10)).div(&ln10(P + 10), P + 10, Round::Nearest), P - 3, "log10");
    }
    // Arguments next to 1 keep full relative accuracy.
    let x = Big::one().add(&Big::pow2(-150));
    close(&v(log(&x, P)), &Big::pow2(-150).sub(&Big::pow2(-301)), P - 2, "log(1+2^-150)");
    let x = Big::one().sub(&Big::pow2(-112));
    close(&v(log(&x, P)), &v(log1p(&Big::pow2(-112).neg(), P)), P - 2, "log(1-2^-112)");
    // Huge and tiny arguments of the F128 range.
    let x = Big::pow2(16000).mul(&Big::from_i64(3));
    close(&v(log(&x, P)), &ln2(P + 30).mul(&Big::from_i64(16000)).add(&v(log(&Big::from_i64(3), P + 10))), P - 2, "log(3 2^16000)");
    close(&v(exp(&v(log(&x, P + 20)), P)), &x, P - 2, "exp(log(3 2^16000))");
    let x = Big::pow2(-16400);
    close(&v(exp(&v(log(&x, P + 20)), P)), &x, P - 2, "exp(log(2^-16400))");
}

#[test]
fn trig_identities() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    let mut g = Rng(21);
    for i in 0..1500 {
        let x = match i % 3 {
            0 => f(g.range(-10.0, 10.0)),
            1 => f(g.f64_exp(-60, 60)),
            _ => f(g.f64_exp(-1000, 1020)),
        };
        let (s, c) = (v(sin(&x, P)), v(cos(&x, P)));
        close_abs(&s.mul(&s).add(&c.mul(&c)), &Big::one(), 0, P - 3, "sin^2 + cos^2");
        close(&v(tan(&x, P)), &s.div(&c, P + 10, Round::Nearest), P - 3, "tan = sin/cos");
        let y = f(g.range(-1.5, 1.5));
        close(&v(atan(&v(tan(&y, P)), P)), &y, P - 4, "atan(tan y)");
        close(&v(asin(&v(sin(&y, P)), P)), &y, P - 6, "asin(sin y)");
        let z = f(g.range(0.1, 3.0));
        close(&v(acos(&v(cos(&z, P)), P)), &z, P - 10, "acos(cos z)");
        // atan2 through sin/cos of the angle.
        let (yy, xx) = (f(g.f64_exp(-20, 20)), f(g.f64_exp(-20, 20)));
        let t = v(atan2(&yy, &xx, P));
        let r = v(hypot(&xx, &yy, P + 10));
        let top = r.ilogb().unwrap();
        close_abs(&v(sin(&t, P)).mul(&r), &yy, top, P - 4, "r sin(atan2)");
        close_abs(&v(cos(&t, P)).mul(&r), &xx, top, P - 4, "r cos(atan2)");
    }
}

#[test]
fn huge_argument_reduction() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    let mut g = Rng(22);
    for i in 0..60 {
        let e = 16000 + (g.next() % 383) as i64;
        let x = if i % 2 == 0 { f(g.f64_exp(900, 1024)) } else { g.big113(e) };
        let (a, b) = (v(sin(&x, P)), v(sin(&x, 2 * P)));
        close(&a, &b, P - 1, "sin at two precisions");
        let (c, d) = (v(cos(&x, P)), v(cos(&x, 2 * P)));
        close(&c, &d, P - 1, "cos at two precisions");
        close_abs(&a.mul(&a).add(&c.mul(&c)), &Big::one(), 0, P - 3, "huge sin^2 + cos^2");
    }
    // A classic: sin(1e22) = -0.8522008497671888017727...
    let s = v(sin(&f(1e22), 100));
    assert_eq!(s.to_f64(), -0.852_200_849_767_188_8, "{}", s.to_decimal(25));
    assert!(s.to_decimal(22).starts_with("-8.522008497671888017727"));
    // Close to a multiple of pi/2: x = nearest double to 2^k pi/2 still gives full accuracy.
    let x = pi(53).mul_pow2(-1);
    let d = pi(600).mul_pow2(-1).sub(&x); // cos(pi/2 - d) = sin d, d about 2^-54
    close(&v(cos(&x, P)), &v(sin(&d, P + 10)), P - 1, "cos(pi/2 double)");
}

#[test]
fn hyperbolic_identities() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    let mut g = Rng(23);
    for _ in 0..1500 {
        let x = f(g.range(1.0, 100.0) * if g.next() & 1 == 0 { 1.0 } else { -1.0 });
        let (e1, e2) = (v(exp(&x, P + 10)), v(exp(&x.neg(), P + 10)));
        close(&v(sinh(&x, P)), &e1.sub(&e2).mul_pow2(-1), P - 3, "sinh");
        close(&v(cosh(&x, P)), &e1.add(&e2).mul_pow2(-1), P - 3, "cosh");
        let y = f(g.range(-10.0, 10.0)).mul_pow2(-((g.next() % 40) as i64));
        let (s, c) = (v(sinh(&y, P)), v(cosh(&y, P)));
        let c2 = c.mul(&c);
        close_abs(&c2.sub(&s.mul(&s)), &Big::one(), c2.ilogb().unwrap(), P - 4, "cosh^2 - sinh^2");
        close(&v(tanh(&y, P)), &s.div(&c, P + 10, Round::Nearest), P - 3, "tanh");
        close(&v(asinh(&s, P)), &y, P - 3, "asinh(sinh)");
        let z = f(g.range(0.5, 50.0));
        close(&v(acosh(&v(cosh(&z, P)), P)), &z, P - 5, "acosh(cosh)");
        let w = f(g.range(-5.0, 5.0));
        close(&v(atanh(&v(tanh(&w, P)), P)), &w, P - 14, "atanh(tanh)");
    }
    // Large arguments take the log(2x) shortcut.
    let x = Big::pow2(5000).mul(&Big::from_i64(5));
    close(&v(asinh(&x, P)), &v(log(&x.mul_pow2(1), P + 10)), P - 2, "asinh huge");
    close(&v(acosh(&x, P)), &v(log(&x.mul_pow2(1), P + 10)), P - 2, "acosh huge");
}

#[test]
fn pow_roots_erf() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    let mut g = Rng(24);
    for _ in 0..1500 {
        let x = f(g.range(0.1, 10.0));
        let y = f(g.range(-50.0, 50.0));
        let reference = v(exp(&y.mul(&v(log(&x, P + 20))), P + 10));
        close(&v(pow(&x, &y, P)), &reference, P - 3, "pow");
        let n = (g.next() % 40) as i64 - 20;
        let xn = f(g.f64_exp(-5, 5));
        let p = pow(&xn, &Big::from_i64(n), P);
        let mut want = Big::one();
        for _ in 0..n.abs() {
            want = want.mul(&xn);
        }
        if n < 0 {
            want = Big::one().div(&want, P + 10, Round::Nearest);
            close(&p.value, &want, P - 1, "pow negative integer");
        } else {
            assert!(p.exact && p.value.identical(&want), "pow({}, {n})", xn.to_hex());
        }
        let c = v(cbrt(&x, P));
        close(&c.mul(&c).mul(&c), &x, P - 2, "cbrt^3");
        let (a, b) = (f(g.f64_any()), f(g.f64_any()));
        if a.is_finite() && b.is_finite() && !a.is_zero() && !b.is_zero() {
            let h = v(hypot(&a, &b, P));
            close(&h, &a.mul(&a).add(&b.mul(&b)).sqrt(P + 10, Round::Nearest), P - 1, "hypot");
        }
        // erf' = 2/sqrt(pi) e^(-x^2): central difference with step 2^-40 (error ~2^-80).
        let t = f(g.range(-4.0, 4.0));
        let h = Big::pow2(-40);
        let d = v(erf(&t.add(&h), P)).sub(&v(erf(&t.sub(&h), P))).mul_pow2(39);
        let want = v(exp(&t.mul(&t).neg(), P)).mul_pow2(1).div(&pi(P).sqrt(P, Round::Nearest), P, Round::Nearest);
        close(&d, &want, 70, "erf derivative");
    }
    assert_eq!(v(erf(&f(1.0), 80)).to_f64(), 0.842_700_792_949_714_9);
    assert_eq!(v(erf(&f(0.5), 80)).to_f64(), 0.520_499_877_813_046_5);
    assert_eq!(v(erf(&f(30.0), 80)).to_f64(), 1.0);
}

#[track_caller]
fn is_exact(a: Approx, want: &Big) {
    assert!(a.exact, "not flagged exact: {}", a.value.to_hex());
    assert!(a.value.identical(want), "{} vs {}", a.value.to_hex(), want.to_hex());
}

fn b(s: &str) -> Big {
    Big::parse(s).unwrap()
}

#[test]
fn exact_results() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    let one = Big::one();
    is_exact(exp(&b("0"), P), &one);
    is_exact(exp(&b("-0"), P), &one);
    is_exact(expm1(&b("-0"), P), &b("-0"));
    is_exact(log(&one, P), &b("0"));
    is_exact(log1p(&b("-0"), P), &b("-0"));
    is_exact(sin(&b("-0"), P), &b("-0"));
    is_exact(cos(&b("0"), P), &one);
    is_exact(tan(&b("-0"), P), &b("-0"));
    is_exact(atan(&b("-0"), P), &b("-0"));
    is_exact(asin(&b("-0"), P), &b("-0"));
    is_exact(acos(&one, P), &b("0"));
    is_exact(sinh(&b("-0"), P), &b("-0"));
    is_exact(cosh(&b("0"), P), &one);
    is_exact(tanh(&b("-0"), P), &b("-0"));
    is_exact(asinh(&b("-0"), P), &b("-0"));
    is_exact(acosh(&one, P), &b("0"));
    is_exact(atanh(&b("-0"), P), &b("-0"));
    is_exact(exp2(&b("10"), P), &b("1024"));
    is_exact(exp2(&b("-1074"), P), &Big::pow2(-1074));
    is_exact(exp10(&b("3"), P), &b("1000"));
    is_exact(exp10(&b("22"), P), &b("1e22"));
    is_exact(log2(&b("0x1p-1074"), P), &b("-1074"));
    is_exact(log2(&b("1024"), P), &b("10"));
    is_exact(log10(&b("1000"), P), &b("3"));
    is_exact(log10(&b("1e30"), P), &b("30"));
    is_exact(pow(&b("4"), &b("0.5"), P), &b("2"));
    is_exact(pow(&b("2"), &b("10"), P), &b("1024"));
    is_exact(pow(&b("9"), &b("1.5"), P), &b("27"));
    is_exact(pow(&b("0x1p-6"), &b("-1.5"), P), &b("512"));
    is_exact(pow(&b("-3"), &b("3"), P), &b("-27"));
    is_exact(pow(&b("-2"), &b("-3"), P), &b("-0.125"));
    is_exact(pow(&b("0.0625"), &b("0.25"), P), &b("0.5"));
    is_exact(pow(&b("1.5"), &b("2"), P), &b("2.25"));
    is_exact(cbrt(&b("-27"), P), &b("-3"));
    is_exact(cbrt(&b("0x1p-1074"), P), &Big::pow2(-358));
    is_exact(cbrt(&b("3.375"), P), &b("1.5"));
    is_exact(hypot(&b("3"), &b("-4"), P), &b("5"));
    is_exact(hypot(&b("0.375"), &b("0.5"), P), &b("0.625"));
    is_exact(hypot(&b("-0"), &b("-7"), P), &b("7"));
    is_exact(atan2(&b("-0"), &b("2"), P), &b("-0"));
    is_exact(atan2(&b("-3"), &b("inf"), P), &b("-0"));
    // Not representable: flagged inexact.
    for a in [
        pow(&b("2"), &b("0.5"), P),
        pow(&b("3"), &b("-1"), P),
        exp10(&b("-1"), P),
        cbrt(&b("2"), P),
        hypot(&b("1"), &b("1"), P),
        log10(&b("2"), P),
        log2(&b("3"), P),
        exp2(&b("0.5"), P),
    ] {
        assert!(!a.exact);
    }
}

#[test]
fn special_values() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    let nan = Big::nan();
    let inf = Big::inf(false);
    let ninf = Big::inf(true);
    let one = Big::one();
    let all: [fn(&Big, u32) -> Approx; 20] =
        [exp, expm1, exp2, exp10, log, log1p, log2, log10, sin, cos, tan, atan, asin, acos, sinh, cosh, tanh, asinh, acosh, atanh];
    for func in all {
        let r = func(&nan, P);
        assert!(r.exact && r.value.is_nan());
    }
    assert!(exp(&ninf, P).value.is_zero());
    assert!(expm1(&ninf, P).value.identical(&one.neg()));
    assert!(log(&b("-0"), P).value.identical(&ninf));
    assert!(log(&b("-1"), P).value.is_nan());
    assert!(log1p(&b("-1"), P).value.identical(&ninf));
    assert!(log1p(&b("-2"), P).value.is_nan());
    assert!(sin(&inf, P).value.is_nan() && cos(&ninf, P).value.is_nan() && tan(&inf, P).value.is_nan());
    assert!(asin(&b("1.5"), P).value.is_nan() && acos(&b("-1.5"), P).value.is_nan());
    assert!(acosh(&b("0.5"), P).value.is_nan() && atanh(&b("2"), P).value.is_nan());
    assert!(atanh(&b("-1"), P).value.identical(&ninf));
    assert!(tanh(&ninf, P).value.identical(&one.neg()));
    assert!(atan(&ninf, P).value.cmp(&pi(P).mul_pow2(-1).neg()) == Some(std::cmp::Ordering::Less));
    // pow (C11 F.10.4.4).
    let pw = |x: &str, y: &str| pow(&b(x), &b(y), P).value;
    assert!(pw("nan", "0").identical(&one) && pw("1", "nan").identical(&one));
    assert!(pw("-0", "-3").identical(&ninf) && pw("-0", "-2").identical(&inf) && pw("0", "-inf").identical(&inf));
    assert!(pw("-0", "3").identical(&b("-0")) && pw("-0", "2").identical(&b("0")));
    assert!(pw("-1", "inf").identical(&one) && pw("-1", "-inf").identical(&one));
    assert!(pw("0.5", "-inf").identical(&inf) && pw("2", "-inf").is_zero() && pw("0.5", "inf").is_zero() && pw("2", "inf").identical(&inf));
    assert!(pw("-inf", "-3").identical(&b("-0")) && pw("-inf", "-2").identical(&b("0")));
    assert!(pw("-inf", "3").identical(&ninf) && pw("-inf", "2").identical(&inf));
    assert!(pw("inf", "-1").identical(&b("0")) && pw("inf", "0.5").identical(&inf));
    assert!(pw("-2", "0.5").is_nan());
    // atan2 corners.
    let at = |y: &str, x: &str| atan2(&b(y), &b(x), 60).value.to_f64();
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};
    assert_eq!(at("0", "-0"), PI);
    assert_eq!(at("-0", "-1"), -PI);
    assert_eq!(at("1", "0"), FRAC_PI_2);
    assert_eq!(at("-inf", "inf"), -FRAC_PI_4);
    assert_eq!(at("inf", "-inf"), 3.0 * FRAC_PI_4);
    assert_eq!(at("2", "-inf"), PI);
    // Absurd exponents: saturation (exp family) and NaN (trigonometric), never a hang.
    let absurd = Big::pow2(1 << 40);
    assert!(exp(&absurd, P).value.round_to(Fmt::F128, Round::Nearest).0 == Fmt::F128.inf_bits(false));
    assert!(exp(&absurd.neg(), P).value.round_to(Fmt::F64, Round::Nearest).0 == 0);
    assert!(sin(&absurd, P).value.is_nan() && tan(&absurd, P).value.is_nan());
    assert!(log(&absurd, P).value.to_f64() > 7.6e11);
    // hypot: infinity wins over NaN.
    assert!(hypot(&nan, &ninf, P).value.identical(&inf));
    assert!(hypot(&nan, &one, P).value.is_nan());
}

#[test]
fn fuzz_contract() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    // Any input, any small precision: no panic; inexact results are nonzero with the sign of
    // the true value (taken from std where it is a nonzero finite number).
    let mut g = Rng(28);
    type F1 = (fn(&Big, u32) -> Approx, fn(f64) -> f64);
    let unary: [F1; 21] = [
        (exp, f64::exp),
        (expm1, f64::exp_m1),
        (exp2, f64::exp2),
        (exp10, |x| 10f64.powf(x)),
        (log, f64::ln),
        (log1p, f64::ln_1p),
        (log2, f64::log2),
        (log10, f64::log10),
        (sin, f64::sin),
        (cos, f64::cos),
        (tan, f64::tan),
        (atan, f64::atan),
        (asin, f64::asin),
        (acos, f64::acos),
        (sinh, f64::sinh),
        (cosh, f64::cosh),
        (tanh, f64::tanh),
        (asinh, f64::asinh),
        (acosh, f64::acosh),
        (atanh, f64::atanh),
        (cbrt, f64::cbrt),
    ];
    for i in 0..3000 {
        let x = g.f64_any();
        let prec = [1, 2, 24, 53][i % 4];
        for (func, std_f) in unary {
            let a = func(&f(x), prec);
            check_contract(&a, std_f(x), x);
        }
        let y = g.f64_any();
        check_contract(&pow(&f(x), &f(y), prec), x.powf(y), x);
        check_contract(&atan2(&f(x), &f(y), prec), x.atan2(y), x);
        check_contract(&hypot(&f(x), &f(y), prec), x.hypot(y), x);
        let _ = erf(&f(x), prec);
    }
}

#[track_caller]
fn check_contract(a: &Approx, std_v: f64, x: f64) {
    if a.exact || a.value.is_nan() {
        return;
    }
    assert!(!a.value.is_zero(), "inexact zero at {x:e}");
    if std_v.is_finite() && std_v != 0.0 {
        assert_eq!(a.value.is_neg(), std_v < 0.0, "sign at {x:e}: {} vs {std_v:e}", a.value.to_hex());
    }
}

/// Correctly rounded binary64 via the Ziv loop.
fn cr(func: fn(&Big, u32) -> Approx, x: f64, mode: Round) -> f64 {
    let bx = f(x);
    let (bits, _) = correctly_rounded(Fmt::F64, mode, 4000, |p| func(&bx, p)).unwrap();
    f64::from_bits(bits as u64)
}

#[test]
fn against_rust_std() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    // Rust's (platform) functions are within a few ulps; ours are correctly rounded.
    let mut g = Rng(25);
    type Check = (fn(&Big, u32) -> Approx, fn(f64) -> f64, f64, f64);
    let checks: [Check; 12] = [
        (exp, f64::exp, -700.0, 700.0),
        (expm1, f64::exp_m1, -5.0, 5.0),
        (log, f64::ln, 1e-300, 1e300),
        (log1p, f64::ln_1p, -0.9, 10.0),
        (sin, f64::sin, -1e6, 1e6),
        (cos, f64::cos, -1e6, 1e6),
        (tan, f64::tan, -100.0, 100.0),
        (atan, f64::atan, -100.0, 100.0),
        (asin, f64::asin, -1.0, 1.0),
        (sinh, f64::sinh, -700.0, 700.0),
        (tanh, f64::tanh, -20.0, 20.0),
        (cbrt, f64::cbrt, -1e300, 1e300),
    ];
    for (func, std_f, lo, hi) in checks {
        let mut worst = 0.0f64;
        for _ in 0..2000 {
            let x = g.range(lo, hi);
            let got = cr(func, x, Round::Nearest);
            let ours = func(&f(x), 130).value;
            let e = ulp_error(Fmt::F64, u128::from(std_f(x).to_bits()), &ours);
            worst = worst.max(e);
            assert!(ulp_error(Fmt::F64, u128::from(got.to_bits()), &ours) <= 0.5 + 1e-9);
            assert!(e < 4.0, "x = {x:e}: std {:e} vs {}", std_f(x), ours.to_decimal(20));
        }
        eprintln!("worst std ulp error: {worst:.3}");
    }
}

#[test]
fn correct_rounding_all_modes_and_formats() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    let mut g = Rng(26);
    let funcs: [fn(&Big, u32) -> Approx; 6] = [exp, log, sin, atan, sinh, log2];
    for fmt in [Fmt::F32, Fmt::F64, Fmt::F80, Fmt::F128] {
        for func in funcs {
            for _ in 0..200 {
                let x = Big::from_f64(g.range(0.01, 50.0));
                let exact = func(&x, 2 * fmt.prec + 40).value;
                for mode in [Round::Nearest, Round::Up, Round::Down, Round::Zero] {
                    let (bits, fl) = correctly_rounded(fmt, mode, 20000, |p| func(&x, p)).unwrap();
                    let e = ulp_error(fmt, bits, &exact);
                    let got = Big::from_bits(fmt, bits);
                    assert!(fl.inexact);
                    match mode {
                        Round::Nearest => assert!(e <= 0.5),
                        _ => {
                            assert!(e < 1.0);
                            let above = got.cmp(&exact) == Some(std::cmp::Ordering::Greater);
                            let want_above = mode == Round::Up || (mode == Round::Zero && exact.is_neg());
                            assert_eq!(above, want_above, "{mode:?}");
                        }
                    }
                }
            }
        }
    }
    // Exact and halfway cases are decided through Approx::exact.
    let (bits, fl) = correctly_rounded(Fmt::F32, Round::Nearest, 1000, |p| exp2(&f(-149.0), p)).unwrap();
    assert_eq!((bits, fl.inexact, fl.underflow), (1, false, false));
    let half = Big::one().add(&Big::pow2(-24)); // exactly halfway between two floats
    let (bits, fl) = correctly_rounded(Fmt::F32, Round::Nearest, 1000, |p| pow(&half, &Big::one(), p)).unwrap();
    assert_eq!((bits as u32, fl.inexact), (1.0f32.to_bits(), true));
    // Asymptotes: tanh(20) rounds down to 1 - 2^-53 once the precision passes ~60 bits.
    let (bits, _) = correctly_rounded(Fmt::F64, Round::Down, 1000, |p| tanh(&f(20.0), p)).unwrap();
    assert_eq!(f64::from_bits(bits as u64), 1.0 - f64::EPSILON / 2.0);
    // cos of a tiny argument needs about twice its exponent in bits.
    let (bits, _) = correctly_rounded(Fmt::F64, Round::Down, 4000, |p| cos(&f(1e-200), p)).unwrap();
    assert_eq!(f64::from_bits(bits as u64), 1.0 - f64::EPSILON / 2.0);
    assert!(correctly_rounded(Fmt::F64, Round::Down, 200, |p| cos(&f(1e-200), p)).is_err());
}

#[test]
fn timing_correctly_rounded_binary64() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    let mut g = Rng(27);
    let n = 10_000;
    for (name, func, lo, hi) in [
        ("exp", exp as fn(&Big, u32) -> Approx, -700.0, 700.0),
        ("sin", sin, -1e3, 1e3),
        ("log", log, 1e-300, 1e300),
    ] {
        let xs: Vec<f64> = (0..n).map(|_| g.range(lo, hi)).collect();
        let t = Instant::now();
        let mut acc = 0u64;
        for &x in &xs {
            acc ^= cr(func, x, Round::Nearest).to_bits();
        }
        let per = t.elapsed().as_secs_f64() / f64::from(n) * 1e6;
        eprintln!("{name}: {per:.1} us per correctly rounded binary64 evaluation ({acc:x})");
        assert!(per < 1000.0);
    }
}
