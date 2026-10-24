//! One entry per elementary function over every format that implements it (D-10.19): the
//! translation's single body per family (`s10_math.tsv`) calls `Real::exp` and the like, which
//! select the double-double implementation for `float` and `double` (`exp_log`, `pow`, `trig`,
//! `hyper`, D-10.13) and the 384-bit one for x87 `long double` and `_Float128` (`wide`).

use crate::float::{Float, F128, F80};
use crate::status::{Rounding, Status};
use crate::{exp_log, fast32, hyper, pow, trig, wide};

/// A format with the elementary functions of spec §10.3.
pub trait Real: Float {
    /// e^x.
    fn exp(self, mode: Rounding) -> (Self, Status);
    /// 2^x.
    fn exp2(self, mode: Rounding) -> (Self, Status);
    /// 10^x.
    fn exp10(self, mode: Rounding) -> (Self, Status);
    /// e^x − 1.
    fn expm1(self, mode: Rounding) -> (Self, Status);
    /// ln x.
    fn log(self, mode: Rounding) -> (Self, Status);
    /// log₂ x.
    fn log2(self, mode: Rounding) -> (Self, Status);
    /// log₁₀ x.
    fn log10(self, mode: Rounding) -> (Self, Status);
    /// ln(1 + x).
    fn log1p(self, mode: Rounding) -> (Self, Status);
    /// x^y.
    fn pow(self, y: Self, mode: Rounding) -> (Self, Status);
    /// ∛x.
    fn cbrt(self, mode: Rounding) -> (Self, Status);
    /// sin x.
    fn sin(self, mode: Rounding) -> (Self, Status);
    /// cos x.
    fn cos(self, mode: Rounding) -> (Self, Status);
    /// tan x.
    fn tan(self, mode: Rounding) -> (Self, Status);
    /// (sin x, cos x).
    fn sincos(self, mode: Rounding) -> ((Self, Self), Status);
    /// asin x.
    fn asin(self, mode: Rounding) -> (Self, Status);
    /// acos x.
    fn acos(self, mode: Rounding) -> (Self, Status);
    /// atan x.
    fn atan(self, mode: Rounding) -> (Self, Status);
    /// atan2(self, x): the angle of (x, self).
    fn atan2(self, x: Self, mode: Rounding) -> (Self, Status);
    /// sinh x.
    fn sinh(self, mode: Rounding) -> (Self, Status);
    /// cosh x.
    fn cosh(self, mode: Rounding) -> (Self, Status);
    /// tanh x.
    fn tanh(self, mode: Rounding) -> (Self, Status);
    /// asinh x.
    fn asinh(self, mode: Rounding) -> (Self, Status);
    /// acosh x.
    fn acosh(self, mode: Rounding) -> (Self, Status);
    /// atanh x.
    fn atanh(self, mode: Rounding) -> (Self, Status);
}

/// The methods of [`Real`], each forwarding to the module that implements the function.
macro_rules! real {
    ($el:ident, $pw:ident, $tr:ident, $hy:ident) => {
        fn exp(self, mode: Rounding) -> (Self, Status) {
            $el::exp(self, mode)
        }
        fn exp2(self, mode: Rounding) -> (Self, Status) {
            $el::exp2(self, mode)
        }
        fn exp10(self, mode: Rounding) -> (Self, Status) {
            $el::exp10(self, mode)
        }
        fn expm1(self, mode: Rounding) -> (Self, Status) {
            $el::expm1(self, mode)
        }
        fn log(self, mode: Rounding) -> (Self, Status) {
            $el::log(self, mode)
        }
        fn log2(self, mode: Rounding) -> (Self, Status) {
            $el::log2(self, mode)
        }
        fn log10(self, mode: Rounding) -> (Self, Status) {
            $el::log10(self, mode)
        }
        fn log1p(self, mode: Rounding) -> (Self, Status) {
            $el::log1p(self, mode)
        }
        fn pow(self, y: Self, mode: Rounding) -> (Self, Status) {
            $pw::pow(self, y, mode)
        }
        fn cbrt(self, mode: Rounding) -> (Self, Status) {
            $pw::cbrt(self, mode)
        }
        fn sin(self, mode: Rounding) -> (Self, Status) {
            $tr::sin(self, mode)
        }
        fn cos(self, mode: Rounding) -> (Self, Status) {
            $tr::cos(self, mode)
        }
        fn tan(self, mode: Rounding) -> (Self, Status) {
            $tr::tan(self, mode)
        }
        fn sincos(self, mode: Rounding) -> ((Self, Self), Status) {
            $tr::sincos(self, mode)
        }
        fn asin(self, mode: Rounding) -> (Self, Status) {
            $tr::asin(self, mode)
        }
        fn acos(self, mode: Rounding) -> (Self, Status) {
            $tr::acos(self, mode)
        }
        fn atan(self, mode: Rounding) -> (Self, Status) {
            $tr::atan(self, mode)
        }
        fn atan2(self, x: Self, mode: Rounding) -> (Self, Status) {
            $tr::atan2(self, x, mode)
        }
        fn sinh(self, mode: Rounding) -> (Self, Status) {
            $hy::sinh(self, mode)
        }
        fn cosh(self, mode: Rounding) -> (Self, Status) {
            $hy::cosh(self, mode)
        }
        fn tanh(self, mode: Rounding) -> (Self, Status) {
            $hy::tanh(self, mode)
        }
        fn asinh(self, mode: Rounding) -> (Self, Status) {
            $hy::asinh(self, mode)
        }
        fn acosh(self, mode: Rounding) -> (Self, Status) {
            $hy::acosh(self, mode)
        }
        fn atanh(self, mode: Rounding) -> (Self, Status) {
            $hy::atanh(self, mode)
        }
    };
}

impl Real for f32 {
    real!(fast32, fast32, fast32, hyper); // fast32 has cbrt and sin/cos/tan; it re-exports pow::pow and the inverse trig
}

impl Real for f64 {
    real!(exp_log, pow, trig, hyper);
}

impl Real for F80 {
    real!(wide, wide, wide, wide);
}

impl Real for F128 {
    real!(wide, wide, wide, wide);
}
