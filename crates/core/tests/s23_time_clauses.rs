// Specified in part from spec §23 (phase 5: §23.1–23.3; phase 6: the cancellation clause and
// §23.4 timers; phase 9: §23.5 broken-down time, §23.6 zones and §23.7 formatting and parsing).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §23: the clocks through the exports, and the translations glibc adds
//! through `liella_legacy::clock_compat` with the kernel's answers injected by `Faulty`.
//!
//! Timers (§23.4): `alarm`, the itimers and process-directed timer signals are process-wide,
//! so those clauses run as the only thread of a process (forked from a re-executed copy of the
//! test). Thread-directed signals (`SIGEV_THREAD_ID`), `SIGEV_NONE`, `SIGEV_THREAD` and
//! `timerfd` run in the test process; `SIGEV_THREAD` functions call only the library and
//! atomics (D-22.10).
//!
//! Broken-down time and zones (§23.5–23.6): `TZ`, the selected zone and the globals are
//! process-wide, so the clauses that depend on them run in a re-executed copy (`alone`).
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::as_conversions, clippy::indexing_slicing, missing_docs)]
#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/scratch.rs"]
mod scratch;
#[path = "support/timeout.rs"]
mod timeout;
#[path = "support/oracle.rs"]
mod oracle;

use core::ffi::c_void;
use std::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use liella::error::Errno;
use liella_abi::layout::{pthread_attr_t, struct_timeb};
use liella_boundary::errno;
use liella_boundary::posix::s22_threads as th;
use liella_boundary::posix::s23_time as x;
use liella_boundary::selftest::ChildEnd;
use liella_boundary::sys::CSys;
use liella_kernel::fd::Fd;
use liella_kernel::sys::Sys;
use liella_kernel::testing::Faulty;
use liella_kernel::uapi::{
    Itimerspec, Itimerval, Rlimit, SigEvent, SigInfo, SigSet, Timespec, Timeval, Timex, Timezone, RLIMIT_AS, RLIMIT_NOFILE, SIG_BLOCK, SIG_SETMASK, SIG_UNBLOCK,
};
use liella_legacy::clock_compat;

/// Build and run C conformance programs against the products (`conformance/run.sh`).
fn conformance(names: &[&str]) {
    static RUNNER: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = RUNNER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let products = ["target/liella/libc.a", "target/liella/liella-crt1.o"];
    assert!(products.iter().all(|p| root.join(p).exists()), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let o = std::process::Command::new("sh").arg(root.join("conformance/run.sh")).args(names).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

fn read(clock: i32) -> Option<Timespec> {
    let mut ts = Timespec::default();
    (x::clock_gettime(clock, Some(&mut ts)) == 0).then_some(ts)
}

fn secs(t: Timespec) -> f64 {
    t.sec as f64 + t.nsec as f64 / 1e9
}

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64()
}

fn root() -> bool {
    CSys.geteuid() == 0
}

/// Runs `body` in a child (this test re-executed alone): signal dispositions are process-wide.
fn alone(name: &str, body: impl FnOnce() -> bool) {
    if std::env::var("LIELLA_C23_ALONE").is_ok_and(|v| v == name) {
        CSys.exit_group(if body() { 0 } else { 1 });
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--test-threads=1"])
        .env("LIELLA_C23_ALONE", name)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{name} failed in its child: {}", child_report(&o));
}

#[test]
fn s23_1_c_0() {
    let r = read(0).unwrap();
    assert!((secs(r) - now()).abs() < 1.0, "the current value of the clock");
}

#[test]
fn s23_1_c_1() {
    for clock in [0, 1, 2, 3, 4, 5, 6, 7, 11] {
        let t = read(clock);
        assert!(t.is_some(), "clock {clock}");
        let t = t.unwrap();
        assert!((0..1_000_000_000).contains(&t.nsec), "clock {clock}");
    }
}

#[test]
fn s23_1_c_1_1() {
    let r = read(0).unwrap();
    assert!(r.sec > 1_700_000_000, "seconds since the Unix epoch");
    assert!((secs(r) - now()).abs() < 1.0);
    errno::set_raw(0);
    if !root() {
        assert_eq!((x::clock_settime(0, Some(&r)), errno::get()), (-1, Errno::EPERM.get()), "settable, with privilege");
    }
}

#[test]
fn s23_1_c_1_2() {
    let a = read(1).unwrap();
    let b = read(7).unwrap();
    assert!(secs(b) >= secs(a), "MONOTONIC <= BOOTTIME (suspend is not counted)");
    errno::set_raw(0);
    assert_eq!((x::clock_settime(1, Some(&a)), errno::get()), (-1, Errno::EINVAL.get()), "not settable");
}

#[test]
fn s23_1_c_1_3() {
    let boot = read(7).unwrap();
    let up: f64 = std::fs::read_to_string("/proc/uptime").unwrap().split_whitespace().next().unwrap().parse().unwrap();
    assert!((secs(boot) - up).abs() < 2.0, "BOOTTIME is the uptime, suspend included");
}

#[test]
fn s23_1_c_1_4() {
    let a = read(4).unwrap();
    std::thread::sleep(Duration::from_millis(20));
    let b = read(4).unwrap();
    let d = secs(b) - secs(a);
    assert!((0.019..1.0).contains(&d), "MONOTONIC_RAW advances with real time: {d}");
}

#[test]
fn s23_1_c_1_5() {
    let mut fine = Timespec::default();
    let mut coarse = Timespec::default();
    x::clock_getres(1, Some(&mut fine));
    x::clock_getres(6, Some(&mut coarse));
    assert!(coarse.nsec > fine.nsec, "lower resolution");
    let (m, mc) = (read(1).unwrap(), read(6).unwrap());
    assert!((secs(m) - secs(mc)).abs() < 0.1, "the same clock, coarser");
    let (r, rc) = (read(0).unwrap(), read(5).unwrap());
    assert!((secs(r) - secs(rc)).abs() < 0.1);
}

#[test]
fn s23_1_c_1_6() {
    let tai = read(11).unwrap();
    let real = read(0).unwrap();
    let mut tx = Timex::default();
    x::adjtimex(Some(&mut tx));
    assert!((secs(tai) - secs(real) - f64::from(tx.tai)).abs() < 1.0, "TAI = UTC + the kernel's TAI offset");
}

#[test]
fn s23_1_c_1_7() {
    let p0 = read(2).unwrap();
    let t0 = read(3).unwrap();
    // Spin until this thread has run 30 ms (not 30 ms of wall time: under load it may get far
    // less CPU in that time, which failed the gate once), at most 10 s.
    let spin = Instant::now();
    while secs(read(3).unwrap()) - secs(t0) < 0.03 && spin.elapsed() < Duration::from_secs(10) {
        std::hint::black_box(0u64);
    }
    let other = std::thread::spawn(|| read(3).unwrap()).join().unwrap();
    let (p1, t1) = (read(2).unwrap(), read(3).unwrap());
    assert!(secs(p1) - secs(p0) >= 0.02 && secs(t1) - secs(t0) >= 0.02, "CPU time of the process and of this thread");
    assert!(secs(other) < secs(t1), "per thread");
}

#[test]
fn s23_1_c_1_8() {
    let mut id = 0;
    assert_eq!(x::clock_getcpuclockid(CSys.getpid(), Some(&mut id)), 0);
    assert!(id < 0, "a dynamic ID encoding the PID");
    assert!(read(id).is_some());
    let tid_clock = ((!CSys.gettid()) << 3) | 1 | 4; // MAKE_THREAD_CPUCLOCK(tid, CPUCLOCK_SCHED)
    assert!(read(tid_clock).is_some(), "a thread's");
    assert!(read((!3) << 3 | 3).is_none(), "a PTP clock descriptor that is not open: EINVAL");
}

#[test]
fn s23_1_c_2() {
    let mut r = Timespec::default();
    for clock in [0, 1, 2, 3] {
        assert_eq!(x::clock_getres(clock, Some(&mut r)), 0);
        assert!(r.sec == 0 && r.nsec > 0);
    }
    let mut id = 0;
    assert_eq!(x::clock_getcpuclockid(0, Some(&mut id)), 0);
    assert_eq!(x::clock_getcpuclockid(i32::MAX - 7, Some(&mut id)), Errno::ESRCH.get(), "no such process");
}

#[test]
fn s23_1_c_3() {
    let t = x::time(None);
    let mut tv = Timeval { sec: 0, usec: 0 };
    let mut tz = Timezone { minuteswest: 9, dsttime: 9 };
    x::gettimeofday(Some(&mut tv), Some(&mut tz));
    let mut tb = struct_timeb { time: 0, millitm: 0, timezone: 9, dstflag: 9 };
    x::ftime(Some(&mut tb));
    assert!((tv.sec - t).abs() <= 1 && (tb.time - t).abs() <= 1, "the same clock at three resolutions");
    assert!(tv.usec < 1_000_000 && tb.millitm < 1000);
    assert_eq!((tz.minuteswest, tz.dsttime, tb.timezone, tb.dstflag), (0, 0, 0, 0), "zones zero-filled");
    // The kernel's failures reach the caller (time() reads the vDSO's time entry, D-23.8;
    // ftime is a translation of clock_gettime, D-23.1).
    let k = Faulty::new(CSys);
    k.fail("time", Errno::EINVAL, 0, 1);
    assert_eq!(clock_compat::time(&k), Err(Errno::EINVAL));
    k.fail("clock_gettime", Errno::EINVAL, 0, 1);
    assert_eq!(clock_compat::ftime(&k).map(|t| t.time), Err(Errno::EINVAL));
}

#[test]
fn s23_1_c_4() {
    let mut a = Timespec::default();
    assert_eq!(x::timespec_get(Some(&mut a), 1), 1);
    let b = read(0).unwrap();
    assert!((secs(b) - secs(a)).abs() < 0.1, "TIME_UTC is CLOCK_REALTIME");
    let mut r = Timespec::default();
    assert_eq!(x::timespec_getres(Some(&mut r), 1), 1);
    let mut rr = Timespec::default();
    x::clock_getres(0, Some(&mut rr));
    assert_eq!(r, rr, "its resolution");
    for base in [0, 2, 3, 4] {
        assert_eq!(x::timespec_get(Some(&mut a), base), 0, "base {base} is not supported in 2.39");
    }
}

#[test]
fn s23_1_c_5() {
    let a = x::clock();
    let p = read(2).unwrap();
    assert!((a as f64 / 1e6 - secs(p)).abs() < 0.05, "CPU time in millionths");
    let k = Faulty::new(CSys);
    k.fail("clock_gettime", Errno::EINVAL, 0, 1);
    assert_eq!(clock_compat::clock(&k), -1, "or -1");
}

#[test]
fn s23_1_g_0() {
    let mut last = read(1).unwrap();
    for _ in 0..100_000 {
        let t = read(1).unwrap();
        assert!((t.sec, t.nsec) >= (last.sec, last.nsec), "never backwards");
        assert!((0..1_000_000_000).contains(&t.nsec));
        last = t;
    }
}

#[test]
fn s23_1_f_0() {
    let mut ts = Timespec::default();
    errno::set_raw(0);
    assert_eq!((x::clock_gettime(12345, Some(&mut ts)), errno::get()), (-1, Errno::EINVAL.get()), "unknown clock");
    // EOVERFLOW concerns 32-bit time_t; x86-64's time_t is 64-bit, so far dates are values.
    assert_eq!(std::mem::size_of_val(&ts.sec), 8);
}

#[test]
fn s23_2_c_1() {
    errno::set_raw(0);
    let tv = Timeval { sec: 1, usec: 0 };
    let tz = Timezone { minuteswest: 60, dsttime: 0 };
    assert_eq!((x::settimeofday(Some(&tv), Some(&tz)), errno::get()), (-1, Errno::EINVAL.get()), "tz with a time: EINVAL");
    if !root() {
        assert_eq!((x::settimeofday(None, Some(&tz)), errno::get()), (-1, Errno::EPERM.get()), "the tz-only call: CAP_SYS_TIME");
        assert_eq!((x::settimeofday(Some(&tv), None), errno::get()), (-1, Errno::EPERM.get()));
    }
    // The paths, with the kernel's answers injected: tz-only goes to settimeofday, a time to
    // clock_settime.
    let k = Faulty::new(CSys);
    k.fail("settimeofday", Errno::EPERM, 0, 1);
    k.fail("clock_settime", Errno::EPERM, 0, 1);
    assert_eq!(clock_compat::settimeofday(&k, None, Some(&tz)), Err(Errno::EPERM));
    assert_eq!(clock_compat::settimeofday(&k, Some(&tv), None), Err(Errno::EPERM));
    assert_eq!(k.injected(), 2);
}

#[test]
fn s23_2_c_2() {
    let mut old = Timeval { sec: 9, usec: 9 };
    assert_eq!(x::adjtime(None, Some(&mut old)), 0);
    errno::set_raw(0);
    if !root() {
        assert_eq!((x::adjtime(Some(&Timeval { sec: 0, usec: 500 }), None), errno::get()), (-1, Errno::EPERM.get()), "slewing needs privilege");
    }
    // The slew request: ADJ_OFFSET_SINGLESHOT with the delta in microseconds.
    let k = Faulty::new(CSys);
    k.fail("clock_adjtime", Errno::EPERM, 0, 1);
    assert_eq!(clock_compat::adjtime(&k, Some(&Timeval { sec: 1, usec: 2_500_000 })), Err(Errno::EPERM), "3.5 s normalised, then the kernel");
    assert_eq!(clock_compat::adjtime(&k, Some(&Timeval { sec: 2200, usec: 0 })), Err(Errno::EINVAL), "beyond INT_MAX microseconds");
}

#[test]
fn s23_2_c_3() {
    let mut tx = Timex::default();
    let state = x::ntp_adjtime(Some(&mut tx));
    assert!((0..=5).contains(&state));
    assert!(tx.tick > 0, "the NTP state: tick, frequency, status, TAI offset");
    let mut cx = Timex::default();
    assert_eq!(x::clock_adjtime(0, Some(&mut cx)), state, "the same through clock_adjtime");
    assert_eq!((cx.tick, cx.tai), (tx.tick, tx.tai));
}

#[test]
fn s23_2_c_4() {
    let mut v = clock_compat::NtpTimevalOld::default();
    assert!(x::ntp_gettime(Some(&mut v)) >= 0);
    assert!(v.maxerror >= v.esterror && v.esterror >= 0, "the time with its error estimates");
}

#[test]
fn s23_3_c_0() {
    let t = Instant::now();
    let cpu0 = read(3).unwrap();
    x::usleep(50_000);
    let cpu1 = read(3).unwrap();
    assert!(t.elapsed() >= Duration::from_millis(50));
    assert!(secs(cpu1) - secs(cpu0) < 0.02, "suspended, not spinning");
}

#[test]
fn s23_3_c_1() {
    alone("s23_3_c_1", || {
        let (res, _) = liella_boundary::selftest::with_counting_handler(14, || {
            let (pid, tid) = (CSys.getpid(), CSys.gettid());
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(100));
                let _ = CSys.tgkill(pid, tid, 14);
            });
            let mut rem = Timespec::default();
            let r = x::nanosleep(Some(&Timespec { sec: 5, nsec: 0 }), Some(&mut rem));
            (r, errno::get(), rem)
        })
        .unwrap();
        let (r, e, rem) = res;
        r == -1 && e == Errno::EINTR.get() && rem.sec >= 4 && rem.sec < 5
    });
}

