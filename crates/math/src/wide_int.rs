//! The integer fixed-point first tier of the x87 `long double` sine and cosine (spec §10.3,
//! D-10.52): `sin x` or `cos x` evaluated in 64-bit limbs with a proved relative error below
//! `2^-94`, and the rounding test of Ziv's strategy on it as an integer test ([`round`]). When
//! every value within `2^-92` of the approximation rounds alike in the caller's direction, that
//! rounding is the correctly rounded result; the four directions are four integer roundings (no
//! MXCSR or x87 control-word traffic, so directed modes cost the same). Otherwise (about `2^-26`
//! of the operands) the caller takes the 384-bit route of [`crate::wide`]. The format with a
//! 64-bit significand uses this kernel; `_Float128` uses [`quad`], the same method on three limbs
//! (D-10.85), which shares the `2/π` table, the `SC` table and the products here.
//!
//! Method. Payne-Hanek reduction (M. Payne, R. Hanek, "Radian reduction for trigonometric
//! functions", SIGNUM Newsletter 18(1), 1983) for every operand, Tang's table method (P. T. P.
//! Tang, "Table-lookup algorithms for elementary functions and their error analysis", ARITH 10,
//! 1991) and fixed-point Horner evaluation in 64-bit words (R. Brent, P. Zimmermann, Modern
//! Computer Arithmetic, 2010, ch. 4). Constants from Python `decimal` at 160 digits (π by
//! Machin's formula), each pinned against the 384-bit arithmetic by `constants_are_the_rounded_values`;
//! the bits of `2/π` are the generated table `TWO_OVER_PI`, pinned by `two_over_pi_table_times_pi_is_two`.
//!
//! Proof of the bound.
//! 1. Reduction. `|x| = m·2^e`, `2^63 <= m < 2^64`, `-97 <= e <= 16320` (the caller sends
//!    `2^-34 <= |x|`). Bit `j` of `2/π` weighs `2^-j`; for `j <= e - 2` the term `m·2^(e-j)` is a
//!    multiple of 4 and is dropped. The 256-bit window `W` of bits `e - 1 ..= e + 254` (zero for
//!    `j < 1`) gives `|x|·2/π = m·W·2^-254 + δ (mod 4)`, `0 <= δ < m·2^-254 < 2^-190`. `P = m·W mod
//!    2^256` is exact (three 64x64->128 products and one low product); its top two bits are the
//!    quadrant and `4P mod 2^256` the fraction in units of `2^-256`, centred to `f` in `[-1/2, 1/2)`
//!    with the quadrant `q` rounded alike: `|x| = (q + f + δ)·π/2`.
//! 2. Table. `j = floor(128|f| + 1/2)` in `[0, 64]`, `d = |f| - j/128` in `[-1/256, 1/256)`. With
//!    `a = f·π/2`, `sin|a| = S_j·C(d) + C_j·S(d)` and `cos|a| = C_j·C(d) - S_j·S(d)`, where `S_j =
//!    sin(jπ/256)`, `C_j = cos(jπ/256)` (128-bit fixed point, within `2^-129`), `S(d) = sin(πd/2)`
//!    (odd), `C(d) = cos(πd/2)` (even). `|d|` is normalised to `D·2^(-128-k)`, `D` in `[2^127,
//!    2^128)`, truncated by less than `2^-127` relative. The tier refuses `|d| < 2^-92`, so that
//!    for `j = 0` (`d = f`) `δ` stays below `2^-98` relative; the continued-fraction sweep of the
//!    tests finds no operand that reaches this refusal.
//! 3. Polynomials in `u = d² <= 2^-16` (units `2^-142`, within `6·2^-142`; its top word in units
//!    `2^-78`): `S(d) = d·Σ_{k<=4} (-1)^k s_k u^k`, `s_k = (π/2)^(2k+1)/(2k+1)!`, and `C(d) =
//!    Σ_{k<=5} (-1)^k c_k u^k`, `c_k = (π/2)^(2k)/(2k)!`. The series alternate with decreasing
//!    terms (`|πd/2| <= π/512`): the remainders are below `2^-98.7` (relative, S) and `2^-117` (C).
//!    Horner from the innermost term, each step a constant minus a smaller product (every partial
//!    sum is positive): the inner steps in 64-bit words, the last two in 128 bits. The 64-bit step
//!    `s_2 - u·(...)` is within `2^-66.4` and reaches `S` multiplied by `u² <= 2^-32`; every other
//!    step's error is below `2^-108` there: `S(d)` within `2^-97.9` relative. Likewise `c_2 -
//!    u·(...)`, within `2^-64.4`, gives `2^-96.4`: `C(d)` within `2^-96.3` absolute.
//! 4. Result. `j = 0`: `S(d)` alone (with `δ`, below `2^-96.9` relative) or `C(d)` (`C >= 0.99`,
//!    below `2^-96.2`). `j >= 1`: two 128-bit products, each truncated by less than `2^-125`;
//!    `sin|a| >= sin(π/512)` and `S_j <= 2·sin|a|`, so the error is below `2·2^-96.3 + 2^-97.9 +
//!    2^-115 < 2^-94.8` relative; `cos|a| >= cos(π/4)`: below `2^-95.7`. The approximation is within
//!    `2^-94` relative; the rounding test assumes `2^-92` ([`B`]).
//! 5. Rounding test: as `wide_dd::decide`, on the 128-bit significand `M` (top bit set) of the
//!    approximation: the true value lies within `2^(128 - B) + 4` units of `M`'s last place, so when
//!    `M`'s low 64 bits are at least that far from a representable value and from a midpoint, every
//!    value in the interval rounds alike in every direction and none is exact. The results lie in
//!    `[2^-80, 1]`: always normal, never overflowing.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::as_conversions,
    clippy::indexing_slicing,
    clippy::unreadable_literal
)] // fixed-point words, exponents and table indices of bounded range (each noted where it is formed)

