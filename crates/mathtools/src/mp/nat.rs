//! Unsigned big integers: little-endian `u64` limbs with no high zero limbs, so every value has
//! exactly one representation and `Vec` equality is numeric equality.
//!
//! Schoolbook multiplication; division is Knuth's Algorithm D (TAOCP vol. 2, 4.3.1); square and
//! cube roots are Newton iterations from above, started from a floating-point estimate.

use std::cmp::Ordering;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Nat {
    d: Vec<u64>,
}

impl Nat {
    pub(crate) fn zero() -> Nat {
        Nat { d: Vec::new() }
    }

    pub(crate) fn from_u64(v: u64) -> Nat {
        let mut n = Nat { d: vec![v] };
        n.trim();
        n
    }

    pub(crate) fn from_u128(v: u128) -> Nat {
        let mut n = Nat { d: vec![v as u64, (v >> 64) as u64] };
        n.trim();
        n
    }

    fn from_limbs(d: Vec<u64>) -> Nat {
        let mut n = Nat { d };
        n.trim();
        n
    }

    fn trim(&mut self) {
        while self.d.last() == Some(&0) {
            self.d.pop();
        }
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.d.is_empty()
    }

    pub(crate) fn is_one(&self) -> bool {
        self.d.len() == 1 && self.d[0] == 1
    }

    pub(crate) fn is_odd(&self) -> bool {
        self.d.first().is_some_and(|&l| l & 1 == 1)
    }

    /// Number of significant bits (0 for zero).
    pub(crate) fn bit_len(&self) -> u64 {
        match self.d.last() {
            None => 0,
            Some(&t) => self.d.len() as u64 * 64 - u64::from(t.leading_zeros()),
        }
    }

    /// Number of trailing zero bits (0 for zero).
    pub(crate) fn trailing_zeros(&self) -> u64 {
        for (i, &l) in self.d.iter().enumerate() {
            if l != 0 {
                return i as u64 * 64 + u64::from(l.trailing_zeros());
            }
        }
        0
    }

    pub(crate) fn bit(&self, i: u64) -> bool {
        let li = i / 64;
        if li >= self.d.len() as u64 {
            return false;
        }
        (self.d[li as usize] >> (i % 64)) & 1 == 1
    }

    /// True if any of the bits below position `k` is set.
    pub(crate) fn low_bits_nonzero(&self, k: u64) -> bool {
        let full = (k / 64).min(self.d.len() as u64) as usize;
        if self.d[..full].iter().any(|&l| l != 0) {
            return true;
        }
        let r = k % 64;
        if (full as u64) < k / 64 || full >= self.d.len() || r == 0 {
            return false;
        }
        self.d[full] & ((1u64 << r) - 1) != 0
    }

    pub(crate) fn to_u128(&self) -> Option<u128> {
        match self.d.len() {
            0 => Some(0),
            1 => Some(u128::from(self.d[0])),
            2 => Some(u128::from(self.d[0]) | (u128::from(self.d[1]) << 64)),
            _ => None,
        }
    }

    pub(crate) fn cmp(&self, o: &Nat) -> Ordering {
        if self.d.len() != o.d.len() {
            return self.d.len().cmp(&o.d.len());
        }
        for (a, b) in self.d.iter().rev().zip(o.d.iter().rev()) {
            if a != b {
                return a.cmp(b);
            }
        }
        Ordering::Equal
    }

    pub(crate) fn add(&self, o: &Nat) -> Nat {
        let (a, b) = if self.d.len() >= o.d.len() { (self, o) } else { (o, self) };
        let mut r = Vec::with_capacity(a.d.len() + 1);
        let mut carry = false;
        for (i, &x) in a.d.iter().enumerate() {
            let y = b.d.get(i).copied().unwrap_or(0);
            let (s1, c1) = x.overflowing_add(y);
            let (s2, c2) = s1.overflowing_add(u64::from(carry));
            r.push(s2);
            carry = c1 || c2;
        }
        if carry {
            r.push(1);
        }
        Nat::from_limbs(r)
    }

    pub(crate) fn add_small(&self, v: u64) -> Nat {
        self.add(&Nat::from_u64(v))
    }

    /// `self - o`, saturating at zero (callers only subtract a smaller value).
    pub(crate) fn sub(&self, o: &Nat) -> Nat {
        if self.cmp(o) != Ordering::Greater {
            return Nat::zero();
        }
        let mut r = Vec::with_capacity(self.d.len());
        let mut borrow = false;
        for (i, &x) in self.d.iter().enumerate() {
            let y = o.d.get(i).copied().unwrap_or(0);
            let (s1, b1) = x.overflowing_sub(y);
            let (s2, b2) = s1.overflowing_sub(u64::from(borrow));
            r.push(s2);
            borrow = b1 || b2;
        }
        Nat::from_limbs(r)
    }

