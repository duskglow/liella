//! The integer fixed-point first tier of the binary128 `e^x` and `ln x` (spec §10.3, D-10.52):
//! three 64-bit limbs (192 bits) with a proved relative error below `2^-141.7` (exp) and
//! `2^-145.9` (log), rounded by the test of [`super::round`] with the bound `2^-B`, `B = 140`:
//! the four directions agree with the 384-bit route, and an approximation within the bound of a
//! rounding boundary returns `None` (that operand alone takes the next tier).
//!
//! exp (Tang, ACM TOMS 15(2), 1989; fixed point as in Brent and Zimmermann, Modern Computer
//! Arithmetic, 2010). X1: `|x| = m·2^e`, `m` in `[2^112, 2^113)`, `|x| < 2^14`. X2: `k =
//! round(|x|·128/ln 2) < 2^21.53` from one 64×64-bit product (`|f| <= 1/2 + 2^-41`). X3: `r = |x|
//! − k·ln2/128` in units `2^-199` modulo `2^192` (`ln2/128` in three limbs: error `k/2 + 1` units,
//! `2^-178.8`), `|r| < 2^-8.52`. X4: `e^r − 1 = r + r²·g`, `g` by Horner from `1/12!` (series
//! rest `2^-143.41`), four steps in one limb and six in two (`g` within `2^-144.45` after `r²`);
//! `r²` from the top 128 bits of `|r|` squared exactly (the dropped cross term: `2^-144.07`).
//! X5: `2^(j/128)` in three limbs times `1 + (e^r − 1)`, `j = k mod 128`, `2^(k div 128)` exact.
//! Sum `2^-143.41·(1 + 0.487 + 0.633 + 1.0) = 2^-141.77`. X6: normal results by the test of
//! [`super::round`]; subnormal and overflowing ones by the same test at the result's last place,
//! then [`float::round`] for the flags (refused at `be = 0` with 113 one bits, where tininess
//! after rounding needs the bit below the subnormal round bit).
//!
//! log (Tang, ACM TOMS 16(4), 1990, with the two-step reduction of Wong and Goto, IEEE TC 43(3),
//! 1994: the x87 tier's reciprocals). L1: `x = y''·2^q`, `u1 = y''·c1' − 1` exact in units
//! `2^-129`, `|u1| <= 2^-7`. L2: `u2 = (1 + u1)·c2 − 1` exact in units `2^-192`, `|u2| <
//! 2^-12.98`. L3: `ln(1 + u2) = u2·h1`, `h_k = 1/k − u2·h_(k+1)` from `1/11` (rest `2^-146.4`
//! relative), `h1` within `2^-150`. L4: `ln x = E·ln 2 − ln c1 − ln c2 + u2·h1`, the tables in
//! three limbs: near 1 (`E = 0`, no table term) `u2·h1` alone (`2^-146`); otherwise at `E = 0` in
//! units `2^-191` (`|ln x| >= 2^-13.02`: `2^-145.9`), else in units `2^-177` (`|ln x| >=
//! 0.288|E|`: `2^-162`). Every result is normal.

use super::{ONE, W3, mulhi3, round_within};
use crate::coeffs::tables::{
    WI_C1, WI_C2, WI_C_E, WI_E3, WI_E4, WI_E8, WI_E9, WI_E10, WI_EXP2T, WI_G3, WI_L, WI_L1, WI_L2, WI_LN2_113, WQ_E5,
    WQ_E6, WQ_E7, WQ_E11, WQ_E12, WQ_EXP2T_LO, WQ_G5, WQ_G6, WQ_G7, WQ_G9, WQ_G10, WQ_G11, WQ_L_LO, WQ_L1_LO, WQ_L2_LO,
    WQ_LN2_LO,
};
use crate::float::{self, Exact, Float, Kind, Num};
use crate::status::{Rounding, Status};
use crate::wide_dd::Kernel;
use crate::wide_int::{M64, mul_wide, mulhi, mulhi64};

/// The rounding test's bound exponent: both kernels are within `2^-B` relative.
const B: u32 = 140;
/// The rounding test's bound in units of the 192-bit significand's last place.
const ERR: u128 = (1 << (192 - B)) + 16;

