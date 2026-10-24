// Specified in part from spec §23 (phase 5: §23.1–23.3; phase 6: the §23.4 timers; phase 9:
// §23.5 broken-down time, §23.6 zones and §23.7 formatting and parsing).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Translation-layer tests for the exported symbols of spec §23.
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//! Readings are checked against the Rust standard library's clocks; setting a clock needs
//! `CAP_SYS_TIME`, so the unprivileged answer (`EPERM`) is what is checked.
//!
//! Timers: `alarm`, `ualarm` and the itimers are process-wide, so they run as the only thread
//! of a process forked from a re-executed copy of the test; the POSIX timers and `timerfd`s
//! are private to the test that makes them. Descriptors that must be seen closed are private
//! numbers (931, 932).
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(non_snake_case)]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::as_conversions, clippy::indexing_slicing, missing_docs)]
#[path = "../../core/tests/support/skip_note.rs"]
mod skip_note;

#[path = "../../core/tests/support/scratch.rs"]
mod scratch;

use core::ffi::c_void;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use liella_abi::layout::{struct_ntptimeval, struct_timeb, struct_timeval};
use liella_boundary::errno;
use liella_boundary::posix::s23_time as s;
use liella_boundary::selftest::ChildEnd;
use liella_boundary::sys::CSys;
use liella_kernel::fd::Fd;
use liella_kernel::ops::FcntlInt;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Itimerspec, Itimerval, SigEvent, Timespec, Timeval, Timex, Timezone, Tms};
use liella_legacy::clock_compat::NtpTimevalOld;

const EPERM: i32 = 1;
const EFAULT: i32 = 14;
const EINVAL: i32 = 22;
const ESRCH: i32 = 3;

fn now() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64()
}

fn fails(r: isize, e: i32) -> bool {
    r == -1 && errno::get() == e
}

fn root() -> bool {
    liella_kernel::sys::Sys::geteuid(&liella_boundary::sys::CSys) == 0
}

fn ntv() -> struct_ntptimeval {
    struct_ntptimeval {
        time: struct_timeval { tv_sec: 0, tv_usec: 0 },
        maxerror: -1,
        esterror: -1,
        tai: -1,
        __glibc_reserved1: 9,
        __glibc_reserved2: 9,
        __glibc_reserved3: 9,
        __glibc_reserved4: 9,
    }
}

// ------------------------------------------------------------------------------ timers (§23.4)

const EBADF: i32 = 9;
const EOPNOTSUPP: i32 = 95;
const ITIMER_REAL: i32 = 0;
const ITIMER_VIRTUAL: i32 = 1;
const SIGUSR1: i32 = 10;
const SIGEV_SIGNAL: i32 = 0;
const SIGEV_NONE: i32 = 1;
const SIGEV_THREAD_ID: i32 = 4;
const TFD_NONBLOCK: i32 = 0o4000;
const TFD_CLOEXEC: i32 = 0o2000000;

fn tv(us: i64) -> Timeval {
    Timeval { sec: us / 1_000_000, usec: us % 1_000_000 }
}

fn itv(value_us: i64, interval_us: i64) -> Itimerval {
    Itimerval { interval: tv(interval_us), value: tv(value_us) }
}

fn us(t: Timeval) -> i64 {
    t.sec * 1_000_000 + t.usec
}

fn its(value: Timespec, interval: Timespec) -> Itimerspec {
    Itimerspec { interval, value }
}

fn ms(n: i64) -> Timespec {
    Timespec { sec: n / 1000, nsec: n % 1000 * 1_000_000 }
}

fn ns(t: Timespec) -> i64 {
    t.sec * 1_000_000_000 + t.nsec
}

fn tid(t: usize) -> *mut c_void {
    core::ptr::with_exposed_provenance_mut(t)
}

/// `timer_create` through the export: the new `timer_t`, or the `errno` of the failure.
fn create(clock: i32, ev: Option<&SigEvent>) -> Result<usize, i32> {
    let mut t = core::ptr::null_mut();
    errno::set_raw(0);
    if s::timer_create(clock, ev, Some(&mut t)) == 0 { Ok(t.expose_provenance()) } else { Err(errno::get()) }
}

fn none() -> SigEvent {
    SigEvent::new(SIGEV_NONE, 0, 0, 0)
}

extern "C" fn never(_v: usize) {}

/// A `SIGEV_THREAD` event calling [`never`] (its timers are never left armed long enough to
/// expire... and it does nothing if they do).
fn thread_event(attr: usize) -> SigEvent {
    SigEvent::thread(0, never as extern "C" fn(usize) as usize as u64, attr as u64)
}

