//! Process-wide credential changes (spec §17.2; D-17.1, D-17.7). Linux credentials are per
//! thread, so a change made by one thread is replayed by every other thread the runtime runs:
//! the caller makes the system call, publishes it as a request, and sends the internal signal
//! (D-20.1) to each other thread, whose handler makes the same raw call ([`serve`]) and reports
//! back. A thread that fails where the caller succeeded ends the process (the contract).
//!
//! Threads the host C library created (a test process, D-22.10) are not reached.

use core::sync::atomic::{AtomicI64, AtomicU32, Ordering::{AcqRel, Acquire, Relaxed, Release}};

use liella_kernel::errno::{Errno, KResult};
use liella_kernel::sys::Sys;
use liella_rt::futex::{self, Scope};
use liella_rt::sync::RawMutex;
use liella_rt::thread::Thread;

use crate::sys::CSys;
use crate::{cancel, raw, tb, thread};

/// One broadcast at a time.
static SERIAL: RawMutex = RawMutex::new();
/// The request's generation: a thread whose served generation is older replays it.
static GENERATION: AtomicU32 = AtomicU32::new(0);
/// The request: the system call number and its three arguments.
static REQUEST: [AtomicI64; 4] = [const { AtomicI64::new(0) }; 4];
/// Threads still to serve the request (the caller waits for 0).
static PENDING: AtomicU32 = AtomicU32::new(0);
/// Set when a thread's replay failed.
static FAILED: AtomicU32 = AtomicU32::new(0);

/// The internal signal's handler, for this thread: replay the current request if it has not
/// yet. Async-signal-safe (atomics, one raw system call, one futex wake).
pub(crate) fn serve(d: &Thread) {
    let g = GENERATION.load(Acquire);
    if d.setxid.load(Relaxed) == g {
        return;
    }
    d.setxid.store(g, Relaxed);
    let [nr, a, b, c] = core::array::from_fn(|i| REQUEST.get(i).map_or(0, |x| x.load(Acquire)));
    if raw::syscall(nr, [a, b, c, 0, 0, 0]).is_err() {
        FAILED.store(1, Release);
    }
    if PENDING.fetch_sub(1, AcqRel) == 1 {
        let _ = futex::wake(&CSys, &PENDING, 1, Scope::Private);
    }
}

/// Make system call `nr(a, b, c)` on every thread: the caller's result (an error leaves the
/// other threads unchanged).
pub(crate) fn broadcast(nr: i64, a: i64, b: i64, c: i64) -> KResult<i64> {
    let _g = SERIAL.lock(&CSys);
    let me = raw::syscall(nr, [a, b, c, 0, 0, 0])?;
    // The name-service cache re-keys before its next lookup (D-16.10).
    liella_legacy::nscache::NSCACHE.credentials_changed();
    if thread::current().is_none() {
        return Ok(me);
    }
    for (slot, v) in REQUEST.iter().zip([nr, a, b, c]) {
        slot.store(v, Release);
    }
    FAILED.store(0, Release);
    let g = GENERATION.fetch_add(1, AcqRel).wrapping_add(1);
    let own = tb::tp();
    // Count the other threads first, then signal them: a handler may finish before the loop.
    let mut others = 0u32;
    thread::each(|d| {
        if d.tp.load(Relaxed) != own && !d.has(liella_rt::thread::DESC_EXITED) {
            others = others.saturating_add(1);
        }
    });
    PENDING.store(others, Release);
    if let Some(d) = thread::current() {
        d.setxid.store(g, Relaxed);
    }
    let pid = CSys.getpid();
    thread::each(|d| {
        if d.tp.load(Relaxed) == own || d.has(liella_rt::thread::DESC_EXITED) {
            return;
        }
        // The list lock is held here: the TID comes from the listed block directly.
        let tid = i32::try_from(tb::listed(d.tp.load(Relaxed)).tid.load(Acquire)).unwrap_or(0);
        let sent = if tid == 0 { Err(Errno::ESRCH) } else { CSys.tgkill(pid, tid, cancel::SIG_INTERNAL) };
        if sent.is_err() {
            // Exited meanwhile: nothing to change there.
            let _ = PENDING.fetch_sub(1, AcqRel);
        }
    });
    loop {
        let n = PENDING.load(Acquire);
        if n == 0 || n > u32::MAX / 2 {
            break;
        }
        let _ = futex::wait(&CSys, &PENDING, n, Scope::Private);
    }
    if FAILED.load(Acquire) != 0 {
        crate::process::fortify_fail(b"setxid failed on another thread");
    }
    Ok(me)
}

/// A credential change that returns no value.
pub(crate) fn change(nr: i64, a: i64, b: i64, c: i64) -> KResult<()> {
    broadcast(nr, a, b, c).map(drop)
}

/// `seteuid`/`setegid`: `-1` is `EINVAL`; otherwise `setres*(-1, id, -1)`.
pub(crate) fn set_effective(nr_setres: i64, id: u32) -> KResult<()> {
    if id == u32::MAX {
        return Err(Errno::EINVAL);
    }
    change(nr_setres, -1, i64::from(id), -1)
}

/// `setfsuid`/`setfsgid`: the previous value; cannot report failure (§17.2).
pub(crate) fn set_fs(nr: i64, id: u32) -> u32 {
    broadcast(nr, i64::from(id), 0, 0).map_or_else(|_| u32::try_from(raw::syscall(nr, [-1, 0, 0, 0, 0, 0]).unwrap_or(0)).unwrap_or(0), |v| u32::try_from(v).unwrap_or(0))
}

/// The argument word of an ID (`uid_t`/`gid_t`, `-1` meaning "unchanged" where allowed).
pub(crate) fn id(v: u32) -> i64 {
    if v == u32::MAX { -1 } else { i64::from(v) }
}

/// `setgroups(n, list)`: the list's address goes to every thread (it outlives the broadcast).
pub(crate) fn set_groups(list: &[u32]) -> KResult<()> {
    let n = i64::try_from(list.len()).map_err(|_| Errno::EINVAL)?;
    change(crate::nr::SETGROUPS, n, i64::from_ne_bytes(list.as_ptr().expose_provenance().to_ne_bytes()), 0)
}

/// `setgroups` with the caller's list (`EINVAL` beyond `NGROUPS_MAX`).
pub(crate) fn groups(list: KResult<&[u32]>) -> KResult<()> {
    let list = list?;
    if list.len() > 65536 {
        return Err(Errno::EINVAL);
    }
    set_groups(list)
}

/// The broadcast's lock, for `fork` (D-18.6).
pub(crate) fn lock() -> &'static RawMutex {
    &SERIAL
}
