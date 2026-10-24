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

#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::indexing_slicing, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap, clippy::cast_precision_loss)] // limb arithmetic of fixed width
#![allow(dead_code)] // the shared arithmetic of `mp_core` is wider than the Bessel series uses

use crate::coeffs::tables as t;
use crate::float::{self, Exact, Float};
use crate::status::{Rounding, Status};

/// Limbs of the significand.
pub const LIMBS: usize = 12;

crate::mp::mp_core!();

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

/// `ln x` for `x > 0`: `x = 2^e·m` with `m` in `[√½, √2)`, `ln m = 2·atanh(s)` with
/// `s = (m − 1)/(m + 1)`, `|s| < 0.172`, summed until the terms fall below 2^-776 of the sum
/// (at most about 160 terms), plus `e·ln 2`. Absolute error below `2^(max(e_ln, 0) + 12 − 768)`
/// for a result below `2^e_ln`.
pub fn ln(x: Mp) -> Mp {
    let mut e = x.exp;
    let mut m = Mp { neg: false, exp: 0, m: x.m }; // in [1/2, 1)
    if m.to_f64() < core::f64::consts::FRAC_1_SQRT_2 {
        m = m.ldexp(1);
        e -= 1;
    }
    let one = Mp::from_i64(1);
    let s = m.sub(one).div(m.add(one));
    let s2 = s.sqr();
    let (mut sum, mut p, mut k) = (s, s, 1u64);
    while !s.is_zero() {
        p = p.mul(s2);
        let term = p.div_u64(2 * k + 1);
        if term.is_zero() || term.exp < sum.exp - BITS - 8 {
            break;
        }
        sum = sum.add(term);
        k += 1;
    }
    sum.ldexp(1).add(ln2().mul(Mp::from_i64(e)))
}

#[cfg(test)]
#[allow(clippy::float_cmp)]
mod tests {
    use super::*;

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
}
