//! The hyperbolic family's boundary wrappers.

use super::sqrtsd;

/// The `float` asinh and acosh fast paths (LIBM-F32-HYPERBOLIC-4) with their square root
/// issued inline by [`sqrtsd`] (in `hot`) (liella-math forbids `unsafe` and so cannot issue it; its
/// own root, [`liella_math::fast32::asinh_nearest_sqrt`]'s fallback, is about 20 dependent
/// operations). A correctly rounded root is within the cores' proved bound, so the result is
/// the same `float`.
#[inline(always)]
pub(crate) fn asinhf_nearest(x: f32) -> Option<f32> {
    liella_math::fast32::asinh_nearest_sqrt(x, sqrtsd)
}

/// [`asinhf_nearest`] for acosh.
#[inline(always)]
pub(crate) fn acoshf_nearest(x: f32) -> Option<f32> {
    liella_math::fast32::acosh_nearest_sqrt(x, sqrtsd)
}