pub(crate) mod arc;
pub(crate) mod quad;

use crate::coeffs::tables as t;
use crate::float::{Float, Kind, Num};
use crate::status::{Flags, Rounding, Status};

/// The rounding test's bound exponent: the approximation is within `2^-B` relative (proved
/// `2^-94`, see the module documentation).
const B: u32 = 92;
/// The rounding test's bound in units of the 128-bit significand's last place.
const ERR: u64 = (1 << (128 - B)) + 4;
/// `|d|` below `2^-92` (the top 128 bits of its 256-bit form below `2^36`) is refused.
const MIN_D: u128 = 1 << 36;
pub(crate) const M64: u128 = (1 << 64) - 1;

// The series of `sin(πd/2)/d` and `cos(πd/2)` in `u = d²` (step 3), each coefficient rounded to
// nearest in the units noted.
const S4: u64 = 0xa83c1a43f73c0dc8; // (π/2)^9/9!, units 2^-76
const S3: u64 = 0x9969667315ec2df3; // (π/2)^7/7!, units 2^-71
const S2: u64 = 0xa335e33bad570e92; // (π/2)^5/5!, units 2^-67
const S1: u128 = 0xa55de731_2df295f5_5dc72f71_2af24826; // (π/2)^3/3!, units 2^-128
const S0: u128 = 0x6487ed51_10b4611a_62633145_c06e0e69; // π/2, units 2^-126
const C5: u64 = 0xd368f95102545d4d; // (π/2)^10/10!, units 2^-79
const C4: u64 = 0xf0fa83448dd5d7a3; // (π/2)^8/8!, units 2^-74
const C3: u64 = 0xaae9e3f1e5ffcfe3; // (π/2)^6/6!, units 2^-69
const C2: u64 = 0x81e0f840dad61d9b; // (π/2)^4/4!, units 2^-65
const C1: u128 = 0x4ef4f326_f9177969_2b71366c_c0460d64; // (π/2)^2/2!, units 2^-126

/// `f(x)` rounded into `F` in `mode` when the tier decides it (`cos`: cosine, else sine); `None`
/// sends the caller to the 384-bit route. `n` is finite, nonzero and at least `2^-34` in
/// magnitude (smaller operands are settled by the caller).
pub(crate) fn sin_cos<F: Float>(n: &Num, cos: bool, mode: Rounding) -> Option<(F, Status)> {
    if F::PREC == 113 {
        return quad::sin_cos::<F>(n, cos, mode);
    }
    if F::PREC != 64 || !F::EXPLICIT {
        return None;
    }
    let (neg, m, e) = kernel(n, cos)?;
    round::<F>(neg, m, e, mode)
}

/// `(sign, M, E)` with `M` in `[2^127, 2^128)` and `±M·2^E` within `2^-94` (relative) of `sin x`
/// (`cos x`), or `None` outside the range of step 1 or for `|d| < 2^-92` (step 2).
fn kernel(n: &Num, cos: bool) -> Option<(bool, u128, i32)> {
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
    // sin|x| = sin(qπ/2 + a), cos|x| = sin((q + 1)π/2 + a); an odd quadrant takes cos a.
    let k = q + u32::from(cos);
    let odd = k & 1 != 0;
    let neg = (!cos && n.neg) ^ (k & 2 != 0) ^ (!odd && negf);
    // j = round(128|f|), d = |f| - j/128 (256 bits; the high word as a signed value).
    let j = ((gh + (1 << 120)) >> 121) as usize; // gh <= 2^127: j <= 64
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
    let kd = dh.leading_zeros(); // |d| <= 2^-8: 7 <= kd <= 91
    let d = (dh << kd) | (dl >> (128 - kd)); // |d| = d·2^(-128-kd)
    let u = mulhi(d, d).checked_shr(2 * kd - 14).unwrap_or(0); // d², units 2^-142
    let u1 = (u >> 64) as u64; // units 2^-78
    if j == 0 {
        return Some(if odd {
            let c = cos_poly(u, u1); // units 2^-127
            let l = c.leading_zeros();
            (neg, c << l, -127 - l as i32)
        } else {
            let s = mulhi(d, sin_poly(u, u1)); // s·2^(-126-kd)
            let l = s.leading_zeros();
            (neg, s << l, -126 - kd as i32 - l as i32)
        });
    }
    let s = mulhi(d, sin_poly(u, u1)) >> (kd - 7); // S(|d|), units 2^-133
    let c = cos_poly(u, u1); // C(d), units 2^-127
    let [sj, cj] = SC[j - 1];
    let r = if odd {
        let (a, b) = (mulhi(cj, c), mulhi(sj, s) >> 6);
        if sd { a + b } else { a - b }
    } else {
        let (a, b) = (mulhi(sj, c), mulhi(cj, s) >> 6);
        if sd { a - b } else { a + b }
    }; // units 2^-127, at least sin(π/512)
    let l = r.leading_zeros();
    Some((neg, r << l, -127 - l as i32))
}

