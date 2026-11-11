//! The integer fixed-point first tier of the binary128 (`_Float128`) sine and cosine (spec §10.3,
//! D-10.52, D-10.85), and of tangent and `sincos` on the same reduction (D-10.91): the method of the x87 tier in [`super`] carried to three 64-bit limbs, with
//! a proved relative error below `2^-156` and the integer rounding test of Ziv's strategy on a
//! 192-bit significand ([`round`]). When every value within `2^-152` of the approximation rounds
//! alike in the caller's direction, that rounding is the correctly rounded result; otherwise
//! (about `2^-37` of the operands) the caller takes the 384-bit route of [`crate::wide`].
//!
//! Shared with the x87 tier: the bits of `2/π` (`TWO_OVER_PI`, pinned there against Machin's π
//! for every bit this reduction reads), the 128-bit `sin(jπ/256)`, `cos(jπ/256)` table `SC` (this
//! tier adds the signed third limb [`SC_LO`]) and the 64- and 128-bit products. The kernels differ
//! in limb count (two against three), so each format keeps its own: the x87 one stays as cheap as
//! its 64-bit significand allows.
//!
//! Method. Payne-Hanek reduction (M. Payne, R. Hanek, "Radian reduction for trigonometric
//! functions", SIGNUM Newsletter 18(1), 1983), Tang's table method (P. T. P. Tang, "Table-lookup
//! algorithms for elementary functions and their error analysis", ARITH 10, 1991) and fixed-point
//! Horner evaluation in 64-, 128- and 192-bit words (R. Brent, P. Zimmermann, Modern Computer
//! Arithmetic, 2010, ch. 4). Constants from Python `decimal` at 200 digits (π by Machin's formula),
//! each pinned against the 384-bit arithmetic by `constants_are_the_rounded_values`.
//!
//! Proof of the bound (units: `W3` values are 192-bit integers; "λ" is a step's own error).
//! 1. Reduction. `|x| = m·2^e`, `0 < m < 2^113`, `-190 <= e <= 16271` (the caller sends
//!    `2^-58 <= |x|`). As in the x87 tier, bit `j` of `2/π` with `j <= e - 2` adds a multiple of
//!    4 and is dropped; the 448-bit window `W` of bits `e - 1 ..= e + 446` gives `|x|·2/π =
//!    m·W·2^-446 + δ (mod 4)`, `0 <= δ < m·2^-446 < 2^-333`. `P = m·W mod 2^448` is exact (eleven
//!    full and two low 64x64 products); its top two bits are the quadrant, `F = 4P mod 2^448` the
//!    fraction in units of `2^-448`.
//! 2. Table. `k = round(128·F)` in `[0, 128]`, `g = F - k/128` in `[-1/256, 1/256)` (the top word
//!    alone changes). `F < 1/2`: `f = F`, `j = k`, `d = g`; else `f = F - 1` (quadrant + 1), `j =
//!    128 - k`, `d = -g`. Either way `|f| = j/128 + d`, `j` in `[0, 64]` and `|d| = |g|`, formed
//!    as a one's complement (less than `2^-448` low). With `a = f·π/2`, `sin|a| = S_j·C(d) +
//!    C_j·S(d)` and `cos|a| = C_j·C(d) - S_j·S(d)`, `S_j = sin(jπ/256)`, `C_j = cos(jπ/256)` (192
//!    bits, within `2^-193`), `S(d) = sin(πd/2)`, `C(d) = cos(πd/2)`. `|d|` is normalised to
//!    `D·2^(-192-kd)`, `D` in `[2^191, 2^192)`, truncated by less than `2^-191` relative. The tier
//!    refuses `|d| < 2^-160` ([`MAX_LZ`]), so that for `j = 0` (`d = f`) `δ` stays below
//!    `2^-173` relative; the continued-fraction sweep of the tests finds no operand that reaches
//!    this refusal.
//! 3. Polynomials in `u = d² <= 2^-16` (units `2^-206`, at most 7 units low; its top 128 bits
//!    `u_h` and top word `u_1` in units `2^-142`, `2^-78`): `S(d) = d·Σ_{k<=7} (-1)^k s_k u^k`,
//!    `s_k = (π/2)^(2k+1)/(2k+1)!`, and `C(d) = Σ_{k<=7} (-1)^k c_k u^k`, `c_k =
//!    (π/2)^(2k)/(2k)!`. The series alternate with decreasing terms (`|πd/2| <= π/512`): the
//!    remainders are below `2^-165.9` (relative, S) and `2^-161.8` (C). Horner from the innermost
//!    term, each step a constant minus a smaller product (every partial sum is positive): steps
//!    `k = 7..5` in 64-bit words, `4..2` in 128 bits, `1, 0` in 192. A step's error λ_k (half a
//!    unit for the constant, one for the truncated product, and the truncation of `u_1`, `u_h`,
//!    `u` times the inner sum) reaches the sum multiplied by `u^k <= 2^-16k`. Sine: λ_5 <=
//!    `2^-81.4` and λ_2 <= `2.5·2^-131` give `2^-161.4` and `2^-161.7`, every other step is below
//!    `2^-180`: with the remainder, `Σ s_k u^k` (at least 1.57) within `2^-161` relative. Cosine:
//!    λ_5 <= `2^-78.4` and λ_2 <= `2.5·2^-129` give `2^-158.4` and `2^-159.7`, the rest below
//!    `2^-179`: `C(d)` within `2^-157.6` absolute (and relative: `C >= 0.9999`). `S(|d|) =
//!    D·Σ` (one 192-bit product, at most 6 units of `2^-197` low) is within `2^-161` relative.
//! 4. Result. `j = 0`: `S(d)` alone (with `δ`: below `2^-160.9` relative) or `C(d)` (below
//!    `2^-157.5`). `j >= 1`: two 192-bit products, each at most 6 units low; `sin|a| >=
//!    sin(π/512)`, `S_j <= 2·sin|a|` and `|S(d)| <= sin|a|`, so the error is below `2·2^-157.6 +
//!    2^-161 + 2^-180 < 2^-156.4` relative; `cos|a| >= 0.7`: below `2^-157`. The approximation is
//!    within `2^-156` relative; the rounding test assumes `2^-152` ([`B`]).
//! 5. Rounding test: on the 192-bit significand `M` (top bit set), the true value lies within
//!    `2^(192 - B) + 16` units of `M`'s last place. The binary128 significand is `M`'s top 113
//!    bits; when the 78 bits below its rounding bit are at least that far from zero and from
//!    `2^78`, every value in the interval rounds alike in every direction and none is exact. The
//!    results lie in `[2^-80, 1]`: always normal, never overflowing.
//! 6. Quotient [`div3`] of `n`, `d` in `[2^191, 2^192)` (`ν = n/2^192`, `δ = d/2^192` in `[1/2,
//!    1)`): Newton's reciprocal iteration and one residual correction (Brent & Zimmermann ch. 3;
//!    J.-M. Muller, Elementary Functions, 3rd ed. 2016, ch. 2), one `divsd` and no integer `div`.
//!    `v0 = 2^115/⌊δ·2^53⌋` (one `divsd`, truncated) is `1/δ` within `2^-50.99` (as the x87 arc
//!    tier's step 1). With `δ'` the top 128 bits of `δ` (`2^-127` relative), `ε = 1 − δ'v0`
//!    (`|ε| < 2^-50.9`) is formed within one unit of `2^-110` from the top of the 192-bit product,
//!    and `v1 = v0 + v0·ε` (the product truncated to `2^-126`) leaves `|1 − δ·v1| <= ε² +
//!    2^-108.9 + 2^-125 < 2^-101.5`. `q = ν'·v1` from `n`'s top 128 bits (truncations below `2^-125`) is `ν/δ`
//!    within `2^-101.4` relative, so the residual `E = ν − qδ` (exact modulo `2^256` in units
//!    `2^-318`, `|E| < 2^-101.4`) is formed exactly and truncated to `2^-192`. `q + E·v1'`, with
//!    `v1'` the top word of `v1` (`|v1' − 1/δ| < 2^-61.9`) and the product truncated to `2^-190`,
//!    is `ν/δ` within `2^-101.4·2^-61.9 + 2^-189.4 < 2^-163.2` absolute: `2^-162.2` relative
//!    (`ν/δ > 1/2`).
//! 7. `tan` and `sincos` ([`tan`], [`sin_and_cos`]): one reduction and both polynomials give
//!    `sin|a|` and `cos|a|` (step 4: within `2^-156.4` and `2^-157` for `j >= 1`, `2^-160.9` and
//!    `2^-157.5` for `j = 0`); `sincos` rounds each as `sin`, `cos` do. `tan|x| = tan a` in even
//!    quadrants and `cot a` in odd ones: the quotient of step 6 adds its `2^-162.2` to the two
//!    relative errors, so `tan` is within `2^-156.4 + 2^-157 + 2^-162.2 < 2^-155.6`, inside the
//!    rounding test's `2^-152`. `|d| >= 2^-160` and `|x| >= 2^-58` keep `tan` in `[2^-59,
//!    2^161]`: normal.