/// Run `body` as the only thread of a process (forked from a re-executed copy of `test`):
/// `alarm` and the itimers are process-wide. `body` returns 0, or the number of the first
/// check that failed.
fn single(test: &str, body: fn(&CSys) -> i32) {
    const PROBE: &str = "LIELLA_S23_PROBE";
    if std::env::var(PROBE).is_ok_and(|v| v == test) {
        let end = liella_boundary::selftest::in_child(body);
        eprintln!("{test}: {end:?}");
        CSys.exit_group(if end == Ok(ChildEnd::Exited(0)) { 0 } else { 1 });
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact", "--test-threads=1", "--nocapture"])
        .env(PROBE, test)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{test} failed in its child: {}", child_report(&o));
}

#[test]
fn sym_adjtime() {
    let mut old = Timeval { sec: 9, usec: 9 };
    assert_eq!(s::adjtime(None, Some(&mut old)), 0, "NULL delta: only read the pending slew");
    assert!(old.sec.abs() < 3600 && old.usec.abs() < 1_000_000);
    errno::set_raw(0);
    assert!(fails(s::adjtime(Some(&Timeval { sec: 1_000_000, usec: 0 }), None) as isize, EINVAL), "beyond glibc's range");
    if !root() {
        assert!(fails(s::adjtime(Some(&Timeval { sec: 0, usec: 1 }), None) as isize, EPERM));
    }
}

#[test]
fn sym___adjtimex() {
    let mut tx = Timex::default();
    let state = s::__adjtimex(Some(&mut tx));
    assert!((0..=5).contains(&state), "TIME_OK .. TIME_ERROR: {state}");
    assert!(tx.tick > 0 && tx.precision > 0, "the kernel filled the record");
    errno::set_raw(0);
    tx.modes = 0x0001; // ADJ_OFFSET
    if !root() {
        assert!(fails(s::__adjtimex(Some(&mut tx)) as isize, EPERM));
    }
    assert!(fails(s::__adjtimex(None) as isize, EFAULT));
}

#[test]
fn sym_adjtimex() {
    let mut tx = Timex::default();
    let state = s::adjtimex(Some(&mut tx));
    assert!((0..=5).contains(&state), "TIME_OK .. TIME_ERROR: {state}");
    assert!(tx.tick > 0 && tx.precision > 0, "the kernel filled the record");
    errno::set_raw(0);
    tx.modes = 0x0001; // ADJ_OFFSET
    if !root() {
        assert!(fails(s::adjtimex(Some(&mut tx)) as isize, EPERM));
    }
    assert!(fails(s::adjtimex(None) as isize, EFAULT));
}

#[test]
fn sym_alarm() {
    single("sym_alarm", |_| {
        if s::alarm(0) != 0 || s::alarm(5) != 0 {
            return 1;
        }
        // The previous alarm's remaining seconds; the new one replaces it.
        if s::alarm(7) != 5 || s::alarm(0) != 7 || s::alarm(0) != 0 {
            return 2;
        }
        // ITIMER_REAL is the same timer, both ways.
        let mut cur = itv(9, 9);
        s::alarm(3);
        if s::getitimer(ITIMER_REAL, Some(&mut cur)) != 0 || us(cur.interval) != 0 || !(2_000_000..=3_000_000).contains(&us(cur.value)) {
            return 3;
        }
        s::setitimer(ITIMER_REAL, Some(&itv(4_000_000, 250_000)), None);
        if s::alarm(0) != 4 || s::getitimer(ITIMER_REAL, Some(&mut cur)) != 0 || cur != itv(0, 0) {
            return 4;
        }
        // The remainder is rounded to the nearest second (and a partial second is never 0).
        s::setitimer(ITIMER_REAL, Some(&itv(1_600_000, 0)), None);
        if s::alarm(0) != 2 {
            return 5;
        }
        s::setitimer(ITIMER_REAL, Some(&itv(300_000, 0)), None);
        if s::alarm(0) != 1 {
            return 6;
        }
        // It cannot fail: the largest request is taken, errno untouched.
        errno::set_raw(0);
        s::alarm(u32::MAX);
        if s::alarm(0) < i32::MAX as u32 || errno::get() != 0 {
            return 7;
        }
        0
    });
}
#[test]
fn sym_asctime() {
    let p = s::asctime(Some(&sep16()));
    assert_eq!(text_at(p.expose_provenance()), "Sun Sep 16 01:03:52 1973\n");
    assert_eq!(s::asctime(Some(&sep16())), p, "one static buffer");
    errno::set_raw(0);
    assert!(s::asctime(None).is_null() && errno::get() == EFAULT);
    let far = struct_tm { tm_year: 8100, ..sep16() };
    errno::set_raw(0);
    assert!(s::asctime(Some(&far)).is_null() && errno::get() == EOVERFLOW);
}
#[test]
fn sym_asctime_r() {
    let mut b = [b'x'; 32];
    let p = s::asctime_r(Some(&sep16()), b.as_mut_ptr().cast());
    assert_eq!(p.cast::<u8>(), b.as_mut_ptr(), "the caller's buffer");
    assert_eq!(&b[..26], b"Sun Sep 16 01:03:52 1973\n\0");
    assert_eq!(b[26], b'x', "26 bytes written");
    errno::set_raw(0);
    assert!(s::asctime_r(Some(&sep16()), core::ptr::null_mut()).is_null() && errno::get() == EFAULT);
    errno::set_raw(0);
    assert!(s::asctime_r(None, b.as_mut_ptr().cast()).is_null() && errno::get() == EFAULT);
    let mut c = [b'y'; 32];
    let far = struct_tm { tm_year: 8100, ..sep16() };
    assert!(s::asctime_r(Some(&far), c.as_mut_ptr().cast()).is_null());
    assert_eq!(c, [b'y'; 32], "nothing written");
}
#[test]
fn sym_clock() {
    let a = s::clock();
    let cpu = || s::clock() - a;
    let spin = Instant::now();
    // Spin until 30 ms of CPU time (not wall time) has passed: under load a 30 ms wall spin may get far less CPU (FL-13).
    while cpu() < 30_000 && spin.elapsed() < Duration::from_secs(10) {
        std::hint::black_box(0u64);
    }
    let b = s::clock();
    assert!(b - a >= 20_000, "CLOCKS_PER_SEC = 1,000,000: 30 ms of CPU is about 30,000 ({a} -> {b})");
}

#[test]
fn sym_clock_adjtime() {
    let mut tx = Timex::default();
    assert!(s::clock_adjtime(0, Some(&mut tx)) >= 0);
    errno::set_raw(0);
    assert!(fails(s::clock_adjtime(1, Some(&mut tx)) as isize, EINVAL) || errno::get() == 95, "only CLOCK_REALTIME and PTP clocks");
}

#[test]
fn sym_clock_getcpuclockid() {
    let mut id = 0;
    assert_eq!(s::clock_getcpuclockid(0, Some(&mut id)), 0);
    let mut ts = Timespec::default();
    assert_eq!(s::clock_gettime(id, Some(&mut ts)), 0, "a readable CPU clock");
    assert_eq!(s::clock_getcpuclockid(std::process::id() as i32, Some(&mut id)), 0);
    errno::set_raw(0);
    assert_eq!(s::clock_getcpuclockid(i32::MAX - 1, Some(&mut id)), ESRCH, "returned, not in errno");
    assert_eq!(errno::get(), 0);
}

#[test]
fn sym_clock_getres() {
    let mut r = Timespec { sec: 9, nsec: 9 };
    assert_eq!(s::clock_getres(1, Some(&mut r)), 0);
    assert_eq!((r.sec, r.nsec), (0, 1), "high-resolution timers: 1 ns");
    assert_eq!(s::clock_getres(6, Some(&mut r)), 0);
    assert!(r.nsec > 1, "CLOCK_MONOTONIC_COARSE: a tick");
    assert_eq!(s::clock_getres(0, None), 0, "NULL: only the check");
    errno::set_raw(0);
    assert!(fails(s::clock_getres(999, None) as isize, EINVAL));
}

#[test]
fn sym_clock_gettime() {
    let mut ts = Timespec::default();
    assert_eq!(s::clock_gettime(0, Some(&mut ts)), 0);
    assert!((ts.sec as f64 + ts.nsec as f64 / 1e9 - now()).abs() < 1.0, "CLOCK_REALTIME");
    assert!((0..1_000_000_000).contains(&ts.nsec));
    let (mut a, mut b) = (Timespec::default(), Timespec::default());
    s::clock_gettime(1, Some(&mut a));
    std::thread::sleep(Duration::from_millis(2));
    s::clock_gettime(1, Some(&mut b));
    assert!((b.sec, b.nsec) > (a.sec, a.nsec), "CLOCK_MONOTONIC advances");
    errno::set_raw(0);
    assert!(fails(s::clock_gettime(1234, Some(&mut ts)) as isize, EINVAL));
    assert!(fails(s::clock_gettime(0, None) as isize, EFAULT));
}

#[test]
fn sym_clock_nanosleep() {
    let mut now_ = Timespec::default();
    s::clock_gettime(1, Some(&mut now_));
    let until = Timespec { sec: now_.sec, nsec: now_.nsec + 10_000_000 };
    let until = if until.nsec >= 1_000_000_000 { Timespec { sec: until.sec + 1, nsec: until.nsec - 1_000_000_000 } } else { until };
    assert_eq!(s::clock_nanosleep(1, 1, Some(&until), None), 0, "TIMER_ABSTIME");
    let mut after = Timespec::default();
    s::clock_gettime(1, Some(&mut after));
    assert!((after.sec, after.nsec) >= (until.sec, until.nsec));
    errno::set_raw(0);
    assert_eq!(s::clock_nanosleep(3, 0, Some(&now_), None), EINVAL, "the thread CPU clock; returned, not in errno");
    assert_eq!(errno::get(), 0);
    // The process CPU clock advances only while a thread of this process runs: a thread spins
    // until the sleep returns (under cargo-nextest the test is alone in its process, and a sleep
    // with no other thread on a CPU would never end).
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let spin = {
        let done = done.clone();
        std::thread::spawn(move || {
            while !done.load(std::sync::atomic::Ordering::Relaxed) {
                std::hint::spin_loop();
            }
        })
    };
    let r = s::clock_nanosleep(2, 0, Some(&Timespec { sec: 0, nsec: 1000 }), None);
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    spin.join().unwrap();
    assert_eq!(r, 0, "the process CPU clock");
}

#[test]
fn sym_clock_settime() {
    let mut ts = Timespec::default();
    s::clock_gettime(0, Some(&mut ts));
    errno::set_raw(0);
    if !root() {
        assert!(fails(s::clock_settime(0, Some(&ts)) as isize, EPERM), "CAP_SYS_TIME");
    }
    assert!(fails(s::clock_settime(1, Some(&ts)) as isize, EINVAL), "CLOCK_MONOTONIC is not settable");
    assert!(fails(s::clock_settime(0, None) as isize, EFAULT));
}

#[test]
fn sym_ctime() {
    zoned("sym_ctime", || {
        set_tz("UTC0");
        let t = 116_989_432i64;
        let p = s::ctime(Some(&t));
        assert_eq!(text_at(p.expose_provenance()), "Sun Sep 16 01:03:52 1973\n");
        assert_eq!(s::asctime(Some(&sep16())), p, "shared with asctime");
        errno::set_raw(0);
        assert!(s::ctime(None).is_null() && errno::get() == EFAULT);
        errno::set_raw(0);
        assert!(s::ctime(Some(&i64::MAX)).is_null() && errno::get() == EOVERFLOW);
        let far = 253_402_300_800i64; // year 10000 does not fit the layout
        errno::set_raw(0);
        assert!(s::ctime(Some(&far)).is_null() && errno::get() == EOVERFLOW);
    });
}
#[test]
fn sym_ctime_r() {
    zoned("sym_ctime_r", || {
        set_tz("JST-9");
        let t = 116_989_432i64;
        let mut b = [0u8; 26];
        assert_eq!(s::ctime_r(Some(&t), b.as_mut_ptr().cast()).cast::<u8>(), b.as_mut_ptr());
        assert_eq!(&b, b"Sun Sep 16 10:03:52 1973\n\0");
        errno::set_raw(0);
        assert!(s::ctime_r(Some(&t), core::ptr::null_mut()).is_null() && errno::get() == EFAULT);
        errno::set_raw(0);
        assert!(s::ctime_r(None, b.as_mut_ptr().cast()).is_null() && errno::get() == EFAULT);
    });
}
#[test]
fn sym___daylight() {
    zoned("sym___daylight", check_globals);
    conformance(&["s23_zones"]);
}
#[test]
fn sym_daylight() {
    zoned("sym_daylight", check_globals);
    conformance(&["s23_zones"]);
}
#[test]
fn sym_difftime() {
    assert_eq!(s::difftime(10, 3), 7.0);
    assert_eq!(s::difftime(i64::MIN, i64::MAX), -(2f64.powi(64)), "no overflow");
}
#[test]
fn sym_dysize() {
    assert_eq!((s::dysize(2024), s::dysize(1900), s::dysize(2000), s::dysize(i32::MIN)), (366, 365, 366, 366));
}
#[test]
fn sym_ftime() {
    let mut tb = struct_timeb { time: 0, millitm: 9999, timezone: 7, dstflag: 7 };
    assert_eq!(s::ftime(Some(&mut tb)), 0);
    assert!((tb.time as f64 - now()).abs() < 2.0);
    assert!(tb.millitm < 1000, "milliseconds");
    assert_eq!((tb.timezone, tb.dstflag), (0, 0));
}

#[test]
fn sym_getitimer() {
    single("sym_getitimer", |k| {
        let mut cur = itv(7, 7);
        if s::getitimer(ITIMER_VIRTUAL, Some(&mut cur)) != 0 || cur != itv(0, 0) {
            return 1;
        }
        if s::setitimer(ITIMER_VIRTUAL, Some(&itv(1_000_000, 20_000)), None) != 0 {
            return 2;
        }
        // (The kernel reports a CPU-time timer rounded up to its tick.)
        if s::getitimer(ITIMER_VIRTUAL, Some(&mut cur)) != 0 || us(cur.interval) != 20_000 || !(900_000..=1_100_000).contains(&us(cur.value)) {
            return 3;
        }
        // An unknown timer (the object untouched), and a NULL object.
        let mut keep = itv(7, 7);
        errno::set_raw(0);
        if s::getitimer(3, Some(&mut keep)) != -1 || errno::get() != EINVAL || keep != itv(7, 7) {
            return 4;
        }
        errno::set_raw(0);
        if s::getitimer(ITIMER_REAL, None) != -1 || errno::get() != EFAULT {
            return 5;
        }
        let _ = k;
        0
    });
}
#[test]
fn sym___gettimeofday() {
    let mut tv = Timeval { sec: 0, usec: -1 };
    let mut tz = Timezone { minuteswest: 7, dsttime: 7 };
    assert_eq!(s::__gettimeofday(Some(&mut tv), Some(&mut tz)), 0);
    assert!((tv.sec as f64 - now()).abs() < 2.0 && (0..1_000_000).contains(&tv.usec), "microseconds");
    assert_eq!((tz.minuteswest, tz.dsttime), (0, 0), "the obsolete zone is zero-filled");
    assert_eq!(s::__gettimeofday(None, None), 0);
}

#[test]
fn sym_gettimeofday() {
    let mut tv = Timeval { sec: 0, usec: -1 };
    let mut tz = Timezone { minuteswest: 7, dsttime: 7 };
    assert_eq!(s::gettimeofday(Some(&mut tv), Some(&mut tz)), 0);
    assert!((tv.sec as f64 - now()).abs() < 2.0 && (0..1_000_000).contains(&tv.usec), "microseconds");
    assert_eq!((tz.minuteswest, tz.dsttime), (0, 0), "the obsolete zone is zero-filled");
    assert_eq!(s::gettimeofday(None, None), 0);
}

#[test]
fn sym_gmtime() {
    let p = s::gmtime(Some(&951_782_400));
    let g = tm_at(p);
    assert_eq!((g.tm_year, g.tm_mon, g.tm_mday, g.tm_wday, g.tm_yday, g.tm_isdst, g.tm_gmtoff), (100, 1, 29, 2, 59, 0, 0));
    assert_eq!(text_at(g.tm_zone), "GMT");
    assert_eq!(s::gmtime(Some(&0)), p, "one static struct tm");
    errno::set_raw(0);
    assert!(s::gmtime(None).is_null() && errno::get() == EFAULT);
    errno::set_raw(0);
    assert!(s::gmtime(Some(&i64::MIN)).is_null() && errno::get() == EOVERFLOW);
}
#[test]
fn sym___gmtime_r() {
    r_form(s::__gmtime_r);
}
#[test]
fn sym_gmtime_r() {
    r_form(s::gmtime_r);
    let mut o = tm_zero();
    s::gmtime_r(Some(&-1), Some(&mut o));
    assert_eq!((o.tm_year, o.tm_mon, o.tm_mday, o.tm_hour, o.tm_min, o.tm_sec, o.tm_yday), (69, 11, 31, 23, 59, 59, 364));
}
#[test]
fn sym_localtime() {
    zoned("sym_localtime", || {
        set_tz("NZST-12NZDT,M9.5.0,M4.1.0/3");
        let p = s::localtime(Some(&1_704_067_200));
        let l = tm_at(p);
        assert_eq!((l.tm_mday, l.tm_hour, l.tm_isdst, l.tm_gmtoff), (1, 13, 1, 46_800));
        assert_eq!(text_at(l.tm_zone), "NZDT");
        assert_eq!(s::gmtime(Some(&0)), p, "shared with gmtime");
        errno::set_raw(0);
        assert!(s::localtime(None).is_null() && errno::get() == EFAULT);
        errno::set_raw(0);
        assert!(s::localtime(Some(&i64::MAX)).is_null() && errno::get() == EOVERFLOW);
    });
}
#[test]
fn sym_localtime_r() {
    zoned("sym_localtime_r", || {
        set_tz("UTC0");
        r_form(s::localtime_r);
        set_tz("CET-1CEST,M3.5.0,M10.5.0/3");
        let mut o = tm_zero();
        s::localtime_r(Some(&1_720_000_000), Some(&mut o));
        assert_eq!((o.tm_hour, o.tm_isdst, o.tm_gmtoff), (11, 1, 7200));
        assert_eq!(text_at(o.tm_zone), "CEST");
    });
}
/// P7: the thread's remembered period of the last rule zone never answers for another one: `TZ`
/// changed between calls of one thread, the same instants, `localtime_r` and `mktime`.
#[test]
fn localtime_r_follows_tz_changes() {
    // Trace: no spec clause (an internal per-thread cache; the answers are those of localtime_r and mktime); P7.
    zoned("localtime_r_follows_tz_changes", || {
        let t = 1_720_000_000i64;
        for round in 0..3 {
            for (tz, hour, isdst, off, name) in [("CET-1CEST,M3.5.0,M10.5.0/3", 11, 1, 7200, "CEST"), ("EST5EDT,M3.2.0,M11.1.0", 5, 1, -14400, "EDT"), ("EST5EDT,M3.2.0,M11.1.0", 5, 1, -14400, "EDT"), ("AAA-10BBB,M10.1.0,M4.1.0/3", 19, 0, 36000, "AAA"), ("CET-1CEST,M3.5.0,M10.5.0/3", 11, 1, 7200, "CEST")] {
                set_tz(tz);
                let mut o = tm_zero();
                s::localtime_r(Some(&t), Some(&mut o));
                assert_eq!((o.tm_hour, o.tm_isdst, o.tm_gmtoff), (hour, isdst, off), "{tz} round {round}");
                assert_eq!(text_at(o.tm_zone), name, "{tz}");
                let mut m = o;
                assert_eq!(s::mktime(Some(&mut m)), t, "mktime {tz}");
                assert_eq!((m.tm_hour, m.tm_isdst), (hour, isdst), "mktime {tz}");
            }
        }
    });
}
/// TB-SLOTS: the thread's period lives in its block's cache words: the `%fs`-relative loads
/// read what the block holds, each thread keeps its own, a hit leaves the words alone, and a
/// forked child starts with its parent's (still valid: zone numbers are never reused) and
/// changes only its own.
#[test]
fn tz_period_words_per_thread_and_fork() {
    // Trace: no spec clause (an internal per-thread cache; the answers are localtime_r's); TB-SLOTS.
    zoned("tz_period_words_per_thread_and_fork", || {
        use liella_boundary::tb;
        use liella_rt::tls::CACHE_TZ_PERIOD;
        use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
        const SUMMER: i64 = 1_720_000_000; // 05:46 EDT
        const WINTER: i64 = 1_700_000_000; // 17:13 EST
        const DST: usize = 1 << 32;
        static PARENT: [AtomicUsize; 3] = [const { AtomicUsize::new(0) }; 3];
        fn words() -> [usize; 3] {
            let r = [tb::read::<tb::TzPeriod0>(), tb::read::<tb::TzPeriod1>(), tb::read::<tb::TzPeriod2>()];
            let c = &tb::current().cache;
            assert_eq!(r, [0, 1, 2].map(|i| c[CACHE_TZ_PERIOD + i].load(Relaxed)), "tb::read vs the block");
            r
        }
        fn kind_at(t: i64) -> (i32, i32) {
            let mut o = tm_zero();
            s::localtime_r(Some(&t), Some(&mut o));
            (o.tm_hour, o.tm_isdst)
        }
        set_tz("EST5EDT,M3.2.0,M11.1.0");
        assert_eq!(kind_at(SUMMER), (5, 1));
        let mine = words();
        assert!(mine[0] & 0xffff_ffff != 0 && mine[0] & DST != 0, "a DST period is kept: {mine:x?}");
        let theirs = std::thread::spawn(|| {
            assert_eq!(kind_at(WINTER), (17, 0));
            let w = words();
            assert_eq!(kind_at(WINTER + 60), (17, 0));
            assert_eq!(words(), w, "a hit stores nothing");
            w
        })
        .join()
        .unwrap();
        assert_eq!(theirs[0], mine[0] & !DST, "the same zone, standard time: {theirs:x?} {mine:x?}");
        assert_eq!(words(), mine, "another thread's period is its own");
        assert_eq!(kind_at(SUMMER + 60), (5, 1));
        assert_eq!(words(), mine);
        for (p, w) in PARENT.iter().zip(mine) {
            p.store(w, Relaxed);
        }
        let end = liella_boundary::selftest::in_child(|_| {
            if words() != PARENT.each_ref().map(|p| p.load(Relaxed)) {
                return 1;
            }
            if kind_at(SUMMER + 120) != (5, 1) || words()[0] & DST == 0 {
                return 2;
            }
            if kind_at(WINTER) != (17, 0) || words()[0] & DST != 0 {
                return 3;
            }
            0
        });
        assert_eq!(end, Ok(ChildEnd::Exited(0)));
        assert_eq!(words(), mine, "the child's period stays in the child");
    });
}

#[test]
fn sym_mktime() {
    zoned("sym_mktime", || mktime_form(s::mktime));
}
#[test]
fn sym_nanosleep() {
    let t = Instant::now();
    let req = Timespec { sec: 0, nsec: 15_000_000 };
    let mut rem = Timespec { sec: 9, nsec: 9 };
    assert_eq!(s::nanosleep(Some(&req), Some(&mut rem)), 0);
    assert!(t.elapsed() >= Duration::from_millis(15));
    errno::set_raw(0);
    assert!(fails(s::nanosleep(Some(&Timespec { sec: 0, nsec: 1_000_000_000 }), None) as isize, EINVAL));
    assert!(fails(s::nanosleep(Some(&Timespec { sec: -1, nsec: 0 }), None) as isize, EINVAL));
    assert!(fails(s::nanosleep(None, None) as isize, EFAULT));
}

#[test]
fn sym___nanosleep() {
    let t = Instant::now();
    let req = Timespec { sec: 0, nsec: 15_000_000 };
    let mut rem = Timespec { sec: 9, nsec: 9 };
    assert_eq!(s::__nanosleep(Some(&req), Some(&mut rem)), 0);
    assert!(t.elapsed() >= Duration::from_millis(15));
    errno::set_raw(0);
    assert!(fails(s::__nanosleep(Some(&Timespec { sec: 0, nsec: 1_000_000_000 }), None) as isize, EINVAL));
    assert!(fails(s::__nanosleep(Some(&Timespec { sec: -1, nsec: 0 }), None) as isize, EINVAL));
    assert!(fails(s::__nanosleep(None, None) as isize, EFAULT));
}

#[test]
fn sym_ntp_adjtime() {
    let mut tx = Timex::default();
    let state = s::ntp_adjtime(Some(&mut tx));
    assert!((0..=5).contains(&state), "TIME_OK .. TIME_ERROR: {state}");
    assert!(tx.tick > 0 && tx.precision > 0, "the kernel filled the record");
    errno::set_raw(0);
    tx.modes = 0x0001; // ADJ_OFFSET
    if !root() {
        assert!(fails(s::ntp_adjtime(Some(&mut tx)) as isize, EPERM));
    }
    assert!(fails(s::ntp_adjtime(None) as isize, EFAULT));
}

#[test]
fn sym_ntp_gettime() {
    let mut old = NtpTimevalOld { time: Timeval { sec: 0, usec: 0 }, maxerror: -1, esterror: -1 };
    let state = s::ntp_gettime(Some(&mut old));
    assert!((0..=5).contains(&state));
    assert!((old.time.sec as f64 - now()).abs() < 2.0);
    assert!(old.maxerror >= 0 && old.esterror >= 0, "the error estimates");
    errno::set_raw(0);
    assert!(fails(s::ntp_gettime(None) as isize, EFAULT));
}

#[test]
fn sym_ntp_gettimex() {
    let mut v = ntv();
    let state = s::ntp_gettimex(Some(&mut v));
    assert!((0..=5).contains(&state));
    assert!((v.time.tv_sec as f64 - now()).abs() < 2.0);
    assert!(v.tai >= 0, "the TAI offset");
    assert_eq!([v.__glibc_reserved1, v.__glibc_reserved2, v.__glibc_reserved3, v.__glibc_reserved4], [0; 4], "reserved fields zeroed");
}

#[test]
fn sym_setitimer() {
    single("sym_setitimer", |_| {
        let mut old = itv(7, 7);
        if s::setitimer(ITIMER_REAL, Some(&itv(3_000_000, 250_000)), Some(&mut old)) != 0 || old != itv(0, 0) {
            return 1;
        }
        // The old setting; ITIMER_REAL is alarm's timer.
        if s::setitimer(ITIMER_REAL, Some(&itv(3_000_000, 250_000)), Some(&mut old)) != 0 || us(old.interval) != 250_000 || us(old.value) > 3_000_000 {
            return 2;
        }
        if s::alarm(0) != 3 {
            return 3;
        }
        let mut cur = itv(7, 7);
        if s::getitimer(ITIMER_REAL, Some(&mut cur)) != 0 || cur != itv(0, 0) {
            return 4;
        }
        // Invalid timers and fields (the old object untouched).
        let bad = [
            (3, itv(0, 0)),
            (-1, itv(0, 0)),
            (ITIMER_REAL, Itimerval { interval: tv(0), value: Timeval { sec: 0, usec: 1_000_000 } }),
            (ITIMER_REAL, Itimerval { interval: Timeval { sec: 0, usec: 1_000_000 }, value: tv(1) }),
            (ITIMER_REAL, Itimerval { interval: tv(0), value: Timeval { sec: -1, usec: 0 } }),
            (ITIMER_REAL, Itimerval { interval: tv(0), value: Timeval { sec: 0, usec: -1 } }),
        ];
        for (i, (which, v)) in bad.iter().enumerate() {
            let mut keep = itv(7, 7);
            errno::set_raw(0);
            if s::setitimer(*which, Some(v), Some(&mut keep)) != -1 || errno::get() != EINVAL || keep != itv(7, 7) {
                return 10 + i as i32;
            }
        }
        // A NULL setting is EFAULT (S6; glibc passes it to the kernel, which takes it as zero).
        errno::set_raw(0);
        if s::setitimer(ITIMER_REAL, None, None) != -1 || errno::get() != EFAULT {
            return 5;
        }
        0
    });
}
#[test]
fn sym_settimeofday() {
    let tv = Timeval { sec: 1, usec: 0 };
    let tz = Timezone { minuteswest: 0, dsttime: 0 };
    errno::set_raw(0);
    assert!(fails(s::settimeofday(Some(&tv), Some(&tz)) as isize, EINVAL), "a time zone with a time (2.31+)");
    assert!(fails(s::settimeofday(Some(&Timeval { sec: 1, usec: 1_000_000 }), None) as isize, EINVAL));
    if !root() {
        assert!(fails(s::settimeofday(Some(&tv), None) as isize, EPERM));
        assert!(fails(s::settimeofday(None, Some(&tz)) as isize, EPERM), "the zone-only call reaches the kernel");
    }
}

#[test]
fn sym_sleep() {
    let t = Instant::now();
    assert_eq!(s::sleep(0), 0);
    assert_eq!(s::sleep(1), 0, "complete: 0");
    assert!(t.elapsed() >= Duration::from_secs(1));
}

#[test]
fn sym_stime() {
    errno::set_raw(0);
    if !root() {
        assert!(fails(s::liella_compat_stime(Some(&1)) as isize, EPERM));
    }
    assert!(fails(s::liella_compat_stime(None) as isize, EFAULT));
}

#[test]
fn sym_time() {
    let mut t = 0i64;
    let r = s::time(Some(&mut t));
    assert_eq!(r, t, "returned and stored");
    assert!((r as f64 - now()).abs() < 2.0);
    assert!(s::time(None) >= r, "NULL: returned only");
}

#[test]
fn sym_timegm() {
    let mut t = struct_tm { tm_mday: 32, tm_year: 124, tm_isdst: 1, tm_gmtoff: 99, ..tm_zero() };
    assert_eq!(s::timegm(Some(&mut t)), 1_706_745_600);
    assert_eq!((t.tm_mon, t.tm_mday, t.tm_yday, t.tm_wday, t.tm_isdst, t.tm_gmtoff), (1, 1, 31, 4, 0, 0));
    assert_eq!(text_at(t.tm_zone), "GMT");
    errno::set_raw(0);
    assert_eq!((s::timegm(None), errno::get()), (-1, EFAULT));
    let mut big = struct_tm { tm_year: i32::MAX, tm_mon: 12, tm_mday: 1, ..tm_zero() };
    errno::set_raw(0);
    assert_eq!((s::timegm(Some(&mut big)), errno::get()), (-1, EOVERFLOW));
}
#[test]
fn sym_timelocal() {
    zoned("sym_timelocal", || mktime_form(s::timelocal));
}
#[test]
fn sym_timer_create() {
    let mut made = Vec::new();
    for clock in [0, 1, 2, 3, 7] {
        made.push(create(clock, Some(&none())).unwrap());
    }
    made.push(create(0, None).unwrap());
    made.push(create(1, Some(&SigEvent::new(SIGEV_SIGNAL, SIGUSR1, 9, 0))).unwrap());
    made.push(create(1, Some(&thread_event(0))).unwrap());
    let mut distinct = made.clone();
    distinct.sort_unstable();
    distinct.dedup();
    assert_eq!(distinct.len(), made.len(), "distinct ids");
    for t in made {
        assert_eq!(s::timer_delete(tid(t)), 0);
    }
    // The clock (MONOTONIC_RAW and the coarse clocks cannot drive a timer).
    assert_eq!(create(99, Some(&none())), Err(EINVAL));
    assert_eq!(create(4, Some(&none())), Err(EOPNOTSUPP));
    assert_eq!(create(5, Some(&none())), Err(EOPNOTSUPP));
    // The notification and the signal.
    assert_eq!(create(1, Some(&SigEvent::new(99, SIGUSR1, 0, 0))), Err(EINVAL));
    assert_eq!(create(1, Some(&SigEvent::new(SIGEV_SIGNAL, 0, 0, 0))), Err(EINVAL));
    assert_eq!(create(1, Some(&SigEvent::new(SIGEV_SIGNAL, 65, 0, 0))), Err(EINVAL));
    assert_eq!(create(1, Some(&SigEvent::new(SIGEV_THREAD_ID, SIGUSR1, 0, 1))), Err(EINVAL), "a thread of another process");
    // SIGEV_THREAD: a function is required (glibc accepts NULL and crashes at the expiry); a
    // timerfd measures neither the CPU-time clocks nor CLOCK_TAI (ENOTSUP; glibc accepts them);
    // the clock errors are the kernel timers'; the attributes must be an object.
    assert_eq!(create(1, Some(&SigEvent::thread(0, 0, 0))), Err(EINVAL));
    assert_eq!(create(2, Some(&thread_event(0))), Err(EOPNOTSUPP));
    assert_eq!(create(3, Some(&thread_event(0))), Err(EOPNOTSUPP));
    assert_eq!(create(11, Some(&thread_event(0))), Err(EOPNOTSUPP));
    assert_eq!(create(4, Some(&thread_event(0))), Err(EOPNOTSUPP));
    assert_eq!(create(99, Some(&thread_event(0))), Err(EINVAL));
    assert_eq!(create(1, Some(&thread_event(0x1003))), Err(EINVAL), "misaligned attributes");
    // NULL for the id: EFAULT (glibc crashes).
    errno::set_raw(0);
    assert_eq!(s::timer_create(1, Some(&none()), None), -1);
    assert_eq!(errno::get(), EFAULT);
}
#[test]
fn sym_timer_delete() {
    for ev in [none(), thread_event(0)] {
        let t = create(1, Some(&ev)).unwrap();
        assert_eq!(s::timer_delete(tid(t)), 0);
        errno::set_raw(0);
        assert_eq!(s::timer_delete(tid(t)), -1, "deleted twice");
        assert_eq!(errno::get(), EINVAL);
    }
    for bad in [0x7fff_1234usize, 1 << 40, usize::MAX] {
        errno::set_raw(0);
        assert_eq!(s::timer_delete(tid(bad)), -1, "{bad:#x}");
        assert_eq!(errno::get(), EINVAL);
    }
}
#[test]
fn sym_timer_getoverrun() {
    for ev in [none(), thread_event(0)] {
        let t = create(1, Some(&ev)).unwrap();
        errno::set_raw(0);
        assert_eq!(s::timer_getoverrun(tid(t)), 0, "no notification yet");
        assert_eq!(errno::get(), 0);
        s::timer_delete(tid(t));
        assert_eq!(s::timer_getoverrun(tid(t)), -1, "a deleted timer");
        assert_eq!(errno::get(), EINVAL);
    }
    errno::set_raw(0);
    assert_eq!(s::timer_getoverrun(tid(0x7fff_1234)), -1);
    assert_eq!(errno::get(), EINVAL);
    // The count itself is s23_4_c_3_4's (signals) and s23_4_c_3_3's (SIGEV_THREAD).
}
#[test]
fn sym_timer_gettime() {
    for ev in [none(), thread_event(0)] {
        let t = create(1, Some(&ev)).unwrap();
        let mut cur = its(ms(7), ms(7));
        assert_eq!(s::timer_gettime(tid(t), Some(&mut cur)), 0);
        assert_eq!(cur, Itimerspec::default(), "disarmed");
        s::timer_settime(tid(t), 0, Some(&its(ms(5000), ms(250))), None);
        assert_eq!(s::timer_gettime(tid(t), Some(&mut cur)), 0);
        assert_eq!(cur.interval, ms(250));
        assert!((4_000_000_000..=5_000_000_000).contains(&ns(cur.value)), "{cur:?}");
        errno::set_raw(0);
        assert_eq!(s::timer_gettime(tid(t), None), -1);
        assert_eq!(errno::get(), EFAULT);
        s::timer_delete(tid(t));
        errno::set_raw(0);
        assert_eq!(s::timer_gettime(tid(t), Some(&mut cur)), -1, "a deleted timer");
        assert_eq!(errno::get(), EINVAL);
    }
}
#[test]
fn sym_timer_settime() {
    for ev in [none(), thread_event(0)] {
        let t = create(1, Some(&ev)).unwrap();
        let mut old = its(ms(7), ms(7));
        assert_eq!(s::timer_settime(tid(t), 0, Some(&its(ms(5000), ms(0))), Some(&mut old)), 0);
        assert_eq!(old, Itimerspec::default(), "it was disarmed");
        assert_eq!(s::timer_settime(tid(t), 0, Some(&its(ms(3000), ms(100))), Some(&mut old)), 0);
        assert!((4_000_000_000..=5_000_000_000).contains(&ns(old.value)) && ns(old.interval) == 0, "{old:?}");
        // Absolute, on the timer's clock.
        let mut now = Timespec::default();
        s::clock_gettime(1, Some(&mut now));
        let at = Timespec { sec: now.sec + 2, nsec: now.nsec };
        assert_eq!(s::timer_settime(tid(t), 1, Some(&its(at, ms(0))), None), 0);
        let mut cur = Itimerspec::default();
        s::timer_gettime(tid(t), Some(&mut cur));
        assert!((1_000_000_000..=2_000_000_000).contains(&ns(cur.value)), "{cur:?}");
        // Unknown flags are ignored.
        assert_eq!(s::timer_settime(tid(t), 2, Some(&its(ms(0), ms(0))), None), 0);
        assert_eq!(s::timer_settime(tid(t), -2, Some(&its(ms(0), ms(0))), None), 0);
        // Invalid settings: EINVAL, and the timer unchanged.
        s::timer_settime(tid(t), 0, Some(&its(ms(5000), ms(0))), None);
        let bad = [
            its(Timespec { sec: 0, nsec: 1_000_000_000 }, ms(0)),
            its(ms(1000), Timespec { sec: 0, nsec: 1_000_000_000 }),
            its(Timespec { sec: -1, nsec: 0 }, ms(0)),
            its(Timespec { sec: 0, nsec: -1 }, ms(0)),
        ];
        for b in &bad {
            errno::set_raw(0);
            assert_eq!(s::timer_settime(tid(t), 0, Some(b), None), -1, "{b:?}");
            assert_eq!(errno::get(), EINVAL);
        }
        errno::set_raw(0);
        assert_eq!(s::timer_settime(tid(t), 0, None, None), -1, "NULL setting");
        assert_eq!(errno::get(), EINVAL);
        s::timer_gettime(tid(t), Some(&mut cur));
        assert!(ns(cur.value) > 4_000_000_000, "unchanged: {cur:?}");
        s::timer_delete(tid(t));
        errno::set_raw(0);
        assert_eq!(s::timer_settime(tid(t), 0, Some(&its(ms(1), ms(0))), None), -1, "a deleted timer");
        assert_eq!(errno::get(), EINVAL);
    }
}
#[test]
fn sym_timerfd_create() {
    let fd = s::timerfd_create(1, 0);
    assert!(fd >= 0);
    assert_eq!(CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFd, 0), Ok(0));
    assert_eq!(CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFl, 0).unwrap() & i64::from(TFD_NONBLOCK), 0);
    let _ = CSys.close(Fd::new(fd));
    let fd = s::timerfd_create(0, TFD_NONBLOCK | TFD_CLOEXEC);
    assert!(fd >= 0);
    assert_eq!(CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFd, 0), Ok(1), "close-on-exec");
    assert_ne!(CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFl, 0).unwrap() & i64::from(TFD_NONBLOCK), 0, "non-blocking");
    let _ = CSys.close(Fd::new(fd));
    let fd = s::timerfd_create(7, 0);
    assert!(fd >= 0, "CLOCK_BOOTTIME");
    let _ = CSys.close(Fd::new(fd));
    for (clock, flags) in [(1, 1), (99, 0), (2, 0), (3, 0), (4, 0)] {
        errno::set_raw(0);
        assert_eq!(s::timerfd_create(clock, flags), -1, "clock {clock}, flags {flags}");
        assert_eq!(errno::get(), EINVAL);
    }
}
#[test]
fn sym_timerfd_gettime() {
    let fd = s::timerfd_create(1, TFD_CLOEXEC);
    let mut cur = its(ms(7), ms(7));
    assert_eq!(s::timerfd_gettime(fd, Some(&mut cur)), 0);
    assert_eq!(cur, Itimerspec::default(), "disarmed");
    s::timerfd_settime(fd, 0, Some(&its(ms(5000), ms(20))), None);
    assert_eq!(s::timerfd_gettime(fd, Some(&mut cur)), 0);
    assert!(cur.interval == ms(20) && (4_000_000_000..=5_000_000_000).contains(&ns(cur.value)), "{cur:?}");
    errno::set_raw(0);
    assert_eq!(s::timerfd_gettime(fd, None), -1);
    assert_eq!(errno::get(), EFAULT);
    let _ = CSys.close(Fd::new(fd));
    errno::set_raw(0);
    assert_eq!(s::timerfd_gettime(931, Some(&mut cur)), -1, "a closed descriptor");
    assert_eq!(errno::get(), EBADF);
}
#[test]
fn sym_timerfd_settime() {
    let fd = s::timerfd_create(1, TFD_CLOEXEC);
    let mut old = its(ms(7), ms(7));
    assert_eq!(s::timerfd_settime(fd, 0, Some(&its(ms(5000), ms(20))), Some(&mut old)), 0);
    assert_eq!(old, Itimerspec::default());
    assert_eq!(s::timerfd_settime(fd, 0, Some(&its(ms(0), ms(0))), Some(&mut old)), 0, "disarm");
    assert!(old.interval == ms(20) && ns(old.value) > 4_000_000_000, "{old:?}");
    let mut now = Timespec::default();
    s::clock_gettime(1, Some(&mut now));
    assert_eq!(s::timerfd_settime(fd, 1, Some(&its(Timespec { sec: now.sec + 2, nsec: 0 }, ms(0))), None), 0, "TFD_TIMER_ABSTIME");
    for (flags, v) in [(4, its(ms(1), ms(0))), (0, its(Timespec { sec: 0, nsec: 1_000_000_000 }, ms(0))), (0, its(Timespec { sec: -1, nsec: 0 }, ms(0)))] {
        errno::set_raw(0);
        assert_eq!(s::timerfd_settime(fd, flags, Some(&v), None), -1, "flags {flags}, {v:?}");
        assert_eq!(errno::get(), EINVAL);
    }
    errno::set_raw(0);
    assert_eq!(s::timerfd_settime(fd, 0, None, None), -1, "NULL setting");
    assert_eq!(errno::get(), EFAULT);
    let _ = CSys.close(Fd::new(fd));
    // TFD_TIMER_CANCEL_ON_SET is accepted on a realtime timer (it acts only with ABSTIME).
    let fd = s::timerfd_create(0, TFD_CLOEXEC);
    assert_eq!(s::timerfd_settime(fd, 2, Some(&its(ms(0), ms(0))), None), 0);
    assert_eq!(s::timerfd_settime(fd, 3, Some(&its(Timespec { sec: now.sec + 1_000_000_000, nsec: 0 }, ms(0))), None), 0);
    let _ = CSys.close(Fd::new(fd));
    // Not a timer, and not a descriptor.
    let [r, w] = CSys.pipe2(0x80000).unwrap();
    errno::set_raw(0);
    assert_eq!(s::timerfd_settime(r.raw(), 0, Some(&its(ms(1), ms(0))), None), -1, "a pipe");
    assert_eq!(errno::get(), EINVAL);
    let _ = (CSys.close(r), CSys.close(w));
    errno::set_raw(0);
    assert_eq!(s::timerfd_settime(932, 0, Some(&its(ms(1), ms(0))), None), -1, "a closed descriptor");
    assert_eq!(errno::get(), EBADF);
}
#[test]
fn sym_times() {
    let mut t = Tms { utime: -1, stime: -1, cutime: -1, cstime: -1 };
    let a = s::times(Some(&mut t));
    assert!(a > 0, "elapsed ticks");
    assert!(t.utime >= 0 && t.stime >= 0 && t.cutime >= 0 && t.cstime >= 0);
    std::thread::sleep(Duration::from_millis(30));
    let b = s::times(None);
    assert!(b - a >= 2, "100 ticks a second: 30 ms is about 3");
}

