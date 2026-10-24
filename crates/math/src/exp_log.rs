//! Exponentials and logarithms (spec §10.3) for `float` and `double`: `exp`, `exp2`, `exp10`,
//! `expm1`, `log`, `log2`, `log10`, `log1p`. Computed in double-double ([`crate::elem`]).
//!
//! * `e^x`: `x = k·ln2/64 + r` with `|r| <= ln2/128` (Cody–Waite reduction with a four-part
//!   `ln2/64`), `e^x = 2^(k div 64) · 2^((k mod 64)/64) · e^r`, the table of `2^(j/64)` and the
//!   Taylor series of `e^r - 1` (Tang's table-driven method, ACM TOMS 15(2), 1989). `2^x` and
//!   `10^x` reduce the same way; `e^x - 1` near zero adds the table of `2^(j/64) - 1` so nothing
//!   cancels.
//! * `ln x`: `x = 2^e · m` with `m` in [√½, √2), `c ≈ 1/m` from a table of 12-bit values,
//!   `ln x = e·ln2 − ln c + ln(1 + r)` with `r = m·c − 1` (exact by Dekker's product),
//!   `ln(1 + r) = 2·atanh(r/(2 + r))` by its odd series. The bucket around 1 uses `c = 1`, so
//!   arguments near 1 lose nothing.
//!
//! The tables are `coeffs/tables.rs` (`cargo xtask tables`).

#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)] // table indices reduced to the tables' ranges; exponents and word positions of bounded range

use crate::coeffs::tables as t;
use crate::dd::{self, horner, two_prod, two_sum, Dd, Residual};
use crate::elem::{self, Small};
use crate::fast64;
use crate::float::{self, Kind};
use crate::status::{Rounding, Status};

const SCALE_E: f64 = f64::from_bits(t::SCALES[0]); // 64/ln2
const SCALE_10: f64 = f64::from_bits(t::SCALES[1]); // 64·log2(10)
const SQRT2: f64 = core::f64::consts::SQRT_2;
/// Below this magnitude `e^x` is `1 + x` to far beyond double-double precision (2^-60).
pub(crate) const TINY: f64 = 8.673_617_379_884_035e-19;

/// `2^(k/64) · e^r` as `v · 2^m` for a reduced `|r| <= ln2/128` (plus a little).
fn exp_reduced(r: Dd, k: i32) -> (Dd, i32) {
    // e^r - 1 = r·(1 + r/2! + r²/3! + ...): terms from r^6 on are below 2^-51 of the sum.
    let q = horner(r, &t::INV_FACT[1..12], 6);
    let p = r.mul(q);
    let tj = Dd::of(t::EXP2_J64[usize::try_from(k & 63).unwrap_or(0)]);
    (tj.add(tj.mul(p)), k >> 6)
}

/// `x − k·c` for the four-part Cody–Waite constant `c` (|k| < 2^17).
pub(crate) fn reduce(x: Dd, kf: f64, c: &[u64; 4]) -> Dd {
    let c = c.map(f64::from_bits);
    let r0 = x.hi - kf * c[0]; // exact: the product is exact and the two are close
    two_sum(r0, -(kf * c[1])).add_f64(-(kf * c[2])).add(two_prod(kf, c[3]).neg()).add_f64(x.lo)
}

/// `e^x` as `v · 2^m`, for `x` (a double-double) with |x| < 750.
pub(crate) fn exp_dd(x: Dd) -> (Dd, i32) {
    let kf = dd::nearest_int(x.hi * SCALE_E);
    exp_reduced(reduce(x, kf, &t::LN2_64_CW), dd::to_i32(kf))
}

/// `e^x − 1` for |x| <= 0.34 (|k| <= 32): `(2^(k/64) − 1) + 2^(k/64)·(e^r − 1)`; the parts do not
/// cancel.
pub(crate) fn expm1_small(x: Dd) -> Dd {
    let kf = dd::nearest_int(x.hi * SCALE_E);
    let r = reduce(x, kf, &t::LN2_64_CW);
    let p = r.mul(horner(r, &t::INV_FACT[1..12], 6));
    let e = Dd::of(t::EXP2M1_J64[usize::try_from(dd::to_i32(kf) + 32).unwrap_or(32)]);
    e.add(Dd::from(1.0).add(e).mul(p))
}

