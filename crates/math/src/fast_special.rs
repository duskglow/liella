//! The `double` `erf`/`erfc` fast tier (Ziv's first step, LIBM-SPECIAL-FAST-TIER; spec §10.3,
//! D-10.13: every result correctly rounded in the caller's mode). Each entry evaluates the
//! function as a double-double `y·2^m` with a proved bound `eps` on its error and hands both to
//! [`fast64::settle`], which returns the correctly rounded result when every value within `eps`
//! of `y` rounds alike, and `None` otherwise; the caller ([`crate::special::erf`],
//! [`crate::special::erfc`]) then runs the 384-bit path unchanged. The accuracy contract is
//! unchanged (correct rounding, ruling #2760 option A).
//!
//! **Tables** (`crates/mathtools/src/special_tables.rs` derives them and proves the per-slot
//! bounds; `cargo xtask tables` regenerates them). A table operand `x` falls in the slot of its
//! exponent and four leading fraction bits, index `(x.to_bits() >> 48) − base`; the slot's centre
//! `c` keeps those bits and sets the fifth (`| 1 << 47`), so `t = x − c` is exact (Sterbenz,
//! `c/2 <= x <= 2c`) and `|t| <= 2^e/32`. A row holds the Taylor coefficients at `c`:
//! `[P0h, P0l, …, P3h, P3l, p4, …, p14]` (`*_DD = 4` double-doubles, degree `*_DEG = 14`).
//! [`taylor`] evaluates them in the order the generator's bound assumes: `f64` Horner from `p14`
//! down to `p4`, then four double-double steps `S ← S·t + P_k` (an exact product, an exact sum,
//! four roundings of the small parts), then Fast2Sum. Each table's `*_EPS` is the largest
//! per-slot bound relative to the function's smallest value on the slot (so `eps·|y.hi|` bounds
//! the absolute error; the stored constant is rounded up by `1 + 10^-9`, which covers the
//! rounding of that product and `y.hi`'s own `2^-53`).
//!
//! * `erf`, `2^-5 <= |x| < 6`: `ERF_T` (base `0x3fa0`), `ERF_T_EPS` = 2^-71.31.
//! * `erf`, `2^-55 <= |x| < 2^-5`: the series `x·Σ q_n x^(2n)` of `ERF_SMALL` ([`erf_small`]),
//!   `ERF_SMALL_EPS` = 2^-73.5 plus `3u²` for the last product (`u = 2^-53`).
//! * `erf`, `|x| < 2^-55`: `x·q0` (the dropped `q1·x²/q0 = x²/3 < 2^-111` relative), bound
//!   `2^-100` relative (the stored `q0` within `2^-106`, two roundings of `2^-105`). Below `2^-900`
//!   `x` is first scaled into `[1, 2)` (`x = xs·2^m` exactly; the product halved, `m + 1`, when it
//!   reaches 2, so that `y.hi` stays in `[1, 2)`) so that `eps`, the slack in
//!   `settle` and every intermediate stay normal; the result is `y·2^m` and `settle`'s subnormal
//!   branch rounds it once.
//! * `erf`, `|x| >= 6`: `erfc 6 < 2.2·10^-17 < 2^-55`, so `erf x` is `±1` moved by less than a
//!   quarter of the last place below 1 (`wide::near_one`, as the 384-bit path does from 12).
//! * `erfc`, `x <= −6`: `2` moved below by less than `2^-55` (`wide::beside`). `−6 < x < 1`:
//!   `1 + erf(−x)` or `1 − erf x` in double-double: the absolute error is `erf`'s plus the one
//!   rounding of the low parts (`2^-104`), so `eps = eps_erf + 2^-103`.
//! * `erfc`, `1 <= x < 27.25`: `e^(−x²)·Y(x)` with `Y = e^(x²)·erfc x` from `ERFCX_T` (base
//!   `0x3ff0`, `ERFCX_T_EPS` = 2^-71.40). `z = −x²` is exact as a double-double (`two_prod`);
//!   `|z.hi| < 742.6 < 746` and `|z.lo| <= 2^-44`, so [`fast64::exp_dd`] gives `E·2^m` within
//!   `|E|·(2^-49·|tail| + 2^-73.8)·2^m`. `Dd::mul` adds below `2^-103` relative (the dropped
//!   `lo·lo` and three roundings). So, relative, `eps = ERFCX_T_EPS + 2^-49·|tail| + 2^-72`
//!   (the cross terms are below `2^-140`). The product is scaled by a power of two into `[1, 2)`
//!   (exact) so that `settle` sees a normalized `y.hi`; from `x >= 27.25` the result is below
//!   `2^-1077`, outside `settle`'s subnormal branch, and the caller keeps those operands away.
//!
//! **Rounding.** `settle` (proof there) needs a normalized `y` with `y.hi != 0` and a true value
//! that is not a `double`; were the value a `double` or a midpoint, the interval
//! `[y − eps, y + eps]` would contain it and the test would refuse (`eps > 0`), so a settled
//! result is always inexact. `erf` is odd: for `x < 0` the magnitude is rounded in the mirrored
//! mode (up and down swapped) and negated, the flags the same.
//!
//! **Fallback rate.** In round-to-nearest the test refuses when a midpoint lies within `eps`:
//! about `2·eps/ulp` of the operands, at most `2·2^-71.3/2^-53 = 2^-17.3` for `erf` and `erfc`
//! above 1, and `2^-15.6` for `erfc` just below 1 (`1 − erf x` near 0.157 carries `erf`'s
//! absolute bound); the tests' ceiling of 1 in 10^4 is above all of these.

