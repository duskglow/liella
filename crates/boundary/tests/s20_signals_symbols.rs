// Specified in part from spec §20 (phase 5: §20.1–20.9); `siglongjmp` and `__sigsetjmp` are
// §21 (phase 6).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Translation-layer tests for the exported symbols of spec §20.
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! Dispositions and masks are process- or thread-wide, so every test that installs a handler,
//! blocks a signal or sends one runs in a re-executed child of this binary (`alone`).
//! `psignal`/`psiginfo` output is compared with the host glibc's, byte for byte.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(non_snake_case)]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicI32, AtomicU32, Ordering};

use liella_abi::layout::sigset_t;
use liella_boundary::errno;
use liella_boundary::posix::s20_signals as s;
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{SigInfo, StackT, Timespec};
use liella_legacy::signal_compat::{CSigaction, CSigstack, CSigvec};

mod products;
#[path = "../../core/tests/support/scratch.rs"]
mod scratch;
use products::{conformance, exported};

const EINVAL: i32 = 22;
const EFAULT: i32 = 14;
const EINTR: i32 = 4;
const EAGAIN: i32 = 11;
const ENOSYS: i32 = 38;
const ESRCH: i32 = 3;
const SIGHUP: i32 = 1;
const SIGINT: i32 = 2;
const SIGKILL: i32 = 9;
const SIGUSR1: i32 = 10;
const SIGUSR2: i32 = 12;
const SIGALRM: i32 = 14;
const SIGTERM: i32 = 15;
const SIGSTOP: i32 = 19;
const SIG_BLOCK: i32 = 0;
const SIG_UNBLOCK: i32 = 1;
const SIG_SETMASK: i32 = 2;
const SA_SIGINFO: i32 = 4;
const SA_RESTART: i32 = 0x1000_0000;
const SA_RESETHAND: i32 = 0x8000_0000_u32 as i32;
const SIG_IGN: usize = 1;
const SIG_HOLD: usize = 2;
const SIG_ERR: usize = usize::MAX;

static HITS: AtomicU32 = AtomicU32::new(0);
static LAST: AtomicI32 = AtomicI32::new(0);
static INFO_PID: AtomicI32 = AtomicI32::new(0);
static INFO_VALUE: AtomicI32 = AtomicI32::new(0);

extern "C" fn count(sig: i32) {
    HITS.fetch_add(1, Ordering::SeqCst);
    LAST.store(sig, Ordering::SeqCst);
}

extern "C" fn with_info(sig: i32, info: Option<&SigInfo>, _ctx: *mut core::ffi::c_void) {
    HITS.fetch_add(1, Ordering::SeqCst);
    LAST.store(sig, Ordering::SeqCst);
    if let Some(i) = info {
        INFO_PID.store(i.pid(), Ordering::SeqCst);
        INFO_VALUE.store(i.value() as i32, Ordering::SeqCst);
    }
}

