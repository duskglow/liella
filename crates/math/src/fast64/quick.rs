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
//! **exp2 and exp10** (LIBM-F64-EXP-FAMILY-QUICK, D-10.64): the same kernel (`T_j` on, above)
//! fed by their own reductions, so `2^x` or `10^x` is `2^(k/128)·e^r` with `r = rh + rl`:
//! * exp2, `2^-60 <= |x| < 1021`: `t = RN(x·128 + SHIFT)` (`x·128` exact, so `|x·128 − k| <=
//!   1/2`), `s = x − kd/128` exact (`kd/128` exact; for `k != 0`, `|x| >= 2^-8` puts `x` and
//!   `kd/128` on the grid of `ulp(x) >= 2^-60` and `|s| <= 2^-8`), `rh = RN(s·LN2H)`; FMA build
//!   `rl = RN(s·LN2L + (s·LN2H − rh))` (the bracket exact by one FMA), SSE2 build `rl = s·LN2L`
//!   and the dropped `|s·LN2H − rh| <= ulp(rh)/2 <= 2^-62` is an extra term `u·rh` (bound term
//!   1's size). No `ln2` reduction constant: `|r| <= ln2/256` with no `2^-35` slack.
//! * exp10, `2^-60 <= |x| < 307`: `t = RN(x·INV10 + SHIFT)`, `INV10 = RN(128/log10 2)`, so
//!   `|x·128/log10 2 − k| <= 1/2 + 2^-35` (`|x·INV10| < 2^17`). With `L = log10(2)/128` split as
//!   `E0 + E1 + E2` (`NE*` negated, within `2^-121.2`; Cody–Waite as above): `r0 = x − kd·E0` is exact (for
//!   `k != 0`, `|x| > L/2 > 2^-10` puts both on the grid `2^-62`, and `|r0| < L/2 + 2^-28.7 <
//!   2^-9`); `sh = RN(r0 − kd·E1)` with `sl = RN(((r0 − sh) − kd·E1) − kd·E2)`, the bracket exact
//!   by Fast2Sum (`kd·E1`'s last place divides `2^-81`, which divides `r0`), so `|sh + sl − (x −
//!   kL)| < |k|·2^-121.2 + 2^-116 < 2^-104`, `|sh| <= 0.001176`, `|sl| <= 2^-62.9`. Then `r =
//!   (sh + sl)·ln10`: `rh = RN(sh·LN10H)`, FMA `rl = RN(sh·LN10L + RN(sl·LN10H + (sh·LN10H −
//!   rh)))`, SSE2 the same without the bracket (dropped `<= 2^-62`, the term `u·rh`). `|r| =
//!   |x·128/log10 2 − k|·ln2/128`: exp's `R` holds.
//! * Bound: terms 2, 3, 5 and the table are exp's with `|rl| <= 2^-62 + |s|·|LN2L|` (exp2) or
//!   `2^-62 + |sh|·|LN10L| + 2^-62.9·LN10H < 2^-60.5` (exp10) in place of `2^-62` (the SSE2
//!   builds drop the `2^-62`); term 4 grows to `|rl|·(e^R − 1)`; term 6 becomes the reduction's
//!   `2^-113` (exp2) and `2.31·2^-104 + 2^-112` (exp10); the SSE2 builds add `u·rh·e^R` for the
//!   dropped product. Totals over `0.99729`: exp2 FMA `2^-67.64`, SSE2 `2^-60.51`; exp10 FMA
//!   `2^-67.30`, SSE2 `2^-60.51`; stated for both `RHO_FAM_FMA = 1.6875·2^-68` (`2^-67.25`) and
//!   `RHO_FAM_SSE2 = 1.4375·2^-61` (`2^-60.48`). Expected fallback: FMA 0.010%, SSE2 1.14% (a
//!   Dekker product would halve the SSE2 rate for about six more operations on every call; at
//!   tier 2's cost the 0.57% it saves is worth about one instruction per call).
//! * Exact results: `2^x` for an integer `x` gives `s = rh = rl = q = 0`, `j = 0`, so `zl = 0`
//!   exactly and the test declines without raising inexact (tier 2 then answers it exactly);
//!   `10^x` for the integers `1 ..= 23` (a double or, at 23, a midpoint) is declined before the
//!   kernel. Every other `2^x`, `10^x` for a double `x` is irrational or not dyadic.
//!
//! **expm1** (LIBM-F64-EXP-FAMILY-QUICK-2, D-10.64; Tang, "Table-driven implementation of the expm1
//! function in IEEE floating-point arithmetic", ACM TOMS 18(2), 1992: a table region and a
//! polynomial region), `2^-60 <= |x| < 708`, `x > −37` (one compare more on the bits):
//! * `|x| >= 2^-5`: exp's reduction and kernel, with `th·rh`'s low part kept in both builds (the
//!   FMA's, or Dekker's product in the SSE2 build: the `−1` magnifies the kernel's error by up to
//!   `e^x/|e^x − 1| <= 32.503` here, and term 1 would dominate). `vh = zh·2^m`; `vh − 1 = s + e`
//!   exactly (2Sum, Knuth TAOCP 2, 4.2.2); `lo = RN(e + zl·2^m)`, then Fast2Sum `(s, lo)`. Kernel
//!   bound (exp's terms 2-6, 4 low-sum roundings in SSE2, 3 with FMA): `2^-67.59` (SSE2),
//!   `RHO_FMA` (FMA); times 32.503, plus `lo`'s rounding (`< 2^-100`): `2^-62.57` / `2^-62.66`.
//! * `|x| < 2^-5`: `e^x − 1 = x + x²/2 + x³·P(x)`, `P` Taylor's `1/3! ..= 1/9!` in Estrin's
//!   order (truncation `X^9/10!·1.003 = 2^-66.79` relative, `X = 2^-5`). The head keeps every bit
//!   of `x`: `xh` = `x` with its low 27 significand bits cleared (one AND), `xl = x − xh` exact,
//!   `x²/2 = xh²/2 + xl·(x + xh)/2` with the first product exact (26-bit `xh`) and the second
//!   below `2^-25·x²` (its two roundings `< 2^-82·|x|`). `h0 + e = x + xh²/2` exactly (Fast2Sum,
//!   `|x| >= xh²/2`); `t = (x·s)·P` with `s = RN(x²)` within `9.01u·X²·pmax·|x|` (three
//!   roundings, Estrin's `P` within `3u`, counted twice for margin; `pmax = 0.16797`); `lo = e +
//!   (hl + t)` two roundings `2^-64.57`; Fast2Sum `(h0, lo)`. Over `|e^x − 1| >= 0.98455·|x|`:
//!   `2^-62.04` (SSE2), `2^-62.09` (FMA).
//! * Stated for both builds and regions `RHO_M1 = 1.5·2^-62` (`2^-61.415`); expected fallback
//!   about 0.6%. `e^x − 1` is irrational for every double `x != 0` (Lindemann–Weierstrass), so
//!   no exact case; `zh` is normal and needs no scaling.
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
const SHIFT: f64 = 6_755_399_441_055_744.0;
/// `bits(SHIFT) − 1023·128`: `bits(t)` minus this is `k + 1023·128 >= 0`; shifted right by 7, the
/// biased exponent of `2^m`.
const KBIAS: u64 = 0x4338_0000_0000_0000 - 1023 * 128;
const INV: f64 = f64::from_bits(t::SCALE_128[0]);
const NL1: f64 = -f64::from_bits(t::LN2_128_CW[0]);
const NL2: f64 = -f64::from_bits(t::LN2_128_CW[1]);
/// `RN(1/3!)` .. `RN(1/6!)` (checked by the `bounds` tests).
const C3: f64 = f64::from_bits(0x3fc5_5555_5555_5555);
const C4: f64 = f64::from_bits(0x3fa5_5555_5555_5555);
const C5: f64 = f64::from_bits(0x3f81_1111_1111_1111);
const C6: f64 = f64::from_bits(0x3f56_c16c_16c1_6c17);
/// `bits(2^-60)` and `bits(708) − bits(2^-60)`: the domain as one unsigned compare on `|x|`'s bits.
const LO_BITS: u64 = 0x3c30_0000_0000_0000;
const SPAN: u64 = 0x4086_2000_0000_0000 - LO_BITS;
const ABS: u64 = 0x7fff_ffff_ffff_ffff;

