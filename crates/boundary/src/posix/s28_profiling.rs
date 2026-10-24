//! Translation for spec §28: profiling and instrumentation (PROFILING-GMON).
//!
//! The data model is `liella::prof`. `mcount`/`_mcount`/`__fentry__` are assembly
//! (csys/src/machine/mcount.S) that save the argument registers and call
//! `liella_mcount_record(frompc, selfpc)`. Sampling is `ITIMER_PROF` with a `SIGPROF` handler
//! (SA_SIGINFO | SA_RESTART, every signal blocked, as glibc installs it) that reads the
//! interrupted pc from the ucontext. `monstartup` maps the histogram and the arc table once and
//! never unmaps them, so a late `mcount` never touches freed memory. Gap: pcs are written
//! unrebased (right for static and non-PIE programs; a PIE's load bias is not yet subtracted).

use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU16, AtomicU32, AtomicU64, AtomicU8, AtomicUsize, Ordering::{AcqRel, Acquire, Relaxed, Release}};

use liella::prof::{self, Arcs, Text};
use liella_abi::ctypes::{c_int, c_uint, c_void};
use liella_kernel::errno::{Errno, KResult};
use liella_kernel::fd::Fd;
use liella_kernel::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Itimerval, SigSet, Timeval, MAP_ANONYMOUS, MAP_PRIVATE, O_CLOEXEC, O_CREAT, O_NOFOLLOW, O_TRUNC, O_WRONLY, PROT_READ, PROT_WRITE, SA_RESTART, SA_SIGINFO, SIGPROF};
use liella_legacy::signal_compat::CSigaction;

use crate::export;
use crate::ret;
use crate::sys::CSys;

const ITIMER_PROF: u32 = 2;
/// Offset of `uc_mcontext.gregs[REG_RIP]` in the kernel's x86_64 `ucontext_t`.
const UC_RIP: usize = 168;

/// `struct prof` of `<sys/profil.h>` (sprofil's regions).
#[repr(C)]
pub struct Prof {
    base: usize,
    size: usize,
    off: usize,
    scale: u64,
}

/// The sampling state the `SIGPROF` handler reads.
static BUF: AtomicUsize = AtomicUsize::new(0);
static NBINS: AtomicUsize = AtomicUsize::new(0);
static OFFSET: AtomicUsize = AtomicUsize::new(0);
static SCALE: AtomicU64 = AtomicU64::new(0);
static REGIONS: AtomicUsize = AtomicUsize::new(0);
static NREGIONS: AtomicUsize = AtomicUsize::new(0);
static WIDE: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);
static OLD_HANDLER: AtomicUsize = AtomicUsize::new(0);
static OLD_FLAGS: AtomicI32 = AtomicI32::new(0);
static OLD_MASK: AtomicU64 = AtomicU64::new(0);

/// `monstartup`'s tables: none, mapped and off, mapped and on, failed.
static MON: AtomicU8 = AtomicU8::new(MON_NONE);
const MON_NONE: u8 = 0;
const MON_OFF: u8 = 1;
const MON_ON: u8 = 2;
const MON_FAILED: u8 = 3;
static TEXT: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
static HIST: AtomicUsize = AtomicUsize::new(0);
static KEYS: AtomicUsize = AtomicUsize::new(0);
static COUNTS: AtomicUsize = AtomicUsize::new(0);
static SLOTS: AtomicUsize = AtomicUsize::new(0);

// UNSAFE(slice): bump views profil counter addresses the caller handed in as atomics.
#[allow(unsafe_code)]
fn bump(addr: usize, wide: bool) {
    if wide {
        // UNSAFE(slice) SAFETY: addr is a 4-aligned counter inside a caller's sprofil region.
        unsafe { &*core::ptr::with_exposed_provenance::<AtomicU32>(addr) }.fetch_add(1, Relaxed);
    } else {
        // UNSAFE(slice) SAFETY: addr is a 2-aligned counter inside the profil buffer or region.
        unsafe { &*core::ptr::with_exposed_provenance::<AtomicU16>(addr) }.fetch_add(1, Relaxed);
    }
}

#[allow(clippy::as_conversions)] // a function pointer's address: the only spelling Rust has
fn handler_addr() -> usize {
    on_sigprof as extern "C" fn(c_int, *mut c_void, *mut c_void) as usize
}

