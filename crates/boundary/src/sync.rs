//! The §22.4–§22.7 exports' calls: the synchronisation objects of `liella_rt::pthread_sync`
//! with the calling thread's context (its TID, its robust list, cancellable waits through the
//! C layer's window, D-22.11), the attribute objects, `pthread_once` with its reset on
//! cancellation, priority protection, the C11 forms, and named semaphores.

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering::{Relaxed, Release}};

use liella_abi::ctypes::c_int;
use liella_kernel::errno::{Errno, KResult};
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{
    SchedParam, Timespec, FUTEX_BITSET_MATCH_ANY, FUTEX_CLOCK_REALTIME, FUTEX_PRIVATE_FLAG, FUTEX_WAIT, FUTEX_WAIT_BITSET, MAP_SHARED, O_CLOEXEC,
    O_CREAT, O_EXCL, O_NOFOLLOW, O_RDWR, PROT_READ, PROT_WRITE, SCHED_FIFO, SCHED_RR,
};
use liella_rt::futex::{Clock, Scope};
use liella_rt::pthread_sync::{self as ps, mattr, Barrier, Cond, Ctx, LockWait, Mutex, RwLock, Sem};

use crate::sys::CSys;
use crate::{cancel, ffi, raw, tb, thread};

/// A mutex's parts (`crate::conv::mutex`).
pub type MutexParts<'a> = KResult<(&'a [AtomicU32], &'a [AtomicUsize], usize)>;

/// The calling thread, as the objects see it.
pub struct ThreadCtx;

impl Ctx for ThreadCtx {
    type S = CSys;
    fn sys(&self) -> &CSys {
        &CSys
    }
    fn tid(&self) -> u32 {
        u32::try_from(thread::own_tid()).unwrap_or(0)
    }
    #[inline]
    fn fast_tid(&self) -> Option<u32> {
        thread::fast_tid()
    }
    #[inline]
    fn alone(&self) -> bool {
        thread::lock_alone()
    }
    fn robust(&self) -> Option<&[AtomicUsize; 3]> {
        thread::desc().ok().map(|d| &d.robust)
    }
    fn links(&self, entry: usize) -> Option<&[AtomicUsize]> {
        // An entry on the calling thread's robust list: a mutex it holds, live (§22.4).
        entry.checked_sub(8).map(|a| tb::words64(a, 2))
    }
    fn wait(&self, word: &AtomicU32, expected: u32, scope: Scope, deadline: Option<(Clock, &Timespec)>, cancellable: bool) -> KResult<()> {
        let private = if scope == Scope::Private { FUTEX_PRIVATE_FLAG } else { 0 };
        let (op, ts, mask) = match deadline {
            None => (FUTEX_WAIT | private, 0usize, 0u32),
            Some((c, t)) => {
                let clk = if c == Clock::Realtime { FUTEX_CLOCK_REALTIME } else { 0 };
                (FUTEX_WAIT_BITSET | private | clk, core::ptr::from_ref(t).expose_provenance(), FUTEX_BITSET_MATCH_ANY)
            }
        };
        let d = if cancellable { thread::current() } else { None };
        let r = match d {
            // A cancellation point (D-22.11): the request is checked inside the window.
            Some(d) => {
                let r = ffi::liella_machine_syscall_cancel(
                    &d.cancel,
                    202,
                    i64::from_ne_bytes(word.as_ptr().expose_provenance().to_ne_bytes()),
                    i64::from(op),
                    i64::from(expected),
                    i64::from_ne_bytes(ts.to_ne_bytes()),
                    0,
                    i64::from(mask),
                );
                // D-22.11 (FUTEX-DUE): the found_due rule of the other points. -ECANCELED, or an
                // interruption, acts only while a request is due; a kernel's own ECANCELED with none
                // due (a seccomp filter) is a wake without a reason, which every caller re-checks.
                // Only the two codes reach the request load (the timeout path pays two compares).
                if crate::sys::found_due(&d.cancel, r) {
                    return Err(Errno::ECANCELED);
                }
                if r == -125 {
                    return Ok(());
                }
                r
            }
            None => return liella_rt::futex::wait_until(&CSys, word, expected, scope, deadline),
        };
        Errno::decode(r).map(drop)
    }
}

/// The result of a cancellation point: a due request is acted on (the thread exits with
/// `PTHREAD_CANCELED`; never returns), other results pass through.
fn acted<T>(r: KResult<T>) -> KResult<T> {
    if let Err(Errno::ECANCELED) = r {
        cancel::unwind(usize::MAX);
    }
    r
}

