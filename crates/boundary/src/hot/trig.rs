//! The `double` sin, cos, tan, sincos family's boundary wrappers.

/// The `double` `sin`, `cos` and `tan` fast paths in their build chosen at startup (as
/// [`exp_nearest`](super::exp_nearest)): the quick phase (LIBM-F64-TRIG-QUICK, D-10.51), then
/// today's tier (LIBM-F64-TRIG-TIER); both builds are correctly rounded wherever they answer.
macro_rules! trig_nearest {
    ($($name:ident = $tiered:ident;)+) => {$(
        #[doc = concat!("`liella_math::trig::quick::plain::", stringify!($tiered), "` in the build chosen at startup.")]
        #[inline(always)]
        pub(crate) fn $name(x: f64) -> Option<f64> {
            if crate::dispatch::fma() {
                // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
                #[allow(unsafe_code)]
                return unsafe { liella_math::trig::quick::fused::$tiered(x) };
            }
            liella_math::trig::quick::plain::$tiered(x)
        }
    )+};
}

trig_nearest! {
    sin_nearest = sin_tiered;
    cos_nearest = cos_tiered;
    tan_nearest = tan_tiered;
}

/// The `double` sin, cos, tan quick phase alone (LIBM-F64-TRIG-QUICK, D-10.51, D-10.61) in its
/// build chosen at startup (as [`exp_nearest`](super::exp_nearest)): `which` 0 sin, 1 cos,
/// otherwise tan; the correctly rounded result or `None` when the rounding test fails (tests).
#[cfg(feature = "selftest")]
pub(crate) fn trig_quick(which: u8, x: f64) -> Option<f64> {
    use liella_math::trig::quick::{fused, plain};
    if crate::dispatch::fma() {
        let f: unsafe fn(f64) -> Option<f64> = match which {
            0 => fused::sin_quick,
            1 => fused::cos_quick,
            _ => fused::tan_quick,
        };
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { f(x) };
    }
    match which {
        0 => plain::sin_quick(x),
        1 => plain::cos_quick(x),
        _ => plain::tan_quick(x),
    }
}

/// The `double` `sincos` quick phase alone in its build chosen at startup (tests).
#[cfg(feature = "selftest")]
pub(crate) fn sincos_quick(x: f64) -> Option<(f64, f64)> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::trig::quick::fused::sincos_quick(x) };
    }
    liella_math::trig::quick::plain::sincos_quick(x)
}

/// The `double` `sincos` fast path (LIBM-F64-TRIG-TIER) in its build chosen at startup (as
/// [`exp_nearest`](super::exp_nearest)): both results or `None`.
#[inline(always)]
pub(crate) fn sincos_nearest(x: f64) -> Option<(f64, f64)> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::trig::quick::fused::sincos_tiered(x) };
    }
    liella_math::trig::quick::plain::sincos_tiered(x)
}