/// Special operands shared by the exponentials: `Some` result for NaN, ±∞ (giving `at_neg_inf`
/// for −∞) and ±0 (1, or `x` itself).
fn exp_special<F: Small>(x: F, at_neg_inf: f64, at_zero_is_x: bool) -> Option<(F, Status)> {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return Some(r);
    }
    match n.kind {
        Kind::Inf if n.neg => Some(elem::exact(at_neg_inf)),
        Kind::Inf => Some((x, Status::OK)),
        Kind::Zero if at_zero_is_x => Some((x, Status::OK)),
        Kind::Zero => Some(elem::exact(1.0)),
        _ => None,
    }
}

/// `exp(x)` = e^x.
pub fn exp<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    if let Some(r) = exp_special(x, 0.0, false) {
        return r;
    }
    let xf = x.widen();
    if xf > 710.0 {
        return elem::overflow(false, mode);
    }
    if xf < -746.0 {
        return elem::underflow(false, mode);
    }
    if dd::abs(xf) < TINY {
        return elem::near(1.0, xf > 0.0, mode);
    }
    if let Some(r) = fast("exp", xf, mode, fast64::exp) {
        return r;
    }
    let (v, m) = exp_dd(Dd::from(xf));
    elem::finish(v, m, Residual::Above, mode)
}

/// `exp2(x)` = 2^x; exact for integral `x`.
pub fn exp2<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    if let Some(r) = exp_special(x, 0.0, false) {
        return r;
    }
    let xf = x.widen();
    if xf >= 1024.0 {
        return elem::overflow(false, mode);
    }
    if xf < -1075.0 {
        return elem::underflow(false, mode);
    }
    if dd::abs(xf) < TINY {
        return elem::near(1.0, xf > 0.0, mode);
    }
    let kf = dd::nearest_int(xf * 64.0);
    let r = xf - kf / 64.0; // exact
    let k = dd::to_i32(kf);
    if r == 0.0 && k & 63 == 0 {
        return elem::finish(Dd::from(1.0), k >> 6, Residual::Exact, mode);
    }
    if let Some(r) = fast("exp2", xf, mode, fast64::exp2) {
        return r;
    }
    let (v, m) = exp_reduced(Dd::of(t::LN2).mul_f64(r), k);
    elem::finish(v, m, Residual::Above, mode)
}

/// `exp10(x)` = 10^x; exact for the integral `x` whose power of ten the format holds.
pub fn exp10<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    if let Some(r) = exp_special(x, 0.0, false) {
        return r;
    }
    let xf = x.widen();
    if xf > 309.0 {
        return elem::overflow(false, mode);
    }
    if xf < -325.0 {
        return elem::underflow(false, mode);
    }
    if dd::abs(xf) < TINY {
        return elem::near(1.0, xf > 0.0, mode);
    }
    // 10^n is exact in `F` for 0 <= n <= 22 (double) or 10 (float): 5^n fits the significand.
    let max_exact = if F::PREC == 24 { 10.0 } else { 22.0 };
    if (1.0..=max_exact).contains(&xf) && dd::nearest_int(xf) == xf {
        let mut p = 1.0;
        for _ in 0..dd::to_i32(xf) {
            p *= 10.0;
        }
        return elem::exact(p);
    }
    if let Some(r) = fast("exp10", xf, mode, fast64::exp10) {
        return r;
    }
    let kf = dd::nearest_int(xf * SCALE_10);
    let r = reduce(Dd::from(xf), kf, &t::LOG10_2_64_CW);
    let (v, m) = exp_reduced(r.mul(Dd::of(t::LN10)), dd::to_i32(kf));
    elem::finish(v, m, Residual::Above, mode)
}

/// `expm1(x)` = e^x − 1, accurate near 0.
pub fn expm1<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    if let Some(r) = exp_special(x, -1.0, true) {
        return r;
    }
    let xf = x.widen();
    if xf > 710.0 {
        return elem::overflow(false, mode);
    }
    if xf < -40.0 {
        // e^x < 2^-57: −1 plus a positive amount far below its last place.
        return elem::near(-1.0, true, mode);
    }
    if dd::abs(xf) < TINY {
        // x + x²/2: just above x.
        return elem::near(xf, true, mode);
    }
    if dd::abs(xf) <= 0.34 {
        return elem::finish(expm1_small(Dd::from(xf)), 0, Residual::Above, mode);
    }
    let (v, m) = exp_dd(Dd::from(xf));
    if m > 110 {
        // The −1 is far below the last place: just below e^x.
        return elem::finish(v, m, Residual::Below, mode);
    }
    let w = v.scale(dd::pow2(m)).add_f64(-1.0);
    elem::finish(w, 0, Residual::Above, mode)
}

