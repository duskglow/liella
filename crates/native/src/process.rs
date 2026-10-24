//! The process (`liella_process_*`): its arguments, its ID, and exit.

#![forbid(unsafe_code)]

use alloc::vec::Vec;

use crate::buf::{self, Fit};
use crate::ffi;
use crate::Errno;

/// End the process with `status`, as C `exit`: the C exit handlers (`atexit`, `__cxa_atexit`,
/// ...) run and the C streams are flushed. Rust destructors of live values do not run.
pub fn exit(status: i32) -> ! {
    ffi::liella_process_exit(status)
}

/// The number of program arguments (`argc`, the program name included).
pub fn arg_count() -> Result<usize, Errno> {
    buf::len(ffi::liella_process_arg_count())
}

/// Argument `index` (0 is the program name as it was started), or `None` past the last.
pub fn arg(index: usize) -> Result<Option<Vec<u8>>, Errno> {
    buf::found(buf::fetch(|b| ffi::process_arg(index, b)))
}

/// Argument `index` into `buf` (whole or not at all), or `None` past the last.
pub fn arg_into(index: usize, buf: &mut [u8]) -> Result<Option<Fit>, Errno> {
    let len = buf.len();
    buf::found(buf::fit(ffi::process_arg(index, buf), len))
}

/// The arguments in order, the program name first.
pub fn args() -> Args {
    Args { next: Some(0) }
}

/// The iterator of [`args`]: arguments until the last, or until the first error (which it
/// yields, then ends).
#[derive(Debug)]
pub struct Args {
    next: Option<usize>,
}

impl Iterator for Args {
    type Item = Result<Vec<u8>, Errno>;

    fn next(&mut self) -> Option<Self::Item> {
        let index = self.next?;
        let r = arg(index).transpose();
        self.next = match r {
            Some(Ok(_)) => index.checked_add(1),
            _ => None,
        };
        r
    }
}

/// The process ID.
pub fn id() -> Result<u32, Errno> {
    buf::value(ffi::liella_process_id()).and_then(|v| u32::try_from(v).map_err(|_| Errno::EOVERFLOW))
}