/// Waits (up to a second) for the handler to have run `n` times: a process-directed signal
/// may be handled by another thread, after the send returned.
fn wait_hits(n: u32) {
    for _ in 0..1000 {
        if HITS.load(Ordering::SeqCst) >= n {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn handler() -> usize {
    count as extern "C" fn(i32) as usize
}

fn empty() -> sigset_t {
    let mut set = sigset_t { __val: [7; 16] };
    s::sigemptyset(Some(&mut set));
    set
}

fn set_of(sigs: &[i32]) -> sigset_t {
    let mut set = empty();
    for &g in sigs {
        s::sigaddset(Some(&mut set), g);
    }
    set
}

fn action(h: usize, flags: i32) -> CSigaction {
    let mut a = CSigaction::new(h, liella_kernel::uapi::SigSet::empty(), 0);
    a.flags = flags;
    a
}

fn fails(r: isize, e: i32) -> bool {
    r == -1 && errno::get() == e
}

/// Runs `body` in a child (this test re-executed alone): dispositions, masks and signals are
/// process- or thread-wide.
fn alone(name: &str, body: impl FnOnce() -> bool) {
    if std::env::var("LIELLA_S20_ALONE").is_ok_and(|v| v == name) {
        CSys.exit_group(if body() { 0 } else { 1 });
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--test-threads=1"])
        .env("LIELLA_S20_ALONE", name)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "{name} failed in its child: {}", child_report(&o));
}

/// The host glibc's standard error for C `body`.
fn host_stderr(tag: &str, body: &str) -> String {
    let dir = scratch::Scratch::dir(&format!("s20-{tag}"));
    let c = dir.join("p.c");
    std::fs::write(&c, format!("#define _GNU_SOURCE 1\n#include <signal.h>\n#include <string.h>\n#include <stdio.h>\nint main(void){{ siginfo_t i; (void)i;\n{body}\nreturn 0; }}\n")).unwrap();
    let exe = dir.join("p");
    assert!(std::process::Command::new("cc").arg("-w").arg(&c).arg("-o").arg(&exe).status().unwrap().success());
    let out = std::process::Command::new(&exe).output().unwrap();
    String::from_utf8(out.stderr).unwrap()
}

/// This binary's own standard error for `body`, run in a child.
fn own_stderr(name: &str, body: impl FnOnce()) -> String {
    if std::env::var("LIELLA_S20_ERR").is_ok_and(|v| v == name) {
        body();
        CSys.exit_group(0);
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--test-threads=1", "--nocapture"])
        .env("LIELLA_S20_ERR", name)
        .output()
        .unwrap();
    let all = String::from_utf8(o.stderr).unwrap();
    all.lines().filter(|l| l.contains("LIELLA")).map(|l| format!("{l}\n")).collect()
}

/// The C string at `p`, read through the library's own `strlen` and `memcpy`.
fn unsafe_cstr(p: *mut core::ffi::c_char) -> String {
    assert!(!p.is_null());
    let n = liella_boundary::posix::s04_strings::strlen(p);
    let mut v = vec![0u8; n];
    liella_boundary::posix::s04_strings::memcpy(v.as_mut_ptr().cast(), p.cast(), n);
    String::from_utf8(v).unwrap()
}

#[allow(dead_code)]
fn unused() -> (i32, *const u8) {
    (SIGUSR1, null())
}

#[test]
fn sym_bsd_signal() {
    alone("sym_bsd_signal", || {
        let first = s::bsd_signal(SIGUSR1, handler());
        let mut a = action(0, 0);
        s::sigaction(SIGUSR1, None, Some(&mut a));
        let bsd = a.flags & SA_RESTART != 0 && a.flags & SA_RESETHAND == 0 && a.mask[0] & (1 << (SIGUSR1 - 1)) != 0;
        s::raise(SIGUSR1);
        s::raise(SIGUSR1);
        let stays = HITS.load(Ordering::SeqCst) == 2;
        errno::set_raw(0);
        let bad = s::bsd_signal(SIGKILL, handler()) == SIG_ERR && errno::get() == EINVAL && s::bsd_signal(SIGUSR1, SIG_ERR) == SIG_ERR;
        first == 0 && bsd && stays && bad && s::bsd_signal(SIGUSR1, SIG_IGN) == handler()
    });
}

#[test]
fn sym_gsignal() {
    alone("sym_gsignal", || {
        s::signal(SIGUSR2, handler());
        let me = CSys.gettid();
        let ok = s::gsignal(SIGUSR2) == 0;
        let handled_here = HITS.load(Ordering::SeqCst) == 1 && CSys.gettid() == me;
        errno::set_raw(0);
        ok && handled_here && fails(s::gsignal(999) as isize, EINVAL)
    });
}

#[test]
fn sym_kill() {
    alone("sym_kill", || {
        s::signal(SIGUSR1, handler());
        let me = std::process::id() as i32;
        let sent = s::kill(me, SIGUSR1) == 0;
        let probe = s::kill(me, 0) == 0;
        wait_hits(1);
        errno::set_raw(0);
        sent && probe && HITS.load(Ordering::SeqCst) == 1 && fails(s::kill(i32::MAX - 1, 0) as isize, ESRCH) && fails(s::kill(me, 99) as isize, EINVAL)
    });
}

#[test]
fn sym_killpg() {
    alone("sym_killpg", || {
        s::signal(SIGUSR1, handler());
        let pgrp = CSys.getpgid(0).unwrap();
        let ok = s::killpg(pgrp, 0) == 0;
        errno::set_raw(0);
        ok && fails(s::killpg(-1, 0) as isize, EINVAL)
    });
}

#[test]
fn sym___libc_allocate_rtsig() {
    alone("sym___libc_allocate_rtsig", || {
        let lo = s::__libc_allocate_rtsig(1);
        let hi = s::__libc_allocate_rtsig(0);
        lo == 33 && hi == 64 && s::__libc_current_sigrtmin() == 34 && s::__libc_current_sigrtmax() == 63
    });
}

#[test]
fn sym___libc_current_sigrtmax() {
    assert_eq!(s::__libc_current_sigrtmax(), 64);
}

#[test]
fn sym___libc_current_sigrtmin() {
    assert_eq!(s::__libc_current_sigrtmin(), 33, "D-20.1: one internal signal (32)");
}

#[test]
fn sym_pause() {
    alone("sym_pause", || {
        s::signal(SIGALRM, handler());
        let (pid, tid) = (CSys.getpid(), CSys.gettid());
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let _ = CSys.tgkill(pid, tid, SIGALRM);
        });
        errno::set_raw(0);
        s::pause() == -1 && errno::get() == EINTR && HITS.load(Ordering::SeqCst) == 1
    });
}

#[test]
fn sym_psiginfo() {
    let cases = [(11, 1, 0x1000u64, 0u64), (4, 2, 0, 0), (8, 6, 0x10, 0), (10, 0, (1234u64) | (1000u64 << 32), 0), (34, -1, 99 | (5u64 << 32), 0), (60, -6, 1 | (2u64 << 32), 0), (17, 1, 4321 | (1000u64 << 32), 7), (29, 1, 3, 0), (10, 77, 0, 0), (2, -60, 0, 0)];
    let mut calls = String::new();
    for (sig, code, w0, w1) in cases {
        calls += &format!("memset(&i, 0, sizeof i); i.si_signo = {sig}; i.si_code = {code}; ((unsigned long *)&i)[2] = {w0}UL; ((unsigned long *)&i)[3] = {w1}UL; psiginfo(&i, \"LIELLA\");\n");
    }
    let want: String = host_stderr("psiginfo", &calls).lines().filter(|l| l.contains("LIELLA")).map(|l| format!("{l}\n")).collect();
    let got = own_stderr("sym_psiginfo", || {
        for (sig, code, w0, w1) in cases {
            let mut i = SigInfo::default();
            // glibc's SIGRTMIN is 34, Liella's 33 (D-20.1): the same name, one number lower.
            i.signo = if (34..60).contains(&sig) { sig - 1 } else { sig };
            i.code = code;
            i.fields[0] = w0;
            i.fields[1] = w1;
            s::psiginfo(Some(&i), c"LIELLA".as_ptr());
        }
    });
    assert_eq!(got, want);
}

#[test]
fn sym_psignal() {
    let calls = "psignal(SIGINT, \"LIELLA\"); psignal(SIGSEGV, \"LIELLA a\"); psignal(40, \"LIELLA b\"); psignal(SIGTERM, \"\"); fputs(\"LIELLA end\\n\", stderr);";
    let want: String = host_stderr("psignal", calls).lines().filter(|l| l.contains("LIELLA")).map(|l| format!("{l}\n")).collect();
    let got = own_stderr("sym_psignal", || {
        s::psignal(SIGINT, c"LIELLA".as_ptr());
        s::psignal(11, c"LIELLA a".as_ptr());
        s::psignal(40, c"LIELLA b".as_ptr());
        s::psignal(SIGTERM, c"".as_ptr());
        liella_boundary::process::stderr(b"LIELLA end\n");
    });
    assert_eq!(got, want);
}

#[test]
fn sym_raise() {
    alone("sym_raise", || {
        s::signal(SIGUSR2, handler());
        let me = CSys.gettid();
        let ok = s::raise(SIGUSR2) == 0;
        let handled_here = HITS.load(Ordering::SeqCst) == 1 && CSys.gettid() == me;
        errno::set_raw(0);
        ok && handled_here && fails(s::raise(999) as isize, EINVAL)
    });
}

#[test]
fn sym_sigabbrev_np() {
    assert_eq!(unsafe_cstr(s::sigabbrev_np(SIGTERM).cast_mut()), "TERM");
    assert_eq!(unsafe_cstr(s::sigabbrev_np(29).cast_mut()), "IO");
    assert!(s::sigabbrev_np(40).is_null());
}

#[test]
fn sym___sigaction() {
    alone("sym___sigaction", || {
        let a = action(with_info as extern "C" fn(i32, Option<&SigInfo>, *mut core::ffi::c_void) as usize, SA_SIGINFO);
        let mut old = action(9, 9);
        let installed = s::__sigaction(SIGUSR1, Some(&a), Some(&mut old)) == 0 && old.handler == 0;
        s::raise(SIGUSR1);
        let delivered = HITS.load(Ordering::SeqCst) == 1 && INFO_PID.load(Ordering::SeqCst) == std::process::id() as i32;
        let mut now = action(0, 0);
        s::__sigaction(SIGUSR1, None, Some(&mut now));
        let reported = now.handler == a.handler && now.flags & SA_SIGINFO != 0 && now.flags & 0x0400_0000 != 0 && now.restorer != 0;
        errno::set_raw(0);
        let refused = fails(s::__sigaction(SIGKILL, Some(&a), None) as isize, EINVAL) && fails(s::__sigaction(32, Some(&a), None) as isize, EINVAL) && fails(s::__sigaction(65, None, Some(&mut now)) as isize, EINVAL);
        installed && delivered && reported && refused
    });
}

#[test]
fn sym_sigaction() {
    alone("sym_sigaction", || {
        let a = action(with_info as extern "C" fn(i32, Option<&SigInfo>, *mut core::ffi::c_void) as usize, SA_SIGINFO);
        let mut old = action(9, 9);
        let installed = s::sigaction(SIGUSR1, Some(&a), Some(&mut old)) == 0 && old.handler == 0;
        s::raise(SIGUSR1);
        let delivered = HITS.load(Ordering::SeqCst) == 1 && INFO_PID.load(Ordering::SeqCst) == std::process::id() as i32;
        let mut now = action(0, 0);
        s::sigaction(SIGUSR1, None, Some(&mut now));
        let reported = now.handler == a.handler && now.flags & SA_SIGINFO != 0 && now.flags & 0x0400_0000 != 0 && now.restorer != 0;
        errno::set_raw(0);
        let refused = fails(s::sigaction(SIGKILL, Some(&a), None) as isize, EINVAL) && fails(s::sigaction(32, Some(&a), None) as isize, EINVAL) && fails(s::sigaction(65, None, Some(&mut now)) as isize, EINVAL);
        installed && delivered && reported && refused
    });
}

#[test]
fn sym___sigaddset() {
    let mut set = empty();
    assert_eq!(s::__sigaddset(Some(&mut set), 32), 0, "no internal-signal check in the inline helper");
    assert_eq!(set.__val[0], 1 << 31);
    assert_eq!(s::__sigdelset(Some(&mut set), 32), 0);
    assert_eq!(set.__val[0], 0);
    assert_eq!(s::__sigaddset(Some(&mut set), 5000), 0);
}

#[test]
fn sym_sigaddset() {
    let mut set = empty();
    assert_eq!(s::sigaddset(Some(&mut set), SIGINT), 0);
    assert_eq!(set.__val[0], 1 << 1);
    errno::set_raw(0);
    assert!(fails(s::sigaddset(Some(&mut set), 32) as isize, EINVAL), "the internal signal");
    assert!(fails(s::sigaddset(Some(&mut set), 0) as isize, EINVAL));
    assert!(fails(s::sigaddset(Some(&mut set), 65) as isize, EINVAL));
}

#[test]
fn sym_sigaltstack() {
    alone("sym_sigaltstack", || {
        let mut mem = vec![0u8; 65536];
        let mut ss = StackT::default();
        ss.sp = mem.as_mut_ptr() as usize;
        ss.size = mem.len();
        let mut old = StackT::default();
        let set = s::sigaltstack(Some(&ss), Some(&mut old)) == 0; // (the Rust runtime installs its own first)
        let mut now = StackT::default();
        s::sigaltstack(None, Some(&mut now));
        let mut tiny = StackT::default();
        tiny.sp = mem.as_mut_ptr() as usize;
        tiny.size = 16;
        errno::set_raw(0);
        let small = fails(s::sigaltstack(Some(&tiny), None) as isize, 12); // ENOMEM: below MINSIGSTKSZ
        let mut off = StackT::default();
        off.flags = 2;
        s::sigaltstack(Some(&off), None);
        set && now.sp == ss.sp && now.size == ss.size && small
    });
}

#[test]
fn sym_sigandset() {
    let mut d = empty();
    assert_eq!(s::sigandset(Some(&mut d), Some(&set_of(&[1, 2, 3])), Some(&set_of(&[2, 3, 4]))), 0);
    assert_eq!(d.__val[0], 0b110);
}

#[test]
fn sym_sigblock() {
    alone("sym_sigblock", || {
        let before = s::siggetmask();
        let old = s::sigblock(1 << (SIGUSR1 - 1));
        let now = s::siggetmask();
        old == before && now & (1 << (SIGUSR1 - 1)) != 0 && s::sigblock(1 << 31) == now && s::siggetmask() & (1 << 31) == 0
    });
}

#[test]
fn sym___sigdelset() {
    let mut set = empty();
    assert_eq!(s::__sigaddset(Some(&mut set), 32), 0, "no internal-signal check in the inline helper");
    assert_eq!(set.__val[0], 1 << 31);
    assert_eq!(s::__sigdelset(Some(&mut set), 32), 0);
    assert_eq!(set.__val[0], 0);
    assert_eq!(s::__sigdelset(Some(&mut set), 5000), 0);
}

#[test]
fn sym_sigdelset() {
    let mut set = set_of(&[SIGINT, SIGTERM]);
    assert_eq!(s::sigdelset(Some(&mut set), SIGINT), 0);
    assert_eq!(set.__val[0], 1 << (SIGTERM - 1));
    errno::set_raw(0);
    assert!(fails(s::sigdelset(Some(&mut set), 32) as isize, EINVAL));
}

#[test]
fn sym_sigdescr_np() {
    assert_eq!(unsafe_cstr(s::sigdescr_np(SIGHUP).cast_mut()), "Hangup");
    assert_eq!(unsafe_cstr(s::sigdescr_np(31).cast_mut()), "Bad system call");
    assert!(s::sigdescr_np(33).is_null() && s::sigdescr_np(0).is_null());
}

#[test]
fn sym_sigemptyset() {
    let mut set = sigset_t { __val: [u64::MAX; 16] };
    assert_eq!(s::sigemptyset(Some(&mut set)), 0);
    assert_eq!(set.__val, [0; 16], "all 1024 bits");
    errno::set_raw(0);
    assert!(fails(s::sigemptyset(None) as isize, EFAULT));
}

#[test]
fn sym_sigfillset() {
    let mut set = empty();
    assert_eq!(s::sigfillset(Some(&mut set)), 0);
    assert_eq!(s::sigismember(Some(&set), 32), 0, "the internal signal is never in a filled set");
    assert_eq!(s::sigismember(Some(&set), 33), 1);
    assert!(set.__val[1..].iter().all(|&w| w == u64::MAX), "bits beyond 64 too");
}

#[test]
fn sym_siggetmask() {
    alone("sym_siggetmask", || {
        s::sigprocmask(SIG_SETMASK, Some(&set_of(&[SIGHUP, SIGALRM])), None);
        s::siggetmask() == (1 << 0) | (1 << 13)
    });
}

#[test]
fn sym_sighold() {
    alone("sym_sighold", || {
        s::sighold(SIGUSR2);
        let mut m = empty();
        s::sigprocmask(SIG_BLOCK, None, Some(&mut m));
        let blocked = s::sigismember(Some(&m), SIGUSR2) == 1;
        s::sigrelse(SIGUSR2);
        s::sigprocmask(SIG_BLOCK, None, Some(&mut m));
        errno::set_raw(0);
        blocked && s::sigismember(Some(&m), SIGUSR2) == 0 && fails(s::sighold(32) as isize, EINVAL) && fails(s::sighold(0) as isize, EINVAL)
    });
}

#[test]
fn sym_sigignore() {
    alone("sym_sigignore", || {
        s::sigignore(SIGUSR1);
        s::raise(SIGUSR1);
        let mut a = action(0, 0);
        s::sigaction(SIGUSR1, None, Some(&mut a));
        a.handler == SIG_IGN && fails(s::sigignore(SIGKILL) as isize, EINVAL)
    });
}

#[test]
fn sym_siginterrupt() {
    alone("sym_siginterrupt", || {
        s::signal(SIGUSR1, handler());
        let off = s::siginterrupt(SIGUSR1, 1) == 0;
        let mut a = action(0, 0);
        s::sigaction(SIGUSR1, None, Some(&mut a));
        let no_restart = a.flags & SA_RESTART == 0 && a.handler == handler();
        s::signal(SIGUSR1, handler());
        s::sigaction(SIGUSR1, None, Some(&mut a));
        let remembered = a.flags & SA_RESTART == 0;
        s::siginterrupt(SIGUSR1, 0);
        s::sigaction(SIGUSR1, None, Some(&mut a));
        off && no_restart && remembered && a.flags & SA_RESTART != 0
    });
}

#[test]
fn sym_sigisemptyset() {
    assert_eq!(s::sigisemptyset(Some(&empty())), 1);
    let mut far = empty();
    far.__val[15] = 1;
    assert_eq!(s::sigisemptyset(Some(&far)), 0, "every word counts");
}

#[test]
fn sym___sigismember() {
    let set = set_of(&[SIGHUP]);
    assert_eq!(s::__sigismember(Some(&set), SIGHUP), 1);
    assert_eq!(s::__sigismember(Some(&set), 2), 0);
    assert_eq!(s::__sigismember(Some(&set), 999), 0, "out of range: nothing (glibc: undefined)");
}

#[test]
fn sym_sigismember() {
    let set = set_of(&[SIGTERM]);
    assert_eq!(s::sigismember(Some(&set), SIGTERM), 1);
    assert_eq!(s::sigismember(Some(&set), SIGINT), 0);
    assert_eq!(s::sigismember(Some(&set), 32), 0, "may be asked about the internal signal");
    errno::set_raw(0);
    assert!(fails(s::sigismember(Some(&set), 0) as isize, EINVAL));
}

#[test]
fn sym_siglongjmp() {
    exported("siglongjmp");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_signal() {
    alone("sym_signal", || {
        let first = s::signal(SIGUSR1, handler());
        let mut a = action(0, 0);
        s::sigaction(SIGUSR1, None, Some(&mut a));
        let bsd = a.flags & SA_RESTART != 0 && a.flags & SA_RESETHAND == 0 && a.mask[0] & (1 << (SIGUSR1 - 1)) != 0;
        s::raise(SIGUSR1);
        s::raise(SIGUSR1);
        let stays = HITS.load(Ordering::SeqCst) == 2;
        errno::set_raw(0);
        let bad = s::signal(SIGKILL, handler()) == SIG_ERR && errno::get() == EINVAL && s::signal(SIGUSR1, SIG_ERR) == SIG_ERR;
        first == 0 && bsd && stays && bad && s::signal(SIGUSR1, SIG_IGN) == handler()
    });
}

#[test]
fn sym_signalfd() {
    alone("sym_signalfd", || {
        let set = set_of(&[SIGUSR1]);
        s::sigprocmask(SIG_BLOCK, Some(&set), None);
        let fd = s::signalfd(-1, Some(&set), 0o2000000);
        s::raise(SIGUSR1);
        let mut rec = [0u32; 32];
        let n = liella_boundary::posix::s11_lowio::read(fd, rec.as_mut_ptr().cast(), 128);
        errno::set_raw(0);
        fd >= 0 && n == 128 && rec[0] == SIGUSR1 as u32 && fails(s::signalfd(-1, None, 0) as isize, EFAULT)
    });
}

#[test]
fn sym_sigorset() {
    let mut d = empty();
    assert_eq!(s::sigorset(Some(&mut d), Some(&set_of(&[1])), Some(&set_of(&[64]))), 0);
    assert_eq!(d.__val[0], 1 | (1 << 63));
}

#[test]
fn sym___sigpause() {
    alone("sym___sigpause", || {
        s::signal(SIGUSR1, handler());
        s::sigprocmask(SIG_BLOCK, Some(&set_of(&[SIGUSR1])), None);
        let (pid, tid) = (CSys.getpid(), CSys.gettid());
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let _ = CSys.tgkill(pid, tid, SIGUSR1);
        });
        errno::set_raw(0);
        let r = s::__sigpause(1 << (SIGUSR2 - 1), 0);
        r == -1 && errno::get() == EINTR && HITS.load(Ordering::SeqCst) == 1
    });
}

#[test]
fn sym_sigpause() {
    alone("sym_sigpause", || {
        s::signal(SIGUSR1, handler());
        s::sigprocmask(SIG_BLOCK, Some(&set_of(&[SIGUSR1])), None);
        let (pid, tid) = (CSys.getpid(), CSys.gettid());
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let _ = CSys.tgkill(pid, tid, SIGUSR1);
        });
        errno::set_raw(0);
        let r = s::sigpause(1 << (SIGUSR2 - 1));
        r == -1 && errno::get() == EINTR && HITS.load(Ordering::SeqCst) == 1
    });
}