#[test]
fn s23_3_c_2() {
    let start = read(0).unwrap();
    let until = Timespec { sec: start.sec + 1, nsec: 0 };
    assert_eq!(x::clock_nanosleep(0, 1, Some(&until), None), 0, "absolute, on CLOCK_REALTIME");
    assert!(secs(read(0).unwrap()) >= secs(until));
    let rel = Timespec { sec: 0, nsec: 5_000_000 };
    assert_eq!(x::clock_nanosleep(1, 0, Some(&rel), None), 0, "relative");
    errno::set_raw(0);
    assert_eq!(x::clock_nanosleep(1, 0, Some(&Timespec { sec: 0, nsec: -1 }), None), Errno::EINVAL.get(), "the error number, returned");
    assert_eq!(errno::get(), 0);
}

#[test]
fn s23_3_c_3() {
    alone("s23_3_c_3", || {
        let (left, _) = liella_boundary::selftest::with_counting_handler(14, || {
            let (pid, tid) = (CSys.getpid(), CSys.gettid());
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(1100));
                let _ = CSys.tgkill(pid, tid, 14);
            });
            x::sleep(5)
        })
        .unwrap();
        let t = Instant::now();
        let full = x::usleep(1_200_000) == 0 && t.elapsed() >= Duration::from_millis(1200);
        left == 3 && full
    });
}

/// §23.3 Cancellation: Yes.
#[test]
fn s23_3_x_0() {
    conformance(&["s22_cancel_points", "s22_cancel"]);
}
// ------------------------------------------------------------------------------ §23.4 timers

const SIGUSR1: i32 = 10;
const SIGUSR2: i32 = 12;
const SIGALRM: i32 = 14;
const SIGVTALRM: i32 = 26;
const SIGPROF: i32 = 27;
const SI_TIMER: i32 = -2;
const SIGEV_SIGNAL: i32 = 0;
const SIGEV_NONE: i32 = 1;
const SIGEV_THREAD_ID: i32 = 4;
const TIMER_ABSTIME: i32 = 1;
const EAGAIN: i32 = 11;
const ENOMEM: i32 = 12;
const EINVAL: i32 = 22;
const RLIMIT_SIGPENDING: u32 = 11;

fn sigset(sigs: &[i32]) -> SigSet {
    sigs.iter().fold(SigSet::empty(), |s, &n| s.with(n))
}

fn usec(us: i64) -> Timeval {
    Timeval { sec: us / 1_000_000, usec: us % 1_000_000 }
}

fn nsec(ns: i64) -> Timespec {
    Timespec { sec: ns / 1_000_000_000, nsec: ns % 1_000_000_000 }
}

fn itv(value_us: i64, interval_us: i64) -> Itimerval {
    Itimerval { interval: usec(interval_us), value: usec(value_us) }
}

fn its(value_ns: i64, interval_ns: i64) -> Itimerspec {
    Itimerspec { interval: nsec(interval_ns), value: nsec(value_ns) }
}

fn ns(t: Timespec) -> i64 {
    t.sec * 1_000_000_000 + t.nsec
}

fn us(t: Timeval) -> i64 {
    t.sec * 1_000_000 + t.usec
}

fn ptr(t: usize) -> *mut c_void {
    core::ptr::with_exposed_provenance_mut(t)
}

/// A timer through the export: its `timer_t`, or the `errno` of the failure.
fn timer(clock: i32, ev: Option<&SigEvent>) -> Result<usize, i32> {
    let mut t = core::ptr::null_mut();
    errno::set_raw(0);
    if x::timer_create(clock, ev, Some(&mut t)) == 0 { Ok(t.expose_provenance()) } else { Err(errno::get()) }
}

/// Wait up to `ms` for one of `set` (blocked in the calling thread); its `siginfo_t`.
fn wait_signal(set: SigSet, ms: i64) -> Option<SigInfo> {
    let mut info = SigInfo::default();
    CSys.sigtimedwait(&set, Some(&mut info), Some(&nsec(ms * 1_000_000))).ok().map(|_| info)
}

fn sleep_ms(ms: u64) {
    std::thread::sleep(Duration::from_millis(ms));
}

/// Runs `body` as the only thread of a process: forked from a re-executed copy of `test` (so
/// process-wide timers, limits and signal dispositions are its own, and a process-directed
/// signal can only reach it). `body` returns 0, or the number of the first check that failed.
/// `probe` names the body among those of one test.
fn single(test: &str, probe: &str, body: fn(&CSys) -> i32) {
    match std::env::var("LIELLA_C23_ALONE") {
        Ok(p) if p == probe => {
            let end = liella_boundary::selftest::in_child(body);
            eprintln!("{probe}: {end:?}");
            CSys.exit_group(if end == Ok(ChildEnd::Exited(0)) { 0 } else { 1 });
        }
        Ok(_) => {}
        Err(_) => {
            let o = std::process::Command::new(std::env::current_exe().unwrap())
                .args([test, "--exact", "--test-threads=1", "--nocapture"])
                .env("LIELLA_C23_ALONE", probe)
                .output()
                .unwrap();
            assert_eq!(o.status.code(), Some(0), "{probe} failed in its child: {}", child_report(&o));
        }
    }
}

/// Burn user CPU time until one of `set` is pending (taken), for at most `limit` of real time.
fn burn_until(set: SigSet, limit: Duration) -> Option<i32> {
    let t = Instant::now();
    let mut acc = 0u64;
    while t.elapsed() < limit {
        for i in 0..200_000u64 {
            acc = std::hint::black_box(acc.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(i));
        }
        if let Ok(sig) = CSys.sigtimedwait(&set, None, Some(&Timespec::default())) {
            return Some(sig);
        }
    }
    None
}

#[test]
fn s23_4_c_1() {
    single("s23_4_c_1", "s23_4_c_1", |k| {
        let alrm = sigset(&[SIGALRM]);
        let _ = k.sigprocmask(SIG_BLOCK, Some(&alrm));
        if x::alarm(0) != 0 || x::alarm(5) != 0 {
            return 1;
        }
        // Replaced, returning what remained of the previous alarm.
        if x::alarm(1) != 5 {
            return 2;
        }
        let t = Instant::now();
        if wait_signal(alrm, 3000).map(|i| i.signo) != Some(SIGALRM) || t.elapsed() < Duration::from_millis(900) {
            return 3;
        }
        if x::alarm(0) != 0 {
            return 4;
        }
        // It cannot fail: the largest request is taken.
        x::alarm(u32::MAX);
        if x::alarm(0) < i32::MAX as u32 {
            return 5;
        }
        // ualarm: the same in microseconds, with a period.
        if x::ualarm(200_000, 0) != 0 {
            return 6;
        }
        if !(100_000..=200_000).contains(&x::ualarm(0, 0)) {
            return 7;
        }
        let t = Instant::now();
        x::ualarm(30_000, 20_000);
        let got = (0..3).filter(|_| wait_signal(alrm, 2000).is_some()).count();
        x::ualarm(0, 0);
        if got != 3 || t.elapsed() < Duration::from_millis(65) {
            return 8;
        }
        0
    });
}

#[test]
fn s23_4_c_2() {
    single("s23_4_c_2", "s23_4_c_2", |k| {
        let set = sigset(&[SIGALRM, SIGVTALRM, SIGPROF]);
        let _ = k.sigprocmask(SIG_BLOCK, Some(&set));
        // ITIMER_REAL: real time, SIGALRM.
        let t = Instant::now();
        if x::setitimer(0, Some(&itv(30_000, 0)), None) != 0 {
            return 1;
        }
        if wait_signal(set, 2000).map(|i| i.signo) != Some(SIGALRM) || t.elapsed() < Duration::from_millis(25) {
            return 2;
        }
        // Microseconds, value and period, read back and returned as the old setting.
        if x::setitimer(0, Some(&itv(2_500_000, 250_000)), None) != 0 {
            return 3;
        }
        let mut cur = Itimerval::default();
        if x::getitimer(0, Some(&mut cur)) != 0 || us(cur.interval) != 250_000 || !(2_000_000..=2_500_000).contains(&us(cur.value)) {
            return 4;
        }
        let mut old = Itimerval::default();
        if x::setitimer(0, Some(&itv(0, 0)), Some(&mut old)) != 0 || us(old.interval) != 250_000 || us(old.value) > 2_500_000 {
            return 5;
        }
        // ITIMER_VIRTUAL: the process's user time, SIGVTALRM.
        if x::setitimer(1, Some(&itv(20_000, 0)), None) != 0 || burn_until(set, Duration::from_secs(10)) != Some(SIGVTALRM) {
            return 6;
        }
        // ITIMER_PROF: user and system time, SIGPROF.
        if x::setitimer(2, Some(&itv(20_000, 0)), None) != 0 || burn_until(set, Duration::from_secs(10)) != Some(SIGPROF) {
            return 7;
        }
        // Three timers only.
        errno::set_raw(0);
        if x::setitimer(3, Some(&itv(0, 0)), None) != -1 || errno::get() != EINVAL {
            return 8;
        }
        0
    });
}

#[test]
fn s23_4_c_3() {
    // Any number per process: many kernel-notified and many SIGEV_THREAD timers at once.
    let none = SigEvent::new(SIGEV_NONE, 0, 0, 0);
    let mut ids: Vec<usize> = (0..500).map(|_| timer(1, Some(&none)).unwrap()).collect();
    let thread = SigEvent::thread(0, quiet as extern "C" fn(usize) as usize as u64, 0);
    ids.extend((0..40).map(|_| timer(1, Some(&thread)).unwrap()));
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), ids.len(), "every timer has its own id");
    for t in ids {
        assert_eq!(x::timer_delete(ptr(t)), 0);
    }
}

extern "C" fn quiet(_v: usize) {}

