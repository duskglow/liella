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
//!
//! Layout: this file is the shared core ([`Fast`] and its dispatch, the shared constants and
//! tables, the rounding test [`round`]); each family's kernels are in `fast32/`: `exp_log.rs`
//! (exp, log, log1p, expm1, cbrt, pow), `trig.rs` (sin, cos, tan, sincos), `inv_trig.rs` (asin,
//! acos, atan, atan2), `hyperbolic.rs` and `erf.rs` (erf, erfc, with their own tests); the
//! shared tests are in `fast32/tests.rs`.
#![allow(clippy::as_conversions, clippy::arithmetic_side_effects, clippy::indexing_slicing)] // bit fields and exponents of bounded width; table indices masked to 0..128; `cvtsd2ss` is the `as f32`

use crate::coeffs::tables as t;
use crate::stats;
use crate::status::{Flags, Rounding, Status};

mod bessel;
mod cexp;
mod clog_csqrt;
mod erf;
mod exp_log;
mod gamma;
mod hyperbolic;
mod inv_trig;
mod trig;

use bessel::{bessel_core, bessel_finish};
#[cfg(test)]
pub(crate) use cexp::{cexp_core, CEXP_DELTA};
pub use cexp::{cexpf, cexpf_nearest};
pub use clog_csqrt::{clogf, clogf_nearest, csqrtf, csqrtf_nearest};
use erf::{erf_core, erfc_core, ERFC_DELTA, ERF_DELTA};
use exp_log::*;
pub use gamma::{eval_lgamma, eval_tgamma};
use gamma::{gamma_core, lgamma_core, TGAMMA_DELTA};
use hyperbolic::*;
use inv_trig::*;
use trig::*;
pub use exp_log::{eval_pow, pow};
pub use hyperbolic::{acosh_nearest_sqrt, asinh_nearest_sqrt};
pub use inv_trig::{atan2, eval_atan2};
pub use trig::{cos_nearest_widen, sin_nearest_widen, sincos, sincos_nearest_widen, tan_nearest_widen};

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
    /// `erff` (LIBM-F32-ERF).
    Erf,
    /// `erfcf` (LIBM-F32-ERF).
    Erfc,
    /// `tgammaf` (LIBM-F32-GAMMA).
    Tgamma,
    /// `lgammaf` (LIBM-F32-GAMMA; the sign by [`eval_lgamma`]).
    Lgamma,
    /// `j0f` (LIBM-F32-BESSEL).
    J0,
    /// `j1f` (LIBM-F32-BESSEL).
    J1,
    /// `y0f` (LIBM-F32-BESSEL).
    Y0,
    /// `y1f` (LIBM-F32-BESSEL).
    Y1,
}

/// The rounding test's margins, in units of the last place of the `double` approximation y:
/// a bound ε on the relative error gives |y - f| < ε·2^53/(1 - ε) ulp(y) (y < 2^(E+1) in
/// binade E). The proofs are at [`exp_core`] and [`log_core`].
/// The fast path's error bounds are far looser than a `float` needs (ε ≤ 2^-45 rather than
/// 2^-52, MATH-M6-F32-KERNEL): a shorter reduction and polynomial, at the price of about
/// 2·DELTA/2^28 < 4·10^-6 of the operands going to the accurate path.
/// exp: ε ≤ 2^-45.16, so < 229 ulp.
const EXP_DELTA: u64 = 512;
/// The one-pass exp and exp2 in the caller's rounding direction (LIBM-F32-ONEPASS, D-10.63):
/// ε ≤ 2^-43.71 in every direction, so < 627 ulp (proof at [`exp_onepass`]).
const EXP_ONEPASS_DELTA: u64 = 1024;
/// log: ε ≤ 2^-44.79 (case A dominates), so < 296 ulp; log2, log10 add the rounding of the
/// constant and of the product: ε ≤ 2^-44.77, < 300 ulp.
const LOG_DELTA: u64 = 640;
/// log1p: ε ≤ 6.8·2^-53 (proof at [`log1p_core`]), so < 6.9 ulp.
const LOG1P_DELTA: u64 = 8;
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
        Fast::Erf => (erf_core(x)?, ERF_DELTA, "erff"),
        Fast::Erfc => (erfc_core(x)?, ERFC_DELTA, "erfcf"),
        Fast::Tgamma => (gamma_core(f64::from(x))?, TGAMMA_DELTA, "tgammaf"),
        Fast::Lgamma => {
            let (y, d) = lgamma_core(x)?;
            (y, d, "lgammaf")
        }
        Fast::J0 => return bessel_finish(x, bessel_core(0, x)?, mode, "j0f"),
        Fast::J1 => return bessel_finish(x, bessel_core(1, x)?, mode, "j1f"),
        Fast::Y0 => return bessel_finish(x, bessel_core(2, x)?, mode, "y0f"),
        Fast::Y1 => return bessel_finish(x, bessel_core(3, x)?, mode, "y1f"),
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

