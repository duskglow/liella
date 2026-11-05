//! The Taylor tables of the `double` `lgamma`/`tgamma` fast tier (`liella_math::fast_gamma`),
//! written to `crates/math/src/coeffs/gamma.rs` by `cargo xtask tables`.
//!
//! **Coefficients.** At `c > 0`, `ln Γ(c + t) = Σ p_k t^k` with `p_0 = ln Γ(c)`, `p_1 = ψ(c)` and
//! `p_k = (−1)^k ζ(k, c)/k` for `k >= 2` (`ψ^(n)(c) = (−1)^(n+1) n! ζ(n+1, c)`, Abramowitz and
//! Stegun 6.4.10; `ζ(k, c) = Σ_(j>=0) (c + j)^−k`). Each sum takes `N = 96` terms directly and the
//! Euler–Maclaurin tail at `a = c + N` with `B_2..B_20` (A&S 23.1.30); `ln Γ(a)` and `ψ(a)` by
//! Stirling's series (A&S 6.1.40, 6.3.18) with the same ten terms. The first omitted term is below
//! `2^-82·(c/a)^k` relative for `ζ` and `2^-120` absolute for `ln Γ` and `ψ`, inside the bounds'
//! 1.01 margin.
//!
//! **Tables** (16 slots a binade and the slot geometry of `special_tables`):
//! * `LG_T`: `ln Γ` on `[2^-5, 16)`. The four slots next to the zeros 1 and 2 (`[31/32, 1)`,
//!   `[1, 17/16)`, `[31/16, 2)`, `[2, 17/8)`) are stored but never read; their bound is excluded.
//! * `LG_Z1`, `LG_Z2`: one row each, `Q(t) = ln Γ(z + t)/t` at `z = 1` (`|t| <= 1/16`) and
//!   `z = 2` (`|t| <= 1/8`): coefficient `k` is `p_(k+1)` at `z`. `Q` has no zero there, so the
//!   bound is relative to `|Q|` and `t·Q` keeps it next to the zeros.
//!
//! **Bounds** (`special_tables` items 1-3, with these inputs):
//! * Truncation at degree `d`: `|p_n(ξ)| <= (c/(c − h))^n·|p_n(c)|` for `n >= 2` and `|ξ − c| <= h`
//!   (each term `(ξ + j)^−n <= (c − h + j)^−n <= (c/(c − h))^n (c + j)^−n`), so `|p_(d+1)|·h^(d+1)`
//!   with that factor; for `Q`, `|p_(d+2)(ξ)|·h^(d+1)`.
//! * The smallest value on a slot: `|p_0| − Σ_(k=1..30) |p_k| h^k − rem(30)` (Taylor's theorem).
//! * `h` is widened by `2^-40` relative: for `x` in `(−1/2, −2^-5]` the evaluator takes the slot of
//!   `RN(1 + x)`, so its exact `t` can exceed the half-width by `ulp/2 = 2^-54` (`fast_gamma`).
//!
//! The target is `2^-75` relative: `tgamma = e^(ln Γ)` turns `2^-75·|ln Γ|` (`|ln Γ| < 28`) into a
//! relative error below `2^-70`.

use crate::mp::{self, Big, Round};
use crate::special_tables::{emit_table, geometry, Slot, MAX_DEG};

const P: u32 = 640;
const N: i64 = 96;
/// 2^-75.
const TARGET: f64 = 2.646_977_960_169_688_6e-23;
/// `1 + 2^-40`.
const WIDEN: f64 = 1.0 + 1.0 / 1_099_511_627_776.0;
/// `B_2, B_4, …, B_20` (A&S table 23.2).
const BERN: [(i64, i64); 10] =
    [(1, 6), (-1, 30), (1, 42), (-1, 30), (5, 66), (-691, 2730), (7, 6), (-3617, 510), (43867, 798), (-174_611, 330)];
/// The four `LG_T` slots next to the zeros 1 and 2 (module doc).
const NEAR_ZERO: [usize; 4] = [79, 80, 95, 96];

fn rd(x: &Big) -> Big {
    x.round(P, Round::Nearest)
}

fn quo(a: &Big, b: &Big) -> Big {
    a.div(b, P, Round::Nearest)
}

