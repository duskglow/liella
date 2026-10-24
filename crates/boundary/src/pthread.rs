//! The §22 exports' calls (spec §22.1–§22.3, §22.6, §22.8): each export converts its C
//! arguments and makes one call here. This module joins the attribute object
//! (`liella_legacy::pthread_compat`), the thread runtime (`crate::thread`), the heap (affinity
//! copies) and the kernel.

use core::sync::atomic::{AtomicUsize, Ordering::{Acquire, Relaxed, Release}};

use liella_abi::ctypes::c_int;
use liella_abi::layout::{pthread_t, struct_sched_param};
use liella_kernel::errno::{Errno, KResult};
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{SchedParam, SigSet, Timespec, O_CLOEXEC, O_RDONLY, O_WRONLY};
use liella_legacy::pthread_compat::{self as pc, AttrWords, ATTR_WORDS};
use liella_rt::futex::Clock;
use liella_rt::thread::{Attr, PAGE};

use crate::heap;
use crate::sys::CSys;
use crate::thread::{self, JoinWait};
use crate::{conv, tb};

// ------------------------------------------------------------------------------ attributes

/// The process-wide default attributes (`pthread_setattr_default_np`).
static DEFAULT_ATTR: [AtomicUsize; ATTR_WORDS] = [const { AtomicUsize::new(0) }; ATTR_WORDS];
static CONCURRENCY: AtomicUsize = AtomicUsize::new(0);

fn words(w: KResult<&[AtomicUsize]>) -> KResult<AttrWords<'_>> {
    w.map(AttrWords::new)
}

/// The affinity copy of an attribute object: (address of the mask, its length).
fn affinity_of(a: &AttrWords<'_>) -> Option<(usize, usize)> {
    match a.affinity_block() {
        0 => None,
        block => {
            let len = tb::words64(block, 1).first().map_or(0, |w| w.load(Relaxed));
            Some((block.wrapping_add(8), len))
        }
    }
}

fn free_block(block: usize) {
    if block != 0 {
        heap::checked("free", liella_legacy::malloc_compat::free(&heap::CTX, block));
    }
}

/// A heap copy of `mask`: its length, then the bytes.
fn copy_mask(mask: &[u8]) -> KResult<usize> {
    let block = liella_legacy::malloc_compat::malloc(&heap::CTX, mask.len().saturating_add(8)).map_err(|_| Errno::ENOMEM)?;
    if let Some(w) = tb::words64(block, 1).first() {
        w.store(mask.len(), Relaxed);
    }
    conv::copy_mapped(core::ptr::from_ref(mask).cast::<u8>().expose_provenance(), block.wrapping_add(8), mask.len());
    Ok(block)
}

/// `pthread_attr_init`.
pub fn attr_init(w: KResult<&[AtomicUsize]>) -> KResult<()> {
    words(w)?.init();
    Ok(())
}

/// `pthread_attr_destroy`: frees the affinity copy.
pub fn attr_destroy(w: KResult<&[AtomicUsize]>) -> KResult<()> {
    let a = words(w)?;
    free_block(a.swap_affinity(0));
    Ok(())
}

/// `pthread_attr_setdetachstate`.
pub fn attr_setdetachstate(w: KResult<&[AtomicUsize]>, v: c_int) -> KResult<()> {
    words(w)?.set_detachstate(v)
}

/// `pthread_attr_getdetachstate`.
pub fn attr_getdetachstate(w: KResult<&[AtomicUsize]>) -> KResult<c_int> {
    Ok(words(w)?.detachstate())
}

/// `pthread_attr_setstacksize`.
pub fn attr_setstacksize(w: KResult<&[AtomicUsize]>, n: usize) -> KResult<()> {
    words(w)?.set_stacksize(n)
}

/// `pthread_attr_getstacksize`.
pub fn attr_getstacksize(w: KResult<&[AtomicUsize]>) -> KResult<usize> {
    Ok(words(w)?.stacksize(thread::default_stack()))
}

/// `pthread_attr_setstack`.
pub fn attr_setstack(w: KResult<&[AtomicUsize]>, addr: usize, size: usize) -> KResult<()> {
    words(w)?.set_stack(addr, size)
}

/// `pthread_attr_getstack`: (lowest address, size).
pub fn attr_getstack(w: KResult<&[AtomicUsize]>) -> KResult<(usize, usize)> {
    Ok(words(w)?.stack(thread::default_stack()))
}