static N_FLAGGED: AtomicU32 = AtomicU32::new(0);

extern "C" fn flagged(_v: usize) {
    N_FLAGGED.store(1, Ordering::SeqCst);
}

#[test]
fn s23_4_c_3_1() {
    single("s23_4_c_3_1", "s23_4_c_3_1", |k| {
        let set = sigset(&[SIGUSR1, SIGALRM]);
        let _ = k.sigprocmask(SIG_BLOCK, Some(&set));
        // SIGEV_SIGNAL: the chosen signal, from a timer (SI_TIMER), with the event's value.
        let Ok(t) = timer(1, Some(&SigEvent::new(SIGEV_SIGNAL, SIGUSR1, 0x5eed, 0))) else { return 1 };
        if x::timer_settime(ptr(t), 0, Some(&its(10_000_000, 0)), None) != 0 {
            return 2;
        }
        match wait_signal(set, 2000) {
            Some(i) if i.signo == SIGUSR1 && i.code == SI_TIMER && i.value() == 0x5eed => {}
            _ => return 3,
        }
        x::timer_delete(ptr(t));
        // No sigevent: SIGEV_SIGNAL with SIGALRM, and the timer's id as the value.
        let Ok(t) = timer(0, None) else { return 4 };
        x::timer_settime(ptr(t), 0, Some(&its(10_000_000, 0)), None);
        match wait_signal(set, 2000) {
            Some(i) if i.signo == SIGALRM && i.code == SI_TIMER && i.value() as u32 == t as u32 => {}
            _ => return 5,
        }
        x::timer_delete(ptr(t));
        0
    });
}

#[test]
fn s23_4_c_3_2() {
    // SIGEV_THREAD_ID: the signal goes to the named thread only.
    let (tx, rx) = std::sync::mpsc::channel();
    let waiter = std::thread::spawn(move || {
        let _ = CSys.sigprocmask(SIG_BLOCK, Some(&sigset(&[SIGUSR2])));
        tx.send(CSys.gettid()).unwrap();
        let got = wait_signal(sigset(&[SIGUSR2]), 3000).map(|i| (i.signo, i.code, i.value()));
        let _ = CSys.sigprocmask(SIG_UNBLOCK, Some(&sigset(&[SIGUSR2])));
        got
    });
    let tid = rx.recv().unwrap();
    let t = timer(1, Some(&SigEvent::new(SIGEV_THREAD_ID, SIGUSR2, 77, tid))).unwrap();
    assert_eq!(x::timer_settime(ptr(t), 0, Some(&its(5_000_000, 0)), None), 0);
    assert_eq!(waiter.join().unwrap(), Some((SIGUSR2, SI_TIMER, 77)), "delivered to the named thread");
    assert!(!CSys.sigpending().unwrap().contains(SIGUSR2), "and to no other");
    assert_eq!(x::timer_delete(ptr(t)), 0);
    assert_eq!(timer(1, Some(&SigEvent::new(SIGEV_THREAD_ID, SIGUSR2, 0, 1))), Err(EINVAL), "a thread of another process");
}

// The SIGEV_THREAD notifications of s23_4_c_3_3 (functions running on notification threads
// call only the library and atomics: D-22.10).
static N_CALLS: AtomicU32 = AtomicU32::new(0);
static N_RUNNING: AtomicU32 = AtomicU32::new(0);
static N_MAX_RUNNING: AtomicU32 = AtomicU32::new(0);
static N_VALUE: AtomicUsize = AtomicUsize::new(0);
static N_MASK: AtomicU64 = AtomicU64::new(0);
static N_DETACHED: AtomicI32 = AtomicI32::new(-1);
static N_TIMER: AtomicUsize = AtomicUsize::new(0);
static N_TIDS: [AtomicI32; 6] = [const { AtomicI32::new(0) }; 6];
static N_OVERRUNS: [AtomicI32; 6] = [const { AtomicI32::new(-1) }; 6];
static N_STACK: AtomicUsize = AtomicUsize::new(0);
static N_EXITS: AtomicU32 = AtomicU32::new(0);

extern "C" fn notified(v: usize) {
    let n = N_CALLS.fetch_add(1, Ordering::SeqCst) as usize;
    let running = N_RUNNING.fetch_add(1, Ordering::SeqCst) + 1;
    N_MAX_RUNNING.fetch_max(running, Ordering::SeqCst);
    if let (Some(tid), Some(over)) = (N_TIDS.get(n), N_OVERRUNS.get(n)) {
        tid.store(CSys.gettid(), Ordering::SeqCst);
        over.store(x::timer_getoverrun(ptr(N_TIMER.load(Ordering::SeqCst))), Ordering::SeqCst);
    }
    if n == 0 {
        N_VALUE.store(v, Ordering::SeqCst);
        N_MASK.store(CSys.sigprocmask(SIG_BLOCK, None).map_or(0, |m| m.0), Ordering::SeqCst);
        let mut a = pthread_attr_t { __size: [0; 56] };
        let mut ds = -1;
        if th::pthread_getattr_np(th::pthread_self(), &mut a) == 0 {
            th::pthread_attr_getdetachstate(&a, Some(&mut ds));
            th::pthread_attr_destroy(&mut a);
        }
        N_DETACHED.store(ds, Ordering::SeqCst);
        // A slow call: the timer keeps expiring meanwhile.
        let _ = CSys.clock_nanosleep(1, 0, &nsec(40_000_000), None);
    }
    N_RUNNING.fetch_sub(1, Ordering::SeqCst);
}

extern "C" fn sized(_v: usize) {
    let mut a = pthread_attr_t { __size: [0; 56] };
    let mut size = 0usize;
    if th::pthread_getattr_np(th::pthread_self(), &mut a) == 0 {
        th::pthread_attr_getstacksize(&a, Some(&mut size));
        th::pthread_attr_destroy(&mut a);
    }
    N_STACK.store(size, Ordering::SeqCst);
}

extern "C" fn exits(_v: usize) {
    N_EXITS.fetch_add(1, Ordering::SeqCst);
    th::pthread_exit(core::ptr::null_mut());
}

fn until(limit: Duration, done: impl Fn() -> bool) -> bool {
    let t = Instant::now();
    while !done() && t.elapsed() < limit {
        sleep_ms(2);
    }
    done()
}

#[test]
fn s23_4_c_3_3() {
    let f = notified as extern "C" fn(usize) as usize as u64;
    let t = timer(1, Some(&SigEvent::thread(0xfeed, f, 0))).unwrap();
    N_TIMER.store(t, Ordering::SeqCst);
    assert_eq!(x::timer_settime(ptr(t), 0, Some(&its(4_000_000, 4_000_000)), None), 0);
    assert!(until(Duration::from_secs(5), || N_CALLS.load(Ordering::SeqCst) >= 4), "the function is called at the expiries");
    x::timer_settime(ptr(t), 0, Some(&its(0, 0)), None);
    sleep_ms(60);
    let tids: Vec<i32> = N_TIDS.iter().take(4).map(|a| a.load(Ordering::SeqCst)).collect();
    let me = CSys.gettid();
    assert!(tids.iter().all(|&t| t != 0 && t != me), "on a thread other than the creator's: {tids:?}");
    let mut distinct = tids.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), tids.len(), "a new thread for each notification: {tids:?}");
    assert_eq!(N_VALUE.load(Ordering::SeqCst), 0xfeed, "with the event's value");
    assert_eq!(N_DETACHED.load(Ordering::SeqCst), 1, "detached");
    assert_eq!(N_MASK.load(Ordering::SeqCst), !(1u64 << 31) & !(1 << 8) & !(1 << 18), "every application signal blocked (but SIGKILL, SIGSTOP)");
    // D-23.3: one notification at a time; the expiries during the slow first call are the
    // second call's overruns.
    assert_eq!(N_MAX_RUNNING.load(Ordering::SeqCst), 1, "one notification at a time");
    assert!(N_OVERRUNS[1].load(Ordering::SeqCst) >= 4, "overruns: {:?}", N_OVERRUNS);
    // Nothing runs after timer_delete.
    assert_eq!(x::timer_settime(ptr(t), 0, Some(&its(1_000_000, 1_000_000)), None), 0);
    sleep_ms(20);
    assert_eq!(x::timer_delete(ptr(t)), 0);
    sleep_ms(10);
    let n = N_CALLS.load(Ordering::SeqCst);
    sleep_ms(60);
    assert_eq!(N_CALLS.load(Ordering::SeqCst), n, "no call after timer_delete");

    // The attributes are the event's (a copy: the caller may destroy them at once).
    let mut a = pthread_attr_t { __size: [0; 56] };
    assert_eq!(th::pthread_attr_init(&mut a), 0);
    assert_eq!(th::pthread_attr_setstacksize(&mut a, 1 << 20), 0);
    let f = sized as extern "C" fn(usize) as usize as u64;
    let t = timer(1, Some(&SigEvent::thread(0, f, core::ptr::from_mut(&mut a).expose_provenance() as u64))).unwrap();
    th::pthread_attr_destroy(&mut a);
    assert_eq!(x::timer_settime(ptr(t), 0, Some(&its(2_000_000, 0)), None), 0);
    assert!(until(Duration::from_secs(5), || N_STACK.load(Ordering::SeqCst) != 0));
    assert!(N_STACK.load(Ordering::SeqCst) >= 1 << 20, "the stack size of the attributes");
    x::timer_delete(ptr(t));

    // However the function ends its thread, the timer notifies again.
    let f = exits as extern "C" fn(usize) as usize as u64;
    let t = timer(1, Some(&SigEvent::thread(0, f, 0))).unwrap();
    assert_eq!(x::timer_settime(ptr(t), 0, Some(&its(3_000_000, 3_000_000)), None), 0);
    assert!(until(Duration::from_secs(5), || N_EXITS.load(Ordering::SeqCst) >= 3), "pthread_exit in the function");
    x::timer_delete(ptr(t));
}

#[test]
fn s23_4_c_3_4() {
    // SIGEV_NONE: the timer runs, nothing is delivered.
    let t = timer(1, Some(&SigEvent::new(SIGEV_NONE, SIGUSR1, 0, 0))).unwrap();
    let mut cur = Itimerspec::default();
    assert_eq!(x::timer_settime(ptr(t), 0, Some(&its(200_000_000, 1_234_567)), None), 0, "relative");
    assert_eq!(x::timer_gettime(ptr(t), Some(&mut cur)), 0);
    assert!((100_000_000..=200_000_000).contains(&ns(cur.value)), "{cur:?}");
    assert_eq!(ns(cur.interval), 1_234_567, "a nanosecond interval");
    let start = read(1).unwrap();
    let at = Timespec { sec: start.sec + 1, nsec: start.nsec };
    assert_eq!(x::timer_settime(ptr(t), TIMER_ABSTIME, Some(&its(ns(at), 2_000_000)), None), 0, "absolute");
    x::timer_gettime(ptr(t), Some(&mut cur));
    assert!((800_000_000..=1_000_000_000).contains(&ns(cur.value)), "{cur:?}");
    let past = Timespec { sec: start.sec - 10, nsec: 0 };
    assert_eq!(x::timer_settime(ptr(t), TIMER_ABSTIME, Some(&its(ns(past), 2_000_000)), None), 0, "a past absolute time");
    sleep_ms(20);
    x::timer_gettime(ptr(t), Some(&mut cur));
    assert!(ns(cur.value) > 0 && ns(cur.value) <= 2_000_000, "expired at once, then periodic: {cur:?}");
    assert!(!CSys.sigpending().unwrap().contains(SIGUSR1), "nothing delivered");
    assert_eq!(x::timer_getoverrun(ptr(t)), 0, "no notification, no overrun");
    x::timer_delete(ptr(t));

    // timer_getoverrun: the expiries missed while the signal stayed pending.
    let u1 = sigset(&[SIGUSR1]);
    let old = CSys.sigprocmask(SIG_BLOCK, Some(&u1)).unwrap();
    let t = timer(1, Some(&SigEvent::new(SIGEV_THREAD_ID, SIGUSR1, 5, CSys.gettid()))).unwrap();
    assert_eq!(x::timer_settime(ptr(t), 0, Some(&its(1_000_000, 1_000_000)), None), 0);
    sleep_ms(40);
    let info = wait_signal(u1, 2000).unwrap();
    let over = x::timer_getoverrun(ptr(t));
    x::timer_delete(ptr(t));
    let _ = wait_signal(u1, 0);
    let _ = CSys.sigprocmask(SIG_SETMASK, Some(&old));
    assert!(over > 10, "{over}");
    assert_eq!((info.fields[0] >> 32) as i32, over, "the signal's si_overrun");
}