#[test]
fn sym_sigpending() {
    alone("sym_sigpending", || {
        s::sigprocmask(SIG_BLOCK, Some(&set_of(&[SIGUSR1, SIGUSR2])), None);
        s::raise(SIGUSR2);
        let mut p = empty();
        s::sigpending(Some(&mut p)) == 0 && s::sigismember(Some(&p), SIGUSR2) == 1 && s::sigismember(Some(&p), SIGUSR1) == 0
    });
}

#[test]
fn sym_sigprocmask() {
    alone("sym_sigprocmask", || {
        let all = { let mut f = empty(); s::sigfillset(Some(&mut f)); f };
        let mut old = empty();
        s::sigprocmask(SIG_SETMASK, Some(&all), Some(&mut old));
        let mut now = empty();
        s::sigprocmask(SIG_BLOCK, None, Some(&mut now));
        let killstop = s::sigismember(Some(&now), SIGKILL) == 0 && s::sigismember(Some(&now), SIGSTOP) == 0;
        let internal = s::sigismember(Some(&now), 32) == 0;
        let others = s::sigismember(Some(&now), SIGUSR1) == 1 && s::sigismember(Some(&now), 64) == 1;
        s::sigprocmask(SIG_UNBLOCK, Some(&set_of(&[SIGUSR1])), None);
        s::sigprocmask(SIG_BLOCK, None, Some(&mut now));
        errno::set_raw(0);
        let bad = fails(s::sigprocmask(7, Some(&all), None) as isize, EINVAL);
        killstop && internal && others && s::sigismember(Some(&now), SIGUSR1) == 0 && bad
    });
}

