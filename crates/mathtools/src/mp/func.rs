//! Elementary functions with proven relative error bounds.
//!
//! Every function takes the output precision `prec` and works at `w = prec + guard` bits with
//! truncating operations, each with relative error below `u = 2^(1-w)`:
//! `mulw`, `divw`, `sqrtw` round the exact result toward zero; `addw` first truncates both
//! operands at `2^(top - w - 4)` (where `top` is the larger leading-bit position), so its
//! absolute error is below `2^-(w+3) max(|a|, |b|) + u |a + b|`. The bound on each kernel is
//! argued next to it; the guard bits (`24 + 2 bitlen(prec)` plus the kernel's own) cover a
//! few times `log2` of the number of operations, so all results are within `2^-prec`.
//!
//! Results whose binary exponent would exceed `2^44` in magnitude (`exp(2^44)` and the like)
//! saturate to `2^(±2^50)` (inexact): every format rounds them exactly as it would the true
//! value, though they are not within `2^-prec` of it. Trigonometric arguments beyond
//! `2^(2^24)` (far outside every format) give NaN rather than a reduction with gigabits of pi.

use std::cmp::Ordering;

use super::consts::{ln10_w, ln2_w, pi_w, two_over_pi_w};
use super::nat::Nat;
use super::{Approx, Big, Round};

/// Arguments with `|x| >= 2^SAT_TOP` saturate the exponential-type functions.
const SAT_TOP: i64 = 44;

/// Trigonometric arguments with `|x| >= 2^(TRIG_TOP+1)` give NaN: reducing them would need
/// more than 2^24 bits of pi, and no format comes near (binary128 stops below 2^16384).
const TRIG_TOP: i64 = 1 << 24;

fn exact(v: Big) -> Approx {
    Approx { value: v, exact: true }
}

fn approx(v: Big) -> Approx {
    Approx { value: v, exact: false }
}

fn nan() -> Approx {
    exact(Big::nan())
}

fn one() -> Big {
    Big::one()
}

fn bitlen(v: u32) -> u32 {
    32 - v.leading_zeros()
}

fn isqrt32(v: u32) -> i64 {
    f64::from(v).sqrt() as i64
}

/// The working precision for a result at `prec` bits.
fn wp(prec: u32) -> u32 {
    prec.max(2).saturating_add(24 + 2 * bitlen(prec))
}

/// Inner precision of a series or iteration at `w` bits: covers `log2` of up to `2^8 w`
/// accumulated operation errors.
fn inner(w: u32) -> u32 {
    w.saturating_add(2 * bitlen(w) + 8)
}

fn tr(x: &Big, w: u32) -> Big {
    x.round(w, Round::Zero)
}

fn mulw(a: &Big, b: &Big, w: u32) -> Big {
    tr(&a.mul(b), w)
}

fn divw(a: &Big, b: &Big, w: u32) -> Big {
    a.div(b, w, Round::Zero)
}

fn divn(a: &Big, n: u64, w: u32) -> Big {
    a.div(&Big::from_u64(n), w, Round::Zero)
}

fn sqrtw(a: &Big, w: u32) -> Big {
    a.sqrt(w, Round::Zero)
}

fn addw(a: &Big, b: &Big, w: u32) -> Big {
    if a.is_zero() {
        return tr(b, w);
    }
    if b.is_zero() {
        return tr(a, w);
    }
    let (Some(ta), Some(tb)) = (a.ilogb(), b.ilogb()) else {
        return a.add(b);
    };
    let low = ta.max(tb) - i64::from(w) - 4;
    tr(&a.round_at(low, Round::Zero).0.add(&b.round_at(low, Round::Zero).0), w)
}

/// `2^(±2^50)`, standing for a result beyond every format's range.
fn saturated(huge: bool, neg: bool) -> Big {
    Big::pow2(if huge { 1 << 50 } else { -(1 << 50) }).with_sign(neg)
}

/// True if a series term no longer affects the sum at `w` bits (the callers' terms then
/// decrease at least geometrically by 1/2, so the tail is below twice this term).
fn negligible(term: &Big, sum: &Big, w: u32) -> bool {
    match (term.ilogb(), sum.ilogb()) {
        (Some(t), Some(s)) => t < s - i64::from(w) - 2,
        (None, _) => true,
        _ => false,
    }
}

// ---------------------------------------------------------------- exponential

/// `e^r - 1` for `|r| <= 1`, relative error below `2^-w`.
///
/// `y = r / 2^s` with `|y| < 2^-k`, `k = sqrt(w)`; the Taylor series of `expm1(y)` (terms
/// decrease by at least 1/2, each with relative error below `2j u`, sum `>= 0.7|y|`) has
/// relative error below `(4N + 5) u` for `N` terms; then `s` doublings
/// `E(2y) = E(y) (E(y) + 2)`, each of which at most doubles the relative error and adds `3u`
/// (for `E > -1`, `|E| <= |E + 2|`). The `s` extra bits of `wi` absorb the doublings.
fn expm1_kernel(r: &Big, w: u32) -> Big {
    let Some(t) = r.ilogb() else { return r.clone() };
    let k = isqrt32(w);
    let s = (t + k + 1).max(0) as u32;
    let wi = inner(w).saturating_add(s);
    let y = r.mul_pow2(-i64::from(s));
    let mut term = y.clone();
    let mut sum = y.clone();
    let mut j = 2u64;
    loop {
        term = divn(&mulw(&term, &y, wi), j, wi);
        if negligible(&term, &sum, wi) {
            break;
        }
        sum = addw(&sum, &term, wi);
        j += 1;
    }
    let two = Big::from_i64(2);
    for _ in 0..s {
        sum = mulw(&sum, &addw(&sum, &two, wi), wi);
    }
    sum
}

/// `e^x` for finite `|x| < 2^47`, relative error below `2^-w`.
///
/// `x = n ln2 + r` with `|r| <= 0.38` (n from a double estimate, at most one off the nearest);
/// ln 2 carries `w + 58` bits, so `r` is within `2^-(w+9)` absolute, i.e. `e^r` within
/// `2^-(w+9)` relative; `1 + expm1(r)` (at least 0.68) adds `2^-(w+4)`, and the final
/// truncation `2^-(w+3)`.
fn exp_core(x: &Big, w: u32) -> Big {
    let n = (x.to_f64() * std::f64::consts::LOG2_E).round() as i64;
    let r = if n == 0 {
        x.clone()
    } else {
        let wl = w.saturating_add(58);
        addw(x, &ln2_w(wl).mul(&Big::from_i64(-n)), wl)
    };
    let e = expm1_kernel(&r, w.saturating_add(4));
    addw(&one(), &e, w.saturating_add(4)).mul_pow2(n)
}

/// `e^x - 1` for finite nonzero `x` at `w` bits.
///
/// For `|x| >= 1`, `e^x - 1` is at least `0.63 max(e^x, 1)`, so subtracting loses under one bit.
fn expm1_core(x: &Big, w: u32) -> Big {
    let Some(t) = x.ilogb() else { return x.clone() };
    if t < 0 {
        return expm1_kernel(x, w);
    }
    if t >= SAT_TOP {
        return if x.is_neg() { Big::from_i64(-1) } else { saturated(true, false) };
    }
    addw(&exp_core(x, w.saturating_add(4)), &Big::from_i64(-1), w.saturating_add(2))
}

