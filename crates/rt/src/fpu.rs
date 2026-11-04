//! The floating-point environment (spec §10.6, D-10.5) over `Machine` MXCSR and x87 access.
//!
//! x86_64 has two floating-point units with separate state: SSE (MXCSR: flags, trap masks,
//! rounding) and x87 (control word: trap masks, rounding, precision; status word: flags). Every
//! operation here keeps them in step: a rounding mode or trap mask is written to both, the flags
//! are read from both (a flag raised by either unit counts) and cleared in both. The
//! environment belongs to the calling thread (the kernel saves both units per thread).
//!
//! Flags are raised without side effects: when the exception's trap is disabled the flag is set
//! in MXCSR directly, so raising `FE_OVERFLOW` does not also raise `FE_INEXACT` (as an
//! arithmetic overflow would). When the trap is enabled, an SSE operation that raises the
//! exception delivers it (`SIGFPE`), which a flag write cannot do.

#![allow(clippy::as_conversions, clippy::arithmetic_side_effects)] // register bit fields of fixed width

use core::hint::black_box;

use liella_kernel::machine::{Machine, X87Env};

/// The exception flags, with the x86 values of `FE_INVALID` ... `FE_INEXACT` (the same bit
/// positions in MXCSR, the x87 status word and the x87 control word's masks).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Flags(pub u8);

impl Flags {
    /// No flag.
    pub const NONE: Flags = Flags(0);
    /// `FE_INVALID`.
    pub const INVALID: Flags = Flags(0x01);
    /// `FE_DIVBYZERO`.
    pub const DIVBYZERO: Flags = Flags(0x04);
    /// `FE_OVERFLOW`.
    pub const OVERFLOW: Flags = Flags(0x08);
    /// `FE_UNDERFLOW`.
    pub const UNDERFLOW: Flags = Flags(0x10);
    /// `FE_INEXACT`.
    pub const INEXACT: Flags = Flags(0x20);
    /// `FE_ALL_EXCEPT`: the five IEEE flags (the x86 denormal-operand flag, 0x02, is not one).
    pub const ALL: Flags = Flags(0x3d);

    /// The flags among the bits of a C `int` (other bits are ignored, as glibc does).
    pub const fn from_c(v: i32) -> Flags {
        Flags((v & 0x3d) as u8)
    }

    /// The C value.
    pub const fn to_c(self) -> i32 {
        self.0 as i32
    }

    /// Both sets.
    #[must_use]
    pub const fn union(self, o: Flags) -> Flags {
        Flags(self.0 | o.0)
    }

    /// Flags in both sets.
    #[must_use]
    pub const fn and(self, o: Flags) -> Flags {
        Flags(self.0 & o.0)
    }

    /// True if any flag of `o` is set.
    pub const fn any(self, o: Flags) -> bool {
        self.0 & o.0 != 0
    }

    /// True if no flag is set.
    pub const fn is_empty(self) -> bool {
        self.0 == 0
    }
}

/// A rounding direction, with the x86 values of `FE_TONEAREST` ... `FE_TOWARDZERO` (the x87
/// control word's RC field; MXCSR's RC field holds the same two bits three positions higher).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Rounding {
    /// To nearest, ties to even (`FE_TONEAREST`, 0).
    #[default]
    Nearest,
    /// Toward −∞ (`FE_DOWNWARD`, 0x400).
    Down,
    /// Toward +∞ (`FE_UPWARD`, 0x800).
    Up,
    /// Toward zero (`FE_TOWARDZERO`, 0xc00).
    Zero,
}

impl Rounding {
    /// The mode named by a C value, or `None` for anything else.
    pub const fn from_c(v: i32) -> Option<Rounding> {
        match v {
            0 => Some(Rounding::Nearest),
            0x400 => Some(Rounding::Down),
            0x800 => Some(Rounding::Up),
            0xc00 => Some(Rounding::Zero),
            _ => None,
        }
    }

    /// The C value.
    pub const fn to_c(self) -> i32 {
        (self.field() << CW_RC_SHIFT) as i32
    }

