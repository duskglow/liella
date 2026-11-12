//! The tests of the `float` fast paths (`super` and its family modules).

use super::*;
use crate::exp_log as acc;

const ALL: [Fast; 23] = [Fast::Exp, Fast::Exp2, Fast::Exp10, Fast::Log, Fast::Log2, Fast::Log10, Fast::Log1p, Fast::Sin, Fast::Cos, Fast::Tan, Fast::Expm1, Fast::Cbrt, Fast::Asin, Fast::Acos, Fast::Atan, Fast::Sinh, Fast::Cosh, Fast::Tanh, Fast::Asinh, Fast::Acosh, Fast::Atanh, Fast::Erf, Fast::Erfc];
const NAMES: [&str; 23] = ["expf", "exp2f", "exp10f", "logf", "log2f", "log10f", "log1pf", "sinf", "cosf", "tanf", "expm1f", "cbrtf", "asinf", "acosf", "atanf", "sinhf", "coshf", "tanhf", "asinhf", "acoshf", "atanhf", "erff", "erfcf"];
/// The functions whose accurate path is cheap enough for [`agree`]'s wide strides: not erff and
/// erfcf (384-bit mp, some 2·10^5 instructions a call), which have their own test.
const CHEAP: usize = 21;
const TRIG: [Fast; 3] = [Fast::Sin, Fast::Cos, Fast::Tan];
pub(super) const MODES: [Rounding; 4] = [Rounding::Nearest, Rounding::Up, Rounding::Down, Rounding::Zero];

/// [`eval`], and for the Bessel functions the large band after its refusal, the order
/// `special::j0` and its siblings take (D-10.109): what the `float` functions answer without the
/// accurate path.
pub(super) fn fast(f: Fast, x: f32, m: Rounding) -> Option<(f32, Status)> {
    let (n, name) = match f {
        Fast::J0 => (0, "j0f"),
        Fast::J1 => (1, "j1f"),
        Fast::Y0 => (2, "y0f"),
        Fast::Y1 => (3, "y1f"),
        _ => return eval(f, x, m),
    };
    eval(f, x, m).or_else(|| bessel_large(n, x, m, name))
}

pub(super) fn accurate(f: Fast, x: f32, m: Rounding) -> (f32, Status) {
    match f {
        Fast::Exp => acc::exp(x, m),
        Fast::Exp2 => acc::exp2(x, m),
        Fast::Exp10 => acc::exp10(x, m),
        Fast::Log => acc::log(x, m),
        Fast::Log2 => acc::log2(x, m),
        Fast::Log10 => acc::log10(x, m),
        Fast::Log1p => acc::log1p(x, m),
        Fast::Sin => crate::trig::sin(x, m),
        Fast::Cos => crate::trig::cos(x, m),
        Fast::Tan => crate::trig::tan(x, m),
        Fast::Expm1 => acc::expm1(x, m),
        Fast::Cbrt => crate::pow::cbrt(x, m),
        Fast::Asin => crate::trig::asin(x, m),
        Fast::Acos => crate::trig::acos(x, m),
        Fast::Atan => crate::trig::atan(x, m),
        Fast::Sinh => crate::hyper::sinh(x, m),
        Fast::Cosh => crate::hyper::cosh(x, m),
        Fast::Tanh => crate::hyper::tanh(x, m),
        Fast::Asinh => crate::hyper::asinh(x, m),
        Fast::Acosh => crate::hyper::acosh(x, m),
        Fast::Atanh => crate::hyper::atanh(x, m),
        Fast::Erf => crate::special::erf(x, m),
        Fast::Erfc => crate::special::erfc(x, m),
        Fast::Tgamma => crate::special::tgamma_accurate::<f32>(x, m),
        Fast::Lgamma => crate::special::lgamma_accurate::<f32>(x, m).0,
        Fast::J0 => crate::special::j0_accurate(x, m),
        Fast::J1 => crate::special::j1_accurate(x, m),
        Fast::Y0 => crate::special::y0_accurate(x, m),
        Fast::Y1 => crate::special::y1_accurate(x, m),
    }
}

/// Compare the fast path with the accurate path on `xs` in every mode; the number taken.
fn agree(xs: impl Iterator<Item = f32> + Clone) -> u64 {
    agree_in(&ALL[..CHEAP], xs)
}

/// [`agree`] for the functions `fs`; with trig among them also `sincosf` against `sinf`, `cosf`.
pub(super) fn agree_in(fs: &[Fast], xs: impl Iterator<Item = f32> + Clone) -> u64 {
    let mut taken = 0;
    if fs.contains(&Fast::Sin) {
        for m in MODES {
            for x in xs.clone() {
                let ((s, c), st) = sincos(x, m);
                let (ws, wt) = sin(x, m);
                let (wc, wu) = cos(x, m);
                assert_eq!((m, x.to_bits(), s.to_bits(), c.to_bits(), st), (m, x.to_bits(), ws.to_bits(), wc.to_bits(), wt.and(wu)), "sincosf");
            }
        }
    }
    for &f in fs {
        for m in MODES {
            for x in xs.clone() {
                if let Some(r) = eval(f, x, m) {
                    taken += 1;
                    let want = accurate(f, x, m);
                    assert_eq!((f, m, x.to_bits(), r.0.to_bits(), r.1), (f, m, x.to_bits(), want.0.to_bits(), want.1));
                }
            }
        }
    }
    taken
}

