//! The `double` inverse-trigonometric and `hypot` family's boundary wrappers.

use super::sqrtsd;

/// The `double` inverse-trigonometric and `hypot` fast paths (LIBM-F64-INVTRIG-HYPOT-TIER) in
/// their build chosen at startup (as [`exp_nearest`](super::exp_nearest); the two builds agree bit for bit).
macro_rules! invtrig_nearest {
    ($($name:ident($($a:ident),+) [$($s:expr)?];)+) => {$(
        #[doc = concat!("`liella_math::invtrig::", stringify!($name), "` in the build chosen at startup.")]
        #[inline(always)]
        pub(crate) fn $name($($a: f64),+) -> Option<f64> {
            if crate::dispatch::fma() {
                // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
                #[allow(unsafe_code)]
                return unsafe { liella_math::invtrig::fused::$name($($a),+ $(, $s)?) };
            }
            liella_math::invtrig::$name($($a),+ $(, $s)?)
        }
    )+};
}

invtrig_nearest! {
    atan_nearest(x) [];
    asin_nearest(x) [sqrtsd];
    acos_nearest(x) [sqrtsd];
    atan2_nearest(y, x) [];
    hypot_nearest(x, y) [sqrtsd];
}

/// The `double` `csqrt` quick tier (LIBM-CPLX-LOG-SQRT, D-10.74) with the hardware root: both
/// parts or `None`.
#[inline(always)]
pub(crate) fn csqrt_nearest(x: f64, y: f64) -> Option<(f64, f64)> {
    liella_math::complex_fast::csqrt_nearest(x, y, sqrtsd)
}

/// The `float` `csqrtf` quick tier (LIBM-CPLX-LOG-SQRT, D-10.74) with the hardware root.
#[inline(always)]
pub(crate) fn csqrtf_nearest(x: f32, y: f32) -> Option<(f32, f32)> {
    liella_math::fast32::csqrtf_nearest(x, y, sqrtsd)
}
