//! The file system (`liella_fs_*`): open, status, directories, names. Paths are bytes, not
//! NUL-terminated, without an interior NUL (`EINVAL`) and shorter than `PATH_MAX`
//! (`ENAMETOOLONG`); a relative path is resolved against the current directory.

#![forbid(unsafe_code)]

use core::mem::{align_of, offset_of, size_of};

use crate::buf;
use crate::clock::Timespec;
use crate::fd::Fd;
use crate::ffi;
use crate::Errno;

/// Open for reading only (`LIELLA_O_RDONLY`).
pub const O_RDONLY: u32 = 0x0;
/// Open for writing only (`LIELLA_O_WRONLY`).
pub const O_WRONLY: u32 = 0x1;
/// Open for reading and writing (`LIELLA_O_RDWR`).
pub const O_RDWR: u32 = 0x2;
/// Create the file if it does not exist, with the mode given (`LIELLA_O_CREAT`).
pub const O_CREAT: u32 = 0x40;
/// With [`O_CREAT`]: fail with `EEXIST` if the file exists (`LIELLA_O_EXCL`).
pub const O_EXCL: u32 = 0x80;
/// Truncate a regular file opened for writing to length 0 (`LIELLA_O_TRUNC`).
pub const O_TRUNC: u32 = 0x200;
/// Every write appends at the end of the file (`LIELLA_O_APPEND`).
pub const O_APPEND: u32 = 0x400;
/// Non-blocking I/O (`LIELLA_O_NONBLOCK`).
pub const O_NONBLOCK: u32 = 0x800;
/// Fail with `ENOTDIR` unless the path names a directory (`LIELLA_O_DIRECTORY`).
pub const O_DIRECTORY: u32 = 0x10000;
/// Fail with `ELOOP` if the final component is a symbolic link (`LIELLA_O_NOFOLLOW`).
pub const O_NOFOLLOW: u32 = 0x20000;
/// Close the descriptor on exec (`LIELLA_O_CLOEXEC`); [`open`] does not imply it.
pub const O_CLOEXEC: u32 = 0x80000;

/// `liella_fs_stat`: describe a final symbolic link itself (`LIELLA_FS_NOFOLLOW`).
const FS_NOFOLLOW: u32 = 0x100;

/// The file-type bits of [`Stat::mode`] (`LIELLA_S_IFMT`).
pub const S_IFMT: u32 = 0o170000;
/// A socket (`LIELLA_S_IFSOCK`).
pub const S_IFSOCK: u32 = 0o140000;
/// A symbolic link (`LIELLA_S_IFLNK`).
pub const S_IFLNK: u32 = 0o120000;
/// A regular file (`LIELLA_S_IFREG`).
pub const S_IFREG: u32 = 0o100000;
/// A block device (`LIELLA_S_IFBLK`).
pub const S_IFBLK: u32 = 0o060000;
/// A directory (`LIELLA_S_IFDIR`).
pub const S_IFDIR: u32 = 0o040000;
/// A character device (`LIELLA_S_IFCHR`).
pub const S_IFCHR: u32 = 0o020000;
/// A FIFO (`LIELLA_S_IFIFO`).
pub const S_IFIFO: u32 = 0o010000;

/// A file's status: `struct liella_stat` (120 bytes, alignment 8), field for field.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Stat {
    /// The device holding the file.
    pub dev: u64,
    /// The inode number.
    pub ino: u64,
    /// The number of hard links.
    pub nlink: u64,
    /// The device a special file stands for.
    pub rdev: u64,
    /// The size in bytes.
    pub size: i64,
    /// The preferred I/O block size.
    pub blksize: i64,
    /// The 512-byte blocks allocated.
    pub blocks: i64,
    /// The last access.
    pub atime: Timespec,
    /// The last modification of the contents.
    pub mtime: Timespec,
    /// The last change of the status.
    pub ctime: Timespec,
    /// The type (`S_IF*`) and permission bits.
    pub mode: u32,
    /// The owner.
    pub uid: u32,
    /// The group.
    pub gid: u32,
    /// Always 0.
    pub reserved: u32,
}

