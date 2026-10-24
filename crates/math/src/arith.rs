//! Exact arithmetic on decoded values: the sum, product, quotient, square root and fused
//! multiply-add of finite operands, computed exactly (or to the operand width plus a sticky bit)
//! and handed to `float::round` for the one rounding into any target format. The same code
//! serves `fma` in every format and the narrowing operations of §10.4, which round a wider
//! format's exact result once into a narrower one.
//!
//! Significands are at most 113 bits, so products fit 226 bits and every intermediate fits the
//! 256-bit [`U256`].

#![allow(clippy::as_conversions, clippy::arithmetic_side_effects)] // bounded-width integer arithmetic

use core::cmp::Ordering;

use crate::float::{Exact, Float, Kind, Num};
use crate::status::Rounding;

/// A 256-bit unsigned integer.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub(crate) struct U256 {
    hi: u128,
    lo: u128,
}

impl U256 {
    pub(crate) const fn from(v: u128) -> U256 {
        U256 { hi: 0, lo: v }
    }

    /// The full product of two 128-bit integers.
    pub(crate) fn mul(a: u128, b: u128) -> U256 {
        let (a1, a0) = (a >> 64, a & u128::from(u64::MAX));
        let (b1, b0) = (b >> 64, b & u128::from(u64::MAX));
        let p00 = a0 * b0;
        let p01 = a0 * b1;
        let p10 = a1 * b0;
        let p11 = a1 * b1;
        let (mid, c1) = p01.overflowing_add(p10);
        let (lo, c2) = p00.overflowing_add(mid << 64);
        let hi = p11 + (mid >> 64) + (u128::from(c1) << 64) + u128::from(c2);
        U256 { hi, lo }
    }

    pub(crate) fn bits(self) -> u32 {
        if self.hi != 0 { 256 - self.hi.leading_zeros() } else { 128 - self.lo.leading_zeros() }
    }

    pub(crate) fn is_zero(self) -> bool {
        self.hi == 0 && self.lo == 0
    }

    pub(crate) fn shl(self, n: u32) -> U256 {
        match n {
            0 => self,
            1..=127 => U256 { hi: (self.hi << n) | (self.lo >> (128 - n)), lo: self.lo << n },
            128..=255 => U256 { hi: self.lo << (n - 128), lo: 0 },
            _ => U256::default(),
        }
    }

    /// Shift right; also whether any nonzero bit was shifted out.
    pub(crate) fn shr(self, n: u32) -> (U256, bool) {
        match n {
            0 => (self, false),
            1..=127 => {
                let out = self.lo & ((1u128 << n) - 1) != 0;
                (U256 { hi: self.hi >> n, lo: (self.lo >> n) | (self.hi << (128 - n)) }, out)
            }
            128 => (U256 { hi: 0, lo: self.hi }, self.lo != 0),
            129..=255 => {
                let out = self.lo != 0 || self.hi & ((1u128 << (n - 128)) - 1) != 0;
                (U256 { hi: 0, lo: self.hi >> (n - 128) }, out)
            }
            _ => (U256::default(), !self.is_zero()),
        }
    }

    pub(crate) fn add(self, o: U256) -> U256 {
        let (lo, c) = self.lo.overflowing_add(o.lo);
        U256 { hi: self.hi.wrapping_add(o.hi).wrapping_add(u128::from(c)), lo }
    }

    /// `self - o`, for `self >= o`.
    pub(crate) fn sub(self, o: U256) -> U256 {
        let (lo, b) = self.lo.overflowing_sub(o.lo);
        U256 { hi: self.hi.wrapping_sub(o.hi).wrapping_sub(u128::from(b)), lo }
    }

    pub(crate) fn cmp(self, o: U256) -> Ordering {
        (self.hi, self.lo).cmp(&(o.hi, o.lo))
    }

    pub(crate) fn bit(self, i: u32) -> bool {
        if i >= 128 { (self.hi >> (i - 128)) & 1 == 1 } else { (self.lo >> i) & 1 == 1 }
    }

    pub(crate) fn low(self) -> u128 {
        self.lo
    }
}

/// An exact value `v · 2^exp` (plus a sticky remainder) with a 256-bit significand.
#[derive(Clone, Copy, Debug)]
struct Wide {
    neg: bool,
    v: U256,
    exp: i32,
    sticky: bool,
}

/// Reduce to the 128-bit significand `float::round` takes, keeping the sticky information.
fn narrow(w: Wide) -> Exact {
    let n = w.v.bits();
    if n <= 128 {
        return Exact { neg: w.neg, sig: w.v.low(), exp: w.exp, sticky: w.sticky };
    }
    let (v, out) = w.v.shr(n - 128);
    Exact { neg: w.neg, sig: v.low(), exp: w.exp + (n - 128) as i32, sticky: w.sticky || out }
}

/// The frame position of the larger operand's leading bit in a sum: the operands are at most 226
/// bits long, so the larger fits whole with room for a carry, and the smaller loses only bits far
/// below any rounding position.
const FRAME_TOP: u32 = 252;

