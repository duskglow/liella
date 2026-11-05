//! Round-to-nearest fast paths of the `double` `atan`, `asin`, `acos`, `atan2` and `hypot`
//! (LIBM-F64-INVTRIG-HYPOT-TIER; Ziv's first step, the in-place entry `mret::elem_fast` of a
//! caller in the default MXCSR). Each `*_nearest` function returns the correctly rounded result or
//! `None` (operand outside the window below, or the rounding test failed); the caller then runs
//! the accurate path unchanged, so every result is the accurate path's (D-10.13). Where a function
//! answers, its hardware arithmetic acts on normal finite `double`s whose results stay normal (no
//! overflow, underflow, division by zero, invalid or denormal operand) and [`settle`] ends on an
//! inexact addition, so the flags raised are exactly inexact, as the entry requires.
//!
//! **Bounds** (`u = 2^-53`; "relative" is relative to the final result `y`).
//!
//! * `atan t` for `t = th + tl`, `0 < th <= 1 + 2^-52`, `|tl| <= ulp(th)`: `j = round(64 th)`,
//!   `c = j/64`, `atan t = atan c + atan v` with `v = (t − c)/(1 + t c)`, `|v| <= 2^-7 (1 + 2^-50)`.
//!   `th − c` is exact (both multiples of `ulp(th)`, difference below `2^-7`), `n = th − c + tl`
//!   by 2Sum, `c·th` exactly (`c` has at most 7 bits), so `n` exactly and `d = 1 + c t` within
//!   `2^-104` relative. The quotient `q = q1 + q2` (one correction step): within `2^-104 |v|`.
//!   `atan v = v + v^3 P(v^2)`, `P = −1/3 + z/5 − z^2/7 + z^3/9` evaluated in `double` on `q1`
//!   alone: truncation `|v|^11/11 < 2^-73.4 |v|`; dropping `q2` from the cubic term
//!   `|v|^2 |q2| < 2^-66.9 |v|`; the five roundings of `q1·P` (with `RN(1/3)` off by `2^-53.4`
//!   relative) `4.7 u · |v|^3/3 < 2^-66.4 |v|`; the final double-double sums `< 2^-104 |y|`. As
//!   `|v| <= |y|` (j = 0: `y ≈ v`; j >= 1: `|v| <= 1/128 <= atan c − |v|`) the error is below
//!   `2^-65.6 |y|`. The table `ATAN_J64` is a double-double within `2^-106` of `atan(j/64)`.
//! * `atan x`, `|x| > 1`: `π/2 − atan(1/x)`, `th = RN(1/x)`, `tl = (1 − th·x)·th` (the residual is
//!   exact, `tl` within `2^-105 t`). The subtracted value is at most `π/4`, so its error stays
//!   below `2^-65.6` relative to the result (at least `π/4`); `π/2` as a double-double adds
//!   `2^-106`.
//! * `asin x = atan(a/s)` or `π/2 − atan(s/a)` (whichever ratio is at most 1), `s = sqrt((1−a)(1+a))`
//!   with `(1 − a)(1 + a)` from two exact 2Sums and one exact product (within `2^-105`), its root
//!   by `sqrtsd` and one correction step (within `2^-104`), the ratio by [`div_dd`] (within
//!   `2^-103`). A relative error `e` of `t` moves `atan t` by at most `e` relative
//!   (`t/((1 + t^2) atan t) <= 1`).
//! * `acos x = 2 atan w` (`x > 0`) or `π − 2 atan w` (`x < 0`), `w = sqrt((1 − a)/(1 + a))`
//!   (within `2^-103`): no cancellation (`2 atan w <= π/2`).
//! * `atan2(y, x)`: the ratio `min/max` as `th = RN(n/d)` and its exact residual divided by `d`
//!   (within `2^-105`), then `r`, `π/2 − r`, `π − r` or `π/2 + r` by sign and order of the
//!   operands (each at least the subtracted `r`'s size).
//!
//!   Every `atan`-family result is within `2^-65.5 |y|`; the rounding test uses
//!   `EPS = 2^-62 |y|`, a factor 11 above it.
//! * `hypot`: `a^2 + b^2` as a double-double from two exact products and one 2Sum (within
//!   `2^-105`), the hardware root and one correction step `(s − r0^2)/(2 r0)` (within `2^-104`).
//!   The test uses `2^-98 |r|`; it also refuses when the root lies within that bound of a
//!   `double` (an exact root such as `hypot(3, 4)`, which must not raise inexact, or too close to
//!   tell): the accurate path decides those.
//!
//! **Square root.** `asin`, `acos` and `hypot` take `sqrt`, which must be the correctly rounded
//! `double` square root in round-to-nearest (the boundary passes `sqrtsd`; this crate forbids
//! `unsafe` and so cannot issue it; the tests pass `f64::sqrt`). Any correctly rounded root gives
//! the same results.
//!
//! **Two builds** (MATH-F64-FMA-PATH, D-10.25): the kernels below take the exact product as a
//! parameter, Dekker's [`dd::two_prod`] or one FMA ([`dd::two_prod_fma`]). Every product here
//! has normal operands within `2^±901` and an error term above `2^-1006`, so both return the same
//! pair bit for bit, the two builds compute the same values and take the same branches: the
//! bounds are those of either build, and the results are identical (tested).