/// The exponential.
pub fn exp(x: &Big, prec: u32) -> Approx {
    if x.is_nan() {
        return nan();
    }
    if x.is_inf() {
        return exact(if x.is_neg() { Big::zero(false) } else { x.clone() });
    }
    match x.ilogb() {
        None => exact(one()),
        Some(t) if t >= SAT_TOP => approx(saturated(!x.is_neg(), false)),
        Some(_) => approx(exp_core(x, wp(prec))),
    }
}

/// `e^x - 1`.
pub fn expm1(x: &Big, prec: u32) -> Approx {
    if x.is_nan() {
        return nan();
    }
    if x.is_inf() {
        return exact(if x.is_neg() { Big::from_i64(-1) } else { x.clone() });
    }
    if x.is_zero() {
        return exact(x.clone());
    }
    approx(expm1_core(x, wp(prec)))
}

/// `2^x`; exact for integers.
///
/// `x = n + f` with `|f| <= 1/2` exactly; `f ln2` has relative (so absolute, as it is below
/// 0.35) error below `2^-(w+2)`, then as in `exp_core`.
pub fn exp2(x: &Big, prec: u32) -> Approx {
    if x.is_nan() {
        return nan();
    }
    if x.is_inf() {
        return exact(if x.is_neg() { Big::zero(false) } else { x.clone() });
    }
    let Some(t) = x.ilogb() else { return exact(one()) };
    if x.is_integer() && t < 62 {
        return exact(Big::pow2(x.to_i64().unwrap_or(0)));
    }
    if t >= SAT_TOP {
        return approx(saturated(!x.is_neg(), false));
    }
    let w = wp(prec);
    let n = x.round_int();
    let f = x.sub(&n);
    let v = mulw(&f, &ln2_w(w + 4), w + 4);
    let e = expm1_kernel(&v, w + 4);
    approx(addw(&one(), &e, w + 4).mul_pow2(n.to_i64().unwrap_or(0)))
}

/// `10^x`; exact for integers `0 <= x <= 50000`.
///
/// `v = x ln10` with `top(x) + 8` extra bits makes the absolute error of `v` below
/// `2^-(w+3)`, hence the relative error of `e^v` below `2^-(w+2)`, plus that of `exp_core`.
pub fn exp10(x: &Big, prec: u32) -> Approx {
    if x.is_nan() {
        return nan();
    }
    if x.is_inf() {
        return exact(if x.is_neg() { Big::zero(false) } else { x.clone() });
    }
    let Some(t) = x.ilogb() else { return exact(one()) };
    if x.is_integer() && !x.is_neg() && t < 16 {
        if let Some(n) = x.to_i64().filter(|&n| n <= 50_000) {
            return exact(Big::fin(false, Nat::from_u64(10).pow(n as u64), 0));
        }
    }
    if t >= SAT_TOP {
        return approx(saturated(!x.is_neg(), false));
    }
    let w = wp(prec);
    let wl = w + t.max(0) as u32 + 8;
    let v = mulw(x, &ln10_w(wl), wl);
    approx(exp_core(&v, w + 2))
}

// ---------------------------------------------------------------- logarithm

/// `log(1 + t)` for `-1/2 <= t <= 1`, relative error below `2^-w`.
///
/// `log(1+t) = 2 atanh(z)`, `z = t/(t+2)`, `|z| <= 1/3`; `h` halvings
/// `atanh(z) = 2 atanh(z / (1 + sqrt(1 - z^2)))` (each about `5u`, amplifying earlier errors by
/// at most 1.06) speed up the series `z + z^3/3 + ...`, whose terms all have the sign of `z`,
/// so the relative errors simply add up.
fn log1p_kernel(t: &Big, w: u32) -> Big {
    if t.is_zero() {
        return t.clone();
    }
    let wi = inner(w);
    let h_max = (isqrt32(w) / 3) as u32;
    let mut z = divw(t, &t.add(&Big::from_i64(2)), wi);
    let o = one();
    let mut h = 0u32;
    while h < h_max && z.ilogb().is_some_and(|e| e > -i64::from(h_max) - 2) {
        let d = addw(&o, &sqrtw(&addw(&o, &mulw(&z, &z, wi).neg(), wi), wi), wi);
        z = divw(&z, &d, wi);
        h += 1;
    }
    let z2 = mulw(&z, &z, wi);
    let mut p = z.clone();
    let mut sum = z.clone();
    let mut k = 1u64;
    loop {
        p = mulw(&p, &z2, wi);
        let term = divn(&p, 2 * k + 1, wi);
        if negligible(&term, &sum, wi) {
            break;
        }
        sum = addw(&sum, &term, wi);
        k += 1;
    }
    sum.mul_pow2(i64::from(h) + 1)
}

/// `log x` for finite `x > 0`, relative error below `2^-w`.
///
/// `x = 2^k y`, `y` in `[0.75, 1.5)`; for `k = 0` the kernel on `y - 1` (exact) keeps the
/// relative accuracy near 1. Otherwise `|k ln2| >= 0.69 > |log y|` (at most 0.41), so the sum
/// loses at most a factor 3.4, covered by 4 extra bits.
pub(crate) fn log_core(x: &Big, w: u32) -> Big {
    let Some(mut k) = x.ilogb() else { return Big::nan() };
    let mut y = x.mul_pow2(-k);
    if y.cmp(&Big::from_f64(1.5)) != Some(Ordering::Less) {
        y = y.mul_pow2(-1);
        k += 1;
    }
    let t = y.sub(&one());
    if k == 0 {
        return log1p_kernel(&t, w);
    }
    let kb = 64 - k.unsigned_abs().leading_zeros();
    let a = ln2_w(w + 4 + kb).mul(&Big::from_i64(k));
    addw(&a, &log1p_kernel(&t, w + 4), w + 4)
}

/// `log(1 + x)` for finite nonzero `x > -1`, relative error below `2^-w`.
///
/// Outside the kernel's range: for `x > 2^(w+4)`, `log(1+x) - log x < 1/x` is negligible
/// against `log x > 1`; otherwise `|log(1+x)| >= 0.69`, so an argument error of `2^-(w+3)`
/// relative costs only `2^-(w+2)`. For `x < -1/2`, `1 + x` is formed exactly.
fn log1p_core(x: &Big, w: u32) -> Big {
    let half = Big::from_f64(-0.5);
    if x.cmp(&half) != Some(Ordering::Less) && x.cmp(&one()) != Some(Ordering::Greater) {
        return log1p_kernel(x, w);
    }
    if x.is_neg() {
        return log_core(&x.add(&one()), w + 2);
    }
    if x.ilogb().is_some_and(|t| t > i64::from(w) + 4) {
        return log_core(x, w + 2);
    }
    log_core(&addw(x, &one(), w + 4), w + 2)
}

/// Shared special cases of the logarithms: `Some` for NaN, negative, zero, infinity and 1.
fn log_special(x: &Big) -> Option<Approx> {
    if x.is_nan() || (x.is_neg() && !x.is_zero()) {
        return Some(nan());
    }
    if x.is_zero() {
        return Some(exact(Big::inf(true)));
    }
    if x.is_inf() {
        return Some(exact(x.clone()));
    }
    if x.cmp(&one()) == Some(Ordering::Equal) {
        return Some(exact(Big::zero(false)));
    }
    None
}

/// The natural logarithm.
pub fn log(x: &Big, prec: u32) -> Approx {
    if let Some(a) = log_special(x) {
        return a;
    }
    approx(log_core(x, wp(prec)))
}

