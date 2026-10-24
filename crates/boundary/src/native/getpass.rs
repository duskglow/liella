//! `liella_getpass_set_limit`: the bound on `getpass`'s line (D-17.8; the reading is
//! `crate::users::getpass`).
//!
//! `getpass` reads at most the bound's number of bytes before the newline, 1 MiB by default
//! (`LIELLA_GETPASS_DEFAULT_LIMIT`). A longer line is never returned cut short: it is read to its
//! end and discarded, every buffer of the library's that held it is zeroed, and the call fails
//! with `EOVERFLOW`.

use super::abi;
use crate::export;
use crate::users;

export! { "LIELLA_NATIVE_1";
/// Set the process's bound on `getpass`'s line to `bytes` (0: the default again,
/// `LIELLA_GETPASS_DEFAULT_LIMIT`): the bound that was in force, or `-EINVAL` above
/// `PTRDIFF_MAX`. It holds for the life of the process and in its `fork` children.
pub extern "C" fn liella_getpass_set_limit(bytes: usize) -> i64 {
    abi::value(users::set_pass_limit(bytes))
}}
