//! The `double` `lgamma`/`tgamma` fast tier (Ziv's first step, LIBM-SPECIAL-GAMMA-TIER; spec
//! §10.3, D-10.13: every result correctly rounded in the caller's mode), the pattern of
//! [`crate::fast_special`]: a double-double value with a proved bound, [`fast64::settle`], and the
//! unchanged 384-bit path when the test refuses. Domain: `2^-60 <= |x| < 16`, `x` not an integer
//! `<= 0` (and for `tgamma` not an integer at all: those are exact).
//!
//! **Tables** (`crates/mathtools/src/gamma_tables.rs` derives them and proves the bounds, relative,
//! `2^-75` each): `LG_T`, `ln Γ` on `[2^-5, 16)` in the slots of `fast_special` (centre `c`,
//! `t = u − c` exact); `LG_Z1`/`LG_Z2`, `Q(t) = ln Γ(z + t)/t` at the zeros `z = 1, 2` for the
//! windows `[31/32, 17/16)` and `[31/16, 17/8)`, so that `ln Γ = t·Q(t)` keeps a relative bound
//! next to the zeros (`two_prod(Q.hi, t)` exact, then `Q.lo·t` and one addition: `2^-103`).
//!
//! **Split** ([`split`]): `ln|Γ(x)| = L − ln|D|` and `Γ(x) = e^L / D`, `|L − L*| <= eps`, `D`
//! within `2^-99` relative (`n <= 16` exact factors, one [`Dd::mul_f64`] each, `2^-104` a step):
//! * `x >= 2^-5`: `L = ln Γ(x)` from the tables, `D = 1`.
//! * `|x| < 2^-5`: `L = ln Γ(1 + x) = x·Q1(x)` (`LG_Z1` with `t = x`), `D = x`.
//! * `x` in `(−1/2, −2^-5]`: `L = ln Γ(1 + x)` at `u = RN(1 + x)`, `t = x + (1 − c)`: `1 − c` is
//!   exact (`c` has six bits), and the sum is exact (a multiple of `2^-57` below `2^-6`); `u`'s
//!   slot may be the neighbour of the true one, so `|t| <= h + 2^-54` (the generator widens `h`).
//!   At `u = 31/32`, `LG_Z1` with `t = x` exactly. `D = x`.
//! * `x <= −1/2`: `n = −⌊x⌋`, `y = x + n` in `(0, 1)` exact (a multiple of `ulp(x)` below `|x|`),
//!   `P = x·(x + 1)···(x + n − 1)` (each factor exact). `y >= 2^-5`: `L = ln Γ(y)` (`t = x + (n − c)`
//!   exact: `n − c` has at most 15 bits and the true `y − c` is a `double`), `D = P`; else
//!   `L = ln Γ(1 + y)`, `D = y·P`.
//!
//! **lgamma** = `L − ln|D|`: `ln|D| = ln|D.hi| + D.lo/D.hi` ([`fast64::log_dd`], the dropped
//! `(D.lo/D.hi)²/2 < 2^-107`), `eps' = eps + eps_log + 2^-96·(|L| + |ln D| + 1)` (the `+ 1`
//! carries `D`'s `2^-99`; the subtraction and the addition `2^-104` each). Near the zeros of
//! `lgamma` for `x < 0` the difference cancels and the rounding test refuses: those operands take
//! the 384-bit path (the fallback rate is measured in `tests/fast_gamma.rs`).
//!
//! **tgamma** = `e^L / D`: [`fast64::exp_dd`] (`|L| < 28`, `|L.lo| <= 2^-48`) within
//! `|E|·(2^-49·|tail| + 2^-73.8)`; `e^(L* − L)` adds `1.01·eps` relative; the division and `D`
//! `2^-96`. The quotient is scaled into `[1, 2)` (exact) for `settle`; `|Γ|` stays in
//! `[2^-45, 2^61]` here, normal. Signs: `D` carries `Γ`'s sign (`settle` rounds either sign).

#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)] // slot indices bounded by the ranges checked before each lookup

use crate::coeffs::tables as t;
use crate::dd::{self, fast_two_sum, two_prod, two_sum, Dd};
use crate::fast64;
use crate::status::{Rounding, Status};

/// 2^-5: `LG_T` from here.
const LO: f64 = f64::from_bits(0x3fa0_0000_0000_0000);
/// The fast tier's bound on `|x|` from above.
pub const HI: f64 = 16.0;
/// The fast tier's bound on `|x|` from below (2^-60).
pub const TINY: f64 = f64::from_bits(0x3c30_0000_0000_0000);
/// 2^-96, 2^-103, 2^-72, 2^-49 (module doc).
const E96: f64 = f64::from_bits(0x39f0_0000_0000_0000);
const E103: f64 = f64::from_bits(0x3980_0000_0000_0000);
const E72: f64 = f64::from_bits(0x3b70_0000_0000_0000);
const TAIL: f64 = f64::from_bits(0x3ce0_0000_0000_0000);

/// The Taylor row `row` (layout of `fast_special`: `m` double-doubles, degree `d`) at `t`.
#[inline(always)]
fn taylor<const W: usize>(row: &[u64; W], m: usize, d: usize, t: f64) -> Dd {
    let c = |k: usize| f64::from_bits(row[k]);
    let mut r = c(d + m);
    for k in (m..d).rev() {
        r = r * t + c(k + m);
    }
    let (mut sh, mut sl) = (r, 0.0);
    for k in (0..m).rev() {
        let p = two_prod(sh, t);
        let h = two_sum(c(2 * k), p.hi);
        sl = h.lo + (c(2 * k + 1) + (p.lo + sl * t));
        sh = h.hi;
    }
    fast_two_sum(sh, sl)
}

