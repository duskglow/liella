// Specified from spec §19 in phase 11 (`login_tty` is in s17_users_symbols, phase 10; `ctermid`
// in s12_stdio_symbols; `revoke` in s13_filesystem_symbols).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Translation-layer tests for the exported symbols of spec §19 (34 entries).
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! Each test checks the export (name and default version in `libc.so.6`) and the C conventions
//! of the translation on private ptys: `-1` and `errno`, the error number returned (and stored
//! in `errno`) by the `_r` forms, NULL and `errno` for the pointer results, the C structures.
//! Behaviour is the clause tests' (`s19_terminals_clauses`). `forkpty` forks, so it runs in the
//! conformance program `s19_terminals`. The `/etc/ttys` functions read a fixed path: only their
//! answers without the file are checked here (in a re-executed child, their cursor being
//! process-wide); with a fixture table they run in `terminals_products.rs`'s namespace.
//! `openpty@GLIBC_2.2.5` and `forkpty@GLIBC_2.2.5` (the compat versions) arrive with phase 15.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(non_snake_case)]
#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]

use core::ptr::null_mut;

use liella_abi::ctypes::c_char;
use liella_abi::layout::{struct_termios, struct_winsize};
use liella_boundary::errno;
use liella_boundary::posix::s04_strings as st;
use liella_boundary::posix::s19_terminals as s;
use liella_boundary::sys::CSys;
use liella_kernel::fd::Fd;
use liella_kernel::ops::{FcntlInt, IoctlIntOut};
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{O_CLOEXEC, O_NOCTTY, O_RDWR};

mod products;
use products::{conformance, exported};

const ENOENT: i32 = 2;
const EIO: i32 = 5;
const EBADF: i32 = 9;
const EFAULT: i32 = 14;
const EINVAL: i32 = 22;
const ENOTTY: i32 = 25;
const ERANGE: i32 = 34;
const ENOSYS: i32 = 38;

const B0: u32 = 0;
const B2400: u32 = 11;
const B9600: u32 = 13;
const B38400: u32 = 15;
const BOTHER: u32 = 0o10000;
const ECHO: u32 = 0o10;
const CS8: u32 = 0o60;

/// A result and the `errno` after it.
fn fails(r: i64) -> (i64, i32) {
    (r, errno::get())
}

fn zt() -> struct_termios {
    struct_termios { c_iflag: 0, c_oflag: 0, c_cflag: 0, c_lflag: 0, c_line: 0, c_cc: [0; 32], c_ispeed: 0, c_ospeed: 0 }
}

/// A private pty pair, closed when dropped.
struct Pair {
    m: i32,
    s: i32,
}

impl Pair {
    fn new() -> Pair {
        let (mut m, mut s) = (-1, -1);
        assert_eq!(s::openpty(Some(&mut m), Some(&mut s), null_mut(), None, None), 0);
        Pair { m, s }
    }
    fn name(&self) -> String {
        let n = CSys.ioctl_get_int(Fd::new(self.m), IoctlIntOut::Tiocgptn).unwrap();
        format!("/dev/pts/{n}")
    }
}

impl Drop for Pair {
    fn drop(&mut self) {
        let _ = CSys.close(Fd::new(self.s));
        let _ = CSys.close(Fd::new(self.m));
    }
}

/// The C text at `p`.
fn text(p: *const c_char) -> String {
    assert!(!p.is_null());
    let n = st::strlen(p);
    let mut v = vec![0u8; n];
    st::memcpy(v.as_mut_ptr().cast(), p.cast(), n);
    String::from_utf8(v).unwrap()
}

/// The text in a C buffer.
fn stored(b: &[c_char]) -> String {
    String::from_utf8(b.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect()).unwrap()
}

/// `/dev/null`, opened close-on-exec.
fn null() -> i32 {
    CSys.openat(Fd::CWD, KStr::from_nul_terminated(b"/dev/null\0").unwrap(), O_RDWR | O_CLOEXEC, 0).unwrap().raw()
}

/// Set in a re-executed copy: the test it runs alone.
const ALONE: &str = "LIELLA_S19_ALONE";

