//! Shared test helpers: a deterministic generator and error measures.

#![allow(dead_code, unreachable_pub)]

use liella_mathtools::mp::{Approx, Big};

/// splitmix64.
pub struct Rng(pub u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f64 {
        (self.next() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in [a, b).
    pub fn range(&mut self, a: f64, b: f64) -> f64 {
        a + (b - a) * self.unit()
    }

    /// Any f64 bit pattern, biased toward interesting ones (subnormals, near overflow, small
    /// integers, specials).
    pub fn f64_any(&mut self) -> f64 {
        let r = self.next();
        match r % 16 {
            0 => f64::from_bits(self.next() & 0x800f_ffff_ffff_ffff), // subnormal
            1 => f64::from_bits((self.next() & 0x800f_ffff_ffff_ffff) | 0x7fe0_0000_0000_0000), // near max
            2 => (self.next() % 2000) as f64 - 1000.0,
            3 => [0.0, -0.0, f64::INFINITY, f64::NEG_INFINITY, f64::NAN, 1.0, f64::MIN_POSITIVE][(self.next() % 7) as usize],
            _ => f64::from_bits(self.next()),
        }
    }

    /// A finite f64 with exponent uniform in [lo, hi), random sign.
    pub fn f64_exp(&mut self, lo: i32, hi: i32) -> f64 {
        let e = lo + (self.next() % (hi - lo) as u64) as i32;
        let m = 1.0 + self.unit();
        let v = m * 2f64.powi(e);
        if self.next() & 1 == 1 { -v } else { v }
    }

    /// A random 113-bit significand value in [1, 2) scaled by 2^e.
    pub fn big113(&mut self, e: i64) -> Big {
        let m = (u128::from(self.next()) << 64 | u128::from(self.next())) >> 15 | (1u128 << 112);
        Big::from_u128(m).mul_pow2(e - 112)
    }
}

/// log2 of the relative difference |a - b| / |b| (-inf if equal).
pub fn rel_log2(a: &Big, b: &Big) -> f64 {
    let d = a.sub(b);
    if d.is_zero() {
        return f64::NEG_INFINITY;
    }
    let (da, db) = (d.ilogb().unwrap_or(0), b.ilogb().unwrap_or(0));
    // Within one of the true value, enough for these checks.
    (da - db) as f64
}

/// log2 of the absolute difference.
pub fn abs_log2(a: &Big, b: &Big) -> f64 {
    let d = a.sub(b);
    d.ilogb().map_or(f64::NEG_INFINITY, |e| e as f64)
}

pub fn v(a: Approx) -> Big {
    a.value
}

pub fn f(x: f64) -> Big {
    Big::from_f64(x)
}
