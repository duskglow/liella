//! Test-only allocation-failure injection (ALLOC-FAIL-INJECT, coordinator #2020; no spec clause:
//! a test facility, not library behaviour).
//!
//! Compiled only with the `selftest` feature, which no product enables: the shipped libraries
//! carry no hook code, no symbol and read no environment variable (the products are
//! byte-identical with and without this module). A test arms the calling thread's plan, calls the
//! function under test and disarms it; while the plan is armed, the allocations it selects fail
//! exactly as exhausted memory does, in both allocation domains:
//!
//! * the internal heap (`liella::mem`'s fallible collections, and any other `try_reserve`): the
//!   test binary installs [`FailAlloc`] as its global allocator, which returns null for a selected
//!   allocation, so the collection reports `ENOMEM` through its ordinary failure path;
//! * the caller-owned `malloc` family (`crate::caller_alloc`): NULL with `errno` `ENOMEM`.
//!
//! Every allocation the calling thread makes while armed counts, the test's own included (arm just
//! around the call under test). Only allocations that reach an allocator count: a `reserve` that
//! spare capacity covers is not one; a `realloc` that shrinks is never failed (Rust's infallible
//! `shrink_to_fit` would abort on it). The plan is per thread, so tests running in parallel do not
//! see each other's plans.
//!
//! ```ignore
//! #[global_allocator]
//! static ALLOC: liella_boundary::selftest::failinject::FailAlloc = liella_boundary::selftest::failinject::FailAlloc;
//!
//! failinject::fail_nth(2); // the second allocation from now fails, once
//! assert_eq!(code_under_test(), Err(Errno::ENOMEM));
//! failinject::off();
//! ```

extern crate std;

use core::cell::Cell;

/// What the calling thread's next allocations do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Plan {
    /// Every allocation succeeds (when memory allows).
    Off,
    /// The allocation this many allocations from now fails (1: the next), then [`Plan::Off`].
    Nth(u64),
    /// Every allocation of at least this many bytes fails until [`off`].
    AtLeast(usize),
}

// Constant-initialised `Copy` cells without destructors: reading them never allocates, so the
// global allocator may consult them.
std::thread_local! {
    static PLAN: Cell<Plan> = const { Cell::new(Plan::Off) };
    static INJECTED: Cell<u64> = const { Cell::new(0) };
}

/// Fail the `n`-th allocation of the calling thread from now (1: the next one), once; the
/// allocations before it and after it succeed. `n == 0` is [`off`].
pub fn fail_nth(n: u64) {
    PLAN.with(|p| p.set(if n == 0 { Plan::Off } else { Plan::Nth(n) }));
}

/// Fail every allocation of at least `bytes` bytes on the calling thread until [`off`].
pub fn fail_at_least(bytes: usize) {
    PLAN.with(|p| p.set(Plan::AtLeast(bytes)));
}

/// Disarm the calling thread's plan: allocations succeed again.
pub fn off() {
    PLAN.with(|p| p.set(Plan::Off));
}

/// How many allocations the calling thread's plans have failed so far (a test checks the hook
/// reached the code it means to test).
pub fn injected() -> u64 {
    INJECTED.try_with(Cell::get).unwrap_or(0)
}

/// Whether the calling thread's allocation of `bytes` bytes, about to be made, must fail; counts
/// it towards [`fail_nth`]'s plan. Called by the allocation paths only.
pub fn should_fail(bytes: usize) -> bool {
    let fail = PLAN
        .try_with(|p| match p.get() {
            Plan::Off => false,
            Plan::Nth(1) => {
                p.set(Plan::Off);
                true
            }
            Plan::Nth(n) => {
                p.set(Plan::Nth(n.saturating_sub(1)));
                false
            }
            Plan::AtLeast(k) => bytes >= k,
        })
        .unwrap_or(false);
    if fail {
        let _ = INJECTED.try_with(|c| c.set(c.get().saturating_add(1)));
    }
    fail
}

/// The host allocator with the calling thread's plan in front of it: a test binary declares it
/// as its `#[global_allocator]` to reach the internal heap's failure paths. It lives in
/// `crate::heap` (the allocator category's file, under `selftest` only).
pub use crate::heap::FailAlloc;