#[test]
fn sym_sigqueue() {
    alone("sym_sigqueue", || {
        let a = action(with_info as extern "C" fn(i32, Option<&SigInfo>, *mut core::ffi::c_void) as usize, SA_SIGINFO);
        s::sigaction(40, Some(&a), None);
        let ok = s::sigqueue(std::process::id() as i32, 40, 1234) == 0;
        wait_hits(1);
        ok && HITS.load(Ordering::SeqCst) == 1 && INFO_VALUE.load(Ordering::SeqCst) == 1234 && LAST.load(Ordering::SeqCst) == 40
    });
}

#[test]
fn sym_sigrelse() {
    alone("sym_sigrelse", || {
        s::sighold(SIGUSR2);
        let mut m = empty();
        s::sigprocmask(SIG_BLOCK, None, Some(&mut m));
        let blocked = s::sigismember(Some(&m), SIGUSR2) == 1;
        s::sigrelse(SIGUSR2);
        s::sigprocmask(SIG_BLOCK, None, Some(&mut m));
        errno::set_raw(0);
        blocked && s::sigismember(Some(&m), SIGUSR2) == 0 && fails(s::sigrelse(32) as isize, EINVAL) && fails(s::sigrelse(0) as isize, EINVAL)
    });
}

#[test]
fn sym_sigreturn() {
    errno::set_raw(0);
    assert!(fails(s::sigreturn(null_mut()) as isize, ENOSYS));
}

