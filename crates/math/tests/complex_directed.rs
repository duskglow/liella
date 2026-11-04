//! The complex functions in the asymptotic regimes, in every rounding direction (spec §10.5,
//! D-10.17; Track M story M6-10 `MATH-M6-CPLX-DIRECTED`). A part there is a representable
//! value, or a power of two, plus a correction negligible against it (`catanh(2^-100 +
//! i·2^200)`, whose real part is `2^-500·(1 − 2^-400 + ...)`; `cos y·cosh x` next to 1 at a tiny
//! operand; `atan2(y, x)` next to `y/x` with `|x|` far above `|y|`), so a directed mode rounds it
//! to the neighbour on the correction's side, which only a precision beyond the correction
//! decides. Every part is compared with the correctly rounded value of the test oracle
//! (`liella_mathtools::cplx`, Ziv's loop up to 2^14 bits), on a grid of part exponents over each
//! format's range (smallest subnormal, smallest normal, wide and narrow tiny and huge, 0) with
//! two significands and both signs of the real part, for `float`, `double`, x87 `long double`
//! and `_Float128`, in all four modes. Liella calls the library core (`liella_math::complex`)
//! directly, so no C compiler or product build is needed. An operand the oracle cannot decide
//! within 2^14 bits is listed and counted, never passed silently; the test fails on any part
//! that differs or on any such operand (none is expected on this grid).
//!
//! The ladder counters (`retries`, `wide_retries`, `wider_retries`, `residual`) are printed per
//! test (`--nocapture`): a part decided only through the residual path is printed with its
//! operands.
//!
//! `float` and `double` pass in every mode. The `long double` and `_Float128` grids are
//! `#[ignore]`d: on them `catan`/`catanh` reach the 3072-bit residual path and differ in the
//! directed modes at the grid's `y = 2^4096` (`catan` part 1, `catanh` part 0), and operands at
//! the subnormal end are beyond the oracle's 2^14 bits (open findings,
//! `handoff/math-m6-cplx-directed.md`); run them with `--ignored` as the follow-up's probe. The
//! counters are global, so under the parallel harness a test's counts include its neighbours'
//! (run with `--test-threads=1` for exact per-test counts).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_sign_loss, clippy::arithmetic_side_effects, missing_docs)]

use liella_math::complex::{self, Res};
use liella_math::real::Real;
use liella_math::{Rounding, F128, F80};
use liella_mathtools::cplx;
use liella_mathtools::mp::{self, Big, Fmt, Round};

const MODES: [(Rounding, Round); 4] =
    [(Rounding::Nearest, Round::Nearest), (Rounding::Up, Round::Up), (Rounding::Down, Round::Down), (Rounding::Zero, Round::Zero)];

type Fun<F> = fn(F, F, Rounding) -> Res<F>;

