//! Hyperbolic functions (spec §10.3) for `float` and `double`: `sinh`, `cosh`, `tanh`, `asinh`,
//! `acosh`, `atanh`, from the double-double exponentials and logarithms of `exp_log`, arranged so
//! that nothing cancels:
//!
//! * `sinh x = (E + E/(E + 1))/2` with `E = e^|x| − 1` for small `|x|`, `(e^x − e^−x)/2` above;
//!   `tanh x = E/(E + 2)` with `E = e^(2|x|) − 1`; `cosh x = (e^x + e^−x)/2`.
//! * `asinh x = ln(1 + w)` with `w = x + x²/(1 + √(1 + x²))`; `acosh x = ln(1 + t + √(2t + t²))`
//!   with `t = x − 1`; `atanh x = ½·ln(1 + 2x/(1 − x))`.

#![allow(clippy::arithmetic_side_effects)] // exponents and integer parts of bounded range

use crate::coeffs::tables as t;
use crate::dd::{self, two_sum, Dd, Residual};
use crate::elem::{self, Small};
use crate::exp_log::{exp_dd, expm1_small, log1p_dd, log_dd};
use crate::float::{self, Kind};
use crate::status::{Rounding, Status};

/// Below this magnitude `x(1 + c·x²)` rounds as `x` plus a tiny amount of the sign of `c·x³`, and
/// above it the correction is far beyond the double-double error (2^-30; see `trig`).
const TINY3: f64 = 9.313_225_746_154_785e-10;

/// `e^a − 1` for `a >= 0` (a < 40) as a double-double.
fn expm1_pos(a: f64) -> Dd {
    if a <= 0.34 {
        return expm1_small(Dd::from(a));
    }
    let (v, m) = exp_dd(Dd::from(a));
    v.scale(dd::pow2(m)).add_f64(-1.0)
}

/// NaN, ±0 and ±∞ of the odd functions that keep them (`sinh`, `asinh`, `tanh` but for ∞).
fn odd_special<F: Small>(x: F, inf: Option<f64>) -> Option<(F, Status)> {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return Some(r);
    }
    match n.kind {
        Kind::Zero => Some((x, Status::OK)),
        Kind::Inf => Some(inf.map_or((x, Status::OK), |v| elem::exact(if n.neg { -v } else { v }))),
        _ => None,
    }
}

fn signed(v: Dd, neg: bool) -> Dd {
    if neg { v.neg() } else { v }
}

/// `sinh(x)`.
pub fn sinh<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    if let Some(r) = odd_special(x, None) {
        return r;
    }
    let xf = x.widen();
    let (a, neg) = (dd::abs(xf), xf < 0.0);
    if a < TINY3 {
        return elem::near(xf, !neg, mode); // x + x³/6
    }
    if a > 711.0 {
        return elem::overflow(neg, mode);
    }
    if a >= 40.0 {
        // e^a/2, the e^−a term below every last place: just below.
        let (v, m) = exp_dd(Dd::from(a));
        return elem::finish(signed(v, neg), m - 1, if neg { Residual::Above } else { Residual::Below }, mode);
    }
    let r = if a <= 0.34 {
        let e = expm1_small(Dd::from(a));
        e.add(e.div(e.add_f64(1.0))).scale(0.5)
    } else {
        let (v, m) = exp_dd(Dd::from(a));
        let ea = v.scale(dd::pow2(m));
        ea.sub(Dd::from(1.0).div(ea)).scale(0.5)
    };
    elem::finish(signed(r, neg), 0, Residual::Above, mode)
}

/// `cosh(x)`.
pub fn cosh<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return r;
    }
    match n.kind {
        Kind::Zero => return elem::exact(1.0),
        Kind::Inf => return elem::exact(f64::INFINITY),
        _ => {}
    }
    let a = dd::abs(x.widen());
    if a < TINY3 {
        return elem::near(1.0, true, mode); // 1 + x²/2
    }
    if a > 711.0 {
        return elem::overflow(false, mode);
    }
    let (v, m) = exp_dd(Dd::from(a));
    if a >= 40.0 {
        return elem::finish(v, m - 1, Residual::Above, mode);
    }
    let ea = v.scale(dd::pow2(m));
    elem::finish(ea.add(Dd::from(1.0).div(ea)).scale(0.5), 0, Residual::Above, mode)
}

