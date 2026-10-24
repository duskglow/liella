//! Conversions for the math exports (spec §10, D-10.2): C values to the formats of
//! `liella-math`, the rounding direction the calling thread has set, and results back to C,
//! raising the status's exception flags (`liella_rt::fpu::raise`) and setting `errno` from its
//! error. This is the only place math touches either.
//!
//! `long double` and `_Float128` arrive as bit structs (`LdBits`, `F128Bits`) from the C
//! forwarders (OD-C4); out-parameters arrive as `Option<&mut T>`, and a NULL one where the C
//! interface has no error channel is a diagnostic plus abort (S6).

use liella_abi::ctypes::{c_int, F128Bits, LdBits};
use liella_abi::layout::{femode_t, fenv_t, fexcept_t};
use liella_kernel::machine::X87Env;
use liella_kernel::errno::Errno;
use liella_math::float::{self, Float};
use liella_math::{MathError, Rounding, Status, F128, F80};
use liella_rt::fpu;

use crate::conv;
use crate::errno;
use crate::ret;
use crate::sys::CSys;

/// A value of `liella-math` and its C representation.
pub trait CVal: Sized {
    /// The C type.
    type C;
    /// To C.
    fn c(self) -> Self::C;
    /// From C.
    fn of(c: &Self::C) -> Self;
}

impl CVal for f32 {
    type C = f32;
    fn c(self) -> f32 {
        self
    }
    fn of(c: &f32) -> f32 {
        *c
    }
}

impl CVal for f64 {
    type C = f64;
    fn c(self) -> f64 {
        self
    }
    fn of(c: &f64) -> f64 {
        *c
    }
}

impl CVal for F80 {
    type C = LdBits;
    fn c(self) -> LdBits {
        LdBits::from_bits(self.0)
    }
    fn of(c: &LdBits) -> F80 {
        F80(c.to_bits())
    }
}

impl CVal for F128 {
    type C = F128Bits;
    fn c(self) -> F128Bits {
        F128Bits::from_bits(self.0)
    }
    fn of(c: &F128Bits) -> F128 {
        F128(c.to_bits())
    }
}

impl CVal for i32 {
    type C = c_int;
    fn c(self) -> c_int {
        self
    }
    fn of(c: &c_int) -> i32 {
        *c
    }
}

impl CVal for i64 {
    type C = i64;
    fn c(self) -> i64 {
        self
    }
    fn of(c: &i64) -> i64 {
        *c
    }
}

impl CVal for u64 {
    type C = u64;
    fn c(self) -> u64 {
        self
    }
    fn of(c: &u64) -> u64 {
        *c
    }
}

/// A truth value as C's `int` (1 or 0).
impl CVal for bool {
    type C = c_int;
    fn c(self) -> c_int {
        c_int::from(self)
    }
    fn of(c: &c_int) -> bool {
        *c != 0
    }
}

/// A `long double` operand.
pub fn f80(x: LdBits) -> F80 {
    F80::of(&x)
}

/// A `_Float128` operand.
pub fn f128(x: F128Bits) -> F128 {
    F128::of(&x)
}

/// The rounding direction of SSE arithmetic (`float`, `double`, `_Float128`).
pub fn mode() -> Rounding {
    fpu::rounding(&CSys)
}

/// The rounding direction of x87 arithmetic (`long double`).
pub fn mode87() -> Rounding {
    fpu::x87_rounding(&CSys)
}

/// The unit whose rounding direction an envelope function ([`elem`] and the forms below) follows:
/// SSE for `float`, `double` and `_Float128` ([`mode`]), x87 for `long double` ([`mode87`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unit {
    /// MXCSR's rounding direction, taken from the one MXCSR read on entry.
    Sse,
    /// The x87 control word's rounding direction (one `fnstcw`).
    X87,
}

/// The caller's MXCSR and whether the computation runs in place ([`fpu::enter`]).
#[derive(Clone, Copy)]
struct Entry {
    saved: u32,
    in_place: bool,
}

/// Enter an envelope function (D-10.13, M6-1): read MXCSR once; switch to the default SSE
/// environment only when the caller's control word is not already the default one. The caller's
/// rounding direction comes from that read (SSE) or from the x87 control word.
#[inline]
fn enter(unit: Unit) -> (Entry, Rounding) {
    let (saved, in_place) = fpu::enter(&CSys);
    let mode = match unit {
        Unit::Sse => fpu::rounding_of(saved),
        Unit::X87 => mode87(),
    };
    (Entry { saved, in_place }, mode)
}

