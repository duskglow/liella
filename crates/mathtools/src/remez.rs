//! Minimax polynomial fitting by the Remez exchange algorithm, in [`mp`] arithmetic.
//!
//! Given `f` on `[a, b]`, a set of free powers and optionally some coefficients fixed in
//! advance, find the coefficients minimizing the maximum absolute or relative error. By the
//! equioscillation theorem the optimum is characterized by `n + 1` points (for `n` free
//! coefficients) where the error reaches its maximum magnitude with alternating signs. The
//! exchange algorithm:
//!
//! 1. start from the Chebyshev extrema of the interval as the reference;
//! 2. solve the linear system `p(x_i) - f(x_i) = (-1)^i E w_i` (`w = 1`, or `f(x_i)` for
//!    relative error) for the free coefficients and the levelled error `E`;
//! 3. locate the local extrema of the error on a dense grid, refine each by golden-section
//!    search, and keep `n + 1` alternating ones including the global maximum (multi-point
//!    exchange);
//! 4. repeat until the extrema equioscillate: `max|e_i| / min|e_i| - 1 < tolerance`.
//!
//! When the interval is symmetric and the powers and the target share a parity, the fit is
//! done on the right half (where the basis is a Haar system; on the whole interval it is not).
//!
//! Afterwards the coefficients are rounded to the storage format and the error of the rounded
//! polynomial is measured exactly (exact polynomial evaluation at points of the format, on a
//! dense grid and around every extremum). Everything is deterministic: the grid and constants
//! come from [`mp`], never from the platform's `libm`.
//!
//! Fit files (`key = value` lines, `#` comments) are described in [`parse_fit`].

use std::cmp::Ordering;
use std::fmt::Write as _;

use crate::mp::{self, Big, Fmt, Round};

/// Parity of a target function.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Parity {
    /// `f(-x) = -f(x)`.
    Odd,
    /// `f(-x) = f(x)`.
    Even,
    /// Neither (or unknown).
    Neither,
}

/// A target function from the registry: `eval(x, prec)` has relative error below `2^-prec`
/// (removable singularities handled exactly).
#[derive(Clone, Copy, Debug)]
pub struct Target {
    /// Name used in fit files.
    pub name: &'static str,
    /// Parity, used to fold symmetric fits.
    pub parity: Parity,
    /// The function.
    pub eval: fn(&Big, u32) -> Big,
}

fn ratio_at_zero(x: &Big, p: u32, num: fn(&Big, u32) -> mp::Approx) -> Big {
    if x.is_zero() {
        return Big::one();
    }
    num(x, p + 4).value.div(x, p + 4, Round::Nearest)
}

/// The registry of targets.
pub const TARGETS: &[Target] = &[
    Target { name: "exp", parity: Parity::Neither, eval: |x, p| mp::exp(x, p).value },
    Target { name: "expm1", parity: Parity::Neither, eval: |x, p| mp::expm1(x, p).value },
    Target { name: "expm1_over_x", parity: Parity::Neither, eval: |x, p| ratio_at_zero(x, p, mp::expm1) },
    Target { name: "exp2", parity: Parity::Neither, eval: |x, p| mp::exp2(x, p).value },
    Target { name: "log1p", parity: Parity::Neither, eval: |x, p| mp::log1p(x, p).value },
    Target { name: "log1p_over_x", parity: Parity::Neither, eval: |x, p| ratio_at_zero(x, p, mp::log1p) },
    Target {
        name: "log2_1p",
        parity: Parity::Neither,
        eval: |x, p| mp::log1p(x, p + 4).value.div(&mp::ln2(p + 8), p + 4, Round::Nearest),
    },
    Target { name: "sin", parity: Parity::Odd, eval: |x, p| mp::sin(x, p).value },
    Target { name: "sin_over_x", parity: Parity::Even, eval: |x, p| ratio_at_zero(x, p, mp::sin) },
    Target { name: "cos", parity: Parity::Even, eval: |x, p| mp::cos(x, p).value },
    Target { name: "tan", parity: Parity::Odd, eval: |x, p| mp::tan(x, p).value },
    Target { name: "atan", parity: Parity::Odd, eval: |x, p| mp::atan(x, p).value },
    Target { name: "atan_over_x", parity: Parity::Even, eval: |x, p| ratio_at_zero(x, p, mp::atan) },
    Target { name: "asin", parity: Parity::Odd, eval: |x, p| mp::asin(x, p).value },
    Target { name: "sinh", parity: Parity::Odd, eval: |x, p| mp::sinh(x, p).value },
    Target { name: "tanh", parity: Parity::Odd, eval: |x, p| mp::tanh(x, p).value },
    Target { name: "log", parity: Parity::Neither, eval: |x, p| mp::log(x, p).value },
    Target { name: "erf", parity: Parity::Odd, eval: |x, p| mp::erf(x, p).value },
];

