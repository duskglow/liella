//! `liella_aio_use_threads`: POSIX asynchronous I/O on worker threads only (D-11.11; the engines
//! are `crate::aio`'s).
//!
//! A process's `aio_*` requests run on io_uring when the kernel offers and permits it, and
//! otherwise on a pool of worker threads; the engine is chosen at the first request. The workers
//! are chosen without trying io_uring in secure-execution mode, under a seccomp filter, with
//! `LIELLA_AIO_URING=0` in the environment at the first request (ignored under `AT_SECURE`, where
//! the workers are used anyway), or after this call.

use super::abi;
use crate::aio;
use crate::export;

export! { "LIELLA_NATIVE_1";
/// Run this process's POSIX AIO requests on worker threads, never on io_uring: 0, or `-EBUSY`
/// once a request has started io_uring (call it before the first request). It holds for the
/// life of the process and in its `fork` children.
pub extern "C" fn liella_aio_use_threads() -> i64 {
    abi::done(aio::use_threads())
}}
