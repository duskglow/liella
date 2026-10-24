//! The C shapes of spec §20 (signals, phase 5): `struct sigaction` and the 1024-bit `sigset_t`
//! over the kernel's 64-bit actions and sets, with the library's internal signal hidden
//! (D-20.1: signal 32 only, so `SIGRTMIN` is 33); `signal`'s BSD semantics and the SVR4, SVID
//! and 4.2BSD forms (D-20.4); `raise`, `killpg`, `sigqueue`, the waits; the real-time signal
//! allocation; and the descriptions of §20.8.
//!
//! Installing a handler needs the handler's address in the kernel action, which only the
//! boundary may pass (D-X.9): the functions here take the system call as `rt`, a closure over
//! `rt_sigaction` with optional new action and returning the old one.

use core::sync::atomic::{AtomicI32, AtomicU64, Ordering};

use liella::error::{Errno, Result};
use liella_abi::layout::sigset_t;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{KSigAction, SigInfo, SigSet, StackT, Timespec};

/// The library's internal signal (D-20.1): cancellation and the credential broadcast.
pub const SIG_INTERNAL: i32 = 32;
/// `_NSIG`: signals are 1..`NSIG`.
pub const NSIG: i32 = 65;
/// `SIGRTMIN` as applications see it (D-20.1).
pub const SIGRTMIN: i32 = 33;
/// `SIGRTMAX`.
pub const SIGRTMAX: i32 = 64;

/// `SIG_DFL` (handler value).
pub const SIG_DFL: usize = 0;
/// `SIG_IGN`.
pub const SIG_IGN: usize = 1;
/// `SIG_HOLD` (SVR4 `sigset`).
pub const SIG_HOLD: usize = 2;
/// `SIG_ERR`.
pub const SIG_ERR: usize = usize::MAX;

const SA_ONSTACK: u64 = 0x0800_0000;
const SA_RESTART: u64 = 0x1000_0000;
const SA_NODEFER: u64 = 0x4000_0000;
const SA_RESETHAND: u64 = 0x8000_0000;
const SA_RESTORER: u64 = 0x0400_0000;

const SIG_BLOCK: u32 = 0;
const SIG_UNBLOCK: u32 = 1;
const SIG_SETMASK: u32 = 2;

/// The internal signal's bit in a kernel set.
const INTERNAL_BIT: u64 = 1 << (SIG_INTERNAL - 1);

/// `struct sigaction` as a C program lays it out (x86-64, 152 bytes): the handler (a union of
/// the one- and three-argument forms, one address), the 1024-bit mask, the flags, and the
/// restorer. Plain fields: the ABI crate's definition models the handler as a union, whose
/// fields only `unsafe` code may read. The layout equality is tested.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct CSigaction {
    /// `sa_handler` / `sa_sigaction`.
    pub handler: usize,
    /// `sa_mask`.
    pub mask: [u64; 16],
    /// `sa_flags`.
    pub flags: i32,
    _pad: i32,
    /// `sa_restorer`.
    pub restorer: usize,
}

impl CSigaction {
    /// An action with `handler`, `mask` and `flags`.
    pub fn new(handler: usize, mask: SigSet, flags: u64) -> CSigaction {
        CSigaction { handler, mask: c_set(mask).__val, flags: i32::from_ne_bytes(u32::try_from(flags & 0xffff_ffff).unwrap_or(0).to_ne_bytes()), _pad: 0, restorer: 0 }
    }
}

// ------------------------------------------------------------------------------ sets

/// Whether `sig` is a signal number at all (1..`NSIG`).
fn in_range(sig: i32) -> bool {
    (1..NSIG).contains(&sig)
}

/// `EINVAL` unless `sig` is a signal an application may name in a set operation or an action
/// (not the internal one).
pub fn check(sig: i32) -> Result<()> {
    if in_range(sig) && sig != SIG_INTERNAL { Ok(()) } else { Err(Errno::EINVAL) }
}

/// The word and bit of `sig` in a `sigset_t` (the kernel's order: signal 1 is bit 0).
fn position(sig: i32) -> (usize, u64) {
    let n = u32::try_from(sig.saturating_sub(1)).unwrap_or(0);
    (usize::try_from(n / 64).unwrap_or(0), 1u64 << (n % 64))
}

/// A kernel set as the C `sigset_t`, with the internal signal hidden.
pub fn c_set(k: SigSet) -> sigset_t {
    let mut s = sigset_t { __val: [0; 16] };
    s.__val[0] = k.0 & !INTERNAL_BIT;
    s
}

