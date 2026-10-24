//! The oracle of the complex functions (spec §10.5, D-10.12): both parts of each function at
//! finite operands, from the real oracles of [`mp`] and exact [`Big`] arithmetic, with a
//! proven relative error bound per part.
//!
//! The formulas are chosen so that nothing cancels, and they differ where they can from the
//! library's (`liella-math`'s `complex`): `csqrt` from `hypot`; the inverse functions from
//! `hypot` of the exact `x ± 1`; `ctanh`'s real part directly as `sinh x cosh x / D` at a
//! precision that resolves its distance from ±1 for `|x| < 20`, and beyond as `±(1 − δ)` with the
//! deviation `δ` from an exact identity (`tanh_far`), at no extra precision (`|x|` above 400, 6000
//! for the wide formats, is evaluated at that cap: the real
//! part's deviation from ±1 keeps the sign of `−cos 2y` and the imaginary part stays below every
//! format's smallest subnormal, so every format rounds both as at the true operand); the real part
//! of `ccos` and `ccosh` at operands below `2^-999` as `1 + D` with the deviation `D` summed from
//! its exact series (`near_one`); `cpow` with
//! absolute error bounds through `c ln|z| − d arg z` and `d ln|z| + c arg z`, so a cancellation
//! there raises the working precision instead of passing unnoticed. Exact parts are recognised
//! (`csqrt` by squaring a 120-bit candidate, `clog10` by `log10`'s exact powers of ten, `cpow` of
//! small integer powers by exact multiplication).
//!
//! Each part is an [`Approx`]: exact, or within `2^-prec` relatively. Operands must be finite;
//! the special values are compared against the reference directly (`complex_products.rs`).

use std::cmp::Ordering;

use crate::mp::{self, Approx, Big, Round};

/// A value with a bound `k` on its relative error in units of `2^-w` (`k = 0`: exact).
#[derive(Clone, Debug)]
struct V {
    v: Big,
    k: f64,
}

fn ex(v: Big) -> V {
    V { v, k: 0.0 }
}

fn of(a: Approx) -> V {
    V { k: if a.exact { 0.0 } else { 1.0 }, v: a.value }
}

fn is_exact_zero(a: &V) -> bool {
    a.k == 0.0 && a.v.is_zero()
}

fn mul(a: &V, b: &V) -> V {
    if is_exact_zero(a) || is_exact_zero(b) {
        return ex(a.v.mul(&b.v));
    }
    V { v: a.v.mul(&b.v), k: a.k + b.k + if a.k + b.k > 0.0 { 1.0 } else { 0.0 } }
}

fn div(a: &V, b: &V, w: u32) -> V {
    if is_exact_zero(a) {
        return ex(a.v.div(&b.v, w, Round::Nearest));
    }
    let q = a.v.div(&b.v, w, Round::Nearest);
    let exact = a.k == 0.0 && b.k == 0.0 && q.mul(&b.v).identical(&a.v);
    V { v: q, k: if exact { 0.0 } else { a.k + b.k + 2.0 } }
}

/// The sum of two values of one sign (the relative error cannot grow).
fn add_pos(a: &V, b: &V) -> V {
    V { v: a.v.add(&b.v), k: a.k.max(b.k) }
}

fn sqrt(a: &V, w: u32) -> V {
    let s = a.v.sqrt(w, Round::Nearest);
    let exact = a.k == 0.0 && s.mul(&s).identical(&a.v);
    V { v: s, k: if exact { 0.0 } else { a.k / 2.0 + 1.0 } }
}

fn neg(a: V) -> V {
    V { v: a.v.neg(), k: a.k }
}

fn with_sign(a: V, neg_: bool) -> V {
    if a.v.is_neg() != neg_ { neg(a) } else { a }
}

fn half(a: V) -> V {
    V { v: a.v.mul_pow2(-1), k: a.k }
}

fn small(v: f64) -> Big {
    Big::from_f64(v)
}

/// `atan2` of inexact operands: `k` of each adds (the angle's condition number is below 1).
fn atan2v(y: &V, x: &V, w: u32) -> V {
    let a = mp::atan2(&y.v, &x.v, w);
    if a.exact {
        return ex(a.value);
    }
    V { v: a.value, k: y.k + x.k + 1.0 }
}

/// `ln(1 + t)` of an inexact `t` (condition number below 1).
fn log1pv(t: &V, w: u32) -> V {
    let a = mp::log1p(&t.v, w);
    if a.exact {
        return ex(a.value);
    }
    V { v: a.value, k: t.k + 1.0 }
}