fn mutex_of(m: MutexParts<'_>) -> KResult<Mutex<'_>> {
    m.map(|(w, l, e)| Mutex::new(w, l, e))
}

fn word0(w: KResult<&[AtomicU32]>) -> KResult<&AtomicU32> {
    w?.first().ok_or(Errno::EINVAL)
}

// ------------------------------------------------------------------------------ mutexes

/// `pthread_mutex_init`: the attribute's type, protocol, robustness and sharing.
pub fn mutex_init(m: MutexParts<'_>, attr: KResult<Option<&[AtomicU32]>>) -> KResult<()> {
    let a = attr?.and_then(|w| w.first()).map_or(0, |x| x.load(Relaxed));
    mutex_of(m)?.init(mattr::kind(a), mattr::ceiling(a));
    Ok(())
}

/// `pthread_mutex_destroy`.
pub fn mutex_destroy(m: MutexParts<'_>) -> KResult<()> {
    mutex_of(m)?.destroy()
}

fn own_sched() -> Option<(u32, i32)> {
    let tid = thread::own_tid();
    let policy = CSys.sched_getscheduler(tid).ok()? & 0xff;
    let prio = CSys.sched_getparam(tid).ok()?.priority;
    Some((policy, prio))
}

/// After acquiring a priority-protected mutex: the thread runs at least at the ceiling
/// (§22.4), or `EINVAL` if its priority is above it (real-time policies only).
fn protect_raise(m: &Mutex<'_>) -> KResult<()> {
    let Some((policy, prio)) = own_sched() else { return Ok(()) };
    if policy != SCHED_FIFO && policy != SCHED_RR {
        return Ok(());
    }
    let ceiling = i32::try_from(m.prioceiling()?).unwrap_or(0);
    if prio > ceiling {
        return Err(Errno::EINVAL);
    }
    if prio < ceiling && CSys.sched_setparam(thread::own_tid(), &SchedParam { priority: ceiling }).is_ok() {
        m.saved().store(u32::try_from(prio).unwrap_or(0).saturating_add(1), Relaxed);
    }
    Ok(())
}

fn protect_restore(m: &Mutex<'_>) {
    let saved = m.saved().swap(0, Relaxed);
    if saved != 0 {
        let _ = CSys.sched_setparam(thread::own_tid(), &SchedParam { priority: i32::try_from(saved.saturating_sub(1)).unwrap_or(0) });
    }
}

/// `pthread_mutex_lock` and its forms.
pub fn mutex_lock(m: MutexParts<'_>, how: LockWait<'_>) -> KResult<()> {
    let m = mutex_of(m)?;
    m.lock(&ThreadCtx, how)?;
    if m.protected() {
        if let Err(e) = protect_raise(&m) {
            m.unlock(&ThreadCtx)?;
            return Err(e);
        }
    }
    Ok(())
}

/// `pthread_mutex_lock`.
// MUTEX-4: `always`: with a plain hint LLVM kept `mutex_unlock` out of line (three exports call
// it), a call plus 28 instructions on every release.
#[inline(always)]
pub fn mutex_lock_block(m: MutexParts<'_>) -> KResult<()> {
    // TINY-HOT: the uncontended plain case inlined into the export; everything else as before.
    // MUTEX-FAST: the slow path takes the parts in registers (five words), so the fast path
    // spills nothing to the stack: every store ahead of the LOCK-prefixed instruction must drain
    // before it completes, and the spilled parts cost more cycles than the instructions showed.
    let (w, l, e) = m?;
    if Mutex::new(w, l, e).lock_fast(&ThreadCtx) {
        return Ok(());
    }
    mutex_lock_slow(w, l, e, false)
}

// MUTEX-5: `cold`: the fast path's tests then stay separate predicted branches (LLVM had
// folded them into setcc/and chains).
#[cold]
#[inline(never)]
fn mutex_lock_slow(w: &[AtomicU32], l: &[AtomicUsize], e: usize, try_only: bool) -> KResult<()> {
    mutex_lock(Ok((w, l, e)), if try_only { LockWait::Try } else { LockWait::Block })
}

/// `pthread_mutex_trylock`: the uncontended plain case as `pthread_mutex_lock`'s (taking a
/// free mutex is the same either way), the rest through the general path.
#[inline]
pub fn mutex_trylock(m: MutexParts<'_>) -> KResult<()> {
    let (w, l, e) = m?;
    if Mutex::new(w, l, e).lock_fast(&ThreadCtx) {
        return Ok(());
    }
    mutex_lock_slow(w, l, e, true)
}

/// `pthread_mutex_timedlock` (`CLOCK_REALTIME`).
pub fn mutex_timedlock(m: MutexParts<'_>, at: Option<&Timespec>) -> KResult<()> {
    mutex_clocklock(m, 0, at)
}

/// `pthread_mutex_clocklock`.
pub fn mutex_clocklock(m: MutexParts<'_>, clock: c_int, at: Option<&Timespec>) -> KResult<()> {
    let c = crate::pthread::clock_of(clock)?;
    let at = at.ok_or(Errno::EINVAL)?;
    mutex_lock(m, LockWait::Until(c, at))
}

/// `pthread_mutex_unlock`.
// MUTEX-4: `always`: with a plain hint LLVM kept `mutex_unlock` out of line (three exports call
// it), a call plus 28 instructions on every release.
#[inline(always)]
pub fn mutex_unlock(m: MutexParts<'_>) -> KResult<()> {
    // TINY-HOT: the plain release inlined into the export (a plain mutex has no ceiling to
    // restore); everything else as before.
    // MUTEX-FAST: parts in registers to the slow path (see `mutex_lock_block`).
    let (w, l, e) = m?;
    if Mutex::new(w, l, e).unlock_fast(&ThreadCtx) {
        return Ok(());
    }
    mutex_unlock_slow(w, l, e)
}

// MUTEX-5: `cold`: the fast path's tests then stay separate predicted branches (LLVM had
// folded them into setcc/and chains).
#[cold]
#[inline(never)]
fn mutex_unlock_slow(w: &[AtomicU32], l: &[AtomicUsize], e: usize) -> KResult<()> {
    let m = Mutex::new(w, l, e);
    m.unlock(&ThreadCtx)?;
    if m.protected() {
        protect_restore(&m);
    }
    Ok(())
}

/// `pthread_mutex_consistent`.
pub fn mutex_consistent(m: MutexParts<'_>) -> KResult<()> {
    mutex_of(m)?.consistent(&ThreadCtx)
}

/// `pthread_mutex_getprioceiling`.
pub fn mutex_getprioceiling(m: MutexParts<'_>) -> KResult<c_int> {
    Ok(c_int::try_from(mutex_of(m)?.prioceiling()?).unwrap_or(0))
}

/// `pthread_mutex_setprioceiling`: the old ceiling.
pub fn mutex_setprioceiling(m: MutexParts<'_>, ceiling: c_int) -> KResult<c_int> {
    let c = u32::try_from(ceiling).map_err(|_| Errno::EINVAL)?;
    Ok(c_int::try_from(mutex_of(m)?.set_prioceiling(&ThreadCtx, c)?).unwrap_or(0))
}

// ------------------------------------------------------------------------------ mutex attributes

fn attr_word(w: KResult<&[AtomicU32]>) -> KResult<&AtomicU32> {
    word0(w)
}

fn attr_update(w: KResult<&[AtomicU32]>, mask: u32, value: u32) -> KResult<()> {
    let a = attr_word(w)?;
    let v = a.load(Relaxed);
    a.store((v & !mask) | (value & mask), Relaxed);
    Ok(())
}

/// `pthread_mutexattr_init`: a normal, private, non-robust mutex without a protocol.
pub fn mutexattr_init(w: KResult<&[AtomicU32]>) -> KResult<()> {
    attr_word(w)?.store(0, Relaxed);
    Ok(())
}

/// `pthread_mutexattr_destroy`.
pub fn mutexattr_destroy(w: KResult<&[AtomicU32]>) -> KResult<()> {
    attr_word(w).map(drop)
}

/// `pthread_mutexattr_settype`: the four types (`EINVAL` otherwise).
pub fn mutexattr_settype(w: KResult<&[AtomicU32]>, t: c_int) -> KResult<()> {
    let t = u32::try_from(t).ok().filter(|&t| t <= ps::MUTEX_ADAPTIVE).ok_or(Errno::EINVAL)?;
    attr_update(w, mattr::KIND_MASK, t)
}

/// `pthread_mutexattr_gettype`.
pub fn mutexattr_gettype(w: KResult<&[AtomicU32]>) -> KResult<c_int> {
    Ok(c_int::try_from(attr_word(w)?.load(Relaxed) & mattr::KIND_MASK).unwrap_or(0))
}

/// `pthread_mutexattr_setpshared`.
pub fn mutexattr_setpshared(w: KResult<&[AtomicU32]>, v: c_int) -> KResult<()> {
    attr_update(w, mattr::PSHARED, if shared(v)? { mattr::PSHARED } else { 0 })
}

/// `pthread_mutexattr_getpshared`.
pub fn mutexattr_getpshared(w: KResult<&[AtomicU32]>) -> KResult<c_int> {
    Ok(c_int::from(attr_word(w)?.load(Relaxed) & mattr::PSHARED != 0))
}

/// `pthread_mutexattr_setprotocol`: NONE, INHERIT or PROTECT.
pub fn mutexattr_setprotocol(w: KResult<&[AtomicU32]>, p: c_int) -> KResult<()> {
    let p = u32::try_from(p).ok().filter(|&p| p <= mattr::PRIO_PROTECT).ok_or(Errno::EINVAL)?;
    attr_update(w, mattr::PROTOCOL_MASK, p << mattr::PROTOCOL_SHIFT)
}

/// `pthread_mutexattr_getprotocol`.
pub fn mutexattr_getprotocol(w: KResult<&[AtomicU32]>) -> KResult<c_int> {
    Ok(c_int::try_from((attr_word(w)?.load(Relaxed) & mattr::PROTOCOL_MASK) >> mattr::PROTOCOL_SHIFT).unwrap_or(0))
}

/// `pthread_mutexattr_setprioceiling`: within `SCHED_FIFO`'s range.
pub fn mutexattr_setprioceiling(w: KResult<&[AtomicU32]>, c: c_int) -> KResult<()> {
    let (lo, hi) = (CSys.sched_get_priority_min(SCHED_FIFO)?, CSys.sched_get_priority_max(SCHED_FIFO)?);
    if c < lo || c > hi {
        return Err(Errno::EINVAL);
    }
    attr_update(w, mattr::CEILING_MASK, u32::try_from(c).unwrap_or(0) << mattr::CEILING_SHIFT)
}

/// `pthread_mutexattr_getprioceiling`.
pub fn mutexattr_getprioceiling(w: KResult<&[AtomicU32]>) -> KResult<c_int> {
    Ok(c_int::try_from(mattr::ceiling(attr_word(w)?.load(Relaxed))).unwrap_or(0))
}

/// `pthread_mutexattr_setrobust`: STALLED (0) or ROBUST (1).
pub fn mutexattr_setrobust(w: KResult<&[AtomicU32]>, r: c_int) -> KResult<()> {
    match r {
        0 => attr_update(w, mattr::ROBUST, 0),
        1 => attr_update(w, mattr::ROBUST, mattr::ROBUST),
        _ => Err(Errno::EINVAL),
    }
}

/// `pthread_mutexattr_getrobust`.
pub fn mutexattr_getrobust(w: KResult<&[AtomicU32]>) -> KResult<c_int> {
    Ok(c_int::from(attr_word(w)?.load(Relaxed) & mattr::ROBUST != 0))
}

fn shared(v: c_int) -> KResult<bool> {
    match v {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(Errno::EINVAL),
    }
}

// ------------------------------------------------------------------------------ condition variables

/// `pthread_condattr_t` (an `int`): bit 0 `CLOCK_MONOTONIC`, bit 1 shared (the condition
/// variable's own flags).
pub fn condattr_init(w: KResult<&[AtomicU32]>) -> KResult<()> {
    attr_word(w)?.store(0, Relaxed);
    Ok(())
}

/// `pthread_condattr_destroy`.
pub fn condattr_destroy(w: KResult<&[AtomicU32]>) -> KResult<()> {
    attr_word(w).map(drop)
}

/// `pthread_condattr_setclock`: `CLOCK_REALTIME` or `CLOCK_MONOTONIC`.
pub fn condattr_setclock(w: KResult<&[AtomicU32]>, clock: c_int) -> KResult<()> {
    let mono = crate::pthread::clock_of(clock)? == Clock::Monotonic;
    attr_update(w, ps::COND_MONOTONIC, if mono { ps::COND_MONOTONIC } else { 0 })
}

/// `pthread_condattr_getclock`.
pub fn condattr_getclock(w: KResult<&[AtomicU32]>) -> KResult<c_int> {
    Ok(c_int::from(attr_word(w)?.load(Relaxed) & ps::COND_MONOTONIC != 0))
}

/// `pthread_condattr_setpshared`.
pub fn condattr_setpshared(w: KResult<&[AtomicU32]>, v: c_int) -> KResult<()> {
    attr_update(w, ps::COND_SHARED, if shared(v)? { ps::COND_SHARED } else { 0 })
}

/// `pthread_condattr_getpshared`.
pub fn condattr_getpshared(w: KResult<&[AtomicU32]>) -> KResult<c_int> {
    Ok(c_int::from(attr_word(w)?.load(Relaxed) & ps::COND_SHARED != 0))
}

/// `pthread_cond_init`.
pub fn cond_init(c: KResult<&[AtomicU32]>, attr: KResult<Option<&[AtomicU32]>>) -> KResult<()> {
    let flags = attr?.and_then(|w| w.first()).map_or(0, |x| x.load(Relaxed));
    Cond::new(c?).init(flags & (ps::COND_MONOTONIC | ps::COND_SHARED));
    Ok(())
}

/// `pthread_cond_destroy`.
pub fn cond_destroy(c: KResult<&[AtomicU32]>) -> KResult<()> {
    c.map(drop)
}

/// `pthread_cond_wait` and its timed forms (cancellation points).
pub fn cond_wait(c: KResult<&[AtomicU32]>, m: MutexParts<'_>, clock: Option<c_int>, at: Option<&Timespec>, timed: bool) -> KResult<()> {
    let c = Cond::new(c?);
    let m = mutex_of(m)?;
    let deadline = if timed {
        let at = at.ok_or(Errno::EINVAL)?;
        let clk = match clock {
            Some(k) => crate::pthread::clock_of(k)?,
            None => c.clock(),
        };
        Some((clk, at))
    } else {
        None
    };
    acted(c.wait(&ThreadCtx, &m, deadline))
}

/// `pthread_cond_signal`.
pub fn cond_signal(c: KResult<&[AtomicU32]>) -> KResult<()> {
    Cond::new(c?).signal(&ThreadCtx);
    Ok(())
}

/// `pthread_cond_broadcast`.
pub fn cond_broadcast(c: KResult<&[AtomicU32]>) -> KResult<()> {
    Cond::new(c?).broadcast(&ThreadCtx);
    Ok(())
}

// ------------------------------------------------------------------------------ old condition variables (LG-3)
// `pthread_cond_*@GLIBC_2.2.5`, the pre-2.3.2 ABI (s22_5_c_1_2). Probed (tools/probe/compat/
// compat_condvar.c, spec/behaviour/compat.glibc.txt): the old object is ONE 8-byte word. `init`
// stores zero in it (also when it then refuses a process-shared or CLOCK_MONOTONIC attribute with
// EINVAL), and zero is the static initializer; the first signal, broadcast or wait fills it in.
// Liella's condition variable needs OLD_WORDS words, more than the word holds, so the word holds
// the address of a heap block: Liella's condition variable (all-zero = default, CLOCK_REALTIME,
// private) plus a reference word. This is the one sync object that holds a pointer (an exception
// to D-22.1, which the old ABI forces: it is never process-shared). The reference word counts the
// waiters inside the block, so a destroy right after a broadcast (allowed by POSIX) cannot free the
// block under a woken waiter that has not yet returned; the last one out frees it.

/// Words of Liella's condition variable in an old block (`pthread_cond_t`'s size).
const OLD_WORDS: usize = size_of::<liella_abi::layout::pthread_cond_t>() / 4;
/// The reference word (the block's last): waiters inside it, plus this bit once destroyed.
const OLD_DEAD: u32 = 1 << 31;

fn old_view<'a>(p: usize) -> KResult<(&'a AtomicU32, &'a [AtomicU32])> {
    crate::conv::old_cond_block(p, OLD_WORDS + 1)
}