/// The stated bounds (module doc) and their [`super::near`] multipliers: the least doubles with
/// `c·(1 − 2^54·rho) > 1 + 2^-53` (checked exactly by the `bounds` tests).
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub const RHO_SSE2: f64 = f64::from_bits(0x3c17_0000_0000_0000);
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub const RHO_FMA: f64 = f64::from_bits(0x3bb4_0000_0000_0000);
pub(crate) const C_SSE2: f64 = f64::from_bits(0x3ff0_1721_3fcb_b4d5);
pub(crate) const C_FMA: f64 = f64::from_bits(0x3ff0_0050_0190_07d1);

// exp2 and exp10 on the same kernel (LIBM-F64-EXP-FAMILY-QUICK, D-10.64): their own reductions
// feed `kernel` a `(rh, rl)` with the bound of the exp2/exp10 section of the module doc.
/// `ln2 = LN2H + LN2L` and `ln10 = LN10H + LN10L` (`|LN2L|`, `|LN10L| < 2^-52`, each pair within
/// `2^-106` relative of exact; `mp::ln2`, `mp::ln10`).
const LN2H: f64 = f64::from_bits(t::LN2[0]);
const LN2L: f64 = f64::from_bits(t::LN2[1]);
const LN10H: f64 = f64::from_bits(t::LN10[0]);
const LN10L: f64 = f64::from_bits(t::LN10[1]);
/// `RN(128/log10 2)`: twice `SCALES[1] = RN(64/log10 2)` (doubling is exact).
const INV10: f64 = 2.0 * f64::from_bits(t::SCALES[1]);
/// `−log10(2)/128` in three parts: `LOG10_2_64_CW` halved (exact), negated; `E0` has 32
/// significant bits (lowest `2^-40`), `E1` 36 (lowest `2^-81`), `E2` 36, so `k·E0`, `k·E1`, `k·E2`
/// are exact for `|k| < 2^17`, and the parts are within `2^-121.2` of `log10(2)/128`.
const NE0: f64 = -0.5 * f64::from_bits(t::LOG10_2_64_CW[0]);
const NE1: f64 = -0.5 * f64::from_bits(t::LOG10_2_64_CW[1]);
const NE2: f64 = -0.5 * f64::from_bits(t::LOG10_2_64_CW[2]);
/// `bits(1021) − bits(2^-60)` and `bits(307) − bits(2^-60)`: the exp2 and exp10 domains.
const SPAN2: u64 = 0x408f_e800_0000_0000 - LO_BITS;
const SPAN10: u64 = 0x4073_3000_0000_0000 - LO_BITS;
/// `bits(1)` and `bits(23) − bits(1)`: exp10's exact cases `1 <= x <= 23` (integers) are left
/// to the entry's exact path, which raises no inexact.
const ONE_BITS: u64 = 0x3ff0_0000_0000_0000;
const SPAN_EXACT10: u64 = 0x4037_0000_0000_0000 - ONE_BITS;

