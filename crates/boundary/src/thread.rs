//! Threads: creation, the start and exit sequences, joining and detaching, and the thread list
//! (spec §22.1, §22.2, §22.6, §22.9; D-22.9, D-22.10). The decisions (layout, join and
//! cancellation states, keys) are `liella_rt::thread`'s; this module maps memory, starts the
//! thread through the C layer and keeps the process-wide lists.
//!
//! **Reclaiming a thread's mapping.** A mapping may be reused or unmapped only once the kernel
//! has cleared the thread's TID word (`CLONE_CHILD_CLEARTID`): until then the thread may still
//! be running on its stack. Exactly one party reclaims (the descriptor's token): the joiner;
//! for a detached thread, the thread itself as it exits (it queues the mapping on the dead
//! list, swept at the next creation); or a detacher that finds the thread already exited. A
//! *kept* thread ([`create_kept`]) is detached for the application, but its creator holds the
//! token and reclaims it ([`release_kept`]), so the creator can tell when the thread has left
//! its stack (a `SIGEV_THREAD` notification on a caller-supplied stack, `crate::stacks`).

#![allow(clippy::arithmetic_side_effects)] // offsets inside one mapping (Layout checked them)

use core::sync::atomic::{AtomicBool, AtomicU8, AtomicUsize, Ordering::{AcqRel, Acquire, Relaxed, Release}};

use liella_kernel::errno::{Errno, KResult};
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{
    SigSet, Timespec, CLONE_CHILD_CLEARTID, CLONE_FILES, CLONE_FS, CLONE_PARENT_SETTID, CLONE_SETTLS, CLONE_SIGHAND, CLONE_SYSVSEM, CLONE_THREAD,
    CLONE_VM, MADV_NOHUGEPAGE, MAP_ANONYMOUS, MAP_NORESERVE, MAP_PRIVATE, MAP_STACK, PROT_NONE, PROT_READ, PROT_WRITE, RLIMIT_STACK, RSEQ_SIG, SIG_SETMASK,
};
use liella_rt::futex::{self, Clock, Scope};
use liella_rt::sync::RawMutex;
use liella_rt::thread::{
    self as rt, KeyTable, Layout, Specific, Thread, DESC_ADOPTED, DESC_C11, DESC_CREATED, DESC_EXITED, DESC_RSEQ, DESC_USER_STACK,
    JOINABLE, KEY_AREA, PAGE, SLOTS, SLOT_AREA, SLOT_SIZE,
};
use liella_rt::tls::{TlsImage, FLAG_OWN};

use crate::sys::CSys;
use crate::{conv, ffi, raw, tb};

/// The process-wide key table (§22.6).
pub(crate) static KEYS: KeyTable = KeyTable::new();

/// `__libc_single_threaded` (§22.1): non-zero until the first thread is created (P5).
use crate::data::__libc_single_threaded as SINGLE_THREADED;

static LIST: RawMutex = RawMutex::new();
/// The newest listed descriptor (linked through `next`/`prev`).
static HEAD: AtomicUsize = AtomicUsize::new(0);
/// Threads running. In a process Liella started the initial thread counts, and the last one to
/// exit ends the process with status 0 (§22.1).
static LIVE: AtomicUsize = AtomicUsize::new(0);
static OWNED: AtomicBool = AtomicBool::new(false);
/// MUTEX-2: [`alone`] in one byte for the mutex fast paths: set when the runtime takes the
/// initial thread (so Liella creates every thread), cleared for good before the first thread is
/// cloned (not set again in a fork child: sticky, as the probes show glibc's single-thread
/// mode is). Private, unlike `__libc_single_threaded`, which a program can write.
static LOCK_ALONE: AtomicBool = AtomicBool::new(false);
/// Thread creation is unavailable (a dynamically linked program under another `ld.so`,
/// D-22.10).
static FORBIDDEN: AtomicBool = AtomicBool::new(false);
/// Liella owns the threads' rseq registration (D-22.7).
pub(crate) static RSEQ_OWNED: AtomicBool = AtomicBool::new(false);
/// The executable's TLS segment: address, file size, memory size, alignment (all 0: none).
static TLS_IMAGE: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];
/// Process-wide defaults (`pthread_setattr_default_np`): 0 means "not decided yet".
pub(crate) static DEFAULT_STACK: AtomicUsize = AtomicUsize::new(0);
/// The default guard size.
pub(crate) static DEFAULT_GUARD: AtomicUsize = AtomicUsize::new(PAGE);

static RECYCLE: RawMutex = RawMutex::new();
/// Mappings kept for reuse (descriptor addresses, linked through `next`), and their bytes.
static CACHE: AtomicUsize = AtomicUsize::new(0);
static CACHE_BYTES: AtomicUsize = AtomicUsize::new(0);
/// Exited detached threads whose TID word may not be clear yet.
static DEAD: AtomicUsize = AtomicUsize::new(0);
/// The stack cache's limit (glibc's figure, D-22.9).
const CACHE_MAX: usize = 40 << 20;

const CLONE_FLAGS: u64 =
    CLONE_VM | CLONE_FS | CLONE_FILES | CLONE_SIGHAND | CLONE_THREAD | CLONE_SYSVSEM | CLONE_SETTLS | CLONE_PARENT_SETTID | CLONE_CHILD_CLEARTID;

pub use liella_rt::thread::Attr;

// ------------------------------------------------------------------------------ process state

