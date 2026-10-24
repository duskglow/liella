//! POSIX timers (spec §23.4; D-23.2, D-23.3): what a `timer_t` names, and the `SIGEV_THREAD`
//! notifications.
//!
//! **Ids.** A timer the kernel notifies (`SIGEV_SIGNAL`, `SIGEV_THREAD_ID`, `SIGEV_NONE`, or a
//! NULL `sigevent`) is the kernel's timer, and its `timer_t` is the kernel's id (0 to
//! `INT_MAX`): every call on it is the system call. A `SIGEV_THREAD` timer is a `timerfd`
//! (D-23.3, D-H18: no internal signal), and its `timer_t` is [`THREAD_TAG`] with a serial
//! number that is never reused, so the id of a deleted timer stays invalid (`EINVAL`). Any
//! other value is `EINVAL`.
//!
//! **Records.** A `SIGEV_THREAD` timer is a heap record on a list that only grows: a deleted
//! timer's record is marked free (serial 0) and reused by a later `timer_create`, never freed.
//! So `timer_settime`, `timer_gettime` and `timer_getoverrun`, which POSIX makes
//! async-signal-safe, find a record without the lock: they pin it (a user count), check its
//! serial again, and use its descriptor; `timer_delete` clears the serial and waits for the
//! pins to go before it closes the descriptor. Everything else (creation, deletion, the
//! helper, the notification threads) works under one lock.
//!
//! **Notifications.** One helper thread per process, made with the first `SIGEV_THREAD`
//! timer, waits on an epoll set of the timers' descriptors (`EPOLLONESHOT`). On an expiry it
//! reads the expiration count and starts a **new detached thread** for the notification (the
//! spec's "a new thread"; POSIX: as if the start routine of a thread created with
//! `sigev_notify_attributes`), which calls the function with `sigev_value`. The thread is made
//! with the timer's attributes (a copy taken at `timer_create`: stack size, stack address,
//! guard size, scheduling, affinity), always detached whatever the attributes say (POSIX
//! leaves a joinable one undefined; nobody could join it; owner ruling 2026-10-04), and with
//! every signal blocked (glibc's observed mask), so a notification thread never takes a process
//! signal. The helper does not count as a running thread (`thread::create_service`).
//!
//! **A caller-supplied stack** (`pthread_attr_setstack`) is used by every notification of the
//! timer, one at a time: the next notification thread starts only once the previous thread on
//! that stack (this timer's or any other notification's) has left it, the kernel having cleared
//! its TID word. `crate::stacks` keeps that rule for every notification path; the helper hands
//! it the notification and never waits for a stack itself.
//!
//! **One notification at a time per timer.** A timer's descriptor is armed again only when its
//! notification thread ends, however it ends (return, `pthread_exit`, cancellation:
//! [`thread_ending`] runs in the thread's exit sequence). Expirations meanwhile accumulate in
//! the descriptor's counter and become the next notification, whose overrun count
//! (`timer_getoverrun`) is the expirations beyond the first; `timer_settime` clears the count
//! (and drops expirations still owed by the previous setting), as the reference's timers do
//! whether the new setting arms or disarms (probe 2026-10-20). This is the rule POSIX gives
//! signals (one pending notification per timer, the rest counted as overruns); glibc instead
//! starts a thread for every expiration, so slow functions run concurrently (a deliberate
//! difference).
//!
//! **A notification thread that cannot be created** (a thread limit, memory) is retried, with
//! a back-off that doubles from 10 ms up to 1 s, until a thread starts or the timer is deleted
//! (owner ruling #269; glibc drops the expiry silently: a deliberate difference). Meanwhile the
//! descriptor stays disarmed and its expirations accumulate, so the notification that finally
//! starts counts them all in its overrun. `timer_settime` drops a pending retry (its
//! expirations were the previous setting's) and arms the descriptor for the new setting;
//! `timer_delete` cancels it. The retries are the helper's: it waits with a timeout only while
//! one is pending. Each failure episode (first refusal to the next start) is noted once, in a
//! counter a test reads (`selftest`); the library has no diagnostic channel to report it.
//! A notification that waited for its caller-supplied stack and then could not be started
//! (`crate::stacks` hands it back: [`abandoned`]) takes the same retry, its expirations owed:
//! re-arming the descriptor alone would lose a one-shot timer's notification for good, since
//! nothing more expires. The helper, then asleep in `epoll_pwait` without a timeout, is woken
//! through an `eventfd` in its set, made with the first timer whose attributes name a stack
//! (only those can be queued; the other timers' path is unchanged).
//!
//! **Message-queue and AIO notifications retry here too** (owner ruling #680, extending #269):
//! a `SIGEV_THREAD` notification of `mq_notify` or of an AIO request whose thread cannot be
//! started, at once or after waiting for its stack, keeps its record and is retried with the same
//! back-off ([`back_off`]) by this helper ([`retry_others`]): it is delivered late, never lost and
//! never twice (glibc drops it: the same deliberate difference). Each module keeps its pending
//! retries in its own records under its own lock and runs them when the helper asks
//! (`mqueue::retry_due`, `aio::retry_due`); a module that schedules one wakes the helper
//! ([`retry_soon`]). The helper and its wake-up descriptor are made with a module's first
//! `SIGEV_THREAD` registration or request ([`retry_helper`]), since a helper made only when a
//! thread was just refused would most likely be refused too. Until something fails, the helper
//! sleeps as before and the delivery paths are unchanged.
//!
//! **Deletion.** `timer_delete` takes the timer out of the set and frees its record under the
//! lock. A notification whose function has not started finds its timer gone and does nothing;
//! one whose function is running finishes (`timer_delete` does not wait for it: it may be that
//! function calling).
//!
//! **Clocks.** A `SIGEV_THREAD` timer's clock is first checked as the kernel's timers check
//! it (so the errors are those of the other notifications: `EINVAL`, `EOPNOTSUPP`, `EPERM`).
//! A clock the kernel's timers accept but a `timerfd` cannot measure (the CPU-time clocks,
//! `CLOCK_TAI`) is `ENOTSUP`, POSIX's error for an unsupported CPU-time timer (glibc accepts
//! them: a deliberate difference). The descriptor limits are `EAGAIN` (§23.4: resource
//! limits).
//!
//! **Processes.** Timers are not inherited (POSIX): a child of `fork` closes the descriptors
//! and starts with no `SIGEV_THREAD` timers and no helper (the parent's records stay
//! allocated: `_Fork`'s child may not use the heap's locks). The descriptors are close-on-exec.
//! Under another `ld.so` no thread can be created (D-22.10), so `SIGEV_THREAD` is `EAGAIN`
//! there.

use core::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering::{AcqRel, Acquire, Relaxed, Release, SeqCst}};

