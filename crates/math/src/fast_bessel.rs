//! The `double` Bessel fast tier, `j0`, `j1`, `y0`, `y1` (Ziv's first step,
//! LIBM-SPECIAL-BESSEL-TIER; spec §10.3, D-10.13: every result correctly rounded in the caller's
//! mode). Each entry evaluates the function as a double-double `y` with a proved absolute bound
//! `eps` and hands both to [`fast64::settle`], which returns the correctly rounded result when
//! every value within `eps` of `y` rounds alike and `None` otherwise; the caller
//! (`crate::special::j0` and the others) then runs the 384-bit path unchanged. The accuracy
//! contract is unchanged (ruling #2760 option A). Near a zero of the function `|y|` is small
//! against `eps`, the test refuses and the 384-bit path answers.
//!
//! **Regions** (`x > 0`; `j0` is even, `j1` odd, `y0` and `y1` take `x > 0` only):
//!
//! * `x < 2^-5`: the power series in `z = x²` of `BESSEL_SMALL` (derivation in
//!   `crates/mathtools/src/bessel_tables.rs`; first omitted term below `2^-110`):
//!   `J0 = A`, `J1 = x·B`, `Y0 = (2/π)·ln x·A + C`, `Y1 = −(2/π)/x + (2/π)·ln x·J1 + x·D`.
//!   `z` is exact (`two_prod`); `dd::horner` with `A`..`D`'s first three terms in double-double
//!   and the rest in `f64` (`|c_k z^k| < 2^-41` from `k = 3`: below `2^-92` relative). No sum
//!   cancels (`A ≈ 1`, `|Y0| > 2.2`, `|Y1| > 20` dominated by its first term), so the roundings of
//!   the dozen double-double operations (each within `2^-100` relative) stay below
//!   `2^-90·|y|`; the logarithm adds `(2/π)·eps_ln·|A|` (`Y0`) or `(2/π)·eps_ln·|J1|` (`Y1`)
//!   with `fast64::log_dd`'s proved `eps_ln`. Bound: `eps = 2^-90·|y| + 0.64·eps_ln·|A or J1|`
//!   (`J0`: `2^-90` absolute), plus an absolute `2^-1068` for `J1` (where `|y|·2^-90` and the
//!   roundings of the last operations fall below `2^-1022`, their errors are absolute: at most
//!   `2^-1075` each).
//! * `x < 2^-27` (the tiny band, D-10.110; D-10.106's leading series terms in double-double):
//!   `j0` is 1 moved down by `x²/4 < 2^-56`, under a quarter of the last place below 1, and `j1`
//!   is `x/2` moved toward 0 by less than `2^-56·|x/2|`: both read off the bits, `j1` below
//!   `2^-1021` (subnormal results) through `float::round` from `(2m − 1)·2^-1076` and a sticky
//!   bit. `Y0 = (2/π)·ln x + C_0 + z·(C_1 − (2/π)·ln x/4)` and
//!   `Y1 = −(2/π)/x + x·((2/π)·ln x/2 + D_0)` (the series above to first order in `z`; dropped
//!   terms below `2^-98` relative), `ln x` by `fast64::log_dd` (a subnormal `x` scaled by 2^128
//!   first), `Y1` carried as `Y1·2^-128` and settled with that scale; below `Y1_OVERFLOW` every
//!   mode overflows (`elem::overflow`).
//! * `2^-5 <= x < 32`: `BESSEL_T`, 16 slots a binade, Taylor rows at the slot centre `c`
//!   (`t = x − c` exact, `|t| <= 2^e/32`), evaluated as `fast_special`'s rows are (`f64` Horner
//!   from `p_D` to `p_M`, then `M` double-double steps, then Fast2Sum); each row's last entry is
//!   its proved absolute bound (truncation, Cauchy tail and the evaluation's roundings:
//!   `bessel_tables.rs`), at most `BESSEL_T_EPS_MAX` (`2^-74`). `eps = row bound + 2^-104·|y|`.
//! * `x >= 32`: Hankel's expansion, `J = a·(P·cos χ − Q·sin χ)`, `Y = a·(P·sin χ + Q·cos χ)`,
//!   `a = √(2/(πx))`, `χ = x − (2n+1)π/4`. `P`, `Q` keep `HANKEL_N` terms each; their
//!   remainders are below the first omitted terms (DLMF 10.17(iii)), below `2^-77` from 32 on.
//!   `cos χ`, `sin χ` come from `sin x`, `cos x` (`trig::sin_cos`, within `2^-90` absolute:
//!   reduction by `π/64` within `2^-100`, its double-double series and table within `2^-100`)
//!   as `(c + s)·√½`, `(s − c)·√½` (`n = 0`) and `(s − c)·√½`, `−(s + c)·√½` (`n = 1`). With
//!   `|P| <= 1`, `|Q| < 2^-7.9` and double-double roundings of `2^-100`, the bracket is within
//!   `2·2^-77 + 2^-88 < 2^-75.5` absolute. `a` is `√((2/π)/m)·2^-k` with `x = m·4^k`,
//!   `1 <= m < 4` (no subnormal intermediate for any `x`), within `2^-98` relative. Bound:
//!   `eps = a·2^-74 + 2^-95·|y|`. `1/x` is `(1/m)·4^-k` (`x` never enters a product: Dekker's
//!   split overflows above 2^996); `r.lo` and `w = 1/x²` underflow only where `Q`'s terms are
//!   negligible (`|Q| < 2^-900`, absolute errors below 2^-1074).
//!
//! **Rounding.** A negative `y` is rounded as `|y|` in the mirrored mode and negated (the flags
//! the same); `eps >= |y|` (or `y = 0`) refuses at once. Every settled result is inexact and
//! normal (`|y| >= 2^-513` on the ranges above), as on the 384-bit path.
//!
//! **Fallback rate** (absolute bound `2^-74` against values of order `0.1` to `1`): about
//! `2·2^-74/ulp(y)`, `2^-17` to `2^-20` of the operands away from the zeros, all operands within
//! about `2^-21` of a zero; measured in the tests (`fallback` ceiling 1 in 10^4).

