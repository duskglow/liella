//! Fixed, sound sequences over `raw` for the boundary's own integration tests (decision
//! D-X.9). Compiled only with the `selftest` feature, which the crate enables for its tests
//! through a dev-dependency on itself; products never enable it.
//!
//! * [`in_child`] runs a function in a forked child process and reports how it ended. The
//!   child is single-threaded, so process-wide operations (`setsid`, `unshare(CLONE_NEWUSER)`,
//!   `set_tid_address`, `exit`) are safe to test there without disturbing the test harness.
//! * [`signal_roundtrip`] installs a counting handler with the boundary's restorer, raises
//!   the signal, and restores the previous action: it exercises the signal frame,
//!   `SA_RESTORER` and `rt_sigreturn`.

#![allow(clippy::as_conversions)] // address and register-width reinterpretation (⚑ KERN)

extern crate std;

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering};

use liella_kernel::errno::{Errno, KResult};
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{SigSet, SIGKILL, SIGSTOP};

use crate::raw;
use crate::sys::CSys;

pub mod failinject;

std::thread_local!(static CALLER_BLOCKS: core::cell::Cell<i64> = const { core::cell::Cell::new(0) });

/// Caller-owned blocks (the `caller_alloc` family: `strdup`, `wordexp`, `getifaddrs`, ...) that
/// the calling thread allocated and has not released: its allocations minus its releases since it
/// started, whichever allocator serves the symbols (FREE-POISON-TESTS-6 ruling #296). A leak test
/// compares it before and after; other threads' blocks never move it.
pub fn caller_blocks() -> i64 {
    CALLER_BLOCKS.with(core::cell::Cell::get)
}

/// [`caller_blocks`] moves by `d` (the `caller_alloc` wrappers).
pub(crate) fn count_caller(d: i64) {
    // Not during the thread's teardown (a `free` from a destructor): nothing reads it then.
    let _ = CALLER_BLOCKS.try_with(|c| c.set(c.get().wrapping_add(d)));
}

/// How a child ended (from the `wait4` status word).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ChildEnd {
    /// `exit_group(code)` or `exit(code)`.
    Exited(i32),
    /// Killed by a signal.
    Signaled(i32),
}

/// Decode a `wait4` status word.
pub fn decode_status(status: i32) -> ChildEnd {
    if status & 0x7f == 0 {
        ChildEnd::Exited((status >> 8) & 0xff)
    } else {
        ChildEnd::Signaled(status & 0x7f)
    }
}

/// Exit code of a child whose function panicked.
pub const PANICKED: i32 = 101;

/// In a new child: die with the forking thread (`parent`: its process and thread IDs). The
/// death signal follows the thread that is the child's parent when it ends; if the forking
/// thread ended before the signal was set (the child then belongs to another thread, or to
/// another process), the child exits at once.
fn orphan_guard(parent: (i32, i32)) {
    use liella_kernel::ops::PrctlInt;
    let _ = CSys.prctl_int(PrctlInt::SetPdeathsig, u64::from(SIGKILL.unsigned_abs()), 0);
    if CSys.getppid() != parent.0 || CSys.tgkill(parent.0, parent.1, 0).is_err() {
        CSys.exit_group(PANICKED);
    }
}

/// THE expected-crash helper (CORE-STORM): call it in a child process, immediately before a crash
/// the test causes ON PURPOSE (abort, fortify failure, assertion, SIGSEGV/SIGFPE probe). It makes
/// the process non-dumpable, so the kernel neither writes a core file nor starts the core-pipe
/// helper (apport) for it: with a pipe `core_pattern` the helper starts for every crash even under
/// `RLIMIT_CORE` 0, and a 6-minute test took 37 minutes when many children crashed on purpose.
/// Never call it in a test process that goes on to run other tests (an unexpected crash must still
/// dump), and not before setup a test does in the child: a non-dumpable process cannot write its
/// own `/proc/self/uid_map` or `setgroups`. `cargo xtask crash-audit` checks every expected-crash
/// test reaches it. The C programs use `conformance/c/expect_crash.h`.
pub fn expect_crash() {
    use liella_kernel::ops::PrctlInt;
    let _ = CSys.prctl_int(PrctlInt::SetDumpable, 0, 0);
}

/// This process's dumpable flag (`PR_GET_DUMPABLE`): 0 after [`expect_crash`], 1 normally.
pub fn dumpable() -> KResult<i64> {
    use liella_kernel::ops::PrctlInt;
    CSys.prctl_int(PrctlInt::GetDumpable, 0, 0)
}

/// [`in_child`] for a body that is meant to crash (abort, SIGFPE, ...): the child first calls
/// [`expect_crash`].
pub fn in_child_quiet(body: fn(&CSys) -> i32) -> KResult<ChildEnd> {
    let pid = child_fork()?;
    if pid == 0 {
        expect_crash();
        let code = std::panic::catch_unwind(|| body(&CSys)).unwrap_or(PANICKED);
        CSys.exit_group(code);
    }
    let (_, status) = CSys.wait4(pid, 0, None)?;
    Ok(decode_status(status))
}

/// Run `body` in a forked child; the child exits with its return value (`PANICKED` on a
/// panic). The parent waits and reports how the child ended.
///
/// The child is a copy of this process with only the calling thread; `body` should avoid
/// locks other test threads may have held at the fork (heap allocation is one).
///
/// The fork is raw: the library's after-fork resets do not run, so the child keeps the
/// parent's per-process helper state (`SIGEV_THREAD` sockets and service threads, timers,
/// `aio`, `syslog`, ...) although the threads behind it are gone. A body that uses such state,
/// or tests what `fork` does to it, uses [`in_library_child`] (MQ-NOTIFY-FORK).
pub fn in_child(body: fn(&CSys) -> i32) -> KResult<ChildEnd> {
    let pid = child_fork()?;
    if pid == 0 {
        let code = std::panic::catch_unwind(|| body(&CSys)).unwrap_or(PANICKED);
        CSys.exit_group(code);
    }
    let (_, status) = CSys.wait4(pid, 0, None)?;
    Ok(decode_status(status))
}

/// Fork a child that runs `body` and exits with its value (`PANICKED` on a panic), **without**
/// waiting: its PID, for a caller that waits itself (`WUNTRACED`, job control) and must reap it.
/// The same restrictions on `body` as [`in_child`].
pub fn spawn_child(body: fn(&CSys) -> i32) -> KResult<i32> {
    let pid = child_fork()?;
    if pid == 0 {
        let code = std::panic::catch_unwind(|| body(&CSys)).unwrap_or(PANICKED);
        CSys.exit_group(code);
    }
    Ok(pid)
}

/// Test processes run under the host libc, where a thread made by Liella's `pthread_create`
/// cannot run its start routine and a wait for it never ends. This makes thread creation fail
/// with `EAGAIN` (D-22.10, as in a program under another `ld.so`), so the test sees the
/// expected error and does not hang. Behind the `selftest` feature, which no product enables.
///
/// The flag is process-wide and never cleared, so it may only be set in a process of its own
/// (a forked or re-executed child, a program that runs one test): a test that sets it in a
/// `cargo test` process makes every other test's thread creation fail while they run
/// concurrently (S23-4-C-3-3-LOAD: a SIGEV_THREAD notification was lost). This panics unless the
/// process was marked as such by [`mark_own_process`], which every child helper of this module
/// calls in the child right after the fork, and which a test that re-executes itself calls in the
/// re-executed process: the mistake fails every time, not by chance.
pub fn forbid_threads() {
    assert!(OWN_PROCESS.load(Ordering::Acquire), "forbid_threads: this process is not marked as a child process of its own (a forked helper of this module, or a test that called mark_own_process after re-executing itself); it is process-wide and would fail other tests' thread creation");
    crate::thread::forbid();
}

/// Undo [`forbid_threads`]: thread creation works again. With it a test makes thread creation
/// fail for a while (as a resource limit would) without any load.
pub fn allow_threads() {
    crate::thread::allow();
}

/// While `on`, a creation [`forbid_threads`] refuses waits (after [`refused_threads`] counts it)
/// until `hold_refused_threads(false)`: a test acts while a thread is inside a refused creation.
pub fn hold_refused_threads(on: bool) {
    crate::thread::hold_refusals(on);
}