/// exp2's and exp10's stated bounds (module doc, "exp2 and exp10") and their [`super::near`]
/// multipliers (checked exactly by the `bounds` tests).
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub const RHO_FAM_SSE2: f64 = f64::from_bits(0x3c27_0000_0000_0000);
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub const RHO_FAM_FMA: f64 = f64::from_bits(0x3bbb_0000_0000_0000);
pub(crate) const C_FAM_SSE2: f64 = f64::from_bits(0x3ff0_2e85_c089_8b72);
pub(crate) const C_FAM_FMA: f64 = f64::from_bits(0x3ff0_006c_02d9_133a);

// expm1 (D-10.64, module doc "expm1"): exp's reduction and kernel for `|x| >= 2^-5`, a series
// with an exact head below.
/// `RN(1/7!)`, `RN(1/8!)`, `RN(1/9!)`: the small region's series beyond exp's `C3 ..= C6`.
const C7: f64 = 1.0 / 5040.0;
const C8: f64 = 1.0 / 40320.0;
const C9: f64 = 1.0 / 362_880.0;
/// Clears the low 27 significand bits: `xh` keeps 26 significant bits, so `xh·xh` is exact.
const MASK27: u64 = !((1 << 27) - 1);
/// `bits(2^-5)`: below it (on `|x|`'s bits) the series region.
const SMALL_M1: u64 = 0x3fa0_0000_0000_0000;
/// `bits(−37)`: a negative `x` with bits at or above it is `x <= −37` (tier 2's limit).
const NEG37_BITS: u64 = 0xc042_8000_0000_0000;
/// expm1's stated bound (both builds, both regions; module doc "expm1") and its [`super::near`]
/// multiplier (checked exactly by the `bounds` tests).
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub const RHO_M1: f64 = f64::from_bits(0x3c18_0000_0000_0000);
pub(crate) const C_M1: f64 = f64::from_bits(0x3ff0_1824_3651_7a38);

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

