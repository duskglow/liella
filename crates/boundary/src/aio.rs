//! POSIX asynchronous I/O (spec §11.11; D-11.8): `aio_read`, `aio_write`, `aio_fsync`,
//! `aio_error`, `aio_return`, `aio_suspend`, `aio_cancel`, `lio_listio`, `aio_init`.
//!
//! **Two engines, one contract.** A request runs on **io_uring** when the kernel offers it and
//! permits it (io_uring(7)), and otherwise on a **pool of worker threads** doing ordinary
//! `pread`/`pwrite`/`fsync`. The engine is chosen at a process's first request and kept: the
//! ring is set up (`io_uring_setup` with `IORING_SETUP_CLAMP`, a 4096-entry completion queue,
//! and the cooperative task-run flags where the kernel knows them; the `READ`, `WRITE`, `FSYNC`
//! and `POLL_ADD` operations and `IORING_FEAT_NODROP` are required), and any refusal (`EPERM`
//! from `kernel.io_uring_disabled` or a seccomp filter, `ENOSYS`, `EACCES`, `ENOMEM`, an older
//! kernel) selects the workers, silently.
//!
//! **When io_uring is not tried** (owner ruling 2026-10-04, D-11.11): the workers are chosen
//! without touching io_uring when the process runs in secure-execution mode (`AT_SECURE`), when
//! a seccomp filter is installed on the calling thread (a filter that kills on a system call it
//! does not list would kill the process at `io_uring_setup`, where no fallback can catch it),
//! when the program asked for threads ([`use_threads`], the native `liella_aio_use_threads`,
//! before its first request), or when the environment says so at the first request
//! (`LIELLA_AIO_URING=0`, read with `secure_getenv`). A filter is seen first without any
//! `prctl`, in the `Seccomp:` line of `/proc/thread-self/status` (on procfs: any value but 0);
//! only when that says 0 or cannot be read is `prctl(PR_GET_SECCOMP)` asked, and anything but 0
//! from it, a failure included, counts as a filter. Tests force the workers with
//! `selftest::aio_force_workers` (the same switch), and choose the engine through an
//! injected-failure kernel with `selftest::aio_choose_engine`.
//!
//! **Requests.** Each request is a record (heap memory on a list that only grows, reused) made at
//! submission from a copy of the `aiocb`'s fields, so later changes to the block do not matter;
//! the block itself is written only for the status: `__error_code` (`EINPROGRESS` until the
//! request completes, then 0 or the error) and `__return_value`, which `aio_error` and
//! `aio_return` read without a lock (both may be called any number of times; a block never
//! submitted reads as its fields say, 0 and 0 when zeroed). The library's state is one lock;
//! the status words, `aio_error`, `aio_return` and `aio_suspend` do not take it.
//!
//! **Order** (§11.11 Guarantees, stronger than POSIX and the manual, which the spec governs).
//! Requests are queued **per descriptor number** and at most one request per descriptor runs at
//! a time, so requests on one descriptor with one priority start, and complete, in submission
//! order. Among queued requests a lower `aio_reqprio` (a higher priority) runs first, then the
//! older. `aio_fsync` is a barrier: it runs after every request queued before it on the
//! descriptor and before every request queued after it, whatever the priorities. Requests on
//! different descriptors run concurrently (io_uring: as many as the completion queue holds; the
//! workers: as many as there are workers).
//!
//! **io_uring engine.** One helper thread per process (`thread::create_service`: every
//! application signal blocked, not counted as running) is the ring's only submitter, so the
//! kernel's inline attempts and its task work run in the helper, whose blocked `SIGPIPE` and
//! `SIGXFSZ` never reach the application (as the reference's helpers: a failed write reports
//! `EPIPE`/`EFBIG` and the signal is not delivered). Application threads put a request whose
//! turn has come on a ready list and wake the helper through an eventfd it polls on the ring;
//! the helper writes the submission entries and calls `io_uring_enter` **under the lock**, so a
//! `close` (below) never races a submission. It sleeps in `io_uring_enter(GETEVENTS)` and
//! completes requests from the completion queue. Where io_uring and a single `pread`/`pwrite`
//! differ, the helper restores the system call's meaning: a descriptor without positions
//! refuses a non-zero offset (`ESPIPE`), so the request runs again at offset 0 (the offset is
//! ignored, as `read` ignores it); an `O_NONBLOCK` descriptor that is not a regular file or a
//! block device gets `RWF_NOWAIT` (`EAGAIN` instead of waiting); a write to a blocking
//! descriptor that completes short continues with the rest until the whole count is written or
//! an error ends it (the count written so far is then the result), as a blocking `write` does.
//! A count is limited to the kernel's `MAX_RW_COUNT` (0x7ffff000, read(2)), which io_uring's 32-bit
//! length holds; a buffer whose end would reach 2^63 is `EFAULT` without a transfer, the answer
//! the kernel gives `pread` for such a count (observed on Linux 6.8, x86-64; the reference reports
//! `EFAULT` for counts above `SSIZE_MAX` and transfers for `INT_MAX + 1`).
//!
//! **Transfers the ring does not take** (kernel-5.10 validation, 2026-10-05). While the ring
//! runs, a read or write on a descriptor that is not a regular file or a block device (a pipe,
//! a socket, a character device) runs on a worker with `read`/`write` instead: io_uring has
//! differed from the system calls there (Linux 5.10 refuses `RWF_NOWAIT` on a pipe with
//! `EOPNOTSUPP`, and its socket reads never completed), and a worker gives exactly the system
//! call's answer, `O_NONBLOCK` included, on every kernel. A transfer the ring refuses with
//! `EFAULT` is also run again by a worker (5.10's io_uring checks the whole count against the
//! buffer, where `pread` transfers what the file holds), so the system call decides.
//!
//! **Worker engine.** Up to `aio_init`'s `aio_threads` workers (default 64; values below 1 mean
//! 1), made as requests wait and kept for later ones, with every application signal blocked and
//! not counted as running. A worker runs a request with one system call (`pread64`/`pwrite64`,
//! or `read`/`write` where the descriptor has no positions, `fsync`/`fdatasync`). `aio_num` and
//! `aio_idle_time` are accepted and ignored.
//!
//! **Notification** after the status is stored: `SIGEV_SIGNAL` queues the signal to the process
//! (`rt_sigqueueinfo`, `si_code` `SI_ASYNCIO`, the event's value; signal 0 sends nothing);
//! `SIGEV_THREAD` calls the function with the value on a new thread, made with a copy of
//! `sigev_notify_attributes` taken at submission (stack size, stack address, guard size,
//! scheduling, affinity: POSIX's "as if" a thread created with those attributes), always
//! detached (POSIX leaves a joinable attribute undefined; the reference makes it joinable;
//! owner ruling 2026-10-04), with no signal blocked (as the reference). A caller-supplied stack
//! serves every request notified with it, one notification at a time (`crate::stacks`):
//! requests completing together notify one after another, each starting once the previous
//! thread on the stack has left it. A `SIGEV_THREAD` notification whose thread cannot be
//! started (a thread limit, memory), at once or after waiting for its stack, keeps its block and
//! is retried with the timers' back-off by the timers' helper (`timer::retry_soon`,
//! [`retry_due`]; owner ruling #680, extending #269): delivered late, once per request, never
//! lost (glibc drops it: a deliberate difference, D-11.11); the helper is made with the first
//! `SIGEV_THREAD` request or list (`timer::retry_helper`). `SIGEV_NONE` and any other kind:
//! nothing. A `lio_listio(LIO_NOWAIT)` list notifies once when
//! every entry has completed (also with no entries).
//!
//! **Errors.** At submission: `EINVAL` for an `aio_reqprio` outside 0..`AIO_PRIO_DELTA_MAX`
//! (20), an `aio_fsync` operation other than `O_SYNC`/`O_DSYNC` (checked first), a `SIGEV_THREAD`
//! without a function (the reference crashes at the completion), a NULL or misaligned block;
//! `EBADF` from `aio_fsync` and `aio_cancel` for a descriptor that is not open; `EAGAIN` when no
//! ring helper or worker can be made, or without memory. Everything else is the request's own
//! result, through `aio_error`: a negative offset is `EINVAL` (also on a pipe), an invalid
//! notification signal is `EINVAL` (nothing is transferred), and the kernel's errors.
//!
//! **Waiting.** `aio_suspend` returns 0 when a listed request is no longer in progress, and also
//! when no listed request is in progress (only NULL entries, `nent` 0, blocks never submitted or
//! already returned: the reference's behaviour, which POSIX does not give), `EAGAIN` when the
//! timeout passes (a timeout of zero or less, or one whose end does not fit the clock, has
//! passed), `EINTR` when a signal handler runs (also with `SA_RESTART`), `EINVAL` for a negative
//! `nent`. `lio_listio(LIO_WAIT)` waits for every entry; a handler without `SA_RESTART` ends the
//! wait with `EINTR` (the entries still complete normally, and nothing the call owned is used
//! afterwards); with `SA_RESTART` the wait goes on. Both are cancellation points. Waiters sleep
//! on a completion counter (`LIO_WAIT` on one that only whole lists advance).
//!
//! **Lists.** `AIO_LISTIO_MAX` has no fixed value (`sysconf` returns -1, as the reference's): a
//! list may hold any number of entries, bounded by memory (`EAGAIN`, with the entries not
//! queued marked `EAGAIN`); a negative `nent` is an empty list (the reference crashes for
//! `INT_MIN`). An unknown opcode makes that entry `EINVAL`; an invalid priority makes that entry
//! `EINVAL` and `LIO_NOWAIT` return -1 `EINVAL` while the other entries run (the reference's
//! behaviour); `LIO_WAIT` returns -1 `EIO` when any entry failed.
//!
//! **Cancelling.** A request that has not been handed to the kernel or a worker is cancelled
//! (`ECANCELED`, notified); a started one is not (`AIO_NOTCANCELED`), on both engines. A block
//! whose status says in progress but which this process does not hold (a `fork` child's copy of
//! the parent's) is not cancellable. A request naming another descriptor is `EINVAL`.
//!
//! **Descriptors closed.** `close`, `close_range`, `closefrom`, `dup2` and `dup3` first cancel
//! the requests on the descriptor that have not started (POSIX: they may be cancelled); a
//! started request completes as if the descriptor were still open (the kernel holds the file).
//! The rest of a started io_uring request (a write cut short, an offset refused) is not
//! cancelled by `aio_cancel`, but a `close` ends it, reporting what was written. With nothing
//! queued, `close` costs a few atomic loads. A `close` from a signal handler that interrupts its own
//! thread inside this module changes nothing (the lock is that thread's). Closing the ring's or
//! the eventfd's descriptor through these calls retires the ring: the workers take over, and the
//! helper only collects what the kernel still holds. `fclose` closes its descriptor without
//! consulting this module.
//!
//! **Processes.** A `fork` child inherits no request (POSIX): its queues, ring, helper and
//! workers are gone, and its own requests run on fresh ones. An `exec` cancels everything: the
//! ring and the eventfd are close-on-exec and the kernel cancels the ring's requests (a transfer
//! in progress stops where it is); no notification reaches the new image.

use core::sync::atomic::{AtomicI32, AtomicU32, AtomicUsize, Ordering::{AcqRel, Acquire, Relaxed, Release, SeqCst}};

use liella_kernel::errno::{Errno, KResult};
use liella_kernel::fd::Fd;
use liella_kernel::ops::{FcntlInt, PrctlInt};
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{SigEvent, SigInfo, Timespec, CLOCK_MONOTONIC, EFD_CLOEXEC, EFD_NONBLOCK, O_DSYNC, O_NONBLOCK, O_SYNC, S_IFBLK, S_IFMT, S_IFREG, SIGEV_NONE, SIGEV_SIGNAL, SIGEV_THREAD};
use liella_legacy::pthread_compat::ATTR_WORDS;
use liella_rt::futex::{self, Clock, Scope};
use liella_rt::sync::{MutexGuard, RawCondvar, RawMutex};
use liella_rt::thread::Attr;

use crate::sys::{CSys, CP};
use crate::{conv, ffi, heap, pthread, raw, stacks, tb, thread};