/// A C `sigset_t` as the kernel's set: the first 64 signals (D-20.3), the internal one never
/// included.
pub fn kernel_set(s: &sigset_t) -> SigSet {
    SigSet(s.__val[0] & !INTERNAL_BIT)
}

/// `sigemptyset`.
pub fn emptyset(s: &mut sigset_t) {
    s.__val = [0; 16];
}

/// `sigfillset`: every signal but the internal one.
pub fn fillset(s: &mut sigset_t) {
    s.__val = [u64::MAX; 16];
    s.__val[0] &= !INTERNAL_BIT;
}

/// `sigaddset`: `EINVAL` for a number out of range or the internal signal.
pub fn addset(s: &mut sigset_t, sig: i32) -> Result<()> {
    check(sig)?;
    let (w, b) = position(sig);
    if let Some(word) = s.__val.get_mut(w) {
        *word |= b;
    }
    Ok(())
}

/// `sigdelset`: as [`addset`].
pub fn delset(s: &mut sigset_t, sig: i32) -> Result<()> {
    check(sig)?;
    let (w, b) = position(sig);
    if let Some(word) = s.__val.get_mut(w) {
        *word &= !b;
    }
    Ok(())
}

/// `sigismember`: `EINVAL` only for a number out of range.
pub fn ismember(s: &sigset_t, sig: i32) -> Result<bool> {
    if !in_range(sig) {
        return Err(Errno::EINVAL);
    }
    let (w, b) = position(sig);
    Ok(s.__val.get(w).is_some_and(|word| word & b != 0))
}

/// `__sigaddset`, `__sigdelset`, `__sigismember` (the compat inline helpers): no checks
/// beyond the range (glibc has none; an out-of-range number changes nothing here).
pub fn raw_op(s: &mut sigset_t, sig: i32, op: u8) -> i32 {
    if !in_range(sig) {
        return 0;
    }
    let (w, b) = position(sig);
    let Some(word) = s.__val.get_mut(w) else { return 0 };
    match op {
        b'+' => *word |= b,
        b'-' => *word &= !b,
        _ => return i32::from(*word & b != 0),
    }
    0
}

/// `__sigismember` on a read-only set (no checks beyond the range).
pub fn raw_ismember(s: &sigset_t, sig: i32) -> i32 {
    let mut copy = *s;
    raw_op(&mut copy, sig, b'?')
}

/// `sigandset` / `sigorset` into the caller's destination.
pub fn combine_into(d: &mut sigset_t, a: &sigset_t, b: &sigset_t, or: bool) {
    *d = combine(a, b, or);
}

/// `sigisemptyset` (GNU).
pub fn isemptyset(s: &sigset_t) -> bool {
    s.__val.iter().all(|&w| w == 0)
}

/// `sigandset` / `sigorset` (GNU): `dest = a & b` or `a | b`.
pub fn combine(a: &sigset_t, b: &sigset_t, or: bool) -> sigset_t {
    let mut d = sigset_t { __val: [0; 16] };
    for ((d, a), b) in d.__val.iter_mut().zip(&a.__val).zip(&b.__val) {
        *d = if or { a | b } else { a & b };
    }
    d
}

// ------------------------------------------------------------------------------ actions

/// The kernel action for `a`, with the library's restorer (⚑ KERN: x86-64 requires one).
fn kernel_action(a: &CSigaction, restorer: usize) -> KSigAction {
    let flags = u64::from(u32::from_ne_bytes(a.flags.to_ne_bytes())) | SA_RESTORER;
    let mut mask = sigset_t { __val: a.mask };
    mask.__val[0] &= !INTERNAL_BIT;
    KSigAction { handler: a.handler, flags, restorer, mask: kernel_set(&mask) }
}

/// The C view of a kernel action, as glibc reports it (`SA_RESTORER` and the restorer
/// included).
fn c_action(k: &KSigAction) -> CSigaction {
    CSigaction {
        handler: k.handler,
        mask: c_set(k.mask).__val,
        flags: i32::from_ne_bytes(u32::try_from(k.flags & 0xffff_ffff).unwrap_or(0).to_ne_bytes()),
        _pad: 0,
        restorer: k.restorer,
    }
}

