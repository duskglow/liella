//! The `double` exp/log fast paths (Track M milestone 6, M6-4, `liella_math::fast64`) against the
//! test oracle (`liella-mathtools::mp`), spec §10.3 and D-10.13 (every result correctly rounded in
//! the caller's rounding direction). Each fast path either returns the correctly rounded result or
//! `None` (its rounding test failed; the caller then runs the accurate path), in all four modes:
//! on random operands over each function's range, on operands whose results sit next to a rounding
//! boundary (where the rounding test must send them to the accurate path), and on subnormal
//! results. The public entry (`exp_log::exp::<f64>` and the others, fast path plus fallback) must
//! give the oracle's result too, and the fast path must be taken for all but at most 1 in 10^4
//! random operands (the M6-0 fallback-rate counter, `liella_math::stats`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::arithmetic_side_effects, missing_docs)]

use liella_math::status::{Rounding, Status};
use liella_math::{exp_log, fast64, stats};
use liella_mathtools::cmp;
use liella_mathtools::mp::{self, Approx, Big, Fmt, Round};

const MODES: [(Rounding, Round); 4] =
    [(Rounding::Nearest, Round::Nearest), (Rounding::Up, Round::Up), (Rounding::Down, Round::Down), (Rounding::Zero, Round::Zero)];

type Fast = fn(f64, Rounding) -> Option<(f64, Status)>;
type Entry = fn(f64, Rounding) -> (f64, Status);

/// (name, fast path, public entry, random operand range or `None` for the logarithms).
type Case = (&'static str, Fast, Entry, Option<(f64, f64)>);

const FUNCS: [Case; 6] = [
    ("exp", fast64::exp, exp_log::exp::<f64>, Some((-745.1, 709.7))),
    ("exp2", fast64::exp2, exp_log::exp2::<f64>, Some((-1074.9, 1023.9))),
    ("exp10", fast64::exp10, exp_log::exp10::<f64>, Some((-323.6, 308.2))),
    ("log", fast64::log, exp_log::log::<f64>, None),
    ("log2", fast64::log2, exp_log::log2::<f64>, None),
    ("log10", fast64::log10, exp_log::log10::<f64>, None),
];

/// xorshift64*: a fixed, reproducible operand stream.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 * f64::from_bits(0x3ca0_0000_0000_0000) // 2^-53
    }
    /// A random operand of `range` (the exponentials), or a random positive finite `double`
    /// (subnormals included) for the logarithms.
    fn operand(&mut self, range: Option<(f64, f64)>) -> f64 {
        match range {
            Some((lo, hi)) => lo + (hi - lo) * self.unit(),
            None => f64::from_bits(1 + self.next() % 0x7fef_ffff_ffff_ffff),
        }
    }
}

/// Operands outside the fast paths' stated domains (the entry keeps them away: exact results and
/// the tiny-argument shortcut of the exponentials).
fn excluded(name: &str, x: f64) -> bool {
    match name {
        "exp" => x.abs() < f64::from_bits(0x3c30_0000_0000_0000), // 2^-60
        "exp2" | "exp10" => x.fract() == 0.0 || x.abs() < f64::from_bits(0x3c30_0000_0000_0000),
        "log" => x == 1.0,
        "log2" => x.to_bits().count_ones() == 1 || x.to_bits() & 0x000f_ffff_ffff_ffff == 0,
        "log10" => (0..=22).any(|k| x == 10f64.powi(k)),
        _ => false,
    }
}

fn oracle(name: &str, x: f64, mode: Round) -> u64 {
    let args = [Big::from_bits(Fmt::F64, u128::from(x.to_bits()))];
    let (bits, _) = mp::correctly_rounded(Fmt::F64, mode, 1 << 15, |p| {
        cmp::oracle(name, &args, p).unwrap_or(Approx { value: Big::nan(), exact: true })
    })
    .unwrap_or_else(|_| panic!("oracle undetermined for {name}({x:e})"));
    bits as u64
}

/// The fast path and the public entry both give the oracle's result for `x` in every mode.
/// Returns how many modes the fast path settled.
fn check(name: &str, fast: Fast, entry: Entry, x: f64) -> u32 {
    let mut settled = 0;
    for (mode, round) in MODES {
        let want = oracle(name, x, round);
        if let Some((v, _)) = fast(x, mode) {
            assert_eq!(v.to_bits(), want, "{name}({x:e}) fast path in {mode:?}: {v:e}, oracle {:e}", f64::from_bits(want));
            settled += 1;
        }
        let v = entry(x, mode).0;
        assert_eq!(v.to_bits(), want, "{name}({x:e}) entry in {mode:?}: {v:e}, oracle {:e}", f64::from_bits(want));
    }
    settled
}

