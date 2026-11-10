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
//!
//! Stated `RHO = 2^-64` for both builds (the FMA build has fewer roundings). Expected fallback
//! `≈ C − 1 = 0.098%` of uniformly spread significands; the fallback test measures it.
//!
//! Both builds are correctly rounded wherever they answer, so they return the same values.
//!
//! **Pair arguments** (LIBM-F64-INVHYP-QUICK, D-10.65): `log_pair_parts(xh, xl)` and
//! `log1p_pair_parts(yh, yl)` take the argument as an unevaluated sum, for asinh/acosh/atanh
//! ([`super::quick_invhyp`]). With `xh` reduced as above, `x = 2^e·(m + xl·2^-e)` and
//! `c·m·(1 + δ) − 1 = r + d`, `d = RN(c·(xl·2^-e))` (`2^-e` exact, one rounding: `u·|d|`), and
//! `log1p(r + d) = log1p(r) + d/(1 + r) − d²/(2(1 + r)²) + …`. `c·2^-e` comes from the bits as
//! for `log1p` below (normal for `e` in `[-1000, 1001]`, so `d = RN(xl·(c·2^-e))` is the same single
//! rounding). The kernel adds `dt = d·(1 − r + r²)` into `lo` (`log`'s low sum, as `log1p`'s
//! correction below), leaving `|d|·|r|³/(1 − |r|) + d²/(1 − |r|)²` (the roundings
//! forming `dt` act on `|d|·|r| < 2^-58`, below `2^-110`). `log1p_pair_parts` with `yh < 2^-7`
//! passes `(r, d) = (yh, yl)` straight to case A (`c = 1`, `e = 0`, `th = tl = 0`, no reduction);
//! otherwise `1 + yh = s + s'` by 2Sum and `(s, s' + yl)` goes to `log_pair_parts`.
//! * Case A (`0 <= r < 2^-7`): the new relative terms are `E1 = |d|·r²/(1 − 2^-7)` and
//!   `E2 = d²/(2|r|)`. Each caller keeps `E1 + E2 <= 2^-64.99` (`|d| <= 2^-51·|r|` gives
//!   `E1 <= 2^-64.99`, `E2 <= 2^-103`; [`super::quick_invhyp`] states each caller's `d`); with
//!   case A's `2^-64.92`: `2^-63.95`.
//! * Cases B and C (`log_pair_parts`, only for `xh + xl >= 2.4`, so `|ln x| >= 0.87`):
//!   `|d| <= 1.27·2^-51` absolute (`|xl| <= 1.25·2^-51·xh`), so the new terms are below
//!   `2^-72.9` absolute; with B/C's `2^-67.64`: below `2^-67.4` relative.
//!
//! Stated `RHO_PAIR = 2^-63` (`C_PAIR − 1 = 0.196%`, the expected fallback); the callers add their
//! own argument errors (below `2^-74`) in [`super::quick_invhyp`]'s bound.
//!
//! **`log2`, `log10`** (D-10.66, LIBM-F64-LOG-FAMILY-WIRE): `log_b x = ln x · k`, `k = 1/ln b`, as
//! one double-double product of `log`'s unrounded `(zh, zl)` and `k`, then Fast2Sum
//! (`INV_LN2`, `INV_LN10`: `[k0, k1, k26, k26l]`, `k0 = RN(k)`, `k1 = RN(k − k0)`, `k26` = `k0`
//! cut to 26 significant bits, `k26l = RN(k − k26)`; the `bounds` test checks them). A scaled
//! table (`−log_b c_j`, `log_b 2` on a grid) was weighed and not taken: the leading `r − r²/2`
//! still needs a double-double product by `k` there, so it saves no operation for `log10` and at
//! most one for `log2`, for 4 KiB more table per function.
//! * FMA build: `p = RN(zh·k0)`, `p' = zh·k0 − p` (exact, 2MultFMA), `lo = zh·k1 + (zl·k0 + p')`
//!   (two FMAs). Error below `2^-103·|p|` (the roundings of `lo`, `|lo| <= 2^-51.4·|p|`; the
//!   dropped `zl·k1`; `|k − k0 − k1| <= 2^-106·k`); stated `2^-100`.
//! * SSE2 build: `zhh` = `zh` cut to 26 bits (one mask), `ph = zhh·k26` exact (26 × 26 bits),
//!   `lo = zhh·k26l + ((zh − zhh) + zl)·k0`: seven terms each below `2^-78·|p|` (`|zh − zhh| <
//!   2^-25·|zh|`, `|k26l| < 2^-25·k`), so below `2^-75.2·|p|`; stated `2^-74`.
//! * Bound: `log`'s worst case (A, `2^-64.92`) plus `2^-74`, below `RHO`; same `C`. Exact
//!   results stay exact: `log_b x` is rational only at `x = b^n` (`x = M·2^E` with `M` odd; for
//!   `b = 10` that forces `x = 10^n`, `0 <= n <= 22` among doubles), where it is the integer
//!   `n`; then `|zh + zl − n| <= RHO·|n|` and `zh = RN(zh + zl)` give `zh = n` whatever the test
//!   says, and at `x = 1` every term is `+0`. Elsewhere `log_b x` is transcendental, so neither a
//!   double nor a midpoint, and the rounding test's proof holds.
//! * Flags: an answer of the in-place fast path must be inexact (the boundary's `elem_fast`
//!   leaves MXCSR as the arithmetic left it), but at `x = b^n`, `n != 0`, the product raises
//!   inexact for an exact result. So `log2_tiered` answers `x = 2^e` with `e` and
//!   `log10_tiered` answers `x = 10^n` with `n` (a 71-entry table by binade) before any
//!   arithmetic (the conversion `i32 → f64` and the stored `n` raise nothing). `x = 1` stays on the quick phase
//!   (every term `+0`, no flag). The value-only `log2_quick`, `log10_quick` keep the rule above.
//!
//! **`log1p`** (D-10.66): the reduction must keep the low bits of `x`, which `RN(1 + x)` loses.
//! * `2^-60 <= |x| < 2^-8`: `log1p(x)` by the series above with `r = x` exactly, `th = tl = 0`
//!   (case A with `|r| < 2^-8`: below `2^-67`). No product underflows (`|x|^10 >= 2^-600`).
//!   `|x| < 2^-60` (and `±0`) goes to tier 2, which keeps the sign of zero and the flags.
//! * `|x| >= 2^-8`, `−1 < x < 2^960`: `1 + x = yh + yl` exactly (Fast2Sum, larger operand
//!   first), `yh >= 2^-53` positive normal. `ln(1 + x) = ln yh + log1p(yl/yh)`; `yh = 2^e·m`,
//!   `c·m = 1 + r`, so `yl/yh = d/(1 + r)` with `d = yl·(c·2^-e)` (`c·2^-e` from the bits:
//!   `bits(c) − (e << 52)`, normal for `e` in `[−53, 960]`), `|d| <= 2^-53·(1 + 2^-7)`. The
//!   correction `d·(1 + (r² − r))` misses `d·r³/(1 + r)` (`<= 2^-74.0`), `(yl/yh)²/2 <= 2^-106`
//!   and its roundings (`< 2^-104`); it joins `log`'s low sum. With `|ln(1 + x)| >= ln(1 + 2^-8)
//!   = 2^-8.006`: relative `2^-65.99` on top of `log`'s cases (relative to `ln yh`, within
//!   `2^-44` of relative to `ln(1 + x)`): `2^-64.34` at worst, below `RHO`; same `C`.
//!   `ln(1 + x)` is transcendental for every double `x != 0` (Lindemann–Weierstrass), so the
//!   rounding test's proof holds.

