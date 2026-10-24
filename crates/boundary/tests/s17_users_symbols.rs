// Specified from spec §17 (phase 5: §17.1, the credential queries; phase 6: credential
// changes, checked through the conformance programs since they change every thread's
// credentials; phase 10: §17.3–17.6, the user, group, shadow and gshadow databases, the login
// name, login shells, login accounting, `initgroups` and `login_tty`).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Translation-layer tests for the exported symbols of spec §17.
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//! The credential answers are compared with the kernel's own record, `/proc/self/status`; the
//! database lookups with the system's files as the test reads them itself (read only), the
//! stream, string and login-record functions with fixture files. Process-wide state (the
//! enumeration cursors, the `utmp` file name, the shell list, the password lock) is used only in
//! re-executed children. The system's `utmp`, `wtmp` and lock file are never written: the calls
//! that would write them run only unprivileged, where they are refused.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(non_snake_case)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::arithmetic_side_effects, missing_docs)]
#[path = "../../core/tests/support/skip_note.rs"]
mod skip_note;

use core::ffi::CStr;
use core::ptr::null_mut;
use std::ffi::CString;

use liella_abi::ctypes::{c_char, c_int, c_void};
use liella_abi::layout::{struct_exit_status, struct_group, struct_passwd, struct_sgrp, struct_spwd, struct_utmp, struct_utmp__ut_tv};
use liella_boundary::posix::s04_strings as st;
use liella_boundary::posix::s12_stdio as sio;
use liella_boundary::sys::CSys;
use liella_kernel::ops::{IoctlIntArg, IoctlIntIn, IoctlIntOut};
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{O_CLOEXEC, O_NOCTTY, O_RDWR};
use liella_kernel::Fd;
use liella_legacy::accounting as acct;

use liella_boundary::errno;
use liella_boundary::posix::s17_users as s;

mod products;
#[path = "../../core/tests/support/scratch.rs"]
mod scratch;
use products::{conformance, exported};

const EFAULT: i32 = 14;
const EINVAL: i32 = 22;

/// The four numbers of a `/proc/self/status` line (`Uid:`, `Gid:`) or all of `Groups:`.
fn status(key: &str) -> Vec<u32> {
    let text = std::fs::read_to_string("/proc/self/status").unwrap();
    let line = text.lines().find(|l| l.starts_with(key)).unwrap();
    line[key.len()..].split_whitespace().map(|v| v.parse().unwrap()).collect()
}

// ------------------------------------------------------------------------------ §17.3–17.6 helpers

const PROBE: &str = "LIELLA_S17_PROBE";
const ENOENT: i32 = 2;
const ENXIO: i32 = 6;
const EACCES: i32 = 13;
const ESRCH: i32 = 3;
const ERANGE: i32 = 34;
const ENOSYS: i32 = 38;
const EPERM: i32 = 1;
const EOVERFLOW: i32 = 75;

/// The probe this process was re-executed for, if any.
fn probe() -> Option<String> {
    std::env::var(PROBE).ok()
}

