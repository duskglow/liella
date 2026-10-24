//! POSIX message queues and shared memory, the process's side (spec §25.3–25.4; D-25.2,
//! D-25.3, D-25.5): `mq_notify`'s `SIGEV_THREAD` notifications, `__mq_open_2` and the
//! shared-memory directory. The names and the calls are `liella::ipc`. Design:
//! D-25.6 (M1-M8).
//!
//! **`SIGEV_THREAD`** (⚑ KERN: `<linux/mqueue.h>`). The kernel does not start threads: at the
//! registration it copies a 32-byte cookie, and later sends it to the netlink socket named in
//! `sigev_signo`, its last byte saying why: `NOTIFY_WOKENUP` (a message arrived on the empty
//! queue: the registration is used up) or `NOTIFY_REMOVED` (`mq_notify(q, NULL)` or the
//! registrant closed the queue). The process makes one such socket (`AF_NETLINK`,
//! `NETLINK_ROUTE`, close-on-exec, unbound, never a standard descriptor) and one service thread
//! reading it, both at its first `SIGEV_THREAD` registration.
//!
//! **Registrations.** Each one is a record: the function, the value, a copy of the thread
//! attributes (the caller may destroy them once `mq_notify` returns) and a key, a number never
//! used twice, which is all the cookie carries. The service thread acts only on a cookie whose
//! key names a live record (it never trusts an address that came through a socket). A woken
//! record starts a **new detached thread** made with its attributes and no signal blocked
//! (`stacks::launch`: a caller-supplied stack serves one notification at a time), which takes
//! the function and the value, frees the record, and calls the function with the value. A
//! removed record is freed. Records are heap blocks on a list that only grows; a freed one is
//! reused by the next registration.
//!
//! **A notification thread that cannot be started** (a thread limit, memory), at once or after
//! waiting for its caller-supplied stack, keeps its record and is retried with the timers'
//! back-off by the timers' helper (`timer::retry_soon`, [`retry_due`]; owner ruling #680,
//! extending #269): delivered late, once, never lost (glibc drops it: a deliberate difference,
//! D-25.5). The message used up the kernel's registration, so meanwhile the queue takes a new one
//! (from this process or another), as when the thread has started; `mq_notify(q, NULL)` does not
//! cancel the pending retry (the notification is due already). The helper is made with the
//! first `SIGEV_THREAD` registration (`timer::retry_helper`), so a retry needs no new thread.
//!
//! **Errors.** A NULL function is `EINVAL`. No thread for the service (a process that may not
//! create threads, D-22.10) is `EAGAIN`; no socket is the socket's error; no memory for a record
//! or the attribute copy is `ENOMEM`; the kernel's own errors (`EBADF`, `EBUSY`, `EINVAL`) are
//! passed on, and a refused registration keeps nothing. Every other notification, and NULL,
//! is the kernel's alone.
//!
//! **`fork`**: the child has no registration (the kernel keeps them for the parent) and no
//! pending retry (the parent delivers its own), no service thread and no socket; its first
//! registration makes its own.

use core::sync::atomic::{AtomicI32, AtomicUsize, Ordering::{Acquire, Relaxed, Release}};

use liella::error::{Errno, KResult};
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{MqAttr, SigEvent, Timespec, AF_NETLINK, CLOCK_MONOTONIC, NETLINK_ROUTE, O_CREAT, SIGEV_THREAD, SOCK_CLOEXEC, SOCK_RAW};
use liella_legacy::pthread_compat::ATTR_WORDS;
use liella_rt::sync::RawMutex;
use liella_rt::thread::Attr;

use crate::sys::CSys;
use crate::timer::{self, Notifier};
use crate::{ffi, heap, pthread, stacks, tb, thread};

/// The cookie's length; the key is in its first 8 bytes, the reason in its last.
const COOKIE_LEN: usize = 32;
const NOTIFY_WOKENUP: u8 = 1;

/// Every signal but the library's internal one (32, D-20.1): the service thread's mask.
const APP_SIGNALS: u64 = !(1 << 31);
/// The service thread's stack: it runs only the library.
const SERVICE_STACK: usize = 128 << 10;

