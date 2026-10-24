//! The correctly rounded arithmetic of spec §10.3 (`sqrt`) and §10.4: `fma`, and the narrowing
//! operations, which compute exactly in the operands' format and round once into a narrower one
//! (`fadd`, `daddl`, `f32mulf128`, ...). One generic implementation per operation, over the source
//! format `F` and the result format `T` (`T = F` for `sqrt` and `fma`).
//!
//! NaN operands: a signalling NaN raises invalid; the result is a quiet NaN with the payload of
//! one operand, rescaled to `T`. Which operand ([`Pick`]) is not specified by IEEE 754; Liella
//! picks the one glibc returns (observed, `spec/behaviour/libm-exact.glibc.txt.gz`), because
//! payloads are exact results: for `long double` and `_Float128` the x87 rule (a quiet NaN over
//! a signalling one, then the larger significand), for `float` and `double` the operand order
//! of each function.

use crate::arith;
use crate::float::{self, Float, Kind, Num};
use crate::status::{MathError, Rounding, Status};

/// Which NaN operand supplies the result's payload.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Pick {
    /// The format's rule: [`Pick::X87`] for x87 and binary128, the first NaN operand otherwise.
    Format,
    /// For `float` and `double`, the first NaN in this order of operand positions (the format's
    /// rule otherwise).
    Order([usize; 3]),
    /// A quiet NaN over a signalling one, then the larger significand (ties: the first).
    X87,
    /// A quiet NaN over a signalling one, then the first (in every format).
    QuietFirst,
}

/// The quiet NaN result of an operation with NaN operands, if any, and the status.
pub(crate) fn nan_operands<F: Float, T: Float>(ops: &[Num]) -> Option<(T, Status)> {
    nan_operands_by::<F, T>(ops, Pick::Format)
}

/// [`nan_operands`] with an explicit choice of operand.
pub(crate) fn nan_operands_by<F: Float, T: Float>(ops: &[Num], pick: Pick) -> Option<(T, Status)> {
    let signalling = ops.iter().any(|n| n.kind == Kind::SNan);
    let invalid_enc = ops.iter().any(|n| n.kind == Kind::Invalid);
    let status = if signalling || invalid_enc { Status::INVALID } else { Status::OK };
    let pick = match pick {
        Pick::QuietFirst => Pick::QuietFirst,
        _ if F::PREC > 53 => Pick::X87,
        Pick::Format => Pick::Order([0, 1, 2]),
        p => p,
    };
    // The x87 unit's rule for `long double` (probed, X87-NAN-RULE-AUDIT): an invalid encoding
    // beside a NaN gives the default NaN, and of two NaNs of one kind with equal significands
    // the positive one.
    let unit = F::PREC == 64 && pick == Pick::X87;
    if unit && invalid_enc {
        return Some((float::default_nan(), Status::INVALID));
    }
    let chosen: Option<&Num> = match pick {
        Pick::Order(order) => order.iter().filter_map(|&i| ops.get(i)).find(|n| n.is_nan()),
        _ => ops.iter().filter(|n| n.is_nan()).fold(None, |c: Option<&Num>, n| match c {
            None => Some(n),
            Some(c) => {
                let better = match (c.kind, n.kind) {
                    (Kind::SNan, Kind::QNan) => true,
                    (Kind::QNan, Kind::SNan) => false,
                    _ => pick == Pick::X87 && (n.sig > c.sig || (unit && n.sig == c.sig && c.neg && !n.neg)),
                };
                Some(if better { n } else { c })
            }
        }),
    };
    match chosen {
        Some(n) => Some((float::nan(n.neg, true, float::rescale_payload::<F, T>(n.sig)), status)),
        None if invalid_enc => Some((float::default_nan(), Status::INVALID)),
        None => None,
    }
}

/// An invalid operation: the default NaN.
fn invalid<T: Float>(error: MathError) -> (T, Status) {
    (float::default_nan(), Status::INVALID.with(error))
}

