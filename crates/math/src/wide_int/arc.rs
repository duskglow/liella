//! The integer fixed-point first tier of the x87 `long double` `atan`, `atan2`, `asin`, `acos`
//! and `tan` (spec §10.3, D-10.52, D-10.75): each value is approximated in 64-bit limbs within
//! `2^-94` relative and rounded by the parent module's integer rounding test ([`super::round`],
//! bound `2^-92`); what the test cannot decide takes the 384-bit route of [`crate::wide`].
//!
//! Method. One arc kernel, `atan(y/x)` for `0 < y <= x` ([`atan_le`]): Tang's table method
//! (P. T. P. Tang, "Table-lookup algorithms for elementary functions and their error analysis",
//! ARITH 10, 1991) with `c = j/128`, `atan(y/x) = atan(c) + atan(r)`, `r = (y − cx)/(x + cy)`,
//! and the odd series of `atan r` in fixed point (R. Brent, P. Zimmermann, Modern Computer
//! Arithmetic, 2010, ch. 4). The quotient and the square root are a `double` estimate refined by
//! one exact integer residual (Newton's correction; Brent & Zimmermann ch. 3; J.-M. Muller,
//! Elementary Functions, 3rd ed. 2016, ch. 2): one `divsd` (the root seeded by a `double` root the caller passes:
//! the boundary's `sqrtsd`), never an integer `div`. `atan x = A(|x|, 1)` or `π/2 − A(1, |x|)`; `atan2` the same on `(|y|, |x|)`, then
//! `π − θ` for `x < 0`; `asin a = atan2(|a|, s)` and `acos a = atan2(s, |a|)` (`π −` that for
//! `a < 0`) with `s = √(1 − a²)`, `1 − a²` exact for `|a| >= 1/2`. `tan` takes the parent's
//! reduction and polynomials once and divides `sin|a|` by `cos|a|` (the other way round in odd
//! quadrants). Constants from Python `decimal` at 200 digits, each pinned against the 384-bit
//! arithmetic by `arc_constants_are_the_rounded_values`.
//!
//! Proof of the bounds (`ε` relative unless noted; the `double` operations may round in any
//! direction, each within `2^-52`, and every conversion to `double` here is exact: 53-bit integers).
//! 1. Quotient [`div`] of `n`, `d` in `[2^127, 2^128)`: `V = ⌊2^115/⌊d/2^75⌋⌋` is `2^126/(d/2^64)`
//!    within `2^-50.99` (`2^-52` the division, `2^-52` the truncated divisor, `2^-62` the integer
//!    part); `q0 = ⌊(n/2^64)·V/2^64⌋` is `(n/d)·2^62` within `2^-50.97`. The residual `E = n −
//!    q0·d/2^62` is formed exactly but for the floor of its last term (below one unit), and `Q =
//!    q0·2^64 + E·V/2^64`: its error is `E·(V/2^64 − 2^126/d)` (at most `2^-50.97·2^-50.99` of the
//!    quotient) plus two units: `Q` is `(n/d)·2^126` within `2^-101.9 + 2^-124 < 2^-101.8`.
//! 2. Square root [`sqrt`] of `w` in `[2^126, 2^128)`: `s0 = ⌊hw(⌊w/2^74⌋)·2^37⌋`, `hw` any
//!    `double` root within `2^-52` (the truncation of `w`, `2^-52`, and the conversion, `2^-52`, cost
//!    `2^-53` each under the root, the product's truncation `2^-63`): `s0` is `√w` within `ε <
//!    2^-51.0`, so `|w − s0²| < 2^78`; `V = 2^127/s0 = 2^90/r` (`r` the root of `w' = ⌊w/2^74⌋`) as
//!    `r·(2^90/w')`, one `divsd` of `w'` beside the root (kept as a `double` for step 3's index):
//!    `r²/w' = (1 + e)²` for `hw`'s `|e| <= 2^-52`, and the division and the product round `2^-53`
//!    each, so `V` is within `2^-49.6`; `S = s0·2^64 + (w − s0²)·V/2^64` (`w − s0²` exact). Newton's
//!    step leaves `(√w − s0)²/(2s0)`, `ε²/2 < 2^-103` relative, `V`'s error reaches `ε·2^-49.6 <
//!    2^-100.6`, two units more: `S` is `√w·2^64` within `2^-100.3` (below `2^128` since `w <= 2^128 −
//!    2^60`).
//! 3. Kernel `A(y, x)`, `t = y/x <= 1 + 2^-99`. `j = round(128t)` from a `double` estimate within
//!    `2^-48.7`: `y`'s top 53 bits (`2^-52`) times `x`'s index scale `256/top53(x)` (one `divsd` and the
//!    truncation, `2^-51`; for the root `s` of step 5 its seed's `2^127/s0` from step 2 scaled by a
//!    power of two: `2^-49.6` and `s0`'s `2^-51`), the product rounded (`2^-52`). Any `j` it gives
//!    keeps `|t − j/128| <= 2^-8(1 + 2^-40)` (`j <= 128` for `t` just above 1), so `|r| <= 2^-8(1 + 2^-40)` and
//!    `u = r² <= 2^-16(1 + 2^-39)`. `j = 0`: `r = t` (exact when `x` is a power of two, else
//!    step 1: `2^-101.8`). `j >= 1` (`t >= 2^-8(1 − 2^-40)`, so `y/x >= 2^-9` in exponents): `x`
//!    and `y` truncated to `2^-120` of `x`, the numerator `y − cx` and denominator `x + cy` exact
//!    on them (`c·v` is `j·v/128` truncated by one unit), `r` by step 1. A relative change `ε` of
//!    `t` changes `atan t` by at most `ε` relative (`t/(1 + t²) <= atan t`), so the truncations
//!    cost `2^-110`; `r`'s own `2^-101.8` and one unit of the shift give at most `2^-8·2^-101.8 +
//!    2^-127` absolute, and `atan(c)` (rounded, half a unit `2^-129`). With `A >= atan(2^-8(1 −
//!    2^-40)) > 2^-8.01`: within `2^-100.7`.
//! 4. Series. `atan r = r·P(u)`, `P(u) = Σ_{k<=6} (−1)^k u^k/(2k+1)`, remainder below
//!    `u^7/15 < 2^-115.9`. Horner from the innermost term, every partial sum positive: the steps of
//!    `1/11`, `1/9`, `1/7` in 64-bit words (two units each, reaching `P` times `u^5`, `u^4`, `u^3`:
//!    below `2^-113`), the last three in 128 bits (`2^-125`): `P` within `2^-112.6`, `r·P` within
//!    `2^-101.8 + 2^-112.6 + 2^-125 < 2^-101.7` of `atan r` given `r`.
//! 5. Compositions in units `2^-126`: `π/2 − A` (`A <= π/4`, result `>= π/4`) and `π − θ` (`θ <=
//!    π/2`, result `>= π/2`) add two units (`2^-124`) to `A`'s or `θ`'s relative error: every arc
//!    result is within `2^-100.6`. `s` (step 2 on `1 − a²`, exact or `2^-127.6` for `|a| < 1/2`)
//!    passes its `2^-100.3` into `t` and so (step 3) into the result: `asin`, `acos` within
//!    `2^-99.4`. Which of `a`, `s` is the numerator comes from `a` alone (`a < s` exactly when
//!    `a² < 1/2`, which never ties), so the index scale of `1/a` is ready before the root; at a
//!    64-bit `a`, `|a − s| > 2^-128`, below the root's `2^-100.3`: `t <= 1 + 2^-99`.
//! 6. `tan`: the parent's `sin|a|` and `cos|a|` (within `2^-94.8` and `2^-95.7` for `j >= 1`,
//!    `2^-96.9` and `2^-96.2` for `j = 0`, its module documentation) and step 1: within `2^-94.1`.
//!
//! Every approximation is within `2^-94`; the rounding test assumes `2^-92`. Results lie in
//! `[2^-36, 2^76]`, normal: the test's exponent check never refuses them.