/// Operands whose results lie within about 2^-100 relative of a `double` or of a midpoint
/// between two, and the subnormal results: the rounding test must refuse the first kind (or
/// settle them right) and `settle`'s subnormal branch must round the second kind right.
fn hard_operands(name: &str) -> Vec<f64> {
    let mut v = Vec::new();
    let ulp = f64::EPSILON; // 2^-52
    for k in 1..=48u32 {
        let d = f64::from(k) * ulp;
        match name {
            // e^(k·2^-53) = 1 + k·2^-53 + k²·2^-107 + ...: a midpoint (odd k) or a double (even k).
            "exp" | "exp2" | "exp10" => v.extend([0.5 * d, -0.5 * d, -0.25 * d]),
            // ln(1 ± k·2^-52) = ±k·2^-52 − k²·2^-105 ± ...: next to a double.
            _ => v.extend([1.0 + d, 1.0 - 0.5 * d]),
        }
    }
    let mut rng = Rng(0x6a09_e667_f3bc_c908 ^ name.len() as u64);
    let sub = match name {
        "exp" => Some((-745.1, -708.4)),
        "exp2" => Some((-1074.9, -1022.0)),
        "exp10" => Some((-323.6, -307.6)),
        _ => None,
    };
    if let Some(r) = sub {
        v.extend((0..400).map(|_| rng.operand(Some(r))));
        // Each subnormal binade's edge and the smallest results.
        v.extend((0..52).map(|i| (-1022.0 - f64::from(i) - 0.5) * if name == "exp" { core::f64::consts::LN_2 } else if name == "exp10" { core::f64::consts::LOG10_2 } else { 1.0 }));
    }
    v.retain(|&x| !excluded(name, x));
    v
}

#[test]
fn double_exp_log_fast_paths_round_correctly_and_rarely_fall_back() {
    // Clauses: s10_3_c_0 (D-10.13).
    // The fallback rate first and alone (the counters are global): 10^5 random operands per
    // function through the public entry, in round-to-nearest and one directed mode.
    for (name, _, entry, range) in FUNCS {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ name.len() as u64);
        let before = stats::counts(name);
        let mut n = 0u64;
        while n < 100_000 {
            let x = rng.operand(range);
            if excluded(name, x) || (range.is_some() && !(-708.0..=709.0).contains(&(x * if name == "exp2" { core::f64::consts::LN_2 } else if name == "exp10" { core::f64::consts::LN_10 } else { 1.0 }))) {
                continue; // the subnormal exponentials are checked below, not counted
            }
            let mode = if n % 2 == 0 { Rounding::Nearest } else { Rounding::Up };
            core::hint::black_box(entry(x, mode));
            n += 1;
        }
        let c = stats::counts(name).since(before);
        assert!(c.fast + c.fallback >= 100_000, "{name}: the fast path was not consulted ({c:?})");
        c.check_fallback(name, 10_000).unwrap_or_else(|e| panic!("{name}: fallback rate {e:?}"));
    }
    // Random operands, then the hard and subnormal ones, against the oracle in all four modes.
    for (name, fast, entry, range) in FUNCS {
        let mut rng = Rng(0xbb67_ae85_84ca_a73b ^ name.len() as u64);
        let (mut settled, mut tried) = (0u32, 0u32);
        for _ in 0..500 {
            let x = rng.operand(range);
            if !excluded(name, x) {
                settled += check(name, fast, entry, x);
                tried += 4;
            }
        }
        assert!(settled * 100 >= tried * 99, "{name}: the fast path settled only {settled} of {tried} random cases");
        let hard = hard_operands(name);
        assert!(!hard.is_empty());
        let mut sub_settled = 0u32;
        for x in hard {
            let s = check(name, fast, entry, x);
            if entry(x, Rounding::Nearest).0.abs() < f64::MIN_POSITIVE {
                sub_settled += s;
            }
        }
        if range.is_some() {
            assert!(sub_settled > 1000, "{name}: the fast path settled only {sub_settled} subnormal cases");
        }
    }
}

fn oracle2(x: f64, y: f64, mode: Round) -> u64 {
    let args = [Big::from_bits(Fmt::F64, u128::from(x.to_bits())), Big::from_bits(Fmt::F64, u128::from(y.to_bits()))];
    let (bits, _) = mp::correctly_rounded(Fmt::F64, mode, 1 << 15, |p| {
        cmp::oracle("pow", &args, p).unwrap_or(Approx { value: Big::nan(), exact: true })
    })
    .unwrap_or_else(|_| panic!("oracle undetermined for pow({x:e}, {y:e})"));
    bits as u64
}

