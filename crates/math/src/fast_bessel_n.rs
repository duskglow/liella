//! `jn`, `yn` (`double` and `float`) of order `|n| >= 2` by three-term recurrence on the fast
//! tier's `J0`, `J1`, `Y0`, `Y1` kernels (Ziv's first step; LIBM-F64-BESSEL-N, D-10.76; spec
//! §10.3, D-10.13: correctly rounded in the caller's mode). Each entry builds a double-double
//! `y·2^e` with a proved absolute bound `eps·2^e` and settles it with the rounding test
//! ([`near64`], [`near32`]); `None` leaves the call to the 384-bit path unchanged. The proofs
//! are in `handoff/libm-f64-bessel-n.md`; in short (`x > 0`, `f_{k+1} = (2k/x)·f_k − f_{k−1}`,
//! A&S 9.1.27):
//!
//! * Forward (`yn` always; `jn` for `n <= x` and up to `⌊x⌋`): an error `δ` put into `f_{k+1}`
//!   reaches `f_m` as `δ·(πx/2)·(Y_k·J_m − J_k·Y_m)` (Casoratian, A&S 9.1.16), bounded with
//!   `|J_k| <= min(1, (x/2)^k/k!)` (A&S 9.1.60, 9.1.62) and `|Y_k| <= M_k <= M_m <= |Y_m| + |J_m|`
//!   (`M_ν² = J_ν² + Y_ν²` increases with `ν`: Nicholson's integral, DLMF 10.9.30). The initial
//!   errors are the kernels' bounds; each step adds at most `2^-100·(|c_k·f_k| + |f_{k+1}|)`
//!   (double-double products, quotient and sum: Joldes, Muller, Popescu, ACM TOMS 44 (2017)).
//! * Backward (`jn`, `n > x`, Miller's algorithm, Gautschi, SIAM Review 9 (1967)): `v_{N+1} = 0`,
//!   `v_N = 1`, down to `m = ⌊x⌋`, then `J_n = J_m·v_n/v_m`. For `k > x` the ratios
//!   `r_k = J_k/J_{k−1}` lie in `(0, q_k]`, `q_k = x/(2k − x) <= 1`, and the relative error of
//!   `v_k/v_{k−1}` obeys `ε_k <= 2^-99 + q_k·q_{k+1}·ε_{k+1}`, `ε_{N+1} = 1`; `N` is the first index
//!   with `(n − m)·Π_{j=n..N} q_j·q_{j+1} <= 2^-110`.
//! * Series (`jn`, `x² <= n + 1`): `(x/2)^n/n!·Σ (−x²/4)^k/(k!·(n+1)_k)` (A&S 9.1.10), terms
//!   alternating and falling by at least 4 each, the sum in `[3/4, 1]`.
//!
//! * Modulus (`m <= x`, both recurrences run): `|Y_k·J_m − J_k·Y_m| <= M_m²` (`tight`), which
//!   keeps the bound near the kernels' own on the oscillatory side.
//!
//! Declines: orders above [`MAX_STEPS`], `x` outside `[2^-990, 2^500]` (on the oscillatory side
//! from about `2^100`, where `tight` has no a priori cap), values beyond `2^960`
//! in a forward recurrence (`yn`'s overflow region stays with the 384-bit path), a bound that
//! does not settle (near the zeros of `J_n`, `Y_n`). Results below half the least subnormal
//! (`jn`'s underflow region) are settled as underflows; subnormal ones by [`fast64::settle`].

#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::cast_precision_loss, clippy::cast_possible_truncation, clippy::cast_sign_loss, clippy::cast_possible_wrap)] // step counts below MAX_STEPS (exact in f64), exponents of bounded range

use core::cmp::Ordering;

use crate::dd::{self, two_prod, Dd};
use crate::fast32::{self, Fast};
use crate::fast64;
use crate::status::{Flags, Rounding, Status};

use super::{hankel_kernel, kernel, mirrored, E95, HANKEL};

