//! The quick phase of `double` `asinh`, `acosh` and `atanh` (D-10.51, D-10.65;
//! LIBM-F64-INVHYP-QUICK): the logarithm's argument as an unevaluated pair `(hi, lo)` accurate to
//! about `2^-100`, then the quick log's pair entries ([`super::quick_log`]: `log_pair_parts`,
//! `log1p_pair_parts`, relative bound `RHO_PAIR = 2^-63`) and Ziv's one-sided rounding test
//! (Ziv, ACM TOMS 17(3), 1991; de Dinechin, Lauter & Muller, RAIRO-ITA 41, 2007) with `C_PAIR`.
//! A miss or an argument outside the ranges below returns `None`: the caller runs the accurate
//! path (tier 2, `crate::hyper`, unchanged).
//!
//! **Method** (`a = |x|`; error-free transformations after Dekker 1971 and Knuth, TAOCP 2, 4.2.2;
//! the inverse hyperbolic identities as in Muller, Elementary Functions, 3rd ed. 2016):
//! * `sqrt(th + tl)` as `(sh, sl)`: `sh = RN(sqrt(th))`, one Newton correction
//!   `sl = ((th − p) − p' + tl)/(2·sh)` with `sh² = p + p'` exactly (2MultFMA or Dekker's
//!   product); `th − p` is exact (Sterbenz), so `sh + sl` is within `2^-100·sh` of the root.
//! * acosh, `1 < x < 2`: `a1 = x − 1` (exact), `t = 2·a1 + a1²` (`a1² = p + p'`, Fast2Sum),
//!   `y = sqrt(t) + a1` (Fast2Sum, `sqrt(t) >= a1`), `acosh x = log1p(y)`. `2 <= x < 2^32`:
//!   `t = x² − 1`, `X = x + sqrt(t)`, `ln X`. `2^32 <= x < 2^1000`: `ln(2x)` (the dropped
//!   `−1/(2x)` is `2^-66` relative to `2x`, `2^-70.5` relative to `ln(2x) >= 22`). `x = 1` gives `+0`
//!   exactly (no test, no flag).
//! * asinh, `2^-200 <= a < 1`: `t = 1 + a²`, `w = sh − 1` (exact, `sh` in `[1, √2]`),
//!   `y = a + w` (Fast2Sum, `w <= a/2`) plus `sl`, `asinh a = log1p(y)`. `1 <= a < 2^32`:
//!   `X = a + sqrt(a² + 1)`, `ln X`. `2^32 <= a < 2^1000`: `ln(2a)` as for acosh. Odd: the sign is
//!   copied onto the result.
//! * atanh, `2^-200 <= a < 1`: `atanh a = log1p(y)/2`, `y = 2a/(1 − a)`. `1 − a = dh + dl`
//!   (Fast2Sum), `inv = RN(1/dh)`, `yh = RN(2a·inv)`, the remainder
//!   `ρ = ((2a − p) − p') − yh·dl` with `yh·dh = p + p'` exactly (`2a − p` exact, Sterbenz), and
//!   `yl = ρ·inv`. One division; `yh + yl` is within `2^-100·y` of `y` (the roundings of `ρ`,
//!   `inv` and the dropped `dl` in the last denominator all act on `|ρ| <= 3u·2a`).
//!
//! **Bound.** Relative to the result, each argument error `ε` (relative to `y` or `X`) reaches the
//! logarithm divided by `(1 + y)·log1p(y)/y >= 1` or `ln X >= 0.87`: below `2^-98`. The low parts'
//! sizes meet the pair entries' conditions: acosh below 2 `|yl| <= 2^-52·1.01·yh`; atanh
//! `|yl| <= 2^-51.4·yh`; asinh below 1 `|yl| <= 2^-53·(1 + yh) + a²/2` when `sh = 1` (then
//! `E1 <= a⁴/2`, `E2 <= a³/8 < 2^-82` for `a² < 2^-53`), else `|yl| <= 2^-52.98` absolute with
//! `r = yh <= 2^-7` (`E1 <= 2^-66.98`, `E2 <= 2^-80`). Total within `RHO_PAIR = 2^-63`, so the test
//! with `C_PAIR` is valid; it needs the result neither a double nor a midpoint, true for every
//! double `x != 0` (and `x != 1` for acosh) by Lindemann–Weierstrass (the logarithm of an
//! algebraic number other than 1 is transcendental). atanh halves `zh` exactly (`zh >= 2^-201`).
//!
//! **Flags.** No spurious flag: `2^-200 <= a` keeps every product (`r⁴`, `d·r²`, `a²`'s low part)
//! normal or exact, `a < 2^1000` keeps `2a`, `x²` finite; the root's argument is positive and
//! `dh > 0`, `sh > 0`. Every answered result is inexact, and the computation raises inexact.
//!
//! The square root is passed in (`sqrt`, the boundary's `sqrtsd`): it must be correctly rounded.
//! Both builds are correctly rounded wherever they answer, so they return the same values.

#![allow(clippy::arithmetic_side_effects)] // floating-point kernels; integer work is bit masks only

const SIGN: u64 = 1 << 63;
/// `2^-200`, `2^32`, `2^1000`: the ranges above.
const TINY: f64 = f64::from_bits(0x3370_0000_0000_0000);
const BIG: f64 = 4_294_967_296.0;
const HUGE: f64 = f64::from_bits(0x7e70_0000_0000_0000);

