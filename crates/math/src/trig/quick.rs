//! The quick phase of `double` `sin`, `cos`, `tan` and `sincos` (D-10.51, D-10.61;
//! LIBM-F64-TRIG-QUICK): a cheap evaluation with a proven relative bound and Ziv's rounding test
//! (Ziv, ACM TOMS 17(3), 1991) in front of today's in-place tier ([`super::kernel`], tier 2,
//! unchanged) and the accurate path (tier 3). One reduction serves all four functions.
//!
//! **Reduction.** `x = k·π/64 + r`, `n = k mod 128`, `|r| <= (1/2 + 2^-27)·π/64 < 2^-5.34`.
//! * `2^-26 <= |x| < 2^20` (Cody & Waite, Software Manual for the Elementary Functions, 1980):
//!   `t = RN(x·64/π + 1.5·2^52)` (one FMA, or two roundings: `k` may then be the second-nearest
//!   integer, `|x·64/π − k| <= 1/2 + 2^-28`); `kd = t − 1.5·2^52` is `k` exactly and `n` is read
//!   from `t`'s low bits (no conversion instruction). `π/64 = P1 + P2 + P3 + τ`, the three parts
//!   of today's tier (`P1` of 27 bits, `P2` of 25 bits, `|τ| < 2^-119.1`, `|k| < 2^24.35`):
//!   `t1 = x − kd·P1` exact, `(sh, sl) = 2Sum(t1, −kd·P2)` exact, `rl1 = sl − kd·P3` (two
//!   roundings, `<= 2^-91` each), then `(rh, rl) = Fast2Sum(sh, rl1)`: `|r − rh − rl| < 2^-89.9`
//!   (today's tier's proof, `super::fast`) and `|rl| <= 2^-53·|rh|`. Fast2Sum is exact when
//!   `|sh| >= |rl1|`; otherwise every value is below `2^-37.8` and its error below `2^-89`.
//! * `|x| >= 2^20` (Payne & Hanek, SIGNUM Newsletter 18(1), 1983) in integers: `|x| = m·2^e`;
//!   the bits of `2/π` before bit `e − 1` add multiples of 128 to `x·64/π`, so a 192-bit window
//!   `W` from that bit (zeros before the point) gives `x·64/π ≡ m·W·2^-185 (mod 128)` from three
//!   64×64-bit products. The top 7 bits above the point are `k`, the next 128 the fraction `f`
//!   (rounded to the nearest integer: `f >= 1/2` becomes `f − 1`, `k + 1`); the dropped product
//!   bits and the window's tail leave `|Δf| < 2^-128 + 2^-132`. The phase declines when `|f| <
//!   2^-60` (the guard, not a claim about how close a double comes to a multiple of `π/64`), so
//!   `f` keeps at least 68 bits and its relative error is below `2^-67.9`. `f` becomes
//!   `fh + fl` (53 + 53 bits, relative `2^-106`) and `r = f·π/64` with the double-double `π/64`:
//!   `rh = RN(fh·Ph)`, its error exact by FMA (or Dekker's product), plus `fh·Pl + fl·Ph`, then
//!   Fast2Sum. Relative error of `r` below `2^-67.8`; nothing is cached across calls.
//!
//! **Evaluation** (Tang-style table, ACM TOMS 15(2) 1989, with today's 128-point table
//! `SIN_K_PI64` of `sin(jπ/64)` in two parts, `<= 2^-106`): with `S = sh + sl = sin(nπ/64)` and
//! `C = ch + cl = cos(nπ/64)`,
//! `sin(nπ/64 + r) = S + C·rh + S·(cos r − 1) + C·(sin r − rh)`. `cos x = sin(x + π/2)` uses
//! index `n + 32`; `tan = sin/cos` by one division and its residual. Per assembly:
//! * `p = C·rh` exact (`ph + pl`, FMA or Dekker's product: the only product whose rounding
//!   would cost `2^-53·|y|`); `(ah, al) = Fast2Sum(sh, ph)` exact (`|sh| >= sin(π/64) >
//!   2·|ph|`, or `sh = 0`);
//! * `q = RN(sh·crh)` with `crh = −RN(rh²)/2`;
//! * `lo = q + ((al + pl) + (ch·(s + rl·(1 + crh)) + (sh·(c4 − rh·rl) + (sl + cl·rh))))`, with
//!   `s ≈ sin rh − rh` (Taylor to `rh^9`) and `c4 ≈ cos rh − 1 + rh²/2` (Taylor to `rh^8`), both
//!   in Estrin's scheme (Estrin 1960; Knuth, TAOCP 2, 4.6.4) on `z = RN(rh²)`, `z² `;
//! * `(yh, yl) = Fast2Sum(ah, lo)` (`|lo| < 2^-10.6·|ah|`).
//!
//! **Bound** (relative to `|y|`, `u = 2^-53`, the SSE2 build where every `a·b + c` rounds twice;
//! the FMA build rounds less, so the bound holds for both). With `S != 0`: `|y| >= sin(π/128)·
//! (1 − 2^-27) > 2^-5.35`, `|S| <= 2|y|` and `|C·r| <= |y|` (both extreme at `n = 1`,
//! `r = −π/128`); with `S = 0` (`n ≡ 0 mod 64`), `|y| >= |r|·(1 − 2^-12)` and the phase needs
//! `|rh| >= 2^-26` (below that, tier 2):
//! 1. `q`'s rounding: `u·|S|·rh²/2 <= 2^-63.69`; `z`'s rounding inside `q`: `2^-63.69`;
//! 2. the last addition `q + (...)`: `|lo| <= 2^-10.66·|y|`, `2^-63.66`; the one before it acts
//!    on `<= 2^-13.2·|y|`: `2^-66.2`;
//! 3. `C·(s + rl·(1 + crh))`: `|s| <= rh³/6 <= 2^-13.24·|y|`; `s` carries `4.8u` (z, `rh·z`,
//!    the coefficients' and Estrin's roundings, the last product), the sum and the product `u`
//!    each: `6.8u·2^-13.24 = 2^-63.47`;
//! 4. `sh·(c4 − rh·rl)`, `cl·rh + sl` and their roundings: `< 2^-74`; truncations: `sin` at
//!    `rh^11/11! < 2^-84` absolute, `cos` at `rh^10/10!·|S| < 2^-74.3·|y|`;
//! 5. dropped second-order terms of `rl` (`|rl| <= u·|rh|`): `S·rl²/2`, `C·rl·(sin rh − rh)`
//!    terms: `< 2^-70`; the table `2^-105`;
//! 6. reduction: `S != 0`: `2^-89.9/2^-5.35 < 2^-84.5` (huge path: `2^-67.8·|r|/|y| <= 2^-67.8`);
//!    `S = 0`: `2^-89.9/2^-26 = 2^-63.9` (huge path `2^-67.8`).
//!
//! Sum with `S != 0`: `2^-63.69·2 + 2^-63.66 + 2^-63.47 + 2^-66.2 + 2^-70 < 0.34·2^-60`; with
//! `S = 0`: `2^-63.9 + 2^-63.47 + 3·2^-66.2 < 0.25·2^-60`. Stated `RHO = 0.875·2^-61` (margin for
//! `|yh|` against `|y|`); test multiplier `C_Q` the least double with `C_Q·(1 − 2^54·RHO) > 1 +
//! 2^-53` rounded up to `1 + 2^-7 + 2^-12`. `tan = s/c`: `qh = RN(sh/ch)`, the residual
//! `sh − qh·ch` exact (FMA or Dekker's product), `ql = ((sh − qh·ch) + sl − qh·cl)/ch`: relative
//! error `<= ρs + ρc + 2ρc(ρs + ρc) + 2^-100 < 2^-60`, multiplier `C_TAN` of today's tier
//! (`rho = 2^-60`). The test (de Dinechin, Lauter & Muller, RAIRO-ITA 41, 2007) needs `y`
//! neither a double nor a midpoint (true: `sin`, `cos`, `tan` of a nonzero double are
//! transcendental, Lindemann–Weierstrass) and normal (`|y| > 2^-27`). Every intermediate is far
//! from the subnormal range, so only inexact is raised. The `bounds` test recomputes the sums.
//!
//! **Why this table and not `π/2` with `sin`, `cos` of `j/64`** (D-10.61): reducing by `π/2`
//! and then by `j/64` shrinks `|r|` to `2^-7` (one Taylor term less in each series) but adds a
//! second shift-and-subtract, an index and a quadrant select on the critical path and a second
//! table; the one-step `π/64` reduction reuses today's table (no new cache lines) and needs no
//! quadrant logic (`cos` is index `n + 32`).

