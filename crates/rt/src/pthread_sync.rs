//! The C synchronisation objects (spec §22.4, §22.5, §22.7; D-22.1, D-22.4): mutexes of every
//! type and protocol, condition variables with the ordering guarantee of §22.5, read-write
//! locks, spin locks, barriers, once objects and semaphores. Each works on the object's own
//! words (the boundary's atomic view of the C object), holds no pointers it follows without the
//! caller's context, and is valid all-zero (static initialisers), so it works process-shared.
//!
//! **Layouts.** The objects keep glibc's static-initialiser bit patterns and the fields a
//! process-shared object needs across binaries where they are visible (⚑ ARB, ⚑ COMPAT):
//! * `pthread_mutex_t` (40 bytes): 32-bit words `__lock` 0, `__count` 1, `__owner` 2,
//!   `__nusers` 3, `__kind` 4 (glibc's encoding: type 0–3, `ROBUST` 16, `PRIO_INHERIT` 32,
//!   `PRIO_PROTECT` 64, `PSHARED` 128), then the robust-list links (`__list.__prev`,
//!   `__list.__next`) as 64-bit words.
//! * `pthread_cond_t` (48 bytes): a sequence word (the waiters' futex), an internal lock, the
//!   waiter count, the pending wakeups, and the flags (clock, shared).
//! * `pthread_rwlock_t` (56 bytes): the state word, the writers waiting, the owning writer's
//!   TID (word 6), shared (7), and glibc's `__flags` (word 12: the kind).
//! * `pthread_barrier_t` (32 bytes), `sem_t` (32 bytes), `pthread_once_t`, `pthread_spinlock_t`.

#![allow(clippy::arithmetic_side_effects)] // counters with explicit overflow checks; word indexes below the object sizes

use core::sync::atomic::{compiler_fence, AtomicU32, AtomicUsize, Ordering::{AcqRel, Acquire, Relaxed, Release, SeqCst}};

use liella_kernel::errno::{Errno, KResult};
use liella_kernel::sys::{FutexArg, Sys};
use liella_kernel::uapi::{
    Timespec, FUTEX_CLOCK_REALTIME, FUTEX_LOCK_PI, FUTEX_OWNER_DIED, FUTEX_PRIVATE_FLAG, FUTEX_TID_MASK, FUTEX_TRYLOCK_PI, FUTEX_UNLOCK_PI, FUTEX_WAITERS,
};

use crate::futex::{self, Clock, Scope};

/// `FUTEX_LOCK_PI2` (Linux 5.14+): `FUTEX_LOCK_PI` with a chosen clock.
const FUTEX_LOCK_PI2: u32 = 13;

/// What the objects need from the calling thread and the runtime.
pub trait Ctx {
    /// The kernel.
    type S: Sys;
    /// The kernel.
    fn sys(&self) -> &Self::S;
    /// The calling thread's TID.
    fn tid(&self) -> u32;
    /// MUTEX-3: the calling thread's TID when it is at hand without a system call (the fast
    /// paths then need no call and no saved registers); `None`: ask [`Ctx::tid`] on the general
    /// path. Default: [`Ctx::tid`].
    #[inline]
    fn fast_tid(&self) -> Option<u32> {
        Some(self.tid())
    }
    /// MUTEX-2: whether the calling thread is certainly the process's only thread and no other
    /// can appear without this turning false first (the runtime creates every thread). The plain
    /// mutex fast paths then take and release a private mutex with plain loads and stores
    /// instead of LOCK-prefixed instructions. Default: never.
    #[inline]
    fn alone(&self) -> bool {
        false
    }
    /// The calling thread's robust-list head: the list (a self-pointing link when empty), the
    /// futex offset, the entry being changed.
    fn robust(&self) -> Option<&[AtomicUsize; 3]>;
    /// The links (`__list.__prev`, `__list.__next`) of the robust mutex whose `__next` word is
    /// at `entry` (an address on the calling thread's robust list).
    fn links(&self, entry: usize) -> Option<&[AtomicUsize]>;
    /// A blocking wait (a cancellation point when `cancellable`): `ECANCELED` if the thread
    /// must act on a cancellation request, otherwise as [`futex::wait_until`].
    fn wait(&self, word: &AtomicU32, expected: u32, scope: Scope, deadline: Option<(Clock, &Timespec)>, cancellable: bool) -> KResult<()> {
        let _ = cancellable;
        futex::wait_until(self.sys(), word, expected, scope, deadline)
    }
}

fn w(words: &[AtomicU32], i: usize) -> &AtomicU32 {
    words.get(i).unwrap_or(&ZERO_WORD)
}

static ZERO_WORD: AtomicU32 = AtomicU32::new(0);

fn valid(t: &Timespec) -> bool {
    (0..1_000_000_000).contains(&t.nsec)
}

// ------------------------------------------------------------------------------ mutex

/// Mutex type: `PTHREAD_MUTEX_NORMAL` / `TIMED_NP` (the default).
pub const MUTEX_NORMAL: u32 = 0;
/// `PTHREAD_MUTEX_RECURSIVE`.
pub const MUTEX_RECURSIVE: u32 = 1;
/// `PTHREAD_MUTEX_ERRORCHECK`.
pub const MUTEX_ERRORCHECK: u32 = 2;
/// `PTHREAD_MUTEX_ADAPTIVE_NP`.
pub const MUTEX_ADAPTIVE: u32 = 3;
/// `__kind` flag: robust.
pub const KIND_ROBUST: u32 = 16;
/// `__kind` flag: priority inheritance.
pub const KIND_PI: u32 = 32;
/// `__kind` flag: priority protection.
pub const KIND_PP: u32 = 64;
/// `__kind` flag: process-shared.
pub const KIND_PSHARED: u32 = 128;
/// `__owner` of a robust mutex whose owner died, until `pthread_mutex_consistent`.
pub const OWNER_INCONSISTENT: u32 = 0x7fff_ffff;
/// `__owner` of a robust mutex unlocked while inconsistent: unusable for good.
pub const OWNER_NOTRECOVERABLE: u32 = 0x7fff_fffe;
/// The priority ceiling's place in `__lock` of a priority-protected mutex.
pub const CEILING_SHIFT: u32 = 19;
const CEILING_MASK: u32 = 0xfff8_0000;

const M_LOCK: usize = 0;
const M_COUNT: usize = 1;
const M_OWNER: usize = 2;
const M_SAVED: usize = 3;
const M_KIND: usize = 4;
/// Spins before sleeping (the adaptive type, and the first try of every other).
const SPINS: u32 = 100;

