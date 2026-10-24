use crate::float::{self, Float, Kind};
use mp::Mp;
use crate::real::Real;
use crate::status::{Rounding, Status};
use crate::{fused, pow as pw};

/// The real circular functions of any format (a part on an axis is the real function's result),
/// by [`Real`]: `f32`/`f64` by `crate::trig`, x87 and `_Float128` by `crate::wide`.
mod trig {
    use crate::real::Real;
    use crate::status::{Rounding, Status};

    pub(super) fn sin<F: Real>(x: F, m: Rounding) -> (F, Status) {
        x.sin(m)
    }
    pub(super) fn cos<F: Real>(x: F, m: Rounding) -> (F, Status) {
        x.cos(m)
    }
    pub(super) fn tan<F: Real>(x: F, m: Rounding) -> (F, Status) {
        x.tan(m)
    }
    pub(super) fn asin<F: Real>(x: F, m: Rounding) -> (F, Status) {
        x.asin(m)
    }
    pub(super) fn acos<F: Real>(x: F, m: Rounding) -> (F, Status) {
        x.acos(m)
    }
    pub(super) fn atan<F: Real>(x: F, m: Rounding) -> (F, Status) {
        x.atan(m)
    }
}

/// The real hyperbolic functions of any format, by [`Real`] (as [`trig`]).
mod hyper {
    use crate::real::Real;
    use crate::status::{Rounding, Status};

    pub(super) fn sinh<F: Real>(x: F, m: Rounding) -> (F, Status) {
        x.sinh(m)
    }
    pub(super) fn cosh<F: Real>(x: F, m: Rounding) -> (F, Status) {
        x.cosh(m)
    }
    pub(super) fn tanh<F: Real>(x: F, m: Rounding) -> (F, Status) {
        x.tanh(m)
    }
    pub(super) fn asinh<F: Real>(x: F, m: Rounding) -> (F, Status) {
        x.asinh(m)
    }
    pub(super) fn acosh<F: Real>(x: F, m: Rounding) -> (F, Status) {
        x.acosh(m)
    }
}

/// Both parts of a complex result and what they report together.
pub(super) type Res<F> = ((F, F), Status);

/// Beyond this magnitude the exponential of an argument is far outside every format (and
/// beyond the range of [`mp::exp`]).
const LIM: f64 = 549_755_813_888.0; // 2^39

// ------------------------------------------------------------------------------- the parts

/// A part before rounding.
#[derive(Clone, Copy, Debug)]
enum Part {
    /// An approximation `v` of a value, with the exponent `err` of a proven bound `2^err` on its
    /// absolute error: rounded by Ziv's test ([`crate::mp::decided`]), and counted as undecided
    /// ([`UNDECIDED`]) when the interval straddles a rounding boundary in the mode.
    Mp(Mp, i64),
    /// An exact value.
    Exact(Mp),
    /// A value just beside the `f64`, above it (toward +∞) if `true`, by far less than any last
    /// place.
    Near(f64, bool),
    /// A signed zero.
    Zero(bool),
    /// `lead · (1 + δ)` for an exact `lead` and a deviation `δ` far below any last place, of
    /// magnitude above `|lead|` if `true`: the leading-order expansions of the regimes where a
    /// part is a short dyadic number to within 2^-300 (tiny parts, `|z|` next to 1, exact roots).
    Dev(Mp, bool),
}

impl Part {
    fn neg(self) -> Part {
        match self {
            Part::Mp(v, err) => Part::Mp(v.neg(), err),
            Part::Exact(v) => Part::Exact(v.neg()),
            Part::Near(v, above) => Part::Near(-v, !above),
            Part::Zero(n) => Part::Zero(!n),
            Part::Dev(v, above) => Part::Dev(v.neg(), above),
        }
    }

    fn with_neg(self, neg: bool) -> Part {
        if neg { self.neg() } else { self }
    }

    fn round<F: Float>(self, mode: Rounding) -> (F, Status) {
        match self {
            Part::Mp(v, err) if !v.is_zero() => crate::mp::decided(crate::mp::Bounded { v, err: err + loose() }, mode).unwrap_or_else(|| undecided(v, mode)),
            Part::Exact(v) if !v.is_zero() => mp::round_exact(v, mode),
            // Only `±1` is ever near (`ctan`, `ctanh`): toward +∞ is above in magnitude for a
            // positive value, below for a negative one.
            Part::Near(v, above) if v != 0.0 => Part::Dev(Mp::from_f64(v), above != (v < 0.0)).round(mode),
            Part::Zero(n) => (float::zero(n), Status::OK),
            // An exact short value with the sticky bit on the side of the deviation.
            Part::Dev(v, true) if !v.is_zero() => mp::round(v, mode),
            Part::Dev(v, false) if !v.is_zero() => mp::round(v.sub(v.ldexp(-300)), mode),
            _ => (float::zero(false), Status::OK),
        }
    }
}

/// The significand width of this evaluation: 384 bits in `n`, 768 in `w`.
const B: i64 = (mp::LIMBS * 64) as i64;

/// Parts left undecided by Ziv's test at this width. In `n` (384 bits) a change makes the caller
/// evaluate again in `w`; in `w` (768 bits) it counts the parts that take the `TODO(ZIV-C)` path.
pub(super) static UNDECIDED: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// An undecided part: counted, then rounded once. In `n` the caller discards the result and
/// evaluates again in 768 bits. TODO(ZIV-C): in `w` this is the end of the loop for now; ZIV-C
/// (#1758) continues with a precision that grows until the part is decided.
#[cold]
#[inline(never)]
fn undecided<F: Float>(v: Mp, mode: Rounding) -> (F, Status) {
    UNDECIDED.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    #[cfg(test)]
    MISSES.with(|c| c.set(c.get() + 1));
    mp::round(v, mode)
}

/// `v` with a relative error below `2^(c − B)` (so an absolute one below `2^(v.exp + c + 1 − B)`,
/// the 1 for `|exact| < 2^v.exp·(1 + 2^(c − B))`).
fn rel(v: Mp, c: i64) -> Part {
    Part::Mp(v, v.exp + c + 1 - B)
}

// The run-time bounds of the regimes whose error grows with an operand (`cexp`, `csinh`,
// `ccosh`, `csin`, `ccos`, `ctanh`, `ctan`, `cpow`): every intermediate is an [`Approx`], a value
// with the exponent of a proven bound on its absolute error, carried through the operation
// bounds of the `mpw` header. Exponents only: `|v| < 2^ex(v)`, and each sum of `n` terms below
// `2^t` is bounded by `2^(t + ⌈log2 n⌉)` of the largest. A bound that cannot be given (an
// argument error too large for the exponential's linearisation, a divisor whose sign is not
// certain) is [`UNB`]: the part is then undecided at this width, never wrong.

/// No bound (an always-undecided part).
const UNB: i64 = 1 << 42;
/// An exact value's "bound" (far below every other exponent; sums of a few stay in `i64`).
const EXACT: i64 = -(1 << 42);

/// An approximation `v` of a real value and the exponent `a` of a bound `2^a` on its absolute
/// error ([`EXACT`] for none, [`UNB`] for no bound).
#[derive(Clone, Copy, Debug)]
struct Approx {
    v: Mp,
    a: i64,
}

impl Approx {
    /// An exact value.
    fn exact(v: Mp) -> Approx {
        Approx { v, a: EXACT }
    }

    /// `v` with a relative error below `2^r`: absolute `< 2^(ex(v) + r + 1)` for `r <= −1`
    /// (`|V| <= |v|/(1 − 2^r) < 2^(ex(v) + 1)`), no bound above.
    fn rel(v: Mp, r: i64) -> Approx {
        Approx { v, a: if r <= -1 { (crate::mp::ex(v) + r + 1).max(EXACT) } else { UNB } }
    }

    fn ex(self) -> i64 {
        crate::mp::ex(self.v)
    }

    fn fix(v: Mp, a: i64) -> Approx {
        Approx { v, a: a.clamp(EXACT, UNB) }
    }

    fn neg(self) -> Approx {
        Approx { v: self.v.neg(), a: self.a }
    }

    /// Exact scaling by a power of two.
    fn ldexp(self, k: i64) -> Approx {
        Approx { v: self.v.ldexp(k), a: if self.a <= EXACT { EXACT } else { self.a + k } }
    }

    /// `|XY − xy| <= |X||Y − y| + |y||X − x|` with `|X| < 2^(max(ex x, a_x) + 1)`, plus `mul`'s
    /// truncation `< 2^(ex(p) − B)`: three terms.
    fn mul(self, o: Approx) -> Approx {
        let p = self.v.mul(o.v);
        Approx::fix(p, (self.ex().max(self.a) + 1 + o.a).max(o.ex() + self.a).max(crate::mp::ex(p) - B) + 2)
    }

    /// Both errors and `add`'s `< 2^(e_big + 2 − B)`: three terms.
    fn add(self, o: Approx) -> Approx {
        let s = self.v.add(o.v);
        Approx::fix(s, self.a.max(o.a).max(self.ex().max(o.ex()) + 2 - B) + 2)
    }

    fn sub(self, o: Approx) -> Approx {
        self.add(o.neg())
    }

    /// `N/D − n/d = ((N − n)d − n(D − d))/(Dd)`: with `|D| >= 2^(ex d − 2)` (so `a_d <= ex d − 2`),
    /// `< 2^(a_n + 2 − ex d) + 2^(ex n + a_d + 3 − 2 ex d)`, plus `div`'s relative `2^(4−B)` of a
    /// quotient below `2^(ex q + 1)`: three terms. No bound when the divisor's sign is not certain.
    fn div(self, d: Approx) -> Approx {
        let q = self.v.div(d.v);
        if d.v.is_zero() || d.a > d.ex() - 2 {
            return Approx { v: q, a: UNB };
        }
        let a = (self.a + 2 - d.ex()).max(self.ex() + d.a + 3 - 2 * d.ex()).max(crate::mp::ex(q) + 5 - B) + 2;
        Approx::fix(q, a)
    }

    /// The part to round.
    fn part(self) -> Part {
        if self.a <= EXACT { Part::Exact(self.v) } else { Part::Mp(self.v, self.a) }
    }
}

/// The exponent of a bound on the relative error of [`exp_mp`]`(x)`: the header's
/// `2^(14−B)·(|k| + 64)`, `|k| <= |x|/ln 2 < 2^(ex x + 1)`, so `|k| + 64 < 2^(max(ex x, 5) + 2)`.
/// From `|x| >= 2^39` ([`LIM`]) `exp_mp` returns a magnitude beyond every format: the clamp at 40
/// gives it a small relative bound, sound because the exact value and every value within the
/// bound of the product it enters (whose other factor is then bounded away from 0, or the part is
/// undecided) lie beyond the format on the same side, so they round alike.
fn exp_rel(x: Mp) -> i64 {
    16 + crate::mp::ex(x).clamp(5, 40) - B
}

/// `e^x` of an approximate `x`: `e^X = e^x·e^(X − x)`, `|e^δ − 1| < 1.07|δ|` for `|δ| <= 1/8`, so a
/// relative error below `2^(max(a_x + 1, exp_rel) + 1)` (against the exact `e^X`). No bound for
/// `a_x > −3`.
fn exp_approx(x: Approx) -> Approx {
    let v = exp_mp(x.v);
    if x.a > -3 {
        return Approx { v, a: UNB };
    }
    Approx::rel(v, (x.a + 1).max(exp_rel(x.v)) + 1)
}

/// The exponent of a bound on the relative error of `mp::sinh_cosh(x)` (header: `2^(7−B)` below
/// ½, else `2^(17−B)·(|k| + 64)`), clamped at 40 as [`exp_rel`] (`hyp` of `|x| >= 2^39`).
fn hyp_rel(x: Mp) -> i64 {
    (if x.exp <= -1 { 7 } else { 19 + x.exp.clamp(5, 40) }) - B
}

/// The exponent of a bound on the absolute error of each of `(sin x, cos x)` from
/// `mp::sin_cos_wide` of an exact operand of exponent `xe`: below 2^29 `mp::sin_cos`, `(|x| + 2)
/// ·2^(4−B) + 2^(9−B) < 2^(max(xe, 4) + 6 − B)`; above, `reduce_wide`'s `2^(11−B)`.
fn trig_abs(xe: i64) -> i64 {
    (if xe <= 29 { xe.max(4) + 6 } else { 11 }) - B
}

// The bounds of the general regimes of `csqrt`, `casin`, `cacos`, `casinh`, `cacosh`, `catanh`
// and `catan` count relative errors in units of `u = 2^-B`, through the operation bounds of the
// `mpw` header, to first order (a product of two relative errors stays below 2^(2·14 − 2B), inside
// the bit of margin every class below keeps). The operands are exact (`f128`). `add` of two
// positive terms keeps the larger relative error of its operands and adds `< 6u` (`3·2^(e_big − B)`
// against a sum of at least `2^(e_big − 1)`); `mul` adds its operands' errors and `2u`; `sqr`
// doubles and adds `2u`; `div` adds both and `16u`; `sqrt` halves and adds `32u`; `ldexp` is
// exact. `1 − a` (or `a − 1`) of an exact `f128` `a > 0` is within `4u`: exact for
// `2^-270 <= a < 2^270` (`shr` keeps every bit of the smaller operand and the difference fits),
// otherwise at least half the larger operand, against `shr`'s `2^(e_big − B)`.

/// The class `c` (relative error below `2^(c − B)`) of `ln1p(t)` for `t > 0` given to within a
/// relative `2^(8 − B)`. The relative condition `t/((1 + t) ln(1 + t))` of `ln1p` is at most 1, so
/// the argument's error passes through. Below 2^-6 (`t.exp <= −6`), the series (`< 2^(9−B)`):
/// `2^9 + 2^8 < 2^10`. Above, `ln_atanh(1 + t)`: the sum adds `< 6u` relative, so `< 6u` absolute
/// to the logarithm; `ln_atanh` adds `< 2^(max(e, 0) + 12 − B)` absolute for a result in
/// `[2^(e−1), 2^e)`, and `ln(1 + t) > 2^-7`: relative `< 2^(13−B)` for `e >= 0`, `< 2^(19−B)`
/// below; with the `6u·2^7` and the argument's `2^8 u`, `< 2^(20−B)`. One bit of margin each.
fn ln1p_class(t: Mp) -> i64 {
    if t.exp <= -6 { 11 } else { 21 }
}

