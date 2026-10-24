//! POSIX asynchronous I/O's engine (`liella_aio_use_threads`, D-11.11): a process's `aio_*`
//! requests run on io_uring when the kernel offers and permits it, else on worker threads. The
//! workers are chosen without trying io_uring in secure-execution mode, under a seccomp filter,
//! with `LIELLA_AIO_URING=0` in the environment at the first request, or after [`use_threads`].

#![forbid(unsafe_code)]

use crate::buf;
use crate::ffi;
use crate::Errno;

/// Run this process's POSIX AIO requests on worker threads, never on io_uring (for the life of
/// the process and its `fork` children). Call it before the first request: `EBUSY` once a
/// request has started io_uring.
pub fn use_threads() -> Result<(), Errno> {
    buf::done(ffi::liella_aio_use_threads())
}
