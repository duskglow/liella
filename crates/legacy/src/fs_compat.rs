//! The C shapes of spec §13.4–13.7 and §13.10 that add something to the kernel call. The thin
//! calls are the typed `Sys` methods themselves; what is here is what glibc's contract adds:
//! `remove`'s two-step, `mknod`'s device-number check, `mkfifo`, the no-follow `chmod` without
//! `fchmodat2`, `faccessat`'s flags without `faccessat2`, the microsecond and second timestamp
//! forms, `statvfs` from `statfs`, and the `__xstat`/`__xmknod` version arguments.

use liella::error::{Errno, Result};
use liella_abi::layout::struct_statvfs;
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{
    Stat, Statfs, Timespec, Timeval, AT_EACCESS, AT_EMPTY_PATH, AT_REMOVEDIR, AT_SYMLINK_NOFOLLOW, F_OK, O_CLOEXEC, O_NOFOLLOW, O_PATH, S_IFIFO, S_IFLNK,
    S_IFMT, X_OK,
};

// ------------------------------------------------------------------------------ §13.4

/// `remove(path)` (§13.4, ISO C): unlink a non-directory, else (`EISDIR`) remove an empty
/// directory. glibc's order: the common case first.
pub fn remove<S: Sys>(sys: &S, path: KStr<'_>) -> Result<()> {
    match sys.unlinkat(Fd::CWD, path, 0) {
        Err(Errno::EISDIR) => sys.unlinkat(Fd::CWD, path, AT_REMOVEDIR),
        r => r,
    }
}

// ------------------------------------------------------------------------------ §13.5

/// `mknodat(dir, path, mode, dev)` (§13.5): the kernel takes a 32-bit device number, so a
/// `dev_t` beyond it is `EINVAL` (glibc's check) rather than silently truncated.
pub fn mknodat<S: Sys>(sys: &S, dir: Fd, path: KStr<'_>, mode: u32, dev: u64) -> Result<()> {
    sys.mknodat(dir, path, mode, kernel_dev(dev)?)
}

/// A `dev_t` as the kernel's 32-bit device number (`mknod`, `ustat`): `EINVAL` beyond it.
pub fn kernel_dev(dev: u64) -> Result<u32> {
    u32::try_from(dev).map_err(|_| Errno::EINVAL)
}

/// `mkfifoat(dir, path, mode)` (§13.5): a FIFO node.
pub fn mkfifoat<S: Sys>(sys: &S, dir: Fd, path: KStr<'_>, mode: u32) -> Result<()> {
    sys.mknodat(dir, path, (mode & !S_IFMT) | S_IFIFO, 0)
}

/// `_MKNOD_VER` (x86-64): the only version `__xmknod`/`__xmknodat` accept (⚑ COMPAT).
pub const MKNOD_VER: i32 = 0;

/// The `__xmknod` family's version argument (§13.5).
pub fn mknod_version(vers: i32) -> Result<()> {
    if vers == MKNOD_VER { Ok(()) } else { Err(Errno::EINVAL) }
}

// ------------------------------------------------------------------------------ §13.6

/// The `__xstat` family's version argument (§13.6, ⚑ COMPAT): on x86-64 `struct stat` is the
/// kernel's, and both `_STAT_VER_KERNEL` (0) and `_STAT_VER_LINUX` (1) name it.
pub fn stat_version(vers: i32) -> Result<()> {
    if vers == 0 || vers == 1 { Ok(()) } else { Err(Errno::EINVAL) }
}

// ------------------------------------------------------------------------------ §13.7

