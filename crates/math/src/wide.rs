//! The elementary functions of the wide formats (spec §10.3, D-10.19): x87 `long double`
//! (`l`, `f64x`), and generic over any `Float` so that `_Float128` can follow on the same code.
//! Each function is evaluated in the 384-bit arithmetic of [`crate::mp`] (relative error near
//! 2^-370 for every kernel used) and rounded once into the format in the caller's direction, so
//! every result is correctly rounded in every mode unless the exact value lies within about
//! 2^-370 of a rounding boundary. The logarithms and `pow` take `ln` from [`mp::ln_atanh`],
//! whose bound is proved (absolute `< 2^(max(e, 0) + 12 − 384)` for `|ln x| < 2^e`, D-10.24):
//! as `|ln x| ≥ 2^-114` for every `x ≠ 1` of `_Float128` (`2^-65` for x87), `log`, `log2` and
//! `log10` are within `2^-257` relative and `y·ln|x|` of `pow` within `2^-244` absolute
//! (`|y·ln|x|| < 2^14` short of overflow), far inside the `2·113 + 10` bits that correct
//! rounding is claimed on. That claim rests on an estimate, not a proof: the closest approach of
//! these functions to a rounding boundary is not known for `_Float128`. Exact results do lie on
//! a boundary; they are detected and
//! computed exactly (`exp2` and `log2` at powers of two, `exp10` and `log10` at powers of ten,
//! perfect cubes, the exact powers of `pow`), and for arguments so small or large that the
//! function differs from a representable value by less than its last place: those are rounded
//! by the side of the deviation ([`beside`]), as `elem::near` does for `float` and `double`.
//!
//! The special operands, NaNs, flags and `errno` follow the `float` and `double` forms (D-10.14).
//!
//! # The narrow first step (PERF-LIBM-LONGDOUBLE)
//!
//! For a format of at most 64 significand bits (x87 `long double`) every function first runs the
//! same code in the 192-bit arithmetic of [`crate::mpn`] (module `fast`; module `slow` is the
//! 384-bit form above). The result is rounded only when the rounding test decides it: every
//! value within `2^(ex(v) − 96)` of the 192-bit result (`ex` its binary exponent) rounds to the
//! same encoding with the same flags. The kernels' proved error bounds at `B = 192` are at most
//! `2^(77 − B) = 2^-115` relative for the arguments of x87 (the logarithms near 1; `mpn`), so the
//! test's allowance of `2^-96` covers them with 19 bits to spare; a result the test cannot decide
//! (within about `2^-96` of a rounding boundary: about one in `2^30` operands) goes to the
//! 384-bit form unchanged. Special operands, exact results and range decisions are width
//! independent. For `_Float128` only the 384-bit form runs.

#![allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation, clippy::as_conversions)] // exponents of bounded range

use crate::float::Float;
use crate::status::{Flags, Rounding, Status};

/// The 384-bit evaluation: the form of every result of a wider format and the retry step of the
/// narrow one.
mod slow {
    #![allow(dead_code)] // `sincos` is the dispatching one below
    use crate::elem;
    use crate::float::{self, Exact, Float, Kind, Num};
    use crate::fused;
    use crate::mp::{self, Mp};
    use crate::status::{Rounding, Status};
    use crate::trig::elem_round_below;

    /// Newton steps of the cube root: 53, 106, 212, 424 bits.
    const CBRT_ITERS: usize = 4;

    /// Round an approximation (see [`mp::round`]).
    fn approx<F: Float>(v: Mp, mode: Rounding) -> (F, Status) {
        mp::round::<F>(v, mode)
    }

    include!("wide_impl.rs");
}

/// The 192-bit evaluation with the rounding test; see the module documentation. A result the
/// test cannot decide carries [`UNDECIDED`] in its status.
mod fast {
    #![allow(dead_code, unreachable_pub)] // the helpers shared with the 384-bit form that only it uses; the public items of the included code
    use super::UNDECIDED;
    use crate::elem;
    use crate::float::{self, Exact, Float, Kind, Num};
    use crate::fused;
    use crate::mpn::{self as mp, Mp};
    use crate::status::{Rounding, Status};
    use crate::trig::elem_round_below;

    /// Newton steps of the cube root: 53, 106, 212 bits, past 192.
    const CBRT_ITERS: usize = 3;

    /// Bits below the leading one of the result that the rounding test leaves for the error of
    /// the evaluation: the allowance is `2^(ex(v) − (BITS − GUARD))`.
    const GUARD: i64 = 96;

