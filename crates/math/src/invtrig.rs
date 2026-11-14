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
//!   exact, `tl` within `2^-105 t`), computed as `(π/2 − atan c) − atan v` with the constant
//!   folded into the table (`HALF_MINUS`, within `2^-104`): the subtracted `atan v` is at most
//!   `2^-7`, below the entry (at least `π/4`), so the two final double-double sums are as above
//!   and the evaluation error of `atan v` stays below `2^-65.6` relative to the result.
//! * `asin x = atan(a/s)` or `π/2 − atan(s/a)` (whichever ratio is at most 1), `s = sqrt((1−a)(1+a))`
//!   with `(1 − a)(1 + a)` from two exact 2Sums and one exact product (within `2^-105`), its root
//!   by `sqrtsd` and one correction step (within `2^-104`), the ratio by [`div_dd`] (within
//!   `2^-103`). A relative error `e` of `t` moves `atan t` by at most `e` relative
//!   (`t/((1 + t^2) atan t) <= 1`).
//! * `acos x = 2 atan w` (`x > 0`) or `π − 2 atan w` (`x < 0`), `w = sqrt((1 − a)/(1 + a))`
//!   (within `2^-103`): no cancellation (`2 atan w <= π/2`).
//! * `atan2(y, x)`: the ratio `min/max` as `th = RN(n/d)` and its exact residual divided by `d`
//!   (within `2^-105`), then `r`, `π/2 − r`, `π − r` or `π/2 + r` by sign and order of the
//!   operands (each at least the subtracted `r`'s size), the constant folded into the table as
//!   for `atan` (`HALF_MINUS`, `PI_MINUS`, `HALF_PLUS`).
//!
//!   Every `atan`-family result is within `2^-65.5 |y|`; the rounding test uses
//!   `EPS = 2^-62 |y|`, a factor 11 above it.
//! * `hypot`: `a^2 + b^2` as a double-double from two exact products and one 2Sum (within
//!   `2^-105`), the hardware root and one correction step `(s − r0^2)/(2 r0)` (within `2^-104`).
//!   The test uses `2^-98 |r|`; it also refuses when the root lies within that bound of a
//!   `double` (an exact root such as `hypot(3, 4)`, which must not raise inexact, or too close to
//!   tell): the accurate path decides those.
//!
//! **Quick phase** (D-10.69, LIBM-F64-INVTRIG-QUICK). The `atan` family first evaluates
//! `atan_quick` on the same reduced argument `(th, tl, tab, sg)` and the same rounding test
//! (`EPS`); only when that test refuses does today's kernel `atan_unit` run (tier 2, unchanged),
//! then the accurate path. Method (table-driven reduction after Tang, ACM TOMS 15(2), 1989;
//! Muller, Elementary Functions, 3rd ed. 2016, ch. 6): with `c = j/64`, `h = th − c` (exact),
//! `1 + c t = (1 + c^2)(1 + g)`, `g = c (h + tl)/(1 + c^2)`, `|g| <= 2^-8`, so
//! `v = (h + tl)·R/(1 + g)` with `R = 1/(1 + c^2)` from the table `RECIP` (`rh + rl` within
//! `2^-78`) and `1/(1 + g) − 1` by its series to `g^7` (truncation `g^8`): no division.
//! `h·rh` is an exact pair (one FMA, or two masked partial products with `rh` cut to 26 bits).
//! Error terms, relative to the result `y` (each `j`; `|g|·|v|/|y| <= 2^-13` because
//! `|g| <= c |h| R`, `|v| <= |h| R` and `|y| >= atan c − 2^-7`): the roundings of `g` (`2^-51.4`
//! relative to `g`) and of `s` (`2^-52`) give `< 2^-64.4`; `rl`'s `2^-78`, the truncation
//! `g^8 < 2^-64` and the dropped `tl·rl`, `e·s` terms below `2^-70` (`h·rl` is `2^-26 |v|`, so
//! it is kept inside the `s` product with `tl·rh`); `vl`'s and the
//! final sums' roundings (`u·|vl + tail|` with `|vl| <= |g||v| + 2^-51 |v|`) `< 2^-65`; the
//! cubic `v^3 P(v^2)` as in `atan_unit` (truncation `2^-73.4`, evaluation `2^-65.6` times
//! `|v|^2 <= 2^-14`), evaluated on `RN(v.hi + vl)` (`vl` carries `−g·v`, up to `2^-8 |v|`, so
//! it cannot be dropped there; the one rounding adds `3u` relative to the cubic, `< 2^-65`). Total below `2^-63 |y|`, a factor 2 under the
//! test's `EPS`; so an answer of the quick phase is the correctly rounded result. Both builds
//! compute the same pair `h·rh` (exact either way) and so the same values.
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
use crate::hwop::HwOps;
use crate::status::{Flags, MathError, Rounding, Status};

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
#[allow(clippy::as_conversions, clippy::cast_sign_loss)] // e in -60..=450: the biased exponent is positive
const fn pow2_bits(e: i32) -> u64 {
    ((1023 + e) as u64) << 52
}
const TWO_M60: f64 = f64::from_bits(pow2_bits(-60));
/// `1.5·2^52`: adding it rounds a value in `[0, 2^51)` to an integer (round-to-nearest).
const ROUNDER: f64 = 6_755_399_441_055_744.0;
/// `2^-62`: the relative bound of the `atan`-family rounding test (the proof: module comment).
pub const EPS: f64 = f64::from_bits(pow2_bits(-62));
/// `2^-98`: the relative bound of the `hypot` rounding test.
pub const EPS_HYPOT: f64 = f64::from_bits(pow2_bits(-98));
/// The bits of `+inf`: a magnitude at or above it is an infinity or a NaN.
const INF_BITS: u64 = 0x7ff << 52;
/// Exponent-field gap from which `hypot` lies strictly inside `(a, a + ulp(a)/2)` (D-10.104):
/// `r − a < b^2/(2a) < 2^(2Eb+1−Ea) <= 2^(Ea−53)` when `Ea − Eb >= 27` (unbiased; `Eb = −1023` for a subnormal).
pub const HYPOT_GAP: u64 = 27;
/// `2^-60` as a step of the exponent field: for `a` with field `Ea > 60`, `a·2^-60` is `a`'s bits
/// minus it (exact, normal, below `ulp(a)/128`), so `a + a·2^-60` rounds to `a` in the default
/// environment and raises inexact only (the fast entry's gap exit; `1 + 2^-60` is no `double`).
const M60_FIELD: u64 = 60 << 52;
/// Ziv's rounding test factor (ACM TOMS 17(3), 1991): `yh + yl·(1 + 2^-40) == yh` leaves a
/// margin `> 2^-94.1 yh` to the rounding boundary on `yl`'s side, above the root's `2^-104`.
const ZIV_HYPOT: f64 = 1.0 + f64::from_bits(pow2_bits(-40));