#![allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)] // table index clamped to 0..=64; exponents of bounded range

use crate::coeffs::tables as t;
use crate::dd::{self, fast_two_sum, two_sum, Dd};

const C3: f64 = -1.0 / 3.0;
const C5: f64 = 1.0 / 5.0;
const C7: f64 = -1.0 / 7.0;
const C9: f64 = 1.0 / 9.0;
const HALF_HI: f64 = f64::from_bits(t::PI_2[0]);
const HALF_LO: f64 = f64::from_bits(t::PI_2[1]);
const PI_HI: f64 = f64::from_bits(t::PI[0]);
const PI_LO: f64 = f64::from_bits(t::PI[1]);
const SIGN: u64 = 1 << 63;
const MANT: u64 = (1 << 52) - 1;
const fn pow2_bits(e: i32) -> u64 {
    ((1023 + e) as u64) << 52
}
const TWO_M60: f64 = f64::from_bits(pow2_bits(-60));
/// `2^-62`: the relative bound of the `atan`-family rounding test (the proof: module comment).
pub const EPS: f64 = f64::from_bits(pow2_bits(-62));
/// `2^-98`: the relative bound of the `hypot` rounding test.
pub const EPS_HYPOT: f64 = f64::from_bits(pow2_bits(-98));

/// `atan(j/64)` for `jf = j` (`0 <= j <= 64`) as a double-double.
#[inline(always)]
fn table(jf: f64) -> [u64; 2] {
    t::ATAN_J64[usize::try_from(dd::to_i32(jf)).unwrap_or(0).min(64)]
}

/// `c·th` exactly for `c = j/64` (at most 7 significant bits): Dekker's product with the split
/// of `c` trivial (`c` is its own upper half).
#[inline(always)]
fn prod_small(c: f64, th: f64) -> Dd {
    let (h, l) = dd::split(th);
    let p = c * th;
    Dd { hi: p, lo: (c * h - p) + c * l }
}

/// `c + sg·r` for a double-double constant `c` and `sg = ±1` (`|r.hi| <= |ch|`).
#[inline(always)]
fn comb(ch: f64, cl: f64, sg: f64, r: Dd) -> Dd {
    let s = two_sum(ch, sg * r.hi);
    fast_two_sum(s.hi, s.lo + (cl + sg * r.lo))
}

/// The magnitude of the nearest `double` of a positive `y` whose value is within `eps` of the
/// true one, when every value within `eps` of `y` rounds to `y.hi` and `|y.lo| > floor` (`floor
/// >= 0`); `None` otherwise. The result is computed as `y.hi + y.lo`: with `0 < |y.lo|` below
/// half the gap to `y.hi`'s neighbour that sum rounds to `y.hi` and raises inexact, so an
/// answer always raises inexact (also when every earlier operation happened to be exact, as
/// for `atan(1/2)`).
#[inline(always)]
fn settle(y: Dd, eps: f64, floor: f64) -> Option<f64> {
    let bits = y.hi.to_bits();
    let ex = bits >> 52;
    if !(55..0x7fe).contains(&ex) {
        return None;
    }
    let half = f64::from_bits((ex - 53) << 52);
    let gap = if bits & MANT == 0 && y.lo < 0.0 { 0.5 * half } else { half };
    let ylo = dd::abs(y.lo);
    if gap - ylo > eps && ylo > floor {
        Some(y.hi + y.lo)
    } else {
        None
    }
}