/// `aio_cancel`'s results.
pub const AIO_CANCELED: i32 = 0;
/// A request could not be cancelled.
pub const AIO_NOTCANCELED: i32 = 1;
/// Nothing was left to cancel.
pub const AIO_ALLDONE: i32 = 2;
/// `lio_listio` operations and modes.
pub const LIO_READ: i32 = 0;
/// See [`LIO_READ`].
pub const LIO_WRITE: i32 = 1;
/// See [`LIO_READ`].
pub const LIO_NOP: i32 = 2;
/// See [`LIO_READ`].
pub const LIO_WAIT: i32 = 0;
/// See [`LIO_READ`].
pub const LIO_NOWAIT: i32 = 1;
/// `AIO_PRIO_DELTA_MAX` (`sysconf(_SC_AIO_PRIO_DELTA_MAX)`).
pub const AIO_PRIO_DELTA_MAX: i32 = 20;
/// `SI_ASYNCIO`: the completion signal's `si_code`.
const SI_ASYNCIO: i32 = -4;
/// Every signal but the library's internal one (32, D-20.1): the helper's and workers' mask.
const APP_SIGNALS: u64 = !(1 << 31);
/// The helper's and workers' stacks: they run the library, thread creation included.
const SERVICE_STACK: usize = 256 << 10;
/// The workers when `aio_init` set none.
const DEFAULT_WORKERS: usize = 64;
/// The most bytes one transfer moves (`MAX_RW_COUNT`, read(2)).
const MAX_RW: usize = 0x7fff_f000;
/// The kernel refuses (`EFAULT`) a transfer whose buffer would end at or past 2^63 (observed:
/// `pread` on Linux 6.8, x86-64, the worker engine's call).
const USER_END: usize = 0x7fff_ffff_ffff_ffff;

fn einprogress() -> usize {
    word_i32(Errno::EINPROGRESS.get())
}

// ------------------------------------------------------------------------------ words

/// The `struct aiocb` (168 bytes) as words: `aio_fildes` and `aio_lio_opcode`, `aio_reqprio`,
/// `aio_buf`, `aio_nbytes`, the `sigevent` (value; signal and kind; function; attributes),
/// `__error_code`, `__return_value`, `aio_offset`.
const CB_WORDS: usize = 21;
const W_FD_OP: usize = 0;
const W_PRIO: usize = 1;
const W_BUF: usize = 2;
const W_NBYTES: usize = 3;
const W_VALUE: usize = 4;
const W_SIGNO_NOTIFY: usize = 5;
const W_FUNCTION: usize = 6;
const W_ATTRS: usize = 7;
const W_ERROR: usize = 14;
const W_RETURN: usize = 15;
const W_OFFSET: usize = 16;

fn w(r: &[AtomicUsize], i: usize) -> usize {
    r.get(i).map_or(0, |x| x.load(Relaxed))
}

fn put(r: &[AtomicUsize], i: usize, v: usize) {
    if let Some(x) = r.get(i) {
        x.store(v, Relaxed);
    }
}

fn lo32(v: usize) -> i32 {
    i32::from_ne_bytes(u32::try_from(v & 0xffff_ffff).unwrap_or(0).to_ne_bytes())
}

fn hi32(v: usize) -> i32 {
    lo32(v >> 32)
}

fn word_i32(v: i32) -> usize {
    usize::try_from(u32::from_ne_bytes(v.to_ne_bytes())).unwrap_or(0)
}

fn word_i64(v: i64) -> usize {
    usize::from_ne_bytes(v.to_ne_bytes())
}

fn as_i64(v: usize) -> i64 {
    i64::from_ne_bytes(v.to_ne_bytes())
}

fn block_of(cb: usize) -> Option<&'static [AtomicUsize]> {
    conv::named_words(cb, CB_WORDS)
}

/// Store a request's final status: the return value, then the error (the word `aio_error`
/// reads, with release ordering).
fn set_status(c: &[AtomicUsize], err: i32, ret: isize) {
    put(c, W_RETURN, usize::from_ne_bytes(ret.to_ne_bytes()));
    if let Some(x) = c.get(W_ERROR) {
        x.store(word_i32(err), Release);
    }
}

// ------------------------------------------------------------------------------ state

/// The lock over every queue, record and engine decision.
static LOCK: RawMutex = RawMutex::new();
/// The TID of the lock's holder (0: none): a signal handler interrupting its own thread's
/// critical section (a `close` in a handler) must not wait for the lock.
static OWNER: AtomicI32 = AtomicI32::new(0);
/// The workers wait here for ready requests.
static WORK: RawCondvar = RawCondvar::new();

const ENGINE_NONE: usize = 0;
const ENGINE_RING: usize = 1;
const ENGINE_WORKERS: usize = 2;
static ENGINE: AtomicUsize = AtomicUsize::new(ENGINE_NONE);
/// The program asked for the workers ([`use_threads`]); kept in a `fork` child.
static FORCE_WORKERS: AtomicUsize = AtomicUsize::new(0);
/// `aio_init`'s `aio_threads` (0: the default).
static MAX_WORKERS: AtomicUsize = AtomicUsize::new(0);
static WORKERS: AtomicUsize = AtomicUsize::new(0);
/// Workers not running a request.
static IDLE: AtomicUsize = AtomicUsize::new(0);

/// A list of requests whose turn has come, not yet handed to the kernel or a worker (linked
/// through `R_NEXT`; changed under the lock).
struct Ready {
    head: AtomicUsize,
    tail: AtomicUsize,
    len: AtomicUsize,
}

impl Ready {
    const fn new() -> Ready {
        Ready { head: AtomicUsize::new(0), tail: AtomicUsize::new(0), len: AtomicUsize::new(0) }
    }

    fn reset(&self) {
        for x in [&self.head, &self.tail, &self.len] {
            x.store(0, Relaxed);
        }
    }
}

/// The ready list of the engine: the ring's helper, or the workers.
static READY: Ready = Ready::new();
/// With the ring: the ready requests the workers run instead (`F_WORKER`).
static WREADY: Ready = Ready::new();
/// Requests queued or ready (not yet started): `close` needs the lock only when there are some.
static PENDING: AtomicUsize = AtomicUsize::new(0);
/// Free records, descriptor queues and list groups (heap blocks linked through word 0).
static FREE_REC: AtomicUsize = AtomicUsize::new(0);
static FREE_NODE: AtomicUsize = AtomicUsize::new(0);
static FREE_GROUP: AtomicUsize = AtomicUsize::new(0);
/// Descriptor queues by descriptor number (chained).
const BUCKETS: usize = 256;
static BUCKET: [AtomicUsize; BUCKETS] = [const { AtomicUsize::new(0) }; BUCKETS];
static SEQ: AtomicUsize = AtomicUsize::new(0);
/// Advances with every completion; `aio_suspend` and `LIO_WAIT` sleep on it.
static DONE: AtomicU32 = AtomicU32::new(0);
static WAITERS: AtomicUsize = AtomicUsize::new(0);
/// Advances when a whole list is done; `LIO_WAIT` sleeps on it (so it is not woken by every
/// completion).
static LIST_DONE: AtomicU32 = AtomicU32::new(0);
static LIST_WAITERS: AtomicUsize = AtomicUsize::new(0);

// A request record's words.
const R_NEXT: usize = 0;
const R_CB: usize = 1;
const R_FD: usize = 2;
const R_OP: usize = 3;
const R_BUF: usize = 4;
const R_NBYTES: usize = 5;
const R_OFFSET: usize = 6;
const R_EPOCH: usize = 7;
const R_RANK: usize = 8;
const R_PRIO: usize = 9;
const R_SEQ: usize = 10;
/// The list group (its address) or 0.
const R_GROUP: usize = 11;
/// An error decided at submission, reported when the request's turn comes (0: none).
const R_PRESET: usize = 12;
/// Bytes already written by a write that continues.
const R_DONE: usize = 13;
const R_STATE: usize = 14;
const R_FLAGS: usize = 15;
const R_EVENT: usize = 16;
const R_WORDS: usize = R_EVENT + E_WORDS;

const OP_READ: usize = 0;
const OP_WRITE: usize = 1;
const OP_SYNC: usize = 2;
const OP_DSYNC: usize = 3;

const ST_FREE: usize = 0;
const ST_QUEUED: usize = 1;
const ST_READY: usize = 2;
const ST_RUNNING: usize = 3;
const ST_NOTIFY: usize = 4;

/// The descriptor has no positions: transfer at offset 0 (the offset is ignored).
const F_STREAM: usize = 1;
/// `O_NONBLOCK` on a descriptor that honours it: never wait.
const F_NOWAIT: usize = 2;
/// The kernel has had the request: what is left of it (a write cut short, an offset refused) is
/// not cancelled by `aio_cancel` (a `close` still stops it).
const F_STARTED: usize = 4;
/// With the ring, run by a worker instead (a descriptor that is not a regular file or a block
/// device, or a transfer the ring refused with `EFAULT`): ordinary `read`/`pread` semantics on
/// every kernel (kernel-5.10 validation).
const F_WORKER: usize = 8;

// A notification event's words (in records and groups).
const E_NOTIFY: usize = 0;
const E_SIGNO: usize = 1;
const E_VALUE: usize = 2;
const E_FUNCTION: usize = 3;
/// The next block on [`RETRIES`] (a block whose notification thread could not be started).
const E_LINK: usize = 4;
/// When its retry is due (`CLOCK_MONOTONIC`, ns).
const E_RETRY_AT: usize = 5;
/// The next retry's delay (`timer::back_off`); 0 outside a failure episode.
const E_BACKOFF: usize = 6;
const E_ATTR: usize = 7;
const E_WORDS: usize = E_ATTR + ATTR_WORDS;

// A descriptor queue's words.
const N_NEXT: usize = 0;
const N_FD: usize = 1;
const N_HEAD: usize = 2;
const N_TAIL: usize = 3;
/// The request ready or running on the descriptor (0: none).
const N_RUNNING: usize = 4;
/// Advanced by every `aio_fsync`: requests of a later epoch run after the sync.
const N_EPOCH: usize = 5;
const N_WORDS: usize = 6;

// A list group's words.
// Word 0 links a free group.
const G_REMAINING: usize = 1;
const G_EVENT: usize = 2;
const G_WORDS: usize = G_EVENT + E_WORDS;

fn rec(a: usize) -> &'static [AtomicUsize] {
    tb::words64(a, R_WORDS)
}

fn node(a: usize) -> &'static [AtomicUsize] {
    tb::words64(a, N_WORDS)
}

fn group(a: usize) -> &'static [AtomicUsize] {
    tb::words64(a, G_WORDS)
}

fn event(r: &[AtomicUsize], base: usize) -> &[AtomicUsize] {
    r.get(base..).unwrap_or(&[])
}

/// The lock, recording its holder.
struct Held {
    _guard: MutexGuard<'static, CSys>,
}

impl Drop for Held {
    fn drop(&mut self) {
        OWNER.store(0, Relaxed);
    }
}

fn lock() -> Held {
    let g = LOCK.lock(&CSys);
    OWNER.store(thread::own_tid(), Relaxed);
    Held { _guard: g }
}

/// Under the lock: a zeroed block of `words` words from `free` or the heap.
fn alloc(free: &AtomicUsize, words: usize) -> KResult<usize> {
    let a = match free.load(Relaxed) {
        0 => liella_legacy::malloc_compat::malloc(&heap::CTX, words.saturating_mul(8)).map_err(|_| Errno::EAGAIN)?,
        a => {
            free.store(w(tb::words64(a, 1), 0), Relaxed);
            a
        }
    };
    for x in tb::words64(a, words) {
        x.store(0, Relaxed);
    }
    Ok(a)
}

fn release(free: &AtomicUsize, a: usize) {
    put(tb::words64(a, 1), 0, free.load(Relaxed));
    free.store(a, Relaxed);
}

fn bump() {
    DONE.fetch_add(1, SeqCst);
    if WAITERS.load(SeqCst) != 0 {
        let _ = futex::wake_all(&CSys, &DONE, Scope::Private);
    }
}

// ------------------------------------------------------------------------------ notifications

const K_NONE: usize = 0;
const K_SIGNAL: usize = 1;
const K_THREAD: usize = 2;

/// A notification to deliver once the lock is released.
#[derive(Clone, Copy, Default)]
struct Notice {
    kind: usize,
    signo: i32,
    value: u64,
    /// For `SIGEV_THREAD`: the record (or the group, tagged with bit 0) holding the event.
    block: usize,
}

