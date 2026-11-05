//! Fast paths for `sinh`, `cosh`, `tanh`, `asinh`, `acosh`, `atanh` of `double` and `float`
//! (PERF-LIBM-HYPERBOLIC; Ziv's strategy as in [`crate::fast64`]). Each builds `f(x)` as a
//! double-double `y` from the fast exponential or logarithm kernels of [`crate::fast64`], with an
//! absolute bound `eps` on `|f(x)/2^m − y|`, then asks [`crate::fast64::settle`] (`double`) or
//! [`settle32`] (`float`) whether every value in `[y − eps, y + eps]` rounds the same in
//! round-to-nearest. If not, or outside the domains below, the entry returns `None` and the caller
//! runs the accurate path of [`crate::hyper`] unchanged: results and flags are that path's.
//!
//! Bounds. `ρ = exp_eps(y,tail)/|y|` (`2^-49·|tail| + 2^-70`, proof at `fast64::exp_kernel`) is the
//! relative error of `E = e^a`; `1/E` ([`recip`], `< 2^-100` relative) has the same `ρ`.
//! * `cosh = (E + 1/E)/2`, a sum of positives: `eps = ρ·S + 2^-90·S`, `S = E + 1/E`.
//! * `sinh = (E − 1/E)/2`: `eps = ρ·(E + 1/E) + 2^-90·E` (absolute, so cancellation is covered;
//!   `a >= 2^-8` keeps the relative error below `2^-61`). From `a >= 40`, `e^-a < 2^-57·E` is below
//!   the last place: `S = E`, and the dropped relative `2^-115` is inside `2^-90`.
//! * `tanh = (F − 1)/(F + 1)`, `F = e^(2a)`: `|dt/dF|·δF = 2ρF/(F+1)^2 <= ρ/2`: `eps = ρ/2 + 2^-90`.
//! * `asinh = ln z`, `z = a + √(a² + 1)`; `acosh = ln z`, `z = x + √(t² + 2t)`, `t = x − 1` exact;
//!   `atanh = ½·ln z`, `z = (1 + a)/(1 − a)` (both sums exact). `z = h + l` is within `2^-99` of
//!   exact; `ln z = ln h + l/h` (dropped `(l/h)²/2 < 2^-106`), `ln h` from `fast64::log_kernel`
//!   with `fast64::log_eps`, plus an absolute `2^-96` for `z`'s error and the division; the
//!   domains keep `ln z >= 2^-25`, so that is below `2^-71` relative.
#![allow(clippy::as_conversions, clippy::arithmetic_side_effects, clippy::cast_possible_truncation)] // `cvtsd2ss` is the `as f32`; exponents of bounded range

use crate::coeffs::tables as t;
use crate::dd::{self, fast_two_sum, two_prod, two_sum, Dd};
use crate::fast64;
use crate::status::Rounding;

/// 2^-90: the double-double operations' own errors, far above their real size.
const EX: f64 = f64::from_bits(0x3a50_0000_0000_0000);
/// 2^-96: `z`'s error and the `l/h` division, absolute, in `ln` units.
const EA: f64 = f64::from_bits(0x39f0_0000_0000_0000);
/// 2^-100 (`fast64`'s `SLACK`): covers the rounding of `y.lo ± eps`.
const SLACK: f64 = f64::from_bits(0x39b0_0000_0000_0000);

/// `(y, m, tail)` with `e^a = y·2^m` (`2^-60 <= a < 708`, [`fast64::exp_kernel`]'s contract).
#[inline(always)]
fn exp_pos(a: f64) -> Option<(Dd, i32, f64)> {
    let kf = dd::nearest_int(a * f64::from_bits(t::SCALES[0]));
    fast64::exp_kernel(fast64::reduce_exp(a, kf), dd::to_i32(kf))
}

/// `1/e` to `2^-100` relative: `q1 = 1/e.hi`, one correction from the exact residual.
#[inline(always)]
fn recip(e: Dd) -> Dd {
    let q1 = 1.0 / e.hi;
    let p = two_prod(e.hi, q1);
    let r = ((1.0 - p.hi) - p.lo) - e.lo * q1; // `1 − p.hi` exact (Sterbenz)
    fast_two_sum(q1, r * q1)
}

/// `n/d` to `2^-100` relative (`n.hi − p.hi` exact: `p.hi` is within `2^-52` of `n.hi`).
#[inline(always)]
fn div(n: Dd, d: Dd) -> Dd {
    let q1 = n.hi / d.hi;
    let p = two_prod(d.hi, q1);
    let r = ((n.hi - p.hi) - p.lo) + (n.lo - d.lo * q1);
    fast_two_sum(q1, r / d.hi)
}

/// `(y, eps, m)` with `cosh a = y·2^m`, `2^-30 <= a < 708`.
#[inline(always)]
fn cosh_core(a: f64) -> Option<(Dd, f64, i32)> {
    let (y, m, tail) = exp_pos(a)?;
    if a >= 40.0 {
        return Some((y, fast64::exp_eps(y, tail) + dd::abs(y.hi) * EX, m - 1));
    }
    let e = y.scale(dd::pow2(m));
    let s = e.add(recip(e));
    Some((s, fast64::exp_eps(s, tail) + dd::abs(s.hi) * EX, -1))
}

/// `sinh a = y·2^m`, `2^-8 <= a < 708` (module bounds).
#[inline(always)]
fn sinh_core(a: f64) -> Option<(Dd, f64, i32)> {
    let (y, m, tail) = exp_pos(a)?;
    if a >= 40.0 {
        return Some((y, fast64::exp_eps(y, tail) + dd::abs(y.hi) * EX, m - 1));
    }
    let e = y.scale(dd::pow2(m));
    let inv = recip(e);
    let s = e.sub(inv);
    let eps = fast64::exp_eps(Dd { hi: e.hi + inv.hi, lo: 0.0 }, tail) + e.hi * EX;
    Some((s, eps, -1))
}