/// `pthread_attr_setstackaddr`.
pub fn attr_setstackaddr(w: KResult<&[AtomicUsize]>, top: usize) -> KResult<()> {
    words(w)?.set_stackaddr(top);
    Ok(())
}

/// `pthread_attr_getstackaddr`.
pub fn attr_getstackaddr(w: KResult<&[AtomicUsize]>) -> KResult<*mut core::ffi::c_void> {
    Ok(core::ptr::with_exposed_provenance_mut(words(w)?.stackaddr()))
}

/// `pthread_attr_getstack` with its two out-parameters.
pub fn attr_getstack_out(w: KResult<&[AtomicUsize]>, addr: Option<&mut *mut core::ffi::c_void>, size: Option<&mut usize>) -> c_int {
    let r = attr_getstack(w).and_then(|(lo, n)| match (addr, size) {
        (Some(a), Some(s)) => {
            *a = core::ptr::with_exposed_provenance_mut(lo);
            *s = n;
            Ok(())
        }
        _ => Err(Errno::EINVAL),
    });
    r.err().map_or(0, Errno::get)
}

/// `pthread_attr_setguardsize`.
pub fn attr_setguardsize(w: KResult<&[AtomicUsize]>, n: usize) -> KResult<()> {
    words(w)?.set_guardsize(n);
    Ok(())
}

/// `pthread_attr_getguardsize`.
pub fn attr_getguardsize(w: KResult<&[AtomicUsize]>) -> KResult<usize> {
    Ok(words(w)?.guardsize(thread::DEFAULT_GUARD.load(Relaxed)))
}

fn priority_range(policy: c_int) -> KResult<(i32, i32)> {
    let p = u32::try_from(policy).map_err(|_| Errno::EINVAL)?;
    Ok((CSys.sched_get_priority_min(p)?, CSys.sched_get_priority_max(p)?))
}

/// `pthread_attr_setschedparam`.
pub fn attr_setschedparam(w: KResult<&[AtomicUsize]>, param: Option<&struct_sched_param>) -> KResult<()> {
    let a = words(w)?;
    let prio = param.ok_or(Errno::EINVAL)?.sched_priority;
    a.set_priority(prio, priority_range(a.policy())?)
}

/// `pthread_attr_getschedparam`.
pub fn attr_getschedparam(w: KResult<&[AtomicUsize]>) -> KResult<struct_sched_param> {
    Ok(struct_sched_param { sched_priority: words(w)?.priority() })
}

/// `pthread_attr_setschedpolicy`.
pub fn attr_setschedpolicy(w: KResult<&[AtomicUsize]>, p: c_int) -> KResult<()> {
    words(w)?.set_policy(p)
}

/// `pthread_attr_getschedpolicy`.
pub fn attr_getschedpolicy(w: KResult<&[AtomicUsize]>) -> KResult<c_int> {
    Ok(words(w)?.policy())
}

/// `pthread_attr_setinheritsched`.
pub fn attr_setinheritsched(w: KResult<&[AtomicUsize]>, v: c_int) -> KResult<()> {
    words(w)?.set_inherit(v)
}

/// `pthread_attr_getinheritsched`.
pub fn attr_getinheritsched(w: KResult<&[AtomicUsize]>) -> KResult<c_int> {
    Ok(words(w)?.inherit())
}

/// `pthread_attr_setscope`.
pub fn attr_setscope(w: KResult<&[AtomicUsize]>, v: c_int) -> KResult<()> {
    words(w)?;
    pc::check_scope(v)
}

/// `pthread_attr_getscope`: always `PTHREAD_SCOPE_SYSTEM`.
pub fn attr_getscope(w: KResult<&[AtomicUsize]>) -> KResult<c_int> {
    words(w)?;
    Ok(pc::SCOPE_SYSTEM)
}

/// `pthread_attr_setaffinity_np`: a NULL mask or size 0 removes the affinity.
pub fn attr_setaffinity(w: KResult<&[AtomicUsize]>, mask: KResult<&[u8]>) -> KResult<()> {
    let a = words(w)?;
    let mask = mask.unwrap_or(&[]);
    let block = if mask.is_empty() { 0 } else { copy_mask(mask)? };
    free_block(a.swap_affinity(block));
    Ok(())
}

