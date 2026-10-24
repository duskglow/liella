//! Error, gamma and Bessel functions (spec §10.3) of every format that has them (`float`,
//! `double`, x87 `long double`; generic over `Float`), evaluated in the 384-bit arithmetic of
//! [`crate::mp`] and rounded once (Track M milestones 3 and 5b):
//!
//! * `erf x = 2x/√π · e^(−x²) · Σ (2x²)ⁿ / (1·3···(2n+1))`, a series of positive terms; `erfc` is
//!   `1 − erf` below 10 (at most 150 of the 384 bits cancel) and the Laplace continued fraction
//!   `erfc x = e^(−x²)/√π · 1/(x + (1/2)/(x + 1/(x + (3/2)/(x + ...))))` above.
//! * `lnΓ` by Stirling's series `(y − ½) ln y − y + ½ ln 2π + Σ B₂ₖ/(2k(2k−1) y^(2k−1))` for
//!   `y ≥ 40` (the terms fall below 2^-360), after shifting `x` up by the recurrence
//!   `Γ(x + n) = x(x+1)···(x+n−1) Γ(x)`; the reflection `Γ(x) Γ(1−x) = π / sin πx` for `x < 0`.
//!   `Γ` is `±e^(lnΓ)`, and exact at the positive integers up to 92 (91! has 380 significant bits).
//! * The Bessel functions: see `j0` and the functions after it.
//!
//! Operands enter as their exact values (`Mp`), so the same code serves every format; the
//! thresholds that depend on the format's range come from `Float::EMAX` and `Float::PREC`.

#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_precision_loss, clippy::cast_sign_loss, clippy::indexing_slicing, clippy::float_cmp)] // exponents, term indices and integer arguments of bounded range; exact comparisons with integers

use core::f64::consts::LN_2;

use crate::coeffs::tables as t;
use crate::dd;
use crate::elem;
use crate::float::{self, Exact, Float, Kind, Num};
use crate::mp::{self, c, Arith, Mp};
use crate::mpw;
use crate::status::{Rounding, Status};
use crate::wide::{self, beside, near_one, one};

/// Below 2^-392 of the running sum: a series term that no longer matters.
/// (`BITS + 8` below for the 768-bit second step.)
fn negligible<M: Arith>(term: M, sum: M) -> bool {
    term.is_zero() || term.expo() < sum.expo() - M::BITS - 8
}

fn signed<M: Arith>(v: M, neg: bool) -> M {
    if neg { v.neg() } else { v }
}

/// The magnitude of a finite nonzero operand: exactly, as an `f64` estimate (saturating, for
/// branch decisions), and as `sig · 2^e` (for Payne–Hanek reduction).
#[derive(Clone, Copy)]
struct Arg {
    v: Mp,
    f: f64,
    sig: u128,
    e: i64,
}

impl Arg {
    fn of(n: &Num) -> Arg {
        let v = wide::value(&Num { neg: false, ..*n });
        Arg { v, f: v.to_f64(), sig: n.sig, e: i64::from(n.exp) }
    }
}

/// `ln 2^(EMAX + PREC)`: a result below `e^-this` is under a quarter of the smallest subnormal.
fn ln_tiny<F: Float>() -> f64 {
    f64::from(F::EMAX + F::PREC as i32) * LN_2
}

/// The domain errors of `tgamma` and `yn`: a positive quiet NaN as glibc's `float`, `double` and
/// `long double` forms give, the default NaN (sign bit set) as its `_Float128` forms give
/// (`special_f128_products.rs`).
fn domain_nan<F: Float>() -> (F, Status) {
    if F::PREC > 64 { elem::domain() } else { elem::domain_pos() }
}

// ----------------------------------------------------------------------------- error function

/// `erf a` for `0 < a < 12` (the series).
fn erf_mp(x: Mp) -> Mp {
    let a = x.to_f64();
    let t2 = x.sqr().ldexp(1); // 2x², exact
    let one = Mp::from_i64(1);
    let (mut sum, mut term) = (one, one);
    let mut n = 1u64;
    loop {
        term = term.mul(t2).div_u64(2 * n + 1);
        // The terms grow while 2x² > 2n + 1, then fall.
        if (2 * n + 1) as f64 > 2.0 * a * a && negligible(term, sum) {
            break;
        }
        sum = sum.add(term);
        n += 1;
    }
    let e = mp::exp(x.sqr().neg());
    c(&t::MP_TWO_OVER_SQRT_PI).mul(x).mul(e).mul(sum)
}

/// `erfc a` for `a >= 10` by the continued fraction, evaluated from depth `n` up: the depth
/// needed falls with `a` (the tail's influence decays like `e^(−2a√(2n))`).
fn erfc_cf(x: Mp) -> Mp {
    let a = x.to_f64();
    let depth = 40 + (60_000.0 / (a * a)) as u64;
    let mut tail = x;
    for n in (1..=depth).rev() {
        tail = x.add(Mp::from_i64(n as i64).ldexp(-1).div(tail));
    }
    let e = mp::exp(x.sqr().neg());
    c(&t::MP_INV_SQRT_PI).mul(e).div(tail)
}

/// `erfc x` is just beside 1 for `|x| < 2^this`: the deviation `2|x|/√π < 2^(this+1)` stays
/// below 2^-(PREC+34) (−100 for `float`, `double` and `long double`; −149 for `_Float128`, where
/// −100 would round `1 − 2^-99` to 1).
fn erfc_beside_one<F: Float>() -> i32 {
    (-100).min(-(F::PREC as i32) - 36)
}