#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)] // bit arithmetic on bounded values (above), indices masked to 0..127

use super::{t, C4, C6, C8, C_TAN, P1, P2, P3, S3, S5, S7, S9, SCALE_PI};

/// `1.5·2^52`: `x·64/π + SHIFT` lies in `[2^52, 2^53)` (`|x·64/π| < 2^25`), where every double
/// is an integer whose low bits are `k mod 128`.
const SHIFT: f64 = 6_755_399_441_055_744.0;
const NP1: f64 = -P1;
const NP3: f64 = -P3;
/// `π/64` as a double-double (the huge path's product).
const PIH: f64 = f64::from_bits(t::PI64[0]);
const PIL: f64 = f64::from_bits(t::PI64[1]);
/// `bits(2^-26)`, `bits(2^20) − bits(2^-26)`: the Cody–Waite domain as one unsigned compare.
const LO_BITS: u64 = 0x3e50_0000_0000_0000;
const HI_BITS: u64 = 0x4130_0000_0000_0000;
const SPAN: u64 = HI_BITS - LO_BITS;
const INF_BITS: u64 = 0x7ff0_0000_0000_0000;
const ABS: u64 = 0x7fff_ffff_ffff_ffff;
/// `2^-26`: with a zero table sine the phase needs `|rh|` at least this (module doc, term 6).
const R26: f64 = f64::from_bits(0x3e50_0000_0000_0000);
/// The stated bound of one assembly and its rounding test's multiplier (module doc; checked by
/// the `bounds` test).
#[doc(hidden)] // public for the boundary's tests only
pub const RHO: f64 = f64::from_bits(0x3c1c_0000_0000_0000);
pub(crate) const C_Q: f64 = f64::from_bits(0x3ff0_2100_0000_0000);