/// `pthread_attr_getaffinity_np`: the stored mask (zero-padded), or every CPU when none was
/// set; `EINVAL` if the stored mask has bits beyond `out`.
pub fn attr_getaffinity(w: KResult<&[AtomicUsize]>, out: KResult<&mut [u8]>) -> KResult<()> {
    let a = words(w)?;
    let out = out?;
    match affinity_of(&a) {
        None => conv::fill(out, 0xff),
        Some((addr, len)) => {
            let stored = conv::mapped_bytes(addr, len);
            if stored.get(out.len()..).is_some_and(|rest| rest.iter().any(|&b| b != 0)) {
                return Err(Errno::EINVAL);
            }
            conv::fill(out, 0);
            let _ = conv::copy_units(out, stored);
        }
    }
    Ok(())
}

/// `pthread_attr_setsigmask_np`: NULL removes the mask.
pub fn attr_setsigmask(w: KResult<&[AtomicUsize]>, set: Option<&liella_abi::layout::sigset_t>) -> KResult<()> {
    words(w)?.set_sigmask(conv::sigmask(set).map(|s| s.0));
    Ok(())
}

/// `pthread_attr_getsigmask_np`: 0 with the mask, `PTHREAD_ATTR_NO_SIGMASK_NP` with an empty
/// set when none was set, or an error number.
pub fn attr_getsigmask(w: KResult<&[AtomicUsize]>, out: Option<&mut liella_abi::layout::sigset_t>) -> c_int {
    let Ok(a) = words(w) else { return Errno::EINVAL.get() };
    let Some(out) = out else { return Errno::EINVAL.get() };
    let mask = a.sigmask();
    *out = liella_legacy::signal_compat::c_set(SigSet(mask.unwrap_or(0)));
    if mask.is_some() { 0 } else { pc::NO_SIGMASK_NP }
}

/// `pthread_getattr_default_np`.
pub fn getattr_default(w: KResult<&[AtomicUsize]>) -> KResult<()> {
    let w = w?;
    let d = AttrWords::new(&DEFAULT_ATTR);
    for (i, slot) in w.iter().enumerate().take(ATTR_WORDS) {
        slot.store(DEFAULT_ATTR.get(i).map_or(0, |x| x.load(Acquire)), Relaxed);
    }
    let a = AttrWords::new(w);
    a.swap_affinity(0);
    if let Some((addr, len)) = affinity_of(&d) {
        a.swap_affinity(copy_mask(conv::mapped_bytes(addr, len))?);
    }
    a.set_stacksize(d.stacksize(thread::default_stack()))?;
    a.set_guardsize(d.guardsize(PAGE));
    Ok(())
}

/// `pthread_setattr_default_np`: a stack address is `EINVAL`.
pub fn setattr_default(w: KResult<&[AtomicUsize]>) -> KResult<()> {
    let w = w?;
    let a = AttrWords::new(w);
    if a.stackaddr() != 0 {
        return Err(Errno::EINVAL);
    }
    let copy = affinity_of(&a).map(|(addr, len)| copy_mask(conv::mapped_bytes(addr, len))).transpose()?;
    let d = AttrWords::new(&DEFAULT_ATTR);
    let old = d.swap_affinity(0);
    for (i, slot) in DEFAULT_ATTR.iter().enumerate() {
        slot.store(w.get(i).map_or(0, |x| x.load(Relaxed)), Release);
    }
    d.swap_affinity(copy.unwrap_or(0));
    free_block(old);
    let size = a.stacksize(0);
    if size != 0 {
        thread::DEFAULT_STACK.store(size, Release);
    }
    thread::DEFAULT_GUARD.store(a.guardsize(PAGE), Release);
    Ok(())
}

/// `pthread_getconcurrency`: the hint last set (XSI; no effect, ⚑ HIST).
pub fn getconcurrency() -> c_int {
    c_int::try_from(CONCURRENCY.load(Relaxed)).unwrap_or(0)
}

/// `pthread_setconcurrency`: `EINVAL` for a negative hint.
pub fn setconcurrency(n: c_int) -> KResult<()> {
    CONCURRENCY.store(usize::try_from(n).map_err(|_| Errno::EINVAL)?, Relaxed);
    Ok(())
}