#[test]
fn sym_timespec_get() {
    let mut ts = Timespec::default();
    assert_eq!(s::timespec_get(Some(&mut ts), 1), 1, "TIME_UTC");
    assert!((ts.sec as f64 - now()).abs() < 2.0);
    assert_eq!(s::timespec_get(Some(&mut ts), 2), 0, "TIME_MONOTONIC is not supported (2.39)");
}

#[test]
fn sym_timespec_getres() {
    let mut ts = Timespec::default();
    assert_eq!(s::timespec_getres(Some(&mut ts), 1), 1);
    assert_eq!((ts.sec, ts.nsec), (0, 1));
    assert_eq!(s::timespec_getres(None, 1), 1, "the resolution need not be stored");
    assert_eq!(s::timespec_getres(Some(&mut ts), 3), 0);
}

#[test]
fn sym___timezone() {
    zoned("sym___timezone", check_globals);
    conformance(&["s23_zones"]);
}
#[test]
fn sym_timezone() {
    zoned("sym_timezone", check_globals);
    conformance(&["s23_zones"]);
}
#[test]
fn sym___tzname() {
    zoned("sym___tzname", check_globals);
    conformance(&["s23_zones"]);
}
#[test]
fn sym_tzname() {
    zoned("sym_tzname", check_globals);
    conformance(&["s23_zones"]);
}
#[test]
fn sym_tzset() {
    zoned("sym_tzset", || {
        set_tz("JST-9");
        assert_eq!(tz_globals().0, "JST");
        // An explicit tzset writes the globals again even when the zone is unchanged.
        let g = liella_boundary::data::tz_globals();
        g.timezone.store(1, std::sync::atomic::Ordering::SeqCst);
        s::tzset();
        assert_eq!(tz_globals().2, -32_400);
        // It reads TZ again.
        let v = std::ffi::CString::new("EST5").unwrap();
        envx::setenv(c"TZ".as_ptr(), v.as_ptr(), 1);
        s::tzset();
        assert_eq!(tz_globals(), ("EST".into(), "EST".into(), 18_000, 0));
    });
}
#[test]
fn sym_ualarm() {
    single("sym_ualarm", |_| {
        if s::ualarm(0, 0) != 0 || s::ualarm(500_000, 0) != 0 {
            return 1;
        }
        if !(400_000..=500_000).contains(&s::ualarm(0, 0)) {
            return 2;
        }
        // A second or more: (useconds_t)-1 with EINVAL, nothing set.
        for (v, i) in [(1_000_000, 0), (0, 1_000_000), (2_500_000, 0)] {
            errno::set_raw(0);
            if s::ualarm(v, i) != u32::MAX || errno::get() != EINVAL || s::alarm(0) != 0 {
                return 3;
            }
        }
        // It sets ITIMER_REAL, period included.
        s::ualarm(300_000, 100_000);
        let mut cur = itv(7, 7);
        if s::getitimer(ITIMER_REAL, Some(&mut cur)) != 0 || us(cur.interval) != 100_000 || !(200_000..=300_000).contains(&us(cur.value)) {
            return 4;
        }
        s::ualarm(0, 0);
        if s::getitimer(ITIMER_REAL, Some(&mut cur)) != 0 || cur != itv(0, 0) {
            return 5;
        }
        // The microseconds left on alarm's timer; beyond useconds_t, the largest value that is
        // not the error result (glibc wraps it).
        s::alarm(2);
        if !(1_900_000..=2_000_000).contains(&s::ualarm(0, 0)) {
            return 6;
        }
        s::alarm(5000);
        if s::ualarm(0, 0) != u32::MAX - 1 {
            return 7;
        }
        0
    });
}
#[test]
fn sym_usleep() {
    let t = Instant::now();
    assert_eq!(s::usleep(20_000), 0);
    assert!(t.elapsed() >= Duration::from_millis(20));
    let t = Instant::now();
    assert_eq!(s::usleep(1_000_001), 0, "a million or more is accepted (glibc)");
    assert!(t.elapsed() >= Duration::from_secs(1));
}