    pub(crate) fn mul(&self, o: &Nat) -> Nat {
        if self.is_zero() || o.is_zero() {
            return Nat::zero();
        }
        let (a, b) = (&self.d, &o.d);
        let mut r = vec![0u64; a.len() + b.len()];
        for (i, &ai) in a.iter().enumerate() {
            if ai == 0 {
                continue;
            }
            let mut carry: u128 = 0;
            for (j, &bj) in b.iter().enumerate() {
                // (2^64-1)^2 + 2(2^64-1) = 2^128-1: no overflow.
                let t = u128::from(ai) * u128::from(bj) + u128::from(r[i + j]) + carry;
                r[i + j] = t as u64;
                carry = t >> 64;
            }
            r[i + b.len()] = carry as u64;
        }
        Nat::from_limbs(r)
    }

    pub(crate) fn mul_small(&self, m: u64) -> Nat {
        if m == 0 || self.is_zero() {
            return Nat::zero();
        }
        let mut r = Vec::with_capacity(self.d.len() + 1);
        let mut carry: u128 = 0;
        for &x in &self.d {
            let t = u128::from(x) * u128::from(m) + carry;
            r.push(t as u64);
            carry = t >> 64;
        }
        r.push(carry as u64);
        Nat::from_limbs(r)
    }

    pub(crate) fn shl(&self, k: u64) -> Nat {
        if self.is_zero() {
            return Nat::zero();
        }
        let limbs = (k / 64) as usize;
        let bits = (k % 64) as u32;
        let mut r = vec![0u64; limbs];
        r.reserve(self.d.len() + 1);
        if bits == 0 {
            r.extend_from_slice(&self.d);
        } else {
            let mut prev = 0u64;
            for &x in &self.d {
                r.push((x << bits) | (prev >> (64 - bits)));
                prev = x;
            }
            r.push(prev >> (64 - bits));
        }
        Nat::from_limbs(r)
    }

    pub(crate) fn shr(&self, k: u64) -> Nat {
        let limbs = k / 64;
        if limbs >= self.d.len() as u64 {
            return Nat::zero();
        }
        let src = &self.d[limbs as usize..];
        let bits = (k % 64) as u32;
        if bits == 0 {
            return Nat::from_limbs(src.to_vec());
        }
        let mut r = Vec::with_capacity(src.len());
        for (i, &x) in src.iter().enumerate() {
            let hi = src.get(i + 1).copied().unwrap_or(0);
            r.push((x >> bits) | (hi << (64 - bits)));
        }
        Nat::from_limbs(r)
    }

    /// Quotient and remainder by a single limb; division by zero gives (0, 0).
    pub(crate) fn divrem_small(&self, v: u64) -> (Nat, u64) {
        if v == 0 {
            return (Nat::zero(), 0);
        }
        let mut q = vec![0u64; self.d.len()];
        let mut rem: u128 = 0;
        for i in (0..self.d.len()).rev() {
            let cur = (rem << 64) | u128::from(self.d[i]);
            q[i] = (cur / u128::from(v)) as u64;
            rem = cur % u128::from(v);
        }
        (Nat::from_limbs(q), rem as u64)
    }

    /// Quotient and remainder (Knuth, Algorithm D). Division by zero gives (0, self).
    pub(crate) fn divrem(&self, v: &Nat) -> (Nat, Nat) {
        if v.is_zero() {
            return (Nat::zero(), self.clone());
        }
        if self.cmp(v) == Ordering::Less {
            return (Nat::zero(), self.clone());
        }
        if v.d.len() == 1 {
            let (q, r) = self.divrem_small(v.d[0]);
            return (q, Nat::from_u64(r));
        }
        let n = v.d.len();
        let m = self.d.len() - n;
        // Normalize so the divisor's top limb has its high bit set; then each quotient
        // estimate from the top two limbs is at most 2 too large.
        let s = u64::from(v.d[n - 1].leading_zeros());
        let vn = v.shl(s).d;
        let mut un = self.shl(s).d;
        un.resize(self.d.len() + 1, 0);
        let b: u128 = 1 << 64;
        let mut q = vec![0u64; m + 1];
        for j in (0..=m).rev() {
            let num = (u128::from(un[j + n]) << 64) | u128::from(un[j + n - 1]);
            let top = u128::from(vn[n - 1]);
            let mut qhat = num / top;
            let mut rhat = num % top;
            while qhat >= b || qhat * u128::from(vn[n - 2]) > ((rhat << 64) | u128::from(un[j + n - 2])) {
                qhat -= 1;
                rhat += top;
                if rhat >= b {
                    break;
                }
            }
            // Multiply and subtract qhat * vn from un[j..=j+n].
            let mut borrow: u64 = 0;
            let mut carry: u128 = 0;
            for i in 0..n {
                let p = qhat * u128::from(vn[i]) + carry;
                carry = p >> 64;
                let (t1, b1) = un[i + j].overflowing_sub(p as u64);
                let (t2, b2) = t1.overflowing_sub(borrow);
                un[i + j] = t2;
                borrow = u64::from(b1) + u64::from(b2);
            }
            let (t1, b1) = un[j + n].overflowing_sub(carry as u64);
            let (t2, b2) = t1.overflowing_sub(borrow);
            un[j + n] = t2;
            if b1 || b2 {
                // qhat was one too large: add the divisor back.
                qhat -= 1;
                let mut c = false;
                for i in 0..n {
                    let (s1, c1) = un[i + j].overflowing_add(vn[i]);
                    let (s2, c2) = s1.overflowing_add(u64::from(c));
                    un[i + j] = s2;
                    c = c1 || c2;
                }
                un[j + n] = un[j + n].wrapping_add(u64::from(c));
            }
            q[j] = qhat as u64;
        }
        un.truncate(n);
        (Nat::from_limbs(q), Nat::from_limbs(un).shr(s))
    }

