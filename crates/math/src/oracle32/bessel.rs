//! The tiered oracle's double-double estimate of J0, J1, Y0, Y1 (LIBM-F32-BESSEL, ruling #2889),
//! on the fast path's range [2^-20, 2^19) (any other operand is [`Est::Unknown`]: the fast path
//! does not take it). Every bound is absolute and first order in `U`, with the slack noted.
//!
//! Below 24, the power series (DLMF 10.2.2 and 10.8.1): `t_k = (−1)^k (x/2)^(2k+n)/(k!(k+n)!)`,
//! `J_n = Σ t_k`, `Y_n = (2/π)·[(ln(x/2) + γ)·J_n − [n = 1]/x − ½·Σ (H_k + H_(k+n))·t_k]`. From
//! 24, Hankel's form (DLMF 10.17.3 and 10.17.4) `J_n = A·(P cos ω − Q sin ω)`,
//! `Y_n = A·(P sin ω + Q cos ω)`, `A = √(2/(πx))`, `ω = x − π/4 − nπ/2`, whose remainders after
//! any number of terms are bounded by the first neglected terms (DLMF 10.17(iii), ν = 0, 1).

// Test-only: term counters below 2^8 (exact as f64), quadrant arithmetic on |k| < 2^20.
#![allow(clippy::as_conversions, clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::neg_cmp_op_on_partial_ord)]

use super::{Est, U};
use crate::dd::{two_prod, Dd};
use crate::mp::{self, Mp};
use std::sync::OnceLock;

/// Where the series hands over to Hankel's form.
const SWITCH: f64 = 24.0;
/// The fast path's range (`fast32::bessel`'s `BES_MIN`, `BES_MAX`).
const LO: f64 = 9.536_743_164_062_5e-7; // 2^-20
const HI: f64 = 524_288.0; // 2^19
/// A series stops at the first term below this fraction of the terms' magnitude sum.
const TINY: f64 = 7.703_719_777_548_943e-34; // 2^-110
/// Relative bound of `exp_log::log_dd` with an absolute floor: |error| ≤ UL·(|ln| + 1)
/// (`bessel_oracle_log_bound` measures it below 2^-102 on its sample).
pub(crate) const UL: f64 = 3.155_443_620_884_047e-30; // 2^-98
/// A factor covering the `f64` roundings of the magnitude sums that the bounds use.
const SLACK: f64 = 1.001;

/// 2/π and γ as double-doubles (each within 2^-106 relative), and π/4 as three `f64` parts
/// (`π/4 − (p1 + p2 + p3)` below 2^-160), from the 384-bit constants.
struct Consts {
    two_over_pi: Dd,
    gamma: Dd,
    quarter_pi: [f64; 3],
}

fn mp_dd(m: Mp) -> Dd {
    let hi = m.to_f64();
    Dd { hi, lo: m.sub(Mp::from_f64(hi)).to_f64() }
}

fn consts() -> &'static Consts {
    static C: OnceLock<Consts> = OnceLock::new();
    C.get_or_init(|| {
        let pi = mp::pi();
        let q = pi.ldexp(-2);
        let p1 = q.to_f64();
        let r1 = q.sub(Mp::from_f64(p1));
        let p2 = r1.to_f64();
        let p3 = r1.sub(Mp::from_f64(p2)).to_f64();
        Consts { two_over_pi: mp_dd(pi.recip().ldexp(1)), gamma: mp_dd(mp::euler()), quarter_pi: [p1, p2, p3] }
    })
}

/// The oracle's estimate of `j0f` (`f` = 0), `j1f` (1), `y0f` (2) or `y1f` (3) at `x`.
pub(crate) fn bessel_est(f: usize, x: f32) -> Est {
    let a = f64::from(x).abs();
    let (n, want_y) = ((f & 1) as u32, f >= 2);
    if !(a >= LO && a < HI) || (want_y && x < 0.0) {
        return Est::Unknown;
    }
    let (v, err) = if a < SWITCH { series(n, a, want_y) } else { hankel(n, a, want_y) };
    // J1 is odd, J0 even; Y only for x > 0.
    Est::Near { v: if n == 1 && x < 0.0 { v.neg() } else { v }, err }
}

