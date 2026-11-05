//! `float` fast paths for `exp`, `exp2`, `exp10`, `log`, `log2`, `log10` (Track M milestone 6,
//! M6-3 `MATH-M6-F32-CORE`), `log1p` (`MATH-M6-F32-REST`), `sin`, `cos`, `tan`, `sincos`
//! (`MATH-M6-F32-TRIG`, the accurate path being [`crate::trig`]; it reuses that module's `sin(kπ/64)` table)
//! and `pow` (`MATH-M6-F32-POWF`). Each evaluates `f(x)` in `double` with a table and a polynomial to a
//! proved relative error bound ε, then applies the rounding test ([`round`]). When the test fails
//! the caller runs the accurate double-double path ([`crate::exp_log`]) unchanged, so every result
//! and every flag is bit-identical to it in every rounding direction (D-10.13).
//!
//! The arithmetic is plain IEEE `double` (no FMA, so one variant serves every x86-64 CPU; ruling
//! #932's FMA variant is not written). It runs in the environment the envelope set up: MXCSR's
//! default control word (round to nearest, FTZ and DAZ off), either the caller's own or switched
//! to (`mathret::elem`). The final conversion to `float` is a hardware `cvtsd2ss`, so a caller's
//! flush-to-zero would be visible if the envelope ever ran this in place under FTZ.
//!
//! Tables (ruling #933): 3 KiB for both families together (EXP2_TAB 1 KiB, LOG_INV + LOG_C 2 KiB).
//! Generated (with the constants) by 80-digit decimal arithmetic; the tests re-check every entry
//! against the accurate path.
#![allow(clippy::as_conversions, clippy::arithmetic_side_effects, clippy::indexing_slicing)] // bit fields and exponents of bounded width; table indices masked to 0..128; `cvtsd2ss` is the `as f32`

use crate::coeffs::tables as t;
use crate::stats;
use crate::status::{Flags, Rounding, Status};

/// The functions with a fast path.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fast {
    /// `expf`.
    Exp,
    /// `exp2f`.
    Exp2,
    /// `exp10f`.
    Exp10,
    /// `logf`.
    Log,
    /// `log2f`.
    Log2,
    /// `log10f`.
    Log10,
    /// `log1pf` (MATH-M6-F32-REST).
    Log1p,
    /// `sinf` (MATH-M6-F32-TRIG).
    Sin,
    /// `cosf` (MATH-M6-F32-TRIG).
    Cos,
    /// `tanf` (MATH-M6-F32-TRIG).
    Tan,
    /// `expm1f` (MATH-M6-F32-REST-2).
    Expm1,
    /// `cbrtf` (MATH-M6-F32-REST-2).
    Cbrt,
    /// `asinf` (LIBM-F32-DOUBLE-KERNELS).
    Asin,
    /// `acosf` (LIBM-F32-DOUBLE-KERNELS).
    Acos,
    /// `atanf` (LIBM-F32-DOUBLE-KERNELS).
    Atan,
    /// `sinhf` (LIBM-F32-HYPERBOLIC).
    Sinh,
    /// `coshf` (LIBM-F32-HYPERBOLIC).
    Cosh,
    /// `tanhf` (LIBM-F32-HYPERBOLIC).
    Tanh,
    /// `asinhf` (LIBM-F32-HYPERBOLIC).
    Asinh,
    /// `acoshf` (LIBM-F32-HYPERBOLIC).
    Acosh,
    /// `atanhf` (LIBM-F32-HYPERBOLIC).
    Atanh,
}

/// The rounding test's margins, in units of the last place of the `double` approximation y:
/// a bound ε on the relative error gives |y - f| < ε·2^53/(1 - ε) ulp(y) (y < 2^(E+1) in
/// binade E). The proofs are at [`exp_core`] and [`log_core`].
/// The fast path's error bounds are far looser than a `float` needs (ε ≤ 2^-45 rather than
/// 2^-52, MATH-M6-F32-KERNEL): a shorter reduction and polynomial, at the price of about
/// 2·DELTA/2^28 < 4·10^-6 of the operands going to the accurate path.
/// exp: ε ≤ 2^-45.16, so < 229 ulp.
const EXP_DELTA: u64 = 512;
/// log: ε ≤ 2^-44.79 (case A dominates), so < 296 ulp; log2, log10 add the rounding of the
/// constant and of the product: ε ≤ 2^-44.77, < 300 ulp.
const LOG_DELTA: u64 = 640;
/// log1p: ε ≤ 6.8·2^-53 (proof at [`log1p_core`]), so < 6.9 ulp.
const LOG1P_DELTA: u64 = 8;
/// sin, cos: ε ≤ 24.3·2^-53 (proof at [`trig_core`]), so < [`TRIG_BOUND`] = 24.4 ulp.
const TRIG_DELTA: u64 = 40;
/// tan = s/c: both quotients' bounds and the division, ε ≤ 49.7·2^-53, so < [`TAN_BOUND`] = 50 ulp.
const TAN_DELTA: u64 = 80;
#[cfg(test)]
const TRIG_BOUND: f64 = 24.4;
#[cfg(test)]
const TAN_BOUND: f64 = 50.0;
/// expm1: ε ≤ 9.1·2^-53 (proof at [`expm1_core`]), so < 9.2 ulp; 12 keeps a margin.
const EXPM1_DELTA: u64 = 12;
/// cbrt: ε ≤ 5.4·2^-53 (proof at [`cbrt_core`]), so < 5.5 ulp.
const CBRT_DELTA: u64 = 7;
/// The binomial series of (1 + r)^(1/3): C(1/3, n) for n = 1..6.
const B1: f64 = 1.0 / 3.0;
const B2: f64 = -1.0 / 9.0;
const B3: f64 = 5.0 / 81.0;
const B4: f64 = -10.0 / 243.0;
const B5: f64 = 22.0 / 729.0;
const B6: f64 = -154.0 / 6561.0;
/// pow: ε < 89·2^-53 (proof at [`pow_core`]), so < 89.01 ulp; 96 leaves room. A `float` result
/// falls back with probability about 2·96/2^28 (one call in 1.4 million).
const POW_DELTA: u64 = 96;

/// x = 1.5·2^52: adding it rounds to an integer, held in the low bits.
const SHIFT: f64 = 6_755_399_441_055_744.0;
/// RN((ln 2/128)^k / k!), k = 1..4: e^(r·ln 2/128) = 1 + E1·r + E2·r² + E3·r³ + E4·r⁴ + O(r⁵).
const E1: f64 = f64::from_bits(0x3f762e42fefa39ef);
const E2: f64 = f64::from_bits(0x3eeebfbdff82c58f);
const E3: f64 = f64::from_bits(0x3e5c6b08d704a0c0);
const E4: f64 = f64::from_bits(0x3dc3b2ab6fba4e77);
const D3: f64 = 1.0 / 3.0;
const D4: f64 = -1.0 / 4.0;
const D5: f64 = 1.0 / 5.0;
const D6: f64 = -1.0 / 6.0;
// log1p's longer series and split ln 2 (log_core's shorter kernel, MATH-M6-F32-KERNEL, does not use them).
const D7: f64 = 1.0 / 7.0;
const D8: f64 = -1.0 / 8.0;
const LN2_HI: f64 = f64::from_bits(0x3fe62e42fefa3a00); // ln 2, 44 significant bits
const LN2_LO: f64 = f64::from_bits(0xbce0ca86c3898d00); // RN(ln 2 - LN2_HI)
// expm1's two-part reduction and Taylor coefficients (exp_core's one-step kernel, MATH-M6-F32-KERNEL,
// does not use them). L_HI = ln 2/128 cut to 38 significant bits, so kd·L_HI is exact for |k| < 2^15;
// L_LO = RN(ln 2/128 - L_HI); |L_HI + L_LO - ln 2/128| < 2^-99.9, ≤ 2^-85 after the ·k scaling.
const L_HI: f64 = f64::from_bits(0x3f762e42fefa0000);
const L_LO: f64 = f64::from_bits(0x3d0cf79abc9e3b3a);
const C3: f64 = 1.0 / 6.0;
const C4: f64 = 1.0 / 24.0;
const C5: f64 = 1.0 / 120.0;
const INV_L: f64 = f64::from_bits(0x40671547652b82fe); // RN(128 / ln 2)
const INV_M: f64 = f64::from_bits(0x407a934f0979a371); // RN(128 log2(10))
const LN2: f64 = f64::from_bits(0x3fe62e42fefa39ef); // RN(ln 2)
const INV_LN2: f64 = f64::from_bits(0x3ff71547652b82fe); // RN(1 / ln 2)
const INV_LN10: f64 = f64::from_bits(0x3fdbcb7b1526e50e); // RN(1 / ln 10)
/// RN(2^(j/128)), j = 0..128 (1 KiB).
const EXP2_TAB: [f64; 128] = [
    f64::from_bits(0x3ff0000000000000), f64::from_bits(0x3ff0163da9fb3335), f64::from_bits(0x3ff02c9a3e778061),
    f64::from_bits(0x3ff04315e86e7f85), f64::from_bits(0x3ff059b0d3158574), f64::from_bits(0x3ff0706b29ddf6de),
    f64::from_bits(0x3ff0874518759bc8), f64::from_bits(0x3ff09e3ecac6f383), f64::from_bits(0x3ff0b5586cf9890f),
    f64::from_bits(0x3ff0cc922b7247f7), f64::from_bits(0x3ff0e3ec32d3d1a2), f64::from_bits(0x3ff0fb66affed31b),
    f64::from_bits(0x3ff11301d0125b51), f64::from_bits(0x3ff12abdc06c31cc), f64::from_bits(0x3ff1429aaea92de0),
    f64::from_bits(0x3ff15a98c8a58e51), f64::from_bits(0x3ff172b83c7d517b), f64::from_bits(0x3ff18af9388c8dea),
    f64::from_bits(0x3ff1a35beb6fcb75), f64::from_bits(0x3ff1bbe084045cd4), f64::from_bits(0x3ff1d4873168b9aa),
    f64::from_bits(0x3ff1ed5022fcd91d), f64::from_bits(0x3ff2063b88628cd6), f64::from_bits(0x3ff21f49917ddc96),
    f64::from_bits(0x3ff2387a6e756238), f64::from_bits(0x3ff251ce4fb2a63f), f64::from_bits(0x3ff26b4565e27cdd),
    f64::from_bits(0x3ff284dfe1f56381), f64::from_bits(0x3ff29e9df51fdee1), f64::from_bits(0x3ff2b87fd0dad990),
    f64::from_bits(0x3ff2d285a6e4030b), f64::from_bits(0x3ff2ecafa93e2f56), f64::from_bits(0x3ff306fe0a31b715),
    f64::from_bits(0x3ff32170fc4cd831), f64::from_bits(0x3ff33c08b26416ff), f64::from_bits(0x3ff356c55f929ff1),
    f64::from_bits(0x3ff371a7373aa9cb), f64::from_bits(0x3ff38cae6d05d866), f64::from_bits(0x3ff3a7db34e59ff7),
    f64::from_bits(0x3ff3c32dc313a8e5), f64::from_bits(0x3ff3dea64c123422), f64::from_bits(0x3ff3fa4504ac801c),
    f64::from_bits(0x3ff4160a21f72e2a), f64::from_bits(0x3ff431f5d950a897), f64::from_bits(0x3ff44e086061892d),
    f64::from_bits(0x3ff46a41ed1d0057), f64::from_bits(0x3ff486a2b5c13cd0), f64::from_bits(0x3ff4a32af0d7d3de),
    f64::from_bits(0x3ff4bfdad5362a27), f64::from_bits(0x3ff4dcb299fddd0d), f64::from_bits(0x3ff4f9b2769d2ca7),
    f64::from_bits(0x3ff516daa2cf6642), f64::from_bits(0x3ff5342b569d4f82), f64::from_bits(0x3ff551a4ca5d920f),
    f64::from_bits(0x3ff56f4736b527da), f64::from_bits(0x3ff58d12d497c7fd), f64::from_bits(0x3ff5ab07dd485429),
    f64::from_bits(0x3ff5c9268a5946b7), f64::from_bits(0x3ff5e76f15ad2148), f64::from_bits(0x3ff605e1b976dc09),
    f64::from_bits(0x3ff6247eb03a5585), f64::from_bits(0x3ff6434634ccc320), f64::from_bits(0x3ff6623882552225),
    f64::from_bits(0x3ff68155d44ca973), f64::from_bits(0x3ff6a09e667f3bcd), f64::from_bits(0x3ff6c012750bdabf),
    f64::from_bits(0x3ff6dfb23c651a2f), f64::from_bits(0x3ff6ff7df9519484), f64::from_bits(0x3ff71f75e8ec5f74),
    f64::from_bits(0x3ff73f9a48a58174), f64::from_bits(0x3ff75feb564267c9), f64::from_bits(0x3ff780694fde5d3f),
    f64::from_bits(0x3ff7a11473eb0187), f64::from_bits(0x3ff7c1ed0130c132), f64::from_bits(0x3ff7e2f336cf4e62),
    f64::from_bits(0x3ff80427543e1a12), f64::from_bits(0x3ff82589994cce13), f64::from_bits(0x3ff8471a4623c7ad),
    f64::from_bits(0x3ff868d99b4492ed), f64::from_bits(0x3ff88ac7d98a6699), f64::from_bits(0x3ff8ace5422aa0db),
    f64::from_bits(0x3ff8cf3216b5448c), f64::from_bits(0x3ff8f1ae99157736), f64::from_bits(0x3ff9145b0b91ffc6),
    f64::from_bits(0x3ff93737b0cdc5e5), f64::from_bits(0x3ff95a44cbc8520f), f64::from_bits(0x3ff97d829fde4e50),
    f64::from_bits(0x3ff9a0f170ca07ba), f64::from_bits(0x3ff9c49182a3f090), f64::from_bits(0x3ff9e86319e32323),
    f64::from_bits(0x3ffa0c667b5de565), f64::from_bits(0x3ffa309bec4a2d33), f64::from_bits(0x3ffa5503b23e255d),
    f64::from_bits(0x3ffa799e1330b358), f64::from_bits(0x3ffa9e6b5579fdbf), f64::from_bits(0x3ffac36bbfd3f37a),
    f64::from_bits(0x3ffae89f995ad3ad), f64::from_bits(0x3ffb0e07298db666), f64::from_bits(0x3ffb33a2b84f15fb),
    f64::from_bits(0x3ffb59728de5593a), f64::from_bits(0x3ffb7f76f2fb5e47), f64::from_bits(0x3ffba5b030a1064a),
    f64::from_bits(0x3ffbcc1e904bc1d2), f64::from_bits(0x3ffbf2c25bd71e09), f64::from_bits(0x3ffc199bdd85529c),
    f64::from_bits(0x3ffc40ab5fffd07a), f64::from_bits(0x3ffc67f12e57d14b), f64::from_bits(0x3ffc8f6d9406e7b5),
    f64::from_bits(0x3ffcb720dcef9069), f64::from_bits(0x3ffcdf0b555dc3fa), f64::from_bits(0x3ffd072d4a07897c),
    f64::from_bits(0x3ffd2f87080d89f2), f64::from_bits(0x3ffd5818dcfba487), f64::from_bits(0x3ffd80e316c98398),
    f64::from_bits(0x3ffda9e603db3285), f64::from_bits(0x3ffdd321f301b460), f64::from_bits(0x3ffdfc97337b9b5f),
    f64::from_bits(0x3ffe264614f5a129), f64::from_bits(0x3ffe502ee78b3ff6), f64::from_bits(0x3ffe7a51fbc74c83),
    f64::from_bits(0x3ffea4afa2a490da), f64::from_bits(0x3ffecf482d8e67f1), f64::from_bits(0x3ffefa1bee615a27),
    f64::from_bits(0x3fff252b376bba97), f64::from_bits(0x3fff50765b6e4540), f64::from_bits(0x3fff7bfdad9cbe14),
    f64::from_bits(0x3fffa7c1819e90d8), f64::from_bits(0x3fffd3c22b8f71f1),
];
/// EXP2_TAB's encodings: the scale 2^(k>>7) is added to the exponent field in an integer register,
/// off the floating-point dependency chain.
const EXP2_BITS: [u64; 128] = {
    let mut a = [0u64; 128];
    let mut j = 0;
    while j < 128 {
        a[j] = EXP2_TAB[j].to_bits();
        j += 1;
    }
    a
};
/// 1/c_i rounded to 24 significant bits (1 at i = 0 and 127; 1 KiB).
const LOG_INV: [f64; 128] = [
    f64::from_bits(0x3ff0000000000000), f64::from_bits(0x3fefa11ca0000000), f64::from_bits(0x3fef6310a0000000),
    f64::from_bits(0x3fef25f640000000), f64::from_bits(0x3feee9c800000000), f64::from_bits(0x3feeae8080000000),
    f64::from_bits(0x3fee741aa0000000), f64::from_bits(0x3fee3a9180000000), f64::from_bits(0x3fee01e020000000),
    f64::from_bits(0x3fedca01e0000000), f64::from_bits(0x3fed92f220000000), f64::from_bits(0x3fed5cac80000000),
    f64::from_bits(0x3fed272ca0000000), f64::from_bits(0x3fecf26e60000000), f64::from_bits(0x3fecbe6da0000000),
    f64::from_bits(0x3fec8b2660000000), f64::from_bits(0x3fec5894e0000000), f64::from_bits(0x3fec26b540000000),
    f64::from_bits(0x3febf583e0000000), f64::from_bits(0x3febc4fd60000000), f64::from_bits(0x3feb951e20000000),
    f64::from_bits(0x3feb65e2e0000000), f64::from_bits(0x3feb374840000000), f64::from_bits(0x3feb094b40000000),
    f64::from_bits(0x3feadbe880000000), f64::from_bits(0x3feaaf1d20000000), f64::from_bits(0x3fea82e660000000),
    f64::from_bits(0x3fea574100000000), f64::from_bits(0x3fea2c2a80000000), f64::from_bits(0x3fea01a020000000),
    f64::from_bits(0x3fe9d79f20000000), f64::from_bits(0x3fe9ae24e0000000), f64::from_bits(0x3fe9852f00000000),
    f64::from_bits(0x3fe95cbb00000000), f64::from_bits(0x3fe934c680000000), f64::from_bits(0x3fe90d4f20000000),
    f64::from_bits(0x3fe8e65280000000), f64::from_bits(0x3fe8bfce80000000), f64::from_bits(0x3fe899c100000000),
    f64::from_bits(0x3fe87427c0000000), f64::from_bits(0x3fe84f00c0000000), f64::from_bits(0x3fe82a4a00000000),
    f64::from_bits(0x3fe8060180000000), f64::from_bits(0x3fe7e22560000000), f64::from_bits(0x3fe7beb3a0000000),
    f64::from_bits(0x3fe79baa60000000), f64::from_bits(0x3fe7790820000000), f64::from_bits(0x3fe756cac0000000),
    f64::from_bits(0x3fe734f0c0000000), f64::from_bits(0x3fe7137860000000), f64::from_bits(0x3fe6f26020000000),
    f64::from_bits(0x3fe6d1a620000000), f64::from_bits(0x3fe6b14900000000), f64::from_bits(0x3fe6914740000000),
    f64::from_bits(0x3fe6719f40000000), f64::from_bits(0x3fe6524f80000000), f64::from_bits(0x3fe63356c0000000),
    f64::from_bits(0x3fe614b360000000), f64::from_bits(0x3fe5f66440000000), f64::from_bits(0x3fe5d867c0000000),
    f64::from_bits(0x3fe5babcc0000000), f64::from_bits(0x3fe59d6200000000), f64::from_bits(0x3fe5805600000000),
    f64::from_bits(0x3fe56397c0000000), f64::from_bits(0x3ff54725e0000000), f64::from_bits(0x3ff52aff60000000),
    f64::from_bits(0x3ff50f22e0000000), f64::from_bits(0x3ff4f38f60000000), f64::from_bits(0x3ff4d843c0000000),
    f64::from_bits(0x3ff4bd3ee0000000), f64::from_bits(0x3ff4a27fa0000000), f64::from_bits(0x3ff4880520000000),
    f64::from_bits(0x3ff46dce40000000), f64::from_bits(0x3ff453d9e0000000), f64::from_bits(0x3ff43a2740000000),
    f64::from_bits(0x3ff420b520000000), f64::from_bits(0x3ff40782e0000000), f64::from_bits(0x3ff3ee8f40000000),
    f64::from_bits(0x3ff3d5d9a0000000), f64::from_bits(0x3ff3bd60e0000000), f64::from_bits(0x3ff3a52440000000),
    f64::from_bits(0x3ff38d22e0000000), f64::from_bits(0x3ff3755be0000000), f64::from_bits(0x3ff35dce60000000),
    f64::from_bits(0x3ff34679a0000000), f64::from_bits(0x3ff32f5ce0000000), f64::from_bits(0x3ff3187760000000),
    f64::from_bits(0x3ff301c820000000), f64::from_bits(0x3ff2eb4ea0000000), f64::from_bits(0x3ff2d50a00000000),
    f64::from_bits(0x3ff2bef980000000), f64::from_bits(0x3ff2a91ca0000000), f64::from_bits(0x3ff2937260000000),
    f64::from_bits(0x3ff27dfa40000000), f64::from_bits(0x3ff268b380000000), f64::from_bits(0x3ff2539d80000000),
    f64::from_bits(0x3ff23eb7a0000000), f64::from_bits(0x3ff22a0120000000), f64::from_bits(0x3ff2157980000000),
    f64::from_bits(0x3ff2012020000000), f64::from_bits(0x3ff1ecf440000000), f64::from_bits(0x3ff1d8f560000000),
    f64::from_bits(0x3ff1c52300000000), f64::from_bits(0x3ff1b17c60000000), f64::from_bits(0x3ff19e0120000000),
    f64::from_bits(0x3ff18ab080000000), f64::from_bits(0x3ff1778a20000000), f64::from_bits(0x3ff1648d60000000),
    f64::from_bits(0x3ff151b9a0000000), f64::from_bits(0x3ff13f0e80000000), f64::from_bits(0x3ff12c8b80000000),
    f64::from_bits(0x3ff11a3020000000), f64::from_bits(0x3ff107fbc0000000), f64::from_bits(0x3ff0f5ee00000000),
    f64::from_bits(0x3ff0e40660000000), f64::from_bits(0x3ff0d24460000000), f64::from_bits(0x3ff0c0a780000000),
    f64::from_bits(0x3ff0af2f80000000), f64::from_bits(0x3ff09ddba0000000), f64::from_bits(0x3ff08cabc0000000),
    f64::from_bits(0x3ff07b9f20000000), f64::from_bits(0x3ff06ab5a0000000), f64::from_bits(0x3ff059eea0000000),
    f64::from_bits(0x3ff04949c0000000), f64::from_bits(0x3ff038c6c0000000), f64::from_bits(0x3ff0286500000000),
    f64::from_bits(0x3ff0182440000000), f64::from_bits(0x3ff0000000000000),
];
/// RN(-ln LOG_INV[i]) (0 at i = 0 and 127; 1 KiB).
const LOG_C: [f64; 128] = [
    f64::from_bits(0x0000000000000000), f64::from_bits(0x3f87dc49e7810add), f64::from_bits(0x3f93cea5df46a5c8),
    f64::from_bits(0x3f9b9fc0afaf91a1), f64::from_bits(0x3fa1b0d90923d990), f64::from_bits(0x3fa58a5b57c8e4dc),
    f64::from_bits(0x3fa95c836cc8e3f4), f64::from_bits(0x3fad276b22db0b5d), f64::from_bits(0x3fb075982498e472),
    f64::from_bits(0x3fb253f6120a1419), f64::from_bits(0x3fb42edcd9a646f2), f64::from_bits(0x3fb60658ad3750c4),
    f64::from_bits(0x3fb7da76907b12cf), f64::from_bits(0x3fb9ab42252033af), f64::from_bits(0x3fbb78c7d2b0edb1),
    f64::from_bits(0x3fbd4313a96cb361), f64::from_bits(0x3fbf0a30391162ca), f64::from_bits(0x3fc06714f3ca5972),
    f64::from_bits(0x3fc14785c6e742be), f64::from_bits(0x3fc2266f328a5ace), f64::from_bits(0x3fc303d74c647fdd),
    f64::from_bits(0x3fc3dfc2c26cc62b), f64::from_bits(0x3fc4ba37269a55f0), f64::from_bits(0x3fc5933896982097),
    f64::from_bits(0x3fc66acd4072ad51), f64::from_bits(0x3fc740f93fc037ba), f64::from_bits(0x3fc815c059c357ff),
    f64::from_bits(0x3fc8e92902886d46), f64::from_bits(0x3fc9bb36547dfb89), f64::from_bits(0x3fca8becdf082f1c),
    f64::from_bits(0x3fcb5b51740fb5ab), f64::from_bits(0x3fcc2968890c18cb), f64::from_bits(0x3fccf6359209c5ee),
    f64::from_bits(0x3fcdc1bcdcabec8b), f64::from_bits(0x3fce8c0250aa5a60), f64::from_bits(0x3fcf550a0ecb7b4b),
    f64::from_bits(0x3fd00e6c38ad501e), f64::from_bits(0x3fd071b860cd590d), f64::from_bits(0x3fd0d46b3d9ab750),
    f64::from_bits(0x3fd13686fa13a8b1), f64::from_bits(0x3fd1980d34542370), f64::from_bits(0x3fd1f8ffa248a2f3),
    f64::from_bits(0x3fd2596011df763a), f64::from_bits(0x3fd2b93013789d31), f64::from_bits(0x3fd31871a4144190),
    f64::from_bits(0x3fd37726827fd863), f64::from_bits(0x3fd3d54f7e81f71c), f64::from_bits(0x3fd432ef2f84e814),
    f64::from_bits(0x3fd490068ec009d2), f64::from_bits(0x3fd4ec9758200275), f64::from_bits(0x3fd548a2aa6dd268),
    f64::from_bits(0x3fd5a42ac334cfe4), f64::from_bits(0x3fd5ff308ea793db), f64::from_bits(0x3fd659b56383e1f4),
    f64::from_bits(0x3fd6b3bb05b59444), f64::from_bits(0x3fd70d42f1789238), f64::from_bits(0x3fd7664dfcb9dbd2),
    f64::from_bits(0x3fd7bede21f7afc4), f64::from_bits(0x3fd816f3fb20d49f), f64::from_bits(0x3fd86e91a5b30ba1),
    f64::from_bits(0x3fd8c5b7dad8b48d), f64::from_bits(0x3fd91c67bf45a84d), f64::from_bits(0x3fd972a345135159),
    f64::from_bits(0x3fd9c86af25c0865), f64::from_bits(0xbfd23ec584deba46), f64::from_bits(0xbfd1e9e183c899ee),
    f64::from_bits(0xbfd1956d385bc2fa), f64::from_bits(0xbfd14167e6767782), f64::from_bits(0xbfd0edd064378081),
    f64::from_bits(0xbfd09aa57a26c6d4), f64::from_bits(0xbfd047e5e31e83aa), f64::from_bits(0xbfcfeb22276a07cc),
    f64::from_bits(0xbfcf474b5c4df214), f64::from_bits(0xbfcea4448d84aaf3), f64::from_bits(0xbfce020d27235a90),
    f64::from_bits(0xbfcd60a15710350e), f64::from_bits(0xbfccc001295b3c2f), f64::from_bits(0xbfcc20289a17f9b3),
    f64::from_bits(0xbfcb81178d3823b1), f64::from_bits(0xbfcae2ca9be72bcd), f64::from_bits(0xbfca4540b3e6aafc),
    f64::from_bits(0xbfc9a877e06baa1e), f64::from_bits(0xbfc90c6e177cbcb7), f64::from_bits(0xbfc8712139d0e994),
    f64::from_bits(0xbfc7d68fe72f5ab3), f64::from_bits(0xbfc73cb8adcfd12d), f64::from_bits(0xbfc6a39a0a3bd37b),
    f64::from_bits(0xbfc60b30b8309461), f64::from_bits(0xbfc5737cbb818cdd), f64::from_bits(0xbfc4dc7b817bc1c7),
    f64::from_bits(0xbfc4462b3bc9b3b6), f64::from_bits(0xbfc3b08bc0d7f28a), f64::from_bits(0xbfc31b996aba4f81),
    f64::from_bits(0xbfc28753ef11ab9a), f64::from_bits(0xbfc1f3b93bf25d3f), f64::from_bits(0xbfc160c80c4b27b0),
    f64::from_bits(0xbfc0ce7f0c4cc27d), f64::from_bits(0xbfc03cdbf7d1ec0c), f64::from_bits(0xbfbf57bc799005db),
    f64::from_bits(0xbfbe3708b530482e), f64::from_bits(0xbfbd1797ba21935f), f64::from_bits(0xbfbbf9680f9fc9fc),
    f64::from_bits(0xbfbadc78265aea86), f64::from_bits(0xbfb9c0c2ba4d252e), f64::from_bits(0xbfb8a647d391dc19),
    f64::from_bits(0xbfb78d01f23d82ce), f64::from_bits(0xbfb674f0ee365a66), f64::from_bits(0xbfb55e10e20e0324),
    f64::from_bits(0xbfb4485dc8dbdfa6), f64::from_bits(0xbfb333d734183f00), f64::from_bits(0xbfb2207ac8785473),
    f64::from_bits(0xbfb10e4612cae81f), f64::from_bits(0xbfaffa694dab92fd), f64::from_bits(0xbfadda8b7c67ee35),
    f64::from_bits(0xbfabbced3a68f3bd), f64::from_bits(0xbfa9a188df73de25), f64::from_bits(0xbfa7885892357793),
    f64::from_bits(0xbfa5715df403ce3f), f64::from_bits(0xbfa35c8b2ca13042), f64::from_bits(0xbfa149e5680059fa),
    f64::from_bits(0xbf9e72bccc13cd9f), f64::from_bits(0xbf9a55f624c5c427), f64::from_bits(0xbf963d615c690bd6),
    f64::from_bits(0xbf9228f827ea2d0e), f64::from_bits(0xbf8c3177b4c75dee), f64::from_bits(0xbf84192bb96832bf),
    f64::from_bits(0xbf78121bb458686f), f64::from_bits(0x0000000000000000),
];