#[cfg(test)]
std::thread_local! {
    /// A test-only offset on every bound of this width (the forced-retry tests).
    static LOOSE: core::cell::Cell<i64> = const { core::cell::Cell::new(0) };
    /// Undecided parts at this width on this thread (tests only: the global counter is shared by
    /// tests running in parallel).
    static MISSES: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
}

/// Undecided parts at this width on this thread (tests only).
#[cfg(test)]
pub(super) fn misses() -> u64 {
    MISSES.with(core::cell::Cell::get)
}

/// The test-only offset added to every bound of this width (0 outside tests).
#[cfg(test)]
fn loose() -> i64 {
    LOOSE.with(core::cell::Cell::get)
}

#[cfg(not(test))]
#[inline(always)]
const fn loose() -> i64 {
    0
}

/// Loosen every bound of this width by `2^k` on this thread (tests only).
#[cfg(test)]
pub(super) fn force_loose(k: i64) {
    LOOSE.with(|c| c.set(k));
}

/// Round both parts.
fn pair<F: Float>(re: Part, im: Part, mode: Rounding) -> Res<F> {
    two(re.round(mode), im.round(mode))
}

/// Two rounded parts as one result.
fn two<F: Float>(re: (F, Status), im: (F, Status)) -> Res<F> {
    ((re.0, im.0), re.1.and(im.1))
}

/// A part to round, then a value.
fn with_im<F: Float>(re: Part, im: F, st: Status, mode: Rounding) -> Res<F> {
    two(re.round(mode), (im, st))
}

/// A value, then a part to round.
fn with_re<F: Float>(re: F, st: Status, im: Part, mode: Rounding) -> Res<F> {
    two((re, st), im.round(mode))
}

/// `k·π/4`, negated if `neg`.
fn pi4(k: u64, neg: bool) -> Part {
    rel(mp::pi().ldexp(-2).mul_u64(k), 3).with_neg(neg)
}

/// The mirrored rounding direction (for a negated result).
fn flip(m: Rounding) -> Rounding {
    match m {
        Rounding::Up => Rounding::Down,
        Rounding::Down => Rounding::Up,
        o => o,
    }
}

/// `f`'s result, negated if `neg` (computed in the mirrored direction, so `−f` is rounded right).
fn signed<F: Float>(neg: bool, mode: Rounding, f: impl FnOnce(Rounding) -> (F, Status)) -> (F, Status) {
    if neg {
        let (v, st) = f(flip(mode));
        (negate(v), st)
    } else {
        f(mode)
    }
}

/// Below this magnitude (2^-150) a part's square lies beyond the 384 bits next to 1: where both
/// parts are that small, each part of the result is given by its leading-order expansion.
///
/// The cut-off holds for any precision up to 73 bits (`f32`, `f64`, x87): the deviations of the
/// expansions are then below 2^-149 relative, while their leading terms (1, a part, or a square
/// of a part: at most 2·PREC bits) lie 0 or at least 2^-(2·PREC + 1) from every rounding boundary;
/// and a part at the cut-off still has its square 2^-300 within the 384 bits. `_Float128`
/// (113 bits, squares of 226) needs the cut-offs derived again (M5-4).
///
/// The cut-off is a bound, not a tuning knob: a part much above 2^-(PREC + 2) is no longer
/// negligible beside 1 (`cexp(2^-61 (1 + i))` has real part 1 + 2^-61 in x87, not 1; unit test
/// `x87_parts_above_the_tiny_cutoff_are_not_expanded`).
///
/// As the exponent of an [`Mp`] (`|v| < 2^exp`): `|v| < 2^-150` exactly when `exp <= -150`.
const TINY_EXP: i64 = -150;

/// The widest precision the cut-offs of this module are derived for: `_Float128` (M5-4).
///
/// [`TINY_EXP`] holds at 113 bits branch by branch: every leading term is 1 or a part (exact, a
/// representable value, so only its neighbours 2^-114 away matter) with a deviation below 2^-149,
/// or a product `xy` (226 bits, 2^-227 from any other boundary) whose deviation is quadratic
/// (below 2^-299). Where a leading term has 2·PREC bits and a first-order deviation (`clog` next
/// to `|z| = 1`), the cut-off is [`near_unit_exp`].
const MAX_PREC: u32 = 113;

/// The exponent below which `t = |z|² − 1` (exact, at most 2·PREC bits) gives `ln|z| = t/2` with
/// its side: the deviation `t/2` (relative) must lie below the 2^-(2·PREC + 1) that separates an
/// inexact `t/2` from every rounding boundary. 2^-150 for up to 73 bits, 2^-230 for `_Float128`;
/// above it the 384-bit `ln1p` resolves the deviation (at least 2^-232 relative).
fn near_unit_exp<F: Float>() -> i64 {
    -(i64::from(F::PREC) * 2 + 4).max(150)
}

/// A deviation's direction (above in magnitude) from the sign of `d`; `tie` when `d` is zero (then
/// the next order decides, as the caller knows).
fn dir(d: Mp, tie: bool) -> bool {
    if d.is_zero() { tie } else { !d.neg }
}

/// `j·u² − k·v²` (exact enough for its sign).
fn sq_diff(u: Mp, j: u64, v: Mp, k: u64) -> Mp {
    u.sqr().mul_u64(j).sub(v.sqr().mul_u64(k))
}

/// The sign of `u + v` for two zeros of signs `n1`, `n2`: theirs if they agree, otherwise −0 only
/// when rounding down (IEEE 754).
fn zero_sum(n1: bool, n2: bool, mode: Rounding) -> bool {
    if n1 == n2 { n1 } else { mode == Rounding::Down }
}

// ---------------------------------------------------------------------------- the operands

/// The class of an operand.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum K {
    Zero,
    Fin,
    Inf,
    Nan,
}

#[derive(Clone, Copy, Debug)]
struct Op<F> {
    v: F,
    k: K,
    neg: bool,
    snan: bool,
}

impl<F: Float> Op<F> {
    fn new(v: F) -> Op<F> {
        let n = float::decode(v);
        let k = match n.kind {
            Kind::Zero => K::Zero,
            Kind::Finite => K::Fin,
            Kind::Inf => K::Inf,
            _ => K::Nan,
        };
        Op { v, k, neg: n.neg, snan: matches!(n.kind, Kind::SNan | Kind::Invalid) }
    }

    fn finite(&self) -> bool {
        matches!(self.k, K::Zero | K::Fin)
    }

    /// The value of a finite operand (exact).
    fn mp(&self) -> Mp {
        let n = float::decode(self.v);
        if n.kind == Kind::Zero { Mp::ZERO } else { Mp::from_u128(n.neg, n.sig, i64::from(n.exp)) }
    }

    fn abs_mp(&self) -> Mp {
        self.mp().abs()
    }

    fn abs(&self) -> F {
        float::with_sign(self.v, false)
    }

    /// A nonzero finite operand below [`TINY_EXP`] (2^-150) in magnitude.
    fn tiny(&self) -> bool {
        self.k == K::Fin && self.mp().exp <= TINY_EXP
    }

    /// `|x| <= 1` of a finite operand.
    fn at_most_one(&self) -> bool {
        self.mp().cmp_abs(Mp::from_i64(1)).is_le()
    }

    /// `|x| == 1` of a finite operand.
    fn is_one(&self) -> bool {
        self.abs_mp() == Mp::from_i64(1)
    }

    /// `(sin x, cos x)` of a finite nonzero operand of any format (Payne–Hanek beyond 2^29).
    fn sin_cos(&self) -> (Mp, Mp) {
        let n = float::decode(self.v);
        mp::sin_cos_wide(n.neg, n.sig, i64::from(n.exp))
    }
}

impl<F: Float> Op<F> {

    /// The operand, quieted if it is a signalling NaN.
    fn quiet(&self) -> F {
        if self.snan { quiet(self.v) } else { self.v }
    }
}

/// The positive quiet NaN without payload (glibc's NaN for most complex special cases).
fn qn<F: Float>() -> F {
    float::nan(false, true, 0)
}

/// The default NaN of an invalid operation (sign set).
fn dn<F: Float>() -> F {
    float::default_nan()
}

fn quiet<F: Float>(v: F) -> F {
    float::quiet(&float::decode(v))
}

fn negate<F: Float>(v: F) -> F {
    float::with_sign(v, !float::decode(v).neg)
}

fn inf<F: Float>(neg: bool) -> F {
    float::inf(neg)
}

fn zero<F: Float>(neg: bool) -> F {
    float::zero(neg)
}

/// ±1.
fn one<F: Float>(neg: bool) -> F {
    mp::round_exact(Mp::from_i64(if neg { -1 } else { 1 }), Rounding::Nearest).0
}

/// Refuse (at compile time) a format wider than the cut-offs of this module are derived for.
fn derived_for<F: Float>() {
    const { assert!(F::PREC <= MAX_PREC, "math::complex: cut-offs not derived for this precision") }
}

/// Invalid if a signalling NaN was among the operands.
fn sn(s: bool) -> Status {
    if s { Status::INVALID } else { Status::OK }
}

/// The signs of `sin t` and `cos t` of a finite nonzero operand of any format.
fn signs_op<F: Float>(t: &Op<F>) -> (bool, bool) {
    let (s, c) = t.sin_cos();
    (s.neg, c.neg)
}

/// A magnitude far beyond every format, with a sign.
fn huge(neg: bool) -> Mp {
    let v = Mp::from_i64(1).ldexp(1 << 40);
    if neg { v.neg() } else { v }
}

/// A nonzero magnitude far below every format, with a sign.
fn tiny(neg: bool) -> Mp {
    let v = Mp::from_i64(1).ldexp(-(1 << 40));
    if neg { v.neg() } else { v }
}

/// `e^x`, or a value beyond every format where that is all that matters.
fn exp_mp(x: Mp) -> Mp {
    let xf = x.to_f64();
    if xf >= LIM {
        huge(false)
    } else if xf <= -LIM {
        tiny(false)
    } else {
        mp::exp(x)
    }
}

// --------------------------------------------------------------------------- the trivial ones

/// `creal`.
pub(super) fn creal<F: Float>(x: F, _y: F) -> F {
    x
}

/// `cimag`.
pub(super) fn cimag<F: Float>(_x: F, y: F) -> F {
    y
}

/// `conj`: the imaginary part's sign flipped (a NaN's too), nothing raised.
pub(super) fn conj<F: Float>(x: F, y: F, _mode: Rounding) -> Res<F> {
    ((x, negate(y)), Status::OK)
}

/// `cproj`: `+∞ + i·(±0)` with the sign of `y` if either part is infinite, otherwise `z` itself
/// (glibc raises invalid for a signalling NaN part, except an imaginary one beside an infinite
/// real part, and returns it as it is).
pub(super) fn cproj<F: Float>(x: F, y: F, _mode: Rounding) -> Res<F> {
    let (a, b) = (Op::new(x), Op::new(y));
    let s = sn(a.snan || (b.snan && a.k != K::Inf));
    if a.k == K::Inf || b.k == K::Inf {
        return ((inf(false), zero(b.neg)), s);
    }
    ((x, y), s)
}

/// `cabs`: `hypot(x, y)`, in every format (D-10.21).
pub(super) fn cabs<F: Float>(x: F, y: F, mode: Rounding) -> (F, Status) {
    pw::hypot(x, y, mode)
}

/// `carg`: `atan2(y, x)`, in every format with an `atan2` (D-10.21).
///
/// `cargf128` of two quiet NaNs that differ only in sign returns the real part's, as glibc's
/// does (complex_f128_products.rs); `atan2`'s choice would give the imaginary part's.
pub(super) fn carg<F: Real>(x: F, y: F, mode: Rounding) -> (F, Status) {
    if F::PREC == 113 {
        let (a, b) = (Op::new(x), Op::new(y));
        if a.k == K::Nan && b.k == K::Nan && !a.snan && !b.snan && a.neg != b.neg && (x.to_bits() ^ y.to_bits()).count_ones() == 1 {
            return (x, sn(false));
        }
    }
    y.atan2(x, mode)
}

// --------------------------------------------------------------------- exponential, logarithm

/// `cexp(x + iy) = e^x (cos y + i sin y)`.
pub(super) fn cexp<F: Real>(x: F, y: F, mode: Rounding) -> Res<F> {
    derived_for::<F>();
    let (a, b) = (Op::new(x), Op::new(y));
    let s = sn(a.snan || b.snan);
    if a.finite() && b.finite() {
        if b.k == K::Zero {
            let (r, st) = x.exp(mode);
            return ((r, y), st);
        }
        if a.k == K::Zero {
            return two(y.cos(mode), y.sin(mode));
        }
        if a.tiny() && b.tiny() {
            // e^x cos y = 1 + (x − y²/2) + ..., e^x sin y = y (1 + x − y²/6 + ...).
            let (xm, ym) = (a.mp(), b.mp());
            let y2 = ym.sqr();
            return pair(Part::Dev(Mp::from_i64(1), dir(xm.ldexp(1).sub(y2), false)), Part::Dev(ym, dir(xm.mul_u64(6).sub(y2), false)), mode);
        }
        let (sy, cy) = b.sin_cos();
        let e = Approx::rel(exp_mp(a.mp()), exp_rel(a.mp()));
        let t = trig_abs(b.mp().exp);
        return pair(e.mul(Approx::fix(cy, t)).part(), e.mul(Approx::fix(sy, t)).part(), mode);
    }
    match (a.k, b.k) {
        (K::Nan, K::Zero) => ((qn(), y), s),
        (K::Nan, K::Nan) => ((qn(), qn()), s),
        (K::Nan, _) => ((qn(), qn()), Status::INVALID),
        (K::Inf, _) if !a.neg => match b.k {
            K::Zero => ((x, y), Status::OK),
            K::Fin => {
                let (sy, cy) = signs_op(&b);
                ((inf(cy), inf(sy)), Status::OK)
            }
            K::Inf => ((x, dn()), Status::DOMAIN),
            // An invalid x87 encoding gives the default NaN (X87-NAN-RULE-AUDIT-2).
            K::Nan if float::decode(y).kind == Kind::Invalid => ((x, dn()), s),
            K::Nan => ((x, b.quiet()), s),
        },
        (K::Inf, _) => match b.k {
            K::Zero => ((zero(false), y), Status::OK),
            K::Fin => {
                let (sy, cy) = signs_op(&b);
                ((zero(cy), zero(sy)), Status::OK)
            }
            _ => ((zero(false), zero(b.neg)), s),
        },
        (_, K::Inf) => ((qn(), qn()), Status::DOMAIN),
        _ => ((qn(), qn()), Status::INVALID),
    }
}

