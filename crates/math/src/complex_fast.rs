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
$(#[$a])*
$(#[$i])*
pub fn cexp_nearest(x: f64, y: f64) -> Option<(f64, f64)> {
    let ([(re, er), (im, ei)], m) = parts(x, y)?;
    Some((round(re, er, m, Rounding::Nearest)?, round(im, ei, m, Rounding::Nearest)?))
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
    Some(((round(re, er, m, mode)?, round(im, ei, m, mode)?), Status::flags(Flags::INEXACT)))
}

/// The quick tier for any format: `float` and `double` operands, `None` for the others.
#[inline(always)]
pub(crate) fn cexp_any<F: Float>(x: F, y: F, mode: Rounding) -> Option<((F, F), Status)> {
    match F::PREC {
        53 => {
            let ((a, b), st) = cexp(f64::from_bits(x.to_bits() as u64), f64::from_bits(y.to_bits() as u64), mode)?;
            Some(((F::from_bits(u128::from(a.to_bits())), F::from_bits(u128::from(b.to_bits()))), st))
        }
        24 => {
            let ((a, b), st) = fast32::cexpf(f32::from_bits(x.to_bits() as u32), f32::from_bits(y.to_bits() as u32), mode)?;
            Some(((F::from_bits(u128::from(a.to_bits())), F::from_bits(u128::from(b.to_bits()))), st))
        }
        _ => None,
    }
}

#[cfg(test)]
#[path = "complex_fast_tests.rs"]
mod tests;