/// Looks a target up by name.
pub fn target(name: &str) -> Option<&'static Target> {
    TARGETS.iter().find(|t| t.name == name)
}

/// A fitting problem.
pub struct Problem<'a> {
    /// The function, as for [`Target::eval`].
    pub f: &'a dyn Fn(&Big, u32) -> Big,
    /// Its parity.
    pub parity: Parity,
    /// Interval start.
    pub a: Big,
    /// Interval end.
    pub b: Big,
    /// Powers whose coefficients are fitted.
    pub free: Vec<u32>,
    /// Powers with coefficients fixed in advance.
    pub fixed: Vec<(u32, Big)>,
    /// Relative (true) or absolute error.
    pub relative: bool,
    /// Working precision in bits.
    pub prec: u32,
    /// Equioscillation tolerance: stop when `max|e_i| / min|e_i| - 1` is below it.
    pub tol: f64,
    /// Iteration limit.
    pub max_iter: u32,
}

impl<'a> Problem<'a> {
    /// A problem with the default precision (256 bits), tolerance (1e-6) and limit (40).
    pub fn new(f: &'a dyn Fn(&Big, u32) -> Big, parity: Parity, a: Big, b: Big, free: Vec<u32>, relative: bool) -> Problem<'a> {
        Problem { f, parity, a, b, free, fixed: Vec::new(), relative, prec: 256, tol: 1e-6, max_iter: 40 }
    }
}

/// The result of [`remez`].
#[derive(Clone, Debug)]
pub struct Fit {
    /// All coefficients (fixed and fitted), by ascending power.
    pub coeffs: Vec<(u32, Big)>,
    /// The levelled error `|E|` of the last solve.
    pub levelled: Big,
    /// Number of linear solves.
    pub iterations: u32,
    /// Final reference points with the error there (alternating in sign).
    pub extrema: Vec<(Big, Big)>,
    /// `max|e_i| / min|e_i| - 1` at the end.
    pub spread: f64,
    /// True if the fit was folded onto the right half of a symmetric interval.
    pub folded: bool,
}

/// Fitting context: the problem plus the (possibly folded) domain.
struct Ctx<'p, 'a> {
    pb: &'p Problem<'a>,
    lo: Big,
    hi: Big,
    p: u32,
    /// The lowest free power is odd and the domain straddles 0: every free basis function
    /// then changes sign at 0, and so does the error (the fixed part, typically `c0 = f(0)`,
    /// pins it to 0 there). Writing the error as `x^m (q(x) - g(x)) / w(x)`, the
    /// equioscillation is that of `q - g`, so the error is oriented by the sign of `x`.
    orient: bool,
}

impl<'p, 'a> Ctx<'p, 'a> {
    fn new(pb: &'p Problem<'a>, lo: Big, hi: Big) -> Ctx<'p, 'a> {
        let m = pb.free.iter().copied().min().unwrap_or(0);
        let orient = m % 2 == 1 && lo.is_neg() && !lo.is_zero() && !hi.is_neg() && !hi.is_zero();
        Ctx { pb, lo, hi, p: pb.prec.max(64), orient }
    }

    /// True where the orientation flips the sign of the error.
    fn flip(&self, x: &Big) -> bool {
        self.orient && x.is_neg()
    }
}

fn parity_of(powers: impl Iterator<Item = u32>) -> Parity {
    let (mut odd, mut even) = (false, false);
    for k in powers {
        if k % 2 == 1 { odd = true } else { even = true }
    }
    match (odd, even) {
        (true, false) => Parity::Odd,
        (false, true) => Parity::Even,
        _ => Parity::Neither,
    }
}

impl Ctx<'_, '_> {
    fn r(&self, x: &Big) -> Big {
        x.round(self.p, Round::Nearest)
    }

    /// `f(x)`, nudged off a zero of `f` for relative error (where the error is a limit).
    fn f_at(&self, x: &Big) -> (Big, Big) {
        let fx = (self.pb.f)(x, self.p + 16);
        if self.pb.relative && fx.is_zero() {
            let h = self.r(&self.pb.b.sub(&self.pb.a).mul_pow2(-60));
            let x2 = if x.add(&h).cmp(&self.hi) == Some(Ordering::Greater) { x.sub(&h) } else { x.add(&h) };
            let f2 = (self.pb.f)(&x2, self.p + 16);
            return (x2, f2);
        }
        (x.clone(), fx)
    }