#[test]
fn tables_and_constants_are_the_rounded_values() {
    for (j, t) in EXP2_TAB.iter().enumerate() {
        assert_eq!(t.to_bits(), acc::exp2(j as f64 / 128.0, Rounding::Nearest).0.to_bits(), "EXP2_TAB[{j}]");
    }
    for i in 0..128 {
        let c = if i == 0 || i == 127 { 1.0 } else if i < 64 { 1.0 + (i as f64 + 0.5) / 128.0 } else { (1.0 + (i as f64 + 0.5) / 128.0) / 2.0 };
        assert_eq!(LOG_INV[i].to_bits() & 0x1fff_ffff, 0, "LOG_INV[{i}] has 24 bits");
        assert!((LOG_INV[i] * c - 1.0).abs() < 1.0 / 16_777_216.0, "LOG_INV[{i}]");
        assert_eq!(LOG_C[i].to_bits(), (0.0 - acc::log(LOG_INV[i], Rounding::Nearest).0).to_bits(), "LOG_C[{i}]"); // +0 for c = 1
    }
    for (j, t) in EXP2_BITS.iter().enumerate() {
        assert_eq!(*t, EXP2_TAB[j].to_bits(), "EXP2_BITS[{j}]");
    }
    let ln2 = acc::log(2.0f64, Rounding::Nearest).0;
    assert_eq!((LN2, E1 * 128.0), (ln2, ln2));
    // E2..E4: (ln 2/128)^k/k! to well within the bound's 2^-61 (exact products of E1 in
    // double-double would be overkill: these are polynomial coefficients, not table values).
    let e = [E1, E1 * E1 / 2.0, E1 * E1 * E1 / 6.0, E1 * E1 * E1 * E1 / 24.0];
    for (k, c) in [E1, E2, E3, E4].into_iter().enumerate() {
        assert!((c / e[k] - 1.0).abs() < 1e-15, "E{}", k + 1);
    }
    assert_eq!((INV_LN2, INV_LN10), (acc::log2(core::f64::consts::E, Rounding::Nearest).0, acc::log10(core::f64::consts::E, Rounding::Nearest).0));
    assert_eq!((INV_L / 128.0, INV_M / 128.0), (INV_LN2, acc::log2(10.0f64, Rounding::Nearest).0));
    // EXP2_LO[j] = RN(2^(j/128) - EXP2_TAB[j]), 2^(j/128) from the accurate path's double-double exp.
    // The reference needs a full double-double ln 2: LN2_HI + LN2_LO holds only about 89 bits
    // (both have trailing zeros), an error near 2^-95 in 2^(j/128), far above EXP2_LO's own
    // rounding (≤ 2^-107). ln2_dd is checked here: RN(ln 2) as its head, and e^ln2_dd = 2.
    let ln2_dd = crate::dd::two_sum(f64::from_bits(0x3fe6_2e42_fefa_39ef), f64::from_bits(0x3c7a_bc9e_3b39_803f));
    assert_eq!(ln2_dd.to_f64(), ln2);
    let (two, m2) = acc::exp_dd(ln2_dd);
    assert!(two.scale(crate::dd::pow2(m2)).add_f64(-2.0).to_f64().abs() <= 2f64.powi(-102), "e^ln2_dd");
    // Tolerance 2^-100 absolute: the reference's error (ln2_dd ≤ 2^-106, exp_dd ≤ 2^-102 as just
    // checked at the far end) plus EXP2_LO's rounding stay below it on a correct table, and it
    // certifies |T + T_lo - 2^(j/128)| ≤ 2^-99, inside the 2^-96 that expm1_core's proof uses.
    // A one-ulp change of an entry (2^-106) is within the proof's slack: equivalent by design.
    for j in 0..128 {
        let (v, m) = acc::exp_dd(ln2_dd.mul_f64(j as f64 / 128.0));
        let lo = v.scale(crate::dd::pow2(m)).sub(crate::dd::Dd::from(EXP2_TAB[j])).to_f64();
        assert!((EXP2_LO[j] - lo).abs() <= 2f64.powi(-100), "EXP2_LO[{j}]: {:e}", EXP2_LO[j] - lo);
    }
    // CBRT_T[i]^3·LOG_INV[i] = 1 within the rounding of CBRT_T[i] (≤ 3/2 ulp relative), CBRT_S[j]^3 = 2^j.
    for i in 0..128 {
        let t = CBRT_T[i];
        let c = crate::dd::Dd::from(t).sqr().mul_f64(t).mul_f64(LOG_INV[i]).add_f64(-1.0).to_f64();
        let ulp = f64::from_bits(t.to_bits() & 0x7ff0_0000_0000_0000) * f64::EPSILON;
        assert!(c.abs() <= 1.5 * ulp / t * (1.0 + 1e-6), "CBRT_T[{i}]: {c:e}");
    }
    for (j, s) in CBRT_S.iter().enumerate() {
        assert_eq!(s.to_bits(), crate::pow::cbrt((1u32 << j) as f64, Rounding::Nearest).0.to_bits(), "CBRT_S[{j}]");
    }
    // Trig: the π/64 parts (27 and 29 bits, their sum π/64 within 2^-119), the exact zeros and
    // ones of the sin(kπ/64) table and every entry against sin² + cos² = 1.
    assert_eq!((PI64_1.to_bits() & 0x3ff_ffff, PI64_2.to_bits() & 0xff_ffff), (0, 0));
    let pi64 = crate::dd::Dd::of(t::PI64);
    let parts = crate::dd::Dd::from(PI64_1).add_f64(PI64_2).add_f64(PI64_3);
    assert!(parts.sub(pi64).to_f64().abs() < 1e-34, "PI64 parts");
    assert_eq!(INV_PI64, crate::dd::Dd::from(1.0).div(pi64).hi, "INV_PI64");
    let tab = |k: usize| crate::dd::Dd::of(t::SIN_K_PI64[k]);
    assert_eq!([0, 32, 64, 96].map(|k| f64::from_bits(t::SIN_K_PI64[k][0])), [0.0, 1.0, 0.0, -1.0]);
    for k in 0..128 {
        let (s, c) = (tab(k), tab((k + 32) & 127));
        assert!(s.sqr().add(c.sqr()).add_f64(-1.0).to_f64().abs() < 1e-30, "SIN_K_PI64[{k}]");
    }
}

/// The rounding test's mask form refuses exactly the margins of the old two-sided test, and
/// the range keeps every result a normal `float`.
#[test]
fn rounding_test_margins_and_range() {
    for low in [0u64, 1, EXP_DELTA - 1, EXP_DELTA, EXP_DELTA + 1, (1 << 28) - EXP_DELTA - 1, (1 << 28) - EXP_DELTA, (1 << 28) - 1] {
        let y = f64::from_bits(0x3ff0_0000_0000_0000 | (5 << 28) | low);
        let refused = low <= EXP_DELTA || low >= (1 << 28) - EXP_DELTA;
        assert_eq!(round(y, EXP_DELTA, Rounding::Nearest).is_none(), refused, "low {low}");
    }
    let mid = 1 << 27;
    assert!(round(f64::from_bits(0x3810_0000_0000_0000 + mid), EXP_DELTA, Rounding::Nearest).is_some());
    assert!(round(f64::from_bits(0x380f_ffff_0000_0000 + mid), EXP_DELTA, Rounding::Nearest).is_none());
    assert!(round(f64::from_bits(0x47ef_ffff_c000_0000 + mid), EXP_DELTA, Rounding::Nearest).is_some());
    assert!(round(f64::from_bits(0x47f0_0000_0000_0000 + mid), EXP_DELTA, Rounding::Nearest).is_none());
}

#[test]
fn fast_path_agrees_with_the_accurate_path() {
    // A stride over every encoding, every float in two ranges with subnormal exp results,
    // the operands near 1 for the logs, and the exact cases (which the test must refuse).
    // The fallback rate (at most 1 in 10^4) on operands in the functions' working ranges: a
    // grid over [-30, 30] for the exps and every 4001st positive normal `float` for the logs.
    // (Over all encodings most exp operands are tiny, e^x is next to 1 and the test refuses
    // them by design.) Measured here, not in a test of its own: the counters are global.
    let exps = (0..1u32 << 17).map(|i| -30.0 + 60.0 * (i as f32) / 131_072.0);
    let logs = (0x0080_0000u32..0x7f80_0000).step_by(4001).map(f32::from_bits);
    // log1pf: a grid over [-0.9, 10] (the bench row's range).
    let log1ps = (0..1u32 << 17).map(|i| -0.9 + 10.9 * (i as f32) / 131_072.0);
    // sinf, cosf, tanf: a grid over [-100, 100].
    let trigs = (0..1u32 << 17).map(|i| -100.0 + 200.0 * (i as f32) / 131_072.0);
    for (i, (f, n)) in ALL.into_iter().zip(NAMES).enumerate() {
        let before = stats::counts(n);
        let xs: std::vec::Vec<f32> = match i {
            0..=2 | 10 => exps.clone().collect(),
            6 => log1ps.clone().collect(),
            7..=9 => trigs.clone().collect(),
            11 => logs.clone().flat_map(|x| [x, -x]).collect(),
            // asinf, acosf over [-1, 1] and atanf over [-10, 10] (the bench rows' ranges).
            12 | 13 => (0..1u32 << 17).map(|i| -1.0 + 2.0 * (i as f32) / 131_072.0).collect(),
            14..=18 => trigs.clone().map(|x| x / 10.0).collect(),
            // acoshf over [1, 100], atanhf over [-0.99, 0.99] (the bench rows' ranges).
            19 => (0..1u32 << 17).map(|i| 1.0 + 99.0 * (i as f32) / 131_072.0).collect(),
            20 => (0..1u32 << 17).map(|i| -0.99 + 1.98 * (i as f32) / 131_072.0).collect(),
            // erff over [-5, 5], erfcf over [-3, 10] (the kernels' working ranges).
            21 => trigs.clone().map(|x| x / 20.0).collect(),
            22 => trigs.clone().map(|x| 3.5 + x * 0.065).collect(),
            _ => logs.clone().collect(),
        };
        for x in xs {
            let _ = eval(f, x, Rounding::Nearest);
        }
        let c = stats::counts(n).since(before);
        assert!(c.check_fallback(n, 10_000).is_ok(), "{n}: fallback rate {c:?}");
    }
    let stride = (0..=u32::MAX).step_by(4099).map(f32::from_bits);
    assert!(agree(stride) > 1_500_000);
    let sub = (0xc2cf_0000u32..0xc2d0_0000).step_by(7).chain((0xc313_0000u32..0xc314_0000).step_by(7)).map(f32::from_bits);
    agree(sub.clone());
    let fast_tiny = sub.filter(|&x| MODES.into_iter().any(|m| eval(Fast::Exp, x, m).is_some() || (x < -126.0 && eval(Fast::Exp2, x, m).is_some())));
    assert_eq!(fast_tiny.count(), 0, "subnormal results are the accurate path's");
    let near1 = (0x3f7f_0000u32..0x3f81_0000).step_by(3).map(f32::from_bits);
    assert!(agree(near1) > 100_000);
    // log1pf: around its 2^-26 cut, where e changes (z = 0.75, 1.5, 2) and near -1.
    let near0 = [0x3280_0000u32, 0xb280_0000, 0x3e80_0000, 0xbe80_0000, 0x3f00_0000, 0x3f80_0000, 0xbf7f_0000]
        .into_iter().flat_map(|b| (b - 0x8000..b + 0x8000).step_by(5)).map(f32::from_bits);
    assert!(agree(near0) > 50_000);
    // Trig: around the 2^-26 and 2^19 cuts and near multiples of π/2 and π/64.
    let cuts = [0x3280_0000u32, 0xb280_0000, 0x4900_0000, 0xc900_0000, 0x3fc9_0fdb, 0x40c9_0fdb, 0x3d49_0fdb, 0x4b49_0fdb]
        .into_iter().flat_map(|b| (b - 0x8000..b + 0x8000).step_by(5)).map(f32::from_bits);
    assert!(agree_in(&TRIG, cuts) > 50_000);
    let exact = (-149..128).flat_map(|k| [k as f32, 2f32.powi(k)]).chain((0..11).map(|k| 10f32.powi(k)));
    agree(exact);
}