/// `erfc a` is `1 − erf a` below this and the continued fraction above. `1 − erf a` loses
/// `−log₂ erfc a` of the 384 bits: about 148 at 10 (236 left, enough for 64-bit significands),
/// 55 at 6 (some 320 left, enough for 113-bit ones, D-10.20). The fraction's depth
/// `40 + 60000/a²` is 1707 at 6.
fn erfc_cf_min<F: Float>() -> f64 {
    if F::PREC > 64 { 6.0 } else { 10.0 }
}

/// `erf(x)`.
pub fn erf<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return r;
    }
    match n.kind {
        Kind::Zero => return (x, Status::OK),
        Kind::Inf => return (one(n.neg), Status::OK),
        _ => {}
    }
    let a = Arg::of(&n);
    if a.f >= 12.0 {
        // 1 − erfc a, erfc a below 2^-200: just inside ±1.
        return near_one(n.neg, false, mode);
    }
    mp::round(signed(erf_mp(a.v), n.neg), mode)
}

/// `erfc(x)`.
pub fn erfc<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return r;
    }
    match n.kind {
        Kind::Zero => return (one(false), Status::OK),
        Kind::Inf => return (if n.neg { float::pack(false, 1, 1) } else { float::zero(false) }, Status::OK),
        _ => {}
    }
    if n.top() < erfc_beside_one::<F>() {
        // 1 ∓ 2x/√π, the deviation below 2^(top+1), far below half a unit of 1's last place.
        return near_one(false, n.neg, mode);
    }
    let a = Arg::of(&n);
    if n.neg {
        if a.f >= 12.0 {
            return beside(Mp::from_i64(2), false, mode);
        }
        return mp::round(Mp::from_i64(1).add(erf_mp(a.v)), mode);
    }
    if a.f * a.f > ln_tiny::<F>() {
        // erfc a < e^(−a²): below a quarter of the smallest subnormal.
        return elem::underflow(false, mode);
    }
    let v = if a.f < erfc_cf_min::<F>() { Mp::from_i64(1).sub(erf_mp(a.v)) } else { erfc_cf(a.v) };
    mp::round(v, mode)
}

// ------------------------------------------------------------------------------ gamma functions

/// The shift threshold of Stirling's series.
const STIRLING_MIN: f64 = 40.0;
/// Γ(n) = (n − 1)! is computed exactly up to this `n` (91! has 380 significant bits).
const EXACT_FACTORIAL_MAX: i64 = 92;

/// `lnΓ(y)` for `y >= 40` by Stirling's series.
fn stirling(y: Mp) -> Mp {
    let half = Mp::from_i64(1).ldexp(-1);
    let mut s = y.sub(half).mul(mp::ln(y)).sub(y).add(c(&t::MP_HALF_LN_2PI));
    let inv = y.recip();
    let inv2 = inv.sqr();
    let mut p = inv;
    for (k, ck) in t::MP_STIRLING.iter().enumerate() {
        let term = c(ck).mul(p);
        if negligible(term, s) {
            break;
        }
        s = if k % 2 == 0 { s.add(term) } else { s.sub(term) };
        p = p.mul(inv2);
    }
    s
}

/// `lnΓ(x)` for `x > 0` (an `Mp`: `1 − x` for a negative `x` is not always representable).
fn lgamma_pos(xm: Mp) -> Mp {
    let x = xm.to_f64();
    if x >= STIRLING_MIN {
        return stirling(xm);
    }
    // Γ(x) = Γ(x + n) / (x (x+1) ··· (x+n−1)) with x + n >= 40.
    let n = (STIRLING_MIN - x) as i64 + 1;
    let mut prod = xm;
    for k in 1..n {
        prod = prod.mul(xm.add(Mp::from_i64(k)));
    }
    stirling(xm.add(Mp::from_i64(n))).sub(mp::ln(prod))
}

/// `sin(πx)` for a finite `x` that is not an integer: `x = m + f` with `|f| <= ½` (both exact),
/// `sin(πx) = (−1)^m sin(πf)`.
fn sin_pi(x: Mp) -> Mp {
    let t = x.frac(); // the sign of x, |t| < 1
    let ip = x.sub(t); // an integer, below 2^113
    let mut odd = !ip.ldexp(-1).frac().is_zero();
    let one = Mp::from_i64(1);
    let f = if t.cmp_abs(one.ldexp(-1)).is_gt() {
        odd = !odd;
        if t.neg { t.add(one) } else { t.sub(one) }
    } else {
        t
    };
    let (s, _) = mp::sin_cos_small(mp::pi().mul(f));
    signed(s, odd)
}

/// `lnΓ(x)` and the sign of `Γ(x)` for a finite `x` that is not zero or a negative integer.
fn lgamma_mp(x: Mp) -> (Mp, i32) {
    if !x.neg {
        return (lgamma_pos(x), 1);
    }
    // Γ(x) = π / (sin(πx) Γ(1 − x)).
    let s = sin_pi(x);
    let lg = mp::ln(mp::pi()).sub(mp::ln(s.abs())).sub(lgamma_pos(Mp::from_i64(1).sub(x)));
    (lg, if s.neg { -1 } else { 1 })
}