/// Run the test `test` again in a child process with the probe `test` set, for process-wide
/// state (the enumeration cursors, the utmp file name, the shell list, the password lock); the
/// child's own assertions decide.
fn in_child(test: &str) {
    let o = std::process::Command::new(std::env::current_exe().unwrap()).args([test, "--exact", "--test-threads=1", "--nocapture"]).env(PROBE, test).output().unwrap();
    assert!(o.status.success(), "child {test}: {:?}\n{}{}", o.status, String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

/// A fresh path under the target directory, with `content` when given (removed when the first
/// value is dropped), and the path as a C string.
fn fixture(tag: &str, content: Option<&str>) -> (scratch::Scratch, CString) {
    let p = scratch::Scratch::path(&format!("s17-{tag}"));
    if let Some(c) = content {
        std::fs::write(&p, c).unwrap();
    }
    let c = p.c();
    (p, c)
}

/// A stream on the file `path`.
fn stream(path: &CString, mode: &CStr) -> *mut c_void {
    let fp = sio::fopen(path.as_ptr(), mode.as_ptr());
    assert!(!fp.is_null(), "fopen {path:?}");
    fp.cast()
}

fn close_stream(fp: *mut c_void) {
    assert_eq!(sio::fclose(fp.cast()), 0);
}

/// The C string at `a`.
fn text_at(a: usize) -> String {
    assert_ne!(a, 0, "a string pointer");
    let p = a as *const c_char;
    let n = st::strlen(p);
    let mut v = vec![0u8; n];
    st::memcpy(v.as_mut_ptr().cast(), p.cast(), n);
    String::from_utf8(v).unwrap()
}

/// A copy of the `T` at `a`.
fn copy_at<T: Copy>(a: usize, mut v: T) -> T {
    assert_ne!(a, 0, "a record pointer");
    st::memcpy(core::ptr::from_mut(&mut v).cast(), a as *const c_void, size_of::<T>());
    v
}

/// The strings of a NULL-terminated array at `a`.
fn list_at(a: usize) -> Vec<String> {
    let mut out = Vec::new();
    for k in 0.. {
        let p = copy_at(a + k * 8, 0usize);
        if p == 0 {
            return out;
        }
        out.push(text_at(p));
    }
    out
}

const PW0: struct_passwd = struct_passwd { pw_name: 0, pw_passwd: 0, pw_uid: 0, pw_gid: 0, pw_gecos: 0, pw_dir: 0, pw_shell: 0 };
const GR0: struct_group = struct_group { gr_name: 0, gr_passwd: 0, gr_gid: 0, gr_mem: 0 };
const SP0: struct_spwd = struct_spwd { sp_namp: 0, sp_pwdp: 0, sp_lstchg: 0, sp_min: 0, sp_max: 0, sp_warn: 0, sp_inact: 0, sp_expire: 0, sp_flag: 0 };
const SG0: struct_sgrp = struct_sgrp { sg_namp: 0, sg_passwd: 0, sg_adm: 0, sg_mem: 0 };
const UT0: struct_utmp = struct_utmp {
    ut_type: 0,
    ut_pid: 0,
    ut_line: [0; 32],
    ut_id: [0; 4],
    ut_user: [0; 32],
    ut_host: [0; 256],
    ut_exit: struct_exit_status { e_termination: 0, e_exit: 0 },
    ut_session: 0,
    ut_tv: struct_utmp__ut_tv { tv_sec: 0, tv_usec: 0 },
    ut_addr_v6: [0; 4],
    __glibc_reserved: [0; 20],
};

/// `name:passwd:uid:gid:gecos:dir:shell` of a record.
fn pw_text(p: &struct_passwd) -> String {
    format!("{}:{}:{}:{}:{}:{}:{}", text_at(p.pw_name), text_at(p.pw_passwd), p.pw_uid, p.pw_gid, text_at(p.pw_gecos), text_at(p.pw_dir), text_at(p.pw_shell))
}

/// `name:passwd:gid:members` of a record.
fn gr_text(g: &struct_group) -> String {
    format!("{}:{}:{}:{}", text_at(g.gr_name), text_at(g.gr_passwd), g.gr_gid, list_at(g.gr_mem).join(","))
}

/// `name:passwd:lstchg:min:max:warn:inact:expire:flag` of a record.
fn sp_text(s: &struct_spwd) -> String {
    format!("{}:{}:{}:{}:{}:{}:{}:{}:{}", text_at(s.sp_namp), text_at(s.sp_pwdp), s.sp_lstchg, s.sp_min, s.sp_max, s.sp_warn, s.sp_inact, s.sp_expire, s.sp_flag as i64)
}

/// `name:passwd:admins:members` of a record.
fn sg_text(g: &struct_sgrp) -> String {
    format!("{}:{}:{}:{}", text_at(g.sg_namp), text_at(g.sg_passwd), list_at(g.sg_adm).join(","), list_at(g.sg_mem).join(","))
}

/// The record lines of a system file as the test reads it itself: fields of the non-comment
/// lines.
fn system_records(path: &str) -> Vec<Vec<String>> {
    let text = std::fs::read_to_string(path).unwrap_or_default();
    text.lines().filter(|l| !l.trim_start().is_empty() && !l.trim_start().starts_with('#')).map(|l| l.trim_start().split(':').map(str::to_string).collect()).collect()
}

/// The `/etc/passwd` record of `pred`, as `pw_text` would print it.
fn passwd_line(pred: impl Fn(&[String]) -> bool) -> Option<String> {
    system_records("/etc/passwd").into_iter().find(|f| f.len() >= 7 && !f[0].starts_with(['+', '-']) && pred(f)).map(|f| format!("{}:{}", f[..6].join(":"), f[6..].join(":")))
}

/// The `/etc/group` record of `pred`, as `gr_text` would print it.
fn group_line(pred: impl Fn(&[String]) -> bool) -> Option<String> {
    system_records("/etc/group").into_iter().find(|f| f.len() >= 4 && !f[0].starts_with(['+', '-']) && pred(f)).map(|f| {
        let mem: Vec<&str> = f[3..].join(":").leak().split(',').map(str::trim_start).filter(|m| !m.is_empty()).collect();
        format!("{}:{}:{}:{}", f[0], f[1], f[2], mem.join(","))
    })
}

/// A login record.
fn ut(kind: i16, pid: i32, line: &str, id: &str, user: &str) -> struct_utmp {
    let mut u = UT0;
    let put = |f: &mut [i8], t: &str| {
        for (d, s) in f.iter_mut().zip(t.bytes()) {
            *d = s as i8;
        }
    };
    u.ut_type = kind;
    u.ut_pid = pid;
    put(&mut u.ut_line, line);
    put(&mut u.ut_id, id);
    put(&mut u.ut_user, user);
    put(&mut u.ut_host, "host.example");
    u
}

/// The text of a fixed field.
fn field(f: &[i8]) -> String {
    f.iter().take_while(|&&c| c != 0).map(|&c| c as u8 as char).collect()
}

/// `type line id user` of a login record.
fn ut_text(u: &struct_utmp) -> String {
    format!("{} {} {} {}", u.ut_type, field(&u.ut_line), field(&u.ut_id), field(&u.ut_user))
}

const LOGIN_PROCESS: i16 = 6;
const USER_PROCESS: i16 = 7;
const DEAD_PROCESS: i16 = 8;
const BOOT_TIME: i16 = 2;

/// A pseudo-terminal: the controller and the path of its terminal.
fn pty() -> (Fd, String) {
    let m = CSys.openat(Fd::CWD, KStr::from_nul_terminated(b"/dev/ptmx\0").unwrap(), O_RDWR | O_NOCTTY | O_CLOEXEC, 0).unwrap();
    CSys.ioctl_set_int(m, IoctlIntIn::Tiocsptlck, 0).unwrap();
    let n = CSys.ioctl_get_int(m, IoctlIntOut::Tiocgptn).unwrap();
    (m, format!("/dev/pts/{n}"))
}

/// Everything readable on `fd` within about `ms` milliseconds.
fn drain(fd: Fd, ms: u64) -> Vec<u8> {
    let mut out = Vec::new();
    let end = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    while std::time::Instant::now() < end {
        let mut p = [liella_kernel::uapi::PollFd { fd: fd.raw(), events: 1, revents: 0 }];
        let mut t = liella_kernel::uapi::Timespec { sec: 0, nsec: 20_000_000 };
        if CSys.ppoll(&mut p, Some(&mut t), None).unwrap_or(0) > 0 {
            let mut b = [0u8; 256];
            match CSys.read(fd, &mut b) {
                Ok(n) if n > 0 => out.extend_from_slice(&b[..n]),
                _ => break,
            }
        }
    }
    out
}

const INIT_PROCESS: i16 = 5;

/// What `getlogin` should answer: the user of the audit login UID, or `ENXIO` without a
/// terminal; `Err(-1)` when the test cannot tell (a terminal without the UID's user).
fn expected_login() -> Result<String, i32> {
    let uid: u32 = std::fs::read_to_string("/proc/self/loginuid").ok().and_then(|t| t.trim().parse().ok()).unwrap_or(u32::MAX);
    if uid != u32::MAX {
        if let Some(l) = passwd_line(|f| f[2] == uid.to_string()) {
            return Ok(l.split(':').next().unwrap().to_string());
        }
    }
    if (0..3).any(|fd| CSys.tcgets(Fd::new(fd)).is_ok()) {
        return Err(-1);
    }
    Err(ENXIO)
}

#[test]
fn sym_endgrent() {
    if probe().is_none() {
        return in_child("sym_endgrent");
    }
    let first = s::getgrent() as usize;
    let name = text_at(copy_at(first, GR0).gr_name);
    s::getgrent();
    s::endgrent();
    assert_eq!(text_at(copy_at(s::getgrent() as usize, GR0).gr_name), name, "after endgrent the enumeration starts again");
    s::endgrent();
}

#[test]
fn sym_endpwent() {
    if probe().is_none() {
        return in_child("sym_endpwent");
    }
    let name = text_at(copy_at(s::getpwent() as usize, PW0).pw_name);
    s::getpwent();
    s::endpwent();
    assert_eq!(text_at(copy_at(s::getpwent() as usize, PW0).pw_name), name, "after endpwent the enumeration starts again");
    s::endpwent();
}

#[test]
fn sym_endsgent() {
    if probe().is_none() {
        return in_child("sym_endsgent");
    }
    // /etc/gshadow is not readable unprivileged: each step reports it; endsgent leaves nothing open.
    errno::set_raw(0);
    let p = s::getsgent();
    if p.is_null() {
        assert!(matches!(errno::get(), EACCES | ENOENT), "errno {}", errno::get());
    }
    s::endsgent();
    s::endsgent();
}

#[test]
fn sym_endspent() {
    if probe().is_none() {
        return in_child("sym_endspent");
    }
    errno::set_raw(0);
    let p = s::getspent();
    if p.is_null() {
        assert!(matches!(errno::get(), EACCES | ENOENT), "errno {}", errno::get());
    }
    s::endspent();
    s::endspent();
}

#[test]
fn sym_endusershell() {
    if probe().is_none() {
        return in_child("sym_endusershell");
    }
    let first = s::getusershell() as usize;
    assert_ne!(first, 0, "a shell (or the default /bin/sh)");
    let first = text_at(first);
    while !s::getusershell().is_null() {}
    s::endusershell();
    assert_eq!(text_at(s::getusershell() as usize), first, "endusershell: the list starts again");
}

#[test]
fn sym_endutent() {
    if probe().is_none() {
        return in_child("sym_endutent");
    }
    let (_g, path) = fixture("endut", Some(""));
    assert_eq!(s::utmpname(path.as_ptr()), 0);
    let a = ut(USER_PROCESS, 1, "pts/1", "p1", "alice");
    s::setutent();
    assert!(!s::pututline(Some(&a)).is_null());
    s::endutent();
    // Closed: the next call opens the file again from its start.
    assert_eq!(ut_text(&copy_at(s::getutent() as usize, UT0)), "7 pts/1 p1 alice");
    s::endutent();
}

#[test]
fn sym_endutxent() {
    if probe().is_none() {
        return in_child("sym_endutxent");
    }
    let (_g, path) = fixture("endutx", Some(""));
    assert_eq!(s::utmpxname(path.as_ptr()), 0);
    let a = ut(LOGIN_PROCESS, 1, "tty1", "1", "LOGIN");
    s::setutxent();
    assert!(!s::pututxline(Some(&a)).is_null());
    s::endutxent();
    assert_eq!(ut_text(&copy_at(s::getutxent() as usize, UT0)), "6 tty1 1 LOGIN");
    s::endutxent();
}

#[test]
fn sym_fgetgrent() {
    let (_g, path) = fixture("fgetgrent", Some("# comment\nroot:x:0:\nshort:x\nusers:x:100: alice ,,bob\n+:::\nlast:x:7:z"));
    let fp = stream(&path, c"r");
    let mut got = Vec::new();
    loop {
        errno::set_raw(0);
        let g = s::fgetgrent(fp);
        if g.is_null() {
            assert_eq!(errno::get(), ENOENT, "the end is ENOENT");
            break;
        }
        got.push(gr_text(&copy_at(g as usize, GR0)));
    }
    assert_eq!(got, ["root:x:0:", "users:x:100:alice ,bob", "+::0:", "last:x:7:z"]);
    close_stream(fp);
}

#[test]
fn sym_fgetgrent_r() {
    let (_g, path) = fixture("fgetgrent_r", Some("users:x:100:alice,bob\nwheel:x:10:alice\n"));
    let fp = stream(&path, c"r");
    let (mut g, mut res) = (GR0, 1usize);
    let mut buf = vec![0u8; 64];
    assert_eq!(s::fgetgrent_r(fp, Some(&mut g), buf.as_mut_ptr().cast(), 8, Some(&mut res)), ERANGE);
    assert_eq!((res, sio::ftell(fp.cast())), (0, 0), "a buffer too small: no result, the stream back at the record");
    assert_eq!(s::fgetgrent_r(fp, Some(&mut g), buf.as_mut_ptr().cast(), 64, Some(&mut res)), 0);
    assert_eq!(res, core::ptr::from_mut(&mut g) as usize);
    assert_eq!(gr_text(&g), "users:x:100:alice,bob");
    assert_eq!(g.gr_mem % 8, 0, "the member array is aligned");
    // An odd buffer start: still aligned inside it.
    assert_eq!(s::fgetgrent_r(fp, Some(&mut g), buf[3..].as_mut_ptr().cast(), 60, Some(&mut res)), 0);
    assert_eq!((gr_text(&g), g.gr_mem % 8), ("wheel:x:10:alice".to_string(), 0));
    assert_eq!(s::fgetgrent_r(fp, Some(&mut g), buf.as_mut_ptr().cast(), 64, Some(&mut res)), ENOENT);
    assert_eq!(res, 0);
    close_stream(fp);
}

#[test]
fn sym_fgetpwent() {
    let (_g, path) = fixture("fgetpwent", Some("root:x:0:0:root:/root:/bin/sh\n\n  spaced:x:1:2:g:/:/s:extra\nshort:x:5\nhex:x:0x10:1::/:\n+nis::::::\nnonl:x:6:6:N:/n:/bin/sh"));
    let fp = stream(&path, c"r");
    let mut got = Vec::new();
    loop {
        errno::set_raw(0);
        let p = s::fgetpwent(fp);
        if p.is_null() {
            assert_eq!(errno::get(), ENOENT);
            break;
        }
        got.push(pw_text(&copy_at(p as usize, PW0)));
    }
    assert_eq!(got, ["root:x:0:0:root:/root:/bin/sh", "spaced:x:1:2:g:/:/s:extra", "+nis::0:0:::", "nonl:x:6:6:N:/n:/bin/sh"]);
    close_stream(fp);
}

#[test]
fn sym_fgetpwent_r() {
    let (_g, path) = fixture("fgetpwent_r", Some("# c\nroot:x:0:0:root:/root:/bin/bash\n"));
    let fp = stream(&path, c"r");
    let (mut p, mut res) = (PW0, 1usize);
    let mut buf = [0u8; 64];
    // The record's strings are 28 bytes with their terminators.
    assert_eq!(s::fgetpwent_r(fp, Some(&mut p), buf.as_mut_ptr().cast(), 27, Some(&mut res)), ERANGE);
    assert_eq!((res, sio::ftell(fp.cast())), (0, 4), "the stream is back where the refused record starts");
    assert_eq!(s::fgetpwent_r(fp, Some(&mut p), buf.as_mut_ptr().cast(), 28, Some(&mut res)), 0);
    assert_eq!((pw_text(&p), res), ("root:x:0:0:root:/root:/bin/bash".to_string(), core::ptr::from_mut(&mut p) as usize));
    assert_eq!(s::fgetpwent_r(fp, Some(&mut p), buf.as_mut_ptr().cast(), 64, Some(&mut res)), ENOENT);
    assert_eq!(s::fgetpwent_r(fp, Some(&mut p), buf.as_mut_ptr().cast(), 64, None), 14, "no result cell: EFAULT");
    close_stream(fp);
}

#[test]
fn sym_fgetsgent_r() {
    let (_g, path) = fixture("fgetsgent_r", Some("g:p:a1,a2:m1\nshort\n"));
    let fp = stream(&path, c"r");
    let (mut g, mut res) = (SG0, 0usize);
    let mut buf = [0u8; 128];
    assert_eq!(s::fgetsgent_r(fp, Some(&mut g), buf.as_mut_ptr().cast(), 16, Some(&mut res)), ERANGE);
    assert_eq!(s::fgetsgent_r(fp, Some(&mut g), buf.as_mut_ptr().cast(), 128, Some(&mut res)), 0);
    assert_eq!(sg_text(&g), "g:p:a1,a2:m1");
    assert_eq!(s::fgetsgent_r(fp, Some(&mut g), buf.as_mut_ptr().cast(), 128, Some(&mut res)), 0);
    assert_eq!(sg_text(&g), "short:::", "only a name: the other fields empty");
    assert_eq!(s::fgetsgent_r(fp, Some(&mut g), buf.as_mut_ptr().cast(), 128, Some(&mut res)), ENOENT);
    close_stream(fp);
}

#[test]
fn sym_fgetsgent() {
    let (_g, path) = fixture("fgetsgent", Some("root:*::\nusers:!:alice:alice,,bob\n#x\n"));
    let fp = stream(&path, c"r");
    let a = s::fgetsgent(fp) as usize;
    assert_eq!(sg_text(&copy_at(a, SG0)), "root:*::");
    let b = s::fgetsgent(fp) as usize;
    assert_eq!(sg_text(&copy_at(b, SG0)), "users:!:alice:alice,bob");
    assert_eq!(a, b, "one record block per thread");
    errno::set_raw(0);
    assert!(s::fgetsgent(fp).is_null());
    assert_eq!(errno::get(), ENOENT);
    close_stream(fp);
}

#[test]
fn sym_fgetspent() {
    let (_g, path) = fixture("fgetspent", Some("root:!:19000:0:99999:7:::\nbob:*:::::::\nneg:x:-5:1:2:3:4:5:\nflag:x:1:2:3:4:5:6:7\n"));
    let fp = stream(&path, c"r");
    let mut got = Vec::new();
    loop {
        let p = s::fgetspent(fp);
        if p.is_null() {
            break;
        }
        got.push(sp_text(&copy_at(p as usize, SP0)));
    }
    assert_eq!(got, ["root:!:19000:0:99999:7:-1:-1:-1", "bob:*:-1:-1:-1:-1:-1:-1:-1", "flag:x:1:2:3:4:5:6:7"]);
    close_stream(fp);
}

#[test]
fn sym_fgetspent_r() {
    let (_g, path) = fixture("fgetspent_r", Some("alice:$6$salt$hash:19500:1:90:14:30:20000:\n"));
    let fp = stream(&path, c"r");
    let (mut sp, mut res) = (SP0, 0usize);
    let mut buf = [0u8; 64];
    assert_eq!(s::fgetspent_r(fp, Some(&mut sp), buf.as_mut_ptr().cast(), 10, Some(&mut res)), ERANGE);
    assert_eq!(sio::ftell(fp.cast()), 0);
    assert_eq!(s::fgetspent_r(fp, Some(&mut sp), buf.as_mut_ptr().cast(), 64, Some(&mut res)), 0);
    assert_eq!(sp_text(&sp), "alice:$6$salt$hash:19500:1:90:14:30:20000:-1");
    assert_eq!(s::fgetspent_r(fp, Some(&mut sp), buf.as_mut_ptr().cast(), 64, Some(&mut res)), ENOENT);
    close_stream(fp);
}

#[test]
fn sym_getegid() {
    assert_eq!(s::getegid(), status("Gid:")[1], "the effective GID");
}

#[test]
fn sym_geteuid() {
    assert_eq!(s::geteuid(), status("Uid:")[1], "the effective UID");
}

#[test]
fn sym_getgid() {
    assert_eq!(s::getgid(), status("Gid:")[0], "the real GID");
}

#[test]
fn sym_getgrent() {
    if probe().is_none() {
        return in_child("sym_getgrent");
    }
    s::setgrent();
    let mut names = Vec::new();
    loop {
        errno::set_raw(77);
        let g = s::getgrent();
        if g.is_null() {
            assert_eq!(errno::get(), 77, "the end leaves errno unchanged (D-17.3)");
            break;
        }
        names.push(text_at(copy_at(g as usize, GR0).gr_name));
    }
    s::endgrent();
    let want: Vec<String> = system_records("/etc/group").into_iter().filter(|f| f.len() >= 3 && !f[0].is_empty() && !f[2].is_empty() && f[2].bytes().all(|c| c.is_ascii_digit())).map(|f| f[0].clone()).collect();
    assert_eq!(names, want);
}

#[test]
fn sym_getgrent_r() {
    if probe().is_none() {
        return in_child("sym_getgrent_r");
    }
    s::setgrent();
    let (mut g, mut res) = (GR0, 0usize);
    let mut buf = vec![0u8; 4096];
    assert_eq!(s::getgrent_r(Some(&mut g), buf.as_mut_ptr().cast(), 1, Some(&mut res)), ERANGE);
    assert_eq!(res, 0);
    assert_eq!(s::getgrent_r(Some(&mut g), buf.as_mut_ptr().cast(), 4096, Some(&mut res)), 0, "the refused record again");
    let first = gr_text(&g);
    assert_eq!(Some(first), group_line(|_| true), "the first group of /etc/group");
    while s::getgrent_r(Some(&mut g), buf.as_mut_ptr().cast(), 4096, Some(&mut res)) == 0 {}
    assert_eq!(s::getgrent_r(Some(&mut g), buf.as_mut_ptr().cast(), 4096, Some(&mut res)), ENOENT);
    s::endgrent();
}

#[test]
fn sym_getgrgid() {
    let g = s::getgrgid(0) as usize;
    assert_eq!(Some(gr_text(&copy_at(g, GR0))), group_line(|f| f[2] == "0"));
    assert_eq!(s::getgrgid(0) as usize, g, "one record block per thread");
    let other = std::thread::spawn(|| s::getgrgid(0) as usize).join().unwrap();
    assert_ne!(other, g, "another thread has its own");
    errno::set_raw(0);
    assert!(s::getgrgid(0x7fff_fff0).is_null());
    assert_eq!(errno::get(), 0, "not found leaves errno");
}

#[test]
fn sym_getgrgid_r() {
    let (mut g, mut res) = (GR0, 1usize);
    let mut buf = vec![0u8; 1024];
    assert_eq!(s::getgrgid_r(0, Some(&mut g), buf.as_mut_ptr().cast(), 1024, Some(&mut res)), 0);
    assert_eq!((Some(gr_text(&g)), res), (group_line(|f| f[2] == "0"), core::ptr::from_mut(&mut g) as usize));
    assert_eq!(s::getgrgid_r(0, Some(&mut g), buf.as_mut_ptr().cast(), 2, Some(&mut res)), ERANGE);
    assert_eq!(res, 0);
    assert_eq!(s::getgrgid_r(0x7fff_fff0, Some(&mut g), buf.as_mut_ptr().cast(), 1024, Some(&mut res)), 0);
    assert_eq!(res, 0, "not found: 0 with a NULL result");
}

#[test]
fn sym_getgrnam() {
    let g = s::getgrnam(c"root".as_ptr()) as usize;
    assert_eq!(Some(gr_text(&copy_at(g, GR0))), group_line(|f| f[0] == "root"));
    errno::set_raw(0);
    assert!(s::getgrnam(c"no-such-group-here".as_ptr()).is_null());
    assert!(s::getgrnam(c"".as_ptr()).is_null());
    assert_eq!(errno::get(), 0);
}

#[test]
fn sym_getgrnam_r() {
    let (mut g, mut res) = (GR0, 0usize);
    let mut buf = vec![0u8; 1024];
    assert_eq!(s::getgrnam_r(c"root".as_ptr(), Some(&mut g), buf.as_mut_ptr().cast(), 1024, Some(&mut res)), 0);
    assert_eq!(Some(gr_text(&g)), group_line(|f| f[0] == "root"));
    assert_eq!(s::getgrnam_r(c"root".as_ptr(), Some(&mut g), core::ptr::null_mut(), 0, Some(&mut res)), ERANGE, "no buffer at all");
    assert_eq!(s::getgrnam_r(c"no-such-group-here".as_ptr(), Some(&mut g), buf.as_mut_ptr().cast(), 1024, Some(&mut res)), 0);
    assert_eq!(res, 0);
}

#[test]
fn sym_getgrouplist() {
    // The calling user's groups: the base GID first, then each group listing the user once.
    let Some(me) = passwd_line(|f| f[2] == s::getuid().to_string()) else {
        skip_note::skipped("skipped: the user is not in /etc/passwd");
        return;
    };
    let name = CString::new(me.split(':').next().unwrap()).unwrap();
    let base = 0x7fff_ff00;
    let mut want = vec![base];
    for f in system_records("/etc/group") {
        if f.len() >= 4 && !f[0].starts_with(['+', '-']) && f[3].split(',').any(|m| m.trim_start() == name.to_str().unwrap()) {
            let gid: u32 = f[2].parse().unwrap();
            if !want.contains(&gid) {
                want.push(gid);
            }
        }
    }
    let mut list = vec![0u32; 64];
    let mut n: c_int = 64;
    assert_eq!(s::getgrouplist(name.as_ptr(), base, list.as_mut_ptr(), Some(&mut n)), want.len() as c_int);
    assert_eq!(&list[..n as usize], &want[..]);
    let mut short: c_int = 0;
    assert_eq!(s::getgrouplist(name.as_ptr(), base, list.as_mut_ptr(), Some(&mut short)), -1, "no room");
    assert_eq!(short, want.len() as c_int, "the count needed");
    let mut one = [0u32; 1];
    let mut n1: c_int = 1;
    assert_eq!(s::getgrouplist(c"no-such-user-here".as_ptr(), 4242, one.as_mut_ptr(), Some(&mut n1)), 1);
    assert_eq!((one[0], n1), (4242, 1), "an unknown user has the base group alone");
}

#[test]
fn sym_getgroups() {
    let n = s::getgroups(0, null_mut());
    let mut want = status("Groups:");
    assert_eq!(n as usize, want.len(), "size 0: the count");
    let mut list = vec![0u32; n as usize];
    assert_eq!(s::getgroups(n, list.as_mut_ptr()), n);
    list.sort_unstable();
    want.sort_unstable();
    assert_eq!(list, want);
    if n > 0 {
        errno::set_raw(0);
        assert_eq!((s::getgroups(n - 1, list.as_mut_ptr()), errno::get()), (-1, EINVAL), "too small");
    }
    assert_eq!((s::getgroups(-1, list.as_mut_ptr()), errno::get()), (-1, EINVAL));
}

#[test]
fn sym___getgroups_chk() {
    if std::env::var("LIELLA_S17_CHK").is_ok() {
        let mut list = [0u32; 1];
        s::__getgroups_chk(2, list.as_mut_ptr(), 4);
        return;
    }
    let n = s::getgroups(0, null_mut());
    let mut list = vec![0u32; n as usize];
    assert_eq!(s::__getgroups_chk(n, list.as_mut_ptr(), list.len() * 4), n);
    // A negative size is getgroups' EINVAL, not an overflow (the reference; validation T10).
    errno::set_raw(0);
    assert_eq!((s::__getgroups_chk(-1, list.as_mut_ptr(), list.len() * 4), errno::get()), (-1, 22));
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["sym___getgroups_chk", "--exact", "--test-threads=1"])
        .env("LIELLA_S17_CHK", "1")
        .output()
        .unwrap();
    assert_eq!(std::os::unix::process::ExitStatusExt::signal(&o.status), Some(6), "more entries than the buffer holds: the fortify abort");
}

#[test]
fn sym_getlogin() {
    let p = s::getlogin() as usize;
    match expected_login() {
        Ok(name) => {
            assert_eq!(text_at(p), name);
            assert_eq!(s::getlogin() as usize, p, "one buffer per thread");
        }
        Err(-1) => skip_note::skipped("skipped: a terminal, and no user for the login UID"),
        Err(e) => {
            assert_eq!(p, 0);
            assert_eq!(errno::get(), e);
        }
    }
}

#[test]
fn sym_getlogin_r() {
    let mut buf = [0u8; 300];
    match expected_login() {
        Ok(name) => {
            assert_eq!(s::getlogin_r(buf.as_mut_ptr().cast(), 300), 0);
            assert_eq!(&buf[..=name.len()], format!("{name}\0").as_bytes());
            assert_eq!(s::getlogin_r(buf.as_mut_ptr().cast(), name.len()), ERANGE, "no room for the terminator");
            assert_eq!(s::getlogin_r(buf.as_mut_ptr().cast(), name.len() + 1), 0);
        }
        Err(-1) => skip_note::skipped("skipped: a terminal, and no user for the login UID"),
        Err(e) => assert_eq!(s::getlogin_r(buf.as_mut_ptr().cast(), 300), e),
    }
}

#[test]
fn sym___getlogin_r_chk() {
    if probe().is_some() {
        let mut buf = [0u8; 8];
        s::__getlogin_r_chk(buf.as_mut_ptr().cast(), 16, 8);
        return;
    }
    let mut buf = [0u8; 300];
    let want = match expected_login() {
        Ok(_) => 0,
        Err(e) => e,
    };
    assert_eq!(s::__getlogin_r_chk(buf.as_mut_ptr().cast(), 300, 300), want);
    let o = std::process::Command::new(std::env::current_exe().unwrap()).args(["sym___getlogin_r_chk", "--exact", "--test-threads=1"]).env(PROBE, "chk").output().unwrap();
    assert_eq!(std::os::unix::process::ExitStatusExt::signal(&o.status), Some(6), "a size beyond the object: the fortify abort");
}

#[test]
fn sym_getpass() {
    match probe().as_deref() {
        Some("pipe") => {
            // No controlling terminal: the prompt goes to standard error, the line comes from
            // standard input.
            let p = s::getpass(c"Prompt: ".as_ptr()) as usize;
            println!("first=[{}]", text_at(p));
            let p = s::getpass(c"Again: ".as_ptr()) as usize;
            println!("second=[{}]", text_at(p));
            let p = s::getpass(c"Last: ".as_ptr()) as usize;
            println!("end=[{}]", text_at(p));
            return;
        }
        Some(tty) if tty.starts_with("/dev/pts/") => {
            // A new session with the pseudo-terminal as its controlling terminal.
            CSys.setsid().unwrap();
            let path = CString::new(tty).unwrap();
            let fd = CSys.openat(Fd::CWD, KStr::from_nul_terminated(path.as_bytes_with_nul()).unwrap(), O_RDWR, 0).unwrap();
            CSys.ioctl_int(fd, IoctlIntArg::Tiocsctty, 0).unwrap();
            let before = CSys.tcgets(fd).unwrap().lflag;
            let p = s::getpass(c"Password: ".as_ptr()) as usize;
            let after = CSys.tcgets(fd).unwrap().lflag;
            println!("got=[{}] echo-before={} restored={}", text_at(p), before & 0o10 != 0, before == after);
            return;
        }
        _ => {}
    }
    // Without a terminal (in a child of a new session, so /dev/tty cannot be opened).
    let mut child = std::process::Command::new("setsid")
        .arg(std::env::current_exe().unwrap())
        .args(["sym_getpass", "--exact", "--test-threads=1", "--nocapture"])
        .env(PROBE, "pipe")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    std::io::Write::write_all(child.stdin.as_mut().unwrap(), b"secret\nsecond line\nabc").unwrap();
    drop(child.stdin.take());
    let o = child.wait_with_output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{out}{}", String::from_utf8_lossy(&o.stderr));
    assert!(out.contains("first=[secret]") && out.contains("second=[second line]") && out.contains("end=[abc]"), "{out}");
    assert!(String::from_utf8_lossy(&o.stderr).contains("Prompt: Again: Last: "), "the prompts on standard error");
    // On a terminal: echo off while reading, restored afterwards, and a newline after the line.
    let (m, tty) = pty();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["sym_getpass", "--exact", "--test-threads=1", "--nocapture"])
        .env(PROBE, &tty)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    let mut seen = Vec::new();
    for _ in 0..100 {
        seen.extend(drain(m, 50));
        if seen.ends_with(b"Password: ") {
            break;
        }
    }
    assert!(seen.ends_with(b"Password: "), "the prompt on the terminal: {seen:?}");
    assert_eq!(CSys.write(m, b"hunter2\n").unwrap(), 8);
    let o = child.wait_with_output().unwrap();
    let after = drain(m, 200);
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{out}{}", String::from_utf8_lossy(&o.stderr));
    assert!(out.contains("got=[hunter2] echo-before=true restored=true"), "{out}");
    assert!(!String::from_utf8_lossy(&after).contains("hunter2"), "not echoed: {after:?}");
    assert!(after.contains(&b'\n'), "a newline after the line: {after:?}");
    let _ = CSys.close(m);
}