/// `f(x)` for `float` `x` by the fast path, or `None` when the operand is outside its domain or
/// the rounding test fails (the caller then runs the accurate path).
///
/// Always inlined (#1362): each `fast_fn!` wrapper passes a constant `f`, so the match folds and
/// every function gets its own straight-line body instead of one shared body with a jump table.
///
/// The result's [`Status::hw`] is the set of MXCSR bits these kernels can raise in hardware (the
/// in-place envelope skips its exit write when the caller already holds them, #1360): inexact
/// always; underflow only from `y as f32` when the result is tiny, and then `flags` has it too;
/// denormal-operand only from `f64::from` of a subnormal `x` or of a subnormal result. Every other
/// operation acts on normal `double`s (the domain checks and `round`'s range bound them: no
/// overflow, no subnormal intermediate, no division, no NaN or infinity), so nothing else is raised.
#[inline(always)]
pub fn eval(f: Fast, x: f32, mode: Rounding) -> Option<(f32, Status)> {
    let (y, delta, name) = match f {
        Fast::Exp => (exp_core(x, -104.0, 89.0, INV_L)?, EXP_DELTA, "expf"),
        Fast::Exp2 => (exp_core(x, -150.5, 128.0, 128.0)?, EXP_DELTA, "exp2f"),
        Fast::Exp10 => (exp_core(x, -45.5, 38.6, INV_M)?, EXP_DELTA, "exp10f"),
        Fast::Log => (log_core(x)?, LOG_DELTA, "logf"),
        Fast::Log2 => (log_core(x)? * INV_LN2, LOG_DELTA, "log2f"),
        Fast::Log10 => (log_core(x)? * INV_LN10, LOG_DELTA, "log10f"),
        Fast::Log1p => (log1p_core(x)?, LOG1P_DELTA, "log1pf"),
        Fast::Sin => (trig_core(x)?.0, TRIG_DELTA, "sinf"),
        Fast::Cos => (trig_core(x)?.1, TRIG_DELTA, "cosf"),
        Fast::Tan => {
            let (s, c) = trig_core(x)?;
            (s / c, TAN_DELTA, "tanf")
        }
        Fast::Expm1 => (expm1_core(x)?, EXPM1_DELTA, "expm1f"),
        Fast::Cbrt => (cbrt_core(x)?, CBRT_DELTA, "cbrtf"),
        Fast::Asin => (asin_core(x)?, ASIN_DELTA, "asinf"),
        Fast::Acos => (acos_core(x)?, ASIN_DELTA, "acosf"),
        Fast::Atan => (atan_core(x)?, ATAN_DELTA, "atanf"),
        Fast::Sinh => (sinh_core(x)?, SINH_DELTA, "sinhf"),
        Fast::Cosh => (cosh_core(x)?, COSH_DELTA, "coshf"),
        Fast::Tanh => (tanh_core(x)?, TANH_DELTA, "tanhf"),
        Fast::Asinh => (asinh_core(x)?, ASINH_DELTA, "asinhf"),
        Fast::Acosh => (acosh_core(x)?, ACOSH_DELTA, "acoshf"),
        Fast::Atanh => (atanh_core(x)?, ATANH_DELTA, "atanhf"),
    };
    finish(x, y, delta, mode, name)
}

/// The rounding test on y, the statistics under `name` and the hardware flags (see [`eval`]).
#[inline(always)]
fn finish(x: f32, y: f64, delta: u64, mode: Rounding, name: &'static str) -> Option<(f32, Status)> {
    let r = round(y, delta, mode);
    stats::record(name, r.is_some());
    let v = r?;
    // The result is a normal `float` (round's range): inexact only; denormal-operand only from
    // `f64::from` of a subnormal x (exp refuses those: y is exactly 1).
    let hw = if x.is_subnormal() { Flags::INEXACT.union(Status::HW_DENORMAL) } else { Flags::INEXACT };
    Some((v, Status::flags(Flags::INEXACT).hw(hw)))
}

/// e^x, 2^x or 10^x as a `double` y with |y/f - 1| ≤ 2^-45.16, for x in (`lo`, `hi`) and
/// `c` = 128·log2(base) rounded (128 exactly for exp2).
///
/// Reduction: z = RN(x·c) = k + r with k the nearest integer, |r| ≤ 1/2, and f = 2^(k>>7)·T[k&127]
/// ·2^(r/128) with T = EXP2_TAB. |z| < 128·104/ln 2 < 2^14.24 (exp; exp10 likewise; exp2 is exact),
/// so c's rounding and the product's together put z within 2^-52·2^14.24 = 2^-37.76 of x·128·log2
/// (base): a relative error of ≤ 2^-37.76·ln 2/128 = 2^-45.29 in f. r = z - k is exact (k is an
/// integer within 1/2 of z, on z's grid).
///
/// 2^(r/128) = e^u, u = r·ln 2/128, |u| ≤ 2^-8.53: p = (1 + E1·r) + r²·((E2 + E3·r) + r²·E4)
/// (Estrin: two short chains instead of one long one). Taylor remainder ≤ |u|^5/120·1.003 <
/// 2^-49.5; the evaluation ≤ 2^-52 (the last sum, near 1, and 1 + E1·r round once each; the r²
/// part is < 2^-18 and its errors < 2^-69); the coefficients' roundings < 2^-61. y = RN(S·p) with
/// S = T·2^(k>>7) (an exact exponent add: k>>7 is in [-152, 128], S is a normal `double`): the
/// table 2^-53, the product 2^-53. Total ≤ 2^-45.29 + 2^-49.5 + 2^-52 + 2^-52 < 2^-45.16.
#[inline(always)]
fn exp_core(x: f32, lo: f64, hi: f64, c: f64) -> Option<f64> {
    let x = f64::from(x);
    if !(x > lo && x < hi) {
        return None;
    }
    let z = x * c;
    let kd = z + SHIFT;
    let k = kd.to_bits() as i32;
    let r = z - (kd - SHIFT);
    let r2 = r * r;
    let p = (1.0 + E1 * r) + r2 * ((E2 + E3 * r) + r2 * E4);
    let s = EXP2_BITS[(k & 127) as usize].wrapping_add((i64::from(k >> 7) as u64) << 52);
    Some(f64::from_bits(s) * p)
}