/// The exact sum of two finite values (zero allowed), as an exact value of either sign. An exact
/// zero sum is reported with `neg` set as `mode` requires (−0 only when rounding down).
fn sum(a: Wide, b: Wide, mode: Rounding) -> Wide {
    if a.v.is_zero() {
        return b;
    }
    if b.v.is_zero() {
        return a;
    }
    let top = |w: &Wide| w.exp + w.v.bits() as i32 - 1;
    let (big, small) = if top(&a) >= top(&b) { (a, b) } else { (b, a) };
    let fe = top(&big) - FRAME_TOP as i32;
    let bv = big.v.shl((big.exp - fe) as u32);
    let (sv, lost) = if small.exp >= fe {
        (small.v.shl((small.exp - fe) as u32), false)
    } else {
        let d = fe - small.exp;
        small.v.shr(d.min(256) as u32)
    };
    let sticky = lost || small.sticky || big.sticky;
    if big.neg == small.neg {
        return Wide { neg: big.neg, v: bv.add(sv), exp: fe, sticky };
    }
    // Subtraction: a lost part of the smaller magnitude makes the difference slightly less than
    // bv - sv, which is (bv - sv - 1) plus a nonzero fraction.
    match bv.cmp(sv) {
        Ordering::Equal if !sticky => Wide { neg: mode == Rounding::Down, v: U256::default(), exp: fe, sticky: false },
        Ordering::Less => Wide { neg: small.neg, v: sv.sub(bv), exp: fe, sticky },
        _ => {
            let d = bv.sub(sv);
            let d = if lost { d.sub(U256::from(1)) } else { d };
            Wide { neg: big.neg, v: d, exp: fe, sticky }
        }
    }
}

fn wide(n: &Num) -> Wide {
    let v = if n.kind == Kind::Finite { U256::from(n.sig) } else { U256::default() };
    Wide { neg: n.neg, v, exp: n.exp, sticky: false }
}

/// `a + b` of finite or zero values, for rounding into `T`. Zero results carry IEEE's sign: the
/// common sign of two zeros, otherwise +0 (−0 when rounding down).
pub(crate) fn add_exact(a: &Num, b: &Num, mode: Rounding) -> Exact {
    if a.kind == Kind::Zero && b.kind == Kind::Zero {
        let neg = if a.neg == b.neg { a.neg } else { mode == Rounding::Down };
        return Exact { neg, sig: 0, exp: 0, sticky: false };
    }
    narrow(sum(wide(a), wide(b), mode))
}

/// `a · b` of finite or zero values.
pub(crate) fn mul_exact(a: &Num, b: &Num) -> Exact {
    let neg = a.neg != b.neg;
    if a.kind == Kind::Zero || b.kind == Kind::Zero {
        return Exact { neg, sig: 0, exp: 0, sticky: false };
    }
    narrow(Wide { neg, v: U256::mul(a.sig, b.sig), exp: a.exp + b.exp, sticky: false })
}

/// `a / b` of a finite or zero `a` and a finite nonzero `b`, to `T`'s precision plus guard bits.
pub(crate) fn div_exact<T: Float>(a: &Num, b: &Num) -> Exact {
    let neg = a.neg != b.neg;
    if a.kind == Kind::Zero {
        return Exact { neg, sig: 0, exp: 0, sticky: false };
    }
    let (la, lb) = (128 - a.sig.leading_zeros() as i32, 128 - b.sig.leading_zeros() as i32);
    // Shift so the quotient has at least T::PREC + 2 bits.
    let k = (T::PREC as i32 + 3 + lb - la).max(0) as u32;
    let dividend = U256::from(a.sig).shl(k);
    // Restoring division, one quotient bit per step; the remainder stays below 2·b < 2^114.
    let mut rem: u128 = 0;
    let mut q: u128 = 0;
    for i in (0..dividend.bits()).rev() {
        rem = (rem << 1) | u128::from(dividend.bit(i));
        q <<= 1;
        if rem >= b.sig {
            rem -= b.sig;
            q |= 1;
        }
    }
    Exact { neg, sig: q, exp: a.exp - b.exp - k as i32, sticky: rem != 0 }
}

/// The square root of a finite positive `a`, to `T`'s precision plus guard bits.
pub(crate) fn sqrt_exact<T: Float>(a: &Num) -> Exact {
    let la = 128 - a.sig.leading_zeros() as i32;
    // The radicand gets at least 2·(T::PREC + 2) bits and an even exponent.
    let mut k = (2 * (T::PREC as i32 + 2) - la).max(0);
    if (a.exp - k) % 2 != 0 {
        k += 1;
    }
    let r = U256::from(a.sig).shl(k as u32);
    // Digit-by-digit square root, two radicand bits per root bit.
    let mut root: u128 = 0;
    let mut rem: u128 = 0;
    let mut i = r.bits().div_ceil(2) as i32;
    while i > 0 {
        i -= 1;
        let two = (u128::from(r.bit(2 * i as u32 + 1)) << 1) | u128::from(r.bit(2 * i as u32));
        rem = (rem << 2) | two;
        let trial = (root << 2) | 1;
        root <<= 1;
        if rem >= trial {
            rem -= trial;
            root |= 1;
        }
    }
    Exact { neg: false, sig: root, exp: (a.exp - k) / 2, sticky: rem != 0 }
}