use super::{cos_poly, mul_wide, mulhi, mulhi64, reduce, round, sin_poly, MIN_D, SC};
use crate::float::{Float, Kind, Num};
use crate::status::{Rounding, Status};

/// A positive value `m·2^e`, `m` in `[2^127, 2^128)`.
#[derive(Clone, Copy, Debug)]
struct Val {
    m: u128,
    e: i32,
}

const ONE: Val = Val { m: 1 << 127, e: -127 };
const TWO115: f64 = (1u128 << 115) as f64;
const TWO90: f64 = (1u128 << 90) as f64;
const TWO37: f64 = (1u64 << 37) as f64;
const TWO_M26: f64 = 1.0 / (1u64 << 26) as f64;
const TWO_M108: f64 = 1.0 / (1u128 << 108) as f64;
const ONE_IX: f64 = 1.0 / (1u64 << 44) as f64; // 256/top53(1) = 256/2^52

const PIO2: u128 = 0x6487ed51_10b4611a_62633145_c06e0e69; // π/2, units 2^-126
const PI: u128 = 0xc90fdaa2_2168c234_c4c6628b_80dc1cd1; // π, units 2^-126
const A13: u64 = 0x9d89d89d89d89d8a; // 1/13, units 2^-67
const A11: u64 = 0xba2e8ba2e8ba2e8c; // 1/11, units 2^-67
const A9: u64 = 0xe38e38e38e38e38e; // 1/9, units 2^-67
const A7: u64 = 0x9249249249249249; // 1/7, units 2^-66
const A5: u128 = 0xcccccccc_cccccccc_cccccccc_cccccccd; // 1/5, units 2^-130
const A3: u128 = 0xaaaaaaaa_aaaaaaaa_aaaaaaaa_aaaaaaab; // 1/3, units 2^-129

/// `atan x` rounded into `F` in `mode` when the tier decides it; `None` sends the caller to the
/// 384-bit route. `x` finite and nonzero (the caller settles `|x| < 2^-34`).
pub(crate) fn atan<F: Float>(n: &Num, mode: Rounding) -> Option<(F, Status)> {
    if F::PREC == 113 {
        return super::quad::arc::atan::<F>(n, mode); // binary128: three limbs (D-10.96)
    }
    finish::<F>(|| atan_approx(n), mode)
}

/// `atan2(y, x)` for finite nonzero `y`, `x` (the caller settles zeros, infinities and
/// `|y/x| < 2^-72` with `x > 0`).
pub(crate) fn atan2<F: Float>(y: &Num, x: &Num, mode: Rounding) -> Option<(F, Status)> {
    if F::PREC == 113 {
        return super::quad::arc::atan2::<F>(y, x, mode); // binary128: three limbs (D-10.96)
    }
    finish::<F>(|| atan2_approx(y, x), mode)
}

/// `asin a` for `0 < |a| < 1` (other operands give `None`); `hw` is a `double` square root
/// (step 2: [`soft_sqrt`] or the boundary's `sqrtsd`).
pub(crate) fn asin<F: Float>(n: &Num, mode: Rounding, hw: impl Fn(f64) -> f64) -> Option<(F, Status)> {
    finish::<F>(|| asin_approx(n, hw), mode)
}

/// `acos a` for `0 < |a| < 1` (other operands give `None`); `hw` as for [`asin`].
pub(crate) fn acos<F: Float>(n: &Num, mode: Rounding, hw: impl Fn(f64) -> f64) -> Option<(F, Status)> {
    finish::<F>(|| acos_approx(n, hw), mode)
}

/// `tan x` for `|x| >= 2^-34` finite.
pub(crate) fn tan<F: Float>(n: &Num, mode: Rounding) -> Option<(F, Status)> {
    if F::PREC == 113 {
        return super::quad::tan::<F>(n, mode); // binary128: three limbs (D-10.91)
    }
    finish::<F>(|| tan_approx(n), mode)
}

fn finish<F: Float>(r: impl FnOnce() -> Option<(bool, Val)>, mode: Rounding) -> Option<(F, Status)> {
    if F::PREC != 64 || !F::EXPLICIT {
        return None; // other formats never compute an approximation
    }
    let (neg, v) = r()?;
    round::<F>(neg, v.m, v.e, mode)
}

/// The magnitude of a finite nonzero x87 operand.
fn val(n: &Num) -> Option<Val> {
    if n.kind != Kind::Finite || n.sig == 0 || n.sig >> 64 != 0 {
        return None;
    }
    let lz = n.sig.leading_zeros(); // sig < 2^64: lz >= 64
    Some(Val { m: n.sig << lz, e: n.exp - lz as i32 })
}