macro_rules! invhyp {
    ($(#[$a:meta])*; $(#[$i:meta])*; $prod:path; $q:ident) => {
/// `sqrt(th + tl) = sh + sl` within `2^-100·sh` for `th > 0`, `|tl| <= 2^-52·th` (module doc).
$(#[$a])*
$(#[$i])*
fn sqrt_pair(th: f64, tl: f64, sqrt: impl Fn(f64) -> f64) -> (f64, f64) {
    let sh = sqrt(th);
    let p = $prod(sh, sh);
    let e = ((th - p.hi) - p.lo) + tl;
    (sh, e / (sh + sh))
}

/// The correctly rounded `acosh x` (round to nearest) when the quick phase decides it, else
/// `None` (module doc).
$(#[$a])*
$(#[$i])*
pub fn acosh_quick(x: f64, sqrt: impl Fn(f64) -> f64) -> Option<f64> {
    if !(x > 1.0 && x < HUGE) {
        return if x == 1.0 { Some(0.0) } else { None };
    }
    let (zh, zl) = if x < 2.0 {
        let a = x - 1.0;
        let p = $prod(a, a);
        let a2 = a + a;
        let th = a2 + p.hi;
        let tl = ((a2 - th) + p.hi) + p.lo;
        let (sh, sl) = sqrt_pair(th, tl, sqrt);
        let yh = sh + a;
        $q::log1p_pair_parts(yh, ((sh - yh) + a) + sl)
    } else if x < BIG {
        let p = $prod(x, x);
        let th = p.hi - 1.0;
        let tl = ((p.hi - th) - 1.0) + p.lo;
        let (sh, sl) = sqrt_pair(th, tl, sqrt);
        let xh = x + sh;
        $q::log_pair_parts(xh, ((x - xh) + sh) + sl)
    } else {
        $q::log_pair_parts(x + x, 0.0)
    };
    if zh + zl * C_PAIR == zh { Some(zh) } else { None }
}

/// The correctly rounded `asinh x` (round to nearest) when the quick phase decides it, else
/// `None` (module doc).
$(#[$a])*
$(#[$i])*
pub fn asinh_quick(x: f64, sqrt: impl Fn(f64) -> f64) -> Option<f64> {
    let a = f64::from_bits(x.to_bits() & !SIGN);
    if !(TINY..HUGE).contains(&a) {
        return None;
    }
    let (zh, zl) = if a < 1.0 {
        let p = $prod(a, a);
        let th = 1.0 + p.hi;
        let tl = ((1.0 - th) + p.hi) + p.lo;
        let (sh, sl) = sqrt_pair(th, tl, sqrt);
        let w = sh - 1.0;
        let yh = a + w;
        $q::log1p_pair_parts(yh, ((a - yh) + w) + sl)
    } else if a < BIG {
        let p = $prod(a, a);
        let th = p.hi + 1.0;
        let tl = ((p.hi - th) + 1.0) + p.lo;
        let (sh, sl) = sqrt_pair(th, tl, sqrt);
        let xh = sh + a;
        $q::log_pair_parts(xh, ((sh - xh) + a) + sl)
    } else {
        $q::log_pair_parts(a + a, 0.0)
    };
    if zh + zl * C_PAIR == zh { Some(f64::from_bits(zh.to_bits() | (x.to_bits() & SIGN))) } else { None }
}

/// The correctly rounded `atanh x` (round to nearest) when the quick phase decides it, else
/// `None` (module doc).
$(#[$a])*
$(#[$i])*
pub fn atanh_quick(x: f64) -> Option<f64> {
    let a = f64::from_bits(x.to_bits() & !SIGN);
    if !(TINY..1.0).contains(&a) {
        return None;
    }
    let dh = 1.0 - a;
    let dl = (1.0 - dh) - a;
    let inv = 1.0 / dh;
    let n = a + a;
    let yh = n * inv;
    let p = $prod(yh, dh);
    let rho = ((n - p.hi) - p.lo) - yh * dl;
    let (zh, zl) = $q::log1p_pair_parts(yh, rho * inv);
    if zh + zl * C_PAIR == zh { Some(f64::from_bits((0.5 * zh).to_bits() | (x.to_bits() & SIGN))) } else { None }
}
    };
}

use super::quick_log::C_PAIR;
use crate::dd;

mod sse2 {
    pub(super) use super::super::quick_log::{log1p_pair_parts, log_pair_parts};
}

invhyp!(; #[inline(always)]; dd::two_prod; sse2);

/// The quick phase's FMA build (D-10.25, D-10.50): callable only on a CPU with FMA whose AVX state
/// the OS saves; the boundary chooses it once at startup (`dispatch::fma`).
// ARCH(x86_64): the run-time FMA build; FMA is base ISA on aarch64, so one build there
#[cfg(target_arch = "x86_64")]
pub mod fused {
    use super::{dd, BIG, C_PAIR, HUGE, SIGN, TINY};
    use crate::fast64::quick_log::fused as q;

    invhyp!(
        #[doc = ""]
        #[doc = "# Safety"]
        #[doc = ""]
        #[doc = "The CPU must support FMA and the OS must save the AVX state (the `#[target_feature]` contract)."]
        #[target_feature(enable = "fma")]; #[inline]; dd::two_prod_fma; q
    );
}
