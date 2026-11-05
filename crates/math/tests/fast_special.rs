//! The `double` `erf`/`erfc` fast tier (`liella_math::fast_special`) against the test oracle
//! (`liella-mathtools::mp`) and against the 384-bit path it falls back to, spec §10.3 and D-10.13
//! (every result correctly rounded in the caller's rounding direction). The fast tier either
//! returns the correctly rounded result or `None`, in all four modes: on random operands, on
//! every slot boundary and range switch, on subnormal operands and results. Its kernels must
//! stay within their stated bounds (`eps`) on every table slot; the public entries
//! (`special::erf::<f64>`, `special::erfc::<f64>`) must give the oracle's result; the fast tier
//! must answer all but at most 1 in 10^4 random calls (the bounds predict at most about
//! 2^-15.6, module doc of `fast_special`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::arithmetic_side_effects, clippy::float_cmp, missing_docs)]

use liella_math::status::{Rounding, Status};
use liella_math::{fast_special, special, stats};
use liella_mathtools::cmp;
use liella_mathtools::mp::{self, Approx, Big, Fmt, Round};

const MODES: [(Rounding, Round); 4] =
    [(Rounding::Nearest, Round::Nearest), (Rounding::Up, Round::Up), (Rounding::Down, Round::Down), (Rounding::Zero, Round::Zero)];

type Fast = fn(f64, Rounding) -> Option<(f64, Status)>;
type Entry = fn(f64, Rounding) -> (f64, Status);

