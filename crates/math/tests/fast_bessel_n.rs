//! The recurrence tier for `jn`, `yn`, `jnf`, `ynf` (`liella_math::fast_bessel::n`, D-10.76)
//! against the 384-bit path it falls back to and the test oracle (`liella-mathtools::mp`), spec
//! §10.3 and D-10.13 (every result correctly rounded in the caller's rounding direction). The
//! operands cover each regime of the tier (series, forward, Miller's backward recurrence for `jn`;
//! the forward recurrence for `yn` below and above the turning point `x = n`), orders up to 300
//! with both signs, negative `x` for `jn`, and the doubles next to zeros of `J_n` and `Y_n`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::cast_possible_wrap, clippy::arithmetic_side_effects, clippy::float_cmp, missing_docs)]

use liella_math::dd::Dd;
use liella_math::fast_bessel::n as fbn;
use liella_math::special;
use liella_math::status::{Rounding, Status};
use liella_mathtools::mp::{self, Big};

const MODES: [Rounding; 4] = [Rounding::Nearest, Rounding::Up, Rounding::Down, Rounding::Zero];

type Fast = fn(i32, f64, Rounding) -> Option<(u64, Status)>;
type Acc = fn(i32, f64, Rounding) -> (u64, Status);

/// Name, whether `float`, the tier, the 384-bit path (results as `f64` bits; a `float` operand
/// is an `f64` holding an `f32` value).
const FUNCS: [(&str, bool, Fast, Acc); 4] = [
    ("jn", false, |n, x, m| fbn::jn(n, x, m).map(|(v, s)| (v.to_bits(), s)), |n, x, m| {
        let (v, s) = special::jn_accurate::<f64>(n, x, m);
        (v.to_bits(), s)
    }),
    ("yn", false, |n, x, m| fbn::yn(n, x, m).map(|(v, s)| (v.to_bits(), s)), |n, x, m| {
        let (v, s) = special::yn_accurate::<f64>(n, x, m);
        (v.to_bits(), s)
    }),
    ("jnf", true, |n, x, m| fbn::jnf(n, x as f32, m).map(|(v, s)| (u64::from(v.to_bits()), s)), |n, x, m| {
        let (v, s) = special::jn_accurate::<f32>(n, x as f32, m);
        (u64::from(v.to_bits()), s)
    }),
    ("ynf", true, |n, x, m| fbn::ynf(n, x as f32, m).map(|(v, s)| (u64::from(v.to_bits()), s)), |n, x, m| {
        let (v, s) = special::yn_accurate::<f32>(n, x as f32, m);
        (u64::from(v.to_bits()), s)
    }),
];

const REGIMES: [&str; 5] = ["j series", "j forward", "j miller", "y x<n", "y x>=n"];

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 * f64::from_bits(0x3ca0_0000_0000_0000)
    }
    fn log(&mut self, lo: f64, hi: f64) -> f64 {
        f64::from_bits(lo.to_bits() + self.next() % (hi.to_bits() - lo.to_bits()))
    }
    /// An order in `2..=300`, small ones more often.
    fn order(&mut self) -> u64 {
        if self.next() % 2 == 0 { 2 + self.next() % 20 } else { 2 + self.next() % 299 }
    }
    /// `(n, x, regime)` for `jn` (`y == false`) or `yn`, with random signs where defined.
    fn operand(&mut self, y: bool, float: bool) -> (i32, f64, usize) {
        let n = self.order();
        let nf = n as f64;
        // Beyond 2^100 the tier's modulus bound has no a priori cap and it declines by design.
        let top = 2f64.powi(100);
        let low = if float { 2f64.powi(-120) } else { 2f64.powi(-980) };
        let mut x = if y {
            match self.next() % 4 {
                0 => self.unit() * nf,
                1 => self.log(low.max(1e-3), nf),
                2 => nf + self.unit() * 3.0 * nf,
                _ => self.log(nf, top),
            }
        } else {
            match self.next() % 4 {
                0 => self.log(low, (nf + 1.0).sqrt()),
                1 => (nf + 1.0).sqrt() + self.unit() * (nf - (nf + 1.0).sqrt()),
                2 => nf + self.unit() * 2.0 * nf,
                _ => self.log(nf, top),
            }
        };
        if float {
            x = f64::from(x as f32);
        }
        let x = x.max(low);
        let regime = if y {
            if x < nf { 3 } else { 4 }
        } else if x * x <= nf + 1.0 {
            0
        } else if nf <= x {
            1
        } else {
            2
        };
        let n = if self.next() % 4 == 0 { -(n as i32) } else { n as i32 };
        let x = if !y && self.next() % 3 == 0 { -x } else { x };
        (n, x, regime)
    }
}

/// The tier against the 384-bit path at `(n, x)` in every mode: result bits and status where
/// the tier settles. Returns the number settled.
fn check(name: &str, fast: Fast, acc: Acc, n: i32, x: f64) -> u32 {
    let mut settled = 0;
    for mode in MODES {
        if let Some((v, st)) = fast(n, x, mode) {
            let (w, sw) = acc(n, x, mode);
            assert_eq!(v, w, "{name}({n}, {x:e}) in {mode:?}: tier {v:#x}, accurate {w:#x}");
            assert_eq!(st, sw, "{name}({n}, {x:e}) in {mode:?}: tier status {st:?}, accurate {sw:?}");
            settled += 1;
        }
    }
    settled
}