/// Serialises the records, the socket's and service thread's start, and the notification
/// threads' reads.
static LOCK: RawMutex = RawMutex::new();
/// The first record (heap blocks of [`WORDS`] words, linked through [`LINK`], never freed).
static FIRST: AtomicUsize = AtomicUsize::new(0);
/// The next key (0 marks a free record).
static NEXT_KEY: AtomicUsize = AtomicUsize::new(1);
/// The netlink socket; -1 until the first `SIGEV_THREAD` registration.
static SOCKET: AtomicI32 = AtomicI32::new(-1);

/// A record's words.
const LINK: usize = 0;
const KEY: usize = 1;
const FUNCTION: usize = 2;
const VALUE: usize = 3;
/// When the pending retry of a notification whose thread could not be started is due
/// (`CLOCK_MONOTONIC`, ns); 0: none pending.
const RETRY_AT: usize = 4;
/// The next retry's delay (`timer::back_off`); 0 outside a failure episode.
const BACKOFF: usize = 5;
/// The copy of the attribute object (`pthread::attr_copy`).
const ATTR: usize = 6;
const WORDS: usize = ATTR + ATTR_WORDS;

fn record(addr: usize) -> &'static [AtomicUsize] {
    tb::words64(addr, WORDS)
}

fn get(r: &[AtomicUsize], i: usize) -> usize {
    r.get(i).map_or(0, |w| w.load(Relaxed))
}

fn put(r: &[AtomicUsize], i: usize, v: usize) {
    if let Some(w) = r.get(i) {
        w.store(v, Relaxed);
    }
}

fn attrs(r: &[AtomicUsize]) -> &[AtomicUsize] {
    r.get(ATTR..).unwrap_or(&[])
}

/// Under the lock: the records, in list order.
fn records() -> impl Iterator<Item = &'static [AtomicUsize]> {
    core::iter::successors(Some(FIRST.load(Acquire)).filter(|&a| a != 0).map(record), |r| Some(get(r, LINK)).filter(|&a| a != 0).map(record))
}

/// Under the lock: the live record of `key`.
fn find(key: usize) -> Option<&'static [AtomicUsize]> {
    if key == 0 {
        return None;
    }
    records().find(|r| get(r, KEY) == key)
}

/// Under the lock: a free record (reused, or a new one listed).
fn free_record() -> KResult<&'static [AtomicUsize]> {
    if let Some(r) = records().find(|r| get(r, KEY) == 0) {
        return Ok(r);
    }
    let addr = liella_legacy::malloc_compat::malloc(&heap::CTX, WORDS.saturating_mul(8)).map_err(|_| Errno::ENOMEM)?;
    let r = record(addr);
    for w in r {
        w.store(0, Relaxed);
    }
    put(r, LINK, FIRST.load(Relaxed));
    FIRST.store(addr, Release);
    Ok(r)
}

/// Under the lock: free a record (its attribute copy too).
fn release(r: &[AtomicUsize]) {
    pthread::attr_release(attrs(r));
    put(r, RETRY_AT, 0);
    put(r, KEY, 0);
}

/// `mq_notify(q, event)`; `attr` is the event's `sigev_notify_attributes`, as words (for
/// `SIGEV_THREAD`). Any other notification, and NULL, is the kernel's.
pub fn notify(q: Fd, event: Option<&SigEvent>, attr: KResult<Option<&[AtomicUsize]>>) -> KResult<()> {
    match event {
        Some(e) if e.notify == SIGEV_THREAD => register(q, e, attr?),
        _ => CSys.mq_notify(q, event),
    }
}

/// A `SIGEV_THREAD` registration.
fn register(q: Fd, e: &SigEvent, attr: Option<&[AtomicUsize]>) -> KResult<()> {
    let [function, _] = e.thread_words();
    if function == 0 {
        return Err(Errno::EINVAL);
    }
    let _g = LOCK.lock(&CSys);
    // The retries' helper first: a refused notification thread is retried without making one.
    timer::retry_helper()?;
    let sock = socket()?;
    let r = free_record()?;
    put(r, RETRY_AT, 0);
    put(r, BACKOFF, 0);
    if let Err(err) = pthread::attr_copy(attr, attrs(r)) {
        pthread::attr_release(attrs(r));
        return Err(err);
    }
    let key = NEXT_KEY.fetch_add(1, Relaxed);
    put(r, FUNCTION, usize::try_from(function).unwrap_or(0));
    put(r, VALUE, usize::try_from(e.value).unwrap_or(0));
    put(r, KEY, key);
    let mut cookie = [0u8; COOKIE_LEN];
    if let Some(head) = cookie.get_mut(..8) {
        head.copy_from_slice(&key.to_ne_bytes());
    }
    let at = u64::try_from(cookie.as_ptr().expose_provenance()).unwrap_or(0);
    let r2 = CSys.mq_notify(q, Some(&SigEvent::new(SIGEV_THREAD, sock.raw(), at, 0)));
    if r2.is_err() {
        release(r);
    }
    r2
}