/// Both parts of `func` at finite operands, at working precision `w`.
fn eval(func: &str, ops: &[Big], w: u32, wide: bool) -> Option<(V, V)> {
    let x = ops.first()?;
    let y = ops.get(1)?;
    let f = |a: Approx| of(a);
    Some(match func {
        "cexp" => {
            let e = f(mp::exp(x, w));
            (mul(&e, &f(mp::cos(y, w))), mul(&e, &f(mp::sin(y, w))))
        }
        "clog" | "clog10" => {
            let n = x.mul(x).add(&y.mul(y));
            let t = n.sub(&Big::one());
            let re = if x.is_zero() && y.is_zero() { ex(Big::inf(true)) } else { half(f(mp::log1p(&t, w))) };
            let im = f(mp::atan2(y, x, w));
            if func == "clog" {
                (re, im)
            } else {
                let l10 = V { v: mp::ln10(w), k: 1.0 };
                let a10 = mp::log10(&n, w);
                let re = if a10.exact || n.is_zero() { half(ex(a10.value)) } else { div(&re, &l10, w) };
                let im = if im.k == 0.0 { im } else { div(&im, &l10, w) };
                (re, im)
            }
        }
        "csqrt" => csqrt(x, y, w),
        "csin" => (mul(&f(mp::sin(x, w)), &f(mp::cosh(y, w))), mul(&f(mp::cos(x, w)), &f(mp::sinh(y, w)))),
        "ccos" => (cos_cosh(x, y, w), neg(mul(&f(mp::sin(x, w)), &f(mp::sinh(y, w))))),
        "csinh" => (mul(&f(mp::sinh(x, w)), &f(mp::cos(y, w))), mul(&f(mp::cosh(x, w)), &f(mp::sin(y, w)))),
        "ccosh" => (cos_cosh(y, x, w), mul(&f(mp::sinh(x, w)), &f(mp::sin(y, w)))),
        "ctanh" => tanh_parts(x, y, w, wide),
        "ctan" => {
            let (h, t) = tanh_parts(y, x, w, wide);
            (t, h)
        }
        "casin" | "cacos" | "casinh" | "cacosh" => asin_family(func, x, y, w),
        "catanh" => atanh_core(x, y, w),
        "catan" => {
            let (r, i) = atanh_core(&y.neg(), x, w);
            (i, neg(r))
        }
        "cpow" => cpow(x, y, ops.get(2)?, ops.get(3)?, w)?,
        _ => return None,
    })
}

fn csqrt(x: &Big, y: &Big, w: u32) -> (V, V) {
    if y.is_zero() {
        let r = sqrt(&ex(x.abs()), w);
        return if x.is_neg() { (ex(Big::zero(false)), with_sign(r, y.is_neg())) } else { (r, ex(y.clone())) };
    }
    let h = of(mp::hypot(x, y, w));
    let t = sqrt(&half(add_pos(&h, &ex(x.abs()))), w);
    let o = div(&ex(y.abs()), &V { v: t.v.mul_pow2(1), k: t.k }, w);
    let (u, v) = if x.is_neg() { (o, t) } else { (t, o) };
    // An exact root: both parts are short dyadic numbers.
    let (uc, vc) = (u.v.round(120, Round::Nearest), v.v.round(120, Round::Nearest));
    let same = |p: &Big, q: &Big| p.cmp(q) == Some(Ordering::Equal);
    if same(&uc.mul(&uc).sub(&vc.mul(&vc)), x) && same(&uc.mul(&vc).mul_pow2(1), &y.abs()) {
        return (ex(uc), with_sign(ex(vc), y.is_neg()));
    }
    (u, with_sign(v, y.is_neg()))
}