/// The largest order (and, for `n > x`, the largest number of steps) taken here.
pub const MAX_STEPS: u64 = 4096;
/// 2^-990: the least operand taken here.
pub const XMIN: f64 = f64::from_bits(0x0210_0000_0000_0000);
/// 2^500: the largest operand taken here.
pub const XMAX: f64 = f64::from_bits(0x5f30_0000_0000_0000);
/// 2^960: a forward recurrence declines beyond (Dekker's split stays below 2^996).
const LIM: f64 = f64::from_bits(0x7bf0_0000_0000_0000);
/// 2^-960: the floor of `min(1, (x/2)^k/k!)` (an upper bound stays one).
const JFLOOR: f64 = f64::from_bits(0x03f0_0000_0000_0000);
/// One forward step's error, relative to `|c_k·f_k| + |f_{k+1}|`.
const U_STEP: f64 = f64::from_bits(0x39b0_0000_0000_0000);
/// One backward step's relative error on a ratio (`2·2^-101 + 2^-104`, rounded up).
const U_BACK: f64 = f64::from_bits(0x39c0_0000_0000_0000);
/// π/2 rounded up, and the slack covering the `f64` roundings of the bounds' own sums.
const PI_2_UP: f64 = core::f64::consts::FRAC_PI_2 * 1.000_001;
const SLK: f64 = 1.001;
const E20: f64 = f64::from_bits(0x3eb0_0000_0000_0000);
const E100: f64 = f64::from_bits(0x39b0_0000_0000_0000);
const E102: f64 = f64::from_bits(0x3990_0000_0000_0000);
const E103: f64 = f64::from_bits(0x3980_0000_0000_0000);
const E107: f64 = f64::from_bits(0x3940_0000_0000_0000);
const E108: f64 = f64::from_bits(0x3930_0000_0000_0000);
const E110: f64 = f64::from_bits(0x3910_0000_0000_0000);
const P400: f64 = f64::from_bits(0x58f0_0000_0000_0000);
const P500: f64 = f64::from_bits(0x5f30_0000_0000_0000);
const M500: f64 = f64::from_bits(0x20b0_0000_0000_0000);

/// The unbiased exponent of a normal `v`.
fn expo(v: f64) -> i32 {
    ((v.to_bits() >> 52) & 0x7ff) as i32 - 1023
}

/// A forward recurrence at index `m`: the value, `S = Σδ`, `SJ = Σδ·Jb(anchor)` and `Jb(m)`.
struct Fwd {
    f: Dd,
    s: f64,
    sj: f64,
    jb: f64,
}

/// `f_m` from `(f_0, eps_0)`, `(f_1, eps_1)` (`m >= 1`, `r = 1/x`), or `None` past [`LIM`].
fn forward(x: f64, r: Dd, f0: (Dd, f64), f1: (Dd, f64), m: u64) -> Option<Fwd> {
    let h = 0.5 * x;
    let jb = |t: f64| t.clamp(JFLOOR, 1.0);
    let mut t = h; // (x/2)^k/k! at k = 1
    let (mut a, mut b) = (f0.0, f1.0);
    let mut s = f0.1 + f1.1;
    // An error in f_1 is anchored at 0 (Jb = 1), one in f_0 at 1.
    let mut sj = f1.1 + f0.1 * jb(t);
    for k in 1..m {
        let p = b.mul_f64((2 * k) as f64).mul(r);
        if !matches!(dd::abs(p.hi).partial_cmp(&LIM), Some(Ordering::Less | Ordering::Equal)) {
            return None;
        }
        let c = p.sub(a);
        let rho = U_STEP * (dd::abs(p.hi) + dd::abs(c.hi));
        s += rho;
        sj += rho * jb(t);
        t = t * h / (k + 1) as f64;
        a = b;
        b = c;
    }
    Some(Fwd { f: b, s, sj, jb: jb(t) })
}

/// The Casoratian bound on `|f_m − f̃_m|` given `ym >= |Y_m|` (module doc).
fn cas(x: f64, w: &Fwd, ym: f64) -> f64 {
    PI_2_UP * x * (ym * w.sj + (ym + w.jb) * w.jb * w.s) * SLK
}

/// The bound for the `Y` recurrence itself (`|Y_m| <= |ỹ_m| + E`: `E <= a + β·E`).
fn ybound(x: f64, w: &Fwd) -> Option<f64> {
    let beta = PI_2_UP * x * (w.sj + w.jb * w.s) * SLK;
    if !matches!(beta.partial_cmp(&E20), Some(Ordering::Less | Ordering::Equal)) {
        return None;
    }
    Some(cas(x, w, dd::abs(w.f.hi) * SLK) * 1.000_002)
}