/// Record the executable's TLS segment (the products' startup).
#[cfg_attr(not(feature = "runtime"), allow(dead_code))]
pub(crate) fn set_tls_image(img: Option<TlsImage>) {
    let w = img.map_or([0; 4], |t| [t.addr, t.filesz, t.memsz, t.align]);
    for (slot, v) in TLS_IMAGE.iter().zip(w) {
        slot.store(v, Release);
    }
}

fn tls_image() -> Option<TlsImage> {
    let w: [usize; 4] = core::array::from_fn(|i| TLS_IMAGE.get(i).map_or(0, |a| a.load(Acquire)));
    (w[2] != 0).then_some(TlsImage { addr: w[0], filesz: w[1], memsz: w[2], align: w[3] })
}

/// A dynamically linked program under another `ld.so`: no thread creation (D-22.10).
#[cfg_attr(not(feature = "runtime"), allow(dead_code))]
pub(crate) fn forbid() {
    FORBIDDEN.store(true, Release);
}

/// Thread creations refused by [`forbid`] (`selftest` only: a test waits for them).
#[cfg(feature = "selftest")]
static REFUSED: AtomicUsize = AtomicUsize::new(0);

/// While set, a creation [`forbid`] refuses waits, after counting the refusal, until it is
/// cleared (`selftest` only): a test can then act while the creating thread is inside the
/// refused attempt (for a timer's helper: with the expiry read and the lock held).
#[cfg(feature = "selftest")]
static HOLD: AtomicBool = AtomicBool::new(false);

/// Hold refused creations ([`HOLD`]) or let them return (`selftest` only).
#[cfg(feature = "selftest")]
pub(crate) fn hold_refusals(on: bool) {
    HOLD.store(on, Release);
}

/// Thread creation works again after [`forbid`] (`selftest` only: an injected, temporary
/// failure to create a thread).
#[cfg(feature = "selftest")]
pub(crate) fn allow() {
    FORBIDDEN.store(false, Release);
}

/// The thread creations [`forbid`] has refused so far (`selftest` only).
#[cfg(feature = "selftest")]
pub(crate) fn refused() -> usize {
    REFUSED.load(Acquire)
}

/// Whether [`forbid`] has been called in this process.
#[cfg_attr(not(feature = "selftest"), allow(dead_code))]
pub(crate) fn forbidden() -> bool {
    FORBIDDEN.load(Acquire)
}

/// The default stack size (§22.2), decided on first use from `RLIMIT_STACK`.
pub(crate) fn default_stack() -> usize {
    match DEFAULT_STACK.load(Acquire) {
        0 => {
            let v = rt::default_stack(CSys.prlimit(0, RLIMIT_STACK, None).ok().map(|r| r.cur));
            DEFAULT_STACK.store(v, Release);
            v
        }
        v => v,
    }
}

/// The initial thread of a process Liella started: its descriptor, the thread list, the
/// robust list and the rseq area (the products' startup).
#[cfg_attr(not(feature = "runtime"), allow(dead_code))]
pub(crate) fn init_initial(static_program: bool) {
    let d = if static_program {
        // The area startup mapped holds the descriptor and keys after the block (zeroed).
        let tp = tb::tp();
        let daddr = tp + liella_rt::tls::BLOCK_SIZE;
        let d = tb::desc_at(daddr);
        d.init_robust(addr_of(&d.robust));
        d.keys.store(daddr + rt::DESC_SIZE, Relaxed);
        d.slots.store(daddr + rt::DESC_SIZE + KEY_AREA, Relaxed);
        d.tp.store(tp, Relaxed);
        tb::current().thread.store(daddr, Release);
        d
    } else {
        let Ok(d) = adopt() else { return };
        d.flags.fetch_and(!DESC_ADOPTED, AcqRel);
        d
    };
    // Joinable like any other (§22.1); its mapping is not the runtime's (no base), and the TID
    // word startup gave `set_tid_address` wakes its joiners.
    d.flags.fetch_or(DESC_CREATED, AcqRel);
    d.tp.store(tb::tp(), Relaxed);
    OWNED.store(true, Release);
    LOCK_ALONE.store(SINGLE_THREADED.load(Acquire) != 0, Release);
    LIVE.store(1, Release);
    register_kernel_areas(d);
    list_insert(d);
}

fn register_kernel_areas(d: &'static Thread) {
    let _ = raw::set_robust_list(addr_of(&d.robust), 24);
    if RSEQ_OWNED.load(Acquire) && CSys.rseq(rseq_static(d), 0, RSEQ_SIG).is_ok() {
        d.flags.fetch_or(DESC_RSEQ, AcqRel);
        // Published once registration works (§22.9); 0 means "none" to applications.
        crate::data::__rseq_offset.store(isize::try_from(liella_rt::tls::RSEQ_OFFSET).unwrap_or(0), Release);
        crate::data::__rseq_size.store(liella_rt::thread::RSEQ_SIZE, Release);
    }
}

