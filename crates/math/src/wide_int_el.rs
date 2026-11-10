//! The integer fixed-point first tier of the x87 `long double` exponentials and logarithms (spec
//! §10.3, D-10.52, D-10.73): `e^x`, `2^x`, `10^x`, `e^x − 1`, `ln x`, `log2 x`, `log10 x` and
//! `ln(1 + x)` evaluated in 64-bit limbs with a proved relative error below `2^-101`, rounded by
//! the integer rounding test of [`crate::wide_int`] (which assumes `2^-92`: the four directions
//! are four integer roundings, no MXCSR or x87 control-word traffic). Undecided operands (about
//! `2^-26`) and results outside the normal range go on to the double-double tier
//! ([`crate::wide_dd`]) and then the 384-bit route. Only the format with a 64-bit significand uses
//! the tier.
//!
//! Reuse (powl, the hyperbolics, `_Float128`). [`exp_fix`] takes `|x|·2^e` and a scaled constant
//! and returns `e^x` (or `e^x − 1`) as `(sign, M, E)`, `M` in `[2^127, 2^128)`; [`ln_fix`] takes
//! any positive `t·2^q` with `t` up to 128 bits and returns `ln` in the same form. A caller that
//! needs more than 64 bits of argument (powl's `y·ln x`) passes the reduced `k` and `|r|` to
//! [`exp_table`] directly; `_Float128` extends the polynomials by the terms named in the proof.
//!
//! Method. Tang's table methods (P. T. P. Tang, "Table-driven implementation of the exponential
//! function in IEEE floating-point arithmetic", ACM TOMS 15(2), 1989; "Table-driven
//! implementation of the logarithm function in IEEE floating-point arithmetic", ACM TOMS 16(4),
//! 1990), a two-step multiplicative reduction for the logarithm (W. F. Wong, E. Goto, "Fast
//! hardware-based algorithms for elementary function computations using rectangular multipliers",
//! IEEE TC 43(3), 1994), Horner evaluation in fixed point (R. Brent, P. Zimmermann, Modern
//! Computer Arithmetic, 2010, ch. 4). Constants and tables from Python `decimal` at 90 digits,
//! each pinned against the 384-bit arithmetic by `constants_are_the_rounded_values`.
//!
//! Notation: `|x| = m·2^e`, `2^63 <= m < 2^64`; a word "in units `2^-u`" holds `v·2^u`.
//!
//! Proof, exponentials (`c = 128/ln 2`, `128` or `128·log2 10`; callers send `2^-70 <= |x| < 2^14`).
//! E1. `m·C` (`C` = `c` in units `2^-120`, `2^-119` for `10^x`, rounded: `|C − c| <= 2^-121`
//!     relative units) is exact in 192 bits; its bits below `2^-128` are dropped (less than
//!     `2^-128`). `k = round(|x|·C)` (below `2^23`), `f = |x|·C − k`, `|f| <= 1/2`; with the
//!     signs, `x·c = k + f + δ`, `|δ| <= 2^14·2^-120 + 2^-128`. `r = f·ln2/128` from `L`
//!     (`ln2/128` in units `2^-135`, within `2^-136`) by one truncated 128x128 product: `|r| <=
//!     ln2/256 < 2^-8.5`, `a = |r|` in units `2^-136`, and `x = k·ln2/128 + r + ε`, `|ε| <=
//!     2^-114.4` (the `δ` term `2^-106.9·ln2/128` dominates).
//! E2. `e^r − 1 = a·h_1` (sign of `r` for odd powers), `h_k = 1/k! ± a·h_(k+1)` to `h_10 = 1/10!`:
//!     the series remainder is below `a^11/11!·1.01 < 2^-110` relative to `a`. `h_9..h_5` in
//!     64-bit words (units noted at each step, each within 1.5 units plus `a·δ_(k+1)`; the top
//!     word of `a` is short by less than `2^-72`), `h_4..h_1` and `P = a·h_1` in 128 bits:
//!     `δ_5 <= 2^-69.4`, `δ_4 <= a·δ_5 <= 2^-77.9`, `δ_3 <= 2^-86.4`, `δ_2 <= 2^-94.9`, `δ_1 <=
//!     2^-103.4`; `P` within `2^-103.3·a + 2^-134` (every partial sum stays positive: each
//!     product is below `2^-8.5` of the coefficient it meets).
//! E3. `e^x = 2^K·T_j·(1 ± P)·e^ε` with `k = 128K + j`, `T_j = 2^(j/128)` in units `2^-127`
//!     (within `2^-128`): `R = T_j ± T_j·P` within `2^-110.4` relative (`2^-128` table, `2^-111`
//!     from `P`, `2^-126.9` truncation, `2^-114.4` from `ε`). `R` lies in `[2^126.99, 2^127.997]`.
//! E4. `e^x − 1`: `k = 0` (`|x| < ln2/256`): `x·h_1` with the exact `x`, within `2^-103.2`
//!     relative. `k != 0`: `|x| >= ln2/256·(1 − 2^-100)`, where `e^x/|e^x − 1| < 2^8.54`; `D =
//!     R − 2^(127−K)` (`0 <= K <= 127`; above, the 1 lies below `2^-128` relative and is
//!     dropped) or `2^127 − ⌊R/2^-K⌋` (`K < 0`, within one unit): within `2^8.54·2^-110.4 +
//!     2^-118.4 < 2^-101.8` relative.
//!
//! Proof, logarithms.
//! L1. `t·2^q` with `t` normalised to `t''` in `[2^110, 2^111)` (bits below dropped: only
//!     `ln(1 + x)` above `2^47` has more than 111 bits, `2^-110` relative, absolute in a result
//!     above 32): `y'' = t''·2^-110` in `[1, 2)`, `i` = its 7 bits after the point, `c1'` from
//!     `C1` (17 bits, about `1/y''`; exactly 1 for `i = 0` and 1/2 for `i = 127`), `Y = t''·c1'`
//!     exact in units `2^-127`. With `E = q + top + [i >= 64]` and `c1 = c1'·2^[i >= 64]`: `ln x =
//!     E·ln2 − ln c1 + ln(1 + u1)`, `u1 = Y·2^-127 − 1`, `|u1| <= 2^-7` (the generator checks
//!     every interval). The intervals around 1 (`i = 0`, `i = 127` with `E = 0`) have `c1 = 1`:
//!     there `u1 = x − 1` exactly.
//! L2. `i2 = round(4096·u1)` in `[-32, 32]`, `c2` from `C2` (`2^63/(1 + i2/4096)` rounded, units
//!     `2^-63`; exactly 1 for `i2 = 0`), `Z = ⌊Y·c2/2^64⌋` in units `2^-126` (within one unit):
//!     `ln(1 + u1) = −ln c2 + ln(1 + u2)`, `u2 = Z·2^-126 − 1`, `|u2| <= 2^-13/(1 − 2^-7) + 2^-63
//!     < 2^-12.98`, within `2^-126` absolute.
//! L3. `ln(1 + u2) = ±w·h_1`, `w = |u2|` in units `2^-140`, `h_k = 1/k ∓ w·h_(k+1)` to `h_8 = 1/8`
//!     (alternating for `u2 > 0`, all positive for `u2 < 0`): the remainder is below `w^8/9 <
//!     2^-107` relative to `w`; `h_7..h_4` in 64-bit words, `h_3..h_1` in 128 bits: `δ_4 <=
//!     2^-64.4`, `δ_3 <= w·δ_4 + 2^-128 <= 2^-77.4`, `δ_2 <= 2^-90.4`, `δ_1 <= 2^-103.4`: `w·h_1`
//!     within `2^-103.3` relative.
//! L4. Result. (a) `E = 0`, `c1 = 1`, `i2 = 0` (`|x − 1| < 2^-13`): `ln x = u1·h_1` with the exact
//!     `u1` (normalised): within `2^-103.2` relative. (b) `E = 0`, `c1 = 1`, `i2 != 0`: `|ln x| >=
//!     2^-13.02`; the sum `−ln c2 + ln(1 + u2)` in units `2^-127` (`L2` rounded to `2^-134`) is
//!     within `2^-116.2` absolute: `2^-103.2` relative. (c) `E = 0`, `c1 != 1`: `|ln x| >= 2^-8.01`,
//!     the same sum with `L1` (units `2^-127`): below `2^-108` relative. (d) `E != 0`: units
//!     `2^-113`, `E·ln2` with `ln2` within `2^-114`, three truncations and the rounded terms:
//!     within `|E|·2^-114 + 2^-111.3` absolute, `|ln x| >= 0.288·|E|`: below `2^-109.5`.
//!     `log2`, `log10`: one more truncated product by `1/ln2` (`1/ln10`), `2^-125` relative.
//!
//! The rounding test ([`crate::wide_int`]) assumes `2^-92`; every result above is within
//! `2^-101.8` relative, and it refuses results outside the normal range (overflow, underflow and
//! subnormal results stay with the later tiers).

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::as_conversions,
    clippy::indexing_slicing,
    clippy::unreadable_literal
)] // fixed-point words, exponents and table indices of bounded range (each noted where it is formed)