/// `lgamma(x)` and the sign of `Γ(x)` (`signgam`: 1 or −1).
pub fn lgamma<F: Float>(x: F, mode: Rounding) -> ((F, Status), i32) {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return (r, 1);
    }
    match n.kind {
        Kind::Inf => return ((float::inf(false), Status::OK), 1),
        Kind::Zero => return (elem::pole(false), if n.neg { -1 } else { 1 }),
        _ => {}
    }
    if n.neg && wide::integer(&n).0 {
        return (elem::pole(false), 1);
    }
    let v = wide::value(&n);
    if v == Mp::from_i64(1) || v == Mp::from_i64(2) {
        return ((float::zero(false), Status::OK), 1);
    }
    let (lg, sign) = lgamma_mp(v);
    (mp::round(lg, mode), sign)
}

/// `tgamma(x)`.
pub fn tgamma<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    let n = float::decode(x);
    if let Some(r) = elem::nan_in(&n) {
        return r;
    }
    match n.kind {
        Kind::Inf => return if n.neg { domain_nan::<F>() } else { (x, Status::OK) },
        Kind::Zero => return elem::pole(n.neg),
        _ => {}
    }
    let (int, k) = wide::integer(&n);
    if int && n.neg {
        return domain_nan::<F>();
    }
    if let (true, Some(k @ 1..=EXACT_FACTORIAL_MAX)) = (int, k) {
        // (k−1)! exactly (at most 380 significant bits).
        let mut p = Mp::from_i64(1);
        for j in 2..k as u64 {
            p = p.mul_u64(j);
        }
        return mp::round_exact(p, mode);
    }
    if n.sig.is_power_of_two() && n.top() < -200 {
        // Γ(±2^-k) = ±2^k − γ + O(2^-k): just below ±2^k in value (the term below 2^-200 of it).
        let k = -n.top();
        // 127 bits: 2^k − 2^(k−127) with the sticky bit is inside 2^k for every format (100 bits
        // made 2^k − 2^(k−100) an exact `_Float128`).
        let (neg, sig) = if n.neg { (true, 1u128 << 126) } else { (false, (1u128 << 127) - 1) };
        let shift = if n.neg { 126 } else { 127 };
        let (r, flags) = float::round::<F>(Exact { neg, sig, exp: k - shift, sticky: true }, mode);
        return (r, float::range_errno(r, Status::flags(flags)));
    }
    let a = Arg::of(&n);
    if !n.neg && a.f > 1.0e6 {
        // Γ(10^6) is far beyond every format.
        return elem::overflow(false, mode);
    }
    let (lg, sign) = lgamma_mp(signed(a.v, n.neg));
    let lf = lg.to_f64();
    if lf > f64::from(F::EMAX + 1) * LN_2 + 1.0 {
        return elem::overflow(sign < 0, mode);
    }
    if lf < -ln_tiny::<F>() {
        return elem::underflow(sign < 0, mode);
    }
    mp::round(signed(mp::exp(lg), sign < 0), mode)
}

// ----------------------------------------------------------------------------- Bessel functions
//
// For x < 100 the power series (A&S 9.1.10, 9.1.11) in 384 bits: the terms reach about
// e^x / (2πx) before they cancel down to a result of order 1/√x, so at most about 150 bits are
// lost. Above, Hankel's asymptotic expansion (A&S 9.2.5–9.2.10) for orders n ≤ √x: its terms
// fall to about e^(−2x) (2^-288 at x = 100) before they diverge. Larger orders by recurrence:
// Y forward from Y0, Y1 (stable); J forward from J0, J1 while n < x, and by Miller's backward
// recurrence normalised by J0 or J1 for n ≥ x. Recurrences of more than 5000 steps run in
// double-double arithmetic (relative error about n·2^-106).

/// The power series below this argument, Hankel's expansion above.
const BESSEL_SERIES_MAX: f64 = 100.0;
/// Recurrences longer than this run in double-double arithmetic (relative error about
/// `n·2^-106`, a small fraction of a last place for 64-bit significands). A 113-bit significand
/// would lose its last bits that way, so `_Float128` runs every recurrence in 384 bits (D-10.20).
fn mp_recurrence_max<F: Float>() -> u64 {
    if F::PREC > 64 { u64::MAX } else { 5000 }
}

/// A value below the smallest subnormal of every format (`mp::round` takes it for an underflow).
fn far_below() -> Mp {
    Mp::from_i64(1).ldexp(-(1 << 22))
}

/// `(x/2)^n / n!` for `x > 0`.
fn half_pow_over_fact(n: u64, x: Mp) -> Mp {
    let h = x.ldexp(-1);
    if n <= 64 {
        let mut p = Mp::from_i64(1);
        for k in 1..=n {
            p = p.mul(h).div_u64(k);
        }
        return p;
    }
    let lg = lgamma_pos(Mp::from_i64(n as i64 + 1));
    let z = mp::ln(h).mul(Mp::from_i64(n as i64)).sub(lg);
    if z.to_f64() < -1.0e9 {
        return far_below();
    }
    mp::exp(z)
}

/// `(x/2)^n / n!` and `(2/x)^n` (the latter only when `want_y`) by `n` products, for `x > 0`:
/// a relative error below `2^(bitlen(4n) + 1 − BITS)` each.
fn powers<M: Arith>(n: u64, x: M, want_y: bool) -> (M, M) {
    let h = x.ldexp(-1);
    let two_over_x = x.recip().ldexp(1);
    let (mut p, mut tx) = (M::int(1), M::int(1));
    for k in 1..=n {
        p = p.mul(h).div_u64(k);
        if want_y {
            tx = tx.mul(two_over_x);
        }
    }
    (p, tx)
}

