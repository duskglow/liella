//! `SIGEV_THREAD` notifications run as if on a thread created with `sigev_notify_attributes`
//! (POSIX 2.4.2; owner ruling 2026-10-04): POSIX timers (§23.4, D-23.3), message-queue
//! notifications (§25.3, D-25.5) and asynchronous I/O (§11.11, D-11.11). Each notification
//! observes its own thread from inside (`pthread_getattr_np`, the address of a local): a
//! requested stack size is honoured, a caller-supplied stack is the one used, and the thread is
//! detached even when the attributes ask for a joinable one (`pthread_detach` on it fails with
//! `EINVAL`). The attribute objects are destroyed right after registration (they are copied).
//!
//! A caller-supplied stack serves one notification at a time on every path (D-23.3, D-25.5,
//! D-11.11; owner, 2026-10-04): two timers made with one stack, a message-queue function that
//! registers itself again and is still running when the next message arrives, and AIO requests
//! completing together (on io_uring and on the workers), alone or as a `lio_listio` list with a
//! list notification on the same stack, never run two functions on the stack at once, and every
//! call runs on it. Each such call checks an in-use count and a stack-canary word (a local given
//! a value unique to the call, which another thread on the stack would overwrite).
//!
//! The notifications are waited for by their effects (a call counter), never by time. Each case
//! writes its own slot of the shared records (the event's value names the slot), so the tests
//! may run in parallel; the message-queue tests run in a private IPC namespace (`unshare`), as
//! `s25_ipc_clauses.rs` does, and the AIO stack cases in a fresh process per engine. A
//! caller-supplied stack is leaked: a detached thread may still be ending on it when the test
//! returns.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::as_conversions, clippy::arithmetic_side_effects, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/scratch.rs"]
mod scratch;

#[path = "support/timeout.rs"]
mod timeout;

use core::ffi::c_void;
use std::process::Command;
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering::SeqCst};
use std::time::{Duration, Instant};

use liella_abi::ctypes::c_int;
use liella_abi::layout::pthread_attr_t;
use liella_boundary::posix::s01_startup as st;
use liella_boundary::posix::s11_lowio as io;
use liella_boundary::posix::s18_processes as pr;
use liella_boundary::posix::s22_threads as th;
use liella_boundary::posix::s23_time as tm;
use liella_boundary::posix::s25_ipc as mq;
use liella_kernel::uapi::{Itimerspec, MqAttr, SigEvent, Timespec};

use scratch::Scratch;

const EINVAL: c_int = 22;
const DETACHED: c_int = 1;
const JOINABLE: c_int = 0;
const CLOCK_MONOTONIC: i32 = 1;
const SIGEV_THREAD: c_int = 2;
/// A stack size above any default (`RLIMIT_STACK`, 8 MiB here): seeing it proves it was taken.
const BIG_STACK: usize = 24 << 20;
/// A caller-supplied stack.
const OWN_STACK: usize = 512 << 10;

/// What one notification case saw (a slot per case).
struct Seen {
    calls: AtomicU32,
    running: AtomicU32,
    max_running: AtomicU32,
    detachstate: AtomicI32,
    detach_result: AtomicI32,
    stack_lo: AtomicUsize,
    stack_size: AtomicUsize,
    /// The bounds the stack's locals must fall in (0, 0: not checked).
    want_lo: AtomicUsize,
    want_hi: AtomicUsize,
    /// Calls whose local was outside the bounds.
    outside: AtomicU32,
    /// Calls whose stack-canary word changed while they ran.
    clobbered: AtomicU32,
}

impl Seen {
    const fn new() -> Seen {
        Seen {
            calls: AtomicU32::new(0),
            running: AtomicU32::new(0),
            max_running: AtomicU32::new(0),
            detachstate: AtomicI32::new(-1),
            detach_result: AtomicI32::new(-1),
            stack_lo: AtomicUsize::new(0),
            stack_size: AtomicUsize::new(0),
            want_lo: AtomicUsize::new(0),
            want_hi: AtomicUsize::new(0),
            outside: AtomicU32::new(0),
            clobbered: AtomicU32::new(0),
        }
    }
}

const TIMER_SIZED: usize = 0;
const TIMER_OWN: usize = 1;
const MQ_SIZED: usize = 2;
const MQ_OWN: usize = 3;
const AIO_SIZED: usize = 4;
const AIO_OWN: usize = 5;
const MQ_AGAIN: usize = 6;
const AIO_TOGETHER: usize = 7;
const LIO_TOGETHER: usize = 8;

static SEEN: [Seen; 9] = [const { Seen::new() }; 9];

/// The notification function, `void f(union sigval)`: the value is the slot. It uses only the
/// library's exports and atomics (it runs on a thread the library made).
extern "C" fn observe(slot: usize) {
    let Some(s) = SEEN.get(slot) else { return };
    let canary = AtomicU64::new(0);
    let token = enter(s, &canary);
    let mut a = pthread_attr_t { __size: [0; 56] };
    if th::pthread_getattr_np(th::pthread_self(), &mut a) == 0 {
        let mut ds = -1;
        th::pthread_attr_getdetachstate(&a, Some(&mut ds));
        s.detachstate.store(ds, SeqCst);
        let (mut addr, mut size) = (core::ptr::null_mut(), 0usize);
        th::pthread_attr_getstack(&a, Some(&mut addr), Some(&mut size));
        s.stack_lo.store(addr.addr(), SeqCst);
        s.stack_size.store(size, SeqCst);
        th::pthread_attr_destroy(&mut a);
    }
    s.detach_result.store(th::pthread_detach(th::pthread_self()), SeqCst);
    // A little work on the stack, so successive notifications overlap if they could.
    let mut spin = [0u8; 4096];
    for (i, b) in spin.iter_mut().enumerate() {
        *b = i as u8;
    }
    core::hint::black_box(&spin);
    leave(s, &canary, token);
}

/// Values of the stack-canary words, one per call.
static TOKENS: AtomicU64 = AtomicU64::new(0x57ac_ca9a_0000_0000);

/// A notification function's start: counted as running (the stack's in-use count), its
/// stack-canary word (a local of the function, `canary`) given a value unique to the call, and
/// the word's address checked against the stack. Another thread running on the same stack at
/// once lays its frames over the same addresses and so overwrites the word, which [`leave`]
/// sees. Returns the value.
fn enter(s: &Seen, canary: &AtomicU64) -> u64 {
    let token = TOKENS.fetch_add(1, SeqCst);
    core::hint::black_box(canary).store(token, SeqCst);
    let here = core::ptr::from_ref(canary).addr();
    let running = s.running.fetch_add(1, SeqCst) + 1;
    s.max_running.fetch_max(running, SeqCst);
    let (lo, hi) = (s.want_lo.load(SeqCst), s.want_hi.load(SeqCst));
    if hi != 0 && !(lo..hi).contains(&here) {
        s.outside.fetch_add(1, SeqCst);
    }
    token
}

/// A notification function's end: its stack-canary word must still hold `token`.
fn leave(s: &Seen, canary: &AtomicU64, token: u64) {
    if core::hint::black_box(canary).load(SeqCst) != token {
        s.clobbered.fetch_add(1, SeqCst);
    }
    s.running.fetch_sub(1, SeqCst);
    s.calls.fetch_add(1, SeqCst);
}

fn observe_addr() -> u64 {
    observe as extern "C" fn(usize) as usize as u64
}

/// Wait (at most 60 s, load-scaled) for `done`, polling: the notification's effect, not a time, ends it.
fn until(done: impl Fn() -> bool) -> bool {
    let dl = timeout::deadline(Duration::from_secs(60));
    while !done() && Instant::now() < dl {
        std::thread::sleep(Duration::from_millis(1));
    }
    done()
}

/// An attribute object asking for `size` bytes of stack and a joinable thread.
fn sized_joinable(size: usize) -> Box<pthread_attr_t> {
    let mut a = Box::new(pthread_attr_t { __size: [0; 56] });
    assert_eq!(th::pthread_attr_init(&mut *a), 0);
    assert_eq!(th::pthread_attr_setstacksize(&mut *a, size), 0);
    assert_eq!(th::pthread_attr_setdetachstate(&mut *a, JOINABLE), 0);
    a
}

/// An attribute object with a caller-supplied stack (leaked: see the module documentation),
/// joinable; the slot's bounds are set to the stack.
fn own_stack(slot: usize) -> (Box<pthread_attr_t>, usize) {
    let mem: &'static mut [u8] = Vec::leak(vec![0u8; OWN_STACK + 4096]);
    let lo = mem.as_mut_ptr().addr().next_multiple_of(4096);
    let ptr: *mut c_void = mem.as_mut_ptr().with_addr(lo).cast();
    let mut a = Box::new(pthread_attr_t { __size: [0; 56] });
    assert_eq!(th::pthread_attr_init(&mut *a), 0);
    assert_eq!(th::pthread_attr_setstack(&mut *a, ptr, OWN_STACK), 0);
    assert_eq!(th::pthread_attr_setdetachstate(&mut *a, JOINABLE), 0);
    SEEN[slot].want_lo.store(lo, SeqCst);
    SEEN[slot].want_hi.store(lo + OWN_STACK, SeqCst);
    (a, lo)
}

fn attr_word(a: &mut pthread_attr_t) -> u64 {
    core::ptr::from_mut(a).expose_provenance() as u64
}

/// The common checks: detached (and not detachable again), the stack as requested.
fn check(slot: usize, what: &str, stack: Option<usize>) {
    let s = &SEEN[slot];
    assert_eq!(s.detachstate.load(SeqCst), DETACHED, "{what}: detached although the attributes ask for joinable");
    assert_eq!(s.detach_result.load(SeqCst), EINVAL, "{what}: pthread_detach on it fails (already detached)");
    match stack {
        Some(lo) => {
            assert_eq!(s.outside.load(SeqCst), 0, "{what}: every call ran on the caller's stack");
            assert_eq!((s.stack_lo.load(SeqCst), s.stack_size.load(SeqCst)), (lo, OWN_STACK), "{what}: pthread_getattr_np reports the caller's stack");
        }
        None => assert!(s.stack_size.load(SeqCst) >= BIG_STACK, "{what}: the requested stack size ({} bytes seen)", s.stack_size.load(SeqCst)),
    }
}

