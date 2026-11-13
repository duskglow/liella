//! The quick tier of `cexp` and `cexpf` (spec §10.5; LIBM-CPLX-EXP-TIER, D-10.53): both parts
//! from the real functions' fast kernels with their proved error bounds, one rounding test per
//! part. When a test cannot decide, or an operand or part leaves the tier's range, the caller runs
//! the 384-bit route ([`crate::complex::cexp_route`]) unchanged; every special and degenerate case
//! (zeros, infinities, NaNs, the axes) goes there too, before this tier is consulted or because
//! the range checks below exclude it.
//!
//! **`double`.** `cexp(x + iy) = e^x·cos y + i·e^x·sin y` with
//!
//! * `e^x = E·2^m`, `E` a double-double from [`fast64::exp_kernel`] with `|E − e^x·2^-m| <= Ee`,
//!   `Ee = `[`fast64::exp_eps`]`(E, tail) = |E.hi|·(2^-49·|tail| + 2^-70)` (proof there), for
//!   `2^-60 <= |x| <= 746` (the reduction's range, [`fast64::reduce_exp`]);
//! * `sin y`, `cos y` as double-doubles `S`, `C` from the trig kernels ([`kernel::reduce`],
//!   [`kernel::poly`], [`kernel::assemble`]; one reduction and one polynomial for both) with the
//!   absolute bounds [`trig::eps`] of each assembly (`2^-68`, or `2^-89 + 2^-63·|t|` when the
//!   table sine is 0: the bound stays relative near a zero of the part, `cos y` for `y` near
//!   `π/2 + kπ`), for `2^-30 <= |y| < 2^20` (beyond, the Payne–Hanek reduction: the 384-bit route);
//! * each part `P = E ⊗ T` ([`Dd::mul`]: `two_prod` of the heads, the cross products added, the
//!   product of the tails dropped; normalized inputs, so within `2^-102.6·|E.hi·T.hi|`).
//!
//! **Bound of a part.** With `A = |E.hi|`, `B = |T.hi|`, `Et` the trig bound: `|E| <= A·(1 + 2^-52)`,
//! `|e^x·2^-m·t − E·T| <= Ee·|t| + |E|·Et` (`t` the exact sine or cosine, `|t| <= B·(1 + 2^-52) +
//! Et`), and `Ee <= 2^-69·A` (`|tail| < 2^-24.5`), so the second-order terms are below
//! `2^-49·A·(Ee/A·B + Et)`. With the product's `2^-102.6·A·B`:
//! `eps = (Ee·B + A·Et + 2^-99·A·B)·(1 + 2^-48)`; the factor also covers the `double` evaluation of
//! `eps` itself (four operations, `4u`). The relative bound is `ε_exp + ε_trig + ε_product`, as
//! D-10.53 states, without a division.
//!
//! **Rounding.** A part is rounded unscaled, then scaled by `2^m` with an integer add to its
//! exponent, only when the scaled value is normal and at least `2^-1021` (biased exponent
//! `2..=2046`): rounding to 53 bits commutes with an exact scaling inside the normal range, and
//! the lowest normal binade is excluded because below `2^-1022` the grid stops shrinking (a value
//! just under a power of two would round on a finer grid unscaled than scaled). So no part of
//! this tier overflows or underflows: near either threshold it falls back. The test per part, to
//! nearest: [`fast64::near`] (de Dinechin, Lauter and Muller 2007) with `rho = 2^-62` and
//! `C_PART = 1 + 2^-8 + 2^-14` (`C_PART·(1 − 2^54·rho) = 1 + 2^-14 − 2^-16 − 2^-22 > 1 + 2^-53`)
//! when `eps <= rho·|P.hi|`; else, and in the directed modes, the interval test
//! [`fast64::settle`] with `eps`. Neither part is a `double` nor a midpoint: for `y != 0` and
//! algebraic `x`, `y`, `e^(x+iy) ± e^(x−iy) = 2q` (or `2iq`) with `q` rational would be a
//! vanishing combination of exponentials of the distinct algebraic numbers `x + iy`, `x − iy`, `0`
//! with algebraic coefficients, which the Lindemann–Weierstrass theorem excludes.
//!
//! **`float`.** [`fast32::cexpf`]: `expf`'s and `sinf`/`cosf`'s kernels in `double` (bound
//! `2^-45.16` and `24.3·2^-53`), each part one `double` product within 255 ulp, rounded to
//! `float` by the online test of the other `float` kernels (normal results only).
//!
//! **In place.** [`cexp_nearest`] and [`fast32::cexpf_nearest`] serve the boundary's in-place
//! entry in the default environment: hardware arithmetic on normal values; each part's test
//! raises inexact itself (`near`'s addition of a nonzero `w` below half an ulp, `settle`'s
//! additions of `P.lo ± eps`, the `float` conversion of a part that is not a `float`), and no
//! other flag can arise. The directed modes run through the envelope and [`cexp`] here.

#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)] // table indices mod 128, exponent fields and format conversions of checked range

use crate::coeffs::tables as t;
use crate::dd::{self, Dd};
use crate::fast32;
use crate::fast64;
use crate::float::Float;
use crate::status::{Flags, Rounding, Status};
use crate::trig::{self, kernel};