    /// From the two-bit RC field (0 nearest, 1 down, 2 up, 3 toward zero).
    const fn from_field(rc: u32) -> Rounding {
        match rc & 3 {
            0 => Rounding::Nearest,
            1 => Rounding::Down,
            2 => Rounding::Up,
            _ => Rounding::Zero,
        }
    }

    const fn field(self) -> u32 {
        match self {
            Rounding::Nearest => 0,
            Rounding::Down => 1,
            Rounding::Up => 2,
            Rounding::Zero => 3,
        }
    }
}

/// MXCSR fields.
const MX_FLAGS: u32 = 0x3f;
const MX_MASK_SHIFT: u32 = 7;
const MX_RC_SHIFT: u32 = 13;
const MX_RC: u32 = 3 << MX_RC_SHIFT;
/// MXCSR after `FE_DFL_ENV`: every exception masked, round to nearest, no flags.
const MX_DEFAULT: u32 = 0x1f80;
/// x87 status word: the exception flags with the denormal flag, then the stack-fault and
/// error-summary bits, which must be clear once no exception is pending.
const SW_FLAGS: u32 = 0x3f;
const SW_SUMMARY: u32 = 0xc0;
/// x87 control word: the rounding field.
const CW_RC_SHIFT: u32 = 10;
const CW_RC: u16 = 3 << CW_RC_SHIFT;
/// The x87 control word after `FE_DFL_ENV` (and at process start): every exception masked,
/// 64-bit precision, round to nearest.
pub const X87_DEFAULT_CONTROL: u16 = 0x037f;

fn flags_in(v: u32) -> Flags {
    Flags((v & u32::from(Flags::ALL.0)) as u8)
}

/// The current rounding direction (MXCSR's; `fesetround` keeps x87's equal to it).
pub fn rounding(m: &impl Machine) -> Rounding {
    rounding_of(m.mxcsr())
}

/// The rounding direction an MXCSR value selects ([`rounding`] of a value already read).
pub fn rounding_of(mxcsr: u32) -> Rounding {
    Rounding::from_field(mxcsr >> MX_RC_SHIFT)
}

/// Whether an MXCSR value holds the default control word, whatever its flags: round to nearest,
/// every exception masked, no flush-to-zero, no denormals-are-zero. That is the environment the
/// elementary functions compute in (D-10.13), so they can run in place without a switch.
pub const fn is_default_control(mxcsr: u32) -> bool {
    mxcsr & !MX_FLAGS == MX_DEFAULT
}

/// MXCSR flush-to-zero (results).
const MX_FTZ: u32 = 1 << 15;

/// MXCSR denormals-are-zero (operands).
const MX_DAZ: u32 = 1 << 6;

/// Whether an MXCSR value has flush-to-zero set: SSE results that would be subnormal are zeros
/// of their sign, with underflow and inexact raised (Intel SDM, FTZ; owner ruling 2026-10-23).
pub const fn is_ftz(mxcsr: u32) -> bool {
    mxcsr & MX_FTZ != 0
}

/// Whether an MXCSR value has denormals-are-zero set: SSE operands that are subnormal are read
/// as zeros of their sign, raising nothing (Intel SDM, DAZ; owner ruling 2026-10-23).
pub const fn is_daz(mxcsr: u32) -> bool {
    mxcsr & MX_DAZ != 0
}

/// Whether `sqrtss`/`sqrtsd` run in place under an MXCSR value (M6-2, D-10.22): every exception
/// masked, in any rounding direction, with or without FTZ and DAZ. The instruction is correctly
/// rounded in every direction by definition; FTZ cannot act on it (the square root of a non-zero
/// finite operand is always normal), and under DAZ it reads a subnormal operand as a zero of its
/// sign, which is the caller's own choice and honoured (owner ruling 2026-10-23).
pub const fn is_sqrt_in_place(mxcsr: u32) -> bool {
    is_default_control(mxcsr & !(MX_RC | MX_FTZ | MX_DAZ))
}