/// While `on`, the `SIGEV_THREAD` timers' helper waits after it has taken a timer's generation
/// and before it reads the timer's descriptor, until `hold_timer_reads(false)`: a test sets the
/// timer inside that window. The helper holds the timers' lock meanwhile.
pub fn hold_timer_reads(on: bool) {
    crate::timer::hold_reads(on);
}

/// How many reads [`hold_timer_reads`] has held so far in this process, for a test that waits
/// until the helper is inside the window.
pub fn held_timer_reads() -> usize {
    crate::timer::held_reads()
}

/// While `on`, a `timer_settime` of a `SIGEV_THREAD` timer waits after it has counted itself
/// in as a setting in progress and before it sets the timer's descriptor, until
/// `hold_timer_sets(false)`: a test has the helper meet a setting in progress.
pub fn hold_timer_sets(on: bool) {
    crate::timer::hold_sets(on);
}

/// While `on`, a `timer_settime` of a `SIGEV_THREAD` timer waits after it has set the timer's
/// descriptor and before it is done, until `hold_timer_sets_late(false)`: the new setting's
/// expiry is in the descriptor while the setting is in progress (counted in
/// [`held_timer_sets`]).
pub fn hold_timer_sets_late(on: bool) {
    crate::timer::hold_sets_late(on);
}

/// While `on`, a `timer_settime` of a `SIGEV_THREAD` timer waits after it is done with the
/// timer's state (descriptor set, generation moved) and before it returns, until
/// `hold_timer_sets_done(false)`: the helper reads and handles the new setting's expiry while
/// the setter has not returned (counted in [`held_timer_sets`]).
pub fn hold_timer_sets_done(on: bool) {
    crate::timer::hold_sets_done(on);
}

/// How many `timer_settime` calls of `SIGEV_THREAD` timers counted themselves in while the
/// helper was inside a read of the timer's descriptor (and so waited for it), so far in this
/// process.
pub fn timer_sets_met_reads() -> usize {
    crate::timer::sets_met_reads()
}

/// Waits until the `SIGEV_THREAD` timers' helper is done with what it holds the timers' lock
/// for (a read and its notification or retry), by taking and releasing that lock.
pub fn timer_helper_settled() {
    crate::timer::with_lock(|| ());
}

/// The `SIGEV_THREAD` timers' last events, oldest first, as `code << 32 | value` (setter:
/// 1 counted in, 2 descriptor set, 3 generation moved, 4 took a retry; helper: 10 read put
/// off, 11 generation read, 12 count read, 13 nothing due, 14 thread started, 15 refused and
/// dropped, 16 refused and retried (ms), 20 event (timer found), 21 retry due; 30 notification
/// thread ran): a test prints them when a notification is lost.
pub fn timer_trace() -> [usize; 64] {
    let (n, ring) = crate::timer::trace_ring();
    core::array::from_fn(|i| ring.get(n.wrapping_add(i) & 63).copied().unwrap_or(0))
}

/// Reads of a timer's descriptor the helper made anyway, with a setting still in progress after
/// it had put the read off as long as it does, so far in this process.
pub fn forced_timer_reads() -> usize {
    crate::timer::forced_reads()
}

/// (settings [`hold_timer_sets`] has held, times the helper put a read off for a setting in
/// progress) so far in this process.
pub fn held_timer_sets() -> (usize, usize) {
    crate::timer::held_sets()
}

/// How many thread creations [`forbid_threads`] has refused so far in this process, for a test
/// that waits until a refusal has happened.
pub fn refused_threads() -> usize {
    crate::thread::refused()
}

/// The failure episodes of `SIGEV_THREAD` timers' notification threads so far in this process:
/// (begun, recovered). An episode begins when a timer's notification thread is first refused
/// and ends when one starts, however many retries come between (D-23.3, owner ruling #269).
pub fn timer_thread_episodes() -> (usize, usize) {
    crate::timer::episodes(crate::timer::Notifier::Timer)
}

/// How many `SIGEV_THREAD` timer notifications that waited for their caller-supplied stack could
/// then not be started and were handed back to the timers (D-23.3), so far in this process.
pub fn timer_notifications_abandoned() -> usize {
    crate::timer::abandoned_count()
}

/// The `SIGEV_THREAD` timers' helper's wake-up `eventfd` (D-23.3), -1 before the first timer
/// whose attributes name a stack: a test reads its count (`/proc/self/fdinfo`) to see the
/// helper drain it.
pub fn timer_wake_fd() -> i32 {
    crate::timer::wake_fd()
}

/// How many `mq_notify` `SIGEV_THREAD` notifications that waited for their caller-supplied stack
/// could then not be started and were retried (D-25.5, owner ruling #680), so far in this process.
pub fn mq_notifications_abandoned() -> usize {
    crate::mqueue::abandoned_count()
}

/// How many AIO `SIGEV_THREAD` notifications that waited for their caller-supplied stack could
/// then not be started and were retried (D-11.11, owner ruling #680), so far in this process.
pub fn aio_notifications_abandoned() -> usize {
    crate::aio::abandoned_count()
}

/// The failure episodes of `mq_notify` `SIGEV_THREAD` notification threads so far in this
/// process: (begun, recovered) (D-25.5, owner ruling #680).
pub fn mq_thread_episodes() -> (usize, usize) {
    crate::timer::episodes(crate::timer::Notifier::Mq)
}

/// The failure episodes of AIO `SIGEV_THREAD` notification threads so far in this process:
/// (begun, recovered) (D-11.11, owner ruling #680).
pub fn aio_thread_episodes() -> (usize, usize) {
    crate::timer::episodes(crate::timer::Notifier::Aio)
}

/// How many AIO request records are free for reuse now (a released notification block is one).
pub fn aio_free_records() -> usize {
    crate::aio::free_records()
}

/// How many notifications wait for a caller-supplied stack now (`boundary::stacks`).
pub fn notifications_queued_for_stacks() -> usize {
    crate::stacks::queued()
}

/// Turn on exactly the diagnostic topics a `LIELLA_DIAG` value names (D-2.9), as the lazy read
/// of the variable does at the first diagnostic (the test sets the answer instead).
pub fn diag_set(value: &[u8]) {
    crate::diag::set(value);
}

/// The topic bits a `LIELLA_DIAG` value names (D-2.9): `timers` is 1.
pub fn diag_topics(value: &[u8]) -> u32 {
    crate::diag::topics(value)
}

/// Set when this process is a child made for one test: [`forbid_threads`] requires it. A plain
/// static, set right after the fork (no allocation, no `setenv`, which could deadlock in a
/// child of a multithreaded parent).
static OWN_PROCESS: AtomicBool = AtomicBool::new(false);

/// Mark this process as one that serves a single test alone, so [`forbid_threads`] may be
/// called in it. The helpers of this module that fork call it in the child; a test that
/// re-executes the test binary for one test (`--exact`, `--test-threads=1`) calls it in the
/// re-executed process, never in a shared `cargo test` process.
pub fn mark_own_process() {
    OWN_PROCESS.store(true, Ordering::Release);
}

/// `raw::fork` for this module's helpers: the child is marked with [`mark_own_process`] at once.
fn child_fork() -> KResult<i32> {
    let pid = raw::fork()?;
    if pid == 0 {
        mark_own_process();
    }
    Ok(pid)
}

/// Whether [`forbid_threads`] has been called in this process (a probe that must run with threads
/// forbidden checks it, so dropping the call fails the probe).
pub fn threads_forbidden() -> bool {
    crate::thread::forbidden()
}

/// A child that ends with the raw thread `exit` (legitimate there: the child has one
/// thread). Tests `exit` as distinct from `exit_group`.
pub fn in_child_thread_exit(code: i32) -> KResult<ChildEnd> {
    let pid = child_fork()?;
    if pid == 0 {
        raw::exit_thread(code);
    }
    let (_, status) = CSys.wait4(pid, 0, None)?;
    Ok(decode_status(status))
}

/// Fork a child that exits with `code` at once, **without** waiting for it. Returns the PID,
/// for tests of `wait4`, `waitid` and pidfds; the caller must reap it.
pub fn spawn_exiting(code: i32) -> KResult<i32> {
    let pid = child_fork()?;
    if pid == 0 {
        CSys.exit_group(code);
    }
    Ok(pid)
}

