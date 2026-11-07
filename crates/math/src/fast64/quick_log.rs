//! The quick phase of `double` `log` (D-10.51, D-10.60; LIBM-F64-LOG-QUICK): a cheap evaluation
//! with a proven relative bound and Ziv's rounding test (Ziv, ACM TOMS 17(3), 1991), in front of
//! today's in-place tier [`super::log_nearest`] (tier 2, unchanged) and the accurate path.
//!
//! **Method** (Tang, "Table-driven implementation of the logarithm function in IEEE
//! floating-point arithmetic", ACM TOMS 16(4), 1990, with reciprocals of few bits so that the
//! reduction is exact, after Gal & Bachelis, ACM TOMS 17(1), 1991):
//! * `x = 2^e·m` with `m` in `[45/64, 90/64)`, read from the bits: `tmp = bits(x) − bits(45/64)`,
//!   `e = tmp >> 52` (arithmetic), `bits(m) = bits(x) − (e << 52)`, `j = (tmp >> 45) mod 128`.
//!   Interval `j` is `[F_j, F_j + w_j)`, `w_j = 2^-8` below 1 and `2^-7` above (`F_j` is `m` with
//!   its 45 low fraction bits cleared). Domain: `x` positive, normal, finite (one unsigned
//!   compare); `e` in `[-1022, 1024]`.
//! * `r = c_j·m − 1` **exactly**: `c_j` has 8 significant bits, `m − F_j = f` is exact (Sterbenz)
//!   with at most 45 bits, so `c_j·f` is exact; `b_j = c_j·F_j − 1` is exact (16 bits) and stored;
//!   `r = c_j·f + b_j` is a multiple of `2^-60` with `|r| < 2^-7`, so representable: the SSE2
//!   build forms it in three exact operations, the FMA build as `fma(m, c_j, −1)` (one rounding
//!   of an exact value). `|r| <= 2^-7.48` except on the two intervals next to 1, where `c_j = 1`
//!   and `r = m − 1` in `[−2^-8, 2^-7)`.
//! * `ln x = e·ln2 + (−ln c_j) + log1p(r)`. `−ln c_j = Lh + Ll` and `ln2 = Kh + Kl` with `Lh`,
//!   `Kh` multiples of `2^-42` (`LOG_Q128`, `LN2_Q`, `cargo xtask tables`), so
//!   `th = e·Kh + Lh` is exact (`|th| < 2^10`, 52 bits); `tl = RN(e·Kl + Ll)`.
//! * `log1p(r) ≈ r − r²/2 + r³·Q(r)`, `Q` Taylor's `1/3 − r/4 + … + r⁶/9` (degree 9 in all;
//!   log's coefficients do not decay, so degree 5-6 would leave `2^-45` near 1), in Estrin's scheme
//!   (Estrin 1960; Knuth, TAOCP 2, 4.6.4). `r − r²/2 = vh + vl` almost exactly: FMA build
//!   `r² = s + s'` by 2MultFMA; SSE2 build `r = rh + rl` with `rh = RN(r + 1.5·2^19) − 1.5·2^19`
//!   (a multiple of `2^-33`, 26 bits, so `rh²/2` is exact) and `r² = rh² + rl·(r + rh)`; then
//!   Fast2Sum (Dekker 1971) of `r` and the exact half-square.
//! * `(sh, sl) = Fast2Sum(th, vh)` (`th = 0`, or `|th| >= 0.0077 > |vh|`, checked per row);
//!   `lo = r³·q + ((sl + vl) + tl)`; `(zh, zl) = Fast2Sum(sh, lo)`.
//! * Rounding: `zh` when `zh + zl·C == zh`, `C` the least double with `C·(1 − 2^54·RHO) > 1 +
//!   2^-53` (de Dinechin, Lauter & Muller, RAIRO-ITA 41, 2007). Its proof needs `ln x` neither a
//!   double nor a midpoint: true for every double `x != 1` (Lindemann–Weierstrass); at `x = 1`
//!   every term is `+0` and `+0` is the answer. `zl = 0` passes: then `|ln x − zh| <=
//!   RHO·|zh| < ulp(zh)/4`.
//!
//! **Bound** (`u = 2^-53`, relative to `|ln x|`; the `bounds` test recomputes it):
//! * A, `e = 0` and `c_j = 1` (`ln x = log1p(r)`, `|r| < 2^-7`): truncation `|r|^9/10/(1 −
//!   2^-8) <= 2^-66.31`; `r³·q` within `2.3u·|r|³` (coefficients `RN(1/n)` within `2^-55`,
//!   the Estrin roundings `1.01u·|Q|`, `r³` two roundings, the SSE2 product one more; `|Q| <=
//!   0.3360`), relative `2.3u·r²/(1 − 2^-8) <= 2^-65.79`; `lo`'s rounding `u·0.34·r²`;
//!   `vh + vl` within `2^-87·|r|`: total `2^-64.92`.
//! * B, `e = 0`, `c_j != 1` (`|ln x| >= 2^-8`, `|r| <= 2^-7.48`): the same absolute terms plus
//!   `Ll`'s and `tl`'s roundings (`< 2^-95`), at worst on row 74: `2^-67.64`.
//! * C, `e != 0` (`|ln x| >= 0.341·|e|`): `2^-72.4` plus `|e|·2^-95` for `Kl`.
//! Stated `RHO = 2^-64` for both builds (the FMA build has fewer roundings). Expected fallback
//! `≈ C − 1 = 0.098%` of uniformly spread significands; the fallback test measures it.
//!
//! Both builds are correctly rounded wherever they answer, so they return the same values.