/// One `getpass` call in a probe child: `len=<n> fill=<all bytes equal the first>` or
/// `NULL errno=<e>`, then what it zeroed (`wipes=<count>/<bytes>/<dirty before>/<dirty after>`).
fn pass_line(prompt: &CStr) -> String {
    errno::set_raw(0);
    let _ = liella_boundary::selftest::pass_wipes();
    let p = s::getpass(prompt.as_ptr()) as usize;
    let w = liella_boundary::selftest::pass_wipes();
    let what = if p == 0 {
        format!("NULL errno={}", errno::get())
    } else {
        let t = text_at(p);
        format!("len={} fill={}", t.len(), t.bytes().all(|b| Some(b) == t.bytes().next()))
    };
    format!("{what} wipes={}/{}/{}/{}", w.count, w.len, w.dirty_before, w.dirty_after)
}

/// The `pass_line` reports in a probe child's output (the harness may print its own text
/// before the first on the same line).
fn reports(out: &str) -> Vec<String> {
    out.lines().filter_map(|l| l.find("len=").or_else(|| l.find("NULL errno=")).map(|i| l[i..].to_string())).collect()
}

/// From `pass_line`'s report: the non-zero bytes before and after the zeroing.
fn wiped(line: &str) -> (usize, usize) {
    let f: Vec<usize> = line.split("wipes=").nth(1).unwrap().split('/').map(|x| x.parse().unwrap()).collect();
    (f[2], f[3])
}

