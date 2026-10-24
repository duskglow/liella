//! Internal primitives on atomics plus `Sys` futex: [`RawMutex`], [`RawOnce`], [`RawRwLock`],
//! [`RawCondvar`]. Each is valid when zero-initialised and holds no pointers (decision D-22.1),
//! so the same words can back `PTHREAD_*_INITIALIZER` objects, process-shared objects, and the
//! library's own statics. Waiting goes through [`crate::futex`]; nothing here allocates.
//!
//! The word encodings and state machines are recorded in
//! D-22.12 ((2) to (5)).

use core::hint::spin_loop;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::Ordering::{Acquire, Relaxed, Release};

use liella_kernel::sys::Sys;

use crate::futex::{self, Scope};

/// Read-only polls of a held lock before a thread registers to sleep.
const SPINS: u32 = 100;

/// [`RawMutex`] word: bit 0 is the lock, bits 1..32 count the threads registered to sleep.
const LOCKED: u32 = 1;
/// One registered sleeper.
const SLEEPER: u32 = 2;

/// A futex mutex. The word is `locked | 2 * sleepers`: the lock bit, and the exact number of
/// threads that registered to sleep on it, so an unlock asks the kernel for a wake only when
/// someone is registered.
#[derive(Debug, Default)]
pub struct RawMutex {
    state: AtomicU32,
}

/// Holds a [`RawMutex`] until dropped.
pub struct MutexGuard<'a, S: Sys> {
    m: &'a RawMutex,
    sys: &'a S,
    scope: Scope,
}

impl RawMutex {
    /// Unlocked (all zero).
    pub const fn new() -> RawMutex {
        RawMutex { state: AtomicU32::new(0) }
    }

    /// Make the lock free again whoever held it: in a `fork` child, for a lock a thread that
    /// does not exist there held.
    pub fn reset(&self) {
        // The registered sleepers do not exist in the child either.
        self.state.store(0, Release);
    }

    /// Take the lock if it is free.
    #[inline]
    pub fn try_lock(&self) -> bool {
        let mut s = match self.state.compare_exchange(0, LOCKED, Acquire, Relaxed) {
            Ok(_) => return true,
            Err(v) => v,
        };
        // Free but with sleepers registered: still free to take.
        while s & LOCKED == 0 {
            match self.state.compare_exchange_weak(s, s | LOCKED, Acquire, Relaxed) {
                Ok(_) => return true,
                Err(v) => s = v,
            }
        }
        false
    }

    /// Take the lock, sleeping while another holder has it.
    #[inline]
    pub fn lock<'a, S: Sys>(&'a self, sys: &'a S) -> MutexGuard<'a, S> {
        self.lock_in(sys, Scope::Private)
    }

    /// [`RawMutex::lock`] with an explicit futex scope.
    #[inline]
    pub fn lock_in<'a, S: Sys>(&'a self, sys: &'a S, scope: Scope) -> MutexGuard<'a, S> {
        self.lock_raw(sys, scope);
        MutexGuard { m: self, sys, scope }
    }

    /// Take the lock without a guard (released with [`RawMutex::unlock`]).
    #[inline]
    pub fn lock_raw<S: Sys>(&self, sys: &S, scope: Scope) {
        if self.state.compare_exchange(0, LOCKED, Acquire, Relaxed).is_err() {
            self.lock_contended(sys, scope);
        }
    }

    /// The slow path: poll briefly while the holder runs, then register as a sleeper and sleep
    /// until the lock bit is clear; take the lock and deregister in one step.
    #[cold]
    #[inline(never)]
    fn lock_contended<S: Sys>(&self, sys: &S, scope: Scope) {
        let mut s = self.state.load(Relaxed);
        let mut spins = SPINS;
        // Polling only pays while nobody sleeps yet (the holder is likely running).
        while s & LOCKED != 0 && s < SLEEPER && spins != 0 {
            spin_loop();
            spins = spins.saturating_sub(1);
            s = self.state.load(Relaxed);
        }
        while s & LOCKED == 0 {
            match self.state.compare_exchange_weak(s, s | LOCKED, Acquire, Relaxed) {
                Ok(_) => return,
                Err(v) => s = v,
            }
        }
        // Register. RMWs on the word are totally ordered, so every later unlock sees the count.
        s = self.state.fetch_add(SLEEPER, Relaxed).wrapping_add(SLEEPER);
        loop {
            if s & LOCKED == 0 {
                match self.state.compare_exchange_weak(s, (s | LOCKED).wrapping_sub(SLEEPER), Acquire, Relaxed) {
                    Ok(_) => return,
                    Err(v) => {
                        s = v;
                        continue;
                    }
                }
            }
            // Sleep on the exact (locked) value seen: any change in between returns at once,
            // and the holder of this value wakes one sleeper when it unlocks.
            let _ = futex::wait(sys, &self.state, s, scope);
            s = self.state.load(Relaxed);
        }
    }

