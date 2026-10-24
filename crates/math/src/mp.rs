//! Fixed-precision binary floating point for the special functions (spec §10.3, Track M
//! milestone 3): a sign, an `i64` exponent and a 384-bit significand, with truncating
//! operations. The error, gamma and Bessel functions evaluate their series and expansions in
//! it and round the result once into `float` or `double` (`round`), so the cancellation those
//! series suffer (up to about 150 bits for the Bessel power series, more near the zeros of
//! `lgamma`) leaves far more than the 53 bits a result needs.
//!
//! A value is `m / 2^384 · 2^exp` with `m`'s top bit set (zero: `m == 0`); `m[0]` is the least
//! significant limb. Each operation truncates to 384 bits, a relative error below 2^-383; the
//! functions built on them (`exp`, `ln`, `sin_cos`, ...) lose at most a few dozen more bits,
//! documented with each.

#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::indexing_slicing, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap, clippy::cast_precision_loss)] // limb arithmetic of fixed width

use crate::coeffs::tables as t;
use crate::float::{self, Exact, Float};
use crate::status::{Rounding, Status};

/// Limbs of the significand.
pub const LIMBS: usize = 6;

/// The arithmetic of `Mp` for the `LIMBS` in scope where it expands: here (384 bits) and in
/// [`crate::mpw`] (768 bits, the second step of Ziv's strategy, D-10.20). The expansion site
/// provides `LIMBS`, `Exact`, `Float`, `Rounding`, `Status`, `float` and the functions `pi`,
/// `euler` and `ln` that [`Arith`] forwards to.
macro_rules! mp_core {
    () => {
/// Bits of the significand.
const BITS: i64 = 64 * LIMBS as i64;

/// A fixed-precision binary floating-point value (see the module documentation).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Mp {
    /// Sign.
    pub neg: bool,
    /// Binary exponent: the value lies in `[2^(exp-1), 2^exp)` in magnitude.
    pub exp: i64,
    /// Significand, least significant limb first.
    pub m: [u64; LIMBS],
}

/// A constant of the generated tables: exponent and significand (see `Mp`).
pub type MpConst = (i64, [u64; LIMBS]);

/// The value of a generated constant.
pub const fn c(k: &MpConst) -> Mp {
    Mp { neg: false, exp: k.0, m: k.1 }
}

/// Normalize the wide significand `w` (little-endian, value `w / 2^(64·len) · 2^exp`) into an
/// `Mp`, truncating.
fn pack(neg: bool, exp: i64, w: &[u64]) -> Mp {
    let n = w.len();
    let Some(top) = (0..n).rev().find(|&i| w[i] != 0) else {
        return Mp::ZERO;
    };
    let lz = (n - 1 - top) as i64 * 64 + i64::from(w[top].leading_zeros());
    // The bit at position (64n - 1 - lz) becomes the top bit; take 384 bits below it.
    let hi_bit = 64 * n as i64 - 1 - lz;
    let mut m = [0u64; LIMBS];
    for (i, limb) in m.iter_mut().enumerate() {
        // Result bit range [64i, 64i + 64) maps to source bits [lo, lo + 64).
        let lo = hi_bit - BITS + 1 + 64 * i as i64;
        *limb = bits_at(w, lo);
    }
    Mp { neg, exp: exp - lz, m }
}

/// The 64 bits of `w` starting at bit `lo` (which may be negative: zeros below).
fn bits_at(w: &[u64], lo: i64) -> u64 {
    let word = |i: i64| if i >= 0 && (i as usize) < w.len() { w[i as usize] } else { 0 };
    let q = lo.div_euclid(64);
    let r = lo.rem_euclid(64) as u32;
    if r == 0 { word(q) } else { (word(q) >> r) | (word(q + 1) << (64 - r)) }
}

/// `m >> k` (truncating), for a significand.
fn shr(m: &[u64; LIMBS], k: i64) -> [u64; LIMBS] {
    let mut out = [0u64; LIMBS];
    if k >= BITS {
        return out;
    }
    for (i, limb) in out.iter_mut().enumerate() {
        *limb = bits_at(m, 64 * i as i64 + k);
    }
    out
}

fn cmp_mag(a: &[u64; LIMBS], b: &[u64; LIMBS]) -> core::cmp::Ordering {
    for i in (0..LIMBS).rev() {
        match a[i].cmp(&b[i]) {
            core::cmp::Ordering::Equal => {}
            o => return o,
        }
    }
    core::cmp::Ordering::Equal
}

// Named methods, not the operator traits, as in `dd`: chains read in evaluation order.
#[allow(clippy::should_implement_trait, clippy::needless_range_loop)]
impl Mp {
    /// Zero.
    pub const ZERO: Mp = Mp { neg: false, exp: 0, m: [0; LIMBS] };

    /// Whether the value is zero.
    pub fn is_zero(&self) -> bool {
        self.m[LIMBS - 1] == 0
    }

    /// The value of an `f64` (exact; zero for a non-finite input, which callers exclude).
    pub fn from_f64(x: f64) -> Mp {
        if x == 0.0 || !x.is_finite() {
            return Mp::ZERO;
        }
        let b = x.to_bits();
        let field = ((b >> 52) & 0x7ff) as i64;
        let frac = b & ((1u64 << 52) - 1);
        let (sig, e) = if field == 0 { (frac, -1074) } else { (frac | (1u64 << 52), field - 1075) };
        // sig · 2^e = (sig · 2^320 / 2^384) · 2^(e + 64).
        let mut w = [0u64; LIMBS];
        w[LIMBS - 1] = sig;
        pack(x < 0.0, e + 64, &w)
    }

    /// The value of an integer.
    pub fn from_i64(v: i64) -> Mp {
        let mut w = [0u64; LIMBS];
        w[LIMBS - 1] = v.unsigned_abs();
        pack(v < 0, 64, &w)
    }

    /// `-self`.
    pub fn neg(self) -> Mp {
        if self.is_zero() { self } else { Mp { neg: !self.neg, ..self } }
    }

    /// `|self|`.
    pub fn abs(self) -> Mp {
        Mp { neg: false, ..self }
    }

    /// `self · 2^k` (exact).
    pub fn ldexp(self, k: i64) -> Mp {
        if self.is_zero() { self } else { Mp { exp: self.exp + k, ..self } }
    }

    /// `self + o`.
    pub fn add(self, o: Mp) -> Mp {
        if o.is_zero() {
            return self;
        }
        if self.is_zero() {
            return o;
        }
        let (big, small) = if (self.exp, cmp_mag(&self.m, &o.m)) >= (o.exp, cmp_mag(&o.m, &self.m)) { (self, o) } else { (o, self) };
        let d = big.exp - small.exp;
        let s = shr(&small.m, d);
        if big.neg == small.neg {
            let mut w = [0u64; LIMBS + 1];
            let mut carry = 0u64;
            for i in 0..LIMBS {
                let (x, c1) = big.m[i].overflowing_add(s[i]);
                let (x, c2) = x.overflowing_add(carry);
                w[i] = x;
                carry = u64::from(c1) + u64::from(c2);
            }
            w[LIMBS] = carry;
            pack(big.neg, big.exp + 64, &w)
        } else {
            let mut w = [0u64; LIMBS];
            let mut borrow = 0u64;
            for i in 0..LIMBS {
                let (x, b1) = big.m[i].overflowing_sub(s[i]);
                let (x, b2) = x.overflowing_sub(borrow);
                w[i] = x;
                borrow = u64::from(b1) + u64::from(b2);
            }
            pack(big.neg, big.exp, &w)
        }
    }

    /// `self - o`.
    pub fn sub(self, o: Mp) -> Mp {
        self.add(o.neg())
    }

    /// `self · o`.
    pub fn mul(self, o: Mp) -> Mp {
        if self.is_zero() || o.is_zero() {
            return Mp::ZERO;
        }
        let mut w = [0u64; 2 * LIMBS];
        for i in 0..LIMBS {
            let mut carry = 0u128;
            for j in 0..LIMBS {
                let p = u128::from(self.m[i]) * u128::from(o.m[j]) + u128::from(w[i + j]) + carry;
                w[i + j] = p as u64;
                carry = p >> 64;
            }
            w[i + LIMBS] = carry as u64;
        }
        pack(self.neg != o.neg, self.exp + o.exp, &w)
    }

    /// `self²`.
    pub fn sqr(self) -> Mp {
        self.mul(self)
    }

    /// `self · k`.
    pub fn mul_u64(self, k: u64) -> Mp {
        if self.is_zero() || k == 0 {
            return Mp::ZERO;
        }
        let mut w = [0u64; LIMBS + 1];
        let mut carry = 0u128;
        for i in 0..LIMBS {
            let p = u128::from(self.m[i]) * u128::from(k) + carry;
            w[i] = p as u64;
            carry = p >> 64;
        }
        w[LIMBS] = carry as u64;
        pack(self.neg, self.exp + 64, &w)
    }

    /// `self / k` for `k > 0`.
    pub fn div_u64(self, k: u64) -> Mp {
        if self.is_zero() || k == 0 {
            return Mp::ZERO;
        }
        // (m · 2^64) / k, one extra limb of quotient below.
        let mut w = [0u64; LIMBS + 1];
        let mut rem = 0u128;
        for i in (0..=LIMBS).rev() {
            let limb = if i == 0 { 0 } else { self.m[i - 1] };
            let cur = (rem << 64) | u128::from(limb);
            w[i] = (cur / u128::from(k)) as u64;
            rem = cur % u128::from(k);
        }
        pack(self.neg, self.exp, &w)
    }

    /// `self · x` for an `f64`.
    pub fn mul_f64(self, x: f64) -> Mp {
        self.mul(Mp::from_f64(x))
    }

    /// An `f64` near the value (truncated to 64 bits, then rounded; overflow to ±∞, underflow
    /// to ±0): for estimates and branch decisions.
    pub fn to_f64(self) -> f64 {
        if self.is_zero() {
            return 0.0;
        }
        let top = self.m[LIMBS - 1] as f64; // in [2^63, 2^64)
        let e = self.exp - 64;
        let v = scale_f64(top, e);
        if self.neg { -v } else { v }
    }

    /// `1/self` (Newton's iteration from an `f64` start: 53, 106, 212, 424 bits).
    pub fn recip(self) -> Mp {
        let f = Mp { neg: false, exp: 0, m: self.m }.to_f64(); // in [0.5, 1)
        let mut y = Mp::from_f64(1.0 / f).ldexp(-self.exp);
        let a = self.abs();
        let one = Mp::from_i64(1);
        for _ in 0..4 {
            let e = one.sub(a.mul(y));
            y = y.add(y.mul(e));
        }
        if self.neg { y.neg() } else { y }
    }

    /// `self / o`.
    pub fn div(self, o: Mp) -> Mp {
        self.mul(o.recip())
    }

    /// `√self` for `self >= 0` (Newton's iteration for the reciprocal root).
    pub fn sqrt(self) -> Mp {
        if self.is_zero() {
            return Mp::ZERO;
        }
        // self = f · 2^(2h) with f in [0.25, 1).
        let h = self.exp.div_euclid(2);
        let f = Mp { neg: false, exp: self.exp - 2 * h, m: self.m };
        let mut y = Mp::from_f64(1.0 / libm_sqrt(f.to_f64()));
        let one = Mp::from_i64(1);
        for _ in 0..4 {
            let e = one.sub(f.mul(y.sqr()));
            y = y.add(y.mul(e).ldexp(-1));
        }
        f.mul(y).ldexp(h)
    }

    /// The nearest integer to the value, when `|self| < 2^62`.
    pub fn round_i64(self) -> i64 {
        if self.is_zero() || self.exp < 0 {
            return 0;
        }
        // Integer part and the first fraction bit.
        let int = if self.exp == 0 { 0 } else { (self.m[LIMBS - 1] >> (64 - self.exp.min(63))) as i64 };
        let half = if self.exp >= 64 { false } else { (self.m[LIMBS - 1] >> (63 - self.exp)) & 1 == 1 };
        let v = int + i64::from(half);
        if self.neg { -v } else { v }
    }

    /// The value as an exact-plus-sticky operand of `float::round` (the bits below the top 128
    /// are folded into the sticky bit, which is always set: the value approximates an
    /// irrational result).
    pub fn to_exact(self) -> Exact {
        let sig = (u128::from(self.m[LIMBS - 1]) << 64) | u128::from(self.m[LIMBS - 2]);
        let exp = (self.exp - 128).clamp(-(1 << 30), 1 << 30) as i32;
        Exact { neg: self.neg, sig, exp, sticky: true }
    }

    /// `v · 2^e` for an integer `v` (exact).
    pub fn from_u128(neg: bool, v: u128, e: i64) -> Mp {
        let mut w = [0u64; LIMBS];
        w[LIMBS - 2] = v as u64;
        w[LIMBS - 1] = (v >> 64) as u64;
        pack(neg, e + 128, &w)
    }

    /// The value rounded to `bits` significant bits (`bits <= 120`), to nearest with ties away
    /// from zero: a candidate for an exactness test.
    pub fn round_bits(self, bits: u32) -> Mp {
        if self.is_zero() {
            return self;
        }
        let top = (u128::from(self.m[LIMBS - 1]) << 64) | u128::from(self.m[LIMBS - 2]);
        let drop = 128 - bits;
        let kept = (top >> drop) + ((top >> (drop - 1)) & 1);
        Mp::from_u128(self.neg, kept, self.exp - i64::from(bits))
    }

    /// The number of significant bits, from the leading to the last nonzero one (0 for zero).
    pub fn sig_bits(self) -> i64 {
        if self.is_zero() {
            return 0;
        }
        let mut tz = 0i64;
        for &l in &self.m {
            if l == 0 {
                tz += 64;
            } else {
                tz += i64::from(l.trailing_zeros());
                break;
            }
        }
        BITS - tz
    }

    /// The fractional part, truncated toward zero (exact).
    pub fn frac(self) -> Mp {
        if self.is_zero() || self.exp <= 0 {
            return self;
        }
        if self.exp >= BITS {
            return Mp::ZERO;
        }
        // Clear the top `exp` bits (the integer part).
        let mut w = self.m;
        let keep = BITS - self.exp; // low bits kept
        for (i, limb) in w.iter_mut().enumerate() {
            let lo = 64 * i as i64;
            if lo >= keep {
                *limb = 0;
            } else if lo + 64 > keep {
                *limb &= (1u64 << (keep - lo)) - 1;
            }
        }
        pack(self.neg, self.exp, &w)
    }

    /// Compare magnitudes.
    pub fn cmp_abs(self, o: Mp) -> core::cmp::Ordering {
        match (self.is_zero(), o.is_zero()) {
            (true, true) => core::cmp::Ordering::Equal,
            (true, false) => core::cmp::Ordering::Less,
            (false, true) => core::cmp::Ordering::Greater,
            _ => self.exp.cmp(&o.exp).then(cmp_mag(&self.m, &o.m)),
        }
    }
}

/// `v · 2^e` for an `f64` `v`, saturating.
fn scale_f64(v: f64, e: i64) -> f64 {
    let e = e.clamp(-2200, 2200) as i32;
    crate::dd::ldexp(v, e)
}

/// `√x` of a positive normal `f64` by Newton's iteration on bits (core has no `sqrt` in
/// `no_std`): an estimate only.
fn libm_sqrt(x: f64) -> f64 {
    let mut y = f64::from_bits((x.to_bits() >> 1) + (0x3ff0_0000_0000_0000 >> 1));
    for _ in 0..6 {
        y = 0.5 * (y + x / y);
    }
    y
}

/// Round an approximation into `F` in `mode`, with `ERANGE` for an overflow to ∞ or an
/// underflow to 0.
pub fn round<F: Float>(v: Mp, mode: Rounding) -> (F, Status) {
    if v.exp > (1 << 20) {
        let (r, flags) = float::round::<F>(Exact { neg: v.neg, sig: 1, exp: 1 << 20, sticky: false }, mode);
        return (r, float::range_errno(r, Status::flags(flags)));
    }
    if v.is_zero() || v.exp < -(1 << 20) {
        let (r, flags) = float::round::<F>(Exact { neg: v.neg, sig: 0, exp: 0, sticky: true }, mode);
        return (r, float::range_errno(r, Status::flags(flags)));
    }
    let (r, flags) = float::round::<F>(v.to_exact(), mode);
    (r, float::range_errno(r, Status::flags(flags)))
}

impl crate::mp::Arith for Mp {
    const BITS: i64 = BITS;
    fn int(v: i64) -> Mp {
        Mp::from_i64(v)
    }
    fn expo(self) -> i64 {
        self.exp
    }
    fn is_zero(&self) -> bool {
        Mp::is_zero(self)
    }
    fn add(self, o: Mp) -> Mp {
        Mp::add(self, o)
    }
    fn sub(self, o: Mp) -> Mp {
        Mp::sub(self, o)
    }
    fn mul(self, o: Mp) -> Mp {
        Mp::mul(self, o)
    }
    fn sqr(self) -> Mp {
        Mp::sqr(self)
    }
    fn neg(self) -> Mp {
        Mp::neg(self)
    }
    fn ldexp(self, k: i64) -> Mp {
        Mp::ldexp(self, k)
    }
    fn mul_u64(self, k: u64) -> Mp {
        Mp::mul_u64(self, k)
    }
    fn div_u64(self, k: u64) -> Mp {
        Mp::div_u64(self, k)
    }
    fn recip(self) -> Mp {
        Mp::recip(self)
    }
    fn to_f64(self) -> f64 {
        Mp::to_f64(self)
    }
    fn pi() -> Mp {
        pi()
    }
    fn euler() -> Mp {
        euler()
    }
    fn ln(self) -> Mp {
        ln(self)
    }
    fn round<F: Float>(self, mode: Rounding) -> (F, Status) {
        round::<F>(self, mode)
    }
}
    };
}
pub(crate) use mp_core;
mp_core!();