/// The bits of `k` (0 for 0).
fn bitlen(k: u64) -> i64 {
    64 - i64::from(k.leading_zeros())
}

/// The exponent of `v` (far below everything for zero), for error bounds.
fn ex<M: Arith>(v: M) -> i64 {
    if v.is_zero() { -(1 << 40) } else { v.expo() }
}

/// A result and the exponent `err` of a bound `2^err` on its absolute error.
#[derive(Clone, Copy)]
struct Bounded<M> {
    v: M,
    err: i64,
}

/// `J_n(x)` and, when `want_y`, `Y_n(x)` by the power series (any `x > 0`; used below 100 in
/// 384 bits and below [`WIDE_SERIES_MAX`] in 768), each with a bound on its absolute error.
/// `p = (x/2)^n / n!` and `tx = (2/x)^n` come from the caller with relative errors below
/// `2^pow_rel`.
///
/// The bound (first order in `u = 2^(1 − BITS)`, the relative error of one operation): after
/// `K` terms, `|t_k|` below `2^emax`, each term carries at most `4k·u` (three operations a step
/// and `x²/4`), each of the `K` additions at most `K·2^emax·u`, so `Σ t_k` is off by less than
/// `5K²·2^emax·u < 2^(emax + 2L + 4 − BITS)` with `L = bitlen(K + n)`; the tail after the
/// break is alternating and decreasing (below the first omitted term). Y's sum weights `t_k` by
/// `|H_k + H_{n+k} − 2γ| < 2L + 2 < 2^FB`: `2^(emax + FB + 2L + 5 − BITS)`. The finite sum has
/// positive terms (`bitlen(8n)` operations), `ln(x/2)` an absolute error below
/// `2^(max(e, 0) + 16 − BITS)`, and the last products and sums add their own `u` each.
fn bessel_series<M: Arith>(n: u64, x: M, want_y: bool, p: M, tx: M, pow_rel: i64) -> (Bounded<M>, Bounded<M>) {
    let bits = M::BITS;
    let xf = x.to_f64();
    let q = x.sqr().ldexp(-2); // x²/4
    let qf = xf * xf / 4.0;
    // J: Σ t_k with t_k = (−x²/4)^k n! / (k! (n+k)!); Y's last sum: Σ (H_k + H_{n+k} − 2γ) t_k.
    let mut hn = M::int(0); // H_n
    if want_y {
        for j in 1..=n {
            hn = hn.add(M::int(1).div_u64(j));
        }
    }
    let two_gamma = M::euler().ldexp(1);
    let (mut sj, mut sy) = (M::int(1), hn.sub(two_gamma));
    let mut term = M::int(1);
    let (mut hk, mut hnk) = (M::int(0), hn);
    let mut emax = 1i64; // |t_0| = 1 < 2^1
    let mut k = 1u64;
    loop {
        term = term.mul(q).div_u64(k).div_u64(n + k).neg();
        if want_y {
            hk = hk.add(M::int(1).div_u64(k));
            hnk = hnk.add(M::int(1).div_u64(n + k));
        }
        let falling = (k as f64) * ((n + k) as f64) > qf;
        if falling && negligible(term, sj) {
            break;
        }
        emax = emax.max(term.expo());
        sj = sj.add(term);
        if want_y {
            sy = sy.add(term.mul(hk.add(hnk).sub(two_gamma)));
        }
        k += 1;
    }
    let l = bitlen(k + n);
    let e_sj = (emax + 2 * l + 4 - bits).max(ex(term));
    let j = p.mul(sj);
    let ej = (ex(p) + e_sj).max(ex(j) + pow_rel + 1) + 1;
    let jb = Bounded { v: j, err: ej };
    if !want_y {
        return (jb, Bounded { v: M::int(0), err: 0 });
    }
    let fb = bitlen(2 * l as u64 + 2);
    let e_sy = (emax + fb + 2 * l + 5 - bits).max(ex(term) + fb + 1);
    let inv_pi = M::pi().recip();
    // −(1/π)(2/x)^n Σ_{k<n} (n−k−1)!/k! (x²/4)^k.
    let mut fin = M::int(0);
    if n > 0 {
        let mut u = M::int(1);
        for j in 1..n {
            u = u.mul_u64(j); // (n−1)!
        }
        for k in 0..n {
            fin = fin.add(u);
            if k + 1 < n {
                u = u.mul(q).div_u64((k + 1) * (n - k - 1));
            }
        }
        fin = fin.mul(tx).mul(inv_pi).neg();
    }
    let lnx2 = x.ldexp(-1).ln();
    let le = ex(lnx2).max(0);
    let jl = j.mul(lnx2).mul(inv_pi).ldexp(1);
    let py = p.mul(sy).mul(inv_pi);
    let y = fin.add(jl).sub(py);
    let e_fin = ex(fin) + (bitlen(8 * n) + 1 - bits).max(pow_rel) + 2;
    let e_jl = (ej + le + 1).max(ex(j) + le + 17 - bits).max(ex(jl) + 4 - bits);
    let e_py = (ex(p) + e_sy).max(ex(py) + pow_rel.max(4 - bits) + 2);
    let e_sum = ex(fin).max(ex(jl)).max(ex(py)) + 2 - bits;
    let ey = e_fin.max(e_jl).max(e_py).max(e_sum) + 3;
    (jb, Bounded { v: y, err: ey })
}

