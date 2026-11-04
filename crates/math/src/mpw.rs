//! The wider second step of Ziv's strategy for the special functions (spec §10.3, Track M
//! milestone 5, D-10.20): the arithmetic of [`crate::mp`] with a 768-bit significand
//! (`mp::mp_core`, expanded here with twelve limbs), and the few kernels the Bessel power series
//! needs in it (`π`, `γ`, `ln`). A result of the 384-bit evaluation whose error interval
//! straddles a rounding boundary is evaluated again here (`special::jn`, `special::yn`); every
//! other result never touches this module, so the regimes that need no extra precision cost
//! nothing more than the rounding test.
//!
//! The constants come from `cargo xtask tables` (`MPW_*` in `coeffs/tables.rs`, 840 bits before
//! truncation). Each operation truncates to 768 bits, a relative error below 2^-767.
//!
//! The same kernels as [`crate::mp`] (`mp::mp_kernels`, expanded here) exist at this width for
//! the retry step of Ziv's strategy in the complex `_Float128` functions (MATH-M5-CPLX-F128-ZIV-A;
//! nothing calls them yet outside their tests). Their error bounds, proved here for a
//! significand of `B` bits (`B = 384` in `mp`, `768` here), with `ε = 2^(1−B)`:
//!
//! - Every operation computes its exact result into a wider buffer and truncates (`pack`) to `B`
//!   bits toward zero: an error below one unit of the result's last place, `2^(e_r − B)`. `mul` and
//!   `mul_u64` are exact before `pack` (relative error `< ε`). `div_u64` keeps one more limb of
//!   quotient, truncated (`< 2^(e_r−B−64)`), so `< 2ε`. `add` of like signs loses `small`'s bits
//!   below `2^(e_big−B)` in `shr` and then truncates: `< 3·2^(e_big−B)`; of unlike signs the
//!   subtraction of the shifted words is exact and fits, so the error is `shr`'s alone,
//!   `< 2^(e_big−B)` absolute (any cancellation keeps it absolute). Both: `< 2^(e_big+2−B)`.
//! - `recip`: the `f64` start has a relative error `δ₀ < 2^-51`; Newton's step gives
//!   `δ' < δ² + 4ε` (three operations): `2^-102, 2^-204, 2^-408, 2^-816 + 4ε`, so `< 2^(3−B)`
//!   after four steps for `B ≤ 800`. `div = mul(recip)`: `< 2^(4−B)`. `sqrt`: the reciprocal-root
//!   step gives `δ' ≤ 1.5·δ² + 6ε` from `δ₀ < 2^-50`, and the final product: `< 2^(5−B)`.
//! - `exp(x)`, `|x| < 2^40`, `k = trunc(x/ln 2)`: `ln 2`'s constant is truncated (`< ε`), so
//!   `r = x − k·ln 2` has an absolute error `< (|k| + 2)·2^(2−B)`, `|r| < 1`; the series of
//!   `e^(r/2^12)` has at most `B/12` terms, the n-th with a relative error `< 2n·ε`, each sum
//!   step `< 2ε`: `< 2^(7−B)` relative; twelve squarings multiply a relative error by `2^12` and
//!   add `< 2^12·ε`. In all: relative `< 2^(14−B)·(|k| + 64)`.
//! - `ln_atanh` (atanh series; `mpw::ln`, and `ln1p`/`ln10` at both widths): absolute
//!   `< 2^(max(e_ln, 0) + 12 − B)` (proved in its documentation). `mp::ln` (Halley, 384 bits)
//!   has no proven bound and no Ziv step uses it.
//!   `ln1p(t)`: below 2^-6, `u = t/(2 + t)` (`< 2^(5−B)`), at most `B/10` terms each `< 2^(6−B)`
//!   relative: relative `< 2^(9−B)`; above, `ln(1 + t)` with the addition's `2^(e+2−B)` and a
//!   result of at least `2^-7`: relative `< 2^(22 + max(e_t, 0) − B)`.
//! - `atan(t)`: each halving `u/(1 + √(1 + u²))` maps a relative error `δ` of `u` to at most `δ`
//!   (its logarithmic derivative is below 1) and adds `< 2^(7−B)`: eight halvings `< 2^(10−B)`;
//!   the alternating series `< 2^(6−B)`; `π/2 − a` with `a ≤ π/4` cancels less than one bit:
//!   relative `< 2^(12−B)`. `atan2` adds a division and `π − a` with `a ≤ π/2`: `< 2^(13−B)`.
//! - `sinh_cosh(x)`: below ½ the two series, relative `< 2^(7−B)`; above, `(e^x ∓ e^−x)/2` with the
//!   bounds of `exp` and `recip` and a cancellation factor at most `2.2` (at `x = ½`): relative
//!   `< 2^(17−B)·(|k| + 64)`.
//! - `sin_cos_small(r)`, `|r| ≤ 1`: at most `B/5` terms of magnitude below 1, each step `< 2^(4−B)`:
//!   absolute `< 2^(9−B)`, and relative `< 2^(9−B)` for the sine. `sin_cos(x)`, `|x| < 2^60`: the
//!   reduced argument's absolute error `< (|x| + 2)·2^(4−B)`, plus the series. `sin_cos_pi(r)`:
//!   `π·t` with `|t| ≤ ¼`: absolute `< 2^(10−B)`; exact (`true`) exactly at the multiples of ½.
//! - `reduce_wide(sig, e)`: the window holds `RW = LIMBS + 3` words of `2/π` from bit `j0 ≥ e − 1`;
//!   the bits after it weigh `< sig·2^(e − j0 − 64·RW + 1) < 2^(116 − 64·RW) = 2^(−B − 76)`
//!   quadrants. The kept fraction of `RW + 1` words is exact; its truncation to `B` bits and the
//!   product with `π/2` add `< 2^(3−B)` relative: `r`'s absolute error `< 2^(3−B)·|r| + 2^(−B−74)`,
//!   and `(sin r, cos r)` rotated by `k` then `< 2^(11−B)`. The table holds 272 words, enough for
//!   `e ≤ 16 383` at `RW = 15` (word 269).
//! - `exact_sqrt`, `mul_x`, `add_x`, `round_exact` are exact or say they are not: `exact_sqrt`
//!   returns a root only when its square equals `v` exactly; a product of `s_a + s_b ≤ B`
//!   significant bits, or a sum spanning `≤ B` bits with its carry, fits the significand.

