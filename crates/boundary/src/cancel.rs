//! Cleanup buffers and the unwinding of an exiting or cancelled thread (spec §22.3; D-22.2,
//! D-22.11).
//!
//! `pthread_cleanup_push` (C, glibc's headers) fills a `__pthread_unwind_buf_t` with
//! `__sigsetjmp(buf, 0)` and registers it (`__pthread_register_cancel`). When the thread exits
//! (`pthread_exit`) or acts on cancellation, the newest buffer is jumped to; the handler code
//! there runs the routine and calls `__pthread_unwind_next(buf)`, which jumps to the next
//! buffer or, after the last, runs the old-style records (`_pthread_cleanup_push`) and the
//! thread's exit sequence (`crate::thread::finish`). No Rust frame is unwound: each jump
//! leaves only this module's frames, which hold nothing to drop (S11).
//!
//! **Interim until phase 14 (D-22.11):** glibc unwinds with `_Unwind_ForcedUnwind` from
//! `libgcc_s`, which also runs C++ destructors; until Liella can load it, only the registered
//! C buffers run.

use core::sync::atomic::{AtomicUsize, Ordering::{AcqRel, Acquire, Relaxed, Release}};

use liella_kernel::errno::KResult;
use liella_abi::ctypes::c_int;
use liella_rt::thread::{CANCEL_ACTING, CANCEL_ASYNC, CANCEL_HIT};

use liella_kernel::errno::Errno;
use liella_kernel::sys::Sys;
use liella_rt::thread::{CANCEL_DISABLED, CANCEL_REQUESTED};

use crate::sys::CSys;
use crate::{ffi, raw, tb, thread};

/// `__pthread_unwind_buf_t` words: the jump buffer (0..8, mask flag at 8), then `prev` (9),
/// `cleanup` (10) and `canceltype` (11, an `int`).
const W_PREV: usize = 9;
const W_TYPE: usize = 11;
/// `_pthread_cleanup_buffer` words: routine, argument, `canceltype`, `prev`.
const O_ROUTINE: usize = 0;
const O_ARG: usize = 1;
const O_TYPE: usize = 2;
const O_PREV: usize = 3;

fn word(w: &[AtomicUsize], i: usize) -> usize {
    w.get(i).map_or(0, |x| x.load(Relaxed))
}

fn set(w: &[AtomicUsize], i: usize, v: usize) {
    if let Some(x) = w.get(i) {
        x.store(v, Relaxed);
    }
}

fn addr(w: &[AtomicUsize]) -> usize {
    w.as_ptr().expose_provenance()
}

/// `__pthread_register_cancel(buf)`.
pub fn register(buf: KResult<&[AtomicUsize]>) {
    let (Ok(b), Ok(d)) = (buf, thread::desc()) else { return };
    set(b, W_PREV, d.cleanup_jmp.load(Acquire));
    d.cleanup_jmp.store(addr(b), Release);
}

/// `__pthread_unregister_cancel(buf)`.
pub fn unregister(buf: KResult<&[AtomicUsize]>) {
    let (Ok(b), Ok(d)) = (buf, thread::desc()) else { return };
    d.cleanup_jmp.store(word(b, W_PREV), Release);
}

/// `__pthread_register_cancel_defer(buf)`: also switches to deferred cancellation, saving the
/// type in the buffer.
pub fn register_defer(buf: KResult<&[AtomicUsize]>) {
    let (Ok(b), Ok(d)) = (buf, thread::desc()) else { return };
    let old = d.cancel.fetch_and(!CANCEL_ASYNC, AcqRel);
    set(b, W_TYPE, usize::from(old & CANCEL_ASYNC != 0));
    set(b, W_PREV, d.cleanup_jmp.load(Acquire));
    d.cleanup_jmp.store(addr(b), Release);
}

/// `__pthread_unregister_cancel_restore(buf)`: restores the saved type (acting on a pending
/// request if that makes it asynchronous, D-22.11).
pub fn unregister_restore(buf: KResult<&[AtomicUsize]>) {
    let (Ok(b), Ok(d)) = (buf, thread::desc()) else { return };
    d.cleanup_jmp.store(word(b, W_PREV), Release);
    if word(b, W_TYPE) & 1 != 0 {
        d.cancel.fetch_or(CANCEL_ASYNC, AcqRel);
        test_cancel();
    }
}

/// `__pthread_unwind_next(buf)`: continue with the buffer below, or end the thread.
pub fn unwind_next(buf: KResult<&[AtomicUsize]>) -> ! {
    let next = buf.map_or(0, |b| word(b, W_PREV));
    let result = thread::desc().map_or(0, |d| {
        d.cleanup_jmp.store(next, Release);
        d.result.load(Acquire)
    });
    continue_unwind(next, result)
}