/// Word `i` of the bits of `2/π` behind two zero words (bit `j` of `2/π` at bit `j + 127`).
fn word(i: usize) -> u64 {
    match i.checked_sub(2) {
        Some(k) => t::TWO_OVER_PI[k],
        None => 0,
    }
}

/// Step 1: `(q mod 4, f < 0, |f|)` for `|x| = m·2^e`, `|f|` in 256 bits (high, low) in units of
/// `2^-256`.
fn reduce(m: u64, e: i32) -> (u32, bool, u128, u128) {
    let p = (e + 126) as usize; // bit e - 1 of 2/π; e >= -97
    let (w, r) = (p / 64, (p % 64) as u32);
    let g = |i: usize| {
        let a = word(w + i);
        if r == 0 { a } else { (a << r) | (word(w + i + 1) >> (64 - r)) }
    };
    let m = u128::from(m);
    let p3 = m * u128::from(g(3));
    let p2 = m * u128::from(g(2));
    let p1 = m * u128::from(g(1));
    let s1 = (p3 >> 64) + (p2 & M64);
    let s2 = (s1 >> 64) + (p2 >> 64) + (p1 & M64);
    let l3 = ((s2 >> 64) as u64).wrapping_add((p1 >> 64) as u64).wrapping_add((m as u64).wrapping_mul(g(0)));
    let q = (l3 >> 62) as u32;
    let gh = (u128::from(l3) << 66) | ((s2 & M64) << 2) | ((s1 & M64) >> 62);
    let gl = ((s1 & M64) << 66) | ((p3 & M64) << 2);
    if gh >> 127 != 0 {
        let lo = gl.wrapping_neg();
        ((q + 1) & 3, true, (!gh).wrapping_add(u128::from(lo == 0)), lo)
    } else {
        (q, false, gh, gl)
    }
}

/// `⌊a·b/2^128⌋` less at most 2 (the low product and two carries dropped).
#[inline]
pub(crate) fn mulhi(a: u128, b: u128) -> u128 {
    let (ah, al, bh, bl) = (a >> 64, a & M64, b >> 64, b & M64);
    ah * bh + ((ah * bl) >> 64) + ((al * bh) >> 64)
}

/// `⌊a·b/2^64⌋` less at most 1.
#[inline]
pub(crate) fn mul_wide(a: u128, b: u64) -> u128 {
    let b = u128::from(b);
    (a >> 64) * b + (((a & M64) * b) >> 64)
}

/// `v as u64` for an integer-valued `v` in `[2^52, 2^64]` (`2^64` gives `2^64 − 1`, as `as` saturates),
/// read from the bits: no SSE conversion. `as u64` of a value at or above `2^63` converts it
/// signed first, which overflows and raises the invalid flag in MXCSR on every call; the
/// envelope's exit drops that flag again with an MXCSR write, and on Ivy Bridge the next call's
/// MXCSR read then stalls in microcode (LIBM-LD-ARC-RETHINK-2's attribution: ~58 microcode uops a
/// call, successive asinl calls no longer overlapping). The root's seed `s0` and its reciprocal
/// lie in `[2^63, 2^64]` on every call; arc's `div` and quad's `div3` reciprocals reach `2^63` when
/// the divisor's top word is below `2^63 + 2^11`, and arc's `soft_sqrt` passes `[2^52, 2^54)` (D-10.95).
#[inline(always)]
pub(crate) fn big_u64(v: f64) -> u64 {
    debug_assert!(v >= (1u64 << 52) as f64);
    let b = v.to_bits();
    let sh = ((b >> 52) as u32).wrapping_sub(1075); // v = m·2^sh, sh in [0, 12]
    let m = (b & ((1 << 52) - 1)) | (1 << 52);
    if sh >= 12 { u64::MAX } else { m << sh }
}