#[test]
fn sym_getpass_bound() {
    // D-17.8 (§17.5, getpass; the spec's HIST note corrected: glibc has no bound, getpass(3)):
    // at most 1 MiB before the newline unless liella_getpass_set_limit changes it; a longer line
    // is read to its end, discarded and zeroed, and the call fails with EOVERFLOW, never
    // returning a truncated password; the next line is read normally.
    const MIB: usize = 1 << 20;
    match probe().as_deref() {
        Some("bound") => {
            for prompt in [c"at: ", c"under: ", c"over: ", c"after: "] {
                println!("{}", pass_line(prompt));
            }
            return;
        }
        Some("bound-set") => {
            assert_eq!(liella_boundary::native::getpass::liella_getpass_set_limit(8), MIB as i64);
            for prompt in [c"at: ", c"over: ", c"after: "] {
                println!("{}", pass_line(prompt));
            }
            return;
        }
        Some(tty) if tty.starts_with("bound-tty:") => {
            CSys.setsid().unwrap();
            let path = CString::new(&tty["bound-tty:".len()..]).unwrap();
            let fd = CSys.openat(Fd::CWD, KStr::from_nul_terminated(path.as_bytes_with_nul()).unwrap(), O_RDWR, 0).unwrap();
            CSys.ioctl_int(fd, IoctlIntArg::Tiocsctty, 0).unwrap();
            assert_eq!(liella_boundary::native::getpass::liella_getpass_set_limit(8), MIB as i64);
            for prompt in [c"P1: ", c"P2: "] {
                println!("{}", pass_line(prompt));
            }
            return;
        }
        _ => {}
    }
    let run = |mode: &str, input: Vec<u8>| -> Vec<String> {
        let mut child = std::process::Command::new("setsid")
            .arg(std::env::current_exe().unwrap())
            .args(["sym_getpass_bound", "--exact", "--test-threads=1", "--nocapture"])
            .env(PROBE, mode)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();
        let mut stdin = child.stdin.take().unwrap();
        let writer = std::thread::spawn(move || {
            let _ = std::io::Write::write_all(&mut stdin, &input);
        });
        let o = child.wait_with_output().unwrap();
        writer.join().unwrap();
        let out = String::from_utf8_lossy(&o.stdout).into_owned();
        assert!(o.status.success(), "{out}{}", String::from_utf8_lossy(&o.stderr));
        reports(&out)
    };
    // The default bound, on standard input: at, one under and one over it.
    let mut input = Vec::new();
    for (c, n) in [(b'a', MIB), (b'b', MIB - 1), (b'c', MIB + 1)] {
        input.extend(std::iter::repeat_n(c, n));
        input.push(b'\n');
    }
    input.extend(b"after\n");
    let r = run("bound", input);
    assert_eq!(r.len(), 4, "{r:?}");
    assert!(r[0].starts_with(&format!("len={MIB} fill=true ")), "at the bound: {}", r[0]);
    assert!(r[1].starts_with(&format!("len={} fill=true ", MIB - 1)), "one under the bound: {}", r[1]);
    assert!(r[2].starts_with(&format!("NULL errno={EOVERFLOW} ")), "one over the bound: {}", r[2]);
    assert!(r[3].starts_with("len=5 fill=false "), "the rest of the long line was consumed: {}", r[3]);
    let (before, _) = wiped(&r[2]);
    assert!(before >= MIB, "the discarded line's bytes (at least the bound) were zeroed: {}", r[2]);
    for line in &r {
        assert_eq!(wiped(line).1, 0, "every buffer zeroed completely: {line}");
    }
    // The override, on standard input.
    let r = run("bound-set", b"12345678\n123456789\nok\n".to_vec());
    assert_eq!(r.len(), 3, "{r:?}");
    assert!(r[0].starts_with("len=8 fill=false "), "at the set bound: {}", r[0]);
    assert!(r[1].starts_with(&format!("NULL errno={EOVERFLOW} ")), "over the set bound: {}", r[1]);
    assert!(wiped(&r[1]).0 >= 8 && wiped(&r[1]).1 == 0, "zeroed: {}", r[1]);
    assert!(r[2].starts_with("len=2 fill=false "), "{}", r[2]);
    // The override on a terminal (read in chunks: the chunk is zeroed too).
    let (m, tty) = pty();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["sym_getpass_bound", "--exact", "--test-threads=1", "--nocapture"])
        .env(PROBE, format!("bound-tty:{tty}"))
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .unwrap();
    for (prompt, line) in [(&b"P1: "[..], &b"123456789\n"[..]), (b"P2: ", b"pass\n")] {
        let mut seen = Vec::new();
        for _ in 0..100 {
            seen.extend(drain(m, 50));
            if seen.ends_with(prompt) {
                break;
            }
        }
        assert!(seen.ends_with(prompt), "the prompt on the terminal: {seen:?}");
        assert_eq!(CSys.write(m, line).unwrap(), line.len());
    }
    let o = child.wait_with_output().unwrap();
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(o.status.success(), "{out}{}", String::from_utf8_lossy(&o.stderr));
    let r = reports(&out);
    assert_eq!(r.len(), 2, "{out}");
    assert!(r[0].starts_with(&format!("NULL errno={EOVERFLOW} ")), "over the bound on a terminal: {}", r[0]);
    assert!(wiped(&r[0]).0 >= 9 && wiped(&r[0]).1 == 0, "zeroed: {}", r[0]);
    assert!(r[1].starts_with("len=4 fill=false "), "{}", r[1]);
    let _ = CSys.close(m);
}

#[test]
fn sym_getpw() {
    let mut buf = [0u8; 1024];
    assert_eq!(s::getpw(0, buf.as_mut_ptr().cast()), 0);
    let n = buf.iter().position(|&c| c == 0).unwrap();
    assert_eq!(Some(String::from_utf8_lossy(&buf[..n]).into_owned()), passwd_line(|f| f[2] == "0"));
    errno::set_raw(0);
    assert_eq!(s::getpw(0x7fff_fff0, buf.as_mut_ptr().cast()), -1);
    assert_eq!(errno::get(), 0, "an unknown UID leaves errno");
}