/// ln x as a `double` y with |y/ln x - 1| ≤ 2^-51, for a positive finite `float` x.
///
/// x = 2^e·m with m in [0.75, 1.5) (as a `double` every `float`, subnormals included, is normal).
/// i = the top 7 fraction bits of m; c_i the centre of its interval (m in [1, 1.5): width 1/128;
/// m in [0.75, 1): width 1/256), except c = 1 for i = 0 (m in [1, 1 + 2^-7)) and i = 127
/// (m in [1 - 2^-8, 1)). LOG_INV[i] has 24 bits and m at most 24, so m·LOG_INV[i] is exact and
/// r = m·LOG_INV[i] - 1 is exact (Sterbenz), |r| < 2^-7 (i = 0), < 0.0040 otherwise.
/// ln x = e·ln 2 + LOG_C[i] + ln(1 + r), ln(1 + r) = r + s, s = r²(-1/2 + r/3) + r⁴(-1/4 + r/5 -
/// r²/6) (Estrin). y = ((e·LN2 + LOG_C[i]) + r) + s.
/// * Case A (e = 0, c = 1): the first two sums are exact (0 + 0 + r), y = RN(r + s): the Taylor
///   remainder |r|^7/7 ≤ 2^-44.8|r| (|r| < 2^-7), the evaluation of s ≤ 2^-51·r² and the last rounding
///   2^-53, over |y| ≥ 0.996|r|: ε ≤ 2^-44.79.
/// * Case B1 (e ≠ 0, |y| ≥ ln 2 - ln 1.5 = 0.28, |e·ln 2| ≤ 2.5|y|, |LOG_C| ≤ 1.5|y|): e·LN2 ≤
///   2^-52|e·ln 2|, LOG_C 2^-53|LOG_C|, three sums 2^-53·1.01|y| each, remainder |r|^7/7 < 2^-51.8 <
///   2^-50|y|: ε ≤ 17.5·2^-53 < 2^-48.8.
/// * Case B2 (e = 0, c ≠ 1, |r| ≤ 2^-8, |y| ≥ ln(1 + 2^-8), |LOG_C| ≤ 1.5|y|): LOG_C, two sums,
///   remainder 2^-58.8 < 2^-50.8|y|: ε < 2^-50.
#[inline(always)]
fn log_core(x: f32) -> Option<f64> {
    log_core_from(x, 1)
}

/// [`log_core`] for `float`s whose bits are at least `lo` (1: every positive finite one;
/// 0x0080_0000: the normal ones, whose conversion to `double` raises nothing).
#[inline(always)]
fn log_core_from(x: f32, lo: u32) -> Option<f64> {
    if x.to_bits().wrapping_sub(lo) >= 0x7f80_0000 - lo {
        return None; // ±0, negative, ∞, NaN: the accurate path's special cases
    }
    let xb = f64::from(x).to_bits();
    let e = (xb.wrapping_sub(0x3fe8_0000_0000_0000) as i64 >> 52) as i32;
    let m = f64::from_bits(xb.wrapping_sub((e as i64 as u64) << 52));
    let i = ((m.to_bits() >> 45) & 127) as usize;
    let r = m * LOG_INV[i] - 1.0;
    let r2 = r * r;
    let s = r2 * (-0.5 + r * D3) + (r2 * r2) * ((D4 + r * D5) + r2 * D6);
    let ef = f64::from(e);
    Some(((ef * LN2 + LOG_C[i]) + r) + s)
}

/// ln(1 + x) as a `double` y with |y/ln(1 + x) - 1| ≤ 6.8·2^-53, for a finite `float` x > -1
/// with |x| ≥ 2^-26 (below that the result is x - x²/2 to within the rounding test's margin, and
/// the test would refuse almost every operand; the accurate path's `near` case takes them).
///
/// z = RN(1 + x) is exact unless x ≥ 2^51: for x in (-1, -1/2] by Sterbenz; otherwise x's last
/// bit is ≥ 2^-49 and its top below 2^52, so 1 + x spans at most 53 bits. z = 2^e·m, i, c and
/// LOG_INV/LOG_C as in [`log_core`]; ln(1 + r) = r + r²(-1/2 + r/3 - ... - r^6/8) (Horner, D3..D8)
/// and e·ln 2 = e·LN2_HI (exact, |e| ≤ 128, 44 bits) + e·LN2_LO, whose error terms (the series
/// remainder, LOG_C's and the sums' roundings, 3.2·2^-53 in all) are the base below, except r:
/// * e = 0 (z in [0.75, 1.5), so z is exact and x = z - 1): r = z·L - 1 = x·L + (L - 1) with
///   L = LOG_INV[i]; x·L is exact (24 by 24 bits), L - 1 is exact (L in [2/3, 4/3], 24 bits), so r
///   is RN of the exact value: 2^-53|r|. Case A (c = 1, L = 1): r = x exactly, as in log_core.
///   Case B (c ≠ 1, |y| ≥ ln(1 + 2^-8) and |r| < 0.0040): r's rounding adds ≤ 1.03·2^-53|y|,
///   4.3·2^-53 in total.
/// * e ≠ 0 (|y| ≥ ln(4/3) > 0.287): m has up to 53 bits, so m·L is rounded once (≤ 2^-53
///   absolute, |m·L - 1| < 2^-7) and the subtraction of 1 is exact (Sterbenz): ln(1 + r) moves by
///   ≤ 1.01·2^-53, ≤ 3.6·2^-53|y|; z's own rounding for x ≥ 2^51 is ≤ 2^-53 absolute against
///   |y| > 35 (≤ 2^-58|y|). With the base 3.2·2^-53: ≤ 6.8·2^-53.
fn log1p_core(x: f32) -> Option<f64> {
    let xd = f64::from(x);
    // NaN, x ≤ -1, ∞ and |x| < 2^-26 (the zeros and subnormals included) go to the accurate path.
    if !(xd > -1.0 && xd <= f64::from(f32::MAX)) || xd.to_bits() & !(1 << 63) < 0x3e50_0000_0000_0000 {
        return None;
    }
    let z = 1.0 + xd;
    let zb = z.to_bits();
    let e = (zb.wrapping_sub(0x3fe8_0000_0000_0000) as i64 >> 52) as i32;
    let m = f64::from_bits(zb.wrapping_sub((e as i64 as u64) << 52));
    let i = ((m.to_bits() >> 45) & 127) as usize;
    let l = LOG_INV[i];
    let r = if e == 0 { xd * l + (l - 1.0) } else { m * l - 1.0 };
    let s = r * r * (-0.5 + r * (D3 + r * (D4 + r * (D5 + r * (D6 + r * (D7 + r * D8))))));
    let ef = f64::from(e);
    Some((ef * LN2_HI + LOG_C[i]) + (r + (s + ef * LN2_LO)))
}

/// The trig fast range: 2^-26 ≤ |x| < 2^19 (n below 2^23.4, so the n·PI64_k products are exact).
const TRIG_MIN: f64 = 1.490_116_119_384_765_6e-8;
const TRIG_MAX: f64 = 524_288.0;
/// Below this |r| (for n ≠ 0) the reduction's absolute error would matter: the accurate path.
const TRIG_R_MIN: f64 = 1.455_191_522_836_685_2e-11; // 2^-36
const INV_PI64: f64 = f64::from_bits(t::SCALES[2]); // RN(64/π)
const PI64_1: f64 = f64::from_bits(0x3fa9_21fb_5400_0000); // π/64 to 27 significant bits
const PI64_2: f64 = f64::from_bits(0x3dc1_0b46_1100_0000); // the next 29 bits
const PI64_3: f64 = f64::from_bits(0x3bf4_c4c6_628b_80dc); // RN(π/64 - PI64_1 - PI64_2)
/// The minimax polynomials for sin and cos on [-0.0246, 0.0246] (`crates/math/fits/fast32_sin.fit`
/// and `fast32_cos.fit`, `cargo xtask remez`, D-10.3): sin odd 1..5 with the r coefficient fixed
/// at 1, relative error 2^-49.07 (15.24·2^-53); cos even 0..6 with the constant fixed at 1,
/// relative error 2^-64.59.
const S3: f64 = f64::from_bits(crate::coeffs::fast32_sin::FAST32_SIN[1]);
const S5: f64 = f64::from_bits(crate::coeffs::fast32_sin::FAST32_SIN[2]);
const K2: f64 = f64::from_bits(crate::coeffs::fast32_cos::FAST32_COS[1]);
const K4: f64 = f64::from_bits(crate::coeffs::fast32_cos::FAST32_COS[2]);
const K6: f64 = f64::from_bits(crate::coeffs::fast32_cos::FAST32_COS[3]);

/// (sin x, cos x) as `double`s, each with |y/f - 1| ≤ 24.3·2^-53, for 2^-26 ≤ |x| < 2^19.
///
/// Reduction (Cody–Waite): x = n·π/64 + r, n = nearest(RN(x·64/π)), so |r| < 0.024544.
/// n·PI64_1 and n·PI64_2 are exact (27 and 29 bits, |n| < 2^24); x - n·PI64_1 is exact (both on
/// the 2^-31 grid, the difference below 2^-5); then two roundings ≤ 2^-53 of their results, the
/// product n·PI64_3 ≤ 2^-94 and the constants' tail ≤ 2^-95.8: |Δr| ≤ 2·2^-53|r| + 2^-93. n = 0
/// gives r = x exactly. |r| < 2^-36 with n ≠ 0 goes to the accurate path.
///
/// sin r = sr = r + r·z(S3 + z·S5), z = r² (|r| ≤ 0.024545 < 0.0246, the fitted range):
/// approximation error ≤ 15.25·2^-53|r| (the fit's 15.24 plus its coefficients' rounding, already
/// in the fit's measured figure); with Δr and the evaluation (the r·z·(...) part is < 10^-4|r|,
/// its roundings below 2^-63|r|; the final sum 2^-53), |Δsr| ≤ (3.01 + 15.25)·2^-53|r| =
/// 18.26·2^-53|r|. cos r - 1 = cm = z(K2 + z(K4 + z·K6)): approximation error ≤ 2^-64.5 < 0.01·2^-53
/// absolute, |cm| ≤ 3.02·10^-4, |Δcm| ≤ 8·2^-53|cm| + 0.01·2^-53. S = sin(nπ/64), C = cos(nπ/64)
/// are the `double` heads of the table (relative error 2^-53; sin 0, sin π, cos ±π/2 are exactly
/// 0 and ±1). sin x = S + (S·cm + C·sr); cos x is the same with (C, -S) for (S, C) (the angle
/// shifted by π/2).
/// * n ≢ 0 (mod 64): the angle is at least π/128 from a multiple of π, so |S| ≤ 2|y| and
///   |C·r| ≤ 1.0001|y|. The errors, in units of 2^-53|y|: S 2, C 1.0001, sr 18.26·1.0001, cm
///   0.005 + 0.02, the two products 1.002, the inner sum 1.02, the outer 1: ≤ 24.3.
/// * n ≡ 0 (mod 64): S = 0, C = ±1, y = ±sr: Δr ≤ (2 + 2^-5)·2^-53|r| (|r| ≥ 2^-36), so
///   ≤ 3.04 + 15.25 = 18.3.
///
/// No operand or intermediate is subnormal (|r| ≥ 2^-36 or r = x ≥ 2^-26, so z^4 ≥ 2^-304), and
/// |y| ≥ 2^-37.
#[inline(always)]
fn trig_core(x: f32) -> Option<(f64, f64)> {
    let xd = f64::from(x);
    let ax = xd.abs();
    // NaN, ±∞, |x| ≥ 2^19 and |x| < 2^-26 (the zeros and subnormals included): the accurate path.
    if !(TRIG_MIN..TRIG_MAX).contains(&ax) {
        return None;
    }
    let k = xd * INV_PI64 + SHIFT;
    let n = (k.to_bits() & 127) as usize; // n mod 128 (the low bits of 2^51 + n)
    let kf = k - SHIFT;
    let r = ((xd - kf * PI64_1) - kf * PI64_2) - kf * PI64_3;
    if r.abs() < TRIG_R_MIN {
        return None;
    }
    let z = r * r;
    let sr = r + r * z * (S3 + z * S5);
    let cm = z * (K2 + z * (K4 + z * K6));
    let s = f64::from_bits(t::SIN_K_PI64[n][0]);
    let c = f64::from_bits(t::SIN_K_PI64[(n + 32) & 127][0]);
    Some((s + (s * cm + c * sr), c + (c * cm - s * sr)))
}

/// `sincosf`: one reduction for both results; each one that fails its rounding test is computed
/// by the accurate path, so the pair and the flags are those of `sinf` and `cosf`.
pub fn sincos(x: f32, mode: Rounding) -> ((f32, f32), Status) {
    let Some((ys, yc)) = trig_core(x) else {
        return crate::trig::sincos(x, mode);
    };
    let s = finish(x, ys, TRIG_DELTA, mode, "sinf").unwrap_or_else(|| crate::trig::sin(x, mode));
    let c = finish(x, yc, TRIG_DELTA, mode, "cosf").unwrap_or_else(|| crate::trig::cos(x, mode));
    ((s.0, c.0), s.1.and(c.1))
}

include!("fast32_tables.rs");

/// e^x - 1 as a `double` y with |y/f - 1| ≤ 9.1·2^-53, for a `float` x in (-18, 89) with
/// |x| ≥ 2^-45 (below that the rounding test refuses every operand: y is within 12 ulp of x).
///
/// Reduction as for e^x in [`exp_core`]: x = k·ln2/128 + u, |u| ≤ 0.00271 with absolute error
/// ≤ 2^-61.4 (|k| < 2^15), and for k = 0, u = x exactly. E = 2^m·2^(j/128) (m = k >> 7, j = k & 127)
/// = 2^m·(T + T_lo) with T = EXP2_TAB[j], T_lo = EXP2_LO[j] (its rounding ≤ 2^-106·T); e^u = 1 + q,
/// q = u·h, h = 1 + u(1/2 + u(1/6 + u(1/24 + u/120))). f = (E - 1) + E·q is evaluated as
/// y = RN(2^m·T - 1) + 2^m·(T_lo + T·q), 2^m exact (m in [-26, 128]). With δ = 2^-53:
/// * k = 0: y = RN(u·h), T = 1, T_lo = 0, the rest exact: h's error ≤ 1.002δ, the product δ, the
///   Taylor remainder u^5/720 ≤ 0.9δ relative: ≤ 2.91δ.
/// * k ≠ 0: |x| ≥ ln2/256, so |u| ≤ |k|·ln2/256 and |E - 1| ≤ 2.003|f|, |E·q| ≤ 1.003|f|,
///   E·|u| ≤ 1.01|f| (the worst case is k = ±1). Terms: RN(2^m·T - 1) ≤ 2.003δ; u's error, scaled by
///   E/|f| ≤ 372: ≤ 1.07δ; h, u·h, T·q (1.002 + 1 + 1)δ·1.003; the sum T_lo + T·q 1.003δ; the
///   Taylor remainder 0.9δ·1.01; T_lo's rounding ≤ 2^-96; the last sum δ. Total ≤ 9.0δ, so 9.1δ.
///
/// Every intermediate is a normal `double` or zero (Horner keeps u's powers out), so only inexact
/// is raised in hardware.
fn expm1_core(x: f32) -> Option<f64> {
    let x = f64::from(x);
    // NaN, x ≤ -18, x ≥ 89 and |x| < 2^-45 (the zeros and subnormals included) go to the accurate path.
    if !(x > -18.0 && x < 89.0) || x.to_bits() & !(1 << 63) < 0x3d20_0000_0000_0000 {
        return None;
    }
    let kd = x * INV_L + SHIFT;
    let k = kd.to_bits() as i32;
    let kd = kd - SHIFT;
    let u = (x - kd * L_HI) - kd * L_LO;
    let j = (k & 127) as usize;
    let t = EXP2_TAB[j];
    let p = f64::from_bits((((k >> 7) + 1023) as u64) << 52);
    let h = 1.0 + u * (0.5 + u * (C3 + u * (C4 + u * C5)));
    let hi = t * p - 1.0;
    Some(hi + p * (EXP2_LO[j] + t * (u * h)))
}

/// ∛x as a `double` y with |y/∛x - 1| ≤ 5.4·2^-53, for a finite nonzero `float` x.
///
/// |x| = 2^e·m with e, m, i, r as in [`log_core`] (r = m·LOG_INV[i] - 1 exact, |r| < 2^-7; the
/// `double` of a subnormal `float` is normal). e = 3q + j, j in {0, 1, 2}. Then
/// ∛|x| = 2^q·CBRT_S[j]·CBRT_T[i]·(1 + r)^(1/3) exactly with the unrounded table values, since
/// m = (1 + r)/LOG_INV[i]. (1 + r)^(1/3) = 1 + r·g, g the binomial series to degree 6: the
/// remainder ≤ |C(1/3, 7)|·|r|^7·1.01 ≤ 0.019·2^-49 < 0.31δ (δ = 2^-53). g's coefficient and
/// Horner roundings are ≤ 2.2δ|g| and scaled by |r·g| < 2^-8.5: ≤ 0.01δ; the sum 1 + r·g δ. The
/// two table entries δ each (CBRT_S[0] = 1 exact), the products S·T and ·p δ each, 2^q exact.
/// Total ≤ 5.33δ, so 5.4δ. Every operation is on normal `double`s: only inexact in hardware.
fn cbrt_core(x: f32) -> Option<f64> {
    let a = x.to_bits() & 0x7fff_ffff;
    if a.wrapping_sub(1) >= 0x7f7f_ffff {
        return None; // ±0, ±∞, NaN: the accurate path's special cases
    }
    let xb = f64::from(f32::from_bits(a)).to_bits();
    let e = (xb.wrapping_sub(0x3fe8_0000_0000_0000) as i64 >> 52) as i32;
    let m = f64::from_bits(xb.wrapping_sub((e as i64 as u64) << 52));
    let i = ((m.to_bits() >> 45) & 127) as usize;
    let r = m * LOG_INV[i] - 1.0;
    let p = 1.0 + r * (B1 + r * (B2 + r * (B3 + r * (B4 + r * (B5 + r * B6)))));
    // e in [-149, 128]: e + 150 = 3(q + 50) + j with a nonnegative dividend.
    let n = (e + 150) as u32;
    let (q, j) = ((n / 3) as i32 - 50, (n % 3) as usize);
    let y = CBRT_S[j] * CBRT_T[i] * p * f64::from_bits(((q + 1023) as u64) << 52);
    Some(if x < 0.0 { -y } else { y })
}

