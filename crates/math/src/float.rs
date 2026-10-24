//! The generic `Float` trait over binary32, binary64, x87 extended and binary128 (D-10.1), the
//! decoded form of a value, and the one rounding routine every correctly rounded operation
//! ends in.
//!
//! Every format is handled through its encoding as an integer (`u128`), so the two formats Rust
//! has no type for (x87 `long double`, `_Float128`) work exactly like `f32` and `f64` (OD-C4).
//! Underflow is detected after rounding, as x86 does in both of its units: a result is tiny when
//! its value rounded to the format's precision with an unbounded exponent is below the smallest
//! normal magnitude.

#![allow(clippy::as_conversions, clippy::arithmetic_side_effects)] // bit fields of bounded width

use core::fmt::Debug;

use crate::status::{Flags, Rounding, Status};

/// A binary floating-point format and its values.
pub trait Float: Copy + Debug + PartialEq {
    /// Significand precision in bits, including the leading bit.
    const PREC: u32;
    /// Largest unbiased exponent.
    const EMAX: i32;
    /// Width of the exponent field.
    const EXP_BITS: u32;
    /// True if the leading significand bit is stored (x87).
    const EXPLICIT: bool;
    /// The encoding (the low bits of the result).
    fn to_bits(self) -> u128;
    /// The value of an encoding (bits above the format's width are ignored).
    fn from_bits(b: u128) -> Self;
}

impl Float for f32 {
    const PREC: u32 = 24;
    const EMAX: i32 = 127;
    const EXP_BITS: u32 = 8;
    const EXPLICIT: bool = false;
    fn to_bits(self) -> u128 {
        u128::from(f32::to_bits(self))
    }
    fn from_bits(b: u128) -> f32 {
        f32::from_bits(b as u32)
    }
}

impl Float for f64 {
    const PREC: u32 = 53;
    const EMAX: i32 = 1023;
    const EXP_BITS: u32 = 11;
    const EXPLICIT: bool = false;
    fn to_bits(self) -> u128 {
        u128::from(f64::to_bits(self))
    }
    fn from_bits(b: u128) -> f64 {
        f64::from_bits(b as u64)
    }
}

/// An x87 80-bit extended value (`long double`, `_Float64x`): its encoding in the low 80 bits.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct F80(pub u128);

impl Float for F80 {
    const PREC: u32 = 64;
    const EMAX: i32 = 16383;
    const EXP_BITS: u32 = 15;
    const EXPLICIT: bool = true;
    fn to_bits(self) -> u128 {
        self.0
    }
    fn from_bits(b: u128) -> F80 {
        F80(b & ((1u128 << 80) - 1))
    }
}

/// An IEEE binary128 value (`_Float128`): its encoding.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct F128(pub u128);

impl Float for F128 {
    const PREC: u32 = 113;
    const EMAX: i32 = 16383;
    const EXP_BITS: u32 = 15;
    const EXPLICIT: bool = false;
    fn to_bits(self) -> u128 {
        self.0
    }
    fn from_bits(b: u128) -> F128 {
        F128(b)
    }
}

/// The class of an encoding.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// ±0.
    Zero,
    /// A nonzero finite value (normal, subnormal, or an x87 pseudo-denormal).
    Finite,
    /// ±∞.
    Inf,
    /// A quiet NaN.
    QNan,
    /// A signalling NaN.
    SNan,
    /// An x87 encoding IEEE does not define (unnormal, pseudo-infinity, pseudo-NaN): the x87
    /// unit treats it as an invalid operand.
    Invalid,
}

/// A decoded value. For `Finite`, the value is `sig · 2^exp` with `0 < sig < 2^PREC`; for NaNs
/// `sig` is the payload (the fraction bits below the quiet bit).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Num {
    /// Sign bit.
    pub neg: bool,
    /// Class.
    pub kind: Kind,
    /// Significand (finite) or payload (NaN).
    pub sig: u128,
    /// Exponent of `sig`'s least significant bit (finite).
    pub exp: i32,
}

impl Num {
    /// A NaN of either kind.
    pub fn is_nan(&self) -> bool {
        matches!(self.kind, Kind::QNan | Kind::SNan)
    }

    /// The exponent of the leading bit (`floor(log2 |x|)`) of a finite value.
    pub fn top(&self) -> i32 {
        self.exp + 127 - self.sig.leading_zeros() as i32
    }
}

