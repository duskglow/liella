//! The `float` sin, cos, tan, sincos family's boundary wrappers. A story adds them here, not in
//! `hot/mod.rs` (LIBM-HOT-PER-FAMILY).

use super::fpregs::widen;

/// The `float` sin fast path with its `double` operand converted inline by [`widen`], one
/// `cvtss2sd` in place (coordinator ruling #4434, D-10.79; liella-math forbids `unsafe` and so cannot
/// pin the conversion's register: its own operand is built from the bits, six instructions). The
/// conversion is exact on the core's range, so the result is the same `float`.
#[inline(always)]
pub(crate) fn sinf_nearest(x: f32) -> Option<f32> {
    liella_math::fast32::sin_nearest_widen(x, widen)
}

/// [`sinf_nearest`] for cos.
#[inline(always)]
pub(crate) fn cosf_nearest(x: f32) -> Option<f32> {
    liella_math::fast32::cos_nearest_widen(x, widen)
}

/// [`sinf_nearest`] for tan.
#[inline(always)]
pub(crate) fn tanf_nearest(x: f32) -> Option<f32> {
    liella_math::fast32::tan_nearest_widen(x, widen)
}