/// The old object's one word.
fn old_word(c: KResult<&[AtomicUsize]>) -> KResult<&AtomicUsize> {
    c?.first().ok_or(Errno::EINVAL)
}

/// Where old blocks come from: the heap's `calloc`/`free` in the products; the unit tests pass a
/// failing or counting pair (plain function pointers the compiler resolves: no cost).
#[derive(Clone, Copy)]
struct OldHeap {
    /// A zeroed block of the given bytes, or `ENOMEM`.
    alloc: fn(usize) -> KResult<usize>,
    /// Gives a block back.
    free: fn(usize),
}

const OLD_HEAP: OldHeap = OldHeap {
    alloc: |n| crate::heap::c_calloc(1, n),
    free: |p| {
        let _ = crate::heap::c_free(p);
    },
};

/// The block the old word names, installed on first use (two first users race with a
/// compare-exchange; the loser frees its block). Allocation failure returns `ENOMEM` with the
/// word still zero, as glibc's old entries (probe: `rc=12 ... word zero`); nothing half-built
/// is ever published, because the block is complete (zeroed) before the compare-exchange.
fn old_block(word: &AtomicUsize, h: OldHeap) -> KResult<usize> {
    use core::sync::atomic::Ordering::{AcqRel, Acquire};
    let p = word.load(Acquire);
    if p != 0 {
        return Ok(p);
    }
    let fresh = (h.alloc)((OLD_WORDS + 1) * 4)?;
    match word.compare_exchange(0, fresh, AcqRel, Acquire) {
        Ok(_) => Ok(fresh),
        Err(won) => {
            (h.free)(fresh);
            Ok(won)
        }
    }
}