/// |x|^y as a `double` with |result/|x|^y - 1| ≤ 89·2^-53 (MATH-M6-F32-POWF), for a finite
/// nonzero `float` x, a finite nonzero `float` y and t = y·ln|x| in (-104, 89); the sign is the
/// caller's. No new tables: ln|x| comes from LOG_INV/LOG_C, e^t from EXP2_TAB. Its kernels are
/// its own (log1p's series D3..D8, expm1's L_HI/L_LO and C3..C5), not [`log_core`]'s and
/// [`exp_core`]'s shorter ones (MATH-M6-F32-KERNEL), whose 2^-45 bounds it does not inherit.
///
/// 1. ln|x| = H + lo, with e, i, r the reduction of [`log_core`] (r exact) and
///    s = r²·(-1/2 + r·(D3 + … + r·D8)), the series of [`log1p_core`]. a = RN(e·LN2_HI + LOG_C[i])
///    and H = RN(a + r) carry their rounding errors exactly (Fast2Sum: e·LN2_HI is exact and
///    either 0 or ≥ 0.69 > 0.41 ≥ |LOG_C| in magnitude; a is 0 or |a| ≥ 0.0058 > 0.0040 ≥ |r|).
///    lo = h_lo + (a_lo + (s + e·LN2_LO)). Errors relative to |ln x|: LOG_C's rounding
///    ≤ 2^-54|LOG_C| ≤ 2^-54·1.51|ln x| (|LOG_C| ≤ 1.51|ln x| in every case of the reduction); the
///    Taylor remainder and the evaluation of s ≤ 2^-58.6|r| ≤ 2^-58.6·1.004|ln x|; the sums in lo
///    (|lo| ≤ 2^-7.9|H|) and LN2_LO's products below 2^-60. In all δ_L ≤ 0.81·2^-53.
/// 2. t = y·(H + lo): H1 = H to 24 significant bits, H2 = H - H1 (exact); P = y·H1 is exact
///    (24 by 24 bits), Q = RN(y·RN(H2 + lo)) with |H2 + lo| ≤ 2^-7.8|H|, so its two roundings
///    add ≤ 2^-60|t|. t_hi = RN(P + Q), t_lo = (P - t_hi) + Q exactly (Fast2Sum, |Q| < |P|).
///    So |Δt| ≤ 0.83·2^-53|t| < 86.5·2^-53 for |t| < 104.
/// 3. e^t: k = round(128·t_hi/ln 2), |k| ≤ 19207 < 2^15 for |t| < 104. D = t_hi - k·L_HI is
///    exact (L_HI has 38 significant bits, so k·L_HI is exact and on the 2^-45 grid; when k ≠ 0,
///    |t_hi| > 2^-9 so t_hi is on the 2^-61 grid; |D| < 2^-8). u = D + (t_lo - k·L_LO): L_HI + L_LO
///    is within 2^-99.9 of ln 2/128 (≤ 2^-84.9 after ·k), |k·L_LO| < 2^-32 and |t_lo| ≤ 2^-46 round
///    below 2^-84, and the last sum's rounding ≤ 2^-53·2^-8.5: ≤ 2^-61 absolute added in all.
///    e^u = 1 + u·(1 + u·(1/2 + u·(C3 + u·(C4 + u·C5)))): Taylor remainder |u|^6/720·1.003 < 2^-60.7
///    (|u| < 0.00272), the coefficients' and Horner's roundings scaled by |u| ≤ 2^-8.5 (< 2^-60
///    in all); v = T + T·(u·h): T's rounding 2^-53, the last sum's 2^-53, the product T·(u·h)'s
///    ≤ 2^-53·0.0028; the scaling by 2^(k>>7) is exact. ≤ 1.02·2^-52 relative.
///
/// Total: e^|Δt| - 1 + 1.02·2^-52 + 2^-61 ≤ (0.84|t| + 2.06)·2^-53 < 89·2^-53 (|t| < 104).
fn pow_core(ax: f64, yd: f64) -> Option<f64> {
    let xb = ax.to_bits();
    let e = (xb.wrapping_sub(0x3fe8_0000_0000_0000) as i64 >> 52) as i32;
    let m = f64::from_bits(xb.wrapping_sub((e as i64 as u64) << 52));
    let i = ((m.to_bits() >> 45) & 127) as usize;
    let r = m * LOG_INV[i] - 1.0;
    let s = r * r * (-0.5 + r * (D3 + r * (D4 + r * (D5 + r * (D6 + r * (D7 + r * D8))))));
    let ef = f64::from(e);
    let a0 = ef * LN2_HI;
    let a = a0 + LOG_C[i];
    let a_lo = (a0 - a) + LOG_C[i];
    let h = a + r;
    let h_lo = (a - h) + r;
    let lo = h_lo + (a_lo + (s + ef * LN2_LO));
    let h1 = f64::from_bits(h.to_bits() & 0xffff_ffff_e000_0000);
    let p = yd * h1;
    let q = yd * ((h - h1) + lo);
    let t = p + q;
    if !(t > -104.0 && t < 89.0) {
        return None; // overflow, underflow to 0 and NaN-free bounds: the accurate path
    }
    let t_lo = (p - t) + q;
    let kd = t * INV_L + SHIFT;
    let k = kd.to_bits() as i32;
    let kd = kd - SHIFT;
    let u = (t - kd * L_HI) + (t_lo - kd * L_LO);
    let tt = EXP2_TAB[(k & 127) as usize];
    let hh = 1.0 + u * (0.5 + u * (C3 + u * (C4 + u * C5)));
    let v = tt + tt * (u * hh);
    // k >> 7 is in [-151, 128]: 2^(k>>7) is a normal `double`.
    Some(v * f64::from_bits((((k >> 7) + 1023) as u64) << 52))
}

/// `powf`'s fast path: x, y finite and nonzero, x > 0 or y an integer (the sign from its parity).
/// Every other operand (the C Annex F special cases, ±0, ±∞, NaN, x < 0 with y not an integer)
/// and every result the rounding test refuses (the exact powers among them) go to [`crate::pow`].
#[inline(always)]
pub fn eval_pow(x: f32, y: f32, mode: Rounding) -> Option<(f32, Status)> {
    let (xb, yb) = (x.to_bits(), y.to_bits());
    let (xa, ya) = (xb & 0x7fff_ffff, yb & 0x7fff_ffff);
    if xa.wrapping_sub(1) >= 0x7f7f_ffff || ya.wrapping_sub(1) >= 0x7f7f_ffff {
        return None; // ±0, ±∞, NaN
    }
    let mut neg = false;
    if xb >> 31 != 0 {
        // y must be an integer: |y| ≥ 2^23, or no fraction bits below its units bit.
        let ey = (ya >> 23) as i32 - 127;
        if ey < 0 {
            return None;
        }
        if ey < 23 {
            let sig = (ya & 0x7f_ffff) | 0x80_0000;
            let sh = 23 - ey as u32;
            if sig & ((1 << sh) - 1) != 0 {
                return None;
            }
            neg = (sig >> sh) & 1 == 1;
        }
    }
    let y0 = pow_core(f64::from(f32::from_bits(xa)), f64::from(y))?;
    let r = round(if neg { -y0 } else { y0 }, POW_DELTA, mode);
    stats::record("powf", r.is_some());
    let v = r?;
    // round's range is the normal `float`s and it refuses exact results: inexact only, as in
    // [`finish`]; denormal-operand from `f64::from` of a subnormal x or y.
    let denormal = x.is_subnormal() || y.is_subnormal();
    let hw = if denormal { Flags::INEXACT.union(Status::HW_DENORMAL) } else { Flags::INEXACT };
    Some((v, Status::flags(Flags::INEXACT).hw(hw)))
}

/// `powf`: the fast path, else the accurate path.
pub fn pow(x: f32, y: f32, mode: Rounding) -> (f32, Status) {
    match eval_pow(x, y, mode) {
        Some(r) => r,
        None => crate::pow::pow(x, y, mode),
    }
}

// asinf, acosf, atanf, atan2f (LIBM-F32-DOUBLE-KERNELS): one `double` reduction of atan, then
// the rounding test; the accurate path is `crate::trig`'s double-double. Errors below are in
// units of u = 2^-53 relative unless marked absolute.

/// The proved bounds of the inverse trigonometric kernels, in ulp of y (the proofs are at
/// [`atan_red`] and its callers); each `*_DELTA` must exceed its bound (a test checks).
#[cfg(test)]
const ATAN_BOUND: f64 = 7.0;
#[cfg(test)]
const ASIN_BOUND: f64 = 10.4;
#[cfg(test)]
const ATAN2_BOUND: f64 = 10.7;
/// atanf: ε ≤ 7.0u. 16 keeps a margin of more than 2.
const ATAN_DELTA: u64 = 16;
/// asinf, acosf: ε ≤ 10.4u.
const ASIN_DELTA: u64 = 24;
/// atan2f: ε ≤ 10.7u.
const ATAN2_DELTA: u64 = 32;
/// The odd minimax polynomial for atan on [-1/64, 1/64] (`crates/math/fits/fast32_atan.fit`,
/// coefficient of u fixed at 1).
const AT3: f64 = f64::from_bits(crate::coeffs::fast32_atan::FAST32_ATAN[1]);
const AT5: f64 = f64::from_bits(crate::coeffs::fast32_atan::FAST32_ATAN[2]);
const AT7: f64 = f64::from_bits(crate::coeffs::fast32_atan::FAST32_ATAN[3]);
/// ATAN32[i] = RN(atan(i/32)), PIO2 = RN(π/2), PI = RN(π): generated by
/// `crates/math/fits/fast32_atan_table_gen.py` (100-digit decimal arithmetic).
const ATAN32: [f64; 33] = [
    f64::from_bits(0x0000000000000000), f64::from_bits(0x3f9ffd55bba97625), f64::from_bits(0x3faff55bb72cfdea),
    f64::from_bits(0x3fb7ee182602f10f), f64::from_bits(0x3fbfd5ba9aac2f6e), f64::from_bits(0x3fc3d6eee8c6626c),
    f64::from_bits(0x3fc7b97b4bce5b02), f64::from_bits(0x3fcb90d7529260a2), f64::from_bits(0x3fcf5b75f92c80dd),
    f64::from_bits(0x3fd18bf5a30bf178), f64::from_bits(0x3fd362773707ebcc), f64::from_bits(0x3fd530ad9951cd4a),
    f64::from_bits(0x3fd6f61941e4def1), f64::from_bits(0x3fd8b24d394a1b25), f64::from_bits(0x3fda64eec3cc23fd),
    f64::from_bits(0x3fdc0db4c94ec9f0), f64::from_bits(0x3fddac670561bb4f), f64::from_bits(0x3fdf40dd0b541418),
    f64::from_bits(0x3fe0657e94db30d0), f64::from_bits(0x3fe1255d9bfbd2a9), f64::from_bits(0x3fe1e00babdefeb4),
    f64::from_bits(0x3fe2958e59308e31), f64::from_bits(0x3fe345f01cce37bb), f64::from_bits(0x3fe3f13fb89e96f4),
    f64::from_bits(0x3fe4978fa3269ee1), f64::from_bits(0x3fe538f57b89061f), f64::from_bits(0x3fe5d58987169b18),
    f64::from_bits(0x3fe66d663923e087), f64::from_bits(0x3fe700a7c5784634), f64::from_bits(0x3fe78f6bbd5d315e),
    f64::from_bits(0x3fe819d0b7158a4d), f64::from_bits(0x3fe89ff5ff57f1f8), f64::from_bits(0x3fe921fb54442d18),
];
const PIO2: f64 = f64::from_bits(0x3ff921fb54442d18);
const PI: f64 = f64::from_bits(0x400921fb54442d18);

/// √x for a normal `double` x in [2^-60, 2^260] (the estimate's error depends only on the
/// exponent's parity and every intermediate stays normal), relative error ≤ 2u, in safe code (the crate
/// forbids `unsafe`, and the `sqrtsd` intrinsic needs it here). An estimate y of 1/√x from the
/// halved exponent (relative error ≤ 3.4%), three Newton steps y·(1.5 - (x/2)·y²) (error
/// e → 1.5e² + e³/2: 3.4% → 1.8·10^-3 → 4.9·10^-6 → 3.6·10^-11, each step's roundings a few u
/// below that), s = RN(x·y) (≤ 3.6·10^-11 + u), then one Heron correction s + (y/2)·(x - s²): the
/// method's residual is quadratic (< 2^-66); s² rounds by ≤ u·x, which (y/2) turns into ≤ 0.5u
/// of s (x - RN(s²) is exact by Sterbenz), the correction's own roundings are < 2^-34·u, and the
/// last sum rounds once (u): ≤ 1.6u. Every operand is a normal `double`: inexact only.
#[inline(always)]
fn sqrt64(x: f64) -> f64 {
    let mut y = f64::from_bits(0x5fe6_eb50_c7b5_37a9u64.wrapping_sub(x.to_bits() >> 1));
    let h = 0.5 * x;
    for _ in 0..3 {
        y *= 1.5 - h * y * y;
    }
    let s = x * y;
    s + (0.5 * y) * (x - s * s)
}

/// atan s (`inv` false) or π/2 - atan s (`inv` true) for an exact `double` s in [0, 1], with
/// relative error ≤ 6.6u (atan) or ≤ 4.6u (π/2 - atan).
///
/// Reduction: c = i/32 the nearest table point (s·32 + SHIFT rounds to the integer i ≤ 32;
/// |s - c| ≤ 1/64) and atan s = atan c + atan u, u = (s - c)/(1 + s·c), |u| ≤ 1/64. For i ≥ 1,
/// c/2 ≤ s ≤ 2c, so s - c is exact (Sterbenz); for i = 0 it is s and the divisor 1 + 0 is
/// exact. The divisor (in [1, 2]) has error ≤ 2^-54 + 2^-53 = 1.5u, the quotient u: u carries
/// ≤ 2.5u. p(u) = u + u·u²·(AT3 + u²·(AT5 + u²·AT7)) is the minimax fit (error ≤ 2^-57.95 < 0.1u
/// relative, the fit file's report); the u³ term is < 2^-13.6·|u| so its rounding errors are
/// < 2^-66·|u|, and the last sum rounds once (u): p carries ≤ 2.5u + 0.1u + u = 3.6u of |p|.
///
/// atan: A + p, A = ATAN32[i] (≤ u·A). The worst case is i = 1, s = 1/64 (atan s ≥ 0.015624,
/// A ≤ 0.031240, |p| ≤ 1/64): (A·u + 3.6u·|p| + u·atan s)/atan s ≤ 2.0u + 3.6u + 1u = 6.6u; for
/// i ≥ 2 the ratios A/atan s and |p|/atan s are smaller, and for i = 0 the result is p (3.6u less
/// the divisor's 1.5u and the quotient's u, since u = s).
/// π/2 - atan: (PIO2 - A) - p, absolute in 2^-53: PIO2 0.5, A 0.5, the difference (in [π/4,
/// π/2]) 0.5, p 3.6/64, the last rounding 1 (result < 2): ≤ 2.6, over a result ≥ π/4: ≤ 3.4u;
/// with the slack for the rounding of s by the callers (below) the stated 4.6u holds.
///
/// A caller's s with relative error δ moves atan s by at most δ·s/(1 + s²) ≤ δ·atan s (s ≤ 1),
/// and π/2 - atan s by at most δ/2 absolute, ≤ 0.64δ relative (result ≥ π/4).
#[inline(always)]
fn atan_red(s: f64, inv: bool) -> f64 {
    let k = s * 32.0 + SHIFT;
    let i = (k.to_bits() & 63) as usize;
    let c = (k - SHIFT) * (1.0 / 32.0);
    let u = (s - c) / (1.0 + s * c);
    let u2 = u * u;
    let p = u + u * u2 * (AT3 + u2 * (AT5 + u2 * AT7));
    if inv { (PIO2 - ATAN32[i]) - p } else { ATAN32[i] + p }
}

/// atan x for finite x, ε ≤ 7.0u: s = |x| exact when |x| ≤ 1 (6.6u); else s = RN(1/|x|)
/// (δ = u, 0.64u) under π/2 - atan (4.6u ≥ 3.4u + 0.64u). ±0 gives 0 (the rounding test refuses
/// it); infinities and NaN go to the accurate path.
#[inline(always)]
fn atan_core(x: f32) -> Option<f64> {
    let a = crate::dd::abs(f64::from(x));
    if !(0.0..f64::INFINITY).contains(&a) {
        return None;
    }
    let y = if a <= 1.0 { atan_red(a, false) } else { atan_red(1.0 / a, true) };
    Some(if x < 0.0 { -y } else { y })
}

/// asin x for |x| < 1, ε ≤ 10.4u: asin a = atan(a/d), d = √(1 - a²). a² is exact (24-bit a),
/// 1 - a² exact for a ≥ 1/8 (on the 2^-48 grid) and within u otherwise; [`sqrt64`] adds 2u and
/// halves the rest; the quotient a/d or d/a adds u: δ ≤ 0.5u + 2u + u = 3.5u. The larger
/// of the kernel's bounds plus δ: 6.6u + 3.5u = 10.1u. |x| = 1, |x| > 1, infinities and NaN go
/// to the accurate path.
#[inline(always)]
fn asin_core(x: f32) -> Option<f64> {
    let a = crate::dd::abs(f64::from(x));
    if !(0.0..1.0).contains(&a) {
        return None;
    }
    let d = sqrt64(1.0 - a * a);
    let y = if a <= d { atan_red(a / d, false) } else { atan_red(d / a, true) };
    Some(if x < 0.0 { -y } else { y })
}

/// acos x for |x| < 1, ε ≤ 10.4u: acos x = 2·atan √((1 - x)/(1 + x)), and for x < 0 the
/// reciprocal under π - 2·atan. 1 ± x is exact for |x| ≥ 2^-29 and within u otherwise; the
/// quotient adds u, [`sqrt64`] halves that (≤ 0.75u) and adds 2u: δ ≤ 2.75u. Doubling is
/// exact: 6.6u + 2.75u = 9.35u. ±1, |x| > 1, infinities and NaN go to the accurate path.
#[inline(always)]
fn acos_core(x: f32) -> Option<f64> {
    let x = f64::from(x);
    if !(x > -1.0 && x < 1.0) {
        return None;
    }
    let (p, q) = (1.0 - x, 1.0 + x);
    let y = if x >= 0.0 { atan_red(sqrt64(p / q), false) } else { atan_red(sqrt64(q / p), true) };
    Some(2.0 * y)
}