/// `pthread_getattr_np`: the running thread's actual attributes into a fresh attribute
/// object (the caller destroys it).
pub fn getattr_np(t: pthread_t, w: KResult<&[AtomicUsize]>) -> KResult<()> {
    let tp = usize::try_from(t.0).map_err(|_| Errno::ESRCH)?;
    let d = thread::find(tp).ok_or(Errno::ESRCH)?;
    let tid = thread::tid_of(tp)?;
    let a = words(w)?;
    a.init();
    if d.join.load(Acquire) == liella_rt::thread::DETACHED {
        a.set_detachstate(pc::CREATE_DETACHED)?;
    }
    let (lo, size, guard) = match d.stack_size.load(Acquire) {
        0 => {
            // The initial thread (or one the host C library made): from the maps (§22.2).
            let here = core::ptr::from_ref(&tid).expose_provenance();
            // Without `/proc` (OD-5) the stack cannot be found: the read's error (ENOENT).
            let maps = liella::system::read_file(&CSys, b"/proc/self/maps\0")?;
            let cur = CSys.prlimit(0, liella_kernel::uapi::RLIMIT_STACK, None).map_or(u64::MAX, |r| r.cur);
            let (top, size) = pc::main_stack(&maps, here, cur).ok_or(Errno::ENOENT)?;
            (top.saturating_sub(size), size, 0)
        }
        size => (d.stack_lo.load(Acquire), size, d.guard.load(Acquire)),
    };
    a.set_stack(lo, size.max(liella_rt::thread::STACK_MIN))?;
    a.set_guardsize(guard);
    let policy = CSys.sched_getscheduler(tid)?;
    let param = CSys.sched_getparam(tid)?;
    a.set_policy(c_int::try_from(policy & 0xff).unwrap_or(0)).ok();
    a.set_priority(param.priority, (param.priority, param.priority))?;
    let mut mask = [0u8; 128];
    let n = CSys.sched_getaffinity(tid, &mut mask)?;
    a.swap_affinity(copy_mask(mask.get(..n).unwrap_or(&mask))?);
    Ok(())
}

/// A private copy of an attribute object, `None` copying the process-wide defaults (a
/// `SIGEV_THREAD` timer keeps its `sigev_notify_attributes` this way, §23.4, since the caller
/// may destroy them after `timer_create`). The affinity mask is copied too; `ENOMEM` without
/// memory for it. [`attr_release`] frees the copy's mask.
pub(crate) fn attr_copy(src: Option<&[AtomicUsize]>, dst: &[AtomicUsize]) -> KResult<()> {
    for (d, s) in dst.iter().zip(src.unwrap_or(&DEFAULT_ATTR)) {
        d.store(s.load(Relaxed), Relaxed);
    }
    let a = AttrWords::new(dst);
    let theirs = affinity_of(&a);
    a.swap_affinity(0);
    if let Some((mask, len)) = theirs {
        a.swap_affinity(copy_mask(conv::mapped_bytes(mask, len))?);
    }
    Ok(())
}

/// Release what [`attr_copy`] allocated.
pub(crate) fn attr_release(w: &[AtomicUsize]) {
    free_block(AttrWords::new(w).swap_affinity(0));
}

/// The thread attributes an attribute object (or an [`attr_copy`]) describes.
pub(crate) fn attr_decode(w: &[AtomicUsize]) -> Attr {
    let a = AttrWords::new(w);
    a.decode(affinity_of(&a))
}

// ------------------------------------------------------------------------------ lifecycle

fn attr_for(w: KResult<Option<&[AtomicUsize]>>) -> KResult<Attr> {
    let w = w?;
    let a = AttrWords::new(w.unwrap_or(&DEFAULT_ATTR));
    Ok(a.decode(affinity_of(&a)))
}

/// `pthread_create`.
pub fn create(attr: KResult<Option<&[AtomicUsize]>>, start: usize, arg: usize) -> KResult<pthread_t> {
    if start == 0 {
        return Err(Errno::EINVAL);
    }
    let tp = thread::create(&attr_for(attr)?, start, arg, false)?;
    Ok(pthread_t(u64::try_from(tp).unwrap_or(0)))
}

/// `pthread_self`.
pub fn self_id() -> pthread_t {
    pthread_t(u64::try_from(tb::tp()).unwrap_or(0))
}

/// `pthread_equal`.
pub fn equal(a: pthread_t, b: pthread_t) -> c_int {
    c_int::from(a.0 == b.0)
}

fn tp_of(t: pthread_t) -> KResult<usize> {
    usize::try_from(t.0).map_err(|_| Errno::ESRCH)
}