fn its(value_ns: i64, interval_ns: i64) -> Itimerspec {
    let t = |n: i64| Timespec { sec: n / 1_000_000_000, nsec: n % 1_000_000_000 };
    Itimerspec { interval: t(interval_ns), value: t(value_ns) }
}

fn timer(ev: &SigEvent) -> *mut c_void {
    let mut t = core::ptr::null_mut();
    assert_eq!(tm::timer_create(CLOCK_MONOTONIC, Some(ev), Some(&mut t)), 0);
    t
}

#[test]
fn timer_notification_attributes() {
    // Clauses: s23_4_c_3_3, s23_4_c_3 (D-23.3).
    // A requested stack size, and joinable: honoured, and detached.
    let mut a = sized_joinable(BIG_STACK);
    let t = timer(&SigEvent::thread(TIMER_SIZED as u64, observe_addr(), attr_word(&mut a)));
    assert_eq!(th::pthread_attr_destroy(&mut *a), 0);
    drop(a);
    assert_eq!(tm::timer_settime(t, 0, Some(&its(1_000_000, 0)), None), 0);
    assert!(until(|| SEEN[TIMER_SIZED].calls.load(SeqCst) == 1), "notified");
    check(TIMER_SIZED, "timer, stack size", None);
    assert_eq!(tm::timer_delete(t), 0);

    // A caller-supplied stack serves every notification of the timer, one at a time: the next
    // starts only once the previous thread has left the stack (D-23.3).
    // Two timers made with the one attribute object share the stack: still one at a time.
    let (mut a, lo) = own_stack(TIMER_OWN);
    let t = timer(&SigEvent::thread(TIMER_OWN as u64, observe_addr(), attr_word(&mut a)));
    let t2 = timer(&SigEvent::thread(TIMER_OWN as u64, observe_addr(), attr_word(&mut a)));
    assert_eq!(th::pthread_attr_destroy(&mut *a), 0);
    drop(a);
    assert_eq!(tm::timer_settime(t, 0, Some(&its(1_000_000, 200_000)), None), 0);
    assert_eq!(tm::timer_settime(t2, 0, Some(&its(1_100_000, 200_000)), None), 0);
    assert!(until(|| SEEN[TIMER_OWN].calls.load(SeqCst) >= 40), "notified again and again");
    for t in [t, t2] {
        assert_eq!(tm::timer_settime(t, 0, Some(&its(0, 0)), None), 0);
        assert_eq!(tm::timer_delete(t), 0);
    }
    check(TIMER_OWN, "timer, caller's stack", Some(lo));
    assert_eq!(SEEN[TIMER_OWN].max_running.load(SeqCst), 1, "one notification at a time on the stack");
    assert_eq!(SEEN[TIMER_OWN].clobbered.load(SeqCst), 0, "no call's stack-canary word overwritten by another on the stack");
}

const ALONE: &str = "LIELLA_SIGEV_ALONE";

/// Run `body` in this test re-executed as a process of its own (for a test that changes
/// process-wide state, such as whether threads can be created).
/// Returns the child's standard error in the parent, `None` in the child itself (which has run
/// `body` and has nothing more to check).
fn alone(test: &str, body: impl FnOnce()) -> Option<String> {
    if std::env::var(ALONE).as_deref() == Ok(test) {
        // This process runs this one test alone: `forbid_threads` may be called in it.
        liella_boundary::selftest::mark_own_process();
        body();
        return None;
    }
    let o = Command::new(std::env::current_exe().unwrap()).args([test, "--exact", "--test-threads=1", "--nocapture"]).env(ALONE, test).output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    let err = String::from_utf8_lossy(&o.stderr).into_owned();
    assert!(o.status.success() && out.contains("1 passed"), "{test} in its own process: {:?}\n{out}{err}", o.status);
    Some(err)
}

/// The diagnostic channel's `timers` lines in a child's standard error (D-2.9).
fn timer_diag_lines(err: &str) -> Vec<&str> {
    err.lines().filter(|l| l.contains("liella: timers: ")).collect()
}

/// The timer of [`timer_expiries_without_a_thread`], its calls and its first call's overrun.
static LOST_TIMER: AtomicUsize = AtomicUsize::new(0);
static LOST_CALLS: AtomicU32 = AtomicU32::new(0);
static LOST_OVERRUN: AtomicI32 = AtomicI32::new(-1);

extern "C" fn lost(_: usize) {
    let t = core::ptr::without_provenance_mut::<c_void>(LOST_TIMER.load(SeqCst));
    let _ = LOST_OVERRUN.compare_exchange(-1, tm::timer_getoverrun(t), SeqCst, SeqCst);
    LOST_CALLS.fetch_add(1, SeqCst);
}

#[test]
fn timer_expiries_without_a_thread() {
    // Clauses: s23_4_c_3_3, s23_4_c_3 (D-23.3).
    // Expirations whose notification thread could not be created are not lost: once a thread
    // can be made again, the next notification stands for them (its overrun counts them). The
    // failure is injected (`forbid_threads`, as a thread limit would), in a process of its own.
    alone("timer_expiries_without_a_thread", || {
        let t = timer(&SigEvent::thread(0, lost as extern "C" fn(usize) as usize as u64, 0));
        LOST_TIMER.store(t.addr(), SeqCst);
        // The helper was made with the timer; from here no notification thread can be.
        liella_boundary::selftest::forbid_threads();
        let before = liella_boundary::selftest::refused_threads();
        assert_eq!(tm::timer_settime(t, 0, Some(&its(1_000_000, 1_000_000)), None), 0);
        // Each refused notification read at least one expiration.
        assert!(until(|| liella_boundary::selftest::refused_threads() >= before + 5), "the expirations were due and their threads refused");
        assert_eq!(LOST_CALLS.load(SeqCst), 0, "no thread, no call");
        liella_boundary::selftest::allow_threads();
        assert!(until(|| LOST_CALLS.load(SeqCst) >= 1), "notified once a thread can be made");
        assert_eq!(tm::timer_settime(t, 0, Some(&its(0, 0)), None), 0);
        let overrun = LOST_OVERRUN.load(SeqCst);
        assert!(overrun >= 5, "the first notification counts the expirations that had no thread: overrun {overrun}");
        assert_eq!(tm::timer_delete(t), 0);
    });
}

/// The timer of [`timer_notification_retried`] in use, its calls, and the last call's overrun and
/// time (ns since [`RETRY_EPOCH`]).
static RETRY_TIMER: AtomicUsize = AtomicUsize::new(0);
static RETRY_CALLS: AtomicU32 = AtomicU32::new(0);
static RETRY_OVERRUN: AtomicI32 = AtomicI32::new(-1);
static RETRY_AT: AtomicU64 = AtomicU64::new(0);
/// The first call's overrun and time: a periodic timer notifies again as soon as the first
/// call returns, so under load the test may read the last call's values after the second one.
static RETRY_FIRST_OVERRUN: AtomicI32 = AtomicI32::new(-1);
static RETRY_FIRST_AT: AtomicU64 = AtomicU64::new(0);
static RETRY_EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

fn since_epoch() -> u64 {
    u64::try_from(RETRY_EPOCH.get_or_init(Instant::now).elapsed().as_nanos()).unwrap()
}

extern "C" fn retried(_: usize) {
    let t = core::ptr::without_provenance_mut::<c_void>(RETRY_TIMER.load(SeqCst));
    let (overrun, at) = (tm::timer_getoverrun(t), since_epoch());
    if RETRY_CALLS.load(SeqCst) == 0 {
        RETRY_FIRST_OVERRUN.store(overrun, SeqCst);
        RETRY_FIRST_AT.store(at, SeqCst);
    }
    RETRY_OVERRUN.store(overrun, SeqCst);
    RETRY_AT.store(at, SeqCst);
    RETRY_CALLS.fetch_add(1, SeqCst);
}

/// A new `SIGEV_THREAD` timer calling [`retried`], with the call record cleared.
fn retry_timer() -> *mut c_void {
    retry_timer_with(0)
}

/// [`retry_timer`] with the attribute object at `attr` (0: none).
fn retry_timer_with(attr: u64) -> *mut c_void {
    let t = timer(&SigEvent::thread(0, retried as extern "C" fn(usize) as usize as u64, attr));
    RETRY_TIMER.store(t.addr(), SeqCst);
    RETRY_CALLS.store(0, SeqCst);
    RETRY_OVERRUN.store(-1, SeqCst);
    RETRY_FIRST_OVERRUN.store(-1, SeqCst);
    t
}

/// The timers' last events (`selftest::timer_trace`), for an assertion's message.
fn timer_events() -> String {
    let trace: Vec<String> = liella_boundary::selftest::timer_trace().iter().filter(|&&e| e != 0).map(|e| format!("{}:{:#x}", e >> 32, e & 0xffff_ffff)).collect();
    trace.join(" ")
}

fn refused() -> usize {
    liella_boundary::selftest::refused_threads()
}

fn episodes() -> (usize, usize) {
    liella_boundary::selftest::timer_thread_episodes()
}