// ------------------------------------------------------------------------------ §23.5–23.6
//
// Broken-down time and zones. `TZ`, the selected zone and the globals are process-wide: the
// symbols that depend on them run in a re-executed copy of the test (`zoned`). Library-owned
// results are read through the exported `strlen`/`memcpy` (no unsafe in tests). The data
// objects are also checked from C, against the products (`conformance/c/s23_zones.c`).

use liella_abi::layout::struct_tm;
use liella_boundary::posix::s01_startup as envx;
use liella_boundary::posix::s04_strings as strs;

const EOVERFLOW: i32 = 75;

fn tm_zero() -> struct_tm {
    struct_tm { tm_sec: 0, tm_min: 0, tm_hour: 0, tm_mday: 0, tm_mon: 0, tm_year: 0, tm_wday: 0, tm_yday: 0, tm_isdst: 0, tm_gmtoff: 0, tm_zone: 0 }
}

/// 1973-09-16 01:03:52, a Sunday.
fn sep16() -> struct_tm {
    struct_tm { tm_sec: 52, tm_min: 3, tm_hour: 1, tm_mday: 16, tm_mon: 8, tm_year: 73, tm_wday: 0, tm_yday: 258, tm_isdst: 0, tm_gmtoff: 0, tm_zone: 0 }
}