/// `(sinh h cosh h / D, sin t cos t / D)`, `D = sinh² h + cos² t` (`|h|` beyond the cap at ±cap).
fn tanh_parts(h: &Big, t: &Big, w: u32, wide: bool) -> (V, V) {
    // Beyond the cap, e^(−2|h|) lies below half the smallest subnormal of the format and far
    // beyond its last places next to ±1, so capping h leaves every rounding unchanged (the sign of
    // the deviation comes from cos 2t). 400 (e^-800 < 2^-1154) covers float and double; the wide
    // formats need 6000 (e^-12000 < 2^-17310, below x87's and _Float128's 2^-16446, 2^-16495):
    // ctanl(2^-70 + 11357i) has real part ≈ 2^-32838, not the 2^-1223 of a cap at 400. The cap
    // sets the working precision (2.9 bits per unit of h), so the narrow formats keep 400.
    let cap = small(if wide { 6000.0 } else { 400.0 });
    let h = if h.cmp_abs(&cap) == Ordering::Greater { if h.is_neg() { cap.neg() } else { cap } } else { h.clone() };
    if h.cmp_abs(&small(FAR)) != Ordering::Less {
        return tanh_far(&h, t, w);
    }
    tanh_direct(&h, t, w)
}

/// Where [`tanh_parts`] takes the real part's deviation from ±1 as its own quantity
/// ([`tanh_far`]); below it the direct quotient costs at most `2.9 · 20 = 58` extra bits.
const FAR: f64 = 20.0;

/// [`tanh_parts`] by the direct quotients, the real part at `2.9 |h|` extra bits (it lies within
/// about `e^(−2|h|)` of ±1).
fn tanh_direct(h: &Big, t: &Big, w: u32) -> (V, V) {
    let w = w + (h.to_f64().abs() * 2.9) as u32;
    let (sh, ch) = (of(mp::sinh(h, w)), of(mp::cosh(h, w)));
    let (s, c) = (of(mp::sin(t, w)), of(mp::cos(t, w)));
    let d = add_pos(&mul(&sh, &sh), &mul(&c, &c));
    (div(&mul(&sh, &ch), &d, w), div(&mul(&s, &c), &d, w))
}

/// [`tanh_parts`] for `|h| >= FAR`: the real part as `sgn(h) (1 − δ)` with `δ` computed as its
/// own quantity, so no working precision grows with `|h|`.
///
/// The identity, with `a = |h|`, `C = cos 2t`, `E = e^(−2a)`: `2 sinh² a = cosh 2a − 1` and
/// `2 cos² t = 1 + C` give `2D = cosh 2a + C`; `2 sinh a cosh a = sinh 2a` gives the real part
/// `sgn(h) sinh 2a / (cosh 2a + C)` and the imaginary part `sin 2t / (cosh 2a + C)`. Since
/// `cosh 2a − sinh 2a = E`, `1 − sinh 2a / (cosh 2a + C) = (C + E) / (cosh 2a + C) = δ`, exactly.
///
/// Bounds (`a >= 20`): `cosh 2a + C >= e^(2a)/2 − 1 > (e^(2a)/2)(1 − 2^-56)` and `|C + E| <= 1 + E`,
/// so `|δ| < 2.0001 e^(−2a) < 2^-e0` with `e0 = floor(2a log2 e) − 3` (the `f64` product is off
/// by far less than the margin of 1 bit beyond 2.0001 < 4). `C + E` cancels only where
/// `cos 2t` is near `−E`; its relative error is `(|C| + E)/|C + E|` units, which `k` carries, and it
/// is never exactly zero: `C + E = 0` is `e^(2it) + e^(−2it) + 2 e^(−2a) = 0`, a vanishing sum of
/// exponentials of the distinct algebraic numbers `±2it`, `−2a` (`t != 0`, `a != 0`, both
/// rational) with nonzero algebraic coefficients, which Lindemann–Weierstrass excludes; `t = 0`
/// gives `C = 1`. (A computed zero sum only raises `k`, and so the precision.) The denominator `cosh 2a + C` has relative
/// error at most `(1 + 2^-55)/(1 − 2^-56) < 2` units (`|C| < 2^-56 cosh 2a`).
///
/// `δ` is computed at `wd = max(w − e0, 0) + 64` bits with relative error `kδ 2^-wd`, so the
/// absolute error of `1 − δ` (exact `Big` subtraction) is below `kδ 2^-(wd + e0) <= kδ 2^-(w + 64)`,
/// and relative to `|1 − δ| > 1/2` below `kδ 2^-63` units of `2^-w`. In the imaginary part's
/// denominator `C` at `wd` bits adds `2^-wd` absolute against `cosh 2a > 2^(e0 + 1)`, under
/// `2^-(w + 65)` relative, so that denominator stays within 2 units of `2^-w`.
fn tanh_far(h: &Big, t: &Big, w: u32) -> (V, V) {
    let a = h.abs();
    let (a2, t2) = (a.mul_pow2(1), t.mul_pow2(1));
    let e0 = ((a.to_f64() * 2.0 * std::f64::consts::LOG2_E).floor() as u32).saturating_sub(3);
    let wd = w.saturating_sub(e0) + 64;
    let c = of(mp::cos(&t2, wd));
    let e = of(mp::exp(&a2.neg(), wd));
    let n = if c.v.is_neg() {
        // Opposite signs: the sum is exact in `Big`; its error is that of the two terms.
        let sum = c.v.add(&e.v);
        let mag = c.v.abs().add(&e.v);
        let k = if sum.is_zero() { 2f64.powi(w.min(900) as i32) } else { mag.div(&sum.abs(), 64, Round::Up).to_f64() * c.k.max(e.k) + 1.0 };
        V { v: sum, k }
    } else {
        add_pos(&c, &e)
    };
    let den = |ch: Approx| V { v: ch.value.add(&c.v), k: 2.0 };
    let delta = div(&n, &den(mp::cosh(&a2, wd)), wd);
    let re = V { v: Big::one().sub(&delta.v), k: delta.k * 2f64.powi(-63) };
    let im = div(&of(mp::sin(&t2, w)), &den(mp::cosh(&a2, w)), w);
    (with_sign(re, h.is_neg()), im)
}