/// The thread exits with `result` (`pthread_exit`, `thrd_exit`, or acting on cancellation with
/// `PTHREAD_CANCELED`): the cleanup buffers, newest first, then the exit sequence.
pub fn unwind(result: usize) -> ! {
    let head = thread::desc().map_or(0, |d| {
        d.result.store(result, Release);
        d.cancel.fetch_or(CANCEL_ACTING, AcqRel);
        d.cleanup_jmp.load(Acquire)
    });
    continue_unwind(head, result)
}

fn continue_unwind(buf: usize, result: usize) -> ! {
    if buf != 0 {
        // The buffer was filled by `__sigsetjmp(buf, 0)` in a live frame of this thread.
        ffi::liella_machine_siglongjmp(buf, 1);
    }
    run_old_records();
    thread::finish(result)
}

/// The old-style records still registered (`_pthread_cleanup_push`), newest first.
fn run_old_records() {
    let Ok(d) = thread::desc() else { return };
    loop {
        let rec = d.cleanup_old.load(Acquire);
        if rec == 0 {
            return;
        }
        let w = tb::registered(rec, 4);
        d.cleanup_old.store(word(w, O_PREV), Release);
        call(word(w, O_ROUTINE), word(w, O_ARG));
    }
}

/// A foreign exception (C++) unwound out of the frame that pushed the newest old-style record
/// whose routine is `routine` (machine/once.S, during the unwinder's cleanup phase): the records
/// of the frames it already left (newer ones, now dead stack) are dropped, that record is
/// removed and its routine runs. Nothing happens when no such record is registered.
pub fn unwound_through(routine: usize) {
    let Ok(d) = thread::desc() else { return };
    loop {
        let rec = d.cleanup_old.load(Acquire);
        if rec == 0 {
            return;
        }
        let w = tb::registered(rec, 4);
        d.cleanup_old.store(word(w, O_PREV), Release);
        if word(w, O_ROUTINE) == routine {
            call(routine, word(w, O_ARG));
            return;
        }
    }
}

/// The type word of a guard record (machine/guard.S): `unwound_guard` stops at the newest one.
const GUARD_TAG: usize = 0x0047_5541_5244;

/// machine/guard.S (`liella_guard_begin`), before the body: `record` (the 4-word buffer in that
/// frame) is registered, so that an unwind leaving the frame, or a cancellation acting inside
/// it, runs `undo(ctx)`.
pub fn guard_begin(record: KResult<&[AtomicUsize]>, undo: usize, ctx: usize) {
    push_old(record, undo, ctx);
    if let Ok(b) = record {
        set(b, O_TYPE, GUARD_TAG);
    }
}

/// machine/guard.S (`liella_guard_end`), after the body returned: the record is removed.
pub fn guard_end(record: KResult<&[AtomicUsize]>) {
    pop_old(record, 0);
}

/// The personality routine of `liella_machine_guard`'s frame (`liella_guard_personality`; as
/// [`crate::sync::once_personality`]): in the cleanup phase it removes the newest guard record
/// (and the records of the frames the unwind already left) and runs its undo routine; the
/// unwind goes on. The search phase changes nothing.
pub fn guard_personality(version: c_int, actions: c_int) -> c_int {
    const UA_CLEANUP_PHASE: c_int = 2;
    if version != 1 {
        return if actions & UA_CLEANUP_PHASE != 0 { 2 } else { 3 };
    }
    if actions & UA_CLEANUP_PHASE != 0 {
        unwound_guard();
    }
    8
}

fn unwound_guard() {
    let Ok(d) = thread::desc() else { return };
    loop {
        let rec = d.cleanup_old.load(Acquire);
        if rec == 0 {
            return;
        }
        let w = tb::registered(rec, 4);
        d.cleanup_old.store(word(w, O_PREV), Release);
        if word(w, O_TYPE) == GUARD_TAG {
            call(word(w, O_ROUTINE), word(w, O_ARG));
            return;
        }
    }
}

fn call(f: usize, arg: usize) {
    if f != 0 {
        let _ = ffi::liella_machine_call3(f, i64::from_ne_bytes(arg.to_ne_bytes()), 0, 0);
    }
}

/// `_pthread_cleanup_push(buffer, routine, arg)`.
pub fn push_old(buf: KResult<&[AtomicUsize]>, routine: usize, arg: usize) {
    let (Ok(b), Ok(d)) = (buf, thread::desc()) else { return };
    set(b, O_ROUTINE, routine);
    set(b, O_ARG, arg);
    set(b, O_PREV, d.cleanup_old.load(Acquire));
    d.cleanup_old.store(addr(b), Release);
}

