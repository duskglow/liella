//! Fast paths for the `double` exponentials and logarithms (Track M milestone 6, story M6-4;
//! Ziv's strategy). Each evaluates `f(x)` as a double-double `y` with a proved bound `eps` on
//! `|f(x) − y|`, then [`settle`] checks that every value in `[y − eps, y + eps]` rounds to the same
//! `double` in the caller's mode. If it does, that is the correctly rounded result (the accurate
//! path in [`crate::exp_log`] would return the same); if not (`None`), the caller runs the accurate
//! path unchanged. The arithmetic is hardware `double` in round-to-nearest (the entry wrapper
//! guarantees the default SSE environment, D-10.13); every product that must be exact uses
//! [`two_prod`] (Dekker, no FMA needed), so the bounds below hold with or without FMA (#932).
//!
//! Tables: the existing `EXP2_J64` (2^(j/64), 1 KiB), `LOG_C`/`LOG_L` (728 B + 1,456 B) and the
//! constants of the accurate path; this story adds no table (#933).
//!
//! **Bounds.** Write `u = 2^-53`. The double-double operations ([`Dd::add`], [`Dd::mul`],
//! [`Dd::mul_f64`]) and the stored double-double constants are accurate to a few `2^-104` of
//! their operands' magnitudes; the Cody–Waite reductions leave `r` within `2^-90` of exact. Every
//! such term is below `2^-80·|y|` in total and is covered by the margin `MARGIN = 2^-70·|y|`.
//! The only error near `2^-70` is the `double` evaluation of the series tail `T` (a few roundings:
//! below `5u·|T| < 2^-50·|T|`), so `eps = 2^-49·|T|·|scale| + 2^-70·|y|` (with the factor 2 for
//! the table value multiplying `T` in `exp`, at most 1.006). See [`exp_kernel`] and [`log`].

#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)] // table indices reduced to the tables' ranges; exponents of bounded range

use crate::coeffs::tables as t;
use crate::dd::{self, two_prod, two_sum, Dd};
use crate::float;
use crate::status::{Flags, Rounding, Status};

/// 2^-70: the margin for all the errors far below the series tail's.
const MARGIN: f64 = f64::from_bits(0x3b90_0000_0000_0000);
/// 2^-49: twice the series tail's own relative error bound.
const TAIL: f64 = f64::from_bits(0x3ce0_0000_0000_0000);
/// 2^-100: covers the rounding of `lo ± eps` in [`settle`].
const SLACK: f64 = f64::from_bits(0x39b0_0000_0000_0000);
/// 1.5·2^52: adding and subtracting it rounds a value below 2^51 to an integer.
const ROUND_INT: f64 = 6_755_399_441_055_744.0;

/// `e^r` times `2^(k/64)` as `(y, m, eps)`: `y·2^m` with `|e^x − y·2^m| <= eps·2^m`, for the
/// reduced `r = rh + rl` (`|rh| <= 0.006`, `|rl| <= 2^-52·|rh|`).
///
/// Proof. `e^r − 1 = r + r²/2 + r³·(1/3! + r/4! + … + r⁵/8!) + δ`, `|δ| <= |r|⁹/9!·1.01 < 2^-86`.
/// The leading `rh + rh²/2` is exact as a double-double (`two_prod`, then `two_sum`); `rl` enters
/// as `rl·(1 + rh)` (dropping `rl·rh²/2 < 2^-76`, `rl²/2 < 2^-120`); the tail
/// `T = rh³·(1/3! + …)` (`|T| < 2^-24.5`) is evaluated in `double` with at most 4 roundings and
/// the coefficients' own roundings (each below `u` relative): error `< 5u·|T| < 2^-50·|T|`. Using
/// `rh` instead of `r` in `T` costs `< |rh|²·|rl| < 2^-74`. `2^(j/64)` is stored as a
/// double-double (error `< 2^-104`), and `y = t + t·p` is formed with `Dd::mul`/`Dd::add`
/// (`< 2^-100` relative). So `|e^r·2^(j/64) − y| <= 2^(j/64)·(2^-50·|T| + 2^-74) + 2^-99 <=
/// 2^-49·|T|·|y| + 2^-70·|y|` (`|y| >= 2^(j/64)·0.994`).
fn exp_kernel(r: Dd, k: i32) -> Option<(Dd, i32, f64)> {
    let (rh, rl) = (r.hi, r.lo);
    if dd::abs(rh) > 0.006 {
        return None;
    }
    const C: [f64; 6] = [1.0 / 6.0, 1.0 / 24.0, 1.0 / 120.0, 1.0 / 720.0, 1.0 / 5040.0, 1.0 / 40320.0];
    let q = C[0] + rh * (C[1] + rh * (C[2] + rh * (C[3] + rh * (C[4] + rh * C[5]))));
    let s = two_prod(rh, rh);
    let tail = rh * s.hi * q;
    let a = two_sum(rh, 0.5 * s.hi);
    let p = Dd { hi: a.hi, lo: a.lo + ((0.5 * s.lo + rl * (1.0 + rh)) + tail) };
    let tj = Dd::of(t::EXP2_J64[usize::try_from(k & 63).unwrap_or(0)]);
    let y = tj.add(tj.mul(p));
    let eps = dd::abs(y.hi) * (TAIL * dd::abs(tail) + MARGIN);
    Some((y, k >> 6, eps))
}