/// Whether an MXCSR value holds the default control word in some rounding direction: every
/// exception masked, no FTZ, no DAZ. `hypotf`'s fast entry runs only there (#1541); under FTZ or
/// DAZ it takes the accurate path, which honours them (owner ruling 2026-10-23).
pub const fn is_default_but_rounding(mxcsr: u32) -> bool {
    is_default_control(mxcsr & !MX_RC)
}

/// Whether an x87 control word is the default one in some rounding direction (M6-2): every
/// exception masked and 64-bit precision, the word `fsqrt` runs in place under.
pub const fn is_x87_default_but_rounding(cw: u16) -> bool {
    cw & !CW_RC == X87_DEFAULT_CONTROL
}

/// The rounding direction x87 arithmetic uses (`long double`).
pub fn x87_rounding(m: &impl Machine) -> Rounding {
    Rounding::from_field(u32::from(m.x87_control()) >> CW_RC_SHIFT)
}

/// Set the rounding direction of both units (`fesetround`).
pub fn set_rounding(m: &impl Machine, r: Rounding) {
    m.set_mxcsr((m.mxcsr() & !MX_RC) | (r.field() << MX_RC_SHIFT));
    m.set_x87_control((m.x87_control() & !CW_RC) | (r.field() << CW_RC_SHIFT) as u16);
}

/// The flags of `mask` that either unit has raised (`fetestexcept`).
pub fn test(m: &impl Machine, mask: Flags) -> Flags {
    flags_in(m.mxcsr() | u32::from(m.x87_status())).and(mask)
}

/// Clear the flags of `mask` in both units (`feclearexcept`).
pub fn clear(m: &impl Machine, mask: Flags) {
    let mask = mask.and(Flags::ALL);
    m.set_mxcsr(m.mxcsr() & !u32::from(mask.0));
    if flags_in(u32::from(m.x87_status())).any(mask) {
        let mut env = m.x87_store_env();
        env.status &= !u32::from(mask.0);
        if env.status & SW_FLAGS == 0 {
            env.status &= !SW_SUMMARY;
        }
        m.x87_load_env(&env);
    }
}

/// Set flags without delivering any trap (`fesetexcept`, `fesetexceptflag`).
pub fn set(m: &impl Machine, flags: Flags) {
    m.set_mxcsr(m.mxcsr() | u32::from(flags.and(Flags::ALL).0));
}

/// The exceptions whose traps are enabled (`fegetexcept`).
pub fn enabled(m: &impl Machine) -> Flags {
    Flags(!flags_in(m.mxcsr() >> MX_MASK_SHIFT).0).and(Flags::ALL)
}

/// `feraiseexcept`: as [`raise`], except that overflow, underflow and inexact (when their traps
/// are disabled) are set in the x87 status word, where glibc's `feraiseexcept` sets them
/// (observed through `fegetenv`, `spec/behaviour/libm-exact.glibc.txt.gz`); invalid and division
/// by zero go to MXCSR. Either way `fetestexcept` reports them.
pub fn raise_c(m: &impl Machine, flags: Flags) {
    let x87 = Flags(flags.0 & (Flags::OVERFLOW.0 | Flags::UNDERFLOW.0 | Flags::INEXACT.0)).and(Flags(!enabled(m).0));
    if !x87.is_empty() {
        let mut env = m.x87_store_env();
        env.status |= u32::from(x87.0);
        m.x87_load_env(&env);
    }
    raise(m, Flags(flags.0 & !x87.0));
}

/// Raise exceptions (the flags a math function reports), in the IEEE order. A flag whose trap is
/// disabled is set in MXCSR directly; one whose trap is enabled is raised by an SSE operation,
/// which delivers `SIGFPE`.
pub fn raise(m: &impl Machine, flags: Flags) {
    let flags = flags.and(Flags::ALL);
    if flags.is_empty() {
        return;
    }
    let trapping = flags.and(enabled(m));
    set(m, Flags(flags.0 & !trapping.0));
    if trapping.any(Flags::INVALID) {
        black_box(black_box(0.0f64) / black_box(0.0f64));
    }
    if trapping.any(Flags::DIVBYZERO) {
        black_box(black_box(1.0f64) / black_box(0.0f64));
    }
    if trapping.any(Flags::OVERFLOW) {
        black_box(black_box(f64::MAX) * black_box(f64::MAX));
    }
    if trapping.any(Flags::UNDERFLOW) {
        black_box(black_box(f64::MIN_POSITIVE) * black_box(f64::MIN_POSITIVE));
    }
    if trapping.any(Flags::INEXACT) {
        black_box(black_box(1.0f64) + black_box(f64::MIN_POSITIVE));
    }
}

