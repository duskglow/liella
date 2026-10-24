//! Correct rounding by Ziv's strategy, and error measurement in ulps.

use super::{Approx, Big, Fmt, Round, Rounded};

/// Error of the Ziv loop: the result could not be decided below `max_prec` bits.
#[derive(Clone, Debug)]
pub struct Undetermined {
    /// The last approximation computed.
    pub value: Big,
}

/// Evaluates `f` (computing an [`Approx`] at a given working precision) correctly rounded to
/// `fmt` in `mode`: starts at `fmt.prec + 32` bits and doubles up to `max_prec`; returns the
/// encoding and flags.
///
/// The true value lies in `[v - |v| 2^-p, v + |v| 2^-p]`; rounding is monotone and so is the
/// tininess test, so if both ends round to the same encoding with the same flags, so does the
/// true value. Exact results (`Approx::exact`) are rounded directly, which handles values
/// that are representable or exactly halfway.
///
/// Values approaching a representable asymptote need a precision of about `-log2` of their
/// distance to it in directed modes (`tanh(20)` rounds down to `1 - 2^-53` only once the
/// working precision exceeds about 58 bits, `cos(2^-600)` only beyond about 1200).
pub fn correctly_rounded(
    fmt: Fmt,
    mode: Round,
    max_prec: u32,
    f: impl Fn(u32) -> Approx,
) -> Result<(u128, Rounded), Undetermined> {
    let mut p = fmt.prec.saturating_add(32);
    loop {
        let a = f(p);
        if a.exact || !a.value.is_finite() {
            return Ok(a.value.round_to(fmt, mode));
        }
        if !a.value.is_zero() {
            let d = a.value.abs().mul_pow2(-i64::from(p));
            let lo = a.value.sub(&d).round_to(fmt, mode);
            let hi = a.value.add(&d).round_to(fmt, mode);
            if lo == hi {
                return Ok(lo);
            }
        }
        if p >= max_prec {
            return Err(Undetermined { value: a.value });
        }
        p = p.saturating_mul(2).min(max_prec);
    }
}

/// The error of `got` (an encoding of `fmt`) against `exact` (an approximation at
/// `>= 2 fmt.prec + 20` bits of the true value), in units in the last place of the format at
/// the exact value: `2^(max(e, emin) - prec + 1)` with `e = floor(log2|exact|)`.
///
/// Infinite or NaN cases: 0 if both are the same special value, infinity otherwise (so a finite
/// exact value beyond the format's range against an infinite result counts as infinite error).
pub fn ulp_error(fmt: Fmt, got: u128, exact: &Big) -> f64 {
    let g = Big::from_bits(fmt, got);
    if g.is_nan() || exact.is_nan() {
        return if g.is_nan() && exact.is_nan() { 0.0 } else { f64::INFINITY };
    }
    if g.is_inf() || exact.is_inf() {
        return if g.is_inf() && exact.is_inf() && g.is_neg() == exact.is_neg() { 0.0 } else { f64::INFINITY };
    }
    let emin = i64::from(fmt.emin());
    let e = exact.ilogb().unwrap_or(emin).max(emin);
    let q = e - i64::from(fmt.prec) + 1;
    g.sub(exact).abs().mul_pow2(-q).to_f64()
}