/// Notifications gathered under the lock.
struct Batch {
    n: usize,
    list: [Notice; 16],
}

impl Batch {
    fn new() -> Batch {
        Batch { n: 0, list: [Notice::default(); 16] }
    }

    /// Room for one more request's notifications (its own and its list's).
    fn room(&self) -> bool {
        self.n.saturating_add(2) <= self.list.len()
    }

    fn push(&mut self, x: Notice) {
        if x.kind == K_NONE {
            return;
        }
        if let Some(s) = self.list.get_mut(self.n) {
            *s = x;
            self.n = self.n.saturating_add(1);
        }
    }

    /// Deliver every notification (without the lock).
    fn deliver(&self) {
        for x in self.list.iter().take(self.n) {
            deliver(x);
        }
    }
}

/// Fill an event from a `sigevent`'s fields (a `SIGEV_THREAD` event copies the attributes).
fn set_event(ev: &[AtomicUsize], notify: i32, signo: i32, value: u64, function: usize, attrs: usize) -> KResult<()> {
    if notify == SIGEV_THREAD {
        // The retries' helper first: a refused notification thread is retried without making one.
        crate::timer::retry_helper().map_err(|_| Errno::EAGAIN)?;
    }
    for i in [E_LINK, E_RETRY_AT, E_BACKOFF] {
        put(ev, i, 0);
    }
    put(ev, E_NOTIFY, word_i32(notify));
    put(ev, E_SIGNO, word_i32(signo));
    put(ev, E_VALUE, usize::try_from(value).unwrap_or(0));
    put(ev, E_FUNCTION, function);
    if notify == SIGEV_THREAD {
        let src = conv::named_words(attrs, ATTR_WORDS);
        pthread::attr_copy(src, event(ev, E_ATTR)).map_err(|_| Errno::EAGAIN)?;
    }
    Ok(())
}

fn release_event(ev: &[AtomicUsize]) {
    if lo32(w(ev, E_NOTIFY)) == SIGEV_THREAD {
        pthread::attr_release(event(ev, E_ATTR));
    }
    put(ev, E_NOTIFY, 0);
}

/// What an event delivers (`block` is where it lives).
fn notice_of(ev: &[AtomicUsize], block: usize) -> Notice {
    let signo = lo32(w(ev, E_SIGNO));
    let value = u64::try_from(w(ev, E_VALUE)).unwrap_or(0);
    match lo32(w(ev, E_NOTIFY)) {
        SIGEV_SIGNAL if (1..=64).contains(&signo) => Notice { kind: K_SIGNAL, signo, value, block },
        SIGEV_THREAD if w(ev, E_FUNCTION) != 0 => Notice { kind: K_THREAD, signo, value, block },
        _ => Notice::default(),
    }
}

/// The event of a notification block (a record, or a group tagged with bit 0).
fn block_event(block: usize) -> &'static [AtomicUsize] {
    if block & 1 != 0 { event(group(block & !1), G_EVENT) } else { event(rec(block), R_EVENT) }
}

/// Under the lock: free a notification block whose thread has taken (or will never take) it.
fn release_block(block: usize) {
    release_event(block_event(block));
    if block & 1 != 0 {
        release(&FREE_GROUP, block & !1);
    } else {
        free_record(block);
    }
}

fn deliver(x: &Notice) {
    match x.kind {
        K_SIGNAL => {
            let pid = CSys.getpid();
            let mut info = SigInfo::queued(x.signo, pid, CSys.getuid(), x.value);
            info.code = SI_ASYNCIO;
            let _ = CSys.sigqueueinfo(pid, x.signo, &info);
        }
        K_THREAD => {
            let ev = block_event(x.block);
            // Always detached, whatever the attributes say (POSIX leaves a joinable one
            // undefined; nobody could join it: owner ruling 2026-10-04). On a caller-supplied
            // stack it may start later (`crate::stacks`): the block waits for it.
            if stacks::launch(event(ev, E_ATTR), 0, notification_addr(), x.block).is_err() {
                {
                    let _g = lock();
                    schedule_retry(x.block);
                }
                crate::timer::retry_soon();
            }
        }
        _ => {}
    }
}

/// The blocks whose notification thread could not be started, waiting for a retry (linked
/// through [`E_LINK`]; under the lock).
static RETRIES: AtomicUsize = AtomicUsize::new(0);

/// Under the lock: the notification of `block` could not be started; retry it after the
/// back-off.
fn schedule_retry(block: usize) {
    let ev = block_event(block);
    let delay = crate::timer::back_off(ev, E_BACKOFF, crate::timer::Notifier::Aio);
    put(ev, E_RETRY_AT, crate::timer::retry_deadline(delay));
    put(ev, E_LINK, RETRIES.load(Relaxed));
    RETRIES.store(block, Relaxed);
}

/// The timers' helper: start the notifications whose retry is due at `now` (ns), and return when
/// the next pending one is due.
pub(crate) fn retry_due(now: usize) -> Option<usize> {
    if RETRIES.load(Relaxed) == 0 {
        return None;
    }
    let _g = lock();
    let mut next: Option<usize> = None;
    let mut b = RETRIES.swap(0, Relaxed);
    while b != 0 {
        let ev = block_event(b);
        let after = w(ev, E_LINK);
        if w(ev, E_RETRY_AT) <= now {
            if stacks::launch(event(ev, E_ATTR), 0, notification_addr(), b).is_ok() {
                // The thread waits for the lock before it takes (and releases) the block.
                crate::timer::recovered(ev, E_BACKOFF, crate::timer::Notifier::Aio);
                b = after;
                continue;
            }
            schedule_retry(b);
        } else {
            put(ev, E_LINK, RETRIES.load(Relaxed));
            RETRIES.store(b, Relaxed);
        }
        let at = w(ev, E_RETRY_AT);
        next = Some(next.map_or(at, |n| n.min(at)));
        b = after;
    }
    next
}

#[allow(clippy::as_conversions)] // a function pointer's address: the only spelling Rust has
fn notification_addr() -> usize {
    notification as extern "C" fn(usize) -> usize as usize
}

/// A `SIGEV_THREAD` notification's start routine: the function with the value.
extern "C" fn notification(block: usize) -> usize {
    let (f, v) = {
        let _g = lock();
        let ev = block_event(block);
        let fv = (w(ev, E_FUNCTION), w(ev, E_VALUE));
        release_block(block);
        fv
    };
    // `void f(union sigval)`: the 8-byte union is passed as an integer (psABI).
    let _ = ffi::liella_machine_call3(f, as_i64(v), 0, 0);
    0
}

/// A notification of `start(arg)` that waited for its caller-supplied stack and then could not
/// be started (`crate::stacks`): for an AIO notification, its block is kept and the notification
/// retried, as when the thread cannot be made at once ([`deliver`]; owner ruling #680, D-11.11;
/// the reference drops one whose thread it cannot create). The request itself has completed:
/// `aio_error` and `aio_return` report it.
pub(crate) fn abandoned(start: usize, arg: usize) {
    if start == notification_addr() {
        {
            let _g = lock();
            schedule_retry(arg);
            #[cfg(feature = "selftest")]
            ABANDONED.fetch_add(1, Relaxed);
        }
        crate::timer::retry_soon();
    }
}

/// AIO notifications handed back by `crate::stacks` so far (`selftest` only).
#[cfg(feature = "selftest")]
static ABANDONED: AtomicUsize = AtomicUsize::new(0);

/// AIO notifications abandoned so far ([`abandoned`]; `selftest` only).
#[cfg(feature = "selftest")]
pub(crate) fn abandoned_count() -> usize {
    ABANDONED.load(Relaxed)
}

/// How many request records are on the free list now (`selftest` only): a test sees a released
/// notification block come back.
#[cfg(feature = "selftest")]
pub(crate) fn free_records() -> usize {
    let _g = lock();
    let mut n = 0usize;
    let mut a = FREE_REC.load(Relaxed);
    while a != 0 {
        n = n.saturating_add(1);
        a = w(tb::words64(a, 1), 0);
    }
    n
}

// ------------------------------------------------------------------------------ records and queues

fn free_record(a: usize) {
    let r = rec(a);
    release_event(event(r, R_EVENT));
    put(r, R_STATE, ST_FREE);
    release(&FREE_REC, a);
}

fn bucket(fd: i32) -> Option<&'static AtomicUsize> {
    BUCKET.get(word_i32(fd) % BUCKETS)
}

/// Under the lock: the queue of descriptor `fd`, if it has one.
fn node_find(fd: i32) -> Option<usize> {
    let mut a = bucket(fd)?.load(Relaxed);
    while a != 0 {
        let n = node(a);
        if lo32(w(n, N_FD)) == fd {
            return Some(a);
        }
        a = w(n, N_NEXT);
    }
    None
}

/// Under the lock: the queue of descriptor `fd`, made if needed.
fn node_get(fd: i32) -> KResult<usize> {
    if let Some(a) = node_find(fd) {
        return Ok(a);
    }
    let b = bucket(fd).ok_or(Errno::EINVAL)?;
    let a = alloc(&FREE_NODE, N_WORDS)?;
    let n = node(a);
    put(n, N_FD, word_i32(fd));
    put(n, N_NEXT, b.load(Relaxed));
    b.store(a, Relaxed);
    Ok(a)
}

/// Under the lock: forget a queue that holds nothing.
fn node_drop_if_idle(a: usize) {
    let n = node(a);
    if w(n, N_HEAD) != 0 || w(n, N_RUNNING) != 0 {
        return;
    }
    let Some(b) = bucket(lo32(w(n, N_FD))) else { return };
    let mut prev: Option<&[AtomicUsize]> = None;
    let mut x = b.load(Relaxed);
    while x != 0 {
        let next = w(node(x), N_NEXT);
        if x == a {
            match prev {
                Some(p) => put(p, N_NEXT, next),
                None => b.store(next, Relaxed),
            }
            release(&FREE_NODE, a);
            return;
        }
        prev = Some(node(x));
        x = next;
    }
}

fn key(r: &[AtomicUsize]) -> (usize, usize, usize, usize) {
    (w(r, R_EPOCH), w(r, R_RANK), w(r, R_PRIO), w(r, R_SEQ))
}

/// Under the lock: queue record `a` on queue `na` by its key.
fn insert(na: usize, a: usize) {
    let n = node(na);
    let r = rec(a);
    put(r, R_NEXT, 0);
    let tail = w(n, N_TAIL);
    if tail == 0 {
        put(n, N_HEAD, a);
        put(n, N_TAIL, a);
        return;
    }
    if key(rec(tail)) <= key(r) {
        put(rec(tail), R_NEXT, a);
        put(n, N_TAIL, a);
        return;
    }
    let mut prev: Option<&[AtomicUsize]> = None;
    let mut x = w(n, N_HEAD);
    while x != 0 && key(rec(x)) <= key(r) {
        prev = Some(rec(x));
        x = w(rec(x), R_NEXT);
    }
    put(r, R_NEXT, x);
    match prev {
        Some(p) => put(p, R_NEXT, a),
        None => put(n, N_HEAD, a),
    }
}

/// Under the lock: take record `a` off queue `na`; whether it was there.
fn unqueue(na: usize, a: usize) -> bool {
    let n = node(na);
    let mut prev: Option<usize> = None;
    let mut x = w(n, N_HEAD);
    while x != 0 {
        let next = w(rec(x), R_NEXT);
        if x == a {
            match prev {
                Some(p) => put(rec(p), R_NEXT, next),
                None => put(n, N_HEAD, next),
            }
            if w(n, N_TAIL) == a {
                put(n, N_TAIL, prev.unwrap_or(0));
            }
            return true;
        }
        prev = Some(x);
        x = next;
    }
    false
}

/// The ready list record `a` goes on: the workers' while the ring runs and the request is
/// routed to them, else the engine's.
fn list_of(a: usize) -> &'static Ready {
    if ENGINE.load(Relaxed) == ENGINE_RING && w(rec(a), R_FLAGS) & F_WORKER != 0 { &WREADY } else { &READY }
}

fn ready_push(a: usize) {
    let l = list_of(a);
    put(rec(a), R_NEXT, 0);
    match l.tail.load(Relaxed) {
        0 => l.head.store(a, Relaxed),
        t => put(rec(t), R_NEXT, a),
    }
    l.tail.store(a, Relaxed);
    l.len.fetch_add(1, Relaxed);
}

