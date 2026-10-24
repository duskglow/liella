//! Arbitrary-precision binary floating point: the reference that `liella-math` is measured
//! against.
//!
//! A [`Big`] is NaN, ±inf, ±0 or `±m * 2^e` with `m` an odd big integer, so every value has one
//! representation. Addition, subtraction and multiplication are exact; division and square root
//! are correctly rounded to a requested precision; [`Big::round_to`] performs IEEE rounding into
//! a concrete format with the exception flags. The elementary functions return an [`Approx`]
//! with a proven relative error bound, and [`correctly_rounded`] turns that into a correctly
//! rounded result by Ziv's strategy.

mod consts;
mod func;
mod nat;
mod text;
mod ziv;

use std::cmp::Ordering;

use nat::Nat;

pub use consts::{ln10, ln2, pi};
pub use func::*;
pub use ziv::{correctly_rounded, ulp_error, Undetermined};

/// Rounding direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Round {
    /// To nearest, ties to even.
    Nearest,
    /// Toward +inf.
    Up,
    /// Toward -inf.
    Down,
    /// Toward zero.
    Zero,
}

/// A binary floating-point format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fmt {
    /// Significand precision in bits, including the leading bit.
    pub prec: u32,
    /// Largest unbiased exponent (also the bias).
    pub emax: i32,
    /// Exponent field width.
    pub exp_bits: u32,
    /// True if the leading significand bit is stored (x87).
    pub explicit_lead: bool,
}

impl Fmt {
    /// IEEE binary32.
    pub const F32: Fmt = Fmt { prec: 24, emax: 127, exp_bits: 8, explicit_lead: false };
    /// IEEE binary64.
    pub const F64: Fmt = Fmt { prec: 53, emax: 1023, exp_bits: 11, explicit_lead: false };
    /// x87 double-extended: explicit leading bit (bit 63), sign at bit 79.
    pub const F80: Fmt = Fmt { prec: 64, emax: 16383, exp_bits: 15, explicit_lead: true };
    /// IEEE binary128.
    pub const F128: Fmt = Fmt { prec: 113, emax: 16383, exp_bits: 15, explicit_lead: false };

    /// Smallest normal exponent.
    pub fn emin(self) -> i32 {
        1 - self.emax
    }

    /// True for a format this module can encode: IEEE-style bias (`emax = 2^(exp_bits-1) - 1`),
    /// `2 <= prec`, `2 <= exp_bits <= 30` and at most 128 bits in all. Other values of the
    /// public fields make [`Big::from_bits`] return NaN and [`Big::round_to`] return 0.
    pub fn is_valid(self) -> bool {
        (2..=30).contains(&self.exp_bits)
            && self.prec >= 2
            && self.prec <= 126
            && i64::from(self.emax) == (1i64 << (self.exp_bits - 1)) - 1
            && self.total_bits() <= 128
    }

    /// Stored significand bits.
    fn frac_bits(self) -> u32 {
        if self.explicit_lead { self.prec } else { self.prec.saturating_sub(1) }
    }

    /// Total encoding width.
    pub fn total_bits(self) -> u32 {
        self.exp_bits.saturating_add(self.frac_bits()).saturating_add(1)
    }

    fn sign_bit(self) -> u128 {
        1u128 << (self.total_bits() - 1)
    }

    fn exp_field_max(self) -> u128 {
        (1u128 << self.exp_bits) - 1
    }

    /// Encoding from sign, biased exponent field and significand (the explicit leading bit
    /// is set by the caller for x87 normals, stripped here otherwise).
    fn encode(self, neg: bool, field: u128, sig: u128) -> u128 {
        let frac = if self.explicit_lead { sig } else { sig & ((1u128 << self.frac_bits()) - 1) };
        let s = if neg { self.sign_bit() } else { 0 };
        s | (field << self.frac_bits()) | frac
    }

    /// The default quiet NaN (sign clear); 0 for an invalid format.
    pub fn nan_bits(self) -> u128 {
        if !self.is_valid() {
            return 0;
        }
        let q = if self.explicit_lead { 3u128 << (self.prec - 2) } else { 1u128 << (self.prec - 2) };
        self.encode(false, self.exp_field_max(), q)
    }