/// `pow(x, y)` through the fast path (when it settles) and the public entry against the oracle in
/// every mode; returns how many modes the fast path settled. The fast path is called the way the
/// entry calls it (`|x|`, the sign from an odd integral `y`), after the entry's exact cases.
fn check_pow(x: f64, y: f64) -> u32 {
    let neg = x < 0.0 && y.fract() == 0.0 && (y * 0.5).fract() != 0.0;
    let mut settled = 0;
    for (mode, round) in MODES {
        let want = oracle2(x, y, round);
        if let Some((v, _)) = fast64::pow(x.abs(), y, neg, mode) {
            assert_eq!(v.to_bits(), want, "pow({x:e}, {y:e}) fast path in {mode:?}: {v:e}, oracle {:e}", f64::from_bits(want));
            settled += 1;
        }
        let v = liella_math::pow::pow::<f64>(x, y, mode).0;
        assert_eq!(v.to_bits(), want, "pow({x:e}, {y:e}) entry in {mode:?}: {v:e}, oracle {:e}", f64::from_bits(want));
    }
    settled
}

#[test]
fn double_pow_fast_path_rounds_correctly_and_rarely_falls_back() {
    // Clauses: s10_3_c_0 (D-10.13).
    // The fallback rate: 10^5 random operands (x in [0.01, 10], y in [−10, 10], the M6-0 bench
    // ranges) through the public entry, in round-to-nearest and one directed mode.
    let mut rng = Rng(0x3c6e_f372_fe94_f82b);
    let before = stats::counts("pow");
    for n in 0..100_000u32 {
        let (x, y) = (0.01 + 9.99 * rng.unit(), -10.0 + 20.0 * rng.unit());
        let mode = if n % 2 == 0 { Rounding::Nearest } else { Rounding::Down };
        core::hint::black_box(liella_math::pow::pow::<f64>(x, y, mode));
    }
    let c = stats::counts("pow").since(before);
    assert!(c.fast + c.fallback >= 100_000, "pow: the fast path was not consulted ({c:?})");
    c.check_fallback("pow", 10_000).unwrap_or_else(|e| panic!("pow: fallback rate {e:?}"));
    // Random operands against the oracle: the bench ranges, wide exponents, negative x with odd
    // integral y.
    let (mut settled, mut tried) = (0u32, 0u32);
    for i in 0..600 {
        let (x, y) = match i % 3 {
            0 => (0.01 + 9.99 * rng.unit(), -10.0 + 20.0 * rng.unit()),
            1 => {
                let x = f64::from_bits(0x0010_0000_0000_0000 + rng.next() % 0x7fd0_0000_0000_0000);
                (x, (-700.0 + 1400.0 * rng.unit()) / x.ln())
            }
            _ => (-(0.01 + 9.99 * rng.unit()), f64::from(2 * (rng.next() % 60) as i32 + 3) * if rng.next() % 2 == 0 { 1.0 } else { -1.0 }),
        };
        settled += check_pow(x, y);
        tried += 4;
    }
    assert!(settled * 100 >= tried * 99, "pow: the fast path settled only {settled} of {tried} random cases");
    // Results within about 2^-100 relative of a double or a midpoint: (1 + k·2^-52)^3 and
    // (1 − k·2^-53)^-2 (not exact: too many bits), and y·ln x tiny.
    for k in 1..=40u32 {
        let d = f64::from(k) * f64::EPSILON;
        for (x, y) in [(1.0 + d, 3.0), (1.0 - 0.5 * d, -2.0), (1.0 + d, -3.0), (-(1.0 + d), 5.0), (1.0 + d, 1e-30 * f64::from(k))] {
            check_pow(x, y);
        }
    }
    // Subnormal results, positive and negative: the fast path must settle most of them.
    let mut sub_settled = 0u32;
    for i in 0..300 {
        let x = 0.05 + 0.9 * rng.unit();
        let t = -745.0 + 36.0 * rng.unit();
        let (x, y) = if i % 3 == 0 { (-x, (2.0 * (t / x.ln() * 0.5).round()) + 1.0) } else { (x, t / x.ln()) };
        let s = check_pow(x, y);
        if liella_math::pow::pow::<f64>(x, y, Rounding::Nearest).0.abs() < f64::MIN_POSITIVE {
            sub_settled += s;
        }
    }
    assert!(sub_settled > 600, "pow: the fast path settled only {sub_settled} subnormal cases");
}