fn ready_push_front(a: usize) {
    let l = list_of(a);
    put(rec(a), R_NEXT, l.head.load(Relaxed));
    l.head.store(a, Relaxed);
    if l.tail.load(Relaxed) == 0 {
        l.tail.store(a, Relaxed);
    }
    l.len.fetch_add(1, Relaxed);
    crate::sys::mark_aio_used(); // the direct close now takes the Rust path (CANCEL-FAST-2)
    PENDING.fetch_add(1, Relaxed);
    put(rec(a), R_STATE, ST_READY);
}

/// Under the lock: the oldest request of the engine's ready list, now running.
fn ready_pop() -> Option<usize> {
    pop_from(&READY)
}

/// Under the lock: the oldest request of list `l`, now running.
fn pop_from(l: &Ready) -> Option<usize> {
    let a = l.head.load(Relaxed);
    if a == 0 {
        return None;
    }
    let next = w(rec(a), R_NEXT);
    l.head.store(next, Relaxed);
    if next == 0 {
        l.tail.store(0, Relaxed);
    }
    l.len.fetch_sub(1, Relaxed);
    PENDING.fetch_sub(1, Relaxed);
    put(rec(a), R_STATE, ST_RUNNING);
    Some(a)
}

/// Under the lock: take ready record `a` off whichever ready list holds it; whether it was
/// there.
fn ready_remove(a: usize) -> bool {
    for l in [&READY, &WREADY] {
        let mut prev: Option<usize> = None;
        let mut x = l.head.load(Relaxed);
        while x != 0 {
            let next = w(rec(x), R_NEXT);
            if x == a {
                match prev {
                    Some(p) => put(rec(p), R_NEXT, next),
                    None => l.head.store(next, Relaxed),
                }
                if l.tail.load(Relaxed) == a {
                    l.tail.store(prev.unwrap_or(0), Relaxed);
                }
                l.len.fetch_sub(1, Relaxed);
                return true;
            }
            prev = Some(x);
            x = next;
        }
    }
    false
}

/// Under the lock: start the next request of queue `na` if none is running; forget the queue
/// when it is empty.
fn schedule(na: usize) {
    let n = node(na);
    if w(n, N_RUNNING) != 0 {
        return;
    }
    let h = w(n, N_HEAD);
    if h == 0 {
        node_drop_if_idle(na);
        return;
    }
    let r = rec(h);
    put(n, N_HEAD, w(r, R_NEXT));
    if w(n, N_HEAD) == 0 {
        put(n, N_TAIL, 0);
    }
    put(r, R_STATE, ST_READY);
    put(n, N_RUNNING, h);
    ready_push(h);
    kick();
}

/// Under the lock: make the engine take the ready list (and, with the ring, the workers the
/// requests routed to them).
fn kick() {
    match ENGINE.load(Relaxed) {
        ENGINE_RING => {
            wake_helper();
            if WREADY.len.load(Relaxed) != 0 {
                wake_workers(WREADY.len.load(Relaxed));
            }
        }
        ENGINE_WORKERS => wake_workers(READY.len.load(Relaxed)),
        _ => {}
    }
}

/// Under the lock: enough workers for `waiting` ready requests (up to the most allowed), and a
/// wake for them.
fn wake_workers(waiting: usize) {
    // With the ring, `aio_init`'s `aio_threads` bounds nothing (D-11.11): the transfers routed
    // to the workers get the default pool at least.
    let cap = if ENGINE.load(Relaxed) == ENGINE_RING { max_workers().max(DEFAULT_WORKERS) } else { max_workers() };
    while IDLE.load(Relaxed) < waiting && WORKERS.load(Relaxed) < cap {
        if add_worker().is_err() {
            break;
        }
    }
    WORK.notify_all(&CSys, Scope::Private);
}

/// Under the lock: store record `a`'s result in its block, wake the waiters and gather its
/// notifications (and its list's); the record is freed, or kept for its notification thread.
fn complete(a: usize, v: KResult<usize>, b: &mut Batch) {
    let r = rec(a);
    if let Some(c) = block_of(w(r, R_CB)) {
        match v {
            Ok(n) => set_status(c, 0, isize::try_from(n).unwrap_or(isize::MAX)),
            Err(e) => set_status(c, e.get(), -1),
        }
    }
    bump();
    let ev = event(r, R_EVENT);
    let own = notice_of(ev, a);
    let g = w(r, R_GROUP);
    if own.kind == K_THREAD {
        put(r, R_STATE, ST_NOTIFY);
    } else {
        free_record(a);
    }
    b.push(own);
    if g != 0 {
        group_done(g, b);
    }
}

/// Under the lock: a running request ended with `v`; the next of its descriptor starts.
fn finish(a: usize, v: KResult<usize>, b: &mut Batch) {
    let fd = lo32(w(rec(a), R_FD));
    let na = node_find(fd);
    if let Some(na) = na {
        if w(node(na), N_RUNNING) == a {
            put(node(na), N_RUNNING, 0);
        }
    }
    complete(a, v, b);
    if let Some(na) = na {
        schedule(na);
    }
}

/// Under the lock: cancel record `a`, queued or ready on queue `na`.
fn cancel_record(na: usize, a: usize, b: &mut Batch) {
    let n = node(na);
    match w(rec(a), R_STATE) {
        ST_QUEUED => {
            unqueue(na, a);
        }
        ST_READY => {
            ready_remove(a);
            if w(n, N_RUNNING) == a {
                put(n, N_RUNNING, 0);
            }
        }
        _ => return,
    }
    PENDING.fetch_sub(1, Relaxed);
    // A write cut short and waiting to continue reports what it wrote.
    let done = w(rec(a), R_DONE);
    complete(a, if done > 0 { Ok(done) } else { Err(Errno::ECANCELED) }, b);
}

/// Under the lock: the cancellable request of queue `na` (ready, else the first queued; for a
/// `close`, also the rest of a started request).
fn cancellable(na: usize, closing: bool) -> Option<usize> {
    let n = node(na);
    let r = w(n, N_RUNNING);
    if r != 0 && w(rec(r), R_STATE) == ST_READY && (closing || w(rec(r), R_FLAGS) & F_STARTED == 0) {
        return Some(r);
    }
    match w(n, N_HEAD) {
        0 => None,
        h => Some(h),
    }
}

// ------------------------------------------------------------------------------ list groups

/// Under the lock: a group for a `LIO_NOWAIT` list notifying as `e` says, counting one guard
/// entry that `lio_listio` drops when it has queued the list.
fn group_new(e: &SigEvent) -> KResult<usize> {
    let a = alloc(&FREE_GROUP, G_WORDS)?;
    let g = group(a);
    put(g, G_REMAINING, 1);
    let [function, attrs] = e.thread_words();
    if let Err(err) = set_event(event(g, G_EVENT), e.notify, e.signo, e.value, usize::try_from(function).unwrap_or(0), usize::try_from(attrs).unwrap_or(0)) {
        release(&FREE_GROUP, a);
        return Err(err);
    }
    Ok(a)
}

/// Under the lock: one of the group's requests (or its guard) is done.
fn group_done(a: usize, b: &mut Batch) {
    let g = group(a);
    let left = w(g, G_REMAINING).saturating_sub(1);
    put(g, G_REMAINING, left);
    if left != 0 {
        return;
    }
    LIST_DONE.fetch_add(1, SeqCst);
    if LIST_WAITERS.load(SeqCst) != 0 {
        let _ = futex::wake_all(&CSys, &LIST_DONE, Scope::Private);
    }
    let x = notice_of(event(g, G_EVENT), a | 1);
    if x.kind != K_THREAD {
        release_event(event(g, G_EVENT));
        release(&FREE_GROUP, a);
    }
    b.push(x);
}

// ------------------------------------------------------------------------------ submission

/// A request as its block describes it at submission.
struct Req {
    cb: usize,
    fd: i32,
    op: usize,
    prio: i32,
    buf: usize,
    nbytes: usize,
    offset: i64,
    notify: i32,
    signo: i32,
    value: u64,
    function: usize,
    attrs: usize,
}

fn read_req(cb: usize, c: &[AtomicUsize], op: usize) -> Req {
    let sn = w(c, W_SIGNO_NOTIFY);
    Req {
        cb,
        fd: lo32(w(c, W_FD_OP)),
        op,
        prio: lo32(w(c, W_PRIO)),
        buf: w(c, W_BUF),
        nbytes: w(c, W_NBYTES),
        offset: as_i64(w(c, W_OFFSET)),
        notify: hi32(sn),
        signo: lo32(sn),
        value: u64::try_from(w(c, W_VALUE)).unwrap_or(0),
        function: w(c, W_FUNCTION),
        attrs: w(c, W_ATTRS),
    }
}

/// Whether io_uring runs transfers on `fd` as `pread`/`pwrite` do: regular files and block
/// devices. On other descriptors (pipes, sockets, character devices) io_uring has differed
/// from the system calls (Linux 5.10: `EOPNOTSUPP` for `RWF_NOWAIT` on a pipe, socket reads
/// that never complete), so their transfers run on a worker with `read`/`write`.
fn ring_file(fd: i32) -> bool {
    CSys.fstat(Fd::new(fd)).is_ok_and(|s| matches!(s.mode & S_IFMT, S_IFREG | S_IFBLK))
}

fn is_rw(op: usize) -> bool {
    op == OP_READ || op == OP_WRITE
}

/// The errors reported at submission (beyond resources).
fn check(q: &Req) -> KResult<()> {
    if is_rw(q.op) && !(0..=AIO_PRIO_DELTA_MAX).contains(&q.prio) {
        return Err(Errno::EINVAL);
    }
    if q.notify == SIGEV_THREAD && q.function == 0 {
        return Err(Errno::EINVAL);
    }
    Ok(())
}

/// The error a request reports when its turn comes, without a transfer (0: none).
fn preset(q: &Req) -> usize {
    let bad_signal = q.notify == SIGEV_SIGNAL && !(0..=64).contains(&q.signo);
    if bad_signal || (is_rw(q.op) && q.offset < 0) { word_i32(Errno::EINVAL.get()) } else { 0 }
}

fn max_workers() -> usize {
    match MAX_WORKERS.load(Relaxed) {
        0 => DEFAULT_WORKERS,
        n => n,
    }
}

/// Whether io_uring must not be tried (see the module's documentation): the program or the
/// environment asked for threads, secure-execution mode, or a seccomp filter.
fn ring_excluded<K: Uring>(k: &K) -> bool {
    FORCE_WORKERS.load(Relaxed) != 0
        || crate::process::secure_getenv(b"LIELLA_AIO_URING").map(crate::strz::env_value) == Some(&b"0"[..])
        || crate::process::secure()
        || seccomp_filtered(k)
}

/// Whether a seccomp filter may be installed on the calling thread (which makes the ring and its
/// helper). First procfs, which needs no `prctl`: a `Seccomp:` value other than 0 settles it
/// (a filter that kills or lies on `prctl` is then never asked). Only when procfs says 0 or
/// cannot be read is `PR_GET_SECCOMP` asked, and anything but 0 from it (2 with a filter; a
/// filter may also make the call fail, with any error) counts as a filter too.
fn seccomp_filtered<K: Uring>(k: &K) -> bool {
    match liella::system::seccomp_mode(k) {
        Some(m) if m != 0 => true,
        _ => k.prctl_int(PrctlInt::GetSeccomp, 0, 0) != Ok(0),
    }
}

/// The kernel as the engine choice sees it: the `Sys` calls, and the io_uring calls, which are
/// not `Sys` methods (`raw`, D-X.9). Production: `CSys`. Tests: `Faulty<CSys>`
/// (`selftest::aio_choose_engine`), whose plan refuses any of them by name
/// (`liella_kernel::testing::RAW_FALLIBLE` for the io_uring ones).
pub(crate) trait Uring: Sys {
    /// `io_uring_setup` ([`raw::io_uring_setup`]).
    fn uring_setup(&self, entries: u32, params: &mut [u32; 30]) -> KResult<i32>;
    /// `io_uring_register(IORING_REGISTER_PROBE)` ([`raw::io_uring_probe`]).
    fn uring_probe(&self, fd: i32, buf: &mut [u8], nr: u32) -> KResult<()>;
}