/// Detach the word's block and free it unless a waiter is inside (then the last one out does).
fn old_destroy(word: &AtomicUsize, h: OldHeap) -> KResult<()> {
    let p = word.swap(0, core::sync::atomic::Ordering::AcqRel);
    if p != 0 && old_view(p)?.0.fetch_or(OLD_DEAD, core::sync::atomic::Ordering::AcqRel) == 0 {
        (h.free)(p);
    }
    Ok(())
}

/// A waiter leaves block `p`: the last one out of a destroyed block frees it.
fn old_leave(refs: &AtomicU32, p: usize, h: OldHeap) {
    if refs.fetch_sub(1, core::sync::atomic::Ordering::AcqRel) == OLD_DEAD | 1 {
        (h.free)(p);
    }
}

/// `pthread_cond_init@GLIBC_2.2.5`: zero the word; only the default attribute is accepted.
pub fn old_cond_init(c: KResult<&[AtomicUsize]>, attr: KResult<Option<&[AtomicU32]>>) -> KResult<()> {
    old_word(c)?.store(0, Release);
    let flags = attr?.and_then(|w| w.first()).map_or(0, |x| x.load(Relaxed));
    if flags & (ps::COND_MONOTONIC | ps::COND_SHARED) != 0 {
        return Err(Errno::EINVAL);
    }
    Ok(())
}

/// `pthread_cond_destroy@GLIBC_2.2.5`: detach the block (the word is zero again, a fresh
/// condition variable) and free it unless a waiter is still inside (then the last one frees it).
pub fn old_cond_destroy(c: KResult<&[AtomicUsize]>) -> KResult<()> {
    old_destroy(old_word(c)?, OLD_HEAP)
}