fn rseq_static(d: &'static Thread) -> &'static liella_kernel::uapi::Rseq {
    &d.rseq
}

fn addr_of<T>(r: &T) -> usize {
    core::ptr::from_ref(r).expose_provenance()
}

// ------------------------------------------------------------------------------ descriptors

/// Whether the calling thread is certainly the process's only thread: Liella started the
/// process (so it creates every thread) and none has been created (`__libc_single_threaded`,
/// cleared by the first creation, the library's own service threads included). Locks that only
/// serialise threads may then be skipped. In a process another C library started, never.
pub(crate) fn alone() -> bool {
    OWNED.load(Acquire) && SINGLE_THREADED.load(Acquire) != 0
}

/// [`alone`] as the mutex fast paths ask it (MUTEX-2): one load of a private byte, and once
/// false never true again in this process (a fork child keeps its parent's value).
#[inline]
pub(crate) fn lock_alone() -> bool {
    LOCK_ALONE.load(Relaxed)
}

/// MUTEX-3: a program is about to start a task that shares our memory and runs beside the
/// caller (`clone(3)` with `CLONE_VM`, not suspended by `CLONE_VFORK`): from here on the mutex
/// fast paths use atomics, for good. Called before the system call (a failed clone leaves the
/// byte cleared, which only costs speed). `__libc_single_threaded` stays as it is (the
/// probes show glibc leaves it set there).
pub(crate) fn leave_lock_alone() {
    LOCK_ALONE.store(false, Release);
}

/// Whether the runtime started this process (so every thread it runs library code in is
/// listed: the initial thread at startup, every other at creation).
pub(crate) fn owned() -> bool {
    OWNED.load(Acquire)
}

/// The calling thread's descriptor, if it has one.
pub(crate) fn current() -> Option<&'static Thread> {
    match tb::current().thread.load(Acquire) {
        0 => None,
        a => Some(tb::desc_at(a)),
    }
}

/// The calling thread's descriptor, made on first need for a thread the runtime did not
/// create (D-22.10): a mapping of its own for the descriptor and the key area.
pub(crate) fn desc() -> KResult<&'static Thread> {
    match current() {
        Some(d) => Ok(d),
        None => adopt(),
    }
}

fn adopt() -> KResult<&'static Thread> {
    let len = (rt::DESC_SIZE + KEY_AREA + SLOT_AREA).next_multiple_of(PAGE);
    let base = map_thread(len, 0)?;
    let d = tb::desc_at(base);
    d.init_robust(base);
    d.flags.store(DESC_ADOPTED, Release);
    d.keys.store(base + rt::DESC_SIZE, Relaxed);
    d.slots.store(base + rt::DESC_SIZE + KEY_AREA, Relaxed);
    d.tp.store(tb::tp(), Relaxed);
    tb::current().thread.store(base, Release);
    Ok(d)
}

/// The calling thread's result slot `i` (D-H3: `SLOT_STRERROR`, ...), `None` when it has no
/// descriptor and none can be made.
pub(crate) fn result_slot(i: usize) -> Option<&'static liella_legacy::result_slots::Buf<SLOT_SIZE>> {
    if i >= SLOTS {
        return None;
    }
    match desc().ok()?.slots.load(Acquire) {
        0 => None,
        a => Some(tb::slot_at(a + i * SLOT_SIZE)),
    }
}

/// The calling thread's result slots from `i` on, as one buffer of `N` bytes (`SLOT_TM`: the
/// shared `struct tm`), `None` when they would pass the slot area or the thread has no
/// descriptor and none can be made.
pub(crate) fn result_area<const N: usize>(i: usize) -> Option<&'static liella_legacy::result_slots::Buf<N>> {
    if i.checked_mul(SLOT_SIZE)?.checked_add(N)? > SLOT_AREA {
        return None;
    }
    match desc().ok()?.slots.load(Acquire) {
        0 => None,
        a => Some(tb::area_at::<N>(a + i * SLOT_SIZE)),
    }
}

/// The calling thread's `SLOT_RECORDS` slot as its typed view (the result table word and the
/// regex scratch busy byte), `None` when the thread has no descriptor and none can be made.
pub(crate) fn records_slot() -> Option<&'static tb::RecordsSlot> {
    const { assert!(rt::SLOT_RECORDS * SLOT_SIZE + size_of::<tb::RecordsSlot>() <= SLOT_AREA) };
    match desc().ok()?.slots.load(Acquire) {
        0 => None,
        a => tb::records_at(a + rt::SLOT_RECORDS * SLOT_SIZE),
    }
}

fn prot_rw() -> i32 {
    i32::try_from(PROT_READ | PROT_WRITE).unwrap_or(3)
}

fn map_flags() -> i32 {
    i32::try_from(MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE | MAP_STACK).unwrap_or(0)
}

/// Whether thread mappings are marked `MADV_NOHUGEPAGE` (D-22.9): 0 until decided, then 1
/// (yes, a kernel before 6.7) or 2 (no).
static NO_HUGE: AtomicU8 = AtomicU8::new(0);

fn no_huge_pages() -> bool {
    match NO_HUGE.load(Relaxed) {
        0 => {
            let yes = CSys.uname().map_or(true, |u| {
                let r = u.release.split(|&b| b == 0).next().unwrap_or(&[]);
                rt::stack_needs_nohugepage(r)
            });
            NO_HUGE.store(if yes { 1 } else { 2 }, Relaxed);
            yes
        }
        v => v == 1,
    }
}