// The layout of `struct liella_stat` (include/liella/native.h asserts the same).
const _: () = assert!(size_of::<Stat>() == 120 && align_of::<Stat>() == 8);
const _: () = assert!(offset_of!(Stat, dev) == 0 && offset_of!(Stat, ino) == 8 && offset_of!(Stat, nlink) == 16);
const _: () = assert!(offset_of!(Stat, rdev) == 24 && offset_of!(Stat, size) == 32 && offset_of!(Stat, blksize) == 40);
const _: () = assert!(offset_of!(Stat, blocks) == 48 && offset_of!(Stat, atime) == 56 && offset_of!(Stat, mtime) == 72);
const _: () = assert!(offset_of!(Stat, ctime) == 88 && offset_of!(Stat, mode) == 104 && offset_of!(Stat, uid) == 108);
const _: () = assert!(offset_of!(Stat, gid) == 112 && offset_of!(Stat, reserved) == 116);

impl Stat {
    /// The file-type bits (`mode & S_IFMT`): one of the `S_IF*` constants.
    pub fn file_type(&self) -> u32 {
        self.mode & S_IFMT
    }

    /// A regular file.
    pub fn is_file(&self) -> bool {
        self.file_type() == S_IFREG
    }

    /// A directory.
    pub fn is_dir(&self) -> bool {
        self.file_type() == S_IFDIR
    }

    /// A symbolic link (only [`lstat`] reports one).
    pub fn is_symlink(&self) -> bool {
        self.file_type() == S_IFLNK
    }

    /// The permission bits (`mode & 0o7777`).
    pub fn permissions(&self) -> u32 {
        self.mode & 0o7777
    }
}

/// Open the file at `path` with `flags` (`O_*`; [`O_CLOEXEC`] is not implied) and, when
/// creating it, `mode` (less the process umask).
pub fn open(path: &[u8], flags: u32, mode: u32) -> Result<Fd, Errno> {
    let raw = buf::value(ffi::fs_open(path, flags, mode))?;
    i32::try_from(raw).map(Fd::owned).map_err(|_| Errno::EBADF)
}

fn stat_at(path: &[u8], flags: u32) -> Result<Stat, Errno> {
    let mut st = Stat::default();
    buf::done(ffi::fs_stat(path, flags, &mut st)).map(|()| st)
}

/// The status of the file at `path`, following a final symbolic link.
pub fn stat(path: &[u8]) -> Result<Stat, Errno> {
    stat_at(path, 0)
}

/// The status of the file at `path`; a final symbolic link is described itself.
pub fn lstat(path: &[u8]) -> Result<Stat, Errno> {
    stat_at(path, FS_NOFOLLOW)
}

/// Create the directory `path` with `mode` (less the process umask).
pub fn mkdir(path: &[u8], mode: u32) -> Result<(), Errno> {
    buf::done(ffi::fs_mkdir(path, mode))
}

/// Remove the empty directory `path`.
pub fn rmdir(path: &[u8]) -> Result<(), Errno> {
    buf::done(ffi::fs_rmdir(path))
}

/// Remove the directory entry `path` (not a directory).
pub fn unlink(path: &[u8]) -> Result<(), Errno> {
    buf::done(ffi::fs_unlink(path))
}

/// Rename `from` to `to`, replacing `to` if it exists.
pub fn rename(from: &[u8], to: &[u8]) -> Result<(), Errno> {
    buf::done(ffi::fs_rename(from, to))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects)]
mod tests {
    use super::*;

    #[test]
    fn file_types() {
        let st = |mode| Stat { mode, ..Stat::default() };
        assert!(st(S_IFREG | 0o644).is_file());
        assert!(st(S_IFDIR | 0o755).is_dir());
        assert!(st(S_IFLNK | 0o777).is_symlink());
        assert!(!st(S_IFIFO | 0o600).is_file());
        assert_eq!(st(S_IFDIR | 0o1755).permissions(), 0o1755);
        assert_eq!(st(S_IFCHR | 0o600).file_type(), S_IFCHR);
    }
}
