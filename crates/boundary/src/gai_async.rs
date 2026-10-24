//! `getaddrinfo_a`, `gai_suspend`, `gai_error`, `gai_cancel` (spec §16.2; libanl, part of libc
//! since 2.34; D-16.8): lookups on worker threads, with `getaddrinfo(3)`'s semantics each.
//!
//! **Requests.** Each `struct gaicb` of the list is queued, its `__return` set to
//! `EAI_INPROGRESS`; the library keeps its state in `__glibc_reserved` (queued, running, done)
//! and the request's group. A worker takes the oldest request, runs `getaddrinfo` with
//! `ar_name`, `ar_service` and `ar_request`, then stores `ar_result` and `__return` (the result
//! code; `gai_error` reads it). Workers are the library's own threads (at most
//! [`MAX_WORKERS`], made as requests wait and kept for later ones; they do not count as running
//! threads, and every application signal is blocked in them). Requests run in parallel up to the
//! number of workers.
//!
//! **Modes** (`getaddrinfo_a(3)`). `GAI_WAIT` returns when every request of the list is done
//! (no notification); `GAI_NOWAIT` returns at once, and the `sigevent` notifies once when the
//! whole list is done (observed): `SIGEV_SIGNAL` queues the signal to the process with
//! `si_code` `SI_ASYNCNL` and the event's value; `SIGEV_THREAD` calls the function with the
//! value on a new detached thread (made as the `SIGEV_THREAD` timers make theirs, D-23.3);
//! `SIGEV_NONE` or no event: nothing. Any other mode is `EAI_SYSTEM` with `EINVAL`. NULL
//! entries are skipped; a list without requests is 0. When no worker can be made (another
//! `ld.so`, D-22.10), `GAI_WAIT` runs the requests in the calling thread, one after the other,
//! and `GAI_NOWAIT` is `EAI_AGAIN` with each request's `__return` `EAI_AGAIN`.
//!
//! **Waiting** (`gai_suspend`, `getaddrinfo_a(3)`): 0 as soon as one listed request is done
//! (the man page; the reference reports `EAI_ALLDONE` for a request completed before the call:
//! a deliberate difference); `EAI_ALLDONE` for a list without requests; `EAI_AGAIN` when the
//! timeout expires (the man page; the reference reports `EAI_SYSTEM`: a deliberate
//! difference); `EAI_INTR` when a signal interrupts the wait.
//!
//! **Cancelling** (`gai_cancel`): a queued request is taken off the queue, `EAI_CANCELED`
//! (its `__return` too, and its group's notification counts it); a running one is not
//! cancelled (`EAI_NOTCANCELED`); a done one is `EAI_ALLDONE`. NULL cancels every queued request
//! of the process: `EAI_CANCELED` if any was, else `EAI_NOTCANCELED` while one runs, else
//! `EAI_ALLDONE` (observed).
//!
//! **Processes.** A child of `fork` starts with no workers and an empty queue (requests of the
//! parent are not run in it).

use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering::{AcqRel, Acquire, Relaxed, Release}};

use liella_abi::layout::struct_addrinfo;
use liella_kernel::errno::Errno;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{SigEvent, SigInfo, Timespec, CLOCK_MONOTONIC, SIGEV_SIGNAL, SIGEV_THREAD};
use liella_rt::futex::{self, Clock, Scope};
use liella_rt::sync::{RawCondvar, RawMutex};
use liella_rt::thread::Attr;

use crate::conv;
use crate::errno;
use crate::ffi;
use crate::netdb;
use crate::sys::CSys;
use crate::thread;

/// `EAI_AGAIN`.
const EAI_AGAIN: i32 = -3;
/// `EAI_SYSTEM`.
const EAI_SYSTEM: i32 = -11;
/// `EAI_INPROGRESS`.
pub const EAI_INPROGRESS: i32 = -100;
/// `EAI_CANCELED`.
pub const EAI_CANCELED: i32 = -101;
/// `EAI_NOTCANCELED`.
pub const EAI_NOTCANCELED: i32 = -102;
/// `EAI_ALLDONE`.
pub const EAI_ALLDONE: i32 = -103;
/// `EAI_INTR`.
pub const EAI_INTR: i32 = -104;
/// `GAI_WAIT`.
pub const GAI_WAIT: i32 = 0;
/// `GAI_NOWAIT`.
pub const GAI_NOWAIT: i32 = 1;
/// `SI_ASYNCNL`: the `si_code` of the completion signal.
const SI_ASYNCNL: i32 = -60;
/// Every signal but the library's internal one (as for the timers' threads, D-23.3).
const APP_SIGNALS: u64 = !(1 << 31);

