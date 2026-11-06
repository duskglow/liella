//! The tables of the `double` Bessel fast tier (`liella_math::fast_bessel`: `j0`, `j1`, `y0`,
//! `y1`), appended to `crates/math/src/coeffs/tables.rs` by `cargo xtask tables`.
//!
//! **Taylor tables** (`BESSEL_T`, `x` in `[2^-5, 32)`, 16 slots a binade, the slot geometry of
//! `special_tables`: centre `c`, half-width `h <= c/33`). The Taylor coefficients `a_k` of
//! `f(c + t)` follow from Bessel's equation `x²f'' + xf' + (x² − n²)f = 0`: the coefficient of
//! `t^k` gives
//! `c²(k+1)(k+2)a_(k+2) = −[c(k+1)(2k+1)a_(k+1) + (k² + c² − n²)a_k + 2c·a_(k−1) + a_(k−2)]`,
//! from `a_0 = f(c)` and `a_1 = f'(c)` (`J0' = −J1`, `Y0' = −Y1`, `J1' = J0 − J1/c`,
//! `Y1' = Y0 − Y1/c`), the four values from the oracle (`mp::bessel_j`, `mp::bessel_y`). The
//! recurrence runs in 640 bits and again in 960; the generator asserts that the two agree far
//! below every bound (`a_k·h^k` within `2^-300`).
//!
//! **Bounds, absolute** (the functions have zeros, so no bound relative to the slot's values
//! exists). Truncation after degree `D`: `Σ_(k=D+1..60) |a_k|·h^k` (computed) plus the tail
//! beyond 60 by Cauchy's estimate on the disc `|z − c| <= c/2` (in the right half-plane, where
//! all four functions are analytic): `|a_k| <= M/(c/2)^k`, so the tail is below
//! `M·q^61/(1 − q)`, `q = 2h/c <= 2/33`. `M` bounds `|f|` on that disc (`c/2 <= |z| <= 3c/2`,
//! `|Im z| <= c/2`): `|J_n(z)| <= |z/2|^n·e^(|Im z|)/n!` (DLMF 10.14.4), and the power series of
//! `Y0`, `Y1` (DLMF 10.8.1, 10.8.2; `H_k <= k`, `|ψ(k+1) + ψ(k+2)| <= 2k + 3`,
//! `|ln(z/2)| <= |ln|z/2|| + π`, `I_0(r) <= e^r`) give the cruder
//! `M = (4/c + 10(1 + c)^4·(2 + |ln(c/4)|))·e^(3c/2)` for all four; `q^61 < 10^-74` makes the tail
//! negligible. The evaluation's roundings are `special_tables::bound`'s (items 2 and 3 there)
//! with the slot minimum replaced by 1. Each row stores its own bound (last entry, rounded up by
//! `1 + 10^-9`); one layout `(M, D)` serves the four tables, the cheapest meeting `2^-74`.
//!
//! **Small operands** (`BESSEL_SMALL`, `x < 2^-5`, `z = x² < 2^-10`), power series in `z` (DLMF
//! 10.2.2, 10.8.1, 10.8.2): `J0 = A(z)`, `J1 = x·B(z)`, `Y0 = (2/π)·ln x·A(z) + C(z)`,
//! `Y1 = −(2/π)/x + (2/π)·ln x·J1 + x·D(z)` with
//! `A = Σ (−z/4)^k/(k!)²`, `B = ½Σ (−z/4)^k/(k!(k+1)!)`,
//! `C = (2/π)[(γ − ln2)·A + Σ_(k>=1) (−1)^(k+1)·H_k·(z/4)^k/(k!)²]`,
//! `D = −(2/π)·ln2·B − (1/(2π))·Σ (ψ(k+1) + ψ(k+2))·(−z/4)^k/(k!(k+1)!)`, eight terms each; the
//! generator asserts that the first omitted term is below `2^-110`.
//!
//! **Hankel's expansion** (`HANKEL_P`, `HANKEL_Q`, `x >= 32`, DLMF 10.17.3): `P = Σ (−1)^k·a_(2k)·w^k`,
//! `Q = (1/x)·Σ (−1)^k·a_(2k+1)·w^k`, `w = 1/x²`, `a_k(ν) = Π_(j=1..k)(4ν² − (2j−1)²)/(k!·8^k)`.
//! For real `ν <= 1` and `x > 0` each remainder is below its first omitted term (DLMF
//! 10.17(iii); Watson §7.32): the generator keeps `N` terms of each, `N` the least for which
//! both first omitted terms at `x = 32` are below `2^-77`.

