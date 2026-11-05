//! The tiered test oracle for `float` functions (tests only; ruling #2889). One call per operand
//! answers all four rounding modes: a double-double estimate `v` of `y = f(x)` with a proven
//! absolute bound `|y − v| ≤ err` decides a mode when no rounding boundary of that mode (the
//! `float`s for the directed modes, their midpoints for nearest) lies within `err` of `v`; only an
//! undecided mode asks the caller's accurate path (the 384-bit mp evaluation). That is the
//! accurate path's own Ziv structure, so the exhaustive sweeps cost about 10^3 instructions per
//! operand instead of some 2·10^5.
//!
//! The tier decides only normal, inexact results (flags: inexact; no errno): an estimate whose
//! `float` neighbourhood reaches the subnormal range, overflows, or straddles a boundary goes to
//! the accurate path, so the exact cases (a value on a boundary) are never decided here.
//!
//! A family adds its estimator below (`erf_est`, `erfc_est`) with its error analysis; the bounds
//! use `U`, a per-operation relative bound for the double-double operations (`dd`'s add, mul,
//! div and sqrt are near 2^-104; `U` keeps a factor 16 of slack), and `UE` for `exp_dd`.

// Test-only: float bit steps of normal values (never at 0 or the top), series indices below 2^17
// and counters below 2^40; `as f64` of those indices is exact. The negated comparisons are the
// NaN-rejecting form (`!(a > 0.0)` is true for NaN).
#![allow(clippy::as_conversions, clippy::arithmetic_side_effects, clippy::indexing_slicing, clippy::neg_cmp_op_on_partial_ord)]

use crate::dd::{self, Dd};
use crate::status::{Flags, Rounding, Status};
use core::sync::atomic::{AtomicU64, Ordering};

/// The modes in the order of the oracle's answers.
pub(crate) const MODES: [Rounding; 4] = [Rounding::Nearest, Rounding::Up, Rounding::Down, Rounding::Zero];

/// Relative error bound of one double-double operation (see the module comment).
pub(crate) const U: f64 = 7.888_609_052_210_118e-31; // 2^-100
/// Relative error bound of `exp_log::exp_dd` (the test of `tables_and_constants_are_the_rounded_values`
/// measures it below 2^-102 at ln 2; 2^-98 keeps slack).
pub(crate) const UE: f64 = 3.155_443_620_884_047e-30; // 2^-98

/// What the double-double tier knows about `y = f(x)`.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Est {
    /// `|y − (v.hi + v.lo)| ≤ err`.
    Near { v: Dd, err: f64 },
    /// `|y|` lies in `(F − s/2, F)` for the `float` `F > 0`, `s` the spacing just below `F`
    /// (erf beside ±1, erfc beside 2); `neg` gives the sign of `y`.
    Below { f: f32, neg: bool },
    /// The tier cannot say: every mode goes to the accurate path.
    Unknown,
}

/// How many operands and modes the oracle answered, and how many modes needed the accurate path.
#[derive(Default)]
pub(crate) struct Tally {
    pub operands: AtomicU64,
    pub mp_modes: AtomicU64,
}

impl Tally {
    /// `"<mp modes> of <modes> modes (<rate>)"`.
    pub(crate) fn summary(&self) -> std::string::String {
        let (n, mp) = (self.operands.load(Ordering::Relaxed) * 4, self.mp_modes.load(Ordering::Relaxed));
        std::format!("mp needed for {mp} of {n} modes ({:.3e})", mp as f64 / n.max(1) as f64)
    }
}

fn next_up(f: f32) -> f32 {
    f32::from_bits(f.to_bits() + 1)
}

fn next_down(f: f32) -> f32 {
    f32::from_bits(f.to_bits() - 1)
}

/// The mode for `|y|` when `y` has the sign `neg`.
fn magnitude_mode(m: Rounding, neg: bool) -> Rounding {
    match (m, neg) {
        (Rounding::Zero, _) => Rounding::Down,
        (Rounding::Up, true) => Rounding::Down,
        (Rounding::Down, true) => Rounding::Up,
        (m, _) => m,
    }
}

