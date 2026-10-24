//! `SIGEV_THREAD` notifications on caller-supplied stacks (D-23.3, D-25.5, D-11.11; owner,
//! 2026-10-04): one thread at a time on a stack, whatever notified it.
//!
//! A notification of a timer, a message queue or an AIO request runs on a new thread made with
//! the attributes the caller gave (POSIX: "as if" created with `sigev_notify_attributes`). When
//! those name a stack (`pthread_attr_setstack`), every notification made with them uses that
//! one stack: a periodic timer, a message-queue callback that registers itself again, AIO
//! requests completing together. Two threads on one stack would corrupt each other, so **a
//! notification on a caller-supplied stack starts only after the previous thread on that stack
//! has left it** (the kernel has cleared its TID word).
//!
//! **The key** is the stack's address range: a notification waits for every thread still on a
//! stack that overlaps its own (the same attribute object, a copy of it, or another object
//! naming the same memory), and for every notification queued before it on an overlapping
//! stack, so they start in the order they became due.
//!
//! **Occupants.** A thread on a caller-supplied stack is made with `thread::create_kept`
//! (detached for the application; this module holds its reclaim token) and recorded with its
//! range. When it ends (any way: return, `pthread_exit`, cancellation), its exit sequence marks
//! it leaving ([`thread_ending`]). A record whose thread has gone is reclaimed by the next use
//! of this module.
//!
//! **Waiting notifications** never block the party that made them due (the timers' helper, the
//! message queues' helper, the AIO helper or worker, or an application thread in `close` or
//! `aio_cancel`), since the thread on the stack may be waiting for that party. They are queued
//! here, with a copy of their attributes, and started by one service thread (made the first
//! time a notification has to wait; `thread::create_service`: every application signal blocked,
//! not counted as running): it waits for the leaving occupants' TID words (briefly: they are in
//! their exit sequence), reclaims them, and starts the queued notifications whose stacks are
//! free. A queued notification that cannot then be started is handed back to its module
//! ([`abandon`]), which does what it does when a thread cannot be made at once.
//!
//! **Processes.** A child of `fork` has no notification thread: nothing is inherited (the
//! parent's records stay allocated: `_Fork`'s child may not use the heap's locks).

use core::sync::atomic::{AtomicU32, AtomicUsize, Ordering::{Relaxed, Release, SeqCst}};

use liella_kernel::errno::{Errno, KResult};
use liella_legacy::pthread_compat::ATTR_WORDS;
use liella_rt::futex::{self, Scope};
use liella_rt::sync::RawMutex;
use liella_rt::thread::Attr;

use crate::sys::CSys;
use crate::{heap, pthread, tb, thread};

/// Every signal but the library's internal one (32, D-20.1): the service thread's mask.
const APP_SIGNALS: u64 = !(1 << 31);
/// The service thread's stack: it runs only the library (thread creation included).
const SERVICE_STACK: usize = 128 << 10;
/// Occupants the service thread waits for in one round.
const ROUND: usize = 16;

/// Serialises the records and the service thread's start.
static LOCK: RawMutex = RawMutex::new();
/// The occupants (records linked through [`NEXT`]), newest first, and how many there are (read
/// without the lock by every exiting thread on a caller-supplied stack).
static OCCUPANTS: AtomicUsize = AtomicUsize::new(0);
static COUNT: AtomicUsize = AtomicUsize::new(0);
/// The queued notifications, oldest first.
static QUEUE_HEAD: AtomicUsize = AtomicUsize::new(0);
static QUEUE_TAIL: AtomicUsize = AtomicUsize::new(0);
/// Free records (heap blocks of [`WORDS`] words, never returned to the heap).
static FREE: AtomicUsize = AtomicUsize::new(0);
/// Advanced whenever the service thread may have work; it sleeps on it.
static WORK: AtomicU32 = AtomicU32::new(0);
/// The service thread exists.
static SERVICE: AtomicUsize = AtomicUsize::new(0);

// A record's words: an occupant (`TP`, `FLAGS`) or a queued notification (`START`, `ARG`,
// `MASK`, `ATTR`); both have a range.
const NEXT: usize = 0;
const LO: usize = 1;
const HI: usize = 2;
const TP: usize = 3;
const FLAGS: usize = 4;
const START: usize = 3;
const ARG: usize = 4;
const MASK: usize = 5;
const ATTR: usize = 6;
const WORDS: usize = ATTR + ATTR_WORDS;

