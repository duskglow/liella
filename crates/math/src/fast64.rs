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
use crate::dd::{self, fast_two_sum, two_prod, two_sum, Dd};
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

/// `e^r` times `2^(k/64)` as `(y, m, tail)`: `y·2^m` with `|e^x − y·2^m| <= eps·2^m` for
/// `eps = `[`exp_eps`]`(y, tail)`, for the reduced `r = rh + rl` (`|rh| <= 0.006`,
/// `|rl| <= 2^-52·|rh|`). The computed series tail is returned rather than `eps`, so that the
/// in-place entries, whose rounding test needs no `eps` ([`near`]), do not form it.
///
/// Proof. `e^r − 1 = r + r²/2 + r³·(1/3! + r/4! + … + r⁵/8!) + δ`, `|δ| <= |r|⁹/9!·1.01 < 2^-86`.
/// The leading `rh + rh²/2` is exact as a double-double (`two_prod`, then `two_sum`); `rl` enters
/// as `rl·(1 + rh)` (dropping `rl·rh²/2 < 2^-76`, `rl²/2 < 2^-120`); the tail
/// `T = rh³·(1/3! + …)` (`|T| < 2^-24.5`) is evaluated in `double` with at most 4 roundings and
/// the coefficients' own roundings (each below `u` relative): error `< 5u·|T| < 2^-50·|T|`. Using
/// `rh` instead of `r` in `T` costs `< |rh|²·|rl| < 2^-74`. `2^(j/64) = th + tl` is stored as a
/// double-double (error `< 2^-104`, `1 <= th < 2`, `|tl| <= 2^-53·th`).
///
/// Assembly `y = t + t·p` (MATH-M6-F64-GAP-ASSEMBLY, register 13.5 #1; Fast2Sum and Dekker's
/// product as in Muller et al., Handbook of Floating-Point Arithmetic, 2018, ch. 4): `p = ph + pl`
/// with `|ph| <= 0.00602`, `pl = a.lo + small` (`|a.lo| <= 2^-53·|ph|`, the small sum carrying `T`).
/// `q = th·ph` exactly (`two_prod`); `th + q.hi` by Fast2Sum, exact since `|q.hi| <= 0.0121 < th`;
/// `y` = Fast2Sum of that sum `b` and `lo = (b.lo + (tl + (q.lo + tl·ph))) + th·pl`, exact since
/// `|lo| < 2^-20 <= |b.hi|`. Dropped: `tl·pl < 2^-53·2·2^-52 = 2^-104`. The four roundings in
/// the first part of `lo` act on partial sums below `2^-51.9`: `< 2^-102.9`. `th·pl` and the
/// last addition each add `u` of `th·|pl| <= th·(|T| + 2^-52.4·|ph|)`: `2u·th·|T| + 2^-104`.
/// `T` itself: `5u·|T|` (above) and the two additions that carry it into `pl`, `2u·|T|`; with
/// the assembly's `2u` that is `9u·th·|T| < 2^-49.8·th·|T|`. So `|e^r·2^(j/64) − y| <=
/// 2^-49.8·th·|T| + th·2^-74 + 2^-101 <= 2^-49·|T|·|y| + 2^-70·|y|` (`|y| >= 0.994·th`; the
/// computed `|tail|` is within `5u` of `|T|`).
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
    let (ph, pl) = (a.hi, a.lo + ((0.5 * s.lo + rl * (1.0 + rh)) + tail));
    let tj = t::EXP2_J64[usize::try_from(k & 63).unwrap_or(0)];
    let (th, tl) = (f64::from_bits(tj[0]), f64::from_bits(tj[1]));
    let q = two_prod(th, ph);
    let b = fast_two_sum(th, q.hi);
    let y = fast_two_sum(b.hi, (b.lo + (tl + (q.lo + tl * ph))) + th * pl);
    Some((y, k >> 6, tail))
}

/// [`exp_kernel`]'s bound `|y|·(2^-49·|tail| + 2^-70)` (proof there).
#[inline(always)]
fn exp_eps(y: Dd, tail: f64) -> f64 {
    dd::abs(y.hi) * (TAIL * dd::abs(tail) + MARGIN)
}

/// `ln2/64` as the first three parts of `LN2_64_CW`, the last two negated: `C0` has 36 significant
/// bits (lowest `2^-42`), `NC1` 35 (lowest `2^-80`), `NC2` 34, so `k·C0`, `k·NC1`, `k·NC2` are exact
/// for `|k| < 2^17`; `|ln2/64 − C0 − C1 − C2| < 1.1·10^-37 < 2^-122.7`.
const C0: f64 = f64::from_bits(t::LN2_64_CW[0]);
const NC1: f64 = f64::from_bits(t::LN2_64_CW[1] ^ 0x8000_0000_0000_0000);
const NC2: f64 = f64::from_bits(t::LN2_64_CW[2] ^ 0x8000_0000_0000_0000);