#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)] // slot indices bounded by the ranges checked before each lookup

use crate::coeffs::tables as t;
use crate::dd::{self, fast_two_sum, two_prod, two_sum, Dd};
use crate::fast64;
use crate::mp::Mp;
use crate::status::{Rounding, Status};
use crate::wide::{beside, near_one};

/// 2^-5: the series below, `ERF_T` from here.
const SMALL: f64 = f64::from_bits(0x3fa0_0000_0000_0000);
/// 2^-55: `x·q0` below.
const LINEAR: f64 = f64::from_bits(0x3c80_0000_0000_0000);
/// 2^-900: the linear case scales `x` into `[1, 2)` below.
const SCALED: f64 = f64::from_bits(0x07b0_0000_0000_0000);
/// 2^500.
const P500: f64 = f64::from_bits(0x5f30_0000_0000_0000);
/// 2^-100 (the linear case's relative bound).
const E100: f64 = f64::from_bits(0x39b0_0000_0000_0000);
/// 2^-103 (`1 ± erf` in double-double).
const E103: f64 = f64::from_bits(0x3980_0000_0000_0000);
/// 2^-104 (the series' last product, `3u² < 2^-104`).
const E104: f64 = f64::from_bits(0x3970_0000_0000_0000);
/// 2^-72 (`e^z`'s and the product's roundings).
const E72: f64 = f64::from_bits(0x3b70_0000_0000_0000);
/// 2^-49 (`e^z`'s series tail).
const TAIL: f64 = f64::from_bits(0x3ce0_0000_0000_0000);
/// `erf x` is within `2^-55` of `±1` from here.
const ERF_ONE: f64 = 6.0;
/// The upper end of `erfc`'s fast range (module doc).
pub const ERFC_MAX: f64 = 27.25;

/// The Taylor row `row` (layout in the module doc, `M = 4` double-doubles, degree 14) at `t`.
#[inline(always)]
fn taylor(row: &[u64; 19], t: f64) -> Dd {
    const M: usize = 4;
    const D: usize = 14;
    let c = |k: usize| f64::from_bits(row[k]);
    let mut r = c(D + M);
    for k in (M..D).rev() {
        r = r * t + c(k + M);
    }
    let (mut sh, mut sl) = (r, 0.0);
    for k in (0..M).rev() {
        let p = two_prod(sh, t);
        let h = two_sum(c(2 * k), p.hi);
        sl = h.lo + (c(2 * k + 1) + (p.lo + sl * t));
        sh = h.hi;
    }
    fast_two_sum(sh, sl)
}

/// The slot centre of `x` (module doc) and its index from `base`.
#[inline(always)]
fn slot(x: f64, base: u64) -> (usize, f64) {
    let b = x.to_bits();
    let i = usize::try_from((b >> 48).wrapping_sub(base)).unwrap_or(usize::MAX);
    (i, f64::from_bits((b & 0xffff_0000_0000_0000) | 1 << 47))
}

/// `erf x` for `2^-55 <= x < 2^-5` (module doc; the generator's bound `ERF_SMALL_EPS` assumes
/// this order): `z = x²` exact, `R = q2 + zh·(q3 + … + zh·q6)` in `f64`,
/// `S = q0 + q1·z + zh²·R` in double-double (`q1·z = two_prod(q1h, zh) + (q1h·zl + q1l·zh)`, the
/// small parts summed first and `zh²·R` last), then `x·S`.
#[inline(always)]
fn erf_small(x: f64) -> Dd {
    let q = |k: usize| f64::from_bits(t::ERF_SMALL[k]);
    let z = two_prod(x, x);
    let mut r = q(8);
    for k in (4..8).rev() {
        r = r * z.hi + q(k);
    }
    let p = two_prod(q(2), z.hi);
    let s = fast_two_sum(q(0), p.hi);
    let lo = ((s.lo + q(1)) + (p.lo + (q(2) * z.lo + q(3) * z.hi))) + (z.hi * z.hi) * r;
    let sum = fast_two_sum(s.hi, lo);
    let y = two_prod(x, sum.hi);
    fast_two_sum(y.hi, y.lo + x * sum.lo)
}

