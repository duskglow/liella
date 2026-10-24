// Specified from spec §19 in phase 11 (`login_tty` and `getpass` were phase 10).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §19 (25 clauses). Every test works on private ptys it creates. What
//! needs a controlling terminal (a new session) or process-wide state runs alone in a
//! re-executed copy of this test binary (`alone`), whose own children (background jobs, an
//! orphaned group) are forked with `selftest::spawn_child`, each with an alarm, and reaped (the
//! orphan by whoever adopts it, as orphans are). `forkpty` and the cancellation points need a
//! process Liella started: they run in the conformance program `s19_terminals`. The cases that
//! need a private devpts instance, `/proc` hidden or a fixture `/etc/ttys` run in the
//! differential test's mount namespace (`terminals_products.rs`); here `ENODEV` is reached
//! through a kernel with injected faults. The whole surface is also compared with the
//! reference by `terminals_products.rs`.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::unwrap_used, clippy::panic, clippy::as_conversions, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]

#[path = "support/scratch.rs"]
mod scratch;

use core::ptr::null_mut;
use std::ffi::CString;
use std::sync::atomic::{AtomicI32, Ordering};

use liella::error::Errno;
use liella::term::{self, TtyLine, When};
use liella_abi::ctypes::c_char;
use liella_abi::layout::{struct_termios, struct_winsize};
use liella_boundary::errno;
use liella_boundary::posix::s04_strings as st;
use liella_boundary::posix::s17_users as us;
use liella_boundary::posix::s19_terminals as x;
use liella_boundary::selftest;
use liella_boundary::sys::CSys;
use liella_kernel::ops::{FcntlInt, IoctlIntOut};
use liella_kernel::path::KStr;
use liella_kernel::sys::{Disposition, Sys};
use liella_kernel::testing::Faulty;
use liella_kernel::uapi::{PollFd, SigSet, Timespec, O_CLOEXEC, O_NOCTTY, O_NONBLOCK, O_RDWR, WUNTRACED};
use liella_kernel::Fd;
use liella_legacy::cursors::{Cursor, Step};

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

// ------------------------------------------------------------------------------ helpers

const ENOENT: i32 = 2;
const ESRCH: i32 = 3;
const EIO: i32 = 5;
const EBADF: i32 = 9;
const EFAULT: i32 = 14;
const EINVAL: i32 = 22;
const ENOTTY: i32 = 25;
const ERANGE: i32 = 34;
const ENOSYS: i32 = 38;

const SIGHUP: i32 = 1;
const SIGKILL: i32 = 9;
const SIGTTOU: i32 = 22;
const SIG_BLOCK: u32 = 0;

const TCSANOW: i32 = 0;
const TCSADRAIN: i32 = 1;
const TCSAFLUSH: i32 = 2;
const TCIFLUSH: i32 = 0;
const TCOFLUSH: i32 = 1;
const TCIOFLUSH: i32 = 2;
const TCOOFF: i32 = 0;
const TCOON: i32 = 1;
const TCIOFF: i32 = 2;
const TCION: i32 = 3;

// Mode bits and control character indices (⚑ KERN; the installed <termios.h>).
const ICRNL: u32 = 0o400;
const IXOFF: u32 = 0o10000;
const OPOST: u32 = 0o1;
const CSIZE: u32 = 0o60;
const CS7: u32 = 0o40;
const CS8: u32 = 0o60;
const CSTOPB: u32 = 0o100;
const CREAD: u32 = 0o200;
const PARENB: u32 = 0o400;
const ISIG: u32 = 0o1;
const ICANON: u32 = 0o2;
const ECHO: u32 = 0o10;
const CBAUD: u32 = 0o10017;
const BOTHER: u32 = 0o10000;
const VINTR: usize = 0;
const VEOF: usize = 4;
const VTIME: usize = 5;
const VMIN: usize = 6;
const VSTART: usize = 8;
const VSTOP: usize = 9;
const VEOL2: usize = 16;

/// The 31 speed codes `B0` .. `B4000000` with their rates.
const CODES: [(u32, u32); 31] = [
    (0, 0),
    (1, 50),
    (2, 75),
    (3, 110),
    (4, 134),
    (5, 150),
    (6, 200),
    (7, 300),
    (8, 600),
    (9, 1200),
    (10, 1800),
    (11, 2400),
    (12, 4800),
    (13, 9600),
    (14, 19200),
    (15, 38400),
    (0o10001, 57600),
    (0o10002, 115_200),
    (0o10003, 230_400),
    (0o10004, 460_800),
    (0o10005, 500_000),
    (0o10006, 576_000),
    (0o10007, 921_600),
    (0o10010, 1_000_000),
    (0o10011, 1_152_000),
    (0o10012, 1_500_000),
    (0o10013, 2_000_000),
    (0o10014, 2_500_000),
    (0o10015, 3_000_000),
    (0o10016, 3_500_000),
    (0o10017, 4_000_000),
];
const B2400: u32 = 11;
const B9600: u32 = 13;
const B19200: u32 = 14;
const B115200: u32 = 0o10002;

/// An all-zero C `struct termios`.
fn zt() -> struct_termios {
    struct_termios { c_iflag: 0, c_oflag: 0, c_cflag: 0, c_lflag: 0, c_line: 0, c_cc: [0; 32], c_ispeed: 0, c_ospeed: 0 }
}

/// What a caller can compare of a structure: the mode words, the 17 named control characters
/// and the two speeds as the getters report them.
fn seen(t: &struct_termios) -> (u32, u32, u32, u32, [u8; 17], u32, u32) {
    let mut cc = [0u8; 17];
    cc.copy_from_slice(&t.c_cc[..17]);
    (t.c_iflag, t.c_oflag, t.c_cflag, t.c_lflag, cc, x::cfgetospeed(Some(t)), x::cfgetispeed(Some(t)))
}

/// The settings of `fd` through `tcgetattr`.
fn attrs(fd: i32) -> struct_termios {
    let mut t = zt();
    assert_eq!(x::tcgetattr(fd, Some(&mut t)), 0, "tcgetattr({fd})");
    t
}

/// A result and the `errno` after it.
fn fails(r: i64) -> (i64, i32) {
    (r, errno::get())
}

/// A private pty pair (`openpty`), closed when dropped.
struct Pair {
    m: i32,
    s: i32,
}

impl Pair {
    fn new() -> Pair {
        let (mut m, mut s) = (-1, -1);
        assert_eq!(x::openpty(Some(&mut m), Some(&mut s), null_mut(), None, None), 0);
        Pair { m, s }
    }

    /// The slave's path, from `ptsname_r`.
    fn name(&self) -> String {
        let mut b = [0 as c_char; 64];
        assert_eq!(x::ptsname_r(self.m, b.as_mut_ptr(), b.len()), 0);
        stored(&b)
    }
}

impl Drop for Pair {
    fn drop(&mut self) {
        let _ = CSys.close(Fd::new(self.s));
        let _ = CSys.close(Fd::new(self.m));
    }
}

/// Held by the tests that call `ptsname`: its result is one process-wide buffer, which tests
/// running in parallel would otherwise overwrite between the call and the read.
static PTSNAME: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The slave's path for master `m`, from `ptsname_r`.
fn slave_name(m: i32) -> String {
    let mut b = [0 as c_char; 64];
    assert_eq!(x::ptsname_r(m, b.as_mut_ptr(), b.len()), 0);
    stored(&b)
}

/// The text stored in a C buffer, up to its terminator.
fn stored(b: &[c_char]) -> String {
    String::from_utf8(b.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect()).unwrap()
}