/// The `double` fast path ([`crate::fast64`], M6-4) when `F` is `double`: its result when its
/// rounding test passes (counted by [`crate::stats`]); `None` sends the caller to the accurate path.
fn fast<F: Small>(name: &'static str, xf: f64, mode: Rounding, f: fn(f64, Rounding) -> Option<(f64, Status)>) -> Option<(F, Status)> {
    if F::PREC != 53 {
        return None;
    }
    let r = f(xf, mode);
    crate::stats::record(name, r.is_some());
    r.map(|(v, st)| (F::narrow(v), st))
}

/// `x = 2^e · m` with `m` in [√½, √2), for a positive normal or subnormal `x`.
///
/// Integer operations only: a subnormal's significand is normalised by a shift, never by a
/// floating multiply. A select-compiled `x · 2^54` ran for every `x` and raised overflow in
/// hardware from `x >= 2^970` up (#1722), which the in-place fast entries of `log`, `log2`,
/// `log10` and `pow` (`fast64`) cannot let reach the caller's MXCSR.
pub(crate) fn decompose(x: f64) -> (i32, f64) {
    const FRAC: u64 = 0x000f_ffff_ffff_ffff;
    let b = x.to_bits();
    let field = i32::try_from((b >> 52) & 0x7ff).unwrap_or(0);
    let (e, frac) = if field == 0 {
        // value = f·2^-1074 with 0 < f < 2^52: shift the leading bit to bit 52.
        let f = b & FRAC;
        let s = f.leading_zeros().saturating_sub(11);
        (-1022 - i32::try_from(s).unwrap_or(0), (f << s) & FRAC)
    } else {
        (field - 1023, b & FRAC)
    };
    let m = f64::from_bits(frac | (1023u64 << 52));
    if m > SQRT2 { (e + 1, m * 0.5) } else { (e, m) }
}

/// `ln x` for a positive double-double `x` (its leading part normal or subnormal, its trailing
/// part not subnormal unless zero).
pub(crate) fn log_dd(x: Dd) -> Dd {
    let (e, m) = decompose(x.hi);
    // The trailing part scaled like the leading one (in two steps: 2^-e may not be an f64).
    let half = e / 2;
    let mlo = x.lo * dd::pow2(-half) * dd::pow2(half - e);
    let i = dd::to_i32(dd::nearest_int((m - 1.0) * 128.0)).clamp(-37, 53);
    let idx = usize::try_from(i + 37).unwrap_or(37);
    let c = f64::from_bits(t::LOG_C[idx]);
    let p = two_prod(m, c);
    let r = two_sum(p.hi - 1.0, p.lo).add(Dd::from(mlo).mul_f64(c));
    // ln(1 + r) = 2·atanh(s), s = r/(2 + r): 2s·(1 + s²/3 + s⁴/5 + ...), |s| < 2^-8.4.
    let s = r.div(Dd::from(2.0).add(r));
    let lr = s.mul(horner(s.sqr(), &t::INV_ODD[..7], 3)).scale(2.0);
    let el = Dd::of(t::LN2).mul_f64(f64::from(e));
    el.add(Dd::of(t::LOG_L[idx])).add(lr)
}

/// `ln(1 + w)` for a double-double `w > −1`, accurate when `w` is small.
pub(crate) fn log1p_dd(w: Dd) -> Dd {
    if dd::abs(w.hi) < 1.0 / 256.0 {
        let s = w.div(Dd::from(2.0).add(w));
        return s.mul(horner(s.sqr(), &t::INV_ODD[..7], 3)).scale(2.0);
    }
    log_dd(Dd::from(1.0).add(w))
}

/// Special operands of the logarithms: NaN, negative (domain), ±0 (pole), +∞.
fn log_special<F: Small>(x: F, positive_nan: bool) -> Option<(F, Status)> {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return Some(r);
    }
    match n.kind {
        Kind::Zero => Some(elem::pole(true)),
        _ if n.neg => Some(if positive_nan { elem::domain_pos() } else { elem::domain() }),
        Kind::Inf => Some((x, Status::OK)),
        _ => None,
    }
}