/// `e^x` or `ln x` rounded into `F` (binary128) in `mode` when the tier decides it; `None` sends
/// the caller to the next tier. `n` is finite, nonzero, inside the function's domain and outside
/// the cases the caller settles (exp: `|x| < 2^-116`; log: `x = 1`).
pub(crate) fn eval<F: Float>(k: Kernel, n: &Num, mode: Rounding) -> Option<(F, Status)> {
    if F::PREC != 113 || F::EXPLICIT || n.kind != Kind::Finite || n.sig == 0 || n.sig >> 113 != 0 {
        return None;
    }
    let lz = n.sig.leading_zeros() - 15;
    let (m, e) = (n.sig << lz, n.exp - lz as i32); // |x| = m·2^e, m in [2^112, 2^113)
    match k {
        Kernel::Exp => {
            let (m, e) = exp_kernel(n.neg, m, e)?;
            round_exp::<F>(m, e, mode)
        }
        Kernel::Log if !n.neg => {
            let (neg, m, e) = ln_kernel(m, e)?;
            round_within::<F>(neg, m, e, mode, ERR)
        }
        _ => None,
    }
}

/// `a − b` modulo `2^192`.
fn wsub(a: W3, b: W3) -> W3 {
    let (l, borrow) = a.l.overflowing_sub(b.l);
    W3 { h: a.h.wrapping_sub(b.h).wrapping_sub(u128::from(borrow)), l }
}

/// `a + b` modulo `2^192`.
fn wadd(a: W3, b: W3) -> W3 {
    let (l, carry) = a.l.overflowing_add(b.l);
    W3 { h: a.h.wrapping_add(b.h).wrapping_add(u128::from(carry)), l }
}

const ZERO: W3 = W3 { h: 0, l: 0 };

/// `h·2^64 + lo` modulo `2^192` (two's complement) for a signed table word and its third limb.
fn signed3(h: i128, lo: i64) -> W3 {
    W3 { h: (h as u128).wrapping_sub(u128::from(lo < 0)), l: lo as u64 }
}

/// `⌊v/2^n⌋` of a signed (two's complement) `v`, `0 < n < 64`.
fn sar(v: W3, n: u32) -> W3 {
    W3 { h: ((v.h as i128) >> n) as u128, l: (v.l >> n) | ((v.h as u64) << (64 - n)) }
}

/// A nonzero `v` shifted left until bit 191 is set, and the shift.
fn normalize(v: W3) -> (W3, u32) {
    let up = |v: W3| W3 { h: (v.h << 64) | u128::from(v.l), l: 0 };
    let (v, base) = if v.h >> 64 != 0 { (v, 0) } else { (up(v), 64) };
    let (v, base) = if v.h >> 64 != 0 { (v, base) } else { (up(v), base + 64) };
    let n = v.h.leading_zeros();
    (v.shl(n), base + n)
}