use super::{M64, SC, mul_wide, mulhi, mulhi64};
use crate::coeffs::tables as t;
use crate::float::{Float, Kind, Num};
use crate::status::{Flags, Rounding, Status};

/// The rounding test's bound exponent: the approximation is within `2^-B` relative (proved
/// `2^-156`, see the module documentation).
const B: u32 = 152;
/// The rounding test's bound in units of the 192-bit significand's last place.
const ERR: u128 = (1 << (192 - B)) + 16;
/// `|d|` below `2^-160` (more than 159 leading zeros in its 448-bit form) is refused.
const MAX_LZ: u32 = 159;

/// A 192-bit fixed-point value `h·2^64 + l`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct W3 {
    h: u128,
    l: u64,
}

impl W3 {
    fn sub(self, b: W3) -> W3 {
        let (l, borrow) = self.l.overflowing_sub(b.l);
        W3 { h: self.h - b.h - u128::from(borrow), l }
    }

    fn add(self, b: W3) -> W3 {
        let (l, carry) = self.l.overflowing_add(b.l);
        W3 { h: self.h + b.h + u128::from(carry), l }
    }

    /// `⌊self/2^n⌋` (zero from `n = 192`).
    fn shr(self, n: u32) -> W3 {
        match n {
            0 => self,
            1..64 => W3 { h: self.h >> n, l: (self.l >> n) | ((self.h as u64) << (64 - n)) },
            64..192 => {
                let v = self.h >> (n - 64);
                W3 { h: v >> 64, l: v as u64 }
            }
            _ => W3 { h: 0, l: 0 },
        }
    }

    /// `self·2^n`, `n < 64`, the top `n` bits zero.
    fn shl(self, n: u32) -> W3 {
        if n == 0 { self } else { W3 { h: (self.h << n) | u128::from(self.l >> (64 - n)), l: self.l << n } }
    }

    /// `h·2^64 + lo` for a signed third limb.
    fn with_lo(h: u128, lo: i64) -> W3 {
        W3 { h: if lo < 0 { h - 1 } else { h }, l: lo as u64 }
    }
}

/// `⌊a·b/2^192⌋` less at most 6 (the products below the third word dropped).
#[inline(always)]
fn mulhi3(a: W3, b: W3) -> W3 {
    let (a2, a1, a0) = (a.h >> 64, a.h & M64, u128::from(a.l));
    let (b2, b1, b0) = (b.h >> 64, b.h & M64, u128::from(b.l));
    let (s, c1) = (a2 * b1).overflowing_add(a1 * b2);
    let (s, c2) = s.overflowing_add(((a2 * b0) >> 64) + ((a1 * b1) >> 64) + ((a0 * b2) >> 64));
    W3 { h: a2 * b2 + (s >> 64) + ((u128::from(c1) + u128::from(c2)) << 64), l: s as u64 }
}

/// `⌊a·b/2^128⌋` less at most 3.
#[inline(always)]
fn mul32(a: W3, b: u128) -> W3 {
    let (a2, a1, a0) = (a.h >> 64, a.h & M64, u128::from(a.l));
    let (b1, b0) = (b >> 64, b & M64);
    let (s, c1) = (a2 * b0).overflowing_add(a1 * b1);
    let (s, c2) = s.overflowing_add(((a1 * b0) >> 64) + ((a0 * b1) >> 64));
    W3 { h: a2 * b1 + (s >> 64) + ((u128::from(c1) + u128::from(c2)) << 64), l: s as u64 }
}