/// Stored fraction bits.
pub(crate) const fn frac_bits<F: Float>() -> u32 {
    if F::EXPLICIT { F::PREC } else { F::PREC - 1 }
}

/// The sign bit of the encoding.
pub(crate) const fn sign_bit<F: Float>() -> u128 {
    1u128 << (frac_bits::<F>() + F::EXP_BITS)
}

const fn exp_field_max<F: Float>() -> u128 {
    (1u128 << F::EXP_BITS) - 1
}

/// The smallest normal exponent.
pub const fn emin<F: Float>() -> i32 {
    1 - F::EMAX
}

/// Exponent of the least significant bit of a subnormal (the smallest `exp` of a `Finite`).
pub const fn lsb_min<F: Float>() -> i32 {
    emin::<F>() - (F::PREC as i32 - 1)
}

/// The quiet bit within the stored fraction.
pub(crate) const fn quiet_bit<F: Float>() -> u128 {
    1u128 << (F::PREC - 2)
}

/// The leading (integer) bit of a normal significand.
pub(crate) const fn lead_bit<F: Float>() -> u128 {
    1u128 << (F::PREC - 1)
}

/// Decode `x`.
pub fn decode<F: Float>(x: F) -> Num {
    decode_bits::<F>(x.to_bits())
}

/// Decode `x` through an optimisation barrier. The compiler may turn bit tests of an `f32` or
/// `f64` back into floating comparisons (it assumes the default floating-point environment),
/// and a comparison with a signalling NaN raises invalid: the classification functions, which
/// must raise nothing, decode this way.
pub fn decode_opaque<F: Float>(x: F) -> Num {
    decode_bits::<F>(core::hint::black_box(x.to_bits()))
}

/// Decode the encoding `b`.
pub fn decode_bits<F: Float>(b: u128) -> Num {
    let fb = frac_bits::<F>();
    let frac = b & ((1u128 << fb) - 1);
    let e = (b >> fb) & exp_field_max::<F>();
    let neg = b & sign_bit::<F>() != 0;
    let nan = |low: u128| {
        let kind = if low & quiet_bit::<F>() != 0 { Kind::QNan } else { Kind::SNan };
        Num { neg, kind, sig: low & (quiet_bit::<F>() - 1), exp: 0 }
    };
    let finite = |sig: u128, exp: i32| Num { neg, kind: Kind::Finite, sig, exp };
    let other = |kind: Kind| Num { neg, kind, sig: 0, exp: 0 };
    let unbias = |e: u128| e as i32 - F::EMAX - (F::PREC as i32 - 1);
    if F::EXPLICIT {
        let lead = frac & lead_bit::<F>() != 0;
        let low = frac & (lead_bit::<F>() - 1);
        match (e, lead) {
            (0, _) if frac == 0 => other(Kind::Zero),
            (0, _) => finite(frac, lsb_min::<F>()),
            (e, true) if e == exp_field_max::<F>() => {
                if low == 0 { other(Kind::Inf) } else { nan(low) }
            }
            (_, false) => other(Kind::Invalid),
            (e, true) => finite(frac, unbias(e)),
        }
    } else {
        match e {
            0 if frac == 0 => other(Kind::Zero),
            0 => finite(frac, lsb_min::<F>()),
            e if e == exp_field_max::<F>() => {
                if frac == 0 { other(Kind::Inf) } else { nan(frac) }
            }
            e => finite(frac | lead_bit::<F>(), unbias(e)),
        }
    }
}

/// ±0.
pub fn zero<F: Float>(neg: bool) -> F {
    F::from_bits(if neg { sign_bit::<F>() } else { 0 })
}

/// ±∞.
pub fn inf<F: Float>(neg: bool) -> F {
    let lead = if F::EXPLICIT { lead_bit::<F>() } else { 0 };
    F::from_bits(if neg { sign_bit::<F>() } else { 0 } | (exp_field_max::<F>() << frac_bits::<F>()) | lead)
}

/// The largest finite magnitude, with sign.
pub fn max_finite<F: Float>(neg: bool) -> F {
    pack(neg, (1u128 << F::PREC) - 1, F::EMAX - (F::PREC as i32 - 1))
}