/// `J_n(x)` and, when `want_y`, `Y_n(x)` by the 384-bit power series, for `0 < x < 100`.
fn bessel_series_mp(n: u64, x: Mp, want_y: bool) -> (Bounded<Mp>, Bounded<Mp>) {
    let bits = Mp::BITS;
    if n <= 64 {
        let (p, tx) = powers(n, x, want_y);
        return bessel_series(n, x, want_y, p, tx, bitlen(4 * n) + 1 - bits);
    }
    let p = half_pow_over_fact(n, x);
    let tx = if want_y { mp::exp(mp::ln(x.recip().ldexp(1)).mul(Mp::from_i64(n as i64))) } else { Mp::ZERO };
    // exp of an argument up to about 2^37 and Stirling's lgamma: about 2^-338 relative.
    bessel_series(n, x, want_y, p, tx, 52 - bits)
}

/// The second step of Ziv's strategy (D-10.20): below this argument a Bessel value whose
/// 384-bit error interval straddles a rounding boundary is evaluated again by the power series
/// in 768 bits. The terms reach about `e^x/√(2πx)` (2^226 at 160) against results down to
/// about 2^-130 next to a zero, which leaves more than 380 of the 768 bits; Hankel's
/// truncation above (`e^(−2x)`, 2^-461 at 160) leaves more than 330 of its 384.
const WIDE_SERIES_MAX: f64 = 160.0;
/// The largest order of the second step (the zeros of `J_n` and `Y_n` lie beyond `x = n`, so
/// larger orders have no zero below [`WIDE_SERIES_MAX`]).
const WIDE_ORDER_MAX: u64 = 160;

/// `J_n(x)` or `Y_n(x)` by the 768-bit power series, when `x` and `n` are in its range.
fn bessel_wide(n: u64, a: &Arg, want_y: bool) -> Option<Bounded<mpw::Mp>> {
    if a.f >= WIDE_SERIES_MAX || n > WIDE_ORDER_MAX {
        return None;
    }
    let x = mpw::Mp::widen(a.v);
    let (p, tx) = powers(n, x, want_y);
    let (j, y) = bessel_series(n, x, want_y, p, tx, bitlen(4 * n) + 1 - mpw::Mp::BITS);
    Some(if want_y { y } else { j })
}

/// The rounding test of Ziv's strategy: `v` rounded in `mode` when every value within
/// `2^err` of it rounds alike (the same encoding and the same flags; rounding is monotone, so
/// both ends of the interval suffice). The ends are widened by four last places of `v` for the
/// truncation of `v ± 2^err` itself.
fn decided<F: Float, M: Arith>(v: Bounded<M>, mode: Rounding) -> Option<(F, Status)> {
    let e = M::int(1).ldexp(v.err.max(ex(v.v) + 4 - M::BITS) + 1);
    let lo = v.v.sub(e).round::<F>(mode);
    let hi = v.v.add(e).round::<F>(mode);
    (lo.0.to_bits() == hi.0.to_bits() && lo.1 == hi.1).then_some(lo)
}

/// Round a Bessel value (`signed` by `neg`): directly when it has no error bound (the
/// recurrences), else by Ziv's strategy, 384 bits and then 768 (`wide`).
fn round_bessel<F: Float>(v: Bounded<Mp>, bounded: bool, neg: bool, mode: Rounding, wide: impl FnOnce() -> Option<Bounded<mpw::Mp>>) -> (F, Status) {
    let v = Bounded { v: signed(v.v, neg), err: v.err };
    if !bounded {
        return mp::round(v.v, mode);
    }
    if let Some(r) = decided::<F, Mp>(v, mode) {
        return r;
    }
    let Some(w) = wide() else {
        return mp::round(v.v, mode);
    };
    let w = Bounded { v: signed(w.v, neg), err: w.err };
    // Still undecided in 768 bits: never seen (the hardest case needs about 236 bits beyond the
    // cancellation); the wider value is the better approximation.
    decided::<F, mpw::Mp>(w, mode).unwrap_or_else(|| w.v.round::<F>(mode))
}

/// `x = k·π/2 + r` with `|r|` about `π/4` at most: in 384 bits below 2^29, by Payne–Hanek
/// (`mp::reduce_wide`) above.
fn reduce(a: &Arg) -> (i64, Mp) {
    if a.v.exp <= 29 {
        let k = (a.f * core::f64::consts::FRAC_2_PI + 0.5) as i64;
        (k, a.v.sub(mp::pi().ldexp(-1).mul(Mp::from_i64(k))))
    } else {
        mp::reduce_wide(a.sig, a.e)
    }
}

/// `(J_n(x), Y_n(x))` by Hankel's asymptotic expansion, for `x >= 100` and `n <= √x`, each
/// with a bound on its absolute error: the remainders of `P` and `Q` lie below the first
/// omitted term (real `x`, past the order's first terms: Watson §7.32), each kept term carries
/// at most `4k·2^-383`; the phase `χ` is off by less than `2^(e_x + 3 − 384)` (`k·π/2` below
/// 2^30) or `2^(8 − 384)` (Payne–Hanek), its sine and cosine and the amplitude add a few last
/// places.
fn hankel(n: u64, a: &Arg) -> (Bounded<Mp>, Bounded<Mp>) {
    let (pq, b) = hankel_pq(n, a);
    let (j, y) = pq;
    let ej = (b + ex(j).max(ex(y))).max(ex(j) + 8 - Mp::BITS);
    let ey = (b + ex(j).max(ex(y))).max(ex(y) + 8 - Mp::BITS);
    // `b` bounds the error relative to the amplitude, which is within a factor 2 of
    // max(|J|, |Y|) (J² + Y² = amp²·(P² + Q²), P near 1).
    (Bounded { v: j, err: ej + 2 }, Bounded { v: y, err: ey + 2 })
}