/// The most worker threads.
pub const MAX_WORKERS: usize = 32;
/// The most requests waiting in the queue.
const QUEUE: usize = 4096;
/// The most lists awaiting a notification at once.
const GROUPS: usize = 256;

// The words of a `struct gaicb` (56 bytes): `ar_name`, `ar_service`, `ar_request`,
// `ar_result`, then `__return` with the first reserved int (the state), then the group.
const W_NAME: usize = 0;
const W_SERVICE: usize = 1;
const W_REQUEST: usize = 2;
const W_RESULT: usize = 3;
const W_STATUS: usize = 4;
const W_GROUP: usize = 5;
const WORDS: usize = 7;

const ST_DONE: usize = 0;
const ST_QUEUED: usize = 1;
const ST_RUNNING: usize = 2;

static LOCK: RawMutex = RawMutex::new();
static WORK: RawCondvar = RawCondvar::new();
static QUEUED: [AtomicUsize; QUEUE] = [const { AtomicUsize::new(0) }; QUEUE];
static HEAD: AtomicUsize = AtomicUsize::new(0);
static LEN: AtomicUsize = AtomicUsize::new(0);
static WORKERS: AtomicUsize = AtomicUsize::new(0);
static IDLE: AtomicUsize = AtomicUsize::new(0);
/// Advances with every completion; `gai_suspend` and `GAI_WAIT` sleep on it.
static DONE: AtomicU32 = AtomicU32::new(0);

/// A list awaiting its notification.
struct Group {
    used: AtomicUsize,
    remaining: AtomicUsize,
    notify: AtomicI32,
    signo: AtomicI32,
    value: AtomicU64,
    function: AtomicUsize,
}

static GROUP: [Group; GROUPS] = [const { Group { used: AtomicUsize::new(0), remaining: AtomicUsize::new(0), notify: AtomicI32::new(0), signo: AtomicI32::new(0), value: AtomicU64::new(0), function: AtomicUsize::new(0) } }; GROUPS];

fn cb(addr: usize) -> Option<&'static [AtomicUsize]> {
    conv::named_words(addr, WORDS)
}

fn status_word(ret: i32, state: usize) -> usize {
    usize::try_from(u32::from_ne_bytes(ret.to_ne_bytes())).unwrap_or(0) | state << 32
}

fn status(c: &[AtomicUsize]) -> (i32, usize) {
    let w = c.get(W_STATUS).map_or(0, |w| w.load(Acquire));
    (i32::from_ne_bytes(u32::try_from(w & 0xffff_ffff).unwrap_or(0).to_ne_bytes()), w >> 32)
}

fn set_status(c: &[AtomicUsize], ret: i32, state: usize) {
    if let Some(w) = c.get(W_STATUS) {
        w.store(status_word(ret, state), Release);
    }
}

fn completed() {
    DONE.fetch_add(1, Release);
    let _ = futex::wake_all(&CSys, &DONE, Scope::Private);
}

/// A request is done: store its result, wake the waiters, count it in its group.
fn finish(c: &[AtomicUsize], ret: i32, result: usize) {
    if let Some(w) = c.get(W_RESULT) {
        w.store(result, Relaxed);
    }
    let group = c.get(W_GROUP).map_or(0, |w| w.swap(0, Relaxed));
    set_status(c, ret, ST_DONE);
    completed();
    if let Some(g) = group.checked_sub(1).and_then(|i| GROUP.get(i)) {
        if g.remaining.fetch_sub(1, AcqRel) == 1 {
            notify(group.saturating_sub(1));
        }
    }
}