/// A NaN with a payload, quiet or signalling (a signalling NaN needs a nonzero payload).
pub fn nan<F: Float>(neg: bool, quiet: bool, payload: u128) -> F {
    let lead = if F::EXPLICIT { lead_bit::<F>() } else { 0 };
    let q = if quiet { quiet_bit::<F>() } else { 0 };
    let p = payload & (quiet_bit::<F>() - 1);
    F::from_bits(if neg { sign_bit::<F>() } else { 0 } | (exp_field_max::<F>() << frac_bits::<F>()) | lead | q | p)
}

/// The NaN an invalid operation produces: x86's "real indefinite", a quiet NaN with the sign
/// set and no payload (what SSE and x87 arithmetic return).
pub fn default_nan<F: Float>() -> F {
    nan(true, true, 0)
}

/// A NaN operand quieted (sign and payload kept).
pub fn quiet<F: Float>(n: &Num) -> F {
    nan(n.neg, true, n.sig)
}

/// The value with its sign bit set to `neg` (any encoding, NaNs included).
pub fn with_sign<F: Float>(x: F, neg: bool) -> F {
    let b = x.to_bits() & !sign_bit::<F>();
    F::from_bits(if neg { b | sign_bit::<F>() } else { b })
}

/// Encode a finite value `sig · 2^exp` that is representable: normalized to `PREC` bits or a
/// subnormal at the smallest exponent. A zero `sig` gives a signed zero.
pub fn pack<F: Float>(neg: bool, sig: u128, exp: i32) -> F {
    let s = if neg { sign_bit::<F>() } else { 0 };
    if sig == 0 {
        return F::from_bits(s);
    }
    // Normalize toward the leading bit, stopping at the subnormal exponent.
    let len = 128 - sig.leading_zeros() as i32;
    let want = F::PREC as i32;
    let (m, x) = if len < want {
        let up = (want - len).min((exp - lsb_min::<F>()).max(0));
        (sig << up, exp - up)
    } else {
        (sig >> (len - want), exp + (len - want))
    };
    if m & lead_bit::<F>() != 0 {
        let biased = (x + (F::PREC as i32 - 1) + F::EMAX) as u128;
        let stored = if F::EXPLICIT { m } else { m & (lead_bit::<F>() - 1) };
        F::from_bits(s | (biased << frac_bits::<F>()) | stored)
    } else {
        F::from_bits(s | m)
    }
}

/// The exact value `(sig + f) · 2^exp`, where `0 <= f < 1` is unknown except that it is nonzero
/// iff `sticky`. Whenever `sticky` is set, `sig` has at least `PREC + 2` significant bits, so the
/// first discarded bit is in `sig`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Exact {
    /// Sign.
    pub neg: bool,
    /// Integer significand.
    pub sig: u128,
    /// Exponent of `sig`'s least significant bit.
    pub exp: i32,
    /// Bits below `sig` are nonzero.
    pub sticky: bool,
}

/// `sig` without its low `drop` bits, rounded in `mode`: the kept bits and whether anything
/// nonzero was discarded.
pub(crate) fn shed(sig: u128, drop: i32, sticky: bool, neg: bool, mode: Rounding) -> (u128, bool) {
    if drop <= 0 {
        return (sig << (-drop).min(127), sticky);
    }
    let d = drop as u32;
    let kept = if d >= 128 { 0 } else { sig >> d };
    let rem = if d >= 128 { sig } else { sig & ((1u128 << d) - 1) };
    let inexact = rem != 0 || sticky;
    let (above, tie) = if d > 128 {
        (false, false)
    } else {
        let half = 1u128 << (d - 1);
        (rem > half || (rem == half && sticky), rem == half && !sticky)
    };
    let up = match mode {
        Rounding::Nearest => above || (tie && kept & 1 == 1),
        Rounding::Up => inexact && !neg,
        Rounding::Down => inexact && neg,
        Rounding::Zero => false,
    };
    (kept + u128::from(up), inexact)
}

const UNDERFLOW_INEXACT: Flags = Flags(Flags::UNDERFLOW.0 | Flags::INEXACT.0);

