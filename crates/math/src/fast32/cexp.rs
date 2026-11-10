//! The `float` `cexp` quick tier (LIBM-CPLX-EXP-TIER, D-10.53): `e^x·cos y` and `e^x·sin y`
//! evaluated in `double` from the `expf` and `sinf`/`cosf` kernels, each part rounded to `float`
//! by the online test of [`round`] (DESIGN.md §3.1 B).

use super::*;

/// The parts' margin in units of the last place of the `double` part: `e^x` from [`exp_core`]
/// (relative `2^-45.16` = 229.1·2^-53), `cos y` or `sin y` from [`trig_core`] (24.3·2^-53), the
/// product's rounding (1·2^-53) and the cross term (`< 2^-90`): `ε <= 254.5·2^-53`, so the part
/// is within `ε·2^53/(1 − ε) < 255` ulp of the exact value; 512 keeps the `1.5·bound + 1` margin
/// of the other kernels.
pub(crate) const CEXP_DELTA: u64 = 512;

/// `(e^x·cos y, e^x·sin y)` as `double`s within [`CEXP_DELTA`] ulp each, for `x` in
/// `(−104, 89)` (where [`exp_core`]'s scale is a normal `double`) and `2^-26 <= |y| < 2^19`
/// ([`trig_core`]'s range); `None` elsewhere (NaN, infinities, zeros included). Every operand
/// and product is a normal `double` (`|e^x| >= 2^-150`, the trig values `>= 2^-37`).
#[inline(always)]
pub(crate) fn cexp_core(x: f32, y: f32) -> Option<(f64, f64)> {
    let e = exp_core(x, -104.0, 89.0, INV_L)?;
    let (s, c) = trig_core(y)?;
    Some((e * c, e * s))
}

/// `cexpf` in `mode`, both parts or `None` (the 384-bit route then runs). Each part is a normal
/// `float` by [`round`]'s range check, so the status is inexact alone.
#[inline(always)]
pub fn cexpf(x: f32, y: f32, mode: Rounding) -> Option<((f32, f32), Status)> {
    let (re, im) = cexp_core(x, y)?;
    Some(((round(re, CEXP_DELTA, mode)?, round(im, CEXP_DELTA, mode)?), Status::flags(Flags::INEXACT)))
}

/// `cexpf` to nearest for the in-place entry: both parts or `None`. Hardware arithmetic on
/// normal `double`s only; the conversion of each part to `float` is inexact (the rounding test
/// excludes a `float` value), so inexact is raised and nothing else.
#[inline(always)]
pub fn cexpf_nearest(x: f32, y: f32) -> Option<(f32, f32)> {
    let (re, im) = cexp_core(x, y)?;
    Some((round(re, CEXP_DELTA, Rounding::Nearest)?, round(im, CEXP_DELTA, Rounding::Nearest)?))
}