/// The occupant's thread is in its exit sequence.
const F_LEAVING: usize = 1;
/// The service thread is waiting for it to go (it holds the reclaim token now).
const F_REAPING: usize = 2;

fn rec(a: usize) -> &'static [AtomicUsize] {
    tb::words64(a, WORDS)
}

fn w(r: &[AtomicUsize], i: usize) -> usize {
    r.get(i).map_or(0, |x| x.load(Relaxed))
}

fn put(r: &[AtomicUsize], i: usize, v: usize) {
    if let Some(x) = r.get(i) {
        x.store(v, Relaxed);
    }
}

fn overlaps(r: &[AtomicUsize], lo: usize, hi: usize) -> bool {
    w(r, LO) < hi && lo < w(r, HI)
}

/// Under the lock: a zeroed record.
fn alloc() -> KResult<usize> {
    let a = match FREE.load(Relaxed) {
        0 => liella_legacy::malloc_compat::malloc(&heap::CTX, WORDS.saturating_mul(8)).map_err(|_| Errno::EAGAIN)?,
        a => {
            FREE.store(w(rec(a), NEXT), Relaxed);
            a
        }
    };
    for x in rec(a) {
        x.store(0, Relaxed);
    }
    Ok(a)
}

/// Under the lock.
fn release(a: usize) {
    put(rec(a), NEXT, FREE.load(Relaxed));
    FREE.store(a, Relaxed);
}

/// Under the lock: take record `a` off the list starting at `head` (`tail`, for the queue, is
/// kept right); whether it was there.
fn unlink(head: &AtomicUsize, tail: Option<&AtomicUsize>, a: usize) -> bool {
    let mut prev = 0usize;
    let mut x = head.load(Relaxed);
    while x != 0 {
        let next = w(rec(x), NEXT);
        if x == a {
            match prev {
                0 => head.store(next, Relaxed),
                p => put(rec(p), NEXT, next),
            }
            if let Some(t) = tail.filter(|t| t.load(Relaxed) == a) {
                t.store(prev, Relaxed);
            }
            return true;
        }
        prev = x;
        x = next;
    }
    false
}