use std::fmt::Write as _;

use crate::mp::{self, Big, Round};
use crate::special_tables::{bound, choose, Slot, MAX_DEG};
use crate::tables::{dd, euler_gamma, f64_of};

const P: u32 = 640;
const P2: u32 = 960;
/// Coefficients computed per slot.
const BK: usize = 60;
const SLOTS: usize = 160;
/// 2^-74.
const TARGET: f64 = 5.293_955_920_339_377e-23;

fn nr(x: &Big, p: u32) -> Big {
    x.round(p, Round::Nearest)
}

/// `[J0, J1, Y0, Y1]` Taylor coefficients at `c`, `BK + 1` each, in `p` bits.
fn taylor4(c: f64, p: u32) -> [Vec<Big>; 4] {
    let cb = Big::from_f64(c);
    let v = |y: bool, n: i64| if y { mp::bessel_y(n, &cb, p).value } else { mp::bessel_j(n, &cb, p).value };
    let (j0, j1, y0, y1) = (v(false, 0), v(false, 1), v(true, 0), v(true, 1));
    let start = [
        (j0.clone(), j1.neg(), 0i64),
        (j1.clone(), nr(&j0.sub(&j1.div(&cb, p, Round::Nearest)), p), 1),
        (y0.clone(), y1.neg(), 0),
        (y1.clone(), nr(&y0.sub(&y1.div(&cb, p, Round::Nearest)), p), 1),
    ];
    let c2 = cb.mul(&cb);
    start.map(|(f, df, n)| {
        let mut a = vec![f, df];
        for k in 0..BK - 1 {
            let kk = k as i64;
            let mut s = cb.mul(&Big::from_i64((kk + 1) * (2 * kk + 1))).mul(&a[k + 1]);
            s = s.add(&c2.add(&Big::from_i64(kk * kk - n * n)).mul(&a[k]));
            if k >= 1 {
                s = s.add(&cb.mul_pow2(1).mul(&a[k - 1]));
            }
            if k >= 2 {
                s = s.add(&a[k - 2]);
            }
            let next = s.neg().div(&c2.mul(&Big::from_i64((kk + 1) * (kk + 2))), p, Round::Nearest);
            a.push(next);
        }
        a
    })
}

fn bessel_slots(i: usize) -> [Slot; 4] {
    let e = -5 + (i / 16) as i32;
    let j = (i % 16) as f64;
    let s = 2f64.powi(e);
    let (c, h) = (s * (1.0 + (2.0 * j + 1.0) / 32.0), s / 32.0);
    let lo = taylor4(c, P);
    let hi = taylor4(c, P2);
    let q = 2.0 * h / c;
    let m = (4.0 / c + 10.0 * (1.0 + c).powi(4) * (2.0 + (c / 4.0).ln().abs())) * (1.5 * c).exp();
    let tail = m * q.powi(BK as i32 + 1) / (1.0 - q);
    let mut out = Vec::new();
    for (a, b) in lo.into_iter().zip(hi) {
        for (k, (x, y)) in a.iter().zip(&b).enumerate() {
            let d = x.sub(y).abs().to_f64() * h.powi(k as i32);
            assert!(d < 2f64.powi(-300), "recurrence unstable at c = {c}, k = {k}: {d:e}");
        }
        let mag: Vec<f64> = a.iter().map(|v| v.to_f64().abs()).collect();
        let rem = (0..=MAX_DEG).map(|d| (d + 1..=BK).map(|k| mag[k] * h.powi(k as i32)).sum::<f64>() + tail).collect();
        out.push(Slot { h, coef: a, mag, rem, fmin: 1.0 });
    }
    out.try_into().unwrap_or_else(|_| unreachable!())
}

