//! `float` fast paths for `exp`, `exp2`, `exp10`, `log`, `log2`, `log10` (Track M milestone 6,
//! M6-3 `MATH-M6-F32-CORE`), `log1p` (`MATH-M6-F32-REST`) and `sin`, `cos`, `tan`, `sincos`
//! (`MATH-M6-F32-TRIG`, the accurate path being [`crate::trig`]; it reuses that module's `sin(kπ/64)` table). Each evaluates `f(x)` in `double` with a table and a polynomial to a
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
/// sin, cos: ε ≤ 9.05·2^-53 (proof at [`trig_core`]), so < 9.1 ulp.
const TRIG_DELTA: u64 = 10;
/// tan = s/c: both quotients' bounds and the division, ε ≤ 19.2·2^-53, so < 19.3 ulp.
const TAN_DELTA: u64 = 20;

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
const S3: f64 = -1.0 / 6.0;
const S5: f64 = 1.0 / 120.0;
const S7: f64 = -1.0 / 5040.0;
const K4: f64 = 1.0 / 24.0;
const K6: f64 = -1.0 / 720.0;
const K8: f64 = 1.0 / 40320.0;

/// (sin x, cos x) as `double`s, each with |y/f - 1| ≤ 9.05·2^-53, for 2^-26 ≤ |x| < 2^19.
///
/// Reduction (Cody–Waite): x = n·π/64 + r, n = nearest(RN(x·64/π)), so |r| < 0.024544.
/// n·PI64_1 and n·PI64_2 are exact (27 and 29 bits, |n| < 2^24); x - n·PI64_1 is exact (both on
/// the 2^-31 grid, the difference below 2^-5); then two roundings ≤ 2^-53 of their results, the
/// product n·PI64_3 ≤ 2^-94 and the constants' tail ≤ 2^-95.8: |Δr| ≤ 2·2^-53|r| + 2^-93. n = 0
/// gives r = x exactly. |r| < 2^-36 with n ≠ 0 goes to the accurate path.
///
/// sin r = r + r·z(S3 + z(S5 + z·S7)), z = r², Taylor remainder ≤ 2^-61.2|r|; with Δr and the final
/// sum, |Δsr| ≤ 3.01·2^-53|r|. cos r - 1 = cm = z(-1/2 + z(K4 + z(K6 + z·K8))): remainder < 2^-75,
/// |cm| ≤ 3.02·10^-4, |Δcm| ≤ 8·2^-53|cm|. S = sin(nπ/64), C = cos(nπ/64) are the `double` heads
/// of the table (relative error 2^-53; sin 0, sin π, cos ±π/2 are exactly 0 and ±1).
/// sin x = S + (S·cm + C·sr); cos x is the same with (C, -S) for (S, C) (the angle shifted by π/2).
/// * n ≢ 0 (mod 64): the angle is at least π/128 from a multiple of π, so |S| ≤ 2|y| and
///   |C·r| ≤ 1.0001|y|. The errors, in units of 2^-53|y|: S 2, C 1.0001, sr 3.0103, cm 0.005,
///   the two products 1.002, the inner sum 1.02, the outer 1: ≤ 9.05.
/// * n ≡ 0 (mod 64): S = 0, C = ±1, y = ±sr: Δr ≤ (2 + 2^-5)·2^-53|r| (|r| ≥ 2^-36), so ≤ 3.04.
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
    let sr = r + r * z * (S3 + z * (S5 + z * S7));
    let cm = z * (-0.5 + z * (K4 + z * (K6 + z * K8)));
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
        // No in-place nearest entry yet (their flags on subnormal operands are unproven here):
        // the caller takes [`eval`].
        Fast::Log1p | Fast::Sin | Fast::Cos | Fast::Tan => return None,
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
pub use crate::exp_log::expm1;
pub use crate::trig::{acos, asin, atan, atan2};

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exp_log as acc;

    const ALL: [Fast; 10] = [Fast::Exp, Fast::Exp2, Fast::Exp10, Fast::Log, Fast::Log2, Fast::Log10, Fast::Log1p, Fast::Sin, Fast::Cos, Fast::Tan];
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
        let names = ["expf", "exp2f", "exp10f", "logf", "log2f", "log10f", "log1pf", "sinf", "cosf", "tanf"];
        for (i, (f, n)) in ALL.into_iter().zip(names).enumerate() {
            let before = stats::counts(n);
            let xs: std::vec::Vec<f32> = match i {
                0..3 => exps.clone().collect(),
                3..6 => logs.clone().collect(),
                6 => log1ps.clone().collect(),
                _ => trigs.clone().collect(),
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

    /// trig_core's sin, cos (< 9.1 ulp) and tan = s/c (< 19.3 ulp) against the double-double
    /// `sin_cos` of the accurate path (its error far below 2^-100).
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
                for (y, f, b, n) in [(s, fs, 9.1, "sin"), (c, fc, 9.1, "cos"), (s / c, fs.div(fc), 19.3, "tan")] {
                    let err = crate::dd::Dd::from(y).sub(f).to_f64();
                    assert!(err.abs() <= b * ulp(y), "trig_core {n}({x:e}): {} ulp", err / ulp(y));
                }
            }
        }
        assert!(seen > 750_000, "{seen}");
    }

    /// `fs` on every `float` operand in every mode; the number taken.
    fn exhaustive(fs: &'static [Fast]) -> u64 {
        let n = std::thread::available_parallelism().map_or(8, |n| n.get()) as u64;
        let per = (1u64 << 32) / n + 1;
        std::thread::scope(|sc| {
            let hs: std::vec::Vec<_> = (0..n).map(|t| sc.spawn(move || {
                let lo = t * per;
                let hi = ((t + 1) * per).min(1 << 32);
                agree_in(fs, (lo..hi).map(|b| f32::from_bits(b as u32)))
            })).collect();
            hs.into_iter().map(|h| h.join().unwrap_or_else(|e| std::panic::resume_unwind(e))).sum()
        })
    }

    /// sinf, cosf, tanf (and sincosf) on every `float` operand in every mode (MATH-M6-F32-TRIG's
    /// proof; its own job, 3·2^34 calls).
    #[test]
    #[ignore]
    fn trig_fast_path_agrees_exhaustively() {
        let taken = exhaustive(&TRIG);
        std::println!("trig fast path taken {taken} of {} calls", 12u64 << 32);
    }

    /// Every `float` operand in every mode (M6-3's proof; run as its own job, about 2^35 calls).
    #[test]
    #[ignore]
    fn fast_path_agrees_exhaustively() {
        let taken = exhaustive(&ALL);
        std::println!("fast path taken {taken} of {} calls", 40u64 << 32);
    }
}