#[test]
fn sym_getpwent() {
    if probe().is_none() {
        return in_child("sym_getpwent");
    }
    let mut names = Vec::new();
    loop {
        errno::set_raw(77);
        let p = s::getpwent();
        if p.is_null() {
            assert_eq!(errno::get(), 77, "the end leaves errno unchanged (D-17.3)");
            break;
        }
        names.push(text_at(copy_at(p as usize, PW0).pw_name));
    }
    let want: Vec<String> = system_records("/etc/passwd").into_iter().filter(|f| f.len() >= 4 && !f[0].is_empty()).map(|f| f[0].clone()).collect();
    assert_eq!(names, want);
    assert!(s::getpwent().is_null(), "still at the end");
    s::setpwent();
    assert_eq!(text_at(copy_at(s::getpwent() as usize, PW0).pw_name), want[0], "setpwent rewinds");
    s::endpwent();
}

#[test]
fn sym_getpwent_r() {
    if probe().is_none() {
        return in_child("sym_getpwent_r");
    }
    s::setpwent();
    let (mut p, mut res) = (PW0, 0usize);
    let mut buf = vec![0u8; 4096];
    assert_eq!(s::getpwent_r(Some(&mut p), buf.as_mut_ptr().cast(), 3, Some(&mut res)), ERANGE);
    assert_eq!(res, 0);
    assert_eq!(s::getpwent_r(Some(&mut p), buf.as_mut_ptr().cast(), 4096, Some(&mut res)), 0);
    assert_eq!(Some(pw_text(&p)), passwd_line(|_| true), "the refused record, then the next calls go on");
    let mut n = 1;
    while s::getpwent_r(Some(&mut p), buf.as_mut_ptr().cast(), 4096, Some(&mut res)) == 0 {
        n += 1;
    }
    assert_eq!(n, system_records("/etc/passwd").iter().filter(|f| f.len() >= 4).count());
    assert_eq!(s::getpwent_r(Some(&mut p), buf.as_mut_ptr().cast(), 4096, Some(&mut res)), ENOENT);
    s::endpwent();
}

#[test]
fn sym_getpwnam() {
    let p = s::getpwnam(c"root".as_ptr()) as usize;
    assert_eq!(Some(pw_text(&copy_at(p, PW0))), passwd_line(|f| f[0] == "root"));
    let g = s::getgrnam(c"root".as_ptr()) as usize;
    assert_eq!(text_at(copy_at(p, PW0).pw_name), "root", "a group lookup leaves the user record");
    assert_ne!(p, g);
    errno::set_raw(0);
    assert!(s::getpwnam(c"no-such-user-here".as_ptr()).is_null());
    assert!(s::getpwnam(c"+".as_ptr()).is_null());
    assert_eq!(errno::get(), 0, "not found leaves errno");
}

#[test]
fn sym_getpwnam_r() {
    let (mut p, mut res) = (PW0, 0usize);
    let mut buf = vec![0u8; 1024];
    for size in [1, 4, 8] {
        assert_eq!(s::getpwnam_r(c"root".as_ptr(), Some(&mut p), buf.as_mut_ptr().cast(), size, Some(&mut res)), ERANGE, "{size}");
        assert_eq!(res, 0);
    }
    assert_eq!(s::getpwnam_r(c"root".as_ptr(), Some(&mut p), buf.as_mut_ptr().cast(), 1024, Some(&mut res)), 0);
    assert_eq!((Some(pw_text(&p)), res), (passwd_line(|f| f[0] == "root"), core::ptr::from_mut(&mut p) as usize));
    let a = p.pw_name;
    assert!(a >= buf.as_ptr() as usize && a < buf.as_ptr() as usize + 1024, "the strings are in the caller's buffer");
    assert_eq!(s::getpwnam_r(c"no-such-user-here".as_ptr(), Some(&mut p), buf.as_mut_ptr().cast(), 1024, Some(&mut res)), 0);
    assert_eq!(res, 0);
}

#[test]
fn sym_getpwuid() {
    let p = s::getpwuid(0) as usize;
    assert_eq!(Some(pw_text(&copy_at(p, PW0))), passwd_line(|f| f[2] == "0"));
    if let Some(me) = passwd_line(|f| f[2] == s::getuid().to_string()) {
        assert_eq!(pw_text(&copy_at(s::getpwuid(s::getuid()) as usize, PW0)), me);
    }
    errno::set_raw(0);
    assert!(s::getpwuid(0x7fff_fff0).is_null());
    assert_eq!(errno::get(), 0);
}

#[test]
fn sym_getpwuid_r() {
    let (mut p, mut res) = (PW0, 0usize);
    let mut buf = vec![0u8; 1024];
    assert_eq!(s::getpwuid_r(0, Some(&mut p), buf.as_mut_ptr().cast(), 1024, Some(&mut res)), 0);
    assert_eq!(Some(pw_text(&p)), passwd_line(|f| f[2] == "0"));
    assert_eq!(s::getpwuid_r(0, Some(&mut p), buf.as_mut_ptr().cast(), 2, Some(&mut res)), ERANGE);
    assert_eq!(s::getpwuid_r(0x7fff_fff0, Some(&mut p), buf.as_mut_ptr().cast(), 1024, Some(&mut res)), 0);
    assert_eq!(res, 0);
    assert_eq!(s::getpwuid_r(0, None, buf.as_mut_ptr().cast(), 1024, Some(&mut res)), 14, "no structure: EFAULT");
}

#[test]
fn sym_getresgid() {
    let (mut r, mut e, mut v) = (u32::MAX, u32::MAX, u32::MAX);
    assert_eq!(s::getresgid(Some(&mut r), Some(&mut e), Some(&mut v)), 0);
    assert_eq!(vec![r, e, v], status("Gid:")[..3].to_vec(), "real, effective, saved");
    errno::set_raw(0);
    assert_eq!((s::getresgid(Some(&mut r), None, Some(&mut v)), errno::get()), (-1, EFAULT));
}

#[test]
fn sym_getresuid() {
    let (mut r, mut e, mut v) = (u32::MAX, u32::MAX, u32::MAX);
    assert_eq!(s::getresuid(Some(&mut r), Some(&mut e), Some(&mut v)), 0);
    assert_eq!(vec![r, e, v], status("Uid:")[..3].to_vec(), "real, effective, saved");
    errno::set_raw(0);
    assert_eq!((s::getresuid(Some(&mut r), None, Some(&mut v)), errno::get()), (-1, EFAULT));
}

#[test]
fn sym_getsgent_r() {
    if probe().is_none() {
        return in_child("sym_getsgent_r");
    }
    let (mut g, mut res) = (SG0, 1usize);
    let mut buf = vec![0u8; 1024];
    let r = s::getsgent_r(Some(&mut g), buf.as_mut_ptr().cast(), 1024, Some(&mut res));
    if std::fs::File::open("/etc/gshadow").is_ok() {
        assert_eq!(r, 0);
        assert!(!text_at(g.sg_namp).is_empty());
    } else {
        assert!(matches!(r, EACCES | ENOENT), "an unreadable database is its error: {r}");
        assert_eq!(res, 0);
    }
    s::endsgent();
}

#[test]
fn sym_getsgent() {
    if probe().is_none() {
        return in_child("sym_getsgent");
    }
    errno::set_raw(0);
    let g = s::getsgent();
    if std::fs::File::open("/etc/gshadow").is_ok() {
        assert!(!g.is_null());
    } else {
        assert!(g.is_null());
        assert!(matches!(errno::get(), EACCES | ENOENT));
    }
    s::endsgent();
}

#[test]
fn sym_getsgnam_r() {
    let (mut g, mut res) = (SG0, 1usize);
    let mut buf = vec![0u8; 1024];
    let r = s::getsgnam_r(c"root".as_ptr(), Some(&mut g), buf.as_mut_ptr().cast(), 1024, Some(&mut res));
    if std::fs::File::open("/etc/gshadow").is_ok() {
        assert_eq!(r, 0);
    } else {
        assert!(matches!(r, EACCES | ENOENT), "{r}");
        assert_eq!(res, 0);
    }
}

#[test]
fn sym_getsgnam() {
    errno::set_raw(0);
    let g = s::getsgnam(c"root".as_ptr());
    if std::fs::File::open("/etc/gshadow").is_ok() {
        assert_eq!(text_at(copy_at(g as usize, SG0).sg_namp), "root");
    } else {
        assert!(g.is_null());
        assert!(matches!(errno::get(), EACCES | ENOENT), "the database's error");
    }
}

#[test]
fn sym_getspent() {
    if probe().is_none() {
        return in_child("sym_getspent");
    }
    errno::set_raw(0);
    let p = s::getspent();
    if std::fs::File::open("/etc/shadow").is_ok() {
        assert!(!p.is_null());
    } else {
        assert!(p.is_null());
        assert!(matches!(errno::get(), EACCES | ENOENT));
    }
    s::endspent();
}

#[test]
fn sym_getspent_r() {
    if probe().is_none() {
        return in_child("sym_getspent_r");
    }
    let (mut sp, mut res) = (SP0, 1usize);
    let mut buf = vec![0u8; 1024];
    let r = s::getspent_r(Some(&mut sp), buf.as_mut_ptr().cast(), 1024, Some(&mut res));
    if std::fs::File::open("/etc/shadow").is_ok() {
        assert_eq!(r, 0);
    } else {
        assert!(matches!(r, EACCES | ENOENT), "{r}");
        assert_eq!(res, 0);
    }
    s::endspent();
}

#[test]
fn sym_getspnam() {
    errno::set_raw(0);
    let p = s::getspnam(c"root".as_ptr());
    if std::fs::File::open("/etc/shadow").is_ok() {
        assert_eq!(text_at(copy_at(p as usize, SP0).sp_namp), "root");
    } else {
        assert!(p.is_null());
        assert!(matches!(errno::get(), EACCES | ENOENT), "the database's error, not 'not found'");
    }
}

#[test]
fn sym_getspnam_r() {
    let (mut sp, mut res) = (SP0, 1usize);
    let mut buf = vec![0u8; 1024];
    let r = s::getspnam_r(c"root".as_ptr(), Some(&mut sp), buf.as_mut_ptr().cast(), 1024, Some(&mut res));
    if std::fs::File::open("/etc/shadow").is_ok() {
        assert_eq!(r, 0);
        assert_eq!(text_at(sp.sp_namp), "root");
    } else {
        assert!(matches!(r, EACCES | ENOENT), "{r}");
        assert_eq!(res, 0);
    }
}

#[test]
fn sym_getuid() {
    assert_eq!(s::getuid(), status("Uid:")[0], "the real UID");
}

#[test]
fn sym_getusershell() {
    if probe().is_none() {
        return in_child("sym_getusershell");
    }
    s::setusershell();
    let mut got = Vec::new();
    loop {
        let p = s::getusershell();
        if p.is_null() {
            break;
        }
        got.push(text_at(p as usize));
    }
    let want: Vec<String> = match std::fs::read_to_string("/etc/shells") {
        Ok(t) => t.lines().filter_map(|l| l.split(|c: char| c.is_ascii_whitespace() || c == '#').find(|w| !w.is_empty()).filter(|w| w.starts_with('/') && !l.trim_start().starts_with('#')).map(str::to_string)).collect(),
        Err(_) => vec!["/bin/sh".into(), "/bin/csh".into()],
    };
    assert_eq!(got, want);
    assert!(s::getusershell().is_null(), "still at the end");
    s::endusershell();
}