/// Under the lock: the netlink socket, made with the service thread the first time.
fn socket() -> KResult<Fd> {
    let s = SOCKET.load(Acquire);
    if s >= 0 {
        return Ok(Fd::new(s));
    }
    let sock = CSys.socket(AF_NETLINK, SOCK_RAW | SOCK_CLOEXEC, NETLINK_ROUTE).and_then(|s| liella::io::above_standard(&CSys, s))?;
    let a = Attr { detached: true, stack_size: SERVICE_STACK, sigmask: Some(APP_SIGNALS), ..Attr::default() };
    if let Err(e) = thread::create_service(&a, service_addr(), usize::try_from(sock.raw()).unwrap_or(0)) {
        let _ = CSys.close(sock);
        return Err(e);
    }
    SOCKET.store(sock.raw(), Release);
    Ok(sock)
}

/// The address of [`service`].
#[allow(clippy::as_conversions)] // a function pointer's address: the only spelling Rust has
fn service_addr() -> usize {
    service as extern "C" fn(usize) -> usize as usize
}

/// The address of [`notification`].
#[allow(clippy::as_conversions)] // a function pointer's address: the only spelling Rust has
fn notification_addr() -> usize {
    notification as extern "C" fn(usize) -> usize as usize
}

/// The service thread: reads the kernel's cookies for as long as the process runs.
extern "C" fn service(sock: usize) -> usize {
    let sock = Fd::new(i32::try_from(sock).unwrap_or(-1));
    let mut cookie = [0u8; COOKIE_LEN];
    loop {
        match CSys.recvfrom(sock, &mut cookie, 0, None) {
            Ok(COOKIE_LEN) => cookie_arrived(&cookie),
            // Not a cookie, an interruption, or messages the socket had no room for (their
            // notifications are lost: the kernel's limit).
            Ok(_) | Err(Errno::EINTR | Errno::ENOBUFS) => {}
            // The socket is unusable (the application closed the descriptor): do not spin.
            Err(_) => {
                let _ = CSys.clock_nanosleep(CLOCK_MONOTONIC, 0, &Timespec { sec: 1, nsec: 0 }, None);
            }
        }
    }
}

/// One cookie: start the notification, or forget a removed registration.
fn cookie_arrived(cookie: &[u8; COOKIE_LEN]) {
    let mut head = [0u8; 8];
    head.copy_from_slice(cookie.get(..8).unwrap_or(&[0; 8]));
    let key = usize::from_ne_bytes(head);
    let g = LOCK.lock(&CSys);
    let Some(r) = find(key) else {
        return;
    };
    if cookie.last() != Some(&NOTIFY_WOKENUP) {
        release(r);
        return;
    }
    if !start(r, key) {
        schedule(r);
        drop(g);
        timer::retry_soon();
    }
}

/// Under the lock: start the notification thread of the woken record `r` (on a caller-supplied
/// stack it may start later: `crate::stacks`). False: it could not be started.
fn start(r: &[AtomicUsize], key: usize) -> bool {
    let ok = stacks::launch(attrs(r), 0, notification_addr(), key).is_ok();
    if ok {
        timer::recovered(r, BACKOFF, Notifier::Mq);
    }
    ok
}

/// Under the lock: the notification of `r` could not be started; retry it after the back-off.
fn schedule(r: &[AtomicUsize]) {
    let delay = timer::back_off(r, BACKOFF, Notifier::Mq);
    put(r, RETRY_AT, timer::retry_deadline(delay));
}

