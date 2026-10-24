//! Translation for spec §13: the thin file-system calls (phase 5: §13.4–13.7, §13.10–13.12,
//! §13.14–13.15, and the lengths of §11.8 whose symbols are listed here).
//!
//! The non-`at` forms pass `AT_FDCWD`. Thin wrappers are the typed `Sys` methods; what
//! glibc's contract adds (`remove`, `mkfifo`, `mknod`'s device check, the no-follow `chmod`,
//! `faccessat`'s flags, the µs and second timestamp forms, `statvfs`, the `__xstat` versions)
//! is in `liella_legacy::fs_compat`. Structures are typed parameters (D-X.8): `struct stat` is
//! the kernel's on x86-64.
//!
//! Phase 12 adds the working directory, directory streams, `scandir`, the walks, `realpath`
//! and the temporary names (§13.1–13.3, §13.8–13.9) through `crate::dirtree`. `DIR *` and
//! `FTS *` are opaque handles (`*mut c_void`); the `*64` names are second exports of the same
//! translations (D-C0.4). The mount-table streams and the `fsent` functions (§13.13, D-13.15)
//! are `crate::mntent`; the `__` names are second exports of the same translations.

use liella::bytes;
use liella::fs;
use liella_abi::callback::{DirFilterFn, FtwFn, NftwFn, SlotCompareFn};
use liella_abi::ctypes::{c_char, c_int, c_long, c_uint, c_ulong, c_void, size_t, ssize_t};
use liella_abi::layout::{struct_dirent, struct_dirent64, struct_fstab, struct_mntent, struct_statvfs, FILE};
use liella_kernel::path::PATH_MAX;
use liella_legacy::fs_tree;
use liella_kernel::fd::Fd;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Stat, Statfs, Statx, Timespec, Timeval, AT_EACCESS, AT_REMOVEDIR, AT_SYMLINK_NOFOLLOW};
use liella_legacy::fs_compat;
use liella_legacy::stubs;

use crate::conv;
use crate::dirtree;
use crate::export;
use crate::mntent;
use crate::raw;
use crate::ret;
use crate::strz;
use crate::sys::CSys;