/// atan2(y, x) as a `double` for finite nonzero y and x, ε ≤ 10.7u: t = atan(s), s = min/max of
/// |y| and |x| (δ = u; s ≥ 2^-277 is a normal `double`), 7.6u; for x < 0, π - t: absolute in
/// 2^-53, t ≤ 7.6u·π/2 = 12.0, PI 2, the last rounding 2: ≤ 16.0 over a result ≥ π/2: 10.2u.
#[inline(always)]
fn atan2_core(y: f32, x: f32) -> Option<f64> {
    let (b, a) = (f64::from(y), f64::from(x));
    let (ay, ax) = (crate::dd::abs(b), crate::dd::abs(a));
    if !(ay > 0.0 && ay < f64::INFINITY && ax > 0.0 && ax < f64::INFINITY) {
        return None;
    }
    let t = if ay <= ax { atan_red(ay / ax, false) } else { atan_red(ax / ay, true) };
    let t = if a < 0.0 { PI - t } else { t };
    Some(if b < 0.0 { -t } else { t })
}

/// `atan2f` by the fast path, or `None` (see [`eval`]; zeros, infinities and NaN are the
/// accurate path's, and so are tiny results, which the rounding test refuses).
#[inline(always)]
pub fn eval_atan2(y: f32, x: f32, mode: Rounding) -> Option<(f32, Status)> {
    let r = round(atan2_core(y, x)?, ATAN2_DELTA, mode);
    stats::record("atan2f", r.is_some());
    let v = r?;
    // As in [`finish`]: inexact; denormal-operand from `f64::from` of a subnormal operand.
    let denormal = x.is_subnormal() || y.is_subnormal();
    let hw = if denormal { Flags::INEXACT.union(Status::HW_DENORMAL) } else { Flags::INEXACT };
    Some((v, Status::flags(Flags::INEXACT).hw(hw)))
}

/// `atan2f`: the fast path, else the accurate path.
pub fn atan2(y: f32, x: f32, mode: Rounding) -> (f32, Status) {
    match eval_atan2(y, x, mode) {
        Some(r) => r,
        None => crate::trig::atan2(y, x, mode),
    }
}

// The hyperbolic family (LIBM-F32-HYPERBOLIC). No new polynomial or table: the kernels reuse
// expm1_core, exp_core, sqrt64 and the log tables, so there is no Remez fit to record. u = 2^-53.
#[cfg(test)]
const SINH_BOUND: f64 = 12.2;
#[cfg(test)]
const COSH_BOUND: f64 = 233.0;
#[cfg(test)]
const TANH_BOUND: f64 = 11.2;
#[cfg(test)]
const ASINH_BOUND: f64 = 11.5;
#[cfg(test)]
const ACOSH_BOUND: f64 = 12.3;
#[cfg(test)]
const ATANH_BOUND: f64 = 9.3;
#[cfg(test)]
const LOG1P64_BOUND: f64 = 8.2;
/// Margins (≥ 1.5·bound + 1 ulp, checked by a test).
const SINH_DELTA: u64 = 20;
const COSH_DELTA: u64 = 512;
const TANH_DELTA: u64 = 18;
const ASINH_DELTA: u64 = 20;
const ACOSH_DELTA: u64 = 20;
const ATANH_DELTA: u64 = 16;
/// The small-|x| cut of the family: below 2^-26 the result is x or 1 plus far less than a
/// `float` ulp, which the rounding test refuses in the directed modes; the accurate path takes them.
const HYP_MIN: f64 = 1.490_116_119_384_765_6e-8;
/// tanh a for a ≥ TANH_SAT lies in (1 - 2^-25, 1): see [`tanh_core`].
const TANH_SAT: f64 = 9.5;

/// ln(1 + w) for a `double` w in [2^-26, 2^130], relative error ≤ 8.2u.
///
/// z = RN(1 + w) ≥ 1, z = 2^e·m, i, L = LOG_INV[i] and LOG_C as in [`log_core`]; ln(1 + w) =
/// e·ln 2 + LOG_C[i] + ln(1 + r) with r = L·(1 + w)/2^e - 1, and the series, e·LN2_HI (exact:
/// e ≤ 130, 44 + 8 bits), e·LN2_LO and the sums as in [`log1p_core`] (3.2u in all) but the
/// series one degree shorter than log1p_core's: ln(1 + r) = r + r²(-1/2 + r·D3 + ... + r^5·D7).
/// Its remainder, r^8/8 - r^9/9 + ..., is below |r|^8/8 for |r| < 2^-7 (|m·L - 1| < 2^-7). r:
/// * e = 0 (z in [1, 1.5), w < 0.5): r = L·w + (L - 1) without rounding z's error in: w = wh + wl
///   with wh its top 28 bits, so wh·L (52 bits) and L - 1 (24 bits, L in [2/3, 1]) are exact. For
///   i = 0, L = 1 and r = wh + wl = w exactly, r in [2^-26, 2^-7), y ≥ r(1 - r/2): the remainder
///   is ≤ 1.004·2^-49/8 = 2.01u relative (case A: 3.2u + 2.01u ≤ 5.3u). For i ≥ 1, w ≥ 2^-8,
///   so wh·L is on the 2^-59 grid, L - 1 on 2^-24, their sum below 2^-7: exact; wl·L rounds by
///   ≤ u·2^-28 and the last sum by u·|r|, |r| < 0.0040 ≤ 1.03|y| (|y| ≥ ln(1 + 2^-8)): 4.3u, and
///   the remainder 0.004^8/8 < 0.02u·|y| (case B: 4.4u).
/// * e ≥ 1 (|y| ≥ ln 1.5 > 0.405): z's rounding moves ln z by ≤ u absolute, ≤ 2.47u relative;
///   RN(m·L) by ≤ u (then the subtraction of 1 is exact, Sterbenz), ln(1 + r) by ≤ 1.004u, 2.48u
///   relative; the remainder ≤ 2^-56/8 absolute, 0.04u relative: 3.2u + 2.47u + 2.48u + 0.04u
///   ≤ 8.2u (case C).
///
/// The overall bound is case C's, so the r^8 term (D8) of log1p_core's series buys nothing here.
///
/// A caller's w with relative error η moves the result by η·w/((1 + w)·ln(1 + w)) ≤ η.
/// Every operand is a normal `double` (w ≥ 2^-26): only inexact is raised.
#[inline(always)]
fn log1p64(w: f64) -> f64 {
    let z = 1.0 + w;
    let zb = z.to_bits();
    let e = (zb.wrapping_sub(0x3fe8_0000_0000_0000) as i64 >> 52) as i32;
    let m = f64::from_bits(zb.wrapping_sub((e as i64 as u64) << 52));
    let i = ((m.to_bits() >> 45) & 127) as usize;
    let l = LOG_INV[i];
    let r = if e == 0 {
        let wh = f64::from_bits(w.to_bits() & !0x1ff_ffff);
        (wh * l + (l - 1.0)) + (w - wh) * l
    } else {
        m * l - 1.0
    };
    let s = r * r * (-0.5 + r * (D3 + r * (D4 + r * (D5 + r * (D6 + r * D7)))));
    let ef = f64::from(e);
    (ef * LN2_HI + LOG_C[i]) + (r + (s + ef * LN2_LO))
}

/// |x| as a `double` when HYP_MIN ≤ |x| < `hi` (NaN fails both tests).
#[inline(always)]
fn hyp_arg(x: f32, hi: f64) -> Option<f64> {
    let a = crate::dd::abs(f64::from(x));
    if a >= HYP_MIN && a < hi { Some(a) } else { None }
}

/// sinh x, ε ≤ 12.2u, for 2^-26 ≤ |x| < 89: y = (E + E/(E + 1))/2, E = e^a - 1 by
/// [`expm1_core`] (9.1u). E/(E + 1) moves by 9.1u/(1 + E) ≤ 9.1u for E's error, plus the sum
/// E + 1 and the quotient (u each): 11.1u; the sum of the two positive terms adds u (12.1u, with
/// the second-order terms 12.2u); halving is exact (y ≥ 2^-27). The overflow range and the cut's
/// small operands are the accurate path's.
#[inline(always)]
fn sinh_core(x: f32) -> Option<f64> {
    let a = hyp_arg(x, 89.0)?;
    let e = expm1_core(a as f32)?; // a is a `float` (exact)
    let y = (e + e / (e + 1.0)) * 0.5;
    Some(if x < 0.0 { -y } else { y })
}

/// cosh x, ε ≤ 2^-45.14 (< 233 ulp), for 2^-26 ≤ |x| < 89.5: y = (E + 1/E)/2, E = e^a by
/// [`exp_core`] (2^-45.16; its proof holds up to 104), 1/E adds u, the positive sum u, halving
/// exact. Results above FLT_MAX are refused by [`round`]: the accurate path's overflow.
#[inline(always)]
fn cosh_core(x: f32) -> Option<f64> {
    let a = hyp_arg(x, 89.5)? as f32;
    let e = exp_core(a, -1.0, 89.5, INV_L)?;
    Some((e + 1.0 / e) * 0.5)
}

/// tanh x, ε ≤ 11.2u, for 2^-26 ≤ |x| < 9.5: y = E/(E + 2), E = e^(2a) - 1 by [`expm1_core`]
/// (2a exact, < 19; 9.1u). The quotient moves by 9.1u·2/(E + 2) ≤ 9.1u for E's error, plus the
/// sum and the division (u each): 11.1u, 11.2u with second-order terms.
/// For finite |x| ≥ 9.5: 1 - tanh a = 2/(e^(2a) + 1) < 2^-26.4, so tanh a and y = 1 - 2^-30 lie
/// strictly inside (1 - 2^-25, 1), between two adjacent rounding boundaries (1 - 2^-25 the
/// midpoint, 1 a `float`): they round alike in every direction, inexact (the rounding test
/// accepts y, whose low 28 bits are far from 0 and 2^28). ∞ and NaN go to the accurate path.
#[inline(always)]
fn tanh_core(x: f32) -> Option<f64> {
    let a = hyp_arg(x, f64::INFINITY)?;
    let y = if a >= TANH_SAT {
        1.0 - 9.313_225_746_154_785e-10
    } else {
        let e = expm1_core((a + a) as f32)?;
        e / (e + 2.0)
    };
    Some(if x < 0.0 { -y } else { y })
}

/// asinh x, ε ≤ 11.5u, for finite |x| ≥ 2^-26: y = ln(1 + w), w = a + a²/(1 + √(1 + a²)).
/// a² is exact (24-bit a, ≥ 2^-52), 1 + a² rounds (u), [`sqrt64`] adds 2u and halves the rest
/// (2.5u), 1 + √ (3.5u: the sum's u and √'s share ≤ 1), the quotient q (4.5u); q < a so a + q
/// carries ≤ 4.5u/2 + u = 3.25u; [`log1p64`] passes that on (≤ 3.25u) and adds 8.2u.
#[inline(always)]
fn asinh_core(x: f32) -> Option<f64> {
    let a = hyp_arg(x, f64::INFINITY)?;
    let a2 = a * a;
    let y = log1p64(a + a2 / (1.0 + sqrt64(1.0 + a2)));
    Some(if x < 0.0 { -y } else { y })
}

/// acosh x, ε ≤ 12.3u, for finite x > 1: y = ln(1 + w), w = t + √(t·(t + 2)), t = x - 1.
/// t is exact for x < 2^53 and within u above; t + 2 and the product round (u each), [`sqrt64`]
/// halves that and adds 2u (3u), the positive sum adds u: w carries ≤ 4u for x < 2^53 (above, the
/// error passed on is ≤ 5u/ln 2^53 < 0.14u); [`log1p64`] adds 8.2u: 12.2u, 12.3u with slack.
#[inline(always)]
fn acosh_core(x: f32) -> Option<f64> {
    let x = f64::from(x);
    if !(x > 1.0 && x < f64::INFINITY) {
        return None;
    }
    let t = x - 1.0;
    Some(log1p64(t + sqrt64(t * (t + 2.0))))
}

/// atanh x, ε ≤ 9.3u, for 2^-26 ≤ |x| < 1: y = ln(1 + w)/2, w = 2a/(1 - a); 1 - a is exact (a on
/// the 2^-49 grid) and the quotient rounds (u); [`log1p64`] adds 8.2u; halving is exact.
#[inline(always)]
fn atanh_core(x: f32) -> Option<f64> {
    let a = hyp_arg(x, 1.0)?;
    let y = 0.5 * log1p64((a + a) / (1.0 - a));
    Some(if x < 0.0 { -y } else { y })
}

/// The rounding test and the final rounding of y = f·(1 + θ), |y - f| < `delta` ulp(y).
///
/// The `float` results of f and y in a direction can differ only if a rounding boundary (a
/// `float`, or the midpoint of two) lies between them; so can the inexact flag (f a `float`).
/// These boundaries are the multiples of half a `float` ulp, the low `s` - 1 bits of y's
/// significand being 0 (s = 29 for a normal result, more for a subnormal one). When those bits
/// are more than `delta` from 0 and from the next multiple, f and y round alike in every
/// direction and f is inexact. y is limited to [2^-126, FLT_MAX + ulp/2), so the result is a
/// normal `float` in every direction (2^-126 is itself a boundary): no overflow, no underflow, no
/// errno. Subnormal results (rare: x < -87.3 for expf) are left to the accurate path, which keeps
/// this test to one fixed mask (s = 29).
#[inline(always)]
fn round(y: f64, delta: u64, mode: Rounding) -> Option<f32> {
    const LO: u64 = 0x3810_0000_0000_0000;
    let a = y.to_bits() & !(1 << 63);
    // (low + delta) mod 2^28 ≤ 2·delta exactly when low ≤ delta or low ≥ 2^28 - delta.
    if a.wrapping_sub(LO) >= 0x47ef_ffff_e000_0000 - LO || (a.wrapping_add(delta) & 0x0fff_ffff) <= 2 * delta {
        return None;
    }
    // Round to nearest in hardware (the default MXCSR), then step for a directed mode.
    let r = y as f32;
    let back = f64::from(r);
    let b = r.to_bits();
    let neg = y < 0.0;
    let bits = match mode {
        Rounding::Up if back < y => if neg { b.wrapping_sub(1) } else { b.wrapping_add(1) },
        Rounding::Down if back > y => if neg { b.wrapping_add(1) } else { b.wrapping_sub(1) },
        Rounding::Zero if (if neg { back < y } else { back > y }) => b.wrapping_sub(1),
        _ => b,
    };
    Some(f32::from_bits(bits))
}

/// `f(x)` rounded to nearest by the fast path, for the in-place entry of a caller in the default
/// MXCSR (MATH-M6-ENVELOPE-COST prototype), or `None`. Where it answers, its hardware arithmetic
/// raises exactly inexact: the operations of [`eval`] on normal `double`s, the final conversion
/// inexact by the rounding test; a subnormal `x` (whose conversion raises denormal-operand) is
/// refused here (exp refuses it anyway: y is exactly 1).
#[inline(always)]
pub fn eval_nearest(f: Fast, x: f32) -> Option<f32> {
    let (y, delta) = match f {
        Fast::Exp => (exp_core(x, -104.0, 89.0, INV_L)?, EXP_DELTA),
        Fast::Exp2 => (exp_core(x, -150.5, 128.0, 128.0)?, EXP_DELTA),
        Fast::Exp10 => (exp_core(x, -45.5, 38.6, INV_M)?, EXP_DELTA),
        Fast::Log => (log_core_from(x, 0x0080_0000)?, LOG_DELTA),
        Fast::Log2 => (log_core_from(x, 0x0080_0000)? * INV_LN2, LOG_DELTA),
        Fast::Log10 => (log_core_from(x, 0x0080_0000)? * INV_LN10, LOG_DELTA),
        // trig_core takes only normal x (2^-26 ≤ |x| < 2^19, so no denormal-operand), keeps every
        // intermediate a normal `double` or zero (its proof) and |c| ≥ 2^-37, so s/c raises no
        // divide-by-zero, overflow or underflow: inexact only (LIBM-F32-SINF-SHRINK).
        Fast::Sin => (trig_core(x)?.0, TRIG_DELTA),
        Fast::Cos => (trig_core(x)?.1, TRIG_DELTA),
        Fast::Tan => {
            let (s, c) = trig_core(x)?;
            (s / c, TAN_DELTA)
        }
        // No in-place nearest entry yet (their flags on subnormal operands are unproven here):
        // the caller takes [`eval`].
        Fast::Log1p | Fast::Expm1 | Fast::Cbrt | Fast::Asin | Fast::Acos | Fast::Atan
        | Fast::Sinh | Fast::Cosh | Fast::Tanh | Fast::Asinh | Fast::Acosh | Fast::Atanh => return None,
    };
    round(y, delta, Rounding::Nearest)
}

macro_rules! nearest_fn {
    ($name:ident, $f:expr) => {
        #[doc = concat!("`", stringify!($name), "` for `float` rounded to nearest by the fast path ([`eval_nearest`]).")]
        #[inline]
        pub fn $name(x: f32) -> Option<f32> {
            eval_nearest($f, x)
        }
    };
}
nearest_fn!(exp_nearest, Fast::Exp);
nearest_fn!(exp2_nearest, Fast::Exp2);
nearest_fn!(exp10_nearest, Fast::Exp10);
nearest_fn!(log_nearest, Fast::Log);
nearest_fn!(log2_nearest, Fast::Log2);
nearest_fn!(log10_nearest, Fast::Log10);
nearest_fn!(sin_nearest, Fast::Sin);
nearest_fn!(cos_nearest, Fast::Cos);
nearest_fn!(tan_nearest, Fast::Tan);

