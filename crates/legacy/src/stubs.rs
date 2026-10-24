//! STREAMS and removed-interface stubs that fail with ENOSYS (spec §29.3-29.4).
//!
//! Implementation phase: 15 (implementation-plan.md §6), except the §13.15 BSD file flags
//! (phase 5).

use liella::error::{Errno, Result};
use liella_kernel::Fd;
use liella_kernel::ops::FcntlInt;
use liella_kernel::sys::Sys;

/// A call that has nothing to do in this design (`__libc_init_first`: initialisation happens in
/// `__libc_start_main`).
pub fn nothing() {}

/// `chflags`/`fchflags` (§13.15, ⚑ COMPAT): Linux has no BSD file flags; always `ENOSYS`.
pub fn bsd_file_flags() -> Result<()> {
    Err(Errno::ENOSYS)
}

/// `gtty`, `stty` and `revoke` (§19.5, ⚑ COMPAT): the 4.2BSD terminal interface and BSD's
/// `revoke` are not implemented on Linux; always `ENOSYS`.
pub fn bsd_terminal() -> Result<()> {
    Err(Errno::ENOSYS)
}

/// The STREAMS calls (`fattach`, `fdetach`, `getmsg`, `putmsg`, `getpmsg`, `putpmsg`, §29.3) and
/// the removed `bdflush`, `sstk` and `sysctl`/`__sysctl` (§24.6, §29.4), all compat-only: Linux
/// has no STREAMS and the others have no system call left to make. Always `ENOSYS`, whatever the
/// arguments, with no system call issued (probe-settled, spec/behaviour/compat.md).
pub fn removed() -> Result<()> {
    Err(Errno::ENOSYS)
}

/// `isastream(fd)` (§29.3, compat-only): no descriptor is a STREAMS one, so 0 for an open
/// descriptor and `EBADF` for any other (a closed one, -1), as the probe shows.
pub fn isastream<S: Sys>(sys: &S, fd: Fd) -> Result<i64> {
    sys.fcntl_int(fd, FcntlInt::GetFd, 0).map(|_| 0)
}