/// Run `body` alone in a re-executed copy of this test binary (process-wide state); the copy's
/// own assertions decide.
fn alone(test: &str, body: impl FnOnce()) {
    if std::env::var(ALONE).is_ok_and(|v| v == test) {
        body();
        return;
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap()).args([test, "--exact", "--test-threads=1"]).env(ALONE, test).output().unwrap();
    assert!(o.status.success(), "{test}: {:?}\n{}{}", o.status, String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

// ------------------------------------------------------------------------------ §19.1 speeds

#[test]
fn sym_cfgetispeed() {
    exported("cfgetispeed");
    let mut t = zt();
    t.c_cflag = B9600 | CS8;
    assert_eq!(s::cfgetispeed(Some(&t)), B9600, "the CBAUD code");
    assert_eq!(s::cfsetispeed(Some(&mut t), 0), 0);
    assert_eq!(s::cfgetispeed(Some(&t)), B0, "input speed 0");
}

#[test]
fn sym_cfgetospeed() {
    exported("cfgetospeed");
    let mut t = zt();
    t.c_cflag = 0xffff_ffff;
    assert_eq!(s::cfgetospeed(Some(&t)), 0o10017, "c_cflag & CBAUD");
}

#[test]
fn sym_cfmakeraw() {
    exported("cfmakeraw");
    let mut t = zt();
    t.c_cflag = B9600;
    s::cfmakeraw(Some(&mut t));
    assert_eq!((t.c_iflag, t.c_oflag, t.c_cflag, t.c_lflag, t.c_cc[6], t.c_cc[5]), (0, 0, B9600 | CS8, 0, 1, 0));
}

#[test]
fn sym_cfsetispeed() {
    exported("cfsetispeed");
    let mut t = zt();
    assert_eq!(s::cfsetispeed(Some(&mut t), B2400), 0);
    assert_eq!((s::cfgetospeed(Some(&t)), s::cfgetispeed(Some(&t))), (B2400, B2400));
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::cfsetispeed(Some(&mut t), 2400))), (-1, EINVAL), "a rate is not a code");
    assert_eq!(fails(i64::from(s::cfsetispeed(None, B2400))), (-1, EFAULT));
}

#[test]
fn sym_cfsetospeed() {
    exported("cfsetospeed");
    let mut t = zt();
    assert_eq!(s::cfsetospeed(Some(&mut t), BOTHER), 0);
    assert_eq!(s::cfgetospeed(Some(&t)), BOTHER);
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::cfsetospeed(Some(&mut t), 0x2000))), (-1, EINVAL));
    assert_eq!(s::cfgetospeed(Some(&t)), BOTHER, "unchanged");
}

#[test]
fn sym_cfsetspeed() {
    exported("cfsetspeed");
    let mut t = zt();
    assert_eq!(s::cfsetspeed(Some(&mut t), 38400), 0, "a rate");
    assert_eq!(s::cfgetospeed(Some(&t)), B38400);
    assert_eq!(s::cfsetspeed(Some(&mut t), B2400), 0, "a code");
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::cfsetspeed(Some(&mut t), BOTHER))), (-1, EINVAL));
}

// ------------------------------------------------------------------------------ §19.1 attributes

#[test]
fn sym_tcgetattr() {
    exported("tcgetattr");
    let p = Pair::new();
    let mut t = zt();
    t.c_cc = [0x77; 32];
    assert_eq!(s::tcgetattr(p.s, Some(&mut t)), 0);
    assert_eq!(s::cfgetospeed(Some(&t)), B38400, "a new pty's speed");
    assert_eq!(&t.c_cc[19..], &[0; 13], "past the kernel's control characters");
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::tcgetattr(null(), Some(&mut t)))), (-1, ENOTTY));
    assert_eq!(fails(i64::from(s::tcgetattr(-1, Some(&mut t)))), (-1, EBADF));
    assert_eq!(fails(i64::from(s::tcgetattr(p.s, None))), (-1, EFAULT));
}

#[test]
fn sym_tcsetattr() {
    exported("tcsetattr");
    let p = Pair::new();
    let mut t = zt();
    s::tcgetattr(p.s, Some(&mut t));
    t.c_lflag ^= ECHO;
    for action in 0..3 {
        t.c_lflag ^= ECHO;
        assert_eq!(s::tcsetattr(p.s, action, Some(&t)), 0, "action {action}");
    }
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::tcsetattr(-1, 3, Some(&t)))), (-1, EINVAL), "the action is checked first");
    assert_eq!(fails(i64::from(s::tcsetattr(-1, 0, Some(&t)))), (-1, EBADF));
    assert_eq!(fails(i64::from(s::tcsetattr(p.s, 0, None))), (-1, EFAULT));
}

#[test]
fn sym_tcsendbreak() {
    exported("tcsendbreak");
    let p = Pair::new();
    assert_eq!(s::tcsendbreak(p.s, 0), 0);
    assert_eq!(s::tcsendbreak(p.s, 250), 0);
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::tcsendbreak(null(), 0))), (-1, ENOTTY));
}

#[test]
fn sym_tcdrain() {
    exported("tcdrain");
    let p = Pair::new();
    assert_eq!(s::tcdrain(p.s), 0);
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::tcdrain(-1))), (-1, EBADF));
}

#[test]
fn sym_tcflush() {
    exported("tcflush");
    let p = Pair::new();
    assert_eq!(s::tcflush(p.s, 2), 0);
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::tcflush(p.s, 3))), (-1, EINVAL));
    assert_eq!(fails(i64::from(s::tcflush(-1, 0))), (-1, EBADF));
}