// The series of `sin(πd/2)/d` and `cos(πd/2)` in `u = d²` (step 3), each coefficient rounded to
// nearest in the units noted.
const S7: u64 = 0xb7d6dcf8aaba1c8b; // (π/2)^15/15!, units 2^-94
const S6: u64 = 0xf47a1a680c6b1994; // (π/2)^13/13!, units 2^-88
const S5: u64 = 0xf183a7ef4438fb12; // (π/2)^11/11!, units 2^-82
const S4: u128 = 0xa83c1a43_f73c0dc8_3d6322ef_56c7a534; // (π/2)^9/9!, units 2^-140
const S3: u128 = 0x99696673_15ec2df3_2f70bfb2_32e0b12b; // (π/2)^7/7!, units 2^-135
const S2: u128 = 0xa335e33b_ad570e92_3f34224f_03d18616; // (π/2)^5/5!, units 2^-131
const S1: W3 = W3 { h: 0xa55de731_2df295f5_5dc72f71_2af24826, l: 0x6701418c324415a0 }; // (π/2)^3/3!, units 2^-192
const S0: W3 = W3 { h: 0x6487ed51_10b4611a_62633145_c06e0e68, l: 0x948127044533e63a }; // π/2, units 2^-190
const C7: u64 = 0xdb7127a25894796e; // (π/2)^14/14!, units 2^-91
const C6: u64 = 0xfce9c51bb1e6146f; // (π/2)^12/12!, units 2^-85
const C5: u64 = 0xd368f95102545d4d; // (π/2)^10/10!, units 2^-79
const C4: u128 = 0xf0fa8344_8dd5d7a3_2742da8f_da4be80e; // (π/2)^8/8!, units 2^-138
const C3: u128 = 0xaae9e3f1_e5ffcfe2_a7d6df6c_89dad8bf; // (π/2)^6/6!, units 2^-133
const C2: u128 = 0x81e0f840_dad61d9a_9980f007_d76d129c; // (π/2)^4/4!, units 2^-129
const C1: W3 = W3 { h: 0x9de9e64d_f22ef2d2_56e26cd9_808c1ac7, l: 0x08566a3fe0d0a228 }; // (π/2)^2/2!, units 2^-191
const ONE: W3 = W3 { h: 1 << 127, l: 0 }; // 1, units 2^-191

/// `f(x)` rounded into `F` (binary128) in `mode` when the tier decides it (`cos`: cosine, else
/// sine); `None` sends the caller to the 384-bit route. `n` is finite, nonzero and at least
/// `2^-58` in magnitude (smaller operands are settled by the caller).
pub(crate) fn sin_cos<F: Float>(n: &Num, cos: bool, mode: Rounding) -> Option<(F, Status)> {
    if F::PREC != 113 || F::EXPLICIT {
        return None;
    }
    let (neg, m, e) = kernel(n, cos)?;
    round::<F>(neg, m, e, mode)
}

/// `tan x` rounded into `F` (binary128) in `mode` when the tier decides it (step 7); `None`
/// sends the caller to the 384-bit route. `n` as for [`sin_cos`].
pub(crate) fn tan<F: Float>(n: &Num, mode: Rounding) -> Option<(F, Status)> {
    if F::PREC != 113 || F::EXPLICIT {
        return None;
    }
    let (neg, m, e) = tan_kernel(n)?;
    round::<F>(neg, m, e, mode)
}

/// A rounded result, or `None` where the rounding test does not decide it.
pub(crate) type Decided<F> = Option<(F, Status)>;

/// `sin x` and `cos x` from one reduction (step 7): each rounded into `F` (binary128) in `mode`,
/// `None` in either place the rounding test does not decide (that function alone takes the
/// 384-bit route); `None` outside the tier. `n` as for [`sin_cos`].
pub(crate) fn sin_and_cos<F: Float>(n: &Num, mode: Rounding) -> Option<(Decided<F>, Decided<F>)> {
    if F::PREC != 113 || F::EXPLICIT {
        return None;
    }
    let (r, s, c) = both(n)?;
    let pick = |cos: bool| {
        let (neg, odd) = sign(n, &r, cos);
        let (m, e) = if odd { c } else { s };
        round::<F>(neg, m, e, mode)
    };
    Some((pick(false), pick(true)))
}

/// The reduced operand of a finite nonzero binary128 `n` in the range of step 1.
fn reduced(n: &Num) -> Option<Reduced> {
    if n.kind != Kind::Finite || n.sig == 0 || n.sig >> 113 != 0 || !(-190..=16271).contains(&n.exp) {
        return None;
    }
    reduce(n.sig, n.exp)
}

/// The sign of `sin x` (`cos x`) and whether it takes `cos|a|`: `sin|x| = sin(qπ/2 + a)`, `cos|x|
/// = sin((q + 1)π/2 + a)`; an odd quadrant takes `cos a`.
fn sign(n: &Num, r: &Reduced, cos: bool) -> (bool, bool) {
    let k = r.q + u32::from(cos);
    let odd = k & 1 != 0;
    ((!cos && n.neg) ^ (k & 2 != 0) ^ (!odd && r.negf), odd)
}

/// A positive value `M·2^E` as `(M, E)`, `M` in `[2^191, 2^192)`.
type Scaled = (W3, i32);

/// `v·2^-units` (`v` at least `2^128`) as `(M, E)`, `M` in `[2^191, 2^192)`.
fn norm(v: W3, units: u32) -> Scaled {
    let l = v.h.leading_zeros();
    (v.shl(l), -(units as i32) - l as i32)
}

/// `d²` in units `2^-206` and `S(|d|) = |d|·Σ` in units `2^-(190 + kd)` (step 3).
fn sq_and_sine(r: &Reduced) -> (W3, W3) {
    let u = mulhi3(r.d, r.d).shr(2 * r.kd - 14); // kd >= 7
    (u, mulhi3(r.d, sin_poly(u)))
}

/// Step 4 for `j >= 1`: `sin|a|` (`odd` false) or `cos|a|` from `S(|d|)` (units `2^-197`) and
/// `C(d)` (units `2^-191`), in units `2^-191`, at least `sin(π/512)`.
fn combine(r: &Reduced, s: W3, c: W3, odd: bool) -> W3 {
    let ([sh, ch], [sl, cl]) = (SC[r.j - 1], SC_LO[r.j - 1]);
    let (sj, cj) = (W3::with_lo(sh, sl), W3::with_lo(ch, cl)); // units 2^-192
    if odd {
        let (a, b) = (mulhi3(cj, c), mulhi3(sj, s).shr(6));
        if r.sd { a.add(b) } else { a.sub(b) }
    } else {
        let (a, b) = (mulhi3(sj, c), mulhi3(cj, s).shr(6));
        if r.sd { a.sub(b) } else { a.add(b) }
    }
}