fn emit_taylor(src: &mut String) {
    let slots: Vec<[Slot; 4]> = (0..SLOTS).map(bessel_slots).collect();
    let all: Vec<Slot> = slots.iter().flat_map(|s| s.iter().map(|x| Slot { h: x.h, coef: x.coef.clone(), mag: x.mag.clone(), rem: x.rem.clone(), fmin: 1.0 })).collect();
    let (m, d, eps) = choose(&all, TARGET);
    let w = 2 * m + d + 1 - m + 1;
    let _ = writeln!(src, "\n/// `BESSEL_T` degree `D` (module `fast_bessel`).\npub(crate) const BESSEL_T_DEG: usize = {d};");
    let _ = writeln!(src, "/// `BESSEL_T`: the first `M` coefficients are double-doubles.\npub(crate) const BESSEL_T_DD: usize = {m};");
    let _ = writeln!(src, "/// `BESSEL_T`'s largest per-slot absolute bound: {:.2} (log2).\npub(crate) const BESSEL_T_EPS_MAX: f64 = {eps:e};", eps.log2());
    let _ = writeln!(src, "/// `[J0, J1, Y0, Y1]` on `[2^-5, 32)`, 16 slots a binade from `2^-5`: Taylor rows, the slot's absolute bound last (module `fast_bessel`).\npub(crate) static BESSEL_T: [[[u64; {w}]; {SLOTS}]; 4] = [");
    for f in 0..4 {
        src.push_str("  [\n");
        for s in &slots {
            let s = &s[f];
            src.push_str("    [");
            for k in 0..=d {
                if k < m {
                    let (hi, lo) = dd(&s.coef[k]);
                    let _ = write!(src, "{hi:#018x}, {lo:#018x}, ");
                } else {
                    let _ = write!(src, "{:#018x}, ", f64_of(&s.coef[k]).to_bits());
                }
            }
            let _ = writeln!(src, "{:#018x}], // p0 {:e}", (bound(s, m, d) * (1.0 + 1e-9)).to_bits(), s.coef[0].to_f64());
        }
        src.push_str("  ],\n");
    }
    src.push_str("];\n");
}

fn dd_list(src: &mut String, v: &[Big]) {
    for x in v {
        let (hi, lo) = dd(x);
        let _ = writeln!(src, "    [{hi:#018x}, {lo:#018x}],");
    }
}

fn emit_small(src: &mut String, k: &Big, ln2: &Big, gamma: &Big) {
    const NS: usize = 8;
    let fact = |n: usize| (1..=n).fold(Big::one(), |a, j| a.mul(&Big::from_i64(j as i64)));
    let q4 = |kk: usize| Big::one().mul_pow2(-2 * kk as i64);
    let sgn = |v: Big, neg: bool| if neg { v.neg() } else { v };
    let harm = |n: usize| (1..=n).fold(Big::from_i64(0), |a, j| a.add(&Big::one().div(&Big::from_i64(j as i64), P, Round::Nearest)));
    let psi = |n: usize| harm(n - 1).sub(gamma);
    let a: Vec<Big> = (0..=NS).map(|j| sgn(q4(j).div(&fact(j).mul(&fact(j)), P, Round::Nearest), j % 2 == 1)).collect();
    let b: Vec<Big> = (0..=NS).map(|j| sgn(q4(j).div(&fact(j).mul(&fact(j + 1)), P, Round::Nearest).mul_pow2(-1), j % 2 == 1)).collect();
    let c: Vec<Big> = (0..=NS)
        .map(|j| {
            let s = if j == 0 { Big::from_i64(0) } else { sgn(harm(j).mul(&q4(j)).div(&fact(j).mul(&fact(j)), P, Round::Nearest), j % 2 == 0) };
            nr(&k.mul(&gamma.sub(ln2).mul(&a[j]).add(&s)), P)
        })
        .collect();
    let inv2pi = k.mul_pow2(-2);
    let d: Vec<Big> = (0..=NS)
        .map(|j| {
            let s = sgn(psi(j + 1).add(&psi(j + 2)).mul(&q4(j)).div(&fact(j).mul(&fact(j + 1)), P, Round::Nearest), j % 2 == 1);
            nr(&k.mul(ln2).mul(&b[j]).add(&inv2pi.mul(&s)).neg(), P)
        })
        .collect();
    let z = 2f64.powi(-10);
    for (name, v) in [("A", &a), ("B", &b), ("C", &c), ("D", &d)] {
        let t = v[NS].to_f64().abs() * z.powi(NS as i32);
        assert!(t < 2f64.powi(-110), "small series {name}: first omitted term {t:e}");
    }
    let _ = writeln!(src, "\n/// `[A, B, C, D]` of the small-operand Bessel series in `z = x²` (module `fast_bessel`), `{NS}` double-doubles each.\npub(crate) static BESSEL_SMALL: [[[u64; 2]; {NS}]; 4] = [");
    for v in [&a, &b, &c, &d] {
        src.push_str("  [\n");
        dd_list(src, &v[..NS]);
        src.push_str("  ],\n");
    }
    src.push_str("];\n");
}

