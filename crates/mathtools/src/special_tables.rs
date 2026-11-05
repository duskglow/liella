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
fn geometry(e0: i32, i: usize) -> (f64, f64, f64, f64) {
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

/// The cheapest `(m, d, eps)` meeting [`TARGET`] on every slot.
fn choose(slots: &[Slot]) -> (usize, usize, f64) {
    choose_for(slots, TARGET)
}

/// The cheapest `(m, d, eps)` meeting `target` on every slot (cost: about 12 operations a
/// double-double step, 2 an `f64` step).
pub(crate) fn choose_for(slots: &[Slot], target: f64) -> (usize, usize, f64) {
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

fn emit_table(src: &mut String, name: &str, doc: &str, slots: &[Slot]) {
    let (m, d, eps) = choose(slots);
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
    emit_table(src, "ERF_T", "`erf` on `[2^-5, 6)`, 16 slots a binade from `2^-5` (module `fast_special`).", &erf);
    let erfcx: Vec<Slot> = (0..76).map(|i| erfcx_slot(0, i, &k2)).collect();
    emit_table(src, "ERFCX_T", "`e^(x²)·erfc x` on `[1, 28)`, 16 slots a binade from 1 (module `fast_special`).", &erfcx);
}
