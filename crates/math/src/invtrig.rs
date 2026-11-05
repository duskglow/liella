//! Round-to-nearest fast paths of `asin`, `acos`, `atan`, `atan2` (`float` and `double`) and of
//! the `double` `hypot` (PERF-LIBM-INVTRIG), for the in-place entry of a caller in the default
//! MXCSR. Each `*_nearest` function returns the correctly rounded result or `None` (operand outside
//! the domain below, or the rounding test failed); the caller then runs the accurate path. Where a
//! function answers, its hardware arithmetic acts on normal finite `double`s only (no overflow,
//! underflow, division by zero, invalid or denormal operand) and the result is inexact, so the
//! only flag it raises is inexact, as the entry requires.
//!
//! * `atan t` for `0 < t <= 1`: `j = round(64 t)`, `atan t = atan(j/64) + atan(u)`, `u = (t - j/64) /
//!   (1 + t j/64)`, `|u| <= 1/128 (1 + 2^-52)`, then `atan u = u + u^3 (-1/3 + u^2/5 - u^4/7 + u^6/9)`
//!   (the next term is below `2^-73 u`). `t - j/64` is exact (both are multiples of `ulp(t)`, the
//!   difference is below `2^-7`). Larger arguments, and `asin`, `acos`, `atan2` go through
//!   `atan` of a ratio in `(0, 1]` and `pi/2 - atan`, `pi - atan` (no cancellation: the
//!   subtracted `atan` is at most `pi/4`).
//! * `float` (one `double` evaluation, `atan_unit32`): `u`, the polynomial and the sum round to
//!   `double` a handful of times: relative error `<= 2^-51.2` for `atan t`. The ratio, `1 - a^2` as
//!   `(1 - a)(1 + a)` (three roundings), the square root and the final subtraction add at most
//!   `7.4 u` (`u = 2^-53`): the result `y` has `|y - f| <= 2^-50 |f|`, which is at most 8.1 units
//!   of the last place of `y`; `DELTA = 24` keeps a factor 3 (the margin of [`fast32::round`]).
//! * `double` (`atan_unit64`): the reduction and quotient in double-double (`~2^-100`), the series
//!   tail in `double` (its rounding errors and the truncation are below `2^-66 |u|`), the table
//!   `ATAN_J64` accurate to `2^-106`. The composition with `pi/2`, `pi` adds `2^-100`. The rounding
//!   test uses `EPS = 2^-62 |y|`, a factor 16 above the proved `2^-66`: every value within `EPS`
//!   of `y` rounds to `y.hi` when `y.lo` is more than `EPS` from the midpoint.
//! * `hypot`: `a^2 + b^2` exactly as a double-double (error `< 2^-104`), the hardware root, and one
//!   double-double correction `(s - r0^2)/(2 r0)` (error `< 2^-104`); inexact is certain when the
//!   correction exceeds the bound `2^-98 |r|`, otherwise (an exact root, or too close to tell) the
//!   accurate path decides.

use crate::coeffs::tables as t;
use crate::dd::{self, fast_two_sum, two_prod, two_sum, Dd};
use crate::fast32;
use crate::status::Rounding;

const C3: f64 = -1.0 / 3.0;
const C5: f64 = 1.0 / 5.0;
const C7: f64 = -1.0 / 7.0;
const C9: f64 = 1.0 / 9.0;
const HALF_HI: f64 = f64::from_bits(t::PI_2[0]);
const HALF_LO: f64 = f64::from_bits(t::PI_2[1]);
const PI_HI: f64 = f64::from_bits(t::PI[0]);
const PI_LO: f64 = f64::from_bits(t::PI[1]);
const SIGN: u64 = 1 << 63;
const fn pow2_bits(e: i32) -> u64 {
    ((1023 + e) as u64) << 52
}
const TWO_M60: f64 = f64::from_bits(pow2_bits(-60));
const F32_DELTA: u64 = 24;
/// `2^-62`: the relative bound of the `double` rounding test.
const EPS: f64 = f64::from_bits(pow2_bits(-62));
/// `2^-98`: the bound for `hypot`.
const EPS_HYPOT: f64 = f64::from_bits(pow2_bits(-98));