/// The power series below [`SWITCH`].
///
/// J: each step `t_k = t_(k−1)·(−x²/4)/(k(k+n))` (x²/4 exact: x has 24 bits) adds at most 2U,
/// so `t_k` is within 2kU; K + 1 additions add at most U·Σ|t| each: (3K + 1)·U·Σ|t|, taken as
/// 8(K + 2)·U·Σ|t|. The loop stops at the first term K with `K(K + n) > x²/2` (from there each
/// ratio is below ½: an alternating, decreasing tail below `|t_K|`) that is also below
/// `TINY·Σ|t|`. Y: `H_k` is within 2kU (positive terms), `w_k = H_k + H_(k+n)` within
/// (2k + 1)U, `w_k t_k` within (4k + 2)U: (5K + 3)·U·Σ|w t| for the weighted sum, again taken as
/// 8(K + 2)U; `w_(k+1)/w_k ≤ 2` for k ≥ 1, so that tail too is alternating and below
/// `|w_K t_K|`. ln(x/2) is within `UL·(|ln| + 1)` and γ, 2/π within U relative; the products and
/// sums of the bracket and the factor 2/π add 8U of its magnitude sum `M`.
fn series(n: u32, x: f64, want_y: bool) -> (Dd, f64) {
    let q = x * x * 0.25;
    let nf = f64::from(n);
    let mut t = Dd::from(if n == 0 { 1.0 } else { x * 0.5 });
    let mut j = t;
    let mut sa = t.hi.abs();
    // H_k and H_(k+n) at k = 0; w_0 t_0 = (H_0 + H_n)·t_0.
    let (mut hk, mut hkn) = (Dd::from(0.0), Dd::from(nf));
    let mut ws = if n == 1 { t } else { Dd::from(0.0) };
    let mut wa = ws.hi.abs();
    let mut k = 0u32;
    let (tail_j, tail_w) = loop {
        k += 1;
        let kf = f64::from(k);
        t = t.mul_f64(-q).div(Dd::from(kf * (kf + nf)));
        hk = hk.add(Dd::from(1.0).div(Dd::from(kf)));
        hkn = if n == 0 { hk } else { hkn.add(Dd::from(1.0).div(Dd::from(kf + nf))) };
        let wt = hk.add(hkn).mul(t);
        if kf * (kf + nf) > 2.0 * q && t.hi.abs() < TINY * sa {
            break (t.hi.abs(), wt.hi.abs());
        }
        j = j.add(t);
        sa += t.hi.abs();
        if want_y {
            ws = ws.add(wt);
            wa += wt.hi.abs();
        }
    };
    let kk = f64::from(k);
    let err_j = SLACK * (8.0 * (kk + 2.0) * U * sa + tail_j);
    if !want_y {
        return (j, err_j);
    }
    let c = consts();
    let half = x * 0.5;
    let ln = crate::exp_log::log_dd(Dd::from(half));
    let l = ln.add(c.gamma);
    let la = l.hi.abs();
    let err_l = UL * (ln.hi.abs() + 1.0) + U * (c.gamma.hi + la);
    let inv_x = if n == 1 { Dd::from(1.0).div(Dd::from(x)) } else { Dd::from(0.0) };
    let b = l.mul(j).sub(inv_x).sub(ws.scale(0.5));
    let m = la * sa + inv_x.hi + 0.5 * wa;
    let err_b = la * err_j + err_l * sa + 0.5 * (8.0 * (kk + 2.0) * U * wa + tail_w) + 8.0 * U * m;
    let v = c.two_over_pi.mul(b);
    (v, SLACK * 0.636_62 * (err_b + 8.0 * U * m))
}