impl Uring for CSys {
    fn uring_setup(&self, entries: u32, params: &mut [u32; 30]) -> KResult<i32> {
        raw::io_uring_setup(entries, params)
    }

    fn uring_probe(&self, fd: i32, buf: &mut [u8], nr: u32) -> KResult<()> {
        raw::io_uring_probe(fd, buf, nr)
    }
}

#[cfg(feature = "selftest")]
impl Uring for liella_kernel::testing::Faulty<CSys> {
    fn uring_setup(&self, entries: u32, params: &mut [u32; 30]) -> KResult<i32> {
        match self.take("io_uring_setup") {
            Some(e) => Err(e),
            None => self.inner().uring_setup(entries, params),
        }
    }

    fn uring_probe(&self, fd: i32, buf: &mut [u8], nr: u32) -> KResult<()> {
        match self.take("io_uring_register") {
            Some(e) => Err(e),
            None => self.inner().uring_probe(fd, buf, nr),
        }
    }
}

/// `liella_aio_use_threads`: run this process's requests on the worker threads, never on
/// io_uring. Before the first request (or once the workers were chosen) it holds for the life
/// of the process, `fork` children included; `EBUSY` once a request has started the ring (the
/// engine is chosen once; the ring is not retired under running requests).
pub fn use_threads() -> KResult<()> {
    let _g = lock();
    if ENGINE.load(Relaxed) == ENGINE_RING {
        return Err(Errno::EBUSY);
    }
    FORCE_WORKERS.store(1, Relaxed);
    Ok(())
}

/// Under the lock: the process's engine, chosen at its first request.
fn engine() -> KResult<usize> {
    engine_with(&CSys)
}

/// [`engine`], asking `k`.
fn engine_with<K: Uring>(k: &K) -> KResult<usize> {
    let e = ENGINE.load(Relaxed);
    if e != ENGINE_NONE {
        return Ok(e);
    }
    if ring_excluded(k) || ring_open(k).is_err() {
        ENGINE.store(ENGINE_WORKERS, Relaxed);
        return Ok(ENGINE_WORKERS);
    }
    let a = Attr { detached: true, stack_size: SERVICE_STACK, sigmask: Some(APP_SIGNALS), ..Attr::default() };
    match thread::create_service(&a, helper_addr(), 0) {
        Ok(_) => {
            ENGINE.store(ENGINE_RING, Relaxed);
            Ok(ENGINE_RING)
        }
        Err(_) => {
            ring_close();
            Err(Errno::EAGAIN)
        }
    }
}

/// Under the lock: queue a checked request (`g`: its list group, or 0). `EAGAIN` when no engine
/// can run it or without memory; nothing is queued then.
fn enqueue(q: &Req, g: usize) -> KResult<()> {
    let eng = engine()?;
    if eng == ENGINE_WORKERS && WORKERS.load(Relaxed) == 0 {
        add_worker().map_err(|_| Errno::EAGAIN)?;
    }
    let a = alloc(&FREE_REC, R_WORDS)?;
    let r = rec(a);
    if set_event(event(r, R_EVENT), q.notify, q.signo, q.value, q.function, q.attrs).is_err() {
        release(&FREE_REC, a);
        return Err(Errno::EAGAIN);
    }
    let na = match node_get(q.fd) {
        Ok(na) => na,
        Err(e) => {
            free_record(a);
            return Err(e);
        }
    };
    put(r, R_CB, q.cb);
    put(r, R_FD, word_i32(q.fd));
    put(r, R_OP, q.op);
    put(r, R_BUF, q.buf);
    put(r, R_NBYTES, q.nbytes);
    put(r, R_OFFSET, word_i64(q.offset));
    put(r, R_PRIO, word_i32(q.prio));
    put(r, R_PRESET, preset(q));
    put(r, R_GROUP, g);
    if eng == ENGINE_RING && is_rw(q.op) && !ring_file(q.fd) {
        put(r, R_FLAGS, F_WORKER);
    }
    let n = node(na);
    let epoch = w(n, N_EPOCH);
    put(r, R_EPOCH, epoch);
    if is_rw(q.op) {
        put(r, R_RANK, 0);
    } else {
        // A sync runs after everything queued before it and before everything after it.
        put(r, R_RANK, 1);
        put(r, R_PRIO, 0);
        put(n, N_EPOCH, epoch.wrapping_add(1));
    }
    put(r, R_SEQ, SEQ.fetch_add(1, Relaxed));
    put(r, R_STATE, ST_QUEUED);
    if let Some(c) = block_of(q.cb) {
        put(c, W_RETURN, 0);
        if let Some(x) = c.get(W_ERROR) {
            x.store(einprogress(), Release);
        }
    }
    if g != 0 {
        let gw = group(g);
        put(gw, G_REMAINING, w(gw, G_REMAINING).saturating_add(1));
    }
    crate::sys::mark_aio_used(); // the direct close now takes the Rust path (CANCEL-FAST-2)
    PENDING.fetch_add(1, Relaxed);
    insert(na, a);
    schedule(na);
    Ok(())
}

fn submit(cb: usize, op: usize) -> KResult<()> {
    let c = block_of(cb).ok_or(Errno::EINVAL)?;
    let q = read_req(cb, c, op);
    check(&q)?;
    let _g = lock();
    enqueue(&q, 0)
}

/// `aio_read(cb)`.
pub fn read(cb: usize) -> KResult<()> {
    submit(cb, OP_READ)
}

/// `aio_write(cb)`.
pub fn write(cb: usize) -> KResult<()> {
    submit(cb, OP_WRITE)
}

/// `aio_fsync(op, cb)`: the operation is checked first, then the descriptor.
pub fn fsync(op: i32, cb: usize) -> KResult<()> {
    let kind = match u32::from_ne_bytes(op.to_ne_bytes()) {
        O_SYNC => OP_SYNC,
        O_DSYNC => OP_DSYNC,
        _ => return Err(Errno::EINVAL),
    };
    let c = block_of(cb).ok_or(Errno::EINVAL)?;
    let fd = lo32(w(c, W_FD_OP));
    CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFd, 0).map_err(|_| Errno::EBADF)?;
    submit(cb, kind)
}

/// `aio_error(cb)`: the request's error, `EINPROGRESS` while it runs.
pub fn error(cb: usize) -> KResult<i32> {
    let c = block_of(cb).ok_or(Errno::EINVAL)?;
    Ok(lo32(c.get(W_ERROR).map_or(0, |x| x.load(Acquire))))
}

/// `aio_return(cb)`: the request's result (-1 without `errno` while it runs or when it failed;
/// `EINVAL` for a NULL or misaligned block).
pub fn status(cb: usize) -> (isize, Option<Errno>) {
    let Some(c) = block_of(cb) else { return (-1, Some(Errno::EINVAL)) };
    if lo32(c.get(W_ERROR).map_or(0, |x| x.load(Acquire))) == Errno::EINPROGRESS.get() {
        return (-1, None);
    }
    (isize::from_ne_bytes(w(c, W_RETURN).to_ne_bytes()), None)
}

/// `aio_init(init)`: the most worker threads (`aio_threads`, at least 1).
pub fn init(threads: Option<i32>) {
    if let Some(t) = threads {
        MAX_WORKERS.store(usize::try_from(t.max(1)).unwrap_or(1), Relaxed);
    }
}

// ------------------------------------------------------------------------------ waiting

/// The entries of a list, as addresses (0 for NULL).
fn entries(list: &[u8]) -> impl Iterator<Item = usize> + Clone + '_ {
    list.chunks_exact(8).map(|c| usize::from_ne_bytes(<[u8; 8]>::try_from(c).unwrap_or([0; 8])))
}

fn in_progress(cb: usize) -> bool {
    block_of(cb).is_some_and(|c| c.get(W_ERROR).is_some_and(|x| lo32(x.load(Acquire)) == Errno::EINPROGRESS.get()))
}

/// The end of a relative timeout on `CLOCK_MONOTONIC`; `None` when it has already passed (zero
/// or less, or an end the clock cannot express).
fn deadline(t: &Timespec) -> Option<Timespec> {
    let now = CSys.clock_gettime(CLOCK_MONOTONIC).unwrap_or_default();
    let span = i128::from(t.sec).saturating_mul(1_000_000_000).saturating_add(i128::from(t.nsec));
    if span <= 0 {
        return None;
    }
    let end = i128::from(now.sec).saturating_mul(1_000_000_000).saturating_add(i128::from(now.nsec)).saturating_add(span);
    let sec = i64::try_from(end.checked_div(1_000_000_000)?).ok()?;
    let nsec = i64::try_from(end.checked_rem(1_000_000_000)?).ok()?;
    Some(Timespec { sec, nsec })
}

/// `aio_suspend(list, nent, timeout)`.
pub fn suspend(nent: i32, list: KResult<&[u8]>, timeout: Option<&Timespec>) -> KResult<()> {
    if nent < 0 {
        return Err(Errno::EINVAL);
    }
    let list = list?;
    let end = timeout.map(deadline);
    loop {
        WAITERS.fetch_add(1, SeqCst);
        let seen = DONE.load(SeqCst);
        let (mut done, mut pending) = (false, false);
        for a in entries(list).filter(|&a| a != 0) {
            if in_progress(a) {
                pending = true;
            } else {
                done = true;
            }
        }
        let r = if done || !pending {
            Ok(true)
        } else {
            match end {
                Some(None) => Err(Errno::EAGAIN),
                Some(Some(at)) => match wait_until(seen, at) {
                    Err(Errno::ETIMEDOUT) => Err(Errno::EAGAIN),
                    r => r.map(|()| false),
                },
                // A wait with a deadline is never restarted after a handler (EINTR even with
                // SA_RESTART), so an unbounded wait sleeps a day at a time.
                None => match wait_until(seen, deadline(&Timespec { sec: 86_400, nsec: 0 }).unwrap_or_default()) {
                    Err(Errno::ETIMEDOUT) => Ok(false),
                    r => r.map(|()| false),
                },
            }
        };
        WAITERS.fetch_sub(1, SeqCst);
        if r? {
            return Ok(());
        }
    }
}

/// Sleep on [`DONE`] while it holds `seen`, until `at` (a cancellation point). `EAGAIN` from the
/// kernel (the word moved) is a normal return.
fn wait_until(seen: u32, at: Timespec) -> KResult<()> {
    match futex::wait_until(&CP, &DONE, seen, Scope::Private, Some((Clock::Monotonic, &at))) {
        Err(Errno::EAGAIN) | Ok(()) => Ok(()),
        Err(e) => Err(e),
    }
}

// ------------------------------------------------------------------------------ lio_listio

/// `lio_listio(mode, list, nent, sig)` (`list`: the entries' bytes; a negative `nent` is an
/// empty list).
pub fn listio(mode: i32, list: KResult<&[u8]>, sig: Option<&SigEvent>) -> KResult<()> {
    if mode != LIO_WAIT && mode != LIO_NOWAIT {
        return Err(Errno::EINVAL);
    }
    let list = list?;
    let notifying = mode == LIO_NOWAIT && sig.is_some_and(|e| e.notify == SIGEV_SIGNAL || e.notify == SIGEV_THREAD);
    if notifying && sig.is_some_and(|e| e.notify == SIGEV_THREAD && matches!(e.thread_words(), [0, _])) {
        return Err(Errno::EINVAL);
    }
    // A list is a group: its notification (`LIO_NOWAIT`), or the wake of `LIO_WAIT` (which
    // ignores the `sigevent`), when its last entry is done.
    let none = SigEvent::new(SIGEV_NONE, 0, 0, 0);
    let g = if notifying || mode == LIO_WAIT {
        let _g = lock();
        group_new(sig.filter(|_| notifying).unwrap_or(&none))?
    } else {
        0
    };
    let (mut refused, mut short) = (false, false);
    for a in entries(list).filter(|&a| a != 0) {
        let Some(c) = block_of(a) else { continue };
        let op = match hi32(w(c, W_FD_OP)) {
            LIO_READ => OP_READ,
            LIO_WRITE => OP_WRITE,
            LIO_NOP => continue,
            _ => {
                set_status(c, Errno::EINVAL.get(), -1);
                continue;
            }
        };
        if short {
            set_status(c, Errno::EAGAIN.get(), 0);
            continue;
        }
        let q = read_req(a, c, op);
        if let Err(e) = check(&q) {
            set_status(c, e.get(), -1);
            refused = true;
            continue;
        }
        let r = {
            let _g = lock();
            enqueue(&q, g)
        };
        if r.is_err() {
            set_status(c, Errno::EAGAIN.get(), 0);
            short = true;
        }
    }
    if g != 0 {
        let mut b = Batch::new();
        {
            let _g = lock();
            group_done(g, &mut b);
        }
        b.deliver();
    }
    if short {
        return Err(Errno::EAGAIN);
    }
    if mode == LIO_NOWAIT {
        return if refused { Err(Errno::EINVAL) } else { Ok(()) };
    }
    let counted = |a: usize| block_of(a).is_some_and(|c| hi32(w(c, W_FD_OP)) != LIO_NOP);
    loop {
        LIST_WAITERS.fetch_add(1, SeqCst);
        let seen = LIST_DONE.load(SeqCst);
        let busy = entries(list).filter(|&a| a != 0 && counted(a)).any(in_progress);
        // Without a deadline: a handler with SA_RESTART restarts the wait (the reference).
        let r = if busy { futex::wait(&CP, &LIST_DONE, seen, Scope::Private) } else { Ok(()) };
        LIST_WAITERS.fetch_sub(1, SeqCst);
        if !busy {
            break;
        }
        match r {
            // The word moved, or a wake: look again.
            Ok(()) | Err(Errno::EAGAIN) => {}
            // A handler without SA_RESTART, or the caller's cancellation (acted on at the export).
            Err(e) => return Err(e),
        }
    }
    let failed = entries(list).filter(|&a| a != 0 && counted(a)).any(|a| block_of(a).is_some_and(|c| lo32(w(c, W_ERROR)) != 0));
    if failed { Err(Errno::EIO) } else { Ok(()) }
}

