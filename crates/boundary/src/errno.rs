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
#[inline]
pub fn set(e: Errno) {
    set_raw(e.get());
}

/// The current value.
/// Products: out of line, like [`set_raw`].
#[cfg_attr(feature = "runtime", inline(never))]
pub fn get() -> i32 {
    conv::errno_cell(__errno_location()).map_or(0, |c| c.load(Ordering::Relaxed))
}

/// Set the raw value (tests that need to zero `errno`, as C programs do, use this).
/// Products: out of line, so each of the library's `errno` sites is a `mov` and a `call` (about
/// 10 bytes, less than the inline `%fs` load and store) and the D-X.60 test is in one place;
/// `errno` is set on failure paths, where the call and return cost two instructions.
#[cfg_attr(feature = "runtime", inline(never))]
pub fn set_raw(v: i32) {
    if let Some(cell) = conv::errno_cell(__errno_location()) {
        cell.store(v, Ordering::Relaxed);
    }
}