    /// Release a lock taken without a guard (the C `pthread_mutex_unlock` shape).
    #[inline]
    pub fn unlock<S: Sys>(&self, sys: &S, scope: Scope) {
        if self.release() {
            self.wake_one(sys, scope);
        }
    }

    /// [`RawMutex::unlock`] in two steps: release the lock, and whether a waiter must be woken with
    /// [`RawMutex::wake_one`] (kept out of line so a caller's fast path makes no call).
    #[inline(always)]
    pub fn release(&self) -> bool {
        // Anything besides the lock bit is a registered sleeper.
        self.state.fetch_sub(LOCKED, Release) != LOCKED
    }

    /// Wake one waiter after [`RawMutex::release`] returned true.
    #[cold]
    #[inline(never)]
    pub fn wake_one<S: Sys>(&self, sys: &S, scope: Scope) {
        let _ = futex::wake(sys, &self.state, 1, scope);
    }

    /// True while some thread holds the lock (a hint only).
    pub fn is_locked(&self) -> bool {
        self.state.load(Relaxed) & LOCKED != 0
    }

    /// True while the lock is held and marked contended: a thread is waiting for it, or was
    /// (a hint only; tests use it to know another thread reached the lock).
    pub fn is_contended(&self) -> bool {
        let s = self.state.load(Relaxed);
        s & LOCKED != 0 && s >= SLEEPER
    }
}

impl<S: Sys> Drop for MutexGuard<'_, S> {
    fn drop(&mut self) {
        self.m.unlock(self.sys, self.scope);
    }
}

/// [`RawOnce`] word: the routine is running.
const ONCE_RUNNING: u32 = 1;
/// Some caller sleeps until the routine finishes.
const ONCE_SLEEPERS: u32 = 2;
/// The routine has finished.
const ONCE_DONE: u32 = 4;

/// One-time initialisation: 0 new, `1` running (`| 2` with sleepers), 4 done.
#[derive(Debug, Default)]
pub struct RawOnce {
    state: AtomicU32,
}

/// Ends a run of the routine: done if it returned, new again if it unwound; wakes the sleepers.
struct OnceRun<'a, S: Sys> {
    word: &'a AtomicU32,
    sys: &'a S,
    finished: bool,
}

impl<S: Sys> Drop for OnceRun<'_, S> {
    fn drop(&mut self) {
        let next = if self.finished { ONCE_DONE } else { 0 };
        if self.word.swap(next, Release) & ONCE_SLEEPERS != 0 {
            let _ = futex::wake_all(self.sys, self.word, Scope::Private);
        }
    }
}

impl RawOnce {
    /// Not yet run (all zero).
    pub const fn new() -> RawOnce {
        RawOnce { state: AtomicU32::new(0) }
    }

    /// True once the initialisation has completed.
    #[inline]
    pub fn is_done(&self) -> bool {
        self.state.load(Acquire) == ONCE_DONE
    }

    /// Run `f` exactly once; concurrent callers wait until it has finished. If `f` unwinds, the
    /// object is as if never run and a later caller runs it.
    #[inline]
    pub fn call_once<S: Sys>(&self, sys: &S, f: impl FnOnce()) {
        if !self.is_done() {
            self.call_slow(sys, f);
        }
    }

    #[cold]
    fn call_slow<S: Sys>(&self, sys: &S, f: impl FnOnce()) {
        let mut s = self.state.load(Acquire);
        loop {
            if s == ONCE_DONE {
                return;
            }
            if s == 0 {
                match self.state.compare_exchange(0, ONCE_RUNNING, Acquire, Acquire) {
                    Ok(_) => {
                        let mut run = OnceRun { word: &self.state, sys, finished: false };
                        f();
                        run.finished = true;
                        return;
                    }
                    Err(v) => {
                        s = v;
                        continue;
                    }
                }
            }
            if s & ONCE_SLEEPERS == 0 {
                if let Err(v) = self.state.compare_exchange(s, s | ONCE_SLEEPERS, Acquire, Acquire) {
                    s = v;
                    continue;
                }
                s |= ONCE_SLEEPERS;
            }
            let _ = futex::wait(sys, &self.state, s, Scope::Private);
            s = self.state.load(Acquire);
        }
    }
}

/// [`RawRwLock`] word: a writer holds the lock.
const RW_WRITER: u32 = 1;
/// Some thread sleeps until the lock is free.
const RW_SLEEPERS: u32 = 2;
/// One reader (readers are counted from bit 2 up).
const RW_READER: u32 = 4;

/// A reader-preferring read-write lock (D-22.4): readers share it whenever no writer holds it.
/// The word holds the reader count (from bit 2), a writer bit and a sleepers bit.
#[derive(Debug, Default)]
pub struct RawRwLock {
    state: AtomicU32,
}