/// Fork a child that blocks in `pause` until a signal ends it, without waiting. The caller
/// must kill and reap it; if it does not, the child still ends (see the body).
pub fn spawn_pausing() -> KResult<i32> {
    let parent = (CSys.getpid(), CSys.gettid());
    // The forking thread waits until the child has armed its death signal. Without this the
    // thread could end between the two: the kernel then hands the child to another thread of
    // the process before the signal is set while the ending thread is still visible to the
    // guard's tgkill, and the child pauses until the 600 s alarm (the likely cause of the one
    // 600 s hang of selftest_children_end_with_their_thread in a gate; FLAKES-6, by analysis).
    let [armed_r, armed_w] = CSys.pipe2(liella_kernel::uapi::O_CLOEXEC)?;
    let pid = match child_fork() {
        Ok(pid) => pid,
        Err(e) => {
            let _ = CSys.close(armed_r);
            let _ = CSys.close(armed_w);
            return Err(e);
        }
    };
    if pid == 0 {
        // It ends on its own (it would otherwise pause forever, an orphan holding whatever the
        // test held, if a test failed before killing it: seen on 5.10, where process_mrelease
        // is ENOSYS): with the forking thread, or after 10 minutes at the latest.
        orphan_guard(parent);
        let _ = CSys.write(armed_w, &[1]);
        let _ = CSys.alarm(600);
        loop {
            let _ = CSys.pause();
        }
    }
    let _ = CSys.close(armed_w);
    // One byte once armed, or end of file if the child ended first (the guard's own exit).
    let mut armed = [0u8; 1];
    while CSys.read(armed_r, &mut armed) == Err(Errno::EINTR) {}
    let _ = CSys.close(armed_r);
    Ok(pid)
}

/// `clone3` with `CLONE_PIDFD`: a child that exits with `code` at once. Returns `(pid,
/// pidfd)`; the caller must reap the child and close the pidfd.
pub fn spawn_exiting_pidfd(code: i32) -> KResult<(i32, i32)> {
    let (pid, pidfd) = raw::fork_pidfd()?;
    if pid == 0 {
        CSys.exit_group(code);
    }
    Ok((pid, pidfd))
}

static HANDLED: AtomicU32 = AtomicU32::new(0);
static LAST_SIG: AtomicU32 = AtomicU32::new(0);

extern "C" fn count_signal(sig: i32) {
    HANDLED.fetch_add(1, Ordering::SeqCst);
    LAST_SIG.store(u32::try_from(sig).unwrap_or(0), Ordering::SeqCst);
}

/// Install a counting handler for `sig`, send `sig` to the calling thread, restore the
/// previous action, and return the handler's `(invocation count, signal number seen)`. A
/// return at all proves the restorer and `rt_sigreturn` resumed the interrupted code.
///
/// `SIGKILL` and `SIGSTOP` cannot be caught; they are refused with `EINVAL` as the kernel
/// would. Callers serialise uses (the counters are process-wide).
pub fn signal_roundtrip(sig: i32) -> KResult<(u32, i32)> {
    if sig == SIGKILL || sig == SIGSTOP {
        return Err(Errno::EINVAL);
    }
    HANDLED.store(0, Ordering::SeqCst);
    LAST_SIG.store(0, Ordering::SeqCst);
    let old = raw::install_handler(sig, count_signal, 0, SigSet::empty())?;
    let sent = CSys.tgkill(CSys.getpid(), CSys.gettid(), sig);
    let restored = raw::restore_action(sig, &old);
    sent?;
    restored?;
    Ok((HANDLED.load(Ordering::SeqCst), i32::try_from(LAST_SIG.load(Ordering::SeqCst)).unwrap_or(0)))
}

/// Fork; the child redirects its standard output to `/dev/null` and `execve`s `path`; the
/// parent waits. A failed `execve` ends the child with 127.
pub fn exec_child(path: liella_kernel::KStr<'_>, argv: &liella_kernel::mem::KPtrArray<'_>, envp: &liella_kernel::mem::KPtrArray<'_>) -> KResult<ChildEnd> {
    let pid = child_fork()?;
    if pid == 0 {
        if let Ok(null) = liella_kernel::KStr::from_nul_terminated(b"/dev/null\0") {
            if let Ok(fd) = CSys.openat(liella_kernel::Fd::CWD, null, liella_kernel::uapi::O_WRONLY, 0) {
                let _ = CSys.dup2(fd, liella_kernel::Fd::STDOUT);
            }
        }
        let _ = CSys.execve(path, argv, envp);
        CSys.exit_group(127);
    }
    let (_, status) = CSys.wait4(pid, 0, None)?;
    Ok(decode_status(status))
}

/// Fork a child that runs `pre`, then waits until the parent has run `between(child_pid)`,
/// then runs `post` and exits with its value (or the first non-zero `pre` value). For steps
/// only another process can take, such as writing a user namespace's ID maps with the setuid
/// helpers.
///
/// The child cannot outlive the call (SETID-TEST-ORPHANS: orphans blocked in `pipe_read` for
/// 29 hours after their test process was killed). It closes the pipe ends it does not use, so
/// the go-ahead pipe reaches end of file once every other writer is gone. It dies with the
/// forking thread (`PR_SET_PDEATHSIG`, armed after `pre` because a credential change clears
/// it), which also covers a write end some other forked process still holds. It waits at most
/// [`SYNCED_GO_TIMEOUT_MS`] for the go-ahead. A panic in `between` closes the go-ahead pipe,
/// reaps the child (status 125) and then resumes.
pub fn in_child_synced(pre: fn(&CSys) -> i32, post: fn(&CSys) -> i32, between: impl FnOnce(i32)) -> KResult<ChildEnd> {
    use liella_kernel::uapi::O_CLOEXEC;
    let parent = (CSys.getpid(), CSys.gettid());
    let [to_parent_r, to_parent_w] = CSys.pipe2(O_CLOEXEC)?;
    let close_all = |fds: &[liella_kernel::Fd]| fds.iter().for_each(|&fd| {
        let _ = CSys.close(fd);
    });
    let [to_child_r, to_child_w] = CSys.pipe2(O_CLOEXEC).inspect_err(|_| close_all(&[to_parent_r, to_parent_w]))?;
    let pid = child_fork().inspect_err(|_| close_all(&[to_parent_r, to_parent_w, to_child_r, to_child_w]))?;
    if pid == 0 {
        close_all(&[to_parent_r, to_child_w]);
        let code = std::panic::catch_unwind(|| {
            let first = pre(&CSys);
            if first != 0 {
                let _ = CSys.write(to_parent_w, &[0]);
                return first;
            }
            orphan_guard(parent);
            let _ = CSys.write(to_parent_w, &[1]);
            match wait_go_ahead(to_child_r) {
                0 => post(&CSys),
                code => code,
            }
        })
        .unwrap_or(PANICKED);
        CSys.exit_group(code);
    }
    close_all(&[to_parent_w, to_child_r]);
    let mut ready = [0u8; 1];
    let mut failed = None;
    if CSys.read(to_parent_r, &mut ready) == Ok(1) && ready[0] == 1 {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| between(pid))) {
            Ok(()) => {
                let _ = CSys.write(to_child_w, &[1]);
            }
            Err(panic) => failed = Some(panic),
        }
    }
    close_all(&[to_parent_r, to_child_w]);
    let waited = CSys.wait4(pid, 0, None);
    if let Some(panic) = failed {
        std::panic::resume_unwind(panic);
    }
    Ok(decode_status(waited?.1))
}

/// The longest the child of [`in_child_synced`] waits for its go-ahead (it then exits with
/// 124): a bound for the case neither end of file nor the death signal ends the wait.
pub const SYNCED_GO_TIMEOUT_MS: i64 = 120_000;

/// In the child of [`in_child_synced`]: 0 once the go-ahead byte arrived, 125 at end of file
/// or an error, 124 after [`SYNCED_GO_TIMEOUT_MS`].
fn wait_go_ahead(fd: liella_kernel::Fd) -> i32 {
    use liella_kernel::uapi::{PollFd, Timespec, POLLIN};
    let mut fds = [PollFd { fd: fd.raw(), events: POLLIN, revents: 0 }];
    let mut left = Timespec { sec: SYNCED_GO_TIMEOUT_MS / 1000, nsec: (SYNCED_GO_TIMEOUT_MS % 1000) * 1_000_000 };
    loop {
        // ppoll writes the time left back, so a retry after EINTR keeps the overall bound.
        match CSys.ppoll(&mut fds, Some(&mut left), None) {
            Ok(0) => return 124,
            Ok(_) => break,
            Err(Errno::EINTR) => {}
            Err(_) => return 125,
        }
    }
    let mut go = [0u8; 1];
    if CSys.read(fd, &mut go) == Ok(1) { 0 } else { 125 }
}

