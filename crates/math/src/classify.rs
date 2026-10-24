//! Classification, comparison and the total order (spec §10.1).
//!
//! The x87 format has encodings IEEE does not define. Classification follows the value an
//! encoding denotes where it denotes one (a pseudo-denormal is a normal-range number) and
//! treats the others (unnormals, pseudo-infinities, pseudo-NaNs) as signalling NaNs, which is
//! how the x87 unit treats them as operands (an invalid-operand exception) and how glibc
//! classifies them (`spec/behaviour/libm-exact.glibc.txt.gz`).

#![allow(clippy::as_conversions, clippy::arithmetic_side_effects)] // encodings of bounded width

use crate::float::{self, Float, Kind};
use crate::status::Status;

/// `FP_NAN`.
pub const FP_NAN: i32 = 0;
/// `FP_INFINITE`.
pub const FP_INFINITE: i32 = 1;
/// `FP_ZERO`.
pub const FP_ZERO: i32 = 2;
/// `FP_SUBNORMAL`.
pub const FP_SUBNORMAL: i32 = 3;
/// `FP_NORMAL`.
pub const FP_NORMAL: i32 = 4;

/// The class of `x` (`fpclassify`).
pub fn fpclassify<F: Float>(x: F) -> i32 {
    let n = float::decode_opaque(x);
    match n.kind {
        Kind::Zero => FP_ZERO,
        Kind::Inf => FP_INFINITE,
        Kind::QNan | Kind::SNan | Kind::Invalid => FP_NAN,
        Kind::Finite if n.top() < float::emin::<F>() => FP_SUBNORMAL,
        Kind::Finite => FP_NORMAL,
    }
}

/// `x` is a NaN (or an x87 encoding that is not a number).
pub fn is_nan<F: Float>(x: F) -> bool {
    fpclassify(x) == FP_NAN
}

/// `isinf`: 1 for +∞, −1 for −∞, 0 otherwise (the sign is a GNU extension, D-10.4).
pub fn isinf<F: Float>(x: F) -> i32 {
    let n = float::decode_opaque(x);
    match n.kind {
        Kind::Inf if n.neg => -1,
        Kind::Inf => 1,
        _ => 0,
    }
}

/// `x` is finite (zero, subnormal or normal).
pub fn is_finite<F: Float>(x: F) -> bool {
    matches!(float::decode_opaque(x).kind, Kind::Zero | Kind::Finite)
}

/// The sign bit, for every encoding including zeros and NaNs.
pub fn signbit<F: Float>(x: F) -> bool {
    x.to_bits() & float::sign_bit::<F>() != 0
}

/// `x` is a signalling NaN (or an x87 encoding that is not a number, which signals as an
/// operand).
pub fn is_signaling<F: Float>(x: F) -> bool {
    matches!(float::decode_opaque(x).kind, Kind::SNan | Kind::Invalid)
}

/// `x` is canonically encoded. Only the x87 format has other encodings: pseudo-denormals and
/// the encodings that are not numbers.
pub fn is_canonical<F: Float>(x: F) -> bool {
    let n = float::decode_opaque(x);
    match n.kind {
        Kind::Invalid => false,
        // A pseudo-denormal: a zero exponent field with the explicit leading bit set.
        Kind::Finite if F::EXPLICIT => !(x.to_bits() >> float::frac_bits::<F>() & 0x7fff == 0 && n.sig >> (F::PREC - 1) != 0),
        _ => true,
    }
}

/// `iseqsig`: equality that signals on any NaN operand (`FE_INVALID` and `EDOM`).
pub fn iseqsig<F: Float>(x: F, y: F) -> (bool, Status) {
    if is_nan(x) || is_nan(y) {
        return (false, Status::DOMAIN);
    }
    (eq(x, y), Status::OK)
}

/// Numeric equality of non-NaN values (+0 equals −0).
fn eq<F: Float>(x: F, y: F) -> bool {
    let (a, b) = (float::decode_opaque(x), float::decode(y));
    match (a.kind, b.kind) {
        (Kind::Zero, Kind::Zero) => true,
        (Kind::Inf, Kind::Inf) => a.neg == b.neg,
        (Kind::Finite, Kind::Finite) => a.neg == b.neg && same_value(a.sig, a.exp, b.sig, b.exp),
        _ => false,
    }
}

/// `s1 · 2^e1 == s2 · 2^e2` for nonzero significands.
fn same_value(s1: u128, e1: i32, s2: u128, e2: i32) -> bool {
    let (t1, t2) = (s1.trailing_zeros() as i32, s2.trailing_zeros() as i32);
    s1 >> t1 == s2 >> t2 && e1 + t1 == e2 + t2
}

/// `totalorder(x, y)`: `x` precedes or equals `y` in the IEEE total order, which compares
/// encodings as sign and magnitude: −NaN < −∞ < ... < −0 < +0 < ... < +∞ < +NaN, NaNs by payload
/// (signalling below quiet among positive NaNs), as IEEE 754's `totalOrder` defines.
pub fn totalorder<F: Float>(x: F, y: F) -> bool {
    let s = float::sign_bit::<F>();
    let (bx, by) = (x.to_bits(), y.to_bits());
    let (mx, my) = (bx & (s - 1), by & (s - 1));
    match (bx & s != 0, by & s != 0) {
        (true, false) => true,
        (false, true) => false,
        (false, false) => mx <= my,
        (true, true) => mx >= my,
    }
}

/// `totalordermag(x, y)`: the total order of the magnitudes.
pub fn totalordermag<F: Float>(x: F, y: F) -> bool {
    totalorder(float::with_sign(x, false), float::with_sign(y, false))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions)]
mod tests {
    use super::*;
    use crate::float::F80;

    #[test]
    fn classes() {
        assert_eq!(fpclassify(0.0f64), FP_ZERO);
        assert_eq!(fpclassify(5e-324f64), FP_SUBNORMAL);
        assert_eq!(fpclassify(1.0f32), FP_NORMAL);
        assert_eq!(fpclassify(f64::NEG_INFINITY), FP_INFINITE);
        assert_eq!(isinf(f64::NEG_INFINITY), -1);
        assert_eq!(fpclassify(f32::NAN), FP_NAN);
        // x87: a pseudo-denormal is a normal-range value; an unnormal is not a number.
        assert_eq!(fpclassify(F80((1u128 << 63) | 1)), FP_NORMAL);
        assert!(!is_canonical(F80((1u128 << 63) | 1)));
        assert!(is_canonical(F80(1)));
        assert!(is_canonical(F80((0x3fffu128 << 64) | (1u128 << 63))));
        assert_eq!(fpclassify(F80(0x3fffu128 << 64 | 1u128 << 62)), FP_NAN);
        assert!(is_signaling(F80(0x3fffu128 << 64 | 1u128 << 62)));
    }

    #[test]
    fn total_order() {
        let order = [f64::from_bits(0xfff8_0000_0000_0001), f64::from_bits(0xfff4_0000_0000_0000), f64::NEG_INFINITY, -1.0, -0.0, 0.0, 5e-324, 1.0,
                     f64::INFINITY, f64::from_bits(0x7ff4_0000_0000_0000), f64::from_bits(0x7ff8_0000_0000_0000), f64::from_bits(0x7ff8_0000_0000_0001)];
        for (i, a) in order.iter().enumerate() {
            for (j, b) in order.iter().enumerate() {
                assert_eq!(totalorder(*a, *b), i <= j, "{a:e} {b:e}");
            }
        }
        assert!(totalordermag(-1.0f64, 2.0));
        assert!(!totalordermag(-3.0f32, 2.0));
    }
}