/// `cos u cosh v`, the real part of `ccos(u + iv)` and of `ccosh(v + iu)`: the product of the
/// real oracles, or next to 1 ([`near_one`]) the series of its deviation from 1.
fn cos_cosh(u: &Big, v: &Big, w: u32) -> V {
    let top = |a: &Big| if a.is_zero() { None } else { a.ilogb() };
    match top(u).max(top(v)) {
        Some(e) if e > NEAR_ONE_EXP => mul(&of(mp::cos(u, w)), &of(mp::cosh(v, w))),
        e => near_one(u, v, e, w),
    }
}

/// Where [`cos_cosh`] sums the series of [`near_one`]: both `|u|, |v| < 2^(NEAR_ONE_EXP + 1)`.
const NEAR_ONE_EXP: i64 = -1000;

/// `cos u cosh v = 1 + D` for `|u|, |v| < 2^(e + 1)`, `e <= NEAR_ONE_EXP` (`e = None`: both
/// zero), with `D` summed as its own quantity, so the value is within `2^-w` relatively at any
/// `w` however close to 1 it lies (at x87's `u = v = 2^-16445`, `D ≈ −u⁴/6 ≈ 2^-65783`).
///
/// The identity: with `z = u + iv`, `cos z = cos u cosh v − i sin u sinh v`, so `cos u cosh v =
/// Re cos z = Σ_{k>=0} (−1)^k Re(z^{2k})/(2k)!` and `D = Σ_{k>=1} (−1)^k Re(s^k)/(2k)!` with
/// `s = z² = (u² − v²) + 2uv i`, exact in `Big` (dyadic operands), as is every `Re(s^k)`.
///
/// Truncation: `|Re(s^k)| <= |z|^{2k}` and `|z|² = u² + v² < 2^(2e + 3) = 2^-L`, `L >= 1997`.
/// For `k > K` successive bounds `|z|^{2k}/(2k)!` shrink by `|z|²/((2k + 1)(2k + 2)) < 1/2`, so
/// the tail after `K` terms is below `2|z|^{2K+2}/(2K+2)! < 2^(1 − L(K+1))`, at most
/// `2^-(w + 2)` with `K = max(1, ceil((w + 3)/L) − 1)`.
///
/// The `K` terms: with `g_K = 1`, `g_{k−1} = g_k (2k)(2k − 1)` (so `g_k = (2K)!/(2k)!`), the
/// partial sum is `N / g_0`, `N = Σ_{k=1..K} (−1)^k Re(s^k) g_k`, all exact; one division at `w`
/// bits leaves a relative error below `2^-w` of a value below `|z|² < 2^-1997`, an absolute error
/// below `2^-(w + 1997)`. `1 + N/g_0` is an exact `Big` sum. Both errors together stay below
/// `2^-(w + 1)` absolutely and, against `cos u cosh v > 1 − |z|² > 1/2`, below `2^-w` relatively:
/// `k = 1` (`u = v = 0`: exactly 1).
fn near_one(u: &Big, v: &Big, e: Option<i64>, w: u32) -> V {
    let Some(e) = e else {
        return ex(Big::one());
    };
    let l = (-(2 * e + 3)) as u64;
    let terms = (u64::from(w) + 3).div_ceil(l).saturating_sub(1).max(1);
    let (sr, si) = (u.mul(u).sub(&v.mul(v)), u.mul(v).mul_pow2(1));
    let mut pw = Vec::with_capacity(terms as usize);
    let (mut pr, mut pi) = (sr.clone(), si.clone());
    for _ in 0..terms {
        pw.push(pr.clone());
        let nr = pr.mul(&sr).sub(&pi.mul(&si));
        pi = pr.mul(&si).add(&pi.mul(&sr));
        pr = nr;
    }
    let (mut n, mut g) = (Big::zero(false), Big::one());
    for k in (1..=terms).rev() {
        let t = pw[(k - 1) as usize].mul(&g);
        n = if k % 2 == 1 { n.sub(&t) } else { n.add(&t) };
        g = g.mul(&Big::from_u64((2 * k) * (2 * k - 1)));
    }
    V { v: Big::one().add(&n.div(&g, w, Round::Nearest)), k: 1.0 }
}