/// Map a thread's memory: `len` bytes, the lowest `guard` of them inaccessible. Before 6.7 the
/// mapping is made inaccessible, marked `MADV_NOHUGEPAGE`, then opened: marked before it is
/// usable, it never merges with a usable neighbour into a range a huge page could back
/// (D-22.9). The advice fails harmlessly (`EINVAL`) without transparent huge pages.
fn map_thread(len: usize, guard: usize) -> KResult<usize> {
    let none = i32::try_from(PROT_NONE).unwrap_or(0);
    let (base, protected) = if no_huge_pages() {
        let base = raw::mmap(0, len, none, map_flags(), -1, 0)?;
        let _ = raw::madvise(base, len, i32::try_from(MADV_NOHUGEPAGE).unwrap_or(0));
        (base, raw::mprotect(base + guard, len - guard, prot_rw()))
    } else {
        let base = raw::mmap(0, len, prot_rw(), map_flags(), -1, 0)?;
        (base, if guard > 0 { raw::mprotect(base, guard, none) } else { Ok(()) })
    };
    if protected.is_err() {
        let _ = raw::munmap(base, len);
        return Err(Errno::EAGAIN);
    }
    Ok(base)
}

// ------------------------------------------------------------------------------ the thread list

fn list_insert(d: &'static Thread) {
    let _g = LIST.lock(&CSys);
    let me = addr_of(d);
    let head = HEAD.load(Relaxed);
    d.next.store(head, Relaxed);
    d.prev.store(0, Relaxed);
    if head != 0 {
        tb::desc_at(head).prev.store(me, Relaxed);
    }
    HEAD.store(me, Release);
    // Its ctype words, under the lock that `setlocale`'s update takes (D-5.5).
    crate::locale::refresh_words(tb::listed(d.tp.load(Relaxed)));
}

fn list_remove(d: &Thread) {
    let _g = LIST.lock(&CSys);
    let (next, prev) = (d.next.load(Relaxed), d.prev.load(Relaxed));
    if prev == 0 {
        if HEAD.load(Relaxed) == addr_of(d) {
            HEAD.store(next, Release);
        }
    } else {
        tb::desc_at(prev).next.store(next, Relaxed);
    }
    if next != 0 {
        tb::desc_at(next).prev.store(prev, Relaxed);
    }
    d.next.store(0, Relaxed);
    d.prev.store(0, Relaxed);
}

/// The listed descriptor of thread `tp` (a `pthread_t`), or the calling thread's own.
pub(crate) fn find(tp: usize) -> Option<&'static Thread> {
    if tp == tb::tp() {
        return desc().ok();
    }
    let _g = LIST.lock(&CSys);
    let mut a = HEAD.load(Acquire);
    while a != 0 {
        let d = tb::desc_at(a);
        if d.tp.load(Relaxed) == tp {
            return Some(d);
        }
        a = d.next.load(Relaxed);
    }
    None
}

/// Visit every listed thread (the thread-list lock held): for the setxid broadcast, `fork`'s
/// child and `pthread_getattr_np`.
pub(crate) fn each(mut f: impl FnMut(&'static Thread)) {
    let _g = LIST.lock(&CSys);
    let mut a = HEAD.load(Acquire);
    while a != 0 {
        let d = tb::desc_at(a);
        a = d.next.load(Relaxed);
        f(d);
    }
}

/// `f` with the thread-list lock held, so that it does not interleave with [`each_block`]; in a
/// process that certainly has one thread, without the lock. The locale's per-thread ctype words
/// (D-5.5).
pub(crate) fn serialized<T>(f: impl FnOnce() -> T) -> T {
    if alone() {
        return f();
    }
    let _g = LIST.lock(&CSys);
    f()
}

/// Visit the thread block of every listed thread, and the calling thread's if it is not listed,
/// with the thread-list lock held (in a process that certainly has one thread, only the calling
/// thread's, without the lock): `setlocale` storing the global locale's ctype words (D-5.5).
pub(crate) fn each_block(mut f: impl FnMut(&'static liella_rt::tls::ThreadBlock)) {
    if alone() {
        return f(tb::current());
    }
    let _g = LIST.lock(&CSys);
    let me = tb::tp();
    let mut listed = false;
    let mut a = HEAD.load(Acquire);
    while a != 0 {
        let d = tb::desc_at(a);
        a = d.next.load(Relaxed);
        let tp = d.tp.load(Relaxed);
        listed |= tp == me;
        f(tb::listed(tp));
    }
    if !listed {
        f(tb::current());
    }
}

/// The thread-list lock, for `fork` (D-18.6).
pub(crate) fn list_lock() -> &'static RawMutex {
    &LIST
}

/// The stack cache's lock, for `fork` (D-18.6).
pub(crate) fn recycle_lock() -> &'static RawMutex {
    &RECYCLE
}

/// `fork`'s child: only the calling thread exists (D-18.6).
pub(crate) fn after_fork_child() {
    // The lock was taken before the fork by the forking thread, which is this one.
    HEAD.store(0, Release);
    CACHE.store(0, Release);
    CACHE_BYTES.store(0, Release);
    DEAD.store(0, Release);
    if let Some(d) = current() {
        d.next.store(0, Relaxed);
        d.prev.store(0, Relaxed);
        d.robust[0].store(addr_of(&d.robust), Relaxed);
        d.robust[2].store(0, Relaxed);
        let _ = raw::set_robust_list(addr_of(&d.robust), 24);
        if OWNED.load(Acquire) || d.has(DESC_CREATED) {
            HEAD.store(addr_of(d), Release);
        }
    }
    LIVE.store(usize::from(OWNED.load(Acquire)), Release);
}

// ------------------------------------------------------------------------------ creation