/// The C text at `p`.
fn text_at(p: *const c_char) -> String {
    assert!(!p.is_null());
    let n = st::strlen(p);
    let mut v = vec![0u8; n];
    st::memcpy(v.as_mut_ptr().cast(), p.cast(), n);
    String::from_utf8(v).unwrap()
}

/// Whether `fd` has input within `ms` milliseconds.
fn readable(fd: i32, ms: i64) -> bool {
    let mut p = [PollFd { fd, events: 1, revents: 0 }];
    let mut t = Timespec { sec: ms / 1000, nsec: (ms % 1000) * 1_000_000 };
    CSys.ppoll(&mut p, Some(&mut t), None).unwrap_or(0) > 0 && p[0].revents & 1 != 0
}

/// Everything readable on `fd` (waiting up to `ms` for the first byte).
fn take(fd: i32, ms: i64) -> Vec<u8> {
    let mut out = Vec::new();
    let mut wait = ms;
    while readable(fd, wait) {
        let mut b = [0u8; 256];
        match CSys.read(Fd::new(fd), &mut b) {
            Ok(n) if n > 0 => out.extend_from_slice(&b[..n]),
            _ => break,
        }
        wait = 50;
    }
    out
}

fn write(fd: i32, b: &[u8]) {
    assert_eq!(CSys.write(Fd::new(fd), b).unwrap(), b.len());
}

/// Raw mode on `fd`.
fn raw(fd: i32) {
    let mut t = attrs(fd);
    x::cfmakeraw(Some(&mut t));
    assert_eq!(x::tcsetattr(fd, TCSANOW, Some(&t)), 0);
}

fn open(path: &str, flags: u32) -> Result<i32, Errno> {
    let c = CString::new(path).unwrap();
    CSys.openat(Fd::CWD, KStr::from_nul_terminated(c.as_bytes_with_nul()).unwrap(), flags, 0).map(Fd::raw)
}

/// Set in a re-executed copy: the case it runs.
const ALONE: &str = "LIELLA_C19_ALONE";

