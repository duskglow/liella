//! The 1536-bit step of Ziv's strategy for the complex `_Float128` functions (spec §10.3,
//! D-10.17, MATH-M5-CPLX-F128-ZIV-C): the arithmetic of [`crate::mp`] (`mp::mp_core`,
//! `mp::mp_kernels`) expanded with 24 limbs. Only a part still undecided after the
//! 768-bit step reaches it (`complex::ladder`).
//!
//! The constants come from `cargo xtask tables` (`MPX_*`, 1600 bits before truncation). The
//! error bounds are those proved in [`crate::mpw`]'s documentation for a significand of `B` bits,
//! with two width-dependent points re-proved for `B = 1536`: the Newton counts of `recip` and
//! `sqrt` (`NEWTON` in `mp_core`, from `B`), and the window of `reduce_wide` (`RW = LIMBS + 3`
//! words from bit `e − 1`: word 282 at most, inside the 312-word table). The series
//! kernels stop on `B` and so take up to `B/768` times as many terms as at 768 bits; each of
//! their bounds is a sum over terms or steps, so it grows by that factor at most, which the
//! evaluation adds as `WIDTH_SLACK` bits (`complex_body.rs`).

#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::indexing_slicing, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap, clippy::cast_precision_loss)] // limb arithmetic of fixed width
#![allow(dead_code)] // the shared arithmetic of `mp_core` is wider than the complex evaluation uses

use crate::coeffs::tables as t;
use crate::float::{self, Exact, Float};
use crate::status::{Rounding, Status};

/// Limbs of the significand.
pub const LIMBS: usize = 24;

crate::mp::mp_core!();
crate::mp::mp_kernels!();

/// `π`.
pub fn pi() -> Mp {
    c(&t::MPX_PI)
}

/// `ln 2`.
pub fn ln2() -> Mp {
    c(&t::MPX_LN2)
}

/// Euler's constant `γ`.
pub fn euler() -> Mp {
    c(&t::MPX_EULER)
}

/// `ln x` for `x > 0`: the shared `ln_atanh` kernel (bound proved in [`crate::mpw`]).
pub fn ln(x: Mp) -> Mp {
    ln_atanh(x)
}

#[cfg(test)]
crate::mp::wide_width_tests!();
