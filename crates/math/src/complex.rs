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

// Ziv's strategy (owner #1719, #1758, D-10.17): the evaluation below is compiled four times, in
// the 384-bit arithmetic of `crate::mp` (`n`), the 768-bit of `crate::mpw` (`w`), the 1536-bit of
// `crate::mpx` (`wx`) and the 3072-bit of `crate::mpy` (`wy`). Each approximate part carries a
// proven bound on its absolute error (`Part::Mp(v, err)`); a part whose interval `v ± 2^err`
// straddles a rounding boundary in the current mode is undecided. Each width counts its undecided
// parts, and an undecided part makes the function evaluate again at the next width (`ladder`,
// entered cold so the 384-bit frame does not grow). At 3072 bits an undecided part is the
// 3072-bit value rounded once and counted ([`residual`]): the one assumption of the guarantee is
// that no binary128 input needs more (D-10.17). The ladder has at most four steps, so it always
// ends; exact parts (`Part::Exact`, `Dev`, `Near`, `Zero`) never take the test.

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

/// The same evaluation in 1536 bits (the third step).
#[allow(dead_code, clippy::large_enum_variant)] // the parts hold values of this width inline (no allocation)
mod wx {
    use crate::mpx as mp;
    include!("complex_body.rs");
}

/// The same evaluation in 3072 bits (the last step).
#[allow(dead_code, clippy::large_enum_variant)] // the parts hold values of this width inline (no allocation)
mod wy {
    use crate::mpy as mp;
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

/// How many evaluations had a part undecided at 768 bits (and were evaluated again at 1536).
pub fn wide_retries() -> u64 {
    w::UNDECIDED.load(Ordering::Relaxed)
}

/// How many evaluations had a part undecided at 1536 bits (and were evaluated again at 3072).
pub fn wider_retries() -> u64 {
    wx::UNDECIDED.load(Ordering::Relaxed)
}

/// How many parts were still undecided at 3072 bits: each is the 3072-bit value rounded once,
/// the case D-10.17's assumption excludes (zero on every suite and sample run).
pub fn residual() -> u64 {
    wy::UNDECIDED.load(Ordering::Relaxed)
}

/// `this`, or `next` when one of `this`'s parts was undecided (`c`, the counter of `this`'s
/// width, grew). The counters only grow; another thread's increment at worst causes one extra
/// (still correct) evaluation at the next width.
#[inline(always)]
fn step<T>(c: &core::sync::atomic::AtomicU64, this: impl FnOnce() -> T, next: impl FnOnce() -> T) -> T {
    let before = c.load(Ordering::Relaxed);
    let r = this();
    if c.load(Ordering::Relaxed) == before { r } else { next() }
}

/// The 384-bit evaluation, and the ladder when it leaves a part undecided.
#[inline]
fn ziv<T>(fast: impl FnOnce() -> T, w: impl FnOnce() -> T, x: impl FnOnce() -> T, y: impl FnOnce() -> T) -> T {
    step(&n::UNDECIDED, fast, || ladder(w, x, y))
}

/// The 768-bit step, then 1536 when it leaves a part undecided.
#[cold]
#[inline(never)]
fn ladder<T>(w: impl FnOnce() -> T, x: impl FnOnce() -> T, y: impl FnOnce() -> T) -> T {
    step(&w::UNDECIDED, w, || wider(x, y))
}

/// The 1536-bit step, then 3072 when it leaves a part undecided.
#[cold]
#[inline(never)]
fn wider<T>(x: impl FnOnce() -> T, y: impl FnOnce() -> T) -> T {
    step(&wx::UNDECIDED, x, y)
}

macro_rules! ziv_fns {
    ($($(#[$m:meta])* $name:ident;)*) => {$(
        $(#[$m])*
        pub fn $name<F: Real>(x: F, y: F, mode: Rounding) -> Res<F> {
            ziv(|| n::$name(x, y, mode), || w::$name(x, y, mode), || wx::$name(x, y, mode), || wy::$name(x, y, mode))
        }
    )*};
}

/// `cexp`: the quick tier of `float` and `double` ([`crate::complex_fast`], D-10.53) when it
/// can prove both parts, else the 384-bit route and its ladder.
pub fn cexp<F: Real>(x: F, y: F, mode: Rounding) -> Res<F> {
    if let Some(r) = crate::complex_fast::cexp_any(x, y, mode) {
        return r;
    }
    cexp_route(x, y, mode)
}

/// `cexp` by the 384-bit route and its ladder alone (the quick tier's fallback, and its
/// reference in the tests); not part of the ABI.
#[doc(hidden)]
pub fn cexp_route<F: Real>(x: F, y: F, mode: Rounding) -> Res<F> {
    ziv(|| n::cexp(x, y, mode), || w::cexp(x, y, mode), || wx::cexp(x, y, mode), || wy::cexp(x, y, mode))
}

ziv_fns! {
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
    ziv(|| n::carg(x, y, mode), || w::carg(x, y, mode), || wx::carg(x, y, mode), || wy::carg(x, y, mode))
}

/// `cpow(x + iy, c + id)`.
pub fn cpow<F: Real>(x: F, y: F, c: F, d: F, mode: Rounding) -> Res<F> {
    ziv(|| n::cpow(x, y, c, d, mode), || w::cpow(x, y, c, d, mode), || wx::cpow(x, y, c, d, mode), || wy::cpow(x, y, c, d, mode))
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

    /// Loosen the bounds of the first `k` widths (384, 768, 1536, 3072) by `2^4000`, or undo it.
    fn force(k: usize, on: bool) {
        let v = if on { 4000 } else { 0 }; // past every boundary at 3072 bits too
        let f: [fn(i64); 4] = [n::force_loose, w::force_loose, wx::force_loose, wy::force_loose];
        for g in f.iter().take(k) {
            g(v);
        }
    }

    /// Each step of the ladder, forced: undecided at the first `k` widths, every family takes
    /// the next step (its counter grows), and the result is the same correctly rounded parts; at
    /// `k = 4` the counted residual path returns the 3072-bit value rounded once (no spec
    /// clause; D-10.17).
    #[test]
    fn forced_undecided_climbs_each_step_of_the_ladder() {
        let counters: [fn() -> u64; 3] = [wide_retries, wider_retries, residual];
        for (k, counter) in (2..=4).zip(counters) {
            for (name, f, x, y) in cases() {
                for m in MODES {
                    let want = f(q(x), q(y), m);
                    let before = counter();
                    force(k, true);
                    let got = f(q(x), q(y), m);
                    force(k, false);
                    assert!(counter() > before, "{name} {m:?}: step {k} not taken");
                    assert!(same(got, want), "{name} {m:?}: step {k} changed the result");
                }
            }
        }
        // carg and cpow take the same ladder.
        let want = carg::<F128>(q(0.3), q(0.7), Rounding::Up);
        let before = residual();
        force(4, true);
        let got = carg::<F128>(q(0.3), q(0.7), Rounding::Up);
        let pw = cpow::<F128>(q(0.3), q(0.7), q(1.5), q(0.25), Rounding::Down);
        force(4, false);
        assert!(residual() >= before + 2, "carg/cpow: residual path not taken");
        assert_eq!(got.0.to_bits(), want.0.to_bits());
        assert!(same(pw, cpow::<F128>(q(0.3), q(0.7), q(1.5), q(0.25), Rounding::Down)), "cpow: ladder changed the result");
    }

    /// A subnormal or minimum-normal operand of a sine or cosine decides at 384 bits in all four
    /// modes (ZIV-C Step 0: `small_trig` gives the sine a relative bound; with the absolute
    /// `2^(10 − B)` such parts were undecided at every width), for each family that takes a sine
    /// or cosine of an operand (no spec clause; D-10.17).
    #[test]
    fn tiny_trig_operands_decide_at_384() {
        let tiny = [F128::from_bits(1), F128::from_bits(1 << 112), F128::from_bits((1 << 127) | (1 << 100))];
        let fams: [(&str, Fun, bool); 7] = [
            ("cexp", cexp::<F128>, true),
            ("csinh", csinh::<F128>, true),
            ("ccosh", ccosh::<F128>, true),
            ("ctanh", ctanh::<F128>, true),
            ("csin", csin::<F128>, false),
            ("ccos", ccos::<F128>, false),
            ("ctan", ctan::<F128>, false),
        ];
        for (name, f, tiny_im) in fams {
            for t in tiny {
                for o in [q(0.5), q(-1.5), q(7.0)] {
                    for m in MODES {
                        let (n0, w0) = (n::misses(), w::misses());
                        let _ = if tiny_im { f(o, t, m) } else { f(t, o, m) };
                        assert_eq!((n::misses(), w::misses()), (n0, w0), "{name} {m:?} tiny {:x}: undecided", t.to_bits());
                    }
                }
            }
        }
        // cpow's angle `Φ = d ln|z| + c arg z` tiny through a tiny `y` and a tiny `d`: `cis Φ`
        // by `small_trig` with `Φ`'s own bound.
        for m in MODES {
            let n0 = n::misses();
            let _ = cpow(q(3.0), F128::from_bits(1 << 112), q(1.5), F128::from_bits(1 << 112), m);
            assert_eq!(n::misses(), n0, "cpow {m:?}: undecided");
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

    /// The csqrt and inverse-function sites proven in ZIV-B1-3 (`sqrt_general`, `asin_parts`'s
    /// `Q` and `I`, `angle`, `atanh_parts`'s `R` and run-time `θ/2`) decide at 384 bits on their
    /// own, and retry at 768 bits with the same parts when loosened, in all four modes; both
    /// branches of `asin_parts` (`|x| < 1`, `|x| >= 1`), `x < 0`, the axes of catanh/catan and a
    /// cancelling `d` next to `|z| = 1` (no spec clause; D-10.17).
    #[test]
    fn inverse_and_sqrt_sites_decide_at_384_and_retry_when_forced() {
        let tiny = 2f64.powi(-200);
        let cases: [(&str, Fun, f64, f64); 20] = [
            ("csqrt", csqrt::<F128>, 0.3, 0.7),
            ("csqrt x < 0", csqrt::<F128>, -3.0, 0.7),
            ("csqrt ratio small part", csqrt::<F128>, 9.0, tiny),
            ("csqrt ratio small part x < 0", csqrt::<F128>, -9.0, tiny),
            ("casin |x| < 1", casin::<F128>, 0.3, 0.7),
            ("casin |x| > 1", casin::<F128>, -3.0, 0.7),
            ("cacos |x| < 1", cacos::<F128>, 0.3, -0.7),
            ("cacos x < -1", cacos::<F128>, -3.0, 0.7),
            ("casinh", casinh::<F128>, 0.7, 0.3),
            ("casinh |y| > 1", casinh::<F128>, 0.7, -3.0),
            ("cacosh", cacosh::<F128>, 0.3, 0.7),
            ("cacosh x < -1", cacosh::<F128>, -3.0, 0.7),
            ("catanh", catanh::<F128>, 0.3, 0.7),
            ("catanh |z| next to 1", catanh::<F128>, 0.6, 0.8),
            ("catanh y = 1", catanh::<F128>, 0.5, 1.0),
            ("catanh axis x > 1", catanh::<F128>, -2.0, 0.0),
            ("catanh large", catanh::<F128>, 5.0, -30.0),
            ("catan", catan::<F128>, 0.7, 0.3),
            ("catan |z| next to 1", catan::<F128>, 0.8, -0.6),
            ("catan axis y > 1", catan::<F128>, 0.0, 2.0),
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

    /// The run-time bounds (R3/R4: `cexp`, the products of `csinh`/`ccosh`/`csin`/`ccos`,
    /// `ctanh`/`ctan` both sides of |h| = 24, `cpow` on an axis, a diagonal, in general and by
    /// a negative integer) decide at 384 bits in all four modes and retry to the same parts when
    /// forced (no spec clause; D-10.17).
    #[test]
    fn run_time_bound_sites_decide_at_384_and_retry_when_forced() {
        let cases: [(&str, Fun, f64, f64); 14] = [
            ("cexp", cexp::<F128>, 0.3, 0.7),
            ("cexp large", cexp::<F128>, 700.0, -1.0e6),
            ("cexp big y", cexp::<F128>, -3.0, 1.0e30),
            ("csinh", csinh::<F128>, 0.3, 0.7),
            ("ccosh", ccosh::<F128>, -2.5, 3.0),
            ("csin", csin::<F128>, 0.3, 0.7),
            ("ccos", ccos::<F128>, 5.0, -0.25),
            ("ctanh", ctanh::<F128>, 0.3, 0.7),
            ("ctanh |h| > 24", ctanh::<F128>, 30.0, 0.7),
            ("ctanh t next to pi/2", ctanh::<F128>, 0.001, core::f64::consts::FRAC_PI_2),
            ("ctan", ctan::<F128>, 0.7, 0.3),
            ("ctan |h| > 24", ctan::<F128>, 0.7, -40.0),
            ("cpow-like csinh big", csinh::<F128>, 1000.0, 2.0),
            ("ccos big", ccos::<F128>, 1.0e20, 3.0),
        ];
        let pows: [(&str, [f64; 4]); 5] = [
            ("cpow general", [0.3, 0.7, 1.5, -0.25]),
            ("cpow axis base -1", [-1.0, 0.0, 0.3, 0.7]),
            ("cpow axis base i", [0.0, 1.0, 2.5, -1.25]),
            ("cpow diagonal", [3.0, 3.0, 0.3, 0.0]),
            ("cpow negative integer", [3.0, 1.0, -3.0, 0.0]),
        ];
        let run = |name: &str, f: &dyn Fn(Rounding) -> Res<F128>| {
            for m in MODES {
                let before = n::misses();
                let want = f(m);
                assert_eq!(n::misses(), before, "{name} {m:?}: undecided at 384 bits");
                n::force_loose(1000);
                let got = f(m);
                n::force_loose(0);
                assert!(n::misses() > before, "{name} {m:?}: no retry when forced");
                assert!(same(got, want), "{name} {m:?}: retry changed the result");
            }
        };
        for (name, f, x, y) in cases {
            run(name, &|m| f(q(x), q(y), m));
        }
        for (name, [x, y, c, d]) in pows {
            run(name, &|m| cpow::<F128>(q(x), q(y), q(c), q(d), m));
        }
    }

    /// The retry count on a random sample (#1761: expected about zero): every family, all four
    /// modes, operands of random sign, 53-bit significand and exponent in [−12, 12]. Run by
    /// hand or as a count job (`-- --ignored --nocapture`); it prints the counts and requires
    /// no part to stay undecided at 768 bits (no spec clause; D-10.17).
    #[test]
    #[ignore = "count job: random-sample retry rate"]
    fn random_sample_retry_count() {
        let fams: [(&str, Fun); 15] = [
            ("cexp", cexp::<F128>), ("clog", clog::<F128>), ("clog10", clog10::<F128>), ("csqrt", csqrt::<F128>),
            ("csinh", csinh::<F128>), ("ccosh", ccosh::<F128>), ("csin", csin::<F128>), ("ccos", ccos::<F128>),
            ("ctanh", ctanh::<F128>), ("ctan", ctan::<F128>), ("casin", casin::<F128>), ("cacos", cacos::<F128>),
            ("casinh", casinh::<F128>), ("cacosh", cacosh::<F128>), ("catanh", catanh::<F128>),
        ];
        let mut s = 0x9e37_79b9_7f4a_7c15u64;
        let mut rnd = || {
            s ^= s << 13;
            s ^= s >> 7;
            s ^= s << 17;
            let m = 1.0 + (s >> 11) as f64 / (1u64 << 53) as f64;
            let e = ((s >> 3) % 25) as i32 - 12;
            if s & 1 == 1 { -m * 2f64.powi(e) } else { m * 2f64.powi(e) }
        };
        const N: u64 = 500;
        let (n0, w0, x0, y0) = (n::misses(), w::misses(), wx::misses(), wy::misses());
        for (name, f) in fams {
            let before = n::misses();
            for _ in 0..N {
                let (x, y) = (rnd(), rnd());
                for m in MODES {
                    let _ = f(q(x), q(y), m);
                }
            }
            std::println!("retry-count {name}: {} of {} calls retried", n::misses() - before, N * 4);
        }
        let before = n::misses();
        for _ in 0..N {
            let (x, y, c, d) = (rnd(), rnd(), rnd() / 64.0, rnd() / 64.0);
            for m in MODES {
                let _ = cpow::<F128>(q(x), q(y), q(c), q(d), m);
            }
        }
        std::println!("retry-count cpow: {} of {} calls retried", n::misses() - before, N * 4);
        std::println!("retry-count total: {} retried of {} calls; at 768 {}, 1536 {}, 3072 {}", n::misses() - n0, N * 4 * 16, w::misses() - w0, wx::misses() - x0, wy::misses() - y0);
        assert_eq!(w::misses(), w0, "a random part was undecided at 768 bits");
    }

    /// #1905: `cpow` with huge `|c|`, `|d|` is not decidable at 768 bits; the ladder decides it
    /// at 1536 or 3072 bits, before the residual path (no spec clause; D-10.17).
    #[test]
    fn cpow_huge_exponents_decide_on_the_ladder() {
        let big = 2f64.powi(1000);
        for m in MODES {
            let (n0, w0, y0) = (n::misses(), w::misses(), wy::misses());
            let _ = cpow::<F128>(q(0.3), q(0.7), q(big), q(big * 0.75), m);
            assert!(n::misses() > n0 && w::misses() > w0, "{m:?}: huge exponents decided at 768 bits");
            assert_eq!(wy::misses(), y0, "{m:?}: huge exponents undecided at 3072 bits");
        }
    }

    /// `F128` of `v · 2^e` (exact for `v < 2^113`).
    fn sc(v: u128, e: i64) -> F128 {
        crate::mp::round_exact::<F128>(crate::mp::Mp::from_u128(false, v, e), Rounding::Nearest).0
    }

    /// Constructed hard-to-round inputs, no bound loosened: Pell's `p² − 3q² = 1` (`p < 2^113`)
    /// at `2^-252` makes the cubic terms of the odd families cancel in one part to `2^-504` of
    /// it (`x² − 3y²` or `y² − 3x²`), so that part is a representable operand within `2^-500`
    /// relative: undecided at 384 bits in the directed modes, decided at 768. `x = y = m·2^-197`
    /// (`m` odd, 57 bits, `m² > 2^113`) makes `ccosh`/`ccos` a midpoint `x²` within `x⁶/90` and
    /// `1` within `x⁴/6`: undecided in all four modes. `cexp` at `x = y²/2` is `1 − y⁴/12`;
    /// `y/x = 2^-200` makes `atan2` and `√` parts a power of two within `2^-400` (`clog`, `csqrt`).
    /// Each family but `clog`, `clog10`, `csqrt` must retry in each directed mode, `ccosh`/`ccos`
    /// also to nearest; every family must give the 768-bit parts (no spec clause; D-10.17).
    #[test]
    fn constructed_hard_cases_retry_at_768() {
        let fams: [(&str, Fun, Fun); 15] = [
            ("cexp", cexp::<F128>, w::cexp::<F128>), ("clog", clog::<F128>, w::clog::<F128>),
            ("clog10", clog10::<F128>, w::clog10::<F128>), ("csqrt", csqrt::<F128>, w::csqrt::<F128>),
            ("csinh", csinh::<F128>, w::csinh::<F128>), ("ccosh", ccosh::<F128>, w::ccosh::<F128>),
            ("csin", csin::<F128>, w::csin::<F128>), ("ccos", ccos::<F128>, w::ccos::<F128>),
            ("ctanh", ctanh::<F128>, w::ctanh::<F128>), ("ctan", ctan::<F128>, w::ctan::<F128>),
            ("casin", casin::<F128>, w::casin::<F128>), ("cacos", cacos::<F128>, w::cacos::<F128>),
            ("casinh", casinh::<F128>, w::casinh::<F128>), ("cacosh", cacosh::<F128>, w::cacosh::<F128>),
            ("catanh", catanh::<F128>, w::catanh::<F128>),
        ];
        let (mut p, mut r) = (2u128, 1u128);
        while 2 * p + 3 * r < 1 << 113 {
            (p, r) = (2 * p + 3 * r, p + 2 * r);
        }
        let m57 = (3u128 << 55) + 1;
        let base = [(sc(p, -252), sc(r, -252)), (sc(r, -252), sc(p, -252)), (sc(m57, -197), sc(m57, -197)), (sc(1, -281), sc(1, -140)), (sc(1, 60), sc(1, -140)), (q(1e16), sc(1, -140))];
        let neg = |v: F128| F128::from_bits(v.to_bits() ^ (1 << 127));
        let mut fails = std::vec::Vec::new();
        for (name, f, wf) in fams.into_iter().chain([("catan", catan::<F128> as Fun, w::catan::<F128> as Fun)]) {
            let mut hit = [0u32; 4];
            for &(x, y) in &base {
                for (x, y) in [(x, y), (neg(x), neg(y))] {
                    for (h, m) in hit.iter_mut().zip(MODES) {
                        let (n0, w0) = (n::misses(), w::misses());
                        let got = f(x, y, m);
                        *h += u32::from(n::misses() > n0);
                        if !same(got, wf(x, y, m)) || w::misses() != w0 {
                            fails.push(std::format!("{name} {m:?} ({:x}, {:x}): not the 768-bit parts", x.to_bits(), y.to_bits()));
                        }
                    }
                }
            }
            std::println!("hard-case {name}: retries N={} U={} D={} Z={}", hit[0], hit[1], hit[2], hit[3]);
            let need_n = name == "ccosh" || name == "ccos";
            // clog, clog10, csqrt: every input built this way lands in an exact or `Dev` regime
            // (far angle, `|z|` next to 1, power of ten, exact root); their `Mp` parts take the same
            // `Part::round` (`part_rounding_test_at_constructed_boundaries`).
            let exempt = matches!(name, "clog" | "clog10" | "csqrt");
            if !exempt && (hit[1] == 0 || hit[2] == 0 || hit[3] == 0 || (need_n && hit[0] == 0)) {
                fails.push(std::format!("{name}: no retry in some mode {hit:?}"));
            }
        }
        assert!(fails.is_empty(), "{fails:#?}");
    }

    /// Exact quotients (`(1 + i)^-2`, `^-4`, `^-6`) are detected and decided without a retry
    /// in every mode, so the step never loops on them (no spec clause; D-10.17).
    #[test]
    fn exact_quotients_decide_without_retry() {
        for (k, re, im) in [(-2.0, 0.0, -0.5), (-4.0, -0.25, 0.0), (-6.0, 0.0, 0.125)] {
            for m in MODES {
                let (n0, w0) = (n::misses(), w::misses());
                let ((a, b), _) = cpow(q(1.0), q(1.0), q(k), q(0.0), m);
                assert!(n::misses() == n0 && w::misses() == w0, "(1+i)^{k} {m:?}: retried");
                assert!(a.to_bits() & !(1 << 127) == q(re).to_bits() & !(1 << 127) && b.to_bits() == q(im).to_bits(), "(1+i)^{k} {m:?}");
            }
        }
    }

    /// The exact value of `a + side·tiny` in format `F` and mode `m`: `a` (at most 113 bits) moved
    /// by `|a|·2^-200` toward `side` (or not at all for `side = 0`), which lies strictly between
    /// `a` and its neighbour on that side, as any deviation far below the last place does.
    fn beside<F: Real>(a: crate::mp::Mp, side: i64, m: Rounding) -> F {
        let d = a.abs().ldexp(-200);
        let v = if side > 0 { a.add(d) } else if side < 0 { a.sub(d) } else { a };
        crate::mp::round_exact::<F>(v, m).0
    }

    /// `cpow(x + iy, n)` against parts given as an exact anchor and the side the exact value
    /// lies on, in all four modes, and decided at 384 bits (no residual, no retry).
    fn far_pow<F: Real>(name: &str, x: crate::mp::Mp, y: crate::mp::Mp, n: i64, re: (crate::mp::Mp, i64), im: (crate::mp::Mp, i64)) {
        use crate::mp::Mp;
        for m in MODES {
            let r = |v: Mp| crate::mp::round_exact::<F>(v, Rounding::Nearest).0;
            let (before, res) = (n::misses(), residual());
            let ((a, b), _) = cpow::<F>(r(x), r(y), r(Mp::from_i64(n)), r(Mp::ZERO), m);
            let (wa, wb) = (beside::<F>(re.0, re.1, m), beside::<F>(im.0, im.1, m));
            assert!(a.to_bits() == wa.to_bits() && b.to_bits() == wb.to_bits(), "{name}^{n} {m:?}: got ({a:?}, {b:?}) want ({wa:?}, {wb:?})");
            assert_eq!((n::misses(), residual()), (before, res), "{name}^{n} {m:?}: not decided at 384 bits");
        }
    }

    /// cpow with a small integer exponent whose exact parts need more than 384 bits (and more
    /// than 3072 for the long double and `_Float128` ones) because one part dominates: the
    /// directed modes round on the side of the exact value, in every format (no spec clause;
    /// D-10.17, #1720).
    #[test]
    fn cpow_integer_powers_beyond_the_exact_width() {
        use crate::float::F80;
        use crate::mp::Mp;
        let one = Mp::from_i64(1);
        let p2 = |k: i64| one.ldexp(k);
        let small = |c: i64, k: i64| Mp::from_i64(c).ldexp(k);
        // (1 + it)^2 = (1 − t²) + 2it; (1 + it)^3 = (1 − 3t²) + i(3t − t³);
        // (1 + it)^-2 = ((1 − t²) − 2it)/(1 + t²)²: the real part below 1, the imaginary part
        // above −2t; (1 + it)^-3: the real part below 1, the imaginary part above −3t;
        // (t + i)^2 = −(1 − t²) + 2it; (t + i)^3 = (t³ − 3t) + i(3t² − 1); (1 + it)^5: below 1
        // and below 5t.
        macro_rules! family {
            ($f:ty, $k:expr) => {{
                let k: i64 = $k;
                let t = p2(-k);
                let name = concat!(stringify!($f), " (1 + i·2^-k)");
                far_pow::<$f>(name, one, t, 2, (one, -1), (small(2, -k), 0));
                far_pow::<$f>(name, one, t, 3, (one, -1), (small(3, -k), -1));
                far_pow::<$f>(name, one, t, 5, (one, -1), (small(5, -k), -1));
                far_pow::<$f>(name, one, t, -2, (one, -1), (small(-2, -k), 1));
                far_pow::<$f>(name, one, t, -3, (one, -1), (small(-3, -k), 1));
                let name = concat!(stringify!($f), " (2^-k + i)");
                far_pow::<$f>(name, t, one, 2, (Mp::from_i64(-1), 1), (small(2, -k), 0));
                far_pow::<$f>(name, t, one, 3, (small(-3, -k), 1), (Mp::from_i64(-1), 1));
                // Scaled: (2^40 + i·2^(40−k))^2 = 2^80·(1 + it)^2.
                far_pow::<$f>(name, p2(40), p2(40 - k), 2, (p2(80), -1), (small(2, 80 - k), 0));
            }};
        }
        family!(f32, 100);
        family!(f64, 200);
        family!(f64, 1000);
        family!(F80, 200);
        family!(F80, 2000);
        family!(F80, 16000);
        family!(F128, 200);
        family!(F128, 2000);
        family!(F128, 16000);
    }
}