/// `e^x` for `x` in the accurate path's finite range (`|x| >= 2^-60`, `-746 <= x <= 710`).
pub fn exp(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    let kf = dd::nearest_int(x * f64::from_bits(t::SCALES[0]));
    let r = crate::exp_log::reduce(Dd::from(x), kf, &t::LN2_64_CW);
    let (y, m, eps) = exp_kernel(r, dd::to_i32(kf))?;
    settle(y, eps, m, mode)
}

/// `2^x` for `x` in the accurate path's finite range, `x` not an integer.
pub fn exp2(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    let kf = dd::nearest_int(x * 64.0);
    let r = x - kf / 64.0; // exact
    // r·ln2: `Dd::mul_f64` of the stored double-double, relative error < 2^-103.
    let (y, m, eps) = exp_kernel(Dd::of(t::LN2).mul_f64(r), dd::to_i32(kf))?;
    settle(y, eps, m, mode)
}

/// `10^x` for `x` in the accurate path's finite range, not one of its exact cases.
pub fn exp10(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    let kf = dd::nearest_int(x * f64::from_bits(t::SCALES[1]));
    let r = crate::exp_log::reduce(Dd::from(x), kf, &t::LOG10_2_64_CW);
    // (x − k·log10(2)/64)·ln10: the reduction is within 2^-95 and the product within 2^-100.
    let (y, m, eps) = exp_kernel(r.mul(Dd::of(t::LN10)), dd::to_i32(kf))?;
    settle(y, eps, m, mode)
}

/// `ln x` as `(y, eps)` for a positive finite `x != 1`.
///
/// Proof. `x = 2^e·m`, `c ≈ 1/m` from `LOG_C` (12 bits, so `m·c` is exact as `two_prod`), and
/// `r = m·c − 1` exact as a double-double (`p.hi − 1` is exact by Sterbenz). Requiring
/// `|rh| <= 2^-7`: `ln(1 + r) = r − r²/2 + r³·(1/3 − r/4 + … − r⁷/10) + δ` with
/// `|δ| <= |r|¹¹/11·1.01 < 2^-80·|r|`. `rh − rh²/2` is exact as a double-double; `rl` enters as
/// `rl·(1 − rh + rh²)` (dropping `< 2^-74·|r|`); the tail `T` costs `< 2^-50·|T|` as in
/// [`exp_kernel`]. `e·ln2` and `−ln c` are stored double-doubles (`< 2^-104` relative), summed by
/// `Dd::add`. Either `e = 0` and `c = 1` (then `y ≈ r`), or `|y| >= 2^-8.1` while `|r| <= 2^-7`,
/// so the absolute errors other than `T`'s are below `2^-72·|y|`.
fn log_kernel(x: f64) -> Option<(Dd, f64)> {
    let (e, m) = crate::exp_log::decompose(x);
    let i = dd::to_i32(dd::nearest_int((m - 1.0) * 128.0)).clamp(-37, 53);
    let idx = usize::try_from(i + 37).unwrap_or(37);
    let p = two_prod(m, f64::from_bits(t::LOG_C[idx]));
    let r = two_sum(p.hi - 1.0, p.lo);
    let (rh, rl) = (r.hi, r.lo);
    if dd::abs(rh) > 0.007_812_5 {
        return None;
    }
    const C: [f64; 8] = [1.0 / 3.0, -1.0 / 4.0, 1.0 / 5.0, -1.0 / 6.0, 1.0 / 7.0, -1.0 / 8.0, 1.0 / 9.0, -1.0 / 10.0];
    let q = C[0] + rh * (C[1] + rh * (C[2] + rh * (C[3] + rh * (C[4] + rh * (C[5] + rh * (C[6] + rh * C[7]))))));
    let s = two_prod(rh, rh);
    let tail = rh * s.hi * q;
    let a = two_sum(rh, -0.5 * s.hi);
    let lr = Dd { hi: a.hi, lo: a.lo + ((rl * (1.0 - rh + s.hi) - 0.5 * s.lo) + tail) };
    let y = Dd::of(t::LN2).mul_f64(f64::from(e)).add(Dd::of(t::LOG_L[idx])).add(lr);
    Some((y, TAIL * dd::abs(tail) + MARGIN * dd::abs(y.hi)))
}