fn text_at(p: usize) -> String {
    let p: *const core::ffi::c_char = core::ptr::with_exposed_provenance(p);
    let n = strs::strlen(p);
    let mut v = vec![0u8; n];
    strs::memcpy(v.as_mut_ptr().cast(), p.cast(), n);
    String::from_utf8(v).unwrap()
}

fn tm_at(p: *mut struct_tm) -> struct_tm {
    let mut out = tm_zero();
    strs::memcpy(core::ptr::from_mut(&mut out).cast(), p.cast_const().cast(), core::mem::size_of::<struct_tm>());
    out
}

fn set_tz(tz: &str) {
    let v = std::ffi::CString::new(tz).unwrap();
    assert_eq!(envx::setenv(c"TZ".as_ptr(), v.as_ptr(), 1), 0);
    s::tzset();
}

/// tzname[0], tzname[1], timezone, daylight.
fn tz_globals() -> (String, String, i64, i32) {
    let g = liella_boundary::data::tz_globals();
    let name = |i: usize| text_at(g.tzname.get(i).unwrap().load(std::sync::atomic::Ordering::SeqCst));
    (name(0), name(1), g.timezone.load(std::sync::atomic::Ordering::SeqCst), g.daylight.load(std::sync::atomic::Ordering::SeqCst))
}