// ------------------------------------------------------------------------------ aio_cancel

/// `aio_cancel(fd, cb)` (`cb` 0: every request on `fd`).
pub fn cancel(fd: Fd, cb: usize) -> KResult<i32> {
    CSys.fcntl_int(fd, FcntlInt::GetFd, 0).map_err(|_| Errno::EBADF)?;
    if cb != 0 {
        let c = block_of(cb).ok_or(Errno::EINVAL)?;
        if lo32(w(c, W_FD_OP)) != fd.raw() {
            return Err(Errno::EINVAL);
        }
        let mut b = Batch::new();
        let r = {
            let _g = lock();
            one_cancel(fd.raw(), cb, &mut b)
        };
        b.deliver();
        return Ok(r);
    }
    let (mut canceled, mut running) = (false, false);
    loop {
        let mut b = Batch::new();
        let more = {
            let _g = lock();
            let (c, r, more) = cancel_fd(fd.raw(), &mut b, false);
            canceled |= c;
            running |= r;
            more
        };
        b.deliver();
        if !more {
            break;
        }
    }
    Ok(if running {
        AIO_NOTCANCELED
    } else if canceled {
        AIO_CANCELED
    } else {
        AIO_ALLDONE
    })
}

/// Under the lock: cancel the request of block `cb` on descriptor `fd`.
fn one_cancel(fd: i32, cb: usize, b: &mut Batch) -> i32 {
    if let Some(na) = node_find(fd) {
        let n = node(na);
        let run = w(n, N_RUNNING);
        let mut hit = (run != 0 && w(rec(run), R_CB) == cb).then_some(run);
        if hit.is_some_and(|r| w(rec(r), R_STATE) != ST_READY || w(rec(r), R_FLAGS) & F_STARTED != 0) {
            return AIO_NOTCANCELED;
        }
        let mut x = w(n, N_HEAD);
        while hit.is_none() && x != 0 {
            if w(rec(x), R_CB) == cb {
                hit = Some(x);
            }
            x = w(rec(x), R_NEXT);
        }
        if let Some(x) = hit {
            cancel_record(na, x, b);
            schedule(na);
            return AIO_CANCELED;
        }
    }
    // Not a request of this process (a `fork` child's copy of its parent's block) or done.
    if in_progress(cb) { AIO_NOTCANCELED } else { AIO_ALLDONE }
}

/// Under the lock: cancel the requests of descriptor `fd` that have not started, as many as the
/// batch holds: (any cancelled, one is running, more remain).
fn cancel_fd(fd: i32, b: &mut Batch, closing: bool) -> (bool, bool, bool) {
    let Some(na) = node_find(fd) else { return (false, false, false) };
    let mut canceled = false;
    while b.room() {
        let Some(a) = cancellable(na, closing) else { break };
        cancel_record(na, a, b);
        canceled = true;
    }
    let more = cancellable(na, closing).is_some();
    let n = node(na);
    let running = w(n, N_RUNNING) != 0;
    node_drop_if_idle(na);
    (canceled, running, more)
}

// ------------------------------------------------------------------------------ descriptors closed

/// Before `fd` is closed: cancel its requests that have not started. Returns `fd`.
pub fn closing(fd: Fd) -> Fd {
    closing_where(Some(fd.raw()), |d| d == fd.raw());
    fd
}

/// Before `close_range(first, last, flags)`: as [`closing`] for each descriptor it closes
/// (none with `CLOSE_RANGE_CLOEXEC`). Returns `first`.
pub fn closing_range(first: u32, last: u32, flags: u32) -> u32 {
    if flags & 4 == 0 {
        closing_where(None, |d| u32::try_from(d).is_ok_and(|d| first <= d && d <= last));
    }
    first
}

/// Before `closefrom(lo)`. Returns `lo`.
pub fn closing_from(lo: i32) -> i32 {
    closing_where(None, |d| d >= lo.max(0));
    lo
}

/// Before `dup2(old, new)`/`dup3`: `new` is closed when it differs from `old` and `old` is open
/// (otherwise the call fails and closes nothing). Returns `new`.
pub fn closing_dup(old: Fd, new: Fd) -> Fd {
    let busy = PENDING.load(Acquire) != 0 || [RING_FD.load(Acquire), EVENT_FD.load(Acquire)].contains(&new.raw());
    if busy && old.raw() != new.raw() && new.raw() >= 0 && CSys.fcntl_int(old, FcntlInt::GetFd, 0).is_ok() {
        closing(new);
    }
    new
}

/// Cancel the requests not started on the descriptors `hit` accepts (`only`: the one descriptor,
/// when there is just one).
fn closing_where(only: Option<i32>, hit: impl Fn(i32) -> bool) {
    let internal = [RING_FD.load(Acquire), EVENT_FD.load(Acquire)].into_iter().any(|d| d >= 0 && hit(d));
    if PENDING.load(Acquire) == 0 && !internal {
        return;
    }
    // A signal handler interrupting this thread's own critical section: nothing can be changed
    // safely, and waiting would never end.
    if OWNER.load(Relaxed) == thread::own_tid() {
        return;
    }
    loop {
        let mut b = Batch::new();
        let more = {
            let _g = lock();
            if internal {
                retire_ring(&hit);
            }
            let mut more = false;
            let slots: &[AtomicUsize] = match only {
                Some(fd) => bucket(fd).map_or(&[][..], core::slice::from_ref),
                None => &BUCKET,
            };
            for slot in slots {
                let mut a = slot.load(Relaxed);
                while a != 0 && b.room() {
                    let n = node(a);
                    let next = w(n, N_NEXT);
                    let fd = lo32(w(n, N_FD));
                    if hit(fd) {
                        more |= cancel_fd(fd, &mut b, true).2;
                    }
                    a = next;
                }
                if !b.room() {
                    more = true;
                    break;
                }
            }
            more
        };
        b.deliver();
        if !more {
            break;
        }
    }
}

// ------------------------------------------------------------------------------ worker engine

#[allow(clippy::as_conversions)] // a function pointer's address: the only spelling Rust has
fn worker_addr() -> usize {
    worker_main as extern "C" fn(usize) -> usize as usize
}

/// Under the lock: one more worker (idle until it takes a request).
fn add_worker() -> KResult<()> {
    let a = Attr { detached: true, stack_size: SERVICE_STACK, sigmask: Some(APP_SIGNALS), ..Attr::default() };
    thread::create_service(&a, worker_addr(), 0)?;
    WORKERS.fetch_add(1, Relaxed);
    IDLE.fetch_add(1, Relaxed);
    Ok(())
}

/// A worker: runs ready requests one at a time, for as long as the process runs.
extern "C" fn worker_main(_: usize) -> usize {
    loop {
        let a = {
            let _g = lock();
            loop {
                // The requests routed to the workers while the ring runs; all of them otherwise.
                let mine = pop_from(&WREADY).or_else(|| if ENGINE.load(Relaxed) == ENGINE_WORKERS { ready_pop() } else { None });
                if let Some(a) = mine {
                    IDLE.fetch_sub(1, Relaxed);
                    break a;
                }
                OWNER.store(0, Relaxed);
                WORK.wait(&CSys, &LOCK, Scope::Private);
                OWNER.store(thread::own_tid(), Relaxed);
            }
        };
        let v = perform(rec(a));
        let mut b = Batch::new();
        {
            let _g = lock();
            IDLE.fetch_add(1, Relaxed);
            finish(a, v, &mut b);
        }
        b.deliver();
    }
}

/// Run a request with one system call (a worker; and the rest of a ring request when the ring
/// is gone).
fn perform(r: &[AtomicUsize]) -> KResult<usize> {
    let preset = lo32(w(r, R_PRESET));
    if preset != 0 {
        return Err(Errno::new(preset).unwrap_or(Errno::EINVAL));
    }
    let fd = lo32(w(r, R_FD));
    let op = w(r, R_OP);
    if !is_rw(op) {
        let f = Fd::new(fd);
        return if op == OP_SYNC { CSys.fsync(f) } else { CSys.fdatasync(f) }.map(|()| 0);
    }
    let done = w(r, R_DONE);
    let buf = w(r, R_BUF).wrapping_add(done);
    let n = w(r, R_NBYTES).saturating_sub(done);
    let off = as_i64(w(r, R_OFFSET)).saturating_add(i64::try_from(done).unwrap_or(0));
    let mut stream = w(r, R_FLAGS) & F_STREAM != 0;
    let out = loop {
        let v = match (op == OP_READ, stream) {
            (true, false) => raw::aio_pread(fd, buf, n, off),
            (true, true) => raw::aio_read(fd, buf, n),
            (false, false) => raw::aio_pwrite(fd, buf, n, off),
            (false, true) => raw::aio_write(fd, buf, n),
        };
        match v {
            Err(Errno::ESPIPE) if !stream => stream = true,
            Err(Errno::EINTR) => {}
            v => break v,
        }
    };
    match out {
        Ok(k) => Ok(done.saturating_add(k)),
        Err(e) if done == 0 => Err(e),
        Err(_) => Ok(done),
    }
}

// ------------------------------------------------------------------------------ io_uring engine

// The kernel's constants (⚑ KERN, `<linux/io_uring.h>`).
const SETUP_CQSIZE: u32 = 1 << 3;
const SETUP_CLAMP: u32 = 1 << 4;
const SETUP_SUBMIT_ALL: u32 = 1 << 7;
const SETUP_COOP_TASKRUN: u32 = 1 << 8;
const FEAT_SINGLE_MMAP: u32 = 1 << 0;
const FEAT_NODROP: u32 = 1 << 1;
const OFF_CQ_RING: i64 = 0x800_0000;
const OFF_SQES: i64 = 0x1000_0000;
const ENTER_GETEVENTS: u32 = 1;
const OPC_FSYNC: u8 = 3;
const OPC_POLL_ADD: u8 = 6;
const OPC_READ: u8 = 22;
const OPC_WRITE: u8 = 23;
const FSYNC_DATASYNC: u32 = 1;
const RWF_NOWAIT: u32 = 8;
const POLLIN: u32 = 1;
const MAP_SHARED_POPULATE: i32 = 0x01 | 0x8000;
const SQ_ENTRIES: u32 = 256;
const CQ_ENTRIES: u32 = 4096;

