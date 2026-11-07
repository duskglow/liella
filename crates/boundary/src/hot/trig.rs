//! The `double` sin, cos, tan, sincos family's boundary wrappers.

/// The `double` `sin`, `cos` and `tan` fast paths (LIBM-F64-TRIG-TIER) in their build chosen at
/// startup (as [`exp_nearest`](super::exp_nearest); the two builds agree bit for bit).
macro_rules! trig_nearest {
    ($($name:ident;)+) => {$(
        #[doc = concat!("`liella_math::trig::kernel::", stringify!($name), "` in the build chosen at startup.")]
        #[inline(always)]
        pub(crate) fn $name(x: f64) -> Option<f64> {
            if crate::dispatch::fma() {
                // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
                #[allow(unsafe_code)]
                return unsafe { liella_math::trig::kernel_fma::$name(x) };
            }
            liella_math::trig::kernel::$name(x)
        }
    )+};
}

trig_nearest! {
    sin_nearest;
    cos_nearest;
    tan_nearest;
}

/// The `double` `sincos` fast path (LIBM-F64-TRIG-TIER) in its build chosen at startup (as
/// [`exp_nearest`](super::exp_nearest)): both results or `None`.
#[inline(always)]
pub(crate) fn sincos_nearest(x: f64) -> Option<(f64, f64)> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::trig::kernel_fma::sincos_nearest(x) };
    }
    liella_math::trig::kernel::sincos_nearest(x)
}

/// The `double` `cexp` quick tier (LIBM-CPLX-EXP-TIER, D-10.53) in its build chosen at startup (as
/// [`exp_nearest`](super::exp_nearest); the two builds agree bit for bit): both parts or `None`.
#[inline(always)]
pub(crate) fn cexp_nearest(x: f64, y: f64) -> Option<(f64, f64)> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::complex_fast::fused::cexp_nearest(x, y) };
    }
    liella_math::complex_fast::cexp_nearest(x, y)
}