/// `sqrt` by the hardware instruction (a correctly rounded square root, one `double` rounding).
#[inline(always)]
fn sqrt_hw(x: f64) -> f64 {
    // ARCH(x86_64): sqrtsd; other targets use the double-double square root's leading word
    #[cfg(target_arch = "x86_64")]
    {
        use core::arch::x86_64::{_mm_cvtsd_f64, _mm_set_sd, _mm_sqrt_sd};
        _mm_cvtsd_f64(_mm_sqrt_sd(_mm_set_sd(x), _mm_set_sd(x)))
    }
    #[cfg(not(target_arch = "x86_64"))]
    {
        dd::sqrt(Dd::from(x)).hi
    }
}

#[inline(always)]
fn table(jf: f64) -> [u64; 2] {
    t::ATAN_J64[usize::try_from(dd::to_i32(jf)).unwrap_or(0).min(64)]
}

#[inline(always)]
fn with_sign(y: f64, bits: u64) -> f64 {
    f64::from_bits(y.to_bits() | (bits & SIGN))
}

// ---------------------------------------------------------------- float

/// `atan t` for `2^-60 <= t <= 1 + 2^-52` to a relative `2^-51.2`.
#[inline(always)]
fn atan_unit32(t: f64) -> f64 {
    let jf = dd::nearest_int(t * 64.0);
    let c = jf * (1.0 / 64.0);
    let u = (t - c) / (1.0 + t * c);
    let z = u * u;
    let p = z * (C3 + z * (C5 + z * C7));
    f64::from_bits(table(jf)[0]) + (u + u * p)
}

#[inline(always)]
fn finish32(y: f64) -> Option<f32> {
    fast32::round(y, F32_DELTA, Rounding::Nearest)
}

/// `float` bits of a normal, finite value in `[2^-60, hi)`: the magnitude as a `double`.
#[inline(always)]
fn mag32(x: f32, lo: u32, hi: u32) -> Option<f64> {
    let a = x.to_bits() & 0x7fff_ffff;
    (lo..hi).contains(&a).then(|| f64::from(f32::from_bits(a)))
}

const F32_2M60: u32 = (127 - 60) << 23;
const F32_2P60: u32 = (127 + 60) << 23;
const F32_ONE: u32 = 127 << 23;
const F32_MIN_NORMAL: u32 = 1 << 23;

/// `atanf(x)` to nearest, for `2^-60 <= |x| < 2^60`.
#[inline(always)]
pub fn atan32_nearest(x: f32) -> Option<f32> {
    let a = mag32(x, F32_2M60, F32_2P60)?;
    let y = if a <= 1.0 { atan_unit32(a) } else { HALF_HI - atan_unit32(1.0 / a) };
    finish32(with_sign(y, u64::from(x.to_bits()) << 32))
}

/// `asinf(x)` to nearest, for `2^-60 <= |x| < 1`.
#[inline(always)]
pub fn asin32_nearest(x: f32) -> Option<f32> {
    let a = mag32(x, F32_2M60, F32_ONE)?;
    let s = sqrt_hw((1.0 - a) * (1.0 + a));
    let y = if a <= s { atan_unit32(a / s) } else { HALF_HI - atan_unit32(s / a) };
    finish32(with_sign(y, u64::from(x.to_bits()) << 32))
}

/// `acosf(x)` to nearest, for a normal `|x| < 1`.
#[inline(always)]
pub fn acos32_nearest(x: f32) -> Option<f32> {
    let a = mag32(x, F32_MIN_NORMAL, F32_ONE)?;
    let w = sqrt_hw((1.0 - a) / (1.0 + a));
    let r = 2.0 * atan_unit32(w);
    finish32(if x.is_sign_negative() { PI_HI - r } else { r })
}

/// `atan2f(y, x)` to nearest, for normal finite `x`, `y` with `|min/max| >= 2^-60`.
#[inline(always)]
pub fn atan2_32_nearest(y: f32, x: f32) -> Option<f32> {
    let (ay, ax) = (mag32(y, F32_MIN_NORMAL, 0x7f80_0000)?, mag32(x, F32_MIN_NORMAL, 0x7f80_0000)?);
    let swap = ay > ax;
    let t = if swap { ax / ay } else { ay / ax };
    if !(t >= TWO_M60) {
        return None;
    }
    let mut r = atan_unit32(t);
    if swap {
        r = HALF_HI - r;
    }
    if x.is_sign_negative() {
        r = PI_HI - r;
    }
    finish32(with_sign(r, u64::from(y.to_bits()) << 32))
}