/// `pthread_cond_wait@GLIBC_2.2.5`/`timedwait@GLIBC_2.2.5` (cancellation points; CLOCK_REALTIME).
/// A failed first-use allocation returns `ENOMEM` before the mutex is touched.
pub fn old_cond_wait(c: KResult<&[AtomicUsize]>, m: MutexParts<'_>, at: Option<&Timespec>, timed: bool) -> KResult<()> {
    let word = old_word(c)?;
    let m = mutex_of(m)?;
    let at = if timed { Some(at.ok_or(Errno::EINVAL)?) } else { None };
    let p = old_block(word, OLD_HEAP)?;
    let (refs, cv) = old_view(p)?;
    refs.fetch_add(1, core::sync::atomic::Ordering::AcqRel);
    let r = Cond::new(cv).wait(&ThreadCtx, &m, at.map(|t| (Clock::Realtime, t)));
    // Leave the block before acting on a cancellation (`acted` does not return then).
    old_leave(refs, p, OLD_HEAP);
    acted(r)
}

/// `pthread_cond_signal@GLIBC_2.2.5` (`all`: `broadcast@GLIBC_2.2.5`).
pub fn old_cond_signal(c: KResult<&[AtomicUsize]>, all: bool) -> KResult<()> {
    old_signal(old_word(c)?, all, OLD_HEAP)
}

fn old_signal(word: &AtomicUsize, all: bool, h: OldHeap) -> KResult<()> {
    let k = Cond::new(old_view(old_block(word, h)?)?.1);
    if all { k.broadcast(&ThreadCtx) } else { k.signal(&ThreadCtx) }
    Ok(())
}

// ------------------------------------------------------------------------------ read-write locks

/// `pthread_rwlockattr_t` (two `int`s): the kind and the sharing.
pub fn rwlockattr_init(w: KResult<&[AtomicU32]>) -> KResult<()> {
    let w = w?;
    for x in w {
        x.store(0, Relaxed);
    }
    Ok(())
}

/// `pthread_rwlockattr_destroy`.
pub fn rwlockattr_destroy(w: KResult<&[AtomicU32]>) -> KResult<()> {
    w.map(drop)
}

/// `pthread_rwlockattr_setkind_np`: the three preferences.
pub fn rwlockattr_setkind(w: KResult<&[AtomicU32]>, k: c_int) -> KResult<()> {
    let k = u32::try_from(k).ok().filter(|&k| k <= ps::RW_PREFER_WRITER_NONRECURSIVE).ok_or(Errno::EINVAL)?;
    attr_word(w)?.store(k, Relaxed);
    Ok(())
}

/// `pthread_rwlockattr_getkind_np`.
pub fn rwlockattr_getkind(w: KResult<&[AtomicU32]>) -> KResult<c_int> {
    Ok(c_int::try_from(attr_word(w)?.load(Relaxed)).unwrap_or(0))
}

/// `pthread_rwlockattr_setpshared`.
pub fn rwlockattr_setpshared(w: KResult<&[AtomicU32]>, v: c_int) -> KResult<()> {
    w?.get(1).ok_or(Errno::EINVAL)?.store(u32::from(shared(v)?), Relaxed);
    Ok(())
}

/// `pthread_rwlockattr_getpshared`.
pub fn rwlockattr_getpshared(w: KResult<&[AtomicU32]>) -> KResult<c_int> {
    Ok(c_int::try_from(w?.get(1).ok_or(Errno::EINVAL)?.load(Relaxed)).unwrap_or(0))
}

/// `pthread_rwlock_init`.
pub fn rwlock_init(l: KResult<&[AtomicU32]>, attr: KResult<Option<&[AtomicU32]>>) -> KResult<()> {
    let a = attr?;
    let kind = a.and_then(|w| w.first()).map_or(0, |x| x.load(Relaxed));
    let sh = a.and_then(|w| w.get(1)).is_some_and(|x| x.load(Relaxed) != 0);
    RwLock::new(l?).init(kind, sh);
    Ok(())
}

/// `pthread_rwlock_destroy`.
pub fn rwlock_destroy(l: KResult<&[AtomicU32]>) -> KResult<()> {
    RwLock::new(l?).destroy()
}

fn wait_of<'t>(clock: Option<c_int>, at: Option<&'t Timespec>) -> KResult<LockWait<'t>> {
    let at = at.ok_or(Errno::EINVAL)?;
    Ok(LockWait::Until(crate::pthread::clock_of(clock.unwrap_or(0))?, at))
}

/// `pthread_rwlock_rdlock`.
pub fn rwlock_rdlock(l: KResult<&[AtomicU32]>) -> KResult<()> {
    RwLock::new(l?).read(&ThreadCtx, LockWait::Block)
}

/// `pthread_rwlock_tryrdlock`.
pub fn rwlock_tryrdlock(l: KResult<&[AtomicU32]>) -> KResult<()> {
    RwLock::new(l?).read(&ThreadCtx, LockWait::Try)
}

/// `pthread_rwlock_timedrdlock`/`clockrdlock`.
pub fn rwlock_timedrdlock(l: KResult<&[AtomicU32]>, clock: Option<c_int>, at: Option<&Timespec>) -> KResult<()> {
    RwLock::new(l?).read(&ThreadCtx, wait_of(clock, at)?)
}

/// `pthread_rwlock_wrlock`.
pub fn rwlock_wrlock(l: KResult<&[AtomicU32]>) -> KResult<()> {
    RwLock::new(l?).write(&ThreadCtx, LockWait::Block)
}

/// `pthread_rwlock_trywrlock`.
pub fn rwlock_trywrlock(l: KResult<&[AtomicU32]>) -> KResult<()> {
    RwLock::new(l?).write(&ThreadCtx, LockWait::Try)
}

/// `pthread_rwlock_timedwrlock`/`clockwrlock`.
pub fn rwlock_timedwrlock(l: KResult<&[AtomicU32]>, clock: Option<c_int>, at: Option<&Timespec>) -> KResult<()> {
    RwLock::new(l?).write(&ThreadCtx, wait_of(clock, at)?)
}

/// `pthread_rwlock_unlock`.
pub fn rwlock_unlock(l: KResult<&[AtomicU32]>) -> KResult<()> {
    RwLock::new(l?).unlock(&ThreadCtx)
}

// ------------------------------------------------------------------------------ spin locks, barriers

/// `pthread_spin_init`: the sharing is accepted (a spin lock needs none).
pub fn spin_init(l: KResult<&[AtomicU32]>, pshared: c_int) -> KResult<()> {
    shared(pshared)?;
    word0(l)?.store(0, Release);
    Ok(())
}

/// `pthread_spin_destroy`.
pub fn spin_destroy(l: KResult<&[AtomicU32]>) -> KResult<()> {
    word0(l).map(drop)
}