/// The bounds `(ej, ey)` of a `J` and a `Y` recurrence to the same `m <= x`, tightened through
/// the modulus `M_m = (J_m² + Y_m²)^(1/2)`: by Cauchy-Schwarz `|Y_k·J_m − J_k·Y_m| <= M_k·M_m <=
/// M_m²` (`k <= m`), so each error is at most `(πx/2)·M_m²·S`. With `a = |J̃_m| + |Ỹ_m|` and
/// `c = (πx/2)·(S_J + S_Y)`, `M_m <= a + c·M_m²`; the given bounds cap `M_m` at `M0`, and when
/// `c·M0 <= 1/4` (below the large root `>= 1/(2c)`) and `u = 4ca <= 1/2`, `M_m` is at most the
/// small root `2a/(1 + √(1−u)) <= a·(1 + u)`. Otherwise the given bounds stand.
fn tight(x: f64, wj: &Fwd, wy: &Fwd, ej: f64, ey: f64) -> (f64, f64) {
    let (aj, ay) = (dd::abs(wj.f.hi) * SLK, dd::abs(wy.f.hi) * SLK);
    let m0 = ay + ey + (aj + ej).min(wj.jb);
    let c = PI_2_UP * x * (wj.s + wy.s) * SLK;
    let a = aj + ay;
    let u = 4.0 * c * a;
    if c * m0 > 0.25 || u > 0.5 || u.is_nan() || m0.is_nan() {
        return (ej, ey);
    }
    let mb = a * (1.0 + u) * SLK;
    let k = PI_2_UP * x * mb * mb * SLK;
    (ej.min(k * wj.s), ey.min(k * wy.s))
}

/// `1/x` (within 2^-102).
fn recip(x: f64) -> Dd {
    Dd::from(1.0).div(Dd::from(x))
}

/// `[J0, J1, Y0, Y1]` at `x` with their bounds.
fn base(x: f64) -> Option<[(Dd, f64); 4]> {
    if x >= HANKEL {
        let (j0, y0, e0) = hankel_kernel(0, x);
        let (j1, y1, e1) = hankel_kernel(1, x);
        let b = |v: Dd, e: f64| (v, e + dd::abs(v.hi) * E95);
        return Some([b(j0, e0), b(j1, e1), b(y0, e0), b(y1, e1)]);
    }
    Some([kernel(0, x)?, kernel(1, x)?, kernel(2, x)?, kernel(3, x)?])
}

/// `Y_n(x)` as `(y, eps)` for `n >= 2`, `XMIN <= x <= XMAX`.
pub fn yn_kernel(n: u64, x: f64) -> Option<(Dd, f64)> {
    if n > MAX_STEPS {
        return None;
    }
    let r = recip(x);
    if (n as f64) <= x {
        // The oscillatory side: the `J` recurrence too, for the modulus bound ([`tight`]).
        let [j0, j1, y0, y1] = base(x)?;
        let wy = forward(x, r, y0, y1, n)?;
        let ey = ybound(x, &wy)?;
        let wj = forward(x, r, j0, j1, n)?;
        let ej = cas(x, &wj, dd::abs(wy.f.hi) * SLK + ey);
        return Some((wy.f, tight(x, &wj, &wy, ej, ey).1));
    }
    let w = forward(x, r, kernel(2, x)?, kernel(3, x)?, n)?;
    let e = ybound(x, &w)?;
    Some((w.f, e))
}

/// `J_n(x)` as `(y, eps, e)` (value `y·2^e`, bound `eps·2^e`) for `n >= 2`, `XMIN <= x <= XMAX`.
pub fn jn_kernel(n: u64, x: f64) -> Option<(Dd, f64, i32)> {
    if n > MAX_STEPS {
        return None;
    }
    if x * x <= (n + 1) as f64 {
        return Some(series(n, x));
    }
    let up = (n as f64) <= x;
    if !up && x > MAX_STEPS as f64 {
        return None;
    }
    let [j0, j1, y0, y1] = base(x)?;
    let r = recip(x);
    // x > √3 here, so ⌊x⌋ >= 1.
    // RANGE: x <= MAX_STEPS when !up (returned above); the disassembly keeps the conversion on
    // that branch (D-10.95 scan).
    let m = if up { n } else { x as u64 };
    let wj = forward(x, r, j0, j1, m)?;
    let wy = forward(x, r, y0, y1, m)?;
    let ey = ybound(x, &wy)?;
    let ej = cas(x, &wj, dd::abs(wy.f.hi) * SLK + ey);
    let ej = tight(x, &wj, &wy, ej, ey).0;
    if up {
        return Some((wj.f, ej, 0));
    }
    let (q, e, rel) = miller(x, r, n, m)?;
    let y = wj.f.mul(q);
    Some((y, (dd::abs(q.hi) * ej + dd::abs(y.hi) * (rel + E102)) * SLK, e))
}

