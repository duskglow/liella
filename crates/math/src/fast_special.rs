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
//!
//! **In-place entries** ([`erf_nearest`], [`erfc_nearest`], LIBM-SPECIAL-FAST-TIER-3): the
//! round-to-nearest results for the boundary's in-place envelope (`mret::elem_fast`, the contract
//! of `fast64::exp_nearest`): `fast64`'s constant test `near` first (one multiply, one add, one
//! compare), `settle` when it misses, `None` for every operand they do not take. `near` needs
//! `eps <= rho·|y.hi|`: `rho = RHO_EXP = 1.125·2^-70` holds for `erf` on `[2^-55, 6)` (2^-71.31 and
//! 2^-73.5 + 2^-104 above), for `erfc` on `(−6, 0)` (`1 + erf|x|` with `|r.hi| >= 1` and
//! `|y.hi| <= |r.hi|`: `2^-71.31 + 2^-103`) and for `erfc` on `[1, 27.25)` (`2^-71.40 + 2^-72 +
//! 2^-49·|tail|`, `|tail| < 2^-24.72` by `fast64::RHO_EXP`'s proof: `0.71·2^-70`). On `(0, 1)`
//! `1 − erf x` carries `erf`'s absolute bound over `|r.hi| >= 0.157`: `eps/|r.hi| <=
//! (0.8427·2^-71.31 + 2^-103)/0.1572 < 2^-68.88`, so that branch uses `RHO_ERFC_LOW = 2^-68` and its
//! multiplier `C_ERFC_LOW`. Results are taken only where they are normal (`erfc`: `m >= -1021` with
//! `y.hi` in `[1, 2)`) and every intermediate is normal, so the arithmetic raises inexact (the test
//! proves an addition inexact) and nothing else; the exact `±1`/`2` limits of `±∞` and the operands
//! below `2^-55` are left to the caller's accurate path.
//!
//! **Builds** (MATH-F64-FMA-PATH, D-10.25): the kernels exist in the baseline build (Dekker's
//! [`two_prod`]) and in [`fused`] (one FMA per exact product, [`dd::two_prod_fma`]). Every product
//! formed is exact in both (operands near 1, far from overflow and underflow), so the two builds
//! return the same pairs bit for bit, take the same branches and return the same results.
//!
//! **Quick phase** (LIBM-F64-SPECIAL-QUICK, D-10.84): a cheaper first evaluation in front of the
//! in-place entries' table path, round-to-nearest only ([`erf_nearest`], [`erfc_nearest`]). For
//! `2^-5 <= |x| < 6` it evaluates the same `ERF_T` row to degree 12 with one exact product:
//! `t² = RN(t·t)`, `t⁴ = RN(t²·t²)`,`a = p3 + p4·t + p5·t²`, `b1 = p6 + … + p9·t³`, `b2 = p10 + p11·t + p12·t²` by
//! `f64` Horner (`P3` by its high part), `b = b1 + t⁴·b2`, `r = P2h + (t·a + t⁴·b)` (Estrin's
//! split, Knuth TAOCP 2 4.6.4: the three chains are independent, so the critical path is about
//! half of degree-12 Horner), `P0h + P1h·t` by the exact product and one Fast2Sum (`|P1h·t| <=
//! |P0h|`), the low parts `s.lo + P0l + (P1h·t)lo + P1l·t + P2l·t²`, then `t²·r` added last and a
//! final Fast2Sum to `(zh, zl)`. The generator proves the bound per slot
//! (`crates/mathtools/src/special_tables.rs`, `quick_bound`: truncation by Cramér's inequality as
//! item 1, every operation a rounding): `|erf x − zh − zl| <= RHO_ERF_Q·|zh|`, `RHO_ERF_Q = 2^-62` (largest
//! slot 2^-62.017 with the generator's 1% margin, near 1).
//! `erfc` on `(−6, 1)` takes `1 ± (zh + zl)` as the in-place entry does: below 0 within
//! `RHO_ERF_Q` (`|r.hi| >= 1`), on `(0, 1)` the absolute bound over `1 − erf b` per slot within
//! `RHO_ERFC_Q_LOW = 2^-61` (same test: 2^-61.55). Rounding: [`fast64`]'s `near` with the matching
//! multiplier (de Dinechin, Lauter & Muller, RAIRO-ITA 41, 2007); `erf` and `erfc` are never a
//! double or a midpoint at a double `x != 0`, so `w != 0` holds wherever the test answers. Every
//! operand the test refuses continues on the table path; the phase sits inside the entries' own
//! `|x| >= 2^-5` branch, so the series path and `erfc`'s `x >= 1` path run with no added compare.
//! Both builds round the same operations (the fused build's only difference is the exact
//! product), so they return the same pairs bit for bit.