/// `v·2^-units` (`v` nonzero) as a [`Val`].
fn norm(v: u128, units: i32) -> Val {
    let l = v.leading_zeros();
    Val { m: v << l, e: -units - l as i32 }
}

/// `c·2^-126 − v` for `v < c·2^-126` (step 5).
fn sub_from(c: u128, v: Val) -> Val {
    let f = v.m.checked_shr((-126 - v.e) as u32).unwrap_or(0); // v <= π/2: e <= -127
    norm(c - f, 126)
}

fn dbl(v: u64) -> f64 {
    v as i64 as f64 // below 2^53 at every call: exact
}

/// Step 1: `(n/d)·2^126` for `n`, `d` in `[2^127, 2^128)`, in `(2^125, 2^127)`.
fn div(n: u128, d: u128) -> u128 {
    let dh = (d >> 64) as u64;
    let v = (TWO115 / dbl(dh >> 11)) as u64; // ≈ 2^126/dh, in (2^62, 2^63]
    let q0 = mulhi64((n >> 64) as u64, v); // ≈ (n/d)·2^62
    let t = ((u128::from(q0) * u128::from(dh)) << 2).wrapping_add((u128::from(q0) * (d & super::M64)) >> 62);
    let e = n.wrapping_sub(t) as i128; // n − q0·d/2^62, |e| < 2^78
    let c = mul_wide(e.unsigned_abs(), v);
    let q = u128::from(q0) << 64;
    if e < 0 { q - c } else { q + c }
}

/// A `double` square root for operands in `[2^52, 2^54)` within `2^-52` (the integer root of
/// `v·2^52`, below `2^53`, scaled): `hw` for this crate's own callers. The crate cannot issue
/// `sqrtsd` (no `unsafe`, no `f64::sqrt` in `core`); the boundary passes it in (D-10.75).
pub(crate) fn soft_sqrt(v: f64) -> f64 {
    let n = (v as u64 as u128) << 52; // below 2^106
    dbl(n.isqrt() as u64) * TWO_M26
}

/// `s0 ≈ √w` within `2^-51.19` and `2^127/s0` as a `double` (step 2): `r·(2^90/w')`, whose
/// `divsd` needs only `w'` and so runs beside the root, not after it.
fn root_seed(w: u128, hw: impl Fn(f64) -> f64) -> (u64, f64) {
    let wd = (w >> 74) as i64 as f64; // w' = ⌊w/2^74⌋, in [2^52, 2^54)
    let iw = TWO90 / wd;
    let r = hw(wd); // √w', in [2^26, 2^27]
    let s0 = big_u64(r * TWO37); // 2^64 − 1 when the root rounds to 2^27 (w just below 2^128)
    (s0, r * iw) // 2^90/r = 2^127/s0
}

/// `v as u64` for an integer-valued `v` in `[2^52, 2^64]` (`2^64` gives `2^64 − 1`, as `as` saturates),
/// read from the bits: no SSE conversion. `as u64` of a value at or above `2^63` converts it
/// signed first, which overflows and raises the invalid flag in MXCSR on every call; the
/// envelope's exit drops that flag again with an MXCSR write, and on Ivy Bridge the next call's
/// MXCSR read then stalls in microcode (LIBM-LD-ARC-RETHINK-2's attribution: ~58 microcode uops a
/// call, successive asinl calls no longer overlapping). The root's seed `s0` and its reciprocal
/// lie in `[2^63, 2^64]` on every call.
#[inline(always)]
fn big_u64(v: f64) -> u64 {
    let b = v.to_bits();
    let sh = ((b >> 52) as u32).wrapping_sub(1075); // v = m·2^sh, sh in [0, 12]
    let m = (b & ((1 << 52) - 1)) | (1 << 52);
    if sh >= 12 { u64::MAX } else { m << sh }
}

/// Step 2: `√w·2^64` for `w` in `[2^126, 2^128 − 2^60]` (no overflow of the result), and the
/// seed's reciprocal `2^127/s0` (a `double` within `2^-50.99`; step 3 scales it into its index).
fn sqrt(w: u128, hw: impl Fn(f64) -> f64) -> (u128, f64) {
    let (s0, vd) = root_seed(w, hw);
    let r = w.wrapping_sub(u128::from(s0) * u128::from(s0)) as i128; // w − s0², |r| < 2^78
    let c = mul_wide(r.unsigned_abs(), big_u64(vd)); // r·2^63/s0 (vd in [2^63, 2^64])
    let s = u128::from(s0) << 64;
    (if r < 0 { s - c } else { s + c }, vd)
}

/// Step 4: `P(u)` in units of `2^-127` (`u` = r² in units of `2^-142`, `u1` its top word).
#[inline]
fn atan_poly(u: u128, u1: u64) -> u128 {
    let a = A11 - (mulhi64(u1, A13) >> 14); // units 2^-67
    let b = A9 - (mulhi64(u1, a) >> 14); // units 2^-67
    let c = A7 - (mulhi64(u1, b) >> 15); // units 2^-66
    let d = A5 - (mul_wide(u, c) >> 14); // units 2^-130
    let e = A3 - (mulhi(u, d) >> 15); // units 2^-129
    (1 << 127) - (mulhi(u, e) >> 16)
}

/// `atan|r|·2^(127 + kr)` for `|r| = rm·2^(-128-kr)`, `rm` in `[2^127, 2^128)`, `kr >= 7`.
/// Inlined: one of its sites runs per call, and the call cost atanl its register saves.
#[inline(always)]
fn atan_small(rm: u128, kr: i32) -> u128 {
    let u = mulhi(rm, rm).checked_shr((2 * kr - 14) as u32).unwrap_or(0); // r², units 2^-142
    mulhi(rm, atan_poly(u, (u >> 64) as u64))
}

/// Step 3's index scale `256/x` from `x`'s top 53 bits (one `divsd`, within `2^-51`).
fn inv_top(x: Val) -> f64 {
    256.0 / dbl((x.m >> 75) as u64)
}

/// Step 3: `atan(y/x)` for `0 < y/x <= 1 + 2^-100` (the shared copy: atan2l, asinl, acosl).
/// `ix` is `x`'s index scale ([`inv_top`], or any value within `2^-48.7` of it).
fn atan_le(y: Val, x: Val, ix: f64) -> Val {
    atan_le_in(y, x, || ix)
}