/// `ln x` for a positive finite `x != 1`.
pub fn log(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    let (y, eps) = log_kernel(x)?;
    settle(y, eps, 0, mode)
}

/// `log2 x` for a positive finite `x` not a power of two: `ln x · (1/ln2)` (the product's error
/// `< 2^-100·|y|`, the bound scaled by `1/ln2 < 1.5`).
pub fn log2(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    let (y, eps) = log_kernel(x)?;
    let y = y.mul(Dd::of(t::INV_LN2));
    settle(y, 1.5 * eps + MARGIN * dd::abs(y.hi), 0, mode)
}

/// `log10 x` for a positive finite `x` not a power of ten (`1/ln10 < 0.5`).
pub fn log10(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    let (y, eps) = log_kernel(x)?;
    let y = y.mul(Dd::of(t::INV_LN10));
    settle(y, 0.5 * eps + MARGIN * dd::abs(y.hi), 0, mode)
}

/// 2^51: at and above it every `double` is an integer (and [`dd::nearest_int`] no longer applies).
const INT_ALL: f64 = 2_251_799_813_685_248.0;

/// Whether `x` is an integer (any `|x| >= 2^51` is).
#[inline(always)]
fn integral(x: f64) -> bool {
    dd::abs(x) >= INT_ALL || dd::nearest_int(x) == x
}

// The in-place entry (MATH-M6-F64-FAST-ENTRY, the `double` twin of the `float` one of
// MATH-M6-ENVELOPE-COST): `f(x)` rounded to nearest by the fast path, or `None`, for a caller
// whose MXCSR control word is the default one (round to nearest, traps masked, no FTZ/DAZ; the
// boundary's `elem_fast` checks it). The answer must be the one the accurate entry gives, and the
// hardware arithmetic must raise exactly the inexact flag, because nothing is merged afterwards:
// * Same answer: each `*_nearest` refuses every argument the accurate function handles before its
//   fast-path call (specials, exact cases, overflow, underflow), so where it answers it is the
//   same call of the same fast path as the accurate function makes in round-to-nearest.
// * Only inexact: the arguments are normal (no denormal-operand); the domains below keep every
//   intermediate of the kernels far from overflow and from the subnormal range (exp: `|r| >= 2^-61`
//   so `r²·2^-53` stays above 2^-230; log: `|m·c − 1|` is 0 or at least 2^-64), and the result
//   normal, so [`settle`]'s `tiny` (underflow) is never reached and `m <= 1022`.
// * Inexact is raised: when `settle` answers in round-to-nearest, `a = y.hi + dn` and
//   `b = y.hi + up` are equal while `up − dn = 2e > 0`; both exact would make them differ (an exact
//   sum of `y.hi` and a nonzero value below half an ulp is impossible, and a value of an ulp or
//   more would separate them), so at least one of the two additions is inexact in hardware.

/// `e^x` for the in-place entry: `2^-60 <= |x| < 708` (the result normal and finite).
#[inline]
pub fn exp_nearest(x: f64) -> Option<f64> {
    if !(crate::exp_log::TINY..708.0).contains(&dd::abs(x)) {
        return None;
    }
    exp(x, Rounding::Nearest).map(|(v, _)| v)
}

