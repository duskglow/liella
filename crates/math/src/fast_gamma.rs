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

// The bands outside [`domain`] (LIBM-RETHINK-GAMMA, D-10.105): closed forms instead of the
// 384-bit path. Series: lnΓ(1 + x) = −γx + Σ_{k≥2} (−1)^k ζ(k) x^k / k and its exponential
// (DLMF 5.7.3, 5.7.1); Stirling's series (DLMF 5.11.1) with its remainder bound for real x > 0
// (DLMF 5.11.ii: smaller than the first omitted term). Every result goes through
// [`fast64::settle`] (Ziv's rounding test, A. Ziv, ACM TOMS 17(3), 1991).

/// Euler's γ as a double-double.
const EULER: Dd = Dd { hi: f64::from_bits(0x3fe2_788c_fc6f_b619), lo: f64::from_bits(0xbc56_cb90_701f_bfab) };
/// ln(2π)/2 and 1/12 as double-doubles.
const HALF_LN_2PI: Dd = Dd { hi: f64::from_bits(0x3fed_67f1_c864_beb5), lo: f64::from_bits(0xbc86_5b5a_1b7f_f5df) };
const TWELFTH: Dd = Dd { hi: f64::from_bits(0x3fb5_5555_5555_5555), lo: f64::from_bits(0x3c55_5555_5555_5555) };
/// Stirling's c_k = B_2k / (2k(2k − 1)), k = 2..10 (the doubles nearest the rationals).
const STIRLING: [u64; 9] = [
    0xbf66_c16c_16c1_6c17, 0x3f4a_01a0_1a01_a01a, 0xbf43_8138_1381_3814, 0x3f4b_951e_2b18_ff23, 0xbf5f_6ab0_d999_3c7d,
    0x3f7a_41a4_1a41_a41a, 0xbf9e_4286_cb0f_5398, 0x3fc6_fe96_381e_0680, 0xbff6_4767_0118_1f3a,
];
/// 2^-968 (γx stays normal above it), 2^-110, 2^-70, 2^-512, 2^512, 2^1012, 2^1015.
const G_MIN: f64 = f64::from_bits(0x0370_0000_0000_0000);
const E110: f64 = f64::from_bits(0x3910_0000_0000_0000);
const E70: f64 = f64::from_bits(0x3b90_0000_0000_0000);
const SC_LO: f64 = f64::from_bits(0x1ff0_0000_0000_0000);
const SC_UP: f64 = f64::from_bits(0x5ff0_0000_0000_0000);
/// Stirling's band end (2^1012): `y < 2^1022` below it.
pub const X_BIG: f64 = f64::from_bits(0x7f30_0000_0000_0000);
/// 2^1015: from here `lnΓ(x) > DBL_MAX` (the threshold is about 2.56e305 < 2^1015).
pub const X_OVER: f64 = f64::from_bits(0x7f60_0000_0000_0000);

/// `lgamma x` for `0 < |x| < 2^-60` as `(y, eps, sign)`: `lnΓ(x) = −ln|x| − γx + R`,
/// `|R| <= ζ(2)x²/2 < 2^-120` (the series' terms alternate and decrease). `γx` is formed for
/// `|x| >= 2^-968` only (below, `|γx| < 2^-968` joins the bound): its rounding `< 2^-113`, `γ.lo·x`
/// `< 2^-116`. The difference rounds within `2^-96·|y|` (`|y| >= 41.5`). `Γ(x)` has `x`'s sign.
pub fn lgamma_tiny(x: f64) -> Option<(Dd, f64, i32)> {
    let a = dd::abs(x);
    if !(a > 0.0 && a < TINY) {
        return None;
    }
    let (ln, le) = fast64::log_dd(a)?;
    let g = if a >= G_MIN { -EULER.hi * x } else { 0.0 };
    let y = Dd::from(g).sub(ln);
    Some((y, le + E96 * dd::abs(ln.hi) + E110, if x < 0.0 { -1 } else { 1 }))
}

