//! Fixed-capacity unsigned big integers for the exact float engine (IP-18).
//!
//! Capacity is fixed (no allocation, so conversions cannot fail with `ENOMEM`) and large enough
//! for every value the engine forms: at most 65,536 bits, which covers binary128's widest
//! decimal-to-binary scaling (12,000 significant digits times or over a power of five; see
//! `parse`). Operations saturate rather than overflow if a caller ever exceeded the capacity,
//! and report it through [`Big::overflowed`].
//!
//! Representation (D-8.11 (1)): little-endian u64 limbs with u128
//! products, and the count of limbs in use, so each operation costs the value's size.

#![allow(clippy::indexing_slicing, clippy::as_conversions, clippy::cast_possible_truncation, clippy::arithmetic_side_effects)] // limb indices < len <= LIMBS; u128 limb products cannot overflow

use core::cmp::Ordering;

/// Limbs (64 bits each).
pub const LIMBS: usize = 1024;

/// An unsigned integer of up to `64 * LIMBS` bits.
#[derive(Clone)]
pub struct Big {
    limbs: [u64; LIMBS],
    /// Limbs in use: `limbs[len..]` are zero and `limbs[len - 1] != 0` (0 for zero).
    len: usize,
    over: bool,
}

impl Default for Big {
    fn default() -> Self {
        Big::zero()
    }
}

impl Big {
    /// Zero.
    pub const fn zero() -> Big {
        Big { limbs: [0; LIMBS], len: 0, over: false }
    }

    /// A small value.
    pub fn from_u128(v: u128) -> Big {
        let mut b = Big::zero();
        b.limbs[0] = v as u64;
        b.limbs[1] = (v >> 64) as u64;
        b.len = 2;
        b.trim();
        b
    }

    fn trim(&mut self) {
        while self.len > 0 && self.limbs[self.len - 1] == 0 {
            self.len -= 1;
        }
    }

    /// Append a carry limb, or record the overflow.
    fn push(&mut self, carry: u64) {
        if carry != 0 {
            if self.len < LIMBS {
                self.limbs[self.len] = carry;
                self.len += 1;
            } else {
                self.over = true;
            }
        }
    }

    /// True if an operation exceeded the capacity (the value is then meaningless).
    pub fn overflowed(&self) -> bool {
        self.over
    }

    /// True for zero.
    pub fn is_zero(&self) -> bool {
        self.len == 0
    }

    /// Number of significant bits (0 for zero).
    pub fn bit_len(&self) -> u32 {
        if self.len == 0 { 0 } else { (self.len as u32 - 1) * 64 + (64 - self.limbs[self.len - 1].leading_zeros()) }
    }

    /// `self = self * m + a`.
    pub fn mul_add_small(&mut self, m: u64, a: u64) {
        let mut carry = a;
        for l in &mut self.limbs[..self.len] {
            let t = u128::from(*l) * u128::from(m) + u128::from(carry);
            *l = t as u64;
            carry = (t >> 64) as u64;
        }
        self.push(carry);
        self.trim();
    }

    /// `self *= 5^n`.
    pub fn mul_pow5(&mut self, mut n: u32) {
        // 5^27 is the largest power of five below 2^64.
        while n > 0 {
            let k = n.min(27);
            self.mul_add_small(5u64.pow(k), 0);
            n -= k;
        }
    }

    /// `self *= 10^n`.
    pub fn mul_pow10(&mut self, n: u32) {
        self.mul_pow5(n);
        self.shl(n);
    }

    /// `self <<= n`.
    pub fn shl(&mut self, n: u32) {
        if self.len == 0 || n == 0 {
            return;
        }
        let (words, bits) = ((n / 64) as usize, n % 64);
        let new_len = self.len + words + 1;
        if new_len > LIMBS && (self.bit_len() as usize + n as usize) > LIMBS * 64 {
            self.over = true;
            return;
        }
        let top = new_len.min(LIMBS);
        for i in (0..top).rev() {
            let hi = i.checked_sub(words).filter(|&j| j < self.len).map_or(0, |j| self.limbs[j]);
            let lo = i.checked_sub(words + 1).filter(|&j| j < self.len).map_or(0, |j| self.limbs[j]);
            self.limbs[i] = if bits == 0 { hi } else { (hi << bits) | (lo >> (64 - bits)) };
        }
        self.len = top;
        self.trim();
    }

    /// `self >>= 1`.
    pub fn shr1(&mut self) {
        for i in 0..self.len {
            let next = if i + 1 < self.len { self.limbs[i + 1] } else { 0 };
            self.limbs[i] = (self.limbs[i] >> 1) | (next << 63);
        }
        self.trim();
    }

    /// `self -= other`, assuming `self >= other`.
    pub fn sub(&mut self, other: &Big) {
        let mut borrow = false;
        for i in 0..self.len {
            let o = if i < other.len { other.limbs[i] } else { 0 };
            let (d1, b1) = self.limbs[i].overflowing_sub(o);
            let (d2, b2) = d1.overflowing_sub(u64::from(borrow));
            self.limbs[i] = d2;
            borrow = b1 || b2;
        }
        self.trim();
    }

    /// Divide by a small non-zero value; returns the remainder (`d = 0` leaves `self` and
    /// returns 0).
    pub fn divrem_small(&mut self, d: u64) -> u64 {
        if d == 0 {
            return 0;
        }
        let mut rem = 0u64;
        for l in self.limbs[..self.len].iter_mut().rev() {
            let t = (u128::from(rem) << 64) | u128::from(*l);
            *l = (t / u128::from(d)) as u64;
            rem = (t % u128::from(d)) as u64;
        }
        self.trim();
        rem
    }