/// `(sign, M, E)` with `M` in `[2^191, 2^192)` and `±M·2^E` within `2^-156` (relative) of `sin x`
/// (`cos x`), or `None` outside the range of step 1 or for `|d| < 2^-160` (step 2).
fn kernel(n: &Num, cos: bool) -> Option<(bool, W3, i32)> {
    // Kept apart from `both` (one function, its helpers written out): the single-function path
    // measured 100 instructions more per call through the shared helpers.
    if n.kind != Kind::Finite || n.sig == 0 || n.sig >> 113 != 0 || !(-190..=16271).contains(&n.exp) {
        return None;
    }
    let r = reduce(n.sig, n.exp)?;
    let k = r.q + u32::from(cos);
    let odd = k & 1 != 0;
    let neg = (!cos && n.neg) ^ (k & 2 != 0) ^ (!odd && r.negf);
    let (d, kd) = (r.d, r.kd);
    let u = mulhi3(d, d).shr(2 * kd - 14); // d², units 2^-206 (kd >= 7)
    if r.j == 0 {
        return Some(if odd {
            let c = cos_poly(u); // units 2^-191
            let l = c.h.leading_zeros();
            (neg, c.shl(l), -191 - l as i32)
        } else {
            let s = mulhi3(d, sin_poly(u)); // s·2^(-190-kd)
            let l = s.h.leading_zeros();
            (neg, s.shl(l), -190 - kd as i32 - l as i32)
        });
    }
    let s = mulhi3(d, sin_poly(u)).shr(kd - 7); // S(|d|), units 2^-197
    let c = cos_poly(u); // C(d), units 2^-191
    let ([sh, ch], [sl, cl]) = (SC[r.j - 1], SC_LO[r.j - 1]);
    let (sj, cj) = (W3::with_lo(sh, sl), W3::with_lo(ch, cl)); // units 2^-192
    let v = if odd {
        let (a, b) = (mulhi3(cj, c), mulhi3(sj, s).shr(6));
        if r.sd { a.add(b) } else { a.sub(b) }
    } else {
        let (a, b) = (mulhi3(sj, c), mulhi3(cj, s).shr(6));
        if r.sd { a.sub(b) } else { a.add(b) }
    }; // units 2^-191, at least sin(π/512)
    let l = v.h.leading_zeros();
    Some((neg, v.shl(l), -191 - l as i32))
}

/// Steps 1 to 4 for both functions: the reduced operand, `sin|a|` and `cos|a|` (each `(M, E)`,
/// `M` in `[2^191, 2^192)`, within the bounds of step 4).
fn both(n: &Num) -> Option<(Reduced, Scaled, Scaled)> {
    let r = reduced(n)?;
    let (u, s) = sq_and_sine(&r);
    let c = cos_poly(u);
    let (sa, ca) = if r.j == 0 {
        (norm(s, 190 + r.kd), norm(c, 191))
    } else {
        let s = s.shr(r.kd - 7);
        (norm(combine(&r, s, c, false), 191), norm(combine(&r, s, c, true), 191))
    };
    Some((r, sa, ca))
}

/// Step 7: `(sign, M, E)` with `±M·2^E` within `2^-155.6` (relative) of `tan x`, `M` in
/// `[2^191, 2^192)`: `tan|x| = tan a` (`q` even) or `cot a` (`q` odd), `a = fπ/2` of `f`'s sign.
fn tan_kernel(n: &Num) -> Option<(bool, W3, i32)> {
    let (r, s, c) = both(n)?;
    let odd = r.q & 1 != 0;
    let (top, bot) = if odd { (c, s) } else { (s, c) };
    let (m, e) = norm(div3(top.0, bot.0), 190);
    Some((n.neg ^ r.negf ^ odd, m, e + top.1 - bot.1))
}

const TWO115: f64 = (1u128 << 115) as f64;

/// Step 6: `(n/d)·2^190` for `n`, `d` in `[2^191, 2^192)`, in `[2^188, 2^191)` (`n/d` in `(1/2,
/// 2)`, the approximation a little either side), within `2^-162` relative.
fn div3(n: W3, d: W3) -> W3 {
    let dh = (d.h >> 64) as u64;
    let v = (TWO115 / ((dh >> 11) as i64 as f64)) as u64; // 2^62/δ within 2^-50.99, at most 2^63
    // ε = 1 − δ'·v0 in units 2^-110 (one unit high at most): δ'·v0 is 2^190 within 2^139.1 in
    // units 2^-190, so ⌊δ'·v0/2^80⌋ is 2^110 within 2^59.1.
    let vw = u128::from(v);
    let p = ((d.h >> 64) * vw + (((d.h & M64) * vw) >> 64)) >> 16;
    let eps = (1i128 << 110) - p as i128;
    let v1 = (vw << 64).wrapping_add(((i128::from(v) * eps) >> 46) as u128); // 1/δ, units 2^-126
    let q = mulhi(n.h, v1); // n/δ, units 2^-126, below 2^128
    // E = n·2^126 − q·d (units 2^-318) modulo 2^256: |E| < 2^217, so its bits 126 ..= 253 are E/2^126.
    let (q1, q0) = (q >> 64, q & M64);
    let (d2, d1, d0) = (d.h >> 64, d.h & M64, u128::from(d.l));
    let (a, b) = (q0 * d0, q1 * d0); // q·d0 = b·2^64 + a
    let (lo, c0) = a.overflowing_add(b << 64);
    let hi = (b >> 64) + u128::from(c0);
    // q·d.h·2^64 modulo 2^256: q0·d1 at 2^64, q1·d1 and q0·d2 at 2^128, q1·d2 at 2^192.
    let m = q0 * d1;
    let (lo, c1) = lo.overflowing_add(m << 64);
    let hi = hi.wrapping_add(m >> 64).wrapping_add(u128::from(c1));
    let hi = hi.wrapping_add(q1 * d1).wrapping_add(q0 * d2).wrapping_add((q1 * d2) << 64);
    // n·2^126 modulo 2^256.
    let (nh, nl) = ((n.h << 62) | u128::from(n.l >> 2), u128::from(n.l) << 126);
    let (el, b0) = nl.overflowing_sub(lo);
    let eh = nh.wrapping_sub(hi).wrapping_sub(u128::from(b0));
    let e = ((eh << 2) | (el >> 126)) as i128; // E/2^126, units 2^-192
    // q + E·v1/2^318, the product from v1's top word: units 2^-190.
    let (ea, vh) = (e.unsigned_abs(), v1 >> 64);
    let corr = (ea >> 64) * vh + (((ea & M64) * vh) >> 64);
    let (q, corr) = (W3 { h: q, l: 0 }, W3 { h: corr >> 64, l: corr as u64 });
    if e < 0 { q.sub(corr) } else { q.add(corr) }
}

/// Word `i` of the bits of `2/π` behind three zero words (bit `j` of `2/π` at bit `j + 191`).
#[inline(always)]
fn word(i: usize) -> u64 {
    match i.checked_sub(3) {
        Some(k) => t::TWO_OVER_PI[k],
        None => 0,
    }
}