/// A table of 65 double-doubles indexed by `j`, `0 <= j <= 64`.
type Table = [[u64; 2]; 65];

/// `ch + cl + s·atan(j/64)` for `s = ±1` (each entry at least `π/4`; within `2^-104` of the
/// value relative, from `ATAN_J64`'s `2^-106` and the constant's `2^-107`: `hi + err` is
/// `ch + s·ah` exactly, one rounding in `lo`, the last step exact).
const fn shifted(a: &Table, ch: f64, cl: f64, s: f64) -> Table {
    let mut o = [[0u64; 2]; 65];
    let mut j = 0;
    while j < 65 {
        let (ah, al) = (s * f64::from_bits(a[j][0]), s * f64::from_bits(a[j][1]));
        let hi = ch + ah;
        let bb = hi - ch;
        let err = (ch - (hi - bb)) + (ah - bb);
        let lo = err + (cl + al);
        let h = hi + lo;
        o[j] = [h.to_bits(), (lo - (h - hi)).to_bits()];
        j += 1;
    }
    o
}

/// `π/2 − atan(j/64)`, `π − atan(j/64)` and `π/2 + atan(j/64)`: the constant of `atan` for
/// `|x| > 1` and of the `atan2` quadrants folded into the table add (no separate subtraction).
static HALF_MINUS: Table = shifted(&t::ATAN_J64, HALF_HI, HALF_LO, -1.0);
static PI_MINUS: Table = shifted(&t::ATAN_J64, PI_HI, PI_LO, -1.0);
static HALF_PLUS: Table = shifted(&t::ATAN_J64, HALF_HI, HALF_LO, 1.0);