#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)] // slot indices bounded by the ranges checked before each lookup

use core::cmp::Ordering;

use crate::coeffs::tables as t;
use crate::dd::{self, fast_two_sum, two_prod, two_sum, Dd};
use crate::fast64;
use crate::status::{Flags, Rounding, Status};

/// 2^-5: the series below, `BESSEL_T` from here.
pub const SMALL: f64 = f64::from_bits(0x3fa0_0000_0000_0000);
/// Hankel's expansion from here.
pub const HANKEL: f64 = 32.0;
/// 2^-1000: `j1`, `y0`, `y1` decline below.
pub const TINY: f64 = f64::from_bits(0x0170_0000_0000_0000);
/// 2^-27: `j0` is just below 1 and `j1(x)` just inside `x/2` under it.
pub const NEAR_SERIES: f64 = f64::from_bits(0x3e40_0000_0000_0000);
/// 2^-1021: `x/2` is normal from here (`j1`'s tiny range).
const J1_HALF: f64 = f64::from_bits(0x0020_0000_0000_0000);
/// 2^-1068: an absolute floor on the series' bound, above the dozen absolute roundings
/// (`2^-1075` each) of results below `2^-1022` and the underflow of `|y|·2^-90` itself.
const FLOOR: f64 = f64::from_bits(0x0000_0000_0000_0040);
/// 2^-74, 2^-90, 2^-95, 2^-104.
const E74: f64 = f64::from_bits(0x3b50_0000_0000_0000);
const E90: f64 = f64::from_bits(0x3a50_0000_0000_0000);
const E95: f64 = f64::from_bits(0x3a00_0000_0000_0000);
const E104: f64 = f64::from_bits(0x3970_0000_0000_0000);
/// 2^64: `small_kernel`'s `Y1` scales `x` up by it and the quotient `(2/π)/x` back.
const S64: f64 = f64::from_bits(0x43f0_0000_0000_0000);
/// `2/π` rounded up (by more than its last place), for the logarithm's share of the bound.
const K_UP: f64 = core::f64::consts::FRAC_2_PI * 1.0001;
/// 2^128 and 2^-128: the tiny band's exact scalings (subnormal operands, `Y1` near overflow).
const S128: f64 = f64::from_bits(0x47f0_0000_0000_0000);
const S128_INV: f64 = f64::from_bits(0x37f0_0000_0000_0000);
/// `ln 2` as a double-double (within 2^-107).
const LN2_DD: [u64; 2] = [0x3fe6_2e42_fefa_39ef, 0x3c7a_bc9e_3b39_803f];
/// `⌊(2/π)·2^50·(1 − 2^-40)⌋·2^-1074`: below it `|Y1| > (2/π)/x > 2^1024·(1 + 2^-40)`, an
/// overflow in every mode.
const Y1_OVERFLOW: f64 = f64::from_bits(716_770_142_402_180);