#[test]
fn sym_getutent() {
    if probe().is_none() {
        return in_child("sym_getutent");
    }
    let (_g, path) = fixture("getutent", Some(""));
    assert_eq!(s::utmpname(path.as_ptr()), 0);
    s::setutent();
    errno::set_raw(0);
    assert!(s::getutent().is_null(), "an empty file");
    assert_eq!(errno::get(), 0, "the end leaves errno");
    for (k, line) in ["tty1", "tty2"].iter().enumerate() {
        assert!(!s::pututline(Some(&ut(LOGIN_PROCESS, k as i32, line, &format!("{k}"), "LOGIN"))).is_null());
    }
    s::setutent();
    let a = s::getutent() as usize;
    assert_eq!(ut_text(&copy_at(a, UT0)), "6 tty1 0 LOGIN");
    let b = s::getutent() as usize;
    assert_eq!(ut_text(&copy_at(b, UT0)), "6 tty2 1 LOGIN");
    assert_eq!(a, b, "one record per thread");
    assert!(s::getutent().is_null());
    s::endutent();
}

#[test]
fn sym_getutent_r() {
    if probe().is_none() {
        return in_child("sym_getutent_r");
    }
    let (_g, path) = fixture("getutent_r", Some(""));
    assert_eq!(s::utmpname(path.as_ptr()), 0);
    s::setutent();
    s::pututline(Some(&ut(USER_PROCESS, 5, "pts/3", "p3", "bob")));
    s::setutent();
    let (mut u, mut res) = (UT0, 0usize);
    assert_eq!(s::getutent_r(Some(&mut u), Some(&mut res)), 0);
    assert_eq!((ut_text(&u), res), ("7 pts/3 p3 bob".to_string(), core::ptr::from_mut(&mut u) as usize));
    assert_eq!(s::getutent_r(Some(&mut u), Some(&mut res)), -1);
    assert_eq!(res, 0, "the end: -1 and a NULL result");
    s::endutent();
}

#[test]
fn sym_getutid() {
    if probe().is_none() {
        return in_child("sym_getutid");
    }
    let (_g, path) = fixture("getutid", Some(""));
    assert_eq!(s::utmpname(path.as_ptr()), 0);
    s::setutent();
    for u in [ut(LOGIN_PROCESS, 1, "tty1", "1", "LOGIN"), ut(BOOT_TIME, 0, "~", "~~", "reboot"), ut(DEAD_PROCESS, 2, "tty9", "1", "")] {
        s::pututline(Some(&u));
    }
    s::setutent();
    let key = ut(USER_PROCESS, 0, "", "1", "");
    assert_eq!(ut_text(&copy_at(s::getutid(Some(&key)) as usize, UT0)), "6 tty1 1 LOGIN", "the process types match by ut_id");
    assert_eq!(ut_text(&copy_at(s::getutid(Some(&key)) as usize, UT0)), "8 tty9 1 ", "the search goes on from there");
    errno::set_raw(0);
    assert!(s::getutid(Some(&key)).is_null());
    assert_eq!(errno::get(), ESRCH);
    s::setutent();
    assert_eq!(ut_text(&copy_at(s::getutid(Some(&ut(BOOT_TIME, 0, "", "", ""))) as usize, UT0)), "2 ~ ~~ reboot", "a time type matches by type");
    errno::set_raw(0);
    assert!(s::getutid(Some(&ut(0, 0, "", "1", ""))).is_null());
    assert_eq!(errno::get(), 22, "EMPTY is not a key: EINVAL");
    s::endutent();
}

#[test]
fn sym_getutid_r() {
    if probe().is_none() {
        return in_child("sym_getutid_r");
    }
    let (_g, path) = fixture("getutid_r", Some(""));
    assert_eq!(s::utmpname(path.as_ptr()), 0);
    s::setutent();
    s::pututline(Some(&ut(USER_PROCESS, 1, "pts/0", "ts/0", "alice")));
    s::setutent();
    let (mut u, mut res) = (UT0, 0usize);
    assert_eq!(s::getutid_r(Some(&ut(DEAD_PROCESS, 0, "", "ts/0", "")), Some(&mut u), Some(&mut res)), 0);
    assert_eq!(ut_text(&u), "7 pts/0 ts/0 alice");
    errno::set_raw(0);
    assert_eq!(s::getutid_r(Some(&ut(DEAD_PROCESS, 0, "", "ts/0", "")), Some(&mut u), Some(&mut res)), -1);
    assert_eq!((res, errno::get()), (0, ESRCH));
    s::endutent();
}

#[test]
fn sym_getutline() {
    if probe().is_none() {
        return in_child("sym_getutline");
    }
    let (_g, path) = fixture("getutline", Some(""));
    assert_eq!(s::utmpname(path.as_ptr()), 0);
    s::setutent();
    for u in [ut(DEAD_PROCESS, 1, "pts/0", "a", ""), ut(USER_PROCESS, 2, "pts/0", "b", "alice")] {
        s::pututline(Some(&u));
    }
    s::setutent();
    let key = ut(0, 0, "pts/0", "", "");
    assert_eq!(ut_text(&copy_at(s::getutline(Some(&key)) as usize, UT0)), "7 pts/0 b alice", "only login and user processes");
    errno::set_raw(0);
    assert!(s::getutline(Some(&key)).is_null());
    assert_eq!(errno::get(), ESRCH);
    s::endutent();
}

#[test]
fn sym_getutline_r() {
    if probe().is_none() {
        return in_child("sym_getutline_r");
    }
    let (_g, path) = fixture("getutline_r", Some(""));
    assert_eq!(s::utmpname(path.as_ptr()), 0);
    s::setutent();
    s::pututline(Some(&ut(LOGIN_PROCESS, 2, "tty3", "3", "LOGIN")));
    s::setutent();
    let (mut u, mut res) = (UT0, 0usize);
    assert_eq!(s::getutline_r(Some(&ut(0, 0, "tty3", "", "")), Some(&mut u), Some(&mut res)), 0);
    assert_eq!((ut_text(&u), res != 0), ("6 tty3 3 LOGIN".to_string(), true));
    assert_eq!(s::getutline_r(Some(&ut(0, 0, "tty3", "", "")), Some(&mut u), Some(&mut res)), -1);
    s::endutent();
}

#[test]
fn sym_getutmp() {
    let mut x = ut(USER_PROCESS, 9, "pts/9", "p9", "carol");
    x.ut_tv.tv_sec = 1234;
    let mut u = UT0;
    s::getutmp(Some(&x), Some(&mut u));
    assert_eq!((ut_text(&u), u.ut_tv.tv_sec, field(&u.ut_host)), ("7 pts/9 p9 carol".to_string(), 1234, "host.example".to_string()));
}

#[test]
fn sym_getutmpx() {
    let u = ut(DEAD_PROCESS, 3, "tty4", "4", "");
    let mut x = UT0;
    s::getutmpx(Some(&u), Some(&mut x));
    assert_eq!(ut_text(&x), "8 tty4 4 ");
}

#[test]
fn sym_getutxent() {
    if probe().is_none() {
        return in_child("sym_getutxent");
    }
    let (_g, path) = fixture("getutxent", Some(""));
    assert_eq!(s::utmpxname(path.as_ptr()), 0);
    s::setutxent();
    s::pututxline(Some(&ut(USER_PROCESS, 1, "pts/1", "p1", "dave")));
    s::setutxent();
    assert_eq!(ut_text(&copy_at(s::getutxent() as usize, UT0)), "7 pts/1 p1 dave");
    assert!(s::getutxent().is_null());
    s::endutxent();
}

#[test]
fn sym_getutxid() {
    if probe().is_none() {
        return in_child("sym_getutxid");
    }
    let (_g, path) = fixture("getutxid", Some(""));
    assert_eq!(s::utmpxname(path.as_ptr()), 0);
    s::setutxent();
    s::pututxline(Some(&ut(INIT_PROCESS, 1, "", "si", "")));
    s::setutxent();
    assert_eq!(ut_text(&copy_at(s::getutxid(Some(&ut(LOGIN_PROCESS, 0, "", "si", ""))) as usize, UT0)), "5  si ");
    s::endutxent();
}

#[test]
fn sym_getutxline() {
    if probe().is_none() {
        return in_child("sym_getutxline");
    }
    let (_g, path) = fixture("getutxline", Some(""));
    assert_eq!(s::utmpxname(path.as_ptr()), 0);
    s::setutxent();
    s::pututxline(Some(&ut(USER_PROCESS, 1, "pts/7", "p7", "erin")));
    s::setutxent();
    assert_eq!(ut_text(&copy_at(s::getutxline(Some(&ut(0, 0, "pts/7", "", ""))) as usize, UT0)), "7 pts/7 p7 erin");
    s::endutxent();
}

#[test]
fn sym_group_member() {
    assert_eq!(s::group_member(s::getegid()), 1, "the effective GID");
    for g in status("Groups:") {
        assert_eq!(s::group_member(g), 1, "supplementary group {g}");
    }
    let outsider = (60000..65000).find(|g| *g != s::getegid() && !status("Groups:").contains(g)).unwrap();
    assert_eq!(s::group_member(outsider), 0);
}

#[test]
fn sym_initgroups() {
    if s::geteuid() == 0 {
        skip_note::skipped("skipped: privileged (initgroups would change the test process's groups)");
        return;
    }
    // Unprivileged, the group list is computed and the kernel refuses to install it.
    errno::set_raw(0);
    assert_eq!(s::initgroups(c"root".as_ptr(), 0), -1);
    assert_eq!(errno::get(), EPERM);
}

#[test]
fn sym_lckpwdf() {
    if probe().is_none() {
        return in_child("sym_lckpwdf");
    }
    if s::geteuid() == 0 {
        skip_note::skipped("skipped: privileged (lckpwdf would create the system's lock file)");
        return;
    }
    // Unprivileged: the lock file cannot be created.
    errno::set_raw(0);
    assert_eq!(s::lckpwdf(), -1);
    assert_eq!(errno::get(), EACCES);
    assert_eq!(s::ulckpwdf(), -1, "nothing to release");
    // The wait (D-17.4: 15 s; a fixture lock file and a short wait here): while another open
    // file holds the lock, the attempt fails after the wait; no signal handler is involved.
    assert_eq!(acct::PWD_LOCK_TIMEOUT_MS, 15_000);
    let (_g, path) = fixture("pwdlock-busy", Some(""));
    let k = &CSys;
    let other = k.openat(Fd::CWD, KStr::from_nul_terminated(path.as_bytes_with_nul()).unwrap(), O_RDWR | O_CLOEXEC, 0).unwrap();
    acct::lock_file(k, other, 1, 0).unwrap();
    let t = std::time::Instant::now();
    assert_eq!(acct::lock_passwd(k, path.as_bytes_with_nul(), 300), Err(liella_kernel::errno::Errno::EAGAIN));
    assert!(t.elapsed() >= std::time::Duration::from_millis(300), "it waited");
    acct::unlock_file(k, other);
    assert_eq!(acct::lock_passwd(k, path.as_bytes_with_nul(), 300), Ok(true), "free: taken");
    assert_eq!(s::ulckpwdf(), 0);
    let _ = k.close(other);
}