#[test]
fn sym_tcflow() {
    exported("tcflow");
    let p = Pair::new();
    assert_eq!(s::tcflow(p.s, 0), 0);
    assert_eq!(s::tcflow(p.s, 1), 0);
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::tcflow(p.s, 4))), (-1, EINVAL));
    assert_eq!(fails(i64::from(s::tcflow(null(), 1))), (-1, ENOTTY));
}

#[test]
fn sym_tcgetsid() {
    exported("tcgetsid");
    let p = Pair::new();
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::tcgetsid(p.s))), (-1, ENOTTY), "no session's terminal");
    assert_eq!(fails(i64::from(s::tcgetsid(-1))), (-1, EBADF));
}

#[test]
fn sym_isatty() {
    exported("isatty");
    let p = Pair::new();
    errno::set_raw(77);
    assert_eq!((s::isatty(p.s), errno::get()), (1, 77), "errno untouched");
    assert_eq!(fails(i64::from(s::isatty(null()))), (0, ENOTTY));
    assert_eq!(fails(i64::from(s::isatty(-1))), (0, EBADF));
}

// ------------------------------------------------------------------------------ §19.2

#[test]
fn sym_tcgetpgrp() {
    exported("tcgetpgrp");
    let p = Pair::new();
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::tcgetpgrp(p.s))), (-1, ENOTTY));
    assert_eq!(fails(i64::from(s::tcgetpgrp(-1))), (-1, EBADF));
}

#[test]
fn sym_tcsetpgrp() {
    exported("tcsetpgrp");
    let p = Pair::new();
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::tcsetpgrp(p.s, CSys.getpgrp()))), (-1, ENOTTY), "not the caller's terminal");
    assert_eq!(fails(i64::from(s::tcsetpgrp(-1, 1))), (-1, EBADF));
}

// ------------------------------------------------------------------------------ §19.3

#[test]
fn sym_ttyname() {
    exported("ttyname");
    let p = Pair::new();
    assert_eq!(text(s::ttyname(p.s)), p.name());
    errno::set_raw(0);
    assert!(s::ttyname(null()).is_null());
    assert_eq!(errno::get(), ENOTTY);
}

#[test]
fn sym_ttyname_r() {
    exported("ttyname_r");
    let p = Pair::new();
    let mut b = [0 as c_char; 64];
    assert_eq!(s::ttyname_r(p.s, b.as_mut_ptr(), b.len()), 0);
    assert_eq!(stored(&b), p.name());
    errno::set_raw(0);
    assert_eq!((s::ttyname_r(p.s, b.as_mut_ptr(), 3), errno::get()), (ERANGE, ERANGE), "returned and stored");
    assert_eq!(s::ttyname_r(p.s, null_mut(), 64), EINVAL);
    assert_eq!(s::ttyname_r(-1, b.as_mut_ptr(), b.len()), EBADF);
}

#[test]
fn sym___ttyname_r_chk() {
    exported("__ttyname_r_chk");
    let p = Pair::new();
    let mut b = [0 as c_char; 64];
    assert_eq!(s::__ttyname_r_chk(p.s, b.as_mut_ptr(), 64, 64), 0);
    assert_eq!(stored(&b), p.name());
    assert_eq!(s::__ttyname_r_chk(p.s, b.as_mut_ptr(), 5, 64), ERANGE);
}

// ------------------------------------------------------------------------------ §19.4

#[test]
fn sym_posix_openpt() {
    exported("posix_openpt");
    let m = s::posix_openpt((O_RDWR | O_NOCTTY | O_CLOEXEC) as i32);
    assert!(m >= 0);
    assert_eq!(CSys.fcntl_int(Fd::new(m), FcntlInt::GetFd, 0).unwrap(), 1, "O_CLOEXEC kept");
    let _ = CSys.close(Fd::new(m));
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::posix_openpt(-1))), (-1, EINVAL));
}

#[test]
fn sym_getpt() {
    exported("getpt");
    let m = s::getpt();
    assert!(m >= 0);
    assert_eq!(CSys.fcntl_int(Fd::new(m), FcntlInt::GetFl, 0).unwrap() & 3, 2, "O_RDWR");
    assert_eq!(s::isatty(m), 1);
    let _ = CSys.close(Fd::new(m));
}

#[test]
fn sym_grantpt() {
    exported("grantpt");
    let p = Pair::new();
    assert_eq!(s::grantpt(p.m), 0);
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::grantpt(p.s))), (-1, EINVAL), "not a master");
    assert_eq!(fails(i64::from(s::grantpt(-1))), (-1, EBADF));
}

