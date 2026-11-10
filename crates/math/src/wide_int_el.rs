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
//! Computer Arithmetic, 2010, ch. 4). Constants and tables from `cargo xtask tables` (400-bit
//! arithmetic), each re-derived from the 384-bit arithmetic by `constants_are_the_rounded_values`.
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
/// `2^-135` (below `2^126.6`), `rneg` the sign of `r`.
pub(crate) fn exp_table(k: i64, rneg: bool, p: u128) -> (u128, i32) {
    let t = EXP2T[(k & 127) as usize];
    let tp = mulhi(t, p) >> 7; // units 2^-134 -> 2^-127
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

/// Steps L1-L4: `ln(t·2^q)`, `t > 0` and `t·2^q != 1` (the caller returns the exact `ln 1 = 0`).
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
    let p = mulhi(w, h1) as i128; // |ln(1 + u2)|, units 2^-139
    let p = if v2 < 0 { -p } else { p };
    let (s, units) = if big_e == 0 {
        (l1 + (L2[ii] >> 6) + (p >> 12), -127)
    } else {
        (i128::from(big_e) * LN2_113 as i128 + (l1 >> 14) + (L2[ii] >> 20) + (p >> 26), -113)
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

// Constants and tables: `cargo xtask tables` (crates/mathtools/src/ld_explog_tables.rs), each
// rounded to nearest in the units noted there; `constants_are_the_rounded_values` re-derives them.
use crate::coeffs::tables::{
    WI_C1 as C1, WI_C2 as C2, WI_C_10 as C_10, WI_C_E as C_E, WI_E10 as E10, WI_E3 as E3, WI_E4 as E4, WI_E5 as E5,
    WI_E6 as E6, WI_E7 as E7, WI_E8 as E8, WI_E9 as E9, WI_EXP2T as EXP2T, WI_G3 as G3, WI_G5 as G5, WI_G6 as G6,
    WI_G7 as G7, WI_L as L, WI_L1 as L1, WI_L2 as L2, WI_LN2_113 as LN2_113, WI_LOG10E as LOG10E, WI_LOG2E as LOG2E,
};

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
        let neg = v.neg && !v.is_zero();
        let r = i128::try_from(ufixed(v.abs(), units)).unwrap_or(i128::MAX); // a pin's assert_eq catches MAX
        if neg { -r } else { r }
    }

    /// `|v|·2^units` rounded to the nearest integer (no ties: see [`fixed`]).
    fn ufixed(v: Mp, units: i64) -> u128 {
        if v.is_zero() {
            return 0;
        }
        let w = v.abs().ldexp(units);
        let top = (u128::from(w.m[mp::LIMBS - 1]) << 64) | u128::from(w.m[mp::LIMBS - 2]);
        let sh = 128 - w.exp;
        assert!((0..=128).contains(&sh), "{sh}");
        match sh {
            0 => top + u128::from(w.m[mp::LIMBS - 3] >> 63),
            128 => top >> 127,
            _ => (top >> sh) + ((top >> (sh - 1)) & 1),
        }
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
                _ => ((1u128 << 26) / (2 * i as u128 + 257)).div_ceil(2),
            };
            let c = u128::from(C1[i]);
            assert_eq!(c, want, "C1[{i}]");
            let c1 = Mp::from_u128(false, c, if i >= 64 { -16 } else { -17 });
            assert_eq!(L1[i], fixed(mp::ln_atanh(c1).neg(), 127), "L1[{i}]");
        }
        for (ii, (&c, &l)) in C2.iter().zip(L2.iter()).enumerate() {
            let d = 4096 + ii as u128 - 32;
            assert_eq!(u128::from(c), ((1u128 << 64) * 4096 / d).div_ceil(2), "C2[{ii}]");
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
            Kernel::Exp | Kernel::Exp2 | Kernel::Expm1 => {
                &[(-67, -10, None), (-9, -7, None), (-6, 4, None), (5, 13, None)]
            }
            Kernel::Exp10 => &[(-68, -10, None), (-9, -7, None), (-6, 4, None), (5, 12, None)],
            Kernel::Log | Kernel::Log2 | Kernel::Log10 => {
                &[(-1, 0, None), (-16445, 16383, None), (-3, 3, None), (-1, 0, None)]
            }
            Kernel::Log1p => &[
                (-67, -8, None),
                (-7, -1, Some(false)),
                (-7, -1, Some(true)),
                (0, 16383, Some(false)),
            ],
        }
    }

    fn operand(g: &mut Rng, k: Kernel, band: usize) -> Num {
        let (lo, hi, sign) = bands(k)[band];
        let top = lo + (g.next() % (hi - lo + 1) as u64) as i32;
        let mut sig = g.next() | 1 << 63;
        let is_log = matches!(k, Kernel::Log | Kernel::Log2 | Kernel::Log10);
        if is_log && band == 3 {
            // Within 2^-8 of 1 (top -1: `1 − d·2^-64` below, top 0: `1 + d·2^-63` above), down
            // to 2^-56, with at least 9 significant bits in the distance and an odd `d` on both
            // sides: a distance `d·2^-j` with `2·v2(d) >= log2(d)` (tiny `d`, powers of two) puts
            // `ln x = δ − δ²/2 + δ³/3 …` within the cube term of a representable value or
            // midpoint (hairline by structure; `hard()` holds those), which is not the rate this
            // band measures. (`u64::MAX − d` below 1 made the distance `d + 1`, even: 0.064% of
            // the long run's log calls were those hairlines.)
            let d = ((g.next() | 1 << 63) >> (8 + g.next() % 48)) | 1;
            sig = if top == 0 { (1 << 63) + d } else { d.wrapping_neg() };
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
        outside: u64,
        worst: f64,
    }

    impl Tally {
        fn add(&mut self, o: &Tally) {
            self.calls += o.calls;
            self.undecided += o.undecided;
            self.refused += o.refused;
            self.outside += o.outside;
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
        // Results outside the normal range (overflow, underflow, subnormal) belong to the later
        // tiers: the tier must refuse them, and they are not counted as undecided.
        // `wide::expm1` settles `x < −(64·0.7 + 6)` itself (−1 plus a tiny amount): never sent here.
        let settled = k == Kernel::Expm1 && Mp::from_u128(n.neg, n.sig, i64::from(n.exp)).to_f64() < -50.8;
        if settled {
            tally.outside += 1;
            return;
        }
        if !(-16381..=16384).contains(&want.exp) {
            tally.outside += 1;
            for mode in MODES {
                assert!(eval::<F80>(k, n, mode).is_none(), "{k:?} {n:?} {mode:?}: result outside the normal range taken");
            }
            return;
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
                    let s = (1 << 63) + (i << 56); // 1 + i/128 (not 1 itself) and the value below it
                    if i != 0 {
                        v.push(num(false, s, -63));
                    }
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
                "wide_int_el {k:?}: {} calls, {} undecided, {} outside the normal range, worst 2^{:.1}; hard: {} calls, {} undecided, {} refused, worst 2^{:.1}",
                tally.calls,
                tally.undecided,
                tally.outside,
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
            let tallies: Vec<[Tally; 4]> = std::thread::scope(|s| {
                let handles: Vec<_> = (0..threads)
                    .map(|i| {
                        s.spawn(move || {
                            let mut g = Rng(0x2545_f491_4f6c_dd1d ^ (i as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15));
                            let mut tally: [Tally; 4] = Default::default();
                            for j in (i..TOTAL).step_by(threads) {
                                check(k, &operand(&mut g, k, j % 4), &mut tally[j % 4]);
                            }
                            tally
                        })
                    })
                    .collect();
                handles.into_iter().map(|h| h.join().unwrap_or_else(|p| std::panic::resume_unwind(p))).collect()
            });
            let mut all = Tally::default();
            let mut band = [0u64; 4];
            for t in &tallies {
                for (b, tb) in t.iter().enumerate() {
                    all.add(tb);
                    band[b] += tb.undecided;
                }
            }
            let mut edge = Tally::default();
            for n in hard(k) {
                check(k, &n, &mut edge);
            }
            eprintln!(
                "wide_int_el long {k:?}: {TOTAL} operands, {} calls, {} undecided (by band {band:?}), {} refused, worst 2^{:.2}; hard {} calls, {} undecided",
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
