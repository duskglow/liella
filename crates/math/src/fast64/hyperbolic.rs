//! Round-to-nearest in-place fast tier for the `double` `sinh`, `cosh` and `tanh`
//! (LIBM-F64-EXPM1-HYPER-CBRT-TIER). The error terms, the exp kernel's bound ([`exp_err`]) and
//! the rounding are `fast64/exp_log.rs`'s (its module doc has the notation and the argument
//! that only inexact is raised and every accepted result is irrational); `None` sends the caller
//! to the accurate path unchanged (D-10.13). The FMA build ([`fused`]) is the same arithmetic
//! with every exact product one FMA, so the same results bit for bit.

#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)] // exponents of bounded range

use super::exp_log::{exp_err, round, R70, R96, SMALL, TAIL, TINY};
use crate::dd::{self, fast_two_sum, Dd};

/// 2^-114: the dropped `e^-2a` of sinh and cosh above 40.
const E114: f64 = f64::from_bits(0x38d0_0000_0000_0000);

/// The entries in one build; `$k` is the kernels' module, `$prod` the exact product.
macro_rules! entries {
    ($(#[$a:meta])*; $(#[$p:meta])*; $k:ident; $prod:path) => {

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

/// `e^a/2` for `40 < a < 709` as `p·2^(m−1)` with its bound.
$(#[$a])*
fn exp_half(a: f64) -> Option<f64> {
    let kf = dd::nearest_int(a * f64::from_bits(crate::coeffs::tables::SCALES[0]));
    let (p, m, tp) = $k::exp_kernel(crate::fast64::reduce_exp(a, kf), dd::to_i32(kf))?;
    round(p, exp_err(p, tp) + E114 * dd::abs(p.hi), m - 1)
}

/// `sinh x` for the in-place entry: `2^-60 <= |x| < 709`.
///
/// Proof. `|x| < 2^-6`: [`odd_series`] with `1/3!, …, 1/9!` (omitted `< |x|^11/11!·1.01 <
/// 2^-85·|x|`). `40 < a`: `sinh a = e^a/2·(1 − e^-2a)`, `e^-2a < 2^-115`, so `p·2^(m−1)` within
/// `ep + 2^-114·|p.hi|`. Else `(p − qs)·2^(m−1)` by `Dd::sub`: `|p.hi − qs.hi| >= 2^-5·0.99`
/// against parts below `2^-52`, so its Fast2Sums are exact and its roundings below
/// `2^-102·|p.hi|`; the error `ep + eq + 2^-100·|p.hi|` in units of `2^(m−1)`.
$(#[$a])*
$(#[$p])*
pub fn sinh_nearest(x: f64) -> Option<f64> {
    let a = dd::abs(x);
    if !(TINY..709.0).contains(&a) {
        return None;
    }
    let v = if a < SMALL {
        let (y, eps) = odd_series(a, &[1.0 / 6.0, 1.0 / 120.0, 1.0 / 5040.0, 1.0 / 362_880.0]);
        round(y, eps, 0)?
    } else if a > 40.0 {
        exp_half(a)?
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
$(#[$p])*
pub fn cosh_nearest(x: f64) -> Option<f64> {
    let a = dd::abs(x);
    if !(TINY..709.0).contains(&a) {
        return None;
    }
    if a > 40.0 {
        return exp_half(a);
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
/// `E = ep + eq + 2^-100·|p.hi|`). For errors `dN`, `dD` below `E·2^m`, with `N < D`:
/// `|N'/D' − N/D| <= E·2^m·(D + N)/(D·D'·2^m) <= 2E/D'` (the `2^m` cancels), at most `2.02·E/d.hi`
/// (`d.hi` within `2^-100` of `D'`); about `2^-69` relative, as `E` is about `2^-70·D`. The quotient: `q1 = RN(N.hi/D.hi)`,
/// the residual `N − q1·D` with `q1·D.hi` exact (`$prod`) and `N.hi − P.hi` exact (Sterbenz),
/// `q2 = RN(res/D.hi)`: the remaining error below `2^-100·|q|` ([`R96`]).
$(#[$a])*
$(#[$p])*
pub fn tanh_nearest(x: f64) -> Option<f64> {
    let a = dd::abs(x);
    if !(TINY..=22.0).contains(&a) {
        return None;
    }
    let v = if a < SMALL {
        let (y, eps) = odd_series(a, &[-1.0 / 3.0, 2.0 / 15.0, -17.0 / 315.0, 62.0 / 2835.0, -1382.0 / 155_925.0]);
        round(y, eps, 0)?
    } else {
        let (p, _, ep, qs, eq) = exp_pm(a)?;
        let (n, d) = (p.sub(qs), p.add(qs));
        let q1 = n.hi / d.hi;
        let pr = $prod(q1, d.hi);
        let res = (((n.hi - pr.hi) - pr.lo) + n.lo) - q1 * d.lo;
        let q = fast_two_sum(q1, res / d.hi);
        let e = 2.02 * (ep + eq + R96 * dd::abs(p.hi)) / d.hi;
        round(q, e + R96 * dd::abs(q.hi), 0)?
    };
    Some(if x < 0.0 { -v } else { v })
}
    };
}

/// The kernels of the plain build (`$k` of `entries!`).
mod kern {
    pub(crate) use crate::fast64::exp_kernel;
}

entries!(#[inline(always)]; ; kern; dd::two_prod);

/// The FMA build of the entries (bit-identical results, module doc). Callable only on a CPU with
/// FMA and the OS-enabled AVX state; the boundary selects it once at startup (`dispatch::fma`).
// ARCH(x86_64): the run-time FMA build (D-10.25); FMA is base ISA on aarch64, so one build there
#[cfg(target_arch = "x86_64")]
pub(super) mod fused {
    use super::{exp_err, fast_two_sum, round, Dd, E114, R70, R96, SMALL, TAIL, TINY};
    use crate::dd;

    /// The kernels of the FMA build.
    mod kern {
        pub(crate) use crate::fast64::fused::exp_kernel;
    }

    entries!(#[target_feature(enable = "fma")] #[inline];
        #[doc = ""] #[doc = "# Safety"] #[doc = ""]
        #[doc = "The CPU must support FMA and the OS must save the AVX state (the `#[target_feature]` contract)."];
        kern; dd::two_prod_fma);
}