    /// The polynomial at `p` bits.
    fn poly(&self, coeffs: &[(u32, Big)], x: &Big) -> Big {
        let mut acc = Big::zero(false);
        let mut xp = Big::one();
        let mut k = 0u32;
        for (pw, c) in coeffs {
            while k < *pw {
                xp = self.r(&xp.mul(x));
                k += 1;
            }
            acc = self.r(&acc.add(&c.mul(&xp)));
        }
        acc
    }

    /// The (oriented) error function at `x`.
    fn err(&self, coeffs: &[(u32, Big)], x: &Big) -> Big {
        let (x, fx) = self.f_at(x);
        let d = self.r(&self.poly(coeffs, &x).sub(&fx));
        let e = if self.pb.relative { d.div(&fx, self.p, Round::Nearest) } else { d };
        if self.flip(&x) { e.neg() } else { e }
    }

    /// All coefficients from the free ones.
    fn merge(&self, free: &[Big]) -> Vec<(u32, Big)> {
        let mut all: Vec<(u32, Big)> = self.pb.fixed.clone();
        all.extend(self.pb.free.iter().copied().zip(free.iter().cloned()));
        all.sort_by_key(|(k, _)| *k);
        all
    }

    /// Solves for the free coefficients and E on a reference.
    fn solve(&self, refs: &[Big]) -> Result<(Vec<Big>, Big), String> {
        let n = self.pb.free.len();
        if refs.len() != n + 1 {
            return Err(format!("{} reference points for {n} coefficients", refs.len()));
        }
        let mut m: Vec<Vec<Big>> = Vec::with_capacity(n + 1);
        for (i, x) in refs.iter().enumerate() {
            let (x, fx) = self.f_at(x);
            let mut row = Vec::with_capacity(n + 2);
            for &k in &self.pb.free {
                row.push(self.r(&pow_int(&x, k, self.p)));
            }
            // p - f = (-1)^i E w, with w = f for relative error and the orientation sign.
            let w = if self.pb.relative { fx.clone() } else { Big::one() };
            row.push(if (i % 2 == 0) != self.flip(&x) { w.neg() } else { w });
            let mut rhs = fx;
            for (k, c) in &self.pb.fixed {
                rhs = self.r(&rhs.sub(&c.mul(&pow_int(&x, *k, self.p))));
            }
            row.push(rhs);
            m.push(row);
        }
        let sol = gauss(m, self.p).ok_or("singular reference system")?;
        let (coef, e) = sol.split_at(n);
        Ok((coef.to_vec(), e[0].abs()))
    }

    /// Golden-section search for the maximum of `s * e(x)` on `[l, r]`.
    fn golden(&self, coeffs: &[(u32, Big)], l: (&Big, &Big), r: (&Big, &Big), neg: bool) -> (Big, Big) {
        let g = |e: &Big| if neg { e.neg() } else { e.clone() };
        let c = Big::from_i64(3).sub(&Big::from_i64(5).sqrt(self.p, Round::Nearest)).mul_pow2(-1);
        let (mut a, mut b) = (l.0.clone(), r.0.clone());
        let at = |a: &Big, b: &Big, t: &Big| self.r(&a.add(&b.sub(a).mul(t)));
        let mut x1 = at(&a, &b, &c);
        let mut x2 = at(&b, &a, &c);
        let mut e1 = self.err(coeffs, &x1);
        let mut e2 = self.err(coeffs, &x2);
        for _ in 0..80 {
            if g(&e1).cmp(&g(&e2)) == Some(Ordering::Less) {
                a = x1;
                x1 = x2;
                e1 = e2;
                x2 = at(&b, &a, &c);
                e2 = self.err(coeffs, &x2);
            } else {
                b = x2;
                x2 = x1;
                e2 = e1;
                x1 = at(&a, &b, &c);
                e1 = self.err(coeffs, &x1);
            }
        }
        let mut best = (l.0.clone(), l.1.clone());
        for (x, e) in [(r.0.clone(), r.1.clone()), (x1, e1), (x2, e2)] {
            if g(&e).cmp(&g(&best.1)) == Some(Ordering::Greater) {
                best = (x, e);
            }
        }
        best
    }