/// `sigaction(sig, act, oact)` (§20.2): the old action in the C shape.
pub fn sigaction(sig: i32, act: Option<&CSigaction>, restorer: usize, rt: impl FnOnce(i32, Option<&KSigAction>) -> Result<KSigAction>) -> Result<CSigaction> {
    check(sig)?;
    let new = act.map(|a| kernel_action(a, restorer));
    rt(sig, new.as_ref()).map(|old| c_action(&old))
}

/// The process-wide set of signals `siginterrupt` made interrupting (glibc's `_sigintr`): for
/// those, `signal` installs without `SA_RESTART`.
static SIGINTR: AtomicU64 = AtomicU64::new(0);

/// `signal(sig, handler)` (§20.2): BSD semantics (D-20.4), the handler kept installed, the
/// signal blocked while it runs, calls restarted unless `siginterrupt` said otherwise.
/// Returns the old handler.
pub fn bsd_signal(sig: i32, handler: usize, restorer: usize, rt: impl FnOnce(i32, Option<&KSigAction>) -> Result<KSigAction>) -> Result<usize> {
    if handler == SIG_ERR {
        return Err(Errno::EINVAL);
    }
    check(sig)?;
    let restart = if SigSet(SIGINTR.load(Ordering::Relaxed)).contains(sig) { 0 } else { SA_RESTART };
    let a = CSigaction::new(handler, SigSet::empty().with(sig), restart);
    sigaction(sig, Some(&a), restorer, rt).map(|old| old.handler)
}

/// `sysv_signal(sig, handler)` (§20.2): SVR4 semantics, a one-shot handler, the signal not
/// blocked while it runs, calls not restarted.
pub fn sysv_signal(sig: i32, handler: usize, restorer: usize, rt: impl FnOnce(i32, Option<&KSigAction>) -> Result<KSigAction>) -> Result<usize> {
    if handler == SIG_ERR {
        return Err(Errno::EINVAL);
    }
    check(sig)?;
    let a = CSigaction::new(handler, SigSet::empty(), SA_RESETHAND | SA_NODEFER);
    sigaction(sig, Some(&a), restorer, rt).map(|old| old.handler)
}

/// `siginterrupt(sig, flag)` (§20.2): toggle `SA_RESTART` on the installed action, and record
/// the choice for later `signal` calls.
pub fn siginterrupt(sig: i32, interrupt: bool, restorer: usize, mut rt: impl FnMut(i32, Option<&KSigAction>) -> Result<KSigAction>) -> Result<()> {
    check(sig)?;
    let mut a = c_action(&rt(sig, None)?);
    let bit = SigSet::empty().with(sig).0;
    let restart: i32 = 0x1000_0000; // SA_RESTART in the C flags word
    if interrupt {
        SIGINTR.fetch_or(bit, Ordering::Relaxed);
        a.flags &= !restart;
    } else {
        SIGINTR.fetch_and(!bit, Ordering::Relaxed);
        a.flags |= restart;
    }
    rt(sig, Some(&kernel_action(&a, restorer))).map(|_| ())
}

/// `sigignore(sig)` (§20.2, SVR4).
pub fn sigignore(sig: i32, restorer: usize, rt: impl FnOnce(i32, Option<&KSigAction>) -> Result<KSigAction>) -> Result<()> {
    sigaction(sig, Some(&CSigaction::new(SIG_IGN, SigSet::empty(), 0)), restorer, rt).map(|_| ())
}

/// `sighold(sig)` / `sigrelse(sig)` (§20.2, SVR4): block or unblock one signal.
pub fn sighold<S: Sys>(sys: &S, sig: i32, hold: bool) -> Result<()> {
    check(sig)?;
    sys.sigprocmask(if hold { SIG_BLOCK } else { SIG_UNBLOCK }, Some(&SigSet::empty().with(sig))).map(|_| ())
}

/// `sigset(sig, disp)` (§20.2, SVR4): `SIG_HOLD` blocks the signal and returns the old
/// disposition (or `SIG_HOLD` if it was already blocked); any other disposition is installed
/// and the signal unblocked, returning `SIG_HOLD` if it had been blocked.
pub fn sigset<S: Sys>(sys: &S, sig: i32, disp: usize, restorer: usize, mut rt: impl FnMut(i32, Option<&KSigAction>) -> Result<KSigAction>) -> Result<usize> {
    check(sig)?;
    let one = SigSet::empty().with(sig);
    if disp == SIG_HOLD {
        let old = sys.sigprocmask(SIG_BLOCK, Some(&one))?;
        if old.contains(sig) {
            return Ok(SIG_HOLD);
        }
        return rt(sig, None).map(|o| o.handler);
    }
    let old = sigaction(sig, Some(&CSigaction::new(disp, SigSet::empty(), 0)), restorer, &mut rt)?;
    let mask = sys.sigprocmask(SIG_UNBLOCK, Some(&one))?;
    Ok(if mask.contains(sig) { SIG_HOLD } else { old.handler })
}