#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)] // bit arithmetic on bounded values (above), indices masked to 0..127

use crate::coeffs::tables as t;

/// `bits(2^-1022)` and `bits(+inf) − bits(2^-1022)`: positive normal finite `x` as one unsigned
/// compare on its bits.
const MIN_NORMAL: u64 = 0x0010_0000_0000_0000;
const SPAN: u64 = 0x7ff0_0000_0000_0000 - MIN_NORMAL;
/// `bits(45/64)`: the low end of `m`'s range; its 45 low fraction bits are 0, so the intervals
/// line up with `F_j`.
const OFF: u64 = 0x3fe6_8000_0000_0000;
/// The 45 fraction bits below the interval index.
const LOW45: u64 = (1 << 45) - 1;
const KH: f64 = f64::from_bits(t::LN2_Q[0]);
const KL: f64 = f64::from_bits(t::LN2_Q[1]);
/// `1.5·2^19`: `RN(r + SPLIT) − SPLIT` is `r` rounded to a multiple of `2^-33` for `|r| < 2^18`.
const SPLIT: f64 = 786_432.0;
/// `RN(±1/n)`, n = 3..9 (checked by the `bounds` test).
const C3: f64 = 1.0 / 3.0;
const C4: f64 = -0.25;
const C5: f64 = 1.0 / 5.0;
const C6: f64 = -1.0 / 6.0;
const C7: f64 = 1.0 / 7.0;
const C8: f64 = -0.125;
const C9: f64 = 1.0 / 9.0;

/// The stated bound (module doc) and its rounding-test multiplier: the least double with
/// `C·(1 − 2^54·RHO) > 1 + 2^-53` (checked exactly by the `bounds` test).
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub const RHO: f64 = f64::from_bits(0x3bf0_0000_0000_0000);
pub(crate) const C: f64 = f64::from_bits(0x3ff0_0401_0040_1005);

/// `a·b + c` in two roundings (the SSE2 build).
#[inline(always)]
fn ma(a: f64, b: f64, c: f64) -> f64 {
    a * b + c
}

/// `r = c·m − 1` exactly (SSE2: `c·(m − F) + b`, each step exact; module doc).
#[inline(always)]
fn reduce(m: f64, mb: u64, c: f64, b: f64) -> f64 {
    c * (m - f64::from_bits(mb & !LOW45)) + b
}

/// `(vh, vl, RN(r²))` with `vh + vl = r − r²/2` within `2^-87·|r|` (SSE2: the split of `r`).
#[inline(always)]
fn half_square(r: f64) -> (f64, f64, f64) {
    let rh = (r + SPLIT) - SPLIT;
    let rl = r - rh;
    let hh = (-0.5 * rh) * rh;
    let vh = r + hh;
    let vl = ((r - vh) + hh) - (0.5 * rl) * (r + rh);
    (vh, vl, r * r)
}