/// `clog(x + iy) = ln|z| + i atan2(y, x)`.
pub(super) fn clog<F: Real>(x: F, y: F, mode: Rounding) -> Res<F> {
    log_c(x, y, false, mode)
}

/// `clog10(x + iy) = log10|z| + i atan2(y, x)/ln 10` (also `__clog10`).
pub(super) fn clog10<F: Real>(x: F, y: F, mode: Rounding) -> Res<F> {
    log_c(x, y, true, mode)
}

fn log_c<F: Real>(x: F, y: F, ten: bool, mode: Rounding) -> Res<F> {
    derived_for::<F>();
    let (a, b) = (Op::new(x), Op::new(y));
    let s = sn(a.snan || b.snan);
    if a.k == K::Inf || b.k == K::Inf {
        if a.k == K::Nan || b.k == K::Nan {
            return ((inf(false), qn()), s);
        }
        let (im, st) = arg(&a, &b, ten, mode);
        return ((inf(false), im), st);
    }
    if a.k == K::Nan || b.k == K::Nan {
        return ((qn(), qn()), s);
    }
    let (im, st) = arg(&a, &b, ten, mode);
    if a.k == K::Zero && b.k == K::Zero {
        return ((inf(true), im), Status::POLE.and(st));
    }
    let (re, rst) = log_abs(&a, &b, ten, mode);
    ((re, im), rst.and(st))
}

/// `arg z = atan2(y, x)` of non-NaN parts, divided by `ln 10` for `clog10`.
fn arg<F: Real>(a: &Op<F>, b: &Op<F>, ten: bool, mode: Rounding) -> (F, Status) {
    if !ten {
        return b.v.atan2(a.v, mode);
    }
    if !a.neg && (b.k == K::Zero || (a.k == K::Inf && b.k != K::Inf)) {
        return (zero(b.neg), Status::OK);
    }
    // atan2 `2^(13−B)` relative (the constant angles less), `ln10` `2^(13−B)` (`ln_atanh`'s
    // `2^(14−B)` absolute over `ln 10 > 2`), the division `2^(4−B)`: below `2^(15−B)`.
    rel(angle(a, b).div(mp::ln10()), 15).round(mode)
}

/// `atan2(y, x)` of non-NaN parts whose result is not zero.
fn angle<F: Float>(a: &Op<F>, b: &Op<F>) -> Mp {
    let v = match (a.k, b.k) {
        (K::Inf, K::Inf) => mp::pi().ldexp(-2).mul_u64(if a.neg { 3 } else { 1 }),
        (_, K::Inf) => mp::pi().ldexp(-1),
        (K::Inf, _) | (_, K::Zero) => mp::pi(),
        _ => mp::atan2(b.abs_mp(), a.mp()),
    };
    if b.neg { v.neg() } else { v }
}

/// `ln|z|²/2` of a finite nonzero `z` (the 1 of `|z|² − 1` cancelled exactly), the class `c` of
/// its proven relative error `< 2^(c − B)` (for [`rel`]), and whether `|z|` is exactly 1.
///
/// Bounds (D-10.17; parts of at most 113 bits, `B = 384` or `768`): the squares are exact.
/// Near 1 (larger part in `[½, 2)`): `x² − 1` is exact (bits from `2^2` to `2^-226`); the sum
/// `t = (x² − 1) + y²` is exact unless it spans more than `B` bits, which needs
/// `e(y²) < e(x² − 1) − 157`, and then `|t| > 2^(e(x²−1) − 2)` with the addition's
/// `< 2^(e_big + 2 − B)`: `t` within `2^(4−B)` relative. `ln1p` maps a relative `δ` of `t` to
/// `< 4δ` of its result (`t/((1 + t)·ln(1 + t)) ≤ 2.2` for `1 + t = |z|² ≥ ¼`, decreasing) and
/// adds `< 2^(25−B)` (`2^(22 + max(e_t, 0) − B)`, `e_t ≤ 3`): `c = 26`. Elsewhere: the sum of
/// squares within `2^(3−B)` relative (like signs), so `ln` moves by `< 2^(3−B)`, and `ln_atanh`
/// adds `< 2^(e_ln + 12 − B)` with `|ln|z|²| ≥ 2^(e_ln − 1)`, `e_ln ≥ 0` (`|z|² ≥ 4` or `≤ ½`):
/// `c = 14`.
fn ln_abs(p: Mp, q: Mp) -> (Mp, i64, bool) {
    let (big, small) = if p.cmp_abs(q).is_ge() { (p, q) } else { (q, p) };
    if big.exp == 0 || big.exp == 1 {
        // The larger part in [½, 2): |z|² − 1 = (x² − 1) + y², the first difference exact.
        let t = big.sqr().sub(Mp::from_i64(1)).add(small.sqr());
        if t.is_zero() {
            return (Mp::ZERO, 0, true);
        }
        return (mp::ln1p(t).ldexp(-1), 26, false);
    }
    (mp::ln_atanh(big.sqr().add(small.sqr())).ldexp(-1), 14, false)
}

/// `ln|z|` (`log10|z|`) of a finite nonzero `z`.
fn log_abs<F: Real>(a: &Op<F>, b: &Op<F>, ten: bool, mode: Rounding) -> (F, Status) {
    if a.k == K::Zero || b.k == K::Zero {
        let m = if a.k == K::Zero { b.abs() } else { a.abs() };
        return if ten { m.log10(mode) } else { m.log(mode) };
    }
    let (p, q) = (a.abs_mp(), b.abs_mp());
    let (ln, c, unit) = ln_abs(p, q);
    if unit {
        return (zero(false), Status::OK);
    }
    let (big, small) = if p.cmp_abs(q).is_ge() { (p, q) } else { (q, p) };
    if !ten {
        // |z|² = 1 + t with t tiny and exact: ln|z| = t/2 (1 − t/2 + ...). The deviation is
        // below 2^-150 relative; t has at most 2·PREC bits (x² − 1 is 0 or at least 2^-2·PREC
        // in magnitude, so the larger part is 1), so t/2 lies 0 or farther from every boundary.
        let (t, exact) = mp::add_x(big.sqr().sub(Mp::from_i64(1)), small.sqr());
        if exact && !t.is_zero() && t.exp < near_unit_exp::<F>() {
            return Part::Dev(t.ldexp(-1), t.neg).round(mode);
        }
    } else if small.exp < big.exp - 150 {
        // |x| = 10^k beside a negligible y: log10|z| = k + (y/x)²/(2 ln 10) + ...
        if let Some(m) = power_of_ten(big, Mp::ZERO) {
            return Part::Dev(Mp::from_i64(m).ldexp(-1), true).round(mode);
        }
    }
    if ten {
        if let Some(m) = power_of_ten(p, q) {
            return Part::Exact(Mp::from_i64(m).ldexp(-1)).round(mode);
        }
        // `ln10` within `2^(14−B)` absolute (`ln_atanh`, `e_ln = 2`), so `2^(13−B)` relative, and
        // the division `2^(4−B)`: `2^c + 2^13 + 2^4 < 2^(c+1)` for `c ≥ 14`.
        return rel(ln.div(mp::ln10()), c + 1).round(mode);
    }
    rel(ln, c).round(mode)
}

/// `m` if `|z|² = 10^m` exactly (`clog10(6 + 8i) = 1`): only possible when the squares are close
/// enough for their sum to be exact here.
///
/// The sum must be exact (a 113-bit part squares to 226 bits, so a sum of parts 100 binary orders
/// apart no longer fits the 384 bits, and a rounded sum could equal `10^m` by accident). `10^m`
/// stays exact here up to `m = 165` (5^165 < 2^384); a larger power would need a sum beyond the
/// 384 bits, which the exactness test refuses (such a `z` takes the general path: a known limit).
fn power_of_ten(p: Mp, q: Mp) -> Option<i64> {
    if (p.exp - q.exp).abs() > 192 {
        return None;
    }
    let (n, exact) = mp::add_x(p.sqr(), q.sqr());
    if !exact {
        return None;
    }
    let mut t = Mp::from_i64(10);
    for m in 1..=165 {
        match t.cmp_abs(n) {
            core::cmp::Ordering::Equal => return Some(m),
            core::cmp::Ordering::Greater => return None,
            core::cmp::Ordering::Less => t = t.mul_u64(10),
        }
    }
    None
}

// ------------------------------------------------------------------------------ square root

/// `csqrt(x + iy)`: the principal square root (real part ≥ +0).
pub(super) fn csqrt<F: Real>(x: F, y: F, mode: Rounding) -> Res<F> {
    derived_for::<F>();
    let (a, b) = (Op::new(x), Op::new(y));
    let s = sn(a.snan || b.snan);
    if b.k == K::Inf {
        return ((inf(false), y), s);
    }
    if a.k == K::Inf {
        return match (a.neg, b.k == K::Nan) {
            (true, true) => ((qn(), inf(b.neg)), s),
            (true, false) => ((zero(false), inf(b.neg)), Status::OK),
            (false, true) => ((x, qn()), s),
            (false, false) => ((x, zero(b.neg)), Status::OK),
        };
    }
    if a.k == K::Nan || b.k == K::Nan {
        return ((qn(), qn()), s);
    }
    if b.k == K::Zero {
        if a.k == K::Zero {
            return ((zero(false), y), Status::OK);
        }
        if !a.neg {
            let (r, st) = fused::sqrt::<F, F>(x, mode);
            return ((r, y), st);
        }
        let (r, st) = signed(b.neg, mode, |m| fused::sqrt::<F, F>(a.abs(), m));
        return ((zero(false), r), st);
    }
    let (p, q) = (a.abs_mp(), b.abs_mp());
    if let Some((u, v)) = sqrt_exact(p, q, F::PREC) {
        let (re, im) = if a.neg { (v, u) } else { (u, v) };
        return pair(Part::Exact(re), Part::Exact(im).with_neg(b.neg), mode);
    }
    if let Some((re, im)) = sqrt_ratio(&a, p, q) {
        return pair(re, im.with_neg(b.neg), mode);
    }
    let (t, o) = sqrt_general(p, q);
    let (re, im) = if a.neg { (o, t) } else { (t, o) };
    pair(rel(re, SQRT_CLASS), rel(im, SQRT_CLASS).with_neg(b.neg), mode)
}

/// The class of both values of [`sqrt_general`].
const SQRT_CLASS: i64 = 8;

/// `(t, o)` with `t = √((|z| + p)/2)` and `o = q/(2t)` for `p, q >= 0` exact, not both zero: the
/// parts of `√z` (`t` real for `x >= 0`), nothing cancels. Relative errors (lemma above, units
/// of `u`): `p², q²` 2; their sum 8; `r = |z|` 4 + 32 = 36; `r + p` 42; `t` 21 + 32 = 53; `o`
/// 53 + 16 = 69 `< 2^(6.2−B)`: class [`SQRT_CLASS`] (8) with more than a bit of margin.
fn sqrt_general(p: Mp, q: Mp) -> (Mp, Mp) {
    let r = p.sqr().add(q.sqr()).sqrt();
    let t = r.add(p).ldexp(-1).sqrt();
    (t, q.div(t.ldexp(1)))
}

/// The parts of `csqrt` (before the sign of `y`) when one part of `z` is negligible beside the
/// other (a ratio below 2^-150) and the leading terms are exact: `√p (1 + ρ/8)` and
/// `q/(2√p) (1 − ρ/8)` with `ρ = (q/p)²` for `q ≪ p`; `√(q/2) (1 ± x/(2q))` for `p ≪ q`.
fn sqrt_ratio<F: Float>(a: &Op<F>, p: Mp, q: Mp) -> Option<(Part, Part)> {
    if !p.is_zero() && q.exp < p.exp - 150 {
        let s = mp::exact_sqrt(p)?;
        let t = s.ldexp(1);
        let c = q.div(t).round_bits(120);
        // Not a short dyadic: `q/(2√p)` alone is off by `ρ/8` (up to 2^-303 relative), so the
        // general formula, which is exact in `ρ`, with its proven class.
        let small = if c.mul(t) == q { Part::Dev(c, false) } else { rel(sqrt_general(p, q).1, SQRT_CLASS) };
        let big = Part::Dev(s, true);
        return Some(if a.neg { (small, big) } else { (big, small) });
    }
    if !p.is_zero() && p.exp < q.exp - 150 {
        let s = mp::exact_sqrt(q.ldexp(-1))?;
        return Some((Part::Dev(s, !a.neg), Part::Dev(s, a.neg)));
    }
    None
}

/// `(√((r + p)/2), √((r − p)/2))` with `r = √(p² + q²)` when all three are dyadic, for parts of
/// `prec` bits. If the root `u + iv` is exact, `2uv = q` makes the odd parts of `u` and `v`
/// divide that of `q`, so both have at most `prec` bits, and `|p/q|` and `|q/p|` stay below
/// 2^(prec/2 + 2) (within 64 binary orders for every format); `r = u² + v²` then has at most
/// 2·prec + 1 bits (`u = 1`, `v = 2^64 − 1` gives 127 for x87), its candidate 2·prec + 2.
fn sqrt_exact(p: Mp, q: Mp, prec: u32) -> Option<(Mp, Mp)> {
    if 2 * prec + 2 > 190 {
        // |p/q| <= u/(2v) and |q/p| = 2uv/((u − v)(u + v)) stay below 2^(prec + 1).
        return if p.is_zero() || (p.exp - q.exp).abs() <= i64::from(prec) + 2 { sqrt_exact_wide(p, q, prec) } else { None };
    }
    if !p.is_zero() && (p.exp - q.exp).abs() > 64 {
        return None;
    }
    let r = exact_root(p.sqr().add(q.sqr()), (2 * prec + 2).max(120))?;
    Some((mp::exact_sqrt(r.add(p).ldexp(-1))?, mp::exact_sqrt(r.sub(p).ldexp(-1))?))
}