/// `y` rounded in mode `m` if `est` decides it.
pub(crate) fn decide(est: &Est, m: Rounding) -> Option<f32> {
    let (r, neg) = match *est {
        Est::Unknown => return None,
        Est::Below { f, neg } => {
            let r = match magnitude_mode(m, neg) {
                Rounding::Down => next_down(f),
                _ => f, // nearest: |y| is above the midpoint F − s/2; up: below F
            };
            if !(r > f32::MIN_POSITIVE) {
                return None;
            }
            (r, neg)
        }
        Est::Near { v, err } => {
            let neg = v.hi < 0.0;
            let (h, l) = if neg { (-v.hi, -v.lo) } else { (v.hi, v.lo) };
            if !(h < 3.0e38 && err >= 0.0) {
                return None;
            }
            let f = h as f32; // the float nearest h: the directed modes' boundary nearest h
            if !(f > f32::MIN_POSITIVE) {
                return None; // the cell below f reaches the subnormals (underflow flags): mp
            }
            let (fd, up, dn) = (f64::from(f), next_up(f), next_down(f));
            // |y − h| ≤ |l| + err must stay far inside the grid, so that only the boundary
            // nearest h can lie between h and y.
            if l.abs() + err > (f64::from(up) - fd).min(fd - f64::from(dn)) / 8.0 {
                return None;
            }
            // s = y − B up to err and its own rounding (2^-53·|s|; h − B is exact by Sterbenz).
            let clear = |s: f64| s.abs() * (1.0 - 1.0 / 4_503_599_627_370_496.0) > err;
            let r = match magnitude_mode(m, neg) {
                Rounding::Nearest => {
                    // The midpoint nearest h lies on h's side of f (the midpoints are exact in f64).
                    let (below, above) = if h >= fd { (f, up) } else { (dn, f) };
                    let mid = (f64::from(below) + f64::from(above)) / 2.0;
                    let s = (h - mid) + l;
                    if !clear(s) {
                        return None;
                    }
                    if s > 0.0 { above } else { below }
                }
                mm => {
                    let s = (h - fd) + l;
                    if !clear(s) {
                        return None;
                    }
                    match (mm, s > 0.0) {
                        (Rounding::Up, true) => up,
                        (Rounding::Up, false) | (_, true) => f,
                        _ => dn,
                    }
                }
            };
            if !(r > f32::MIN_POSITIVE) {
                return None;
            }
            (r, neg)
        }
    };
    Some(if neg { -r } else { r })
}

/// `f(x)` in every mode of [`MODES`]: the tier's answer where `est` decides, else `accurate(m)`.
pub(crate) fn oracle(est: Est, tally: &Tally, mut accurate: impl FnMut(Rounding) -> (f32, Status)) -> [(f32, Status); 4] {
    tally.operands.fetch_add(1, Ordering::Relaxed);
    MODES.map(|m| match decide(&est, m) {
        Some(r) => (r, Status::flags(Flags::INEXACT)),
        None => {
            tally.mp_modes.fetch_add(1, Ordering::Relaxed);
            accurate(m)
        }
    })
}

// ----------------------------------------------------------------------------- error function

/// 2/√π and 1/√π in double-double: 1/√π = 1/sqrt(π) from the table's π (relative error of π
/// about 2^-107, of sqrt and div each `U`): within 3U.
fn inv_sqrt_pi() -> Dd {
    Dd::from(1.0).div(dd::sqrt(Dd::of(crate::coeffs::tables::PI)))
}

/// `erf a` for `0 < a < 4` and its absolute bound: `2a/√π · e^(−a²) · S`, `S = Σ (2a²)^n /
/// (1·3···(2n+1))`, all terms positive. Term n carries 2n operations (≤ 2nU), each sum one (the
/// sum of N terms: ≤ NU more); the series stops at the first term T with (2n+3) > 2a² and
/// T/(1 − r) ≤ 2^-107·S, r = 2a²/(2n+3) the bound on every later ratio, so the tail is below
/// 2^-106·S. Then 1/√π (3U), exp_dd (UE) and three products (3U).
fn erf_series(a: f64) -> (Dd, f64) {
    let t2 = 2.0 * a * a; // exact: a has 24 bits
    let (mut sum, mut term, mut n) = (Dd::from(1.0), Dd::from(1.0), 1u64);
    loop {
        term = term.mul_f64(t2).div(Dd::from((2 * n + 1) as f64));
        let k = (2 * n + 3) as f64;
        if k > t2 && term.hi <= sum.hi * (1.0 - t2 / k) * 6.162_975_822_039_155e-33 {
            break; // 2^-107
        }
        sum = sum.add(term);
        n += 1;
    }
    let (e, m) = crate::exp_log::exp_dd(Dd::from(-a * a));
    let v = inv_sqrt_pi().scale(2.0).mul_f64(a).mul(e.scale(dd::pow2(m))).mul(sum);
    let rel = (3 * n + 10) as f64 * U + UE + 1.232_595_164_407_831e-32; // + 2^-106
    (v, rel * v.hi.abs() * (1.0 + 1e-9))
}

