//! Round-to-nearest in-place fast tier for `expm1`, `log1p`, `sinh`, `cosh`, `tanh` and `cbrt`
//! (`double`; LIBM-F64-EXPM1-HYPER-CBRT-TIER, Ziv's strategy as in [`crate::fast64`]). Each
//! entry forms `f(x)` as a double-double `y` with a proved bound `eps` and rounds it by
//! [`crate::fast64::settle`]; `None` sends the caller to the accurate path unchanged (D-10.13),
//! so every answer is the correctly rounded one and no accuracy contract changes.
//!
//! Notation: `u = 2^-53`; "units of `2^m`" means a value is stored as `y` with the real value
//! `y·2^m`. The exp kernel ([`crate::fast64::exp_kernel`]) gives `y` with `|e^r·2^(j/64) − y| <=
//! 2^-49.8·th·|T| + th·2^-74 + 2^-101` (its proof), and `fast64::reduce_exp` is within `2^-106.5`;
//! with `th <= |y|/0.994` and the computed `|tail|` within `5u` of `|T|` that is at most
//! `|y.hi|·(2^-49·|tail| + 2^-73)` in units of `2^m` ([`exp_err`]).
//!
//! The arguments: every entry refuses NaN, infinities, `|x| < 2^-60` and the ends of its range
//! before any arithmetic, so every intermediate stays normal and far from overflow and only
//! inexact is raised in hardware (the reasoning of the `fast64` in-place entries). The results
//! are irrational for every accepted `x` (Lindemann–Weierstrass for the exponential forms, the
//! logarithm of a rational other than 1) except the exact cubes of `cbrt`, which are declined
//! after the test ([`cbrt_nearest`]; the refused attempt's inexact is undone by the caller,
//! `mathret::elem_fast_cold`). The FMA build ([`fused`]) is the same arithmetic with every exact
//! product one FMA (`dd::two_prod_fma`), so the same results bit for bit (see `fast64`'s
//! `kernels!`).

#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)] // exponents of bounded range

use crate::dd::{self, fast_two_sum, two_sum, Dd};

/// 2^-49: the series tails' relative error bound, doubled.
const TAIL: f64 = f64::from_bits(0x3ce0_0000_0000_0000);
/// 2^-73: the exp kernel's error other than the tail's, relative to `|y.hi|` ([`exp_err`]).
const E73: f64 = f64::from_bits(0x3b60_0000_0000_0000);
/// 2^-70: covers the truncation of each short series and its other roundings (each below 2^-72).
const R70: f64 = f64::from_bits(0x3b90_0000_0000_0000);
/// 2^-96: covers the double-double steps after a kernel (each below 2^-98 of the result).
const R96: f64 = f64::from_bits(0x39f0_0000_0000_0000);
/// 2^-104 and 2^-114: the cbrt residual's rounding and the dropped `e^-2a` of sinh and cosh.
const E104: f64 = f64::from_bits(0x3970_0000_0000_0000);
const E114: f64 = f64::from_bits(0x38d0_0000_0000_0000);
/// 2^-60, the least `|x|` handled (as the `fast64` entries).
const TINY: f64 = crate::exp_log::TINY;
/// 2^-6: below it `expm1`, `sinh` and `tanh` use their series, at and above it the exp kernel.
const SMALL: f64 = 0.015_625;
/// 2^-7: below it `log1p` uses its series.
const SMALL_LOG: f64 = 0.007_812_5;
/// `x < 2^960` for `log1p` keeps `w.lo/w.hi` normal (`|w.lo| >= 2^-52` when nonzero and `x >= 2`).
const LOG1P_MAX: f64 = f64::from_bits(0x7bf0_0000_0000_0000);
/// The bit-trick seed of `z^(-1/3)` for `z` in `[1, 8)`: `(1023 + 341)·2^52`, exact at 1 and 8.
pub(crate) const CBRT_SEED: u64 = 0x5540_0000_0000_0000;

/// [`exp_kernel`](crate::fast64::exp_kernel)'s error in units of `2^m` (module doc).
#[inline(always)]
fn exp_err(y: Dd, tail: f64) -> f64 {
    dd::abs(y.hi) * (TAIL * dd::abs(tail) + E73)
}

/// The rounding: `y·2^m` correctly rounded to nearest, or `None` (the exact test).
#[inline(always)]
fn round(y: Dd, eps: f64, m: i32) -> Option<f64> {
    crate::fast64::settle(y, eps, m, crate::status::Rounding::Nearest).map(|(v, _)| v)
}