/// Word `j` of `2/π` with a zero word in front (`j = 0` is the word before the point).
#[inline(always)]
fn zword(j: usize) -> u64 {
    if j == 0 { 0 } else { t::TWO_OVER_PI.get(j - 1).copied().unwrap_or(0) }
}

/// The fraction `f` of `|x|·64/π` (`|x| >= 2^20`, finite) as `(k mod 128, fh, fl, ok)`:
/// `f = fh + fl` within `2^-67.9·|f|`, `ok` false when `|f| < 2^-60` (module doc).
#[inline(always)]
#[allow(clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::cast_possible_wrap, clippy::cast_sign_loss)] // u128 halves, 53-bit integers to f64 (exact), bounded shifts
fn payne_hanek_bits(ix: u64) -> (u64, f64, f64, bool) {
    let m = (ix & ((1u64 << 52) - 1)) | (1u64 << 52);
    let e = (ix >> 52) as i64 - 1075; // |x| = m·2^e, e in [-32, 971]
    // The window starts at bit e − 1 (1-based) of 2/π: bit index e − 2 + 64 of the zero-prefixed words.
    let g = (e + 62) as usize;
    let (j, o) = (g / 64, (g % 64) as u32);
    let (z0, z1, z2, z3) = (zword(j), zword(j + 1), zword(j + 2), zword(j + 3));
    let sh = |a: u64, b: u64| (a << o) | (b >> 1 >> (63 - o));
    let (w0, w1, w2) = (sh(z0, z1), sh(z1, z2), sh(z2, z3));
    let m = u128::from(m);
    let a2 = m * u128::from(w2);
    let a1 = m * u128::from(w1) + (a2 >> 64);
    let a0 = m * u128::from(w0) + (a1 >> 64);
    let (d0, d1, d2) = (a2 as u64, a1 as u64, a0 as u64);
    // Integer bits at 185.. (d2's top 7), fraction bits 57..185.
    let fhi = (d2 << 7) | (d1 >> 57);
    let flo = (d1 << 7) | (d0 >> 57);
    let k = (d2 >> 57) + (fhi >> 63);
    let v = ((u128::from(fhi) << 64) | u128::from(flo)) as i128;
    let a = v.unsigned_abs();
    let lz = a.leading_zeros();
    let an = a << (lz & 127);
    let h = (an >> 75) as u64;
    let l = ((an >> 22) as u64) & ((1u64 << 53) - 1);
    let s1 = f64::from_bits(u64::from(1023 - 53 - lz.min(59)) << 52);
    let s2 = f64::from_bits(u64::from(1023 - 106 - lz.min(59)) << 52);
    let (fh, fl) = ((h as i64) as f64 * s1, (l as i64) as f64 * s2);
    let neg = v < 0;
    (k, if neg { -fh } else { fh }, if neg { -fl } else { fl }, lz <= 59)
}

