//! The `double` erf, erfc family's boundary wrappers (build chosen at startup).

/// The `double` `erf` in-place path in its build chosen at startup (as
/// [`crate::hot::exp_nearest`]; LIBM-SPECIAL-FAST-TIER-3): both builds return the same results bit
/// for bit.
#[inline(always)]
pub(crate) fn erf_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_log::exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast_special::fused::erf_nearest(x) };
    }
    liella_math::fast_special::erf_nearest(x)
}

/// The `double` `erfc` in-place path in its build chosen at startup (as [`erf_nearest`]).
#[inline(always)]
pub(crate) fn erfc_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_log::exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast_special::fused::erfc_nearest(x) };
    }
    liella_math::fast_special::erfc_nearest(x)
}