/// Run one request in the calling thread.
fn run(addr: usize) {
    let Some(c) = cb(addr) else { return };
    let word = |i: usize| c.get(i).map_or(0, |w| w.load(Relaxed));
    let node = conv::named_text(word(W_NAME));
    let service = conv::named_text(word(W_SERVICE));
    let hints = conv::named_bytes(word(W_REQUEST), size_of::<struct_addrinfo>()).map(|b| {
        let int = |at: usize| i32::from_ne_bytes(<[u8; 4]>::try_from(b.get(at..at.saturating_add(4)).unwrap_or(&[0; 4])).unwrap_or([0; 4]));
        struct_addrinfo { ai_flags: int(0), ai_family: int(4), ai_socktype: int(8), ai_protocol: int(12), ai_addrlen: 0, ai_addr: 0, ai_canonname: 0, ai_next: 0 }
    });
    let mut head = 0usize;
    let ret = netdb::getaddrinfo(Ok(node), Ok(service), hints.as_ref(), Some(&mut head));
    finish(c, ret, if ret == 0 { head } else { 0 });
}

/// The worker's start routine.
extern "C" fn worker(_: usize) -> usize {
    loop {
        let next = {
            let _g = LOCK.lock(&CSys);
            while LEN.load(Relaxed) == 0 {
                IDLE.fetch_add(1, Relaxed);
                WORK.wait(&CSys, &LOCK, Scope::Private);
                IDLE.fetch_sub(1, Relaxed);
            }
            pop()
        };
        if next != 0 {
            run(next);
        }
    }
}

#[allow(clippy::as_conversions)] // a function pointer's address: the only spelling Rust has
fn worker_addr() -> usize {
    worker as extern "C" fn(usize) -> usize as usize
}

#[allow(clippy::as_conversions)] // a function pointer's address: the only spelling Rust has
fn notifier_addr() -> usize {
    notifier as extern "C" fn(usize) -> usize as usize
}

/// Under the lock: the oldest queued request (marked running), or 0.
fn pop() -> usize {
    while LEN.load(Relaxed) > 0 {
        let h = HEAD.load(Relaxed);
        let a = QUEUED.get(h % QUEUE).map_or(0, |s| s.swap(0, Relaxed));
        HEAD.store(h.wrapping_add(1), Relaxed);
        LEN.fetch_sub(1, Relaxed);
        if a != 0 {
            if let Some(c) = cb(a) {
                set_status(c, EAI_INPROGRESS, ST_RUNNING);
            }
            return a;
        }
    }
    0
}

/// Under the lock: queue `addr`; false when the queue is full.
fn push(addr: usize) -> bool {
    let len = LEN.load(Relaxed);
    if len >= QUEUE {
        return false;
    }
    let at = HEAD.load(Relaxed).wrapping_add(len) % QUEUE;
    if let Some(s) = QUEUED.get(at) {
        s.store(addr, Relaxed);
    }
    LEN.store(len.saturating_add(1), Relaxed);
    true
}

/// Under the lock: take `addr` off the queue (leaving a hole the workers skip).
fn unqueue(addr: usize) -> bool {
    let (h, len) = (HEAD.load(Relaxed), LEN.load(Relaxed));
    for k in 0..len {
        if let Some(s) = QUEUED.get(h.wrapping_add(k) % QUEUE) {
            if s.load(Relaxed) == addr {
                s.store(0, Relaxed);
                return true;
            }
        }
    }
    false
}

/// Under the lock: start workers while requests wait and none is idle. Whether a worker can
/// run the queue.
fn workers() -> bool {
    while IDLE.load(Relaxed) < LEN.load(Relaxed) && WORKERS.load(Relaxed) < MAX_WORKERS {
        let a = Attr { detached: true, stack_size: 256 << 10, sigmask: Some(APP_SIGNALS), ..Attr::default() };
        if thread::create_service(&a, worker_addr(), 0).is_err() {
            break;
        }
        WORKERS.fetch_add(1, Relaxed);
        IDLE.fetch_add(1, Relaxed);
    }
    WORKERS.load(Relaxed) > 0
}