/// The operations the Bessel power series and the rounding test of Ziv's strategy need, for
/// the 384-bit `Mp` and the 768-bit [`crate::mpw::Mp`] alike (each forwards to its own
/// arithmetic; every operation truncates to `BITS` bits, a relative error below `2^(1 − BITS)`).
pub trait Arith: Copy {
    /// Bits of the significand.
    const BITS: i64;
    /// The value of an integer.
    fn int(v: i64) -> Self;
    /// The binary exponent: the value lies in `[2^(e−1), 2^e)` in magnitude.
    fn expo(self) -> i64;
    /// Whether the value is zero.
    fn is_zero(&self) -> bool;
    /// `self + o`.
    fn add(self, o: Self) -> Self;
    /// `self − o`.
    fn sub(self, o: Self) -> Self;
    /// `self · o`.
    fn mul(self, o: Self) -> Self;
    /// `self²`.
    fn sqr(self) -> Self;
    /// `−self`.
    fn neg(self) -> Self;
    /// `self · 2^k` (exact).
    fn ldexp(self, k: i64) -> Self;
    /// `self · k`.
    fn mul_u64(self, k: u64) -> Self;
    /// `self / k`.
    fn div_u64(self, k: u64) -> Self;
    /// `1 / self`.
    fn recip(self) -> Self;
    /// An `f64` estimate.
    fn to_f64(self) -> f64;
    /// `π`.
    fn pi() -> Self;
    /// Euler's constant `γ`.
    fn euler() -> Self;
    /// `ln self` for `self > 0`.
    fn ln(self) -> Self;
    /// Round into `F` in `mode` (see [`round`]).
    fn round<F: Float>(self, mode: Rounding) -> (F, Status);
}