#[test]
fn s23_4_c_4() {
    let fd = x::timerfd_create(1, 0o4000 | 0o2000000);
    assert!(fd >= 0);
    let mut n = [0u8; 8];
    assert_eq!(CSys.read(Fd::new(fd), &mut n), Err(Errno::EAGAIN), "disarmed: nothing to read");
    assert_eq!(x::timerfd_settime(fd, 0, Some(&its(10_000_000, 0)), None), 0);
    sleep_ms(30);
    assert_eq!(CSys.read(Fd::new(fd), &mut n), Ok(8));
    assert_eq!(u64::from_ne_bytes(n), 1, "the expirations, as a counter");
    assert_eq!(x::timerfd_settime(fd, 0, Some(&its(1_000_000, 1_000_000)), None), 0);
    sleep_ms(30);
    assert_eq!(CSys.read(Fd::new(fd), &mut n), Ok(8));
    assert!(u64::from_ne_bytes(n) >= 10, "every expiration counted: {}", u64::from_ne_bytes(n));
    let _ = CSys.close(Fd::new(fd));
    // TFD_TIMER_CANCEL_ON_SET (with an absolute realtime timer): a read fails with ECANCELED
    // once the realtime clock is set (which needs CAP_SYS_TIME).
    let fd = x::timerfd_create(0, 0o4000 | 0o2000000);
    let now = read(0).unwrap();
    assert_eq!(x::timerfd_settime(fd, 1 | 2, Some(&its(ns(now) + 100_000_000_000, 0)), None), 0);
    if root() {
        assert_eq!(x::clock_settime(0, Some(&read(0).unwrap())), 0);
        assert_eq!(CSys.read(Fd::new(fd), &mut n), Err(Errno::ECANCELED));
    } else {
        assert_eq!(CSys.read(Fd::new(fd), &mut n), Err(Errno::EAGAIN), "the clock was not set");
    }
    let _ = CSys.close(Fd::new(fd));
}

#[test]
fn s23_4_f_0() {
    // No `forbid_threads` here: it is process-wide, and the tests of this file share one process
    // under `cargo test` (a SIGEV_THREAD timer of another test then lost its notification:
    // S23-4-C-3-3-LOAD). The two probes below forbid threads in their own child processes.
    // EINVAL: an unknown clock, notification or signal, a bad time, an unknown timer.
    assert_eq!(timer(99, Some(&SigEvent::new(SIGEV_NONE, 0, 0, 0))), Err(EINVAL));
    assert_eq!(timer(1, Some(&SigEvent::new(99, SIGUSR1, 0, 0))), Err(EINVAL));
    assert_eq!(timer(1, Some(&SigEvent::new(SIGEV_SIGNAL, 65, 0, 0))), Err(EINVAL));
    let t = timer(1, Some(&SigEvent::new(SIGEV_NONE, 0, 0, 0))).unwrap();
    errno::set_raw(0);
    let bad = Itimerspec { interval: Timespec::default(), value: Timespec { sec: 0, nsec: 1_000_000_000 } };
    assert_eq!(x::timer_settime(ptr(t), 0, Some(&bad), None), -1);
    assert_eq!(errno::get(), EINVAL);
    x::timer_delete(ptr(t));
    errno::set_raw(0);
    assert_eq!(x::timer_gettime(ptr(t), Some(&mut Itimerspec::default())), -1);
    assert_eq!(errno::get(), EINVAL, "a deleted timer");
    errno::set_raw(0);
    assert_eq!(x::timerfd_create(99, 0), -1);
    assert_eq!(errno::get(), EINVAL);
    // EAGAIN: resource limits (queued-signal slots for a signal timer; descriptors for a
    // SIGEV_THREAD timer).
    single("s23_4_f_0", "s23_4_f_0/eagain", |k| {
        liella_boundary::selftest::forbid_threads();
        if !liella_boundary::selftest::threads_forbidden() {
            return 9;
        }
        let none = Rlimit { cur: 0, max: 0 };
        if k.prlimit(0, RLIMIT_SIGPENDING, Some(&none)).is_err() || timer(1, Some(&SigEvent::new(SIGEV_SIGNAL, SIGUSR1, 0, 0))) != Err(EAGAIN) {
            return 1;
        }
        let f = quiet as extern "C" fn(usize) as usize as u64;
        if k.prlimit(0, RLIMIT_NOFILE, Some(&none)).is_err() || timer(1, Some(&SigEvent::thread(0, f, 0))) != Err(EAGAIN) {
            return 2;
        }
        0
    });
    // ENOMEM: no memory for a SIGEV_THREAD timer's record.
    single("s23_4_f_0", "s23_4_f_0/enomem", |k| {
        liella_boundary::selftest::forbid_threads();
        if !liella_boundary::selftest::threads_forbidden() {
            return 9;
        }
        // Read into a stack buffer: a forked child must not allocate (FORK-CHILD-ALLOC-LINT).
        let mut statm = [0u8; 256];
        let n = std::fs::File::open("/proc/self/statm").and_then(|mut f| std::io::Read::read(&mut f, &mut statm)).unwrap_or(0);
        let Some(vm) = core::str::from_utf8(&statm[..n]).ok().and_then(|s| s.split(' ').next().and_then(|p| p.parse::<u64>().ok())) else {
            return 1;
        };
        let cap = Rlimit { cur: vm * 4096, max: u64::MAX };
        if k.prlimit(0, RLIMIT_AS, Some(&cap)).is_err() {
            return 2;
        }
        // Every size in 16-byte steps, not a few powers of two: a size class the loop skips may
        // hold free blocks of a slab that earlier allocations opened (e.g. a growing `realloc`),
        // and the record would come from there.
        for size in core::iter::once(1usize << 20).chain((1..=4096).rev().map(|i| i * 16)) {
            while !liella_boundary::selftest::opaque::malloc(size).is_null() {}
        }
        let f = quiet as extern "C" fn(usize) as usize as u64;
        if timer(1, Some(&SigEvent::thread(0, f, 0))) != Err(ENOMEM) {
            return 3;
        }
        0
    });
    // The probes above forbid threads only in their children: the threads of this process are
    // still allowed (the other tests of the file run in it at the same time). A SIGEV_THREAD
    // notification here is the check, and it fails every time if this test forbids threads.
    let f = flagged as extern "C" fn(usize) as usize as u64;
    let t = timer(1, Some(&SigEvent::thread(0, f, 0)));
    assert!(t.is_ok(), "a notification thread can still be made in this process");
    let t = t.unwrap();
    assert_eq!(x::timer_settime(ptr(t), 0, Some(&its(1_000_000, 0)), None), 0);
    assert!(!liella_boundary::selftest::threads_forbidden(), "threads are forbidden in the shared test process");
    assert!(until(Duration::from_secs(5), || N_FLAGGED.load(Ordering::SeqCst) != 0), "a notification thread can still be made in this process");
    x::timer_delete(ptr(t));
}
// ------------------------------------------------------------------------------ §23.5–23.6
//
// Broken-down time and zones through the exports. `TZ`, `TZDIR`, the selected zone and the
// globals are process-wide, so every clause that depends on the zone runs in a re-executed
// child (`alone`). Zone files are the system's (`/usr/share/zoneinfo`); private zone files are
// copies of them in a temporary directory.

use core::ffi::c_char;
use std::ffi::CString;

use liella_abi::layout::struct_tm;
use liella_boundary::posix::s01_startup as envx;
use liella_boundary::posix::s04_strings as strs;

const EOVERFLOW: i32 = 75;
const EFAULT: i32 = 14;

fn tm_zero() -> struct_tm {
    struct_tm { tm_sec: 0, tm_min: 0, tm_hour: 0, tm_mday: 0, tm_mon: 0, tm_year: 0, tm_wday: 0, tm_yday: 0, tm_isdst: 0, tm_gmtoff: 0, tm_zone: 0 }
}

/// A `struct tm` for `y-mo-d h:mi:s` (month 1-based) with `isdst`; weekday and day of the year
/// deliberately wrong (mktime/timegm must ignore and recompute them).
fn tm_of(y: i32, mo: i32, d: i32, h: i32, mi: i32, s: i32, isdst: i32) -> struct_tm {
    struct_tm { tm_sec: s, tm_min: mi, tm_hour: h, tm_mday: d, tm_mon: mo - 1, tm_year: y - 1900, tm_wday: 99, tm_yday: 999, tm_isdst: isdst, tm_gmtoff: 7, tm_zone: 0 }
}

/// The C text at `p`, through the exported `strlen`/`memcpy` (no unsafe in tests).
fn text_at(p: usize) -> String {
    let p: *const c_char = core::ptr::with_exposed_provenance(p);
    let n = strs::strlen(p);
    let mut v = vec![0u8; n];
    strs::memcpy(v.as_mut_ptr().cast(), p.cast(), n);
    String::from_utf8(v).unwrap()
}

/// The `struct tm` at `p` (a library-owned result), copied through the exported `memcpy`.
fn tm_at(p: *mut struct_tm) -> struct_tm {
    let mut out = tm_zero();
    strs::memcpy(core::ptr::from_mut(&mut out).cast(), p.cast_const().cast(), core::mem::size_of::<struct_tm>());
    out
}

/// (year, month 1-based, day, hour, minute, second).
fn ymd(t: &struct_tm) -> (i64, i32, i32, i32, i32, i32) {
    (i64::from(t.tm_year) + 1900, t.tm_mon + 1, t.tm_mday, t.tm_hour, t.tm_min, t.tm_sec)
}

fn setenv_var(name: &str, v: Option<&str>) {
    let n = CString::new(name).unwrap();
    match v {
        Some(v) => {
            let v = CString::new(v).unwrap();
            assert_eq!(envx::setenv(n.as_ptr(), v.as_ptr(), 1), 0);
        }
        None => assert_eq!(envx::unsetenv(n.as_ptr()), 0),
    }
}

/// Set `TZ` (None: unset) and call `tzset`.
fn zone(tz: Option<&str>) {
    setenv_var("TZ", tz);
    x::tzset();
}

fn gm(t: i64) -> Option<struct_tm> {
    let mut o = tm_zero();
    (!x::gmtime_r(Some(&t), Some(&mut o)).is_null()).then_some(o)
}

fn local(t: i64) -> struct_tm {
    let mut o = tm_zero();
    assert!(!x::localtime_r(Some(&t), Some(&mut o)).is_null(), "localtime_r({t})");
    o
}

/// tzname[0], tzname[1], timezone, daylight.
fn globals() -> (String, String, i64, i32) {
    let g = liella_boundary::data::tz_globals();
    let name = |i: usize| text_at(g.tzname.get(i).unwrap().load(Ordering::SeqCst));
    (name(0), name(1), g.timezone.load(Ordering::SeqCst), g.daylight.load(Ordering::SeqCst))
}

fn mk(tm: &mut struct_tm) -> (i64, i32) {
    errno::set_raw(0);
    let r = x::mktime(Some(tm));
    (r, errno::get())
}

/// A private directory for zone files, removed when dropped.
fn private_dir(tag: &str) -> scratch::Scratch {
    scratch::Scratch::dir(&format!("s23-{tag}"))
}

/// Copy the system zone `name` to `to`, replacing it atomically (a new file, renamed over).
fn install_zone(name: &str, to: &std::path::Path) {
    let tmp = to.with_extension("new");
    std::fs::copy(std::path::Path::new("/usr/share/zoneinfo").join(name), &tmp).unwrap();
    std::fs::rename(&tmp, to).unwrap();
}

const T2024: i64 = 1_704_067_200; // 2024-01-01T00:00:00Z
const NY_SPRING: i64 = 1_710_054_000; // 2024-03-10 02:00 EST -> 03:00 EDT
const NY_FALL: i64 = 1_730_613_600; // 2024-11-03 02:00 EDT -> 01:00 EST

