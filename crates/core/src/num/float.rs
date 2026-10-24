//! Binary floating-point formats as data (IP-18): binary32, binary64, the x87 80-bit extended
//! format (`long double`) and binary128 (`_Float128`), with exact packing and one rounding
//! routine that every conversion uses. Values travel as raw bits (`u128`), so formats Rust has
//! no type for are handled the same way as the ones it has (OD-C4).
//!
//! Rewritten from the COLD-REWRITE-W-far-05 decisions (D-8.11
//! (3)): one rounding step at a computed least-significant-bit exponent, shared by normal and
//! subnormal results; underflow is inexact-and-tiny with tininess judged after rounding to the
//! full precision with an unbounded exponent (probed against glibc's `strtod`/`strtof`).

#![allow(clippy::as_conversions, clippy::arithmetic_side_effects)] // bit-field arithmetic on bounded widths

/// A binary interchange or extended format.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Format {
    /// Significand precision in bits, including the leading bit.
    pub prec: u32,
    /// Largest unbiased exponent.
    pub emax: i32,
    /// Exponent field width.
    pub exp_bits: u32,
    /// True if the leading significand bit is stored (x87).
    pub explicit_lead: bool,
}

impl Format {
    /// IEEE binary32 (`float`).
    pub const F32: Format = Format { prec: 24, emax: 127, exp_bits: 8, explicit_lead: false };
    /// IEEE binary64 (`double`).
    pub const F64: Format = Format { prec: 53, emax: 1023, exp_bits: 11, explicit_lead: false };
    /// x87 double-extended (`long double`, `_Float64x`).
    pub const F80: Format = Format { prec: 64, emax: 16383, exp_bits: 15, explicit_lead: true };
    /// IEEE binary128 (`_Float128`).
    pub const F128: Format = Format { prec: 113, emax: 16383, exp_bits: 15, explicit_lead: false };

    /// Smallest normal unbiased exponent.
    pub const fn emin(self) -> i32 {
        1 - self.emax
    }

    /// Width of the stored significand field (the leading bit included only for x87).
    const fn sig_field(self) -> u32 {
        if self.explicit_lead { self.prec } else { self.prec - 1 }
    }

    /// The all-ones exponent field (infinities and NaNs).
    const fn exp_all_ones(self) -> u128 {
        (1u128 << self.exp_bits) - 1
    }

    /// The quiet bit: the most significant bit of the fraction (below the leading bit).
    const fn quiet(self) -> u128 {
        1u128 << (self.prec - 2)
    }

    /// Exponent of a subnormal's (and the smallest normal's) least significant bit.
    const fn lsb_min(self) -> i32 {
        self.emin() - (self.prec as i32 - 1)
    }
}

/// The class and magnitude of a value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Value {
    /// Zero.
    Zero,
    /// `mant * 2^exp`, with `0 < mant < 2^prec` (not necessarily normalized).
    Finite {
        /// Integer significand.
        mant: u128,
        /// Binary exponent of the significand's least significant bit.
        exp: i32,
    },
    /// Infinity.
    Inf,
    /// Not a number.
    Nan {
        /// The payload (fraction bits below the quiet bit).
        payload: u128,
        /// True for a quiet NaN.
        quiet: bool,
    },
}

/// A signed value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Float {
    /// Sign bit.
    pub neg: bool,
    /// Magnitude.
    pub value: Value,
}

