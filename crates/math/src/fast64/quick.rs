//! The quick phase of `double` `exp` (D-10.51, D-10.54; LIBM-F64-EXP-QUICK): a cheap evaluation with
//! a proven relative bound and Ziv's rounding test (Ziv, ACM TOMS 17(3), 1991), in front of today's
//! in-place tier [`super::exp_nearest`] (tier 2, unchanged) and the accurate path. Not wired into
//! the C entry yet (LIBM-F64-EXP-QUICK-WIRE): `None` means "run tier 2".
//!
//! **Method.** `x = (128m + j)·ln2/128 + r` (Tang, "Table-driven implementation of the exponential
//! function in IEEE floating-point arithmetic", ACM TOMS 15(2), 1989; N = 128):
//! * `t = RN(x·INV + SHIFT)` with `INV = RN(128/ln2)`, `SHIFT = 1.5·2^52` (one FMA in the FMA
//!   build); `t` lies in `[2^52, 2^53)`, so `kd = t − SHIFT` is the integer `k` exactly and `k` is
//!   read from `t`'s bits (`bits(t) − bits(SHIFT)`, no conversion instruction): `j = k mod 128`,
//!   `2^m` from `(k + 1023·128) >> 7`.
//! * Cody–Waite (Cody & Waite, Software Manual for the Elementary Functions, 1980):
//!   `ln2/128 = L1 + L2`, `L1` of 36 bits (`k·L1` exact for `|k| < 2^17`). `r1 = x − kd·L1` is
//!   exact: for `k != 0`, `|x| >= 2^-9` makes `x` a multiple of `2^-61`, `kd·L1` is a multiple of
//!   `2^-43`, and `|r1| < 2^-8` (the Sterbenz-type grid argument). `rh = RN(r1 − kd·L2)`,
//!   `rl ≈ (r1 − rh) − kd·L2` (the low part kept).
//! * `T_j = 2^(j/128) = th + tl` within `2^-105·th` (`EXP2_J128`, `cargo xtask tables`, 2 KiB).
//! * `q ≈ e^rh − 1 − rh` by Taylor's degree 6 in Estrin's scheme (Estrin 1960; Knuth, TAOCP 2,
//!   4.6.4): `r2 = rh²`, `pa = rh·C3 + 1/2`, `pb = rh·C5 + C4`, `pc = r2·C6 + pb`,
//!   `q = r2·(r2·pc + pa)`.
//! * `y = T·(1 + r + q)`: `u = RN(th·rh)`; `th + u = yh + e1` by Fast2Sum (`|u| < th`);
//!   `w = th·(rl + q) + tl·(1 + rh)`; `yl = (e1 + ul) + w` with `ul = th·rh − u` exactly by one
//!   FMA in the FMA build and `ul = 0` (dropped) in the SSE2 build; `(zh, zl)` = Fast2Sum of
//!   `(yh, yl)` (Dekker 1971; Muller et al., Handbook of Floating-Point Arithmetic, 2nd ed. 2018).
//! * Rounding: the one-sided test of [`super::near`] (de Dinechin, Lauter & Muller, RAIRO-ITA 41,
//!   2007) with `c` the least double with `c·(1 − 2^54·rho) > 1 + 2^-53`; `zh·2^m` is formed
//!   beside the test, not after it. Its proof needs `e^x` neither a double nor a midpoint (true for
//!   every double `x != 0`: Lindemann–Weierstrass) and `zh·2^m` normal (below).
//! * Domain `2^-60 <= |x| < 708` (today's `exp_near`; one integer subtract and compare on the
//!   bits, NaN and infinities fall outside): `|k| <= 130740 < 2^17`, `-1022 <= m <= 1021`, and
//!   `zh >= 1` whenever `m = -1022` (`j >= 76` there), so `zh·2^m` is normal and finite.
//!
//! **Bound** (relative to `th`, `u = 2^-53`, `|r| <= R = (1/2 + 2^-35)·ln2/128 < 2^-8.5287` since
//! `|x·128/ln2 − kd| <= 1/2 + 2^-35`, `|rh| <= R + 2^-62`, `|rl| <= 2^-62 + 2^-79`):
//! 1. rounding of `th·rh` (SSE2 build only): `u·R = 2^-61.529`;
//! 2. polynomial evaluation in Estrin's order: `pa`, `pb`, `pc` and the inner sum carry at most
//!    `1.002u` absolute on a value `>= 0.4995`, `r2` and the last product `u` each: `q` within
//!    `4.01u·|q|` (SSE2; fewer roundings with FMA), stated `4.05u·qmax`, `qmax = 2^-18.056`:
//!    `2^-69.04`;
//! 3. the low sum: `RN(rl + q)`, `RN(th·a)` (SSE2 only), the sum forming `w` and the one forming
//!    `yl`, each `u·(qmax + 2^-62) = 2^-71.06`: 4 of them `2^-69.06` (SSE2), 3 `2^-69.47` (FMA);
//!    `e1 + ul` and `tl·(1 + rh)`'s roundings act on `2^-52` quantities (`< 2^-104`);
//! 4. neglected `rl·(e^rh − 1)`: `2^-70.53`; neglected `tl·(e^r − 1 − rh)`: `2^-71.06`;
//! 5. truncation `R^7/7!·e^R`: `2^-72.00`;
//! 6. reduction `|kd|·|ln2/128 − L1 − L2| + ` the two roundings forming `rl` (`<= 3·2^-80`, the
//!    terms `kd·L2` below `2^-27`): `2^-78.41`; the table `2^-105`.
//!
//! Divided by `|zh|/th >= e^-R·(1 − 2^-60) = 0.99729`: FMA `2^-67.727`, stated `RHO_FMA =
//! 1.25·2^-68` (`2^-67.678`); SSE2 `2^-61.503`, stated `RHO_SSE2 = 1.4375·2^-62` (`2^-61.476`).
//! The `bounds` tests recompute every term from the constants. Expected fallback `≈ c − 1`: SSE2
//! 0.565%, FMA 0.0076%.
//!
//! **Design targets** (D-10.51): instructions per call with the fallback included FMA <= 80,
//! SSE2 <= 95; FP operations 31 (FMA) and 39 (SSE2) including the two compares; critical path
//! 16 dependent FP operations plus the compare (FMA: `t, kd, r1, rh, r2, pc, inner, q, rl + q, w,
//! yl, zh, yh − zh, zl, zl·c, zh + w`), 21 in the SSE2 build (each FMA two operations).
//!
//! Both builds are correctly rounded wherever they answer, so they return the same values; their
//! fallback sets differ.