#[inline]
pub(crate) fn mulhi64(a: u64, b: u64) -> u64 {
    ((u128::from(a) * u128::from(b)) >> 64) as u64
}

/// `sin(πd/2)/d` in units of `2^-126` (`u` = d² in units of `2^-142`, `u1` its top word).
#[inline]
fn sin_poly(u: u128, u1: u64) -> u128 {
    let a = S3 - (mulhi64(u1, S4) >> 19); // units 2^-71
    let b = S2 - (mulhi64(u1, a) >> 18); // units 2^-67
    let c = S1 - (mul_wide(u, b) >> 17); // units 2^-128
    S0 - (mulhi(u, c) >> 16)
}

/// `cos(πd/2)` in units of `2^-127`.
#[inline]
fn cos_poly(u: u128, u1: u64) -> u128 {
    let a = C4 - (mulhi64(u1, C5) >> 19); // units 2^-74
    let b = C3 - (mulhi64(u1, a) >> 19); // units 2^-69
    let c = C2 - (mulhi64(u1, b) >> 18); // units 2^-65
    let h = C1 - (mul_wide(u, c) >> 17); // units 2^-126
    (1 << 127) - (mulhi(u, h) >> 13)
}

/// Step 5: `±m·2^e` rounded into `F` (64-bit significand) in `mode`, or `None` when the bound
/// does not decide it.
pub(crate) fn round<F: Float>(neg: bool, m: u128, e: i32, mode: Rounding) -> Option<(F, Status)> {
    let lo = m as u64;
    // The distance above the last representable value or midpoint must lie in [ERR, 2^63 - ERR].
    if (lo & (u64::MAX >> 1)).wrapping_sub(ERR) > (1 << 63) - 2 * ERR {
        return None;
    }
    let up = match mode {
        Rounding::Nearest => lo >> 63 != 0,
        Rounding::Zero => false,
        Rounding::Up => !neg,
        Rounding::Down => neg,
    };
    let (sig, e) = match ((m >> 64) as u64).checked_add(u64::from(up)) {
        Some(s) => (s, e),
        None => (1 << 63, e + 1),
    };
    // sig·2^(e + 64), sig in [2^63, 2^64): biased exponent e + 127 + bias.
    let be = e + 127 + F::EMAX;
    if be <= 0 || be > 2 * F::EMAX {
        return None;
    }
    let bits = (u128::from(neg) << 79) | ((be as u128) << 64) | u128::from(sig);
    Some((F::from_bits(bits), Status::flags(Flags::INEXACT)))
}

