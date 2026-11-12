//! The quick and middle tiers of `cpow` and `cpowf` (spec §10.5; LIBM-CPLX-POW, under D-10.53):
//! `z^w = e^E·cis Φ` with `w = c + id`, `L = log z = ℓr + iℓi` (principal branch),
//! `E = c·ℓr − d·ℓi`, `Φ = d·ℓr + c·ℓi`. Every special, degenerate or out-of-range operand, and
//! every part neither tier decides, takes the 384-bit route ([`crate::complex::cpow_route`]).
//!
//! **Error of `E` and `Φ`.** With `ℓr`, `ℓi` double-doubles within `er`, `ei` (absolute), the
//! four products by [`Dd::mul_f64`] (`3u²/2 + 4u³`, Joldes, Muller and Popescu, ACM TOMS 44(2),
//! 2017, Alg. 9) and the sum by [`Dd::add`] (`3u²/(1 − 4u)` of the result, Alg. 6):
//! `eE = |c|·er + |d|·ei + DD_REL·(|c·ℓr| + |d·ℓi|)`, `eΦ = |d|·er + |c|·ei + DD_REL·(|d·ℓr| +
//! |c·ℓi|)`, `DD_REL = 2^-102` (proved `2^-103.3`). What reaches a part is ABSOLUTE in `E` and
//! `Φ`: `e^(E+δ) = e^E·(1 + δ + O(δ²))` and `|cos', sin'| <= 1`; so near `|z| = 1` an absolute
//! bound on `ℓr` suffices. Both must be below `2^-30` (else decline), so the second-order terms
//! fit in the factor `W30 = 1 + 2^-29`, which also covers the `double` evaluation of the bound.
//!
//! **Part bound.** `P = V ⊗ T` ([`Dd::mul`], `2^-102.6`), `A = |V.hi|`, `B = |T.hi|`, `V·2^m ≈
//! e^E` within `ev` (absolute in `V`), `T ≈ cos Φ` or `sin Φ` within `et` (absolute):
//! `|part·2^-m − P| <= ((ev + A·eE)·B + A·(et + eΦ) + PROD·A·B)·W30` (as `cexp`'s proof in
//! [`super`], plus the two argument errors).
//!
//! **Quick tier.** `L` from [`super::clog_parts`] (`ℓi` within `2^-65` relative: the
//! bottleneck), `V` from [`fast64::exp_kernel`] on [`fast64::reduce_pow`] (`E.lo` folded: at most
//! `2^-97` absolute more, `EXP_FOLD = 2^-96` added to `eE`), `ev` = [`fast64::exp_eps`]; `T` from
//! the trig kernels with `Φ.lo` folded into the reduced argument by two 2Sums (at most `2^-110`
//! more, inside [`trig::eps`]'s `2^-89` reduction term; `|r| < 2^-5.34` still).
//!
//! **Middle tier** (cold; only a part the quick test declines): `ℓr = ln(s)/2`, `s = x² + y²`
//! ([`super::sum_squares`], `2^-104.4`) by [`crate::exp_log::log_dd`] (table reduction, exact
//! `m·c − 1`, the `atanh` series: relative `2^-100.2` on the series part, `2^-104.4` per
//! addition of the `e·ln 2` and table terms): `er = LR_REL·|ℓr| + LR_ABS`; `ℓi` by
//! [`crate::trig::atan_dd`] of `|y|/|x|` ([`Dd::div`], `π − θ` for `x < 0`): within `2^-99.5`
//! relative (atan is 1-conditioned in `t`), `ei = LI_REL·|ℓi|`. `V = e^E` from
//! [`crate::wide_dd::exp_dd`] (`2^-103`) times `1 + E.lo + E.lo²/2` (`|E.lo| <= 2^-44`), `T` by
//! [`sin_or_cos_dd`] (`2^-100` absolute: `cexp`'s middle proof, `Φ.lo` added to the reduced
//! argument, `2^-105` more), the part bound `(A·B + A)·MID_EPS` of `cexp`'s middle tier plus
//! the argument terms. The constants carry a factor 2 or more over the estimates; the tests pin
//! the observed errors against the 384-bit route.
//!
//! **Strict test** ([`strict`]): a part is answered only when the interval `P ± eps` holds no
//! `double` and no midpoint (`|P.lo| > eps`, then [`super::round`]). The parts of `z^w` may be
//! `double`s (`(3+4i)^1.5 = 2 + 11i`) and, for `d != 0`, no theorem excludes rational parts, so
//! exactness is never assumed: such operands decline and the route decides value and flags.
//! Expected declines (the bound over an ulp): quick about `2^-10` per part, middle about
//! `2^-44` per quick decline for `|w·log z| <= 2^4` (`cpow_decline_rates` measures both).
//!
//! **`float`** ([`cpowf`]): the quick tier's parts from the widened operands, rounded by
//! [`round32`]: answered when `P ± eps` lies strictly between two neighbours of the 25-bit grid
//! (the `float`s and their midpoints), in every mode; the route otherwise.

