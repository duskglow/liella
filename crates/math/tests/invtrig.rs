//! The `double` `atan`/`asin`/`acos`/`atan2`/`hypot` fast tier (`liella_math::invtrig`,
//! LIBM-F64-INVTRIG-HYPOT-TIER) against the accurate path, spec §10.3 and D-10.13 (every result
//! correctly rounded): where a kernel answers it gives the accurate path's round-to-nearest
//! result bit for bit; hard cases (results next to a rounding boundary, exact roots, the table's
//! nodes, the window edges) are refused or settled right; random operands rarely fall back. The
//! exported entries (fast tier plus fallback) give the accurate path's bits in both builds of
//! the kernels (Dekker and FMA, D-10.25, switched by `selftest::set_math_fma`).
#![no_builtins] // calls to libc names (atan, hypot, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_possible_wrap, clippy::cast_sign_loss, clippy::cast_precision_loss, clippy::arithmetic_side_effects, clippy::float_arithmetic, missing_docs)]

use liella_boundary::posix::s10_math_gen as m;
use liella_boundary::selftest;
use liella_math::status::Rounding;
use liella_math::{invtrig, pow, trig};

type K1 = fn(f64) -> Option<f64>;
type K2 = fn(f64, f64) -> Option<f64>;

/// xorshift64*: a fixed, reproducible operand stream.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }
    /// A random `double` of magnitude in `[2^lo, 2^hi)` (uniform exponent and significand, random sign).
    fn mag(&mut self, lo: i64, hi: i64) -> f64 {
        let r = self.next();
        let e = lo + (r >> 53) as i64 % (hi - lo);
        f64::from_bits((((1023 + e) as u64) << 52) | (self.next() >> 12) | (r & 1) << 63)
    }
}

fn accurate1(f: &str, x: f64) -> f64 {
    match f {
        "atan" => trig::atan::<f64>(x, Rounding::Nearest).0,
        "asin" => trig::asin::<f64>(x, Rounding::Nearest).0,
        _ => trig::acos::<f64>(x, Rounding::Nearest).0,
    }
}

fn accurate2(f: &str, a: f64, b: f64) -> f64 {
    if f == "atan2" { trig::atan2::<f64>(a, b, Rounding::Nearest).0 } else { pow::hypot_exact::<f64>(a, b, Rounding::Nearest).0 }
}

fn entry1(f: &str, x: f64) -> f64 {
    match f {
        "atan" => m::atan(x),
        "asin" => m::asin(x),
        _ => m::acos(x),
    }
}

fn entry2(f: &str, a: f64, b: f64) -> f64 {
    if f == "atan2" { m::atan2(a, b) } else { m::hypot(a, b) }
}

/// The kernel, where it answers, and the exported entry in the build in force give the accurate
/// path's bits. Returns whether the kernel answered.
fn check1(f: &str, k: K1, x: f64) -> bool {
    let want = accurate1(f, x);
    let got = k(x);
    if let Some(v) = got {
        assert_eq!(v.to_bits(), want.to_bits(), "{f}({x:e}) = {v:e} on the fast tier, accurate {want:e}: rounding test FAILED");
    }
    let e = entry1(f, x);
    assert_eq!(e.to_bits(), want.to_bits(), "{f}({x:e}) = {e:e} by the entry (fma {}), accurate {want:e}: entry FAILED", selftest::math_fma());
    got.is_some()
}

fn check2(f: &str, k: K2, a: f64, b: f64) -> bool {
    let want = accurate2(f, a, b);
    let got = k(a, b);
    if let Some(v) = got {
        assert_eq!(v.to_bits(), want.to_bits(), "{f}({a:e}, {b:e}) = {v:e} on the fast tier, accurate {want:e}: rounding test FAILED");
    }
    let e = entry2(f, a, b);
    assert_eq!(e.to_bits(), want.to_bits(), "{f}({a:e}, {b:e}) = {e:e} by the entry (fma {}), accurate {want:e}: entry FAILED", selftest::math_fma());
    got.is_some()
}

/// The builds to run: the baseline, and FMA when the CPU has it (printed when it does not).
fn builds() -> Vec<bool> {
    if selftest::set_math_fma(true) {
        vec![false, true]
    } else {
        println!("FMA build not tested: this CPU or OS has no FMA");
        vec![false]
    }
}

const N: usize = 100_000;