    /// The grid: `n` Chebyshev-distributed points on `[lo, hi]` (denser near the ends, where
    /// the error oscillates fastest), endpoints included.
    fn grid(&self, lo: &Big, hi: &Big, n: usize) -> Vec<Big> {
        let pi = mp::pi(80);
        let w = hi.sub(lo);
        (0..n)
            .map(|k| {
                let t = pi.mul(&Big::from_u64(k as u64)).div(&Big::from_u64((n - 1) as u64), 80, Round::Nearest);
                let c = Big::one().sub(&mp::cos(&t, 64).value).mul_pow2(-1).round(64, Round::Nearest);
                if k == 0 {
                    lo.clone()
                } else if k + 1 == n {
                    hi.clone()
                } else {
                    self.r(&lo.add(&w.mul(&c)))
                }
            })
            .collect()
    }

    /// `n + 1` alternating extrema of the error, including the global maximum.
    fn extrema(&self, coeffs: &[(u32, Big)], refs: &[Big]) -> Result<Vec<(Big, Big)>, String> {
        let want = self.pb.free.len() + 1;
        let mut xs = self.grid(&self.lo, &self.hi, (40 * want).max(400));
        xs.extend(refs.iter().cloned());
        xs.sort_by(|a, b| a.cmp(b).unwrap_or(Ordering::Equal));
        xs.dedup_by(|a, b| a.cmp(b) == Some(Ordering::Equal));
        let es: Vec<Big> = xs.iter().map(|x| self.err(coeffs, x)).collect();
        // Runs of one sign; the largest sample of each run.
        let mut picks: Vec<usize> = Vec::new();
        for i in 0..xs.len() {
            if es[i].is_zero() {
                continue;
            }
            match picks.last() {
                Some(&j) if es[j].is_neg() == es[i].is_neg() => {
                    if es[i].cmp_abs(&es[j]) == Ordering::Greater {
                        if let Some(l) = picks.last_mut() {
                            *l = i;
                        }
                    }
                }
                _ => picks.push(i),
            }
        }
        let mut ext: Vec<(Big, Big)> = picks
            .iter()
            .map(|&i| {
                let l = i.saturating_sub(1);
                let r = (i + 1).min(xs.len() - 1);
                self.golden(coeffs, (&xs[l], &es[l]), (&xs[r], &es[r]), es[i].is_neg())
            })
            .collect();
        // Refinement can only increase |e|; it keeps each sign.
        while ext.len() > want {
            if ext.len() == want + 1 {
                let last = ext.len() - 1;
                let drop = if ext[0].1.cmp_abs(&ext[last].1) == Ordering::Less { 0 } else { last };
                ext.remove(drop);
                continue;
            }
            let mut m = 0;
            for i in 1..ext.len() {
                if ext[i].1.cmp_abs(&ext[m].1) == Ordering::Less {
                    m = i;
                }
            }
            if m == 0 || m == ext.len() - 1 {
                ext.remove(m);
            } else {
                // Removing an interior extremum leaves its neighbours with one sign: keep the
                // larger of them.
                let drop = if ext[m - 1].1.cmp_abs(&ext[m + 1].1) == Ordering::Less { m - 1 } else { m + 1 };
                ext.remove(m.max(drop));
                ext.remove(m.min(drop));
            }
        }
        if ext.len() < want {
            let list: Vec<String> = ext.iter().map(|(x, e)| format!("{:.6e}: {:.3e}", x.to_f64(), e.to_f64())).collect();
            return Err(format!(
                "the error alternates only {} times where {want} are needed (extrema {}); the powers do not form a Haar system on this interval",
                ext.len(),
                list.join(", ")
            ));
        }
        Ok(ext)
    }
}

/// `x^k` at `p` bits.
fn pow_int(x: &Big, k: u32, p: u32) -> Big {
    let mut r = Big::one();
    for _ in 0..k {
        r = r.mul(x).round(p, Round::Nearest);
    }
    r
}

/// Gaussian elimination with partial pivoting on an augmented matrix, at `p` bits.
fn gauss(mut m: Vec<Vec<Big>>, p: u32) -> Option<Vec<Big>> {
    let n = m.len();
    let r = |x: Big| x.round(p, Round::Nearest);
    for col in 0..n {
        let piv = (col..n).max_by(|&a, &b| m[a][col].cmp_abs(&m[b][col]))?;
        if m[piv][col].is_zero() {
            return None;
        }
        m.swap(col, piv);
        let pivot = m[col].clone();
        for row in m.iter_mut().skip(col + 1) {
            let f = row[col].div(&pivot[col], p, Round::Nearest);
            if f.is_zero() {
                continue;
            }
            for (v, pv) in row.iter_mut().zip(pivot.iter()).skip(col) {
                *v = r(v.sub(&f.mul(pv)));
            }
        }
    }
    let mut x = vec![Big::zero(false); n];
    for row in (0..n).rev() {
        let mut s = m[row][n].clone();
        for k in row + 1..n {
            s = r(s.sub(&m[row][k].mul(&x[k])));
        }
        x[row] = s.div(&m[row][row], p, Round::Nearest);
    }
    Some(x)
}

fn spread(ext: &[(Big, Big)]) -> f64 {
    let mut mx = ext[0].1.abs();
    let mut mn = ext[0].1.abs();
    for (_, e) in ext {
        if e.cmp_abs(&mx) == Ordering::Greater {
            mx = e.abs();
        }
        if e.cmp_abs(&mn) == Ordering::Less {
            mn = e.abs();
        }
    }
    if mn.is_zero() {
        return f64::INFINITY;
    }
    mx.div(&mn, 64, Round::Nearest).to_f64() - 1.0
}

/// Runs the Remez exchange.
pub fn remez(pb: &Problem) -> Result<Fit, String> {
    if pb.free.is_empty() {
        return Err("no free coefficients".into());
    }
    if pb.a.cmp(&pb.b) != Some(Ordering::Less) {
        return Err("empty interval".into());
    }
    let powers = pb.free.iter().copied().chain(pb.fixed.iter().map(|(k, _)| *k));
    let pp = parity_of(powers);
    let folded = pb.a.neg().cmp(&pb.b) == Some(Ordering::Equal) && pp != Parity::Neither && pp == pb.parity;
    let lo = match (folded, pp) {
        (true, Parity::Odd) => pb.b.mul_pow2(-24), // all basis functions vanish at 0
        (true, _) => Big::zero(false),
        _ => pb.a.clone(),
    };
    let ctx = Ctx::new(pb, lo, pb.b.clone());
    let n = pb.free.len();
    // Chebyshev extrema of the (folded) interval. Where every free basis function vanishes at
    // 0, a reference point there carries no information: move it halfway to its neighbour.
    let mut refs: Vec<Big> = ctx.grid(&ctx.lo, &ctx.hi, n + 1);
    let tiny = ctx.hi.sub(&ctx.lo).mul_pow2(-30);
    if ctx.orient {
        if let Some(i) = refs.iter().position(|x| x.cmp_abs(&tiny) == Ordering::Less) {
            if let Some(next) = refs.get(i + 1) {
                refs[i] = next.mul_pow2(-1);
            }
        }
    }
    let mut iterations = 0;
    loop {
        let (free, e) = ctx.solve(&refs)?;
        iterations += 1;
        let coeffs = ctx.merge(&free);
        let ext = ctx.extrema(&coeffs, &refs)?;
        let s = spread(&ext);
        if s < pb.tol || iterations >= pb.max_iter {
            if s >= pb.tol {
                return Err(format!("no equioscillation after {iterations} iterations (spread {s:e})"));
            }
            return Ok(Fit { coeffs, levelled: e, iterations, extrema: ext, spread: s, folded });
        }
        refs = ext.into_iter().map(|(x, _)| x).collect();
    }
}

/// Exact value of a polynomial with exact coefficients at an exact point (Horner).
pub fn poly_exact(coeffs: &[(u32, Big)], x: &Big) -> Big {
    let deg = coeffs.iter().map(|(k, _)| *k).max().unwrap_or(0);
    let mut acc = Big::zero(false);
    for k in (0..=deg).rev() {
        acc = acc.mul(x);
        if let Some((_, c)) = coeffs.iter().find(|(j, _)| *j == k) {
            acc = acc.add(c);
        }
    }
    acc
}

/// Maximum error of `coeffs` (evaluated exactly) over points of `fmt` in `[a, b]`: a dense
/// grid of `n` points plus 33 points around each of `around` (and their mirror images).
/// Returns the maximum `|error|` and the number of points used.
pub fn measure(pb: &Problem, coeffs: &[(u32, Big)], fmt: Fmt, n: usize, around: &[Big]) -> (Big, usize) {
    let ctx = Ctx::new(pb, pb.a.clone(), pb.b.clone());
    let mut pts = ctx.grid(&pb.a, &pb.b, n.max(2));
    let h = pb.b.sub(&pb.a).div(&Big::from_u64(16 * n.max(2) as u64), 64, Round::Nearest);
    for x in around {
        for c in [x.clone(), x.neg()] {
            for k in -16i64..=16 {
                pts.push(c.add(&h.mul(&Big::from_i64(k))));
            }
        }
    }
    let mut worst = Big::zero(false);
    let mut count = 0;
    for x in pts {
        let x = Big::from_bits(fmt, x.round_to(fmt, Round::Nearest).0);
        if x.cmp(&pb.a) == Some(Ordering::Less) || x.cmp(&pb.b) == Some(Ordering::Greater) {
            continue;
        }
        let fx = (pb.f)(&x, ctx.p + 16);
        let px = poly_exact(coeffs, &x);
        let d = px.sub(&fx);
        let e = if pb.relative {
            if fx.is_zero() {
                if d.is_zero() {
                    continue; // 0/0 at a common zero: the error there is a limit, sampled nearby
                }
                return (Big::inf(false), count);
            }
            d.div(&fx, 64, Round::Nearest)
        } else {
            d.round(64, Round::Nearest)
        };
        count += 1;
        if e.cmp_abs(&worst) == Ordering::Greater {
            worst = e.abs();
        }
    }
    (worst, count)
}

/// `log2 |x|` as an `f64` (deterministic: computed in [`mp`]).
pub fn log2_abs(x: &Big) -> f64 {
    if x.is_zero() {
        return f64::NEG_INFINITY;
    }
    mp::log2(&x.abs(), 64).value.to_f64()
}

/// A parsed fit file.
#[derive(Clone, Debug)]
pub struct FitSpec {
    /// Constant name (as written; the generated constant is upper-cased).
    pub name: String,
    /// Target name (see [`TARGETS`]).
    pub target: String,
    /// Interval endpoints as written.
    pub interval_text: (String, String),
    /// Interval endpoints.
    pub interval: (Big, Big),
    /// The powers line as written.
    pub powers_text: String,
    /// All powers (fixed ones included), ascending.
    pub powers: Vec<u32>,
    /// Fixed coefficients, rounded to the format.
    pub fixed: Vec<(u32, Big)>,
    /// Relative error (else absolute).
    pub relative: bool,
    /// Storage format name and format.
    pub format: (String, Fmt),
    /// Output path relative to the workspace root.
    pub output: String,
    /// Working precision (default 256).
    pub prec: u32,
    /// Equioscillation tolerance (default 1e-6).
    pub tol: f64,
    /// Measurement grid size (default 8192).
    pub points: usize,
}

/// Parses an interval endpoint or fixed coefficient: a decimal or C hex float, or `pi`, `ln2`,
/// `ln10`, optionally signed and multiplied or divided by further such terms
/// (`-pi/4`, `ln2/2`, `0x1.8p-3`). Rounded to nearest at `prec` bits.
pub fn parse_expr(s: &str, prec: u32) -> Option<Big> {
    let s = s.trim();
    let (neg, s) = match s.strip_prefix('-') {
        Some(r) => (true, r),
        None => (false, s.strip_prefix('+').unwrap_or(s)),
    };
    let w = prec + 16;
    let term = |t: &str| -> Option<Big> {
        match t.trim() {
            "pi" => Some(mp::pi(w)),
            "ln2" => Some(mp::ln2(w)),
            "ln10" => Some(mp::ln10(w)),
            t => Big::parse_rounded(t, w),
        }
    };
    let mut acc: Option<Big> = None;
    let mut op = '*';
    let mut start = 0;
    let bytes = s.as_bytes();
    // Split on '*' and '/' that are not part of an exponent ("1e-3" has none of them).
    for i in 0..=bytes.len() {
        if i == bytes.len() || bytes[i] == b'*' || bytes[i] == b'/' {
            let t = term(&s[start..i])?;
            acc = Some(match acc {
                None => t,
                Some(a) if op == '*' => a.mul(&t).round(w, Round::Nearest),
                Some(a) => a.div(&t, w, Round::Nearest),
            });
            if i < bytes.len() {
                op = bytes[i] as char;
            }
            start = i + 1;
        }
    }
    let v = acc?.round(prec, Round::Nearest);
    Some(if neg { v.neg() } else { v })
}

/// Parses a powers specification: comma-separated items, each an integer, an inclusive range
/// `a..b` (or `a..=b`), optionally prefixed by `even` or `odd`: `0..7`, `1,3,5,7`,
/// `even 0..10`, `odd 1..13`.
pub fn parse_powers(s: &str) -> Option<Vec<u32>> {
    let mut out = Vec::new();
    for item in s.split(',') {
        let item = item.trim();
        let (filter, body) = if let Some(r) = item.strip_prefix("even") {
            (Some(0), r.trim())
        } else if let Some(r) = item.strip_prefix("odd") {
            (Some(1), r.trim())
        } else {
            (None, item)
        };
        let (lo, hi) = match body.split_once("..") {
            Some((a, b)) => (a.trim().parse::<u32>().ok()?, b.trim_start_matches('=').trim().parse::<u32>().ok()?),
            None => {
                let v = body.parse::<u32>().ok()?;
                (v, v)
            }
        };
        if hi < lo || hi > 200 {
            return None;
        }
        out.extend((lo..=hi).filter(|k| filter.is_none_or(|f| k % 2 == f)));
    }
    out.sort_unstable();
    out.dedup();
    if out.is_empty() { None } else { Some(out) }
}

/// Parses a fit file.
///
/// Keys: `name`, `target`, `interval` (two endpoints, optionally in brackets), `powers`,
/// `error` (`relative` | `absolute`), `format` (`f32` | `f64` | `f80` | `f128`), `fixed`
/// (optional, `k:value, ...`), `output` (path relative to the workspace root), and the optional
/// `precision` (working bits, default 256), `tolerance` (default 1e-6) and `points`
/// (measurement grid, default 8192).
pub fn parse_fit(text: &str) -> Result<FitSpec, String> {
    let mut kv: Vec<(String, String)> = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let (k, v) = line.split_once('=').ok_or(format!("line {}: expected key = value", n + 1))?;
        let k = k.trim().to_string();
        if kv.iter().any(|(j, _)| *j == k) {
            return Err(format!("line {}: duplicate key {k}", n + 1));
        }
        kv.push((k, v.trim().to_string()));
    }
    let get = |k: &str| kv.iter().find(|(j, _)| j == k).map(|(_, v)| v.clone());
    let need = |k: &str| get(k).ok_or(format!("missing key {k}"));
    for (k, _) in &kv {
        if !["name", "target", "interval", "powers", "error", "format", "fixed", "output", "precision", "tolerance", "points"]
            .contains(&k.as_str())
        {
            return Err(format!("unknown key {k}"));
        }
    }
    let prec: u32 = get("precision").map_or(Ok(256), |v| v.parse().map_err(|_| "bad precision".to_string()))?;
    let tol: f64 = get("tolerance").map_or(Ok(1e-6), |v| v.parse().map_err(|_| "bad tolerance".to_string()))?;
    let points: usize = get("points").map_or(Ok(8192), |v| v.parse().map_err(|_| "bad points".to_string()))?;
    let name = need("name")?;
    if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        return Err(format!("bad name {name}"));
    }
    let target_name = need("target")?;
    if target(&target_name).is_none() {
        let names: Vec<&str> = TARGETS.iter().map(|t| t.name).collect();
        return Err(format!("unknown target {target_name} (known: {})", names.join(", ")));
    }
    let iv = need("interval")?;
    let iv = iv.trim().trim_start_matches('[').trim_end_matches(']');
    let (ta, tb) = iv.split_once(',').ok_or("interval: expected two endpoints")?;
    let (ta, tb) = (ta.trim().to_string(), tb.trim().to_string());
    let a = parse_expr(&ta, prec).ok_or(format!("bad endpoint {ta}"))?;
    let b = parse_expr(&tb, prec).ok_or(format!("bad endpoint {tb}"))?;
    let powers_text = need("powers")?;
    let mut powers = parse_powers(&powers_text).ok_or(format!("bad powers {powers_text}"))?;
    let relative = match need("error")?.as_str() {
        "relative" => true,
        "absolute" => false,
        e => return Err(format!("error must be relative or absolute, not {e}")),
    };
    let fname = need("format")?;
    let fmt = match fname.as_str() {
        "f32" => Fmt::F32,
        "f64" => Fmt::F64,
        "f80" => Fmt::F80,
        "f128" => Fmt::F128,
        f => return Err(format!("unknown format {f}")),
    };
    let mut fixed = Vec::new();
    if let Some(fx) = get("fixed") {
        for item in fx.split(',').filter(|s| !s.trim().is_empty()) {
            let (k, v) = item.split_once(':').ok_or(format!("fixed: expected k:value in {item}"))?;
            let k: u32 = k.trim().parse().map_err(|_| format!("fixed: bad power {k}"))?;
            let v = parse_expr(v, prec).ok_or(format!("fixed: bad value {v}"))?;
            let v = Big::from_bits(fmt, v.round_to(fmt, Round::Nearest).0);
            fixed.push((k, v));
            powers.push(k);
        }
    }
    powers.sort_unstable();
    powers.dedup();
    Ok(FitSpec {
        name,
        target: target_name,
        interval_text: (ta, tb),
        interval: (a, b),
        powers_text,
        powers,
        fixed,
        relative,
        format: (fname, fmt),
        output: need("output")?,
        prec,
        tol,
        points,
    })
}