    /// Signed infinity; 0 for an invalid format.
    pub fn inf_bits(self, neg: bool) -> u128 {
        if !self.is_valid() {
            return 0;
        }
        let lead = if self.explicit_lead { 1u128 << (self.prec - 1) } else { 0 };
        self.encode(neg, self.exp_field_max(), lead)
    }

    /// Largest finite magnitude; 0 for an invalid format.
    pub fn max_bits(self, neg: bool) -> u128 {
        if !self.is_valid() {
            return 0;
        }
        self.encode(neg, self.exp_field_max() - 1, (1u128 << self.prec) - 1)
    }
}

/// What rounding to a format did. Tininess is detected after rounding (the x86 rule): underflow
/// iff the result is inexact and the value rounded to `prec` bits with unbounded exponent is
/// below `2^emin` in magnitude.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Rounded {
    /// The result differs from the exact value.
    pub inexact: bool,
    /// The rounded value exceeded the format's range.
    pub overflow: bool,
    /// Tiny and inexact.
    pub underflow: bool,
}

/// An approximation of a real function value.
///
/// If `exact`, `value` is exactly the mathematical result. Otherwise
/// `|value - f(x)| <= 2^-prec * |f(x)|` and `value` is nonzero with the sign of `f(x)`.
#[derive(Clone, Debug)]
pub struct Approx {
    /// The value.
    pub value: Big,
    /// True if `value` is exact.
    pub exact: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Nan,
    Inf,
    Zero,
    Fin,
}

/// An arbitrary-precision binary floating-point number: NaN, ±inf, ±0 or `±m * 2^e` with `m` an
/// unsigned big integer (kept odd).
#[derive(Clone, Debug)]
pub struct Big {
    kind: Kind,
    neg: bool,
    man: Nat,
    exp: i64,
}

/// Beyond this many bits between two addends the smaller one is reduced to a sticky bit (see
/// [`Big::add`]); no format comes within a factor of ten of it.
const ADD_GAP_LIMIT: i64 = 1 << 20;

impl Big {
    /// Signed zero.
    pub fn zero(neg: bool) -> Big {
        Big { kind: Kind::Zero, neg, man: Nat::zero(), exp: 0 }
    }

    /// Signed infinity.
    pub fn inf(neg: bool) -> Big {
        Big { kind: Kind::Inf, neg, man: Nat::zero(), exp: 0 }
    }

    /// Not a number.
    pub fn nan() -> Big {
        Big { kind: Kind::Nan, neg: false, man: Nat::zero(), exp: 0 }
    }

    /// `±man * 2^exp`, normalized to an odd significand.
    fn fin(neg: bool, man: Nat, exp: i64) -> Big {
        if man.is_zero() {
            return Big::zero(neg);
        }
        let tz = man.trailing_zeros();
        if tz == 0 {
            Big { kind: Kind::Fin, neg, man, exp }
        } else {
            Big { kind: Kind::Fin, neg, man: man.shr(tz), exp: exp.saturating_add(tz as i64) }
        }
    }

    /// One.
    pub fn one() -> Big {
        Big::from_i64(1)
    }

    /// Exact conversion.
    pub fn from_i64(v: i64) -> Big {
        Big::fin(v < 0, Nat::from_u64(v.unsigned_abs()), 0)
    }

    /// Exact conversion.
    pub fn from_u64(v: u64) -> Big {
        Big::fin(false, Nat::from_u64(v), 0)
    }

    /// Exact conversion.
    pub fn from_u128(v: u128) -> Big {
        Big::fin(false, Nat::from_u128(v), 0)
    }

    /// Exact conversion.
    pub fn from_f64(x: f64) -> Big {
        Big::from_bits(Fmt::F64, u128::from(x.to_bits()))
    }

    /// Exact conversion.
    pub fn from_f32(x: f32) -> Big {
        Big::from_bits(Fmt::F32, u128::from(x.to_bits()))
    }

    /// `2^k`.
    pub fn pow2(k: i64) -> Big {
        Big::fin(false, Nat::from_u64(1), k)
    }

