//! The C shapes of spec §24.1–24.4: the C numbering of `sysconf`, `confstr` and `pathconf`
//! names (the fixed answers are the reference glibc's, `crate::sysconf_table`; the computed
//! ones are `liella::system`'s), and what glibc adds to the limit, usage and priority calls:
//! `ulimit`, `vlimit`, `vtimes`, `nice`, and `sched_getaffinity`'s zero-filled tail.

use liella::error::{Errno, Result};
use liella::system::{self, Aux, Cache, Query};

/// `gnu_get_libc_version()` (§24.5): the reference's version, NUL-terminated.
pub static LIBC_VERSION: [u8; 5] = *b"2.39\0";
/// `gnu_get_libc_release()` (§24.5).
pub static LIBC_RELEASE: [u8; 7] = *b"stable\0";
use liella_kernel::fd::Fd;
use liella_kernel::machine::Machine;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Rlimit, Rusage, Stat, Statfs};

use crate::sysconf_table::{self, Fixed};

/// The computed `sysconf` names (C numbering, `<bits/confname.h>`).
fn computed(name: i32) -> Option<Query> {
    Some(match name {
        0 => Query::ArgMax,
        1 => Query::ChildMax,
        2 => Query::ClkTck,
        3 => Query::NgroupsMax,
        4 => Query::OpenMax,
        30 => Query::PageSize,
        34 => Query::SigqueueMax,
        75 => Query::ThreadStackMin,
        83 => Query::NprocsConf,
        84 => Query::NprocsOnln,
        85 => Query::PhysPages,
        86 => Query::AvphysPages,
        249 => Query::MinSigStkSz,
        250 => Query::SigStkSz,
        // _SC_LEVEL1_ICACHE_SIZE (185) .. _SC_LEVEL4_CACHE_LINESIZE (199): three fields per
        // cache, level 1 instruction first.
        185..=199 => {
            let i = u32::try_from(name.checked_sub(185)?).ok()?;
            let (cache, field) = (i / 3, i % 3);
            Query::Cache(Cache { level: if cache == 0 { 1 } else { cache }, instruction: cache == 0, field })
        }
        _ => return None,
    })
}

/// `sysconf(name)` (§24.4; D-24.1): `Ok(None)` is -1 without `errno` ("no limit", "not
/// supported"); an unknown name is `EINVAL`.
pub fn sysconf<S: Sys, M: Machine, A: Aux>(sys: &S, m: &M, aux: &A, name: i32) -> Result<Option<i64>> {
    if let Some(q) = computed(name) {
        return system::sysconf(sys, m, aux, q);
    }
    match sysconf_table::sysconf(name) {
        Some(Fixed::Value(-1)) => Ok(None),
        Some(Fixed::Value(v)) => Ok(Some(v)),
        Some(Fixed::Invalid) | None => Err(Errno::EINVAL),
    }
}

/// `confstr(name, buf, len)` (§24.4): the full length of the text including its terminator;
/// the text is copied into `buf`, truncated and terminated, when `buf` is not empty. An
/// unknown name is `EINVAL`.
pub fn confstr(name: i32, buf: &mut [u8]) -> Result<usize> {
    let text = sysconf_table::confstr(name).ok_or(Errno::EINVAL)?;
    if let Some(room) = buf.len().checked_sub(1) {
        let n = room.min(text.len());
        for (d, s) in buf.iter_mut().zip(text.get(..n).unwrap_or(&[]).iter().chain(&[0u8])) {
            *d = *s;
        }
    }
    Ok(text.len().saturating_add(1))
}

/// The file `pathconf` or `fpathconf` asks about.
#[derive(Clone, Copy)]
pub enum Target<'a> {
    /// `pathconf(path, ...)`.
    Path(KStr<'a>),
    /// `fpathconf(fd, ...)`.
    Fd(Fd),
}