    pub(crate) fn pow(&self, mut e: u64) -> Nat {
        let mut base = self.clone();
        let mut r = Nat::from_u64(1);
        while e > 0 {
            if e & 1 == 1 {
                r = r.mul(&base);
            }
            e >>= 1;
            if e > 0 {
                base = base.mul(&base);
            }
        }
        r
    }

    /// `floor(sqrt(self))`. Newton's iteration `x <- (x + n/x)/2` decreases monotonically from
    /// any start `>= floor(sqrt(n))` and stops at it (the first `y >= x`).
    pub(crate) fn isqrt(&self) -> Nat {
        if self.is_zero() {
            return Nat::zero();
        }
        let (t, s) = self.shr_even_top();
        // sqrt(t) has relative error < 2^-52; the 2^-40 margin and +2 keep the start above
        // sqrt(t + 1) >= the true root of the shifted-out value.
        let r = (t.sqrt() * (1.0 + 2f64.powi(-40))) as u128 + 2;
        let mut x = Nat::from_u128(r).shl(s / 2);
        loop {
            let (q, _) = self.divrem(&x);
            let y = x.add(&q).shr(1);
            if y.cmp(&x) != Ordering::Less {
                return x;
            }
            x = y;
        }
    }

    /// `floor(cbrt(self))`, by Newton's iteration `x <- (2x + n/x^2)/3` from above (by the
    /// AM-GM inequality each iterate stays `>= floor(cbrt(n))`).
    pub(crate) fn icbrt(&self) -> Nat {
        if self.is_zero() {
            return Nat::zero();
        }
        let s0 = self.bit_len().saturating_sub(100);
        let s = s0 + (3 - s0 % 3) % 3;
        let t = self.shr(s).to_u128().unwrap_or(0) as f64;
        let r = (t.cbrt() * (1.0 + 2f64.powi(-40))) as u128 + 2;
        let mut x = Nat::from_u128(r).shl(s / 3);
        loop {
            let (q, _) = self.divrem(&x.mul(&x));
            let (y, _) = x.shl(1).add(&q).divrem_small(3);
            if y.cmp(&x) != Ordering::Less {
                return x;
            }
            x = y;
        }
    }

    /// The top bits of `self` with an even shift: `(self >> s) as f64`, `s` even.
    fn shr_even_top(&self) -> (f64, u64) {
        let s = self.bit_len().saturating_sub(100) & !1;
        (self.shr(s).to_u128().unwrap_or(0) as f64, s)
    }

    /// Lower-case hexadecimal digits, at least `min_digits` of them.
    pub(crate) fn to_hex(&self, min_digits: usize) -> String {
        let mut s = String::new();
        for (i, &l) in self.d.iter().rev().enumerate() {
            if i == 0 {
                s.push_str(&format!("{l:x}"));
            } else {
                s.push_str(&format!("{l:016x}"));
            }
        }
        if s.is_empty() {
            s.push('0');
        }
        if s.len() < min_digits {
            s = "0".repeat(min_digits - s.len()) + &s;
        }
        s
    }