#[test]
fn s23_5_c_0() {
    // time_t counts seconds since the epoch, without leap seconds: every day is 86400 s.
    let e = gm(0).unwrap();
    assert_eq!((ymd(&e), e.tm_wday, e.tm_yday, e.tm_isdst, e.tm_gmtoff), ((1970, 1, 1, 0, 0, 0), 4, 0, 0, 0));
    assert_eq!(text_at(e.tm_zone), "GMT", "tm_zone");
    let t = gm(951_782_400).unwrap(); // 2000-02-29
    assert_eq!((ymd(&t), t.tm_wday, t.tm_yday), ((2000, 2, 29, 0, 0, 0), 2, 59), "month 0-11, year - 1900, weekday, day of year");
    assert_eq!((t.tm_mon, t.tm_year), (1, 100));
    let day = 86_400i64;
    for d in [-800_000i64, -1, 0, 1, 10_957, 19_723, 800_000] {
        let a = gm(d * day).unwrap();
        let b = gm(d * day + day - 1).unwrap();
        assert_eq!((a.tm_hour, a.tm_min, a.tm_sec), (0, 0, 0));
        assert_eq!((b.tm_hour, b.tm_min, b.tm_sec), (23, 59, 59), "86400 seconds a day");
    }
    // Second 60 (a leap second in the fields) is accepted as input and normalised.
    let mut s = tm_of(2016, 12, 31, 23, 59, 60, 0);
    assert_eq!(x::timegm(Some(&mut s)), 1_483_228_800);
    assert_eq!(ymd(&s), (2017, 1, 1, 0, 0, 0));
}

#[test]
fn s23_5_c_1() {
    let t = 1_720_000_000i64; // 2024-07-03T09:46:40Z
    assert_eq!(ymd(&gm(t).unwrap()), (2024, 7, 3, 9, 46, 40), "gmtime: UTC");
    alone("s23_5_c_1", || {
        zone(Some("JST-9"));
        let l = local(t);
        assert_eq!((ymd(&l), l.tm_gmtoff, l.tm_isdst), ((2024, 7, 3, 18, 46, 40), 32_400, 0));
        assert_eq!(text_at(l.tm_zone), "JST");
        zone(Some("America/New_York"));
        let l = local(t);
        assert_eq!((ymd(&l), l.tm_gmtoff, l.tm_isdst), ((2024, 7, 3, 5, 46, 40), -14_400, 1));
        assert_eq!(text_at(l.tm_zone), "EDT");
        let w = local(T2024);
        assert_eq!((w.tm_gmtoff, w.tm_isdst, text_at(w.tm_zone)), (-18_000, 0, "EST".to_string()));
        // History: before 1883 New York kept local mean time.
        assert_eq!(local(-3_786_825_600).tm_gmtoff, -17_762);
        zone(Some("UTC"));
        let (l, g) = (local(t), gm(t).unwrap());
        assert_eq!((ymd(&l), l.tm_gmtoff), (ymd(&g), 0));
        // localtime (not _r) converts in the zone too.
        zone(Some("Europe/Berlin"));
        let p = x::localtime(Some(&t));
        assert_eq!(ymd(&tm_at(p)), (2024, 7, 3, 11, 46, 40));
        true
    });
}

/// Every local time of `zone` between `from` and `to` (step `step`) and around its transitions
/// goes back to its instant through mktime (with the flag localtime gave, and with -1 except
/// in a repeated hour's second occurrence).
fn round_trips(from: i64, to: i64, step: i64) {
    let mut ts: Vec<i64> = (0..).map(|k| from + k * step).take_while(|&t| t < to).collect();
    // The transitions in the range, found from the offsets, with the seconds around them.
    let mut prev = local(from);
    let mut t = from;
    while t < to {
        let cur = local(t);
        if cur.tm_gmtoff != prev.tm_gmtoff || cur.tm_isdst != prev.tm_isdst {
            let (mut lo, mut hi) = (t - 3600, t);
            while hi - lo > 1 {
                let mid = lo + (hi - lo) / 2;
                if local(mid).tm_gmtoff == prev.tm_gmtoff && local(mid).tm_isdst == prev.tm_isdst { lo = mid } else { hi = mid }
            }
            ts.extend([-3601, -3600, -1800, -1, 0, 1, 1800, 3599, 3600, 3601].map(|d| hi + d));
            prev = cur;
        }
        t += 3600;
    }
    for &t in &ts {
        let l = local(t);
        let mut a = l;
        a.tm_wday = 99;
        a.tm_yday = 999;
        assert_eq!(mk(&mut a), (t, 0), "mktime(localtime({t}))");
        assert_eq!((a.tm_wday, a.tm_yday, a.tm_gmtoff, a.tm_zone), (l.tm_wday, l.tm_yday, l.tm_gmtoff, l.tm_zone));
        let mut b = l;
        b.tm_isdst = -1;
        let (r, e) = mk(&mut b);
        assert_eq!(e, 0);
        assert_eq!(ymd(&b), ymd(&l), "-1 at {t}: the same local time");
        assert!(r == t || (r < t && t - r <= 7200), "-1 at {t}: {r} (the first occurrence)");
    }
}

#[test]
fn s23_5_c_2() {
    alone("s23_5_c_2", || {
        for z in ["America/New_York", "Europe/Berlin", "Australia/Lord_Howe", "Asia/Kolkata", "NZST-12NZDT,M9.5.0,M4.1.0/3", "<-03>3<-02>,M3.5.0/-2,M10.5.0/-1"] {
            zone(Some(z));
            round_trips(T2024, T2024 + 366 * 86_400, 7919);
            round_trips(-500_000_000, -400_000_000, 86_413);
        }
        zone(Some("America/New_York"));
        // Normalisation: day 32 is the next month's first, minute -1 the hour before, month 13
        // the next year's January.
        let mut n = tm_of(2024, 1, 32, 25, 61, 61, -1);
        assert_eq!(mk(&mut n), (T2024 + 2_772_121 + 18_000, 0));
        assert_eq!((ymd(&n), n.tm_wday, n.tm_yday, n.tm_isdst), ((2024, 2, 2, 2, 2, 1), 5, 32, 0));
        let mut n = tm_of(2024, 13, 1, 0, -1, 0, 0);
        mk(&mut n);
        assert_eq!(ymd(&n), (2024, 12, 31, 23, 59, 0));
        // tm_isdst = -1 is resolved; in a skipped hour the time is read with the offset before
        // the change (so it lands after it); in a repeated hour, the first occurrence.
        let mut g = tm_of(2024, 3, 10, 2, 30, 0, -1);
        assert_eq!(mk(&mut g), (NY_SPRING + 1800, 0));
        assert_eq!((ymd(&g), g.tm_isdst), ((2024, 3, 10, 3, 30, 0), 1));
        let mut o = tm_of(2024, 11, 3, 1, 30, 0, -1);
        assert_eq!(mk(&mut o).0, NY_FALL - 1800);
        assert_eq!(o.tm_isdst, 1);
        // An explicit flag picks the occurrence, or the offset the time is read with.
        let mut o = tm_of(2024, 11, 3, 1, 30, 0, 0);
        assert_eq!(mk(&mut o).0, NY_FALL + 1800);
        let mut g = tm_of(2024, 3, 10, 2, 30, 0, 1);
        assert_eq!(mk(&mut g).0, NY_SPRING - 1800);
        let mut w = tm_of(2024, 1, 15, 12, 0, 0, 1);
        mk(&mut w);
        assert_eq!((ymd(&w), w.tm_isdst), ((2024, 1, 15, 11, 0, 0), 0), "DST asked in winter: read as EDT");
        // DST asked of a zone without DST reads one hour ahead (as glibc).
        zone(Some("JST-9"));
        let mut j = tm_of(2024, 7, 1, 12, 0, 0, 1);
        assert_eq!(mk(&mut j).0, 1_719_799_200);
        assert_eq!((j.tm_hour, j.tm_isdst), (11, 0));
        // The seconds are elapsed time after the instant of the other fields: 02:00 (skipped,
        // read as EST) plus 3600 s is 04:00 EDT, while 03:00:00 is 03:00 EDT.
        zone(Some("America/New_York"));
        let mut s = tm_of(2024, 3, 10, 2, 0, 3600, -1);
        assert_eq!(mk(&mut s).0, NY_SPRING + 3600);
        assert_eq!(ymd(&s), (2024, 3, 10, 4, 0, 0));
        let mut s = tm_of(2024, 3, 10, 3, 0, 0, -1);
        assert_eq!(mk(&mut s).0, NY_SPRING);
        let mut s = tm_of(2024, 3, 10, 3, 0, -1, -1);
        assert_eq!((mk(&mut s).0, s.tm_isdst), (NY_SPRING - 1, 0));
        true
    });
}

#[test]
fn s23_5_c_3() {
    for t in (-70_000_000_000i64..70_000_000_000).step_by(987_654_321).chain([0, -1, 2_147_483_647, 2_147_483_648, 253_402_300_799]) {
        let mut g = gm(t).unwrap();
        g.tm_wday = 99;
        assert_eq!(x::timegm(Some(&mut g)), t, "timegm(gmtime({t}))");
        assert_eq!(g.tm_wday, gm(t).unwrap().tm_wday);
    }
    // tm_isdst is ignored and set to 0.
    let mut d = tm_of(2024, 7, 1, 0, 0, 0, 1);
    assert_eq!(x::timegm(Some(&mut d)), 1_719_792_000);
    assert_eq!(d.tm_isdst, 0);
    alone("s23_5_c_3", || {
        zone(Some("Europe/Dublin"));
        for t in [0i64, 1_720_000_000, 1_711_846_800, -1_000_000_000] {
            let mut a = local(t);
            let mut b = a;
            assert_eq!(x::timelocal(Some(&mut a)), t, "timelocal is mktime");
            assert_eq!(x::mktime(Some(&mut b)), t);
        }
        true
    });
}

#[test]
fn s23_5_c_4() {
    assert_eq!(x::difftime(10, 3), 7.0);
    assert_eq!(x::difftime(3, 10), -7.0);
    assert_eq!(x::difftime(0, i64::MAX), -(i64::MAX as f64));
    assert_eq!(x::difftime(i64::MAX, i64::MIN), 2f64.powi(64), "no overflow");
    assert_eq!(x::difftime(9_007_199_254_740_993, 0), 9_007_199_254_740_992.0, "rounded to a double");
}

fn asc(tm: &struct_tm) -> Option<String> {
    let p = x::asctime(Some(tm));
    (!p.is_null()).then(|| text_at(p.expose_provenance()))
}

#[test]
fn s23_5_c_5() {
    let mut t = tm_of(1973, 9, 16, 1, 3, 52, 0);
    t.tm_wday = 0;
    assert_eq!(asc(&t).as_deref(), Some("Sun Sep 16 01:03:52 1973\n"));
    let mut buf = [b'x'; 64];
    let p = x::asctime_r(Some(&t), buf.as_mut_ptr().cast());
    assert_eq!(p.cast::<u8>(), buf.as_mut_ptr());
    assert_eq!(&buf[..25], b"Sun Sep 16 01:03:52 1973\n");
    assert_eq!(buf[25], 0);
    t.tm_mday = 1;
    t.tm_wday = 6;
    assert_eq!(asc(&t).as_deref(), Some("Sat Sep  1 01:03:52 1973\n"), "the day right-aligned in three columns");
    alone("s23_5_c_5", || {
        zone(Some("UTC0"));
        let t = 116_989_432i64; // 1973-09-16T01:03:52Z
        let p = x::ctime(Some(&t));
        assert_eq!(text_at(p.expose_provenance()), "Sun Sep 16 01:03:52 1973\n", "ctime(t) is asctime(localtime(t))");
        let mut b = [0u8; 26];
        assert!(!x::ctime_r(Some(&t), b.as_mut_ptr().cast()).is_null());
        assert_eq!(&b[..], b"Sun Sep 16 01:03:52 1973\n\0");
        zone(Some("JST-9"));
        assert_eq!(text_at(x::ctime(Some(&t)).expose_provenance()), "Sun Sep 16 10:03:52 1973\n");
        true
    });
}

#[test]
fn s23_5_c_6() {
    for (y, n) in [(1900, 365), (2000, 366), (2023, 365), (2024, 366), (0, 366), (-4, 366), (2100, 365), (-1, 365)] {
        assert_eq!(x::dysize(y), n, "dysize({y})");
    }
}

#[test]
fn s23_5_g_0() {
    // The _r forms write to caller storage: two calls, two results.
    let (mut a, mut b) = (tm_zero(), tm_zero());
    let pa = x::gmtime_r(Some(&0), Some(&mut a));
    let pb = x::gmtime_r(Some(&86_400), Some(&mut b));
    assert_eq!((pa, pb), (core::ptr::from_mut(&mut a), core::ptr::from_mut(&mut b)));
    assert_eq!((a.tm_mday, b.tm_mday), (1, 2));
    alone("s23_5_g_0", || {
        zone(Some("JST-9"));
        // gmtime and localtime share one struct tm, overwritten by either.
        let g = x::gmtime(Some(&0));
        assert_eq!(tm_at(g).tm_hour, 0);
        let l = x::localtime(Some(&0));
        assert_eq!(g, l, "one static struct tm");
        assert_eq!(tm_at(g).tm_hour, 9, "overwritten by localtime");
        x::gmtime(Some(&3600));
        assert_eq!(tm_at(l).tm_hour, 1, "overwritten by gmtime");
        // asctime and ctime share one buffer.
        let c = x::ctime(Some(&0));
        let s = x::asctime(Some(&tm_of(2000, 1, 1, 0, 0, 0, 0)));
        assert_eq!(c, s);
        // Per thread (D-23.5): another thread's results do not overwrite this one's.
        let mine = x::gmtime(Some(&0));
        let theirs = std::thread::spawn(|| x::gmtime(Some(&1_000_000_000)).expose_provenance()).join().unwrap();
        assert_ne!(mine.expose_provenance(), theirs);
        assert_eq!(ymd(&tm_at(mine)), (1970, 1, 1, 0, 0, 0));
        true
    });
}