/// [`sqrt_exact`] for `_Float128`, where `r` (up to 227 bits) has a square beyond the 384: the
/// candidates `u` and `v` (at most `prec` bits each, see above) are the approximations rounded to
/// `prec` bits, and the root is exact when `(u + iv)² = p + iq` exactly (`u²`, `v²` and `uv` have at
/// most 226 bits, and `u² − v²` spans at most 226 + 128 bits, all checked by `mul_x`/`add_x`).
fn sqrt_exact_wide(p: Mp, q: Mp, prec: u32) -> Option<(Mp, Mp)> {
    let r = p.sqr().add(q.sqr()).sqrt();
    let u = r.add(p).ldexp(-1).sqrt().round_bits(prec);
    if u.is_zero() {
        return None;
    }
    let v = q.div(u.ldexp(1)).round_bits(prec);
    let (u2, e1) = mp::mul_x(u, u);
    let (v2, e2) = mp::mul_x(v, v);
    let (d, e3) = mp::add_x(u2, v2.neg());
    let (uv, e4) = mp::mul_x(u, v);
    (e1 && e2 && e3 && e4 && d == p && uv.ldexp(1) == q).then_some((u, v))
}

/// The square root of an exact `v >= 0` when it is a dyadic number of at most `bits` significant
/// bits (`bits <= 190`, so the candidate's square is exact here): the approximation rounded to
/// `bits` bits (to nearest, ties away), squared exactly.
fn exact_root(v: Mp, bits: u32) -> Option<Mp> {
    if bits <= 120 || v.is_zero() {
        return mp::exact_sqrt(v);
    }
    let s = v.sqrt();
    let sh = i64::from(bits) - s.exp;
    let t = s.ldexp(sh).add(Mp::from_f64(0.5));
    let r = t.sub(t.frac()).ldexp(-sh);
    if r.sqr() == v { Some(r) } else { None }
}

// ------------------------------------------------------------- trigonometric and hyperbolic

/// A factor of a product: an exact signed zero, or a value (exact or not).
#[derive(Clone, Copy, Debug)]
enum Fac {
    Z(bool),
    V(Approx),
}

/// The product of two factors (a zero's sign by the rule of signs).
fn mulf(a: Fac, b: Fac) -> Part {
    match (a, b) {
        (Fac::Z(n), Fac::Z(m)) => Part::Zero(n != m),
        (Fac::Z(n), Fac::V(v)) | (Fac::V(v), Fac::Z(n)) => Part::Zero(n != v.v.neg),
        // Two exact factors are 1 and a value (the product is exact); otherwise `Approx::mul`.
        (Fac::V(u), Fac::V(v)) => {
            if u.a <= EXACT && v.a <= EXACT { Part::Exact(u.v.mul(v.v)) } else { u.mul(v).part() }
        }
    }
}

/// `(sinh h, cosh h)` of a finite `h`.
fn hyp<F: Float>(h: &Op<F>) -> (Fac, Fac) {
    if h.k == K::Zero {
        return (Fac::Z(h.neg), Fac::V(Approx::exact(Mp::from_i64(1))));
    }
    let hm = h.mp();
    if hm.abs().to_f64() >= LIM {
        let r = hyp_rel(hm);
        return (Fac::V(Approx::rel(huge(h.neg), r)), Fac::V(Approx::rel(huge(false), r)));
    }
    let (s, c) = mp::sinh_cosh(hm);
    let r = hyp_rel(hm);
    (Fac::V(Approx::rel(s, r)), Fac::V(Approx::rel(c, r)))
}

/// `(sin t, cos t)` of a finite `t` (Payne–Hanek over the whole exponent range of the format).
fn circ<F: Float>(t: &Op<F>) -> (Fac, Fac) {
    if t.k == K::Zero {
        return (Fac::Z(t.neg), Fac::V(Approx::exact(Mp::from_i64(1))));
    }
    let (s, c) = t.sin_cos();
    let e = trig_abs(t.mp().exp);
    (Fac::V(Approx::fix(s, e)), Fac::V(Approx::fix(c, e)))
}

/// `csinh(x + iy) = sinh x cos y + i cosh x sin y`.
pub(super) fn csinh<F: Real>(x: F, y: F, mode: Rounding) -> Res<F> {
    derived_for::<F>();
    let (a, b) = (Op::new(x), Op::new(y));
    let s = sn(a.snan || b.snan);
    if a.finite() && b.finite() {
        if b.k == K::Zero {
            let (r, st) = hyper::sinh(x, mode);
            return ((r, y), st);
        }
        if a.k == K::Zero {
            return two((zero(a.neg != signs_op(&b).1), Status::OK), trig::sin(y, mode));
        }
        if a.tiny() && b.tiny() {
            // x (1 + x²/6 − y²/2), y (1 + x²/2 − y²/6).
            let (xm, ym) = (a.mp(), b.mp());
            return pair(Part::Dev(xm, dir(sq_diff(xm, 1, ym, 3), false)), Part::Dev(ym, dir(sq_diff(xm, 3, ym, 1), false)), mode);
        }
        let ((sh, ch), (sy, cy)) = (hyp(&a), circ(&b));
        return pair(mulf(sh, cy), mulf(ch, sy), mode);
    }
    match a.k {
        K::Zero => match b.k {
            K::Inf => ((x, dn()), Status::DOMAIN),
            _ => ((x, b.quiet()), s),
        },
        K::Fin => match b.k {
            K::Inf => ((qn(), qn()), Status::DOMAIN),
            _ => ((qn(), qn()), Status::INVALID),
        },
        K::Inf => match b.k {
            K::Zero => ((x, y), Status::OK),
            K::Fin => {
                let (sy, cy) = signs_op(&b);
                ((inf(a.neg != cy), inf(sy)), Status::OK)
            }
            K::Inf => ((inf(false), dn()), Status::DOMAIN),
            K::Nan => ((inf(false), b.quiet()), s),
        },
        K::Nan => match b.k {
            K::Zero => ((qn(), y), s),
            _ => ((qn(), qn()), s),
        },
    }
}

/// `ccosh(x + iy) = cosh x cos y + i sinh x sin y`.
pub(super) fn ccosh<F: Real>(x: F, y: F, mode: Rounding) -> Res<F> {
    derived_for::<F>();
    let (a, b) = (Op::new(x), Op::new(y));
    let s = sn(a.snan || b.snan);
    if a.finite() && b.finite() {
        if b.k == K::Zero {
            let (r, st) = hyper::cosh(x, mode);
            return ((r, zero(a.neg != b.neg)), st);
        }
        if a.k == K::Zero {
            return two(trig::cos(y, mode), (zero(a.neg != signs_op(&b).0), Status::OK));
        }
        if a.tiny() && b.tiny() {
            // 1 + (x² − y²)/2 and xy (1 + (x² − y²)/6); at |x| = |y| both fall (−x⁴/6, −x⁶/90).
            let (xm, ym) = (a.mp(), b.mp());
            let d = dir(sq_diff(xm, 1, ym, 1), false);
            return pair(Part::Dev(Mp::from_i64(1), d), Part::Dev(xm.mul(ym), d), mode);
        }
        let ((sh, ch), (sy, cy)) = (hyp(&a), circ(&b));
        return pair(mulf(ch, cy), mulf(sh, sy), mode);
    }
    match a.k {
        K::Zero => match b.k {
            K::Inf => ((dn(), zero(false)), Status::DOMAIN),
            _ => ((b.quiet(), zero(false)), s),
        },
        K::Fin => match b.k {
            K::Inf => ((dn(), qn()), Status::DOMAIN),
            _ => ((b.quiet(), qn()), s),
        },
        K::Inf => match b.k {
            K::Zero => ((inf(false), zero(a.neg != b.neg)), Status::OK),
            K::Fin => {
                let (sy, cy) = signs_op(&b);
                ((inf(cy), inf(a.neg != sy)), Status::OK)
            }
            K::Inf => ((inf(false), dn()), Status::DOMAIN),
            K::Nan => ((inf(false), b.quiet()), s),
        },
        K::Nan => match b.k {
            K::Zero => ((qn(), y), s),
            _ => ((qn(), qn()), s),
        },
    }
}

/// `csin(x + iy) = sin x cosh y + i cos x sinh y`.
pub(super) fn csin<F: Real>(x: F, y: F, mode: Rounding) -> Res<F> {
    derived_for::<F>();
    let (a, b) = (Op::new(x), Op::new(y));
    let s = sn(a.snan || b.snan);
    if a.finite() && b.finite() {
        if b.k == K::Zero {
            let (r, st) = trig::sin(x, mode);
            let cneg = a.k != K::Zero && signs_op(&a).1;
            return ((r, zero(cneg != b.neg)), st);
        }
        if a.k == K::Zero {
            let (r, st) = hyper::sinh(y, mode);
            return ((x, r), st);
        }
        if a.tiny() && b.tiny() {
            // x (1 − x²/6 + y²/2), y (1 − x²/2 + y²/6).
            let (xm, ym) = (a.mp(), b.mp());
            return pair(Part::Dev(xm, dir(sq_diff(ym, 3, xm, 1), false)), Part::Dev(ym, dir(sq_diff(ym, 1, xm, 3), false)), mode);
        }
        let ((sh, ch), (sx, cx)) = (hyp(&b), circ(&a));
        return pair(mulf(sx, ch), mulf(cx, sh), mode);
    }
    match a.k {
        K::Zero => match b.k {
            K::Inf => ((x, y), Status::OK),
            _ => ((x, qn()), s),
        },
        K::Fin => match b.k {
            K::Inf => {
                let (sx, cx) = signs_op(&a);
                ((inf(sx), inf(cx != b.neg)), Status::OK)
            }
            _ => ((qn(), qn()), s),
        },
        K::Inf => match b.k {
            K::Zero => ((dn(), y), Status::DOMAIN),
            K::Fin => ((qn(), qn()), Status::DOMAIN),
            K::Inf => ((dn(), inf(false)), Status::DOMAIN),
            K::Nan => ((qn(), qn()), s),
        },
        K::Nan => {
            let ax = float::with_sign(a.quiet(), false);
            match b.k {
                K::Zero => ((ax, y), s),
                K::Fin => ((qn(), qn()), Status::INVALID),
                K::Inf => ((ax, inf(false)), s),
                K::Nan => ((qn(), qn()), s),
            }
        }
    }
}

/// `ccos(x + iy) = cos x cosh y − i sin x sinh y`.
pub(super) fn ccos<F: Real>(x: F, y: F, mode: Rounding) -> Res<F> {
    derived_for::<F>();
    let (a, b) = (Op::new(x), Op::new(y));
    let s = sn(a.snan || b.snan);
    if a.finite() && b.finite() {
        if b.k == K::Zero {
            let (r, st) = trig::cos(x, mode);
            let sneg = if a.k == K::Zero { a.neg } else { signs_op(&a).0 };
            return ((r, zero(sneg == b.neg)), st);
        }
        if a.k == K::Zero {
            let (r, st) = hyper::cosh(y, mode);
            return ((r, zero(a.neg == b.neg)), st);
        }
        if a.tiny() && b.tiny() {
            // 1 + (y² − x²)/2 and −xy (1 + (y² − x²)/6); at |x| = |y| both fall.
            let (xm, ym) = (a.mp(), b.mp());
            let d = dir(sq_diff(ym, 1, xm, 1), false);
            return pair(Part::Dev(Mp::from_i64(1), d), Part::Dev(xm.mul(ym).neg(), d), mode);
        }
        let ((sh, ch), (sx, cx)) = (hyp(&b), circ(&a));
        return pair(mulf(cx, ch), mulf(sx, sh).neg(), mode);
    }
    match a.k {
        K::Zero => match b.k {
            K::Inf => ((inf(false), zero(a.neg == b.neg)), Status::OK),
            _ => ((qn(), x), s),
        },
        K::Fin => match b.k {
            K::Inf => {
                let (sx, cx) = signs_op(&a);
                ((inf(cx), inf(sx == b.neg)), Status::OK)
            }
            _ => ((qn(), qn()), s),
        },
        K::Inf => match b.k {
            K::Zero => ((dn(), zero(false)), Status::DOMAIN),
            K::Fin => ((dn(), qn()), Status::DOMAIN),
            K::Inf => ((inf(false), dn()), Status::DOMAIN),
            K::Nan => ((qn(), qn()), s),
        },
        K::Nan => match b.k {
            K::Zero => ((a.quiet(), zero(false)), s),
            K::Fin => ((a.quiet(), qn()), s),
            K::Inf => ((inf(false), a.quiet()), s),
            K::Nan => ((qn(), qn()), s),
        },
    }
}

/// The parts of `tanh(h + it)` for finite nonzero `h`, `t`: `(sinh h cosh h / D, sin t cos t / D)`
/// with `D = sinh² h + cos² t`, the first as `±(1 + δ)` far from the axis.
///
/// The cut-offs hold for every format: beyond `|h| = 24` the real part is `1 + δ` with `|δ| <
/// 4e^(−48) < 2^-67`, computed from its own formula so that no digit of `δ` is lost beside the
/// 1 (an x87 last place below 1 is 2^-64, so even there `δ` decides the rounding with 300 bits to
/// spare); below 2^-200, `δ` is far under half a last place of every format up to 113 bits and
/// cannot be zero (`cos 2t = −e^(−2|h|)` has no solution in representable `t`, `h`), so `±1` with
/// its side is exact enough; beyond `|h| = 2^39`, `e^(−2|h|)` is far below every format.
fn tan_core<F: Float>(hop: &Op<F>, top: &Op<F>) -> (Part, Part) {
    let (s, c) = top.sin_cos();
    let neg = hop.neg;
    let x = hop.abs_mp();
    let ha = x.to_f64();
    if ha >= LIM {
        // δ = −(cos 2t + e^(−2|h|))/(2D) has the sign of −cos 2t; D is beyond every format.
        let cos2_neg = c.sub(s).mul(c.add(s)).neg;
        return (Part::Near(1.0, cos2_neg).with_neg(neg), Part::Dev(tiny(s.neg != c.neg), true));
    }
    let (sh, ch) = mp::sinh_cosh(x);
    let (sh, ch) = (Approx::rel(sh, hyp_rel(x)), Approx::rel(ch, hyp_rel(x)));
    let te = trig_abs(top.mp().exp);
    let (s, c) = (Approx::fix(s, te), Approx::fix(c, te));
    let d = sh.mul(sh).add(c.mul(c));
    let trig_part = s.mul(c).div(d).part();
    let hyp_part = if ha > 24.0 {
        let cos2 = c.sub(s).mul(c.add(s));
        let e2 = Approx::rel(mp::exp(x.ldexp(1).neg()), exp_rel(x.ldexp(1)));
        let delta = cos2.add(e2).div(d.ldexp(1)).neg();
        // `±1` with its side only when the sign of δ is certain (otherwise the bounded sum, which
        // is then undecided at this width).
        if delta.v.exp < -200 && delta.a < delta.ex() - 1 {
            Part::Near(1.0, !delta.v.neg)
        } else {
            Approx::exact(Mp::from_i64(1)).add(delta).part()
        }
    } else {
        sh.mul(ch).div(d).part()
    };
    (hyp_part.with_neg(neg), trig_part)
}

