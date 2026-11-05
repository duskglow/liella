//! The `double` exp/log family's boundary wrappers (build chosen at startup).

/// The `double` `exp` fast path in its build chosen at startup (MATH-F64-FMA-PATH, D-10.25):
/// both builds return the same results bit for bit; the FMA one is shorter.
#[inline(always)]
pub(crate) fn exp_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: a call to the FMA build; the flag is set only on a CPU with
        // FMA whose AVX state the OS saves (`dispatch::select_fma`'s callers).
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::fused::exp_nearest(x) };
    }
    liella_math::fast64::exp_nearest(x)
}

/// The `double` `exp10` fast path in its build chosen at startup (as [`exp_nearest`]).
#[inline(always)]
pub(crate) fn exp10_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::fused::exp10_nearest(x) };
    }
    liella_math::fast64::exp10_nearest(x)
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
/// LIBM-F64-EXPLOGPOW-TIER).
#[inline(always)]
pub(crate) fn exp2_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::fused::exp2_nearest(x) };
    }
    liella_math::fast64::exp2_nearest(x)
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