/// `v_n/v_m` as `(q, e, rel)` (`q·2^e`, relative bound `rel`) by the backward recurrence from
/// `N` (module doc), `x < m + 1 <= n`.
fn miller(x: f64, r: Dd, n: u64, m: u64) -> Option<(Dd, i32, f64)> {
    let q = |j: u64| x / ((2 * j) as f64 - x);
    let span = (n - m) as f64;
    let (mut p, mut top, mut qj) = (1.0f64, n, q(n));
    loop {
        let qn = q(top + 1);
        p *= qj * qn;
        if p * SLK * span <= E110 {
            break;
        }
        top += 1;
        if top - m > MAX_STEPS {
            return None;
        }
        qj = qn;
    }
    let (mut a, mut b) = (Dd::from(0.0), Dd::from(1.0)); // v_{k+1}, v_k at k = N
    let (mut scale, mut vn, mut sn) = (0i32, b, 0i32);
    let mut k = top;
    while k > m {
        let c = b.mul_f64((2 * k) as f64).mul(r).sub(a);
        a = b;
        b = c;
        k -= 1;
        if dd::abs(b.hi) > P400 {
            a = a.scale(M500);
            b = b.scale(M500);
            scale += 500;
        }
        if k == n {
            vn = b;
            sn = scale;
        }
    }
    let rel = 1.01 * (span * (top + 1 - m) as f64 * U_BACK + span * p * SLK) + E102;
    Some((vn.div(b), sn - scale, rel))
}

/// `J_n(x)` for `x² <= n + 1` by the power series (module doc), as in [`jn_kernel`].
fn series(n: u64, x: f64) -> (Dd, f64, i32) {
    let h = 0.5 * x;
    let he = expo(h);
    let hm = h * dd::pow2(-he);
    let (mut num, mut den, mut e) = (Dd::from(1.0), Dd::from(1.0), he * n as i32);
    for k in 1..=n {
        num = num.mul_f64(hm);
        den = den.mul_f64(k as f64);
        if num.hi > P500 {
            num = num.scale(M500);
            e += 500;
        }
        if den.hi > P500 {
            den = den.scale(M500);
            e -= 500;
        }
    }
    let zf = x * x * 0.25;
    let (mut t, mut kk) = (1.0f64, 0u64);
    loop {
        let nt = t * zf / ((kk + 1) * (n + kk + 1)) as f64;
        if nt < E108 {
            break;
        }
        t = nt;
        kk += 1;
    }
    // z exact: kk >= 1 only when x² >= 2^-106.
    let z = two_prod(x, x).scale(0.25);
    let mut s = Dd::from(1.0);
    for k in (1..=kk).rev() {
        s = Dd::from(1.0).sub(z.mul(s).div(Dd::from((k * (n + k)) as f64)));
    }
    let y = num.div(den).mul(s);
    let rel = (2 * n + 3) as f64 * E103 + (kk as f64 * E100 + E107) * 1.34;
    (y, dd::abs(y.hi) * rel * SLK, e)
}

/// `(y ± eps)·2^e` correctly rounded to `double` in `mode`, or `None`: the rounding test.
pub fn near64(y: Dd, eps: f64, e: i32, mode: Rounding) -> Option<(f64, Status)> {
    if eps.partial_cmp(&dd::abs(y.hi)) != Some(Ordering::Less) {
        return None;
    }
    let neg = y.hi < 0.0;
    let (ya, md) = if neg { (y.neg(), mirrored(mode)) } else { (y, mode) };
    let k = expo(ya.hi);
    let s = dd::pow2(-k);
    let m = e + k;
    if m < -1080 {
        // Below 4·2^-1080: under half the least subnormal.
        return Some(crate::elem::underflow::<f64>(neg, mode));
    }
    let (v, st) = fast64::settle(ya.scale(s), eps * s, m, md)?;
    Some((if neg { -v } else { v }, st))
}

