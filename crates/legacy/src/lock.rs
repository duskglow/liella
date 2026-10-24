//! A minimal spin lock for the contract-visible global states of this crate (`rand`, `random`,
//! `drand48`). Critical sections are a few dozen instructions with no system calls, so
//! spinning is adequate; phase 3's `rt::sync` primitives will replace it.

use core::sync::atomic::{AtomicBool, Ordering};

/// A spin lock that protects atomics-only state (S9: no data outside atomics).
pub struct SpinLock {
    held: AtomicBool,
}

/// Holds the lock until dropped.
pub struct Guard<'a>(&'a SpinLock);

impl SpinLock {
    /// Unlocked.
    pub const fn new() -> SpinLock {
        SpinLock { held: AtomicBool::new(false) }
    }

    /// Acquire.
    pub fn lock(&self) -> Guard<'_> {
        while self.held.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
        Guard(self)
    }
}

impl Default for SpinLock {
    fn default() -> Self {
        SpinLock::new()
    }
}

impl Drop for Guard<'_> {
    fn drop(&mut self) {
        self.0.held.store(false, Ordering::Release);
    }
}
