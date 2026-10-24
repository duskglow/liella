//! `CSys`: the production implementation of `liella_kernel::{Sys, Machine}`. Safe code.
//!
//! Each method converts typed arguments to the C layer's register arguments, calls one
//! class-*sys* wrapper, and decodes the raw result (`Errno::decode`, rule X3). Pointers are
//! derived only from the references and slices the method received, so the kernel only
//! touches memory the caller lent for that purpose. The few checks here exist for
//! soundness, not policy (decision D-X.9): `MAP_FIXED` and `SHM_REMAP` are refused, lengths
//! handed to the kernel never exceed the Rust buffer, and a `Region`'s bookkeeping follows
//! each successful change.
//!
//! Clocks, `gettimeofday`, `time` and `getcpu` go through the vDSO when the kernel provides it
//! (rule P3); the symbols are resolved once, lazily.

#![allow(clippy::as_conversions)] // register-width reinterpretation is the kernel ABI (⚑ KERN)

use core::convert::Infallible;
use core::ffi::c_void;
use core::ptr;
use core::sync::atomic::{AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering};

use liella_kernel::errno::{Errno, KResult};
use liella_kernel::fd::Fd;
use liella_kernel::machine::{Machine, X87Env, MXCSR_DEFINED};
use liella_kernel::mem::{IoVec, IoVecMut, KPtrArray, MMsgHdr, MMsgHdrMut, MsgHdr, MsgHdrMut, Region, RegionKind, RemoteIoVec, PAGE_SIZE};
use liella_kernel::ops::*;
use liella_kernel::path::KStr;
use liella_kernel::sys::{Disposition, FsValue, FutexArg, Sys};
use liella_kernel::uapi::*;
use liella_kernel::{auxv, vdso};

use crate::conv;
use crate::ffi;
use crate::ffi_sys::*;
use crate::nr;

/// The kernel, reached through the C syscall layer. A zero-sized value. With `CP` the blocking
/// calls are cancellation points (C0.2, D-22.11): they run inside the C layer's cancellation
/// window. Only the exports that are cancellation points use that form ([`CP`]); every other
/// use, the library's own reads included, cannot be cancelled ([`CSys`]).
#[derive(Clone, Copy, Default, Debug)]
pub struct CSysImpl<const CP: bool>;

/// The kernel (no call is a cancellation point).
pub type CSys = CSysImpl<false>;

/// The kernel value.
#[allow(non_upper_case_globals)]
pub const CSys: CSys = CSysImpl;

/// The kernel for the exports that are cancellation points.
pub type Cp = CSysImpl<true>;

/// The cancellation-point kernel value.
pub const CP: Cp = CSysImpl;

/// A system-call argument as a register.
trait Arg {
    fn raw(self) -> i64;
}
impl Arg for i64 {
    fn raw(self) -> i64 {
        self
    }
}
impl Arg for *const c_void {
    fn raw(self) -> i64 {
        self.expose_provenance() as i64
    }
}
impl Arg for *mut c_void {
    fn raw(self) -> i64 {
        self.expose_provenance() as i64
    }
}

/// System call `nr`: inside the cancellation window when `CP` and the calling thread has a
/// descriptor (D-22.11), the plain path otherwise.
fn call<const CP: bool>(nr: i64, a: [i64; 6]) -> i64 {
    let [a0, a1, a2, a3, a4, a5] = a;
    if CP {
        if let Some(d) = crate::thread::current() {
            let r = ffi::liella_machine_syscall_cancel(&d.cancel, nr, a0, a1, a2, a3, a4, a5);
            // -ECANCELED (125) from the window, or an interruption, while a request is due: the
            // export acts once its result is formed (`crate::cancel::point`).
            if found_due(&d.cancel, r) {
                d.cancel.fetch_or(liella_rt::thread::CANCEL_HIT, core::sync::atomic::Ordering::AcqRel);
            }
            return r;
        }
    }
    liella_sys_syscall(nr, a0, a1, a2, a3, a4, a5)
}

/// CANCEL-FAST (D-22.11): system call `nr` with at most four arguments as a cancellation point,
/// acting on a due request itself (no `CANCEL_HIT` round trip through [`crate::cancel::point`]).
/// The window's first instruction is the one load that decides "nothing can be acted on", in a
/// single-threaded process and in every thread alike; what follows it is unchanged: -ECANCELED
/// from the window, or an interruption while a request is due, sets `errno` from the result and
/// acts, exactly as `call::<true>` followed by `point` did.
#[inline]
fn point4(nr: i64, a0: i64, a1: i64, a2: i64, a3: i64) -> i64 {
    let Some(d) = crate::thread::current() else { return liella_sys_syscall(nr, a0, a1, a2, a3, 0, 0) };
    let r = ffi::liella_machine_syscall_cancel4(&d.cancel, nr, a0, a1, a2, a3);
    if found_due(&d.cancel, r) {
        crate::cancel::act(r);
    }
    r
}

/// D-22.11 (CANCEL-FAST-2): a point's raw result `r` acts on a request only when the word says it
/// is due. -ECANCELED from the window implies that (the window returns it only for a due request,
/// and only the thread itself can make a due request not due); a kernel's own ECANCELED (a
/// timerfd read after a clock change, a seccomp filter) with no request due is an ordinary error.
#[inline]
pub(crate) fn found_due(word: &core::sync::atomic::AtomicU32, r: i64) -> bool {
    (r == -125 || r == -4) && liella_rt::thread::cancel_due(word.load(Ordering::Acquire))
}

/// The direct entries' error path (machine/points.S, CANCEL-FAST-2): a due request is acted on,
/// with errno from `r`, for an error result `r` that [`found_due`] accepts, or for any result of
/// `close` (`closed`: the descriptor is closed regardless, §22.3); otherwise `r` is returned.
pub(crate) fn point_result(r: i64, closed: bool) -> i64 {
    if let Some(d) = crate::thread::current() {
        let due = if closed { liella_rt::thread::cancel_due(d.cancel.load(Ordering::Acquire)) } else { found_due(&d.cancel, r) };
        if due {
            crate::cancel::act(r);
        }
    }
    r
}

/// `read` through its direct entry (machine/points.S): the C result, errno set on failure.
pub(crate) fn direct_read(fd: i32, buf: *mut c_void, n: usize) -> isize {
    ffi::liella_machine_read(fd, buf, n)
}

/// `write` through its direct entry (machine/points.S).
pub(crate) fn direct_write(fd: i32, buf: *const c_void, n: usize) -> isize {
    ffi::liella_machine_write(fd, buf, n)
}

/// `pread` through its direct entry (machine/points.S).
pub(crate) fn direct_pread(fd: i32, buf: *mut c_void, n: usize, off: i64) -> isize {
    ffi::liella_machine_pread(fd, buf, n, off)
}

/// `pwrite` through its direct entry (machine/points.S).
pub(crate) fn direct_pwrite(fd: i32, buf: *const c_void, n: usize, off: i64) -> isize {
    ffi::liella_machine_pwrite(fd, buf, n, off)
}

/// `close` through its direct entry (machine/points.S).
pub(crate) fn direct_close(fd: i32) -> i32 {
    ffi::liella_machine_close(fd)
}

/// Mark asynchronous I/O as started (before its first request or internal descriptor): from
/// then on the direct `close` takes the Rust path (`crate::aio::closing`).
pub(crate) fn mark_aio_used() {
    if !ffi::liella_machine_aio_used.load(Ordering::Relaxed) {
        ffi::liella_machine_aio_used.store(true, Ordering::Release);
    }
}

/// `read(2)` as a cancellation point (CANCEL-FAST): the raw result (`ret::raw_ssize`).
pub(crate) fn read_point(fd: Fd, buf: &mut [u8]) -> i64 {
    point4(nr::READ, f(fd), Arg::raw(bm(buf)), z(buf.len()), 0)
}

/// `write(2)` as a cancellation point (CANCEL-FAST): the raw result (`ret::raw_ssize`).
pub(crate) fn write_point(fd: Fd, buf: &[u8]) -> i64 {
    point4(nr::WRITE, f(fd), Arg::raw(b(buf)), z(buf.len()), 0)
}

/// `pread64(2)` as a cancellation point (CANCEL-FAST): the raw result (`ret::raw_ssize`).
pub(crate) fn pread_point(fd: Fd, buf: &mut [u8], offset: i64) -> i64 {
    point4(nr::PREAD64, f(fd), Arg::raw(bm(buf)), z(buf.len()), offset)
}

/// `pwrite64(2)` as a cancellation point (CANCEL-FAST): the raw result (`ret::raw_ssize`).
pub(crate) fn pwrite_point(fd: Fd, buf: &[u8], offset: i64) -> i64 {
    point4(nr::PWRITE64, f(fd), Arg::raw(b(buf)), z(buf.len()), offset)
}

/// `close(2)` as a cancellation point (CANCEL-FAST): the descriptor is closed regardless, outside
/// the window (§22.3, ⚑ KERN), then a due request is acted on with `errno` set from the result,
/// as `CP.close` followed by `point` did. The raw result (`ret::raw_zero`).
pub(crate) fn close_point(fd: Fd) -> i64 {
    let r = liella_sys_close(f(fd));
    if let Some(d) = crate::thread::current() {
        if liella_rt::thread::cancel_due(d.cancel.load(Ordering::Acquire)) {
            crate::cancel::act(r);
        }
    }
    r
}

/// A blocking system call by number, a cancellation point in the `CP` kernel.
macro_rules! sc {
    ($nr:expr; $($arg:expr),* $(,)?) => {{
        let mut args = [0i64; 6];
        for (slot, v) in args.iter_mut().zip([$(Arg::raw($arg)),*]) {
            *slot = v;
        }
        call::<CP>($nr, args)
    }};
}

// ---------------------------------------------------------------------------- conversions

fn r(raw: i64) -> KResult<u64> {
    Errno::decode(raw)
}
fn unit(raw: i64) -> KResult<()> {
    r(raw).map(|_| ())
}
fn int(raw: i64) -> KResult<i32> {
    r(raw).map(|v| v as i32)
}
fn size(raw: i64) -> KResult<usize> {
    r(raw).map(|v| v as usize)
}
fn fdr(raw: i64) -> KResult<Fd> {
    r(raw).map(|v| Fd::new(v as i32))
}
fn f(fd: Fd) -> i64 {
    i64::from(fd.raw())
}
fn u(v: u32) -> i64 {
    i64::from(v)
}
fn z(v: usize) -> i64 {
    v as i64
}
fn c<T>(v: &T) -> *const c_void {
    ptr::from_ref(v).cast()
}
fn m<T>(v: &mut T) -> *mut c_void {
    ptr::from_mut(v).cast()
}
fn oc<T>(v: Option<&T>) -> *const c_void {
    v.map_or(ptr::null(), c)
}
fn om<T>(v: Option<&mut T>) -> *mut c_void {
    v.map_or(ptr::null_mut(), m)
}
fn s(k: KStr<'_>) -> *const c_void {
    k.as_ptr().cast()
}
fn os(k: Option<KStr<'_>>) -> *const c_void {
    k.map_or(ptr::null(), s)
}
fn b(v: &[u8]) -> *const c_void {
    v.as_ptr().cast()
}
fn bm(v: &mut [u8]) -> *mut c_void {
    v.as_mut_ptr().cast()
}
/// An exposed address as an integer argument (for calls whose C prototype is `long` but whose
/// kernel meaning is a pointer: `ioctl`, `fcntl`, `prctl`, `ptrace`, `arch_prctl`, `semctl`).
fn xm<T>(v: &mut T) -> i64 {
    ptr::from_mut(v).expose_provenance() as i64
}
fn xc<T>(v: &T) -> i64 {
    ptr::from_ref(v).expose_provenance() as i64
}
fn addr_ptr(a: usize) -> *const c_void {
    ptr::with_exposed_provenance(a)
}
fn page_round(len: usize) -> KResult<usize> {
    len.checked_add(PAGE_SIZE - 1).map(|v| v & !(PAGE_SIZE - 1)).ok_or(Errno::ENOMEM)
}

const PROT_VALID: u32 = PROT_READ | PROT_WRITE | PROT_EXEC | 0x8; // 0x8: PROT_SEM
const SIGSET_SIZE: i64 = 8;
const SHM_EXEC: u32 = 0o100000;

