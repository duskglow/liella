//! Resource limits, scheduling, configuration queries, system identity (spec §24).
//!
//! Phase 5 holds the configuration queries of §24.4 that are computed rather than fixed:
//! [`Query`] and [`sysconf`] (D-24.1: `Ok(None)` means "no limit"), the processor counts read
//! from sysfs, the memory counts from `sysinfo`, the cache geometry from CPUID, the
//! signal-stack minimums, and the per-file-system answers of `pathconf`. The C numbering of the
//! names, and the answers that are fixed, are in `liella_legacy::sys_compat`.

use crate::config;
use crate::error::{Errno, Result};
use crate::mem::TryVec;
use liella_kernel::fd::Fd;
use liella_kernel::machine::Machine;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Rlimit, Statfs, O_CLOEXEC, O_RDONLY};

/// `RLIM_INFINITY`.
const INFINITY: u64 = u64::MAX;

/// A configuration value computed when asked.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Query {
    /// `_SC_ARG_MAX`: the kernel's limit for `execve` (execve(2)): a quarter of the stack
    /// limit, at most 3/4 of the kernel's 8 MiB `_STK_LIM` (so 6 MiB for an unlimited stack),
    /// at least the historical 128 KiB.
    ArgMax,
    /// `_SC_CHILD_MAX`: `RLIMIT_NPROC`.
    ChildMax,
    /// `_SC_CLK_TCK`: the kernel's `AT_CLKTCK`, else 100.
    ClkTck,
    /// `_SC_NGROUPS_MAX`: `/proc/sys/kernel/ngroups_max`, else 65536.
    NgroupsMax,
    /// `_SC_OPEN_MAX`: `RLIMIT_NOFILE` (capped at `INT_MAX`).
    OpenMax,
    /// `_SC_PAGESIZE`: the kernel's `AT_PAGESZ`.
    PageSize,
    /// `_SC_SIGQUEUE_MAX`: `RLIMIT_SIGPENDING`.
    SigqueueMax,
    /// `_SC_NPROCESSORS_CONF`: [`nprocs_conf`].
    NprocsConf,
    /// `_SC_NPROCESSORS_ONLN`: [`nprocs`].
    NprocsOnln,
    /// `_SC_PHYS_PAGES`.
    PhysPages,
    /// `_SC_AVPHYS_PAGES`.
    AvphysPages,
    /// `_SC_LEVEL*`: a cache's size, associativity or line size ([`Cache`]).
    Cache(Cache),
    /// `_SC_MINSIGSTKSZ`.
    MinSigStkSz,
    /// `_SC_SIGSTKSZ`.
    SigStkSz,
    /// `_SC_THREAD_STACK_MIN`.
    ThreadStackMin,
}

/// One cache field of `_SC_LEVEL1_ICACHE_SIZE` .. `_SC_LEVEL4_CACHE_LINESIZE`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Cache {
    /// 1–4.
    pub level: u32,
    /// For level 1: the instruction cache (else the data cache).
    pub instruction: bool,
    /// 0 size, 1 associativity, 2 line size.
    pub field: u32,
}

/// The inputs the queries read besides the kernel calls: the auxiliary vector.
pub trait Aux {
    /// The value of auxiliary-vector entry `key`, if the kernel supplied it.
    fn aux(&self, key: u64) -> Option<u64>;
}

const AT_PAGESZ: u64 = 6;
const AT_CLKTCK: u64 = 17;
const AT_MINSIGSTKSZ: u64 = 51;

/// glibc's historical `ARG_MAX` floor.
const LEGACY_ARG_MAX: i64 = 131_072;
/// The kernel's cap on argument and environment space: 3/4 of `_STK_LIM` (8 MiB), execve(2).
const KERNEL_ARG_MAX: i64 = 6_291_456;
/// The constant `MINSIGSTKSZ` and `SIGSTKSZ` of the x86-64 headers.
const MINSIGSTKSZ: i64 = 2048;
const SIGSTKSZ: i64 = 8192;
/// `PTHREAD_STACK_MIN` (x86-64).
const PTHREAD_STACK_MIN: i64 = 16384;

fn soft_limit<S: Sys>(sys: &S, resource: u32) -> Result<Rlimit> {
    sys.prlimit(0, resource, None)
}

/// A limit as `sysconf` reports it: `RLIM_INFINITY` is "no limit".
fn limit(v: u64) -> Option<i64> {
    if v == INFINITY { None } else { i64::try_from(v).ok() }
}