#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)] // bit arithmetic on bounded values (above), indices masked to 0..127

use crate::coeffs::tables as t;

/// `1.5·2^52`: `x·INV + SHIFT` lies in `[2^52, 2^53)`, where every double is an integer.
pub(super) const SHIFT: f64 = 6_755_399_441_055_744.0;
/// `bits(SHIFT) − 1023·128`: `bits(t)` minus this is `k + 1023·128 >= 0`; shifted right by 7, the
/// biased exponent of `2^m`.
pub(super) const KBIAS: u64 = 0x4338_0000_0000_0000 - 1023 * 128;
pub(super) const INV: f64 = f64::from_bits(t::SCALE_128[0]);
pub(super) const NL1: f64 = -f64::from_bits(t::LN2_128_CW[0]);
pub(super) const NL2: f64 = -f64::from_bits(t::LN2_128_CW[1]);
/// `RN(1/3!)` .. `RN(1/6!)` (checked by the `bounds` tests).
pub(super) const C3: f64 = f64::from_bits(0x3fc5_5555_5555_5555);
pub(super) const C4: f64 = f64::from_bits(0x3fa5_5555_5555_5555);
pub(super) const C5: f64 = f64::from_bits(0x3f81_1111_1111_1111);
pub(super) const C6: f64 = f64::from_bits(0x3f56_c16c_16c1_6c17);
/// `bits(2^-60)` and `bits(708) − bits(2^-60)`: the domain as one unsigned compare on `|x|`'s bits.
pub(super) const LO_BITS: u64 = 0x3c30_0000_0000_0000;
pub(super) const SPAN: u64 = 0x4086_2000_0000_0000 - LO_BITS;
pub(super) const ABS: u64 = 0x7fff_ffff_ffff_ffff;

/// The stated bounds (module doc) and their [`super::near`] multipliers: the least doubles with
/// `c·(1 − 2^54·rho) > 1 + 2^-53` (checked exactly by the `bounds` tests).
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub const RHO_SSE2: f64 = f64::from_bits(0x3c17_0000_0000_0000);
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub const RHO_FMA: f64 = f64::from_bits(0x3bb4_0000_0000_0000);
pub(crate) const C_SSE2: f64 = f64::from_bits(0x3ff0_1721_3fcb_b4d5);
pub(crate) const C_FMA: f64 = f64::from_bits(0x3ff0_0050_0190_07d1);

/// `a·b + c` in two roundings (the SSE2 build).
#[inline(always)]
fn ma(a: f64, b: f64, c: f64) -> f64 {
    a * b + c
}

/// The low part of `th·rh` the SSE2 build drops (bound term 1).
#[inline(always)]
fn no_lo(_th: f64, _rh: f64, _u: f64) -> f64 {
    0.0
}

