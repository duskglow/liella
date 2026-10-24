//! The bound on `getpass`'s line (`liella_getpass_set_limit`, D-17.8): a C `getpass` in the
//! program reads at most this many bytes before the newline. A longer line is never returned
//! cut short: it is read to its end and discarded, every buffer of the library's that held it is
//! zeroed, and `getpass` fails with `EOVERFLOW`.

#![forbid(unsafe_code)]

use crate::buf;
use crate::ffi;
use crate::Errno;

/// The default bound, in bytes (1 MiB; `LIELLA_GETPASS_DEFAULT_LIMIT`).
pub const DEFAULT_LIMIT: usize = 1048576;

/// Set the process's bound to `bytes` (0: [`DEFAULT_LIMIT`] again), for the life of the
/// process and its `fork` children: the bound that was in force, or `EINVAL` above
/// `isize::MAX`.
pub fn set_limit(bytes: usize) -> Result<usize, Errno> {
    buf::len(ffi::liella_getpass_set_limit(bytes))
}
