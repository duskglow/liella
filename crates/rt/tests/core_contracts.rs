//! Contract tests for the runtime core (COLD-REWRITE-W-far-13): `futex`, `sync`, `cpu`, `tls`
//! and `fatal`. Written against the implementation before its rewrite so that the rewrite is
//! held to the same observable behaviour. The `fatal` cases run in a child copy of this test
//! binary (they write to descriptor 2 and kill the process), selected by an environment variable.
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::panic, missing_docs)]
use std::cell::Cell;
use std::os::unix::process::ExitStatusExt;
use std::process::Command;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use liella_boundary::sys::CSys;
use liella_kernel::errno::Errno;
use liella_kernel::machine::{Machine, X87Env};
use liella_kernel::sys::{Disposition, Sys};
use liella_kernel::testing::Faulty;
use liella_kernel::uapi::{SigSet, Timespec, SIGABRT, SIG_BLOCK};
use liella_rt::cpu::Features;
use liella_rt::futex::{self, Clock, Scope};
use liella_rt::sync::{RawCondvar, RawMutex, RawOnce, RawRwLock};
use liella_rt::thread::{DESC_SIZE, KEY_AREA, SLOT_AREA};
use liella_rt::{fatal, tls};

const LONG: Duration = Duration::from_secs(20);

/// Poll `f` until it holds; fail after [`LONG`].
fn eventually(what: &str, f: impl Fn() -> bool) {
    let t = Instant::now();
    while !f() {
        assert!(t.elapsed() < LONG, "timed out waiting for: {what}");
        std::thread::sleep(Duration::from_millis(1));
    }
}

