//! The process environment (`liella_env_*`): the one `environ` names, shared with the C code's
//! `getenv` and `setenv`. Names, values and entries are bytes; a name is not empty and
//! contains no `=` (else `EINVAL`), and nothing contains a NUL (`EINVAL`).

#![forbid(unsafe_code)]

use alloc::vec::Vec;

use crate::buf::{self, Fit};
use crate::ffi;
use crate::Errno;

/// The value of the variable `name`, or `None` if it is not set.
pub fn get(name: &[u8]) -> Result<Option<Vec<u8>>, Errno> {
    buf::found(buf::fetch(|b| ffi::env_get(name, b)))
}

/// The value of the variable `name` into `buf` (whole or not at all), or `None` if it is not
/// set.
pub fn get_into(name: &[u8], buf: &mut [u8]) -> Result<Option<Fit>, Errno> {
    let len = buf.len();
    buf::found(buf::fit(ffi::env_get(name, buf), len))
}

/// Set the variable `name` to a copy of `value`. If it is set already, it is replaced only
/// when `overwrite` is true.
pub fn set(name: &[u8], value: &[u8], overwrite: bool) -> Result<(), Errno> {
    buf::done(ffi::env_set(name, value, overwrite))
}

/// Remove every entry of the variable `name` (none: success).
pub fn unset(name: &[u8]) -> Result<(), Errno> {
    buf::done(ffi::env_unset(name))
}

/// Turn the environment contract (a Liella extension, off by default) on or off: the previous
/// state. While it is on, `getenv` answers from a table that `setenv`, `putenv`, `unsetenv`,
/// `clearenv` and this module's writers keep current; after changing `environ` any other way,
/// call [`reload`].
pub fn contract(on: bool) -> bool {
    ffi::liella_environ_contract(i32::from(on)) != 0
}

/// Rebuild the environment contract's table from `environ` (nothing while it is off).
pub fn reload() {
    ffi::liella_environ_reload();
}

/// The number of entries in the environment.
pub fn count() -> Result<usize, Errno> {
    buf::len(ffi::liella_env_count())
}

/// Entry `index` (`NAME=VALUE`, 0-based), or `None` past the last.
pub fn entry(index: usize) -> Result<Option<Vec<u8>>, Errno> {
    buf::found(buf::fetch(|b| ffi::env_entry(index, b)))
}

/// Entry `index` into `buf` (whole or not at all), or `None` past the last.
pub fn entry_into(index: usize, buf: &mut [u8]) -> Result<Option<Fit>, Errno> {
    let len = buf.len();
    buf::found(buf::fit(ffi::env_entry(index, buf), len))
}

/// The entries in order (`NAME=VALUE`), each read when the iterator reaches it: if another
/// thread changes the environment meanwhile, entries may be seen twice or missed, as with a
/// walk of `environ`.
pub fn vars() -> Vars {
    Vars { next: Some(0) }
}

/// The iterator of [`vars`]: entries until the last, or until the first error (which it
/// yields, then ends).
#[derive(Debug)]
pub struct Vars {
    next: Option<usize>,
}

impl Iterator for Vars {
    type Item = Result<Vec<u8>, Errno>;

    fn next(&mut self) -> Option<Self::Item> {
        let index = self.next?;
        let r = entry(index).transpose();
        self.next = match r {
            Some(Ok(_)) => index.checked_add(1),
            _ => None,
        };
        r
    }
}
