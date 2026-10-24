//! Powers and roots (spec §10.3): `pow`, `cbrt`, `hypot` (`sqrt` is correctly rounded in
//! `fused`).
//!
//! * `pow(x, y)` = `e^(y·ln|x|)` with `ln|x|` and the product in double-double and the sign from
//!   the parity of `y` (C Annex F special cases first). Results that are dyadic numbers of at most
//!   128 bits (powers of two, `y = ±1, 1/2`, and `y = m/2^k` with an exact `2^k`-th root and a
//!   power that fits: `wide::exact_power`) are computed exactly, so every rounding direction is
//!   right.
//! * `cbrt(x)`: Newton's iteration in `f64`, one correction in double-double; perfect cubes are
//!   recognised and exact.
//! * `hypot(x, y)`: `√(x² + y²)` from the exact sum of the exact squares, rounded once
//!   (correctly rounded in every format, without overflow or underflow in between).

#![allow(clippy::arithmetic_side_effects)] // exponents and integer parts of bounded range

use crate::arith;
use crate::dd::{self, two_prod, Dd, Residual};
use crate::elem::{self, Small};
use crate::exp_log::{exp_dd, log_dd};
use crate::fast64;
use crate::float::{self, Exact, Float, Kind, Num};
use crate::fused;
use crate::wide;
use crate::status::{Rounding, Status};

/// Is the finite `y` an integer, and an odd one?
fn integer_parity(y: f64) -> (bool, bool) {
    let n = float::decode(y);
    if n.kind != Kind::Finite {
        return (n.kind == Kind::Zero, false);
    }
    if n.exp >= 0 {
        return (true, n.exp == 0 && n.sig & 1 == 1);
    }
    let frac_bits = n.exp.unsigned_abs();
    if frac_bits >= 128 || n.sig & ((1u128 << frac_bits) - 1) != 0 {
        return (false, false);
    }
    (true, (n.sig >> frac_bits) & 1 == 1)
}

/// The odd part of a positive finite `x` and its exponent: `x = odd · 2^e`.
fn odd_part(x: f64) -> (u64, i32) {
    let n = float::decode(x);
    let tz = n.sig.trailing_zeros();
    (u64::try_from(n.sig >> tz).unwrap_or(u64::MAX), n.exp + i32::try_from(tz).unwrap_or(0))
}

/// Round an exact `odd · 2^e` with a sign.
fn exact_int<F: Small>(neg: bool, odd: u128, e: i64, mode: Rounding) -> (F, Status) {
    let e = i32::try_from(e.clamp(-100_000, 100_000)).unwrap_or(0);
    let (v, flags) = float::round::<F>(Exact { neg, sig: odd, exp: e, sticky: false }, mode);
    (v, float::range_errno(v, Status::flags(flags)))
}