/// Enable the traps of `flags` in both units; the previously enabled set (`feenableexcept`).
pub fn enable(m: &impl Machine, flags: Flags) -> Flags {
    let old = enabled(m);
    let f = flags.and(Flags::ALL);
    m.set_mxcsr(m.mxcsr() & !(u32::from(f.0) << MX_MASK_SHIFT));
    m.set_x87_control(m.x87_control() & !u16::from(f.0));
    old
}

/// Disable the traps of `flags` in both units; the previously enabled set (`fedisableexcept`).
pub fn disable(m: &impl Machine, flags: Flags) -> Flags {
    let old = enabled(m);
    let f = flags.and(Flags::ALL);
    m.set_mxcsr(m.mxcsr() | (u32::from(f.0) << MX_MASK_SHIFT));
    m.set_x87_control(m.x87_control() | u16::from(f.0));
    old
}

/// The whole environment: the x87 environment and MXCSR (`fenv_t`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Env {
    /// The x87 environment (control, status and tag words, instruction pointers).
    pub x87: X87Env,
    /// MXCSR.
    pub mxcsr: u32,
}

impl Env {
    /// `FE_DFL_ENV`: the start-up environment (all traps masked, round to nearest, no flags,
    /// x87 extended precision, an empty x87 register stack).
    pub const DEFAULT: Env = Env {
        x87: X87Env { control: X87_DEFAULT_CONTROL as u32, status: 0, tag: 0xffff, rest: [0; 4] },
        mxcsr: MX_DEFAULT,
    };
    /// `FE_NOMASK_ENV`: the default environment with the trap of every IEEE exception enabled
    /// (the x86 denormal-operand exception stays masked).
    pub const NOMASK: Env = Env {
        x87: X87Env { control: (X87_DEFAULT_CONTROL & !0x3d) as u32, status: 0, tag: 0xffff, rest: [0; 4] },
        mxcsr: MX_DEFAULT & !(0x3d << MX_MASK_SHIFT),
    };
}

/// The current environment (`fegetenv`).
pub fn get_env(m: &impl Machine) -> Env {
    Env { x87: m.x87_store_env(), mxcsr: m.mxcsr() }
}

/// Install an environment (`fesetenv`). Neither unit traps on flags it finds already set.
pub fn set_env(m: &impl Machine, env: &Env) {
    m.x87_load_env(&env.x87);
    m.set_mxcsr(env.mxcsr);
}

/// Save the environment, clear the flags and disable every trap (`feholdexcept`).
pub fn hold(m: &impl Machine) -> Env {
    let env = get_env(m);
    let mut x87 = env.x87;
    x87.status &= !(SW_FLAGS | SW_SUMMARY);
    x87.control |= u32::from(Flags::ALL.0);
    m.x87_load_env(&x87);
    m.set_mxcsr((env.mxcsr & !MX_FLAGS) | (u32::from(Flags::ALL.0) << MX_MASK_SHIFT));
    env
}

/// Install `env`, then raise the flags that were set before (`feupdateenv`).
pub fn update(m: &impl Machine, env: &Env) {
    let pending = test(m, Flags::ALL);
    set_env(m, env);
    raise_c(m, pending);
}

/// The control modes (`femode_t`): the x87 control word and MXCSR without its flags.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Mode {
    /// The x87 control word.
    pub control: u16,
    /// MXCSR with the flag bits clear.
    pub mxcsr: u32,
}

impl Mode {
    /// `FE_DFL_MODE`.
    pub const DEFAULT: Mode = Mode { control: X87_DEFAULT_CONTROL, mxcsr: MX_DEFAULT };
}