/// `erfc a` for `3 ≤ a < 10` and its absolute bound, by Laplace's continued fraction
/// `√π·e^(a²)·erfc a = 1/(a + (1/2)/(a + (2/2)/(a + (3/2)/(a + ...))))`. Its partial numerators
/// and denominators are positive, so consecutive convergents bracket the value: the gap between
/// the depth-n and depth-(n+1) convergents bounds the truncation. The backward evaluation does
/// not amplify a relative error (`a + c/t` passes at most t's relative error on), so each
/// convergent is within (2n + 1)U; then exp_dd (UE), 1/√π (3U) and two products (2U).
fn erfc_fraction(a: f64) -> (Dd, f64) {
    let conv = |n: u64| {
        let mut t = Dd::from(a);
        for k in (1..=n).rev() {
            t = Dd::from(k as f64 / 2.0).div(t).add_f64(a);
        }
        Dd::from(1.0).div(t)
    };
    let mut n = 16 + (4000.0 / (a * a)) as u64;
    let (k0, gap) = loop {
        let (k0, k1) = (conv(n), conv(n + 1));
        let gap = k0.sub(k1).hi.abs();
        if gap <= k0.hi * f64::from_bits(0x39f0_0000_0000_0000) || n > 1 << 16 {
            break (k0, gap); // 2^-96
        }
        n *= 2;
    };
    let (e, m) = crate::exp_log::exp_dd(Dd::from(-a * a));
    let w = inv_sqrt_pi().mul(e).mul(k0);
    let v = w.scale(dd::pow2(m));
    // k0 within (2n + 1)U; the computed gap within that and k1's (2n + 3)U of the true one.
    let rel = (4 * n + 10) as f64 * U + UE + gap / k0.hi * 1.000_001;
    (v, rel * v.hi.abs() * (1.0 + 1e-9))
}

/// The oracle's estimate of `erff(x)`. For |x| ≥ 4, erfc|x| < e^(−x²)/(|x|·√π) < 1.6·10^-8,
/// below half the spacing 2^-24 below 1 (the midpoint 1 − 2^-25 = 2.98·10^-8).
pub(crate) fn erf_est(x: f32) -> Est {
    let a = f64::from(x).abs();
    if !(a > 0.0) || a.is_infinite() {
        return Est::Unknown; // ±0, ±∞, NaN: exact or not inexact
    }
    if a >= 4.0 {
        return Est::Below { f: 1.0, neg: x < 0.0 };
    }
    let (v, err) = erf_series(a);
    Est::Near { v: if x < 0.0 { v.neg() } else { v }, err }
}