/// casin, cacos, casinh, cacosh from `A = (|z + 1| + |z − 1|)/2` (Hull, Fairgrieve and Tang).
fn asin_family(func: &str, x: &Big, y: &Big, w: u32) -> (V, V) {
    // casinh(x + iy) = −i casin(−y + ix); cacosh from cacos.
    let (a, b) = if func == "casinh" { (y, x) } else { (x, y) };
    let one = Big::one();
    let (aa, ba) = (a.abs(), b.abs());
    // (R, I, Q): casin(|a| + i|b|) = R + iI, Q = √((A − |a|)(A + |a|)).
    let (r, i, q) = if ba.is_zero() {
        if aa.cmp(&one) != Some(Ordering::Greater) {
            (of(mp::asin(&aa, w)), ex(Big::zero(false)), ex(Big::zero(false)))
        } else {
            (V { v: mp::pi(w).mul_pow2(-1), k: 1.0 }, of(mp::acosh(&aa, w)), ex(Big::zero(false)))
        }
    } else if aa.is_zero() {
        (ex(Big::zero(false)), of(mp::asinh(&ba, w)), ex(Big::zero(false)))
    } else {
        let b2 = ex(ba.mul(&ba));
        let (ap1, am1) = (aa.add(&one), aa.sub(&one));
        let rr = of(mp::hypot(&ap1, &ba, w));
        let ss = of(mp::hypot(&am1, &ba, w));
        let t1 = div(&b2, &add_pos(&rr, &ex(ap1.clone())), w);
        let (amo, amx) = if am1.is_neg() {
            let t2 = div(&b2, &add_pos(&ss, &ex(am1.neg())), w);
            let amo = half(add_pos(&t1, &t2));
            let amx = add_pos(&amo, &ex(am1.neg()));
            (amo, amx)
        } else {
            let amo = half(add_pos(&t1, &add_pos(&ss, &ex(am1.clone()))));
            let amx = half(add_pos(&t1, &div(&b2, &add_pos(&ss, &ex(am1.clone())), w)));
            (amo, amx)
        };
        let big_a = add_pos(&amo, &ex(one.clone()));
        let q = sqrt(&mul(&amx, &add_pos(&big_a, &ex(aa.clone()))), w);
        let i = log1pv(&add_pos(&amo, &sqrt(&mul(&amo, &add_pos(&big_a, &ex(one.clone()))), w)), w);
        (atan2v(&ex(aa.clone()), &q, w), i, q)
    };
    match func {
        "casin" => (with_sign(r, a.is_neg()), with_sign(i, b.is_neg())),
        "casinh" => (with_sign(i, x.is_neg()), with_sign(r, y.is_neg())),
        _ => {
            // cacos: Re = atan2(Q, x) (acos x on the real segment, 0 or π beyond); Im = ∓I.
            let re = if ba.is_zero() {
                if aa.cmp(&one) != Some(Ordering::Greater) {
                    of(mp::acos(a, w))
                } else if a.is_neg() {
                    V { v: mp::pi(w), k: 1.0 }
                } else {
                    ex(Big::zero(false))
                }
            } else if aa.is_zero() {
                V { v: mp::pi(w).mul_pow2(-1), k: 1.0 }
            } else {
                atan2v(&q, &ex(a.clone()), w)
            };
            if func == "cacos" {
                (re, with_sign(i, !b.is_neg()))
            } else {
                (i, with_sign(re, b.is_neg()))
            }
        }
    }
}

