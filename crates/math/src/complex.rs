//! Complex functions (spec §10.5, Track M milestones 4 and 5) of `float`, `double` and x87
//! `long double` and `_Float128` (every function generic over [`Real`]; the cut-offs are derived
//! for precisions up to 113 bits, [`MAX_PREC`], and a wider format is refused at compile time).
//!
//! Each function takes the real and imaginary parts and returns both parts of the result with
//! one [`Status`]. A finite result is evaluated in the 384-bit arithmetic of [`crate::mp`] from
//! formulas in which nothing cancels, and each part is rounded once, so every part is correctly
//! rounded in every rounding direction; parts that are exact (`csqrt(−4) = 2i`,
//! `clog10(6 + 8i) = 1`, `cpow(1 + i, 2) = 2i`, the zeros) are recognised and returned exactly.
//! The formulas, with `z = x + iy`:
//!
//! * `cexp z = e^x (cos y + i sin y)`; `clog z = ½ ln(x² + y²) + i atan2(y, x)` with
//!   `ln(1 + (x² − 1 + y²))` near `|z| = 1` (`x² − 1` is exact); `clog10` divides by `ln 10`.
//! * `csqrt z`: `t = √((|z| + |x|)/2)` and `|y|/(2t)`, placed by the sign of `x`.
//! * `csin`, `ccos`, `csinh`, `ccosh`: products of `sin`, `cos`, `sinh`, `cosh` of the parts.
//! * `ctanh z = (sinh x cosh x + i sin y cos y) / (sinh² x + cos² y)` (`ctan` swaps the roles);
//!   far from the axis the real part is `±(1 + δ)` with `δ = −(cos 2y + e^(−2|x|)) / (2D)`.
//! * `casin`, `cacos`, `casinh`, `cacosh` by Hull, Fairgrieve and Tang's decomposition (ACM
//!   TOMS 23(3), 1997): `A = (|z + 1| + |z − 1|)/2`, with `A − 1` and `A − |x|` as sums of
//!   positive terms; `casin z = atan2(|x|, √((A − |x|)(A + |x|))) + i ln(1 + (A − 1) +
//!   √((A − 1)(A + 1)))` with the signs of `z`.
//! * `catanh z = ¼ ln(1 + 4|x|/((1 − |x|)² + y²)) + i ½ atan2(2|y|, (1 − |x|)(1 + |x|) − y²)`
//!   (the larger part paired with the 1, so the difference is exact before it is truncated once).
//! * `cpow(z, w) = e^(c ln|z| − d arg z) · cis(d ln|z| + c arg z)` for `w = c + id`; a real
//!   exponent of a base on an axis by exact angles (`(−8)^(1/3)`, `i²`), small integer powers by
//!   exact multiplication.
//!
//! The special values follow C Annex G as glibc implements them (D-10.18,
//! `spec/behaviour/libm-complex.md`): its choice of NaN (the positive quiet NaN, the operand, or
//! the default NaN of an invalid operation), its invalid flag for a quiet NaN where Annex G makes
//! it optional, and `cpow` with an infinite, NaN or zero operand as `cexp(w · clog z)` with
//! Annex G's multiplication (G.5.1), which is how glibc computes it. Flags follow IEEE 754
//! (D-10.14: inexact exactly when a part is inexact); `errno` follows §10.0 (`ERANGE` for a part
//! that overflows to ±∞ or underflows to ±0 and for a pole, `EDOM` for an invalid operation on
//! operands that are not NaNs).

#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::cast_possible_truncation, clippy::float_cmp, clippy::too_many_lines, clippy::many_single_char_names)] // exponents and small integer powers of bounded range; exact comparisons with representable values

use crate::float::Float;
use crate::real::Real;
use crate::status::{Rounding, Status};
use core::sync::atomic::Ordering;

// Ziv's strategy (owner #1719, D-10.17): the evaluation below is compiled twice, in the 384-bit
// arithmetic of `crate::mp` (`n`) and the 768-bit arithmetic of `crate::mpw` (`w`). Each
// approximate part carries a proven bound on its absolute error (`Part::Mp(v, err)`); a part
// whose interval `v ± 2^err` straddles a rounding boundary in the current mode is undecided.
// `n` counts it, and the function is evaluated again in `w`; a part still undecided there takes
// the counted `TODO(ZIV-C)` path (`w`'s counter, [`ziv_c_pending`]). The loop has at most two
// steps, so it always ends; exact parts (`Part::Exact`, `Dev`, `Near`, `Zero`) never take the
// test.