/// The reduced operand of steps 1 and 2.
struct Reduced {
    /// The quadrant (mod 4) of the centred fraction.
    q: u32,
    /// `f < 0`.
    negf: bool,
    /// `j = round(128|f|)`.
    j: usize,
    /// `d < 0`.
    sd: bool,
    /// `|d| = d·2^(-192-kd)`, top bit set.
    d: W3,
    kd: u32,
}

/// Steps 1 and 2 for `|x| = m·2^e`, or `None` for `|d| < 2^-160`.
#[inline(always)]
fn reduce(m: u128, e: i32) -> Option<Reduced> {
    let p = (e + 190) as usize; // bit e - 1 of 2/π; e >= -190
    let (w, r) = (p / 64, (p % 64) as u32);
    let g = |i: usize| {
        let a = word(w + i);
        if r == 0 { a } else { (a << r) | (word(w + i + 1) >> (64 - r)) }
    };
    // The window, least significant word first.
    let a: [u128; 7] = core::array::from_fn(|k| u128::from(g(6 - k)));
    let (m0, m1) = (m & M64, m >> 64);
    // P = m·W mod 2^448, column by column (m1 < 2^49: each column sum fits 128 bits).
    let mut pw = [0u64; 7];
    let mut c = 0u128;
    for k in 0..7 {
        let mut s = c + ((m0 * a[k]) & M64);
        if k >= 1 {
            s += ((m0 * a[k - 1]) >> 64) + ((m1 * a[k - 1]) & M64);
        }
        if k >= 2 {
            s += (m1 * a[k - 2]) >> 64;
        }
        pw[k] = s as u64;
        c = s >> 64;
    }
    let q = (pw[6] >> 62) as u32;
    // F = 4P mod 2^448.
    let mut f = [0u64; 7];
    f[0] = pw[0] << 2;
    for k in 1..7 {
        f[k] = (pw[k] << 2) | (pw[k - 1] >> 62);
    }
    let top = f[6];
    let half = top >> 63 != 0;
    let k = ((u128::from(top) + (1 << 56)) >> 57) as u64; // round(128·F), at most 128
    f[6] = top.wrapping_sub(k << 57); // g, two's complement
    let gneg = (f[6] as i64) < 0;
    if gneg {
        for x in &mut f {
            *x = !*x;
        }
    }
    let lz = if f[6] != 0 {
        f[6].leading_zeros()
    } else if f[5] != 0 {
        64 + f[5].leading_zeros()
    } else {
        128 + f[4].leading_zeros()
    };
    if lz > MAX_LZ {
        return None;
    }
    let (ws, bs) = ((lz / 64) as usize, lz % 64);
    let at = |i: usize| {
        let a = f[6 - ws - i];
        if bs == 0 { a } else { (a << bs) | (f[5 - ws - i] >> (64 - bs)) }
    };
    let d = W3 { h: (u128::from(at(0)) << 64) | u128::from(at(1)), l: at(2) };
    Some(Reduced {
        q: if half { (q + 1) & 3 } else { q },
        negf: half,
        j: (if half { 128 - k } else { k }) as usize,
        sd: gneg ^ half,
        d,
        kd: lz,
    })
}

/// `sin(πd/2)/d` in units of `2^-190`.
#[inline(always)]
fn sin_poly(u: W3) -> W3 {
    let (uh, u1) = (u.h, (u.h >> 64) as u64);
    let a = S6 - (mulhi64(u1, S7) >> 20); // units 2^-88
    let a = S5 - (mulhi64(u1, a) >> 20); // units 2^-82
    let a = S4 - (mul_wide(uh, a) >> 20); // units 2^-140
    let a = S3 - (mulhi(uh, a) >> 19); // units 2^-135
    let a = S2 - (mulhi(uh, a) >> 18); // units 2^-131
    let a = S1.sub(mul32(u, a).shr(17)); // units 2^-192
    S0.sub(mulhi3(u, a).shr(16))
}

/// `cos(πd/2)` in units of `2^-191`.
#[inline(always)]
fn cos_poly(u: W3) -> W3 {
    let (uh, u1) = (u.h, (u.h >> 64) as u64);
    let b = C6 - (mulhi64(u1, C7) >> 20); // units 2^-85
    let b = C5 - (mulhi64(u1, b) >> 20); // units 2^-79
    let b = C4 - (mul_wide(uh, b) >> 19); // units 2^-138
    let b = C3 - (mulhi(uh, b) >> 19); // units 2^-133
    let b = C2 - (mulhi(uh, b) >> 18); // units 2^-129
    let b = C1.sub(mul32(u, b).shr(16)); // units 2^-191
    ONE.sub(mulhi3(u, b).shr(14))
}

/// Step 5: `±m·2^e` rounded into `F` (binary128) in `mode`, or `None` when the bound does not
/// decide it.
pub(crate) fn round<F: Float>(neg: bool, m: W3, e: i32, mode: Rounding) -> Option<(F, Status)> {
    let low = ((m.h & 0x7fff) << 64) | u128::from(m.l); // the 79 bits below the significand
    // The distance above the last representable value or midpoint must lie in [ERR, 2^78 - ERR].
    if (low & ((1 << 78) - 1)).wrapping_sub(ERR) > (1 << 78) - 2 * ERR {
        return None;
    }
    let up = match mode {
        Rounding::Nearest => low >> 78 != 0,
        Rounding::Zero => false,
        Rounding::Up => !neg,
        Rounding::Down => neg,
    };
    let (sig, e) = match (m.h >> 15) + u128::from(up) {
        s if s >> 113 != 0 => (s >> 1, e + 80),
        s => (s, e + 79),
    };
    // sig·2^e, sig in [2^112, 2^113): biased exponent e + 112 + bias.
    let be = e + 112 + F::EMAX;
    if be <= 0 || be > 2 * F::EMAX {
        return None;
    }
    let bits = (u128::from(neg) << 127) | ((be as u128) << 112) | (sig & ((1 << 112) - 1));
    Some((F::from_bits(bits), Status::flags(Flags::INEXACT)))
}