#[test]
fn timer_notification_retried() {
    // Clauses: s23_4_c_3_3, s23_4_c_3 (D-23.3, owner ruling #269).
    // A notification thread that cannot be created is retried with a bounded back-off until it
    // starts or the timer is deleted; expirations meanwhile join its overrun; `timer_settime`
    // drops the retry (and the expirations of a refused attempt it overlaps) and `timer_delete`
    // cancels it; each failure episode is noted once. The
    // failure is injected (`forbid_threads`, as a thread limit would), in a process of its own.
    let Some(err) = alone("timer_notification_retried", || {
        let ms = Duration::from_millis;
        since_epoch();
        // 1. A one-shot whose thread is refused is notified once a thread can be made, the retries
        //    being few (back-off) and at most 1 s apart (ceiling).
        let t = retry_timer();
        liella_boundary::selftest::forbid_threads();
        let (e0, r0) = (episodes(), refused());
        assert_eq!(tm::timer_settime(t, 0, Some(&its(1_000_000, 0)), None), 0);
        assert!(until(|| refused() > r0), "the expiry's thread was refused");
        std::thread::sleep(ms(2600));
        let tries = refused() - r0;
        // Retries due 10, 30, 70, 150, 310, 630, 1270, 2270 ms after the refusal: 9 attempts.
        assert!((6..=11).contains(&tries), "attempts while refused for 2.6 s: {tries} (doubling from 10 ms, at most 1 s apart)");
        assert_eq!(RETRY_CALLS.load(SeqCst), 0, "no thread, no call");
        assert_eq!(episodes(), (e0.0 + 1, e0.1), "one episode begun, noted once for {tries} refusals");
        let allowed = Instant::now();
        liella_boundary::selftest::allow_threads();
        assert!(until(|| RETRY_CALLS.load(SeqCst) == 1), "the one-shot is notified once a thread can be made");
        let late = allowed.elapsed();
        assert!(late < ms(1500), "notified {late:?} after threads were allowed: the retries are at most 1 s apart");
        assert_eq!(RETRY_OVERRUN.load(SeqCst), 0, "one expiration, no overrun");
        assert_eq!(episodes(), (e0.0 + 1, e0.1 + 1), "the episode's recovery noted once");
        std::thread::sleep(ms(100));
        assert_eq!(RETRY_CALLS.load(SeqCst), 1, "a one-shot is notified once");
        assert_eq!(tm::timer_delete(t), 0);

        // 2. A periodic timer: the notification that finally starts counts every expiration since
        //    the arming (overrun = expirations - 1), up to the moment it started.
        let t = retry_timer();
        liella_boundary::selftest::forbid_threads();
        let armed = since_epoch();
        assert_eq!(tm::timer_settime(t, 0, Some(&its(10_000_000, 10_000_000)), None), 0);
        std::thread::sleep(ms(200));
        let allowed = since_epoch();
        liella_boundary::selftest::allow_threads();
        assert!(until(|| RETRY_CALLS.load(SeqCst) >= 1), "notified once a thread can be made");
        // The count is read at the launch, between `allowed` and the call; 1 of slack each way
        // for the clocks. It is the FIRST call's: the timer notifies again once that call
        // returns, and under load the test may run only after the second.
        let (overrun, called) = (RETRY_FIRST_OVERRUN.load(SeqCst), RETRY_FIRST_AT.load(SeqCst));
        assert_eq!(tm::timer_settime(t, 0, Some(&its(0, 0)), None), 0);
        let (lo, hi) = ((allowed - armed) / 10_000_000, (called - armed) / 10_000_000);
        let total = u64::try_from(overrun).unwrap() + 1;
        assert!(lo.saturating_sub(1) <= total && total <= hi + 1, "first notification stands for {total} expirations; {lo}..={hi} were due");
        assert_eq!(episodes(), (e0.0 + 2, e0.1 + 2), "a second episode, begun and recovered once each");
        assert_eq!(tm::timer_delete(t), 0);

        // 3. `timer_delete` cancels a pending retry: no more attempts, no call.
        let t = retry_timer();
        liella_boundary::selftest::forbid_threads();
        let r1 = refused();
        assert_eq!(tm::timer_settime(t, 0, Some(&its(1_000_000, 0)), None), 0);
        assert!(until(|| refused() > r1), "the expiry's thread was refused");
        assert_eq!(tm::timer_delete(t), 0);
        let r2 = refused();
        std::thread::sleep(ms(400));
        assert_eq!(refused(), r2, "no retry after timer_delete");
        liella_boundary::selftest::allow_threads();
        std::thread::sleep(ms(100));
        assert_eq!(RETRY_CALLS.load(SeqCst), 0, "a deleted timer is not notified");

        // 4. `timer_settime` drops a pending retry: a disarm stops the attempts, nothing is
        //    notified, and a later arming is notified alone (overrun 0) at its expiry, not at the
        //    dropped retry's time (the back-off has grown to 1 s by then).
        let t = retry_timer();
        liella_boundary::selftest::forbid_threads();
        let r3 = refused();
        assert_eq!(tm::timer_settime(t, 0, Some(&its(1_000_000, 0)), None), 0);
        assert!(until(|| refused() > r3), "the expiry's thread was refused");
        // Retries at 10, 30, 70, 150, 310, 630, 1270 ms, then 2270 ms: the disarm comes midway,
        // clear of an attempt in progress (one it overlaps is step 5), and the wait runs past
        // the retry it dropped. (It came 30 ms after the 1270 ms retry once: a retry late under
        // load was still being refused after the disarm returned, gate of 2026-10-22.)
        std::thread::sleep(ms(1770));
        assert_eq!(tm::timer_settime(t, 0, Some(&its(0, 0)), None), 0);
        let r4 = refused();
        std::thread::sleep(ms(800));
        assert_eq!(refused(), r4, "no retry after the timer was disarmed");
        liella_boundary::selftest::allow_threads();
        std::thread::sleep(ms(100));
        assert_eq!(RETRY_CALLS.load(SeqCst), 0, "the dropped expiry is not notified");
        let armed = Instant::now();
        assert_eq!(tm::timer_settime(t, 0, Some(&its(1_000_000, 0)), None), 0);
        assert!(until(|| RETRY_CALLS.load(SeqCst) == 1), "a new arming is notified");
        let late = armed.elapsed();
        assert!(late < ms(250), "a new arming after a dropped retry notified after {late:?}: at its expiry, not the old retry's time");
        assert_eq!(RETRY_OVERRUN.load(SeqCst), 0, "the dropped expiry is not this setting's overrun");
        assert_eq!(tm::timer_delete(t), 0);

        // 5. A `timer_settime` during a refused attempt (the helper inside it, holding the
        //    expiry it read and the lock, so the setting finds no retry to drop and arms
        //    nothing): the old expiry is dropped and the new setting is notified at its expiry,
        //    not at the next retry, 1 s away by then.
        let t = retry_timer();
        liella_boundary::selftest::forbid_threads();
        let r5 = refused();
        assert_eq!(tm::timer_settime(t, 0, Some(&its(1_000_000, 0)), None), 0);
        // The refusal and its retries at 10, 30, 70, 150, 310, 630 ms; the one at 1270 ms is held.
        assert!(until(|| refused() >= r5 + 7), "the expiry's thread was refused and retried");
        liella_boundary::selftest::hold_refused_threads(true);
        assert!(until(|| refused() >= r5 + 8), "the retry at 1270 ms was refused");
        let set_at = Instant::now();
        assert_eq!(tm::timer_settime(t, 0, Some(&its(20_000_000, 0)), None), 0);
        liella_boundary::selftest::allow_threads();
        liella_boundary::selftest::hold_refused_threads(false);
        assert!(until(|| RETRY_CALLS.load(SeqCst) == 1), "the new setting is notified");
        let late = set_at.elapsed();
        assert!(late < ms(500), "a setting made during a refused attempt (due 20 ms after it) notified after {late:?}: at its expiry, not at the next retry 1 s away");
        assert_eq!(RETRY_OVERRUN.load(SeqCst), 0, "the old setting's expiry is not the new one's overrun");
        std::thread::sleep(ms(100));
        assert_eq!(RETRY_CALLS.load(SeqCst), 1, "the new setting is notified once");
        assert_eq!(tm::timer_delete(t), 0);
    }) else {
        return;
    };
    // The diagnostic channel is off: no `diag_set`, and `LIELLA_DIAG` is not in the test's
    // environment, so the lazy read at the first episode finds nothing. Five episodes, no line
    // (D-2.9: default silent).
    assert_eq!(timer_diag_lines(&err), Vec::<&str>::new(), "the diagnostic channel is off by default");
}

#[test]
fn timer_retry_back_off_ceiling_and_reset() {
    // Clauses: s23_4_c_3_3, s23_4_c_3 (D-23.3, owner rulings #269, #680).
    // The back-off the timers share with the message-queue and AIO retries has a ceiling and
    // starts again from its first delay in each failure episode: a refusal held well past the
    // ceiling is retried at most 1 s apart, so the notification starts within the ceiling once
    // threads are allowed; a second episode on the same timer (a periodic one, not set again
    // in between) begins anew, its first retry 10 ms after the refusal, not 1 s. The failure
    // is injected (`forbid_threads`), in a process of its own.
    alone("timer_retry_back_off_ceiling_and_reset", || {
        let ms = Duration::from_millis;
        since_epoch();
        let t = retry_timer();
        liella_boundary::selftest::forbid_threads();
        let (e0, r0) = (episodes(), refused());
        // Period 5 s: the second expiry comes after the first notification (about 3.3 s in).
        assert_eq!(tm::timer_settime(t, 0, Some(&its(1_000_000, 5_000_000_000)), None), 0);
        assert!(until(|| refused() > r0), "the first expiry's thread was refused");
        // Retries 10, 30, 70, 150, 310, 630, 1270, 2270, 3270 ms after the refusal: the delays
        // reach the 1 s ceiling after 1270 ms; without it the retry after 2550 ms would come at
        // 5110 ms, 2.5 s after threads are allowed.
        std::thread::sleep(ms(2600));
        let allowed = Instant::now();
        liella_boundary::selftest::allow_threads();
        assert!(until(|| RETRY_CALLS.load(SeqCst) == 1), "notified once a thread can be made");
        let late = allowed.elapsed();
        assert!(late < ms(1500), "notified {late:?} after threads were allowed, the refusal held past the ceiling: retries at most 1 s apart (back-off ceiling)");
        assert_eq!(episodes(), (e0.0 + 1, e0.1 + 1), "the first episode begun and recovered once each");

        // A second episode on the same timer: its next expiry is refused.
        liella_boundary::selftest::forbid_threads();
        let r1 = refused();
        assert!(until(|| refused() > r1), "the second expiry's thread was refused");
        let refused_at = Instant::now();
        assert!(until(|| refused() > r1 + 1), "the second episode is retried");
        let first = refused_at.elapsed();
        assert_eq!(episodes(), (e0.0 + 2, e0.1 + 1), "a second episode begun: the first one's recovery reset the back-off");
        assert!(first < ms(500), "the second episode's first retry came {first:?} after its refusal: it starts again from 10 ms, not from the first episode's 1 s");
        liella_boundary::selftest::allow_threads();
        assert!(until(|| RETRY_CALLS.load(SeqCst) == 2), "the second expiry is notified");
        assert_eq!(episodes(), (e0.0 + 2, e0.1 + 2), "the second episode recovered once");
        assert_eq!(tm::timer_delete(t), 0);
    });
}