/// The entries in one build; `$k` is the kernels' module, `$prod` the exact product.
macro_rules! entries {
    ($(#[$a:meta])*; $k:ident; $prod:path) => {

/// `x + x²/2 + x³·(1/3! + … + x⁶/9!)` for `TINY <= |x| < 2^-6`, the error at most
/// `2^-49·|tail| + 2^-70·|y|`. Truncation: the omitted terms are below `|x|^10/10!·1.02 <
/// 2^-75·|x|`. The head `x + x²/2` is exact as a double-double (`$prod`, `two_sum`); the tail
/// `T` is evaluated with the coefficients' roundings, six Horner steps on terms that fall by
/// `2^-8` each and two products: below `6u·|T| < 2^-50·|T|`, and carried by two additions
/// (`2u·|T|` and `2^-105·|x|`). The last Fast2Sum is exact (`|lo| <= 2^-12·|a.hi|`).
$(#[$a])*
fn expm1_series(x: f64) -> (Dd, f64) {
    const C: [f64; 7] = [1.0 / 6.0, 1.0 / 24.0, 1.0 / 120.0, 1.0 / 720.0, 1.0 / 5040.0, 1.0 / 40320.0, 1.0 / 362_880.0];
    let q = C[0] + x * (C[1] + x * (C[2] + x * (C[3] + x * (C[4] + x * (C[5] + x * C[6])))));
    let s = $prod(x, x);
    let tail = x * s.hi * q;
    let a = two_sum(x, 0.5 * s.hi);
    let y = fast_two_sum(a.hi, a.lo + (0.5 * s.lo + tail));
    (y, TAIL * dd::abs(tail) + R70 * dd::abs(y.hi))
}

/// `e^x − 1` for the in-place entry: `2^-60 <= |x|`, `−37 < x < 709`.
///
/// Proof (`|x| >= 2^-6`). `v = y·2^m` (exact scaling: `−54 <= m <= 1022`, `y.lo·2^m` normal or 0)
/// is `e^x` within `2^m·`[`exp_err`]. The −1 is folded in by `w = two_sum(v.hi, −1)` (exact) and
/// `z = Fast2Sum(w.hi, w.lo + v.lo)`: `|e^x − 1| >= 2^-6·0.99`, so `|w.hi| >= 2^-6.02`, and
/// `|v.hi| <= 65·|w.hi|` (`e^a/|e^a − 1|` at `a = 2^-6`), so `|w.lo + v.lo| <= 2^-46.9·|w.hi|`:
/// the Fast2Sum is exact and its one rounding is below `2^-99.9·|z.hi|` ([`R96`]).
$(#[$a])*
pub fn expm1_nearest(x: f64) -> Option<f64> {
    let a = dd::abs(x);
    if !(TINY..709.0).contains(&a) || x <= -37.0 {
        return None;
    }
    if a < SMALL {
        let (y, eps) = expm1_series(x);
        return round(y, eps, 0);
    }
    let kf = dd::nearest_int(x * f64::from_bits(crate::coeffs::tables::SCALES[0]));
    let (y, m, tail) = $k::exp_kernel(crate::fast64::reduce_exp(x, kf), dd::to_i32(kf))?;
    let s = dd::pow2(m);
    let (vh, vl) = (y.hi * s, y.lo * s);
    let w = two_sum(vh, -1.0);
    let z = fast_two_sum(w.hi, w.lo + vl);
    round(z, exp_err(y, tail) * s + R96 * dd::abs(z.hi), 0)
}

/// `ln(1 + x)` for the in-place entry: `−1 < x < 2^960`, `|x| >= 2^-60`.
///
/// Proof. `|x| < 2^-7`: the series of `fast64::log_series` with `r = x` exactly (`rl = 0`):
/// truncation below `2^-80·|x|`, the tail's evaluation and carry below `6u·|T|`; the head exact.
/// Otherwise `1 + x = w.hi + w.lo` exactly (`two_sum`), `w.hi` positive normal and not 1, and
/// `ln(1 + x) = ln w.hi + ln(1 + d0)` with `d0 = w.lo/w.hi`, `|d0| <= u`. `fast64::log_kernel`
/// gives `ln w.hi` within `fast64::log_eps`; `d = RN(d0)` is within
/// `d0²/2 + u·|d0| <= 2^-105` of `ln(1 + d0)`; `z = Fast2Sum(y.hi, y.lo + d)` is exact
/// (`|y.hi| >= 2^-7.01 > 2^-52`) and its rounding is below `2^-106·(|y.hi| + 1)`. With
/// `|z.hi| >= 2^-7.1` all of that is below `2^-96·|z.hi|`. `x < 2^960` keeps `d` normal or 0.
$(#[$a])*
pub fn log1p_nearest(x: f64) -> Option<f64> {
    if !(x > -1.0 && x < LOG1P_MAX) || dd::abs(x) < TINY {
        return None;
    }
    if dd::abs(x) < SMALL_LOG {
        const C: [f64; 8] = [1.0 / 3.0, -1.0 / 4.0, 1.0 / 5.0, -1.0 / 6.0, 1.0 / 7.0, -1.0 / 8.0, 1.0 / 9.0, -1.0 / 10.0];
        let q = C[0] + x * (C[1] + x * (C[2] + x * (C[3] + x * (C[4] + x * (C[5] + x * (C[6] + x * C[7]))))));
        let s = $prod(x, x);
        let tail = x * s.hi * q;
        let a = two_sum(x, -0.5 * s.hi);
        let y = fast_two_sum(a.hi, a.lo + (tail - 0.5 * s.lo));
        return round(y, TAIL * dd::abs(tail) + R70 * dd::abs(y.hi), 0);
    }
    let w = two_sum(1.0, x);
    let (y, tail) = $k::log_kernel(w.hi)?;
    let z = fast_two_sum(y.hi, y.lo + w.lo / w.hi);
    round(z, crate::fast64::log_eps(y, tail) + R96 * dd::abs(z.hi), 0)
}

/// `x + x³·q(x²)`, `q` the first `n` Taylor coefficients `C` of an odd function, for
/// `TINY <= |x| < 2^-6`: the error is at most `2^-49·|tail| + 2^-70·|y|` when the omitted terms
/// are below `2^-72·|x|` (each caller states it). `s = RN(x²)` (relative `u`), the tail's
/// evaluation `< 6u·|T|`; `Fast2Sum(x, tail)` is exact (`|tail| <= 2^-12·|x|`).
$(#[$a])*
fn odd_series<const N: usize>(x: f64, c: &[f64; N]) -> (Dd, f64) {
    let s = x * x;
    let mut q = c[N - 1];
    let mut i = N - 1;
    while i > 0 {
        i -= 1;
        q = c[i] + s * q;
    }
    let tail = x * s * q;
    let y = fast_two_sum(x, tail);
    (y, TAIL * dd::abs(tail) + R70 * dd::abs(y.hi))
}

/// `e^a` and `e^-a` as `(p, m, ep, qs, eq)`: `p·2^m` and `qs·2^m` (both in units of `2^m`)
/// within `ep` and `eq`, for `TINY <= a <= 40`. One reduction serves both: `e^-a` is the
/// kernel at `(−r, −k)` (`2^(−k/64) = 2^(((−k)&63)/64)·2^((−k)>>6)`), and its scaling by
/// `2^(n−m) >= 2^-116` is exact (its parts stay normal or 0).
$(#[$a])*
fn exp_pm(a: f64) -> Option<(Dd, i32, f64, Dd, f64)> {
    let kf = dd::nearest_int(a * f64::from_bits(crate::coeffs::tables::SCALES[0]));
    let r = crate::fast64::reduce_exp(a, kf);
    let kk = dd::to_i32(kf);
    let (p, m, tp) = $k::exp_kernel(r, kk)?;
    let (q, n, tq) = $k::exp_kernel(r.neg(), -kk)?;
    let s = dd::pow2(n - m);
    Some((p, m, exp_err(p, tp), q.scale(s), exp_err(q, tq) * s))
}

/// `sinh x` for the in-place entry: `2^-60 <= |x| < 709`.
///
/// Proof. `|x| < 2^-6`: [`odd_series`] with `1/3!, …, 1/9!` (omitted `< |x|^11/11!·1.01 <
/// 2^-85·|x|`). `40 < a`: `sinh a = e^a/2·(1 − e^-2a)`, `e^-2a < 2^-115`, so `p·2^(m−1)` within
/// `ep + 2^-114·|p.hi|`. Else `(p − qs)·2^(m−1)` by `Dd::sub`: `|p.hi − qs.hi| >= 2^-5·0.99`
/// against parts below `2^-52`, so its Fast2Sums are exact and its roundings below
/// `2^-102·|p.hi|`; the error `ep + eq + 2^-100·|p.hi|` in units of `2^(m−1)`.
$(#[$a])*
pub fn sinh_nearest(x: f64) -> Option<f64> {
    let a = dd::abs(x);
    if !(TINY..709.0).contains(&a) {
        return None;
    }
    let v = if a < SMALL {
        let (y, eps) = odd_series(a, &[1.0 / 6.0, 1.0 / 120.0, 1.0 / 5040.0, 1.0 / 362_880.0]);
        round(y, eps, 0)?
    } else if a > 40.0 {
        let kf = dd::nearest_int(a * f64::from_bits(crate::coeffs::tables::SCALES[0]));
        let (p, m, tp) = $k::exp_kernel(crate::fast64::reduce_exp(a, kf), dd::to_i32(kf))?;
        round(p, exp_err(p, tp) + E114 * dd::abs(p.hi), m - 1)?
    } else {
        let (p, m, ep, qs, eq) = exp_pm(a)?;
        let d = p.sub(qs);
        round(d, ep + eq + R96 * dd::abs(p.hi), m - 1)?
    };
    Some(if x < 0.0 { -v } else { v })
}

/// `cosh x` for the in-place entry: `2^-60 <= |x| < 709`.
///
/// Proof. `40 < a`: as [`sinh_nearest`]. Else `(p + qs)·2^(m−1)` by `Dd::add` (no cancellation,
/// within `2^-104` of the sum): the error `ep + eq + 2^-100·|p.hi|`.
$(#[$a])*
pub fn cosh_nearest(x: f64) -> Option<f64> {
    let a = dd::abs(x);
    if !(TINY..709.0).contains(&a) {
        return None;
    }
    if a > 40.0 {
        let kf = dd::nearest_int(a * f64::from_bits(crate::coeffs::tables::SCALES[0]));
        let (p, m, tp) = $k::exp_kernel(crate::fast64::reduce_exp(a, kf), dd::to_i32(kf))?;
        return round(p, exp_err(p, tp) + E114 * dd::abs(p.hi), m - 1);
    }
    let (p, m, ep, qs, eq) = exp_pm(a)?;
    let c = p.add(qs);
    round(c, ep + eq + R96 * dd::abs(c.hi), m - 1)
}

/// `tanh x` for the in-place entry: `2^-60 <= |x| <= 22`.
///
/// Proof. `|x| < 2^-6`: [`odd_series`] with the Taylor coefficients `−1/3, 2/15, −17/315,
/// 62/2835, −1382/155925` (alternating and falling: the omitted terms are below
/// `0.004·|x|^13 < 2^-80·|x|`). Else `N = p − qs`, `D = p + qs` (units of `2^m`, each within
/// `E = ep + eq + 2^-100·|p.hi|`), the real `D·2^m >= 2`. For errors `dN`, `dD` below `E·2^m`:
/// `|N'/D' − N/D| <= E·2^m·(D + N)/(D·D') <= 1.01·E·2^m`. The quotient: `q1 = RN(N.hi/D.hi)`,
/// the residual `N − q1·D` with `q1·D.hi` exact (`$prod`) and `N.hi − P.hi` exact (Sterbenz),
/// `q2 = RN(res/D.hi)`: the remaining error below `2^-100·|q|` ([`R96`]).
$(#[$a])*
pub fn tanh_nearest(x: f64) -> Option<f64> {
    let a = dd::abs(x);
    if !(TINY..=22.0).contains(&a) {
        return None;
    }
    let v = if a < SMALL {
        let (y, eps) = odd_series(a, &[-1.0 / 3.0, 2.0 / 15.0, -17.0 / 315.0, 62.0 / 2835.0, -1382.0 / 155_925.0]);
        round(y, eps, 0)?
    } else {
        let (p, m, ep, qs, eq) = exp_pm(a)?;
        let (n, d) = (p.sub(qs), p.add(qs));
        let q1 = n.hi / d.hi;
        let pr = $prod(q1, d.hi);
        let res = (((n.hi - pr.hi) - pr.lo) + n.lo) - q1 * d.lo;
        let q = fast_two_sum(q1, res / d.hi);
        let e = 1.01 * (ep + eq + R96 * dd::abs(p.hi)) * dd::pow2(m);
        round(q, e + R96 * dd::abs(q.hi), 0)?
    };
    Some(if x < 0.0 { -v } else { v })
}

/// `∛x` for the in-place entry: `x` normal and finite (nonzero).
///
/// Proof. `|x| = 2^(3q)·z`, `z` in `[1, 8)` (integer exponent arithmetic, exact). The seed
/// `g = from_bits(CBRT_SEED − bits(z)/3)` is `z^(-1/3)` within 6% (piecewise linear, exact at 1
/// and 8). Each refinement `g·(1 + h/3 + 2h²/9 + 14h³/81)` with `h = 1 − z·g³` is the series of
/// `(1 − h)^(-1/3)` to `h³` (left out: below `0.2·|h|⁴`): `6% -> 2^-12 -> 2^-43`. None of this is
/// trusted: `y = RN(z·g²)`, the residual `ρ = z − y³` (`y² = a.hi + a.lo` and `a.hi·y = c.hi + c.lo`
/// exact by `$prod`, `z − c.hi` exact by Sterbenz) within `2u·|ρ| + 2^-105·z`, and `δ =
/// RN(ρ/RN(3·a.hi))` within `5u·|δ| + 2^-106.5·y` of `ρ/(3y²)`. With `ε = ρ/y³`, `|ε| <= 2^-18`
/// (checked as `|δ| <= 2^-20·y`), `∛z = y·(1 + ε)^(1/3)` and `|∛z − y − ρ/(3y²)| <=
/// y·(ε²/9)·(1 + |ε|) <= 1.01·δ²/y <= 1.02·δ²·g` (`1/y <= g·(1 + |ε|)`, as `(z·g³)² ≈ 1 + ε`).
/// So `y + δ` (exact Fast2Sum) is within `1.02·δ²·g + 2^-50·|δ| + 2^-104·y` of `∛z`. An exact
/// cube has a root of at most 18 significant bits (`o³` odd below 2^53 needs `o < 2^17.7`), so
/// a result with the low 35 bits of its significand clear is declined (the exact case and a
/// rare inexact one alike); no midpoint is a cube root of a `double` (`o³` with `o` odd of 54
/// bits has more than 53).
$(#[$a])*
pub fn cbrt_nearest(x: f64) -> Option<f64> {
    let b = x.to_bits() & 0x7fff_ffff_ffff_ffff;
    if !(0x0010_0000_0000_0000..0x7ff0_0000_0000_0000).contains(&b) {
        return None;
    }
    let e = i32::try_from(b >> 52).unwrap_or(1023) - 1023;
    let q = e.div_euclid(3);
    let zb = b.wrapping_add_signed(-(i64::from(3 * q) << 52));
    let z = f64::from_bits(zb);
    let mut g = f64::from_bits(CBRT_SEED - zb / 3);
    for _ in 0..2 {
        let h = 1.0 - z * (g * g * g);
        g *= 1.0 + h * (1.0 / 3.0 + h * (2.0 / 9.0 + h * (14.0 / 81.0)));
    }
    let y = z * (g * g);
    let sq = $prod(y, y);
    let c = $prod(sq.hi, y);
    let rho = ((z - c.hi) - c.lo) - sq.lo * y;
    let d = rho / (3.0 * sq.hi);
    if dd::abs(d) > y * f64::from_bits(0x3eb0_0000_0000_0000) {
        return None; // 2^-20
    }
    let eps = 1.02 * d * d * g + TAIL * 0.5 * dd::abs(d) + E104 * y;
    let v = round(fast_two_sum(y, d), eps, q)?;
    if v.to_bits() & 0x0000_0007_ffff_ffff == 0 {
        return None; // at most 18 significant bits: possibly an exact cube
    }
    Some(if x < 0.0 { -v } else { v })
}
    };
}

/// The kernels of the plain build (`$k` of `entries!`).
mod kern {
    pub(crate) use crate::fast64::{exp_kernel, log_kernel};
}

entries!(#[inline(always)]; kern; dd::two_prod);

/// The FMA build of the entries (bit-identical results, module doc). Callable only on a CPU with
/// FMA and the OS-enabled AVX state; the boundary selects it once at startup (`dispatch::fma`).
// ARCH(x86_64): the run-time FMA build (D-10.25); FMA is base ISA on aarch64, so one build there
#[cfg(target_arch = "x86_64")]
pub mod fused {
    use super::{exp_err, fast_two_sum, round, two_sum, Dd, CBRT_SEED, E104, E114, LOG1P_MAX, R70, R96, SMALL, SMALL_LOG, TAIL, TINY};
    use crate::dd;

    /// The kernels of the FMA build.
    mod kern {
        pub(crate) use crate::fast64::fused::{exp_kernel, log_kernel};
    }

    entries!(#[target_feature(enable = "fma")] #[inline]; kern; dd::two_prod_fma);
}