/// `log(1 + x)`.
pub fn log1p(x: &Big, prec: u32) -> Approx {
    if x.is_nan() {
        return nan();
    }
    if x.is_zero() || (x.is_inf() && !x.is_neg()) {
        return exact(x.clone());
    }
    match x.cmp(&Big::from_i64(-1)) {
        Some(Ordering::Equal) => exact(Big::inf(true)),
        Some(Ordering::Less) => nan(),
        _ => approx(log1p_core(x, wp(prec))),
    }
}

/// Base-2 logarithm; exact for powers of two.
pub fn log2(x: &Big, prec: u32) -> Approx {
    if let Some(a) = log_special(x) {
        return a;
    }
    if x.man.is_one() {
        return exact(Big::from_i64(x.exp));
    }
    let w = wp(prec);
    approx(divw(&log_core(x, w + 4), &ln2_w(w + 4), w + 2))
}

/// Base-10 logarithm; exact for the powers of ten `10^n`, `n >= 0` (the only dyadic ones).
pub fn log10(x: &Big, prec: u32) -> Approx {
    if let Some(a) = log_special(x) {
        return a;
    }
    // 10^n = 5^n 2^n: odd part 5^n, exponent n.
    if (1..=100_000).contains(&x.exp) && x.man.bit_len() <= 3 * x.exp as u64 && x.man == Nat::from_u64(5).pow(x.exp as u64) {
        return exact(Big::from_i64(x.exp));
    }
    let w = wp(prec);
    approx(divw(&log_core(x, w + 4), &ln10_w(w + 4), w + 2))
}

// ---------------------------------------------------------------- trigonometric

/// `x` modulo 4 for an integer.
fn quadrant(n: &Big) -> u8 {
    if !n.is_fin() || n.exp >= 2 {
        return 0;
    }
    let low = (u8::from(n.man.bit(0)) | (u8::from(n.man.bit(1)) << 1)) << n.exp;
    let low = low & 3;
    if n.neg { (4 - low) & 3 } else { low }
}

/// `x = q pi/2 + r` with `|r| <= pi/4` (up to rounding) and `r` within `2^-(w+2)` relative.
///
/// `y = x * 2/pi` with 2/pi to `top(x) + wf + 16` bits is within `2^-(wf+15)` absolute; with
/// `n` the nearest integer and `f = y - n`, the relative error of `f` is below `2^-(w+31)` once
/// `|f| >= 2^-(wf-w-16)`. Otherwise `x` is very close to a multiple of pi/2 and `wf` doubles; this
/// ends because `x` is rational and pi is not.
fn reduce(x: &Big, w: u32) -> (u8, Big) {
    if x.cmp_abs(&Big::from_f64(0.78)) != Ordering::Greater {
        return (0, x.clone());
    }
    let e = x.ilogb().unwrap_or(0);
    let mut wf = i64::from(w) + 24;
    loop {
        let fbits = (e + wf + 16).clamp(64, i64::from(u32::MAX >> 2)) as u32;
        let y = x.mul(&two_over_pi_w(fbits));
        let n = y.round_int();
        let f = y.sub(&n);
        let enough = f.ilogb().is_some_and(|t| t >= -(wf - i64::from(w) - 16));
        if enough || wf > 1 << 26 {
            let r = mulw(&f, &pi_w(w + 4).mul_pow2(-1), w + 4);
            return (quadrant(&n), r);
        }
        wf *= 2;
    }
}

/// `sin r` for `|r| <= 0.8`, relative error below `2^-w`: alternating Taylor series with
/// decreasing terms; the sum is at least `0.89 |r|` and the terms sum to at most `1.11 |r|`.
fn sin_series(r: &Big, w: u32) -> Big {
    if r.is_zero() {
        return r.clone();
    }
    let wi = inner(w);
    let r2 = mulw(r, r, wi);
    let mut term = r.clone();
    let mut sum = r.clone();
    let mut j = 1u64;
    loop {
        term = divn(&mulw(&term, &r2, wi), (2 * j) * (2 * j + 1), wi).neg();
        if negligible(&term, &sum, wi) {
            break;
        }
        sum = addw(&sum, &term, wi);
        j += 1;
    }
    sum
}

/// `cos r` for `|r| <= 0.8`, relative error below `2^-w` (the sum is at least 0.69).
fn cos_series(r: &Big, w: u32) -> Big {
    if r.is_zero() {
        return one();
    }
    let wi = inner(w);
    let r2 = mulw(r, r, wi);
    let mut term = one();
    let mut sum = one();
    let mut j = 1u64;
    loop {
        term = divn(&mulw(&term, &r2, wi), (2 * j - 1) * (2 * j), wi).neg();
        if negligible(&term, &sum, wi) {
            break;
        }
        sum = addw(&sum, &term, wi);
        j += 1;
    }
    sum
}

/// `sin` and `cos` after reduction: an error `d` in `r` changes `sin r` by at most `d` relative
/// (`|r cot r| <= 1`) and `cos r` by at most `0.8 d` (`|r tan r| <= 0.8`).
fn sin_cos_reduced(x: &Big, w: u32, cosine: bool) -> Big {
    let (q, r) = reduce(x, w + 4);
    let q = if cosine { (q + 1) & 3 } else { q };
    match q {
        0 => sin_series(&r, w + 2),
        1 => cos_series(&r, w + 2),
        2 => sin_series(&r, w + 2).neg(),
        _ => cos_series(&r, w + 2).neg(),
    }
}

/// The sine; arguments of any size are reduced with as many bits of pi as needed.
pub fn sin(x: &Big, prec: u32) -> Approx {
    if x.is_nan() || x.is_inf() || x.ilogb().is_some_and(|t| t > TRIG_TOP) {
        return nan();
    }
    if x.is_zero() {
        return exact(x.clone());
    }
    approx(sin_cos_reduced(x, wp(prec), false))
}

/// The cosine.
pub fn cos(x: &Big, prec: u32) -> Approx {
    if x.is_nan() || x.is_inf() || x.ilogb().is_some_and(|t| t > TRIG_TOP) {
        return nan();
    }
    if x.is_zero() {
        return exact(one());
    }
    approx(sin_cos_reduced(x, wp(prec), true))
}

/// The tangent: `sin r / cos r`, or `-cos r / sin r` in odd quadrants.
pub fn tan(x: &Big, prec: u32) -> Approx {
    if x.is_nan() || x.is_inf() || x.ilogb().is_some_and(|t| t > TRIG_TOP) {
        return nan();
    }
    if x.is_zero() {
        return exact(x.clone());
    }
    let w = wp(prec);
    let (q, r) = reduce(x, w + 6);
    let (s, c) = (sin_series(&r, w + 4), cos_series(&r, w + 4));
    approx(if q & 1 == 0 { divw(&s, &c, w + 2) } else { divw(&c, &s, w + 2).neg() })
}

/// `atan a` for `0 < a <= 1`, relative error below `2^-w`.
///
/// Halvings `atan a = 2 atan(a / (1 + sqrt(1 + a^2)))` (each about `5u`; the map does not
/// amplify relative errors, its logarithmic derivative being in (0, 1]) bring `a` below
/// `2^-k`; then the alternating series `a - a^3/3 + ...` (sum at least `0.94 a`).
fn atan_small(a: &Big, w: u32) -> Big {
    let wi = inner(w);
    let k = (isqrt32(w) / 3).max(2);
    let o = one();
    let mut a = a.clone();
    let mut h = 0i64;
    while h < 64 && a.ilogb().is_some_and(|t| t > -k) {
        let d = addw(&o, &sqrtw(&addw(&o, &mulw(&a, &a, wi), wi), wi), wi);
        a = divw(&a, &d, wi);
        h += 1;
    }
    let a2 = mulw(&a, &a, wi);
    let mut p = a.clone();
    let mut sum = a.clone();
    let mut j = 1u64;
    loop {
        p = mulw(&p, &a2, wi).neg();
        let term = divn(&p, 2 * j + 1, wi);
        if negligible(&term, &sum, wi) {
            break;
        }
        sum = addw(&sum, &term, wi);
        j += 1;
    }
    sum.mul_pow2(h)
}