/// `√(a² + b²)` of finite or zero values, to `T`'s precision plus guard bits: the squares and
/// their sum exactly in 256 bits, then the root of its leading `2·(T::PREC + 2)` bits, the
/// remainder and every bit below them making the sticky bit (⌊√⌊x⌋⌋ = ⌊√x⌋ for real `x`).
pub(crate) fn hypot_exact<T: Float>(a: &Num, b: &Num) -> Exact {
    let sq = |n: &Num| {
        let v = if n.kind == Kind::Finite { U256::mul(n.sig, n.sig) } else { U256::default() };
        Wide { neg: false, v, exp: 2 * n.exp, sticky: false }
    };
    let s = sum(sq(a), sq(b), Rounding::Nearest);
    if s.v.is_zero() {
        return Exact { neg: false, sig: 0, exp: 0, sticky: false };
    }
    let want = 2 * (T::PREC + 2);
    let n = s.v.bits();
    let (r, exp, below) = if n > want {
        let mut k = n - want;
        if (s.exp + k as i32) % 2 != 0 {
            k += 1;
        }
        let (v, out) = s.v.shr(k);
        (v, s.exp + k as i32, out || s.sticky)
    } else {
        let mut k = want - n;
        if (s.exp - k as i32) % 2 != 0 {
            k += 1;
        }
        (s.v.shl(k), s.exp - k as i32, s.sticky)
    };
    // Digit-by-digit square root: the root has at most T::PREC + 3 bits, the remainder one more.
    let mut root: u128 = 0;
    let mut rem: u128 = 0;
    let mut i = r.bits().div_ceil(2);
    while i > 0 {
        i -= 1;
        let two = (u128::from(r.bit(2 * i + 1)) << 1) | u128::from(r.bit(2 * i));
        rem = (rem << 2) | two;
        let trial = (root << 2) | 1;
        root <<= 1;
        if rem >= trial {
            rem -= trial;
            root |= 1;
        }
    }
    Exact { neg: false, sig: root, exp: exp / 2, sticky: rem != 0 || below }
}

/// `a · b + c` of finite or zero values, exactly, for one rounding.
pub(crate) fn fma_exact(a: &Num, b: &Num, c: &Num, mode: Rounding) -> Exact {
    let pneg = a.neg != b.neg;
    let p = if a.kind == Kind::Zero || b.kind == Kind::Zero {
        Wide { neg: pneg, v: U256::default(), exp: 0, sticky: false }
    } else {
        Wide { neg: pneg, v: U256::mul(a.sig, b.sig), exp: a.exp + b.exp, sticky: false }
    };
    if p.v.is_zero() && c.kind == Kind::Zero {
        let neg = if pneg == c.neg { pneg } else { mode == Rounding::Down };
        return Exact { neg, sig: 0, exp: 0, sticky: false };
    }
    narrow(sum(p, wide(c), mode))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions)]
mod tests {
    use super::*;

    #[test]
    fn u256_product_matches_schoolbook() {
        let cases = [(u128::MAX, u128::MAX), (1u128 << 127, 3), (0xdead_beef_u128 << 70, 0x1234_5678_9abc_def0_u128 << 40), (0, 5)];
        for (a, b) in cases {
            let p = U256::mul(a, b);
            // Check through 64-bit limbs.
            let limbs = |x: u128| [x as u64, (x >> 64) as u64];
            let (al, bl) = (limbs(a), limbs(b));
            let mut want = [0u64; 4];
            for i in 0..2 {
                let mut carry = 0u128;
                for j in 0..2 {
                    let t = u128::from(al[i]) * u128::from(bl[j]) + u128::from(want[i + j]) + carry;
                    want[i + j] = t as u64;
                    carry = t >> 64;
                }
                want[i + 2] = carry as u64;
            }
            let lo = u128::from(want[0]) | (u128::from(want[1]) << 64);
            let hi = u128::from(want[2]) | (u128::from(want[3]) << 64);
            assert_eq!(p, U256 { hi, lo }, "{a:x} * {b:x}");
        }
    }

    #[test]
    fn shifts_report_lost_bits() {
        let x = U256 { hi: 1, lo: 1 };
        assert_eq!(x.shr(1), (U256 { hi: 0, lo: 1u128 << 127 }, true));
        assert_eq!(x.shr(128), (U256 { hi: 0, lo: 1 }, true));
        assert_eq!(U256 { hi: 2, lo: 0 }.shr(129), (U256 { hi: 0, lo: 1 }, false));
        assert_eq!(x.shl(127).shr(127).0, x);
        assert_eq!(x.bits(), 129);
    }
}