/// `[sin(jπ/256), cos(jπ/256)]` for `j = 1..=64` (row `j - 1`), in units of `2^-128`.
const SC: [[u128; 2]; 64] = [
    [0x03243a3f_9bd8f08c_c3c75f41_b6ce7aa8, 0xfffb10b4_dc96dabb_b47903f7_a19f8ee2],
    [0x0648557d_e8d99f7e_4e29cf6e_5fed0679, 0xffec4304_266865d9_56575523_66961732],
    [0x096c32ba_ca2ae68b_437b2dd4_9d5fca3c, 0xffd3977f_f7bae4e9_664649b4_d541b9c5],
    [0x0c8fb2f8_86ec09f3_76a17954_b2b7c517, 0xffb10f1b_cb6bef1d_421e8eda_af59453e],
    [0x0fb2b73c_fc106ff6_8f0a0e36_a000c735, 0xff84ab2c_738d6a03_519c3149_73ccae6b],
    [0x12d52092_ce19f5cc_beeeae81_29a786b9, 0xff4e6d68_0c41d0a9_0f668633_f1ab858a],
    [0x15f6d00a_9aa418c1_7fba5f81_26cb2986, 0xff0e57e5_ead848d1_1f190154_4271c3f8],
    [0x1917a6bc_29b42be1_d8e72d91_2977ee71, 0xfec46d1e_89292cf0_41390efd_c726e9ef],
    [0x1c3785c7_9ec2d4f5_86310766_026f8dcc, 0xfe70afeb_6d33d6a2_2907cf2b_3f6feac2],
    [0x1f564e56_a9730e34_4e08e535_cadaf147, 0xfe132387_0cfe9a3d_90cd1d95_9db674ef],
    [0x2273e19d_b5eaed56_e5e59726_1830d391, 0xfdabcb8c_aeba091b_fac7397c_c07a6470],
    [0x259020dd_1cc27444_c002a268_4781f080, 0xfd3aabf8_4528b50b_eae6bd95_1c1dabbe],
    [0x28aaed62_527cb3b5_e976fb98_213b8498, 0xfcbfc926_484cd43a_a3e22b4d_38917e73],
    [0x2bc42889_167f8ca9_8ffbbcee_d62c7c43, 0xfc3b27d3_8a5d49ab_256778ff_cb5c1769],
    [0x2edbb3bc_a17e628e_af776280_d975ac75, 0xfbaccd1d_0903bb09_e63ae863_2b84473c],
    [0x31f17078_d34c156c_97323003_93f33614, 0xfb14be7f_bae58156_2172a361_fd2a722f],
    [0x3505404b_6008a13c_37d0017b_da992a81, 0xfa7301d8_59796671_1fe196a5_3fb5b237],
    [0x381704d4_fc9ec5f9_43af186b_79b2a0f3, 0xf9c79d63_272c4628_4504ae08_d19b2980],
    [0x3b269fca_8a8622ba_dc913b88_3d64660f, 0xf91297bb_b1d6cdbe_68fc6e4d_6a920bd2],
    [0x3e33f2f6_42be355e_90887712_e9dc9663, 0xf853f7dc_9186b952_c7adc6b4_988891bb],
    [0x413ee038_dff6b7fd_7da6491b_4f867804, 0xf78bc51f_239e12c6_214cffce_e9dd33ca],
    [0x4447498a_c7d9dd82_4c20ab7a_a99a2183, 0xf6ba073b_424b19e8_2c791f59_cc1ffc23],
    [0x474d10fd_336cf746_f95f1f65_7313c4ea, 0xf5dec646_f85ba1c6_c8c615e7_2768d6b5],
    [0x4a5018bb_567c16a2_d725d3b9_ed35fbaa, 0xf4fa0ab6_316ed2ec_163c5c7f_03b718c5],
    [0x4d50430b_860546c3_4ffd0691_f9e13631, 0xf40bdd5a_6688662f_5019794a_1f5896e5],
    [0x504d7250_5d98050c_97c4afa2_5181e605, 0xf3144762_47088f74_a5486bdc_455d56a2],
    [0x53478909_e39da892_0b90c3c9_42bfa62d, 0xf2135259_5e0bf350_e7112e89_103cc0c7],
    [0x563e69d6_ac7f73f8_408fca9c_c277fc1f, 0xf1090827_b43725fd_67127db3_5b287316],
    [0x5931f774_fc9f1844_34ad2999_0485d84e, 0xeff57311_6df1555d_62aef7b5_5319d1d4],
    [0x5c2214c3_e9167abb_4e61f79b_3a36f1dc, 0xeed89db6_6611e307_86f8c20f_b664b01b],
    [0x5f0ea4c4_77339c06_68fc87bc_fa363f00, 0xedb29311_c504d652_11815196_b9fbf5df],
    [0x61f78a9a_baa58b46_98916152_cf7eee1c, 0xec835e79_946a3145_7e610231_ac1d6181],
    [0x64dca98e_f24f5cb4_218cf2d6_ad86e5c2, 0xeb4b0b9e_4f345617_39e39c6c_2ab3655d],
    [0x67bde50e_a3b628b6_d409485e_dd56b172, 0xea09a68a_6e49cd62_15ad45b4_a1b5e823],
    [0x6a9b20ad_b4ff262a_0ef911a3_308e35a6, 0xe8bf3ba1_f1aedfbb_f8972aff_b3d98e1f],
    [0x6d744027_857300ad_9b165cba_0c171818, 0xe76bd7a1_e63b9786_12512952_9d48a92f],
    [0x70492760_047b9a7e_c5544aea_df9f4275, 0xe60f879f_e7e2e1e5_7613b68f_6ab03130],
    [0x7319ba64_c711785a_1439670d_fe3d68e6, 0xe4aa5909_a08fa7b4_122785ae_67f5515d],
    [0x75e5dd6e_1b8e2555_0ec87bc0_574a88a0, 0xe33c59a4_439cd8ec_36563e2f_fad8351a],
    [0x78ad74e0_1bd8ec78_362474f1_a105878f, 0xe1c5978c_05ed8691_f4e8a837_2f8c5810],
    [0x7b70654b_bde35622_f083cc12_119ff7a3, 0xe0462133_92aa486c_55ff6038_a5197367],
    [0x7e2e936f_e26ae7ed_13e03e48_89485c69, 0xdebe0563_7ca94cfb_4b19aa71_fec3ae6d],
    [0x80e7e43a_61f5b6cb_5ca183dc_973abc22, 0xdd2d5339_ac8692fd_49c6e0ea_76cbcaac],
    [0x839c3cc9_17ff6cb4_bfd79717_f2880abf, 0xdb941a28_cb71ec87_2c19b632_53da43fc],
    [0x864b826a_ec4c74e5_85043222_c9bdd18d, 0xd9f269f7_aab88c29_28e81dcb_6dab91ac],
    [0x88f59aa0_da591421_b892ca83_61d8c84c, 0xd84852c0_a80ffcdb_24b9fe00_663574a4],
    [0x8b9a6b1e_f6da4502_21a6675f_51580bc4, 0xd695e4f1_0ea88570_083f082b_570611d7],
    [0x8e39d9cd_73464364_bba4cfec_bff54867, 0xd4db3148_750d1819_f630e8b6_dac83e69],
    [0x90d3ccc9_9f5ac58b_09d1072e_09b72292, 0xd31848d8_17d70e16_eeeaddb7_2f00e0dd],
    [0x93682a66_e896f544_b1782191_1e71c16e, 0xd14d3d02_313c0eed_744fea20_e8abef92],
    [0x95f6d92f_d79f4fba_d9f8e1a4_46e973b9, 0xcf7a1f79_4d7ca1b1_dfcb6044_5c1bf973],
    [0x987fbfe7_0b81a708_19cec845_ac87a5c6, 0xcd9f023f_9c3a059e_23af31db_7179a4aa],
    [0x9b02c588_32cf95c0_698b94f5_0326a043, 0xcbbbf7a6_3eba0dd5_70cbb7f3_343451be],
    [0x9d7fd149_0285c9e3_e25e3954_9638ae68, 0xc9d1124c_931fda7a_8335241b_e1693225],
    [0x9ff6ca9a_2ab6a26d_22cc118a_0c118aa0, 0xc7de651f_7ca06749_02b31bc8_6877fd2c],
    [0xa2679928_48eeb0c0_3b5167ee_359a234e, 0xc5e40358_a8ba05a7_43da25d9_9267326b],
    [0xa4d224dc_d849c5b0_23d251cc_8d7975cc, 0xc3e2007d_d175f5a4_a87e7813_6665cdb2],
    [0xa73655df_1f2f489e_149f6e75_993468a3, 0xc1d8705f_fcbb6e90_bdf0715c_b8b20bd7],
    [0xa9941495_1aacae5e_d147625f_da929af8, 0xbfc7671a_b8bb84c6_e4e62d86_dd136e78],
    [0xabeb49a4_6764fd15_1becda80_89c1a94c, 0xbdaef913_557d76f0_ac85320f_528d6d5d],
    [0xae3bddf3_280c620d_3d538178_65422565, 0xbb8f3af8_1b93095c_fce8d840_68e825b6],
    [0xb085baa8_e966f6da_e4cad00d_5c94bcd2, 0xb96841bf_7ffcb21a_9de1e3b2_2b8bf4db],
    [0xb2c8c92f_83c1eb87_ac9f7ebb_c469ef59, 0xb73a22a7_55457448_7f86f63b_b23f496a],
    [0xb504f333_f9de6484_597d89b3_754abe9f, 0xb504f333_f9de6484_597d89b3_754abe9f],
];

