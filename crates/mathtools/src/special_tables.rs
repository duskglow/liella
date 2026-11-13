//! The Taylor tables of the `double` `erf`/`erfc` fast tier (`liella_math::fast_special`),
//! written to `crates/math/src/coeffs/erf.rs` by `cargo xtask tables`.
//!
//! **Slots.** An operand `x` in a table's range falls in the slot of its exponent and its four
//! leading fraction bits: `[2^e·(1 + j/16), 2^e·(1 + (j+1)/16))`. The slot's centre
//! `c = 2^e·(1 + (2j+1)/32)` has six significant bits and `|x − c| <= h = 2^e/32`; `x − c` is
//! exact (Sterbenz: `c/2 <= x <= 2c`). Each slot stores the Taylor coefficients `p_k` of its
//! function at `c` up to degree `D`, the first `M` as double-doubles (nearest `f64`, then the
//! nearest `f64` to the remainder), the rest as `f64`.
//!
//! * `ERF_T`, `x` in `[2^-5, 6)`: `erf` at `c`: `p_0 = erf c`, and for `k >= 1`
//!   `p_k = erf^(k)(c)/k! = (2/√π)·e^(−c²)·(−1)^(k−1)·H_(k−1)(c)/k!` (Hermite polynomials,
//!   `H_(n+1) = 2c·H_n − 2n·H_(n−1)`).
//! * `ERFCX_T`, `x` in `[1, 28)`: the scaled complement `Y(x) = e^(x²)·erfc x` at `c`:
//!   `Y' = 2xY − 2/√π` and, differentiating, `Y^(k+1) = 2x·Y^(k) + 2k·Y^(k−1)` (`k >= 1`).
//! * `ERF_SMALL`, `x < 2^-5`: `erf x = x·Σ q_n x^(2n)`, `q_n = (2/√π)(−1)^n/(n!(2n+1))`.
//!
//! **Bounds** (all relative to the smallest value of the function on the slot, `u = 2^-53`;
//! the generator evaluates each one per slot and keeps the largest as the table's `*_EPS`):
//!
//! 1. Truncation, `n = D + 1`: `|f^(n)(ξ)|·h^n/n!`. For `erf`, `|H_m(ξ)| <= K·2^(m/2)·√(m!)·e^(ξ²/2)`
//!    with `K = 1.086435` (Cramér's inequality; Abramowitz and Stegun 22.14.17), so
//!    `|erf^(n)(ξ)| <= (2/√π)·K·√(2^(n−1)·(n−1)!)·e^(−a²/2)` on the slot `[a, b]`. For `Y`,
//!    `Y(x) = (2/√π)∫₀^∞ e^(−t²−2xt) dt` gives `Y^(n)(x) = (2/√π)∫₀^∞ (−2t)^n e^(−t²−2xt) dt`,
//!    whose magnitude falls as `x` grows: `|Y^(n)(ξ)| <= |Y^(n)(a)|`, computed exactly here.
//! 2. The `f64` Horner steps `r_k = RN(RN(r_(k+1)·t) + p_k)` for `k = D..M`: with the running
//!    magnitudes `R_k = |p_k| + h·R_(k+1)`, `|r_k − V_k| <= h·|r_(k+1) − V_(k+1)| + u·(2R_k + |p_k|)`
//!    (two roundings and the coefficient's own), carried into the result times `h^M`.
//! 3. The double-double steps `S ← S·t + P_k` for `k = M−1..0` (one exact product, one exact sum,
//!    four roundings of terms below `3u·R_k`): below `18u²·R_k·h^k` each; the stored `P_k`:
//!    `u²·|p_k|·h^k`.
//!
//! The working precision is 640 bits: the `Y` recurrence cancels at most 11 bits a step at
//! `x = 28` (`2x²/k`), under 200 bits over 18 steps.

use std::fmt::Write as _;

use crate::mp::{self, Big, Round};
use crate::tables::{dd, f64_of};