/// `p_0..=p_kmax` of `ln Γ` at `c > 0` (module doc).
fn coefficients(c: &Big, kmax: usize) -> Vec<Big> {
    let one = Big::one();
    let mut zeta = vec![Big::zero(false); kmax + 1];
    let (mut psi, mut prod) = (Big::zero(false), Big::one());
    for j in 0..N {
        let v = c.add(&Big::from_i64(j));
        prod = rd(&prod.mul(&v));
        let w = quo(&one, &v);
        psi = psi.sub(&w);
        let mut pw = w.clone();
        for z in zeta.iter_mut().skip(2) {
            pw = rd(&pw.mul(&w));
            *z = z.add(&pw);
        }
    }
    let a = c.add(&Big::from_i64(N));
    let ia = quo(&one, &a);
    let ia2 = rd(&ia.mul(&ia));
    let ln_a = mp::log(&a, P).value;
    let half_ln_2pi = mp::log(&mp::pi(P).mul_pow2(1), P).value.mul_pow2(-1);
    let bern: Vec<Big> = BERN.iter().map(|&(n, d)| quo(&Big::from_i64(n), &Big::from_i64(d))).collect();
    let mut lg = rd(&a.sub(&Big::pow2(-1)).mul(&ln_a)).sub(&a).add(&half_ln_2pi);
    let mut ps = ln_a.sub(&ia.mul_pow2(-1));
    let mut pw = ia.clone(); // a^-(2i-1)
    for (i, b) in bern.iter().enumerate() {
        let i2 = 2 * (i as i64 + 1);
        let t = rd(&b.mul(&pw));
        lg = lg.add(&quo(&t, &Big::from_i64(i2 * (i2 - 1))));
        ps = ps.sub(&quo(&rd(&t.mul(&ia)), &Big::from_i64(i2)));
        pw = rd(&pw.mul(&ia2));
    }
    let mut out = vec![rd(&lg.sub(&mp::log(&prod, P).value)), rd(&ps.add(&psi))];
    let mut ak = ia.clone(); // a^-(k-1)
    for (k, zk) in zeta.iter().enumerate().skip(2) {
        let kk = k as i64;
        let mut tail = quo(&ak, &Big::from_i64(kk - 1)).add(&rd(&ak.mul(&ia)).mul_pow2(-1));
        let mut aw = rd(&ak.mul(&ia2)); // a^-(k-1)·a^-2i
        let (mut fact, mut rising) = (Big::from_i64(2), Big::from_i64(kk)); // (2i)!, k(k+1)…(k+2i−2)
        for (i, b) in bern.iter().enumerate() {
            if i > 0 {
                let i2 = 2 * (i as i64 + 1);
                fact = fact.mul(&Big::from_i64((i2 - 1) * i2));
                rising = rising.mul(&Big::from_i64((kk + i2 - 3) * (kk + i2 - 2)));
            }
            tail = tail.add(&quo(&rd(&b.mul(&rising).mul(&aw)), &fact));
            aw = rd(&aw.mul(&ia2));
        }
        let v = quo(&zk.add(&tail), &Big::from_i64(kk));
        out.push(if k % 2 == 1 { v.neg() } else { v });
        ak = rd(&ak.mul(&ia));
    }
    out
}

/// A slot for `coef` (the expansion of `f` at `c`, `f`'s coefficient `k` being `p_(k+shift)`),
/// half-width `h` (widened), `rem[d] = |p_(d+1+shift)(c)|·grow^(d+1+shift)·h^(d+1)`.
fn slot_of(p: &[Big], shift: usize, c: f64, h: f64, used: bool) -> Slot {
    let h = h * WIDEN;
    let coef: Vec<Big> = p[shift..].to_vec();
    let mag: Vec<f64> = coef.iter().map(|v| v.to_f64().abs()).collect();
    let grow = c / (c - h);
    let rem: Vec<f64> =
        (0..=MAX_DEG).map(|d| mag[d + 1] * grow.powi((d + 1 + shift) as i32) * h.powi(d as i32 + 1)).collect();
    let s: f64 = (1..=MAX_DEG).map(|k| mag[k] * h.powi(k as i32)).sum::<f64>() + rem[MAX_DEG];
    let fmin = mag[0] * (1.0 - 1e-12) - s * 1.001;
    assert!(!used || fmin > 0.0, "slot at {c} has no lower bound ({fmin:e})");
    Slot { h, coef, mag, rem, fmin: if used { fmin } else { f64::INFINITY } }
}

/// The `lgamma`/`tgamma` fast-tier tables (module doc).
pub fn emit(src: &mut String) {
    src.push_str("\n// ---- lgamma/tgamma fast tier (crates/mathtools/src/gamma_tables.rs has the derivation and bounds).\n");
    let lg: Vec<Slot> = (0..144)
        .map(|i| {
            let (_, _, c, h) = geometry(-5, i);
            slot_of(&coefficients(&Big::from_f64(c), MAX_DEG + 1), 0, c, h, !NEAR_ZERO.contains(&i))
        })
        .collect();
    emit_table(src, "LG_T", "`ln Γ` on `[2^-5, 16)`, 16 slots a binade from `2^-5` (module `fast_gamma`).", &lg, TARGET);
    for (name, z, h) in [("LG_Z1", 1, 0.0625), ("LG_Z2", 2, 0.125)] {
        let s = slot_of(&coefficients(&Big::from_i64(z), MAX_DEG + 2), 1, z as f64, h, true);
        let doc = format!("`ln Γ({z} + t)/t` for `|t| <= {h}` (module `fast_gamma`).");
        emit_table(src, name, &doc, &[s], TARGET);
    }
}
