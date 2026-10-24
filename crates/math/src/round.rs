//! Rounding to integral values and to integers (spec §10.2): `ceil`, `floor`, `trunc`, `round`,
//! `roundeven`, `rint`, `nearbyint`, `lrint`/`llrint`, `lround`/`llround`, the `fromfp` family and
//! `modf`. All are exact: the only rounding is the one requested.

#![allow(clippy::as_conversions, clippy::arithmetic_side_effects)] // integer parts of bounded width

use crate::float::{self, Float, Kind, Num};
use crate::status::{Flags, Rounding, Status};

/// A direction for rounding to an integer.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Dir {
    /// Toward +∞ (`ceil`, `FP_INT_UPWARD`).
    Up,
    /// Toward −∞ (`floor`, `FP_INT_DOWNWARD`).
    Down,
    /// Toward zero (`trunc`, `FP_INT_TOWARDZERO`).
    Zero,
    /// To nearest, ties away from zero (`round`, `FP_INT_TONEARESTFROMZERO`).
    Away,
    /// To nearest, ties to even (`roundeven`, `FP_INT_TONEAREST`).
    Even,
}

impl Dir {
    /// The direction of a rounding mode (`rint`, `lrint`).
    pub const fn of(mode: Rounding) -> Dir {
        match mode {
            Rounding::Nearest => Dir::Even,
            Rounding::Up => Dir::Up,
            Rounding::Down => Dir::Down,
            Rounding::Zero => Dir::Zero,
        }
    }

    /// The direction named by a C `FP_INT_*` value; other values round to nearest, ties to even
    /// (`FP_INT_TONEAREST`), so no argument has an undefined result.
    pub const fn from_c(v: i32) -> Dir {
        match v {
            0 => Dir::Up,
            1 => Dir::Down,
            2 => Dir::Zero,
            3 => Dir::Away,
            _ => Dir::Even,
        }
    }
}

/// The integer nearest a finite `n` in direction `dir`, as magnitude, and whether it differs from
/// `n`.
fn integer_part(n: &Num, dir: Dir) -> (u128, bool) {
    if n.kind != Kind::Finite {
        return (0, false);
    }
    if n.exp >= 0 {
        // Already an integer (callers handle magnitudes above 2^127 before asking for it).
        return (n.sig.checked_shl(n.exp as u32).unwrap_or(u128::MAX), false);
    }
    let d = (-n.exp) as u32;
    let (kept, rem, half) = if d >= 128 {
        (0, n.sig, if d == 128 { 1u128 << 127 } else { u128::MAX })
    } else {
        (n.sig >> d, n.sig & ((1u128 << d) - 1), 1u128 << (d - 1))
    };
    let inexact = rem != 0;
    // With d > 128 the remainder is below half (a significand has at most 113 bits).
    let above_half = d <= 128 && rem > half;
    let tie = d <= 128 && rem == half;
    let up = match dir {
        Dir::Up => inexact && !n.neg,
        Dir::Down => inexact && n.neg,
        Dir::Zero => false,
        Dir::Away => above_half || tie,
        Dir::Even => above_half || (tie && kept & 1 == 1),
    };
    (kept + u128::from(up), inexact)
}

/// `x` rounded to an integral value in direction `dir`; `inexact` raises `FE_INEXACT` when the
/// result differs (`rint`), otherwise nothing is raised (`ceil` ... `roundeven`, `nearbyint`).
/// A signalling NaN raises invalid and is quieted.
pub fn integral<F: Float>(x: F, dir: Dir, inexact: bool) -> (F, Status) {
    let n = float::decode(x);
    match n.kind {
        Kind::Zero | Kind::Inf | Kind::QNan => (x, Status::OK),
        Kind::SNan => (float::quiet(&n), Status::INVALID),
        Kind::Invalid => (float::default_nan(), Status::INVALID),
        Kind::Finite if n.exp >= 0 => (x, Status::OK),
        Kind::Finite => {
            let (k, lost) = integer_part(&n, dir);
            let st = if inexact && lost { Status::flags(Flags::INEXACT) } else { Status::OK };
            (float::pack(n.neg, k, 0), st)
        }
    }
}