/// Map (or reuse) the memory for a thread with `lay`'s layout: its base.
fn take_mapping(lay: &Layout) -> KResult<usize> {
    {
        let _g = RECYCLE.lock(&CSys);
        sweep_locked();
        // A cached mapping of the same size and guard.
        let mut prev: Option<&Thread> = None;
        let mut a = CACHE.load(Relaxed);
        while a != 0 {
            let d = tb::desc_at(a);
            let next = d.next.load(Relaxed);
            if d.map_len.load(Relaxed) == lay.len && d.guard.load(Relaxed) == lay.guard {
                match prev {
                    Some(p) => p.next.store(next, Relaxed),
                    None => CACHE.store(next, Relaxed),
                }
                CACHE_BYTES.fetch_sub(lay.len, Relaxed);
                let base = d.map_base.load(Relaxed);
                // The parts above the stack start clean; the stack itself need not.
                let span = KEYS.span();
                conv::zero_mapped(base + lay.tls, lay.keys - lay.tls);
                conv::zero_mapped(base + lay.keys, (span * 16).min(KEY_AREA));
                return Ok(base);
            }
            prev = Some(d);
            a = next;
        }
    }
    map_thread(lay.len, lay.guard)
}

/// Return a reclaimed thread's mapping (its TID word clear): to the cache, or unmapped.
fn release_mapping(d: &Thread) {
    let (base, len) = (d.map_base.load(Relaxed), d.map_len.load(Relaxed));
    if base == 0 {
        return;
    }
    if !d.has(DESC_USER_STACK) && CACHE_BYTES.load(Relaxed) + len <= CACHE_MAX {
        let _g = RECYCLE.lock(&CSys);
        d.next.store(CACHE.load(Relaxed), Relaxed);
        CACHE.store(addr_of(d), Relaxed);
        CACHE_BYTES.fetch_add(len, Relaxed);
        return;
    }
    let _ = raw::munmap(base, len);
}

/// Reclaim the exited detached threads whose TID word the kernel has cleared (RECYCLE held).
fn sweep_locked() {
    let mut prev: Option<&Thread> = None;
    let mut a = DEAD.load(Relaxed);
    while a != 0 {
        let d = tb::desc_at(a);
        let next = d.next.load(Relaxed);
        if tb::listed(d.tp.load(Relaxed)).tid.load(Acquire) == 0 {
            match prev {
                Some(p) => p.next.store(next, Relaxed),
                None => DEAD.store(next, Relaxed),
            }
            let (base, len) = (d.map_base.load(Relaxed), d.map_len.load(Relaxed));
            if !d.has(DESC_USER_STACK) && CACHE_BYTES.load(Relaxed) + len <= CACHE_MAX {
                d.next.store(CACHE.load(Relaxed), Relaxed);
                CACHE.store(a, Relaxed);
                CACHE_BYTES.fetch_add(len, Relaxed);
            } else {
                let _ = raw::munmap(base, len);
            }
        } else {
            prev = Some(d);
        }
        a = next;
    }
}

/// `pthread_create`/`thrd_create`: start `start(arg)` on a new thread; returns its
/// `pthread_t` (the thread pointer).
pub(crate) fn create(a: &Attr, start: usize, arg: usize, c11: bool) -> KResult<usize> {
    // §22.1: thread or memory limits are EAGAIN.
    create_inner(a, start, arg, c11, false).map_err(|e| if e == Errno::ENOMEM { Errno::EAGAIN } else { e })
}

/// [`create`] for a detached thread whose creator keeps the reclaim token: the thread is
/// detached for the application (no join, no detach), and it stays listed after it exits until
/// the creator calls [`release_kept`] with the returned `pthread_t`, exactly once.
pub(crate) fn create_kept(a: &Attr, start: usize, arg: usize) -> KResult<usize> {
    let a = Attr { detached: true, ..*a };
    create_inner(&a, start, arg, false, true).map_err(|e| if e == Errno::ENOMEM { Errno::EAGAIN } else { e })
}

/// Reclaim a thread made by [`create_kept`] once the kernel has cleared its TID word (it no
/// longer runs on its stack): waiting for that when `wait`, else only if it has happened. True
/// when reclaimed (`tp` is then gone); false (`wait` unset) while the thread still runs.
pub(crate) fn release_kept(tp: usize, wait: bool) -> bool {
    let b = tb::listed(tp);
    if wait {
        let _ = wait_tid(b, None, false);
    } else if b.tid.load(Acquire) != 0 {
        return false;
    }
    let d = tb::desc_at(b.thread.load(Acquire));
    list_remove(d);
    release_mapping(d);
    true
}

/// [`create`] for a thread of the library's own that never exits (the `SIGEV_THREAD` timers'
/// helper, D-23.3): it is listed like any thread (setxid reaches it, `fork` drops it), but it
/// does not count as running, so the process still ends when the last application thread
/// exits (§22.1). The caller is running, so the count cannot reach 0 here.
pub(crate) fn create_service(a: &Attr, start: usize, arg: usize) -> KResult<usize> {
    let tp = create(a, start, arg, false)?;
    LIVE.fetch_sub(1, AcqRel);
    Ok(tp)
}