/// The current control modes (`fegetmode`).
pub fn get_mode(m: &impl Machine) -> Mode {
    Mode { control: m.x87_control(), mxcsr: m.mxcsr() & !MX_FLAGS }
}

/// Install control modes, keeping the flags (`fesetmode`).
pub fn set_mode(m: &impl Machine, mode: &Mode) {
    m.set_x87_control(mode.control);
    m.set_mxcsr((mode.mxcsr & !MX_FLAGS) | (m.mxcsr() & MX_FLAGS));
}

/// Enter the default SSE environment for a computation that assumes it (the elementary
/// functions, D-10.13): round to nearest, every exception masked, no flags, no flush-to-zero.
/// Returns the caller's MXCSR for [`leave`].
pub fn enter_default(m: &impl Machine) -> u32 {
    let saved = m.mxcsr();
    m.set_mxcsr(MX_DEFAULT);
    saved
}

/// Restore the MXCSR saved by [`enter_default`] (the flags the computation raised are dropped;
/// the caller raises the ones its result reports).
pub fn leave(m: &impl Machine, saved: u32) {
    m.set_mxcsr(saved);
}

/// Enter the elementary functions' environment with one MXCSR read (M6-1): when the caller's
/// MXCSR already holds the default control word ([`is_default_control`]) nothing is written and
/// the computation runs in place; otherwise switch as [`enter_default`] does. Returns the caller's
/// MXCSR and whether the computation runs in place: leave with [`settle`] when it does, with
/// [`leave`] and [`raise`] when it does not.
pub fn enter(m: &impl Machine) -> (u32, bool) {
    let saved = m.mxcsr();
    (saved, enter_from(m, saved))
}

/// [`enter`] on an MXCSR value the caller already read (a fast entry whose attempt refused:
/// `hypotf`, #1541), so the slow path does not read MXCSR a second time. Returns whether the
/// computation runs in place.
#[inline]
pub fn enter_from(m: &impl Machine, saved: u32) -> bool {
    let in_place = is_default_control(saved);
    if !in_place {
        m.set_mxcsr(MX_DEFAULT);
    }
    in_place
}

/// Read MXCSR once and say whether its control word is the default one, writing nothing: the
/// fast-path entry (MATH-M6-ENVELOPE-COST), which runs only in place.
#[inline]
pub fn peek(m: &impl Machine) -> (u32, bool) {
    let saved = m.mxcsr();
    (saved, is_default_control(saved))
}

/// End a computation [`enter`] ran in place: MXCSR becomes `saved` with `flags` raised, exactly
/// what [`leave`] then [`raise`] leave. Every trap is masked in the default control word, so
/// raising a flag is setting it, and a flag the computation raised on its own (an intermediate's
/// inexact, the denormal-operand bit) is dropped as [`leave`] drops it. MXCSR is not read
/// again (#1037). `hw` is a superset of the MXCSR flag bits the computation may have raised in
/// hardware (the denormal-operand bit included). When `saved` already holds every bit of `flags`
/// and `hw`, MXCSR now is `saved` exactly and the write is skipped (#1360, amending #1037's
/// always-write): an `ldmxcsr` per call serialises consecutive calls on Intel (Skylake-SP 57 vs
/// 33 cycles per call in MATH-M6-CALL-OVERHEAD's model; free on Zen1), and real programs keep
/// inexact sticky after their first inexact operation. Otherwise it is written once, as before.
#[inline]
pub fn settle(m: &impl Machine, saved: u32, flags: Flags, hw: Flags) {
    let flags = u32::from(flags.and(Flags::ALL).0);
    let need = flags | u32::from(hw.0);
    if saved & need != need {
        m.set_mxcsr(saved | flags);
    }
}

