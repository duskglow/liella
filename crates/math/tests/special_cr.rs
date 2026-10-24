//! The special functions of `float`, `double` (Track M milestone 3) and `long double` (5b) against
//! the test oracle: correctly rounded in every rounding direction, with the inexact, underflow and
//! overflow flags of that rounding, on operands across each function's domain (deterministic
//! sample), and for `long double` at the zeros of `lgamma` and the Bessel functions.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::float_cmp, missing_docs)]

use liella_math::elem::Small;
use liella_math::{special, Flags, Rounding, Status};
use liella_mathtools::cmp;
use liella_mathtools::mp::{self, Big, Fmt, Round};

const MODES: [Rounding; 4] = [Rounding::Nearest, Rounding::Up, Rounding::Down, Rounding::Zero];

fn mp_round(m: Rounding) -> Round {
    match m {
        Rounding::Nearest => Round::Nearest,
        Rounding::Up => Round::Up,
        Rounding::Down => Round::Down,
        Rounding::Zero => Round::Zero,
    }
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn f(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (self.next() >> 11) as f64 / (1u64 << 53) as f64 * (hi - lo)
    }
}

type Unary<F> = fn(F, Rounding) -> (F, Status);

/// The nearest value of the format to an `f64`.
trait Near: Small {
    fn near(x: f64) -> Self;
}
impl Near for f32 {
    fn near(x: f64) -> f32 {
        x as f32
    }
}
impl Near for f64 {
    fn near(x: f64) -> f64 {
        x
    }
}

fn lgamma_value<F: Small>(x: F, m: Rounding) -> (F, Status) {
    special::lgamma(x, m).0
}

/// Check `f` against the oracle of `name` at the operands `xs` in every mode.
fn check<F: Near>(name: &str, f: Unary<F>, fmt: Fmt, xs: &[f64]) {
    let mut bad = Vec::new();
    for (i, &x) in xs.iter().enumerate() {
        let xv = F::near(x);
        let xb = xv.to_bits();
        let args = [Big::from_bits(fmt, xb)];
        for mode in MODES {
            let Ok((want, wf)) = mp::correctly_rounded(fmt, mp_round(mode), 1 << 14, |p| cmp::oracle(name, &args, p).unwrap()) else {
                continue;
            };
            let (got, st) = f(xv, mode);
            if Big::from_bits(fmt, want).is_nan() {
                if !Big::from_bits(fmt, got.to_bits()).is_nan() {
                    bad.push(format!("{name} {xb:x} {mode:?}: NaN expected"));
                }
                continue;
            }
            if got.to_bits() != want {
                bad.push(format!("{name} {xb:x} ({x:e}) {mode:?}: got {:x} want {want:x}", got.to_bits()));
            } else if !st.flags.any(Flags::DIVBYZERO) && (st.flags.any(Flags::INEXACT) != wf.inexact || st.flags.any(Flags::UNDERFLOW) != wf.underflow || st.flags.any(Flags::OVERFLOW) != wf.overflow) {
                bad.push(format!("{name} {xb:x} {mode:?}: flags {:?} want X={} U={} O={}", st.flags, wf.inexact, wf.underflow, wf.overflow));
            }
        }
        let _ = i;
    }
    assert!(bad.is_empty(), "{} problems:\n{}", bad.len(), bad.iter().take(30).cloned().collect::<Vec<_>>().join("\n"));
}

fn operands(seed: u64, n: usize, ranges: &[(f64, f64)]) -> Vec<f64> {
    let mut r = Rng(seed);
    (0..n).map(|i| {
        let (lo, hi) = ranges[i % ranges.len()];
        r.f(lo, hi)
    }).collect()
}

fn both(name: &str, f32f: Unary<f32>, f64f: Unary<f64>, xs: &[f64]) {
    check::<f32>(name, f32f, Fmt::F32, xs);
    check::<f64>(name, f64f, Fmt::F64, xs);
}