/// `catanh(x + iy) = ¼ ln(1 + 4|x|/((1 − |x|)² + y²)) + i ½ atan2(2y, (1 − x)(1 + x) − y²)`.
fn atanh_core(x: &Big, y: &Big, w: u32) -> (V, V) {
    let one = Big::one();
    let ax = x.abs();
    if y.is_zero() && ax.cmp(&one) == Some(Ordering::Equal) {
        return (ex(Big::inf(x.is_neg())), ex(y.clone()));
    }
    let den = one.sub(&ax).mul(&one.sub(&ax)).add(&y.mul(y));
    let q = div(&ex(ax.mul_pow2(2)), &ex(den), w);
    let l = log1pv(&q, w);
    let r = V { v: l.v.mul_pow2(-2), k: l.k };
    // (1 − x²) − y², exact: atan2 gives ±0, ±π/2 or ±π on the axes as Annex F says.
    let d = one.sub(&x.mul(x)).sub(&y.mul(y));
    (with_sign(r, x.is_neg()), half(of(mp::atan2(&y.mul_pow2(1), &d, w))))
}

/// `(cos, sin)` of `c·k·π/2^s`, the multiple of π reduced exactly: exact zeros and units at the
/// multiples of π/2, otherwise approximations whose bounds carry the reduced angle's error.
fn axis_angle(c: &Big, k: i64, s: i64, w: u32) -> (V, V) {
    let v = c.mul(&Big::from_i64(k)).mul_pow2(-s);
    let r = v.sub(&v.mul_pow2(-1).round_int().mul_pow2(1)); // in [−1, 1]
    let two_r = r.mul_pow2(1);
    if two_r.is_integer() {
        let t = two_r.to_i64().unwrap_or(0).rem_euclid(4); // r = t/2 (mod 2)
        let (cs, sn) = [(1, 0), (0, 1), (-1, 0), (0, -1)][t as usize];
        let unit = |s: i64| if s == 0 { ex(Big::zero(false)) } else { ex(Big::from_i64(s)) };
        return (unit(cs), unit(sn));
    }
    let arg = mp::pi(w).mul(&r);
    let (cv, sv) = (mp::cos(&arg, w), mp::sin(&arg, w));
    let at = arg.to_f64().abs();
    let tan = (sv.value.to_f64() / cv.value.to_f64()).abs();
    (V { v: cv.value, k: 1.0 + at * tan }, V { v: sv.value, k: 1.0 + at / tan.max(1e-300) })
}