/// The oracle's estimate of `erfcf(x)`: 1 + erf|x| for x < 0 (beside 2 from −4: the spacing
/// below 2 is 2^-23), 1 − erf x below 3 (the subtraction's error is U·(1 + erf x) absolute) and
/// the continued fraction from 3 to 10.
pub(crate) fn erfc_est(x: f32) -> Est {
    let a = f64::from(x).abs();
    if !(a > 0.0) || !(f64::from(x) < 10.0) {
        return Est::Unknown;
    }
    if x < 0.0 {
        if a >= 4.0 {
            return Est::Below { f: 2.0, neg: false };
        }
        let (e, err) = erf_series(a);
        let v = Dd::from(1.0).add(e);
        return Est::Near { v, err: err + 2.0 * U * v.hi };
    }
    if a < 3.0 {
        let (e, err) = erf_series(a);
        let v = Dd::from(1.0).sub(e);
        return Est::Near { v, err: err + 2.0 * U * (1.0 + e.hi) };
    }
    let (v, err) = erfc_fraction(a);
    Est::Near { v, err }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The tier refuses every boundary: a value on a `float` decides nearest only, one on a
    /// midpoint decides the directed modes only, and a bound that reaches the boundary decides
    /// nothing; the sign maps up and down. (No spec clause: ruling #2889's oracle.)
    #[test]
    fn oracle_decides_only_clear_cells() {
        let near = |hi: f64, lo: f64, err: f64| Est::Near { v: dd::two_sum(hi, lo), err };
        let on_float = near(1.5, 0.0, 1e-30);
        assert_eq!(MODES.map(|m| decide(&on_float, m)), [Some(1.5), None, None, None]);
        let mid = 1.5 + 1.0 / 16_777_216.0; // 1.5 + 2^-24: halfway to the next float (spacing 2^-23)
        let on_mid = near(mid, 0.0, 1e-30);
        let (lo, hi) = (1.5f32, f32::from_bits(1.5f32.to_bits() + 1));
        assert_eq!(MODES.map(|m| decide(&on_mid, m)), [None, Some(hi), Some(lo), Some(lo)]);
        let above_mid = near(mid, 1e-20, 1e-21);
        assert_eq!(MODES.map(|m| decide(&above_mid, m)), [Some(hi), Some(hi), Some(lo), Some(lo)]);
        let reaching = near(mid, 1e-20, 1e-20);
        assert_eq!(decide(&reaching, Rounding::Nearest), None);
        let below_float = near(-1.5, 1e-20, 1e-21); // y just above −1.5
        let nb = -f32::from_bits(1.5f32.to_bits() - 1);
        assert_eq!(MODES.map(|m| decide(&below_float, m)), [Some(-1.5), Some(nb), Some(-1.5), Some(nb)]);
        let beside = Est::Below { f: 1.0, neg: true };
        let p1 = -f32::from_bits(1.0f32.to_bits() - 1);
        assert_eq!(MODES.map(|m| decide(&beside, m)), [Some(-1.0), Some(p1), Some(-1.0), Some(p1)]);
        let tiny = near(f64::from(f32::MIN_POSITIVE) * (1.25 + 1.0 / 1_073_741_824.0), 0.0, 0.0);
        assert!(MODES.iter().all(|&m| decide(&tiny, m).is_some()));
        let at_min = near(f64::from(f32::MIN_POSITIVE) * 1.000_000_01, 0.0, 0.0);
        assert!(MODES.iter().all(|&m| decide(&at_min, m).is_none()), "a cell touching the subnormals");
    }

    /// The estimates hold their bounds against the 384-bit mp erf and erfc: every 997th `float`
    /// in [2^-26, 10), both signs (No spec clause: ruling #2889's oracle.)
    #[test]
    fn erf_estimates_are_within_their_bounds() {
        use crate::mp::Mp;
        let mut checked = 0;
        for b in (0x3280_0000u32..0x4120_0000).step_by(997).flat_map(|b| [b, b | 1 << 31]) {
            let x = f32::from_bits(b);
            let (e, c) = crate::special::erf_erfc_mp(f64::from(x).abs());
            let neg = x < 0.0;
            let (fe, fc) = (if neg { e.neg() } else { e }, if neg { Mp::from_i64(2).sub(c) } else { c });
            for (est, want, name) in [(erf_est(x), fe, "erf"), (erfc_est(x), fc, "erfc")] {
                match est {
                    Est::Near { v, err } => {
                        let d = want.sub(Mp::from_f64(v.hi)).sub(Mp::from_f64(v.lo)).to_f64().abs();
                        assert!(d <= err, "{name}({x:e}): error {d:e} above the bound {err:e}");
                        assert!(err <= v.hi.abs() * 2f64.powi(-70), "{name}({x:e}): bound {err:e} too loose");
                        checked += 1;
                    }
                    Est::Below { f, .. } => {
                        let gap = Mp::from_f64(f64::from(f)).sub(if neg && name == "erf" { e } else { want }).to_f64();
                        let s = f64::from(f) - f64::from(next_down(f));
                        assert!(gap > 0.0 && gap < s / 2.0, "{name}({x:e}) beside {f}: {gap:e}");
                    }
                    Est::Unknown => assert!(name == "erfc" && f64::from(x) >= 10.0, "{name}({x:e}) unknown"),
                }
            }
        }
        assert!(checked > 20_000, "{checked}");
    }
}