#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)] // slot indices bounded by the ranges checked before each lookup

use crate::coeffs::tables as t;
use crate::dd::{self, fast_two_sum, two_prod, two_sum, Dd};
use crate::fast64::{self, finish, C_EXP};
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
/// 2^-63: the in-place `±1` and `2` limits subtract `w` in `[6, 8]·2^-63` (inexact in hardware).
const E63: f64 = f64::from_bits(0x3c00_0000_0000_0000);
/// The relative bound of `erfc`'s in-place `1 − erf x` branch (module doc): 2^-68.
#[allow(dead_code)] // the stated bound; the multiplier below is derived from it (tests)
pub(crate) const RHO_ERFC_LOW: f64 = f64::from_bits(0x3bb0_0000_0000_0000);
/// The least `double` `C` with `C·(1 − 2^54·RHO_ERFC_LOW) > 1 + 2^-53`, the multiplier of
/// `fast64`'s `near` for that bound: `1 + 2^-14 + 2^-28 + 2^-42 + 2^-52` (checked exactly by the
/// tests below).
pub(crate) const C_ERFC_LOW: f64 = f64::from_bits(0x3ff0_0040_0100_0401);
/// The quick phase's bound for `erf` and `erfc` below 0 (module doc, "Quick phase"): 2^-62.
#[allow(dead_code)] // the stated bound; the multiplier below is derived from it (tests)
pub(crate) const RHO_ERF_Q: f64 = f64::from_bits(0x3c10_0000_0000_0000);
/// The least `double` `C` with `C·(1 − 2^54·RHO_ERF_Q) > 1 + 2^-53` (checked by the tests).
pub(crate) const C_ERF_Q: f64 = f64::from_bits(0x3ff0_1010_1010_1011);
/// The quick phase's bound for `erfc` on `(0, 1)`: 2^-61.
#[allow(dead_code)] // as RHO_ERF_Q
pub(crate) const RHO_ERFC_Q_LOW: f64 = f64::from_bits(0x3c20_0000_0000_0000);
/// The least `C` for `RHO_ERFC_Q_LOW` (as `C_ERF_Q`).
pub(crate) const C_ERFC_Q_LOW: f64 = f64::from_bits(0x3ff0_2040_8102_0409);
/// The least `m` of an in-place `erfc` result `y.hi·2^m` (`y.hi` in `[1, 2)`: normal).
const M_NORMAL: i32 = -1021;

/// `v` with `x`'s sign for `v > 0` (`erf |x|` here, never zero or NaN): `−v` for negative `x`,
/// `v` otherwise, the same value as `if x < 0.0 { -v } else { v }` (`x` is not zero or NaN
/// where it is used). One mask and one OR in place of a compare-and-blend select.
#[inline(always)]
fn with_sign_of(v: f64, x: f64) -> f64 {
    f64::from_bits(v.to_bits() | (x.to_bits() & 0x8000_0000_0000_0000))
}