/// The finite result of an exact computation, rounded once into `T`.
fn finish<T: Float>(e: float::Exact, mode: Rounding, errno: bool) -> (T, Status) {
    let (v, flags) = float::round::<T>(e, mode);
    let st = Status::flags(flags);
    (v, if errno { float::range_errno(v, st) } else { st })
}

/// `x + y`, rounded once into `T` (`fadd`, `daddl`, `f32addf128`, ...). Overflow and
/// underflow report `ERANGE`; ∞ − ∞ is a domain error.
pub fn add<F: Float, T: Float>(x: F, y: F, mode: Rounding) -> (T, Status) {
    let (a, b) = (float::decode(x), float::decode(y));
    if let Some(r) = nan_operands::<F, T>(&[a, b]) {
        return r;
    }
    match (a.kind, b.kind) {
        (Kind::Inf, Kind::Inf) if a.neg != b.neg => invalid(MathError::Domain),
        (Kind::Inf, _) => (float::inf(a.neg), Status::OK),
        (_, Kind::Inf) => (float::inf(b.neg), Status::OK),
        _ => finish(arith::add_exact(&a, &b, mode), mode, true),
    }
}

/// `x − y`, rounded once into `T`.
pub fn sub<F: Float, T: Float>(x: F, y: F, mode: Rounding) -> (T, Status) {
    let b = float::decode(y);
    if b.is_nan() || b.kind == Kind::Invalid {
        return add::<F, T>(x, y, mode);
    }
    add::<F, T>(x, float::with_sign(y, !b.neg), mode)
}

/// `x · y`, rounded once into `T`. ∞ · 0 is a domain error.
pub fn mul<F: Float, T: Float>(x: F, y: F, mode: Rounding) -> (T, Status) {
    let (a, b) = (float::decode(x), float::decode(y));
    if let Some(r) = nan_operands::<F, T>(&[a, b]) {
        return r;
    }
    let neg = a.neg != b.neg;
    match (a.kind, b.kind) {
        (Kind::Inf, Kind::Zero) | (Kind::Zero, Kind::Inf) => invalid(MathError::Domain),
        (Kind::Inf, _) | (_, Kind::Inf) => (float::inf(neg), Status::OK),
        _ => finish(arith::mul_exact(&a, &b), mode, true),
    }
}

/// `x / y`, rounded once into `T`. 0/0 and ∞/∞ are domain errors; a finite nonzero over zero
/// is a pole.
pub fn div<F: Float, T: Float>(x: F, y: F, mode: Rounding) -> (T, Status) {
    let (a, b) = (float::decode(x), float::decode(y));
    if let Some(r) = nan_operands::<F, T>(&[a, b]) {
        return r;
    }
    let neg = a.neg != b.neg;
    match (a.kind, b.kind) {
        (Kind::Zero, Kind::Zero) | (Kind::Inf, Kind::Inf) => invalid(MathError::Domain),
        (Kind::Inf, _) => (float::inf(neg), Status::OK),
        (_, Kind::Inf) => (float::zero(neg), Status::OK),
        (_, Kind::Zero) => (float::inf(neg), Status::POLE),
        _ => finish(arith::div_exact::<T>(&a, &b), mode, true),
    }
}

/// The square root, rounded once into `T` (`sqrt` with `T = F`, and `fsqrt`, `dsqrtl`, ...).
/// `sqrt(−0)` is −0; a negative operand is a domain error.
pub fn sqrt<F: Float, T: Float>(x: F, mode: Rounding) -> (T, Status) {
    let a = float::decode(x);
    if let Some(r) = nan_operands::<F, T>(&[a]) {
        return r;
    }
    match a.kind {
        Kind::Zero => (float::zero(a.neg), Status::OK),
        _ if a.neg => invalid(MathError::Domain),
        Kind::Inf => (float::inf(false), Status::OK),
        _ => finish(arith::sqrt_exact::<T>(&a), mode, true),
    }
}

