//! Re-executed test cases that read what the handler of a process-directed signal set (D-X.42).
//!
//! A copy of a test binary re-executed for one case (`--exact`, `--test-threads=1`) still has
//! libtest's main thread besides the case's own, with every signal unblocked. A signal sent to
//! the whole process (`kill(getpid())`, `kill(0)`, `killpg`, `sigqueue`, a child's `SIGCHLD`)
//! may be taken by that thread, whose handler can still be running after the call that sent or
//! caused the signal returned: a case that reads the handler's state then depends on which
//! thread took the signal and when it was scheduled. Such a case runs its body in a `fork`
//! child, where the calling thread is the only one: POSIX `kill`/`sigqueue` deliver the signal
//! to the sending thread before the call returns when no other thread has it unblocked, and a
//! signal sent from a thread the body creates is taken by one of the two before that thread
//! ends (the body joins it). No sleep or retry is needed.
//!
//! The case checks [`sole_thread`] where it reads the handler's state, so a case moved back to
//! the multi-threaded copy fails at once instead of failing once in a few thousand runs.

#![allow(dead_code)]

use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;

/// The number of threads of this process (entries of `/proc/self/task`; 0 if unreadable).
pub fn threads() -> usize {
    std::fs::read_dir("/proc/self/task").map(|d| d.count()).unwrap_or(0)
}

/// Whether the caller is the only thread of its process (reported on standard error when not).
pub fn sole_thread() -> bool {
    let n = threads();
    if n != 1 {
        eprintln!("sole_thread: {n} threads where the case reads a handler's state (run it through single_threaded)");
    }
    n == 1
}

/// Run `body` in a child made by the library's `fork` (only the calling thread is copied) and
/// wait for it: true when `body` returned true (false when it returned false or panicked).
pub fn single_threaded(body: impl FnOnce() -> bool) -> bool {
    let pid = liella_boundary::fork::fork().unwrap();
    if pid == 0 {
        let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body)).unwrap_or(false);
        CSys.exit_group(if ok { 0 } else { 1 });
    }
    let (_, status) = CSys.wait4(pid, 0, None).unwrap();
    status == 0
}
