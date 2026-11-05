//! The `double` Bessel fast tier (`liella_math::fast_bessel`: `j0`, `j1`, `y0`, `y1`) against the
//! test oracle (`liella-mathtools::mp`) and the 384-bit path it falls back to, spec §10.3 and
//! D-10.13 (every result correctly rounded in the caller's rounding direction). Double operands
//! cannot be swept, so the directed samples are the hard ones: every table slot's ends, the region
//! switches (`2^-27`, `2^-5`, 32, `2^-1000`), operands next to the functions' zeros, huge
//! operands; in all four modes. The kernels must stay within their bounds on every slot and
//! region, and the fast tier must answer all but at most 1 in 10^4 random calls.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::arithmetic_side_effects, clippy::float_cmp, missing_docs)]

use liella_math::status::{Rounding, Status};
use liella_math::{fast_bessel as fb, special, stats};
use liella_mathtools::cmp;
use liella_mathtools::mp::{self, Approx, Big, Fmt, Round};

const MODES: [(Rounding, Round); 4] =
    [(Rounding::Nearest, Round::Nearest), (Rounding::Up, Round::Up), (Rounding::Down, Round::Down), (Rounding::Zero, Round::Zero)];

type Fast = fn(f64, Rounding) -> Option<(f64, Status)>;
type Entry = fn(f64, Rounding) -> (f64, Status);
type Case = (&'static str, Fast, Entry, Entry);

const FUNCS: [Case; 4] = [
    ("j0", fb::j0, special::j0::<f64>, special::j0_accurate::<f64>),
    ("j1", fb::j1, special::j1::<f64>, special::j1_accurate::<f64>),
    ("y0", fb::y0, special::y0::<f64>, special::y0_accurate::<f64>),
    ("y1", fb::y1, special::y1::<f64>, special::y1_accurate::<f64>),
];

/// Zeros of `J0`, `J1`, `Y0`, `Y1` below 32 (to 16 digits; the samples are the doubles around them).
const ZEROS: [&[f64]; 4] = [
    &[2.404825557695773, 5.520078110286311, 8.653727912911013, 11.79153443901428, 30.63460646843198],
    &[3.831705970207512, 7.015586669815619, 10.17346813506272, 13.32369193631422, 29.04682853491686],
    &[0.8935769662791675, 3.957678419314858, 7.086051060301773, 10.22234504349642, 30.66967011810648],
    &[2.197141326031017, 5.429681040794135, 8.596005868331169, 11.74915483402736, 29.09564964201445],
];

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
    fn log(&mut self, lo: f64, hi: f64) -> f64 {
        f64::from_bits(lo.to_bits() + self.next() % (hi.to_bits() - lo.to_bits()))
    }
    /// Half the bench's range (`j` on `[-20, 20]`, `y` on `[0.01, 20]`), half log-uniform up to 2^1000.
    fn operand(&mut self, name: &str) -> f64 {
        let x = match self.next() % 4 {
            0 | 1 => self.uniform(0.01, 20.0),
            2 => self.log(fb::TINY, 64.0),
            _ => self.log(fb::TINY, 2f64.powi(1000)),
        };
        if name.starts_with('j') && self.next() % 2 == 0 { -x } else { x }
    }
}

fn oracle(name: &str, x: f64, mode: Round) -> u64 {
    let args = [Big::from_bits(Fmt::F64, u128::from(x.to_bits()))];
    let (bits, _) = mp::correctly_rounded(Fmt::F64, mode, 1 << 15, |p| cmp::oracle(name, &args, p).unwrap_or(Approx { value: Big::nan(), exact: true }))
        .unwrap_or_else(|_| panic!("oracle undetermined for {name}({x:e})"));
    bits as u64
}

fn check(name: &str, fast: Fast, entry: Entry, x: f64) -> u32 {
    let mut settled = 0;
    for (mode, round) in MODES {
        let want = oracle(name, x, round);
        if let Some((v, _)) = fast(x, mode) {
            assert_eq!(v.to_bits(), want, "{name}({x:e}) fast tier in {mode:?}: {v:e}, oracle {:e}", f64::from_bits(want));
            settled += 1;
        }
        let v = entry(x, mode).0;
        assert_eq!(v.to_bits(), want, "{name}({x:e}) entry in {mode:?}: {v:e}, oracle {:e}", f64::from_bits(want));
    }
    settled
}

/// Slot ends, region switches, the doubles next to the zeros, huge operands.
fn hard_operands(f: usize) -> Vec<f64> {
    let mut v = Vec::new();
    let mut edge = |x: f64| {
        let b = x.to_bits();
        v.extend([x, f64::from_bits(b - 1), f64::from_bits(b + 1)]);
    };
    for e in -5..=4 {
        for j in 0..16 {
            edge(2f64.powi(e) * (1.0 + f64::from(j) / 16.0));
        }
    }
    for x in [2f64.powi(-27), 2f64.powi(-1000), 32.0, 2f64.powi(30), 1e300] {
        edge(x);
    }
    for z in ZEROS[f] {
        let b = z.to_bits();
        v.extend((0..40).map(|d| f64::from_bits(b - 20 + d)));
        v.extend([z * (1.0 + 1e-9), z * (1.0 - 1e-6)]);
    }
    let mut rng = Rng(0x6a09_e667_f3bc_c908 + f as u64);
    v.extend((0..40).map(|_| rng.log(32.0, f64::MAX)));
    v
}