const M: usize = t::BESSEL_T_DD;
const D: usize = t::BESSEL_T_DEG;
const W: usize = M + D + 2;

/// The Taylor row `row` at `t` (layout and order in the module doc).
#[inline(always)]
fn taylor(row: &[u64; W], t: f64) -> Dd {
    let c = |k: usize| f64::from_bits(row[k]);
    let mut r = c(D + M);
    for k in (M..D).rev() {
        r = r * t + c(k + M);
    }
    let (mut sh, mut sl) = (r, 0.0);
    for k in (0..M).rev() {
        let p = two_prod(sh, t);
        let h = two_sum(c(2 * k), p.hi);
        sl = h.lo + (c(2 * k + 1) + (p.lo + sl * t));
        sh = h.hi;
    }
    fast_two_sum(sh, sl)
}

/// `f(x)` (`f`: 0 `J0`, 1 `J1`, 2 `Y0`, 3 `Y1`) for `2^-5 <= x < 32` as `(y, eps)`.
pub fn table_kernel(f: usize, x: f64) -> (Dd, f64) {
    let b = x.to_bits();
    let i = usize::try_from((b >> 48).wrapping_sub(0x3fa0)).unwrap_or(usize::MAX);
    let c = f64::from_bits((b & 0xffff_0000_0000_0000) | 1 << 47);
    let row = &t::BESSEL_T[f][i];
    let y = taylor(row, x - c);
    (y, f64::from_bits(row[W - 1]) + dd::abs(y.hi) * E104)
}

/// `f(x)` for `2^-1000 <= x < 2^-5` as `(y, eps)`, or `None` when the logarithm declines.
pub fn small_kernel(f: usize, x: f64) -> Option<(Dd, f64)> {
    let z = two_prod(x, x);
    let s = |i: usize| dd::horner(z, &t::BESSEL_SMALL[i], 3);
    let k = Dd::of(t::TWO_OVER_PI_DD);
    Some(match f {
        0 => (s(0), E90),
        1 => {
            let y = s(1).mul_f64(x);
            (y, dd::abs(y.hi) * E90 + FLOOR)
        }
        2 => {
            let (l, el) = fast64::log_dd(x)?;
            let a = s(0);
            let y = k.mul(l).mul(a).add(s(2));
            (y, K_UP * el * dd::abs(a.hi) * 1.01 + dd::abs(y.hi) * E90)
        }
        _ => {
            let (l, el) = fast64::log_dd(x)?;
            let j1 = s(1).mul_f64(x);
            // `(2/π)/x` as `((2/π)/(x·2^64))·2^64` (exact scalings): a quotient above 2^996
            // would overflow Dekker's split inside the division.
            let y = k.div(Dd::from(x * S64)).scale(S64).neg().add(k.mul(l).mul(j1)).add(s(3).mul_f64(x));
            (y, K_UP * el * dd::abs(j1.hi) * 1.01 + dd::abs(y.hi) * E90)
        }
    })
}

