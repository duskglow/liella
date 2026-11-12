//! Hardware operations for the kernels (D-NEW, LIBM-HYPOT-DIRECTED; design rules S1/S2).
//!
//! This crate has no `unsafe` (S1), so it cannot issue an instruction that `core` reaches only
//! through `core::arch` (for example `sqrtsd`). A kernel that needs such an operation is generic
//! over [`HwOps`]; the crate's own callers and tests use the safe software default [`SoftOps`],
//! and `liella_boundary` supplies an implementation that issues the instruction through its
//! `hot-kernel` category (S2). The methods are associated functions on a zero-sized type, so after
//! monomorphisation and inlining a call is the instruction itself: no function pointer, no call.
//!
//! The contract of each method is the weaker of the two implementations: a kernel's proof may
//! assume only what is written here. A hardware implementation is usually stricter (a correctly
//! rounded root rather than one within `2^-50`), which every such proof accepts.
//! Room is left for further operations (a fused multiply-add, ...): add a method with its
//! contract and a software default here, its instruction in the boundary.
//! When `core_float_math` is stable, `core`'s safe `sqrt` may become the default; nothing here
//! depends on it.

/// Hardware operations a kernel may be generic over (see the module comment).
pub trait HwOps {
    /// `sqrt(x)` for a normal `x > 0`, within `2^-50` relative of the true root, under the
    /// floating-point environment in force at the call (the kernels run in the default one).
    /// No exception other than inexact may rise.
    fn sqrt(x: f64) -> f64;
}

/// The software implementation of [`HwOps`]: no `unsafe`, any target.
#[derive(Clone, Copy, Debug, Default)]
pub struct SoftOps;

impl HwOps for SoftOps {
    #[inline]
    fn sqrt(x: f64) -> f64 {
        sqrt_soft(x)
    }
}

/// `sqrt(x)` for a normal `x > 0` within `2^-50` relative, in software: the inverse root from
/// the halved exponent (within 3.4%), four Newton steps (each `1.5 e^2` plus about `2^-52` of
/// rounding), times `x`.
#[inline]
fn sqrt_soft(x: f64) -> f64 {
    let mut y = f64::from_bits(0x5fe6_eb50_c7b5_37a9u64.wrapping_sub(x.to_bits() >> 1));
    for _ in 0..4 {
        y *= 1.5 - 0.5 * x * y * y;
    }
    x * y
}