/// `ctanh(x + iy)`.
pub(super) fn ctanh<F: Real>(x: F, y: F, mode: Rounding) -> Res<F> {
    derived_for::<F>();
    let (a, b) = (Op::new(x), Op::new(y));
    let s = sn(a.snan || b.snan);
    if a.finite() && b.finite() {
        if a.k == K::Zero {
            let (t, st) = trig::tan(y, mode);
            return ((x, t), st);
        }
        if b.k == K::Zero {
            let (t, st) = hyper::tanh(x, mode);
            return ((t, y), st);
        }
        if a.tiny() && b.tiny() {
            // tanh z = z − z³/3: x (1 − (x² − 3y²)/3), y (1 − (3x² − y²)/3).
            let (xm, ym) = (a.mp(), b.mp());
            return pair(Part::Dev(xm, dir(sq_diff(ym, 3, xm, 1), false)), Part::Dev(ym, dir(sq_diff(ym, 1, xm, 3), false)), mode);
        }
        let (hp, tp) = tan_core(&a, &b);
        return pair(hp, tp, mode);
    }
    match a.k {
        K::Zero => match b.k {
            K::Inf => ((x, qn()), Status::DOMAIN),
            _ => ((x, qn()), s),
        },
        K::Fin => match b.k {
            K::Inf => ((qn(), qn()), Status::DOMAIN),
            _ => ((qn(), qn()), s),
        },
        K::Inf => {
            let im = if b.k == K::Fin {
                let (sy, cy) = signs_op(&b);
                zero(sy != cy)
            } else {
                zero(b.neg)
            };
            ((one(a.neg), im), s)
        }
        K::Nan => match b.k {
            K::Zero => ((x, y), s),
            K::Inf => ((qn(), qn()), Status::INVALID),
            _ => ((qn(), qn()), s),
        },
    }
}

/// `ctan(x + iy) = −i ctanh(ix)`: `(sin x cos x + i sinh y cosh y) / (cos² x + sinh² y)`.
pub(super) fn ctan<F: Real>(x: F, y: F, mode: Rounding) -> Res<F> {
    derived_for::<F>();
    let (a, b) = (Op::new(x), Op::new(y));
    let s = sn(a.snan || b.snan);
    if a.finite() && b.finite() {
        if b.k == K::Zero {
            let (t, st) = trig::tan(x, mode);
            return ((t, y), st);
        }
        if a.k == K::Zero {
            let (t, st) = hyper::tanh(y, mode);
            return ((x, t), st);
        }
        if a.tiny() && b.tiny() {
            // tan z = z + z³/3: x (1 + (x² − 3y²)/3), y (1 + (3x² − y²)/3).
            let (xm, ym) = (a.mp(), b.mp());
            return pair(Part::Dev(xm, dir(sq_diff(xm, 1, ym, 3), false)), Part::Dev(ym, dir(sq_diff(xm, 3, ym, 1), false)), mode);
        }
        let (hp, tp) = tan_core(&b, &a);
        return pair(tp, hp, mode);
    }
    match a.k {
        K::Zero => match b.k {
            K::Inf => ((x, one(b.neg)), Status::OK),
            _ => ((x, y), s),
        },
        K::Fin => match b.k {
            K::Inf => {
                let (sx, cx) = signs_op(&a);
                ((zero(sx != cx), one(b.neg)), Status::OK)
            }
            _ => ((qn(), qn()), s),
        },
        K::Inf => match b.k {
            K::Zero => ((qn(), y), Status::DOMAIN),
            K::Fin => ((qn(), qn()), Status::DOMAIN),
            K::Inf => ((zero(a.neg), one(b.neg)), Status::OK),
            K::Nan => ((qn(), qn()), Status::INVALID),
        },
        K::Nan => match b.k {
            K::Zero => ((qn(), y), s),
            K::Inf => ((zero(a.neg), one(b.neg)), s),
            _ => ((qn(), qn()), s),
        },
    }
}

// --------------------------------------------------------------------- inverse functions

/// For `a = |x|`, `b = |y|` (finite, `a > 0`, `b > 0`): `(Q, I)` with `Q = √((A − a)(A + a))`
/// and `I = acosh A = ln(1 + (A − 1) + √((A − 1)(A + 1)))`, where `A = (|z + 1| + |z − 1|)/2`;
/// `A − 1` and `A − a` are sums of positive terms (Hull, Fairgrieve and Tang), so nothing
/// cancels. `casin(a + ib) = atan2(a, Q) + iI`, `cacos(x + ib) = atan2(Q, x) − iI`.
///
/// Bounds (lemma above [`ln1p_class`], relative, units of `u`): `b²` 2, `a + 1` 6, `a − 1` 4;
/// `r` (14 → 20) 42, `s` (10 → 16) 40; `r + (a + 1)` 48, `t1` 66; then `s ± (a − 1)` (both
/// positive) 46, `b²/(…)` 64, `A − 1` 72 and `A − a` 78 (a sum of two of them, or `A − 1 + (1 − a)`);
/// `A` 78, `A + a` 84, `Q` (78 + 84 + 2)/2 + 32 = 114 `< 2^(7−B)`: class [`Q_CLASS`]. `I`: `A + 1`
/// 84, `(A − 1)(A + 1)` 158, its root 111, the argument of `ln1p` 117 `< 2^(8−B)`: the part
/// carries [`ln1p_class`].
fn asin_parts(a: Mp, b: Mp) -> (Mp, Part) {
    let one = Mp::from_i64(1);
    let (ap1, am1) = (a.add(one), a.sub(one));
    let b2 = b.sqr();
    let r = ap1.sqr().add(b2).sqrt();
    let s = am1.sqr().add(b2).sqrt();
    let t1 = b2.div(r.add(ap1)); // r − (a + 1)
    let (amo, amx) = if am1.neg {
        let amo = t1.add(b2.div(s.sub(am1))).ldexp(-1); // s − (1 − a) = b²/(s + 1 − a)
        (amo, amo.add(am1.neg()))
    } else {
        let amo = t1.add(s.add(am1)).ldexp(-1);
        (amo, t1.add(b2.div(s.add(am1))).ldexp(-1)) // s − (a − 1) = b²/(s + a − 1)
    };
    let big_a = amo.add(one);
    let q = amx.mul(big_a.add(a)).sqrt();
    let arg = amo.add(amo.mul(big_a.add(one)).sqrt());
    (q, rel(mp::ln1p(arg), ln1p_class(arg)))
}

/// The class of `Q` from [`asin_parts`].
const Q_CLASS: i64 = 7;

/// An angle of [`asin_parts`]: `atan2(n, Q)` or `atan2(Q, d)` with `n`, `d` exact and `Q` within
/// a relative `δ < 2^(Q_CLASS − B)`. Moving `Q` by `δQ` moves the angle `θ` by at most
/// `δ·|sin 2θ|/2 <= δ·θ` (the partial derivatives are `∓n/(n² + Q²)`, `d/(d² + Q²)`), and `atan2`
/// adds `< 2^(13−B)` relative: `2^7 + 2^13 < 2^14`, class 14 with most of a bit of margin.
fn inv_angle(theta: Mp) -> Part {
    // 2^Q_CLASS + 2^13 < 2^14 needs Q_CLASS below 13.
    const _: () = assert!(Q_CLASS < 13);
    rel(theta, 14)
}

/// Where [`far_angle`] applies: a divisor of at least 2^152 (`exp >= 153`) and a quotient below
/// 2^-152 (`n.exp − d.exp <= −153`, as `n < 2^n.exp` and `d >= 2^(d.exp − 1)`).
const FAR_EXP: i64 = 153;

/// `n/d` when it is a dyadic number of at most 120 significant bits (the quotient rounded to 120
/// bits, multiplied back exactly: at most 120 + [`MAX_PREC`] bits, within the 384).
fn exact_quotient(n: Mp, d: Mp) -> Option<Mp> {
    let q = n.div(d).round_bits(120);
    if q.mul(d) == n { Some(q) } else { None }
}
/// The small angle `atan2(n, Q)` (`casin`, `casinh`) or `atan2(Q, d)` (`cacos`, `cacosh` with
/// `x = d > 0`) of [`asin_parts`] at huge operands, as `t (1 ± δ)` with `t = n/d` when `t` is
/// exact: the 384 bits of [`Mp`] cannot see `δ`, so the directed roundings of an exact `t` need
/// its sign (casinf64x(2^64 − 1 − i·LDBL_MAX) upward was `2^-16320` plus one unit).
///
/// Identities (`a, b > 0`, `A = (|z + 1| + |z − 1|)/2`, `B = a/A`, `Q = √(A² − a²)`): with
/// `r = |z + 1|`, `s = |z − 1|`, `A² + B² = (r² + s²)/2 = 1 + a² + b²`, so `Q² = 1 + b² − B²`;
/// and `1 − B² = Q²/A²` gives `Q² (1 − 1/A²) = b²`, so `Q = bA/√(A² − 1)`. Let `e = 1/A²`:
/// `A² − a² = Q² > 0` and `A² − b² = 1 + a² − B² > 0`, so `e < min(1/a², 1/b²)`.
///
/// `atan2(a, Q)` with `d = b >= 2^152`, `t = a/b < 2^-152`: `a/Q = t √(1 − e)`, so the angle is
/// `atan(t √(1 − e)) < t` (below), and above `t (1 − e/2 − e²)(1 − t²/3)`: `δ < 2^-303`.
///
/// `atan2(Q, a)` with `d = a >= 2^152`, `t = b/a < 2^-152`: `Q/a = w = t (1 − e)^(-1/2)`, and
/// `atan w = w − w³/3 + θw⁵` (`0 <= θ <= 1/5`) give `atan w / t − 1 = e/2 − t²/3 + R` with
/// `|R| <= e² + t²e + t⁴`. Since `a²/A² = 1 − (1 + b² − B²)/A²`, `a² (e/2 − t²/3) =
/// (3 − 2b²)/6 − η` with `0 <= η <= (1 + b²)/(2a²)`, and `a²|R| <= (1 + b² + b⁴)/a²`: both
/// together stay below `2(1 + b²)²/a²`. That is below `|3 − 2b²|/6` for every `b` of at most
/// `MAX_PREC` bits: for `b² >= 2`, `2b² − 3 >= b²/2` against `4.5 b⁴/a² = 4.5 b² t²`; for
/// `b² < 2`, `3 − 2b²` is a nonzero multiple of 2^-(2·MAX_PREC − 2) (`2b² = 3` has no dyadic
/// root), at least 2^-144, against `18/a² <= 2^-299`. So the angle lies above `t` exactly when
/// `2b² < 3`, and `δ <= e/2 + t²/3 + |R| < 2^-303`.
///
/// Both `δ` lie far below the 2^-300 that [`Part::Dev`] places the sticky bit at; an inexact `t`
/// (no short dyadic) is at least about 2^-(2·MAX_PREC + 2) relative from every rounding boundary,
/// so the general `Part::Mp` path rounds those right, and this returns `None`.
fn far_angle(n: Mp, d: Mp, above: bool) -> Option<Part> {
    if d.exp >= FAR_EXP && n.exp - d.exp <= -FAR_EXP { exact_quotient(n, d).map(|q| Part::Dev(q, above)) } else { None }
}

/// [`far_angle`] for `cacos`'s real part and `cacosh`'s imaginary part, `atan2(Q, x)` with
/// `x > 0` (for `x < 0` the angle lies next to π).
fn acos_far<F: Float>(a: &Op<F>, b: &Op<F>) -> Option<Part> {
    if a.neg {
        return None;
    }
    let above = b.abs_mp().sqr().ldexp(1).cmp_abs(Mp::from_i64(3)).is_lt();
    far_angle(b.abs_mp(), a.mp(), above)
}

/// For `a = |x|`, `b = |y|` (finite, not both zero, not the pole `a = 1, b = 0`): `(R, θ)` with
/// `catanh(a + ib) = R + iθ/2`, returned as the parts `R` and `θ/2` with their bounds.
///
/// `R = ln1p(4a/den)/4`, `den = (1 − a)² + b²` (lemma above [`ln1p_class`], units of `u`):
/// `1 − a` 4, its square 10, `b²` 2, `den` 16, the quotient 32 `< 2^(8−B)`: [`ln1p_class`].
///
/// `θ = atan2(2b, d)` with `d = (1 − g)(1 + g) − h²` (`g`, `h` the larger and smaller of `a`, `b`)
/// may cancel, so its bound is taken at run time. The product is within `(4 + 6 + 2)u`, `h²`
/// within `2u`, and the difference adds `shr`'s `2^(e_M − B)` for its larger operand `M < 2^e_M`
/// (an exact zero product: `M = h²`): `|d̃ − d| < 15·2^(e_M − B)`, kept as `2^E`, `E = e_M + 5 − B`.
/// The angle moves by at most `2^E · max|∂θ/∂d|` over the `d` between `d̃` and the exact one, with
/// `|∂θ/∂d| = 2b/(4b² + d²)`: at most `1/(2b) <= 2^(−b.exp)` everywhere, and at most
/// `2b/d² < 2^(b.exp + 5 − 2·d̃.exp)` when `E <= d̃.exp − 2` (then every such `d` has
/// `|d| >= 2^(d̃.exp − 1) − 2^E >= 2^(d̃.exp − 2)`). `atan2` adds `< 2^(13−B)` relative, so
/// `< 2^(θ.exp + 14 − B)` absolute. The sum is below twice the larger term; halving `θ` takes
/// the bit off again.
fn atanh_parts(a: Mp, b: Mp) -> (Part, Part) {
    let one = Mp::from_i64(1);
    let den = one.sub(a).sqr().add(b.sqr());
    let t = a.ldexp(2).div(den);
    let r = rel(mp::ln1p(t).ldexp(-2), ln1p_class(t));
    let (big, small) = if a.cmp_abs(b).is_ge() { (a, b) } else { (b, a) };
    let (prod, h2) = (one.sub(big).mul(one.add(big)), small.sqr());
    let d = prod.sub(h2);
    let th = mp::atan2(b.ldexp(1), d);
    let e_m = if prod.is_zero() { h2.exp } else if h2.is_zero() { prod.exp } else { prod.exp.max(h2.exp) };
    let e = e_m + 5 - B;
    let mut f = -b.exp;
    if !d.is_zero() && e <= d.exp - 2 {
        f = f.min(b.exp + 5 - 2 * d.exp);
    }
    (r, Part::Mp(th.ldexp(-1), (e + f).max(th.exp + 14 - B)))
}