/// A value as the caller's denormals-are-zero reads it and flush-to-zero leaves it (owner ruling
/// 2026-10-23, D-10.22): the caller's floating-point environment is honoured. Only `float` and
/// `double` are SSE formats; every other format (x87 `long double`, the software `_Float128`)
/// and the integers keep these identities. The test is on the bits, so the sound internal
/// arithmetic (D-10.13) never runs under the caller's FTZ/DAZ.
pub trait Denorm: Copy {
    /// The operand as DAZ reads it: a subnormal is a zero of its sign, raising nothing.
    #[inline]
    fn daz(self) -> Self {
        self
    }
    /// The result as FTZ leaves it, and whether it was flushed: a subnormal is a zero of its sign.
    /// `tiny` is the computation's underflow report (tininess after rounding with an unbounded
    /// exponent, as x86 detects it): a result that the subnormal grid rounded up to the smallest
    /// normal while the value was tiny is flushed too (probed: `mulss`/`mulsd` of (1-2^-24)*2^-126
    /// under MXCSR FTZ give +0 with underflow and inexact in every rounding direction, where the
    /// default word gives the smallest normal).
    #[inline]
    fn ftz(self, _tiny: bool) -> (Self, bool) {
        (self, false)
    }
}

impl Denorm for f64 {
    #[inline]
    fn daz(self) -> f64 {
        self.ftz(false).0
    }
    #[inline]
    fn ftz(self, tiny: bool) -> (f64, bool) {
        if self.is_subnormal() || (tiny && self.abs() == f64::MIN_POSITIVE) {
            (f64::from_bits(self.to_bits() & (1 << 63)), true)
        } else {
            (self, false)
        }
    }
}

impl Denorm for f32 {
    #[inline]
    fn daz(self) -> f32 {
        self.ftz(false).0
    }
    #[inline]
    fn ftz(self, tiny: bool) -> (f32, bool) {
        if self.is_subnormal() || (tiny && self.abs() == f32::MIN_POSITIVE) {
            (f32::from_bits(self.to_bits() & (1 << 31)), true)
        } else {
            (self, false)
        }
    }
}

impl Denorm for F80 {}
impl Denorm for F128 {}
impl Denorm for LdBits {}
impl Denorm for F128Bits {}
impl Denorm for i32 {}
impl Denorm for i64 {}
impl Denorm for u64 {}
impl Denorm for bool {}

impl<A: Denorm, B: Denorm> Denorm for (A, B) {
    #[inline]
    fn daz(self) -> (A, B) {
        (self.0.daz(), self.1.daz())
    }
    #[inline]
    fn ftz(self, tiny: bool) -> ((A, B), bool) {
        let ((a, fa), (b, fb)) = (self.0.ftz(tiny), self.1.ftz(tiny));
        ((a, b), fa || fb)
    }
}

impl<T: Denorm> Denorm for Complex<T> {
    #[inline]
    fn daz(self) -> Complex<T> {
        Complex { re: self.re.daz(), im: self.im.daz() }
    }
    #[inline]
    fn ftz(self, tiny: bool) -> (Complex<T>, bool) {
        let ((re, fr), (im, fi)) = (self.re.ftz(tiny), self.im.ftz(tiny));
        (Complex { re, im }, fr || fi)
    }
}

impl Entry {
    /// The operand as the caller's DAZ reads it. In place the caller's word is the default one
    /// (no DAZ), so the hot path tests nothing but `in_place`.
    #[inline]
    fn operand<A: Denorm>(&self, x: A) -> A {
        if !self.in_place && fpu::is_daz(self.saved) { x.daz() } else { x }
    }

    /// The result as the caller's FTZ leaves it: a subnormal result is a zero of its sign with
    /// underflow and inexact raised, as the SSE unit does under FTZ (Intel SDM). `errno` stays
    /// what the computation reported.
    #[inline]
    fn result<R: Denorm>(&self, v: R, st: &mut Status) -> R {
        if !self.in_place && fpu::is_ftz(self.saved) {
            // The status carries the union of the parts' flags: a pair or complex result whose
            // other part underflowed and which holds exactly the smallest normal is flushed
            // too (an edge of an edge; the parts are not told apart).
            let (v, flushed) = v.ftz(st.flags.any(fpu::Flags::UNDERFLOW));
            if flushed {
                st.flags = st.flags.union(fpu::Flags::UNDERFLOW).union(fpu::Flags::INEXACT);
            }
            v
        } else {
            v
        }
    }
}

/// Leave an envelope function: restore the caller's MXCSR with the result's flags raised, then
/// store its error. In place this is one MXCSR read and a write only when it differs
/// ([`fpu::settle`]); after a switch, the restore and [`report`] as before.
#[inline]
fn exit(e: Entry, st: Status) {
    if e.in_place {
        fpu::settle(&CSys, e.saved, st.flags, st.hw);
        store_error(st.error);
    } else {
        fpu::leave(&CSys, e.saved);
        report(st);
    }
}

/// Raise the status's flags and store its error.
fn report(st: Status) {
    fpu::raise(&CSys, st.flags);
    store_error(st.error);
}