/// `tanh a = y`, `2^-8 <= a < 22`.
#[inline(always)]
fn tanh_core(a: f64) -> Option<(Dd, f64, i32)> {
    let (y, m, tail) = exp_pos(2.0 * a)?;
    let f = y.scale(dd::pow2(m));
    let q = div(f.add_f64(-1.0), f.add_f64(1.0));
    Some((q, 0.5 * fast64::exp_eps(Dd::from(1.0), tail) + EX, 0))
}

/// `ln z` for `z = h + l`, `z > 1` and `ln z >= 2^-25`: `(y, eps)`.
#[inline(always)]
fn ln_z(z: Dd) -> Option<(Dd, f64)> {
    let (y0, tail) = fast64::log_kernel(z.hi)?;
    let y = y0.add_f64(z.lo / z.hi);
    Some((y, fast64::log_eps(y, tail) + EA))
}

/// `asinh a`, `2^-25 <= a < 2^50`.
#[inline(always)]
fn asinh_core(a: f64) -> Option<(Dd, f64, i32)> {
    let s = dd::sqrt(two_prod(a, a).add_f64(1.0));
    let (y, eps) = ln_z(s.add_f64(a))?;
    Some((y, eps, 0))
}

/// `acosh x`, `1 + 2^-30 <= x < 2^50`.
#[inline(always)]
fn acosh_core(x: f64) -> Option<(Dd, f64, i32)> {
    let t1 = x - 1.0;
    let s = dd::sqrt(two_prod(t1, t1).add_f64(2.0 * t1));
    let (y, eps) = ln_z(s.add_f64(x))?;
    Some((y, eps, 0))
}

/// `atanh a = ½·ln z`, `2^-25 <= a < 1`.
#[inline(always)]
fn atanh_core(a: f64) -> Option<(Dd, f64, i32)> {
    let z = div(two_sum(1.0, a), two_sum(1.0, -a));
    let (y, eps) = ln_z(z)?;
    Some((y, eps, -1))
}

/// The `float` rounding test: `y·2^m` rounded to nearest `float` when both ends of
/// `y.hi + (y.lo ± eps)` round to one `float` and neither is a `float` midpoint (a double that is not
/// a midpoint is at least one last place from every midpoint, and the exact end is within half a
/// last place of its double: no midpoint lies between the exact end and its double).
#[inline(always)]
fn settle32(y: Dd, eps: f64, m: i32) -> Option<f32> {
    let e = eps + dd::abs(y.hi) * SLACK;
    let (a, b) = (y.hi + (y.lo - e), y.hi + (y.lo + e));
    let mid = |v: f64| v.to_bits() & 0x1fff_ffff == 0x1000_0000;
    if mid(a) || mid(b) {
        return None;
    }
    let p = dd::pow2(m);
    let (fa, fb) = ((a * p) as f32, (b * p) as f32);
    if fa.to_bits() == fb.to_bits() { Some(fa) } else { None }
}

macro_rules! entries {
    ($($name:ident, $namef:ident, $core:ident, $abs:expr, $neg:expr, $lo64:expr, $lo32:expr, $hi64:expr, $hi32:expr;)*) => {$(
        /// The in-place round-to-nearest entry for `double` (`None`: run the accurate path).
        #[inline(always)]
        pub fn $name(x: f64) -> Option<f64> {
            let a = if $abs { dd::abs(x) } else { x };
            if !($lo64..$hi64).contains(&a) {
                return None;
            }
            let (y, eps, m) = $core(a)?;
            let v = fast64::settle(y, eps, m, Rounding::Nearest)?.0;
            Some(if $neg && x < 0.0 { -v } else { v })
        }
        /// The in-place round-to-nearest entry for `float` (`None`: run the accurate path).
        #[inline(always)]
        pub fn $namef(x: f32) -> Option<f32> {
            let x = f64::from(x);
            let a = if $abs { dd::abs(x) } else { x };
            if !($lo32..$hi32).contains(&a) {
                return None;
            }
            let (y, eps, m) = $core(a)?;
            let v = settle32(y, eps, m)?;
            Some(if $neg && x < 0.0 { -v } else { v })
        }
    )*};
}

const TWO_M8: f64 = f64::from_bits(0x3f70_0000_0000_0000);
const TWO_M12: f64 = f64::from_bits(0x3f30_0000_0000_0000);
const TWO_M25: f64 = f64::from_bits(0x3e60_0000_0000_0000);
const TWO_M30: f64 = f64::from_bits(0x3e10_0000_0000_0000);
const ONE_PLUS_2M30: f64 = 1.0 + TWO_M30;
const TWO_P50: f64 = f64::from_bits(0x4310_0000_0000_0000);

entries! {
    sinh_nearest, sinhf_nearest, sinh_core, true, true, TWO_M8, TWO_M12, 708.0, 88.0;
    cosh_nearest, coshf_nearest, cosh_core, true, false, TWO_M30, TWO_M12, 708.0, 88.0;
    tanh_nearest, tanhf_nearest, tanh_core, true, true, TWO_M8, TWO_M12, 22.0, 9.0;
    asinh_nearest, asinhf_nearest, asinh_core, true, true, TWO_M25, TWO_M12, TWO_P50, TWO_P50;
    acosh_nearest, acoshf_nearest, acosh_core, false, false, ONE_PLUS_2M30, 1.000_000_1, TWO_P50, TWO_P50;
    atanh_nearest, atanhf_nearest, atanh_core, true, true, TWO_M25, TWO_M12, 1.0, 1.0;
}
