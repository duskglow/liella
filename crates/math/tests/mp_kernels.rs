//! The fixed-precision kernels of the special functions (`liella_math::mp`) against the test
//! oracle (`liella-mathtools::mp`): every operation and transcendental kernel within the
//! relative error its documentation states, on a deterministic sample of operands.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::cast_possible_wrap, clippy::float_cmp, missing_docs)]

use liella_math::mp::{self, Mp, LIMBS};
use liella_mathtools::mp::{self as o, Big};

fn big(x: Mp) -> Big {
    let mut v = Big::zero(false);
    for i in (0..LIMBS).rev() {
        v = v.mul_pow2(64).add(&Big::from_u64(x.m[i]));
    }
    let v = v.mul_pow2(x.exp - 64 * LIMBS as i64);
    if x.neg { v.neg() } else { v }
}

/// `-log2` of the relative error of `got` against `want` (999 when equal).
fn bits(got: Mp, want: &Big) -> i64 {
    let d = big(got).sub(want);
    if d.is_zero() {
        return 999;
    }
    want.ilogb().unwrap() - d.ilogb().unwrap()
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn f(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (self.next() >> 11) as f64 / (1u64 << 53) as f64 * (hi - lo)
    }
}

const P: u32 = 500;

#[test]
fn arithmetic_is_within_one_unit_of_384_bits() {
    // no spec clause: the multiprecision kernels are an internal engine (D-10.12).
    let mut r = Rng(0x1234_5678);
    for _ in 0..2000 {
        let (a, b) = (r.f(-1e6, 1e6), r.f(-3.0, 3.0));
        let (x, y) = (Mp::from_f64(a).div_u64(7).add(Mp::from_f64(b)), Mp::from_f64(b).div_u64(13).sub(Mp::from_f64(a)));
        let (bx, by) = (big(x), big(y));
        assert!(bits(x.mul(y), &bx.mul(&by)) >= 382);
        assert!(bits(x.add(y), &bx.add(&by)) >= 380 || bx.add(&by).is_zero());
        assert!(bits(x.div(y), &bx.div(&by, P, o::Round::Nearest)) >= 378);
        assert!(bits(x.abs().sqrt(), &bx.abs().sqrt(P, o::Round::Nearest)) >= 378);
        assert!(bits(x.mul_u64(1_000_003), &bx.mul(&Big::from_i64(1_000_003))) >= 382);
        assert!(bits(x.div_u64(1_000_003), &bx.div(&Big::from_i64(1_000_003), P, o::Round::Nearest)) >= 382);
    }
}

#[test]
fn transcendental_kernels_are_within_2_to_the_minus_360() {
    // no spec clause: the multiprecision kernels are an internal engine (D-10.12).
    let mut r = Rng(0x9e37_79b9);
    for i in 0..400 {
        let a = if i % 2 == 0 { r.f(-745.0, 710.0) } else { r.f(-1.0, 1.0) };
        let x = Mp::from_f64(a).add(Mp::from_f64(r.f(0.0, 1.0)).ldexp(-60));
        let bx = big(x);
        assert!(bits(mp::exp(x), &o::exp(&bx, P).value) >= 360, "exp {a}");
        let p = Mp::from_f64(r.f(1e-300, 1e300).abs()).add(Mp::from_f64(r.f(0.0, 1.0)).ldexp(-80));
        let want = o::log(&big(p), P).value;
        assert!(bits(mp::ln(p), &want) >= 360 || want.abs().to_f64() < 1e-10, "ln {}", p.to_f64());
        let s = Mp::from_f64(r.f(-0.8, 0.8));
        let (sn, cs) = mp::sin_cos_small(s);
        assert!(bits(sn, &o::sin(&big(s), P).value) >= 370);
        assert!(bits(cs, &o::cos(&big(s), P).value) >= 370);
    }
    // ln near 1: absolute accuracy.
    let near = Mp::from_i64(1).add(Mp::from_i64(1).ldexp(-200));
    assert!(bits(mp::ln(near), &o::log(&big(near), P).value) >= 170);
}

#[test]
fn reduction_by_half_pi_matches_the_oracle() {
    // no spec clause: the multiprecision kernels are an internal engine (D-10.12).
    let mut r = Rng(0x0dd_ba11);
    for i in 0..300 {
        let x = match i % 3 {
            0 => r.f(-1e6, 1e6),
            1 => f64::from_bits((r.next() & 0x7fef_ffff_ffff_ffff) | 0x4000_0000_0000_0000),
            _ => r.f(1e9, 1e15),
        };
        let (k, rr) = mp::reduce_half_pi(x);
        // sin x from the reduction against the oracle's sin x.
        let (s, c) = mp::sin_cos_small(rr);
        let sin = match k {
            0 => s,
            1 => c,
            2 => s.neg(),
            _ => c.neg(),
        };
        let want = o::sin(&Big::from_f64(x), P).value;
        assert!(bits(sin, &want) >= 300, "sin {x:e}: k {k}");
    }
}