/// Run `body` with a counting handler installed for `sig` (with the boundary's restorer), then
/// restore the previous action. Returns `body`'s value and the number of deliveries.
pub fn with_counting_handler<R>(sig: i32, body: impl FnOnce() -> R) -> KResult<(R, u32)> {
    with_handler_flags(sig, 0, body)
}

/// [`with_counting_handler`] with `sa_flags` (for example `SA_RESTART`).
pub fn with_handler_flags<R>(sig: i32, flags: u64, body: impl FnOnce() -> R) -> KResult<(R, u32)> {
    if sig == SIGKILL || sig == SIGSTOP {
        return Err(Errno::EINVAL);
    }
    HANDLED.store(0, Ordering::SeqCst);
    let old = raw::install_handler(sig, count_signal, flags, SigSet::empty())?;
    let r = body();
    raw::restore_action(sig, &old)?;
    Ok((r, HANDLED.load(Ordering::SeqCst)))
}

static ALT_LO: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static ALT_HI: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
static ON_ALT: AtomicU32 = AtomicU32::new(0);

extern "C" fn note_stack(_sig: i32) {
    let local = 0u8;
    let sp = core::ptr::from_ref(&local).addr();
    let inside = sp >= ALT_LO.load(Ordering::SeqCst) && sp < ALT_HI.load(Ordering::SeqCst);
    ON_ALT.store(u32::from(inside), Ordering::SeqCst);
}

/// Install a fresh alternate signal stack, deliver `sig` to a handler registered with
/// `SA_ONSTACK`, and report whether the handler ran on that stack. The stack is disabled and
/// unmapped afterwards.
pub fn altstack_roundtrip(sig: i32) -> KResult<bool> {
    use liella_kernel::uapi::{MAP_PRIVATE, PROT_READ, PROT_WRITE, SA_ONSTACK};
    const SIZE: usize = 64 * 1024;
    let region = CSys.mmap(0, SIZE, PROT_READ | PROT_WRITE, MAP_PRIVATE, None, 0)?;
    ALT_LO.store(region.addr(), Ordering::SeqCst);
    ALT_HI.store(region.addr().saturating_add(SIZE), Ordering::SeqCst);
    ON_ALT.store(0, Ordering::SeqCst);
    raw::sigaltstack_set(region.addr(), SIZE)?;
    let old = raw::install_handler(sig, note_stack, SA_ONSTACK, SigSet::empty())?;
    let sent = CSys.tgkill(CSys.getpid(), CSys.gettid(), sig);
    raw::restore_action(sig, &old)?;
    CSys.sigaltstack_disable()?;
    CSys.munmap(region)?;
    sent?;
    Ok(ON_ALT.load(Ordering::SeqCst) == 1)
}

/// Robust-futex protocol (spec Appendix A.2 "Shared memory protocols"): a child registers a
/// robust list whose one entry names a futex word in shared memory that it owns, then exits
/// while holding it. Returns the word as the kernel left it: `FUTEX_OWNER_DIED` set, the TID
/// cleared.
pub fn robust_list_owner_died() -> KResult<u32> {
    use liella_kernel::uapi::{MAP_SHARED, PROT_READ, PROT_WRITE, RobustListHead};
    let region = CSys.mmap(0, 4096, PROT_READ | PROT_WRITE, MAP_SHARED, None, 0)?;
    let words = CSys.region_words(&region).ok_or(Errno::EINVAL)?;
    let pid = child_fork()?;
    if pid == 0 {
        static HEAD: RobustListHead = RobustListHead {
            list: core::sync::atomic::AtomicUsize::new(0),
            futex_offset: -8,
            list_op_pending: core::sync::atomic::AtomicUsize::new(0),
        };
        // Word 0: the futex. Words 2..4: the list entry (its `next` points back at the head).
        let (Some(futex), Some(lo), Some(hi)) = (words.first(), words.get(2), words.get(3)) else {
            CSys.exit_group(2)
        };
        let head_addr = core::ptr::from_ref(&HEAD).expose_provenance();
        lo.store(head_addr as u32, Ordering::SeqCst);
        hi.store((head_addr >> 32) as u32, Ordering::SeqCst);
        HEAD.list.store(core::ptr::from_ref(lo).expose_provenance(), Ordering::SeqCst);
        futex.store(CSys.gettid() as u32, Ordering::SeqCst);
        let code = i32::from(raw::set_robust_list(head_addr, core::mem::size_of::<RobustListHead>()).is_err());
        CSys.exit_group(code);
    }
    let (_, status) = CSys.wait4(pid, 0, None)?;
    let word = words.first().map_or(0, |w| w.load(Ordering::SeqCst));
    CSys.munmap(region)?;
    match decode_status(status) {
        ChildEnd::Exited(0) => Ok(word),
        _ => Err(Errno::EIO),
    }
}

/// The positive control for [`strcmp_clean_upper`]: the same clean entry around a call that sets
/// ymm15 to all ones; must return nonzero (AVX2 CPUs only; machine code in csys/).
pub fn dirty_upper_probe() -> u64 {
    let mut r = -1;
    let n = core::ptr::null();
    crate::ffi::liella_machine_clean_upper_call(crate::ffi::liella_machine_ymm15_dirty_cmp, n, n, &mut r)
}

/// `strcmp` of the NUL-terminated byte strings `a` and `b`, entered with clean upper halves
/// (`vzeroupper` just before, no code in between); returns the sign and the OR of the upper
/// halves of ymm0..ymm15 afterwards: 0 when the kernel left no upper state behind (D-4.38).
/// AVX2 CPUs only (either scan path).
pub fn strcmp_clean_upper(a: &[u8], b: &[u8]) -> (i32, u64) {
    assert!(a.contains(&0) && b.contains(&0), "both strings need a terminator");
    let mut r = 0;
    let f = crate::ffi::liella_machine_strcmp;
    let upper = crate::ffi::liella_machine_clean_upper_call(f, a.as_ptr(), b.as_ptr(), &mut r);
    (r, upper)
}

/// `liella_machine_strdiff` (D-4.37) on the strings at `a` and `b`: the first index at which
/// they differ or `a`'s byte is the terminator. Test-only contract: both are readable
/// NUL-terminated strings (the guard-page tests place them before an unmapped page).
pub fn strdiff(a: *const u8, b: *const u8) -> usize {
    crate::ffi::liella_machine_strdiff(a, b).index
}

/// `liella_machine_strdiff`'s second word (STRCOLL-4, D-4.37) as a flag: whether a byte of the
/// string at `a` up to the index [`strdiff`] returns has bit 7 set. Same contract as [`strdiff`].
pub fn strdiff_high(a: *const u8, b: *const u8) -> bool {
    crate::ffi::liella_machine_strdiff(a, b).high != 0
}

/// Force the scanners' path (D-4.14): `true` for AVX2, `false` for SSE2. Returns `false`
/// (and changes nothing) if AVX2 is asked for on a CPU or OS without it.
pub fn set_scan_avx2(avx2: bool) -> bool {
    if avx2 && !crate::cpu::avx2() {
        return false;
    }
    crate::ffi::liella_machine_set_scan_level(u32::from(avx2));
    crate::cpu::force_casecmp_kernel(Some(if avx2 { crate::cpu::CASECMP_AVX2 } else { crate::cpu::CASECMP_SSE2 }));
    true
}

/// Force the `strcasecmp`/`strncasecmp` kernel (CASECMP): 0 = none (the measured path), 1 = SSE2,
/// 2 = AVX2. Returns `false` (and changes nothing) for another value or AVX2 on a CPU or OS
/// without it. [`set_scan_avx2`] also sets it (AVX2 or SSE2).
pub fn set_casecmp_kernel(kernel: u32) -> bool {
    let k = match kernel {
        0 => crate::cpu::CASECMP_MEASURED,
        1 => crate::cpu::CASECMP_SSE2,
        2 if crate::cpu::avx2() => crate::cpu::CASECMP_AVX2,
        _ => return false,
    };
    crate::cpu::force_casecmp_kernel(Some(k));
    true
}