    /// Exact decode of an encoding in `fmt` (the low `fmt.total_bits()` bits of `bits`). NaN
    /// encodings give NaN; x87 pseudo-denormals decode as their value, other invalid x87
    /// encodings (unnormals, pseudo-infinities, pseudo-NaNs) as NaN.
    pub fn from_bits(fmt: Fmt, bits: u128) -> Big {
        if !fmt.is_valid() {
            return Big::nan();
        }
        let total = fmt.total_bits();
        let bits = if total < 128 { bits & ((1u128 << total) - 1) } else { bits };
        let fb = fmt.frac_bits();
        let neg = bits & fmt.sign_bit() != 0;
        let field = (bits >> fb) & fmt.exp_field_max();
        let frac = bits & ((1u128 << fb) - 1);
        let p = i64::from(fmt.prec);
        let emin = i64::from(fmt.emin());
        let bias = i64::from(fmt.emax);
        let lead = 1u128 << (fmt.prec - 1);
        if field == fmt.exp_field_max() {
            let payload = if fmt.explicit_lead {
                if frac & lead == 0 {
                    return Big::nan();
                }
                frac & (lead - 1)
            } else {
                frac
            };
            return if payload == 0 { Big::inf(neg) } else { Big::nan() };
        }
        if field == 0 {
            // Subnormal (and x87 pseudo-denormal, whose value uses the same exponent).
            return Big::fin(neg, Nat::from_u128(frac), emin - (p - 1));
        }
        let sig = if fmt.explicit_lead {
            if frac & lead == 0 {
                return Big::nan();
            }
            frac
        } else {
            frac | lead
        };
        Big::fin(neg, Nat::from_u128(sig), field as i64 - bias - (p - 1))
    }

    /// True for NaN.
    pub fn is_nan(&self) -> bool {
        self.kind == Kind::Nan
    }

    /// True for ±inf.
    pub fn is_inf(&self) -> bool {
        self.kind == Kind::Inf
    }

    /// True for ±0.
    pub fn is_zero(&self) -> bool {
        self.kind == Kind::Zero
    }

    /// True for zero or a finite nonzero value.
    pub fn is_finite(&self) -> bool {
        matches!(self.kind, Kind::Zero | Kind::Fin)
    }

    /// The sign bit (false for NaN).
    pub fn is_neg(&self) -> bool {
        self.neg && self.kind != Kind::Nan
    }

    /// True for a finite nonzero value.
    fn is_fin(&self) -> bool {
        self.kind == Kind::Fin
    }

    /// Bit-for-bit identity: same class, sign and value (NaN is identical to NaN).
    pub fn identical(&self, o: &Big) -> bool {
        self.kind == o.kind && self.is_neg() == o.is_neg() && self.man == o.man && self.exp == o.exp
    }

    /// Number of significant bits (0 unless finite nonzero).
    pub fn sig_bits(&self) -> u64 {
        self.man.bit_len()
    }

    /// Position of the leading bit, `floor(log2|x|)`; only meaningful for finite nonzero.
    fn top(&self) -> i64 {
        self.exp.saturating_add(self.man.bit_len() as i64 - 1)
    }

    /// Negation.
    pub fn neg(&self) -> Big {
        let mut r = self.clone();
        if r.kind != Kind::Nan {
            r.neg = !r.neg;
        }
        r
    }

    /// Absolute value.
    pub fn abs(&self) -> Big {
        let mut r = self.clone();
        r.neg = false;
        r
    }

    /// Copy with the given sign.
    fn with_sign(&self, neg: bool) -> Big {
        let mut r = self.clone();
        if r.kind != Kind::Nan {
            r.neg = neg;
        }
        r
    }

