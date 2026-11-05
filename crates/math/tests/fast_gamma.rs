//! The `double` `lgamma`/`tgamma` fast tier (`liella_math::fast_gamma`) against the oracle
//! (`liella-mathtools::mp`) and the 384-bit path, spec §10.3 and D-10.13 (correct rounding in
//! every mode): the oracle's result or `None` on random operands, slot edges, the zero windows
//! and both sides of the negative poles; kernels within their bounds; identity with the accurate
//! path (result and status); the fallback rate on the bench's range `[-10, 10]`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::arithmetic_side_effects, clippy::float_cmp, missing_docs)]

use liella_math::status::{Rounding, Status};
use liella_math::{fast_gamma, special, stats};
use liella_mathtools::cmp;
use liella_mathtools::mp::{self, Approx, Big, Fmt, Round};

const MODES: [(Rounding, Round); 4] =
    [(Rounding::Nearest, Round::Nearest), (Rounding::Up, Round::Up), (Rounding::Down, Round::Down), (Rounding::Zero, Round::Zero)];

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * ((self.next() >> 11) as f64 * f64::from_bits(0x3ca0_0000_0000_0000))
    }
    /// The bench's range half the time, log-uniform magnitudes with a random sign otherwise.
    fn operand(&mut self) -> f64 {
        let x = match self.next() % 4 {
            0 | 1 => self.uniform(-10.0, 10.0),
            r => {
                let (lo, hi) = (fast_gamma::TINY.to_bits(), fast_gamma::HI.to_bits());
                let m = f64::from_bits(lo + self.next() % (hi - lo));
                if r == 2 { m } else { -m }
            }
        };
        if fast_gamma::domain(x, true) { x } else { 0.75 }
    }
}

fn fast_lg(x: f64, m: Rounding) -> Option<(f64, Status)> {
    if x == 1.0 || x == 2.0 { None } else { fast_gamma::lgamma(x, m).map(|r| r.0) }
}
fn entry_lg(x: f64, m: Rounding) -> (f64, Status) {
    special::lgamma::<f64>(x, m).0
}
fn acc_lg(x: f64, m: Rounding) -> (f64, Status) {
    special::lgamma_accurate::<f64>(x, m).0
}
type Fast = fn(f64, Rounding) -> Option<(f64, Status)>;
type Entry = fn(f64, Rounding) -> (f64, Status);
const FUNCS: [(&str, Fast, Entry, Entry); 2] = [
    ("lgamma", fast_lg, entry_lg, acc_lg),
    ("tgamma", fast_gamma::tgamma, special::tgamma::<f64>, special::tgamma_accurate::<f64>),
];

fn oracle(name: &str, x: f64, mode: Round) -> u64 {
    let args = [Big::from_bits(Fmt::F64, u128::from(x.to_bits()))];
    let (bits, _) = mp::correctly_rounded(Fmt::F64, mode, 1 << 15, |p| {
        cmp::oracle(name, &args, p).unwrap_or(Approx { value: Big::nan(), exact: true })
    })
    .unwrap_or_else(|_| panic!("oracle undetermined for {name}({x:e})"));
    bits as u64
}

/// Slot edges of `LG_T`, the zero windows, both sides of every pole in `(-16, 0)`, the range ends.
fn hard_operands() -> Vec<f64> {
    let mut v = Vec::new();
    let mut edge = |x: f64| {
        let b = x.to_bits();
        v.extend([x, f64::from_bits(b - 1), f64::from_bits(b + 1), -x, -f64::from_bits(b - 1), -f64::from_bits(b + 1)]);
    };
    for e in -5..=3 {
        for j in 0..16 {
            edge(2f64.powi(e) * (1.0 + f64::from(j) / 16.0));
        }
    }
    for x in [fast_gamma::TINY, 0.968_75, 1.0625, 1.9375, 2.125, 0.5, 1.4616321449683622, 2.0 - 2f64.powi(-40), 1.0 + 2f64.powi(-40)] {
        edge(x);
    }
    v.retain(|&x| fast_gamma::domain(x, true));
    v
}