// ---------------------------------------------------------------- double

/// `atan(th + tl)` for `2^-60 <= th <= 1 + 2^-52`, `|tl| <= ulp(th)/2`: relative error `< 2^-66`.
#[inline(always)]
fn atan_unit64(th: f64, tl: f64) -> Dd {
    let jf = dd::nearest_int(th * 64.0);
    let c = jf * (1.0 / 64.0);
    let n = two_sum(th - c, tl);
    // c·th exactly (c has at most 7 significant bits): Dekker with the split of c trivial
    let (thh, thl) = dd::split(th);
    let ph = c * th;
    let pl = (c * thh - ph) + c * thl;
    let d1 = fast_two_sum(1.0, ph);
    let d = fast_two_sum(d1.hi, d1.lo + (pl + c * tl));
    let q1 = n.hi / d.hi;
    let pq = two_prod(q1, d.hi);
    let rem = (((n.hi - pq.hi) - pq.lo) + n.lo) - q1 * d.lo;
    let q2 = rem / d.hi;
    let z = q1 * q1;
    let p = z * (C3 + z * (C5 + z * (C7 + z * C9)));
    let a = table(jf);
    let s = fast_two_sum(f64::from_bits(a[0]), q1);
    fast_two_sum(s.hi, s.lo + (f64::from_bits(a[1]) + (q2 + q1 * p)))
}

/// The quotient `n/d` of two positive double-doubles (error `< 2^-100`).
#[inline(always)]
fn div_dd(n: Dd, d: Dd) -> Dd {
    let q1 = n.hi / d.hi;
    let pq = two_prod(q1, d.hi);
    let rem = (((n.hi - pq.hi) - pq.lo) + n.lo) - q1 * d.lo;
    fast_two_sum(q1, rem / d.hi)
}

/// The square root of a positive double-double (error `< 2^-104`).
#[inline(always)]
fn sqrt_dd(x: Dd) -> Dd {
    let s = sqrt_hw(x.hi);
    let ps = two_prod(s, s);
    fast_two_sum(s, (((x.hi - ps.hi) - ps.lo) + x.lo) / (2.0 * s))
}

/// `c + sg·r` for a double-double constant `c` and `sg = ±1`.
#[inline(always)]
fn comb(ch: f64, cl: f64, sg: f64, r: Dd) -> Dd {
    let s = two_sum(ch, sg * r.hi);
    fast_two_sum(s.hi, s.lo + (cl + sg * r.lo))
}

/// The nearest `double` of `y` when every value within `EPS·|y|` of it rounds alike, and `y.lo`
/// is more than the bound from zero when `inexact` is wanted.
#[inline(always)]
fn settle(y: Dd, eps: f64, bits_sign: u64, need_inexact: bool) -> Option<f64> {
    let bits = y.hi.to_bits();
    let ex = (bits >> 52) & 0x7ff;
    if !(55..0x7fe).contains(&ex) {
        return None;
    }
    let half = f64::from_bits((ex - 53) << 52);
    let down = (y.lo < 0.0) != (y.hi < 0.0);
    let gap = if bits & ((1 << 52) - 1) == 0 && down { 0.5 * half } else { half };
    let ylo = dd::abs(y.lo);
    if (gap - ylo).partial_cmp(&eps) != Some(core::cmp::Ordering::Greater) {
        return None;
    }
    if need_inexact && ylo.partial_cmp(&eps) != Some(core::cmp::Ordering::Greater) {
        return None;
    }
    Some(with_sign(y.hi, bits_sign))
}

#[inline(always)]
fn done64(y: Dd, sign_from: u64) -> Option<f64> {
    settle(y, EPS * dd::abs(y.hi), sign_from, false)
}