/// Reads a small file whole (sysfs and procfs texts) into a growable buffer.
pub fn read_file<S: Sys>(sys: &S, path: &[u8]) -> Result<TryVec<u8>> {
    let fd = sys.openat(Fd::CWD, KStr::from_nul_terminated(path)?, O_RDONLY | O_CLOEXEC, 0)?;
    let r = (|| {
        let mut out = TryVec::with_capacity(4096)?;
        let mut chunk = [0u8; 4096];
        loop {
            let n = sys.read(fd, &mut chunk)?;
            if n == 0 {
                return Ok(out);
            }
            out.extend_from_slice(chunk.get(..n).unwrap_or(&[]))?;
        }
    })();
    let _ = sys.close(fd);
    r
}

/// A copy of the first line of the file `path` that `pred` accepts (lines as `text.split` at
/// `\n` gives them, the text after the last newline included), or `None`. The same answer as
/// searching the text [`read_file`] returns, but reading stops at the line found.
pub fn find_line<S: Sys>(sys: &S, path: &[u8], mut pred: impl FnMut(&[u8]) -> bool) -> Result<Option<TryVec<u8>>> {
    let fd = sys.openat(Fd::CWD, KStr::from_nul_terminated(path)?, O_RDONLY | O_CLOEXEC, 0)?;
    let r = (|| {
        // The start of a line that continues in the next chunk.
        let mut pending: TryVec<u8> = TryVec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = sys.read(fd, &mut chunk)?;
            if n == 0 {
                return Ok(pred(pending.as_slice()).then_some(pending));
            }
            let mut rest = chunk.get(..n).unwrap_or(&[]);
            while let Some(i) = rest.iter().position(|&c| c == b'\n') {
                let line = rest.get(..i).unwrap_or(&[]);
                rest = rest.get(i.saturating_add(1)..).unwrap_or(&[]);
                if pending.is_empty() {
                    if pred(line) {
                        return Ok(Some(TryVec::from_slice(line)?));
                    }
                } else {
                    pending.extend_from_slice(line)?;
                    if pred(pending.as_slice()) {
                        return Ok(Some(pending));
                    }
                    pending.clear();
                }
            }
            pending.extend_from_slice(rest)?;
        }
    })();
    let _ = sys.close(fd);
    r
}

// ------------------------------------------------------------------------------ §24.5–24.6

/// A `utsname` field's text (up to its NUL; the kernel always terminates the 65 bytes).
fn uts_text(field: &[u8; 65]) -> &[u8] {
    let n = field.iter().position(|&b| b == 0).unwrap_or(field.len());
    field.get(..n).unwrap_or(&[])
}

/// Copy `text` and its NUL into `buf`, as much as fits: whether it all fitted. A name that does
/// not fit leaves `buf.len()` bytes of it, unterminated (the reference's behaviour, probed
/// 2026-10-04: `gethostname(buf, 4)` with a 4-byte node name writes the four bytes).
fn copy_name(text: &[u8], buf: &mut [u8]) -> bool {
    for (d, s) in buf.iter_mut().zip(text.iter().chain(core::iter::once(&0u8))) {
        *d = *s;
    }
    buf.len() > text.len()
}

/// `gethostname(buf, n)` (§24.5): the node name of `uname` and its NUL; `ENAMETOOLONG` when it
/// does not fit (the bytes that fit are written).
pub fn hostname<S: Sys>(sys: &S, buf: &mut [u8]) -> Result<()> {
    let u = sys.uname()?;
    if copy_name(uts_text(&u.nodename), buf) { Ok(()) } else { Err(Errno::ENAMETOOLONG) }
}

/// `getdomainname(buf, n)` (§24.5): the NIS domain name of `uname` and its NUL, silently cut to
/// `n` bytes when longer (getdomainname(2): "returns the first len bytes"; the reference, probed:
/// no error).
pub fn domainname<S: Sys>(sys: &S, buf: &mut [u8]) -> Result<()> {
    let u = sys.uname()?;
    copy_name(uts_text(&u.domainname), buf);
    Ok(())
}

