//! pi and ln 2 checked against independent second methods.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]
#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::indexing_slicing)]

mod common;

use common::rel_log2;
use liella_mathtools::mp::{ln10, ln2, log, pi, Big, Fmt, Round};

/// pi by the Gauss-Legendre (Brent-Salamin) AGM iteration, at `p` bits per operation.
fn pi_agm(p: u32) -> Big {
    let r = |x: Big| x.round(p, Round::Nearest);
    let mut a = Big::one();
    let mut b = Big::one().div(&Big::from_i64(2).sqrt(p, Round::Nearest), p, Round::Nearest);
    let mut t = Big::pow2(-2);
    let mut k = Big::one();
    for _ in 0..16 {
        let an = a.add(&b).mul_pow2(-1);
        let bn = a.mul(&b).sqrt(p, Round::Nearest);
        let d = a.sub(&an);
        t = r(t.sub(&k.mul(&r(d.mul(&d)))));
        k = k.mul_pow2(1);
        a = an;
        b = bn;
    }
    let s = a.add(&b);
    r(s.mul(&s)).div(&t.mul_pow2(2), p, Round::Nearest)
}

/// ln 2 by `sum_{k>=1} 1 / (k 2^k)` (a different series from the library's `2 atanh(1/3)`).
fn ln2_series(p: u32) -> Big {
    let mut s = Big::zero(false);
    for k in 1..=(i64::from(p) + 16) {
        s = s.add(&Big::one().div(&Big::from_i64(k), p, Round::Nearest).mul_pow2(-k)).round(p + 16, Round::Nearest);
    }
    s
}

#[test]
fn pi_two_methods() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    let agm = pi_agm(1100);
    let p = pi(1000);
    assert!(rel_log2(&p, &agm) <= -999.0, "{}", rel_log2(&p, &agm));
    assert!(p.identical(&agm.round(1000, Round::Nearest)));
    // The library's own Machin value at a much higher precision agrees.
    assert!(pi(20000).round(1000, Round::Nearest).identical(&p));
    assert_eq!(pi(53).to_f64(), std::f64::consts::PI);
    assert_eq!(pi(24).round_to(Fmt::F32, Round::Nearest).0 as u32, std::f32::consts::PI.to_bits());
    assert_eq!(pi(8).to_hex(), "0x1.92p+1");
}

#[test]
fn ln2_two_methods() {
    // no spec clause: test tooling of the math oracle (D-10.12).
    let s = ln2_series(1100);
    let l = ln2(1000);
    assert!(rel_log2(&l, &s) <= -999.0, "{}", rel_log2(&l, &s));
    assert!(l.identical(&s.round(1000, Round::Nearest)));
    assert_eq!(ln2(53).to_f64(), std::f64::consts::LN_2);
    assert_eq!(ln10(53).to_f64(), std::f64::consts::LN_10);
    // ln 10 = log(10) from the general logarithm, at high precision.
    let l10 = log(&Big::from_i64(10), 1000).value;
    assert!(rel_log2(&l10, &ln10(1100)) <= -999.0);
}