/// Hankel's form from [`SWITCH`].
///
/// `b_k = a_k(n)/x^k = b_(k−1)·(4n² − (2k − 1)²)/(8kx)` (the factor exact, 8kx exact) is within
/// 2kU; P and Q, with K + 1 additions, within 8(K + 2)·U·Σ|b|; the remainders below
/// `|b_K| + |b_(K+1)|` (one first neglected term of each, DLMF 10.17(iii)). The loop stops at
/// the first term below 2^-112 or above its predecessor (or at 200 terms).
///
/// `ω = r + (k − n)·π/2` with `r = x − (2k + 1)·π/4`: `(2k + 1)·p1` and `(2k + 1)·p2` exact by
/// `two_prod`, `x − hi(that)` exact (Sterbenz: x ≥ 24 and |r| < 0.8), `(2k + 1)·p3` and π/4's
/// own truncation below 2^-138 together, three double-double subtractions of terms below 1 at
/// most 3U: r within 4U. sin r and cos r by their Taylor series to r^31 and r^30 (truncation
/// below 2^-115 for |r| < 0.8), each term within (3j + 2)U and the sums within 16U·Σ|term|
/// (Σ|term| < cosh 0.8 < 1.34): sin ω and cos ω within 128U. A = √(2/(πx)) within 4U, the last
/// products and the sum another 4U of A·(|P| + |Q|).
fn hankel(n: u32, x: f64, want_y: bool) -> (Dd, f64) {
    let mu = f64::from(4 * n * n);
    let mut b = Dd::from(1.0);
    let (mut p, mut qs) = (b, Dd::from(0.0));
    let mut ba = 1.0;
    let mut k = 0u32;
    let tail = loop {
        k += 1;
        let kf = f64::from(k);
        let o = 2.0 * kf - 1.0;
        let prev = b.hi.abs();
        b = b.mul_f64(mu - o * o).div(Dd::from(8.0 * kf * x));
        let a = b.hi.abs();
        if a < 1.925_929_944_387_236e-34 || a > prev || k >= 200 {
            // 2^-112. The other series' first neglected term is b_(K+1).
            let o1 = 2.0 * kf + 1.0;
            let next = b.mul_f64(mu - o1 * o1).div(Dd::from(8.0 * (kf + 1.0) * x));
            break a + next.hi.abs();
        }
        // Even k: P's term with sign (−1)^(k/2); odd k: Q's with sign (−1)^((k−1)/2).
        let term = if (k / 2) % 2 == 0 { b } else { b.neg() };
        if k % 2 == 0 {
            p = p.add(term);
        } else {
            qs = qs.add(term);
        }
        ba += a;
    };
    let c = consts();
    let [p1, p2, p3] = c.quarter_pi;
    // k = round(2x/π − ½); the estimate's error only moves |r| by far less than 0.01.
    let kq = (x * c.two_over_pi.hi - 0.5).round();
    let odd = 2.0 * kq + 1.0;
    let h = two_prod(odd, p1);
    let g = two_prod(odd, p2);
    let r = Dd::from(x - h.hi).sub(Dd::from(h.lo)).sub(g).sub(Dd::from(odd * p3));
    let r2 = r.sqr();
    let (mut s, mut cs) = (r, Dd::from(1.0));
    let (mut ts, mut tc) = (r, Dd::from(1.0));
    for i in 1..=15u32 {
        let jf = f64::from(2 * i);
        ts = ts.mul(r2).div(Dd::from(-jf * (jf + 1.0)));
        tc = tc.mul(r2).div(Dd::from(-(jf - 1.0) * jf));
        s = s.add(ts);
        cs = cs.add(tc);
    }
    let (cos_w, sin_w) = match (kq as i64 - i64::from(n)).rem_euclid(4) {
        0 => (cs, s),
        1 => (s.neg(), cs),
        2 => (cs.neg(), s.neg()),
        _ => (s, cs.neg()),
    };
    let amp = crate::dd::sqrt(c.two_over_pi.div(Dd::from(x)));
    let v = if want_y { p.mul(sin_w).add(qs.mul(cos_w)) } else { p.mul(cos_w).sub(qs.mul(sin_w)) };
    let kk = f64::from(k);
    let err = SLACK * amp.hi * ((128.0 * U + 8.0 * (kk + 3.0) * U) * ba + tail);
    (amp.mul(v), err)
}
