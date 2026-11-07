//! The `double` exp/log family's boundary wrappers (build chosen at startup).

/// The `double` `exp` fast path in its build chosen at startup (MATH-F64-FMA-PATH, D-10.25): the
/// quick phase with `fast64::exp_nearest` as tier 2 on a miss (D-10.51,
/// LIBM-F64-EXP-QUICK-WIRE); both builds return the same results bit for bit, the FMA one is
/// shorter.
#[inline(always)]
pub(crate) fn exp_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: a call to the FMA build; the flag is set only on a CPU with
        // FMA whose AVX state the OS saves (`dispatch::select_fma`'s callers).
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick::fused::exp_tiered(x) };
    }
    liella_math::fast64::quick::exp_tiered(x)
}

/// The `double` `exp` quick phase alone (LIBM-F64-EXP-QUICK, D-10.51) in its build chosen at
/// startup (as [`exp_nearest`]): the correctly rounded result, or `None` when the rounding test
/// fails (bound and count tests).
#[cfg(feature = "selftest")]
pub(crate) fn exp_quick(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick::fused::exp_quick(x) };
    }
    liella_math::fast64::quick::exp_quick(x)
}

/// [`exp_quick`]'s unrounded `(zh, zl, scale)` in the build chosen at startup (bound tests).
#[cfg(feature = "selftest")]
pub(crate) fn exp_quick_parts(x: f64) -> Option<(f64, f64, f64)> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick::fused::exp_quick_parts(x) };
    }
    liella_math::fast64::quick::exp_quick_parts(x)
}

/// The `double` `exp2` quick phase alone (D-10.64) in the build chosen at startup (bound tests).
#[cfg(feature = "selftest")]
pub(crate) fn exp2_quick(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick::fused::exp2_quick(x) };
    }
    liella_math::fast64::quick::exp2_quick(x)
}

/// [`exp2_quick`]'s unrounded `(zh, zl, scale)` in the build chosen at startup (bound tests).
#[cfg(feature = "selftest")]
pub(crate) fn exp2_quick_parts(x: f64) -> Option<(f64, f64, f64)> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick::fused::exp2_quick_parts(x) };
    }
    liella_math::fast64::quick::exp2_quick_parts(x)
}

/// The `double` `exp10` quick phase alone (D-10.64) in the build chosen at startup (bound tests).
#[cfg(feature = "selftest")]
pub(crate) fn exp10_quick(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick::fused::exp10_quick(x) };
    }
    liella_math::fast64::quick::exp10_quick(x)
}

/// [`exp10_quick`]'s unrounded `(zh, zl, scale)` in the build chosen at startup (bound tests).
#[cfg(feature = "selftest")]
pub(crate) fn exp10_quick_parts(x: f64) -> Option<(f64, f64, f64)> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick::fused::exp10_quick_parts(x) };
    }
    liella_math::fast64::quick::exp10_quick_parts(x)
}

/// The `double` `exp10` fast path in its build chosen at startup (as [`exp_nearest`]): the quick
/// phase with `fast64::exp10_nearest` as tier 2 (D-10.64, LIBM-F64-EXP-FAMILY-QUICK).
#[inline(always)]
pub(crate) fn exp10_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick::fused::exp10_tiered(x) };
    }
    liella_math::fast64::quick::exp10_tiered(x)
}

/// The `double` `log` fast path in its build chosen at startup (as [`exp_nearest`]).
#[inline(always)]
pub(crate) fn log_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::fused::log_nearest(x) };
    }
    liella_math::fast64::log_nearest(x)
}

/// The `double` `exp2` fast path in its build chosen at startup (as [`exp_nearest`];
/// LIBM-F64-EXPLOGPOW-TIER): the quick phase with `fast64::exp2_nearest` as tier 2 (D-10.64,
/// LIBM-F64-EXP-FAMILY-QUICK).
#[inline(always)]
pub(crate) fn exp2_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick::fused::exp2_tiered(x) };
    }
    liella_math::fast64::quick::exp2_tiered(x)
}

/// The `double` `log2` fast path in its build chosen at startup (as [`exp_nearest`]).
#[inline(always)]
pub(crate) fn log2_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::fused::log2_nearest(x) };
    }
    liella_math::fast64::log2_nearest(x)
}

/// The `double` `log10` fast path in its build chosen at startup (as [`exp_nearest`]).
#[inline(always)]
pub(crate) fn log10_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::fused::log10_nearest(x) };
    }
    liella_math::fast64::log10_nearest(x)
}

/// The `double` `pow` fast path in its build chosen at startup (as [`exp_nearest`]); takes the
/// pair the generated entry passes.
#[inline(always)]
pub(crate) fn pow_nearest((x, y): (f64, f64)) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::fused::pow_nearest(x, y) };
    }
    liella_math::fast64::pow_nearest(x, y)
}

/// The `double` `expm1` fast path (LIBM-F64-EXPM1-HYPER-CBRT-TIER) in its build chosen at
/// startup (as [`exp_nearest`]).
#[inline(always)]
pub(crate) fn expm1_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::fused::expm1_nearest(x) };
    }
    liella_math::fast64::expm1_nearest(x)
}

/// The `double` `log1p` fast path in its build chosen at startup (as [`exp_nearest`]).
#[inline(always)]
pub(crate) fn log1p_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::fused::log1p_nearest(x) };
    }
    liella_math::fast64::log1p_nearest(x)
}

/// The `double` `cbrt` fast path in its build chosen at startup (as [`exp_nearest`]); it declines
/// exact cubes, so a refused attempt's inexact is undone by `mathret::elem_fast_cold`. Its kernel
/// lives in `liella_math::fast64::exp_log`, so its wrapper lives here.
#[inline(always)]
pub(crate) fn cbrt_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::fused::cbrt_nearest(x) };
    }
    liella_math::fast64::cbrt_nearest(x)
}