/// `x · y + z` with one rounding into `T` (`fma` with `T = F`, and `ffma`, `dfmal`, ...).
/// `fma` reports no `errno` (its contract); the narrowing forms report as the other narrowing
/// operations do (`errno` selects which).
pub fn fma_to<F: Float, T: Float>(x: F, y: F, z: F, mode: Rounding, errno: bool) -> (T, Status) {
    let (a, b, c) = (float::decode(x), float::decode(y), float::decode(z));
    // An invalid x87 encoding in any position, beside NaNs too: the default NaN (D-10.20, #902).
    if F::PREC == 64 && [a.kind, b.kind, c.kind].contains(&Kind::Invalid) {
        return (float::default_nan(), Status::INVALID);
    }
    let inf_zero = matches!((a.kind, b.kind), (Kind::Inf, Kind::Zero) | (Kind::Zero, Kind::Inf));
    // NaN operands, as glibc's `fma` propagates them: for `float` and `double` the hardware FMA's
    // operand order (y, x, z); for x87 and binary128 the product's NaN (by the x87 rule), then
    // the x87 rule between it and the addend. ∞ · 0 with a quiet NaN addend raises nothing.
    // In those two formats ∞ · 0 is the default NaN with invalid even then; added to a NaN it
    // gives the addend's NaN (x87) or itself (binary128).
    if inf_zero && c.is_nan() && F::PREC > 53 {
        let v = if F::EXPLICIT { float::nan(c.neg, true, float::rescale_payload::<F, T>(c.sig)) } else { float::default_nan() };
        return (v, Status::INVALID);
    }
    let nan = if F::PREC > 53 {
        match nan_operands_by::<F, F>(&[a, b], Pick::X87) {
            Some((p, st)) => nan_operands_by::<F, T>(&[float::decode(p), c], Pick::X87).map(|(v, s2)| (v, st.and(s2))),
            None => nan_operands_by::<F, T>(&[c], Pick::X87),
        }
    } else {
        nan_operands_by::<F, T>(&[a, b, c], Pick::Order([1, 0, 2]))
    };
    if let Some(r) = nan {
        return r;
    }
    let domain = if errno { MathError::Domain } else { MathError::None };
    let pneg = a.neg != b.neg;
    match (a.kind, b.kind, c.kind) {
        _ if inf_zero => invalid(domain),
        (Kind::Inf, _, Kind::Inf) | (_, Kind::Inf, Kind::Inf) if pneg != c.neg => invalid(domain),
        (Kind::Inf, _, _) | (_, Kind::Inf, _) => (float::inf(pneg), Status::OK),
        (_, _, Kind::Inf) => (float::inf(c.neg), Status::OK),
        _ => finish(arith::fma_exact(&a, &b, &c, mode), mode, errno),
    }
}

/// `fma(x, y, z)`: `x · y + z` with one rounding. Never sets `errno` (glibc's documented
/// behaviour, `fma(3)`).
pub fn fma<F: Float>(x: F, y: F, z: F, mode: Rounding) -> (F, Status) {
    fma_to::<F, F>(x, y, z, mode, false)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp, clippy::arithmetic_side_effects, clippy::as_conversions)]
mod tests {
    use super::*;
    use crate::status::Flags;

    /// A small deterministic generator (xorshift64*).
    fn rng(seed: &mut u64) -> u64 {
        *seed ^= *seed >> 12;
        *seed ^= *seed << 25;
        *seed ^= *seed >> 27;
        seed.wrapping_mul(0x2545_f491_4f6c_dd1d)
    }

    /// Random f64 operands with exponents near each other, plus subnormals and specials.
    fn operand(seed: &mut u64) -> f64 {
        let r = rng(seed);
        match r % 16 {
            0 => f64::from_bits(rng(seed) & 0x000f_ffff_ffff_ffff),
            1 => [0.0, -0.0, f64::INFINITY, f64::NEG_INFINITY, f64::MAX, f64::MIN_POSITIVE][(r >> 8) as usize % 6],
            _ => f64::from_bits((rng(seed) & 0x800f_ffff_ffff_ffff) | ((0x3c0 + (r >> 8) % 128) << 52)),
        }
    }