/// `struct sigvec` (4.2BSD, compat): handler, an `int` mask of signals 1–32, `SV_*` flags.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct CSigvec {
    /// `sv_handler`.
    pub handler: usize,
    /// `sv_mask`.
    pub mask: i32,
    /// `sv_flags`.
    pub flags: i32,
}

const SV_ONSTACK: i32 = 1;
const SV_INTERRUPT: i32 = 2;
const SV_RESETHAND: i32 = 4;

/// A BSD `int` mask (bit `n - 1` for signal `n`, signals 1–32) as a kernel set.
fn bsd_mask(mask: i32) -> SigSet {
    SigSet(u64::from(u32::from_ne_bytes(mask.to_ne_bytes())))
}

/// A kernel set's signals 1–32 as a BSD `int` mask.
fn bsd_int(s: SigSet) -> i32 {
    i32::from_ne_bytes(u32::try_from(s.0 & 0xffff_ffff).unwrap_or(0).to_ne_bytes())
}

/// `sigvec(sig, vec, ovec)` (§20.2, compat).
pub fn sigvec(sig: i32, vec: Option<&CSigvec>, restorer: usize, rt: impl FnOnce(i32, Option<&KSigAction>) -> Result<KSigAction>) -> Result<CSigvec> {
    let new = vec.map(|v| {
        let mut flags = 0;
        if v.flags & SV_ONSTACK != 0 {
            flags |= SA_ONSTACK;
        }
        if v.flags & SV_INTERRUPT == 0 {
            flags |= SA_RESTART;
        }
        if v.flags & SV_RESETHAND != 0 {
            flags |= SA_RESETHAND;
        }
        CSigaction::new(v.handler, bsd_mask(v.mask), flags)
    });
    let old = sigaction(sig, new.as_ref(), restorer, rt)?;
    let oflags = u64::from(u32::from_ne_bytes(old.flags.to_ne_bytes()));
    let mut sv = 0;
    if oflags & SA_ONSTACK != 0 {
        sv |= SV_ONSTACK;
    }
    if oflags & SA_RESTART == 0 {
        sv |= SV_INTERRUPT;
    }
    if oflags & SA_RESETHAND != 0 {
        sv |= SV_RESETHAND;
    }
    Ok(CSigvec { handler: old.handler, mask: bsd_int(kernel_set(&sigset_t { __val: old.mask })), flags: sv })
}

// ------------------------------------------------------------------------------ masks

/// `sigprocmask(how, set, oset)` (§20.3): the internal signal is never blocked and never
/// shown; `SIGKILL`/`SIGSTOP` are dropped by the kernel.
pub fn sigprocmask<S: Sys>(sys: &S, how: i32, set: Option<&sigset_t>) -> Result<sigset_t> {
    let how = u32::try_from(how).map_err(|_| Errno::EINVAL)?;
    let new = set.map(kernel_set);
    sys.sigprocmask(how, new.as_ref()).map(c_set)
}

/// `sigblock(mask)` (§20.3, BSD): add the `int` mask; the old mask as an `int`.
pub fn sigblock<S: Sys>(sys: &S, mask: i32) -> Result<i32> {
    sys.sigprocmask(SIG_BLOCK, Some(&SigSet(bsd_mask(mask).0 & !INTERNAL_BIT))).map(bsd_int)
}

/// `sigsetmask(mask)` (§20.3, BSD): replace the first 32 signals' mask with `mask`; the old
/// mask as an `int`.
pub fn sigsetmask<S: Sys>(sys: &S, mask: i32) -> Result<i32> {
    sys.sigprocmask(SIG_SETMASK, Some(&SigSet(bsd_mask(mask).0 & !INTERNAL_BIT))).map(bsd_int)
}

/// `siggetmask()` (§20.3, BSD).
pub fn siggetmask<S: Sys>(sys: &S) -> Result<i32> {
    sys.sigprocmask(SIG_BLOCK, None).map(|m| bsd_int(SigSet(m.0 & !INTERNAL_BIT)))
}

