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
//!   (`J0`: `2^-90` absolute). Below `2^-1000` `j1`, `y0`, `y1` decline (subnormal and overflowing
//!   results are the 384-bit path's); `j0` below `2^-27` is 1 moved down by `x²/4 < 2^-56`, less
//!   than a quarter of the last place below 1 (`wide::beside`).
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
//!   `eps = a·2^-74 + 2^-95·|y|`. `1/x`, `w = 1/x²` underflow only where `Q`'s terms are
//!   negligible (`|Q| < 2^-1000`).
//!
//! **Rounding.** A negative `y` is rounded as `|y|` in the mirrored mode and negated (the flags
//! the same); `eps >= |y|` (or `y = 0`) refuses at once. Every settled result is inexact and
//! normal (`|y| >= 2^-513` on the ranges above), as on the 384-bit path.
//!
//! **Fallback rate** (absolute bound `2^-74` against values of order `0.1` to `1`): about
//! `2·2^-74/ulp(y)`, `2^-17` to `2^-20` of the operands away from the zeros, all operands within
//! about `2^-21` of a zero; measured in the tests (`fallback` ceiling 1 in 10^4).

#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)] // slot indices bounded by the ranges checked before each lookup

use crate::coeffs::tables as t;
use crate::dd::{self, fast_two_sum, two_prod, two_sum, Dd};
use crate::fast64;
use crate::mp::Mp;
use crate::status::{Rounding, Status};
use crate::wide::beside;

/// 2^-5: the series below, `BESSEL_T` from here.
pub const SMALL: f64 = f64::from_bits(0x3fa0_0000_0000_0000);
/// Hankel's expansion from here.
pub const HANKEL: f64 = 32.0;
/// 2^-1000: `j1`, `y0`, `y1` decline below.
pub const TINY: f64 = f64::from_bits(0x0170_0000_0000_0000);
/// 2^-27: `j0` is just below 1 under it.
const J0_ONE: f64 = f64::from_bits(0x3e40_0000_0000_0000);
/// 2^-74, 2^-90, 2^-95, 2^-104.
const E74: f64 = f64::from_bits(0x3b50_0000_0000_0000);
const E90: f64 = f64::from_bits(0x3a50_0000_0000_0000);
const E95: f64 = f64::from_bits(0x3a00_0000_0000_0000);
const E104: f64 = f64::from_bits(0x3970_0000_0000_0000);
/// `2/π` rounded up, for the logarithm's share of the bound.
const K_UP: f64 = 0.636_62;

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
            (y, dd::abs(y.hi) * E90)
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
            let y = k.div(Dd::from(x)).neg().add(k.mul(l).mul(j1)).add(s(3).mul_f64(x));
            (y, K_UP * el * dd::abs(j1.hi) * 1.01 + dd::abs(y.hi) * E90)
        }
    })
}

/// `(J_n(x), Y_n(x), eps)` for `n` 0 or 1 and `x >= 32` by Hankel's expansion (module doc).
pub fn hankel_kernel(n: usize, x: f64) -> (Dd, Dd, f64) {
    let r = Dd::from(1.0).div(Dd::from(x));
    let w = r.sqr();
    let p = dd::horner(w, &t::HANKEL_P[n], 3);
    let q = dd::horner(w, &t::HANKEL_Q[n], 3).mul(r);
    let (s, c) = crate::trig::sin_cos(x);
    let h = Dd::of(t::SQRT_HALF_DD);
    let (sc, cc) = if n == 0 { (s.sub(c).mul(h), c.add(s).mul(h)) } else { (s.add(c).mul(h).neg(), s.sub(c).mul(h)) };
    let e = i32::try_from((x.to_bits() >> 52) & 0x7ff).unwrap_or(0) - 1023;
    let k = e >> 1;
    let m = dd::ldexp(x, -2 * k);
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

/// `y` (within `eps`) correctly rounded in `mode`, or `None`.
fn round(y: Dd, eps: f64, mode: Rounding) -> Option<(f64, Status)> {
    if !(eps < dd::abs(y.hi)) {
        return None;
    }
    if y.hi < 0.0 {
        fast64::settle(y.neg(), eps, 0, mirrored(mode)).map(|(v, st)| (-v, st))
    } else {
        fast64::settle(y, eps, 0, mode)
    }
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
    if ax < J0_ONE {
        return Some(beside::<f64>(Mp::from_i64(1), false, mode));
    }
    let (y, eps) = kernel(0, ax)?;
    round(y, eps, mode)
}

/// `j1(x)` for a finite nonzero `x`, or `None`.
pub fn j1(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    let ax = dd::abs(x);
    if ax < TINY {
        return None;
    }
    let (y, eps) = kernel(1, ax)?;
    round(if x < 0.0 { y.neg() } else { y }, eps, mode)
}

/// `y0(x)` for a finite `x > 0`, or `None`.
pub fn y0(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    if x < TINY {
        return None;
    }
    let (y, eps) = kernel(2, x)?;
    round(y, eps, mode)
}

/// `y1(x)` for a finite `x > 0`, or `None`.
pub fn y1(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    if x < TINY {
        return None;
    }
    let (y, eps) = kernel(3, x)?;
    round(y, eps, mode)
}