include!("fast32_tables.rs");

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
pub(crate) fn round(y: f64, delta: u64, mode: Rounding) -> Option<f32> {
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
        // The hyperbolic family (LIBM-F32-HYPERBOLIC-3): the cores of [`eval`], unchanged. Where
        // they answer, |x| ≥ 2^-26 (acosh: x > 1), so x is a normal `float` and its conversion
        // raises nothing; a subnormal x raises denormal-operand in that conversion and is then
        // refused (the entry's cold path drops the bit, FAST_ATTEMPT_HW). Every intermediate is a
        // normal `double` (each core's proof: the operands of expm1_core, exp_core, sqrt64 and
        // log1p64 lie inside their stated ranges) and each divisor is a normal `double` ≥ 2^-24
        // (E + 1, E, E + 2, 1 + √, 1 - a): no division by zero, no overflow, no underflow. So
        // the arithmetic raises inexact and nothing else, and the rounding test makes the
        // result inexact.
        Fast::Sinh => (sinh_core(x)?, SINH_DELTA),
        Fast::Cosh => (cosh_core(x)?, COSH_DELTA),
        Fast::Tanh => (tanh_core(x)?, TANH_DELTA),
        Fast::Asinh => (asinh_core(x)?, ASINH_DELTA),
        Fast::Acosh => (acosh_core(x)?, ACOSH_DELTA),
        Fast::Atanh => (atanh_core(x)?, ATANH_DELTA),
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
        Fast::Log1p | Fast::Expm1 | Fast::Cbrt | Fast::Asin | Fast::Acos | Fast::Atan | Fast::Erf | Fast::Erfc | Fast::Tgamma | Fast::Lgamma | Fast::J0
        | Fast::J1 | Fast::Y0 | Fast::Y1 => {
            return None;
        }
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
nearest_fn!(sinh_nearest, Fast::Sinh);
nearest_fn!(cosh_nearest, Fast::Cosh);
nearest_fn!(tanh_nearest, Fast::Tanh);
nearest_fn!(asinh_nearest, Fast::Asinh);
nearest_fn!(acosh_nearest, Fast::Acosh);
nearest_fn!(atanh_nearest, Fast::Atanh);

/// `expf` in the caller's rounding direction and MXCSR, which it does not read, by one `double`
/// evaluation and one conversion (LIBM-F32-ONEPASS, D-10.63), or `None` for the envelope. Where
/// it answers it raises exactly inexact (proof at [`exp_onepass`]).
#[inline]
pub fn exp_onepass_any(x: f32) -> Option<f32> {
    exp_onepass(x, INV_L, 0x42ae_a8f6) // |x| < 87.33
}

/// `exp2f` as [`exp_onepass_any`].
#[inline]
pub fn exp2_onepass_any(x: f32) -> Option<f32> {
    exp_onepass(x, 128.0, 0x42fc_0000) // |x| < 126
}

/// `exp10f` as [`exp_onepass_any`] (D-10.72): `c` = RN(128·log2 10), |x| < 37.9 (|x·log2 10| <
/// 125.9, so [`exp_onepass`]'s range and proof hold unchanged: |z| < 2^13.98). 10^n is a `float`
/// for n = 1..=10 while z = x·c is inexact there, so the operands in [1, 10] with their low 20
/// bits clear (the integers among them; at most 4 significant bits) are refused on the bits first,
/// by one compare: d = bits(x) - bits(1) rotated right by 20 is ≤ 0x1a exactly when d's low 20
/// bits are clear (any set bit lands at 2^12 or above) and d ≤ bits(10) - bits(1) = 0x1a·2^20 (x
/// below 1 wraps d above). Two separate tests compiled branchless, 5 instructions more (D-10.72).
#[inline]
pub fn exp10_onepass_any(x: f32) -> Option<f32> {
    if x.to_bits().wrapping_sub(0x3f80_0000).rotate_right(20) <= (0x4120_0000 - 0x3f80_0000) >> 20 {
        return None;
    }
    exp_onepass(x, INV_M, 0x4217_999a)
}

/// `logf` in the caller's rounding direction and MXCSR, unread (D-10.72; proof at [`log_onepass`]).
#[inline]
pub fn log_onepass_any(x: f32) -> Option<f32> {
    log_onepass::<0>(x)
}

/// `log2f` as [`log_onepass_any`].
#[inline]
pub fn log2_onepass_any(x: f32) -> Option<f32> {
    log_onepass::<1>(x)
}

/// `log10f` as [`log_onepass_any`].
#[inline]
pub fn log10_onepass_any(x: f32) -> Option<f32> {
    log_onepass::<2>(x)
}

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
fast_fn!(erf, Fast::Erf, special);
fast_fn!(erfc, Fast::Erfc, special);

#[cfg(test)]
mod tests;