/// Force the hot copy and fill kernels' build (D-4.25): `true` for AVX2, `false` for the SSE2
/// baseline. Returns `false` (and changes nothing) if AVX2 is asked for on a CPU or OS without
/// it. Test-only: the startup decision (`cpu::init_with`) is the only other writer.
pub fn set_kernel_avx2(avx2: bool) -> bool {
    if avx2 && !crate::cpu::avx2() {
        return false;
    }
    crate::dispatch::select(avx2);
    true
}

/// Force NULL folding in the copy and fill entries (D-4.22) on or off: `true` takes sizes 1-128
/// with no NULL test (a NULL faults on page zero), `false` keeps the explicit S6 checks. Returns
/// `false` (and changes nothing) when folding is asked for but the startup guard would refuse it
/// (`vm.mmap_min_addr` below 64 KiB or unreadable). Test-only besides the startup decision.
pub fn set_null_fold(fold: bool) -> bool {
    if fold && !crate::dispatch::page_zero_unmappable(&CSys) {
        return false;
    }
    crate::dispatch::select_null_fold(fold);
    true
}

/// Forget the NULL-folding decision, as at process start (STARTUP-1): folding off, undecided; the
/// first small copy or fill decides it. Test-only.
pub fn unresolve_null_fold() {
    crate::dispatch::unresolve_null_fold();
}

/// Whether the NULL-folding decision (the `vm.mmap_min_addr` read) has been taken.
pub fn null_fold_resolved() -> bool {
    crate::dispatch::fold_resolved()
}

/// Whether NULL folding is on in the copy and fill entries.
pub fn null_fold() -> bool {
    crate::dispatch::null_folded()
}

/// The startup guard's reading of a `vm.mmap_min_addr` text: whether it allows NULL folding.
pub fn null_fold_allowed_by(text: &[u8]) -> bool {
    crate::dispatch::min_addr_allows_fold(text)
}

/// Which build of the hot copy and fill kernels runs: `true` for AVX2.
pub fn kernel_avx2() -> bool {
    crate::dispatch::avx2()
}

/// The AVX2 copy into `dst` from `src` (the size range and placement checks are bypassed):
/// the bytes copied, or `None` (nothing copied) on a CPU or OS without AVX2 or below 64 bytes.
pub fn copy_avx2(dst: &mut [u8], src: &[u8]) -> Option<usize> {
    let n = dst.len().min(src.len());
    if n < 64 || !crate::cpu::avx2() {
        return None;
    }
    crate::ffi::liella_machine_copy_avx2(dst.as_mut_ptr(), src.as_ptr(), n);
    Some(n)
}

/// The AVX2 copy over regions that may overlap inside `buf` (MEM-BULK): `n` bytes from offset
/// `src` to offset `dst`, the direction chosen by the routine. The bytes moved, or `None` (nothing
/// moved) on a CPU or OS without AVX2, below 64 bytes, or when a range leaves `buf`.
pub fn move_avx2(buf: &mut [u8], src: usize, dst: usize, n: usize) -> Option<usize> {
    let end = src.max(dst).checked_add(n)?;
    if n < 64 || end > buf.len() || !crate::cpu::avx2() {
        return None;
    }
    let p = buf.as_mut_ptr();
    let _ = crate::ffi::liella_machine_move_avx2(p.wrapping_add(dst), p.wrapping_add(src).cast_const(), n);
    Some(n)
}

/// The SSE2 move over regions that may overlap inside `buf` (MEM-FALLBACK): `n` bytes from
/// offset `src` to offset `dst`, the direction chosen by the routine. The bytes moved, or `None`
/// (nothing moved) below 64 bytes or when a range leaves `buf`. Runs on every x86-64 CPU.
pub fn move_sse2(buf: &mut [u8], src: usize, dst: usize, n: usize) -> Option<usize> {
    let end = src.max(dst).checked_add(n)?;
    if n < 64 || end > buf.len() {
        return None;
    }
    let p = buf.as_mut_ptr();
    let _ = crate::ffi::liella_machine_move_sse2(p.wrapping_add(dst), p.wrapping_add(src).cast_const(), n);
    Some(n)
}

/// The AVX2 fill of `buf` with `c` (MEM-FALLBACK; the size and ERMS checks are bypassed): the
/// bytes set, or `None` (nothing written) on a CPU or OS without AVX2 or below 128 bytes.
pub fn fill_avx2(buf: &mut [u8], c: u8) -> Option<usize> {
    if buf.len() < 128 || !crate::cpu::avx2() {
        return None;
    }
    let _ = crate::ffi::liella_machine_fill_avx2(buf.as_mut_ptr(), i32::from(c), buf.len());
    Some(buf.len())
}

fn bulk_name(k: crate::hot::Bulk) -> &'static str {
    match k {
        crate::hot::Bulk::Avx2 => "avx2",
        crate::hot::Bulk::Sse2 => "sse2",
        crate::hot::Bulk::Erms => "erms",
        crate::hot::Bulk::ErmsA64 => "erms-a64",
        crate::hot::Bulk::Checked => "checked",
        crate::hot::Bulk::Fallback => "fallback",
    }
}

/// The kernel the `memmove`/`memcpy` entry picks for `n` bytes to address `dst` from `src` under
/// the current flags (`set_kernel_avx2`, `set_erms`): "avx2", "sse2", "erms" or "checked".
pub fn bulk_move_kernel(dst: usize, src: usize, n: usize) -> &'static str {
    crate::cpu::ready();
    match crate::hot::bulk_move(n, crate::dispatch::avx2()) {
        crate::hot::Bulk::Fallback => bulk_name(crate::hot::fallback_move(dst, src, n)),
        crate::hot::Bulk::Avx2 if crate::hot::avx2_erms(dst, src, n) => "erms",
        k => bulk_name(k),
    }
}

/// The kernel the `memset` entry picks for `n` bytes under the current flags, as
/// [`bulk_move_kernel`].
pub fn bulk_fill_kernel(n: usize) -> &'static str {
    crate::cpu::ready();
    bulk_name(crate::hot::bulk_fill(n, crate::dispatch::avx2(), crate::cpu::erms(), crate::cpu::fill_erms_min()))
}

/// The library's internal copy (`liella_internal_memcpy`/`_memmove`, D-X.15) through an opaque
/// pointer: what the products' own `memcpy`/`memmove` calls reach (COPY-INTERNAL).
pub fn internal_memmove(d: *mut V, s: *const V, n: usize) -> *mut V {
    core::hint::black_box(crate::conv::liella_internal_memmove as extern "C" fn(*mut V, *const V, usize) -> *mut V)(d, s, n)
}

type V = liella_abi::ctypes::c_void;

/// Force the ERMS flag (`rep movsb`/`rep stosb` paths) on or off whatever the CPU reports; they
/// are correct everywhere, ERMS only makes them fast. `crate::cpu::init` restores the decision.
pub fn set_erms(on: bool) {
    crate::cpu::force_erms(on);
}

/// The block loops in ascending (`back == false`) or descending address order: the bytes
/// copied.
pub fn copy_loops(dst: &mut [u8], src: &[u8], back: bool) -> usize {
    if back { liella::bytes::copy_back(dst, src) } else { liella::bytes::copy(dst, src) }
}

/// `rep movsb` into `dst` from `src`, whatever the size (the threshold and the CPU check are
/// bypassed; ERMS only affects speed). Returns the bytes copied.
pub fn copy_erms(dst: &mut [u8], src: &[u8]) -> usize {
    let n = dst.len().min(src.len());
    crate::ffi::liella_machine_copy_erms(dst.as_mut_ptr(), src.as_ptr(), n);
    n
}

/// `rep stosb` over `dst`, whatever the size.
pub fn fill_erms(dst: &mut [u8], v: u8) {
    crate::ffi::liella_machine_fill_erms(dst.as_mut_ptr(), i32::from(v), dst.len());
}

/// The NUMA node holding the page at `addr` (`get_mempolicy`), through `raw`; `ENOSYS`
/// without `CONFIG_NUMA`. Faults the page in.
pub fn node_of(addr: usize) -> KResult<u32> {
    raw::node_of(addr)
}

/// The memory policy at `addr` (`get_mempolicy(MPOL_F_ADDR)`): `(mode, first node-mask word)`;
/// `ENOSYS` without `CONFIG_NUMA`. Does not fault the page in.
pub fn policy_of(addr: usize) -> KResult<(i32, u64)> {
    raw::policy_of(addr)
}

