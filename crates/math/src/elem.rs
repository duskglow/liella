//! What the elementary functions of `float` and `double` share (spec §10.3, D-10.13): the formats
//! they are implemented for, special-value handling, and the final rounding.
//!
//! Each function computes `f(x)` in double-double arithmetic ([`crate::dd`]) to a relative error
//! of about 2^-100 and rounds that approximation once into the result format in the caller's
//! rounding direction. For `float` this is correctly rounded (the approximation is far closer
//! than any binary32 rounding boundary needs; the exhaustive sweeps check it); for `double` it is
//! correctly rounded except where the exact value lies within about 2^-100 of a rounding
//! boundary, which random inputs essentially never meet.

use crate::dd::{self, Dd, Residual};
use crate::float::{self, Exact, Float, Kind, Num};
use crate::status::{Rounding, Status};

/// The formats computed in double precision: `float` and `double` (every value is an `f64`).
pub trait Small: Float {
    /// The value as an `f64` (exact).
    fn widen(self) -> f64;
    /// An `f64` result in this format; used only where the value is already representable
    /// (the `double` fast paths call it only for `double`).
    fn narrow(v: f64) -> Self;
}

impl Small for f32 {
    fn widen(self) -> f64 {
        f64::from(self)
    }
    fn narrow(v: f64) -> f32 {
        #[allow(clippy::as_conversions, clippy::cast_possible_truncation)] // callers pass representable values only
        let r = v as f32;
        r
    }
}

impl Small for f64 {
    fn widen(self) -> f64 {
        self
    }
    fn narrow(v: f64) -> f64 {
        v
    }
}

/// Round an approximation `v · 2^scale`; `ERANGE` for an overflow to ∞ or an underflow to 0.
pub fn finish<F: Small>(v: Dd, scale: i32, res: Residual, mode: Rounding) -> (F, Status) {
    let (r, flags) = dd::round::<F>(v, scale, res, mode);
    (r, float::range_errno(r, Status::flags(flags)))
}

/// An exactly representable result (an `f64` that fits `F`).
pub fn exact<F: Small>(v: f64) -> (F, Status) {
    if v.is_infinite() {
        return (float::inf(v < 0.0), Status::OK);
    }
    finish(Dd::from(v), 0, Residual::Exact, Rounding::Nearest)
}

/// A NaN operand's result: the quieted NaN, invalid for a signalling one.
pub fn nan_in<F: Float>(n: &Num) -> Option<(F, Status)> {
    match n.kind {
        Kind::QNan => Some((float::quiet(n), Status::OK)),
        Kind::SNan => Some((float::quiet(n), Status::INVALID)),
        Kind::Invalid => Some((float::default_nan(), Status::INVALID)),
        _ => None,
    }
}

/// A domain error: the default NaN, invalid and `EDOM`.
pub fn domain<F: Float>() -> (F, Status) {
    (float::default_nan(), Status::DOMAIN)
}

/// A domain error with a positive quiet NaN (glibc's `asin`, `acos` and `log10`; the others give
/// the default NaN, which has the sign bit set).
pub fn domain_pos<F: Float>() -> (F, Status) {
    (float::nan(false, true, 0), Status::DOMAIN)
}

/// A pole: ±∞, division by zero and `ERANGE`.
pub fn pole<F: Float>(neg: bool) -> (F, Status) {
    (float::inf(neg), Status::POLE)
}

/// A result far beyond the format's range, as rounding it gives: ∞ or the largest finite
/// value, overflow and inexact, `ERANGE` for ∞.
pub fn overflow<F: Float>(neg: bool, mode: Rounding) -> (F, Status) {
    let (r, flags) = float::round::<F>(Exact { neg, sig: 1, exp: 1 << 20, sticky: false }, mode);
    (r, float::range_errno(r, Status::flags(flags)))
}

/// A nonzero result far below the format's smallest subnormal: ±0 or the smallest subnormal,
/// underflow and inexact, `ERANGE` for 0.
pub fn underflow<F: Float>(neg: bool, mode: Rounding) -> (F, Status) {
    let (r, flags) = float::round::<F>(Exact { neg, sig: 0, exp: 0, sticky: true }, mode);
    (r, float::range_errno(r, Status::flags(flags)))
}

/// `x + δ` for a `δ` far below `x`'s last place, of the sign `above` gives: the result of
/// `f(x) = x(1 + O(x²))` for tiny `x` (`sin`, `tan`, `atan`, `expm1`, ...).
pub fn near<F: Small>(x: f64, above: bool, mode: Rounding) -> (F, Status) {
    finish(Dd::from(x), 0, if above { Residual::Above } else { Residual::Below }, mode)
}