use crate::float::{Float, Kind, Num};
use crate::status::{Rounding, Status};
use crate::wide_dd::Kernel;
use crate::wide_int::{mul_wide, mulhi, mulhi64, round, M64};

/// A result before rounding: `(negative, M, E)` for `±M·2^E`, `M` in `[2^127, 2^128)`.
pub(crate) type Fix = (bool, u128, i32);

/// `f(x)` rounded into `F` in `mode` when the tier decides it; `None` sends the caller to the
/// double-double tier. `n` is finite, nonzero, inside the function's domain and outside the cases
/// the caller settles itself (exact results, tiny arguments).
pub(crate) fn eval<F: Float>(k: Kernel, n: &Num, mode: Rounding) -> Option<(F, Status)> {
    if F::PREC != 64 || !F::EXPLICIT {
        return None;
    }
    let (neg, m, e) = kernel(k, n)?;
    round::<F>(neg, m, e, mode)
}

/// The approximation of step E4 or L4, or `None` outside the proved ranges.
fn kernel(k: Kernel, n: &Num) -> Option<Fix> {
    if n.kind != Kind::Finite || n.sig == 0 || n.sig >> 64 != 0 {
        return None;
    }
    let lz = (n.sig as u64).leading_zeros();
    let m = (n.sig as u64) << lz;
    let e = n.exp - lz as i32; // |x| = m·2^e
    match k {
        Kernel::Exp => exp_fix(n.neg, m, e, C_E, 120, false),
        Kernel::Exp2 => exp_fix(n.neg, m, e, 1 << 127, 120, false),
        Kernel::Exp10 => exp_fix(n.neg, m, e, C_10, 119, false),
        Kernel::Expm1 => exp_fix(n.neg, m, e, C_E, 120, true),
        Kernel::Log => (!n.neg).then(|| ln_fix(u128::from(m), e)),
        Kernel::Log2 => (!n.neg).then(|| times(ln_fix(u128::from(m), e), LOG2E, 127)),
        Kernel::Log10 => (!n.neg).then(|| times(ln_fix(u128::from(m), e), LOG10E, 129)),
        Kernel::Log1p => log1p_fix(n.neg, m, e),
    }
}

/// `r·c·2^-cs` (one truncated product, `2^-125` relative).
fn times((neg, m, e): Fix, c: u128, cs: i32) -> Fix {
    let p = mulhi(m, c);
    let l = p.leading_zeros();
    (neg, p << l, e + 128 - cs - l as i32)
}

/// Steps E1-E4: `e^(±m·2^e·c·ln2/128)` (`expm1`: minus 1) for `c` in units `2^-cs` (`cs` 119 or
/// 120), `2^-70 <= |x| < 2^14`.
pub(crate) fn exp_fix(neg: bool, m: u64, e: i32, c: u128, cs: i32, minus_one: bool) -> Option<Fix> {
    if !(-70..14).contains(&(e + 63)) {
        return None;
    }
    // E1: m·C in 192 bits (p2: top word), then the integer and the 128-bit fraction.
    let s = (cs - 128 - e) as u32; // 41..=125
    let mm = u128::from(m);
    let lo = mm * (c & M64);
    let hi = mm * (c >> 64);
    let mid = (lo >> 64) + (hi & M64);
    let p2 = (hi >> 64) + (mid >> 64);
    let p01 = (mid << 64) | (lo & M64);
    let frac = (p01 >> s) | (p2 << (128 - s));
    let up = frac >> 127 != 0; // f >= 1/2: k rounds up, r < 0
    let k = (p2 >> s) as i64 + i64::from(up); // below 2^23
    let f = if up { frac.wrapping_neg() } else { frac }; // |f| <= 2^127
    let a = mulhi(f, L) << 1; // |r|, units 2^-136
    let (k, rneg) = if neg { (-k, !up) } else { (k, up) };
    let h1 = exp_poly(a, rneg);
    if minus_one && k == 0 {
        // E4, |x| < ln2/256: x·h_1 with the exact x.
        let p = mulhi(mm << 64, h1); // units 2^(e − 63)
        let l = p.leading_zeros();
        return Some((neg, p << l, e - 63 - l as i32));
    }
    let (r, big) = exp_table(k, rneg, mulhi(a, h1));
    if !minus_one {
        let l = r.leading_zeros(); // 0 or 1
        return Some((false, r << l, big - 127 - l as i32));
    }
    let (dneg, d, de) = if big >= 0 {
        (false, if big <= 127 { r - (1 << (127 - big)) } else { r }, big - 127)
    } else {
        (true, (1 << 127) - r.checked_shr((-big) as u32).unwrap_or(0), -127)
    };
    let l = d.leading_zeros(); // d >= 2^118 (step E4)
    Some((dneg, d << l, de - l as i32))
}

/// Step E3: `(R, K)` with `R·2^(K − 127) = 2^(k/128)·(1 ± P)`, `P` = `|e^r − 1|` in units
/// `2^-136` (at most `2^127.5`), `rneg` the sign of `r`.
pub(crate) fn exp_table(k: i64, rneg: bool, p: u128) -> (u128, i32) {
    let t = EXP2T[(k & 127) as usize];
    let tp = mulhi(t, p) >> 8; // units 2^-127
    (if rneg { t - tp } else { t + tp }, (k >> 7) as i32)
}

/// Step E2: `h_1` in units `2^-127` (`a` = `|r|` in units `2^-136`, `neg`: `r < 0`).
#[inline]
fn exp_poly(a: u128, neg: bool) -> u128 {
    let a1 = (a >> 64) as u64; // units 2^-72
    let s64 = |c: u64, v: u64| if neg { c - v } else { c + v };
    let s128 = |c: u128, v: u128| if neg { c - v } else { c + v };
    let h = s64(E9, mulhi64(a1, E10) >> 11); // units 2^-82
    let h = s64(E8, mulhi64(a1, h) >> 11); // units 2^-79
    let h = s64(E7, mulhi64(a1, h) >> 11); // units 2^-76
    let h = s64(E6, mulhi64(a1, h) >> 11); // units 2^-73
    let h = s64(E5, mulhi64(a1, h) >> 11); // units 2^-70
    let h = s128(E4, mul_wide(a, h) >> 10); // units 2^-132
    let h = s128(E3, mulhi(a, h) >> 10); // units 2^-130
    let h = s128(1 << 127, mulhi(a, h) >> 10); // 1/2, units 2^-128
    s128(1 << 127, mulhi(a, h) >> 9) // 1, units 2^-127
}

/// Steps L1-L4: `ln(t·2^q)`, `t > 0`.
pub(crate) fn ln_fix(t: u128, q: i32) -> Fix {
    let top = 127 - t.leading_zeros() as i32;
    let t2 = if top <= 110 { t << (110 - top) } else { t >> (top - 110) }; // y'', units 2^-110
    let i = ((t2 >> 103) & 127) as usize;
    let y = t2 * u128::from(C1[i]); // 1 + u1, units 2^-127, exact
    ln_core(y, q + top + i32::from(i >= 64), L1[i], None)
}