/// `sigpause` (§20.3): the BSD form (`is_sig` false) waits with the `int` mask; the XSI form
/// (`__xpg_sigpause`) waits with the current mask minus `sig`.
pub fn sigpause<S: Sys>(sys: &S, sig_or_mask: i32, is_sig: bool) -> Result<()> {
    let set = if is_sig {
        let cur = sys.sigprocmask(SIG_BLOCK, None)?;
        let gone = SigSet::empty().with(sig_or_mask).0;
        SigSet(cur.0 & !gone)
    } else {
        bsd_mask(sig_or_mask)
    };
    sys.sigsuspend(&SigSet(set.0 & !INTERNAL_BIT))
}

// ------------------------------------------------------------------------------ sending

/// Every signal an application can block (the internal one excluded).
const APP_SIGNALS: SigSet = SigSet(!INTERNAL_BIT);

/// `raise(sig)` (§20.4): to the calling thread, with every application signal blocked around
/// reading the thread's ID and sending, so a handler that forks cannot redirect it. The
/// internal signal is `EINVAL` (OD-8, as glibc refuses its own; `kill`, `tgkill`, `sigqueue`
/// and `killpg` stay the kernel's, as glibc's: there the library's handler takes it).
pub fn raise<S: Sys>(sys: &S, sig: i32) -> Result<()> {
    if sig == SIG_INTERNAL {
        return Err(Errno::EINVAL);
    }
    let old = sys.sigprocmask(SIG_BLOCK, Some(&APP_SIGNALS))?;
    let r = sys.tgkill(sys.getpid(), sys.gettid(), sig);
    let _ = sys.sigprocmask(SIG_SETMASK, Some(&old));
    r
}

/// `killpg(pgrp, sig)` (§20.4): a negative group is `EINVAL`.
pub fn killpg<S: Sys>(sys: &S, pgrp: i32, sig: i32) -> Result<()> {
    if pgrp < 0 {
        return Err(Errno::EINVAL);
    }
    sys.kill(pgrp.checked_neg().ok_or(Errno::EINVAL)?, sig)
}

/// `sigqueue(pid, sig, value)` (§20.4): the signal with a value (`SI_QUEUE`, from this
/// process).
pub fn sigqueue<S: Sys>(sys: &S, pid: i32, sig: i32, value: u64) -> Result<()> {
    let info = SigInfo::queued(sig, sys.getpid(), sys.getuid(), value);
    sys.sigqueueinfo(pid, sig, &info)
}

// ------------------------------------------------------------------------------ waiting

/// `sigsuspend(mask)` (§20.5).
pub fn sigsuspend<S: Sys>(sys: &S, mask: &sigset_t) -> Result<()> {
    sys.sigsuspend(&kernel_set(mask))
}

/// `sigwait(set, &sig)` (§20.5): accept a pending signal of `set`, retrying after a handler
/// interrupted the wait. The internal signal is never accepted.
pub fn sigwait<S: Sys>(sys: &S, set: &sigset_t) -> Result<i32> {
    let k = kernel_set(set);
    loop {
        match sys.sigtimedwait(&k, None, None) {
            Err(Errno::EINTR) => {}
            r => return r,
        }
    }
}

/// `sigtimedwait(set, info, timeout)` / `sigwaitinfo` (§20.5): the accepted signal's number.
pub fn sigtimedwait<S: Sys>(sys: &S, set: &sigset_t, info: Option<&mut SigInfo>, timeout: Option<&Timespec>) -> Result<i32> {
    sys.sigtimedwait(&kernel_set(set), info, timeout)
}

/// `sigpending(set)` (§20.5).
pub fn sigpending<S: Sys>(sys: &S) -> Result<sigset_t> {
    sys.sigpending().map(c_set)
}

// ------------------------------------------------------------------------------ stacks

/// `struct sigstack` (compat): the stack and whether the thread is on it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct CSigstack {
    /// `ss_sp`.
    pub sp: usize,
    /// `ss_onstack`.
    pub onstack: i32,
    _pad: i32,
}

/// `sigstack(ss, oss)` (§20.7, compat): setting a stack is `ENOSYS` (the size is unknown);
/// the current one is reported.
pub fn sigstack(set: bool, current: impl FnOnce() -> Result<StackT>) -> Result<CSigstack> {
    if set {
        return Err(Errno::ENOSYS);
    }
    let s = current()?;
    Ok(CSigstack { sp: s.sp, onstack: s.flags & 1, _pad: 0 })
}