#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)] // exponent fields and format conversions of checked range

use super::{t, Part, COS_DD, HALF_PI, MID_EPS, PROD, SIN_DD, TWO_OVER_PI, X_MAX, X_MIN, Y_MAX, Y_MIN};
use crate::dd::{self, Dd};
use crate::fast64;
use crate::float::Float;
use crate::status::{Flags, Rounding, Status};
use crate::trig::{self, kernel};

/// `2^-102`: the products' and the sum's relative bound (proved `2^-103.3`).
const DD_REL: f64 = f64::from_bits(0x3990_0000_0000_0000);
/// `2^-96`: `E.lo` folded into the exponential's reduced argument.
const EXP_FOLD: f64 = f64::from_bits(0x39f0_0000_0000_0000);
/// `2^-30`: the largest argument error the second-order terms allow.
const ARG_MAX: f64 = f64::from_bits(0x3e10_0000_0000_0000);
/// `1 + 2^-29`.
const W30: f64 = f64::from_bits(0x3ff0_0000_0080_0000);
/// `2^-100`: the strict test's slack (as [`fast64::settle`]'s).
const SLACK: f64 = f64::from_bits(0x39b0_0000_0000_0000);
/// The middle tier's logarithm bounds: `2^-99` relative and `2^-102` absolute for `ℓr`, `2^-98`
/// relative for `ℓi`.
const LR_REL: f64 = f64::from_bits(0x39c0_0000_0000_0000);
const LR_ABS: f64 = f64::from_bits(0x3990_0000_0000_0000);
const LI_REL: f64 = f64::from_bits(0x39d0_0000_0000_0000);
/// `2^-60` and `2^20`: the range of a nonzero `|c|`, `|d|` (every product stays normal).
const W_LO: f64 = f64::from_bits(0x3c30_0000_0000_0000);
const W_HI: f64 = 1_048_576.0;

/// `E`, `Φ` and their absolute bounds from `L` and `w`, or `None` when an error exceeds
/// [`ARG_MAX`] or `w` is out of range.
#[inline(always)]
fn exponent(lr: (Dd, f64), li: (Dd, f64), c: f64, d: f64) -> Option<(Dd, Dd, f64, f64)> {
    let ok = |v: f64| v == 0.0 || (W_LO..=W_HI).contains(&dd::abs(v));
    if !ok(c) || !ok(d) || (c == 0.0 && d == 0.0) {
        return None;
    }
    let ((r, er), (i, ei)) = (lr, li);
    let (ac, ad, ar, ai) = (dd::abs(c), dd::abs(d), dd::abs(r.hi), dd::abs(i.hi));
    let e = r.mul_f64(c).sub(i.mul_f64(d));
    let f = r.mul_f64(d).add(i.mul_f64(c));
    let ee = ac * er + ad * ei + DD_REL * (ac * ar + ad * ai);
    let ef = ad * er + ac * ei + DD_REL * (ad * ar + ac * ai);
    if !(ee <= ARG_MAX && ef <= ARG_MAX) || !(X_MIN..=X_MAX).contains(&dd::abs(e.hi)) || !(Y_MIN..Y_MAX).contains(&dd::abs(f.hi)) {
        return None;
    }
    Some((e, f, ee, ef))
}

