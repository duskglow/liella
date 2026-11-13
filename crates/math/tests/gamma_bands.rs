//! The `double` `lgamma`/`tgamma` band forms (`liella_math::fast_gamma::lgamma_band`,
//! `tgamma_band`, D-10.105) against the 384-bit path, spec §10.3 and D-10.13 (correct rounding
//! in every mode): identical result, status and sign on operands spread over each band
//! (`|x| < 2^-60` with the subnormals, `x >= 16` up to the overflow) and at the band edges,
//! including the powers of two where the tiny `tgamma` value sits next to `1/x`, and the
//! subnormal `x` where `tgamma` reaches `2^1023` and overflows.
#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::arithmetic_side_effects)]

use liella_math::fast_gamma;
use liella_math::special;
use liella_math::status::Rounding;

const MODES: [Rounding; 4] = [Rounding::Nearest, Rounding::Up, Rounding::Down, Rounding::Zero];

/// A deterministic spread: `n` positive operands `2^e·(1 + f)`, `e` uniform in `[e0, e1)`, random
/// `f`; below `2^-1022` the subnormal of that magnitude (the significand shifted, low bits dropped).
fn spread(n: u64, e0: i64, e1: i64, seed: u64) -> Vec<f64> {
    let mut s = seed;
    (0..n)
        .map(|_| {
            s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            let e = e0 + ((s >> 33) as i64).rem_euclid(e1 - e0);
            let frac = s & ((1 << 52) - 1);
            let bits = if e >= -1022 { (((e + 1023) as u64) << 52) | frac } else { ((1 << 52) | frac) >> (-1022 - e) };
            f64::from_bits(bits)
        })
        .collect()
}

/// The operands (with the mode) the band form left to the accurate path, for the share asserts.
type Misses = Vec<(f64, Rounding)>;

fn check_lgamma(xs: &[f64]) -> (u64, Misses) {
    let mut fast = 0;
    let mut miss = Vec::new();
    for &x in xs {
        for m in MODES {
            let got = special::lgamma::<f64>(x, m);
            let want = special::lgamma_accurate::<f64>(x, m);
            assert!(got.0 .0.to_bits() == want.0 .0.to_bits() && got.0 .1 == want.0 .1 && got.1 == want.1, "lgamma({x:e}) in {m:?}: {got:?} != {want:?}");
            let hit = fast_gamma::lgamma_band(x, m).is_some();
            fast += u64::from(hit);
            if !hit {
                miss.push((x, m));
            }
        }
    }
    (fast, miss)
}

fn check_tgamma(xs: &[f64]) -> (u64, Misses) {
    let mut fast = 0;
    let mut miss = Vec::new();
    for &x in xs {
        for m in MODES {
            let got = special::tgamma::<f64>(x, m);
            let want = special::tgamma_accurate::<f64>(x, m);
            assert!(got.0.to_bits() == want.0.to_bits() && got.1 == want.1, "tgamma({x:e}) in {m:?}: {got:?} != {want:?}");
            let hit = fast_gamma::tgamma_band(x, m).is_some();
            fast += u64::from(hit);
            if !hit {
                miss.push((x, m));
            }
        }
    }
    (fast, miss)
}

/// The first misses, smallest `|x|` first, for an assert message.
fn first_misses(miss: &Misses) -> String {
    let mut v = miss.clone();
    v.sort_by(|a, b| a.0.abs().total_cmp(&b.0.abs()));
    v.iter().take(8).map(|(x, m)| format!("{x:e} ({:#018x}) {m:?}", x.to_bits())).collect::<Vec<_>>().join(", ")
}

/// `|x| < 2^-60` (both signs, subnormals included) for `lgamma` and `tgamma`: the band form
/// agrees with the 384-bit path and answers (nearly) every operand itself.
#[test]
fn tiny_operands_agree_with_the_accurate_path() {
    // Clauses: s10_3_c_0 (D-10.13, D-10.105).
    let mut xs = spread(600, -1074, -60, 1);
    xs.extend(spread(200, -1074, -60, 2).iter().map(|x| -x));
    xs.extend([f64::from_bits(1), -f64::from_bits(1), f64::MIN_POSITIVE, 2f64.powi(-61), -2f64.powi(-61), 2f64.powi(-200), -2f64.powi(-600)]);
    // tgamma's overflow edge: 2^-1024 (just under 2^1024, the accurate path), its subnormal
    // neighbours (|Γ| just under DBL_MAX, or over it), and 2^-1023 (|Γ| just under 2^1023).
    let p1024 = 1u64 << 50;
    for b in [p1024 - 2, p1024 - 1, p1024, p1024 + 1, p1024 + 2, p1024 + 3, 2 * p1024, 3 * p1024] {
        xs.extend([f64::from_bits(b), -f64::from_bits(b)]);
    }
    let (fl, ml) = check_lgamma(&xs);
    let (ft, mt) = check_tgamma(&xs);
    let n = 4 * xs.len() as u64;
    assert!(fl * 100 >= n * 99, "lgamma tiny: {fl} of {n} by the band form; missed {}", first_misses(&ml));
    assert!(ft * 100 >= n * 99, "tgamma tiny: {ft} of {n} by the band form; missed {}", first_misses(&mt));
}

/// `x >= 16`: Stirling's form up to `2^1012`, the overflow from `X_OVER`, and the band edges.
#[test]
fn large_operands_agree_with_the_accurate_path() {
    // Clauses: s10_3_c_0 (D-10.13, D-10.105).
    let mut xs = spread(800, 4, 1012, 3);
    xs.extend([16.0, 16.5, 17.25, 31.999_999_999_999_996, 1e15 + 0.5, 2f64.powi(500), fast_gamma::X_OVER, f64::MAX, 3e305, 2.5e305]);
    let (fl, ml) = check_lgamma(&xs);
    let n = 4 * xs.len() as u64;
    assert!(fl * 100 >= n * 98, "lgamma large: {fl} of {n} by the band form; missed {}", first_misses(&ml));
}