#[test]
fn s23_5_f_1() {
    // The largest and smallest instants whose year fits an int, and one beyond each.
    let hi = 67_768_036_191_676_799i64; // 2147485547-12-31T23:59:59Z: tm_year = INT_MAX
    let lo = -67_768_040_609_740_800i64; // -2147481748-01-01T00:00:00Z: tm_year = INT_MIN
    assert_eq!(gm(hi).unwrap().tm_year, i32::MAX);
    assert_eq!(gm(lo).unwrap().tm_year, i32::MIN);
    for t in [hi + 1, lo - 1, i64::MAX, i64::MIN] {
        let mut o = tm_of(1, 1, 1, 1, 1, 1, 0);
        errno::set_raw(0);
        assert!(x::gmtime_r(Some(&t), Some(&mut o)).is_null(), "gmtime_r({t})");
        assert_eq!(errno::get(), EOVERFLOW);
        assert_eq!(o.tm_year, -1899, "the caller's struct is left alone");
        errno::set_raw(0);
        assert!(x::gmtime(Some(&t)).is_null());
        assert_eq!(errno::get(), EOVERFLOW);
    }
    // timegm: the normalised year must fit; on failure the struct is left as given.
    let mut m = tm_of(0, 13, 1, 0, 0, 0, 0);
    m.tm_year = i32::MAX;
    errno::set_raw(0);
    assert_eq!((x::timegm(Some(&mut m)), errno::get()), (-1, EOVERFLOW));
    assert_eq!((m.tm_mon, m.tm_wday, m.tm_yday), (12, 99, 999));
    let mut m = tm_of(0, 1, 1, 0, 0, 0, 0);
    m.tm_year = i32::MAX;
    assert_eq!(x::timegm(Some(&mut m)), 67_768_036_160_140_800);
    alone("s23_5_f_1", || {
        zone(Some("America/New_York"));
        let mut o = tm_zero();
        errno::set_raw(0);
        assert!(x::localtime_r(Some(&i64::MAX), Some(&mut o)).is_null());
        assert_eq!(errno::get(), EOVERFLOW);
        let mut m = tm_of(0, 13, 1, 0, 0, 0, 0);
        m.tm_year = i32::MAX;
        assert_eq!(mk(&mut m), (-1, EOVERFLOW));
        assert_eq!(m.tm_mon, 12, "left alone");
        let mut m = tm_of(0, 1, 1, 0, 0, -1, 0);
        m.tm_year = i32::MIN;
        assert_eq!(mk(&mut m), (-1, EOVERFLOW), "the year before INT_MIN");
        true
    });
}

#[test]
fn s23_5_f_2() {
    alone("s23_5_f_2", || {
        zone(Some("UTC0"));
        // -1 is also a valid result: only errno tells them apart.
        let mut v = tm_of(1969, 12, 31, 23, 59, 59, -1);
        assert_eq!(mk(&mut v), (-1, 0), "1969-12-31 23:59:59 UTC is -1, without errno");
        let mut e = tm_of(0, 13, 1, 0, 0, 0, -1);
        e.tm_year = i32::MAX;
        assert_eq!(mk(&mut e), (-1, EOVERFLOW), "an error is -1 with errno");
        errno::set_raw(0);
        assert_eq!((x::mktime(None), errno::get()), (-1, EFAULT), "NULL (S6)");
        true
    });
}

#[test]
fn s23_5_f_3() {
    // The fixed 26-byte layout: a year that fits (here -999 to 9999) is printed; one that does
    // not is NULL with EOVERFLOW (Liella decides by the layout; glibc agrees for asctime_r).
    let mut t = tm_of(1973, 9, 16, 1, 3, 52, 0);
    t.tm_wday = 0;
    for (y, want) in [(1000, Some("Sun Sep 16 01:03:52 1000\n")), (9999, Some("Sun Sep 16 01:03:52 9999\n")), (999, Some("Sun Sep 16 01:03:52 999\n")), (0, Some("Sun Sep 16 01:03:52 0\n")), (-999, Some("Sun Sep 16 01:03:52 -999\n")), (10_000, None), (-1000, None), (2_147_481_747, None)] {
        t.tm_year = y - 1900;
        errno::set_raw(0);
        assert_eq!(asc(&t).as_deref(), want, "year {y}");
        assert_eq!(errno::get(), if want.is_some() { 0 } else { EOVERFLOW });
        let mut buf = [b'x'; 32];
        let p = x::asctime_r(Some(&t), buf.as_mut_ptr().cast());
        assert_eq!(p.is_null(), want.is_none());
        if want.is_none() {
            assert_eq!(buf, [b'x'; 32], "nothing written");
        }
    }
    // Fields that widen the text past 26 bytes fail the same way; names out of range are ???.
    t.tm_year = 73;
    let mut w = t;
    w.tm_mday = 123;
    w.tm_hour = 100;
    errno::set_raw(0);
    assert_eq!((asc(&w), errno::get()), (None, EOVERFLOW));
    w.tm_hour = 10;
    assert_eq!(asc(&w).as_deref(), Some("Sun Sep123 10:03:52 1973\n"));
    let mut n = t;
    n.tm_wday = 7;
    n.tm_mon = 12;
    assert_eq!(asc(&n).as_deref(), Some("??? ??? 16 01:03:52 1973\n"));
}

#[test]
fn s23_6_c_0() {
    alone("s23_6_c_0", || {
        zone(Some("EST5EDT,M3.2.0,M11.1.0"));
        assert_eq!(globals(), ("EST".into(), "EDT".into(), 18_000, 1));
        zone(Some("JST-9"));
        assert_eq!(globals(), ("JST".into(), "JST".into(), -32_400, 0));
        zone(Some("Europe/Berlin"));
        assert_eq!(globals(), ("CET".into(), "CEST".into(), -3600, 1));
        assert_eq!(local(T2024).tm_gmtoff, 3600);
        true
    });
}

/// The instants in 2024 at which the offset or flag changes.
fn changes_2024() -> Vec<i64> {
    let mut out = Vec::new();
    let mut prev = local(T2024);
    let mut t = T2024 + 900;
    while t <= T2024 + 366 * 86_400 {
        let cur = local(t);
        if (cur.tm_gmtoff, cur.tm_isdst) != (prev.tm_gmtoff, prev.tm_isdst) {
            let (mut lo, mut hi) = (t - 900, t);
            while hi - lo > 1 {
                let mid = lo + (hi - lo) / 2;
                if (local(mid).tm_gmtoff, local(mid).tm_isdst) == (prev.tm_gmtoff, prev.tm_isdst) { lo = mid } else { hi = mid }
            }
            out.push(hi);
            prev = cur;
        }
        t += 900;
    }
    out
}

#[test]
fn s23_6_c_1() {
    alone("s23_6_c_1", || {
        // Mm.w.d dates, default times 02:00, the DST offset one hour east by default.
        zone(Some("EST5EDT,M3.2.0,M11.1.0"));
        assert_eq!(changes_2024(), [NY_SPRING, NY_FALL]);
        assert_eq!(local(NY_SPRING).tm_gmtoff, -14_400);
        for (tz, want) in [
            ("AAA3BBB,J60/2,J300/2", [1_709_269_200, 1_730_001_600]), // Jn: no February 29
            ("AAA3BBB,59/2,299/2", [1_709_182_800, 1_729_915_200]),   // n: February 29 counted
            ("AAA3BBB,M3.2.0/-1,M11.1.0/26", [1_710_036_000, 1_730_692_800]), // times beyond the day
            ("AAA3BBB,M3.2.0/167,M11.1.0/-167", [1_710_640_800, 1_729_998_000]),
            ("NZST-12NZDT,M9.5.0,M4.1.0/3", [1_712_412_000, 1_727_532_000]), // southern hemisphere
            ("AAA3BBB,M3.2.0", [1_710_046_800, 1_730_606_400]),               // the default end
            ("AAA3BBB,M13.1.0,M11.1.0", [1_710_046_800, 1_730_606_400]),     // out of range: the default
            ("AAA3BBB,J0/2,M11.1.0", [1_710_046_800, 1_730_606_400]),
        ] {
            zone(Some(tz));
            assert_eq!(changes_2024(), want, "{tz}");
        }
        zone(Some("AAA3BBB-1:30,M3.2.0,M11.1.0"));
        assert_eq!(local(1_720_000_000).tm_gmtoff, 5400, "an explicit DST offset");
        zone(Some("<+0330>-3:30"));
        let l = local(0);
        assert_eq!((l.tm_gmtoff, text_at(l.tm_zone)), (12_600, "+0330".to_string()), "a quoted name");
        assert_eq!(globals(), ("+0330".into(), "+0330".into(), -12_600, 0));
        zone(Some("AAA24:59:59"));
        assert_eq!(local(0).tm_gmtoff, -89_999);
        // Rules apply in every year (glibc applies them only from 1970).
        zone(Some("EST5EDT,M3.2.0,M11.1.0"));
        assert_eq!(local(-614_347_200).tm_isdst, 1, "July 1950");
        // Year-round DST.
        zone(Some("AAA3BBB,J1/0,J365/25"));
        assert!(changes_2024().is_empty() && local(T2024).tm_isdst == 1);
        // Not a rule: UTC, named by the value's leading valid name.
        for (tz, name) in [("AAA", "AAA"), ("Invalid//Zone", "Invalid"), ("AB1", ""), ("<AB>1", ""), ("AAA25", "AAA"), ("", "Universal")] {
            zone(Some(tz));
            let l = local(T2024);
            assert_eq!((l.tm_gmtoff, l.tm_isdst, text_at(l.tm_zone)), (0, 0, name.to_string()), "TZ={tz:?}");
            assert_eq!(globals(), (name.to_string(), String::new(), 0, 0), "TZ={tz:?}");
        }
        true
    });
}

#[test]
fn s23_6_c_2() {
    alone("s23_6_c_2", || {
        for tz in ["Asia/Tokyo", ":Asia/Tokyo", "/usr/share/zoneinfo/Asia/Tokyo", ":/usr/share/zoneinfo/Asia/Tokyo"] {
            zone(Some(tz));
            let l = local(-680_000_000); // 1948: Japan kept DST then
            assert_eq!((l.tm_gmtoff, text_at(l.tm_zone)), (36_000, "JDT".to_string()), "TZ={tz}");
            assert_eq!(globals(), ("JST".into(), "JDT".into(), -32_400, 1));
        }
        // TZDIR replaces the zone directory.
        let dir = private_dir("c_2");
        std::fs::create_dir_all(dir.join("Foo")).unwrap();
        install_zone("Asia/Kolkata", &dir.join("Foo/Bar"));
        setenv_var("TZDIR", Some(dir.to_str().unwrap()));
        zone(Some("Foo/Bar"));
        assert_eq!(local(T2024).tm_gmtoff, 19_800);
        zone(Some("Europe/Berlin"));
        assert_eq!((local(T2024).tm_gmtoff, globals().0), (0, "Europe".to_string()), "not under TZDIR: not a zone");
        setenv_var("TZDIR", None);
        // A file that is not TZif is not a zone.
        std::fs::write(dir.join("junk"), b"not a zone").unwrap();
        zone(Some(dir.join("junk").to_str().unwrap()));
        assert_eq!(globals(), (String::new(), String::new(), 0, 0));
        true
    });
}

#[test]
fn s23_6_c_3() {
    alone("s23_6_c_3", || {
        // TZ unset: /etc/localtime.
        let sys = liella::time::Zone::from_tzif(&std::fs::read("/etc/localtime").unwrap()).unwrap();
        zone(None);
        for t in [0i64, T2024, 1_720_000_000] {
            let (l, s) = (local(t), sys.at(t).unwrap());
            assert_eq!((l.tm_gmtoff, l.tm_isdst, text_at(l.tm_zone)), (s.offset, i32::from(s.isdst), String::from_utf8(sys.name(s.name).to_vec()).unwrap()));
        }
        // timezone is west of UTC, tm_gmtoff east; daylight says whether the zone has DST.
        zone(Some("America/New_York"));
        let (_, _, tz, dl) = globals();
        assert_eq!((tz, local(T2024).tm_gmtoff, dl), (18_000, -18_000, 1));
        zone(Some("Pacific/Kiritimati"));
        assert_eq!(globals(), ("+14".into(), "+14".into(), -50_400, 0));
        true
    });
}