/// Steps L2-L4 on `Y = 1 + u1` (units `2^-127`), `E` and `−ln c1`; `exact`: `|u1|` normalised
/// with its exponent when `Y` does not hold `u1` exactly (`ln(1 + x)` for tiny `x`).
fn ln_core(y: u128, big_e: i32, l1: i128, exact: Option<(u128, i32)>) -> Fix {
    let v1 = y.wrapping_sub(1 << 127) as i128; // u1, |u1| <= 2^120 units
    let i2 = ((v1 + (1 << 114)) >> 115) as i32; // round(4096·u1), -32..=32
    let ii = (i2 + 32) as usize;
    let v2 = mul_wide(y, C2[ii]).wrapping_sub(1 << 126) as i128; // u2, units 2^-126
    let w = v2.unsigned_abs() << 14; // |u2|, units 2^-140
    let h1 = ln_poly(w, v2 < 0);
    if big_e == 0 && l1 == 0 && i2 == 0 {
        // L4 (a): u1·h_1 with u1 exact.
        let (a, ae) = exact.unwrap_or_else(|| {
            let a = v1.unsigned_abs();
            let l = a.leading_zeros();
            (a << l, -127 - l as i32)
        });
        let p = mulhi(a, h1); // units 2^(ae + 1)
        let l = p.leading_zeros();
        return (v1 < 0, p << l, ae + 1 - l as i32);
    }
    let p = mulhi(w, h1) as i128; // |ln(1 + u2)|, units 2^-140
    let p = if v2 < 0 { -p } else { p };
    let (s, units) = if big_e == 0 {
        (l1 + (L2[ii] >> 6) + (p >> 13), -127)
    } else {
        (i128::from(big_e) * LN2_113 as i128 + (l1 >> 14) + (L2[ii] >> 20) + (p >> 27), -113)
    };
    let a = s.unsigned_abs();
    let l = a.leading_zeros();
    (s < 0, a << l, units - l as i32)
}

/// Step L3: `h_1` in units `2^-127` (`w = |u2|` in units `2^-140`, `neg`: `u2 < 0`).
#[inline]
fn ln_poly(w: u128, neg: bool) -> u128 {
    let w1 = (w >> 64) as u64; // units 2^-76
    let s64 = |c: u64, v: u64| if neg { c + v } else { c - v };
    let s128 = |c: u128, v: u128| if neg { c + v } else { c - v };
    let h = s64(G7, w1 >> 13); // w·(1/8): units 2^-66
    let h = s64(G6, mulhi64(w1, h) >> 12); // units 2^-66
    let h = s64(G5, mulhi64(w1, h) >> 12); // units 2^-66
    let h = s64(1 << 63, mulhi64(w1, h) >> 13); // 1/4, units 2^-65
    let h = s128(G3, mul_wide(w, h) >> 12); // units 2^-129
    let h = s128(1 << 127, mulhi(w, h) >> 13); // 1/2, units 2^-128
    s128(1 << 127, mulhi(w, h) >> 13) // 1, units 2^-127
}

/// `ln(1 + x)` for `x > −1`: `|x| < 2^-7` runs L2-L4 on `1 + x` with the exact `x`; otherwise
/// `1 + x` exactly (below `2^47`) or to 111 bits through [`ln_fix`].
fn log1p_fix(neg: bool, m: u64, e: i32) -> Option<Fix> {
    let mm = u128::from(m);
    if e + 63 < -7 {
        if e + 63 < -70 {
            return None;
        }
        let sh = e + 127; // -6..=55
        let ax = if sh >= 0 { mm << sh } else { mm >> -sh }; // |x|, units 2^-127
        let y = if neg { (1 << 127) - ax } else { (1 << 127) + ax };
        return Some(ln_core(y, 0, 0, Some((mm << 64, e - 64))));
    }
    let (t, q) = if e <= 0 {
        let one = 1u128 << -e; // -e <= 70
        (if neg { one - mm } else { one + mm }, e)
    } else if neg {
        return None;
    } else {
        ((mm << 63) + if e <= 63 { 1 << (63 - e) } else { 0 }, e - 63)
    };
    (t != 0).then(|| ln_fix(t, q))
}