/// Run `body` alone in a re-executed copy of this test binary (test `test`, case `case`); the
/// copy exits 0 when `body` returns true, and the parent asserts that it did. In a copy made for
/// another case, `body` does not run.
fn alone(test: &str, case: &str, body: impl FnOnce() -> bool) {
    match std::env::var(ALONE) {
        Ok(v) if v == case => {
            // A copy never outlives a stuck case; closing the master of its own controlling
            // terminal (when its pair is dropped) hangs it up, which must not end the copy.
            let _ = CSys.alarm(60);
            let _ = CSys.sigaction_set(SIGHUP, Disposition::Ignore, 0, SigSet::empty());
            let ok = body();
            CSys.exit_group(if ok { 0 } else { 1 })
        }
        Ok(_) => return,
        Err(_) => {}
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap()).args([test, "--exact", "--test-threads=1", "--nocapture"]).env(ALONE, case).output().unwrap();
    assert!(o.status.success(), "case {case}: {:?}\n{}{}", o.status, String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

/// In an `alone` copy: start a new session and give it `p`'s slave as its controlling
/// terminal (opened by name without `O_NOCTTY`); the descriptor of that open.
fn take_terminal(p: &Pair) -> i32 {
    assert_eq!(CSys.setsid().unwrap(), CSys.getpid(), "a new session");
    let fd = open(&p.name(), O_RDWR).unwrap();
    assert!(open("/dev/tty", O_RDWR).is_ok_and(|t| CSys.close(Fd::new(t)).is_ok()), "the session has a terminal");
    fd
}

/// The descriptor of the controlling terminal, for forked children.
static TTY: AtomicI32 = AtomicI32::new(-1);
/// The write end of a report pipe, for forked children.
static REPORT: AtomicI32 = AtomicI32::new(-1);

fn tty() -> Fd {
    Fd::new(TTY.load(Ordering::Relaxed))
}

/// A background job: its own process group, then `tcsetpgrp` to it; 0 when its group is then
/// the foreground.
fn bg_tcsetpgrp(k: &CSys) -> i32 {
    let _ = k.alarm(10);
    if k.setpgid(0, 0).is_err() {
        return 90;
    }
    match term::set_foreground(k, tty(), k.getpgrp()) {
        Ok(()) if term::foreground(k, tty()) == Ok(k.getpgrp()) => 0,
        Ok(()) => 95,
        Err(e) => e.get(),
    }
}

/// As [`bg_tcsetpgrp`], with `SIGTTOU` ignored.
fn bg_tcsetpgrp_ignoring(k: &CSys) -> i32 {
    if k.sigaction_set(SIGTTOU, Disposition::Ignore, 0, SigSet::empty()).is_err() {
        return 91;
    }
    bg_tcsetpgrp(k)
}

/// As [`bg_tcsetpgrp`], with `SIGTTOU` blocked.
fn bg_tcsetpgrp_blocking(k: &CSys) -> i32 {
    if k.sigprocmask(SIG_BLOCK, Some(&SigSet::empty().with(SIGTTOU))).is_err() {
        return 92;
    }
    bg_tcsetpgrp(k)
}

/// A background job reading the settings and the foreground group (allowed, no signal).
fn bg_reads(k: &CSys) -> i32 {
    let _ = k.alarm(10);
    if k.setpgid(0, 0).is_err() {
        return 90;
    }
    match (term::get(k, tty()), term::foreground(k, tty())) {
        (Ok(_), Ok(g)) if g == k.getsid(0).unwrap_or(-1) => 0,
        _ => 96,
    }
}

/// The orphan: waits until its parent (its group's only other member) has gone, then reports
/// what `tcsetattr` and `tcgetattr` give it on the controlling terminal (two error numbers).
fn orphan_leaf(k: &CSys) -> i32 {
    let _ = k.alarm(10);
    let parent = k.getppid();
    let mut n = 0;
    while k.getppid() == parent && n < 500 {
        let t = Timespec { sec: 0, nsec: 10_000_000 };
        let _ = k.clock_nanosleep(1, 0, &t, None);
        n += 1;
    }
    let set = term::get(k, tty()).and_then(|t| term::set(k, tty(), When::Now, &t)).err().map_or(0, Errno::get);
    let get = term::get(k, tty()).err().map_or(0, Errno::get);
    let report = [u8::try_from(set).unwrap_or(255), u8::try_from(get).unwrap_or(255)];
    let _ = k.write(Fd::new(REPORT.load(Ordering::Relaxed)), &report);
    0
}

/// The orphan's parent: a new process group, the orphan in it, then gone.
fn orphan_parent(k: &CSys) -> i32 {
    if k.setpgid(0, 0).is_err() {
        return 90;
    }
    match selftest::spawn_child(orphan_leaf) {
        Ok(_) => 0,
        Err(_) => 93,
    }
}

/// Another session's process trying to take the terminal: 0 when that is `EPERM`.
fn steal(k: &CSys) -> i32 {
    let _ = k.alarm(10);
    match term::login(k, tty()) {
        Err(Errno::EPERM) => 0,
        _ => 1,
    }
}

/// Wait for `pid`, also when it stops: its status word.
fn wait_untraced(pid: i32) -> i32 {
    CSys.wait4(pid, WUNTRACED, None).unwrap().1
}

/// The signal that stopped a child, from its status word.
fn stopped_by(st: i32) -> Option<i32> {
    (st & 0xff == 0x7f).then_some((st >> 8) & 0xff)
}

/// Make the caller's group the foreground again (with `SIGTTOU` ignored meanwhile).
fn take_foreground(fd: i32) {
    CSys.sigaction_set(SIGTTOU, Disposition::Ignore, 0, SigSet::empty()).unwrap();
    assert_eq!(x::tcsetpgrp(fd, CSys.getpgrp()), 0);
    CSys.sigaction_set(SIGTTOU, Disposition::Default, 0, SigSet::empty()).unwrap();
}

// ------------------------------------------------------------------------------ §19.1 attributes

#[test]
fn s19_1_c_1() {
    // tcgetattr and tcsetattr read and write the terminal's settings; on a pty the master and
    // the slave share them.
    let p = Pair::new();
    let s = attrs(p.s);
    assert_eq!(seen(&attrs(p.m)), seen(&s), "the master reports the slave's settings");
    let mut t = s;
    t.c_lflag ^= ECHO;
    assert_eq!(x::tcsetattr(p.m, TCSANOW, Some(&t)), 0, "written through the master");
    assert_eq!(attrs(p.s).c_lflag, t.c_lflag, "seen on the slave");
    assert_eq!(term::get(&CSys, Fd::new(p.s)).unwrap().lflag, t.c_lflag, "the internal form agrees");
    errno::set_raw(0);
    assert_eq!(fails(i64::from(x::tcgetattr(p.s, None))), (-1, EFAULT), "no structure: EFAULT (S6)");
    assert_eq!(fails(i64::from(x::tcsetattr(p.s, TCSANOW, None))), (-1, EFAULT));
}

#[test]
fn s19_1_c_1_1() {
    // Input, output, control and local modes are written, read back, and take effect.
    let p = Pair::new();
    write(p.m, b"a\r");
    assert_eq!(take(p.s, 1000), b"a\n", "the defaults: canonical input with ICRNL");
    let _ = take(p.m, 100);
    let mut t = attrs(p.s);
    t.c_iflag = (t.c_iflag & !ICRNL) | IXOFF;
    t.c_oflag &= !OPOST;
    t.c_cflag |= CSTOPB;
    t.c_lflag &= !(ICANON | ECHO);
    t.c_cc[VMIN] = 1;
    t.c_cc[VTIME] = 0;
    assert_eq!(x::tcsetattr(p.s, TCSANOW, Some(&t)), 0);
    let u = attrs(p.s);
    assert_eq!((u.c_iflag, u.c_oflag, u.c_cflag, u.c_lflag), (t.c_iflag, t.c_oflag, t.c_cflag, t.c_lflag), "every mode word as written");
    write(p.m, b"b\r");
    assert_eq!(take(p.s, 1000), b"b\r", "no ICRNL, no line buffering");
    write(p.s, b"c\n");
    assert_eq!(take(p.m, 1000), b"c\n", "no output processing");
}

#[test]
fn s19_1_c_1_2() {
    // Control characters are written, read back and take effect; the C structure's bytes past
    // the kernel's 19 are not kept (§19.1 ⚑ KERN) and read back as 0.
    let p = Pair::new();
    let mut t = attrs(p.s);
    t.c_cc[VINTR] = 1;
    t.c_cc[VEOF] = 2;
    t.c_cc[VMIN] = 5;
    t.c_cc[VTIME] = 7;
    t.c_cc[VEOL2] = 0x42;
    for c in &mut t.c_cc[17..] {
        *c = 0x55;
    }
    assert_eq!(x::tcsetattr(p.s, TCSANOW, Some(&t)), 0);
    let mut u = zt();
    u.c_cc = [0x77; 32];
    assert_eq!(x::tcgetattr(p.s, Some(&mut u)), 0);
    assert_eq!(&u.c_cc[..19], &t.c_cc[..19], "the kernel's 19 control characters round-trip");
    assert_eq!(&u.c_cc[19..], &[0u8; 13], "the rest reads back as 0, also over other bytes");
    write(p.m, b"ab\x02");
    assert_eq!(take(p.s, 1000), b"ab", "VEOF ends the line and is not passed on");
}

#[test]
fn s19_1_c_1_3() {
    // Line speeds; when the settings apply; partial application (§19.1 ⚑ HIST); arbitrary
    // rates internally (D-19.1).
    let p = Pair::new();
    let orig = attrs(p.s);
    for &(code, _) in &CODES[1..] {
        let mut t = orig;
        assert_eq!(x::cfsetspeed(Some(&mut t), code), 0);
        assert_eq!(x::tcsetattr(p.s, TCSANOW, Some(&t)), 0, "{code:#o}");
        let u = attrs(p.s);
        assert_eq!((x::cfgetospeed(Some(&u)), x::cfgetispeed(Some(&u))), (code, code), "{code:#o} read back");
    }
    // Any rate through the internal form: BOTHER and the rate, kept by the terminal and by a
    // C structure's round trip.
    let fd = Fd::new(p.s);
    let mut k = term::get(&CSys, fd).unwrap();
    k.set_output_speed(12345);
    term::set(&CSys, fd, When::Now, &k).unwrap();
    let k = term::get(&CSys, fd).unwrap();
    assert_eq!((k.output_speed(), k.input_speed(), k.cflag & CBAUD), (12345, 12345, BOTHER), "an arbitrary rate");
    let c = attrs(p.s);
    assert_eq!((x::cfgetospeed(Some(&c)), c.c_ospeed), (BOTHER, 12345), "the C structure carries the rate");
    assert_eq!(x::tcsetattr(p.s, TCSANOW, Some(&orig)), 0);
    assert_eq!(x::tcsetattr(p.s, TCSANOW, Some(&c)), 0);
    assert_eq!(term::get(&CSys, fd).unwrap().output_speed(), 12345, "and gives it back");
    let mut k = term::get(&CSys, fd).unwrap();
    k.set_output_speed(9600);
    k.set_input_speed(2400);
    term::set(&CSys, fd, When::Now, &k).unwrap();
    let k = term::get(&CSys, fd).unwrap();
    assert_eq!((k.output_speed(), k.input_speed()), (9600, 2400), "separate input and output rates");
    // Input speed 0: the output speed when applied.
    let mut t = orig;
    x::cfsetospeed(Some(&mut t), B19200);
    x::cfsetispeed(Some(&mut t), 0);
    assert_eq!(x::tcsetattr(p.s, TCSANOW, Some(&t)), 0);
    let u = attrs(p.s);
    assert_eq!((x::cfgetospeed(Some(&u)), x::cfgetispeed(Some(&u))), (B19200, B19200));
    assert_eq!(term::get(&CSys, fd).unwrap().input_speed(), 19200);
    // When: TCSANOW and TCSADRAIN keep pending input, TCSAFLUSH discards it.
    raw(p.s);
    let r = attrs(p.s);
    for (action, kept) in [(TCSANOW, true), (TCSADRAIN, true), (TCSAFLUSH, false)] {
        write(p.m, b"abc");
        assert!(readable(p.s, 1000));
        assert_eq!(x::tcsetattr(p.s, action, Some(&r)), 0);
        assert_eq!(readable(p.s, 200), kept, "action {action}");
        let _ = take(p.s, 50);
    }
    errno::set_raw(0);
    for bad in [3, 4, -1, 0x100] {
        let mut t = r;
        t.c_lflag ^= ECHO;
        assert_eq!(fails(i64::from(x::tcsetattr(p.s, bad, Some(&t)))), (-1, EINVAL), "action {bad}");
        assert_eq!(attrs(p.s).c_lflag, r.c_lflag, "unchanged");
    }
    // Partial application: success when a requested change was taken (or none was requested);
    // EINVAL when none was taken.
    assert_eq!(x::tcsetattr(p.s, TCSANOW, Some(&orig)), 0);
    assert_eq!(x::tcsetattr(p.s, TCSANOW, Some(&orig)), 0, "nothing to change");
    let mut t = orig;
    t.c_cflag = (t.c_cflag & !CSIZE) | CS7;
    assert_eq!(fails(i64::from(x::tcsetattr(p.s, TCSANOW, Some(&t)))), (-1, EINVAL), "a pty keeps CS8: no change taken");
    assert_eq!(seen(&attrs(p.s)), seen(&orig));
    t.c_cflag |= PARENB;
    t.c_lflag ^= ECHO;
    assert_eq!(x::tcsetattr(p.s, TCSANOW, Some(&t)), 0, "the ECHO change is taken");
    let u = attrs(p.s);
    assert_eq!((u.c_cflag & CSIZE, u.c_cflag & PARENB, u.c_lflag & ECHO), (CS8, 0, t.c_lflag & ECHO), "callers re-read to see what was taken");
}

#[test]
fn s19_1_c_2() {
    // The cf* functions get and set speeds as B* codes.
    for &(code, _) in &CODES {
        let mut t = zt();
        t.c_cflag = CS8 | CREAD;
        assert_eq!(x::cfsetospeed(Some(&mut t), code), 0);
        assert_eq!((x::cfgetospeed(Some(&t)), x::cfgetispeed(Some(&t)), t.c_cflag & CBAUD), (code, code, code), "cfsetospeed {code:#o}");
        let mut t = zt();
        assert_eq!(x::cfsetispeed(Some(&mut t), code), 0);
        assert_eq!(x::cfgetispeed(Some(&t)), code, "cfsetispeed {code:#o}");
        let mut t = zt();
        assert_eq!(x::cfsetspeed(Some(&mut t), code), 0);
        assert_eq!((x::cfgetospeed(Some(&t)), x::cfgetispeed(Some(&t))), (code, code), "cfsetspeed {code:#o}");
    }
    // Not a code: EINVAL, the structure unchanged; cfsetspeed also takes the standard rates.
    let mut t = zt();
    x::cfsetspeed(Some(&mut t), B2400);
    let before = seen(&t);
    errno::set_raw(0);
    for v in [9600, 16, 0x1010, 0x2000, u32::MAX] {
        assert_eq!(fails(i64::from(x::cfsetospeed(Some(&mut t), v))), (-1, EINVAL), "cfsetospeed {v}");
        assert_eq!(fails(i64::from(x::cfsetispeed(Some(&mut t), v))), (-1, EINVAL), "cfsetispeed {v}");
        assert_eq!(seen(&t), before);
    }
    for &(code, rate) in &CODES {
        let mut t = zt();
        assert_eq!(x::cfsetspeed(Some(&mut t), rate), 0, "cfsetspeed {rate}");
        assert_eq!(x::cfgetospeed(Some(&t)), code);
    }
    assert_eq!(fails(i64::from(x::cfsetspeed(Some(&mut t), 12345))), (-1, EINVAL));
    assert_eq!(fails(i64::from(x::cfsetspeed(Some(&mut t), BOTHER))), (-1, EINVAL), "cfsetspeed has no rate for BOTHER");
    assert_eq!(x::cfsetospeed(Some(&mut t), BOTHER), 0, "the setters take BOTHER");
    assert_eq!(x::cfgetospeed(Some(&t)), BOTHER);
    assert_eq!(fails(i64::from(x::cfsetospeed(None, B9600))), (-1, EFAULT), "no structure (S6)");
    // Input speed 0, and the one speed the structure holds.
    let mut t = zt();
    x::cfsetospeed(Some(&mut t), B9600);
    assert_eq!(x::cfsetispeed(Some(&mut t), 0), 0);
    assert_eq!((x::cfgetospeed(Some(&t)), x::cfgetispeed(Some(&t))), (B9600, 0), "input speed 0");
    x::cfsetospeed(Some(&mut t), B2400);
    assert_eq!(x::cfgetispeed(Some(&t)), 0, "kept over a new output speed");
    x::cfsetispeed(Some(&mut t), B115200);
    assert_eq!((x::cfgetospeed(Some(&t)), x::cfgetispeed(Some(&t))), (B115200, B115200), "an input speed is also the output speed");
    let mut t = zt();
    t.c_cflag = B9600 | (B2400 << 16);
    assert_eq!((x::cfgetospeed(Some(&t)), x::cfgetispeed(Some(&t))), (B9600, B9600), "the CIBAUD bits are not read");
}

#[test]
fn s19_1_c_3() {
    // cfmakeraw: exactly the raw-mode bits change; speeds and other characters are kept.
    let mut t = zt();
    t.c_iflag = u32::MAX;
    t.c_oflag = u32::MAX;
    t.c_cflag = u32::MAX;
    t.c_lflag = u32::MAX;
    t.c_cc = [0xaa; 32];
    x::cfmakeraw(Some(&mut t));
    assert_eq!((t.c_iflag, t.c_oflag, t.c_cflag, t.c_lflag), (0xffff_fa14, 0xffff_fffe, 0xffff_feff, 0xffff_7fb4));
    assert_eq!((t.c_cc[VMIN], t.c_cc[VTIME]), (1, 0));
    assert!(t.c_cc.iter().enumerate().all(|(i, &c)| i == VMIN || i == VTIME || c == 0xaa), "other control characters kept");
    let mut t = zt();
    x::cfsetospeed(Some(&mut t), B115200);
    x::cfmakeraw(Some(&mut t));
    assert_eq!((t.c_iflag, t.c_oflag, t.c_cflag & !CBAUD, t.c_lflag, x::cfgetospeed(Some(&t))), (0, 0, CS8, 0, B115200));
    // On the terminal: bytes pass unchanged and generate no signal.
    let p = Pair::new();
    raw(p.s);
    write(p.m, b"\x03\r\x04");
    assert_eq!(take(p.s, 1000), b"\x03\r\x04");
}

#[test]
fn s19_1_c_4() {
    // tcsendbreak, tcdrain, tcflush, tcflow. (tcdrain as a cancellation point: the conformance
    // program `s19_terminals`.)
    let p = Pair::new();
    raw(p.s);
    for (fd, d) in [(p.s, 0), (p.s, 10), (p.s, -1), (p.m, 0)] {
        assert_eq!(x::tcsendbreak(fd, d), 0, "tcsendbreak({fd}, {d})");
    }
    assert!(take(p.m, 100).is_empty() && take(p.s, 100).is_empty(), "a pty transmits no break bytes");
    assert_eq!((x::tcdrain(p.s), x::tcdrain(p.m)), (0, 0));
    // tcflush: input waiting on the slave, output waiting for the master.
    for q in [TCIFLUSH, TCIOFLUSH] {
        write(p.m, b"data");
        assert!(readable(p.s, 1000));
        assert_eq!(x::tcflush(p.s, q), 0);
        assert!(!readable(p.s, 200), "queue {q} discards the slave's input");
    }
    write(p.m, b"data");
    assert!(readable(p.s, 1000));
    assert_eq!(x::tcflush(p.s, TCOFLUSH), 0);
    assert!(readable(p.s, 200), "TCOFLUSH keeps input");
    let _ = take(p.s, 50);
    write(p.s, b"data");
    assert!(readable(p.m, 1000));
    assert_eq!(x::tcflush(p.m, TCIFLUSH), 0);
    assert!(!readable(p.m, 200), "the master's input is the slave's output");
    // tcflow: output suspended and resumed; STOP and START transmitted.
    assert_eq!(x::tcflow(p.s, TCOOFF), 0);
    let fl = CSys.fcntl_int(Fd::new(p.s), FcntlInt::GetFl, 0).unwrap();
    CSys.fcntl_int(Fd::new(p.s), FcntlInt::SetFl, fl | i64::from(O_NONBLOCK)).unwrap();
    assert_eq!(CSys.write(Fd::new(p.s), b"x"), Err(Errno::EAGAIN), "output suspended");
    assert!(!readable(p.m, 200));
    assert_eq!(x::tcflow(p.s, TCOON), 0);
    assert_eq!(CSys.write(Fd::new(p.s), b"y"), Ok(1), "output resumed");
    assert_eq!(take(p.m, 1000), b"y");
    CSys.fcntl_int(Fd::new(p.s), FcntlInt::SetFl, fl).unwrap();
    assert_eq!(x::tcflow(p.s, TCIOFF), 0);
    assert_eq!(take(p.m, 1000), b"\x13", "TCIOFF transmits VSTOP");
    assert_eq!(x::tcflow(p.s, TCION), 0);
    assert_eq!(take(p.m, 1000), b"\x11", "TCION transmits VSTART");
    let mut t = attrs(p.s);
    t.c_cc[VSTOP] = 1;
    t.c_cc[VSTART] = 2;
    assert_eq!(x::tcsetattr(p.s, TCSANOW, Some(&t)), 0);
    assert_eq!(x::tcflow(p.s, TCIOFF), 0);
    assert_eq!(take(p.m, 1000), b"\x01", "the terminal's own STOP character");
    assert_eq!(x::tcflow(p.s, TCION), 0);
    assert_eq!(take(p.m, 1000), b"\x02");
}

#[test]
fn s19_1_c_5() {
    // tcgetsid: the session whose controlling terminal it is.
    let p = Pair::new();
    errno::set_raw(0);
    assert_eq!(fails(i64::from(x::tcgetsid(p.s))), (-1, ENOTTY), "no session's terminal");
    assert_eq!(fails(i64::from(x::tcgetsid(-1))), (-1, EBADF));
    alone("s19_1_c_5", "tcgetsid", || {
        let p = Pair::new();
        let _tty = take_terminal(&p);
        let me = CSys.getpid();
        x::tcgetsid(p.s) == me && x::tcgetsid(p.m) == me
    });
}

#[test]
fn s19_1_c_6() {
    // isatty: 1 for a terminal (errno unchanged), else 0 with ENOTTY (EBADF for no descriptor).
    let p = Pair::new();
    errno::set_raw(1234);
    assert_eq!((x::isatty(p.s), x::isatty(p.m), errno::get()), (1, 1, 1234));
    let null = open("/dev/null", O_RDWR | O_CLOEXEC).unwrap();
    let [r, w] = CSys.pipe2(O_CLOEXEC).unwrap();
    for fd in [null, r.raw()] {
        errno::set_raw(0);
        assert_eq!(fails(i64::from(x::isatty(fd))), (0, ENOTTY));
    }
    errno::set_raw(0);
    assert_eq!(fails(i64::from(x::isatty(-1))), (0, EBADF));
    for fd in [Fd::new(null), r, w] {
        let _ = CSys.close(fd);
    }
}

#[test]
fn s19_1_f_0() {
    // ENOTTY, EINVAL, EIO (an orphaned background group). EINTR: a pty's drain never waits, so
    // an interrupted tcdrain or TCSADRAIN cannot be produced here.
    let p = Pair::new();
    let [r, w] = CSys.pipe2(O_CLOEXEC).unwrap();
    let mut t = attrs(p.s);
    errno::set_raw(0);
    assert_eq!(fails(i64::from(x::tcgetattr(r.raw(), Some(&mut t)))), (-1, ENOTTY));
    assert_eq!(fails(i64::from(x::tcsetattr(r.raw(), TCSANOW, Some(&t)))), (-1, ENOTTY));
    assert_eq!(fails(i64::from(x::tcdrain(r.raw()))), (-1, ENOTTY));
    assert_eq!(fails(i64::from(x::tcsetattr(r.raw(), 7, Some(&t)))), (-1, EINVAL), "an invalid action first");
    assert_eq!(fails(i64::from(x::tcflush(r.raw(), 3))), (-1, EINVAL), "an invalid queue first");
    assert_eq!(fails(i64::from(x::tcflow(r.raw(), 4))), (-1, EINVAL), "an invalid action first");
    assert_eq!(fails(i64::from(x::tcflush(p.s, -1))), (-1, EINVAL));
    let _ = CSys.close(r);
    let _ = CSys.close(w);
    alone("s19_1_f_0", "orphaned", || {
        let p = Pair::new();
        TTY.store(take_terminal(&p), Ordering::Relaxed);
        let [r, w] = CSys.pipe2(O_CLOEXEC).unwrap();
        REPORT.store(w.raw(), Ordering::Relaxed);
        let pid = selftest::spawn_child(orphan_parent).unwrap();
        let _ = CSys.setpgid(pid, pid);
        let parent_ok = CSys.wait4(pid, 0, None).unwrap().1 == 0;
        let _ = CSys.close(w);
        let got = take(r.raw(), 8000);
        parent_ok && got == [EIO as u8, 0]
    });
}

// ------------------------------------------------------------------------------ §19.2 job control

#[test]
fn s19_2_c_1() {
    // tcgetpgrp: the foreground group of the caller's controlling terminal.
    let p = Pair::new();
    errno::set_raw(0);
    assert_eq!(fails(i64::from(x::tcgetpgrp(p.s))), (-1, ENOTTY), "not the caller's terminal");
    assert_eq!(fails(i64::from(x::tcgetpgrp(-1))), (-1, EBADF));
    alone("s19_2_c_1", "tcgetpgrp", || {
        let p = Pair::new();
        let fd = take_terminal(&p);
        let me = CSys.getpgrp();
        TTY.store(fd, Ordering::Relaxed);
        // A background job reads the settings and the foreground group without a signal.
        let pid = selftest::spawn_child(bg_reads).unwrap();
        let _ = CSys.setpgid(pid, pid);
        let bg = wait_untraced(pid);
        x::tcgetpgrp(p.s) == me && x::tcgetpgrp(p.m) == me && x::tcgetpgrp(fd) == me && bg == 0
    });
}

#[test]
fn s19_2_c_2() {
    // tcsetpgrp: the foreground group; a background caller is stopped by SIGTTOU unless it
    // blocks or ignores it.
    alone("s19_2_c_2", "tcsetpgrp", || {
        let p = Pair::new();
        let fd = take_terminal(&p);
        TTY.store(fd, Ordering::Relaxed);
        let me = CSys.getpgrp();
        assert_eq!(x::tcsetpgrp(fd, me), 0, "its own group");
        errno::set_raw(0);
        assert_eq!(fails(i64::from(x::tcsetpgrp(fd, -1))), (-1, EINVAL));
        assert_eq!(fails(i64::from(x::tcsetpgrp(fd, 0x3fff_fff0))), (-1, ESRCH), "no such group");
        assert_eq!(fails(i64::from(x::tcsetpgrp(-1, me))), (-1, EBADF));
        // In the background with SIGTTOU at its default: stopped.
        let pid = selftest::spawn_child(bg_tcsetpgrp).unwrap();
        let _ = CSys.setpgid(pid, pid);
        let st = wait_untraced(pid);
        assert_eq!(stopped_by(st), Some(SIGTTOU), "status {st:#x}");
        CSys.kill(pid, SIGKILL).unwrap();
        let _ = CSys.wait4(pid, 0, None);
        assert_eq!(x::tcgetpgrp(fd), me, "the foreground did not change");
        // Ignored or blocked: the job becomes the foreground.
        for body in [bg_tcsetpgrp_ignoring as fn(&CSys) -> i32, bg_tcsetpgrp_blocking] {
            let pid = selftest::spawn_child(body).unwrap();
            let _ = CSys.setpgid(pid, pid);
            assert_eq!(wait_untraced(pid), 0, "the job's tcsetpgrp made it the foreground");
            take_foreground(fd);
            assert_eq!(x::tcgetpgrp(fd), me);
        }
        true
    });
}

// ------------------------------------------------------------------------------ §19.3 names

#[test]
fn s19_3_c_0() {
    // ttyname: the terminal's path, in one static buffer; ttyname_r into the caller's.
    let p = Pair::new();
    let name = p.name();
    let a = x::ttyname(p.s);
    assert_eq!(text_at(a), name, "the slave's /dev/pts path");
    let b = x::ttyname(p.m);
    assert_eq!(text_at(b), "/dev/ptmx", "the master's");
    assert_eq!(a, b, "one static buffer");
    let mut buf = [0 as c_char; 64];
    assert_eq!(x::ttyname_r(p.s, buf.as_mut_ptr(), buf.len()), 0);
    assert_eq!(stored(&buf), name);
    let dup = CSys.dup(Fd::new(p.s)).unwrap();
    assert_eq!(text_at(x::ttyname(dup.raw())), name, "any descriptor of it");
    let _ = CSys.close(dup);
}

#[test]
fn s19_3_f_1() {
    // ENOTTY: not a terminal (EBADF: not a descriptor). ttyname_r returns the number and also
    // stores it in errno.
    let null = open("/dev/null", O_RDWR | O_CLOEXEC).unwrap();
    errno::set_raw(0);
    assert!(x::ttyname(null).is_null());
    assert_eq!(errno::get(), ENOTTY);
    let mut buf = [0 as c_char; 64];
    errno::set_raw(0);
    assert_eq!((x::ttyname_r(null, buf.as_mut_ptr(), buf.len()), errno::get()), (ENOTTY, ENOTTY));
    assert_eq!(x::ttyname_r(-1, buf.as_mut_ptr(), buf.len()), EBADF);
    let _ = CSys.close(Fd::new(null));
}

#[test]
fn s19_3_f_2() {
    // ENODEV: the kernel's name for the device is not that device here (D-19.3); when the path
    // names nothing (the entry is gone) the name is not given. Without /proc (OD-5, D-X.12)
    // the name the device number gives, only when it is the same terminal. The namespace cases
    // (another devpts instance, /proc hidden) run in `terminals_products.rs` and
    // `noproc_products.rs`.
    let p = Pair::new();
    let mut out = [0u8; 4096];
    let n = term::name(&CSys, Fd::new(p.s), &mut out).unwrap();
    assert_eq!(std::str::from_utf8(&out[..n]).unwrap(), p.name());
    let k = Faulty::new(CSys);
    assert!(k.fail("readlinkat", Errno::ENOENT, 0, 1));
    let n = term::name(&k, Fd::new(p.s), &mut out).unwrap();
    assert_eq!(std::str::from_utf8(&out[..n]).unwrap(), p.name(), "no /proc: the name from the device number");
    let k = Faulty::new(CSys);
    assert!(k.fail("readlinkat", Errno::ENOENT, 0, 1));
    assert!(k.fail("fstatat", Errno::ENOENT, 0, 1));
    assert_eq!(term::name(&k, Fd::new(p.s), &mut out), Err(Errno::ENODEV), "no /proc, and no /dev node for the number");
    let k = Faulty::new(CSys);
    assert!(k.fail("readlinkat", Errno::EACCES, 0, 1));
    assert_eq!(term::name(&k, Fd::new(p.s), &mut out), Err(Errno::ENODEV), "a link that cannot be read for another reason");
    let null = std::fs::File::open("/dev/null").unwrap();
    let st = CSys.fstat(Fd::new(std::os::fd::AsRawFd::as_raw_fd(&null))).unwrap();
    assert_eq!(term::name_by_device(&CSys, &st, &mut out), Err(Errno::ENODEV), "a device number that names no terminal");
    let k = Faulty::new(CSys);
    assert!(k.fail("fstatat", Errno::ENOENT, 0, 1));
    assert_eq!(term::name(&k, Fd::new(p.s), &mut out), Err(Errno::ENODEV), "the /dev entry is gone");
    let k = Faulty::new(CSys);
    assert!(k.fail("tcgets2", Errno::ENOTTY, 0, 1));
    assert_eq!(term::name(&k, Fd::new(p.s), &mut out), Err(Errno::ENOTTY), "not a terminal comes first");
}

#[test]
fn s19_3_f_3() {
    // ERANGE: a buffer that cannot hold the name and its terminator, untouched; also for
    // ptsname_r. A NULL buffer is EINVAL.
    let p = Pair::new();
    let len = p.name().len();
    for size in [0, 1, len - 1, len] {
        let mut buf = [b'z' as c_char; 64];
        errno::set_raw(0);
        assert_eq!((x::ttyname_r(p.s, buf.as_mut_ptr(), size), errno::get()), (ERANGE, ERANGE), "ttyname_r size {size}");
        errno::set_raw(0);
        assert_eq!((x::ptsname_r(p.m, buf.as_mut_ptr(), size), errno::get()), (ERANGE, ERANGE), "ptsname_r size {size}");
        assert!(buf.iter().all(|&c| c == b'z' as c_char), "untouched");
    }
    let mut buf = [0 as c_char; 64];
    assert_eq!(x::ttyname_r(p.s, buf.as_mut_ptr(), len + 1), 0);
    assert_eq!(x::ptsname_r(p.m, buf.as_mut_ptr(), len + 1), 0);
    assert_eq!(x::ttyname_r(p.s, null_mut(), 64), EINVAL);
    assert_eq!(x::ptsname_r(p.m, null_mut(), 64), EINVAL);
}

// ------------------------------------------------------------------------------ §19.4 pseudo-terminals

#[test]
fn s19_4_c_1() {
    // posix_openpt opens a new master with the file flags given; flags that would not open a
    // terminal are EINVAL. getpt: O_RDWR.
    let _g = PTSNAME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    for (flags, cloexec, nonblock) in [(O_RDWR | O_NOCTTY, false, false), (O_RDWR | O_CLOEXEC, true, false), (O_RDWR | O_NONBLOCK, false, true)] {
        let m = x::posix_openpt(flags as i32);
        assert!(m >= 0, "{flags:#o}");
        assert_eq!(x::isatty(m), 1);
        let fd = CSys.fcntl_int(Fd::new(m), FcntlInt::GetFd, 0).unwrap();
        let fl = CSys.fcntl_int(Fd::new(m), FcntlInt::GetFl, 0).unwrap();
        assert_eq!((fd & 1 != 0, fl & i64::from(O_NONBLOCK) != 0, fl & 3), (cloexec, nonblock, 2));
        assert!(!x::ptsname(m).is_null(), "a master");
        let _ = CSys.close(Fd::new(m));
    }
    errno::set_raw(0);
    for bad in [0o10_000_002, 0o200_002, 0x4000_0002, -1] {
        assert_eq!(fails(i64::from(x::posix_openpt(bad))), (-1, EINVAL), "{bad:#o}");
    }
    let m = x::getpt();
    assert!(m >= 0 && x::isatty(m) == 1, "getpt: a master");
    assert_eq!(CSys.fcntl_int(Fd::new(m), FcntlInt::GetFl, 0).unwrap() & 3, 2, "O_RDWR");
    let _ = CSys.close(Fd::new(m));
}

#[test]
fn s19_4_c_2() {
    // grantpt: the kernel's devpts already gave the slave to the caller (mode 620); grantpt
    // checks that it was given a master and changes nothing.
    use std::os::unix::fs::MetadataExt;
    let m = x::posix_openpt((O_RDWR | O_NOCTTY | O_CLOEXEC) as i32);
    let name = slave_name(m);
    let before = std::fs::metadata(&name).unwrap();
    assert_eq!(x::grantpt(m), 0);
    assert_eq!(x::grantpt(m), 0, "again");
    let after = std::fs::metadata(&name).unwrap();
    assert_eq!((after.mode() & 0o7777, after.uid(), after.gid()), (before.mode() & 0o7777, before.uid(), before.gid()));
    assert_eq!((after.mode() & 0o7777, after.uid()), (0o620, CSys.geteuid()));
    let _ = CSys.close(Fd::new(m));
}

#[test]
fn s19_4_c_3() {
    // unlockpt: the slave can be opened only after it.
    let m = x::posix_openpt((O_RDWR | O_NOCTTY | O_CLOEXEC) as i32);
    let name = slave_name(m);
    assert_eq!(open(&name, O_RDWR | O_NOCTTY | O_CLOEXEC), Err(Errno::EIO), "locked");
    assert_eq!(x::unlockpt(m), 0);
    assert_eq!(x::unlockpt(m), 0, "again");
    let s = open(&name, O_RDWR | O_NOCTTY | O_CLOEXEC).unwrap();
    assert_eq!(x::isatty(s), 1);
    let _ = CSys.close(Fd::new(s));
    let _ = CSys.close(Fd::new(m));
}

#[test]
fn s19_4_c_4() {
    // ptsname: the slave's path, /dev/pts/N, in one static buffer.
    let _g = PTSNAME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let a = Pair::new();
    let b = Pair::new();
    let n = CSys.ioctl_get_int(Fd::new(a.m), IoctlIntOut::Tiocgptn).unwrap();
    let pa = x::ptsname(a.m);
    assert_eq!(text_at(pa), format!("/dev/pts/{n}"));
    let pb = x::ptsname(b.m);
    assert_eq!(pa, pb, "one static buffer");
    assert_eq!(text_at(pb), b.name());
    assert_ne!(a.name(), b.name());
}

#[test]
fn s19_4_c_5() {
    // openpty: a pair, both open and not close-on-exec, the slave opened from the master
    // (TIOCGPTPEER, D-19.2), with termp and winp applied; a round trip through it.
    let mut name = [b'q' as c_char; 64];
    let mut want = zt();
    want.c_iflag = ICRNL;
    want.c_oflag = OPOST;
    want.c_cflag = CS8 | CREAD;
    want.c_lflag = ICANON | ISIG;
    want.c_cc[VINTR] = 3;
    want.c_cc[VEOF] = 4;
    x::cfsetspeed(Some(&mut want), B115200);
    let ws = struct_winsize { ws_row: 24, ws_col: 80, ws_xpixel: 640, ws_ypixel: 480 };
    let (mut m, mut s) = (-1, -1);
    assert_eq!(x::openpty(Some(&mut m), Some(&mut s), name.as_mut_ptr(), Some(&want), Some(&ws)), 0);
    let p = Pair { m, s };
    let nm = stored(&name);
    assert_eq!(nm, p.name(), "name receives the slave's path");
    for fd in [m, s] {
        assert_eq!(CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFd, 0).unwrap(), 0, "not close-on-exec");
        assert_eq!(CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFl, 0).unwrap() & 3, 2, "O_RDWR");
    }
    let st = CSys.fstat(Fd::new(s)).unwrap();
    let c = CString::new(nm).unwrap();
    let there = CSys.fstatat(Fd::CWD, KStr::from_nul_terminated(c.as_bytes_with_nul()).unwrap(), 0).unwrap();
    assert_eq!((st.rdev, st.ino), (there.rdev, there.ino), "the slave is the master's peer");
    assert_eq!(seen(&attrs(s)), seen(&want), "termp applied");
    let w = CSys.tiocgwinsz(Fd::new(m)).unwrap();
    assert_eq!((w.row, w.col, w.xpixel, w.ypixel), (24, 80, 640, 480), "winp, seen on both sides");
    assert_eq!(CSys.tiocgwinsz(Fd::new(s)).unwrap(), w);
    raw(s);
    write(m, b"ping");
    assert_eq!(take(s, 1000), b"ping");
    write(s, b"pong");
    assert_eq!(take(m, 1000), b"pong");
    drop(p);
    // NULL descriptors: EFAULT, nothing opened. The internal pair with a kernel whose
    // TIOCGPTPEER fails: the master is closed again and the error returned.
    errno::set_raw(0);
    let mut one = -1;
    assert_eq!(fails(i64::from(x::openpty(None, Some(&mut one), null_mut(), None, None))), (-1, EFAULT));
    assert_eq!(fails(i64::from(x::openpty(Some(&mut one), None, null_mut(), None, None))), (-1, EFAULT));
    assert_eq!(one, -1);
    let k = Faulty::new(CSys);
    assert!(k.fail("ioctl_int", Errno::ENOTTY, 0, 1));
    assert_eq!(term::open_pair(&k, &k, None, None), Err(Errno::ENOTTY));
}