/// `√|y|` when `|x| = 1`, `y` is tiny and its square root is exact: there
/// `acosh A = √|y| (1 + |y|/12 + ...)` and `acos(1/A) = √|y| (1 − |y|/12 + ...)`.
///
/// For every precision up to [`MAX_PREC`]: the root of a `y` of at most PREC bits is exact with at
/// most (PREC + 1)/2 bits, so it is never a rounding boundary of PREC + 1 bits; an inexact root
/// lies at least about 2^-(2·PREC + 3) (relative) from every such boundary, far beyond the
/// deviation `|y|/12 < 2^-153`.
fn unit_regime<F: Float>(a: &Op<F>, b: &Op<F>) -> Option<Mp> {
    if a.k == K::Fin && a.is_one() && b.tiny() { mp::exact_sqrt(b.abs_mp()) } else { None }
}

/// `casin(x + iy)`.
pub(super) fn casin<F: Real>(x: F, y: F, mode: Rounding) -> Res<F> {
    derived_for::<F>();
    let (a, b) = (Op::new(x), Op::new(y));
    let s = sn(a.snan || b.snan);
    if a.finite() && b.finite() {
        if a.k == K::Zero {
            let (r, st) = hyper::asinh(y, mode);
            return ((x, r), st);
        }
        if b.k == K::Zero {
            if a.at_most_one() {
                let (r, st) = trig::asin(x, mode);
                return ((r, y), st);
            }
            let im = signed(b.neg, mode, |m| hyper::acosh(a.abs(), m));
            return two(pi4(2, a.neg).round(mode), im);
        }
        if a.tiny() && b.tiny() {
            // casin z = z + z³/6: x (1 + (x² − 3y²)/6), y (1 + (3x² − y²)/6).
            let (xm, ym) = (a.mp(), b.mp());
            return pair(Part::Dev(xm, dir(sq_diff(xm, 1, ym, 3), false)), Part::Dev(ym, dir(sq_diff(xm, 3, ym, 1), false)), mode);
        }
        let (q, i) = asin_parts(a.abs_mp(), b.abs_mp());
        let im = unit_regime(&a, &b).map_or(i, |s| Part::Dev(s, true)).with_neg(b.neg);
        let re = far_angle(a.abs_mp(), b.abs_mp(), false).unwrap_or_else(|| inv_angle(mp::atan2(a.abs_mp(), q)));
        return pair(re.with_neg(a.neg), im, mode);
    }
    match a.k {
        K::Zero => ((x, y), if b.k == K::Inf { Status::OK } else { s }),
        K::Fin => match b.k {
            K::Inf => ((zero(a.neg), y), Status::OK),
            _ => ((qn(), qn()), s),
        },
        K::Inf => match b.k {
            K::Nan => ((qn(), inf(b.neg)), s),
            K::Inf => with_im(pi4(1, a.neg), y, Status::OK, mode),
            _ => with_im(pi4(2, a.neg), inf(b.neg), Status::OK, mode),
        },
        K::Nan => match b.k {
            K::Inf => ((qn(), y), s),
            _ => ((qn(), qn()), s),
        },
    }
}

/// `cacos(x + iy)`.
pub(super) fn cacos<F: Real>(x: F, y: F, mode: Rounding) -> Res<F> {
    derived_for::<F>();
    let (a, b) = (Op::new(x), Op::new(y));
    let s = sn(a.snan || b.snan);
    if a.finite() && b.finite() {
        if a.k == K::Zero {
            return two(pi4(2, false).round(mode), hyper::asinh(negate(y), mode));
        }
        if b.k == K::Zero {
            if a.at_most_one() {
                let (r, st) = trig::acos(x, mode);
                return ((r, zero(!b.neg)), st);
            }
            let re = if a.neg { pi4(4, false) } else { Part::Zero(false) };
            return two(re.round(mode), signed(!b.neg, mode, |m| hyper::acosh(a.abs(), m)));
        }
        let (q, i) = asin_parts(a.abs_mp(), b.abs_mp());
        let re = match unit_regime(&a, &b) {
            Some(s) if !a.neg => Part::Dev(s, false),
            _ => acos_far(&a, &b).unwrap_or_else(|| inv_angle(mp::atan2(q, a.mp()))),
        };
        let im = if a.tiny() && b.tiny() {
            Part::Dev(b.mp(), dir(sq_diff(a.mp(), 3, b.mp(), 1), false))
        } else {
            unit_regime(&a, &b).map_or(i, |s| Part::Dev(s, true)).with_neg(b.neg)
        };
        return pair(re, im.neg(), mode);
    }
    match a.k {
        K::Zero => match b.k {
            K::Inf => with_im(pi4(2, false), inf(!b.neg), Status::OK, mode),
            _ => with_im(pi4(2, false), negate(y), s, mode),
        },
        K::Fin => match b.k {
            K::Inf => with_im(pi4(2, false), inf(!b.neg), Status::OK, mode),
            _ => ((qn(), negate(qn())), s),
        },
        K::Inf => {
            let im = inf(!b.neg);
            match b.k {
                K::Nan => ((qn(), im), s),
                K::Inf => with_im(pi4(if a.neg { 3 } else { 1 }, false), im, Status::OK, mode),
                _ if a.neg => with_im(pi4(4, false), im, Status::OK, mode),
                _ => ((zero(false), im), Status::OK),
            }
        }
        K::Nan => match b.k {
            K::Inf => ((qn(), inf(!b.neg)), s),
            _ => ((qn(), negate(qn())), s),
        },
    }
}

/// `casinh(x + iy) = −i casin(ix)`.
pub(super) fn casinh<F: Real>(x: F, y: F, mode: Rounding) -> Res<F> {
    derived_for::<F>();
    let (a, b) = (Op::new(x), Op::new(y));
    let s = sn(a.snan || b.snan);
    if a.finite() && b.finite() {
        if a.k == K::Zero {
            if b.at_most_one() {
                let (r, st) = trig::asin(y, mode);
                return ((x, r), st);
            }
            let re = signed(a.neg, mode, |m| hyper::acosh(b.abs(), m));
            return two(re, pi4(2, b.neg).round(mode));
        }
        if b.k == K::Zero {
            let (r, st) = hyper::asinh(x, mode);
            return ((r, y), st);
        }
        if a.tiny() && b.tiny() {
            // casinh z = z − z³/6: x (1 − (x² − 3y²)/6), y (1 − (3x² − y²)/6).
            let (xm, ym) = (a.mp(), b.mp());
            return pair(Part::Dev(xm, dir(sq_diff(ym, 3, xm, 1), false)), Part::Dev(ym, dir(sq_diff(ym, 1, xm, 3), false)), mode);
        }
        let (q, i) = asin_parts(b.abs_mp(), a.abs_mp());
        let re = unit_regime(&b, &a).map_or(i, |s| Part::Dev(s, true)).with_neg(a.neg);
        let im = far_angle(b.abs_mp(), a.abs_mp(), false).unwrap_or_else(|| inv_angle(mp::atan2(b.abs_mp(), q)));
        return pair(re, im.with_neg(b.neg), mode);
    }
    match a.k {
        K::Zero | K::Fin => match b.k {
            K::Inf => with_re(inf(a.neg), Status::OK, pi4(2, b.neg), mode),
            _ => ((qn(), qn()), s),
        },
        K::Inf => match b.k {
            K::Nan => ((x, qn()), s),
            K::Inf => with_re(x, Status::OK, pi4(1, b.neg), mode),
            _ => ((x, zero(b.neg)), Status::OK),
        },
        K::Nan => match b.k {
            K::Inf => ((inf(a.neg), qn()), s),
            K::Zero => ((x, y), s),
            _ => ((x, qn()), s),
        },
    }
}

/// `cacosh(x + iy) = acosh A ± i·Re cacos z` (real part ≥ +0).
pub(super) fn cacosh<F: Real>(x: F, y: F, mode: Rounding) -> Res<F> {
    derived_for::<F>();
    let (a, b) = (Op::new(x), Op::new(y));
    let s = sn(a.snan || b.snan);
    if a.finite() && b.finite() {
        if a.k == K::Zero {
            return two(hyper::asinh(b.abs(), mode), pi4(2, b.neg).round(mode));
        }
        if b.k == K::Zero {
            if a.at_most_one() {
                return two((zero(false), Status::OK), signed(b.neg, mode, |m| trig::acos(x, m)));
            }
            let im = if a.neg { pi4(4, b.neg) } else { Part::Zero(b.neg) };
            return two(hyper::acosh(a.abs(), mode), im.round(mode));
        }
        let (q, i) = asin_parts(a.abs_mp(), b.abs_mp());
        let re = if a.tiny() && b.tiny() {
            Part::Dev(b.abs_mp(), dir(sq_diff(a.mp(), 3, b.mp(), 1), false))
        } else {
            unit_regime(&a, &b).map_or(i, |s| Part::Dev(s, true))
        };
        let im = match unit_regime(&a, &b) {
            Some(s) if !a.neg => Part::Dev(s, false),
            _ => acos_far(&a, &b).unwrap_or_else(|| inv_angle(mp::atan2(q, a.mp()))),
        };
        return pair(re, im.with_neg(b.neg), mode);
    }
    match a.k {
        K::Zero => match b.k {
            K::Inf => with_re(inf(false), Status::OK, pi4(2, b.neg), mode),
            _ => with_re(qn(), s, pi4(2, false), mode),
        },
        K::Fin => match b.k {
            K::Inf => with_re(inf(false), Status::OK, pi4(2, b.neg), mode),
            _ => ((qn(), qn()), s),
        },
        K::Inf => match b.k {
            K::Nan => ((inf(false), qn()), s),
            K::Inf => with_re(inf(false), Status::OK, pi4(if a.neg { 3 } else { 1 }, b.neg), mode),
            _ if a.neg => with_re(inf(false), Status::OK, pi4(4, b.neg), mode),
            _ => ((inf(false), zero(b.neg)), Status::OK),
        },
        K::Nan => match b.k {
            K::Inf => ((inf(false), qn()), s),
            _ => ((qn(), qn()), s),
        },
    }
}

/// `catanh(x + iy)`.
pub(super) fn catanh<F: Real>(x: F, y: F, mode: Rounding) -> Res<F> {
    derived_for::<F>();
    let (a, b) = (Op::new(x), Op::new(y));
    let s = sn(a.snan || b.snan);
    if a.finite() && b.finite() {
        if b.k == K::Zero {
            if a.abs_mp().cmp_abs(Mp::from_i64(1)).is_lt() {
                let (r, st) = x.atanh(mode);
                return ((r, y), st);
            }
            if a.is_one() {
                return ((inf(a.neg), y), Status::POLE);
            }
            let (r, _) = atanh_parts(a.abs_mp(), Mp::ZERO);
            return pair(r.with_neg(a.neg), pi4(2, b.neg), mode);
        }
        if a.k == K::Zero {
            let (r, st) = trig::atan(y, mode);
            return ((x, r), st);
        }
        let (xm, ym) = (a.mp(), b.mp());
        if a.tiny() && b.tiny() {
            // catanh z = z + z³/3: x (1 + (x² − 3y²)/3), y (1 + (3x² − y²)/3).
            return pair(Part::Dev(xm, dir(sq_diff(xm, 1, ym, 3), false)), Part::Dev(ym, dir(sq_diff(xm, 3, ym, 1), false)), mode);
        }
        let (r, th) = atanh_parts(a.abs_mp(), b.abs_mp());
        // On the lines y = ±1 next to the axis: x/2 (1 − x²/6 + ...).
        let re = if a.tiny() && b.is_one() { Part::Dev(xm.ldexp(-1), false) } else { r.with_neg(a.neg) };
        return pair(re, th.with_neg(b.neg), mode);
    }
    if b.k == K::Inf {
        return with_re(zero(a.neg), s, pi4(2, b.neg), mode);
    }
    match a.k {
        K::Inf if b.k == K::Nan => ((zero(a.neg), qn()), s),
        K::Inf => with_re(zero(a.neg), Status::OK, pi4(2, b.neg), mode),
        K::Zero => ((x, qn()), s),
        _ => ((qn(), qn()), s),
    }
}

/// `catan(x + iy) = −i catanh(ix)`.
pub(super) fn catan<F: Real>(x: F, y: F, mode: Rounding) -> Res<F> {
    derived_for::<F>();
    let (a, b) = (Op::new(x), Op::new(y));
    let s = sn(a.snan || b.snan);
    if a.finite() && b.finite() {
        if a.k == K::Zero {
            if b.abs_mp().cmp_abs(Mp::from_i64(1)).is_lt() {
                let (r, st) = y.atanh(mode);
                return ((x, r), st);
            }
            if b.is_one() {
                return ((x, inf(b.neg)), Status::POLE);
            }
            let (r, _) = atanh_parts(b.abs_mp(), Mp::ZERO);
            return pair(pi4(2, a.neg), r.with_neg(b.neg), mode);
        }
        if b.k == K::Zero {
            let (r, st) = trig::atan(x, mode);
            return ((r, y), st);
        }
        let (xm, ym) = (a.mp(), b.mp());
        if a.tiny() && b.tiny() {
            // catan z = z − z³/3: x (1 − (x² − 3y²)/3), y (1 − (3x² − y²)/3).
            return pair(Part::Dev(xm, dir(sq_diff(ym, 3, xm, 1), false)), Part::Dev(ym, dir(sq_diff(ym, 1, xm, 3), false)), mode);
        }
        let (r, th) = atanh_parts(b.abs_mp(), a.abs_mp());
        let im = if b.tiny() && a.is_one() { Part::Dev(ym.ldexp(-1), false) } else { r.with_neg(b.neg) };
        return pair(th.with_neg(a.neg), im, mode);
    }
    if a.k == K::Inf {
        return with_im(pi4(2, a.neg), zero(b.neg), Status::OK, mode);
    }
    if b.k == K::Inf {
        return if a.k == K::Nan { ((qn(), zero(b.neg)), s) } else { with_im(pi4(2, a.neg), zero(b.neg), Status::OK, mode) };
    }
    if a.k == K::Nan && b.k == K::Zero {
        return ((qn(), y), s);
    }
    ((qn(), qn()), s)
}