/// The current program break (`brk(0)`), through `raw`.
pub fn brk_query() -> usize {
    raw::brk(0)
}

/// Ask for a break the kernel must refuse (an address it cannot grant): the kernel answers
/// with the unchanged break (⚑ KERN). Only addresses that cannot succeed are accepted here, so
/// no memory Rust references is ever released.
pub fn brk_try(addr: usize) -> usize {
    if addr < (1usize << 47) {
        return raw::brk(0);
    }
    raw::brk(addr)
}

/// `set_robust_list` with a wrong structure length: the kernel refuses it (`EINVAL`).
pub fn set_robust_list_bad_len() -> KResult<()> {
    static HEAD: liella_kernel::uapi::RobustListHead = liella_kernel::uapi::RobustListHead {
        list: core::sync::atomic::AtomicUsize::new(0),
        futex_offset: 0,
        list_op_pending: core::sync::atomic::AtomicUsize::new(0),
    };
    raw::set_robust_list(core::ptr::from_ref(&HEAD).expose_provenance(), 7)
}

/// `process_vm_writev` into a forked child's copy of a static word; the child reports through
/// its exit status whether it saw the new value. Returns that verdict.
pub fn vm_writev_roundtrip() -> KResult<bool> {
    use liella_kernel::mem::{IoVec, RemoteIoVec};
    static WORD: AtomicU32 = AtomicU32::new(0);
    let [r, w] = CSys.pipe2(liella_kernel::uapi::O_CLOEXEC)?;
    let pid = child_fork()?;
    if pid == 0 {
        let mut go = [0u8; 1];
        let _ = CSys.read(r, &mut go);
        CSys.exit_group(i32::from(WORD.load(Ordering::SeqCst) != 0x1234_5678));
    }
    let value = 0x1234_5678u32.to_ne_bytes();
    let remote = [RemoteIoVec { base: core::ptr::from_ref(&WORD).addr(), len: 4 }];
    let wrote = raw::process_vm_writev(pid, &[IoVec::new(&value)], &remote, 0);
    let _ = CSys.write(w, &[1]);
    let (_, status) = CSys.wait4(pid, 0, None)?;
    let _ = CSys.close(r);
    let _ = CSys.close(w);
    Ok(wrote? == 4 && decode_status(status) == ChildEnd::Exited(0))
}

/// Fork a child that waits (non-private futex, so across processes) on `word` while it is 0,
/// up to five seconds, and exits 0 if it then sees 1. The caller stores 1, wakes it, and reaps.
pub fn spawn_waiter_on_shared_word(word: &AtomicU32) -> KResult<i32> {
    use liella_kernel::sys::FutexArg;
    use liella_kernel::uapi::{Timespec, FUTEX_WAIT};
    let pid = child_fork()?;
    if pid == 0 {
        let timeout = Timespec { sec: 5, nsec: 0 };
        while word.load(Ordering::SeqCst) == 0 {
            if CSys.futex(word, FUTEX_WAIT, 0, FutexArg::Timeout(&timeout), None, 0) == Err(Errno::ETIMEDOUT) {
                break;
            }
        }
        CSys.exit_group(i32::from(word.load(Ordering::SeqCst) != 1));
    }
    Ok(pid)
}

/// Fork a child that sleeps `ms` milliseconds and then exits with `code`, without waiting for
/// it. The caller must reap it.
pub fn spawn_exiting_after(ms: i64, code: i32) -> KResult<i32> {
    use liella_kernel::uapi::{Timespec, CLOCK_MONOTONIC};
    let pid = child_fork()?;
    if pid == 0 {
        let t = Timespec { sec: ms / 1000, nsec: (ms % 1000).saturating_mul(1_000_000) };
        let _ = CSys.clock_nanosleep(CLOCK_MONOTONIC, 0, &t, None);
        CSys.exit_group(code);
    }
    Ok(pid)
}

/// Runs `body` while every lock of the library is held: the heap's, the environment's, the
/// exit registries', the global generator's, the `SIGEV_THREAD` timers'. Phase 5's exit criterion: the async-signal-safe
/// functions, called from a handler interrupting this, must take none of them and allocate
/// nothing (an attempt blocks for good). Contract: `body` itself takes none of them.
pub fn with_all_locks<R>(body: impl FnOnce() -> R) -> R {
    crate::heap::HEAP.lock_all(&CSys);
    crate::process::hold_env_lock(true);
    crate::process::EXIT_HANDLERS.hold(&CSys);
    crate::process::QUICK_HANDLERS.hold(&CSys);
    let prng = liella_legacy::prng_global::hold();
    let r = crate::timer::with_lock(body);
    drop(prng);
    crate::process::QUICK_HANDLERS.release(&CSys);
    crate::process::EXIT_HANDLERS.release(&CSys);
    crate::process::hold_env_lock(false);
    crate::heap::HEAP.unlock_all(&CSys);
    r
}

/// Park the calling thread's heap cache, as a Liella thread's exit does (D-3.11): for test
/// threads the host C library created, which Liella never sees exit (D-22.10).
pub fn park_heap_cache() {
    crate::heap::HEAP.park(&CSys, &crate::heap::KernelPages);
}

/// The identifier of the calling thread's heap cache (0 before its first allocation).
pub fn heap_cache_id() -> u64 {
    crate::heap::HEAP.own_cache_id(&crate::heap::KernelPages)
}

/// Write the identifiers of the heap caches parked right now into `out`; returns how many
/// there are (more than `out` holds when it is too short).
pub fn parked_heap_cache_ids(out: &mut [u64]) -> usize {
    crate::heap::HEAP.parked_ids(&crate::heap::KernelPages, out)
}

/// `fork` (the library's, with its lock handling, D-18.6) while another thread holds `popen`'s
/// and `system`'s locks: the holder releases them once the fork waits for them (or after 3 s).
/// The child gets 10 s (an alarm) to run `child`, whose result is its exit status. Returns how
/// the child ended and whether the fork was seen waiting for the locks.
pub fn fork_while_spawn_locks_held(child: fn(&CSys) -> i32) -> KResult<(ChildEnd, bool)> {
    static HELD: AtomicBool = AtomicBool::new(false);
    HELD.store(false, Ordering::SeqCst);
    let holder = std::thread::spawn(|| crate::spawn::hold_locks_until_contended(&HELD, 3000));
    while !HELD.load(Ordering::SeqCst) {
        std::thread::yield_now();
    }
    let pid = crate::fork::fork();
    if pid == Ok(0) {
        mark_own_process();
        let _ = CSys.alarm(10);
        CSys.exit_group(child(&CSys));
    }
    let seen = holder.join().unwrap_or(false);
    let (_, status) = CSys.wait4(pid?, 0, None)?;
    Ok((decode_status(status), seen))
}

/// How many `mq_notify` `SIGEV_THREAD` records hold a registration in this process (a refused
/// registration must not keep one).
pub fn mq_records_in_use() -> usize {
    crate::mqueue::records_in_use()
}

/// Run `body` in a child made by the library's own `fork` (D-18.6: the other threads' heap
/// caches parked, the locks handled), waiting for it as [`in_child`] does.
pub fn in_library_child(body: fn(&CSys) -> i32) -> KResult<ChildEnd> {
    let pid = crate::fork::fork()?;
    if pid == 0 {
        mark_own_process();
        let code = std::panic::catch_unwind(|| body(&CSys)).unwrap_or(PANICKED);
        CSys.exit_group(code);
    }
    let (_, status) = CSys.wait4(pid, 0, None)?;
    Ok(decode_status(status))
}

// ------------------------------------------------------------------------------ secure blocks

/// The secure heap block being watched (its address; 0 for none), and the record of its wipes:
/// count, bytes, non-zero bytes before, non-zero bytes after.
static WIPE_WATCH: AtomicUsize = AtomicUsize::new(0);
static WIPE_SEEN: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];

/// What the heap saw when it zeroed a watched secure block on its release (D-X.11).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Wipe {
    /// How many times the block was zeroed (released).
    pub count: usize,
    /// The bytes zeroed: the block's whole mapping.
    pub len: usize,
    /// How many of them were not zero just before.
    pub dirty_before: usize,
    /// How many were not zero just after (0: every byte was zeroed).
    pub dirty_after: usize,
}