/// (name, fast tier, public entry, accurate path).
type Case = (&'static str, Fast, Entry, Entry);

const FUNCS: [Case; 2] = [
    ("erf", fast_special::erf, special::erf::<f64>, special::erf_accurate::<f64>),
    ("erfc", fast_special::erfc, special::erfc::<f64>, special::erfc_accurate::<f64>),
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
    /// Uniform in `[lo, hi)`.
    fn uniform(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.unit()
    }
    /// Uniform in the encodings of `[lo, hi)` (`0 < lo < hi`): every binade alike.
    fn log(&mut self, lo: f64, hi: f64) -> f64 {
        f64::from_bits(lo.to_bits() + self.next() % (hi.to_bits() - lo.to_bits()))
    }
    /// A random operand of the fast tier's domain: half uniform, half log-uniform with a random
    /// sign (`erfc`: mostly positive, where its own kernel runs).
    fn operand(&mut self, name: &str) -> f64 {
        let r = self.next();
        match (name, r % 4) {
            ("erf", 0 | 1) => self.uniform(-7.0, 7.0),
            ("erf", 2) => self.log(f64::from_bits(1), 7.0),
            ("erf", _) => -self.log(f64::from_bits(1), 7.0),
            (_, 0) => self.uniform(-7.0, fast_special::ERFC_MAX),
            (_, 1) => self.uniform(1.0, fast_special::ERFC_MAX),
            (_, 2) => self.log(f64::from_bits(0x39b0_0000_0000_0000), fast_special::ERFC_MAX),
            _ => -self.log(f64::from_bits(0x39b0_0000_0000_0000), 7.0),
        }
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

/// The fast tier and the public entry both give the oracle's result for `x` in every mode.
/// Returns how many modes the fast tier settled.
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

/// Every table slot's two ends and its neighbours, the range switches, subnormal operands
/// (`erf`) and subnormal results (`erfc`), with both signs.
fn hard_operands(name: &str) -> Vec<f64> {
    let mut v = Vec::new();
    let mut edge = |x: f64| {
        let b = x.to_bits();
        v.extend([x, f64::from_bits(b - 1), f64::from_bits(b + 1), -x, -f64::from_bits(b - 1)]);
    };
    // ERF_T's and ERFCX_T's slot boundaries: 2^e·(1 + j/16).
    for e in -5..=4 {
        for j in 0..16 {
            edge(2f64.powi(e) * (1.0 + f64::from(j) / 16.0));
        }
    }
    for x in [2f64.powi(-55), 2f64.powi(-100), 2f64.powi(-900), 6.0, 27.0] {
        edge(x);
    }
    let mut rng = Rng(0x6a09_e667_f3bc_c908 ^ name.len() as u64);
    if name == "erf" {
        v.extend([f64::from_bits(1), f64::from_bits(3), 1.5 * 2f64.powi(-1023), 1.25 * 2f64.powi(-1022), 2f64.powi(-1030)]);
        v.extend((0..60).map(|_| rng.log(f64::from_bits(1), 2f64.powi(-1000))));
        // Subnormal operands (subnormal results), and the binade just below 2^-1022.
        v.extend((0..60).map(|_| f64::from_bits(1 + rng.next() % (1 << 52))));
        v.extend((0..40).map(|_| rng.log(2f64.powi(-1023), 2f64.powi(-1022))));
    } else {
        // erfc x in [2^-1077, 2^-1022): x from about 26.5 to the fast range's end.
        v.extend((0..200).map(|_| rng.uniform(26.45, fast_special::ERFC_MAX)));
    }
    v.retain(|x| x.is_finite() && *x != 0.0 && (name == "erf" || (x.abs() >= 2f64.powi(-100) && *x < fast_special::ERFC_MAX)));
    v
}

#[test]
fn double_erf_erfc_fast_tier_rounds_correctly_and_rarely_falls_back() {
    // Clauses: s10_3_c_0 (D-10.13).
    // The fallback rate first and alone (the counters are global and no other test of this
    // binary calls the entries): 10^5 random operands per function, in round-to-nearest and one
    // directed mode.
    for (name, fast, entry, _) in FUNCS {
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ name.len() as u64);
        let before = stats::counts(name);
        for n in 0..100_000u32 {
            let x = rng.operand(name);
            let mode = if n % 2 == 0 { Rounding::Nearest } else { Rounding::Up };
            core::hint::black_box(entry(x, mode));
            if fast(x, mode).is_none() {
                eprintln!("{name}: fallback at {x:e} ({:#018x}) in {mode:?}", x.to_bits());
            }
        }
        let c = stats::counts(name).since(before);
        eprintln!("{name}: fast {} fallback {} of 100000", c.fast, c.fallback);
        assert!(c.fast + c.fallback >= 99_000, "{name}: the fast tier was not consulted ({c:?})");
        c.check_fallback(name, 10_000).unwrap_or_else(|e| panic!("{name}: fallback rate {e:?}"));
    }
    // erfc x for x in [26.545, 26.575] lies in [2^-1023, 2^-1022), where the subnormal grid's
    // scaled significand reaches 2^51: the rounding test settles these like any other (at most
    // 1 in 500 refused; a test that refused z.hi >= 2^51 or ties of z.hi refused about 1 in 4).
    let mut rng = Rng(0x510e_527f_ade6_82d1);
    let refused = (0..2000).filter(|_| fast_special::erfc(rng.uniform(26.545, 26.575), Rounding::Nearest).is_none()).count();
    assert!(refused <= 4, "erfc near 2^-1022: {refused} of 2000 refused");
    // Random operands, then the hard ones, against the oracle in all four modes.
    for (name, fast, entry, _) in FUNCS {
        let mut rng = Rng(0xbb67_ae85_84ca_a73b ^ name.len() as u64);
        let (mut settled, mut tried) = (0u32, 0u32);
        for _ in 0..400 {
            settled += check(name, fast, entry, rng.operand(name));
            tried += 4;
        }
        assert!(settled * 100 >= tried * 99, "{name}: the fast tier settled only {settled} of {tried} random cases");
        let (mut sub_settled, mut hard_settled) = (0u32, 0u32);
        for x in hard_operands(name) {
            let s = check(name, fast, entry, x);
            hard_settled += s;
            if entry(x, Rounding::Nearest).0.abs() < f64::MIN_POSITIVE {
                sub_settled += s;
            }
        }
        assert!(hard_settled > 1500, "{name}: the fast tier settled only {hard_settled} hard cases");
        assert!(sub_settled > 100,"{name}: the fast tier settled only {sub_settled} subnormal cases");
    }
}

#[test]
fn double_erf_erfc_fast_tier_matches_the_accurate_path() {
    // Clauses: s10_3_c_0 (D-10.13).
    // Where the fast tier settles, its result and status (flags and errno) are the 384-bit
    // path's, in every mode. `LIELLA_ERF_SWEEP` operands per function and mode (default 2000;
    // the accuracy job runs 10^7).
    let n: u32 = std::env::var("LIELLA_ERF_SWEEP").ok().and_then(|s| s.parse().ok()).unwrap_or(2000);
    for (name, fast, _, accurate) in FUNCS {
        let mut rng = Rng(0x3c6e_f372_fe94_f82b ^ name.len() as u64);
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
        eprintln!("{name}: {settled} of {} settled and identical", u64::from(n) * 4);
        assert!(settled * 100 >= u64::from(n) * 4 * 99, "{name}: settled only {settled}");
    }
}

/// `|f(x) − y·2^m| / (eps·2^m)` against the oracle at 240 bits.
fn ratio(name: &str, x: f64, y: liella_math::dd::Dd, eps: f64, m: i32) -> f64 {
    let exact = cmp::oracle(name, &[Big::from_f64(x)], 240).unwrap().value;
    let got = Big::from_f64(y.hi).add(&Big::from_f64(y.lo)).mul_pow2(i64::from(m));
    exact.sub(&got).abs().mul_pow2(-i64::from(m)).to_f64() / eps
}

#[test]
fn double_erf_erfc_kernels_stay_within_their_bounds() {
    // Clauses: s10_3_c_0 (D-10.13).
    // Each kernel's error against the oracle stays within its `eps` on 12 operands of every
    // table slot (its two ends included), on the series and on the linear range: a wrong
    // coefficient, slot or range switch shows here whatever the rounding test would do.
    let mut rng = Rng(0xa54f_f53a_5f1d_36f1);
    let mut worst = [0f64; 2];
    let slots = |e0: i32, n: usize| {
        (0..n).map(move |i| {
            let s = 2f64.powi(e0 + (i / 16) as i32);
            let j = (i % 16) as f64;
            (s * (1.0 + j / 16.0), s * (1.0 + (j + 1.0) / 16.0))
        })
    };
    let mut erf_ops: Vec<f64> = Vec::new();
    for (a, b) in slots(-5, 120) {
        erf_ops.extend([a, f64::from_bits(b.to_bits() - 1)]);
        erf_ops.extend((0..10).map(|_| rng.uniform(a, b)));
    }
    erf_ops.extend((0..400).map(|_| rng.log(2f64.powi(-60), 2f64.powi(-5))));
    erf_ops.extend((0..50).map(|_| rng.log(f64::from_bits(1), 2f64.powi(-900))));
    for x in erf_ops {
        let (y, eps, m) = fast_special::erf_kernel(x);
        let r = ratio("erf", x, y, eps, m);
        assert!(r <= 1.0, "erf_kernel({x:e}): error {r} times its bound");
        worst[0] = worst[0].max(r);
    }
    let mut erfc_ops: Vec<f64> = Vec::new();
    for (a, b) in slots(0, 76) {
        let b = b.min(fast_special::ERFC_MAX);
        if a < b {
            erfc_ops.extend([a, f64::from_bits(b.to_bits() - 1)]);
            erfc_ops.extend((0..10).map(|_| rng.uniform(a, b)));
        }
    }
    for x in erfc_ops {
        let Some((y, eps, m)) = fast_special::erfc_kernel(x) else { panic!("erfc_kernel({x:e}) declined") };
        assert!((1.0..2.0).contains(&y.hi), "erfc_kernel({x:e}): y.hi {} not normalized", y.hi);
        let r = ratio("erfc", x, y, eps, m);
        assert!(r <= 1.0, "erfc_kernel({x:e}): error {r} times its bound");
        worst[1] = worst[1].max(r);
    }
    eprintln!("worst error over bound: erf {:.3e}, erfc {:.3e}", worst[0], worst[1]);
}