#[test]
fn erf_and_erfc_are_correctly_rounded() {
    // Clauses: s10_3_c_0, s10_0_g_0 (D-10.15).
    let xs = operands(1, 400, &[(-1.0, 1.0), (-6.0, 6.0), (-30.0, 30.0), (-1e-5, 1e-5), (9.5, 10.5)]);
    let mut xs = xs;
    xs.extend([1e-310, -3e-320, 5.9, 11.9, 12.1, 26.5, 27.2, 27.3, 27.9, -11.5]);
    both("erf", special::erf::<f32>, special::erf::<f64>, &xs);
    // j1 just above and below its tiny shortcut (2^-180), where x³/16 is near 2^-392 of x/2.
    let tiny: Vec<f64> = [-199, -197, -190, -181, -179].iter().map(|&k| p2(k) * 1.25).collect();
    check::<f64>("j1", special::j1::<f64>, Fmt::F64, &tiny);
    both("erfc", special::erfc::<f32>, special::erfc::<f64>, &xs);
}

#[test]
fn lgamma_and_tgamma_are_correctly_rounded() {
    // Clauses: s10_3_c_0, s10_0_g_0 (D-10.15).
    let mut xs = operands(2, 300, &[(0.0, 3.0), (-5.0, 5.0), (-180.0, 180.0), (1e-8, 1e-3), (0.99, 1.01), (1.99, 2.01), (-3.0, -2.0)]);
    xs.extend([1.0e300, 3.0e305, 2.6e305, 0.5, -0.5, 1e-300, -1e-310, 5.0, 23.0, 24.0, 30.0, 171.6, 171.7, -171.5, -180.5, -2.457_024_738_220_8, 1.461_632_144_968_362_3]);
    both("lgamma", lgamma_value::<f32>, lgamma_value::<f64>, &xs);
    both("tgamma", special::tgamma::<f32>, special::tgamma::<f64>, &xs);
}

#[test]
fn bessel_functions_of_orders_0_and_1_are_correctly_rounded() {
    // Clauses: s10_3_c_0, s10_0_g_0 (D-10.15).
    let mut xs = operands(3, 240, &[(0.0, 5.0), (0.0, 100.0), (99.0, 101.0), (100.0, 1e4), (-50.0, 50.0), (1e-10, 1e-3)]);
    xs.extend([1e-300, 2.404_825_557_695_773, 3.831_705_970_207_512, 0.893_576_966_279_167_5, 1e10, 1e22, 1.7e308, 5e-310]);
    both("j0", special::j0::<f32>, special::j0::<f64>, &xs);
    both("j1", special::j1::<f32>, special::j1::<f64>, &xs);
    let pos: Vec<f64> = xs.iter().map(|x| x.abs()).collect();
    both("y0", special::y0::<f32>, special::y0::<f64>, &pos);
    both("y1", special::y1::<f32>, special::y1::<f64>, &pos);
}