/// `/proc/self/fd/<fd>` with its terminator, in `buf`.
fn proc_fd_path(fd: Fd, buf: &mut [u8; 32]) -> Result<KStr<'_>> {
    const PREFIX: &[u8] = b"/proc/self/fd/";
    let n = u32::try_from(fd.raw()).map_err(|_| Errno::EBADF)?;
    let mut d = [0u8; 20];
    let digits = liella_rt::fatal::decimal(u64::from(n), &mut d);
    for (slot, &b) in buf.iter_mut().zip(PREFIX.iter().chain(digits).chain(&[0u8])) {
        *slot = b;
    }
    let len = PREFIX.len().saturating_add(digits.len()).saturating_add(1);
    KStr::from_nul_terminated(buf.get(..len).ok_or(Errno::ENAMETOOLONG)?)
}

/// `fchmodat(dir, path, mode, flags)` (§13.7). Without `AT_SYMLINK_NOFOLLOW` it is the
/// system call. With it: `fchmodat2` (Linux 6.6), else glibc's fallback, an `O_PATH`
/// descriptor checked not to be a symlink (Linux symlinks have no mode: `EOPNOTSUPP`) and
/// changed through `/proc/self/fd`. Without `/proc` the fallback cannot work: `EOPNOTSUPP`
/// (fchmodat(2): the flag "is not supported"; as the reference), never the `ENOENT` of the
/// missing `/proc` path, which would say the file does not exist (milestone 6, 2026-10-05).
pub fn fchmodat<S: Sys>(sys: &S, dir: Fd, path: KStr<'_>, mode: u32, flags: u32) -> Result<()> {
    if flags & !AT_SYMLINK_NOFOLLOW != 0 {
        return Err(Errno::EINVAL);
    }
    if flags == 0 {
        return sys.fchmodat(dir, path, mode);
    }
    match sys.fchmodat2(dir, path, mode, flags) {
        Err(Errno::ENOSYS) => {}
        r => return r,
    }
    let fd = sys.openat(dir, path, O_PATH | O_NOFOLLOW | O_CLOEXEC, 0)?;
    let r = (|| {
        let st = sys.fstatat(fd, KStr::from_nul_terminated(b"\0")?, AT_EMPTY_PATH)?;
        if st.mode & S_IFMT == S_IFLNK {
            return Err(Errno::EOPNOTSUPP);
        }
        let mut buf = [0u8; 32];
        // The descriptor holds the file, so its `/proc/self/fd` entry is missing only when
        // `/proc` is.
        sys.fchmodat(Fd::CWD, proc_fd_path(fd, &mut buf)?, mode).map_err(|e| if e == Errno::ENOENT { Errno::EOPNOTSUPP } else { e })
    })();
    let _ = sys.close(fd);
    r
}

/// `faccessat(dir, path, mode, flags)` (§13.7). Without flags it is the system call; with
/// `AT_EACCESS` or `AT_SYMLINK_NOFOLLOW` it is `faccessat2`, and where that is missing
/// (`ENOSYS`, for example under a seccomp filter) glibc's emulation: the answer computed from
/// `fstatat` and the credentials. The emulation is the classic permission-bit check, so it
/// does not see ACLs or capabilities beyond root's.
pub fn faccessat<S: Sys>(sys: &S, dir: Fd, path: KStr<'_>, mode: u32, flags: u32) -> Result<()> {
    if flags & !(AT_EACCESS | AT_SYMLINK_NOFOLLOW) != 0 {
        return Err(Errno::EINVAL);
    }
    if flags == 0 {
        return sys.faccessat(dir, path, mode);
    }
    match sys.faccessat2(dir, path, mode, flags) {
        Err(Errno::ENOSYS) => {}
        r => return r,
    }
    let (ruid, euid, _) = sys.getresuid()?;
    let (rgid, egid, _) = sys.getresgid()?;
    if (flags & AT_EACCESS == 0 || (ruid == euid && rgid == egid)) && flags & AT_SYMLINK_NOFOLLOW == 0 {
        return sys.faccessat(dir, path, mode);
    }
    let st = sys.fstatat(dir, path, flags & AT_SYMLINK_NOFOLLOW)?;
    access_from_stat(sys, &st, mode, flags & AT_EACCESS != 0, (ruid, euid), (rgid, egid))
}