// ----------------------------------------------------------------------------------- power

/// `cpow(x + iy, c + id)`.
pub(super) fn cpow<F: Real>(x: F, y: F, c: F, d: F, mode: Rounding) -> Res<F> {
    derived_for::<F>();
    let (a, b, p, q) = (Op::new(x), Op::new(y), Op::new(c), Op::new(d));
    if !(a.finite() && b.finite() && p.finite() && q.finite()) || (a.k == K::Zero && b.k == K::Zero) {
        return cpow_naive(x, y, c, d, mode);
    }
    if q.k == K::Zero && (a.k == K::Zero || b.k == K::Zero) {
        return pow_axis(&a, &b, &p, &q, mode);
    }
    if q.k == K::Zero {
        if let Some(r) = pow_int(&a, &b, &p, mode) {
            return r;
        }
        if p.mp() == Mp::from_f64(0.5) {
            // The principal square root, with its exact cases (3 + 4i)^½ = 2 + i.
            return csqrt(x, y, mode);
        }
    }
    pow_general(&a, &b, &p, &q, mode)
}

/// `cexp(w · clog z)` with Annex G's multiplication: glibc's `cpow` for the special operands.
fn cpow_naive<F: Real>(x: F, y: F, c: F, d: F, mode: Rounding) -> Res<F> {
    let ((lr, li), s1) = clog(x, y, mode);
    let ((pr, pi_), s2) = cmul(c, d, lr, li, mode);
    let (r, s3) = cexp(pr, pi_, mode);
    (r, s1.and(s2).and(s3))
}

/// IEEE operations in a rounding direction, collecting their flags (no `errno`).
struct Soft {
    st: Status,
    mode: Rounding,
}

impl Soft {
    fn mul<F: Float>(&mut self, u: F, v: F) -> F {
        let (r, s) = fused::mul::<F, F>(u, v, self.mode);
        self.st = self.st.and(s.no_errno());
        r
    }

    fn add<F: Float>(&mut self, u: F, v: F) -> F {
        let (r, s) = fused::add::<F, F>(u, v, self.mode);
        self.st = self.st.and(s.no_errno());
        r
    }

    fn sub<F: Float>(&mut self, u: F, v: F) -> F {
        let (r, s) = fused::sub::<F, F>(u, v, self.mode);
        self.st = self.st.and(s.no_errno());
        r
    }
}

fn is_nan<F: Float>(v: F) -> bool {
    float::decode(v).is_nan()
}

fn is_inf<F: Float>(v: F) -> bool {
    float::decode(v).kind == Kind::Inf
}

/// `copysign(isinf(v) ? 1 : 0, v)`.
fn unit<F: Float>(v: F) -> F {
    let neg = float::decode(v).neg;
    if is_inf(v) { one(neg) } else { zero(neg) }
}

/// `isnan(v) ? copysign(0, v) : v`.
fn zero_nan<F: Float>(v: F) -> F {
    if is_nan(v) { zero(float::decode(v).neg) } else { v }
}

/// `(a + ib)(c + id)` as C Annex G's example `_Cmultd` (G.5.1) computes it: the plain products,
/// and when both parts are NaN, infinities recovered from the operands.
fn cmul<F: Float>(a: F, b: F, c: F, d: F, mode: Rounding) -> Res<F> {
    let mut o = Soft { st: Status::OK, mode };
    let (ac, bd, ad, bc) = (o.mul(a, c), o.mul(b, d), o.mul(a, d), o.mul(b, c));
    let (mut x, mut y) = (o.sub(ac, bd), o.add(ad, bc));
    if is_nan(x) && is_nan(y) {
        let (mut a, mut b, mut c, mut d) = (a, b, c, d);
        let mut recalc = false;
        if is_inf(a) || is_inf(b) {
            (a, b, c, d) = (unit(a), unit(b), zero_nan(c), zero_nan(d));
            recalc = true;
        }
        if is_inf(c) || is_inf(d) {
            (c, d, a, b) = (unit(c), unit(d), zero_nan(a), zero_nan(b));
            recalc = true;
        }
        if !recalc && (is_inf(ac) || is_inf(bd) || is_inf(ad) || is_inf(bc)) {
            (a, b, c, d) = (zero_nan(a), zero_nan(b), zero_nan(c), zero_nan(d));
            recalc = true;
        }
        if recalc {
            let i = inf::<F>(false);
            let (t1, t2) = (o.mul(a, c), o.mul(b, d));
            let t = o.sub(t1, t2);
            x = o.mul(i, t);
            let (u1, u2) = (o.mul(a, d), o.mul(b, c));
            let u = o.add(u1, u2);
            y = o.mul(i, u);
        }
    }
    ((x, y), o.st)
}

/// A base on an axis to a real power (`d = ±0`): the angle `c·k·π/2` reduced exactly, so the
/// parts of `(−8)^(1/3)`, `(−2)^3`, `i^2` and `(−4)^½` are exact where they should be.
fn pow_axis<F: Real>(a: &Op<F>, b: &Op<F>, p: &Op<F>, q: &Op<F>, mode: Rounding) -> Res<F> {
    // |z| and arg z = k·π/2.
    let (mo, k) = if b.k == K::Zero { (a, if !a.neg { 0 } else if b.neg { -2 } else { 2 }) } else { (b, if b.neg { -1 } else { 1 }) };
    let m = mo.abs();
    let ln_neg = mo.at_most_one() && !mo.is_one(); // the sign of ln|z| (+0 at 1)
    if k == 0 || p.k == K::Zero {
        // The angle d·ln|z| + c·arg z is a sum of two zeros.
        let th_neg = if k == 0 { b.neg } else { k < 0 };
        let im = zero(zero_sum(q.neg != ln_neg, p.neg != th_neg, mode));
        let (r, st) = if p.k == K::Zero { (one(false), Status::OK) } else { m.pow(p.v, mode) };
        return ((r, im), st);
    }
    let r = mod2(p.mp().mul(Mp::from_i64(k)).ldexp(-1));
    let ((sn_, cs), exact) = mp::sin_cos_pi(r);
    if exact {
        // One part is ±|z|^c, the other exactly zero.
        let mag = |neg: bool| signed(neg, mode, |mm| m.pow(p.v, mm));
        return if cs.is_zero() { two((zero(false), Status::OK), mag(sn_.neg)) } else { two(mag(cs.neg), (zero(false), Status::OK)) };
    }
    if mo.is_one() {
        // `|z| = 1`: the sine and cosine alone, `< 2^(10−B)` absolute.
        let part = |t: Mp| if t.is_zero() { Part::Zero(false) } else { Part::Mp(t, 10 - B) };
        return pair(part(cs), part(sn_), mode);
    }
    // ln|z| within `2^(25−B)` relative: `ln1p(m − 1)` for `m` in `[½, 2)` (`m − 1` exact for a
    // part of at most 113 bits; `ln1p` `< 2^(22−B)`), else `ln_atanh` (`< 2^(e_ln + 12 − B)` absolute
    // with `|ln m| ≥ ln 2`, so `< 2^(14−B)` relative). `x = c·ln|z|` within `2^(26−B)` relative,
    // `2^(x.exp + 27 − B)` absolute; `exp` adds `2^(14−B)·(|k| + 64) < 2^(15 + max(x.exp + 1, 6) − B)`
    // relative: `e^x` within `2^(max(x.exp, 6) + 29 − B)` relative. A part `e·t` with `t` the sine or
    // cosine (`< 2^(10−B)` absolute, `|t| ≤ 1`) and the product (`ε`): absolute error
    // `< 2^(e.exp + max(x.exp, 6) + 30 − B)` (D-10.17). Beyond `2^39` the exponential is a
    // marker far outside every format: an exact-short part (#1906).
    let m = mo.abs_mp();
    let one = Mp::from_i64(1);
    let l = if m.exp == 0 || m.exp == 1 { mp::ln1p(m.sub(one)) } else { mp::ln_atanh(m) };
    let x = p.mp().mul(l);
    if x.to_f64().abs() >= LIM {
        let e = exp_mp(x);
        return pair(Part::Dev(e.mul(cs), true), Part::Dev(e.mul(sn_), true), mode);
    }
    let e = mp::exp(x);
    let err = e.exp + x.exp.max(6) + 30 - B;
    let part = |t: Mp| if t.is_zero() { Part::Zero(false) } else { Part::Mp(e.mul(t), err) };
    pair(part(cs), part(sn_), mode)
}

/// `v` modulo 2 into `[−1, 1]`, exactly, for an exact `v` (of a few bits more than a format's
/// precision: `c·k/2` or `c·k/4`); any `v` beyond the 384 bits is a multiple of 2.
fn mod2(v: Mp) -> Mp {
    let two = Mp::from_i64(2);
    let r = v.ldexp(-1).frac().ldexp(1); // in (−2, 2), the sign of v
    if r.cmp_abs(Mp::from_i64(1)).is_le() {
        r
    } else if r.neg {
        r.add(two)
    } else {
        r.sub(two)
    }
}

/// An exact part (zero as +0: the sign of an exactly cancelled part is not specified).
fn exact_part(v: Mp) -> Part {
    if v.is_zero() { Part::Zero(false) } else { Part::Exact(v) }
}

/// `v / n`, exact when the quotient is a short dyadic number.
fn quot(v: Mp, n: Mp) -> Part {
    if v.is_zero() {
        return Part::Zero(false);
    }
    let qv = v.div(n);
    let c = qv.round_bits(120);
    let (prod, exact) = mp::mul_x(c, n);
    // Exact operands: only `div`'s relative `2^(4−B)` (absolute `< 2^(ex q + 5 − B)`).
    if exact && prod == v { Part::Exact(c) } else { Part::Mp(qv, qv.exp + 5 - B) }
}

/// `z^n` for an integer exponent `|n| <= 64` (`d = ±0`) when the powers are exact in 384 bits.
fn pow_int<F: Float>(a: &Op<F>, b: &Op<F>, p: &Op<F>, mode: Rounding) -> Option<Res<F>> {
    let c = p.mp();
    if p.k == K::Zero || c.cmp_abs(Mp::from_i64(64)).is_gt() || !c.frac().is_zero() {
        return None;
    }
    let n = c.round_i64();
    let (x, y) = (a.mp(), b.mp());
    let (mut re, mut im) = (x, y);
    for _ in 1..n.abs() {
        let (rx, e1) = mp::mul_x(re, x);
        let (iy, e2) = mp::mul_x(im, y);
        let (ry, e3) = mp::mul_x(re, y);
        let (ix, e4) = mp::mul_x(im, x);
        let (nr, e5) = mp::add_x(rx, iy.neg());
        let (ni, e6) = mp::add_x(ry, ix);
        if !(e1 && e2 && e3 && e4 && e5 && e6) {
            return None;
        }
        (re, im) = (nr, ni);
    }
    if n > 0 {
        return Some(pair(exact_part(re), exact_part(im), mode));
    }
    // z^n = conj(z^|n|) / |z^|n||².
    let (rr, e1) = mp::mul_x(re, re);
    let (ii, e2) = mp::mul_x(im, im);
    let (nn, e3) = mp::add_x(rr, ii);
    if !(e1 && e2 && e3) {
        return None;
    }
    Some(pair(quot(re, nn), quot(im.neg(), nn), mode))
}

/// `(sin φ, cos φ)` of any finite `φ` (beyond 2^40, reduced by whole turns in this precision:
/// only an exponent `w` near 2^58 or above gets that far with a representable result).
///
/// Absolute error of each: `φ`'s own (sine and cosine are 1-Lipschitz) plus the kernel's. Up to
/// 2^40, `mp::sin_cos`'s `2^(max(ex φ, 4) + 6 − B)` ([`trig_abs`]). Beyond: `2π` truncated
/// (`< 2^(1−B)` relative), its `recip` (`< 2^(4−B)` with that), the product `t` (`< 2^(5−B)`, so
/// `< 2^(ex φ + 3 − B)` absolute for `|t| < 2^(ex φ − 2)`); `frac` is exact and moves `t` by an
/// integer, which the period absorbs; `frac·2π` adds `< 2^(ex φ + 6 − B) + 2^(5−B)` and
/// `mp::sin_cos` of `|r| < 2^3` `< 2^(10−B)`: below `2^(ex φ + 8 − B)` for `ex φ > 40`.
fn sin_cos_any(phi: Approx) -> (Approx, Approx) {
    let (k, (s, c)) = if phi.v.exp > 40 {
        let turn = mp::pi().ldexp(1);
        (phi.v.exp + 8 - B, mp::sin_cos(phi.v.mul(turn.recip()).frac().mul(turn)))
    } else {
        (trig_abs(phi.v.exp), mp::sin_cos(phi.v))
    };
    let a = phi.a.max(k) + 1;
    (Approx::fix(s, a), Approx::fix(c, a))
}

