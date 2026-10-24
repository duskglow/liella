//! How often a kept file is checked against the file system (owner ruling 2026-10-05; D-7.12,
//! D-23.6): at most once per second per file, timed with the coarse monotonic clock (the vDSO's,
//! P3), so a replaced file is seen up to one second late. No watch of any kind. The first use in a
//! process always checks (the file is read then), and the callers check at once, whatever the
//! time, after the explicit resets and setting changes the ruling lists (`tzset`, `setlocale`,
//! a changed `textdomain`, `bindtextdomain` or `bind_textdomain_codeset` value (not the same value
//! again: owner ruling 2026-10-23, #1056), a changed `TZ` (not `LANGUAGE`:
//! owner ruling 2026-10-07, D-7.12), the invalidate calls). The time of the last check is one atomic word kept with the file's record;
//! reading and writing it takes no lock.

use core::sync::atomic::{AtomicU64, Ordering::Relaxed};

use liella_kernel::sys::Sys;
use liella_kernel::uapi::CLOCK_MONOTONIC_COARSE;

/// The shortest time between two checks of one file (nanoseconds).
pub const INTERVAL_NS: u64 = 1_000_000_000;

/// The monotonic clock in nanoseconds, never 0 (0 marks "never checked"): the coarse clock, a
/// vDSO read of the last tick (a few nanoseconds, where the precise clock costs tens), ample for
/// a one-second interval.
pub fn now<S: Sys>(sys: &S) -> u64 {
    let t = sys.clock_gettime(CLOCK_MONOTONIC_COARSE).unwrap_or_default();
    let ns = u64::try_from(t.sec).unwrap_or(0).saturating_mul(1_000_000_000).saturating_add(u64::try_from(t.nsec).unwrap_or(0));
    #[cfg(feature = "selftest")]
    let ns = ns.saturating_add(SKEW_NS.load(Relaxed));
    ns.max(1)
}

/// Test hook (`selftest` builds only, never a product): nanoseconds added to [`now`], so a test
/// passes the interval without sleeping. A test that moves it also turns the TSC gate off
/// (`PR_SET_TSC`), which trusts the counter, not this clock.
#[cfg(feature = "selftest")]
pub static SKEW_NS: AtomicU64 = AtomicU64::new(0);

/// Whether a file last checked at `last` (0: never) is due for a check at `now`.
pub fn due(last: u64, now: u64) -> bool {
    last == 0 || now.saturating_sub(last) >= INTERVAL_NS || now < last
}

/// The longest TSC span the gate trusts (D-7.12, GETTEXT-6): 2^28 ticks are under one second for any
/// TSC above 268 MHz.
pub const TSC_SPAN: u64 = 1 << 28;

/// The TSC ticks during which a state last checked at `oldest` (a [`now`] value) still holds, seen at
/// the clock read `at`: the time left of the interval, as ticks at the slowest trusted rate
/// (2^28 ticks per second; 274/1024 of a tick per nanosecond is below that rate), capped at
/// [`TSC_SPAN`]. 0: no gate.
pub fn tsc_allow(oldest: u64, at: u64) -> u64 {
    if oldest == 0 || at < oldest {
        return 0;
    }
    let left = INTERVAL_NS.saturating_sub(at.saturating_sub(oldest));
    (left.saturating_mul(274) >> 10).min(TSC_SPAN)
}

/// Whether the state still holds `tsc` after the counter read `stamp` when `allow` ticks were left
/// ([`tsc_allow`]) with no clock read. A stamp or counter of 0 (no counter), a counter that went
/// backwards or wrapped, and a jump of `allow` or more all say no, and the caller reads the clock.
pub fn tsc_holds(stamp: u64, allow: u64, tsc: u64) -> bool {
    stamp != 0 && tsc >= stamp && tsc.saturating_sub(stamp) < allow
}

/// The last check of one file.
#[derive(Debug, Default)]
pub struct LastCheck(AtomicU64);

impl LastCheck {
    /// Never checked.
    pub const fn new() -> Self {
        LastCheck(AtomicU64::new(0))
    }

    /// Whether a check is due at `now`.
    pub fn due(&self, now: u64) -> bool {
        due(self.0.load(Relaxed), now)
    }

    /// A check was made at `now`.
    pub fn checked(&self, now: u64) {
        self.0.store(now, Relaxed);
    }

    /// Forget the last check: the next use checks (an explicit reset).
    pub fn forget(&self) {
        self.0.store(0, Relaxed);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::arithmetic_side_effects)]
mod tests {
    use super::*;

    #[test]
    fn tsc_gate_cases() {
        let allow = tsc_allow(1_000, 1_000 + 100);
        assert!(allow > 0 && allow <= TSC_SPAN);
        // Fresh, then just inside and at the edge.
        assert!(tsc_holds(50, allow, 50));
        assert!(tsc_holds(50, allow, 50 + allow - 1));
        assert!(!tsc_holds(50, allow, 50 + allow));
        // Backwards, wrapped, a huge jump, no counter: the clock is read.
        assert!(!tsc_holds(50, allow, 49));
        assert!(!tsc_holds(u64::MAX - 5, allow, 5));
        assert!(!tsc_holds(50, allow, u64::MAX));
        assert!(!tsc_holds(50, allow, 50 + TSC_SPAN * 4));
        assert!(!tsc_holds(0, allow, 10));
        assert!(!tsc_holds(50, allow, 0));
        // The allowance is the time left, never above the span, never past the interval.
        assert_eq!(tsc_allow(1, 1 + INTERVAL_NS), 0);
        assert_eq!(tsc_allow(0, 5), 0);
        assert_eq!(tsc_allow(10, 5), 0);
        assert!(tsc_allow(10, 10) <= TSC_SPAN);
        // 2^28 ticks per second is the slowest rate trusted: the allowance in time at that rate
        // never exceeds what is left.
        for age in [0u64, 1, 1_000, 500_000_000, 999_999_999] {
            let ticks = tsc_allow(10, 10 + age);
            let ns_at_slowest = u128::from(ticks) * 1_000_000_000 / (1 << 28);
            assert!(ns_at_slowest <= u128::from(INTERVAL_NS - age), "age {age}");
        }
    }

    #[test]
    fn once_per_interval() {
        assert!(due(0, 5));
        assert!(!due(10, 10));
        assert!(!due(10, 10 + INTERVAL_NS - 1));
        assert!(due(10, 10 + INTERVAL_NS));
        // A clock that went backwards (a value from another process image) checks.
        assert!(due(10 + INTERVAL_NS, 10));
        let c = LastCheck::new();
        assert!(c.due(1));
        c.checked(1);
        assert!(!c.due(2));
        c.forget();
        assert!(c.due(2));
    }
}