// ------------------------------------------------------------------------------ real-time

static RT_LOW: AtomicI32 = AtomicI32::new(SIGRTMIN);
static RT_HIGH: AtomicI32 = AtomicI32::new(SIGRTMAX);

/// `__libc_current_sigrtmin()` (§20.1): 33, after any allocations from the bottom.
pub fn current_rtmin() -> i32 {
    RT_LOW.load(Ordering::Relaxed)
}

/// `__libc_current_sigrtmax()` (§20.1).
pub fn current_rtmax() -> i32 {
    RT_HIGH.load(Ordering::Relaxed)
}

/// `__libc_allocate_rtsig(high)` (§20.1): take a real-time signal from the top (`high`) or
/// the bottom of the remaining range; -1 when none is left.
pub fn allocate_rtsig(high: bool) -> i32 {
    loop {
        let (lo, hi) = (RT_LOW.load(Ordering::Relaxed), RT_HIGH.load(Ordering::Relaxed));
        if lo > hi {
            return -1;
        }
        let (cell, cur, next) = if high { (&RT_LOW, lo, lo.saturating_add(1)) } else { (&RT_HIGH, hi, hi.saturating_sub(1)) };
        if cell.compare_exchange(cur, next, Ordering::Relaxed, Ordering::Relaxed).is_ok() {
            return cur;
        }
    }
}

/// `sigreturn` (§20.2): an exported stub; returning from a handler goes through the
/// restorer.
pub fn sigreturn() -> Result<()> {
    Err(Errno::ENOSYS)
}

// ------------------------------------------------------------------------------ §20.8

/// `strsignal`'s text for a number without a description when the thread has no result slot.
const UNKNOWN_SIGNAL: &[u8] = b"Unknown signal\0";

/// The address of a static text, or 0 (NULL) for none.
pub fn text_addr(t: Option<&'static [u8]>) -> usize {
    t.map_or(0, |t| t.as_ptr().expose_provenance())
}

/// `strsignal(sig)` (§20.8): the description, else the unknown-signal text in `slot` (the
/// calling thread's, D-H3). Without a slot (a thread that could not get one): the constant
/// `Unknown signal`, never a buffer another thread writes (D-C0.2). The address for the C
/// caller.
pub fn strsignal(sig: i32, slot: Option<&crate::result_slots::Buf<32>>) -> usize {
    match sigdescr(sig) {
        Some(d) => text_addr(Some(d)),
        None => {
            let mut b = [0u8; 32];
            let n = unknown_text(sig, &mut b);
            let Some(buf) = slot else { return text_addr(Some(UNKNOWN_SIGNAL)) };
            buf.store(b.get(..n).unwrap_or(&[]));
            buf.addr()
        }
    }
}

/// glibc's signal descriptions (`sigdescr_np`, `sys_siglist`), NUL-terminated.
pub const DESCRIPTIONS: [&[u8]; 32] = [
    b"\0", b"Hangup\0", b"Interrupt\0", b"Quit\0", b"Illegal instruction\0", b"Trace/breakpoint trap\0", b"Aborted\0", b"Bus error\0",
    b"Floating point exception\0", b"Killed\0", b"User defined signal 1\0", b"Segmentation fault\0", b"User defined signal 2\0",
    b"Broken pipe\0", b"Alarm clock\0", b"Terminated\0", b"Stack fault\0", b"Child exited\0", b"Continued\0", b"Stopped (signal)\0",
    b"Stopped\0", b"Stopped (tty input)\0", b"Stopped (tty output)\0", b"Urgent I/O condition\0", b"CPU time limit exceeded\0",
    b"File size limit exceeded\0", b"Virtual timer expired\0", b"Profiling timer expired\0", b"Window changed\0", b"I/O possible\0",
    b"Power failure\0", b"Bad system call\0",
];