#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::indexing_slicing, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap, clippy::cast_precision_loss)] // limb arithmetic of fixed width
#![allow(dead_code)] // the shared arithmetic of `mp_core` is wider than the Bessel series uses

use crate::coeffs::tables as t;
use crate::float::{self, Exact, Float};
use crate::status::{Rounding, Status};

/// Limbs of the significand.
pub const LIMBS: usize = 12;

crate::mp::mp_core!();
crate::mp::mp_kernels!();

impl Mp {
    /// The 768-bit value of a 384-bit one (exact).
    pub fn widen(v: crate::mp::Mp) -> Mp {
        let mut m = [0u64; LIMBS];
        m[LIMBS - crate::mp::LIMBS..].copy_from_slice(&v.m);
        Mp { neg: v.neg, exp: v.exp, m }
    }
}

/// `π`.
pub fn pi() -> Mp {
    c(&t::MPW_PI)
}

/// `ln 2`.
pub fn ln2() -> Mp {
    c(&t::MPW_LN2)
}

/// Euler's constant `γ`.
pub fn euler() -> Mp {
    c(&t::MPW_EULER)
}

/// `ln x` for `x > 0`: the shared `ln_atanh` kernel (`mp::mp_kernels`, where its bound is
/// proved): absolute error below `2^(max(e_ln, 0) + 12 − 768)` for a result below `2^e_ln`.
pub fn ln(x: Mp) -> Mp {
    ln_atanh(x)
}

#[cfg(test)]
#[allow(clippy::float_cmp, clippy::unwrap_used)]
mod tests {
    use super::*;
    use liella_mathtools::mp::{self as o, Big, Round};