/// `ceil`.
pub fn ceil<F: Float>(x: F) -> (F, Status) {
    integral(x, Dir::Up, false)
}

/// `floor`.
pub fn floor<F: Float>(x: F) -> (F, Status) {
    integral(x, Dir::Down, false)
}

/// `trunc`.
pub fn trunc<F: Float>(x: F) -> (F, Status) {
    integral(x, Dir::Zero, false)
}

/// `round`: to nearest, ties away from zero.
pub fn round<F: Float>(x: F) -> (F, Status) {
    integral(x, Dir::Away, false)
}

/// `roundeven`: to nearest, ties to even.
pub fn roundeven<F: Float>(x: F) -> (F, Status) {
    integral(x, Dir::Even, false)
}

/// `rint`: in the current rounding direction, raising inexact.
pub fn rint<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    integral(x, Dir::of(mode), true)
}

/// `nearbyint`: in the current rounding direction, raising nothing.
pub fn nearbyint<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    integral(x, Dir::of(mode), false)
}

/// The signed integer of `x` rounded in `dir`, if within `[lo, hi]`.
fn to_int<F: Float>(x: F, dir: Dir, lo: i128, hi: i128) -> Option<(i128, bool)> {
    let n = float::decode(x);
    match n.kind {
        Kind::Zero => Some((0, false)),
        Kind::Finite if n.top() <= 126 => {
            let (k, lost) = integer_part(&n, dir);
            let v = if n.neg { -(k as i128) } else { k as i128 };
            (lo..=hi).contains(&v).then_some((v, lost))
        }
        _ => None,
    }
}

/// The result of `lrint`/`lround` out of range. The SSE and x87 conversions give `LONG_MIN` (the
/// "integer indefinite"); glibc's software binary128 saturates by the sign bit.
fn out_of_range<F: Float>(x: F) -> i64 {
    if F::PREC == 113 && !crate::classify::signbit(x) { i64::MAX } else { i64::MIN }
}

/// `lrint`/`llrint`: rounded in the current direction to a 64-bit integer, raising inexact.
/// Out of range (and NaN, ∞): `FE_INVALID` and `LONG_MIN`, `errno` unchanged.
pub fn lrint<F: Float>(x: F, mode: Rounding) -> (i64, Status) {
    match to_int(x, Dir::of(mode), i128::from(i64::MIN), i128::from(i64::MAX)) {
        Some((v, lost)) => (v as i64, if lost { Status::flags(Flags::INEXACT) } else { Status::OK }),
        None => (out_of_range::<F>(x), Status::INVALID),
    }
}

/// `lround`/`llround`: rounded to nearest, ties away from zero, to a 64-bit integer; no inexact.
/// Out of range: `FE_INVALID` and `LONG_MIN`, `errno` unchanged.
pub fn lround<F: Float>(x: F) -> (i64, Status) {
    match to_int(x, Dir::Away, i128::from(i64::MIN), i128::from(i64::MAX)) {
        Some((v, _)) => (v as i64, Status::OK),
        None => (out_of_range::<F>(x), Status::INVALID),
    }
}

/// The `fromfp` family (glibc 2.39's interface: the result is an integer of type `intmax_t` or
/// `uintmax_t`): `x` rounded in direction `rnd` (an `FP_INT_*` value) to a signed or unsigned
/// integer of `width` bits (at most 64). Out of range, NaN or ∞: `FE_INVALID`, `EDOM`, and the
/// nearest end of the range (the largest value for a NaN). `inexact` (the `x` forms) raises
/// `FE_INEXACT` for a non-integral in-range result.
pub fn fromfp<F: Float>(x: F, rnd: i32, width: u32, signed: bool, inexact: bool) -> (i128, Status) {
    let w = width.min(64);
    let (lo, hi) = match (signed, w) {
        (_, 0) => (1, 0), // empty
        (true, w) => (-(1i128 << (w - 1)), (1i128 << (w - 1)) - 1),
        (false, w) => (0, (1i128 << w) - 1),
    };
    if let Some((v, lost)) = to_int(x, Dir::from_c(rnd), lo, hi) {
        if w > 0 {
            return (v, if inexact && lost { Status::flags(Flags::INEXACT) } else { Status::OK });
        }
    }
    let n = float::decode(x);
    // glibc picks the end by the sign bit, NaNs included.
    // An invalid x87 encoding counts as the default NaN (negative), whatever its sign bit (#902).
    let low_end = n.neg || n.kind == Kind::Invalid;
    let v = match (w, low_end) {
        (0, _) => 0,
        (_, true) => lo,
        (_, false) => hi,
    };
    (v, Status::DOMAIN)
}