/// A list's notification (its group `g` is then free).
fn notify(g: usize) {
    let Some(grp) = GROUP.get(g) else { return };
    match grp.notify.load(Relaxed) {
        SIGEV_SIGNAL => {
            let pid = CSys.getpid();
            let mut info = SigInfo::queued(grp.signo.load(Relaxed), pid, CSys.getuid(), grp.value.load(Relaxed));
            info.code = SI_ASYNCNL;
            let _ = CSys.sigqueueinfo(pid, grp.signo.load(Relaxed), &info);
            grp.used.store(0, Release);
        }
        SIGEV_THREAD => {
            let a = Attr { detached: true, sigmask: Some(APP_SIGNALS), ..Attr::default() };
            if thread::create(&a, notifier_addr(), g, false).is_err() {
                grp.used.store(0, Release);
            }
        }
        _ => grp.used.store(0, Release),
    }
}

/// A `SIGEV_THREAD` notification's start routine: the function with the value.
extern "C" fn notifier(g: usize) -> usize {
    let Some(grp) = GROUP.get(g) else { return 0 };
    let (f, v) = (grp.function.load(Relaxed), grp.value.load(Relaxed));
    grp.used.store(0, Release);
    // `void f(union sigval)`: the 8-byte union is passed as an integer (psABI).
    let _ = ffi::liella_machine_call3(f, i64::from_ne_bytes(v.to_ne_bytes()), 0, 0);
    0
}

/// A free group for a list of `n` requests notifying as `e` says (`None`: none left).
fn group(e: &SigEvent, n: usize) -> Option<usize> {
    let i = GROUP.iter().position(|g| g.used.compare_exchange(0, 1, AcqRel, Relaxed).is_ok())?;
    let g = GROUP.get(i)?;
    g.remaining.store(n, Relaxed);
    g.notify.store(e.notify, Relaxed);
    g.signo.store(e.signo, Relaxed);
    g.value.store(e.value, Relaxed);
    let [f, _] = e.thread_words();
    g.function.store(usize::try_from(f).unwrap_or(0), Relaxed);
    Some(i)
}

/// `getaddrinfo_a(mode, list, nitems, sevp)` (`list`: the entries' addresses, 0 for NULL).
pub fn getaddrinfo_a(mode: i32, list: KList<'_>, sig: Option<&SigEvent>) -> i32 {
    if mode != GAI_WAIT && mode != GAI_NOWAIT {
        errno::set(Errno::EINVAL);
        return EAI_SYSTEM;
    }
    let Ok(list) = list else {
        errno::set(Errno::EFAULT);
        return EAI_SYSTEM;
    };
    let reqs = || entries(list).filter(|&a| a != 0);
    let n = reqs().count();
    if n == 0 {
        return 0;
    }
    let notifying = mode == GAI_NOWAIT && sig.is_some_and(|e| e.notify == SIGEV_SIGNAL || e.notify == SIGEV_THREAD);
    let g = match (notifying, sig) {
        (true, Some(e)) => match group(e, n) {
            Some(g) => Some(g),
            None => return EAI_AGAIN,
        },
        _ => None,
    };
    let inline = {
        let _g = LOCK.lock(&CSys);
        let mut ok = true;
        for a in reqs() {
            let Some(c) = cb(a) else { continue };
            set_status(c, EAI_INPROGRESS, ST_QUEUED);
            if let Some(w) = c.get(W_GROUP) {
                w.store(g.map_or(0, |g| g.saturating_add(1)), Relaxed);
            }
            if !push(a) {
                ok = false;
            }
        }
        let runnable = ok && workers();
        if runnable {
            WORK.notify_all(&CSys, Scope::Private);
            false
        } else {
            for a in reqs() {
                unqueue(a);
            }
            if mode == GAI_NOWAIT {
                for a in reqs() {
                    if let Some(c) = cb(a) {
                        if let Some(w) = c.get(W_GROUP) {
                            w.store(0, Relaxed);
                        }
                        set_status(c, EAI_AGAIN, ST_DONE);
                    }
                }
                if let Some(grp) = g.and_then(|g| GROUP.get(g)) {
                    grp.used.store(0, Release);
                }
                return EAI_AGAIN;
            }
            true
        }
    };
    if inline {
        for a in reqs() {
            run(a);
        }
        return 0;
    }
    if mode == GAI_WAIT {
        loop {
            let seen = DONE.load(Acquire);
            if reqs().all(|a| cb(a).is_none_or(|c| status(c).1 == ST_DONE)) {
                break;
            }
            let _ = futex::wait(&CSys, &DONE, seen, Scope::Private);
        }
    }
    0
}