// Constants and tables (generated: Python `decimal`, 90 digits; each rounded to nearest in the
// units noted; `constants_are_the_rounded_values` pins them).
const C_E: u128 = 0xb8aa3b29_5c17f0bb_be87fed0_691d3e89; // 128/ln2, units 2^-120
const C_10: u128 = 0xd49a784b_cd1b8afe_492bf6ff_4dafdb4d; // 128·log2(10), units 2^-119
const L: u128 = 0xb17217f7_d1cf79ab_c9e3b398_03f2f6af; // ln2/128, units 2^-135
const E10: u64 = 0x93f27dbbc4fae397; // 1/10!, units 2^-85
const E9: u64 = 0xb8ef1d2ab6399c7d; // 1/9!, units 2^-82
const E8: u64 = 0xd00d00d00d00d00d; // 1/8!, units 2^-79
const E7: u64 = 0xd00d00d00d00d00d; // 1/7!, units 2^-76
const E6: u64 = 0xb60b60b60b60b60b; // 1/6!, units 2^-73
const E5: u64 = 0x8888888888888889; // 1/5!, units 2^-70
const E4: u128 = 0xaaaaaaaa_aaaaaaaa_aaaaaaaa_aaaaaaab; // 1/4!, units 2^-132
const E3: u128 = 0xaaaaaaaa_aaaaaaaa_aaaaaaaa_aaaaaaab; // 1/3!, units 2^-130
const LN2_113: u128 = 0x000162e4_2fefa39e_f35793c7_673007e6; // ln2, units 2^-113
const LOG2E: u128 = 0xb8aa3b29_5c17f0bb_be87fed0_691d3e89; // 1/ln2, units 2^-127
const LOG10E: u128 = 0xde5bd8a9_37287195_355baaaf_ad33dc32; // 1/ln10, units 2^-129
const G7: u64 = 0x9249249249249249; // 1/7, units 2^-66
const G6: u64 = 0xaaaaaaaaaaaaaaab; // 1/6, units 2^-66
const G5: u64 = 0xcccccccccccccccd; // 1/5, units 2^-66
const G3: u128 = 0xaaaaaaaa_aaaaaaaa_aaaaaaaa_aaaaaaab; // 1/3, units 2^-129
const EXP2T: [u128; 128] = [
    0x80000000_00000000_00000000_00000000,
    0x80b1ed4f_d999ab6c_25335719_b6e6fd20,
    0x8164d1f3_bc030773_7be56527_bd14def5,
    0x8218af43_73fc25eb_9c7cd106_d23f3768,
    0x82cd8698_ac2ba1d7_3e2a475b_46520bff,
    0x8383594e_efb6ee36_e201d4ec_3d93f684,
    0x843a28c3_acde4046_1af92eca_13fd1582,
    0x84f1f656_379c1a29_0f03062c_26b5ba5d,
    0x85aac367_cc487b14_c5c95b8c_2154c1b2,
    0x8664915b_923fba03_db82dc49_ee2f4556,
    0x871f6196_9e8d1010_3a1727c5_7b52a956,
    0x87db357f_f698d791_9048eec5_0a1328a7,
    0x88980e80_92da8527_5df8d76c_98c67563,
    0x8955ee03_618e5fdc_95d69926_b4717b94,
    0x8a14d575_496efd9a_080ca1d9_2c3680c2,
    0x8ad4c645_2c728924_06ab9eea_b09dfc95,
    0x8b95c1e3_ea8bd6e6_fbe46287_58a53c90,
    0x8c57c9c4_646f4ddd_fb85cd1e_1282e4be,
    0x8d1adf5b_7e5ba9e5_b4c7b496_8e41ad36,
    0x8ddf0420_22e69cd5_8f395a21_3f1afcd6,
    0x8ea4398b_45cd53c0_2dc0144c_8783d4c6,
    0x8f6a8117_e6c8e5c4_0cffb089_0e8f2827,
    0x9031dc43_1466b1dc_775814a8_494e87e2,
    0x90fa4c8b_eee4b12a_97e9494a_5eda5b0f,
    0x91c3d373_ab11c336_0fd6d8e0_ae5ac9d8,
    0x928e727d_9531f9ac_155bef4f_4a408d4e,
    0x935a2b2f_13e6e92b_d339940e_9d924ee7,
    0x9426ff0f_ab1c04b6_78ae781e_504b3fed,
    0x94f4efa8_fef70961_2e8afad1_2551de54,
    0x95c3fe86_d6cc7fee_f52329c7_e55c4221,
    0x96942d37_20185a00_48ea9b68_3a9c22c5,
    0x97657d49_f17ab08e_507a2ea9_1c19d7b1,
    0x9837f051_8db8a96f_46ad2318_2e42f6f6,
    0x990b87e2_66c189a9_ce78e180_47c36ef2,
    0x99e04593_20b7fa64_e43086cb_34b5fcaf,
    0x9ab62afc_94ff864a_311a3b1b_9d79c6b7,
    0x9b8d39b9_d54e5538_a2a817a2_a3cc3f1f,
    0x9c657368_2ec32c2d_4e586cdf_686429df,
    0x9d3ed9a7_2cffb750_de494cf0_50e99b0b,
    0x9e196e18_9d472420_00f9145a_c79bbaf0,
    0x9ef53260_91a111ad_a0911f09_ebb9fdd1,
    0x9fd22825_6400dd05_fb80d520_c197dc61,
    0xa0b0510f_b9714fc2_192dc79e_db0fd9a9,
    0xa18faeca_8544b6e3_8221ca08_667640f1,
    0xa2704303_0c496818_9b7a04ef_80cfdea8,
    0xa3520f68_e802bb92_897a2c91_4ecbefa0,
    0xa43515ae_09e6809e_0d1db483_1781e1ef,
    0xa5195786_be9ef339_6c5e7a37_cac3230f,
    0xa5fed6a9_b15138ea_1cbd7f62_1710701b,
    0xa6e594cf_eee86b1d_9b778d4f_06624259,
    0xa7cd93b4_e9653569_9ec5b4d5_039f72af,
    0xa8b6d516_7b320e08_97a96426_c110c874,
    0xa9a15ab4_ea7c0ef8_541e24ec_3531fa73,
    0xaa8d2652_ec907629_76310121_a6533932,
    0xab7a39b5_a93ed337_658023b2_759e0079,
    0xac6896a4_be3fe929_5e15b9a1_de79764a,
    0xad583eea_42a14ac6_4980a8c8_f59a2ec4,
    0xae493452_ca35b80e_258dc0b4_c35101ec,
    0xaf3b78ad_690a4374_df26101c_cbb35033,
    0xb02f0dcb_b6e04583_b7ac9524_371d9a75,
    0xb123f581_d2ac258f_87d037e9_6d215d8e,
    0xb21a31a6_6618fe3b_7c38a627_6cd27208,
    0xb311c412_a9112489_3ecf14dc_798a519c,
    0xb40aaea2_654b9840_e2b913dc_f9938360,
    0xb504f333_f9de6484_597d89b3_754abe9f,
    0xb60093a8_5ed5f76b_b54cc007_a799fef6,
    0xb6fd91e3_28d17791_07165f0d_dd541a5a,
    0xb7fbefca_8ca41e7c_3f0da79f_109dffce,
    0xb8fbaf47_62fb9ee9_1b879778_566b65a2,
    0xb9fcd245_2c0b9dea_e4d27345_588c1571,
    0xbaff5ab2_133e45fb_74d519d2_4593838c,
    0xbc034a7e_f2e9fb0c_d7014042_c595d95f,
    0xbd08a39f_580c36be_a8811fb6_6d0faf7a,
    0xbe0f6809_860993e2_499a22c9_bab1596e,
    0xbf1799b6_7a731082_e815d0ab_cbf0b851,
    0xc0213aa1_f0d08db0_6f33b24d_1aa75383,
    0xc12c4cca_66709456_7c457d59_a50087b5,
    0xc238d231_1e3d6672_97b5cbe3_204a9b88,
    0xc346ccda_24976407_20ec8561_28b83a42,
    0xc4563ecc_5334cb32_985e6f96_a74eb094,
    0xc5672a11_5506dadd_3e2ad0c9_64dd9f37,
    0xc67990b5_aa245f79_550e68b0_e2aec255,
    0xc78d74c8_abb9b15c_c13a2e39_76c0277e,
    0xc8a2d85c_8ffe2c45_30da34fb_5b8700e1,
    0xc9b9bd86_6e2f27a2_80e1f92a_0511697e,
    0xcad2265e_4290774d_a41b4ad0_7e37be3f,
    0xcbec14fe_f2727c5c_f4907c8f_45ebf6dd,
    0xcd078b86_503dcdd1_884dc623_39bdf58d,
    0xce248c15_1f8480e3_e235838f_95f2c6ed,
    0xcf4318cf_191918c1_2653c732_6370087d,
    0xd06333da_ef2b2594_d6d45c65_59a4d502,
    0xd184df62_51699ac6_0b8fbb86_d56aa3fd,
    0xd2a81d91_f12ae45a_12248e57_c3de4028,
    0xd3ccf099_859ac379_6fd958ac_78d4c3cb,
    0xd4f35aab_cfedfa1f_5921deff_a6262c5b,
    0xd61b5dfe_9f9bce06_dcb35189_32fe39f2,
    0xd744fcca_d69d6af4_39a68bb9_902d3fde,
    0xd870394c_6db32c84_21566fe3_7b65072f,
    0xd99d15c2_78afd7b5_fe873dec_a3e12bac,
    0xdacb946f_2ac9cc71_c40888b2_439e38b9,
    0xdbfbb797_daf23755_3d840d5a_9e29aa64,
    0xdd2d8185_08324c20_659e357a_da3f94b9,
    0xde60f482_5e0e9123_dd07a2d9_e8466859,
    0xdf9612de_b8f04420_46b8128c_71a24fd0,
    0xe0ccdeec_2a94e111_06589504_8dd333ca,
    0xe2055aff_fe83d368_a6fc1078_c14529b3,
    0xe33f8972_be8a5a51_09bfe907_95980eed,
    0xe47b6ca0_373da88d_65e24402_e2216edb,
    0xe5b906e7_7c8348a8_1e5e8f4a_4edbb0ed,
    0xe6f85aaa_ee1fce22_7c4ac7d6_28df28b0,
    0xe8396a50_3c4bdc68_791790d0_ac70c7de,
    0xe97c3840_6c4f8c56_f091cc4f_51012da6,
    0xeac0c6e7_dd24392e_d02d75b3_706e54fb,
    0xec0718b6_4c1cbddc_27ce8244_02fc25f6,
    0xed4f301e_d9942b84_600d2db6_a64bfb12,
    0xee990f98_0da3025b_4aef1e03_1851c991,
    0xefe4b99b_dcdaf5cb_46561cf6_948db913,
    0xf13230a7_ad094509_3b0fd0bd_6d3233f4,
    0xf281773c_59ffb139_e8980a9c_c8f47a4b,
    0xf3d28fde_3a641a5a_a4594191_bc33ac54,
    0xf5257d15_2486cc2c_7b9d0c7a_ed980fc3,
    0xf67a416c_733f846d_81897dca_4e77a310,
    0xf7d0df73_0ad13bb8_fe90d496_d60fb6eb,
    0xf92959bb_5dd4ba74_34b7e1b1_c86a6357,
    0xfa83b2db_722a033a_7c25bb14_315d7fcd,
    0xfbdfed6c_e5f09c48_9da5ff39_5ecae2e7,
    0xfd3e0c0c_f486c174_853f3a59_31e0ee03,
    0xfe9e115c_7b8f884b_add25995_e79d2f09,
];
const C1: [u32; 128] = [
    131072, 129554, 128561, 127583, 126620, 125672, 124738, 123817,
    122910, 122016, 121135, 120267, 119411, 118567, 117735, 116914,
    116105, 115307, 114520, 113744, 112978, 112222, 111477, 110741,
    110015, 109298, 108590, 107892, 107203, 106522, 105850, 105186,
    104531, 103884, 103244, 102613, 101989, 101373, 100764, 100162,
    99568, 98981, 98400, 97826, 97259, 96699, 96145, 95597,
    95055, 94520, 93990, 93466, 92949, 92436, 91930, 91429,
    90933, 90443, 89958, 89478, 89004, 88534, 88069, 87609,
    87154, 86704, 86258, 85817, 85380, 84948, 84520, 84096,
    83677, 83262, 82850, 82443, 82040, 81641, 81246, 80854,
    80466, 80082, 79702, 79325, 78952, 78582, 78215, 77853,
    77493, 77137, 76784, 76434, 76087, 75744, 75403, 75066,
    74731, 74400, 74072, 73746, 73423, 73103, 72786, 72472,
    72160, 71851, 71545, 71241, 70940, 70641, 70345, 70051,
    69760, 69471, 69184, 68900, 68618, 68339, 68062, 67787,
    67514, 67243, 66975, 66709, 66444, 66182, 65922, 65536,
];
const L1: [i128; 128] = [
    0x00000000_00000000_00000000_00000000,
    0x017db6f5_f8c6e761_01d23283_8e36dddb,
    0x0279d748_68dc3e9e_024f5917_a5692857,
    0x037411c4_f8894358_67bf770a_51db6813,
    0x046c5762_4d3760ab_deafec2c_14a94550,
    0x056298eb_f23d3523_254e7f7c_7f38ea92,
    0x06570a43_b4333aa4_06965293_1260753b,
    0x0749e122_b6e9e485_be76add7_171e5229,
    0x083acc9a_cc727898_0dcfdde7_10629f07,
    0x092a0317_854a0b7f_285f9e2b_22a495c3,
    0x0a17779f_53790821_fc9521e0_d16cfff3,
    0x0b031d14_9c90f1c6_76b133b0_ed918e70,
    0x0bed2c76_be62f631_6b872b6d_92dd0eb7,
    0x0cd599e3_104e666b_fa474ed2_a7ccd855,
    0x0dbc5955_d8ea607d_a97e4f0c_a53046bb,
    0x0ea1a66b_3985ef27_b34f4c8a_d810e833,
    0x0f852e20_0a8a6366_553b8d10_a5440006,
    0x10672cd2_cc8d7e6f_decd5dc9_e6d417a0,
    0x11479784_6886ac87_02082d0a_58608333,
    0x12266319_0aeacb74_51f06802_6dfb8931,
    0x1303ce98_e4c812fb_618e55dc_bf60639c,
    0x13dfd030_ed7c69d7_692f33ce_15b63298,
    0x14ba12b3_9f77f462_0d364ffc_d99f3661,
    0x15932219_9a261595_f25d8251_b37dbaa5,
    0x166aa982_77ab5ff0_e71a9254_6299ff32,
    0x1740eb75_40ede4bc_e3003e2d_70647145,
    0x1815df21_46fa2b6a_49b504e5_c8a4ba5a,
    0x18e92dde_888640aa_ff4968f7_0d11f2c6,
    0x19bb1b6e_80c712f2_25e1718f_12bc7075,
    0x1a8bed7c_882f58c2_e7bb8792_56d4d5da,
    0x1b5b4d1e_8fc9e46e_107f739b_a66a2ce9,
    0x1c2980d5_8e7100ef_a07cc6f4_8c04d831,
    0x1cf6308e_09dc6c48_579ca3fb_a6e211e4,
    0x1dc1a3a0_ae5d7d0f_71cafccd_a9c51f92,
    0x1e8c2352_ae9b60a4_4be9f749_ae4f7bdf,
    0x1f550596_4b91c706_88f62de5_2f9a31a9,
    0x201ce54b_5b42c997_e6368e99_37c01563,
    0x20e368ff_9aee2a37_c16ab9d1_684d70d6,
    0x21a8dbaf_35879641_a1cb207f_a11b2de0,
    0x226d3685_2db9a217_b0e6e26b_7abe505c,
    0x23301e5b_a856dece_f3e2de7b_df9704bc,
    0x23f1df7c_9535f474_4e7cdd3e_2d12a1ce,
    0x24b2c821_bf2c73e0_bbd21c9b_ef08a50d,
    0x25727cf5_746689b3_dbe43626_5f7a95ba,
    0x2630f6d2_a9f59a1a_99b8b56f_73ffe421,
    0x26ee2e85_8343c554_7e6a604b_19fa08a0,
    0x27aa740b_8b3cad69_1021e822_4db585c9,
    0x2865c194_0d0ab6ef_71a3f95b_c579dee8,
    0x29201140_8025b198_674da571_4d4c456b,
    0x29d90464_c6e8d1dc_cc202a48_2faa709a,
    0x2a914607_5ba5059f_98ecb1e8_933d2830,
    0x2b4877e1_ee46040e_814f4c82_a0a2b61c,
    0x2bfe39a1_552c3655_f9472c50_ca764135,
    0x2cb393e6_0e54e66a_ad16d8e2_3432ebe7,
    0x2d6771c4_6b3cbb12_43c99c67_e2511f89,
    0x2e1a8304_f12bfbab_31250e69_81ef2542,
    0x2eccc2e4_79952eb1_5789e5a7_cae4245d,
    0x2f7dcfd4_70e58f64_5dc42dfe_972fb9c6,
    0x302e00bb_44016bb8_0650a728_f8cfd5cc,
    0x30dd50b4_6e2d81fe_955ee699_7a86eef7,
    0x318b5c90_b2d214eb_f106b4fc_051ca702,
    0x3238db56_8bd4bb17_7b7ecc54_0ea1ce57,
    0x32e56a42_2ba0c1c0_d97b1658_d6d0b1d1,
    0x33910445_c06bd73a_85f098ad_28be62c2,
    -0x247d67b2_38885124_d7a8c002_c5d6ea0d,
    -0x23d3c6cf_1123e8a8_eb70f826_47d6190d,
    -0x232ac8f7_3653b4a1_e4dc3875_456b8264,
    -0x2282d39e_6ce0f604_0776d3ef_1d571142,
    -0x21db89c1_6cf0015b_f4250e71_6eec1147,
    -0x213551e5_cd5ca852_ba0ee434_8b71790b,
    -0x208fce19_bd036f7c_4df90a59_c4cc3f3c,
    -0x1feb0233_e607cc5b_8cb57a31_c1fd98d3,
    -0x1f475653_4d7398e2_717fc52d_1171be9e,
    -0x1ea46b1e_fee0b5b4_6a4bddee_55bace20,
    -0x1e01df46_1b447355_38e44449_06f28344,
    -0x1d6080bf_8c0483d8_d4c13345_15cf4e97,
    -0x1cbfeec9_d9f85236_9451d5e0_88f7ff31,
    -0x1c202d6b_17e3247c_d58e32a9_cb35d541,
    -0x1b8140b0_b1699381_48ee6b57_a844f340,
    -0x1ae2c4ef_670d9469_f9551e2c_ac932955,
    -0x1a452502_e3b6c4cf_92d399f6_9eede286,
    -0x19a8650d_ea53f85c_255123fe_ad8012f2,
    -0x190c8939_f9c899ad_a2361ba0_4102bc57,
    -0x18712bf7_5076047c_19e1c4ba_84d774e3,
    -0x17d6ba3c_a876d167_19b04983_351b0ff3,
    -0x173ccd87_4e2d0cd8_584feebe_aa1a765b,
    -0x16a3689a_b259f0f7_5d7cbe97_ff8ca384,
    -0x160b65c0_a5b5e8d1_1534901d_17052221,
    -0x15738609_01374d57_f2a1601e_84daeb50,
    -0x14dca449_754538fb_d03f4ea4_0a26eb53,
    -0x1446579d_c223dd8f_56af7099_58254246,
    -0x13b0a2e7_55ca694f_1e827631_267ef031,
    -0x131b890d_394774b1_1875b89b_026f945f,
    -0x12877bbc_0b6ba5f4_6fd0b6dc_110bcdef,
    -0x11f3a0e5_f01130cb_e68f162f_bb8d42ef,
    -0x1160d982_49f570d0_9412a02a_d58f48c7,
    -0x10ce498d_c1aef993_479a8a79_cea1047d,
    -0x103cd40a_51ac0d4b_ce0a3383_47fbe4c9,
    -0x0fac0c3e_bfbeef48_88a1c766_544e5638,
    -0x0f1b8377_1823fda1_3972f004_708801b2,
    -0x0e8bad84_0d36a189_af58fc65_c108a8ed,
    -0x0dfc8d7b_4a077734_16611721_907721b4,
    -0x0d6e2677_2ba705f3_37ed437c_21b520b4,
    -0x0ce07b96_a3eea47b_3b5af559_0401e310,
    -0x0c531bbd_48ae145d_ad2320c5_0b797928,
    -0x0bc67d53_1eb359b5_32fb686f_eef64d25,
    -0x0b3aa384_93deae72_02fe7c50_462c139b,
    -0x0aaf1bc2_857bb1ff_6065ed1f_4dd80c27,
    -0x0a245e01_e54ed8c0_327ef5af_e1f657e5,
    -0x0999f6bc_0bac15bb_9dc47ede_dbba59fe,
    -0x09105eee_37d8bf85_f3286c43_96645061,
    -0x08872219_e573884b_6b4ae5cd_10f435d7,
    -0x07feba44_66e4c0cd_d098b154_e2232014,
    -0x0776b1f7_191303a7_5f8d35bf_27f01c29,
    -0x06ef0aff_113d05cf_2dd7d04a_1e10517e,
    -0x066840ed_58b69e2d_ac79626f_d56b1698,
    -0x05e1dcd6_8073aac0_368054e1_cb6b9405,
    -0x055c5b53_00e1abb9_76786d0a_21e67c22,
    -0x04d7447e_a3e9db82_7e8130e3_428f500e,
    -0x04529a38_fbc513fa_27ff53e5_bdc34022,
    -0x03ce5e65_024f8a2d_1bccc966_e0c7d208,
    -0x034a92e9_113e762f_b21f9b73_3e828632,
    -0x02c7b6ef_0cadeac5_b0fa8354_157325e8,
    -0x02455023_f3fd3754_35f60751_71c81863,
    -0x01c2e238_417ced5e_99ecaba4_c20e161d,
    -0x01416b1f_906aedb7_ed450d17_ed90ccb7,
    -0x00c06f10_9e6119dc_a101cdc8_071b6a2d,
    0x00000000_00000000_00000000_00000000,
];
const C2: [u64; 65] = [
    0x8102040810204081,
    0x80f9e42a1181ebb9,
    0x80f1c551f9b431dd,
    0x80e9a77f974227e8,
    0x80e18ab2b8c355d6,
    0x80d96eeb2cdbb2be,
    0x80d15428c23ba0e5,
    0x80c93a6b479fe9dd,
    0x80c121b28bd1ba98,
    0x80b909fe5da69f85,
    0x80b0f34e8c0080b1,
    0x80a8dda2e5cd9ddf,
    0x80a0c8fb3a088aad,
    0x8098b55757b82ab3,
    0x8090a2b70defada3,
    0x8088911a2bce8b74,
    0x8080808080808081,
    0x807870e9db3d89b1,
    0x807062560b49e0a5,
    0x806854c4dff5f7d9,
    0x80604836289e76d9,
    0x80583ca9b4ac3665,
    0x8050321f53943ca6,
    0x80482896d4d7b958,
    0x8040201008040201,
    0x8038188abcb28e1e,
    0x80301206c288f35b,
    0x80280c83e938e1c7,
    0x8020080200802008,
    0x80180480d8288799,
    0x8010020040080100,
    0x8008008008008008,
    0x8000000000000000,
    0x7ff8007ff8007ff8,
    0x7ff001ffc007ff00,
    0x7fe8047f28287869,
    0x7fe007fe007fe008,
    0x7fd80c7c19381e76,
    0x7fd011f942870d5b,
    0x7fc818754cae73ad,
    0x7fc01ff007fc01ff,
    0x7fb8286944c94ec4,
    0x7fb031e0d37bd29c,
    0x7fa83c568484e4a3,
    0x7fa047ca2861b6b7,
    0x7f98543b8f9b51ce,
    0x7f9061aa8ac69240,
    0x7f887016ea84241e,
    0x7f807f807f807f80,
    0x7f788fe71a73e4dd,
    0x7f70a14a8c22595b,
    0x7f68b3aaa55ba32e,
    0x7f60c70736fb45e9,
    0x7f58db6011e87eda,
    0x7f50f0b507164166,
    0x7f490705e7833366,
    0x7f411e528439a982,
    0x7f39369aae4fa390,
    0x7f314fde36e6c8f9,
    0x7f296a1cef2c6515,
    0x7f218556a8596392,
    0x7f19a18b33b24cd5,
    0x7f11beba62874264,
    0x7f09dce40633fb49,
    0x7f01fc07f01fc07f,
];
const L2: [i128; 65] = [
    -0x404055d6_2379c4a2_b316a0be_76803d23,
    -0x3e3c5e06_168bb008_a7605920_9273b38e,
    -0x3c3886b3_75b9f4e5_15bb6021_4ab9bf49,
    -0x3a34cfda_29cc3f4a_a9d0f201_6924a01b,
    -0x38313976_1c4ff550_14ef53f6_62879bf6,
    -0x362dc383_3798066c_ef7ee933_f74e280d,
    -0x342a6dfd_66bcb8cb_89b69c24_97ba16f4,
    -0x322738e0_959b7947_75a77a17_c37e4957,
    -0x30242428_b0d6a823_14b9f937_7a1d36b3,
    -0x2e212fd1_a5d567ea_eab9f8d0_fea164ff,
    -0x2c1e5bd7_62c36d09_d89122ea_6b3f11e3,
    -0x2a1ba835_d690cad2_ecd1ebbe_950871bf,
    -0x281914e8_f0f1c378_e838000c_4978f7c1,
    -0x2616a1ec_a25e963a_8045862d_bb21b269,
    -0x24144f3c_dc134d80_ba2727a9_bba49c5c,
    -0x22121cd5_900f8f6c_cc0b6962_e26cce69,
    -0x20100ab2_b1166b1c_731c67b1_b3b66f35,
    -0x1e0e18d0_32ae27de_7c4d9996_fc96b61f,
    -0x1c0c472a_09201599_1e31cacb_a0693e33,
    -0x1a0a95bc_297859a6_a21007c3_bb7ecfe1,
    -0x18090482_8985c060_ba70c0c4_fed91424,
    -0x16079379_1fd98a23_0d5ef1f4_3969141c,
    -0x1406429b_e3c73c3d_c09ba5bc_dc7b2146,
    -0x120511e6_cd646edd_ce04ad2f_a215e083,
    -0x10040155_d5889de6_e671eeec_0bfcefe5,
    -0x0e0310e4_f5ccf702_cd4f30db_ef38a804,
    -0x0c024090_288c2a23_173ac379_608053d6,
    -0x0a019053_68e238b7_a5f3f686_cf3d6bd5,
    -0x0801002a_b2ac4499_2be6bf0f_a435e838,
    -0x06009012_02886127_39a47536_02f13b92,
    -0x04004005_55d5621b_779c0dc1_0dddab4e,
    -0x02001000_aab2ab10_f666af8e_f8e871ba,
    0x00000000_00000000_00000000_00000000,
    0x01fff000_aaa2ab11_2bbc04dc_4e3dc0a9,
    0x03ffc005_54d56228_ccf15b16_6319670c,
    0x05ff7011_fd78613f_19a3a82d_fd2d01e4,
    0x07ff002a_a2ac43ee_81341464_9324d1d2,
    0x09fe7053_41d2362e_ef199cd0_6ae5d3aa,
    0x0bfdc08f_d78c229b_166dbb62_4f973978,
    0x0dfcf0e4_5fbce401_1be4cfbd_830393c2,
    0x0ffc0154_d588733c_73c742a7_c7635639,
    0x11faf1e5_3354185b_297dedf5_866d23d9,
    0x13f9c299_72c6998c_7641e42b_7a76b195,
    0x15f87375_8cc869d8_f47d459b_27e62d69,
    0x17f7047d_7983da66_4d686481_39ffb5b2,
    0x19f575b5_30654802_4e6c0ea3_c878f5f5,
    0x1bf3c720_a81b4bf9_cbcf6919_fdd70b55,
    0x1df1f8c3_d696e9f6_ca356245_dc84f9f8,
    0x1ff00aa2_b10bc06a_086b569b_4d4b76a2,
    0x21edfcc1_2bf036bb_df081e94_89493e39,
    0x23ebcf23_3afdadc8_7358572a_a814531d,
    0x25e981cc_d130ace1_4f125250_f509c857,
    0x27e714c1_e0c9129b_ce49ba4a_022f560d,
    0x29e48806_5b4a4325_bd189135_90fdfe9a,
    0x2be1db9e_317b56e2_be70d5ed_aff7e06e,
    0x2ddf0f8d_53674912_2587b82f_2d76c113,
    0x2fdc23d7_b05d273e_4b48e81d_13803d69,
    0x31d91881_36f03ff3_453e306f_9cdc3c92,
    0x33d5ed8d_d4f8503b_c3552f0c_50cf777a,
    0x35d2a301_7791b3a9_c5eaa55e_08c468a9,
    0x37cf38e0_0b1d91e3_6d808087_ea4ec78a,
    0x39cbaf2d_7b420dfa_6981617d_312ed81d,
    0x3bc805ed_b2ea73c7_ab721620_5990097c,
    0x3dc43d24_9c476752_27ef1ece_2c5ec807,
    0x3fc054d6_20cf11f8_eed20723_b8505a56,
];