fn each(head: &AtomicUsize, mut f: impl FnMut(usize, &'static [AtomicUsize]) -> bool) {
    let mut x = head.load(Relaxed);
    while x != 0 {
        let r = rec(x);
        let next = w(r, NEXT);
        if !f(x, r) {
            return;
        }
        x = next;
    }
}

/// Under the lock: reclaim the occupants whose thread has gone (not those the service thread is
/// waiting for).
fn sweep() {
    let mut gone = [0usize; ROUND];
    let mut n = 0;
    each(&OCCUPANTS, |a, r| {
        if w(r, FLAGS) & F_REAPING == 0 && thread::release_kept(w(r, TP), false) {
            if let Some(g) = gone.get_mut(n) {
                *g = a;
                n = n.saturating_add(1);
            }
        }
        n < ROUND
    });
    for &a in gone.iter().take(n) {
        drop_occupant(a);
    }
}

/// Under the lock: forget occupant record `a` (its thread already reclaimed).
fn drop_occupant(a: usize) {
    if unlink(&OCCUPANTS, None, a) {
        COUNT.fetch_sub(1, SeqCst);
        release(a);
    }
}

/// Under the lock: whether a notification on `lo..hi` must wait (an overlapping stack is
/// occupied, or a notification on one is already queued).
fn busy(lo: usize, hi: usize) -> bool {
    let mut busy = false;
    for head in [&OCCUPANTS, &QUEUE_HEAD] {
        each(head, |_, r| {
            busy = overlaps(r, lo, hi);
            !busy
        });
        if busy {
            return true;
        }
    }
    false
}

/// Under the lock: start `start(arg)` on a kept thread with `a` (its stack `lo..hi`) and record
/// it as the stack's occupant.
fn occupy(a: &Attr, lo: usize, hi: usize, start: usize, arg: usize) -> KResult<()> {
    let o = alloc()?;
    // Counted before the thread exists: if it ends before it is recorded, its exit sequence
    // still takes the lock (and so finds its record).
    COUNT.fetch_add(1, SeqCst);
    match thread::create_kept(a, start, arg) {
        Ok(tp) => {
            let r = rec(o);
            put(r, LO, lo);
            put(r, HI, hi);
            put(r, TP, tp);
            put(r, NEXT, OCCUPANTS.load(Relaxed));
            OCCUPANTS.store(o, Relaxed);
            Ok(())
        }
        Err(e) => {
            COUNT.fetch_sub(1, SeqCst);
            release(o);
            Err(e)
        }
    }
}

/// Start `start(arg)` on a new detached notification thread made with the attributes `attr` (a
/// copy taken by `pthread::attr_copy`) and the signal mask `sigmask`. Without a caller-supplied
/// stack the thread starts at once. With one, it starts at once when no thread is on an
/// overlapping stack and none is queued for one; otherwise it is queued (with its own copy of
/// the attributes) and started later, or handed to [`abandon`] if it then cannot be made. An
/// error means nothing will run (the caller does what it does when a thread cannot be made).
pub(crate) fn launch(attr: &[AtomicUsize], sigmask: u64, start: usize, arg: usize) -> KResult<()> {
    let mut a = pthread::attr_decode(attr);
    a.detached = true;
    a.sigmask = Some(sigmask);
    let Some((lo, size)) = a.stack else {
        return thread::create(&a, start, arg, false).map(drop);
    };
    let hi = lo.saturating_add(size);
    let _g = LOCK.lock(&CSys);
    sweep();
    if !busy(lo, hi) {
        return occupy(&a, lo, hi, start, arg);
    }
    service()?;
    let j = alloc()?;
    let r = rec(j);
    if pthread::attr_copy(Some(attr), r.get(ATTR..).unwrap_or(&[])).is_err() {
        release(j);
        return Err(Errno::EAGAIN);
    }
    put(r, LO, lo);
    put(r, HI, hi);
    put(r, START, start);
    put(r, ARG, arg);
    put(r, MASK, usize::try_from(sigmask).unwrap_or(0));
    match QUEUE_TAIL.load(Relaxed) {
        0 => QUEUE_HEAD.store(j, Relaxed),
        t => put(rec(t), NEXT, j),
    }
    QUEUE_TAIL.store(j, Relaxed);
    // The occupant may already be leaving (it will not say so again).
    poke();
    Ok(())
}

fn poke() {
    WORK.fetch_add(1, SeqCst);
    let _ = futex::wake_all(&CSys, &WORK, Scope::Private);
}

/// The exit sequence of a thread on a caller-supplied stack (`tp`, its `pthread_t`): if it is a
/// notification's, it is leaving its stack, and a notification waiting for the stack may start
/// once it has gone.
pub(crate) fn thread_ending(tp: usize) {
    if COUNT.load(SeqCst) == 0 {
        return;
    }
    let _g = LOCK.lock(&CSys);
    let mut found = false;
    each(&OCCUPANTS, |_, r| {
        found = w(r, TP) == tp;
        if found {
            put(r, FLAGS, w(r, FLAGS) | F_LEAVING);
        }
        !found
    });
    if found && QUEUE_HEAD.load(Relaxed) != 0 {
        poke();
    }
}

/// Under the lock: the service thread, made the first time.
fn service() -> KResult<()> {
    if SERVICE.load(Relaxed) != 0 {
        return Ok(());
    }
    let a = Attr { detached: true, stack_size: SERVICE_STACK, sigmask: Some(APP_SIGNALS), ..Attr::default() };
    thread::create_service(&a, service_addr(), 0)?;
    SERVICE.store(1, Relaxed);
    Ok(())
}

#[allow(clippy::as_conversions)] // a function pointer's address: the only spelling Rust has
fn service_addr() -> usize {
    service_main as extern "C" fn(usize) -> usize as usize
}

/// The service thread: starts queued notifications as their stacks become free, for as long as
/// the process runs.
extern "C" fn service_main(_: usize) -> usize {
    loop {
        let seen = WORK.load(SeqCst);
        // The leaving occupants that queued notifications wait for: this thread takes them.
        let mut reap = [(0usize, 0usize); ROUND];
        let mut n = 0;
        {
            let _g = LOCK.lock(&CSys);
            sweep();
            each(&OCCUPANTS, |a, r| {
                if w(r, FLAGS) & (F_LEAVING | F_REAPING) == F_LEAVING && waited_for(r) {
                    put(r, FLAGS, w(r, FLAGS) | F_REAPING);
                    if let Some(s) = reap.get_mut(n) {
                        *s = (a, w(r, TP));
                        n = n.saturating_add(1);
                    }
                }
                n < ROUND
            });
        }
        // Without the lock: the rest of their exit may need it (or anything else).
        for &(a, tp) in reap.iter().take(n) {
            thread::release_kept(tp, true);
            let _g = LOCK.lock(&CSys);
            drop_occupant(a);
        }
        let mut failed = [(0usize, 0usize); ROUND];
        let started = {
            let _g = LOCK.lock(&CSys);
            start_free(&mut failed)
        };
        for &(start, arg) in failed.iter().filter(|f| f.0 != 0) {
            abandon(start, arg);
        }
        if n == 0 && started == 0 {
            let _ = futex::wait(&CSys, &WORK, seen, Scope::Private);
        }
    }
}

/// Under the lock: whether a queued notification waits for occupant `r`'s stack.
fn waited_for(r: &[AtomicUsize]) -> bool {
    let mut hit = false;
    each(&QUEUE_HEAD, |_, j| {
        hit = overlaps(j, w(r, LO), w(r, HI));
        !hit
    });
    hit
}

/// Under the lock: start the queued notifications whose stacks are free, oldest first (one
/// waits for every earlier one on an overlapping stack). Those that cannot be made go to
/// `failed` (as many as it holds; the rest stay queued for the next round). How many left the
/// queue.
fn start_free(failed: &mut [(usize, usize); ROUND]) -> usize {
    let mut done = 0usize;
    let mut nfailed = 0usize;
    let mut x = QUEUE_HEAD.load(Relaxed);
    while x != 0 && nfailed < ROUND {
        let r = rec(x);
        let next = w(r, NEXT);
        let (lo, hi) = (w(r, LO), w(r, HI));
        let mut earlier = false;
        each(&QUEUE_HEAD, |y, e| {
            if y == x {
                return false;
            }
            earlier = overlaps(e, lo, hi);
            !earlier
        });
        let mut occupied = false;
        each(&OCCUPANTS, |_, o| {
            occupied = overlaps(o, lo, hi);
            !occupied
        });
        if !earlier && !occupied {
            let attr = r.get(ATTR..).unwrap_or(&[]);
            let mut a = pthread::attr_decode(attr);
            a.detached = true;
            a.sigmask = Some(u64::try_from(w(r, MASK)).unwrap_or(0));
            let (start, arg) = (w(r, START), w(r, ARG));
            if occupy(&a, lo, hi, start, arg).is_err() {
                if let Some(f) = failed.get_mut(nfailed) {
                    *f = (start, arg);
                }
                nfailed = nfailed.saturating_add(1);
            }
            unlink(&QUEUE_HEAD, Some(&QUEUE_TAIL), x);
            pthread::attr_release(attr);
            release(x);
            done = done.saturating_add(1);
        }
        x = next;
    }
    done
}

/// A queued notification that could not be started: its module retries it, as when a thread
/// cannot be made at once (owner rulings #269, #680).
fn abandon(start: usize, arg: usize) {
    crate::timer::abandoned(start, arg);
    crate::mqueue::abandoned(start, arg);
    crate::aio::abandoned(start, arg);
}

/// How many notifications are queued now (`selftest` only).
#[cfg(feature = "selftest")]
pub(crate) fn queued() -> usize {
    let _g = LOCK.lock(&CSys);
    let mut n = 0usize;
    each(&QUEUE_HEAD, |_, _| {
        n = n.saturating_add(1);
        true
    });
    n
}

/// A child of `fork`: no notification thread, nothing queued, no service thread.
/// Async-signal-safe (`_Fork`): no allocation, and the lock is reset rather than taken.
pub(crate) fn after_fork_child() {
    LOCK.reset();
    for x in [&OCCUPANTS, &COUNT, &QUEUE_HEAD, &QUEUE_TAIL, &SERVICE] {
        x.store(0, Release);
    }
    WORK.store(0, Release);
}
