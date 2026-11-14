//! The `float` asin, acos, atan, atan2, hypot family's boundary wrappers. A story adds them here, not in
//! `hot/mod.rs` (LIBM-HOT-PER-FAMILY).

use super::sqrtsd;

/// `asinf`'s one-pass fast path (LIBM-F32-REST, D-10.82) with its square root issued inline by
/// [`sqrtsd`] (liella-math forbids `unsafe` and so cannot issue it). `sqrtsd` rounds in the
/// caller's direction (the kernel's bound allows it) and writes its whole register.
#[inline(always)]
pub(crate) fn asinf_onepass(x: f32) -> Option<f32> {
    liella_math::fast32::asin_onepass_any(x, sqrtsd)
}

/// [`asinf_onepass`] for acos.
#[inline(always)]
pub(crate) fn acosf_onepass(x: f32) -> Option<f32> {
    liella_math::fast32::acos_onepass_any(x, sqrtsd)
}
