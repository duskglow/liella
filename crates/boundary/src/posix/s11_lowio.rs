//! Translation for spec §11: low-level I/O on file descriptors (phase 5; AIO, §11.11, phase 13:
//! `crate::aio`, which `close`, `close_range`, `closefrom`, `dup2` and `dup3` also consult first,
//! D-11.8).
//!
//! Thin wrappers are the typed `Sys` methods themselves (D-11.1); what glibc's contract adds is
//! in `liella_legacy::io_compat`. Fixed-size C objects (`fd_set`, `timeval`, `timespec`,
//! `epoll_event`, the `off_t` cells) are typed parameters (D-X.8). The memory calls take the C
//! caller's raw addresses through `crate::raw` (D-11.7). `open`, `openat`, `fcntl`, `ioctl` and
//! `mremap` are variadic: their C entry points are adapters (`csys/src/adapter/variadic.c`)
//! forwarding to the fixed-arity `liella_*` exports here (OD-C1, version `LIELLA_PRIVATE`).

use liella_abi::ctypes::{c_char, c_int, c_long, c_uint, c_ulong, c_void, size_t, ssize_t};
use liella_abi::layout::{sigset_t, struct_aioinit};
use liella_kernel::fd::Fd;
use liella_kernel::mem::{IoVec, IoVecMut, RemoteIoVec};
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{EpollEvent, FdSet, PollFd, SigEvent, Timespec, Timeval};
use liella_legacy::io_compat;

use crate::aio;
use crate::cancel;
use crate::conv;
use crate::export;
use crate::raw;
use crate::ret;
use crate::strz;
use crate::sys::{self, CSys, CP};

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_open3(path: *const c_char, flags: c_int, mode: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§11.1).
    cancel::point(ret::fd(unsafe { strz::path(path) }.and_then(|p| CP.openat(Fd::CWD, p, conv::bits(flags), mode))))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_openat4(dirfd: c_int, path: *const c_char, flags: c_int, mode: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§11.1).
    cancel::point(ret::fd(unsafe { strz::path(path) }.and_then(|p| CP.openat(conv::fd(dirfd), p, conv::bits(flags), mode))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn creat(path: *const c_char, mode: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§11.1).
    cancel::point(ret::fd(unsafe { strz::path(path) }.and_then(|p| CP.openat(Fd::CWD, p, io_compat::CREAT_FLAGS, mode))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn creat64(path: *const c_char, mode: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§11.1).
    cancel::point(ret::fd(unsafe { strz::path(path) }.and_then(|p| CP.openat(Fd::CWD, p, io_compat::CREAT_FLAGS, mode))))
}}

export! { "GLIBC_2.7";
pub extern "C" fn __open_2(path: *const c_char, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§11.1).
    cancel::point(ret::fd(unsafe { strz::path(path) }.and_then(|p| CP.openat(Fd::CWD, p, conv::bits(conv::open_without_mode(flags)), 0))))
}}

export! { "GLIBC_2.7";
pub extern "C" fn __open64_2(path: *const c_char, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§11.1).
    ret::fd(unsafe { strz::path(path) }.and_then(|p| CSys.openat(Fd::CWD, p, conv::bits(conv::open_without_mode(flags)), 0)))
}}

export! { "GLIBC_2.7";
pub extern "C" fn __openat_2(dirfd: c_int, path: *const c_char, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§11.1).
    cancel::point(ret::fd(unsafe { strz::path(path) }.and_then(|p| CP.openat(conv::fd(dirfd), p, conv::bits(conv::open_without_mode(flags)), 0))))
}}

export! { "GLIBC_2.7";
pub extern "C" fn __openat64_2(dirfd: c_int, path: *const c_char, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§11.1).
    ret::fd(unsafe { strz::path(path) }.and_then(|p| CSys.openat(conv::fd(dirfd), p, conv::bits(conv::open_without_mode(flags)), 0)))
}}

export! { "GLIBC_2.14";
pub extern "C" fn name_to_handle_at(dirfd: c_int, path: *const c_char, handle: *mut c_void, mount_id: *mut c_int, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string; handle and mount_id are the caller's objects (§11.1).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| raw::name_to_handle_at(dirfd, p, conv::addr(handle), conv::addr(mount_id), flags)))
}}