/// `round(c·2^127)` for `c = 2/√π`, as its high and low 64 bits (tools/proofs/erf_tiny_f64.py).
const C127_HI: u64 = 0x906e_ba82_14db_688d;
const C127_LO: u64 = 0x71d4_8a7f_6bfe_c344;
/// `ceil(2^53/c)`: the least 53-bit significand `X` with `X·c >= 2^53`.
const C_T: u64 = 0x001c_5bf8_91b4_ef6b;
/// The largest subnormal significand `m` whose `m·c` is tiny when rounded to 53 bits (below
/// `2^52 − 1/4`); `m·c` for this one rounds to `2^52` on the subnormal grid (the tool above).
const M_TINY: u64 = 0x000e_2dfc_48da_77b5;
/// 2^-60: `x·2^-60` for a subnormal `x` is zero, inexact and tiny.
const E60: f64 = f64::from_bits(0x3c30_0000_0000_0000);

/// `erf x` for `|x| < 2^-55` rounded to nearest with the default environment's flags, by integer
/// arithmetic (LIBM-ERF-TINY-F64; the proof is tools/proofs/erf_tiny_f64.py, case I): `erf x`
/// lies below `c·x` by less than `2^-110/3` relative, and no `c·x` comes within `2^-57.3` ulps of a
/// rounding breakpoint, so the result is `c·x` rounded, `n = round(X'·c/2)` with the 128-bit
/// constant (`X' = X`, `2X` or `2m` in `[2^53/c, 2^54/c)`, error at most `2^-75` ulps); its bits
/// are `(g − 1)·2^52 + n`, the significand's carry into the exponent included. Flags: `x·2^-60`
/// (subnormal `x` up to `M_TINY`) raises inexact and underflow and adds a zero; otherwise
/// `1 + |x|` (exactly 1, inexact) multiplies the result. `±0` returns itself, no flag.
#[inline(always)]
#[allow(clippy::as_conversions, clippy::cast_possible_truncation)] // n < 2^54 after the shift
fn erf_linear_int(x: f64, ax: f64) -> f64 {
    let b = ax.to_bits();
    let e = b >> 52;
    let (xq, base) = if e == 0 {
        (b << 1, 0)
    } else {
        let xm = (b & 0x000f_ffff_ffff_ffff) | 1 << 52;
        if xm >= C_T { (xm, e << 52) } else { (xm << 1, (e - 1) << 52) }
    };
    let hi = u128::from(xq) * u128::from(C127_HI);
    let lo = u128::from(xq) * u128::from(C127_LO);
    let n = ((hi + ((lo + (1 << 127)) >> 64)) >> 64) as u64;
    let v = f64::from_bits((base + n) | (x.to_bits() & 0x8000_0000_0000_0000));
    if b <= M_TINY { v + x * E60 } else { v * (1.0 + ax) }
}