/// `modf`: the fractional and integral parts, both with `x`'s sign. ∞ splits into ±0 and ±∞.
pub fn modf<F: Float>(x: F) -> ((F, F), Status) {
    let n = float::decode(x);
    match n.kind {
        Kind::Zero => ((x, x), Status::OK),
        Kind::Inf => ((float::zero(n.neg), x), Status::OK),
        Kind::QNan => ((x, x), Status::OK),
        Kind::SNan => {
            let q: F = float::quiet(&n);
            ((q, q), Status::INVALID)
        }
        Kind::Invalid => {
            let q: F = float::default_nan();
            ((q, q), Status::INVALID)
        }
        Kind::Finite if n.exp >= 0 => ((float::zero(n.neg), x), Status::OK),
        Kind::Finite => {
            let (k, _) = integer_part(&n, Dir::Zero);
            let d = (-n.exp) as u32;
            let frac = if d >= 128 { n.sig } else { n.sig & ((1u128 << d) - 1) };
            ((float::pack(n.neg, frac, n.exp), float::pack(n.neg, k, 0)), Status::OK)
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp, clippy::arithmetic_side_effects, clippy::as_conversions)]
mod tests {
    use super::*;

    #[test]
    fn integral_directions_agree_with_the_hardware() {
        let mut seed = 0x9e37_79b9_7f4a_7c15u64;
        for i in 0..200_000u64 {
            seed = seed.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
            let e = 0x3f0 + (seed >> 58) % 0x60; // exponents around the integer boundary
            let x = f64::from_bits((seed & 0x800f_ffff_ffff_ffff) | (e << 52)) * if i % 7 == 0 { 0.5 } else { 1.0 };
            assert_eq!(ceil(x).0.to_bits(), x.ceil().to_bits(), "{x:e}");
            assert_eq!(floor(x).0.to_bits(), x.floor().to_bits(), "{x:e}");
            assert_eq!(trunc(x).0.to_bits(), x.trunc().to_bits(), "{x:e}");
            assert_eq!(round(x).0.to_bits(), x.round().to_bits(), "{x:e}");
            assert_eq!(roundeven(x).0.to_bits(), x.round_ties_even().to_bits(), "{x:e}");
        }
    }

    #[test]
    fn conversions() {
        assert_eq!(lrint(2.5f64, Rounding::Nearest), (2, Status::flags(Flags::INEXACT)));
        assert_eq!(lrint(-2.5f64, Rounding::Down), (-3, Status::flags(Flags::INEXACT)));
        assert_eq!(lrint(9.3e18f64, Rounding::Nearest), (i64::MIN, Status::INVALID));
        assert_eq!(lrint(-(2f64.powi(63)), Rounding::Nearest), (i64::MIN, Status::OK));
        assert_eq!(lround(0.5f32), (1, Status::OK));
        assert_eq!(fromfp(1.5f64, 0, 8, true, false), (2, Status::OK));
        assert_eq!(fromfp(1e10f64, 0, 8, true, false), (127, Status::DOMAIN));
        assert_eq!(fromfp(-1e10f64, 0, 8, false, false), (0, Status::DOMAIN));
        assert_eq!(fromfp(f64::NAN, 0, 64, false, false), (u64::MAX.into(), Status::DOMAIN));
        assert_eq!(fromfp(0.5f64, 4, 64, true, true), (0, Status::flags(Flags::INEXACT)));
        let ((f, i), _) = modf(-3.25f64);
        assert_eq!((f, i), (-0.25, -3.0));
    }
}