/// `pthread_spin_lock`.
pub fn spin_lock(l: KResult<&[AtomicU32]>) -> KResult<()> {
    ps::spin_lock(word0(l)?);
    Ok(())
}

/// `pthread_spin_trylock`.
pub fn spin_trylock(l: KResult<&[AtomicU32]>) -> KResult<()> {
    ps::spin_trylock(word0(l)?)
}

/// `pthread_spin_unlock`.
pub fn spin_unlock(l: KResult<&[AtomicU32]>) -> KResult<()> {
    ps::spin_unlock(word0(l)?);
    Ok(())
}

/// `pthread_barrierattr_init`/`destroy`/`setpshared`/`getpshared` (an `int`: shared).
pub fn barrierattr_init(w: KResult<&[AtomicU32]>) -> KResult<()> {
    attr_word(w)?.store(0, Relaxed);
    Ok(())
}

/// `pthread_barrierattr_destroy`.
pub fn barrierattr_destroy(w: KResult<&[AtomicU32]>) -> KResult<()> {
    attr_word(w).map(drop)
}

/// `pthread_barrierattr_setpshared`.
pub fn barrierattr_setpshared(w: KResult<&[AtomicU32]>, v: c_int) -> KResult<()> {
    attr_word(w)?.store(u32::from(shared(v)?), Relaxed);
    Ok(())
}

/// `pthread_barrierattr_getpshared`.
pub fn barrierattr_getpshared(w: KResult<&[AtomicU32]>) -> KResult<c_int> {
    Ok(c_int::try_from(attr_word(w)?.load(Relaxed)).unwrap_or(0))
}

/// `pthread_barrier_init`.
pub fn barrier_init(b: KResult<&[AtomicU32]>, attr: KResult<Option<&[AtomicU32]>>, count: u32) -> KResult<()> {
    let sh = attr?.and_then(|w| w.first()).is_some_and(|x| x.load(Relaxed) != 0);
    Barrier::new(b?).init(count, sh)
}

/// `pthread_barrier_destroy`.
pub fn barrier_destroy(b: KResult<&[AtomicU32]>) -> KResult<()> {
    b.map(drop)
}

/// `pthread_barrier_wait`: `PTHREAD_BARRIER_SERIAL_THREAD`, 0, or an error number.
pub fn barrier_wait(b: KResult<&[AtomicU32]>) -> c_int {
    match b {
        Ok(w) => Barrier::new(w).wait(&ThreadCtx),
        Err(e) => e.get(),
    }
}

// ------------------------------------------------------------------------------ once

extern "C" fn once_cancelled(word: usize) {
    // The record's argument: the once object's word (live: the caller's object).
    if let Some(w) = tb::words32_of(word) {
        ps::once_reset(&CSys, w);
    }
}

#[allow(clippy::as_conversions)] // a function pointer's address: the only spelling Rust has
fn once_cancelled_addr() -> usize {
    once_cancelled as extern "C" fn(usize) as usize
}

/// `pthread_once`/`call_once`: `init` runs once. The routine runs in the frame of
/// `liella_machine_once` (machine/once.S), so that a C++ exception unwinds through no Rust frame
/// (S11): if it is cancelled or an exception leaves it, the object is reset (§22.5) and a later
/// call runs it again.
pub fn once(w: KResult<&[AtomicU32]>, init: usize) -> KResult<()> {
    let word = word0(w)?;
    if init == 0 {
        return Err(Errno::EINVAL);
    }
    match ffi::liella_machine_once(word.as_ptr().expose_provenance(), init) {
        0 => Ok(()),
        e => Err(Errno::new(e).unwrap_or(Errno::EINVAL)),
    }
}

/// machine/once.S (`liella_once_begin`), before the routine: -1 when the calling thread now runs
/// `init` (`record`, the 4-word buffer in that frame, is registered so that cancellation resets
/// the object), else the result (0 when the routine completed, or an error number).
pub fn once_begin(w: KResult<&[AtomicU32]>, init: usize, record: KResult<&[AtomicUsize]>) -> c_int {
    let word = match word0(w) {
        Ok(w) => w,
        Err(e) => return e.get(),
    };
    if init == 0 {
        return Errno::EINVAL.get();
    }
    if !ps::once_begin(&CSys, word) {
        return 0;
    }
    cancel::push_old(record, once_cancelled_addr(), word.as_ptr().expose_provenance());
    -1
}

/// machine/once.S (`liella_once_end`), after the routine returned: the record is removed, the
/// object done.
pub fn once_end(w: KResult<&[AtomicU32]>, record: KResult<&[AtomicUsize]>) {
    cancel::pop_old(record, 0);
    if let Ok(word) = word0(w) {
        ps::once_end(&CSys, word);
    }
}

/// The personality routine of `liella_machine_once`'s frame (`liella_once_personality`; Itanium
/// C++ ABI: version 1; `_UA_CLEANUP_PHASE` = 2; `_URC_CONTINUE_UNWIND` = 8,
/// `_URC_FATAL_PHASE1_ERROR` = 3, `_URC_FATAL_PHASE2_ERROR` = 2). It never handles an exception
/// and installs no landing pad: in the cleanup phase (an exception thrown by the routine, or a
/// forced unwind) it resets the object the frame runs, through that frame's cancellation record,
/// and lets the unwind go on. The search phase changes nothing.
pub fn once_personality(version: c_int, actions: c_int) -> c_int {
    const UA_CLEANUP_PHASE: c_int = 2;
    if version != 1 {
        return if actions & UA_CLEANUP_PHASE != 0 { 2 } else { 3 };
    }
    if actions & UA_CLEANUP_PHASE != 0 {
        cancel::unwound_through(once_cancelled_addr());
    }
    8
}

// ------------------------------------------------------------------------------ semaphores (errno convention)

/// `sem_init`: `EINVAL` above `SEM_VALUE_MAX`.
pub fn sem_init(s: KResult<&[AtomicU32]>, pshared: c_int, value: u32) -> KResult<()> {
    Sem::new(s?).init(value, pshared != 0)
}

/// `sem_destroy`.
pub fn sem_destroy(s: KResult<&[AtomicU32]>) -> KResult<()> {
    s.map(drop)
}

/// `sem_post` (async-signal-safe).
pub fn sem_post(s: KResult<&[AtomicU32]>) -> KResult<()> {
    Sem::new(s?).post(&CSys)
}