static RING_FD: AtomicI32 = AtomicI32::new(-1);
static EVENT_FD: AtomicI32 = AtomicI32::new(-1);
/// The mappings: rings (address, length), entries, completion ring (address 0: in the first).
static MAPS: [AtomicUsize; 6] = [const { AtomicUsize::new(0) }; 6];
/// Word indices: submission head, tail, array; completion head, tail, entries. Then values:
/// submission mask and size, completion mask.
static OFFS: [AtomicUsize; 9] = [const { AtomicUsize::new(0) }; 9];
const O_SQ_HEAD: usize = 0;
const O_SQ_TAIL: usize = 1;
const O_SQ_ARRAY: usize = 2;
const O_CQ_HEAD: usize = 3;
const O_CQ_TAIL: usize = 4;
const O_CQES: usize = 5;
const O_SQ_MASK: usize = 6;
const O_SQ_SIZE: usize = 7;
const O_CQ_MASK: usize = 8;
/// Requests in the kernel, and the most allowed (the completion queue's room).
static INFLIGHT: AtomicUsize = AtomicUsize::new(0);
static LIMIT: AtomicUsize = AtomicUsize::new(0);
/// The helper is (about to be) asleep and needs the eventfd to wake.
static WAITING: AtomicUsize = AtomicUsize::new(0);
/// The ring can no longer be used (its descriptor was closed or refused): the workers run the
/// requests, and the helper only collects what is in the kernel.
static DEAD: AtomicUsize = AtomicUsize::new(0);

fn pv(p: &[u32; 30], i: usize) -> u32 {
    p.get(i).copied().unwrap_or(0)
}

fn usz(v: u32) -> usize {
    usize::try_from(v).unwrap_or(0)
}

/// Under the lock: set up the ring and its eventfd; any failure leaves nothing behind.
fn ring_open<K: Uring>(k: &K) -> KResult<()> {
    // The newest flags first; an older kernel refuses the ones it does not know (EINVAL).
    let mut made = Err(Errno::EINVAL);
    for flags in [SETUP_CQSIZE | SETUP_CLAMP | SETUP_SUBMIT_ALL | SETUP_COOP_TASKRUN, SETUP_CQSIZE | SETUP_CLAMP, 0] {
        let mut p = [0u32; 30];
        if let Some(x) = p.get_mut(1) {
            *x = CQ_ENTRIES;
        }
        if let Some(x) = p.get_mut(2) {
            *x = flags;
        }
        match k.uring_setup(SQ_ENTRIES, &mut p) {
            Err(Errno::EINVAL) => {}
            r => {
                made = r.map(|fd| (fd, p));
                break;
            }
        }
    }
    let (fd, p) = made?;
    // Kept for the life of the process: never a standard descriptor (validation T12).
    let fd = liella::io::above_standard(k, Fd::new(fd))?.raw();
    crate::sys::mark_aio_used(); // the direct close now takes the Rust path (CANCEL-FAST-2)
    RING_FD.store(fd, Release);
    let r = ring_map(k, fd, &p);
    if r.is_err() {
        ring_close();
    }
    r
}

fn mmap_ring(fd: i32, len: usize, off: i64) -> KResult<usize> {
    raw::mmap(0, len, 3, MAP_SHARED_POPULATE, fd, off)
}

fn ring_map<K: Uring>(k: &K, fd: i32, p: &[u32; 30]) -> KResult<()> {
    let features = pv(p, 5);
    if features & FEAT_NODROP == 0 {
        return Err(Errno::EINVAL);
    }
    let (sq_size, cq_size) = (usz(pv(p, 0)), usz(pv(p, 1)));
    let sq_len = usz(pv(p, 16)).saturating_add(sq_size.saturating_mul(4));
    let cq_len = usz(pv(p, 25)).saturating_add(cq_size.saturating_mul(16));
    let single = features & FEAT_SINGLE_MMAP != 0;
    let sq_map = if single { sq_len.max(cq_len) } else { sq_len };
    let sq = mmap_ring(fd, sq_map, 0)?;
    store_map(0, sq, sq_map);
    if !single {
        let cq = mmap_ring(fd, cq_len, OFF_CQ_RING)?;
        store_map(4, cq, cq_len);
    }
    let sqes_len = sq_size.saturating_mul(64);
    let sqes = mmap_ring(fd, sqes_len, OFF_SQES)?;
    store_map(2, sqes, sqes_len);
    // The operations the engine uses must all be there.
    let mut probe = [0u8; 16 + 8 * 32];
    k.uring_probe(fd, &mut probe, 32)?;
    let supported = |op: u8| probe.get(16usize.saturating_add(8usize.saturating_mul(usize::from(op))).saturating_add(2)).is_some_and(|f| f & 1 != 0);
    if !(probe.first().is_some_and(|&last| last >= OPC_WRITE) && [OPC_FSYNC, OPC_POLL_ADD, OPC_READ, OPC_WRITE].into_iter().all(supported)) {
        return Err(Errno::ENOSYS);
    }
    let ev = liella::io::above_standard(k, k.eventfd2(0, EFD_NONBLOCK | EFD_CLOEXEC)?)?;
    crate::sys::mark_aio_used(); // the direct close now takes the Rust path (CANCEL-FAST-2)
    EVENT_FD.store(ev.raw(), Release);
    k.fcntl_int(Fd::new(fd), FcntlInt::SetFd, 1)?;
    let idx = |byte: u32| usz(byte) / 4;
    for (slot, v) in OFFS.iter().zip([idx(pv(p, 10)), idx(pv(p, 11)), idx(pv(p, 16)), idx(pv(p, 20)), idx(pv(p, 21)), idx(pv(p, 25)), sq_size.saturating_sub(1), sq_size, cq_size.saturating_sub(1)]) {
        slot.store(v, Release);
    }
    // One completion slot for the eventfd's poll, and some slack.
    LIMIT.store(cq_size.saturating_sub(8).max(1), Release);
    Ok(())
}

fn store_map(at: usize, addr: usize, len: usize) {
    if let (Some(a), Some(l)) = (MAPS.get(at), MAPS.get(at.saturating_add(1))) {
        a.store(addr, Release);
        l.store(len, Release);
    }
}

/// Undo a ring setup (before any view of the mappings exists).
fn ring_close() {
    for at in [0, 2, 4] {
        let (a, l) = (MAPS.get(at).map_or(0, |x| x.swap(0, AcqRel)), MAPS.get(at.saturating_add(1)).map_or(0, |x| x.swap(0, AcqRel)));
        if a != 0 && l != 0 {
            let _ = raw::munmap(a, l);
        }
    }
    for d in [&RING_FD, &EVENT_FD] {
        let fd = d.swap(-1, AcqRel);
        if fd >= 0 {
            let _ = CSys.close(Fd::new(fd));
        }
    }
}

/// The ring's views, as the helper uses them.
struct Ring {
    fd: i32,
    sq: &'static [AtomicU32],
    sqes: &'static [AtomicU32],
    cq: &'static [AtomicU32],
    o: [usize; 9],
}

fn map_at(i: usize) -> usize {
    MAPS.get(i).map_or(0, |x| x.load(Acquire))
}

impl Ring {
    fn load() -> Option<Ring> {
        let sq = conv::ring_words(map_at(0), map_at(1) / 4)?;
        let sqes = conv::ring_words(map_at(2), map_at(3) / 4)?;
        let cq = if map_at(4) == 0 { sq } else { conv::ring_words(map_at(4), map_at(5) / 4)? };
        let mut o = [0usize; 9];
        for (d, s) in o.iter_mut().zip(OFFS.iter()) {
            *d = s.load(Acquire);
        }
        Some(Ring { fd: RING_FD.load(Acquire), sq, sqes, cq, o })
    }

    fn off(&self, i: usize) -> usize {
        self.o.get(i).copied().unwrap_or(0)
    }

    fn sqw(&self, i: usize) -> Option<&AtomicU32> {
        self.sq.get(self.off(i))
    }

    fn cqw(&self, i: usize) -> Option<&AtomicU32> {
        self.cq.get(self.off(i))
    }

    fn mask(&self, i: usize) -> u32 {
        u32::try_from(self.off(i)).unwrap_or(0)
    }

    /// Entries written but not yet consumed by the kernel.
    fn unsubmitted(&self) -> u32 {
        let tail = self.sqw(O_SQ_TAIL).map_or(0, |x| x.load(Relaxed));
        let head = self.sqw(O_SQ_HEAD).map_or(0, |x| x.load(Acquire));
        tail.wrapping_sub(head)
    }

    /// Write a submission entry; false when the ring is full.
    fn push(&self, e: &[u32; 16]) -> bool {
        let (Some(tail_w), Some(head_w)) = (self.sqw(O_SQ_TAIL), self.sqw(O_SQ_HEAD)) else { return false };
        let tail = tail_w.load(Relaxed);
        if tail.wrapping_sub(head_w.load(Acquire)) >= self.mask(O_SQ_SIZE) {
            return false;
        }
        let idx = tail & self.mask(O_SQ_MASK);
        let slot = usz(idx);
        let base = slot.saturating_mul(16);
        for (k, v) in e.iter().enumerate() {
            if let Some(x) = self.sqes.get(base.saturating_add(k)) {
                x.store(*v, Relaxed);
            }
        }
        if let Some(x) = self.sq.get(self.off(O_SQ_ARRAY).saturating_add(slot)) {
            x.store(idx, Relaxed);
        }
        tail_w.store(tail.wrapping_add(1), Release);
        true
    }

    fn has_cqe(&self) -> bool {
        let head = self.cqw(O_CQ_HEAD).map_or(0, |x| x.load(Relaxed));
        let tail = self.cqw(O_CQ_TAIL).map_or(0, |x| x.load(Acquire));
        head != tail
    }

    /// The next completion: (user data, result).
    fn pop(&self) -> Option<(usize, i32)> {
        let head_w = self.cqw(O_CQ_HEAD)?;
        let head = head_w.load(Relaxed);
        if head == self.cqw(O_CQ_TAIL)?.load(Acquire) {
            return None;
        }
        let base = self.off(O_CQES).saturating_add(usz(head & self.mask(O_CQ_MASK)).saturating_mul(4));
        let at = |k: usize| self.cq.get(base.saturating_add(k)).map_or(0, |x| x.load(Relaxed));
        let ud = usz(at(0)) | usz(at(1)) << 32;
        let res = i32::from_ne_bytes(at(2).to_ne_bytes());
        head_w.store(head.wrapping_add(1), Release);
        Some((ud, res))
    }

    /// The user data of the entries the kernel has not consumed (the ring is being abandoned).
    fn unconsumed(&self, mut each: impl FnMut(usize)) {
        let tail = self.sqw(O_SQ_TAIL).map_or(0, |x| x.load(Relaxed));
        let mut i = self.sqw(O_SQ_HEAD).map_or(0, |x| x.load(Acquire));
        while i != tail {
            let base = usz(i & self.mask(O_SQ_MASK)).saturating_mul(16);
            let at = |k: usize| self.sqes.get(base.saturating_add(k)).map_or(0, |x| x.load(Relaxed));
            each(usz(at(8)) | usz(at(9)) << 32);
            i = i.wrapping_add(1);
        }
    }
}

fn split(v: u64) -> (u32, u32) {
    (u32::try_from(v & 0xffff_ffff).unwrap_or(0), u32::try_from(v >> 32).unwrap_or(0))
}

/// A submission entry's words.
fn sqe(opcode: u8, fd: i32, off: u64, addr: usize, len: u32, op_flags: u32, ud: usize) -> [u32; 16] {
    let (o0, o1) = split(off);
    let (a0, a1) = split(u64::try_from(addr).unwrap_or(0));
    let (u0, u1) = split(u64::try_from(ud).unwrap_or(0));
    [u32::from(opcode), u32::from_ne_bytes(fd.to_ne_bytes()), o0, o1, a0, a1, len, op_flags, u0, u1, 0, 0, 0, 0, 0, 0]
}

/// Under the lock: wake the helper if it sleeps.
fn wake_helper() {
    if WAITING.swap(0, Relaxed) != 0 {
        let ev = EVENT_FD.load(Acquire);
        if ev >= 0 {
            let _ = CSys.write(Fd::new(ev), &1u64.to_ne_bytes());
        }
    }
}

/// Under the lock: the application is closing the ring's descriptor or the eventfd (`hit`). The
/// ring is no longer used, the workers take the ready requests, and neither number is used
/// again by the library (it may be someone else's soon; the ring stays alive through its
/// mappings until the helper has collected what the kernel holds).
fn retire_ring(hit: &dyn Fn(i32) -> bool) {
    for d in [&RING_FD, &EVENT_FD] {
        let fd = d.load(Relaxed);
        if fd >= 0 && hit(fd) {
            d.store(-1, Release);
        }
    }
    EVENT_FD.store(-1, Release);
    if ENGINE.load(Relaxed) == ENGINE_RING {
        DEAD.store(1, Release);
        ENGINE.store(ENGINE_WORKERS, Relaxed);
        kick();
    }
}