/// Whether the 384-bit result in round-to-nearest is one the tier is meant to settle: finite,
/// and for `float` normal (`near32` leaves subnormal and overflowing results to the 384-bit path).
fn eligible(acc: Acc, float: bool, n: i32, x: f64) -> bool {
    let (w, _) = acc(n, x, Rounding::Nearest);
    if float {
        let v = f32::from_bits(w as u32);
        v.is_normal()
    } else {
        f64::from_bits(w).is_finite()
    }
}

#[test]
fn bessel_n_tier_matches_the_accurate_path_and_rarely_falls_back() {
    // Clauses: s10_3_c_0 (D-10.13, D-10.76).
    // 600 operands per function over the tier's regimes, every mode; then the share settled in
    // round-to-nearest among the results the tier is meant to give, per regime.
    let mut report = Vec::new();
    for (f, (name, float, fast, acc)) in FUNCS.into_iter().enumerate() {
        let y = name.starts_with('y');
        let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ ((f as u64 + 1) * 0x1234_5678));
        let mut tried = [0u32; 5];
        let mut got = [0u32; 5];
        for _ in 0..600 {
            let (n, x, regime) = rng.operand(y, float);
            check(name, fast, acc, n, x);
            if eligible(acc, float, n, x) {
                tried[regime] += 1;
                if fast(n, x, Rounding::Nearest).is_some() {
                    got[regime] += 1;
                } else {
                    eprintln!("{name}: fallback at n = {n}, x = {x:e} ({regime})");
                }
            }
        }
        for r in 0..5 {
            if tried[r] > 0 {
                eprintln!("{name} {}: settled {} of {}", REGIMES[r], got[r], tried[r]);
                report.push((name, r, got[r], tried[r]));
            }
        }
    }
    for (name, r, got, tried) in report {
        assert!(got * 100 >= tried * 95, "{name} {}: the tier settled only {got} of {tried}", REGIMES[r]);
    }
}

/// The doubles `x` in `(lo, hi)` around the zero where `sign(acc(n, x))` changes (bisection on
/// the bits; `lo`, `hi` positive with opposite signs).
fn zero_between(acc: Acc, float: bool, n: i32, mut lo: f64, mut hi: f64) -> f64 {
    let sign = |x: f64| {
        let w = acc(n, x, Rounding::Nearest).0;
        if float { (w >> 31) & 1 } else { w >> 63 }
    };
    let s = sign(lo);
    for _ in 0..80 {
        let mid = if float {
            f64::from(f32::from_bits(((lo as f32).to_bits() + (hi as f32).to_bits()) / 2))
        } else {
            f64::from_bits((lo.to_bits() + hi.to_bits()) / 2)
        };
        if mid == lo || mid == hi {
            break;
        }
        if sign(mid) == s { lo = mid } else { hi = mid }
    }
    lo
}

#[test]
fn bessel_n_tier_rounds_correctly_next_to_zeros() {
    // Clauses: s10_3_c_0 (D-10.13, D-10.76).
    // The first two positive zeros of J_n and Y_n for n in {2, 3, 7, 20, 60} (found by bisection
    // on the 384-bit path's sign) and the 24 representable operands around each, every mode:
    // where the rounding test must decline or the bound is tight.
    for (name, float, fast, acc) in FUNCS {
        let mut settled = 0;
        for n in [2i32, 3, 7, 20, 60] {
            let mut x = f64::from(n) + 0.5;
            let mut zeros = 0;
            let mut prev = acc(n, x, Rounding::Nearest).0;
            while zeros < 2 {
                let nx = x + 0.25;
                let cur = acc(n, nx, Rounding::Nearest).0;
                let flip = if float { ((prev ^ cur) >> 31) & 1 } else { (prev ^ cur) >> 63 };
                if flip == 1 {
                    let z = zero_between(acc, float, n, x, nx);
                    for d in -12i64..12 {
                        let v = if float {
                            f64::from(f32::from_bits(((z as f32).to_bits() as i64 + d) as u32))
                        } else {
                            f64::from_bits((z.to_bits() as i64 + d) as u64)
                        };
                        settled += check(name, fast, acc, n, v);
                        settled += check(name, fast, acc, -n, v);
                    }
                    zeros += 1;
                }
                x = nx;
                prev = cur;
            }
        }
        eprintln!("{name}: settled {settled} near zeros");
    }
}