/// `sem_wait` (a cancellation point).
pub fn sem_wait(s: KResult<&[AtomicU32]>) -> KResult<()> {
    acted(Sem::new(s?).wait(&ThreadCtx, LockWait::Block))
}

/// `sem_trywait`.
pub fn sem_trywait(s: KResult<&[AtomicU32]>) -> KResult<()> {
    Sem::new(s?).wait(&ThreadCtx, LockWait::Try)
}

/// `sem_timedwait`/`sem_clockwait` (cancellation points).
pub fn sem_timedwait(s: KResult<&[AtomicU32]>, clock: Option<c_int>, at: Option<&Timespec>) -> KResult<()> {
    acted(Sem::new(s?).wait(&ThreadCtx, wait_of(clock, at)?))
}

/// `sem_getvalue`.
pub fn sem_getvalue(s: KResult<&[AtomicU32]>) -> KResult<c_int> {
    Ok(c_int::try_from(Sem::new(s?).value()).unwrap_or(c_int::MAX))
}

// Named semaphores (§22.7): `/dev/shm/sem.<name>`, mapped shared; one mapping per name in the
// process (opening it twice gives the same address), counted.
const SEM_SIZE: usize = 32;
const NAMED_MAX: usize = 256;
static NAMED_LOCK: liella_rt::sync::RawMutex = liella_rt::sync::RawMutex::new();
/// Per open name: (device, inode) as one key, the address, the open count.
static NAMED: [[AtomicUsize; 4]; NAMED_MAX] = [const { [const { AtomicUsize::new(0) }; 4] }; NAMED_MAX];

/// `/dev/shm/sem.<name>` for `name` (one leading slash dropped): `EINVAL` for an empty name or
/// one with another slash, `ENAMETOOLONG` beyond `NAME_MAX`.
fn sem_path(name: &[u8]) -> KResult<[u8; 300]> {
    let n = name.strip_prefix(b"/").unwrap_or(name);
    if n.is_empty() || n.contains(&b'/') || n == b"." || n == b".." {
        return Err(Errno::EINVAL);
    }
    if n.len() > 251 {
        return Err(Errno::ENAMETOOLONG);
    }
    let mut out = [0u8; 300];
    let prefix = b"/dev/shm/sem.";
    for (o, &b) in out.iter_mut().zip(prefix.iter().chain(n)) {
        *o = b;
    }
    Ok(out)
}

fn kstr(p: &[u8]) -> KResult<KStr<'_>> {
    let end = p.iter().position(|&b| b == 0).ok_or(Errno::EINVAL)?;
    KStr::from_nul_terminated(p.get(..=end).ok_or(Errno::EINVAL)?)
}

/// `sem_open(name, oflag, mode, value)`: the semaphore's address.
pub fn sem_open(name: KResult<&[u8]>, oflag: c_int, mode: u32, value: u32) -> KResult<usize> {
    let path = sem_path(name?)?;
    let oflag = u32::from_ne_bytes(oflag.to_ne_bytes());
    let create = oflag & O_CREAT != 0;
    if create && value > ps::SEM_VALUE_MAX {
        return Err(Errno::EINVAL);
    }
    let mut created = false;
    let fd = loop {
        match CSys.openat(Fd::CWD, kstr(&path)?, O_RDWR | O_NOFOLLOW | O_CLOEXEC, 0) {
            Ok(fd) if create && oflag & O_EXCL != 0 && !created => {
                let _ = CSys.close(fd);
                return Err(Errno::EEXIST);
            }
            Ok(fd) => break fd,
            Err(Errno::ENOENT) if create => match sem_create(&path, mode, value) {
                Ok(()) => {
                    created = true;
                    continue;
                }
                Err(Errno::EEXIST) if oflag & O_EXCL == 0 => continue,
                Err(e) => return Err(e),
            },
            Err(e) => return Err(e),
        }
    };
    let r = sem_map(fd);
    let _ = CSys.close(fd);
    r
}

/// Create the file atomically: a temporary file with the initialised semaphore, linked to the
/// final name (`EEXIST` if another process won).
fn sem_create(path: &[u8; 300], mode: u32, value: u32) -> KResult<()> {
    let mut tmp = [0u8; 40];
    let mut rnd = [0u8; 6];
    let _ = CSys.getrandom(&mut rnd, 0);
    const LETTERS: &[u8; 32] = b"abcdefghijklmnopqrstuvwxyz012345";
    let random = rnd.map(|b| LETTERS.get(usize::from(b % 32)).copied().unwrap_or(b'x'));
    for (o, &b) in tmp.iter_mut().zip(b"/dev/shm/sem.liella-".iter().chain(&random)) {
        *o = b;
    }
    let fd = CSys.openat(Fd::CWD, kstr(&tmp)?, O_RDWR | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, mode & 0o7777)?;
    let mut init = [0u8; SEM_SIZE];
    for (o, b) in init.iter_mut().zip(value.to_ne_bytes()) {
        *o = b;
    }
    // Word 2: shared (a named semaphore always is).
    if let Some(o) = init.get_mut(8) {
        *o = 1;
    }
    let w = CSys.write(fd, &init);
    let _ = CSys.close(fd);
    let r = w.and_then(|n| if n == SEM_SIZE { Ok(()) } else { Err(Errno::EIO) }).and_then(|()| CSys.linkat(Fd::CWD, kstr(&tmp)?, Fd::CWD, kstr(path)?, 0));
    let _ = CSys.unlinkat(Fd::CWD, kstr(&tmp)?, 0);
    r
}

fn sem_map(fd: Fd) -> KResult<usize> {
    let st = CSys.fstat(fd)?;
    let key = usize::try_from(st.dev).unwrap_or(0).rotate_left(32) ^ usize::try_from(st.ino).unwrap_or(0);
    let _g = NAMED_LOCK.lock(&CSys);
    if let Some(e) = NAMED.iter().find(|e| e[3].load(Relaxed) != 0 && e[0].load(Relaxed) == key) {
        e[3].fetch_add(1, Relaxed);
        return Ok(e[1].load(Relaxed));
    }
    let slot = NAMED.iter().find(|e| e[3].load(Relaxed) == 0).ok_or(Errno::EMFILE)?;
    let prot = i32::try_from(PROT_READ | PROT_WRITE).unwrap_or(3);
    let flags = i32::try_from(MAP_SHARED).unwrap_or(1);
    let addr = raw::mmap(0, SEM_SIZE, prot, flags, fd.raw(), 0)?;
    slot[0].store(key, Relaxed);
    slot[1].store(addr, Relaxed);
    slot[3].store(1, Relaxed);
    Ok(addr)
}