/// Steps X1-X5: `e^x ≈ M·2^E`, `M` in `[2^191, 2^192)`, for `x = ±m·2^e`, `m` in `[2^112,
/// 2^113)`; `None` from `|x| >= 2^14` (the 384-bit route settles those).
fn exp_kernel(neg: bool, m: u128, e: i32) -> Option<(W3, i32)> {
    if e + 112 >= 14 {
        return None;
    }
    // X2: k = round(|x|·128/ln 2), the product in units 2^(e - 7).
    let p = (m >> 49) * (WI_C_E >> 64);
    let sh = 7 - e; // >= 106
    let k = if sh >= 129 { 0 } else { (((p >> (sh - 1)) + 1) >> 1) as u64 };
    // X3: r = |x| − k·ln2/128 in units 2^-199, modulo 2^192.
    let t = e + 199;
    let x = if t >= 64 { W3 { h: m << (t - 64), l: 0 } } else { W3 { h: m, l: 0 }.shr((64 - t) as u32) };
    let lq = W3::with_lo(WI_L, WQ_L_LO);
    let lo = u128::from(k) * u128::from(lq.l);
    let r = wsub(x, W3 { h: u128::from(k).wrapping_mul(lq.h).wrapping_add(lo >> 64), l: lo as u64 });
    let rneg = (r.h >> 127 != 0) != neg; // the sign of the reduced argument
    let a = if r.h >> 127 != 0 { wsub(ZERO, r) } else { r };
    let ks = if neg { -(k as i64) } else { k as i64 };
    // X4: h_k = 1/k! ± a·h_(k+1) from h_12 = 1/12!; g = h_2 in units 2^-128.
    let a1 = (a.h >> 63) as u64; // a·2^-127, units 2^-72
    let ah = (a.h << 1) | u128::from(a.l >> 63); // a·2^-63, units 2^-136
    let s64 = |c: u64, v: u64| if rneg { c - v } else { c + v };
    let s128 = |c: u128, v: u128| if rneg { c - v } else { c + v };
    let h = s64(WQ_E11, mulhi64(a1, WQ_E12) >> 11);
    let h = s64(WI_E10, mulhi64(a1, h) >> 12);
    let h = s64(WI_E9, mulhi64(a1, h) >> 11);
    let h = s64(WI_E8, mulhi64(a1, h) >> 11);
    let h = s128(WQ_E7, mul_wide(ah, h) >> 11);
    let h = s128(WQ_E6, mulhi(ah, h) >> 11);
    let h = s128(WQ_E5, mulhi(ah, h) >> 11);
    let h = s128(WI_E4, mulhi(ah, h) >> 10);
    let h = s128(WI_E3, mulhi(ah, h) >> 10);
    let g = s128(1 << 127, mulhi(ah, h) >> 10);
    // a² = ⌊ah²/2^127⌋ in units 2^-145 from the exact 256-bit square.
    let (a_hi, a_lo) = (ah >> 64, ah & M64);
    let mid = a_hi * a_lo;
    let (sq_lo, c) = (a_lo * a_lo).overflowing_add(mid << 65);
    let sq_hi = a_hi * a_hi + (mid >> 63) + u128::from(c);
    let q = mulhi((sq_hi << 1) | (sq_lo >> 127), g); // a²·g, units 2^-145
    let q = W3 { h: q >> 10, l: (q << 54) as u64 }; // units 2^-199
    let p = if rneg { a.sub(q) } else { a.add(q) }; // |e^r − 1|
    // X5: e^x = 2^(ks div 128)·2^(j/128)·(1 ± p).
    let j = (ks & 127) as usize;
    let tj = W3::with_lo(WI_EXP2T[j], WQ_EXP2T_LO[j]); // units 2^-191
    let d = mulhi3(tj, p).shr(7);
    let rv = if rneg { tj.sub(d) } else { tj.add(d) };
    let kk = (ks >> 7) as i32;
    Some(if rv.h >> 127 == 0 { (rv.shl(1), kk - 192) } else { (rv, kk - 191) })
}

/// Step X6: `M·2^E` (`M` in `[2^191, 2^192)`) rounded into `F` in `mode`, or `None` when the bound
/// does not decide it. A normal result takes [`super::round`]'s test; a subnormal or overflowing
/// one the same test at its last place (`s` bits of `M` below it), then [`float::round`].
fn round_exp<F: Float>(m: W3, e: i32, mode: Rounding) -> Option<(F, Status)> {
    let be = e + 191 + F::EMAX;
    if (1..2 * F::EMAX).contains(&be) {
        return round_within::<F>(false, m, e, mode, ERR);
    }
    let s = if be <= 0 { 80 - be } else { 79 };
    if s > 192 {
        // Below half the least subnormal, but at be = −113 an `M` near 2^192 may lie within the
        // bound of it.
        if be == -113 && m.h >> 64 == u128::from(u64::MAX) {
            return None;
        }
        let (v, flags) = float::round::<F>(Exact { neg: false, sig: 0, exp: 0, sticky: true }, mode);
        return Some((v, float::range_errno(v, Status::flags(flags))));
    }
    if be == 0 && m.h >> 15 == (1 << 113) - 1 {
        return None;
    }
    let m2 = m.shr((s - 79) as u32); // the last place at bit 79
    let low = ((m2.h & 0x7fff) << 64) | u128::from(m2.l);
    if (low & ((1 << 78) - 1)).wrapping_sub(ERR) > (1 << 78) - 2 * ERR {
        return None;
    }
    let (v, flags) = float::round::<F>(Exact { neg: false, sig: m2.h >> 14, exp: e + s - 1, sticky: true }, mode);
    Some((v, float::range_errno(v, Status::flags(flags))))
}