/// `log(x)` = ln x.
pub fn log<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    if let Some(r) = log_special(x, false) {
        return r;
    }
    let xf = x.widen();
    if xf == 1.0 {
        return elem::exact(0.0);
    }
    if let Some(r) = fast("log", xf, mode, fast64::log) {
        return r;
    }
    elem::finish(log_dd(Dd::from(xf)), 0, Residual::Above, mode)
}

/// `log2(x)`; exact for powers of two.
pub fn log2<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    if let Some(r) = log_special(x, false) {
        return r;
    }
    let xf = x.widen();
    let n = float::decode(xf);
    if n.sig.is_power_of_two() {
        return elem::exact(f64::from(n.top()));
    }
    if let Some(r) = fast("log2", xf, mode, fast64::log2) {
        return r;
    }
    elem::finish(log_dd(Dd::from(xf)).mul(Dd::of(t::INV_LN2)), 0, Residual::Above, mode)
}

/// `log10(x)`; exact for the powers of ten the format holds.
pub fn log10<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    if let Some(r) = log_special(x, true) {
        return r;
    }
    let xf = x.widen();
    if xf == 1.0 {
        return elem::exact(0.0);
    }
    let mut p = 1.0;
    for k in 1..=22 {
        p *= 10.0;
        if xf == p {
            return elem::exact(f64::from(k));
        }
    }
    if let Some(r) = fast("log10", xf, mode, fast64::log10) {
        return r;
    }
    elem::finish(log_dd(Dd::from(xf)).mul(Dd::of(t::INV_LN10)), 0, Residual::Above, mode)
}

/// `log1p(x)` = ln(1 + x), accurate near 0.
pub fn log1p<F: Small>(x: F, mode: Rounding) -> (F, Status) {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return r;
    }
    let xf = x.widen();
    match n.kind {
        Kind::Zero => return (x, Status::OK),
        Kind::Inf if !n.neg => return (x, Status::OK),
        _ if xf == -1.0 => return elem::pole(true),
        _ if xf < -1.0 => return elem::domain(),
        _ => {}
    }
    if dd::abs(xf) < TINY {
        // x − x²/2: just below x.
        return elem::near(xf, false, mode);
    }
    elem::finish(log1p_dd(Dd::from(xf)), 0, Residual::Above, mode)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    #[test]
    fn exact_and_special_values() {
        assert_eq!(exp(0.0f64, Rounding::Nearest), (1.0, Status::OK));
        assert_eq!(exp2(-3.0f32, Rounding::Up).0, 0.125);
        assert_eq!(exp10(5.0f64, Rounding::Down), (100_000.0, Status::OK));
        assert_eq!(log2(1024.0f64, Rounding::Nearest), (10.0, Status::OK));
        assert_eq!(log10(1000.0f32, Rounding::Zero), (3.0, Status::OK));
        assert_eq!(log(0.0f64, Rounding::Nearest), (f64::NEG_INFINITY, Status::POLE));
        assert_eq!(log(-1.0f64, Rounding::Nearest).1, Status::DOMAIN);
        assert_eq!(expm1(f64::NEG_INFINITY, Rounding::Nearest).0, -1.0);
        assert_eq!(exp(1000.0f64, Rounding::Nearest).0, f64::INFINITY);
        assert_eq!(exp(-1000.0f64, Rounding::Up).0.to_bits(), 1);
    }

    #[test]
    fn close_to_the_host() {
        // The host's functions are within an ulp or two; ours must agree within two.
        let mut x = 1e-3;
        while x < 700.0 {
            for v in [x, -x] {
                let near = |a: f64, b: f64| (a - b).abs() <= 2.0 * f64::EPSILON * b.abs();
                assert!(near(exp(v, Rounding::Nearest).0, v.exp()), "exp {v}");
                assert!(near(expm1(v, Rounding::Nearest).0, v.exp_m1()), "expm1 {v}");
                assert!(near(exp2(v / 2.0, Rounding::Nearest).0, (v / 2.0).exp2()), "exp2 {v}");
                assert!(near(log(x, Rounding::Nearest).0, x.ln()), "log {x}");
                assert!(near(log1p(v / 701.0, Rounding::Nearest).0, (v / 701.0).ln_1p()), "log1p {v}");
            }
            x *= 1.37;
        }
    }
}