/// `2^x` for the in-place entry: `2^-60 <= |x| < 1021`, `x` not an integer (exact).
#[inline]
pub fn exp2_nearest(x: f64) -> Option<f64> {
    if !(crate::exp_log::TINY..1021.0).contains(&dd::abs(x)) || integral(x) {
        return None;
    }
    exp2(x, Rounding::Nearest).map(|(v, _)| v)
}

/// Whether `x` is a positive normal finite `double` other than 1 (the logarithms' fast domain).
/// No cap below `f64::MAX`: the kernel's only magnitude-dependent steps are the exponent `e`
/// (`|e| <= 1024`, `e·ln2` within the bound of [`log_kernel`] and [`log_pow`]) and the
/// decomposition, which is integer-only (#1722: its old floating `x·2^54` overflowed in
/// hardware from 2^970 up, and the domain was capped at 2^960).
#[inline(always)]
fn log_domain(x: f64) -> bool {
    (f64::MIN_POSITIVE..=f64::MAX).contains(&x) && x != 1.0
}

/// `ln x` for the in-place entry: `x` positive, normal, finite, not 1.
#[inline]
pub fn log_nearest(x: f64) -> Option<f64> {
    if !log_domain(x) {
        return None;
    }
    log(x, Rounding::Nearest).map(|(v, _)| v)
}

/// `log2 x` for the in-place entry: as [`log_nearest`], `x` not a power of two (exact).
#[inline]
pub fn log2_nearest(x: f64) -> Option<f64> {
    if !log_domain(x) || x.to_bits() & 0x000f_ffff_ffff_ffff == 0 {
        return None;
    }
    log2(x, Rounding::Nearest).map(|(v, _)| v)
}

/// `log10 x` for the in-place entry: as [`log_nearest`], `x` not one of the powers of ten
/// `10^1 ..= 10^22` (exact; only integers in that range are compared).
#[inline]
pub fn log10_nearest(x: f64) -> Option<f64> {
    if !log_domain(x) {
        return None;
    }
    if (10.0..=1e22).contains(&x) && integral(x) {
        let mut p = 1.0;
        for _ in 1..=22 {
            p *= 10.0;
            if x == p {
                return None;
            }
        }
    }
    log10(x, Rounding::Nearest).map(|(v, _)| v)
}

/// `x^y` for the in-place entry: `x` positive, normal, finite, not 1; `2^-200 <= |y|`, and
/// `y·2^11` not an integer.
///
/// No exact case is refused by more than this: an exact `x^y` (a `double` or a midpoint) needs
/// `y = n/2^k` with `k <= 10`. If `x = 2^e` (`|e| <= 1074 < 2^11`), `2^(e·y)` is dyadic only for
/// an integral `e·y`; otherwise `x = u·2^e` with an odd `u >= 3` below 2^53, and
/// `x^(n/2^k) = v·2^f` (`v` odd, at most 54 bits) means `u^n = v^(2^k)` with `v >= 3`, so
/// `3^(2^k) <= v^(2^k) = u^n`; for `n = 1` that is `3^(2^k) < 2^53`, `k <= 5`, and in general the
/// odd `u` is itself a `2^k`-th power `w^(2^k)` (`n` odd and coprime to `2^k`), so `3^(2^k) <= u <
/// 2^53` again. Every `y` with `y·2^11` integral is refused. `y·2^11` integral also covers every
/// `|y| >= 2^40`, so `|y·ln x| < 745·2^41` (no overflow of the estimate in [`pow`]), and `|y| >=
/// 2^-200` keeps `|y·ln x| >= 2^-253`: the exp kernel's `r²·2^-53` stays above 2^-560.
#[inline]
pub fn pow_nearest(x: f64, y: f64) -> Option<f64> {
    if !log_domain(x) || !(f64::from_bits(0x3370_0000_0000_0000)..).contains(&dd::abs(y)) || integral(y * 2048.0) {
        return None; // 2^-200; a NaN y fails the comparison
    }
    // A subnormal or zero result is left to the full path: its final scaling in `tiny` is exact,
    // so hardware raises no underflow although the result is tiny and inexact.
    pow(x, y, false, Rounding::Nearest).map(|(v, _)| v).filter(|v| *v >= f64::MIN_POSITIVE)
}

