//! The tiered oracle's double-double estimate of J0, J1, Y0, Y1 (LIBM-F32-BESSEL, ruling #2889),
//! on every finite operand from 2^-149 to `FLT_MAX` (Y: x > 0; anything else is [`Est::Unknown`]):
//! the fast path's whole range, the tiny band (D-10.106) and the large band from 2^19 (D-10.109)
//! included. Every bound is absolute and first order in `U`, with the slack noted.
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
/// The smallest operand estimated (the series serves the tiny band too, D-10.106), and where the
/// phase reduction turns to fixed point (`large_reduce`, the fast path's large band, D-10.109).
const LO: f64 = 1.401_298_464_324_817e-45; // 2^-149
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
    if !(LO..=f64::from(f32::MAX)).contains(&a) || (want_y && x < 0.0) {
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
    // |2/π| as the nearest f64: its rounding (below 2^-53 relative) is within SLACK.
    (v, SLACK * core::f64::consts::FRAC_2_PI * (err_b + 8.0 * U * m))
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
    let (kq, r, cut) = if x >= HI {
        large_reduce(x)
    } else {
        // k = round(2x/π − ½); the estimate's error only moves |r| by far less than 0.01.
        let kq = (x * c.two_over_pi.hi - 0.5).round();
        let odd = 2.0 * kq + 1.0;
        let h = two_prod(odd, p1);
        let g = two_prod(odd, p2);
        (kq.rem_euclid(4.0) as i64, Dd::from(x - h.hi).sub(Dd::from(h.lo)).sub(g).sub(Dd::from(odd * p3)), 0.0)
    };
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
    let (cos_w, sin_w) = match (kq - i64::from(n)).rem_euclid(4) {
        0 => (cs, s),
        1 => (s.neg(), cs),
        2 => (cs.neg(), s.neg()),
        _ => (s, cs.neg()),
    };
    let amp = crate::dd::sqrt(c.two_over_pi.div(Dd::from(x)));
    let v = if want_y { p.mul(sin_w).add(qs.mul(cos_w)) } else { p.mul(cos_w).sub(qs.mul(sin_w)) };
    let kk = f64::from(k);
    let err = SLACK * amp.hi * ((128.0 * U + 8.0 * (kk + 3.0) * U) * ba + tail + cut * (ba + 1.0));
    (amp.mul(v), err)
}

/// The bits of 2/π after the point, 320 of them, most significant first, from the 384-bit
/// constant (independent of the fast path's `coeffs::trig::TWO_OVER_PI`).
fn two_over_pi_words() -> &'static [u64; 5] {
    static W: OnceLock<[u64; 5]> = OnceLock::new();
    W.get_or_init(|| {
        let mut v = mp::pi().recip().ldexp(1);
        let mut w = [0u64; 5];
        for word in &mut w {
            for _ in 0..2 {
                v = v.ldexp(32);
                let mut d = v.to_f64().floor();
                let mut rem = v.sub(Mp::from_f64(d));
                if rem.to_f64() < 0.0 {
                    d -= 1.0;
                    rem = rem.add(Mp::from_f64(1.0));
                }
                *word = (*word << 32) | d as u64;
                v = rem;
            }
        }
        w
    })
}

/// From 2^19 (LIBM-F32-BESSEL-LARGE, D-10.109): `x·2/π − ½ = k + r·2/π` in fixed point (Payne and
/// Hanek): x = m·2^(e − 23), m < 2^24, times the 128 bits of 2/π ending at bit e + 103 (the bits
/// before them add multiples of 4), a wrapping product in units of 2^-126 within m·2^-126 < 2^-102.
/// Returns k mod 4, r = g·π/2 with g ∈ [−½, ½) (g to 106 bits, π/2 within 2^-160, two Dd
/// roundings: within 4U of r) and the absolute bound on r of the 2/π cut, 2^-101.
fn large_reduce(x: f64) -> (i64, Dd, f64) {
    let a = (x as f32).to_bits();
    let m = u128::from((a & 0x007f_ffff) | 0x0080_0000);
    let p = (a >> 23) as usize - 24;
    let t = two_over_pi_words();
    let w = if p <= 128 {
        ((u128::from(t[0]) << 64) | u128::from(t[1])) >> (128 - p)
    } else {
        let (j, s) = (p / 64 - 2, (p % 64) as u32);
        (u128::from(t[j]) << (64 + s)) | (u128::from(t[j + 1]) << s) | u128::from((t[j + 2] >> 1) >> (63 - s))
    };
    // − ½ for the phase, + ½ to round: they cancel.
    let v = m.wrapping_mul(w);
    let g = ((v & ((1 << 126) - 1)) as i128) - (1 << 125);
    let hi = g as f64;
    let gd = Dd { hi, lo: (g - hi as i128) as f64 };
    let [p1, p2, _] = consts().quarter_pi;
    let half_pi = Dd { hi: 2.0 * p1, lo: 2.0 * p2 };
    let r = Dd { hi: gd.hi * TWO_M126, lo: gd.lo * TWO_M126 }.mul(half_pi);
    ((v >> 126) as i64, r, 4.0e-31)
}

const TWO_M126: f64 = 1.175_494_350_822_287_5e-38; // 2^-126