    /// Round `v` when the rounding test decides it, else the marker of an undecided result.
    pub(super) fn approx<F: Float>(v: Mp, mode: Rounding) -> (F, Status) {
        if !v.is_zero() {
            let allow = Mp::from_i64(1).ldexp(v.exp - (mp::WIDTH - GUARD));
            let lo = mp::round::<F>(v.sub(allow), mode);
            let hi = mp::round::<F>(v.add(allow), mode);
            if lo.0.to_bits() == hi.0.to_bits() && lo.1 == hi.1 {
                return lo;
            }
        }
        (float::zero(false), UNDECIDED)
    }

    include!("wide_impl.rs");
}

/// The marker of a result the narrow evaluation cannot decide: `hw` carries a bit outside the
/// six MXCSR flags, which survives the union of statuses.
const UNDECIDED: Status = Status { flags: Flags::NONE, error: crate::status::MathError::None, hw: Flags(0x40) };

/// Whether a format takes the narrow step: at most 64 significand bits (x87 `long double`).
fn narrow<F: Float>() -> bool {
    F::PREC <= 64
}

/// Whether `st` marks an undecided result.
fn undecided(st: &Status) -> bool {
    st.hw.0 & 0x40 != 0
}

pub use slow::*;

macro_rules! dispatch1 {
    ($($name:ident),*) => {$(
        #[doc = concat!("`", stringify!($name), "(x)`: the narrow evaluation where it decides, else the 384-bit one.")]
        pub fn $name<F: Float>(x: F, mode: Rounding) -> (F, Status) {
            if narrow::<F>() {
                let r = fast::$name(x, mode);
                if !undecided(&r.1) {
                    return r;
                }
            }
            slow::$name(x, mode)
        }
    )*};
}

dispatch1!(
    exp, exp2, exp10, expm1, log, log2, log10, log1p, cbrt, sin, cos, tan, atan, asin, acos, sinh, cosh, tanh, asinh,
    acosh, atanh
);

macro_rules! dispatch2 {
    ($($name:ident),*) => {$(
        #[doc = concat!("`", stringify!($name), "(x, y)`: the narrow evaluation where it decides, else the 384-bit one.")]
        pub fn $name<F: Float>(x: F, y: F, mode: Rounding) -> (F, Status) {
            if narrow::<F>() {
                let r = fast::$name(x, y, mode);
                if !undecided(&r.1) {
                    return r;
                }
            }
            slow::$name(x, y, mode)
        }
    )*};
}

dispatch2!(pow, atan2);

/// `sincos(x)`: `(sin x, cos x)` with the flags of both.
pub fn sincos<F: Float>(x: F, mode: Rounding) -> ((F, F), Status) {
    let (s, st) = sin(x, mode);
    let (c, ct) = cos(x, mode);
    ((s, c), st.and(ct))
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::float;
    use crate::F80;

    fn f80(v: f64) -> F80 {
        float::convert::<f64, F80>(v, Rounding::Nearest).0
    }

    #[test]
    fn exact_cases_are_exact() {
        let n = Rounding::Nearest;
        assert_eq!(exp2(f80(-3.0), Rounding::Up), (f80(0.125), Status::OK));
        assert_eq!(exp10(f80(22.0), Rounding::Down), (f80(1e22), Status::OK));
        assert_eq!(log2(f80(1024.0), n), (f80(10.0), Status::OK));
        assert_eq!(log10(f80(1e20), Rounding::Zero), (f80(20.0), Status::OK));
        assert_eq!(pow(f80(9.0), f80(1.5), Rounding::Down), (f80(27.0), Status::OK));
        assert_eq!(pow(f80(-3.0), f80(3.0), Rounding::Up), (f80(-27.0), Status::OK));
        assert_eq!(pow(f80(16.0), f80(0.75), Rounding::Up), (f80(8.0), Status::OK));
        assert_eq!(cbrt(f80(-0.125), Rounding::Up), (f80(-0.5), Status::OK));
        assert_eq!(log(f80(1.0), Rounding::Down).0, f80(0.0));
        assert_eq!(acosh(f80(1.0), Rounding::Down).0, f80(0.0));
    }

    #[test]
    fn close_to_the_host() {
        let near = |a: F80, b: f64| {
            let (v, _) = float::convert::<F80, f64>(a, Rounding::Nearest);
            (v - b).abs() <= 4.0 * f64::EPSILON * b.abs()
        };
        for x in [0.3f64, -1.7, 2.5, 10.0, -0.01] {
            assert!(near(exp(f80(x), Rounding::Nearest).0, x.exp()), "exp {x}");
            assert!(near(sin(f80(x), Rounding::Nearest).0, x.sin()), "sin {x}");
            assert!(near(tan(f80(x), Rounding::Nearest).0, x.tan()), "tan {x}");
            assert!(near(sinh(f80(x), Rounding::Nearest).0, x.sinh()), "sinh {x}");
            assert!(near(cbrt(f80(x), Rounding::Nearest).0, x.cbrt()), "cbrt {x}");
            assert!(near(atan2(f80(x), f80(0.7), Rounding::Nearest).0, x.atan2(0.7)), "atan2 {x}");
            assert!(near(pow(f80(1.3), f80(x), Rounding::Nearest).0, 1.3f64.powf(x)), "pow {x}");
        }
        assert!(near(sin(f80(1e300), Rounding::Nearest).0, 1e300f64.sin()), "sin 1e300");
    }
}