/// `cpow(x + iy, c + id)` for finite operands, `z ≠ 0`.
fn cpow(x: &Big, y: &Big, c: &Big, d: &Big, w: u32) -> Option<(V, V)> {
    if x.is_zero() && y.is_zero() {
        return None;
    }
    // Small integer powers exactly.
    if d.is_zero() && c.is_integer() {
        if let Some(n) = c.to_i64().filter(|n| n.abs() <= 64 && *n != 0) {
            let (mut re, mut im) = (x.clone(), y.clone());
            for _ in 1..n.abs() {
                (re, im) = (re.mul(x).sub(&im.mul(y)), re.mul(y).add(&im.mul(x)));
            }
            if n > 0 {
                return Some((ex(re), ex(im)));
            }
            let nn = ex(re.mul(&re).add(&im.mul(&im)));
            return Some((div(&ex(re), &nn, w), div(&ex(im.neg()), &nn, w)));
        }
    }
    // A base on an axis: arg z = k·π/2, the angle c·k·π/2 reduced exactly. A real exponent
    // gives |z|^c; for |z| = 1 (−1, ±i) the magnitude is e^(−d·k·π/2).
    let on_axis = y.is_zero() || x.is_zero();
    let (m, k) = if y.is_zero() { (x.abs(), if !x.is_neg() { 0 } else if y.is_neg() { -2 } else { 2 }) } else { (y.abs(), if y.is_neg() { -1 } else { 1 }) };
    if on_axis && d.is_zero() {
        let mag = of(mp::pow(&m, c, w));
        if k == 0 {
            return Some((mag, ex(Big::zero(false))));
        }
        let (cs, sn) = axis_angle(c, k, 1, w);
        return Some((mul(&mag, &cs), mul(&mag, &sn)));
    }
    if on_axis && k != 0 && m.cmp(&Big::one()) == Some(Ordering::Equal) {
        let e = d.mul(&Big::from_i64(k)).mul(&mp::pi(w)).mul_pow2(-1).neg();
        let mag = V { k: 1.0 + e.to_f64().abs(), v: mp::exp(&e, w).value };
        let (cs, sn) = axis_angle(c, k, 1, w);
        return Some((mul(&mag, &cs), mul(&mag, &sn)));
    }
    // z^½ is the principal square root (exact for (3 + 4i)^½ = 2 + i).
    if d.is_zero() && c.cmp(&Big::from_f64(0.5)) == Some(Ordering::Equal) {
        return Some(csqrt(x, y, w));
    }
    // A base on a diagonal: arg z = k·π/4; |z|^c = (2x²)^(c/2).
    if d.is_zero() && !x.is_zero() && x.cmp_abs(y) == Ordering::Equal {
        let k = match (x.is_neg(), y.is_neg()) {
            (false, false) => 1,
            (true, false) => 3,
            (true, true) => -3,
            (false, true) => -1,
        };
        let mag = of(mp::pow(&x.mul(x).mul_pow2(1), &c.mul_pow2(-1), w));
        let (cs, sn) = axis_angle(c, k, 2, w);
        return Some((mul(&mag, &cs), mul(&mag, &sn)));
    }
    // The general case, with absolute error bounds through E and Φ.
    let t = x.mul(x).add(&y.mul(y)).sub(&Big::one());
    let l = half(of(mp::log1p(&t, w)));
    let th = of(mp::atan2(y, x, w));
    let e = c.mul(&l.v).sub(&d.mul(&th.v));
    let phi = d.mul(&l.v).add(&c.mul(&th.v));
    let abs_err = |p: &Big, q: &Big| p.mul(&l.v).to_f64().abs() * l.k + q.mul(&th.v).to_f64().abs() * th.k;
    let (ee, ep) = (abs_err(c, d), abs_err(d, c));
    // Beyond about 2^1000 the angle (or the exponent) is unknown at any precision this
    // evaluation reaches: undecided.
    if !(ee < 1e300 && ep < 1e300) {
        return None;
    }
    let mag = if e.is_zero() && ee == 0.0 { ex(Big::one()) } else { V { v: mp::exp(&e, w).value, k: 1.0 + ee } };
    if phi.is_zero() && ep == 0.0 {
        return Some((mag, ex(Big::zero(false))));
    }
    let (cv, sv) = (mp::cos(&phi, w), mp::sin(&phi, w));
    let tan = (sv.value.to_f64() / cv.value.to_f64()).abs();
    let kc = V { v: cv.value, k: 1.0 + ep * tan };
    let ks = V { v: sv.value, k: 1.0 + ep / tan.max(1e-300) };
    Some((mul(&mag, &kc), mul(&mag, &ks)))
}

/// Both parts of a complex function at finite operands (`ops`: the real and imaginary parts, and
/// for `cpow` the exponent's), each within `2^-prec` or exact; `None` for an unknown function or
/// `cpow` of zero (whose special values are compared with the reference directly).
pub fn complex(func: &str, ops: &[Big], prec: u32) -> Option<(Approx, Approx)> {
    complex_in(func, ops, prec, false)
}

/// [`complex`] for the wide exponent range of x87 long double and _Float128 (a larger far-out cap
/// for ctan and ctanh, at a higher working precision there).
pub fn complex_wide(func: &str, ops: &[Big], prec: u32) -> Option<(Approx, Approx)> {
    complex_in(func, ops, prec, true)
}

fn complex_in(func: &str, ops: &[Big], prec: u32, wide: bool) -> Option<(Approx, Approx)> {
    settle(prec, |w| eval(func, ops, w, wide))
}