// The kernel in two builds (D-10.25, D-10.50): `$ma` is `a·b + c` (two roundings, or one FMA),
// `$lo` the low part of `th·rh` (0, or exact by one FMA), `$c` the rounding test's multiplier.
macro_rules! quick {
    ($(#[$a:meta])*; $(#[$i:meta])*; $ma:path; $lo:path; $c:expr) => {
/// `e^x = (zh + zl)·sc` within `rho·|zh|·sc` for `2^-60 <= |x| < 708`, else `None` (module doc).
$(#[$a])*
$(#[$i])*
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub fn exp_quick_parts(x: f64) -> Option<(f64, f64, f64)> {
    if (x.to_bits() & ABS).wrapping_sub(LO_BITS) >= SPAN {
        return None;
    }
    let tt = $ma(x, INV, SHIFT);
    let kd = tt - SHIFT;
    let kb = tt.to_bits().wrapping_sub(KBIAS);
    let r1 = $ma(kd, NL1, x);
    let rh = $ma(kd, NL2, r1);
    let rl = $ma(kd, NL2, r1 - rh);
    let r2 = rh * rh;
    let pa = $ma(rh, C3, 0.5);
    let pb = $ma(rh, C5, C4);
    let pc = $ma(r2, C6, pb);
    let q = r2 * $ma(r2, pc, pa);
    let j = usize::try_from(kb & 127).unwrap_or(0);
    let th = f64::from_bits(t::EXP2_J128[j][0]);
    let tl = f64::from_bits(t::EXP2_J128[j][1]);
    let u = th * rh;
    let yh = th + u;
    let e1 = (th - yh) + u;
    let w = $ma(th, rl + q, tl * (1.0 + rh));
    let yl = (e1 + $lo(th, rh, u)) + w;
    let zh = yh + yl;
    let zl = (yh - zh) + yl;
    Some((zh, zl, f64::from_bits((kb >> 7) << 52)))
}

/// The correctly rounded `e^x` (round to nearest) when the quick phase's test decides it, else
/// `None` (the caller runs [`super::exp_nearest`]).
$(#[$a])*
$(#[$i])*
pub fn exp_quick(x: f64) -> Option<f64> {
    let (zh, zl, sc) = exp_quick_parts(x)?;
    let v = zh * sc;
    let w = zl * $c;
    if zh + w == zh && w != 0.0 { Some(v) } else { None }
}

/// `exp`'s in-place fast path (D-10.51): the quick phase, then on a miss tier 2
/// (`fast64::exp_nearest` in the same build, out of line). `None` outside `2^-60 <= |x| < 708`
/// (tier 2's own domain, so it is not tried) or when tier 2 declines too: the caller then runs
/// the accurate path.
$(#[$a])*
$(#[$i])*
pub fn exp_tiered(x: f64) -> Option<f64> {
    let (zh, zl, sc) = exp_quick_parts(x)?;
    let v = zh * sc;
    let w = zl * $c;
    if zh + w == zh && w != 0.0 {
        return Some(v);
    }
    tier2(x)
}
    };
}

/// Tier 2 of [`exp_tiered`] (the SSE2 build of [`super::exp_nearest`]), out of line and cold so
/// the quick phase keeps its registers: about 0.5% of calls reach it.
#[cold]
#[inline(never)]
fn tier2(x: f64) -> Option<f64> {
    super::exp_nearest(x)
}

quick!(; #[inline(always)]; ma; no_lo; C_SSE2);

/// The quick phase's FMA build (D-10.25, D-10.50): callable only on a CPU with FMA whose AVX state
/// the OS saves; the boundary chooses it once at startup (`dispatch::fma`).
// ARCH(x86_64): the run-time FMA build; FMA is base ISA on aarch64, so one build there
#[cfg(target_arch = "x86_64")]
pub mod fused {
    use super::{t, ABS, C3, C4, C5, C6, C_FMA, INV, KBIAS, LO_BITS, NL1, NL2, SHIFT, SPAN};
    // ARCH(x86_64): the scalar FMA intrinsics
    use core::arch::x86_64::{_mm_cvtsd_f64, _mm_fmadd_sd, _mm_fmsub_sd, _mm_set_sd};

    /// `a·b + c` in one rounding.
    #[target_feature(enable = "fma")]
    #[inline]
    fn ma(a: f64, b: f64, c: f64) -> f64 {
        _mm_cvtsd_f64(_mm_fmadd_sd(_mm_set_sd(a), _mm_set_sd(b), _mm_set_sd(c)))
    }

    /// `th·rh − u` exactly for `u = RN(th·rh)` (2MultFMA; the product is far from underflow).
    #[target_feature(enable = "fma")]
    #[inline]
    fn lo(th: f64, rh: f64, u: f64) -> f64 {
        _mm_cvtsd_f64(_mm_fmsub_sd(_mm_set_sd(th), _mm_set_sd(rh), _mm_set_sd(u)))
    }

    /// Tier 2 of [`exp_tiered`] in the FMA build (`fast64::fused::exp_nearest`), out of line and
    /// cold: under 0.01% of calls reach it.
    #[target_feature(enable = "fma")]
    #[cold]
    #[inline(never)]
    fn tier2(x: f64) -> Option<f64> {
        crate::fast64::fused::exp_nearest(x)
    }

    quick!(
        #[doc = ""]
        #[doc = "# Safety"]
        #[doc = ""]
        #[doc = "The CPU must support FMA and the OS must save the AVX state (the `#[target_feature]` contract)."]
        #[target_feature(enable = "fma")]; #[inline]; ma; lo; C_FMA
    );
}

#[cfg(test)]
mod tests;