/// [`atan_le`]'s body, inlined into `atanl` so its constant `x = 1` (or `y = 1`) folds: the
/// `x.m == 2^127` test, the table index's scale and the call. `ix` is read only for `k <= 8`.
#[inline(always)]
fn atan_le_in(y: Val, x: Val, ix: impl FnOnce() -> f64) -> Val {
    let k = x.e - y.e; // >= 0
    let mut j = 0;
    if k <= 8 {
        let h = (dbl((y.m >> 75) as u64) * ix()) as usize; // ⌊256·y.m/x.m⌋ within one
        j = ((h >> k) + 1) >> 1;
    }
    if j == 0 {
        let (rm, kr) = if x.m == 1 << 127 {
            (y.m, k - 1)
        } else {
            let q = div(y.m, x.m);
            let l = q.leading_zeros();
            (q << l, k + l as i32 - 2)
        };
        let p = atan_small(rm, kr);
        let l = p.leading_zeros();
        return Val { m: p << l, e: -127 - kr - l as i32 };
    }
    let xv = x.m >> 8;
    let yv = y.m >> (8 + k);
    let num = yv as i128 - ((j as u128 * xv) >> 7) as i128; // y − cx
    let den = xv + ((j as u128 * yv) >> 7); // x + cy
    let tj = ATAN_J[j - 1]; // units 2^-128
    let a = if num == 0 {
        tj
    } else {
        let an = num.unsigned_abs();
        let (ln, ld) = (an.leading_zeros(), den.leading_zeros());
        let q = div(an << ln, den << ld);
        let l = q.leading_zeros();
        let kr = (l + ln) as i32 - ld as i32 - 2;
        let p = atan_small(q << l, kr).checked_shr((kr - 1) as u32).unwrap_or(0); // units 2^-128
        if num < 0 { tj - p } else { tj + p }
    };
    norm(a, 128)
}

/// `atan2(y, x)` for positive `y`, `x`: in `(0, π/2)`.
fn atan2_pos(y: Val, x: Val) -> Val {
    if (y.e, y.m) <= (x.e, x.m) { atan_le(y, x, inv_top(x)) } else { sub_from(PIO2, atan_le(x, y, inv_top(y))) }
}

fn atan_approx(n: &Num) -> Option<(bool, Val)> {
    let y = val(n)?;
    Some((n.neg, if (y.e, y.m) <= (ONE.e, ONE.m) { atan_le_in(y, ONE, || ONE_IX) } else { sub_from(PIO2, atan_le_in(ONE, y, || inv_top(y))) }))
}

fn atan2_approx(b: &Num, a: &Num) -> Option<(bool, Val)> {
    let t = atan2_pos(val(b)?, val(a)?);
    Some((b.neg, if a.neg { sub_from(PI, t) } else { t }))
}

/// `√(1 − a²)` for `0 < a < 1` (step 5), its index scale ([`inv_top`] from the root's own
/// reciprocal: no `divsd` after the root) and whether `a < √(1 − a²)`. Inlined into its two
/// callers (asinl, acosl): out of line it costs a call and the spills around it.
#[inline(always)]
fn root_one_minus_sq(a: Val, hw: impl Fn(f64) -> f64) -> Option<(Val, f64, bool)> {
    if a.e > -128 {
        return None; // a >= 1
    }
    let m = a.m >> 64; // the 64-bit significand: a = m·2^(e + 64)
    let sh = (-2 * a.e - 256) as u32; // a² = m²·2^-(128 + sh)
    let v = (m * m).checked_shr(sh).unwrap_or(0).max(1 << 60); // a² in units 2^-128 (exact for a >= 1/2)
    // a² < 1/2 (exact for a >= 1/2; a² = 1/2 has no solution): a < √(1 − a²).
    let below = v < 1 << 127;
    let w = v.wrapping_neg(); // 1 − a²
    let lw = w.leading_zeros() & !1;
    let (s, vd) = sqrt(w << lw, hw); // √(1 − a²)·2^(128 + lw/2)
    let l = s.leading_zeros(); // 0 or 1
    // top53(s << l) = s0·2^(l − 11) within 2^-51, so 256/top53 = vd·2^(−108 − l) (step 3).
    let ix = vd * if l == 0 { TWO_M108 } else { TWO_M108 / 2.0 };
    Some((Val { m: s << l, e: -128 - (lw / 2) as i32 - l as i32 }, ix, below))
}

/// `asin a = atan2(a, s)`: `A(a, s)` when `a < s`, else `π/2 − A(s, a)` (`1/a` needs no root).
fn asin_approx(n: &Num, hw: impl Fn(f64) -> f64) -> Option<(bool, Val)> {
    let a = val(n)?;
    let ia = inv_top(a);
    let (s, is, below) = root_one_minus_sq(a, hw)?;
    Some((n.neg, if below { atan_le(a, s, is) } else { sub_from(PIO2, atan_le(s, a, ia)) }))
}

/// `acos a = atan2(s, |a|)` the same way, then `π −` that for `a < 0`.
fn acos_approx(n: &Num, hw: impl Fn(f64) -> f64) -> Option<(bool, Val)> {
    let a = val(n)?;
    let ia = inv_top(a);
    let (s, is, below) = root_one_minus_sq(a, hw)?;
    let t = if below { sub_from(PIO2, atan_le(a, s, is)) } else { atan_le(s, a, ia) };
    Some((false, if n.neg { sub_from(PI, t) } else { t }))
}