// ------------------------------------------------- the rounding test of Ziv's strategy

/// The exponent of `v` (far below everything for zero), for error bounds.
pub fn ex<M: Arith>(v: M) -> i64 {
    if v.is_zero() { -(1 << 40) } else { v.expo() }
}

/// A result and the exponent `err` of a bound `2^err` on its absolute error.
#[derive(Clone, Copy)]
pub struct Bounded<M> {
    /// The value.
    pub v: M,
    /// The exponent of the bound on the absolute error of `v`.
    pub err: i64,
}

/// The rounding test of Ziv's strategy: `v` rounded in `mode` when every value within
/// `2^err` of it rounds alike (the same encoding and the same flags; rounding is monotone, so
/// both ends of the interval suffice). The ends are widened by four last places of `v` for the
/// truncation of `v ± 2^err` itself.
pub fn decided<F: Float, M: Arith>(v: Bounded<M>, mode: Rounding) -> Option<(F, Status)> {
    let e = M::int(1).ldexp(v.err.max(ex(v.v) + 4 - M::BITS) + 1);
    let lo = v.v.sub(e).round::<F>(mode);
    let hi = v.v.add(e).round::<F>(mode);
    (lo.0.to_bits() == hi.0.to_bits() && lo.1 == hi.1).then_some(lo)
}

