//! Directory streams (spec §13.2; D-13.2, D-13.3).
//!
//! A [`DirStream`] reads a directory with `getdents64` into a buffer it owns and hands the
//! records out one at a time, in the order the file system produces them. The records are the
//! kernel's own (`struct linux_dirent64`, which on x86-64 is also `struct dirent` and `struct
//! dirent64`, spec Appendix C), so a C caller can be given the address of one in place.
//!
//! * Positions ([`DirStream::tell`], [`DirStream::seek`]) are the file system's cookies, passed
//!   through unchanged (D-13.3): the `d_off` of the last entry returned, or the position last
//!   set.
//! * A failed `getdents64` is reported (D-13.2, owner ruling 2026-10-03): a directory removed
//!   while open ends with the kernel's `ENOENT` once the entries already buffered are used up,
//!   on every further call too. The end of the directory is `Ok(None)`.

use crate::error::{Errno, Result};
use crate::mem::TryVec;
use liella_kernel::fd::Fd;
use liella_kernel::ops::FcntlInt;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{FD_CLOEXEC, O_CLOEXEC, O_DIRECTORY, O_NONBLOCK, O_PATH, O_RDONLY, O_WRONLY, SEEK_CUR, SEEK_SET, S_IFDIR, S_IFMT};

/// A stream's buffer: big enough that one `getdents64` call returns a small directory whole.
pub const STREAM_BUFFER: usize = 32 * 1024;

/// The part of a new stream buffer cleared before the first `getdents64` call (see
/// [`DirStream::with_fd_reusing`]): one page, enough for a small directory.
pub const FIRST_FILL: usize = 4096;

/// Offset of `d_ino` in a record.
pub const D_INO: usize = 0;
/// Offset of `d_off`.
pub const D_OFF: usize = 8;
/// Offset of `d_reclen`.
pub const D_RECLEN: usize = 16;
/// Offset of `d_type`.
pub const D_TYPE: usize = 18;
/// Offset of `d_name`.
pub const D_NAME: usize = 19;

/// `d_type` values (⚑ KERN).
pub const DT_UNKNOWN: u8 = 0;
/// A directory.
pub const DT_DIR: u8 = 4;
/// A symbolic link.
pub const DT_LNK: u8 = 10;

/// `offset` of an adopted stream before its first entry: the descriptor's position, asked
/// for only if `telldir` needs it (no file system cookie is negative).
const OFFSET_UNKNOWN: i64 = i64::MIN;

/// One entry: where its record lies in the stream's buffer, and its fields.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    /// `d_ino`.
    pub ino: u64,
    /// `d_off`: the position after this entry.
    pub off: i64,
    /// `d_reclen`: the record's length.
    pub reclen: u16,
    /// `d_type`.
    pub kind: u8,
    start: usize,
}

/// A directory stream on a descriptor it owns.
pub struct DirStream {
    fd: Fd,
    buf: TryVec<u8>,
    /// Where the 8-aligned record area starts in `buf`.
    base: usize,
    cap: usize,
    /// The largest `cap` the buffer may grow to (`cap` itself for a fixed buffer).
    max: usize,
    /// The last `getdents64` call nearly filled the buffer.
    nearly_full: bool,
    pos: usize,
    len: usize,
    offset: i64,
}

fn field<const N: usize>(b: &[u8], at: usize) -> Option<[u8; N]> {
    b.get(at..at.checked_add(N)?)?.try_into().ok()
}

/// The record at `pos` of `area` (`len` bytes in use): its entry, or `EIO` for a malformed
/// record (never produced by the kernel). The name is not scanned here: `getdents64` always
/// terminates it, and `name`/`name_z` stop at the first NUL inside the record (READDIR-1).
fn parse(area: &[u8], pos: usize) -> Result<Entry> {
    let rec = area.get(pos..).ok_or(Errno::EIO)?;
    let reclen = field::<2>(rec, D_RECLEN).map(u16::from_ne_bytes).ok_or(Errno::EIO)?;
    let n = usize::from(reclen);
    if n <= D_NAME || n > rec.len() {
        return Err(Errno::EIO);
    }
    let ino = field::<8>(rec, D_INO).map(u64::from_ne_bytes).ok_or(Errno::EIO)?;
    let off = field::<8>(rec, D_OFF).map(i64::from_ne_bytes).ok_or(Errno::EIO)?;
    let kind = rec.get(D_TYPE).copied().ok_or(Errno::EIO)?;
    Ok(Entry { ino, off, reclen, kind, start: pos })
}