/// A request list: the bytes of its entries (8 each).
pub type KList<'a> = liella_kernel::errno::KResult<&'a [u8]>;

/// The entries of a list, as addresses (0 for NULL).
fn entries(list: &[u8]) -> impl Iterator<Item = usize> + Clone + '_ {
    list.chunks_exact(8).map(|c| usize::from_ne_bytes(<[u8; 8]>::try_from(c).unwrap_or([0; 8])))
}

/// `gai_error(req)`.
pub fn gai_error(req: usize) -> i32 {
    match cb(req) {
        Some(c) => status(c).0,
        None => {
            errno::set(Errno::EFAULT);
            EAI_SYSTEM
        }
    }
}

/// `gai_suspend(list, nitems, timeout)`.
pub fn gai_suspend(list: KList<'_>, timeout: Option<&Timespec>) -> i32 {
    let Ok(list) = list else { return EAI_ALLDONE };
    let reqs = || entries(list).filter(|&a| a != 0);
    if reqs().next().is_none() {
        return EAI_ALLDONE;
    }
    let deadline = timeout.map(|t| {
        let now = CSys.clock_gettime(CLOCK_MONOTONIC).unwrap_or_default();
        let ns = now.nsec.saturating_add(t.nsec.clamp(0, 999_999_999));
        Timespec { sec: now.sec.saturating_add(t.sec.max(0)).saturating_add(ns / 1_000_000_000), nsec: ns % 1_000_000_000 }
    });
    loop {
        let seen = DONE.load(Acquire);
        if reqs().any(|a| cb(a).is_some_and(|c| status(c).0 != EAI_INPROGRESS)) {
            return 0;
        }
        match futex::wait_until(&CSys, &DONE, seen, Scope::Private, deadline.as_ref().map(|d| (Clock::Monotonic, d))) {
            Err(e) if e == Errno::ETIMEDOUT => return EAI_AGAIN,
            Err(e) if e == Errno::EINTR => return EAI_INTR,
            _ => {}
        }
    }
}

/// `gai_cancel(req)` (0: every queued request).
pub fn gai_cancel(req: usize) -> i32 {
    let mut canceled: [usize; 64] = [0; 64];
    let mut n = 0usize;
    let r = {
        let _g = LOCK.lock(&CSys);
        if req == 0 {
            let (h, len) = (HEAD.load(Relaxed), LEN.load(Relaxed));
            let mut running = WORKERS.load(Relaxed) > IDLE.load(Relaxed);
            for k in 0..len {
                if let Some(s) = QUEUED.get(h.wrapping_add(k) % QUEUE) {
                    let a = s.load(Relaxed);
                    if a != 0 && n < canceled.len() {
                        s.store(0, Relaxed);
                        if let Some(slot) = canceled.get_mut(n) {
                            *slot = a;
                        }
                        n = n.saturating_add(1);
                    } else if a != 0 {
                        running = true;
                    }
                }
            }
            if n > 0 {
                EAI_CANCELED
            } else if running {
                EAI_NOTCANCELED
            } else {
                EAI_ALLDONE
            }
        } else {
            match cb(req) {
                None => EAI_ALLDONE,
                Some(c) => match status(c) {
                    (EAI_INPROGRESS, ST_QUEUED) if unqueue(req) => {
                        if let Some(slot) = canceled.first_mut() {
                            *slot = req;
                        }
                        n = 1;
                        EAI_CANCELED
                    }
                    (EAI_INPROGRESS, _) => EAI_NOTCANCELED,
                    _ => EAI_ALLDONE,
                },
            }
        }
    };
    for &a in canceled.get(..n).unwrap_or(&[]) {
        if let Some(c) = cb(a) {
            finish(c, EAI_CANCELED, 0);
        }
    }
    r
}

/// The child of `fork`: no workers, an empty queue, no groups.
pub(crate) fn after_fork_child() {
    LOCK.reset();
    for s in QUEUED.iter() {
        s.store(0, Relaxed);
    }
    HEAD.store(0, Relaxed);
    LEN.store(0, Relaxed);
    WORKERS.store(0, Relaxed);
    IDLE.store(0, Relaxed);
    for g in GROUP.iter() {
        g.used.store(0, Relaxed);
    }
}