/// The third limb of `SC` (`round(v·2^192) - SC·2^64` for `v = sin(jπ/256)`, `cos(jπ/256)`,
/// row `j - 1`): `SC·2^64 + SC_LO` is `v` in units of `2^-192`, within half a unit.
const SC_LO: [[i64; 2]; 64] = [
    [-1142379357152727259, -32218618080951844],
    [7090125544463075500, 6237404749295529779],
    [247016144575515607, 3512086147274618245],
    [1303359570892083436, -3507208194524764917],
    [-344927953190351011, 4373080621701970883],
    [-9056068227078791663, -7939980083762204177],
    [-6674125038546561553, 6847221312806070042],
    [2378753895827361940, 6497734857222403432],
    [4140844717272109686, -50709621317420504],
    [-2143022415424457953, -3702700546528774040],
    [352983557049771224, -7680391591428166159],
    [-6926184070741725680, -6802965590462900439],
    [-228672836092403484, 4169184661188951951],
    [2582148876217484098, 2624996387069465489],
    [-6067455406916969633, -681972412979049718],
    [-894838031874838755, -4182702486534911262],
    [-8338439538392466369, 8675521246531154036],
    [3214303618570023356, 8770029814131100513],
    [-4523675601236518579, 3317307317781830754],
    [6160933496977246131, -7664655204881121358],
    [-3818559181135072721, 5697290301896551022],
    [-633546110623335386, -5735241045678777464],
    [3103263154696331443, -3785489129245581639],
    [-4105818211420304767, 6837784307740776233],
    [-1605500819432851065, -2854816043056668810],
    [8865910099116048829, 7838809258027452120],
    [-2451680580428070673, -8939899782677829603],
    [1879545779452175445, -8513295466186431958],
    [-6817621754210287681, -1145120274257162501],
    [-1191967914010640085, -265803775659848128],
    [5058428154496241070, 8013046866966181903],
    [-4759753174838025652, -1105566377108117066],
    [-3592712123309088451, -1211199862878068938],
    [1656978895993889481, 4420008092110585711],
    [-8754794086597158609, 1765443871429165962],
    [296800552269693579, 3394503729177945169],
    [2796784371090926488, -8945598673964799396],
    [6511242249670796596, -8699841782338655601],
    [-2980521892698433950, -2720829275128923556],
    [-2987384626304212783, -150237333847461080],
    [3224874363516294326, -3920283517626312501],
    [-5378739394035867572, -3124838887838358662],
    [-4990835048403794329, 8675895587370687943],
    [-2973361030461935933, -5300012407139288957],
    [-6626910829619309199, 8542751760365503237],
    [2151753241873406008, -2509583258746886912],
    [-3382670474758682336, -98428663538606688],
    [8982384693265277759, -5446790184044814677],
    [1023206592962271305, 8240242902769885477],
    [4950972209123637862, -2051728441724185255],
    [6232903826426306915, -2957944506566845890],
    [7742054951872310607, -7132873985768719042],
    [7848936877514453810, -7136454592147372135],
    [-8700311996543411725, 5475336574015856543],
    [-1765907586156227931, 6687459553488948084],
    [-4359079768021961216, -3137821659096186408],
    [-4699934463676314992, -7226825126616523886],
    [-7301741320581765459, -2916713185010325598],
    [-6373836474199492809, -6577763018217987538],
    [3445521470090825716, -7176642919963374928],
    [-2809980951907646774, 9070039801655596949],
    [-6466531838619425740, 4877725496848427680],
    [-6729171898828472932, -2015751018222173123],
    [2121020303797364813, 2121020303797364813],
];

#[cfg(test)]
mod tests {
    // Spec clause: §10.3 (_Float128 sinf128/cosf128 correctly rounded in the four directions, D-10.19).
    use super::super::tests::{MODES, Rng, Tally};
    use super::*;
    use crate::float::F128;
    use crate::mp::{self, Mp};

    /// `v·2^units` rounded to the nearest integer as 192 bits (these values are irrational: no ties).
    fn fixed3(v: Mp, units: i64) -> (u128, u64) {
        let w = v.ldexp(units);
        let top = W3 { h: (u128::from(w.m[mp::LIMBS - 1]) << 64) | u128::from(w.m[mp::LIMBS - 2]), l: w.m[mp::LIMBS - 3] };
        let sh = 192 - w.exp;
        assert!((0..192).contains(&sh), "{sh}");
        let r = if sh == 0 {
            top.add(W3 { h: 0, l: w.m[mp::LIMBS - 4] >> 63 })
        } else {
            let r = top.shr(sh as u32 - 1);
            r.shr(1).add(W3 { h: 0, l: r.l & 1 })
        };
        (r.h, r.l)
    }

    #[test]
    fn constants_are_the_rounded_values() {
        let half_pi = mp::pi().ldexp(-1);
        let mut p = vec![Mp::from_i64(1)]; // p[k] = (π/2)^k / k!
        for k in 1..=15u64 {
            let next = p[p.len() - 1].mul(half_pi).div_u64(k);
            p.push(next);
        }
        let x1 = |v: u64| (0, v);
        let x2 = |v: u128| (v >> 64, v as u64);
        let x3 = |v: W3| (v.h, v.l);
        let pinned: [((u128, u64), usize, i64); 15] = [
            (x1(S7), 15, 94),
            (x1(S6), 13, 88),
            (x1(S5), 11, 82),
            (x2(S4), 9, 140),
            (x2(S3), 7, 135),
            (x2(S2), 5, 131),
            (x3(S1), 3, 192),
            (x3(S0), 1, 190),
            (x1(C7), 14, 91),
            (x1(C6), 12, 85),
            (x1(C5), 10, 79),
            (x2(C4), 8, 138),
            (x2(C3), 6, 133),
            (x2(C2), 4, 129),
            (x3(C1), 2, 191),
        ];
        for (c, k, units) in pinned {
            assert_eq!(c, fixed3(p[k], units), "(π/2)^{k}/{k}!");
        }
        let pi = mp::pi();
        for i in 0..64 {
            let (ws, wc) = mp::sin_cos(pi.mul_u64(i as u64 + 1).ldexp(-8));
            let (s, c) = (W3::with_lo(SC[i][0], SC_LO[i][0]), W3::with_lo(SC[i][1], SC_LO[i][1]));
            assert_eq!((s.h, s.l), fixed3(ws, 192), "sin({}π/256)", i + 1);
            assert_eq!((c.h, c.l), fixed3(wc, 192), "cos({}π/256)", i + 1);
        }
    }

    /// `±M·2^e` exactly, in the 384-bit arithmetic.
    fn value(neg: bool, m: W3, e: i32) -> Mp {
        Mp::from_u128(neg, m.h, i64::from(e) + 64).add(Mp::from_u128(neg, u128::from(m.l), i64::from(e)))
    }