/// A mutex: its 32-bit words and its robust-list links.
pub struct Mutex<'a> {
    words: &'a [AtomicU32],
    links: &'a [AtomicUsize],
    /// The address of `__list.__next` (the robust-list entry).
    entry: usize,
}

/// How a lock attempt waits.
#[derive(Clone, Copy)]
pub enum LockWait<'t> {
    /// Until acquired.
    Block,
    /// Not at all (`trylock`: `EBUSY`).
    Try,
    /// Until an absolute deadline on a clock.
    Until(Clock, &'t Timespec),
}

impl<'a> Mutex<'a> {
    /// The mutex over `words` (at least 5), `links` (2 words) at `entry` (the `__next` word's
    /// address).
    pub fn new(words: &'a [AtomicU32], links: &'a [AtomicUsize], entry: usize) -> Mutex<'a> {
        Mutex { words, links, entry }
    }

    fn kind(&self) -> u32 {
        w(self.words, M_KIND).load(Relaxed)
    }

    fn scope(&self) -> Scope {
        if self.kind() & KIND_PSHARED != 0 { Scope::Shared } else { Scope::Private }
    }

    /// `pthread_mutex_init`: `kind` in glibc's `__kind` encoding, `ceiling` for a
    /// priority-protected one.
    pub fn init(&self, kind: u32, ceiling: u32) {
        for (i, x) in self.words.iter().enumerate().take(5) {
            x.store(if i == M_KIND { kind } else { 0 }, Relaxed);
        }
        if kind & KIND_PP != 0 {
            w(self.words, M_LOCK).store(ceiling << CEILING_SHIFT, Relaxed);
        }
        for l in self.links {
            l.store(0, Relaxed);
        }
    }

    /// `pthread_mutex_destroy`: `EBUSY` while locked.
    pub fn destroy(&self) -> KResult<()> {
        let lock = w(self.words, M_LOCK).load(Acquire);
        let held = if self.kind() & KIND_PP != 0 { lock & !CEILING_MASK != 0 } else { lock != 0 };
        if held { Err(Errno::EBUSY) } else { Ok(()) }
    }

    /// `pthread_mutex_lock`, `trylock`, `timedlock`, `clocklock`. `EOWNERDEAD` means the
    /// caller holds it and must make it consistent (robust mutexes).
    /// The uncontended case of [`Mutex::lock`] (TINY-HOT), small enough to inline into the
    /// exports: a plain mutex (normal, recursive, error-checking or adaptive; no robust, PI, PP
    /// or process-shared flag) that is unlocked is taken with one CAS. The CAS succeeding means
    /// `__lock` was 0, so the caller did not hold it (`lock`'s relock checks would not apply)
    /// and `lock` would take it the same way, with the same owner and count stores. `false`:
    /// nothing was changed; call `lock`.
    #[inline(always)]
    pub fn lock_fast<C: Ctx>(&self, c: &C) -> bool {
        if let [lock, count, owner, _, kind, ..] = self.words {
            let k = kind.load(Relaxed);
            // MUTEX-3: the TID first (nothing changed yet if it needs a system call).
            let Some(tid) = c.fast_tid() else { return false };
            // MUTEX-2: the only thread takes a free private mutex without a LOCK-prefixed CAS
            // (no other thread can race it; a signal handler that takes and releases it between
            // the load and the store leaves it free again). The compiler fences keep the
            // critical section's accesses between the two stores (no CPU fence: one thread).
            // A held one goes on to the recursive check below without a CAS that must fail.
            if k & !3 == 0
                && if c.alone() {
                    lock.load(Relaxed) == 0 && {
                        lock.store(1, Relaxed);
                        compiler_fence(SeqCst);
                        true
                    }
                } else {
                    lock.compare_exchange(0, 1, Acquire, Relaxed).is_ok()
                }
            {
                owner.store(tid, Relaxed);
                count.store(1, Relaxed);
                return true;
            }
            // MUTEX-FAST: a recursive mutex the caller already holds, below the count limit:
            // one more level, exactly as `lock` counts it (no atomic read-modify-write: only the
            // owner writes `__count`). `lock` reports EAGAIN at the limit.
            if k == MUTEX_RECURSIVE && owner.load(Relaxed) == tid && lock.load(Relaxed) != 0 {
                let n = count.load(Relaxed);
                if n != u32::MAX {
                    count.store(n + 1, Relaxed);
                    return true;
                }
            }
        }
        false
    }

    /// The common case of [`Mutex::unlock`] (TINY-HOT): a plain mutex (as in `lock_fast`)
    /// released for good: a normal or adaptive one, or a recursive or error-checking one the
    /// caller holds once, exactly as `unlock` releases it. `false`: nothing was changed (not the
    /// owner: `unlock` reports EPERM; a recursion count above 1; another type); call `unlock`.
    // MUTEX-5: `always`: LLVM kept `unlock_fast` out of line (two callers), a call and a
    // saved register on every release.
    #[inline(always)]
    pub fn unlock_fast<C: Ctx>(&self, c: &C) -> bool {
        if let [lock, count, owner, _, kind, ..] = self.words {
            let k = kind.load(Relaxed);
            if k & !3 == 0
                && (k == MUTEX_NORMAL
                    || k == MUTEX_ADAPTIVE
                    || (c.fast_tid() == Some(owner.load(Relaxed)) && lock.load(Relaxed) != 0 && (k != MUTEX_RECURSIVE || count.load(Relaxed) <= 1)))
            {
                owner.store(0, Relaxed);
                count.store(0, Relaxed);
                // MUTEX-2: the only thread releases with a plain store (see `lock_fast`); a
                // waiter mark (2, e.g. left in a fork child) takes the general release below.
                if c.alone() && lock.load(Relaxed) == 1 {
                    compiler_fence(SeqCst);
                    lock.store(0, Relaxed);
                    return true;
                }
                if lock.swap(0, Release) == 2 {
                    let _ = futex::wake(c.sys(), lock, 1, Scope::Private);
                }
                return true;
            }
            // MUTEX-FAST: a recursive mutex the caller holds more than once: one level less,
            // still held (as `unlock` does it; owner-only, so a plain store).
            if k == MUTEX_RECURSIVE && c.fast_tid() == Some(owner.load(Relaxed)) && lock.load(Relaxed) != 0 {
                let n = count.load(Relaxed);
                if n > 1 {
                    count.store(n - 1, Relaxed);
                    return true;
                }
            }
        }
        false
    }

    /// Locks the mutex: [`Mutex::lock_fast`] for the plain uncontended case (TINY-HOT), else the general path,
    /// which waits as `how` says and applies the type's rules (recursion, error checking, robustness).
    pub fn lock<C: Ctx>(&self, c: &C, how: LockWait<'_>) -> KResult<()> {
        if self.lock_fast(c) {
            return Ok(());
        }
        let kind = self.kind();
        let ty = kind & 3;
        let me = c.tid();
        if (ty == MUTEX_RECURSIVE || ty == MUTEX_ERRORCHECK || kind & (KIND_ROBUST | KIND_PI) != 0) && self.owner_is(me, kind) {
            {
                if ty == MUTEX_RECURSIVE {
                    let n = w(self.words, M_COUNT).load(Relaxed);
                    if n == u32::MAX {
                        return Err(Errno::EAGAIN);
                    }
                    w(self.words, M_COUNT).store(n + 1, Relaxed);
                    return Ok(());
                }
                // `trylock` on a mutex the caller holds is `EBUSY`, whatever the type (POSIX:
                // "locked (by any thread, including the current thread)"; §22.4 Failure; the
                // reference agrees; validation T19).
                if matches!(how, LockWait::Try) {
                    return Err(Errno::EBUSY);
                }
                // An error-checking mutex, or a normal robust or PI one (glibc reports EDEADLK),
                // relocked by its owner.
                return Err(Errno::EDEADLK);
            }
        }
        // An invalid deadline is an error only if the call would block (§22.4).
        let invalid = matches!(how, LockWait::Until(_, t) if !valid(t));
        let r = if invalid {
            if self.try_fast(c, kind, me) { Ok(()) } else { Err(Errno::EINVAL) }
        } else if kind & KIND_PI != 0 {
            self.lock_pi(c, kind, me, how)
        } else if kind & KIND_ROBUST != 0 {
            self.lock_robust(c, me, how)
        } else if kind & KIND_PP != 0 {
            self.lock_pp(c, how)
        } else {
            self.lock_plain(c, ty, how)
        };
        match r {
            Ok(()) | Err(Errno::EOWNERDEAD) => {
                if w(self.words, M_OWNER).load(Relaxed) == OWNER_NOTRECOVERABLE && kind & KIND_ROBUST != 0 {
                    // Unusable for good: give it back.
                    self.release(c, kind, me);
                    return Err(Errno::ENOTRECOVERABLE);
                }
                if r.is_ok() {
                    w(self.words, M_OWNER).store(me, Relaxed);
                }
                w(self.words, M_COUNT).store(1, Relaxed);
                r
            }
            Err(e) => Err(e),
        }
    }

    fn owner_is(&self, me: u32, kind: u32) -> bool {
        if kind & (KIND_ROBUST | KIND_PI) != 0 {
            w(self.words, M_LOCK).load(Relaxed) & FUTEX_TID_MASK == me
        } else {
            w(self.words, M_OWNER).load(Relaxed) == me && w(self.words, M_LOCK).load(Relaxed) != 0
        }
    }

    fn try_fast<C: Ctx>(&self, c: &C, kind: u32, me: u32) -> bool {
        if kind & (KIND_ROBUST | KIND_PI | KIND_PP) != 0 {
            return false;
        }
        let _ = (c, me);
        w(self.words, M_LOCK).compare_exchange(0, 1, Acquire, Relaxed).is_ok()
    }

    fn lock_plain<C: Ctx>(&self, c: &C, ty: u32, how: LockWait<'_>) -> KResult<()> {
        let lock = w(self.words, M_LOCK);
        if lock.compare_exchange(0, 1, Acquire, Relaxed).is_ok() {
            return Ok(());
        }
        if matches!(how, LockWait::Try) {
            return Err(Errno::EBUSY);
        }
        let spins = if ty == MUTEX_ADAPTIVE { SPINS * 10 } else { SPINS };
        for _ in 0..spins {
            core::hint::spin_loop();
            if lock.load(Relaxed) == 0 && lock.compare_exchange(0, 1, Acquire, Relaxed).is_ok() {
                return Ok(());
            }
        }
        let deadline = match how {
            LockWait::Until(clk, t) => Some((clk, t)),
            _ => None,
        };
        while lock.swap(2, Acquire) != 0 {
            match c.wait(lock, 2, self.scope(), deadline, false) {
                Err(Errno::ETIMEDOUT) => return Err(Errno::ETIMEDOUT),
                _ => continue,
            }
        }
        Ok(())
    }

    fn lock_pp<C: Ctx>(&self, c: &C, how: LockWait<'_>) -> KResult<()> {
        // The state lives below the ceiling bits.
        let lock = w(self.words, M_LOCK);
        loop {
            let v = lock.load(Relaxed);
            let state = v & !CEILING_MASK;
            if state == 0 {
                if lock.compare_exchange(v, v | 1, Acquire, Relaxed).is_ok() {
                    return Ok(());
                }
                continue;
            }
            match how {
                LockWait::Try => return Err(Errno::EBUSY),
                _ => {
                    let want = (v & CEILING_MASK) | 2;
                    if state != 2 && lock.compare_exchange(v, want, Relaxed, Relaxed).is_err() {
                        continue;
                    }
                    let deadline = match how {
                        LockWait::Until(clk, t) => Some((clk, t)),
                        _ => None,
                    };
                    if let Err(Errno::ETIMEDOUT) = c.wait(lock, want, self.scope(), deadline, false) {
                        return Err(Errno::ETIMEDOUT);
                    }
                }
            }
        }
    }

    fn lock_robust<C: Ctx>(&self, c: &C, me: u32, how: LockWait<'_>) -> KResult<()> {
        let head = c.robust();
        if let Some(h) = head {
            h[2].store(self.entry, Release);
        }
        let lock = w(self.words, M_LOCK);
        let r = loop {
            let v = lock.load(Relaxed);
            if v & FUTEX_OWNER_DIED != 0 && v & FUTEX_TID_MASK == 0 {
                // The owner died: take it over, inconsistent.
                if lock.compare_exchange(v, me | (v & FUTEX_WAITERS), Acquire, Relaxed).is_ok() {
                    w(self.words, M_OWNER).store(OWNER_INCONSISTENT, Relaxed);
                    break Err(Errno::EOWNERDEAD);
                }
                continue;
            }
            if v == 0 {
                if lock.compare_exchange(0, me, Acquire, Relaxed).is_ok() {
                    break Ok(());
                }
                continue;
            }
            if matches!(how, LockWait::Try) {
                break Err(Errno::EBUSY);
            }
            if v & FUTEX_WAITERS == 0 && lock.compare_exchange(v, v | FUTEX_WAITERS, Relaxed, Relaxed).is_err() {
                continue;
            }
            let deadline = match how {
                LockWait::Until(clk, t) => Some((clk, t)),
                _ => None,
            };
            // Robust mutexes may be shared: the kernel's wake at the owner's death is shared.
            if let Err(Errno::ETIMEDOUT) = c.wait(lock, v | FUTEX_WAITERS, Scope::Shared, deadline, false) {
                break Err(Errno::ETIMEDOUT);
            }
        };
        if matches!(r, Ok(()) | Err(Errno::EOWNERDEAD)) {
            self.enqueue(c);
        }
        if let Some(h) = head {
            h[2].store(0, Release);
        }
        r
    }

    fn lock_pi<C: Ctx>(&self, c: &C, kind: u32, me: u32, how: LockWait<'_>) -> KResult<()> {
        let robust = kind & KIND_ROBUST != 0;
        let head = if robust { c.robust() } else { None };
        if let Some(h) = head {
            h[2].store(self.entry, Release);
        }
        let lock = w(self.words, M_LOCK);
        let shared = kind & KIND_PSHARED != 0 || robust;
        let private = if shared { 0 } else { FUTEX_PRIVATE_FLAG };
        let r = if lock.compare_exchange(0, me, Acquire, Relaxed).is_ok() {
            Ok(())
        } else {
            match how {
                LockWait::Try => c.sys().futex(lock, FUTEX_TRYLOCK_PI | private, 0, FutexArg::None, None, 0).map(drop),
                LockWait::Block => loop {
                    match c.sys().futex(lock, FUTEX_LOCK_PI | private, 0, FutexArg::None, None, 0) {
                        Err(Errno::EINTR | Errno::EAGAIN) => continue,
                        other => break other.map(drop),
                    }
                },
                LockWait::Until(clk, t) => loop {
                    let op = match clk {
                        Clock::Realtime => FUTEX_LOCK_PI,
                        Clock::Monotonic => FUTEX_LOCK_PI2,
                    };
                    match c.sys().futex(lock, op | private | if clk == Clock::Realtime && op == FUTEX_LOCK_PI2 { FUTEX_CLOCK_REALTIME } else { 0 }, 0, FutexArg::Timeout(t), None, 0) {
                        Err(Errno::EINTR | Errno::EAGAIN) => continue,
                        other => break other.map(drop),
                    }
                },
            }
            .map_err(|e| if e == Errno::EDEADLK { Errno::EDEADLK } else { e })
        };
        let r = match r {
            Ok(()) if lock.load(Relaxed) & FUTEX_OWNER_DIED != 0 => {
                lock.fetch_and(!FUTEX_OWNER_DIED, Relaxed);
                w(self.words, M_OWNER).store(OWNER_INCONSISTENT, Relaxed);
                if robust { Err(Errno::EOWNERDEAD) } else { Ok(()) }
            }
            other => other,
        };
        if robust && matches!(r, Ok(()) | Err(Errno::EOWNERDEAD)) {
            self.enqueue(c);
        }
        if let Some(h) = head {
            h[2].store(0, Release);
        }
        r
    }

    /// Put this mutex on the calling thread's robust list (newest first).
    fn enqueue<C: Ctx>(&self, c: &C) {
        let Some(h) = c.robust() else { return };
        let head_addr = core::ptr::from_ref(h).expose_provenance();
        let first = h[0].load(Relaxed);
        if let Some(l) = self.links.get(1) {
            l.store(first, Relaxed);
        }
        if let Some(p) = self.links.first() {
            p.store(head_addr, Relaxed);
        }
        if first != head_addr {
            if let Some(fl) = c.links(first) {
                if let Some(p) = fl.first() {
                    p.store(self.entry, Relaxed);
                }
            }
        }
        h[0].store(self.entry, Release);
    }

    /// Take this mutex off the calling thread's robust list.
    fn dequeue<C: Ctx>(&self, c: &C) {
        let Some(h) = c.robust() else { return };
        let head_addr = core::ptr::from_ref(h).expose_provenance();
        let prev = self.links.first().map_or(0, |x| x.load(Relaxed));
        let next = self.links.get(1).map_or(0, |x| x.load(Relaxed));
        if prev == head_addr || prev == 0 {
            h[0].store(if next == 0 { head_addr } else { next }, Release);
        } else if let Some(pl) = c.links(prev) {
            if let Some(n) = pl.get(1) {
                n.store(next, Release);
            }
        }
        if next != head_addr && next != 0 {
            if let Some(nl) = c.links(next) {
                if let Some(p) = nl.first() {
                    p.store(prev, Relaxed);
                }
            }
        }
    }

    /// `pthread_mutex_unlock`: `EPERM` for an error-checking, recursive, robust or PI mutex
    /// the caller does not hold.
    pub fn unlock<C: Ctx>(&self, c: &C) -> KResult<()> {
        if self.unlock_fast(c) {
            return Ok(());
        }
        let kind = self.kind();
        let ty = kind & 3;
        let me = c.tid();
        let checked = ty == MUTEX_RECURSIVE || ty == MUTEX_ERRORCHECK || kind & (KIND_ROBUST | KIND_PI) != 0;
        if checked && !self.owner_is(me, kind) {
            return Err(Errno::EPERM);
        }
        if ty == MUTEX_RECURSIVE {
            let n = w(self.words, M_COUNT).load(Relaxed);
            if n > 1 {
                w(self.words, M_COUNT).store(n - 1, Relaxed);
                return Ok(());
            }
        }
        if kind & KIND_ROBUST != 0 && w(self.words, M_OWNER).load(Relaxed) == OWNER_INCONSISTENT {
            // Unlocked without pthread_mutex_consistent: unusable from now on.
            w(self.words, M_OWNER).store(OWNER_NOTRECOVERABLE, Relaxed);
        } else {
            w(self.words, M_OWNER).store(0, Relaxed);
        }
        w(self.words, M_COUNT).store(0, Relaxed);
        self.release(c, kind, me);
        Ok(())
    }

    fn release<C: Ctx>(&self, c: &C, kind: u32, me: u32) {
        let lock = w(self.words, M_LOCK);
        if kind & KIND_ROBUST != 0 {
            let h = c.robust();
            if let Some(h) = h {
                h[2].store(self.entry, Release);
            }
            self.dequeue(c);
            if kind & KIND_PI != 0 {
                self.release_pi(c, kind, me);
            } else {
                let old = lock.swap(0, Release);
                if old & FUTEX_WAITERS != 0 {
                    let _ = futex::wake(c.sys(), lock, 1, Scope::Shared);
                }
            }
            if let Some(h) = h {
                h[2].store(0, Release);
            }
        } else if kind & KIND_PI != 0 {
            self.release_pi(c, kind, me);
        } else if kind & KIND_PP != 0 {
            let old = lock.fetch_and(CEILING_MASK, Release);
            if old & !CEILING_MASK == 2 {
                let _ = futex::wake(c.sys(), lock, 1, self.scope());
            }
        } else if lock.swap(0, Release) == 2 {
            let _ = futex::wake(c.sys(), lock, 1, self.scope());
        }
    }

    fn release_pi<C: Ctx>(&self, c: &C, kind: u32, me: u32) {
        let lock = w(self.words, M_LOCK);
        if lock.compare_exchange(me, 0, Release, Relaxed).is_err() {
            let private = if kind & (KIND_PSHARED | KIND_ROBUST) != 0 { 0 } else { FUTEX_PRIVATE_FLAG };
            let _ = c.sys().futex(lock, FUTEX_UNLOCK_PI | private, 0, FutexArg::None, None, 0);
        }
    }

    /// `pthread_mutex_consistent`: `EINVAL` unless robust, inconsistent and held.
    pub fn consistent<C: Ctx>(&self, c: &C) -> KResult<()> {
        let kind = self.kind();
        if kind & KIND_ROBUST == 0 || w(self.words, M_OWNER).load(Relaxed) != OWNER_INCONSISTENT || !self.owner_is(c.tid(), kind) {
            return Err(Errno::EINVAL);
        }
        w(self.words, M_OWNER).store(c.tid(), Relaxed);
        Ok(())
    }

    /// `pthread_mutex_getprioceiling`: `EINVAL` unless priority-protected.
    pub fn prioceiling(&self) -> KResult<u32> {
        if self.kind() & KIND_PP == 0 {
            return Err(Errno::EINVAL);
        }
        Ok(w(self.words, M_LOCK).load(Relaxed) >> CEILING_SHIFT)
    }

    /// `pthread_mutex_setprioceiling`: takes the mutex, sets the ceiling, releases it; the old
    /// ceiling.
    pub fn set_prioceiling<C: Ctx>(&self, c: &C, ceiling: u32) -> KResult<u32> {
        if self.kind() & KIND_PP == 0 || ceiling > (CEILING_MASK >> CEILING_SHIFT) {
            return Err(Errno::EINVAL);
        }
        let held_by_me = w(self.words, M_OWNER).load(Relaxed) == c.tid();
        if !held_by_me {
            self.lock(c, LockWait::Block)?;
        }
        let lock = w(self.words, M_LOCK);
        let old = lock.load(Relaxed) >> CEILING_SHIFT;
        let v = lock.load(Relaxed);
        lock.store((v & !CEILING_MASK) | (ceiling << CEILING_SHIFT), Relaxed);
        if !held_by_me {
            self.unlock(c)?;
        }
        Ok(old)
    }

    /// The saved-priority word (`__nusers`), for priority protection.
    pub fn saved(&self) -> &AtomicU32 {
        w(self.words, M_SAVED)
    }

    /// True for a priority-protected mutex.
    pub fn protected(&self) -> bool {
        self.kind() & KIND_PP != 0
    }
}