/// Step 6: `tan x` from one reduction (the parent's steps 1 to 4 for both `sin|a|` and `cos|a|`).
fn tan_approx(n: &Num) -> Option<(bool, Val)> {
    if n.kind != Kind::Finite || n.sig == 0 || n.sig >> 64 != 0 {
        return None;
    }
    let lz = (n.sig as u64).leading_zeros();
    let m = (n.sig as u64) << lz;
    let e = n.exp - lz as i32;
    if !(-97..=16320).contains(&e) {
        return None;
    }
    let (q, negf, gh, gl) = reduce(m, e);
    // tan|x| = tan a (q even) or −1/tan a (q odd), a = fπ/2 of f's sign.
    let odd = q & 1 != 0;
    let j = ((gh + (1 << 120)) >> 121) as usize;
    let dh = gh.wrapping_sub((j as u128) << 121);
    let (sd, dh, dl) = if (dh as i128) < 0 {
        let lo = gl.wrapping_neg();
        (true, (!dh).wrapping_add(u128::from(lo == 0)), lo)
    } else {
        (false, dh, gl)
    };
    if dh < MIN_D {
        return None;
    }
    let kd = dh.leading_zeros();
    let d = (dh << kd) | (dl >> (128 - kd));
    let u = mulhi(d, d).checked_shr(2 * kd - 14).unwrap_or(0);
    let u1 = (u >> 64) as u64;
    let (s, c) = if j == 0 {
        (norm(mulhi(d, sin_poly(u, u1)), 126 + kd as i32), norm(cos_poly(u, u1), 127))
    } else {
        let sp = mulhi(d, sin_poly(u, u1)) >> (kd - 7);
        let cp = cos_poly(u, u1);
        let [sj, cj] = SC[j - 1];
        let (a, b) = (mulhi(sj, cp), mulhi(cj, sp) >> 6);
        let sn = if sd { a - b } else { a + b };
        let (a, b) = (mulhi(cj, cp), mulhi(sj, sp) >> 6);
        let cs = if sd { a + b } else { a - b };
        (norm(sn, 127), norm(cs, 127))
    };
    let (top, bot) = if odd { (c, s) } else { (s, c) };
    let r = norm(div(top.m, bot.m), 126);
    Some((n.neg ^ negf ^ odd, Val { m: r.m, e: r.e + top.e - bot.e }))
}