/// 2^-48: the bound on the relative error of the `double` series tail in [`log_pow`], doubled.
const LTAIL: f64 = f64::from_bits(0x3cf0_0000_0000_0000);
/// 2^-99 and 2^-100: the bounds for the double-double steps of [`log_pow`] and [`pow`].
const DD99: f64 = f64::from_bits(0x39c0_0000_0000_0000);
const DD100: f64 = f64::from_bits(0x39b0_0000_0000_0000);
/// 1/3 as a double-double (the error below 2^-110).
const THIRD: Dd = Dd { hi: f64::from_bits(0x3fd5_5555_5555_5555), lo: f64::from_bits(0x3c75_5555_5555_5555) };

/// `ln x` as `(y, el)` with `|ln x − y| <= el` (absolute), for a positive finite `x != 1`, about
/// 2^-73 relative: [`log_kernel`] with the `r³/3` term carried in double-double too, for [`pow`],
/// whose product `y·ln x` multiplies the logarithm's error by `|y|`.
///
/// Proof. `r = rh + rl = m·c − 1` exactly, `|rl| <= 2^-53·|rh|`, `|rh| <= 2^-7` (checked).
/// `ln(1 + r) = ln(1 + rh) + rl·(1 − rh + rh² − …) − rl²/(2(1 + rh)²)`, and
/// `ln(1 + rh) = rh − rh²/2 + rh³/3 + T + δ` with `T = rh⁴·(−1/4 + rh/5 − … − rh⁷/11)` and
/// `|δ| <= rh¹²/12·1.01 <= 2^-56·|T|` (`|T| >= 0.24·rh⁴`). Then:
/// * `rh − rh²/2`: `two_prod` and `two_sum`, exact as a double-double.
/// * `rh³/3`: the exact `rh²` times `rh` (`Dd::mul_f64`) times [`THIRD`] (`Dd::mul`), below
///   `2^-100·|rh|³`.
/// * `T` in `double`: the coefficients' and Horner's roundings (`|rh·…| <= 2^-7·0.21` against
///   `1/4`, at most `4u` relative), `rh⁴ = s.hi²` (`2u` from `s.hi` plus `u`), the product
///   (`u`): `< 2^-50·|T|`.
/// * `rl` enters as `rl·(1 − rh + rh²)`: the dropped `rl·rh³·1.01 < 2^-50.9·|T|` and
///   `rl²/2·1.01 < 2^-107·|rh|`; its rounding `< 2^-106·|rh|`.
/// * The small sum `(−s.lo/2 + rl·(…)) + T`: two roundings, `< 2^-52·|T| + 2^-104·|rh|`.
/// * Adding `rh³/3` and the small sum to the exact `rh − rh²/2` (`Dd::add`, `Dd::add_f64`):
///   `< 2^-103·|rh|` (`|lr| <= 1.01·|rh|`).
///
/// So `|ln(1 + r) − lr| <= (2^-50 + 2^-50.9 + 2^-52 + 2^-56)·|T| + 2^-100·|rh|
/// <= 2^-48·|T| + 2^-100·|rh|`. The assembly `e·ln2 + (−ln c) + lr` is exact when `e = 0` and
/// `c = 1` (both other terms are 0); otherwise `e·ln2` (`two_prod` of the stored `LN2.hi`, the
/// trailing parts below `|e|·2^-106`), `−ln c` (stored, `< 2^-104`) and the two `Dd::add`s
/// (`< 2^-104` of the operands' magnitudes, all below `|y| + 1`) cost `< 2^-99·(|y| + 1)`.
fn log_pow(x: f64) -> Option<(Dd, f64)> {
    let (e, m) = crate::exp_log::decompose(x);
    let i = dd::to_i32(dd::nearest_int((m - 1.0) * 128.0)).clamp(-37, 53);
    let idx = usize::try_from(i + 37).unwrap_or(37);
    let p = two_prod(m, f64::from_bits(t::LOG_C[idx]));
    let r = two_sum(p.hi - 1.0, p.lo);
    let (rh, rl) = (r.hi, r.lo);
    if dd::abs(rh) > 0.007_812_5 {
        return None;
    }
    const C: [f64; 8] = [-1.0 / 4.0, 1.0 / 5.0, -1.0 / 6.0, 1.0 / 7.0, -1.0 / 8.0, 1.0 / 9.0, -1.0 / 10.0, 1.0 / 11.0];
    let q = C[0] + rh * (C[1] + rh * (C[2] + rh * (C[3] + rh * (C[4] + rh * (C[5] + rh * (C[6] + rh * C[7]))))));
    let s = two_prod(rh, rh);
    let tail = s.hi * s.hi * q;
    let a = two_sum(rh, -0.5 * s.hi);
    let cube = s.mul_f64(rh).mul(THIRD);
    let small = (rl * (1.0 - rh + s.hi) - 0.5 * s.lo) + tail;
    let lr = a.add(cube).add_f64(small);
    let mut el = LTAIL * dd::abs(tail) + DD100 * dd::abs(rh);
    let y = if e == 0 && i == 0 {
        lr
    } else {
        let y = Dd::of(t::LN2).mul_f64(f64::from(e)).add(Dd::of(t::LOG_L[idx])).add(lr);
        el += DD99 * (dd::abs(y.hi) + 1.0);
        y
    };
    Some((y, el))
}