#[test]
fn sym_login() {
    // Unprivileged, the system's login records cannot be written, so `login` changes nothing
    // (its own writes are tested on fixture files in the clause tests).
    if s::geteuid() == 0 || CSys.faccessat(Fd::CWD, KStr::from_nul_terminated(b"/var/log/wtmp\0").unwrap(), 2).is_ok() {
        skip_note::skipped("skipped: the system's wtmp is writable");
        return;
    }
    let size = |p: &str| std::fs::metadata(p).map(|m| m.len()).ok();
    let before = (size("/var/run/utmp"), size("/var/log/wtmp"));
    let rec = ut(0, 0, "", "zz", "nobody-test");
    s::login(Some(&rec));
    assert_eq!((size("/var/run/utmp"), size("/var/log/wtmp")), before);
    assert_eq!(rec.ut_type, 0, "the caller's record is not changed");
}

#[test]
fn sym_login_tty() {
    if let Some(tty) = probe() {
        let path = CString::new(tty).unwrap();
        let fd = CSys.openat(Fd::CWD, KStr::from_nul_terminated(path.as_bytes_with_nul()).unwrap(), O_RDWR | O_NOCTTY, 0).unwrap();
        let fd = CSys.dup2(fd, Fd::new(77)).unwrap();
        assert_eq!(s::login_tty(fd.raw()), 0);
        let me = CSys.getpid();
        assert_eq!(CSys.getsid(0).unwrap(), me, "a new session");
        assert_eq!(CSys.ioctl_get_int(Fd::STDIN, IoctlIntOut::Tiocgsid).unwrap(), me, "the terminal is the session's");
        for std in [Fd::STDIN, Fd::STDOUT, Fd::STDERR] {
            assert!(CSys.tcgets(std).is_ok(), "standard descriptors are the terminal");
        }
        assert!(CSys.tcgets(fd).is_err(), "the descriptor given is closed");
        CSys.exit_group(0);
    }
    let (m, tty) = pty();
    let o = std::process::Command::new(std::env::current_exe().unwrap()).args(["sym_login_tty", "--exact", "--test-threads=1"]).env(PROBE, &tty).output().unwrap();
    assert!(o.status.success(), "{:?}\n{}{}", o.status, String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    errno::set_raw(0);
    assert_eq!(s::login_tty(0x7fff_0000), -1, "not a descriptor");
    let _ = CSys.close(m);
}

#[test]
fn sym_logout() {
    if s::geteuid() == 0 {
        skip_note::skipped("skipped: privileged");
        return;
    }
    assert_eq!(s::logout(c"no-such-line-here".as_ptr()), 0, "nothing to mark");
}

#[test]
fn sym_logwtmp() {
    if s::geteuid() == 0 || CSys.faccessat(Fd::CWD, KStr::from_nul_terminated(b"/var/log/wtmp\0").unwrap(), 2).is_ok() {
        skip_note::skipped("skipped: the system's wtmp is writable");
        return;
    }
    errno::set_raw(0);
    s::logwtmp(c"ttyX".as_ptr(), c"".as_ptr(), c"".as_ptr());
    assert!(matches!(errno::get(), EACCES | ENOENT | EPERM), "the refusal is in errno: {}", errno::get());
}

#[test]
fn sym_putgrent() {
    let (path, cpath) = fixture("putgrent", None);
    let fp = stream(&cpath, c"w");
    let (name, pw) = (c"staff", c"x");
    let mem = [c"alice".as_ptr() as usize, c"bob".as_ptr() as usize, 0];
    let g = struct_group { gr_name: name.as_ptr() as usize, gr_passwd: pw.as_ptr() as usize, gr_gid: 50, gr_mem: mem.as_ptr() as usize };
    assert_eq!(s::putgrent(Some(&g), fp), 0);
    let none = struct_group { gr_mem: 0, gr_passwd: 0, ..g };
    assert_eq!(s::putgrent(Some(&none), fp), 0, "NULL members and password are empty");
    let comma = [c"a,b".as_ptr() as usize, 0];
    errno::set_raw(0);
    assert_eq!(s::putgrent(Some(&struct_group { gr_mem: comma.as_ptr() as usize, ..g }), fp), -1);
    assert_eq!(errno::get(), 22, "a member that would not read back: EINVAL");
    assert_eq!(s::putgrent(None, fp), -1);
    close_stream(fp);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "staff:x:50:alice,bob\nstaff::50:\n");
}

#[test]
fn sym_putpwent() {
    let (path, cpath) = fixture("putpwent", None);
    let fp = stream(&cpath, c"w");
    let t = |c: &'static CStr| c.as_ptr() as usize;
    let p = struct_passwd { pw_name: t(c"dave"), pw_passwd: t(c"x"), pw_uid: 1010, pw_gid: 1010, pw_gecos: t(c"Dave, Jr."), pw_dir: t(c"/home/dave"), pw_shell: t(c"/bin/zsh") };
    assert_eq!(s::putpwent(Some(&p), fp), 0);
    assert_eq!(s::putpwent(Some(&struct_passwd { pw_name: t(c"+nis"), pw_uid: 0, pw_gid: 7, pw_gecos: 0, ..p }), fp), 0);
    for bad in [struct_passwd { pw_name: t(c"a:b"), ..p }, struct_passwd { pw_shell: t(c"/bin/sh\n"), ..p }, struct_passwd { pw_name: 0, ..p }] {
        errno::set_raw(0);
        assert_eq!(s::putpwent(Some(&bad), fp), -1);
        assert_eq!(errno::get(), 22);
    }
    errno::set_raw(0);
    assert_eq!(s::putpwent(None, fp), -1);
    assert_eq!(errno::get(), 22);
    close_stream(fp);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "dave:x:1010:1010:Dave, Jr.:/home/dave:/bin/zsh\n+nis:x::7::/home/dave:/bin/zsh\n");
}

#[test]
fn sym_putsgent() {
    let (path, cpath) = fixture("putsgent", None);
    let fp = stream(&cpath, c"w");
    let adm = [c"root".as_ptr() as usize, 0];
    let mem = [c"alice".as_ptr() as usize, c"bob".as_ptr() as usize, 0];
    let g = struct_sgrp { sg_namp: c"staff".as_ptr() as usize, sg_passwd: c"!".as_ptr() as usize, sg_adm: adm.as_ptr() as usize, sg_mem: mem.as_ptr() as usize };
    assert_eq!(s::putsgent(Some(&g), fp), 0);
    assert_eq!(s::putsgent(Some(&struct_sgrp { sg_adm: 0, sg_mem: 0, sg_passwd: 0, ..g }), fp), 0);
    assert_eq!(s::putsgent(Some(&struct_sgrp { sg_namp: c"a:b".as_ptr() as usize, ..g }), fp), -1);
    close_stream(fp);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "staff:!:root:alice,bob\nstaff:::\n");
}

#[test]
fn sym_putspent() {
    let (path, cpath) = fixture("putspent", None);
    let fp = stream(&cpath, c"w");
    let sp = struct_spwd { sp_namp: c"dave".as_ptr() as usize, sp_pwdp: c"$1$x$y".as_ptr() as usize, sp_lstchg: 19000, sp_min: 0, sp_max: 99999, sp_warn: 7, sp_inact: -1, sp_expire: -1, sp_flag: u64::MAX };
    assert_eq!(s::putspent(Some(&sp), fp), 0);
    assert_eq!(s::putspent(Some(&struct_spwd { sp_pwdp: 0, sp_flag: 7, sp_inact: 5, ..sp }), fp), 0);
    assert_eq!(s::putspent(Some(&struct_spwd { sp_namp: c"x:y".as_ptr() as usize, ..sp }), fp), -1);
    close_stream(fp);
    assert_eq!(std::fs::read_to_string(path).unwrap(), "dave:$1$x$y:19000:0:99999:7:::\ndave::19000:0:99999:7:5::7\n");
}

#[test]
fn sym_pututline() {
    if probe().is_none() {
        return in_child("sym_pututline");
    }
    let (path, cpath) = fixture("pututline", Some(""));
    assert_eq!(s::utmpname(cpath.as_ptr()), 0);
    s::setutent();
    let a = ut(LOGIN_PROCESS, 1, "tty1", "1", "LOGIN");
    assert_eq!(s::pututline(Some(&a)) as usize, core::ptr::from_ref(&a) as usize, "the caller's record is returned");
    s::pututline(Some(&ut(USER_PROCESS, 2, "pts/0", "ts/0", "alice")));
    s::setutent();
    s::pututline(Some(&ut(DEAD_PROCESS, 2, "pts/0", "ts/0", "")));
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 768, "the record with the same id was replaced");
    s::setutent();
    let all: Vec<String> = std::iter::from_fn(|| Some(s::getutent() as usize).filter(|&p| p != 0)).map(|p| ut_text(&copy_at(p, UT0))).collect();
    assert_eq!(all, ["6 tty1 1 LOGIN", "8 pts/0 ts/0 "]);
    // Read-only: the write is refused with the open's error.
    s::endutent();
    std::fs::set_permissions(&path, std::os::unix::fs::PermissionsExt::from_mode(0o444)).unwrap();
    if s::geteuid() != 0 {
        s::setutent();
        errno::set_raw(0);
        assert!(s::pututline(Some(&a)).is_null());
        assert_eq!(errno::get(), EACCES);
    }
    s::endutent();
}

#[test]
fn sym_pututxline() {
    if probe().is_none() {
        return in_child("sym_pututxline");
    }
    let (path, cpath) = fixture("pututxline", Some(""));
    assert_eq!(s::utmpxname(cpath.as_ptr()), 0);
    s::setutxent();
    let b = ut(BOOT_TIME, 0, "~", "~~", "reboot");
    assert_eq!(s::pututxline(Some(&b)) as usize, core::ptr::from_ref(&b) as usize);
    s::setutxent();
    s::pututxline(Some(&b));
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 384, "the same time type is replaced");
    s::endutxent();
}

#[test]
fn sym_setegid() {
    exported("setegid");
    conformance(&["s17_setxid_ids"]);
}
#[test]
fn sym_seteuid() {
    exported("seteuid");
    conformance(&["s17_setxid_ids"]);
}
#[test]
fn sym_setfsgid() {
    exported("setfsgid");
    conformance(&["s17_setxid_ids"]);
}
#[test]
fn sym_setfsuid() {
    exported("setfsuid");
    conformance(&["s17_setxid_ids"]);
}
#[test]
fn sym_setgid() {
    exported("setgid");
    conformance(&["s17_setxid_ids"]);
}
#[test]
fn sym_setgrent() {
    if probe().is_none() {
        return in_child("sym_setgrent");
    }
    let first = text_at(copy_at(s::getgrent() as usize, GR0).gr_name);
    s::getgrent();
    s::setgrent();
    assert_eq!(text_at(copy_at(s::getgrent() as usize, GR0).gr_name), first, "setgrent rewinds");
    s::endgrent();
}