/// `jn` and `yn` against the oracle at orders up to 200 (both formats, every mode).
#[test]
fn bessel_functions_of_integer_order_are_correctly_rounded() {
    // Clauses: s10_3_c_0, s10_0_g_0 (D-10.15).
    let mut r = Rng(4);
    let mut bad = Vec::new();
    for i in 0..200 {
        let n = [2i32, 3, 5, 17, -4, -7, 60, 200][i % 8];
        let x = match i % 5 {
            0 => r.f(0.0, 10.0),
            1 => r.f(0.0, 120.0),
            2 => r.f(100.0, 3000.0),
            3 => r.f(1e4, 1e9),
            _ => r.f(-30.0, 30.0),
        };
        for mode in MODES {
            for (yfun, name) in [(false, "jn"), (true, "yn")] {
                if yfun && x < 0.0 {
                    continue;
                }
                let args = [Big::from_f64(x)];
                let want = mp::correctly_rounded(Fmt::F64, mp_round(mode), 1 << 14, |p| if yfun { mp::bessel_y(i64::from(n), &args[0], p) } else { mp::bessel_j(i64::from(n), &args[0], p) });
                let Ok((want, _)) = want else { continue };
                let got = if yfun { special::yn::<f64>(n, x, mode).0 } else { special::jn::<f64>(n, x, mode).0 };
                if u128::from(got.to_bits()) != want {
                    bad.push(format!("{name}({n}, {x:e}) {mode:?}: got {:x} want {want:x}", got.to_bits()));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} problems:\n{}", bad.len(), bad.iter().take(30).cloned().collect::<Vec<_>>().join("\n"));
}

// ---------------------------------------------------------------------------------------------
// `long double` (Track M milestone 5b): the same functions, the same oracle, 64-bit significands.
// ---------------------------------------------------------------------------------------------

use liella_math::{Float, F80};

/// The 80-bit encoding of `x` with low significand bits below the double's filled from `x`'s bits
/// (an operand no `double` can hold), for a normal `x`.
fn ld_bits(x: f64) -> u128 {
    let v = liella_math::float::convert::<f64, F80>(x, Rounding::Nearest).0.to_bits();
    let field = (v >> 64) & 0x7fff;
    if field == 0 || field == 0x7fff { v } else { v | u128::from((x.to_bits() >> 5) & 0x7ff) }
}

/// Check `f` against the oracle of `name` at the 80-bit operands `xs` in every mode.
fn check_ld(name: &str, f: Unary<F80>, xs: &[u128]) {
    let mut bad = Vec::new();
    for &xb in xs {
        let args = [Big::from_bits(Fmt::F80, xb)];
        for mode in MODES {
            let Ok((want, wf)) = mp::correctly_rounded(Fmt::F80, mp_round(mode), 1 << 16, |p| cmp::oracle(name, &args, p).unwrap()) else {
                bad.push(format!("{name}l {xb:x} {mode:?}: oracle undecided"));
                continue;
            };
            let (got, st) = f(F80::from_bits(xb), mode);
            if Big::from_bits(Fmt::F80, want).is_nan() {
                if !Big::from_bits(Fmt::F80, got.to_bits()).is_nan() {
                    bad.push(format!("{name}l {xb:x} {mode:?}: NaN expected"));
                }
                continue;
            }
            if got.to_bits() != want {
                bad.push(format!("{name}l {xb:x} {mode:?}: got {:x} want {want:x}", got.to_bits()));
            } else if !st.flags.any(Flags::DIVBYZERO) && (st.flags.any(Flags::INEXACT) != wf.inexact || st.flags.any(Flags::UNDERFLOW) != wf.underflow || st.flags.any(Flags::OVERFLOW) != wf.overflow) {
                bad.push(format!("{name}l {xb:x} {mode:?}: flags {:?} want X={} U={} O={}", st.flags, wf.inexact, wf.underflow, wf.overflow));
            }
        }
    }
    assert!(bad.is_empty(), "{} problems:\n{}", bad.len(), bad.iter().take(30).cloned().collect::<Vec<_>>().join("\n"));
}

fn ld_operands(seed: u64, n: usize, ranges: &[(f64, f64)]) -> Vec<u128> {
    operands(seed, n, ranges).into_iter().map(ld_bits).collect()
}

/// `F80` encodings of the extremes: the smallest subnormal and normal, the largest finite value.
const LD_TINY: u128 = 1;
const LD_MIN_NORMAL: u128 = (1 << 64) | (1 << 63);
const LD_MAX: u128 = (0x7ffe << 64) | (u64::MAX as u128);
const LD_SIGN: u128 = 1 << 79;

fn is_neg_ld(f: Unary<F80>, b: u128) -> bool {
    Big::from_bits(Fmt::F80, f(F80::from_bits(b), Rounding::Nearest).0.to_bits()).is_neg()
}

/// The encodings around a sign change of `f` (in round-to-nearest) between the normal operands
/// `lo < hi` of one sign (encodings ordered by magnitude): the last operand before the change,
/// the first after it and one neighbour on each side.
fn around_zero(f: Unary<F80>, lo: u128, hi: u128) -> Vec<u128> {
    let (mut a, mut b) = (lo, hi);
    let sa = is_neg_ld(f, a);
    assert_ne!(sa, is_neg_ld(f, b), "no sign change between {lo:x} and {hi:x}");
    while b - a > 1 {
        let mut mid = a + (b - a) / 2;
        if mid & (1 << 63) == 0 {
            // Inside the gap between two binades' encodings: the next binade's first value.
            mid = (mid & !((1u128 << 64) - 1)) | (1 << 63);
        }
        if mid <= a || mid >= b {
            break;
        }
        if is_neg_ld(f, mid) == sa { a = mid } else { b = mid }
    }
    vec![ld_prev(a), a, b, ld_next(b)]
}

/// The next smaller-magnitude encoding of a normal value (crossing into the binade below).
fn ld_prev(b: u128) -> u128 {
    if b & u128::from(u64::MAX) == 1 << 63 { (((b >> 64) - 1) << 64) | u128::from(u64::MAX) } else { b - 1 }
}

/// The next larger-magnitude encoding of a normal value (crossing into the binade above).
fn ld_next(b: u128) -> u128 {
    if b & u128::from(u64::MAX) == u128::from(u64::MAX) { (((b >> 64) + 1) << 64) | (1 << 63) } else { b + 1 }
}

/// The operands around each zero of `f` on `(lo, hi)` (positive), found on a grid of `steps`.
fn zeros_on_grid(f: Unary<F80>, lo: f64, hi: f64, steps: usize) -> Vec<u128> {
    let grid: Vec<u128> = (0..=steps).map(|i| liella_math::float::convert::<f64, F80>(lo + (hi - lo) * i as f64 / steps as f64, Rounding::Nearest).0.to_bits()).collect();
    let mut out = Vec::new();
    for w in grid.windows(2) {
        if is_neg_ld(f, w[0]) != is_neg_ld(f, w[1]) {
            out.extend(around_zero(f, w[0], w[1]));
        }
    }
    out
}

#[test]
fn long_double_erf_and_erfc_are_correctly_rounded() {
    // Clauses: s10_3_c_0, s10_0_g_0 (D-10.20).
    let mut xs = ld_operands(11, 150, &[(-1.0, 1.0), (-6.0, 6.0), (-30.0, 30.0), (-1e-5, 1e-5), (9.5, 10.5), (25.0, 110.0)]);
    xs.extend([LD_TINY, LD_TINY | LD_SIGN, LD_MIN_NORMAL, ld_bits(p2(-100)), ld_bits(-(1.0 + f64::EPSILON) * p2(-101)), ld_bits(11.9), ld_bits(12.1), ld_bits(-11.99)]);
    xs.extend([ld_bits(106.5), ld_bits(106.75), ld_bits(106.76), ld_bits(106.8), ld_bits(107.0), ld_bits(-107.0), LD_MAX]);
    check_ld("erf", special::erf::<F80>, &xs);
    check_ld("erfc", special::erfc::<F80>, &xs);
}

fn lgamma_ld(x: F80, m: Rounding) -> (F80, Status) {
    special::lgamma(x, m).0
}

#[test]
fn long_double_lgamma_and_tgamma_are_correctly_rounded() {
    // Clauses: s10_3_c_0, s10_0_g_0 (D-10.20).
    let mut xs = ld_operands(12, 150, &[(0.0, 3.0), (-5.0, 5.0), (-180.0, 180.0), (1e-8, 1e-3), (0.99, 1.01), (1.99, 2.01), (-3.0, -2.0), (1700.0, 1760.0), (-1760.0, -1700.0)]);
    xs.extend([ld_bits(0.5), ld_bits(-0.5), ld_bits(1e-300), ld_bits(-1e-300), LD_TINY, LD_TINY | LD_SIGN, LD_MIN_NORMAL, LD_MAX, ld_bits(1e300)]);
    xs.extend([ld_bits(p2(-190)), ld_bits(p2(-201)), ld_bits(-p2(-260)), ld_bits(1755.5), ld_bits(-1754.5), ld_bits(-1766.5), ld_bits(-(2.0 - f64::EPSILON) * p2(61)) | 0x3ff]);
    // Next to 1 and 2, where lgamma vanishes.
    for c in [1.0f64, 2.0] {
        let b = liella_math::float::convert::<f64, F80>(c, Rounding::Nearest).0.to_bits();
        xs.extend([ld_prev(b), ld_next(b), ld_next(ld_next(b))]);
    }
    // The zeros of lgamma on (−4, −2): the operands around each (negative: magnitude order).
    for k in [2.0f64, 3.0] {
        let grid: Vec<u128> = (1..64).map(|i| liella_math::float::convert::<f64, F80>(-k - f64::from(i) / 64.0, Rounding::Nearest).0.to_bits()).collect();
        for w in grid.windows(2) {
            if is_neg_ld(lgamma_ld, w[0]) != is_neg_ld(lgamma_ld, w[1]) {
                xs.extend(around_zero(lgamma_ld, w[0], w[1]));
            }
        }
    }
    check_ld("lgamma", lgamma_ld, &xs);
    // tgamma: the factorials (exact up to 92, rounded beyond) and the overflow threshold. Not the
    // smallest normal value: Γ(2^-16382) = 2^16382 − γ + ... needs some 16,400 bits of the oracle
    // (Liella's power-of-two path there is the one 2^-201 checks).
    xs.retain(|&b| b != LD_MIN_NORMAL);
    xs.extend((1..=100u32).chain((1700..=1760).step_by(3)).map(|n| liella_math::float::convert::<f64, F80>(f64::from(n), Rounding::Nearest).0.to_bits()));
    check_ld("tgamma", special::tgamma::<F80>, &xs);
}

/// GATE-SPEED: one test per function (the operands are the same for each), so the four run side by
/// side; `BESSEL_FUNCTIONS` is the list the shards index (the shard tests are 0..4).
const BESSEL_FUNCTIONS: [&str; 4] = ["j0", "j1", "y0", "y1"];

/// GATE-MATH-SHARD: j0 and j1 took 849 s and 685 s on one core in the CI gate, so their operands
/// are split into `BESSEL_SHARDS` shards by index (operand `i` goes to shard `i % BESSEL_SHARDS`);
/// y0 and y1 run whole (`shards` = 1). Each shard checks that it took exactly its share of the
/// operand list, so the shards together check the same operands in the same modes as one test.
const BESSEL_SHARDS: usize = 8;

fn long_double_bessel_function(which: usize, shard: usize, shards: usize) {
    // Clauses: s10_3_c_0, s10_0_g_0 (D-10.20).
    let mut xs = ld_operands(13, 80, &[(0.0, 5.0), (0.0, 100.0), (99.0, 101.0), (100.0, 1e4), (1e-10, 1e-3), (1e5, 1e18)]);
    // Tiny operands on both sides of the shortcuts (2^-60 for j0, 2^-180 for j1), huge ones up to
    // the largest finite value (π·2^16000 to 64 bits among them).
    xs.extend([LD_TINY, LD_MIN_NORMAL, ld_bits(p2(-61)), ld_bits(p2(-59)), ld_bits(p2(-179)), ld_bits(p2(-181)), ld_bits(p2(-197)), ld_bits(1e22), ld_bits(p2(1000))]);
    xs.extend([(0x7e7fu128 << 64) | 0xc90f_daa2_2168_c235, LD_MAX]);
    let fs: [(&str, Unary<F80>); 4] = [("j0", special::j0::<F80>), ("j1", special::j1::<F80>), ("y0", special::y0::<F80>), ("y1", special::y1::<F80>)];
    for (name, f) in fs {
        if name != BESSEL_FUNCTIONS[which] {
            continue;
        }
        let mut ys = xs.clone();
        ys.extend(zeros_on_grid(f, 0.5, 30.0, 120));
        if name.starts_with('j') {
            ys.extend(xs.iter().map(|b| b | LD_SIGN));
        }
        let mine: Vec<u128> = ys.iter().enumerate().filter(|(i, _)| i % shards == shard).map(|(_, &b)| b).collect();
        // The case count: this shard's share of the whole list (shard k of n takes ceil((len - k) / n)).
        let share = (ys.len() + shards - 1 - shard) / shards;
        eprintln!("{name}l shard {shard}/{shards}: {} of {} operands, {} cases", mine.len(), ys.len(), mine.len() * MODES.len());
        assert_eq!(mine.len(), share, "{name}l shard {shard}/{shards}: checked {} operands, its share of {} is {share}", mine.len(), ys.len());
        check_ld(name, f, &mine);
    }
}

#[test]
fn long_double_bessel_functions_are_correctly_rounded_j0_s0() {
    // no spec clause: a shard of the long-double Bessel correct-rounding sweep.
    long_double_bessel_function(0, 0, BESSEL_SHARDS);
}

#[test]
fn long_double_bessel_functions_are_correctly_rounded_j0_s1() {
    // no spec clause: a shard of the long-double Bessel correct-rounding sweep.
    long_double_bessel_function(0, 1, BESSEL_SHARDS);
}

#[test]
fn long_double_bessel_functions_are_correctly_rounded_j0_s2() {
    // no spec clause: a shard of the long-double Bessel correct-rounding sweep.
    long_double_bessel_function(0, 2, BESSEL_SHARDS);
}

#[test]
fn long_double_bessel_functions_are_correctly_rounded_j0_s3() {
    // no spec clause: a shard of the long-double Bessel correct-rounding sweep.
    long_double_bessel_function(0, 3, BESSEL_SHARDS);
}

#[test]
fn long_double_bessel_functions_are_correctly_rounded_j0_s4() {
    // no spec clause: a shard of the long-double Bessel correct-rounding sweep.
    long_double_bessel_function(0, 4, BESSEL_SHARDS);
}

#[test]
fn long_double_bessel_functions_are_correctly_rounded_j0_s5() {
    // no spec clause: a shard of the long-double Bessel correct-rounding sweep.
    long_double_bessel_function(0, 5, BESSEL_SHARDS);
}

#[test]
fn long_double_bessel_functions_are_correctly_rounded_j0_s6() {
    // no spec clause: a shard of the long-double Bessel correct-rounding sweep.
    long_double_bessel_function(0, 6, BESSEL_SHARDS);
}

#[test]
fn long_double_bessel_functions_are_correctly_rounded_j0_s7() {
    // no spec clause: a shard of the long-double Bessel correct-rounding sweep.
    long_double_bessel_function(0, 7, BESSEL_SHARDS);
}

#[test]
fn long_double_bessel_functions_are_correctly_rounded_j1_s0() {
    // no spec clause: a shard of the long-double Bessel correct-rounding sweep.
    long_double_bessel_function(1, 0, BESSEL_SHARDS);
}

#[test]
fn long_double_bessel_functions_are_correctly_rounded_j1_s1() {
    // no spec clause: a shard of the long-double Bessel correct-rounding sweep.
    long_double_bessel_function(1, 1, BESSEL_SHARDS);
}

#[test]
fn long_double_bessel_functions_are_correctly_rounded_j1_s2() {
    // no spec clause: a shard of the long-double Bessel correct-rounding sweep.
    long_double_bessel_function(1, 2, BESSEL_SHARDS);
}

#[test]
fn long_double_bessel_functions_are_correctly_rounded_j1_s3() {
    // no spec clause: a shard of the long-double Bessel correct-rounding sweep.
    long_double_bessel_function(1, 3, BESSEL_SHARDS);
}

#[test]
fn long_double_bessel_functions_are_correctly_rounded_j1_s4() {
    // no spec clause: a shard of the long-double Bessel correct-rounding sweep.
    long_double_bessel_function(1, 4, BESSEL_SHARDS);
}

#[test]
fn long_double_bessel_functions_are_correctly_rounded_j1_s5() {
    // no spec clause: a shard of the long-double Bessel correct-rounding sweep.
    long_double_bessel_function(1, 5, BESSEL_SHARDS);
}

#[test]
fn long_double_bessel_functions_are_correctly_rounded_j1_s6() {
    // no spec clause: a shard of the long-double Bessel correct-rounding sweep.
    long_double_bessel_function(1, 6, BESSEL_SHARDS);
}

#[test]
fn long_double_bessel_functions_are_correctly_rounded_j1_s7() {
    // no spec clause: a shard of the long-double Bessel correct-rounding sweep.
    long_double_bessel_function(1, 7, BESSEL_SHARDS);
}

#[test]
fn long_double_bessel_functions_are_correctly_rounded_y0() {
    // no spec clause: a shard of the long-double Bessel correct-rounding sweep.
    long_double_bessel_function(2, 0, 1);
}

#[test]
fn long_double_bessel_functions_are_correctly_rounded_y1() {
    // no spec clause: a shard of the long-double Bessel correct-rounding sweep.
    long_double_bessel_function(3, 0, 1);
}

/// `jnl` and `ynl` against the oracle at orders up to 200 (every mode).
#[test]
fn long_double_bessel_functions_of_integer_order_are_correctly_rounded() {
    // Clauses: s10_3_c_0, s10_0_g_0 (D-10.20).
    let mut r = Rng(14);
    let mut bad = Vec::new();
    for i in 0..80 {
        let n = [2i32, 3, 5, 17, -4, -7, 60, 200][i % 8];
        let x = match i % 5 {
            0 => r.f(0.0, 10.0),
            1 => r.f(0.0, 120.0),
            2 => r.f(100.0, 3000.0),
            3 => r.f(1e4, 1e9),
            _ => r.f(-30.0, 30.0),
        };
        let xb = ld_bits(x);
        for mode in MODES {
            for (yfun, name) in [(false, "jnl"), (true, "ynl")] {
                if yfun && x < 0.0 {
                    continue;
                }
                let args = [Big::from_bits(Fmt::F80, xb)];
                let want = mp::correctly_rounded(Fmt::F80, mp_round(mode), 1 << 16, |p| if yfun { mp::bessel_y(i64::from(n), &args[0], p) } else { mp::bessel_j(i64::from(n), &args[0], p) });
                let Ok((want, _)) = want else {
                    bad.push(format!("{name}({n}, {xb:x}) {mode:?}: oracle undecided"));
                    continue;
                };
                let xv = F80::from_bits(xb);
                let got = if yfun { special::yn::<F80>(n, xv, mode).0 } else { special::jn::<F80>(n, xv, mode).0 };
                if got.to_bits() != want {
                    bad.push(format!("{name}({n}, {xb:x}) {mode:?}: got {:x} want {want:x}", got.to_bits()));
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} problems:\n{}", bad.len(), bad.iter().take(30).cloned().collect::<Vec<_>>().join("\n"));
}

/// 2^k (exact).
fn p2(k: i32) -> f64 {
    2f64.powi(k)
}

/// `jn` at tiny operands where `(x/2)^n / n!` is a short dyadic number (`n = 2`, or the odd part
/// of `n!` dividing `odd(x)^n`) and the correction lies beyond 384 bits: just inside it.
#[test]
fn bessel_jn_beside_its_dyadic_leading_term() {
    // Clauses: s10_3_c_0, s10_0_g_0 (D-10.20).
    let mut bad = Vec::new();
    for (n, odd, e) in [(2i32, 1.0f64, -200), (2, 1.0, -300), (-2, 3.0, -190), (3, 3.0, -201), (-3, 3.0, -201), (3, 1.0, -200), (4, 3.0, -250), (5, 15.0, -220), (6, 45.0, -199), (2, 1.0, -1000)] {
        for neg in [false, true] {
            let x = if neg { -odd * p2(e) } else { odd * p2(e) };
            for (fmt, xb) in [(Fmt::F64, u128::from(x.to_bits())), (Fmt::F80, ld_bits(x))] {
                let args = [Big::from_bits(fmt, xb)];
                for mode in MODES {
                    let Ok((want, _)) = mp::correctly_rounded(fmt, mp_round(mode), 1 << 14, |p| mp::bessel_j(i64::from(n), &args[0], p)) else {
                        bad.push(format!("jn({n}, {xb:x}) {mode:?}: oracle undecided"));
                        continue;
                    };
                    let got = if fmt == Fmt::F64 { u128::from(special::jn::<f64>(n, x, mode).0.to_bits()) } else { special::jn::<F80>(n, F80::from_bits(xb), mode).0.to_bits() };
                    if got != want {
                        bad.push(format!("jn({n}, {xb:x}) {fmt:?} {mode:?}: got {got:x} want {want:x}"));
                    }
                }
            }
        }
    }
    assert!(bad.is_empty(), "{} problems:\n{}", bad.len(), bad.join("\n"));
}

// ---------------------------------------------------------------------------------------------
// `_Float128` next to the zeros of the Bessel functions (MATH-M5-MP-WIDE, D-10.20): the operands
// nearest each zero on 20 <= x < 160, where the 384-bit power series (below 100) and Hankel's
// expansion (above) cancel or truncate too much to decide every rounding and the 768-bit step
// of Ziv's strategy takes over. The oracle works in up to 2^16 bits.
// ---------------------------------------------------------------------------------------------

use liella_math::F128;

type BesselQ = fn(F128, Rounding) -> (F128, Status);

fn is_neg_q(f: BesselQ, b: u128) -> bool {
    f(F128(b), Rounding::Nearest).0.to_bits() >> 127 == 1
}

/// The positive encodings around each sign change of `f` on a grid of `steps` over `(lo, hi)`
/// (positive encodings are ordered like their values): the last two before the change and
/// the first two after it.
fn zeros_q(f: BesselQ, lo: f64, hi: f64, steps: usize) -> Vec<u128> {
    let enc = |x: f64| liella_math::float::convert::<f64, F128>(x, Rounding::Nearest).0.to_bits();
    let grid: Vec<u128> = (0..=steps).map(|i| enc(lo + (hi - lo) * i as f64 / steps as f64)).collect();
    let mut out = Vec::new();
    for w in grid.windows(2) {
        let (mut a, mut b) = (w[0], w[1]);
        let sa = is_neg_q(f, a);
        if sa == is_neg_q(f, b) {
            continue;
        }
        while b - a > 1 {
            let mid = a + (b - a) / 2;
            if is_neg_q(f, mid) == sa { a = mid } else { b = mid }
        }
        out.extend([a - 1, a, b, b + 1]);
    }
    out
}

/// Check `f` (named `label`) against `oracle` at the `_Float128` operands `xs` in every mode
/// (result and flags).
fn check_q(label: &str, f: BesselQ, oracle: &dyn Fn(&Big, u32) -> mp::Approx, xs: &[u128]) {
    let mut bad = Vec::new();
    for &xb in xs {
        let x = Big::from_bits(Fmt::F128, xb);
        for mode in MODES {
            let Ok((want, wf)) = mp::correctly_rounded(Fmt::F128, mp_round(mode), 1 << 16, |p| oracle(&x, p)) else {
                bad.push(format!("{label} {xb:x} {mode:?}: oracle undecided"));
                continue;
            };
            let (got, st) = f(F128(xb), mode);
            if got.to_bits() != want {
                bad.push(format!("{label} {xb:x} {mode:?}: got {:x} want {want:x}", got.to_bits()));
            } else if st.flags.any(Flags::INEXACT) != wf.inexact || st.flags.any(Flags::UNDERFLOW) != wf.underflow {
                bad.push(format!("{label} {xb:x} {mode:?}: flags {:?} want X={} U={}", st.flags, wf.inexact, wf.underflow));
            }
        }
    }
    assert!(bad.is_empty(), "{} problems:\n{}", bad.len(), bad.iter().take(30).cloned().collect::<Vec<_>>().join("\n"));
}

/// Shard `which` of the `_Float128` Bessel functions next to their zeros: J0, J1, Y0, Y1, then
/// orders 2, 3 and 7 of `jnf128`/`ynf128` (fewer zeros: a coarser window).
fn float128_bessel_next_to_zeros(which: usize) {
    let fs: [(&str, BesselQ, i64, bool); 4] = [("j0f128", special::j0::<F128>, 0, false), ("j1f128", special::j1::<F128>, 1, false), ("y0f128", special::y0::<F128>, 0, true), ("y1f128", special::y1::<F128>, 1, true)];
    let orders: [(&str, BesselQ, i64, bool); 4] = [
        ("jnf128(2)", |x, m| special::jn::<F128>(2, x, m), 2, false),
        ("ynf128(3)", |x, m| special::yn::<F128>(3, x, m), 3, true),
        ("jnf128(7)", |x, m| special::jn::<F128>(7, x, m), 7, false),
        ("ynf128(7)", |x, m| special::yn::<F128>(7, x, m), 7, true),
    ];
    let (label, f, n, y) = if which < 4 { fs[which] } else { orders[which - 4] };
    let xs = if which < 4 { zeros_q(f, 20.0, 160.0, 280) } else { zeros_q(f, 20.0, 160.0, 140).into_iter().step_by(3).collect() };
    assert!(xs.len() >= 40, "{label}: only {} operands next to zeros", xs.len());
    let oracle = move |x: &Big, p: u32| if y { mp::bessel_y(n, x, p) } else { mp::bessel_j(n, x, p) };
    check_q(label, f, &oracle, &xs);
}

#[test]
fn float128_bessel_functions_are_correctly_rounded_next_to_zeros_j0() {
    // Clauses: s10_3_c_0 (D-10.20).
    float128_bessel_next_to_zeros(0);
}

#[test]
fn float128_bessel_functions_are_correctly_rounded_next_to_zeros_j1() {
    // Clauses: s10_3_c_0 (D-10.20).
    float128_bessel_next_to_zeros(1);
}

#[test]
fn float128_bessel_functions_are_correctly_rounded_next_to_zeros_y0() {
    // Clauses: s10_3_c_0 (D-10.20).
    float128_bessel_next_to_zeros(2);
}

#[test]
fn float128_bessel_functions_are_correctly_rounded_next_to_zeros_y1() {
    // Clauses: s10_3_c_0 (D-10.20).
    float128_bessel_next_to_zeros(3);
}

#[test]
fn float128_bessel_functions_of_integer_order_are_correctly_rounded_next_to_zeros() {
    // Clauses: s10_3_c_0 (D-10.20).
    for which in 4..8 {
        float128_bessel_next_to_zeros(which);
    }
}