fn create_inner(a: &Attr, start: usize, arg: usize, c11: bool, kept: bool) -> KResult<usize> {
    if FORBIDDEN.load(Acquire) {
        #[cfg(feature = "selftest")]
        {
            REFUSED.fetch_add(1, AcqRel);
            while HOLD.load(Acquire) {
                CSys.sched_yield();
            }
        }
        return Err(Errno::EAGAIN);
    }
    let img = tls_image();
    let stack_size = if a.stack_size == 0 { default_stack() } else { a.stack_size };
    let guard = a.guard.unwrap_or_else(|| DEFAULT_GUARD.load(Relaxed));
    let (lay, base, sp) = match a.stack {
        None => {
            let lay = Layout::new(stack_size, guard, img.as_ref()).ok_or(Errno::EINVAL)?;
            let base = take_mapping(&lay)?;
            (lay, base, base + lay.sp)
        }
        Some((lo, size)) => {
            let (lay, sp) = Layout::for_user_stack(lo, size, img.as_ref()).ok_or(Errno::EINVAL)?;
            let base = map_thread(lay.len, 0)?;
            (lay, base, sp)
        }
    };
    let tp = base + lay.tp;
    // The TLS block: the image, then zeros (fresh or cleared memory).
    if let Some(t) = img {
        conv::copy_mapped(t.addr, base + lay.tls, t.filesz);
    }
    let me = tb::current();
    let b = tb::fresh(tp);
    b.tcb.store(tp, Relaxed);
    b.self_ptr.store(tp, Relaxed);
    b.stack_guard.store(me.stack_guard.load(Relaxed), Relaxed);
    b.pointer_guard.store(me.pointer_guard.load(Relaxed), Relaxed);
    b.multiple_threads.store(1, Relaxed);
    b.flags.store(FLAG_OWN, Relaxed);
    b.tid.store(0, Relaxed);
    // A new thread uses the global locale, whatever its creator uses (D-5.5 probes); its ctype
    // words are stored when it is listed.
    b.locale.store(0, Relaxed);
    let daddr = base + lay.desc;
    b.thread.store(daddr, Release);
    let d = tb::desc_at(daddr);
    d.init_robust(addr_of(&d.robust));
    let mut flags = DESC_CREATED;
    if c11 {
        flags |= DESC_C11;
    }
    if a.stack.is_some() {
        flags |= DESC_USER_STACK;
    }
    d.flags.store(flags, Relaxed);
    d.join.store(if a.detached { rt::DETACHED } else { JOINABLE }, Relaxed);
    // A kept thread's token is its creator's from the start ([`create_kept`]).
    d.reclaim.store(u32::from(kept), Relaxed);
    d.start.store(start, Relaxed);
    d.arg.store(arg, Relaxed);
    d.map_base.store(base, Relaxed);
    d.map_len.store(lay.len, Relaxed);
    let (lo, size) = a.stack.unwrap_or((base + lay.guard, lay.tls - lay.guard));
    d.stack_lo.store(lo, Relaxed);
    d.stack_size.store(size, Relaxed);
    d.guard.store(if a.stack.is_some() { 0 } else { lay.guard }, Relaxed);
    d.keys.store(base + lay.keys, Relaxed);
    d.slots.store(base + lay.keys + KEY_AREA, Relaxed);
    d.tp.store(tp, Relaxed);
    let creator_mask = CSys.sigprocmask(liella_kernel::uapi::SIG_BLOCK, None).map_or(0, |m| m.0);
    d.sigmask.store(a.sigmask.unwrap_or(creator_mask), Relaxed);
    let handshake = a.sched.is_some() || a.affinity.is_some();
    if let Some((policy, prio)) = a.sched {
        d.sched_policy.store(policy + 1, Relaxed);
        d.sched_prio.store(prio, Relaxed);
    }
    if let Some((mask, len)) = a.affinity {
        d.affinity.store(mask, Relaxed);
        d.affinity_len.store(len, Relaxed);
    }
    // The two words the C layer pops in the new thread: the entry and its argument.
    conv::store_words(sp, &[entry_addr(), daddr]);

    me.multiple_threads.store(1, Relaxed);
    SINGLE_THREADED.store(0, Release);
    // Before the clone: from here on every mutex operation uses atomics (the new thread reads
    // the cleared byte, written before the clone system call).
    LOCK_ALONE.store(false, Release);
    list_insert(d);
    LIVE.fetch_add(1, AcqRel);
    // The new thread starts with every signal blocked and sets its own mask.
    let all = CSys.sigprocmask(SIG_SETMASK, Some(&SigSet(!0))).unwrap_or(SigSet(creator_mask));
    let r = ffi::liella_machine_clone_thread(CLONE_FLAGS, sp, &b.tid, &b.tid, tp);
    let _ = CSys.sigprocmask(SIG_SETMASK, Some(&all));
    if r < 0 {
        list_remove(d);
        LIVE.fetch_sub(1, AcqRel);
        if a.stack.is_some() {
            let _ = raw::munmap(base, lay.len);
        } else {
            release_mapping(d);
        }
        return Err(Errno::decode(r).err().map_or(Errno::EAGAIN, |e| if e == Errno::ENOMEM { Errno::EAGAIN } else { e }));
    }
    if handshake {
        while d.started.load(Acquire) == 0 {
            let _ = futex::wait(&CSys, &d.started, 0, Scope::Private);
        }
        let e = d.start_error.load(Acquire);
        if e != 0 {
            // The thread ended without running `start`: wait for the kernel, then reclaim.
            wait_tid(b, None, false).ok();
            list_remove(d);
            if kept || d.take_reclaim() {
                release_mapping(d);
            }
            return Err(i32::try_from(e).ok().and_then(Errno::new).unwrap_or(Errno::EINVAL));
        }
    }
    Ok(tp)
}