#[cfg(test)]
mod tests {
    // Spec clause: §10.3 (x87 expl, exp2l, exp10l, expm1l, logl, log2l, log10l, log1pl correctly
    // rounded in the four directions, D-10.19).
    use super::*;
    use crate::float::F80;
    use crate::mp::{self, Mp};

    const MODES: [Rounding; 4] = [Rounding::Nearest, Rounding::Down, Rounding::Up, Rounding::Zero];
    const ALL: [Kernel; 8] = [
        Kernel::Exp,
        Kernel::Exp2,
        Kernel::Exp10,
        Kernel::Expm1,
        Kernel::Log,
        Kernel::Log2,
        Kernel::Log10,
        Kernel::Log1p,
    ];

    /// `|v|·2^units` rounded to the nearest integer (irrational or exactly representable values:
    /// no ties), with the sign of `v`.
    fn fixed(v: Mp, units: i64) -> i128 {
        let neg = v.to_f64() < 0.0;
        let w = v.abs().ldexp(units);
        let top = (u128::from(w.m[mp::LIMBS - 1]) << 64) | u128::from(w.m[mp::LIMBS - 2]);
        let sh = 128 - w.exp;
        assert!((0..128).contains(&sh), "{sh}");
        let r = if sh == 0 { top + u128::from(w.m[mp::LIMBS - 3] >> 63) } else { (top >> sh) + ((top >> (sh - 1)) & 1) };
        let r = i128::try_from(r).unwrap_or(i128::MAX);
        if neg { -r } else { r }
    }