// ------------------------------------------------------------------------------ mutex attributes

/// `pthread_mutexattr_t` (an `int`, glibc's encoding): the type in the low bits, protocol at
/// 28, ceiling at 12, robust bit 30, shared bit 31.
pub mod mattr {
    /// Type mask.
    pub const KIND_MASK: u32 = 0xfff;
    /// Protocol shift.
    pub const PROTOCOL_SHIFT: u32 = 28;
    /// Protocol mask.
    pub const PROTOCOL_MASK: u32 = 0x3000_0000;
    /// Ceiling shift.
    pub const CEILING_SHIFT: u32 = 12;
    /// Ceiling mask.
    pub const CEILING_MASK: u32 = 0x00ff_f000;
    /// Robust.
    pub const ROBUST: u32 = 0x4000_0000;
    /// Process-shared.
    pub const PSHARED: u32 = 0x8000_0000;
    /// `PTHREAD_PRIO_NONE`.
    pub const PRIO_NONE: u32 = 0;
    /// `PTHREAD_PRIO_INHERIT`.
    pub const PRIO_INHERIT: u32 = 1;
    /// `PTHREAD_PRIO_PROTECT`.
    pub const PRIO_PROTECT: u32 = 2;

    /// The mutex `__kind` an attribute value gives.
    pub fn kind(a: u32) -> u32 {
        let mut k = a & 3;
        if a & ROBUST != 0 {
            k |= super::KIND_ROBUST;
        }
        if a & PSHARED != 0 {
            k |= super::KIND_PSHARED;
        }
        match (a & PROTOCOL_MASK) >> PROTOCOL_SHIFT {
            PRIO_INHERIT => k |= super::KIND_PI,
            PRIO_PROTECT => k |= super::KIND_PP,
            _ => {}
        }
        k
    }

