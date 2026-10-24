//! The C shapes of spec §23.1–23.3 that add something to the kernel call: the older clock
//! readings as translations of `clock_gettime` (D-23.1: `time`, `gettimeofday`, `ftime`,
//! `clock`), C11's `timespec_get`, the process CPU clock of another process, `settimeofday`'s
//! time-zone rule, `stime`, `adjtime`'s slew through `clock_adjtime`, the NTP readings, the
//! sleeps (`sleep`'s remainder, `usleep`, the CPU clocks of `clock_nanosleep`), and `ualarm`
//! (§23.4) as a translation of `ITIMER_REAL`.

use liella::error::{Errno, Result};
use liella_abi::layout::{struct_ntptimeval, struct_timeb, struct_timeval};
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Itimerval, Timespec, Timeval, Timex, Timezone, Tms, CLOCK_MONOTONIC, CLOCK_PROCESS_CPUTIME_ID, CLOCK_REALTIME, CLOCK_THREAD_CPUTIME_ID, ITIMER_REAL};

/// `CLOCKS_PER_SEC` (XSI fixes it at one million).
pub const CLOCKS_PER_SEC: i64 = 1_000_000;

/// C11's `TIME_UTC`, the only `timespec_get` base glibc 2.39 supports.
pub const TIME_UTC: i32 = 1;

/// `time()` (§23.1): the seconds of `CLOCK_REALTIME`, through the vDSO's `time` entry as glibc
/// reads it (whole seconds as of the last tick, a shared-variable read; D-23.1).
pub fn time<S: Sys>(sys: &S) -> Result<i64> {
    sys.time()
}

/// `gettimeofday()` (§23.1): `CLOCK_REALTIME` in microseconds, and the obsolete time zone
/// zero-filled (D-23.1).
pub fn gettimeofday<S: Sys>(sys: &S) -> Result<(Timeval, Timezone)> {
    let t = sys.clock_gettime(CLOCK_REALTIME)?;
    Ok((Timeval { sec: t.sec, usec: t.nsec / 1000 }, Timezone { minuteswest: 0, dsttime: 0 }))
}

/// `ftime()` (§23.1, compat): `CLOCK_REALTIME` in milliseconds; the zone fields are zero.
pub fn ftime<S: Sys>(sys: &S) -> Result<struct_timeb> {
    let t = sys.clock_gettime(CLOCK_REALTIME)?;
    Ok(struct_timeb { time: t.sec, millitm: u16::try_from(t.nsec / 1_000_000).unwrap_or(0), timezone: 0, dstflag: 0 })
}

/// `clock()` (§23.1): the process CPU time in `CLOCKS_PER_SEC` units, or -1 when unavailable.
/// The value wraps as glibc's does (⚑ HIST).
pub fn clock<S: Sys>(sys: &S) -> i64 {
    sys.clock_gettime(CLOCK_PROCESS_CPUTIME_ID)
        .map_or(-1, |t| t.sec.wrapping_mul(CLOCKS_PER_SEC).wrapping_add(t.nsec / (1_000_000_000 / CLOCKS_PER_SEC)))
}

/// `timespec_get(ts, base)` (§23.1, C11): `base` and the time for `TIME_UTC`, 0 for any
/// other base.
pub fn timespec_get<S: Sys>(sys: &S, base: i32) -> (i32, Option<Timespec>) {
    if base != TIME_UTC {
        return (0, None);
    }
    match sys.clock_gettime(CLOCK_REALTIME) {
        Ok(t) => (base, Some(t)),
        Err(_) => (0, None),
    }
}

/// `timespec_getres(ts, base)` (§23.1, C23): as [`timespec_get`] with the resolution.
pub fn timespec_getres<S: Sys>(sys: &S, base: i32) -> (i32, Option<Timespec>) {
    if base != TIME_UTC {
        return (0, None);
    }
    match sys.clock_getres(CLOCK_REALTIME) {
        Ok(t) => (base, Some(t)),
        Err(_) => (0, None),
    }
}

/// `CPUCLOCK_SCHED`: the per-process CPU clock the kernel encodes in a clock ID.
const CPUCLOCK_SCHED: i32 = 2;

/// The kernel's clock ID for process `pid`'s CPU time (`MAKE_PROCESS_CPUCLOCK`).
fn process_cpuclock(pid: i32) -> i32 {
    ((!pid) << 3) | CPUCLOCK_SCHED
}

