//! The C shapes of spec §11 that add something to the kernel call (D-11.1–D-11.9). The thin
//! calls are the typed `Sys` methods themselves; what is here is what glibc's contract adds:
//! the fortify checks, `closefrom`'s fallback, `fcntl`'s typed commands, `lockf` over record
//! locks, `select`/`poll` timeouts, `epoll_create`'s size check, the `eventfd` helpers,
//! `posix_fallocate`'s emulation, `posix_madvise`'s advice map, and the protection-key
//! register.

use liella::error::{Errno, Result};
use liella_kernel::fd::Fd;
use liella_kernel::machine::Machine;
use liella_kernel::ops::{FcntlHint, FcntlInt, FcntlLock};
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{
    EpollEvent, FOwnerEx, Flock, FdSet, PollFd, SigSet, Timespec, Timeval, F_UNLCK, F_WRLCK, O_APPEND, O_CREAT, O_DIRECTORY, O_RDONLY, O_TMPFILE,
    SEEK_CUR, SEEK_DATA, SEEK_HOLE, S_IFIFO, S_IFMT, S_IFREG,
};

// ------------------------------------------------------------------------------ §11.1–11.2

/// Whether `open`'s flags need a mode (`O_CREAT`, or `O_TMPFILE`): `__open_2` without one is a
/// program bug the fortify check reports (§11.1).
pub fn needs_mode(flags: i32) -> bool {
    let f = u32::from_ne_bytes(flags.to_ne_bytes());
    f & O_CREAT != 0 || f & O_TMPFILE == O_TMPFILE
}

/// `creat`'s flags: `O_CREAT | O_WRONLY | O_TRUNC` (§11.1).
pub const CREAT_FLAGS: u32 = O_CREAT | 0o1 | 0o1000;

/// `closefrom(lo)` (§11.2): `close_range`, else every descriptor listed in `/proc/self/fd`.
/// `Err` only if both fail (the caller then aborts: `closefrom` never fails).
pub fn closefrom<S: Sys>(sys: &S, lo: i32) -> Result<()> {
    let lo = u32::try_from(lo).unwrap_or(0);
    match sys.close_range(lo, u32::MAX, 0) {
        Ok(()) => return Ok(()),
        Err(Errno::ENOSYS | Errno::EINVAL) => {}
        Err(e) => return Err(e),
    }
    let dir = sys.openat(Fd::CWD, KStr::from_nul_terminated(b"/proc/self/fd\0")?, O_RDONLY | O_DIRECTORY | 0o2000000, 0)?;
    let mut buf = [0u8; 4096];
    let mut result = Ok(());
    loop {
        let n = match sys.getdents64(dir, &mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                result = Err(e);
                break;
            }
        };
        let mut off = 0usize;
        while let Some(rec) = buf.get(off..n) {
            let Some(reclen) = rec.get(16..18).and_then(|b| <[u8; 2]>::try_from(b).ok()).map(|b| usize::from(u16::from_ne_bytes(b))) else { break };
            if reclen == 0 {
                break;
            }
            let name = rec.get(19..reclen).unwrap_or(&[]);
            let name = name.split(|&b| b == 0).next().unwrap_or(&[]);
            if let Some(fd) = decimal(name) {
                if fd >= lo && fd != u32::try_from(dir.raw()).unwrap_or(u32::MAX) {
                    let _ = sys.close(Fd::new(i32::try_from(fd).unwrap_or(-1)));
                }
            }
            off = off.saturating_add(reclen);
        }
    }
    let _ = sys.close(dir);
    result
}

fn decimal(s: &[u8]) -> Option<u32> {
    if s.is_empty() {
        return None;
    }
    s.iter().try_fold(0u32, |acc, &b| acc.checked_mul(10)?.checked_add(char::from(b).to_digit(10)?))
}

// ------------------------------------------------------------------------------ §11.5 fcntl