    #[test]
    fn f64_arithmetic_equals_the_hardware_in_round_to_nearest() {
        let mut seed = 0x1234_5678_9abc_def0;
        for _ in 0..200_000 {
            let (x, y, z) = (operand(&mut seed), operand(&mut seed), operand(&mut seed));
            let check = |name: &str, got: f64, want: f64| {
                assert!(got.to_bits() == want.to_bits() || (got.is_nan() && want.is_nan()), "{name} {x:e} {y:e} {z:e}: {got:e} vs {want:e}");
            };
            check("add", add::<f64, f64>(x, y, Rounding::Nearest).0, x + y);
            check("sub", sub::<f64, f64>(x, y, Rounding::Nearest).0, x - y);
            check("mul", mul::<f64, f64>(x, y, Rounding::Nearest).0, x * y);
            check("div", div::<f64, f64>(x, y, Rounding::Nearest).0, x / y);
            check("sqrt", sqrt::<f64, f64>(x, Rounding::Nearest).0, x.sqrt());
            check("fma", fma(x, y, z, Rounding::Nearest).0, x.mul_add(y, z));
            check("fadd", f64::from(add::<f64, f32>(x, y, Rounding::Nearest).0), f64::from(narrow_ref(x, y)));
        }
    }

    /// `x + y` rounded once to f32, by an independent route: the exact sum as two f64 (TwoSum),
    /// then rounding the pair with round-to-odd on the high part.
    fn narrow_ref(x: f64, y: f64) -> f32 {
        let s = x + y;
        if !s.is_finite() {
            return s as f32;
        }
        let bb = s - x;
        let err = (x - (s - bb)) + (y - bb);
        // Round to odd: if the f64 sum was inexact, force its last bit to 1 toward the error.
        let hi = if err == 0.0 {
            s
        } else {
            let b = s.to_bits();
            let adj = if (err > 0.0) == (s > 0.0) { b | 1 } else if b & 1 == 1 { b } else { b - 1 };
            f64::from_bits(adj)
        };
        hi as f32
    }

    #[test]
    fn exact_zero_signs_and_specials() {
        assert_eq!(add::<f64, f64>(1.0, -1.0, Rounding::Nearest).0.to_bits(), 0);
        assert_eq!(add::<f64, f64>(1.0, -1.0, Rounding::Down).0.to_bits(), 0x8000_0000_0000_0000);
        assert_eq!(fma(-0.0f64, 1.0, -0.0, Rounding::Nearest).0.to_bits(), 0x8000_0000_0000_0000);
        assert_eq!(sqrt::<f64, f64>(-0.0, Rounding::Nearest).0.to_bits(), 0x8000_0000_0000_0000);
        let (v, st) = sqrt::<f64, f64>(-1.0, Rounding::Nearest);
        assert_eq!((v.to_bits(), st), (0xfff8_0000_0000_0000, Status::DOMAIN));
        let (_, st) = fma(f64::INFINITY, 0.0, f64::NAN, Rounding::Nearest);
        assert_eq!(st, Status::OK); // glibc: ∞ · 0 + qNaN raises nothing (float, double)
        let (v, st) = div::<f64, f64>(1.0, -0.0, Rounding::Nearest);
        assert_eq!((v, st), (f64::NEG_INFINITY, Status::POLE));
    }

    #[test]
    fn f128_sqrt_and_fma_are_exact_where_they_must_be() {
        use crate::float::F128;
        let two = F128(0x4000u128 << 112);
        let four = F128(0x4001u128 << 112);
        assert_eq!(sqrt::<F128, F128>(four, Rounding::Nearest), (two, Status::OK));
        // sqrt(2)^2 rounded is not 2: the product of the rounded root with itself, minus 2,
        // computed by fma, is the exact (small) error.
        let (r, st) = sqrt::<F128, F128>(two, Rounding::Nearest);
        assert_eq!(st.flags, Flags::INEXACT);
        let neg_two = F128(two.0 | (1u128 << 127));
        let (e, _) = fma(r, r, neg_two, Rounding::Nearest);
        let n = float::decode(e);
        assert_eq!(n.kind, Kind::Finite);
        assert!(n.top() < -110, "{n:?}");
    }
}