/// Store a status's error in `errno` (nothing for none).
fn store_error(e: MathError) {
    match e {
        MathError::None => {}
        MathError::Domain => errno::set(Errno::EDOM),
        MathError::Range => errno::set(Errno::ERANGE),
    }
}

/// A result with its status.
pub fn ret<T: CVal>(r: (T, Status)) -> T::C {
    report(r.1);
    r.0.c()
}

/// A format whose square root has a hardware form ([`sqrt`], M6-2).
pub trait Sqrt: CVal {
    /// The square root, correctly rounded in the caller's direction, with its flags and `errno`.
    fn sqrt_c(self) -> Self::C;
}

impl Sqrt for f64 {
    #[inline]
    fn sqrt_c(self) -> f64 {
        let mx = crate::ffi::liella_machine_mxcsr_get();
        if fpu::is_sqrt_in_place(mx) {
            let r = crate::ffi::liella_machine_sqrtsd(self);
            // EDOM exactly when a non-NaN operand gave NaN, that is for x < 0 as the caller's
            // DAZ reads x (D-10.22: -0, a negative subnormal under DAZ and negative NaNs give none).
            if r.is_nan() && !self.is_nan() {
                store_error(MathError::Domain);
            }
            r
        } else {
            let x = if fpu::is_daz(mx) { self.daz() } else { self };
            ret(liella_math::fused::sqrt::<f64, f64>(x, fpu::rounding_of(mx)))
        }
    }
}

impl Sqrt for f32 {
    #[inline]
    fn sqrt_c(self) -> f32 {
        let mx = crate::ffi::liella_machine_mxcsr_get();
        if fpu::is_sqrt_in_place(mx) {
            let r = crate::ffi::liella_machine_sqrtss(self);
            // EDOM exactly when a non-NaN operand gave NaN, that is for x < 0 as the caller's
            // DAZ reads x (D-10.22: -0, a negative subnormal under DAZ and negative NaNs give none).
            if r.is_nan() && !self.is_nan() {
                store_error(MathError::Domain);
            }
            r
        } else {
            let x = if fpu::is_daz(mx) { self.daz() } else { self };
            ret(liella_math::fused::sqrt::<f32, f32>(x, fpu::rounding_of(mx)))
        }
    }
}

impl Sqrt for F80 {
    #[inline]
    fn sqrt_c(self) -> LdBits {
        let x = self.c();
        let e = x.se & 0x7fff;
        // The canonical encodings: exponent 0 with the explicit bit clear (zeros, subnormals),
        // any other exponent with it set. Pseudo-denormals and the invalid encodings keep the
        // software path's rules (D-10.20).
        let canonical = (e == 0) == (x.mant >> 63 == 0);
        if canonical && u16::try_from(crate::ffi::liella_machine_x87_cw_get()).is_ok_and(fpu::is_x87_default_but_rounding) {
            let mut r = LdBits::default();
            crate::ffi::liella_machine_fsqrt(&x, &mut r);
            let nan = e == 0x7fff && x.mant << 1 != 0;
            let zero = e == 0 && x.mant == 0;
            if x.se & 0x8000 != 0 && !nan && !zero {
                store_error(MathError::Domain);
            }
            r
        } else {
            ret(liella_math::fused::sqrt::<F80, F80>(self, mode87()))
        }
    }
}

impl Sqrt for F128 {
    #[inline]
    fn sqrt_c(self) -> F128Bits {
        ret(liella_math::fused::sqrt::<F128, F128>(self, mode()))
    }
}

/// `sqrt`, `sqrtf`, `sqrtl` and their `fN`/`fNx` names (M6-2): `sqrtss`, `sqrtsd` and x87 `fsqrt`
/// are correctly rounded in the unit's current direction by definition (IEEE 754 squareRoot,
/// Intel SDM), so they run in place on the caller's control word and raise their flags there:
/// inexact; invalid for a signalling NaN or an operand below −0, with x86's default NaN; never
/// overflow or underflow (a non-zero root is at least 2^-75 in `float`, 2^-538 in `double`,
/// 2^-8223 in `long double`, all normal), so flush-to-zero could not act either. `errno` EDOM
/// for an operand below −0 that is not a NaN; NaN operands as the hardware propagates them
/// (D-10.14). A caller with denormals-are-zero, flush-to-zero, an unmasked exception or x87
/// precision below 64 bits gets the software path (`fused::sqrt`) as before, and so does
/// `_Float128`. The hardware also sets the denormal-operand bit for a subnormal operand, as
/// glibc's `sqrt` does; it is outside `FE_ALL_EXCEPT`.
pub fn sqrt<T: Sqrt>(x: T) -> T::C {
    x.sqrt_c()
}

/// A format whose `hypot` has a fast form ([`hypot`], #1541).
pub trait Hypot: CVal {
    /// `hypot(self, y)`, correctly rounded in the caller's direction, with its flags and `errno`.
    fn hypot_c(self, y: Self) -> Self::C;
}