/// While not 0, [`blocker`] stays in its function (on the caller's stack it was given).
static BLOCK: AtomicU32 = AtomicU32::new(0);
/// Calls of [`blocker`] begun.
static BLOCKED: AtomicU32 = AtomicU32::new(0);

/// A notification function that holds its thread (and so its stack) until [`BLOCK`] is cleared.
/// It uses only atomics (it runs on a thread the library made).
extern "C" fn blocker(_: usize) {
    BLOCKED.fetch_add(1, SeqCst);
    while BLOCK.load(SeqCst) != 0 {
        core::hint::spin_loop();
    }
}

/// An attribute object with a caller-supplied stack of its own (leaked, as in [`own_stack`]).
fn bare_stack() -> Box<pthread_attr_t> {
    let mem: &'static mut [u8] = Vec::leak(vec![0u8; OWN_STACK + 4096]);
    let lo = mem.as_mut_ptr().addr().next_multiple_of(4096);
    let mut a = Box::new(pthread_attr_t { __size: [0; 56] });
    assert_eq!(th::pthread_attr_init(&mut *a), 0);
    assert_eq!(th::pthread_attr_setstack(&mut *a, mem.as_mut_ptr().with_addr(lo).cast(), OWN_STACK), 0);
    a
}

/// The count of the timers' helper's wake-up `eventfd` (`/proc/self/fdinfo`): 0 once the helper
/// has drained it.
fn wake_count() -> u64 {
    let fd = liella_boundary::selftest::timer_wake_fd();
    assert!(fd >= 0, "a timer on a caller-supplied stack made the helper's wake-up eventfd");
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{fd}")).unwrap();
    let count = info.lines().find_map(|l| l.strip_prefix("eventfd-count:")).expect("an eventfd's fdinfo has its count");
    u64::from_str_radix(count.trim(), 16).unwrap()
}

/// An occupant timer (its function [`blocker`]) made with `a` and fired: its thread holds the
/// stack until [`BLOCK`] is cleared.
fn occupy_stack(a: &mut pthread_attr_t) -> *mut c_void {
    let occupant = timer(&SigEvent::thread(0, blocker as extern "C" fn(usize) as usize as u64, attr_word(a)));
    let b0 = BLOCKED.load(SeqCst);
    BLOCK.store(1, SeqCst);
    assert_eq!(tm::timer_settime(occupant, 0, Some(&its(1_000_000, 0)), None), 0);
    assert!(until(|| BLOCKED.load(SeqCst) > b0), "the occupant runs on the stack");
    occupant
}

#[test]
fn timer_abandoned_notification_retried() {
    // Clauses: s23_4_c_3_3, s23_4_c_3 (D-23.3, owner ruling #269).
    // A notification that waits for its caller-supplied stack (another notification's thread is
    // on it) and then cannot be started when the stack is free (no thread can be made) is
    // retried as one refused at once: a one-shot timer is still notified once threads can be
    // made again (re-arming its descriptor alone would lose it: nothing more expires), a
    // periodic one (the control) with the expirations meanwhile in its overrun. The failure is
    // injected (`forbid_threads`), in a process of its own.
    alone("timer_abandoned_notification_retried", || {
        let ms = Duration::from_millis;
        since_epoch();
        for (interval, what) in [(0, "one-shot"), (50_000_000, "periodic")] {
            let mut a = bare_stack();
            let occupant = timer(&SigEvent::thread(0, blocker as extern "C" fn(usize) as usize as u64, attr_word(&mut a)));
            let t = retry_timer_with(attr_word(&mut a));
            assert_eq!(th::pthread_attr_destroy(&mut *a), 0);
            drop(a);
            // 1. The occupant's thread holds the stack; the timer's notification waits for it.
            let b0 = BLOCKED.load(SeqCst);
            BLOCK.store(1, SeqCst);
            assert_eq!(tm::timer_settime(occupant, 0, Some(&its(1_000_000, 0)), None), 0);
            assert!(until(|| BLOCKED.load(SeqCst) > b0), "{what}: the occupant runs on the stack");
            assert_eq!(tm::timer_settime(t, 0, Some(&its(1_000_000, interval)), None), 0);
            assert!(until(|| liella_boundary::selftest::notifications_queued_for_stacks() == 1), "{what}: the expiry's notification waits for the stack");
            // 2. The stack is freed while no thread can be made: the notification is abandoned.
            let (e0, a0) = (episodes(), liella_boundary::selftest::timer_notifications_abandoned());
            liella_boundary::selftest::forbid_threads();
            BLOCK.store(0, SeqCst);
            assert!(until(|| liella_boundary::selftest::timer_notifications_abandoned() > a0), "{what}: the waiting notification could not be started");
            let r0 = refused();
            std::thread::sleep(ms(400));
            assert_eq!(RETRY_CALLS.load(SeqCst), 0, "{what}: no thread, no call");
            assert!(refused() > r0, "{what}: the abandoned notification is retried while threads are refused");
            assert_eq!(episodes(), (e0.0 + 1, e0.1), "{what}: one failure episode begun");
            assert_eq!(wake_count(), 0, "{what}: the helper drained the eventfd that woke it for the retry (a level-triggered event left set would make it spin)");
            // 3. Threads can be made again: the next retry (at most 1 s away) notifies.
            let allowed = Instant::now();
            liella_boundary::selftest::allow_threads();
            assert!(until(|| RETRY_CALLS.load(SeqCst) >= 1), "{what}: notified once a thread can be made");
            let late = allowed.elapsed();
            assert!(late < ms(1500), "{what}: notified {late:?} after threads were allowed: the retries are at most 1 s apart");
            assert_eq!(episodes(), (e0.0 + 1, e0.1 + 1), "{what}: the episode's recovery noted once");
            let overrun = RETRY_FIRST_OVERRUN.load(SeqCst);
            if interval == 0 {
                assert_eq!(overrun, 0, "one-shot: one expiration, no overrun");
                std::thread::sleep(ms(100));
                assert_eq!(RETRY_CALLS.load(SeqCst), 1, "one-shot: notified once");
            } else {
                assert!(overrun >= 4, "periodic: the expirations while refused (400 ms of 50 ms periods) join the overrun: {overrun}");
            }
            for t in [t, occupant] {
                assert_eq!(tm::timer_settime(t, 0, Some(&its(0, 0)), None), 0);
                assert_eq!(tm::timer_delete(t), 0);
            }
        }
    });
}

/// Calls of [`deleted`], the function of timers deleted while their notification waits.
static DELETED_CALLS: AtomicU32 = AtomicU32::new(0);

extern "C" fn deleted(_: usize) {
    DELETED_CALLS.fetch_add(1, SeqCst);
}

#[test]
fn timer_deleted_while_waiting_never_called() {
    // Clauses: s23_4_c_3_3, s23_4_c_3 (D-23.3, owner ruling #269).
    // Once `timer_delete` has returned, no call of the timer's function starts (one already
    // running may finish; none runs here): not that of a notification waiting for its
    // caller-supplied stack (its thread starts once the stack is free and finds the timer gone),
    // nor that of one abandoned and waiting for a retry (the delete cancels the retry), nor that
    // of one deleted while waiting and then abandoned (nothing is retried for a deleted timer).
    // The failure is injected (`forbid_threads`), in a process of its own.
    alone("timer_deleted_while_waiting_never_called", || {
        let ms = Duration::from_millis;
        let st = || liella_boundary::selftest::notifications_queued_for_stacks();
        let abandoned = || liella_boundary::selftest::timer_notifications_abandoned();
        for what in ["queued", "abandoned", "queued then abandoned"] {
            let mut a = bare_stack();
            let occupant = occupy_stack(&mut a);
            let t = timer(&SigEvent::thread(0, deleted as extern "C" fn(usize) as usize as u64, attr_word(&mut a)));
            assert_eq!(th::pthread_attr_destroy(&mut *a), 0);
            drop(a);
            let c0 = DELETED_CALLS.load(SeqCst);
            assert_eq!(tm::timer_settime(t, 0, Some(&its(1_000_000, 0)), None), 0);
            assert!(until(|| st() == 1), "{what}: the expiry's notification waits for the stack");
            match what {
                "queued" => {
                    assert_eq!(tm::timer_delete(t), 0);
                    BLOCK.store(0, SeqCst);
                    assert!(until(|| st() == 0), "{what}: the waiting notification's thread starts once the stack is free");
                }
                "abandoned" => {
                    let a0 = abandoned();
                    liella_boundary::selftest::forbid_threads();
                    BLOCK.store(0, SeqCst);
                    assert!(until(|| abandoned() > a0), "{what}: the waiting notification could not be started");
                    let r0 = refused();
                    assert!(until(|| refused() > r0), "{what}: it is retried while threads are refused");
                    assert_eq!(tm::timer_delete(t), 0);
                    liella_boundary::selftest::allow_threads();
                }
                _ => {
                    assert_eq!(tm::timer_delete(t), 0);
                    liella_boundary::selftest::forbid_threads();
                    BLOCK.store(0, SeqCst);
                    assert!(until(|| st() == 0), "{what}: the waiting notification is handed back");
                    std::thread::sleep(ms(100));
                    liella_boundary::selftest::allow_threads();
                }
            }
            // A retry is at most 1 s away (the back-off's ceiling).
            std::thread::sleep(ms(1500));
            assert_eq!(DELETED_CALLS.load(SeqCst), c0, "{what}: a timer deleted while its notification waited is not notified");
            assert_eq!(tm::timer_delete(occupant), 0);
        }
    });
}