/// The evaluation in 384 bits.
mod n {
    use crate::mp;
    include!("complex_body.rs");
}

/// The same evaluation in 768 bits (the retry step; the functions without approximate parts
/// are never called here).
#[allow(dead_code)]
mod w {
    use crate::mpw as mp;
    include!("complex_body.rs");
}

/// Both parts of a complex result and what they report together.
pub type Res<F> = ((F, F), Status);

/// `creal`.
pub fn creal<F: Float>(x: F, y: F) -> F {
    n::creal(x, y)
}

/// `cimag`.
pub fn cimag<F: Float>(x: F, y: F) -> F {
    n::cimag(x, y)
}

/// `conj`.
pub fn conj<F: Float>(x: F, y: F, mode: Rounding) -> Res<F> {
    n::conj(x, y, mode)
}

/// `cproj`.
pub fn cproj<F: Float>(x: F, y: F, mode: Rounding) -> Res<F> {
    n::cproj(x, y, mode)
}

/// `cabs` (no approximate part: `hypot`).
pub fn cabs<F: Float>(x: F, y: F, mode: Rounding) -> (F, Status) {
    n::cabs(x, y, mode)
}

/// How many evaluations had a part undecided at 384 bits (and were evaluated again at 768).
pub fn retries() -> u64 {
    n::UNDECIDED.load(Ordering::Relaxed)
}

/// How many parts were still undecided at 768 bits: the path ZIV-C extends (`TODO(ZIV-C)`); each
/// such part is the 768-bit value rounded once.
pub fn ziv_c_pending() -> u64 {
    w::UNDECIDED.load(Ordering::Relaxed)
}

/// `fast`, or `wide` when one of `fast`'s parts was undecided. The counter only grows; another
/// thread's increment at worst causes one extra (still correct) evaluation in 768 bits.
#[inline]
fn ziv<T>(fast: impl FnOnce() -> T, wide: impl FnOnce() -> T) -> T {
    let before = n::UNDECIDED.load(Ordering::Relaxed);
    let r = fast();
    if n::UNDECIDED.load(Ordering::Relaxed) == before { r } else { wide() }
}