/// Hankel's `(J_n, Y_n)` and the exponent of their error relative to the amplitude.
fn hankel_pq(n: u64, a: &Arg) -> ((Mp, Mp), i64) {
    let x8 = a.v.mul_u64(8);
    let mu = Mp::from_i64(n as i64).sqr().mul_u64(4);
    let (mut p, mut q) = (Mp::from_i64(1), Mp::ZERO);
    let mut t = Mp::from_i64(1);
    let mut tmax = 1i64;
    let mut k = 1u64;
    loop {
        let odd = Mp::from_i64(2 * k as i64 - 1).sqr();
        let next = t.mul(mu.sub(odd)).div(x8).div_u64(k);
        // Stop at negligible terms, or where the terms start to grow again (the expansion's
        // error is then below the smallest term, about e^(−2x)).
        if next.is_zero() || negligible(next, p) || (k > n + 1 && next.cmp_abs(t).is_gt()) {
            t = next;
            break;
        }
        t = next;
        tmax = tmax.max(t.exp);
        match k % 4 {
            1 => q = q.add(t),
            2 => p = p.sub(t),
            3 => q = q.sub(t),
            _ => p = p.add(t),
        }
        k += 1;
    }
    // χ = x − (2n+1)π/4 = (k_x − n)π/2 + (r − π/4) where x = k_x π/2 + r.
    let (kx, r) = reduce(a);
    let theta = r.sub(mp::pi().ldexp(-2));
    let quad = (kx - (n % 4) as i64).rem_euclid(4);
    let (s, cs) = mp::sin_cos_small(theta);
    let (sin_chi, cos_chi) = match quad {
        0 => (s, cs),
        1 => (cs, s.neg()),
        2 => (s.neg(), cs.neg()),
        _ => (cs.neg(), s),
    };
    let amp = c(&t::MP_TWO_OVER_PI).div(a.v).sqrt();
    let j = amp.mul(p.mul(cos_chi).sub(q.mul(sin_chi)));
    let y = amp.mul(p.mul(sin_chi).add(q.mul(cos_chi)));
    // `t` is now the first omitted term (or the last kept one, when it was negligible).
    let e_pq = ex(t).max(tmax + 2 * bitlen(k) + 4 - Mp::BITS) + 1;
    let e_chi = if a.v.exp <= 29 { a.v.exp.max(0) + 3 } else { 8 } - Mp::BITS;
    // |P| + |Q| < 2 times the phase error, twice the remainder, the amplitude's few places.
    let b = (e_pq + 1).max(e_chi + 2).max(8 - Mp::BITS) + 1;
    ((j, y), b)
}

/// `[J_0(x), J_1(x), Y_0(x), Y_1(x)]` for `x > 0` (the Y values only when `want_y`).
fn bessel01(a: &Arg, want_y: bool) -> [Mp; 4] {
    let ((j0, y0), (j1, y1)) = if a.f < BESSEL_SERIES_MAX {
        (bessel_series_mp(0, a.v, want_y), bessel_series_mp(1, a.v, want_y))
    } else {
        (hankel(0, a), hankel(1, a))
    };
    [j0.v, j1.v, y0.v, y1.v]
}

/// The double-double value of an `Mp` within the `f64` range.
fn mp_to_dd(m: Mp) -> dd::Dd {
    let hi = m.to_f64();
    dd::Dd { hi, lo: m.sub(Mp::from_f64(hi)).to_f64() }
}

/// `v · 2^e` as an `Mp`.
fn dd_to_mp(v: dd::Dd, e: i64) -> Mp {
    Mp::from_f64(v.hi).add(Mp::from_f64(v.lo)).ldexp(e)
}

/// `F_n` from `F_0 = f0`, `F_1 = f1` by `F_{k+1} = (2k/x) F_k − F_{k−1}` (forward), stopping
/// once `|F|` exceeds `2^limit` (an overflow in the format). Here `x < n² <= 2^62`.
fn forward(n: u64, a: &Arg, f0: Mp, f1: Mp, limit: i64, mp_max: u64) -> Mp {
    if n == 0 {
        return f0;
    }
    if n <= mp_max {
        let two_over_x = a.v.recip().ldexp(1);
        let (mut p, mut b) = (f0, f1);
        for k in 1..n {
            let next = b.mul(two_over_x.mul_u64(k)).sub(p);
            p = b;
            b = next;
            if b.exp > limit {
                break;
            }
        }
        return b;
    }
    // Double-double with a common power-of-two scale: the true values are p, b times 2^scale.
    let mut scale = f0.exp.max(f1.exp);
    let (mut p, mut b) = (mp_to_dd(f0.ldexp(-scale)), mp_to_dd(f1.ldexp(-scale)));
    let tx = dd::Dd::from(2.0).div(mp_to_dd(a.v));
    for k in 1..n {
        let next = b.mul(tx.mul_f64(k as f64)).sub(p);
        p = b;
        b = next;
        if dd::abs(b.hi) > 1.0e200 {
            p = p.scale(dd::pow2(-664));
            b = b.scale(dd::pow2(-664));
            scale += 664;
            if scale > limit + 100 {
                break;
            }
        }
    }
    dd_to_mp(b, scale)
}