/// glibc's abbreviations (`sigabbrev_np`, `sys_sigabbrev`): the names without `SIG`.
pub const ABBREVIATIONS: [&[u8]; 32] = [
    b"\0", b"HUP\0", b"INT\0", b"QUIT\0", b"ILL\0", b"TRAP\0", b"ABRT\0", b"BUS\0", b"FPE\0", b"KILL\0", b"USR1\0", b"SEGV\0", b"USR2\0",
    b"PIPE\0", b"ALRM\0", b"TERM\0", b"STKFLT\0", b"CHLD\0", b"CONT\0", b"STOP\0", b"TSTP\0", b"TTIN\0", b"TTOU\0", b"URG\0", b"XCPU\0",
    b"XFSZ\0", b"VTALRM\0", b"PROF\0", b"WINCH\0", b"IO\0", b"PWR\0", b"SYS\0",
];

fn table(t: &'static [&'static [u8]; 32], sig: i32) -> Option<&'static [u8]> {
    usize::try_from(sig).ok().filter(|&s| s >= 1).and_then(|s| t.get(s)).copied()
}

/// `sigdescr_np(sig)` (§20.8, 2.32): the untranslated description, `None` for other numbers.
pub fn sigdescr(sig: i32) -> Option<&'static [u8]> {
    table(&DESCRIPTIONS, sig)
}

/// `sigabbrev_np(sig)` (§20.8, 2.32).
pub fn sigabbrev(sig: i32) -> Option<&'static [u8]> {
    table(&ABBREVIATIONS, sig)
}

/// Appends `n` in decimal.
fn decimal(out: &mut impl FnMut(&[u8]), n: i64) {
    let mut d = [0u8; 20];
    let neg = n < 0;
    let digits = liella_rt::fatal::decimal(n.unsigned_abs(), &mut d);
    if neg {
        out(b"-");
    }
    out(digits);
}

/// `strsignal(sig)`'s text for a number without a description (§20.8): "Real-time signal N"
/// (counted from `SIGRTMIN`) or "Unknown signal N", NUL-terminated, into `buf`; returns the
/// length written.
pub fn unknown_text(sig: i32, buf: &mut [u8]) -> usize {
    let mut n = 0usize;
    let mut put = |b: &[u8]| {
        for (d, s) in buf.iter_mut().skip(n).zip(b) {
            *d = *s;
        }
        n = n.saturating_add(b.len());
    };
    let (lo, hi) = (current_rtmin(), current_rtmax());
    if (lo..=hi).contains(&sig) {
        put(b"Real-time signal ");
        decimal(&mut put, i64::from(sig.saturating_sub(lo)));
    } else {
        put(b"Unknown signal ");
        decimal(&mut put, i64::from(sig));
    }
    put(b"\0");
    n
}

/// `psignal(sig, s)` (§20.8): "s: description\n" (or the unknown-signal text) to `out`.
pub fn psignal(sig: i32, prefix: Option<&[u8]>, mut out: impl FnMut(&[u8])) {
    if let Some(p) = prefix.filter(|p| !p.is_empty()) {
        out(p);
        out(b": ");
    }
    match sigdescr(sig) {
        Some(d) => {
            out(d.strip_suffix(b"\0").unwrap_or(d));
            out(b"\n");
        }
        None => {
            out(b"Unknown signal ");
            decimal(&mut out, i64::from(sig));
            out(b"\n");
        }
    }
}

const ILL: [&[u8]; 8] = [b"Illegal opcode", b"Illegal operand", b"Illegal addressing mode", b"Illegal trap", b"Privileged opcode", b"Privileged register", b"Coprocessor error", b"Internal stack error"];
const FPE: [&[u8]; 8] = [
    b"Integer divide by zero", b"Integer overflow", b"Floating-point divide by zero", b"Floating-point overflow", b"Floating-point underflow",
    b"Floating-poing inexact result", b"Invalid floating-point operation", b"Subscript out of range",
];
const SEGV: [&[u8]; 2] = [b"Address not mapped to object", b"Invalid permissions for mapped object"];
const BUS: [&[u8]; 3] = [b"Invalid address alignment", b"Nonexisting physical address", b"Object-specific hardware error"];
const TRAP: [&[u8]; 2] = [b"Process breakpoint", b"Process trace trap"];
const CLD: [&[u8]; 6] = [
    b"Child has exited", b"Child has terminated abnormally and did not create a core file", b"Child has terminated abnormally and created a core file",
    b"Traced child has trapped", b"Child has stopped", b"Stopped child has continued",
];
const POLL: [&[u8]; 6] = [b"Data input available", b"Output buffers available", b"Input message available", b"I/O error", b"High priority input available", b"Device disconnected"];