impl DirStream {
    /// A stream on `fd` (owned from now on) at position `offset`, with a buffer of `cap` bytes.
    pub fn with_fd(fd: Fd, cap: usize, offset: i64) -> Result<DirStream> {
        DirStream::with_fd_reusing(fd, cap, offset, None)
    }

    /// Like [`with_fd`](Self::with_fd), on `spare` (a buffer an earlier stream gave up with
    /// [`close_keep`](Self::close_keep)) when it is large enough: no allocation and nothing to
    /// clear, since only the bytes `getdents64` writes are ever read (WL-HOT-DU).
    ///
    /// A new buffer reserves all `cap` bytes but clears (and so touches) only the first
    /// [`FIRST_FILL`]; it grows in place to `cap` once a `getdents64` call nearly fills it. A
    /// program holding one stream per tree level (tar, du, ls -R) then faults in one page per
    /// small directory, not eight (FAULTS-TARGZ-2). A large directory costs one more
    /// `getdents64` call, no copy and no new allocation.
    #[inline]
    pub fn with_fd_reusing(fd: Fd, cap: usize, offset: i64, spare: Option<TryVec<u8>>) -> Result<DirStream> {
        let cap = cap.max(1024);
        let full = cap.saturating_add(8);
        let mut buf = match spare {
            Some(b) if b.capacity() >= full => b,
            _ => TryVec::with_capacity(full)?,
        };
        let first = cap.min(FIRST_FILL).saturating_add(8);
        if buf.len() < first {
            buf.resize(first, 0)?;
        }
        let start = buf.len().min(full).saturating_sub(8);
        let mis = buf.as_slice().as_ptr().addr().checked_rem(8).unwrap_or(0);
        let base = 8usize.wrapping_sub(mis).checked_rem(8).unwrap_or(0);
        Ok(DirStream { fd, buf, base, cap: start, max: cap, nearly_full: false, pos: 0, len: 0, offset })
    }

    /// `opendir(path)` relative to `dir`: the directory opened for reading, close-on-exec and
    /// non-blocking (so a FIFO named by mistake cannot block; `O_DIRECTORY` refuses it).
    pub fn open<S: Sys>(sys: &S, dir: Fd, path: KStr<'_>, cap: usize) -> Result<DirStream> {
        DirStream::open_reusing(sys, dir, path, cap, None)
    }

    /// [`open`](Self::open) on a spare buffer (see [`with_fd_reusing`](Self::with_fd_reusing)).
    #[inline]
    pub fn open_reusing<S: Sys>(sys: &S, dir: Fd, path: KStr<'_>, cap: usize, spare: Option<TryVec<u8>>) -> Result<DirStream> {
        let fd = sys.openat(dir, path, O_RDONLY | O_NONBLOCK | O_CLOEXEC | O_DIRECTORY, 0)?;
        DirStream::with_fd_reusing(fd, cap, 0, spare).inspect_err(|_| {
            let _ = sys.close(fd);
        })
    }

    /// Like [`open`](Self::open), with a buffer that starts at `start` bytes and grows (by
    /// eight, up to `max`) each time a `getdents64` call fills it. A small directory, the usual
    /// case, then costs one small buffer to clear instead of `max` bytes.
    pub fn open_growing<S: Sys>(sys: &S, dir: Fd, path: KStr<'_>, start: usize, max: usize) -> Result<DirStream> {
        let mut s = DirStream::open(sys, dir, path, start)?;
        s.max = max.max(s.cap);
        Ok(s)
    }