/// `J_n(x)` for `n >= x >= 100` by Miller's backward recurrence from far above `n`,
/// normalised by `J_0(x)` or `J_1(x)`, whichever is larger.
fn miller(n: u64, a: &Arg, j0: Mp, j1: Mp, mp_max: u64) -> Mp {
    let top = n + 64 + (8.0 * cbrt_est(a.f)) as u64;
    let use_j0 = j0.cmp_abs(j1).is_ge();
    if top <= mp_max {
        let two_over_x = a.v.recip().ldexp(1);
        let (mut above, mut cur) = (Mp::ZERO, Mp::from_i64(1).ldexp(-1000));
        let mut at_n = Mp::ZERO;
        for k in (1..=top).rev() {
            let prev = cur.mul(two_over_x.mul_u64(k)).sub(above);
            above = cur;
            cur = prev;
            if k - 1 == n {
                at_n = cur;
            }
        }
        // cur = b_0, above = b_1.
        return if use_j0 { at_n.mul(j0).div(cur) } else { at_n.mul(j1).div(above) };
    }
    let tx = dd::Dd::from(2.0).div(mp_to_dd(a.v));
    let (mut above, mut cur) = (dd::Dd::from(0.0), dd::Dd::from(1.0));
    let mut scale = 0i64; // the true b_k are the stored values times 2^scale
    let mut at_n = (dd::Dd::from(0.0), 0i64);
    for k in (1..=top).rev() {
        let prev = cur.mul(tx.mul_f64(k as f64)).sub(above);
        above = cur;
        cur = prev;
        if dd::abs(cur.hi) > 1.0e200 {
            cur = cur.scale(dd::pow2(-664));
            above = above.scale(dd::pow2(-664));
            scale += 664;
        }
        if k - 1 == n {
            at_n = (cur, scale);
        }
    }
    let jn = dd_to_mp(at_n.0, at_n.1);
    if use_j0 { jn.mul(j0).div(dd_to_mp(cur, scale)) } else { jn.mul(j1).div(dd_to_mp(above, scale)) }
}

/// An estimate of `∛x` for `x >= 1` (Newton's iteration).
fn cbrt_est(x: f64) -> f64 {
    let mut y = x.max(1.0);
    for _ in 0..1000 {
        let next = (2.0 * y + x / (y * y)) / 3.0;
        if next >= y {
            break;
        }
        y = next;
    }
    y
}

/// `J_n(x)` for `n >= 0`, `x > 0`; recurrences stop beyond `2^limit`.
/// The value comes with an error bound (`true`) from the series and Hankel's expansion, without
/// one from the recurrences.
fn jn_pos(n: u64, a: &Arg, limit: i64, mp_max: u64) -> (Bounded<Mp>, bool) {
    if a.f < BESSEL_SERIES_MAX {
        return (bessel_series_mp(n, a.v, false).0, true);
    }
    if (n as f64) * (n as f64) <= a.f {
        return (hankel(n, a).0, true);
    }
    let [j0, j1, ..] = bessel01(a, false);
    let v = if (n as f64) < a.f { forward(n, a, j0, j1, limit, mp_max) } else { miller(n, a, j0, j1, mp_max) };
    (Bounded { v, err: 0 }, false)
}

/// `Y_n(x)` for `n >= 0`, `x > 0` (a value beyond `2^limit` stands for an overflow). `big` is
/// the logarithm beyond which `|Y_n|` overflows the format whatever the estimate's error.
fn yn_pos(n: u64, a: &Arg, limit: i64, big: f64, mp_max: u64) -> (Bounded<Mp>, bool) {
    if a.f < BESSEL_SERIES_MAX {
        // |Y_n| ≈ (n−1)! (2/x)^n / π: an overflow far beyond the format needs no series.
        if n >= 2 {
            let nf = n as f64;
            let est = (nf - 0.5) * ln_est(nf) - nf + 0.92 + nf * (LN_2 - ln_mp(a.v));
            if est > big {
                return (Bounded { v: Mp::from_i64(-1).ldexp(1 << 21), err: 0 }, false);
            }
        }
        return (bessel_series_mp(n, a.v, true).1, true);
    }
    if (n as f64) * (n as f64) <= a.f {
        return (hankel(n, a).1, true);
    }
    let [_, _, y0, y1] = bessel01(a, true);
    (Bounded { v: forward(n, a, y0, y1, limit, mp_max), err: 0 }, false)
}

/// An `f64` estimate of `ln x` for `x > 0` (about 1e-12).
fn ln_est(x: f64) -> f64 {
    let b = x.to_bits();
    let e = ((b >> 52) & 0x7ff) as i64 - 1023;
    let m = f64::from_bits((b & ((1u64 << 52) - 1)) | 0x3ff0_0000_0000_0000);
    let s = (m - 1.0) / (m + 1.0);
    let s2 = s * s;
    let (mut sum, mut p, mut k) = (0.0, s, 1.0);
    for _ in 0..30 {
        sum += p / k;
        p *= s2;
        k += 2.0;
    }
    2.0 * sum + e as f64 * LN_2
}

/// An `f64` estimate of `ln v` for a positive `Mp` of any exponent.
fn ln_mp(v: Mp) -> f64 {
    ln_est(Mp { neg: false, exp: 0, m: v.m }.to_f64()) + v.exp as f64 * LN_2
}

/// `j0(x)`.
pub fn j0<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    jn(0, x, mode)
}

/// `j1(x)`.
pub fn j1<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    jn(1, x, mode)
}

/// `y0(x)`.
pub fn y0<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    yn(0, x, mode)
}