    /// Divide by 10^9, two 64-bit divisions by a constant per limb (the compiler makes them
    /// multiplications); returns the remainder.
    pub fn divrem_1e9(&mut self) -> u32 {
        const D: u64 = 1_000_000_000;
        let mut rem = 0u64;
        for l in self.limbs[..self.len].iter_mut().rev() {
            let hi = (rem << 32) | (*l >> 32);
            let (qh, rh) = (hi / D, hi % D);
            let lo = (rh << 32) | (*l & 0xffff_ffff);
            let (ql, rl) = (lo / D, lo % D);
            *l = (qh << 32) | ql;
            rem = rl;
        }
        self.trim();
        rem as u32
    }

    /// Remove and return the bits at and above bit `k` (the caller knows they fit 64 bits).
    pub fn split_above(&mut self, k: u32) -> u64 {
        let (w, b) = ((k / 64) as usize, k % 64);
        let mut v = 0u64;
        if w < self.len {
            v = self.limbs[w] >> b;
            if b != 0 && w + 1 < self.len {
                v |= self.limbs[w + 1] << (64 - b);
            }
            self.limbs[w] &= (1u64 << b).wrapping_sub(1);
            for l in &mut self.limbs[w + 1..self.len] {
                *l = 0;
            }
            self.trim();
        }
        v
    }

    /// The top 128 bits as an integer, the number of bits below them, and whether any of those
    /// lower bits is set: `self = top * 2^shift + rest` with `rest != 0` as the flag.
    pub fn top128(&self) -> (u128, u32, bool) {
        let n = self.bit_len();
        if n <= 128 {
            return (self.low128(), 0, false);
        }
        let shift = n - 128;
        let top = (0..128).fold(0u128, |acc, i| acc | (u128::from(self.bit(shift + i)) << i));
        let (w, b) = ((shift / 64) as usize, shift % 64);
        let rest = self.limbs[..w].iter().any(|&l| l != 0) || self.limbs[w] & ((1u64 << b).wrapping_sub(1)) != 0;
        (top, shift, rest)
    }

    /// Bit `i`.
    pub fn bit(&self, i: u32) -> bool {
        let w = (i / 64) as usize;
        w < self.len && (self.limbs[w] >> (i % 64)) & 1 == 1
    }

    /// Low 128 bits.
    pub fn low128(&self) -> u128 {
        u128::from(self.limbs[0]) | (u128::from(self.limbs[1]) << 64)
    }

    /// `floor(self / d)` for a quotient known to be below 2^128; `self` becomes the remainder.
    pub fn div_small_quotient(&mut self, d: &Big) -> u128 {
        if d.is_zero() || self.cmp_big(d) == Ordering::Less {
            return 0;
        }
        // Shift-and-subtract over the quotient's bits, the divisor shifted once and halved.
        let k = (self.bit_len() - d.bit_len()).min(127);
        let mut t = d.clone();
        t.shl(k);
        let mut q = 0u128;
        for i in (0..=k).rev() {
            if self.cmp_big(&t) != Ordering::Less {
                self.sub(&t);
                q |= 1 << i;
            }
            t.shr1();
        }
        q
    }

    /// Compare.
    pub fn cmp_big(&self, other: &Big) -> Ordering {
        self.len.cmp(&other.len).then_with(|| {
            for i in (0..self.len).rev() {
                match self.limbs[i].cmp(&other.limbs[i]) {
                    Ordering::Equal => {}
                    o => return o,
                }
            }
            Ordering::Equal
        })
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_agrees_with_u128() {
        let mut x = 0x9E37_79B9_7F4A_7C15u64;
        for _ in 0..2000 {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            let a = u128::from(x >> (x % 64)) * u128::from(x % 1000 + 1);
            let mut b = Big::from_u128(a);
            assert_eq!((b.low128(), b.bit_len()), (a, 128 - a.leading_zeros()));
            let mut c = b.clone();
            c.mul_add_small(7, 3);
            if let Some(w) = a.checked_mul(7).and_then(|v| v.checked_add(3)) {
                assert_eq!(c.low128(), w);
            }
            let s = (x % 40) as u32;
            let mut d = b.clone();
            d.shl(s);
            if a.leading_zeros() >= s {
                assert_eq!(d.low128(), a << s);
                assert_eq!(d.split_above(s), a as u64, "split_above returns the bits from s up");
                assert!(d.is_zero());
            }
            let r = b.divrem_small(1_000_000_000);
            assert_eq!((b.low128(), u128::from(r)), (a / 1_000_000_000, a % 1_000_000_000));
            let mut e = Big::from_u128(a);
            let r9 = e.divrem_1e9();
            assert_eq!((e.low128(), u128::from(r9)), (a / 1_000_000_000, a % 1_000_000_000));
        }
    }

    #[test]
    fn long_division_and_top_bits() {
        let mut n = Big::from_u128(3);
        n.mul_pow10(300);
        let mut d = Big::from_u128(7);
        d.mul_pow10(280);
        let q = n.div_small_quotient(&d);
        assert_eq!(q, 3 * 10u128.pow(20) / 7);
        assert!(n.cmp_big(&d) == Ordering::Less, "the remainder is below the divisor");
        let mut p = Big::from_u128(1);
        p.shl(1000);
        p.mul_add_small(1, 1);
        assert_eq!(p.top128(), (1 << 127, 873, true));
        assert!(!p.overflowed());
        let mut big = Big::from_u128(1);
        big.shl(65_535);
        assert!(!big.overflowed());
        big.shl(1);
        assert!(big.overflowed());
    }
}
