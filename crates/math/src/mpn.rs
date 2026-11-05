//! The narrow first step of Ziv's strategy for the x87 `long double` functions (spec §10.3,
//! D-10.19): the arithmetic of [`crate::mp`] (`mp::mp_core`, `mp::mp_kernels`) expanded with
//! three limbs, a 192-bit significand. A result for a 64-bit format needs about 2^-75 relative
//! accuracy to be decided, so the 384-bit evaluation of [`crate::wide`] is the retry step for the
//! few results whose error interval straddles a rounding boundary ([`crate::wide`]'s `fast`
//! tier), not the common path.
//!
//! The constants are the 384-bit ones of [`crate::mp`] truncated toward zero to 192 bits (an
//! error below one unit of the last place, as in the proofs of [`crate::mpw`]). Every error
//! bound proved there for a significand of `B` bits holds here with `B = 192`; the width-dependent
//! points are the Newton counts (`NEWTON`, four at `B ≤ 800`) and the window of `reduce_wide`
//! (`RW = LIMBS + 3` words from bit `e − 1`, the table's remainder weighs `< 2^(−B−76)`
//! quadrants). The largest of those bounds relative to the result, for the arguments `long
//! double` can take, is `2^(77 − B)` (the logarithms near 1, where `|ln x| ≥ 2^-65`); the
//! rounding test of `wide`'s fast tier allows `2^(96 − B)`.

#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::indexing_slicing, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap, clippy::cast_precision_loss)] // limb arithmetic of fixed width
#![allow(dead_code)] // the shared arithmetic of `mp_core` is wider than the long double evaluation uses

use crate::coeffs::tables as t;
use crate::float::{self, Exact, Float};
use crate::status::{Rounding, Status};

/// Limbs of the significand.
pub const LIMBS: usize = 3;

/// Bits of the significand, for the rounding test of [`crate::wide`].
pub const WIDTH: i64 = 64 * LIMBS as i64;

crate::mp::mp_core!();
crate::mp::mp_kernels!();

/// A 384-bit constant truncated to this width.
const fn narrow(k: &crate::mp::MpConst) -> MpConst {
    (k.0, [k.1[3], k.1[4], k.1[5]])
}

/// `π`.
pub fn pi() -> Mp {
    c(&narrow(&t::MP_PI))
}

/// `ln 2`.
pub fn ln2() -> Mp {
    c(&narrow(&t::MP_LN2))
}

/// Euler's constant `γ`.
pub fn euler() -> Mp {
    c(&narrow(&t::MP_EULER))
}

/// `ln x` for `x > 0`: the shared `ln_atanh` kernel (bound proved in [`crate::mpw`]).
pub fn ln(x: Mp) -> Mp {
    ln_atanh(x)
}

#[cfg(test)]
crate::mp::wide_width_tests!();