    /// `fdopendir(fd)`: a stream on an open directory descriptor, at its current position (no
    /// implicit rewind). `ENOTDIR` for anything but a directory, `EBADF` for a descriptor that
    /// is not open or cannot be read (`O_PATH`, write-only). On success the descriptor belongs to
    /// the stream and is close-on-exec; on failure it is left as it was. The position is not
    /// asked for here: [`tell`](Self::tell) asks the kernel while no entry has been read
    /// (WL-HOT-DU: one `lseek` less per directory). `FD_CLOEXEC` is the only descriptor flag,
    /// so it is set without reading the flags first.
    pub fn adopt<S: Sys>(sys: &S, fd: Fd, cap: usize, spare: Option<TryVec<u8>>) -> Result<DirStream> {
        let st = sys.fstat(fd)?;
        if st.mode & S_IFMT != S_IFDIR {
            return Err(Errno::ENOTDIR);
        }
        let fl = u32::try_from(sys.fcntl_int(fd, FcntlInt::GetFl, 0)?).unwrap_or(0);
        if fl & O_PATH != 0 || fl & 3 == O_WRONLY {
            return Err(Errno::EBADF);
        }
        let stream = DirStream::with_fd_reusing(fd, cap, OFFSET_UNKNOWN, spare)?;
        sys.fcntl_int(fd, FcntlInt::SetFd, FD_CLOEXEC)?;
        Ok(stream)
    }

    /// The descriptor.
    pub fn fd(&self) -> Fd {
        self.fd
    }

    /// The next entry, `Ok(None)` at the end of the directory, or the kernel's error.
    pub fn next<S: Sys>(&mut self, sys: &S) -> Result<Option<Entry>> {
        loop {
            if self.pos < self.len {
                let area = self.area();
                let e = parse(area.get(..self.len).unwrap_or(&[]), self.pos)?;
                self.pos = self.pos.saturating_add(usize::from(e.reclen));
                self.offset = e.off;
                return Ok(Some(e));
            }
            if !self.refill(sys)? {
                return Ok(None);
            }
        }
    }

    /// [`next`](Self::next) for `readdir`: the address of the next record (see
    /// [`record_addr`](Self::record_addr)), `Ok(None)` at the end.
    #[inline]
    pub fn next_addr<S: Sys>(&mut self, sys: &S) -> Result<Option<usize>> {
        loop {
            if let Some(a) = self.buffered_addr()? {
                return Ok(Some(a));
            }
            if !self.refill(sys)? {
                return Ok(None);
            }
        }
    }

    /// The address of the next record if one is buffered (`Ok(None)` when the buffer is used
    /// up): a few loads and compares, no [`Entry`] built, no system call (DU-3).
    #[inline]
    pub fn buffered_addr(&mut self) -> Result<Option<usize>> {
        if self.pos >= self.len {
            return Ok(None);
        }
        // `base + len <= buf.len()` (`len <= cap`, the buffer holds `base + cap` bytes), so
        // neither sum wraps; `get` checks the range anyway.
        let rec = self.buf.get(self.base.wrapping_add(self.pos)..self.base.wrapping_add(self.len)).ok_or(Errno::EIO)?;
        let reclen = field::<2>(rec, D_RECLEN).map(u16::from_ne_bytes).ok_or(Errno::EIO)?;
        let n = usize::from(reclen);
        if n <= D_NAME || n > rec.len() {
            return Err(Errno::EIO);
        }
        self.offset = field::<8>(rec, D_OFF).map(i64::from_ne_bytes).ok_or(Errno::EIO)?;
        // `n <= rec.len() = len - pos`: no wrap.
        self.pos = self.pos.wrapping_add(n);
        Ok(Some(rec.as_ptr().expose_provenance()))
    }

    /// Read the next batch of records into the (used up) buffer: `false` at the end of the
    /// directory. Out of line: once per buffer, not per entry.
    #[inline(never)]
    fn refill<S: Sys>(&mut self, sys: &S) -> Result<bool> {
        self.pos = 0;
        self.len = 0;
        // The last call nearly filled the buffer: the next one gets a larger one (every
        // entry of the old one is used up, so nothing is copied). Within the reserved capacity
        // it doubles: a grown page is cleared (touched) whether or not a record lands in it, so
        // a directory just over one page costs one more page, not the whole reservation
        // (TAR-HEAP: tar holds one stream per tree level and each grew to 8 KiB-32 KiB at once).
        if self.nearly_full && self.cap < self.max {
            self.nearly_full = false;
            let doubled = self.max.min(self.cap.saturating_mul(2));
            let next = if doubled.saturating_add(8) <= self.buf.capacity() { doubled } else { self.max.min(self.cap.saturating_mul(8)) };
            self.grow(next)?;
        }
        let (base, cap) = (self.base, self.cap);
        let area = self.buf.get_mut(base..base.saturating_add(cap)).unwrap_or(&mut []);
        let n = sys.getdents64(self.fd, area)?;
        if n == 0 {
            return Ok(false);
        }
        self.len = n.min(cap);
        self.nearly_full = n.saturating_add(512) > cap;
        Ok(true)
    }