    #[test]
    fn rounding_test_refuses_near_a_boundary() {
        // The bound of step 5: 2^(192 − 152) + 16 units of the 192-bit significand's last place,
        // measured in the 78 bits below the rounding bit.
        let err = (1u128 << 40) + 16;
        let half = 1u128 << 78;
        let cases = [
            (err - 1, false),
            (err, true),
            (half - err, true),
            (half - err + 1, false),
            (half + err - 1, false),
            (half + err, true),
            ((1 << 79) - err, true),
            ((1 << 79) - err + 1, false),
        ];
        // A significand of π/2's bits and one of all ones (rounding up carries into the exponent).
        for hi in [S0.h | 1 << 127, u128::MAX] {
            for (low, decided) in cases {
                let m = W3 { h: (hi & !0x7fff) | (low >> 64), l: low as u64 };
                for neg in [false, true] {
                    for mode in MODES {
                        let got = round::<F128>(neg, m, -200, mode);
                        assert_eq!(got.is_some(), decided, "{low:#x} {mode:?}");
                        if let Some(got) = got {
                            assert_eq!(got, mp::round::<F128>(value(neg, m, -200), mode), "{hi:#x} {low:#x} {neg} {mode:?}");
                        }
                    }
                }
            }
        }
    }

    /// Bands of `floor(log2 |x|)`: from the smallest operand the tier takes, moderate, large, and
    /// up to the largest binary128 value.
    const BANDS: [(i32, i32); 4] = [(-58, -1), (0, 12), (13, 70), (71, 16383)];

    fn operand(g: &mut Rng, (lo, hi): (i32, i32)) -> Num {
        let top = lo + (g.next() % (hi - lo + 1) as u64) as i32;
        let sig = (((u128::from(g.next()) << 64) | u128::from(g.next())) >> 15) | 1 << 112;
        Num { neg: g.next() & 1 != 0, kind: Kind::Finite, sig, exp: top - 112 }
    }

    /// The tier on `n` against the 384-bit route in the four modes, and the kernel's error against
    /// the bound the rounding test assumes.
    fn check(n: &Num, tally: &mut Tally) {
        let (s, c) = mp::sin_cos_wide(n.neg, n.sig, i64::from(n.exp));
        for (cos, want) in [(false, s), (true, c)] {
            match kernel(n, cos) {
                None => tally.refused += 1,
                Some((neg, m, e)) => {
                    let rel = (value(neg, m, e).sub(want).to_f64() / want.to_f64()).abs();
                    assert!(rel < (-f64::from(B)).exp2(), "{n:?} cos={cos}: error {rel:e}");
                    tally.worst = tally.worst.max(rel);
                }
            }
            for mode in MODES {
                tally.calls += 1;
                match sin_cos::<F128>(n, cos, mode) {
                    Some(got) => assert_eq!(got, mp::round::<F128>(want, mode), "{n:?} cos={cos} {mode:?}"),
                    None => tally.undecided += 1,
                }
            }
        }
        // Step 7: tan against the 384-bit quotient, sincos against the same roundings.
        let t = s.div(c);
        match tan_kernel(n) {
            None => tally.refused += 1,
            Some((neg, m, e)) => {
                let rel = (value(neg, m, e).sub(t).to_f64() / t.to_f64()).abs();
                assert!(rel < (-f64::from(B)).exp2(), "{n:?} tan: error {rel:e}");
                tally.worst = tally.worst.max(rel);
            }
        }
        for mode in MODES {
            tally.calls += 3;
            match tan::<F128>(n, mode) {
                Some(got) => assert_eq!(got, mp::round::<F128>(t, mode), "{n:?} tan {mode:?}"),
                None => tally.undecided += 1,
            }
            let both = sin_and_cos::<F128>(n, mode).unwrap_or((None, None));
            for (got, want) in [(both.0, s), (both.1, c)] {
                match got {
                    Some(got) => assert_eq!(got, mp::round::<F128>(want, mode), "{n:?} sincos {mode:?}"),
                    None => tally.undecided += 1,
                }
            }
        }
    }

    /// Step 6 against the 384-bit quotient: random significands and the extremes (`δ = 1/2`,
    /// where `v0 = 2^63` and the Newton step is zero; `δ` just above it, where the step is
    /// negative; `ν`, `δ` at the top of the range).
    #[test]
    fn quotient_meets_its_bound() {
        let full = |h: u128, l: u64| W3 { h: h | 1 << 127, l };
        let mut cases = vec![
            (full(0, 0), full(0, 0)),
            (full(u128::MAX, u64::MAX), full(0, 0)),
            (full(0, 0), full(u128::MAX, u64::MAX)),
            (full(u128::MAX, u64::MAX), full(u128::MAX, u64::MAX)),
            (full(u128::MAX, u64::MAX), full(0, 1)),
            (full(0, 1), full(1 << 64, 0)),
            (full(S0.h, S0.l), full((1 << 75) - 1, u64::MAX)),
        ];
        let mut g = Rng(0x0123_4567_89ab_cdef);
        for _ in 0..20_000 {
            let mut w = || full((u128::from(g.next()) << 64) | u128::from(g.next()), g.next());
            cases.push((w(), w()));
        }
        let mut worst = 0f64;
        for (n, d) in cases {
            let q = div3(n, d);
            assert!(q.h >> 124 != 0 && q.h >> 127 == 0, "{n:?} / {d:?}: {q:?} out of range");
            let want = value(false, n, 0).div(value(false, d, 0));
            let rel = (value(false, q, -190).sub(want).to_f64() / want.to_f64()).abs();
            assert!(rel < (-162f64).exp2(), "{n:?} / {d:?}: error {rel:e}");
            worst = worst.max(rel);
        }
        eprintln!("wide_int::quad div3: worst error 2^{:.1}", worst.log2());
    }

    /// A 320-bit unsigned integer, least significant word first.
    type Big = [u64; 5];

    fn bits(a: &Big) -> u32 {
        (0..5).rev().find(|&i| a[i] != 0).map_or(0, |i| 64 * i as u32 + 64 - a[i].leading_zeros())
    }

    fn below(a: &Big, b: &Big) -> bool {
        (0..5).rev().find(|&i| a[i] != b[i]).is_some_and(|i| a[i] < b[i])
    }

    fn shl(a: &Big, s: u32) -> Big {
        let (ws, bs) = ((s / 64) as usize, s % 64);
        core::array::from_fn(|i| {
            let x = if i >= ws { a[i - ws] } else { 0 };
            let y = if i > ws && bs != 0 { a[i - ws - 1] >> (64 - bs) } else { 0 };
            if bs == 0 { x } else { (x << bs) | y }
        })
    }

    fn sub(a: &mut Big, b: &Big) {
        let mut borrow = false;
        for (x, &y) in a.iter_mut().zip(b) {
            let (v, b1) = x.overflowing_sub(y);
            let (v, b2) = v.overflowing_sub(u64::from(borrow));
            *x = v;
            borrow = b1 || b2;
        }
        assert!(!borrow);
    }