/// log1p_core's y is within its proved bound (< 6.9 ulp of y) of ln(1 + x), measured against
/// the double-double `log1p_dd` (error far below 2^-100). The agreement test cannot see a
/// loss of a few ulp of the `double`: it misrounds only next to a `float` boundary.
#[test]
fn log1p_core_is_within_its_bound() {
    let grid = (0..1u32 << 16).map(|i| -0.99 + 20.0 * (i as f32) / 65_536.0);
    let wide = (0x3280_0000u32..0x7f80_0000).step_by(1009).flat_map(|b| [b, b | 1 << 31]).map(f32::from_bits);
    let mut seen = 0u32;
    for x in grid.chain(wide) {
        if let Some(y) = log1p_core(x) {
            seen += 1;
            let f = crate::exp_log::log1p_dd(crate::dd::Dd::from(f64::from(x)));
            let err = (crate::dd::Dd::from(y).sub(f)).to_f64();
            let ulp = f64::from_bits(y.to_bits() & 0x7ff0_0000_0000_0000) / 4_503_599_627_370_496.0;
            assert!(err.to_bits() & !(1 << 63) <= (6.9 * ulp).to_bits(),"log1p_core({x:e}): {} ulp", err / ulp);
        }
    }
    assert!(seen > 1_000_000, "{seen}");
}

/// trig_core's sin, cos (< TRIG_BOUND ulp) and tan = s/c (< TAN_BOUND ulp) against the
/// double-double `sin_cos` of the accurate path (its error far below 2^-100); the margins
/// TRIG_DELTA, TAN_DELTA are at least 1.5·bound + 1. (No spec clause: LIBM-F32-SINF-SHRINK.)
#[test]
fn trig_core_is_within_its_bound() {
    let grid = (0..1u32 << 16).map(|i| -40.0 + 80.0 * (i as f32) / 65_536.0);
    let wide = (0x3280_0000u32..0x4900_0000).step_by(1009).flat_map(|b| [b, b | 1 << 31]).map(f32::from_bits);
    let ulp = |y: f64| f64::from_bits(y.to_bits() & 0x7ff0_0000_0000_0000) / 4_503_599_627_370_496.0;
    let mut seen = 0u32;
    for x in grid.chain(wide) {
        if let Some((s, c)) = trig_core(x) {
            seen += 1;
            let (fs, fc) = crate::trig::sin_cos(f64::from(x));
            for (y, f, b, n) in [(s, fs, TRIG_BOUND, "sin"), (c, fc, TRIG_BOUND, "cos"), (s / c, fs.div(fc), TAN_BOUND, "tan")] {
                let err = crate::dd::Dd::from(y).sub(f).to_f64();
                assert!(err.abs() <= b * ulp(y), "trig_core {n}({x:e}): {} ulp", err / ulp(y));
            }
        }
    }
    assert!(seen > 750_000, "{seen}");
    for (d, b) in [(TRIG_DELTA, TRIG_BOUND), (TAN_DELTA, TAN_BOUND)] {
        assert!(d as f64 >= 1.5 * b + 1.0, "margin {d} ulp for the bound {b} ulp");
    }
}

/// `fs` on every `float` operand in every mode; the number taken. The threads take 2^16-operand
/// chunks from one counter, so one that meets a cheap range (NaN, a refused domain) takes more
/// chunks instead of idling while another finishes a fixed slice of expensive operands.
/// `LIELLA_SWEEP_STRIDE=k` checks every k-th encoding only (sizing a sweep before its job).
fn exhaustive(fs: &'static [Fast]) -> u64 {
    exhaustive_by(|xs| agree_in(fs, xs.iter().copied()))
}

/// [`exhaustive`] with the check `check` on each chunk's operands.
pub(super) fn exhaustive_by(check: impl Fn(&[f32]) -> u64 + Sync) -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    const CHUNK: u64 = 1 << 16;
    let n = std::thread::available_parallelism().map_or(8, |n| n.get());
    let stride = std::env::var("LIELLA_SWEEP_STRIDE").ok().and_then(|s| s.parse::<usize>().ok()).filter(|&k| k > 0).unwrap_or(1);
    let next = AtomicU64::new(0);
    let (taken, seen) = std::thread::scope(|sc| {
        let hs: std::vec::Vec<_> = (0..n).map(|_| sc.spawn(|| {
            let (mut taken, mut seen) = (0, 0);
            loop {
                let lo = next.fetch_add(CHUNK, Ordering::Relaxed);
                if lo >= 1 << 32 {
                    return (taken, seen);
                }
                let xs: std::vec::Vec<f32> = (lo..lo + CHUNK).step_by(stride).map(|b| f32::from_bits(b as u32)).collect();
                seen += xs.len() as u64;
                taken += check(&xs);
            }
        })).collect();
        hs.into_iter().map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e))).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1))
    });
    // Every chunk exactly once: a lost or repeated chunk would pass unseen otherwise.
    assert_eq!(seen, ((1u64 << 32) / CHUNK) * CHUNK.div_ceil(stride as u64), "sweep coverage (stride {stride})");
    taken
}

/// ATAN32, PIO2 and PI are the rounded values (against the double-double `atan_dd`, error far
/// below 2^-100), and each inverse trigonometric margin exceeds its proved bound by half and
/// one ulp. (No spec clause: LIBM-F32-DOUBLE-KERNELS.)
#[test]
fn inverse_trig_tables_and_margins() {
    use crate::dd::Dd;
    for (i, &a) in ATAN32.iter().enumerate() {
        assert_eq!(a.to_bits(), crate::trig::atan_dd(Dd::from(i as f64 / 32.0)).to_f64().to_bits(), "ATAN32[{i}]");
    }
    let q = crate::trig::atan_dd(Dd::from(1.0));
    assert_eq!(PIO2.to_bits(), q.scale(2.0).to_f64().to_bits());
    assert_eq!(PI.to_bits(), q.scale(4.0).to_f64().to_bits());
    for (d, b) in [(ATAN_DELTA, ATAN_BOUND), (ASIN_DELTA, ASIN_BOUND), (ATAN2_DELTA, ATAN2_BOUND)] {
        assert!(d as f64 >= 1.5 * b + 1.0, "margin {d} ulp for the bound {b} ulp");
    }
}