#[test]
fn sym_sigset() {
    alone("sym_sigset", || {
        let a = s::sigset(SIGUSR1, handler()) == 0;
        let held = s::sigset(SIGUSR1, SIG_HOLD) == handler();
        let again = s::sigset(SIGUSR1, SIG_HOLD) == SIG_HOLD;
        s::raise(SIGUSR1);
        let pending = HITS.load(Ordering::SeqCst) == 0;
        let back = s::sigset(SIGUSR1, handler()) == SIG_HOLD;
        a && held && again && pending && back && HITS.load(Ordering::SeqCst) == 1
    });
}

#[test]
fn sym___sigsetjmp() {
    exported("__sigsetjmp");
    conformance(&["s21_setjmp", "s22_cancel"]);
}
#[test]
fn sym_sigsetmask() {
    alone("sym_sigsetmask", || {
        let old = s::sigsetmask(1 << (SIGUSR2 - 1));
        let now = s::sigsetmask(old);
        now == 1 << (SIGUSR2 - 1) && s::siggetmask() == old
    });
}

#[test]
fn sym_sigstack() {
    let mut o = CSigstack::default();
    assert_eq!(s::sigstack(None, Some(&mut o)), 0);
    assert_eq!(o.onstack, 0);
    errno::set_raw(0);
    assert!(fails(s::sigstack(Some(&o), None) as isize, ENOSYS), "setting is not possible: the size is unknown");
}