/// `erf x` for `0 < x < 6` as `(y, eps, m)`: `|erf x − y·2^m| <= eps·2^m` (module doc).
pub fn erf_kernel(x: f64) -> (Dd, f64, i32) {
    if x >= SMALL {
        let (i, c) = slot(x, 0x3fa0);
        let y = taylor(&t::ERF_T[i], x - c);
        return (y, dd::abs(y.hi) * f64::from_bits(t::ERF_T_EPS), 0);
    }
    if x >= LINEAR {
        let y = erf_small(x);
        return (y, dd::abs(y.hi) * (f64::from_bits(t::ERF_SMALL_EPS) + E104), 0);
    }
    let (xs, m) = if x < SCALED {
        // exact: both products are normal; then xs·2^m = x with xs in [1, 2)
        let w = x * P500 * P500;
        let e = i32::try_from((w.to_bits() >> 52) & 0x7ff).unwrap_or(0) - 1023;
        (f64::from_bits((w.to_bits() & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000), e - 1000)
    } else {
        (x, 0)
    };
    let p = two_prod(xs, f64::from_bits(t::ERF_SMALL[0]));
    let y = fast_two_sum(p.hi, p.lo + xs * f64::from_bits(t::ERF_SMALL[1]));
    // Scaled: keep y.hi in [1, 2) (exact halving) so `settle` sees a normalized significand.
    let (y, m) = if m != 0 && y.hi >= 2.0 { (y.scale(0.5), m + 1) } else { (y, m) };
    (y, dd::abs(y.hi) * E100, m)
}

/// `erfc x` for `1 <= x < ERFC_MAX` as `(y, eps, m)`, `1 <= y.hi < 2`: `|erfc x − y·2^m| <= eps·2^m`
/// (module doc), or `None` when `e^(−x²)`'s kernel declines.
pub fn erfc_kernel(x: f64) -> Option<(Dd, f64, i32)> {
    let s = two_prod(x, x);
    let (e, m, tail) = fast64::exp_dd(s.neg())?;
    let (i, c) = slot(x, 0x3ff0);
    let y = e.mul(taylor(&t::ERFCX_T[i], x - c));
    // y.hi = 2^k·f with f in [1, 2): scale to [1, 2) (exact), m + k.
    let k = i32::try_from((y.hi.to_bits() >> 52) & 0x7ff).unwrap_or(0) - 1023;
    let y = y.scale(dd::pow2(-k));
    let eps = dd::abs(y.hi) * (f64::from_bits(t::ERFCX_T_EPS) + TAIL * dd::abs(tail) + E72);
    Some((y, eps, m + k))
}

/// `mode` with up and down swapped (rounding `−a` is the negation of rounding `a` in it).
fn mirrored(mode: Rounding) -> Rounding {
    match mode {
        Rounding::Up => Rounding::Down,
        Rounding::Down => Rounding::Up,
        other => other,
    }
}

/// `erf x` for a finite nonzero `x`, correctly rounded in `mode`, or `None` (the caller runs the
/// accurate path).
pub fn erf(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    let ax = dd::abs(x);
    let neg = x < 0.0;
    if ax >= ERF_ONE {
        return Some(near_one::<f64>(neg, false, mode));
    }
    let (y, eps, m) = erf_kernel(ax);
    if neg {
        fast64::settle(y, eps, m, mirrored(mode)).map(|(v, st)| (-v, st))
    } else {
        fast64::settle(y, eps, m, mode)
    }
}

/// `erfc x` for a finite `x` with `2^-100 <= |x|` and `x < ERFC_MAX`, correctly rounded in
/// `mode`, or `None` (the caller runs the accurate path).
pub fn erfc(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    if x <= -ERF_ONE {
        return Some(beside::<f64>(Mp::from_i64(2), false, mode));
    }
    if x < 1.0 {
        let (y, eps, _) = erf_kernel(dd::abs(x));
        let y = if x < 0.0 { y } else { y.neg() };
        let s = fast_two_sum(1.0, y.hi);
        let r = fast_two_sum(s.hi, s.lo + y.lo);
        return fast64::settle(r, eps + E103, 0, mode);
    }
    let (y, eps, m) = erfc_kernel(x)?;
    fast64::settle(y, eps, m, mode)
}
