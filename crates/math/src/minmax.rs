//! Maximum, minimum and positive difference (spec §10.2): `fmax`/`fmin` (C: a quiet NaN is
//! missing data), the IEEE 754-2019 operations `fmaximum`/`fminimum` (NaN-propagating),
//! `fmaximum_num`/`fminimum_num` (number-preferring) and their `_mag` forms, TS 18661-1's
//! `fmaxmag`/`fminmag`, and `fdim`. −0 orders below +0 in every one of them.

use core::cmp::Ordering;

use crate::float::{self, Float, Kind, Num};
use crate::fused::{self, nan_operands, nan_operands_by, Pick};
use crate::manip::compare;
use crate::status::{Rounding, Status};

/// How an operation treats NaN operands.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Nans {
    /// C's `fmax`/`fmin` and TS 18661-1's `fmaxmag`/`fminmag`: a quiet NaN is missing data; a
    /// signalling NaN raises invalid and gives a NaN.
    Missing,
    /// IEEE `maximum`/`minimum`: any NaN gives a NaN.
    Propagate,
    /// IEEE `maximumNumber`/`minimumNumber`: a NaN of either kind is missing data (a signalling
    /// one still raises invalid).
    Number,
}

/// A maximum or minimum.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Select {
    /// The maximum (else the minimum).
    pub max: bool,
    /// Compare magnitudes first (ties decided by the values).
    pub mag: bool,
    /// NaN treatment.
    pub nans: Nans,
}

/// `fmax`.
pub const FMAX: Select = Select { max: true, mag: false, nans: Nans::Missing };
/// `fmin`.
pub const FMIN: Select = Select { max: false, mag: false, nans: Nans::Missing };
/// `fmaximum`.
pub const FMAXIMUM: Select = Select { max: true, mag: false, nans: Nans::Propagate };
/// `fminimum`.
pub const FMINIMUM: Select = Select { max: false, mag: false, nans: Nans::Propagate };
/// `fmaximum_num`.
pub const FMAXIMUM_NUM: Select = Select { max: true, mag: false, nans: Nans::Number };
/// `fminimum_num`.
pub const FMINIMUM_NUM: Select = Select { max: false, mag: false, nans: Nans::Number };
/// `fmaximum_mag`.
pub const FMAXIMUM_MAG: Select = Select { max: true, mag: true, nans: Nans::Propagate };
/// `fminimum_mag`.
pub const FMINIMUM_MAG: Select = Select { max: false, mag: true, nans: Nans::Propagate };
/// `fmaximum_mag_num`.
pub const FMAXIMUM_MAG_NUM: Select = Select { max: true, mag: true, nans: Nans::Number };
/// `fminimum_mag_num`.
pub const FMINIMUM_MAG_NUM: Select = Select { max: false, mag: true, nans: Nans::Number };
/// `fmaxmag` (TS 18661-1).
pub const FMAXMAG: Select = Select { max: true, mag: true, nans: Nans::Missing };
/// `fminmag` (TS 18661-1).
pub const FMINMAG: Select = Select { max: false, mag: true, nans: Nans::Missing };

/// The order of non-NaN values with −0 below +0.
fn signed_order(a: &Num, b: &Num) -> Ordering {
    match compare(a, b) {
        Some(Ordering::Equal) if a.kind == Kind::Zero && b.kind == Kind::Zero => b.neg.cmp(&a.neg),
        Some(o) => o,
        None => Ordering::Equal,
    }
}