/// `t·Q(t)` for a zero row, with its absolute bound.
#[inline(always)]
fn zero_row<const W: usize>(row: &[u64; W], m: usize, d: usize, eps: u64, t: f64) -> (Dd, f64) {
    let q = taylor(row, m, d, t);
    let p = two_prod(q.hi, t);
    let y = fast_two_sum(p.hi, p.lo + q.lo * t);
    (y, dd::abs(y.hi) * (f64::from_bits(eps) + E103))
}

/// `ln Γ(1 + t)` for `|t| <= 1/16`.
#[inline(always)]
fn z1(t: f64) -> (Dd, f64) {
    zero_row(&t::LG_Z1[0], t::LG_Z1_DD, t::LG_Z1_DEG, t::LG_Z1_EPS, t)
}

/// `ln Γ(x + n)` with its absolute bound, `RN(x + n)` in `[2^-5, 16)` (module doc).
#[inline(always)]
fn lg_shift(x: f64, n: f64) -> (Dd, f64) {
    let u = x + n;
    if (0.968_75..1.0625).contains(&u) {
        return z1(x + (n - 1.0));
    }
    if (1.9375..2.125).contains(&u) {
        return zero_row(&t::LG_Z2[0], t::LG_Z2_DD, t::LG_Z2_DEG, t::LG_Z2_EPS, x + (n - 2.0));
    }
    let b = u.to_bits();
    let i = usize::try_from((b >> 48).wrapping_sub(0x3fa0)).unwrap_or(usize::MAX);
    let c = f64::from_bits((b & 0xffff_0000_0000_0000) | 1 << 47);
    let y = taylor(&t::LG_T[i], t::LG_T_DD, t::LG_T_DEG, x + (n - c));
    (y, dd::abs(y.hi) * f64::from_bits(t::LG_T_EPS))
}

/// Whether `x` is in the fast tier's domain (module doc; `tgamma` also excludes integers).
pub fn domain(x: f64, tgamma: bool) -> bool {
    let a = dd::abs(x);
    let int = dd::nearest_int(x) == x;
    (TINY..HI).contains(&a) && !(int && (tgamma || x < 0.0))
}

/// `(L, eps, D)` of the module doc (`None` for `D = 1`), `x` in [`domain`].
pub fn split(x: f64) -> (Dd, f64, Option<Dd>) {
    if x >= LO {
        let (l, e) = lg_shift(x, 0.0);
        return (l, e, None);
    }
    if x > -LO {
        let (l, e) = z1(x);
        return (l, e, Some(Dd::from(x)));
    }
    if x > -0.5 {
        let (l, e) = lg_shift(x, 1.0);
        return (l, e, Some(Dd::from(x)));
    }
    let k = dd::nearest_int(x);
    let n = if k > x { 1.0 - k } else { -k };
    let y = x + n;
    let mut p = Dd::from(x);
    let mut j = 1.0;
    while j < n {
        p = p.mul_f64(x + j);
        j += 1.0;
    }
    if y < LO {
        let (l, e) = z1(y);
        (l, e, Some(p.mul_f64(y)))
    } else {
        let (l, e) = lg_shift(x, n);
        (l, e, Some(p))
    }
}

/// `lgamma x` as `(y, eps)` with `|ln|Γ(x)| − y| <= eps`, and the sign of `Γ(x)`, or `None`.
pub fn lgamma_kernel(x: f64) -> Option<(Dd, f64, i32)> {
    let (l, eps, d) = split(x);
    let Some(d) = d else { return Some((l, eps, 1)) };
    let a = dd::abs(d.hi);
    if a == 1.0 {
        return None;
    }
    let (ln, le) = fast64::log_dd(a)?;
    let y = l.sub(ln.add_f64(d.lo / d.hi));
    Some((y, eps + le + E96 * (dd::abs(l.hi) + dd::abs(ln.hi) + 1.0), if d.hi < 0.0 { -1 } else { 1 }))
}

/// `lgamma x` correctly rounded in `mode` and the sign of `Γ(x)`, for `x` in [`domain`] other
/// than 1 and 2 (exact zeros), or `None` (the caller runs the accurate path).
pub fn lgamma(x: f64, mode: Rounding) -> Option<((f64, Status), i32)> {
    let (y, eps, sign) = lgamma_kernel(x)?;
    if y.hi == 0.0 {
        return None;
    }
    fast64::settle(y, eps, 0, mode).map(|r| (r, sign))
}

/// `tgamma x` as `(y, eps, m)`, `1 <= |y.hi| < 2`: `|Γ(x) − y·2^m| <= eps·2^m`, or `None`.
pub fn tgamma_kernel(x: f64) -> Option<(Dd, f64, i32)> {
    let (l, eps, d) = split(x);
    let (e, m, tail) = fast64::exp_dd(l)?;
    let r = match d {
        None => e,
        Some(d) => e.div(d),
    };
    let k = i32::try_from((r.hi.to_bits() >> 52) & 0x7ff).unwrap_or(0) - 1023;
    let r = r.scale(dd::pow2(-k));
    let rel = 1.01 * eps + TAIL * dd::abs(tail) + E72 + E96;
    Some((r, dd::abs(r.hi) * rel * 1.001, m + k))
}

/// `tgamma x` correctly rounded in `mode` for a non-integer `x` in [`domain`], or `None`.
pub fn tgamma(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    let (y, eps, m) = tgamma_kernel(x)?;
    fast64::settle(y, eps, m, mode)
}