#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)] // bit arithmetic on bounded values (above), indices masked to 0..127

use crate::coeffs::tables as t;

/// `bits(2^-1022)` and `bits(+inf) − bits(2^-1022)`: positive normal finite `x` as one unsigned
/// compare on its bits.
pub(super) const MIN_NORMAL: u64 = 0x0010_0000_0000_0000;
pub(super) const SPAN: u64 = 0x7ff0_0000_0000_0000 - MIN_NORMAL;
/// `bits(45/64)`: the low end of `m`'s range; its 45 low fraction bits are 0, so the intervals
/// line up with `F_j`.
pub(super) const OFF: u64 = 0x3fe6_8000_0000_0000;
/// The 45 fraction bits below the interval index.
pub(super) const LOW45: u64 = (1 << 45) - 1;
pub(super) const KH: f64 = f64::from_bits(t::LN2_Q[0]);
pub(super) const KL: f64 = f64::from_bits(t::LN2_Q[1]);
/// `1.5·2^19`: `RN(r + SPLIT) − SPLIT` is `r` rounded to a multiple of `2^-33` for `|r| < 2^18`.
pub(super) const SPLIT: f64 = 786_432.0;
/// `RN(±1/n)`, n = 3..9 (checked by the `bounds` test).
pub(super) const C3: f64 = 1.0 / 3.0;
pub(super) const C4: f64 = -0.25;
pub(super) const C5: f64 = 1.0 / 5.0;
pub(super) const C6: f64 = -1.0 / 6.0;
pub(super) const C7: f64 = 1.0 / 7.0;
pub(super) const C8: f64 = -0.125;
pub(super) const C9: f64 = 1.0 / 9.0;