    /// Exact sum (IEEE signs of zero: `x + (-x) = +0`, `-0 + -0 = -0`).
    ///
    /// The one approximation: if the smaller operand lies entirely more than 2^20 bits below
    /// the larger one's last bit, it is replaced by a value of the same sign just below that
    /// gap (relative change below 2^-(2^20)), so memory stays bounded.
    pub fn add(&self, o: &Big) -> Big {
        use Kind::*;
        match (self.kind, o.kind) {
            (Nan, _) | (_, Nan) => Big::nan(),
            (Inf, Inf) => if self.neg == o.neg { self.clone() } else { Big::nan() },
            (Inf, _) => self.clone(),
            (_, Inf) => o.clone(),
            (Zero, Zero) => Big::zero(self.neg && o.neg),
            (Zero, _) => o.clone(),
            (_, Zero) => self.clone(),
            (Fin, Fin) => {
                let (hi, lo) = if self.exp >= o.exp { (self, o) } else { (o, self) };
                let lo_owned;
                let lo = if lo.top() < hi.exp.saturating_sub(ADD_GAP_LIMIT) {
                    lo_owned = Big::pow2(hi.exp - ADD_GAP_LIMIT).with_sign(lo.neg);
                    &lo_owned
                } else {
                    lo
                };
                let a = hi.man.shl((hi.exp - lo.exp) as u64);
                let b = &lo.man;
                if hi.neg == lo.neg {
                    Big::fin(hi.neg, a.add(b), lo.exp)
                } else {
                    match a.cmp(b) {
                        Ordering::Equal => Big::zero(false),
                        Ordering::Greater => Big::fin(hi.neg, a.sub(b), lo.exp),
                        Ordering::Less => Big::fin(lo.neg, b.sub(&a), lo.exp),
                    }
                }
            }
        }
    }

    /// Exact difference.
    pub fn sub(&self, o: &Big) -> Big {
        self.add(&o.neg())
    }

    /// Exact product.
    pub fn mul(&self, o: &Big) -> Big {
        use Kind::*;
        let neg = self.neg != o.neg;
        match (self.kind, o.kind) {
            (Nan, _) | (_, Nan) => Big::nan(),
            (Inf, Zero) | (Zero, Inf) => Big::nan(),
            (Inf, _) | (_, Inf) => Big::inf(neg),
            (Zero, _) | (_, Zero) => Big::zero(neg),
            (Fin, Fin) => Big::fin(neg, self.man.mul(&o.man), self.exp.saturating_add(o.exp)),
        }
    }

    /// Exact product by `2^k`.
    pub fn mul_pow2(&self, k: i64) -> Big {
        let mut r = self.clone();
        if r.kind == Kind::Fin {
            r.exp = r.exp.saturating_add(k);
        }
        r
    }

    /// Rounds a finite nonzero value to an integer multiple of `2^q`; returns the result and
    /// whether it is inexact. Other values are returned unchanged.
    fn round_at(&self, q: i64, mode: Round) -> (Big, bool) {
        if self.kind != Kind::Fin || self.exp >= q {
            return (self.clone(), false);
        }
        let shift = q.saturating_sub(self.exp) as u64;
        let bl = self.man.bit_len();
        // The significand is odd, so something nonzero is always discarded here.
        let (int, half, sticky) = if shift > bl {
            (Nat::zero(), false, true)
        } else {
            (self.man.shr(shift), self.man.bit(shift - 1), self.man.low_bits_nonzero(shift - 1))
        };
        let up = match mode {
            Round::Nearest => half && (sticky || int.is_odd()),
            Round::Zero => false,
            Round::Up => !self.neg,
            Round::Down => self.neg,
        };
        let int = if up { int.add_small(1) } else { int };
        (Big::fin(self.neg, int, q), true)
    }

    /// Rounds to `prec` significant bits (at least 1), unbounded exponent.
    pub fn round(&self, prec: u32, mode: Round) -> Big {
        if self.kind != Kind::Fin {
            return self.clone();
        }
        let q = self.top() - i64::from(prec.max(1)) + 1;
        self.round_at(q, mode).0
    }

    /// Rounds `±(man + sticky) * 2^exp` where `man` has at least `prec + 2` bits and `sticky`
    /// says a nonzero fraction below `man`'s last bit was dropped.
    fn round_sticky(neg: bool, man: Nat, exp: i64, sticky: bool, prec: u32, mode: Round) -> Big {
        if sticky {
            Big::fin(neg, man.shl(1).add_small(1), exp.saturating_sub(1)).round(prec, mode)
        } else {
            Big::fin(neg, man, exp).round(prec, mode)
        }
    }