use liella_abi::layout::pthread_attr_t;
use liella_kernel::errno::{Errno, KResult};
use liella_kernel::fd::Fd;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{
    EpollEvent, Itimerspec, SigEvent, Timespec, CLOCK_MONOTONIC, EFD_CLOEXEC, EFD_NONBLOCK, EPOLLIN, EPOLLONESHOT, EPOLL_CLOEXEC, EPOLL_CTL_ADD, EPOLL_CTL_DEL, EPOLL_CTL_MOD,
    SIGEV_NONE, SIGEV_THREAD, TFD_CLOEXEC, TFD_NONBLOCK, TIMER_ABSTIME,
};
use liella_legacy::pthread_compat::ATTR_WORDS;
use liella_rt::sync::RawMutex;
use liella_rt::thread::Attr;

use crate::diag::{self, Topic};
use crate::sys::CSys;
use crate::{ffi, heap, pthread, stacks, tb, thread};

/// The tag of a `SIGEV_THREAD` timer's `timer_t` (above every kernel id); the bits below it
/// are the timer's serial number (from 1).
pub const THREAD_TAG: usize = 1 << 62;
const SERIAL_MASK: usize = THREAD_TAG - 1;

/// `DELAYTIMER_MAX` (`sysconf(_SC_DELAYTIMER_MAX)`): overrun counts saturate there.
const DELAYTIMER_MAX: usize = 0x7fff_ffff;

/// Every signal but the library's internal one (32, D-20.1), which no thread may block (the
/// setxid broadcast and cancellation need it).
const APP_SIGNALS: u64 = !(1 << 31);

/// The helper's stack: it runs only the library (epoll, thread creation).
const HELPER_STACK: usize = 128 << 10;

/// The first retry of a notification thread that could not be created comes after this long
/// (ns); each later one after twice the previous delay, up to [`RETRY_CEILING_NS`].
const RETRY_FIRST_NS: usize = 10_000_000;
/// The longest delay between two retries (ns): a permanent failure costs at most one attempt a
/// second per timer, and a recovered system is noticed within a second.
const RETRY_CEILING_NS: usize = 1_000_000_000;

/// Serialises creation, deletion, the helper and the notification threads.
static LOCK: RawMutex = RawMutex::new();
/// The newest record (heap blocks of [`WORDS`] words, linked through [`NEXT`], never freed).
static HEAD: AtomicUsize = AtomicUsize::new(0);
static SERIAL: AtomicUsize = AtomicUsize::new(0);
/// The helper's epoll descriptor; -1 until the first `SIGEV_THREAD` timer.
static EPOLL: AtomicI32 = AtomicI32::new(-1);
/// Some timer may have a retry pending (set and read by the helper, under the lock): only then
/// does the helper look for due retries and wait with a timeout.
static RETRYING: AtomicBool = AtomicBool::new(false);
/// A message-queue or AIO notification may have a retry pending ([`retry_soon`]): only then does
/// the helper ask those modules for their due retries.
static OTHERS: AtomicBool = AtomicBool::new(false);
/// The `eventfd` that wakes the helper for a retry scheduled by another thread ([`abandoned`]);
/// -1 until the first timer whose attributes name a caller-supplied stack.
static WAKE: AtomicI32 = AtomicI32::new(-1);
/// The epoll data of [`WAKE`]'s event: above every serial ([`SERIAL_MASK`]).
const WAKE_TOKEN: u64 = u64::MAX;

// A record's words. `SERIAL_NO` is 0 while the record is free, and is written last when a
// timer takes it (so the other words are in place for a lock-free reader that matches it).
const NEXT: usize = 0;
const SERIAL_NO: usize = 1;
/// Lock-free users pinning the record ([`pinned`]).
const USERS: usize = 2;
const FD: usize = 3;
const FUNCTION: usize = 4;
const VALUE: usize = 5;
/// The overrun count of the latest notification.
const OVERRUN: usize = 6;
/// Expirations read but not notified (a notification thread could not be created).
const MISSED: usize = 7;
/// The setting's generation, the settings in progress and the helper's read, in one word
/// ([`GENERATION`], [`PENDING`], [`READING`]), so the helper can tell an expiration count read
/// before a new setting from one read after it. A `timer_settime` counts itself in before its
/// `timerfd_settime` and, done, moves the generation and counts itself out (lock-free); the
/// helper reads the descriptor only while no setting is in progress, and a setting begun
/// during that read waits until it ends (one non-blocking `read` by a thread that runs no
/// signal handler: async-signal-safe). A count is then wholly one generation's (#544).
const SETS: usize = 8;
/// [`SETS`]: the helper is reading the descriptor.
const READING: usize = 1;
/// [`SETS`]: one `timer_settime` in progress (bits 1 to 31 count them).
const PENDING: usize = 1 << 1;
const PENDING_MASK: usize = (1 << 32) - PENDING;
/// [`SETS`]: one setting made (bits 32 up; compared for equality only).
const GENERATION: usize = 1 << 32;
/// How many times the helper yields to settings in progress before it puts the timer's read off
/// ([`READ_DEFERS`]).
const READ_YIELDS: usize = 64;
/// How many times in a row the helper puts a timer's read off for settings still in progress
/// after [`READ_YIELDS`] yields, as a retry ([`schedule_retry`]) whose delay doubles from
/// [`READ_DEFER_FIRST_NS`] up to [`READ_DEFER_CEILING_NS`] (about 2.3 s in all), before it reads
/// anyway. A setting can be in progress after its `timerfd_settime`, with its expiry already in
/// the descriptor: a count read then may be the new setting's, yet would be taken as the
/// previous one's and dropped when its thread is refused (SIGEV-TIMER-RACE: 64 yields were too
/// short for a setter preempted on a loaded host). The setting arms the descriptor again when
/// it ends (it takes the retry). Only a setting that never ends (its thread left it by
/// `longjmp` from a signal handler) meets the read made anyway: as before #544, its count is
/// then taken as the previous setting's.
const READ_DEFERS: usize = 16;
const READ_DEFER_FIRST_NS: usize = 1_000_000;
const READ_DEFER_CEILING_NS: usize = 256_000_000;
/// The generation [`MISSED`] was counted in: a later setting drops those expirations.
const MISSED_GEN: usize = 9;
/// The generation of the expirations the latest notification stands for.
const LAUNCH_GEN: usize = 10;
/// 1 while a notification that could not be started waits for a retry ([`retry_due`]); its
/// descriptor stays disarmed meanwhile. Cleared by whoever takes the retry: the helper, or
/// `timer_settime` (lock-free), which drops it.
const RETRY: usize = 11;
/// When the pending retry is due (`CLOCK_MONOTONIC`, ns).
const RETRY_AT: usize = 12;
/// The next retry's delay (ns); 0 outside a failure episode (no notification thread refused since
/// the last one started).
const BACKOFF: usize = 13;
/// A copy of the attribute object ([`pthread::attr_copy`]).
/// Reads put off in a row for settings in progress ([`READ_DEFERS`]); 0 once a read is made.
const DEFERS: usize = 14;
const ATTR: usize = 15;
const WORDS: usize = ATTR + ATTR_WORDS;