/// `pthread_join` and its forms: the result as a pointer value.
pub fn join(t: pthread_t, how: JoinWait<'_>) -> KResult<*mut core::ffi::c_void> {
    let r = thread::join(tp_of(t)?, how)?;
    Ok(core::ptr::with_exposed_provenance_mut(r))
}

/// `pthread_join`.
pub fn join_block(t: pthread_t) -> KResult<*mut core::ffi::c_void> {
    join(t, JoinWait::Block)
}

/// `pthread_tryjoin_np`.
pub fn join_try(t: pthread_t) -> KResult<*mut core::ffi::c_void> {
    join(t, JoinWait::Try)
}

/// `pthread_timedjoin_np` (`CLOCK_REALTIME`); NULL waits without limit.
pub fn join_timed(t: pthread_t, at: Option<&Timespec>) -> KResult<*mut core::ffi::c_void> {
    join_clock(t, 0, at)
}

/// `pthread_clockjoin_np`: `CLOCK_REALTIME` or `CLOCK_MONOTONIC`.
pub fn join_clock(t: pthread_t, clock: c_int, at: Option<&Timespec>) -> KResult<*mut core::ffi::c_void> {
    let c = clock_of(clock)?;
    match at {
        None => join(t, JoinWait::Block),
        Some(ts) => {
            if ts.nsec < 0 || ts.nsec >= 1_000_000_000 {
                return Err(Errno::EINVAL);
            }
            join(t, JoinWait::Until(c, ts))
        }
    }
}

/// The clock of a timed wait (§22.4–§22.5).
pub fn clock_of(clock: c_int) -> KResult<Clock> {
    match clock {
        0 => Ok(Clock::Realtime),
        1 => Ok(Clock::Monotonic),
        _ => Err(Errno::EINVAL),
    }
}

/// `pthread_detach`.
pub fn detach(t: pthread_t) -> KResult<()> {
    thread::detach(tp_of(t)?)
}

/// `pthread_exit`: the cleanup buffers, then the thread's exit sequence.
pub fn exit(v: usize) -> ! {
    crate::cancel::unwind(v)
}

// ------------------------------------------------------------------------------ C11 threads

/// `thrd_create`.
pub fn thrd_create(out: Option<&mut liella_abi::layout::thrd_t>, start: usize, arg: usize) -> c_int {
    let r = (|| {
        let out = out.ok_or(Errno::EINVAL)?;
        if start == 0 {
            return Err(Errno::EINVAL);
        }
        let tp = thread::create(&attr_for(Ok(None))?, start, arg, true)?;
        *out = liella_abi::layout::thrd_t(u64::try_from(tp).unwrap_or(0));
        Ok(())
    })();
    pc::c11_create_code(r)
}

/// `thrd_join`: the `int` result.
pub fn thrd_join(t: liella_abi::layout::thrd_t, res: Option<&mut c_int>) -> c_int {
    let r = thread::join(usize::try_from(t.0).unwrap_or(0), JoinWait::Block).map(|v| {
        if let Some(res) = res {
            // A C11 start routine returns `int`: the low 32 bits.
            *res = c_int::from_ne_bytes(u32::try_from(v & 0xffff_ffff).unwrap_or(0).to_ne_bytes());
        }
    });
    pc::c11_code(r)
}

/// `thrd_detach`.
pub fn thrd_detach(t: liella_abi::layout::thrd_t) -> c_int {
    pc::c11_code(thread::detach(usize::try_from(t.0).unwrap_or(0)))
}

/// `thrd_exit`.
pub fn thrd_exit(res: c_int) -> ! {
    crate::cancel::unwind(usize::from_ne_bytes(i64::from(res).to_ne_bytes()))
}

/// `thrd_current`.
pub fn thrd_current() -> liella_abi::layout::thrd_t {
    liella_abi::layout::thrd_t(self_id().0)
}

/// `thrd_equal`.
pub fn thrd_equal(a: liella_abi::layout::thrd_t, b: liella_abi::layout::thrd_t) -> c_int {
    c_int::from(a.0 == b.0)
}

/// `thrd_sleep`: 0, -1 if interrupted (the rest stored), another negative value on error.
pub fn thrd_sleep(d: Option<&Timespec>, rem: Option<&mut Timespec>) -> c_int {
    let Some(d) = d else { return -2 };
    match CSys.clock_nanosleep(0, 0, d, rem) {
        Ok(()) => 0,
        Err(Errno::EINTR) => -1,
        Err(_) => -2,
    }
}