#[cfg(test)]
mod tests {
    // Spec clause: §10.3 (x87 sinl/cosl correctly rounded in the four directions, D-10.19).
    use super::*;
    use crate::float::{self, Exact, F80};
    use crate::mp::{self, Mp};

    pub(super) const MODES: [Rounding; 4] = [Rounding::Nearest, Rounding::Down, Rounding::Up, Rounding::Zero];

    /// `v·2^units` rounded to the nearest integer (these values are irrational: no ties).
    fn fixed(v: Mp, units: i64) -> u128 {
        let w = v.ldexp(units);
        let top = (u128::from(w.m[mp::LIMBS - 1]) << 64) | u128::from(w.m[mp::LIMBS - 2]);
        let sh = 128 - w.exp;
        assert!((0..128).contains(&sh), "{sh}");
        if sh == 0 { top + u128::from(w.m[mp::LIMBS - 3] >> 63) } else { (top >> sh) + ((top >> (sh - 1)) & 1) }
    }

    #[test]
    fn constants_are_the_rounded_values() {
        let half_pi = mp::pi().ldexp(-1);
        let mut p = vec![Mp::from_i64(1)]; // p[k] = (π/2)^k / k!
        for k in 1..=10u64 {
            let next = p[p.len() - 1].mul(half_pi).div_u64(k);
            p.push(next);
        }
        let pinned: [(u128, usize, i64); 10] = [
            (S0, 1, 126),
            (S1, 3, 128),
            (S2.into(), 5, 67),
            (S3.into(), 7, 71),
            (S4.into(), 9, 76),
            (C1, 2, 126),
            (C2.into(), 4, 65),
            (C3.into(), 6, 69),
            (C4.into(), 8, 74),
            (C5.into(), 10, 79),
        ];
        for (c, k, units) in pinned {
            assert_eq!(c, fixed(p[k], units), "(π/2)^{k}/{k}!");
        }
        let pi = mp::pi();
        for (i, &[s, c]) in SC.iter().enumerate() {
            let (ws, wc) = mp::sin_cos(pi.mul_u64(i as u64 + 1).ldexp(-8));
            assert_eq!(s, fixed(ws, 128), "sin({}π/256)", i + 1);
            assert_eq!(c, fixed(wc, 128), "cos({}π/256)", i + 1);
        }
    }

