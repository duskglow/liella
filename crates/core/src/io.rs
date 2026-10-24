//! Descriptor I/O, polling, durability, memory mapping, event descriptors, locks, async I/O (spec
//! §11).
//!
//! Most of §11 lives in `liella-boundary` and `liella-legacy`; this module holds the rule the
//! library's own descriptors follow.

use crate::error::{Errno, Result};
use liella_kernel::fd::Fd;
use liella_kernel::ops::FcntlInt;
use liella_kernel::sys::Sys;

/// The lowest number a descriptor the library keeps across calls may have.
pub const FIRST_PRIVATE_FD: i64 = 3;

/// `fd`, a descriptor the library keeps for itself across calls, moved above the standard
/// descriptors (0, 1, 2) if it is one of them: the duplicate is close-on-exec, and `fd` is
/// closed. A program that closed its standard output must never find its output going to the
/// library's cache region, a lock file or an event descriptor, and must get `EBADF` as it
/// would without the library (validation T12). On failure (no descriptor above 2: `EMFILE`)
/// `fd` is closed too and the error returned.
pub fn above_standard<S: Sys>(sys: &S, fd: Fd) -> Result<Fd> {
    if fd.raw() >= 3 || fd.raw() < 0 {
        return Ok(fd);
    }
    let moved = sys.fcntl_int(fd, FcntlInt::DupFdCloexec, FIRST_PRIVATE_FD);
    let _ = sys.close(fd);
    Ok(Fd::new(i32::try_from(moved?).map_err(|_| Errno::EBADF)?))
}