impl Hypot for f32 {
    #[inline]
    fn hypot_c(self, y: f32) -> f32 {
        // One MXCSR read serves both paths (the envelope fast entry's pattern, MATH-M6-ENVELOPE-COST):
        // a refusal takes its rounding direction from the value read on entry.
        let mxcsr = crate::ffi::liella_machine_mxcsr_get();
        if fpu::is_default_but_rounding(mxcsr)
            && let Some(r) = hypotf_fast(self, y)
        {
            return r;
        }
        ret(liella_math::pow::hypot(self, y, fpu::rounding_of(mxcsr)))
    }
}

impl Hypot for f64 {
    #[inline]
    fn hypot_c(self, y: f64) -> f64 {
        ret(liella_math::pow::hypot(self, y, mode()))
    }
}

impl Hypot for F80 {
    #[inline]
    fn hypot_c(self, y: F80) -> LdBits {
        ret(liella_math::pow::hypot(self, y, mode87()))
    }
}

impl Hypot for F128 {
    #[inline]
    fn hypot_c(self, y: F128) -> F128Bits {
        ret(liella_math::pow::hypot(self, y, mode()))
    }
}

/// The `float` hypot on the hardware square root, or `None` when the rounding is not decided.
/// Run only on a default MXCSR in any direction (no DAZ/FTZ, every exception masked).
///
/// `x` and `y` widen exactly to `double`; their squares are exact there (24-bit significands
/// give 48-bit products, and every square of a `float` is a normal `double` between 2^-298 and
/// 2^256, so no scaling is needed). The sum and the root (`sqrtsd`) are each rounded once in
/// the caller's direction, so the double root `r` is within a few `double` ulps of the true
/// root `t`. Every `float` f and every midpoint m between two floats squares exactly in
/// `double` (at most 50 significant bits), so the rounding is monotone: t < v gives r <= v and
/// t > v gives r >= v for v = f or m. Directed modes round the sum and the root the same way
/// (r <= t downward and toward zero, r >= t upward), so r never crosses a float the true root
/// does not, and the final narrowing (`cvtsd2ss`, in the caller's direction) gives t's
/// rounding. In nearest the one failure is r landing exactly on a midpoint while t does not
/// (ties-to-even may then pick the wrong float; pinned in
/// `math_hypotf_fast_path_is_the_accurate_hypotf`), and such an r has the 29 bits below
/// `float` precision equal to 2^28: exactly those roots go to the accurate path, in every
/// direction. The flags are the hardware's: an exact root (r a float with t = r) raises
/// nothing, any other raises inexact in the sum, the root or the narrowing, and nothing else
/// (a signalling NaN raises invalid here and again on the accurate path). A root below
/// `FLT_MIN` or above `FLT_MAX`, a NaN or an infinity also go to the accurate path, so the fast
/// result never overflows, underflows or sets `errno`.
#[doc(hidden)]
#[inline]
pub fn hypotf_fast(x: f32, y: f32) -> Option<f32> {
    let (a, b) = (f64::from(x), f64::from(y));
    let r = crate::ffi::liella_machine_sqrtsd(a * a + b * b);
    if !(r >= f64::from(f32::MIN_POSITIVE) && r <= f64::from(f32::MAX)) {
        return None;
    }
    if r.to_bits() & ((1 << 29) - 1) == 1 << 28 {
        return None;
    }
    #[allow(clippy::as_conversions, clippy::cast_possible_truncation)] // the narrowing (cvtsd2ss) is the rounding step
    Some(r as f32)
}

/// `hypot` and its `fN`/`fNx` names (#1541): `hypotf` tries [`hypotf_fast`] on a default MXCSR
/// in any direction and falls back to the correctly rounded `pow::hypot` when the rounding is
/// undecided or the operands are special; a caller with DAZ, FTZ or an unmasked exception, and
/// every other format, take `pow::hypot` as before.
pub fn hypot<T: Hypot>(x: T, y: T) -> T::C {
    x.hypot_c(y)
}

/// A result that raises nothing.
pub fn pure<T: CVal>(v: T) -> T::C {
    v.c()
}

/// The value behind a required `const T *` operand.
pub fn get<T: CVal>(p: Option<&T::C>) -> T {
    T::of(ret::must(conv::req(p)))
}

/// A result with a second result stored through a required out-parameter (`frexp`, `modf`).
pub fn ret_out<A: CVal, B: CVal>(r: ((A, B), Status), out: Option<&mut B::C>) -> A::C {
    let ((a, b), st) = r;
    *ret::must(conv::req(out)) = b.c();
    ret((a, st))
}

/// `remquo`: the quotient bits are stored when the function defines them.
pub fn remquo<T: CVal>(r: ((T, Option<i32>), Status), quo: Option<&mut c_int>) -> T::C {
    let ((v, q), st) = r;
    let dst = ret::must(conv::req(quo));
    if let Some(q) = q {
        *dst = q;
    }
    ret((v, st))
}