fn record(addr: usize) -> &'static [AtomicUsize] {
    tb::words64(addr, WORDS)
}

fn word(r: &[AtomicUsize], i: usize) -> usize {
    r.get(i).map_or(0, |w| w.load(Relaxed))
}

fn set(r: &[AtomicUsize], i: usize, v: usize) {
    if let Some(w) = r.get(i) {
        w.store(v, Relaxed);
    }
}

fn serial_of(r: &[AtomicUsize]) -> usize {
    r.get(SERIAL_NO).map_or(0, |w| w.load(SeqCst))
}

fn fd_of(r: &[AtomicUsize]) -> Fd {
    Fd::new(i32::try_from(word(r, FD)).unwrap_or(-1))
}

fn attrs(r: &[AtomicUsize]) -> &[AtomicUsize] {
    r.get(ATTR..).unwrap_or(&[])
}

fn next_of(r: &[AtomicUsize]) -> usize {
    r.get(NEXT).map_or(0, |w| w.load(Acquire))
}

// ------------------------------------------------------------------------------ ids

/// What a `timer_t` names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Id {
    /// A kernel timer.
    Kernel(i32),
    /// A `SIGEV_THREAD` timer: its serial number.
    Thread(usize),
}

/// The timer a `timer_t` value names; `EINVAL` for a value no timer can have.
pub fn id(t: usize) -> KResult<Id> {
    if let Ok(k) = i32::try_from(t) {
        return Ok(Id::Kernel(k));
    }
    match t & SERIAL_MASK {
        s if t & !SERIAL_MASK == THREAD_TAG && s != 0 => Ok(Id::Thread(s)),
        _ => Err(Errno::EINVAL),
    }
}

fn kernel_id(k: i32) -> usize {
    usize::try_from(k).unwrap_or(0)
}

// ------------------------------------------------------------------------------ the calls

/// The attribute object a `sigevent` names for `SIGEV_THREAD` (`sigev_notify_attributes`);
/// NULL for any other notification, whose union holds something else.
pub fn attributes(event: Option<&SigEvent>) -> *const pthread_attr_t {
    match event {
        Some(e) if e.notify == SIGEV_THREAD => {
            let [_, attr] = e.thread_words();
            core::ptr::with_exposed_provenance(usize::try_from(attr).unwrap_or(0))
        }
        _ => core::ptr::null(),
    }
}

/// `timer_create(clock, event, &id)`: the new timer's `timer_t`. `attr` is the event's
/// [`attributes`] object, as words.
pub fn create(clock: i32, event: Option<&SigEvent>, attr: KResult<Option<&[AtomicUsize]>>) -> KResult<usize> {
    match event {
        Some(e) if e.notify == SIGEV_THREAD => create_thread(clock, e, attr?),
        _ => CSys.timer_create(clock, event).map(kernel_id),
    }
}

/// `timer_delete`.
pub fn delete(t: usize) -> KResult<()> {
    match id(t)? {
        Id::Kernel(k) => CSys.timer_delete(k),
        Id::Thread(serial) => {
            let _g = LOCK.lock(&CSys);
            let r = record(find(serial).ok_or(Errno::EINVAL)?);
            if let Some(w) = r.get(SERIAL_NO) {
                w.store(0, SeqCst);
            }
            // A pending retry is cancelled (owner ruling #269).
            set(r, RETRY, 0);
            // Wait out the lock-free users that matched the serial before it was cleared.
            while r.get(USERS).is_some_and(|w| w.load(SeqCst) != 0) {
                CSys.sched_yield();
            }
            if let Some(ep) = epoll() {
                let _ = CSys.epoll_ctl(ep, EPOLL_CTL_DEL, fd_of(r), None);
            }
            let _ = CSys.close(fd_of(r));
            pthread::attr_release(attrs(r));
            Ok(())
        }
    }
}

/// `timer_settime(t, flags, new, &old)`: the previous setting. Flags other than
/// `TIMER_ABSTIME` are ignored (as the kernel's timers ignore them); a NULL setting is
/// `EINVAL`. Async-signal-safe.
pub fn settime(t: usize, flags: i32, new: Option<&Itimerspec>) -> KResult<Itimerspec> {
    let new = new.ok_or(Errno::EINVAL)?;
    match id(t)? {
        Id::Kernel(k) => CSys.timer_settime(k, u32::from_ne_bytes(flags.to_ne_bytes()), new),
        Id::Thread(serial) => settime_thread(serial, flags, new),
    }
}

/// [`settime`] of a `SIGEV_THREAD` timer, out of line so the kernel timers' path keeps its own
/// small frame. Async-signal-safe.
#[inline(never)]
fn settime_thread(serial: usize, flags: i32, new: &Itimerspec) -> KResult<Itimerspec> {
    let abs = (flags & TIMER_ABSTIME).unsigned_abs();
    pinned(serial, |r| {
        let sets = r.get(SETS).ok_or(Errno::EINVAL)?;
        // In progress from here ([`SETS`]); a helper inside its read is waited for.
        let was = sets.fetch_add(PENDING, SeqCst);
        trace(1, was);
        #[cfg(feature = "selftest")]
        if was & READING != 0 {
            SETS_MET_READS.fetch_add(1, AcqRel);
        }
        #[cfg(feature = "selftest")]
        held_set();
        if was & READING != 0 {
            while sets.load(SeqCst) & READING != 0 {
                CSys.sched_yield();
            }
        }
        let old = match CSys.timerfd_settime(fd_of(r), abs, new) {
            Ok(old) => old,
            Err(e) => {
                sets.fetch_sub(PENDING, SeqCst);
                return Err(e);
            }
        };
        trace(2, 0);
        #[cfg(feature = "selftest")]
        held_set_late();
        // A notification waiting for a retry is dropped (owner ruling #269): its
        // expirations were the previous setting's. Whoever clears the flag owns the
        // descriptor, so it is armed again here for the new setting. This is done while the
        // setting is still in progress, before the generation moves: until then the helper
        // reads nothing of the new setting (it puts its reads off), so a retry taken here is
        // a put-off read or a previous setting's. Taken after the move, it could be the new
        // setting's own refused expiry, already read from the descriptor: dropped with it, the
        // setting was never notified (SIGEV-TIMER-RACE-2, part 1 of
        // `timer_set_while_the_helper_reads` under load).
        let took = r.get(RETRY).is_some_and(|w| w.swap(0, SeqCst) != 0);
        trace(4, usize::from(took));
        if took {
            if let Some(ep) = epoll() {
                let _ = arm(ep, fd_of(r), serial, EPOLL_CTL_MOD);
            }
        }
        // A setting, arming or disarming, clears the overrun count, as the reference's
        // timers do. The generation moves first: a helper that read the expirations of the
        // previous setting and stores their count after this point clears it again
        // ([`expired`]).
        let moved_to = sets.fetch_add(GENERATION - PENDING, SeqCst);
        trace(3, moved_to);
        if let Some(w) = r.get(OVERRUN) {
            w.store(0, SeqCst);
        }
        #[cfg(feature = "selftest")]
        held_set_done();
        Ok(old)
    })
}