/// The address of [`entry`].
#[allow(clippy::as_conversions)] // a function pointer's address: the only spelling Rust has
fn entry_addr() -> usize {
    entry as extern "C" fn(usize) -> i32 as usize
}

/// The new thread's first Rust frame (called by the C layer on its stack).
extern "C" fn entry(daddr: usize) -> i32 {
    let d = tb::desc_at(daddr);
    register_kernel_areas(d);
    if d.started.load(Relaxed) == 0 && (d.sched_policy.load(Relaxed) != 0 || d.affinity.load(Relaxed) != 0) {
        let verdict = apply_start_attrs(d).err().map_or(0, |e| u32::try_from(e.get()).unwrap_or(22));
        d.start_error.store(verdict, Release);
        d.started.store(1, Release);
        let _ = futex::wake(&CSys, &d.started, 1, Scope::Private);
        if verdict != 0 {
            LIVE.fetch_sub(1, AcqRel);
            raw::exit_thread(0);
        }
    }
    let _ = CSys.sigprocmask(SIG_SETMASK, Some(&SigSet(d.sigmask.load(Relaxed))));
    let f = d.start.load(Relaxed);
    let a = d.arg.load(Relaxed);
    let r = ffi::liella_machine_call3(f, i64::from_ne_bytes(a.to_ne_bytes()), 0, 0);
    finish(usize::from_ne_bytes(r.to_ne_bytes()))
}

fn apply_start_attrs(d: &Thread) -> KResult<()> {
    let tid = CSys.gettid();
    let policy = d.sched_policy.load(Relaxed);
    if policy != 0 {
        let param = liella_kernel::uapi::SchedParam { priority: d.sched_prio.load(Relaxed) };
        CSys.sched_setscheduler(tid, u32::try_from(policy - 1).map_err(|_| Errno::EINVAL)?, &param)?;
    }
    let (mask, len) = (d.affinity.load(Relaxed), d.affinity_len.load(Relaxed));
    if mask != 0 {
        CSys.sched_setaffinity(tid, conv::mapped_bytes(mask, len))?;
    }
    Ok(())
}

// ------------------------------------------------------------------------------ exit

/// The calling thread's exit sequence (§22.1): its result, `thread_local` destructors, key
/// destructors, the heap cache, then the thread-only `exit` (or `exit(0)` for the last thread
/// of a process Liella started). Cleanup buffers have already run (`crate::cancel`).
pub(crate) fn finish(result: usize) -> ! {
    let d = desc().ok();
    if let Some(d) = d {
        d.result.store(result, Release);
        d.cancel.fetch_or(rt::CANCEL_ACTING, AcqRel);
    }
    crate::process::run_thread_dtors();
    if let Some(d) = d {
        run_key_destructors(d);
        // A `SIGEV_THREAD` notification ends here however its function left (D-23.3).
        crate::timer::thread_ending(d.start.load(Relaxed), d.arg.load(Relaxed));
    }
    // After every destructor: one calling getpw*/netdb functions allocates the thread's
    // result table or resolver state again, which only these release.
    crate::users::thread_ending();
    crate::resolv::thread_ending();
    crate::heap::HEAP.park(&CSys, &crate::heap::KernelPages);
    if LIVE.fetch_sub(1, AcqRel) == 1 && OWNED.load(Acquire) {
        crate::process::exit(0);
    }
    if let Some(d) = d.filter(|d| d.has(DESC_CREATED)) {
        d.flags.fetch_or(DESC_EXITED, AcqRel);
        if d.join.load(Acquire) == rt::DETACHED && d.take_reclaim() {
            list_remove(d);
            let _g = RECYCLE.lock(&CSys);
            d.next.store(DEAD.load(Relaxed), Relaxed);
            DEAD.store(addr_of(d), Relaxed);
        }
        // A notification on a caller-supplied stack is leaving it (D-23.3, D-25.5, D-11.11).
        if d.has(DESC_USER_STACK) {
            crate::stacks::thread_ending(d.tp.load(Relaxed));
        }
    }
    raw::exit_thread(0)
}

/// The key destructors of the calling thread (§22.6), up to four rounds.
pub(crate) fn run_key_destructors(d: &Thread) {
    let keys = d.keys.load(Acquire);
    if keys == 0 {
        return;
    }
    let sp = Specific::new(tb::keys_at(keys));
    sp.run_destructors(&KEYS, |f, v| {
        let _ = ffi::liella_machine_call3(f, i64::from_ne_bytes(v.to_ne_bytes()), 0, 0);
    });
}

/// The calling thread's key area.
pub(crate) fn specific() -> KResult<Specific<'static>> {
    let d = desc()?;
    Ok(Specific::new(tb::keys_at(d.keys.load(Acquire))))
}

// ------------------------------------------------------------------------------ join, detach

/// Wait until the kernel has cleared `b`'s TID word (the thread has exited), until `deadline`
/// if given.
fn wait_tid(b: &liella_rt::tls::ThreadBlock, deadline: Option<(Clock, &Timespec)>, cancellable: bool) -> KResult<()> {
    use liella_rt::pthread_sync::Ctx;
    loop {
        let t = b.tid.load(Acquire);
        if t == 0 {
            return Ok(());
        }
        // The kernel's wake at exit is not process-private. `pthread_join` is a cancellation
        // point (§22.3).
        match crate::sync::ThreadCtx.wait(&b.tid, t, Scope::Shared, deadline, cancellable) {
            Err(e @ (Errno::ETIMEDOUT | Errno::ECANCELED)) => return Err(e),
            _ => continue,
        }
    }
}

