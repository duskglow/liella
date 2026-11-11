#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, missing_docs)]
//! The math kernels raise no invalid flag in hardware for operands that are not invalid (D-10.95,
//! LIBM-INVALID-CONVERT-SCAN). A float-to-integer conversion of a value outside the signed 64-bit
//! range raises invalid in hardware; the envelope then clears it with an MXCSR write (no caller sees
//! it, every call pays it), so the kernels are called here directly, between `feclearexcept` and
//! `fetestexcept`. Covered: the `long double` arc kernels (`atan`, `atan2`, `asin`, `acos`), whose
//! root seed and its reciprocal lie in [2^63, 2^64] on every call and whose quotient seed reaches
//! 2^63 for a power-of-two divisor.

use liella_boundary::posix::s10_math::{feclearexcept, fetestexcept};
use liella_math::float::F80;
use liella_math::status::Rounding;

const FE_ALL: i32 = 0x3d;
const FE_INVALID: i32 = 0x01;

/// A finite nonzero `double` as the x87 encoding of the same value.
fn f80(x: f64) -> F80 {
    let b = x.to_bits();
    let sign = u128::from(b >> 63) << 79;
    let e = u128::from((b >> 52) & 0x7ff) + 16383 - 1023;
    let m = (1u128 << 63) | (u128::from(b & ((1 << 52) - 1)) << 11);
    F80(sign | (e << 64) | m)
}

fn no_invalid<T>(name: &str, arg: &str, f: impl FnOnce() -> T) {
    feclearexcept(FE_ALL);
    std::hint::black_box(f());
    let raised = fetestexcept(FE_INVALID);
    assert_eq!(raised, 0, "{name}({arg}): invalid raised in hardware for a valid operand");
}

// s10_3_c_0 (D-10.13, D-10.95): asinl, acosl, atanl and atan2l's kernels on operands across
// their ranges (powers of two included: the quotient seed's 2^63 edge) raise no invalid.
#[test]
fn long_double_arc_kernels_raise_no_invalid_in_hardware() {
    let mut xs: Vec<f64> = (1..512).map(|k| f64::from(k) / 512.0).collect();
    xs.extend((-20..20).map(|e| 2f64.powi(e)));
    xs.extend([std::f64::consts::FRAC_1_SQRT_2, 0.999_999_999, 0.5000000000000001, 1.0e-5, 3.0, 1.0e10]);
    let ys: Vec<f64> = xs.iter().flat_map(|&x| [x, -x]).collect();
    for &x in &ys {
        let a = f80(x);
        if x.abs() <= 1.0 {
            no_invalid("asinl", &format!("{x:e}"), || liella_math::wide::asin::<F80>(a, Rounding::Nearest));
            no_invalid("acosl", &format!("{x:e}"), || liella_math::wide::acos::<F80>(a, Rounding::Nearest));
        }
        no_invalid("atanl", &format!("{x:e}"), || liella_math::wide::atan::<F80>(a, Rounding::Nearest));
    }
    for &y in ys.iter().step_by(5) {
        for &x in ys.iter().step_by(7) {
            no_invalid("atan2l", &format!("{y:e}, {x:e}"), || liella_math::wide::atan2::<F80>(f80(y), f80(x), Rounding::Nearest));
        }
    }
}
