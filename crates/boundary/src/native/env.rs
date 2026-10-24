//! `liella_env_*`: the process environment (the one `environ` names, shared with `getenv` and
//! `setenv`). Names and values are byte slices; copies out are made under the environment's
//! writers' lock, so they never see an entry a concurrent `setenv` is replacing.

use super::abi;
use crate::conv;
use crate::export;
use crate::process;

export! { "LIELLA_NATIVE_1";
/// The value of the variable `name` into `buf` (copied only if it fits whole): its length, or
/// `-ENOENT` if it is not set.
pub extern "C" fn liella_env_get(name: *const u8, name_len: usize, buf: *mut u8, buf_len: usize) -> i64 {
    // UNSAFE(slice) SAFETY: name holds name_len bytes; buf holds buf_len writable bytes (D-X.10).
    abi::value(abi::text(unsafe { conv::buf(name.cast(), name_len) }).and_then(|n| unsafe { conv::buf_mut(buf.cast(), buf_len) }.and_then(|b| abi::entry(process::getenv_into(n, b)))))
}}

export! { "LIELLA_NATIVE_1";
/// Set the variable `name` to `value` (a copy). If it is set already, it is replaced only when
/// `overwrite` is non-zero. An empty name, or one containing `=`, is `-EINVAL`.
pub extern "C" fn liella_env_set(name: *const u8, name_len: usize, value: *const u8, value_len: usize, overwrite: u32) -> i64 {
    // UNSAFE(slice) SAFETY: name holds name_len bytes and value value_len bytes (D-X.10).
    abi::done(abi::text(unsafe { conv::buf(name.cast(), name_len) }).and_then(|n| abi::text(unsafe { conv::buf(value.cast(), value_len) }).and_then(|v| process::setenv(n, v, abi::flag(overwrite)))))
}}

export! { "LIELLA_NATIVE_1";
/// Remove every entry of the variable `name` (none: success). An empty name, or one
/// containing `=`, is `-EINVAL`.
pub extern "C" fn liella_env_unset(name: *const u8, name_len: usize) -> i64 {
    // UNSAFE(slice) SAFETY: name holds name_len bytes (D-X.10).
    abi::done(abi::text(unsafe { conv::buf(name.cast(), name_len) }).and_then(process::unsetenv))
}}

export! { "LIELLA_NATIVE_1";
/// The number of entries in the environment.
pub extern "C" fn liella_env_count() -> i64 {
    abi::count(process::env_count())
}}

export! { "LIELLA_NATIVE_1";
/// Entry `index` (`NAME=VALUE`, 0-based) into `buf` (copied only if it fits whole): its
/// length, or `-ENOENT` past the last entry.
pub extern "C" fn liella_env_entry(index: usize, buf: *mut u8, buf_len: usize) -> i64 {
    // UNSAFE(slice) SAFETY: buf holds buf_len writable bytes (D-X.10).
    abi::value(unsafe { conv::buf_mut(buf.cast(), buf_len) }.and_then(|b| abi::entry(process::env_entry_into(index, b))))
}}

export! { "LIELLA_NATIVE_1";
/// The environment contract (D-1.12), a Liella extension: non-zero `on` turns it on, 0 off; the
/// previous state (1: on). While it is on, `getenv` answers from a table this library's
/// environment writers keep current; other changes to `environ` need `liella_environ_reload`.
pub extern "C" fn liella_environ_contract(on: i32) -> i32 {
    i32::from(process::environ_contract(on != 0))
}}

export! { "LIELLA_NATIVE_1";
/// Rebuild the environment contract's table from `environ` (nothing while it is off).
pub extern "C" fn liella_environ_reload() {
    process::environ_reload();
}}