/// `atan x` for finite nonzero `x`, relative error below `2^-w`. For `|x| > 1`,
/// `pi/2 - atan(1/|x|)` is at least pi/4, so the subtraction loses at most one bit.
fn atan_core(x: &Big, w: u32) -> Big {
    let a = x.abs();
    let v = match a.cmp(&one()) {
        Some(Ordering::Less) => atan_small(&a, w),
        Some(Ordering::Equal) => pi_w(w + 2).mul_pow2(-2),
        _ => {
            let t = atan_small(&divw(&one(), &a, w + 4), w + 4);
            addw(&pi_w(w + 4).mul_pow2(-1), &t.neg(), w + 4)
        }
    };
    v.with_sign(x.is_neg())
}

/// The arc tangent.
pub fn atan(x: &Big, prec: u32) -> Approx {
    if x.is_nan() {
        return nan();
    }
    if x.is_zero() {
        return exact(x.clone());
    }
    if x.is_inf() {
        return approx(pi_w(wp(prec)).mul_pow2(-1).with_sign(x.is_neg()));
    }
    approx(atan_core(x, wp(prec)))
}

/// The arc sine: `atan(x / sqrt((1-x)(1+x)))`, with `1 ± x` exact.
pub fn asin(x: &Big, prec: u32) -> Approx {
    if x.is_nan() {
        return nan();
    }
    if x.is_zero() {
        return exact(x.clone());
    }
    let w = wp(prec);
    match x.cmp_abs(&one()) {
        Ordering::Greater => nan(),
        Ordering::Equal => approx(pi_w(w).mul_pow2(-1).with_sign(x.is_neg())),
        Ordering::Less => {
            let wi = w + 8;
            let a = x.abs();
            let d = mulw(&one().sub(&a), &one().add(&a), wi);
            let t = divw(&a, &sqrtw(&d, wi), wi);
            approx(atan_core(&t, w + 4).with_sign(x.is_neg()))
        }
    }
}

/// The arc cosine: `2 atan(sqrt((1-x)/(1+x)))`, with `1 ± x` exact (no cancellation anywhere).
pub fn acos(x: &Big, prec: u32) -> Approx {
    if x.is_nan() {
        return nan();
    }
    let w = wp(prec);
    if x.cmp_abs(&one()) == Ordering::Greater {
        return nan();
    }
    if x.cmp(&one()) == Some(Ordering::Equal) {
        return exact(Big::zero(false));
    }
    if x.cmp(&Big::from_i64(-1)) == Some(Ordering::Equal) {
        return approx(pi_w(w));
    }
    if x.is_zero() {
        return approx(pi_w(w).mul_pow2(-1));
    }
    let wi = w + 8;
    let t = divw(&one().sub(x), &one().add(x), wi);
    approx(atan_core(&sqrtw(&t, wi), w + 4).mul_pow2(1))
}

/// `atan2(y, x)` with the C Annex F special cases. For `x < 0` the result is
/// `±(pi - atan|y/x|)`, at least pi/2 in magnitude, so nothing cancels.
pub fn atan2(y: &Big, x: &Big, prec: u32) -> Approx {
    if y.is_nan() || x.is_nan() {
        return nan();
    }
    let w = wp(prec);
    let yneg = y.is_neg();
    let pi = |w: u32| pi_w(w);
    if y.is_zero() {
        return if x.is_neg() { approx(pi(w).with_sign(yneg)) } else { exact(y.clone()) };
    }
    if y.is_inf() {
        let v = if !x.is_inf() {
            pi(w).mul_pow2(-1)
        } else if x.is_neg() {
            pi(w + 2).mul(&Big::from_i64(3)).mul_pow2(-2)
        } else {
            pi(w).mul_pow2(-2)
        };
        return approx(v.with_sign(yneg));
    }
    if x.is_zero() {
        return approx(pi(w).mul_pow2(-1).with_sign(yneg));
    }
    if x.is_inf() {
        return if x.is_neg() { approx(pi(w).with_sign(yneg)) } else { exact(Big::zero(yneg)) };
    }
    let t = divw(&y.abs(), &x.abs(), w + 6);
    if !x.is_neg() {
        approx(atan_core(&t, w + 4).with_sign(yneg))
    } else {
        approx(addw(&pi(w + 6), &atan_core(&t, w + 6).neg(), w + 4).with_sign(yneg))
    }
}

// ---------------------------------------------------------------- hyperbolic

/// The hyperbolic sine: `(E + E/(E+1)) / 2` with `E = expm1|x|` (positive terms only).
pub fn sinh(x: &Big, prec: u32) -> Approx {
    if x.is_nan() {
        return nan();
    }
    if x.is_inf() || x.is_zero() {
        return exact(x.clone());
    }
    let a = x.abs();
    if a.ilogb().is_some_and(|t| t >= SAT_TOP) {
        return approx(saturated(true, x.is_neg()));
    }
    let w = wp(prec) + 4;
    let e = expm1_core(&a, w);
    let v = addw(&e, &divw(&e, &addw(&e, &one(), w), w), w).mul_pow2(-1);
    approx(v.with_sign(x.is_neg()))
}

/// The hyperbolic cosine: `(e^|x| + e^-|x|) / 2`.
pub fn cosh(x: &Big, prec: u32) -> Approx {
    if x.is_nan() {
        return nan();
    }
    if x.is_inf() {
        return exact(Big::inf(false));
    }
    if x.is_zero() {
        return exact(one());
    }
    let a = x.abs();
    if a.ilogb().is_some_and(|t| t >= SAT_TOP) {
        return approx(saturated(true, false));
    }
    let w = wp(prec) + 4;
    let e = exp_core(&a, w);
    approx(addw(&e, &divw(&one(), &e, w), w).mul_pow2(-1))
}

/// The hyperbolic tangent: `E / (E + 2)` with `E = expm1(2|x|)`.
pub fn tanh(x: &Big, prec: u32) -> Approx {
    if x.is_nan() {
        return nan();
    }
    if x.is_inf() {
        return exact(one().with_sign(x.is_neg()));
    }
    if x.is_zero() {
        return exact(x.clone());
    }
    let a2 = x.abs().mul_pow2(1);
    if a2.ilogb().is_some_and(|t| t >= SAT_TOP) {
        return approx(one().with_sign(x.is_neg()));
    }
    let w = wp(prec) + 4;
    let e = expm1_core(&a2, w);
    approx(divw(&e, &addw(&e, &Big::from_i64(2), w), w - 2).with_sign(x.is_neg()))
}

/// The inverse hyperbolic sine: `log1p(a + a^2 / (1 + sqrt(1 + a^2)))`, `a = |x|`, or
/// `log(2a)` once `a^2 > 2^(w+8)` (the difference is below `1/(4a^2)`).
pub fn asinh(x: &Big, prec: u32) -> Approx {
    if x.is_nan() {
        return nan();
    }
    if x.is_inf() || x.is_zero() {
        return exact(x.clone());
    }
    let w = wp(prec);
    let a = x.abs();
    let v = if a.ilogb().is_some_and(|t| t > i64::from(w / 2) + 4) {
        log_core(&a.mul_pow2(1), w + 2)
    } else {
        let wi = w + 6;
        let a2 = mulw(&a, &a, wi);
        let d = addw(&one(), &sqrtw(&addw(&one(), &a2, wi), wi), wi);
        log1p_core(&addw(&a, &divw(&a2, &d, wi), wi), w + 2)
    };
    approx(v.with_sign(x.is_neg()))
}