// UNSAFE(slice): the SIGPROF handler reads the interrupted pc and the caller's sprofil regions.
#[allow(unsafe_code)]
extern "C" fn on_sigprof(_sig: c_int, _info: *mut c_void, ctx: *mut c_void) {
    if ctx.is_null() {
        return;
    }
    // UNSAFE(slice) SAFETY: the kernel passes the interrupted context to an SA_SIGINFO handler.
    let pc = unsafe { ctx.cast::<u8>().wrapping_add(UC_RIP).cast::<usize>().read_unaligned() };
    let n = NREGIONS.load(Acquire);
    if n == 0 {
        if let Some(i) = prof::profil_bin(pc, OFFSET.load(Relaxed), SCALE.load(Relaxed), NBINS.load(Relaxed)) {
            bump(BUF.load(Relaxed).wrapping_add(i.wrapping_mul(2)), false);
        }
        return;
    }
    let wide = WIDE.load(Relaxed);
    let unit = if wide { 4 } else { 2 };
    // UNSAFE(slice) SAFETY: sprofil's caller keeps its n regions alive while profiling runs.
    let regions = unsafe { core::slice::from_raw_parts(core::ptr::with_exposed_provenance::<Prof>(REGIONS.load(Relaxed)), n) };
    for r in regions {
        if let Some(i) = prof::profil_bin(pc, r.off, r.scale, r.size.checked_div(unit).unwrap_or(0)) {
            bump(r.base.wrapping_add(i.wrapping_mul(unit)), wide);
            return;
        }
    }
}

fn timer(usec: i64) -> KResult<()> {
    let t = Timeval { sec: 0, usec };
    CSys.setitimer(ITIMER_PROF, &Itimerval { interval: t, value: t }).map(|_| ())
}

fn sigaction(act: &CSigaction, old: Option<&mut CSigaction>) -> KResult<()> {
    let prev = liella_legacy::signal_compat::sigaction(SIGPROF, Some(act), crate::sys::restorer_addr(), crate::raw::sigaction)?;
    if let Some(o) = old {
        *o = prev;
    }
    Ok(())
}

/// Start sampling with the parameters already stored, or stop (`on == false`).
fn sampling(on: bool) -> KResult<()> {
    if !on {
        if RUNNING.swap(false, AcqRel) {
            timer(0)?;
            let mut act = CSigaction::new(OLD_HANDLER.load(Relaxed), SigSet::empty(), 0);
            act.flags = OLD_FLAGS.load(Relaxed);
            act.mask[0] = OLD_MASK.load(Relaxed);
            sigaction(&act, None)?;
        }
        return Ok(());
    }
    if !RUNNING.load(Acquire) {
        let mut act = CSigaction::new(handler_addr(), SigSet::empty(), SA_SIGINFO | SA_RESTART);
        act.mask = [!0; 16];
        let mut old = CSigaction::new(0, SigSet::empty(), 0);
        sigaction(&act, Some(&mut old))?;
        OLD_HANDLER.store(old.handler, Relaxed);
        OLD_FLAGS.store(old.flags, Relaxed);
        OLD_MASK.store(old.mask[0], Relaxed);
        RUNNING.store(true, Release);
    }
    timer(prof::TICK_USEC)
}

fn profil_set(buf: usize, size: usize, offset: usize, scale: u64) -> KResult<c_int> {
    if buf == 0 {
        sampling(false)?;
        return Ok(0);
    }
    NREGIONS.store(0, Release);
    BUF.store(buf, Relaxed);
    NBINS.store(size / 2, Relaxed);
    OFFSET.store(offset, Relaxed);
    SCALE.store(scale, Release);
    sampling(true).map(|()| 0)
}