/// `1/(1 + c^2)` for `c = j/64` as `[rh, rl]` (D-10.69): `rh` its nearest `double` cut to 26
/// significant bits (so `h·rh` is exact in two masked partial products, [`prod_short`]), `rl`
/// the rest rounded once. `d = 1 + c^2` has at most 14 bits, so `d·rh` (40 bits) and
/// `1 − d·rh` (below `2^-25`, Sterbenz) are exact: `rh + rl` is within `2^-78` of `1/d`.
const fn recip_table() -> [[f64; 2]; 65] {
    let mut o = [[0.0f64; 2]; 65];
    let mut j = 0;
    let mut c = 0.0f64;
    while j < 65 {
        let d = 1.0 + c * c;
        let rh = f64::from_bits((1.0 / d).to_bits() & !((1u64 << 27) - 1));
        o[j] = [rh, (1.0 - d * rh) / d];
        j += 1;
        c += 1.0 / 64.0;
    }
    o
}
static RECIP: [[f64; 2]; 65] = recip_table();

/// `h·rh` as an exact pair for `rh` with at most 26 significant bits (a [`RECIP`] high part):
/// `h = hh + hl` by a mask (`hh` 27 bits, `hl` at most 26), both partial products exact; `hh·rh − p`
/// is exact (within `2^-26 |p|` of each other on a common grid) and the last sum is the exact,
/// representable error of `p`. Bit for bit the FMA build's `two_prod_fma(h, rh)`.
#[inline(always)]
fn prod_short(h: f64, rh: f64) -> Dd {
    let p = h * rh;
    let hh = f64::from_bits(h.to_bits() & !((1u64 << 26) - 1));
    Dd { hi: p, lo: (hh * rh - p) + (h - hh) * rh }
}