#[test]
fn double_bessel_fast_tier_rounds_correctly_and_rarely_falls_back() {
    // Clauses: s10_3_c_0 (D-10.13).
    for (name, fast, entry, _) in FUNCS {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ name.as_bytes()[1] as u64);
        let before = stats::counts(name);
        for n in 0..100_000u32 {
            let mode = if n % 2 == 0 { Rounding::Nearest } else { Rounding::Up };
            core::hint::black_box(entry(rng.operand(name), mode));
        }
        let c = stats::counts(name).since(before);
        eprintln!("{name}: fast {} fallback {} of 100000", c.fast, c.fallback);
        assert!(c.fast + c.fallback >= 99_000, "{name}: the fast tier was not consulted ({c:?})");
        c.check_fallback(name, 10_000).unwrap_or_else(|e| panic!("{name}: fallback rate {e:?}"));
    }
    for (f, (name, fast, entry, _)) in FUNCS.into_iter().enumerate() {
        let mut rng = Rng(0xbb67_ae85_84ca_a73b + f as u64);
        let (mut settled, mut tried) = (0u32, 0u32);
        for _ in 0..300 {
            settled += check(name, fast, entry, rng.operand(name));
            tried += 4;
        }
        assert!(settled * 100 >= tried * 99, "{name}: the fast tier settled only {settled} of {tried} random cases");
        let hard: u32 = hard_operands(f).into_iter().map(|x| check(name, fast, entry, x)).sum();
        assert!(hard > 900, "{name}: the fast tier settled only {hard} hard cases");
    }
}

#[test]
fn double_bessel_fast_tier_matches_the_accurate_path() {
    // Clauses: s10_3_c_0 (D-10.13).
    // Where the fast tier settles, result and status are the 384-bit path's in every mode
    // (`LIELLA_BESSEL_SWEEP` operands per function and mode, default 1500).
    let n: u32 = std::env::var("LIELLA_BESSEL_SWEEP").ok().and_then(|s| s.parse().ok()).unwrap_or(1500);
    for (name, fast, _, accurate) in FUNCS {
        let mut rng = Rng(0x3c6e_f372_fe94_f82b ^ name.as_bytes()[1] as u64);
        let mut settled = 0u64;
        for _ in 0..n {
            let x = rng.operand(name);
            for (mode, _) in MODES {
                if let Some((v, st)) = fast(x, mode) {
                    let (w, sw) = accurate(x, mode);
                    assert_eq!(v.to_bits(), w.to_bits(), "{name}({x:e}) in {mode:?}: fast {v:e}, accurate {w:e}");
                    assert_eq!(st, sw, "{name}({x:e}) in {mode:?}: fast status {st:?}, accurate {sw:?}");
                    settled += 1;
                }
            }
        }
        assert!(settled * 100 >= u64::from(n) * 4 * 99, "{name}: settled only {settled}");
    }
}

#[test]
fn double_bessel_kernels_stay_within_their_bounds() {
    // Clauses: s10_3_c_0 (D-10.13).
    // Each kernel's error against the oracle (240 bits) stays within its `eps` on 8 operands of
    // every slot (both ends included), on the series range and on Hankel's range up to 2^1000.
    let mut rng = Rng(0xa54f_f53a_5f1d_36f1);
    let mut ops: Vec<f64> = Vec::new();
    for i in 0..160 {
        let s = 2f64.powi(-5 + (i / 16) as i32);
        let j = (i % 16) as f64;
        let (a, b) = (s * (1.0 + j / 16.0), s * (1.0 + (j + 1.0) / 16.0));
        ops.extend([a, f64::from_bits(b.to_bits() - 1)]);
        ops.extend((0..6).map(|_| rng.uniform(a, b)));
    }
    ops.extend((0..200).map(|_| rng.log(fb::TINY, fb::SMALL)));
    ops.extend((0..200).map(|_| rng.log(fb::HANKEL, 2f64.powi(1000))));
    ops.extend((0..100).map(|_| rng.uniform(fb::HANKEL, 40.0)));
    let names = ["j0", "j1", "y0", "y1"];
    let mut worst = [0f64; 4];
    for x in ops {
        for (f, name) in names.iter().enumerate() {
            let (y, eps) = fb::kernel(f, x).unwrap_or_else(|| panic!("{name} kernel({x:e}) declined"));
            let exact = cmp::oracle(name, &[Big::from_f64(x)], 240).unwrap().value;
            let r = exact.sub(&Big::from_f64(y.hi).add(&Big::from_f64(y.lo))).abs().to_f64() / eps;
            assert!(r <= 1.0, "{name} kernel({x:e}): error {r} times its bound");
            worst[f] = worst[f].max(r);
        }
    }
    eprintln!("worst error over bound: {worst:.3e?}");
}
