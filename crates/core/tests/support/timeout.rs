//! Load-tolerant watchdog limits for the test harnesses (task FL-9).
//!
//! A fixed limit sized for an idle machine kills a child that is only slow when the gate shares the
//! machine (16 test threads on few CPUs, a second gate on the same host). `scale()` multiplies every
//! limit by a factor that grows with the load, so a slow child lives and a hung one still dies, at
//! most `MAX` times later than on an idle machine. Same formula as `oracle/tscale.h` and
//! `oracle/tscale.sh`; keep them in step:
//!
//! ```text
//! pressure = max(load1, running tasks - 1)      (hundredths of a task)
//! scale    = clamp(1 + ceil(pressure / cpus), 2, 10)
//! ```
//!
//! `LIELLA_TEST_TIMEOUT_SCALE=<n>` (1..=100) overrides it. On expiry the report says which limit
//! expired, the scale and the elapsed time (`expired`).
#![allow(dead_code, clippy::arithmetic_side_effects)]

use std::time::Duration;

const MIN: u64 = 2;
const MAX: u64 = 10;

/// The scale for `/proc/loadavg` contents `loadavg` on a machine with `cpus` CPUs.
pub(crate) fn scale_from(loadavg: &str, cpus: u64) -> u64 {
    let mut f = loadavg.split_whitespace();
    let load1 = f.next().unwrap_or("0");
    let (whole, frac) = load1.split_once('.').unwrap_or((load1, "0"));
    let frac = format!("{frac}00");
    let mut pressure = whole.parse::<u64>().unwrap_or(0) * 100 + frac[..2].parse::<u64>().unwrap_or(0);
    if let Some(run) = f.nth(2).and_then(|rt| rt.split('/').next()).and_then(|r| r.parse::<u64>().ok()) {
        pressure = pressure.max(run.saturating_sub(1) * 100);
    }
    let cpus = cpus.max(1);
    (1 + pressure.div_ceil(100 * cpus)).clamp(MIN, MAX)
}

/// The scale now: the override, else from the load. Not cached: the load changes over a long run.
pub(crate) fn scale() -> u64 {
    if let Some(n) = std::env::var("LIELLA_TEST_TIMEOUT_SCALE").ok().and_then(|v| v.parse::<u64>().ok()) {
        return n.clamp(1, 100);
    }
    let cpus = std::thread::available_parallelism().map_or(1, |n| u64::try_from(n.get()).unwrap_or(1));
    scale_from(&std::fs::read_to_string("/proc/loadavg").unwrap_or_default(), cpus)
}

/// `base` seconds scaled, as a string for `timeout N ...` command lines.
pub(crate) fn secs(base: u64) -> String {
    (base * scale()).to_string()
}

/// `base` seconds scaled, as a `Duration`.
pub(crate) fn duration(base: u64) -> Duration {
    Duration::from_secs(base * scale())
}

/// The explanation to put in a failure message when a watchdog of `base` seconds expired after `elapsed`.
pub(crate) fn expired(what: &str, base: u64, elapsed: Duration) -> String {
    expired_at(what, base, scale(), elapsed)
}

/// Like `expired`, for a limit computed with the scale `s` (a harness that fixed the scale before it started the child
/// must report that one: the load, and so `scale()`, may have changed since).
pub(crate) fn expired_at(what: &str, base: u64, s: u64, elapsed: Duration) -> String {
    format!("{what}: the watchdog expired after {:.1} s (limit {base} s x load scale {s} = {} s)", elapsed.as_secs_f64(), base * s)
}

/// The shared helper for tests with a wall-clock bound (task FL-13). A bound written as `elapsed < N s` flakes
/// when the machine is loaded (a gate at load 20 failed a 120 s bound whose usual time is 63-74 s). Write the bound
/// as its idle-machine value `base` and use these instead of a bare `Duration`:
/// * `assert_within(what, base, elapsed)`: after the work, fail if `elapsed` exceeds `base x scale()`; the message
///   names the bound, the scale and the measured time. For a bound that is a performance claim ("polynomial time"),
///   keep `base` generous: the scale only absorbs load, not a slow algorithm.
/// * `deadline(base)`: the `Instant` at `base x scale()` from now, for polling loops and `recv_timeout`-style waits
///   (`limit(base)` is the matching `Duration`).
///
/// A bound that can be an instruction or iteration budget should be one (no clock at all); this is for the rest.
pub(crate) fn assert_within(what: &str, base: Duration, elapsed: Duration) {
    assert_within_at(what, base, scale(), elapsed);
}

/// `assert_within` with the scale `s` given (testable; a harness may fix the scale before the work starts).
pub(crate) fn assert_within_at(what: &str, base: Duration, s: u64, elapsed: Duration) {
    let limit = base.saturating_mul(u32::try_from(s).unwrap_or(u32::MAX));
    assert!(
        elapsed <= limit,
        "{what}: took {:.2} s, over the bound {:.2} s (idle-machine bound {:.2} s x load scale {s})",
        elapsed.as_secs_f64(),
        limit.as_secs_f64(),
        base.as_secs_f64()
    );
}

/// `base` scaled by the load, as a `Duration` (see `duration` for whole seconds).
pub(crate) fn limit(base: Duration) -> Duration {
    base.saturating_mul(u32::try_from(scale()).unwrap_or(u32::MAX))
}

/// The instant `limit(base)` from now.
pub(crate) fn deadline(base: Duration) -> std::time::Instant {
    std::time::Instant::now().checked_add(limit(base)).unwrap_or_else(std::time::Instant::now)
}