/// Operands whose result lies next to a midpoint between two `double`s: for tiny `x`,
/// `atan x = x − x^3/3 + ...` and `asin x = x + x^3/6 + ...`, so where `x^3/div` is `k + 1/2`
/// ulps of `x` the result sits within the next term (about `2^-52` ulp) of a midpoint. Around
/// each such crossing (exponents -27..=-24) the 129 nearest `double`s: the crossing moves by
/// `2^-52`..`2^-48` ulp per `double`, so the nearest ones lie within the kernel's own rounding
/// error of the midpoint, where only the rounding test's margin decides.
fn near_midpoints(div: f64) -> Vec<f64> {
    let mut v = Vec::new();
    for e in -27..=-24i32 {
        let ulp = 2f64.powi(e - 52);
        for k in 0..64u32 {
            let x0 = (div * (f64::from(k) + 0.5) * ulp).cbrt();
            if ((x0.to_bits() >> 52) as i64) - 1023 == i64::from(e) {
                v.extend((0..129u64).map(|d| f64::from_bits(x0.to_bits() - 64 + d)));
            }
        }
    }
    v
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

/// Primitive Pythagorean triples `(a, b, c)` with `c` odd just below `2^53` and both legs above
/// `2^52`: `a^2 + b^2` takes 106 bits, so the kernel's double-double sum can be off by one unit
/// and the root's low word is then a tiny nonzero value instead of 0. Only the exact-root check's
/// floor keeps such a root (exact, so it must not raise inexact) from being answered.
fn big_triples() -> Vec<(f64, f64, f64)> {
    let mut v = Vec::new();
    for r in [2.0f64, 2.2, 2.414, 2.7] {
        let n0 = (2f64.powf(52.95) / (r * r + 1.0)).sqrt() as u64;
        for n in n0 - 300..n0 {
            let mut m = (n as f64 * r) as u64;
            if (m ^ n) & 1 == 0 {
                m += 1;
            }
            let c = m * m + n * n;
            if gcd(m, n) == 1 && c < 1 << 53 {
                v.push(((m * m - n * n) as f64, (2 * m * n) as f64, c as f64));
            }
        }
    }
    v
}

#[test]
fn double_invtrig_fast_tier_is_the_accurate_path_and_rarely_falls_back() {
    // Clauses: s10_3_c_0 (D-10.13).
    let one: [(&str, K1, (i64, i64)); 3] =
        [("atan", invtrig::atan_nearest, (-62, 62)), ("asin", |x| invtrig::asin_nearest(x, f64::sqrt), (-62, 0)), ("acos", |x| invtrig::acos_nearest(x, f64::sqrt), (-62, 0))];
    for fma in builds() {
        selftest::set_math_fma(fma);
        for (f, k, (lo, hi)) in one {
            let mut rng = Rng(0x9e37_79b9_7f4a_7c15 ^ f.len() as u64);
            let mut answered = 0usize;
            for i in 0..N {
                // Half the operands log-uniform, half in [1/2, 1) (where asin/acos results vary most).
                let x = if i % 2 == 0 { rng.mag(lo, hi) } else { rng.mag(-1, 0) };
                answered += usize::from(check1(f, k, x));
            }
            // Outside the window: about 2/62 of the log-uniform half; the rounding test refuses ~2^-8.
            assert!(answered * 100 > N * 96, "{f}: fast tier answered {answered} of {N}");
            // Hard cases: the table's nodes j/64, the window edges, results next to a boundary
            // (atan x ≈ x − x^3/3 near 2^-26, asin/acos just below 1, atan just below 2^60).
            let mut hard = vec![1.0, 0.5, f64::from_bits(0x3c30_0000_0000_0000), f64::from_bits(0x43af_ffff_ffff_ffff)];
            hard.extend((0..=64).map(|j| f64::from(j) / 64.0));
            for d in 0..2000u64 {
                hard.extend([0x3e50_0000_0000_0000 + d, 0x3e4f_ffff_ffff_ffff - d, 0x3fef_ffff_ffff_ffff - d, 0x43af_ffff_ffff_ffff - d].map(f64::from_bits));
            }
            match f {
                "atan" => hard.extend(near_midpoints(3.0)),
                "asin" => hard.extend(near_midpoints(6.0)),
                _ => {}
            }
            for &x in &hard {
                check1(f, k, x);
                check1(f, k, -x);
            }
        }
        // atan(1/2) and atan(1): every operation up to the table add is exact; they still answer
        // (and raise inexact: the boundary test).
        assert!(invtrig::atan_nearest(0.5).is_some() && invtrig::atan_nearest(1.0).is_some(), "atan(1/2), atan(1) refused");
    }
    selftest::set_math_fma(false);
}

#[test]
fn double_atan2_hypot_fast_tier_is_the_accurate_path_and_rarely_falls_back() {
    // Clauses: s10_3_c_0 (D-10.13).
    let two: [(&str, K2); 2] = [("atan2", invtrig::atan2_nearest), ("hypot", |a, b| invtrig::hypot_nearest(a, b, f64::sqrt))];
    for fma in builds() {
        selftest::set_math_fma(fma);
        for (f, k) in two {
            let mut rng = Rng(0x6a09_e667_f3bc_c908 ^ f.len() as u64);
            let mut answered = 0usize;
            for _ in 0..N {
                let a = rng.mag(-40, 40);
                let e = ((a.to_bits() >> 52) & 0x7ff) as i64 - 1023;
                let b = rng.mag(e - 20, e + 20); // within 2^21 of each other: inside both windows
                answered += usize::from(check2(f, k, a, b));
            }
            assert!(answered * 100 > N * 98, "{f}: fast tier answered {answered} of {N}");
            // Hard cases: equal magnitudes, exact roots (scaled Pythagorean triples), roots
            // next to a double (1 + y^2/2 with y^2 near ulp(1)/2), the window edges.
            let mut hard: Vec<(f64, f64)> = vec![(1.0, 1.0), (1.0, f64::from_bits(0x3e10_0000_0000_0000))];
            for (p, q) in [(3.0, 4.0), (5.0, 12.0), (8.0, 15.0), (20.0, 21.0), (119.0, 120.0), (4_400_489.0, 4_399_920.0)] {
                for s in [-300, -1, 0, 7, 300] {
                    hard.push((p * 2f64.powi(s), q * 2f64.powi(s)));
                }
            }
            for d in 0..2000u64 {
                hard.push((1.0, f64::from_bits(0x3e50_0000_0000_0000 + d)));
                hard.push((f64::from_bits(0x3ff0_0000_0000_0000 + d), f64::from_bits(0x3ff0_0000_0000_0000 + d)));
                hard.push((f64::from_bits(0x2700_0000_0000_0000 + d), f64::from_bits(0x2700_0000_0000_0000 + 3 * d)));
            }
            for &(a, b) in &hard {
                for (sa, sb) in [(1.0, 1.0), (-1.0, 1.0), (1.0, -1.0), (-1.0, -1.0)] {
                    check2(f, k, sa * a, sb * b);
                    check2(f, k, sb * b, sa * a);
                }
            }
        }
        for (a, b) in [(3.0, 4.0), (5.0, 12.0), (3.0 * 2f64.powi(-300), 4.0 * 2f64.powi(-300))] {
            assert_eq!(invtrig::hypot_nearest(a, b, f64::sqrt), None, "hypot({a:e}, {b:e}): an exact root must go to the accurate path (exact-root check FAILED)");
        }
        let triples = big_triples();
        assert!(triples.len() > 400, "only {} triples", triples.len());
        for (a, b, c) in triples {
            assert_eq!(invtrig::hypot_nearest(a, b, f64::sqrt), None, "hypot({a:e}, {b:e}) = {c:e} exactly: an exact root must go to the accurate path (exact-root check FAILED)");
            assert_eq!(m::hypot(a, b).to_bits(), c.to_bits(), "hypot({a:e}, {b:e}) by the entry (fma {fma}), exact root {c:e}: entry FAILED");
        }
        assert!(invtrig::hypot_nearest(1.0, f64::from_bits(0x3e10_0000_0000_0000), f64::sqrt).is_some(), "hypot(1, 2^-30) refused");
    }
    selftest::set_math_fma(false);
}

#[test]
fn double_atan_fast_tier_table_agrees_with_the_multiprecision_atan() {
    // Clauses: s10_3_c_0 (D-10.13). The accurate path reads the same `atan(j/64)` table as the
    // kernel, so agreement with it cannot catch a wrong entry: here each entry's neighbourhood
    // (operands within 1/128 of j/64) is checked against `mp::atan`, which uses no table,
    // rounded to nearest: the Dekker kernel directly, and the exported entry in each build.
    use liella_math::mp::{self, Mp};
    let mut rng = Rng(0x51_7cc1_b727_220a);
    for fma in builds() {
        selftest::set_math_fma(fma);
        for j in 1..=64u32 {
            for _ in 0..16 {
                let x = (f64::from(j) - 0.5 + (rng.next() >> 11) as f64 * 2f64.powi(-53)) / 64.0;
                let want: f64 = mp::round::<f64>(mp::atan(Mp::from_f64(x)), Rounding::Nearest).0;
                if let Some(v) = invtrig::atan_nearest(x) {
                    assert_eq!(v.to_bits(), want.to_bits(), "atan({x:e}) = {v:e} near node {j}/64, multiprecision {want:e}: table entry FAILED");
                }
                let e = m::atan(x);
                assert_eq!(e.to_bits(), want.to_bits(), "atan({x:e}) = {e:e} by the entry (fma {fma}) near node {j}/64, multiprecision {want:e}: table entry FAILED");
                // `|x| > 1` with `1/x` near the node: the folded `π/2 − atan(j/64)` entry.
                let xr = 1.0 / x;
                let want: f64 = mp::round::<f64>(mp::atan(Mp::from_f64(xr)), Rounding::Nearest).0;
                if let Some(v) = invtrig::atan_nearest(xr) {
                    assert_eq!(v.to_bits(), want.to_bits(), "atan({xr:e}) = {v:e} near 64/{j}, multiprecision {want:e}: folded table entry FAILED");
                }
                let e = m::atan(xr);
                assert_eq!(e.to_bits(), want.to_bits(), "atan({xr:e}) = {e:e} by the entry (fma {fma}) near 64/{j}, multiprecision {want:e}: folded table entry FAILED");
            }
        }
    }
    selftest::set_math_fma(false);
}

#[test]
fn double_atan_quick_phase_answers_alone_and_agrees_with_the_multiprecision_atan() {
    // Clauses: s10_3_c_0 (D-10.13, D-10.69). The quick phase without tier 2 behind it: it must
    // answer nearly every operand by itself (a quick phase that always refuses would hide behind
    // tier 2) and each answer must be the multiprecision `atan` rounded to nearest, also next to
    // every table node j/64 (where `|v|/|y|` is largest) and at the nodes themselves.
    use liella_math::mp::{self, Mp};
    let mut rng = Rng(0x2f1d_33a7_9b0c_51e3);
    let (mut answered, mut total) = (0usize, 0usize);
    let mut xs: Vec<f64> = (0..=64).map(|j| f64::from(j) / 64.0).filter(|&x| x > 0.0).collect();
    for j in 1..=64u32 {
        for _ in 0..64 {
            xs.push((f64::from(j) - 0.5 + (rng.next() >> 11) as f64 * 2f64.powi(-53)) / 64.0);
        }
    }
    for _ in 0..4096 {
        xs.push(rng.mag(-60, 0).abs());
    }
    for &x in &xs {
        let want: f64 = mp::round::<f64>(mp::atan(Mp::from_f64(x)), Rounding::Nearest).0;
        total += 1;
        if let Some(v) = invtrig::atan_quick_only(-x) {
            answered += 1;
            assert_eq!((-v).to_bits(), want.to_bits(), "atan(-{x:e}) = {v:e} by the quick phase, multiprecision -{want:e}: quick phase FAILED");
        }
    }
    // The test refuses about 2^-8 of operands (EPS = 2^-62 against a 2^-63 bound).
    assert!(answered * 100 > total * 98, "quick phase answered only {answered} of {total}: quick phase FAILED");
}

fn gcd64(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

/// Pythagorean triples `s^2 − t^2, 2st, s^2 + t^2` with legs below `2^53`, from `s` in `s0..s0 + n`.
fn triples(s0: u64, n: u64, ties: bool) -> Vec<(f64, f64)> {
    let mut out = Vec::new();
    for s in s0..s0 + n {
        for t in [s * 21 / 50, s * 21 / 50 + 1, s * 2 / 5, s * 2 / 5 + 1] {
            let (a, b, c) = (s * s - t * t, 2 * s * t, s * s + t * t);
            if (s + t) % 2 == 1 && gcd64(s, t) == 1 && a < 1 << 53 && b < 1 << 53 && (c >= 1 << 53) == ties && c < 1 << 54 {
                out.push((a as f64, b as f64));
            }
        }
    }
    out
}

/// The `double` kernel `invtrig::hypot_rounded` (D-10.104) is the exact path (D-10.13) in every
/// direction, value bits, flags and `errno`, and the fast entry gives its nearest result in both
/// builds: bit patterns, near-binade pairs at every magnitude, the gap boundary (26/27/28), ties
/// (triples with an odd 54-bit hypotenuse) and exact roots at several scales, overflow at
/// `DBL_MAX`, two subnormals (the `2^-1022` tininess edge) and a subnormal beside a normal.
#[test]
fn double_hypot_kernel_is_the_exact_path_in_every_direction() {
    // Clauses: s10_3_c_0 (D-10.104).
    let mut st = 0x243f_6a88_85a3_08d3u64;
    let mut next = move || {
        st ^= st << 13;
        st ^= st >> 7;
        st ^= st << 17;
        st
    };
    let mant = (1u64 << 52) - 1;
    let mut v: Vec<(f64, f64)> = Vec::new();
    for _ in 0..3000 {
        let (p, q) = (next() % (0x7ff << 52), next() % (0x7ff << 52));
        v.push((f64::from_bits(p), f64::from_bits(q)));
        let e = 1 + next() % 2046;
        let eb = (e + next() % 5).saturating_sub(2).min(2046);
        v.push((f64::from_bits(e << 52 | (next() & mant)), f64::from_bits(eb << 52 | (next() & mant))));
    }
    for d in 25..=29u64 {
        for _ in 0..60 {
            let e = 30 + next() % 2016;
            v.push((f64::from_bits(e << 52 | (next() & mant)), f64::from_bits((e - d) << 52 | (next() & mant))));
        }
    }
    let ties = triples(95_000_000, 3000, true);
    let exact = triples(66_000_000, 600, false);
    assert!(ties.len() > 100 && exact.len() > 100, "{} ties, {} exact roots", ties.len(), exact.len());
    for &(a, b) in ties.iter().chain(&exact) {
        for k in [-1000, -1, 0, 900, 960] {
            v.push((a * 2f64.powi(k), b * 2f64.powi(k)));
        }
    }
    let max = f64::MAX;
    for b in [max, 1.0, max * 2f64.powi(-26), max * 2f64.powi(-27), max * 0.7, f64::from_bits(1)] {
        v.push((max, b));
        v.push((max * 0.75, b.min(max * 0.7)));
    }
    let c = 3_184_525_836_262_886u64; // about 2^51.5: two such subnormals have a root next to 2^-1022
    for j in 0..300u64 {
        v.push((f64::from_bits(c + j % 3), f64::from_bits(c - j)));
        v.push((f64::from_bits(next() & mant), f64::from_bits(next() & mant)));
        v.push((f64::from_bits((1 + next() % 30) << 52 | (next() & mant)), f64::from_bits(next() & mant)));
    }
    let modes = [Rounding::Nearest, Rounding::Up, Rounding::Down, Rounding::Zero];
    for &(x, y) in &v {
        for (x, y) in [(x, y), (-y, x)] {
            for mode in modes {
                let (w, ws) = pow::hypot_exact::<f64>(x, y, mode);
                let (g, gs) = invtrig::hypot_rounded(x, y, mode).expect("a finite pair");
                assert!(g.to_bits() == w.to_bits() && gs == ws, "hypot({x:e}, {y:e}) {mode:?}: kernel {g:e} {gs:?}, exact {w:e} {ws:?}: kernel FAILED");
            }
            let w = pow::hypot_exact::<f64>(x, y, Rounding::Nearest).0;
            if let Some(f) = invtrig::hypot_nearest(x, y, f64::sqrt) {
                assert_eq!(f.to_bits(), w.to_bits(), "hypot({x:e}, {y:e}): fast entry {f:e}, exact {w:e}: fast FAILED");
            }
            for fma in builds() {
                selftest::set_math_fma(fma);
                assert_eq!(m::hypot(x, y).to_bits(), w.to_bits(), "hypot({x:e}, {y:e}) by the entry (fma {fma}): entry FAILED");
            }
        }
    }
    selftest::set_math_fma(false);
}
