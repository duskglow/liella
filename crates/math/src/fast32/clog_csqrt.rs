//! The `float` `clog` and `csqrt` quick tiers (LIBM-CPLX-LOG-SQRT, D-10.74): each part evaluated
//! in `double` (`u = 2^-53`) and rounded to `float` by the online test of [`round`], which also
//! declines a part within its margin of a `float` or a midpoint (so an exact `csqrtf` part, as in
//! `csqrtf(3 + 4i)`, goes to the route). Operands: normal finite nonzero `float`s (their
//! conversion to `double` raises nothing); every intermediate is a normal `double` (`|x|`, `|y|`
//! in `[2^-126, 2^128)`, so squares lie in `[2^-252, 2^256)`).

use super::*;

/// `clogf`'s real part margin: `log|z| = ln(s)/2`, `a >= b` the absolute parts.
/// * `s = RN(a² + b²) < 1/2` (squares exact, one rounding): `ln s` moves by `<= 1.01u` against
///   `|ln s| >= ln 2`, `1.46u`; `log_quick_parts` and its head alone `1.01u`: `2.5u`.
/// * otherwise `w = (a − 1)(a + 1) + b²`: for `a < 1` (`a >= 1/2`) the three operations before
///   the sum are exact (24- and 25-bit factors), so `w` is the sum rounded once, `u·|w|`, however
///   much its terms cancel; for `a >= 1` every term is positive, `4.1u·|w|`. `log1p` moves by
///   `δ·|w|/((1 + w)·|log1p w|) <= 1.443δ` (`w >= −1/2`): `5.92u`, with `log1p_quick_parts` and
///   its head `7u`. So `|y − f| <= 7u·|f| < 14` ulp of `y`; 32 keeps the `1.5·bound + 1` margin.
pub(crate) const CLOG_DELTA: u64 = 32;
/// `csqrtf`'s margin: `a² + b²` `u`, its root `1.5u`, `+ a` `2.5u`, the halving exact, the root
/// `2.25u`, the quotient `b/(2t)` `3.25u`: below 6.5 ulp; 16 keeps the margin.
pub(crate) const CSQRT_DELTA: u64 = 16;

/// `|x|` and `|y|` as `double`s when both are normal finite nonzero `float`s.
#[inline(always)]
fn operands(x: f32, y: f32) -> Option<(f64, f64)> {
    let (ax, ay) = (x.to_bits() & 0x7fff_ffff, y.to_bits() & 0x7fff_ffff);
    if !(0x0080_0000..0x7f80_0000).contains(&ax) || !(0x0080_0000..0x7f80_0000).contains(&ay) {
        return None;
    }
    Some((f64::from(f32::from_bits(ax)), f64::from(f32::from_bits(ay))))
}

/// `(log|z|, arg z)` as `double`s within [`CLOG_DELTA`] and [`ATAN2_DELTA`] ulp, or `None`.
#[inline(always)]
fn clog_core(x: f32, y: f32) -> Option<(f64, f64)> {
    let (ax, ay) = operands(x, y)?;
    let im = atan2_core(y, x)?;
    let (a, b) = if ax >= ay { (ax, ay) } else { (ay, ax) };
    let s = a * a + b * b;
    let (zh, _) = if s < 0.5 {
        crate::fast64::quick_log::log_quick_parts(s)?
    } else {
        crate::fast64::quick_log::log1p_quick_parts((a - 1.0) * (a + 1.0) + b * b)?
    };
    Some((0.5 * zh, im))
}

/// `clogf` in `mode`, both parts or `None` (the 384-bit route then runs).
#[inline(always)]
pub fn clogf(x: f32, y: f32, mode: Rounding) -> Option<((f32, f32), Status)> {
    let (re, im) = clog_core(x, y)?;
    Some(((round(re, CLOG_DELTA, mode)?, round(im, ATAN2_DELTA, mode)?), Status::flags(Flags::INEXACT)))
}

/// `clogf` to nearest for the in-place entry (hardware arithmetic on normal `double`s, the final
/// conversions inexact by the rounding test: inexact alone).
#[inline(always)]
pub fn clogf_nearest(x: f32, y: f32) -> Option<(f32, f32)> {
    let Some((re, im)) = clog_core(x, y) else {
        return clogf_axis(x, y);
    };
    Some((round(re, CLOG_DELTA, Rounding::Nearest)?, round(im, ATAN2_DELTA, Rounding::Nearest)?))
}

/// The normal finite nonzero `float` magnitudes (bit patterns without the sign).
const NORMAL: core::ops::Range<u32> = 0x0080_0000..0x7f80_0000;

/// `v` (positive) with the sign bit of `s`.
#[inline(always)]
fn with_sign(v: f32, s: f32) -> f32 {
    f32::from_bits(v.to_bits() | (s.to_bits() & 0x8000_0000))
}