/// `timer_gettime`. Async-signal-safe.
pub fn gettime(t: usize) -> KResult<Itimerspec> {
    match id(t)? {
        Id::Kernel(k) => CSys.timer_gettime(k),
        Id::Thread(serial) => pinned(serial, |r| CSys.timerfd_gettime(fd_of(r))),
    }
}

/// `timer_getoverrun`: the expirations beyond the first that the latest notification stood
/// for, at most `DELAYTIMER_MAX`. Async-signal-safe.
pub fn getoverrun(t: usize) -> KResult<i32> {
    match id(t)? {
        Id::Kernel(k) => CSys.timer_getoverrun(k),
        Id::Thread(serial) => pinned(serial, |r| Ok(i32::try_from(r.get(OVERRUN).map_or(0, |w| w.load(Acquire))).unwrap_or(i32::MAX))),
    }
}

/// `f` on the record of the live timer `serial`, without the lock: the record is pinned while
/// `f` runs, so its descriptor stays open.
fn pinned<T>(serial: usize, f: impl FnOnce(&[AtomicUsize]) -> KResult<T>) -> KResult<T> {
    let mut a = HEAD.load(Acquire);
    while a != 0 {
        let r = record(a);
        if serial_of(r) == serial {
            let users = r.get(USERS).ok_or(Errno::EINVAL)?;
            users.fetch_add(1, SeqCst);
            // Deleted in between: the serial is gone (serials are never reused).
            let out = if serial_of(r) == serial { f(r) } else { Err(Errno::EINVAL) };
            users.fetch_sub(1, SeqCst);
            return out;
        }
        a = next_of(r);
    }
    Err(Errno::EINVAL)
}

// ------------------------------------------------------------------------------ SIGEV_THREAD

fn create_thread(clock: i32, e: &SigEvent, attr: Option<&[AtomicUsize]>) -> KResult<usize> {
    let [function, _] = e.thread_words();
    // No function: nothing could be notified (glibc accepts it and crashes at the expiry).
    let function = usize::try_from(function).map_err(|_| Errno::EINVAL)?;
    if function == 0 {
        return Err(Errno::EINVAL);
    }
    // The clock as the kernel's timers check it.
    let probe = CSys.timer_create(clock, Some(&SigEvent::new(SIGEV_NONE, 0, 0, 0)))?;
    let _ = CSys.timer_delete(probe);
    let flags = (TFD_NONBLOCK | TFD_CLOEXEC).unsigned_abs();
    let fd = CSys.timerfd_create(clock, flags).map_err(|e| match e {
        Errno::EINVAL => Errno::EOPNOTSUPP,
        e => resource(e),
    })?;
    // Kept until `timer_delete`: never a standard descriptor (validation T12).
    let fd = liella::io::above_standard(&CSys, fd).map_err(resource)?;
    let made = {
        let _g = LOCK.lock(&CSys);
        publish(fd, function, usize::try_from(e.value).unwrap_or(0), attr)
    };
    if made.is_err() {
        let _ = CSys.close(fd);
    }
    made
}

/// Descriptor limits are resource limits (§23.4: `EAGAIN`).
fn resource(e: Errno) -> Errno {
    if e == Errno::EMFILE || e == Errno::ENFILE { Errno::EAGAIN } else { e }
}

/// Under the lock: take a free record (or a new one), start the helper if needed, fill the
/// record, arm its descriptor and give it a serial; the new `timer_t`. On failure the record
/// stays free.
fn publish(fd: Fd, function: usize, value: usize, attr: Option<&[AtomicUsize]>) -> KResult<usize> {
    let r = record(free_record()?);
    let ep = helper()?;
    set(r, FD, usize::try_from(fd.raw()).unwrap_or(0));
    set(r, FUNCTION, function);
    set(r, VALUE, value);
    set(r, OVERRUN, 0);
    set(r, MISSED, 0);
    set(r, SETS, 0);
    set(r, MISSED_GEN, 0);
    set(r, LAUNCH_GEN, 0);
    set(r, RETRY, 0);
    set(r, RETRY_AT, 0);
    set(r, BACKOFF, 0);
    set(r, DEFERS, 0);
    pthread::attr_copy(attr, attrs(r))?;
    // Only a notification on a caller-supplied stack can be queued and then abandoned
    // ([`abandoned`]), which needs a way to wake the helper.
    if pthread::attr_decode(attrs(r)).stack.is_some() {
        if let Err(e) = waker(ep) {
            pthread::attr_release(attrs(r));
            return Err(resource(e));
        }
    }
    let serial = SERIAL.fetch_add(1, Relaxed).wrapping_add(1) & SERIAL_MASK;
    if let Err(e) = arm(ep, fd, serial, EPOLL_CTL_ADD) {
        pthread::attr_release(attrs(r));
        return Err(resource(e));
    }
    if let Some(w) = r.get(SERIAL_NO) {
        w.store(serial, SeqCst);
    }
    Ok(THREAD_TAG | serial)
}

/// Under the lock: a free record, from the list or newly listed.
fn free_record() -> KResult<usize> {
    let mut a = HEAD.load(Acquire);
    while a != 0 {
        let r = record(a);
        if serial_of(r) == 0 {
            return Ok(a);
        }
        a = next_of(r);
    }
    let a = liella_legacy::malloc_compat::malloc(&heap::CTX, WORDS.saturating_mul(8)).map_err(|_| Errno::ENOMEM)?;
    let r = record(a);
    for w in r {
        w.store(0, Relaxed);
    }
    set(r, NEXT, HEAD.load(Relaxed));
    HEAD.store(a, Release);
    Ok(a)
}

fn epoll() -> Option<Fd> {
    let ep = EPOLL.load(Acquire);
    (ep >= 0).then(|| Fd::new(ep))
}

/// Under the lock: the helper's epoll descriptor, starting the helper the first time.
fn helper() -> KResult<Fd> {
    if let Some(ep) = epoll() {
        return Ok(ep);
    }
    // Kept for the life of the process: never a standard descriptor (validation T12).
    let ep = CSys.epoll_create1(EPOLL_CLOEXEC).and_then(|ep| liella::io::above_standard(&CSys, ep)).map_err(resource)?;
    let a = Attr { detached: true, stack_size: HELPER_STACK, sigmask: Some(APP_SIGNALS), ..Attr::default() };
    if let Err(e) = thread::create_service(&a, helper_addr(), usize::try_from(ep.raw()).unwrap_or(0)) {
        let _ = CSys.close(ep);
        return Err(e);
    }
    EPOLL.store(ep.raw(), Release);
    Ok(ep)
}