/// A generated coefficient file.
#[derive(Clone, Debug)]
pub struct Generated {
    /// Output path relative to the workspace root.
    pub output: String,
    /// The Rust source.
    pub source: String,
    /// A human-readable report.
    pub report: String,
}

/// Fits a spec and renders the Rust source. `fit_path` is how the header names the fit file.
pub fn generate(spec: &FitSpec, fit_path: &str) -> Result<Generated, String> {
    let t = target(&spec.target).ok_or("unknown target")?;
    let f = |x: &Big, p: u32| (t.eval)(x, p);
    let free: Vec<u32> = spec.powers.iter().copied().filter(|k| !spec.fixed.iter().any(|(j, _)| j == k)).collect();
    let mut pb = Problem::new(&f, t.parity, spec.interval.0.clone(), spec.interval.1.clone(), free, spec.relative);
    pb.fixed = spec.fixed.clone();
    pb.prec = spec.prec;
    pb.tol = spec.tol;
    let fit = remez(&pb)?;
    let fmt = spec.format.1;
    let rounded: Vec<(u32, Big)> =
        fit.coeffs.iter().map(|(k, c)| (*k, Big::from_bits(fmt, c.round_to(fmt, Round::Nearest).0))).collect();
    let around: Vec<Big> = fit.extrema.iter().map(|(x, _)| x.clone()).collect();
    let (maxerr, npts) = measure(&pb, &rounded, fmt, spec.points, &around);
    let lev = log2_abs(&fit.levelled);
    let meas = log2_abs(&maxerr);
    let units = maxerr.mul_pow2(i64::from(fmt.prec)).to_f64();
    let kind = if spec.relative { "relative" } else { "absolute" };
    let fname = &spec.format.0;
    let unit_text = if spec.relative {
        format!("{units:.2} ulp of {fname} (units of 2^-{} relative)", fmt.prec)
    } else {
        format!("{units:.2} units of 2^-{} (absolute)", fmt.prec)
    };
    let (ty, hexw) = match fmt.total_bits() {
        32 => ("u32", 8),
        64 => ("u64", 16),
        80 => ("u128", 20),
        _ => ("u128", 32),
    };
    let digits = (f64::from(fmt.prec) * std::f64::consts::LOG10_2).ceil() as usize + 1;
    let powers_list: Vec<String> = rounded.iter().map(|(k, _)| format!("x^{k}")).collect();
    let mut s = String::new();
    let _ = writeln!(s, "// @generated by `cargo run --release -p liella-mathtools --bin remez -- {fit_path}`. Do not edit.");
    let _ = writeln!(
        s,
        "//! {}: minimax {kind} approximation of {} on [{}, {}], powers {}, coefficients rounded to {fname}.",
        spec.name, spec.target, spec.interval_text.0, spec.interval_text.1, spec.powers_text
    );
    let _ = writeln!(
        s,
        "//! Derivation (D-10.3): Remez exchange, {} iterations at {}-bit precision; levelled error 2^{lev:.2};",
        fit.iterations, pb.prec
    );
    let _ = writeln!(s, "//! after rounding the coefficients to {fname}: max error 2^{meas:.2} (measured on {npts} points), i.e. {unit_text}.");
    if !spec.fixed.is_empty() {
        let fx: Vec<String> = spec.fixed.iter().map(|(k, c)| format!("x^{k} = {}", c.to_hex())).collect();
        let _ = writeln!(s, "//! Fixed in advance: {}.", fx.join(", "));
    }
    let _ = writeln!(s);
    let _ = writeln!(s, "/// Coefficients of {} ({fname} bit patterns), lowest power first.", powers_list.join(", "));
    let _ = writeln!(s, "pub(crate) const {}: [{ty}; {}] = [", spec.name.to_ascii_uppercase(), rounded.len());
    for (k, c) in &rounded {
        let bits = c.round_to(fmt, Round::Nearest).0;
        let _ = writeln!(s, "    0x{bits:0hexw$x}, // x^{k}: {}", c.to_decimal(digits));
    }
    let _ = writeln!(s, "];");
    let mut report = String::new();
    let _ = writeln!(report, "{} ({} on [{}, {}], {kind}, {fname}):", spec.name, spec.target, spec.interval_text.0, spec.interval_text.1);
    let _ = writeln!(
        report,
        "  {} iterations, spread {:.2e}{}; levelled error 2^{lev:.2}",
        fit.iterations,
        fit.spread,
        if fit.folded { ", folded onto the right half" } else { "" }
    );
    let _ = writeln!(report, "  rounded to {fname}: max error 2^{meas:.2} on {npts} points = {unit_text}");
    Ok(Generated { output: spec.output.clone(), source: s, report })
}