/// Appends `v` as `%p` does: `0x` and lowercase hex, or `(nil)` for 0.
fn pointer(out: &mut impl FnMut(&[u8]), v: u64) {
    if v == 0 {
        out(b"(nil)");
        return;
    }
    let mut d = [0u8; 16];
    let mut i = d.len();
    let mut x = v;
    while x != 0 {
        i = i.saturating_sub(1);
        if let Some(slot) = d.get_mut(i) {
            *slot = b"0123456789abcdef".get(usize::try_from(x & 15).unwrap_or(0)).copied().unwrap_or(b'0');
        }
        x >>= 4;
    }
    out(b"0x");
    out(d.get(i..).unwrap_or(&[]));
}

/// `psiginfo(info, s)` (§20.8): glibc's format: the description, the code's meaning (or its
/// number), and the address of a fault, the child's status, the band of `SIGPOLL`, or the
/// sender's PID and UID.
pub fn psiginfo(info: &SigInfo, prefix: Option<&[u8]>, mut out: impl FnMut(&[u8])) {
    if let Some(p) = prefix.filter(|p| !p.is_empty()) {
        out(p);
        out(b": ");
    }
    let sig = info.signo;
    let (lo, hi) = (current_rtmin(), current_rtmax());
    let desc = sigdescr(sig);
    if desc.is_none() && !(lo..hi).contains(&sig) {
        out(b"Unknown signal ");
        decimal(&mut out, i64::from(sig));
        out(b"\n");
        return;
    }
    match desc {
        Some(d) => out(d.strip_suffix(b"\0").unwrap_or(d)),
        None => {
            // glibc names a real-time signal relative to the nearer end of the range, bare at
            // the end itself.
            let (from_lo, from_hi) = (sig.saturating_sub(lo), hi.saturating_sub(sig));
            if from_lo <= from_hi {
                out(b"SIGRTMIN");
                if from_lo != 0 {
                    out(b"+");
                    decimal(&mut out, i64::from(from_lo));
                }
            } else {
                out(b"SIGRTMAX");
                if from_hi != 0 {
                    out(b"-");
                    decimal(&mut out, i64::from(from_hi));
                }
            }
        }
    }
    out(b" (");
    let codes: &[&[u8]] = match sig {
        4 => &ILL,
        8 => &FPE,
        11 => &SEGV,
        7 => &BUS,
        5 => &TRAP,
        17 => &CLD,
        29 => &POLL,
        _ => &[],
    };
    let code = info.code;
    let text: Option<&[u8]> = match code {
        0 => Some(b"Signal sent by kill()"),
        -1 => Some(b"Signal sent by sigqueue()"),
        -2 => Some(b"Signal generated by the expiration of a timer"),
        -3 => Some(b"Signal generated by the completion of an asynchronous I/O request"),
        -4 => Some(b"Signal generated by the arrival of a message on an empty message queue"),
        -6 => Some(b"Signal sent by tkill()"),
        -60 => Some(b"Signal generated by the completion of an asynchronous name lookup request"),
        -5 => Some(b"Signal generated by the completion of an I/O request"),
        0x80 => Some(b"Signal sent by the kernel"),
        c if c >= 1 => usize::try_from(c.saturating_sub(1)).ok().and_then(|i| codes.get(i)).copied(),
        _ => None,
    };
    match text {
        Some(t) => {
            out(t);
            out(b" ");
        }
        None => {
            decimal(&mut out, i64::from(code));
            out(b" ");
        }
    }
    let word = |i: usize| info.fields.get(i).copied().unwrap_or(0);
    let lo32 = |w: u64| i64::from(i32::from_ne_bytes(u32::try_from(w & 0xffff_ffff).unwrap_or(0).to_ne_bytes()));
    let hi32 = |w: u64| i64::from(u32::try_from(w >> 32).unwrap_or(0));
    match sig {
        4 | 8 | 11 | 7 => {
            out(b"[");
            pointer(&mut out, word(0));
            out(b"])\n");
        }
        17 => {
            decimal(&mut out, lo32(word(0)));
            out(b" ");
            decimal(&mut out, lo32(word(1)));
            out(b" ");
            decimal(&mut out, hi32(word(0)));
            out(b")\n");
        }
        29 => {
            decimal(&mut out, i64::from_ne_bytes(word(0).to_ne_bytes()));
            out(b")\n");
        }
        _ => {
            decimal(&mut out, lo32(word(0)));
            out(b" ");
            decimal(&mut out, hi32(word(0)));
            out(b")\n");
        }
    }
}