/// `fcntl`'s third argument, typed by the command (T7).
pub enum FcntlArg<'a> {
    /// An integer command (or one without an argument).
    Int(FcntlInt, i64),
    /// A record-lock command with its `struct flock`.
    Lock(FcntlLock, Option<&'a mut Flock>),
    /// `F_GETOWN_EX` (false) or `F_SETOWN_EX` (true) with its `struct f_owner_ex`.
    OwnerEx(bool, Option<&'a mut FOwnerEx>),
    /// A read/write-hint command with its `uint64_t`.
    Hint(FcntlHint, Option<&'a mut u64>),
    /// A command number the kernel does not have.
    Unknown,
}

/// `F_GETOWN_EX`.
pub const F_GETOWN_EX: u32 = 16;
/// `F_SETOWN_EX`.
pub const F_SETOWN_EX: u32 = 15;

/// Whether `cmd` takes a pointer argument (and which kind), for the boundary's conversion.
pub fn fcntl_kind(cmd: i32) -> FcntlKind {
    let c = u32::from_ne_bytes(cmd.to_ne_bytes());
    if let Some(l) = FcntlLock::from_code(c) {
        FcntlKind::Lock(l)
    } else if c == F_GETOWN_EX || c == F_SETOWN_EX {
        FcntlKind::OwnerEx(c == F_SETOWN_EX)
    } else if let Some(h) = FcntlHint::from_code(c) {
        FcntlKind::Hint(h)
    } else if let Some(i) = FcntlInt::from_code(c) {
        FcntlKind::Int(i)
    } else {
        FcntlKind::Unknown
    }
}

/// The argument kind of an `fcntl` command.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FcntlKind {
    /// An integer command.
    Int(FcntlInt),
    /// A `struct flock *` command.
    Lock(FcntlLock),
    /// A `struct f_owner_ex *` command (`true`: set).
    OwnerEx(bool),
    /// A `uint64_t *` command.
    Hint(FcntlHint),
    /// Not a command.
    Unknown,
}

/// `fcntl(fd, cmd, arg)`.
pub fn fcntl<S: Sys>(sys: &S, fd: Fd, arg: FcntlArg<'_>) -> Result<i64> {
    match arg {
        FcntlArg::Int(cmd, v) => sys.fcntl_int(fd, cmd, v),
        FcntlArg::Lock(cmd, Some(l)) => sys.fcntl_lock(fd, cmd, l).map(|()| 0),
        FcntlArg::OwnerEx(set, Some(o)) => sys.fcntl_owner_ex(fd, set, o).map(|()| 0),
        FcntlArg::Hint(cmd, Some(h)) => sys.fcntl_hint(fd, cmd, h).map(|()| 0),
        FcntlArg::Lock(_, None) | FcntlArg::OwnerEx(_, None) | FcntlArg::Hint(_, None) => Err(Errno::EFAULT),
        FcntlArg::Unknown => Err(Errno::EINVAL),
    }
}

// ------------------------------------------------------------------------------ §11.13 lockf

/// `F_ULOCK`.
pub const F_ULOCK: i32 = 0;
/// `F_LOCK`.
pub const F_LOCK: i32 = 1;
/// `F_TLOCK`.
pub const F_TLOCK: i32 = 2;
/// `F_TEST`.
pub const F_TEST: i32 = 3;

/// `lockf(fd, cmd, len)` (§11.13): an exclusive POSIX record lock on `len` bytes from the
/// current offset (to the end of file for 0), with the per-process semantics of `fcntl` locks.
/// `F_TEST` fails with `EACCES` if another process holds a conflicting lock.
pub fn lockf<S: Sys>(sys: &S, fd: Fd, cmd: i32, len: i64) -> Result<()> {
    let whence = i16::try_from(SEEK_CUR).unwrap_or(1);
    match cmd {
        F_ULOCK => sys.fcntl_lock(fd, FcntlLock::SetLk, &mut Flock::new(F_UNLCK, whence, 0, len)),
        F_LOCK => sys.fcntl_lock(fd, FcntlLock::SetLkw, &mut Flock::new(F_WRLCK, whence, 0, len)),
        F_TLOCK => sys.fcntl_lock(fd, FcntlLock::SetLk, &mut Flock::new(F_WRLCK, whence, 0, len)),
        F_TEST => {
            let mut l = Flock::new(F_WRLCK, whence, 0, len);
            sys.fcntl_lock(fd, FcntlLock::GetLk, &mut l)?;
            if l.ltype == F_UNLCK || l.pid == sys.getpid() { Ok(()) } else { Err(Errno::EACCES) }
        }
        _ => Err(Errno::EINVAL),
    }
}

// ------------------------------------------------------------------------------ §11.7 multiplexing

/// A `struct timeval` as a `timespec` (`EINVAL` if out of range).
fn timespec_of(tv: &Timeval) -> Result<Timespec> {
    if tv.sec < 0 || !(0..1_000_000).contains(&tv.usec) {
        return Err(Errno::EINVAL);
    }
    Ok(Timespec { sec: tv.sec, nsec: tv.usec.saturating_mul(1000) })
}

/// `select(nfds, r, w, e, timeout)` (§11.7) through `pselect6`; the timeout is rewritten with
/// the time remaining, as Linux does.
pub fn select<S: Sys>(sys: &S, nfds: i32, r: Option<&mut FdSet>, w: Option<&mut FdSet>, e: Option<&mut FdSet>, timeout: Option<&mut Timeval>) -> Result<usize> {
    let n = u32::try_from(nfds).map_err(|_| Errno::EINVAL)?;
    match timeout {
        None => sys.pselect6(n, r, w, e, None, None),
        Some(tv) => {
            let mut ts = timespec_of(tv)?;
            let res = sys.pselect6(n, r, w, e, Some(&mut ts), None);
            tv.sec = ts.sec;
            tv.usec = ts.nsec / 1000;
            res
        }
    }
}

/// `pselect` (§11.7): the caller's timeout is not modified; the mask applies during the wait.
pub fn pselect<S: Sys>(sys: &S, nfds: i32, r: Option<&mut FdSet>, w: Option<&mut FdSet>, e: Option<&mut FdSet>, timeout: Option<&Timespec>, mask: Option<SigSet>) -> Result<usize> {
    let n = u32::try_from(nfds).map_err(|_| Errno::EINVAL)?;
    let mut ts = timeout.copied();
    sys.pselect6(n, r, w, e, ts.as_mut(), mask.as_ref())
}

/// `poll(fds, timeout_ms)` (§11.7) through `ppoll`; a negative timeout waits forever.
pub fn poll<S: Sys>(sys: &S, fds: &mut [PollFd], timeout_ms: i32) -> Result<usize> {
    let mut ts = (timeout_ms >= 0).then(|| Timespec { sec: i64::from(timeout_ms / 1000), nsec: i64::from(timeout_ms % 1000).saturating_mul(1_000_000) });
    sys.ppoll(fds, ts.as_mut(), None)
}

/// `ppoll` (§11.7): the caller's timeout is not modified.
pub fn ppoll<S: Sys>(sys: &S, fds: &mut [PollFd], timeout: Option<&Timespec>, mask: Option<SigSet>) -> Result<usize> {
    let mut ts = timeout.copied();
    sys.ppoll(fds, ts.as_mut(), mask.as_ref())
}

/// `epoll_create(size)` (§11.7): the size is ignored but must be positive.
pub fn epoll_create<S: Sys>(sys: &S, size: i32) -> Result<Fd> {
    if size <= 0 {
        return Err(Errno::EINVAL);
    }
    sys.epoll_create1(0)
}

/// `epoll_wait(ep, events, timeout)`.
pub fn epoll_wait<S: Sys>(sys: &S, ep: Fd, events: &mut [EpollEvent], timeout_ms: i32) -> Result<usize> {
    sys.epoll_pwait(ep, events, timeout_ms, None)
}

/// `__fdelt_chk(d)`: the word index of descriptor `d` in an `fd_set`, `None` for a descriptor
/// outside `[0, FD_SETSIZE)` (the fortify failure, §11.7).
pub fn fdelt(d: i64) -> Option<u64> {
    u64::try_from(d).ok().filter(|&d| d < 1024).map(|d| d / 64)
}

// ------------------------------------------------------------------------------ §11.12 eventfd

/// `eventfd_read(fd, &value)`: one 8-byte read into `value`. A short read reports `EINVAL`.
pub fn eventfd_read<S: Sys>(sys: &S, fd: Fd, value: &mut u64) -> Result<()> {
    let mut b = [0u8; 8];
    match sys.read(fd, &mut b)? {
        8 => {
            *value = u64::from_ne_bytes(b);
            Ok(())
        }
        _ => Err(Errno::EINVAL),
    }
}

/// `eventfd_write(fd, value)`: one 8-byte write.
pub fn eventfd_write<S: Sys>(sys: &S, fd: Fd, value: u64) -> Result<()> {
    match sys.write(fd, &value.to_ne_bytes())? {
        8 => Ok(()),
        _ => Err(Errno::EINVAL),
    }
}

// ------------------------------------------------------------------------------ §11.8 fallocate

/// `posix_fallocate(fd, off, len)` (§11.8; D-11.5): `fallocate`, else an emulation that writes
/// a zero byte into each block of the range that lies in a hole (found with `SEEK_HOLE` and
/// `SEEK_DATA`, so existing data is never written) and extends the file to `off + len`.
/// `O_APPEND` descriptors are refused with `EBADF`.
pub fn posix_fallocate<S: Sys>(sys: &S, fd: Fd, off: i64, len: i64) -> Result<()> {
    if off < 0 || len <= 0 {
        return Err(Errno::EINVAL);
    }
    let end = off.checked_add(len).ok_or(Errno::EFBIG)?;
    match sys.fallocate(fd, 0, off, len) {
        Err(Errno::EOPNOTSUPP) => {}
        other => return other,
    }
    let fl = sys.fcntl_int(fd, FcntlInt::GetFl, 0)?;
    if u32::try_from(fl).unwrap_or(0) & O_APPEND != 0 {
        return Err(Errno::EBADF);
    }
    let st = sys.fstat(fd)?;
    match st.mode & S_IFMT {
        S_IFREG => {}
        S_IFIFO => return Err(Errno::ESPIPE),
        _ => return Err(Errno::ENODEV),
    }
    let block = st.blksize.max(512);
    let mut pos = off;
    while pos < end {
        let hole = match sys.lseek(fd, pos, SEEK_HOLE) {
            Ok(h) => i64::try_from(h).unwrap_or(i64::MAX),
            Err(Errno::ENXIO) => pos,
            Err(Errno::EINVAL) => st.size.max(pos),
            Err(e) => return Err(e),
        };
        if hole >= end {
            break;
        }
        let data = match sys.lseek(fd, hole, SEEK_DATA) {
            Ok(d) => i64::try_from(d).unwrap_or(i64::MAX),
            Err(Errno::ENXIO | Errno::EINVAL) => end,
            Err(e) => return Err(e),
        };
        let stop = data.min(end);
        // A zero byte at the start of every block that the hole covers.
        let mut b = hole.div_euclid(block).saturating_mul(block).max(hole);
        while b < stop {
            sys.pwrite(fd, &[0], b)?;
            b = (b.div_euclid(block).saturating_add(1)).saturating_mul(block);
        }
        pos = stop.max(pos.saturating_add(1));
    }
    if sys.fstat(fd)?.size < end {
        sys.pwrite(fd, &[0], end.saturating_sub(1))?;
    }
    Ok(())
}

// ------------------------------------------------------------------------------ §11.10 advice, keys

/// `POSIX_MADV_DONTNEED`.
pub const POSIX_MADV_DONTNEED: i32 = 4;

/// `posix_madvise(addr, len, advice)` (§11.10; D-11.7): `POSIX_MADV_DONTNEED` never discards
/// data, so it is a no-op; the other advice values equal `madvise`'s, applied by `madvise`.
pub fn posix_madvise(advice: i32, madvise: impl FnOnce(i32) -> Result<()>) -> Result<()> {
    match advice {
        POSIX_MADV_DONTNEED => Ok(()),
        0..=3 => madvise(advice),
        _ => Err(Errno::EINVAL),
    }
}

/// `pkey_get(key)` (§11.10): the two access-rights bits of `key` in this thread's PKRU.
/// `ENOSYS` where the CPU or the OS lacks protection keys (glibc would fault).
pub fn pkey_get<M: Machine>(m: &M, key: i32) -> Result<i32> {
    let k = u32::try_from(key).ok().filter(|&k| k < 16).ok_or(Errno::EINVAL)?;
    let pkru = m.pkru().ok_or(Errno::ENOSYS)?;
    i32::try_from((pkru >> k.saturating_mul(2)) & 3).map_err(|_| Errno::EINVAL)
}

/// `pkey_set(key, rights)` (§11.10): set the access-rights bits of `key` in this thread's PKRU.
pub fn pkey_set<M: Machine>(m: &M, key: i32, rights: u32) -> Result<()> {
    let k = u32::try_from(key).ok().filter(|&k| k < 16).ok_or(Errno::EINVAL)?;
    if rights > 3 {
        return Err(Errno::EINVAL);
    }
    let pkru = m.pkru().ok_or(Errno::ENOSYS)?;
    let shift = k.saturating_mul(2);
    let new = (pkru & !(3 << shift)) | (rights << shift);
    if m.set_pkru(new) { Ok(()) } else { Err(Errno::ENOSYS) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn small_helpers() {
        assert!(needs_mode(0o100) && needs_mode(0o20200000) && !needs_mode(0o2) && !needs_mode(0o200000));
        assert_eq!(fdelt(0), Some(0));
        assert_eq!(fdelt(1023), Some(15));
        assert_eq!((fdelt(1024), fdelt(-1)), (None, None));
        assert_eq!(decimal(b"17"), Some(17));
        assert_eq!(decimal(b"."), None);
        assert_eq!(fcntl_kind(5), FcntlKind::Lock(FcntlLock::GetLk));
        assert_eq!(fcntl_kind(1), FcntlKind::Int(FcntlInt::GetFd));
        assert_eq!(fcntl_kind(16), FcntlKind::OwnerEx(false));
        assert_eq!(fcntl_kind(99999), FcntlKind::Unknown);
    }
}