#[test]
fn sym___sigsuspend() {
    alone("sym___sigsuspend", || {
        s::signal(SIGUSR1, handler());
        s::sigprocmask(SIG_BLOCK, Some(&set_of(&[SIGUSR1])), None);
        s::raise(SIGUSR1);
        let pending = HITS.load(Ordering::SeqCst) == 0;
        errno::set_raw(0);
        let r = s::__sigsuspend(Some(&empty()));
        let mut m = empty();
        s::sigprocmask(SIG_BLOCK, None, Some(&mut m));
        pending && r == -1 && errno::get() == EINTR && HITS.load(Ordering::SeqCst) == 1 && s::sigismember(Some(&m), SIGUSR1) == 1
    });
}

#[test]
fn sym_sigsuspend() {
    alone("sym_sigsuspend", || {
        s::signal(SIGUSR1, handler());
        s::sigprocmask(SIG_BLOCK, Some(&set_of(&[SIGUSR1])), None);
        s::raise(SIGUSR1);
        let pending = HITS.load(Ordering::SeqCst) == 0;
        errno::set_raw(0);
        let r = s::sigsuspend(Some(&empty()));
        let mut m = empty();
        s::sigprocmask(SIG_BLOCK, None, Some(&mut m));
        pending && r == -1 && errno::get() == EINTR && HITS.load(Ordering::SeqCst) == 1 && s::sigismember(Some(&m), SIGUSR1) == 1
    });
}