/// Under the lock: the helper's wake-up descriptor ([`WAKE`]), made and added to its set the
/// first time.
fn waker(ep: Fd) -> KResult<Fd> {
    let fd = WAKE.load(Relaxed);
    if fd >= 0 {
        return Ok(Fd::new(fd));
    }
    // Kept for the life of the process: never a standard descriptor (validation T12).
    let fd = CSys.eventfd2(0, EFD_CLOEXEC | EFD_NONBLOCK).and_then(|fd| liella::io::above_standard(&CSys, fd))?;
    let ev = EpollEvent { events: EPOLLIN, data: WAKE_TOKEN };
    if let Err(e) = CSys.epoll_ctl(ep, EPOLL_CTL_ADD, fd, Some(&ev)) {
        let _ = CSys.close(fd);
        return Err(e);
    }
    WAKE.store(fd.raw(), Release);
    Ok(fd)
}

/// (Re)arm a timer's descriptor in the helper's set: one event, then disarmed.
fn arm(ep: Fd, fd: Fd, serial: usize, op: u32) -> KResult<()> {
    let ev = EpollEvent { events: EPOLLIN | EPOLLONESHOT, data: u64::try_from(serial).unwrap_or(0) };
    CSys.epoll_ctl(ep, op, fd, Some(&ev))
}

/// Under the lock: the record of the live timer `serial`.
fn find(serial: usize) -> Option<usize> {
    let mut a = HEAD.load(Acquire);
    while a != 0 {
        let r = record(a);
        if serial_of(r) == serial {
            return Some(a);
        }
        a = next_of(r);
    }
    None
}

/// The address of [`helper_main`].
#[allow(clippy::as_conversions)] // a function pointer's address: the only spelling Rust has
fn helper_addr() -> usize {
    helper_main as extern "C" fn(usize) -> usize as usize
}

/// The address of [`notify`].
#[allow(clippy::as_conversions)] // a function pointer's address: the only spelling Rust has
fn notify_addr() -> usize {
    notify as extern "C" fn(usize) -> usize as usize
}

/// The helper thread: waits for expirations for as long as the process runs.
extern "C" fn helper_main(ep: usize) -> usize {
    let ep = Fd::new(i32::try_from(ep).unwrap_or(-1));
    let mut events = [EpollEvent::default(); 16];
    loop {
        // Without a pending retry the helper sleeps until an expiry, as before.
        let mut timeout = if RETRYING.load(Relaxed) { retry_due(ep) } else { -1 };
        if OTHERS.swap(false, SeqCst) {
            timeout = match (timeout, retry_others()) {
                (-1, t) | (t, -1) => t,
                (a, b) => a.min(b),
            };
        }
        match CSys.epoll_pwait(ep, &mut events, timeout, None) {
            Ok(n) => {
                for e in events.iter().take(n) {
                    if e.data == WAKE_TOKEN {
                        // A retry scheduled elsewhere ([`abandoned`], [`retry_soon`]): the next
                        // pass runs it.
                        let _ = CSys.read(Fd::new(WAKE.load(Acquire)), &mut [0u8; 8]);
                    } else {
                        expired(ep, usize::try_from(e.data).unwrap_or(0));
                    }
                }
            }
            Err(Errno::EINTR) => {}
            // The set is unusable (the application closed the descriptor): do not spin.
            Err(_) => {
                let _ = CSys.clock_nanosleep(CLOCK_MONOTONIC, 0, &Timespec { sec: 1, nsec: 0 }, None);
            }
        }
    }
}

/// The helper, for an event of the timer `serial`: read the expirations and start the
/// notification thread (or arm the descriptor again when there is nothing to notify).
fn expired(ep: Fd, serial: usize) {
    let _g = LOCK.lock(&CSys);
    let found = find(serial);
    trace(20, usize::from(found.is_some()));
    let Some(rec) = found else { return };
    deliver(ep, serial, record(rec));
}

/// The last [`TRACE_LEN`] events of the `SIGEV_THREAD` timers' setters and helper, each
/// `code << 32 | value` (`selftest` only: a test prints them when a notification is lost).
#[cfg(feature = "selftest")]
const TRACE_LEN: usize = 64;
#[cfg(feature = "selftest")]
static TRACE: [AtomicUsize; TRACE_LEN] = [const { AtomicUsize::new(0) }; TRACE_LEN];
#[cfg(feature = "selftest")]
static TRACE_AT: AtomicUsize = AtomicUsize::new(0);

/// Record an event in [`TRACE`] (`selftest` only; nothing otherwise).
#[cfg(feature = "selftest")]
fn trace(code: usize, value: usize) {
    let i = TRACE_AT.fetch_add(1, AcqRel);
    if let Some(w) = TRACE.get(i & (TRACE_LEN - 1)) {
        w.store(code.wrapping_shl(32) | (value & 0xffff_ffff), Release);
    }
}

#[cfg(not(feature = "selftest"))]
#[inline(always)]
fn trace(_: usize, _: usize) {}

/// (events recorded so far, the [`TRACE`] ring) (`selftest` only).
#[cfg(feature = "selftest")]
pub(crate) fn trace_ring() -> (usize, [usize; TRACE_LEN]) {
    (TRACE_AT.load(Acquire), core::array::from_fn(|i| TRACE.get(i).map_or(0, |w| w.load(Acquire))))
}