/// `pthread_yield`: 0.
pub fn yield_now() -> c_int {
    CSys.sched_yield();
    0
}

/// `thrd_yield`.
pub fn thrd_yield() {
    CSys.sched_yield();
}

/// `pthread_kill_other_threads_np`: a no-op kept for binary compatibility (⚑ COMPAT).
pub fn no_op() {}

// ------------------------------------------------------------------------------ keys

/// `pthread_key_create`.
pub fn key_create(dtor: usize) -> KResult<liella_abi::layout::pthread_key_t> {
    let k = thread::KEYS.create(dtor).ok_or(Errno::EAGAIN)?;
    Ok(liella_abi::layout::pthread_key_t(u32::try_from(k).map_err(|_| Errno::EAGAIN)?))
}

/// `pthread_key_delete`.
pub fn key_delete(k: liella_abi::layout::pthread_key_t) -> KResult<()> {
    if thread::KEYS.delete(usize::try_from(k.0).unwrap_or(usize::MAX)) { Ok(()) } else { Err(Errno::EINVAL) }
}

/// `pthread_getspecific`: NULL for no value (or no memory for the thread's area).
pub fn getspecific(k: liella_abi::layout::pthread_key_t) -> *mut core::ffi::c_void {
    let v = thread::specific().map_or(0, |s| s.get(&thread::KEYS, usize::try_from(k.0).unwrap_or(usize::MAX)));
    core::ptr::with_exposed_provenance_mut(v)
}

/// `pthread_setspecific`.
pub fn setspecific(k: liella_abi::layout::pthread_key_t, v: usize) -> KResult<()> {
    let s = thread::specific()?;
    if s.set(&thread::KEYS, usize::try_from(k.0).unwrap_or(usize::MAX), v) { Ok(()) } else { Err(Errno::EINVAL) }
}

/// `tss_create`.
pub fn tss_create(out: Option<&mut liella_abi::layout::tss_t>, dtor: usize) -> c_int {
    let r = out.ok_or(Errno::EINVAL).and_then(|o| key_create(dtor).map(|k| *o = liella_abi::layout::tss_t(k.0)));
    pc::c11_code(r)
}

/// `tss_set`.
pub fn tss_set(k: liella_abi::layout::tss_t, v: usize) -> c_int {
    pc::c11_code(setspecific(liella_abi::layout::pthread_key_t(k.0), v))
}

/// `tss_get`.
pub fn tss_get(k: liella_abi::layout::tss_t) -> *mut core::ffi::c_void {
    getspecific(liella_abi::layout::pthread_key_t(k.0))
}

/// `tss_delete`.
pub fn tss_delete(k: liella_abi::layout::tss_t) {
    let _ = key_delete(liella_abi::layout::pthread_key_t(k.0));
}

// ------------------------------------------------------------------------------ naming, scheduling

/// `pthread_setname_np`: at most 15 bytes (`ERANGE`).
pub fn setname(t: pthread_t, name: KResult<&[u8]>) -> KResult<()> {
    let comm = pc::comm(name?)?;
    let tp = tp_of(t)?;
    if tp == tb::tp() {
        return CSys.prctl_set_name(&comm);
    }
    let tid = thread::tid_of(tp)?;
    let path = pc::comm_path(tid);
    let fd = CSys.openat(Fd::CWD, KStr::from_nul_terminated(path.get(..=path.iter().position(|&b| b == 0).unwrap_or(0)).unwrap_or(&[]))?, O_WRONLY | O_CLOEXEC, 0)?;
    let len = comm.iter().position(|&b| b == 0).unwrap_or(16);
    let r = CSys.write(fd, comm.get(..len).unwrap_or(&[]));
    let _ = CSys.close(fd);
    r.map(drop)
}