export! { "GLIBC_2.2.5";
pub extern "C" fn profil(buf: *mut u16, size: usize, offset: usize, scale: c_uint) -> c_int {
    ret::int32(profil_set(buf.addr(), size, offset, u64::from(scale)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sprofil(regions: *const Prof, count: c_int, tvp: Option<&mut Timeval>, flags: c_uint) -> c_int {
    let n = usize::try_from(count).unwrap_or(0);
    let r = if regions.is_null() || n == 0 {
        sampling(false).map(|()| 0)
    } else {
        WIDE.store(flags & 1 != 0, Relaxed);
        REGIONS.store(regions.addr(), Relaxed);
        NREGIONS.store(n, Release);
        sampling(true).map(|()| 0)
    };
    if let (Ok(_), Some(tv)) = (&r, tvp) {
        *tv = Timeval { sec: 0, usec: prof::TICK_USEC };
    }
    ret::int32(r)
}}

fn text() -> Text {
    Text { low: TEXT[0].load(Relaxed), high: TEXT[1].load(Relaxed), bins: TEXT[2].load(Relaxed), scale: u64::try_from(TEXT[3].load(Relaxed)).unwrap_or(0) }
}

// UNSAFE(slice): arcs views the arc table monstartup mapped, which is never unmapped.
#[allow(unsafe_code)]
fn arcs() -> Arcs<'static> {
    let n = SLOTS.load(Acquire);
    // UNSAFE(slice) SAFETY: monstartup mapped n zeroed key slots and never unmaps them.
    let keys = unsafe { core::slice::from_raw_parts(core::ptr::with_exposed_provenance::<AtomicU64>(KEYS.load(Relaxed)), n) };
    // UNSAFE(slice) SAFETY: monstartup mapped n zeroed count slots and never unmaps them.
    let counts = unsafe { core::slice::from_raw_parts(core::ptr::with_exposed_provenance::<AtomicU32>(COUNTS.load(Relaxed)), n) };
    Arcs::new(keys, counts)
}

export! { "GLIBC_2.2.5";
pub extern "C" fn monstartup(low: usize, high: usize) {
    if MON.compare_exchange(MON_NONE, MON_FAILED, AcqRel, Acquire).is_err() {
        return;
    }
    let Some(t) = Text::new(low, high) else { return };
    let slots = t.arc_slots();
    let hist = t.bins.saturating_mul(2).next_multiple_of(8);
    let total = hist.saturating_add(slots.saturating_mul(12));
    let Ok(region) = CSys.mmap(0, total, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, None, 0) else { return };
    let base = region.addr();
    for (cell, v) in TEXT.iter().zip([t.low, t.high, t.bins, usize::try_from(t.scale).unwrap_or(0)]) {
        cell.store(v, Relaxed);
    }
    HIST.store(base, Relaxed);
    KEYS.store(base.wrapping_add(hist), Relaxed);
    COUNTS.store(base.wrapping_add(hist).wrapping_add(slots.wrapping_mul(8)), Relaxed);
    SLOTS.store(slots, Release);
    MON.store(MON_OFF, Release);
    moncontrol(1);
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __monstartup(low: usize, high: usize) {
    monstartup(low, high)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn moncontrol(mode: c_int) {
    let state = MON.load(Acquire);
    if state != MON_OFF && state != MON_ON {
        return;
    }
    if mode != 0 {
        let t = text();
        if profil_set(HIST.load(Relaxed), t.bins.wrapping_mul(2), t.low, t.scale).is_ok() {
            MON.store(MON_ON, Release);
        }
    } else {
        MON.store(MON_OFF, Release);
        let _ = sampling(false);
    }
}}

export! { local "liella_mcount_record";
/// The body of `mcount`, `_mcount` and `__fentry__` (csys/src/machine/mcount.S): one traversal
/// of the arc from the instrumented function's caller (`from`) to the function (`to`).
pub extern "C" fn mcount_record(from: usize, to: usize) {
    if MON.load(Acquire) == MON_ON {
        arcs().record(&text(), from, to);
    }
}}

/// Every recorded arc as `(from, self, count)`, for the in-process tests of `mcount` and
/// `__fentry__`; nothing before a successful `monstartup`.
pub fn each_arc(f: impl FnMut(usize, usize, u32)) {
    let state = MON.load(Acquire);
    if state == MON_OFF || state == MON_ON {
        arcs().each(&text(), f);
    }
}

/// Test fixture for `mcount` and `__fentry__` (csys/src/machine/mcount.S): the text addresses of
/// an instrumented fentry site, an instrumented mcount site and the caller that calls them.
pub fn probe_sites() -> [usize; 3] {
    let mut out = [0usize; 3];
    crate::ffi::liella_machine_probe_addrs(&mut out);
    out
}

/// Test fixture: call the mcount site (`mcount`) or the fentry site `n` times from the probe caller.
pub fn probe_run(mcount: bool, n: u32) {
    crate::ffi::liella_machine_probe_run(u32::from(mcount), n);
}

/// `gmon.out` or `$GMON_OUT_PREFIX.<pid>` (secure_getenv), NUL-terminated, in `buf`: its length.
fn out_name(buf: &mut [u8; 4096]) -> Option<usize> {
    let mut len = 0usize;
    let mut put = |bytes: &[u8]| -> Option<()> {
        let end = len.checked_add(bytes.len()).filter(|&e| e < 4096)?;
        buf.get_mut(len..end)?.copy_from_slice(bytes);
        len = end;
        Some(())
    };
    match crate::process::secure_getenv(b"GMON_OUT_PREFIX").map(crate::strz::env_value) {
        Some(prefix) => {
            let mut digits = [0u8; 20];
            let mut at = digits.len();
            let mut pid = u64::try_from(CSys.getpid()).unwrap_or(0);
            loop {
                at = at.checked_sub(1)?;
                *digits.get_mut(at)? = b'0'.wrapping_add(u8::try_from(pid % 10).unwrap_or(0));
                pid /= 10;
                if pid == 0 {
                    break;
                }
            }
            put(prefix)?;
            put(b".")?;
            put(digits.get(at..)?)?;
        }
        None => put(b"gmon.out")?,
    }
    put(b"\0")?;
    Some(len)
}

// UNSAFE(slice): write_out views the histogram monstartup mapped, which is never unmapped.
#[allow(unsafe_code)]
fn write_out() -> KResult<()> {
    let mut path = [0u8; 4096];
    let n = out_name(&mut path).ok_or(Errno::ENAMETOOLONG)?;
    let fd = CSys.openat(Fd::CWD, KStr::from_nul_terminated(path.get(..n).unwrap_or(&[]))?, O_WRONLY | O_CREAT | O_TRUNC | O_NOFOLLOW | O_CLOEXEC, 0o666)?;
    let mut out = Out { fd, buf: [0; 4096], len: 0, err: None };
    let t = text();
    out.put(&prof::GMON_HEADER);
    out.put(&prof::hist_record(&t, 0));
    // UNSAFE(slice) SAFETY: monstartup mapped t.bins 16-bit bins at HIST.
    let bins = unsafe { core::slice::from_raw_parts(core::ptr::with_exposed_provenance::<AtomicU16>(HIST.load(Relaxed)), t.bins) };
    for b in bins {
        out.put(&b.load(Relaxed).to_le_bytes());
    }
    arcs().each(&t, |from, to, count| out.put(&prof::arc_record(from, to, count, 0)));
    out.flush();
    let _ = CSys.close(fd);
    out.err.map_or(Ok(()), Err)
}

struct Out {
    fd: Fd,
    buf: [u8; 4096],
    len: usize,
    err: Option<Errno>,
}

impl Out {
    fn put(&mut self, bytes: &[u8]) {
        if self.len.saturating_add(bytes.len()) > self.buf.len() {
            self.flush();
        }
        if let Some(dst) = self.buf.get_mut(self.len..self.len.saturating_add(bytes.len())) {
            dst.copy_from_slice(bytes);
            self.len = self.len.saturating_add(bytes.len());
        }
    }
    fn flush(&mut self) {
        let mut done = 0;
        while done < self.len && self.err.is_none() {
            match CSys.write(self.fd, self.buf.get(done..self.len).unwrap_or(&[])) {
                Ok(0) => self.err = Some(Errno::EIO),
                Ok(k) => done = done.saturating_add(k),
                Err(Errno::EINTR) => {}
                Err(e) => self.err = Some(e),
            }
        }
        self.len = 0;
    }
}

export! { "GLIBC_2.2.5";
pub extern "C" fn _mcleanup() {
    moncontrol(0);
    if MON.compare_exchange(MON_OFF, MON_FAILED, AcqRel, Acquire).is_ok() {
        // Profiling data is best effort at exit: an unwritable file is not reported (D-28.1).
        let _ = write_out();
    }
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __profile_frequency() -> c_int {
    prof::PROFILE_HZ
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __cyg_profile_func_enter(_this_fn: *mut c_void, _call_site: *mut c_void) {}
}

export! { "GLIBC_2.2.5";
pub extern "C" fn __cyg_profile_func_exit(_this_fn: *mut c_void, _call_site: *mut c_void) {}
}
