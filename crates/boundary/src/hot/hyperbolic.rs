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

/// The `double` `sinh` fast path in its build chosen at startup (as [`super::exp_nearest`]): the
/// quick phase on the exp kernel (D-10.70, LIBM-F64-HYPERB-QUICK), then tier 2
/// (LIBM-F64-EXPM1-HYPER-CBRT-TIER) out of line.
#[inline(always)]
pub(crate) fn sinh_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_log::exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick::fused::sinh_tiered(x) };
    }
    liella_math::fast64::quick::sinh_tiered(x)
}

/// The `double` `cosh` fast path in its build chosen at startup (as [`super::exp_nearest`]).
#[inline(always)]
pub(crate) fn cosh_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_log::exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick::fused::cosh_tiered(x) };
    }
    liella_math::fast64::quick::cosh_tiered(x)
}

/// The `double` `tanh` fast path in its build chosen at startup (as [`super::exp_nearest`]).
#[inline(always)]
pub(crate) fn tanh_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_log::exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick::fused::tanh_tiered(x) };
    }
    liella_math::fast64::quick::tanh_tiered(x)
}

/// The `double` `asinh` quick phase (D-10.65, LIBM-F64-INVHYP-QUICK) in its build chosen at
/// startup (as [`super::exp_nearest`]), with its square root issued inline by [`sqrtsd`]: the
/// correctly rounded result, or `None` (a miss, or outside the quick ranges), and the caller runs
/// the accurate path (tier 2, unchanged).
#[inline(always)]
pub(crate) fn asinh_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_log::exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick_invhyp::fused::asinh_quick(x, sqrtsd) };
    }
    liella_math::fast64::quick_invhyp::asinh_quick(x, sqrtsd)
}

/// [`asinh_nearest`] for `acosh`.
#[inline(always)]
pub(crate) fn acosh_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_log::exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick_invhyp::fused::acosh_quick(x, sqrtsd) };
    }
    liella_math::fast64::quick_invhyp::acosh_quick(x, sqrtsd)
}

/// [`asinh_nearest`] for `atanh` (no square root).
#[inline(always)]
pub(crate) fn atanh_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_log::exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick_invhyp::fused::atanh_quick(x) };
    }
    liella_math::fast64::quick_invhyp::atanh_quick(x)
}

/// The `double` `sinh` (`f = 0`), `cosh` (1) or `tanh` (2) quick phase alone (D-10.70) in the
/// build chosen at startup: the correctly rounded result, or `None` (tests only).
pub(crate) fn hyp_quick(f: u8, x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_log::exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick::fused::hyp_quick(f, x) };
    }
    liella_math::fast64::quick::hyp_quick(f, x)
}