    /// The ceiling an attribute value carries.
    pub fn ceiling(a: u32) -> u32 {
        (a & CEILING_MASK) >> CEILING_SHIFT
    }
}

// ------------------------------------------------------------------------------ condition variable

const C_SEQ: usize = 0;
const C_LOCK: usize = 1;
const C_WAITERS: usize = 2;
const C_WAKEUPS: usize = 3;
const C_FLAGS: usize = 4;
/// Condition-variable flag: waits use `CLOCK_MONOTONIC`.
pub const COND_MONOTONIC: u32 = 1;
/// Condition-variable flag: process-shared.
pub const COND_SHARED: u32 = 2;

/// A condition variable (§22.5). A waiter records the sequence number when it starts waiting;
/// a signal advances the sequence and grants one wakeup, which only a waiter whose recorded
/// number differs (one that waited before the signal) may consume: the guarantee that a
/// signal wakes only a waiter that blocked before it.
pub struct Cond<'a> {
    words: &'a [AtomicU32],
}

impl<'a> Cond<'a> {
    /// The condition variable over `words` (at least 5).
    pub fn new(words: &'a [AtomicU32]) -> Cond<'a> {
        Cond { words }
    }

    fn scope(&self) -> Scope {
        if w(self.words, C_FLAGS).load(Relaxed) & COND_SHARED != 0 { Scope::Shared } else { Scope::Private }
    }

    /// `pthread_cond_init` with the attribute's flags.
    pub fn init(&self, flags: u32) {
        for (i, x) in self.words.iter().enumerate() {
            x.store(if i == C_FLAGS { flags } else { 0 }, Relaxed);
        }
    }

    /// The clock the timed waits use without an explicit one.
    pub fn clock(&self) -> Clock {
        if w(self.words, C_FLAGS).load(Relaxed) & COND_MONOTONIC != 0 { Clock::Monotonic } else { Clock::Realtime }
    }

    fn lock<C: Ctx>(&self, c: &C) {
        let l = w(self.words, C_LOCK);
        if l.compare_exchange(0, 1, Acquire, Relaxed).is_ok() {
            return;
        }
        while l.swap(2, Acquire) != 0 {
            let _ = futex::wait(c.sys(), l, 2, self.scope());
        }
    }

    fn unlock<C: Ctx>(&self, c: &C) {
        let l = w(self.words, C_LOCK);
        if l.swap(0, Release) == 2 {
            let _ = futex::wake(c.sys(), l, 1, self.scope());
        }
    }

    /// `pthread_cond_wait`/`timedwait`/`clockwait`: release `m`, wait, retake `m`.
    /// `ECANCELED` (a cancellation request acted on while waiting) comes back with `m` held.
    pub fn wait<C: Ctx>(&self, c: &C, m: &Mutex<'_>, deadline: Option<(Clock, &Timespec)>) -> KResult<()> {
        if let Some((_, t)) = deadline {
            if !valid(t) {
                return Err(Errno::EINVAL);
            }
        }
        self.lock(c);
        let mut seen = w(self.words, C_SEQ).load(Relaxed);
        w(self.words, C_WAITERS).fetch_add(1, Relaxed);
        self.unlock(c);
        if let Err(e) = m.unlock(c) {
            // Not the caller's mutex (EPERM): no wait happened.
            self.lock(c);
            w(self.words, C_WAITERS).fetch_sub(1, Relaxed);
            self.unlock(c);
            return Err(e);
        }
        let outcome = loop {
            let r = c.wait(w(self.words, C_SEQ), seen, self.scope(), deadline, true);
            self.lock(c);
            let now = w(self.words, C_SEQ).load(Relaxed);
            let pending = w(self.words, C_WAKEUPS).load(Relaxed);
            if now != seen && pending > 0 {
                // A signal after this waiter started: take its wakeup.
                w(self.words, C_WAKEUPS).store(pending - 1, Relaxed);
                w(self.words, C_WAITERS).fetch_sub(1, Relaxed);
                self.unlock(c);
                break Ok(());
            }
            if let Err(e @ (Errno::ETIMEDOUT | Errno::ECANCELED)) = r {
                w(self.words, C_WAITERS).fetch_sub(1, Relaxed);
                self.unlock(c);
                break Err(e);
            }
            // The signals so far were taken by others: wait for the next.
            seen = now;
            self.unlock(c);
        };
        // Retaking the mutex: EOWNERDEAD (a robust mutex whose owner died) comes back as is.
        let relock = m.lock(c, LockWait::Block);
        match (outcome, relock) {
            (_, Err(e)) => Err(e),
            (o, Ok(())) => o,
        }
    }

    /// `pthread_cond_signal`: wake one waiter that started waiting before this call.
    pub fn signal<C: Ctx>(&self, c: &C) {
        self.lock(c);
        let waiters = w(self.words, C_WAITERS).load(Relaxed);
        let pending = w(self.words, C_WAKEUPS).load(Relaxed);
        if waiters > pending {
            w(self.words, C_WAKEUPS).store(pending + 1, Relaxed);
            w(self.words, C_SEQ).fetch_add(1, Release);
            let _ = futex::wake(c.sys(), w(self.words, C_SEQ), 1, self.scope());
        }
        self.unlock(c);
    }

    /// `pthread_cond_broadcast`: wake every current waiter.
    pub fn broadcast<C: Ctx>(&self, c: &C) {
        self.lock(c);
        let waiters = w(self.words, C_WAITERS).load(Relaxed);
        if waiters > w(self.words, C_WAKEUPS).load(Relaxed) {
            w(self.words, C_WAKEUPS).store(waiters, Relaxed);
            w(self.words, C_SEQ).fetch_add(1, Release);
            let _ = futex::wake_all(c.sys(), w(self.words, C_SEQ), self.scope());
        }
        self.unlock(c);
    }
}

// ------------------------------------------------------------------------------ read-write lock

const R_STATE: usize = 0;
const R_WWAIT: usize = 1;
const R_WRITER: usize = 6;
const R_SHARED: usize = 7;
const R_FLAGS: usize = 12;
const WRITER: u32 = 1 << 30;
const WAITING: u32 = 1 << 31;
const READERS: u32 = WRITER - 1;
/// `PTHREAD_RWLOCK_PREFER_READER_NP`.
pub const RW_PREFER_READER: u32 = 0;
/// `PTHREAD_RWLOCK_PREFER_WRITER_NP`.
pub const RW_PREFER_WRITER: u32 = 1;
/// `PTHREAD_RWLOCK_PREFER_WRITER_NONRECURSIVE_NP`.
pub const RW_PREFER_WRITER_NONRECURSIVE: u32 = 2;

/// A read-write lock (§22.5, D-22.4): readers share it unless a writer holds it, or, with
/// `PREFER_WRITER_NONRECURSIVE`, while a writer waits.
pub struct RwLock<'a> {
    words: &'a [AtomicU32],
}