#[test]
fn s23_6_g_0() {
    alone("s23_6_g_0", || {
        let t = 1_720_000_000i64;
        zone(Some("UTC0"));
        // localtime and mktime see a TZ changed without tzset; localtime_r keeps the zone
        // last selected (strftime is §23.7's).
        setenv_var("TZ", Some("JST-9"));
        assert_eq!(local(t).tm_gmtoff, 0, "localtime_r: no implicit tzset");
        assert_eq!(globals().0, "UTC");
        assert_eq!(tm_at(x::localtime(Some(&t))).tm_gmtoff, 32_400, "localtime: implicit tzset");
        assert_eq!(globals(), ("JST".into(), "JST".into(), -32_400, 0), "the globals follow");
        setenv_var("TZ", Some("EST5EDT,M3.2.0,M11.1.0"));
        let mut m = tm_of(2024, 7, 3, 12, 0, 0, -1);
        assert_eq!(mk(&mut m).0, 1_720_022_400, "mktime: implicit tzset");
        assert_eq!(text_at(m.tm_zone), "EDT");
        // A file TZ names is read again when it changes: at once at an explicit tzset, and by an
        // implicit one at most once per second (D-23.6; owner ruling 2026-10-05).
        let dir = private_dir("g_0");
        let file = dir.join("zone");
        install_zone("Asia/Tokyo", &file);
        setenv_var("TZ", Some(file.to_str().unwrap()));
        let loaded = std::time::Instant::now();
        let a = tm_at(x::localtime(Some(&t)));
        assert_eq!((a.tm_gmtoff, text_at(a.tm_zone)), (32_400, "JST".to_string()));
        let again = tm_at(x::localtime(Some(&t)));
        assert_eq!(again.tm_zone, a.tm_zone, "unchanged: the same zone");
        install_zone("Europe/Berlin", &file);
        assert_eq!(local(t).tm_gmtoff, 32_400, "localtime_r does not check");
        let within = tm_at(x::localtime(Some(&t))).tm_gmtoff;
        if loaded.elapsed() < std::time::Duration::from_millis(900) {
            assert_eq!(within, 32_400, "an implicit tzset checks the file at most once per second");
        }
        std::thread::sleep(std::time::Duration::from_millis(1100));
        assert_eq!(tm_at(x::localtime(Some(&t))).tm_gmtoff, 7200, "a second later an implicit tzset sees the new file");
        install_zone("Asia/Tokyo", &file);
        x::tzset();
        assert_eq!(tm_at(x::localtime(Some(&t))).tm_gmtoff, 32_400, "an explicit tzset checks at once");
        install_zone("Europe/Berlin", &file);
        x::tzset();
        let b = tm_at(x::localtime(Some(&t)));
        assert_eq!((b.tm_gmtoff, text_at(b.tm_zone)), (7200, "CEST".to_string()), "replaced: read again");
        assert_eq!(text_at(a.tm_zone), "JST", "the old tm_zone stays valid");
        assert_eq!(globals().1, "CEST");
        // The same mechanism for an unset TZ, with a private default file.
        let def = dir.join("localtime");
        install_zone("Asia/Tokyo", &def);
        let mut path = def.to_str().unwrap().as_bytes().to_vec();
        path.push(0);
        let first = liella::time::load(&CSys, None, None, &path).unwrap();
        let (_, id) = first.file.as_ref().unwrap();
        assert_eq!(first.zone.as_ref().unwrap().at(t).unwrap().offset, 32_400);
        assert_eq!(liella::time::file_id(&CSys, &path), *id, "unchanged");
        install_zone("Europe/Berlin", &def);
        assert_ne!(liella::time::file_id(&CSys, &path), *id, "replaced: seen by stat");
        let second = liella::time::load(&CSys, None, None, &path).unwrap();
        assert_eq!(second.zone.as_ref().unwrap().at(t).unwrap().offset, 7200);
        true
    });
}
// ------------------------------------------------------------------------------ §23.7
//
// Formatting and parsing time through the exports. The global locale of the test process is
// "C"; named locales are compiled with the host's `localedef` into `target/test-locales` and
// opened with `newlocale` in a re-executed child (`LOCPATH` is process-wide), as are the
// clauses that depend on `TZ`. Without `localedef` the locale-dependent parts say so and pass.

use liella_boundary::posix::s07_locale as lx;

const LC_ALL_MASK: i32 = 0x1FBF;

static UTC_ZONE: &[u8] = b"UTC\0";

/// A `struct tm` with every field given (month 1-based), `tm_zone` "UTC".
#[allow(clippy::too_many_arguments)]
fn tmz(y: i32, mo: i32, d: i32, h: i32, mi: i32, s: i32, wday: i32, yday: i32) -> struct_tm {
    struct_tm {
        tm_sec: s,
        tm_min: mi,
        tm_hour: h,
        tm_mday: d,
        tm_mon: mo - 1,
        tm_year: y - 1900,
        tm_wday: wday,
        tm_yday: yday,
        tm_isdst: 0,
        tm_gmtoff: 0,
        tm_zone: UTC_ZONE.as_ptr().expose_provenance(),
    }
}

/// `strftime` into a 256-byte buffer: (result, returned length).
fn ftime_n(fmt: &str, t: &struct_tm, l: Option<*mut c_void>) -> (String, usize) {
    let f = CString::new(fmt).unwrap();
    let mut buf = [0x7fu8; 256];
    let n = match l {
        None => x::strftime(buf.as_mut_ptr().cast(), buf.len(), f.as_ptr(), Some(t)),
        Some(l) => x::strftime_l(buf.as_mut_ptr().cast(), buf.len(), f.as_ptr(), Some(t), l),
    };
    if n > 0 {
        assert_eq!(buf[n], 0, "{fmt}: terminated");
    }
    (String::from_utf8(buf[..n].to_vec()).unwrap(), n)
}

fn ftime(fmt: &str, t: &struct_tm) -> String {
    ftime_n(fmt, t, None).0
}

fn ftime_l(fmt: &str, t: &struct_tm, l: *mut c_void) -> String {
    ftime_n(fmt, t, Some(l)).0
}

/// `strptime` (in locale `l` if given) on a `struct tm` of sentinels (-77): the bytes consumed
/// and the structure.
fn ptime(input: &str, fmt: &str, l: Option<*mut c_void>) -> Option<(usize, struct_tm)> {
    let (s, f) = (CString::new(input).unwrap(), CString::new(fmt).unwrap());
    let mut t = struct_tm { tm_sec: -77, tm_min: -77, tm_hour: -77, tm_mday: -77, tm_mon: -77, tm_year: -77, tm_wday: -77, tm_yday: -77, tm_isdst: -77, tm_gmtoff: -77, tm_zone: 0 };
    let end = match l {
        None => x::strptime(s.as_ptr(), f.as_ptr(), Some(&mut t)),
        Some(l) => x::strptime_l(s.as_ptr(), f.as_ptr(), Some(&mut t), l),
    };
    (!end.is_null()).then(|| (end.expose_provenance() - s.as_ptr().expose_provenance(), t))
}

/// (year, month 1-based, day, hour, minute, second) of a parse, -77 kept for fields not set.
fn parsed(t: &struct_tm) -> (i32, i32, i32, i32, i32, i32) {
    let keep = |v: i32, off: i32| if v == -77 { -77 } else { v + off };
    (keep(t.tm_year, 1900), keep(t.tm_mon, 1), t.tm_mday, t.tm_hour, t.tm_min, t.tm_sec)
}

/// The reference locales compiled into `target/test-locales` (the directory, for `LOCPATH`), or
/// `None` without `localedef`.
fn time_locales() -> Option<std::path::PathBuf> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-locales");
    for name in ["de_DE.UTF-8", "ja_JP.UTF-8", "th_TH.UTF-8", "fa_IR.UTF-8"] {
        let (lang, charset) = name.split_once('.').unwrap();
        if !oracle::locale(&dir.join(name), lang, charset, "LC_TIME") {
            return None;
        }
    }
    Some(dir)
}

/// In a child: point `LOCPATH` at the reference locales and open `name`.
fn open_locale(dir: &std::path::Path, name: &str) -> *mut c_void {
    setenv_var("LOCPATH", Some(dir.to_str().unwrap()));
    let n = CString::new(name).unwrap();
    let l = lx::newlocale(LC_ALL_MASK, n.as_ptr(), core::ptr::null_mut());
    assert!(!l.is_null(), "newlocale({name})");
    l
}

#[test]
fn s23_7_c_1() {
    // A directive language: ordinary text is copied, directives are replaced; the result is
    // terminated and its length returned.
    let t = tmz(2024, 7, 4, 13, 5, 9, 4, 185);
    let (s, n) = ftime_n("Date: %Y-%m-%d, time: %H:%M:%S.", &t, None);
    assert_eq!((s.as_str(), n), ("Date: 2024-07-04, time: 13:05:09.", 33));
    assert_eq!(ftime("no directives", &t), "no directives");
    assert_eq!(ftime("100%%", &t), "100%");
    // The fields are read as given: no normalisation, names out of range are "?".
    let odd = struct_tm { tm_mday: 40, tm_mon: 13, tm_wday: 9, ..t };
    assert_eq!(ftime("%d %a %b", &odd), "40 ? ?");
    // What is not a directive is copied, with its width.
    assert_eq!(ftime("%Q|%Ea|%OY|%+Y|%5", &t), "%Q|%Ea|%OY|%+Y|   %5");
}

#[test]
fn s23_7_c_1_1() {
    let t = tmz(2024, 7, 4, 13, 5, 9, 4, 185);
    let want = [
        ("%a", "Thu"), ("%A", "Thursday"), ("%b", "Jul"), ("%B", "July"), ("%c", "Thu Jul  4 13:05:09 2024"), ("%d", "04"), ("%H", "13"), ("%I", "01"),
        ("%j", "186"), ("%m", "07"), ("%M", "05"), ("%p", "PM"), ("%S", "09"), ("%U", "26"), ("%w", "4"), ("%W", "27"), ("%x", "07/04/24"),
        ("%X", "13:05:09"), ("%y", "24"), ("%Y", "2024"), ("%Z", "UTC"), ("%%", "%"),
    ];
    for (f, w) in want {
        assert_eq!(ftime(f, &t), w, "{f}");
    }
    // Week numbers: days before the first Sunday (%U) or Monday (%W) are week 0.
    let jan1 = tmz(2023, 1, 1, 0, 0, 0, 0, 0); // a Sunday
    assert_eq!(ftime("%U %W %I %p", &jan1), "01 00 12 AM");
    let leap = tmz(2016, 12, 31, 23, 59, 60, 6, 365); // a leap second
    assert_eq!(ftime("%j %S %U %W", &leap), "366 60 52 52");
}

#[test]
fn s23_7_c_1_2() {
    let t = tmz(2024, 7, 4, 7, 5, 9, 4, 185);
    let want = [
        ("%C", "20"), ("%D", "07/04/24"), ("%e", " 4"), ("%F", "2024-07-04"), ("%g", "24"), ("%G", "2024"), ("%h", "Jul"), ("%n", "\n"),
        ("%r", "07:05:09 AM"), ("%R", "07:05"), ("%t", "\t"), ("%T", "07:05:09"), ("%u", "4"), ("%V", "27"), ("%z", "+0000"),
    ];
    for (f, w) in want {
        assert_eq!(ftime(f, &t), w, "{f}");
    }
    // ISO 8601 weeks: 2021-01-03 is in week 53 of 2020, 2024-12-30 in week 1 of 2025.
    assert_eq!(ftime("%G-W%V-%u %g", &tmz(2021, 1, 3, 0, 0, 0, 0, 2)), "2020-W53-7 20");
    assert_eq!(ftime("%G-W%V-%u", &tmz(2024, 12, 30, 0, 0, 0, 1, 364)), "2025-W01-1");
    // %C is the year divided by 100 with at least two digits; negative years floor.
    assert_eq!(ftime("%C %y %Y", &tmz(512, 1, 1, 0, 0, 0, 0, 0)), "05 12 512");
    assert_eq!(ftime("%C %y %Y", &tmz(-1, 1, 1, 0, 0, 0, 0, 0)), "-1 99 -1");
    // %z: tm_gmtoff as +hhmm; nothing when tm_isdst says no zone is known.
    let mut z = tmz(2024, 1, 1, 0, 0, 0, 1, 0);
    z.tm_gmtoff = 19_800;
    assert_eq!(ftime("%z", &z), "+0530");
    z.tm_gmtoff = -12_600;
    assert_eq!(ftime("%z", &z), "-0330");
    z.tm_isdst = -1;
    assert_eq!(ftime("[%z]", &z), "[]");
    // strptime reads the same directives back.
    let (n, p) = ptime("07/04/24 07:05:09", "%D %T", None).unwrap();
    assert_eq!((n, parsed(&p), p.tm_wday, p.tm_yday), (17, (2024, 7, 4, 7, 5, 9), 4, 185));
    let (_, p) = ptime("2024-07-04 +0530", "%F %z", None).unwrap();
    assert_eq!((parsed(&p).0, p.tm_gmtoff), (2024, 19_800));
}