/// Steps L1-L4: `ln x ≈ ±M·2^E`, `M` in `[2^191, 2^192)`, for `x = m·2^e`, `m` in `[2^112,
/// 2^113)`, `x != 1` (`None` at 1).
fn ln_kernel(m: u128, e: i32) -> Option<(bool, W3, i32)> {
    // L1: y'' = m·2^-112, x = y''·2^(e + 112); u1 = y''·c1' − 1 exactly, in units 2^-129.
    let i = ((m >> 105) & 127) as usize;
    let ee = e + 112 + i32::from(i >= 64);
    let u1 = m.wrapping_mul(u128::from(WI_C1[i])) as i128;
    // L2: u2 = (1 + u1)·c2 − 1 = u1·c2 + (c2 − 1) exactly, in units 2^-192.
    let i2 = ((u1 + (1 << 116)) >> 117) as i32;
    let c2 = WI_C2[(i2 + 32) as usize];
    let ua = u1.unsigned_abs();
    let lo = (ua & M64) * u128::from(c2);
    let prod = W3 { h: (ua >> 64) * u128::from(c2) + (lo >> 64), l: lo as u64 };
    let prod = if u1 < 0 { wsub(ZERO, prod) } else { prod };
    let u2 = W3 { h: prod.h.wrapping_add(((i128::from(c2) - (1 << 63)) << 65) as u128), l: prod.l };
    let upos = u2.h >> 127 == 0;
    let w = if upos { u2 } else { wsub(ZERO, u2) };
    // L3: h_k = 1/k ∓ w·h_(k+1) from h_11 = 1/11; h1 in units 2^-191.
    let w1 = (w.h >> 52) as u64; // w·2^-116, units 2^-76
    let wh = (w.h << 12) | u128::from(w.l >> 52); // w·2^-52, units 2^-140
    let s64 = |c: u64, v: u64| if upos { c - v } else { c + v };
    let s128 = |c: u128, v: u128| if upos { c - v } else { c + v };
    let h = s64(WQ_G10, mulhi64(w1, WQ_G11) >> 12);
    let h = s64(WQ_G9, mulhi64(w1, h) >> 12);
    let h = s64(1 << 63, mulhi64(w1, h) >> 13); // 1/8, units 2^-66
    let h = s128(WQ_G7, mul_wide(wh, h) >> 12);
    let h = s128(WQ_G6, mulhi(wh, h) >> 12);
    let h = s128(WQ_G5, mulhi(wh, h) >> 12);
    let h = s128(1 << 127, mulhi(wh, h) >> 13); // 1/4, units 2^-129
    let h3 = s128(WI_G3, mulhi(wh, h) >> 12);
    let t = mulhi(mulhi(wh, wh), h3); // w²·h3, units 2^-153
    let half = w.shr(2); // w/2, units 2^-191
    let h1 = if upos { ONE.sub(half) } else { ONE.add(half) }.add(W3 { h: t >> 26, l: (t << 38) as u64 });
    // L4: |ln(1 + u2)| = w·h1 = P, units 2^-(191 + lzw).
    let (p, lzw) = if w == ZERO {
        (ZERO, 0)
    } else {
        let (wn, lzw) = normalize(w);
        (mulhi3(wn, h1), lzw)
    };
    if ee == 0 && WI_L1[i] == 0 && i2 == 0 {
        // (a) near 1: ±P alone.
        if p == ZERO {
            return None;
        }
        let (pn, l) = normalize(p);
        return Some((!upos, pn, -191 - (lzw + l) as i32));
    }
    let l1 = signed3(WI_L1[i], WQ_L1_LO[i]); // units 2^-191
    let l2 = signed3(WI_L2[(i2 + 32) as usize], WQ_L2_LO[(i2 + 32) as usize]); // units 2^-197
    let (s, units) = if ee == 0 {
        // (b) units 2^-191.
        let pt = p.shr(lzw);
        (wadd(wadd(l1, sar(l2, 6)), if upos { pt } else { wsub(ZERO, pt) }), 191)
    } else {
        // (c) units 2^-177: E·ln 2 + the tables + P.
        let ln2 = W3::with_lo(WI_LN2_113, WQ_LN2_LO);
        let ea = u64::from(ee.unsigned_abs());
        let lo = u128::from(ea) * u128::from(ln2.l);
        let el = W3 { h: u128::from(ea) * ln2.h + (lo >> 64), l: lo as u64 };
        let pt = p.shr(lzw + 14);
        let s = wadd(wadd(if ee < 0 { wsub(ZERO, el) } else { el }, sar(l1, 14)), sar(l2, 20));
        (if upos { wadd(s, pt) } else { wsub(s, pt) }, 177)
    };
    let neg = s.h >> 127 != 0;
    let (sn, l) = normalize(if neg { wsub(ZERO, s) } else { s });
    Some((neg, sn, -(units + l as i32)))
}