/// The fast paths' Cody–Waite reduction (MATH-M6-F64-GAP-EXP-REDUCE, register 13.5 #2):
/// `(h, l)` with `h + l = xh − k·(C0 + C1) − k·C2` up to one rounding of `l`, for
/// `k = kf = nearest(xh·64/ln2)` and `|xh| <= 746` (`|k| <= 68,880 < 2^17`). The caller adds its
/// low part to `l` and joins the two by Fast2Sum.
///
/// Proof. If `k = 0` every step is exact (`h = xh`, `l = 0`). Otherwise `|xh| >= 0.0054 > 2^-8`, so
/// `xh` is a multiple of `2^-60`, and so is `xh − k·C0` (`k·C0` is a multiple of `2^-42`); that
/// difference is below `(1/2 + 2^-36)·ln2/64 + |k|·2^-45.1 < 0.005416 < 2^-7`, so it fits 53 bits:
/// `r0` is exact. Fast2Sum is exact when its first operand is a multiple of the second's last place
/// (Handbook of Floating-Point Arithmetic, 2018, sec. 4.3.1, the condition on the exponents):
/// `|k·C1| < 2^-29`, whose last place is at most `2^-81`, divides `2^-60`; so `s.hi + s.lo =
/// r0 − k·C1` exactly, and `s.hi` is a multiple of `2^-80` (the exact difference is). Then
/// `|s.lo| <= 2^-61` and `|k·C2| < 2^-68`: `l = RN(s.lo − k·C2)` is off by at most `2^-114`. With
/// the parts' truncation, `|k|·2^-122.7 < 2^-106.6`, `h + l` is within `2^-106.5` of `xh − k·ln2/64`.
#[inline(always)]
fn reduce_ln2(xh: f64, kf: f64) -> (f64, f64) {
    let s = fast_two_sum(xh - kf * C0, kf * NC1);
    (s.hi, s.lo + kf * NC2)
}

/// [`exp`]'s reduced argument (proof there).
#[inline(always)]
fn reduce_exp(x: f64, kf: f64) -> Dd {
    let (h, l) = reduce_ln2(x, kf);
    fast_two_sum(h, l)
}

/// [`pow`]'s reduced argument (proof there).
#[inline(always)]
fn reduce_pow(z: Dd, kf: f64) -> Dd {
    let (h, l) = reduce_ln2(z.hi, kf);
    fast_two_sum(h, l + z.lo)
}

/// `e^x` for `x` in the accurate path's finite range (`|x| >= 2^-60`, `-746 <= x <= 710`).
///
/// `r = Fast2Sum(h, l)` of [`reduce_ln2`]: exact, since `h` is a multiple of `2^-80` and `|l| <
/// 2^-60` has its last place below `2^-112`. So `r` is normalized (`|rl| <= 2^-53·|rh|`) and within
/// `2^-106.5` of `x − k·ln2/64`, far inside [`exp_kernel`]'s `2^-90`; `x.lo = 0` adds nothing.
pub fn exp(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    let kf = dd::nearest_int(x * f64::from_bits(t::SCALES[0]));
    let (y, m, tail) = exp_kernel(reduce_exp(x, kf), dd::to_i32(kf))?;
    settle(y, exp_eps(y, tail), m, mode)
}

/// `ln 2` (`LN2[0]`) split by Dekker for [`exp2`]'s product: `LN2A` its upper 26 bits, `LN2B` the
/// rest (`LN2A + LN2B = LN2[0]` exactly, `LN2B` of 26 bits).
const LN2A: f64 = f64::from_bits(0x3fe6_2e43_0000_0000);
const LN2B: f64 = f64::from_bits(0xbe20_5c61_1000_0000);

/// `2^x` for `x` in the accurate path's finite range, `x` not an integer.
///
/// `r = x − k/64` is exact (`|r| <= 2^-7`). `r·ln2` as `p + q`: `p = RN(r·LN2[0])` and Dekker's
/// error term with `LN2[0]` split ahead of time (`LN2A`, `LN2B`), so `p + e = r·LN2[0]` exactly;
/// `q = e + r·LN2[1]` (`|LN2[1]| < 2^-55.2`). Not renormalized: `|e| <= 2^-53·|p|` and
/// `|r·LN2[1]| <= 2^-54.7·|p|`, so `|q| <= 2^-52.5·|p|`, which [`exp_kernel`] accepts. Error:
/// `|r|·|ln2 − LN2[0] − LN2[1]| < 2^-106·|r|`, the rounding of `r·LN2[1]` `2^-108.2·|r|`, of the
/// sum `2^-105.5·|p|`: below `2^-104·|r| < 2^-111`, inside the kernel's `2^-90`.
pub fn exp2(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    let kf = dd::nearest_int(x * 64.0);
    let (y, m, tail) = exp_kernel(ln2_times(x - kf / 64.0), dd::to_i32(kf))?; // exact difference
    settle(y, exp_eps(y, tail), m, mode)
}

/// `r·ln2` for [`exp2`] (proof there).
#[inline(always)]
fn ln2_times(r: f64) -> Dd {
    let p = r * f64::from_bits(t::LN2[0]);
    let (ra, rb) = dd::split(r);
    let e = ((ra * LN2A - p) + ra * LN2B + rb * LN2A) + rb * LN2B;
    Dd { hi: p, lo: e + r * f64::from_bits(t::LN2[1]) }
}