impl<'a> RwLock<'a> {
    /// The lock over `words` (14).
    pub fn new(words: &'a [AtomicU32]) -> RwLock<'a> {
        RwLock { words }
    }

    fn scope(&self) -> Scope {
        if w(self.words, R_SHARED).load(Relaxed) != 0 { Scope::Shared } else { Scope::Private }
    }

    /// `pthread_rwlock_init`.
    pub fn init(&self, kind: u32, shared: bool) {
        for x in self.words {
            x.store(0, Relaxed);
        }
        w(self.words, R_FLAGS).store(kind, Relaxed);
        w(self.words, R_SHARED).store(u32::from(shared), Relaxed);
    }

    /// `pthread_rwlock_destroy`: `EBUSY` while held.
    pub fn destroy(&self) -> KResult<()> {
        if w(self.words, R_STATE).load(Acquire) & !WAITING != 0 { Err(Errno::EBUSY) } else { Ok(()) }
    }

    fn writers_first(&self) -> bool {
        w(self.words, R_FLAGS).load(Relaxed) == RW_PREFER_WRITER_NONRECURSIVE
    }

    /// `pthread_rwlock_rdlock` and its forms. `EDEADLK` if the caller holds it for writing.
    pub fn read<C: Ctx>(&self, c: &C, how: LockWait<'_>) -> KResult<()> {
        if w(self.words, R_WRITER).load(Relaxed) == c.tid() {
            return Err(Errno::EDEADLK);
        }
        let st = w(self.words, R_STATE);
        loop {
            let s = st.load(Relaxed);
            let blocked = s & WRITER != 0 || (self.writers_first() && w(self.words, R_WWAIT).load(Relaxed) != 0);
            if !blocked {
                if s & READERS == READERS {
                    return Err(Errno::EAGAIN);
                }
                if st.compare_exchange_weak(s, s + 1, Acquire, Relaxed).is_ok() {
                    return Ok(());
                }
                continue;
            }
            self.wait_change(c, s, how)?;
        }
    }

    /// `pthread_rwlock_wrlock` and its forms. `EDEADLK` if the caller holds it for writing.
    pub fn write<C: Ctx>(&self, c: &C, how: LockWait<'_>) -> KResult<()> {
        let me = c.tid();
        if w(self.words, R_WRITER).load(Relaxed) == me {
            return Err(Errno::EDEADLK);
        }
        let st = w(self.words, R_STATE);
        let wwait = w(self.words, R_WWAIT);
        let mut counted = false;
        let r = loop {
            let s = st.load(Relaxed);
            if s & !WAITING == 0 {
                if st.compare_exchange_weak(s, s | WRITER, Acquire, Relaxed).is_ok() {
                    break Ok(());
                }
                continue;
            }
            if !counted && !matches!(how, LockWait::Try) {
                wwait.fetch_add(1, Relaxed);
                counted = true;
            }
            if let Err(e) = self.wait_change(c, s, how) {
                break Err(e);
            }
        };
        if counted {
            wwait.fetch_sub(1, Relaxed);
            // Readers held back for this writer may proceed.
            if r.is_err() {
                let _ = futex::wake_all(c.sys(), st, self.scope());
            }
        }
        if r.is_ok() {
            w(self.words, R_WRITER).store(me, Relaxed);
        }
        r
    }

    fn wait_change<C: Ctx>(&self, c: &C, s: u32, how: LockWait<'_>) -> KResult<()> {
        let st = w(self.words, R_STATE);
        let deadline = match how {
            LockWait::Try => return Err(Errno::EBUSY),
            LockWait::Until(clk, t) => {
                if !valid(t) {
                    return Err(Errno::EINVAL);
                }
                Some((clk, t))
            }
            LockWait::Block => None,
        };
        if s & WAITING == 0 && st.compare_exchange(s, s | WAITING, Relaxed, Relaxed).is_err() {
            return Ok(());
        }
        match c.wait(st, s | WAITING, self.scope(), deadline, false) {
            Err(Errno::ETIMEDOUT) => Err(Errno::ETIMEDOUT),
            _ => Ok(()),
        }
    }

    /// `pthread_rwlock_unlock`: `EPERM` if not held.
    pub fn unlock<C: Ctx>(&self, c: &C) -> KResult<()> {
        let st = w(self.words, R_STATE);
        let s = st.load(Relaxed);
        let after = if s & WRITER != 0 {
            if w(self.words, R_WRITER).load(Relaxed) != c.tid() {
                return Err(Errno::EPERM);
            }
            w(self.words, R_WRITER).store(0, Relaxed);
            st.fetch_and(!WRITER, Release) & !WRITER
        } else if s & READERS != 0 {
            st.fetch_sub(1, Release) - 1
        } else {
            return Err(Errno::EPERM);
        };
        if after & !WAITING == 0 && after & WAITING != 0 && st.compare_exchange(after, after & !WAITING, Relaxed, Relaxed).is_ok() {
            let _ = futex::wake_all(c.sys(), st, self.scope());
        }
        Ok(())
    }
}

// ------------------------------------------------------------------------------ spin lock

/// `pthread_spin_lock` over the lock word: busy-waits, never blocks (§22.5).
pub fn spin_lock(word: &AtomicU32) {
    loop {
        if word.compare_exchange_weak(0, 1, Acquire, Relaxed).is_ok() {
            return;
        }
        while word.load(Relaxed) != 0 {
            core::hint::spin_loop();
        }
    }
}

/// `pthread_spin_trylock`: `EBUSY` if held.
pub fn spin_trylock(word: &AtomicU32) -> KResult<()> {
    word.compare_exchange(0, 1, Acquire, Relaxed).map(drop).map_err(|_| Errno::EBUSY)
}

/// `pthread_spin_unlock`.
pub fn spin_unlock(word: &AtomicU32) {
    word.store(0, Release);
}

// ------------------------------------------------------------------------------ barrier

const B_COUNT: usize = 0;
const B_ARRIVED: usize = 1;
const B_ROUND: usize = 2;
const B_SHARED: usize = 3;
/// `PTHREAD_BARRIER_SERIAL_THREAD`.
pub const BARRIER_SERIAL: i32 = -1;

/// A barrier (§22.5): the last of `count` arrivals is the serial thread and releases the rest.
pub struct Barrier<'a> {
    words: &'a [AtomicU32],
}