impl Target<'_> {
    fn statfs<S: Sys>(self, sys: &S) -> Result<Statfs> {
        match self {
            Target::Path(p) => sys.statfs(p),
            Target::Fd(fd) => sys.fstatfs(fd),
        }
    }
    fn stat<S: Sys>(self, sys: &S) -> Result<Stat> {
        match self {
            Target::Path(p) => sys.fstatat(Fd::CWD, p, 0),
            Target::Fd(fd) => sys.fstat(fd),
        }
    }
}

/// `_PC_PIPE_BUF`: `PIPE_BUF`, the atomic-write size, whatever the file. A pipe's capacity is
/// a different quantity; glibc 2.39 reports 4096 for pipes too.
const PIPE_BUF: i64 = 4096;

/// `pathconf`/`fpathconf` (§24.4): glibc's answers on Linux. File-system-dependent names
/// consult `statfs` (`_PC_LINK_MAX`, `_PC_FILESIZEBITS`, `_PC_2_SYMLINKS` by `f_type`; the
/// block sizes; the name length); `Ok(None)` is -1 without `errno`.
pub fn pathconf<S: Sys>(sys: &S, t: Target<'_>, name: i32) -> Result<Option<i64>> {
    if let Target::Path(p) = t {
        if p.is_empty() {
            return Err(Errno::ENOENT);
        }
    }
    Ok(match name {
        0 => {
            let fs = t.statfs(sys)?;
            Some(system::link_max(sys, &fs, t.stat(sys).map(|s| s.dev).unwrap_or(0)))
        }
        1 | 2 => Some(255),
        3 => Some(t.statfs(sys)?.namelen),
        4 => Some(4096),
        5 => Some(PIPE_BUF),
        6 => {
            t.statfs(sys)?;
            Some(1)
        }
        7 => Some(1),
        8 => Some(0),
        9 | 11 | 12 | 14 | 15 | 19 => None,
        10 => {
            let mode = t.stat(sys)?.mode & 0o170000;
            (mode == 0o100000 || mode == 0o060000).then_some(1)
        }
        13 => Some(system::filesize_bits(&t.statfs(sys)?)),
        16 => Some(t.statfs(sys)?.bsize),
        17 | 18 => {
            let fs = t.statfs(sys)?;
            Some(if fs.frsize != 0 { fs.frsize } else { fs.bsize })
        }
        20 => Some(system::has_symlinks(&t.statfs(sys)?)),
        _ => return Err(Errno::EINVAL),
    })
}

/// `RLIM_INFINITY`.
const INFINITY: u64 = u64::MAX;
const RLIMIT_FSIZE: u32 = 1;

/// `ulimit(cmd, newlimit)` (§24.1, SVID): `UL_GETFSIZE` (1) is the file-size limit in 512-byte
/// blocks (`LONG_MAX` for none), `UL_SETFSIZE` (2) sets it and returns the new value, and
/// `__UL_GETOPENMAX` (4) is `sysconf(_SC_OPEN_MAX)`.
pub fn ulimit<S: Sys, M: Machine, A: Aux>(sys: &S, m: &M, aux: &A, cmd: i32, newlimit: i64) -> Result<i64> {
    match cmd {
        1 => {
            let l = sys.prlimit(0, RLIMIT_FSIZE, None)?;
            Ok(if l.cur == INFINITY { i64::MAX } else { i64::try_from(l.cur / 512).unwrap_or(i64::MAX) })
        }
        2 => {
            let old = sys.prlimit(0, RLIMIT_FSIZE, None)?;
            let as_unsigned = u64::from_ne_bytes(newlimit.to_ne_bytes());
            let (cur, len) = if as_unsigned > INFINITY / 512 { (INFINITY, i64::MAX / 512) } else { (as_unsigned.saturating_mul(512), newlimit) };
            sys.prlimit(0, RLIMIT_FSIZE, Some(&Rlimit { cur, max: old.max }))?;
            Ok(len)
        }
        4 => Ok(system::sysconf(sys, m, aux, Query::OpenMax)?.unwrap_or(-1)),
        _ => Err(Errno::EINVAL),
    }
}