/// `(y ± eps)·2^e` correctly rounded to a normal `float` in `mode`, or `None`.
pub fn near32(y: Dd, eps: f64, e: i32, mode: Rounding) -> Option<(f32, Status)> {
    if eps.partial_cmp(&dd::abs(y.hi)) != Some(Ordering::Less) {
        return None;
    }
    let k = expo(y.hi) + e;
    if !(-126..=127).contains(&k) {
        return None;
    }
    let s = dd::pow2(e);
    let (v, lo) = (y.hi * s, y.lo * s);
    let ulp = dd::pow2(k - 52);
    // The error in f64 units of `v`, rounded up (truncation plus 2: no `ceil` in core).
    let d = (dd::abs(lo) + eps * s) / ulp;
    if d.partial_cmp(&67_108_864.0) != Some(Ordering::Less) {
        return None;
    }
    // RANGE: 0 <= d < 2^26 (NaN and larger returned above).
    let r = fast32::round(v, d as u64 + 2, mode)?;
    Some((r, Status::flags(Flags::INEXACT).hw(Flags::INEXACT)))
}

/// The order's magnitude and whether `(−1)^n` flips the sign for `n < 0`.
fn order(n: i32) -> (u64, bool) {
    let m = u64::from(n.unsigned_abs());
    (m, m % 2 == 1 && n < 0)
}

/// `jn(n, x)` (`J_{−n} = (−1)^n·J_n`, `J_n(−x) = (−1)^n·J_n(x)`), or `None`.
pub fn jn(n: i32, x: f64, mode: Rounding) -> Option<(f64, Status)> {
    if !x.is_finite() || x == 0.0 {
        return None;
    }
    let (m, flip) = order(n);
    match m {
        0 => super::j0(x, mode),
        1 => super::j1(if flip { -x } else { x }, mode),
        _ => {
            let ax = dd::abs(x);
            if !(XMIN..=XMAX).contains(&ax) {
                return None;
            }
            let (y, eps, e) = jn_kernel(m, ax)?;
            let neg = flip != (m % 2 == 1 && x < 0.0);
            near64(if neg { y.neg() } else { y }, eps, e, mode)
        }
    }
}

/// `yn(n, x)` for `x > 0` (`Y_{−n} = (−1)^n·Y_n`), or `None`.
pub fn yn(n: i32, x: f64, mode: Rounding) -> Option<(f64, Status)> {
    if x.partial_cmp(&0.0) != Some(Ordering::Greater) || !x.is_finite() {
        return None;
    }
    let (m, flip) = order(n);
    match m {
        0 => super::y0(x, mode),
        1 if flip => super::y1(x, mirrored(mode)).map(|(v, st)| (-v, st)),
        1 => super::y1(x, mode),
        _ => {
            if !(XMIN..=XMAX).contains(&x) {
                return None;
            }
            let (y, eps) = yn_kernel(m, x)?;
            near64(if flip { y.neg() } else { y }, eps, 0, mode)
        }
    }
}

/// `jnf(n, x)` through the `double` kernels, or `None` (zero, subnormal and non-finite `x`).
/// (`special::jn` answers orders 0 and ±1 by `j0f` and `j1f` before this tier.)
pub fn jnf(n: i32, x: f32, mode: Rounding) -> Option<(f32, Status)> {
    if !x.is_normal() {
        return None;
    }
    let (m, flip) = order(n);
    match m {
        0 => fast32::eval(Fast::J0, x, mode),
        1 => fast32::eval(Fast::J1, if flip { -x } else { x }, mode),
        _ => {
            let (y, eps, e) = jn_kernel(m, f64::from(x.abs()))?;
            let neg = flip != (m % 2 == 1 && x < 0.0);
            near32(if neg { y.neg() } else { y }, eps, e, mode)
        }
    }
}

/// `ynf(n, x)` through the `double` kernels, or `None`. (`special::yn` answers orders 0 and ±1
/// by `y0f` and `y1f` before this tier.)
pub fn ynf(n: i32, x: f32, mode: Rounding) -> Option<(f32, Status)> {
    if !x.is_normal() || x.is_sign_negative() {
        return None;
    }
    let (m, flip) = order(n);
    match m {
        0 => fast32::eval(Fast::Y0, x, mode),
        1 if flip => fast32::eval(Fast::Y1, x, mirrored(mode)).map(|(v, st)| (-v, st)),
        1 => fast32::eval(Fast::Y1, x, mode),
        _ => {
            let (y, eps) = yn_kernel(m, f64::from(x))?;
            near32(if flip { y.neg() } else { y }, eps, 0, mode)
        }
    }
}