#[test]
fn s19_4_c_6() {
    // forkpty: openpty, fork, login_tty in the child. Liella's fork needs a process Liella
    // started: the conformance program checks the child's session, terminal, settings, window
    // size and descriptors, the parent's master and name, and the bytes and status the child
    // sends back.
    conformance(&["s19_terminals"]);
}

#[test]
fn s19_4_c_7() {
    // login_tty: a new session with the terminal as its controlling terminal and as
    // descriptors 0-2; the descriptor given is closed. A caller already leading a session keeps
    // it; a descriptor that is no terminal changes nothing; another session's terminal is EPERM.
    alone("s19_4_c_7", "login", || {
        let p = Pair::new();
        let s = CSys.dup2(Fd::new(p.s), Fd::new(77)).unwrap().raw();
        let me = CSys.getpid();
        let name = p.name();
        if us::login_tty(s) != 0 {
            return false;
        }
        let mut ok = CSys.getsid(0) == Ok(me) && x::tcgetsid(0) == me && x::tcgetpgrp(0) == CSys.getpgrp();
        for fd in 0..3 {
            ok &= text_at(x::ttyname(fd)) == name && CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFd, 0) == Ok(0);
        }
        ok && CSys.fcntl_int(Fd::new(s), FcntlInt::GetFd, 0) == Err(Errno::EBADF)
    });
    alone("s19_4_c_7", "not-a-terminal", || {
        let null = open("/dev/null", O_RDWR | O_CLOEXEC).unwrap();
        let sid = CSys.getsid(0);
        errno::set_raw(0);
        let r = us::login_tty(null);
        r == -1 && errno::get() == ENOTTY && CSys.getsid(0) == sid && sid != Ok(CSys.getpid())
    });
    alone("s19_4_c_7", "leader", || {
        let _ = CSys.setsid();
        let p = Pair::new();
        errno::set_raw(0);
        us::login_tty(p.s) == 0 && errno::get() == 0 && x::tcgetsid(0) == CSys.getpid()
    });
    alone("s19_4_c_7", "taken", || {
        let p = Pair::new();
        let fd = take_terminal(&p);
        TTY.store(fd, Ordering::Relaxed);
        let pid = selftest::spawn_child(steal).unwrap();
        CSys.wait4(pid, 0, None).unwrap().1 == 0 && x::tcgetsid(fd) == CSys.getpid()
    });
}