/// `setpayload`/`setpayloadsig`: the NaN stored and 0, or +0 stored and 1.
pub fn payload<T: CVal + Float>(r: Option<T>, out: Option<&mut T::C>) -> c_int {
    let dst = ret::must(conv::req(out));
    match r {
        Some(v) => {
            *dst = v.c();
            0
        }
        None => {
            *dst = float::zero::<T>(false).c();
            1
        }
    }
}

/// `canonicalize`: the canonical encoding stored and 0, or nothing stored and 1.
pub fn canonical<T: CVal>(r: Option<(T, Status)>, out: Option<&mut T::C>) -> c_int {
    let dst = ret::must(conv::req(out));
    match r {
        Some((v, st)) => {
            *dst = ret((v, st));
            0
        }
        None => 1,
    }
}

/// An `intmax_t` result (`fromfp`, `fromfpx`).
pub fn intmax(r: (i128, Status)) -> i64 {
    report(r.1);
    i64::try_from(r.0).unwrap_or(i64::MAX)
}

/// A `uintmax_t` result (`ufromfp`, `ufromfpx`).
pub fn uintmax(r: (i128, Status)) -> u64 {
    report(r.1);
    u64::try_from(r.0).unwrap_or(u64::MAX)
}

/// A C function that cannot fail: run it, then 0.
pub fn done(f: impl FnOnce()) -> c_int {
    f();
    0
}

/// `fesetround`: 0, or 1 for a value that names no rounding direction (nothing changes).
pub fn set_round(r: Option<Rounding>) -> c_int {
    match r {
        Some(r) => {
            fpu::set_rounding(&CSys, r);
            0
        }
        None => 1,
    }
}

/// The flags saved in a `fexcept_t` (required).
pub fn fexcept(p: Option<&fexcept_t>) -> fpu::Flags {
    fpu::Flags::from_c(i32::from(ret::must(conv::req(p)).0))
}

/// `fegetexceptflag`: store the flags; 0.
pub fn store_fexcept(p: Option<&mut fexcept_t>, f: fpu::Flags) -> c_int {
    *ret::must(conv::req(p)) = fexcept_t(u16::from(f.0));
    0
}

/// The `fenv_t` layout (spec Appendix C) of an environment: the x87 environment as `fnstenv`
/// stores it, then MXCSR.
fn fenv_of(e: &fpu::Env) -> fenv_t {
    let lo = |w: u32| u16::try_from(w & 0xffff).unwrap_or(0);
    let hi = |w: u32| u16::try_from(w >> 16).unwrap_or(0);
    let [eip, sel_op, data, data_sel] = e.x87.rest;
    fenv_t {
        __control_word: lo(e.x87.control),
        __glibc_reserved1: hi(e.x87.control),
        __status_word: lo(e.x87.status),
        __glibc_reserved2: hi(e.x87.status),
        __tags: lo(e.x87.tag),
        __glibc_reserved3: hi(e.x87.tag),
        __eip: eip,
        __cs_selector: lo(sel_op),
        __opcode: hi(sel_op).to_le_bytes(),
        __data_offset: data,
        __data_selector: lo(data_sel),
        __glibc_reserved5: hi(data_sel),
        __mxcsr: e.mxcsr,
    }
}

fn env_of(f: &fenv_t) -> fpu::Env {
    let w = |lo: u16, hi: u16| u32::from(lo) | (u32::from(hi) << 16);
    fpu::Env {
        x87: X87Env {
            control: w(f.__control_word, f.__glibc_reserved1),
            status: w(f.__status_word, f.__glibc_reserved2),
            tag: w(f.__tags, f.__glibc_reserved3),
            rest: [f.__eip, w(f.__cs_selector, u16::from_le_bytes(f.__opcode)), f.__data_offset, w(f.__data_selector, f.__glibc_reserved5)],
        },
        mxcsr: f.__mxcsr,
    }
}

/// `fegetenv`/`feholdexcept`: store the environment; 0.
pub fn store_env(p: Option<&mut fenv_t>, e: fpu::Env) -> c_int {
    *ret::must(conv::req(p)) = fenv_of(&e);
    0
}

/// The environment a `const fenv_t *` names (NULL: S6).
pub(crate) fn env(a: conv::FeArg<'_, fenv_t>) -> fpu::Env {
    match a {
        conv::FeArg::Object(f) => env_of(f),
        conv::FeArg::Default => fpu::Env::DEFAULT,
        conv::FeArg::NoMask => fpu::Env::NOMASK,
        conv::FeArg::Null => ret::fatal(Errno::EFAULT),
    }
}

/// `fegetmode`: store the control modes; 0.
pub fn store_mode(p: Option<&mut femode_t>, m: fpu::Mode) -> c_int {
    *ret::must(conv::req(p)) = femode_t { __control_word: m.control, __glibc_reserved: 0, __mxcsr: m.mxcsr };
    0
}