// ---------------------------------------------------------------------------- the vDSO

const V_CLOCK_GETTIME: usize = 0;
const V_CLOCK_GETRES: usize = 1;
const V_GETTIMEOFDAY: usize = 2;
const V_TIME: usize = 3;
const V_GETCPU: usize = 4;
/// In [`crate::startup_names`] (STARTUP-READ-CONSTANTS): the first heap slow path resolves them.
const VDSO_NAMES: [&[u8]; 5] = {
    use crate::startup_names as n;
    [n::VDSO_CLOCK_GETTIME, n::VDSO_CLOCK_GETRES, n::VDSO_GETTIMEOFDAY, n::VDSO_TIME, n::VDSO_GETCPU]
};

/// `AT_SYSINFO_EHDR`: `usize::MAX` until known, 0 if the kernel provided no vDSO.
static SYSINFO_EHDR: AtomicUsize = AtomicUsize::new(usize::MAX);
static VDSO_STATE: AtomicU8 = AtomicU8::new(0); // 0 unresolved, 1 resolved
static VDSO_FNS: [AtomicUsize; 5] = [const { AtomicUsize::new(0) }; 5];

/// Record the vDSO base from the process's auxiliary vector (the process entry,
/// `runtime::start`). A process the entry did not start reads `/proc/self/auxv` on first use.
/// Only the kernel's own value may be passed here: it becomes a slice (`conv::kernel_image`).
pub(crate) fn set_sysinfo_ehdr(base: usize) {
    SYSINFO_EHDR.store(base, Ordering::Release);
}

fn sysinfo_ehdr() -> usize {
    let known = SYSINFO_EHDR.load(Ordering::Acquire);
    if known != usize::MAX {
        return known;
    }
    let base = auxv_from_proc(AT_SYSINFO_EHDR).unwrap_or(0) as usize;
    SYSINFO_EHDR.store(base, Ordering::Release);
    base
}

/// One auxiliary-vector value read from `/proc/self/auxv` (hosted processes, and processes
/// started before the entry code records the vector).
pub(crate) fn auxv_from_proc(key: u64) -> Option<u64> {
    // The vector never changes during the process's life: it is read once. Concurrent first
    // readers store the same words, so the race is benign; the length is published last.
    static WORDS: [AtomicU64; 512] = [const { AtomicU64::new(0) }; 512];
    static LEN: AtomicUsize = AtomicUsize::new(0);
    if LEN.load(Ordering::Acquire) == 0 {
        let mut words = [0u64; 512];
        let n = auxv_read_proc(&mut words)?;
        for (a, w) in WORDS.iter().zip(&words).take(n) {
            a.store(*w, Ordering::Relaxed);
        }
        LEN.store(n, Ordering::Release);
    }
    // The (key, value) pairs in place, up to AT_NULL.
    let n = LEN.load(Ordering::Acquire);
    WORDS
        .get(..n)?
        .chunks_exact(2)
        .filter_map(|p| Some((p.first()?.load(Ordering::Relaxed), p.get(1)?.load(Ordering::Relaxed))))
        .take_while(|&(k, _)| k != 0)
        .find(|&(k, _)| k == key)
        .map(|(_, v)| v)
}

/// `/proc/self/auxv` as words into `words`; how many.
fn auxv_read_proc(words: &mut [u64; 512]) -> Option<usize> {
    let sys = CSys;
    let path = KStr::from_nul_terminated(b"/proc/self/auxv\0").ok()?;
    let fd = sys.openat(Fd::CWD, path, O_RDONLY | O_CLOEXEC, 0).ok()?;
    let mut bytes = [0u8; 4096];
    let mut len = 0usize;
    while let Some(rest) = bytes.get_mut(len..) {
        match sys.read(fd, rest) {
            Ok(0) | Err(_) => break,
            Ok(n) => len = len.saturating_add(n),
        }
    }
    let _ = sys.close(fd);
    let n = auxv::words_from_bytes(bytes.get(..len)?, words);
    (n != 0).then_some(n)
}

/// Resolve the vDSO functions from the image at `base` (0 = no vDSO: nothing resolves).
fn resolve_vdso(base: usize) -> [usize; 5] {
    let mut out = [0usize; 5];
    if base == 0 {
        return out;
    }
    let header = conv::kernel_image(base, vdso::HEADER_LEN);
    let Some(len) = vdso::image_len(header) else { return out };
    let Some(image) = vdso::Vdso::parse(conv::kernel_image(base, len)) else { return out };
    // One walk over the symbols for all five (SETLOCALE-2).
    for (slot, off) in out.iter_mut().zip(image.lookup_many(&VDSO_NAMES, crate::startup_names::VDSO_VERSION)) {
        *slot = off.map_or(0, |off| base.wrapping_add(off));
    }
    out
}

fn vdso_fn(which: usize) -> Option<usize> {
    if VDSO_STATE.load(Ordering::Acquire) == 0 {
        let fns = resolve_vdso(sysinfo_ehdr());
        for (slot, v) in VDSO_FNS.iter().zip(fns) {
            slot.store(v, Ordering::Relaxed);
        }
        VDSO_STATE.store(1, Ordering::Release);
    }
    VDSO_FNS.get(which).map(|a| a.load(Ordering::Relaxed)).filter(|&a| a != 0)
}

impl<const CP: bool> CSysImpl<CP> {
    /// True if the kernel provides a vDSO `clock_gettime` (spec Appendix A.2 vDSO).
    pub fn vdso_clock_gettime_available(&self) -> bool {
        vdso_fn(V_CLOCK_GETTIME).is_some()
    }

    /// `clock_gettime` through the system call, bypassing the vDSO (for tests of the call).
    pub fn clock_gettime_syscall(&self, clock: i32) -> KResult<Timespec> {
        let mut ts = Timespec::default();
        unit(liella_sys_clock_gettime(i64::from(clock), m(&mut ts))).map(|()| ts)
    }

    /// `gettimeofday` through the system call.
    pub fn gettimeofday_syscall(&self) -> KResult<(Timeval, Timezone)> {
        let (mut tv, mut tz) = (Timeval::default(), Timezone::default());
        unit(liella_sys_gettimeofday(m(&mut tv), m(&mut tz))).map(|()| (tv, tz))
    }

    /// `time` through the system call.
    pub fn time_syscall(&self) -> KResult<i64> {
        r(liella_sys_time(ptr::null_mut())).map(|v| v as i64)
    }

    /// `getcpu` through the system call.
    pub fn getcpu_syscall(&self) -> KResult<(u32, u32)> {
        let (mut cpu, mut node) = (0u32, 0u32);
        unit(liella_sys_getcpu(m(&mut cpu), m(&mut node), ptr::null_mut())).map(|()| (cpu, node))
    }

    /// `clock_getres` through the system call.
    pub fn clock_getres_syscall(&self, clock: i32) -> KResult<Timespec> {
        let mut ts = Timespec::default();
        unit(liella_sys_clock_getres(i64::from(clock), m(&mut ts))).map(|()| ts)
    }

    /// `fstatat` with the kernel writing the result straight into `dst` (the caller's object):
    /// no zeroed temporary and no two 144-byte copies (WL-HOT-DU-2: they were 290 of the export's
    /// ~310 user instructions per call). `dst` is written only on success (the kernel copies the
    /// result out as its last step).
    pub fn fstatat_into(&self, dir: Fd, path: KStr<'_>, flags: u32, dst: &mut Stat) -> KResult<()> {
        unit(liella_sys_newfstatat(f(dir), s(path), m(dst), u(flags)))
    }

    /// `fstat` into the caller's object (see [`fstatat_into`](Self::fstatat_into)).
    pub fn fstat_into(&self, fd: Fd, dst: &mut Stat) -> KResult<()> {
        unit(liella_sys_fstat(f(fd), m(dst)))
    }

    /// `statx` into the caller's object (see [`fstatat_into`](Self::fstatat_into); 256 bytes).
    pub fn statx_into(&self, dir: Fd, path: KStr<'_>, flags: u32, mask: u32, dst: &mut Statx) -> KResult<()> {
        match unit(liella_sys_statx(f(dir), s(path), u(flags), u(mask), m(dst))) {
            Err(Errno::ENOSYS) => self.statx_from_fstatat(dir, path, flags, dst),
            other => other,
        }
    }

    /// `statx` emulated with `newfstatat` for a kernel (or seccomp policy) that answers the call
    /// with `ENOSYS`: only reached after that answer, so the normal path pays nothing. As the
    /// reference does, `mask` is ignored, only the flags `fstatat` can honour (`AT_SYMLINK_NOFOLLOW`,
    /// `AT_NO_AUTOMOUNT`, `AT_EMPTY_PATH`) are accepted (any other bit is `EINVAL`), and the result
    /// carries exactly the `stat` fields (`stx_mask` = `STATX_BASIC_STATS` minus `STATX_BTIME`
    /// minus `STATX_MNT_ID`, i.e. 0x7ff); the rest stays zero.
    #[cold]
    fn statx_from_fstatat(&self, dir: Fd, path: KStr<'_>, flags: u32, dst: &mut Statx) -> KResult<()> {
        const KEPT: u32 = 0x100 | 0x800 | 0x1000;
        if flags & !KEPT != 0 {
            return Err(Errno::EINVAL);
        }
        let mut st = Stat::default();
        self.fstatat_into(dir, path, flags, &mut st)?;
        *dst = statx_of_stat(&st);
        Ok(())
    }
}

/// The `statx` shape of a `stat` result (the `ENOSYS` emulation); `stx_mask` names exactly the
/// fields filled.
fn statx_of_stat(st: &Stat) -> Statx {
    const STATX_BASIC_NO_BTIME_MNT: u32 = 0x7ff;
    // The kernel's `new_decode_dev`: major in bits 8..20 and 32.., minor in bits 0..8 and 12..
    fn major(d: u64) -> u32 {
        u32::try_from(((d >> 8) & 0xfff) | ((d >> 32) & !0xfff)).unwrap_or(0)
    }
    fn minor(d: u64) -> u32 {
        u32::try_from((d & 0xff) | ((d >> 12) & !0xff)).unwrap_or(0)
    }
    fn ts(t: Timespec) -> StatxTimestamp {
        let mut x = StatxTimestamp::default();
        x.sec = t.sec;
        x.nsec = u32::try_from(t.nsec).unwrap_or(0);
        x
    }
    let mut x = Statx::default();
    x.mask = STATX_BASIC_NO_BTIME_MNT;
    x.blksize = u32::try_from(st.blksize).unwrap_or(0);
    x.nlink = u32::try_from(st.nlink).unwrap_or(u32::MAX);
    x.uid = st.uid;
    x.gid = st.gid;
    x.mode = u16::try_from(st.mode & 0xffff).unwrap_or(0);
    x.ino = st.ino;
    x.size = u64::try_from(st.size).unwrap_or(0);
    x.blocks = u64::try_from(st.blocks).unwrap_or(0);
    x.atime = ts(st.atime);
    x.ctime = ts(st.ctime);
    x.mtime = ts(st.mtime);
    x.rdev_major = major(st.rdev);
    x.rdev_minor = minor(st.rdev);
    x.dev_major = major(st.dev);
    x.dev_minor = minor(st.dev);
    x
}

// ---------------------------------------------------------------------------- Sys

/// Whether `rdtsc` may be executed (D-7.14): read once per process, at the first gate use, from
/// `prctl(PR_GET_TSC)`. A process that set `PR_TSC_SIGSEGV` (replay tools, sandboxes) would take a
/// SIGSEGV from the instruction; it, and a failed query, keep the clock read. fork inherits the
/// setting and the cache; exec starts a new image.
// ARCH(x86_64): rdtsc and its PR_GET_TSC gate (D-7.14); aarch64 reads CNTVCT_EL0 (A-6)
#[cfg(target_arch = "x86_64")]
static TSC_STATE: AtomicU8 = AtomicU8::new(TSC_UNKNOWN);
#[cfg(target_arch = "x86_64")]
const TSC_UNKNOWN: u8 = 0;
// ARCH(x86_64): rdtsc and its PR_GET_TSC gate (D-7.14); aarch64 reads CNTVCT_EL0 (A-6)
#[cfg(target_arch = "x86_64")]
const TSC_OK: u8 = 1;
#[cfg(target_arch = "x86_64")]
const TSC_DISABLED: u8 = 2;
/// `PR_TSC_ENABLE`.
// ARCH(x86_64): rdtsc and its PR_GET_TSC gate (D-7.14); aarch64 reads CNTVCT_EL0 (A-6)
#[cfg(target_arch = "x86_64")]
const PR_TSC_ENABLE: i32 = 1;