/// `atan(j/128)` for `j = 1..=128` (row `j - 1`), in units of `2^-128`.
pub(super) const ATAN_J: [u128; 128] = [
    0x01fffd55_5bbba972_d00c46a3_f77cc15f,
    0x03ffeaab_776e5356_ef9e3159_0057dd81,
    0x05ffb806_12970d6b_ce7603f5_cab5251c,
    0x07ff556e_ea5d892a_13bcebbb_6ed46311,
    0x09feb2f8_b4e4ec8b_918538b2_30da72f6,
    0x0bfdc0c2_186d14fc_f220e10d_61df56ec,
    0x0dfc6ef8_9ce221ce_c03969ab_00b0ebad,
    0x0ffaaddb_967ef4e3_6cb2792d_c0e2e0d5,
    0x11f86dbf_082d58de_d447119a_20fdb0fe,
    0x13f59f0e_7c559d6b_1338a177_e11cd9bf,
    0x15f2324f_d2d7b262_a3691004_c637eace,
    0x17ee1826_02f10e8c_126acfcf_099f06cf,
    0x19e94153_cfdcf168_ccb875a7_11f8151f,
    0x1be39ebe_6f07c37d_ee3ca681_661cbb3e,
    0x1ddd2170_1eba6e65_3bff35db_f95fab5a,
    0x1fd5ba9a_ac2f6dc6_5912f313_e7d111df,
    0x21cd5b99_e8110314_72e72233_97963968,
    0x23c3f5f6_086e4dc9_6f4dd64a_60e82be6,
    0x25b97b65_f743fe63_ca1c1558_54d14fcb,
    0x27adddd1_8cc4d8b0_d1d86749_40d83fa1,
    0x29a10f53_b49e2e8f_991f8d6f_8abcc1f8,
    0x2b93023c_7d84d3be_ad534ffb_c30b7a65,
    0x2d83a913_1267b1b5_aad627f0_ef6549a1,
    0x2f72f697_9cb6044d_1ec2d3e2_07271d22,
    0x3160ddc5_0f385177_39967e7b_9d85f2c6,
    0x334d51d2_d90c4c39_ec03cf68_691bbacf,
    0x35384636_8064f287_45df44e6_542a8758,
    0x3721aea5_24c14408_bd886970_72d54bc1,
    0x39097f14_e85cdb9a_908be5f4_5766d522,
    0x3aefabbe_40ae6ce3_2468a9a2_cbef5e3a,
    0x3cd4291d_2dd89ac3_3591014f_bb281a07,
    0x3eb6ebf2_5901bac5_5b71e7bd_7de885f9,
    0x4097e944_1996d698_d2097cd0_5b85a52f,
    0x4277165f_618d8962_e47390cb_8655e9d1,
    0x445468d8_90c726b2_37236f88_4613c81b,
    0x462fd68c_2fc5e098_6523a458_dfc414c7,
    0x4809559f_91f25773_e6e6b85e_b78ccc12,
    0x49e0dc81_5fbd16f8_8322c920_37f0a23d,
    0x4bb661ea_08f3f8dc_892c7500_964d0de3,
    0x4d89dcdc_1faf2f34_e2d5da4c_693d7994,
    0x4f5b44a4_9c44d113_7ca41cc9_589e8ce2,
    0x512a90db_0abc26a2_a1bc3aa4_c45c6cf2,
    0x52f7b961_a2439b0d_91c41eea_c549d5fa,
    0x54c2b665_4735276d_4cdbfbbd_fbecf461,
    0x568b805d_783d3913_b7a8f82e_45741deb,
    0x5852100c_273f8658_da8ea8ee_100507e1,
    0x5a165e7d_7e9bf7db_50fd65ca_4ac7b49b,
    0x5bd86507_937bc239_c5519091_6e7f2242,
    0x5d981d4a_05d407c4_5dd0dc67_13fffa14,
    0x5f55812d_8ecfdd69_c885c2b2_49a08813,
    0x61108ae3_7e575dd7_6a0299b4_1b5c3a3c,
    0x62c934e5_286c95b6_d0ba3748_fa85146f,
    0x647f79f3_43198910_74188054_b536bec6,
    0x66335515_35ac619e_6c988fd0_a76cdbe2,
    0x67e4c198_5a000637_d8fb836b_c6581a7c,
    0x6993bb0f_308ff2db_213e4af4_800f389b,
    0x6b403d50_88162dfc_4c33891d_2e7eef7e,
    0x6cea4476_9971b1ae_187b1ca5_04031a2f,
    0x6e91ccde_18929551_9a1b46e4_afe929cf,
    0x7036d325_3b27be33_e318f6cb_3cc65c02,
    0x71d9542a_b5c7e28b_474008fb_c1b87ea8,
    0x73794d0c_b04d425d_305bbe70_e536e164,
    0x7516bb27_b218acc4_a108b1f7_c969a4a3,
    0x76b19c15_86ed3da2_b7f222f6_5e1d4682,
    0x7849edac_1d12bfb5_3d9e719c_ad55185c,
    0x79dfadfc_5d68d10e_53dc1bf3_4356f9fd,
    0x7b72db50_fe10d380_da2733dd_ec71326e,
    0x7d03742d_50505f2e_33691e3e_aee47661,
    0x7e91774c_0a496235_a9c23d72_74b5310f,
    0x801ce39e_0d205c99_a6d6c6c5_4d938596,
    0x81a5b849_2824418f_9b38d855_40ff5432,
    0x832bf4a6_d9867e2a_4b6a09cb_61a515c1,
    0x84af9843_0d2c7eed_e4df5ea5_60d2aaeb,
    0x8630a2da_da1ed065_d3e84ed5_013ca37e,
    0x87af145b_3f14a800_988d8bf6_4c868143,
    0x892aecdf_de9547b5_094478fc_472b4afc,
    0x8aa42cb1_bb234d68_b0756e81_879e843a,
    0x8c1ad445_f3e09b8c_439d8018_60205921,
    0x8d8ee43c_8214276f_0b9ba883_86b8a2cd,
    0x8f005d5e_f7f59f9b_5c835e16_65c43748,
    0x906f409f_411d8d0d_c5158289_17850ab6,
    0x91db8f16_64f350e2_10e4f9c1_126e0220,
    0x93454a03_4b6d3074_4d228131_cf31c982,
    0x94ac72c9_847186f6_18c4f393_f78a32f9,
    0x96110af0_12232fd6_8fde4b8b_683e2686,
    0x97731420_365e538b_abd3fe19_f1aeb6b3,
    0x98d29024_43a5f281_9570f4f5_84892a43,
    0x9a2f80e6_71bdda20_4226f8e2_204ff3bd,
    0x9b89e86f_b6281fac_21d87f3a_7e4b6dd4,
    0x9ce1c8e6_a0b8cdb9_f799c4e8_174cf11c,
    0x9e37248e_3c6e243d_75ed605e_8b128b9c,
    0x9f89fdc4_f4b7a1ec_f8b49264_4f0701e0,
    0xa0da5703_7f52089e_b052993b_b28ec568,
    0xa22832db_cadaae08_92fe9c08_637af0e6,
    0xa37393f7_f238af63_232723dd_99b9efd4,
    0xa4bc7d19_34f70924_19a87f2a_457dac9f,
    0xa602f116_f4a7247e_a7c1ec1a_242451d4,
    0xa746f2dd_b7602294_67b7d66f_2d74e019,
    0xa888856e_2f6c0923_dcd6832a_63de1ee9,
    0xa9c7abdc_4830f5c8_916a84b5_be7933f6,
    0xab04694e_3861a332_739e1bd0_11000fd5,
    0xac3ec0fb_997dd6a1_a36273a5_6afa8ef4,
    0xad76b62c_84a8bae6_25ee2952_86cdb591,
    0xaeac4c38_b4d8c080_14725e2f_3e52070a,
    0xafdf8686_ae624f92_cc0bffd2_3aa0ae62,
    0xb110688a_ebdc6f6a_43d65788_b9f6a7b5,
    0xb23ef5c7_105c7f84_d7eff716_b8c33243,
    0xb36b31c9_1f043691_59014174_4462f93a,
    0xb495202a_b7db53b2_972b51d4_5971ca78,
    0xb5bcc490_59ecc4af_f8f3cee7_5e3907d5,
    0xb6e222a8_aa9d7790_1de72a31_f5c86498,
    0xb8053e2b_c2319e73_cb2da552_10a4443d,
    0xb9261ada_7d73d865_22ccb6cd_266bc460,
    0xba44bc7d_d470782f_654c2cb1_0942e386,
    0xbb6126e6_36360232_a4aa35b9_07915ae6,
    0xbc7b5dea_e98af280_d4113006_e80fb290,
    0xbd936569_7287ec62_fe419f7d_c09b1792,
    0xbea94144_fd049aac_1043c5e7_55282e7d,
    0xbfbcf565_cbc4c3fa_b9501d5a_441a24b6,
    0xc0ce85b8_ac526640_89dd62c4_6e92fa25,
    0xc1ddf62e_6f711098_838827fe_361448e7,
    0xc2eb4abb_661628b5_b373fe45_c61bb9fb,
    0xc3f68756_e2d1510a_f5ad957f_4bc61566,
    0xc4ffaffa_bf8fbd54_8cb43d10_bc9e0221,
    0xc606c8a2_e7a4eb7d_480edb88_66516455,
    0xc70bd54c_e602ee13_e7d54fbd_09f2be38,
    0xc80ed9f7_778c3ad0_ccbcac07_91ef71d4,
    0xc90fdaa2_2168c234_c4c6628b_80dc1cd1,
];

#[cfg(test)]
mod tests {
    // Spec clause: §10.3 (x87 atanl, atan2l, asinl, acosl, tanl correctly rounded in the four
    // directions, D-10.19).
    use super::*;
    use crate::float::F80;
    use crate::mp::{self, Mp};