/// Round an exact value into `F` in `mode`: the result and the flags (inexact, overflow,
/// underflow after rounding). Division by zero and invalid are the caller's.
pub fn round<F: Float>(x: Exact, mode: Rounding) -> (F, Flags) {
    let Exact { neg, sig, exp, sticky } = x;
    let prec = F::PREC as i32;
    if sig == 0 {
        // Only a nonzero remainder below any representable bit: a tiny value.
        let (m, inexact) = shed(0, 1, sticky, neg, mode);
        return (pack(neg, m, lsb_min::<F>()), if inexact { UNDERFLOW_INEXACT } else { Flags::NONE });
    }
    let len = 128 - sig.leading_zeros() as i32;
    let top = exp + len - 1;
    // Discard down to PREC bits, or to the subnormal exponent if that keeps fewer.
    let drop = (len - prec).max(lsb_min::<F>() - exp);
    let (mut m, inexact) = shed(sig, drop, sticky, neg, mode);
    let mut lsb = exp + drop;
    if m >> prec != 0 {
        m >>= 1;
        lsb += 1;
    }
    if m == 0 {
        return (zero(neg), if inexact { UNDERFLOW_INEXACT } else { Flags::NONE });
    }
    let lead = lsb + 127 - m.leading_zeros() as i32;
    if lead > F::EMAX {
        let to_inf = match mode {
            Rounding::Nearest => true,
            Rounding::Up => !neg,
            Rounding::Down => neg,
            Rounding::Zero => false,
        };
        let v = if to_inf { inf(neg) } else { max_finite(neg) };
        return (v, Flags(Flags::OVERFLOW.0 | Flags::INEXACT.0));
    }
    let mut flags = if inexact { Flags::INEXACT } else { Flags::NONE };
    if inexact && top < emin::<F>() {
        // Tiny unless rounding to PREC bits with an unbounded exponent reaches 2^emin, which
        // only a value whose leading bit is just below it can do.
        let tiny = top < emin::<F>() - 1 || {
            let (r, _) = shed(sig, len - prec, sticky, neg, mode);
            r >> prec == 0
        };
        if tiny {
            flags = flags.union(Flags::UNDERFLOW);
        }
    }
    (pack(neg, m, lsb), flags)
}

/// `ERANGE` as glibc's functions report it: for an overflow whose result is infinite and an
/// underflow whose result is zero, not for a result rounded to the largest finite value or to a
/// subnormal (`spec/behaviour/libm-exact.glibc.txt.gz`).
pub fn range_errno<F: Float>(v: F, st: Status) -> Status {
    let k = decode(v).kind;
    let range = (st.flags.any(Flags::OVERFLOW) && k == Kind::Inf) || (st.flags.any(Flags::UNDERFLOW) && k == Kind::Zero);
    if range { st.with(crate::status::MathError::Range) } else { st }
}

/// A NaN payload moved between formats: the fraction bits aligned at the top.
pub(crate) fn rescale_payload<F: Float, T: Float>(p: u128) -> u128 {
    let (from, to) = (F::PREC as i32 - 2, T::PREC as i32 - 2);
    if to >= from { p << (to - from) } else { p >> (from - to) }
}

