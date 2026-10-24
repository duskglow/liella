//! The kernel fallbacks: each public call that has a fallback for a newer system call still
//! meets its clause when that call fails with `ENOSYS` (an older kernel, or a seccomp policy
//! that does not know the call). Each test runs this binary again under
//! `tests/oracle/enosys_trap.c` (an unprivileged seccomp filter: `PR_SET_NO_NEW_PRIVS` plus
//! `SECCOMP_RET_ERRNO(ENOSYS)` for the named calls; needs a C compiler), and the child checks
//! the trap is in force, then the public call. The expected results were probed against the
//! host glibc 2.39 under the same filter. The list comes from the code: `clone3` (spawn),
//! `close_range` (`closefrom`), `faccessat2`, `fchmodat2`, `getrandom` (the `mkstemp` names);
//! `pidfd_open` has no fallback, as in glibc, and passes the `ENOSYS` through.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

#[path = "support/skip_note.rs"]
mod skip_note;

#[path = "support/scratch.rs"]
mod scratch;

use std::ffi::CString;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use liella_abi::ctypes::c_char;
use liella_boundary::errno;
use liella_boundary::posix::s09_random as rnd;
use liella_boundary::posix::s11_lowio as lowio;
use liella_boundary::posix::s13_filesystem as fs;
use liella_boundary::posix::s18_processes as x;
use liella_boundary::sys::CSys;
use liella_kernel::errno::Errno;
use liella_kernel::fd::Fd;
use liella_kernel::ops::FcntlInt;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_legacy::spawn_compat::CFileActions;

use scratch::Scratch;

/// The calls the child runs under (set only in the child) and its scratch directory.
const CASE: &str = "LIELLA_TEST_ENOSYS_CASE";
const DIR: &str = "LIELLA_TEST_ENOSYS_DIR";
const AT_FDCWD: i32 = -100;
const AT_SYMLINK_NOFOLLOW: i32 = 0x100;
const AT_EACCESS: i32 = 0x200;
const F_OK: i32 = 0;
const X_OK: i32 = 1;
const R_OK: i32 = 4;
const ENOENT: i32 = 2;
const EACCES: i32 = 13;
const ENOSYS: i32 = 38;
const EOPNOTSUPP: i32 = 95;

/// In the child for `calls`: its scratch directory, after checking the filter is in force.
fn child(calls: &str) -> Option<PathBuf> {
    if std::env::var(CASE).ok()? != calls {
        return None;
    }
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    assert!(status.lines().any(|l| l == "Seccomp:\t2"), "the child runs under the filter");
    Some(PathBuf::from(std::env::var(DIR).unwrap()))
}

/// Run `test` again under the trap for `calls`; it must pass and reach its end.
fn under_trap(test: &str, calls: &str) {
    let dir = Scratch::dir("enosys");
    let trap = dir.join("enosys_trap");
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/oracle/enosys_trap.c");
    match Command::new("cc").args(["-O1", "-o"]).arg(&trap).arg(&src).output() {
        Ok(o) if o.status.success() => {}
        Ok(o) => panic!("enosys_trap.c does not build: {}", String::from_utf8_lossy(&o.stderr)),
        Err(_) => {
            skip_note::skipped("skipped: no C compiler");
            return;
        }
    }
    let me = std::env::current_exe().unwrap();
    let o = Command::new(&trap).arg(calls).arg(me).args([test, "--exact", "--test-threads=1", "--nocapture"]).env(CASE, calls).env(DIR, &*dir).output().unwrap();
    let out = format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    for note in out.lines().filter(|l| l.starts_with("note:")) {
        eprintln!("{note}");
    }
    assert!(o.status.success() && out.contains(&format!("ENOSYS-CHILD-OK {calls}")), "{test} with {calls} refused (ENOSYS): {:?}\n{out}", o.status);
}

fn done(calls: &str) {
    println!("ENOSYS-CHILD-OK {calls}");
}

fn cs(s: &str) -> CString {
    CString::new(s).unwrap()
}

fn cpath(p: &Path) -> CString {
    CString::new(p.as_os_str().as_encoded_bytes()).unwrap()
}

/// `sh -c "exit 7"` as an argument vector, with an empty environment.
/// `_strings` keeps alive the strings `argv` points to.
struct Argv {
    _strings: Vec<CString>,
    argv: Vec<*mut c_char>,
    envp: Vec<*mut c_char>,
}

impl Argv {
    fn sh_exit7() -> Argv {
        Argv::sh("exit 7")
    }
    fn sh(script: &str) -> Argv {
        let s = vec![cs("sh"), cs("-c"), cs(script)];
        let mut p: Vec<*mut c_char> = s.iter().map(|c| c.as_ptr().cast_mut()).collect();
        p.push(std::ptr::null_mut());
        Argv { _strings: s, argv: p, envp: vec![std::ptr::null_mut()] }
    }
}