/// Euler's constant `γ`.
pub fn euler() -> Mp {
    c(&t::MP_EULER)
}

/// π.
pub fn pi() -> Mp {
    c(&t::MP_PI)
}

/// ln 2.
pub fn ln2() -> Mp {
    c(&t::MP_LN2)
}

/// `ln x` for `x > 0`: `x = 2^e·m` with `m` in `[√½, √2)`, `ln m` by Halley's iteration on
/// `e^y = m` from an `f64` start (cubic: 53, 159, 477 bits), plus `e·ln 2`.
pub fn ln(x: Mp) -> Mp {
    let mut e = x.exp;
    let mut m = Mp { neg: false, exp: 0, m: x.m };
    if m.to_f64() < core::f64::consts::FRAC_1_SQRT_2 {
        m = m.ldexp(1);
        e -= 1;
    }
    let mut y = Mp::from_f64(ln_f64(m.to_f64()));
    for _ in 0..3 {
        let ey = exp(y);
        y = y.add(m.sub(ey).div(m.add(ey)).ldexp(1));
    }
    y.add(ln2().mul(Mp::from_i64(e)))
}

/// An `f64` estimate of `ln m` for `m` in `[0.5, 2]` (series of `atanh`, about 1e-12): only a
/// starting value.
fn ln_f64(m: f64) -> f64 {
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    let mut sum = 0.0;
    let mut p = s;
    let mut k = 1.0;
    for _ in 0..20 {
        sum += p / k;
        p *= s2;
        k += 2.0;
    }
    2.0 * sum
}