/// `(J_n(x), Y_n(x), eps)` for `n` 0 or 1 and `x >= 32` by Hankel's expansion (module doc).
pub fn hankel_kernel(n: usize, x: f64) -> (Dd, Dd, f64) {
    let e = i32::try_from((x.to_bits() >> 52) & 0x7ff).unwrap_or(0) - 1023;
    let k = e >> 1;
    let m = dd::ldexp(x, -2 * k);
    // `1/x` as `(1/m)·4^-k`: Dekker's split of an operand above 2^996 overflows, so `x` itself
    // never enters a product (the scaling is exact; a subnormal `r.lo` errs by under 2^-1074).
    let r = Dd::from(1.0).div(Dd::from(m)).scale(dd::pow2(-2 * k));
    let w = r.sqr();
    let p = dd::horner(w, &t::HANKEL_P[n], 3);
    let q = dd::horner(w, &t::HANKEL_Q[n], 3).mul(r);
    let (s, c) = crate::trig::sin_cos(x);
    let h = Dd::of(t::SQRT_HALF_DD);
    let (sc, cc) = if n == 0 { (s.sub(c).mul(h), c.add(s).mul(h)) } else { (s.add(c).mul(h).neg(), s.sub(c).mul(h)) };
    let a = dd::sqrt(Dd::of(t::TWO_OVER_PI_DD).div(Dd::from(m))).scale(dd::pow2(-k));
    let j = a.mul(p.mul(cc).sub(q.mul(sc)));
    let y = a.mul(p.mul(sc).add(q.mul(cc)));
    (j, y, a.hi * E74)
}

/// `mode` with up and down swapped.
fn mirrored(mode: Rounding) -> Rounding {
    match mode {
        Rounding::Up => Rounding::Down,
        Rounding::Down => Rounding::Up,
        other => other,
    }
}

/// `y·2^m` (`y` within `eps`) correctly rounded in `mode`, or `None`.
fn round(y: Dd, eps: f64, m: i32, mode: Rounding) -> Option<(f64, Status)> {
    if eps.partial_cmp(&dd::abs(y.hi)) != Some(Ordering::Less) {
        return None;
    }
    if y.hi < 0.0 {
        fast64::settle(y.neg(), eps, m, mirrored(mode)).map(|(v, st)| (-v, st))
    } else {
        fast64::settle(y, eps, m, mode)
    }
}

/// `v` (positive, normal) moved toward 0 by less than a quarter of its last place, signed by
/// `neg`: `v` or its neighbour toward 0, inexact (`J0` below 1, `J1` inside `x/2`).
fn below(v: f64, neg: bool, mode: Rounding) -> (f64, Status) {
    let inward = match mode {
        Rounding::Zero => true,
        Rounding::Down => !neg,
        Rounding::Up => neg,
        Rounding::Nearest => false,
    };
    let r = if inward { f64::from_bits(v.to_bits() - 1) } else { v };
    (if neg { -r } else { r }, Status::flags(Flags::INEXACT))
}

/// `j1(x)` for `0 < |x| < 2^-1021`: `|x|/2 = m·2^-1075` (`m` the bits of `|x|`, its significand
/// at the scale 2^-1074 on both sides of 2^-1022) moved toward 0 by far less than 2^-1076, so
/// `|J1|` lies strictly between `(2m − 1)·2^-1076` and `2m·2^-1076`: rounded from there by
/// [`crate::float::round`] (subnormal results with underflow, `ERANGE` only for 0).
fn j1_subnormal(x: f64, mode: Rounding) -> (f64, Status) {
    let m = u128::from(dd::abs(x).to_bits());
    let e = crate::float::Exact { neg: x < 0.0, sig: 2 * m - 1, exp: -1076, sticky: true };
    let (r, flags) = crate::float::round::<f64>(e, mode);
    (r, crate::float::range_errno(r, Status::flags(flags)))
}

/// `ln x` as `(y, eps)` for `0 < x < 2^-27`: [`fast64::log_dd`], a subnormal `x` scaled by 2^128
/// first (exact) and `128·ln 2` taken off (`|y| > 18`: the subtraction's rounding and
/// `LN2_DD`'s `128·2^-107` stay below `2^-95·|y|`).
fn ln_tiny(x: f64) -> Option<(Dd, f64)> {
    if x >= f64::MIN_POSITIVE {
        return fast64::log_dd(x);
    }
    let (l, e) = fast64::log_dd(x * S128)?;
    let y = l.sub(Dd::of(LN2_DD).scale(128.0));
    Some((y, e + dd::abs(y.hi) * E95))
}