    /// Decimal digits.
    pub(crate) fn to_dec(&self) -> String {
        const CHUNK: u64 = 10_000_000_000_000_000_000;
        let mut parts = Vec::new();
        let mut n = self.clone();
        while !n.is_zero() {
            let (q, r) = n.divrem_small(CHUNK);
            parts.push(r);
            n = q;
        }
        let mut s = String::new();
        for (i, p) in parts.iter().rev().enumerate() {
            if i == 0 {
                s.push_str(&p.to_string());
            } else {
                s.push_str(&format!("{p:019}"));
            }
        }
        if s.is_empty() {
            s.push('0');
        }
        s
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            // splitmix64
            self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
            let mut z = self.0;
            z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
            z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
            z ^ (z >> 31)
        }
        /// A u128 with a random number of significant bits, so small values are frequent.
        fn u128(&mut self) -> u128 {
            let v = (u128::from(self.next()) << 64) | u128::from(self.next());
            v >> (self.next() % 128)
        }
        fn nat(&mut self, max_limbs: u64) -> Nat {
            let n = self.next() % (max_limbs + 1);
            let mut d: Vec<u64> = (0..n).map(|_| self.next()).collect();
            if let Some(l) = d.last_mut() {
                *l >>= self.next() % 64;
            }
            // Runs of all-ones or zero limbs stress the carry and correction paths.
            for l in &mut d {
                match self.next() % 8 {
                    0 => *l = u64::MAX,
                    1 => *l = 0,
                    _ => {}
                }
            }
            Nat::from_limbs(d)
        }
    }

    #[test]
    fn against_u128() {
        let mut g = Rng(1);
        for _ in 0..200_000 {
            let (a, b) = (g.u128(), g.u128());
            let (na, nb) = (Nat::from_u128(a), Nat::from_u128(b));
            if let Some(s) = a.checked_add(b) {
                assert_eq!(na.add(&nb).to_u128(), Some(s));
            }
            assert_eq!(na.sub(&nb).to_u128(), Some(a.saturating_sub(b)));
            if let Some(p) = a.checked_mul(b) {
                assert_eq!(na.mul(&nb).to_u128(), Some(p));
            }
            if let (Some(wq), Some(wr)) = (a.checked_div(b), a.checked_rem(b)) {
                let (q, r) = na.divrem(&nb);
                assert_eq!((q.to_u128(), r.to_u128()), (Some(wq), Some(wr)), "{a} / {b}");
            }
            assert_eq!(na.cmp(&nb), a.cmp(&b));
            let k = g.next() % 140;
            assert_eq!(na.shr(k).to_u128(), Some(a.checked_shr(k as u32).unwrap_or(0)));
            if k < 128 && (a.leading_zeros() as u64) >= k {
                assert_eq!(na.shl(k).to_u128(), Some(a << k));
            }
            assert_eq!(na.bit_len(), u64::from(128 - a.leading_zeros()));
            assert_eq!(na.isqrt().to_u128(), Some(isqrt_u128(a)));
            assert_eq!(na.low_bits_nonzero(k), k > 0 && a & (u128::MAX >> (128 - k.min(128))) != 0);
        }
    }

    fn isqrt_u128(a: u128) -> u128 {
        let mut lo = 0u128;
        let mut hi = 1u128 << 64;
        while hi - lo > 1 {
            let mid = lo + (hi - lo) / 2;
            if mid.checked_mul(mid).is_some_and(|p| p <= a) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        lo
    }

    #[test]
    fn multi_limb_division_identity() {
        let mut g = Rng(2);
        for _ in 0..30_000 {
            let b = g.nat(12);
            if b.is_zero() {
                continue;
            }
            let q = g.nat(12);
            let r = g.nat(12).divrem(&b).1;
            let a = q.mul(&b).add(&r);
            let (q2, r2) = a.divrem(&b);
            assert_eq!((q2, r2), (q, r));
        }
    }

    #[test]
    fn roots() {
        let mut g = Rng(3);
        for _ in 0..3000 {
            let n = g.nat(20);
            let s = n.isqrt();
            assert!(s.mul(&s).cmp(&n) != Ordering::Greater);
            let s1 = s.add_small(1);
            assert_eq!(s1.mul(&s1).cmp(&n), Ordering::Greater);
            let c = n.icbrt();
            assert!(c.pow(3).cmp(&n) != Ordering::Greater);
            assert_eq!(c.add_small(1).pow(3).cmp(&n), Ordering::Greater);
            // Exact powers.
            let x = g.nat(5);
            assert_eq!(x.mul(&x).isqrt(), x);
            assert_eq!(x.pow(3).icbrt(), x);
        }
    }

    #[test]
    fn text() {
        assert_eq!(Nat::from_u128(u128::MAX).to_dec(), u128::MAX.to_string());
        assert_eq!(Nat::from_u128(0xabc_0000_0000_0000_0001).to_hex(0), "abc0000000000000001");
        assert_eq!(Nat::from_u64(10).pow(40).to_dec(), format!("1{}", "0".repeat(40)));
    }
}