/// `pthread_getname_np`: `ERANGE` for a buffer under 16 bytes.
pub fn getname(t: pthread_t, out: KResult<&mut [u8]>) -> KResult<()> {
    let out = out?;
    if out.len() < 16 {
        return Err(Errno::ERANGE);
    }
    let tp = tp_of(t)?;
    let name = if tp == tb::tp() {
        CSys.prctl_get_name()?
    } else {
        let tid = thread::tid_of(tp)?;
        let path = pc::comm_path(tid);
        let fd = CSys.openat(Fd::CWD, KStr::from_nul_terminated(path.get(..=path.iter().position(|&b| b == 0).unwrap_or(0)).unwrap_or(&[]))?, O_RDONLY | O_CLOEXEC, 0)?;
        let mut buf = [0u8; 16];
        let r = CSys.read(fd, &mut buf);
        let _ = CSys.close(fd);
        let n = r?;
        let mut name = [0u8; 16];
        let text = buf.get(..n).unwrap_or(&[]);
        let text = text.strip_suffix(b"\n").unwrap_or(text);
        let _ = conv::copy_units(&mut name[..15], text);
        name
    };
    conv::fill(out, 0);
    let len = name.iter().position(|&b| b == 0).unwrap_or(15);
    let _ = conv::copy_units(out, name.get(..len).unwrap_or(&[]));
    Ok(())
}

/// `pthread_setschedparam`.
pub fn setschedparam(t: pthread_t, policy: c_int, param: Option<&struct_sched_param>) -> KResult<()> {
    let tid = thread::tid_of(tp_of(t)?)?;
    let p = param.ok_or(Errno::EINVAL)?;
    CSys.sched_setscheduler(tid, u32::try_from(policy).map_err(|_| Errno::EINVAL)?, &SchedParam { priority: p.sched_priority })
}

/// `pthread_getschedparam`: (policy, parameters).
pub fn getschedparam(t: pthread_t) -> KResult<(c_int, struct_sched_param)> {
    let tid = thread::tid_of(tp_of(t)?)?;
    let policy = CSys.sched_getscheduler(tid)?;
    let param = CSys.sched_getparam(tid)?;
    Ok((c_int::try_from(policy & 0x3fff_ffff).unwrap_or(0), struct_sched_param { sched_priority: param.priority }))
}

/// `pthread_getschedparam` with its two out-parameters.
pub fn getschedparam_out(t: pthread_t, policy: Option<&mut c_int>, param: Option<&mut struct_sched_param>) -> c_int {
    let r = getschedparam(t).and_then(|(p, q)| match (policy, param) {
        (Some(a), Some(b)) => {
            *a = p;
            *b = q;
            Ok(())
        }
        _ => Err(Errno::EINVAL),
    });
    r.err().map_or(0, Errno::get)
}

/// `pthread_setschedprio`.
pub fn setschedprio(t: pthread_t, prio: c_int) -> KResult<()> {
    let tid = thread::tid_of(tp_of(t)?)?;
    CSys.sched_setparam(tid, &SchedParam { priority: prio })
}

/// `pthread_setaffinity_np`.
pub fn setaffinity(t: pthread_t, mask: KResult<&[u8]>) -> KResult<()> {
    let tid = thread::tid_of(tp_of(t)?)?;
    CSys.sched_setaffinity(tid, mask?)
}

/// `pthread_getaffinity_np`: the rest of the caller's mask is cleared.
pub fn getaffinity(t: pthread_t, out: KResult<&mut [u8]>) -> KResult<()> {
    let tid = thread::tid_of(tp_of(t)?)?;
    let out = out?;
    let n = CSys.sched_getaffinity(tid, out)?;
    if let Some(rest) = out.get_mut(n..) {
        conv::fill(rest, 0);
    }
    Ok(())
}

/// `pthread_getcpuclockid`.
pub fn getcpuclockid(t: pthread_t) -> KResult<c_int> {
    Ok(pc::thread_cpuclock(thread::tid_of(tp_of(t)?)?))
}

/// `gettid`: the thread block's word in a thread Liella runs (D-22.9).
pub fn gettid() -> c_int {
    thread::own_tid()
}

// ------------------------------------------------------------------------------ cancellation, signals

/// `pthread_cancel`.
pub fn cancel(t: pthread_t) -> KResult<()> {
    crate::cancel::request(tp_of(t)?)
}

/// `pthread_kill`.
pub fn kill(t: pthread_t, sig: c_int) -> KResult<()> {
    crate::cancel::kill(tp_of(t)?, sig)
}

/// `pthread_sigqueue` (the value as `union sigval`, one word).
pub fn sigqueue(t: pthread_t, sig: c_int, value: usize) -> KResult<()> {
    crate::cancel::sigqueue(tp_of(t)?, sig, u64::try_from(value).unwrap_or(0))
}