/// `tgamma x` for `2^-1022 <= |x| < 2^-60` as `(y, eps, m)` for [`fast64::settle`]:
/// `Γ(x) = 1/x − γ + R`, `|R| <= 0.99|x| + |x|² < 2^-59.9` (DLMF 5.7.1). With `s = 2^512` when
/// `|x| < 2^-512` (else 1; exact scaling), `q = RN(1/(x·s))`, `|q| <= 2^510`, and the residual
/// `r = 1 − q·x·s` is exact (`two_prod`, then two exact subtractions: `r` is representable, the
/// classical division residual). So `Γ(x)/s = q + r/(x·s) − γ/s + R/s`: `lo = RN(r/(xs) − γ.hi/s)`
/// is within `2^-52·(|r/(xs)| + |lo|)`, and `γ.lo + R < 2^-56` (over `s`). Directed results with
/// `r = 0` (`x` a power of two) are decided by `lo`'s sign only while it clears `settle`'s slack.
pub fn tgamma_tiny(x: f64) -> Option<(Dd, f64, i32)> {
    let a = dd::abs(x);
    if !(f64::MIN_POSITIVE..TINY).contains(&a) {
        return None;
    }
    let (xs, s, k) = if a < SC_LO { (x * SC_UP, SC_LO, 512) } else { (x, 1.0, 0) };
    let q = 1.0 / xs;
    let p = two_prod(q, xs);
    let r = (1.0 - p.hi) - p.lo;
    let rx = r / xs;
    let lo = rx - EULER.hi * s;
    let y = fast_two_sum(q, lo);
    let eps = f64::EPSILON * (dd::abs(rx) + dd::abs(lo)) + s * f64::from_bits(0x3c70_0000_0000_0000);
    let e = i32::try_from((y.hi.to_bits() >> 52) & 0x7ff).unwrap_or(0) - 1023;
    let sc = dd::pow2(-e);
    Some((y.scale(sc), eps * sc, e + k))
}

/// `lgamma x` for `16 <= x < X_BIG` as `(y, eps)`: Stirling, `lnΓ(x) = x(ln x − 1) − ln(x)/2 +
/// ln(2π)/2 + S`, `S = Σ_{k=1}^{10} c_k x^{1−2k}` within `|c_11|·x^-21 < 2^-80` of the series'
/// value. `ln x` by [`fast64::log_dd`] (`le`, times `x` in the product); `(ln x − 1)·x` and the
/// sums `2^-96·(|p| + 2)`; `1/(12x)` as a double-double (`u = 1/x` with its exact residual,
/// `2^-104` relative, `<= 2^-110`); the rest of `S` in `double`, `|rest| <= 6.8e-7`, within
/// `2^-72`. `y >= 27.9`, below `2^1022` for `x < 2^1012`.
pub fn lgamma_large(x: f64) -> Option<(Dd, f64)> {
    if !(HI..X_BIG).contains(&x) {
        return None;
    }
    let (ln, le) = fast64::log_dd(x)?;
    // Dekker's split overflows above 2^996: the product runs on x·2^-512 (exact), scaled back.
    let (xm, back) = if x >= SC_UP { (x * SC_LO, SC_UP) } else { (x, 1.0) };
    let p = ln.add_f64(-1.0).mul_f64(xm).scale(back);
    let uh = 1.0 / x;
    let s = if x < 67_108_864.0 {
        let pr = two_prod(uh, x);
        let ul = ((1.0 - pr.hi) - pr.lo) / x;
        let w = uh * uh;
        let c = |k: usize| f64::from_bits(STIRLING[k]);
        let mut t = c(8);
        for k in (0..8).rev() {
            t = t * w + c(k);
        }
        let s1 = two_prod(TWELFTH.hi, uh);
        fast_two_sum(s1.hi, s1.lo + (TWELFTH.hi * ul + TWELFTH.lo * uh) + uh * w * t)
    } else {
        // x >= 2^26: 1/(12x) in `double` (< 2^-83 off) and the rest (< 2^-86) join E70.
        Dd::from(TWELFTH.hi * uh)
    };
    let y = p.sub(ln.scale(0.5)).add(HALF_LN_2PI).add(s);
    Some((y, le * x + E96 * (dd::abs(p.hi) + 2.0) + E70))
}

/// `lgamma` on the bands above, correctly rounded, with `Γ(x)`'s sign, or `None` (the caller runs
/// the accurate path): `|x| < 2^-60`, and `x >= 16` (overflow from `X_OVER`).
pub fn lgamma_band(x: f64, mode: Rounding) -> Option<((f64, Status), i32)> {
    if x >= X_OVER {
        return (x < f64::INFINITY).then(|| (crate::elem::overflow::<f64>(false, mode), 1));
    }
    let (y, eps, sign) = if x >= HI { lgamma_large(x).map(|(y, e)| (y, e, 1))? } else { lgamma_tiny(x)? };
    fast64::settle(y, eps, 0, mode).map(|r| (r, sign))
}

/// `tgamma` for `2^-1022 <= |x| < 2^-60`, correctly rounded, or `None`.
pub fn tgamma_band(x: f64, mode: Rounding) -> Option<(f64, Status)> {
    let (y, eps, m) = tgamma_tiny(x)?;
    fast64::settle(y, eps, m, mode)
}