/// Watch the secure heap block at `addr` (resetting the record): when any path releases it,
/// the heap's wipe records what the block held just before and just after the zeroing, while
/// it is still mapped. One block at a time, process-wide; observing touches only memory the
/// heap owns at that moment, so no freed memory is ever read.
pub fn watch_wipe(addr: usize) {
    for w in &WIPE_SEEN {
        w.store(0, Ordering::SeqCst);
    }
    WIPE_WATCH.store(addr, Ordering::SeqCst);
}

/// The record of the watched block's wipes.
pub fn watched_wipe() -> Wipe {
    let [count, len, dirty_before, dirty_after] = [0, 1, 2, 3].map(|i: usize| WIPE_SEEN.get(i).map_or(0, |w| w.load(Ordering::SeqCst)));
    Wipe { count, len, dirty_before, dirty_after }
}

fn dirty(block: &[u8]) -> usize {
    block.iter().filter(|&&b| b != 0).count()
}

fn record(i: usize, v: usize) {
    if let Some(w) = WIPE_SEEN.get(i) {
        w.store(v, Ordering::SeqCst);
    }
}

/// Before the heap zeroes the secure block at `addr`: whether it is watched (then its size and
/// non-zero bytes are recorded).
pub(crate) fn wipe_before(addr: usize, block: &[u8]) -> bool {
    let watched = addr != 0 && WIPE_WATCH.load(Ordering::SeqCst) == addr;
    if watched {
        record(1, block.len());
        record(2, dirty(block));
    }
    watched
}

/// After the heap zeroed a block: for the watched one, the non-zero bytes left and the count.
pub(crate) fn wipe_after(watched: bool, block: &[u8]) {
    if watched {
        record(3, dirty(block));
        if let Some(w) = WIPE_SEEN.first() {
            w.fetch_add(1, Ordering::SeqCst);
        }
    }
}

// ------------------------------------------------------------------------------ getpass

/// What `getpass` zeroed since the last [`pass_wipes`] (D-17.8): buffers, bytes, non-zero bytes
/// before, non-zero bytes after.
static PASS_WIPES: [AtomicUsize; 4] = [const { AtomicUsize::new(0) }; 4];

/// The record of `getpass`'s zeroing since the last call (then reset), in [`Wipe`]'s terms:
/// `count` buffers, `len` bytes in all, `dirty_before` of them not zero just before and
/// `dirty_after` not zero just after (0: every byte was zeroed).
pub fn pass_wipes() -> Wipe {
    let [count, len, dirty_before, dirty_after] = [0, 1, 2, 3].map(|i: usize| PASS_WIPES.get(i).map_or(0, |w| w.swap(0, Ordering::SeqCst)));
    Wipe { count, len, dirty_before, dirty_after }
}

fn add_pass(i: usize, v: usize) {
    if let Some(w) = PASS_WIPES.get(i) {
        w.fetch_add(v, Ordering::SeqCst);
    }
}

/// Before `getpass` zeroes `buf`: its non-zero bytes.
pub(crate) fn pass_wipe_before(buf: &[u8]) -> usize {
    dirty(buf)
}

/// After `getpass` zeroed `buf`, which held `dirty_before` non-zero bytes: the record.
pub(crate) fn pass_wipe_after(dirty_before: usize, buf: &[u8]) {
    add_pass(0, 1);
    add_pass(1, buf.len());
    add_pass(2, dirty_before);
    add_pass(3, dirty(buf));
}

/// The name-service cache's shared regions in `dir` (with its final `/`; `None`: private tables
/// only, the default of a process the library did not start). Tests only (D-16.10): the code
/// accepts a region location other than the products' `/dev/shm/` only here.
pub fn nscache_region_dir(dir: Option<&[u8]>) {
    liella_legacy::nscache::NSCACHE.set_region_dir(dir);
}

/// Make the name-service cache attach anew before its next lookup, as a `fork` child does
/// (the caller has no other thread doing lookups).
pub fn nscache_reattach() {
    liella_legacy::nscache::NSCACHE.after_fork_child();
}

// ------------------------------------------------------------------------------ test-only locations

/// The process-wide logger's channels in tests: the socket path, the console path (both
/// without a NUL) and the longest wait for a busy receiver, in nanoseconds.
type LogPaths = (&'static [u8], &'static [u8], i64);
static SYSLOG_PATHS: std::sync::RwLock<Option<LogPaths>> = std::sync::RwLock::new(None);
static SHM_DIR: std::sync::RwLock<Option<&'static [u8]>> = std::sync::RwLock::new(None);

/// A NUL-terminated copy of `b` that lives for the process.
fn leak_z(b: &[u8]) -> &'static [u8] {
    let v: std::vec::Vec<u8> = b.iter().copied().chain(core::iter::once(0)).collect();
    v.leak()
}

/// Send the process-wide logger's records (`openlog`, `syslog`) to the socket `socket` and the
/// console `console` instead of `/dev/log` and `/dev/console`, waiting at most `wait_ns` for a
/// busy receiver (`None`: the system's channels again). Tests only: the code accepts other
/// channels only here, so a test never writes to the system log.
pub fn syslog_paths(paths: Option<(&[u8], &[u8], i64)>) {
    let v = paths.map(|(s, c, w)| (leak_z(s), leak_z(c), w));
    *SYSLOG_PATHS.write().unwrap_or_else(std::sync::PoisonError::into_inner) = v;
}

pub(crate) fn syslog_override() -> Option<LogPaths> {
    *SYSLOG_PATHS.read().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Make `shm_open` and `shm_unlink` use the directory `dir` (with its final `/`) instead of
/// `/dev/shm/` (`None`: `/dev/shm/` again). Tests only (D-25.2): the code accepts another
/// directory only here, so a test never touches the system's `/dev/shm`.
pub fn shm_dir(dir: Option<&[u8]>) {
    let v = dir.map(|d| {
        let z = leak_z(d);
        z.get(..d.len()).unwrap_or(&[])
    });
    *SHM_DIR.write().unwrap_or_else(std::sync::PoisonError::into_inner) = v;
}

pub(crate) fn shm_override() -> Option<&'static [u8]> {
    *SHM_DIR.read().unwrap_or_else(std::sync::PoisonError::into_inner)
}

// ------------------------------------------------------------------------------ asynchronous I/O

/// Run this process's POSIX AIO requests on the worker pool instead of io_uring (D-11.8), so
/// tests exercise both engines. Tests only: call it before the process's first request (a
/// re-executed test binary), since the engine is chosen once.
pub fn aio_force_workers(on: bool) {
    crate::aio::force_workers(on);
}

/// Choose this process's POSIX AIO engine now, as its first request would, with `k` as the
/// kernel (D-11.11): a plan in `k` refuses `io_uring_setup` or `io_uring_register` (named in
/// `liella_kernel::testing::RAW_FALLIBLE`) or any `Sys` call the choice makes (reading
/// `/proc/thread-self/status`, `prctl`, the eventfd), as a kernel or a seccomp filter would.
/// Tests only: call it before the process's first request (a re-executed test binary); the
/// requests that follow run on the engine chosen ([`aio_engine`]).
pub fn aio_choose_engine(k: &liella_kernel::testing::Faulty<CSys>) -> KResult<()> {
    crate::aio::choose_engine(k)
}

/// The engine this process's POSIX AIO requests run on: `"ring"`, `"workers"`, or `"none"`
/// before the first request.
pub fn aio_engine() -> &'static str {
    crate::aio::engine_name()
}

/// The `io_uring_setup` flags [`io_uring_setup`] passes on (⚑ KERN, `<linux/io_uring.h>`):
/// `IORING_SETUP_CQSIZE`, `CLAMP`, `SUBMIT_ALL` and `COOP_TASKRUN`. Left out are the flags that
/// let the kernel consume submission entries by itself (`SQPOLL`) or use memory the parameters
/// name (`NO_MMAP`), so no entry a test writes into a mapped ring can ever reach the kernel.
const URING_SETUP_FLAGS: u32 = (1 << 3) | (1 << 4) | (1 << 7) | (1 << 8);

/// `io_uring_setup(entries, params)` through the C layer (the kernel-primitive tests): the new
/// instance's descriptor, which the caller closes. `params` is `struct io_uring_params` as 30
/// words (the flags in word 2). A flag outside [`URING_SETUP_FLAGS`] is `EINVAL` without a call.
pub fn io_uring_setup(entries: u32, params: &mut [u32; 30]) -> KResult<i32> {
    if params.get(2).is_some_and(|f| f & !URING_SETUP_FLAGS != 0) {
        return Err(Errno::EINVAL);
    }
    raw::io_uring_setup(entries, params)
}