/// The double-double references of the inverse trigonometric kernels: atan2(y, x) by the
/// octant (asin and acos are atan2(a, √(1 - a²)) and atan2(√(1 - x), √(1 + x))·2).
fn atan2_dd(y: crate::dd::Dd, x: crate::dd::Dd) -> crate::dd::Dd {
    let half = crate::trig::atan_dd(crate::dd::Dd::from(1.0)).scale(2.0);
    let (ay, ax) = (if y.hi < 0.0 { y.neg() } else { y }, if x.hi < 0.0 { x.neg() } else { x });
    let t = if ay.sub(ax).hi <= 0.0 { crate::trig::atan_dd(ay.div(ax)) } else { half.sub(crate::trig::atan_dd(ax.div(ay))) };
    let t = if x.hi < 0.0 { half.scale(2.0).sub(t) } else { t };
    if y.hi < 0.0 { t.neg() } else { t }
}

/// atan_core (≤ ATAN_BOUND ulp), asin_core and acos_core (≤ ASIN_BOUND) and atan2_core
/// (≤ ATAN2_BOUND) against [`atan2_dd`]. The agreement tests cannot see a loss of a few ulp of
/// the `double`: it misrounds only next to a `float` boundary. (No spec clause.)
#[test]
fn inverse_trig_cores_are_within_their_bounds() {
    let Some(verdict) = crate::verdict::Key::new(concat!(env!("CARGO_CRATE_NAME"), ".fast32.inverse_trig_cores_are_within_their_bounds")).begin() else {
        return;
    };
    use crate::dd::{two_sum, Dd};
    let ulp = |y: f64| f64::from_bits(y.to_bits() & 0x7ff0_0000_0000_0000) / 4_503_599_627_370_496.0;
    let check = |y: Option<f64>, f: Dd, b: f64, what: &str, x: f32| {
        assert!(y.is_some(), "{what}({x:e}) refused");
        let y = y.unwrap_or(f64::NAN);
        let err = Dd::from(y).sub(f).to_f64();
        assert!(err.abs() <= b * ulp(y), "{what}({x:e}): {} ulp", err / ulp(y));
    };
    let one = Dd::from(1.0);
    // sqrt64 (≤ 2u) over its working range (1 - a², (1 ± x)/(1 ∓ x): 2^-50 to 1).
    for k in (0x3cd0_0000_0000_0000u64..=0x3ff0_0000_0000_0000).step_by(0x0000_0f00_0f00_0f01) {
        let x = f64::from_bits(k);
        let (s, f) = (sqrt64(x), crate::dd::sqrt(Dd::from(x)));
        assert!(Dd::from(s).sub(f).to_f64().abs() <= 2.0 * s / 9_007_199_254_740_992.0, "sqrt64({x:e})");
    }
    let mut seen = 0u32;
    // Every 61st `float` in [2^-26, 1), both signs, then atan up to 2^30.
    for b in (0x3280_0000u32..0x3f80_0000).step_by(61).flat_map(|b| [b, b | 1 << 31]) {
        let x = f32::from_bits(b);
        let a = f64::from(x);
        check(atan_core(x), atan2_dd(Dd::from(a), one), ATAN_BOUND, "atan_core", x);
        let d = crate::dd::sqrt(two_sum(1.0, -a).mul(two_sum(1.0, a)));
        check(asin_core(x), atan2_dd(Dd::from(a), d), ASIN_BOUND, "asin_core", x);
        let (p, q) = (crate::dd::sqrt(two_sum(1.0, -a)), crate::dd::sqrt(two_sum(1.0, a)));
        check(acos_core(x), atan2_dd(p, q).scale(2.0), ASIN_BOUND, "acos_core", x);
        seen += 1;
    }
    for b in (0x3f80_0000u32..0x4e80_0000).step_by(61).flat_map(|b| [b, b | 1 << 31]) {
        let x = f32::from_bits(b);
        check(atan_core(x), atan2_dd(Dd::from(f64::from(x)), one), ATAN_BOUND, "atan_core", x);
    }
    // atan2: ratios across the range, every quadrant and the octant edges.
    let vals: std::vec::Vec<f32> = (0x0080_0000u32..0x7f80_0000).step_by(0x7_0001).map(f32::from_bits).collect();
    for &u in &vals {
        for &v in &vals {
            for (y, x) in [(u, v), (-u, v), (u, -v), (-u, -v), (u, u), (u, -u)] {
                let f = atan2_dd(Dd::from(f64::from(y)), Dd::from(f64::from(x)));
                check(atan2_core(y, x), f, ATAN2_BOUND, "atan2_core", y);
            }
        }
    }
    assert!(seen > 1_000_000, "{seen}");
    verdict.pass();
}

/// atan2f's fast path against the accurate path in every mode: pairs across the range with
/// both signs, the octant edges |y| = |x| ± 1 ulp, the bench row's square [-10, 10]² (and its
/// fallback rate), and the C Annex F F.10.1.4 special cases (zeros, infinities, NaN), which
/// the fast path must refuse. (Clause s10_0_f_0's Annex F reference; LIBM-F32-DOUBLE-KERNELS.)
#[test]
fn atan2_fast_path_agrees_with_the_accurate_path() {
    let mut taken = 0u64;
    let mut pair = |y: f32, x: f32| {
        for m in MODES {
            if let Some(r) = eval_atan2(y, x, m) {
                taken += 1;
                let want = crate::trig::atan2(y, x, m);
                assert_eq!((m, y.to_bits(), x.to_bits(), r.0.to_bits(), r.1), (m, y.to_bits(), x.to_bits(), want.0.to_bits(), want.1));
            }
        }
    };
    let vals: std::vec::Vec<f32> = (1u32..0x7f80_0000).step_by(0x1f_0003).map(f32::from_bits).collect();
    for &u in &vals {
        for &v in &vals {
            for (y, x) in [(u, v), (-u, v), (u, -v), (-u, -v)] {
                pair(y, x);
            }
        }
        let (up, dn) = (f32::from_bits(u.to_bits() + 1), f32::from_bits(u.to_bits() - 1));
        for (y, x) in [(u, u), (up, u), (dn, u), (-u, -u), (up, -u), (-dn, -u)] {
            pair(y, x);
        }
    }
    let before = stats::counts("atan2f");
    for i in 0..256u32 {
        for j in 0..256u32 {
            pair(-10.0 + 20.0 * (i as f32 + 0.37) / 256.0, -10.0 + 20.0 * (j as f32 + 0.61) / 256.0);
        }
    }
    let c = stats::counts("atan2f").since(before);
    assert!(c.check_fallback("atan2f", 10_000).is_ok(), "atan2f: fallback rate {c:?}");
    assert!(taken > 400_000, "{taken}");
    let (inf, nan) = (f32::INFINITY, f32::NAN);
    for y in [0.0, -0.0, inf, -inf, nan, 1.5, -1.5, f32::MIN_POSITIVE / 4.0] {
        for x in [0.0, -0.0, inf, -inf, nan, 1.5, -1.5] {
            let special = !(y.is_finite() && x.is_finite() && y != 0.0 && x != 0.0);
            for m in MODES {
                let r = eval_atan2(y, x, m);
                assert!(!special || r.is_none(), "atan2f({y}, {x}) taken");
                let (got, want) = (atan2(y, x, m), crate::trig::atan2(y, x, m));
                assert_eq!((got.0.to_bits(), got.1), (want.0.to_bits(), want.1), "atan2f({y}, {x}) {m:?}");
            }
        }
    }
}