/// The permission-bit answer of the `faccessat` emulation.
fn access_from_stat<S: Sys>(sys: &S, st: &Stat, mode: u32, effective: bool, uids: (u32, u32), gids: (u32, u32)) -> Result<()> {
    if mode & !7 != 0 {
        return Err(Errno::EINVAL);
    }
    if mode == F_OK {
        return Ok(());
    }
    let uid = if effective { uids.1 } else { uids.0 };
    let gid = if effective { gids.1 } else { gids.0 };
    // Root may do anything, except execute a file no one may execute.
    if uid == 0 && (mode & X_OK == 0 || st.mode & 0o111 != 0) {
        return Ok(());
    }
    let granted = if uid == st.uid {
        (st.mode >> 6) & 7
    } else if crate::cred_compat::in_group(sys, st.gid, gid)? {
        (st.mode >> 3) & 7
    } else {
        st.mode & 7
    };
    if granted & mode == mode { Ok(()) } else { Err(Errno::EACCES) }
}

/// The `utimes`/`futimes`/`lutimes`/`futimesat` pair (µs) as `utimensat`'s (§13.7): a
/// microsecond field outside `[0, 1_000_000)` is `EINVAL`.
pub fn timevals(tv: Option<&[Timeval; 2]>) -> Result<Option<[Timespec; 2]>> {
    let Some(tv) = tv else { return Ok(None) };
    let one = |t: &Timeval| -> Result<Timespec> {
        if !(0..1_000_000).contains(&t.usec) {
            return Err(Errno::EINVAL);
        }
        Ok(Timespec { sec: t.sec, nsec: t.usec.saturating_mul(1000) })
    };
    let [a, m] = tv;
    Ok(Some([one(a)?, one(m)?]))
}

/// `utime`'s `struct utimbuf` (`actime`, `modtime`, whole seconds) as `utimensat`'s pair.
pub fn utimbuf(times: Option<&[i64; 2]>) -> Option<[Timespec; 2]> {
    times.map(|t| [Timespec { sec: t[0], nsec: 0 }, Timespec { sec: t[1], nsec: 0 }])
}

// ------------------------------------------------------------------------------ §13.10

/// `ST_VALID`: the kernel filled `f_flags` (always, on the supported kernels; D-13.8).
const ST_VALID: i64 = 0x20;

/// `statvfs` from `statfs` (§13.10; D-13.8): `f_flag` is `statfs.f_flags` without
/// `ST_VALID`, `f_fsid` packs both halves (glibc's formula, the low half sign-extended),
/// `f_favail` is `f_ffree`, and `f_type` is the file system's magic (2.39).
pub fn statvfs(s: &Statfs) -> struct_statvfs {
    let [lo, hi] = s.fsid;
    let fsid = u64::from_ne_bytes(i64::from(lo).to_ne_bytes()) | (u64::from(u32::from_ne_bytes(hi.to_ne_bytes())) << 32);
    struct_statvfs {
        f_bsize: u64::from_ne_bytes(s.bsize.to_ne_bytes()),
        f_frsize: u64::from_ne_bytes((if s.frsize != 0 { s.frsize } else { s.bsize }).to_ne_bytes()),
        f_blocks: s.blocks,
        f_bfree: s.bfree,
        f_bavail: s.bavail,
        f_files: s.files,
        f_ffree: s.ffree,
        f_favail: s.ffree,
        f_fsid: fsid,
        f_flag: u64::from_ne_bytes((s.flags & !ST_VALID).to_ne_bytes()),
        f_namemax: u64::from_ne_bytes(s.namelen.to_ne_bytes()),
        // Magic numbers are 32-bit values the kernel widens.
        f_type: u32::try_from(s.ftype & 0xffff_ffff).unwrap_or(0),
        __f_spare: [0; 5],
    }
}