    /// π·T = 2 for the table T of the bits of 2/π, with π from Machin's formula (π = 16·atan(1/5)
    /// − 4·atan(1/239)) in 320-word fixed point: a flipped table bit j moves the product by
    /// π·2^-j, visible for every j up to 19905 (the x87 reduction reads bits below 16600).
    #[test]
    fn two_over_pi_table_times_pi_is_two() {
        const N: usize = 320; // fraction words; one more for the integer part
        fn div_small(a: &mut [u64], d: u64) {
            let mut r = 0u128;
            for x in a.iter_mut().rev() {
                let cur = (r << 64) | u128::from(*x);
                *x = (cur / u128::from(d)) as u64;
                r = cur % u128::from(d);
            }
        }
        fn add_sub(a: &mut [u64], b: &[u64], sub: bool) {
            let mut carry = false;
            for (x, &y) in a.iter_mut().zip(b) {
                let (v, c1) = if sub { x.overflowing_sub(y) } else { x.overflowing_add(y) };
                let (v, c2) = if sub { v.overflowing_sub(u64::from(carry)) } else { v.overflowing_add(u64::from(carry)) };
                *x = v;
                carry = c1 || c2;
            }
            assert!(!carry);
        }
        fn mul_small(a: &mut [u64], k: u64) {
            let mut c = 0u128;
            for x in a.iter_mut() {
                let p = u128::from(*x) * u128::from(k) + c;
                *x = p as u64;
                c = p >> 64;
            }
            assert_eq!(c, 0);
        }
        fn atan_inv(n: u64) -> Vec<u64> {
            let mut t = vec![0u64; N + 1];
            t[N] = 1;
            div_small(&mut t, n); // 1/n
            let mut sum = vec![0u64; N + 1];
            let mut k = 0u64;
            while t.iter().any(|&x| x != 0) {
                let mut term = t.clone();
                div_small(&mut term, 2 * k + 1);
                add_sub(&mut sum, &term, k % 2 == 1);
                div_small(&mut t, n * n);
                k += 1;
            }
            sum
        }
        let mut pi = atan_inv(5);
        mul_small(&mut pi, 16);
        let mut b = atan_inv(239);
        mul_small(&mut b, 4);
        add_sub(&mut pi, &b, true);
        assert_eq!(pi[N], 3);
        // Q = π·T, value Q·2^(-64·(N + 312)) = 2 − ε with ε below 2^-19966.
        let tl: Vec<u64> = t::TWO_OVER_PI.iter().rev().copied().collect();
        let mut q = vec![0u64; N + 1 + tl.len() + 1];
        for (i, &a) in pi.iter().enumerate() {
            let mut c = 0u128;
            for (k, &b) in tl.iter().enumerate() {
                let v = u128::from(a) * u128::from(b) + u128::from(q[i + k]) + c;
                q[i + k] = v as u64;
                c = v >> 64;
            }
            q[i + tl.len()] = c as u64;
        }
        let top = N + tl.len();
        assert_eq!(q[top], 1, "π·(2/π) not below 2");
        assert!(q[N + 1..top].iter().all(|&w| w == u64::MAX), "π·(2/π) not within 2^-19966 of 2");
    }

    #[test]
    fn rounding_test_refuses_near_a_boundary() {
        // The bound of step 5: 2^(128 − 92) + 4 units of the significand's last place.
        let err = (1u64 << 36) + 4;
        let half = 1u64 << 63;
        let cases = [
            (err - 1, false),
            (err, true),
            (half - err, true),
            (half - err + 1, false),
            (half + err - 1, false),
            (half + err, true),
            (u64::MAX - err + 1, true),
            (u64::MAX - err + 2, false),
        ];
        for hi in [0xc90f_daa2_2168_c234u64, u64::MAX] {
            for (lo, decided) in cases {
                let m = (u128::from(hi) << 64) | u128::from(lo);
                for neg in [false, true] {
                    for mode in MODES {
                        let got = round::<F80>(neg, m, -130, mode);
                        assert_eq!(got.is_some(), decided, "{lo:#x} {mode:?}");
                        if let Some(got) = got {
                            let (r, flags) = float::round::<F80>(Exact { neg, sig: m, exp: -130, sticky: true }, mode);
                            assert_eq!(got, (r, Status::flags(flags)), "{hi:#x} {lo:#x} {neg} {mode:?}");
                        }
                    }
                }
            }
        }
    }

    pub(super) struct Rng(pub(super) u64);

    impl Rng {
        pub(super) fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
    }

    /// Bands of `floor(log2 |x|)`: below 1, moderate, large, and up to the largest x87 value.
    const BANDS: [(i32, i32); 4] = [(-34, -1), (0, 12), (13, 70), (71, 16383)];

    fn operand(g: &mut Rng, (lo, hi): (i32, i32)) -> Num {
        let top = lo + (g.next() % (hi - lo + 1) as u64) as i32;
        Num { neg: g.next() & 1 != 0, kind: Kind::Finite, sig: u128::from(g.next() | 1 << 63), exp: top - 63 }
    }