    /// Quotient correctly rounded to `prec` bits (unbounded exponent).
    pub fn div(&self, o: &Big, prec: u32, mode: Round) -> Big {
        use Kind::*;
        let neg = self.neg != o.neg;
        match (self.kind, o.kind) {
            (Nan, _) | (_, Nan) | (Inf, Inf) | (Zero, Zero) => Big::nan(),
            (Inf, _) | (_, Zero) => Big::inf(neg),
            (Zero, _) | (_, Inf) => Big::zero(neg),
            (Fin, Fin) => {
                let p = u64::from(prec.max(1));
                let (la, lb) = (self.man.bit_len(), o.man.bit_len());
                // (ma << s) / mb >= 2^(la + s - 1 - lb) >= 2^(p + 2).
                let s = (p + 3 + lb).saturating_sub(la);
                let (q, r) = self.man.shl(s).divrem(&o.man);
                let exp = self.exp.saturating_sub(o.exp).saturating_sub(s as i64);
                Big::round_sticky(neg, q, exp, !r.is_zero(), prec.max(1), mode)
            }
        }
    }

    /// The integer square root of the significand scaled to at least `prec + 2` bits: returns
    /// `(root, exp, exact)` with `sqrt(self) = (root + f) * 2^exp`, `0 <= f < 1`, `f = 0` iff
    /// `exact`. Self must be finite and positive.
    fn sqrt_parts(&self, prec: u32) -> (Nat, i64, bool) {
        let p = u64::from(prec.max(1));
        let bl = self.man.bit_len();
        let mut s = (2 * p + 6).saturating_sub(bl);
        if (self.exp - s as i64).rem_euclid(2) != 0 {
            s += 1;
        }
        let m = self.man.shl(s);
        let root = m.isqrt();
        let exact = root.mul(&root) == m;
        (root, (self.exp - s as i64) / 2, exact)
    }

    /// Square root correctly rounded to `prec` bits; `sqrt(-0) = -0`, negative values give NaN.
    pub fn sqrt(&self, prec: u32, mode: Round) -> Big {
        match self.kind {
            Kind::Nan => Big::nan(),
            Kind::Zero => self.clone(),
            _ if self.neg => Big::nan(),
            Kind::Inf => self.clone(),
            Kind::Fin => {
                let (root, exp, exact) = self.sqrt_parts(prec);
                Big::round_sticky(false, root, exp, !exact, prec.max(1), mode)
            }
        }
    }

    /// Square root, exactly if it is a dyadic rational (with at most about `prec` bits more
    /// than needed), else truncated to at least `prec + 2` bits. Returns `(value, exact)`.
    fn sqrt_exact(&self, prec: u32) -> (Big, bool) {
        let (root, exp, exact) = self.sqrt_parts(prec);
        (Big::fin(false, root, exp), exact)
    }

    /// IEEE rounding into `fmt`: subnormals, overflow to inf or the largest finite value per
    /// `mode`, signed zeros; NaN gives the format's default quiet NaN (sign clear).
    pub fn round_to(&self, fmt: Fmt, mode: Round) -> (u128, Rounded) {
        let none = Rounded::default();
        if !fmt.is_valid() {
            return (0, none);
        }
        match self.kind {
            Kind::Nan => return (fmt.nan_bits(), none),
            Kind::Inf => return (fmt.inf_bits(self.neg), none),
            Kind::Zero => return (fmt.encode(self.neg, 0, 0), none),
            Kind::Fin => {}
        }
        let p = i64::from(fmt.prec);
        let emin = i64::from(fmt.emin());
        let emax = i64::from(fmt.emax);
        let (unb, inexact_unb) = self.round_at(self.top() - p + 1, mode);
        let tiny = unb.top() < emin;
        let (r, inexact) = if self.top() >= emin { (unb, inexact_unb) } else { self.round_at(emin - p + 1, mode) };
        let flags = Rounded { inexact, overflow: false, underflow: tiny && inexact };
        if r.is_zero() {
            return (fmt.encode(self.neg, 0, 0), flags);
        }
        let e = r.top();
        if e > emax {
            let to_inf = match mode {
                Round::Nearest => true,
                Round::Zero => false,
                Round::Up => !self.neg,
                Round::Down => self.neg,
            };
            let bits = if to_inf { fmt.inf_bits(self.neg) } else { fmt.max_bits(self.neg) };
            return (bits, Rounded { inexact: true, overflow: true, underflow: false });
        }
        // r = sig * 2^(max(e, emin) - p + 1) with sig < 2^p.
        let q = e.max(emin) - p + 1;
        let sig = r.man.shl((r.exp - q) as u64).to_u128().unwrap_or(0);
        let field = if e >= emin { (e + emax) as u128 } else { 0 };
        (fmt.encode(self.neg, field, sig), flags)
    }

