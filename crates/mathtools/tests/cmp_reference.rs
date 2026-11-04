//! The ulp-error reference of the mathcmp measurement (D-10.3) on the cosh q mode U records of tiny
//! operands: `cmp::ulp_reference` is the 2p+40-bit value, never below the round-toward-zero result 1,
//! and the decided result nextup(1) is within 1 ulp of it.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]
#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::indexing_slicing)]

use liella_mathtools::cmp::{oracle, ulp_reference};
use liella_mathtools::mp::*;

/// Operands of the seed-1 cosh q sample whose results round to 1 (x near 2^-9900), plus 2^-10000.
const TINY: [u128; 7] = [
    0x1947_ef5c_1f73_6468_338e_17db_094f_f35a,
    0x01d5_24f7_1e21_e9e7_54a9_d8a9_74e2_14e9,
    0x32bd_21bd_9b7a_928e_e3e3_ccf2_a304_ba00,
    0x058a_7028_b05c_c9f5_a598_171a_69d3_284b,
    0x2dd2_93a7_e52f_d587_0e3a_71a8_bd29_7113,
    0x0768_e456_c1ff_cffe_dddf_d69f_0769_17fc,
    6383_u128 << 112,
];

#[test]
fn cosh_tiny_reference_is_not_below_one_and_nextup_one_is_within_one_ulp() {
    // no spec clause: test tooling of the math oracle (D-10.3 measurement, cosh q mode U row).
    let fmt = Fmt::F128;
    let one_bits: u128 = 0x3FFF_u128 << 112;
    let nextup_one = one_bits + 1;
    for bits in TINY {
        let args = [Big::from_bits(fmt, bits)];
        let raw = oracle("cosh", &args, 2 * fmt.prec + 40).unwrap().value;
        assert!(raw.cmp_abs(&Big::one()).is_ge(), "{bits:#x}: 2p+40-bit cosh is not below 1");
        let r = ulp_reference("cosh", fmt, Round::Up, &args, nextup_one);
        assert!(r.cmp_abs(&raw).is_eq(), "{bits:#x}: the reference is the 2p+40-bit value");
        let e = ulp_error(fmt, nextup_one, &r);
        assert!(e <= 1.0, "{bits:#x}: nextup(1) is within 1 ulp of the reference, got {e}");
    }
}