/// How `join` waits.
#[derive(Clone, Copy)]
pub enum JoinWait<'a> {
    /// Until the thread exits.
    Block,
    /// Not at all (`pthread_tryjoin_np`: `EBUSY` if it runs).
    Try,
    /// Until an absolute deadline.
    Until(Clock, &'a Timespec),
}

/// `pthread_join` and its `_np` forms: the thread's result.
pub(crate) fn join(tp: usize, how: JoinWait<'_>) -> KResult<usize> {
    let me = desc()?;
    if tp == tb::tp() {
        return Err(Errno::EDEADLK);
    }
    let d = find(tp).ok_or(Errno::ESRCH)?;
    if !d.has(DESC_CREATED) {
        return Err(Errno::EINVAL);
    }
    if d.joining.load(Acquire) == addr_of(me) {
        return Err(Errno::EDEADLK);
    }
    if !rt::claim_join(&d.join) {
        return Err(Errno::EINVAL);
    }
    let b = tb::listed(tp);
    me.joining.store(addr_of(d), Release);
    let waited = match how {
        JoinWait::Block => wait_tid(b, None, true),
        JoinWait::Try => {
            if b.tid.load(Acquire) == 0 { Ok(()) } else { Err(Errno::EBUSY) }
        }
        JoinWait::Until(c, t) => wait_tid(b, Some((c, t)), true),
    };
    me.joining.store(0, Release);
    if let Err(e) = waited {
        // Still joinable (a cancelled join leaves the target as it was).
        d.join.store(JOINABLE, Release);
        if e == Errno::ECANCELED {
            crate::cancel::unwind(usize::MAX);
        }
        return Err(e);
    }
    let result = d.result.load(Acquire);
    list_remove(d);
    if d.take_reclaim() {
        release_mapping(d);
    }
    Ok(result)
}

/// `pthread_detach`.
pub(crate) fn detach(tp: usize) -> KResult<()> {
    let d = find(tp).ok_or(Errno::ESRCH)?;
    if !d.has(DESC_CREATED) {
        return Err(Errno::EINVAL);
    }
    let exited = rt::detach(&d.join, d.has(DESC_EXITED)).ok_or(Errno::EINVAL)?;
    if exited && d.take_reclaim() {
        list_remove(d);
        let b = tb::listed(tp);
        if b.tid.load(Acquire) == 0 {
            release_mapping(d);
        } else {
            let _g = RECYCLE.lock(&CSys);
            d.next.store(DEAD.load(Relaxed), Relaxed);
            DEAD.store(addr_of(d), Relaxed);
        }
    }
    Ok(())
}

/// The kernel TID of thread `tp` (a listed or the calling thread), for `pthread_kill`,
/// naming, scheduling and affinity; `ESRCH` once it has exited.
pub(crate) fn tid_of(tp: usize) -> KResult<i32> {
    if tp == tb::tp() {
        return Ok(own_tid());
    }
    let _ = find(tp).ok_or(Errno::ESRCH)?;
    match tb::listed(tp).tid.load(Acquire) {
        0 => Err(Errno::ESRCH),
        t => i32::try_from(t).map_err(|_| Errno::ESRCH),
    }
}

/// MUTEX-3: the calling thread's TID when the block's word holds it (a thread Liella runs and
/// the word set, as [`own_tid`] reads it), from two `%fs`-relative loads; `None`: ask [`own_tid`].
#[inline]
pub(crate) fn fast_tid() -> Option<u32> {
    let t = tb::read32::<tb::Tid>();
    // MUTEX-5: one signed test (`try_from` + `> 0` compiled to two branches on one flag set).
    (tb::read32::<tb::Flags>() & FLAG_OWN != 0 && i32::from_ne_bytes(t.to_ne_bytes()) > 0).then_some(t)
}

/// [`own_tid`] without its system-call fallback: the TID the thread block holds, `None` when it
/// holds none (a thread the library did not set up, or not yet). FILE-LOCK-2: the stream-lock fast
/// paths take it with no call, so they need no stack frame; `None` sends them to the full path.
#[inline(always)]
pub(crate) fn own_tid_fast() -> Option<u32> {
    if tb::read32::<tb::Flags>() & FLAG_OWN == 0 {
        return None;
    }
    let t = tb::read32::<tb::Tid>();
    // The same value as `own_tid` (through `i32`): only a positive TID counts. One signed test
    // (MUTEX-5: `try_from` + `> 0` compiled to two branches; STDIO-LOCKED-PATH).
    (i32::from_ne_bytes(t.to_ne_bytes()) > 0).then_some(t)
}

/// The calling thread's TID: the thread block's word in a thread Liella runs (`gettid` is that
/// load, D-22.9); the kernel's answer elsewhere, where the word may be stale (D-22.10).
#[inline]
pub(crate) fn own_tid() -> i32 {
    // MUTEX-3: two independent `%fs`-relative loads instead of `%fs:0` and two loads after it
    // (the mutex fast paths store the owner's TID).
    if tb::read32::<tb::Flags>() & FLAG_OWN != 0 {
        match tb::read32::<tb::Tid>() {
            0 => CSys.gettid(),
            t => i32::try_from(t).unwrap_or(0),
        }
    } else {
        CSys.gettid()
    }
}