/// Runs `body` in a re-executed copy of this test, alone (the zone is process-wide).
fn zoned(test: &str, body: impl FnOnce()) {
    const PROBE: &str = "LIELLA_S23_ZONE";
    if std::env::var(PROBE).is_ok_and(|v| v == test) {
        body();
        CSys.exit_group(0);
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact", "--test-threads=1", "--nocapture"])
        .env(PROBE, test)
        .output()
        .unwrap();
    assert!(o.status.success(), "{test} failed in its child: {}", child_report(&o));
}

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

/// A `_r` conversion's protocol: the caller's object returned and filled; NULL arguments are
/// EFAULT (S6) and an overflow EOVERFLOW, the object left alone.
fn r_form(f: extern "C" fn(Option<&i64>, Option<&mut struct_tm>) -> *mut struct_tm) {
    let mut o = tm_zero();
    let p = f(Some(&0), Some(&mut o));
    assert_eq!(p, core::ptr::from_mut(&mut o));
    assert_eq!((o.tm_year, o.tm_mday), (70, 1));
    errno::set_raw(0);
    assert!(f(None, Some(&mut o)).is_null() && errno::get() == EFAULT);
    errno::set_raw(0);
    assert!(f(Some(&0), None).is_null() && errno::get() == EFAULT);
    let mut keep = sep16();
    errno::set_raw(0);
    assert!(f(Some(&i64::MAX), Some(&mut keep)).is_null() && errno::get() == EOVERFLOW);
    assert_eq!(keep.tm_year, 73, "left alone");
}

/// `mktime`/`timelocal`: normalised in place, NULL is EFAULT.
fn mktime_form(f: extern "C" fn(Option<&mut struct_tm>) -> i64) {
    set_tz("JST-9");
    let mut t = struct_tm { tm_mday: 32, tm_year: 124, tm_isdst: -1, ..tm_zero() };
    assert_eq!(f(Some(&mut t)), 1_706_713_200, "2024-02-01 00:00 JST");
    assert_eq!((t.tm_mon, t.tm_mday, t.tm_yday, t.tm_wday, t.tm_isdst, t.tm_gmtoff), (1, 1, 31, 4, 0, 32_400));
    assert_eq!(text_at(t.tm_zone), "JST");
    errno::set_raw(0);
    assert_eq!((f(None), errno::get()), (-1, EFAULT));
    let mut big = struct_tm { tm_year: i32::MAX, tm_mon: 12, tm_mday: 1, ..tm_zero() };
    errno::set_raw(0);
    assert_eq!((f(Some(&mut big)), errno::get()), (-1, EOVERFLOW));
    assert_eq!(big.tm_mon, 12, "left alone");
}

/// The data objects: written by `tzset` as the zone says.
fn check_globals() {
    set_tz("EST5EDT,M3.2.0,M11.1.0");
    assert_eq!(tz_globals(), ("EST".into(), "EDT".into(), 18_000, 1));
    set_tz("<+0530>-5:30");
    assert_eq!(tz_globals(), ("+0530".into(), "+0530".into(), -19_800, 0));
    set_tz("Invalid//Zone");
    assert_eq!(tz_globals(), ("Invalid".into(), String::new(), 0, 0));
}

// ------------------------------------------------------------------------------ §23.7
//
// Formatting and parsing time. The test process's global locale is "C"; a named locale is
// compiled with the host's `localedef` into `target/test-locales` and opened in a re-executed
// copy of the test (`zoned`: `LOCPATH`, `TZ` and `DATEMSK` are process-wide). `getdate_err` is
// also checked from C, against the products (`conformance/c/s23_timefmt.c`).

use core::ffi::c_char;
use liella_abi::ctypes::locale_t;
use liella_boundary::posix::s07_locale as lx;

const LC_ALL_MASK: i32 = 0x1FBF;

static ZONE_UTC: &[u8] = b"UTC\0";

/// 2024-02-29 13:05:09, a Thursday, `tm_zone` "UTC".
fn feb29() -> struct_tm {
    struct_tm {
        tm_sec: 9,
        tm_min: 5,
        tm_hour: 13,
        tm_mday: 29,
        tm_mon: 1,
        tm_year: 124,
        tm_wday: 4,
        tm_yday: 59,
        tm_isdst: 0,
        tm_gmtoff: 0,
        tm_zone: ZONE_UTC.as_ptr().expose_provenance(),
    }
}

/// The reference locale `name` in `target/test-locales` (the directory), or `None` without
/// `localedef`.
fn reference_locale(name: &str) -> Option<std::path::PathBuf> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-locales");
    let path = dir.join(name);
    let (lang, charset) = name.split_once('.')?;
    if !compile_locale_atomic(&path, lang, charset, "LC_TIME") {
        return None;
    }
    Some(dir)
}

/// `LOCPATH` at `dir`: the library's `environ` is the process's, so only a zoned/single child sets it.
fn set_locpath(dir: &std::path::Path) {
    let v = std::ffi::CString::new(dir.to_str().unwrap()).unwrap();
    assert_eq!(envx::setenv(c"LOCPATH".as_ptr(), v.as_ptr(), 1), 0);
}

/// `newlocale(LC_ALL_MASK, name, 0)`.
fn open_locale(name: &str) -> locale_t {
    let n = std::ffi::CString::new(name).unwrap();
    let l = lx::newlocale(LC_ALL_MASK, n.as_ptr(), core::ptr::null_mut());
    assert!(!l.is_null(), "newlocale({name})");
    l
}

/// `LC_GLOBAL_LOCALE`.
fn global_locale() -> locale_t {
    core::ptr::without_provenance_mut(usize::MAX)
}

/// A wide string (terminated).
fn wide(s: &str) -> Vec<i32> {
    s.chars().map(|c| c as i32).chain([0]).collect()
}

fn from_wide(w: &[i32]) -> String {
    w.iter().map(|&c| char::from_u32(c as u32).unwrap()).collect()
}

type StrftimeL = extern "C" fn(*mut c_char, usize, *const c_char, Option<&struct_tm>, locale_t) -> usize;
type WcsftimeL = extern "C" fn(*mut i32, usize, *const i32, Option<&struct_tm>, locale_t) -> usize;

fn ftime_l(f: StrftimeL, fmt: &core::ffi::CStr, t: &struct_tm, l: locale_t) -> String {
    let mut b = [0u8; 128];
    let n = f(b.as_mut_ptr().cast(), b.len(), fmt.as_ptr(), Some(t), l);
    String::from_utf8(b[..n].to_vec()).unwrap()
}