/// The quick tier's parts unscaled with their bounds, `[(Re, eps), (Im, eps)]`, and the scale.
#[inline(always)]
fn parts(x: f64, y: f64, c: f64, d: f64) -> Option<([(Dd, f64); 2], i32)> {
    let [lr, li] = super::clog_parts(x, y)?;
    if dd::abs(lr.0.hi) < W_LO || dd::abs(li.0.hi) < W_LO {
        return None;
    }
    let (e, f, ee, ef) = exponent(lr, li, c, d)?;
    let kf = dd::nearest_int(e.hi * f64::from_bits(t::SCALES[0]));
    let (v, m, tail) = fast64::exp_kernel(fast64::reduce_pow(e, kf), dd::to_i32(kf))?;
    let ev = fast64::exp_eps(v, tail);
    let (n, r) = kernel::reduce(f.hi);
    let s = dd::two_sum(r.hi, f.lo);
    let p = kernel::poly(dd::two_sum(s.hi, s.lo + r.lo));
    let (sn, ss) = kernel::assemble(n, p);
    let (cs, cc) = kernel::assemble(n + 32, p);
    let a = dd::abs(v.hi);
    let ee = ee + EXP_FOLD;
    let part = |u: Dd, et: f64| {
        let b = dd::abs(u.hi);
        (v.mul(u), ((ev + a * ee) * b + a * (et + ef) + PROD * a * b) * W30)
    };
    Some(([part(cs, trig::eps(cc, cs)), part(sn, trig::eps(ss, sn))], m))
}

/// `P·2^m` in `mode` when `P ± eps` holds no `double` and no midpoint, else `None`.
#[inline(always)]
fn strict(p: Dd, eps: f64, m: i32, mode: Rounding) -> Option<f64> {
    if dd::abs(p.lo) <= eps + dd::abs(p.hi) * SLACK {
        return None;
    }
    super::round(p, eps, m, mode)
}

/// `cpow(x + iy, c + id)` in `mode` by the quick tier and, for a part it declines, the middle
/// tier: both parts or `None`.
#[inline(always)]
pub fn cpow(x: f64, y: f64, c: f64, d: f64, mode: Rounding) -> Option<((f64, f64), Status)> {
    let ([(re, er), (im, ei)], m) = parts(x, y, c, d)?;
    let a = match strict(re, er, m, mode) {
        Some(v) => v,
        None => middle(x, y, c, d, Part::Re, mode)?,
    };
    let b = match strict(im, ei, m, mode) {
        Some(v) => v,
        None => middle(x, y, c, d, Part::Im, mode)?,
    };
    Some(((a, b), Status::flags(Flags::INEXACT)))
}

/// `cos Φ` or `sin Φ` of a double-double `Φ` (`2^-30 <= |Φ.hi| < 2^20`) within `2^-100`
/// absolute: [`super::sin_or_cos`] with `Φ.lo` added to the reduced argument.
fn sin_or_cos_dd(phi: Dd, part: Part) -> Dd {
    let n = dd::nearest_int(phi.hi * TWO_OVER_PI);
    let h = dd::two_prod(n, HALF_PI[0]);
    let g = dd::two_prod(n, HALF_PI[1]);
    let r = dd::two_sum(phi.hi, -h.hi).add(Dd::from(-h.lo)).add(g.neg()).add_f64(-(n * HALF_PI[2])).add_f64(phi.lo);
    let z = r.mul(r);
    let q = (dd::to_i32(n) + i32::from(part == Part::Re)) & 3;
    let v = if q & 1 == 0 { r.mul(dd::horner(z, &SIN_DD, 8)) } else { dd::horner(z, &COS_DD, 9) };
    if q & 2 == 0 { v } else { v.neg() }
}

/// The middle tier's part, its bound and its scale (see the module comment).
fn middle_part(x: f64, y: f64, c: f64, d: f64, part: Part) -> Option<(Dd, f64, i32)> {
    let (ax, ay) = super::window(x, y)?;
    let lr = crate::exp_log::log_dd(super::sum_squares(ax, ay)).scale(0.5);
    let th = crate::trig::atan_dd(Dd::from(ay).div(Dd::from(ax)));
    let th = if x < 0.0 { Dd::of(t::PI_2).scale(2.0).sub(th) } else { th };
    let li = if y < 0.0 { th.neg() } else { th };
    let er = LR_REL * dd::abs(lr.hi) + LR_ABS;
    let (e, f, ee, ef) = exponent((lr, er), (li, LI_REL * dd::abs(li.hi)), c, d)?;
    let (v, s) = crate::wide_dd::exp_dd(&crate::float::decode(e.hi))?;
    let v = v.mul(dd::two_sum(1.0, e.lo).add_f64(0.5 * e.lo * e.lo));
    let u = sin_or_cos_dd(f, part);
    let (a, b) = (dd::abs(v.hi), dd::abs(u.hi));
    Some((v.mul(u), ((a * b + a) * MID_EPS + a * b * ee + a * ef) * W30, s))
}