    #[test]
    fn wide_kernels_agree_with_the_narrow_ones() {
        // no spec clause: unit check of the 768-bit kernels against the 384-bit ones.
        let n = |v: Mp| crate::mp::Mp { neg: v.neg, exp: v.exp, m: v.m[LIMBS - crate::mp::LIMBS..].try_into().unwrap_or([0; crate::mp::LIMBS]) };
        let close = |a: crate::mp::Mp, b: crate::mp::Mp| a.sub(b).is_zero() || a.sub(b).exp < a.exp - 360; // the 384-bit `exp`/`ln`: about 2^-370
        assert!(close(n(pi()), crate::mp::pi()));
        assert!(close(n(euler()), crate::mp::euler()));
        for v in [0.75, 1.5, 3.0, 100.0, 1e-30, 7.0e20] {
            let w = ln(Mp::from_f64(v));
            assert!(close(n(w), crate::mp::ln(crate::mp::Mp::from_f64(v))), "ln {v}");
        }
        // ln 2 itself, through the series at m = 1 and e = 1: exactly the constant.
        assert_eq!(ln(Mp::from_i64(2)), ln2());
        // 768 bits: (1 + 2^-700) − 1 survives here and not in 384 bits.
        let one = Mp::from_i64(1);
        assert_eq!(one.add(one.ldexp(-700)).sub(one), one.ldexp(-700));
    }

    /// Working precision of the oracle: far beyond the 768 bits checked.
    const P: u32 = 1200;
    /// Slack of the truncation check, in bits (0: an error below one unit in the last place).
    const TRUNC: i64 = 0;

    /// The exact value of an `Mp`.
    fn big(v: Mp) -> Big {
        let mut b = Big::zero(false);
        for (i, &l) in v.m.iter().enumerate() {
            b = b.add(&Big::from_u64(l).mul_pow2(v.exp - BITS + 64 * i as i64));
        }
        if v.neg { b.neg() } else { b }
    }

    /// `|v − x| < 2^k` exactly.
    fn below(v: Mp, x: &Big, k: i64) -> bool {
        big(v).sub(x).cmp_abs(&Big::one().mul_pow2(k)).is_lt()
    }

    /// `|v − x| < 2^(k − B)·|x|` (relative bound `2^(k−B)`).
    fn rel(v: Mp, x: &Big, k: i64) -> bool {
        big(v).sub(x).cmp_abs(&x.abs().mul_pow2(k - BITS)).is_lt()
    }