/// Set the flags of `mask` to their state in `saved`, without traps (`fesetexceptflag`).
pub fn restore_flags(m: &impl Machine, saved: Flags, mask: Flags) {
    clear(m, mask);
    set(m, saved.and(mask));
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use core::cell::Cell;

    /// A machine whose MXCSR and x87 words are plain cells: the logic, not the instructions.
    struct Fake {
        mxcsr: Cell<u32>,
        env: Cell<X87Env>,
    }

    impl Fake {
        fn new() -> Fake {
            Fake { mxcsr: Cell::new(MX_DEFAULT), env: Cell::new(Env::DEFAULT.x87) }
        }
    }

    impl Machine for Fake {
        fn thread_pointer(&self) -> usize { 0 }
        fn cpuid(&self, _: u32, _: u32) -> [u32; 4] { [0; 4] }
        fn xcr0(&self) -> Option<u64> { None }
        fn mxcsr(&self) -> u32 { self.mxcsr.get() }
        fn set_mxcsr(&self, v: u32) { self.mxcsr.set(v & 0xffbf) }
        fn x87_control(&self) -> u16 { self.env.get().control as u16 }
        fn set_x87_control(&self, v: u16) {
            let mut e = self.env.get();
            e.control = u32::from(v);
            self.env.set(e);
        }
        fn x87_status(&self) -> u16 { self.env.get().status as u16 }
        fn x87_clear_exceptions(&self) {
            let mut e = self.env.get();
            e.status &= !0xff;
            self.env.set(e);
        }
        fn x87_store_env(&self) -> X87Env { self.env.get() }
        fn x87_load_env(&self, env: &X87Env) { self.env.set(*env) }
        fn pkru(&self) -> Option<u32> { None }
        fn set_pkru(&self, _: u32) -> bool { false }
        fn barrier(&self, _: &[u8]) {}
    }

    #[test]
    fn rounding_is_written_to_both_units() {
        let m = Fake::new();
        for r in [Rounding::Up, Rounding::Down, Rounding::Zero, Rounding::Nearest] {
            set_rounding(&m, r);
            assert_eq!(rounding(&m), r);
            assert_eq!(x87_rounding(&m), r);
            assert_eq!(Rounding::from_c(r.to_c()), Some(r));
        }
        assert_eq!(Rounding::from_c(0x1234), None);
    }

    #[test]
    fn flags_of_either_unit_are_seen_and_cleared() {
        let m = Fake::new();
        let mut e = m.x87_store_env();
        e.status |= 0x05;
        m.x87_load_env(&e);
        set(&m, Flags::INEXACT);
        assert_eq!(test(&m, Flags::ALL), Flags(0x25));
        clear(&m, Flags::DIVBYZERO);
        assert_eq!(test(&m, Flags::ALL), Flags(0x21));
        clear(&m, Flags::ALL);
        assert_eq!(test(&m, Flags::ALL), Flags::NONE);
    }

    #[test]
    fn traps_hold_and_modes() {
        let m = Fake::new();
        assert_eq!(enable(&m, Flags::DIVBYZERO), Flags::NONE);
        assert_eq!(enable(&m, Flags::INVALID), Flags::DIVBYZERO);
        assert_eq!(enabled(&m), Flags(0x05));
        assert_eq!(m.x87_control() & 0x3f, 0x3f & !0x05);
        set(&m, Flags::OVERFLOW);
        let saved = hold(&m);
        assert_eq!(enabled(&m), Flags::NONE);
        assert_eq!(test(&m, Flags::ALL), Flags::NONE);
        set(&m, Flags::INEXACT);
        assert_eq!(disable(&m, Flags::ALL), Flags::NONE);
        // feupdateenv re-raises INEXACT; its trap is disabled in the environment installed.
        let mut quiet = saved;
        quiet.mxcsr |= 0x3d << MX_MASK_SHIFT;
        update(&m, &quiet);
        assert_eq!(test(&m, Flags::ALL), Flags(0x28));
        set_env(&m, &Env::DEFAULT);
        set_rounding(&m, Rounding::Down);
        let mode = get_mode(&m);
        set(&m, Flags::INVALID);
        set_mode(&m, &Mode::DEFAULT);
        assert_eq!((rounding(&m), test(&m, Flags::ALL)), (Rounding::Nearest, Flags::INVALID));
        set_mode(&m, &mode);
        assert_eq!(rounding(&m), Rounding::Down);
        restore_flags(&m, Flags::DIVBYZERO, Flags(0x05));
        assert_eq!(test(&m, Flags::ALL), Flags::DIVBYZERO);
    }
}