impl<'a> Barrier<'a> {
    /// The barrier over `words` (8).
    pub fn new(words: &'a [AtomicU32]) -> Barrier<'a> {
        Barrier { words }
    }

    /// `pthread_barrier_init`: `EINVAL` for a count of 0.
    pub fn init(&self, count: u32, shared: bool) -> KResult<()> {
        if count == 0 {
            return Err(Errno::EINVAL);
        }
        for x in self.words {
            x.store(0, Relaxed);
        }
        w(self.words, B_COUNT).store(count, Relaxed);
        w(self.words, B_SHARED).store(u32::from(shared), Relaxed);
        Ok(())
    }

    /// `pthread_barrier_wait`: `PTHREAD_BARRIER_SERIAL_THREAD` for exactly one thread, 0 for
    /// the others.
    pub fn wait<C: Ctx>(&self, c: &C) -> i32 {
        let scope = if w(self.words, B_SHARED).load(Relaxed) != 0 { Scope::Shared } else { Scope::Private };
        let round = w(self.words, B_ROUND).load(Acquire);
        let n = w(self.words, B_ARRIVED).fetch_add(1, AcqRel) + 1;
        if n >= w(self.words, B_COUNT).load(Relaxed) {
            w(self.words, B_ARRIVED).store(0, SeqCst);
            w(self.words, B_ROUND).fetch_add(1, SeqCst);
            let _ = futex::wake_all(c.sys(), w(self.words, B_ROUND), scope);
            return BARRIER_SERIAL;
        }
        while w(self.words, B_ROUND).load(Acquire) == round {
            let _ = futex::wait(c.sys(), w(self.words, B_ROUND), round, scope);
        }
        0
    }
}

// ------------------------------------------------------------------------------ once

const ONCE_NEW: u32 = 0;
const ONCE_RUNNING: u32 = 1;
const ONCE_WAITED: u32 = 2;
const ONCE_DONE: u32 = 3;

/// `pthread_once`/`call_once` (§22.5): the first caller runs the routine (`run`); others wait
/// until it completes. If the routine is cancelled or a foreign exception unwinds out of it,
/// [`once_reset`] makes the object new again and wakes the waiters (one of them runs it).
pub fn once<S: Sys>(sys: &S, word: &AtomicU32, run: impl FnOnce()) {
    if once_begin(sys, word) {
        run();
        once_end(sys, word);
    }
}

/// The first half of [`once`]: `true` when the caller now owns the object (running) and must
/// run the routine, then call [`once_end`] (or [`once_reset`] if the routine never completes);
/// `false` when the routine has completed (here or in another thread).
pub fn once_begin<S: Sys>(sys: &S, word: &AtomicU32) -> bool {
    loop {
        match word.load(Acquire) {
            ONCE_DONE => return false,
            ONCE_NEW => {
                if word.compare_exchange(ONCE_NEW, ONCE_RUNNING, Acquire, Acquire).is_ok() {
                    return true;
                }
            }
            ONCE_RUNNING => {
                let _ = word.compare_exchange(ONCE_RUNNING, ONCE_WAITED, Relaxed, Relaxed);
            }
            _ => {
                let _ = futex::wait(sys, word, ONCE_WAITED, Scope::Shared);
            }
        }
    }
}

/// The routine of a once object the caller owns completed: done, waiters woken.
pub fn once_end<S: Sys>(sys: &S, word: &AtomicU32) {
    if word.swap(ONCE_DONE, Release) == ONCE_WAITED {
        let _ = futex::wake_all(sys, word, Scope::Shared);
    }
}

/// The routine of a once object was cancelled or unwound by an exception: back to new, waiters woken (one of them runs
/// it).
pub fn once_reset<S: Sys>(sys: &S, word: &AtomicU32) {
    word.store(ONCE_NEW, Release);
    let _ = futex::wake_all(sys, word, Scope::Shared);
}

// ------------------------------------------------------------------------------ semaphore

const S_VALUE: usize = 0;
const S_WAITERS: usize = 1;
const S_SHARED: usize = 2;
/// `SEM_VALUE_MAX`.
pub const SEM_VALUE_MAX: u32 = 0x7fff_ffff;

/// A counting semaphore (§22.7). `post` is async-signal-safe: one atomic add, and a wake only
/// when someone waits.
pub struct Sem<'a> {
    words: &'a [AtomicU32],
}

