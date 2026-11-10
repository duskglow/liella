//! The quick phase of `double` `pow` (D-10.51, D-10.62; LIBM-F64-POW-QUICK): `x^y = e^(y·ln x)`
//! from the quick log (D-10.60) and the quick exp (D-10.54), with Ziv's rounding test (Ziv, ACM
//! TOMS 17(3), 1991) on a bound composed at run time, in front of today's in-place tier
//! [`super::pow_nearest`] (tier 2, unchanged) and the accurate path.
//!
//! **Domain.** [`super::pow_domain`] first (`x` positive, normal, finite, not 1; `|y| >= 2^-200`;
//! `y·2^11` not an integer): it refuses every operand whose `x^y` is a `double` or a midpoint, so
//! the exact cases stay ahead of this phase, and it gives `|y| < 2^40`. Then `2^-60 <= |th| < 708`
//! (the quick exp's domain, on `th` below) and `|y·r³| <= 2^-12` (below); outside them tier 2.
//!
//! **Method.**
//! * `ln x = lh + ll`: the quick log's reduction, table and evaluation (`quick_log`, D-10.60:
//!   `x = 2^e·m`, `r = c_j·m − 1` exact, `−ln c_j` and `ln 2` on the `2^-42` grid) with one more
//!   Taylor term (degree 10, `C10 = RN(−1/10)` in Estrin's `qb`), and the closing Fast2Sum, so
//!   `|ll| <= ulp(lh)/2`. It also returns `r3 = RN(RN(r²)·r)`.
//! * `y·lh = th + e` exactly (2MultFMA, or Dekker's product with Veltkamp's split in the SSE2
//!   build: `|y| < 2^40`, `|lh| < 745`, and `|y·lh| >= 2^-253` keep it exact), `tl = RN(y·ll + e)`
//!   (one FMA; two roundings in SSE2): `|y·(lh + ll) − (th + tl)| <= 2^-103·|th|`, `|tl| <=
//!   2^-51·|th| < 2^-41`.
//! * `e^(th + tl)` by the quick exp's kernel with `tl` folded into the reduction: `k` from `th`,
//!   `r1 = th − kd·L1` (exact, as there), `s = RN(tl − kd·L2)`, `rh = RN(r1 + s)`, `rl =
//!   RN(RN(r1 − rh) + s)`; the rest is the quick exp's code unchanged (`zh`, `zl`, `sc = 2^m`).
//!   `|(th + tl)·128/ln2 − kd| <= 1/2 + 2^-35 + 185·2^-41 < (1 + 2^-32)/2`, so `|rh| <= R' =
//!   (1 + 2^-32)·ln2/256 + 2^-61`; `|s| <= 2^-27 + 2^-41`, so the reduction's roundings are at
//!   most `3u·(2^-27 + 2^-41) + u·2^-61` (`u = 2^-53`; the quick exp had `3·2^-80`), and `|rl| <=
//!   2^-62 + 2^-79` as there. With these two changes every term of D-10.54's bound is recomputed
//!   (`pow_quick_constants_and_bounds`) and stays within its `RHO_E`: `RHO_FMA = 1.25·2^-68`,
//!   `RHO_SSE2 = 1.4375·2^-62`, relative to `|zh|`, of `e^(th + tl)` (scaled by `2^-m`).
//!
//! **Bound.** The log's absolute error, from D-10.60's terms with the extra term: `r³·q` within
//! `2.3u·|r|³` (`q` within `3.85u` of `Q`: three Estrin roundings on values near `|Q|`, `RN(1/3)`;
//! two roundings of `r³`; the SSE2 product one more; `|Q| <= 0.336`; the degree-10 term changes
//! these by under `0.01u`), `lo`'s rounding `0.343u·|r|³` plus terms below `2^-104·|ln x|`,
//! truncation `|r|^11/11/(1 − 2^-7) <= 2^-59.4·|r|³` (`|r| < 2^-7`): together at most `KC·|r|³`,
//! `KC = 2.75u`. Every other term (`vh + vl` within `2^-87·|r|`; `ln 2` and `−ln c_j` within
//! `2^-96` per unit of `|e|` and per row; `tl`'s and the sums' roundings) is below `2^-84·|ln x|`:
//! `|r| <= 1.44·|ln x|` in every case (A: `r = x − 1`; B: `|ln x| >= 2^-8 > |r|/1.44`; C: `|ln x|
//! >= 0.341·|e|`), the table terms vanish in case A (`c_j = 1`, `e = 0`) and in case B are
//! `2^-95 <= 2^-87·|ln x|`, and in case C `|e| + 1 <= 5.9·|ln x|`. The test
//! `pow_log_error_within_bound` checks `KC·|r3|·(1 + 2^-50) + 2^-84·|lh|` over the operands.
//!
//! So `t = y·ln x = th + tl + d` with `|d| <= D = KC'·|y·r3| + 2^-83·|th|` (`KC' = KC·(1 +
//! 2^-49)` covers `r3`'s two roundings, `|y·ln x| <= 1.001·|th|`), and `|x^y − (zh + zl)·sc| <=
//! |zh|·sc·rho` with `rho = RHO_E + D·(1 + 2^-38)` (`|e^d − 1| <= |d|·(1 + 2^-40)` for `|d| <=
//! 2^-40`, and `e^(th + tl) <= |zh|·sc·(1 + 2^-52)`). With `|y·r3| <= 2^-12` and `|th| < 708`,
//! `2^54·rho <= 2^-7.16`. [`super::near`]'s proof (de Dinechin, Lauter & Muller, RAIRO-ITA 41,
//! 2007) holds for any `c` with `c·(1 − 2^54·rho) > 1 + u` once `2^54·rho < 1/2` (the far side's
//! distance `H/2` then exceeds `rho·|zh|`); `zl = 0` passes rightly too. The least such `c` is
//! below `1 + (2^54·rho + u)/(1 − 2^-7)`, which is linear in `|y·r3|` and `|th|`: the `|th|`
//! part is bounded by its value at 708 (`2^54·2^-83·708/(1 − 2^-7) = 1.34·10^-6`) and folded
//! into the constant, so `c = RN(|y·r3|·A2 + C0)` (one FMA), `A2 = 89/16`, `C0` from the stated
//! `RHO_E` (`C0_FMA = 1 + 7.82·10^-5`, `C0_SSE2 = 1 + 0.00566`; two roundings of the sum, each
//! at most `u`, are in `C0`). `pow_quick_constants_and_bounds` checks `c·(1 − 2^54·rho) > 1 + u`
//! exactly at `|y·r3| = 0` and `2^-12` (the product is concave in `|y·r3|`). `x^y` is neither a
//! `double` nor a midpoint (the domain), and the result is normal and finite (`th` in the quick
//! exp's domain, `|tl|` far below the margins of its `m = −1022` argument).
//!
//! Expected fallback: about `c − 1` (FMA `7.8·10^-5` plus `5.6·|y·r3|`; SSE2 `0.57%`). Both
//! builds are correctly rounded wherever they answer, so they return the same values.