/// `getloadavg(out)` (§24.6): up to three load averages, from `/proc/loadavg` (the 1, 5 and 15
/// minute averages, two decimals each); how many were stored (at most `out.len()` and 3).
pub fn loadavg<S: Sys>(sys: &S, out: &mut [f64]) -> Result<usize> {
    let mut text = [0u8; 128];
    let fd = sys.openat(Fd::CWD, KStr::from_nul_terminated(b"/proc/loadavg\0")?, O_RDONLY | O_CLOEXEC, 0)?;
    let n = sys.read(fd, &mut text);
    let _ = sys.close(fd);
    let text = text.get(..n?).unwrap_or(&[]);
    let mut stored = 0usize;
    for (slot, field) in out.iter_mut().take(3).zip(text.split(|&b| b == b' ')) {
        *slot = decimal_fraction(field).ok_or(Errno::EINVAL)?;
        stored = stored.saturating_add(1);
    }
    Ok(stored)
}

/// A non-negative decimal such as `0.42` or `12.5` as a double.
fn decimal_fraction(t: &[u8]) -> Option<f64> {
    let (int, frac) = match t.iter().position(|&b| b == b'.') {
        Some(i) => (t.get(..i)?, t.get(i.checked_add(1)?..)?),
        None => (t, &b""[..]),
    };
    if int.is_empty() || !int.iter().chain(frac).all(u8::is_ascii_digit) || frac.len() > 9 {
        return None;
    }
    let digits = |s: &[u8]| s.iter().try_fold(0u64, |v, &d| v.checked_mul(10)?.checked_add(u64::from(d.wrapping_sub(b'0'))));
    let (i, f) = (digits(int)?, digits(frac)?);
    let scale = 10u64.checked_pow(u32::try_from(frac.len()).ok()?)?;
    // Exact for the values the kernel prints (the integer part far below 2^53).
    Some(u32::try_from(i).map(f64::from).ok()? + f64::from(u32::try_from(f).ok()?) / f64::from(u32::try_from(scale).ok()?))
}

/// `PROC_SUPER_MAGIC`: the file-system type `fstatfs` reports for procfs (⚑ KERN, statfs(2)).
const PROC_SUPER_MAGIC: i64 = 0x9fa0;

/// The calling thread's seccomp mode as procfs reports it, read without `prctl`: the `Seccomp:`
/// line of `/proc/thread-self/status` (proc(5): 0 none, 1 strict, 2 filter; the calling
/// thread's, as `prctl(PR_GET_SECCOMP)` answers, since a filter belongs to a thread). `None` when
/// the file cannot be opened or read, is not procfs's (`fstatfs`: a file mounted over it says
/// nothing), or has no such line (a kernel before 3.8). Allocates nothing.
pub fn seccomp_mode<S: Sys>(sys: &S) -> Option<u32> {
    let path = KStr::from_nul_terminated(b"/proc/thread-self/status\0").ok()?;
    let fd = sys.openat(Fd::CWD, path, O_RDONLY | O_CLOEXEC, 0).ok()?;
    let mut mode = None;
    if sys.fstatfs(fd).is_ok_and(|s| s.ftype == PROC_SUPER_MAGIC) {
        let mut field = StatusField::new(b"Seccomp:");
        let mut chunk = [0u8; 512];
        while let Ok(n @ 1..) = sys.read(fd, &mut chunk) {
            if let Some(v) = field.feed(chunk.get(..n).unwrap_or(&[])) {
                mode = v;
                break;
            }
        }
    }
    let _ = sys.close(fd);
    mode.and_then(|v| u32::try_from(v).ok())
}

/// A scanner for one numeric field of a procfs `status` text, fed in chunks: the line that
/// starts with `key` (its colon included), then blanks, decimal digits and a newline. The first
/// line starting with `key` decides.
pub struct StatusField<'k> {
    key: &'k [u8],
    /// Bytes of a newline followed by `key` matched (the text's start counts as a newline).
    matched: usize,
    inside: bool,
    value: Option<u64>,
}