/// `[k0, k1, k26, k26l]` for `k = 1/ln 2` (module doc, `log2`).
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub const INV_LN2: [f64; 4] = [
    f64::from_bits(0x3ff7_1547_652b_82fe),
    f64::from_bits(0x3c77_77d0_ffda_0d24),
    f64::from_bits(0x3ff7_1547_6000_0000),
    f64::from_bits(0x3e54_ae0b_f85d_df44),
];
/// `[k0, k1, k26, k26l]` for `k = 1/ln 10` (module doc, `log10`).
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub const INV_LN10: [f64; 4] = [
    f64::from_bits(0x3fdb_cb7b_1526_e50e),
    f64::from_bits(0x3c69_5355_baaa_fad3),
    f64::from_bits(0x3fdb_cb7b_1000_0000),
    f64::from_bits(0x3e34_9b94_38ca_9aae),
];
/// The 27 low bits `scale` cuts from `zh` (SSE2 build).
const LOW27: u64 = (1 << 27) - 1;
/// `log1p`'s regions (module doc): `bits(2^-60)`, `bits(2^-8)`, and the top `2^960`.
const L1P_TINY: u64 = 0x3c30_0000_0000_0000;
const L1P_SMALL: u64 = 0x3f70_0000_0000_0000;
const L1P_MAX: f64 = f64::from_bits(0x7bf0_0000_0000_0000);
const SIGN: u64 = 1 << 63;
/// `log10`'s exact results (module doc, `log2`/`log10` flags): entry `i` is `(bits(10^n), n)` for
/// the power of ten `10^n` (`1 <= n <= 22`, exact doubles) whose biased exponent is `1026 + i`,
/// else `(0, 0)`; a binade holds at most one power of ten (`10 > 2^3`). `10^1` has biased
/// exponent 1026 and `10^22` 1096, hence 71 entries.
const POW10_BY_BINADE: [(u64, f64); 71] = {
    let mut v = [(0u64, 0.0f64); 71];
    let (mut i, mut e, mut p, mut n) = (0usize, 1026u64, 10.0f64, 1.0f64);
    while i < 71 {
        // `p = 10^n` exactly while `n <= 22` (`5^22 < 2^53`); the guard drops 10^23.
        if n <= 22.0 && p.to_bits() >> 52 == e {
            v[i] = (p.to_bits(), n);
            p *= 10.0;
            n += 1.0;
        }
        i += 1;
        e += 1;
    }
    v
};

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
pub(in crate::fast64) fn reduce(m: f64, mb: u64, c: f64, b: f64) -> f64 {
    c * (m - f64::from_bits(mb & !LOW45)) + b
}

/// `(vh, vl, RN(r²))` with `vh + vl = r − r²/2` within `2^-87·|r|` (SSE2: the split of `r`).
#[inline(always)]
pub(in crate::fast64) fn half_square(r: f64) -> (f64, f64, f64) {
    let rh = (r + SPLIT) - SPLIT;
    let rl = r - rh;
    let hh = (-0.5 * rh) * rh;
    let vh = r + hh;
    let vl = ((r - vh) + hh) - (0.5 * rl) * (r + rh);
    (vh, vl, r * r)
}