#[test]
fn sym_unlockpt() {
    exported("unlockpt");
    let m = s::posix_openpt((O_RDWR | O_NOCTTY | O_CLOEXEC) as i32);
    let name = format!("/dev/pts/{}\0", CSys.ioctl_get_int(Fd::new(m), IoctlIntOut::Tiocgptn).unwrap());
    let open = || CSys.openat(Fd::CWD, KStr::from_nul_terminated(name.as_bytes()).unwrap(), O_RDWR | O_NOCTTY | O_CLOEXEC, 0);
    assert_eq!(open().err().map(liella_kernel::errno::Errno::get), Some(EIO), "locked");
    assert_eq!(s::unlockpt(m), 0);
    let _ = CSys.close(open().unwrap());
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::unlockpt(null()))), (-1, EINVAL));
    let _ = CSys.close(Fd::new(m));
}

#[test]
fn sym_ptsname() {
    exported("ptsname");
    let p = Pair::new();
    assert_eq!(text(s::ptsname(p.m)), p.name());
    errno::set_raw(0);
    assert!(s::ptsname(p.s).is_null());
    assert_eq!(errno::get(), ENOTTY);
}

#[test]
fn sym_ptsname_r() {
    exported("ptsname_r");
    let p = Pair::new();
    let mut b = [b'z' as c_char; 64];
    let len = p.name().len();
    errno::set_raw(0);
    assert_eq!((s::ptsname_r(p.m, b.as_mut_ptr(), len), errno::get()), (ERANGE, ERANGE));
    assert_eq!(b[0], b'z' as c_char, "untouched");
    assert_eq!(s::ptsname_r(p.m, b.as_mut_ptr(), len + 1), 0);
    assert_eq!(stored(&b), p.name());
    assert_eq!(s::ptsname_r(p.m, null_mut(), 64), EINVAL);
    assert_eq!(s::ptsname_r(p.s, b.as_mut_ptr(), 64), ENOTTY);
}

#[test]
fn sym___ptsname_r_chk() {
    exported("__ptsname_r_chk");
    let p = Pair::new();
    let mut b = [0 as c_char; 64];
    assert_eq!(s::__ptsname_r_chk(p.m, b.as_mut_ptr(), 64, 64), 0);
    assert_eq!(stored(&b), p.name());
}

#[test]
fn sym_openpty() {
    exported("openpty");
    let (mut m, mut s2) = (-1, -1);
    let mut name = [0 as c_char; 32];
    let ws = struct_winsize { ws_row: 5, ws_col: 6, ws_xpixel: 0, ws_ypixel: 0 };
    assert_eq!(s::openpty(Some(&mut m), Some(&mut s2), name.as_mut_ptr(), None, Some(&ws)), 0);
    let p = Pair { m, s: s2 };
    assert_eq!(stored(&name), p.name());
    let w = CSys.tiocgwinsz(Fd::new(p.s)).unwrap();
    assert_eq!((w.row, w.col), (5, 6));
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::openpty(None, Some(&mut s2), null_mut(), None, None))), (-1, EFAULT));
}

#[test]
fn sym_forkpty() {
    exported("forkpty");
    conformance(&["s19_terminals"]);
}

// ------------------------------------------------------------------------------ §19.5

#[test]
fn sym_setttyent() {
    exported("setttyent");
    alone("sym_setttyent", || {
        errno::set_raw(0);
        let r = s::setttyent();
        if std::path::Path::new("/etc/ttys").exists() {
            assert_eq!(r, 1);
        } else {
            assert_eq!((r, errno::get()), (0, ENOENT));
        }
        s::endttyent();
    });
}

#[test]
fn sym_endttyent() {
    exported("endttyent");
    alone("sym_endttyent", || {
        assert_eq!(s::endttyent(), 1);
        assert_eq!(s::endttyent(), 1, "also when nothing is open");
    });
}

#[test]
fn sym_getttyent() {
    exported("getttyent");
    alone("sym_getttyent", || {
        if !std::path::Path::new("/etc/ttys").exists() {
            errno::set_raw(0);
            assert!(s::getttyent().is_null());
            assert_eq!(errno::get(), ENOENT);
        }
        s::endttyent();
    });
}

#[test]
fn sym_getttynam() {
    exported("getttynam");
    alone("sym_getttynam", || {
        errno::set_raw(0);
        assert!(s::getttynam(c"no-such-terminal".as_ptr()).is_null());
        if !std::path::Path::new("/etc/ttys").exists() {
            assert_eq!(errno::get(), ENOENT);
        }
    });
}

#[test]
fn sym_gtty() {
    exported("gtty");
    let mut sg = [0u64; 4];
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::gtty(0, sg.as_mut_ptr().cast()))), (-1, ENOSYS));
}

#[test]
fn sym_stty() {
    exported("stty");
    let sg = [0u64; 4];
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::stty(0, sg.as_ptr().cast()))), (-1, ENOSYS));
}