    const MODES: [Rounding; 4] = [Rounding::Nearest, Rounding::Down, Rounding::Up, Rounding::Zero];
    /// The proved bounds (module documentation): arc functions `2^-99.9`, `tan` `2^-94.1`.
    fn bound(f: Fun) -> f64 {
        // The module documentation's steps 5 and 6.
        2f64.powf(match f {
            Fun::Tan => -94.1,
            Fun::Asin | Fun::Acos => -100.1,
            Fun::Atan | Fun::Atan2 => -100.6,
        })
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

    /// `v·2^units` rounded to the nearest integer.
    fn fixed(v: Mp, units: i64) -> u128 {
        let w = v.ldexp(units);
        let top = (u128::from(w.m[mp::LIMBS - 1]) << 64) | u128::from(w.m[mp::LIMBS - 2]);
        let sh = 128 - w.exp;
        assert!((0..128).contains(&sh), "{sh}");
        if sh == 0 { top + u128::from(w.m[mp::LIMBS - 3] >> 63) } else { (top >> sh) + ((top >> (sh - 1)) & 1) }
    }

    fn num(neg: bool, sig: u64, top: i32) -> Num {
        Num { neg, kind: Kind::Finite, sig: u128::from(sig | 1 << 63), exp: top - 63 }
    }

    /// The square root rounded up (operands in `[2^52, 2^54)`, roots with ulp `2^-26`).
    fn ceil_sqrt(v: f64) -> f64 {
        let r = v.sqrt();
        let ri = (r * (1u64 << 26) as f64) as u128;
        if ri * ri < (v as u128) << 52 { f64::from_bits(r.to_bits() + 1) } else { r }
    }

    fn value(n: &Num) -> Mp {
        Mp::from_u128(n.neg, n.sig, i64::from(n.exp))
    }

    #[test]
    fn arc_constants_are_the_rounded_values() {
        let pi = mp::pi();
        assert_eq!(PIO2, fixed(pi.ldexp(-1), 126));
        assert_eq!(PI, fixed(pi, 126));
        let one = Mp::from_i64(1);
        for (c, k, units) in [(A13.into(), 13, 67), (A11.into(), 11, 67), (A9.into(), 9, 67), (A7.into(), 7, 66), (A5, 5, 130), (A3, 3, 129)] {
            assert_eq!(c, fixed(one.div_u64(k), units), "1/{k}");
        }
        for (i, &t) in ATAN_J.iter().enumerate() {
            assert_eq!(t, fixed(mp::atan(Mp::from_i64(i as i64 + 1).ldexp(-7)), 128), "atan({}/128)", i + 1);
        }
    }

    /// Steps 1 and 2 on random operands and the ends of their ranges.
    #[test]
    fn quotient_and_root_meet_their_bounds() {
        let mut g = Rng(0x0123_4567_89ab_cdef);
        let (mut worst, mut root_worst): (f64, f64) = (0.0, 0.0);
        let ends = [1u128 << 127, (1 << 127) + 1, u128::MAX, u128::MAX - 1, 3 << 126];
        let mut pairs: Vec<(u128, u128)> = ends.iter().flat_map(|&a| ends.iter().map(move |&b| (a, b))).collect();
        for _ in 0..20_000 {
            pairs.push((u128::from(g.next()) << 64 | u128::from(g.next()) | 1 << 127, u128::from(g.next()) << 64 | u128::from(g.next()) | 1 << 127));
        }
        for (n, d) in pairs {
            let want = Mp::from_u128(false, n, 126).div(Mp::from_u128(false, d, 0));
            let got = Mp::from_u128(false, div(n, d), 0);
            let rel = (got.sub(want).to_f64() / want.to_f64()).abs();
            assert!(rel < 2f64.powf(-101.8), "div {n:#x} {d:#x}: {rel:e}");
            worst = worst.max(rel);
            // [2^126, 2^128 − 2^60]: 1 − a² for a >= 2^-34 (the caller's floor) in units 2^-128.
            let w = if d & 1 == 0 { n.min(u128::MAX - (1 << 60) + 1) } else { n >> 1 };
            let want = Mp::from_u128(false, w, 128).sqrt();
            // Step 2 allows any `double` root within 2^-52: the truncated soft root, the
            // correctly rounded one (`sqrtsd`'s) and the one rounded up.
            for (k, got) in [sqrt(w, soft_sqrt).0, sqrt(w, f64::sqrt).0, sqrt(w, ceil_sqrt).0].into_iter().enumerate() {
                let rel = (Mp::from_u128(false, got, 0).sub(want).to_f64() / want.to_f64()).abs();
                assert!(rel < 2f64.powf(-100.3), "sqrt {w:#x} root {k}: {rel:e}");
                root_worst = root_worst.max(rel);
            }
        }
        eprintln!("arc: worst root error 2^{:.1}", root_worst.log2());
        eprintln!("arc: worst quotient error 2^{:.1}", worst.log2());
    }

    /// `big_u64` is `as u64` on its whole domain `[2^52, 2^64]` (no spec clause: the root's
    /// conversion without SSE flags, step 2): every binade's ends, random significands, `2^64`.
    #[test]
    fn big_u64_is_the_saturating_conversion() {
        let mut g = Rng(0x5eed_0b16_u64);
        let mut vs = vec![2f64.powi(64)];
        for e in 52..64 {
            let lo = 2f64.powi(e);
            vs.extend([lo, lo * (2.0 - f64::EPSILON)]);
            vs.extend((0..64).map(|_| f64::from_bits(lo.to_bits() | g.next() >> 12)));
        }
        for v in vs {
            assert_eq!(big_u64(v), v as u64, "big_u64({v:e})");
        }
    }

    #[derive(Clone, Copy, Debug, PartialEq)]
    enum Fun {
        Atan,
        Asin,
        Acos,
        Atan2,
        Tan,
    }

    #[derive(Default)]
    struct Tally {
        calls: u64,
        undecided: u64,
        refused: u64,
        worst: f64,
    }

    /// The tier against the 384-bit route for one operand (pair) in the four modes, and the
    /// approximation's error against the proved bound.
    fn check(f: Fun, y: &Num, x: &Num, tally: &mut Tally) {
        if f == Fun::Atan2 && !x.neg && y.top() - x.top() < -72 {
            return; // wide::atan2's tiny-ratio path (x > 0, |y/x| < 2^-72) never reaches the tier
        }
        let one = Mp::from_i64(1);
        let a = value(x);
        let (want, approx) = match f {
            Fun::Atan => {
                let t = mp::atan(a.abs()); // mp::atan takes |t|
                (if x.neg { t.neg() } else { t }, atan_approx(x))
            }
            Fun::Asin => {
                let s = one.sub(a.abs()).mul(one.add(a.abs())).sqrt();
                (mp::atan2(a, s), asin_approx(x, f64::sqrt))
            }
            Fun::Acos => (mp::atan2(one.sub(a).mul(one.add(a)).sqrt(), a), acos_approx(x, f64::sqrt)),
            Fun::Atan2 => (mp::atan2(value(y), a), atan2_approx(y, x)),
            Fun::Tan => {
                let (s, c) = mp::sin_cos_wide(x.neg, x.sig, i64::from(x.exp));
                (s.div(c), tan_approx(x))
            }
        };
        match approx {
            None => tally.refused += 1,
            Some((neg, v)) => {
                let rel = (Mp::from_u128(neg, v.m, i64::from(v.e)).sub(want).to_f64() / want.to_f64()).abs();
                assert!(rel < bound(f),"{f:?} {y:?} {x:?}: error 2^{:.2}", rel.log2());
                tally.worst = tally.worst.max(rel);
            }
        }
        for mode in MODES {
            tally.calls += 1;
            let got = match f {
                Fun::Atan => atan::<F80>(x, mode),
                Fun::Asin => asin::<F80>(x, mode, f64::sqrt),
                Fun::Acos => acos::<F80>(x, mode, f64::sqrt),
                Fun::Atan2 => atan2::<F80>(y, x, mode),
                Fun::Tan => tan::<F80>(x, mode),
            };
            match got {
                Some(got) => assert_eq!(got, mp::round::<F80>(want, mode), "{f:?} {y:?} {x:?} {mode:?}"),
                None => tally.undecided += 1,
            }
        }
    }

    /// Operand `k` of function `f`'s sweep: a general draw or the function's hard region in turn.
    fn draw(f: Fun, k: usize, g: &mut Rng) -> (Num, Num) {
        let neg = g.next() & 1 != 0;
        let sig = g.next();
        let pick = |g: &mut Rng, lo: i32, hi: i32| lo + (g.next() % (hi - lo + 1) as u64) as i32;
        let general = |g: &mut Rng| {
            let band = [(-34, -1), (0, 12), (13, 70), (71, 16383)][g.next() as usize % 4];
            pick(g, band.0, band.1)
        };
        let x = match (f, k % 2) {
            // |x| near 1 (1 − k·2^-64 for k below 2^24) and tiny |x| for acos.
            (Fun::Asin | Fun::Acos, 1) => match g.next() % 3 {
                0 => Num { neg, kind: Kind::Finite, sig: u128::from(u64::MAX - (g.next() >> 40)), exp: -64 },
                1 => num(neg, sig, -1),
                _ => num(neg, sig, if f == Fun::Acos { pick(g, -16382, -35) } else { -34 }),
            },
            (Fun::Asin | Fun::Acos, _) => num(neg, sig, pick(g, -34, -1)),
            // Huge |x|.
            (Fun::Atan, 1) => num(neg, sig, pick(g, 64, 16383)),
            (Fun::Tan, 1) => {
                let e = pick(g, -60, 16320);
                match crate::wide_int::tests::near_multiple(e) {
                    Some(n) if n.exp + 127 - (n.sig.leading_zeros() as i32) >= -34 => Num { neg, ..n },
                    _ => num(neg, sig, general(g)),
                }
            }
            _ => num(neg, sig, general(g)),
        };
        let y = if f == Fun::Atan2 {
            // Every quadrant; ratios near 1 and down to the 2^-72 cut-off on one side.
            let top = x.exp + 63 + if k % 2 == 1 { pick(g, -1, 1) } else { pick(g, -71, 71) };
            num(g.next() & 1 != 0, g.next(), top.clamp(-16382, 16383))
        } else {
            x
        };
        (y, x)
    }

    fn sweep(f: Fun, total: usize, seed: u64) -> Tally {
        let mut g = Rng(seed);
        let mut tally = Tally::default();
        for k in 0..total {
            let (y, x) = draw(f, k, &mut g);
            check(f, &y, &x, &mut tally);
        }
        tally
    }

    const FUNS: [Fun; 5] = [Fun::Atan, Fun::Asin, Fun::Acos, Fun::Atan2, Fun::Tan];

    #[test]
    fn arc_tier_agrees_with_the_wide_route() {
        for (i, f) in FUNS.into_iter().enumerate() {
            let t = sweep(f, 2000, 0x9e37_79b9_7f4a_7c15 ^ i as u64);
            assert!(t.undecided * 1000 <= t.calls, "{f:?}: {} of {} undecided", t.undecided, t.calls);
            assert_eq!(t.refused, 0, "{f:?}");
            eprintln!("arc {f:?}: {} calls, {} undecided, worst error 2^{:.1}", t.calls, t.undecided, t.worst.log2());
        }
        // Fixed points: the x87 values nearest 1, 1/√2, π/4's tangent 1, the largest value.
        let pts = [(u64::MAX, -1), (1 << 63, 0), (0xb504_f333_f9de_6484, -1), (0xb504_f333_f9de_6485, -1), (u64::MAX, 16383)];
        for (sig, top) in pts {
            for neg in [false, true] {
                let x = num(neg, sig, top);
                for f in FUNS {
                    if matches!(f, Fun::Asin | Fun::Acos) && top >= 0 {
                        continue;
                    }
                    check(f, &x, &x, &mut Tally::default());
                    check(f, &num(!neg, 1 << 63, 0), &x, &mut Tally::default());
                }
            }
        }
    }

    /// The brief's sweep: 10^7 operands per function in the four modes, half of them in the
    /// function's hard region. `cargo test --release -p liella-math --lib arc -- --ignored`.
    #[test]
    #[ignore = "5x10^7 operands against the 384-bit route: run in release as a job"]
    fn arc_tier_agrees_with_the_wide_route_long() {
        const TOTAL: usize = 10_000_000;
        let threads = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
        for (i, f) in FUNS.into_iter().enumerate() {
            let per = TOTAL.div_ceil(threads);
            let tallies: Vec<Tally> = std::thread::scope(|s| {
                let hs: Vec<_> = (0..threads)
                    .map(|t| s.spawn(move || sweep(f, per, 0x2545_f491_4f6c_dd1d ^ ((i * 1000 + t) as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15))))
                    .collect();
                hs.into_iter().map(|h| h.join().unwrap_or_else(|p| std::panic::resume_unwind(p))).collect()
            });
            let mut all = Tally::default();
            for t in &tallies {
                all.calls += t.calls;
                all.undecided += t.undecided;
                all.refused += t.refused;
                all.worst = all.worst.max(t.worst);
            }
            eprintln!(
                "arc long {f:?}: {} operands, {} calls, {} undecided, {} refused, worst error 2^{:.2}",
                per * threads,
                all.calls,
                all.undecided,
                all.refused,
                all.worst.log2()
            );
            assert!(all.undecided * 1000 <= all.calls);
        }
    }
}