#[cfg(test)]
mod tests {
    // Spec clause: §10.3 (_Float128 expf128/logf128 correctly rounded in the four directions, D-10.19).
    use super::super::super::tests::{MODES, Rng, Tally};
    use super::super::tests::{fixed3, value};
    use super::*;
    use crate::float::F128;
    use crate::mp::{self, Mp};

    fn x3(v: W3) -> (u128, u64) {
        (v.h, v.l)
    }

    #[test]
    fn constants_are_the_rounded_values() {
        let one = Mp::from_i64(1);
        let mut f = vec![one]; // f[k] = 1/k!
        for k in 1..=12u64 {
            let next = f[f.len() - 1].div_u64(k);
            f.push(next);
        }
        let x1 = |v: u64| (0, v);
        let x2 = |v: u128| (v >> 64, v as u64);
        let pinned: [((u128, u64), Mp, i64); 11] = [
            (x1(WQ_E12), f[12], 92),
            (x1(WQ_E11), f[11], 89),
            (x2(WQ_E7), f[7], 140),
            (x2(WQ_E6), f[6], 137),
            (x2(WQ_E5), f[5], 134),
            (x1(WQ_G11), one.div_u64(11), 67),
            (x1(WQ_G10), one.div_u64(10), 67),
            (x1(WQ_G9), one.div_u64(9), 67),
            (x2(WQ_G7), one.div_u64(7), 130),
            (x2(WQ_G6), one.div_u64(6), 130),
            (x2(WQ_G5), one.div_u64(5), 130),
        ];
        for (c, v, units) in pinned {
            assert_eq!(c, fixed3(v, units), "units {units}");
        }
        let ln2 = mp::ln2();
        assert_eq!(x3(W3::with_lo(WI_L, WQ_L_LO)), fixed3(ln2.ldexp(-7), 199), "ln2/128");
        assert_eq!(x3(W3::with_lo(WI_LN2_113, WQ_LN2_LO)), fixed3(ln2, 177), "ln 2");
        for j in 0..128 {
            let t = mp::exp(ln2.mul(Mp::from_i64(j as i64)).ldexp(-7));
            assert_eq!(x3(W3::with_lo(WI_EXP2T[j], WQ_EXP2T_LO[j])), fixed3(t, 191), "2^({j}/128)");
        }
        // −ln c in three limbs: the magnitude rounded, negative exactly where c > 1.
        let check = |v: W3, c: Mp, unit: bool, above: bool, units: i64, what: &str| {
            if unit {
                assert_eq!(x3(v), (0, 0), "{what}");
                return;
            }
            assert_eq!(v.h >> 127 != 0, above, "{what}: sign");
            let a = if above { wsub(ZERO, v) } else { v };
            assert_eq!(x3(a), fixed3(mp::ln_atanh(c), units), "{what}");
        };
        for i in 0..128 {
            let one_c = if i >= 64 { 1u32 << 16 } else { 1 << 17 };
            let c = Mp::from_u128(false, u128::from(WI_C1[i]), if i >= 64 { -16 } else { -17 });
            check(signed3(WI_L1[i], WQ_L1_LO[i]), c, WI_C1[i] == one_c, WI_C1[i] > one_c, 191, &format!("L1[{i}]"));
        }
        for ii in 0..65 {
            let c = Mp::from_u128(false, u128::from(WI_C2[ii]), -63);
            let l = signed3(WI_L2[ii], WQ_L2_LO[ii]);
            check(l, c, WI_C2[ii] == 1 << 63, WI_C2[ii] > 1 << 63, 197, &format!("L2[{ii}]"));
        }
    }