/// `pow(x, y)` = x^y.
#[allow(clippy::too_many_lines)] // the Annex F table is long
pub fn pow<F: Small>(x: F, y: F, mode: Rounding) -> (F, Status) {
    let (a, b) = (float::decode(x), float::decode(y));
    let (xf, yf) = (x.widen(), y.widen());
    // x^±0 = 1 and 1^y = 1, even for a quiet NaN.
    if b.kind == Kind::Zero || (xf == 1.0 && a.kind == Kind::Finite) {
        let st = if a.kind == Kind::SNan || b.kind == Kind::SNan { Status::INVALID } else { Status::OK };
        return (elem::exact::<F>(1.0).0, st);
    }
    if let Some(r) = fused::nan_operands::<F, F>(&[a, b]) {
        return r;
    }
    let (y_int, y_odd) = if b.kind == Kind::Finite { integer_parity(yf) } else { (false, false) };
    let neg = a.neg && y_odd;
    // y = ±∞.
    if b.kind == Kind::Inf {
        let ax = dd::abs(xf);
        let v = if ax == 1.0 {
            1.0
        } else if (ax < 1.0) == b.neg {
            f64::INFINITY
        } else {
            0.0
        };
        return elem::exact(v);
    }
    // x = ±0.
    if a.kind == Kind::Zero {
        return if b.neg { elem::pole(neg) } else { elem::exact(if neg { -0.0 } else { 0.0 }) };
    }
    // x = ±∞.
    if a.kind == Kind::Inf {
        let v = if b.neg { 0.0 } else { f64::INFINITY };
        return elem::exact(if neg { -v } else { v });
    }
    if a.neg && !y_int {
        return elem::domain();
    }
    let ax = dd::abs(xf);
    // The exactly computable cases.
    if yf == 1.0 {
        return (x, Status::OK);
    }
    if yf == -1.0 {
        let (v, st) = fused::div::<F, F>(elem::exact::<F>(1.0).0, x, mode);
        return (v, float::range_errno(v, st));
    }
    if yf == 0.5 {
        return fused::sqrt::<F, F>(float::with_sign(x, false), mode);
    }
    let (odd, e) = odd_part(ax);
    if odd == 1 {
        // A power of two: 2^(e·y) is exact when e·y is an integer.
        let p = two_prod(f64::from(e), yf);
        if p.lo == 0.0 && dd::nearest_int(p.hi) == p.hi && dd::abs(p.hi) < 1e6 {
            return exact_int(neg, 1, i64::from(dd::to_i32(p.hi)), mode);
        }
    }
    // Every other dyadic result of at most 128 bits (integral `y`, and `y = m/2^k` with an exact
    // `2^k`-th root, as pow(9, 1.5) = 27), computed exactly so that every direction is right.
    if let Some(r) = wide::exact_power::<F>(&Num { neg: false, ..a }, &b, neg, mode) {
        return r;
    }
    // The `double` fast path (M6-5): the correctly rounded result or `None` (then the accurate
    // path below runs unchanged). Every exact result was returned above.
    if F::PREC == 53 {
        let r = fast64::pow(ax, yf, neg, mode);
        crate::stats::record("pow", r.is_some());
        if let Some((v, st)) = r {
            return (F::narrow(v), st);
        }
    }
    // The general case: e^(y·ln|x|), its range checked before the double-double product (whose
    // splitting would overflow for huge y).
    let l = log_dd(Dd::from(ax));
    let est = l.hi * yf;
    if est > 710.0 {
        return elem::overflow(neg, mode);
    }
    if est < -746.0 {
        return elem::underflow(neg, mode);
    }
    let z = l.mul_f64(yf);
    let (v, m) = exp_dd(z);
    elem::finish(if neg { v.neg() } else { v }, m, Residual::Above, mode)
}

/// `cbrt(x)`, the real cube root.
pub fn cbrt<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return r;
    }
    if n.kind != Kind::Finite {
        return (x, Status::OK);
    }
    let a = dd::abs(x.widen());
    // a = 2^(3q) · m with m in [1, 8).
    let top = n.top();
    let q = top.div_euclid(3);
    let m = dd::ldexp(a, -q * 3);
    let mut y = 1.5;
    for _ in 0..7 {
        y -= (y * y * y - m) / (3.0 * y * y);
    }
    // One Newton correction in double-double: y + (m − y³)/(3y²).
    let y3 = two_prod(y, y).mul_f64(y);
    let corr = Dd::from(m).sub(y3).div(two_prod(y, y).mul_f64(3.0));
    let r = Dd::from(y).add(corr);
    // A perfect cube: c³ = |x| exactly, for the double nearest the root.
    let c = r.to_f64();
    let (oc, ec) = odd_part(c);
    let (oa, ea) = odd_part(a);
    let res = if oc < 1 << 18 && u64::pow(oc, 3) == oa && 3 * (ec + q) == ea {
        Residual::Exact
    } else {
        Residual::Above
    };
    let v = if res == Residual::Exact { Dd::from(c) } else { r };
    elem::finish(if n.neg { v.neg() } else { v }, q, res, mode)
}

/// `hypot(x, y)` = √(x² + y²), correctly rounded in every format. `hypot(±∞, y)` is +∞ even
/// for a quiet NaN `y`.
pub fn hypot<F: Float>(x: F, y: F, mode: Rounding) -> (F, Status) {
    let (a, b) = (float::decode(x), float::decode(y));
    let inf = a.kind == Kind::Inf || b.kind == Kind::Inf;
    let signalling = [a.kind, b.kind].iter().any(|k| matches!(k, Kind::SNan | Kind::Invalid));
    if inf && !signalling {
        return (float::inf(false), Status::OK);
    }
    if let Some(r) = fused::nan_operands::<F, F>(&[a, b]) {
        return r;
    }
    let root = arith::hypot_exact::<F>(&a, &b);
    if root.sig == 0 {
        return (float::zero(false), Status::OK);
    }
    let (v, flags) = float::round::<F>(root, mode);
    (v, float::range_errno(v, Status::flags(flags)))
}