impl Float {
    /// Decode raw bits of `fmt` (the low bits of `bits`). x87 encodings that IEEE does not
    /// define (unnormals, pseudo-infinities, pseudo-NaNs) decode as quiet NaN; pseudo-denormals
    /// decode as the value they represent.
    pub fn unpack(bits: u128, fmt: Format) -> Float {
        let sf = fmt.sig_field();
        let sig = bits & ((1u128 << sf) - 1);
        let field = (bits >> sf) & fmt.exp_all_ones();
        let neg = (bits >> (sf + fmt.exp_bits)) & 1 == 1;
        // The fraction proper: the significand without its leading bit.
        let frac = sig & ((1u128 << (fmt.prec - 1)) - 1);
        let lead_stored = fmt.explicit_lead && sig >> (fmt.prec - 1) == 1;
        let value = if field == 0 {
            // Zero or subnormal; an x87 pseudo-denormal (lead bit set) has the same value formula.
            if sig == 0 { Value::Zero } else { Value::Finite { mant: sig, exp: fmt.lsb_min() } }
        } else if fmt.explicit_lead && !lead_stored {
            // x87 unnormal, pseudo-infinity or pseudo-NaN: invalid operands, read as quiet NaN.
            let payload = if field == fmt.exp_all_ones() { frac & (fmt.quiet() - 1) } else { 0 };
            Value::Nan { payload, quiet: true }
        } else if field == fmt.exp_all_ones() {
            if frac == 0 { Value::Inf } else { Value::Nan { payload: frac & (fmt.quiet() - 1), quiet: frac & fmt.quiet() != 0 } }
        } else {
            let lsb = field as i32 - fmt.emax - (fmt.prec as i32 - 1);
            Value::Finite { mant: frac | (1u128 << (fmt.prec - 1)), exp: lsb }
        };
        Float { neg, value }
    }

    /// Encode as raw bits of `fmt`. A `Finite` value must already be representable (as produced
    /// by [`round`]); a NaN keeps the payload bits that fit.
    pub fn pack(self, fmt: Format) -> u128 {
        let sf = fmt.sig_field();
        let lead = if fmt.explicit_lead { 1u128 << (fmt.prec - 1) } else { 0 };
        let (field, sig) = match self.value {
            Value::Zero => (0, 0),
            Value::Inf => (fmt.exp_all_ones(), lead),
            Value::Nan { payload, quiet } => {
                let mut frac = payload & (fmt.quiet() - 1);
                if quiet {
                    frac |= fmt.quiet();
                } else if frac == 0 {
                    frac = 1; // a signalling NaN needs a nonzero fraction
                }
                (fmt.exp_all_ones(), lead | frac)
            }
            Value::Finite { mant, exp } => {
                if mant == 0 {
                    (0, 0)
                } else {
                    let n = 128 - mant.leading_zeros();
                    let top = i64::from(exp) + i64::from(n) - 1;
                    if top >= i64::from(fmt.emin()) {
                        // Normal: align the leading bit to position prec - 1.
                        let m = shift(mant, i64::from(fmt.prec) - i64::from(n));
                        let field = (top + i64::from(fmt.emax)) as u128;
                        (field, if fmt.explicit_lead { m } else { m & !(1u128 << (fmt.prec - 1)) })
                    } else {
                        (0, shift(mant, i64::from(exp) - i64::from(fmt.lsb_min())))
                    }
                }
            }
        };
        let sign = u128::from(self.neg) << (sf + fmt.exp_bits);
        sign | (field << sf) | (sig & ((1u128 << sf) - 1))
    }
}

/// `m * 2^by` for a shift known to keep every set bit (left) or to drop only zeros (right).
fn shift(m: u128, by: i64) -> u128 {
    if by >= 0 { m << by.min(127) } else { m >> (-by).min(127) }
}

/// A rounding direction (the C rounding modes).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Rounding {
    /// To nearest, ties to even (`FE_TONEAREST`).
    #[default]
    Nearest,
    /// Toward +∞ (`FE_UPWARD`).
    Up,
    /// Toward −∞ (`FE_DOWNWARD`).
    Down,
    /// Toward zero (`FE_TOWARDZERO`).
    Zero,
}

impl Rounding {
    /// True if the discarded part rounds the kept magnitude up by one unit, given the kept least
    /// significant digit's parity (`odd`), whether the discarded part is strictly above half a
    /// unit (`above_half`) or exactly half (`exact_half`), and whether anything nonzero was
    /// discarded at all (`inexact`). Nothing discarded never rounds.
    pub fn rounds_up(self, neg: bool, odd: bool, above_half: bool, exact_half: bool, inexact: bool) -> bool {
        inexact
            && match self {
                Rounding::Nearest => above_half || (exact_half && odd),
                Rounding::Up => !neg,
                Rounding::Down => neg,
                Rounding::Zero => false,
            }
    }