/// asinf on every `float` operand in every mode against the accurate path (one job per
/// function, 2^34 calls; LIBM-F32-DOUBLE-KERNELS's accuracy check). (No spec clause.)
#[test]
#[ignore]
fn asinf_fast_path_agrees_exhaustively() {
    let taken = exhaustive(&[Fast::Asin]);
    std::println!("asinf fast path taken {taken} of {} calls", 4u64 << 32);
}

/// acosf on every `float` operand in every mode (as for asinf). (No spec clause.)
#[test]
#[ignore]
fn acosf_fast_path_agrees_exhaustively() {
    let taken = exhaustive(&[Fast::Acos]);
    std::println!("acosf fast path taken {taken} of {} calls", 4u64 << 32);
}

/// atanf on every `float` operand in every mode (as for asinf). (No spec clause.)
#[test]
#[ignore]
fn atanf_fast_path_agrees_exhaustively() {
    let taken = exhaustive(&[Fast::Atan]);
    std::println!("atanf fast path taken {taken} of {} calls", 4u64 << 32);
}

/// atan2f on 2^24 y (every 256th encoding) against 64 x spread over the range and both signs,
/// every mode (atan2f has 2^64 operand pairs: a sample, in its own job).
#[test]
#[ignore]
fn atan2_fast_path_agrees_on_a_wide_sample() {
    let n = std::thread::available_parallelism().map_or(8, |n| n.get()) as u32;
    let xs: std::vec::Vec<f32> = (0..32u32).map(|k| f32::from_bits(0x0040_0000 + k * 0x03f8_0000 + k * 977)).flat_map(|v| [v, -v]).collect();
    let taken: u64 = std::thread::scope(|sc| {
        let hs: std::vec::Vec<_> = (0..n).map(|t| {
            let xs = &xs;
            sc.spawn(move || {
                let mut taken = 0u64;
                for k in (t..1u32 << 24).step_by(n as usize) {
                    let y = f32::from_bits(k << 8 | (k & 0xff));
                    for &x in xs {
                        for m in MODES {
                            if let Some(r) = eval_atan2(y, x, m) {
                                taken += 1;
                                let want = crate::trig::atan2(y, x, m);
                                assert_eq!((m, y.to_bits(), x.to_bits(), r.0.to_bits(), r.1), (m, y.to_bits(), x.to_bits(), want.0.to_bits(), want.1));
                            }
                        }
                    }
                }
                taken
            })
        }).collect();
        hs.into_iter().map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e))).sum()
    });
    std::println!("atan2f fast path taken {taken} of {} calls", (1u64 << 24) * 64 * 4);
}

/// The in-place nearest entries of sinf, cosf, tanf, where they answer, give the accurate
/// path's round-to-nearest result. (No spec clause: LIBM-F32-SINF-SHRINK.) Not through
/// [`eval`]: its statistics are shared with the fallback-rate check of the agreement test.
#[test]
fn trig_nearest_entries_match_the_accurate_path() {
    let grid = (0..1u32 << 16).map(|i| -40.0 + 80.0 * (i as f32) / 65_536.0);
    let wide = (0u32..0x7f80_0001).step_by(4099).flat_map(|b| [b, b | 1 << 31]).map(f32::from_bits);
    let m = Rounding::Nearest;
    let mut taken = 0u32;
    for x in grid.chain(wide) {
        for (f, near) in [(Fast::Sin, sin_nearest(x)), (Fast::Cos, cos_nearest(x)), (Fast::Tan, tan_nearest(x))] {
            let Some(v) = near else { continue };
            taken += 1;
            let a = match f {
                Fast::Sin => crate::trig::sin(x, m),
                Fast::Cos => crate::trig::cos(x, m),
                _ => crate::trig::tan(x, m),
            };
            assert_eq!(v.to_bits(), a.0.to_bits(), "{f:?}_nearest({x:e})");
        }
    }
    assert!(taken > 500_000, "{taken}");
}

/// Each hyperbolic margin exceeds its proved bound by half and one ulp. (No spec clause:
/// LIBM-F32-HYPERBOLIC.)
#[test]
fn hyperbolic_margins() {
    for (b, d, n) in [(SINH_BOUND, SINH_DELTA, "sinh"), (COSH_BOUND, COSH_DELTA, "cosh"), (TANH_BOUND, TANH_DELTA, "tanh"), (ASINH_BOUND, ASINH_DELTA, "asinh"), (ACOSH_BOUND, ACOSH_DELTA, "acosh"), (ATANH_BOUND, ATANH_DELTA, "atanh")] {
        assert!(d as f64 >= 1.5 * b + 1.0, "{n}: margin {d} ulp for a bound of {b} ulp");
    }
    // The log1p64-based bounds: the argument's error (passed on at most 1:1) plus log1p64's.
    for (w, b, n) in [(3.25, ASINH_BOUND, "asinh"), (4.0, ACOSH_BOUND, "acosh"), (1.0, ATANH_BOUND, "atanh")] {
        assert!(LOG1P64_BOUND + w <= b, "{n}: {w} + log1p64's bound over {b}");
    }
}

/// The hyperbolic cores and log1p64 are within their proved bounds, against double-double
/// references built from the accurate path's `exp_dd`, `expm1_small`, `log1p_dd` and `dd::sqrt`
/// (errors far below 2^-90 relative): every 1021st `float` from 2^-26 up, both signs, plus a
/// grid over the bench rows' ranges. tanh's saturated range is checked by the agreement tests.
/// The agreement tests cannot see a loss of a few ulp of the `double`. (No spec clause.)
#[test]
fn hyperbolic_cores_are_within_their_bounds() {
    use crate::dd::Dd;
    use crate::exp_log::{exp_dd, expm1_small, log1p_dd};
    let ulp = |y: f64| f64::from_bits(y.to_bits() & 0x7ff0_0000_0000_0000) / 4_503_599_627_370_496.0;
    let check = |y: Option<f64>, f: Dd, b: f64, what: &str, x: f64| {
        assert!(y.is_some(), "{what}({x:e}) refused");
        let y = y.unwrap_or(f64::NAN);
        let err = Dd::from(y).sub(f).to_f64();
        assert!(err.abs() <= b * ulp(y), "{what}({x:e}): {} ulp", err / ulp(y));
    };
    let expm1 = |a: f64| if a <= 0.34 { expm1_small(Dd::from(a)) } else { let (v, m) = exp_dd(Dd::from(a)); v.scale(crate::dd::pow2(m)).add_f64(-1.0) };
    let one = Dd::from(1.0);
    // log1p64 on doubles (not only on the w the callers form): every scale from 2^-26 to 2^130,
    // and densely over [2^-8, 2^-7), where case A's series remainder peaks. Each case against
    // its own bound (A: z < 1 + 2^-7, 5.3u; B: z < 1.5, 4.4u; C: LOG1P64_BOUND).
    let scales = (0x3e50_0000_0000_0000u64..0x4810_0000_0000_0000).step_by(0x0000_0f00_0f00_0f01);
    let case_a_top = (0x3f70_0000_0000_0000u64..0x3f80_0000_0000_0000).step_by(0x0000_0040_0000_0001);
    for k in scales.chain(case_a_top) {
        let w = f64::from_bits(k);
        let z = 1.0 + w;
        let b = if z < 1.0 + 1.0 / 128.0 { 5.3 } else if z < 1.5 { 4.4 } else { LOG1P64_BOUND };
        check(Some(log1p64(w)), log1p_dd(Dd::from(w)), b, "log1p64", w);
    }
    // sqrt64 over the range the family adds (1 + a², t(t + 2): up to 2^257).
    for k in (0x3ff0_0000_0000_0000u64..=0x5030_0000_0000_0000).step_by(0x0000_0f00_0f00_0f01) {
        let x = f64::from_bits(k);
        let (s, f) = (sqrt64(x), crate::dd::sqrt(Dd::from(x)));
        assert!(Dd::from(s).sub(f).to_f64().abs() <= 2.0 * s / 9_007_199_254_740_992.0, "sqrt64({x:e})");
    }
    let grid = (0..1u32 << 15).map(|i| f32::from_bits(0x3280_0000) + 100.0 * (i as f32) / 32_768.0);
    let wide = (0x3280_0000u32..0x7f80_0000).step_by(1021).map(f32::from_bits);
    let mut seen = 0u32;
    for x in grid.chain(wide) {
        let a = f64::from(x);
        let da = Dd::from(a);
        for s in [1.0f32, -1.0] {
            let xs = x * s;
            let sg = |v: Dd| if s < 0.0 { v.neg() } else { v };
            if a < 89.0 {
                let e = expm1(a);
                check(sinh_core(xs), sg(e.add(e.div(e.add_f64(1.0))).scale(0.5)), SINH_BOUND, "sinh_core", a);
            }
            if a < 89.5 {
                let (v, m) = exp_dd(da);
                let e = v.scale(crate::dd::pow2(m));
                check(cosh_core(xs), e.add(one.div(e)).scale(0.5), COSH_BOUND, "cosh_core", a);
            }
            if a < TANH_SAT {
                let e = expm1(2.0 * a);
                check(tanh_core(xs), sg(e.div(e.add_f64(2.0))), TANH_BOUND, "tanh_core", a);
            }
            let a2 = da.mul(da);
            let w = da.add(a2.div(crate::dd::sqrt(a2.add_f64(1.0)).add_f64(1.0)));
            check(asinh_core(xs), sg(log1p_dd(w)), ASINH_BOUND, "asinh_core", a);
            check(asinh_core_with(xs, f64::sqrt), sg(log1p_dd(w)), ASINH_BOUND, "asinh_core_with(sqrtsd)", a);
            if a < 1.0 {
                let w = da.scale(2.0).div(one.sub(da));
                check(atanh_core(xs), sg(log1p_dd(w).scale(0.5)), ATANH_BOUND, "atanh_core", a);
            }
        }
        if a > 1.0 {
            let t = da.add_f64(-1.0);
            let w = t.add(crate::dd::sqrt(t.mul(t.add_f64(2.0))));
            check(acosh_core(x), log1p_dd(w), ACOSH_BOUND, "acosh_core", a);
            check(acosh_core_with(x, f64::sqrt), log1p_dd(w), ACOSH_BOUND, "acosh_core_with(sqrtsd)", a);
        }
        seen += 1;
    }
    assert!(seen > 1_200_000, "{seen}");
}