    /// A larger buffer; only called when none of the current one is in use. Within the reserved
    /// capacity it grows in place (the start stays put, so `base` does too).
    fn grow(&mut self, cap: usize) -> Result<()> {
        if cap.saturating_add(8) <= self.buf.capacity() {
            self.buf.resize(cap.saturating_add(8), 0)?;
            self.cap = cap;
            return Ok(());
        }
        let mut buf = TryVec::with_capacity(cap.saturating_add(8))?;
        buf.resize(cap.saturating_add(8), 0)?;
        let mis = buf.as_slice().as_ptr().addr().checked_rem(8).unwrap_or(0);
        self.base = 8usize.wrapping_sub(mis).checked_rem(8).unwrap_or(0);
        self.buf = buf;
        self.cap = cap;
        Ok(())
    }

    fn area(&self) -> &[u8] {
        self.buf.get(self.base..self.base.saturating_add(self.cap)).unwrap_or(&[])
    }

    /// The whole record of `e` (`d_reclen` bytes).
    pub fn record(&self, e: &Entry) -> &[u8] {
        self.area().get(e.start..e.start.saturating_add(usize::from(e.reclen))).unwrap_or(&[])
    }

    /// The name of `e`, without its terminator.
    pub fn name(&self, e: &Entry) -> &[u8] {
        let at = e.start.saturating_add(D_NAME);
        let rec = self.area().get(at..e.start.saturating_add(usize::from(e.reclen))).unwrap_or(&[]);
        rec.get(..rec.iter().position(|&b| b == 0).unwrap_or(0)).unwrap_or(&[])
    }

    /// The name of `e` with its terminator, for the kernel.
    pub fn name_z(&self, e: &Entry) -> &[u8] {
        let at = e.start.saturating_add(D_NAME);
        let rec = self.area().get(at..e.start.saturating_add(usize::from(e.reclen))).unwrap_or(&[]);
        rec.get(..rec.iter().position(|&b| b == 0).map_or(0, |n| n.saturating_add(1))).unwrap_or(&[])
    }

    /// The address of `e`'s record, which stays valid until the stream reads again (a C
    /// caller's `struct dirent *`).
    pub fn record_addr(&self, e: &Entry) -> usize {
        self.area().as_ptr().expose_provenance().saturating_add(e.start)
    }

    /// `telldir`: the position of the next entry (the descriptor's own position for an
    /// adopted stream that has not read an entry yet).
    pub fn tell<S: Sys>(&self, sys: &S) -> i64 {
        if self.offset != OFFSET_UNKNOWN {
            return self.offset;
        }
        sys.lseek(self.fd, 0, SEEK_CUR).ok().and_then(|o| i64::try_from(o).ok()).unwrap_or(0)
    }

    /// `seekdir(pos)`: continue at a position [`tell`](Self::tell) returned. The buffer is
    /// dropped whatever the kernel answers.
    pub fn seek<S: Sys>(&mut self, sys: &S, pos: i64) -> Result<()> {
        self.pos = 0;
        self.len = 0;
        self.offset = pos;
        sys.lseek(self.fd, pos, SEEK_SET).map(|_| ())
    }

    /// `rewinddir`: back to the first entry; entries created or removed since are seen.
    pub fn rewind<S: Sys>(&mut self, sys: &S) -> Result<()> {
        self.seek(sys, 0)
    }

    /// `closedir`: close the descriptor (released even when `close` reports an error).
    pub fn close<S: Sys>(self, sys: &S) -> Result<()> {
        sys.close(self.fd)
    }

    /// [`close`](Self::close), handing back the buffer for a later stream.
    pub fn close_keep<S: Sys>(self, sys: &S) -> (Result<()>, TryVec<u8>) {
        (sys.close(self.fd), self.buf)
    }

    /// Give up the stream, keeping its descriptor open.
    pub fn into_fd(self) -> Fd {
        self.fd
    }
}