/// `vlimit(resource, value)` (§24.1, compat): `LIM_CPU` (1) .. `LIM_MAXRSS` (6) are the
/// resource limits numbered one higher; the soft limit becomes `value`.
pub fn vlimit<S: Sys>(sys: &S, resource: i32, value: i32) -> Result<()> {
    if !(1..=6).contains(&resource) {
        return Err(Errno::EINVAL);
    }
    let res = u32::try_from(resource.saturating_sub(1)).map_err(|_| Errno::EINVAL)?;
    let old = sys.prlimit(0, res, None)?;
    let cur = u64::from_ne_bytes(i64::from(value).to_ne_bytes());
    sys.prlimit(0, res, Some(&Rlimit { cur, max: old.max })).map(|_| ())
}

/// `struct vtimes` (§24.2, compat): CPU times in 1/60 s and the usage counts.
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
#[repr(C)]
pub struct Vtimes {
    /// User time (1/60 s).
    pub utime: i32,
    /// System time (1/60 s).
    pub stime: i32,
    /// Integral data and stack size.
    pub idsrss: u32,
    /// Integral text size.
    pub ixrss: u32,
    /// Peak resident set.
    pub maxrss: i32,
    /// Major faults.
    pub majflt: i32,
    /// Minor faults.
    pub minflt: i32,
    /// Swaps.
    pub nswap: i32,
    /// Blocks in.
    pub inblk: i32,
    /// Blocks out.
    pub oublk: i32,
}

fn sixtieths(sec: i64, usec: i64) -> i32 {
    i32::try_from(sec.saturating_mul(60).saturating_add(usec.saturating_mul(60) / 1_000_000)).unwrap_or(i32::MAX)
}

fn narrow(v: i64) -> i32 {
    i32::try_from(v).unwrap_or(i32::MAX)
}

fn vtimes_of(r: &Rusage) -> Vtimes {
    Vtimes {
        utime: sixtieths(r.utime.sec, r.utime.usec),
        stime: sixtieths(r.stime.sec, r.stime.usec),
        idsrss: u32::try_from(r.idrss.saturating_add(r.isrss)).unwrap_or(u32::MAX),
        ixrss: u32::try_from(r.ixrss).unwrap_or(u32::MAX),
        maxrss: narrow(r.maxrss),
        majflt: narrow(r.majflt),
        minflt: narrow(r.minflt),
        nswap: narrow(r.nswap),
        inblk: narrow(r.inblock),
        oublk: narrow(r.oublock),
    }
}

/// `vtimes(current, child)` (§24.2, compat): `getrusage` for the process and for its
/// waited-for children, in the 4.2BSD shape.
pub fn vtimes<S: Sys>(sys: &S) -> Result<(Vtimes, Vtimes)> {
    Ok((vtimes_of(&sys.getrusage(0)?), vtimes_of(&sys.getrusage(-1)?)))
}

/// `nice(inc)` (§24.3): add to the nice value and return the new one. -1 is a valid result;
/// `EACCES` from the kernel is reported as `EPERM` (glibc, POSIX).
pub fn nice<S: Sys>(sys: &S, inc: i32) -> Result<i64> {
    let prio = getpriority(sys, 0, 0)?;
    match sys.setpriority(0, 0, i32::try_from(prio).unwrap_or(0).saturating_add(inc)) {
        Err(Errno::EACCES) => return Err(Errno::EPERM),
        Err(e) => return Err(e),
        Ok(()) => {}
    }
    getpriority(sys, 0, 0)
}

/// `getpriority(which, who)` (§24.3): the nice value (-20..19), the kernel's bias undone.
pub fn getpriority<S: Sys>(sys: &S, which: u32, who: u32) -> Result<i64> {
    sys.getpriority(which, who).map(|p| i64::from(liella_kernel::uapi::nice_from_priority(p)))
}