#[test]
fn s19_4_f_0() {
    // Not a master: grantpt and unlockpt EINVAL, ptsname ENOTTY (EBADF for no descriptor). No
    // ptys available (EAGAIN/ENOENT) would need the system's limit reached: the multiplexer's
    // error is passed on, shown with an injected one.
    let _g = PTSNAME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let p = Pair::new();
    let null = open("/dev/null", O_RDWR | O_CLOEXEC).unwrap();
    for fd in [p.s, null] {
        errno::set_raw(0);
        assert_eq!(fails(i64::from(x::grantpt(fd))), (-1, EINVAL));
        assert_eq!(fails(i64::from(x::unlockpt(fd))), (-1, EINVAL));
        assert!(x::ptsname(fd).is_null());
        assert_eq!(errno::get(), ENOTTY);
        let mut b = [0 as c_char; 64];
        assert_eq!(x::ptsname_r(fd, b.as_mut_ptr(), b.len()), ENOTTY);
    }
    errno::set_raw(0);
    assert_eq!(fails(i64::from(x::grantpt(-1))), (-1, EBADF));
    assert_eq!(fails(i64::from(x::unlockpt(-1))), (-1, EBADF));
    let _ = CSys.close(Fd::new(null));
    for e in [Errno::EAGAIN, Errno::ENOENT] {
        let k = Faulty::new(CSys);
        assert!(k.fail("openat", e, 0, 2));
        assert_eq!(term::open_master(&k, O_RDWR), Err(e));
        assert_eq!(term::open_pair(&CSys, &k, None, None), Err(e));
    }
}