#[test]
fn sym_sigtimedwait() {
    alone("sym_sigtimedwait", || {
        let set = set_of(&[SIGUSR2]);
        s::sigprocmask(SIG_BLOCK, Some(&set), None);
        errno::set_raw(0);
        let r = s::sigtimedwait(Some(&set), None, Some(&Timespec { sec: 0, nsec: 10_000_000 }));
        r == -1 && errno::get() == EAGAIN
    });
}

#[test]
fn sym_sigvec() {
    alone("sym_sigvec", || {
        let v = CSigvec { handler: handler(), mask: 1 << (SIGHUP - 1), flags: 2 }; // SV_INTERRUPT
        let mut old = CSigvec::default();
        let ok = s::sigvec(SIGUSR1, Some(&v), Some(&mut old)) == 0 && old.handler == 0;
        let mut a = action(0, 0);
        s::sigaction(SIGUSR1, None, Some(&mut a));
        let mapped = a.flags & SA_RESTART == 0 && a.mask[0] & 1 != 0;
        let mut back = CSigvec::default();
        s::sigvec(SIGUSR1, None, Some(&mut back));
        ok && mapped && back.handler == handler() && back.flags & 2 != 0 && back.mask & 1 != 0
    });
}

#[test]
fn sym_sigwait() {
    alone("sym_sigwait", || {
        let set = set_of(&[SIGUSR1]);
        s::sigprocmask(SIG_BLOCK, Some(&set), None);
        s::raise(SIGUSR1);
        let mut got = 0;
        errno::set_raw(0);
        s::sigwait(Some(&set), Some(&mut got)) == 0 && got == SIGUSR1 && errno::get() == 0
    });
}

#[test]
fn sym_sigwaitinfo() {
    alone("sym_sigwaitinfo", || {
        // Thread-directed (a process-directed signal could go to a thread not blocking it).
        let set = set_of(&[SIGUSR2]);
        s::sigprocmask(SIG_BLOCK, Some(&set), None);
        s::raise(SIGUSR2);
        let mut info = SigInfo::default();
        s::sigwaitinfo(Some(&set), Some(&mut info)) == SIGUSR2 && info.code == -6 && info.pid() == std::process::id() as i32
    });
}