/// Under the lock, for a timer whose descriptor is disarmed (an event, or a due retry): read the
/// expirations and start the notification thread for them and any still owed; when it cannot be
/// started, schedule a retry ([`schedule_retry`]); with nothing to notify, arm the descriptor.
fn deliver(ep: Fd, serial: usize, r: &[AtomicUsize]) {
    let fd = fd_of(r);
    // The generation of the count read ([`SETS`]): if it moves (or a setting begins) before the
    // count is stored, the count is that of a previous setting.
    let Some(generation) = begin_read(r) else {
        // A setting still in progress: its expiry may already be in the descriptor. The read
        // is put off, the descriptor left disarmed with its count; the setting, once done,
        // takes the retry and arms it again, or the retry reads.
        let n = word(r, DEFERS);
        trace(10, n);
        set(r, DEFERS, n.saturating_add(1));
        schedule_retry(r, READ_DEFER_FIRST_NS.checked_shl(u32::try_from(n).unwrap_or(u32::MAX)).unwrap_or(usize::MAX).min(READ_DEFER_CEILING_NS));
        return;
    };
    #[cfg(feature = "selftest")]
    held_read();
    let mut buf = [0u8; 8];
    // Nothing to read: the timer was set again since the event, or nothing expired since a
    // refused notification.
    let count = match CSys.read(fd, &mut buf) {
        Ok(8) => usize::try_from(u64::from_ne_bytes(buf)).unwrap_or(usize::MAX),
        _ => 0,
    };
    if let Some(w) = r.get(SETS) {
        w.fetch_and(!READING, SeqCst);
    }
    trace(11, generation);
    trace(12, count);
    // Expirations of an earlier setting are not this setting's overruns.
    let missed = if word(r, MISSED_GEN) == generation { word(r, MISSED) } else { 0 };
    let due = count.saturating_add(missed);
    if due == 0 {
        trace(13, 0);
        set(r, MISSED, 0);
        let _ = arm(ep, fd, serial, EPOLL_CTL_MOD);
        return;
    }
    // The thread takes the lock before it reads the record, so the overrun count is in place
    // before the function can ask for it. On a caller-supplied stack it may start later
    // (`crate::stacks`); the descriptor stays disarmed until it ends.
    match stacks::launch(attrs(r), APP_SIGNALS, notify_addr(), serial) {
        Ok(_) => {
            trace(14, due);
            if let Some(w) = r.get(OVERRUN) {
                w.store(due.saturating_sub(1).min(DELAYTIMER_MAX), SeqCst);
                // A `timer_settime` since the read: the notification is of the old setting and
                // leaves the count cleared, as a signal of the reference's timers does when it is
                // delivered after the timer was set again.
                if moved(r, generation) {
                    w.store(0, SeqCst);
                }
            }
            set(r, MISSED, 0);
            set(r, LAUNCH_GEN, generation);
            recovered(r, BACKOFF, Notifier::Timer);
        }
        Err(_) => {
            let delay = back_off(r, BACKOFF, Notifier::Timer);
            // A `timer_settime` came during the attempt (`timer_notification_retried`, step 5):
            // it found no retry to drop and so armed nothing. What was owed is the previous
            // setting's: dropped, as a pending retry is, and the descriptor armed for the new
            // setting, whose expiry is then notified when it comes, not at a retry up to a
            // second away.
            if moved(r, generation) {
                trace(15, word(r, SETS));
                set(r, MISSED, 0);
                let _ = arm(ep, fd, serial, EPOLL_CTL_MOD);
                return;
            }
            trace(16, delay / 1_000_000);
            set(r, MISSED, due);
            set(r, MISSED_GEN, generation);
            schedule_retry(r, delay);
        }
    }
}

/// Under the lock, before the helper reads the timer's descriptor: mark the read ([`READING`])
/// once no setting is in progress, and return the generation the count read will be (the
/// [`SETS`] word, which a later setting changes); `None`: the read is put off for a setting
/// still in progress ([`READ_DEFERS`]).
fn begin_read(r: &[AtomicUsize]) -> Option<usize> {
    let Some(w) = r.get(SETS) else { return Some(0) };
    for _ in 0..READ_YIELDS {
        let was = w.fetch_or(READING, SeqCst);
        if was & PENDING_MASK == 0 {
            set(r, DEFERS, 0);
            return Some(was);
        }
        w.fetch_and(!READING, SeqCst);
        #[cfg(feature = "selftest")]
        DEFERRED_READS.fetch_add(1, AcqRel);
        CSys.sched_yield();
    }
    if word(r, DEFERS) < READ_DEFERS {
        return None;
    }
    set(r, DEFERS, 0);
    #[cfg(feature = "selftest")]
    FORCED_READS.fetch_add(1, AcqRel);
    Some(w.fetch_or(READING, SeqCst) & !READING)
}

/// The timer was set, or a setting began, since [`begin_read`] returned `generation`: the count
/// read is the previous setting's.
fn moved(r: &[AtomicUsize], generation: usize) -> bool {
    r.get(SETS).is_some_and(|w| w.load(SeqCst) & !READING != generation)
}

/// While set, the helper waits between [`begin_read`] and its read (`selftest` only): a test
/// sets the timer in that window.
#[cfg(feature = "selftest")]
static HOLD_READ: AtomicBool = AtomicBool::new(false);
/// Reads [`HOLD_READ`] has held so far (`selftest` only).
#[cfg(feature = "selftest")]
static HELD_READS: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "selftest")]
fn held_read() {
    if HOLD_READ.load(Acquire) {
        HELD_READS.fetch_add(1, AcqRel);
        while HOLD_READ.load(Acquire) {
            CSys.sched_yield();
        }
    }
}

/// Hold the helper's reads ([`HOLD_READ`]) or let them go on (`selftest` only).
#[cfg(feature = "selftest")]
pub(crate) fn hold_reads(on: bool) {
    HOLD_READ.store(on, Release);
}

/// The helper's reads held so far (`selftest` only).
#[cfg(feature = "selftest")]
pub(crate) fn held_reads() -> usize {
    HELD_READS.load(Acquire)
}

/// While set, a `timer_settime` of a `SIGEV_THREAD` timer waits after counting itself in and
/// before its `timerfd_settime` (`selftest` only): a test has the helper meet a setting in
/// progress.
#[cfg(feature = "selftest")]
static HOLD_SET: AtomicBool = AtomicBool::new(false);
/// Settings [`HOLD_SET`] has held so far (`selftest` only).
#[cfg(feature = "selftest")]
static HELD_SETS: AtomicUsize = AtomicUsize::new(0);
/// Times the helper put its read off for a setting in progress ([`begin_read`]; `selftest` only).
#[cfg(feature = "selftest")]
static DEFERRED_READS: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "selftest")]
fn held_set() {
    if HOLD_SET.load(Acquire) {
        HELD_SETS.fetch_add(1, AcqRel);
        while HOLD_SET.load(Acquire) {
            CSys.sched_yield();
        }
    }
}

/// Hold settings in progress ([`HOLD_SET`]) or let them go on (`selftest` only).
#[cfg(feature = "selftest")]
pub(crate) fn hold_sets(on: bool) {
    HOLD_SET.store(on, Release);
}

/// While set, a `timer_settime` of a `SIGEV_THREAD` timer waits after its `timerfd_settime`
/// and before it is done (`selftest` only): the new setting's expiry is in the descriptor while
/// the setting is still in progress (SIGEV-TIMER-RACE).
#[cfg(feature = "selftest")]
static HOLD_SET_LATE: AtomicBool = AtomicBool::new(false);
/// Reads the helper made anyway with a setting in progress ([`READ_DEFERS`]; `selftest` only).
#[cfg(feature = "selftest")]
static FORCED_READS: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "selftest")]
fn held_set_late() {
    if HOLD_SET_LATE.load(Acquire) {
        HELD_SETS.fetch_add(1, AcqRel);
        while HOLD_SET_LATE.load(Acquire) {
            CSys.sched_yield();
        }
    }
}

/// Hold settings after their `timerfd_settime` ([`HOLD_SET_LATE`]) or let them go on
/// (`selftest` only).
#[cfg(feature = "selftest")]
pub(crate) fn hold_sets_late(on: bool) {
    HOLD_SET_LATE.store(on, Release);
}