#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)] // bit arithmetic on bounded values (above), indices masked to 0..127

use super::pow_domain;
use super::quick::{ABS, C3 as E3, C4 as E4, C5 as E5, C6 as E6, INV, KBIAS, LO_BITS, NL1, NL2, SHIFT, SPAN as E_SPAN};
use super::quick_log::{C3 as L3, C4 as L4, C5 as L5, C6 as L6, C7 as L7, C8 as L8, C9 as L9, KH, KL, OFF};
use crate::coeffs::tables as t;
use crate::dd;

/// `RN(−1/10)`: the log's degree-10 term.
const L10: f64 = -0.1;
/// `2^-12`: the largest `|y·r3|` the quick phase takes (module doc).
const YR_MAX: f64 = f64::from_bits(0x3f30_0000_0000_0000);
/// The rounding test's multiplier `c = RN(|y·r3|·A2 + C0)` (module doc; checked exactly by the
/// `bounds` test).
const A2: f64 = 5.5625;
pub(crate) const C0_SSE2: f64 = f64::from_bits(0x3ff0_172f_c183_060e);
pub(crate) const C0_FMA: f64 = f64::from_bits(0x3ff0_0052_060c_1832);
/// `KC = 2.75·2^-53`: the log's error is within `KC·|r|³ + 2^-84·|ln x|` (module doc).
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub const KC: f64 = 2.75 / 9_007_199_254_740_992.0;

/// `a·b + c` in two roundings (the SSE2 build).
#[inline(always)]
fn ma(a: f64, b: f64, c: f64) -> f64 {
    a * b + c
}