/// `Y0` for `0 < x < 2^-27` as `(y, eps)`: `(2/π)·ln x·A(z) + C(z)` to first order in `z = x²`,
/// `(2/π)·ln x + C_0 + z·(C_1 − (2/π)·ln x/4)` (`z < 2^-54`; the dropped terms are below
/// `z²·(|ln x|/64 + 1) < 2^-98` against `|Y0| > 11`, the correction's roundings below 2^-100).
fn y0_tiny(x: f64) -> Option<(Dd, f64)> {
    let (l, el) = ln_tiny(x)?;
    let c = &t::BESSEL_SMALL[2];
    let kl = Dd::of(t::TWO_OVER_PI_DD).mul(l);
    let corr = x * x * (f64::from_bits(c[1][0]) - 0.25 * kl.hi);
    let y = kl.add(Dd::of(c[0])).add(Dd::from(corr));
    Some((y, K_UP * el * 1.01 + dd::abs(y.hi) * E90))
}

/// `Y1·2^-128` for `Y1_OVERFLOW <= x < 2^-27` as `(y, eps)`: `−(2/π)/x + x·((2/π)·ln x/2 + D_0)`,
/// the first-order form of `−(2/π)/x + (2/π)·ln x·J1 + x·D(z)` (the next terms below `2^-100`
/// relative: `|Y1| > 2^26`); the quotient as `(2/π)/(x·2^128)` (exact scaling, below 2^946,
/// inside Dekker's range), the correction scaled by 2^-128 (absolute errors of 2^-1075 where it
/// is subnormal, against `|y| > 2^-102`).
fn y1_tiny(x: f64) -> Option<(Dd, f64)> {
    let (l, el) = ln_tiny(x)?;
    let k = Dd::of(t::TWO_OVER_PI_DD);
    let d0 = f64::from_bits(t::BESSEL_SMALL[3][0][0]);
    let xs = x * S128_INV;
    let y = k.div(Dd::from(x * S128)).neg().add(Dd::from(xs * (0.5 * k.hi * l.hi + d0)));
    Some((y, K_UP * el * xs + dd::abs(y.hi) * E90))
}

/// `f(x)` for `x >= 2^-1000` (`f` as in [`table_kernel`]) as `(y, eps)`.
pub fn kernel(f: usize, x: f64) -> Option<(Dd, f64)> {
    if x < SMALL {
        return small_kernel(f, x);
    }
    if x < HANKEL {
        return Some(table_kernel(f, x));
    }
    let (j, y, e) = hankel_kernel(f & 1, x);
    let v = if f < 2 { j } else { y };
    Some((v, e + dd::abs(v.hi) * E95))
}

/// `j0(x)` for a finite nonzero `x`, correctly rounded in `mode`, or `None`.
pub fn j0(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    let ax = dd::abs(x);
    if ax < NEAR_SERIES {
        return Some(below(1.0, false, mode));
    }
    let (y, eps) = kernel(0, ax)?;
    round(y, eps, 0, mode)
}

/// `j1(x)` for a finite nonzero `x`, or `None`.
pub fn j1(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    let ax = dd::abs(x);
    if ax < NEAR_SERIES {
        // `J1(x) = (x/2)·(1 − x²/8 + ...)`: `x/2` (exact and normal from 2^-1021) moved toward 0
        // by less than `2^-56` of itself, under a quarter of its last place.
        // At 2^-1021 the neighbour of `x/2 = 2^-1022` toward 0 is subnormal: rounded with underflow.
        return Some(if ax <= J1_HALF { j1_subnormal(x, mode) } else { below(ax * 0.5, x < 0.0, mode) });
    }
    let (y, eps) = kernel(1, ax)?;
    round(if x < 0.0 { y.neg() } else { y }, eps, 0, mode)
}

/// `y0(x)` for a finite `x > 0`, or `None`.
pub fn y0(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    let (y, eps) = if x < NEAR_SERIES { y0_tiny(x)? } else { kernel(2, x)? };
    round(y, eps, 0, mode)
}

/// `y1(x)` for a finite `x > 0`, or `None`.
pub fn y1(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    if x < NEAR_SERIES {
        if x < Y1_OVERFLOW {
            return Some(crate::elem::overflow::<f64>(true, mode));
        }
        let (y, eps) = y1_tiny(x)?;
        return round(y, eps, 128, mode);
    }
    let (y, eps) = kernel(3, x)?;
    round(y, eps, 0, mode)
}

/// `jn`, `yn` and their `float` forms for orders `|n| >= 2` (LIBM-F64-BESSEL-N).
#[path = "fast_bessel_n.rs"]
pub mod n;