/// While set, a `timer_settime` of a `SIGEV_THREAD` timer waits after it is done with the
/// timer's state and before it returns (`selftest` only): the helper reads and handles the new
/// setting's expiry meanwhile, so a retry it schedules is the new setting's own
/// (SIGEV-TIMER-RACE-2: a setter that took the retry after this point dropped it).
#[cfg(feature = "selftest")]
static HOLD_SET_DONE: AtomicBool = AtomicBool::new(false);
/// Settings that counted themselves in while the helper was inside a read (`selftest` only).
#[cfg(feature = "selftest")]
static SETS_MET_READS: AtomicUsize = AtomicUsize::new(0);

#[cfg(feature = "selftest")]
fn held_set_done() {
    if HOLD_SET_DONE.load(Acquire) {
        HELD_SETS.fetch_add(1, AcqRel);
        while HOLD_SET_DONE.load(Acquire) {
            CSys.sched_yield();
        }
    }
}

/// Hold settings once done with the timer's state ([`HOLD_SET_DONE`]) or let them return
/// (`selftest` only).
#[cfg(feature = "selftest")]
pub(crate) fn hold_sets_done(on: bool) {
    HOLD_SET_DONE.store(on, Release);
}

/// Settings that met the helper inside a read so far ([`SETS_MET_READS`]; `selftest` only).
#[cfg(feature = "selftest")]
pub(crate) fn sets_met_reads() -> usize {
    SETS_MET_READS.load(Acquire)
}

/// Reads made anyway with a setting in progress so far ([`FORCED_READS`]; `selftest` only).
#[cfg(feature = "selftest")]
pub(crate) fn forced_reads() -> usize {
    FORCED_READS.load(Acquire)
}

/// Runs `body` with the timers' lock held (`selftest::with_all_locks` only).
#[cfg(feature = "selftest")]
pub(crate) fn with_lock<R>(body: impl FnOnce() -> R) -> R {
    let _g = LOCK.lock(&CSys);
    body()
}

/// (settings held, reads deferred) so far (`selftest` only).
#[cfg(feature = "selftest")]
pub(crate) fn held_sets() -> (usize, usize) {
    (HELD_SETS.load(Acquire), DEFERRED_READS.load(Acquire))
}

/// Whose notification a retry is (owner rulings #269, #680): each keeps its own records and
/// failure episodes; the back-off and the helper running the retries are shared.
#[derive(Clone, Copy)]
pub(crate) enum Notifier {
    Timer,
    Mq,
    Aio,
}

impl Notifier {
    #[cfg(feature = "selftest")]
    fn index(self) -> usize {
        match self {
            Notifier::Timer => 0,
            Notifier::Mq => 1,
            Notifier::Aio => 2,
        }
    }
}

/// Under the owner's lock, for a notification whose thread could not be created (owner rulings
/// #269, #680): the delay before the next attempt, kept in `r[i]`, which doubles from
/// [`RETRY_FIRST_NS`] up to [`RETRY_CEILING_NS`] within a failure episode (the first refusal
/// begins one).
pub(crate) fn back_off(r: &[AtomicUsize], i: usize, who: Notifier) -> usize {
    let backoff = word(r, i);
    if backoff == 0 {
        note_episode(who, Episode::Refused);
    }
    let delay = backoff.clamp(RETRY_FIRST_NS, RETRY_CEILING_NS);
    set(r, i, delay.saturating_mul(2).min(RETRY_CEILING_NS));
    delay
}

/// Under the owner's lock, once a notification's thread has started: a failure episode
/// ([`back_off`] in `r[i]`) ends.
pub(crate) fn recovered(r: &[AtomicUsize], i: usize, who: Notifier) {
    if word(r, i) != 0 {
        set(r, i, 0);
        note_episode(who, Episode::Recovered);
    }
}

/// When a retry `delay` ns from now is due (`CLOCK_MONOTONIC`, ns; never 0, so 0 can mean "no
/// retry pending").
pub(crate) fn retry_deadline(delay: usize) -> usize {
    now_ns().saturating_add(delay).max(1)
}

/// The helper and its wake-up descriptor, made now if they are not yet: a message-queue or AIO
/// module calls this with its first `SIGEV_THREAD` registration or request (one load once they
/// exist), so a retry needs no thread to be made after a refusal.
pub(crate) fn retry_helper() -> KResult<()> {
    if WAKE.load(Acquire) >= 0 {
        return Ok(());
    }
    let _g = LOCK.lock(&CSys);
    let ep = helper()?;
    waker(ep).map(drop).map_err(resource)
}

/// A message-queue or AIO module has scheduled a retry (outside its lock): wake the helper, which
/// then asks it for its due retries ([`retry_others`]).
pub(crate) fn retry_soon() {
    OTHERS.store(true, SeqCst);
    let wake = WAKE.load(Acquire);
    if wake >= 0 {
        let _ = CSys.write(Fd::new(wake), &1u64.to_ne_bytes());
    }
}

/// The helper, without the timers' lock: run the message-queue and AIO retries that are due and
/// return how long it may wait (ms) before the next one, or -1 (none pending).
fn retry_others() -> i32 {
    let now = now_ns();
    let next = match (crate::mqueue::retry_due(now), crate::aio::retry_due(now)) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (a, b) => a.or(b),
    };
    next.map_or(-1, |at| {
        // Still pending: the next pass asks again.
        OTHERS.store(true, SeqCst);
        i32::try_from(at.saturating_sub(now_ns()).div_ceil(1_000_000)).unwrap_or(i32::MAX)
    })
}

/// Under the lock, after [`back_off`]: keep the expirations owed and the descriptor disarmed,
/// and have the helper try again after `delay` ns. Expirations meanwhile stay in the
/// descriptor's counter and join the notification when it starts.
fn schedule_retry(r: &[AtomicUsize], delay: usize) {
    set(r, RETRY_AT, now_ns().saturating_add(delay));
    if let Some(w) = r.get(RETRY) {
        w.store(1, SeqCst);
    }
    RETRYING.store(true, Relaxed);
}

/// The helper, before it waits: run the retries that are due and return how long it may wait
/// (ms) before the next one, or -1 (no retry pending: wait for an expiry only).
fn retry_due(ep: Fd) -> i32 {
    let _g = LOCK.lock(&CSys);
    RETRYING.store(false, Relaxed);
    let now = now_ns();
    let mut next: Option<usize> = None;
    let mut a = HEAD.load(Acquire);
    while a != 0 {
        let r = record(a);
        let serial = serial_of(r);
        if serial != 0 && word(r, RETRY) != 0 {
            // `timer_settime` may take the retry at the same moment: the one that clears the flag
            // owns the descriptor.
            if word(r, RETRY_AT) <= now && r.get(RETRY).is_some_and(|w| w.swap(0, SeqCst) != 0) {
                trace(21, 0);
                deliver(ep, serial, r);
            }
            if word(r, RETRY) != 0 {
                RETRYING.store(true, Relaxed);
                let at = word(r, RETRY_AT);
                next = Some(next.map_or(at, |n| n.min(at)));
            }
        }
        a = next_of(r);
    }
    next.map_or(-1, |at| i32::try_from(at.saturating_sub(now_ns()).div_ceil(1_000_000)).unwrap_or(i32::MAX))
}