/// `2^-60` and `746`: the range of `|x|` ([`fast64::reduce_exp`]'s proof, the kernel's exact
/// products).
const X_MIN: f64 = f64::from_bits(0x3c30_0000_0000_0000);
const X_MAX: f64 = 746.0;
/// `2^-30` and `2^20`: the range of `|y|` (the trig kernels' Cody–Waite reduction, `|r| >= 2^-30`
/// when `n = 0`).
const Y_MIN: f64 = f64::from_bits(0x3e10_0000_0000_0000);
const Y_MAX: f64 = 1_048_576.0;
/// `2^-99`: the product's relative bound with room to spare (`2^-102.6` proved).
const PROD: f64 = f64::from_bits(0x39c0_0000_0000_0000);
/// `1 + 2^-48`: the second-order terms and the rounding of the bound's own evaluation.
const WIDEN: f64 = f64::from_bits(0x3ff0_0000_0000_0010);
/// `rho = 2^-62` and its multiplier for [`fast64::near`].
const RHO: f64 = f64::from_bits(0x3c10_0000_0000_0000);
const C_PART: f64 = f64::from_bits(0x3ff0_1040_0000_0000);

// The parts and the in-place entry in two builds (D-10.25, as `fast64` and `trig`): `$prod` is
// Dekker's `two_prod` or one FMA, `$exp` and `$trig` the kernels' matching builds. Every product
// is exact in both (the operands are normal and far from the subnormal range: `|E.hi| >= 2^-1`,
// the trig values `>= 2^-31`), so both builds compute the same values and take the same
// branches: the bounds above hold for either, and the results agree bit for bit.
// `#[inline(always)]` cannot be combined with `#[target_feature]`, hence `$i`.
macro_rules! cexp_kernels {
    ($(#[$a:meta])*; $(#[$i:meta])*; $prod:path; $exp:path; $trig:path) => {
/// The two parts unscaled with their bounds, `[(e^x·cos y, eps), (e^x·sin y, eps)]`, and the
/// scale `m`, or `None` outside the tier's range.
$(#[$a])*
$(#[$i])*
fn parts(x: f64, y: f64) -> Option<([(Dd, f64); 2], i32)> {
    use $exp as ek;
    use $trig as tk;
    if !(X_MIN..=X_MAX).contains(&dd::abs(x)) || !(Y_MIN..Y_MAX).contains(&dd::abs(y)) {
        return None;
    }
    let kf = dd::nearest_int(x * f64::from_bits(t::SCALES[0]));
    let (e, m, tail) = ek::exp_kernel(fast64::reduce_exp(x, kf), dd::to_i32(kf))?;
    let ee = fast64::exp_eps(e, tail);
    let (n, r) = tk::reduce(y);
    let p = tk::poly(r);
    let (s, ss) = tk::assemble(n, p);
    let (c, cs) = tk::assemble(n + 32, p);
    let a = dd::abs(e.hi);
    // `Dd::mul` with `$prod`: the product of the heads exact, the cross products added.
    let part = |v: Dd, et: f64| {
        let b = dd::abs(v.hi);
        let q = $prod(e.hi, v.hi);
        (dd::fast_two_sum(q.hi, q.lo + (e.hi * v.lo + e.lo * v.hi)), (ee * b + a * et + PROD * a * b) * WIDEN)
    };
    Some(([part(c, trig::eps(cs, c)), part(s, trig::eps(ss, s))], m))
}

/// `cexp(x + iy)` to nearest for the boundary's in-place entry (see the module comment).
///
/// # Safety
///
/// In the FMA build the CPU must support FMA and the OS must save the AVX state (the
/// `#[target_feature]` contract); the baseline build is safe.
///
/// A part the quick test declines makes this decline too: the middle tier is reached from the
/// boundary's cold fallback ([`cexp`] through the envelope), never from here, so no operand has
/// to stay live across the kernel for it (LIBM-CEXP-COLD-MIDDLE, D-10.99).
$(#[$a])*
$(#[$i])*
pub fn cexp_nearest(x: f64, y: f64) -> Option<(f64, f64)> {
    let Some(([(re, er), (im, ei)], m)) = parts(x, y) else {
        return axis(x, y);
    };
    Some((round(re, er, m, Rounding::Nearest)?, round(im, ei, m, Rounding::Nearest)?))
}

/// `cexp(x ± 0i) = (e^x, ±0)` for the in-place entry (LIBM-CPLX-LOG-SQRT-4, D-10.74; C Annex
/// G.6.3.1: the imaginary part is `e^x·sin(±0)`): `e^x` from the real function's in-place kernel
/// (correctly rounded, normal and finite for `2^-60 <= |x| < 708`, inexact raised in hardware),
/// and `cexp(±0 ± 0i) = (1, ±0)` exact. Out of line: only the tier's declines reach it.
$(#[$a])*
#[inline(never)]
fn axis(x: f64, y: f64) -> Option<(f64, f64)> {
    use $exp as ek;
    if y != 0.0 || !(0.0..708.0).contains(&dd::abs(x)) {
        return None;
    }
    if x == 0.0 {
        return Some((1.0, y));
    }
    Some((ek::exp_nearest(x)?, y))
}
    };
}

cexp_kernels!(; #[inline(always)]; dd::two_prod; fast64; kernel);

/// [`cexp_nearest`], FMA build: the same results bit for bit. Callable only on a CPU with FMA and
/// the OS-enabled AVX state; the boundary selects it once at startup (`dispatch::fma`).
// ARCH(x86_64): the run-time FMA build (D-10.25); FMA is base ISA on aarch64, so one build there
#[cfg(target_arch = "x86_64")]
pub mod fused {
    use super::{fast64, round, t, trig, Dd, Rounding, PROD, WIDEN, X_MAX, X_MIN, Y_MAX, Y_MIN};
    use crate::dd;

    cexp_kernels!(#[target_feature(enable = "fma")]; #[inline]; dd::two_prod_fma; fast64::fused; trig::kernel_fma);
}

/// `v·2^m` when it is a normal `double` of biased exponent `2..=2046` (see the module comment).
#[inline(always)]
fn scale(v: f64, m: i32) -> Option<f64> {
    let b = v.to_bits();
    let eb = ((b >> 52) & 0x7ff) as i32 + m;
    if !(2..=0x7fe).contains(&eb) {
        return None;
    }
    Some(f64::from_bits(b.wrapping_add((i64::from(m) as u64) << 52)))
}

/// One part rounded in `mode` and scaled, or `None`.
#[inline(always)]
fn round(p: Dd, eps: f64, m: i32, mode: Rounding) -> Option<f64> {
    if mode == Rounding::Nearest && eps <= RHO * dd::abs(p.hi) {
        if let Some(v) = fast64::near(p, C_PART, 0) {
            return scale(v, m);
        }
    }
    scale(fast64::settle(p, eps, 0, mode)?.0, m)
}

/// `cexp(x + iy)` in `mode` by the quick tier, both parts or `None`.
#[inline(always)]
pub fn cexp(x: f64, y: f64, mode: Rounding) -> Option<((f64, f64), Status)> {
    let ([(re, er), (im, ei)], m) = parts(x, y)?;
    let a = match round(re, er, m, mode) {
        Some(v) => v,
        None => middle(x, y, Part::Re, mode)?,
    };
    let b = match round(im, ei, m, mode) {
        Some(v) => v,
        None => middle(x, y, Part::Im, mode)?,
    };
    Some(((a, b), Status::flags(Flags::INEXACT)))
}

// ---------------------------------------------------------------------------------------------
// The middle tier of `cexp` (LIBM-CEXP-ROUTE, D-10.99). A part whose quick rounding test cannot
// decide (its exact value within about `2^-66` of a rounding boundary: about one part in a
// thousand) is evaluated again near `2^-99` before the 384-bit route is consulted; only a part
// within that of a boundary still reaches the route. Only [`cexp`] (the envelope's tier) calls
// this out-of-line code: both builds of the in-place [`cexp_nearest`] decline such a part, and
// the boundary's cold fallback then runs [`cexp`] (LIBM-CEXP-COLD-MIDDLE: the hot kernel keeps no
// operand live for the call), so the in-place results stay bit-identical between the builds.
//
// * `e^x = V·2^s` from [`wide_dd::exp_dd`] within `2^-100·|V|` (its module comment: below
//   `2^-103` proved), for `|x| < 2^14`.
// * `sin y` or `cos y` as a double-double `T` within `TRIG_ABS = 2^-100` absolute
//   ([`sin_or_cos`], `|y| < 2^20`). Reduction: `n = RN(RN(y·2/π))`, `|n| < 2^20`, so `|r| <=
//   π/4 + 2^-31` for `r = y − n·π/2`. With `π/2 = P1 + P2 + P3 + δ` (`|δ| < 2^-163`):
//   `n·P1 = h` and `n·P2 = g` exactly ([`dd::two_prod`]), `y − h.hi` exactly ([`dd::two_sum`]),
//   then three double-double additions (`−h.lo`, `−g`, `−RN(n·P3)`), each within
//   `3u²/(1 − 4u) < 2^-104.4` of its result for any signs (Joldes, Muller and Popescu, "Tight and
//   rigorous error bounds for basic building blocks of double-word arithmetic", ACM TOMS 44(2),
//   2017, Algorithms 4 and 6), every partial sum below `0.79`; `RN(n·P3)` within `2^-142`,
//   `n·δ` below `2^-143`: `r̃` within `2^-103.6` of `r`, so the result moves by at most that
//   (sine and cosine are 1-Lipschitz). Then `z = r̃·r̃` (`2^-102.6`, as the product above) and
//   Taylor series in `z` by [`dd::horner`]: `cos r̃ = Σ (−1)^k z^k/(2k)!` and `sin r̃ = r̃·Σ (−1)^k
//   z^k/(2k+1)!`, `k <= 13` (truncation below `2^-107` and `2^-102` relative to the sum,
//   `z <= 0.617`), the coefficients double-doubles within `2^-106` (exact rationals rounded twice,
//   Python `fractions`, pinned by the tests against the 384-bit arithmetic), the terms below
//   `2^-54` (from `k = 8` for the sine, `k = 9` for the cosine) in `double` (`12u` of each).
//   Each Horner step's product (`2^-102.5`) and sum (`2^-104.3`) errors are scaled by `z^k`; the
//   partial sums stay within `1.1` times their leading coefficient (each next term at most `0.31`
//   of the one before), so the sums are within `2^-102.4` absolute (`2^-102.2` with `z`'s own
//   error, `|d cos √z/dz| <= 1/2`), the sine's final product with `r̃` within `2^-101.8`: with
//   the reduction, below `2^-101.4 < TRIG_ABS`.
// * `P = V ⊗ T` ([`Dd::mul`], `2^-102.6·A·B`, `A = |V.hi|`, `B = |T.hi|`): `|e^x·2^-s·t − P| <=
//   2^-100·|V|·|t| + |V|·2^-100 + 2^-102.6·A·B < (A·B + A)·2^-99 = eps` with room for `eps`'s own
//   evaluation (`|t| <= B·(1 + 2^-52) + 2^-100`). [`fast64::settle`] with `eps` then decides (or
//   declines) in every mode, as the quick tier's interval test; scaling by `2^s` as [`scale`].
//
// No intermediate value comes near the subnormal range (`|r̃| >= 2^-30` for `n = 0`; for
// `n != 0` no `double` lies within `2^-62` of a multiple of `π/2`, J.-M. Muller, "Elementary
// Functions", 3rd ed., 2016, §11.3, and the Horner products stay normal for any `|r̃| > 2^-200`;
// `|x| >= 2^-60` keeps `e^x`'s polynomial normal), so in
// the in-place entry the only flag raised is `settle`'s inexact, as in the quick tier.

/// Which part of `cexp`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Part {
    /// `e^x·cos y`.
    Re,
    /// `e^x·sin y`.
    Im,
}

/// `π/2 = P1 + P2 + P3` within `2^-163` (each the nearest `double` to the rest).
const HALF_PI: [f64; 3] = [
    f64::from_bits(0x3ff9_21fb_5444_2d18),
    f64::from_bits(0x3c91_a626_3314_5c07),
    f64::from_bits(0xb91f_1976_b7ed_8fbc),
];
/// `RN(2/π)`.
const TWO_OVER_PI: f64 = f64::from_bits(0x3fe4_5f30_6dc9_c883);
/// `2^-100`: the absolute bound of [`sin_or_cos`] (folded into [`MID_EPS`]; the tests check it).
#[cfg(test)]
const TRIG_ABS: f64 = f64::from_bits(0x39b0_0000_0000_0000);
/// `2^-99`: the part's bound per unit of `A·B + A`.
const MID_EPS: f64 = f64::from_bits(0x39c0_0000_0000_0000);
/// `(−1)^k/(2k+1)!`, `k = 0..=13`, as double-doubles.
const SIN_DD: [[u64; 2]; 14] = [
    [0x3ff0000000000000, 0x0000000000000000],
    [0xbfc5555555555555, 0xbc65555555555555],
    [0x3f81111111111111, 0x3c01111111111111],
    [0xbf2a01a01a01a01a, 0xbb6a01a01a01a01a],
    [0x3ec71de3a556c734, 0xbb6c154f8ddc6c00],
    [0xbe5ae64567f544e4, 0x3afc062e06d1f209],
    [0x3de6124613a86d09, 0x3a8f28e0cc748ebe],
    [0xbd6ae7f3e733b81f, 0xb9e1d8656b0ee8cb],
    [0x3ce952c77030ad4a, 0x398ac981465ddc6c],
    [0xbc62f49b46814157, 0xb8f2650f61dbdcb4],
    [0x3bd71b8ef6dcf572, 0xb87d043ae40c4647],
    [0xbb4761b41316381a, 0x37d3423c7d91404f],
    [0x3ab3f3ccdd165fa9, 0xb7458ddadf344487],
    [0xba1d1ab1c2dccea3, 0xb6a054d0c78aea14],
];
/// `(−1)^k/(2k)!`, `k = 0..=13`, as double-doubles.
const COS_DD: [[u64; 2]; 14] = [
    [0x3ff0000000000000, 0x0000000000000000],
    [0xbfe0000000000000, 0x0000000000000000],
    [0x3fa5555555555555, 0x3c45555555555555],
    [0xbf56c16c16c16c17, 0x3bef49f49f49f49f],
    [0x3efa01a01a01a01a, 0x3b3a01a01a01a01a],
    [0xbe927e4fb7789f5c, 0xbb3cbbc05b4fa99a],
    [0x3e21eed8eff8d898, 0xbac2aec959e14c06],
    [0xbda93974a8c07c9d, 0xba305d6f8a2efd1f],
    [0x3d2ae7f3e733b81f, 0x39a1d8656b0ee8cb],
    [0xbca6827863b97d97, 0xb94eec01221a8b0b],
    [0x3c1e542ba4020225, 0x387ea72b4afe3c2f],
    [0xbb90ce396db7f853, 0x383aebcdbd20331c],
    [0x3aff2cf01972f578, 0xb789ada5fcc1ab14],
    [0xba688e85fc6a4e5a, 0x37071c37ebd16540],
];

/// `cos y` or `sin y` (by `part`) as a double-double within [`TRIG_ABS`], `2^-30 <= |y| < 2^20`.
fn sin_or_cos(y: f64, part: Part) -> Dd {
    let n = dd::nearest_int(y * TWO_OVER_PI);
    let h = dd::two_prod(n, HALF_PI[0]);
    let g = dd::two_prod(n, HALF_PI[1]);
    let r = dd::two_sum(y, -h.hi).add(Dd::from(-h.lo)).add(g.neg()).add_f64(-(n * HALF_PI[2]));
    let z = r.mul(r);
    // The quadrant of `y` (`n mod 4`), plus one for the cosine: `cos y = sin(y + π/2)`.
    let q = (dd::to_i32(n) + i32::from(part == Part::Re)) & 3;
    let v = if q & 1 == 0 { r.mul(dd::horner(z, &SIN_DD, 8)) } else { dd::horner(z, &COS_DD, 9) };
    if q & 2 == 0 { v } else { v.neg() }
}

/// The middle tier's part, its bound and its scale: `e^x·cos y` or `e^x·sin y ≈ P·2^s`,
/// `|e^x·t·2^-s − P| <= eps` (see above); `None` outside the exponential's range.
fn middle_part(x: f64, y: f64, part: Part) -> Option<(Dd, f64, i32)> {
    let (v, s) = crate::wide_dd::exp_dd(&crate::float::decode(x))?;
    let t = sin_or_cos(y, part);
    let a = dd::abs(v.hi);
    Some((v.mul(t), (a * dd::abs(t.hi) + a) * MID_EPS, s))
}

/// `P·2^s` rounded in `mode` when every value within `eps` of `P` rounds alike, else `None`.
#[inline(always)]
fn middle_round(p: Dd, eps: f64, s: i32, mode: Rounding) -> Option<f64> {
    scale(fast64::settle(p, eps, 0, mode)?.0, s)
}

/// One part of `cexp(x + iy)` by the middle tier, for an operand inside the quick tier's range
/// whose quick rounding test declined; `None` sends the operand to the 384-bit route.
#[cold]
#[inline(never)]
fn middle(x: f64, y: f64, part: Part, mode: Rounding) -> Option<f64> {
    let (p, eps, s) = middle_part(x, y, part)?;
    middle_round(p, eps, s, mode)
}

/// A quick tier for any format: `d` for `double` operands, `f` for `float` ones, `None` for the
/// others.
#[inline(always)]
fn tier_any<F: Float>(
    x: F,
    y: F,
    d: impl FnOnce(f64, f64) -> Option<((f64, f64), Status)>,
    f: impl FnOnce(f32, f32) -> Option<((f32, f32), Status)>,
) -> Option<((F, F), Status)> {
    match F::PREC {
        53 => {
            let ((a, b), st) = d(f64::from_bits(x.to_bits() as u64), f64::from_bits(y.to_bits() as u64))?;
            Some(((F::from_bits(u128::from(a.to_bits())), F::from_bits(u128::from(b.to_bits()))), st))
        }
        24 => {
            let ((a, b), st) = f(f32::from_bits(x.to_bits() as u32), f32::from_bits(y.to_bits() as u32))?;
            Some(((F::from_bits(u128::from(a.to_bits())), F::from_bits(u128::from(b.to_bits()))), st))
        }
        _ => None,
    }
}

/// The `cexp` quick tier for any format.
#[inline(always)]
pub(crate) fn cexp_any<F: Float>(x: F, y: F, mode: Rounding) -> Option<((F, F), Status)> {
    tier_any(x, y, |a, b| cexp(a, b, mode), |a, b| fast32::cexpf(a, b, mode))
}

// ---------------------------------------------------------------------------------------------
// `clog` and `csqrt` (LIBM-CPLX-LOG-SQRT, D-10.53, D-10.74): the same shape as `cexp` above, each
// part a double-double with a proved bound and one rounding test ([`round`]); the 384-bit route
// for everything else. Both take `2^-400 <= |x|, |y| < 2^400` (so every square below is an exact
// `two_prod` with a normal error term, every part is a normal `double`, and no scaling is
// needed); zeros, infinities, NaNs, subnormals and wider ranges go to the route.
//
// **`csqrt`** (Kahan's formulation, W. Kahan, "Branch cuts for complex elementary functions",
// 1987): with `a = |x|`, `b = |y|`, `h = |z| = sqrt(a² + b²)`, `t = sqrt((a + h)/2)` and
// `o = b/(2t)`, `csqrt z = t + i·sign(y)·o` for `x >= 0` and `o + i·sign(y)·t` for `x < 0`: no
// cancellation anywhere (every sum adds positive terms). Errors, relative: `a² + b²` from two
// exact products and one 2Sum, `2^-104.4`; [`sqrt_dd`] adds `2^-104` and halves the operand's
// error, so `h` within `2^-103.4`; `a + h` (2Sum, then one addition of the tails, `2^-104`):
// `2^-102.6`; halving is exact; `t` within `2^-102.6/2 + 2^-104 < 2^-102.8`; `2t` exact, `o`
// by [`div_b`] (`2^-103`) within `2^-101.9`. So `SQRT_EPS = 2^-100` relative to the part's
// head covers both parts. A part may be a `double` (`csqrt(3 + 4i) = 2 + i`), but never a
// midpoint: `y = 2·re·im` with `re` (or `im`) of 54 significant bits would need at least 54
// bits. An exact part has `|P.lo| <= eps` (its head is that `double`), so the tier declines
// those (the route decides their inexact flag); [`round`] declines any interval that holds a
// midpoint or, in a directed mode, a `double`.
//
// **`clog`**: `arg z` from [`invtrig::atan2_abs`] (within `2^-65.5` relative, `min/max >= 2^-60`),
// `ARG_EPS = 2^-65` of its head. `log|z| = ln(s)/2`, `s = a² + b²` (`a >= b`):
//
// * `s` far from 1 (`RN(a² + b²)` outside `[0.75, 1.25)`): `s = sh + sl` within `2^-104.4`,
//   `ln sh = l` from [`fast64::log_kernel`] within [`fast64::log_eps`], then
//   `ln s = l + sl/sh`: the dropped `(sl/sh)²/2 <= 2^-107`, the quotient's rounding `2^-106`,
//   the error of `s` `2^-104` absolute, the addition of the tails `2^-106·(1 + |l|)`: in all
//   below `LOG_EXTRA·(1 + |l.hi|)`, `LOG_EXTRA = 2^-102` (`|l| >= 0.22`, so the Fast2Sum holds).
// * `s` near 1: `w = s − 1 = d·(a + 1) + b²` with `d = a − 1` exact (Sterbenz: `a` is in
//   `[0.61, 1.12]`), `a + 1 = e.hi + e.lo` exact (2Sum), `d·e.hi` and `b²` exact products, so
//   the cancellation between them costs nothing; the tails' sum (three additions and the
//   rounded `d·e.lo`) is within `2^-102.6·T`, `T = |d·e.hi| + b²`, and `w` is that sum's 2Sum.
//   `log1p(w)` moves by at most `2^-102.6·T/(1 + w) < W_ERR·T`, `W_ERR = 2^-102` (`1 + w >=
//   0.749`). Then `log1p(w.hi) = zh + zl` from [`quick_log::log1p_quick_parts`] within
//   `2^-64·|zh|` and `log1p(w) = z + w.lo/(1 + w.hi)` (quotient and dropped square below
//   `2^-104.3·|zh|`, the addition of the tails `2^-104·|zh|`): with the kernel's, below
//   `L1P_REL·|zh|`, `L1P_REL = 2^-63`. A deep cancellation (`T >> |w|`) shows in the bound,
//   and the rounding test then sends the operand to the route.
//
// Neither part of `clog` is a `double` or a midpoint (for nonzero `x`, `y`: `ln q` of a rational
// `q != 1` and `atan2` of two nonzero rationals are transcendental, Lindemann–Weierstrass; and
// `a² + b² = 1` has no solution in nonzero dyadic rationals). The halving of `ln s` is exact.
//
// **`float`**: [`fast32::clogf`] and [`fast32::csqrtf`], `double` arithmetic with a margin in
// ulp of the `double` part and `float`'s online rounding test.

/// The window of `|x|` and `|y|` for `clog` and `csqrt` (bits of `2^-400` and `2^400`).
const W_MIN: u64 = 0x26f0_0000_0000_0000;
const W_MAX: u64 = 0x58f0_0000_0000_0000;
/// `csqrt`'s relative bound per part (`2^-100`, proved `2^-101.9`).
const SQRT_EPS: f64 = f64::from_bits(0x39b0_0000_0000_0000);
/// `clog`'s bounds (see above): `2^-65`, `2^-102`, `2^-102`, `2^-63`.
const ARG_EPS: f64 = f64::from_bits(0x3be0_0000_0000_0000);
const LOG_EXTRA: f64 = f64::from_bits(0x3990_0000_0000_0000);
const W_ERR: f64 = f64::from_bits(0x3990_0000_0000_0000);
const L1P_REL: f64 = f64::from_bits(0x3c00_0000_0000_0000);
/// `[0.75, 1.25)`: the values of `RN(a² + b²)` that take `log1p`.
const NEAR_LO: f64 = 0.75;
const NEAR_HI: f64 = 1.25;

/// `(|x|, |y|)` when both lie in `[2^-400, 2^400)`.
#[inline(always)]
fn window(x: f64, y: f64) -> Option<(f64, f64)> {
    let (ax, ay) = (x.to_bits() & !(1 << 63), y.to_bits() & !(1 << 63));
    if !(W_MIN..W_MAX).contains(&ax) || !(W_MIN..W_MAX).contains(&ay) {
        return None;
    }
    Some((f64::from_bits(ax), f64::from_bits(ay)))
}

/// `a² + b²` as a normalized double-double within `2^-104.4` relative (exact products, one 2Sum,
/// the tails added in two roundings).
#[inline(always)]
fn sum_squares(a: f64, b: f64) -> Dd {
    let (pa, pb) = (dd::two_prod(a, a), dd::two_prod(b, b));
    let s = dd::two_sum(pa.hi, pb.hi);
    dd::fast_two_sum(s.hi, s.lo + (pa.lo + pb.lo))
}

/// The root of a positive normalized double-double, `sqrt` correctly rounded to nearest: one
/// correction step `(x − s²)/(2s)` with `s²` exact, within `2^-104` relative plus half the
/// operand's relative error (as the `hypot` kernel, `crate::invtrig`'s module comment).
#[inline(always)]
fn sqrt_dd(x: Dd, sqrt: &impl Fn(f64) -> f64) -> Dd {
    let s = sqrt(x.hi);
    let ps = dd::two_prod(s, s);
    dd::fast_two_sum(s, (((x.hi - ps.hi) - ps.lo) + x.lo) / (2.0 * s))
}

/// `b/d` for a positive `double` `b` and a positive normalized double-double `d`, within `2^-103`
/// relative (one correction step on the exact residual, as `invtrig`'s `div_dd`).
#[inline(always)]
fn div_b(b: f64, d: Dd) -> Dd {
    let q1 = b / d.hi;
    let pq = dd::two_prod(q1, d.hi);
    dd::fast_two_sum(q1, (((b - pq.hi) - pq.lo) - q1 * d.lo) / d.hi)
}

/// `csqrt`'s parts `[re, im]` (see above), `sqrt` the correctly rounded `double` root.
#[inline(always)]
fn csqrt_parts(x: f64, y: f64, sqrt: &impl Fn(f64) -> f64) -> Option<[Dd; 2]> {
    let (a, b) = window(x, y)?;
    let h = sqrt_dd(sum_squares(a, b), sqrt);
    let s = dd::two_sum(a, h.hi);
    let m = dd::fast_two_sum(s.hi, s.lo + h.lo);
    let t = sqrt_dd(m.scale(0.5), sqrt);
    let o = div_b(b, t.scale(2.0));
    let (re, im) = if x >= 0.0 { (t, o) } else { (o, t) };
    Some([re, if y < 0.0 { im.neg() } else { im }])
}

/// One `csqrt` part rounded in `mode`: an exact part (`|P.lo| <= eps`) declines.
#[inline(always)]
fn round_root(p: Dd, mode: Rounding) -> Option<f64> {
    let eps = SQRT_EPS * dd::abs(p.hi);
    if dd::abs(p.lo) <= eps {
        return None;
    }
    round(p, eps, 0, mode)
}

/// `csqrt(x + iy)` to nearest for the boundary's in-place entry, `sqrt` the hardware root.
#[inline(always)]
pub fn csqrt_nearest(x: f64, y: f64, sqrt: impl Fn(f64) -> f64) -> Option<(f64, f64)> {
    let Some([re, im]) = csqrt_parts(x, y, &sqrt) else {
        return csqrt_axis(x, y, &sqrt);
    };
    Some((round_root(re, Rounding::Nearest)?, round_root(im, Rounding::Nearest)?))
}

/// `v` (positive) with the sign bit of `s`.
#[inline(always)]
fn with_sign(v: f64, s: f64) -> f64 {
    f64::from_bits(v.to_bits() | (s.to_bits() & (1 << 63)))
}

/// Whether `v` is a normal finite nonzero `double` (either sign).
#[inline(always)]
fn normal(v: f64) -> bool {
    (f64::MIN_POSITIVE..=f64::MAX).contains(&dd::abs(v))
}

/// `csqrt` on an axis for the in-place entry (LIBM-CPLX-LOG-SQRT-4, D-10.74): one part an exact
/// zero, the other normal. `csqrt(x ± 0i)` is `(√x, ±0)` for `x > 0` and `(+0, ±√|x|)` for
/// `x < 0`; `csqrt(±0 + iy)` is `(t, t)` with the sign of `y` on the second, `t = √(|y|/2)`
/// (`|y|/2` exact for `|y| >= 2^-1021`) (C Annex G.6.4.2 and conjugate symmetry). Each part is
/// one correctly rounded hardware root, which raises inexact exactly when it is inexact and
/// nothing else on a normal operand. Out of line: only the tier's declines reach it.
#[inline(never)]
fn csqrt_axis(x: f64, y: f64, sqrt: &impl Fn(f64) -> f64) -> Option<(f64, f64)> {
    if y == 0.0 && normal(x) {
        let r = sqrt(dd::abs(x));
        return Some(if x > 0.0 { (r, y) } else { (0.0, with_sign(r, y)) });
    }
    if x == 0.0 && normal(y) && dd::abs(y) >= 2.0 * f64::MIN_POSITIVE {
        let t = sqrt(0.5 * dd::abs(y));
        return Some((t, with_sign(t, y)));
    }
    None
}

/// `clog` on an axis for the in-place entry (LIBM-CPLX-LOG-SQRT-4, D-10.74): one part an exact
/// zero, the other normal. `clog(x ± 0i) = (ln|x|, ±0)` for `x > 0` and `(ln|x|, ±π)` for
/// `x < 0`; `clog(±0 + iy) = (ln|y|, π/2)` with the sign of `y` (C Annex G.6.3.2 and conjugate
/// symmetry). `ln|v|` is the real logarithm's in-place kernel ([`fast64::log_nearest`]: correctly
/// rounded, inexact raised in hardware, `|v| != 1`); `clog(1 ± 0i) = (+0, ±0)` is exact. At
/// `|v| = 1` otherwise the real part is an exact zero and the rounded `π` or `π/2` would raise
/// nothing, so those decline.
#[inline(never)]
fn clog_axis(x: f64, y: f64) -> Option<(f64, f64)> {
    let (v, im) = if y == 0.0 && normal(x) {
        if x == 1.0 {
            return Some((0.0, y));
        }
        (x, if x > 0.0 { y } else { with_sign(core::f64::consts::PI, y) })
    } else if x == 0.0 && normal(y) {
        (y, with_sign(core::f64::consts::FRAC_PI_2, y))
    } else {
        return None;
    };
    let v = dd::abs(v);
    if v == 1.0 {
        return None;
    }
    Some((fast64::log_nearest(v)?, im))
}

/// The correctly rounded `double` root in software (the envelope's modes; no `sqrtsd` here).
fn soft_sqrt(v: f64) -> f64 {
    crate::fused::sqrt::<f64, f64>(v, Rounding::Nearest).0
}

/// `csqrt(x + iy)` in `mode` by the quick tier, both parts or `None`.
#[inline(always)]
pub fn csqrt(x: f64, y: f64, mode: Rounding) -> Option<((f64, f64), Status)> {
    let [re, im] = csqrt_parts(x, y, &soft_sqrt)?;
    Some(((round_root(re, mode)?, round_root(im, mode)?), Status::flags(Flags::INEXACT)))
}

/// `clog`'s parts with their bounds, `[(log|z|, eps), (arg z, eps)]` (see above).
#[inline(always)]
fn clog_parts(x: f64, y: f64) -> Option<[(Dd, f64); 2]> {
    let (ax, ay) = window(x, y)?;
    let th = crate::invtrig::atan2_abs(y, x)?;
    let arg = (if y < 0.0 { th.neg() } else { th }, ARG_EPS * dd::abs(th.hi));
    let (a, b) = if ax >= ay { (ax, ay) } else { (ay, ax) };
    let (pa, pb) = (dd::two_prod(a, a), dd::two_prod(b, b));
    let (z, eps) = if (NEAR_LO..NEAR_HI).contains(&(pa.hi + pb.hi)) {
        // |z| near 1: w = (a − 1)(a + 1) + b² with exact products.
        let d = a - 1.0;
        let e = dd::two_sum(a, 1.0);
        let dp = dd::two_prod(d, e.hi);
        let h = dd::two_sum(dp.hi, pb.hi);
        let w = dd::two_sum(h.hi, ((h.lo + dp.lo) + pb.lo) + d * e.lo);
        let (zh, zl) = crate::fast64::quick_log::log1p_quick_parts(w.hi)?;
        let z = dd::fast_two_sum(zh, zl + w.lo / (1.0 + w.hi));
        (z, L1P_REL * dd::abs(zh) + W_ERR * (dd::abs(dp.hi) + pb.hi))
    } else {
        let s = dd::two_sum(pa.hi, pb.hi);
        let s = dd::fast_two_sum(s.hi, s.lo + (pa.lo + pb.lo));
        let (l, tail) = fast64::log_kernel(s.hi)?;
        let z = dd::fast_two_sum(l.hi, l.lo + s.lo / s.hi);
        (z, fast64::log_eps(l, tail) + LOG_EXTRA * (1.0 + dd::abs(l.hi)))
    };
    Some([(z.scale(0.5), 0.5 * eps * WIDEN), arg])
}

/// `clog(x + iy)` to nearest for the boundary's in-place entry.
#[inline(always)]
pub fn clog_nearest(x: f64, y: f64) -> Option<(f64, f64)> {
    let Some([(re, er), (im, ei)]) = clog_parts(x, y) else {
        return clog_axis(x, y);
    };
    Some((round(re, er, 0, Rounding::Nearest)?, round(im, ei, 0, Rounding::Nearest)?))
}

/// `clog(x + iy)` in `mode` by the quick tier, both parts or `None`.
#[inline(always)]
pub fn clog(x: f64, y: f64, mode: Rounding) -> Option<((f64, f64), Status)> {
    let [(re, er), (im, ei)] = clog_parts(x, y)?;
    Some(((round(re, er, 0, mode)?, round(im, ei, 0, mode)?), Status::flags(Flags::INEXACT)))
}

/// The `clog` quick tier for any format.
#[inline(always)]
pub(crate) fn clog_any<F: Float>(x: F, y: F, mode: Rounding) -> Option<((F, F), Status)> {
    tier_any(x, y, |a, b| clog(a, b, mode), |a, b| fast32::clogf(a, b, mode))
}

/// The `csqrt` quick tier for any format.
#[inline(always)]
pub(crate) fn csqrt_any<F: Float>(x: F, y: F, mode: Rounding) -> Option<((F, F), Status)> {
    tier_any(x, y, |a, b| csqrt(a, b, mode), |a, b| fast32::csqrtf(a, b, mode, soft_sqrt))
}

#[path = "complex_fast_pow.rs"]
pub mod pow;

#[cfg(test)]
#[path = "complex_fast_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "complex_fast_ls_tests.rs"]
mod ls_tests;