#[test]
fn sym_strftime() {
    let t = feb29();
    let f = c"%Y-%m-%d %H:%M:%S %Z";
    let mut b = [b'x'; 32];
    errno::set_raw(0);
    assert_eq!(s::strftime(b.as_mut_ptr().cast(), 32, f.as_ptr(), Some(&t)), 23);
    assert_eq!(&b[..24], b"2024-02-29 13:05:09 UTC\0");
    assert_eq!(errno::get(), 0, "success leaves errno alone");
    // 0 when the text and its terminator do not fit (no terminator then), and for nothing.
    let mut b = [b'x'; 32];
    assert_eq!(s::strftime(b.as_mut_ptr().cast(), 23, f.as_ptr(), Some(&t)), 0);
    assert_eq!(&b[..20], b"2024-02-29 13:05:09 ", "the fields that fitted");
    assert_eq!(s::strftime(core::ptr::null_mut(), 0, f.as_ptr(), Some(&t)), 0);
    assert_eq!(s::strftime(b.as_mut_ptr().cast(), 1, c"".as_ptr(), Some(&t)), 0);
    assert_eq!(b[0], 0, "an empty text is terminated");
    // NULL arguments: 0 with EFAULT (S6).
    for (buf, fmt, tm) in [(b.as_mut_ptr(), f.as_ptr(), None), (b.as_mut_ptr(), core::ptr::null(), Some(&t)), (core::ptr::null_mut(), f.as_ptr(), Some(&t))] {
        errno::set_raw(0);
        assert_eq!((s::strftime(buf.cast(), 32, fmt, tm), errno::get()), (0, EFAULT));
    }
    zoned("sym_strftime", || {
        // %Z with a NULL tm_zone and %s read the process's zone, as after tzset.
        set_tz("EST5EDT,M3.2.0,M11.1.0");
        let mut n = feb29();
        n.tm_zone = 0;
        n.tm_isdst = 1;
        let mut b = [0u8; 64];
        let len = s::strftime(b.as_mut_ptr().cast(), 64, c"%Z %s".as_ptr(), Some(&n));
        assert_eq!(&b[..len], b"EDT 1709226309");
        n.tm_isdst = 0;
        let len = s::strftime(b.as_mut_ptr().cast(), 64, c"%Z".as_ptr(), Some(&n));
        assert_eq!(&b[..len], b"EST");
    });
}

fn strftime_l_form(test: &str, f: StrftimeL) {
    let t = feb29();
    let c = open_locale("C");
    assert_eq!(ftime_l(f, c"%A %B %p", &t, c), "Thursday February PM");
    assert_eq!(ftime_l(f, c"%c", &t, global_locale()), "Thu Feb 29 13:05:09 2024", "LC_GLOBAL_LOCALE");
    errno::set_raw(0);
    assert_eq!((f(core::ptr::null_mut(), 8, c"%Y".as_ptr(), Some(&t), c), errno::get()), (0, EFAULT));
    lx::freelocale(c);
    let Some(dir) = reference_locale("de_DE.UTF-8") else { return skip_note::skipped("skipped: no localedef") };
    zoned(test, || {
        set_locpath(&dir);
        let de = open_locale("de_DE.UTF-8");
        assert_eq!(ftime_l(f, c"%A, %d. %B %Y|%x|%p", &t, de), "Donnerstag, 29. Februar 2024|29.02.2024|");
        assert_eq!(ftime_l(f, c"%A", &t, global_locale()), "Thursday", "the global locale is still C");
    });
}


/// `nl_langinfo_l(item, l)` as text.
fn langinfo_l(item: i32, l: locale_t) -> String {
    let p = lx::nl_langinfo_l(item, l);
    text_at(p.expose_provenance())
}

/// Every name strftime copies equals the `nl_langinfo_l` answer for the same item, in `l`
/// (`strftime_l`) and, with `uselocale(l)`, in the calling thread's locale (`strftime`).
fn names_equal_langinfo(l: locale_t, what: &str) {
    const ABDAY_1: i32 = 0x2_0000;
    const DAY_1: i32 = 0x2_0007;
    const ABMON_1: i32 = 0x2_000E;
    const MON_1: i32 = 0x2_001A;
    const AM_STR: i32 = 0x2_0026;
    const PM_STR: i32 = 0x2_0027;
    let old = lx::uselocale(l);
    let mut checked = 0;
    for hour in [5, 13] {
        for (wday, mon) in (0..7).zip(0..12).chain((7..12).map(|m| (m % 7, m))) {
            let t = struct_tm { tm_wday: wday, tm_mon: mon, tm_hour: hour, ..feb29() };
            let want = format!(
                "{}|{}|{}|{}|{}|{}",
                langinfo_l(ABDAY_1 + wday, l),
                langinfo_l(DAY_1 + wday, l),
                langinfo_l(ABMON_1 + mon, l),
                langinfo_l(MON_1 + mon, l),
                langinfo_l(if hour >= 12 { PM_STR } else { AM_STR }, l),
                langinfo_l(ABMON_1 + mon, l),
            );
            assert_eq!(ftime_l(s::strftime_l, c"%a|%A|%b|%B|%p|%h", &t, l), want, "{what}: strftime_l wday {wday} mon {mon} hour {hour}");
            let mut b = [0u8; 128];
            let n = s::strftime(b.as_mut_ptr().cast(), b.len(), c"%a|%A|%b|%B|%p|%h".as_ptr(), Some(&t));
            assert_eq!(String::from_utf8(b[..n].to_vec()).unwrap(), want, "{what}: strftime under uselocale, wday {wday} mon {mon} hour {hour}");
            checked += 1;
        }
    }
    assert_eq!(checked, 24);
    lx::uselocale(old);
}

#[test]
fn strftime_names_follow_the_locale() {
    // Trace: no spec clause (an internal per-locale name view; the answers are those of nl_langinfo_l); D-23.12.
    let c = open_locale("C");
    names_equal_langinfo(c, "C");
    let (Some(dir), Some(_)) = (reference_locale("de_DE.UTF-8"), reference_locale("ja_JP.UTF-8")) else {
        return skip_note::skipped("skipped: no localedef");
    };
    zoned("strftime_names_follow_the_locale", || {
        set_locpath(&dir);
        let t = feb29();
        let (c, de, ja) = (open_locale("C"), open_locale("de_DE.UTF-8"), open_locale("ja_JP.UTF-8"));
        names_equal_langinfo(de, "de_DE");
        names_equal_langinfo(ja, "ja_JP");
        // A switch of the thread's locale and back is seen at once by the next call.
        for (l, want) in [(de, "Donnerstag"), (c, "Thursday"), (ja, "\u{6728}\u{66dc}\u{65e5}"), (de, "Donnerstag"), (c, "Thursday")] {
            let old = lx::uselocale(l);
            let mut b = [0u8; 64];
            let n = s::strftime(b.as_mut_ptr().cast(), b.len(), c"%A".as_ptr(), Some(&t));
            assert_eq!(core::str::from_utf8(&b[..n]).unwrap(), want);
            lx::uselocale(old);
        }
        assert_eq!(ftime_l(s::strftime_l, c"%A", &t, de), "Donnerstag");
        assert_eq!(ftime_l(s::strftime_l, c"%A", &t, ja), "\u{6728}\u{66dc}\u{65e5}");
    });
}
#[test]
fn sym___strftime_l() {
    strftime_l_form("sym___strftime_l", s::__strftime_l);
}

#[test]
fn sym_strftime_l() {
    strftime_l_form("sym_strftime_l", s::strftime_l);
}

#[test]
fn sym_wcsftime() {
    let t = feb29();
    let f = wide("%a %d %b %Y|%Z|%%");
    let mut w = [-1i32; 32];
    errno::set_raw(0);
    let n = s::wcsftime(w.as_mut_ptr(), 32, f.as_ptr(), Some(&t));
    assert_eq!((n, from_wide(&w[..n]), w[n], errno::get()), (21, "Thu 29 Feb 2024|UTC|%".to_string(), 0, 0));
    // The length counts wide characters; 0 when it and the terminator do not fit.
    assert_eq!(s::wcsftime(w.as_mut_ptr(), 21, f.as_ptr(), Some(&t)), 0);
    let lit = wide("x\u{e9}y");
    assert_eq!(s::wcsftime(w.as_mut_ptr(), 32, lit.as_ptr(), Some(&t)), 3, "non-ASCII format text is copied");
    assert_eq!(w[1], 0xe9);
    errno::set_raw(0);
    assert_eq!((s::wcsftime(w.as_mut_ptr(), 32, f.as_ptr(), None), errno::get()), (0, EFAULT));
    errno::set_raw(0);
    assert_eq!((s::wcsftime(w.as_mut_ptr(), 32, core::ptr::null(), Some(&t)), errno::get()), (0, EFAULT));
    // A buffer that cannot be a wchar_t array (misaligned) is EFAULT.
    let odd = w.as_mut_ptr().cast::<u8>().wrapping_add(1).cast::<i32>();
    errno::set_raw(0);
    assert_eq!((s::wcsftime(odd, 8, f.as_ptr(), Some(&t)), errno::get()), (0, EFAULT));
}

fn wcsftime_l_form(test: &str, f: WcsftimeL) {
    let t = feb29();
    let c = open_locale("C");
    let fmt = wide("%^A %j");
    let mut w = [0i32; 64];
    let n = f(w.as_mut_ptr(), 64, fmt.as_ptr(), Some(&t), c);
    assert_eq!(from_wide(&w[..n]), "THURSDAY 060");
    lx::freelocale(c);
    let Some(dir) = reference_locale("ja_JP.UTF-8") else { return skip_note::skipped("skipped: no localedef") };
    zoned(test, || {
        // Names, eras and alternative digits decoded to wide characters.
        set_locpath(&dir);
        let ja = open_locale("ja_JP.UTF-8");
        let fmt = wide("%EY|%Oy|%A");
        let mut w = [0i32; 64];
        let n = f(w.as_mut_ptr(), 64, fmt.as_ptr(), Some(&t), ja);
        assert_eq!(from_wide(&w[..n]), "令和06年|二十四|木曜日");
    });
}

#[test]
fn sym___wcsftime_l() {
    wcsftime_l_form("sym___wcsftime_l", s::__wcsftime_l);
}

#[test]
fn sym_wcsftime_l() {
    wcsftime_l_form("sym_wcsftime_l", s::wcsftime_l);
}