/// The inverse hyperbolic cosine: `log1p(t + sqrt(t (t + 2)))` with `t = x - 1` exact, or
/// `log(2x)` for huge `x`.
pub fn acosh(x: &Big, prec: u32) -> Approx {
    if x.is_nan() {
        return nan();
    }
    match x.cmp(&one()) {
        Some(Ordering::Less) => return nan(),
        Some(Ordering::Equal) => return exact(Big::zero(false)),
        _ => {}
    }
    if x.is_inf() {
        return exact(x.clone());
    }
    let w = wp(prec);
    if x.ilogb().is_some_and(|t| t > i64::from(w / 2) + 4) {
        return approx(log_core(&x.mul_pow2(1), w + 2));
    }
    let wi = w + 6;
    let t = x.sub(&one());
    let s = sqrtw(&mulw(&t, &addw(&t, &Big::from_i64(2), wi), wi), wi);
    approx(log1p_core(&addw(&t, &s, wi), w + 2))
}

/// The inverse hyperbolic tangent: `log1p(2a / (1 - a)) / 2` with `1 - a` exact.
pub fn atanh(x: &Big, prec: u32) -> Approx {
    if x.is_nan() {
        return nan();
    }
    if x.is_zero() {
        return exact(x.clone());
    }
    match x.cmp_abs(&one()) {
        Ordering::Greater => nan(),
        Ordering::Equal => exact(Big::inf(x.is_neg())),
        Ordering::Less => {
            let w = wp(prec);
            let a = x.abs();
            let t = divw(&a.mul_pow2(1), &one().sub(&a), w + 6);
            approx(log1p_core(&t, w + 2).mul_pow2(-1).with_sign(x.is_neg()))
        }
    }
}

// ---------------------------------------------------------------- power and roots

/// `|x|^y` exactly when it is a dyadic rational of moderate size (finite nonzero `x != 1`,
/// finite nonzero `y`). A non-dyadic power (a negative power of an odd number, a root that is
/// not a perfect power) is never representable nor a rounding boundary, so Ziv decides it.
fn pow_exact(ax: &Big, y: &Big, prec: u32) -> Option<Big> {
    let limit = (64 * u128::from(prec)).max(1 << 16);
    let (m, e) = (&ax.man, ax.exp);
    if y.is_integer() {
        let yi = y.to_i64()?;
        if m.is_one() {
            let ey = i128::from(e) * i128::from(yi);
            return (ey.abs() < 1 << 62).then(|| Big::pow2(ey as i64));
        }
        if yi < 0 || yi as u128 * u128::from(m.bit_len()) > limit {
            return None;
        }
        return Some(Big::fin(false, m.pow(yi as u64), e.checked_mul(yi)?));
    }
    // y = k / 2^j with k odd.
    let j = -y.exp;
    if j > 60 {
        return None;
    }
    let k = i64::try_from(y.man.to_u128()?).ok()?;
    let k = if y.is_neg() { -k } else { k };
    if e.rem_euclid(1 << j) != 0 {
        return None;
    }
    let mut r = m.clone();
    for _ in 0..j {
        let s = r.isqrt();
        if s.mul(&s) != r {
            return None;
        }
        r = s;
    }
    let eroot = e >> j;
    if k < 0 && !r.is_one() {
        return None;
    }
    if u128::from(k.unsigned_abs()) * u128::from(r.bit_len()) > limit {
        return None;
    }
    Some(Big::fin(false, r.pow(k.unsigned_abs()), eroot.checked_mul(k)?))
}

/// `|x|^y = e^(y log|x|)` for finite positive `x != 1`, finite nonzero `y`.
///
/// The exponent `v` needs absolute accuracy `2^-(w+5)`, so `log|x|` carries `top(v) + 8` extra
/// bits (`top(v)` from a 24-bit first estimate, which is within one of the truth).
fn pow_core(ax: &Big, y: &Big, w: u32) -> Big {
    let v0 = y.mul(&log_core(ax, 24));
    let vt = v0.ilogb().unwrap_or(0);
    if vt > SAT_TOP {
        return saturated(!v0.is_neg(), false);
    }
    let wl = w + 8 + (vt + 1).max(0) as u32;
    let v = mulw(y, &log_core(ax, wl), wl);
    exp_core(&v, w + 2)
}

/// `x^y` with the C Annex F special cases; exact whenever the result is a dyadic rational
/// (`pow(4, 0.5) = 2`, `pow(2, 10)`, `pow(0x1p-6, -1.5) = 2^9`, ...).
pub fn pow(x: &Big, y: &Big, prec: u32) -> Approx {
    if y.is_zero() || x.cmp(&one()) == Some(Ordering::Equal) {
        return exact(one());
    }
    if x.is_nan() || y.is_nan() {
        return nan();
    }
    let y_odd = y.is_odd_integer();
    if x.is_zero() {
        return exact(if y.is_neg() { Big::inf(x.is_neg() && y_odd) } else { Big::zero(x.is_neg() && y_odd) });
    }
    if y.is_inf() {
        return exact(match x.cmp_abs(&one()) {
            Ordering::Equal => one(),
            Ordering::Less => if y.is_neg() { Big::inf(false) } else { Big::zero(false) },
            Ordering::Greater => if y.is_neg() { Big::zero(false) } else { Big::inf(false) },
        });
    }
    if x.is_inf() {
        let neg = x.is_neg() && y_odd;
        return exact(if y.is_neg() { Big::zero(neg) } else { Big::inf(neg) });
    }
    if x.is_neg() && !y.is_integer() {
        return nan();
    }
    let neg = x.is_neg() && y_odd;
    let ax = x.abs();
    if ax.cmp(&one()) == Some(Ordering::Equal) {
        // (−1)^y for an integer y of any size.
        return exact(one().with_sign(neg));
    }
    if let Some(v) = pow_exact(&ax, y, prec) {
        return exact(v.with_sign(neg));
    }
    approx(pow_core(&ax, y, wp(prec)).with_sign(neg))
}

/// The cube root, by the integer cube root of the significand scaled to `3 (prec + 3)` bits
/// (truncation error below `2^-(prec+2)` relative); exact for perfect cubes.
pub fn cbrt(x: &Big, prec: u32) -> Approx {
    if x.is_nan() {
        return nan();
    }
    if x.is_inf() || x.is_zero() {
        return exact(x.clone());
    }
    let p = u64::from(prec.max(1));
    let mut s = (3 * p + 9).saturating_sub(x.man.bit_len());
    while (x.exp - s as i64).rem_euclid(3) != 0 {
        s += 1;
    }
    let m = x.man.shl(s);
    let root = m.icbrt();
    let is_exact = root.pow(3) == m;
    let v = Big::fin(x.is_neg(), root, (x.exp - s as i64) / 3);
    Approx { value: v, exact: is_exact }
}

