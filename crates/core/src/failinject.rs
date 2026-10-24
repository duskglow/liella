//! Test-only allocation-failure injection (ALLOC-FAIL-INJECT, coordinator #2020; no spec clause:
//! a test facility, not library behaviour).
//!
//! Compiled only with the `selftest` feature, which no product enables: the shipped libraries
//! carry no hook code, no symbol and read no environment variable (`failinject_products` checks
//! the products). A test arms the calling thread's plan, calls the function under test and
//! disarms it; while the plan is armed, the allocations it selects fail exactly as exhausted
//! memory does: [`crate::mem`]'s fallible collections return `ENOMEM`, and the caller-owned
//! domain (`liella_boundary::caller_alloc`) returns NULL with `errno` set to `ENOMEM`.
//!
//! Only allocations that would really reach the allocator count (a `TryVec::reserve` that its
//! spare capacity already covers is not one). The plan is per thread, so tests running in
//! parallel do not see each other's plans.
//!
//! ```ignore
//! liella::failinject::fail_nth(2);   // the second allocation from now fails, once
//! assert_eq!(code_under_test(), Err(Errno::ENOMEM));
//! liella::failinject::off();
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
    INJECTED.with(Cell::get)
}

/// Whether the calling thread's allocation of `bytes` bytes, about to be made, must fail; counts
/// it towards [`fail_nth`]'s plan. Called by the allocation paths only.
#[inline(never)]
pub fn should_fail(bytes: usize) -> bool {
    let fail = PLAN.with(|p| match p.get() {
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
    });
    if fail {
        INJECTED.with(|c| c.set(c.get().saturating_add(1)));
    }
    fail
}

/// [`should_fail`] for a vector holding `len` of `cap` elements of `elem` bytes that is asked
/// for room for `more`: only a growth that reaches the allocator counts; its size is the whole
/// array's.
#[inline(never)]
pub fn grow_fails(cap: usize, len: usize, more: usize, elem: usize) -> bool {
    if elem == 0 || cap.saturating_sub(len) >= more {
        return false;
    }
    should_fail(len.saturating_add(more).saturating_mul(elem))
}