    fn ufixed(v: Mp, units: i64) -> u128 {
        let w = v.ldexp(units);
        let top = (u128::from(w.m[mp::LIMBS - 1]) << 64) | u128::from(w.m[mp::LIMBS - 2]);
        let sh = 128 - w.exp;
        assert!((0..128).contains(&sh), "{sh}");
        if sh == 0 { top + u128::from(w.m[mp::LIMBS - 3] >> 63) } else { (top >> sh) + ((top >> (sh - 1)) & 1) }
    }

    #[test]
    fn constants_are_the_rounded_values() {
        let one = Mp::from_i64(1);
        let (ln2, ln10) = (mp::ln2(), mp::ln10());
        let mut fact = vec![1u64];
        for k in 1..=10u64 {
            fact.push(fact[fact.len() - 1] * k);
        }
        let inv = |k: usize| one.div_u64(fact[k]);
        let pinned: [(u128, Mp, i64); 18] = [
            (C_E, Mp::from_i64(128).div(ln2), 120),
            (C_10, Mp::from_i64(128).mul(ln10).div(ln2), 119),
            (L, ln2.ldexp(-7), 135),
            (E10.into(), inv(10), 85),
            (E9.into(), inv(9), 82),
            (E8.into(), inv(8), 79),
            (E7.into(), inv(7), 76),
            (E6.into(), inv(6), 73),
            (E5.into(), inv(5), 70),
            (E4, inv(4), 132),
            (E3, inv(3), 130),
            (LN2_113, ln2, 113),
            (LOG2E, one.div(ln2), 127),
            (LOG10E, one.div(ln10), 129),
            (G7.into(), one.div_u64(7), 66),
            (G6.into(), one.div_u64(6), 66),
            (G5.into(), one.div_u64(5), 66),
            (G3, one.div_u64(3), 129),
        ];
        for (i, (c, v, units)) in pinned.into_iter().enumerate() {
            assert_eq!(c, ufixed(v, units), "constant {i}");
        }
        for (j, &t) in EXP2T.iter().enumerate() {
            assert_eq!(t, ufixed(mp::exp(ln2.mul_u64(j as u64).ldexp(-7)), 127), "2^({j}/128)");
        }
        for i in 0..128usize {
            // c1' about 1/y'' at the interval's midpoint; 1 and 1/2 at the intervals around 1.
            // round(2^17/(1 + (i + 1/2)/128)) = round(2^25/(2i + 257)): no ties (odd divisor).
            let want = match i {
                0 => 1 << 17,
                127 => 1 << 16,
                _ => ((1u128 << 26) / (2 * i as u128 + 257) + 1) / 2,
            };
            let c = u128::from(C1[i]);
            assert_eq!(c, want, "C1[{i}]");
            let c1 = Mp::from_u128(false, c, if i >= 64 { -16 } else { -17 });
            assert_eq!(L1[i], fixed(mp::ln_atanh(c1).neg(), 127), "L1[{i}]");
        }
        for (ii, (&c, &l)) in C2.iter().zip(L2.iter()).enumerate() {
            let d = 4096 + ii as u128 - 32;
            assert_eq!(u128::from(c), ((1u128 << 64) * 4096 / d + 1) / 2, "C2[{ii}]");
            assert_eq!(l, fixed(mp::ln_atanh(Mp::from_u128(false, c.into(), -63)).neg(), 133), "L2[{ii}]");
        }
    }