/// The low part of `eh·rh` the SSE2 build drops (the quick exp's bound term 1).
#[inline(always)]
fn no_lo(_eh: f64, _rh: f64, _u: f64) -> f64 {
    0.0
}

// The kernel in two builds (D-10.25, D-10.50): `$ma` is `a·b + c` (two roundings, or one FMA),
// `$red` the log's exact reduction, `$sq` its half-square, `$prod` the exact product, `$lo` the
// low part of `eh·rh` (0, or exact by one FMA), `$c0` the rounding test's constant.
macro_rules! quick {
    ($(#[$a:meta])*; $(#[$i:meta])*; $ma:path; $red:path; $sq:path; $prod:path; $lo:path; $c0:expr) => {
/// `(lh, ll, r3)`: `ln x = lh + ll` within `KC·|r|³ + 2^-84·|ln x|`, `r3 = RN(RN(r²)·r)`, for a
/// positive normal finite `x` (module doc).
$(#[$a])*
$(#[$i])*
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub fn pow_log_parts(x: f64) -> (f64, f64, f64) {
    let ix = x.to_bits();
    let tmp = ix.wrapping_sub(OFF);
    let e = i64::from_ne_bytes(tmp.to_ne_bytes()) >> 52;
    let mb = ix.wrapping_sub(tmp & (0xfff << 52));
    let row = &t::LOG_Q128[usize::try_from((tmp >> 45) & 127).unwrap_or(0)];
    let r = $red(f64::from_bits(mb), mb, f64::from_bits(row[0]), f64::from_bits(row[1]));
    let ef = f64::from(i32::try_from(e).unwrap_or(0));
    let th = $ma(ef, KH, f64::from_bits(row[2]));
    let tl = $ma(ef, KL, f64::from_bits(row[3]));
    let (vh, vl, r2) = $sq(r);
    let p01 = $ma(r, L4, L3);
    let p23 = $ma(r, L6, L5);
    let p45 = $ma(r, L8, L7);
    let r4 = r2 * r2;
    let qa = $ma(r2, p23, p01);
    let qb = $ma(r2, $ma(r, L10, L9), p45);
    let q = $ma(r4, qb, qa);
    let r3 = r2 * r;
    let sh = th + vh;
    let sl = (th - sh) + vh;
    let lo = $ma(r3, q, (sl + vl) + tl);
    let lh = sh + lo;
    (lh, (sh - lh) + lo, r3)
}

/// `(zh, zl, sc, c)`: `x^y = (zh + zl)·sc` within the bound [`super::near`] needs for the
/// multiplier `c`, for `x`, `y` in [`super::pow_domain`] (unchecked here) with `2^-60 <= |th| <
/// 708` and `|y·r3| <= 2^-12`, else `None` (module doc).
$(#[$a])*
$(#[$i])*
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub fn pow_quick_parts(x: f64, y: f64) -> Option<(f64, f64, f64, f64)> {
    let (lh, ll, r3) = pow_log_parts(x);
    let yr = dd::abs(y * r3);
    let p = $prod(y, lh);
    let th = p.hi;
    if (th.to_bits() & ABS).wrapping_sub(LO_BITS) >= E_SPAN || yr > YR_MAX {
        return None;
    }
    let tl = $ma(y, ll, p.lo);
    let tt = $ma(th, INV, SHIFT);
    let kd = tt - SHIFT;
    let kb = tt.to_bits().wrapping_sub(KBIAS);
    let r1 = $ma(kd, NL1, th);
    let s = $ma(kd, NL2, tl);
    let rh = r1 + s;
    let rl = (r1 - rh) + s;
    let r2 = rh * rh;
    let pa = $ma(rh, E3, 0.5);
    let pb = $ma(rh, E5, E4);
    let pc = $ma(r2, E6, pb);
    let q = r2 * $ma(r2, pc, pa);
    let j = usize::try_from(kb & 127).unwrap_or(0);
    let eh = f64::from_bits(t::EXP2_J128[j][0]);
    let el = f64::from_bits(t::EXP2_J128[j][1]);
    let u = eh * rh;
    let yh = eh + u;
    let e1 = (eh - yh) + u;
    let w = $ma(eh, rl + q, el * (1.0 + rh));
    let yl = (e1 + $lo(eh, rh, u)) + w;
    let zh = yh + yl;
    let zl = (yh - zh) + yl;
    Some((zh, zl, f64::from_bits((kb >> 7) << 52), $ma(yr, A2, $c0)))
}

/// The correctly rounded `x^y` (round to nearest) when the quick phase's test decides it, else
/// `None` (outside [`super::pow_domain`] too; the caller runs [`super::pow_nearest`]).
$(#[$a])*
$(#[$i])*
pub fn pow_quick(x: f64, y: f64) -> Option<f64> {
    if !pow_domain(x, y) {
        return None;
    }
    let (zh, zl, sc, c) = pow_quick_parts(x, y)?;
    if zh + zl * c == zh { Some(zh * sc) } else { None }
}

/// `pow`'s in-place fast path (D-10.51): the exact cases and specials refused first
/// ([`super::pow_domain`]), the quick phase, then on a miss tier 2 (`fast64::pow_nearest` in the
/// same build, out of line). `None` when tier 2 declines too: the caller then runs the accurate
/// path.
$(#[$a])*
$(#[$i])*
pub fn pow_tiered(x: f64, y: f64) -> Option<f64> {
    if !pow_domain(x, y) {
        return None;
    }
    if let Some((zh, zl, sc, c)) = pow_quick_parts(x, y) {
        if zh + zl * c == zh {
            return Some(zh * sc);
        }
    }
    tier2(x, y)
}
    };
}

/// Tier 2 of [`pow_tiered`] (the SSE2 build of [`super::pow_nearest`]), out of line and cold so
/// the quick phase keeps its registers.
#[cold]
#[inline(never)]
fn tier2(x: f64, y: f64) -> Option<f64> {
    super::pow_nearest(x, y)
}

quick!(; #[inline(always)]; ma; super::quick_log::reduce; super::quick_log::half_square; dd::two_prod; no_lo; C0_SSE2);

/// The quick phase's FMA build (D-10.25, D-10.50): callable only on a CPU with FMA whose AVX state
/// the OS saves; the boundary chooses it once at startup (`dispatch::fma`).
// ARCH(x86_64): the run-time FMA build; FMA is base ISA on aarch64, so one build there
#[cfg(target_arch = "x86_64")]
pub mod fused {
    use super::{dd, pow_domain, t, A2, ABS, C0_FMA, E3, E4, E5, E6, INV, KBIAS, KH, KL, L10, L3, L4, L5, L6, L7, L8, L9, LO_BITS, NL1, NL2, OFF, SHIFT, E_SPAN, YR_MAX};
    use crate::fast64::quick_log::fused::{half_square, reduce};
    // ARCH(x86_64): the scalar FMA intrinsics
    use core::arch::x86_64::{_mm_cvtsd_f64, _mm_fmadd_sd, _mm_fmsub_sd, _mm_set_sd};

    /// `a·b + c` in one rounding.
    #[target_feature(enable = "fma")]
    #[inline]
    fn ma(a: f64, b: f64, c: f64) -> f64 {
        _mm_cvtsd_f64(_mm_fmadd_sd(_mm_set_sd(a), _mm_set_sd(b), _mm_set_sd(c)))
    }

    /// `eh·rh − u` exactly for `u = RN(eh·rh)` (2MultFMA; the product is far from underflow).
    #[target_feature(enable = "fma")]
    #[inline]
    fn lo(eh: f64, rh: f64, u: f64) -> f64 {
        _mm_cvtsd_f64(_mm_fmsub_sd(_mm_set_sd(eh), _mm_set_sd(rh), _mm_set_sd(u)))
    }

    /// Tier 2 of [`pow_tiered`] in the FMA build (`fast64::fused::pow_nearest`), out of line and
    /// cold.
    #[target_feature(enable = "fma")]
    #[cold]
    #[inline(never)]
    fn tier2(x: f64, y: f64) -> Option<f64> {
        crate::fast64::fused::pow_nearest(x, y)
    }

    quick!(
        #[doc = ""]
        #[doc = "# Safety"]
        #[doc = ""]
        #[doc = "The CPU must support FMA and the OS must save the AVX state (the `#[target_feature]` contract)."]
        #[target_feature(enable = "fma")]; #[inline]; ma; reduce; half_square; dd::two_prod_fma; lo; C0_FMA
    );
}

#[cfg(test)]
mod tests;