// ------------------------------------------------------------------------------ §19.5 legacy interfaces

/// The fields of a `/etc/ttys` line as text (`None` for NULL).
type Ent = (String, Option<String>, Option<String>, i32, Option<String>, Option<String>);

fn ent(line: &str) -> Option<Ent> {
    let f = |v: Option<term::Field<'_>>| v.map(|f| String::from_utf8(f.bytes().collect()).unwrap());
    TtyLine::parse(line.as_bytes()).map(|e| (f(Some(e.name)).unwrap(), f(e.getty), f(e.kind), e.status, f(e.window), e.comment.map(|c| String::from_utf8(c.to_vec()).unwrap())))
}

fn some(s: &str) -> Option<String> {
    Some(s.to_string())
}

#[test]
fn s19_5_c_0() {
    // /etc/ttys entries (BSD ttys(5)); gtty, stty and revoke fail with ENOSYS.
    assert_eq!(ent("console\t\"/usr/libexec/getty std.9600\"\tvt100\ton secure"), Some(("console".into(), some("/usr/libexec/getty std.9600"), some("vt100"), 3, None, None)));
    assert_eq!(ent("ttyv0 \"g Pc\"\t\tcons25 on  secure window=\"/usr/X11/bin/xterm -display :0\""), Some(("ttyv0".into(), some("g Pc"), some("cons25"), 3, some("/usr/X11/bin/xterm -display :0"), None)));
    assert_eq!(ent("ttyv1\tnone\tnetwork\toff"), Some(("ttyv1".into(), some("none"), some("network"), 0, None, None)));
    assert_eq!(ent("   indented   getty1   type1   on"), Some(("indented".into(), some("getty1"), some("type1"), 1, None, None)), "leading white space");
    for skipped in ["# a comment", "", "  \t", "   # indented comment"] {
        assert_eq!(ent(skipped), None, "{skipped:?}");
    }
    assert_eq!(ent("d0\tg\tdialup\toff # trailing comment").unwrap().5, some("trailing comment"));
    assert_eq!(ent("d1\tg\tt\ton#nospace").unwrap().5, some("on#nospace"), "an unknown word starts the comment, whole");
    assert_eq!(ent("q\tg\tt\ton insecure").map(|e| (e.3, e.5)), Some((1, some("insecure"))));
    assert_eq!(ent("f\tg\tt\ton off secure unknown"), Some(("f".into(), some("g"), some("t"), 2, None, some("unknown"))), "off clears on");
    assert_eq!(ent("u\tg\tt\tON SECURE").unwrap().5, some("ON SECURE"), "words are case-sensitive");
    assert_eq!(ent("nameonly"), Some(("nameonly".into(), None, None, 0, None, None)));
    assert_eq!(ent("nameonly2 # c"), Some(("nameonly2".into(), some(""), None, 0, None, some("c"))));
    assert_eq!(ent("q\t\"a \\\"b\\\" c\"\t\"t y p e\"\ton"), Some(("q".into(), some("a \"b\" c"), some("t y p e"), 1, None, None)), "quoted fields");
    assert_eq!(ent("h\t\"x#y\"\tt#z\ton"), Some(("h".into(), some("x#y"), some("t"), 0, None, some("z\ton"))), "# ends an unquoted field");
    assert_eq!(ent("w\tg\tt\twindow=\"a b\" on"), Some(("w".into(), some("g"), some("t"), 1, some("a b"), None)));
    assert_eq!(ent("e\t\"\"\t\"\"\ton"), Some(("e".into(), some(""), some(""), 1, None, None)), "empty quoted fields");
    assert_eq!(ent("o\t\"abc def"), Some(("o".into(), some("abc def"), None, 0, None, None)), "an open quote runs to the end");
    let long = format!("long\t{}\tt\ton", "a".repeat(300));
    assert_eq!(ent(&long).unwrap().1.unwrap().len(), 300, "any length");
    // An enumeration over a fixture table (the path is a parameter; the exports' /etc/ttys is
    // checked in the differential test's namespace).
    let dir = scratch::Scratch::dir("c19");
    let file = dir.join("ttys");
    std::fs::write(&file, format!("# t\nconsole\tg\tvt100\ton\n\n{long}\nlast\tg\tt\toff")).unwrap();
    let path = CString::new(file.to_str().unwrap()).unwrap();
    let cursor = Cursor::new();
    let mut names = Vec::new();
    while let Some(n) = cursor.next(&CSys, path.as_bytes_with_nul(), |l| TtyLine::parse(l).map_or(Step::Skip, |e| Step::Take(String::from_utf8(e.name.bytes().collect()).unwrap()))).unwrap() {
        names.push(n);
    }
    assert_eq!(names, ["console", "long", "last"], "long lines and a last line without a newline are entries");
    cursor.end(&CSys);
    drop(dir); // before `alone`: its child ends with `exit_group`, which runs no destructors
    // setttyent reports whether the table can be read (process-wide state: alone).
    alone("s19_5_c_0", "setttyent", || {
        errno::set_raw(0);
        let r = x::setttyent();
        let ok = if std::path::Path::new("/etc/ttys").exists() { r == 1 } else { r == 0 && errno::get() == ENOENT };
        ok && x::endttyent() == 1 && x::endttyent() == 1
    });
    // The 4.2BSD interfaces.
    let p = Pair::new();
    let mut sg = [0u64; 8];
    errno::set_raw(0);
    assert_eq!(fails(i64::from(x::gtty(p.s, sg.as_mut_ptr().cast()))), (-1, ENOSYS));
    assert_eq!(fails(i64::from(x::stty(p.s, sg.as_ptr().cast()))), (-1, ENOSYS));
    let name = CString::new(p.name()).unwrap();
    assert_eq!(fails(i64::from(x::revoke(name.as_ptr()))), (-1, ENOSYS));
    assert_eq!(CSys.write(Fd::new(p.s), b"x"), Ok(1), "the terminal is still usable");
}