    #[derive(Default)]
    pub(super) struct Tally {
        pub(super) calls: u64,
        pub(super) undecided: u64,
        pub(super) refused: u64,
        pub(super) worst: f64,
    }

    /// The tier on `n` against the 384-bit route in the four modes, and the kernel's error against
    /// the bound the rounding test assumes.
    fn check(n: &Num, tally: &mut Tally) {
        let (s, c) = mp::sin_cos_wide(n.neg, n.sig, i64::from(n.exp));
        for (cos, want) in [(false, s), (true, c)] {
            match kernel(n, cos) {
                None => tally.refused += 1,
                Some((neg, m, e)) => {
                    let rel = (Mp::from_u128(neg, m, i64::from(e)).sub(want).to_f64() / want.to_f64()).abs();
                    assert!(rel < (-f64::from(B)).exp2(), "{n:?} cos={cos}: error {rel:e}");
                    tally.worst = tally.worst.max(rel);
                }
            }
            for mode in MODES {
                tally.calls += 1;
                match sin_cos::<F80>(n, cos, mode) {
                    Some(got) => assert_eq!(got, mp::round::<F80>(want, mode), "{n:?} cos={cos} {mode:?}"),
                    None => tally.undecided += 1,
                }
            }
        }
    }

    /// `frac(2^e·2/π)` to 127 bits (units of 2^-127), `e >= -128`.
    fn alpha(e: i32) -> u128 {
        let p = (e + 128) as usize;
        let (w, r) = (p / 64, (p % 64) as u32);
        let hi = (u128::from(word(w)) << 64) | u128::from(word(w + 1));
        let v = if r == 0 { hi } else { (hi << r) | u128::from(word(w + 2) >> (64 - r)) };
        v >> 1
    }

    /// The operand `q·2^e` closest to a multiple of π/2 that the continued fraction of
    /// `frac(2^e·2/π)` gives (Muller, Elementary Functions, 3rd ed. 2016, ch. 11): the last
    /// convergent denominator below 2^62 (as far as the 127-bit expansion's convergents are the
    /// number's own).
    pub(super) fn near_multiple(e: i32) -> Option<Num> {
        let (mut num, mut den) = (alpha(e), 1u128 << 127);
        let (mut q2, mut q1) = (1u128, 0u128);
        let mut best = None;
        while den != 0 {
            let a = num / den;
            let Some(q) = a.checked_mul(q1).and_then(|v| v.checked_add(q2)) else { break };
            if q >= 1 << 62 {
                break;
            }
            if q >= 2 {
                best = Some(q);
            }
            (q2, q1) = (q1, q);
            (num, den) = (den, num - a * den);
        }
        best.map(|q| Num { neg: false, kind: Kind::Finite, sig: q, exp: e })
    }

    /// The near multiples of π/2 for every `step`-th exponent: each checked like the random
    /// operands; none may meet the `|d| < 2^-92` refusal. Returns the largest number of leading
    /// zeros of `|f|`'s top 128 bits seen.
    fn near_multiples(step: usize, tally: &mut Tally) -> u32 {
        let mut closest = 0;
        for e in (-60..=16320).step_by(step) {
            let Some(n) = near_multiple(e) else { continue };
            if n.exp + 127 - (n.sig.leading_zeros() as i32) < -34 {
                continue;
            }
            let lz = (n.sig as u64).leading_zeros();
            let (_, _, gh, _) = reduce((n.sig as u64) << lz, n.exp - lz as i32);
            closest = closest.max(gh.leading_zeros());
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
        // Near a multiple of π/2 one of sin, cos is ±(1 − r²/2) within 2^-92 of a representable
        // value: undecided by design (the 384-bit route settles it). With them: the smallest
        // operand the tier takes, the x87 values nearest π/2 and π, the largest value.
        let mut near = Tally::default();
        for (sig, exp) in [(1u64 << 63, -97), (0xc90f_daa2_2168_c235, -63), (0xc90f_daa2_2168_c235, -62), (u64::MAX, 16320)] {
            for neg in [false, true] {
                check(&Num { neg, kind: Kind::Finite, sig: sig.into(), exp }, &mut near);
            }
        }
        let closest = near_multiples(61, &mut near);
        eprintln!(
            "wide_int: {} calls, {} undecided, worst error 2^{:.1}; near multiples: {} calls, {} undecided, worst 2^{:.1}, closest |f| 2^-{}",
            tally.calls,
            tally.undecided,
            tally.worst.log2(),
            near.calls,
            near.undecided,
            near.worst.log2(),
            closest + 1
        );
    }

    /// The brief's sweep: 10^7 seeded operands over the four bands and the near multiples of π/2 at
    /// every exponent. `cargo test --release -p liella-math --lib wide_int -- --ignored`.
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
            "wide_int long: {} operands, {} calls, {} undecided, {} refused, worst error 2^{:.2}; near multiples: {} calls, {} undecided, worst 2^{:.2}, closest |f| 2^-{}",
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