export! { "GLIBC_2.2.5";
pub extern "C" fn ftruncate(fd: c_int, len: i64) -> c_int {
    ret::zero(CSys.ftruncate(conv::fd(fd), len))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ftruncate64(fd: c_int, len: i64) -> c_int {
    ret::zero(CSys.ftruncate(conv::fd(fd), len))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn truncate(path: *const c_char, len: i64) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| CSys.truncate(p, len)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn truncate64(path: *const c_char, len: i64) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| CSys.truncate(p, len)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn link(old: *const c_char, new: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: both paths are NUL-terminated strings (§13.4).
    ret::zero(unsafe { strz::path(old) }.and_then(|o| unsafe { strz::path(new) }.and_then(|n| CSys.linkat(Fd::CWD, o, Fd::CWD, n, 0))))
}}

export! { "GLIBC_2.4";
pub extern "C" fn linkat(olddir: c_int, old: *const c_char, newdir: c_int, new: *const c_char, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: both paths are NUL-terminated strings (§13.4).
    ret::zero(unsafe { strz::path(old) }.and_then(|o| unsafe { strz::path(new) }.and_then(|n| CSys.linkat(conv::fd(olddir), o, conv::fd(newdir), n, conv::bits(flags)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn symlink(target: *const c_char, path: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: both paths are NUL-terminated strings (§13.4).
    ret::zero(unsafe { strz::path(target) }.and_then(|t| unsafe { strz::path(path) }.and_then(|p| CSys.symlinkat(t, Fd::CWD, p))))
}}

export! { "GLIBC_2.4";
pub extern "C" fn symlinkat(target: *const c_char, dirfd: c_int, path: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: both paths are NUL-terminated strings (§13.4).
    ret::zero(unsafe { strz::path(target) }.and_then(|t| unsafe { strz::path(path) }.and_then(|p| CSys.symlinkat(t, conv::fd(dirfd), p))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn readlink(path: *const c_char, buf: *mut c_char, n: size_t) -> ssize_t {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string; buf holds n bytes (§13.4).
    ret::ssize(unsafe { strz::path(path) }.and_then(|p| unsafe { conv::buf_mut(buf.cast(), n) }.and_then(|b| CSys.readlinkat(Fd::CWD, p, b))))
}}

export! { "GLIBC_2.4";
pub extern "C" fn readlinkat(dirfd: c_int, path: *const c_char, buf: *mut c_char, n: size_t) -> ssize_t {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string; buf holds n bytes (§13.4).
    ret::ssize(unsafe { strz::path(path) }.and_then(|p| unsafe { conv::buf_mut(buf.cast(), n) }.and_then(|b| CSys.readlinkat(conv::fd(dirfd), p, b))))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __readlink_chk(path: *const c_char, buf: *mut c_char, n: size_t, buflen: size_t) -> ssize_t {
    // UNSAFE(slice) SAFETY: as readlink, after the fortify check.
    ret::ssize(unsafe { strz::path(path) }.and_then(|p| unsafe { conv::buf_mut(buf.cast(), conv::chk(n, buflen)) }.and_then(|b| CSys.readlinkat(Fd::CWD, p, b))))
}}

export! { "GLIBC_2.5";
pub extern "C" fn __readlinkat_chk(dirfd: c_int, path: *const c_char, buf: *mut c_char, n: size_t, buflen: size_t) -> ssize_t {
    // UNSAFE(slice) SAFETY: as readlinkat, after the fortify check.
    ret::ssize(unsafe { strz::path(path) }.and_then(|p| unsafe { conv::buf_mut(buf.cast(), conv::chk(n, buflen)) }.and_then(|b| CSys.readlinkat(conv::fd(dirfd), p, b))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn unlink(path: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| CSys.unlinkat(Fd::CWD, p, 0)))
}}

export! { "GLIBC_2.4";
pub extern "C" fn unlinkat(dirfd: c_int, path: *const c_char, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| CSys.unlinkat(conv::fd(dirfd), p, conv::bits(flags))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn rmdir(path: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| CSys.unlinkat(Fd::CWD, p, AT_REMOVEDIR)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn remove(path: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| fs_compat::remove(&CSys, p)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn rename(old: *const c_char, new: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: both paths are NUL-terminated strings (§13.4).
    ret::zero(unsafe { strz::path(old) }.and_then(|o| unsafe { strz::path(new) }.and_then(|n| CSys.renameat2(Fd::CWD, o, Fd::CWD, n, 0))))
}}

export! { "GLIBC_2.4";
pub extern "C" fn renameat(olddir: c_int, old: *const c_char, newdir: c_int, new: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: both paths are NUL-terminated strings (§13.4).
    ret::zero(unsafe { strz::path(old) }.and_then(|o| unsafe { strz::path(new) }.and_then(|n| CSys.renameat2(conv::fd(olddir), o, conv::fd(newdir), n, 0))))
}}

export! { "GLIBC_2.28";
pub extern "C" fn renameat2(olddir: c_int, old: *const c_char, newdir: c_int, new: *const c_char, flags: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: both paths are NUL-terminated strings (§13.4).
    ret::zero(unsafe { strz::path(old) }.and_then(|o| unsafe { strz::path(new) }.and_then(|n| CSys.renameat2(conv::fd(olddir), o, conv::fd(newdir), n, flags))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mkdir(path: *const c_char, mode: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| CSys.mkdirat(Fd::CWD, p, mode)))
}}

export! { "GLIBC_2.4";
pub extern "C" fn mkdirat(dirfd: c_int, path: *const c_char, mode: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| CSys.mkdirat(conv::fd(dirfd), p, mode)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mkfifo(path: *const c_char, mode: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| fs_compat::mkfifoat(&CSys, Fd::CWD, p, mode)))
}}

export! { "GLIBC_2.4";
pub extern "C" fn mkfifoat(dirfd: c_int, path: *const c_char, mode: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| fs_compat::mkfifoat(&CSys, conv::fd(dirfd), p, mode)))
}}

export! { "GLIBC_2.33";
pub extern "C" fn mknod(path: *const c_char, mode: c_uint, dev: u64) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| fs_compat::mknodat(&CSys, Fd::CWD, p, mode, dev)))
}}

export! { "GLIBC_2.33";
pub extern "C" fn mknodat(dirfd: c_int, path: *const c_char, mode: c_uint, dev: u64) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| fs_compat::mknodat(&CSys, conv::fd(dirfd), p, mode, dev)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __xmknod(vers: c_int, path: *const c_char, mode: c_uint, dev: Option<&u64>) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(fs_compat::mknod_version(vers).and_then(|()| conv::req(dev)).and_then(|d| unsafe { strz::path(path) }.and_then(|p| fs_compat::mknodat(&CSys, Fd::CWD, p, mode, conv::copy(d)))))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __xmknodat(vers: c_int, dirfd: c_int, path: *const c_char, mode: c_uint, dev: Option<&u64>) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(fs_compat::mknod_version(vers).and_then(|()| conv::req(dev)).and_then(|d| unsafe { strz::path(path) }.and_then(|p| fs_compat::mknodat(&CSys, conv::fd(dirfd), p, mode, conv::copy(d)))))
}}

export! { "GLIBC_2.3.3";
pub extern "C" fn gnu_dev_major(dev: u64) -> c_uint {
    fs::major(dev)
}}

export! { "GLIBC_2.3.3";
pub extern "C" fn gnu_dev_minor(dev: u64) -> c_uint {
    fs::minor(dev)
}}

export! { "GLIBC_2.3.3";
pub extern "C" fn gnu_dev_makedev(major: c_uint, minor: c_uint) -> u64 {
    fs::makedev(major, minor)
}}

export! { "GLIBC_2.33";
pub extern "C" fn stat(path: *const c_char, buf: Option<&mut Stat>) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::store_into(buf, |b| unsafe { strz::path(path) }.and_then(|p| CSys.fstatat_into(Fd::CWD, p, 0, b)))
}}

export! { "GLIBC_2.33";
pub extern "C" fn stat64(path: *const c_char, buf: Option<&mut Stat>) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::store_into(buf, |b| unsafe { strz::path(path) }.and_then(|p| CSys.fstatat_into(Fd::CWD, p, 0, b)))
}}

export! { "GLIBC_2.33";
pub extern "C" fn lstat(path: *const c_char, buf: Option<&mut Stat>) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::store_into(buf, |b| unsafe { strz::path(path) }.and_then(|p| CSys.fstatat_into(Fd::CWD, p, AT_SYMLINK_NOFOLLOW, b)))
}}

export! { "GLIBC_2.33";
pub extern "C" fn lstat64(path: *const c_char, buf: Option<&mut Stat>) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::store_into(buf, |b| unsafe { strz::path(path) }.and_then(|p| CSys.fstatat_into(Fd::CWD, p, AT_SYMLINK_NOFOLLOW, b)))
}}

export! { "GLIBC_2.33";
pub extern "C" fn fstat(fd: c_int, buf: Option<&mut Stat>) -> c_int {
    ret::store_into(buf, |b| CSys.fstat_into(conv::fd(fd), b))
}}

export! { "GLIBC_2.33";
pub extern "C" fn fstat64(fd: c_int, buf: Option<&mut Stat>) -> c_int {
    ret::store_into(buf, |b| CSys.fstat_into(conv::fd(fd), b))
}}

export! { "GLIBC_2.33";
pub extern "C" fn fstatat(dirfd: c_int, path: *const c_char, buf: Option<&mut Stat>, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::store_into(buf, |b| unsafe { strz::path(path) }.and_then(|p| CSys.fstatat_into(conv::fd(dirfd), p, conv::bits(flags), b)))
}}

export! { "GLIBC_2.33";
pub extern "C" fn fstatat64(dirfd: c_int, path: *const c_char, buf: Option<&mut Stat>, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::store_into(buf, |b| unsafe { strz::path(path) }.and_then(|p| CSys.fstatat_into(conv::fd(dirfd), p, conv::bits(flags), b)))
}}

export! { "GLIBC_2.28";
pub extern "C" fn statx(dirfd: c_int, path: *const c_char, flags: c_int, mask: c_uint, buf: Option<&mut Statx>) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::store_into(buf, |b| unsafe { strz::path(path) }.and_then(|p| CSys.statx_into(conv::fd(dirfd), p, conv::bits(flags), mask, b)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __xstat(vers: c_int, path: *const c_char, buf: Option<&mut Stat>) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::store_into(buf, |b| fs_compat::stat_version(vers).and_then(|()| unsafe { strz::path(path) }).and_then(|p| CSys.fstatat_into(Fd::CWD, p, 0, b)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __xstat64(vers: c_int, path: *const c_char, buf: Option<&mut Stat>) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::store_into(buf, |b| fs_compat::stat_version(vers).and_then(|()| unsafe { strz::path(path) }).and_then(|p| CSys.fstatat_into(Fd::CWD, p, 0, b)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __lxstat(vers: c_int, path: *const c_char, buf: Option<&mut Stat>) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::store_into(buf, |b| fs_compat::stat_version(vers).and_then(|()| unsafe { strz::path(path) }).and_then(|p| CSys.fstatat_into(Fd::CWD, p, AT_SYMLINK_NOFOLLOW, b)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __lxstat64(vers: c_int, path: *const c_char, buf: Option<&mut Stat>) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::store_into(buf, |b| fs_compat::stat_version(vers).and_then(|()| unsafe { strz::path(path) }).and_then(|p| CSys.fstatat_into(Fd::CWD, p, AT_SYMLINK_NOFOLLOW, b)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __fxstat(vers: c_int, fd: c_int, buf: Option<&mut Stat>) -> c_int {
    ret::store_into(buf, |b| fs_compat::stat_version(vers).and_then(|()| CSys.fstat_into(conv::fd(fd), b)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __fxstat64(vers: c_int, fd: c_int, buf: Option<&mut Stat>) -> c_int {
    ret::store_into(buf, |b| fs_compat::stat_version(vers).and_then(|()| CSys.fstat_into(conv::fd(fd), b)))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __fxstatat(vers: c_int, dirfd: c_int, path: *const c_char, buf: Option<&mut Stat>, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::store_into(buf, |b| fs_compat::stat_version(vers).and_then(|()| unsafe { strz::path(path) }).and_then(|p| CSys.fstatat_into(conv::fd(dirfd), p, conv::bits(flags), b)))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __fxstatat64(vers: c_int, dirfd: c_int, path: *const c_char, buf: Option<&mut Stat>, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::store_into(buf, |b| fs_compat::stat_version(vers).and_then(|()| unsafe { strz::path(path) }).and_then(|p| CSys.fstatat_into(conv::fd(dirfd), p, conv::bits(flags), b)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn chmod(path: *const c_char, mode: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| CSys.fchmodat(Fd::CWD, p, mode)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fchmod(fd: c_int, mode: c_uint) -> c_int {
    ret::zero(CSys.fchmod(conv::fd(fd), mode))
}}

export! { "GLIBC_2.4";
pub extern "C" fn fchmodat(dirfd: c_int, path: *const c_char, mode: c_uint, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| fs_compat::fchmodat(&CSys, conv::fd(dirfd), p, mode, conv::bits(flags))))
}}

export! { "GLIBC_2.3.2";
pub extern "C" fn lchmod(path: *const c_char, mode: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| fs_compat::fchmodat(&CSys, Fd::CWD, p, mode, AT_SYMLINK_NOFOLLOW)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn chown(path: *const c_char, uid: c_uint, gid: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| CSys.fchownat(Fd::CWD, p, uid, gid, 0)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn lchown(path: *const c_char, uid: c_uint, gid: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| CSys.fchownat(Fd::CWD, p, uid, gid, AT_SYMLINK_NOFOLLOW)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fchown(fd: c_int, uid: c_uint, gid: c_uint) -> c_int {
    ret::zero(CSys.fchown(conv::fd(fd), uid, gid))
}}

export! { "GLIBC_2.4";
pub extern "C" fn fchownat(dirfd: c_int, path: *const c_char, uid: c_uint, gid: c_uint, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| CSys.fchownat(conv::fd(dirfd), p, uid, gid, conv::bits(flags))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn umask(mask: c_uint) -> c_uint {
    CSys.umask(mask)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn access(path: *const c_char, mode: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| fs_compat::faccessat(&CSys, Fd::CWD, p, conv::bits(mode), 0)))
}}

export! { "GLIBC_2.4";
pub extern "C" fn faccessat(dirfd: c_int, path: *const c_char, mode: c_int, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| fs_compat::faccessat(&CSys, conv::fd(dirfd), p, conv::bits(mode), conv::bits(flags))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn euidaccess(path: *const c_char, mode: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| fs_compat::faccessat(&CSys, Fd::CWD, p, conv::bits(mode), AT_EACCESS)))
}}

export! { "GLIBC_2.4";
pub extern "C" fn eaccess(path: *const c_char, mode: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| fs_compat::faccessat(&CSys, Fd::CWD, p, conv::bits(mode), AT_EACCESS)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn utime(path: *const c_char, times: Option<&[i64; 2]>) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| CSys.utimensat(Fd::CWD, Some(p), fs_compat::utimbuf(times).as_ref(), 0)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn utimes(path: *const c_char, tv: Option<&[Timeval; 2]>) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| fs_compat::timevals(tv).and_then(|t| CSys.utimensat(Fd::CWD, Some(p), t.as_ref(), 0))))
}}

export! { "GLIBC_2.3";
pub extern "C" fn lutimes(path: *const c_char, tv: Option<&[Timeval; 2]>) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| fs_compat::timevals(tv).and_then(|t| CSys.utimensat(Fd::CWD, Some(p), t.as_ref(), AT_SYMLINK_NOFOLLOW))))
}}

export! { "GLIBC_2.3";
pub extern "C" fn futimes(fd: c_int, tv: Option<&[Timeval; 2]>) -> c_int {
    ret::zero(conv::file_fd(fd).and_then(|fd| fs_compat::timevals(tv).and_then(|t| CSys.utimensat(fd, None, t.as_ref(), 0))))
}}

export! { "GLIBC_2.4";
pub extern "C" fn futimesat(dirfd: c_int, path: *const c_char, tv: Option<&[Timeval; 2]>) -> c_int {
    // UNSAFE(slice) SAFETY: path is NULL or a NUL-terminated string (§13.7).
    ret::zero(unsafe { strz::opt_path(path) }.and_then(|p| fs_compat::timevals(tv).and_then(|t| CSys.utimensat(conv::fd(dirfd), p, t.as_ref(), 0))))
}}

export! { "GLIBC_2.6";
pub extern "C" fn utimensat(dirfd: c_int, path: *const c_char, times: Option<&[Timespec; 2]>, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is NULL or a NUL-terminated string (§13.7).
    ret::zero(unsafe { strz::opt_path(path) }.and_then(|p| CSys.utimensat(conv::fd(dirfd), p, times, conv::bits(flags))))
}}

export! { "GLIBC_2.6";
pub extern "C" fn futimens(fd: c_int, times: Option<&[Timespec; 2]>) -> c_int {
    ret::zero(conv::file_fd(fd).and_then(|fd| CSys.utimensat(fd, None, times, 0)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn statfs(path: *const c_char, buf: Option<&mut Statfs>) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::store(buf, unsafe { strz::path(path) }.and_then(|p| CSys.statfs(p)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn statfs64(path: *const c_char, buf: Option<&mut Statfs>) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::store(buf, unsafe { strz::path(path) }.and_then(|p| CSys.statfs(p)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __statfs(path: *const c_char, buf: Option<&mut Statfs>) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::store(buf, unsafe { strz::path(path) }.and_then(|p| CSys.statfs(p)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fstatfs(fd: c_int, buf: Option<&mut Statfs>) -> c_int {
    ret::store(buf, CSys.fstatfs(conv::fd(fd)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fstatfs64(fd: c_int, buf: Option<&mut Statfs>) -> c_int {
    ret::store(buf, CSys.fstatfs(conv::fd(fd)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn statvfs(path: *const c_char, buf: Option<&mut struct_statvfs>) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::store(buf, unsafe { strz::path(path) }.and_then(|p| CSys.statfs(p)).map(|s| fs_compat::statvfs(&s)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn statvfs64(path: *const c_char, buf: Option<&mut struct_statvfs>) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::store(buf, unsafe { strz::path(path) }.and_then(|p| CSys.statfs(p)).map(|s| fs_compat::statvfs(&s)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fstatvfs(fd: c_int, buf: Option<&mut struct_statvfs>) -> c_int {
    ret::store(buf, CSys.fstatfs(conv::fd(fd)).map(|s| fs_compat::statvfs(&s)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fstatvfs64(fd: c_int, buf: Option<&mut struct_statvfs>) -> c_int {
    ret::store(buf, CSys.fstatfs(conv::fd(fd)).map(|s| fs_compat::statvfs(&s)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ustat(dev: u64, buf: Option<&mut [u8; 32]>) -> c_int {
    ret::store(buf, fs_compat::kernel_dev(dev).and_then(raw::ustat))
}}

export! { "GLIBC_2.3";
pub extern "C" fn setxattr(path: *const c_char, name: *const c_char, value: *const c_void, size: size_t, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path and name are NUL-terminated strings; value holds size bytes (§13.11).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| unsafe { strz::path(name) }.and_then(|n| unsafe { conv::buf(value, size) }.and_then(|v| CSys.setxattr(p, false, n, v, conv::bits(flags))))))
}}

export! { "GLIBC_2.3";
pub extern "C" fn lsetxattr(path: *const c_char, name: *const c_char, value: *const c_void, size: size_t, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path and name are NUL-terminated strings; value holds size bytes (§13.11).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| unsafe { strz::path(name) }.and_then(|n| unsafe { conv::buf(value, size) }.and_then(|v| CSys.setxattr(p, true, n, v, conv::bits(flags))))))
}}

export! { "GLIBC_2.3";
pub extern "C" fn fsetxattr(fd: c_int, name: *const c_char, value: *const c_void, size: size_t, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: name is a NUL-terminated string; value holds size bytes (§13.11).
    ret::zero(unsafe { strz::path(name) }.and_then(|n| unsafe { conv::buf(value, size) }.and_then(|v| CSys.fsetxattr(conv::fd(fd), n, v, conv::bits(flags)))))
}}

export! { "GLIBC_2.3";
pub extern "C" fn getxattr(path: *const c_char, name: *const c_char, value: *mut c_void, size: size_t) -> ssize_t {
    // UNSAFE(slice) SAFETY: path and name are NUL-terminated strings; value holds size bytes (§13.11).
    ret::ssize(unsafe { strz::path(path) }.and_then(|p| unsafe { strz::path(name) }.and_then(|n| unsafe { conv::buf_mut(value, size) }.and_then(|v| CSys.getxattr(p, false, n, v)))))
}}

export! { "GLIBC_2.3";
pub extern "C" fn lgetxattr(path: *const c_char, name: *const c_char, value: *mut c_void, size: size_t) -> ssize_t {
    // UNSAFE(slice) SAFETY: path and name are NUL-terminated strings; value holds size bytes (§13.11).
    ret::ssize(unsafe { strz::path(path) }.and_then(|p| unsafe { strz::path(name) }.and_then(|n| unsafe { conv::buf_mut(value, size) }.and_then(|v| CSys.getxattr(p, true, n, v)))))
}}

export! { "GLIBC_2.3";
pub extern "C" fn fgetxattr(fd: c_int, name: *const c_char, value: *mut c_void, size: size_t) -> ssize_t {
    // UNSAFE(slice) SAFETY: name is a NUL-terminated string; value holds size bytes (§13.11).
    ret::ssize(unsafe { strz::path(name) }.and_then(|n| unsafe { conv::buf_mut(value, size) }.and_then(|v| CSys.fgetxattr(conv::fd(fd), n, v))))
}}

export! { "GLIBC_2.3";
pub extern "C" fn listxattr(path: *const c_char, list: *mut c_char, size: size_t) -> ssize_t {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string; list holds size bytes (§13.11).
    ret::ssize(unsafe { strz::path(path) }.and_then(|p| unsafe { conv::buf_mut(list.cast(), size) }.and_then(|l| CSys.listxattr(p, false, l))))
}}

export! { "GLIBC_2.3";
pub extern "C" fn llistxattr(path: *const c_char, list: *mut c_char, size: size_t) -> ssize_t {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string; list holds size bytes (§13.11).
    ret::ssize(unsafe { strz::path(path) }.and_then(|p| unsafe { conv::buf_mut(list.cast(), size) }.and_then(|l| CSys.listxattr(p, true, l))))
}}

export! { "GLIBC_2.3";
pub extern "C" fn flistxattr(fd: c_int, list: *mut c_char, size: size_t) -> ssize_t {
    // UNSAFE(slice) SAFETY: list holds size bytes (§13.11).
    ret::ssize(unsafe { conv::buf_mut(list.cast(), size) }.and_then(|l| CSys.flistxattr(conv::fd(fd), l)))
}}

export! { "GLIBC_2.3";
pub extern "C" fn removexattr(path: *const c_char, name: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: path and name are NUL-terminated strings (§13.11).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| unsafe { strz::path(name) }.and_then(|n| CSys.removexattr(p, false, n))))
}}

export! { "GLIBC_2.3";
pub extern "C" fn lremovexattr(path: *const c_char, name: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: path and name are NUL-terminated strings (§13.11).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| unsafe { strz::path(name) }.and_then(|n| CSys.removexattr(p, true, n))))
}}

export! { "GLIBC_2.3";
pub extern "C" fn fremovexattr(fd: c_int, name: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: name is a NUL-terminated string (§13.11).
    ret::zero(unsafe { strz::path(name) }.and_then(|n| CSys.fremovexattr(conv::fd(fd), n)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mount(source: *const c_char, target: *const c_char, fstype: *const c_char, flags: c_ulong, data: *const c_void) -> c_int {
    // UNSAFE(slice) SAFETY: each string argument is NULL (where optional) or NUL-terminated; mount's data is a string (§13.12).
    ret::zero(unsafe { strz::opt_path(source) }.and_then(|s| unsafe { strz::path(target) }.and_then(|t| unsafe { strz::opt_path(fstype) }.and_then(|f| unsafe { strz::opt_path(data.cast()) }.and_then(|d| CSys.mount(s, t, f, flags, d))))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn umount(target: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: each string argument is NULL (where optional) or NUL-terminated; mount's data is a string (§13.12).
    ret::zero(unsafe { strz::path(target) }.and_then(|t| CSys.umount2(t, 0)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn umount2(target: *const c_char, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: each string argument is NULL (where optional) or NUL-terminated; mount's data is a string (§13.12).
    ret::zero(unsafe { strz::path(target) }.and_then(|t| CSys.umount2(t, conv::bits(flags))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pivot_root(new_root: *const c_char, put_old: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: each string argument is NULL (where optional) or NUL-terminated; mount's data is a string (§13.12).
    ret::zero(unsafe { strz::path(new_root) }.and_then(|n| unsafe { strz::path(put_old) }.and_then(|o| CSys.pivot_root(n, o))))
}}

export! { "GLIBC_2.36";
pub extern "C" fn fsopen(fstype: *const c_char, flags: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: each string argument is NULL (where optional) or NUL-terminated; mount's data is a string (§13.12).
    ret::fd(unsafe { strz::path(fstype) }.and_then(|f| CSys.fsopen(f, flags)))
}}

export! { "GLIBC_2.36";
pub extern "C" fn fsconfig(fd: c_int, cmd: c_uint, key: *const c_char, value: *const c_void, aux: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: key is NULL or NUL-terminated; value is what cmd takes (§13.12).
    ret::zero(conv::fsconfig_cmd(cmd).and_then(|c| unsafe { strz::opt_path(key) }.and_then(|k| unsafe { conv::fs_value(c, value, aux) }.and_then(|v| CSys.fsconfig(conv::fd(fd), c, k, v, aux)))))
}}

export! { "GLIBC_2.36";
pub extern "C" fn fsmount(fd: c_int, flags: c_uint, attr_flags: c_uint) -> c_int {
    ret::fd(CSys.fsmount(conv::fd(fd), flags, attr_flags))
}}

export! { "GLIBC_2.36";
pub extern "C" fn fspick(dirfd: c_int, path: *const c_char, flags: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::fd(unsafe { strz::path(path) }.and_then(|p| CSys.fspick(conv::fd(dirfd), p, flags)))
}}

export! { "GLIBC_2.36";
pub extern "C" fn open_tree(dirfd: c_int, path: *const c_char, flags: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::fd(unsafe { strz::path(path) }.and_then(|p| CSys.open_tree(conv::fd(dirfd), p, flags)))
}}

export! { "GLIBC_2.36";
pub extern "C" fn move_mount(fromdir: c_int, from: *const c_char, todir: c_int, to: *const c_char, flags: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: both paths are NUL-terminated strings (§13.4).
    ret::zero(unsafe { strz::path(from) }.and_then(|f| unsafe { strz::path(to) }.and_then(|t| CSys.move_mount(conv::fd(fromdir), f, conv::fd(todir), t, flags))))
}}

export! { "GLIBC_2.36";
pub extern "C" fn mount_setattr(dirfd: c_int, path: *const c_char, flags: c_uint, attr: *mut c_void, size: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: path is NUL-terminated; attr holds size bytes (§13.12).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| unsafe { conv::mount_attr(attr, size) }.and_then(|a| CSys.mount_setattr(conv::fd(dirfd), p, flags, a))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn chflags(_path: *const c_char, _flags: c_ulong) -> c_int {
    ret::zero(stubs::bsd_file_flags())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fchflags(_fd: c_int, _flags: c_ulong) -> c_int {
    ret::zero(stubs::bsd_file_flags())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn swapon(path: *const c_char, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| CSys.swapon(p, conv::bits(flags))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn swapoff(path: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| CSys.swapoff(p)))
}}

// ------------------------------------------------------------------------------ §13.1 working directory

export! { "GLIBC_2.2.5";
pub extern "C" fn getcwd(buf: *mut c_char, size: size_t) -> *mut c_char {
    // UNSAFE(slice) SAFETY: buf is NULL or holds size writable bytes (§13.1).
    ret::null(dirtree::getcwd(unsafe { conv::opt_buf_mut(buf, size) }, size))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __getcwd_chk(buf: *mut c_char, size: size_t, buflen: size_t) -> *mut c_char {
    // UNSAFE(slice) SAFETY: buf is NULL or an object of buflen writable bytes (§13.1, fortified).
    ret::null(dirtree::getcwd_chk(unsafe { conv::opt_buf_mut(buf, size.min(buflen)) }, size, buflen))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getwd(buf: *mut c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: buf is NULL or holds PATH_MAX writable bytes (§13.1).
    ret::null(dirtree::getwd(unsafe { conv::opt_buf_mut(buf, PATH_MAX) }))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __getwd_chk(buf: *mut c_char, buflen: size_t) -> *mut c_char {
    // UNSAFE(slice) SAFETY: buf is NULL or an object of buflen writable bytes (§13.1, fortified).
    ret::null(dirtree::getwd(unsafe { conv::opt_buf_mut(buf, buflen.min(PATH_MAX)) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn get_current_dir_name() -> *mut c_char {
    ret::null(dirtree::get_current_dir_name())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn chdir(path: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13.1).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| CSys.chdir(p)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fchdir(fd: c_int) -> c_int {
    ret::zero(CSys.fchdir(conv::fd(fd)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn chroot(path: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13.1).
    ret::zero(unsafe { strz::path(path) }.and_then(|p| CSys.chroot(p)))
}}

// ------------------------------------------------------------------------------ §13.2 directory streams

export! { "GLIBC_2.2.5";
pub extern "C" fn opendir(path: *const c_char) -> *mut c_void {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13.2).
    ret::null(dirtree::opendir(Fd::CWD, unsafe { strz::path(path) }))
}}

export! { "GLIBC_2.4";
pub extern "C" fn fdopendir(fd: c_int) -> *mut c_void {
    ret::null(dirtree::fdopendir(fd))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn readdir(d: *mut c_void) -> *mut struct_dirent {
    ret::entry(dirtree::readdir(conv::addr(d)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn readdir64(d: *mut c_void) -> *mut struct_dirent64 {
    ret::entry(dirtree::readdir(conv::addr(d)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn readdir_r(d: *mut c_void, entry: Option<&mut struct_dirent>, result: Option<&mut *mut struct_dirent>) -> c_int {
    ret::errnum(dirtree::readdir_r(conv::addr(d), entry, result))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn readdir64_r(d: *mut c_void, entry: Option<&mut struct_dirent64>, result: Option<&mut *mut struct_dirent64>) -> c_int {
    ret::errnum(dirtree::readdir64_r(conv::addr(d), entry, result))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn rewinddir(d: *mut c_void) {
    ret::errno_only(dirtree::rewinddir(conv::addr(d)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn seekdir(d: *mut c_void, pos: c_long) {
    ret::errno_only(dirtree::seekdir(conv::addr(d), pos))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn telldir(d: *mut c_void) -> c_long {
    ret::long(dirtree::telldir(conv::addr(d)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn closedir(d: *mut c_void) -> c_int {
    ret::zero(dirtree::closedir(conv::addr(d)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn dirfd(d: *mut c_void) -> c_int {
    ret::int(dirtree::dirfd(conv::addr(d)))
}}

export! { "GLIBC_2.30";
pub extern "C" fn getdents64(fd: c_int, buf: *mut c_void, n: size_t) -> ssize_t {
    // UNSAFE(slice) SAFETY: buf holds n writable bytes (§13.2).
    ret::ssize(unsafe { conv::buf_mut(buf, n) }.and_then(|b| CSys.getdents64(conv::fd(fd), b)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getdirentries(fd: c_int, buf: *mut c_char, n: size_t, basep: Option<&mut i64>) -> ssize_t {
    // UNSAFE(slice) SAFETY: buf holds n writable bytes (§13.2).
    ret::ssize(dirtree::getdirentries(fd, unsafe { conv::buf_mut(buf.cast(), n) }, basep))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getdirentries64(fd: c_int, buf: *mut c_char, n: size_t, basep: Option<&mut i64>) -> ssize_t {
    // UNSAFE(slice) SAFETY: buf holds n writable bytes (§13.2).
    ret::ssize(dirtree::getdirentries(fd, unsafe { conv::buf_mut(buf.cast(), n) }, basep))
}}

// ------------------------------------------------------------------------------ §13.3 scandir, walks

export! { "GLIBC_2.2.5";
pub extern "C" fn scandir(dir: *const c_char, list: Option<&mut *mut *mut struct_dirent>, filter: Option<DirFilterFn>, compar: Option<SlotCompareFn>) -> c_int {
    // UNSAFE(slice) SAFETY: dir is a NUL-terminated string (§13.3).
    ret::int(dirtree::scandirat(Fd::CWD, unsafe { strz::path(dir) }, list, filter, compar))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn scandir64(dir: *const c_char, list: Option<&mut *mut *mut struct_dirent64>, filter: Option<DirFilterFn>, compar: Option<SlotCompareFn>) -> c_int {
    // UNSAFE(slice) SAFETY: dir is a NUL-terminated string (§13.3).
    ret::int(dirtree::scandirat(Fd::CWD, unsafe { strz::path(dir) }, list, filter, compar))
}}

export! { "GLIBC_2.15";
pub extern "C" fn scandirat(dfd: c_int, dir: *const c_char, list: Option<&mut *mut *mut struct_dirent>, filter: Option<DirFilterFn>, compar: Option<SlotCompareFn>) -> c_int {
    // UNSAFE(slice) SAFETY: dir is a NUL-terminated string (§13.3).
    ret::int(dirtree::scandirat(conv::fd(dfd), unsafe { strz::path(dir) }, list, filter, compar))
}}

export! { "GLIBC_2.15";
pub extern "C" fn scandirat64(dfd: c_int, dir: *const c_char, list: Option<&mut *mut *mut struct_dirent64>, filter: Option<DirFilterFn>, compar: Option<SlotCompareFn>) -> c_int {
    // UNSAFE(slice) SAFETY: dir is a NUL-terminated string (§13.3).
    ret::int(dirtree::scandirat(conv::fd(dfd), unsafe { strz::path(dir) }, list, filter, compar))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn alphasort(a: *const *const c_void, b: *const *const c_void) -> c_int {
    // UNSAFE(slice) SAFETY: a and b point to pointers to entries with terminated names (§13.3);
    // compared as strcoll compares the names, in the current locale.
    unsafe { strz::dirent_coll(a, b, crate::locale::current()) }
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn alphasort64(a: *const *const c_void, b: *const *const c_void) -> c_int {
    // UNSAFE(slice) SAFETY: a and b point to pointers to entries with terminated names (§13.3);
    // compared as strcoll compares the names, in the current locale.
    unsafe { strz::dirent_coll(a, b, crate::locale::current()) }
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn versionsort(a: *const *const c_void, b: *const *const c_void) -> c_int {
    // UNSAFE(slice) SAFETY: a and b point to pointers to entries with terminated names (§13.3).
    ret::order(unsafe { strz::dirent_name(a) }.and_then(|x| unsafe { strz::dirent_name(b) }.map(|y| bytes::compare_versions(x, y))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn versionsort64(a: *const *const c_void, b: *const *const c_void) -> c_int {
    // UNSAFE(slice) SAFETY: a and b point to pointers to entries with terminated names (§13.3).
    ret::order(unsafe { strz::dirent_name(a) }.and_then(|x| unsafe { strz::dirent_name(b) }.map(|y| bytes::compare_versions(x, y))))
}}

/// A walk in a guard frame (OD-6 revised): if a callback throws, the walk's directories are
/// closed and the caller's working directory restored on the way to the caller's handler.
fn walk_guarded(walk: impl FnOnce(&mut fs_tree::WalkState) -> liella_kernel::errno::KResult<i32>) -> liella_kernel::errno::KResult<i32> {
    let mut st = fs_tree::WalkState::default();
    crate::posix::s22_threads::guarded(&mut st, walk, |st| fs_tree::unwound(&CSys, st)).unwrap_or(Err(liella_kernel::errno::Errno::EFAULT))
}

export! { "GLIBC_2.2.5";
pub extern "C" fn ftw(path: *const c_char, f: Option<FtwFn>, _nopenfd: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13.3).
    ret::signed(unsafe { strz::strz(path) }.and_then(|p| conv::req(f).and_then(|f| walk_guarded(|st| fs_tree::ftw(&CSys, p, f, st)))).map(i64::from))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ftw64(path: *const c_char, f: Option<FtwFn>, _nopenfd: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13.3).
    ret::signed(unsafe { strz::strz(path) }.and_then(|p| conv::req(f).and_then(|f| walk_guarded(|st| fs_tree::ftw(&CSys, p, f, st)))).map(i64::from))
}}

export! { "GLIBC_2.3.3";
pub extern "C" fn nftw(path: *const c_char, f: Option<NftwFn>, _nopenfd: c_int, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13.3).
    ret::signed(unsafe { strz::strz(path) }.and_then(|p| conv::req(f).and_then(|f| walk_guarded(|st| fs_tree::nftw(&CSys, p, f, flags, st)))).map(i64::from))
}}

export! { "GLIBC_2.3.3";
pub extern "C" fn nftw64(path: *const c_char, f: Option<NftwFn>, _nopenfd: c_int, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: path is a NUL-terminated string (§13.3).
    ret::signed(unsafe { strz::strz(path) }.and_then(|p| conv::req(f).and_then(|f| walk_guarded(|st| fs_tree::nftw(&CSys, p, f, flags, st)))).map(i64::from))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fts_open(argv: *const *mut c_char, options: c_int, compar: Option<SlotCompareFn>) -> *mut c_void {
    // UNSAFE(slice) SAFETY: argv is a NULL-terminated array of NUL-terminated strings (§13.3).
    ret::null(dirtree::fts_open(unsafe { conv::argv(argv) }, options, compar))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fts_read(ftsp: *mut c_void) -> *mut c_void {
    ret::entry_or_zero(dirtree::fts_read(conv::addr(ftsp)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fts_children(ftsp: *mut c_void, options: c_int) -> *mut c_void {
    ret::entry_or_zero(dirtree::fts_children(conv::addr(ftsp), options))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fts_set(ftsp: *mut c_void, f: *mut c_void, instr: c_int) -> c_int {
    ret::zero(dirtree::fts_set(conv::addr(ftsp), conv::addr(f), instr))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fts_close(ftsp: *mut c_void) -> c_int {
    ret::zero(dirtree::fts_close(conv::addr(ftsp)))
}}

export! { "GLIBC_2.23";
pub extern "C" fn fts64_open(argv: *const *mut c_char, options: c_int, compar: Option<SlotCompareFn>) -> *mut c_void {
    // UNSAFE(slice) SAFETY: argv is a NULL-terminated array of NUL-terminated strings (§13.3).
    ret::null(dirtree::fts_open(unsafe { conv::argv(argv) }, options, compar))
}}

export! { "GLIBC_2.23";
pub extern "C" fn fts64_read(ftsp: *mut c_void) -> *mut c_void {
    ret::entry_or_zero(dirtree::fts_read(conv::addr(ftsp)))
}}

export! { "GLIBC_2.23";
pub extern "C" fn fts64_children(ftsp: *mut c_void, options: c_int) -> *mut c_void {
    ret::entry_or_zero(dirtree::fts_children(conv::addr(ftsp), options))
}}

export! { "GLIBC_2.23";
pub extern "C" fn fts64_set(ftsp: *mut c_void, f: *mut c_void, instr: c_int) -> c_int {
    ret::zero(dirtree::fts_set(conv::addr(ftsp), conv::addr(f), instr))
}}

export! { "GLIBC_2.23";
pub extern "C" fn fts64_close(ftsp: *mut c_void) -> c_int {
    ret::zero(dirtree::fts_close(conv::addr(ftsp)))
}}

// ------------------------------------------------------------------------------ §13.8 realpath

export! { "GLIBC_2.3";
pub extern "C" fn realpath(path: *const c_char, resolved: *mut c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: path is NULL or a NUL-terminated string; resolved is NULL or holds PATH_MAX writable bytes (§13.8).
    ret::null(dirtree::realpath(unsafe { strz::strz_opt(path) }, unsafe { conv::opt_buf_mut(resolved, PATH_MAX) }))
}}

crate::internal_entry! {
/// `realpath@GLIBC_2.2.5`: a NULL `resolved` fails with `EINVAL` (no allocation); with a
/// buffer it is `realpath` (probe-settled, LG-2). Exported only under that compat version.
pub extern "C" fn liella_compat_realpath(path: *const c_char, resolved: *mut c_char) -> *mut c_char {
    if let Err(e) = liella_legacy::versioned::realpath_2_2_5(!resolved.is_null()) {
        crate::errno::set(e);
        return core::ptr::null_mut();
    }
    realpath(path, resolved)
}}

export! { "GLIBC_2.4";
pub extern "C" fn __realpath_chk(path: *const c_char, resolved: *mut c_char, resolvedlen: size_t) -> *mut c_char {
    // UNSAFE(slice) SAFETY: path is NULL or a NUL-terminated string; resolved is NULL or an object of resolvedlen writable bytes (§13.8, fortified).
    ret::null(dirtree::realpath(unsafe { strz::strz_opt(path) }, unsafe { conv::opt_buf_mut(resolved, resolvedlen.min(PATH_MAX)) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn canonicalize_file_name(path: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: path is NULL or a NUL-terminated string (§13.8).
    ret::null(dirtree::realpath(unsafe { strz::strz_opt(path) }, Ok(None)))
}}

// ------------------------------------------------------------------------------ §13.9 temporary names

export! { "GLIBC_2.2.5";
pub extern "C" fn mkstemp(template: *mut c_char) -> c_int {
    // UNSAFE(slice) SAFETY: template is a writable NUL-terminated string (§13.9).
    ret::fd(dirtree::mkostemps(unsafe { strz::strz_mut_z(template) }, 0, 0))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mkstemp64(template: *mut c_char) -> c_int {
    // UNSAFE(slice) SAFETY: template is a writable NUL-terminated string (§13.9).
    ret::fd(dirtree::mkostemps(unsafe { strz::strz_mut_z(template) }, 0, 0))
}}

export! { "GLIBC_2.7";
pub extern "C" fn mkostemp(template: *mut c_char, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: template is a writable NUL-terminated string (§13.9).
    ret::fd(dirtree::mkostemps(unsafe { strz::strz_mut_z(template) }, 0, flags))
}}

export! { "GLIBC_2.7";
pub extern "C" fn mkostemp64(template: *mut c_char, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: template is a writable NUL-terminated string (§13.9).
    ret::fd(dirtree::mkostemps(unsafe { strz::strz_mut_z(template) }, 0, flags))
}}

export! { "GLIBC_2.11";
pub extern "C" fn mkstemps(template: *mut c_char, suffixlen: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: template is a writable NUL-terminated string (§13.9).
    ret::fd(dirtree::mkostemps(unsafe { strz::strz_mut_z(template) }, suffixlen, 0))
}}

export! { "GLIBC_2.11";
pub extern "C" fn mkstemps64(template: *mut c_char, suffixlen: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: template is a writable NUL-terminated string (§13.9).
    ret::fd(dirtree::mkostemps(unsafe { strz::strz_mut_z(template) }, suffixlen, 0))
}}

export! { "GLIBC_2.11";
pub extern "C" fn mkostemps(template: *mut c_char, suffixlen: c_int, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: template is a writable NUL-terminated string (§13.9).
    ret::fd(dirtree::mkostemps(unsafe { strz::strz_mut_z(template) }, suffixlen, flags))
}}

export! { "GLIBC_2.11";
pub extern "C" fn mkostemps64(template: *mut c_char, suffixlen: c_int, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: template is a writable NUL-terminated string (§13.9).
    ret::fd(dirtree::mkostemps(unsafe { strz::strz_mut_z(template) }, suffixlen, flags))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mkdtemp(template: *mut c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: template is a writable NUL-terminated string (§13.9).
    ret::this_or_null(template, dirtree::mkdtemp(unsafe { strz::strz_mut_z(template) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mktemp(template: *mut c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: template is a writable NUL-terminated string (§13.9).
    ret::value_with(template, dirtree::mktemp(unsafe { strz::strz_mut_z(template) }))
}}

// ------------------------------------------------------------------------------ §13.13

export! { "GLIBC_2.2.5";
pub extern "C" fn setmntent(file: *const c_char, mode: *const c_char) -> *mut FILE {
    // UNSAFE(slice) SAFETY: file and mode are strings (§13.13).
    ret::null(mntent::setmntent(unsafe { strz::path(file) }, unsafe { strz::strz(mode) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __setmntent(file: *const c_char, mode: *const c_char) -> *mut FILE {
    // UNSAFE(slice) SAFETY: as setmntent.
    ret::null(mntent::setmntent(unsafe { strz::path(file) }, unsafe { strz::strz(mode) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getmntent(fp: *mut FILE) -> *mut struct_mntent {
    ret::entry(mntent::getmntent(conv::addr(fp)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getmntent_r(fp: *mut FILE, out: Option<&mut struct_mntent>, buf: *mut c_char, buflen: c_int) -> *mut struct_mntent {
    // UNSAFE(slice) SAFETY: buf holds buflen bytes (§13.13).
    ret::entry(mntent::getmntent_r(conv::addr(fp), out, unsafe { conv::buf_mut(buf.cast(), conv::nonneg(buflen)) }, buflen))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __getmntent_r(fp: *mut FILE, out: Option<&mut struct_mntent>, buf: *mut c_char, buflen: c_int) -> *mut struct_mntent {
    // UNSAFE(slice) SAFETY: as getmntent_r.
    ret::entry(mntent::getmntent_r(conv::addr(fp), out, unsafe { conv::buf_mut(buf.cast(), conv::nonneg(buflen)) }, buflen))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn addmntent(fp: *mut FILE, ent: Option<&struct_mntent>) -> c_int {
    // UNSAFE(slice) SAFETY: the entry's strings are NULL or strings (§13.13).
    ret::zero_or_one(mntent::addmntent(conv::addr(fp), ent, &|a| unsafe { strz::strz_opt(core::ptr::with_exposed_provenance(a)) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn endmntent(fp: *mut FILE) -> c_int {
    mntent::endmntent(conv::addr(fp))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __endmntent(fp: *mut FILE) -> c_int {
    mntent::endmntent(conv::addr(fp))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn hasmntopt(ent: Option<&struct_mntent>, opt: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: opt and the entry's mnt_opts are NULL or strings (§13.13).
    ret::null(mntent::hasmntopt(ent, unsafe { strz::strz_opt(opt) }, &|a| unsafe { strz::strz_opt(core::ptr::with_exposed_provenance(a)) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setfsent() -> c_int {
    mntent::setfsent()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getfsent() -> *mut struct_fstab {
    ret::entry(mntent::getfsent())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getfsspec(name: *const c_char) -> *mut struct_fstab {
    // UNSAFE(slice) SAFETY: name is a string (§13.13).
    ret::entry(mntent::lookup(unsafe { strz::strz(name) }, 0))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getfsfile(name: *const c_char) -> *mut struct_fstab {
    // UNSAFE(slice) SAFETY: name is a string (§13.13).
    ret::entry(mntent::lookup(unsafe { strz::strz(name) }, 1))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn endfsent() {
    mntent::endfsent()
}}