/// A `struct tm` of sentinels (-77): what `strptime` leaves alone shows.
fn sentinels() -> struct_tm {
    struct_tm { tm_sec: -77, tm_min: -77, tm_hour: -77, tm_mday: -77, tm_mon: -77, tm_year: -77, tm_wday: -77, tm_yday: -77, tm_isdst: -77, tm_gmtoff: -77, tm_zone: 0 }
}

#[test]
fn sym_strptime() {
    let input = c"2024-02-29 13:05 rest";
    let mut t = sentinels();
    errno::set_raw(0);
    let end = s::strptime(input.as_ptr(), c"%Y-%m-%d %H:%M".as_ptr(), Some(&mut t));
    assert_eq!(end.cast_const(), input.as_ptr().wrapping_add(16), "a pointer past the parsed text");
    assert_eq!((t.tm_year, t.tm_mon, t.tm_mday, t.tm_hour, t.tm_min, t.tm_wday, t.tm_yday), (124, 1, 29, 13, 5, 4, 59));
    assert_eq!((t.tm_sec, t.tm_isdst, t.tm_gmtoff, t.tm_zone), (-77, -77, -77, 0), "only what was parsed");
    // A mismatch is NULL and leaves errno alone.
    assert!(s::strptime(c"2024".as_ptr(), c"%Y-%m".as_ptr(), Some(&mut t)).is_null());
    assert_eq!(errno::get(), 0);
    // %z sets tm_gmtoff.
    let mut z = sentinels();
    assert!(!s::strptime(c"-03:30".as_ptr(), c"%z".as_ptr(), Some(&mut z)).is_null());
    assert_eq!((z.tm_gmtoff, z.tm_hour), (-12_600, -77));
    for (st, fmt, tm) in [(core::ptr::null(), c"%Y".as_ptr(), true), (c"1".as_ptr(), core::ptr::null(), true), (c"1".as_ptr(), c"%Y".as_ptr(), false)] {
        let mut x = sentinels();
        errno::set_raw(0);
        assert!(s::strptime(st, fmt, if tm { Some(&mut x) } else { None }).is_null());
        assert_eq!(errno::get(), EFAULT);
    }
    zoned("sym_strptime", || {
        // %s: every field from the local time of the instant, tm_zone included.
        set_tz("JST-9");
        let mut t = sentinels();
        assert!(!s::strptime(c"0".as_ptr(), c"%s".as_ptr(), Some(&mut t)).is_null());
        assert_eq!((t.tm_year, t.tm_hour, t.tm_isdst, t.tm_gmtoff, text_at(t.tm_zone)), (70, 9, 0, 32_400, "JST".to_string()));
    });
}

#[test]
fn sym_strptime_l() {
    let c = open_locale("C");
    let mut t = sentinels();
    assert!(!s::strptime_l(c"Thursday February".as_ptr(), c"%A %B".as_ptr(), Some(&mut t), c).is_null());
    assert_eq!((t.tm_wday, t.tm_mon, t.tm_yday), (4, 1, -77));
    lx::freelocale(c);
    errno::set_raw(0);
    assert!(s::strptime_l(c"1".as_ptr(), core::ptr::null(), Some(&mut t), global_locale()).is_null());
    assert_eq!(errno::get(), EFAULT);
    let Some(dir) = reference_locale("de_DE.UTF-8") else { return skip_note::skipped("skipped: no localedef") };
    zoned("sym_strptime_l", || {
        set_locpath(&dir);
        let de = open_locale("de_DE.UTF-8");
        let mut t = sentinels();
        let input = c"donnerstag, 29. FEBRUAR 2024";
        let end = s::strptime_l(input.as_ptr(), c"%A, %d. %B %Y".as_ptr(), Some(&mut t), de);
        assert_eq!(end.cast_const(), input.as_ptr().wrapping_add(input.to_bytes().len()));
        assert_eq!((t.tm_wday, t.tm_mday, t.tm_mon, t.tm_year, t.tm_yday), (4, 29, 1, 124, 59));
        assert!(s::strptime_l(c"Donnerstag".as_ptr(), c"%A".as_ptr(), Some(&mut sentinels()), global_locale()).is_null(), "the global locale is C");
    });
}

/// A private directory for template files, removed when dropped.
fn datemsk_dir(tag: &str) -> crate::scratch::Scratch {
    crate::scratch::Scratch::dir(&format!("s23-getdate-{tag}"))
}

fn set_datemsk(v: Option<&std::path::Path>) {
    match v {
        Some(p) => {
            let v = std::ffi::CString::new(p.to_str().unwrap()).unwrap();
            assert_eq!(envx::setenv(c"DATEMSK".as_ptr(), v.as_ptr(), 1), 0);
        }
        None => assert_eq!(envx::unsetenv(c"DATEMSK".as_ptr()), 0),
    }
}

fn getdate_err() -> i32 {
    liella_boundary::data::getdate_err.load(std::sync::atomic::Ordering::SeqCst)
}

#[test]
fn sym_getdate() {
    zoned("sym_getdate", || {
        set_tz("UTC0");
        let dir = datemsk_dir("getdate");
        let file = dir.join("templates");
        std::fs::write(&file, "%Y-%m-%d %H:%M:%S\n%d.%m.%Y\n").unwrap();
        set_datemsk(Some(&file));
        let p = s::getdate(c"  2024-02-29 13:05:09 ".as_ptr());
        assert!(!p.is_null());
        let r = tm_at(p);
        assert_eq!((r.tm_year, r.tm_mon, r.tm_mday, r.tm_hour, r.tm_min, r.tm_sec, r.tm_wday, r.tm_yday, r.tm_isdst), (124, 1, 29, 13, 5, 9, 4, 59, 0));
        assert_eq!(text_at(r.tm_zone), "UTC");
        // The second template; the result is the same static object, overwritten.
        let q = s::getdate(c"01.03.2024".as_ptr());
        assert_eq!(q, p, "static storage");
        let r = tm_at(q);
        assert_eq!((r.tm_mon, r.tm_mday, r.tm_wday), (2, 1, 5));
        // Failures: NULL with getdate_err.
        for (input, err) in [("2023-02-29 00:00:00", 8), ("31.04.2024", 8), ("not a date", 7), ("", 7)] {
            let c = std::ffi::CString::new(input).unwrap();
            assert!(s::getdate(c.as_ptr()).is_null());
            assert_eq!(getdate_err(), err, "{input:?}");
        }
        set_datemsk(None);
        assert!(s::getdate(c"2024-02-29 13:05:09".as_ptr()).is_null());
        assert_eq!(getdate_err(), 1);
        set_datemsk(Some(&dir.join("missing")));
        assert!(s::getdate(c"2024-02-29 13:05:09".as_ptr()).is_null());
        assert_eq!(getdate_err(), 2, "POSIX: cannot be opened");
        set_datemsk(Some(&dir));
        assert!(s::getdate(c"2024-02-29 13:05:09".as_ptr()).is_null());
        assert_eq!(getdate_err(), 4, "not a regular file");
        assert!(s::getdate(core::ptr::null()).is_null());
        assert_eq!(getdate_err(), 8, "NULL input");
    });
}

#[test]
fn sym_getdate_r() {
    zoned("sym_getdate_r", || {
        set_tz("UTC0");
        let dir = datemsk_dir("getdate_r");
        let file = dir.join("templates");
        std::fs::write(&file, "%Y-%m-%d %H:%M\n").unwrap();
        set_datemsk(Some(&file));
        let mut r = tm_zero();
        assert_eq!(s::getdate_r(c"2024-02-29 13:05".as_ptr(), Some(&mut r)), 0);
        assert_eq!((r.tm_year, r.tm_mday, r.tm_hour, r.tm_min, r.tm_sec, r.tm_wday), (124, 29, 13, 5, 0, 4));
        // Errors are returned; getdate_err and the result are left alone.
        liella_boundary::data::getdate_err.store(0, std::sync::atomic::Ordering::SeqCst);
        let mut keep = tm_zero();
        assert_eq!(s::getdate_r(c"2024-02-30 00:00".as_ptr(), Some(&mut keep)), 8);
        assert_eq!(s::getdate_r(c"later".as_ptr(), Some(&mut keep)), 7);
        assert_eq!((keep.tm_year, getdate_err()), (0, 0));
        assert_eq!(s::getdate_r(c"2024-02-29 13:05".as_ptr(), None), 8, "no result object");
        set_datemsk(None);
        assert_eq!(s::getdate_r(c"2024-02-29 13:05".as_ptr(), Some(&mut keep)), 1);
    });
}

#[test]
fn sym_getdate_err() {
    zoned("sym_getdate_err", || {
        set_tz("UTC0");
        set_datemsk(None);
        liella_boundary::data::getdate_err.store(0, std::sync::atomic::Ordering::SeqCst);
        assert!(s::getdate(c"2024".as_ptr()).is_null());
        assert_eq!(getdate_err(), 1, "written by a failing getdate");
        let dir = datemsk_dir("getdate_err");
        let file = dir.join("templates");
        std::fs::write(&file, "%Y\n").unwrap();
        set_datemsk(Some(&file));
        assert!(!s::getdate(c"2024".as_ptr()).is_null());
        assert_eq!(getdate_err(), 1, "a success leaves it");
    });
    conformance(&["s23_timefmt"]);
}

/// Compile locale `lang`.`charset` into `dest` with the host's `localedef`, atomically: it is built
/// in a private sibling directory and renamed into place, so that concurrent tests and test
/// binaries never see (or write into) a half-built locale. True when `dest/<probe>` exists after.
fn compile_locale_atomic(dest: &std::path::Path, lang: &str, charset: &str, probe: &str) -> bool {
    static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    if dest.join(probe).exists() {
        return true;
    }
    let Some(parent) = dest.parent() else { return false };
    let _ = std::fs::create_dir_all(parent);
    let tmp = parent.join(format!(".build-{}-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::Relaxed), dest.file_name().unwrap().to_string_lossy()));
    let _ = std::fs::remove_dir_all(&tmp);
    let made = std::process::Command::new("localedef").args(["-i", lang, "-f", charset]).arg(&tmp).output().is_ok_and(|o| o.status.code().is_some_and(|c| c <= 1));
    if made && tmp.join(probe).exists() {
        // Losing the race (the destination appeared meanwhile) is fine: theirs is complete too.
        let _ = std::fs::rename(&tmp, dest);
    }
    let _ = std::fs::remove_dir_all(&tmp);
    dest.join(probe).exists()
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