fn emit_hankel(src: &mut String) {
    let x = 32.0f64;
    let coef = |nu: i64, kmax: usize| {
        let mut out = vec![Big::one()];
        let mut a = Big::one();
        for j in 1..=kmax {
            let jj = j as i64;
            a = a.mul(&Big::from_i64(4 * nu * nu - (2 * jj - 1) * (2 * jj - 1))).div(&Big::from_i64(8 * jj), P, Round::Nearest);
            out.push(a.clone());
        }
        out
    };
    let c: [Vec<Big>; 2] = [coef(0, 80), coef(1, 80)];
    let size = |k: usize| c.iter().map(|v| v[k].to_f64().abs() / x.powi(k as i32)).fold(0.0, f64::max);
    let n = (1..39).find(|&n| size(2 * n) < 2f64.powi(-77) && size(2 * n + 1) < 2f64.powi(-77)).unwrap_or(39);
    assert!(n < 39, "no Hankel term count meets 2^-77");
    let _ = writeln!(src, "\n/// Terms kept of Hankel's `P` and `Q` (module `fast_bessel`); first omitted at 32: {:.1} (log2).\npub(crate) const HANKEL_N: usize = {n};", size(2 * n).max(size(2 * n + 1)).log2());
    for (name, off) in [("HANKEL_P", 0), ("HANKEL_Q", 1)] {
        let _ = writeln!(src, "/// `(−1)^k·a_(2k+{off})(ν)` for `ν = 0, 1` (module `fast_bessel`).\npub(crate) static {name}: [[[u64; 2]; {n}]; 2] = [");
        for v in &c {
            src.push_str("  [\n");
            let row: Vec<Big> = (0..n).map(|k| if k % 2 == 1 { v[2 * k + off].neg() } else { v[2 * k + off].clone() }).collect();
            dd_list(src, &row);
            src.push_str("  ],\n");
        }
        src.push_str("];\n");
    }
}

/// The Bessel fast-tier tables (module doc).
pub fn emit(src: &mut String) {
    let pi = mp::pi(P);
    let k = Big::from_i64(2).div(&pi, P, Round::Nearest);
    let ln2 = mp::log(&Big::from_i64(2), P).value;
    let gamma = euler_gamma(P);
    src.push_str("\n// ---- j0/j1/y0/y1 fast tier (crates/mathtools/src/bessel_tables.rs has the derivation and bounds).\n");
    let (h, l) = dd(&k);
    let _ = writeln!(src, "/// `2/π` as a double-double.\npub(crate) const TWO_OVER_PI_DD: [u64; 2] = [{h:#018x}, {l:#018x}];");
    let (h, l) = dd(&Big::one().mul_pow2(-1).sqrt(P, Round::Nearest));
    let _ = writeln!(src, "/// `√½` as a double-double.\npub(crate) const SQRT_HALF_DD: [u64; 2] = [{h:#018x}, {l:#018x}];");
    emit_small(src, &k, &ln2, &gamma);
    emit_hankel(src);
    emit_taylor(src);
}