impl<const CP: bool> CSysImpl<CP> {
    /// The first use of the counter: ask the kernel whether it may be read, remember the answer.
    // ARCH(x86_64): rdtsc and its PR_GET_TSC gate (D-7.14); aarch64 reads CNTVCT_EL0 (A-6)
    #[cfg(target_arch = "x86_64")]
    #[cold]
    fn tsc_probe(&self) -> u64 {
        let ok = matches!(self.prctl_get_int(PrctlIntOut::GetTsc), Ok(PR_TSC_ENABLE));
        TSC_STATE.store(if ok { TSC_OK } else { TSC_DISABLED }, Ordering::Relaxed);
        if ok { ffi::liella_machine_rdtsc() } else { 0 }
    }
}

impl<const CP: bool> Sys for CSysImpl<CP> {
    // ------------------------------------------------------------ process lifecycle
    fn execve(&self, path: KStr<'_>, argv: &KPtrArray<'_>, envp: &KPtrArray<'_>) -> KResult<Infallible> {
        Err(r(liella_sys_execve(s(path), addr_ptr(argv.addr()), addr_ptr(envp.addr()))).err().unwrap_or(Errno::EINVAL))
    }
    fn execveat(&self, dir: Fd, path: KStr<'_>, argv: &KPtrArray<'_>, envp: &KPtrArray<'_>, flags: u32) -> KResult<Infallible> {
        Err(r(liella_sys_execveat(f(dir), s(path), addr_ptr(argv.addr()), addr_ptr(envp.addr()), u(flags))).err().unwrap_or(Errno::EINVAL))
    }
    fn wait4(&self, pid: i32, options: u32, usage: Option<&mut Rusage>) -> KResult<(i32, i32)> {
        let mut status = 0i32;
        int(sc!(nr::WAIT4; i64::from(pid), m(&mut status), u(options), om(usage))).map(|p| (p, status))
    }
    fn waitid(&self, idtype: u32, id: i32, info: &mut SigInfo, options: u32, usage: Option<&mut Rusage>) -> KResult<()> {
        unit(sc!(nr::WAITID; u(idtype), i64::from(id), m(info), u(options), om(usage)))
    }
    fn pidfd_open(&self, pid: i32, flags: u32) -> KResult<Fd> {
        fdr(liella_sys_pidfd_open(i64::from(pid), u(flags)))
    }
    fn pidfd_getfd(&self, pidfd: Fd, target: i32, flags: u32) -> KResult<Fd> {
        fdr(liella_sys_pidfd_getfd(f(pidfd), i64::from(target), u(flags)))
    }
    fn pidfd_send_signal(&self, pidfd: Fd, sig: i32, info: Option<&SigInfo>, flags: u32) -> KResult<()> {
        unit(liella_sys_pidfd_send_signal(f(pidfd), i64::from(sig), oc(info), u(flags)))
    }
    fn set_tid_address(&self, word: &'static AtomicU32) -> KResult<i32> {
        int(liella_sys_set_tid_address(word.as_ptr().cast()))
    }

    // ------------------------------------------------------------ credentials
    fn getresuid(&self) -> KResult<(u32, u32, u32)> {
        let (mut a, mut b2, mut c2) = (0u32, 0u32, 0u32);
        unit(liella_sys_getresuid(m(&mut a), m(&mut b2), m(&mut c2))).map(|()| (a, b2, c2))
    }
    fn getresgid(&self) -> KResult<(u32, u32, u32)> {
        let (mut a, mut b2, mut c2) = (0u32, 0u32, 0u32);
        unit(liella_sys_getresgid(m(&mut a), m(&mut b2), m(&mut c2))).map(|()| (a, b2, c2))
    }
    fn setuid(&self, uid: u32) -> KResult<()> {
        unit(liella_sys_setuid(u(uid)))
    }
    fn setgid(&self, gid: u32) -> KResult<()> {
        unit(liella_sys_setgid(u(gid)))
    }
    fn setreuid(&self, ruid: u32, euid: u32) -> KResult<()> {
        unit(liella_sys_setreuid(u(ruid), u(euid)))
    }
    fn setregid(&self, rgid: u32, egid: u32) -> KResult<()> {
        unit(liella_sys_setregid(u(rgid), u(egid)))
    }
    fn setresuid(&self, r0: u32, e: u32, s0: u32) -> KResult<()> {
        unit(liella_sys_setresuid(u(r0), u(e), u(s0)))
    }
    fn setresgid(&self, r0: u32, e: u32, s0: u32) -> KResult<()> {
        unit(liella_sys_setresgid(u(r0), u(e), u(s0)))
    }
    fn getgroups(&self, list: &mut [u32]) -> KResult<usize> {
        let n = i32::try_from(list.len()).unwrap_or(i32::MAX);
        size(liella_sys_getgroups(i64::from(n), list.as_mut_ptr().cast()))
    }
    fn setgroups(&self, list: &[u32]) -> KResult<()> {
        let n = i32::try_from(list.len()).map_err(|_| Errno::EINVAL)?;
        unit(liella_sys_setgroups(i64::from(n), list.as_ptr().cast()))
    }
    fn capget(&self, hdr: &mut CapHeader, data: Option<&mut [CapData; 2]>) -> KResult<()> {
        unit(liella_sys_capget(m(hdr), om(data)))
    }
    fn capset(&self, hdr: &mut CapHeader, data: &[CapData; 2]) -> KResult<()> {
        unit(liella_sys_capset(m(hdr), c(data)))
    }

    // ------------------------------------------------------------ sessions
    fn setsid(&self) -> KResult<i32> {
        int(liella_sys_setsid())
    }
    fn getsid(&self, pid: i32) -> KResult<i32> {
        int(liella_sys_getsid(i64::from(pid)))
    }
    fn setpgid(&self, pid: i32, pgid: i32) -> KResult<()> {
        unit(liella_sys_setpgid(i64::from(pid), i64::from(pgid)))
    }
    fn getpgid(&self, pid: i32) -> KResult<i32> {
        int(liella_sys_getpgid(i64::from(pid)))
    }

    // ------------------------------------------------------------ signals
    fn sigaction_get(&self, sig: i32) -> KResult<KSigAction> {
        let mut old = KSigAction::default();
        unit(liella_sys_rt_sigaction(i64::from(sig), ptr::null(), m(&mut old), SIGSET_SIZE)).map(|()| old)
    }
    fn sigaction_set(&self, sig: i32, disposition: Disposition, flags: u64, mask: SigSet) -> KResult<KSigAction> {
        let handler = match disposition {
            Disposition::Default => SIG_DFL,
            Disposition::Ignore => SIG_IGN,
        };
        let new = KSigAction { handler, flags: flags & !SA_RESTORER, restorer: 0, mask };
        let mut old = KSigAction::default();
        unit(liella_sys_rt_sigaction(i64::from(sig), c(&new), m(&mut old), SIGSET_SIZE)).map(|()| old)
    }
    fn sigprocmask(&self, how: u32, set: Option<&SigSet>) -> KResult<SigSet> {
        let mut old = SigSet::empty();
        unit(liella_sys_rt_sigprocmask(u(how), oc(set), m(&mut old), SIGSET_SIZE)).map(|()| old)
    }
    fn sigsuspend(&self, mask: &SigSet) -> KResult<()> {
        unit(sc!(nr::RT_SIGSUSPEND; c(mask), SIGSET_SIZE))
    }
    fn sigpending(&self) -> KResult<SigSet> {
        let mut set = SigSet::empty();
        unit(liella_sys_rt_sigpending(m(&mut set), SIGSET_SIZE)).map(|()| set)
    }
    fn sigtimedwait(&self, set: &SigSet, info: Option<&mut SigInfo>, timeout: Option<&Timespec>) -> KResult<i32> {
        int(sc!(nr::RT_SIGTIMEDWAIT; c(set), om(info), oc(timeout), SIGSET_SIZE))
    }
    fn sigqueueinfo(&self, pid: i32, sig: i32, info: &SigInfo) -> KResult<()> {
        unit(liella_sys_rt_sigqueueinfo(i64::from(pid), i64::from(sig), c(info)))
    }
    fn tgsigqueueinfo(&self, tgid: i32, tid: i32, sig: i32, info: &SigInfo) -> KResult<()> {
        unit(liella_sys_rt_tgsigqueueinfo(i64::from(tgid), i64::from(tid), i64::from(sig), c(info)))
    }
    fn kill(&self, pid: i32, sig: i32) -> KResult<()> {
        unit(liella_sys_kill(i64::from(pid), i64::from(sig)))
    }
    fn tgkill(&self, tgid: i32, tid: i32, sig: i32) -> KResult<()> {
        unit(liella_sys_tgkill(i64::from(tgid), i64::from(tid), i64::from(sig)))
    }
    fn sigaltstack_get(&self) -> KResult<StackT> {
        let mut old = StackT::default();
        unit(liella_sys_sigaltstack(ptr::null(), m(&mut old))).map(|()| old)
    }
    fn sigaltstack_disable(&self) -> KResult<StackT> {
        let mut new = StackT::default();
        new.flags = SS_DISABLE;
        let mut old = StackT::default();
        unit(liella_sys_sigaltstack(c(&new), m(&mut old))).map(|()| old)
    }
    fn signalfd(&self, fd: Option<Fd>, mask: &SigSet, flags: u32) -> KResult<Fd> {
        fdr(liella_sys_signalfd4(fd.map_or(-1, f), c(mask), SIGSET_SIZE, u(flags)))
    }
    fn pause(&self) -> KResult<()> {
        unit(sc!(nr::PAUSE; ))
    }