/// `zh·k + zl·k` as a pair within `2^-74·|p|` (SSE2 build; module doc, `log2`, `log10`).
#[inline(always)]
fn scale(zh: f64, zl: f64, k: &[f64; 4]) -> (f64, f64) {
    let zhh = f64::from_bits(zh.to_bits() & !LOW27);
    let ph = zhh * k[2];
    let lo = zhh * k[3] + ((zh - zhh) + zl) * k[0];
    let p = ph + lo;
    (p, (ph - p) + lo)
}

// The kernel in two builds (D-10.25, D-10.50): `$ma` is `a·b + c` (two roundings, or one FMA),
// `$red` the exact reduction, `$sq` the half-square, `$sc` the product by `1/ln b`. `$d` documents
// the public functions, `$tf` is the build's target feature, `$i` its inline attribute.
macro_rules! quick {
    ($(#[$d:meta])*; $(#[$tf:meta])*; $(#[$i:meta])*; $ma:path; $red:path; $sq:path; $sc:path) => {
/// `(r, th, tl, bits(c_j·2^-e))` of positive normal finite `y` (bits `iy`): `ln y = th + tl +
/// log1p(r)` (module doc); the last is `log1p`'s correction scale.
$(#[$tf])*
$(#[$i])*
fn split(iy: u64) -> (f64, f64, f64, u64) {
    let tmp = iy.wrapping_sub(OFF);
    let e = i64::from_ne_bytes(tmp.to_ne_bytes()) >> 52;
    let eb = tmp & (0xfff << 52);
    let mb = iy.wrapping_sub(eb);
    let row = &t::LOG_Q128[usize::try_from((tmp >> 45) & 127).unwrap_or(0)];
    let r = $red(f64::from_bits(mb), mb, f64::from_bits(row[0]), f64::from_bits(row[1]));
    let ef = f64::from(i32::try_from(e).unwrap_or(0));
    let th = $ma(ef, KH, f64::from_bits(row[2]));
    let tl = $ma(ef, KL, f64::from_bits(row[3]));
    (r, th, tl, row[0].wrapping_sub(eb))
}

/// `(vh, vl, r³, q)` with `log1p(r) ≈ (vh + vl) + r³·q` (module doc).
$(#[$tf])*
$(#[$i])*
fn series(r: f64) -> (f64, f64, f64, f64) {
    let (vh, vl, r2) = $sq(r);
    let p01 = $ma(r, C4, C3);
    let p23 = $ma(r, C6, C5);
    let p45 = $ma(r, C8, C7);
    let r4 = r2 * r2;
    let qa = $ma(r2, p23, p01);
    let qb = $ma(r2, C9, p45);
    let q = $ma(r4, qb, qa);
    (vh, vl, r2 * r, q)
}

/// `ln y = zh + zl` from [`split`]'s parts; with `X`, `extra` (`log1p`'s correction) joins the
/// low sum.
$(#[$tf])*
$(#[$i])*
fn join<const X: bool>(r: f64, th: f64, tl: f64, extra: f64) -> (f64, f64) {
    let (vh, vl, r3, q) = series(r);
    let sh = th + vh;
    let sl = (th - sh) + vh;
    let low = (sl + vl) + tl;
    let lo = $ma(r3, q, if X { low + extra } else { low });
    let zh = sh + lo;
    let zl = (sh - zh) + lo;
    (zh, zl)
}

/// `ln x = zh + zl` within `RHO·|zh|` for positive normal finite `x`, else `None` (module doc).
$(#[$d])*
$(#[$tf])*
$(#[$i])*
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub fn log_quick_parts(x: f64) -> Option<(f64, f64)> {
    let ix = x.to_bits();
    if ix.wrapping_sub(MIN_NORMAL) >= SPAN {
        return None;
    }
    let (r, th, tl, _) = split(ix);
    Some(join::<false>(r, th, tl, 0.0))
}

/// The correctly rounded `ln x` (round to nearest) when the quick phase's test decides it, else
/// `None` (the caller runs [`super::log_nearest`]).
$(#[$d])*
$(#[$tf])*
$(#[$i])*
pub fn log_quick(x: f64) -> Option<f64> {
    let (zh, zl) = log_quick_parts(x)?;
    if zh + zl * C == zh { Some(zh) } else { None }
}

/// `log`'s in-place fast path (D-10.51): the quick phase, then on a miss or outside its domain
/// tier 2 (`fast64::log_nearest` in the same build, out of line). `None` when tier 2 declines
/// too: the caller then runs the accurate path.
$(#[$d])*
$(#[$tf])*
$(#[$i])*
pub fn log_tiered(x: f64) -> Option<f64> {
    if let Some((zh, zl)) = log_quick_parts(x) {
        if zh + zl * C == zh {
            return Some(zh);
        }
    }
    tier2(x)
}

/// `log1p(r + d)` plus `th + tl` as `(zh, zl)`: [`join`] with the low part `d` of the reduced
/// argument folded in as `d·(1 − r + r²)` (module doc, "Pair arguments").
$(#[$tf])*
$(#[$i])*
fn pair_tail(r: f64, d: f64, th: f64, tl: f64) -> (f64, f64) {
    let dt = $ma(d, $ma(r, r, -r), d);
    join::<true>(r, th, tl, dt)
}

/// `ln(xh + xl) = zh + zl` within `RHO_PAIR·|zh|` for `xh` in `[2^-1000, 2^1001]` and
/// `|xl| <= 2^-51·xh`, `xh + xl >= 2.4` (the inverse hyperbolic kernels' range; module doc).
$(#[$d])*
$(#[$tf])*
$(#[$i])*
#[doc(hidden)] // public for the inverse hyperbolic kernels and the boundary's tests
pub fn log_pair_parts(xh: f64, xl: f64) -> (f64, f64) {
    // `cs = bits(c·2^-e)`, normal for `e` in `[-1000, 1001]` (module doc, "Pair arguments").
    let (r, th, tl, cs) = split(xh.to_bits());
    pair_tail(r, xl * f64::from_bits(cs), th, tl)
}

/// `log1p(yh + yl) = zh + zl` within `RHO_PAIR·|zh|` for `yh` in `[2^-200, 2^1000]` and
/// `|yl| <= 2^-51·yh` (module doc).
$(#[$d])*
$(#[$tf])*
$(#[$i])*
#[doc(hidden)] // public for the inverse hyperbolic kernels and the boundary's tests
pub fn log1p_pair_parts(yh: f64, yl: f64) -> (f64, f64) {
    if yh < SMALL {
        // 1 + y in [1, 1 + 2^-7): the interval with c = 1, e = 0, so r = y exactly.
        return pair_tail(yh, yl, 0.0, 0.0);
    }
    // 2Sum (Knuth, TAOCP 2, 4.2.2): 1 + yh = s + s' exactly, in either order of magnitude.
    let s = 1.0 + yh;
    let bv = s - 1.0;
    let sl = ((1.0 - (s - bv)) + (yh - bv)) + yl;
    log_pair_parts(s, sl)
}

/// `log2 x = zh + zl` within `RHO·|zh|` for positive normal finite `x`, else `None` (module
/// doc, `log2`).
$(#[$d])*
$(#[$tf])*
$(#[$i])*
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub fn log2_quick_parts(x: f64) -> Option<(f64, f64)> {
    let (zh, zl) = log_quick_parts(x)?;
    Some($sc(zh, zl, &INV_LN2))
}

/// The correctly rounded `log2 x` when the quick phase's test decides it, else `None`.
$(#[$d])*
$(#[$tf])*
$(#[$i])*
pub fn log2_quick(x: f64) -> Option<f64> {
    let (zh, zl) = log2_quick_parts(x)?;
    if zh + zl * C == zh { Some(zh) } else { None }
}

/// `log2`'s in-place fast path (D-10.66): the quick phase, then tier 2 (`fast64::log2_nearest`
/// in the same build, out of line); `None` when tier 2 declines too.
$(#[$d])*
$(#[$tf])*
$(#[$i])*
pub fn log2_tiered(x: f64) -> Option<f64> {
    let ix = x.to_bits();
    // x = 2^e (positive normal, fraction 0): `e` exactly and no flag, before any arithmetic
    // (the quick phase would raise inexact; module doc).
    if ix << 12 == 0 && ix.wrapping_sub(MIN_NORMAL) < SPAN {
        return Some(f64::from(i32::try_from(ix >> 52).unwrap_or(0) - 1023));
    }
    if let Some((zh, zl)) = log2_quick_parts(x) {
        if zh + zl * C == zh {
            return Some(zh);
        }
    }
    tier2_log2(x)
}

/// `log10 x = zh + zl` within `RHO·|zh|` for positive normal finite `x`, else `None` (module
/// doc, `log10`).
$(#[$d])*
$(#[$tf])*
$(#[$i])*
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub fn log10_quick_parts(x: f64) -> Option<(f64, f64)> {
    let (zh, zl) = log_quick_parts(x)?;
    Some($sc(zh, zl, &INV_LN10))
}

/// The correctly rounded `log10 x` when the quick phase's test decides it, else `None`.
$(#[$d])*
$(#[$tf])*
$(#[$i])*
pub fn log10_quick(x: f64) -> Option<f64> {
    let (zh, zl) = log10_quick_parts(x)?;
    if zh + zl * C == zh { Some(zh) } else { None }
}

/// `log10`'s in-place fast path (D-10.66): the quick phase, then tier 2
/// (`fast64::log10_nearest` in the same build, out of line); `None` when tier 2 declines too.
$(#[$d])*
$(#[$tf])*
$(#[$i])*
pub fn log10_tiered(x: f64) -> Option<f64> {
    let ix = x.to_bits();
    // x = 10^n, 1 <= n <= 22: `n` exactly and no flag, before any arithmetic (module doc).
    let i = (ix >> 52).wrapping_sub(1026);
    if i < 71 {
        let (b, n) = POW10_BY_BINADE[usize::try_from(i).unwrap_or(0)];
        if b == ix {
            return Some(n);
        }
    }
    if let Some((zh, zl)) = log10_quick_parts(x) {
        if zh + zl * C == zh {
            return Some(zh);
        }
    }
    tier2_log10(x)
}

/// `ln(1 + x) = zh + zl` within `RHO·|zh|` for `2^-60 <= |x|`, `−1 < x < 2^960`, else `None`
/// (module doc, `log1p`).
$(#[$d])*
$(#[$tf])*
$(#[$i])*
#[doc(hidden)] // public for the boundary's FMA-build tests only
pub fn log1p_quick_parts(x: f64) -> Option<(f64, f64)> {
    let ax = x.to_bits() & !SIGN;
    if ax < L1P_SMALL {
        if ax < L1P_TINY {
            return None;
        }
        let (vh, vl, r3, q) = series(x);
        let lo = $ma(r3, q, vl);
        let zh = vh + lo;
        return Some((zh, (vh - zh) + lo));
    }
    if !(x > -1.0 && x < L1P_MAX) {
        return None;
    }
    let (a, b) = if x > 1.0 { (x, 1.0) } else { (1.0, x) };
    let yh = a + b;
    let yl = (a - yh) + b;
    let (r, th, tl, cs) = split(yh.to_bits());
    let d = yl * f64::from_bits(cs);
    let corr = $ma(d, $ma(r, r, -r), d);
    Some(join::<true>(r, th, tl, corr))
}

/// The correctly rounded `log1p x` when the quick phase's test decides it, else `None`.
$(#[$d])*
$(#[$tf])*
$(#[$i])*
pub fn log1p_quick(x: f64) -> Option<f64> {
    let (zh, zl) = log1p_quick_parts(x)?;
    if zh + zl * C == zh { Some(zh) } else { None }
}

/// `log1p`'s in-place fast path (D-10.66): the quick phase, then tier 2
/// (`fast64::log1p_nearest` in the same build, out of line); `None` when tier 2 declines too.
$(#[$d])*
$(#[$tf])*
$(#[$i])*
pub fn log1p_tiered(x: f64) -> Option<f64> {
    if let Some((zh, zl)) = log1p_quick_parts(x) {
        if zh + zl * C == zh {
            return Some(zh);
        }
    }
    tier2_log1p(x)
}
    };
}

/// `2^-7`: below it `log1p`'s pair argument is the reduced argument itself.
const SMALL: f64 = 0.0078125;

/// The pair entries' stated bound (module doc, "Pair arguments") and its rounding-test
/// multiplier: the least double with `C_PAIR·(1 − 2^54·RHO_PAIR) > 1 + 2^-53`.
#[doc(hidden)] // public for the boundary's tests only
pub const RHO_PAIR: f64 = f64::from_bits(0x3c00_0000_0000_0000);
#[doc(hidden)] // public for the inverse hyperbolic kernels and the boundary's tests
pub const C_PAIR: f64 = f64::from_bits(0x3ff0_0804_0201_0081);

/// Tier 2 of [`log_tiered`] (the SSE2 build of [`super::log_nearest`]), out of line and cold so
/// the quick phase keeps its registers: about 0.1% of calls reach it.
#[cold]
#[inline(never)]
fn tier2(x: f64) -> Option<f64> {
    super::log_nearest(x)
}

/// Tier 2 of [`log2_tiered`] (SSE2 build), out of line and cold.
#[cold]
#[inline(never)]
fn tier2_log2(x: f64) -> Option<f64> {
    super::log2_nearest(x)
}

/// Tier 2 of [`log10_tiered`] (SSE2 build), out of line and cold.
#[cold]
#[inline(never)]
fn tier2_log10(x: f64) -> Option<f64> {
    super::log10_nearest(x)
}

/// Tier 2 of [`log1p_tiered`] (SSE2 build), out of line and cold.
#[cold]
#[inline(never)]
fn tier2_log1p(x: f64) -> Option<f64> {
    super::log1p_nearest(x)
}

quick!(; ; #[inline(always)]; ma; reduce; half_square; scale);

/// The quick phase's FMA build (D-10.25, D-10.50): callable only on a CPU with FMA whose AVX state
/// the OS saves; the boundary chooses it once at startup (`dispatch::fma`).
// ARCH(x86_64): the run-time FMA build; FMA is base ISA on aarch64, so one build there
#[cfg(target_arch = "x86_64")]
pub mod fused {
    use super::{t, C, C3, C4, C5, C6, C7, C8, C9, INV_LN10, INV_LN2, KH, KL, L1P_MAX, L1P_SMALL, L1P_TINY, MIN_NORMAL, OFF, POW10_BY_BINADE, SIGN, SMALL, SPAN};
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
    pub(in crate::fast64) fn reduce(m: f64, _mb: u64, c: f64, _b: f64) -> f64 {
        _mm_cvtsd_f64(_mm_fmsub_sd(_mm_set_sd(m), _mm_set_sd(c), _mm_set_sd(1.0)))
    }

    /// `(vh, vl, s)` with `s = RN(r²)`, `r² = s + s'` exactly (2MultFMA) and `vh + vl = r − r²/2`
    /// within `u²·|r|`.
    #[target_feature(enable = "fma")]
    #[inline]
    pub(in crate::fast64) fn half_square(r: f64) -> (f64, f64, f64) {
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

    /// `zh·k + zl·k` as a pair within `2^-100·|p|` (module doc, `log2`, `log10`): 2MultFMA.
    #[target_feature(enable = "fma")]
    #[inline]
    fn scale(zh: f64, zl: f64, k: &[f64; 4]) -> (f64, f64) {
        let p = zh * k[0];
        let pl = _mm_cvtsd_f64(_mm_fmsub_sd(_mm_set_sd(zh), _mm_set_sd(k[0]), _mm_set_sd(p)));
        let lo = ma(zh, k[1], ma(zl, k[0], pl));
        let h = p + lo;
        (h, (p - h) + lo)
    }

    /// Tier 2 of [`log2_tiered`] in the FMA build, out of line and cold.
    #[target_feature(enable = "fma")]
    #[cold]
    #[inline(never)]
    fn tier2_log2(x: f64) -> Option<f64> {
        crate::fast64::fused::log2_nearest(x)
    }

    /// Tier 2 of [`log10_tiered`] in the FMA build, out of line and cold.
    #[target_feature(enable = "fma")]
    #[cold]
    #[inline(never)]
    fn tier2_log10(x: f64) -> Option<f64> {
        crate::fast64::fused::log10_nearest(x)
    }

    /// Tier 2 of [`log1p_tiered`] in the FMA build, out of line and cold.
    #[target_feature(enable = "fma")]
    #[cold]
    #[inline(never)]
    fn tier2_log1p(x: f64) -> Option<f64> {
        crate::fast64::fused::log1p_nearest(x)
    }

    quick!(
        #[doc = ""]
        #[doc = "# Safety"]
        #[doc = ""]
        #[doc = "The CPU must support FMA and the OS must save the AVX state (the `#[target_feature]` contract)."];
        #[target_feature(enable = "fma")]; #[inline]; ma; reduce; half_square; scale
    );
}

#[cfg(test)]
mod tests;