/// `x^y = ±e^(y·ln x)` for a positive finite `x != 1` (the magnitude of the caller's `x`), a
/// finite `y`, the sign `neg` (`x < 0` and `y` an odd integer). The caller keeps out every case
/// whose result is exact (a `double` or a midpoint between two: `pow`'s exact cases), so the
/// result is inexact as [`settle`] requires. Outside `2^-900 <= |y·ln x| <= 709.7` (and when the
/// result would be the largest finite or overflow: `m > 1022` in [`settle`]) it returns `None`;
/// the accurate path then decides, overflow and underflow included.
///
/// Proof. `(l, el)` from [`log_pow`]: `|ln x − l| <= el`. The range check uses
/// `est = RN(l.hi·y)`; in range, `|y| <= 709.7/|l.hi| < 2^63` (`|ln x| >= 2^-53` for `x != 1`),
/// so the Dekker splitting in `z = l·y` (`Dd::mul_f64`) cannot overflow and, `|z| >= 2^-901`,
/// its partial products do not underflow: `|l·y − z| <= 2^-100·|z|`. So `z` is within
/// `dz = |y|·el + 2^-100·|z.hi|·1.01` of `y·ln x`. `exp`'s reduction ([`exp`], `|z| < 746`, the
/// trailing part `z.lo` added in double-double) and [`exp_kernel`] give `v·2^m` with
/// `|e^z − v·2^m| <= eps·2^m`; `|e^(y·ln x) − e^z| <= e^z·|dz|·1.01` for `|dz| <= 2^-30`
/// (checked), and `e^z·2^-m <= |v.hi|·(1 + 2^-50)`. So `eps + 1.02·|v.hi|·dz` bounds the
/// error of `v`. The sign: `x^y = −|x|^y`, and rounding `−a` in `mode` is the negation of
/// rounding `a` in the mirrored mode (up and down swapped), the flags and `errno` the same.
pub fn pow(x: f64, y: f64, neg: bool, mode: Rounding) -> Option<(f64, Status)> {
    let (l, el) = log_pow(x)?;
    let est = l.hi * y;
    if !(-745.1..=709.7).contains(&est) || dd::abs(est) < f64::from_bits(0x07b0_0000_0000_0000) {
        return None; // 2^-900
    }
    let z = l.mul_f64(y);
    let dz = dd::abs(y) * el + DD100 * 1.01 * dd::abs(z.hi);
    if dz > f64::from_bits(0x3e10_0000_0000_0000) {
        return None; // 2^-30
    }
    let kf = dd::nearest_int(z.hi * f64::from_bits(t::SCALES[0]));
    let r = crate::exp_log::reduce(z, kf, &t::LN2_64_CW);
    let (v, m, eps) = exp_kernel(r, dd::to_i32(kf))?;
    let eps = eps + 1.02 * dd::abs(v.hi) * dz;
    if !neg {
        return settle(v, eps, m, mode);
    }
    let mirrored = match mode {
        Rounding::Up => Rounding::Down,
        Rounding::Down => Rounding::Up,
        other => other,
    };
    settle(v, eps, m, mirrored).map(|(w, st)| (-w, st))
}

/// The neighbour of a nonzero finite `h` toward +∞ (`up`) or −∞.
fn step(h: f64, up: bool) -> f64 {
    let b = h.to_bits();
    f64::from_bits(if (h > 0.0) == up { b + 1 } else { b - 1 })
}