/// `y1(x)`.
pub fn y1<F: Float>(x: F, mode: Rounding) -> (F, Status) {
    yn(1, x, mode)
}

/// The exponent beyond which a recurrence has overflowed the format.
fn limit<F: Float>() -> i64 {
    i64::from(F::EMAX) + 100
}

/// `jn(n, x)`: `J_{−n} = (−1)^n J_n` and `J_n(−x) = (−1)^n J_n(x)`.
pub fn jn<F: Float>(n: i32, x: F, mode: Rounding) -> (F, Status) {
    let d = float::decode(x);
    if let Some(r) = elem::nan_in(&d) {
        return r;
    }
    let m = u64::from(n.unsigned_abs());
    let odd = m % 2 == 1;
    // The sign: (−1)^n for n < 0, and again for x < 0.
    let neg = (odd && n < 0) != (odd && d.neg);
    match d.kind {
        // The zero limits carry the sign of the symmetries (glibc's convention).
        // glibc's `_Float128` forms give +0 for orders ±1 at ±∞ (`special_f128_products.rs`).
        Kind::Inf => return (float::zero(neg && !(m == 1 && F::PREC > 64)), Status::OK),
        Kind::Zero => return if m == 0 { (one(false), Status::OK) } else { (float::zero(neg), Status::OK) },
        _ => {}
    }
    let a = Arg::of(&d);
    if m == 0 && d.top() < -60 {
        // 1 − x²/4 with x² below 2^-120: just below 1.
        return near_one(false, false, mode);
    }
    if m >= 1 && d.top() < -180 {
        if let Some(r) = jn_tiny::<F>(m, &d, neg, mode) {
            return r;
        }
    }
    let (v, bounded) = jn_pos(m, &a, limit::<F>(), mp_recurrence_max::<F>());
    round_bessel::<F>(v, bounded, neg, mode, || bessel_wide(m, &a, false))
}

/// `J_n(x)` for `|x| < 2^-180`, `n >= 1`: `L·(1 − x²/(4(n+1)) + ...)` with `L = (x/2)^n / n!`,
/// the correction below 2^-362 of `L` and beyond the series' reach (below 2^-392 it drops the
/// term). Where `L` is a dyadic number (always for `n = 1, 2`, and whenever the odd part of `n!`
/// divides `odd(x)^n`, as for `J_3(3·2^-201)`) it is computed exactly and the result is just
/// inside it; for `n >= 92` the value underflows every format. `None`: `L` is not dyadic (the
/// series' value is then rounded as usual) or not computable exactly here.
fn jn_tiny<F: Float>(m: u64, d: &Num, neg: bool, mode: Rounding) -> Option<(F, Status)> {
    if m >= 92 {
        // |x/2|^n < 2^-181n <= 2^-16652.
        return Some(elem::underflow(neg, mode));
    }
    let tz = d.sig.trailing_zeros();
    let odd = d.sig >> tz; // up to 113 bits
    let e = i64::from(d.exp) + i64::from(tz); // |x| = odd · 2^e
    // odd^n exactly (at most 380 bits), then divided exactly by each odd prime power of n!.
    if u64::from(128 - odd.leading_zeros()) * m > 380 {
        return None;
    }
    let odd_mp = Mp::from_u128(false, odd, 0);
    let mut p = Mp::from_i64(1);
    for _ in 0..m {
        p = p.mul(odd_mp);
    }
    let mut v2 = 0i64; // the power of two in n!
    for k in 2..=m {
        let (mut q, mut twos) = (k, 0i64);
        while q % 2 == 0 {
            q /= 2;
            twos += 1;
        }
        v2 += twos;
        // The odd part of k: divide it out of odd^n, which must hold it.
        let mut f = 3;
        while f <= q {
            while q % f == 0 {
                let r = p.div_u64(f);
                if r.mul_u64(f) != p {
                    return None;
                }
                p = r;
                q /= f;
            }
            f += 2;
        }
    }
    // L = odd^n / odd(n!) · 2^(e·n − n − v2), exact.
    let l = signed(p.ldexp(e * m as i64 - m as i64 - v2), neg);
    // Just inside L: its value with the bits beyond 128 as sticky when it has any, else beside it.
    if l.m[..mp::LIMBS - 2].iter().any(|&w| w != 0) {
        Some(mp::round(l, mode))
    } else {
        Some(beside(l, false, mode))
    }
}

/// `yn(n, x)`: `Y_{−n} = (−1)^n Y_n`; a negative `x` is a domain error, zero a pole.
pub fn yn<F: Float>(n: i32, x: F, mode: Rounding) -> (F, Status) {
    let d = float::decode(x);
    if let Some(r) = elem::nan_in(&d) {
        return r;
    }
    let m = u64::from(n.unsigned_abs());
    let neg = m % 2 == 1 && n < 0;
    match d.kind {
        Kind::Zero => return elem::pole(!neg),
        // glibc's sign of the limit: −0 for order −1 (that is, −Y_1), +0 for every other order.
        Kind::Inf if !d.neg => return (float::zero(n == -1), Status::OK),
        _ => {}
    }
    if d.neg {
        return domain_nan::<F>();
    }
    let a = Arg::of(&d);
    let (v, bounded) = yn_pos(m, &a, limit::<F>(), ln_tiny::<F>() + 50.0, mp_recurrence_max::<F>());
    round_bessel::<F>(v, bounded, neg, mode, || bessel_wide(m, &a, true))
}