/// The kernels built on `mp_core` for the `LIMBS` in scope where it expands: here (384 bits)
/// and in [`crate::mpw`] (768 bits, the retry step of Ziv's strategy). The expansion site also
/// provides `pi`, `ln2`, `ln` and `t::TWO_OVER_PI`. The error bounds are proved in [`crate::mpw`]'s
/// module documentation, for a significand of `BITS` bits.
macro_rules! mp_kernels {
    () => {
/// Round a value that is exact in `BITS` bits (an integer product, say): the bits below the top
/// 128 are the sticky bit as they are.
pub fn round_exact<F: Float>(v: Mp, mode: Rounding) -> (F, Status) {
    let mut e = v.to_exact();
    e.sticky = v.m[..LIMBS - 2].iter().any(|&l| l != 0);
    let (r, flags) = float::round::<F>(e, mode);
    (r, float::range_errno(r, Status::flags(flags)))
}

/// `e^x` for `|x| < 2^40`: `x = k·ln 2 + r`, `e^(r/2^12)` by its Taylor series, squared twelve
/// times (each squaring doubles the relative error: about 2^-370 in all).
pub fn exp(x: Mp) -> Mp {
    if x.is_zero() {
        return Mp::from_i64(1);
    }
    let k = (x.to_f64() / core::f64::consts::LN_2) as i64;
    let r = x.sub(ln2().mul(Mp::from_i64(k)));
    let s = r.ldexp(-12);
    let one = Mp::from_i64(1);
    let mut sum = one;
    let mut term = one;
    let mut n = 1u64;
    loop {
        term = term.mul(s).div_u64(n);
        if term.is_zero() || term.exp < sum.exp - BITS - 8 {
            break;
        }
        sum = sum.add(term);
        n += 1;
    }
    for _ in 0..12 {
        sum = sum.sqr();
    }
    sum.ldexp(k)
}

/// `(sin r, cos r)` for `|r| <= 1` by the Taylor series (terms below 2^-400 of the sums).
pub fn sin_cos_small(r: Mp) -> (Mp, Mp) {
    let r2 = r.sqr().neg();
    let one = Mp::from_i64(1);
    let (mut s, mut cs) = (r, one);
    let (mut ts, mut tc) = (r, one);
    let mut n = 1u64;
    loop {
        ts = ts.mul(r2).div_u64((2 * n) * (2 * n + 1));
        tc = tc.mul(r2).div_u64((2 * n - 1) * (2 * n));
        let small = |t: Mp, sum: Mp| t.is_zero() || t.exp < sum.exp - BITS - 8;
        if small(ts, s) && small(tc, cs) {
            break;
        }
        s = s.add(ts);
        cs = cs.add(tc);
        n += 1;
    }
    (s, cs)
}

/// `ln(1 + t)` for `t > −1`. Below 2^-6 in magnitude by the series of `2 atanh(t/(2 + t))`
/// (the relative error stays near 2^-380 however small `t` is); above, `ln` of `1 + t`, whose
/// result is at least 2^-7 in magnitude, so the truncated sum costs nothing.
pub fn ln1p(t: Mp) -> Mp {
    if t.is_zero() {
        return Mp::ZERO;
    }
    if t.exp > -6 {
        return ln(Mp::from_i64(1).add(t));
    }
    let u = t.div(Mp::from_i64(2).add(t));
    let u2 = u.sqr();
    let (mut sum, mut p) = (u, u);
    let mut k = 1u64;
    loop {
        p = p.mul(u2);
        k += 2;
        let term = p.div_u64(k);
        if term.is_zero() || term.exp < sum.exp - BITS - 8 {
            break;
        }
        sum = sum.add(term);
    }
    sum.ldexp(1)
}

/// `atan t` for `t >= 0`: `π/2 − atan(1/t)` above 1 (at least π/4, so nothing cancels), then
/// eight halvings `t ← t/(1 + √(1 + t²))` (each halves the angle) to below 2^-8 and the
/// alternating series, about 24 terms; relative error near 2^-375.
pub fn atan(t: Mp) -> Mp {
    if t.is_zero() {
        return Mp::ZERO;
    }
    let one = Mp::from_i64(1);
    let (u0, flip) = if t.cmp_abs(one).is_gt() { (one.div(t), true) } else { (t, false) };
    let halvings = if u0.exp < -10 { 0 } else { 8 };
    let mut u = u0;
    for _ in 0..halvings {
        u = u.div(one.add(one.add(u.sqr()).sqrt()));
    }
    let u2 = u.sqr().neg();
    let (mut sum, mut p) = (u, u);
    let mut k = 1u64;
    loop {
        p = p.mul(u2);
        k += 2;
        let term = p.div_u64(k);
        if term.is_zero() || term.exp < sum.exp - BITS - 8 {
            break;
        }
        sum = sum.add(term);
    }
    let a = sum.ldexp(halvings);
    if flip { pi().ldexp(-1).sub(a) } else { a }
}

/// `atan2(y, x)` for values not both zero, in `[−π, π]` (a zero `y` gives 0 or π: the caller
/// owns the sign of zero).
pub fn atan2(y: Mp, x: Mp) -> Mp {
    let v = if x.is_zero() {
        pi().ldexp(-1)
    } else {
        let a = atan(y.abs().div(x.abs()));
        if x.neg { pi().sub(a) } else { a }
    };
    if y.neg { v.neg() } else { v }
}

/// `(sinh x, cosh x)` for `|x| < 2^39`: the Taylor series below ½ (no cancellation for tiny
/// `x`), `(e^x ∓ e^−x)/2` above (at most two bits cancel).
pub fn sinh_cosh(x: Mp) -> (Mp, Mp) {
    let one = Mp::from_i64(1);
    if x.is_zero() {
        return (Mp::ZERO, one);
    }
    if x.exp <= -1 {
        let x2 = x.sqr();
        let (mut s, mut c) = (x, one);
        let (mut ts, mut tc) = (x, one);
        let mut n = 1u64;
        loop {
            ts = ts.mul(x2).div_u64((2 * n) * (2 * n + 1));
            tc = tc.mul(x2).div_u64((2 * n - 1) * (2 * n));
            let small = |t: Mp, sum: Mp| t.is_zero() || t.exp < sum.exp - BITS - 8;
            if small(ts, s) && small(tc, c) {
                break;
            }
            s = s.add(ts);
            c = c.add(tc);
            n += 1;
        }
        return (s, c);
    }
    let e = exp(x);
    let r = e.recip();
    (e.sub(r).ldexp(-1), e.add(r).ldexp(-1))
}

/// The quadrant rotation of `(sin r, cos r)` by `k·π/2`.
fn rotate(k: i64, s: Mp, c: Mp) -> (Mp, Mp) {
    match k.rem_euclid(4) {
        0 => (s, c),
        1 => (c, s.neg()),
        2 => (s.neg(), c.neg()),
        _ => (c.neg(), s),
    }
}

/// `(sin y, cos y)` of a finite `f64` (reduced by [`reduce_half_pi`]).
pub fn sin_cos_f64(y: f64) -> (Mp, Mp) {
    let (k, r) = reduce_half_pi(y);
    let (s, c) = sin_cos_small(r);
    rotate(k, s, c)
}

/// `(sin x, cos x)` for `|x| < 2^60`: `x − k·π/2` with `k` from `x·2/π` in this precision (the
/// reduced argument keeps an absolute error near `|x|·2^-383`, the order of `x`'s own).
pub fn sin_cos(x: Mp) -> (Mp, Mp) {
    let k = x.mul(pi().recip().ldexp(1)).round_i64();
    let r = x.sub(pi().ldexp(-1).mul(Mp::from_i64(k)));
    let (s, c) = sin_cos_small(r);
    rotate(k, s, c)
}

/// `(sin πr, cos πr)` for an exact `r` in `[−1, 1]`, exact (zero or ±1) at the multiples of ½:
/// `(values, exact)`.
pub fn sin_cos_pi(r: Mp) -> ((Mp, Mp), bool) {
    let q = r.ldexp(1).round_i64();
    let t = r.sub(Mp::from_i64(q).ldexp(-1)); // in [−¼, ¼], exact
    if t.is_zero() {
        return (rotate(q, Mp::ZERO, Mp::from_i64(1)), true);
    }
    let (s, c) = sin_cos_small(pi().mul(t));
    (rotate(q, s, c), false)
}

/// The square root of an exact `v >= 0` when it is a dyadic number of at most 120 significant
/// bits (the candidate, the approximation rounded to 120 bits, squared exactly).
pub fn exact_sqrt(v: Mp) -> Option<Mp> {
    if v.is_zero() {
        return Some(Mp::ZERO);
    }
    let r = v.sqrt().round_bits(120);
    if r.sqr() == v { Some(r) } else { None }
}

/// `a · b`, and whether it is exact (the significands' bits fit the `BITS`).
pub fn mul_x(a: Mp, b: Mp) -> (Mp, bool) {
    (a.mul(b), a.sig_bits() + b.sig_bits() <= BITS)
}

/// `a + b`, and whether it is exact (the bits of both, and a carry, fit the `BITS`).
pub fn add_x(a: Mp, b: Mp) -> (Mp, bool) {
    if a.is_zero() || b.is_zero() {
        return (a.add(b), true);
    }
    let hi = a.exp.max(b.exp) + 1;
    let lo = (a.exp - a.sig_bits()).min(b.exp - b.sig_bits());
    (a.add(b), hi - lo <= BITS)
}

/// `ln 10`.
pub fn ln10() -> Mp {
    ln(Mp::from_i64(10))
}

/// `x` reduced by multiples of π/2: `(k mod 4, r)` with `x = k·π/2 + r`, `|r| <= π/4` (about),
/// for a finite `f64` `x`; huge arguments by the bits of `2/π` (Payne–Hanek).
pub fn reduce_half_pi(x: f64) -> (i64, Mp) {
    let ax = if x < 0.0 { -x } else { x };
    let (k, r) = if ax < 1.0e9 {
        let k = (ax * core::f64::consts::FRAC_2_PI + 0.5) as i64;
        (k, Mp::from_f64(ax).sub(pi().ldexp(-1).mul(Mp::from_i64(k))))
    } else {
        reduce_huge(ax)
    };
    if x < 0.0 { ((-k).rem_euclid(4), r.neg()) } else { (k.rem_euclid(4), r) }
}

/// Payne–Hanek reduction of a positive `x >= 1e9`.
fn reduce_huge(x: f64) -> (i64, Mp) {
    let b = x.to_bits();
    let sig = (b & ((1u64 << 52) - 1)) | (1u64 << 52);
    let e = ((b >> 52) & 0x7ff) as i64 - 1075; // x = sig · 2^e, e >= -23
    reduce_wide(u128::from(sig), e)
}

/// Payne–Hanek reduction of `x = sig · 2^e >= 2^29` (`sig` below 2^113, any exponent of the
/// formats): `(k mod 4, r)` with `x = k·π/2 + r`, `|r| <= π/4`. `x·(2/π)` modulo 4 comes from a
/// window of the bits of `2/π` (bits whose products with `x` are multiples of 4 dropped), the
/// fraction to `BITS + 190` bits (see the bounds in [`crate::mpw`]).
pub fn reduce_wide(sig: u128, e: i64) -> (i64, Mp) {
    // Bit j (1-based) of 2/π weighs 2^-j; sig·2^e·2^-j is a multiple of 4 for j <= e - 2: start
    // at j0 = max(1, e - 1) and take RW = LIMBS + 3 words (BITS + 192 bits).
    const RW: usize = LIMBS + 3;
    let j0 = (e - 1).max(1);
    let word = |i: usize| t::TWO_OVER_PI.get(i).copied().unwrap_or(0);
    let mut g = [0u64; RW]; // big-endian window, g[0] most significant
    for (i, gi) in g.iter_mut().enumerate() {
        let o = (j0 - 1) as usize + 64 * i;
        let (q, r) = (o / 64, (o % 64) as u32);
        *gi = if r == 0 { word(q) } else { (word(q) << r) | (word(q + 1) >> (64 - r)) };
    }
    // P = sig · G (11 limbs, little-endian); value = P · 2^(e - j0 - 64·RW + 1).
    let s = [sig as u64, (sig >> 64) as u64];
    let mut p = [0u64; RW + 2];
    for (a, &sa) in s.iter().enumerate() {
        let mut carry = 0u128;
        for i in 0..RW {
            let v = u128::from(sa) * u128::from(g[RW - 1 - i]) + u128::from(p[i + a]) + carry;
            p[i + a] = v as u64;
            carry = v >> 64;
        }
        p[RW + a] = carry as u64;
    }
    // The binary point sits `f = j0 + 64·RW − 1 − e` bits up from the bottom.
    let f = j0 + 64 * RW as i64 - 1 - e;
    let int = bits_at(&p, f); // the integer part's low bits
    let mut frac = [0u64; RW + 1];
    for (i, limb) in frac.iter_mut().enumerate() {
        let lo = f - 64 * (RW as i64 + 1) + 64 * i as i64;
        *limb = bits_at(&p, lo);
    }
    // frac / 2^(64·(RW + 1)) in [0, 1); centre it.
    let mut k = (int & 3) as i64;
    let mut fr = pack(false, 0, &frac);
    if fr.exp == 0 && !fr.is_zero() {
        // fr >= 0.5
        fr = fr.sub(Mp::from_i64(1));
        k += 1;
    }
    (k, fr.mul(pi().ldexp(-1)))
}

/// `(sin x, cos x)` of `x = ±sig · 2^e` (a finite nonzero value of any format): reduced in this
/// precision below 2^29, by [`reduce_wide`] above.
pub fn sin_cos_wide(neg: bool, sig: u128, e: i64) -> (Mp, Mp) {
    let a = Mp::from_u128(false, sig, e);
    let (s, c) = if a.exp <= 29 {
        sin_cos(a)
    } else {
        let (k, r) = reduce_wide(sig, e);
        let (s, c) = sin_cos_small(r);
        rotate(k, s, c)
    };
    if neg { (s.neg(), c) } else { (s, c) }
}
    };
}
pub(crate) use mp_kernels;
mp_kernels!();

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    fn close(a: Mp, b: f64) -> bool {
        let d = a.to_f64() - b;
        (if d < 0.0 { -d } else { d }) <= 1e-15 * (if b < 0.0 { -b } else { b }).max(1e-300)
    }

    #[test]
    fn arithmetic_matches_f64_where_exact() {
        let a = Mp::from_f64(1.5);
        let b = Mp::from_f64(-0.25);
        assert_eq!(a.add(b).to_f64(), 1.25);
        assert_eq!(a.mul(b).to_f64(), -0.375);
        assert_eq!(b.sub(a).to_f64(), -1.75);
        assert_eq!(Mp::from_i64(7).div_u64(2).to_f64(), 3.5);
        assert_eq!(Mp::from_i64(7).mul_u64(3).to_f64(), 21.0);
        assert_eq!(Mp::from_f64(1e-310).to_f64(), 1e-310);
        assert!(close(Mp::from_i64(1).div(Mp::from_i64(3)), 1.0 / 3.0));
        assert!(close(Mp::from_i64(2).sqrt(), core::f64::consts::SQRT_2));
        assert_eq!(Mp::from_f64(2.5).round_i64(), 3);
        assert_eq!(Mp::from_f64(-7.4).round_i64(), -7);
        // Cancellation keeps the low bits: (1 + 2^-300) - 1 = 2^-300.
        let tiny = Mp::from_i64(1).ldexp(-300);
        assert_eq!(Mp::from_i64(1).add(tiny).sub(Mp::from_i64(1)), tiny);
    }

    #[test]
    fn transcendental_kernels() {
        assert!(close(exp(Mp::from_i64(1)), core::f64::consts::E));
        assert!(close(ln(Mp::from_i64(10)), core::f64::consts::LN_10));
        assert!(close(ln(exp(Mp::from_f64(0.3))), 0.3));
        let (s, c) = sin_cos_small(Mp::from_f64(0.5));
        assert!(close(s, 0.479_425_538_604_203));
        assert!(close(c, 0.877_582_561_890_372_8));
        let (k, r) = reduce_half_pi(10.0);
        assert_eq!(k, 2);
        assert!(close(r, 0.575_222_039_230_620_6));
        let (k, r) = reduce_half_pi(1e22);
        // 1e22 = 6_366_197_723_675_813_430_755 (2/π units) + ..., k = 3 (mod 4); sin(1e22) = -0.8522..
        let (s, c) = sin_cos_small(r);
        let sin = match k { 0 => s, 1 => c, 2 => s.neg(), _ => c.neg() };
        assert!(close(sin, -0.852_200_849_767_189));
    }

    /// A digest of every bit of the 384-bit kernels' results on a fixed set of operands.
    pub(crate) fn narrow_digest() -> u64 {
        let mut h = 0xcbf2_9ce4_8422_2325u64;
        let mut eat = |v: Mp| {
            for w in [u64::from(v.neg), v.exp as u64].into_iter().chain(v.m) {
                h = (h ^ w).wrapping_mul(0x0000_0100_0000_01b3).rotate_left(29);
            }
        };
        let ops = [0.75, 1.5, -3.25, 100.0, 1e-30, -7.0e5, 0.015_625, 0.999, 33.3, -0.4]; // within every kernel's domain
        for &v in &ops {
            let x = Mp::from_f64(v);
            eat(exp(x.ldexp(-4)));
            eat(ln(x.abs()));
            eat(ln1p(x.abs().ldexp(-9)));
            eat(ln1p(x.abs()));
            eat(atan(x.abs()));
            eat(atan2(x, Mp::from_f64(-1.25)));
            eat(atan2(Mp::from_f64(0.5), x));
            let (s, c) = sinh_cosh(x.ldexp(-3));
            eat(s);
            eat(c);
            let (s, c) = sin_cos(x);
            eat(s);
            eat(c);
            let ((s, c), _) = sin_cos_pi(x.frac());
            eat(s);
            eat(c);
            eat(exact_sqrt(x.sqr()).unwrap_or(Mp::ZERO));
            eat(x.sqrt().abs());
            eat(x.recip());
            eat(mul_x(x, Mp::from_f64(1.1)).0);
            eat(add_x(x, Mp::from_f64(1e-90)).0);
        }
        eat(ln10());
        for (sig, e) in [(0x1_2345_6789_abcd_ef01_2345_6789_abcdu128, 16271i64), (0x1_ffff_ffff_ffff_ffff_ffff_ffff_ffffu128, 8000), (0x1_0000_0000_0000_0000_0000_0000_0001u128, -60), (12345, 40)] {
            let (k, r) = reduce_wide(sig, e);
            eat(Mp::from_i64(k));
            eat(r);
            let (s, c) = sin_cos_wide(true, sig, e);
            eat(s);
            eat(c);
        }
        h
    }

    #[test]
    fn narrow_kernels_unchanged() {
        // no spec clause: the 384-bit results are bit-for-bit those before the 768-bit kernels
        // were added (MATH-M5-CPLX-F128-ZIV-A; the digest was taken on c30e8f83, the code before it).
        assert_eq!(narrow_digest(), 0xec36_a478_896a_203d);
    }
}