    /// True if an overflowing result becomes infinity (otherwise the largest finite value).
    fn overflows_to_inf(self, neg: bool) -> bool {
        match self {
            Rounding::Nearest => true,
            Rounding::Up => !neg,
            Rounding::Down => neg,
            Rounding::Zero => false,
        }
    }
}

/// What rounding to a format did.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rounded {
    /// The representable result.
    pub value: Float,
    /// The result differs from the exact value.
    pub inexact: bool,
    /// The exact value exceeded the format's range.
    pub overflow: bool,
    /// The result is tiny (below the smallest normal after rounding to full precision with an
    /// unbounded exponent, or zero) and inexact.
    pub underflow: bool,
}

/// `(q + s) * 2^exp` (`0 <= s < 1`, nonzero iff `sticky`) rounded to an integer multiple of
/// `2^lsb`: the multiplier and whether anything was discarded. The multiplier may carry into
/// one more bit than the caller asked for.
fn round_at(neg: bool, q: u128, exp: i64, sticky: bool, lsb: i64, mode: Rounding) -> (u128, bool) {
    let d = lsb - exp;
    let (kept, above, half, inexact) = if d <= 0 {
        // Every bit of q is kept (the caller bounds the shift); only the sticky part is lost,
        // and it is below half a unit.
        (q << (-d).min(127), false, false, sticky)
    } else if d > 128 {
        (0, false, false, q != 0 || sticky)
    } else {
        let d = d as u32;
        let kept = if d == 128 { 0 } else { q >> d };
        let half_bit = (q >> (d - 1)) & 1 == 1;
        let below = q & ((1u128 << (d - 1)) - 1) != 0 || sticky;
        (kept, half_bit && below, half_bit && !below, half_bit || below)
    };
    (kept + u128::from(mode.rounds_up(neg, kept & 1 == 1, above, half, inexact)), inexact)
}

/// Round the exact value `(q + s) * 2^exp` to `fmt`, where `0 <= s < 1` is not known except
/// that it is nonzero iff `sticky`. Callers keep `q` at least `fmt.prec + 2` bits wide whenever
/// `sticky` is set, so the first discarded bit is always in `q`; `q = 0` with `sticky` stands
/// for a positive value below half the least subnormal (certain underflow).
pub fn round(neg: bool, q: u128, exp: i32, sticky: bool, fmt: Format, mode: Rounding) -> Rounded {
    let exact = |value| Rounded { value: Float { neg, value }, inexact: false, overflow: false, underflow: false };
    if q == 0 {
        if !sticky {
            return exact(Value::Zero);
        }
        // Only the sticky part: a positive value below half the least subnormal (the callers'
        // certain underflow). Zero, or that subnormal when the mode rounds the magnitude up.
        let up = mode.rounds_up(neg, false, false, false, true);
        let value = if up { Value::Finite { mant: 1, exp: fmt.lsb_min() } } else { Value::Zero };
        return Rounded { value: Float { neg, value }, inexact: true, overflow: false, underflow: true };
    }
    let prec = i64::from(fmt.prec);
    let exp = i64::from(exp);
    // Exponent of the value's leading bit.
    let top = exp + i64::from(127 - q.leading_zeros());
    let lsb_full = top - (prec - 1);
    let lsb = lsb_full.max(i64::from(fmt.lsb_min()));
    let (mut mant, inexact) = round_at(neg, q, exp, sticky, lsb, mode);
    let mut lsb = lsb;
    if mant >> fmt.prec != 0 {
        // Rounding carried to 2^prec: one bit more, exactly representable one place up.
        mant >>= 1;
        lsb += 1;
    }
    let rtop = lsb + i64::from(128 - mant.leading_zeros()) - 1;
    if mant != 0 && rtop > i64::from(fmt.emax) {
        let value = if mode.overflows_to_inf(neg) {
            Value::Inf
        } else {
            Value::Finite { mant: (1u128 << fmt.prec) - 1, exp: fmt.emax - (fmt.prec as i32 - 1) }
        };
        return Rounded { value: Float { neg, value }, inexact: true, overflow: true, underflow: false };
    }
    // Tininess after rounding: round again to full precision, ignoring the exponent's lower
    // bound, and ask whether the result is below 2^emin.
    let emin = i64::from(fmt.emin());
    let tiny = top < emin && {
        let (m, _) = round_at(neg, q, exp, sticky, lsb_full, mode);
        m == 0 || lsb_full + i64::from(128 - m.leading_zeros()) - 1 < emin
    };
    let value = if mant == 0 { Value::Zero } else { Value::Finite { mant, exp: lsb as i32 } };
    Rounded { value: Float { neg, value }, inexact, overflow: false, underflow: inexact && tiny }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, clippy::byte_char_slices)]