impl<'k> StatusField<'k> {
    /// A scanner for `key`.
    pub fn new(key: &'k [u8]) -> StatusField<'k> {
        StatusField { key, matched: 1, inside: false, value: None }
    }

    /// Feed the next chunk: `Some` once the field's line has been read (`Some(None)`: the line
    /// is not a number), `None` while it has not been seen whole.
    pub fn feed(&mut self, chunk: &[u8]) -> Option<Option<u64>> {
        for &b in chunk {
            if self.inside {
                match b {
                    b' ' | b'\t' if self.value.is_none() => {}
                    b'0'..=b'9' => match self.value.unwrap_or(0).checked_mul(10).and_then(|v| v.checked_add(u64::from(b.wrapping_sub(b'0')))) {
                        Some(v) => self.value = Some(v),
                        None => return Some(None),
                    },
                    b'\n' => return Some(self.value),
                    _ => return Some(None),
                }
                continue;
            }
            let want = match self.matched {
                0 => Some(&b'\n'),
                m => self.key.get(m.saturating_sub(1)),
            };
            if want == Some(&b) {
                self.matched = self.matched.saturating_add(1);
                self.inside = self.matched == self.key.len().saturating_add(1);
            } else {
                self.matched = usize::from(b == b'\n');
            }
        }
        None
    }
}

/// The number of CPUs in a sysfs list such as `0-3,8,10-11`.
pub fn count_cpu_list(text: &[u8]) -> Option<i64> {
    let mut total: i64 = 0;
    for part in text.split(|&b| b == b',') {
        let part = part.trim_ascii();
        if part.is_empty() {
            continue;
        }
        let mut ends = part.splitn(2, |&b| b == b'-');
        let lo = decimal(ends.next()?)?;
        let hi = match ends.next() {
            Some(h) => decimal(h)?,
            None => lo,
        };
        total = total.checked_add(hi.checked_sub(lo)?.checked_add(1)?)?;
    }
    (total > 0).then_some(total)
}

fn decimal(s: &[u8]) -> Option<i64> {
    if s.is_empty() {
        return None;
    }
    s.iter().try_fold(0i64, |acc, &b| acc.checked_mul(10)?.checked_add(i64::from(char::from(b).to_digit(10)?)))
}

/// The `cpuN` lines of `/proc/stat` (the fallback of both processor counts).
fn proc_stat_cpus<S: Sys>(sys: &S) -> Option<i64> {
    let text = read_file(sys, b"/proc/stat\0").ok()?;
    let n = config::lines(text.as_slice())
        .filter(|l| l.len() > 3 && l.starts_with(b"cpu") && l.get(3).is_some_and(u8::is_ascii_digit))
        .count();
    i64::try_from(n).ok().filter(|&n| n > 0)
}

/// `get_nprocs()` (§24.4): the online processors, from `/sys/devices/system/cpu/online`,
/// falling back to `/proc/stat`, then to the affinity mask; at least 1.
pub fn nprocs<S: Sys>(sys: &S) -> i64 {
    if let Some(n) = read_file(sys, b"/sys/devices/system/cpu/online\0").ok().and_then(|t| count_cpu_list(t.as_slice())) {
        return n;
    }
    if let Some(n) = proc_stat_cpus(sys) {
        return n;
    }
    let mut mask = [0u8; 128];
    match sys.sched_getaffinity(0, &mut mask) {
        Ok(len) => {
            let ones: u32 = mask.get(..len).unwrap_or(&[]).iter().map(|b| b.count_ones()).sum();
            i64::from(ones.max(1))
        }
        Err(_) => 1,
    }
}

/// `get_nprocs_conf()` (§24.4): the configured processors, the `cpuN` entries of
/// `/sys/devices/system/cpu/`, falling back to `/proc/stat`, then to [`nprocs`].
pub fn nprocs_conf<S: Sys>(sys: &S) -> i64 {
    let counted = (|| -> Result<i64> {
        let dir = sys.openat(Fd::CWD, KStr::from_nul_terminated(b"/sys/devices/system/cpu\0")?, O_RDONLY | O_CLOEXEC | 0o200000, 0)?;
        let mut buf = [0u8; 4096];
        let mut n: i64 = 0;
        let r = loop {
            let len = match sys.getdents64(dir, &mut buf) {
                Ok(0) => break Ok(n),
                Ok(len) => len,
                Err(e) => break Err(e),
            };
            let mut off = 0usize;
            while let Some(rec) = buf.get(off..len) {
                // struct linux_dirent64: ino (8), off (8), reclen (2), type (1), name.
                let Some(&[r0, r1]) = rec.get(16..18) else { break };
                let reclen = usize::from(u16::from_ne_bytes([r0, r1]));
                if reclen == 0 {
                    break;
                }
                let name = rec.get(19..reclen).unwrap_or(&[]);
                let name = name.split(|&b| b == 0).next().unwrap_or(&[]);
                if name.len() > 3 && name.starts_with(b"cpu") && name.get(3..).is_some_and(|d| d.iter().all(u8::is_ascii_digit)) {
                    n = n.saturating_add(1);
                }
                off = off.saturating_add(reclen);
            }
        };
        let _ = sys.close(dir);
        r
    })();
    match counted {
        Ok(n) if n > 0 => n,
        _ => proc_stat_cpus(sys).unwrap_or_else(|| nprocs(sys)),
    }
}

/// `_SC_PHYS_PAGES` / `_SC_AVPHYS_PAGES`: `sysinfo`'s total or free memory in pages.
pub fn pages<S: Sys>(sys: &S, page: i64, available: bool) -> Result<i64> {
    let si = sys.sysinfo()?;
    let unit = u64::from(si.mem_unit.max(1));
    let bytes = if available { si.freeram } else { si.totalram }.saturating_mul(unit);
    let page = u64::try_from(page).ok().filter(|&p| p > 0).ok_or(Errno::EINVAL)?;
    i64::try_from(bytes.checked_div(page).unwrap_or(0)).map_err(|_| Errno::EOVERFLOW)
}

/// The minimum signal stack (`_SC_MINSIGSTKSZ`): the kernel's `AT_MINSIGSTKSZ`; without it
/// (kernels before 5.14) the signal frame computed from the XSAVE area size, as glibc
/// emulates it; without XSAVE the constant `MINSIGSTKSZ`.
pub fn min_sigstksz<A: Aux, M: Machine>(aux: &A, m: &M) -> i64 {
    if let Some(v) = aux.aux(AT_MINSIGSTKSZ).filter(|&v| v != 0) {
        return i64::try_from(v).unwrap_or(MINSIGSTKSZ);
    }
    let [max_leaf, ..] = m.cpuid(0, 0);
    let [_, _, ecx1, _] = m.cpuid(1, 0);
    let osxsave = ecx1 & (1 << 27) != 0;
    if max_leaf >= 0xd && osxsave {
        // sizeof(struct rt_sigframe) + the return address, aligned to 16; up to 64 bytes of
        // padding that align the XSAVE buffer to 64; the buffer; FP_XSTATE_MAGIC2. (Validated on
        // Linux 5.10, which has no AT_MINSIGSTKSZ: 448 + 64 + 832 + 4 = 1348 bytes, the
        // reference's answer on the same CPU; the padding had been counted as 48. Kernel-5.10
        // validation, 2026-10-05.)
        let frame: u32 = (440 + 8 + 15) & !15;
        let [_, xsave, _, _] = m.cpuid(0xd, 0);
        return i64::from(frame).saturating_add(64).saturating_add(i64::from(xsave)).saturating_add(4);
    }
    MINSIGSTKSZ
}

/// `_SC_SIGSTKSZ`: four times the minimum (at least `MINSIGSTKSZ`'s), and at least the
/// constant `SIGSTKSZ`.
pub fn sigstksz(min: i64) -> i64 {
    min.max(MINSIGSTKSZ).saturating_mul(4).max(SIGSTKSZ)
}

/// The cache geometry of the deterministic cache-parameter leaves (Intel leaf 4, AMD
/// 0x8000001D): size, associativity, line size; `None` when the level is absent.
/// Whether the CPU is AMD or Hygon ("AuthenticAMD", "HygonGenuine": EBX and EDX of leaf 0).
fn amd<M: Machine>(m: &M) -> bool {
    let [_, vb, _, vd] = m.cpuid(0, 0);
    (vb, vd) == (0x6874_7541, 0x6974_6e65) || (vb, vd) == (0x6f67_6e48, 0x656e_6975)
}

fn cache_leaf<M: Machine>(m: &M, level: u32, instruction: bool) -> Option<[i64; 3]> {
    let [max_leaf, ..] = m.cpuid(0, 0);
    let leaf = if amd(m) {
        let [ext_max, ..] = m.cpuid(0x8000_0000, 0);
        if ext_max < 0x8000_001d {
            return None;
        }
        0x8000_001d
    } else {
        if max_leaf < 4 {
            return None;
        }
        4
    };
    for sub in 0..32 {
        let [eax, ebx, ecx, _] = m.cpuid(leaf, sub);
        let kind = eax & 0x1f;
        if kind == 0 {
            break;
        }
        let lvl = (eax >> 5) & 7;
        // Level 1 has separate data (1) and instruction (2) caches; above it, unified (3).
        let wanted = if level == 1 { kind == if instruction { 2 } else { 1 } } else { kind == 3 || kind == 1 };
        if lvl == level && wanted {
            // Each field is the count minus one; the masks keep them far below overflow.
            let ways = i64::from(ebx >> 22).saturating_add(1);
            let parts = i64::from((ebx >> 12) & 0x3ff).saturating_add(1);
            let line = i64::from(ebx & 0xfff).saturating_add(1);
            let sets = i64::from(ecx).saturating_add(1);
            return Some([ways.saturating_mul(parts).saturating_mul(line).saturating_mul(sets), ways, line]);
        }
    }
    None
}

/// A `_SC_LEVEL*` answer (§24.4, CPUID). glibc's conventions are kept: the level-1
/// instruction cache's associativity and level 4's associativity and line size are always -1,
/// an absent level is -1, except that on AMD and Hygon an absent level 4's size is 0 (D-24.7).
pub fn cache<M: Machine>(m: &M, c: Cache) -> i64 {
    if (c.level == 1 && c.instruction && c.field == 1) || (c.level == 4 && c.field != 0) {
        return -1;
    }
    let absent = if c.level == 4 && amd(m) { 0 } else { -1 };
    cache_leaf(m, c.level, c.instruction).and_then(|g| g.get(usize::try_from(c.field).ok()?).copied()).unwrap_or(absent)
}

/// `sysconf` for a computed name (D-24.1): `Ok(None)` is "no limit" (-1 without `errno`).
pub fn sysconf<S: Sys, M: Machine, A: Aux>(sys: &S, m: &M, aux: &A, q: Query) -> Result<Option<i64>> {
    const RLIMIT_STACK: u32 = 3;
    const RLIMIT_NPROC: u32 = 6;
    const RLIMIT_NOFILE: u32 = 7;
    const RLIMIT_SIGPENDING: u32 = 11;
    let page = || aux.aux(AT_PAGESZ).and_then(|p| i64::try_from(p).ok()).unwrap_or(4096);
    Ok(match q {
        Query::ArgMax => Some(match soft_limit(sys, RLIMIT_STACK) {
            Ok(l) => LEGACY_ARG_MAX.max(i64::try_from(l.cur / 4).unwrap_or(i64::MAX).min(KERNEL_ARG_MAX)),
            Err(_) => LEGACY_ARG_MAX,
        }),
        Query::ChildMax => limit(soft_limit(sys, RLIMIT_NPROC)?.cur),
        Query::ClkTck => Some(aux.aux(AT_CLKTCK).and_then(|v| i64::try_from(v).ok()).filter(|&v| v > 0).unwrap_or(100)),
        Query::NgroupsMax => Some(
            read_file(sys, b"/proc/sys/kernel/ngroups_max\0")
                .ok()
                .and_then(|t| decimal(t.as_slice().trim_ascii()))
                .unwrap_or(65536),
        ),
        Query::OpenMax => Some(match soft_limit(sys, RLIMIT_NOFILE) {
            Ok(l) => i64::try_from(l.cur).unwrap_or(i64::MAX).min(i64::from(i32::MAX)),
            Err(_) => 256,
        }),
        Query::PageSize => Some(page()),
        Query::SigqueueMax => limit(soft_limit(sys, RLIMIT_SIGPENDING)?.cur),
        Query::NprocsConf => Some(nprocs_conf(sys)),
        Query::NprocsOnln => Some(nprocs(sys)),
        Query::PhysPages => Some(pages(sys, page(), false)?),
        Query::AvphysPages => Some(pages(sys, page(), true)?),
        Query::Cache(c) => Some(cache(m, c)),
        Query::MinSigStkSz => Some(min_sigstksz(aux, m)),
        Query::SigStkSz => Some(sigstksz(min_sigstksz(aux, m))),
        Query::ThreadStackMin => Some(sigstksz(min_sigstksz(aux, m)).max(PTHREAD_STACK_MIN)),
    })
}

// ------------------------------------------------------------------------------ pathconf

/// File-system magic numbers `pathconf` distinguishes (`statfs.f_type`).
mod magic {
    pub(super) const EXT2: i64 = 0xef53;
    pub(super) const BTRFS: i64 = 0x9123_683e;
    pub(super) const MINIX: i64 = 0x137f;
    pub(super) const MINIX_2: i64 = 0x138f;
    pub(super) const MINIX2: i64 = 0x2468;
    pub(super) const MINIX2_2: i64 = 0x2478;
    pub(super) const XENIX: i64 = 0x012f_f7b4;
    pub(super) const SYSV4: i64 = 0x012f_f7b5;
    pub(super) const SYSV2: i64 = 0x012f_f7b6;
    pub(super) const COH: i64 = 0x012f_f7b7;
    pub(super) const UFS: i64 = 0x0001_1954;
    pub(super) const UFS_CIGAM: i64 = 0x5419_0100;
    pub(super) const REISERFS: i64 = 0x5265_4973;
    pub(super) const XFS: i64 = 0x5846_5342;
    pub(super) const LUSTRE: i64 = 0x0bd0_0bd0;
    pub(super) const SMB: i64 = 0x517b;
    pub(super) const NTFS: i64 = 0x5346_544e;
    pub(super) const UDF: i64 = 0x1501_3346;
    pub(super) const JFS: i64 = 0x3153_464a;
    pub(super) const VXFS: i64 = 0xa501_fcf5;
    pub(super) const CGROUP: i64 = 0x0027_e0eb;
    pub(super) const MSDOS: i64 = 0x4d44;
    pub(super) const ROMFS: i64 = 0x7275;
    pub(super) const ADFS: i64 = 0xadf5;
    pub(super) const BFS: i64 = 0x1bad_face;
    pub(super) const CRAMFS: i64 = 0x28cd_3d45;
    pub(super) const DEVPTS: i64 = 0x1cd1;
    pub(super) const EFS: i64 = 0x0041_4a53;
    pub(super) const EFS_2: i64 = 0x0007_2959;
    pub(super) const QNX4: i64 = 0x002f;
}

/// `_PC_LINK_MAX` by file system (glibc's table). The ext2/3/4 family shares a magic number;
/// `ext4` (65000, not 32000) is recognised by the mount's type in `/proc/self/mountinfo`.
pub fn link_max<S: Sys>(sys: &S, fs: &Statfs, dev: u64) -> i64 {
    use magic::*;
    match fs.ftype {
        EXT2 => {
            if mount_type_is(sys, dev, b"ext4") { 65000 } else { 32000 }
        }
        MINIX | MINIX_2 => 250,
        MINIX2 | MINIX2_2 => 65530,
        XENIX | SYSV4 | SYSV2 => 126,
        COH => 10000,
        UFS | UFS_CIGAM => 32000,
        REISERFS => 64535,
        XFS => 2_147_483_647,
        LUSTRE => 65000,
        _ => 127,
    }
}

/// `_PC_FILESIZEBITS` by file system (glibc's table).
pub fn filesize_bits(fs: &Statfs) -> i64 {
    use magic::*;
    match fs.ftype {
        BTRFS => 255,
        EXT2 | UFS | UFS_CIGAM | REISERFS | XFS | SMB | NTFS | UDF | JFS | VXFS | CGROUP | LUSTRE => 64,
        _ => 32,
    }
}

/// `_PC_2_SYMLINKS`: 0 for the file systems without symbolic links (glibc's table), else 1.
pub fn has_symlinks(fs: &Statfs) -> i64 {
    use magic::*;
    match fs.ftype {
        ADFS | BFS | CRAMFS | DEVPTS | EFS | EFS_2 | MSDOS | NTFS | QNX4 | ROMFS => 0,
        _ => 1,
    }
}

/// Whether the mount holding device `dev` has file-system type `ty` (`/proc/self/mountinfo`,
/// D-13.8).
fn mount_type_is<S: Sys>(sys: &S, dev: u64, ty: &[u8]) -> bool {
    let Ok(text) = read_file(sys, b"/proc/self/mountinfo\0") else { return false };
    let (major, minor) = (crate::fs::major(dev), crate::fs::minor(dev));
    config::lines(text.as_slice()).any(|line| {
        let id = line.split(|&b| b == b' ').nth(2).unwrap_or(&[]);
        let mut mm = id.splitn(2, |&b| b == b':');
        let same = mm.next().and_then(decimal) == Some(i64::from(major)) && mm.next().and_then(decimal) == Some(i64::from(minor));
        // After the optional fields, " - " precedes the type.
        let after = line.windows(3).position(|w| w == b" - ").and_then(|p| line.get(p.saturating_add(3)..)).unwrap_or(&[]);
        same && after.split(|&b| b == b' ').next() == Some(ty)
    })
}