const P: u32 = 640;
pub(crate) const U: f64 = 1.0 / 9_007_199_254_740_992.0;
/// The bound every table is sized for (relative): `2^-71`.
const TARGET: f64 = 4.235_164_736_271_502e-22;
pub(crate) const MAX_DEG: usize = 30;

pub(crate) struct Slot {
    pub(crate) h: f64,
    pub(crate) coef: Vec<Big>,
    pub(crate) mag: Vec<f64>,
    pub(crate) rem: Vec<f64>,
    pub(crate) fmin: f64,
}

fn round(x: &Big) -> Big {
    x.round(P, Round::Nearest)
}

fn fact(n: usize) -> f64 {
    (1..=n).fold(1.0, |a, k| a * k as f64)
}

/// The slot `i` of a table starting at `2^e0`: `(a, b, c, h)`.
pub(crate) fn geometry(e0: i32, i: usize) -> (f64, f64, f64, f64) {
    let e = e0 + (i / 16) as i32;
    let j = (i % 16) as f64;
    let s = 2f64.powi(e);
    (s * (1.0 + j / 16.0), s * (1.0 + (j + 1.0) / 16.0), s * (1.0 + (2.0 * j + 1.0) / 32.0), s / 32.0)
}

fn two_over_sqrt_pi() -> Big {
    let pi = mp::pi(P);
    Big::from_i64(2).div(&pi.sqrt(P, Round::Nearest), P, Round::Nearest)
}

fn erf_slot(e0: i32, i: usize, k2: &Big) -> Slot {
    let (a, _, c, h) = geometry(e0, i);
    let cb = Big::from_f64(c);
    let g = round(&k2.mul(&mp::exp(&cb.mul(&cb).neg(), P).value));
    let mut coef = vec![mp::erf(&cb, P).value];
    let (mut h0, mut h1) = (Big::one(), cb.mul_pow2(1));
    let mut kf = Big::one();
    for k in 1..=MAX_DEG {
        // H_(k−1)(c)
        let hk = if k == 1 { h0.clone() } else { h1.clone() };
        kf = kf.mul(&Big::from_i64(k as i64));
        let v = g.mul(&hk).div(&kf, P, Round::Nearest);
        coef.push(if k % 2 == 0 { v.neg() } else { v });
        if k >= 2 {
            let next = round(&cb.mul_pow2(1).mul(&h1).sub(&h0.mul(&Big::from_i64(2 * (k as i64 - 1)))));
            h0 = h1;
            h1 = next;
        }
    }
    let mag = coef.iter().map(|v| v.to_f64().abs()).collect();
    let k2f = k2.to_f64();
    let rem = (0..=MAX_DEG)
        .map(|d| {
            let n = d + 1;
            k2f * 1.086_435 * (2f64.powi(n as i32 - 1) * fact(n - 1)).sqrt() * (-a * a / 2.0).exp() * h.powi(n as i32) / fact(n)
        })
        .collect();
    let fmin = mp::erf(&Big::from_f64(a), P).value.to_f64();
    Slot { h, coef, mag, rem, fmin }
}

/// `Y^(k)(x)/k!` for `k = 0..=n` (exact recurrence in `P` bits).
fn erfcx_taylor(x: f64, n: usize, k2: &Big) -> Vec<Big> {
    let xb = Big::from_f64(x);
    let y0 = round(&mp::erfc(&xb, P).value.mul(&mp::exp(&xb.mul(&xb), P).value));
    let mut d = vec![y0.clone(), round(&xb.mul_pow2(1).mul(&y0).sub(k2))];
    for k in 1..n {
        let next = round(&xb.mul_pow2(1).mul(&d[k]).add(&d[k - 1].mul(&Big::from_i64(2 * k as i64))));
        d.push(next);
    }
    let mut kf = Big::one();
    d.iter()
        .enumerate()
        .map(|(k, v)| {
            if k > 0 {
                kf = kf.mul(&Big::from_i64(k as i64));
            }
            v.div(&kf, P, Round::Nearest)
        })
        .collect()
}