/// The selected operand.
pub fn select<F: Float>(x: F, y: F, s: Select) -> (F, Status) {
    let (a, b) = (float::decode(x), float::decode(y));
    // An invalid x87 encoding is not a number for any selection, `_num` forms included: the
    // default NaN with invalid (D-10.20, ruling #902, X87-INVALID-ENC-SWEEP).
    if F::PREC == 64 && (a.kind == Kind::Invalid || b.kind == Kind::Invalid) {
        return (float::default_nan(), Status::INVALID);
    }
    let nan_a = a.is_nan() || a.kind == Kind::Invalid;
    let nan_b = b.is_nan() || b.kind == Kind::Invalid;
    if nan_a || nan_b {
        let signalling = [a.kind, b.kind].iter().any(|k| matches!(k, Kind::SNan | Kind::Invalid));
        let st = if signalling { Status::INVALID } else { Status::OK };
        let keep_number = match s.nans {
            Nans::Missing => !signalling,
            Nans::Propagate => false,
            Nans::Number => true,
        };
        if keep_number && !(nan_a && nan_b) {
            return (if nan_a { y } else { x }, st);
        }
        // Which NaN glibc returns for C's fmax/fmin family: binary128 takes a quiet operand over
        // a signalling one, then the first; x87 follows the x87 unit's rule, except that x87
        // fmaxmag/fminmag take the first of two quiet NaNs (probed, X87-NAN-RULE-AUDIT-2).
        let pick = match (s.nans, s.mag, F::PREC) {
            (Nans::Missing, _, 113) | (Nans::Missing, true, 64) if F::PREC == 113 || !signalling => Pick::QuietFirst,
            _ => Pick::Format,
        };
        return nan_operands_by::<F, F>(&[a, b], pick).unwrap_or((float::default_nan(), st));
    }
    // C's fmax/fmin family does not order the zeros: equal operands give y. IEEE 754-2019's
    // operations put −0 below +0.
    let order = |p: &Num, q: &Num| if s.nans == Nans::Missing { compare(p, q).unwrap_or(Ordering::Equal) } else { signed_order(p, q) };
    let ord = if s.mag {
        let (ma, mb) = (Num { neg: false, ..a }, Num { neg: false, ..b });
        match compare(&ma, &mb) {
            Some(Ordering::Equal) | None => order(&a, &b),
            Some(o) => o,
        }
    } else {
        order(&a, &b)
    };
    // Which of two equal operands glibc returns there (only zeros of opposite signs show it):
    // `x` for binary128 `fmax`/`fmin` and x87 `fmin`, `y` otherwise.
    let tie_x = !s.mag && (F::PREC == 113 || (F::PREC == 64 && !s.max));
    let first = match (s.max, s.nans == Nans::Missing) {
        (true, true) => ord == Ordering::Greater || (tie_x && ord == Ordering::Equal),
        (false, true) => ord == Ordering::Less || (tie_x && ord == Ordering::Equal),
        (true, false) => ord != Ordering::Less,
        (false, false) => ord != Ordering::Greater,
    };
    (if first { x } else { y }, Status::OK)
}

/// `fdim(x, y)`: `x − y` if `x > y`, else +0. The subtraction rounds in `mode`; overflow reports
/// `ERANGE`.
pub fn fdim<F: Float>(x: F, y: F, mode: Rounding) -> (F, Status) {
    let (a, b) = (float::decode(x), float::decode(y));
    if let Some(r) = nan_operands::<F, F>(&[a, b]) {
        return r;
    }
    match compare(&a, &b) {
        Some(Ordering::Greater) => fused::sub::<F, F>(x, y, mode),
        _ => (float::zero(false), Status::OK),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp, clippy::arithmetic_side_effects, clippy::as_conversions)]
mod tests {
    use super::*;

    const MAXIMUM: Select = FMAXIMUM;
    const MINMAG_NUM: Select = FMINIMUM_MAG_NUM;

    #[test]
    fn selection() {
        assert_eq!(select(f64::NAN, 1.0, FMAX).0, 1.0);
        assert!(select(f64::NAN, 1.0, MAXIMUM).0.is_nan());
        assert_eq!(select(-0.0f64, 0.0, FMAX).0.to_bits(), 0);
        assert_eq!(select(-0.0f64, 0.0, FMIN).0.to_bits(), 0); // C: equal operands give y
        assert_eq!(select(-0.0f64, 0.0, FMINIMUM).0.to_bits(), 1 << 63);
        assert_eq!(select(-3.0f64, 2.0, MINMAG_NUM).0, 2.0);
        assert_eq!(select(-2.0f64, 2.0, MINMAG_NUM).0, -2.0);
        let snan = f64::from_bits(0x7ff4_0000_0000_0000);
        assert_eq!(select(snan, 1.0, FMAX).1, Status::INVALID);
        assert_eq!(select(snan, 1.0, MINMAG_NUM).0, 1.0);
        assert_eq!(fdim(1.0f64, 3.0, Rounding::Nearest).0, 0.0);
        assert_eq!(fdim(3.0f64, 1.0, Rounding::Nearest).0, 2.0);
    }
}