macro_rules! fast_fn {
    ($name:ident, $f:expr) => {
        fast_fn!($name, $f, exp_log);
    };
    ($name:ident, $f:expr, $acc:ident) => {
        #[doc = concat!("`", stringify!($name), "` for `float`: the fast path, else the accurate path.")]
        pub fn $name(x: f32, mode: Rounding) -> (f32, Status) {
            match eval($f, x, mode) {
                Some(r) => r,
                None => crate::$acc::$name(x, mode),
            }
        }
    };
}
fast_fn!(exp, Fast::Exp);
fast_fn!(exp2, Fast::Exp2);
fast_fn!(exp10, Fast::Exp10);
fast_fn!(log, Fast::Log);
fast_fn!(log2, Fast::Log2);
fast_fn!(log10, Fast::Log10);
fast_fn!(log1p, Fast::Log1p);
fast_fn!(sin, Fast::Sin, trig);
fast_fn!(cos, Fast::Cos, trig);
fast_fn!(tan, Fast::Tan, trig);
fast_fn!(asin, Fast::Asin, trig);
fast_fn!(acos, Fast::Acos, trig);
fast_fn!(atan, Fast::Atan, trig);
fast_fn!(expm1, Fast::Expm1);
fast_fn!(cbrt, Fast::Cbrt, pow);
fast_fn!(sinh, Fast::Sinh, hyper);
fast_fn!(cosh, Fast::Cosh, hyper);
fast_fn!(tanh, Fast::Tanh, hyper);
fast_fn!(asinh, Fast::Asinh, hyper);
fast_fn!(acosh, Fast::Acosh, hyper);
fast_fn!(atanh, Fast::Atanh, hyper);

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exp_log as acc;

    const ALL: [Fast; 21] = [Fast::Exp, Fast::Exp2, Fast::Exp10, Fast::Log, Fast::Log2, Fast::Log10, Fast::Log1p, Fast::Sin, Fast::Cos, Fast::Tan, Fast::Expm1, Fast::Cbrt, Fast::Asin, Fast::Acos, Fast::Atan, Fast::Sinh, Fast::Cosh, Fast::Tanh, Fast::Asinh, Fast::Acosh, Fast::Atanh];
    const NAMES: [&str; 21] = ["expf", "exp2f", "exp10f", "logf", "log2f", "log10f", "log1pf", "sinf", "cosf", "tanf", "expm1f", "cbrtf", "asinf", "acosf", "atanf", "sinhf", "coshf", "tanhf", "asinhf", "acoshf", "atanhf"];
    const TRIG: [Fast; 3] = [Fast::Sin, Fast::Cos, Fast::Tan];
    const MODES: [Rounding; 4] = [Rounding::Nearest, Rounding::Up, Rounding::Down, Rounding::Zero];

    fn accurate(f: Fast, x: f32, m: Rounding) -> (f32, Status) {
        match f {
            Fast::Exp => acc::exp(x, m),
            Fast::Exp2 => acc::exp2(x, m),
            Fast::Exp10 => acc::exp10(x, m),
            Fast::Log => acc::log(x, m),
            Fast::Log2 => acc::log2(x, m),
            Fast::Log10 => acc::log10(x, m),
            Fast::Log1p => acc::log1p(x, m),
            Fast::Sin => crate::trig::sin(x, m),
            Fast::Cos => crate::trig::cos(x, m),
            Fast::Tan => crate::trig::tan(x, m),
            Fast::Expm1 => acc::expm1(x, m),
            Fast::Cbrt => crate::pow::cbrt(x, m),
            Fast::Asin => crate::trig::asin(x, m),
            Fast::Acos => crate::trig::acos(x, m),
            Fast::Atan => crate::trig::atan(x, m),
            Fast::Sinh => crate::hyper::sinh(x, m),
            Fast::Cosh => crate::hyper::cosh(x, m),
            Fast::Tanh => crate::hyper::tanh(x, m),
            Fast::Asinh => crate::hyper::asinh(x, m),
            Fast::Acosh => crate::hyper::acosh(x, m),
            Fast::Atanh => crate::hyper::atanh(x, m),
        }
    }

    /// Compare the fast path with the accurate path on `xs` in every mode; the number taken.
    fn agree(xs: impl Iterator<Item = f32> + Clone) -> u64 {
        agree_in(&ALL, xs)
    }

    /// [`agree`] for the functions `fs`; with trig among them also `sincosf` against `sinf`, `cosf`.
    fn agree_in(fs: &[Fast], xs: impl Iterator<Item = f32> + Clone) -> u64 {
        let mut taken = 0;
        if fs.contains(&Fast::Sin) {
            for m in MODES {
                for x in xs.clone() {
                    let ((s, c), st) = sincos(x, m);
                    let (ws, wt) = sin(x, m);
                    let (wc, wu) = cos(x, m);
                    assert_eq!((m, x.to_bits(), s.to_bits(), c.to_bits(), st), (m, x.to_bits(), ws.to_bits(), wc.to_bits(), wt.and(wu)), "sincosf");
                }
            }
        }
        for &f in fs {
            for m in MODES {
                for x in xs.clone() {
                    if let Some(r) = eval(f, x, m) {
                        taken += 1;
                        let want = accurate(f, x, m);
                        assert_eq!((f, m, x.to_bits(), r.0.to_bits(), r.1), (f, m, x.to_bits(), want.0.to_bits(), want.1));
                    }
                }
            }
        }
        taken
    }

    #[test]
    fn tables_and_constants_are_the_rounded_values() {
        for (j, t) in EXP2_TAB.iter().enumerate() {
            assert_eq!(t.to_bits(), acc::exp2(j as f64 / 128.0, Rounding::Nearest).0.to_bits(), "EXP2_TAB[{j}]");
        }
        for i in 0..128 {
            let c = if i == 0 || i == 127 { 1.0 } else if i < 64 { 1.0 + (i as f64 + 0.5) / 128.0 } else { (1.0 + (i as f64 + 0.5) / 128.0) / 2.0 };
            assert_eq!(LOG_INV[i].to_bits() & 0x1fff_ffff, 0, "LOG_INV[{i}] has 24 bits");
            assert!((LOG_INV[i] * c - 1.0).abs() < 1.0 / 16_777_216.0, "LOG_INV[{i}]");
            assert_eq!(LOG_C[i].to_bits(), (0.0 - acc::log(LOG_INV[i], Rounding::Nearest).0).to_bits(), "LOG_C[{i}]"); // +0 for c = 1
        }
        for (j, t) in EXP2_BITS.iter().enumerate() {
            assert_eq!(*t, EXP2_TAB[j].to_bits(), "EXP2_BITS[{j}]");
        }
        let ln2 = acc::log(2.0f64, Rounding::Nearest).0;
        assert_eq!((LN2, E1 * 128.0), (ln2, ln2));
        // E2..E4: (ln 2/128)^k/k! to well within the bound's 2^-61 (exact products of E1 in
        // double-double would be overkill: these are polynomial coefficients, not table values).
        let e = [E1, E1 * E1 / 2.0, E1 * E1 * E1 / 6.0, E1 * E1 * E1 * E1 / 24.0];
        for (k, c) in [E1, E2, E3, E4].into_iter().enumerate() {
            assert!((c / e[k] - 1.0).abs() < 1e-15, "E{}", k + 1);
        }
        assert_eq!((INV_LN2, INV_LN10), (acc::log2(core::f64::consts::E, Rounding::Nearest).0, acc::log10(core::f64::consts::E, Rounding::Nearest).0));
        assert_eq!((INV_L / 128.0, INV_M / 128.0), (INV_LN2, acc::log2(10.0f64, Rounding::Nearest).0));
        // EXP2_LO[j] = RN(2^(j/128) - EXP2_TAB[j]), 2^(j/128) from the accurate path's double-double exp.
        // The reference needs a full double-double ln 2: LN2_HI + LN2_LO holds only about 89 bits
        // (both have trailing zeros), an error near 2^-95 in 2^(j/128), far above EXP2_LO's own
        // rounding (≤ 2^-107). ln2_dd is checked here: RN(ln 2) as its head, and e^ln2_dd = 2.
        let ln2_dd = crate::dd::two_sum(f64::from_bits(0x3fe6_2e42_fefa_39ef), f64::from_bits(0x3c7a_bc9e_3b39_803f));
        assert_eq!(ln2_dd.to_f64(), ln2);
        let (two, m2) = acc::exp_dd(ln2_dd);
        assert!(two.scale(crate::dd::pow2(m2)).add_f64(-2.0).to_f64().abs() <= 2f64.powi(-102), "e^ln2_dd");
        // Tolerance 2^-100 absolute: the reference's error (ln2_dd ≤ 2^-106, exp_dd ≤ 2^-102 as just
        // checked at the far end) plus EXP2_LO's rounding stay below it on a correct table, and it
        // certifies |T + T_lo - 2^(j/128)| ≤ 2^-99, inside the 2^-96 that expm1_core's proof uses.
        // A one-ulp change of an entry (2^-106) is within the proof's slack: equivalent by design.
        for j in 0..128 {
            let (v, m) = acc::exp_dd(ln2_dd.mul_f64(j as f64 / 128.0));
            let lo = v.scale(crate::dd::pow2(m)).sub(crate::dd::Dd::from(EXP2_TAB[j])).to_f64();
            assert!((EXP2_LO[j] - lo).abs() <= 2f64.powi(-100), "EXP2_LO[{j}]: {:e}", EXP2_LO[j] - lo);
        }
        // CBRT_T[i]^3·LOG_INV[i] = 1 within the rounding of CBRT_T[i] (≤ 3/2 ulp relative), CBRT_S[j]^3 = 2^j.
        for i in 0..128 {
            let t = CBRT_T[i];
            let c = crate::dd::Dd::from(t).sqr().mul_f64(t).mul_f64(LOG_INV[i]).add_f64(-1.0).to_f64();
            let ulp = f64::from_bits(t.to_bits() & 0x7ff0_0000_0000_0000) * f64::EPSILON;
            assert!(c.abs() <= 1.5 * ulp / t * (1.0 + 1e-6), "CBRT_T[{i}]: {c:e}");
        }
        for (j, s) in CBRT_S.iter().enumerate() {
            assert_eq!(s.to_bits(), crate::pow::cbrt((1u32 << j) as f64, Rounding::Nearest).0.to_bits(), "CBRT_S[{j}]");
        }
        // Trig: the π/64 parts (27 and 29 bits, their sum π/64 within 2^-119), the exact zeros and
        // ones of the sin(kπ/64) table and every entry against sin² + cos² = 1.
        assert_eq!((PI64_1.to_bits() & 0x3ff_ffff, PI64_2.to_bits() & 0xff_ffff), (0, 0));
        let pi64 = crate::dd::Dd::of(t::PI64);
        let parts = crate::dd::Dd::from(PI64_1).add_f64(PI64_2).add_f64(PI64_3);
        assert!(parts.sub(pi64).to_f64().abs() < 1e-34, "PI64 parts");
        assert_eq!(INV_PI64, crate::dd::Dd::from(1.0).div(pi64).hi, "INV_PI64");
        let tab = |k: usize| crate::dd::Dd::of(t::SIN_K_PI64[k]);
        assert_eq!([0, 32, 64, 96].map(|k| f64::from_bits(t::SIN_K_PI64[k][0])), [0.0, 1.0, 0.0, -1.0]);
        for k in 0..128 {
            let (s, c) = (tab(k), tab((k + 32) & 127));
            assert!(s.sqr().add(c.sqr()).add_f64(-1.0).to_f64().abs() < 1e-30, "SIN_K_PI64[{k}]");
        }
    }

    /// The rounding test's mask form refuses exactly the margins of the old two-sided test, and
    /// the range keeps every result a normal `float`.
    #[test]
    fn rounding_test_margins_and_range() {
        for low in [0u64, 1, EXP_DELTA - 1, EXP_DELTA, EXP_DELTA + 1, (1 << 28) - EXP_DELTA - 1, (1 << 28) - EXP_DELTA, (1 << 28) - 1] {
            let y = f64::from_bits(0x3ff0_0000_0000_0000 | (5 << 28) | low);
            let refused = low <= EXP_DELTA || low >= (1 << 28) - EXP_DELTA;
            assert_eq!(round(y, EXP_DELTA, Rounding::Nearest).is_none(), refused, "low {low}");
        }
        let mid = 1 << 27;
        assert!(round(f64::from_bits(0x3810_0000_0000_0000 + mid), EXP_DELTA, Rounding::Nearest).is_some());
        assert!(round(f64::from_bits(0x380f_ffff_0000_0000 + mid), EXP_DELTA, Rounding::Nearest).is_none());
        assert!(round(f64::from_bits(0x47ef_ffff_c000_0000 + mid), EXP_DELTA, Rounding::Nearest).is_some());
        assert!(round(f64::from_bits(0x47f0_0000_0000_0000 + mid), EXP_DELTA, Rounding::Nearest).is_none());
    }

    #[test]
    fn fast_path_agrees_with_the_accurate_path() {
        // A stride over every encoding, every float in two ranges with subnormal exp results,
        // the operands near 1 for the logs, and the exact cases (which the test must refuse).
        // The fallback rate (at most 1 in 10^4) on operands in the functions' working ranges: a
        // grid over [-30, 30] for the exps and every 4001st positive normal `float` for the logs.
        // (Over all encodings most exp operands are tiny, e^x is next to 1 and the test refuses
        // them by design.) Measured here, not in a test of its own: the counters are global.
        let exps = (0..1u32 << 17).map(|i| -30.0 + 60.0 * (i as f32) / 131_072.0);
        let logs = (0x0080_0000u32..0x7f80_0000).step_by(4001).map(f32::from_bits);
        // log1pf: a grid over [-0.9, 10] (the bench row's range).
        let log1ps = (0..1u32 << 17).map(|i| -0.9 + 10.9 * (i as f32) / 131_072.0);
        // sinf, cosf, tanf: a grid over [-100, 100].
        let trigs = (0..1u32 << 17).map(|i| -100.0 + 200.0 * (i as f32) / 131_072.0);
        for (i, (f, n)) in ALL.into_iter().zip(NAMES).enumerate() {
            let before = stats::counts(n);
            let xs: std::vec::Vec<f32> = match i {
                0..=2 | 10 => exps.clone().collect(),
                6 => log1ps.clone().collect(),
                7..=9 => trigs.clone().collect(),
                11 => logs.clone().flat_map(|x| [x, -x]).collect(),
                // asinf, acosf over [-1, 1] and atanf over [-10, 10] (the bench rows' ranges).
                12 | 13 => (0..1u32 << 17).map(|i| -1.0 + 2.0 * (i as f32) / 131_072.0).collect(),
                14..=18 => trigs.clone().map(|x| x / 10.0).collect(),
                // acoshf over [1, 100], atanhf over [-0.99, 0.99] (the bench rows' ranges).
                19 => (0..1u32 << 17).map(|i| 1.0 + 99.0 * (i as f32) / 131_072.0).collect(),
                20 => (0..1u32 << 17).map(|i| -0.99 + 1.98 * (i as f32) / 131_072.0).collect(),
                _ => logs.clone().collect(),
            };
            for x in xs {
                let _ = eval(f, x, Rounding::Nearest);
            }
            let c = stats::counts(n).since(before);
            assert!(c.check_fallback(n, 10_000).is_ok(), "{n}: fallback rate {c:?}");
        }
        let stride = (0..=u32::MAX).step_by(4099).map(f32::from_bits);
        assert!(agree(stride) > 1_500_000);
        let sub = (0xc2cf_0000u32..0xc2d0_0000).step_by(7).chain((0xc313_0000u32..0xc314_0000).step_by(7)).map(f32::from_bits);
        agree(sub.clone());
        let fast_tiny = sub.filter(|&x| MODES.into_iter().any(|m| eval(Fast::Exp, x, m).is_some() || (x < -126.0 && eval(Fast::Exp2, x, m).is_some())));
        assert_eq!(fast_tiny.count(), 0, "subnormal results are the accurate path's");
        let near1 = (0x3f7f_0000u32..0x3f81_0000).step_by(3).map(f32::from_bits);
        assert!(agree(near1) > 100_000);
        // log1pf: around its 2^-26 cut, where e changes (z = 0.75, 1.5, 2) and near -1.
        let near0 = [0x3280_0000u32, 0xb280_0000, 0x3e80_0000, 0xbe80_0000, 0x3f00_0000, 0x3f80_0000, 0xbf7f_0000]
            .into_iter().flat_map(|b| (b - 0x8000..b + 0x8000).step_by(5)).map(f32::from_bits);
        assert!(agree(near0) > 50_000);
        // Trig: around the 2^-26 and 2^19 cuts and near multiples of π/2 and π/64.
        let cuts = [0x3280_0000u32, 0xb280_0000, 0x4900_0000, 0xc900_0000, 0x3fc9_0fdb, 0x40c9_0fdb, 0x3d49_0fdb, 0x4b49_0fdb]
            .into_iter().flat_map(|b| (b - 0x8000..b + 0x8000).step_by(5)).map(f32::from_bits);
        assert!(agree_in(&TRIG, cuts) > 50_000);
        let exact = (-149..128).flat_map(|k| [k as f32, 2f32.powi(k)]).chain((0..11).map(|k| 10f32.powi(k)));
        agree(exact);
    }

    /// log1p_core's y is within its proved bound (< 6.9 ulp of y) of ln(1 + x), measured against
    /// the double-double `log1p_dd` (error far below 2^-100). The agreement test cannot see a
    /// loss of a few ulp of the `double`: it misrounds only next to a `float` boundary.
    #[test]
    fn log1p_core_is_within_its_bound() {
        let grid = (0..1u32 << 16).map(|i| -0.99 + 20.0 * (i as f32) / 65_536.0);
        let wide = (0x3280_0000u32..0x7f80_0000).step_by(1009).flat_map(|b| [b, b | 1 << 31]).map(f32::from_bits);
        let mut seen = 0u32;
        for x in grid.chain(wide) {
            if let Some(y) = log1p_core(x) {
                seen += 1;
                let f = crate::exp_log::log1p_dd(crate::dd::Dd::from(f64::from(x)));
                let err = (crate::dd::Dd::from(y).sub(f)).to_f64();
                let ulp = f64::from_bits(y.to_bits() & 0x7ff0_0000_0000_0000) / 4_503_599_627_370_496.0;
                assert!(err.to_bits() & !(1 << 63) <= (6.9 * ulp).to_bits(),"log1p_core({x:e}): {} ulp", err / ulp);
            }
        }
        assert!(seen > 1_000_000, "{seen}");
    }

    /// trig_core's sin, cos (< TRIG_BOUND ulp) and tan = s/c (< TAN_BOUND ulp) against the
    /// double-double `sin_cos` of the accurate path (its error far below 2^-100); the margins
    /// TRIG_DELTA, TAN_DELTA are at least 1.5·bound + 1. (No spec clause: LIBM-F32-SINF-SHRINK.)
    #[test]
    fn trig_core_is_within_its_bound() {
        let grid = (0..1u32 << 16).map(|i| -40.0 + 80.0 * (i as f32) / 65_536.0);
        let wide = (0x3280_0000u32..0x4900_0000).step_by(1009).flat_map(|b| [b, b | 1 << 31]).map(f32::from_bits);
        let ulp = |y: f64| f64::from_bits(y.to_bits() & 0x7ff0_0000_0000_0000) / 4_503_599_627_370_496.0;
        let mut seen = 0u32;
        for x in grid.chain(wide) {
            if let Some((s, c)) = trig_core(x) {
                seen += 1;
                let (fs, fc) = crate::trig::sin_cos(f64::from(x));
                for (y, f, b, n) in [(s, fs, TRIG_BOUND, "sin"), (c, fc, TRIG_BOUND, "cos"), (s / c, fs.div(fc), TAN_BOUND, "tan")] {
                    let err = crate::dd::Dd::from(y).sub(f).to_f64();
                    assert!(err.abs() <= b * ulp(y), "trig_core {n}({x:e}): {} ulp", err / ulp(y));
                }
            }
        }
        assert!(seen > 750_000, "{seen}");
        for (d, b) in [(TRIG_DELTA, TRIG_BOUND), (TAN_DELTA, TAN_BOUND)] {
            assert!(d as f64 >= 1.5 * b + 1.0, "margin {d} ulp for the bound {b} ulp");
        }
    }

    /// `fs` on every `float` operand in every mode; the number taken. The threads take 2^16-operand
    /// chunks from one counter, so one that meets a cheap range (NaN, a refused domain) takes more
    /// chunks instead of idling while another finishes a fixed slice of expensive operands.
    /// `LIELLA_SWEEP_STRIDE=k` checks every k-th encoding only (sizing a sweep before its job).
    fn exhaustive(fs: &'static [Fast]) -> u64 {
        use std::sync::atomic::{AtomicU64, Ordering};
        const CHUNK: u64 = 1 << 16;
        let n = std::thread::available_parallelism().map_or(8, |n| n.get());
        let stride = std::env::var("LIELLA_SWEEP_STRIDE").ok().and_then(|s| s.parse::<usize>().ok()).filter(|&k| k > 0).unwrap_or(1);
        let next = AtomicU64::new(0);
        let (taken, seen) = std::thread::scope(|sc| {
            let hs: std::vec::Vec<_> = (0..n).map(|_| sc.spawn(|| {
                let (mut taken, mut seen) = (0, 0);
                loop {
                    let lo = next.fetch_add(CHUNK, Ordering::Relaxed);
                    if lo >= 1 << 32 {
                        return (taken, seen);
                    }
                    let xs = (lo..lo + CHUNK).step_by(stride).map(|b| f32::from_bits(b as u32));
                    seen += xs.clone().count() as u64;
                    taken += agree_in(fs, xs);
                }
            })).collect();
            hs.into_iter().map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e))).fold((0, 0), |a, b| (a.0 + b.0, a.1 + b.1))
        });
        // Every chunk exactly once: a lost or repeated chunk would pass unseen otherwise.
        assert_eq!(seen, ((1u64 << 32) / CHUNK) * CHUNK.div_ceil(stride as u64), "sweep coverage (stride {stride})");
        taken
    }

    /// ATAN32, PIO2 and PI are the rounded values (against the double-double `atan_dd`, error far
    /// below 2^-100), and each inverse trigonometric margin exceeds its proved bound by half and
    /// one ulp. (No spec clause: LIBM-F32-DOUBLE-KERNELS.)
    #[test]
    fn inverse_trig_tables_and_margins() {
        use crate::dd::Dd;
        for (i, &a) in ATAN32.iter().enumerate() {
            assert_eq!(a.to_bits(), crate::trig::atan_dd(Dd::from(i as f64 / 32.0)).to_f64().to_bits(), "ATAN32[{i}]");
        }
        let q = crate::trig::atan_dd(Dd::from(1.0));
        assert_eq!(PIO2.to_bits(), q.scale(2.0).to_f64().to_bits());
        assert_eq!(PI.to_bits(), q.scale(4.0).to_f64().to_bits());
        for (d, b) in [(ATAN_DELTA, ATAN_BOUND), (ASIN_DELTA, ASIN_BOUND), (ATAN2_DELTA, ATAN2_BOUND)] {
            assert!(d as f64 >= 1.5 * b + 1.0, "margin {d} ulp for the bound {b} ulp");
        }
    }

    /// The double-double references of the inverse trigonometric kernels: atan2(y, x) by the
    /// octant (asin and acos are atan2(a, √(1 - a²)) and atan2(√(1 - x), √(1 + x))·2).
    fn atan2_dd(y: crate::dd::Dd, x: crate::dd::Dd) -> crate::dd::Dd {
        let half = crate::trig::atan_dd(crate::dd::Dd::from(1.0)).scale(2.0);
        let (ay, ax) = (if y.hi < 0.0 { y.neg() } else { y }, if x.hi < 0.0 { x.neg() } else { x });
        let t = if ay.sub(ax).hi <= 0.0 { crate::trig::atan_dd(ay.div(ax)) } else { half.sub(crate::trig::atan_dd(ax.div(ay))) };
        let t = if x.hi < 0.0 { half.scale(2.0).sub(t) } else { t };
        if y.hi < 0.0 { t.neg() } else { t }
    }

    /// atan_core (≤ ATAN_BOUND ulp), asin_core and acos_core (≤ ASIN_BOUND) and atan2_core
    /// (≤ ATAN2_BOUND) against [`atan2_dd`]. The agreement tests cannot see a loss of a few ulp of
    /// the `double`: it misrounds only next to a `float` boundary. (No spec clause.)
    #[test]
    fn inverse_trig_cores_are_within_their_bounds() {
        use crate::dd::{two_sum, Dd};
        let ulp = |y: f64| f64::from_bits(y.to_bits() & 0x7ff0_0000_0000_0000) / 4_503_599_627_370_496.0;
        let check = |y: Option<f64>, f: Dd, b: f64, what: &str, x: f32| {
            assert!(y.is_some(), "{what}({x:e}) refused");
            let y = y.unwrap_or(f64::NAN);
            let err = Dd::from(y).sub(f).to_f64();
            assert!(err.abs() <= b * ulp(y), "{what}({x:e}): {} ulp", err / ulp(y));
        };
        let one = Dd::from(1.0);
        // sqrt64 (≤ 2u) over its working range (1 - a², (1 ± x)/(1 ∓ x): 2^-50 to 1).
        for k in (0x3cd0_0000_0000_0000u64..=0x3ff0_0000_0000_0000).step_by(0x0000_0f00_0f00_0f01) {
            let x = f64::from_bits(k);
            let (s, f) = (sqrt64(x), crate::dd::sqrt(Dd::from(x)));
            assert!(Dd::from(s).sub(f).to_f64().abs() <= 2.0 * s / 9_007_199_254_740_992.0, "sqrt64({x:e})");
        }
        let mut seen = 0u32;
        // Every 61st `float` in [2^-26, 1), both signs, then atan up to 2^30.
        for b in (0x3280_0000u32..0x3f80_0000).step_by(61).flat_map(|b| [b, b | 1 << 31]) {
            let x = f32::from_bits(b);
            let a = f64::from(x);
            check(atan_core(x), atan2_dd(Dd::from(a), one), ATAN_BOUND, "atan_core", x);
            let d = crate::dd::sqrt(two_sum(1.0, -a).mul(two_sum(1.0, a)));
            check(asin_core(x), atan2_dd(Dd::from(a), d), ASIN_BOUND, "asin_core", x);
            let (p, q) = (crate::dd::sqrt(two_sum(1.0, -a)), crate::dd::sqrt(two_sum(1.0, a)));
            check(acos_core(x), atan2_dd(p, q).scale(2.0), ASIN_BOUND, "acos_core", x);
            seen += 1;
        }
        for b in (0x3f80_0000u32..0x4e80_0000).step_by(61).flat_map(|b| [b, b | 1 << 31]) {
            let x = f32::from_bits(b);
            check(atan_core(x), atan2_dd(Dd::from(f64::from(x)), one), ATAN_BOUND, "atan_core", x);
        }
        // atan2: ratios across the range, every quadrant and the octant edges.
        let vals: std::vec::Vec<f32> = (0x0080_0000u32..0x7f80_0000).step_by(0x7_0001).map(f32::from_bits).collect();
        for &u in &vals {
            for &v in &vals {
                for (y, x) in [(u, v), (-u, v), (u, -v), (-u, -v), (u, u), (u, -u)] {
                    let f = atan2_dd(Dd::from(f64::from(y)), Dd::from(f64::from(x)));
                    check(atan2_core(y, x), f, ATAN2_BOUND, "atan2_core", y);
                }
            }
        }
        assert!(seen > 1_000_000, "{seen}");
    }

    /// atan2f's fast path against the accurate path in every mode: pairs across the range with
    /// both signs, the octant edges |y| = |x| ± 1 ulp, the bench row's square [-10, 10]² (and its
    /// fallback rate), and the C Annex F F.10.1.4 special cases (zeros, infinities, NaN), which
    /// the fast path must refuse. (Clause s10_0_f_0's Annex F reference; LIBM-F32-DOUBLE-KERNELS.)
    #[test]
    fn atan2_fast_path_agrees_with_the_accurate_path() {
        let mut taken = 0u64;
        let mut pair = |y: f32, x: f32| {
            for m in MODES {
                if let Some(r) = eval_atan2(y, x, m) {
                    taken += 1;
                    let want = crate::trig::atan2(y, x, m);
                    assert_eq!((m, y.to_bits(), x.to_bits(), r.0.to_bits(), r.1), (m, y.to_bits(), x.to_bits(), want.0.to_bits(), want.1));
                }
            }
        };
        let vals: std::vec::Vec<f32> = (1u32..0x7f80_0000).step_by(0x1f_0003).map(f32::from_bits).collect();
        for &u in &vals {
            for &v in &vals {
                for (y, x) in [(u, v), (-u, v), (u, -v), (-u, -v)] {
                    pair(y, x);
                }
            }
            let (up, dn) = (f32::from_bits(u.to_bits() + 1), f32::from_bits(u.to_bits() - 1));
            for (y, x) in [(u, u), (up, u), (dn, u), (-u, -u), (up, -u), (-dn, -u)] {
                pair(y, x);
            }
        }
        let before = stats::counts("atan2f");
        for i in 0..256u32 {
            for j in 0..256u32 {
                pair(-10.0 + 20.0 * (i as f32 + 0.37) / 256.0, -10.0 + 20.0 * (j as f32 + 0.61) / 256.0);
            }
        }
        let c = stats::counts("atan2f").since(before);
        assert!(c.check_fallback("atan2f", 10_000).is_ok(), "atan2f: fallback rate {c:?}");
        assert!(taken > 400_000, "{taken}");
        let (inf, nan) = (f32::INFINITY, f32::NAN);
        for y in [0.0, -0.0, inf, -inf, nan, 1.5, -1.5, f32::MIN_POSITIVE / 4.0] {
            for x in [0.0, -0.0, inf, -inf, nan, 1.5, -1.5] {
                let special = !(y.is_finite() && x.is_finite() && y != 0.0 && x != 0.0);
                for m in MODES {
                    let r = eval_atan2(y, x, m);
                    assert!(!special || r.is_none(), "atan2f({y}, {x}) taken");
                    let (got, want) = (atan2(y, x, m), crate::trig::atan2(y, x, m));
                    assert_eq!((got.0.to_bits(), got.1), (want.0.to_bits(), want.1), "atan2f({y}, {x}) {m:?}");
                }
            }
        }
    }

    /// asinf on every `float` operand in every mode against the accurate path (one job per
    /// function, 2^34 calls; LIBM-F32-DOUBLE-KERNELS's accuracy check). (No spec clause.)
    #[test]
    #[ignore]
    fn asinf_fast_path_agrees_exhaustively() {
        let taken = exhaustive(&[Fast::Asin]);
        std::println!("asinf fast path taken {taken} of {} calls", 4u64 << 32);
    }

    /// acosf on every `float` operand in every mode (as for asinf). (No spec clause.)
    #[test]
    #[ignore]
    fn acosf_fast_path_agrees_exhaustively() {
        let taken = exhaustive(&[Fast::Acos]);
        std::println!("acosf fast path taken {taken} of {} calls", 4u64 << 32);
    }

    /// atanf on every `float` operand in every mode (as for asinf). (No spec clause.)
    #[test]
    #[ignore]
    fn atanf_fast_path_agrees_exhaustively() {
        let taken = exhaustive(&[Fast::Atan]);
        std::println!("atanf fast path taken {taken} of {} calls", 4u64 << 32);
    }

    /// atan2f on 2^24 y (every 256th encoding) against 64 x spread over the range and both signs,
    /// every mode (atan2f has 2^64 operand pairs: a sample, in its own job).
    #[test]
    #[ignore]
    fn atan2_fast_path_agrees_on_a_wide_sample() {
        let n = std::thread::available_parallelism().map_or(8, |n| n.get()) as u32;
        let xs: std::vec::Vec<f32> = (0..32u32).map(|k| f32::from_bits(0x0040_0000 + k * 0x03f8_0000 + k * 977)).flat_map(|v| [v, -v]).collect();
        let taken: u64 = std::thread::scope(|sc| {
            let hs: std::vec::Vec<_> = (0..n).map(|t| {
                let xs = &xs;
                sc.spawn(move || {
                    let mut taken = 0u64;
                    for k in (t..1u32 << 24).step_by(n as usize) {
                        let y = f32::from_bits(k << 8 | (k & 0xff));
                        for &x in xs {
                            for m in MODES {
                                if let Some(r) = eval_atan2(y, x, m) {
                                    taken += 1;
                                    let want = crate::trig::atan2(y, x, m);
                                    assert_eq!((m, y.to_bits(), x.to_bits(), r.0.to_bits(), r.1), (m, y.to_bits(), x.to_bits(), want.0.to_bits(), want.1));
                                }
                            }
                        }
                    }
                    taken
                })
            }).collect();
            hs.into_iter().map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e))).sum()
        });
        std::println!("atan2f fast path taken {taken} of {} calls", (1u64 << 24) * 64 * 4);
    }

    /// The in-place nearest entries of sinf, cosf, tanf, where they answer, give the accurate
    /// path's round-to-nearest result. (No spec clause: LIBM-F32-SINF-SHRINK.) Not through
    /// [`eval`]: its statistics are shared with the fallback-rate check of the agreement test.
    #[test]
    fn trig_nearest_entries_match_the_accurate_path() {
        let grid = (0..1u32 << 16).map(|i| -40.0 + 80.0 * (i as f32) / 65_536.0);
        let wide = (0u32..0x7f80_0001).step_by(4099).flat_map(|b| [b, b | 1 << 31]).map(f32::from_bits);
        let m = Rounding::Nearest;
        let mut taken = 0u32;
        for x in grid.chain(wide) {
            for (f, near) in [(Fast::Sin, sin_nearest(x)), (Fast::Cos, cos_nearest(x)), (Fast::Tan, tan_nearest(x))] {
                let Some(v) = near else { continue };
                taken += 1;
                let a = match f {
                    Fast::Sin => crate::trig::sin(x, m),
                    Fast::Cos => crate::trig::cos(x, m),
                    _ => crate::trig::tan(x, m),
                };
                assert_eq!(v.to_bits(), a.0.to_bits(), "{f:?}_nearest({x:e})");
            }
        }
        assert!(taken > 500_000, "{taken}");
    }

    /// Each hyperbolic margin exceeds its proved bound by half and one ulp. (No spec clause:
    /// LIBM-F32-HYPERBOLIC.)
    #[test]
    fn hyperbolic_margins() {
        for (b, d, n) in [(SINH_BOUND, SINH_DELTA, "sinh"), (COSH_BOUND, COSH_DELTA, "cosh"), (TANH_BOUND, TANH_DELTA, "tanh"), (ASINH_BOUND, ASINH_DELTA, "asinh"), (ACOSH_BOUND, ACOSH_DELTA, "acosh"), (ATANH_BOUND, ATANH_DELTA, "atanh")] {
            assert!(d as f64 >= 1.5 * b + 1.0, "{n}: margin {d} ulp for a bound of {b} ulp");
        }
        // The log1p64-based bounds: the argument's error (passed on at most 1:1) plus log1p64's.
        for (w, b, n) in [(3.25, ASINH_BOUND, "asinh"), (4.0, ACOSH_BOUND, "acosh"), (1.0, ATANH_BOUND, "atanh")] {
            assert!(LOG1P64_BOUND + w <= b, "{n}: {w} + log1p64's bound over {b}");
        }
    }

    /// The hyperbolic cores and log1p64 are within their proved bounds, against double-double
    /// references built from the accurate path's `exp_dd`, `expm1_small`, `log1p_dd` and `dd::sqrt`
    /// (errors far below 2^-90 relative): every 1021st `float` from 2^-26 up, both signs, plus a
    /// grid over the bench rows' ranges. tanh's saturated range is checked by the agreement tests.
    /// The agreement tests cannot see a loss of a few ulp of the `double`. (No spec clause.)
    #[test]
    fn hyperbolic_cores_are_within_their_bounds() {
        use crate::dd::Dd;
        use crate::exp_log::{exp_dd, expm1_small, log1p_dd};
        let ulp = |y: f64| f64::from_bits(y.to_bits() & 0x7ff0_0000_0000_0000) / 4_503_599_627_370_496.0;
        let check = |y: Option<f64>, f: Dd, b: f64, what: &str, x: f64| {
            assert!(y.is_some(), "{what}({x:e}) refused");
            let y = y.unwrap_or(f64::NAN);
            let err = Dd::from(y).sub(f).to_f64();
            assert!(err.abs() <= b * ulp(y), "{what}({x:e}): {} ulp", err / ulp(y));
        };
        let expm1 = |a: f64| if a <= 0.34 { expm1_small(Dd::from(a)) } else { let (v, m) = exp_dd(Dd::from(a)); v.scale(crate::dd::pow2(m)).add_f64(-1.0) };
        let one = Dd::from(1.0);
        // log1p64 on doubles (not only on the w the callers form): every scale from 2^-26 to 2^130,
        // and densely over [2^-8, 2^-7), where case A's series remainder peaks. Each case against
        // its own bound (A: z < 1 + 2^-7, 5.3u; B: z < 1.5, 4.4u; C: LOG1P64_BOUND).
        let scales = (0x3e50_0000_0000_0000u64..0x4810_0000_0000_0000).step_by(0x0000_0f00_0f00_0f01);
        let case_a_top = (0x3f70_0000_0000_0000u64..0x3f80_0000_0000_0000).step_by(0x0000_0040_0000_0001);
        for k in scales.chain(case_a_top) {
            let w = f64::from_bits(k);
            let z = 1.0 + w;
            let b = if z < 1.0 + 1.0 / 128.0 { 5.3 } else if z < 1.5 { 4.4 } else { LOG1P64_BOUND };
            check(Some(log1p64(w)), log1p_dd(Dd::from(w)), b, "log1p64", w);
        }
        // sqrt64 over the range the family adds (1 + a², t(t + 2): up to 2^257).
        for k in (0x3ff0_0000_0000_0000u64..=0x5030_0000_0000_0000).step_by(0x0000_0f00_0f00_0f01) {
            let x = f64::from_bits(k);
            let (s, f) = (sqrt64(x), crate::dd::sqrt(Dd::from(x)));
            assert!(Dd::from(s).sub(f).to_f64().abs() <= 2.0 * s / 9_007_199_254_740_992.0, "sqrt64({x:e})");
        }
        let grid = (0..1u32 << 15).map(|i| f32::from_bits(0x3280_0000) + 100.0 * (i as f32) / 32_768.0);
        let wide = (0x3280_0000u32..0x7f80_0000).step_by(1021).map(f32::from_bits);
        let mut seen = 0u32;
        for x in grid.chain(wide) {
            let a = f64::from(x);
            let da = Dd::from(a);
            for s in [1.0f32, -1.0] {
                let xs = x * s;
                let sg = |v: Dd| if s < 0.0 { v.neg() } else { v };
                if a < 89.0 {
                    let e = expm1(a);
                    check(sinh_core(xs), sg(e.add(e.div(e.add_f64(1.0))).scale(0.5)), SINH_BOUND, "sinh_core", a);
                }
                if a < 89.5 {
                    let (v, m) = exp_dd(da);
                    let e = v.scale(crate::dd::pow2(m));
                    check(cosh_core(xs), e.add(one.div(e)).scale(0.5), COSH_BOUND, "cosh_core", a);
                }
                if a < TANH_SAT {
                    let e = expm1(2.0 * a);
                    check(tanh_core(xs), sg(e.div(e.add_f64(2.0))), TANH_BOUND, "tanh_core", a);
                }
                let a2 = da.mul(da);
                let w = da.add(a2.div(crate::dd::sqrt(a2.add_f64(1.0)).add_f64(1.0)));
                check(asinh_core(xs), sg(log1p_dd(w)), ASINH_BOUND, "asinh_core", a);
                if a < 1.0 {
                    let w = da.scale(2.0).div(one.sub(da));
                    check(atanh_core(xs), sg(log1p_dd(w).scale(0.5)), ATANH_BOUND, "atanh_core", a);
                }
            }
            if a > 1.0 {
                let t = da.add_f64(-1.0);
                let w = t.add(crate::dd::sqrt(t.mul(t.add_f64(2.0))));
                check(acosh_core(x), log1p_dd(w), ACOSH_BOUND, "acosh_core", a);
            }
            seen += 1;
        }
        assert!(seen > 1_200_000, "{seen}");
    }

    /// sinhf on every `float` operand in every mode against the accurate path (one job per
    /// function, 2^34 calls; LIBM-F32-HYPERBOLIC's accuracy check). (No spec clause.)
    #[test]
    #[ignore]
    fn sinhf_fast_path_agrees_exhaustively() {
        let taken = exhaustive(&[Fast::Sinh]);
        std::println!("sinhf fast path taken {taken} of {} calls", 4u64 << 32);
    }

    /// coshf on every `float` operand in every mode (as for sinhf). (No spec clause.)
    #[test]
    #[ignore]
    fn coshf_fast_path_agrees_exhaustively() {
        let taken = exhaustive(&[Fast::Cosh]);
        std::println!("coshf fast path taken {taken} of {} calls", 4u64 << 32);
    }

    /// tanhf on every `float` operand in every mode (as for sinhf). (No spec clause.)
    #[test]
    #[ignore]
    fn tanhf_fast_path_agrees_exhaustively() {
        let taken = exhaustive(&[Fast::Tanh]);
        std::println!("tanhf fast path taken {taken} of {} calls", 4u64 << 32);
    }

    /// asinhf on every `float` operand in every mode (as for sinhf). (No spec clause.)
    #[test]
    #[ignore]
    fn asinhf_fast_path_agrees_exhaustively() {
        let taken = exhaustive(&[Fast::Asinh]);
        std::println!("asinhf fast path taken {taken} of {} calls", 4u64 << 32);
    }

    /// acoshf on every `float` operand in every mode (as for sinhf). (No spec clause.)
    #[test]
    #[ignore]
    fn acoshf_fast_path_agrees_exhaustively() {
        let taken = exhaustive(&[Fast::Acosh]);
        std::println!("acoshf fast path taken {taken} of {} calls", 4u64 << 32);
    }

    /// atanhf on every `float` operand in every mode (as for sinhf). (No spec clause.)
    #[test]
    #[ignore]
    fn atanhf_fast_path_agrees_exhaustively() {
        let taken = exhaustive(&[Fast::Atanh]);
        std::println!("atanhf fast path taken {taken} of {} calls", 4u64 << 32);
    }

    /// sinf (and sincosf) on every `float` operand in every mode against the accurate path, one
    /// job per function (LIBM-F32-SINF-SHRINK's accuracy check, 2^34 calls). (No spec clause.)
    #[test]
    #[ignore]
    fn sinf_fast_path_agrees_exhaustively() {
        let taken = exhaustive(&[Fast::Sin]);
        std::println!("sinf fast path taken {taken} of {} calls", 4u64 << 32);
    }

    /// cosf on every `float` operand in every mode (as for sinf). (No spec clause.)
    #[test]
    #[ignore]
    fn cosf_fast_path_agrees_exhaustively() {
        let taken = exhaustive(&[Fast::Cos]);
        std::println!("cosf fast path taken {taken} of {} calls", 4u64 << 32);
    }

    /// tanf on every `float` operand in every mode (as for sinf). (No spec clause.)
    #[test]
    #[ignore]
    fn tanf_fast_path_agrees_exhaustively() {
        let taken = exhaustive(&[Fast::Tan]);
        std::println!("tanf fast path taken {taken} of {} calls", 4u64 << 32);
    }

    /// expm1_core's y is within its proved bound (|y/f - 1| ≤ 9.1·2^-53) of f = e^x - 1, f from the
    /// double-double `exp_dd` minus 1. Below |x| = 2^-20 exp_dd's absolute error (about 2^-104)
    /// is no longer far below 2^-53·|f|, so the grid starts there. (Clause M6-3: no spec clause.)
    #[test]
    fn expm1_core_is_within_its_bound() {
        let grid = (0..1u32 << 16).map(|i| -17.9 + 106.8 * (i as f32) / 65_536.0);
        let near0 = (0x3580_0000u32..0x3c00_0000).step_by(97).flat_map(|b| [b, b | 1 << 31]).map(f32::from_bits);
        let mut seen = 0u32;
        for x in grid.chain(near0) {
            if x.abs() < 2f32.powi(-20) {
                continue;
            }
            if let Some(y) = expm1_core(x) {
                seen += 1;
                let (v, m) = acc::exp_dd(crate::dd::Dd::from(f64::from(x)));
                let f = v.scale(crate::dd::pow2(m)).add_f64(-1.0);
                let err = crate::dd::Dd::from(y).sub(f).to_f64();
                assert!(err.abs() <= 9.1 * f.to_f64().abs() / 9_007_199_254_740_992.0, "expm1_core({x:e}): {} δ", err / f.to_f64() * 9_007_199_254_740_992.0);
            }
        }
        assert!(seen > 100_000, "{seen}");
    }

    /// powf's fast path against the accurate path on pairs, every mode; the number taken.
    fn agree_pow(xs: &[f32], ys: &[f32]) -> u64 {
        let mut taken = 0;
        for m in MODES {
            for &x in xs {
                for &y in ys {
                    if let Some(r) = eval_pow(x, y, m) {
                        taken += 1;
                        let want = crate::pow::pow(x, y, m);
                        assert_eq!((m, x.to_bits(), y.to_bits(), r.0.to_bits(), r.1), (m, x.to_bits(), y.to_bits(), want.0.to_bits(), want.1));
                    }
                }
            }
        }
        taken
    }

    /// C Annex F F.10.4.4 (no spec clause beyond s10_0_f_0's Annex F reference): every special case
    /// of `powf` through the fast entry, in every mode; none of them may take the fast path.
    #[test]
    fn powf_annex_f_special_cases() {
        let (inf, nan) = (f32::INFINITY, f32::NAN);
        let ok = Status::OK;
        let cases: [(f32, f32, f32, Status); 30] = [
            (0.0, -3.0, inf, Status::POLE), (-0.0, -3.0, -inf, Status::POLE), (0.0, -2.5, inf, Status::POLE),
            (-0.0, -2.0, inf, Status::POLE), (0.0, -inf, inf, ok), (-0.0, -inf, inf, ok),
            (0.0, 3.0, 0.0, ok), (-0.0, 3.0, -0.0, ok), (-0.0, 2.5, 0.0, ok), (-0.0, 4.0, 0.0, ok),
            (-1.0, inf, 1.0, ok), (-1.0, -inf, 1.0, ok), (1.0, nan, 1.0, ok), (1.0, -7.5, 1.0, ok),
            (nan, 0.0, 1.0, ok), (-3.0, -0.0, 1.0, ok), (-2.0, 0.5, nan, Status::DOMAIN),
            (0.5, -inf, inf, ok), (-0.5, -inf, inf, ok), (2.0, -inf, 0.0, ok), (0.5, inf, 0.0, ok),
            (-2.0, inf, inf, ok), (-inf, -3.0, -0.0, ok), (-inf, -2.5, 0.0, ok), (-inf, 3.0, -inf, ok),
            (-inf, 2.5, inf, ok), (inf, -0.5, 0.0, ok), (inf, 0.5, inf, ok), (nan, 2.0, nan, ok), (2.0, nan, nan, ok),
        ];
        for m in MODES {
            for (x, y, v, st) in cases {
                assert!(eval_pow(x, y, m).is_none(), "{x} {y}");
                let (got, gst) = pow(x, y, m);
                let same = if v.is_nan() { got.is_nan() } else { got.to_bits() == v.to_bits() };
                assert!(same && (gst.flags, gst.error) == (st.flags, st.error), "pow({x}, {y}) = {got} {gst:?}");
            }
        }
    }

    #[test]
    fn powf_fast_path_agrees_with_the_accurate_path() {
        // Grids of x and y over the working ranges (negative x with integer y), the subnormal
        // operands and results, the exact powers (which the test must refuse), and the rate.
        let xs: std::vec::Vec<f32> = (0..1500u32).map(|i| f32::from_bits(0x0000_1000 + i * 0x0005_6a3b)).chain((0..300).map(|i| -0.01 - 0.37 * i as f32)).collect();
        let ys: std::vec::Vec<f32> = (0..400).map(|i| -30.0 + 0.1503 * i as f32).chain((-12..13).map(|i| i as f32)).chain([1e-3, -7e-5, 1e-30, 2e-40, 130.7, -77.1, 1e4, 3e5]).collect();
        let n = agree_pow(&xs, &ys);
        assert!(n > 150_000, "{n}");
        let near1: std::vec::Vec<f32> = (0x3f7f_f000u32..0x3f80_1000).step_by(3).map(f32::from_bits).collect();
        assert!(agree_pow(&near1, &[1e3, -2.5e4, 3e6, 0.37]) > 20_000);
        let exact: std::vec::Vec<f32> = (1..40).map(|i| i as f32).chain([0.25, 0.125, 1.5, -2.0, -3.0]).collect();
        agree_pow(&exact, &[2.0, 3.0, 0.5, -1.0, -2.0, 1.5, 0.25, 4.0, 10.0, 24.0]);
        // Subnormal results: x^y near 2^-126 .. 2^-149.
        let sub: std::vec::Vec<f32> = (0..2000).map(|i| -86.0 - 0.009 * i as f32).collect();
        assert!(agree_pow(&[core::f32::consts::E, 0.5, 2.0, 7.25], &sub) > 10_000);
        let before = stats::counts("powf");
        // The rate on pairs with |y·ln x| ≤ 70 (x in [0.01, 100], |y| ≤ 15); next to the overflow
        // and underflow thresholds the results leave the fast range by design.
        for x in (0..2000).map(|i| 0.01 + 0.049_99 * i as f32) {
            for y in (0..500).map(|i| -15.0 + 0.0601 * i as f32) {
                let _ = eval_pow(x, y, Rounding::Nearest);
            }
        }
        let c = stats::counts("powf").since(before);
        assert!(c.check_fallback("powf", 10_000).is_ok(), "powf: fallback rate {c:?}");
    }

    /// pow_core's result is within its proved bound (< 89.01 ulp) of |x|^y, measured against the
    /// double-double e^(y·ln x) (error far below 2^-100); the agreement tests cannot see a loss
    /// of a few ulp of the `double`.
    #[test]
    fn pow_core_is_within_its_bound() {
        let mut seen = 0u32;
        let xs = (0x0000_0100u32..0x7f80_0000).step_by(100_003).map(f32::from_bits);
        for x in xs {
            for j in 0..64 {
                let y = (-40.0 + 1.27 * j as f32) * if j % 3 == 0 { 1.0 } else { 0.3 };
                let Some(v) = pow_core(f64::from(x), f64::from(y)) else { continue };
                seen += 1;
                let l = crate::exp_log::log_dd(crate::dd::Dd::from(f64::from(x))).mul_f64(f64::from(y));
                let (f, sc) = crate::exp_log::exp_dd(l);
                let err = crate::dd::Dd::from(v).sub(f.scale(f64::from_bits(((sc + 1023) as u64) << 52))).to_f64(); // e^l = f·2^sc
                let ulp = f64::from_bits(v.to_bits() & 0x7ff0_0000_0000_0000) / 4_503_599_627_370_496.0;
                // The proof's bound as a function of t: (0.84|t| + 2.06)·2^-53, below 89 overall.
                assert!(err.abs() <= (0.84 * l.hi.abs() + 2.1) * ulp, "pow_core({x:e}, {y:e}): {} ulp", err / ulp);
            }
        }
        assert!(seen > 100_000, "{seen}");
    }

    /// cbrt_core's y is within its proved bound (|y/∛x - 1| ≤ 5.4·2^-53), measured as
    /// |y³/x - 1| ≤ 3·5.4·2^-53 (plus 2^-60 for the square of the error) in double-double. (Clause M6-3: no spec clause.)
    #[test]
    fn cbrt_core_is_within_its_bound() {
        let wide = (1u32..0x7f80_0000).step_by(997).flat_map(|b| [b, b | 1 << 31]).map(f32::from_bits);
        let near1 = (0x3f70_0000u32..0x3f90_0000).step_by(7).map(f32::from_bits);
        let mut seen = 0u32;
        for x in wide.chain(near1) {
            if let Some(y) = cbrt_core(x) {
                seen += 1;
                let x = f64::from(x);
                let c = crate::dd::Dd::from(y).sqr().mul_f64(y).add_f64(-x).to_f64() / x;
                assert!(c.abs() <= 16.2 / 9_007_199_254_740_992.0 + 2f64.powi(-60), "cbrt_core({x:e}): {} δ", c * 9_007_199_254_740_992.0 / 3.0);
            }
        }
        assert!(seen > 1_000_000, "{seen}");
    }

    /// powf: every `float` x for a set of y, and every `float` y for a set of x, in every mode
    /// (MATH-M6-F32-POWF's proof; its own job, 18 slices of 2^34 calls).
    #[test]
    #[ignore]
    fn powf_fast_path_agrees_exhaustively() {
        let n = std::thread::available_parallelism().map_or(8, |n| n.get()) as u64;
        let per = (1u64 << 32) / n + 1;
        let fixed = [2.0f32, 3.0, -2.0, 0.5, 1.5, -1.5, 1.0 / 3.0, 2.5, 7.1, -0.7, 1e-3, 33.3];
        let other = [2.0f32, 0.9, 1.0001, 10.0, -3.0, 1.5];
        let slices = fixed.iter().map(|&y| (true, y)).chain(other.iter().map(|&x| (false, x)));
        let mut taken = 0u64;
        for (all_x, c) in slices {
            taken += std::thread::scope(|sc| {
                let hs: std::vec::Vec<_> = (0..n).map(|t| sc.spawn(move || {
                    let lo = t * per;
                    let hi = ((t + 1) * per).min(1 << 32);
                    let v: std::vec::Vec<f32> = (lo..hi).map(|b| f32::from_bits(b as u32)).collect();
                    if all_x { agree_pow(&v, &[c]) } else { agree_pow(&[c], &v) }
                })).collect();
                hs.into_iter().map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e))).sum::<u64>()
            });
            std::println!("slice all_x={all_x} c={c:e}: fast path taken {taken} so far");
        }
    }

    /// Every `float` operand in every mode (M6-3's proof; run as its own job, about 2^35 calls).
    /// `FAST32_SWEEP=expm1f,cbrtf` limits it to the named functions (default: all of ALL).
    #[test]
    #[ignore]
    fn fast_path_agrees_exhaustively() {
        let want = std::env::var("FAST32_SWEEP").unwrap_or_default();
        let fs: std::vec::Vec<Fast> = ALL.into_iter().zip(NAMES)
            .filter(|(_, n)| want.is_empty() || want.split(',').any(|w| w == *n)).map(|(f, _)| f).collect();
        assert!(!fs.is_empty(), "FAST32_SWEEP={want} names no function");
        std::println!("sweeping {fs:?}");
        let fs = &fs[..];
        let n = std::thread::available_parallelism().map_or(8, |n| n.get()) as u64;
        let per = (1u64 << 32) / n + 1;
        let taken: u64 = std::thread::scope(|sc| {
            let hs: std::vec::Vec<_> = (0..n).map(|t| sc.spawn(move || {
                let lo = t * per;
                let hi = ((t + 1) * per).min(1 << 32);
                agree_in(fs, (lo..hi).map(|b| f32::from_bits(b as u32)))
            })).collect();
            hs.into_iter().map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e))).sum()
        });
        std::println!("fast path taken {taken} of {} calls", (fs.len() as u64 * 4) << 32);
    }
}