fn erfcx_slot(e0: i32, i: usize, k2: &Big) -> Slot {
    let (a, b, c, h) = geometry(e0, i);
    let coef = erfcx_taylor(c, MAX_DEG, k2);
    let mag = coef.iter().map(|v| v.to_f64().abs()).collect();
    let at_a = erfcx_taylor(a, MAX_DEG + 1, k2);
    let rem = (0..=MAX_DEG).map(|d| at_a[d + 1].to_f64().abs() * h.powi(d as i32 + 1)).collect();
    let fmin = erfcx_taylor(b, 0, k2)[0].to_f64();
    Slot { h, coef, mag, rem, fmin }
}

/// The relative bound of the evaluation with `m` double-double and `d + 1 − m` `f64`
/// coefficients (module doc, items 1-3).
pub(crate) fn bound(s: &Slot, m: usize, d: usize) -> f64 {
    let h = s.h;
    let mut rb = vec![0.0; d + 2];
    for k in (0..=d).rev() {
        rb[k] = s.mag[k] + h * rb[k + 1];
    }
    let mut e = 0.0;
    for k in (m..=d).rev() {
        e = h * e + U * (2.0 * rb[k] + s.mag[k]);
    }
    let mut tot = e * h.powi(m as i32) + s.rem[d];
    for (k, (r, g)) in rb.iter().zip(&s.mag).take(m).enumerate() {
        tot += (18.0 * r + g) * U * U * h.powi(k as i32);
    }
    tot * 1.01 / s.fmin
}

/// One `f64` Horner chain over the coefficients `lo..=hi` of `s` (item 2's step, the stored
/// coefficients `f64`): `(R, E)`, the magnitude of its value for `|t| <= h` and its error bound.
#[cfg(test)]
fn chain(s: &Slot, lo: usize, hi: usize) -> (f64, f64) {
    let h = s.h;
    let (mut r, mut e) = (0.0, 0.0);
    for k in (lo..=hi).rev() {
        r = s.mag[k] + h * r;
        e = h * e + U * (2.0 * r + s.mag[k]);
    }
    (r, e)
}

/// The relative bound of the quick phase's evaluation of an `ERF_T` row (degree 12;
/// `liella_math::fast_special`, "Quick phase"), every operation counted as a rounding (the SSE2
/// build; the FMA build rounds the same operations, its only difference is the exact product):
/// `t² = RN(t·t)`, `t⁴ = RN(t²·t²)` (`t⁴` within `3u` relative); `a = p3 + p4·t + p5·t²`,
/// `b1 = p6 + … + p9·t³`, `b2 = p10 + p11·t + p12·t²` by `f64` Horner (`P3` by its high part);
/// `b = RN(b1 + RN(t⁴·b2))`; `q = RN(RN(t·a) + RN(t⁴·b))`; `r = RN(P2h + q)`; `P0 + P1·t` with
/// `P1h·t` exact and one exact Fast2Sum (`None` when its order `|P0h| >= |P1h·t|` is not certain);
/// the low parts `s.lo`, `P0l`, the product's, `P1l·t`, `P2l·t²` (below `3.1u·(|p0| + h·|p1|)`) in
/// six roundings; then `RN(RN(t²·r) + low)`.
#[cfg(test)]
fn quick_bound(s: &Slot) -> Option<f64> {
    let (h, u) = (s.h, U);
    if s.mag[0] * (1.0 - 4.0 * u) < h * s.mag[1] * (1.0 + 4.0 * u) {
        return None;
    }
    let (ra, ea) = chain(s, 3, 5);
    let (r1, e1) = chain(s, 6, 9);
    let (r2, e2) = chain(s, 10, 12);
    let (h2, h4) = (h * h, h.powi(4));
    let rb = r1 + h4 * r2;
    let eb = e1 + h4 * (e2 + 4.0 * u * r2) + u * rb;
    let rq = h * ra + h4 * rb;
    let eq = h * (ea + u * ra) + h4 * (eb + 4.0 * u * rb) + u * rq;
    let rr = s.mag[2] + rq;
    let er = eq + u * rr;
    let top = s.mag[0] + h * s.mag[1] + h2 * s.mag[2];
    let low = 3.1 * u * top;
    let lo = h2 * (er + 2.0 * u * rr) + u * (h2 * rr + low) + 6.0 * u * low + u * u * top;
    Some((s.rem[12] + lo) * 1.01 / s.fmin)
}