mod tests {
    use super::*;

    #[test]
    fn f64_bits_round_trip_through_unpack_and_pack() {
        let samples = [0.0f64, -0.0, 1.0, -2.5, f64::MIN_POSITIVE, 5e-324, f64::MAX, f64::INFINITY, f64::NEG_INFINITY, 0.1, 1e300];
        for x in samples {
            let b = u128::from(x.to_bits());
            assert_eq!(Float::unpack(b, Format::F64).pack(Format::F64), b, "{x}");
        }
        let nan = u128::from(f64::NAN.to_bits());
        assert_eq!(Float::unpack(nan, Format::F64).pack(Format::F64), nan);
    }

    #[test]
    fn rounding_agrees_with_hardware_for_f32() {
        // Every q in a dense range, exponents spanning normal and subnormal results: the
        // nearest-even result must equal Rust's own conversion of the exact value.
        for q in (1u128..5000).chain([(1 << 30) + 1, (1 << 30) - 1, 0xffff_ffff, 0x1_0000_0001]) {
            for exp in [-160, -150, -149, -140, -126, -30, 0, 10, 100] {
                let exact = (q as f64) * 2f64.powi(exp);
                let want = exact as f32;
                let r = round(false, q, exp, false, Format::F32, Rounding::Nearest);
                let got = f32::from_bits(r.value.pack(Format::F32) as u32);
                assert_eq!(got.to_bits(), want.to_bits(), "q={q} exp={exp}");
            }
        }
    }

    #[test]
    fn underflow_is_judged_after_rounding() {
        // Probed (glibc 2.39 strtod/strtof): 2^-1022 (1 - 2^-54) gives DBL_MIN without ERANGE
        // (full-precision rounding reaches 2^-1022); just below it, ERANGE.
        let q = (1u128 << 54) - 1; // (1 - 2^-54) * 2^54
        let r = round(false, q, -1022 - 54, false, Format::F64, Rounding::Nearest);
        assert_eq!(r.value.pack(Format::F64), u128::from(f64::MIN_POSITIVE.to_bits()));
        assert!(r.inexact && !r.underflow);
        let r = round(false, q, -1022 - 54, true, Format::F64, Rounding::Nearest);
        assert!(!r.underflow, "above the halfway point still reaches DBL_MIN at full precision");
        let r = round(false, (q << 4) - 1, -1022 - 58, false, Format::F64, Rounding::Nearest);
        assert_eq!(r.value.pack(Format::F64), u128::from(f64::MIN_POSITIVE.to_bits()));
        assert!(r.inexact && r.underflow, "below the halfway point is tiny after rounding");
        // strtof("0x1.fffffep-127"): tiny, ERANGE; 1.5 * 2^-1074 rounds to 2^-1073 with ERANGE.
        let r = round(false, 0x1ff_fffe, -127 - 24, false, Format::F32, Rounding::Nearest);
        assert!(r.underflow && r.value.pack(Format::F32) == u128::from(f32::MIN_POSITIVE.to_bits()));
        let r = round(false, 3, -1075, false, Format::F64, Rounding::Nearest);
        assert!(r.underflow && r.value.pack(Format::F64) == 2);
        let r = round(false, 1, -1074, false, Format::F64, Rounding::Nearest);
        assert!(!r.underflow && !r.inexact, "an exact subnormal does not underflow");
    }