    /// Step X6 at the edges of its ranges: decided results equal the rounding of the approximation
    /// itself (with flags and errno), and the test refuses at the last place's boundary.
    #[test]
    fn exp_rounding_matches_the_exact_value_in_every_range() {
        let mut g = Rng(0x5851_f42d_4c95_7f2d);
        let emax = F128::EMAX;
        let mut decided = 0;
        for be in [-130, -113, -112, -60, -1, 0, 1, 2, 2 * emax - 1, 2 * emax, 2 * emax + 1] {
            let e = be - 191 - emax;
            for t in 0..24 {
                let mut m = W3 { h: (u128::from(g.next()) << 64) | u128::from(g.next()) | 1 << 127, l: g.next() };
                if t % 3 == 0 {
                    m.h |= (1 << 127) - 1; // a carry out of the significand
                }
                for mode in MODES {
                    if let Some(got) = round_exp::<F128>(m, e, mode) {
                        decided += 1;
                        assert_eq!(got, mp::round::<F128>(value(false, m, e), mode), "be {be} {m:?} {mode:?}");
                    }
                }
            }
            // The last place lies s − 79 bits above bit 79: ERR units from a boundary decide,
            // ERR − 1 do not.
            let up = if be <= 0 { (1 - be) as u32 } else { 0 };
            if up < 64 {
                for (low, ok) in [(ERR, true), (ERR - 1, false), ((1 << 78) - ERR, true), ((1 << 78) - ERR + 1, false)] {
                    let m = W3 { h: (1 << (127 - up)) | (low >> 64), l: low as u64 }.shl(up);
                    assert_eq!(round_exp::<F128>(m, e, Rounding::Nearest).is_some(), ok, "be {be} low {low:#x}");
                }
            }
        }
        assert!(decided > 500, "{decided} decided");
    }

    /// The tier's operands: exp over exponent bands, the overflow edge, subnormal results and the
    /// bottom of the range (bands 0-5); log over exponent bands, subnormal `x` (6-9), and `x` near 1
    /// from above and below (10, 11: their roundings are near a boundary by design).
    fn operand(g: &mut Rng, b: usize) -> (Kernel, Num) {
        let r128 = |g: &mut Rng| (u128::from(g.next()) << 64) | u128::from(g.next());
        let sig = (r128(g) >> 15) | 1 << 112;
        let fin = |neg: bool, sig: u128, exp: i32| Num { neg, kind: Kind::Finite, sig, exp };
        let mut range = |lo: u128, hi: u128| lo + r128(g) % (hi - lo);
        match b {
            3 => (Kernel::Exp, fin(false, range(11355 << 99, 11357 << 99), -99)),
            4 => (Kernel::Exp, fin(true, range(11355 << 99, 11433 << 99), -99)),
            5 => (Kernel::Exp, fin(true, range((114327 << 99) / 10, (114335 << 99) / 10), -99)),
            9 => (Kernel::Log, fin(false, (r128(g) >> 16) | 1, -16494)),
            10 | 11 => {
                let d = (r128(g) >> (16 + g.next() % 100)) | 1;
                (Kernel::Log, if b == 10 { fin(false, (1 << 112) + d, -112) } else { fin(false, (1 << 113) - d, -113) })
            }
            _ => {
                let (k, (lo, hi)) = match b {
                    0..3 => (Kernel::Exp, [(-116, -9), (-8, 3), (4, 13)][b]),
                    _ => (Kernel::Log, [(-16382, -2), (-1, 0), (1, 16383)][b - 6]),
                };
                let top = lo + (g.next() % (hi - lo + 1) as u64) as i32;
                (k, fin(k == Kernel::Exp && g.next() & 1 != 0, sig, top - 112))
            }
        }
    }