    // ------------------------------------------------------------ memory
    fn mmap(&self, hint: usize, len: usize, prot: u32, flags: u32, fd: Option<Fd>, offset: i64) -> KResult<Region> {
        if flags & MAP_FIXED != 0 || prot & !PROT_VALID != 0 {
            return Err(Errno::EINVAL);
        }
        let anon = fd.is_none() || flags & MAP_ANONYMOUS != 0;
        let flags = if anon { flags | MAP_ANONYMOUS } else { flags };
        let fdv = if anon { -1 } else { fd.map_or(-1, f) };
        let rounded = page_round(len)?;
        let addr = r(liella_sys_mmap(z(hint), z(len), u(prot), u(flags), fdv, offset))? as usize;
        let shared = flags & 0x3 != MAP_PRIVATE;
        let kind = if flags & (MAP_GROWSDOWN | MAP_HUGETLB) != 0 {
            RegionKind::Special
        } else if !anon {
            RegionKind::File
        } else if shared {
            RegionKind::AnonShared
        } else {
            RegionKind::AnonPrivate
        };
        Ok(Region::__from_kernel(addr, rounded, prot, kind))
    }
    fn munmap(&self, region: Region) -> KResult<()> {
        unit(liella_sys_munmap(z(region.addr()), z(region.len())))
    }
    fn mprotect(&self, region: &mut Region, prot: u32) -> KResult<()> {
        if prot & !PROT_VALID != 0 {
            return Err(Errno::EINVAL);
        }
        unit(liella_sys_mprotect(z(region.addr()), z(region.len()), u(prot)))?;
        let (a, l, k) = (region.addr(), region.len(), region.pkey());
        region.__kernel_update(a, l, prot, k);
        Ok(())
    }
    fn mremap(&self, region: &mut Region, new_len: usize, may_move: bool) -> KResult<()> {
        let rounded = page_round(new_len)?;
        let flags = if may_move { MREMAP_MAYMOVE } else { 0 };
        let addr = r(liella_sys_mremap(z(region.addr()), z(region.len()), z(new_len), u(flags), 0))? as usize;
        let (p, k) = (region.prot(), region.pkey());
        region.__kernel_update(addr, rounded, p, k);
        Ok(())
    }
    fn madvise(&self, region: &mut Region, advice: Advice) -> KResult<()> {
        unit(liella_sys_madvise(z(region.addr()), z(region.len()), u(advice.code())))
    }
    fn msync(&self, region: &Region, flags: u32) -> KResult<()> {
        unit(liella_sys_msync(z(region.addr()), z(region.len()), u(flags)))
    }
    fn mincore(&self, region: &Region, vec: &mut [u8]) -> KResult<()> {
        if vec.len() < region.len().div_ceil(PAGE_SIZE) {
            return Err(Errno::EINVAL);
        }
        unit(liella_sys_mincore(z(region.addr()), z(region.len()), bm(vec)))
    }
    fn mlock(&self, region: &Region) -> KResult<()> {
        unit(liella_sys_mlock(z(region.addr()), z(region.len())))
    }
    fn mlock2(&self, region: &Region, flags: u32) -> KResult<()> {
        unit(liella_sys_mlock2(z(region.addr()), z(region.len()), u(flags)))
    }
    fn munlock(&self, region: &Region) -> KResult<()> {
        unit(liella_sys_munlock(z(region.addr()), z(region.len())))
    }
    fn mlockall(&self, flags: u32) -> KResult<()> {
        unit(liella_sys_mlockall(u(flags)))
    }
    fn munlockall(&self) -> KResult<()> {
        unit(liella_sys_munlockall())
    }
    fn remap_file_pages(&self, region: &Region, pgoff: usize) -> KResult<()> {
        if region.kind() != RegionKind::File {
            return Err(Errno::EINVAL);
        }
        unit(liella_sys_remap_file_pages(z(region.addr()), z(region.len()), 0, z(pgoff), 0))
    }
    fn memfd_create(&self, name: KStr<'_>, flags: u32) -> KResult<Fd> {
        fdr(liella_sys_memfd_create(s(name), u(flags)))
    }
    fn pkey_alloc(&self, flags: u32, access: u32) -> KResult<i32> {
        int(liella_sys_pkey_alloc(u(flags), u(access)))
    }
    fn pkey_free(&self, key: i32) -> KResult<()> {
        unit(liella_sys_pkey_free(i64::from(key)))
    }
    fn pkey_mprotect(&self, region: &mut Region, prot: u32, key: i32) -> KResult<()> {
        if prot & !PROT_VALID != 0 {
            return Err(Errno::EINVAL);
        }
        unit(liella_sys_pkey_mprotect(z(region.addr()), z(region.len()), u(prot), i64::from(key)))?;
        let (a, l) = (region.addr(), region.len());
        region.__kernel_update(a, l, prot, key.max(0));
        Ok(())
    }
    fn process_madvise(&self, pidfd: Fd, ranges: &[RemoteIoVec], advice: RemoteAdvice, flags: u32) -> KResult<usize> {
        size(liella_sys_process_madvise(f(pidfd), ranges.as_ptr().cast(), z(ranges.len()), u(advice.code()), u(flags)))
    }
    fn process_mrelease(&self, pidfd: Fd, flags: u32) -> KResult<()> {
        unit(liella_sys_process_mrelease(f(pidfd), u(flags)))
    }
    fn process_vm_readv(&self, pid: i32, local: &mut [IoVecMut<'_>], remote: &[RemoteIoVec], flags: u32) -> KResult<usize> {
        size(liella_sys_process_vm_readv(i64::from(pid), local.as_ptr().cast(), z(local.len()), remote.as_ptr().cast(), z(remote.len()), u(flags)))
    }
    fn map_shadow_stack(&self, hint: usize, size_: usize, flags: u32) -> KResult<Region> {
        let rounded = page_round(size_)?;
        let addr = r(liella_sys_map_shadow_stack(z(hint), z(size_), u(flags)))? as usize;
        Ok(Region::__from_kernel(addr, rounded, PROT_READ, RegionKind::ShadowStack))
    }

    // ------------------------------------------------------------ futexes, threads
    fn futex(&self, word: &AtomicU32, op: u32, val: u32, arg: FutexArg<'_>, word2: Option<&AtomicU32>, val3: u32) -> KResult<i64> {
        let a4: *const c_void = match arg {
            FutexArg::None => ptr::null(),
            FutexArg::Timeout(t) => c(t),
            FutexArg::Val2(v) => ptr::without_provenance(v as usize),
        };
        let w2 = word2.map_or(ptr::null_mut(), |w| w.as_ptr().cast());
        r(liella_sys_futex(word.as_ptr().cast(), u(op), u(val), a4, w2, u(val3))).map(|v| v as i64)
    }
    fn rseq(&self, area: &'static Rseq, flags: u32, sig: u32) -> KResult<()> {
        let p = ptr::from_ref(area).cast_mut().cast();
        unit(liella_sys_rseq(p, z(size_of::<Rseq>()), u(flags), u(sig)))
    }
    fn arch_prctl_get(&self, which: ArchGet) -> KResult<u64> {
        let mut v = 0u64;
        unit(liella_sys_arch_prctl(u(which.code()), xm(&mut v))).map(|()| v)
    }
    fn arch_prctl_get_cpuid(&self) -> KResult<i32> {
        int(liella_sys_arch_prctl(u(ARCH_GET_CPUID), 0))
    }
    fn arch_prctl_set_cpuid(&self, enable: bool) -> KResult<()> {
        unit(liella_sys_arch_prctl(0x1012, i64::from(enable)))
    }

    // ------------------------------------------------------------ descriptors
    fn openat(&self, dir: Fd, path: KStr<'_>, flags: u32, mode: u32) -> KResult<Fd> {
        fdr(sc!(nr::OPENAT; f(dir), s(path), u(flags), u(mode)))
    }
    fn openat2(&self, dir: Fd, path: KStr<'_>, how: &OpenHow) -> KResult<Fd> {
        fdr(liella_sys_openat2(f(dir), s(path), c(how), z(size_of::<OpenHow>())))
    }
    fn open_by_handle_at(&self, mount_fd: Fd, handle: &FileHandle, flags: u32) -> KResult<Fd> {
        fdr(liella_sys_open_by_handle_at(f(mount_fd), c(handle), u(flags)))
    }
    fn name_to_handle_at(&self, dir: Fd, path: KStr<'_>, handle: &mut FileHandle, flags: u32) -> KResult<i32> {
        let mut mount_id = 0i32;
        unit(liella_sys_name_to_handle_at(f(dir), s(path), m(handle), m(&mut mount_id), u(flags))).map(|()| mount_id)
    }
    fn close(&self, fd: Fd) -> KResult<()> {
        // A cancellation point whose descriptor is closed regardless (§22.3, ⚑ KERN): the call
        // is made outside the window, then a due request is acted on.
        let r = liella_sys_close(f(fd));
        if CP {
            if let Some(d) = crate::thread::current() {
                if liella_rt::thread::cancel_due(d.cancel.load(core::sync::atomic::Ordering::Acquire)) {
                    d.cancel.fetch_or(liella_rt::thread::CANCEL_HIT, core::sync::atomic::Ordering::AcqRel);
                }
            }
        }
        unit(r)
    }
    fn close_range(&self, first: u32, last: u32, flags: u32) -> KResult<()> {
        unit(liella_sys_close_range(u(first), u(last), u(flags)))
    }
    fn dup(&self, fd: Fd) -> KResult<Fd> {
        fdr(liella_sys_dup(f(fd)))
    }
    fn dup2(&self, old: Fd, new: Fd) -> KResult<Fd> {
        fdr(liella_sys_dup2(f(old), f(new)))
    }
    fn dup3(&self, old: Fd, new: Fd, flags: u32) -> KResult<Fd> {
        fdr(liella_sys_dup3(f(old), f(new), u(flags)))
    }
    fn fcntl_int(&self, fd: Fd, cmd: FcntlInt, arg: i64) -> KResult<i64> {
        r(liella_sys_fcntl(f(fd), u(cmd.code()), arg)).map(|v| v as i64)
    }
    fn fcntl_lock(&self, fd: Fd, cmd: FcntlLock, lock: &mut Flock) -> KResult<()> {
        unit(liella_sys_fcntl(f(fd), u(cmd.code()), xm(lock)))
    }
    fn fcntl_owner_ex(&self, fd: Fd, set: bool, owner: &mut FOwnerEx) -> KResult<()> {
        let cmd = if set { F_SETOWN_EX } else { F_GETOWN_EX };
        unit(liella_sys_fcntl(f(fd), u(cmd), xm(owner)))
    }
    fn fcntl_hint(&self, fd: Fd, cmd: FcntlHint, hint: &mut u64) -> KResult<()> {
        unit(liella_sys_fcntl(f(fd), u(cmd.code()), xm(hint)))
    }
    fn ioctl_none(&self, fd: Fd, req: IoctlNone) -> KResult<i64> {
        r(liella_sys_ioctl(f(fd), u(req.code()), 0)).map(|v| v as i64)
    }
    fn ioctl_int(&self, fd: Fd, req: IoctlIntArg, arg: i64) -> KResult<i64> {
        // `TCSBRK` waits for output to drain: a cancellation point in the `CP` kernel
        // (`tcdrain`, §19.1).
        r(sc!(nr::IOCTL; f(fd), u(req.code()), arg)).map(|v| v as i64)
    }
    fn ioctl_get_int(&self, fd: Fd, req: IoctlIntOut) -> KResult<i32> {
        let mut v = 0i32;
        unit(liella_sys_ioctl(f(fd), u(req.code()), xm(&mut v))).map(|()| v)
    }
    fn ioctl_set_int(&self, fd: Fd, req: IoctlIntIn, value: i32) -> KResult<i64> {
        r(liella_sys_ioctl(f(fd), u(req.code()), xc(&value))).map(|v| v as i64)
    }
    fn tcgets(&self, fd: Fd) -> KResult<KTermios> {
        let mut t = KTermios::default();
        unit(liella_sys_ioctl(f(fd), u(TCGETS), xm(&mut t))).map(|()| t)
    }
    fn tcsets(&self, fd: Fd, when: TcSet, t: &KTermios) -> KResult<()> {
        unit(liella_sys_ioctl(f(fd), u(when.code()), xc(t)))
    }
    fn tcgets2(&self, fd: Fd) -> KResult<KTermios2> {
        let mut t = KTermios2::default();
        unit(liella_sys_ioctl(f(fd), u(TCGETS2), xm(&mut t))).map(|()| t)
    }
    fn tcsets2(&self, fd: Fd, when: TcSet2, t: &KTermios2) -> KResult<()> {
        unit(liella_sys_ioctl(f(fd), u(when.code()), xc(t)))
    }
    fn tiocgwinsz(&self, fd: Fd) -> KResult<Winsize> {
        let mut w = Winsize::default();
        unit(liella_sys_ioctl(f(fd), u(TIOCGWINSZ), xm(&mut w))).map(|()| w)
    }
    fn tiocswinsz(&self, fd: Fd, ws: &Winsize) -> KResult<()> {
        unit(liella_sys_ioctl(f(fd), 0x5414, xc(ws)))
    }
    fn pipe2(&self, flags: u32) -> KResult<[Fd; 2]> {
        let mut p = [0i32; 2];
        unit(liella_sys_pipe2(m(&mut p), u(flags))).map(|()| p.map(Fd::new))
    }

    // ------------------------------------------------------------ data transfer
    fn read(&self, fd: Fd, buf: &mut [u8]) -> KResult<usize> {
        size(sc!(nr::READ; f(fd), bm(buf), z(buf.len())))
    }
    fn write(&self, fd: Fd, buf: &[u8]) -> KResult<usize> {
        size(sc!(nr::WRITE; f(fd), b(buf), z(buf.len())))
    }
    fn pread(&self, fd: Fd, buf: &mut [u8], offset: i64) -> KResult<usize> {
        size(sc!(nr::PREAD64; f(fd), bm(buf), z(buf.len()), offset))
    }
    fn pwrite(&self, fd: Fd, buf: &[u8], offset: i64) -> KResult<usize> {
        size(sc!(nr::PWRITE64; f(fd), b(buf), z(buf.len()), offset))
    }
    fn readv(&self, fd: Fd, iov: &mut [IoVecMut<'_>]) -> KResult<usize> {
        size(sc!(nr::READV; f(fd), iov.as_ptr().cast(), z(iov.len())))
    }
    fn writev(&self, fd: Fd, iov: &[IoVec<'_>]) -> KResult<usize> {
        size(sc!(nr::WRITEV; f(fd), iov.as_ptr().cast(), z(iov.len())))
    }
    fn preadv(&self, fd: Fd, iov: &mut [IoVecMut<'_>], offset: i64) -> KResult<usize> {
        size(sc!(nr::PREADV; f(fd), iov.as_ptr().cast(), z(iov.len()), offset, 0))
    }
    fn pwritev(&self, fd: Fd, iov: &[IoVec<'_>], offset: i64) -> KResult<usize> {
        size(sc!(nr::PWRITEV; f(fd), iov.as_ptr().cast(), z(iov.len()), offset, 0))
    }
    fn preadv2(&self, fd: Fd, iov: &mut [IoVecMut<'_>], offset: i64, flags: u32) -> KResult<usize> {
        size(sc!(nr::PREADV2; f(fd), iov.as_ptr().cast(), z(iov.len()), offset, 0, u(flags)))
    }
    fn pwritev2(&self, fd: Fd, iov: &[IoVec<'_>], offset: i64, flags: u32) -> KResult<usize> {
        size(sc!(nr::PWRITEV2; f(fd), iov.as_ptr().cast(), z(iov.len()), offset, 0, u(flags)))
    }
    fn lseek(&self, fd: Fd, offset: i64, whence: u32) -> KResult<u64> {
        r(liella_sys_lseek(f(fd), offset, u(whence)))
    }
    fn sendfile(&self, out: Fd, input: Fd, offset: Option<&mut i64>, count: usize) -> KResult<usize> {
        size(liella_sys_sendfile(f(out), f(input), om(offset), z(count)))
    }
    fn copy_file_range(&self, input: Fd, in_off: Option<&mut i64>, out: Fd, out_off: Option<&mut i64>, len: usize, flags: u32) -> KResult<usize> {
        size(liella_sys_copy_file_range(f(input), om(in_off), f(out), om(out_off), z(len), u(flags)))
    }
    fn splice(&self, input: Fd, in_off: Option<&mut i64>, out: Fd, out_off: Option<&mut i64>, len: usize, flags: u32) -> KResult<usize> {
        size(liella_sys_splice(f(input), om(in_off), f(out), om(out_off), z(len), u(flags)))
    }
    fn tee(&self, input: Fd, out: Fd, len: usize, flags: u32) -> KResult<usize> {
        size(liella_sys_tee(f(input), f(out), z(len), u(flags)))
    }
    fn vmsplice(&self, fd: Fd, iov: &mut [IoVecMut<'_>], flags: u32) -> KResult<usize> {
        size(liella_sys_vmsplice(f(fd), iov.as_ptr().cast(), z(iov.len()), u(flags)))
    }
    fn readahead(&self, fd: Fd, offset: i64, count: usize) -> KResult<()> {
        unit(liella_sys_readahead(f(fd), offset, z(count)))
    }

    // ------------------------------------------------------------ readiness, events
    fn pselect6(&self, nfds: u32, read: Option<&mut FdSet>, write: Option<&mut FdSet>, except: Option<&mut FdSet>, timeout: Option<&mut Timespec>, mask: Option<&SigSet>) -> KResult<usize> {
        if nfds > 1024 {
            return Err(Errno::EINVAL);
        }
        let sig = mask.map(|mk| [ptr::from_ref(mk).expose_provenance(), 8usize]);
        size(sc!(nr::PSELECT6; u(nfds), om(read), om(write), om(except), om(timeout), oc(sig.as_ref())))
    }
    fn ppoll(&self, fds: &mut [PollFd], timeout: Option<&mut Timespec>, mask: Option<&SigSet>) -> KResult<usize> {
        size(sc!(nr::PPOLL; fds.as_mut_ptr().cast(), z(fds.len()), om(timeout), oc(mask), SIGSET_SIZE))
    }
    fn epoll_create1(&self, flags: u32) -> KResult<Fd> {
        fdr(liella_sys_epoll_create1(u(flags)))
    }
    fn epoll_ctl(&self, ep: Fd, op: u32, fd: Fd, event: Option<&EpollEvent>) -> KResult<()> {
        unit(liella_sys_epoll_ctl(f(ep), u(op), f(fd), oc(event)))
    }
    fn epoll_pwait(&self, ep: Fd, events: &mut [EpollEvent], timeout_ms: i32, mask: Option<&SigSet>) -> KResult<usize> {
        let n = i32::try_from(events.len()).unwrap_or(i32::MAX);
        size(sc!(nr::EPOLL_PWAIT; f(ep), events.as_mut_ptr().cast(), i64::from(n), i64::from(timeout_ms), oc(mask), SIGSET_SIZE))
    }
    fn epoll_pwait2(&self, ep: Fd, events: &mut [EpollEvent], timeout: Option<&Timespec>, mask: Option<&SigSet>) -> KResult<usize> {
        let n = i32::try_from(events.len()).unwrap_or(i32::MAX);
        size(sc!(nr::EPOLL_PWAIT2; f(ep), events.as_mut_ptr().cast(), i64::from(n), oc(timeout), oc(mask), SIGSET_SIZE))
    }
    fn eventfd2(&self, initval: u32, flags: u32) -> KResult<Fd> {
        fdr(liella_sys_eventfd2(u(initval), u(flags)))
    }
    fn inotify_init1(&self, flags: u32) -> KResult<Fd> {
        fdr(liella_sys_inotify_init1(u(flags)))
    }
    fn inotify_add_watch(&self, fd: Fd, path: KStr<'_>, mask: u32) -> KResult<i32> {
        int(liella_sys_inotify_add_watch(f(fd), s(path), u(mask)))
    }
    fn inotify_rm_watch(&self, fd: Fd, wd: i32) -> KResult<()> {
        unit(liella_sys_inotify_rm_watch(f(fd), i64::from(wd)))
    }
    fn fanotify_init(&self, flags: u32, event_flags: u32) -> KResult<Fd> {
        fdr(liella_sys_fanotify_init(u(flags), u(event_flags)))
    }
    fn fanotify_mark(&self, fd: Fd, flags: u32, mask: u64, dir: Fd, path: Option<KStr<'_>>) -> KResult<()> {
        unit(liella_sys_fanotify_mark(f(fd), u(flags), mask as i64, f(dir), os(path)))
    }

    // ------------------------------------------------------------ durability, space
    fn fsync(&self, fd: Fd) -> KResult<()> {
        unit(sc!(nr::FSYNC; f(fd)))
    }
    fn fdatasync(&self, fd: Fd) -> KResult<()> {
        unit(sc!(nr::FDATASYNC; f(fd)))
    }
    fn syncfs(&self, fd: Fd) -> KResult<()> {
        unit(liella_sys_syncfs(f(fd)))
    }
    fn sync_file_range(&self, fd: Fd, offset: i64, len: i64, flags: u32) -> KResult<()> {
        unit(liella_sys_sync_file_range(f(fd), offset, len, u(flags)))
    }
    fn fadvise(&self, fd: Fd, offset: i64, len: i64, advice: u32) -> KResult<()> {
        unit(liella_sys_fadvise64(f(fd), offset, len, u(advice)))
    }
    fn fallocate(&self, fd: Fd, mode: u32, offset: i64, len: i64) -> KResult<()> {
        unit(liella_sys_fallocate(f(fd), u(mode), offset, len))
    }
    fn ftruncate(&self, fd: Fd, len: i64) -> KResult<()> {
        unit(liella_sys_ftruncate(f(fd), len))
    }
    fn truncate(&self, path: KStr<'_>, len: i64) -> KResult<()> {
        unit(liella_sys_truncate(s(path), len))
    }
    fn flock(&self, fd: Fd, op: u32) -> KResult<()> {
        unit(liella_sys_flock(f(fd), u(op)))
    }

    // ------------------------------------------------------------ namespace and metadata
    fn fstatat(&self, dir: Fd, path: KStr<'_>, flags: u32) -> KResult<Stat> {
        let mut st = Stat::default();
        unit(liella_sys_newfstatat(f(dir), s(path), m(&mut st), u(flags))).map(|()| st)
    }
    fn fstat(&self, fd: Fd) -> KResult<Stat> {
        let mut st = Stat::default();
        unit(liella_sys_fstat(f(fd), m(&mut st))).map(|()| st)
    }
    fn statx(&self, dir: Fd, path: KStr<'_>, flags: u32, mask: u32) -> KResult<Statx> {
        let mut st = Statx::default();
        self.statx_into(dir, path, flags, mask, &mut st).map(|()| st)
    }
    fn statfs(&self, path: KStr<'_>) -> KResult<Statfs> {
        let mut st = Statfs::default();
        unit(liella_sys_statfs(s(path), m(&mut st))).map(|()| st)
    }
    fn fstatfs(&self, fd: Fd) -> KResult<Statfs> {
        let mut st = Statfs::default();
        unit(liella_sys_fstatfs(f(fd), m(&mut st))).map(|()| st)
    }
    fn getdents64(&self, fd: Fd, buf: &mut [u8]) -> KResult<usize> {
        let n = u32::try_from(buf.len()).unwrap_or(u32::MAX);
        size(liella_sys_getdents64(f(fd), bm(buf), u(n)))
    }
    fn getcwd(&self, buf: &mut [u8]) -> KResult<usize> {
        size(liella_sys_getcwd(bm(buf), z(buf.len())))
    }
    fn chdir(&self, path: KStr<'_>) -> KResult<()> {
        unit(liella_sys_chdir(s(path)))
    }
    fn fchdir(&self, fd: Fd) -> KResult<()> {
        unit(liella_sys_fchdir(f(fd)))
    }
    fn chroot(&self, path: KStr<'_>) -> KResult<()> {
        unit(liella_sys_chroot(s(path)))
    }
    fn mkdirat(&self, dir: Fd, path: KStr<'_>, mode: u32) -> KResult<()> {
        unit(liella_sys_mkdirat(f(dir), s(path), u(mode)))
    }
    fn mknodat(&self, dir: Fd, path: KStr<'_>, mode: u32, dev: u32) -> KResult<()> {
        unit(liella_sys_mknodat(f(dir), s(path), u(mode), u(dev)))
    }
    fn linkat(&self, old_dir: Fd, old: KStr<'_>, new_dir: Fd, new: KStr<'_>, flags: u32) -> KResult<()> {
        unit(liella_sys_linkat(f(old_dir), s(old), f(new_dir), s(new), u(flags)))
    }
    fn symlinkat(&self, target: KStr<'_>, dir: Fd, path: KStr<'_>) -> KResult<()> {
        unit(liella_sys_symlinkat(s(target), f(dir), s(path)))
    }
    fn readlinkat(&self, dir: Fd, path: KStr<'_>, buf: &mut [u8]) -> KResult<usize> {
        let n = i32::try_from(buf.len()).unwrap_or(i32::MAX);
        size(liella_sys_readlinkat(f(dir), s(path), bm(buf), i64::from(n)))
    }
    fn unlinkat(&self, dir: Fd, path: KStr<'_>, flags: u32) -> KResult<()> {
        unit(liella_sys_unlinkat(f(dir), s(path), u(flags)))
    }
    fn renameat2(&self, old_dir: Fd, old: KStr<'_>, new_dir: Fd, new: KStr<'_>, flags: u32) -> KResult<()> {
        unit(liella_sys_renameat2(f(old_dir), s(old), f(new_dir), s(new), u(flags)))
    }
    fn fchmodat(&self, dir: Fd, path: KStr<'_>, mode: u32) -> KResult<()> {
        unit(liella_sys_fchmodat(f(dir), s(path), u(mode)))
    }
    fn fchmodat2(&self, dir: Fd, path: KStr<'_>, mode: u32, flags: u32) -> KResult<()> {
        unit(liella_sys_fchmodat2(f(dir), s(path), u(mode), u(flags)))
    }
    fn fchmod(&self, fd: Fd, mode: u32) -> KResult<()> {
        unit(liella_sys_fchmod(f(fd), u(mode)))
    }
    fn fchownat(&self, dir: Fd, path: KStr<'_>, uid: u32, gid: u32, flags: u32) -> KResult<()> {
        unit(liella_sys_fchownat(f(dir), s(path), u(uid), u(gid), u(flags)))
    }
    fn fchown(&self, fd: Fd, uid: u32, gid: u32) -> KResult<()> {
        unit(liella_sys_fchown(f(fd), u(uid), u(gid)))
    }
    fn faccessat(&self, dir: Fd, path: KStr<'_>, mode: u32) -> KResult<()> {
        unit(liella_sys_faccessat(f(dir), s(path), u(mode)))
    }
    fn faccessat2(&self, dir: Fd, path: KStr<'_>, mode: u32, flags: u32) -> KResult<()> {
        unit(liella_sys_faccessat2(f(dir), s(path), u(mode), u(flags)))
    }
    fn utimensat(&self, dir: Fd, path: Option<KStr<'_>>, times: Option<&[Timespec; 2]>, flags: u32) -> KResult<()> {
        unit(liella_sys_utimensat(f(dir), os(path), oc(times), u(flags)))
    }
    fn setxattr(&self, path: KStr<'_>, nofollow: bool, name: KStr<'_>, value: &[u8], flags: u32) -> KResult<()> {
        let call = if nofollow { liella_sys_lsetxattr } else { liella_sys_setxattr };
        unit(call(s(path), s(name), b(value), z(value.len()), u(flags)))
    }
    fn fsetxattr(&self, fd: Fd, name: KStr<'_>, value: &[u8], flags: u32) -> KResult<()> {
        unit(liella_sys_fsetxattr(f(fd), s(name), b(value), z(value.len()), u(flags)))
    }
    fn getxattr(&self, path: KStr<'_>, nofollow: bool, name: KStr<'_>, buf: &mut [u8]) -> KResult<usize> {
        let call = if nofollow { liella_sys_lgetxattr } else { liella_sys_getxattr };
        size(call(s(path), s(name), bm(buf), z(buf.len())))
    }
    fn fgetxattr(&self, fd: Fd, name: KStr<'_>, buf: &mut [u8]) -> KResult<usize> {
        size(liella_sys_fgetxattr(f(fd), s(name), bm(buf), z(buf.len())))
    }
    fn listxattr(&self, path: KStr<'_>, nofollow: bool, buf: &mut [u8]) -> KResult<usize> {
        let call = if nofollow { liella_sys_llistxattr } else { liella_sys_listxattr };
        size(call(s(path), bm(buf), z(buf.len())))
    }
    fn flistxattr(&self, fd: Fd, buf: &mut [u8]) -> KResult<usize> {
        size(liella_sys_flistxattr(f(fd), bm(buf), z(buf.len())))
    }
    fn removexattr(&self, path: KStr<'_>, nofollow: bool, name: KStr<'_>) -> KResult<()> {
        let call = if nofollow { liella_sys_lremovexattr } else { liella_sys_removexattr };
        unit(call(s(path), s(name)))
    }
    fn fremovexattr(&self, fd: Fd, name: KStr<'_>) -> KResult<()> {
        unit(liella_sys_fremovexattr(f(fd), s(name)))
    }

    // ------------------------------------------------------------ mounting
    fn mount(&self, source: Option<KStr<'_>>, target: KStr<'_>, fstype: Option<KStr<'_>>, flags: u64, data: Option<KStr<'_>>) -> KResult<()> {
        unit(liella_sys_mount(os(source), s(target), os(fstype), flags as i64, os(data)))
    }
    fn umount2(&self, target: KStr<'_>, flags: u32) -> KResult<()> {
        unit(liella_sys_umount2(s(target), u(flags)))
    }
    fn pivot_root(&self, new_root: KStr<'_>, put_old: KStr<'_>) -> KResult<()> {
        unit(liella_sys_pivot_root(s(new_root), s(put_old)))
    }
    fn fsopen(&self, fstype: KStr<'_>, flags: u32) -> KResult<Fd> {
        fdr(liella_sys_fsopen(s(fstype), u(flags)))
    }
    fn fsconfig(&self, fd: Fd, cmd: FsconfigCmd, key: Option<KStr<'_>>, value: FsValue<'_>, aux: i32) -> KResult<()> {
        let v = match value {
            FsValue::None => ptr::null(),
            FsValue::Str(k) => s(k),
            FsValue::Binary(bytes) => b(bytes),
        };
        unit(liella_sys_fsconfig(f(fd), u(cmd.code()), os(key), v, i64::from(aux)))
    }
    fn fsmount(&self, fd: Fd, flags: u32, attrs: u32) -> KResult<Fd> {
        fdr(liella_sys_fsmount(f(fd), u(flags), u(attrs)))
    }
    fn fspick(&self, dir: Fd, path: KStr<'_>, flags: u32) -> KResult<Fd> {
        fdr(liella_sys_fspick(f(dir), s(path), u(flags)))
    }
    fn move_mount(&self, from_dir: Fd, from: KStr<'_>, to_dir: Fd, to: KStr<'_>, flags: u32) -> KResult<()> {
        unit(liella_sys_move_mount(f(from_dir), s(from), f(to_dir), s(to), u(flags)))
    }
    fn open_tree(&self, dir: Fd, path: KStr<'_>, flags: u32) -> KResult<Fd> {
        fdr(liella_sys_open_tree(f(dir), s(path), u(flags)))
    }
    fn mount_setattr(&self, dir: Fd, path: KStr<'_>, flags: u32, attr: &MountAttr) -> KResult<()> {
        unit(liella_sys_mount_setattr(f(dir), s(path), u(flags), c(attr), z(size_of::<MountAttr>())))
    }
    fn swapon(&self, path: KStr<'_>, flags: u32) -> KResult<()> {
        unit(liella_sys_swapon(s(path), u(flags)))
    }
    fn swapoff(&self, path: KStr<'_>) -> KResult<()> {
        unit(liella_sys_swapoff(s(path)))
    }
    fn quotactl(&self, cmd: QuotaCmd, qtype: u32, special: Option<KStr<'_>>, id: i32, buf: &mut [u8]) -> KResult<()> {
        if buf.len() < cmd.buffer_len() {
            return Err(Errno::EINVAL);
        }
        let addr = if cmd.buffer_len() == 0 { ptr::null_mut() } else { bm(buf) };
        unit(liella_sys_quotactl(u((cmd.code() << 8) | (qtype & 0xff)), os(special), i64::from(id), addr))
    }
    fn statmount(&self, req: &MntIdReq, buf: &mut [u8], flags: u32) -> KResult<()> {
        unit(liella_sys_statmount(c(req), bm(buf), z(buf.len()), u(flags)))
    }
    fn listmount(&self, req: &MntIdReq, ids: &mut [u64], flags: u32) -> KResult<usize> {
        size(liella_sys_listmount(c(req), ids.as_mut_ptr().cast(), z(ids.len()), u(flags)))
    }
    fn quotactl_on(&self, qtype: u32, special: KStr<'_>, format: i32, quota_file: KStr<'_>) -> KResult<()> {
        // Q_QUOTAON reads the quota file name through `addr` (the table's `p` kind covers both
        // directions; this command only reads).
        unit(liella_sys_quotactl(u((0x80_0002 << 8) | (qtype & 0xff)), s(special), i64::from(format), s(quota_file).cast_mut()))
    }

    // ------------------------------------------------------------ sockets
    fn socket(&self, domain: u32, stype: u32, protocol: u32) -> KResult<Fd> {
        fdr(liella_sys_socket(u(domain), u(stype), u(protocol)))
    }
    fn socketpair(&self, domain: u32, stype: u32, protocol: u32) -> KResult<[Fd; 2]> {
        let mut sv = [0i32; 2];
        unit(liella_sys_socketpair(u(domain), u(stype), u(protocol), m(&mut sv))).map(|()| sv.map(Fd::new))
    }
    fn bind(&self, fd: Fd, addr: &[u8]) -> KResult<()> {
        let n = u32::try_from(addr.len()).map_err(|_| Errno::EINVAL)?;
        unit(liella_sys_bind(f(fd), b(addr), u(n)))
    }
    fn listen(&self, fd: Fd, backlog: i32) -> KResult<()> {
        unit(liella_sys_listen(f(fd), i64::from(backlog)))
    }
    fn accept4(&self, fd: Fd, peer: Option<&mut SockAddrBuf>, flags: u32) -> KResult<Fd> {
        match peer {
            None => fdr(sc!(nr::ACCEPT4; f(fd), ptr::null_mut(), ptr::null_mut(), u(flags))),
            Some(p) => {
                let mut len = p.capacity();
                let a = ptr::with_exposed_provenance_mut(p.storage_addr());
                let out = fdr(sc!(nr::ACCEPT4; f(fd), a, m(&mut len), u(flags)))?;
                p.__set_len(len);
                Ok(out)
            }
        }
    }
    fn connect(&self, fd: Fd, addr: &[u8]) -> KResult<()> {
        let n = u32::try_from(addr.len()).map_err(|_| Errno::EINVAL)?;
        unit(sc!(nr::CONNECT; f(fd), b(addr), u(n)))
    }
    fn shutdown(&self, fd: Fd, how: u32) -> KResult<()> {
        unit(liella_sys_shutdown(f(fd), u(how)))
    }
    fn getsockname(&self, fd: Fd, addr: &mut SockAddrBuf) -> KResult<()> {
        let mut len = addr.capacity();
        let a = ptr::with_exposed_provenance_mut(addr.storage_addr());
        unit(liella_sys_getsockname(f(fd), a, m(&mut len)))?;
        addr.__set_len(len);
        Ok(())
    }
    fn getpeername(&self, fd: Fd, addr: &mut SockAddrBuf) -> KResult<()> {
        let mut len = addr.capacity();
        let a = ptr::with_exposed_provenance_mut(addr.storage_addr());
        unit(liella_sys_getpeername(f(fd), a, m(&mut len)))?;
        addr.__set_len(len);
        Ok(())
    }
    fn getsockopt(&self, fd: Fd, level: u32, name: u32, value: &mut [u8]) -> KResult<usize> {
        let mut len = u32::try_from(value.len()).unwrap_or(u32::MAX);
        unit(liella_sys_getsockopt(f(fd), u(level), u(name), bm(value), m(&mut len)))?;
        Ok(len as usize)
    }
    fn setsockopt(&self, fd: Fd, level: SockLevel, name: u32, value: &[u8]) -> KResult<()> {
        let n = u32::try_from(value.len()).map_err(|_| Errno::EINVAL)?;
        unit(liella_sys_setsockopt(f(fd), u(level.code()), u(name), b(value), u(n)))
    }
    fn sendto(&self, fd: Fd, buf: &[u8], flags: u32, addr: Option<&[u8]>) -> KResult<usize> {
        let (a, n) = match addr {
            Some(a) => (b(a), u32::try_from(a.len()).map_err(|_| Errno::EINVAL)?),
            None => (ptr::null(), 0),
        };
        size(sc!(nr::SENDTO; f(fd), b(buf), z(buf.len()), u(flags), a, u(n)))
    }
    fn recvfrom(&self, fd: Fd, buf: &mut [u8], flags: u32, from: Option<&mut SockAddrBuf>) -> KResult<usize> {
        match from {
            None => size(sc!(nr::RECVFROM; f(fd), bm(buf), z(buf.len()), u(flags), ptr::null_mut(), ptr::null_mut())),
            Some(p) => {
                let mut len = p.capacity();
                let a = ptr::with_exposed_provenance_mut(p.storage_addr());
                let n = size(sc!(nr::RECVFROM; f(fd), bm(buf), z(buf.len()), u(flags), a, m(&mut len)))?;
                p.__set_len(len);
                Ok(n)
            }
        }
    }
    fn sendmsg(&self, fd: Fd, msg: &MsgHdr<'_>, flags: u32) -> KResult<usize> {
        size(sc!(nr::SENDMSG; f(fd), c(msg), u(flags)))
    }
    fn recvmsg(&self, fd: Fd, msg: &mut MsgHdrMut<'_>, flags: u32) -> KResult<usize> {
        size(sc!(nr::RECVMSG; f(fd), m(msg), u(flags)))
    }
    fn sendmmsg(&self, fd: Fd, msgs: &mut [MMsgHdr<'_>], flags: u32) -> KResult<usize> {
        let n = u32::try_from(msgs.len()).unwrap_or(u32::MAX);
        size(sc!(nr::SENDMMSG; f(fd), msgs.as_mut_ptr().cast(), u(n), u(flags)))
    }
    fn recvmmsg(&self, fd: Fd, msgs: &mut [MMsgHdrMut<'_>], flags: u32, timeout: Option<&mut Timespec>) -> KResult<usize> {
        let n = u32::try_from(msgs.len()).unwrap_or(u32::MAX);
        size(sc!(nr::RECVMMSG; f(fd), msgs.as_mut_ptr().cast(), u(n), u(flags), om(timeout)))
    }

    // ------------------------------------------------------------ time
    fn tsc(&self) -> u64 {
        // ARCH(x86_64): rdtsc and its PR_GET_TSC gate (D-7.14); aarch64 reads CNTVCT_EL0 (A-6)
        #[cfg(target_arch = "x86_64")]
        return match TSC_STATE.load(core::sync::atomic::Ordering::Relaxed) {
            TSC_OK => ffi::liella_machine_rdtsc(),
            TSC_UNKNOWN => self.tsc_probe(),
            _ => 0,
        };
        // ARCH(x86_64): rdtsc and its PR_GET_TSC gate (D-7.14); aarch64 reads CNTVCT_EL0 (A-6)
        #[cfg(not(target_arch = "x86_64"))]
        0
    }

    fn clock_gettime(&self, clock: i32) -> KResult<Timespec> {
        match vdso_fn(V_CLOCK_GETTIME) {
            Some(func) => {
                let mut ts = Timespec::default();
                unit(ffi::liella_machine_vcall2(func, i64::from(clock), m(&mut ts))).map(|()| ts)
            }
            None => self.clock_gettime_syscall(clock),
        }
    }
    fn clock_getres(&self, clock: i32) -> KResult<Timespec> {
        match vdso_fn(V_CLOCK_GETRES) {
            Some(func) => {
                let mut ts = Timespec::default();
                unit(ffi::liella_machine_vcall2(func, i64::from(clock), m(&mut ts))).map(|()| ts)
            }
            None => self.clock_getres_syscall(clock),
        }
    }
    fn clock_settime(&self, clock: i32, ts: &Timespec) -> KResult<()> {
        unit(liella_sys_clock_settime(i64::from(clock), c(ts)))
    }
    fn clock_adjtime(&self, clock: i32, tx: &mut Timex) -> KResult<i32> {
        int(liella_sys_clock_adjtime(i64::from(clock), m(tx)))
    }
    fn clock_nanosleep(&self, clock: i32, flags: u32, req: &Timespec, rem: Option<&mut Timespec>) -> KResult<()> {
        unit(sc!(nr::CLOCK_NANOSLEEP; i64::from(clock), u(flags), c(req), om(rem)))
    }
    fn gettimeofday(&self) -> KResult<(Timeval, Timezone)> {
        match vdso_fn(V_GETTIMEOFDAY) {
            Some(func) => {
                let (mut tv, mut tz) = (Timeval::default(), Timezone::default());
                unit(ffi::liella_machine_vcall3(func, m(&mut tv), m(&mut tz), ptr::null_mut())).map(|()| (tv, tz))
            }
            None => self.gettimeofday_syscall(),
        }
    }
    fn settimeofday(&self, tv: Option<&Timeval>, tz: Option<&Timezone>) -> KResult<()> {
        unit(liella_sys_settimeofday(oc(tv), oc(tz)))
    }
    fn time(&self) -> KResult<i64> {
        match vdso_fn(V_TIME) {
            Some(func) => r(ffi::liella_machine_vcall3(func, ptr::null_mut(), ptr::null_mut(), ptr::null_mut())).map(|v| v as i64),
            None => self.time_syscall(),
        }
    }
    fn times(&self, buf: &mut Tms) -> KResult<u64> {
        r(liella_sys_times(m(buf)))
    }
    fn setitimer(&self, which: u32, new: &Itimerval) -> KResult<Itimerval> {
        let mut old = Itimerval::default();
        unit(liella_sys_setitimer(u(which), c(new), m(&mut old))).map(|()| old)
    }
    fn getitimer(&self, which: u32) -> KResult<Itimerval> {
        let mut cur = Itimerval::default();
        unit(liella_sys_getitimer(u(which), m(&mut cur))).map(|()| cur)
    }
    fn alarm(&self, seconds: u32) -> KResult<u32> {
        r(liella_sys_alarm(u(seconds))).map(|v| v as u32)
    }
    fn timer_create(&self, clock: i32, event: Option<&SigEvent>) -> KResult<i32> {
        let mut id = 0i32;
        unit(liella_sys_timer_create(i64::from(clock), oc(event), m(&mut id))).map(|()| id)
    }
    fn timer_settime(&self, id: i32, flags: u32, new: &Itimerspec) -> KResult<Itimerspec> {
        let mut old = Itimerspec::default();
        unit(liella_sys_timer_settime(i64::from(id), u(flags), c(new), m(&mut old))).map(|()| old)
    }
    fn timer_gettime(&self, id: i32) -> KResult<Itimerspec> {
        let mut cur = Itimerspec::default();
        unit(liella_sys_timer_gettime(i64::from(id), m(&mut cur))).map(|()| cur)
    }
    fn timer_getoverrun(&self, id: i32) -> KResult<i32> {
        int(liella_sys_timer_getoverrun(i64::from(id)))
    }
    fn timer_delete(&self, id: i32) -> KResult<()> {
        unit(liella_sys_timer_delete(i64::from(id)))
    }
    fn timerfd_create(&self, clock: i32, flags: u32) -> KResult<Fd> {
        fdr(liella_sys_timerfd_create(i64::from(clock), u(flags)))
    }
    fn timerfd_settime(&self, fd: Fd, flags: u32, new: &Itimerspec) -> KResult<Itimerspec> {
        let mut old = Itimerspec::default();
        unit(liella_sys_timerfd_settime(f(fd), u(flags), c(new), m(&mut old))).map(|()| old)
    }
    fn timerfd_gettime(&self, fd: Fd) -> KResult<Itimerspec> {
        let mut cur = Itimerspec::default();
        unit(liella_sys_timerfd_gettime(f(fd), m(&mut cur))).map(|()| cur)
    }

    // ------------------------------------------------------------ scheduling, resources
    fn sched_setscheduler(&self, pid: i32, policy: u32, param: &SchedParam) -> KResult<()> {
        unit(liella_sys_sched_setscheduler(i64::from(pid), u(policy), c(param)))
    }
    fn sched_getscheduler(&self, pid: i32) -> KResult<u32> {
        r(liella_sys_sched_getscheduler(i64::from(pid))).map(|v| v as u32)
    }
    fn sched_setparam(&self, pid: i32, param: &SchedParam) -> KResult<()> {
        unit(liella_sys_sched_setparam(i64::from(pid), c(param)))
    }
    fn sched_getparam(&self, pid: i32) -> KResult<SchedParam> {
        let mut p = SchedParam::default();
        unit(liella_sys_sched_getparam(i64::from(pid), m(&mut p))).map(|()| p)
    }
    fn sched_get_priority_min(&self, policy: u32) -> KResult<i32> {
        int(liella_sys_sched_get_priority_min(u(policy)))
    }
    fn sched_get_priority_max(&self, policy: u32) -> KResult<i32> {
        int(liella_sys_sched_get_priority_max(u(policy)))
    }
    fn sched_rr_get_interval(&self, pid: i32) -> KResult<Timespec> {
        let mut ts = Timespec::default();
        unit(liella_sys_sched_rr_get_interval(i64::from(pid), m(&mut ts))).map(|()| ts)
    }
    fn sched_setaffinity(&self, pid: i32, mask: &[u8]) -> KResult<()> {
        unit(liella_sys_sched_setaffinity(i64::from(pid), z(mask.len()), b(mask)))
    }
    fn sched_getaffinity(&self, pid: i32, mask: &mut [u8]) -> KResult<usize> {
        size(liella_sys_sched_getaffinity(i64::from(pid), z(mask.len()), bm(mask)))
    }
    fn getcpu(&self) -> KResult<(u32, u32)> {
        match vdso_fn(V_GETCPU) {
            Some(func) => {
                let (mut cpu, mut node) = (0u32, 0u32);
                unit(ffi::liella_machine_vcall3(func, m(&mut cpu), m(&mut node), ptr::null_mut())).map(|()| (cpu, node))
            }
            None => self.getcpu_syscall(),
        }
    }
    fn getpriority(&self, which: u32, who: u32) -> KResult<i32> {
        int(liella_sys_getpriority(u(which), u(who)))
    }
    fn setpriority(&self, which: u32, who: u32, prio: i32) -> KResult<()> {
        unit(liella_sys_setpriority(u(which), u(who), i64::from(prio)))
    }
    fn prlimit(&self, pid: i32, resource: u32, new: Option<&Rlimit>) -> KResult<Rlimit> {
        let mut old = Rlimit::default();
        unit(liella_sys_prlimit64(i64::from(pid), u(resource), oc(new), m(&mut old))).map(|()| old)
    }
    fn getrusage(&self, who: i32) -> KResult<Rusage> {
        let mut ru = Rusage::default();
        unit(liella_sys_getrusage(i64::from(who), m(&mut ru))).map(|()| ru)
    }
    fn sysinfo(&self) -> KResult<Sysinfo> {
        let mut si = Sysinfo::default();
        unit(liella_sys_sysinfo(m(&mut si))).map(|()| si)
    }
    fn sched_setattr(&self, pid: i32, attr: &SchedAttr, flags: u32) -> KResult<()> {
        unit(liella_sys_sched_setattr(i64::from(pid), c(attr), u(flags)))
    }
    fn sched_getattr(&self, pid: i32, flags: u32) -> KResult<SchedAttr> {
        let mut a = SchedAttr::default();
        let size = a.size();
        unit(liella_sys_sched_getattr(i64::from(pid), m(&mut a), u(size), u(flags))).map(|()| a)
    }

    // ------------------------------------------------------------ IPC
    fn msgget(&self, key: i32, flags: u32) -> KResult<i32> {
        int(liella_sys_msgget(i64::from(key), u(flags)))
    }
    fn msgsnd(&self, id: i32, msg: &[u8], flags: u32) -> KResult<()> {
        let text = msg.len().checked_sub(8).ok_or(Errno::EINVAL)?;
        unit(liella_sys_msgsnd(i64::from(id), b(msg), z(text), u(flags)))
    }
    fn msgrcv(&self, id: i32, buf: &mut [u8], mtype: i64, flags: u32) -> KResult<usize> {
        let text = buf.len().checked_sub(8).ok_or(Errno::EINVAL)?;
        size(liella_sys_msgrcv(i64::from(id), bm(buf), z(text), mtype, u(flags)))
    }
    fn msgctl(&self, id: i32, cmd: MsgCtl, buf: &mut MsqidDs) -> KResult<i32> {
        int(liella_sys_msgctl(i64::from(id), u(cmd.code()), m(buf)))
    }
    fn semget(&self, key: i32, nsems: i32, flags: u32) -> KResult<i32> {
        int(liella_sys_semget(i64::from(key), i64::from(nsems), u(flags)))
    }
    fn semtimedop(&self, id: i32, ops: &[Sembuf], timeout: Option<&Timespec>) -> KResult<()> {
        unit(liella_sys_semtimedop(i64::from(id), ops.as_ptr().cast(), z(ops.len()), oc(timeout)))
    }
    fn semctl_int(&self, id: i32, num: i32, cmd: SemCtlInt, value: i32) -> KResult<i32> {
        int(liella_sys_semctl(i64::from(id), i64::from(num), u(cmd.code()), i64::from(value)))
    }
    fn semctl_ds(&self, id: i32, cmd: SemCtlDs, buf: &mut SemidDs) -> KResult<i32> {
        int(liella_sys_semctl(i64::from(id), 0, u(cmd.code()), xm(buf)))
    }
    fn semctl_getall(&self, id: i32, values: &mut [u16]) -> KResult<()> {
        let mut ds = SemidDs::default();
        self.semctl_ds(id, SemCtlDs::Stat, &mut ds)?;
        if (values.len() as u64) < ds.nsems() {
            return Err(Errno::EINVAL);
        }
        unit(liella_sys_semctl(i64::from(id), 0, u(GETALL), values.as_mut_ptr().expose_provenance() as i64))
    }
    fn semctl_setall(&self, id: i32, values: &[u16]) -> KResult<()> {
        let mut ds = SemidDs::default();
        self.semctl_ds(id, SemCtlDs::Stat, &mut ds)?;
        if (values.len() as u64) < ds.nsems() {
            return Err(Errno::EINVAL);
        }
        unit(liella_sys_semctl(i64::from(id), 0, u(SETALL), values.as_ptr().expose_provenance() as i64))
    }
    fn shmget(&self, key: i32, size_: usize, flags: u32) -> KResult<i32> {
        int(liella_sys_shmget(i64::from(key), z(size_), u(flags)))
    }
    fn shmat(&self, id: i32, flags: u32) -> KResult<Region> {
        if flags & SHM_REMAP != 0 {
            return Err(Errno::EINVAL);
        }
        let mut ds = ShmidDs::default();
        self.shmctl(id, ShmCtl::Stat, &mut ds)?;
        let len = page_round(ds.segsz() as usize)?;
        let addr = r(liella_sys_shmat(i64::from(id), 0, u(flags)))? as usize;
        let mut prot = if flags & SHM_RDONLY != 0 { PROT_READ } else { PROT_READ | PROT_WRITE };
        if flags & SHM_EXEC != 0 {
            prot |= PROT_EXEC;
        }
        Ok(Region::__from_kernel(addr, len, prot, RegionKind::Shm))
    }
    fn shmdt(&self, region: Region) -> KResult<()> {
        if region.kind() != RegionKind::Shm {
            return Err(Errno::EINVAL);
        }
        unit(liella_sys_shmdt(z(region.addr())))
    }
    fn shmctl(&self, id: i32, cmd: ShmCtl, buf: &mut ShmidDs) -> KResult<i32> {
        int(liella_sys_shmctl(i64::from(id), u(cmd.code()), m(buf)))
    }
    fn mq_open(&self, name: KStr<'_>, flags: u32, mode: u32, attr: Option<&MqAttr>) -> KResult<Fd> {
        fdr(liella_sys_mq_open(s(name), u(flags), u(mode), oc(attr)))
    }
    fn mq_unlink(&self, name: KStr<'_>) -> KResult<()> {
        unit(liella_sys_mq_unlink(s(name)))
    }
    fn mq_timedsend(&self, fd: Fd, msg: &[u8], prio: u32, timeout: Option<&Timespec>) -> KResult<()> {
        // A cancellation point in the `CP` kernel (§25.3).
        unit(sc!(nr::MQ_TIMEDSEND; f(fd), b(msg), z(msg.len()), u(prio), oc(timeout)))
    }
    fn mq_timedreceive(&self, fd: Fd, buf: &mut [u8], prio: Option<&mut u32>, timeout: Option<&Timespec>) -> KResult<usize> {
        size(sc!(nr::MQ_TIMEDRECEIVE; f(fd), bm(buf), z(buf.len()), om(prio), oc(timeout)))
    }
    fn mq_notify(&self, fd: Fd, event: Option<&SigEvent>) -> KResult<()> {
        unit(liella_sys_mq_notify(f(fd), oc(event)))
    }
    fn mq_getsetattr(&self, fd: Fd, new: Option<&MqAttr>) -> KResult<MqAttr> {
        let mut old = MqAttr::default();
        unit(liella_sys_mq_getsetattr(f(fd), oc(new), m(&mut old))).map(|()| old)
    }

    // ------------------------------------------------------------ randomness
    fn getrandom(&self, buf: &mut [u8], flags: u32) -> KResult<usize> {
        size(sc!(nr::GETRANDOM; bm(buf), z(buf.len()), u(flags)))
    }

    // ------------------------------------------------------------ system
    fn uname(&self) -> KResult<Utsname> {
        let mut un = Utsname::default();
        unit(liella_sys_uname(m(&mut un))).map(|()| un)
    }
    fn sethostname(&self, name: &[u8]) -> KResult<()> {
        unit(liella_sys_sethostname(b(name), z(name.len())))
    }
    fn setdomainname(&self, name: &[u8]) -> KResult<()> {
        unit(liella_sys_setdomainname(b(name), z(name.len())))
    }
    fn reboot(&self, magic1: u32, magic2: u32, cmd: u32, arg: Option<KStr<'_>>) -> KResult<()> {
        unit(liella_sys_reboot(u(magic1), u(magic2), u(cmd), os(arg)))
    }
    fn syslog(&self, action: i32, buf: &mut [u8]) -> KResult<usize> {
        let n = i32::try_from(buf.len()).unwrap_or(i32::MAX);
        size(liella_sys_syslog(i64::from(action), bm(buf), i64::from(n)))
    }
    fn acct(&self, path: Option<KStr<'_>>) -> KResult<()> {
        unit(liella_sys_acct(os(path)))
    }
    fn ioperm(&self, from: u64, num: u64, on: bool) -> KResult<()> {
        unit(liella_sys_ioperm(from as i64, num as i64, i64::from(on)))
    }
    fn iopl(&self, level: u32) -> KResult<()> {
        unit(liella_sys_iopl(u(level)))
    }
    fn modify_ldt_read(&self, func: u32, buf: &mut [u8]) -> KResult<usize> {
        if func != 0 && func != 2 {
            return Err(Errno::EINVAL);
        }
        // The kernel's result is an `int` in the low half of the register (an error such as
        // -EFAULT arrives as 0xfffffff2): read it as an `int` before decoding.
        size(i64::from(liella_sys_modify_ldt(u(func), bm(buf), z(buf.len())) as i32))
    }
    fn init_module(&self, image: &[u8], params: KStr<'_>) -> KResult<()> {
        unit(liella_sys_init_module(b(image), z(image.len()), s(params)))
    }
    fn finit_module(&self, fd: Fd, params: KStr<'_>, flags: u32) -> KResult<()> {
        unit(liella_sys_finit_module(f(fd), s(params), u(flags)))
    }
    fn delete_module(&self, name: KStr<'_>, flags: u32) -> KResult<()> {
        unit(liella_sys_delete_module(s(name), u(flags)))
    }
    fn vhangup(&self) -> KResult<()> {
        unit(liella_sys_vhangup())
    }
    fn personality(&self, persona: u32) -> KResult<u32> {
        r(liella_sys_personality(u(persona))).map(|v| v as u32)
    }
    fn prctl_int(&self, opt: PrctlInt, a2: u64, a3: u64) -> KResult<i64> {
        r(liella_sys_prctl(u(opt.code()), a2 as i64, a3 as i64, 0, 0)).map(|v| v as i64)
    }
    fn prctl_get_int(&self, opt: PrctlIntOut) -> KResult<i32> {
        let mut v = 0i32;
        unit(liella_sys_prctl(u(opt.code()), xm(&mut v), 0, 0, 0)).map(|()| v)
    }
    fn prctl_set_name(&self, name: &[u8; 16]) -> KResult<()> {
        unit(liella_sys_prctl(u(PR_SET_NAME), xc(name), 0, 0, 0))
    }
    fn prctl_get_name(&self) -> KResult<[u8; 16]> {
        let mut name = [0u8; 16];
        unit(liella_sys_prctl(u(PR_GET_NAME), xm(&mut name), 0, 0, 0)).map(|()| name)
    }
    fn unshare(&self, flags: u64) -> KResult<()> {
        unit(liella_sys_unshare(flags as i64))
    }
    fn setns(&self, fd: Fd, nstype: u32) -> KResult<()> {
        unit(liella_sys_setns(f(fd), u(nstype)))
    }
    fn ptrace(&self, op: PtraceOp, pid: i32, addr: u64, data: u64) -> KResult<i64> {
        r(liella_sys_ptrace(u(op.code()), i64::from(pid), addr as i64, data as i64)).map(|v| v as i64)
    }
    fn ptrace_peek(&self, op: PtracePeek, pid: i32, addr: u64) -> KResult<u64> {
        let mut word = 0u64;
        unit(liella_sys_ptrace(u(op.code()), i64::from(pid), addr as i64, xm(&mut word))).map(|()| word)
    }

    // ------------------------------------------------------------ infallible
    fn getpid(&self) -> i32 {
        liella_sys_getpid() as i32
    }
    fn getppid(&self) -> i32 {
        liella_sys_getppid() as i32
    }
    fn gettid(&self) -> i32 {
        liella_sys_gettid() as i32
    }
    fn getuid(&self) -> u32 {
        liella_sys_getuid() as u32
    }
    fn geteuid(&self) -> u32 {
        liella_sys_geteuid() as u32
    }
    fn getgid(&self) -> u32 {
        liella_sys_getgid() as u32
    }
    fn getegid(&self) -> u32 {
        liella_sys_getegid() as u32
    }
    fn setfsuid(&self, uid: u32) -> u32 {
        liella_sys_setfsuid(u(uid)) as u32
    }
    fn setfsgid(&self, gid: u32) -> u32 {
        liella_sys_setfsgid(u(gid)) as u32
    }
    fn getpgrp(&self) -> i32 {
        liella_sys_getpgrp() as i32
    }
    fn sched_yield(&self) {
        let _ = liella_sys_sched_yield();
    }
    fn sync(&self) {
        let _ = liella_sys_sync();
    }
    fn umask(&self, mask: u32) -> u32 {
        liella_sys_umask(u(mask)) as u32
    }
    fn exit_group(&self, status: i32) -> ! {
        loop {
            let _ = liella_sys_exit_group(i64::from(status));
        }
    }
    fn region_bytes<'a>(&self, region: &'a mut Region) -> Option<&'a mut [u8]> {
        conv::region_bytes(region)
    }
    fn region_words<'a>(&self, region: &'a Region) -> Option<&'a [AtomicU32]> {
        conv::region_words(region)
    }
}

// ---------------------------------------------------------------------------- Machine

/// Cached `MXCSR_MASK` (0 = not yet read).
static MXCSR_MASK: AtomicU32 = AtomicU32::new(0);

fn mxcsr_mask() -> u32 {
    let cached = MXCSR_MASK.load(Ordering::Relaxed);
    if cached != 0 {
        return cached;
    }
    // FXSAVE reports 0 when the CPU predates MXCSR_MASK; the architectural default then applies.
    let mask = match ffi::liella_machine_mxcsr_mask() {
        0 => MXCSR_DEFINED,
        v => v,
    };
    MXCSR_MASK.store(mask, Ordering::Relaxed);
    mask
}

impl Machine for CSys {
    fn thread_pointer(&self) -> usize {
        crate::tb::tp()
    }
    fn cpuid(&self, leaf: u32, subleaf: u32) -> [u32; 4] {
        let mut out = [0u32; 4];
        ffi::liella_machine_cpuid(leaf, subleaf, &mut out);
        out
    }
    fn xcr0(&self) -> Option<u64> {
        let [_, _, ecx, _] = self.cpuid(1, 0);
        (ecx & (1 << 27) != 0).then(|| ffi::liella_machine_xgetbv0())
    }
    fn mxcsr(&self) -> u32 {
        ffi::liella_machine_mxcsr_get()
    }
    fn set_mxcsr(&self, value: u32) {
        ffi::liella_machine_mxcsr_set(value & mxcsr_mask());
    }
    fn x87_control(&self) -> u16 {
        ffi::liella_machine_x87_cw_get() as u16
    }
    fn set_x87_control(&self, value: u16) {
        ffi::liella_machine_x87_cw_set(u32::from(value));
    }
    fn x87_status(&self) -> u16 {
        ffi::liella_machine_x87_sw_get() as u16
    }
    fn x87_clear_exceptions(&self) {
        ffi::liella_machine_x87_clex();
    }
    fn x87_store_env(&self) -> X87Env {
        let mut env = X87Env::default();
        ffi::liella_machine_x87_env_store(&mut env);
        env
    }
    fn x87_load_env(&self, env: &X87Env) {
        ffi::liella_machine_x87_env_load(env);
    }
    fn pkru(&self) -> Option<u32> {
        let [_, _, ecx, _] = self.cpuid(7, 0);
        (ecx & (1 << 4) != 0).then(|| ffi::liella_machine_rdpkru())
    }
    fn set_pkru(&self, value: u32) -> bool {
        let [_, _, ecx, _] = self.cpuid(7, 0);
        if ecx & (1 << 4) == 0 {
            return false;
        }
        ffi::liella_machine_wrpkru(value & !3);
        true
    }
    fn barrier(&self, bytes: &[u8]) {
        ffi::liella_machine_barrier(bytes.as_ptr());
    }
}

/// The restorer's address, for handler installation in `raw`.
pub(crate) fn restorer_addr() -> usize {
    ffi::liella_machine_restorer as extern "C" fn() as usize
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, clippy::byte_char_slices)]
mod tests {
    use super::*;

    #[test]
    fn no_vdso_resolves_nothing() {
        assert_eq!(resolve_vdso(0), [0; 5]);
    }
}