/// The cheapest `(m, d, eps)` meeting `target` on every slot (cost: about 12 operations a
/// double-double step, 2 an `f64` step).
pub(crate) fn choose(slots: &[Slot], target: f64) -> (usize, usize, f64) {
    let mut best: Option<(usize, usize, usize, f64)> = None;
    for m in 1..=8 {
        for d in m..=MAX_DEG {
            let eps = slots.iter().map(|s| bound(s, m, d)).fold(0.0, f64::max);
            if eps <= target {
                let cost = 12 * m + 2 * (d - m);
                if best.is_none_or(|b| cost < b.0) {
                    best = Some((cost, m, d, eps));
                }
                break;
            }
        }
    }
    let (_, m, d, eps) = best.unwrap_or((0, 1, MAX_DEG, f64::INFINITY));
    assert!(eps <= target, "no layout meets the target");
    (m, d, eps)
}

pub(crate) fn emit_table(src: &mut String, name: &str, doc: &str, slots: &[Slot], target: f64) {
    let (m, d, eps) = choose(slots, target);
    let w = 2 * m + d + 1 - m;
    let _ = writeln!(src, "\n/// `{name}` degree `D` (module `fast_special`).\npub(crate) const {name}_DEG: usize = {d};");
    let _ = writeln!(src, "/// `{name}`: the first `M` coefficients are double-doubles.\npub(crate) const {name}_DD: usize = {m};");
    let _ = writeln!(
        src,
        "/// `{name}`'s relative error bound, {:.2} (log2), the largest over its slots (rounded up to an `f64`).\npub(crate) const {name}_EPS: u64 = {:#018x};",
        eps.log2(),
        (eps * (1.0 + 1e-9)).to_bits()
    );
    let _ = writeln!(src, "/// {doc}\npub(crate) static {name}: [[u64; {w}]; {}] = [", slots.len());
    for s in slots {
        src.push_str("    [");
        for k in 0..=d {
            if k < m {
                let (hi, lo) = dd(&s.coef[k]);
                let _ = write!(src, "{hi:#018x}, {lo:#018x}, ");
            } else {
                let _ = write!(src, "{:#018x}, ", f64_of(&s.coef[k]).to_bits());
            }
        }
        let _ = writeln!(src, "], // p0 {:e}", s.coef[0].to_f64());
    }
    src.push_str("];\n");
}

