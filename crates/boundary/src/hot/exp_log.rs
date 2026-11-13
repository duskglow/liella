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

/// The `double` `exp` quick phase in the caller's directed rounding direction (D-10.94) in its
/// build chosen at startup (as [`exp_nearest`]): `e^x` rounded in `mode`, or `None` (the entry then
/// switches). `mode` is the caller's direction, never round to nearest. The direction picks an
/// instance by a branch (predicted: the mode rarely changes), so the arithmetic depends on the
/// MXCSR read by control only; as data (`h` selected from it) it waited for that read, and the
/// read for the caller's earlier arithmetic, which serialised successive calls (D-10.94).
#[inline(always)]
pub(crate) fn exp_directed(x: f64, mode: liella_math::Rounding) -> Option<f64> {
    if matches!(mode, liella_math::Rounding::Up) { exp_directed_in::<true>(x) } else { exp_directed_in::<false>(x) }
}

/// [`exp_directed`] for one direction class (`UP`: upward; else downward or toward zero).
#[inline(always)]
fn exp_directed_in<const UP: bool>(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick::fused::exp_directed::<UP>(x) };
    }
    liella_math::fast64::quick::exp_directed::<UP>(x)
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

/// The `double` `log` fast path in its build chosen at startup (as [`exp_nearest`]): the quick
/// phase with `fast64::log_nearest` as tier 2 on a miss (D-10.51, D-10.60, LIBM-F64-LOG-QUICK).
#[inline(always)]
pub(crate) fn log_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick_log::fused::log_tiered(x) };
    }
    liella_math::fast64::quick_log::log_tiered(x)
}

/// The `double` `log` quick phase alone (D-10.60) in its build chosen at startup (as
/// [`exp_nearest`]): the correctly rounded result, or `None` when the rounding test fails.
#[cfg(feature = "selftest")]
pub(crate) fn log_quick(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick_log::fused::log_quick(x) };
    }
    liella_math::fast64::quick_log::log_quick(x)
}

/// [`log_quick`]'s unrounded `(zh, zl)` in the build chosen at startup (bound tests).
#[cfg(feature = "selftest")]
pub(crate) fn log_quick_parts(x: f64) -> Option<(f64, f64)> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick_log::fused::log_quick_parts(x) };
    }
    liella_math::fast64::quick_log::log_quick_parts(x)
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

/// The `double` `log2` fast path in its build chosen at startup (as [`exp_nearest`]): the quick
/// phase with `fast64::log2_nearest` as tier 2 on a miss (D-10.51, D-10.66).
#[inline(always)]
pub(crate) fn log2_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick_log::fused::log2_tiered(x) };
    }
    liella_math::fast64::quick_log::log2_tiered(x)
}

/// The `double` `log10` fast path in its build chosen at startup (as [`exp_nearest`]): the quick
/// phase with `fast64::log10_nearest` as tier 2 on a miss (D-10.51, D-10.66).
#[inline(always)]
pub(crate) fn log10_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick_log::fused::log10_tiered(x) };
    }
    liella_math::fast64::quick_log::log10_tiered(x)
}

/// The quick phase of `log2`, `log10` or `log1p` (`f` names it; D-10.66) in its build chosen at
/// startup: the unrounded `(zh, zl)` and the rounded answer (`None` when the test fails).
#[cfg(feature = "selftest")]
pub(crate) fn log_family_quick(f: &str, x: f64) -> (Option<(f64, f64)>, Option<f64>) {
    use liella_math::fast64::quick_log as q;
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: calls to the FMA build in the arms below; as in
        // `exp_nearest`, the flag is set only on a CPU with FMA whose AVX state the OS saves.
        #[allow(unsafe_code)]
        return match f {
            "log2" => (
                // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
                unsafe { q::fused::log2_quick_parts(x) },
                // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
                unsafe { q::fused::log2_quick(x) },
            ),
            "log10" => (
                // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
                unsafe { q::fused::log10_quick_parts(x) },
                // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
                unsafe { q::fused::log10_quick(x) },
            ),
            _ => (
                // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
                unsafe { q::fused::log1p_quick_parts(x) },
                // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
                unsafe { q::fused::log1p_quick(x) },
            ),
        };
    }
    match f {
        "log2" => (q::log2_quick_parts(x), q::log2_quick(x)),
        "log10" => (q::log10_quick_parts(x), q::log10_quick(x)),
        _ => (q::log1p_quick_parts(x), q::log1p_quick(x)),
    }
}

/// The `double` `pow` fast path in its build chosen at startup (as [`exp_nearest`]); takes the
/// pair the generated entry passes. Tier 1 is the quick phase (D-10.62), tier 2 today's
/// `fast64::pow_nearest` on a miss.
#[inline(always)]
pub(crate) fn pow_nearest((x, y): (f64, f64)) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick_pow::fused::pow_tiered(x, y) };
    }
    liella_math::fast64::quick_pow::pow_tiered(x, y)
}

/// The `double` `pow` quick phase alone (D-10.62) in its build chosen at startup (as
/// [`exp_nearest`]): the correctly rounded result, or `None` when the phase declines.
#[cfg(feature = "selftest")]
pub(crate) fn pow_quick(x: f64, y: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick_pow::fused::pow_quick(x, y) };
    }
    liella_math::fast64::quick_pow::pow_quick(x, y)
}

/// The `double` `expm1` fast path (LIBM-F64-EXPM1-HYPER-CBRT-TIER) in its build chosen at
/// startup (as [`exp_nearest`]).
#[inline(always)]
pub(crate) fn expm1_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick::fused::expm1_tiered(x) };
    }
    liella_math::fast64::quick::expm1_tiered(x)
}

/// The `double` `expm1` quick phase alone (D-10.64) in the build chosen at startup (tests).
#[cfg(feature = "selftest")]
pub(crate) fn expm1_quick(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick::fused::expm1_quick(x) };
    }
    liella_math::fast64::quick::expm1_quick(x)
}

/// [`expm1_quick`]'s unrounded `(zh, zl)` in the build chosen at startup (bound tests).
#[cfg(feature = "selftest")]
pub(crate) fn expm1_quick_parts(x: f64) -> Option<(f64, f64)> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick::fused::expm1_quick_parts(x) };
    }
    liella_math::fast64::quick::expm1_quick_parts(x)
}

/// The `double` `log1p` fast path in its build chosen at startup (as [`exp_nearest`]): the quick
/// phase with `fast64::log1p_nearest` as tier 2 on a miss (D-10.51, D-10.66).
#[inline(always)]
pub(crate) fn log1p_nearest(x: f64) -> Option<f64> {
    if crate::dispatch::fma() {
        // UNSAFE(hot-kernel) SAFETY: as in `exp_nearest`.
        #[allow(unsafe_code)]
        return unsafe { liella_math::fast64::quick_log::fused::log1p_tiered(x) };
    }
    liella_math::fast64::quick_log::log1p_tiered(x)
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