/// `10^x` for `x` in the accurate path's finite range, not one of its exact cases.
pub fn exp10(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    let kf = dd::nearest_int(x * f64::from_bits(t::SCALES[1]));
    let r = crate::exp_log::reduce(Dd::from(x), kf, &t::LOG10_2_64_CW);
    // (x − k·log10(2)/64)·ln10: the reduction is within 2^-95 and the product within 2^-100.
    let (y, m, tail) = exp_kernel(r.mul(Dd::of(t::LN10)), dd::to_i32(kf))?;
    settle(y, exp_eps(y, tail), m, mode)
}

/// `ln x` as `(y, tail)` for a positive finite `x != 1`: `|ln x − y| <= eps` for
/// `eps = `[`log_eps`]`(y, tail)` (the computed series tail returned, as in [`exp_kernel`]).
///
/// Proof. `x = 2^e·m`, `c ≈ 1/m` from `LOG_C` (12 bits, so `m·c` is exact as `two_prod`), and
/// `r = m·c − 1` exact as a double-double (`p.hi − 1` is exact by Sterbenz). Requiring
/// `|rh| <= 2^-7`: `ln(1 + r) = r − r²/2 + r³·(1/3 − r/4 + … − r⁷/10) + δ` with
/// `|δ| <= |r|¹¹/11·1.01 < 2^-80·|r|`. `rh − rh²/2` is exact as a double-double; `rl` enters as
/// `rl·(1 − rh + rh²)` (dropping `< 2^-74·|r|`); the tail `T` costs `< 2^-50·|T|` as in
/// [`exp_kernel`], and the one addition that carries it in [`log_assemble`] costs `u·|T|`: `6u·|T|`
/// in all. The assembly's other errors are below `2^-95·|e| + 2^-103` ([`log_assemble`]). Either
/// `e = 0` and `c = 1` (then `y = r` up to the series' errors), or `e = 0`, `|m − 1| >= 2^-8` and
/// `|y| >= 2^-8.01`, or `e != 0` and `|y| >= |e|·ln2 − ln√2 >= 0.34·|e|`: so the absolute errors
/// other than `T`'s are below `2^-72·|y|`.
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
    let y = log_assemble(e, idx, a.hi, a.lo + ((rl * (1.0 - rh + s.hi) - 0.5 * s.lo) + tail));
    Some((y, tail))
}

/// [`log_kernel`]'s bound `2^-49·|tail| + 2^-70·|y|` (proof there).
#[inline(always)]
fn log_eps(y: Dd, tail: f64) -> f64 {
    TAIL * dd::abs(tail) + MARGIN * dd::abs(y.hi)
}

/// ln 2 split for [`log_assemble`]: `LN2H` keeps 42 significant bits, so `e·LN2H` is exact for
/// `|e| < 2^11`; `LN2L = RN(ln2 − LN2H) < 2^-43`, and `|ln2 − LN2H − LN2L| < 2^-101`.
const LN2H: f64 = f64::from_bits(0x3fe6_2e42_fefa_3800);
const LN2L: f64 = f64::from_bits(0x3d2e_f357_93c7_6730);

/// `e·ln2 + (−ln c) + (lh + ll)`, the logarithms' assembly (MATH-M6-F64-GAP-ASSEMBLY, register
/// 13.5 #1), for `|e| <= 1075`, `−ln c = LOG_L[idx] = ch + cl` (`|ch| <= 0.347`, `|cl| <=
/// 2^-53·|ch|`) and `log1p r = lh + ll` (`|lh| <= 2^-7·1.01`, `|ll| <= |T| + 2^-51·|lh|`).
///
/// Proof. `H = e·LN2H` is exact; `H + ch` by Fast2Sum is exact (`|H| >= 0.693 > |ch|`, or
/// `H = 0`); `b.hi + lh` by 2Sum is exact. `y` = Fast2Sum of `u.hi` and
/// `lo = (((u.lo + b.lo) + cl) + e·LN2L) + ll`, exact since `|lo| < 2^-40·(|e| + 1) + 2^-28·|lh|`
/// is below `|u.hi|` (which is `|y|` within `2^-40`, and `|y|` is at least `0.34·|e|`, `2^-8.01`
/// or `|lh|·(1 − 2^-28)` by the cases of [`log_kernel`]). Errors, with `S = |H| + |ch| + |lh|`:
/// `e·(ln2 − LN2H − LN2L) < |e|·2^-101`; `RN(e·LN2L)`: `|e|·2^-96.9`; the roundings of the first
/// three additions of `lo` (partial sums below `2^-52·S`, the third also `|e|·2^-43.9`):
/// `2^-103.4·S + |e|·2^-96.9`; the last addition: `u·|ll| + 2^-104.9·S`, that is `u·|T|` plus
/// `2^-103.9·|lh|`; the stored `LOG_L`: `2^-104·|ch|`. In all, `u·|T|` plus below
/// `2^-95.2·|e| + 2^-102.5·S <= 2^-95·|e| + 2^-103` (`S <= 0.694·|e| + 0.36`).
#[inline(always)]
fn log_assemble(e: i32, idx: usize, lh: f64, ll: f64) -> Dd {
    let ef = f64::from(e);
    let c = t::LOG_L[idx];
    let b = fast_two_sum(ef * LN2H, f64::from_bits(c[0]));
    let u = two_sum(b.hi, lh);
    fast_two_sum(u.hi, (((u.lo + b.lo) + f64::from_bits(c[1])) + ef * LN2L) + ll)
}