#[test]
fn timer_set_while_the_helper_reads() {
    // Clauses: s23_4_c_3_3, s23_4_c_3 (D-23.3, owner ruling #269; #544).
    // A `timer_settime` that comes after the helper has taken the timer's generation and before
    // it reads the descriptor, with the new setting expiring at once, while notification threads
    // are refused: the helper's read is the previous setting's (the setting waits for it), so the
    // new setting's expiry is not dropped with it and is notified once threads can be made. The
    // helper is held in that window (`hold_timer_reads`), in a process of its own.
    alone("timer_set_while_the_helper_reads", || {
        let ms = Duration::from_millis;
        since_epoch();
        // 1. The helper held between taking the generation and its read.
        let t = retry_timer();
        liella_boundary::selftest::forbid_threads();
        let h0 = liella_boundary::selftest::held_timer_reads();
        liella_boundary::selftest::hold_timer_reads(true);
        assert_eq!(tm::timer_settime(t, 0, Some(&its(1_000_000, 0)), None), 0);
        assert!(until(|| liella_boundary::selftest::held_timer_reads() > h0), "the helper took the expiry and is held before its read");
        // The new setting expires 1 ns after it is made; the helper reads only after 100 ms.
        let addr = t.addr();
        let setter = std::thread::spawn(move || tm::timer_settime(core::ptr::without_provenance_mut(addr), 0, Some(&its(1, 0)), None));
        std::thread::sleep(ms(100));
        let r0 = refused();
        liella_boundary::selftest::hold_timer_reads(false);
        assert_eq!(setter.join().unwrap(), 0);
        // Threads stay refused until the read's notification has been refused: the count read is
        // then dropped (as the previous setting's) or kept, never notified at once.
        assert!(until(|| refused() > r0), "the notification of the count read was refused");
        liella_boundary::selftest::allow_threads();
        let dl = timeout::deadline(Duration::from_secs(5));
        while RETRY_CALLS.load(SeqCst) == 0 && Instant::now() < dl {
            std::thread::sleep(ms(1));
        }
        assert_eq!(RETRY_CALLS.load(SeqCst), 1, "a setting made while the helper was about to read its descriptor is notified (its expiry was not dropped as the previous setting's); timer events: {}", timer_events());
        assert_eq!(RETRY_OVERRUN.load(SeqCst), 0, "the previous setting's expiry is not the new one's overrun");
        std::thread::sleep(ms(100));
        assert_eq!(RETRY_CALLS.load(SeqCst), 1, "the new setting is notified once");
        assert_eq!(tm::timer_delete(t), 0);

        // 2. The other order: a setting in progress (held before its `timerfd_settime`) when a
        //    retry of the previous setting's refused expiry comes. The helper puts its read off
        //    (it cannot tell which setting a count would be), the old expiry is dropped, and the
        //    new setting is notified once threads can be made.
        let t = retry_timer();
        liella_boundary::selftest::forbid_threads();
        let r0 = refused();
        assert_eq!(tm::timer_settime(t, 0, Some(&its(1_000_000, 0)), None), 0);
        assert!(until(|| refused() > r0), "the expiry's thread was refused");
        let (s0, d0) = liella_boundary::selftest::held_timer_sets();
        let m0 = liella_boundary::selftest::timer_sets_met_reads();
        liella_boundary::selftest::hold_timer_sets(true);
        let addr = t.addr();
        let setter = std::thread::spawn(move || tm::timer_settime(core::ptr::without_provenance_mut(addr), 0, Some(&its(1, 0)), None));
        assert!(until(|| liella_boundary::selftest::held_timer_sets().0 > s0), "the setting is held in progress");
        // Either order of the retry's read and the setter's counting in is allowed. Usually the
        // setter counts in first and the retry puts its read off. Under load the retry (10 ms
        // after the refusal) can begin its read just before the setter counts in
        // (SIGEV-TIMER-RACE-3, ~1 in 3000 runs at load 37-64; trace `16:0xa 21 1:0x1 11 12:0x0
        // 15:0x2`: retry scheduled, retry due, setter counts in meeting READING, read of
        // count 0 under the old generation, refused and dropped as moved). The setter then
        // waits for that read. The read is of the previous setting (its generation is the
        // old one), and ruling #269 drops a refused notification of a previous setting, so no
        // put-off read is needed. What #269 does not allow is still checked below: the new
        // setting is notified exactly once, and the dropped expiry is not counted as its overrun.
        let met_or_deferred = || liella_boundary::selftest::held_timer_sets().1 > d0 || liella_boundary::selftest::timer_sets_met_reads() > m0;
        let dl = timeout::deadline(Duration::from_secs(5));
        while !met_or_deferred() && Instant::now() < dl {
            std::thread::sleep(ms(1));
        }
        assert!(met_or_deferred(), "the helper's retry put its read off for the setting in progress, or the setting met the retry's read; timer events: {}", timer_events());
        liella_boundary::selftest::hold_timer_sets(false);
        assert_eq!(setter.join().unwrap(), 0);
        liella_boundary::selftest::allow_threads();
        let dl = timeout::deadline(Duration::from_secs(5));
        while RETRY_CALLS.load(SeqCst) == 0 && Instant::now() < dl {
            std::thread::sleep(ms(1));
        }
        assert_eq!(RETRY_CALLS.load(SeqCst), 1, "a setting the helper met in progress is notified");
        assert_eq!(RETRY_OVERRUN.load(SeqCst), 0, "the dropped expiry is not the new setting's overrun");
        std::thread::sleep(ms(100));
        assert_eq!(RETRY_CALLS.load(SeqCst), 1, "the new setting is notified once");
        assert_eq!(tm::timer_delete(t), 0);

        // 3. A setting still in progress after its `timerfd_settime` (held there), its 1 ns
        //    expiry already in the descriptor, when the helper's event comes (SIGEV-TIMER-RACE: a
        //    setter preempted there on a loaded host). A count read then would be the new
        //    setting's, taken as the previous one's and dropped with the refused thread: the
        //    helper puts its read off instead, and the setting, once done, is notified once.
        let t = retry_timer();
        liella_boundary::selftest::forbid_threads();
        let (s0, d0) = liella_boundary::selftest::held_timer_sets();
        let f0 = liella_boundary::selftest::forced_timer_reads();
        liella_boundary::selftest::hold_timer_sets_late(true);
        let addr = t.addr();
        let setter = std::thread::spawn(move || tm::timer_settime(core::ptr::without_provenance_mut(addr), 0, Some(&its(1, 0)), None));
        assert!(until(|| liella_boundary::selftest::held_timer_sets().0 > s0), "the setting is held after setting the descriptor");
        let dl = timeout::deadline(Duration::from_secs(5));
        while liella_boundary::selftest::held_timer_sets().1 == d0 && Instant::now() < dl {
            std::thread::sleep(ms(1));
        }
        assert!(liella_boundary::selftest::held_timer_sets().1 > d0, "the helper met the setting in progress");
        // Long enough for the helper's first put-off reads (1, 2, 4, 8 ms) to come round.
        std::thread::sleep(ms(20));
        liella_boundary::selftest::hold_timer_sets_late(false);
        assert_eq!(setter.join().unwrap(), 0);
        liella_boundary::selftest::allow_threads();
        let dl = timeout::deadline(Duration::from_secs(5));
        while RETRY_CALLS.load(SeqCst) == 0 && Instant::now() < dl {
            std::thread::sleep(ms(1));
        }
        assert_eq!(RETRY_CALLS.load(SeqCst), 1, "a setting whose expiry the helper met in progress is notified");
        assert_eq!(liella_boundary::selftest::forced_timer_reads(), f0, "the helper did not read while the setting was in progress");
        assert_eq!(RETRY_OVERRUN.load(SeqCst), 0, "no expiry of the previous setting is the new one's overrun");
        std::thread::sleep(ms(100));
        assert_eq!(RETRY_CALLS.load(SeqCst), 1, "the new setting is notified once");
        assert_eq!(tm::timer_delete(t), 0);

        // 4. A setting done with the timer's state (descriptor set, generation moved) but not
        //    yet returned (held there), its 1 ns expiry read by the helper under the new
        //    generation and refused: the retry scheduled is the new setting's own. The setter
        //    must not take it as a previous setting's retry and drop it (SIGEV-TIMER-RACE-2: a
        //    setter preempted between moving the generation and taking the retry did, and the
        //    setting was never notified). Forced with the hold, not by timing.
        let t = retry_timer();
        liella_boundary::selftest::forbid_threads();
        let (s0, _) = liella_boundary::selftest::held_timer_sets();
        let r0 = refused();
        liella_boundary::selftest::hold_timer_sets_done(true);
        let addr = t.addr();
        let setter = std::thread::spawn(move || tm::timer_settime(core::ptr::without_provenance_mut(addr), 0, Some(&its(1, 0)), None));
        assert!(until(|| liella_boundary::selftest::held_timer_sets().0 > s0), "the setting is held once done with the timer's state");
        assert!(until(|| refused() > r0), "the new setting's expiry was read and its thread refused while the setter was held; timer events: {}", timer_events());
        // The helper schedules the retry after the refusal, under the timers' lock.
        liella_boundary::selftest::timer_helper_settled();
        liella_boundary::selftest::hold_timer_sets_done(false);
        assert_eq!(setter.join().unwrap(), 0);
        liella_boundary::selftest::allow_threads();
        let dl = timeout::deadline(Duration::from_secs(5));
        while RETRY_CALLS.load(SeqCst) == 0 && Instant::now() < dl {
            std::thread::sleep(ms(1));
        }
        assert_eq!(RETRY_CALLS.load(SeqCst), 1, "a setting whose own expiry was refused before the setter returned is notified (its retry was not dropped as a previous setting's); timer events: {}", timer_events());
        assert_eq!(RETRY_OVERRUN.load(SeqCst), 0, "the refused expiry is notified without overrun");
        std::thread::sleep(ms(100));
        assert_eq!(RETRY_CALLS.load(SeqCst), 1, "the new setting is notified once");
        assert_eq!(tm::timer_delete(t), 0);
    });
}