/// `a·b − p` exactly for `p = RN(a·b)` without FMA (Dekker's product, Veltkamp's splitting at 27
/// bits; Dekker 1971): expm1's SSE2 build keeps `th·rh`'s low part, which its `−1` would otherwise
/// magnify (module doc "expm1"). `|a| < 2`, `|b| < 2^-8`: no overflow, and the parts' products
/// stay normal or 0 for the `rh` that reach it.
#[inline(always)]
fn dekker_lo(a: f64, b: f64, p: f64) -> f64 {
    const SPLIT: f64 = 134_217_729.0;
    let ca = a * SPLIT;
    let ah = ca - (ca - a);
    let al = a - ah;
    let cb = b * SPLIT;
    let bh = cb - (cb - b);
    let bl = b - bh;
    (((ah * bh - p) + ah * bl) + al * bh) + al * bl
}

// The kernel in two builds (D-10.25, D-10.50): `$ma` is `a·b + c` (two roundings, or one FMA),
// `$lo` the low part of `th·rh` (0, or exact by one FMA), `$lox` that low part exactly (Dekker's
// product, or the FMA) for expm1, `$c` the rounding test's multiplier.
macro_rules! quick {
    ($(#[$a:meta])*; $(#[$i:meta])*; $ma:path; $lo:path; $lox:path; $c:expr; $cf:expr) => {
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
    Some(kernel::<false>(kb, rh, rl))
}

/// `2^x = (zh + zl)·sc` within `RHO_FAM_*·|zh|·sc` for `2^-60 <= |x| < 1021`, else `None`
/// (module doc, "exp2 and exp10"). An integer `x` gives `zl = 0` exactly, which the rounding test
/// declines (the entry's exact path answers it).
$(#[$a])*
$(#[$i])*
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub fn exp2_quick_parts(x: f64) -> Option<(f64, f64, f64)> {
    if (x.to_bits() & ABS).wrapping_sub(LO_BITS) >= SPAN2 {
        return None;
    }
    let tt = $ma(x, 128.0, SHIFT);
    let kd = tt - SHIFT;
    let kb = tt.to_bits().wrapping_sub(KBIAS);
    let s = $ma(kd, -1.0 / 128.0, x);
    let rh = s * LN2H;
    let rl = $ma(s, LN2L, $lo(s, LN2H, rh));
    Some(kernel::<false>(kb, rh, rl))
}

/// `10^x = (zh + zl)·sc` within `RHO_FAM_*·|zh|·sc` for `2^-60 <= |x| < 307` other than the
/// integers `1 ..= 23`, else `None` (module doc, "exp2 and exp10").
$(#[$a])*
$(#[$i])*
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub fn exp10_quick_parts(x: f64) -> Option<(f64, f64, f64)> {
    let b = x.to_bits();
    if (b & ABS).wrapping_sub(LO_BITS) >= SPAN10 {
        return None;
    }
    if b.wrapping_sub(ONE_BITS) <= SPAN_EXACT10 && (x + SHIFT) - SHIFT == x {
        return None;
    }
    let tt = $ma(x, INV10, SHIFT);
    let kd = tt - SHIFT;
    let kb = tt.to_bits().wrapping_sub(KBIAS);
    let r0 = $ma(kd, NE0, x);
    let e1 = kd * NE1;
    let sh = r0 + e1;
    let sl = $ma(kd, NE2, (r0 - sh) + e1);
    let rh = sh * LN10H;
    let rl = $ma(sh, LN10L, $ma(sl, LN10H, $lo(sh, LN10H, rh)));
    Some(kernel::<false>(kb, rh, rl))
}

/// The kernel shared by exp, exp2 and exp10: `2^(k/128)·e^(rh + rl)` as `(zh, zl, 2^m)` from
/// `kb = k + 1023·128` (module doc, from `T_j` on).
$(#[$a])*
$(#[$i])*
fn kernel<const X: bool>(kb: u64, rh: f64, rl: f64) -> (f64, f64, f64) {
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
    let yl = (e1 + if X { $lox(th, rh, u) } else { $lo(th, rh, u) }) + w;
    let zh = yh + yl;
    let zl = (yh - zh) + yl;
    (zh, zl, f64::from_bits((kb >> 7) << 52))
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

/// The correctly rounded `2^x` (round to nearest) when the quick phase's test decides it, else
/// `None`.
$(#[$a])*
$(#[$i])*
pub fn exp2_quick(x: f64) -> Option<f64> {
    let (zh, zl, sc) = exp2_quick_parts(x)?;
    let v = zh * sc;
    let w = zl * $cf;
    if zh + w == zh && w != 0.0 { Some(v) } else { None }
}

/// `exp2`'s in-place fast path (as [`exp_tiered`]): the quick phase, then on a miss tier 2
/// (`fast64::exp2_nearest` in the same build, out of line; it answers integers exactly).
$(#[$a])*
$(#[$i])*
pub fn exp2_tiered(x: f64) -> Option<f64> {
    let Some((zh, zl, sc)) = exp2_quick_parts(x) else { return tier2_exp2(x) };
    let v = zh * sc;
    let w = zl * $cf;
    if zh + w == zh && w != 0.0 {
        return Some(v);
    }
    tier2_exp2(x)
}

/// The correctly rounded `10^x` (round to nearest) when the quick phase's test decides it, else
/// `None`.
$(#[$a])*
$(#[$i])*
pub fn exp10_quick(x: f64) -> Option<f64> {
    let (zh, zl, sc) = exp10_quick_parts(x)?;
    let v = zh * sc;
    let w = zl * $cf;
    if zh + w == zh && w != 0.0 { Some(v) } else { None }
}

/// `exp10`'s in-place fast path (as [`exp_tiered`]): the quick phase, then on a miss tier 2
/// (`fast64::exp10_nearest` in the same build, out of line).
$(#[$a])*
$(#[$i])*
pub fn exp10_tiered(x: f64) -> Option<f64> {
    let Some((zh, zl, sc)) = exp10_quick_parts(x) else { return tier2_exp10(x) };
    let v = zh * sc;
    let w = zl * $cf;
    if zh + w == zh && w != 0.0 {
        return Some(v);
    }
    tier2_exp10(x)
}

/// `e^x − 1 = zh + zl` within `RHO_M1·|zh|` for `2^-60 <= |x| < 708`, `x > −37`, else `None`
/// (module doc, "expm1"): the series with an exact head below `2^-5`, else exp's kernel with the
/// `−1` folded in before rounding.
$(#[$a])*
$(#[$i])*
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub fn expm1_quick_parts(x: f64) -> Option<(f64, f64)> {
    let b = x.to_bits();
    let a = b & ABS;
    if a.wrapping_sub(LO_BITS) >= SPAN || b >= NEG37_BITS {
        return None;
    }
    if a < SMALL_M1 {
        let xh = f64::from_bits(b & MASK27);
        let xl = x - xh;
        let hh = 0.5 * (xh * xh);
        let hl = (0.5 * xl) * (x + xh);
        let s = x * x;
        let p01 = $ma(x, C4, C3);
        let p23 = $ma(x, C6, C5);
        let p4 = $ma(s, C9, $ma(x, C8, C7));
        let p = $ma(s, $ma(s, p4, p23), p01);
        let tl = (x * s) * p;
        let h0 = x + hh;
        let lo = ((x - h0) + hh) + (hl + tl);
        let zh = h0 + lo;
        return Some((zh, (h0 - zh) + lo));
    }
    let tt = $ma(x, INV, SHIFT);
    let kd = tt - SHIFT;
    let kb = tt.to_bits().wrapping_sub(KBIAS);
    let r1 = $ma(kd, NL1, x);
    let rh = $ma(kd, NL2, r1);
    let rl = $ma(kd, NL2, r1 - rh);
    let (zh, zl, sc) = kernel::<true>(kb, rh, rl);
    let vh = zh * sc;
    let s = vh - 1.0;
    let bb = s - vh;
    let lo = ((vh - (s - bb)) + (-1.0 - bb)) + zl * sc;
    let h = s + lo;
    Some((h, (s - h) + lo))
}

/// The correctly rounded `e^x − 1` (round to nearest) when the quick phase's test decides it,
/// else `None`.
$(#[$a])*
$(#[$i])*
pub fn expm1_quick(x: f64) -> Option<f64> {
    let (zh, zl) = expm1_quick_parts(x)?;
    let w = zl * C_M1;
    if zh + w == zh && w != 0.0 { Some(zh) } else { None }
}

/// `expm1`'s in-place fast path (as [`exp_tiered`]): the quick phase, then on a miss or outside
/// its domain tier 2 (`fast64::expm1_nearest` in the same build, out of line).
$(#[$a])*
$(#[$i])*
pub fn expm1_tiered(x: f64) -> Option<f64> {
    let Some((zh, zl)) = expm1_quick_parts(x) else { return tier2_expm1(x) };
    let w = zl * C_M1;
    if zh + w == zh && w != 0.0 {
        return Some(zh);
    }
    tier2_expm1(x)
}
    };
}

/// Tier 2 of [`expm1_tiered`] (and its answer outside the quick domain), out of line and cold.
#[cold]
#[inline(never)]
fn tier2_expm1(x: f64) -> Option<f64> {
    super::expm1_nearest(x)
}

/// Tier 2 of [`exp2_tiered`] (and its answer outside the quick domain: NaN, infinities, integers),
/// out of line and cold.
#[cold]
#[inline(never)]
fn tier2_exp2(x: f64) -> Option<f64> {
    super::exp2_nearest(x)
}

/// Tier 2 of [`exp10_tiered`] (as [`tier2_exp2`]).
#[cold]
#[inline(never)]
fn tier2_exp10(x: f64) -> Option<f64> {
    super::exp10_nearest(x)
}

/// Tier 2 of [`exp_tiered`] (the SSE2 build of [`super::exp_nearest`]), out of line and cold so
/// the quick phase keeps its registers: about 0.5% of calls reach it.
#[cold]
#[inline(never)]
fn tier2(x: f64) -> Option<f64> {
    super::exp_nearest(x)
}

quick!(; #[inline(always)]; ma; no_lo; dekker_lo; C_SSE2; C_FAM_SSE2);

/// The quick phase's FMA build (D-10.25, D-10.50): callable only on a CPU with FMA whose AVX state
/// the OS saves; the boundary chooses it once at startup (`dispatch::fma`).
// ARCH(x86_64): the run-time FMA build; FMA is base ISA on aarch64, so one build there
#[cfg(target_arch = "x86_64")]
pub mod fused {
    use super::{
        t, ABS, C3, C4, C5, C6, C7, C8, C9, C_FAM_FMA, C_FMA, C_M1, INV, INV10, KBIAS, LN10H, LN10L, LN2H, LN2L, LO_BITS, MASK27, NE0, NE1, NE2,
        NEG37_BITS, NL1, NL2, ONE_BITS, SHIFT, SMALL_M1, SPAN, SPAN10, SPAN2, SPAN_EXACT10,
    };
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

    /// Tier 2 of [`exp2_tiered`] in the FMA build (`fast64::fused::exp2_nearest`), out of line
    /// and cold.
    #[target_feature(enable = "fma")]
    #[cold]
    #[inline(never)]
    fn tier2_exp2(x: f64) -> Option<f64> {
        crate::fast64::fused::exp2_nearest(x)
    }

    /// Tier 2 of [`exp10_tiered`] in the FMA build (`fast64::fused::exp10_nearest`), out of
    /// line and cold.
    #[target_feature(enable = "fma")]
    #[cold]
    #[inline(never)]
    fn tier2_exp10(x: f64) -> Option<f64> {
        crate::fast64::fused::exp10_nearest(x)
    }

    /// Tier 2 of [`expm1_tiered`] in the FMA build (`fast64::fused::expm1_nearest`), out of
    /// line and cold.
    #[target_feature(enable = "fma")]
    #[cold]
    #[inline(never)]
    fn tier2_expm1(x: f64) -> Option<f64> {
        crate::fast64::fused::expm1_nearest(x)
    }

    quick!(
        #[doc = ""]
        #[doc = "# Safety"]
        #[doc = ""]
        #[doc = "The CPU must support FMA and the OS must save the AVX state (the `#[target_feature]` contract)."]
        #[target_feature(enable = "fma")]; #[inline]; ma; lo; lo; C_FMA; C_FAM_FMA
    );
}

#[cfg(test)]
mod tests;