    /// The exact value of `f(x)` to 384 bits.
    fn oracle(k: Kernel, n: &Num) -> Mp {
        let v = Mp::from_u128(n.neg, n.sig, i64::from(n.exp));
        match k {
            Kernel::Exp => mp::exp(v),
            Kernel::Exp2 => mp::exp(v.mul(mp::ln2())),
            Kernel::Exp10 => mp::exp(v.mul(mp::ln10())),
            Kernel::Expm1 => mp::exp(v).sub(Mp::from_i64(1)),
            Kernel::Log => mp::ln_atanh(v),
            Kernel::Log2 => mp::ln_atanh(v).div(mp::ln2()),
            Kernel::Log10 => mp::ln_atanh(v).div(mp::ln10()),
            Kernel::Log1p => mp::ln1p(v),
        }
    }

    struct Rng(u64);

    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
    }

    /// Bands of `floor(log2 |x|)` and the sign (`None`: either) per function: the whole range
    /// the tier takes, the band near the table's switch and the near-1 band of the logarithms.
    fn bands(k: Kernel) -> &'static [(i32, i32, Option<bool>)] {
        match k {
            Kernel::Exp | Kernel::Exp2 | Kernel::Expm1 => &[(-67, -10), (-9, -7), (-6, 4), (5, 13)],
            Kernel::Exp10 => &[(-68, -10), (-9, -7), (-6, 4), (5, 12)],
            Kernel::Log | Kernel::Log2 | Kernel::Log10 => &[(-1, 0), (-16445, 16383), (-3, 3), (-1, 0)],
            Kernel::Log1p => &[(-67, -8), (-7, -1), (-7, -1), (0, 16383)],
        }
    }

    fn operand(g: &mut Rng, k: Kernel, band: usize) -> Num {
        let (lo, hi, sign) = bands(k)[band];
        let top = lo + (g.next() % (hi - lo + 1) as u64) as i32;
        let mut sig = g.next() | 1 << 63;
        let is_log = matches!(k, Kernel::Log | Kernel::Log2 | Kernel::Log10);
        if is_log && band == 3 {
            // Within 2^-8 of 1 (top -1: below, top 0: above), down to 2^-63.
            let d = g.next() >> (8 + g.next() % 56);
            sig = if top == 0 { (1 << 63) + d } else { u64::MAX - d };
        }
        let neg = match sign {
            Some(s) => s,
            None => !is_log && !(k == Kernel::Log1p && top >= 0) && g.next() & 1 != 0,
        };
        Num { neg, kind: Kind::Finite, sig: sig.into(), exp: top - 63 }
    }

    #[derive(Default)]
    struct Tally {
        calls: u64,
        undecided: u64,
        refused: u64,
        worst: f64,
    }

    impl Tally {
        fn add(&mut self, o: &Tally) {
            self.calls += o.calls;
            self.undecided += o.undecided;
            self.refused += o.refused;
            self.worst = self.worst.max(o.worst);
        }
    }

    /// The tier on `n` against the 384-bit route in the four modes, and the kernel's error against
    /// the bound the rounding test assumes.
    fn check(k: Kernel, n: &Num, tally: &mut Tally) {
        let want = oracle(k, n);
        match kernel(k, n) {
            None => tally.refused += 1,
            Some((neg, m, e)) => {
                let rel = Mp::from_u128(neg, m, i64::from(e)).sub(want).div(want).to_f64().abs();
                assert!(rel < (-92f64).exp2(), "{k:?} {n:?}: error 2^{:.2}", rel.log2());
                tally.worst = tally.worst.max(rel);
            }
        }
        for mode in MODES {
            tally.calls += 1;
            match eval::<F80>(k, n, mode) {
                Some(got) => assert_eq!(got, mp::round::<F80>(want, mode), "{k:?} {n:?} {mode:?}"),
                None => tally.undecided += 1,
            }
        }
    }

    fn num(neg: bool, sig: u64, exp: i32) -> Num {
        Num { neg, kind: Kind::Finite, sig: sig.into(), exp }
    }

    /// Hard and edge operands: results within a hair of a representable value or a midpoint (`1
    /// ± 2^-j` near 1, `±2^-j` for `expm1` and `log1p`), both sides of the table switches and the
    /// limits of the ranges.
    fn hard(k: Kernel) -> Vec<Num> {
        let mut v = Vec::new();
        match k {
            Kernel::Log | Kernel::Log2 | Kernel::Log10 => {
                for j in 1..=63 {
                    v.push(num(false, (1 << 63) + (1 << (63 - j)), -63)); // 1 + 2^-j
                    v.push(num(false, u64::MAX << (64 - j).min(63), -64)); // 1 − 2^-j (and 1 − 2^-64·…)
                }
                for i in 0..128u64 {
                    let s = (1 << 63) + (i << 56); // 1 + i/128 and the value below it
                    v.push(num(false, s, -63));
                    v.push(num(false, s - 1, -63));
                }
                v.push(num(false, u64::MAX, 16320)); // largest
                v.push(num(false, 1, -16445)); // smallest subnormal
                v.push(num(false, (1 << 63) - 1, -16445)); // largest subnormal
            }
            Kernel::Log1p => {
                for j in 7..=67 {
                    v.push(num(false, 1 << 63, -63 - j));
                    v.push(num(true, 1 << 63, -63 - j));
                    v.push(num(true, u64::MAX, -64 - j));
                }
                v.push(num(true, u64::MAX, -64)); // −1 + 2^-64
                v.push(num(false, u64::MAX, 16320));
            }
            _ => {
                for j in 9..=67 {
                    v.push(num(false, 1 << 63, -63 - j));
                    v.push(num(true, 1 << 63, -63 - j));
                }
                // Both sides of ±ln2/256 (the expm1 switch) and ±ln2/128·(j + 1/2), j = 0..4.
                for h in 1..=9u64 {
                    let c = Mp::from_u128(false, h.into(), -8).mul(mp::ln2());
                    let c = if k == Kernel::Exp2 { Mp::from_u128(false, h.into(), -8) } else { c };
                    let c = if k == Kernel::Exp10 { c.div(mp::ln10()) } else { c };
                    let w = c.ldexp(70);
                    let s = u64::try_from(fixed(w, 0)).unwrap_or(1 << 63);
                    let l = s.leading_zeros();
                    for d in [0u64, 1, 2] {
                        for neg in [false, true] {
                            v.push(num(neg, (s << l) - d, -70 - l as i32));
                            v.push(num(neg, (s << l) + d, -70 - l as i32));
                        }
                    }
                }
                v.push(num(false, 0xb17217f7d1cf79ab, -50)); // 2^14·ln2·…: overflow (refused)
                v.push(num(true, 0xb17217f7d1cf79ab, -50));
            }
        }
        v
    }

    #[test]
    fn tier_agrees_with_the_wide_route() {
        let mut g = Rng(0x9e37_79b9_7f4a_7c15);
        for k in ALL {
            let mut tally = Tally::default();
            for band in 0..4 {
                for _ in 0..200 {
                    check(k, &operand(&mut g, k, band), &mut tally);
                }
            }
            let mut edge = Tally::default();
            for n in hard(k) {
                check(k, &n, &mut edge);
            }
            assert!(tally.undecided * 1000 <= tally.calls, "{k:?}: {} of {} undecided", tally.undecided, tally.calls);
            assert!(tally.refused == 0, "{k:?}: {} refused", tally.refused);
            eprintln!(
                "wide_int_el {k:?}: {} calls, {} undecided, worst 2^{:.1}; hard: {} calls, {} undecided, {} refused, worst 2^{:.1}",
                tally.calls,
                tally.undecided,
                tally.worst.log2(),
                edge.calls,
                edge.undecided,
                edge.refused,
                edge.worst.log2()
            );
        }
    }

    /// The rounding test refuses the results within a hair of a representable value: `ln(1 +
    /// 2^-63)` lies `2^-190` below `(2^64 − 1)·2^-127`, `e^(2^-60) − 1` just above `2^-60·(1 +
    /// 2^-61)`.
    #[test]
    fn hairline_results_are_refused() {
        for (k, n) in [
            (Kernel::Log, num(false, (1 << 63) + 1, -63)),
            (Kernel::Log1p, num(false, 1 << 63, -126)),
            (Kernel::Expm1, num(false, 1 << 63, -123)),
            (Kernel::Exp, num(true, 1 << 63, -123)),
        ] {
            for mode in MODES {
                assert!(eval::<F80>(k, &n, mode).is_none(), "{k:?} {n:?} {mode:?} decided");
            }
        }
    }

    /// The brief's sweep: 10^7 seeded operands per function over its four bands, in the four
    /// modes, and the hard operands. `cargo test --release -p liella-math --lib wide_int_el --
    /// --ignored`.
    #[test]
    #[ignore = "8 x 10^7 operands against the 384-bit route: run in release as a job"]
    fn tier_agrees_with_the_wide_route_long() {
        const TOTAL: usize = 10_000_000;
        let threads = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
        for k in ALL {
            let tallies: Vec<Tally> = std::thread::scope(|s| {
                let handles: Vec<_> = (0..threads)
                    .map(|i| {
                        s.spawn(move || {
                            let mut g = Rng(0x2545_f491_4f6c_dd1d ^ (i as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15));
                            let mut tally = Tally::default();
                            for j in (i..TOTAL).step_by(threads) {
                                check(k, &operand(&mut g, k, j % 4), &mut tally);
                            }
                            tally
                        })
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap_or_else(|p| std::panic::resume_unwind(p))).collect()
            });
            let mut all = Tally::default();
            for t in &tallies {
                all.add(t);
            }
            let mut edge = Tally::default();
            for n in hard(k) {
                check(k, &n, &mut edge);
            }
            eprintln!(
                "wide_int_el long {k:?}: {TOTAL} operands, {} calls, {} undecided, {} refused, worst 2^{:.2}; hard {} calls, {} undecided",
                all.calls,
                all.undecided,
                all.refused,
                all.worst.log2(),
                edge.calls,
                edge.undecided
            );
            assert!(all.undecided * 100_000 <= all.calls, "{k:?}: {} of {} undecided", all.undecided, all.calls);
        }
    }
}