/// sinhf on every `float` operand in every mode against the accurate path (one job per
/// function, 2^34 calls; LIBM-F32-HYPERBOLIC's accuracy check). (No spec clause.)
#[test]
#[ignore]
fn sinhf_fast_path_agrees_exhaustively() {
    let taken = exhaustive(&[Fast::Sinh]);
    std::println!("sinhf fast path taken {taken} of {} calls", 4u64 << 32);
}

/// coshf on every `float` operand in every mode (as for sinhf). (No spec clause.)
#[test]
#[ignore]
fn coshf_fast_path_agrees_exhaustively() {
    let taken = exhaustive(&[Fast::Cosh]);
    std::println!("coshf fast path taken {taken} of {} calls", 4u64 << 32);
}

/// tanhf on every `float` operand in every mode (as for sinhf). (No spec clause.)
#[test]
#[ignore]
fn tanhf_fast_path_agrees_exhaustively() {
    let taken = exhaustive(&[Fast::Tanh]);
    std::println!("tanhf fast path taken {taken} of {} calls", 4u64 << 32);
}

/// asinhf on every `float` operand in every mode (as for sinhf). (No spec clause.)
#[test]
#[ignore]
fn asinhf_fast_path_agrees_exhaustively() {
    let taken = exhaustive(&[Fast::Asinh]);
    std::println!("asinhf fast path taken {taken} of {} calls", 4u64 << 32);
}

/// acoshf on every `float` operand in every mode (as for sinhf). (No spec clause.)
#[test]
#[ignore]
fn acoshf_fast_path_agrees_exhaustively() {
    let taken = exhaustive(&[Fast::Acosh]);
    std::println!("acoshf fast path taken {taken} of {} calls", 4u64 << 32);
}

/// atanhf on every `float` operand in every mode (as for sinhf). (No spec clause.)
#[test]
#[ignore]
fn atanhf_fast_path_agrees_exhaustively() {
    let taken = exhaustive(&[Fast::Atanh]);
    std::println!("atanhf fast path taken {taken} of {} calls", 4u64 << 32);
}

/// `asinh_nearest_sqrt` and `acosh_nearest_sqrt` with a correctly rounded root (`f64::sqrt`,
/// the `sqrtsd` the boundary passes) on the `float`s `xs`: where they answer, the accurate
/// path's nearest result (the entry's flags are the boundary test's); the number taken.
fn agree_sqrt(xs: impl Iterator<Item = f32> + Clone) -> u64 {
    let mut taken = 0;
    for (f, g) in [(Fast::Asinh, asinh_nearest_sqrt as fn(f32, fn(f64) -> f64) -> Option<f32>), (Fast::Acosh, acosh_nearest_sqrt)] {
        for x in xs.clone() {
            if let Some(r) = g(x, f64::sqrt) {
                taken += 1;
                let want = accurate(f, x, Rounding::Nearest).0;
                assert_eq!((f, x.to_bits(), r.to_bits()), (f, x.to_bits(), want.to_bits()), "nearest with sqrtsd");
            }
        }
    }
    taken
}

/// asinhf and acoshf with the hardware root on every 16411th `float` encoding (the gate's
/// share of the exhaustive sweep below). (No spec clause: LIBM-F32-HYPERBOLIC-4.)
#[test]
fn hyperbolic_hardware_sqrt_agrees_sampled() {
    let taken = agree_sqrt((0..u32::MAX).step_by(16411).map(f32::from_bits));
    assert!(taken > 150_000, "{taken}");
}

/// The `float` sin, cos and tan entries with the boundary's conversion (`f64::from`, what its
/// in-place `cvtss2sd` computes) answer exactly where, and exactly what, the bit-built entries do,
/// on every 16411th `float` encoding: the operand handed to the core is x's `double` and each
/// entry rounds its own result. (No spec clause: LIBM-SINF-IVB-RATCHET-2, D-10.79.)
#[test]
fn trig_widen_entries_match_the_bit_built_ones() {
    let mut taken = 0u64;
    for x in (0..u32::MAX).step_by(16411).map(f32::from_bits) {
        let pairs = [(sin_nearest(x), sin_nearest_widen(x, f64::from)), (cos_nearest(x), cos_nearest_widen(x, f64::from)), (tan_nearest(x), tan_nearest_widen(x, f64::from))];
        for (k, (want, got)) in pairs.into_iter().enumerate() {
            assert_eq!(got.map(f32::to_bits), want.map(f32::to_bits), "entry {k} at {:#010x}", x.to_bits());
            taken += u64::from(got.is_some());
        }
    }
    // 261,712 encodings sampled, 45 of 256 exponents in the core's range: about 3 x 46,000 entry
    // calls reach the core, less its refusals (128,091 taken). A floor that proves the core ran.
    assert!(taken > 120_000, "{taken}");
}