/// The control modes a `const femode_t *` names (`FE_DFL_MODE` is the address −1; NULL: S6).
pub(crate) fn modes(a: conv::FeArg<'_, femode_t>) -> fpu::Mode {
    match a {
        conv::FeArg::Object(m) => fpu::Mode { control: m.__control_word, mxcsr: m.__mxcsr },
        conv::FeArg::Default | conv::FeArg::NoMask => fpu::Mode::DEFAULT,
        conv::FeArg::Null => ret::fatal(Errno::EFAULT),
    }
}

/// The payload `nan(tag)` derives from its tag: the tag read as `strtoull` reads it (saturating),
/// if the whole tag is a number; otherwise 0 (spec §10.2, `strtod("NAN(tag)")`).
pub fn nan_tag(tag: &[u8]) -> u128 {
    use liella::num::parse::{parse_u64, Dialect, Status as P};
    if !tag.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_') {
        return 0;
    }
    let p = parse_u64(tag, 0, Dialect::C99);
    match p.parsed.status {
        P::Ok | P::Overflow if p.parsed.used == tag.len() && !p.negated => u128::from(p.parsed.value),
        _ => 0,
    }
}

/// Run an elementary function (D-10.13): read the caller's MXCSR once, take the rounding
/// direction of `unit` (`f` gets it), and compute in the default SSE environment (round to
/// nearest, exceptions masked, no flush-to-zero), which the double-double arithmetic assumes.
/// When the caller's control word is already the default one the computation runs in place;
/// otherwise MXCSR is switched and restored around it (M6-1). Either way the caller's MXCSR is
/// left with only the result's own flags added. The optimisation barriers keep the computation
/// between entry and exit.
pub fn elem<A: Denorm, R: CVal + Denorm>(x: A, unit: Unit, f: impl FnOnce(A, Rounding) -> (R, Status)) -> R::C {
    let (e, mode) = enter(unit);
    let x = core::hint::black_box(e.operand(x));
    let (v, mut st) = core::hint::black_box(f(x, mode));
    let v = e.result(v, &mut st);
    exit(e, st);
    v.c()
}

/// An envelope `float` function with a round-to-nearest fast path (MATH-M6-ENVELOPE-COST
/// prototype): one MXCSR read; when the caller's control word is the default one (round to
/// nearest, every trap masked, no FTZ/DAZ), `fast` runs in place and its answer is returned with
/// nothing else to do. `fast` must answer only where its hardware arithmetic raises exactly the
/// inexact flag and nothing else (the result is inexact by its rounding test), so MXCSR already
/// holds what the caller must see and no write, no flag merge and no errno are needed. Any other
/// mode goes to [`elem`]; a refusal in the default mode runs `f` in place against the MXCSR read
/// on entry (the attempt may have raised inexact or denormal-operand in hardware: both are in the
/// `hw` passed to [`fpu::settle`], so the exit write drops them unless the result raises them).
///
/// `HW` is the set of MXCSR flag bits `f` may raise in hardware in round to nearest, fixed per
/// function at compile time by `cargo xtask math-gen` from the function's row in s10_math.tsv
/// (#1395, #1407): the accurate path does not carry or merge the runtime [`Status::hw`], so the
/// exit test compares against a constant. It must be a superset of every `hw` `f` can report
/// (checked in debug builds); today `f` narrows nothing on this path, so every row gives
/// [`Status::HW_ANY`].
#[inline(always)]
pub fn elem32_fast<const HW: u8>(
    x: f32,
    fast: impl FnOnce(f32) -> Option<f32>,
    f: impl FnOnce(f32, Rounding) -> (f32, Status),
) -> f32 {
    elem_fast::<HW, f32, f32>(x, fast, f)
}

/// [`elem32_fast`] for any argument and a `float` or `double` result: the `double` exp, exp2,
/// log, log2, log10 and pow entries use it with the `*_nearest` functions of
/// `liella_math::fast64` (MATH-M6-F64-FAST-ENTRY), under the same contract (including `HW`).
#[inline(always)]
pub fn elem_fast<const HW: u8, A: Copy, R: CVal<C = R>>(
    x: A,
    fast: impl FnOnce(A) -> Option<R>,
    f: impl FnOnce(A, Rounding) -> (R, Status),
) -> R {
    let (saved, in_place) = fpu::peek(&CSys);
    if in_place {
        // The barrier keeps the fast path's arithmetic after the read and behind the branch: a
        // caller with an unmasked trap must never see it run before the switch.
        let x = core::hint::black_box(x);
        if let Some(v) = fast(x) {
            return v;
        }
        let (v, st) = core::hint::black_box(f(x, Rounding::Nearest));
        debug_assert!(st.hw.0 & !HW == 0, "the compile-time hw set misses a bit the computation reports");
        fpu::settle(&CSys, saved, st.flags, fpu::Flags(HW | FAST_ATTEMPT_HW.0));
        store_error(st.error);
        return v;
    }
    elem(x, Unit::Sse, f)
}