/// `CLOCK_MONOTONIC` in ns.
fn now_ns() -> usize {
    CSys.clock_gettime(CLOCK_MONOTONIC).map_or(0, |t| {
        let s = usize::try_from(t.sec).unwrap_or(0);
        s.saturating_mul(1_000_000_000).saturating_add(usize::try_from(t.nsec).unwrap_or(0))
    })
}

/// A failure episode of a timer's notification threads: the first refusal, and the start that
/// ends it (once each, however many retries come between).
#[derive(Clone, Copy)]
enum Episode {
    Refused,
    Recovered,
}

/// Episodes seen so far, refused and recovered, per [`Notifier`] (`selftest` only: counters a
/// test reads).
#[cfg(feature = "selftest")]
static EPISODES: [[AtomicUsize; 2]; 3] = [const { [AtomicUsize::new(0), AtomicUsize::new(0)] }; 3];

/// Note an episode's start or end (called on the failure path and on the first start after it
/// only): counted in `selftest` builds, and for a timer reported on the diagnostic channel's
/// `timers` topic (`LIELLA_DIAG=timers`, D-2.9, owner ruling #422) when it is on.
fn note_episode(who: Notifier, e: Episode) {
    #[cfg(feature = "selftest")]
    if let Some(c) = EPISODES.get(who.index()).and_then(|k| {
        k.get(match e {
            Episode::Refused => 0,
            Episode::Recovered => 1,
        })
    }) {
        c.fetch_add(1, SeqCst);
    }
    if !matches!(who, Notifier::Timer) {
        return;
    }
    diag::note(Topic::TIMERS, match e {
        Episode::Refused => b"a SIGEV_THREAD timer's notification thread cannot be created (thread or memory starvation?); retrying with back-off",
        Episode::Recovered => b"a SIGEV_THREAD timer's notification thread started again after a failure episode",
    });
}

/// The notification-thread failure episodes so far: (begun, recovered) (`selftest` only).
#[cfg(feature = "selftest")]
pub(crate) fn episodes(who: Notifier) -> (usize, usize) {
    EPISODES.get(who.index()).map_or((0, 0), |[a, b]| (a.load(SeqCst), b.load(SeqCst)))
}

/// A notification thread's start routine: the timer's function with its value, unless the
/// timer was deleted since the expiry.
extern "C" fn notify(serial: usize) -> usize {
    let call = {
        let _g = LOCK.lock(&CSys);
        find(serial).map(|rec| {
            let r = record(rec);
            (word(r, FUNCTION), word(r, VALUE))
        })
    };
    trace(30, usize::from(call.is_some()));
    if let Some((f, v)) = call {
        // `void f(union sigval)`: the 8-byte union is passed as an integer (psABI).
        let _ = ffi::liella_machine_call3(f, i64::from_ne_bytes(v.to_ne_bytes()), 0, 0);
    }
    0
}

/// The exit sequence of a thread that started at `start` with `arg`: for a notification
/// thread, its timer may notify again.
pub(crate) fn thread_ending(start: usize, arg: usize) {
    if start != notify_addr() {
        return;
    }
    let _g = LOCK.lock(&CSys);
    if let (Some(rec), Some(ep)) = (find(arg), epoll()) {
        let _ = arm(ep, fd_of(record(rec)), arg, EPOLL_CTL_MOD);
    }
}

/// A notification of `start(arg)` that waited for its caller-supplied stack and then could not
/// be started (`crate::stacks`): for a timer's, its expirations stay owed and the notification is
/// retried as one whose thread could not be created at once (owner ruling #269), the helper
/// woken for it ([`WAKE`]). Re-arming the descriptor alone would lose a one-shot timer's
/// notification: nothing more expires. A `timer_settime` since the launch drops the owed
/// expirations at the retry ([`MISSED_GEN`]).
pub(crate) fn abandoned(start: usize, arg: usize) {
    if start != notify_addr() {
        return;
    }
    let _g = LOCK.lock(&CSys);
    if let (Some(rec), Some(ep)) = (find(arg), epoll()) {
        let r = record(rec);
        let lost = r.get(OVERRUN).map_or(0, |w| w.load(Acquire)).saturating_add(1);
        set(r, MISSED, word(r, MISSED).saturating_add(lost));
        set(r, MISSED_GEN, word(r, LAUNCH_GEN));
        #[cfg(feature = "selftest")]
        ABANDONED.fetch_add(1, AcqRel);
        let wake = WAKE.load(Acquire);
        if wake < 0 {
            // Not reached: a timer on a caller-supplied stack made the wake-up descriptor.
            let _ = arm(ep, fd_of(r), arg, EPOLL_CTL_MOD);
            return;
        }
        let delay = back_off(r, BACKOFF, Notifier::Timer);
        schedule_retry(r, delay);
        let _ = CSys.write(Fd::new(wake), &1u64.to_ne_bytes());
    }
}

/// Timer notifications handed back by `crate::stacks` so far (`selftest` only).
#[cfg(feature = "selftest")]
static ABANDONED: AtomicUsize = AtomicUsize::new(0);

/// The helper's wake-up descriptor ([`WAKE`]), -1 before it is made (`selftest` only: a test
/// checks the helper drains it).
#[cfg(feature = "selftest")]
pub(crate) fn wake_fd() -> i32 {
    WAKE.load(Acquire)
}

/// Timer notifications abandoned so far ([`abandoned`]; `selftest` only).
#[cfg(feature = "selftest")]
pub(crate) fn abandoned_count() -> usize {
    ABANDONED.load(Acquire)
}

/// A child of `fork`: no timer is inherited (the live timers' descriptors are closed) and
/// there is no helper. Async-signal-safe (`_Fork`): no allocation, and the lock is reset rather
/// than taken.
pub(crate) fn after_fork_child() {
    LOCK.reset();
    let mut a = HEAD.swap(0, AcqRel);
    while a != 0 {
        let r = record(a);
        if serial_of(r) != 0 {
            let _ = CSys.close(fd_of(r));
        }
        a = next_of(r);
    }
    if let Some(ep) = epoll() {
        let _ = CSys.close(ep);
    }
    EPOLL.store(-1, Release);
    let wake = WAKE.swap(-1, AcqRel);
    if wake >= 0 {
        let _ = CSys.close(Fd::new(wake));
    }
    RETRYING.store(false, Relaxed);
    OTHERS.store(false, Relaxed);
}