/// `ln x` for a positive finite `x != 1`.
pub fn log(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    let (y, tail) = log_kernel(x)?;
    settle(y, log_eps(y, tail), 0, mode)
}

/// `log2 x` for a positive finite `x` not a power of two: `ln x · (1/ln2)` (the product's error
/// `< 2^-100·|y|`, the bound scaled by `1/ln2 < 1.5`).
pub fn log2(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    let (l, tail) = log_kernel(x)?;
    let y = l.mul(Dd::of(t::INV_LN2));
    settle(y, 1.5 * log_eps(l, tail) + MARGIN * dd::abs(y.hi), 0, mode)
}

/// `log10 x` for a positive finite `x` not a power of ten (`1/ln10 < 0.5`).
pub fn log10(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    let (l, tail) = log_kernel(x)?;
    let y = l.mul(Dd::of(t::INV_LN10));
    settle(y, 0.5 * log_eps(l, tail) + MARGIN * dd::abs(y.hi), 0, mode)
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
//   more would separate them), so at least one of the two additions is inexact in hardware. When
//   [`near`] answers, `y.hi + w` rounded to `y.hi` with `w != 0`: that addition was inexact.

/// The relative bounds `rho` behind the constant rounding test [`near`] (register 13.5 #5): every
/// `eps/|y.hi|` where the test is used is at most `rho`. Each is stated with slack and checked on
/// operands by the `bounds` tests.
/// [`exp`] and [`exp2`]: `eps/|y.hi| = 2^-49·|tail| + 2^-70` ([`exp_eps`]). For `|rh| <= 0.006`
/// the computed `|q| <= 1/6 + 0.006/24·1.01 < 0.16692`, `s.hi <= 0.006²·(1 + u)`, so `|tail| <=
/// 0.006³·0.16692·(1 + 4u) < 2^-24.72` and `eps/|y.hi| < 2^-70·(1 + 2^-3.72) < 1.0757·2^-70`;
/// `RHO_EXP = 1.125·2^-70`.
const RHO_EXP: f64 = f64::from_bits(0x3b92_0000_0000_0000);
/// [`pow`] with `dz <= DZ_NEAR = 2^-72` (checked before the test): `eps/|v.hi| = 2^-49·|tail| +
/// 2^-70 + 1.02·dz` ([`pow_eps`]; the computed `dz` is within `4u` of its real formula) `<
/// (1.0757 + 0.2551)·2^-70`; `RHO_POW = 1.375·2^-70`.
const RHO_POW: f64 = f64::from_bits(0x3b96_0000_0000_0000);
const DZ_NEAR: f64 = f64::from_bits(0x3b70_0000_0000_0000);
/// The logarithms with `|l.hi| >= LOG_NEAR = 0.34` for `ln x = l` (every `x` outside about
/// `[0.71, 1.41]`, so every `e != 0`): `|rh| <= 2^-7` gives a computed `|q| < 0.3354`,
/// `|tail| < 2^-21·0.3354·(1 + 3u) < 2^-22.57`, so `eps/|l.hi| = 2^-49·|tail|/|l.hi| + 2^-70 <
/// 2^-71.57/0.34 + 2^-70 < 1.99·2^-70` ([`log_eps`]). `log2` and `log10` multiply `l` by `1/ln b`
/// (error `< 2^-100·|y|`); the bound `eps·(1/ln b)` keeps the ratio: `eps'/|y.hi| <
/// 1.99·2^-70·(1 + 2^-99) + 2^-99.9`. (Near 1 the ratio reaches 2^-64, so there the exact test
/// decides.) `RHO_LOG = 2^-69`.
const RHO_LOG: f64 = f64::from_bits(0x3ba0_0000_0000_0000);
const LOG_NEAR: f64 = 0.34;
/// The multipliers of [`near`]: for each `rho` above, the least `double` `C` with
/// `C·(1 − 2^54·rho) > 1 + 2^-53` (checked exactly by the `bounds` tests): `1 + 1.12502·2^-16`,
/// `1 + 1.37503·2^-16` and `1 + 2.00006·2^-16`.
const C_EXP: f64 = f64::from_bits(0x3ff0_0012_0014_4018);
const C_POW: f64 = f64::from_bits(0x3ff0_0016_001e_402b);
const C_LOG: f64 = f64::from_bits(0x3ff0_0020_0040_0081);

/// The round-to-nearest result `y.hi·2^m` when `w = RN(y.lo·c)` leaves `y.hi + w` rounding to
/// `y.hi` (`w != 0`), else `None` (one multiply, one add, one compare: de Dinechin, Lauter and
/// Muller, "Fast and correct rounding of the logarithm in double precision", 2007; the CRlibm
/// report). The caller gives `|f − (y.hi + y.lo)| <= eps <= rho·|y.hi|` with `c·(1 − 2^54·rho) >
/// 1 + u` (`u = 2^-53`), `f` not a `double` nor a midpoint, and `y.hi·2^m` normal (scaling exact).
///
/// Proof. Let `H` be the distance from `y.hi` to the midpoint on `w`'s side: `ulp(y.hi)/2`, or
/// `ulp/4` when `y.hi` is a power of two and `w` points toward zero; `H` is a power of two and
/// `|y.hi| <= 2^54·H`. If `RN(y.hi + w) = y.hi`, then `|w| <= H` (beyond the midpoint the sum
/// rounds away from `y.hi`; at it, ties to even may keep `y.hi`). The successor of `H` is
/// `H·(1 + 2u)`, so `|w| <= H` gives `|y.lo|·c <= H·(1 + u)` and `|y.lo| <= H·(1 + u)/c <
/// H·(1 − 2^54·rho)`. Then on `w`'s side `|y.lo| + eps < H − 2^54·rho·H + rho·|y.hi| <= H`, and on
/// the other side `eps < 2^-15·H` is below that side's distance (at least `H/2`): every value
/// within `eps` of `y.hi + y.lo`, `f` among them, rounds to `y.hi`. No normalization of `y` is
/// needed. With `y.lo` uniform over half an ulp the test misses with probability about `c − 1`;
/// then the exact test decides ([`finish`]).
#[inline(always)]
fn near(y: Dd, c: f64, m: i32) -> Option<f64> {
    let w = y.lo * c;
    if y.hi + w == y.hi && w != 0.0 { Some(y.hi * dd::pow2(m)) } else { None }
}

/// The in-place entries' rounding: [`near`] with `c` when the caller's bound allows it, then the
/// exact test [`settle`] with `eps()` when that misses. `st` records which answered (0 near, 1 near
/// missed, 2 near not allowed, 3 neither: the accurate path; the callers start it at 4, refused
/// before the test) for [`nearest_stage`]; the entries discard it.
#[inline(always)]
fn finish(y: Dd, c: Option<f64>, m: i32, eps: impl FnOnce() -> f64, st: &mut u8) -> Option<f64> {
    *st = 2;
    if let Some(c) = c {
        if let Some(v) = near(y, c, m) {
            *st = 0;
            return Some(v);
        }
        *st = 1;
    }
    let v = settle(y, eps(), m, Rounding::Nearest).map(|(v, _)| v);
    if v.is_none() {
        *st = 3;
    }
    v
}

/// Which step of the in-place entry `which` (0 exp, 1 exp2, 2 log, 3 log2, 4 log10, 5 pow; `y`
/// only for pow) answers: the codes of [`finish`], 4 refused before the test. For the sweep's
/// count of the constant test's miss rate (MATH-M6-F64-GAP-ROUND-TEST); not part of the ABI.
#[doc(hidden)]
pub fn nearest_stage(which: usize, x: f64, y: f64) -> u8 {
    let mut st = 4;
    let _ = match which {
        0 => exp_near(x, &mut st),
        1 => exp2_near(x, &mut st),
        2 => log_near(x, &mut st),
        3 => log2_near(x, &mut st),
        4 => log10_near(x, &mut st),
        _ => pow_near(x, y, &mut st),
    };
    st
}

/// `e^x` for the in-place entry: `2^-60 <= |x| < 708` (the result normal and finite).
#[inline]
pub fn exp_nearest(x: f64) -> Option<f64> {
    exp_near(x, &mut 0)
}

#[inline(always)]
fn exp_near(x: f64, st: &mut u8) -> Option<f64> {
    if !(crate::exp_log::TINY..708.0).contains(&dd::abs(x)) {
        return None;
    }
    let kf = dd::nearest_int(x * f64::from_bits(t::SCALES[0]));
    let (y, m, tail) = exp_kernel(reduce_exp(x, kf), dd::to_i32(kf))?;
    finish(y, Some(C_EXP), m, || exp_eps(y, tail), st)
}

/// `2^x` for the in-place entry: `2^-60 <= |x| < 1021`, `x` not an integer (exact).
#[inline]
pub fn exp2_nearest(x: f64) -> Option<f64> {
    exp2_near(x, &mut 0)
}

#[inline(always)]
fn exp2_near(x: f64, st: &mut u8) -> Option<f64> {
    if !(crate::exp_log::TINY..1021.0).contains(&dd::abs(x)) || integral(x) {
        return None;
    }
    let kf = dd::nearest_int(x * 64.0);
    let (y, m, tail) = exp_kernel(ln2_times(x - kf / 64.0), dd::to_i32(kf))?; // exact difference
    finish(y, Some(C_EXP), m, || exp_eps(y, tail), st)
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
    log_near(x, &mut 0)
}

#[inline(always)]
fn log_near(x: f64, st: &mut u8) -> Option<f64> {
    if !log_domain(x) {
        return None;
    }
    let (y, tail) = log_kernel(x)?;
    finish(y, log_c(y), 0, || log_eps(y, tail), st)
}

/// [`near`]'s multiplier for a logarithm whose `ln x` is `l` (see [`RHO_LOG`]).
#[inline(always)]
fn log_c(l: Dd) -> Option<f64> {
    (dd::abs(l.hi) >= LOG_NEAR).then_some(C_LOG)
}

/// `ln x · (1/ln b)` rounded for [`log2_near`] and [`log10_near`], with the bound of [`log2`] and
/// [`log10`] (`scale·eps + 2^-70·|y|`) for the exact test.
#[inline(always)]
fn logb_near(x: f64, inv: [u64; 2], scale: f64, st: &mut u8) -> Option<f64> {
    let (l, tail) = log_kernel(x)?;
    let y = l.mul(Dd::of(inv));
    finish(y, log_c(l), 0, || scale * log_eps(l, tail) + MARGIN * dd::abs(y.hi), st)
}

/// `log2 x` for the in-place entry: as [`log_nearest`], `x` not a power of two (exact).
#[inline]
pub fn log2_nearest(x: f64) -> Option<f64> {
    log2_near(x, &mut 0)
}

#[inline(always)]
fn log2_near(x: f64, st: &mut u8) -> Option<f64> {
    if !log_domain(x) || x.to_bits() & 0x000f_ffff_ffff_ffff == 0 {
        return None;
    }
    logb_near(x, t::INV_LN2, 1.5, st)
}

/// `log10 x` for the in-place entry: as [`log_nearest`], `x` not one of the powers of ten
/// `10^1 ..= 10^22` (exact; only integers in that range are compared).
#[inline]
pub fn log10_nearest(x: f64) -> Option<f64> {
    log10_near(x, &mut 0)
}

#[inline(always)]
fn log10_near(x: f64, st: &mut u8) -> Option<f64> {
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
    logb_near(x, t::INV_LN10, 0.5, st)
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
    pow_near(x, y, &mut 0)
}

/// [`near`] only with `dz <= DZ_NEAR` ([`RHO_POW`]) and `-1021 <= m <= 1022`, where `v.hi·2^m` is
/// normal and finite (`0.99 < |v.hi| < 2`).
#[inline(always)]
fn pow_near(x: f64, y: f64, st: &mut u8) -> Option<f64> {
    if !log_domain(x) || !(f64::from_bits(0x3370_0000_0000_0000)..).contains(&dd::abs(y)) || integral(y * 2048.0) {
        return None; // 2^-200; a NaN y fails the comparison
    }
    // A subnormal or zero result is left to the full path: its final scaling in `tiny` is exact,
    // so hardware raises no underflow although the result is tiny and inexact.
    let (v, m, tail, dz) = pow_kernel(x, y)?;
    let c = (dz <= DZ_NEAR && (-1021..=1022).contains(&m)).then_some(C_POW);
    finish(v, c, m, || pow_eps(v, tail, dz), st).filter(|v| *v >= f64::MIN_POSITIVE)
}

/// 2^-48: the bound on the relative error of the `double` series tail in [`log_pow`], doubled.
const LTAIL: f64 = f64::from_bits(0x3cf0_0000_0000_0000);
/// 2^-94 (per unit of `|e|`) and 2^-100: the bounds for the double-double steps of [`log_pow`]
/// and [`pow`].
const DD94: f64 = f64::from_bits(0x3a10_0000_0000_0000);
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
/// * `rh³/3` (register 13.5 #6): `c = s.hi·rh` exactly (`two_prod`), `rh³ = c.hi + c.lo +
///   s.lo·rh`; `g = c.hi·THIRD.hi` exactly (`two_prod`, Dekker's split of the constant folded);
///   `cube = g.hi + cube_lo`, `cube_lo = g.lo + ((c.lo + s.lo·rh)·THIRD.hi + c.hi·THIRD.lo)`.
///   Dropped `(c.lo + s.lo·rh)·THIRD.lo` and `1/3 − THIRD` (`< 2^-110`): `< 2^-105·|rh³|`; six
///   roundings on partial sums below `2^-51·|rh³|`: `< 2^-101.4·|rh³|`. So `< 2^-101·|rh|³ <=
///   2^-115·|rh|`.
/// * `T` in `double`: the coefficients' and Horner's roundings (`|rh·…| <= 2^-7·0.21` against
///   `1/4`, at most `4u` relative), `rh⁴ = s.hi²` (`2u` from `s.hi` plus `u`), the product
///   (`u`): `< 2^-50·|T|`.
/// * `rl` enters as `rl·(1 − rh + rh²)`: the dropped `rl·rh³·1.01 < 2^-50.9·|T|` and
///   `rl²/2·1.01 < 2^-107·|rh|`; its rounding `< 2^-106·|rh|`.
/// * The small sum `(−s.lo/2 + rl·(…)) + T`: two roundings, `< 2^-52·|T| + 2^-104·|rh|`.
/// * `lh + ll`: `lh = RN(a.hi + g.hi)` by Fast2Sum (exact: `|g.hi| <= 2^-15.5·|rh| <= |a.hi|`)
///   and `ll = ((h.lo + a.lo) + cube_lo) + small`: the first two roundings act on partial sums
///   below `2^-51.9·|rh|`, the last on those plus `|T|·1.01`: `< 2^-53·|T| + 2^-103·|rh|`
///   (`|ll| <= |T| + 2^-51·|rh|`).
///
/// So `|ln(1 + r) − (lh + ll)| <= (2^-50 + 2^-50.9 + 2^-52 + 2^-53 + 2^-56)·|T| + 2^-101·|rh|
/// <= 2^-49·|T| + 2^-101·|rh|`. When `e = 0` and `c = 1` the result is `lh + ll` normalized by
/// Fast2Sum (exact: `|ll| <= 2^-28·|lh|`). Otherwise [`log_assemble`] adds `u·|T|` (still
/// within `2^-48·|T|`) and below `2^-95·|e| + 2^-103`, charged as `DD94·|e| + DD100`.
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
    let c = two_prod(s.hi, rh);
    let g = two_prod(c.hi, THIRD.hi);
    let cube_lo = g.lo + ((c.lo + s.lo * rh) * THIRD.hi + c.hi * THIRD.lo);
    let h = fast_two_sum(a.hi, g.hi);
    let small = (rl * (1.0 - rh + s.hi) - 0.5 * s.lo) + tail;
    let ll = ((h.lo + a.lo) + cube_lo) + small;
    let mut el = LTAIL * dd::abs(tail) + DD100 * dd::abs(rh);
    let y = if e == 0 && i == 0 {
        fast_two_sum(h.hi, ll)
    } else {
        el += DD94 * dd::abs(f64::from(e)) + DD100;
        log_assemble(e, idx, h.hi, ll)
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
/// `dz = |y|·el + 2^-100·|z.hi|·1.01` of `y·ln x`. `exp`'s reduction ([`reduce_ln2`], `|z| < 746`, the
/// trailing part `z.lo` added to its low part, within `2^-96.9`) and [`exp_kernel`] give `v·2^m` with
/// `|e^z − v·2^m| <= eps·2^m`; `|e^(y·ln x) − e^z| <= e^z·|dz|·1.01` for `|dz| <= 2^-30`
/// (checked), and `e^z·2^-m <= |v.hi|·(1 + 2^-50)`. So `eps + 1.02·|v.hi|·dz` bounds the
/// error of `v`. The sign: `x^y = −|x|^y`, and rounding `−a` in `mode` is the negation of
/// rounding `a` in the mirrored mode (up and down swapped), the flags and `errno` the same.
pub fn pow(x: f64, y: f64, neg: bool, mode: Rounding) -> Option<(f64, Status)> {
    let (v, m, tail, dz) = pow_kernel(x, y)?;
    let eps = pow_eps(v, tail, dz);
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

/// [`pow`]'s value `v·2^m` of `|x|^y` before rounding, with [`exp_kernel`]'s tail and the bound
/// `dz` on `|z − y·ln x|` (proof at [`pow`]).
#[inline(always)]
fn pow_kernel(x: f64, y: f64) -> Option<(Dd, i32, f64, f64)> {
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
    // [`reduce_ln2`] on `z.hi`, then `z.lo` (`|z.lo| <= 2^-44`, `z` normalized and `|z.hi| < 2^10`)
    // joins the low part: that sum is below `2^-43` and rounds by at most `2^-97`; the Fast2Sum is
    // exact as in [`exp`] (`k != 0`: `h` a multiple of `2^-80`, the sum's last place at most
    // `2^-96`; `k = 0`: `h = z.hi`, `l = 0`, `|z.lo| <= |z.hi|`). `r` within `2^-96.9` of `z − k·ln2/64`.
    let r = reduce_pow(z, kf);
    let (v, m, tail) = exp_kernel(r, dd::to_i32(kf))?;
    Some((v, m, tail, dz))
}

/// [`pow`]'s bound `eps + 1.02·|v.hi|·dz` on the error of `v` (proof at [`pow`]).
#[inline(always)]
fn pow_eps(v: Dd, tail: f64, dz: f64) -> f64 {
    exp_eps(v, tail) + 1.02 * dd::abs(v.hi) * dz
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

// The kernels' error bounds checked directly against the multi-precision oracle (`crate::mp`):
// each dropped or misplaced term of an assembly is far above its bound, so these catch what the
// rounding tests would see only rarely (MATH-M6-F64-GAP-ASSEMBLY). No spec clause: internal
// bounds of the fast paths (D-10.13).
#[cfg(test)]
mod bounds {
    use super::*;
    use crate::mp::{self, Mp};

    fn operands(n: usize, lo: f64, hi: f64) -> impl Iterator<Item = f64> {
        let mut st = 0x2545_f491_4f6c_dd1du64;
        (0..n).map(move |_| {
            st ^= st << 13;
            st ^= st >> 7;
            st ^= st << 17;
            lo + (hi - lo) * (f64::from_bits(0x3ff0_0000_0000_0000 | (st >> 12)) - 1.0)
        })
    }

    fn within(exact: Mp, y: Dd, bound: f64, what: &str, x: f64) {
        let err = exact.sub(Mp::from_f64(y.hi)).sub(Mp::from_f64(y.lo));
        assert!(err.cmp_abs(Mp::from_f64(bound)).is_le(), "{what}({x:e}): error {:e} above the bound {bound:e}", err.to_f64());
    }

    #[test]
    fn log_kernels_stay_within_their_bounds() {
        let near = operands(3000, 0.95, 1.05);
        let wide = operands(3000, -700.0, 700.0).map(f64::exp2);
        for x in near.chain(wide).chain(operands(2000, 0.5, 4.0)) {
            if x == 1.0 {
                continue;
            }
            let exact = mp::ln(Mp::from_f64(x));
            if let Some((y, tail)) = log_kernel(x) {
                within(exact, y, log_eps(y, tail), "log_kernel", x);
            }
            if let Some((y, el)) = log_pow(x) {
                within(exact, y, el, "log_pow", x);
            }
        }
    }

    #[test]
    fn exp_kernel_stays_within_its_bound() {
        for x in operands(6000, -700.0, 700.0).chain(operands(2000, -0.02, 0.02)) {
            if dd::abs(x) < crate::exp_log::TINY {
                continue;
            }
            let kf = dd::nearest_int(x * f64::from_bits(t::SCALES[0]));
            if let Some((y, m, tail)) = exp_kernel(reduce_exp(x, kf), dd::to_i32(kf)) {
                let exact = mp::exp(Mp::from_f64(x)).mul(Mp::from_f64(dd::pow2(-m)));
                within(exact, y, exp_eps(y, tail), "exp_kernel", x);
            }
        }
    }

    /// `z − k·ln2/64` exactly, for the reduction tests.
    fn reduced(zh: f64, zl: f64, kf: f64) -> Mp {
        Mp::from_f64(zh).add(Mp::from_f64(zl)).sub(mp::ln2().mul_f64(kf).ldexp(-6))
    }

    /// [`reduce_ln2`] as [`exp`] and [`pow`] use it: within `2^-106` (exp) and `2^-96.9` (pow) of
    /// exact and normalized, over the whole range and next to the multiples of ln2/64 (where `r`
    /// is tiny and the Fast2Sums rest on the exponent condition, not on ordered magnitudes).
    #[test]
    fn exp_reduction_stays_within_its_bound() {
        let near = (-68_880i32..=68_880).step_by(11).flat_map(|k| {
            let c = (f64::from(k) * f64::from_bits(t::LN2[0])) / 64.0;
            [c, f64::from_bits(c.to_bits() + 1), f64::from_bits(c.to_bits().wrapping_sub(1))]
        });
        let b_exp = f64::from_bits(0x3950_0000_0000_0000); // 2^-106
        let b_pow = f64::from_bits(0x39f0_0000_0000_0000); // 2^-96
        for x in operands(20_000, -746.0, 710.0).chain(operands(4000, -0.02, 0.02)).chain(near) {
            let kf = dd::nearest_int(x * f64::from_bits(t::SCALES[0]));
            let r = reduce_exp(x, kf);
            within(reduced(x, 0.0, kf), r, b_exp, "exp reduction", x);
            assert!(dd::abs(r.lo) <= dd::abs(r.hi) * f64::EPSILON * 0.5, "exp reduction({x:e}) not normalized");
            // pow: a double-double argument with a full-size low part.
            let z = fast_two_sum(x, x * f64::from_bits(0x3ca0_0000_0000_0000) * 0.49);
            let kf = dd::nearest_int(z.hi * f64::from_bits(t::SCALES[0]));
            let r = reduce_pow(z, kf);
            within(reduced(z.hi, z.lo, kf), r, b_pow, "pow reduction", x);
            assert!(dd::abs(r.lo) <= dd::abs(r.hi) * f64::EPSILON * 0.5, "pow reduction({x:e}) not normalized");
        }
    }

    /// [`exp2`]'s `r·ln2` within `2^-104·|r|`, its low part within `2^-52` of its high part.
    #[test]
    fn near_constants_cover_their_bounds() {
        // Each multiplier is the least double with c·(1 − 2^54·rho) > 1 + 2^-53 (exact in Mp), and
        // the kernels' eps/|y.hi| stays below rho where `near` is used.
        let one_u = Mp::from_f64(1.0).add(Mp::from_f64(f64::EPSILON / 2.0));
        for (c, rho) in [(C_EXP, RHO_EXP), (C_POW, RHO_POW), (C_LOG, RHO_LOG)] {
            let k = Mp::from_f64(1.0).sub(Mp::from_f64(rho * dd::pow2(54)));
            assert!(Mp::from_f64(c).mul(k).cmp_abs(one_u).is_gt(), "{c:e} below its bound");
            assert!(Mp::from_f64(f64::from_bits(c.to_bits() - 1)).mul(k).cmp_abs(one_u).is_le(), "{c:e} not the least");
        }
        for x in operands(20000, -700.0, 700.0) {
            let kf = dd::nearest_int(x * f64::from_bits(t::SCALES[0]));
            if let Some((y, _, tail)) = exp_kernel(reduce_exp(x, kf), dd::to_i32(kf)) {
                assert!(exp_eps(y, tail) <= RHO_EXP * dd::abs(y.hi), "exp({x:e})");
            }
            if let Some((y, tail)) = log_kernel(dd::abs(x).exp2()) {
                assert!(log_c(y).is_none() || log_eps(y, tail) <= RHO_LOG * dd::abs(y.hi), "log(2^{x:e})");
            }
        }
        assert!(1.0757 * MARGIN + 1.02 * 1.0001 * DZ_NEAR <= RHO_POW);
    }

    #[test]
    fn exp2_product_stays_within_its_bound() {
        for x in operands(20_000, -1075.0, 1024.0).chain(operands(4000, -0.02, 0.02)) {
            let kf = dd::nearest_int(x * 64.0);
            let r = x - kf / 64.0;
            if r == 0.0 {
                continue;
            }
            let y = ln2_times(r);
            within(mp::ln2().mul_f64(r), y, dd::abs(r) * f64::from_bits(0x3970_0000_0000_0000), "exp2 r·ln2", x);
            assert!(dd::abs(y.lo) <= dd::abs(y.hi) * f64::from_bits(0x3cb0_0000_0000_0000), "exp2 r·ln2({x:e}) low part too large");
        }
        assert_eq!(LN2A + LN2B, f64::from_bits(t::LN2[0]));
        assert_eq!(dd::split(f64::from_bits(t::LN2[0])), (LN2A, LN2B));
    }
}