/// The exit status of child `pid`.
fn exit_status(pid: i32) -> i32 {
    let mut st = -1;
    assert_eq!(x::waitpid(pid, Some(&mut st), 0), pid);
    (st >> 8) & 0xff
}

fn mode_of(p: &Path) -> u32 {
    std::fs::symlink_metadata(p).unwrap().permissions().mode() & 0o777
}

/// A regular file `f` (mode 0600) and a dangling symlink `dangling` in `dir`.
fn file_and_link(dir: &Path) -> (PathBuf, PathBuf) {
    let (f, l) = (dir.join("f"), dir.join("dangling"));
    std::fs::write(&f, b"x").unwrap();
    std::fs::set_permissions(&f, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::os::unix::fs::symlink("nowhere", &l).unwrap();
    (f, l)
}

#[test]
fn clone3_enosys_spawn_falls_back() {
    // Trace: s18_4_c_0 (posix_spawn and pidfd_spawn report the child when clone3 is ENOSYS; D-18.3 copy-on-write fallback).
    let calls = "clone3";
    if child(calls).is_some() {
        let a = Argv::sh_exit7();
        let mut pid = -1;
        assert_eq!(x::posix_spawn(Some(&mut pid), cs("/bin/sh").as_ptr(), None, None, a.argv.as_ptr(), a.envp.as_ptr()), 0, "posix_spawn");
        assert_eq!(exit_status(pid), 7, "the child ran the program");
        assert_eq!(x::posix_spawn(Some(&mut pid), cs("/nonexistent/x").as_ptr(), None, None, a.argv.as_ptr(), a.envp.as_ptr()), ENOENT, "the exec error comes back");
        let mut fd = -1;
        assert_eq!(x::pidfd_spawn(Some(&mut fd), cs("/bin/sh").as_ptr(), None, None, a.argv.as_ptr(), a.envp.as_ptr()), 0, "pidfd_spawn");
        let pid = x::pidfd_getpid(fd);
        assert!(fd >= 0 && pid > 0, "the pidfd refers to the child: fd {fd} pid {pid}");
        assert_eq!(exit_status(pid), 7);
        let _ = CSys.close(Fd::new(fd));
        return done(calls);
    }
    under_trap("clone3_enosys_spawn_falls_back", calls);
}

#[test]
fn close_range_enosys_closefrom_falls_back() {
    // Trace: s11_2_c_0 (closefrom closes every descriptor from lo when close_range is ENOSYS).
    let calls = "close_range";
    if child(calls).is_some() {
        assert_eq!(CSys.close_range(300, 300, 0), Err(Errno::ENOSYS), "the trap");
        for fd in [200, 201, 230] {
            CSys.dup2(Fd::new(0), Fd::new(fd)).unwrap();
        }
        lowio::closefrom(201);
        assert!(CSys.fcntl_int(Fd::new(200), FcntlInt::GetFd, 0).is_ok(), "below lo stays open");
        for fd in [201, 230] {
            assert_eq!(CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFd, 0), Err(Errno::EBADF), "{fd} closed");
        }
        let _ = CSys.close(Fd::new(200));
        // Trace: s18_4_c_0 (the closefrom file action closes from fd up, spawning when close_range is ENOSYS).
        for fd in [200, 201, 230, 400] {
            CSys.dup2(Fd::new(0), Fd::new(fd)).unwrap();
        }
        let mut fa = Box::new(CFileActions::new());
        assert_eq!(x::posix_spawn_file_actions_init(Some(&mut fa)), 0);
        assert_eq!(x::posix_spawn_file_actions_addclosefrom_np(Some(&mut fa), 201), 0);
        // 200 stays (status 7 only if it is open); 201, 230 and 400 are gone (9 when any is open).
        let script = "[ -e /proc/self/fd/200 ] || exit 8; for n in 201 230 400; do [ -e /proc/self/fd/$n ] && exit 9; done; exit 7";
        let (a, mut pid) = (Argv::sh(script), -1);
        let r = x::posix_spawn(Some(&mut pid), cs("/bin/sh").as_ptr(), Some(&fa), None, a.argv.as_ptr(), a.envp.as_ptr());
        assert_eq!(r, 0, "posix_spawn with addclosefrom_np spawns");
        assert_eq!(exit_status(pid), 7, "descriptors from 201 closed in the child, 200 kept");
        x::posix_spawn_file_actions_destroy(Some(&mut fa));
        for fd in [200, 201, 230, 400] {
            let _ = CSys.close(Fd::new(fd));
        }
        return done(calls);
    }
    under_trap("close_range_enosys_closefrom_falls_back", calls);
}