/// `clock_getcpuclockid(pid)` (§23.1): the CPU-time clock of `pid`, checked by asking the
/// kernel for its resolution; a process that does not exist is `ESRCH` (glibc's mapping of the
/// kernel's `EINVAL`).
pub fn clock_getcpuclockid<S: Sys>(sys: &S, pid: i32) -> Result<i32> {
    let id = process_cpuclock(pid);
    match sys.clock_getres(id) {
        Ok(_) => Ok(id),
        Err(Errno::EINVAL) => Err(Errno::ESRCH),
        Err(e) => Err(e),
    }
}

/// `settimeofday(tv, tz)` (§23.2): a time zone with a time is `EINVAL` (2.31+); a time zone
/// alone is the kernel's special call (warping the clock to the zone at boot); otherwise the
/// time is set through `clock_settime`.
pub fn settimeofday<S: Sys>(sys: &S, tv: Option<&Timeval>, tz: Option<&Timezone>) -> Result<()> {
    match (tv, tz) {
        (Some(_), Some(_)) => Err(Errno::EINVAL),
        (None, Some(z)) => sys.settimeofday(None, Some(z)),
        (Some(t), None) => {
            if !(0..1_000_000).contains(&t.usec) {
                return Err(Errno::EINVAL);
            }
            sys.clock_settime(CLOCK_REALTIME, &Timespec { sec: t.sec, nsec: t.usec.saturating_mul(1000) })
        }
        (None, None) => Err(Errno::EFAULT),
    }
}

/// `stime(t)` (§23.2, compat): set `CLOCK_REALTIME` to whole seconds.
pub fn stime<S: Sys>(sys: &S, t: i64) -> Result<()> {
    sys.clock_settime(CLOCK_REALTIME, &Timespec { sec: t, nsec: 0 })
}

/// `ADJ_OFFSET_SINGLESHOT` and `ADJ_OFFSET_SS_READ`: `adjtime`'s modes of `adjtimex`.
const ADJ_OFFSET_SINGLESHOT: u32 = 0x0001 | 0x8000;
const ADJ_OFFSET_SS_READ: u32 = 0xa001;

/// `adjtime(delta, old)` (§23.2): slew `CLOCK_REALTIME` by `delta` (or only read the slew
/// still pending), and return the pending slew before the call. glibc's range check: the
/// delta in microseconds must fit the kernel's `int`-based offset.
pub fn adjtime<S: Sys>(sys: &S, delta: Option<&Timeval>) -> Result<Timeval> {
    // INT_MAX / 1000000 - 2 and INT_MIN / 1000000 + 2.
    const MAX_SEC: i64 = 2145;
    const MIN_SEC: i64 = -2145;
    let mut tx = Timex::default();
    match delta {
        Some(d) => {
            let sec = d.sec.checked_add(d.usec / 1_000_000).ok_or(Errno::EINVAL)?;
            let usec = d.usec % 1_000_000;
            if !(MIN_SEC..=MAX_SEC).contains(&sec) {
                return Err(Errno::EINVAL);
            }
            tx.modes = ADJ_OFFSET_SINGLESHOT;
            // In range by the check above.
            tx.offset = sec.saturating_mul(1_000_000).saturating_add(usec);
        }
        None => tx.modes = ADJ_OFFSET_SS_READ,
    }
    sys.clock_adjtime(CLOCK_REALTIME, &mut tx)?;
    // Truncation toward zero on both fields, as glibc splits a negative offset.
    Ok(Timeval { sec: tx.offset / 1_000_000, usec: tx.offset % 1_000_000 })
}

/// The old `struct ntptimeval` (`ntp_gettime`'s, before 2.12 added `tai`): only these three
/// fields are written, since programs built before 2.12 have only these.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct NtpTimevalOld {
    /// The current time (microseconds, or nanoseconds with `STA_NANO`).
    pub time: Timeval,
    /// Maximum error (µs).
    pub maxerror: i64,
    /// Estimated error (µs).
    pub esterror: i64,
}

/// Reads the kernel's NTP state (`adjtimex` with no modes): the clock state and the record.
fn ntp_read<S: Sys>(sys: &S) -> Result<(i32, Timex)> {
    let mut tx = Timex::default();
    let state = sys.clock_adjtime(CLOCK_REALTIME, &mut tx)?;
    Ok((state, tx))
}

/// `ntp_gettime(ntv)` (§23.2): the clock state, and the time with its error estimates.
pub fn ntp_gettime<S: Sys>(sys: &S) -> Result<(i32, NtpTimevalOld)> {
    let (state, tx) = ntp_read(sys)?;
    Ok((state, NtpTimevalOld { time: tx.time, maxerror: tx.maxerror, esterror: tx.esterror }))
}