// ---------------------------------------------------------------- futex

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn futex_wait_returns_eagain_when_the_word_differs() {
    let w = AtomicU32::new(5);
    for scope in [Scope::Private, Scope::Shared] {
        assert_eq!(futex::wait(&CSys, &w, 4, scope), Err(Errno::EAGAIN));
        assert_eq!(futex::wait_until(&CSys, &w, 4, scope, None), Err(Errno::EAGAIN));
    }
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn futex_wake_with_no_waiters_wakes_none() {
    let w = AtomicU32::new(0);
    for scope in [Scope::Private, Scope::Shared] {
        assert_eq!(futex::wake(&CSys, &w, 1, scope), Ok(0));
        assert_eq!(futex::wake_all(&CSys, &w, scope), Ok(0));
    }
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn futex_deadline_in_the_past_times_out_on_both_clocks() {
    let w = AtomicU32::new(0);
    let past = Timespec { sec: 1, nsec: 0 };
    for clock in [Clock::Monotonic, Clock::Realtime] {
        for scope in [Scope::Private, Scope::Shared] {
            assert_eq!(futex::wait_until(&CSys, &w, 0, scope, Some((clock, &past))), Err(Errno::ETIMEDOUT), "{clock:?} {scope:?}");
        }
    }
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn futex_deadline_is_absolute_on_its_clock() {
    // A deadline 50 ms ahead on CLOCK_MONOTONIC: the wait lasts about that long, not 50 ms
    // past the epoch (which would return at once) nor an unbounded time.
    let now = CSys.clock_gettime(1).unwrap();
    let mut d = Timespec { sec: now.sec, nsec: now.nsec + 50_000_000 };
    if d.nsec >= 1_000_000_000 {
        d.sec += 1;
        d.nsec -= 1_000_000_000;
    }
    let w = AtomicU32::new(0);
    let t = Instant::now();
    assert_eq!(futex::wait_until(&CSys, &w, 0, Scope::Private, Some((Clock::Monotonic, &d))), Err(Errno::ETIMEDOUT));
    let e = t.elapsed();
    assert!(e >= Duration::from_millis(40) && e < Duration::from_secs(10), "{e:?}");
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13-3): a `CLOCK_REALTIME`
/// deadline is read on the realtime clock. 50 ms past realtime-now is decades past the monotonic
/// clock's now, so reading it on the wrong clock never returns by itself; the test then wakes
/// the sleeper and fails instead of hanging.
#[test]
fn futex_deadline_realtime_is_read_on_the_realtime_clock() {
    let now = CSys.clock_gettime(0).unwrap();
    let mut d = Timespec { sec: now.sec, nsec: now.nsec + 50_000_000 };
    if d.nsec >= 1_000_000_000 {
        d.sec += 1;
        d.nsec -= 1_000_000_000;
    }
    let w = Arc::new(AtomicU32::new(0));
    let (tx, rx) = std::sync::mpsc::channel();
    let h = {
        let w = Arc::clone(&w);
        std::thread::spawn(move || {
            let mut r = futex::wait_until(&CSys, &w, 0, Scope::Private, Some((Clock::Realtime, &d)));
            // A wake from the rescue below is not the timeout under test.
            while r.is_ok() && w.load(Ordering::SeqCst) == 0 {
                r = futex::wait_until(&CSys, &w, 0, Scope::Private, Some((Clock::Realtime, &d)));
            }
            tx.send(r).unwrap();
        })
    };
    let got = rx.recv_timeout(LONG);
    if got.is_err() {
        w.store(1, Ordering::SeqCst);
        let _ = futex::wake_all(&CSys, &w, Scope::Private);
    }
    h.join().unwrap();
    assert_eq!(got.ok(), Some(Err(Errno::ETIMEDOUT)), "a CLOCK_REALTIME deadline 50 ms ahead did not expire");
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn futex_wake_counts_the_sleepers_it_woke() {
    for scope in [Scope::Private, Scope::Shared] {
        let w = Arc::new(AtomicU32::new(0));
        let hs: Vec<_> = (0..3)
            .map(|_| {
                let w = Arc::clone(&w);
                std::thread::spawn(move || while w.load(Ordering::Acquire) == 0 {
                    let _ = futex::wait(&CSys, &w, 0, scope);
                })
            })
            .collect();
        let mut woken = 0;
        let t = Instant::now();
        while woken < 3 {
            woken += futex::wake(&CSys, &w, 1, scope).unwrap();
            assert!(t.elapsed() < LONG, "sleepers never arrived");
        }
        // Each wake took a sleeper back to `wait` (the word is still 0), so all are asleep again.
        w.store(1, Ordering::Release);
        let t = Instant::now();
        let mut all = 0;
        while all < 3 && t.elapsed() < LONG {
            all += futex::wake_all(&CSys, &w, scope).unwrap();
            if hs.iter().all(|h| h.is_finished()) {
                break;
            }
        }
        for h in hs {
            h.join().unwrap();
        }
    }
}

// ---------------------------------------------------------------- RawMutex

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn mutex_try_lock_reset_and_hints() {
    let m = RawMutex::new();
    assert!(!m.is_locked() && !m.is_contended());
    assert!(m.try_lock());
    assert!(m.is_locked() && !m.is_contended());
    assert!(!m.try_lock());
    m.reset();
    assert!(!m.is_locked());
    assert!(m.try_lock());
    assert!(!m.release(), "nobody waited: no wake needed");
    assert!(!m.is_locked());
    {
        let _g = m.lock(&CSys);
        assert!(m.is_locked() && !m.try_lock());
    }
    assert!(!m.is_locked());
    m.lock_raw(&CSys, Scope::Shared);
    assert!(m.is_locked());
    m.unlock(&CSys, Scope::Shared);
    assert!(m.try_lock());
    assert!(!m.release());
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn mutex_release_reports_a_waiter_and_wake_one_lets_it_in() {
    let m = Arc::new(RawMutex::new());
    assert!(m.try_lock());
    let got = Arc::new(AtomicBool::new(false));
    let h = {
        let (m, got) = (Arc::clone(&m), Arc::clone(&got));
        std::thread::spawn(move || {
            m.lock_raw(&CSys, Scope::Private);
            got.store(true, Ordering::Release);
            m.unlock(&CSys, Scope::Private);
        })
    };
    eventually("the waiter marks the lock contended", || m.is_contended());
    // Give the waiter time to reach the futex (the contract holds either way).
    std::thread::sleep(Duration::from_millis(20));
    assert!(m.release(), "a waiter is recorded: the releaser must wake one");
    m.wake_one(&CSys, Scope::Private);
    h.join().unwrap();
    assert!(got.load(Ordering::Acquire));
    // Quiet again: an uncontended round needs no wake.
    assert!(m.try_lock());
    assert!(!m.release());
    assert!(!m.is_locked());
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn mutex_stress_mixed_entry_points_lose_no_update() {
    const THREADS: usize = 8;
    const ROUNDS: usize = 20_000;
    let m = Arc::new(RawMutex::new());
    let counter = Arc::new(AtomicUsize::new(0));
    let hs: Vec<_> = (0..THREADS)
        .map(|t| {
            let (m, counter) = (Arc::clone(&m), Arc::clone(&counter));
            std::thread::spawn(move || {
                for i in 0..ROUNDS {
                    match (t + i) % 4 {
                        0 => {
                            let _g = m.lock(&CSys);
                            let v = counter.load(Ordering::Relaxed);
                            counter.store(v + 1, Ordering::Relaxed);
                        }
                        1 => {
                            m.lock_raw(&CSys, Scope::Private);
                            let v = counter.load(Ordering::Relaxed);
                            counter.store(v + 1, Ordering::Relaxed);
                            m.unlock(&CSys, Scope::Private);
                        }
                        2 => {
                            while !m.try_lock() {
                                std::hint::spin_loop();
                            }
                            let v = counter.load(Ordering::Relaxed);
                            counter.store(v + 1, Ordering::Relaxed);
                            if m.release() {
                                m.wake_one(&CSys, Scope::Private);
                            }
                        }
                        _ => {
                            m.lock_raw(&CSys, Scope::Private);
                            let v = counter.load(Ordering::Relaxed);
                            std::thread::yield_now();
                            counter.store(v + 1, Ordering::Relaxed);
                            if m.release() {
                                m.wake_one(&CSys, Scope::Private);
                            }
                        }
                    }
                }
            })
        })
        .collect();
    for h in hs {
        h.join().unwrap();
    }
    assert_eq!(counter.load(Ordering::Relaxed), THREADS * ROUNDS);
    assert!(!m.is_locked());
    assert!(m.try_lock());
    assert!(!m.release(), "no waiter left behind");
}

// ---------------------------------------------------------------- RawOnce

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn once_runs_once_and_late_callers_see_its_effects() {
    let o = Arc::new(RawOnce::new());
    assert!(!o.is_done());
    let runs = Arc::new(AtomicUsize::new(0));
    let value = Arc::new(AtomicUsize::new(0));
    let hs: Vec<_> = (0..8)
        .map(|_| {
            let (o, runs, value) = (Arc::clone(&o), Arc::clone(&runs), Arc::clone(&value));
            std::thread::spawn(move || {
                o.call_once(&CSys, || {
                    runs.fetch_add(1, Ordering::Relaxed);
                    std::thread::sleep(Duration::from_millis(30));
                    value.store(42, Ordering::Relaxed);
                });
                assert!(o.is_done());
                assert_eq!(value.load(Ordering::Relaxed), 42, "call_once returned before the routine finished");
            })
        })
        .collect();
    for h in hs {
        h.join().unwrap();
    }
    assert_eq!(runs.load(Ordering::Relaxed), 1);
    o.call_once(&CSys, || panic!("ran twice"));
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13-2: an unwinding routine
/// leaves the object new and lets the caller sleeping on it run the routine itself).
#[test]
fn once_unwinding_routine_lets_a_waiting_caller_run_it() {
    let o = Arc::new(RawOnce::new());
    let started = Arc::new(AtomicBool::new(false));
    let first = {
        let (o, started) = (Arc::clone(&o), Arc::clone(&started));
        std::thread::spawn(move || {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                o.call_once(&CSys, || {
                    started.store(true, Ordering::Relaxed);
                    std::thread::sleep(Duration::from_millis(100));
                    panic!("init routine unwinds (expected)");
                })
            }))
            .is_err()
        })
    };
    eventually("first run started", || started.load(Ordering::Relaxed));
    let runs = AtomicUsize::new(0);
    o.call_once(&CSys, || {
        runs.fetch_add(1, Ordering::Relaxed);
    });
    assert!(first.join().unwrap(), "the first routine did not unwind");
    assert_eq!(runs.load(Ordering::Relaxed), 1, "the waiting caller did not run the routine after the unwind");
    assert!(o.is_done());
}

// ---------------------------------------------------------------- RawRwLock

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn rwlock_try_paths() {
    let l = RawRwLock::new();
    assert!(l.try_read() && l.try_read());
    assert!(!l.try_write());
    l.unlock(&CSys, Scope::Private);
    assert!(!l.try_write());
    l.unlock(&CSys, Scope::Private);
    assert!(l.try_write());
    assert!(!l.try_read() && !l.try_write());
    l.unlock(&CSys, Scope::Private);
    l.write(&CSys, Scope::Shared);
    assert!(!l.try_read());
    l.unlock(&CSys, Scope::Shared);
    l.read(&CSys, Scope::Shared);
    assert!(!l.try_write());
    l.unlock(&CSys, Scope::Shared);
    assert!(l.try_write());
    l.unlock(&CSys, Scope::Private);
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn rwlock_prefers_readers_while_a_writer_waits() {
    // D-22.4: readers share the lock whenever no writer holds it, even with a writer waiting.
    let l = Arc::new(RawRwLock::new());
    l.read(&CSys, Scope::Private);
    let done = Arc::new(AtomicBool::new(false));
    let h = {
        let (l, done) = (Arc::clone(&l), Arc::clone(&done));
        std::thread::spawn(move || {
            l.write(&CSys, Scope::Private);
            done.store(true, Ordering::Release);
            l.unlock(&CSys, Scope::Private);
        })
    };
    std::thread::sleep(Duration::from_millis(30));
    assert!(l.try_read(), "a waiting writer must not shut readers out");
    l.read(&CSys, Scope::Private);
    assert!(!done.load(Ordering::Acquire));
    for _ in 0..3 {
        l.unlock(&CSys, Scope::Private);
    }
    h.join().unwrap();
    assert!(done.load(Ordering::Acquire));
    assert!(l.try_write());
    l.unlock(&CSys, Scope::Private);
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn rwlock_stress_writers_exclude_everyone() {
    let l = Arc::new(RawRwLock::new());
    let inside = Arc::new(AtomicUsize::new(0));
    let writes = Arc::new(AtomicUsize::new(0));
    const W: usize = 1 << 20;
    let hs: Vec<_> = (0..8)
        .map(|t| {
            let (l, inside, writes) = (Arc::clone(&l), Arc::clone(&inside), Arc::clone(&writes));
            std::thread::spawn(move || {
                for i in 0..5_000 {
                    if (t + i) % 3 == 0 {
                        l.write(&CSys, Scope::Private);
                        assert_eq!(inside.swap(W, Ordering::AcqRel), 0, "writer met someone inside");
                        writes.fetch_add(1, Ordering::Relaxed);
                        assert_eq!(inside.swap(0, Ordering::AcqRel), W);
                    } else {
                        l.read(&CSys, Scope::Private);
                        assert!(inside.fetch_add(1, Ordering::AcqRel) < W, "reader met a writer");
                        inside.fetch_sub(1, Ordering::AcqRel);
                    }
                    l.unlock(&CSys, Scope::Private);
                }
            })
        })
        .collect();
    for h in hs {
        h.join().unwrap();
    }
    assert!(writes.load(Ordering::Relaxed) > 0);
    assert!(l.try_write());
    l.unlock(&CSys, Scope::Private);
}

// ---------------------------------------------------------------- RawCondvar

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn condvar_notify_all_wakes_every_waiter() {
    let m = Arc::new(RawMutex::new());
    let cv = Arc::new(RawCondvar::new());
    let flag = Arc::new(AtomicBool::new(false));
    let out = Arc::new(AtomicUsize::new(0));
    let hs: Vec<_> = (0..6)
        .map(|_| {
            let (m, cv, flag, out) = (Arc::clone(&m), Arc::clone(&cv), Arc::clone(&flag), Arc::clone(&out));
            std::thread::spawn(move || {
                m.lock_raw(&CSys, Scope::Private);
                while !flag.load(Ordering::Relaxed) {
                    cv.wait(&CSys, &m, Scope::Private);
                    assert!(m.is_locked(), "wait returns with the mutex held");
                }
                out.fetch_add(1, Ordering::Relaxed);
                m.unlock(&CSys, Scope::Private);
            })
        })
        .collect();
    std::thread::sleep(Duration::from_millis(30));
    m.lock_raw(&CSys, Scope::Private);
    flag.store(true, Ordering::Relaxed);
    cv.notify_all(&CSys, Scope::Private);
    m.unlock(&CSys, Scope::Private);
    for h in hs {
        h.join().unwrap();
    }
    assert_eq!(out.load(Ordering::Relaxed), 6);
    assert!(!m.is_locked());
}

static SIG_M: RawMutex = RawMutex::new();
static SIG_CV: RawCondvar = RawCondvar::new();

extern "C" fn notify_from_handler(_sig: i32) {
    SIG_CV.notify_all(&CSys, Scope::Private);
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13-3): a notify that lands
/// after `wait` released the mutex is not lost, whether the waiter is still on its way to the
/// sleep or already in it. The notify runs in a signal handler on the waiter's own thread
/// (`SA_RESTART`), so the waiter's sleep starts, or restarts, only after the notify: it must
/// return because of that notify, with no later wake-up to rescue it. The test wakes a waiter
/// that missed it and fails instead of hanging.
#[test]
fn condvar_notify_between_release_and_sleep_is_not_lost() {
    let tid = Arc::new(std::sync::atomic::AtomicI32::new(0));
    let done = Arc::new(AtomicBool::new(false));
    let lost = liella_boundary::selftest::with_handler_and_flags(liella_kernel::uapi::SIGUSR1, notify_from_handler, liella_kernel::uapi::SA_RESTART, || {
        let h = {
            let (tid, done) = (Arc::clone(&tid), Arc::clone(&done));
            std::thread::spawn(move || {
                SIG_M.lock_raw(&CSys, Scope::Private);
                tid.store(CSys.gettid(), Ordering::SeqCst);
                SIG_CV.wait(&CSys, &SIG_M, Scope::Private);
                done.store(true, Ordering::SeqCst);
                SIG_M.unlock(&CSys, Scope::Private);
            })
        };
        eventually("the waiter holds the mutex", || tid.load(Ordering::SeqCst) != 0);
        // The waiter gives the mutex up only inside wait, after it has read the condition.
        eventually("wait releases the mutex", || SIG_M.try_lock());
        CSys.tgkill(CSys.getpid(), tid.load(Ordering::SeqCst), liella_kernel::uapi::SIGUSR1).unwrap();
        SIG_M.unlock(&CSys, Scope::Private);
        let t = Instant::now();
        while !done.load(Ordering::SeqCst) && t.elapsed() < LONG {
            std::thread::sleep(Duration::from_millis(1));
        }
        let lost = !done.load(Ordering::SeqCst);
        if lost {
            SIG_CV.notify_all(&CSys, Scope::Private);
        }
        h.join().unwrap();
        lost
    })
    .unwrap();
    assert!(!lost, "condvar_notify_between_release_and_sleep_is_not_lost: the notify was lost");
    assert!(!SIG_M.is_locked());
}

// ---------------------------------------------------------------- cpu

/// A CPU that answers from a table and counts the questions.
struct FakeCpu {
    max: u32,
    max_ext: u32,
    l1: [u32; 2],
    l7: [u32; 2],
    e1: u32,
    xcr0: Option<u64>,
    calls: Cell<u32>,
}

impl FakeCpu {
    fn new(l1: [u32; 2], l7: [u32; 2], e1: u32, xcr0: Option<u64>) -> FakeCpu {
        FakeCpu { max: 0xd, max_ext: 0x8000_0008, l1, l7, e1, xcr0, calls: Cell::new(0) }
    }
}

impl Machine for FakeCpu {
    fn thread_pointer(&self) -> usize { 0 }
    fn cpuid(&self, leaf: u32, subleaf: u32) -> [u32; 4] {
        self.calls.set(self.calls.get() + 1);
        match (leaf, subleaf) {
            (0, _) => [self.max, 0, 0, 0],
            (1, _) if self.max >= 1 => [0, 0, self.l1[0], self.l1[1]],
            (7, 0) if self.max >= 7 => [0, self.l7[0], self.l7[1], 0],
            (0x8000_0000, _) => [self.max_ext, 0, 0, 0],
            (0x8000_0001, _) if self.max_ext >= 0x8000_0001 => [0, 0, self.e1, 0],
            // Out-of-range leaves answer garbage on real parts.
            _ => [u32::MAX; 4],
        }
    }
    fn xcr0(&self) -> Option<u64> { self.xcr0 }
    fn mxcsr(&self) -> u32 { 0x1f80 }
    fn set_mxcsr(&self, _: u32) {}
    fn x87_control(&self) -> u16 { 0x37f }
    fn set_x87_control(&self, _: u16) {}
    fn x87_status(&self) -> u16 { 0 }
    fn x87_clear_exceptions(&self) {}
    fn x87_store_env(&self) -> X87Env { X87Env::default() }
    fn x87_load_env(&self, _: &X87Env) {}
    fn pkru(&self) -> Option<u32> { None }
    fn set_pkru(&self, _: u32) -> bool { false }
    fn barrier(&self, _: &[u8]) {}
}

const OSXSAVE: u32 = 1 << 27;
const AVX_STATE: u64 = 0b110;
const AVX512_STATE: u64 = 0b1110_0110;

/// (name, register index 0..4 = leaf1 ecx, leaf1 edx, leaf7 ebx, ext1 ecx, bit, needs AVX state,
/// needs AVX-512 state).
type Probe = (&'static str, fn(&Features) -> bool, usize, u32, bool);
const BITS: &[Probe] = &[
    ("sse3", Features::sse3, 0, 0, false),
    ("ssse3", Features::ssse3, 0, 9, false),
    ("fma", Features::fma, 0, 12, true),
    ("cx16", Features::cx16, 0, 13, false),
    ("sse41", Features::sse41, 0, 19, false),
    ("sse42", Features::sse42, 0, 20, false),
    ("movbe", Features::movbe, 0, 22, false),
    ("popcnt", Features::popcnt, 0, 23, false),
    ("avx", Features::avx, 0, 28, true),
    ("f16c", Features::f16c, 0, 29, true),
    ("sse2", Features::sse2, 1, 26, false),
    ("bmi1", Features::bmi1, 2, 3, false),
    ("avx2", Features::avx2, 2, 5, true),
    ("bmi2", Features::bmi2, 2, 8, false),
    ("erms", Features::erms, 2, 9, false),
    ("lahf", Features::lahf, 3, 0, false),
    ("lzcnt", Features::lzcnt, 3, 5, false),
];

fn regs_with(reg: usize, bit: u32) -> ([u32; 2], [u32; 2], u32) {
    let mut r = [0u32; 4];
    r[reg] = 1 << bit;
    ([r[0] | OSXSAVE, r[1]], [r[2], 0], r[3])
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn cpu_each_feature_reads_its_own_bit() {
    for &(name, f, reg, bit, needs_state) in BITS {
        let (l1, l7, e1) = regs_with(reg, bit);
        let on = Features::detect(&FakeCpu::new(l1, l7, e1, Some(AVX512_STATE)));
        assert!(f(&on), "{name} with its bit set");
        let others: Vec<_> = BITS.iter().filter(|p| p.0 != name && (p.1)(&on)).map(|p| p.0).collect();
        assert!(others.is_empty(), "{name}'s bit also turned on {others:?}");
        let off = Features::detect(&FakeCpu::new([OSXSAVE, 0], [0, 0], 0, Some(AVX512_STATE)));
        assert!(!f(&off), "{name} with no bit");
        let stateless = Features::detect(&FakeCpu::new(l1, l7, e1, None));
        assert_eq!(f(&stateless), !needs_state, "{name} without OS-enabled AVX state");
        let sse_only = Features::detect(&FakeCpu::new(l1, l7, e1, Some(0b10)));
        assert_eq!(f(&sse_only), !needs_state, "{name} with XCR0 = SSE only");
    }
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn cpu_keeps_the_raw_registers() {
    let cpu = FakeCpu::new([0x1234_5678 | OSXSAVE, 0x9abc_def0], [0x0fed_cba9, 0x8765_4321], 0x5555_aaaa, Some(0xe7));
    let f = Features::detect(&cpu);
    assert_eq!((f.leaf1_ecx, f.leaf1_edx, f.leaf7_ebx, f.leaf7_ecx, f.ext1_ecx, f.xcr0), (0x1234_5678 | OSXSAVE, 0x9abc_def0, 0x0fed_cba9, 0x8765_4321, 0x5555_aaaa, 0xe7));
    assert_eq!(Features::detect(&FakeCpu::new([0, 0], [0, 0], 0, None)).xcr0, 0, "no xgetbv: XCR0 reads 0");
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13-3): leaf 7 is asked only
/// when the highest basic leaf reaches 7. Below that a part answers garbage for leaf 7 (the fake
/// answers all ones), which must not turn on any leaf-7 feature.
#[test]
fn cpu_leaf7_is_ignored_below_its_maximum_leaf() {
    for max in [1, 6] {
        let mut cpu = FakeCpu::new([OSXSAVE, 0], [u32::MAX, u32::MAX], 0, Some(AVX512_STATE));
        cpu.max = max;
        let f = Features::detect(&cpu);
        assert_eq!((f.leaf7_ebx, f.leaf7_ecx), (0, 0), "cpu_leaf7: max leaf {max}");
        assert!(!f.bmi1() && !f.bmi2() && !f.erms() && !f.avx2(), "cpu_leaf7: max leaf {max}");
    }
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn cpu_asks_few_questions() {
    // Startup cost (STARTUP-COST: the whole process makes ~12 cpuid): detection makes at most
    // five queries (leaf 0, 1, 7, 0x8000_0000, 0x8000_0001).
    let cpu = FakeCpu::new([u32::MAX, u32::MAX], [u32::MAX, u32::MAX], u32::MAX, Some(AVX512_STATE));
    let _ = Features::detect(&cpu);
    assert!(cpu.calls.get() <= 5, "{} cpuid calls", cpu.calls.get());
}

const V2: [(usize, u32); 7] = [(0, 0), (0, 9), (0, 13), (0, 19), (0, 20), (0, 23), (3, 0)];
const V3: [(usize, u32); 8] = [(0, 12), (0, 22), (0, 28), (0, 29), (2, 3), (2, 5), (2, 8), (3, 5)];
const V4: [(usize, u32); 5] = [(2, 16), (2, 17), (2, 28), (2, 30), (2, 31)];

fn cpu_with(bits: &[(usize, u32)], xcr0: Option<u64>) -> Features {
    let mut r = [0u32; 4];
    r[1] = 1 << 26; // SSE2
    for &(reg, bit) in bits {
        r[reg] |= 1 << bit;
    }
    Features::detect(&FakeCpu::new([r[0] | OSXSAVE, r[1]], [r[2], 0], r[3], xcr0))
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn cpu_levels_need_every_member_and_the_os_state() {
    let all: Vec<_> = V2.iter().chain(&V3).chain(&V4).copied().collect();
    assert_eq!(cpu_with(&[], Some(AVX512_STATE)).level(), 1);
    assert_eq!(cpu_with(&all, Some(AVX512_STATE)).level(), 4);
    assert!(cpu_with(&all, Some(AVX512_STATE)).avx512_v4());
    assert_eq!(cpu_with(&all, Some(AVX_STATE | 1)).level(), 3, "no AVX-512 state");
    assert!(!cpu_with(&all, Some(AVX_STATE | 1)).avx512_v4());
    assert_eq!(cpu_with(&all, None).level(), 2, "no AVX state");
    let upto: Vec<_> = V2.iter().chain(&V3).copied().collect();
    assert_eq!(cpu_with(&upto, Some(AVX512_STATE)).level(), 3);
    assert_eq!(cpu_with(&V2, Some(AVX512_STATE)).level(), 2);
    for skip in 0..all.len() {
        let less: Vec<_> = all.iter().enumerate().filter(|&(i, _)| i != skip).map(|(_, &b)| b).collect();
        let want = if skip < 7 { 1 } else if skip < 15 { 2 } else { 3 };
        assert_eq!(cpu_with(&less, Some(AVX512_STATE)).level(), want, "without {:?}", all[skip]);
    }
    // Each AVX-512 state component is needed.
    for c in [5, 6, 7] {
        assert_eq!(cpu_with(&all, Some(AVX512_STATE & !(1 << c))).level(), 3, "XCR0 bit {c} clear");
    }
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn cpu_detect_on_this_machine_is_consistent() {
    let f = Features::detect(&CSys);
    assert!(f.sse2(), "x86-64 baseline");
    assert!((1..=4).contains(&f.level()));
    if f.level() >= 3 {
        assert!(f.avx2() && f.avx());
    }
}

// ---------------------------------------------------------------- tls

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn tls_fixed_offsets() {
    assert_eq!(tls::BLOCK_SIZE, 0x300);
    assert_eq!(tls::ERRNO_OFFSET, 0x80);
    assert_eq!(tls::H_ERRNO_OFFSET, 0x128);
    assert_eq!(tls::STACK_GUARD_OFFSET, 0x28);
    assert_eq!(tls::RSEQ_OFFSET, tls::BLOCK_SIZE + liella_rt::thread::RSEQ_IN_DESC);
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn tls_block_size_rounds_to_the_alignment() {
    let img = |memsz, align| tls::TlsImage { addr: 0, filesz: 0, memsz, align };
    assert_eq!(img(100, 16).block_size(), Some(112));
    assert_eq!(img(100, 0).block_size(), Some(100));
    assert_eq!(img(100, 1).block_size(), Some(100));
    assert_eq!(img(0, 64).block_size(), Some(0));
    assert_eq!(img(64, 64).block_size(), Some(64));
    assert_eq!(img(usize::MAX, 4096).block_size(), None);
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn tls_area_property() {
    let tail = tls::BLOCK_SIZE + DESC_SIZE + KEY_AREA + SLOT_AREA;
    for align in [0usize, 1, 2, 8, 16, 32, 64, 128, 4096, 8192, 65536] {
        for memsz in [0usize, 1, 63, 64, 100, 5000, 70_000] {
            let img = tls::TlsImage { addr: 0x40_0000, filesz: memsz / 2, memsz, align };
            let a = tls::InitialArea::new(Some(&img)).unwrap();

            let size = img.block_size().unwrap();
            for base in [0x7f00_0000_0000usize, 0x7f00_0000_1000, 0x7f00_0000_3000, 0x7f00_0001_f000] {
                let (tp, start) = a.place(base).unwrap();
                assert_eq!(tp % align.max(64), 0, "align {align} memsz {memsz} base {base:#x}");
                assert_eq!(tp - start, size);
                assert!(start >= base);
                assert!(tp + tail <= base + a.len, "align {align} memsz {memsz}: the descriptor and key area fit");
            }
        }
    }
    let none = tls::InitialArea::new(None).unwrap();
    assert_eq!(none.place(0x5000).unwrap(), (0x5000, 0x5000));
    assert!(none.len >= tail);
    assert_eq!(tls::InitialArea::new(Some(&tls::TlsImage { addr: 0, filesz: 0, memsz: usize::MAX - 100, align: 64 })), None);
    assert_eq!(tls::InitialArea::new(Some(&tls::TlsImage { addr: 0, filesz: 0, memsz: 8, align: 48 })), None);
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn tls_guards_use_both_halves_of_at_random() {
    let mut r = [0u8; 16];
    r[0] = 0xff;
    r[7] = 0x80;
    r[8] = 1;
    r[15] = 0x7f;
    let (c, p) = tls::guards(&r);
    assert_eq!(c, 0x8000_0000_0000_0000);
    assert_eq!(p, 0x7f00_0000_0000_0001);
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13-3): only the canary's
/// lowest byte is forced to zero; every other byte comes from its own `AT_RANDOM` byte.
#[test]
fn tls_guards_canary_keeps_each_random_byte_in_place() {
    let r: [u8; 16] = [0xa0, 0xa1, 0xa2, 0xa3, 0xa4, 0xa5, 0xa6, 0xa7, 0xb0, 0xb1, 0xb2, 0xb3, 0xb4, 0xb5, 0xb6, 0xb7];
    let (c, p) = tls::guards(&r);
    assert_eq!(c, 0xa7a6_a5a4_a3a2_a100, "tls_guards_canary: canary");
    assert_eq!(p, 0xb7b6_b5b4_b3b2_b1b0, "tls_guards_canary: pointer guard");
}

// ---------------------------------------------------------------- fatal (in a child)

const CHILD: &str = "LIELLA_CORE_CONTRACT_CHILD";

/// Run `case` in a child copy of this binary: (stderr, exit code, terminating signal).
fn child(case: &str) -> (Vec<u8>, Option<i32>, Option<i32>) {
    let out = Command::new(std::env::current_exe().unwrap())
        .args(["fatal_child", "--exact", "--nocapture", "--test-threads=1", "-q"])
        .env(CHILD, case)
        .output()
        .unwrap();
    (out.stderr, out.status.code(), out.status.signal())
}

/// The child side: does nothing unless the variable names a case.
/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn fatal_child() {
    let Ok(case) = std::env::var(CHILD) else { return };
    let s = &CSys;
    match case.as_str() {
        "message" => fatal::message(s, &[b"a", b"bc", b"", b"def\n"]),
        "big" => {
            let part = [b'x'; 1000];
            let parts: Vec<&[u8]> = (0..300).map(|_| &part[..]).collect();
            fatal::message(s, &parts);
        }
        "assert" => fatal::assert_message(s, b"prog", b"x == 1", b"f.c", 12, Some(b"main")),
        "assert_bare" => fatal::assert_message(s, b"", b"0", b"g.c", 4_000_000_000, None),
        "perror" => fatal::assert_perror_message(s, b"prog", b"No such file or directory", b"h.c", 7, Some(b"fn")),
        "perror_bare" => fatal::assert_perror_message(s, b"", b"Bad", b"h.c", 0, None),
        "abort" => fatal::abort(s, || {}),
        "abort_blocked" => {
            s.sigprocmask(SIG_BLOCK, Some(&SigSet::empty().with(SIGABRT))).unwrap();
            fatal::abort(s, || {})
        }
        "abort_ignored" => {
            s.sigaction_set(SIGABRT, Disposition::Ignore, 0, SigSet::empty()).unwrap();
            fatal::abort(s, || {})
        }
        "fatal" => fatal::fatal(s, &[b"*** ", b"boom", b" ***: terminated\n"], || {}),
        "eintr" => {
            let f = Faulty::new(CSys);
            assert!(f.fail("writev", Errno::EINTR, 0, 3));
            fatal::message(&f, &[b"ab", b"", b"c\n"]);
            assert_eq!(f.injected(), 3);
        }
        "error" => {
            let f = Faulty::new(CSys);
            assert!(f.fail("writev", Errno::EIO, 0, 1));
            fatal::message(&f, &[b"never\n"]);
            assert_eq!(f.injected(), 1);
        }
        _ => panic!("unknown case {case}"),
    }
    s.exit_group(0);
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn fatal_message_writes_parts_in_order() {
    let (err, code, _) = child("message");
    assert_eq!((err.as_slice(), code), (&b"abcdef\n"[..], Some(0)));
    let (err, code, _) = child("big");
    assert_eq!(code, Some(0));
    assert_eq!(err.len(), 300_000);
    assert!(err.iter().all(|&b| b == b'x'));
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13-2: `EINTR` retries the
/// write, any other error gives up without writing).
#[test]
fn fatal_message_retries_eintr_and_gives_up_on_errors() {
    let (err, code, _) = child("eintr");
    assert_eq!((err.as_slice(), code), (&b"abc\n"[..], Some(0)));
    let (err, code, _) = child("error");
    assert_eq!((err.as_slice(), code), (&b""[..], Some(0)));
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn fatal_assert_texts() {
    assert_eq!(child("assert").0, b"prog: f.c:12: main: Assertion `x == 1' failed.\n");
    assert_eq!(child("assert_bare").0, b"g.c:4000000000: Assertion `0' failed.\n");
    assert_eq!(child("perror").0, b"prog: h.c:7: fn: Unexpected error: No such file or directory.\n");
    assert_eq!(child("perror_bare").0, b"h.c:0: Unexpected error: Bad.\n");
}

/// Internal-primitive contract (no spec clause; COLD-REWRITE-W-far-13 equivalence).
#[test]
fn fatal_abort_dies_by_sigabrt() {
    // no spec clause here: an internal contract of rt::fatal (abort itself is §1.2).
    for case in ["abort", "abort_blocked", "abort_ignored"] {
        let (err, code, sig) = child(case);
        assert_eq!((code, sig), (None, Some(SIGABRT)), "{case}: {}", String::from_utf8_lossy(&err));
    }
    let (err, _, sig) = child("fatal");
    assert_eq!(sig, Some(SIGABRT));
    assert_eq!(err, b"*** boom ***: terminated\n");
}