// The kernel in two builds (D-10.25, D-10.50): `$ma` is `a·b + c` (two roundings, or one FMA),
// `$red` the exact reduction, `$sq` the half-square.
macro_rules! quick {
    ($(#[$a:meta])*; $(#[$i:meta])*; $ma:path; $red:path; $sq:path) => {
/// `ln x = zh + zl` within `RHO·|zh|` for positive normal finite `x`, else `None` (module doc).
$(#[$a])*
$(#[$i])*
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub fn log_quick_parts(x: f64) -> Option<(f64, f64)> {
    let ix = x.to_bits();
    if ix.wrapping_sub(MIN_NORMAL) >= SPAN {
        return None;
    }
    let tmp = ix.wrapping_sub(OFF);
    let e = tmp.cast_signed() >> 52;
    let mb = ix.wrapping_sub(tmp & (0xfff << 52));
    let row = &t::LOG_Q128[usize::try_from((tmp >> 45) & 127).unwrap_or(0)];
    let r = $red(f64::from_bits(mb), mb, f64::from_bits(row[0]), f64::from_bits(row[1]));
    let ef = f64::from(i32::try_from(e).unwrap_or(0));
    let th = $ma(ef, KH, f64::from_bits(row[2]));
    let tl = $ma(ef, KL, f64::from_bits(row[3]));
    let (vh, vl, r2) = $sq(r);
    let p01 = $ma(r, C4, C3);
    let p23 = $ma(r, C6, C5);
    let p45 = $ma(r, C8, C7);
    let r4 = r2 * r2;
    let qa = $ma(r2, p23, p01);
    let qb = $ma(r2, C9, p45);
    let q = $ma(r4, qb, qa);
    let r3 = r2 * r;
    let sh = th + vh;
    let sl = (th - sh) + vh;
    let lo = $ma(r3, q, (sl + vl) + tl);
    let zh = sh + lo;
    let zl = (sh - zh) + lo;
    Some((zh, zl))
}

/// The correctly rounded `ln x` (round to nearest) when the quick phase's test decides it, else
/// `None` (the caller runs [`super::log_nearest`]).
$(#[$a])*
$(#[$i])*
pub fn log_quick(x: f64) -> Option<f64> {
    let (zh, zl) = log_quick_parts(x)?;
    if zh + zl * C == zh { Some(zh) } else { None }
}

/// `log`'s in-place fast path (D-10.51): the quick phase, then on a miss or outside its domain
/// tier 2 (`fast64::log_nearest` in the same build, out of line). `None` when tier 2 declines
/// too: the caller then runs the accurate path.
$(#[$a])*
$(#[$i])*
pub fn log_tiered(x: f64) -> Option<f64> {
    if let Some((zh, zl)) = log_quick_parts(x) {
        if zh + zl * C == zh {
            return Some(zh);
        }
    }
    tier2(x)
}
    };
}

/// Tier 2 of [`log_tiered`] (the SSE2 build of [`super::log_nearest`]), out of line and cold so
/// the quick phase keeps its registers: about 0.1% of calls reach it.
#[cold]
#[inline(never)]
fn tier2(x: f64) -> Option<f64> {
    super::log_nearest(x)
}

quick!(; #[inline(always)]; ma; reduce; half_square);

/// The quick phase's FMA build (D-10.25, D-10.50): callable only on a CPU with FMA whose AVX state
/// the OS saves; the boundary chooses it once at startup (`dispatch::fma`).
// ARCH(x86_64): the run-time FMA build; FMA is base ISA on aarch64, so one build there
#[cfg(target_arch = "x86_64")]
pub mod fused {
    use super::{t, C, C3, C4, C5, C6, C7, C8, C9, KH, KL, MIN_NORMAL, OFF, SPAN};
    // ARCH(x86_64): the scalar FMA intrinsics
    use core::arch::x86_64::{_mm_cvtsd_f64, _mm_fmadd_sd, _mm_fmsub_sd, _mm_set_sd};

    /// `a·b + c` in one rounding.
    #[target_feature(enable = "fma")]
    #[inline]
    fn ma(a: f64, b: f64, c: f64) -> f64 {
        _mm_cvtsd_f64(_mm_fmadd_sd(_mm_set_sd(a), _mm_set_sd(b), _mm_set_sd(c)))
    }

    /// `r = c·m − 1`: exact (module doc), so one FMA.
    #[target_feature(enable = "fma")]
    #[inline]
    fn reduce(m: f64, _mb: u64, c: f64, _b: f64) -> f64 {
        _mm_cvtsd_f64(_mm_fmsub_sd(_mm_set_sd(m), _mm_set_sd(c), _mm_set_sd(1.0)))
    }

    /// `(vh, vl, s)` with `s = RN(r²)`, `r² = s + s'` exactly (2MultFMA) and `vh + vl = r − r²/2`
    /// within `u²·|r|`.
    #[target_feature(enable = "fma")]
    #[inline]
    fn half_square(r: f64) -> (f64, f64, f64) {
        let s = r * r;
        let s1 = _mm_cvtsd_f64(_mm_fmsub_sd(_mm_set_sd(r), _mm_set_sd(r), _mm_set_sd(s)));
        let hh = -0.5 * s;
        let vh = r + hh;
        (vh, ma(-0.5, s1, (r - vh) + hh), s)
    }

    /// Tier 2 of [`log_tiered`] in the FMA build (`fast64::fused::log_nearest`), out of line and
    /// cold.
    #[target_feature(enable = "fma")]
    #[cold]
    #[inline(never)]
    fn tier2(x: f64) -> Option<f64> {
        crate::fast64::fused::log_nearest(x)
    }

    quick!(
        #[doc = ""]
        #[doc = "# Safety"]
        #[doc = ""]
        #[doc = "The CPU must support FMA and the OS must save the AVX state (the `#[target_feature]` contract)."]
        #[target_feature(enable = "fma")]; #[inline]; ma; reduce; half_square
    );
}

#[cfg(test)]
mod tests;