/// `sqrt(x^2 + y^2)` with the Annex F special cases (an infinity wins over NaN); exact when
/// `x^2 + y^2` is the square of a dyadic rational.
pub fn hypot(x: &Big, y: &Big, prec: u32) -> Approx {
    if x.is_inf() || y.is_inf() {
        return exact(Big::inf(false));
    }
    if x.is_nan() || y.is_nan() {
        return nan();
    }
    if x.is_zero() {
        return exact(y.abs());
    }
    if y.is_zero() {
        return exact(x.abs());
    }
    let s = x.mul(x).add(&y.mul(y));
    let (v, is_exact) = s.sqrt_exact(prec);
    Approx { value: v, exact: is_exact }
}

/// The error function: `erf x = 2x/sqrt(pi) e^(-x^2) sum (2x^2)^n / (1 3 5 ... (2n+1))`, a series
/// of positive terms (no cancellation; about `2 x^2 + prec` terms). Once
/// `erfc|x| < e^(-x^2) / 3.5 < 2^-(prec+2)`, ±1 itself is within the bound.
pub fn erf(x: &Big, prec: u32) -> Approx {
    if x.is_nan() {
        return nan();
    }
    if x.is_inf() {
        return exact(one().with_sign(x.is_neg()));
    }
    if x.is_zero() {
        return exact(x.clone());
    }
    let a = x.abs();
    let a2 = a.mul(&a);
    if a.ilogb().is_some_and(|t| t >= 1) && a2.to_f64() > (f64::from(prec) + 2.0) * std::f64::consts::LN_2 {
        return approx(one().with_sign(x.is_neg()));
    }
    let w = wp(prec);
    let wi = inner(w + 4);
    let t2 = a2.mul_pow2(1);
    let ratio_cap = 2.0 * t2.to_f64();
    let mut term = one();
    let mut sum = one();
    let mut n = 1u64;
    loop {
        term = divn(&mulw(&term, &t2, wi), 2 * n + 1, wi);
        if (2 * n + 1) as f64 > ratio_cap && negligible(&term, &sum, wi) {
            break;
        }
        sum = addw(&sum, &term, wi);
        n += 1;
    }
    let e = exp_core(&tr(&a2, wi).neg(), wi);
    let c = divw(&a.mul_pow2(1), &sqrtw(&pi_w(wi), wi), wi);
    approx(mulw(&mulw(&c, &e, wi), &sum, w).with_sign(x.is_neg()))
}

// ---------------------------------------------------------------- special functions (milestone 3)

/// `erf a` for finite `a > 0` at `w` bits by the positive series (the `erf` kernel without its
/// shortcut to ±1): relative error below `2^-w`.
fn erf_value(a: &Big, w: u32) -> Big {
    let wi = inner(w + 4);
    let a2 = a.mul(a);
    let t2 = a2.mul_pow2(1);
    let ratio_cap = 2.0 * t2.to_f64();
    let mut term = one();
    let mut sum = one();
    let mut n = 1u64;
    loop {
        term = divn(&mulw(&term, &t2, wi), 2 * n + 1, wi);
        if (2 * n + 1) as f64 > ratio_cap && negligible(&term, &sum, wi) {
            break;
        }
        sum = addw(&sum, &term, wi);
        n += 1;
    }
    let e = exp_core(&tr(&a2, wi).neg(), wi);
    let c = divw(&a.mul_pow2(1), &sqrtw(&pi_w(wi), wi), wi);
    mulw(&mulw(&c, &e, wi), &sum, w)
}

/// The complementary error function: `1 + erf |x|` for `x < 0`; `1 − erf x` for `x > 0` with
/// the precision raised by the bits the subtraction cancels (`erfc x > e^(−x²)/(2x√π)` for
/// `x >= 1`, so at most `1.45 x² + log2(4x) + 1` bits). Beyond `x = 110` the value is below every
/// format's smallest subnormal and saturates to `2^-(2^50)`.
pub fn erfc(x: &Big, prec: u32) -> Approx {
    if x.is_nan() {
        return nan();
    }
    if x.is_inf() {
        return exact(if x.is_neg() { Big::from_i64(2) } else { Big::zero(false) });
    }
    if x.is_zero() {
        return exact(one());
    }
    let w = wp(prec);
    let a = x.abs();
    if x.is_neg() {
        return approx(addw(&one(), &erf_value(&a, w + 4), w));
    }
    let af = a.to_f64();
    if af > 110.0 {
        return approx(saturated(false, false));
    }
    if af < 2.0 {
        // erfc(2) > 2^-8: at most 8 bits cancel.
        return approx(tr(&one().sub(&erf_value(&a, w + 12)), w));
    }
    approx(erfc_cf(&a, w))
}

/// `ln Γ(x)` for finite `x > 0` by Spouge's formula with parameter `n`:
/// `Γ(z+1) = (z+n)^(z+½) e^(−z−n) [c₀ + Σ_{k=1}^{n−1} c_k/(z+k) + ε]`, `c₀ = √(2π)`,
/// `c_k = (−1)^(k−1)/(k−1)! (n−k)^(k−½) e^(n−k)`, with `|ε|` relative below
/// `n^(−½) (2π)^(−(n+½))` (Spouge 1994); `n` grows with `w`. The coefficients alternate and
/// reach about `e^n`, so the sum is computed with `1.45 n` more bits. Returns the value with an
/// absolute error below `2^-w` times its largest part (`|(z+½) ln(z+n)| + |z+n| + 1`), and that
/// magnitude.
fn lgamma_spouge(x: &Big, w: u32) -> (Big, Big) {
    let n = u64::from(w) * 10 / 26 + 4;
    let ws = w + (n as u32) * 3 / 2 + 32;
    let z = x.sub(&one());
    let zn = z.add(&Big::from_u64(n));
    let half = Big::from_f64(0.5);
    let mut sum = sqrtw(&pi_w(ws).mul_pow2(1), ws);
    let mut fact = one(); // (k−1)!
    for k in 1..n {
        if k > 1 {
            fact = fact.mul(&Big::from_u64(k - 1));
        }
        let nk = Big::from_u64(n - k);
        // (n−k)^(k−½) e^(n−k) = exp((k−½) ln(n−k) + n − k).
        let ln_nk = log_core(&nk, ws);
        let expo = addw(&mulw(&Big::from_u64(k).sub(&half), &ln_nk, ws), &nk, ws);
        let ck = divw(&exp_core(&expo, ws), &fact, ws);
        let t = divw(&ck, &z.add(&Big::from_u64(k)), ws);
        sum = if k % 2 == 1 { addw(&sum, &t, ws) } else { addw(&sum, &t.neg(), ws) };
    }
    let lead = mulw(&z.add(&half), &log_core(&zn, ws), ws);
    let mag = lead.abs().add(&zn.abs()).add(&one());
    let v = addw(&addw(&lead, &zn.neg(), ws), &log_core(&sum, ws), ws);
    (v, mag)
}

/// `ln|Γ(x)|` for finite `x` that is not zero or a negative integer, to an absolute error below
/// `2^-(w+4)` times the largest part, with that magnitude: Spouge for `x > 0`, the reflection
/// `ln|Γ(x)| = ln π − ln|sin πx| − ln Γ(1−x)` for `x < 0` (`sin πx` from `x` minus its nearest
/// integer, exact).
fn lgamma_abs(x: &Big, w: u32) -> (Big, Big) {
    if !x.is_neg() {
        return lgamma_spouge(x, w + 4);
    }
    let m = x.round_int();
    let f = x.sub(&m);
    let s = sin_series(&mulw(&pi_w(w + 16), &f, w + 16), w + 8).abs();
    let (g, gm) = lgamma_spouge(&one().sub(x), w + 8);
    let lp = log_core(&pi_w(w + 16), w + 8);
    let ls = log_core(&s, w + 8);
    let v = addw(&addw(&lp, &ls.neg(), w + 8), &g.neg(), w + 8);
    (v, gm.add(&ls.abs()).add(&Big::from_i64(2)))
}

