//! The ulp-error reference of the mathcmp measurement (D-10.3): `cmp::ulp_reference` never lies below
//! the round-toward-zero result in magnitude.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]
#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::indexing_slicing)]

use liella_mathtools::cmp::{oracle, ulp_reference};
use liella_mathtools::mp::*;

#[test]
fn cosh_tiny_reference_is_clamped_to_round_toward_zero() {
    // no spec clause: test tooling of the math oracle (D-10.3 measurement, cosh q mode U row).
    let fmt = Fmt::F128;
    let one_bits: u128 = 0x3FFF_u128 << 112;
    let x = Big::from_bits(fmt, 6383_u128 << 112); // 2^-10000
    let args = [x];
    // Precondition: the 2p+40-bit value is below 1, the case the clamp exists for.
    let raw = oracle("cosh", &args, 2 * fmt.prec + 40).unwrap().value;
    assert!(raw.cmp_abs(&Big::one()).is_lt(), "precondition: cosh(2^-10000) at 2p+40 bits is below 1");
    let cr = correctly_rounded(fmt, Round::Up, 4096, |p| oracle("cosh", &args, p).unwrap()).map(|r| r.0);
    let nextup_one = one_bits + 1;
    if let Ok(cr) = cr {
        assert_eq!(cr, nextup_one, "cosh(2^-10000) rounded up is nextup(1)");
    }
    let r = ulp_reference("cosh", fmt, Round::Up, &args, nextup_one);
    assert!(r.cmp_abs(&Big::one()).is_ge(), "reference raised to the round-toward-zero result 1");
    let e = ulp_error(fmt, nextup_one, &r);
    assert!(e <= 1.0, "nextup(1) is within 1 ulp of the reference, got {e}");
    assert_eq!(ulp_error(fmt, one_bits, &r), 0.0, "1 is the reference itself");
}