    /// Comparison; `None` if NaN is involved, and `-0 == +0`.
    #[allow(clippy::should_implement_trait)] // fixed API; Big has no total order
    pub fn cmp(&self, o: &Big) -> Option<Ordering> {
        if self.is_nan() || o.is_nan() {
            return None;
        }
        let (sa, sb) = (self.signum(), o.signum());
        if sa != sb {
            return Some(sa.cmp(&sb));
        }
        if sa == 0 {
            return Some(Ordering::Equal);
        }
        let mag = self.cmp_abs(o);
        Some(if sa < 0 { mag.reverse() } else { mag })
    }

    /// -1, 0 or 1 (NaN gives 0).
    fn signum(&self) -> i8 {
        match self.kind {
            Kind::Nan | Kind::Zero => 0,
            _ if self.neg => -1,
            _ => 1,
        }
    }

    /// Comparison of magnitudes (NaN compares as zero).
    pub fn cmp_abs(&self, o: &Big) -> Ordering {
        let rank = |b: &Big| match b.kind {
            Kind::Nan | Kind::Zero => 0,
            Kind::Fin => 1,
            Kind::Inf => 2,
        };
        let (ra, rb) = (rank(self), rank(o));
        if ra != rb || ra != 1 {
            return ra.cmp(&rb);
        }
        match self.top().cmp(&o.top()) {
            Ordering::Equal => {}
            c => return c,
        }
        let e = self.exp.min(o.exp);
        self.man.shl((self.exp - e) as u64).cmp(&o.man.shl((o.exp - e) as u64))
    }

    /// Round to nearest `f64`.
    pub fn to_f64(&self) -> f64 {
        f64::from_bits(self.round_to(Fmt::F64, Round::Nearest).0 as u64)
    }

    /// `floor(log2|x|)` for finite nonzero values.
    pub fn ilogb(&self) -> Option<i64> {
        if self.is_fin() { Some(self.top()) } else { None }
    }

    /// Largest integer not above the value (exact; specials unchanged).
    pub fn floor(&self) -> Big {
        self.round_at(0, Round::Down).0
    }

    /// Integer part (exact; keeps the sign, so `trunc(-0.5) = -0`).
    pub fn trunc(&self) -> Big {
        self.round_at(0, Round::Zero).0
    }

    /// Nearest integer, ties to even.
    pub fn round_int(&self) -> Big {
        self.round_at(0, Round::Nearest).0
    }

    /// True for zero and finite integers.
    pub fn is_integer(&self) -> bool {
        match self.kind {
            Kind::Zero => true,
            Kind::Fin => self.exp >= 0,
            _ => false,
        }
    }

    /// True for odd integers.
    pub fn is_odd_integer(&self) -> bool {
        self.kind == Kind::Fin && self.exp == 0
    }

    /// The value as an `i64` if it is an integer in range.
    pub fn to_i64(&self) -> Option<i64> {
        match self.kind {
            Kind::Zero => Some(0),
            Kind::Fin if self.exp >= 0 && self.top() < 63 => {
                let m = self.man.shl(self.exp as u64).to_u128()? as i64;
                Some(if self.neg { -m } else { m })
            }
            _ => None,
        }
    }
}