/// `io_uring_enter(fd, 0, min_complete, flags, NULL, 0)` through the C layer: waits for
/// completions only. Nothing is submitted, so no entry is consumed.
pub fn io_uring_enter(fd: i32, min_complete: u32, flags: u32) -> KResult<u32> {
    raw::io_uring_enter(fd, 0, min_complete, flags)
}

/// `io_uring_register(fd, IORING_REGISTER_PROBE, buf, nr)` through the C layer: `buf` receives
/// the probe (16 bytes, then 8 per operation); `EINVAL` without room for `nr` operations.
pub fn io_uring_probe(fd: i32, buf: &mut [u8], nr: u32) -> KResult<()> {
    raw::io_uring_probe(fd, buf, nr)
}

/// Run `body` with `handler` installed for `sig` (with the boundary's restorer), then restore
/// the previous action.
pub fn with_handler<R>(sig: i32, handler: extern "C" fn(i32), body: impl FnOnce() -> R) -> KResult<R> {
    if sig == SIGKILL || sig == SIGSTOP {
        return Err(Errno::EINVAL);
    }
    with_handler_and_flags(sig, handler, 0, body)
}

/// [`with_handler`] with `sa_flags` (for example `SA_RESTART`, so that an interrupted blocking
/// call is restarted after `handler` returns).
pub fn with_handler_and_flags<R>(sig: i32, handler: extern "C" fn(i32), flags: u64, body: impl FnOnce() -> R) -> KResult<R> {
    if sig == SIGKILL || sig == SIGSTOP {
        return Err(Errno::EINVAL);
    }
    let old = raw::install_handler(sig, handler, flags, SigSet::empty())?;
    let r = body();
    raw::restore_action(sig, &old)?;
    Ok(r)
}

/// How many `strcasecmp`/`strncasecmp` calls the AVX2 kernel decided, and how many took the
/// measured path (CASECMP-K1), so far.
pub fn casecmp_paths() -> (usize, usize) {
    (crate::strz::CASECMP_KERNEL.load(Ordering::Relaxed), crate::strz::CASECMP_MEASURED.load(Ordering::Relaxed))
}

/// The rescans taken so far because text changed under a scanner (D-4.21).
pub fn rescans() -> usize {
    crate::strz::RESCANS.load(Ordering::Relaxed).saturating_add(crate::strz::strcmp_rescans())
}

/// The length check of `strlen`/`strnlen` (D-4.21) applied to the answer `n` for `text` (which
/// must hold a NUL; `None` otherwise) and `limit`, as if a scanner had reported `n`.
pub fn settle_strnlen(text: &[u8], limit: usize, n: usize) -> Option<usize> {
    crate::strz::settle_len_in(text, limit, n)
}

/// The check of `strcmp` (D-4.21) applied to the answer `i` for `a` and `b` (each must hold a
/// NUL after `i`; `None` otherwise).
pub fn settle_strcmp(a: &[u8], b: &[u8], i: usize) -> Option<core::cmp::Ordering> {
    crate::strz::settle_compare_in(a, b, i)
}

/// The check of `rawmemchr` (D-4.21) applied to the answer `n` for `text` and `c` (`text` must
/// hold `c` at or after `n`; `None` otherwise).
pub fn settle_rawmemchr(text: &[u8], c: u8, n: usize) -> Option<usize> {
    crate::strz::settle_until_in(text, c, n)
}

/// The check of `memchr` (D-4.21) applied to the answer `i` for `s` and `c`.
pub fn settle_memchr(s: &[u8], c: u8, i: usize) -> Option<usize> {
    crate::strz::settle_find_in(s, c, i)
}

/// The check of `memcmp` (D-4.21) applied to the answer `i` for `a` and `b`.
pub fn settle_memcmp(a: &[u8], b: &[u8], i: usize) -> core::cmp::Ordering {
    crate::conv::settle_compare(a, b, i)
}

/// The check of `getenv` (D-4.21) applied to `entry` (which must hold a NUL; `None` otherwise)
/// as the entry the C layer reported for `name`: the address of the value.
pub fn settle_getenv(name: &[u8], entry: &[u8]) -> Option<usize> {
    if !entry.contains(&0) {
        return None;
    }
    crate::process::settle_getenv(name, entry.as_ptr().expose_provenance())
}

/// Opaque calls for tests: the same exported functions, called through a pointer the compiler
/// cannot see through. With feature `export` the definitions carry the C names `malloc`, `free`,
/// `memcpy`, ... and an optimising build (opt-level >= 1) treats a call to such a name as the
/// standard function: it deletes a `malloc` whose result is unused, folds `calloc(1 << 33, 1 << 33)`
/// to NULL, assumes `memcpy` operands do not overlap and are not NULL, and so on. A test that
/// observes the real behaviour calls these wrappers instead (GATE-SPEED-4). Behind the `selftest`
/// feature, which no product enables; the wrappers have Rust-mangled names.
pub mod opaque {
    use core::hint::black_box;

    use liella_abi::ctypes::{c_char, c_int, c_void, size_t};

    use crate::posix::{s03_memory as m, s04_strings as st};

    /// `malloc` through an opaque pointer.
    pub fn malloc(n: size_t) -> *mut c_void {
        black_box(m::malloc as extern "C" fn(size_t) -> *mut c_void)(n)
    }
    /// `calloc` through an opaque pointer.
    pub fn calloc(k: size_t, n: size_t) -> *mut c_void {
        black_box(m::calloc as extern "C" fn(size_t, size_t) -> *mut c_void)(k, n)
    }
    /// `realloc` through an opaque pointer.
    pub fn realloc(p: *mut c_void, n: size_t) -> *mut c_void {
        black_box(m::realloc as extern "C" fn(*mut c_void, size_t) -> *mut c_void)(p, n)
    }
    /// `free` through an opaque pointer.
    pub fn free(p: *mut c_void) {
        black_box(m::free as extern "C" fn(*mut c_void))(p);
    }
    /// `memcpy` through an opaque pointer.
    pub fn memcpy(d: *mut c_void, s: *const c_void, n: size_t) -> *mut c_void {
        black_box(st::memcpy as extern "C" fn(*mut c_void, *const c_void, size_t) -> *mut c_void)(d, s, n)
    }
    /// `memmove` through an opaque pointer.
    pub fn memmove(d: *mut c_void, s: *const c_void, n: size_t) -> *mut c_void {
        black_box(st::memmove as extern "C" fn(*mut c_void, *const c_void, size_t) -> *mut c_void)(d, s, n)
    }
    /// `memset` through an opaque pointer.
    pub fn memset(d: *mut c_void, c: c_int, n: size_t) -> *mut c_void {
        black_box(st::memset as extern "C" fn(*mut c_void, c_int, size_t) -> *mut c_void)(d, c, n)
    }
    /// `aligned_alloc` through an opaque pointer.
    pub fn aligned_alloc(a: size_t, n: size_t) -> *mut c_void {
        black_box(m::aligned_alloc as extern "C" fn(size_t, size_t) -> *mut c_void)(a, n)
    }
    /// `posix_memalign` through an opaque pointer.
    pub fn posix_memalign(out: Option<&mut *mut c_void>, a: size_t, n: size_t) -> c_int {
        black_box(m::posix_memalign as extern "C" fn(Option<&mut *mut c_void>, size_t, size_t) -> c_int)(out, a, n)
    }
    /// `__memcpy_chk` through an opaque pointer.
    pub fn __memcpy_chk(d: *mut c_void, s: *const c_void, n: size_t, destlen: size_t) -> *mut c_void {
        black_box(st::__memcpy_chk as extern "C" fn(*mut c_void, *const c_void, size_t, size_t) -> *mut c_void)(d, s, n, destlen)
    }
    /// `__memset_chk` through an opaque pointer.
    pub fn __memset_chk(d: *mut c_void, c: c_int, n: size_t, destlen: size_t) -> *mut c_void {
        black_box(st::__memset_chk as extern "C" fn(*mut c_void, c_int, size_t, size_t) -> *mut c_void)(d, c, n, destlen)
    }
    /// `strdup` through an opaque pointer.
    pub fn strdup(s: *const c_char) -> *mut c_char {
        black_box(st::strdup as extern "C" fn(*const c_char) -> *mut c_char)(s)
    }
}