    /// `(⌊a/b⌋, a mod b)`, `None` when the quotient reaches 2^120.
    fn divmod(a: &Big, b: &Big) -> Option<(u128, Big)> {
        let mut r = *a;
        let (na, nb) = (bits(a), bits(b));
        if na < nb {
            return Some((0, r));
        }
        if na - nb >= 120 {
            return None;
        }
        let mut q = 0u128;
        for s in (0..=na - nb).rev() {
            let t = shl(b, s);
            if !below(&r, &t) {
                sub(&mut r, &t);
                q |= 1 << s;
            }
        }
        Some((q, r))
    }

    /// `frac(2^e·2/π)` to 256 bits (units of 2^-256), `e >= -192`.
    fn alpha(e: i32) -> Big {
        let p = (e + 192) as usize;
        let (w, r) = (p / 64, (p % 64) as u32);
        let g = |i: usize| {
            let a = word(w + i);
            if r == 0 { a } else { (a << r) | (word(w + i + 1) >> (64 - r)) }
        };
        [g(3), g(2), g(1), g(0), 0]
    }

    /// The operand `q·2^e` closest to a multiple of π/2 that the continued fraction of
    /// `frac(2^e·2/π)` gives (Muller, Elementary Functions, 3rd ed. 2016, ch. 11): the last
    /// convergent denominator below 2^113 (the 256-bit expansion's convergents are the number's
    /// own that far), normalised to a binary128 significand.
    fn near_multiple(e: i32) -> Option<Num> {
        let (mut num, mut den) = (alpha(e), [0, 0, 0, 0, 1]);
        let (mut q2, mut q1) = (1u128, 0u128);
        let mut best = None;
        while bits(&den) != 0 {
            let Some((a, rem)) = divmod(&num, &den) else { break };
            let Some(q) = a.checked_mul(q1).and_then(|v| v.checked_add(q2)) else { break };
            if q >> 113 != 0 {
                break;
            }
            if q >= 2 {
                best = Some(q);
            }
            (q2, q1) = (q1, q);
            (num, den) = (den, rem);
        }
        best.map(|q| {
            let lz = q.leading_zeros() - 15;
            Num { neg: false, kind: Kind::Finite, sig: q << lz, exp: e - lz as i32 }
        })
    }

    /// The near multiples of π/2 for every `step`-th exponent: each checked like the random
    /// operands; none may meet the `|d| < 2^-160` refusal. Returns the largest number of leading
    /// zeros of `|f|` (448-bit form) seen.
    fn near_multiples(step: usize, tally: &mut Tally) -> u32 {
        let mut closest = 0;
        for e in (-170..=16271).step_by(step) {
            let Some(n) = near_multiple(e) else { continue };
            if n.exp + 112 < -58 {
                continue;
            }
            let r = reduce(n.sig, n.exp);
            assert!(r.is_some(), "{n:?} refused");
            if let Some(r) = r.filter(|r| r.j == 0) {
                closest = closest.max(r.kd);
            }
            let before = tally.refused;
            check(&n, tally);
            assert_eq!(tally.refused, before, "{n:?} refused");
        }
        closest
    }

    #[test]
    fn tier_agrees_with_the_wide_route() {
        let mut g = Rng(0x9e37_79b9_7f4a_7c15);
        let mut tally = Tally::default();
        for band in BANDS {
            for _ in 0..300 {
                check(&operand(&mut g, band), &mut tally);
            }
        }
        assert!(tally.undecided * 1000 <= tally.calls, "{} of {} undecided", tally.undecided, tally.calls);
        // Near a multiple of π/2 one of sin, cos is ±(1 − r²/2) within 2^-152 of a representable
        // value: undecided by design (the 384-bit route settles it). With them: the smallest
        // operand the tier takes, the binary128 values nearest π/2 and π, the largest value.
        let mut near = Tally::default();
        let half_pi = (S0.h >> 14) + ((S0.h >> 13) & 1);
        for (sig, exp) in [(1u128 << 112, -170), (half_pi, -112), (half_pi, -111), ((1 << 113) - 1, 16271)] {
            for neg in [false, true] {
                check(&Num { neg, kind: Kind::Finite, sig, exp }, &mut near);
            }
        }
        let closest = near_multiples(61, &mut near);
        eprintln!(
            "wide_int::quad: {} calls, {} undecided, worst error 2^{:.1}; near multiples: {} calls, {} undecided, worst 2^{:.1}, closest |f| 2^-{}",
            tally.calls,
            tally.undecided,
            tally.worst.log2(),
            near.calls,
            near.undecided,
            near.worst.log2(),
            closest + 1
        );
    }

    /// The brief's sweep: 10^7 seeded operands over the four bands in the four modes, and the near
    /// multiples of π/2 at every exponent.
    /// `cargo test --release -p liella-math --lib wide_int::quad -- --ignored`.
    #[test]
    #[ignore = "10^7 operands against the 384-bit route: run in release as a job"]
    fn tier_agrees_with_the_wide_route_long() {
        const TOTAL: usize = 10_000_000;
        let threads = std::thread::available_parallelism().map_or(4, std::num::NonZero::get);
        let tallies: Vec<Tally> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..threads)
                .map(|i| {
                    s.spawn(move || {
                        let mut g = Rng(0x2545_f491_4f6c_dd1d ^ (i as u64 + 1).wrapping_mul(0x9e37_79b9_7f4a_7c15));
                        let mut tally = Tally::default();
                        for k in (i..TOTAL).step_by(threads) {
                            check(&operand(&mut g, BANDS[k % 4]), &mut tally);
                        }
                        tally
                    })
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap_or_else(|p| std::panic::resume_unwind(p))).collect()
        });
        let mut all = Tally::default();
        for t in &tallies {
            all.calls += t.calls;
            all.undecided += t.undecided;
            all.refused += t.refused;
            all.worst = all.worst.max(t.worst);
        }
        let mut near = Tally::default();
        let closest = near_multiples(1, &mut near);
        eprintln!(
            "wide_int::quad long: {} operands, {} calls, {} undecided, {} refused, worst error 2^{:.2}; near multiples: {} calls, {} undecided, worst 2^{:.2}, closest |f| 2^-{}",
            TOTAL,
            all.calls,
            all.undecided,
            all.refused,
            all.worst.log2(),
            near.calls,
            near.undecided,
            near.worst.log2(),
            closest + 1
        );
        assert!(all.undecided * 100_000 <= all.calls, "{} of {} undecided", all.undecided, all.calls);
    }
}