// The phase in two builds (D-10.25, D-10.50): `$ma` is `a·b + c` (two roundings, or one FMA),
// `$prod` Dekker's exact product or 2MultFMA, `$k` the module of today's tier in that build.
// `#[inline(always)]` cannot be combined with `#[target_feature]`, hence `$i`.
macro_rules! quick {
    ($(#[$a:meta])*; $(#[$i:meta])*; $ma:path; $prod:path; $k:path) => {
use $k as tier;

/// `x = n·π/64 + rh + rl` (module doc): `(n, rh, rl)`, or `None` outside `2^-26 <= |x|`, for
/// NaN, infinities and a huge `x` whose fraction is below `2^-60`.
$(#[$a])*
$(#[$i])*
#[doc(hidden)] // public for the boundary's tests only
pub fn reduce(x: f64) -> Option<(usize, f64, f64)> {
    let ix = x.to_bits() & ABS;
    if ix.wrapping_sub(LO_BITS) >= SPAN {
        return reduce_huge(x, ix);
    }
    let tt = $ma(x, SCALE_PI, SHIFT);
    let kd = tt - SHIFT;
    #[allow(clippy::as_conversions, clippy::cast_possible_truncation)] // the low 7 bits
    let n = (tt.to_bits() & 127) as usize;
    let t1 = $ma(kd, NP1, x);
    let s = crate::dd::two_sum(t1, -(kd * P2));
    let rl1 = $ma(kd, NP3, s.lo);
    let rh = s.hi + rl1;
    Some((n, rh, rl1 - (rh - s.hi)))
}

/// [`reduce`] for `|x| >= 2^20` (integer Payne–Hanek), out of line.
$(#[$a])*
#[inline(never)]
fn reduce_huge(x: f64, ix: u64) -> Option<(usize, f64, f64)> {
    if !(HI_BITS..INF_BITS).contains(&ix) {
        return None;
    }
    let (k, fh, fl, ok) = payne_hanek_bits(ix);
    if !ok {
        return None;
    }
    let p = $prod(fh, PIH);
    let rl = p.lo + $ma(fh, PIL, fl * PIH);
    let rh = p.hi + rl;
    let rl = rl - (rh - p.hi);
    #[allow(clippy::as_conversions, clippy::cast_possible_truncation)] // the low 7 bits
    let k = (k & 127) as usize;
    if x < 0.0 { Some(((128 - k) & 127, -rh, -rl)) } else { Some((k, rh, rl)) }
}

/// The shared series of [`reduce`]'s `r`: `[rh, crh, s + rl·(1 + crh), c4 − rh·rl]` (module doc).
$(#[$a])*
$(#[$i])*
pub(super) fn series(rh: f64, rl: f64) -> [f64; 4] {
    let z = rh * rh;
    let z2 = z * z;
    let crh = -0.5 * z;
    let sp = $ma(z2, $ma(z, S9, S7), $ma(z, S5, S3));
    let s = (rh * z) * sp;
    let c4 = z2 * $ma(z2, C8, $ma(z, C6, C4));
    [rh, crh, $ma(rl, crh, rl) + s, $ma(-rh, rl, c4)]
}

/// `sin(i·π/64 + r)` from [`series`] as `(yh, yl, sh)` (`sh` the table sine; module doc).
$(#[$a])*
$(#[$i])*
pub(super) fn assemble(i: usize, [rh, crh, sr, cr]: [f64; 4]) -> (f64, f64, f64) {
    let e = t::SIN_K_PI64[i & 127];
    let (sh, sl) = (f64::from_bits(e[0]), f64::from_bits(e[1]));
    let e = t::SIN_K_PI64[(i + 32) & 127];
    let (ch, cl) = (f64::from_bits(e[0]), f64::from_bits(e[1]));
    let p = $prod(ch, rh);
    let ah = sh + p.hi;
    let al = (sh - ah) + p.hi;
    let q = sh * crh;
    let lo1 = $ma(ch, sr, $ma(sh, cr, $ma(cl, rh, sl)));
    let lo = q + ((al + p.lo) + lo1);
    let yh = ah + lo;
    (yh, (ah - yh) + lo, sh)
}

/// The rounding test of one assembly: `yh` when it is the correctly rounded value, else `None`.
#[inline(always)]
fn decide((yh, yl, sh): (f64, f64, f64), rh: f64) -> Option<f64> {
    let w = yl * C_Q;
    if yh + w == yh && w != 0.0 && (sh != 0.0 || crate::dd::abs(rh) >= R26) { Some(yh) } else { None }
}

/// The quick `sin x` (round to nearest) or `None` (module doc).
$(#[$a])*
$(#[$i])*
pub fn sin_quick(x: f64) -> Option<f64> {
    let (n, rh, rl) = reduce(x)?;
    decide(assemble(n, series(rh, rl)), rh)
}

/// The quick `cos x` (round to nearest) or `None` (module doc).
$(#[$a])*
$(#[$i])*
pub fn cos_quick(x: f64) -> Option<f64> {
    let (n, rh, rl) = reduce(x)?;
    decide(assemble(n + 32, series(rh, rl)), rh)
}

/// The quick `tan x` (round to nearest) or `None` (module doc): both assemblies, one division.
$(#[$a])*
$(#[$i])*
pub fn tan_quick(x: f64) -> Option<f64> {
    let (n, rh, rl) = reduce(x)?;
    let p = series(rh, rl);
    let (sh, sl, ss) = assemble(n, p);
    let (ch, cl, cs) = assemble(n + 32, p);
    let qh = sh / ch;
    let m = $prod(qh, ch);
    let ql = ((sh - m.hi - m.lo) + $ma(-qh, cl, sl)) / ch;
    let w = ql * C_TAN;
    if qh + w == qh && w != 0.0 && (ss * cs != 0.0 || crate::dd::abs(rh) >= R26) { Some(qh) } else { None }
}

/// The quick `(sin x, cos x)` (round to nearest), both or `None` (module doc).
$(#[$a])*
$(#[$i])*
pub fn sincos_quick(x: f64) -> Option<(f64, f64)> {
    let (n, rh, rl) = reduce(x)?;
    let p = series(rh, rl);
    Some((decide(assemble(n, p), rh)?, decide(assemble(n + 32, p), rh)?))
}

/// `sin`'s in-place fast path (D-10.51): the quick phase, then on a miss tier 2 out of line.
/// `None` when both decline (the caller runs the accurate path).
$(#[$a])*
$(#[$i])*
pub fn sin_tiered(x: f64) -> Option<f64> {
    if let Some(v) = sin_quick(x) {
        return Some(v);
    }
    tier2_sin(x)
}

/// `cos` as [`sin_tiered`].
$(#[$a])*
$(#[$i])*
pub fn cos_tiered(x: f64) -> Option<f64> {
    if let Some(v) = cos_quick(x) {
        return Some(v);
    }
    tier2_cos(x)
}

/// `tan` as [`sin_tiered`].
$(#[$a])*
$(#[$i])*
pub fn tan_tiered(x: f64) -> Option<f64> {
    if let Some(v) = tan_quick(x) {
        return Some(v);
    }
    tier2_tan(x)
}

/// `sincos` as [`sin_tiered`].
$(#[$a])*
$(#[$i])*
pub fn sincos_tiered(x: f64) -> Option<(f64, f64)> {
    if let Some(v) = sincos_quick(x) {
        return Some(v);
    }
    tier2_sincos(x)
}

// Tier 2 (today's kernels in the same build), cold and out of line so the quick phase keeps
// its registers.
$(#[$a])*
#[cold]
#[inline(never)]
fn tier2_sin(x: f64) -> Option<f64> {
    tier::sin_nearest(x)
}
$(#[$a])*
#[cold]
#[inline(never)]
fn tier2_cos(x: f64) -> Option<f64> {
    tier::cos_nearest(x)
}
$(#[$a])*
#[cold]
#[inline(never)]
fn tier2_tan(x: f64) -> Option<f64> {
    tier::tan_nearest(x)
}
$(#[$a])*
#[cold]
#[inline(never)]
fn tier2_sincos(x: f64) -> Option<(f64, f64)> {
    tier::sincos_nearest(x)
}
    };
}

/// `a·b + c` in two roundings (the SSE2 build).
#[inline(always)]
fn ma(a: f64, b: f64, c: f64) -> f64 {
    a * b + c
}

/// The quick phase, SSE2 build (Dekker's exact product).
pub mod plain {
    use super::{payne_hanek_bits, t, ABS, C4, C6, C8, C_Q, C_TAN, HI_BITS, INF_BITS, LO_BITS, NP1, NP3, P2, PIH, PIL, R26, S3, S5, S7, S9, SCALE_PI, SHIFT, SPAN};
    use super::ma;
    use crate::dd::two_prod;

    quick!(; #[inline(always)]; ma; two_prod; super::super::kernel);
}

/// The quick phase's FMA build (D-10.25, D-10.50): callable only on a CPU with FMA whose AVX state
/// the OS saves; the boundary chooses it once at startup (`dispatch::fma`).
// ARCH(x86_64): the run-time FMA build; FMA is base ISA on aarch64, so one build there
#[cfg(target_arch = "x86_64")]
pub mod fused {
    use super::{payne_hanek_bits, t, ABS, C4, C6, C8, C_Q, C_TAN, HI_BITS, INF_BITS, LO_BITS, NP1, NP3, P2, PIH, PIL, R26, S3, S5, S7, S9, SCALE_PI, SHIFT, SPAN};
    use crate::dd::two_prod_fma;
    // ARCH(x86_64): the scalar FMA intrinsic
    use core::arch::x86_64::{_mm_cvtsd_f64, _mm_fmadd_sd, _mm_set_sd};

    /// `a·b + c` in one rounding.
    #[target_feature(enable = "fma")]
    #[inline]
    fn ma(a: f64, b: f64, c: f64) -> f64 {
        _mm_cvtsd_f64(_mm_fmadd_sd(_mm_set_sd(a), _mm_set_sd(b), _mm_set_sd(c)))
    }

    quick!(
        #[doc = ""]
        #[doc = "# Safety"]
        #[doc = ""]
        #[doc = "The CPU must support FMA and the OS must save the AVX state (the `#[target_feature]` contract)."]
        #[target_feature(enable = "fma")]; #[inline]; ma; two_prod_fma; super::super::kernel_fma
    );
}

#[cfg(test)]
mod tests;