/// The correctly rounded `(y ± eps)·2^m` in `mode`, or `None` when the interval holds a rounding
/// boundary (or the result is outside the ranges handled here). `y` is normalized
/// (`y.hi = RN(y.hi + y.lo)`) and `y.hi != 0`; the true value is not representable (the callers
/// keep the exact cases out), so the result is inexact.
///
/// Proof (normal results, `y.hi·2^m` and its neighbours normal: then scaling by `2^m` is exact).
/// `e = eps + 2^-100·|y.hi|` covers the rounding of `y.lo ± e` (`< 2^-105·|y.hi|`), so
/// `y.hi + fl(y.lo − e) <= f(x) <= y.hi + fl(y.lo + e)`. Nearest: rounding is monotone, so equal
/// roundings of both ends give the rounding of `f(x)`. Directed: `y.hi` is the nearest double to
/// `y.hi + y.lo`, so `f(x)` lies strictly between `y.hi` and its neighbour on the side of `y.lo`
/// (when `y.hi` is a power of two and `y.lo < 0`, `|y.lo|` is at most a quarter of `ulp(y.hi)`);
/// when both ends lie on one side of `y.hi`, the directed rounding is `y.hi` or that neighbour.
pub(crate) fn settle(y: Dd, eps: f64, m: i32, mode: Rounding) -> Option<(f64, Status)> {
    let e = eps + dd::abs(y.hi) * SLACK;
    if m < -1022 || (m == -1022 && y.hi <= 1.0) {
        return tiny(y, e, m, mode);
    }
    if m > 1022 {
        return None;
    }
    let (dn, up) = (y.lo - e, y.lo + e);
    let v = if mode == Rounding::Nearest {
        let (a, b) = (y.hi + dn, y.hi + up);
        if a.to_bits() != b.to_bits() {
            return None;
        }
        a
    } else {
        let above = if dn > 0.0 {
            true
        } else if up < 0.0 {
            false
        } else {
            return None;
        };
        let toward_up = match mode {
            Rounding::Up => true,
            Rounding::Down => false,
            _ => y.hi < 0.0,
        };
        if toward_up == above { step(y.hi, above) } else { y.hi }
    };
    Some((v * dd::pow2(m), Status::flags(Flags::INEXACT)))
}

/// [`settle`] for a positive result with `m <= -1022` (subnormal, or in the lowest normal binade,
/// where the grid is 2^-1074 throughout; anything at or above 2^-1022 falls back). In units of 2^-1074, `z = y·2^(m+1074)` (exact scaling,
/// `m >= -1077`); `t` is the integer nearest `z.hi` (`z.hi < 2^51`), `d = (z.hi − t) + z.lo`
/// (`z.hi − t` exact: `|z.hi − t| <= 1/2`). Nearest: `|d| + e < 1/2` makes `t` the rounding.
/// Directed: `d` of one sign (with `|d| < 1`) puts `f(x)` strictly between `t` and `t ± 1`. The
/// result `n·2^-1074` is formed by two multiplications, the last one producing the subnormal (so
/// the caller's flush-to-zero, were it ever in force here, would show). The result is tiny before
/// and after rounding (`n < 2^52`), so underflow and inexact; `ERANGE` when it is 0.
fn tiny(y: Dd, e: f64, m: i32, mode: Rounding) -> Option<(f64, Status)> {
    if !(-1077..=-1022).contains(&m) || y.hi <= 0.0 {
        return None;
    }
    let s = dd::pow2(m + 1074);
    let (zh, zl, ze) = (y.hi * s, y.lo * s, e * s);
    if zh >= 2_251_799_813_685_248.0 {
        return None; // 2^51
    }
    let t = (zh + ROUND_INT) - ROUND_INT;
    let d = (zh - t) + zl;
    let (dn, up) = (d - ze, d + ze);
    let n = if mode == Rounding::Nearest {
        if dn <= -0.5 || up >= 0.5 {
            return None;
        }
        t
    } else {
        let above = if dn > 0.0 {
            true
        } else if up < 0.0 {
            false
        } else {
            return None;
        };
        if (mode == Rounding::Up) == above { t + if above { 1.0 } else { -1.0 } } else { t }
    };
    if !(0.0..4_503_599_627_370_496.0).contains(&n) {
        return None; // 2^52: a normal result
    }
    let r = n * dd::pow2(-474) * dd::pow2(-600);
    let st = Status::flags(Flags(Flags::UNDERFLOW.0 | Flags::INEXACT.0));
    Some((r, float::range_errno(r, st)))
}
