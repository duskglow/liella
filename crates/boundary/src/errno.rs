//! The calling thread's `errno` (spec C0.1, D-2.1), reached through `__errno_location`.

use core::sync::atomic::Ordering;

use liella_kernel::errno::Errno;

// Products with the runtime: the inline twin of their own `__errno_location` (D-X.60). Other
// builds (the test processes): the host C library's, which owns `errno` there.
#[cfg(not(feature = "runtime"))]
use crate::caller_alloc::__errno_location;
use crate::conv;
#[cfg(feature = "runtime")]
use crate::tb::errno_inline as __errno_location;

/// Store `e` (never 0, C0.1 clause 2).
pub fn set(e: Errno) {
    if let Some(cell) = conv::errno_cell(__errno_location()) {
        cell.store(e.get(), Ordering::Relaxed);
    }
}

/// The current value.
pub fn get() -> i32 {
    conv::errno_cell(__errno_location()).map_or(0, |c| c.load(Ordering::Relaxed))
}

/// Set the raw value (tests that need to zero `errno`, as C programs do, use this).
pub fn set_raw(v: i32) {
    if let Some(cell) = conv::errno_cell(__errno_location()) {
        cell.store(v, Ordering::Relaxed);
    }
}