#[test]
fn timer_episodes_reported_on_the_diagnostic_channel() {
    // Clauses: s23_4_c_3_3 (D-23.3, D-2.9, owner ruling #422).
    // `LIELLA_DIAG=timers`: a notification-thread failure episode writes one line to descriptor
    // 2 when it begins and one when a thread starts again, however many retries come between.
    // The value names topics separated by commas (`all`: every one); other names are ignored.
    let topics = liella_boundary::selftest::diag_topics;
    for (value, on) in [
        (&b"timers"[..], 1),
        (b"all", 1),
        (b"", 0),
        (b"nosuch,timers", 1),
        (b"timers,", 1),
        (b",,", 0),
        (b"Timers", 0),
        (b"timer", 0),
        (b"timersx", 0),
        (b" timers", 0),
    ] {
        assert_eq!(topics(value), on, "LIELLA_DIAG={:?}", String::from_utf8_lossy(value));
    }
    let Some(err) = alone("timer_episodes_reported_on_the_diagnostic_channel", || {
        liella_boundary::selftest::diag_set(b"timers");
        let t = retry_timer();
        liella_boundary::selftest::forbid_threads();
        let r0 = refused();
        assert_eq!(tm::timer_settime(t, 0, Some(&its(1_000_000, 0)), None), 0);
        // The first refusal and at least three retries (10, 30, 70 ms after it).
        assert!(until(|| refused() > r0 + 3), "the expiry's thread was refused and retried");
        liella_boundary::selftest::allow_threads();
        assert!(until(|| RETRY_CALLS.load(SeqCst) == 1), "the one-shot is notified once a thread can be made");
        assert_eq!(tm::timer_delete(t), 0);
    }) else {
        return;
    };
    let lines = timer_diag_lines(&err);
    assert_eq!(lines.len(), 2, "one line at the episode's start, one at its recovery: {lines:?}");
    assert!(lines[0].ends_with("liella: timers: a SIGEV_THREAD timer's notification thread cannot be created (thread or memory starvation?); retrying with back-off"), "{lines:?}");
    assert!(lines[1].ends_with("liella: timers: a SIGEV_THREAD timer's notification thread started again after a failure episode"), "{lines:?}");
}

// ------------------------------------------------------------------------------ message queues

const IPCNS: &str = "LIELLA_SIGEV_IPCNS";
const O_RDWR: c_int = 2;
const O_CREAT: c_int = 0o100;
const O_NONBLOCK: c_int = 0o4000;

/// Run `body` in this test re-executed in a private user and IPC namespace (its own message
/// queues); without `unshare` the test says so and passes.
fn in_ipc_ns(test: &str, body: impl FnOnce()) {
    if std::env::var(IPCNS).as_deref() == Ok(test) {
        body();
        return;
    }
    let ns = ["--user", "--ipc", "--map-current-user"];
    if !Command::new("unshare").args(ns).arg("true").status().is_ok_and(|s| s.success()) {
        skip_note::skipped("skipped: no private IPC namespace (unshare --user --ipc --map-current-user)");
        return;
    }
    let o = Command::new("unshare").args(ns).arg(std::env::current_exe().unwrap()).args([test, "--exact", "--test-threads=1", "--nocapture"]).env(IPCNS, test).output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success() && out.contains("1 passed"), "{test} in its namespace: {:?}\n{out}{}", o.status, String::from_utf8_lossy(&o.stderr));
}

fn queue(name: &core::ffi::CStr) -> c_int {
    let attr = MqAttr::new(4, 8);
    let q = mq::liella_mq_open4(name.as_ptr(), O_RDWR | O_CREAT | O_NONBLOCK, 0o600, &attr);
    assert!(q >= 0, "mq_open");
    q
}

#[test]
fn mq_notification_attributes() {
    // Clauses: s25_3_c_3 (D-25.5).
    in_ipc_ns("mq_notification_attributes", || {
        for (slot, name) in [(MQ_SIZED, c"/sized"), (MQ_OWN, c"/own")] {
            let q = queue(name);
            let (mut a, lo) = if slot == MQ_OWN { let (a, lo) = own_stack(slot); (a, Some(lo)) } else { (sized_joinable(BIG_STACK), None) };
            assert_eq!(mq::mq_notify(q, Some(&SigEvent::thread(slot as u64, observe_addr(), attr_word(&mut a)))), 0);
            assert_eq!(th::pthread_attr_destroy(&mut *a), 0);
            drop(a);
            assert_eq!(mq::mq_send(q, c"m".as_ptr(), 1, 0), 0);
            assert!(until(|| SEEN[slot].calls.load(SeqCst) == 1), "notified");
            check(slot, if lo.is_some() { "mq_notify, caller's stack" } else { "mq_notify, stack size" }, lo);
            assert_eq!(io::close(q), 0);
            assert_eq!(mq::mq_unlink(name.as_ptr()), 0);
        }
    });
}

/// Notifications of the self-registering queue.
const AGAIN_ROUNDS: u32 = 20;
static AGAIN_QUEUE: AtomicI32 = AtomicI32::new(-1);
/// The attribute object (with the caller's stack) the function registers with, kept alive.
static AGAIN_ATTR: AtomicUsize = AtomicUsize::new(0);
/// Registrations made, and messages sent, so far.
static AGAIN_REGISTERED: AtomicU32 = AtomicU32::new(0);
static AGAIN_SENT: AtomicU32 = AtomicU32::new(0);

/// The wait limit of the notification functions, in ms. They run on small caller-supplied stacks, where the load
/// scale (a /proc read, allocations) must not be computed: the test thread primes it (`prime_notify_limit`) first
/// (FL-13: computing it inside the function crashed the stack-sharing tests with SIGSEGV).
static NOTIFY_LIMIT_MS: AtomicU64 = AtomicU64::new(20_000);

fn prime_notify_limit() {
    NOTIFY_LIMIT_MS.store(u64::try_from(timeout::limit(Duration::from_secs(60)).as_millis()).unwrap_or(u64::MAX), SeqCst);
}

fn notify_deadline() -> Instant {
    Instant::now() + Duration::from_millis(NOTIFY_LIMIT_MS.load(SeqCst))
}

/// The self-registering notification function: it takes the message, registers again with the
/// same attributes (so the same stack), and stays until the next message has been sent, so the
/// next notification is due while it still runs on the stack.
extern "C" fn again(slot: usize) {
    let s = &SEEN[slot];
    let canary = AtomicU64::new(0);
    let token = enter(s, &canary);
    let round = s.calls.load(SeqCst);
    let q = AGAIN_QUEUE.load(SeqCst);
    let mut buf = [0 as core::ffi::c_char; 8];
    mq::mq_receive(q, buf.as_mut_ptr(), buf.len(), None);
    if round + 1 < AGAIN_ROUNDS {
        let ev = SigEvent::thread(slot as u64, again as extern "C" fn(usize) as usize as u64, AGAIN_ATTR.load(SeqCst) as u64);
        if mq::mq_notify(q, Some(&ev)) == 0 {
            AGAIN_REGISTERED.fetch_add(1, SeqCst);
            let dl = notify_deadline();
            while AGAIN_SENT.load(SeqCst) < round + 2 && Instant::now() < dl {
                std::thread::yield_now();
            }
        }
    }
    leave(s, &canary, token);
}

#[test]
fn mq_reregistering_shares_stack() {
    // Clauses: s25_3_c_3 (D-25.5).
    in_ipc_ns("mq_reregistering_shares_stack", || {
        prime_notify_limit();
        let q = queue(c"/again");
        AGAIN_QUEUE.store(q, SeqCst);
        let (a, _) = own_stack(MQ_AGAIN);
        let a: &'static mut pthread_attr_t = Box::leak(a);
        AGAIN_ATTR.store(attr_word(a) as usize, SeqCst);
        let ev = SigEvent::thread(MQ_AGAIN as u64, again as extern "C" fn(usize) as usize as u64, attr_word(a));
        assert_eq!(mq::mq_notify(q, Some(&ev)), 0);
        AGAIN_REGISTERED.store(1, SeqCst);
        for round in 0..AGAIN_ROUNDS {
            // The previous function has registered again (and is still running).
            assert!(until(|| AGAIN_REGISTERED.load(SeqCst) > round), "round {round}: registered again");
            assert_eq!(mq::mq_send(q, c"m".as_ptr(), 1, 0), 0);
            AGAIN_SENT.store(round + 1, SeqCst);
        }
        let s = &SEEN[MQ_AGAIN];
        assert!(until(|| s.calls.load(SeqCst) == AGAIN_ROUNDS), "every message notified ({} calls)", s.calls.load(SeqCst));
        assert_eq!(s.outside.load(SeqCst), 0, "every call ran on the caller's stack");
        assert_eq!(s.max_running.load(SeqCst), 1, "one notification at a time on the stack");
        assert_eq!(s.clobbered.load(SeqCst), 0, "no call's stack-canary word overwritten by another on the stack");
        assert_eq!(io::close(q), 0);
        assert_eq!(mq::mq_unlink(c"/again".as_ptr()), 0);
    });
}

// ------------------------------------------------------------------------------ asynchronous I/O

/// A `struct aiocb` (168 bytes, Appendix C) with the fields the test sets. The library writes
/// its status words concurrently, so a submitted block is reached only through a raw pointer.
#[repr(C, align(8))]
#[derive(Clone, Copy, Default)]
struct Cb {
    fildes: c_int,
    opcode: c_int,
    reqprio: c_int,
    _p0: c_int,
    buf: usize,
    nbytes: usize,
    value: u64,
    signo: c_int,
    notify: c_int,
    function: usize,
    attrs: usize,
    _sev: [u64; 4],
    _next: usize,
    _abs: [c_int; 2],
    error: c_int,
    _p1: c_int,
    ret: isize,
    offset: i64,
    _res: [u64; 4],
}
const _: () = assert!(size_of::<Cb>() == 168);

#[test]
fn aio_notification_attributes() {
    // Clauses: s11_11_c_0 (D-11.11).
    let file = Scratch::path("sigev-aio");
    std::fs::write(&*file, b"0123456789").unwrap();
    let fd = io::liella_open3(file.c().as_ptr(), 0, 0);
    assert!(fd >= 0);
    for slot in [AIO_SIZED, AIO_OWN] {
        let (mut a, lo) = if slot == AIO_OWN { let (a, lo) = own_stack(slot); (a, Some(lo)) } else { (sized_joinable(BIG_STACK), None) };
        let mut buf = [0u8; 4];
        let mut cb = Cb { fildes: fd, buf: buf.as_mut_ptr().addr(), nbytes: 4, value: slot as u64, notify: SIGEV_THREAD, function: observe as extern "C" fn(usize) as usize, attrs: attr_word(&mut a) as usize, ..Cb::default() };
        let p = &raw mut cb;
        assert_eq!(io::aio_read(p.cast()), 0);
        assert_eq!(th::pthread_attr_destroy(&mut *a), 0);
        drop(a);
        assert!(until(|| SEEN[slot].calls.load(SeqCst) == 1), "notified");
        assert_eq!(io::aio_return(p.cast()), 4);
        check(slot, if lo.is_some() { "aio, caller's stack" } else { "aio, stack size" }, lo);
    }
    assert_eq!(io::close(fd), 0);
}