#[test]
fn s23_7_c_1_3() {
    // Without eras or alternative digits (the C locale), E and O are the unmodified conversions;
    // other combinations are not directives.
    let t = tmz(2024, 1, 1, 0, 0, 0, 1, 0);
    assert_eq!(ftime("%Ec|%EC|%Ex|%EX|%Ey|%EY", &t), "Mon Jan  1 00:00:00 2024|20|01/01/24|00:00:00|24|2024");
    assert_eq!(ftime("%Od|%Oe|%OH|%OI|%Om|%OM|%OS|%Ou|%OU|%OV|%Ow|%OW|%Oy", &t), "01| 1|00|12|01|00|00|1|00|01|1|01|24");
    assert_eq!(ftime("%Ea|%Od|%EE|%OY", &t), "%Ea|01|%EE|%OY");
    assert_eq!(parsed(&ptime("24", "%Ey", None).unwrap().1).0, 2024, "strptime: as %y");
    assert_eq!(parsed(&ptime("07:08", "%OH:%OM", None).unwrap().1), (-77, -77, -77, 7, 8, -77));
    let Some(dir) = time_locales() else { return skip_note::skipped("skipped: no localedef") };
    alone("s23_7_c_1_3", || {
        // Eras (ja_JP: imperial eras, the first year 元年, years before AD 1 counted back).
        let ja = open_locale(&dir, "ja_JP.UTF-8");
        assert_eq!(ftime_l("%EC|%Ey|%EY", &t, ja), "令和|06|令和06年");
        assert_eq!(ftime_l("%Ec", &t, ja), "令和06年01月01日 00時00分00秒");
        assert_eq!(ftime_l("%-EY|%10Ey", &t, ja), "令和6年|0000000006", "flags reach the era's format");
        assert_eq!(ftime_l("%EY", &tmz(1873, 1, 1, 0, 0, 0, 3, 0), ja), "明治06年");
        assert_eq!(ftime_l("%EY", &tmz(1912, 7, 30, 0, 0, 0, 2, 211), ja), "大正元年");
        assert_eq!(ftime_l("%EY", &tmz(0, 3, 1, 0, 0, 0, 3, 60), ja), "紀元前01年");
        assert_eq!(ftime_l("%EY", &tmz(-1, 12, 31, 0, 0, 0, 5, 364), ja), "紀元前02年");
        // Alternative digits (kanji), unpadded unless a width asks.
        assert_eq!(ftime_l("%Od|%Oy|%OI|%OH|%5Od", &t, ja), "一|二十四|十二|〇|  一");
        // strptime reads them back.
        let (n, p) = ptime("令和06年01月01日", "%Ex", Some(ja)).unwrap();
        assert_eq!((n, parsed(&p).0, p.tm_wday), ("令和06年01月01日".len(), 2024, 1));
        assert_eq!(parsed(&ptime("紀元前02年", "%EY", Some(ja)).unwrap().1).0, -1);
        assert_eq!(parsed(&ptime("三十一 十二", "%Od %Om", Some(ja)).unwrap().1), (-77, 12, 31, -77, -77, -77));
        // The Buddhist era (th_TH) and Persian digits (fa_IR).
        let th = open_locale(&dir, "th_TH.UTF-8");
        assert_eq!(ftime_l("%EY|%Ey", &t, th), "พ.ศ. 2567|2567");
        assert_eq!(parsed(&ptime("พ.ศ. 2567", "%EY", Some(th)).unwrap().1).0, 2024);
        let fa = open_locale(&dir, "fa_IR.UTF-8");
        assert_eq!(ftime_l("%Od|%x", &t, fa), "۰۱|۲۴/۰۱/۰۱");
        true
    });
}

#[test]
fn s23_7_c_1_4() {
    let t = tmz(2024, 1, 5, 19, 0, 0, 5, 4);
    assert_eq!(ftime("%k|%l|%P|%p", &t), "19| 7|pm|PM");
    assert_eq!(ftime("%k|%l", &tmz(2024, 1, 5, 7, 0, 0, 5, 4)), " 7| 7");
    // Flags: _ spaces, - none, 0 zeros, ^ upper case, # swapped case; a width pads.
    assert_eq!(ftime("%_d|%-d|%0e|%^a|%#A|%#Z|%#p|%10Y|%-10Y|%_3j|%04k|%^10B", &t), " 5|5|05|FRI|FRIDAY|utc|pm|0000002024|      2024|  5|0019|   JANUARY");
    // Negative years keep their sign before zeros and after spaces.
    assert_eq!(ftime("%6Y|%_6Y", &tmz(-5, 1, 1, 0, 0, 0, 0, 0)), "-00005|    -5");
    // 0 when the result and its terminator do not fit; the length when they do.
    let f = CString::new("%Y-%m-%d").unwrap();
    let mut buf = [b'x'; 16];
    assert_eq!(x::strftime(buf.as_mut_ptr().cast(), 11, f.as_ptr(), Some(&t)), 10);
    assert_eq!(&buf[..11], b"2024-01-05\0");
    assert_eq!(x::strftime(buf.as_mut_ptr().cast(), 10, f.as_ptr(), Some(&t)), 0);
    assert_eq!(x::strftime(buf.as_mut_ptr().cast(), 0, f.as_ptr(), Some(&t)), 0);
    let huge = CString::new("%9999999999d").unwrap();
    assert_eq!(x::strftime(buf.as_mut_ptr().cast(), 16, huge.as_ptr(), Some(&t)), 0, "an absurd width does not fit");
    alone("s23_7_c_1_4", || {
        // %s: the seconds since the epoch of the fields read as local time (an implicit tzset).
        let t = tmz(2024, 1, 1, 0, 0, 0, 1, 0);
        zone(Some("UTC0"));
        assert_eq!(ftime("%s", &t), "1704067200");
        setenv_var("TZ", Some("JST-9"));
        assert_eq!(ftime("%s", &t), "1704034800", "TZ changed without tzset");
        let mut n = t;
        n.tm_zone = 0;
        assert_eq!(ftime("%Z", &n), "JST", "a NULL tm_zone names the current zone");
        // Names come from LC_TIME.
        let Some(dir) = time_locales() else { return true };
        let de = open_locale(&dir, "de_DE.UTF-8");
        assert_eq!(ftime_l("%A %d. %B %Y|%a|%b|%c", &t, de), "Montag 01. Januar 2024|Mo|Jan|Mo 01 Jan 2024 00:00:00 UTC");
        let mar = tmz(2024, 3, 5, 0, 0, 0, 2, 64);
        assert_eq!(ftime_l("%#B|%^b", &mar, de), "MÄRZ|MÄR", "case changes per character");
        // An empty result is also 0 (de_DE has no AM/PM strings).
        assert_eq!(ftime_n("%p", &t, Some(de)), (String::new(), 0));
        assert_eq!(ftime_l("%r", &t, de), "12:00:00 ", "%r without T_FMT_AMPM is %I:%M:%S %p");
        let (_, p) = ptime("Dienstag, 5. März 2024", "%A, %d. %B %Y", Some(de)).unwrap();
        assert_eq!((parsed(&p), p.tm_wday, p.tm_yday), ((2024, 3, 5, -77, -77, -77), 2, 64));
        true
    });
}

// ------------------------------------------------------------------------------ clauses after a flag line (2026-10-04)

#[test]
fn s23_1_c_6() {
    // times: the process's and its children's CPU times in clock ticks, and an elapsed value.
    let mut t = liella_kernel::uapi::Tms { utime: -1, stime: -1, cutime: -1, cstime: -1 };
    let a = x::times(Some(&mut t));
    assert!(a > 0 && t.utime >= 0 && t.stime >= 0 && t.cutime >= 0 && t.cstime >= 0);
    // Spin (burning CPU) until `times` itself reports the progress this clause needs: at least 3 ticks (30 ms)
    // of elapsed time and of process CPU time. The tick accounting of the kernel lags the CPU actually used and a
    // loaded machine deschedules the thread, so a fixed spin (50-60 ms) may be too short (HYGIENE-1): poll
    // `times` until both advanced, bounded by a load-scaled deadline (20 s). A `times` whose clock never
    // advances still fails, at the deadline.
    let (began, limit) = (std::time::Instant::now(), timeout::deadline(std::time::Duration::from_secs(20)));
    let mut u = liella_kernel::uapi::Tms { utime: 0, stime: 0, cutime: 0, cstime: 0 };
    let b = loop {
        let spin = std::time::Instant::now();
        while spin.elapsed() < std::time::Duration::from_millis(5) {
            std::hint::spin_loop();
        }
        let b = x::times(Some(&mut u));
        if (b >= a + 3 && u.utime + u.stime >= t.utime + t.stime + 3) || std::time::Instant::now() >= limit {
            break b;
        }
    };
    assert!(b >= a + 3, "elapsed ticks (100 a second): {a} -> {b}; {}", timeout::expired("times elapsed ticks", 20, began.elapsed()));
    assert!(u.utime + u.stime >= t.utime + t.stime + 3, "CPU time in ticks; {}", timeout::expired("times CPU ticks", 20, began.elapsed()));
}

#[test]
fn s23_7_c_2() {
    // strptime fills the fields it parses, computes the weekday and day of the year from a
    // full date, and returns a pointer past the consumed input, or NULL.
    let s = c"2024-07-04 rest";
    let mut t = tm_zero();
    let r = x::strptime(s.as_ptr(), c"%Y-%m-%d".as_ptr(), Some(&mut t));
    assert_eq!(r as usize, s.as_ptr() as usize + 10, "past the date");
    assert_eq!((t.tm_year, t.tm_mon, t.tm_mday, t.tm_wday, t.tm_yday, t.tm_hour), (124, 6, 4, 4, 185, 0));
    assert!(x::strptime(c"July".as_ptr(), c"%d".as_ptr(), Some(&mut t)).is_null(), "a mismatch: NULL");
}

#[test]
fn s23_7_c_3() {
    // getdate tries the templates of the file DATEMSK names; getdate_r returns the error
    // (1: DATEMSK unset or empty).
    let mut t = tm_zero();
    if std::env::var_os("DATEMSK").is_none() {
        assert_eq!(x::getdate_r(c"2024-07-04".as_ptr(), Some(&mut t)), 1);
    }
    let d = scratch::Scratch::dir("c73");
    std::fs::write(d.join("templates"), "%Y-%m-%d\n").unwrap();
    let r = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["s23_7_c_3_child", "--exact", "--test-threads=1", "--nocapture", "--ignored"])
        .env("DATEMSK", d.join("templates"))
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&r.stdout).contains("getdate_r=0 mday=4 mon=6"), "{}", String::from_utf8_lossy(&r.stdout));
}

/// `s23_7_c_3` in a process with DATEMSK set (run by it).
#[test]
#[ignore = "run by s23_7_c_3 with DATEMSK set"]
fn s23_7_c_3_child() {
    let mut t = tm_zero();
    let r = x::getdate_r(c"2024-07-04".as_ptr(), Some(&mut t));
    println!("getdate_r={r} mday={} mon={}", t.tm_mday, t.tm_mon);
}

/// The child's exit status, stdout and stderr (the last 60 lines of each) for a failure message.
fn child_report(o: &std::process::Output) -> String {
    let tail = |b: &[u8]| {
        let s = String::from_utf8_lossy(b);
        let lines: Vec<&str> = s.lines().collect();
        lines[lines.len().saturating_sub(60)..].join("\n")
    };
    format!("exit {:?}\n--- stdout ---\n{}\n--- stderr ---\n{}", o.status, tail(&o.stdout), tail(&o.stderr))
}