/// asinhf and acoshf with the hardware root on every `float` (2^33 calls, its own job:
/// LIBM-F32-HYPERBOLIC-4's accuracy check). (No spec clause.)
#[test]
#[ignore]
fn hyperbolic_hardware_sqrt_agrees_exhaustively() {
    use std::sync::atomic::{AtomicU64, Ordering};
    const CHUNK: u64 = 1 << 16;
    let n = std::thread::available_parallelism().map_or(8, |n| n.get());
    let next = AtomicU64::new(0);
    let taken: u64 = std::thread::scope(|sc| {
        let hs: std::vec::Vec<_> = (0..n).map(|_| sc.spawn(|| {
            let mut taken = 0;
            loop {
                let lo = next.fetch_add(CHUNK, Ordering::Relaxed);
                if lo >= 1 << 32 {
                    return taken;
                }
                taken += agree_sqrt((lo..lo + CHUNK).map(|b| f32::from_bits(b as u32)));
            }
        })).collect();
        hs.into_iter().map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e))).sum()
    });
    std::println!("asinhf/acoshf hardware-root fast path taken {taken} of {} calls", 2u64 << 32);
}

/// sinf (and sincosf) on every `float` operand in every mode against the accurate path, one
/// job per function (LIBM-F32-SINF-SHRINK's accuracy check, 2^34 calls). (No spec clause.)
#[test]
#[ignore]
fn sinf_fast_path_agrees_exhaustively() {
    let taken = exhaustive(&[Fast::Sin]);
    std::println!("sinf fast path taken {taken} of {} calls", 4u64 << 32);
}

/// cosf on every `float` operand in every mode (as for sinf). (No spec clause.)
#[test]
#[ignore]
fn cosf_fast_path_agrees_exhaustively() {
    let taken = exhaustive(&[Fast::Cos]);
    std::println!("cosf fast path taken {taken} of {} calls", 4u64 << 32);
}

/// tanf on every `float` operand in every mode (as for sinf). (No spec clause.)
#[test]
#[ignore]
fn tanf_fast_path_agrees_exhaustively() {
    let taken = exhaustive(&[Fast::Tan]);
    std::println!("tanf fast path taken {taken} of {} calls", 4u64 << 32);
}

/// expm1_core's y is within its proved bound (|y/f - 1| ≤ 9.1·2^-53) of f = e^x - 1, f from the
/// double-double `exp_dd` minus 1. Below |x| = 2^-20 exp_dd's absolute error (about 2^-104)
/// is no longer far below 2^-53·|f|, so the grid starts there. (Clause M6-3: no spec clause.)
#[test]
fn expm1_core_is_within_its_bound() {
    let grid = (0..1u32 << 16).map(|i| -17.9 + 106.8 * (i as f32) / 65_536.0);
    let near0 = (0x3580_0000u32..0x3c00_0000).step_by(97).flat_map(|b| [b, b | 1 << 31]).map(f32::from_bits);
    let mut seen = 0u32;
    for x in grid.chain(near0) {
        if x.abs() < 2f32.powi(-20) {
            continue;
        }
        if let Some(y) = expm1_core(x) {
            seen += 1;
            let (v, m) = acc::exp_dd(crate::dd::Dd::from(f64::from(x)));
            let f = v.scale(crate::dd::pow2(m)).add_f64(-1.0);
            let err = crate::dd::Dd::from(y).sub(f).to_f64();
            assert!(err.abs() <= 9.1 * f.to_f64().abs() / 9_007_199_254_740_992.0, "expm1_core({x:e}): {} δ", err / f.to_f64() * 9_007_199_254_740_992.0);
        }
    }
    assert!(seen > 100_000, "{seen}");
}

/// powf's fast path against the accurate path on pairs, every mode; the number taken.
fn agree_pow(xs: &[f32], ys: &[f32]) -> u64 {
    let mut taken = 0;
    for m in MODES {
        for &x in xs {
            for &y in ys {
                if let Some(r) = eval_pow(x, y, m) {
                    taken += 1;
                    let want = crate::pow::pow(x, y, m);
                    assert_eq!((m, x.to_bits(), y.to_bits(), r.0.to_bits(), r.1), (m, x.to_bits(), y.to_bits(), want.0.to_bits(), want.1));
                }
            }
        }
    }
    taken
}

/// C Annex F F.10.4.4 (no spec clause beyond s10_0_f_0's Annex F reference): every special case
/// of `powf` through the fast entry, in every mode; none of them may take the fast path.
#[test]
fn powf_annex_f_special_cases() {
    let (inf, nan) = (f32::INFINITY, f32::NAN);
    let ok = Status::OK;
    let cases: [(f32, f32, f32, Status); 30] = [
        (0.0, -3.0, inf, Status::POLE), (-0.0, -3.0, -inf, Status::POLE), (0.0, -2.5, inf, Status::POLE),
        (-0.0, -2.0, inf, Status::POLE), (0.0, -inf, inf, ok), (-0.0, -inf, inf, ok),
        (0.0, 3.0, 0.0, ok), (-0.0, 3.0, -0.0, ok), (-0.0, 2.5, 0.0, ok), (-0.0, 4.0, 0.0, ok),
        (-1.0, inf, 1.0, ok), (-1.0, -inf, 1.0, ok), (1.0, nan, 1.0, ok), (1.0, -7.5, 1.0, ok),
        (nan, 0.0, 1.0, ok), (-3.0, -0.0, 1.0, ok), (-2.0, 0.5, nan, Status::DOMAIN),
        (0.5, -inf, inf, ok), (-0.5, -inf, inf, ok), (2.0, -inf, 0.0, ok), (0.5, inf, 0.0, ok),
        (-2.0, inf, inf, ok), (-inf, -3.0, -0.0, ok), (-inf, -2.5, 0.0, ok), (-inf, 3.0, -inf, ok),
        (-inf, 2.5, inf, ok), (inf, -0.5, 0.0, ok), (inf, 0.5, inf, ok), (nan, 2.0, nan, ok), (2.0, nan, nan, ok),
    ];
    for m in MODES {
        for (x, y, v, st) in cases {
            assert!(eval_pow(x, y, m).is_none(), "{x} {y}");
            let (got, gst) = pow(x, y, m);
            let same = if v.is_nan() { got.is_nan() } else { got.to_bits() == v.to_bits() };
            assert!(same && (gst.flags, gst.error) == (st.flags, st.error), "pow({x}, {y}) = {got} {gst:?}");
        }
    }
}

#[test]
fn powf_fast_path_agrees_with_the_accurate_path() {
    // Grids of x and y over the working ranges (negative x with integer y), the subnormal
    // operands and results, the exact powers (which the test must refuse), and the rate.
    let xs: std::vec::Vec<f32> = (0..1500u32).map(|i| f32::from_bits(0x0000_1000 + i * 0x0005_6a3b)).chain((0..300).map(|i| -0.01 - 0.37 * i as f32)).collect();
    let ys: std::vec::Vec<f32> = (0..400).map(|i| -30.0 + 0.1503 * i as f32).chain((-12..13).map(|i| i as f32)).chain([1e-3, -7e-5, 1e-30, 2e-40, 130.7, -77.1, 1e4, 3e5]).collect();
    let n = agree_pow(&xs, &ys);
    assert!(n > 150_000, "{n}");
    let near1: std::vec::Vec<f32> = (0x3f7f_f000u32..0x3f80_1000).step_by(3).map(f32::from_bits).collect();
    assert!(agree_pow(&near1, &[1e3, -2.5e4, 3e6, 0.37]) > 20_000);
    let exact: std::vec::Vec<f32> = (1..40).map(|i| i as f32).chain([0.25, 0.125, 1.5, -2.0, -3.0]).collect();
    agree_pow(&exact, &[2.0, 3.0, 0.5, -1.0, -2.0, 1.5, 0.25, 4.0, 10.0, 24.0]);
    // Subnormal results: x^y near 2^-126 .. 2^-149.
    let sub: std::vec::Vec<f32> = (0..2000).map(|i| -86.0 - 0.009 * i as f32).collect();
    assert!(agree_pow(&[core::f32::consts::E, 0.5, 2.0, 7.25], &sub) > 10_000);
    let before = stats::counts("powf");
    // The rate on pairs with |y·ln x| ≤ 70 (x in [0.01, 100], |y| ≤ 15); next to the overflow
    // and underflow thresholds the results leave the fast range by design.
    for x in (0..2000).map(|i| 0.01 + 0.049_99 * i as f32) {
        for y in (0..500).map(|i| -15.0 + 0.0601 * i as f32) {
            let _ = eval_pow(x, y, Rounding::Nearest);
        }
    }
    let c = stats::counts("powf").since(before);
    assert!(c.check_fallback("powf", 10_000).is_ok(), "powf: fallback rate {c:?}");
}