/// The log-gamma function `ln|Γ(x)|`.
pub fn lgamma(x: &Big, prec: u32) -> Approx {
    if x.is_nan() {
        return nan();
    }
    if x.is_inf() || x.is_zero() || (x.is_neg() && x.is_integer()) {
        return exact(Big::inf(false));
    }
    if x.cmp(&one()) == Some(Ordering::Equal) || x.cmp(&Big::from_i64(2)) == Some(Ordering::Equal) {
        return exact(Big::zero(false));
    }
    // Raise the precision until the absolute error is below 2^-(prec+2) of the value.
    let mut w = wp(prec);
    loop {
        let (v, mag) = lgamma_abs(x, w);
        let cancel = match (mag.ilogb(), v.ilogb()) {
            (Some(m), Some(t)) => (m - t).max(0),
            _ => i64::from(w),
        };
        if i64::from(w) >= i64::from(prec) + cancel + 8 || w > 1 << 16 {
            return approx(tr(&v, prec + 8));
        }
        w = (prec + 16).saturating_add(cancel as u32);
    }
}

/// The gamma function: `±e^(ln|Γ(x)|)` with the sign of `sin πx` for `x < 0`; exact factorials
/// at positive integers.
pub fn tgamma(x: &Big, prec: u32) -> Approx {
    if x.is_nan() || (x.is_inf() && x.is_neg()) || (x.is_neg() && x.is_integer() && !x.is_zero()) {
        return nan();
    }
    if x.is_inf() {
        return exact(x.clone());
    }
    if x.is_zero() {
        return exact(Big::inf(x.is_neg()));
    }
    if x.is_integer() && x.to_f64() <= 4000.0 {
        let n = x.to_i64().unwrap_or(1);
        let mut p = one();
        for k in 2..n {
            p = p.mul(&Big::from_i64(k));
        }
        return exact(p);
    }
    let w = wp(prec);
    // The sign of Γ(x) for x < 0 is that of sin(πx): negative when floor(x) is odd.
    let neg = x.is_neg() && x.floor().is_odd_integer();
    // e^L has relative error |dL|: the logarithm to absolute 2^-(w+4).
    let (l0, _) = lgamma_abs(x, 64);
    let mag = l0.ilogb().unwrap_or(0).max(0) as u32;
    if mag > 46 {
        return approx(saturated(!l0.is_neg(), neg));
    }
    let (l, _) = lgamma_abs(x, w + 8 + mag + 16);
    let v = exp_core(&l, w + 4);
    approx(if neg { v.neg() } else { v })
}

// ---------------------------------------------------------------- Bessel functions (milestone 3)

/// Below this argument the oracle uses the power series; above, Hankel's expansion (or a
/// recurrence for orders above `√x`).
const BESSEL_SERIES_LIMIT: f64 = 2000.0;

/// `(J_n(x), Y_n(x), m)` by the power series (A&S 9.1.10, 9.1.11) at working precision `w`,
/// `x > 0`, with `m` the largest magnitude summed (the bits the sums cancel are
/// `log2(m / result)`). `Y` only when `want_y`.
fn bessel_series_big(n: u64, x: &Big, w: u32, want_y: bool) -> (Big, Big, Big) {
    let wi = inner(w) + 16;
    let q = x.mul(x).mul_pow2(-2);
    let h = x.mul_pow2(-1);
    // P = (x/2)^n / n!.
    let mut p = one();
    for k in 1..=n {
        p = divn(&mulw(&p, &h, wi), k, wi);
    }
    let gamma2 = euler_w(wi).mul_pow2(1);
    let mut hn = Big::zero(false);
    if want_y {
        for j in 1..=n {
            hn = addw(&hn, &divn(&one(), j, wi), wi);
        }
    }
    let (mut sj, mut sy) = (one(), addw(&hn, &gamma2.neg(), wi));
    let mut term = one();
    let (mut hk, mut hnk) = (Big::zero(false), hn.clone());
    let mut mag = one().add(&sy.abs());
    let mut k = 1u64;
    let qf = q.to_f64();
    loop {
        term = divn(&divn(&mulw(&term, &q, wi), k, wi), n + k, wi).neg();
        if want_y {
            hk = addw(&hk, &divn(&one(), k, wi), wi);
            hnk = addw(&hnk, &divn(&one(), n + k, wi), wi);
        }
        if (k as f64) * ((n + k) as f64) > qf && negligible(&term, &mag, wi + 8) {
            break;
        }
        sj = addw(&sj, &term, wi);
        if term.cmp_abs(&mag) == Ordering::Greater {
            mag = term.abs();
        }
        if want_y {
            let ty = mulw(&term, &addw(&addw(&hk, &hnk, wi), &gamma2.neg(), wi), wi);
            if ty.cmp_abs(&mag) == Ordering::Greater {
                mag = ty.abs();
            }
            sy = addw(&sy, &ty, wi);
        }
        k += 1;
    }
    let j = mulw(&p, &sj, wi);
    let jmag = mulw(&p, &mag, wi);
    if !want_y {
        return (j, Big::zero(false), jmag);
    }
    let inv_pi = divw(&one(), &pi_w(wi), wi);
    let mut fin = Big::zero(false);
    let mut ymag = jmag.clone();
    if n > 0 {
        let mut u = one();
        for j in 1..n {
            u = u.mul(&Big::from_u64(j));
        }
        for k in 0..n {
            fin = addw(&fin, &u, wi);
            if k + 1 < n {
                u = divn(&mulw(&u, &q, wi), (k + 1) * (n - k - 1), wi);
            }
        }
        let two_over_x = divw(&Big::from_i64(2), x, wi);
        let mut pw = one();
        for _ in 0..n {
            pw = mulw(&pw, &two_over_x, wi);
        }
        fin = mulw(&mulw(&fin, &pw, wi), &inv_pi, wi).neg();
        if fin.cmp_abs(&ymag) == Ordering::Greater {
            ymag = fin.abs();
        }
    }
    let lnx2 = log_core(&h, wi);
    let mid = mulw(&mulw(&j, &lnx2, wi), &inv_pi, wi).mul_pow2(1);
    let last = mulw(&mulw(&p, &sy, wi), &inv_pi, wi);
    let ymag = ymag.add(&mid.abs()).add(&mulw(&p, &mag, wi).mul(&lnx2.abs().add(&one())));
    let y = addw(&addw(&fin, &mid, wi), &last.neg(), wi);
    (j, y, ymag)
}

/// Euler's constant at `w` bits (Brent–McMillan, as in the table generator).
fn euler_w(w: u32) -> Big {
    static CACHE: std::sync::OnceLock<Big> = std::sync::OnceLock::new();
    if w + 8 <= 2048 {
        return CACHE.get_or_init(|| crate::tables::euler_gamma(2048)).round(w + 8, Round::Nearest);
    }
    crate::tables::euler_gamma(w + 8)
}