// The kernels and in-place entries in two builds (module doc, "Builds"): `$prod` is the exact
// product, `$exp_dd` the matching build of `fast64::exp_dd`; `$i` is
// the inlining (`#[inline(always)]` cannot be combined with `#[target_feature]`).
macro_rules! kernels {
    ($(#[$a:meta])*; $(#[$i:meta])*; $prod:path; $exp_dd:path; $lin:path) => {
/// The Taylor row `row` (layout in the module doc, `M = 4` double-doubles, degree 14) at `t`.
$(#[$a])*
$(#[$i])*
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
        let p = $prod(sh, t);
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
$(#[$a])*
$(#[$i])*
fn erf_small(x: f64) -> Dd {
    let q = |k: usize| f64::from_bits(t::ERF_SMALL[k]);
    let z = $prod(x, x);
    let mut r = q(8);
    for k in (4..8).rev() {
        r = r * z.hi + q(k);
    }
    let p = $prod(q(2), z.hi);
    let s = fast_two_sum(q(0), p.hi);
    let lo = ((s.lo + q(1)) + (p.lo + (q(2) * z.lo + q(3) * z.hi))) + (z.hi * z.hi) * r;
    let sum = fast_two_sum(s.hi, lo);
    let y = $prod(x, sum.hi);
    fast_two_sum(y.hi, y.lo + x * sum.lo)
}

/// `erf x` for `2^-5 <= x < 6` as `(y, eps)`: `|erf x − y| <= eps` (module doc).
$(#[$a])*
$(#[$i])*
fn erf_table_dd(x: f64) -> (Dd, f64) {
    let (i, c) = slot(x, 0x3fa0);
    let y = taylor(&t::ERF_T[i], x - c);
    (y, dd::abs(y.hi) * f64::from_bits(t::ERF_T_EPS))
}

/// `erf x` for `2^-55 <= x < 2^-5` as `(y, eps)`: `|erf x − y| <= eps` (module doc).
$(#[$a])*
$(#[$i])*
fn erf_small_dd(x: f64) -> (Dd, f64) {
    let y = erf_small(x);
    (y, dd::abs(y.hi) * (f64::from_bits(t::ERF_SMALL_EPS) + E104))
}

/// `erf x` for `0 < x < 6` as `(y, eps, m)`: `|erf x − y·2^m| <= eps·2^m` (module doc).
///
/// # Safety
///
/// As [`erf_nearest`].
$(#[$a])*
$(#[$i])*
pub fn erf_kernel(x: f64) -> (Dd, f64, i32) {
    if x >= LINEAR {
        let (y, eps) = if x >= SMALL { erf_table_dd(x) } else { erf_small_dd(x) };
        return (y, eps, 0);
    }
    let (xs, m) = if x < SCALED {
        // exact: both products are normal; then xs·2^m = x with xs in [1, 2)
        let w = x * P500 * P500;
        let e = i32::try_from((w.to_bits() >> 52) & 0x7ff).unwrap_or(0) - 1023;
        (f64::from_bits((w.to_bits() & 0x000f_ffff_ffff_ffff) | 0x3ff0_0000_0000_0000), e - 1000)
    } else {
        (x, 0)
    };
    let p = $prod(xs, f64::from_bits(t::ERF_SMALL[0]));
    let y = fast_two_sum(p.hi, p.lo + xs * f64::from_bits(t::ERF_SMALL[1]));
    // Scaled: keep y.hi in [1, 2) (exact halving) so `settle` sees a normalized significand.
    let (y, m) = if m != 0 && y.hi >= 2.0 { (y.scale(0.5), m + 1) } else { (y, m) };
    (y, dd::abs(y.hi) * E100, m)
}

/// `erfc x` for `1 <= x < ERFC_MAX` as `(y, eps, m)`, `1 <= y.hi < 2`: `|erfc x − y·2^m| <= eps·2^m`
/// (module doc), or `None` when `e^(−x²)`'s kernel declines.
///
/// # Safety
///
/// As [`erf_nearest`].
$(#[$a])*
$(#[$i])*
pub fn erfc_kernel(x: f64) -> Option<(Dd, f64, i32)> {
    let s = $prod(x, x);
    let (e, m, tail) = $exp_dd(s.neg())?;
    let (i, c) = slot(x, 0x3ff0);
    let b = taylor(&t::ERFCX_T[i], x - c);
    // `Dd::mul` with this build's product (the same arithmetic).
    let p = $prod(e.hi, b.hi);
    let y = fast_two_sum(p.hi, p.lo + (e.hi * b.lo + e.lo * b.hi));
    // y.hi = 2^k·f with f in [1, 2): scale to [1, 2) (exact), m + k.
    let k = i32::try_from((y.hi.to_bits() >> 52) & 0x7ff).unwrap_or(0) - 1023;
    let y = y.scale(dd::pow2(-k));
    let eps = dd::abs(y.hi) * (f64::from_bits(t::ERFCX_T_EPS) + TAIL * dd::abs(tail) + E72);
    Some((y, eps, m + k))
}

/// `erf x` rounded to nearest for the in-place envelope (module doc, "In-place entries"), or
/// `None`: NaN and `±∞` always, and the rare operands both rounding tests refuse. `|x| < 2^-55`
/// is `c·x` rounded (`erf_linear_int`, or one FMA in [`fused`]; LIBM-ERF-TINY-F64).
///
/// # Safety
///
/// In [`fused`] the CPU must support FMA and the OS must save the AVX state (the
/// `#[target_feature]` contract); the baseline build has no requirement.
$(#[$a])*
$(#[$i])*
pub fn erf_nearest(x: f64) -> Option<f64> {
    let ax = dd::abs(x);
    if !(LINEAR..).contains(&ax) {
        // |x| < 2^-55 (LIBM-ERF-TINY-F64): c·x rounded, inside the test the entry already makes,
        // so the typical path sees no new compare. NaN fails both tests.
        if ax < LINEAR {
            return Some($lin(x, ax));
        }
        return None;
    }
    if ax >= ERF_ONE {
        if ax == f64::INFINITY {
            return None;
        }
        // erf x within 2^-55 of ±1 (module doc): ±1, the subtraction inexact.
        let v = 1.0 - ax.min(8.0) * E63;
        return Some(with_sign_of(v, x));
    }
    // The quick phase shares the table branch's range check, so the series path sees no new
    // compare (module doc, "Quick phase"); the table path it falls back to is out of line.
    // `ax >= SMALL` (not `ax < SMALL` with the arms swapped): the ordered `>=` compares `ax`
    // against the constant in memory, `<` first loads the constant (one more instruction on
    // the SSE2 series path).
    let v = if ax >= SMALL {
        match erf_quick_near(ax) {
            Some(v) => v,
            None => erf_table_near(ax)?,
        }
    } else {
        let (y, eps) = erf_small_dd(ax);
        finish(y, Some(C_EXP), 0, || eps, &mut 0)?
    };
    Some(with_sign_of(v, x))
}

/// `erfc x` rounded to nearest for the in-place envelope (module doc, "In-place entries"), or
/// `None`: NaN, `±∞`, `|x| < 2^-55`, results below `2^-1021` and the rare operands both
/// rounding tests refuse.
///
/// # Safety
///
/// As [`erf_nearest`].
$(#[$a])*
$(#[$i])*
pub fn erfc_nearest(x: f64) -> Option<f64> {
    if x < 1.0 {
        let ax = dd::abs(x);
        if !(LINEAR..).contains(&ax) {
            return None;
        }
        if x <= -ERF_ONE {
            if x == f64::NEG_INFINITY {
                return None;
            }
            // erfc x within 2^-55 of 2 (module doc): 2, the subtraction inexact.
            return Some(2.0 - ax.min(8.0) * E63);
        }
        // As in `erf_nearest`: the quick phase inside the table branch only.
        if ax >= SMALL {
            return match erfc_quick_near(x, ax) {
                Some(v) => Some(v),
                None => erfc_table_near(x, ax),
            };
        }
        return erfc_low_near(x, erf_small_dd(ax));
    }
    if !(..ERFC_MAX).contains(&x) {
        return None;
    }
    let (y, eps, m) = erfc_kernel(x)?;
    if m < M_NORMAL {
        return None;
    }
    finish(y, Some(C_EXP), m, || eps, &mut 0)
}
/// The quick phase's `erf x` for `2^-5 <= x < 6` as `(zh, zl)` within `RHO_ERF_Q·|zh|` (module
/// doc, "Quick phase"; the order is the bound's).
$(#[$a])*
$(#[$i])*
fn erf_quick_dd(x: f64) -> Dd {
    let (i, c) = slot(x, 0x3fa0);
    let row = &t::ERF_T[i];
    // [P0h, P0l, P1h, P1l, P2h, P2l, P3h, P3l, p4, ..., p14]: p_k at row[k + 4] from k = 4.
    let p = |k: usize| f64::from_bits(row[k]);
    let t = x - c;
    let t2 = t * t;
    let t4 = t2 * t2;
    let a = (p(9) * t + p(8)) * t + p(6);
    let b1 = ((p(13) * t + p(12)) * t + p(11)) * t + p(10);
    let b2 = (p(16) * t + p(15)) * t + p(14);
    let b = b1 + t4 * b2;
    let r = p(4) + (t * a + t4 * b);
    let ph = $prod(p(2), t);
    let s = fast_two_sum(p(0), ph.hi);
    let low = (s.lo + p(1)) + (ph.lo + (p(3) * t + p(5) * t2));
    fast_two_sum(s.hi, t2 * r + low)
}

/// `erfc x` on `(−6, 1)` as `1 ± erf |x|` from `erf |x|` as `(y, eps)`, rounded to nearest, or
/// `None` (the in-place entry's `1 − erf` branch).
$(#[$a])*
$(#[$i])*
fn erfc_low_near(x: f64, (y, eps): (Dd, f64)) -> Option<f64> {
    let (y, c) = if x < 0.0 { (y, C_EXP) } else { (y.neg(), C_ERFC_LOW) };
    let s = fast_two_sum(1.0, y.hi);
    let r = fast_two_sum(s.hi, s.lo + y.lo);
    finish(r, Some(c), 0, || eps + E103, &mut 0)
}

/// The table path for `2^-5 <= x < 6` when the quick phase's rounding test refuses: out of line
/// and cold, so the quick phase keeps no table value live across it (the SSE2 build spilled
/// them when both shared one body).
$(#[$a])*
#[cold]
#[inline(never)]
fn erf_table_near(x: f64) -> Option<f64> {
    let (y, eps) = erf_table_dd(x);
    finish(y, Some(C_EXP), 0, || eps, &mut 0)
}

/// As [`erf_table_near`] for `erfc` (`ax = |x|`, `2^-5 <= ax < 6`, `x < 1`).
$(#[$a])*
#[cold]
#[inline(never)]
fn erfc_table_near(x: f64, ax: f64) -> Option<f64> {
    erfc_low_near(x, erf_table_dd(ax))
}

/// The quick phase's `erf x` rounded to nearest for `2^-5 <= x < 6`, or `None` when its rounding
/// test refuses (module doc, "Quick phase").
$(#[$a])*
$(#[$i])*
fn erf_quick_near(x: f64) -> Option<f64> {
    fast64::near(erf_quick_dd(x), C_ERF_Q, 0)
}

/// The quick phase's `erfc x` rounded to nearest for `ax = |x|`, `2^-5 <= ax < 6`, `x < 1`, or
/// `None` when its rounding test refuses.
$(#[$a])*
$(#[$i])*
fn erfc_quick_near(x: f64, ax: f64) -> Option<f64> {
    let y = erf_quick_dd(ax);
    let (y, c) = if x < 0.0 { (y, C_ERF_Q) } else { (y.neg(), C_ERFC_Q_LOW) };
    let s = fast_two_sum(1.0, y.hi);
    fast64::near(fast_two_sum(s.hi, s.lo + y.lo), c, 0)
}

/// `erf x` rounded to nearest by the quick phase alone, or `None` (outside `2^-5 <= |x| < 6`, or
/// the rounding test refuses); [`erf_nearest`] runs the same phase first.
///
/// # Safety
///
/// As [`erf_nearest`].
$(#[$a])*
$(#[$i])*
pub fn erf_quick(x: f64) -> Option<f64> {
    let ax = dd::abs(x);
    if !(SMALL..ERF_ONE).contains(&ax) {
        return None;
    }
    let v = erf_quick_near(ax)?;
    Some(with_sign_of(v, x))
}

/// `erfc x` rounded to nearest by the quick phase alone (`−6 < x < 1`, `|x| >= 2^-5`), or `None`;
/// [`erfc_nearest`] runs the same phase first.
///
/// # Safety
///
/// As [`erf_nearest`].
$(#[$a])*
$(#[$i])*
pub fn erfc_quick(x: f64) -> Option<f64> {
    let ax = dd::abs(x);
    if !(SMALL..ERF_ONE).contains(&ax) || x >= 1.0 {
        return None;
    }
    erfc_quick_near(x, ax)
}
    };
}

/// The baseline build's tiny-band call: [`erf_linear_int`] kept out of line, so the in-place
/// entry's typical path keeps its register assignment (LIBM-ERF-TINY-F64-2: inlined, the band
/// cost the baseline build's typical erf one extra register copy per call).
#[cold]
#[inline(never)]
fn erf_linear_cold(x: f64, ax: f64) -> f64 {
    erf_linear_int(x, ax)
}

kernels!(; #[inline(always)]; two_prod; fast64::exp_dd; erf_linear_cold);

/// The FMA build of the kernels and in-place entries (module doc, "Builds"; MATH-F64-FMA-PATH,
/// D-10.25): the same arithmetic with every exact product a single FMA, so the same results bit
/// for bit. Callable only on a CPU with FMA and the OS-enabled AVX state; the boundary selects it
/// once at startup (`dispatch::fma`).
// ARCH(x86_64): the run-time FMA build (D-10.25); FMA is base ISA on aarch64, so one build there
#[cfg(target_arch = "x86_64")]
pub mod fused {
    use super::*;

    /// `c·2^64` as a double-double (tools/proofs/erf_tiny_f64.py, case F).
    const CH64: f64 = f64::from_bits(0x43f2_0dd7_5042_9b6d);
    const CL64: f64 = f64::from_bits(0x4071_ae3a_914f_ed80);
    /// 2^-64.
    const E64: f64 = f64::from_bits(0x3bf0_0000_0000_0000);

    /// [`super::erf_linear_int`]'s result for normal `x` by one FMA (case F of the proof tool):
    /// `fma(x, CH64, x·CL64)·2^-64`, every intermediate normal and the last product exact; the
    /// tool checks the operands where its error could reach a breakpoint. Inexact alone (the
    /// FMA). Subnormal `x` take the integer path.
    #[target_feature(enable = "fma")]
    #[inline]
    fn erf_linear_fma(x: f64, ax: f64) -> f64 {
        // ARCH(x86_64): the scalar FMA intrinsic (vfmadd); aarch64 would use fmadd from the base ISA
        use core::arch::x86_64::{_mm_cvtsd_f64, _mm_fmadd_sd, _mm_set_sd};
        if ax < f64::MIN_POSITIVE {
            return super::erf_linear_int(x, ax);
        }
        let t = x * CL64;
        _mm_cvtsd_f64(_mm_fmadd_sd(_mm_set_sd(x), _mm_set_sd(CH64), _mm_set_sd(t))) * E64
    }

    kernels!(#[target_feature(enable = "fma")]; #[inline]; dd::two_prod_fma; fast64::fused::exp_dd; erf_linear_fma);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` operands spread over `[lo, hi)` (a fixed Weyl sequence).
    fn operands(n: u32, lo: f64, hi: f64) -> impl Iterator<Item = f64> {
        (0..n).map(move |k| lo + (hi - lo) * ((f64::from(k) * 0.618_033_988_749_894_9) % 1.0))
    }

    // Clauses: s10_3_c_0 (D-10.13): the in-place entries' constant test only answers within the
    // bound its multiplier was derived for.
    #[test]
    fn in_place_bounds_are_within_their_multipliers() {
        // C_ERFC_LOW is the least double with C·(1 − 2^54·rho) > 1 + 2^-53 (exact in Mp).
        let one_u = Mp::from_f64(1.0).add(Mp::from_f64(f64::EPSILON / 2.0));
        let k = Mp::from_f64(1.0).sub(Mp::from_f64(RHO_ERFC_LOW * dd::pow2(54)));
        assert!(Mp::from_f64(C_ERFC_LOW).mul(k).cmp_abs(one_u).is_gt(), "C_ERFC_LOW below its bound");
        let below = f64::from_bits(C_ERFC_LOW.to_bits() - 1);
        assert!(Mp::from_f64(below).mul(k).cmp_abs(one_u).is_le(), "C_ERFC_LOW not the least");
        // erf on [2^-55, 6) and erfc on (−6, 0) within RHO_EXP; erfc on (0, 1) within RHO_ERFC_LOW.
        let tiny = (0..4000u32).map(|k| dd::pow2(i32::try_from(k % 50).unwrap_or(0) - 55) * (1.0 + f64::from(k) / 4000.0));
        for x in operands(40_000, LINEAR, ERF_ONE).chain(tiny) {
            let (y, eps) = if x >= SMALL { erf_table_dd(x) } else { erf_small_dd(x) };
            assert!(eps <= fast64::RHO_EXP * dd::abs(y.hi), "erf({x:e})");
            for (sign, rho) in [(1.0, fast64::RHO_EXP), (-1.0, RHO_ERFC_LOW)] {
                if sign < 0.0 && x >= 1.0 {
                    continue;
                }
                let s = fast_two_sum(1.0, sign * y.hi);
                let r = fast_two_sum(s.hi, s.lo + sign * y.lo);
                assert!(eps + E103 <= rho * dd::abs(r.hi), "erfc({:e})", -sign * x);
            }
        }
        // erfc on [1, 27.25) within RHO_EXP.
        for x in operands(40_000, 1.0, ERFC_MAX) {
            if let Some((y, eps, _)) = erfc_kernel(x) {
                assert!(eps <= fast64::RHO_EXP * dd::abs(y.hi), "erfc({x:e})");
            }
        }
    }

    // Clauses: s10_3_c_0 (D-10.84): the quick phase's multipliers are the least for their stated
    // bounds, and every result it gives is the correctly rounded one (the accurate path's).
    #[test]
    fn quick_phase_rounds_correctly() {
        let one_u = Mp::from_f64(1.0).add(Mp::from_f64(f64::EPSILON / 2.0));
        for (rho, c) in [(RHO_ERF_Q, C_ERF_Q), (RHO_ERFC_Q_LOW, C_ERFC_Q_LOW)] {
            let k = Mp::from_f64(1.0).sub(Mp::from_f64(rho * dd::pow2(54)));
            assert!(Mp::from_f64(c).mul(k).cmp_abs(one_u).is_gt(), "quick C below its bound");
            let below = f64::from_bits(c.to_bits() - 1);
            assert!(Mp::from_f64(below).mul(k).cmp_abs(one_u).is_le(), "quick C not the least");
        }
        // The tier's hard cases: operands where the quick `(zh, zl)` lies on a midpoint (or within
        // 2^-12 ulp of one) and the exact `erf x` on its other side, so only the rounding test keeps
        // the phase from misrounding them (from a search of 2^28 operands of [2^-5, 6) against the
        // accurate path: 3867 such operands, about 1 in 69000; LIBM-F64-SPECIAL-QUICK-2).
        const HARD: [u64; 10] = [
            0x3ff0_2c47_393a_484c, // 2^-12 ulp from the midpoint
            0x3ff4_f02c_80f0_96af,
            0x3fe1_f91d_dd34_d5fa,
            0x3ffe_0b69_ef59_cc12,
            0x4010_1d2a_dfa7_5c87, // x ≈ 4.03, erf x just below 1
            0x3ff3_2328_b759_f1d1, // (zh, zl) exactly on the midpoint
            0x3fdd_ef19_dd2b_ee47,
            0x3fc5_f184_b3f6_c9a4,
            0x3fb7_0371_50f2_948d,
            0x4000_0264_5585_4952,
        ];
        let (mut miss, mut n) = (0u32, 0u32);
        for x in operands(20_000, SMALL, ERF_ONE).chain(HARD.iter().map(|&b| f64::from_bits(b))) {
            for x in [x, -x] {
                let want = crate::special::erf(x, Rounding::Nearest).0;
                match erf_quick(x) {
                    Some(v) => assert_eq!(v.to_bits(), want.to_bits(), "erf quick({x:e}) misrounded"),
                    None => miss += 1,
                }
                n += 1;
                if x < 1.0 {
                    let want = crate::special::erfc(x, Rounding::Nearest).0;
                    match erfc_quick(x) {
                        Some(v) => assert_eq!(v.to_bits(), want.to_bits(), "erfc quick({x:e}) misrounded"),
                        None => miss += 1,
                    }
                    n += 1;
                }
            }
        }
        eprintln!("erf/erfc quick: {miss} misses of {n}");
        assert!(miss * 100 <= n * 3, "quick fallback {miss} of {n} above 3%");
    }
}