/// `_pthread_cleanup_pop(buffer, execute)`.
pub fn pop_old(buf: KResult<&[AtomicUsize]>, execute: i32) {
    let (Ok(b), Ok(d)) = (buf, thread::desc()) else { return };
    d.cleanup_old.store(word(b, O_PREV), Release);
    if execute != 0 {
        call(word(b, O_ROUTINE), word(b, O_ARG));
    }
}

/// `_pthread_cleanup_push_defer`: as [`push_old`], switching to deferred cancellation.
pub fn push_old_defer(buf: KResult<&[AtomicUsize]>, routine: usize, arg: usize) {
    let Ok(d) = thread::desc() else { return };
    let old = d.cancel.fetch_and(!CANCEL_ASYNC, AcqRel);
    if let Ok(b) = buf {
        set(b, O_TYPE, usize::from(old & CANCEL_ASYNC != 0));
    }
    push_old(buf, routine, arg);
}

/// `_pthread_cleanup_pop_restore`: as [`pop_old`], restoring the saved type.
pub fn pop_old_restore(buf: KResult<&[AtomicUsize]>, execute: i32) {
    let restore = buf.as_ref().map_or(0, |b| word(b, O_TYPE));
    pop_old(buf, execute);
    if restore & 1 != 0 {
        if let Ok(d) = thread::desc() {
            d.cancel.fetch_or(CANCEL_ASYNC, AcqRel);
        }
        test_cancel();
    }
}

/// `__pthread_cleanup_routine(frame)`: the C++/`-fexceptions` form's per-frame call (routine,
/// argument, `__do_it`, `__cancel_type`).
pub fn cleanup_routine(frame: KResult<&[AtomicUsize]>) {
    let Ok(f) = frame else { return };
    if word(f, 2) & 0xffff_ffff != 0 {
        call(word(f, 0), word(f, 1));
    }
}

/// Act on a due cancellation request now (`pthread_testcancel` and the async-cancel-safe
/// calls; filled in with the internal signal's handler).
pub fn test_cancel() {
    if let Ok(d) = thread::desc() {
        if liella_rt::thread::cancel_due(d.cancel.load(Acquire)) {
            unwind(usize::MAX);
        }
    }
}

/// A cancellation point's result (C0.2): if the call found a request due (`CANCEL_HIT`), the
/// thread acts on it now and exits with `PTHREAD_CANCELED`; otherwise the result is returned.
/// The export's own frames hold nothing to drop (S11).
///
/// `CANCEL_HIT` is only ever set by the calling thread itself (the cancellation window's exit in
/// `sys`), never by another thread, so a plain load that sees it clear is final: the atomic
/// clear (a locked read-modify-write, ~20 cycles on every read/write/close) runs only when the
/// bit is set (WL-RESID).
pub fn point<T>(v: T) -> T {
    if let Some(d) = thread::current() {
        if d.cancel.load(Acquire) & CANCEL_HIT != 0 && d.cancel.fetch_and(!CANCEL_HIT, AcqRel) & CANCEL_HIT != 0 {
            unwind(usize::MAX);
        }
    }
    v
}

/// A fast cancellation point (CANCEL-FAST, `crate::sys::point4` and `close_point`) found a
/// request due: `errno` is set from the call's raw result `r` (if it is an error), as the export's
/// result conversion did before [`point`] acted, and the thread acts now.
#[cold]
#[inline(never)]
pub(crate) fn act(r: i64) -> ! {
    if let Err(e) = Errno::decode(r) {
        crate::errno::set(e);
    }
    unwind(usize::MAX)
}

// ------------------------------------------------------------------------------ requests (§22.3)

/// The library's internal signal (D-20.1): cancellation requests, and the setxid broadcast.
pub const SIG_INTERNAL: i32 = 32;

/// The internal signal's handler. A due cancellation request interrupting a cancellation
/// point inside the window moves the saved program counter to the window's cancelled exit, so
/// the call returns `-ECANCELED` with no effect (D-22.11). Asynchronous requests take effect at
/// the thread's next safe point (owner ruling, D-22.11). Async-signal-safe: atomics and the
/// setxid service only.
extern "C" fn internal(_sig: i32, _info: usize, ctx: usize) {
    let Some(d) = thread::current() else { return };
    crate::setxid::serve(d);
    if !liella_rt::thread::cancel_due(d.cancel.load(Acquire)) {
        return;
    }
    let mut b = [0usize; 3];
    ffi::liella_machine_cancel_bounds(&mut b);
    if let Some(pc) = tb::saved_pc(ctx) {
        let at = pc.load(Relaxed);
        if b[0] <= at && at < b[1] {
            pc.store(b[2], Relaxed);
        }
    }
}