#[test]
fn sym_ssignal() {
    alone("sym_ssignal", || {
        let first = s::ssignal(SIGUSR1, handler());
        let mut a = action(0, 0);
        s::sigaction(SIGUSR1, None, Some(&mut a));
        let bsd = a.flags & SA_RESTART != 0 && a.flags & SA_RESETHAND == 0 && a.mask[0] & (1 << (SIGUSR1 - 1)) != 0;
        s::raise(SIGUSR1);
        s::raise(SIGUSR1);
        let stays = HITS.load(Ordering::SeqCst) == 2;
        errno::set_raw(0);
        let bad = s::ssignal(SIGKILL, handler()) == SIG_ERR && errno::get() == EINVAL && s::ssignal(SIGUSR1, SIG_ERR) == SIG_ERR;
        first == 0 && bsd && stays && bad && s::ssignal(SIGUSR1, SIG_IGN) == handler()
    });
}

#[test]
fn sym_strsignal() {
    let text = |sig: i32| unsafe_cstr(s::strsignal(sig));
    assert_eq!(text(SIGINT), "Interrupt");
    assert_eq!(text(SIGKILL), "Killed");
    assert_eq!(text(33), "Real-time signal 0");
    assert_eq!(text(64), "Real-time signal 31");
    assert_eq!(text(32), "Unknown signal 32", "the internal signal is not a real-time signal for applications");
    assert_eq!(text(100), "Unknown signal 100");
}

#[test]
fn sym_sys_sigabbrev() {
    let table = &liella_boundary::data::sys_sigabbrev;
    assert_eq!(table.len(), 65, "the 2.3.3 objects: 65 entries");
    assert_eq!(unsafe_cstr(table[9].load(Ordering::Relaxed).cast()), "KILL");
    assert!(table[0].load(Ordering::Relaxed).is_null() && table[40].load(Ordering::Relaxed).is_null(), "NULL where there is no text");
}

#[test]
fn sym__sys_siglist() {
    let table = &liella_boundary::data::_sys_siglist;
    assert_eq!(table.len(), 65, "the 2.3.3 objects: 65 entries");
    assert_eq!(unsafe_cstr(table[15].load(Ordering::Relaxed).cast()), "Terminated");
    assert!(table[0].load(Ordering::Relaxed).is_null() && table[40].load(Ordering::Relaxed).is_null(), "NULL where there is no text");
}

#[test]
fn sym_sys_siglist() {
    let table = &liella_boundary::data::sys_siglist;
    assert_eq!(table.len(), 65, "the 2.3.3 objects: 65 entries");
    assert_eq!(unsafe_cstr(table[2].load(Ordering::Relaxed).cast()), "Interrupt");
    assert!(table[0].load(Ordering::Relaxed).is_null() && table[40].load(Ordering::Relaxed).is_null(), "NULL where there is no text");
}

#[test]
fn sym___sysv_signal() {
    alone("sym___sysv_signal", || {
        s::__sysv_signal(SIGUSR1, handler());
        let mut a = action(0, 0);
        s::sigaction(SIGUSR1, None, Some(&mut a));
        let svr4 = a.flags & SA_RESETHAND != 0 && a.flags & SA_RESTART == 0 && a.mask[0] == 0;
        s::raise(SIGUSR1);
        let mut after = action(9, 0);
        s::sigaction(SIGUSR1, None, Some(&mut after));
        svr4 && HITS.load(Ordering::SeqCst) == 1 && after.handler == 0
    });
}

#[test]
fn sym_sysv_signal() {
    alone("sym_sysv_signal", || {
        s::sysv_signal(SIGUSR1, handler());
        let mut a = action(0, 0);
        s::sigaction(SIGUSR1, None, Some(&mut a));
        let svr4 = a.flags & SA_RESETHAND != 0 && a.flags & SA_RESTART == 0 && a.mask[0] == 0;
        s::raise(SIGUSR1);
        let mut after = action(9, 0);
        s::sigaction(SIGUSR1, None, Some(&mut after));
        svr4 && HITS.load(Ordering::SeqCst) == 1 && after.handler == 0
    });
}

#[test]
fn sym_tgkill() {
    alone("sym_tgkill", || {
        s::signal(SIGUSR2, handler());
        let ok = s::tgkill(CSys.getpid(), CSys.gettid(), SIGUSR2) == 0;
        errno::set_raw(0);
        ok && HITS.load(Ordering::SeqCst) == 1 && fails(s::tgkill(CSys.getpid(), i32::MAX - 1, 0) as isize, ESRCH)
    });
}

#[test]
fn sym___xpg_sigpause() {
    alone("sym___xpg_sigpause", || {
        s::signal(SIGUSR1, handler());
        s::sigprocmask(SIG_BLOCK, Some(&set_of(&[SIGUSR1])), None);
        let (pid, tid) = (CSys.getpid(), CSys.gettid());
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(50));
            let _ = CSys.tgkill(pid, tid, SIGUSR1);
        });
        errno::set_raw(0);
        let r = s::__xpg_sigpause(SIGUSR1);
        r == -1 && errno::get() == EINTR && HITS.load(Ordering::SeqCst) == 1
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