/// The timers' helper: start the notifications whose retry is due at `now` (ns), and return when
/// the next pending one is due.
pub(crate) fn retry_due(now: usize) -> Option<usize> {
    let _g = LOCK.lock(&CSys);
    let mut next: Option<usize> = None;
    for r in records() {
        let (key, at) = (get(r, KEY), get(r, RETRY_AT));
        if key == 0 || at == 0 {
            continue;
        }
        if at <= now {
            put(r, RETRY_AT, 0);
            if start(r, key) {
                continue;
            }
            schedule(r);
        }
        let at = get(r, RETRY_AT);
        next = Some(next.map_or(at, |n| n.min(at)));
    }
    next
}

/// A notification thread: the registration's function with its value (the record is freed
/// first, so the function may register again).
extern "C" fn notification(key: usize) -> usize {
    let call = {
        let _g = LOCK.lock(&CSys);
        find(key).map(|r| {
            let fv = (get(r, FUNCTION), get(r, VALUE));
            release(r);
            fv
        })
    };
    if let Some((function, value)) = call {
        let _ = ffi::liella_machine_call3(function, i64::from_ne_bytes(value.to_ne_bytes()), 0, 0);
    }
    0
}

/// A notification of `start(arg)` that waited for its caller-supplied stack and then could not
/// be started (`crate::stacks`): for a message queue's, its record is kept and the notification
/// retried, as when the thread cannot be made at once ([`cookie_arrived`]; owner ruling #680,
/// D-25.5; the reference drops one whose thread it cannot create). The kernel's registration was
/// used up by the message: the queue takes a new one at once.
pub(crate) fn abandoned(start: usize, arg: usize) {
    if start != notification_addr() {
        return;
    }
    let g = LOCK.lock(&CSys);
    if let Some(r) = find(arg) {
        schedule(r);
        #[cfg(feature = "selftest")]
        ABANDONED.fetch_add(1, Relaxed);
        drop(g);
        timer::retry_soon();
    }
}

/// Message-queue notifications handed back by `crate::stacks` so far (`selftest` only).
#[cfg(feature = "selftest")]
static ABANDONED: AtomicUsize = AtomicUsize::new(0);

/// Message-queue notifications abandoned so far ([`abandoned`]; `selftest` only).
#[cfg(feature = "selftest")]
pub(crate) fn abandoned_count() -> usize {
    ABANDONED.load(Acquire)
}

/// A child of `fork`: no registration, no service thread, no socket. Async-signal-safe
/// (`_Fork`): no allocation (the records' attribute copies stay allocated), and the lock is
/// reset rather than taken.
pub(crate) fn after_fork_child() {
    LOCK.reset();
    let s = SOCKET.swap(-1, Relaxed);
    if s >= 0 {
        let _ = CSys.close(Fd::new(s));
    }
    for r in records() {
        put(r, KEY, 0);
    }
}

/// Tests only: how many records hold a registration (key not 0). A refused registration must
/// leave this unchanged (M3/M4).
#[cfg(feature = "selftest")]
pub(crate) fn records_in_use() -> usize {
    let _g = LOCK.lock(&CSys);
    records().filter(|r| get(r, KEY) != 0).count()
}

/// The shared-memory directory (D-25.2): `/dev/shm/` (in tests, the one `selftest::shm_dir`
/// names).
pub fn shm_dir() -> &'static [u8] {
    #[cfg(feature = "selftest")]
    if let Some(dir) = crate::selftest::shm_override() {
        return dir;
    }
    b"/dev/shm/"
}

/// `__mq_open_2(name, flags)`: `mq_open` without the optional arguments, which `O_CREAT`
/// needs: then `EINVAL`, creating nothing (the reference aborts).
pub fn open_2(name: KStr<'_>, flags: u32) -> KResult<Fd> {
    if flags & O_CREAT != 0 {
        return Err(Errno::EINVAL);
    }
    liella::ipc::mq_open(&CSys, name, flags, 0, None)
}

/// How many `struct mq_attr` `mq_open` reads at `attr`: one with `O_CREAT` and a non-NULL
/// pointer, none otherwise (without `O_CREAT` the argument may be anything).
pub fn creation_attrs(flags: u32, attr: *const MqAttr) -> i64 {
    i64::from(flags & O_CREAT != 0 && !attr.is_null())
}