/// `(J_n(x), Y_n(x), m)` by Hankel's expansion (DLMF 10.17.3/10.17.4; the remainders of P and Q
/// are below the first neglected term once `k >= n − ½`, DLMF 10.17(iii)), for large `x`.
fn hankel_big(n: u64, x: &Big, w: u32) -> (Big, Big, Big) {
    let wi = inner(w) + 16;
    let x8 = x.mul_pow2(3);
    let mu = Big::from_u64(n).mul(&Big::from_u64(n)).mul_pow2(2);
    let (mut p, mut q) = (one(), Big::zero(false));
    let mut a = one();
    let mut k = 1u64;
    loop {
        let odd = Big::from_u64(2 * k - 1).mul(&Big::from_u64(2 * k - 1));
        let next = divn(&divw(&mulw(&a, &mu.sub(&odd), wi), &x8, wi), k, wi);
        if next.is_zero() || negligible(&next, &one(), wi) || (k > n + 1 && next.cmp_abs(&a) == Ordering::Greater) {
            break;
        }
        a = next;
        match k % 4 {
            1 => q = addw(&q, &a, wi),
            2 => p = addw(&p, &a.neg(), wi),
            3 => q = addw(&q, &a.neg(), wi),
            _ => p = addw(&p, &a, wi),
        }
        k += 1;
    }
    // χ = x − (2n+1)π/4, with π to enough bits for the size of x.
    let top = x.ilogb().unwrap_or(0).max(0) as u32;
    let wp_ = wi + top + 16;
    let chi = x.sub(&pi_w(wp_).mul(&Big::from_u64(2 * n + 1)).mul_pow2(-2)).round(wp_, Round::Zero);
    let (s, c) = (sin_cos_reduced(&chi, wi, false), sin_cos_reduced(&chi, wi, true));
    let amp = sqrtw(&divw(&Big::from_i64(2), &mulw(&pi_w(wi), x, wi), wi), wi);
    let j = mulw(&amp, &addw(&mulw(&p, &c, wi), &mulw(&q, &s, wi).neg(), wi), wi);
    let y = mulw(&amp, &addw(&mulw(&p, &s, wi), &mulw(&q, &c, wi), wi), wi);
    let m = mulw(&amp, &p.abs().add(&q.abs()), wi);
    (j, y, m)
}

/// `(J_n(x), Y_n(x), m)` for `x > 0`: the series for small `x`, Hankel for `n <= √x`, else
/// recurrences from orders 0 and 1 (Y forward; J forward below `x`, backward from far above `n`
/// otherwise), each with `log2 n + 32` guard bits.
fn bessel_big(n: u64, x: &Big, w: u32, want_y: bool) -> (Big, Big, Big) {
    let xf = x.to_f64();
    if xf < BESSEL_SERIES_LIMIT {
        return bessel_series_big(n, x, w, want_y);
    }
    let nf = n as f64;
    if nf * nf <= xf {
        return hankel_big(n, x, w);
    }
    let wr = w + 64 - n.leading_zeros() + 32;
    let (j0, y0, _) = hankel_big(0, x, wr);
    let (j1, y1, _) = hankel_big(1, x, wr);
    let two_over_x = divw(&Big::from_i64(2), x, wr);
    let fwd = |f0: Big, f1: Big| {
        let (mut a, mut b) = (f0, f1);
        for k in 1..n {
            let next = addw(&mulw(&mulw(&b, &two_over_x, wr), &Big::from_u64(k), wr), &a.neg(), wr);
            a = b;
            b = next;
        }
        if n == 0 { a } else { b }
    };
    let y = if want_y { fwd(y0.clone(), y1.clone()) } else { Big::zero(false) };
    let j = if nf < xf {
        fwd(j0, j1)
    } else {
        let top = n + 128 + (20.0 * xf.cbrt()) as u64;
        let (mut above, mut cur) = (Big::zero(false), Big::pow2(-4000));
        let mut at_n = Big::zero(false);
        for k in (1..=top).rev() {
            let prev = addw(&mulw(&mulw(&cur, &two_over_x, wr), &Big::from_u64(k), wr), &above.neg(), wr);
            above = cur;
            cur = prev;
            if k - 1 == n {
                at_n = cur.clone();
            }
        }
        if j0.cmp_abs(&j1) != Ordering::Less { divw(&mulw(&at_n, &j0, wr), &cur, wr) } else { divw(&mulw(&at_n, &j1, wr), &above, wr) }
    };
    let m = j.abs().add(&y.abs());
    (j, y, m)
}

/// `J_n(x)` or `Y_n(x)` for an integer order, with the precision raised until the bits the
/// evaluation cancels are covered.
fn bessel_oracle(n: i64, x: &Big, prec: u32, want_y: bool) -> Approx {
    if x.is_nan() {
        return nan();
    }
    let m = n.unsigned_abs();
    let odd = m % 2 == 1;
    if want_y {
        if x.is_neg() && !x.is_zero() {
            return nan();
        }
        if x.is_zero() {
            return exact(Big::inf(!(odd && n < 0)));
        }
        if x.is_inf() {
            return exact(Big::zero(false));
        }
    } else {
        if x.is_inf() {
            return exact(Big::zero(false));
        }
        if x.is_zero() {
            return exact(if m == 0 { one() } else { Big::zero(false) });
        }
    }
    let neg = if want_y { odd && n < 0 } else { (odd && n < 0) != (odd && x.is_neg()) };
    let a = x.abs();
    let mut w = wp(prec) + 32;
    loop {
        let (j, y, mag) = bessel_big(m, &a, w, want_y);
        let v = if want_y { y } else { j };
        let cancel = match (mag.ilogb(), v.ilogb()) {
            (Some(mt), Some(vt)) => (mt - vt).max(0),
            _ => i64::from(w),
        };
        if i64::from(w) >= i64::from(prec) + cancel + 24 || w > 1 << 15 {
            return approx(tr(&v.with_sign(v.is_neg() != neg), prec + 8));
        }
        w = (wp(prec) + 32).saturating_add(cancel as u32);
    }
}

/// `J_n(x)`.
pub fn bessel_j(n: i64, x: &Big, prec: u32) -> Approx {
    bessel_oracle(n, x, prec, false)
}

/// `Y_n(x)`.
pub fn bessel_y(n: i64, x: &Big, prec: u32) -> Approx {
    bessel_oracle(n, x, prec, true)
}

/// `erfc x` for `x >= 2` by Laplace's continued fraction
/// `erfc x = e^(−x²)/√π · 1/(x + (1/2)/(x + 1/(x + (3/2)/(x + ...))))`. Its partial numerators are
/// positive, so successive truncations bracket the value; the depth doubles until two of them
/// agree to `w + 2` bits. Each truncation is evaluated from the bottom up (stable), with
/// `log2(depth) + 16` guard bits.
fn erfc_cf(x: &Big, w: u32) -> Big {
    let eval = |depth: u64, wi: u32| {
        let mut t = x.clone();
        for n in (1..=depth).rev() {
            t = addw(x, &divw(&Big::from_u64(n).mul_pow2(-1), &t, wi), wi);
        }
        divw(&one(), &t, wi)
    };
    let mut depth = 16u64;
    loop {
        let wi = w + 16 + 2 * (64 - depth.leading_zeros());
        let (a, b) = (eval(depth, wi), eval(depth + 1, wi));
        let d = a.sub(&b);
        let close = d.is_zero() || match (d.ilogb(), a.ilogb()) {
            (Some(td), Some(ta)) => td < ta - i64::from(w) - 2,
            _ => false,
        };
        if close || depth > 1 << 24 {
            let wf = w + 8;
            let e = exp_core(&tr(&x.mul(x), wf + 8).neg(), wf);
            return mulw(&divw(&e, &sqrtw(&pi_w(wf), wf), wf), &a, w);
        }
        depth *= 2;
    }
}