#[test]
fn bessel_n_kernels_stay_within_their_bounds() {
    // Clauses: s10_3_c_0 (D-10.13, D-10.76).
    // `jn_kernel`'s `(y ± eps)·2^e` and `yn_kernel`'s `y ± eps` hold the value the oracle gives at
    // 240 bits, in every regime (series, forward, Miller, Y below and above the turning point).
    let mut rng = Rng(0xa54f_f53a_5f1d_36f1);
    let mut worst = [0f64; 2];
    for i in 0..400 {
        let y = i % 2 == 1;
        let (n, x, _) = rng.operand(y, false);
        let (n, x) = (u64::from(n.unsigned_abs()), x.abs().min(1e6));
        let big = Big::from_f64(x);
        let (dd, eps, e, exact) = if y {
            let Some((v, eps)) = fbn::yn_kernel(n, x) else { continue };
            (v, eps, 0, mp::bessel_y(n as i64, &big, 240).value)
        } else {
            let Some((v, eps, e)) = fbn::jn_kernel(n, x) else { continue };
            (v, eps, e, mp::bessel_j(n as i64, &big, 240).value)
        };
        let got = Big::from_f64(dd.hi).add(&Big::from_f64(dd.lo));
        let r = exact.mul_pow2(-i64::from(e)).sub(&got).abs().to_f64() / eps;
        let name = if y { "yn" } else { "jn" };
        assert!(r <= 1.0, "{name}_kernel({n}, {x:e}): error {r} times its bound");
        worst[usize::from(y)] = worst[usize::from(y)].max(r);
    }
    eprintln!("worst error over bound (jn, yn): {worst:?}");
}

#[test]
fn bessel_n_rounding_tests_decline_straddles() {
    // Clauses: s10_3_c_0 (D-10.13, D-10.76).
    // A value within its bound of a rounding boundary has no settled rounding: the tests must
    // decline, and settle the same value with a bound that leaves the boundary outside.
    let p = |k: i32| 2f64.powi(k);
    // Just above the midpoint between 1 and 1 + 2^-52 (nearest), and just above 1 (directed).
    let mid = Dd { hi: 1.0, lo: p(-53) + p(-80) };
    let above = Dd { hi: 1.0, lo: p(-80) };
    for (y, mode) in [(mid, Rounding::Nearest), (above, Rounding::Up), (above, Rounding::Down), (above, Rounding::Zero)] {
        assert!(fbn::near64(y, p(-70), 0, mode).is_none(), "near64 settled a straddle in {mode:?}");
        assert!(fbn::near64(y.neg(), p(-70), 3, mode).is_none(), "near64 settled a negative straddle in {mode:?}");
        assert!(fbn::near64(y, p(-90), 0, mode).is_some(), "near64 declined a clear case in {mode:?}");
    }
    // `near32` rounds `y.hi`: 2^-48 (16 units of its last place) beyond the boundary.
    let mid32 = Dd { hi: 1.0 + p(-24) + p(-48), lo: 0.0 };
    let above32 = Dd { hi: 1.0 + p(-48), lo: 0.0 };
    for (y, mode) in [(mid32, Rounding::Nearest), (above32, Rounding::Up), (above32, Rounding::Down)] {
        assert!(fbn::near32(y, p(-45), 0, mode).is_none(), "near32 settled a straddle in {mode:?}");
        assert!(fbn::near32(y, p(-60), 0, mode).is_some(), "near32 declined a clear case in {mode:?}");
    }
}

#[test]
#[ignore = "10^7 operands per function: run by the accuracy job (LIELLA_BESSEL_N_SWEEP)"]
fn bessel_n_tier_sweep() {
    // Clauses: s10_3_c_0 (D-10.13, D-10.76).
    // `LIELLA_BESSEL_N_SWEEP` operands (default 10^7) per function over the regimes, every mode,
    // the tier against the 384-bit path, on every core; `LIELLA_BESSEL_N_FUNCS` picks functions.
    let total: u64 = std::env::var("LIELLA_BESSEL_N_SWEEP").ok().and_then(|s| s.parse().ok()).unwrap_or(10_000_000);
    let pick = std::env::var("LIELLA_BESSEL_N_FUNCS").unwrap_or_default();
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get() as u64);
    for (f, (name, float, fast, acc)) in FUNCS.into_iter().enumerate() {
        if !pick.is_empty() && !pick.split(',').any(|p| p == name) {
            continue;
        }
        let y = name.starts_with('y');
        let counts: Vec<(u64, [u64; 5])> = std::thread::scope(|s| {
            let hs: Vec<_> = (0..threads)
                .map(|t| {
                    s.spawn(move || {
                        let mut rng = Rng(0x510e_527f_ade6_82d1 ^ ((f as u64) << 40) ^ ((t + 1) * 0x9e37_79b9));
                        let (mut settled, mut fallback) = (0u64, [0u64; 5]);
                        for _ in 0..(total / threads + 1) {
                            let (n, x, regime) = rng.operand(y, float);
                            let k = u64::from(check(name, fast, acc, n, x));
                            settled += k;
                            fallback[regime] += 4 - k;
                        }
                        (settled, fallback)
                    })
                })
                .collect();
            hs.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let settled: u64 = counts.iter().map(|c| c.0).sum();
        let fb: Vec<u64> = (0..5).map(|r| counts.iter().map(|c| c.1[r]).sum()).collect();
        eprintln!("{name}: {} operands x 4 modes, settled {settled} (all equal the 384-bit path), declined per regime {fb:?}", (total / threads + 1) * threads);
    }
}