/// Raises the working precision of `eval` until both parts are within `2^-prec`.
fn settle(prec: u32, eval: impl Fn(u32) -> Option<(V, V)>) -> Option<(Approx, Approx)> {
    let mut w = prec.saturating_add(16);
    for _ in 0..8 {
        let (re, im) = eval(w)?;
        let worst = re.k.max(im.k);
        if !worst.is_finite() || worst > 1e300 {
            return None;
        }
        let need = if worst <= 1.0 { 0 } else { worst.log2().ceil() as u32 };
        if w >= prec.saturating_add(need).saturating_add(2) {
            let part = |p: V| Approx { exact: p.k == 0.0, value: p.v };
            return Some((part(re), part(im)));
        }
        w = prec.saturating_add(need).saturating_add(16);
    }
    None
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
    use super::*;
    use crate::mp::{Fmt, correctly_rounded};

    /// The far-out real and imaginary parts by the identity of [`tanh_far`] round, in every
    /// direction and in both wide formats, exactly as the direct quotients at `2.9 |h|` extra
    /// bits do, on far-out operands where the direct quotients still finish quickly (`|h|` up
    /// to a few hundred), including `t` next to `π/4` (`cos 2t` near zero) and `π/2`.
    #[test]
    fn far_identity_rounds_as_direct_quotients() {
        let hs = [20.0, 20.5, 37.25, 64.0, -100.125, 211.0, 355.5];
        let ts = [0.5, -1.25, 3000.0, std::f64::consts::FRAC_PI_4, std::f64::consts::FRAC_PI_2, 1e-30, 0.0];
        let modes = [Round::Nearest, Round::Up, Round::Down, Round::Zero];
        let mut checked = 0;
        for &h in &hs {
            for &t in &ts {
                let (h, t) = (small(h), small(t));
                for fmt in [Fmt::F80, Fmt::F128] {
                    for mode in modes {
                        for j in 0..2 {
                            let pick = |r: Option<(Approx, Approx)>| {
                                let (a, b) = r.unwrap();
                                if j == 0 { a } else { b }
                            };
                            let old = correctly_rounded(fmt, mode, 1 << 14, |p| pick(settle(p, |w| Some(tanh_direct(&h, &t, w)))));
                            let new = correctly_rounded(fmt, mode, 1 << 14, |p| pick(settle(p, |w| Some(tanh_far(&h, &t, w)))));
                            let (old, new) = (old.expect("direct decides"), new.expect("far decides"));
                            assert_eq!(old, new, "h {h:?} t {t:?} {fmt:?} {mode:?} part {j}");
                            checked += 1;
                        }
                    }
                }
            }
        }
        assert_eq!(checked, 7 * 7 * 2 * 4 * 2);
    }

    /// The real part of ccos/ccosh next to 1 by the series of [`near_one`] rounds, in every
    /// direction and in both wide formats, exactly as the product `cos u · cosh v` does where the
    /// product still decides quickly (`|u|, |v|` near `2^-1000`, the deviation near `2^-2000` or,
    /// with `|u| = |v|`, `2^-4000`), and it decides x87's `u = v = ±2^-16445` (deviation about
    /// `2^-65783`) at 2^17 bits on the side below 1.
    #[test]
    fn near_one_series_rounds_as_the_product() {
        let us = [Big::pow2(-1001), Big::pow2(-1001).neg(), Big::from_f64(1.5).mul(&Big::pow2(-1003)), Big::zero(false)];
        let modes = [Round::Nearest, Round::Up, Round::Down, Round::Zero];
        let mut checked = 0;
        for u in &us {
            for v in &us {
                let e = [u, v].iter().filter(|a| !a.is_zero()).map(|a| a.ilogb().unwrap()).max();
                for fmt in [Fmt::F80, Fmt::F128] {
                    for mode in modes {
                        let prod = |w: u32| Some((mul(&of(mp::cos(u, w)), &of(mp::cosh(v, w))), ex(Big::zero(false))));
                        let series = |w: u32| Some((near_one(u, v, e, w), ex(Big::zero(false))));
                        let old = correctly_rounded(fmt, mode, 1 << 14, |p| settle(p, prod).unwrap().0);
                        let new = correctly_rounded(fmt, mode, 1 << 14, |p| settle(p, series).unwrap().0);
                        assert_eq!(old.expect("product decides"), new.expect("series decides"), "u {u:?} v {v:?} {fmt:?} {mode:?}");
                        checked += 1;
                    }
                }
            }
        }
        assert_eq!(checked, 4 * 4 * 2 * 4);
        let m = Big::pow2(-16445);
        for (func, x, y) in [("ccos", m.clone(), m.neg()), ("ccosh", m.neg(), m.clone())] {
            for mode in modes {
                let got = correctly_rounded(Fmt::F80, mode, 1 << 17, |p| complex_wide(func, &[x.clone(), y.clone()], p).unwrap().0);
                let want = Big::one().sub(&Big::pow2(-200)).round_to(Fmt::F80, mode);
                assert_eq!(got.expect("decides at 2^17").0, want.0, "{func} {mode:?}");
            }
        }
    }
}
