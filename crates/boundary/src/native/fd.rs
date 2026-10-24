//! `liella_fd_*`: operations on open file descriptors. Each is the system call itself, with
//! the native conventions; none is a cancellation point and none retries `EINTR`.

use liella_kernel::sys::Sys;

use super::abi;
use super::types::LiellaStat;
use crate::conv;
use crate::export;
use crate::sys::CSys;

export! { "LIELLA_NATIVE_1";
/// Read up to `len` bytes from `fd` into `buf`: the count (0 at end of file).
pub extern "C" fn liella_fd_read(fd: i32, buf: *mut u8, len: usize) -> i64 {
    // UNSAFE(slice) SAFETY: buf holds len writable bytes (D-X.10).
    abi::value(unsafe { conv::buf_mut(buf.cast(), len) }.and_then(|b| CSys.read(conv::fd(fd), b)))
}}

export! { "LIELLA_NATIVE_1";
/// Write up to `len` bytes from `buf` to `fd`: the count.
pub extern "C" fn liella_fd_write(fd: i32, buf: *const u8, len: usize) -> i64 {
    // UNSAFE(slice) SAFETY: buf holds len bytes (D-X.10).
    abi::value(unsafe { conv::buf(buf.cast(), len) }.and_then(|b| CSys.write(conv::fd(fd), b)))
}}

export! { "LIELLA_NATIVE_1";
/// Read up to `len` bytes at file offset `offset` (the descriptor's offset unchanged): the
/// count.
pub extern "C" fn liella_fd_pread(fd: i32, buf: *mut u8, len: usize, offset: i64) -> i64 {
    // UNSAFE(slice) SAFETY: buf holds len writable bytes (D-X.10).
    abi::value(unsafe { conv::buf_mut(buf.cast(), len) }.and_then(|b| CSys.pread(conv::fd(fd), b, offset)))
}}

export! { "LIELLA_NATIVE_1";
/// Write up to `len` bytes at file offset `offset` (the descriptor's offset unchanged): the
/// count.
pub extern "C" fn liella_fd_pwrite(fd: i32, buf: *const u8, len: usize, offset: i64) -> i64 {
    // UNSAFE(slice) SAFETY: buf holds len bytes (D-X.10).
    abi::value(unsafe { conv::buf(buf.cast(), len) }.and_then(|b| CSys.pwrite(conv::fd(fd), b, offset)))
}}

export! { "LIELLA_NATIVE_1";
/// Close `fd`. The descriptor is released even when the result is an error (`-EINTR`,
/// `-EIO`): it must not be closed again.
pub extern "C" fn liella_fd_close(fd: i32) -> i64 {
    abi::done(CSys.close(conv::fd(fd)))
}}

export! { "LIELLA_NATIVE_1";
/// Move the offset of `fd` (`whence`: `LIELLA_SEEK_SET`, `_CUR` or `_END`): the new offset.
pub extern "C" fn liella_fd_seek(fd: i32, offset: i64, whence: u32) -> i64 {
    abi::offset(CSys.lseek(conv::fd(fd), offset, whence))
}}

export! { "LIELLA_NATIVE_1";
/// The status of the file open as `fd`, into `*out`.
pub extern "C" fn liella_fd_stat(fd: i32, out: Option<&mut LiellaStat>) -> i64 {
    abi::store(out, CSys.fstat(conv::fd(fd)))
}}