/// `atan(x)` to nearest, for `2^-60 <= |x| < 2^60`.
#[inline(always)]
pub fn atan64_nearest(x: f64) -> Option<f64> {
    let bits = x.to_bits();
    let ab = bits & !SIGN;
    if !(pow2_bits(-60)..pow2_bits(60)).contains(&ab) {
        return None;
    }
    let a = f64::from_bits(ab);
    let y = if a <= 1.0 {
        atan_unit64(a, 0.0)
    } else {
        let th = 1.0 / a;
        let p = two_prod(th, a);
        let tl = ((1.0 - p.hi) - p.lo) * th;
        comb(HALF_HI, HALF_LO, -1.0, atan_unit64(th, tl))
    };
    done64(y, bits)
}

/// `asin(x)` to nearest, for `2^-60 <= |x| < 1`.
#[inline(always)]
pub fn asin64_nearest(x: f64) -> Option<f64> {
    let bits = x.to_bits();
    let ab = bits & !SIGN;
    if !(pow2_bits(-60)..pow2_bits(0)).contains(&ab) {
        return None;
    }
    let a = f64::from_bits(ab);
    let (m1, m2) = (two_sum(1.0, -a), two_sum(1.0, a));
    let p = two_prod(m1.hi, m2.hi);
    let s = sqrt_dd(fast_two_sum(p.hi, p.lo + (m1.hi * m2.lo + m1.lo * m2.hi)));
    let av = Dd { hi: a, lo: 0.0 };
    let flip = a > s.hi;
    let t = if flip { div_dd(s, av) } else { div_dd(av, s) };
    let at = atan_unit64(t.hi, t.lo);
    done64(if flip { comb(HALF_HI, HALF_LO, -1.0, at) } else { at }, bits)
}

/// `acos(x)` to nearest, for `2^-60 <= |x| < 1`.
#[inline(always)]
pub fn acos64_nearest(x: f64) -> Option<f64> {
    let bits = x.to_bits();
    let ab = bits & !SIGN;
    if !(pow2_bits(-60)..pow2_bits(0)).contains(&ab) {
        return None;
    }
    let a = f64::from_bits(ab);
    let w = sqrt_dd(div_dd(two_sum(1.0, -a), two_sum(1.0, a)));
    let at = atan_unit64(w.hi, w.lo);
    let y2 = Dd { hi: 2.0 * at.hi, lo: 2.0 * at.lo };
    done64(if bits & SIGN != 0 { comb(PI_HI, PI_LO, -1.0, y2) } else { y2 }, 0)
}

/// `atan2(y, x)` to nearest, for `2^-400 <= |x|, |y| < 2^400` with `|min/max| >= 2^-60`.
#[inline(always)]
pub fn atan2_64_nearest(y: f64, x: f64) -> Option<f64> {
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
    let p = two_prod(th, d);
    let tl = ((n - p.hi) - p.lo) / d;
    let r = atan_unit64(th, tl);
    let neg_x = xb & SIGN != 0;
    let r = match (swap, neg_x) {
        (false, false) => r,
        (true, false) => comb(HALF_HI, HALF_LO, -1.0, r),
        (false, true) => comb(PI_HI, PI_LO, -1.0, r),
        (true, true) => comb(HALF_HI, HALF_LO, 1.0, r),
    };
    done64(r, yb)
}

/// `hypot(x, y)` to nearest, for `2^-450 <= |x|, |y| < 2^450` within a factor `2^60` of each other,
/// when the root is certainly inexact.
#[inline(always)]
pub fn hypot64_nearest(x: f64, y: f64) -> Option<f64> {
    let (ab, bb) = (x.to_bits() & !SIGN, y.to_bits() & !SIGN);
    let win = pow2_bits(-450)..pow2_bits(450);
    if !(win.contains(&ab) && win.contains(&bb)) || (ab >> 52).abs_diff(bb >> 52) > 60 {
        return None;
    }
    let (a, b) = (f64::from_bits(ab), f64::from_bits(bb));
    let (pa, pb) = (two_prod(a, a), two_prod(b, b));
    let s = two_sum(pa.hi, pb.hi);
    let d = fast_two_sum(s.hi, s.lo + (pa.lo + pb.lo));
    let r = sqrt_dd(d);
    settle(r, EPS_HYPOT * r.hi, 0, true)
}