/// Requests completing together, each notifying on the one caller-supplied stack.
const TOGETHER: usize = 8;
/// Their control blocks (addresses), for the function to wait on.
static TOGETHER_CBS: [AtomicUsize; TOGETHER] = [const { AtomicUsize::new(0) }; TOGETHER];
const ENGINE: &str = "LIELLA_SIGEV_AIO_ENGINE";
const EINPROGRESS: c_int = 115;

/// The function of the requests completing together: it stays until every request has
/// completed, so the other notifications are due while it runs on the stack.
extern "C" fn together(slot: usize) {
    let s = &SEEN[slot];
    let canary = AtomicU64::new(0);
    let token = enter(s, &canary);
    let dl = notify_deadline();
    let busy = || TOGETHER_CBS.iter().any(|c| io::aio_error(core::ptr::with_exposed_provenance(c.load(SeqCst))) == EINPROGRESS);
    while busy() && Instant::now() < dl {
        std::thread::yield_now();
    }
    leave(s, &canary, token);
}

/// Run `test` once per AIO engine, each in a fresh process (the engine is chosen once per
/// process): `None` in this process (the runs have passed), the engine in such a run (the
/// worker pool already forced for "workers").
fn per_engine(test: &str) -> Option<String> {
    let Ok(engine) = std::env::var(ENGINE) else {
        for engine in ["ring", "workers"] {
            let o = Command::new(std::env::current_exe().unwrap())
                .args([test, "--exact", "--test-threads=1", "--nocapture"])
                .env(ENGINE, engine)
                .output()
                .unwrap();
            let out = String::from_utf8_lossy(&o.stdout);
            assert!(o.status.success() && out.contains("1 passed"), "{engine}: {:?}\n{out}{}", o.status, String::from_utf8_lossy(&o.stderr));
        }
        return None;
    };
    if engine == "workers" {
        liella_boundary::selftest::aio_force_workers(true);
    }
    Some(engine)
}

#[test]
fn aio_completions_share_stack() {
    // Clauses: s11_11_c_0 (D-11.11).
    prime_notify_limit();
    let Some(engine) = per_engine("aio_completions_share_stack") else { return };
    let file = Scratch::path("sigev-aio-together");
    std::fs::write(&*file, b"0123456789").unwrap();
    let (a, _) = own_stack(AIO_TOGETHER);
    let a: &'static mut pthread_attr_t = Box::leak(a);
    let mut bufs = [[0u8; 4]; TOGETHER];
    let mut cbs = [Cb::default(); TOGETHER];
    let mut fds = [0; TOGETHER];
    // One descriptor each, so the requests run (and complete) concurrently.
    for (i, cb) in cbs.iter_mut().enumerate() {
        fds[i] = io::liella_open3(file.c().as_ptr(), 0, 0);
        assert!(fds[i] >= 0);
        *cb = Cb { fildes: fds[i], buf: bufs[i].as_mut_ptr().addr(), nbytes: 4, value: AIO_TOGETHER as u64, notify: SIGEV_THREAD, function: together as extern "C" fn(usize) as usize, attrs: attr_word(a) as usize, ..Cb::default() };
        TOGETHER_CBS[i].store(core::ptr::from_mut(cb).expose_provenance(), SeqCst);
    }
    let p = cbs.as_mut_ptr();
    for i in 0..TOGETHER {
        assert_eq!(io::aio_read(p.wrapping_add(i).cast()), 0);
    }
    let s = &SEEN[AIO_TOGETHER];
    assert!(until(|| s.calls.load(SeqCst) == TOGETHER as u32), "every request notified ({} calls)", s.calls.load(SeqCst));
    for (i, &fd) in fds.iter().enumerate() {
        assert_eq!(io::aio_return(p.wrapping_add(i).cast()), 4);
        assert_eq!(io::close(fd), 0);
    }
    assert_eq!(s.outside.load(SeqCst), 0, "{engine}: every call ran on the caller's stack");
    assert_eq!(s.max_running.load(SeqCst), 1, "{engine}: one notification at a time on the stack");
    assert_eq!(s.clobbered.load(SeqCst), 0, "{engine}: no call's stack-canary word overwritten by another on the stack");
    if engine == "workers" {
        assert_eq!(liella_boundary::selftest::aio_engine(), "workers");
    }
}

const LIO_READ: c_int = 0;
const LIO_NOWAIT: c_int = 1;

#[test]
fn lio_completions_share_stack() {
    // Clauses: s11_11_c_0 (D-11.11).
    // A `lio_listio(LIO_NOWAIT)` list whose entries and whose list notification all name the one
    // caller-supplied stack: the entries' notifications and the list's never run on it at once.
    prime_notify_limit();
    let Some(engine) = per_engine("lio_completions_share_stack") else { return };
    let file = Scratch::path("sigev-lio-together");
    std::fs::write(&*file, b"0123456789").unwrap();
    let (a, _) = own_stack(LIO_TOGETHER);
    let a: &'static mut pthread_attr_t = Box::leak(a);
    let function = together as extern "C" fn(usize) as usize;
    let mut bufs = [[0u8; 4]; TOGETHER];
    let mut cbs = [Cb::default(); TOGETHER];
    let mut fds = [0; TOGETHER];
    for (i, cb) in cbs.iter_mut().enumerate() {
        fds[i] = io::liella_open3(file.c().as_ptr(), 0, 0);
        assert!(fds[i] >= 0);
        *cb = Cb { fildes: fds[i], opcode: LIO_READ, buf: bufs[i].as_mut_ptr().addr(), nbytes: 4, value: LIO_TOGETHER as u64, notify: SIGEV_THREAD, function, attrs: attr_word(a) as usize, ..Cb::default() };
        TOGETHER_CBS[i].store(core::ptr::from_mut(cb).expose_provenance(), SeqCst);
    }
    let p = cbs.as_mut_ptr();
    let list: [*mut c_void; TOGETHER] = core::array::from_fn(|i| p.wrapping_add(i).cast());
    let sig = SigEvent::thread(LIO_TOGETHER as u64, function as u64, attr_word(a));
    assert_eq!(io::lio_listio(LIO_NOWAIT, list.as_ptr(), TOGETHER as c_int, Some(&sig)), 0);
    let s = &SEEN[LIO_TOGETHER];
    // One call per entry and one for the list.
    let want = TOGETHER as u32 + 1;
    assert!(until(|| s.calls.load(SeqCst) == want), "{engine}: every entry and the list notified ({} calls)", s.calls.load(SeqCst));
    for (i, &fd) in fds.iter().enumerate() {
        assert_eq!(io::aio_return(p.wrapping_add(i).cast()), 4);
        assert_eq!(io::close(fd), 0);
    }
    assert_eq!(s.outside.load(SeqCst), 0, "{engine}: every call ran on the caller's stack");
    assert_eq!(s.max_running.load(SeqCst), 1, "{engine}: one notification at a time on the stack");
    assert_eq!(s.clobbered.load(SeqCst), 0, "{engine}: no call's stack-canary word overwritten by another on the stack");
}

/// Calls of [`retried_once`], the function of the notifications whose thread is refused first.
static RETRIED_CALLS: AtomicU32 = AtomicU32::new(0);

extern "C" fn retried_once(_: usize) {
    RETRIED_CALLS.fetch_add(1, SeqCst);
}

fn retried_once_addr() -> u64 {
    retried_once as extern "C" fn(usize) as usize as u64
}

/// The retried notifications have been called `n` times: a while later still `n` (never twice).
fn settle(n: u32, what: &str) {
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(RETRIED_CALLS.load(SeqCst), n, "{what}: notified once, never twice");
}

/// A child of `fork` while a retry is pending: with threads allowed again it runs `body` (true:
/// as expected) and never delivers the parent's notification (D-25.5, D-11.11: nothing pending
/// is inherited).
fn child_without_the_retry(body: impl FnOnce() -> bool) {
    let calls = RETRIED_CALLS.load(SeqCst);
    let pid = pr::fork();
    assert!(pid >= 0, "fork");
    if pid == 0 {
        liella_boundary::selftest::allow_threads();
        let ok = body();
        std::thread::sleep(Duration::from_millis(300));
        st::_exit(if ok && RETRIED_CALLS.load(SeqCst) == calls { 0 } else { 1 });
    }
    let mut status = -1;
    assert_eq!(pr::waitpid(pid, Some(&mut status), 0), pid);
    assert_eq!(status, 0, "the child has no pending retry, and its own registration works");
}