    #[test]
    fn overflow_and_directed_modes() {
        let max = u128::from(f64::MAX.to_bits());
        let inf = u128::from(f64::INFINITY.to_bits());
        let q = (1u128 << 54) - 1; // just below 2^1024 when scaled by 2^(1024-54)
        for (mode, neg, want) in [
            (Rounding::Nearest, false, inf),
            (Rounding::Zero, false, max),
            (Rounding::Up, false, inf),
            (Rounding::Down, false, max),
            (Rounding::Up, true, max),
            (Rounding::Down, true, inf),
        ] {
            let r = round(neg, q, 1024 - 54, false, Format::F64, mode);
            // Probed (glibc 2.39 strtod, fesetround): ERANGE exactly when the result is the
            // infinity; truncation to DBL_MAX is IEEE's no-overflow case.
            assert_eq!(r.overflow, want == inf, "{mode:?} neg={neg}");
            assert_eq!(r.value.pack(Format::F64) & !(1 << 63), want, "{mode:?} neg={neg}");
        }
        // Directed rounding of 1 + 2^-60 and a sticky-only tail.
        let one = u128::from(1f64.to_bits());
        let r = round(false, (1 << 60) + 1, -60, false, Format::F64, Rounding::Up);
        assert_eq!(r.value.pack(Format::F64), one + 1);
        let r = round(true, 1 << 60, -60, true, Format::F64, Rounding::Up);
        assert_eq!(r.value.pack(Format::F64), one | 1 << 63, "toward +inf truncates a negative");
        let r = round(false, 0, 0, true, Format::F64, Rounding::Up);
        assert!(r.underflow && r.value.pack(Format::F64) == 1, "a sticky-only tail rounds up to the least subnormal");
    }

    #[test]
    // ARCH(x86_64): x87 80-bit encodings; long double is binary128 on aarch64 (A-10)
    fn x87_and_binary128_encodings() {
        // x87: 1.0, a pseudo-denormal (exponent 0, lead bit set) and an unnormal.
        let one = (16383u128 << 64) | (1 << 63);
        assert_eq!(Float::unpack(one, Format::F80).value, Value::Finite { mant: 1 << 63, exp: -63 });
        assert_eq!(Float::unpack(one, Format::F80).pack(Format::F80), one);
        let pseudo_denormal = (1u128 << 63) | 5;
        assert_eq!(Float::unpack(pseudo_denormal, Format::F80).value, Value::Finite { mant: (1 << 63) | 5, exp: -16382 - 63 });
        assert!(matches!(Float::unpack(16383u128 << 64, Format::F80).value, Value::Nan { quiet: true, .. }));
        assert!(matches!(Float::unpack(0x7fffu128 << 64, Format::F80).value, Value::Nan { quiet: true, .. }), "pseudo-infinity");
        assert_eq!(Float::unpack((0x7fffu128 << 64) | (1 << 63), Format::F80).value, Value::Inf);
        // binary128: the least subnormal and 1.0 round-trip.
        assert_eq!(Float::unpack(1, Format::F128).value, Value::Finite { mant: 1, exp: -16382 - 112 });
        let one128 = 16383u128 << 112;
        assert_eq!(Float::unpack(one128, Format::F128).pack(Format::F128), one128);
        // A signalling NaN with no payload bits keeps a nonzero fraction.
        let snan = Float { neg: false, value: Value::Nan { payload: 0, quiet: false } }.pack(Format::F64);
        assert!(f64::from_bits(snan as u64).is_nan());
    }
}
