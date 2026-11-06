//! Test-only allocation-failure injection (ALLOC-FAIL-INJECT, coordinator #2020; no spec clause:
//! a test facility, not library behaviour).
//!
//! Compiled only with the `selftest` feature, which no product enables: the shipped libraries
//! carry no hook code, no symbol and read no environment variable; no product-compiled file
//! refers to it, so the products are byte-identical with and without this module. A test arms
//! the calling thread's plan, calls the function under test and disarms it; while the plan is
//! armed, the allocations it selects fail exactly as exhausted memory does. It reaches every allocation made through Rust's global
//! allocator (`liella::mem`'s fallible collections, and any other `try_reserve`): the test binary
//! installs [`FailAlloc`] as its global allocator, which returns null for a selected allocation,
//! so the collection reports `ENOMEM` through its ordinary failure path.
//!
//! It does NOT reach allocations that bypass the global allocator: the caller-owned `malloc`
//! family (`crate::caller_alloc`: `strdup`, `FTSENT` blocks, stdio buffers, ...), Liella's own C
//! `malloc` (`heap::HEAP`), `KernelPages` mappings and the sync `OLD_HEAP`. Hooking those would
//! change product-compiled files, which a test-only hook may not do (coordinator #2101).
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
use core::sync::atomic::{AtomicU64, Ordering};

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
    static LIVE: Cell<i64> = const { Cell::new(0) };
}

/// Bytes the calling thread has obtained through [`FailAlloc`] and not given back (allocations
/// minus deallocations, by requested size, since the thread started). A leak test compares it before
/// and after a call; blocks another thread frees move that thread's count, so use it on one thread.
pub fn live_bytes() -> i64 {
    LIVE.try_with(Cell::get).unwrap_or(0)
}

/// [`live_bytes`] moves by `d`. Called by [`FailAlloc`] only.
pub(super) fn count_live(d: i64) {
    let _ = LIVE.try_with(|c| c.set(c.get().wrapping_add(d)));
}

// Calls into the host allocator through [`FailAlloc`], all threads: counted when a call begins
// and again when it has returned, so a reader can tell whether a call of another thread
// overlapped a window (see [`others_begun`]). `CALLS` counts the calling thread's own calls.
static BEGUN: AtomicU64 = AtomicU64::new(0);
static DONE: AtomicU64 = AtomicU64::new(0);
std::thread_local! {
    static CALLS: Cell<u64> = const { Cell::new(0) };
}

/// A call of the calling thread into the host allocator begins. Called by [`FailAlloc`] only.
pub(super) fn call_begins() {
    let _ = CALLS.try_with(|c| c.set(c.get().wrapping_add(1)));
    BEGUN.fetch_add(1, Ordering::SeqCst);
}

/// That call has returned. Called by [`FailAlloc`] only.
pub(super) fn call_done() {
    DONE.fetch_add(1, Ordering::SeqCst);
}

/// Global-allocator calls OTHER threads have begun (process-wide count minus the calling
/// thread's own). With [`others_done`] it tells a test whether a process-wide measure, such as
/// the C heap's `mallinfo2().uordblks`, may include another thread's traffic: read
/// `d = others_done()` BEFORE the first measurement and `b = others_begun()` AFTER the last;
/// `b == d` proves that no other thread's allocator call ran (even in part) between them.
/// Calls a thread makes to the C heap without the global allocator are not counted.
pub fn others_begun() -> u64 {
    BEGUN.load(Ordering::SeqCst).wrapping_sub(CALLS.try_with(Cell::get).unwrap_or(0))
}

/// Global-allocator calls other threads have completed (see [`others_begun`]).
pub fn others_done() -> u64 {
    DONE.load(Ordering::SeqCst).wrapping_sub(CALLS.try_with(Cell::get).unwrap_or(0))
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
/// it towards [`fail_nth`]'s plan. Called by [`FailAlloc`] only.
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
/// as its `#[global_allocator]` to reach the internal heap's failure paths (`selftest/failalloc.rs`).
pub use super::failalloc::FailAlloc;
