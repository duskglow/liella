//! `liella_process_*`: the process's identity, arguments and end.

use liella_kernel::sys::Sys;

use super::abi;
use crate::conv;
use crate::export;
use crate::process;
use crate::sys::CSys;

export! { "LIELLA_NATIVE_1";
/// End the process as `exit(status)`: the C exit handlers (`atexit`, `__cxa_atexit`, ...)
/// run, most recent first, the C streams are flushed, and the process exits with
/// `status & 0xff`. Never returns.
pub extern "C" fn liella_process_exit(status: i32) -> ! {
    process::exit(status)
}}

export! { "LIELLA_NATIVE_1";
/// The number of program arguments (`argc`; 0 in a process Liella's runtime did not start).
pub extern "C" fn liella_process_arg_count() -> i64 {
    abi::count(process::arg_count())
}}

export! { "LIELLA_NATIVE_1";
/// Argument `index` (0 is the program name as invoked) into `buf` (copied only if it fits
/// whole): its length, or `-ENOENT` for `index` at or past the count.
pub extern "C" fn liella_process_arg(index: usize, buf: *mut u8, buf_len: usize) -> i64 {
    // UNSAFE(slice) SAFETY: buf holds buf_len writable bytes (D-X.10).
    abi::value(unsafe { conv::buf_mut(buf.cast(), buf_len) }.and_then(|b| abi::entry(process::arg_into(index, b))))
}}

export! { "LIELLA_NATIVE_1";
/// The process ID.
pub extern "C" fn liella_process_id() -> i64 {
    i64::from(CSys.getpid())
}}