/// The two evaluations against each other (spec §10.3 correct rounding, D-10.19; no spec clause
/// beyond it): wherever the narrow evaluation decides a result it is the 384-bit one, in every
/// rounding direction, with the same flags and `errno`; and it decides nearly all of them.
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tier_tests {
    use super::*;
    use crate::float::F80;
    use crate::mpn::{self, Mp};

    type F1 = fn(F80, Rounding) -> (F80, Status);
    type F2 = fn(F80, F80, Rounding) -> (F80, Status);

    const MODES: [Rounding; 4] = [Rounding::Nearest, Rounding::Up, Rounding::Down, Rounding::Zero];

    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 ^= self.0 << 13;
            self.0 ^= self.0 >> 7;
            self.0 ^= self.0 << 17;
            self.0
        }
    }

    fn pack80(neg: bool, e: i64, sig: u64) -> F80 {
        F80((u128::from(neg) << 79) | (((e + 16383) as u128) << 64) | u128::from(sig | 1 << 63))
    }

    /// Operands: the common ranges, the whole range, values next to 1, and a few special ones.
    fn operands(seed: u64, n: usize) -> Vec<F80> {
        let mut r = Rng(seed);
        let mut v = vec![F80(0), F80(1u128 << 79), pack80(false, 0, 0), pack80(true, 0, 0), pack80(false, 1, 0), pack80(false, -1, 0)];
        for k in -6i64..=6 {
            v.push(pack80(false, 0, (1u64 << 63).wrapping_add_signed(k)));
            v.push(pack80(true, 0, (1u64 << 63).wrapping_add_signed(k)));
            v.push(pack80(false, -1, (1u64 << 63).wrapping_add_signed(k * 3)));
        }
        for i in 0..n {
            let neg = r.next() & 3 == 0;
            let e = match i % 8 {
                0..=3 => (r.next() % 41) as i64 - 20,
                4 | 5 => (r.next() % 161) as i64 - 80,
                6 => (r.next() % 32766) as i64 - 16382,
                _ => (r.next() % 8) as i64 - 4,
            };
            v.push(pack80(neg, e, r.next()));
        }
        v
    }

    #[test]
    fn narrow_matches_wide_where_it_decides() {
        let one: [(&str, F1, F1); 21] = [
            ("exp", fast::exp::<F80>, slow::exp::<F80>),
            ("exp2", fast::exp2::<F80>, slow::exp2::<F80>),
            ("exp10", fast::exp10::<F80>, slow::exp10::<F80>),
            ("expm1", fast::expm1::<F80>, slow::expm1::<F80>),
            ("log", fast::log::<F80>, slow::log::<F80>),
            ("log2", fast::log2::<F80>, slow::log2::<F80>),
            ("log10", fast::log10::<F80>, slow::log10::<F80>),
            ("log1p", fast::log1p::<F80>, slow::log1p::<F80>),
            ("cbrt", fast::cbrt::<F80>, slow::cbrt::<F80>),
            ("sin", fast::sin::<F80>, slow::sin::<F80>),
            ("cos", fast::cos::<F80>, slow::cos::<F80>),
            ("tan", fast::tan::<F80>, slow::tan::<F80>),
            ("atan", fast::atan::<F80>, slow::atan::<F80>),
            ("asin", fast::asin::<F80>, slow::asin::<F80>),
            ("acos", fast::acos::<F80>, slow::acos::<F80>),
            ("sinh", fast::sinh::<F80>, slow::sinh::<F80>),
            ("cosh", fast::cosh::<F80>, slow::cosh::<F80>),
            ("tanh", fast::tanh::<F80>, slow::tanh::<F80>),
            ("asinh", fast::asinh::<F80>, slow::asinh::<F80>),
            ("acosh", fast::acosh::<F80>, slow::acosh::<F80>),
            ("atanh", fast::atanh::<F80>, slow::atanh::<F80>),
        ];
        let xs = operands(0x9e37_79b9_7f4a_7c15, 240);
        let (mut calls, mut undecided_n) = (0usize, 0usize);
        for (name, f, g) in one {
            for &x in &xs {
                for mode in MODES {
                    let (a, b) = (f(x, mode), g(x, mode));
                    calls += 1;
                    if undecided(&a.1) {
                        undecided_n += 1;
                        continue;
                    }
                    assert!(a.0.to_bits() == b.0.to_bits() && a.1 == b.1, "{name}({:#x}) {mode:?}: {:#x} vs {:#x}", x.0, a.0 .0, b.0 .0);
                }
            }
        }
        let two: [(&str, F2, F2); 2] = [("pow", fast::pow::<F80>, slow::pow::<F80>), ("atan2", fast::atan2::<F80>, slow::atan2::<F80>)];
        let ys = operands(0x1234_5678_9abc_def1, 24);
        for (name, f, g) in two {
            for &x in xs.iter().step_by(3) {
                for &y in &ys {
                    for mode in [Rounding::Nearest, Rounding::Up] {
                        let (a, b) = (f(x, y, mode), g(x, y, mode));
                        calls += 1;
                        if undecided(&a.1) {
                            undecided_n += 1;
                            continue;
                        }
                        assert!(a.0.to_bits() == b.0.to_bits() && a.1 == b.1, "{name}({:#x}, {:#x}) {mode:?}", x.0, y.0);
                    }
                }
            }
        }
        // The test decides nearly everything: well under one result in a hundred is passed on
        // (the exact results and the results at a boundary of a range decision are among them).
        assert!(undecided_n * 100 < calls, "{undecided_n} of {calls} undecided");
    }

    /// The rounding test of the narrow step on values placed around the midpoint and the
    /// representable neighbours at 1 (ulp 2^-63): decided only when every value within
    /// `2^(e − 96)` rounds alike.
    #[test]
    fn rounding_test_decides_only_clear_cases() {
        let one = Mp::from_i64(1);
        let at = |k: i64| one.ldexp(k);
        let half = one.add(at(-64)); // the midpoint of 1 and 1 + 2^-63
        let up = F80(0x3fffu128 << 64 | (1u128 << 63) | 1); // 1 + 2^-63
        let near = Rounding::Nearest;
        assert!(undecided(&fast::approx::<F80>(half, near).1), "the midpoint itself");
        assert!(undecided(&fast::approx::<F80>(half.add(at(-100)), near).1), "a hair above the midpoint");
        assert!(undecided(&fast::approx::<F80>(half.sub(at(-100)), near).1), "a hair below the midpoint");
        assert!(undecided(&fast::approx::<F80>(one, Rounding::Up).1), "an exact value, upward");
        assert!(undecided(&fast::approx::<F80>(one.add(at(-150)), Rounding::Up).1), "a hair above 1, upward");
        let r = fast::approx::<F80>(half.add(at(-80)), near);
        assert!(!undecided(&r.1) && r.0 == up, "clearly above the midpoint");
        let r = fast::approx::<F80>(half.sub(at(-80)), near);
        assert!(!undecided(&r.1) && r.0 == F80(0x3fffu128 << 64 | (1u128 << 63)), "clearly below the midpoint");
        let r = fast::approx::<F80>(one.add(at(-70)), Rounding::Up);
        assert!(!undecided(&r.1) && r.0 == up, "clearly above 1, upward");
        let r = fast::approx::<F80>(one.add(at(-70)), Rounding::Down);
        assert!(!undecided(&r.1) && r.0 == F80(0x3fffu128 << 64 | (1u128 << 63)), "clearly above 1, downward");
        // The narrow width is the one the documentation and the allowance state.
        assert_eq!(mpn::WIDTH, 192);
    }

    /// Only a format of at most 64 bits takes the narrow step.
    #[test]
    fn narrow_step_is_for_x87_only() {
        assert!(narrow::<F80>());
        assert!(!narrow::<crate::float::F128>());
    }
}