/// The general case: `e^E cis Φ` with `E = c ln|z| − d arg z`, `Φ = d ln|z| + c arg z`.
fn pow_general<F: Float>(a: &Op<F>, b: &Op<F>, p: &Op<F>, q: &Op<F>, mode: Rounding) -> Res<F> {
    // The bases −1 and ±i: |z| = 1, so the angle c·k·π/2 is reduced exactly and only the
    // magnitude e^(−d·k·π/2) is approximate (the base 1 is exact below).
    let on_axis = a.k == K::Zero || b.k == K::Zero;
    if on_axis && (if a.k == K::Zero { b.is_one() } else { a.is_one() }) && !(b.k == K::Zero && !a.neg) {
        let k: i64 = if b.k == K::Zero { if b.neg { -2 } else { 2 } } else if b.neg { -1 } else { 1 };
        let r = mod2(p.mp().mul(Mp::from_i64(k)).ldexp(-1));
        let ((sn_, cs), exact) = mp::sin_cos_pi(r);
        // π truncated (`< 2^(2−B)` absolute below 4), then exact `k`, `d` products bounded.
        let pi2 = Approx::fix(mp::pi(), 2 - B).ldexp(-1);
        let mag = exp_approx(Approx::exact(q.mp()).mul(pi2.mul(Approx::exact(Mp::from_i64(k)))).neg());
        // `sin_cos_pi` of the exact reduced `r`: `< 2^(10−B)` absolute (exact at the multiples of ½).
        let part = |v: Mp| if exact && v.is_zero() { Part::Zero(false) } else { mag.mul(Approx::fix(v, if exact { EXACT } else { 10 - B })).part() };
        return pair(part(cs), part(sn_), mode);
    }
    // A base on a diagonal (|x| = |y|): arg z = k·π/4, and with a real exponent the angle
    // c·k·π/4 is reduced exactly (so (1 + i)^(10^10) is real). The only other bases whose
    // argument is a rational multiple of π are those on the axes (Niven's theorem).
    if q.k == K::Zero && p.k != K::Zero && a.k != K::Zero && a.abs_mp() == b.abs_mp() {
        let k: i64 = match (a.neg, b.neg) {
            (false, false) => 1,
            (true, false) => 3,
            (true, true) => -3,
            (false, true) => -1,
        };
        let r = mod2(p.mp().mul(Mp::from_i64(k)).ldexp(-2));
        let ((sn_, cs), exact) = mp::sin_cos_pi(r);
        let (lr, lc, _) = ln_abs(a.abs_mp(), b.abs_mp());
        let mag = exp_approx(Approx::exact(p.mp()).mul(Approx::rel(lr, lc - B)));
        let part = |v: Mp| if exact && v.is_zero() { Part::Zero(false) } else { mag.mul(Approx::fix(v, if exact { EXACT } else { 10 - B })).part() };
        return pair(part(cs), part(sn_), mode);
    }
    let (xm, ym) = (a.mp(), b.mp());
    let (lr, lc, lr_zero) = ln_abs(xm.abs(), ym.abs());
    let th_zero = b.k == K::Zero && !a.neg;
    let th = if th_zero {
        Mp::ZERO
    } else if b.k == K::Zero {
        if b.neg { mp::pi().neg() } else { mp::pi() }
    } else {
        mp::atan2(ym, xm)
    };
    let (cm, dm) = (p.mp(), q.mp());
    // ln|z| of class `lc` (relative `2^(lc−B)`); the angle: π truncated (`2^(1−B)` relative),
    // `atan2`'s `2^(13−B)`; zero exactly.
    let lr = if lr_zero { Approx::exact(Mp::ZERO) } else { Approx::rel(lr, lc - B) };
    let th = if th_zero { Approx::exact(th) } else { Approx::rel(th, if b.k == K::Zero { 1 } else { 13 } - B) };
    let (cm, dm) = (Approx::exact(cm), Approx::exact(dm));
    let e_zero = (p.k == K::Zero || lr_zero) && (q.k == K::Zero || th_zero);
    let phi_zero = (q.k == K::Zero || lr_zero) && (p.k == K::Zero || th_zero);
    let one = Mp::from_i64(1);
    let mag = if e_zero { Fac::V(Approx::exact(one)) } else { Fac::V(exp_approx(cm.mul(lr).sub(dm.mul(th)))) };
    if phi_zero {
        let ln_neg = !lr_zero && lr.v.neg;
        let neg = zero_sum(q.neg != ln_neg, p.neg != b.neg, mode);
        return pair(mulf(mag, Fac::V(Approx::exact(one))), Part::Zero(neg), mode);
    }
    let (s, co) = sin_cos_any(dm.mul(lr).add(cm.mul(th)));
    // #1905: with huge `|c|`, `|d|` (E and Φ cancelling between terms up to about 2^1000) the
    // bounds exceed what 768 bits can decide: the part takes the counted `TODO(ZIV-C)` path,
    // where ZIV-C's ladder chooses the precision from the exponents of `c` and `d`.
    pair(mulf(mag, Fac::V(co)), mulf(mag, Fac::V(s)), mode)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::float_cmp)]
mod tests {
    use super::*;

    /// `_Float128` cut-offs (M5-4): an exact root whose parts are 112 binary orders apart, an
    /// exact `|z|² − 1` of 226 bits beside the 1 (below 2^-150, above the 2^-230 cut-off), and an
    /// inexact sum of squares that the 384 bits would round onto `10^2`.
    #[test]
    fn f128_cutoffs_are_derived_for_113_bits() {
        use crate::float::F128;
        let q = |v: Mp| mp::round_exact::<F128>(v, Rounding::Nearest).0;
        let one = Mp::from_i64(1);
        let (u, v) = (one.ldexp(112), one.ldexp(112).sub(one));
        let (p, im) = (u.sqr().sub(v.sqr()), u.mul(v).ldexp(1));
        for m in [Rounding::Nearest, Rounding::Up, Rounding::Down, Rounding::Zero] {
            let ((re, ii), st) = csqrt::<F128>(q(p), q(im), m);
            assert_eq!((re, ii, st), (q(u), q(v), Status::OK), "{m:?}");
        }
        // z = 1 + i(2^112 + 1)·2^-200: t = y² ≈ 2^-176 (225 bits after the halving) lies 2^-224
        // (relative) above the 113-bit R = 2^-177 (1 + 2^-111); ln|z| = t/2 (1 − t/2 + ...) lies
        // 2^-177 below t/2, so below R: toward zero it is R's predecessor (the 2^-150 cut-off of
        // the narrower formats would give R).
        let y = one.ldexp(112).add(one).ldexp(-200);
        let t = y.sqr();
        let ((re, _), _) = clog::<F128>(q(one), q(y), Rounding::Zero);
        let want = mp::round::<F128>(t.ldexp(-1).sub(t.sqr().ldexp(-2)), Rounding::Zero).0;
        let r = mp::round_exact::<F128>(one.add(one.ldexp(-111)).ldexp(-177), Rounding::Nearest).0;
        assert!(want != r);
        assert_eq!(re, want);
        // clog10(6 + 8i) = 1 + i·atan2(8, 6)/ln 10: still exact (the sum of squares is exact).
        let ((re, _), _) = clog10::<F128>(q(Mp::from_i64(6)), q(Mp::from_i64(8)), Rounding::Up);
        assert_eq!(re, q(one));
    }

    /// The small angles of the inverse functions at huge operands are an exact `2^-200` (or
    /// `1.5 · 2^-200`) times `1 ± δ` with `δ` near 2^-400, beyond the 384 bits ([`far_angle`]):
    /// the directed roundings fall on the side of `δ`'s sign (the regime of the logged x87 lines,
    /// e.g. casinf64x upward at `2^64 − 1 − i·LDBL_MAX`, `2^-16320` exactly).
    #[test]
    fn far_angles_round_on_the_side_of_their_deviation() {
        let t = 2f64.powi(-200);
        let (big, up, dn) = (2f64.powi(200), Rounding::Up, Rounding::Down);
        let below = f64::from_bits(t.to_bits() - 1);
        let above = f64::from_bits(t.to_bits() + 1);
        // atan2(a, Q) < a/b always.
        assert_eq!(casin::<f64>(1.0, -big, up).0.0, t);
        assert_eq!(casin::<f64>(1.0, -big, dn).0.0, below);
        assert_eq!(casinh::<f64>(big, 1.0, up).0.1, t);
        assert_eq!(casinh::<f64>(big, 1.0, Rounding::Zero).0.1, below);
        // atan2(Q, a) > b/a exactly when 2b² < 3.
        assert_eq!(cacos::<f64>(big, 1.0, up).0.0, above);
        assert_eq!(cacos::<f64>(big, 1.0, dn).0.0, t);
        assert_eq!(cacosh::<f64>(big, -1.0, dn).0.1, -above);
        let t15 = 1.5 * t;
        assert_eq!(cacos::<f64>(big, 1.5, up).0.0, t15);
        assert_eq!(cacos::<f64>(big, 1.5, dn).0.0, f64::from_bits(t15.to_bits() - 1));
        assert_eq!(cacosh::<f64>(big, 1.5, up).0.1, t15);
    }

    const N: Rounding = Rounding::Nearest;

    #[test]
    fn exact_parts_are_exact() {
        assert_eq!(csqrt(-4.0f64, 0.0, N), ((0.0, 2.0), Status::OK));
        assert_eq!(csqrt(3.0f64, 4.0, N), ((2.0, 1.0), Status::OK));
        assert_eq!(csqrt(0.0f64, 2.0, N), ((1.0, 1.0), Status::OK));
        assert_eq!(csqrt(-3.0f64, -4.0, Rounding::Up), ((1.0, -2.0), Status::OK));
        assert_eq!(clog10(6.0f64, 8.0, Rounding::Down).0 .0, 1.0);
        assert_eq!(cpow(1.0f64, 1.0, 2.0, 0.0, Rounding::Up).0, (0.0, 2.0));
        assert_eq!(cpow(-8.0f64, 0.0, 2.0, 0.0, N).0, (64.0, 0.0));
        assert_eq!(cpow(-4.0f64, 0.0, 0.5, 0.0, N).0, (0.0, 2.0));
        assert_eq!(cpow(0.0f64, 1.0, 2.0, 0.0, N).0, (-1.0, 0.0));
        assert_eq!(cpow(1.0f64, 1.0, -2.0, 0.0, N).0, (0.0, -0.5));
        assert_eq!(ccosh(0.0f64, 0.0, N), ((1.0, 0.0), Status::OK));
    }

    #[test]
    fn x87_exact_parts_are_exact() {
        use crate::float::F80;
        let v = |neg: bool, m: u128, e: i64| mp::round_exact::<F80>(Mp::from_u128(neg, m, e), N).0;
        let (one, two_, six, eight) = (v(false, 1, 0), v(false, 1, 1), v(false, 3, 1), v(false, 1, 3));
        let z: F80 = float::zero(false);
        // (1 + i(2^64 − 1))²: |z| = 1 + (2^64 − 1)² has 127 bits, beyond a 120-bit candidate.
        let (p, q) = (v(true, (1 << 63) - 1, 65), v(false, (1 << 64) - 1, 1));
        assert_eq!(csqrt(p, q, Rounding::Up), ((one, v(false, (1 << 64) - 1, 0)), Status::OK));
        assert_eq!(csqrt(p, q, Rounding::Down), ((one, v(false, (1 << 64) - 1, 0)), Status::OK));
        assert_eq!(clog10(six, eight, Rounding::Down).0 .0, one);
        assert_eq!(cpow(one, one, two_, z, Rounding::Up).0, (z, two_));
        // (−1)^(2^62 + 1) and i^(2^62 + 2): the angle reduced exactly beyond 2^56.
        let big = v(false, (1 << 62) + 1, 0);
        assert_eq!(cpow(v(true, 1, 0), z, big, z, N).0 .0, v(true, 1, 0));
        let ((re, im), _) = cpow(z, one, v(false, (1 << 62) + 2, 0), z, N);
        assert_eq!((re, im), (v(true, 1, 0), z));
        // e^(i·2^16000): the real part's sign from Payne–Hanek, the parts below 1.
        let ((re, im), st) = cexp(z, v(false, 1, 16000), N);
        assert!(re != z && im != z && st != Status::OK);
    }

    #[test]
    fn x87_parts_above_the_tiny_cutoff_are_not_expanded() {
        use crate::float::F80;
        let v = |neg: bool, m: u128, e: i64| mp::round_exact::<F80>(Mp::from_u128(neg, m, e), N).0;
        // e^(2^-61 (1 + i)): both parts lie between 2^-150 and 2^-60, so a cut-off at 2^-60 would
        // take the leading-order expansion and give the real part 1. The true parts are
        // 1 + 2^-61 - 2^-183/3 + ... and 2^-61 + 2^-122 - ..., whose nearest x87 values are
        // 1 + 2^-61 and 2^-61 + 2^-122 (62 significant bits each).
        let t = v(false, 1, -61);
        let ((re, im), _) = cexp(t, t, N);
        assert_eq!(re, v(false, (1 << 61) + 1, -61));
        assert_eq!(im, v(false, (1 << 61) + 1, -122));
    }

    #[test]
    fn x87_trig_and_inverse_regimes() {
        use crate::float::F80;
        let v = |neg: bool, m: u128, e: i64| mp::round_exact::<F80>(Mp::from_u128(neg, m, e), N).0;
        let m = |f: F80| {
            let n = float::decode(f);
            Mp::from_u128(n.neg, n.sig, i64::from(n.exp))
        };
        let p2 = |e: i64| Mp::from_i64(1).ldexp(e);
        let one = v(false, 1, 0);
        // casin(1 + i 2^-40): |y| is above the tiny cut-off, so the part is not the expansion
        // √|y| = 2^-20 (exact) but √|y| (1 + |y|/12 + ...), about 2^19.4 last places above it.
        let ((_, im), _) = casin(one, v(false, 1, -40), N);
        let d = m(im).sub(p2(-20));
        assert!(!d.neg && !d.is_zero() && d.cmp_abs(p2(-60)).is_lt(), "casinl(1 + i 2^-40) = {:?}", m(im));
        // ctan(1 + 40i): the real part sin 2 · 2e^-80 (about 2^-114.6, between the far-out
        // cut-off 2^39 and the formats' range), the imaginary part 1 + δ with δ far below a last
        // place.
        let ((re, im), _) = ctan(one, v(false, 5, 3), N);
        assert!(!m(re).neg && m(re).cmp_abs(p2(-116)).is_gt() && m(re).cmp_abs(p2(-113)).is_lt(), "re {:?}", m(re));
        assert_eq!(im, one);
    }

    #[test]
    fn values_near_known_points() {
        let close = |a: f64, b: f64| (a - b).abs() <= 1e-15 * b.abs().max(1e-300);
        let ((re, im), _) = cexp(1.0f64, 1.0, N);
        assert!(close(re, 1.468_693_939_915_885_2) && close(im, 2.287_355_287_178_842_3));
        let ((re, im), _) = clog(1.0f64, 1.0, N);
        assert!(close(re, 0.346_573_590_279_972_65) && close(im, core::f64::consts::FRAC_PI_4));
        let ((re, im), _) = casin(1.0f64, 1.0, N);
        assert!(close(re, 0.666_239_432_492_515_2) && close(im, 1.061_275_061_905_035_7));
        let ((re, im), _) = catan(1.0f64, 1.0, N);
        assert!(close(re, 1.017_221_967_897_851_4) && close(im, 0.402_359_478_108_525_1));
        let ((re, im), _) = ctan(1.0f64, 1.0, N);
        assert!(close(re, 0.271_752_585_319_511_7) && close(im, 1.083_923_327_338_694_4));
    }
}