/// `sem_close`: the mapping goes when the last open of the name in this process closes.
pub fn sem_close(addr: usize) -> KResult<()> {
    let _g = NAMED_LOCK.lock(&CSys);
    let e = NAMED.iter().find(|e| e[3].load(Relaxed) != 0 && e[1].load(Relaxed) == addr).ok_or(Errno::EINVAL)?;
    if e[3].fetch_sub(1, Relaxed) == 1 {
        let _ = raw::munmap(addr, SEM_SIZE);
        e[0].store(0, Relaxed);
        e[1].store(0, Relaxed);
    }
    Ok(())
}

/// `sem_unlink`.
pub fn sem_unlink(name: KResult<&[u8]>) -> KResult<()> {
    let path = sem_path(name?)?;
    CSys.unlinkat(Fd::CWD, kstr(&path)?, 0).map_err(|e| if e == Errno::EPERM { Errno::EACCES } else { e })
}

// ------------------------------------------------------------------------------ C11

/// `mtx_init`: `mtx_plain` (0), `mtx_recursive` (1), `mtx_timed` (2) and their combinations.
pub fn mtx_init(m: MutexParts<'_>, kind: c_int) -> c_int {
    let r = mutex_of(m).and_then(|m| {
        if !(0..=3).contains(&kind) {
            return Err(Errno::EINVAL);
        }
        m.init(if kind & 1 != 0 { ps::MUTEX_RECURSIVE } else { ps::MUTEX_NORMAL }, 0);
        Ok(())
    });
    liella_legacy::pthread_compat::c11_code(r)
}

/// A C11 result code for a lock-family call.
pub fn c11<T>(r: KResult<T>) -> c_int {
    liella_legacy::pthread_compat::c11_code(r.map(drop))
}

/// `mtx_destroy`.
pub fn mtx_destroy(_m: MutexParts<'_>) {}

/// `cnd_destroy`.
pub fn cnd_destroy(_c: KResult<&[AtomicU32]>) {}

/// `call_once`.
pub fn call_once(w: KResult<&[AtomicU32]>, f: usize) {
    let _ = once(w, f);
}

/// `cnd_wait`/`cnd_timedwait` (C11: `CLOCK_REALTIME`).
pub fn cnd_wait(c: KResult<&[AtomicU32]>, m: MutexParts<'_>, at: Option<&Timespec>, timed: bool) -> c_int {
    c11(cond_wait(c, m, Some(0), at, timed))
}

/// The old condition variables' heap conditions (ruling #1153): a failing allocator gives
/// `ENOMEM` and leaves the word zero; destroy frees the block; a waiter still inside keeps it
/// until it leaves. Waits themselves need the products (D-22.10).
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod old_cond_tests {
    use super::*;
    use core::sync::atomic::AtomicIsize;

    fn failing(_: usize) -> KResult<usize> {
        Err(Errno::ENOMEM)
    }
    fn no_free(_: usize) {
        panic!("nothing to free after a failed allocation");
    }
    const FAILING: OldHeap = OldHeap { alloc: failing, free: no_free };

    /// Blocks live: allocated minus freed (one counter per test, they run in parallel).
    static LIVE_CYCLE: AtomicIsize = AtomicIsize::new(0);
    static LIVE_WAITER: AtomicIsize = AtomicIsize::new(0);
    const CYCLE: OldHeap = OldHeap {
        alloc: |n| (OLD_HEAP.alloc)(n).inspect(|_| { LIVE_CYCLE.fetch_add(1, Relaxed); }),
        free: |p| { LIVE_CYCLE.fetch_sub(1, Relaxed); (OLD_HEAP.free)(p) },
    };
    const WAITER: OldHeap = OldHeap {
        alloc: |n| (OLD_HEAP.alloc)(n).inspect(|_| { LIVE_WAITER.fetch_add(1, Relaxed); }),
        free: |p| { LIVE_WAITER.fetch_sub(1, Relaxed); (OLD_HEAP.free)(p) },
    };

    #[test]
    fn old_cond_enomem_leaves_word_zero() {
        let w = AtomicUsize::new(0);
        assert_eq!(old_block(&w, FAILING), Err(Errno::ENOMEM));
        assert_eq!(old_signal(&w, false, FAILING), Err(Errno::ENOMEM), "signal");
        assert_eq!(old_signal(&w, true, FAILING), Err(Errno::ENOMEM), "broadcast");
        assert_eq!(w.load(Relaxed), 0, "no half-built block is published");
        assert_eq!(old_destroy(&w, FAILING), Ok(()), "destroy of the never-installed object");
    }

    #[test]
    fn old_cond_destroy_frees_every_block() {
        let w = AtomicUsize::new(0);
        for _ in 0..1000 {
            old_signal(&w, false, CYCLE).unwrap();
            assert_ne!(w.load(Relaxed), 0);
            old_destroy(&w, CYCLE).unwrap();
            assert_eq!(w.load(Relaxed), 0);
        }
        assert_eq!(LIVE_CYCLE.load(Relaxed), 0, "1000 init/use/destroy cycles leave no block");
    }

    #[test]
    fn old_cond_waiter_inside_keeps_the_block() {
        let w = AtomicUsize::new(0);
        let p = old_block(&w, WAITER).unwrap();
        let (refs, _) = old_view(p).unwrap();
        refs.fetch_add(2, core::sync::atomic::Ordering::AcqRel); // two waiters inside
        old_destroy(&w, WAITER).unwrap();
        assert_eq!(w.load(Relaxed), 0);
        assert_eq!(LIVE_WAITER.load(Relaxed), 1, "destroy after a broadcast keeps the block for its waiters");
        old_leave(refs, p, WAITER);
        assert_eq!(LIVE_WAITER.load(Relaxed), 1, "one waiter still inside");
        old_leave(refs, p, WAITER);
        assert_eq!(LIVE_WAITER.load(Relaxed), 0, "the last one out frees it");
    }
}