#[test]
fn faccessat2_enosys_faccessat_emulates() {
    // Trace: s13_7_c_4 (faccessat with AT_EACCESS and AT_SYMLINK_NOFOLLOW when faccessat2 is ENOSYS).
    let calls = "faccessat2";
    if let Some(dir) = child(calls) {
        let (f, l) = file_and_link(&dir);
        let (f, l) = (cpath(&f), cpath(&l));
        assert_eq!(CSys.faccessat2(Fd::CWD, KStr::from_nul_terminated(f.as_bytes_with_nul()).unwrap(), 4, 0x200), Err(Errno::ENOSYS), "the trap");
        assert_eq!(fs::faccessat(AT_FDCWD, f.as_ptr(), R_OK, AT_EACCESS), 0, "R_OK on a 0600 file");
        errno::set_raw(0);
        // Without any execute bit X_OK fails for every user, root included.
        assert_eq!((fs::faccessat(AT_FDCWD, f.as_ptr(), X_OK, AT_EACCESS), errno::get()), (-1, EACCES), "X_OK on a 0600 file");
        assert_eq!(fs::faccessat(AT_FDCWD, l.as_ptr(), F_OK, AT_SYMLINK_NOFOLLOW), 0, "the dangling symlink itself exists");
        errno::set_raw(0);
        assert_eq!((fs::faccessat(AT_FDCWD, l.as_ptr(), F_OK, AT_EACCESS), errno::get()), (-1, ENOENT), "its target does not");
        return done(calls);
    }
    under_trap("faccessat2_enosys_faccessat_emulates", calls);
}

#[test]
fn statx_enosys_emulated_from_fstatat() {
    // Trace: s13_6_c_0 (statx fills the inode's metadata when statx is ENOSYS: emulated with fstatat, mask 0x7ff, other fields zero; flags fstatat cannot honour are EINVAL).
    let calls = "statx";
    if let Some(dir) = child(calls) {
        use liella_kernel::uapi::Statx;
        let (f, l) = file_and_link(&dir);
        let (f, l) = (cpath(&f), cpath(&l));
        let mut raw = Statx::default();
        assert_eq!(CSys.statx_into(Fd::CWD, KStr::from_nul_terminated(f.as_bytes_with_nul()).unwrap(), 0, 0, &mut raw), Ok(()), "the emulation answers where the call is trapped");
        let mut x = Statx::default();
        let call = |p: *const c_char, fl: i32, mask: u32, x: &mut Statx| (fs::statx(AT_FDCWD, p, fl, mask, Some(x)), errno::get());
        // Mask 0x7ff whatever is asked (even bits no kernel knows); the unprovided fields stay zero.
        for mask in [0u32, 0xfff, 0x8000_0000] {
            x = Statx::default();
            assert_eq!(call(f.as_ptr(), 0, mask, &mut x).0, 0, "regular file, mask {mask:x}");
            assert_eq!(x.mask, 0x7ff);
            assert_eq!((u32::from(x.mode), x.size, x.nlink), (0o100_600, 1, 1));
            assert!(x.btime.sec == 0 && x.mnt_id == 0 && x.attributes == 0 && x.attributes_mask == 0 && x.dio_mem_align == 0);
        }
        let st = std::fs::metadata(std::str::from_utf8(f.to_bytes()).unwrap()).unwrap();
        {
            use std::os::unix::fs::MetadataExt;
            assert_eq!((x.ino, u64::from(x.uid), i64::from(x.blksize)), (st.ino(), u64::from(st.uid()), i64::try_from(st.blksize()).unwrap()));
            assert_eq!((i64::from(x.mtime.nsec), x.mtime.sec), (st.mtime_nsec(), st.mtime()));
        }
        errno::set_raw(0);
        assert_eq!(call(l.as_ptr(), 0, 0x7ff, &mut x), (-1, ENOENT), "a dangling symlink followed");
        assert_eq!(call(l.as_ptr(), AT_SYMLINK_NOFOLLOW, 0x7ff, &mut x).0, 0, "AT_SYMLINK_NOFOLLOW");
        assert_eq!(u32::from(x.mode) & 0o170_000, 0o120_000);
        let fd = CSys.openat(Fd::CWD, KStr::from_nul_terminated(f.as_bytes_with_nul()).unwrap(), 0, 0).unwrap();
        let empty = cs("");
        let mut y = Statx::default();
        assert_eq!(fs::statx(fd.raw(), empty.as_ptr(), 0x1000, 0x7ff, Some(&mut y)), 0, "AT_EMPTY_PATH");
        assert_eq!((y.mask, y.ino, y.size), (0x7ff, st_ino(&f), 1));
        errno::set_raw(0);
        assert_eq!((fs::statx(fd.raw(), empty.as_ptr(), 0, 0x7ff, Some(&mut y)), errno::get()), (-1, ENOENT), "empty path without AT_EMPTY_PATH");
        assert_eq!(fs::statx(AT_FDCWD, f.as_ptr(), 0x800, 0x7ff, Some(&mut y)), 0, "AT_NO_AUTOMOUNT");
        assert_eq!(fs::statx(AT_FDCWD, f.as_ptr(), 0, 0x7ff, Some(&mut y)), 0);
        // Flags fstatat cannot honour: EINVAL (AT_STATX_FORCE_SYNC, AT_STATX_DONT_SYNC, AT_REMOVEDIR, AT_EACCESS, unknown).
        for fl in [0x2000, 0x4000, 0x6000, 0x200, 0x200 | 0x100, 0x1, 0x8000_0000u32 as i32] {
            errno::set_raw(0);
            assert_eq!((fs::statx(AT_FDCWD, f.as_ptr(), fl, 0x7ff, Some(&mut y)), errno::get()), (-1, 22), "flags {fl:x}");
        }
        let _ = CSys.close(fd);
        return done(calls);
    }
    under_trap("statx_enosys_emulated_from_fstatat", calls);
}