impl<'a> Sem<'a> {
    /// The semaphore over `words` (8).
    pub fn new(words: &'a [AtomicU32]) -> Sem<'a> {
        Sem { words }
    }

    fn scope(&self) -> Scope {
        if w(self.words, S_SHARED).load(Relaxed) != 0 { Scope::Shared } else { Scope::Private }
    }

    /// `sem_init`: `EINVAL` above `SEM_VALUE_MAX`.
    pub fn init(&self, value: u32, shared: bool) -> KResult<()> {
        if value > SEM_VALUE_MAX {
            return Err(Errno::EINVAL);
        }
        w(self.words, S_VALUE).store(value, Relaxed);
        w(self.words, S_WAITERS).store(0, Relaxed);
        w(self.words, S_SHARED).store(u32::from(shared), Relaxed);
        Ok(())
    }

    /// `sem_post`: `EOVERFLOW` at `SEM_VALUE_MAX`.
    pub fn post<S: Sys>(&self, sys: &S) -> KResult<()> {
        let v = w(self.words, S_VALUE);
        let mut cur = v.load(Relaxed);
        loop {
            if cur >= SEM_VALUE_MAX {
                return Err(Errno::EOVERFLOW);
            }
            match v.compare_exchange_weak(cur, cur + 1, Release, Relaxed) {
                Ok(_) => break,
                Err(now) => cur = now,
            }
        }
        if w(self.words, S_WAITERS).load(SeqCst) != 0 {
            let _ = futex::wake(sys, v, 1, self.scope());
        }
        Ok(())
    }

    fn try_take(&self) -> bool {
        let v = w(self.words, S_VALUE);
        let mut cur = v.load(Relaxed);
        while cur > 0 {
            match v.compare_exchange_weak(cur, cur - 1, Acquire, Relaxed) {
                Ok(_) => return true,
                Err(now) => cur = now,
            }
        }
        false
    }

    /// `sem_wait`, `sem_trywait` (`EAGAIN`), `sem_timedwait`/`clockwait` (`ETIMEDOUT`); a
    /// cancellation point (`ECANCELED`) and interruptible (`EINTR`).
    pub fn wait<C: Ctx>(&self, c: &C, how: LockWait<'_>) -> KResult<()> {
        if self.try_take() {
            return Ok(());
        }
        let deadline = match how {
            LockWait::Try => return Err(Errno::EAGAIN),
            LockWait::Until(clk, t) => {
                if !valid(t) {
                    return Err(Errno::EINVAL);
                }
                Some((clk, t))
            }
            LockWait::Block => None,
        };
        w(self.words, S_WAITERS).fetch_add(1, SeqCst);
        let r = loop {
            if self.try_take() {
                break Ok(());
            }
            match c.wait(w(self.words, S_VALUE), 0, self.scope(), deadline, true) {
                Err(e @ (Errno::ETIMEDOUT | Errno::EINTR | Errno::ECANCELED)) => {
                    if self.try_take() {
                        break Ok(());
                    }
                    break Err(e);
                }
                _ => continue,
            }
        };
        w(self.words, S_WAITERS).fetch_sub(1, SeqCst);
        r
    }

    /// `sem_getvalue`: never negative (§22.7).
    pub fn value(&self) -> u32 {
        w(self.words, S_VALUE).load(Relaxed)
    }
}
