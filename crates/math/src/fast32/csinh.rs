//! The `float` `csinh` and `csin` quick tier (LIBM-CPLX-SINCOS, as [`super::cexpf`]): `sinh x·cos y`
//! and `cosh x·sin y` in `double` from [`sinh_core`] (12.2u, `2^-26 <= |x| < 89`), [`cosh_core`]
//! (`2^-45.14` = 233u, `|x| < 89.5`) and [`trig_core`] (24.3u each, `2^-26 <= |y| < 2^19`), each part
//! one `double` product (u): within 37.5u and 258.3u (second-order terms below 0.01u), so within
//! 38 and 259 ulp of the `double` part; rounded to `float` by [`round`] with the margins below
//! (`>= 1.5·bound + 1`, as `cexpf`'s). `csin(x + iy)` is `csinh(y + ix)` with its parts swapped.
//! Every operand and product is a normal `double` (`|sinh x| >= 2^-26`, the trig values `>=
//! 2^-37`, `cosh x >= 1`), and [`round`] refuses results outside the normal `float` range: only
//! inexact can arise.

use super::*;

/// The margins of `sinh x·cos y` and `cosh x·sin y` in ulp of the `double` part.
pub(crate) const CSINH_RE_DELTA: u64 = 64;
pub(crate) const CSINH_IM_DELTA: u64 = 512;

/// `(sinh x·cos y, cosh x·sin y)` as `double`s within the margins above, or `None`.
#[inline(always)]
pub(crate) fn csinh_core(x: f32, y: f32) -> Option<(f64, f64)> {
    let sh = sinh_core(x)?;
    let ch = cosh_core(x)?;
    let (s, c) = trig_core(y)?;
    Some((sh * c, ch * s))
}

/// `csinhf` in `mode`, both parts or `None` (the 384-bit route then runs).
#[inline(always)]
pub fn csinhf(x: f32, y: f32, mode: Rounding) -> Option<((f32, f32), Status)> {
    let (re, im) = csinh_core(x, y)?;
    Some(((round(re, CSINH_RE_DELTA, mode)?, round(im, CSINH_IM_DELTA, mode)?), Status::flags(Flags::INEXACT)))
}

/// `csinf` in `mode`: `csinhf(y + ix)` swapped.
#[inline(always)]
pub fn csinf(x: f32, y: f32, mode: Rounding) -> Option<((f32, f32), Status)> {
    let (im, re) = csinh_core(y, x)?;
    Some(((round(re, CSINH_IM_DELTA, mode)?, round(im, CSINH_RE_DELTA, mode)?), Status::flags(Flags::INEXACT)))
}

/// `csinhf` to nearest for the in-place entry: both parts or `None` (each conversion to `float`
/// is inexact, the test excluding a `float` value: inexact is raised and nothing else).
#[inline(always)]
pub fn csinhf_nearest(x: f32, y: f32) -> Option<(f32, f32)> {
    let (re, im) = csinh_core(x, y)?;
    Some((round(re, CSINH_RE_DELTA, Rounding::Nearest)?, round(im, CSINH_IM_DELTA, Rounding::Nearest)?))
}

/// `csinf` to nearest for the in-place entry.
#[inline(always)]
pub fn csinf_nearest(x: f32, y: f32) -> Option<(f32, f32)> {
    let (im, re) = csinh_core(y, x)?;
    Some((round(re, CSINH_IM_DELTA, Rounding::Nearest)?, round(im, CSINH_RE_DELTA, Rounding::Nearest)?))
}