    /// A truncated result: below `x` in magnitude by less than one unit of its last place.
    fn truncated(v: Mp, x: &Big) -> bool {
        big(v).cmp_abs(x).is_le() && (x.is_zero() || below(v, x, v.exp - BITS - TRUNC))
    }

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
        /// A random value with a full significand and an exponent in `[lo, hi]`.
        fn mp(&mut self, lo: i64, hi: i64) -> Mp {
            let mut m = [0u64; LIMBS];
            for l in &mut m {
                *l = self.next();
            }
            m[LIMBS - 1] |= 1 << 63;
            Mp { neg: self.next() & 1 == 1, exp: lo + (self.next() % (hi - lo + 1) as u64) as i64, m }
        }
    }

    /// Values whose additions carry through every limb or cancel to the last bit.
    fn edges() -> [Mp; 4] {
        let ones = Mp { neg: false, exp: 0, m: [u64::MAX; LIMBS] }; // 1 − 2^-768
        let mut low = [0u64; LIMBS];
        low[0] = 1;
        low[LIMBS - 1] = 1 << 63;
        [ones, Mp { neg: false, exp: 1, m: low }, ones.ldexp(-1).neg(), Mp::from_i64(1)]
    }

    #[test]
    fn wide_arithmetic_matches_the_exact_model() {
        // no spec clause: 768-bit add, sub, mul, mul_u64, div_u64 against exact rational values.
        let mut g = Rng(0x9e37_79b9_7f4a_7c15);
        let mut pairs: Vec<(Mp, Mp)> = (0..400).map(|_| (g.mp(-20, 20), g.mp(-20, 20))).collect();
        for a in edges() {
            for b in edges() {
                pairs.push((a, b));
                pairs.push((a, b.ldexp(-767)));
            }
        }
        for (a, b) in pairs {
            let (x, y) = (big(a), big(b));
            let p = x.mul(&y);
            assert!(truncated(a.mul(b), &p), "mul {a:?} {b:?}");
            let s = x.add(&y);
            let e = a.exp.max(b.exp);
            assert!(s.is_zero() && a.add(b).is_zero() || below(a.add(b), &s, e + 2 - BITS), "add {a:?} {b:?}");
            assert!(below(a.sub(b), &x.sub(&y), e + 2 - BITS), "sub {a:?} {b:?}");
            let k = b.m[LIMBS - 1] | 1;
            assert!(truncated(a.mul_u64(k), &x.mul(&Big::from_u64(k))), "mul_u64 {a:?} {k}");
            let q = a.div_u64(k);
            let qx = x.div(&Big::from_u64(k), P, Round::Zero);
            assert!(big(q).cmp_abs(&qx).is_le() && rel(q, &qx, 2), "div_u64 {a:?} {k}");
        }
        // The carry runs through all twelve limbs: (1 − 2^-768) + 2^-768 = 1.
        let [ones, ..] = edges();
        assert_eq!(ones.add(Mp::from_i64(1).ldexp(-768)), Mp::from_i64(1));
        // The borrow runs through all twelve limbs: 1 − (1 − 2^-767) = 2^-767, exact (the
        // subtrahend loses no bit when aligned to the exponent of 1).
        let mut m = [u64::MAX; LIMBS];
        m[0] = u64::MAX - 1;
        let near_one = Mp { neg: false, exp: 0, m }; // 1 − 2^-767
        assert_eq!(Mp::from_i64(1).sub(near_one), Mp::from_i64(1).ldexp(-767));
        // 1 − (1 − 2^-768): the alignment drops the last bit of `ones`, so the result is 2^-767,
        // within the proved bound 2^(e+2−B) and not exact.
        let d = Mp::from_i64(1).sub(ones);
        assert!(below(d, &Big::one().mul_pow2(-768), 1 + 2 - BITS), "1 - ones = {d:?}");
    }

    #[test]
    fn wide_kernels_within_their_proven_bounds() {
        // no spec clause: each 768-bit kernel against the oracle (1200 bits) with the bound proved
        // in the module documentation.
        let mut g = Rng(0x2545_f491_4f6c_dd1d);
        let mut xs: Vec<Mp> = (0..48).map(|_| g.mp(-12, 3)).collect();
        xs.extend([0.5, 1.0, -1.0, 0.75, 1e-30, -9.5, 13.0, 2.0f64.powi(-60)].map(Mp::from_f64));
        xs.extend(edges());
        let pi = o::pi(P);
        for &x in &xs {
            let bx = big(x);
            let ax = x.abs();
            assert!(rel(x.recip(), &Big::one().div(&bx, P, Round::Nearest), 3), "recip {x:?}");
            assert!(rel(ax.sqrt(), &bx.abs().sqrt(P, Round::Nearest), 5), "sqrt {x:?}");
            let k = (x.to_f64() / core::f64::consts::LN_2).abs() as i64 + 1;
            let kb = 14 + (64 + k).ilog2() as i64 + 1;
            assert!(rel(exp(x), &o::exp(&bx, P).value, kb), "exp {x:?}");
            let l = o::log(&bx.abs(), P).value;
            let el = l.to_f64().abs().log2().ceil().max(0.0) as i64;
            assert!(below(ln(ax), &l, el + 12 - BITS), "ln {x:?}");
            let ex = ax.exp.max(0);
            assert!(rel(ln1p(ax), &o::log1p(&bx.abs(), P).value, 22 + ex), "ln1p {x:?}");
            assert!(rel(atan(ax), &o::atan(&bx.abs(), P).value, 12), "atan {x:?}");
            let y = Mp::from_f64(-0.625);
            assert!(rel(atan2(y, x), &o::atan2(&big(y), &bx, P).value, 13), "atan2 {x:?}");
            assert!(rel(atan2(x, y), &o::atan2(&bx, &big(y), P).value, 13), "atan2' {x:?}");
            let (s, c) = sinh_cosh(x);
            assert!(rel(s, &o::sinh(&bx, P).value, 17 + kb - 14) && rel(c, &o::cosh(&bx, P).value, 17 + kb - 14), "sinh_cosh {x:?}");
            if x.exp <= 1 {
                let r = if x.exp == 1 { x.ldexp(-1) } else { x };
                let (s, c) = sin_cos_small(r);
                let br = big(r);
                assert!(rel(s, &o::sin(&br, P).value, 9) && below(c, &o::cos(&br, P).value, 9 - BITS), "sin_cos_small {r:?}");
            }
            let (s, c) = sin_cos(x);
            let ab = 4 + (x.to_f64().abs() + 2.0).log2().ceil() as i64;
            let ab = ab.max(9) + 1;
            assert!(below(s, &o::sin(&bx, P).value, ab - BITS) && below(c, &o::cos(&bx, P).value, ab - BITS), "sin_cos {x:?}");
            let t = x.frac();
            let ((s, c), exact) = sin_cos_pi(t);
            let pt = pi.mul(&big(t));
            assert!(exact == t.ldexp(1).frac().is_zero() && below(s, &o::sin(&pt, P).value, 10 - BITS) && below(c, &o::cos(&pt, P).value, 10 - BITS), "sin_cos_pi {t:?}");
        }
        assert!(below(ln10(), &o::ln10(P), 14 - BITS));
        for q in -2..=2 {
            let ((s, c), exact) = sin_cos_pi(Mp::from_i64(q).ldexp(-1));
            assert!(exact && s.sig_bits() <= 1 && c.sig_bits() <= 1, "sin_cos_pi exact {q}");
        }
        // Payne–Hanek: binary128 operands up to the largest exponent (the table's last words).
        let sigs = [0x1_2345_6789_abcd_ef01_2345_6789_abcdu128, 0x1_ffff_ffff_ffff_ffff_ffff_ffff_ffffu128, (1u128 << 112) | 1];
        for (i, &sig) in sigs.iter().enumerate() {
            for e in [30i64, 200, 1000, 4000, 16_271] {
                if e > 1000 && i > 0 {
                    continue; // the oracle's reduction of the largest values is slow: one operand each
                }
                let x = Big::from_u128(sig).mul_pow2(e);
                let (k, r) = reduce_wide(sig, e);
                // k·π/2 + r ≡ x (mod 2π): compare (sin, cos) of r rotated by k with the oracle's.
                let (s, c) = sin_cos_small(r);
                let (s, c) = match k.rem_euclid(4) { 0 => (s, c), 1 => (c, s.neg()), 2 => (s.neg(), c.neg()), _ => (c.neg(), s) };
                let (es, ec) = (o::sin(&x, P).value, o::cos(&x, P).value);
                assert!(below(s, &es, 11 - BITS) && below(c, &ec, 11 - BITS), "reduce_wide {sig:#x} {e}");
                let (ws, wc) = sin_cos_wide(true, sig, e);
                assert!(below(ws, &es.neg(), 11 - BITS) && below(wc, &ec, 11 - BITS), "sin_cos_wide {sig:#x} {e}");
            }
        }
    }

    #[test]
    fn wide_exactness_tests() {
        // no spec clause: the exact cases are detected exactly at 768 bits.
        let v = Mp::from_u128(false, (1u128 << 112) + 3, -40);
        assert_eq!(exact_sqrt(v.sqr()), Some(v));
        assert_eq!(exact_sqrt(Mp::from_i64(2)), None);
        let a = Mp::from_u128(false, u128::MAX, 0);
        let (p, ex) = mul_x(a, a);
        assert!(ex && truncated(p, &big(a).mul(&big(a))) && big(p).cmp_abs(&big(a).mul(&big(a))).is_eq());
        let w = Mp { neg: false, exp: 0, m: [u64::MAX; LIMBS] };
        assert!(!mul_x(w, w).1);
        assert!(add_x(Mp::from_i64(1), Mp::from_i64(1).ldexp(-700)).1);
        assert!(!add_x(Mp::from_i64(1), Mp::from_i64(1).ldexp(-770)).1);
        // round_exact: the low limbs are the sticky bit (here clear: 1 + 2^-112 is a binary128).
        let one_ulp = Mp::from_i64(1).add(Mp::from_i64(1).ldexp(-112));
        let (r, _) = round_exact::<f64>(one_ulp, Rounding::Up);
        assert_eq!(r, 1.0 + f64::EPSILON);
        let (r, _) = round_exact::<f64>(Mp::from_i64(1).add(Mp::from_i64(1).ldexp(-760)), Rounding::Up);
        assert_eq!(r, 1.0 + f64::EPSILON);
    }
}