/// `sched_getaffinity(pid, size, mask)` (§24.3): the kernel writes the bytes it knows of;
/// glibc zeroes the rest of the caller's set.
pub fn getaffinity<S: Sys>(sys: &S, pid: i32, mask: &mut [u8]) -> Result<()> {
    let n = sys.sched_getaffinity(pid, mask)?;
    for b in mask.iter_mut().skip(n) {
        *b = 0;
    }
    Ok(())
}

/// `__sched_cpucount(size, set)` (`CPU_COUNT_S`): the CPUs in the set.
pub fn cpu_count(set: &[u8]) -> i64 {
    set.iter().map(|b| i64::from(b.count_ones())).sum()
}

/// `CPU_ALLOC_SIZE(count)`: whole `unsigned long` words.
pub fn cpu_alloc_size(count: usize) -> usize {
    count.div_ceil(64).saturating_mul(8)
}

/// `sched_yield()` (§24.3): the system call cannot fail; 0.
pub fn sched_yield<S: Sys>(sys: &S) -> Result<()> {
    sys.sched_yield();
    Ok(())
}

// ------------------------------------------------------------------ §24.5 host identifier

/// Where `gethostid` and `sethostid` keep the identifier (`gethostid(3)`), NUL-terminated.
pub const HOSTID_FILE: &[u8] = b"/etc/hostid\0";

/// `gethostid()` (§24.5) with the identifier file `file` (NUL-terminated): its first 4 bytes
/// (native order); without them, the host's IPv4 address (its node name looked up in the
/// `hosts` database through the switch, `crate::netdb_compat`) read as a native-order `u32`
/// with its 16-bit halves exchanged (the man page's "bit-twiddling"; observed: 127.0.1.1 gives
/// 0x007f0101); without one, 0. A file shorter than 4 bytes, or empty, counts as absent; of a
/// longer one only the first 4 bytes
/// count (observed in a private mount namespace, 2026-10-03).
pub fn gethostid<S: Sys>(sys: &S, file: &[u8]) -> i32 {
    if let Ok(text) = system::read_file(sys, file) {
        if let Some(b) = text.get(..4).and_then(|b| <[u8; 4]>::try_from(b).ok()) {
            return i32::from_ne_bytes(b);
        }
    }
    let Ok(u) = sys.uname() else { return 0 };
    let end = u.nodename.iter().position(|&c| c == 0).unwrap_or(u.nodename.len());
    let name = u.nodename.get(..end).unwrap_or(&[]);
    match crate::netdb_compat::resolve(sys, name, liella::names::Family::V4) {
        Ok(Some(crate::netdb_compat::Resolved { addr: liella::names::Ip::V4(a), .. })) => i32::from_ne_bytes(u32::from_ne_bytes(a).rotate_left(16).to_ne_bytes()),
        _ => 0,
    }
}

/// `sethostid(id)` (§24.5) with the identifier file `file` (NUL-terminated): its 4 bytes,
/// native order, replace the file's contents. `EOVERFLOW` for a value outside `int32_t`
/// (observed: checked first), `EPERM` when the effective user or group ID differs from the real
/// one (the man page), otherwise the error of creating or writing the file (observed: `EACCES`
/// for a user who may not write `/etc`).
pub fn sethostid<S: Sys>(sys: &S, file: &[u8], id: i64) -> Result<()> {
    let v = i32::try_from(id).map_err(|_| Errno::EOVERFLOW)?;
    if sys.geteuid() != sys.getuid() || sys.getegid() != sys.getgid() {
        return Err(Errno::EPERM);
    }
    use liella_kernel::uapi::{O_CLOEXEC, O_CREAT, O_TRUNC, O_WRONLY};
    let fd = sys.openat(Fd::CWD, KStr::from_nul_terminated(file)?, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0o644)?;
    let r = sys.write(fd, &v.to_ne_bytes()).and_then(|n| if n == 4 { Ok(()) } else { Err(Errno::ENOSPC) });
    let c = sys.close(fd);
    r.and(c)
}