    /// The tier on `n` against the 384-bit route in the four modes, and the kernel's error against
    /// the bound the rounding test assumes.
    fn check(k: Kernel, n: &Num, tally: &mut Tally) {
        let v = Mp::from_u128(n.neg, n.sig, i64::from(n.exp));
        let want = if k == Kernel::Exp { mp::exp(v) } else { mp::ln_atanh(v) };
        let lz = n.sig.leading_zeros() - 15;
        let (m, e) = (n.sig << lz, n.exp - lz as i32);
        let fix = if k == Kernel::Exp { exp_kernel(n.neg, m, e).map(|(m, e)| (false, m, e)) } else { ln_kernel(m, e) };
        match fix {
            None => tally.refused += 1,
            Some((neg, m, e)) => {
                let rel = value(neg, m, e).sub(want).div(want).to_f64().abs();
                assert!(rel < (-f64::from(B)).exp2(), "{n:?} exp={}: error {rel:e}", k == Kernel::Exp);
                tally.worst = tally.worst.max(rel);
            }
        }
        for mode in MODES {
            tally.calls += 1;
            match eval::<F128>(k, n, mode) {
                Some(got) => assert_eq!(got, mp::round::<F128>(want, mode), "{n:?} exp={} {mode:?}", k == Kernel::Exp),
                None => tally.undecided += 1,
            }
        }
    }

    fn sweep(g: &mut Rng, per_band: usize, tally: &mut Tally, near: &mut Tally) {
        for b in 0..12 {
            for _ in 0..per_band {
                let (k, n) = operand(g, b);
                check(k, &n, if b >= 10 { &mut *near } else { &mut *tally });
            }
        }
    }

    #[test]
    fn tier_agrees_with_the_wide_route() {
        let (mut tally, mut near) = (Tally::default(), Tally::default());
        sweep(&mut Rng(0x9e37_79b9_7f4a_7c15), 150, &mut tally, &mut near);
        // The smallest operand the exp tier takes, both signs; log of the extremes.
        for (k, neg, sig, exp) in [
            (Kernel::Exp, false, 1u128 << 112, -228),
            (Kernel::Exp, true, 1 << 112, -228),
            (Kernel::Log, false, 1, -16494),
            (Kernel::Log, false, (1 << 113) - 1, 16271),
        ] {
            check(k, &Num { neg, kind: Kind::Finite, sig, exp }, &mut tally);
        }
        assert_eq!(tally.refused, 0, "refused inside the tier's range");
        assert!(tally.undecided * 1000 <= tally.calls, "{} of {} undecided", tally.undecided, tally.calls);
        eprintln!(
            "wide_int::quad::explog: {} calls, {} undecided, worst 2^{:.1}; near 1: {} calls, {} undecided, worst 2^{:.1}",
            tally.calls,
            tally.undecided,
            tally.worst.log2(),
            near.calls,
            near.undecided,
            near.worst.log2()
        );
    }

    /// The brief's sweep: 10^7 seeded operands over the twelve bands in the four modes.
    /// `cargo test --release -p liella-math --lib wide_int::quad::explog -- --ignored`.
    #[test]
    #[ignore = "10^7 operands against the 384-bit route: cargo xtask long-sweeps runs it in release (nightly and release runs)"]
    fn tier_agrees_with_the_wide_route_long() {
        const TOTAL: usize = 10_000_000;
        let threads = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
        let tallies: Vec<(Tally, Tally)> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..threads)
                .map(|i| {
                    s.spawn(move || {
                        let mut g = Rng(0x2545_f491_4f6c_dd1d ^ (i as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15));
                        let (mut tally, mut near) = (Tally::default(), Tally::default());
                        sweep(&mut g, TOTAL / 12 / threads, &mut tally, &mut near);
                        (tally, near)
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap_or_else(|p| std::panic::resume_unwind(p))).collect()
        });
        let (mut all, mut near) = (Tally::default(), Tally::default());
        for (t, n) in &tallies {
            for (a, b) in [(&mut all, t), (&mut near, n)] {
                a.calls += b.calls;
                a.undecided += b.undecided;
                a.refused += b.refused;
                a.worst = a.worst.max(b.worst);
            }
        }
        eprintln!(
            "wide_int::quad::explog long: {} calls, {} undecided, {} refused, worst 2^{:.2}; near 1: {} calls, {} undecided, worst 2^{:.2}",
            all.calls,
            all.undecided,
            all.refused,
            all.worst.log2(),
            near.calls,
            near.undecided,
            near.worst.log2()
        );
        assert!(all.undecided * 100_000 <= all.calls, "{} of {} undecided", all.undecided, all.calls);
    }
}