/// The `atan`-family test with `EPS`, the sign from `sign`'s top bit.
#[inline(always)]
fn done(y: Dd, sign: u64) -> Option<f64> {
    let v = settle(y, EPS * y.hi, 0.0)?;
    Some(f64::from_bits(v.to_bits() | (sign & SIGN)))
}

// The kernels in two builds (module comment): `$prod` the exact product, `$small` the exact
// product by `j/64`, `$a` the build's attributes, `$i` the helpers' inlining (`#[inline(always)]`
// cannot be combined with `#[target_feature]`).
macro_rules! kernels {
    ($(#[$a:meta])*; $(#[$i:meta])*; $prod:path, $small:path) => {
        /// `atan(th + tl)` for `2^-60 <= th <= 1 + 2^-52`, `|tl| <= ulp(th)`: within `2^-65.6`
        /// relative (module comment).
        $(#[$a])* $(#[$i])*
        fn atan_unit(th: f64, tl: f64) -> Dd {
            let jf = dd::nearest_int(th * 64.0);
            let c = jf * (1.0 / 64.0);
            let n = two_sum(th - c, tl); // th − c exact
            let p = $small(c, th);
            let d1 = fast_two_sum(1.0, p.hi);
            let d = fast_two_sum(d1.hi, d1.lo + (p.lo + c * tl));
            let q1 = n.hi / d.hi;
            let pq = $prod(q1, d.hi);
            let rem = (((n.hi - pq.hi) - pq.lo) + n.lo) - q1 * d.lo;
            let q2 = rem / d.hi;
            let z = q1 * q1;
            let p = z * (C3 + z * (C5 + z * (C7 + z * C9)));
            let a = table(jf);
            let s = fast_two_sum(f64::from_bits(a[0]), q1);
            fast_two_sum(s.hi, s.lo + (f64::from_bits(a[1]) + (q2 + q1 * p)))
        }

        /// The quotient `n/d` of two positive double-doubles (within `2^-103` relative).
        $(#[$a])* $(#[$i])*
        fn div_dd(n: Dd, d: Dd) -> Dd {
            let q1 = n.hi / d.hi;
            let pq = $prod(q1, d.hi);
            let rem = (((n.hi - pq.hi) - pq.lo) + n.lo) - q1 * d.lo;
            fast_two_sum(q1, rem / d.hi)
        }

        /// The square root of a positive normalized double-double (within `2^-104` relative).
        $(#[$a])* $(#[$i])*
        fn sqrt_dd(x: Dd, sqrt: impl Fn(f64) -> f64) -> Dd {
            let s = sqrt(x.hi);
            let ps = $prod(s, s);
            fast_two_sum(s, (((x.hi - ps.hi) - ps.lo) + x.lo) / (2.0 * s))
        }

        /// `atan(x)` to nearest, for `2^-60 <= |x| < 2^60`.
        $(#[$a])* #[inline]
        pub fn atan_nearest(x: f64) -> Option<f64> {
            let bits = x.to_bits();
            let ab = bits & !SIGN;
            if !(pow2_bits(-60)..pow2_bits(60)).contains(&ab) {
                return None;
            }
            let a = f64::from_bits(ab);
            let y = if a <= 1.0 {
                atan_unit(a, 0.0)
            } else {
                let th = 1.0 / a;
                let p = $prod(th, a);
                let tl = ((1.0 - p.hi) - p.lo) * th;
                comb(HALF_HI, HALF_LO, -1.0, atan_unit(th, tl))
            };
            done(y, bits)
        }

        /// `asin(x)` to nearest, for `2^-60 <= |x| < 1`.
        $(#[$a])* #[inline]
        pub fn asin_nearest(x: f64, sqrt: impl Fn(f64) -> f64) -> Option<f64> {
            let bits = x.to_bits();
            let ab = bits & !SIGN;
            if !(pow2_bits(-60)..pow2_bits(0)).contains(&ab) {
                return None;
            }
            let a = f64::from_bits(ab);
            let (m1, m2) = (two_sum(1.0, -a), two_sum(1.0, a));
            let p = $prod(m1.hi, m2.hi);
            let s = sqrt_dd(fast_two_sum(p.hi, p.lo + (m1.hi * m2.lo + m1.lo * m2.hi)), sqrt);
            let av = Dd { hi: a, lo: 0.0 };
            let y = if a > s.hi {
                comb(HALF_HI, HALF_LO, -1.0, { let t = div_dd(s, av); atan_unit(t.hi, t.lo) })
            } else {
                let t = div_dd(av, s);
                atan_unit(t.hi, t.lo)
            };
            done(y, bits)
        }

        /// `acos(x)` to nearest, for `2^-60 <= |x| < 1`.
        $(#[$a])* #[inline]
        pub fn acos_nearest(x: f64, sqrt: impl Fn(f64) -> f64) -> Option<f64> {
            let bits = x.to_bits();
            let ab = bits & !SIGN;
            if !(pow2_bits(-60)..pow2_bits(0)).contains(&ab) {
                return None;
            }
            let a = f64::from_bits(ab);
            let w = sqrt_dd(div_dd(two_sum(1.0, -a), two_sum(1.0, a)), sqrt);
            let at = atan_unit(w.hi, w.lo);
            let y2 = Dd { hi: 2.0 * at.hi, lo: 2.0 * at.lo };
            done(if bits & SIGN != 0 { comb(PI_HI, PI_LO, -1.0, y2) } else { y2 }, 0)
        }

        /// `atan2(y, x)` to nearest, for `2^-400 <= |x|, |y| < 2^400` with `min/max >= 2^-60`.
        $(#[$a])* #[inline]
        pub fn atan2_nearest(y: f64, x: f64) -> Option<f64> {
            let (yb, xb) = (y.to_bits(), x.to_bits());
            let (ay, ax) = (yb & !SIGN, xb & !SIGN);
            let win = pow2_bits(-400)..pow2_bits(400);
            if !(win.contains(&ay) && win.contains(&ax)) {
                return None;
            }
            let (ay, ax) = (f64::from_bits(ay), f64::from_bits(ax));
            let swap = ay > ax;
            let (n, d) = if swap { (ax, ay) } else { (ay, ax) };
            let th = n / d;
            if !(th >= TWO_M60) {
                return None;
            }
            let p = $prod(th, d);
            let tl = ((n - p.hi) - p.lo) / d;
            let r = atan_unit(th, tl);
            let r = match (swap, xb & SIGN != 0) {
                (false, false) => r,
                (true, false) => comb(HALF_HI, HALF_LO, -1.0, r),
                (false, true) => comb(PI_HI, PI_LO, -1.0, r),
                (true, true) => comb(HALF_HI, HALF_LO, 1.0, r),
            };
            done(r, yb)
        }

        /// `hypot(x, y)` to nearest, for `2^-450 <= |x|, |y| < 2^450` within a factor `2^61` of
        /// each other, when the root is certainly not a `double`.
        $(#[$a])* #[inline]
        pub fn hypot_nearest(x: f64, y: f64, sqrt: impl Fn(f64) -> f64) -> Option<f64> {
            let (ab, bb) = (x.to_bits() & !SIGN, y.to_bits() & !SIGN);
            let win = pow2_bits(-450)..pow2_bits(450);
            if !(win.contains(&ab) && win.contains(&bb)) || (ab >> 52).abs_diff(bb >> 52) > 60 {
                return None;
            }
            let (a, b) = (f64::from_bits(ab), f64::from_bits(bb));
            let (pa, pb) = ($prod(a, a), $prod(b, b));
            let s = two_sum(pa.hi, pb.hi);
            let r = sqrt_dd(fast_two_sum(s.hi, s.lo + (pa.lo + pb.lo)), sqrt);
            let eps = EPS_HYPOT * r.hi;
            settle(r, eps, eps)
        }
    };
}

kernels!(; #[inline(always)]; dd::two_prod, prod_small);

/// The FMA build of the kernels (MATH-F64-FMA-PATH, D-10.25): the same arithmetic with every
/// exact product one FMA, so the same results bit for bit (module comment). Every function here
/// may be called only on a CPU with FMA whose AVX state the OS saves (the `#[target_feature]`
/// contract); the boundary selects it once at startup (`dispatch::fma`).
// ARCH(x86_64): the run-time FMA build (D-10.25); FMA is base ISA on aarch64, so one build there
#[cfg(target_arch = "x86_64")]
pub mod fused {
    use super::{comb, done, settle, table, two_sum, fast_two_sum, Dd, dd, pow2_bits};
    use super::{C3, C5, C7, C9, EPS_HYPOT, HALF_HI, HALF_LO, PI_HI, PI_LO, SIGN, TWO_M60};

    kernels!(#[target_feature(enable = "fma")]; #[inline]; dd::two_prod_fma, dd::two_prod_fma);
}