/// Under the lock: the submission entry of a ready request, or its result when it ends
/// without one.
fn start_entry(a: usize) -> Result<[u32; 16], KResult<usize>> {
    let r = rec(a);
    let preset = lo32(w(r, R_PRESET));
    if preset != 0 {
        return Err(Err(Errno::new(preset).unwrap_or(Errno::EINVAL)));
    }
    let fd = lo32(w(r, R_FD));
    let op = w(r, R_OP);
    if !is_rw(op) {
        return Ok(sqe(OPC_FSYNC, fd, 0, 0, 0, if op == OP_DSYNC { FSYNC_DATASYNC } else { 0 }, a));
    }
    let (buf, n) = (w(r, R_BUF), w(r, R_NBYTES));
    if buf.checked_add(n).is_none_or(|end| end > USER_END) {
        return Err(Err(Errno::EFAULT));
    }
    // O_NONBLOCK, where the descriptor honours it: never wait (io_uring would poll).
    let fl = CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFl, 0).unwrap_or(0);
    if fl & i64::from(O_NONBLOCK) != 0 {
        let mode = CSys.fstat(Fd::new(fd)).map_or(0, |s| s.mode & S_IFMT);
        if mode != S_IFREG && mode != S_IFBLK {
            put(r, R_FLAGS, w(r, R_FLAGS) | F_NOWAIT);
        }
    }
    put(r, R_FLAGS, w(r, R_FLAGS) | F_STARTED);
    Ok(transfer_entry(a))
}

/// The submission entry of a read or write, from what is left of it.
fn transfer_entry(a: usize) -> [u32; 16] {
    let r = rec(a);
    let done = w(r, R_DONE);
    let left = w(r, R_NBYTES).min(MAX_RW).saturating_sub(done);
    let flags = w(r, R_FLAGS);
    let off = if flags & F_STREAM != 0 { 0 } else { as_i64(w(r, R_OFFSET)).saturating_add(i64::try_from(done).unwrap_or(0)) };
    let opcode = if w(r, R_OP) == OP_READ { OPC_READ } else { OPC_WRITE };
    let rw = if flags & F_NOWAIT != 0 { RWF_NOWAIT } else { 0 };
    sqe(opcode, lo32(w(r, R_FD)), u64::from_ne_bytes(off.to_ne_bytes()), w(r, R_BUF).wrapping_add(done), u32::try_from(left).unwrap_or(u32::MAX), rw, a)
}

/// What a completion means for its request: run the rest, or this result.
enum Step {
    Again,
    Final(KResult<usize>),
}

fn step(a: usize, res: i32) -> Step {
    let r = rec(a);
    let op = w(r, R_OP);
    if !is_rw(op) {
        return Step::Final(if res < 0 { Err(Errno::new(res.saturating_neg()).unwrap_or(Errno::EIO)) } else { Ok(0) });
    }
    let flags = w(r, R_FLAGS);
    if res == Errno::ESPIPE.get().saturating_neg() && flags & F_STREAM == 0 {
        // A descriptor without positions refuses an offset: the offset is ignored.
        put(r, R_FLAGS, flags | F_STREAM);
        return Step::Again;
    }
    let done = w(r, R_DONE);
    match usize::try_from(res) {
        Ok(k) => {
            let total = done.saturating_add(k);
            // A blocking write goes on until all of it is written (write(2) on a pipe).
            if op == OP_WRITE && k > 0 && flags & F_NOWAIT == 0 && total < w(r, R_NBYTES).min(MAX_RW) {
                put(r, R_DONE, total);
                return Step::Again;
            }
            Step::Final(Ok(total))
        }
        Err(_) if done > 0 => Step::Final(Ok(done)),
        // The ring's EFAULT for a count beyond the buffer, where the system call transfers what
        // the file holds (Linux 5.10, `aio_read` of `INT_MAX + 1` bytes into 4096): the system
        // call decides, on a worker.
        Err(_) if res == Errno::EFAULT.get().saturating_neg() && flags & F_WORKER == 0 => {
            put(r, R_FLAGS, flags | F_WORKER);
            Step::Again
        }
        Err(_) => Step::Final(Err(Errno::new(res.saturating_neg()).unwrap_or(Errno::EIO))),
    }
}

#[allow(clippy::as_conversions)] // a function pointer's address: the only spelling Rust has
fn helper_addr() -> usize {
    helper_main as extern "C" fn(usize) -> usize as usize
}

/// A completion's request is done: finish it (without the lock held).
fn ring_final(a: usize, v: KResult<usize>) {
    let mut b = Batch::new();
    {
        let _g = lock();
        INFLIGHT.fetch_sub(1, Relaxed);
        finish(a, v, &mut b);
    }
    b.deliver();
}

/// The helper: the ring's only submitter, for as long as the process runs.
extern "C" fn helper_main(_: usize) -> usize {
    let Some(ring) = Ring::load() else { park() };
    let ev = EVENT_FD.load(Acquire);
    let mut armed = false;
    loop {
        while let Some((ud, res)) = ring.pop() {
            if ud == 0 {
                // The eventfd's poll fired: clear it (unless it was retired: its number may be
                // someone else's now).
                if EVENT_FD.load(Acquire) == ev {
                    let mut buf = [0u8; 8];
                    let _ = CSys.read(Fd::new(ev), &mut buf);
                }
                armed = false;
                continue;
            }
            match step(ud, res) {
                Step::Final(v) => ring_final(ud, v),
                Step::Again => {
                    // The rest starts like a new request, under the lock (a `close` meanwhile
                    // cancels it there instead of racing the submission).
                    let _g = lock();
                    INFLIGHT.fetch_sub(1, Relaxed);
                    ready_push_front(ud);
                    // A rest routed to the workers (`F_WORKER`) needs them awake.
                    kick();
                }
            }
        }
        let mut b = Batch::new();
        let wait = {
            let _g = lock();
            WAITING.store(0, Relaxed);
            if DEAD.load(Acquire) != 0 {
                None
            } else {
                if !armed {
                    armed = ring.push(&sqe(OPC_POLL_ADD, ev, 0, 0, 0, POLLIN, 0));
                }
                let mut empty = false;
                while INFLIGHT.load(Relaxed) < LIMIT.load(Relaxed) && b.room() {
                    let Some(a) = ready_pop() else {
                        empty = true;
                        break;
                    };
                    match start_entry(a) {
                        Ok(e) => {
                            if !ring.push(&e) {
                                ready_push_front(a);
                                break;
                            }
                            INFLIGHT.fetch_add(1, Relaxed);
                        }
                        Err(v) => finish(a, v, &mut b),
                    }
                }
                let n = ring.unsubmitted();
                let entered = if n > 0 { raw::io_uring_enter(ring.fd, n, 0, 0).map(drop) } else { Ok(()) };
                match entered {
                    Err(e) if !matches!(e, Errno::EINTR | Errno::EAGAIN | Errno::EBUSY) => {
                        abandon(&ring);
                        None
                    }
                    // The kernel is short of resources: wait for a completion to free some, or
                    // (none in flight) a moment.
                    Err(Errno::EAGAIN | Errno::EBUSY) => {
                        if INFLIGHT.load(Relaxed) == 0 {
                            let _ = CSys.clock_nanosleep(CLOCK_MONOTONIC, 0, &Timespec { sec: 0, nsec: 1_000_000 }, None);
                        }
                        Some(INFLIGHT.load(Relaxed) > 0)
                    }
                    _ => {
                        // Sleep only with nothing to do: the ready list empty, every entry
                        // submitted, the eventfd's poll armed (an application wakes it) and no
                        // completion waiting.
                        let idle = empty && armed && ring.unsubmitted() == 0 && !ring.has_cqe();
                        if idle {
                            WAITING.store(1, Relaxed);
                        }
                        Some(idle || INFLIGHT.load(Relaxed) >= LIMIT.load(Relaxed))
                    }
                }
            }
        };
        b.deliver();
        match wait {
            None => drain(&ring),
            Some(true) if DEAD.load(Acquire) == 0 => {
                if let Err(e) = raw::io_uring_enter(ring.fd, 0, 1, ENTER_GETEVENTS) {
                    if !matches!(e, Errno::EINTR | Errno::EAGAIN | Errno::EBUSY) {
                        let _g = lock();
                        abandon(&ring);
                    }
                }
            }
            Some(_) => {}
        }
    }
}

/// Under the lock: stop using the ring; the entries the kernel has not consumed go back to the
/// ready list for the workers.
fn abandon(ring: &Ring) {
    if ENGINE.load(Relaxed) == ENGINE_RING {
        DEAD.store(1, Release);
        ENGINE.store(ENGINE_WORKERS, Relaxed);
    }
    ring.unconsumed(|ud| {
        if ud != 0 {
            INFLIGHT.fetch_sub(1, Relaxed);
            ready_push_front(ud);
        }
    });
    // Nothing is submitted from this ring again: forget the entries just handed back.
    if let (Some(t), Some(h)) = (ring.sqw(O_SQ_TAIL), ring.sqw(O_SQ_HEAD)) {
        t.store(h.load(Acquire), Release);
    }
    kick();
}

/// The helper once the ring is abandoned: collect what the kernel still holds, then rest.
fn drain(ring: &Ring) -> ! {
    {
        let _g = lock();
        abandon(ring);
    }
    loop {
        while let Some((ud, res)) = ring.pop() {
            if ud == 0 {
                continue;
            }
            match step(ud, res) {
                Step::Final(v) => ring_final(ud, v),
                Step::Again => {
                    let _g = lock();
                    INFLIGHT.fetch_sub(1, Relaxed);
                    ready_push_front(ud);
                    kick();
                }
            }
        }
        if INFLIGHT.load(Relaxed) == 0 {
            park();
        }
        let _ = CSys.clock_nanosleep(CLOCK_MONOTONIC, 0, &Timespec { sec: 0, nsec: 10_000_000 }, None);
    }
}

/// Rest forever (a service thread with nothing left to do).
fn park() -> ! {
    static NEVER: AtomicU32 = AtomicU32::new(0);
    loop {
        let _ = futex::wait(&CSys, &NEVER, 0, Scope::Private);
    }
}

// ------------------------------------------------------------------------------ processes

/// A child of `fork`: no request, ring, helper or worker is inherited. Async-signal-safe
/// (`_Fork`): no allocation, and the lock is reset rather than taken. The free lists stay (their
/// blocks are this process's memory); records in use by the parent are left alone.
pub(crate) fn after_fork_child() {
    LOCK.reset();
    OWNER.store(0, Relaxed);
    ENGINE.store(ENGINE_NONE, Relaxed);
    for x in [&WORKERS, &IDLE, &PENDING, &WAITERS, &LIST_WAITERS, &INFLIGHT, &WAITING, &DEAD] {
        x.store(0, Relaxed);
    }
    READY.reset();
    WREADY.reset();
    for b in BUCKET.iter() {
        b.store(0, Relaxed);
    }
    // The parent's pending retries are its own (their blocks stay allocated here).
    RETRIES.store(0, Relaxed);
    ring_close();
}

// ------------------------------------------------------------------------------ tests only

/// Run this process's requests on the workers, or not (tests: both engines are exercised). Only
/// before the process's first request.
#[cfg(feature = "selftest")]
pub(crate) fn force_workers(on: bool) {
    FORCE_WORKERS.store(usize::from(on), Relaxed);
}

/// Choose this process's engine now, as its first request would, asking `k` (tests: a `Faulty`
/// plan refusing io_uring or a call around it). Only before the process's first request.
#[cfg(feature = "selftest")]
pub(crate) fn choose_engine(k: &liella_kernel::testing::Faulty<CSys>) -> KResult<()> {
    let _g = lock();
    engine_with(k).map(drop)
}

/// The engine this process's requests run on: `ring`, `workers` or `none` (no request yet).
#[cfg(feature = "selftest")]
pub(crate) fn engine_name() -> &'static str {
    match ENGINE.load(Relaxed) {
        ENGINE_RING => "ring",
        ENGINE_WORKERS => "workers",
        _ => "none",
    }
}