fn st_ino(p: &std::ffi::CStr) -> u64 {
    use std::os::unix::fs::MetadataExt;
    std::fs::metadata(std::str::from_utf8(p.to_bytes()).unwrap()).unwrap().ino()
}

#[test]
fn fchmodat2_enosys_nofollow_falls_back() {
    // Trace: s13_7_c_1 (fchmodat AT_SYMLINK_NOFOLLOW through O_PATH and /proc/self/fd when fchmodat2 is ENOSYS).
    let calls = "fchmodat2";
    if let Some(dir) = child(calls) {
        let (fp, lp) = file_and_link(&dir);
        let (f, l) = (cpath(&fp), cpath(&lp));
        assert_eq!(CSys.fchmodat2(Fd::CWD, KStr::from_nul_terminated(f.as_bytes_with_nul()).unwrap(), 0o640, 0x100), Err(Errno::ENOSYS), "the trap");
        assert_eq!(fs::fchmodat(AT_FDCWD, f.as_ptr(), 0o640, AT_SYMLINK_NOFOLLOW), 0, "a regular file");
        assert_eq!(mode_of(&fp), 0o640, "its mode changed");
        errno::set_raw(0);
        assert_eq!((fs::fchmodat(AT_FDCWD, l.as_ptr(), 0o640, AT_SYMLINK_NOFOLLOW), errno::get()), (-1, EOPNOTSUPP), "a symlink has no mode");
        return done(calls);
    }
    under_trap("fchmodat2_enosys_nofollow_falls_back", calls);
}

#[test]
fn getrandom_enosys_mkstemp_still_unique() {
    // Trace: s13_9_c_1 (mkstemp names when getrandom is ENOSYS); s9_1_c_1 (getrandom itself reports ENOSYS, as glibc).
    let calls = "getrandom";
    if let Some(dir) = child(calls) {
        let mut b = [0u8; 8];
        assert_eq!(CSys.getrandom(&mut b, 0), Err(Errno::ENOSYS), "the trap");
        let mut names = Vec::new();
        for _ in 0..4 {
            let mut t = cpath(&dir.join("tXXXXXX")).into_bytes_with_nul();
            let fd = fs::mkstemp(t.as_mut_ptr().cast());
            assert!(fd >= 0, "mkstemp: errno {}", errno::get());
            let name = PathBuf::from(String::from_utf8(t[..t.len() - 1].to_vec()).unwrap());
            assert_eq!(mode_of(&name), 0o600, "{}", name.display());
            let _ = CSys.close(Fd::new(fd));
            names.push(name);
        }
        names.sort();
        names.dedup();
        assert_eq!(names.len(), 4, "four distinct names: {names:?}");
        errno::set_raw(0);
        assert_eq!((rnd::getrandom(b.as_mut_ptr().cast(), 8, 0), errno::get()), (-1, ENOSYS), "getrandom");
        return done(calls);
    }
    under_trap("getrandom_enosys_mkstemp_still_unique", calls);
}

#[test]
fn pidfd_open_enosys_reported() {
    // Trace: s18_5_c_1 (pidfd_open has no fallback; ENOSYS is reported, as glibc reports it).
    let calls = "pidfd_open";
    if child(calls).is_some() {
        errno::set_raw(0);
        assert_eq!((x::pidfd_open(std::process::id() as i32, 0), errno::get()), (-1, ENOSYS));
        return done(calls);
    }
    under_trap("pidfd_open_enosys_reported", calls);
}