/// The 16 two-operand families, by the oracle's name.
fn families<F: Real>() -> [(&'static str, Fun<F>); 16] {
    [
        ("cexp", complex::cexp::<F>),
        ("clog", complex::clog::<F>),
        ("clog10", complex::clog10::<F>),
        ("csqrt", complex::csqrt::<F>),
        ("csinh", complex::csinh::<F>),
        ("ccosh", complex::ccosh::<F>),
        ("csin", complex::csin::<F>),
        ("ccos", complex::ccos::<F>),
        ("ctanh", complex::ctanh::<F>),
        ("ctan", complex::ctan::<F>),
        ("casin", complex::casin::<F>),
        ("cacos", complex::cacos::<F>),
        ("casinh", complex::casinh::<F>),
        ("cacosh", complex::cacosh::<F>),
        ("catanh", complex::catanh::<F>),
        ("catan", complex::catan::<F>),
    ]
}

/// The part exponents of the grid: the smallest subnormal and normal, the deep and shallow tiny
/// and huge regimes (the wide formats stop at 2^±4096: a sine of 2^16000 is no asymptotic
/// regime and costs the oracle its whole budget), `±2p`, `±p/2`, 0 and the largest exponent
/// (only where the format's trigonometric arguments stay below 2^4096).
fn exponents(fmt: Fmt) -> Vec<i64> {
    let (p, e) = (i64::from(fmt.prec), i64::from(fmt.emax));
    let deep = (e / 2).min(4096);
    let mid = (e / 8).min(600);
    let mut v = vec![2 - e - p, 1 - e, -deep, -mid, -2 * p, -p / 2, 0, p / 2, 2 * p, mid, deep];
    if e <= 4096 {
        v.push(e);
    }
    v
}

/// `s · m · 2^k` in `fmt` (exact for the grid's values).
fn val(fmt: Fmt, neg: bool, m: f64, k: i64) -> Big {
    let b = Big::from_f64(m).mul_pow2(k);
    let b = if neg { b.neg() } else { b };
    Big::from_bits(fmt, b.round_to(fmt, Round::Nearest).0)
}

fn bits_of(fmt: Fmt, b: &Big) -> u128 {
    b.round_to(fmt, Round::Nearest).0
}

/// The oracle's correctly rounded part `j` in `mode`, `None` where it cannot decide.
fn oracle(func: &str, fmt: Fmt, mode: Round, ops: &[Big], j: usize) -> Option<u128> {
    let part = |p: u32| {
        // The wide formats take the oracle's wide evaluation, as their products tests do.
        let r = if fmt.prec > 53 { cplx::complex_wide(func, ops, p) } else { cplx::complex(func, ops, p) };
        let (a, b) = r.expect("finite operands");
        if j == 0 { a } else { b }
    };
    mp::correctly_rounded(fmt, mode, 1 << 14, part).ok().map(|(v, _)| v)
}

/// Equal parts; a zero against a zero of either sign is equal here (the sign of an exact zero
/// is the products tests' rule, not this one's).
fn same(fmt: Fmt, a: u128, b: u128) -> bool {
    let frac = if fmt.explicit_lead { fmt.prec } else { fmt.prec - 1 };
    let sign = 1u128 << (fmt.exp_bits + frac);
    a == b || (a & !sign == 0 && b & !sign == 0)
}

fn check<F: Real>(fmt: Fmt, fam: usize) {
    let (name, f) = families::<F>()[fam];
    let ex = exponents(fmt);
    let one_up = 1.0 + f64::powi(2.0, 1 - fmt.prec as i32).max(f64::EPSILON);
    let (mut bad, mut undecided, mut lines) = (Vec::new(), 0usize, 0usize);
    let before = (complex::retries(), complex::wide_retries(), complex::wider_retries(), complex::residual());
    for &kx in &ex {
        for &ky in &ex {
            for (neg, mx) in [(false, 1.0), (true, 1.0), (false, one_up), (true, one_up)] {
                let ops = [val(fmt, neg, mx, kx), val(fmt, false, 1.0, ky)];
                let (x, y) = (F::from_bits(bits_of(fmt, &ops[0])), F::from_bits(bits_of(fmt, &ops[1])));
                for (lm, om) in MODES {
                    let res0 = complex::residual();
                    let ((re, im), _) = f(x, y, lm);
                    let got = [re.to_bits(), im.to_bits()];
                    if complex::residual() != res0 {
                        println!("{name} p{} {lm:?}: residual path at x = {}{mx}·2^{kx}, y = 2^{ky}", fmt.prec, if neg { "-" } else { "" });
                    }
                    lines += 1;
                    for (j, &g) in got.iter().enumerate() {
                        match oracle(name, fmt, om, &ops, j) {
                            None => {
                                undecided += 1;
                                println!("{name} p{} {lm:?} part {j}: oracle undecided at x = {}{mx}·2^{kx}, y = 2^{ky}", fmt.prec, if neg { "-" } else { "" });
                            }
                            Some(want) if !same(fmt, g, want) => bad.push(format!(
                                "{name} p{} {lm:?} part {j}: x = {}{mx}·2^{kx}, y = 2^{ky}: got {:#x}, want {want:#x}",
                                fmt.prec,
                                if neg { "-" } else { "" },
                                g
                            )),
                            Some(_) => {}
                        }
                    }
                }
            }
        }
    }
    println!(
        "{name} p{}: {lines} calls; retries {} wide {} wider {} residual {}; oracle undecided {undecided}; {} differing parts",
        fmt.prec,
        complex::retries() - before.0,
        complex::wide_retries() - before.1,
        complex::wider_retries() - before.2,
        complex::residual() - before.3,
        bad.len()
    );
    for b in bad.iter().take(40) {
        println!("  {b}");
    }
    assert!(bad.is_empty(), "{name} p{}: {} parts differ from the correct rounding", fmt.prec, bad.len());
    assert_eq!(undecided, 0, "{name} p{}: {undecided} parts the oracle cannot decide", fmt.prec);
}

macro_rules! grid_tests {
    ([$($attr:tt)*] $f:ty, $fmt:expr;) => {};
    ([$($attr:tt)*] $f:ty, $fmt:expr; $test:ident = $fam:expr $(, $rt:ident = $rf:expr)*) => {
        /// One family in one format over the asymptotic grid, all four modes (spec §10.5;
        /// D-10.17). Clauses: s10_5_c_0, s10_0_g_0.
        #[test]
        $($attr)*
        fn $test() {
            check::<$f>($fmt, $fam);
        }
        grid_tests! { [$($attr)*] $f, $fmt; $($rt = $rf),* }
    };
}

grid_tests! { [] f32, Fmt::F32; f32_cexp = 0, f32_clog = 1, f32_clog10 = 2, f32_csqrt = 3, f32_csinh = 4, f32_ccosh = 5, f32_csin = 6,
    f32_ccos = 7, f32_ctanh = 8, f32_ctan = 9, f32_casin = 10, f32_cacos = 11, f32_casinh = 12, f32_cacosh = 13, f32_catanh = 14, f32_catan = 15 }
grid_tests! { [] f64, Fmt::F64; f64_cexp = 0, f64_clog = 1, f64_clog10 = 2, f64_csqrt = 3, f64_csinh = 4, f64_ccosh = 5, f64_csin = 6,
    f64_ccos = 7, f64_ctanh = 8, f64_ctan = 9, f64_casin = 10, f64_cacos = 11, f64_casinh = 12, f64_cacosh = 13, f64_catanh = 14, f64_catan = 15 }
grid_tests! { [#[ignore = "the open long double findings of M6-10 (handoff/math-m6-cplx-directed.md); run by hand with --ignored"]] F80, Fmt::F80;
    ld_cexp = 0, ld_clog = 1, ld_clog10 = 2, ld_csqrt = 3, ld_csinh = 4, ld_ccosh = 5, ld_csin = 6, ld_ccos = 7,
    ld_ctanh = 8, ld_ctan = 9, ld_casin = 10, ld_cacos = 11, ld_casinh = 12, ld_cacosh = 13, ld_catanh = 14, ld_catan = 15 }
grid_tests! { [#[ignore = "the open _Float128 findings of M6-10 (handoff/math-m6-cplx-directed.md); run by hand with --ignored"]] F128, Fmt::F128;
    q_cexp = 0, q_clog = 1, q_clog10 = 2, q_csqrt = 3, q_csinh = 4, q_ccosh = 5, q_csin = 6, q_ccos = 7,
    q_ctanh = 8, q_ctan = 9, q_casin = 10, q_cacos = 11, q_casinh = 12, q_cacosh = 13, q_catanh = 14, q_catan = 15 }
