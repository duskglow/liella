//! File descriptors (`liella_fd_*`): [`Fd`], an owned descriptor that [`fs::open`] returns and
//! that closes itself when dropped, and [`Stdio`], the standard descriptors 0, 1 and 2, which
//! the program does not own. Transfers are single system calls: a count below the length asked
//! is not an error, and `EINTR` is reported (`write_all` alone completes, retrying both).
//!
//! [`fs::open`]: crate::fs::open

#![forbid(unsafe_code)]

use core::mem::ManuallyDrop;

use crate::buf;
use crate::ffi;
use crate::fs::Stat;
use crate::Errno;

/// The origin of a [`seek`](Fd::seek) (`LIELLA_SEEK_*`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Whence {
    /// The offset is absolute (`LIELLA_SEEK_SET`).
    Set,
    /// The offset is relative to the current offset (`LIELLA_SEEK_CUR`).
    Current,
    /// The offset is relative to the end of the file (`LIELLA_SEEK_END`).
    End,
}

impl Whence {
    /// The C ABI's value.
    pub const fn raw(self) -> u32 {
        match self {
            Whence::Set => 0,
            Whence::Current => 1,
            Whence::End => 2,
        }
    }
}

fn read(fd: i32, buf: &mut [u8]) -> Result<usize, Errno> {
    buf::len(ffi::fd_read(fd, buf))
}

fn write(fd: i32, buf: &[u8]) -> Result<usize, Errno> {
    buf::len(ffi::fd_write(fd, buf))
}

fn write_all(fd: i32, mut data: &[u8]) -> Result<(), Errno> {
    while !data.is_empty() {
        match write(fd, data) {
            Ok(0) => return Err(Errno::EIO),
            Ok(n) => data = data.get(n..).unwrap_or_default(),
            Err(e) if e == Errno::EINTR => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

fn stat(fd: i32) -> Result<Stat, Errno> {
    let mut st = Stat::default();
    buf::done(ffi::liella_fd_stat(fd, &mut st)).map(|()| st)
}

/// An open file descriptor the program owns: dropping it closes it (an error from that close
/// is lost; [`close`](Fd::close) reports it).
#[derive(Debug)]
pub struct Fd {
    raw: i32,
}

impl Fd {
    /// Take ownership of a descriptor a native call returned.
    pub(crate) fn owned(raw: i32) -> Fd {
        Fd { raw }
    }

    /// The descriptor number (still owned by `self`).
    pub fn raw(&self) -> i32 {
        self.raw
    }

    /// Give up ownership: the descriptor stays open and is the caller's.
    pub fn into_raw(self) -> i32 {
        ManuallyDrop::new(self).raw
    }

    /// Read up to `buf.len()` bytes: the count, 0 at end of file.
    pub fn read(&self, buf: &mut [u8]) -> Result<usize, Errno> {
        read(self.raw, buf)
    }

    /// Write up to `buf.len()` bytes: the count.
    pub fn write(&self, buf: &[u8]) -> Result<usize, Errno> {
        write(self.raw, buf)
    }

    /// Write all of `data`, continuing after short writes and `EINTR`. A write that transfers
    /// nothing is `EIO`.
    pub fn write_all(&self, data: &[u8]) -> Result<(), Errno> {
        write_all(self.raw, data)
    }

    /// Read up to `buf.len()` bytes at `offset` (the file offset is unchanged): the count.
    pub fn pread(&self, buf: &mut [u8], offset: i64) -> Result<usize, Errno> {
        buf::len(ffi::fd_pread(self.raw, buf, offset))
    }

    /// Write up to `buf.len()` bytes at `offset` (the file offset is unchanged): the count.
    pub fn pwrite(&self, buf: &[u8], offset: i64) -> Result<usize, Errno> {
        buf::len(ffi::fd_pwrite(self.raw, buf, offset))
    }

    /// Move the file offset: the new offset from the start of the file.
    pub fn seek(&self, offset: i64, whence: Whence) -> Result<u64, Errno> {
        buf::value(ffi::liella_fd_seek(self.raw, offset, whence.raw()))
    }

    /// The status of the open file.
    pub fn stat(&self) -> Result<Stat, Errno> {
        stat(self.raw)
    }

    /// Close the descriptor, reporting the result. It is released even when the result is an
    /// error (`EINTR` included): it must not be closed again.
    pub fn close(self) -> Result<(), Errno> {
        buf::done(ffi::liella_fd_close(self.into_raw()))
    }
}

impl Drop for Fd {
    fn drop(&mut self) {
        let _ = ffi::liella_fd_close(self.raw);
    }
}

/// A standard descriptor (0, 1 or 2). The program does not own it: nothing closes it.
/// Formatting into a copy (`let mut out = Stdio::OUT; writeln!(out, ...)`) writes each piece
/// with `write_all`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stdio {
    raw: i32,
}

impl Stdio {
    /// Standard input.
    pub const IN: Stdio = Stdio { raw: 0 };
    /// Standard output.
    pub const OUT: Stdio = Stdio { raw: 1 };
    /// Standard error.
    pub const ERR: Stdio = Stdio { raw: 2 };

    /// The descriptor number.
    pub fn raw(self) -> i32 {
        self.raw
    }

    /// Read up to `buf.len()` bytes: the count, 0 at end of file.
    pub fn read(self, buf: &mut [u8]) -> Result<usize, Errno> {
        read(self.raw, buf)
    }

    /// Write up to `buf.len()` bytes: the count.
    pub fn write(self, buf: &[u8]) -> Result<usize, Errno> {
        write(self.raw, buf)
    }

    /// Write all of `data`, as [`Fd::write_all`].
    pub fn write_all(self, data: &[u8]) -> Result<(), Errno> {
        write_all(self.raw, data)
    }

    /// The status of the open file.
    pub fn stat(self) -> Result<Stat, Errno> {
        stat(self.raw)
    }
}

impl core::fmt::Write for Stdio {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        write_all(self.raw, s.as_bytes()).map_err(|_| core::fmt::Error)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects)]
mod tests {
    use super::*;

    #[test]
    fn whence_has_the_kernel_values() {
        assert_eq!([Whence::Set.raw(), Whence::Current.raw(), Whence::End.raw()], [0, 1, 2]);
    }
}