impl RawRwLock {
    /// Unlocked (all zero).
    pub const fn new() -> RawRwLock {
        RawRwLock { state: AtomicU32::new(0) }
    }

    /// Take a shared lock if no writer holds it.
    pub fn try_read(&self) -> bool {
        let mut s = self.state.load(Relaxed);
        loop {
            if s & RW_WRITER != 0 {
                return false;
            }
            let Some(next) = s.checked_add(RW_READER) else { return false };
            match self.state.compare_exchange_weak(s, next, Acquire, Relaxed) {
                Ok(_) => return true,
                Err(v) => s = v,
            }
        }
    }

    /// Take a shared lock.
    pub fn read<S: Sys>(&self, sys: &S, scope: Scope) {
        let mut s = self.state.load(Relaxed);
        loop {
            if s & RW_WRITER == 0 {
                match s.checked_add(RW_READER) {
                    Some(next) => match self.state.compare_exchange_weak(s, next, Acquire, Relaxed) {
                        Ok(_) => return,
                        Err(v) => s = v,
                    },
                    None => {
                        // The reader count is full: let a holder leave.
                        sys.sched_yield();
                        s = self.state.load(Relaxed);
                    }
                }
                continue;
            }
            s = self.sleep(sys, s, scope);
        }
    }

    /// Take the exclusive lock if it is free.
    pub fn try_write(&self) -> bool {
        let mut s = self.state.load(Relaxed);
        while s & !RW_SLEEPERS == 0 {
            match self.state.compare_exchange_weak(s, s | RW_WRITER, Acquire, Relaxed) {
                Ok(_) => return true,
                Err(v) => s = v,
            }
        }
        false
    }

    /// Take the exclusive lock.
    pub fn write<S: Sys>(&self, sys: &S, scope: Scope) {
        let mut s = self.state.load(Relaxed);
        loop {
            if s & !RW_SLEEPERS == 0 {
                // Keep the sleepers bit: this writer's unlock must wake them.
                match self.state.compare_exchange_weak(s, s | RW_WRITER, Acquire, Relaxed) {
                    Ok(_) => return,
                    Err(v) => s = v,
                }
                continue;
            }
            s = self.sleep(sys, s, scope);
        }
    }

    /// Mark the word as having sleepers and sleep on it while it is unchanged; returns the
    /// word's new value.
    #[cold]
    fn sleep<S: Sys>(&self, sys: &S, s: u32, scope: Scope) -> u32 {
        if s & RW_SLEEPERS == 0 {
            if let Err(v) = self.state.compare_exchange(s, s | RW_SLEEPERS, Relaxed, Relaxed) {
                return v;
            }
        }
        let _ = futex::wait(sys, &self.state, s | RW_SLEEPERS, scope);
        self.state.load(Relaxed)
    }

    /// Release a shared or exclusive lock.
    pub fn unlock<S: Sys>(&self, sys: &S, scope: Scope) {
        let mut s = self.state.load(Relaxed);
        loop {
            let held = if s & RW_WRITER != 0 { s & !RW_WRITER } else { s.wrapping_sub(RW_READER) };
            // The release that leaves the lock free wakes everyone; readers never wait for
            // readers, so an earlier one has nobody to let in.
            let free = held & !RW_SLEEPERS == 0;
            let next = if free { 0 } else { held };
            match self.state.compare_exchange_weak(s, next, Release, Relaxed) {
                Ok(_) => {
                    if free && held & RW_SLEEPERS != 0 {
                        let _ = futex::wake_all(sys, &self.state, scope);
                    }
                    return;
                }
                Err(v) => s = v,
            }
        }
    }
}

/// A condition variable: a sequence word that every signal advances.
#[derive(Debug, Default)]
pub struct RawCondvar {
    seq: AtomicU32,
}

impl RawCondvar {
    /// Fresh (all zero).
    pub const fn new() -> RawCondvar {
        RawCondvar { seq: AtomicU32::new(0) }
    }

    /// Atomically release `m`, sleep until signalled (or spuriously), and retake `m`.
    pub fn wait<S: Sys>(&self, sys: &S, m: &RawMutex, scope: Scope) {
        // Read under the mutex: a notify after the unlock changes the word and the sleep
        // returns at once.
        let seq = self.seq.load(Relaxed);
        m.unlock(sys, scope);
        let _ = futex::wait(sys, &self.seq, seq, scope);
        m.lock_raw(sys, scope);
    }

    /// Wake every waiter.
    pub fn notify_all<S: Sys>(&self, sys: &S, scope: Scope) {
        self.seq.fetch_add(1, Release);
        let _ = futex::wake_all(sys, &self.seq, scope);
    }
}