/// pow_core's result is within its proved bound (< 89.01 ulp) of |x|^y, measured against the
/// double-double e^(y·ln x) (error far below 2^-100); the agreement tests cannot see a loss
/// of a few ulp of the `double`.
#[test]
fn pow_core_is_within_its_bound() {
    let mut seen = 0u32;
    let xs = (0x0000_0100u32..0x7f80_0000).step_by(100_003).map(f32::from_bits);
    for x in xs {
        for j in 0..64 {
            let y = (-40.0 + 1.27 * j as f32) * if j % 3 == 0 { 1.0 } else { 0.3 };
            let Some(v) = pow_core(f64::from(x), f64::from(y)) else { continue };
            seen += 1;
            let l = crate::exp_log::log_dd(crate::dd::Dd::from(f64::from(x))).mul_f64(f64::from(y));
            let (f, sc) = crate::exp_log::exp_dd(l);
            let err = crate::dd::Dd::from(v).sub(f.scale(f64::from_bits(((sc + 1023) as u64) << 52))).to_f64(); // e^l = f·2^sc
            let ulp = f64::from_bits(v.to_bits() & 0x7ff0_0000_0000_0000) / 4_503_599_627_370_496.0;
            // The proof's bound as a function of t: (0.84|t| + 2.06)·2^-53, below 89 overall.
            assert!(err.abs() <= (0.84 * l.hi.abs() + 2.1) * ulp, "pow_core({x:e}, {y:e}): {} ulp", err / ulp);
        }
    }
    assert!(seen > 100_000, "{seen}");
}

/// cbrt_core's y is within its proved bound (|y/∛x - 1| ≤ 5.4·2^-53), measured as
/// |y³/x - 1| ≤ 3·5.4·2^-53 (plus 2^-60 for the square of the error) in double-double. (Clause M6-3: no spec clause.)
#[test]
fn cbrt_core_is_within_its_bound() {
    let wide = (1u32..0x7f80_0000).step_by(997).flat_map(|b| [b, b | 1 << 31]).map(f32::from_bits);
    let near1 = (0x3f70_0000u32..0x3f90_0000).step_by(7).map(f32::from_bits);
    let mut seen = 0u32;
    for x in wide.chain(near1) {
        if let Some(y) = cbrt_core(x) {
            seen += 1;
            let x = f64::from(x);
            let c = crate::dd::Dd::from(y).sqr().mul_f64(y).add_f64(-x).to_f64() / x;
            assert!(c.abs() <= 16.2 / 9_007_199_254_740_992.0 + 2f64.powi(-60), "cbrt_core({x:e}): {} δ", c * 9_007_199_254_740_992.0 / 3.0);
        }
    }
    assert!(seen > 1_000_000, "{seen}");
}

/// powf: every `float` x for a set of y, and every `float` y for a set of x, in every mode
/// (MATH-M6-F32-POWF's proof; its own job, 18 slices of 2^34 calls).
#[test]
#[ignore]
fn powf_fast_path_agrees_exhaustively() {
    let n = std::thread::available_parallelism().map_or(8, |n| n.get()) as u64;
    let per = (1u64 << 32) / n + 1;
    let fixed = [2.0f32, 3.0, -2.0, 0.5, 1.5, -1.5, 1.0 / 3.0, 2.5, 7.1, -0.7, 1e-3, 33.3];
    let other = [2.0f32, 0.9, 1.0001, 10.0, -3.0, 1.5];
    let slices = fixed.iter().map(|&y| (true, y)).chain(other.iter().map(|&x| (false, x)));
    let mut taken = 0u64;
    for (all_x, c) in slices {
        taken += std::thread::scope(|sc| {
            let hs: std::vec::Vec<_> = (0..n).map(|t| sc.spawn(move || {
                let lo = t * per;
                let hi = ((t + 1) * per).min(1 << 32);
                let v: std::vec::Vec<f32> = (lo..hi).map(|b| f32::from_bits(b as u32)).collect();
                if all_x { agree_pow(&v, &[c]) } else { agree_pow(&[c], &v) }
            })).collect();
            hs.into_iter().map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e))).sum::<u64>()
        });
        std::println!("slice all_x={all_x} c={c:e}: fast path taken {taken} so far");
    }
}

/// Every `float` operand in every mode (M6-3's proof; run as its own job, about 2^35 calls).
/// `FAST32_SWEEP=expm1f,cbrtf` limits it to the named functions (default: all of ALL).
#[test]
#[ignore]
fn fast_path_agrees_exhaustively() {
    let want = std::env::var("FAST32_SWEEP").unwrap_or_default();
    let fs: std::vec::Vec<Fast> = ALL.into_iter().zip(NAMES)
        .filter(|(_, n)| want.is_empty() || want.split(',').any(|w| w == *n)).map(|(f, _)| f).collect();
    assert!(!fs.is_empty(), "FAST32_SWEEP={want} names no function");
    std::println!("sweeping {fs:?}");
    let fs = &fs[..];
    let n = std::thread::available_parallelism().map_or(8, |n| n.get()) as u64;
    let per = (1u64 << 32) / n + 1;
    let taken: u64 = std::thread::scope(|sc| {
        let hs: std::vec::Vec<_> = (0..n).map(|t| sc.spawn(move || {
            let lo = t * per;
            let hi = ((t + 1) * per).min(1 << 32);
            agree_in(fs, (lo..hi).map(|b| f32::from_bits(b as u32)))
        })).collect();
        hs.into_iter().map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e))).sum()
    });
    std::println!("fast path taken {taken} of {} calls", (fs.len() as u64 * 4) << 32);
}

/// `f` against the tiered oracle (ruling #2889) on `xs`, `est` its family's estimator: one
/// oracle call per operand the fast path takes in any mode, the accurate path only for the
/// modes the double-double tier leaves undecided; the number of calls taken.
pub(super) fn agree_oracle(f: Fast, est: impl Fn(f32) -> crate::oracle32::Est, xs: impl Iterator<Item = f32>, tally: &crate::oracle32::Tally) -> u64 {
    use crate::oracle32 as o;
    let mut taken = 0;
    for x in xs {
        let got = o::MODES.map(|m| fast(f, x, m));
        if got.iter().all(Option::is_none) {
            continue;
        }
        let want = o::oracle(est(x), tally, |m| accurate(f, x, m));
        for (i, g) in got.into_iter().enumerate() {
            if let Some(r) = g {
                taken += 1;
                let m = o::MODES[i];
                assert_eq!((f, m, x.to_bits(), r.0.to_bits(), r.1), (f, m, x.to_bits(), want[i].0.to_bits(), want[i].1));
            }
        }
    }
    taken
}

/// `f` on every `float` operand in every mode against the tiered oracle (`est` its family's
/// estimator; `LIELLA_SWEEP_STRIDE` thins the sweep); prints the calls taken and how often the
/// oracle needed the mp path.
pub(super) fn exhaustive_oracle(f: Fast, name: &str, est: impl Fn(f32) -> crate::oracle32::Est + Sync) {
    let tally = crate::oracle32::Tally::default();
    let taken = exhaustive_by(|xs| agree_oracle(f, &est, xs.iter().copied(), &tally));
    std::println!("{name} fast path taken {taken} of {} calls; oracle: {}", 4u64 << 32, tally.summary());
}