#[test]
fn mq_refused_notification_retried() {
    // Clauses: s25_3_c_3 (D-25.5, owner ruling #680).
    // A message-queue notification whose thread cannot be made is retried with the timers'
    // back-off, not dropped (the reference drops it): once a thread can be made it arrives, once.
    // Meanwhile the message has used the registration up, so the queue takes a new one (from
    // another process, or this one; one at a time), and a child of `fork` does not deliver it.
    // The failure is injected (`forbid_threads`), in a process of its own (the namespace's).
    in_ipc_ns("mq_refused_notification_retried", || {
        liella_boundary::selftest::mark_own_process();
        let q = queue(c"/retried");
        let ev = SigEvent::thread(0, retried_once_addr(), 0);
        assert_eq!(mq::mq_notify(q, Some(&ev)), 0);
        let (e0, r0) = (liella_boundary::selftest::mq_thread_episodes(), refused());
        liella_boundary::selftest::forbid_threads();
        assert_eq!(mq::mq_send(q, c"m".as_ptr(), 1, 0), 0);
        assert!(until(|| refused() >= r0 + 2), "the notification thread was refused, then retried");
        assert_eq!(RETRIED_CALLS.load(SeqCst), 0, "no thread, no call");
        // Another process's registration: the queue's registration is free.
        child_without_the_retry(|| mq::mq_notify(q, Some(&SigEvent::new(1, 0, 0, 0))) == 0 && mq::mq_notify(q, None) == 0);
        // This process registers again; a second registration is still refused.
        assert_eq!(mq::mq_notify(q, Some(&ev)), 0, "the message used the registration up");
        assert_ne!(mq::mq_notify(q, Some(&ev)), 0, "one registration at a time");
        liella_boundary::selftest::allow_threads();
        assert!(until(|| RETRIED_CALLS.load(SeqCst) == 1), "notified once a thread can be made");
        settle(1, "the retried notification");
        let e1 = liella_boundary::selftest::mq_thread_episodes();
        assert_eq!((e1.0 - e0.0, e1.1 - e0.1), (1, 1), "one failure episode, ended by the start");
        // The new registration notifies with the next message on the empty queue.
        let mut buf = [0 as core::ffi::c_char; 8];
        assert_eq!(mq::mq_receive(q, buf.as_mut_ptr(), buf.len(), None), 1);
        assert_eq!(mq::mq_send(q, c"n".as_ptr(), 1, 0), 0);
        assert!(until(|| RETRIED_CALLS.load(SeqCst) == 2), "the new registration notifies");
        settle(2, "the new registration");
        assert_eq!(io::close(q), 0);
        assert_eq!(mq::mq_unlink(c"/retried".as_ptr()), 0);
    });
}

#[test]
fn mq_abandoned_notification_retried() {
    // Clauses: s25_3_c_3 (D-25.5, owner ruling #680).
    // A message-queue notification that waits for its caller-supplied stack (another
    // notification's thread is on it) and then cannot be started when the stack is free (no
    // thread can be made) keeps its record and is retried, as one whose thread cannot be made at
    // once: it arrives once a thread can be made, once, and its record is then freed; the queue
    // takes a new registration, which notifies. The failure is injected (`forbid_threads`), in a
    // process of its own (the namespace's).
    in_ipc_ns("mq_abandoned_notification_retried", || {
        liella_boundary::selftest::mark_own_process();
        let mut a = bare_stack();
        let q = queue(c"/abandoned");
        let ev = SigEvent::thread(0, retried_once_addr(), attr_word(&mut a));
        assert_eq!(mq::mq_notify(q, Some(&ev)), 0);
        let used = liella_boundary::selftest::mq_records_in_use();
        // 1. The occupant holds the stack; the message's notification waits for it.
        let occupant = occupy_stack(&mut a);
        assert_eq!(mq::mq_send(q, c"m".as_ptr(), 1, 0), 0);
        assert!(until(|| liella_boundary::selftest::notifications_queued_for_stacks() == 1), "the message's notification waits for the stack");
        // 2. The stack is freed while no thread can be made: the notification is abandoned.
        let a0 = liella_boundary::selftest::mq_notifications_abandoned();
        liella_boundary::selftest::forbid_threads();
        BLOCK.store(0, SeqCst);
        assert!(until(|| liella_boundary::selftest::mq_notifications_abandoned() > a0), "the waiting notification could not be started");
        assert_eq!(liella_boundary::selftest::mq_records_in_use(), used, "the abandoned registration's record is kept for the retry");
        assert_eq!(RETRIED_CALLS.load(SeqCst), 0, "no thread, no call");
        // 3. Threads can be made again: the retry delivers it, once, and frees the record.
        liella_boundary::selftest::allow_threads();
        assert!(until(|| RETRIED_CALLS.load(SeqCst) == 1), "the abandoned notification is retried");
        settle(1, "the abandoned notification");
        assert_eq!(liella_boundary::selftest::mq_records_in_use(), used - 1, "its record is freed once it started");
        assert_eq!(liella_boundary::selftest::notifications_queued_for_stacks(), 0, "nothing left waiting");
        // 4. The message used the registration up: the queue takes a new one, on the free stack.
        let mut buf = [0 as core::ffi::c_char; 8];
        assert_eq!(mq::mq_receive(q, buf.as_mut_ptr(), buf.len(), None), 1);
        assert_eq!(mq::mq_notify(q, Some(&ev)), 0);
        assert_eq!(mq::mq_send(q, c"n".as_ptr(), 1, 0), 0);
        assert!(until(|| RETRIED_CALLS.load(SeqCst) == 2), "the new registration notifies");
        assert_eq!(tm::timer_delete(occupant), 0);
        assert_eq!(th::pthread_attr_destroy(&mut *a), 0);
        assert_eq!(io::close(q), 0);
        assert_eq!(mq::mq_unlink(c"/abandoned".as_ptr()), 0);
    });
}

#[test]
fn aio_refused_notification_retried() {
    // Clauses: s11_11_c_0 (D-11.11, owner ruling #680).
    // AIO completions whose notification threads cannot be made are retried with the timers'
    // back-off, not dropped (the reference drops them): each request is notified once when a
    // thread can be made, its result reported meanwhile, and a child of `fork` delivers none.
    // The failure is injected (`forbid_threads`), in a process of its own.
    alone("aio_refused_notification_retried", || {
        let file = Scratch::path("sigev-aio-retried");
        std::fs::write(&*file, b"0123456789").unwrap();
        let fd = io::liella_open3(file.c().as_ptr(), 0, 0);
        assert!(fd >= 0);
        let mut bufs = [[0u8; 4]; 3];
        let mut cbs = bufs.each_mut().map(|b| Cb { fildes: fd, buf: b.as_mut_ptr().addr(), nbytes: 4, notify: SIGEV_THREAD, function: retried_once as extern "C" fn(usize) as usize, ..Cb::default() });
        let p = cbs.as_mut_ptr();
        // The first request makes the engine and the retries' helper, and notifies.
        assert_eq!(io::aio_read(p.cast()), 0);
        assert!(until(|| RETRIED_CALLS.load(SeqCst) == 1), "notified");
        let (e0, r0) = (liella_boundary::selftest::aio_thread_episodes(), refused());
        liella_boundary::selftest::forbid_threads();
        for i in 1..3 {
            assert_eq!(io::aio_read(p.wrapping_add(i).cast()), 0);
        }
        assert!(until(|| refused() >= r0 + 4), "the notification threads were refused, then retried");
        for i in 1..3 {
            assert_eq!(io::aio_error(p.wrapping_add(i).cast()), 0, "the request completed");
            assert_eq!(io::aio_return(p.wrapping_add(i).cast()), 4);
        }
        assert_eq!(RETRIED_CALLS.load(SeqCst), 1, "no thread, no call");
        child_without_the_retry(|| true);
        liella_boundary::selftest::allow_threads();
        assert!(until(|| RETRIED_CALLS.load(SeqCst) == 3), "each request notified once a thread can be made");
        settle(3, "the retried notifications");
        let e1 = liella_boundary::selftest::aio_thread_episodes();
        assert_eq!((e1.0 - e0.0, e1.1 - e0.1), (2, 2), "a failure episode per notification, each ended by its start");
        assert_eq!(io::close(fd), 0);
    });
}

#[test]
fn aio_abandoned_notification_retried() {
    // Clauses: s11_11_c_0 (D-11.11, owner ruling #680).
    // An AIO completion's notification that waits for its caller-supplied stack (another
    // notification's thread is on it) and then cannot be started when the stack is free (no
    // thread can be made) keeps its block and is retried, as one whose thread cannot be made at
    // once: it arrives once a thread can be made, once, then its block is released; the request's
    // result stays, and a later request notifies on the stack. The failure is injected
    // (`forbid_threads`), in a process of its own.
    alone("aio_abandoned_notification_retried", || {
        let file = Scratch::path("sigev-aio-abandoned");
        std::fs::write(&*file, b"0123456789").unwrap();
        let fd = io::liella_open3(file.c().as_ptr(), 0, 0);
        assert!(fd >= 0);
        let mut a = bare_stack();
        let mut bufs = [[0u8; 4]; 2];
        let [b1, b2] = &mut bufs;
        let mk = |buf: &mut [u8; 4], a: &mut pthread_attr_t| Cb { fildes: fd, buf: buf.as_mut_ptr().addr(), nbytes: 4, notify: SIGEV_THREAD, function: retried_once as extern "C" fn(usize) as usize, attrs: attr_word(a) as usize, ..Cb::default() };
        let mut cb = mk(b1, &mut a);
        let p = &raw mut cb;
        // 1. The occupant holds the stack; the completion's notification waits for it.
        let occupant = occupy_stack(&mut a);
        assert_eq!(io::aio_read(p.cast()), 0);
        assert!(until(|| liella_boundary::selftest::notifications_queued_for_stacks() == 1), "the completion's notification waits for the stack");
        // 2. The stack is freed while no thread can be made: the notification is abandoned.
        let (a0, f0) = (liella_boundary::selftest::aio_notifications_abandoned(), liella_boundary::selftest::aio_free_records());
        liella_boundary::selftest::forbid_threads();
        BLOCK.store(0, SeqCst);
        assert!(until(|| liella_boundary::selftest::aio_notifications_abandoned() > a0), "the waiting notification could not be started");
        assert_eq!(liella_boundary::selftest::aio_free_records(), f0, "the abandoned notification's block is kept for the retry");
        assert_eq!(io::aio_error(p.cast()), 0, "the request completed");
        assert_eq!(io::aio_return(p.cast()), 4);
        // 3. Threads can be made again: the retry delivers it, once, and releases the block.
        liella_boundary::selftest::allow_threads();
        assert!(until(|| RETRIED_CALLS.load(SeqCst) == 1), "the abandoned notification is retried");
        settle(1, "the abandoned notification");
        assert!(until(|| liella_boundary::selftest::aio_free_records() == f0 + 1), "its block is released once it started");
        // 4. A later request notifies on the free stack.
        let mut cb2 = mk(b2, &mut a);
        let p2 = &raw mut cb2;
        assert_eq!(io::aio_read(p2.cast()), 0);
        assert!(until(|| RETRIED_CALLS.load(SeqCst) == 2), "the later request notifies");
        assert_eq!(io::aio_return(p2.cast()), 4);
        assert_eq!(tm::timer_delete(occupant), 0);
        assert_eq!(th::pthread_attr_destroy(&mut *a), 0);
        assert_eq!(io::close(fd), 0);
    });
}