/// `j = round(64 th)` as a `double` and as the index (`0 <= th <= 1 + 2^-52`, so `0 <= j <= 64`):
/// `m = 64 th + 1.5·2^52` holds `j` in its low bits, read directly instead of converting `j`.
#[inline(always)]
fn index(th: f64) -> (f64, usize) {
    let m = th * 64.0 + ROUNDER;
    (m - ROUNDER, usize::try_from(m.to_bits() & 0x7f).unwrap_or(0).min(64))
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
    ($(#[$a:meta])*; $(#[$i:meta])*; $prod:path, $small:path, $short:path) => {
        /// The quick phase (D-10.69): `tab[j] + sg·atan v` for the same arguments as
        /// [`atan_unit`], with no division: `1 + c t = (1 + c^2)(1 + g)`, `g = c (h + tl) R`,
        /// so `v = (h + tl)·R·(1 + s)`, `s = −g + g^2 (1 − g)(1 + g^2 + g^4)` (the series of
        /// `1/(1 + g) − 1` to `g^7`). Within `2^-63` relative (module comment, "Quick phase").
        $(#[$a])* $(#[$i])*
        fn atan_quick(th: f64, tl: f64, tab: &Table, sg: f64) -> Dd {
            let (jf, j) = index(th);
            let c = jf * (1.0 / 64.0);
            let h = th - c; // exact
            let [rh, rl] = RECIP[j];
            let g = (c * (h + tl)) * (rh + rl);
            let v = $short(h, rh);
            let w = h * rl + tl * rh; // `rl` is `2^-26 R`: it takes part in the `s` product
            let g2 = g * g;
            let s = g2 * (((g2 * g2 + g2) + 1.0) * (1.0 - g)) - g;
            let vl = (v.hi + w) * s + (v.lo + w);
            let vt = v.hi + vl; // `vl` holds `−g·v`, up to `2^-8 v`: the cubic needs it
            let z = vt * vt;
            let tail = vt * z * (C3 + z * (C5 + z * (C7 + z * C9)));
            let a = tab[j];
            let y = fast_two_sum(f64::from_bits(a[0]), sg * v.hi);
            fast_two_sum(y.hi, y.lo + (f64::from_bits(a[1]) + sg * (vl + tail)))
        }

        /// The quick phase's answer, else today's kernel ([`atan_unit`], tier 2) on the same
        /// reduced argument; `post` maps either sum to the result's magnitude.
        $(#[$a])* $(#[$i])*
        fn two_tier(th: f64, tl: f64, tab: &Table, sg: f64, sign: u64, post: impl Fn(Dd) -> Dd) -> Option<f64> {
            done(post(atan_quick(th, tl, tab, sg)), sign).or_else(|| done(post(atan_unit(th, tl, tab, sg)), sign))
        }

        /// The quick phase alone on `atan x`, `2^-60 <= |x| <= 1` (no tier 2): for the tests,
        /// which show it answers nearly every call by itself.
        ///
        /// # Safety
        ///
        /// The FMA build (`fused`) only: the CPU must support FMA and the OS must save the AVX state.
        #[doc(hidden)]
        $(#[$a])* #[inline]
        pub fn atan_quick_only(x: f64) -> Option<f64> {
            let bits = x.to_bits();
            let ab = bits & !SIGN;
            if !(pow2_bits(-60)..=1.0f64.to_bits()).contains(&ab) {
                return None;
            }
            done(atan_quick(f64::from_bits(ab), 0.0, &t::ATAN_J64, 1.0), bits)
        }

        /// `tab[j] + sg·atan v` (`sg = ±1`, `tab` = `ATAN_J64` or a [`shifted`] table), that is
        /// `atan(th + tl)`, or a constant plus or minus it, for `2^-60 <= th <= 1 + 2^-52`,
        /// `|tl| <= ulp(th)`: within `2^-65.6` relative (module comment).
        $(#[$a])* $(#[$i])*
        fn atan_unit(th: f64, tl: f64, tab: &Table, sg: f64) -> Dd {
            let (jf, j) = index(th);
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
            let a = tab[j];
            let s = fast_two_sum(f64::from_bits(a[0]), sg * q1);
            fast_two_sum(s.hi, s.lo + (f64::from_bits(a[1]) + sg * (q2 + q1 * p)))
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
        ///
        /// # Safety
        ///
        /// The FMA build (`fused`) only: the CPU must support FMA and the OS must save the AVX state.
        $(#[$a])* #[inline]
        pub fn atan_nearest(x: f64) -> Option<f64> {
            let bits = x.to_bits();
            let ab = bits & !SIGN;
            if !(pow2_bits(-60)..pow2_bits(60)).contains(&ab) {
                return None;
            }
            let a = f64::from_bits(ab);
            if a <= 1.0 {
                two_tier(a, 0.0, &t::ATAN_J64, 1.0, bits, |y| y)
            } else {
                let th = 1.0 / a;
                let p = $prod(th, a);
                let tl = ((1.0 - p.hi) - p.lo) * th;
                two_tier(th, tl, &HALF_MINUS, -1.0, bits, |y| y)
            }
        }

        /// `asin(x)` to nearest, for `2^-60 <= |x| < 1`.
        ///
        /// # Safety
        ///
        /// The FMA build (`fused`) only: the CPU must support FMA and the OS must save the AVX state.
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
            if a > s.hi {
                let t = div_dd(s, av);
                two_tier(t.hi, t.lo, &HALF_MINUS, -1.0, bits, |y| y)
            } else {
                let t = div_dd(av, s);
                two_tier(t.hi, t.lo, &t::ATAN_J64, 1.0, bits, |y| y)
            }
        }

        /// `acos(x)` to nearest, for `2^-60 <= |x| < 1`.
        ///
        /// # Safety
        ///
        /// The FMA build (`fused`) only: the CPU must support FMA and the OS must save the AVX state.
        $(#[$a])* #[inline]
        pub fn acos_nearest(x: f64, sqrt: impl Fn(f64) -> f64) -> Option<f64> {
            let bits = x.to_bits();
            let ab = bits & !SIGN;
            if !(pow2_bits(-60)..pow2_bits(0)).contains(&ab) {
                return None;
            }
            let a = f64::from_bits(ab);
            let w = sqrt_dd(div_dd(two_sum(1.0, -a), two_sum(1.0, a)), sqrt);
            let neg = bits & SIGN != 0;
            two_tier(w.hi, w.lo, &t::ATAN_J64, 1.0, 0, |at| {
                let y2 = Dd { hi: 2.0 * at.hi, lo: 2.0 * at.lo };
                if neg { comb(PI_HI, PI_LO, -1.0, y2) } else { y2 }
            })
        }

        /// `atan2(y, x)` to nearest, for `2^-400 <= |x|, |y| < 2^400` with `min/max >= 2^-60`.
        ///
        /// # Safety
        ///
        /// The FMA build (`fused`) only: the CPU must support FMA and the OS must save the AVX state.
        $(#[$a])* #[inline]
        pub fn atan2_nearest(y: f64, x: f64) -> Option<f64> {
            let (th, tl, tab, sg) = atan2_reduce(y, x)?;
            two_tier(th, tl, tab, sg, y.to_bits(), |r| r)
        }

        /// `atan2(|y|, x)` as a double-double within `2^-65.5` relative (module comment, tier 2
        /// [`atan_unit`]), for `2^-400 <= |x|, |y| < 2^400` with `min/max >= 2^-60`, else `None`:
        /// the argument of the `clog` quick tier (`crate::complex_fast`, LIBM-CPLX-LOG-SQRT).
        $(#[$a])* $(#[$i])*
        #[allow(dead_code)] // the clog tier calls the plain build only; the FMA build's copy is unused
        pub(crate) fn atan2_abs(y: f64, x: f64) -> Option<Dd> {
            let (th, tl, tab, sg) = atan2_reduce(y, x)?;
            Some(atan_unit(th, tl, tab, sg))
        }

        /// The reduction shared by [`atan2_nearest`] and [`atan2_abs`]: `atan2(|y|, x)` =
        /// `tab[j] + sg·atan(th + tl)`, or `None` outside their window.
        $(#[$a])* $(#[$i])*
        fn atan2_reduce(y: f64, x: f64) -> Option<(f64, f64, &'static Table, f64)> {
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
            if th < TWO_M60 {
                return None;
            }
            let p = $prod(th, d);
            let tl = ((n - p.hi) - p.lo) / d;
            let (tab, sg): (&'static Table, f64) = match (swap, xb & SIGN != 0) {
                (false, false) => (&t::ATAN_J64, 1.0),
                (true, false) => (&HALF_MINUS, -1.0),
                (false, true) => (&PI_MINUS, -1.0),
                (true, true) => (&HALF_PLUS, 1.0),
            };
            Some((th, tl, tab, sg))
        }

        /// `sqrt(a^2 + b^2)` for normal `a >= b` with `a` in `[1, 4)`, `b >= 2^-77` (D-10.104):
        /// two exact products, an exact Fast2Sum (`a^2 >= b^2`), the root of the double-double
        /// with one correction step; within `2^-104` relative (module comment), normalized.
        $(#[$a])* $(#[$i])*
        fn hypot_root(a: f64, b: f64, sqrt: impl Fn(f64) -> f64) -> Dd {
            let (pa, pb) = ($prod(a, a), $prod(b, b));
            let s = fast_two_sum(pa.hi, pb.hi);
            sqrt_dd(fast_two_sum(s.hi, s.lo + (pa.lo + pb.lo)), sqrt)
        }

        /// `hypot(x, y)` to nearest on the default MXCSR (D-10.104), or `None`: a NaN or an
        /// infinity, a subnormal smaller operand within the gap, the top binade (the result may
        /// overflow), and every root within `2^-98` of a `double` or of a midpoint (an exact
        /// root, a tie, too close to tell; `hypot_rounded` settles those exactly). A zero
        /// operand gives the other's magnitude with no operation; a gap of 27 or more binades
        /// gives `a + a·2^-60`, which rounds to `a` and raises inexact only (`None` for an
        /// exponent field of 60 or less, where `a·2^-60` is not normal); otherwise the
        /// root of the operands scaled by `2^(1023 − Ea)` (exact) is tested by Ziv's rounding
        /// test and scaled back (exact). An answered root is not a `double`, so an operation
        /// was inexact in hardware; no other flag can rise (every operand and term is normal).
        ///
        /// # Safety
        ///
        /// The FMA build (`fused`) only: the CPU must support FMA and the OS must save the AVX state.
        $(#[$a])* #[inline]
        pub fn hypot_nearest(x: f64, y: f64, sqrt: impl Fn(f64) -> f64) -> Option<f64> {
            let (ab, bb) = (x.to_bits() & !SIGN, y.to_bits() & !SIGN);
            let (hi, lo) = if ab >= bb { (ab, bb) } else { (bb, ab) };
            if hi >= INF_BITS || lo == 0 {
                return if hi >= INF_BITS { None } else { Some(f64::from_bits(hi)) };
            }
            let (ea, eb) = (hi >> 52, lo >> 52);
            let a = f64::from_bits(hi);
            if ea >= eb + HYPOT_GAP {
                return if ea > 60 { Some(a + f64::from_bits(hi - M60_FIELD)) } else { None };
            }
            if eb == 0 || ea == 0x7fe {
                return None;
            }
            let scale = f64::from_bits((0x7fe - ea) << 52);
            let r = hypot_root(a * scale, f64::from_bits(lo) * scale, sqrt);
            if dd::abs(r.lo) > EPS_HYPOT * r.hi && r.hi + r.lo * ZIV_HYPOT == r.hi {
                return Some(r.hi * f64::from_bits(ea << 52));
            }
            None
        }

        /// `hypot(x, y)` correctly rounded in `mode`, with its flags and `errno` (D-10.104); `None`
        /// only for a NaN or an infinity. The arithmetic runs in round-to-nearest (the default
        /// environment). Every finite pair: a zero operand, the both-subnormal integer root, the gap
        /// exit (`a`, or its successor upward), else the scaled root with Ziv's test (`2^-98`) and,
        /// within it, the exact integer sign of `a^2 + b^2 − m^2` at `yh` or at the midpoint (a tie
        /// goes to even); scaled back, an overflow is `inf` (nearest, upward; `ERANGE`) or `DBL_MAX`.
        /// The first root is `O::sqrt` ([`HwOps`]): the boundary passes its `sqrtsd`, the tests both.
        /// The FMA build (`fused`, LIBM-HYPOT-DIRECTED-FAST) gives the same result bit for bit: its
        /// exact products are FMAs (module comment).
        ///
        /// # Safety
        ///
        /// The FMA build (`fused`) only: the CPU must support FMA and the OS must save the AVX state.
        $(#[$a])*
        pub fn hypot_rounded<O: HwOps>(x: f64, y: f64, mode: Rounding) -> Option<(f64, Status)> {
            use core::cmp::Ordering::{Equal, Greater, Less};
            let (ab, bb) = (x.to_bits() & !SIGN, y.to_bits() & !SIGN);
            let (hi, lo) = if ab >= bb { (ab, bb) } else { (bb, ab) };
            if hi >= INF_BITS {
                return None;
            }
            if lo == 0 {
                return Some((f64::from_bits(hi), Status::OK));
            }
            let (ea, eb) = (hi >> 52, lo >> 52);
            if ea == 0 {
                return Some(hypot_tiny::<O>(hi, lo, mode));
            }
            let up = mode == Rounding::Up;
            if ea >= eb + HYPOT_GAP {
                return Some(hypot_finish(hi + u64::from(up), 1.0, false, mode));
            }
            let k = ea.min(0x7fd);
            let scale = f64::from_bits((0x7fe - k) << 52);
            let (a, b) = (f64::from_bits(hi) * scale, f64::from_bits(lo) * scale);
            let r = hypot_root(a, b, O::sqrt);
            let (yb, eta, t) = (r.hi.to_bits(), EPS_HYPOT * r.hi, dd::abs(r.lo));
            let (ym, ye) = parts(r.hi);
            let at_y = |_: ()| if t > eta { if r.lo > 0.0 { Greater } else { Less } } else { sq_cmp(a, b, ym, ye) };
            let (q, exact) = if mode == Rounding::Nearest {
                let half = f64::from_bits((yb & !MANT) - (53 << 52));
                let pow2_below = yb & MANT == 0 && r.lo < 0.0;
                let gap = if pow2_below { 0.5 * half } else { half };
                if t <= eta {
                    (yb, sq_cmp(a, b, ym, ye) == Equal)
                } else if gap - t > eta {
                    (yb, false)
                } else {
                    let (mm, me, other) = if r.lo > 0.0 {
                        (2 * ym + 1, ye - 1, yb + 1)
                    } else if pow2_below {
                        (4 * ym - 1, ye - 2, yb - 1)
                    } else {
                        (2 * ym - 1, ye - 1, yb - 1)
                    };
                    let q = match (sq_cmp(a, b, mm, me), r.lo > 0.0) {
                        (Equal, _) => if yb & 1 == 0 { yb } else { other },
                        (Greater, true) | (Less, false) => other,
                        _ => yb,
                    };
                    (q, false)
                }
            } else {
                match (at_y(()), up) {
                    (Equal, _) => (yb, true),
                    (Greater, true) => (yb + 1, false),
                    (Less, false) => (yb - 1, false),
                    _ => (yb, false),
                }
            };
            Some(hypot_finish(q, f64::from_bits(k << 52), exact, mode))
        }
    };
}

kernels!(; #[inline(always)]; dd::two_prod, prod_small, prod_short);

// The first root of `hypot_rounded` and `hypot_tiny` is `O::sqrt` ([`HwOps`]: within `2^-50`
// relative; the boundary's `sqrtsd` in the default environment is correctly rounded, within
// `2^-53`). `sqrt_dd` with a first root within `d <= 2^-50` stays within
// `d^2/2 + 6·2^-53 d + 2^-105 < 2^-99.5` relative, under the test's `2^-98`.

/// The significand and exponent field of a normal `double`: `v = M·2^(E − 1075)`.
fn parts(v: f64) -> (u128, i32) {
    let b = v.to_bits();
    (u128::from((b & MANT) | (1 << 52)), i32::from(u16::try_from(b >> 52).unwrap_or(0)))
}

/// The sign of `a^2 + b^2 − m^2` for normal `a`, `b` and `m = mm·2^(me − 1075)`, exactly, in
/// wrapping 128-bit integers (D-10.104): every term is scaled to the smallest exponent; a shift
/// of 128 or more is `0` modulo `2^128`. Exact because the true difference is below `2^127` in
/// those units wherever it is asked (a root within `2^-97` of `m`, `b >= 2^-28 a` near a
/// midpoint; `m = a` and the difference `b^2` otherwise).
#[allow(clippy::as_conversions, clippy::cast_possible_wrap, clippy::cast_sign_loss)] // a two's-complement reading
fn sq_cmp(a: f64, b: f64, mm: u128, me: i32) -> core::cmp::Ordering {
    let ((am, ae), (bm, be)) = (parts(a), parts(b));
    let base = ae.min(be).min(me);
    let sh = |m: u128, e: i32| (m * m).checked_shl(2 * (e - base) as u32).unwrap_or(0);
    let d = sh(am, ae).wrapping_add(sh(bm, be)).wrapping_sub(sh(mm, me)) as i128;
    d.cmp(&0)
}

/// `hypot` of two subnormal magnitudes `A·2^-1074`, `B·2^-1074` (bits `hi >= lo > 0`): the root
/// in units of `2^-1074` is `sqrt(N)`, `N = A^2 + B^2 < 2^105`; `q = floor(sqrt(N))` from the
/// first root `O::sqrt` (within a few units of it) fixed by exact squares; the grid is the integers below
/// `2^53`, so the result's bits are the rounded integer. Underflow is detected after rounding
/// (x86): tiny unless the root rounded to 53 bits reaches `2^52` (D-10.104).
#[allow(clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::cast_sign_loss)] // n < 2^105, its root < 2^53
fn hypot_tiny<O: HwOps>(hi: u64, lo: u64, mode: Rounding) -> (f64, Status) {
    let n = u128::from(hi) * u128::from(hi) + u128::from(lo) * u128::from(lo);
    let mut q = O::sqrt(n as f64) as u64; // RANGE: the root of 2 <= n < 2^105 is in [1, 2^53)
    while u128::from(q) * u128::from(q) > n {
        q -= 1;
    }
    while u128::from(q + 1) * u128::from(q + 1) <= n {
        q += 1;
    }
    let sq = u128::from(q) * u128::from(q);
    if sq == n {
        return (f64::from_bits(q), Status::OK);
    }
    let r = match mode {
        Rounding::Nearest => q + u64::from(n > sq + u128::from(q)),
        Rounding::Up => q + 1,
        Rounding::Down | Rounding::Zero => q,
    };
    let edge = 1u128 << 104;
    let tiny = r < 1 << 52
        || (r == 1 << 52
            && match mode {
                Rounding::Nearest => n <= edge - (1 << 51),
                Rounding::Up => n <= edge - (1 << 52),
                Rounding::Down | Rounding::Zero => false,
            });
    let flags = if tiny { Flags::INEXACT.union(Flags::UNDERFLOW) } else { Flags::INEXACT };
    (f64::from_bits(r), Status::flags(flags))
}

/// A scaled result `q` (bits) times `unscale` (exact unless it overflows), with its flags.
fn hypot_finish(q: u64, unscale: f64, exact: bool, mode: Rounding) -> (f64, Status) {
    let v = f64::from_bits(q) * unscale;
    if v.is_infinite() {
        let range = Status::flags(Flags::OVERFLOW.union(Flags::INEXACT));
        return match mode {
            Rounding::Nearest | Rounding::Up => (v, range.with(MathError::Range)),
            Rounding::Down | Rounding::Zero => (f64::MAX, range),
        };
    }
    (v, if exact { Status::OK } else { Status::flags(Flags::INEXACT) })
}

/// The FMA build of the kernels (MATH-F64-FMA-PATH, D-10.25): the same arithmetic with every
/// exact product one FMA, so the same results bit for bit (module comment). Every function here
/// may be called only on a CPU with FMA whose AVX state the OS saves (the `#[target_feature]`
/// contract); the boundary selects it once at startup (`dispatch::fma`).
// ARCH(x86_64): the run-time FMA build (D-10.25); FMA is base ISA on aarch64, so one build there
#[cfg(target_arch = "x86_64")]
pub mod fused {
    use super::{comb, done, index, two_sum, fast_two_sum, t, Dd, Table, dd, pow2_bits};
    use super::{HYPOT_GAP, INF_BITS, M60_FIELD, ZIV_HYPOT};
    use super::{hypot_finish, hypot_tiny, parts, sq_cmp, HwOps, Rounding, Status, MANT};
    use super::{C3, C5, C7, C9, EPS_HYPOT, HALF_MINUS, HALF_PLUS, PI_HI, PI_LO, PI_MINUS, RECIP, SIGN, TWO_M60};

    kernels!(#[target_feature(enable = "fma")]; #[inline]; dd::two_prod_fma, dd::two_prod_fma, dd::two_prod_fma);
}