#[allow(clippy::as_conversions)] // a function pointer's address: the only spelling Rust has
fn internal_addr() -> usize {
    internal as extern "C" fn(i32, usize, usize) as usize
}

/// Install the internal signal's handler (the products' startup): every signal blocked while
/// it runs, restarting interrupted calls, on the alternate stack if one is set.
pub fn install() {
    use liella_kernel::uapi::{SigSet, SA_ONSTACK, SA_RESTART};
    let _ = raw::install_action(SIG_INTERNAL, internal_addr(), SA_RESTART | SA_ONSTACK, SigSet(!0));
}

/// `pthread_cancel`: mark the request; a thread that can act is interrupted with the internal
/// signal (its cancellation point then returns without effect). The calling thread with
/// asynchronous cancellation acts at once (`pthread_cancel` is async-cancel-safe).
pub fn request(tp: usize) -> KResult<()> {
    let d = thread::find(tp).ok_or(Errno::ESRCH)?;
    if d.has(liella_rt::thread::DESC_EXITED) {
        return Err(Errno::ESRCH);
    }
    let now = d.cancel.fetch_or(CANCEL_REQUESTED, AcqRel) | CANCEL_REQUESTED;
    if tp == tb::tp() {
        if liella_rt::thread::cancel_due(now) && now & CANCEL_ASYNC != 0 {
            unwind(usize::MAX);
        }
        return Ok(());
    }
    if liella_rt::thread::cancel_due(now) {
        if let Ok(tid) = thread::tid_of(tp) {
            let _ = CSys.tgkill(CSys.getpid(), tid, SIG_INTERNAL);
        }
    }
    Ok(())
}

/// `pthread_setcancelstate`: the previous state (`PTHREAD_CANCEL_ENABLE` 0, `DISABLE` 1).
/// Enabling with asynchronous cancellation and a request pending acts (a safe point).
pub fn set_state(state: i32) -> KResult<i32> {
    let d = thread::desc()?;
    let old = match state {
        0 => d.cancel.fetch_and(!CANCEL_DISABLED, AcqRel),
        1 => d.cancel.fetch_or(CANCEL_DISABLED, AcqRel),
        _ => return Err(Errno::EINVAL),
    };
    if state == 0 && d.cancel.load(Acquire) & CANCEL_ASYNC != 0 {
        test_cancel();
    }
    Ok(i32::from(old & CANCEL_DISABLED != 0))
}

/// `pthread_setcanceltype`: the previous type (`DEFERRED` 0, `ASYNCHRONOUS` 1). Switching to
/// asynchronous with a request pending acts (a safe point).
pub fn set_type(kind: i32) -> KResult<i32> {
    let d = thread::desc()?;
    let old = match kind {
        0 => d.cancel.fetch_and(!CANCEL_ASYNC, AcqRel),
        1 => d.cancel.fetch_or(CANCEL_ASYNC, AcqRel),
        _ => return Err(Errno::EINVAL),
    };
    if kind == 1 {
        test_cancel();
    }
    Ok(i32::from(old & CANCEL_ASYNC != 0))
}

/// `pthread_kill`: signal 0 tests for existence; the internal signal is `EINVAL`. A thread that
/// has exited but not been joined (still listed) gives 0 and is sent nothing (§20.4, 2.34+),
/// also when it exits between the lookup and `tgkill`.
pub fn kill(tp: usize, sig: i32) -> KResult<()> {
    if sig != 0 {
        liella_legacy::signal_compat::check(sig)?;
    }
    let exited = |e: Errno| if e == Errno::ESRCH && thread::find(tp).is_some() { Ok(()) } else { Err(e) };
    let tid = match thread::tid_of(tp) {
        Ok(tid) => tid,
        Err(e) => return exited(e),
    };
    if sig == 0 {
        return Ok(());
    }
    CSys.tgkill(CSys.getpid(), tid, sig).or_else(exited)
}

/// `pthread_sigqueue`: the signal with a value (`SI_QUEUE`) to one thread.
pub fn sigqueue(tp: usize, sig: i32, value: u64) -> KResult<()> {
    if sig != 0 {
        liella_legacy::signal_compat::check(sig)?;
    }
    let tid = thread::tid_of(tp)?;
    let info = liella_kernel::uapi::SigInfo::queued(sig, CSys.getpid(), CSys.getuid(), value);
    CSys.tgsigqueueinfo(CSys.getpid(), tid, sig, &info)
}