/// One part of `cpow` by the middle tier, for an operand whose quick test declined; `None` sends
/// the operand to the 384-bit route.
#[cold]
#[inline(never)]
fn middle(x: f64, y: f64, c: f64, d: f64, part: Part, mode: Rounding) -> Option<f64> {
    let (p, eps, s) = middle_part(x, y, c, d, part)?;
    strict(p, eps, s, mode)
}

/// `P·2^m` rounded to `float` in `mode` when `P ± eps` lies strictly between two neighbours of
/// the 25-bit grid (so it holds no `float` and no midpoint), normal results only.
fn round32(p: Dd, eps: f64, m: i32, mode: Rounding) -> Option<f32> {
    let v = super::scale(p.hi, m)?;
    let av = dd::abs(v);
    if !(f64::from(f32::MIN_POSITIVE)..=f64::from(f32::MAX)).contains(&av) {
        return None;
    }
    let bits = av.to_bits();
    // The grid step `g` of `av`'s binade and the grid point `k` at or below `av` (exact).
    let g = f64::from_bits(((bits >> 52) - 24) << 52);
    let k = f64::from_bits(bits & !((1u64 << 28) - 1));
    let lo = p.lo * dd::pow2(m);
    let dl = ((av - k) - 0.5 * g) + if v < 0.0 { -lo } else { lo };
    // The rounding of `dl` and of the threshold is below `2^-52·g`.
    let thr = g * (0.5 - SLACK_32) - eps * dd::pow2(m) * (1.0 + SLACK_32);
    if !(dd::abs(dl) < thr) {
        return None;
    }
    let odd = bits & (1u64 << 28) != 0;
    let (below, above) = if odd { (k - g, k + g) } else { (k, k + 2.0 * g) };
    let mag = match (mode, v < 0.0) {
        (Rounding::Nearest, _) => {
            if odd {
                above
            } else {
                below
            }
        }
        (Rounding::Down, false) | (Rounding::Up, true) | (Rounding::Zero, _) => below,
        _ => above,
    };
    let r = (if v < 0.0 { -mag } else { mag }) as f32;
    r.is_finite().then_some(r)
}

/// `2^-49`: [`round32`]'s margin.
const SLACK_32: f64 = f64::from_bits(0x3ce0_0000_0000_0000);

/// `cpowf` by the quick tier's parts and [`round32`], both parts or `None`.
pub fn cpowf(x: f32, y: f32, c: f32, d: f32, mode: Rounding) -> Option<((f32, f32), Status)> {
    let ([(re, er), (im, ei)], m) = parts(x.into(), y.into(), c.into(), d.into())?;
    Some(((round32(re, er, m, mode)?, round32(im, ei, m, mode)?), Status::flags(Flags::INEXACT)))
}

/// The `cpow` tiers for any format: `double` and `float`, `None` for the others.
#[inline(always)]
pub(crate) fn cpow_any<F: Float>(x: F, y: F, c: F, d: F, mode: Rounding) -> Option<((F, F), Status)> {
    let ((a, b), st) = match F::PREC {
        53 => {
            let w = |v: F| f64::from_bits(v.to_bits() as u64);
            let ((a, b), st) = cpow(w(x), w(y), w(c), w(d), mode)?;
            ((u128::from(a.to_bits()), u128::from(b.to_bits())), st)
        }
        24 => {
            let w = |v: F| f32::from_bits(v.to_bits() as u32);
            let ((a, b), st) = cpowf(w(x), w(y), w(c), w(d), mode)?;
            ((u128::from(a.to_bits()), u128::from(b.to_bits())), st)
        }
        _ => return None,
    };
    Some(((F::from_bits(a), F::from_bits(b)), st))
}

#[cfg(test)]
#[path = "complex_fast_pow_tests.rs"]
mod tests;