/// Convert a value between formats, rounding in `mode` (exact when widening). A signalling NaN
/// raises invalid and is quieted; NaN payloads keep their high bits.
pub fn convert<F: Float, T: Float>(x: F, mode: Rounding) -> (T, Status) {
    let n = decode(x);
    match n.kind {
        Kind::Zero => (zero(n.neg), Status::OK),
        Kind::Inf => (inf(n.neg), Status::OK),
        Kind::QNan => (nan(n.neg, true, rescale_payload::<F, T>(n.sig)), Status::OK),
        Kind::SNan => (nan(n.neg, true, rescale_payload::<F, T>(n.sig)), Status::INVALID),
        Kind::Invalid => (default_nan(), Status::INVALID),
        Kind::Finite => {
            let (v, f) = round::<T>(Exact { neg: n.neg, sig: n.sig, exp: n.exp, sticky: false }, mode);
            (v, Status::flags(f))
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::float_cmp, clippy::arithmetic_side_effects, clippy::as_conversions)]
mod tests {
    use super::*;

    #[test]
    fn decode_pack_round_trip_f64() {
        for x in [0.0f64, -0.0, 1.0, -2.5, f64::MIN_POSITIVE, 5e-324, f64::MAX, 0.1, 1e300, -3e-310] {
            let n = decode(x);
            assert_eq!(n.kind, if x == 0.0 { Kind::Zero } else { Kind::Finite });
            let back: f64 = pack(n.neg, n.sig, n.exp);
            assert_eq!(back.to_bits(), x.to_bits(), "{x}");
        }
        assert_eq!(decode(f64::INFINITY).kind, Kind::Inf);
        assert_eq!(decode(f64::NAN).kind, Kind::QNan);
        assert_eq!(decode(f64::from_bits(0x7ff4_0000_0000_0000)).kind, Kind::SNan);
        assert_eq!(f64::to_bits(default_nan()), 0xfff8_0000_0000_0000);
        assert_eq!(f64::to_bits(max_finite(false)), f64::MAX.to_bits());
        assert_eq!(f64::to_bits(inf(true)), f64::NEG_INFINITY.to_bits());
    }

    #[test]
    fn x87_encodings() {
        let one = F80((0x3fffu128 << 64) | (1u128 << 63));
        let n = decode(one);
        assert_eq!((n.kind, n.sig, n.exp), (Kind::Finite, 1u128 << 63, -63));
        assert_eq!(pack::<F80>(false, 1, 0), one);
        assert_eq!(decode(F80(0x3fffu128 << 64)).kind, Kind::Invalid); // unnormal
        assert_eq!(decode(F80(0x7fffu128 << 64)).kind, Kind::Invalid); // pseudo-infinity
        assert_eq!(decode(F80(1u128 << 63)).kind, Kind::Finite); // pseudo-denormal
        assert_eq!(inf::<F80>(false), F80((0x7fffu128 << 64) | (1u128 << 63)));
        assert_eq!(default_nan::<F80>(), F80((0xffffu128 << 64) | (3u128 << 62)));
    }

    #[test]
    fn rounding_matches_hardware_f32_from_f64() {
        // Exact f64 values near the f32 subnormal and overflow ranges: rounding them to f32
        // must equal the hardware conversion.
        for q in (1u128..3000).chain([(1 << 30) + 1, (1 << 30) - 1, 0xffff_ffff, 0x1_0000_0001]) {
            for exp in [-180, -160, -150, -149, -148, -127, -126, -125, -30, 0, 10, 100, 104, 105] {
                let exact = (q as f64) * 2f64.powi(exp);
                let want = exact as f32;
                let (got, _) = round::<f32>(Exact { neg: false, sig: q, exp, sticky: false }, Rounding::Nearest);
                assert_eq!(got.to_bits(), want.to_bits(), "q={q} exp={exp}");
            }
        }
    }

    #[test]
    fn tininess_is_detected_after_rounding() {
        // (2^24 - 1) · 2^-150 has 24 bits and lies just below 2^-126: on the subnormal grid it
        // rounds to 2^-126, but with an unbounded exponent it is representable as it is, so it
        // is tiny: inexact and underflow.
        let (v, f) = round::<f32>(Exact { neg: false, sig: (1 << 24) - 1, exp: -150, sticky: false }, Rounding::Nearest);
        assert_eq!(v, f32::MIN_POSITIVE);
        assert_eq!(f, Flags(0x30));
        // (2^25 - 1) · 2^-151 rounds to 2^-126 at 24 bits as well: not tiny.
        let (v, f) = round::<f32>(Exact { neg: false, sig: (1 << 25) - 1, exp: -151, sticky: false }, Rounding::Nearest);
        assert_eq!(v, f32::MIN_POSITIVE);
        assert_eq!(f, Flags::INEXACT);
        // Exact subnormals raise nothing.
        let (v, f) = round::<f32>(Exact { neg: true, sig: 3, exp: -149, sticky: false }, Rounding::Nearest);
        assert_eq!(v.to_bits(), 0x8000_0003);
        assert_eq!(f, Flags::NONE);
    }

    #[test]
    fn overflow_follows_the_mode() {
        let big = Exact { neg: false, sig: 1, exp: 128, sticky: false };
        assert_eq!(round::<f32>(big, Rounding::Nearest).0, f32::INFINITY);
        assert_eq!(round::<f32>(big, Rounding::Zero).0, f32::MAX);
        assert_eq!(round::<f32>(Exact { neg: true, ..big }, Rounding::Up).0, f32::MIN);
        assert_eq!(round::<f32>(big, Rounding::Up).1, Flags(0x28));
    }
}