#[test]
fn double_gamma_fast_tier_rounds_correctly() {
    // Clauses: s10_3_c_0 (D-10.13).
    let mut rng = Rng(0xbb67_ae85_84ca_a73b);
    let mut ops: Vec<f64> = (0..300).map(|_| rng.operand()).collect();
    ops.extend(hard_operands());
    for (name, fast, entry, _) in FUNCS {
        let mut settled = 0u32;
        for &x in &ops {
            for (mode, round) in MODES {
                let want = oracle(name, x, round);
                if let Some((v, _)) = fast(x, mode) {
                    assert_eq!(v.to_bits(), want, "{name}({x:e}) fast tier in {mode:?}: {v:e}, oracle {:e}", f64::from_bits(want));
                    settled += 1;
                }
                let v = entry(x, mode).0;
                assert_eq!(v.to_bits(), want, "{name}({x:e}) entry in {mode:?}: {v:e}, oracle {:e}", f64::from_bits(want));
            }
        }
        assert!(settled as usize * 100 >= ops.len() * 4 * 97, "{name}: the fast tier settled only {settled}");
    }
}

#[test]
fn double_gamma_fast_tier_matches_the_accurate_path() {
    // Clauses: s10_3_c_0 (D-10.13).
    let n: u32 = std::env::var("LIELLA_GAMMA_SWEEP").ok().and_then(|s| s.parse().ok()).unwrap_or(1500);
    for (name, fast, _, accurate) in FUNCS {
        let mut rng = Rng(0x3c6e_f372_fe94_f82b ^ name.len() as u64);
        for _ in 0..n {
            let x = rng.operand();
            for (mode, _) in MODES {
                if let Some((v, st)) = fast(x, mode) {
                    let (w, sw) = accurate(x, mode);
                    assert_eq!((v.to_bits(), st), (w.to_bits(), sw), "{name}({x:e}) in {mode:?}: fast {v:e}, accurate {w:e}");
                }
            }
        }
    }
}

#[test]
fn double_gamma_kernels_stay_within_their_bounds_and_rarely_fall_back() {
    // Clauses: s10_3_c_0 (D-10.13).
    let mut rng = Rng(0xa54f_f53a_5f1d_36f1);
    let mut worst = [0f64; 2];
    let mut ops: Vec<f64> = (0..3000).map(|_| rng.operand()).collect();
    ops.extend(hard_operands());
    for &x in &ops {
        let exact = cmp::oracle("lgamma", &[Big::from_f64(x)], 240).unwrap().value;
        if let Some((y, eps, _)) = fast_gamma::lgamma_kernel(x) {
            let got = Big::from_f64(y.hi).add(&Big::from_f64(y.lo));
            let r = exact.sub(&got).abs().to_f64() / eps;
            assert!(r <= 1.0, "lgamma_kernel({x:e}): error {r} times its bound");
            worst[0] = worst[0].max(r);
        }
        let exact = cmp::oracle("tgamma", &[Big::from_f64(x)], 240).unwrap().value;
        let (y, eps, m) = fast_gamma::tgamma_kernel(x).unwrap_or_else(|| panic!("tgamma_kernel({x:e}) declined"));
        let got = Big::from_f64(y.hi).add(&Big::from_f64(y.lo)).mul_pow2(i64::from(m));
        let r = exact.sub(&got).abs().mul_pow2(-i64::from(m)).to_f64() / eps;
        assert!(r <= 1.0, "tgamma_kernel({x:e}): error {r} times its bound");
        worst[1] = worst[1].max(r);
    }
    eprintln!("worst error over bound: lgamma {:.3e}, tgamma {:.3e}", worst[0], worst[1]);
    // The fallback rate on the bench's range, Nearest and Up alternately (10^5 calls each).
    for (name, _, entry, _) in FUNCS {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ name.len() as u64);
        let before = stats::counts(name);
        for n in 0..100_000u32 {
            let x = rng.uniform(-10.0, 10.0);
            core::hint::black_box(entry(x, if n % 2 == 0 { Rounding::Nearest } else { Rounding::Up }));
        }
        let c = stats::counts(name).since(before);
        eprintln!("{name}: fast {} fallback {} of 100000 on [-10, 10]", c.fast, c.fallback);
        c.check_fallback(name, 500).unwrap_or_else(|e| panic!("{name}: fallback rate {e:?}"));
    }
}