macro_rules! ziv_fns {
    ($($(#[$m:meta])* $name:ident;)*) => {$(
        $(#[$m])*
        pub fn $name<F: Real>(x: F, y: F, mode: Rounding) -> Res<F> {
            ziv(|| n::$name(x, y, mode), || w::$name(x, y, mode))
        }
    )*};
}

ziv_fns! {
    /// `cexp`.
    cexp;
    /// `clog`.
    clog;
    /// `clog10`.
    clog10;
    /// `csqrt`.
    csqrt;
    /// `csinh`.
    csinh;
    /// `ccosh`.
    ccosh;
    /// `csin`.
    csin;
    /// `ccos`.
    ccos;
    /// `ctanh`.
    ctanh;
    /// `ctan`.
    ctan;
    /// `casin`.
    casin;
    /// `cacos`.
    cacos;
    /// `casinh`.
    casinh;
    /// `cacosh`.
    cacosh;
    /// `catanh`.
    catanh;
    /// `catan`.
    catan;
}

/// `carg`.
pub fn carg<F: Real>(x: F, y: F, mode: Rounding) -> (F, Status) {
    ziv(|| n::carg(x, y, mode), || w::carg(x, y, mode))
}

/// `cpow(x + iy, c + id)`.
pub fn cpow<F: Real>(x: F, y: F, c: F, d: F, mode: Rounding) -> Res<F> {
    ziv(|| n::cpow(x, y, c, d, mode), || w::cpow(x, y, c, d, mode))
}

/// The retry of Ziv's strategy, forced through the test-only bound offset of each width
/// (MATH-M5-CPLX-F128-ZIV-B1; spec D-10.17).
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod ziv_tests {
    use super::*;
    use crate::float::{F128, Float};

    const MODES: [Rounding; 4] = [Rounding::Nearest, Rounding::Up, Rounding::Down, Rounding::Zero];

    fn q(v: f64) -> F128 {
        crate::mp::round_exact::<F128>(crate::mp::Mp::from_f64(v), Rounding::Nearest).0
    }

    type Fun = fn(F128, F128, Rounding) -> Res<F128>;

    /// Inputs that reach the general (`Part::Mp`) regime of each family.
    fn cases() -> [(&'static str, Fun, f64, f64); 9] {
        [
            ("carg10/clog10", clog10::<F128>, 0.3, 0.7),
            ("clog", clog::<F128>, 3.0, 0.7),
            ("clog near 1", clog::<F128>, 0.6, 0.7),
            ("casin", casin::<F128>, 0.3, 0.7),
            ("cacos", cacos::<F128>, 0.3, 0.7),
            ("casinh", casinh::<F128>, 0.3, 0.7),
            ("cacosh", cacosh::<F128>, 0.3, 0.7),
            ("catanh", catanh::<F128>, 0.3, 0.7),
            ("catan", catan::<F128>, 0.3, 0.7),
        ]
    }

    fn same(a: Res<F128>, b: Res<F128>) -> bool {
        a.0 .0.to_bits() == b.0 .0.to_bits() && a.0 .1.to_bits() == b.0 .1.to_bits() && a.1 == b.1
    }

    /// Loosening the 384-bit bound past every boundary takes the retry, and the 768-bit
    /// evaluation gives the same correctly rounded parts (no spec clause; D-10.17).
    #[test]
    fn forced_384_undecided_retries_at_768() {
        for (name, f, x, y) in cases() {
            for m in MODES {
                let want = f(q(x), q(y), m);
                let before = retries();
                n::force_loose(1000);
                let got = f(q(x), q(y), m);
                n::force_loose(0);
                assert!(retries() > before, "{name} {m:?}: no retry");
                assert!(same(got, want), "{name} {m:?}: retry changed the result");
            }
        }
    }

    /// Undecided at both widths: the counted `TODO(ZIV-C)` path is taken and returns the 768-bit
    /// value rounded once (no spec clause; D-10.17).
    #[test]
    fn forced_768_undecided_takes_the_ziv_c_path() {
        for (name, f, x, y) in cases() {
            let want = f(q(x), q(y), Rounding::Nearest);
            let before = ziv_c_pending();
            n::force_loose(1000);
            w::force_loose(1000);
            let got = f(q(x), q(y), Rounding::Nearest);
            n::force_loose(0);
            w::force_loose(0);
            assert!(ziv_c_pending() > before, "{name}: ZIV-C path not counted");
            assert!(same(got, want), "{name}: ZIV-C path changed the result");
        }
    }

    /// `carg` and `cpow` go through the same step (no spec clause; D-10.17).
    #[test]
    fn carg_and_cpow_retry() {
        let before = retries();
        n::force_loose(1000);
        let a = carg(q(0.3), q(0.7), Rounding::Up);
        let p = cpow(q(0.3), q(0.7), q(1.5), q(0.25), Rounding::Down);
        n::force_loose(0);
        assert!(retries() >= before + 2);
        assert_eq!(a.0.to_bits(), carg(q(0.3), q(0.7), Rounding::Up).0.to_bits());
        assert!(same(p, cpow(q(0.3), q(0.7), q(1.5), q(0.25), Rounding::Down)));
    }

    /// The sites with a proven bound (clog/clog10 through `ln_abs`, carg10, cpow of a base on an
    /// axis) decide at 384 bits on their own; loosened past every boundary they retry at 768
    /// bits and give the same parts, in all four modes (no spec clause; D-10.17).
    #[test]
    fn proven_sites_decide_at_384_and_retry_when_forced() {
        let cases: [(&str, Fun, f64, f64); 8] = [
            ("clog |z| > 2 (ln_atanh)", clog::<F128>, 3.0, 0.7),
            ("clog |z| < 1/2 (ln_atanh)", clog::<F128>, 0.25, 0.125),
            ("clog near 1 (ln1p)", clog::<F128>, 0.6, 0.7),
            ("clog10 near 1 + carg10", clog10::<F128>, 0.3, 0.7),
            ("clog10 |z| > 2 + carg10", clog10::<F128>, 3.0, 0.7),
            ("cpow axis |z| = 8 (ln_atanh)", |x, y, m| cpow(x, y, q(0.3), q(0.0), m), -8.0, 0.0),
            ("cpow axis |z| = 1.5 (ln1p)", |x, y, m| cpow(x, y, q(0.3), q(0.0), m), 0.0, 1.5),
            ("cpow axis |z| = 1", |x, y, m| cpow(x, y, q(0.3), q(0.0), m), 0.0, -1.0),
        ];
        for (name, f, x, y) in cases {
            for m in MODES {
                let before = n::misses();
                let want = f(q(x), q(y), m);
                assert_eq!(n::misses(), before, "{name} {m:?}: undecided at 384 bits");
                n::force_loose(1000);
                let got = f(q(x), q(y), m);
                n::force_loose(0);
                assert!(n::misses() > before, "{name} {m:?}: no retry when forced");
                assert!(same(got, want), "{name} {m:?}: retry changed the result");
            }
        }
    }
}