/// What a refused fast attempt may have raised in hardware ([`elem32_fast`]).
const FAST_ATTEMPT_HW: fpu::Flags = fpu::Flags(fpu::Flags::INEXACT.0 | Status::HW_DENORMAL.0);

/// `lgamma` and `gamma`: [`elem`], with the sign of `Γ(x)` stored in `signgam` (D-10.4).
pub fn lgamma<A: Denorm, R: CVal + Denorm>(x: A, unit: Unit, f: impl FnOnce(A, Rounding) -> ((R, Status), i32)) -> R::C {
    let (e, mode) = enter(unit);
    let x = core::hint::black_box(e.operand(x));
    let ((v, mut st), sign) = core::hint::black_box(f(x, mode));
    let v = e.result(v, &mut st);
    crate::data::signgam.store(sign, core::sync::atomic::Ordering::Relaxed);
    crate::data::__signgam.store(sign, core::sync::atomic::Ordering::Relaxed);
    exit(e, st);
    v.c()
}

/// `lgamma_r`: [`elem`], with the sign of `Γ(x)` stored through the required out-parameter
/// (`signgam` untouched).
pub fn lgamma_r<A: Denorm, R: CVal + Denorm>(x: A, unit: Unit, f: impl FnOnce(A, Rounding) -> ((R, Status), i32), out: Option<&mut c_int>) -> R::C {
    let dst = ret::must(conv::req(out));
    let (e, mode) = enter(unit);
    let x = core::hint::black_box(e.operand(x));
    let ((v, mut st), sign) = core::hint::black_box(f(x, mode));
    let v = e.result(v, &mut st);
    *dst = sign;
    exit(e, st);
    v.c()
}

/// A complex value at the C boundary: `float _Complex` and `double _Complex` are two consecutive
/// parts, passed and returned in SSE registers like this `repr(C)` pair (SysV x86-64 ABI §3.2.3).
/// `long double _Complex` and `_Float128 _Complex` arrive from their C forwarders as a pair of
/// bit structs (`Complex<LdBits>`, `Complex<F128Bits>`, in memory both ways, D-10.21).
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Complex<T> {
    /// The real part.
    pub re: T,
    /// The imaginary part.
    pub im: T,
}

/// A part of a [`Complex`] at the C boundary and the format `liella-math` computes it in.
pub trait CPart: Copy + Denorm {
    /// The format.
    type V: Copy + Denorm;
    /// From C.
    fn v(self) -> Self::V;
    /// To C.
    fn of_v(v: Self::V) -> Self;
    /// The invalid-encoding rule for a complex result (ruling #902, D-10.20): with an invalid
    /// encoding among the operand parts, every NaN part of the result is the default NaN and
    /// invalid is raised (returns whether it applied). Only the x87 format has such encodings;
    /// the other formats keep this default and pay nothing.
    fn invalid_rule(_ops: &[Self], _out: &mut Complex<Self>) -> bool {
        false
    }
}

/// An x87 encoding IEEE does not define: a nonzero exponent field with the explicit leading
/// bit clear (pseudo-NaN, pseudo-infinity, unnormal).
fn ld_invalid(p: &LdBits) -> bool {
    p.se & 0x7fff != 0 && p.mant >> 63 == 0
}

/// A NaN, or an encoding that is not a number at all.
fn ld_not_number(p: &LdBits) -> bool {
    ld_invalid(p) || (p.se & 0x7fff == 0x7fff && p.mant != 1 << 63)
}

impl CPart for f32 {
    type V = f32;
    fn v(self) -> f32 {
        self
    }
    fn of_v(v: f32) -> f32 {
        v
    }
}

impl CPart for f64 {
    type V = f64;
    fn v(self) -> f64 {
        self
    }
    fn of_v(v: f64) -> f64 {
        v
    }
}

impl CPart for LdBits {
    type V = F80;
    fn v(self) -> F80 {
        F80::of(&self)
    }
    fn of_v(v: F80) -> LdBits {
        v.c()
    }
    fn invalid_rule(ops: &[LdBits], out: &mut Complex<LdBits>) -> bool {
        if !ops.iter().any(ld_invalid) {
            return false;
        }
        let dn = LdBits { mant: 0xc000_0000_0000_0000, se: 0xffff, pad: [0; 3] };
        for p in [&mut out.re, &mut out.im] {
            if ld_not_number(p) {
                *p = dn;
            }
        }
        true
    }
}

impl CPart for F128Bits {
    type V = F128;
    fn v(self) -> F128 {
        F128::of(&self)
    }
    fn of_v(v: F128) -> F128Bits {
        v.c()
    }
}