/// The small-operand series: `(coefficient bits, eps)` with `q0`, `q1` as double-doubles.
fn small(src: &mut String, k2: &Big) {
    let z = 2f64.powi(-10);
    let mut q = Vec::new();
    let mut kf = Big::one();
    for n in 0..20usize {
        if n > 0 {
            kf = kf.mul(&Big::from_i64(n as i64));
        }
        let v = k2.div(&kf.mul(&Big::from_i64(2 * n as i64 + 1)), P, Round::Nearest);
        q.push(if n % 2 == 1 { v.neg() } else { v });
    }
    let mag: Vec<f64> = q.iter().map(|v| v.to_f64().abs()).collect();
    let q0 = mag[0];
    let ns = (3..19).find(|&n| mag[n + 1] * z.powi(n as i32 + 1) <= 2f64.powi(-80) * q0).unwrap_or(18);
    let mut rb = vec![0.0; ns + 2];
    for n in (2..=ns).rev() {
        rb[n] = mag[n] + z * rb[n + 1];
    }
    let mut er = 0.0;
    for n in (2..=ns).rev() {
        er = z * er + U * (2.0 * rb[n] + mag[n]) + U * 1.01 * (n - 2) as f64 * mag[n] * z.powi(n as i32 - 2);
    }
    let tot = mag[ns + 1] * z.powi(ns as i32 + 1) + z * z * (er + 3.01 * U * rb[2]) + U * z * z * rb[2] + 23.0 * U * U * q0;
    let eps = tot * 1.01 / (q0 * (1.0 - z / 3.0));
    assert!(eps <= TARGET, "small-operand series {eps:e}");
    let _ = writeln!(
        src,
        "\n/// `ERF_SMALL`'s relative error bound, {:.2} (log2), for `x < 2^-5`.\npub(crate) const ERF_SMALL_EPS: u64 = {:#018x};",
        eps.log2(),
        (eps * (1.0 + 1e-9)).to_bits()
    );
    let _ = writeln!(src, "/// `erf x = x·Σ q_n x^(2n)`, `q_n = (2/√π)(−1)^n/(n!(2n+1))`: `q0`, `q1` as double-doubles, then `q2..q{ns}`.\npub(crate) static ERF_SMALL: [u64; {}] = [", ns + 3);
    for (n, v) in q.iter().enumerate().take(ns + 1) {
        if n < 2 {
            let (hi, lo) = dd(v);
            let _ = writeln!(src, "    {hi:#018x}, {lo:#018x}, // q{n}");
        } else {
            let _ = writeln!(src, "    {:#018x}, // q{n}", f64_of(v).to_bits());
        }
    }
    src.push_str("];\n");
}

/// The `erf`/`erfc` fast-tier tables (module doc).
pub fn emit(src: &mut String) {
    let k2 = two_over_sqrt_pi();
    src.push_str("\n// ---- erf/erfc fast tier (crates/mathtools/src/special_tables.rs has the derivation and bounds).\n");
    small(src, &k2);
    let erf: Vec<Slot> = (0..120).map(|i| erf_slot(-5, i, &k2)).collect();
    emit_table(src, "ERF_T", "`erf` on `[2^-5, 6)`, 16 slots a binade from `2^-5` (module `fast_special`).", &erf, TARGET);
    let erfcx: Vec<Slot> = (0..76).map(|i| erfcx_slot(0, i, &k2)).collect();
    emit_table(src, "ERFCX_T", "`e^(x²)·erfc x` on `[1, 28)`, 16 slots a binade from 1 (module `fast_special`).", &erfcx, TARGET);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::print_stdout)]
mod quick_tests {
    use super::*;

    /// The quick phase's stated bounds (`liella_math::fast_special`, "Quick phase", the same
    /// values): every `ERF_T` slot within `RHO_ERF_Q` = 2^-62 relative to `erf`, and on the slots
    /// below 1 the absolute bound over `erfc`'s least value (`1 − erf x`) within `RHO_ERFC_Q_LOW`
    /// = 2^-61. no spec clause (the bound behind spec §10.3's correct rounding in the quick phase).
    #[test]
    fn quick_phase_bounds() {
        let k2 = two_over_sqrt_pi();
        let (mut worst, mut low) = (0.0f64, 0.0f64);
        for i in 0..120 {
            let s = erf_slot(-5, i, &k2);
            let e = quick_bound(&s).unwrap();
            worst = worst.max(e);
            let (_, b, _, _) = geometry(-5, i);
            if b <= 1.0 {
                let fb = mp::erf(&Big::from_f64(b), P).value.to_f64();
                // e·fmin bounds the absolute error over the slot; erf <= fb there.
                low = low.max(e * s.fmin * (1.0 + 1e-9) / (1.0 - fb) + 2.0 * U * U);
            }
        }
        println!("quick erf 2^{:.3} erfc-low 2^{:.3}", worst.log2(), low.log2());
        assert!(worst <= 2f64.powi(-62), "erf quick bound 2^{}", worst.log2());
        assert!(low <= 2f64.powi(-61), "erfc quick low bound 2^{}", low.log2());
    }
}