export! { "GLIBC_2.14";
pub extern "C" fn open_by_handle_at(mount_fd: c_int, handle: *mut c_void, flags: c_int) -> c_int {
    ret::fd(raw::open_by_handle_at(mount_fd, conv::addr(handle), flags))
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn close(fd: c_int) -> c_int {
    sys::direct_close(fd)
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn __close(fd: c_int) -> c_int {
    sys::direct_close(fd)
}}

export! { local "liella_close_rest";
/// `close`'s Rust path (CANCEL-FAST-2, from the direct entry in machine/points.S while any
/// asynchronous I/O was ever started): the descriptor's requests are cancelled first.
pub extern "C" fn close_rest(fd: c_int) -> c_int {
    ret::raw_zero(sys::close_point(aio::closing(conv::fd(fd))))
}}

export! { local "liella_close_point_fail";
/// The direct `close`'s error or due-request result (machine/points.S): a due request is acted
/// on (errno from `r`), else `r` is converted (`-1` and errno, or 0).
pub extern "C" fn close_point_fail(r: i64) -> c_int {
    ret::raw_zero(sys::point_result(r, true))
}}

export! { local "liella_cancel_point_fail";
/// The direct read/write entries' error result (machine/points.S): a due request is acted on
/// (D-22.11: only when the cancellation word says it is due), else `-1` and errno.
pub extern "C" fn cancel_point_fail(r: i64) -> ssize_t {
    ret::raw_ssize(sys::point_result(r, false))
}}

export! { "GLIBC_2.34";
pub extern "C" fn close_range(first: c_uint, last: c_uint, flags: c_int) -> c_int {
    ret::zero(CSys.close_range(aio::closing_range(first, last, conv::bits(flags)), last, conv::bits(flags)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn closefrom(lo: c_int) {
    ret::must(io_compat::closefrom(&CSys, aio::closing_from(lo)))
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn read(fd: c_int, buf: *mut c_void, n: size_t) -> ssize_t {
    sys::direct_read(fd, buf, n)
}}

export! { local "liella_read_rest";
/// `read`'s Rust path (CANCEL-FAST-2, from the direct entry in machine/points.S): no descriptor
/// yet, a count of 0 or above isize::MAX, or a NULL buffer.
pub extern "C" fn read_rest(fd: c_int, buf: *mut c_void, n: size_t) -> ssize_t {
    // UNSAFE(slice) SAFETY: buf holds n bytes (§11.3).
    ret::raw_ssize(unsafe { conv::buf_mut(buf, n) }.map_or_else(ret::raw_err, |b| sys::read_point(conv::fd(fd), b)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __read(fd: c_int, buf: *mut c_void, n: size_t) -> ssize_t {
    // UNSAFE(slice) SAFETY: buf holds n bytes (§11.3).
    ret::ssize(unsafe { conv::buf_mut(buf, n) }.and_then(|b| CSys.read(conv::fd(fd), b)))
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn write(fd: c_int, buf: *const c_void, n: size_t) -> ssize_t {
    sys::direct_write(fd, buf, n)
}}

export! { local "liella_write_rest";
/// `write`'s Rust path (CANCEL-FAST-2, from the direct entry in machine/points.S): no descriptor
/// yet, a count of 0 or above isize::MAX, or a NULL buffer.
pub extern "C" fn write_rest(fd: c_int, buf: *const c_void, n: size_t) -> ssize_t {
    // UNSAFE(slice) SAFETY: buf holds n bytes (§11.3).
    ret::raw_ssize(unsafe { conv::buf(buf, n) }.map_or_else(ret::raw_err, |b| sys::write_point(conv::fd(fd), b)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __write(fd: c_int, buf: *const c_void, n: size_t) -> ssize_t {
    // UNSAFE(slice) SAFETY: buf holds n bytes (§11.3).
    ret::ssize(unsafe { conv::buf(buf, n) }.and_then(|b| CSys.write(conv::fd(fd), b)))
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn pread(fd: c_int, buf: *mut c_void, n: size_t, off: i64) -> ssize_t {
    sys::direct_pread(fd, buf, n, off)
}}

export! { local "liella_pread_rest";
/// `pread`'s Rust path (CANCEL-FAST-2, from the direct entry in machine/points.S): no descriptor
/// yet, a count of 0 or above isize::MAX, or a NULL buffer.
pub extern "C" fn pread_rest(fd: c_int, buf: *mut c_void, n: size_t, off: i64) -> ssize_t {
    // UNSAFE(slice) SAFETY: buf holds n bytes (§11.3).
    ret::raw_ssize(unsafe { conv::buf_mut(buf, n) }.map_or_else(ret::raw_err, |b| sys::pread_point(conv::fd(fd), b, off)))
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn pread64(fd: c_int, buf: *mut c_void, n: size_t, off: i64) -> ssize_t {
    sys::direct_pread(fd, buf, n, off)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __pread64(fd: c_int, buf: *mut c_void, n: size_t, off: i64) -> ssize_t {
    // UNSAFE(slice) SAFETY: buf holds n bytes (§11.3).
    ret::ssize(unsafe { conv::buf_mut(buf, n) }.and_then(|b| CSys.pread(conv::fd(fd), b, off)))
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn pwrite(fd: c_int, buf: *const c_void, n: size_t, off: i64) -> ssize_t {
    sys::direct_pwrite(fd, buf, n, off)
}}

export! { local "liella_pwrite_rest";
/// `pwrite`'s Rust path (CANCEL-FAST-2, from the direct entry in machine/points.S): no descriptor
/// yet, a count of 0 or above isize::MAX, or a NULL buffer.
pub extern "C" fn pwrite_rest(fd: c_int, buf: *const c_void, n: size_t, off: i64) -> ssize_t {
    // UNSAFE(slice) SAFETY: buf holds n bytes (§11.3).
    ret::raw_ssize(unsafe { conv::buf(buf, n) }.map_or_else(ret::raw_err, |b| sys::pwrite_point(conv::fd(fd), b, off)))
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn pwrite64(fd: c_int, buf: *const c_void, n: size_t, off: i64) -> ssize_t {
    sys::direct_pwrite(fd, buf, n, off)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __pwrite64(fd: c_int, buf: *const c_void, n: size_t, off: i64) -> ssize_t {
    // UNSAFE(slice) SAFETY: buf holds n bytes (§11.3).
    ret::ssize(unsafe { conv::buf(buf, n) }.and_then(|b| CSys.pwrite(conv::fd(fd), b, off)))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __read_chk(fd: c_int, buf: *mut c_void, n: size_t, buflen: size_t) -> ssize_t {
    // UNSAFE(slice) SAFETY: as read, after the fortify check.
    ret::raw_ssize(unsafe { conv::buf_mut(buf, conv::chk(n, buflen)) }.map_or_else(ret::raw_err, |b| sys::read_point(conv::fd(fd), b)))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __pread_chk(fd: c_int, buf: *mut c_void, n: size_t, off: i64, buflen: size_t) -> ssize_t {
    // UNSAFE(slice) SAFETY: as pread, after the fortify check.
    ret::raw_ssize(unsafe { conv::buf_mut(buf, conv::chk(n, buflen)) }.map_or_else(ret::raw_err, |b| sys::pread_point(conv::fd(fd), b, off)))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __pread64_chk(fd: c_int, buf: *mut c_void, n: size_t, off: i64, buflen: size_t) -> ssize_t {
    // UNSAFE(slice) SAFETY: as pread, after the fortify check.
    ret::raw_ssize(unsafe { conv::buf_mut(buf, conv::chk(n, buflen)) }.map_or_else(ret::raw_err, |b| sys::pread_point(conv::fd(fd), b, off)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn readv(fd: c_int, iov: *mut c_void, iovcnt: c_int) -> ssize_t {
    // UNSAFE(slice) SAFETY: iov holds iovcnt iovec entries, each describing valid memory (§11.3).
    cancel::point(ret::ssize(unsafe { conv::array_mut::<IoVecMut<'_>>(iov, conv::iovcnt(iovcnt)) }.and_then(|v| CP.readv(conv::fd(fd), v))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn writev(fd: c_int, iov: *const c_void, iovcnt: c_int) -> ssize_t {
    // UNSAFE(slice) SAFETY: iov holds iovcnt iovec entries, each describing valid memory (§11.3).
    cancel::point(ret::ssize(unsafe { conv::array::<IoVec<'_>>(iov, conv::iovcnt(iovcnt)) }.and_then(|v| CP.writev(conv::fd(fd), v))))
}}

export! { "GLIBC_2.10";
pub extern "C" fn preadv(fd: c_int, iov: *mut c_void, iovcnt: c_int, off: i64) -> ssize_t {
    // UNSAFE(slice) SAFETY: iov holds iovcnt iovec entries, each describing valid memory (§11.3).
    cancel::point(ret::ssize(unsafe { conv::array_mut::<IoVecMut<'_>>(iov, conv::iovcnt(iovcnt)) }.and_then(|v| CP.preadv(conv::fd(fd), v, off))))
}}

export! { "GLIBC_2.10";
pub extern "C" fn preadv64(fd: c_int, iov: *mut c_void, iovcnt: c_int, off: i64) -> ssize_t {
    // UNSAFE(slice) SAFETY: iov holds iovcnt iovec entries, each describing valid memory (§11.3).
    cancel::point(ret::ssize(unsafe { conv::array_mut::<IoVecMut<'_>>(iov, conv::iovcnt(iovcnt)) }.and_then(|v| CP.preadv(conv::fd(fd), v, off))))
}}

export! { "GLIBC_2.10";
pub extern "C" fn pwritev(fd: c_int, iov: *const c_void, iovcnt: c_int, off: i64) -> ssize_t {
    // UNSAFE(slice) SAFETY: iov holds iovcnt iovec entries, each describing valid memory (§11.3).
    cancel::point(ret::ssize(unsafe { conv::array::<IoVec<'_>>(iov, conv::iovcnt(iovcnt)) }.and_then(|v| CP.pwritev(conv::fd(fd), v, off))))
}}

export! { "GLIBC_2.10";
pub extern "C" fn pwritev64(fd: c_int, iov: *const c_void, iovcnt: c_int, off: i64) -> ssize_t {
    // UNSAFE(slice) SAFETY: iov holds iovcnt iovec entries, each describing valid memory (§11.3).
    cancel::point(ret::ssize(unsafe { conv::array::<IoVec<'_>>(iov, conv::iovcnt(iovcnt)) }.and_then(|v| CP.pwritev(conv::fd(fd), v, off))))
}}

export! { "GLIBC_2.26";
pub extern "C" fn preadv2(fd: c_int, iov: *mut c_void, iovcnt: c_int, off: i64, flags: c_int) -> ssize_t {
    // UNSAFE(slice) SAFETY: iov holds iovcnt iovec entries, each describing valid memory (§11.3).
    cancel::point(ret::ssize(unsafe { conv::array_mut::<IoVecMut<'_>>(iov, conv::iovcnt(iovcnt)) }.and_then(|v| CP.preadv2(conv::fd(fd), v, off, conv::bits(flags)))))
}}

export! { "GLIBC_2.26";
pub extern "C" fn preadv64v2(fd: c_int, iov: *mut c_void, iovcnt: c_int, off: i64, flags: c_int) -> ssize_t {
    // UNSAFE(slice) SAFETY: iov holds iovcnt iovec entries, each describing valid memory (§11.3).
    cancel::point(ret::ssize(unsafe { conv::array_mut::<IoVecMut<'_>>(iov, conv::iovcnt(iovcnt)) }.and_then(|v| CP.preadv2(conv::fd(fd), v, off, conv::bits(flags)))))
}}

export! { "GLIBC_2.26";
pub extern "C" fn pwritev2(fd: c_int, iov: *const c_void, iovcnt: c_int, off: i64, flags: c_int) -> ssize_t {
    // UNSAFE(slice) SAFETY: iov holds iovcnt iovec entries, each describing valid memory (§11.3).
    cancel::point(ret::ssize(unsafe { conv::array::<IoVec<'_>>(iov, conv::iovcnt(iovcnt)) }.and_then(|v| CP.pwritev2(conv::fd(fd), v, off, conv::bits(flags)))))
}}

export! { "GLIBC_2.26";
pub extern "C" fn pwritev64v2(fd: c_int, iov: *const c_void, iovcnt: c_int, off: i64, flags: c_int) -> ssize_t {
    // UNSAFE(slice) SAFETY: iov holds iovcnt iovec entries, each describing valid memory (§11.3).
    cancel::point(ret::ssize(unsafe { conv::array::<IoVec<'_>>(iov, conv::iovcnt(iovcnt)) }.and_then(|v| CP.pwritev2(conv::fd(fd), v, off, conv::bits(flags)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn lseek(fd: c_int, off: i64, whence: c_int) -> i64 {
    ret::off(CSys.lseek(conv::fd(fd), off, conv::bits(whence)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn lseek64(fd: c_int, off: i64, whence: c_int) -> i64 {
    ret::off(CSys.lseek(conv::fd(fd), off, conv::bits(whence)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __lseek(fd: c_int, off: i64, whence: c_int) -> i64 {
    ret::off(CSys.lseek(conv::fd(fd), off, conv::bits(whence)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn llseek(fd: c_int, off: i64, whence: c_int) -> i64 {
    ret::off(CSys.lseek(conv::fd(fd), off, conv::bits(whence)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn dup(fd: c_int) -> c_int {
    ret::fd(CSys.dup(conv::fd(fd)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn dup2(old: c_int, new: c_int) -> c_int {
    ret::fd(CSys.dup2(conv::fd(old), aio::closing_dup(conv::fd(old), conv::fd(new))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __dup2(old: c_int, new: c_int) -> c_int {
    ret::fd(CSys.dup2(conv::fd(old), aio::closing_dup(conv::fd(old), conv::fd(new))))
}}

export! { "GLIBC_2.9";
pub extern "C" fn dup3(old: c_int, new: c_int, flags: c_int) -> c_int {
    ret::fd(CSys.dup3(conv::fd(old), aio::closing_dup(conv::fd(old), conv::fd(new)), conv::bits(flags)))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_fcntl3(fd: c_int, cmd: c_int, arg: usize) -> c_int {
    // UNSAFE(slice) SAFETY: for a pointer command, arg is that command's structure (§11.5).
    ret::int(io_compat::fcntl(&CSys, conv::fd(fd), unsafe { conv::fcntl_arg(cmd, arg) }))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_ioctl3(fd: c_int, request: c_ulong, arg: usize) -> c_int {
    ret::int(raw::ioctl(fd, request, arg))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn select(nfds: c_int, r: Option<&mut FdSet>, w: Option<&mut FdSet>, e: Option<&mut FdSet>, timeout: Option<&mut Timeval>) -> c_int {
    cancel::point(ret::count(io_compat::select(&CP, nfds, r, w, e, timeout)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __select(nfds: c_int, r: Option<&mut FdSet>, w: Option<&mut FdSet>, e: Option<&mut FdSet>, timeout: Option<&mut Timeval>) -> c_int {
    ret::count(io_compat::select(&CSys, nfds, r, w, e, timeout))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pselect(nfds: c_int, r: Option<&mut FdSet>, w: Option<&mut FdSet>, e: Option<&mut FdSet>, timeout: Option<&Timespec>, mask: Option<&sigset_t>) -> c_int {
    cancel::point(ret::count(io_compat::pselect(&CP, nfds, r, w, e, timeout, conv::sigmask(mask))))
}}

export! { "GLIBC_2.15";
pub extern "C" fn __fdelt_chk(d: c_long) -> c_long {
    ret::fdelt(io_compat::fdelt(d))
}}

export! { "GLIBC_2.15";
pub extern "C" fn __fdelt_warn(d: c_long) -> c_long {
    ret::fdelt(io_compat::fdelt(d))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn poll(fds: *mut c_void, nfds: c_ulong, timeout: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: fds holds nfds pollfd entries (§11.7).
    cancel::point(ret::count(unsafe { conv::array_mut::<PollFd>(fds, conv::count_ul(nfds)) }.and_then(|f| io_compat::poll(&CP, f, timeout))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __poll(fds: *mut c_void, nfds: c_ulong, timeout: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: fds holds nfds pollfd entries (§11.7).
    ret::count(unsafe { conv::array_mut::<PollFd>(fds, conv::count_ul(nfds)) }.and_then(|f| io_compat::poll(&CSys, f, timeout)))
}}

export! { "GLIBC_2.16";
pub extern "C" fn __poll_chk(fds: *mut c_void, nfds: c_ulong, timeout: c_int, fdslen: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: as poll, after the fortify check.
    cancel::point(ret::count(unsafe { conv::array_mut::<PollFd>(fds, conv::count_ul(conv::chk_elems(nfds, fdslen, 8))) }.and_then(|f| io_compat::poll(&CP, f, timeout))))
}}

export! { "GLIBC_2.4";
pub extern "C" fn ppoll(fds: *mut c_void, nfds: c_ulong, timeout: Option<&Timespec>, mask: Option<&sigset_t>) -> c_int {
    // UNSAFE(slice) SAFETY: fds holds nfds pollfd entries (§11.7).
    cancel::point(ret::count(unsafe { conv::array_mut::<PollFd>(fds, conv::count_ul(nfds)) }.and_then(|f| io_compat::ppoll(&CP, f, timeout, conv::sigmask(mask)))))
}}

export! { "GLIBC_2.16";
pub extern "C" fn __ppoll_chk(fds: *mut c_void, nfds: c_ulong, timeout: Option<&Timespec>, mask: Option<&sigset_t>, fdslen: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: as ppoll, after the fortify check.
    cancel::point(ret::count(unsafe { conv::array_mut::<PollFd>(fds, conv::count_ul(conv::chk_elems(nfds, fdslen, 8))) }.and_then(|f| io_compat::ppoll(&CP, f, timeout, conv::sigmask(mask)))))
}}

export! { "GLIBC_2.3.2";
pub extern "C" fn epoll_create(size: c_int) -> c_int {
    ret::fd(io_compat::epoll_create(&CSys, size))
}}

export! { "GLIBC_2.9";
pub extern "C" fn epoll_create1(flags: c_int) -> c_int {
    ret::fd(CSys.epoll_create1(conv::bits(flags)))
}}

export! { "GLIBC_2.3.2";
pub extern "C" fn epoll_ctl(ep: c_int, op: c_int, fd: c_int, event: Option<&EpollEvent>) -> c_int {
    ret::zero(CSys.epoll_ctl(conv::fd(ep), conv::bits(op), conv::fd(fd), event))
}}

export! { "GLIBC_2.3.2";
pub extern "C" fn epoll_wait(ep: c_int, events: *mut c_void, maxevents: c_int, timeout: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: events has room for maxevents entries (§11.7).
    cancel::point(ret::count(unsafe { conv::array_mut::<EpollEvent>(events, i64::from(maxevents)) }.and_then(|e| io_compat::epoll_wait(&CP, conv::fd(ep), e, timeout))))
}}

export! { "GLIBC_2.6";
pub extern "C" fn epoll_pwait(ep: c_int, events: *mut c_void, maxevents: c_int, timeout: c_int, mask: Option<&sigset_t>) -> c_int {
    // UNSAFE(slice) SAFETY: events has room for maxevents entries (§11.7).
    cancel::point(ret::count(unsafe { conv::array_mut::<EpollEvent>(events, i64::from(maxevents)) }.and_then(|e| CP.epoll_pwait(conv::fd(ep), e, timeout, conv::sigmask(mask).as_ref()))))
}}

export! { "GLIBC_2.35";
pub extern "C" fn epoll_pwait2(ep: c_int, events: *mut c_void, maxevents: c_int, timeout: Option<&Timespec>, mask: Option<&sigset_t>) -> c_int {
    // UNSAFE(slice) SAFETY: events has room for maxevents entries (§11.7).
    cancel::point(ret::count(unsafe { conv::array_mut::<EpollEvent>(events, i64::from(maxevents)) }.and_then(|e| CP.epoll_pwait2(conv::fd(ep), e, timeout, conv::sigmask(mask).as_ref()))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fsync(fd: c_int) -> c_int {
    cancel::point(ret::zero(CP.fsync(conv::fd(fd))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fdatasync(fd: c_int) -> c_int {
    cancel::point(ret::zero(CP.fdatasync(conv::fd(fd))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sync() {
    CSys.sync()
}}

export! { "GLIBC_2.14";
pub extern "C" fn syncfs(fd: c_int) -> c_int {
    ret::zero(CSys.syncfs(conv::fd(fd)))
}}

export! { "GLIBC_2.6";
pub extern "C" fn sync_file_range(fd: c_int, off: i64, n: i64, flags: c_uint) -> c_int {
    ret::zero(CSys.sync_file_range(conv::fd(fd), off, n, flags))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_fadvise(fd: c_int, off: i64, len: i64, advice: c_int) -> c_int {
    ret::errnum(CSys.fadvise(conv::fd(fd), off, len, conv::bits(advice)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_fadvise64(fd: c_int, off: i64, len: i64, advice: c_int) -> c_int {
    ret::errnum(CSys.fadvise(conv::fd(fd), off, len, conv::bits(advice)))
}}

export! { "GLIBC_2.3";
pub extern "C" fn readahead(fd: c_int, off: i64, count: size_t) -> ssize_t {
    ret::ssize_zero(CSys.readahead(conv::fd(fd), off, count))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_fallocate(fd: c_int, off: i64, len: i64) -> c_int {
    ret::errnum(io_compat::posix_fallocate(&CSys, conv::fd(fd), off, len))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_fallocate64(fd: c_int, off: i64, len: i64) -> c_int {
    ret::errnum(io_compat::posix_fallocate(&CSys, conv::fd(fd), off, len))
}}

export! { "GLIBC_2.10";
pub extern "C" fn fallocate(fd: c_int, mode: c_int, off: i64, len: i64) -> c_int {
    ret::zero(CSys.fallocate(conv::fd(fd), conv::bits(mode), off, len))
}}

export! { "GLIBC_2.10";
pub extern "C" fn fallocate64(fd: c_int, mode: c_int, off: i64, len: i64) -> c_int {
    ret::zero(CSys.fallocate(conv::fd(fd), conv::bits(mode), off, len))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sendfile(out: c_int, input: c_int, off: Option<&mut i64>, count: size_t) -> ssize_t {
    ret::ssize(CSys.sendfile(conv::fd(out), conv::fd(input), off, count))
}}

export! { "GLIBC_2.3";
pub extern "C" fn sendfile64(out: c_int, input: c_int, off: Option<&mut i64>, count: size_t) -> ssize_t {
    ret::ssize(CSys.sendfile(conv::fd(out), conv::fd(input), off, count))
}}

export! { "GLIBC_2.27";
pub extern "C" fn copy_file_range(input: c_int, in_off: Option<&mut i64>, out: c_int, out_off: Option<&mut i64>, len: size_t, flags: c_uint) -> ssize_t {
    ret::ssize(CSys.copy_file_range(conv::fd(input), in_off, conv::fd(out), out_off, len, flags))
}}

export! { "GLIBC_2.5";
pub extern "C" fn splice(input: c_int, in_off: Option<&mut i64>, out: c_int, out_off: Option<&mut i64>, len: size_t, flags: c_uint) -> ssize_t {
    ret::ssize(CSys.splice(conv::fd(input), in_off, conv::fd(out), out_off, len, flags))
}}

export! { "GLIBC_2.5";
pub extern "C" fn tee(input: c_int, out: c_int, len: size_t, flags: c_uint) -> ssize_t {
    ret::ssize(CSys.tee(conv::fd(input), conv::fd(out), len, flags))
}}

export! { "GLIBC_2.5";
pub extern "C" fn vmsplice(fd: c_int, iov: *mut c_void, nr: size_t, flags: c_uint) -> ssize_t {
    // UNSAFE(slice) SAFETY: iov holds nr iovec entries, each describing valid memory (§11.9).
    ret::ssize(unsafe { conv::array_mut::<IoVecMut<'_>>(iov, conv::count_sz(nr)) }.and_then(|v| CSys.vmsplice(conv::fd(fd), v, flags)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mmap(addr: *mut c_void, len: size_t, prot: c_int, flags: c_int, fd: c_int, off: i64) -> *mut c_void {
    ret::addr_or_neg1(raw::mmap(conv::addr(addr), len, prot, flags, fd, off))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mmap64(addr: *mut c_void, len: size_t, prot: c_int, flags: c_int, fd: c_int, off: i64) -> *mut c_void {
    ret::addr_or_neg1(raw::mmap(conv::addr(addr), len, prot, flags, fd, off))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn munmap(addr: *mut c_void, len: size_t) -> c_int {
    ret::zero(raw::munmap(conv::addr(addr), len))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mprotect(addr: *mut c_void, len: size_t, prot: c_int) -> c_int {
    ret::zero(raw::mprotect(conv::addr(addr), len, prot))
}}

export! { "GLIBC_2.27";
pub extern "C" fn pkey_mprotect(addr: *mut c_void, len: size_t, prot: c_int, key: c_int) -> c_int {
    ret::zero(raw::pkey_mprotect(conv::addr(addr), len, prot, key))
}}

export! { "GLIBC_2.27";
pub extern "C" fn pkey_alloc(flags: c_uint, access: c_uint) -> c_int {
    ret::int32(CSys.pkey_alloc(flags, access))
}}

export! { "GLIBC_2.27";
pub extern "C" fn pkey_free(key: c_int) -> c_int {
    ret::zero(CSys.pkey_free(key))
}}

export! { "GLIBC_2.27";
pub extern "C" fn pkey_get(key: c_int) -> c_int {
    ret::int32(io_compat::pkey_get(&CSys, key))
}}

export! { "GLIBC_2.27";
pub extern "C" fn pkey_set(key: c_int, rights: c_uint) -> c_int {
    ret::zero(io_compat::pkey_set(&CSys, key, rights))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn madvise(addr: *mut c_void, len: size_t, advice: c_int) -> c_int {
    ret::zero(raw::madvise(conv::addr(addr), len, advice))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_madvise(addr: *mut c_void, len: size_t, advice: c_int) -> c_int {
    ret::errnum(io_compat::posix_madvise(advice, |a| raw::madvise(conv::addr(addr), len, a)))
}}

export! { "GLIBC_2.36";
pub extern "C" fn process_madvise(pidfd: c_int, iov: *const c_void, n: size_t, advice: c_int, flags: c_uint) -> ssize_t {
    // UNSAFE(slice) SAFETY: iov holds n iovec entries naming ranges of the target process (§11.10).
    ret::ssize(unsafe { conv::array::<RemoteIoVec>(iov, conv::count_sz(n)) }.and_then(|v| conv::remote_advice(advice).and_then(|a| CSys.process_madvise(conv::fd(pidfd), v, a, flags))))
}}

export! { "GLIBC_2.36";
pub extern "C" fn process_mrelease(pidfd: c_int, flags: c_uint) -> c_int {
    ret::zero(CSys.process_mrelease(conv::fd(pidfd), flags))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_mremap5(old: *mut c_void, old_len: size_t, new_len: size_t, flags: c_int, new_addr: *mut c_void) -> *mut c_void {
    ret::addr_or_neg1(raw::mremap_c(conv::addr(old), old_len, new_len, flags, conv::addr(new_addr)))
}}

export! { "GLIBC_2.3.3";
pub extern "C" fn remap_file_pages(addr: *mut c_void, size: size_t, prot: c_int, pgoff: size_t, flags: c_int) -> c_int {
    ret::zero(raw::remap_file_pages(conv::addr(addr), size, prot, pgoff, flags))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn msync(addr: *mut c_void, len: size_t, flags: c_int) -> c_int {
    ret::zero(raw::msync(conv::addr(addr), len, flags))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mincore(addr: *mut c_void, len: size_t, vec: *mut u8) -> c_int {
    // UNSAFE(slice) SAFETY: vec holds one byte per page of the range (§11.10).
    ret::zero(unsafe { conv::mincore_vec(vec, len) }.and_then(|v| raw::mincore(conv::addr(addr), len, v)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mlock(addr: *const c_void, len: size_t) -> c_int {
    ret::zero(raw::mlock(conv::addr(addr), len))
}}

export! { "GLIBC_2.27";
pub extern "C" fn mlock2(addr: *const c_void, len: size_t, flags: c_uint) -> c_int {
    ret::zero(raw::mlock2(conv::addr(addr), len, flags))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn munlock(addr: *const c_void, len: size_t) -> c_int {
    ret::zero(raw::munlock(conv::addr(addr), len))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mlockall(flags: c_int) -> c_int {
    ret::zero(CSys.mlockall(conv::bits(flags)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn munlockall() -> c_int {
    ret::zero(CSys.munlockall())
}}

export! { "GLIBC_2.27";
pub extern "C" fn memfd_create(name: *const c_char, flags: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: name is a NUL-terminated string (§11.10).
    ret::fd(unsafe { strz::path(name) }.and_then(|n| CSys.memfd_create(n, flags)))
}}

export! { "GLIBC_2.15";
pub extern "C" fn process_vm_readv(pid: c_int, local: *mut c_void, liovcnt: c_ulong, remote: *const c_void, riovcnt: c_ulong, flags: c_ulong) -> ssize_t {
    // UNSAFE(slice) SAFETY: local and remote hold liovcnt and riovcnt iovec entries; the local ones describe valid memory (§11.10).
    ret::ssize(unsafe { conv::array_mut::<IoVecMut<'_>>(local, conv::count_ul(liovcnt)) }.and_then(|l| unsafe { conv::array::<RemoteIoVec>(remote, conv::count_ul(riovcnt)) }.and_then(|r| CSys.process_vm_readv(pid, l, r, conv::flags_ul(flags)))))
}}

export! { "GLIBC_2.15";
pub extern "C" fn process_vm_writev(pid: c_int, local: *const c_void, liovcnt: c_ulong, remote: *const c_void, riovcnt: c_ulong, flags: c_ulong) -> ssize_t {
    // UNSAFE(slice) SAFETY: local and remote hold liovcnt and riovcnt iovec entries; the local ones describe valid memory (§11.10).
    ret::ssize(unsafe { conv::array::<IoVec<'_>>(local, conv::count_ul(liovcnt)) }.and_then(|l| unsafe { conv::array::<RemoteIoVec>(remote, conv::count_ul(riovcnt)) }.and_then(|r| raw::process_vm_writev(pid, l, r, conv::flags_ul(flags)))))
}}

export! { "GLIBC_2.7";
pub extern "C" fn eventfd(initval: c_uint, flags: c_int) -> c_int {
    ret::fd(CSys.eventfd2(initval, conv::bits(flags)))
}}

export! { "GLIBC_2.7";
pub extern "C" fn eventfd_read(fd: c_int, value: Option<&mut u64>) -> c_int {
    ret::zero(conv::req(value).and_then(|v| io_compat::eventfd_read(&CSys, conv::fd(fd), v)))
}}

export! { "GLIBC_2.7";
pub extern "C" fn eventfd_write(fd: c_int, value: u64) -> c_int {
    ret::zero(io_compat::eventfd_write(&CSys, conv::fd(fd), value))
}}

export! { "GLIBC_2.4";
pub extern "C" fn inotify_init() -> c_int {
    ret::fd(CSys.inotify_init1(0))
}}

export! { "GLIBC_2.9";
pub extern "C" fn inotify_init1(flags: c_int) -> c_int {
    ret::fd(CSys.inotify_init1(conv::bits(flags)))
}}

export! { "GLIBC_2.4";
pub extern "C" fn inotify_add_watch(fd: c_int, path: *const c_char, mask: u32) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§11.1).
    ret::int32(unsafe { strz::path(path) }.and_then(|p| CSys.inotify_add_watch(conv::fd(fd), p, mask)))
}}

export! { "GLIBC_2.4";
pub extern "C" fn inotify_rm_watch(fd: c_int, wd: c_int) -> c_int {
    ret::zero(CSys.inotify_rm_watch(conv::fd(fd), wd))
}}

export! { "GLIBC_2.13";
pub extern "C" fn fanotify_init(flags: c_uint, event_flags: c_uint) -> c_int {
    ret::fd(CSys.fanotify_init(flags, event_flags))
}}

export! { "GLIBC_2.13";
pub extern "C" fn fanotify_mark(fd: c_int, flags: c_uint, mask: u64, dirfd: c_int, path: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: path is NULL or a NUL-terminated string (§11.12).
    ret::zero(unsafe { strz::opt_path(path) }.and_then(|p| CSys.fanotify_mark(conv::fd(fd), flags, mask, conv::fd(dirfd), p)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn flock(fd: c_int, op: c_int) -> c_int {
    ret::zero(CSys.flock(conv::fd(fd), conv::bits(op)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn lockf(fd: c_int, cmd: c_int, len: i64) -> c_int {
    ret::zero(io_compat::lockf(&CSys, conv::fd(fd), cmd, len))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn lockf64(fd: c_int, cmd: c_int, len: i64) -> c_int {
    ret::zero(io_compat::lockf(&CSys, conv::fd(fd), cmd, len))
}}

// ------------------------------------------------------------------------------ asynchronous I/O (§11.11)
//
// `crate::aio` (D-11.8). The `struct aiocb` and the `sigevent` attributes it names are read as
// words there (`conv::named_words`): the AIO contract keeps a submitted block valid and
// unchanged until the request completes. The `64` forms are the same functions (`struct
// aiocb64` is `struct aiocb`, with a 64-bit offset in both). The default versions are
// GLIBC_2.34 (formerly librt); the compat versions arrive with phase 15.

export! { "GLIBC_2.34";
pub extern "C" fn aio_read(cb: *mut c_void) -> c_int {
    ret::zero(aio::read(conv::addr(cb)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn aio_read64(cb: *mut c_void) -> c_int {
    ret::zero(aio::read(conv::addr(cb)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn aio_write(cb: *mut c_void) -> c_int {
    ret::zero(aio::write(conv::addr(cb)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn aio_write64(cb: *mut c_void) -> c_int {
    ret::zero(aio::write(conv::addr(cb)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn aio_fsync(op: c_int, cb: *mut c_void) -> c_int {
    ret::zero(aio::fsync(op, conv::addr(cb)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn aio_fsync64(op: c_int, cb: *mut c_void) -> c_int {
    ret::zero(aio::fsync(op, conv::addr(cb)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn aio_error(cb: *const c_void) -> c_int {
    ret::int32(aio::error(conv::addr(cb)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn aio_error64(cb: *const c_void) -> c_int {
    ret::int32(aio::error(conv::addr(cb)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn aio_return(cb: *mut c_void) -> ssize_t {
    ret::with_errno(aio::status(conv::addr(cb)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn aio_return64(cb: *mut c_void) -> ssize_t {
    ret::with_errno(aio::status(conv::addr(cb)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn aio_suspend(list: *const *const c_void, nent: c_int, timeout: Option<&Timespec>) -> c_int {
    // UNSAFE(slice) SAFETY: list holds nent entries, each NULL or a struct aiocb (aio.h).
    cancel::point(ret::zero(aio::suspend(nent, unsafe { conv::elements(list.cast(), conv::nonneg(nent), 8) }, timeout)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn aio_suspend64(list: *const *const c_void, nent: c_int, timeout: Option<&Timespec>) -> c_int {
    // UNSAFE(slice) SAFETY: list holds nent entries, each NULL or a struct aiocb64 (aio.h).
    cancel::point(ret::zero(aio::suspend(nent, unsafe { conv::elements(list.cast(), conv::nonneg(nent), 8) }, timeout)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn aio_cancel(fd: c_int, cb: *mut c_void) -> c_int {
    ret::int32(aio::cancel(conv::fd(fd), conv::addr(cb)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn aio_cancel64(fd: c_int, cb: *mut c_void) -> c_int {
    ret::int32(aio::cancel(conv::fd(fd), conv::addr(cb)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn lio_listio(mode: c_int, list: *const *mut c_void, nent: c_int, sig: Option<&SigEvent>) -> c_int {
    // UNSAFE(slice) SAFETY: list holds nent entries, each NULL or a struct aiocb (aio.h).
    cancel::point(ret::zero(aio::listio(mode, unsafe { conv::elements(list.cast(), conv::nonneg(nent), 8) }, sig)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn lio_listio64(mode: c_int, list: *const *mut c_void, nent: c_int, sig: Option<&SigEvent>) -> c_int {
    // UNSAFE(slice) SAFETY: list holds nent entries, each NULL or a struct aiocb64 (aio.h).
    cancel::point(ret::zero(aio::listio(mode, unsafe { conv::elements(list.cast(), conv::nonneg(nent), 8) }, sig)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn aio_init(init: Option<&struct_aioinit>) {
    aio::init(init.map(|i| i.aio_threads))
}}