/// `tanh(x)`.
pub fn tanh<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    if let Some(r) = odd_special(x, Some(1.0)) {
        return r;
    }
    let xf = x.widen();
    let (a, neg) = (dd::abs(xf), xf < 0.0);
    if a < TINY3 {
        return elem::near(xf, neg, mode); // x − x³/3
    }
    if a >= 22.0 {
        // 1 − 2e^−2a, the deviation below a quarter of the last place under 1 (and, above
        // about 35, below the double-double error): just inside ±1.
        return elem::near(if neg { -1.0 } else { 1.0 }, neg, mode);
    }
    let e = expm1_pos(2.0 * a);
    elem::finish(signed(e.div(e.add_f64(2.0)), neg), 0, Residual::Above, mode)
}

/// `asinh(x)`.
pub fn asinh<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    if let Some(r) = odd_special(x, None) {
        return r;
    }
    let xf = x.widen();
    let (a, neg) = (dd::abs(xf), xf < 0.0);
    if a < TINY3 {
        return elem::near(xf, neg, mode); // x − x³/6
    }
    let r = if a > 1.8e16 {
        // ln(2a) + 1/(4a²): the second term is below every last place.
        log_dd(Dd::from(a)).add(Dd::of(t::LN2))
    } else if a <= 2.0 {
        // w = a + a²/(1 + √(1 + a²)), ln(1 + w).
        let a2 = dd::two_prod(a, a);
        let w = Dd::from(a).add(a2.div(dd::sqrt(a2.add_f64(1.0)).add_f64(1.0)));
        log1p_dd(w)
    } else {
        log_dd(Dd::from(a).add(dd::sqrt(dd::two_prod(a, a).add_f64(1.0))))
    };
    elem::finish(signed(r, neg), 0, Residual::Above, mode)
}

/// `acosh(x)`.
pub fn acosh<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return r;
    }
    let xf = x.widen();
    if xf < 1.0 {
        return elem::domain();
    }
    if xf == 1.0 {
        return elem::exact(0.0);
    }
    if n.kind == Kind::Inf {
        return (x, Status::OK);
    }
    let r = if xf > 1.8e16 {
        log_dd(Dd::from(xf)).add(Dd::of(t::LN2))
    } else if xf <= 2.0 {
        // t = x − 1 (exact), ln(1 + t + √(2t + t²)).
        let tt = xf - 1.0;
        let root = dd::sqrt(dd::two_prod(tt, tt).add(Dd::from(2.0 * tt)));
        log1p_dd(root.add_f64(tt))
    } else {
        log_dd(Dd::from(xf).add(dd::sqrt(dd::two_prod(xf, xf).add_f64(-1.0))))
    };
    elem::finish(r, 0, Residual::Above, mode)
}

/// `atanh(x)`.
pub fn atanh<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    if float::decode(x).kind == Kind::Inf {
        return elem::domain();
    }
    if let Some(r) = odd_special(x, None) {
        return r;
    }
    let xf = x.widen();
    let (a, neg) = (dd::abs(xf), xf < 0.0);
    if a > 1.0 {
        return elem::domain();
    }
    if a == 1.0 {
        return elem::pole(neg);
    }
    if a < TINY3 {
        return elem::near(xf, !neg, mode); // x + x³/3
    }
    // ½·ln(1 + 2a/(1 − a)), 1 − a exact as a double-double.
    let q = Dd::from(2.0 * a).div(two_sum(1.0, -a));
    elem::finish(signed(log1p_dd(q).scale(0.5), neg), 0, Residual::Above, mode)
}