/// `clogf` on an axis for the in-place entry (LIBM-CPLX-LOG-SQRT-4, D-10.74; the parts as
/// `crate::complex_fast`'s `double` axis): one part an exact zero, the other normal. The real part
/// is [`clog_core`]'s with `b = 0`: `s = a²` and `(a − 1)(a + 1)` are exact in `double` (a `float`
/// `a`), so its [`CLOG_DELTA`] bound holds with room; the argument `0`, `±π` or `±π/2` is the
/// correctly rounded constant, and the inexact real part's conversion raises inexact.
/// `clogf(1 ± 0i) = (+0, ±0)` is exact; the other `|v| = 1` cases decline (an exact real part).
#[inline(never)]
fn clogf_axis(x: f32, y: f32) -> Option<(f32, f32)> {
    let (ax, ay) = (x.to_bits() & 0x7fff_ffff, y.to_bits() & 0x7fff_ffff);
    let (v, im) = if ay == 0 && NORMAL.contains(&ax) {
        if x == 1.0 {
            return Some((0.0, y));
        }
        (ax, if x > 0.0 { y } else { with_sign(core::f32::consts::PI, y) })
    } else if ax == 0 && NORMAL.contains(&ay) {
        (ay, with_sign(core::f32::consts::FRAC_PI_2, y))
    } else {
        return None;
    };
    if v == 1.0f32.to_bits() {
        return None;
    }
    let a = f64::from(f32::from_bits(v));
    let s = a * a;
    let (zh, _) = if s < 0.5 {
        crate::fast64::quick_log::log_quick_parts(s)?
    } else {
        crate::fast64::quick_log::log1p_quick_parts((a - 1.0) * (a + 1.0))?
    };
    Some((round(0.5 * zh, CLOG_DELTA, Rounding::Nearest)?, im))
}

/// `csqrtf` on an axis for the in-place entry (LIBM-CPLX-LOG-SQRT-4, D-10.74; the parts as
/// `crate::complex_fast`'s `double` axis): one part an exact zero, the other normal. Each part is
/// the `double` hardware root of a `float` (or of half of one, exact) converted to `float`: the
/// correctly rounded `float` root (a root rounded to 53 bits, then to 24, rounds as once:
/// `53 >= 2·24 + 2`, Figueroa 1995), and the root or the conversion raises inexact exactly when
/// the `float` root is inexact.
#[inline(never)]
fn csqrtf_axis(x: f32, y: f32, sqrt: &impl Fn(f64) -> f64) -> Option<(f32, f32)> {
    let (ax, ay) = (x.to_bits() & 0x7fff_ffff, y.to_bits() & 0x7fff_ffff);
    if ay == 0 && NORMAL.contains(&ax) {
        let r = sqrt(f64::from(f32::from_bits(ax))) as f32;
        return Some(if x > 0.0 { (r, y) } else { (0.0, with_sign(r, y)) });
    }
    if ax == 0 && NORMAL.contains(&ay) {
        let t = sqrt(0.5 * f64::from(f32::from_bits(ay))) as f32;
        return Some((t, with_sign(t, y)));
    }
    None
}

/// `csqrt`'s parts in `double` (Kahan's form, `crate::complex_fast`), `sqrt` correctly rounded.
#[inline(always)]
fn csqrt_core(x: f32, y: f32, sqrt: impl Fn(f64) -> f64) -> Option<(f64, f64)> {
    let (a, b) = operands(x, y)?;
    let t = sqrt(0.5 * (a + sqrt(a * a + b * b)));
    let o = b / (2.0 * t);
    let (re, im) = if x >= 0.0 { (t, o) } else { (o, t) };
    Some((re, if y < 0.0 { -im } else { im }))
}

/// `csqrtf` in `mode`, both parts or `None`.
#[inline(always)]
pub fn csqrtf(x: f32, y: f32, mode: Rounding, sqrt: impl Fn(f64) -> f64) -> Option<((f32, f32), Status)> {
    let (re, im) = csqrt_core(x, y, sqrt)?;
    Some(((round(re, CSQRT_DELTA, mode)?, round(im, CSQRT_DELTA, mode)?), Status::flags(Flags::INEXACT)))
}

/// `csqrtf` to nearest for the in-place entry, `sqrt` the hardware root.
#[inline(always)]
pub fn csqrtf_nearest(x: f32, y: f32, sqrt: impl Fn(f64) -> f64) -> Option<(f32, f32)> {
    let Some((re, im)) = csqrt_core(x, y, &sqrt) else {
        return csqrtf_axis(x, y, &sqrt);
    };
    Some((round(re, CSQRT_DELTA, Rounding::Nearest)?, round(im, CSQRT_DELTA, Rounding::Nearest)?))
}