/// `creal`, `cimag`: one part, nothing raised and the environment untouched.
pub fn cpart<C: CPart>(z: Complex<C>, f: impl FnOnce(C::V, C::V) -> C::V) -> C {
    C::of_v(f(z.re.v(), z.im.v()))
}

/// A complex function of a complex argument (spec §10.5), in [`elem`]'s environment.
pub fn cplx<C: CPart>(z: Complex<C>, unit: Unit, f: impl FnOnce(C::V, C::V, Rounding) -> ((C::V, C::V), Status)) -> Complex<C> {
    let (e, mode) = enter(unit);
    let z = core::hint::black_box(e.operand(z));
    let ((re, im), mut st) = core::hint::black_box(f(z.re.v(), z.im.v(), mode));
    let (re, im) = e.result((re, im), &mut st);
    let mut r = Complex { re: C::of_v(re), im: C::of_v(im) };
    if C::invalid_rule(&[z.re, z.im], &mut r) {
        st.flags = st.flags.union(Status::INVALID.flags);
    }
    exit(e, st);
    r
}

/// `cpow`: a complex function of two complex arguments.
pub fn cplx2<C: CPart>(z: Complex<C>, w: Complex<C>, unit: Unit, f: impl FnOnce(C::V, C::V, C::V, C::V, Rounding) -> ((C::V, C::V), Status)) -> Complex<C> {
    let (e, mode) = enter(unit);
    let (z, w) = core::hint::black_box(e.operand((z, w)));
    let ((re, im), mut st) = core::hint::black_box(f(z.re.v(), z.im.v(), w.re.v(), w.im.v(), mode));
    let (re, im) = e.result((re, im), &mut st);
    let mut r = Complex { re: C::of_v(re), im: C::of_v(im) };
    if C::invalid_rule(&[z.re, z.im, w.re, w.im], &mut r) {
        st.flags = st.flags.union(Status::INVALID.flags);
    }
    exit(e, st);
    r
}

/// A real function of a complex argument (`cabs`, `carg`), in [`elem`]'s environment.
pub fn cplx_real<C: CPart, R: CVal + Denorm>(z: Complex<C>, unit: Unit, f: impl FnOnce(C::V, C::V, Rounding) -> (R, Status)) -> R::C {
    elem(z, unit, |z, m| f(z.re.v(), z.im.v(), m))
}

/// `sincos`: [`elem`] with the two results stored through required out-parameters.
pub fn sincos<A: CVal + Denorm>(x: A, unit: Unit, f: impl FnOnce(A, Rounding) -> ((A, A), Status), s: Option<&mut A::C>, c: Option<&mut A::C>) {
    let (s, c) = (ret::must(conv::req(s)), ret::must(conv::req(c)));
    let (e, mode) = enter(unit);
    let x = core::hint::black_box(e.operand(x));
    let ((sv, cv), mut st) = core::hint::black_box(f(x, mode));
    let (sv, cv) = e.result((sv, cv), &mut st);
    *s = sv.c();
    *c = cv.c();
    exit(e, st);
}

#[cfg(test)]
mod tests {
    use super::Denorm;

    #[test]
    fn ftz_flushes_the_tininess_edge() {
        // Clauses: s10_0_f_0.
        // Probed black-box on shiki with ldmxcsr and mulss/mulsd: (1-2^-24)*2^-126 and
        // (1-2^-53)*2^-1022 (ties on the subnormal grid, rounding to the smallest normal) give the
        // smallest normal with underflow+inexact under the default word, and a zero of the sign with
        // underflow+inexact under FTZ, in all four rounding directions: the value is tiny after
        // rounding with an unbounded exponent and FTZ flushes a tiny result. (1-2^-26)*2^-126,
        // which that rounding takes to 2^-126, is not tiny and stays. So with the computation's
        // underflow report (`tiny`, the same after-rounding rule) the smallest normal is flushed:
        assert_eq!(f64::MIN_POSITIVE.ftz(true), (0.0, true));
        assert_eq!((-f64::MIN_POSITIVE).ftz(true).0.to_bits(), 1 << 63);
        assert_eq!(f32::MIN_POSITIVE.ftz(true), (0.0, true));
        assert_eq!((-f32::MIN_POSITIVE).ftz(true).0.to_bits(), 1 << 31);
        // and not without it, nor any other normal, while a subnormal always is.
        assert_eq!(f64::MIN_POSITIVE.ftz(false), (f64::MIN_POSITIVE, false));
        assert_eq!(1.0f64.ftz(true), (1.0, false));
        assert_eq!(f32::MIN_POSITIVE.ftz(false), (f32::MIN_POSITIVE, false));
        assert_eq!(f64::from_bits(1).ftz(false), (0.0, true));
        assert_eq!(f32::from_bits(1).ftz(true), (0.0, true));
        assert_eq!(f64::from_bits(1).daz(), 0.0);
        assert_eq!(f64::MIN_POSITIVE.daz(), f64::MIN_POSITIVE);
    }
}