/// `ntp_gettimex(ntv)` (§23.2, 2.12): as [`ntp_gettime`] with the TAI offset; the reserved
/// fields are zeroed.
pub fn ntp_gettimex<S: Sys>(sys: &S) -> Result<(i32, struct_ntptimeval)> {
    let (state, tx) = ntp_read(sys)?;
    Ok((
        state,
        struct_ntptimeval {
            time: struct_timeval { tv_sec: tx.time.sec, tv_usec: tx.time.usec },
            maxerror: tx.maxerror,
            esterror: tx.esterror,
            tai: i64::from(tx.tai),
            __glibc_reserved1: 0,
            __glibc_reserved2: 0,
            __glibc_reserved3: 0,
            __glibc_reserved4: 0,
        },
    ))
}

/// `times(buf)` (§23.1): the elapsed ticks from an arbitrary origin, and the process's and
/// its waited-for children's CPU times in ticks.
pub fn times<S: Sys>(sys: &S) -> Result<(i64, Tms)> {
    let mut t = Tms { utime: 0, stime: 0, cutime: 0, cstime: 0 };
    let ticks = sys.times(&mut t)?;
    Ok((i64::from_ne_bytes(ticks.to_ne_bytes()), t))
}

/// `nanosleep(req, rem)` (§23.3): a relative sleep on `CLOCK_MONOTONIC`; interrupted by a
/// handler, `EINTR` with the remainder in `rem`.
pub fn nanosleep<S: Sys>(sys: &S, req: &Timespec, rem: Option<&mut Timespec>) -> Result<()> {
    sys.clock_nanosleep(CLOCK_MONOTONIC, 0, req, rem)
}

/// `clock_nanosleep(clock, flags, req, rem)` (§23.3): the thread CPU clock cannot be slept on
/// (`EINVAL`), and the process CPU clock is the kernel's per-process CPU clock of the caller.
pub fn clock_nanosleep<S: Sys>(sys: &S, clock: i32, flags: i32, req: &Timespec, rem: Option<&mut Timespec>) -> Result<()> {
    let clock = match clock {
        CLOCK_THREAD_CPUTIME_ID => return Err(Errno::EINVAL),
        CLOCK_PROCESS_CPUTIME_ID => process_cpuclock(0),
        c => c,
    };
    let flags = u32::try_from(flags).map_err(|_| Errno::EINVAL)?;
    sys.clock_nanosleep(clock, flags, req, rem)
}

/// `sleep(seconds)` (§23.3): the unslept whole seconds (0 when complete). An interrupted sleep
/// returns what remained, as glibc does (truncated).
pub fn sleep<S: Sys>(sys: &S, seconds: u32) -> u32 {
    let req = Timespec { sec: i64::from(seconds), nsec: 0 };
    let mut rem = Timespec::default();
    match nanosleep(sys, &req, Some(&mut rem)) {
        Ok(()) => 0,
        Err(_) => u32::try_from(rem.sec).unwrap_or(seconds),
    }
}

/// `usleep(us)` (§23.3): a sleep of `us` microseconds (glibc accepts a million and more).
pub fn usleep<S: Sys>(sys: &S, us: u32) -> Result<()> {
    let req = Timespec { sec: i64::from(us / 1_000_000), nsec: i64::from(us % 1_000_000).saturating_mul(1000) };
    nanosleep(sys, &req, None)
}

/// The largest `ualarm` result that is not its error value (`(useconds_t)-1`).
pub const UALARM_MAX: u32 = u32::MAX - 1;

/// `ualarm(value, interval)` (§23.4): sets `ITIMER_REAL`, the timer `alarm` sets (⚑ KERN), to
/// `value` microseconds and its period to `interval`, and returns the microseconds that remained
/// on the previous setting. Either argument of a second or more is `EINVAL` (POSIX's limit, also
/// glibc's). A remainder that does not fit `useconds_t` is reported as [`UALARM_MAX`]: glibc
/// returns it modulo 2^32 (a deliberate difference).
pub fn ualarm<S: Sys>(sys: &S, value: u32, interval: u32) -> Result<u32> {
    if value >= 1_000_000 || interval >= 1_000_000 {
        return Err(Errno::EINVAL);
    }
    let new = Itimerval { interval: Timeval { sec: 0, usec: i64::from(interval) }, value: Timeval { sec: 0, usec: i64::from(value) } };
    let old = sys.setitimer(ITIMER_REAL.unsigned_abs(), &new)?;
    let us = old.value.sec.saturating_mul(1_000_000).saturating_add(old.value.usec);
    Ok(u32::try_from(us).map_or(UALARM_MAX, |v| v.min(UALARM_MAX)))
}
