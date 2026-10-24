// Specified in part from spec §20 (phase 5); `pthread_kill`/`pthread_sigqueue` (milestone 5,
// 2026-10-04) through conformance/c/s20_pthread_kill.c.
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §20. Every test that changes a disposition, a mask or sends a signal
//! runs in a re-executed child (`alone`): they are process- or thread-wide.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering};

use liella::error::Errno;
use liella_abi::layout::sigset_t;
use liella_boundary::errno;
use liella_boundary::posix::s11_lowio as io;
use liella_boundary::posix::s20_signals as x;
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{SigInfo, SigSet, StackT};
use liella_legacy::signal_compat::{self, CSigaction};

/// Build and run C conformance programs against the products (`conformance/run.sh`).
fn conformance(names: &[&str]) {
    static RUNNER: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = RUNNER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let products = ["target/liella/libc.a", "target/liella/liella-crt1.o"];
    assert!(products.iter().all(|p| root.join(p).exists()), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let o = std::process::Command::new("sh").arg(root.join("conformance/run.sh")).args(names).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

const SIGHUP: i32 = 1;
const SIGINT: i32 = 2;
const SIGKILL: i32 = 9;
const SIGUSR1: i32 = 10;
const SIGUSR2: i32 = 12;
const SIGALRM: i32 = 14;
const SIGTERM: i32 = 15;
const SIGCHLD: i32 = 17;
const SIGSTOP: i32 = 19;
const SA_NOCLDSTOP: i32 = 1;
const SA_NOCLDWAIT: i32 = 2;
const SA_SIGINFO: i32 = 4;
const SA_ONSTACK: i32 = 0x0800_0000;
const SA_RESTART: i32 = 0x1000_0000;
const SA_NODEFER: i32 = 0x4000_0000;
const SA_RESETHAND: i32 = 0x8000_0000_u32 as i32;

static HITS: AtomicU32 = AtomicU32::new(0);
static SEEN_MASK: AtomicU64 = AtomicU64::new(0);
static SEEN_TID: AtomicI32 = AtomicI32::new(0);
static SEEN_SP: AtomicUsize = AtomicUsize::new(0);
static DEPTH: AtomicU32 = AtomicU32::new(0);
static MAX_DEPTH: AtomicU32 = AtomicU32::new(0);

extern "C" fn count(_sig: i32) {
    HITS.fetch_add(1, Ordering::SeqCst);
}

/// Records the mask, thread and stack the handler runs with.
extern "C" fn observe(_sig: i32) {
    HITS.fetch_add(1, Ordering::SeqCst);
    let mut m = sigset_t { __val: [0; 16] };
    x::sigprocmask(0, None, Some(&mut m));
    SEEN_MASK.store(m.__val[0], Ordering::SeqCst);
    SEEN_TID.store(CSys.gettid(), Ordering::SeqCst);
    let local = 0u8;
    SEEN_SP.store(core::ptr::from_ref(&local) as usize, Ordering::SeqCst);
}

/// Re-raises its own signal once from inside the handler: nested only with `SA_NODEFER`.
extern "C" fn reenter(sig: i32) {
    let d = DEPTH.fetch_add(1, Ordering::SeqCst) + 1;
    MAX_DEPTH.fetch_max(d, Ordering::SeqCst);
    if HITS.fetch_add(1, Ordering::SeqCst) == 0 {
        x::raise(sig);
    }
    DEPTH.fetch_sub(1, Ordering::SeqCst);
}

static INFO_CODE: AtomicI32 = AtomicI32::new(0);
static CTX_SEEN: AtomicU32 = AtomicU32::new(0);

extern "C" fn three(_sig: i32, info: Option<&SigInfo>, ctx: *mut core::ffi::c_void) {
    if let Some(i) = info {
        INFO_CODE.store(i.code, Ordering::SeqCst);
    }
    CTX_SEEN.store(u32::from(!ctx.is_null()), Ordering::SeqCst);
}

/// Waits (up to a second) for `n` handler runs: a process-directed signal may be handled by
/// another thread, after the send returned.
fn wait_hits(n: u32) {
    for _ in 0..1000 {
        if HITS.load(Ordering::SeqCst) >= n {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn h(f: extern "C" fn(i32)) -> usize {
    f as usize
}

fn act(handler: usize, flags: i32, mask: &[i32]) -> CSigaction {
    let mut m = SigSet::empty();
    for &s in mask {
        m = m.with(s);
    }
    let mut a = CSigaction::new(handler, m, 0);
    a.flags = flags;
    a
}

fn set_of(sigs: &[i32]) -> sigset_t {
    let mut s = sigset_t { __val: [0; 16] };
    for &g in sigs {
        x::sigaddset(Some(&mut s), g);
    }
    s
}

fn mask_now() -> u64 {
    let mut m = sigset_t { __val: [0; 16] };
    x::sigprocmask(0, None, Some(&mut m));
    m.__val[0]
}

fn bit(sig: i32) -> u64 {
    1 << (sig - 1)
}

/// Sends `sig` to the calling thread after `ms` milliseconds, from another thread.
fn later(sig: i32, ms: u64) {
    let (pid, tid) = (CSys.getpid(), CSys.gettid());
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(ms));
        let _ = CSys.tgkill(pid, tid, sig);
    });
}

fn alone(name: &str, body: impl FnOnce() -> bool) {
    if std::env::var("LIELLA_C20_ALONE").is_ok_and(|v| v == name) {
        CSys.exit_group(if body() { 0 } else { 1 });
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--test-threads=1"])
        .env("LIELLA_C20_ALONE", name)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{name} failed in its child: {}", child_report(&o));
}

#[test]
fn s20_1_c_0() {
    let mut s = sigset_t { __val: [0; 16] };
    for sig in 1..=64 {
        if sig != 32 {
            assert_eq!(x::sigaddset(Some(&mut s), sig), 0, "signal {sig}");
        }
    }
    errno::set_raw(0);
    assert_eq!((x::sigaddset(Some(&mut s), 65), errno::get()), (-1, Errno::EINVAL.get()), "_NSIG = 65");
    assert_eq!((x::sigaddset(Some(&mut s), 0), errno::get()), (-1, Errno::EINVAL.get()));
}

#[test]
fn s20_1_c_1() {
    for sig in 1..=31 {
        assert!(signal_compat::sigdescr(sig).is_some(), "standard signal {sig} has a description");
    }
    // Standard signals do not queue: two while blocked are delivered once.
    alone("s20_1_c_1", || {
        x::signal(SIGUSR1, h(count));
        x::sigprocmask(0, Some(&set_of(&[SIGUSR1])), None);
        x::raise(SIGUSR1);
        x::raise(SIGUSR1);
        x::sigprocmask(1, Some(&set_of(&[SIGUSR1])), None);
        HITS.load(Ordering::SeqCst) == 1
    });
}

#[test]
fn s20_1_c_2() {
    // D-20.1 (approved): one internal signal, 32, so applications see SIGRTMIN = 33 (glibc: 34).
    assert_eq!((x::__libc_current_sigrtmin(), x::__libc_current_sigrtmax()), (33, 64));
    // Real-time signals queue, carry a value, and are dequeued lowest first.
    alone("s20_1_c_2", || {
        let set = set_of(&[35, 40]);
        x::sigprocmask(0, Some(&set), None);
        x::raise(40);
        x::raise(35);
        x::raise(40);
        let mut order = Vec::new();
        for _ in 0..3 {
            order.push(x::sigwaitinfo(Some(&set), None));
        }
        let mut p = sigset_t { __val: [0; 16] };
        x::sigpending(Some(&mut p));
        order == [35, 40, 40] && x::sigisemptyset(Some(&p)) == 1
    });
    // OD-8: 32 is never an application signal: no name, no real-time text, not sendable by
    // raise, pthread_kill or pthread_sigqueue, never in a set the library reports (the C
    // program, with the library's real start-up and handler).
    conformance(&["s20_internal_signal"]);
}

#[test]
fn s20_2_c_1() {
    alone("s20_2_c_1", || {
        let mut old = act(9, 9, &[]);
        let a = act(h(observe), 0, &[SIGHUP]);
        let set = x::sigaction(SIGUSR1, Some(&a), Some(&mut old)) == 0 && old.handler == 0;
        x::raise(SIGUSR1);
        let mask = SEEN_MASK.load(Ordering::SeqCst);
        let mut ign = act(1, 0, &[]);
        x::sigaction(SIGUSR2, Some(&ign), None);
        x::raise(SIGUSR2);
        x::sigaction(SIGUSR2, None, Some(&mut ign));
        set && mask & bit(SIGUSR1) != 0 && mask & bit(SIGHUP) != 0 && ign.handler == 1 && mask_now() & bit(SIGHUP) == 0
    });
}

#[test]
fn s20_2_c_1_1() {
    alone("s20_2_c_1_1", || {
        let a = act(three as extern "C" fn(i32, Option<&SigInfo>, *mut core::ffi::c_void) as usize, SA_SIGINFO, &[]);
        x::sigaction(SIGUSR1, Some(&a), None);
        x::raise(SIGUSR1);
        INFO_CODE.load(Ordering::SeqCst) == -6 && CTX_SEEN.load(Ordering::SeqCst) == 1
    });
}

#[test]
fn s20_2_c_1_2() {
    // SA_RESTART resumes a blocking read; without it the read fails with EINTR (C0.3).
    alone("s20_2_c_1_2", || {
        let mut p = [-1; 2];
        liella_boundary::posix::s25_ipc::pipe(Some(&mut p));
        let (r, w) = (p[0], p[1]);
        x::sigaction(SIGALRM, Some(&act(h(count), SA_RESTART, &[])), None);
        later(SIGALRM, 50);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            io::write(w, b"x".as_ptr().cast(), 1);
        });
        let mut b = [0u8; 1];
        let restarted = io::read(r, b.as_mut_ptr().cast(), 1) == 1 && HITS.load(Ordering::SeqCst) == 1;
        x::sigaction(SIGALRM, Some(&act(h(count), 0, &[])), None);
        later(SIGALRM, 50);
        errno::set_raw(0);
        let interrupted = io::read(r, b.as_mut_ptr().cast(), 1) == -1 && errno::get() == Errno::EINTR.get();
        restarted && interrupted
    });
}

#[test]
fn s20_2_c_1_3() {
    alone("s20_2_c_1_3", || {
        x::sigaction(SIGUSR1, Some(&act(h(reenter), 0, &[])), None);
        x::raise(SIGUSR1);
        let deferred = MAX_DEPTH.load(Ordering::SeqCst) == 1 && HITS.load(Ordering::SeqCst) == 2;
        HITS.store(0, Ordering::SeqCst);
        MAX_DEPTH.store(0, Ordering::SeqCst);
        x::sigaction(SIGUSR2, Some(&act(h(reenter), SA_NODEFER, &[])), None);
        x::raise(SIGUSR2);
        let nested = MAX_DEPTH.load(Ordering::SeqCst) == 2;
        HITS.store(0, Ordering::SeqCst);
        x::sigaction(SIGHUP, Some(&act(h(count), SA_RESETHAND, &[])), None);
        x::raise(SIGHUP);
        let mut after = act(9, 0, &[]);
        x::sigaction(SIGHUP, None, Some(&mut after));
        deferred && nested && after.handler == 0
    });
}

#[test]
fn s20_2_c_1_4() {
    alone("s20_2_c_1_4", || {
        let mut mem = vec![0u8; 1 << 16];
        let mut ss = StackT::default();
        ss.sp = mem.as_mut_ptr() as usize;
        ss.size = mem.len();
        x::sigaltstack(Some(&ss), None);
        x::sigaction(SIGUSR1, Some(&act(h(observe), SA_ONSTACK, &[])), None);
        x::raise(SIGUSR1);
        let sp = SEEN_SP.load(Ordering::SeqCst);
        sp >= ss.sp && sp < ss.sp + ss.size
    });
}

#[test]
fn s20_2_c_1_5() {
    alone("s20_2_c_1_5", || {
        x::sigaction(SIGCHLD, Some(&act(h(count), SA_NOCLDSTOP, &[])), None);
        let mut child = std::process::Command::new("sleep").arg("5").spawn().unwrap();
        let pid = child.id() as i32;
        x::kill(pid, SIGSTOP);
        std::thread::sleep(std::time::Duration::from_millis(100));
        let quiet = HITS.load(Ordering::SeqCst) == 0;
        x::kill(pid, SIGKILL);
        let _ = child.wait();
        let mut a = act(0, 0, &[]);
        x::sigaction(SIGCHLD, Some(&act(0, SA_NOCLDWAIT, &[])), None);
        x::sigaction(SIGCHLD, None, Some(&mut a));
        quiet && a.flags & SA_NOCLDWAIT != 0
    });
}

#[test]
fn s20_2_c_2() {
    alone("s20_2_c_2", || {
        x::signal(SIGUSR1, h(observe));
        x::raise(SIGUSR1);
        x::raise(SIGUSR1);
        let bsd = HITS.load(Ordering::SeqCst) == 2 && SEEN_MASK.load(Ordering::SeqCst) & bit(SIGUSR1) != 0;
        HITS.store(0, Ordering::SeqCst);
        x::sysv_signal(SIGUSR2, h(observe));
        x::raise(SIGUSR2);
        let unblocked = SEEN_MASK.load(Ordering::SeqCst) & bit(SIGUSR2) == 0;
        let mut a = act(9, 0, &[]);
        x::sigaction(SIGUSR2, None, Some(&mut a));
        bsd && unblocked && a.handler == 0 && a.flags & SA_RESTART == 0
    });
}

#[test]
fn s20_2_c_3() {
    alone("s20_2_c_3", || {
        x::signal(SIGALRM, h(count));
        x::siginterrupt(SIGALRM, 1);
        let mut p = [-1; 2];
        liella_boundary::posix::s25_ipc::pipe(Some(&mut p));
        later(SIGALRM, 50);
        let mut b = [0u8; 1];
        errno::set_raw(0);
        io::read(p[0], b.as_mut_ptr().cast(), 1) == -1 && errno::get() == Errno::EINTR.get()
    });
}

#[test]
fn s20_2_c_4() {
    alone("s20_2_c_4", || {
        x::sigset(SIGUSR1, h(count));
        x::sighold(SIGUSR1);
        x::raise(SIGUSR1);
        let held = HITS.load(Ordering::SeqCst) == 0;
        x::sigrelse(SIGUSR1);
        let released = HITS.load(Ordering::SeqCst) == 1;
        x::sigignore(SIGUSR1);
        x::raise(SIGUSR1);
        held && released && HITS.load(Ordering::SeqCst) == 1
    });
}

#[test]
fn s20_2_c_5() {
    alone("s20_2_c_5", || {
        x::ssignal(SIGUSR1, h(count));
        x::gsignal(SIGUSR1) == 0 && HITS.load(Ordering::SeqCst) == 1
    });
}

#[test]
fn s20_2_c_6() {
    alone("s20_2_c_6", || {
        let v = signal_compat::CSigvec { handler: h(count), mask: 0, flags: 0 };
        x::sigvec(SIGUSR2, Some(&v), None);
        x::raise(SIGUSR2);
        HITS.load(Ordering::SeqCst) == 1
    });
}

#[test]
fn s20_2_c_7() {
    errno::set_raw(0);
    assert_eq!((x::sigreturn(core::ptr::null_mut()), errno::get()), (-1, Errno::ENOSYS.get()));
    // The real return: a handler returns through the restorer and the interrupted code goes on
    // with its state intact.
    alone("s20_2_c_7", || {
        x::signal(SIGUSR1, h(count));
        let mut acc = 0u64;
        for i in 0..1000u64 {
            acc = acc.wrapping_mul(31).wrapping_add(i);
            if i % 100 == 0 {
                x::raise(SIGUSR1);
            }
        }
        let mut want = 0u64;
        for i in 0..1000u64 {
            want = want.wrapping_mul(31).wrapping_add(i);
        }
        acc == want && HITS.load(Ordering::SeqCst) == 10
    });
}

#[test]
fn s20_2_f_0() {
    let a = act(h(count), 0, &[]);
    errno::set_raw(0);
    for sig in [0, 65, 32, SIGKILL, SIGSTOP] {
        assert_eq!((x::sigaction(sig, Some(&a), None), errno::get()), (-1, Errno::EINVAL.get()), "signal {sig}");
    }
    assert_eq!((x::sigaction(SIGKILL, Some(&act(1, 0, &[])), None), errno::get()), (-1, Errno::EINVAL.get()), "nor ignore it");
    // OD-8: no route changes the internal signal's action: sigaction, signal, sigset,
    // sigignore, sighold, sigrelse, siginterrupt (the kernel's action read back raw).
    conformance(&["s20_internal_signal"]);
}

#[test]
fn s20_3_c_1() {
    alone("s20_3_c_1", || {
        let before = mask_now();
        let other = std::thread::spawn(|| {
            x::pthread_sigmask(0, Some(&set_of(&[SIGUSR1])), None);
            mask_now()
        })
        .join()
        .unwrap();
        errno::set_raw(0);
        let errnum = x::pthread_sigmask(5, Some(&set_of(&[1])), None) == Errno::EINVAL.get() && errno::get() == 0;
        let errno_conv = x::sigprocmask(5, Some(&set_of(&[1])), None) == -1 && errno::get() == Errno::EINVAL.get();
        other & bit(SIGUSR1) != 0 && mask_now() == before && errnum && errno_conv
    });
}

#[test]
fn s20_3_c_2() {
    let a = set_of(&[1, 2]);
    let b = set_of(&[2, 3]);
    let mut d = sigset_t { __val: [0; 16] };
    x::sigandset(Some(&mut d), Some(&a), Some(&b));
    assert_eq!(d.__val[0], 0b10);
    x::sigorset(Some(&mut d), Some(&a), Some(&b));
    assert_eq!(d.__val[0], 0b111);
    assert_eq!(x::sigisemptyset(Some(&set_of(&[]))), 1);
    let mut raw = sigset_t { __val: [0; 16] };
    x::__sigaddset(Some(&mut raw), 5);
    assert_eq!(x::__sigismember(Some(&raw), 5), 1);
}

#[test]
fn s20_3_c_3() {
    alone("s20_3_c_3", || {
        x::sigsetmask(0);
        let old = x::sigblock((1 << (SIGINT - 1)) | (1 << (SIGTERM - 1)));
        let now = x::siggetmask();
        old == 0 && now == (1 << 1) | (1 << 14) && x::sigsetmask(0) == now && x::siggetmask() == 0
    });
}

#[test]
fn s20_3_c_4() {
    alone("s20_3_c_4", || {
        x::signal(SIGUSR1, h(count));
        x::signal(SIGUSR2, h(count));
        x::sigprocmask(0, Some(&set_of(&[SIGUSR1, SIGUSR2])), None);
        later(SIGUSR1, 30);
        let xsi = x::__xpg_sigpause(SIGUSR1) == -1;
        let after_xsi = mask_now() & bit(SIGUSR2) != 0 && HITS.load(Ordering::SeqCst) == 1;
        later(SIGUSR2, 30);
        let bsd = x::sigpause(0) == -1;
        xsi && after_xsi && bsd && HITS.load(Ordering::SeqCst) == 2 && mask_now() & bit(SIGUSR1) != 0
    });
}

#[test]
fn s20_3_g_0() {
    alone("s20_3_g_0", || {
        let all = sigset_t { __val: [u64::MAX; 16] };
        x::sigprocmask(2, Some(&all), None);
        let m = mask_now();
        let mut raw = sigset_t { __val: [0; 16] };
        x::__sigaddset(Some(&mut raw), 32);
        x::sigprocmask(0, Some(&raw), None);
        let k = signal_compat::kernel_set(&all);
        m & bit(SIGKILL) == 0 && m & bit(SIGSTOP) == 0 && m & bit(32) == 0 && mask_now() & bit(32) == 0 && !k.contains(32)
    });
    // OD-8: no route blocks the internal signal: sigprocmask, pthread_sigmask, sigblock,
    // sigsetmask, an action's sa_mask, setcontext and swapcontext masks, signalfd's set,
    // sigsuspend's and the waits' sets, posix_spawn's SETSIGMASK; the kernel's view read raw.
    conformance(&["s20_internal_signal"]);
}

#[test]
fn s20_4_c_1() {
    alone("s20_4_c_1", || {
        // A group of its own: the group signals must not reach the test harness. A
        // process-directed signal may be handled by any thread of the child.
        let _ = CSys.setpgid(0, 0);
        x::signal(SIGUSR1, h(count));
        let me = std::process::id() as i32;
        let exists = x::kill(me, 0) == 0;
        let to_me = x::kill(me, SIGUSR1) == 0;
        wait_hits(1);
        let group = x::kill(0, SIGUSR1) == 0;
        wait_hits(2);
        let neg = x::kill(-CSys.getpgid(0).unwrap(), SIGUSR1) == 0;
        wait_hits(3);
        exists && to_me && group && neg && HITS.load(Ordering::SeqCst) == 3
    });
}

#[test]
fn s20_4_c_2() {
    alone("s20_4_c_2", || {
        let _ = CSys.setpgid(0, 0);
        x::signal(SIGUSR2, h(count));
        let ok = x::killpg(CSys.getpgid(0).unwrap(), SIGUSR2) == 0;
        wait_hits(1);
        ok && HITS.load(Ordering::SeqCst) == 1
    });
}

#[test]
fn s20_4_c_3() {
    alone("s20_4_c_3", || {
        x::signal(SIGUSR1, h(observe));
        let me = CSys.gettid();
        let r = x::raise(SIGUSR1);
        let delivered_before_return = HITS.load(Ordering::SeqCst) == 1;
        let other = std::thread::spawn(|| {
            let t = CSys.gettid();
            x::raise(SIGUSR1);
            t
        })
        .join()
        .unwrap();
        r == 0 && delivered_before_return && SEEN_TID.load(Ordering::SeqCst) == other && other != me
    });
    // OD-8: raise of the internal signal is EINVAL (as glibc refuses its own internal signals),
    // checked before anything is sent.
    errno::set_raw(0);
    assert_eq!((x::raise(32), errno::get()), (-1, Errno::EINVAL.get()));
}

#[test]
fn s20_4_c_4() {
    alone("s20_4_c_4", || {
        static VALUES: [AtomicI32; 2] = [const { AtomicI32::new(0) }; 2];
        static CODES: [AtomicI32; 2] = [const { AtomicI32::new(0) }; 2];
        static ENTERED: AtomicU32 = AtomicU32::new(0);
        // The signal is process-directed, so the two handlers may run at once on two threads
        // (libtest's and the main one): each claims its slot with ENTERED and counts in HITS
        // only after its store, so `wait_hits(2)` never reads a slot still being written.
        extern "C" fn val(_sig: i32, info: Option<&SigInfo>, _ctx: *mut core::ffi::c_void) {
            let i = ENTERED.fetch_add(1, Ordering::SeqCst) as usize;
            if let (Some(info), Some(slot)) = (info, VALUES.get(i)) {
                CODES[i].store(info.code, Ordering::SeqCst);
                slot.store(info.value() as i32 * if info.code == -1 { 1 } else { -1 }, Ordering::SeqCst);
            }
            HITS.fetch_add(1, Ordering::SeqCst);
        }
        x::sigaction(40, Some(&act(val as extern "C" fn(i32, Option<&SigInfo>, *mut core::ffi::c_void) as usize, SA_SIGINFO, &[])), None);
        let me = std::process::id() as i32;
        let ra = (x::sigqueue(me, 40, 1), errno::get());
        let rb = (x::sigqueue(me, 40, 2), errno::get());
        wait_hits(2);
        let (entered, stored) = (ENTERED.load(Ordering::SeqCst), HITS.load(Ordering::SeqCst));
        let seen = [VALUES[0].load(Ordering::SeqCst), VALUES[1].load(Ordering::SeqCst)];
        let codes = [CODES[0].load(Ordering::SeqCst), CODES[1].load(Ordering::SeqCst)];
        let mut got = seen;
        got.sort_unstable();
        let ok = ra.0 == 0 && rb.0 == 0 && got == [1, 2];
        if !ok {
            // Straight to fd 2: libtest's capture of `eprintln!` is lost when the child exits.
            use std::io::Write as _;
            let _ = writeln!(
                std::io::stderr(),
                "s20_4_c_4: sigqueue(1) = {ra:?} (rc, errno), sigqueue(2) = {rb:?}; handler entries {entered}, \
                 stores done {stored}; values {seen:?} (negative: si_code not SI_QUEUE); si_code {codes:?}"
            );
        }
        ok
    });
}

/// §20.4: `pthread_kill` and `pthread_sigqueue` target one thread of this process. Threads
/// must be Liella's own (`pthread_create`), so the clause is checked by the self-checking C
/// program `conformance/c/s20_pthread_kill.c`: the handler runs on the target thread, signal 0
/// only checks, an invalid signal is `EINVAL` returned with `errno` untouched, the queued value
/// and `SI_QUEUE` arrive, the caller's own thread gets the signal before the call returns, and a
/// thread that has exited unjoined accepts `pthread_kill` (0) while `pthread_sigqueue` is `ESRCH`.
#[test]
fn s20_4_c_5() {
    conformance(&["s20_pthread_kill"]);
}
#[test]
fn s20_4_c_6() {
    alone("s20_4_c_6", || {
        x::signal(SIGUSR1, h(observe));
        let tid = std::thread::spawn(|| {
            let t = CSys.gettid();
            std::thread::sleep(std::time::Duration::from_millis(200));
            t
        });
        std::thread::sleep(std::time::Duration::from_millis(50));
        let _ = tid;
        x::tgkill(CSys.getpid(), CSys.gettid(), SIGUSR1) == 0 && SEEN_TID.load(Ordering::SeqCst) == CSys.gettid()
    });
}

#[test]
fn s20_4_f_0() {
    errno::set_raw(0);
    assert_eq!((x::kill(std::process::id() as i32, 99), errno::get()), (-1, Errno::EINVAL.get()), "EINVAL");
    assert_eq!((x::kill(i32::MAX - 1, 0), errno::get()), (-1, Errno::ESRCH.get()), "ESRCH");
    if CSys.geteuid() != 0 {
        assert_eq!((x::kill(1, 0), errno::get()), (-1, Errno::EPERM.get()), "EPERM: init belongs to root");
    }
}

#[test]
fn s20_5_c_1() {
    alone("s20_5_c_1", || {
        x::signal(SIGUSR1, h(count));
        later(SIGUSR1, 30);
        errno::set_raw(0);
        x::pause() == -1 && errno::get() == Errno::EINTR.get() && HITS.load(Ordering::SeqCst) == 1
    });
}

#[test]
fn s20_5_c_2() {
    alone("s20_5_c_2", || {
        x::signal(SIGUSR1, h(observe));
        x::sigprocmask(2, Some(&set_of(&[SIGUSR1, SIGHUP])), None);
        later(SIGUSR1, 30);
        let r = x::sigsuspend(Some(&set_of(&[SIGHUP])));
        let during = SEEN_MASK.load(Ordering::SeqCst);
        r == -1 && during & bit(SIGHUP) != 0 && mask_now() == bit(SIGUSR1) | bit(SIGHUP)
    });
}

#[test]
fn s20_5_c_3() {
    alone("s20_5_c_3", || {
        let set = set_of(&[SIGUSR2]);
        x::sigprocmask(0, Some(&set), None);
        later(SIGUSR2, 20);
        let mut got = 0;
        errno::set_raw(0);
        let r = x::sigwait(Some(&set), Some(&mut got));
        r == 0 && got == SIGUSR2 && x::sigwait(None, Some(&mut got)) == Errno::EFAULT.get()
    });
}

#[test]
fn s20_5_c_4() {
    alone("s20_5_c_4", || {
        let set = set_of(&[SIGUSR1]);
        x::sigprocmask(0, Some(&set), None);
        x::raise(SIGUSR1);
        let mut info = SigInfo::default();
        let got = x::sigwaitinfo(Some(&set), Some(&mut info)) == SIGUSR1 && info.signo == SIGUSR1;
        errno::set_raw(0);
        let ts = liella_kernel::uapi::Timespec { sec: 0, nsec: 5_000_000 };
        let timed = x::sigtimedwait(Some(&set), None, Some(&ts)) == -1 && errno::get() == Errno::EAGAIN.get();
        got && timed
    });
}

#[test]
fn s20_5_c_5() {
    alone("s20_5_c_5", || {
        x::sigprocmask(0, Some(&set_of(&[SIGINT, SIGTERM])), None);
        x::raise(SIGTERM);
        let mut p = sigset_t { __val: [0; 16] };
        x::sigpending(Some(&mut p)) == 0 && p.__val[0] == bit(SIGTERM)
    });
}

/// §20.5 Cancellation: All the waiting functions are cancellation points.
#[test]
fn s20_5_x_0() {
    conformance(&["s22_cancel_points", "s22_cancel"]);
}
#[test]
fn s20_6_c_0() {
    alone("s20_6_c_0", || {
        let set = set_of(&[SIGUSR1, SIGUSR2]);
        x::sigprocmask(0, Some(&set), None);
        let fd = x::signalfd(-1, Some(&set), 0o4000);
        x::raise(SIGUSR2);
        let mut rec = [0u32; 32];
        let a = io::read(fd, rec.as_mut_ptr().cast(), 128) == 128 && rec[0] == SIGUSR2 as u32;
        errno::set_raw(0);
        let empty = io::read(fd, rec.as_mut_ptr().cast(), 128) == -1 && errno::get() == Errno::EAGAIN.get();
        let changed = x::signalfd(fd, Some(&set_of(&[SIGUSR1])), 0) == fd;
        a && empty && changed
    });
}

#[test]
fn s20_7_c_0() {
    alone("s20_7_c_0", || {
        let mut mem = vec![0u8; 1 << 16];
        let mut ss = StackT::default();
        ss.sp = mem.as_mut_ptr() as usize;
        ss.size = mem.len();
        ss.flags = (1u32 << 31) as i32; // SS_AUTODISARM
        let set = x::sigaltstack(Some(&ss), None) == 0;
        x::sigaction(SIGUSR1, Some(&act(h(observe), SA_ONSTACK, &[])), None);
        x::raise(SIGUSR1);
        let sp = SEEN_SP.load(Ordering::SeqCst);
        let on = sp >= ss.sp && sp < ss.sp + ss.size;
        let mut off = StackT::default();
        off.flags = 2; // SS_DISABLE
        x::sigaltstack(Some(&off), None);
        let mut now = StackT::default();
        x::sigaltstack(None, Some(&mut now));
        set && on && now.flags & 2 != 0
    });
}

#[test]
fn s20_7_i_0() {
    // The minimum is the kernel's (AT_MINSIGSTKSZ), what sysconf reports.
    let min = liella_boundary::posix::s24_system::sysconf(249);
    assert!(min >= 1024);
    alone("s20_7_i_0", || {
        let min = liella_boundary::posix::s24_system::sysconf(249) as usize;
        let mut mem = vec![0u8; 1 << 16];
        let mut ss = StackT::default();
        ss.sp = mem.as_mut_ptr() as usize;
        ss.size = min.min(2048) - 1;
        errno::set_raw(0);
        let small = x::sigaltstack(Some(&ss), None) == -1 && errno::get() == Errno::ENOMEM.get();
        ss.size = liella_boundary::posix::s24_system::sysconf(250) as usize;
        small && x::sigaltstack(Some(&ss), None) == 0
    });
}

#[test]
fn s20_8_c_1() {
    let text = |p: *mut core::ffi::c_char| {
        let n = liella_boundary::posix::s04_strings::strlen(p);
        let mut v = vec![0u8; n];
        liella_boundary::posix::s04_strings::memcpy(v.as_mut_ptr().cast(), p.cast(), n);
        String::from_utf8(v).unwrap()
    };
    assert_eq!(text(x::strsignal(SIGTERM)), "Terminated", "the C locale's text (translation is phase 7)");
    let a = x::strsignal(99);
    assert_eq!(text(a), "Unknown signal 99");
    assert_eq!(x::strsignal(98), a, "unknown numbers share the library's buffer (per thread once threads have slots, phase 6)");
}

#[test]
fn s20_8_c_2() {
    let p = x::sigabbrev_np(SIGINT);
    let n = liella_boundary::posix::s04_strings::strlen(p.cast_mut());
    assert_eq!(n, 3, "INT");
    assert!(!x::sigdescr_np(SIGINT).is_null());
    assert!(x::sigdescr_np(33).is_null(), "no description for real-time signals");
}

#[test]
fn s20_8_c_3() {
    let mut out = Vec::new();
    signal_compat::psignal(SIGINT, Some(b"prog"), |b| out.extend_from_slice(b));
    assert_eq!(out, b"prog: Interrupt\n");
    out.clear();
    signal_compat::psignal(SIGINT, None, |b| out.extend_from_slice(b));
    assert_eq!(out, b"Interrupt\n", "no prefix: no colon");
    out.clear();
    let mut i = SigInfo::default();
    i.signo = SIGALRM;
    signal_compat::psiginfo(&i, Some(b"p"), |b| out.extend_from_slice(b));
    assert_eq!(out, b"p: Alarm clock (Signal sent by kill() 0 0)\n");
}

#[test]
fn s20_9_c_0() {
    alone("s20_9_c_0", || {
        x::sigaction(SIGUSR1, Some(&act(h(observe), 0, &[SIGTERM])), None);
        let me = CSys.gettid();
        errno::set_raw(4321);
        x::raise(SIGUSR1);
        let mask = SEEN_MASK.load(Ordering::SeqCst);
        SEEN_TID.load(Ordering::SeqCst) == me && mask & bit(SIGUSR1) != 0 && mask & bit(SIGTERM) != 0 && errno::get() == 4321
    });
}

/// The child's exit status, stdout and stderr (the last 60 lines of each) for a failure message.
fn child_report(o: &std::process::Output) -> String {
    let tail = |b: &[u8]| {
        let s = String::from_utf8_lossy(b);
        let lines: Vec<&str> = s.lines().collect();
        lines[lines.len().saturating_sub(60)..].join("\n")
    };
    format!("exit {:?}\n--- stdout ---\n{}\n--- stderr ---\n{}", o.status, tail(&o.stdout), tail(&o.stderr))
}