#[test]
fn sym_setgroups() {
    exported("setgroups");
    conformance(&["s17_setxid"]);
}
#[test]
fn sym_setlogin() {
    errno::set_raw(0);
    assert_eq!(s::setlogin(c"someone".as_ptr()), -1);
    assert_eq!(errno::get(), ENOSYS, "not available on Linux");
}

#[test]
fn sym_setpwent() {
    if probe().is_none() {
        return in_child("sym_setpwent");
    }
    s::setpwent();
    let first = text_at(copy_at(s::getpwent() as usize, PW0).pw_name);
    s::getpwent();
    s::setpwent();
    assert_eq!(text_at(copy_at(s::getpwent() as usize, PW0).pw_name), first, "setpwent rewinds");
    // The cursor is the process's: another thread continues it.
    let second = std::thread::spawn(|| text_at(copy_at(s::getpwent() as usize, PW0).pw_name)).join().unwrap();
    assert_eq!(Some(second), system_records("/etc/passwd").get(1).map(|f| f[0].clone()));
    s::endpwent();
}

#[test]
fn sym_setregid() {
    exported("setregid");
    conformance(&["s17_setxid_ids"]);
}
#[test]
fn sym_setresgid() {
    exported("setresgid");
    conformance(&["s17_setxid"]);
}
#[test]
fn sym_setresuid() {
    exported("setresuid");
    conformance(&["s17_setxid_ids"]);
}
#[test]
fn sym_setreuid() {
    exported("setreuid");
    conformance(&["s17_setxid"]);
}
#[test]
fn sym_setsgent() {
    if probe().is_none() {
        return in_child("sym_setsgent");
    }
    s::setsgent();
    errno::set_raw(0);
    let first = s::getsgent();
    if first.is_null() {
        assert!(matches!(errno::get(), EACCES | ENOENT));
    }
    s::endsgent();
}

#[test]
fn sym_setspent() {
    if probe().is_none() {
        return in_child("sym_setspent");
    }
    s::setspent();
    errno::set_raw(0);
    let first = s::getspent();
    if first.is_null() {
        assert!(matches!(errno::get(), EACCES | ENOENT));
    }
    s::endspent();
}

#[test]
fn sym_setuid() {
    exported("setuid");
    conformance(&["s17_setxid", "s17_setxid_ids"]);
}
#[test]
fn sym_setusershell() {
    if probe().is_none() {
        return in_child("sym_setusershell");
    }
    let first = text_at(s::getusershell() as usize);
    s::getusershell();
    s::setusershell();
    assert_eq!(text_at(s::getusershell() as usize), first, "setusershell rewinds");
    s::endusershell();
}

#[test]
fn sym_setutent() {
    if probe().is_none() {
        return in_child("sym_setutent");
    }
    let (_g, path) = fixture("setutent", Some(""));
    assert_eq!(s::utmpname(path.as_ptr()), 0);
    s::setutent();
    s::pututline(Some(&ut(USER_PROCESS, 1, "pts/1", "p1", "alice")));
    assert!(s::getutent().is_null(), "after the record written");
    s::setutent();
    assert_eq!(ut_text(&copy_at(s::getutent() as usize, UT0)), "7 pts/1 p1 alice", "setutent rewinds");
    s::endutent();
}

#[test]
fn sym_setutxent() {
    if probe().is_none() {
        return in_child("sym_setutxent");
    }
    let (_g, path) = fixture("setutxent", Some(""));
    assert_eq!(s::utmpxname(path.as_ptr()), 0);
    s::setutxent();
    s::pututxline(Some(&ut(USER_PROCESS, 1, "pts/2", "p2", "bob")));
    s::setutxent();
    assert_eq!(ut_text(&copy_at(s::getutxent() as usize, UT0)), "7 pts/2 p2 bob");
    s::endutxent();
}

#[test]
fn sym_sgetsgent_r() {
    let (mut g, mut res) = (SG0, 0usize);
    let mut buf = [0u8; 128];
    assert_eq!(s::sgetsgent_r(c"g:p:a1,a2:m1".as_ptr(), Some(&mut g), buf.as_mut_ptr().cast(), 128, Some(&mut res)), 0);
    assert_eq!((sg_text(&g), res), ("g:p:a1,a2:m1".to_string(), core::ptr::from_mut(&mut g) as usize));
    assert_eq!(s::sgetsgent_r(c"g:p:a1,a2:m1".as_ptr(), Some(&mut g), buf.as_mut_ptr().cast(), 8, Some(&mut res)), ERANGE);
    assert_eq!(s::sgetsgent_r(c"# not a record".as_ptr(), Some(&mut g), buf.as_mut_ptr().cast(), 128, Some(&mut res)), 22);
    assert_eq!(res, 0);
}

#[test]
fn sym_sgetsgent() {
    assert_eq!(sg_text(&copy_at(s::sgetsgent(c"g:p:a1,a2:m1\n".as_ptr()) as usize, SG0)), "g:p:a1,a2:m1");
    assert_eq!(sg_text(&copy_at(s::sgetsgent(c"g:p".as_ptr()) as usize, SG0)), "g:p::");
    errno::set_raw(0);
    assert!(s::sgetsgent(c"".as_ptr()).is_null());
    assert_eq!(errno::get(), 22, "no record: EINVAL");
}

#[test]
fn sym_sgetspent() {
    assert_eq!(sp_text(&copy_at(s::sgetspent(c"carol:hash:1:2:3:4:5:6:".as_ptr()) as usize, SP0)), "carol:hash:1:2:3:4:5:6:-1");
    assert_eq!(sp_text(&copy_at(s::sgetspent(c"carol::::::::".as_ptr()) as usize, SP0)), "carol::-1:-1:-1:-1:-1:-1:-1");
    for bad in [c"carol:hash", c"carol:hash:a:b:c:d:e:f:", c"", c"carol:h:1:2:3:4:5:6:7:8", c"carol:h:+1:2:3:4:5:6:"] {
        errno::set_raw(0);
        assert!(s::sgetspent(bad.as_ptr()).is_null(), "{bad:?}");
        assert_eq!(errno::get(), 22);
    }
}

#[test]
fn sym_sgetspent_r() {
    let (mut sp, mut res) = (SP0, 0usize);
    let mut buf = [0u8; 64];
    assert_eq!(s::sgetspent_r(c"carol:hash:1:2:3:4:5:6:7".as_ptr(), Some(&mut sp), buf.as_mut_ptr().cast(), 64, Some(&mut res)), 0);
    assert_eq!(sp_text(&sp), "carol:hash:1:2:3:4:5:6:7");
    assert_eq!(s::sgetspent_r(c"carol:hash:1:2:3:4:5:6:7".as_ptr(), Some(&mut sp), buf.as_mut_ptr().cast(), 10, Some(&mut res)), ERANGE, "10 bytes, the strings need 11");
    assert_eq!(s::sgetspent_r(c"carol:hash:1:2:3:4:5:6:7".as_ptr(), Some(&mut sp), buf.as_mut_ptr().cast(), 11, Some(&mut res)), 0);
}

#[test]
fn sym_ttyslot() {
    // The test runs without /etc/ttys or without a terminal on its standard descriptors: 0.
    if std::path::Path::new("/etc/ttys").exists() && (0..3).any(|fd| CSys.tcgets(Fd::new(fd)).is_ok()) {
        skip_note::skipped("skipped: a terminal table and a terminal");
        return;
    }
    assert_eq!(s::ttyslot(), 0);
}

#[test]
fn sym_ulckpwdf() {
    if probe().is_none() {
        return in_child("sym_ulckpwdf");
    }
    assert_eq!(s::ulckpwdf(), -1, "not held");
    // Held (on a fixture lock file, through the same process-wide state), it is released once.
    let (_g, path) = fixture("pwdlock", None);
    let k = &CSys;
    assert_eq!(acct::lock_passwd(k, path.as_bytes_with_nul(), 100), Ok(true));
    assert_eq!(acct::lock_passwd(k, path.as_bytes_with_nul(), 100), Ok(false), "held already");
    assert_eq!(s::lckpwdf(), -1, "held already: not taken again");
    assert_eq!(s::ulckpwdf(), 0);
    assert_eq!(s::ulckpwdf(), -1);
}

#[test]
fn sym_updwtmp() {
    let (path, cpath) = fixture("wtmp", Some(""));
    s::updwtmp(cpath.as_ptr(), Some(&ut(USER_PROCESS, 1, "pts/0", "p0", "alice")));
    s::updwtmp(cpath.as_ptr(), Some(&ut(DEAD_PROCESS, 1, "pts/0", "p0", "")));
    let b = std::fs::read(&path).unwrap();
    assert_eq!(b.len(), 768, "two records appended");
    assert_eq!(&b[44..49], b"alice");
    assert_eq!(i16::from_ne_bytes([b[384], b[385]]), DEAD_PROCESS);
    let (missing, cmissing) = fixture("nowtmp", None);
    errno::set_raw(0);
    s::updwtmp(cmissing.as_ptr(), Some(&ut(USER_PROCESS, 1, "pts/0", "p0", "alice")));
    assert_eq!(errno::get(), ENOENT, "a missing file is not created");
    assert!(!missing.exists());
}

#[test]
fn sym_updwtmpx() {
    let (path, cpath) = fixture("wtmpx", Some(""));
    s::updwtmpx(cpath.as_ptr(), Some(&ut(BOOT_TIME, 0, "~", "~~", "reboot")));
    assert_eq!(std::fs::metadata(path).unwrap().len(), 384);
}

#[test]
fn sym_utmpname() {
    if probe().is_none() {
        return in_child("sym_utmpname");
    }
    let (_g, a) = fixture("utmpname-a", Some(""));
    let (_g, b) = fixture("utmpname-b", Some(""));
    assert_eq!(s::utmpname(a.as_ptr()), 0);
    s::setutent();
    s::pututline(Some(&ut(USER_PROCESS, 1, "pts/1", "1", "in-a")));
    assert_eq!(s::utmpname(b.as_ptr()), 0, "another file: the first is closed");
    s::setutent();
    assert!(s::getutent().is_null(), "b is empty");
    assert_eq!(s::utmpname(c"".as_ptr()), 0, "any name is accepted");
    s::setutent();
    errno::set_raw(0);
    assert!(s::getutent().is_null());
    assert_eq!(errno::get(), ENOENT, "and fails when used");
    let long = CString::new(vec![b'x'; 5000]).unwrap();
    assert_eq!(s::utmpname(long.as_ptr()), -1);
    assert_eq!(errno::get(), 36, "ENAMETOOLONG");
    s::endutent();
}

#[test]
fn sym_utmpxname() {
    if probe().is_none() {
        return in_child("sym_utmpxname");
    }
    let (_g, a) = fixture("utmpxname", Some(""));
    assert_eq!(s::utmpxname(a.as_ptr()), 0);
    s::setutxent();
    s::pututxline(Some(&ut(USER_PROCESS, 1, "pts/1", "1", "x")));
    s::setutent();
    assert_eq!(ut_text(&copy_at(s::getutent() as usize, UT0)), "7 pts/1 1 x", "utmpname and utmpxname name the same file");
    s::endutxent();
}

