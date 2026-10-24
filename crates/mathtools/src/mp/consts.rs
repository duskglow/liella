//! Constants computed at run time and cached at the largest precision requested so far.
//!
//! * pi by Machin's formula `pi = 16 atan(1/5) - 4 atan(1/239)`;
//! * ln 2 by `ln 2 = 2 atanh(1/3) = sum 2 / ((2k+1) 3^(2k+1))`;
//! * ln 10 and 2/pi from those with the general routines.
//!
//! The series are summed in fixed point with `W` fraction bits. Every term is a truncated
//! quotient, so its error is below 2.1 units (the running power carries at most 1.1 units of
//! error, divided again by `2k+1`, plus one unit of truncation); with `T < W` terms, the
//! total error is below `50 W` units, so `W = bits + 40` leaves the result within
//! `2^-bits` relative for any `bits < 2^30`.

use std::sync::Mutex;

use super::nat::Nat;
use super::{Big, Round};

type Cache = Mutex<Option<(u32, Big)>>;

static PI: Cache = Mutex::new(None);
static LN2: Cache = Mutex::new(None);
static LN10: Cache = Mutex::new(None);
static TWO_OVER_PI: Cache = Mutex::new(None);

/// A value of the constant with relative error below `2^-w`, truncated to `w + 2` bits.
fn cached(cache: &Cache, w: u32, compute: fn(u32) -> Big) -> Big {
    let need = w.saturating_add(4);
    {
        let g = match cache.lock() {
            Ok(g) => g,
            Err(p) => p.into_inner(),
        };
        if let Some((bits, v)) = g.as_ref() {
            if *bits >= need {
                // Cached error < 2^-(w+4); truncation adds < 2^-(w+1).
                return v.round(w.saturating_add(2), Round::Zero);
            }
        }
    }
    // Grow geometrically so a slowly rising demand costs O(1) recomputations.
    let old = match cache.lock() {
        Ok(g) => g.as_ref().map_or(0, |c| c.0),
        Err(p) => p.into_inner().as_ref().map_or(0, |c| c.0),
    };
    let bits = need.max(old.saturating_mul(2)).max(256);
    let v = compute(bits);
    let r = v.round(w.saturating_add(2), Round::Zero);
    let mut g = match cache.lock() {
        Ok(g) => g,
        Err(p) => p.into_inner(),
    };
    if g.as_ref().is_none_or(|c| c.0 < bits) {
        *g = Some((bits, v));
    }
    r
}

/// `sum_k (-1)^k 2^W / ((2k+1) n^(2k+1))` (or without the signs), in fixed point.
fn atan_inv(n: u64, wbits: u64, alternate: bool) -> (Nat, Nat) {
    let n2 = n * n;
    let mut p = Nat::from_u64(1).shl(wbits).divrem_small(n).0;
    let (mut pos, mut neg) = (Nat::zero(), Nat::zero());
    let mut k: u64 = 0;
    while !p.is_zero() {
        let t = p.divrem_small(2 * k + 1).0;
        if alternate && k % 2 == 1 {
            neg = neg.add(&t);
        } else {
            pos = pos.add(&t);
        }
        p = p.divrem_small(n2).0;
        k += 1;
    }
    (pos, neg)
}

fn compute_pi(bits: u32) -> Big {
    let w = u64::from(bits) + 40;
    let (p5, n5) = atan_inv(5, w, true);
    let (p239, n239) = atan_inv(239, w, true);
    // 16 (p5 - n5) - 4 (p239 - n239), all as one non-negative combination.
    let plus = p5.mul_small(16).add(&n239.mul_small(4));
    let minus = n5.mul_small(16).add(&p239.mul_small(4));
    Big::fin(false, plus.sub(&minus), -(w as i64))
}

fn compute_ln2(bits: u32) -> Big {
    let w = u64::from(bits) + 40;
    let (s, _) = atan_inv(3, w, false);
    Big::fin(false, s.shl(1), -(w as i64))
}

fn compute_ln10(bits: u32) -> Big {
    super::func::log_core(&Big::from_i64(10), bits.saturating_add(8))
}

fn compute_two_over_pi(bits: u32) -> Big {
    Big::from_i64(2).div(&pi_w(bits.saturating_add(4)), bits.saturating_add(4), Round::Zero)
}

/// pi with relative error below `2^-w`.
pub(crate) fn pi_w(w: u32) -> Big {
    cached(&PI, w, compute_pi)
}

/// ln 2 with relative error below `2^-w`.
pub(crate) fn ln2_w(w: u32) -> Big {
    cached(&LN2, w, compute_ln2)
}

/// ln 10 with relative error below `2^-w`.
pub(crate) fn ln10_w(w: u32) -> Big {
    cached(&LN10, w, compute_ln10)
}

/// 2/pi with relative error below `2^-w`.
pub(crate) fn two_over_pi_w(w: u32) -> Big {
    cached(&TWO_OVER_PI, w, compute_two_over_pi)
}

/// Rounds a constant correctly: `f(w)` must have relative error below `2^-w`. Ziv's loop; it
/// ends because the constants are irrational.
fn round_const(prec: u32, f: fn(u32) -> Big) -> Big {
    let mut w = prec.saturating_add(32);
    loop {
        let v = f(w);
        let d = v.mul_pow2(-i64::from(w));
        let lo = v.sub(&d).round(prec, Round::Nearest);
        let hi = v.add(&d).round(prec, Round::Nearest);
        if lo.identical(&hi) || w >= prec.saturating_mul(8).saturating_add(4096) {
            return hi;
        }
        w = w.saturating_mul(2);
    }
}

/// pi correctly rounded to nearest at `prec` bits.
pub fn pi(prec: u32) -> Big {
    round_const(prec, pi_w)
}

/// ln 2 correctly rounded to nearest at `prec` bits.
pub fn ln2(prec: u32) -> Big {
    round_const(prec, ln2_w)
}

/// ln 10 correctly rounded to nearest at `prec` bits.
pub fn ln10(prec: u32) -> Big {
    round_const(prec, ln10_w)
}
