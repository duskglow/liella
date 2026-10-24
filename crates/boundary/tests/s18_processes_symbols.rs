// Specified from spec §18: §18.1 (process identity) in phase 5; fork, vfork and clone in phase 6
// (checked through the conformance programs); the rest in phase 11.
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Translation-layer tests for the exported symbols of spec §18.
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! Each test checks the export (name and default version in `libc.so.6`) and the C conventions
//! of the translation: `-1` and `errno`, or an error number returned (the spawn family), the C
//! objects' layouts and values. The variadic entry points (`execl`, `execle`, `execlp`, `prctl`,
//! `ptrace`) and `daemon` (products only) are exercised by `conformance/c/s18_exec.c`; their
//! fixed-arity targets are called here. Behaviour is the clause tests' (`s18_processes_clauses`).
//! `posix_spawn@GLIBC_2.2.5` and `posix_spawnp@GLIBC_2.2.5` (the compat versions) arrive with
//! phase 15.
#![allow(non_snake_case)]
#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]

use core::mem::{align_of, offset_of, size_of};
use std::ffi::CString;

use liella_abi::ctypes::c_char;
use liella_abi::layout::sigset_t;
use liella_boundary::errno;
use liella_boundary::posix::s18_processes as s;
use liella_boundary::sys::CSys;
use liella_kernel::fd::Fd;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Rusage, SchedParam, SigInfo};
use liella_legacy::spawn_compat::{CFileActions, CSpawnAttr};

mod products;
use products::{conformance, exported};

const EPERM: i32 = 1;
const ENOENT: i32 = 2;
const ESRCH: i32 = 3;
const EBADF: i32 = 9;
const ECHILD: i32 = 10;
const EACCES: i32 = 13;
const EINVAL: i32 = 22;

fn status(key: &str) -> i32 {
    let text = std::fs::read_to_string("/proc/thread-self/status").unwrap();
    text.lines().find(|l| l.starts_with(key)).unwrap()[key.len()..].trim().parse().unwrap()
}

/// Field `n` (1-based, as proc(5) numbers them) of `/proc/self/stat`.
fn stat_field(n: usize) -> i32 {
    let text = std::fs::read_to_string("/proc/self/stat").unwrap();
    let after = &text[text.rfind(')').unwrap() + 2..];
    after.split(' ').nth(n - 3).unwrap().parse().unwrap()
}

fn cs(s: &str) -> CString {
    CString::new(s).unwrap()
}

/// A C argument array that owns its strings.
struct Args {
    _keep: Vec<CString>,
    ptrs: Vec<*mut c_char>,
}

impl Args {
    fn new(items: &[&str]) -> Args {
        let keep: Vec<CString> = items.iter().map(|s| cs(s)).collect();
        let mut ptrs: Vec<*mut c_char> = keep.iter().map(|c| c.as_ptr().cast_mut()).collect();
        ptrs.push(core::ptr::null_mut());
        Args { _keep: keep, ptrs }
    }
    fn p(&self) -> *const *mut c_char {
        self.ptrs.as_ptr()
    }
}

fn environ() -> *const *mut c_char {
    core::ptr::with_exposed_provenance(liella_boundary::spawn::environ())
}

/// The result and errno of a call that should fail.
fn fails(r: i64) -> (i64, i32) {
    (r, errno::get())
}

/// `posix_spawn` of `sh -c script`: the error number and the child's status (0 on failure).
fn spawn_status(script: &str, fa: Option<&CFileActions>, attr: Option<&CSpawnAttr>) -> (i32, i32) {
    let argv = Args::new(&["sh", "-c", script]);
    let mut pid = -1;
    let r = s::posix_spawn(Some(&mut pid), cs("/bin/sh").as_ptr(), fa, attr, argv.p(), environ());
    let mut st = 0;
    if r == 0 {
        assert_eq!(s::waitpid(pid, Some(&mut st), 0), pid);
    }
    (r, st)
}

fn new_attr() -> Box<CSpawnAttr> {
    let mut at = Box::new(CSpawnAttr::new());
    assert_eq!(s::posix_spawnattr_init(Some(&mut at)), 0);
    at
}

fn new_actions() -> Box<CFileActions> {
    let mut fa = Box::new(CFileActions::new());
    assert_eq!(s::posix_spawn_file_actions_init(Some(&mut fa)), 0);
    fa
}

fn one_set(sig: i32) -> sigset_t {
    let mut set = sigset_t { __val: [0; 16] };
    set.__val[0] = 1u64 << (sig - 1);
    set
}

// ------------------------------------------------------------------------------ §18.9 attributes

#[test]
// ARCH(x86_64): arch_prctl is exported on x86_64 only (RL-8)
fn sym___arch_prctl() {
    exported("__arch_prctl");
    let mut fs = 0u64;
    // ARCH(x86_64): arch_prctl is exported on x86_64 only (RL-8)
    assert_eq!(s::__arch_prctl(0x1003, core::ptr::from_mut(&mut fs) as u64), 0, "ARCH_GET_FS");
    assert_eq!(fs as usize, liella_boundary::tb::tp());
    errno::set_raw(0);
    // ARCH(x86_64): arch_prctl is exported on x86_64 only (RL-8)
    assert_eq!(fails(i64::from(s::__arch_prctl(0x7777, 0))), (-1, EINVAL));
}

#[test]
// ARCH(x86_64): arch_prctl is exported on x86_64 only (RL-8)
fn sym_arch_prctl() {
    exported("arch_prctl");
    let mut fs = 0u64;
    // ARCH(x86_64): arch_prctl is exported on x86_64 only (RL-8)
    assert_eq!(s::arch_prctl(0x1003, core::ptr::from_mut(&mut fs) as u64), 0, "ARCH_GET_FS");
    assert_eq!(fs as usize, liella_boundary::tb::tp());
    errno::set_raw(0);
    // ARCH(x86_64): arch_prctl is exported on x86_64 only (RL-8)
    assert_eq!(fails(i64::from(s::arch_prctl(0x7777, 0))), (-1, EINVAL));
}

// ------------------------------------------------------------------------------ §18.8 groups

#[test]
fn sym___bsd_getpgrp() {
    exported("__bsd_getpgrp");
    assert_eq!(s::__bsd_getpgrp(0), stat_field(5), "getpgid(pid): the group of pid");
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::__bsd_getpgrp(0x3fff_fff0))), (-1, ESRCH));
}

#[test]
fn sym___clone() {
    exported("__clone");
    conformance(&["s18_fork_aliases"]);
}
#[test]
fn sym_clone() {
    exported("clone");
    conformance(&["s18_fork"]);
}

#[test]
fn sym_daemon() {
    exported("daemon");
    conformance(&["s18_exec"]);
}

// ------------------------------------------------------------------------------ §18.3 exec

#[test]
fn sym_execl() {
    // Variadic: the C entry point (and its va_list reader, which a test build cannot link)
    // runs in the conformance program.
    exported("execl");
    conformance(&["s18_exec"]);
}

#[test]
fn sym_execle() {
    exported("execle");
    conformance(&["s18_exec"]);
}

#[test]
fn sym_execlp() {
    exported("execlp");
    conformance(&["s18_exec"]);
}

#[test]
fn sym_execv() {
    exported("execv");
    errno::set_raw(0);
    let a = Args::new(&["x"]);
    assert_eq!(fails(i64::from(s::execv(cs("/nonexistent/x").as_ptr(), a.p()))), (-1, ENOENT));
    assert_eq!(fails(i64::from(s::execv(cs("/").as_ptr(), a.p()))), (-1, EACCES));
}

#[test]
fn sym_execve() {
    exported("execve");
    errno::set_raw(0);
    let a = Args::new(&["x"]);
    assert_eq!(fails(i64::from(s::execve(cs("/nonexistent/x").as_ptr(), a.p(), environ()))), (-1, ENOENT));
    assert_eq!(fails(i64::from(s::execve(core::ptr::null(), a.p(), environ()))), (-1, 14), "EFAULT for a NULL path");
}

#[test]
fn sym_execveat() {
    exported("execveat");
    errno::set_raw(0);
    let a = Args::new(&["x"]);
    assert_eq!(fails(i64::from(s::execveat(-100, cs("/bin/sh").as_ptr(), a.p(), environ(), 0x1))), (-1, EINVAL), "an unknown flag");
    assert_eq!(fails(i64::from(s::execveat(-5, cs("sh").as_ptr(), a.p(), environ(), 0))), (-1, EBADF));
}

#[test]
fn sym_execvp() {
    exported("execvp");
    errno::set_raw(0);
    let a = Args::new(&["x"]);
    assert_eq!(fails(i64::from(s::execvp(cs("liella-no-such-program-xyz").as_ptr(), a.p()))), (-1, ENOENT), "not found along PATH");
    assert_eq!(fails(i64::from(s::execvp(cs("").as_ptr(), a.p()))), (-1, ENOENT));
}

#[test]
fn sym_execvpe() {
    exported("execvpe");
    errno::set_raw(0);
    let a = Args::new(&["x"]);
    assert_eq!(fails(i64::from(s::execvpe(cs("liella-no-such-program-xyz").as_ptr(), a.p(), environ()))), (-1, ENOENT));
}

#[test]
fn sym_fexecve() {
    exported("fexecve");
    errno::set_raw(0);
    let a = Args::new(&["x"]);
    assert_eq!(fails(i64::from(s::fexecve(-1, a.p(), environ()))), (-1, EINVAL));
    assert_eq!(fails(i64::from(s::fexecve(1000, a.p(), environ()))), (-1, EBADF));
}

#[test]
fn sym___fork() {
    exported("__fork");
    conformance(&["s18_fork_aliases"]);
}
#[test]
fn sym_fork() {
    exported("fork");
    conformance(&["s18_fork", "s22_more"]);
}
#[test]
fn sym__Fork() {
    exported("_Fork");
    conformance(&["s18_fork"]);
}

#[test]
fn sym___getpgid() {
    exported("__getpgid");
    assert_eq!(s::__getpgid(0), stat_field(5));
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::__getpgid(0x3fff_fff0))), (-1, ESRCH));
}

#[test]
fn sym_getpgid() {
    exported("getpgid");
    assert_eq!(s::getpgid(0), stat_field(5));
    assert_eq!(s::getpgid(s::getpid()), stat_field(5));
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::getpgid(0x3fff_fff0))), (-1, ESRCH));
}

#[test]
fn sym_getpgrp() {
    exported("getpgrp");
    assert_eq!(s::getpgrp(), stat_field(5));
}

#[test]
fn sym___getpid() {
    assert_eq!(s::__getpid(), std::process::id() as i32, "the thread-group ID");
    let other = std::thread::spawn(|| s::__getpid()).join().unwrap();
    assert_eq!(other, s::__getpid(), "the same in every thread");
}

#[test]
fn sym_getpid() {
    assert_eq!(s::getpid(), std::process::id() as i32, "the thread-group ID");
    let other = std::thread::spawn(|| s::getpid()).join().unwrap();
    assert_eq!(other, s::getpid(), "the same in every thread");
}

#[test]
fn sym_getppid() {
    assert_eq!(s::getppid(), status("PPid:"));
    assert_eq!(s::getppid(), std::os::unix::process::parent_id() as i32);
}

#[test]
fn sym_getsid() {
    exported("getsid");
    assert_eq!(s::getsid(0), stat_field(6));
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::getsid(0x3fff_fff0))), (-1, ESRCH));
}

#[test]
fn sym_gettid() {
    assert_eq!(s::gettid(), status("Pid:"), "this thread's kernel ID");
    let other = std::thread::spawn(|| s::gettid()).join().unwrap();
    assert_ne!(other, s::gettid(), "per thread");
}

#[test]
fn sym_personality() {
    exported("personality");
    let text = std::fs::read_to_string("/proc/self/personality").unwrap();
    assert_eq!(s::personality(0xffff_ffff), i32::from_str_radix(text.trim(), 16).unwrap(), "0xffffffff queries");
}

// ------------------------------------------------------------------------------ §18.5 pidfd

#[test]
fn sym_pidfd_getfd() {
    exported("pidfd_getfd");
    let me = s::pidfd_open(s::getpid(), 0);
    let got = s::pidfd_getfd(me, 1, 0);
    assert!(got >= 0, "a duplicate of one's own descriptor 1: errno {}", errno::get());
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::pidfd_getfd(me, 1, 1))), (-1, EINVAL));
    assert_eq!(fails(i64::from(s::pidfd_getfd(-1, 1, 0))), (-1, EBADF));
    for fd in [got, me] {
        let _ = CSys.close(Fd::new(fd));
    }
}

#[test]
fn sym_pidfd_getpid() {
    exported("pidfd_getpid");
    let me = s::pidfd_open(s::getpid(), 0);
    assert_eq!(s::pidfd_getpid(me), s::getpid());
    let _ = CSys.close(Fd::new(me));
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::pidfd_getpid(-1))), (-1, EBADF));
    assert_eq!(fails(i64::from(s::pidfd_getpid(0x7fff))), (-1, EBADF), "not open");
}

#[test]
fn sym_pidfd_open() {
    exported("pidfd_open");
    let me = s::pidfd_open(s::getpid(), 0);
    assert!(me >= 0);
    let _ = CSys.close(Fd::new(me));
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::pidfd_open(0x3fff_fff0, 0))), (-1, ESRCH));
    assert_eq!(fails(i64::from(s::pidfd_open(s::getpid(), 1))), (-1, EINVAL));
}

#[test]
fn sym_pidfd_send_signal() {
    exported("pidfd_send_signal");
    let me = s::pidfd_open(s::getpid(), 0);
    assert_eq!(s::pidfd_send_signal(me, 0, None, 0), 0, "signal 0 checks");
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::pidfd_send_signal(me, 0, None, 1))), (-1, EINVAL));
    assert_eq!(fails(i64::from(s::pidfd_send_signal(-1, 0, None, 0))), (-1, EBADF));
    let _ = CSys.close(Fd::new(me));
}

// ------------------------------------------------------------------------------ §18.4 spawn

#[test]
fn sym_pidfd_spawn() {
    exported("pidfd_spawn");
    let argv = Args::new(&["sh", "-c", "exit 6"]);
    let mut pfd = -1;
    assert_eq!(s::pidfd_spawn(Some(&mut pfd), cs("/bin/sh").as_ptr(), None, None, argv.p(), environ()), 0);
    let mut si = SigInfo::default();
    assert_eq!(s::waitid(3, pfd as u32, Some(&mut si), 4), 0, "P_PIDFD, WEXITED");
    assert_eq!(si.status(), 6);
    let _ = CSys.close(Fd::new(pfd));
    let mut untouched = -77;
    assert_eq!(s::pidfd_spawn(Some(&mut untouched), cs("/nonexistent/x").as_ptr(), None, None, argv.p(), environ()), ENOENT, "an error number, not -1");
    assert_eq!(untouched, -77, "not written on failure");
}

#[test]
fn sym_pidfd_spawnp() {
    exported("pidfd_spawnp");
    let argv = Args::new(&["sh", "-c", "exit 7"]);
    let mut pfd = -1;
    assert_eq!(s::pidfd_spawnp(Some(&mut pfd), cs("sh").as_ptr(), None, None, argv.p(), environ()), 0);
    let mut si = SigInfo::default();
    assert_eq!(s::waitid(3, pfd as u32, Some(&mut si), 4), 0);
    assert_eq!(si.status(), 7);
    let _ = CSys.close(Fd::new(pfd));
}

#[test]
fn sym_posix_spawn() {
    exported("posix_spawn");
    assert_eq!(spawn_status("exit 3", None, None), (0, 3 << 8));
    let argv = Args::new(&["x"]);
    let mut pid = -77;
    errno::set_raw(0);
    assert_eq!(s::posix_spawn(Some(&mut pid), cs("/nonexistent/x").as_ptr(), None, None, argv.p(), environ()), ENOENT, "an error number");
    assert_eq!(errno::get(), 0, "errno untouched");
    let ok = Args::new(&["sh", "-c", "exit 0"]);
    assert_eq!(s::posix_spawn(None, cs("/bin/sh").as_ptr(), None, None, ok.p(), environ()), 0, "pid may be NULL");
}

#[test]
fn sym_posix_spawn_file_actions_addchdir_np() {
    exported("posix_spawn_file_actions_addchdir_np");
    let mut fa = new_actions();
    assert_eq!(s::posix_spawn_file_actions_addchdir_np(Some(&mut fa), cs("/").as_ptr()), 0);
    let (r, st) = spawn_status("[ \"$(pwd)\" = / ]", Some(&fa), None);
    assert_eq!((r, st), (0, 0));
    assert_eq!(s::posix_spawn_file_actions_destroy(Some(&mut fa)), 0);
}

#[test]
fn sym_posix_spawn_file_actions_addclose() {
    exported("posix_spawn_file_actions_addclose");
    let mut fa = new_actions();
    assert_eq!(s::posix_spawn_file_actions_addclose(Some(&mut fa), -1), EBADF);
    assert_eq!(s::posix_spawn_file_actions_addclose(Some(&mut fa), i32::MAX), EBADF, "at or above the descriptor limit");
    assert_eq!(s::posix_spawn_file_actions_addclose(Some(&mut fa), 0), 0);
    assert_eq!(spawn_status("[ -e /proc/$$/fd/0 ] && exit 1; exit 0", Some(&fa), None), (0, 0));
    s::posix_spawn_file_actions_destroy(Some(&mut fa));
}

#[test]
fn sym_posix_spawn_file_actions_addclosefrom_np() {
    exported("posix_spawn_file_actions_addclosefrom_np");
    let mut fa = new_actions();
    assert_eq!(s::posix_spawn_file_actions_addclosefrom_np(Some(&mut fa), -1), EBADF);
    assert_eq!(s::posix_spawn_file_actions_addclosefrom_np(Some(&mut fa), 1), 0);
    assert_eq!(spawn_status("[ -e /proc/$$/fd/1 ] && exit 1; exit 0", Some(&fa), None), (0, 0));
    s::posix_spawn_file_actions_destroy(Some(&mut fa));
}

#[test]
fn sym_posix_spawn_file_actions_adddup2() {
    exported("posix_spawn_file_actions_adddup2");
    let mut fa = new_actions();
    assert_eq!(s::posix_spawn_file_actions_adddup2(Some(&mut fa), -1, 5), EBADF);
    assert_eq!(s::posix_spawn_file_actions_adddup2(Some(&mut fa), 1, -1), EBADF);
    assert_eq!(s::posix_spawn_file_actions_adddup2(Some(&mut fa), 2, 7), 0);
    assert_eq!(spawn_status("[ -e /proc/$$/fd/7 ]", Some(&fa), None), (0, 0));
    s::posix_spawn_file_actions_destroy(Some(&mut fa));
}

#[test]
fn sym_posix_spawn_file_actions_addfchdir_np() {
    exported("posix_spawn_file_actions_addfchdir_np");
    let mut fa = new_actions();
    assert_eq!(s::posix_spawn_file_actions_addfchdir_np(Some(&mut fa), -1), EBADF, "checked when added, as every descriptor");
    let root = CSys.openat(Fd::CWD, liella_kernel::KStr::from_nul_terminated(b"/\0").unwrap(), 0o2000000 | 0o200000, 0).unwrap();
    assert_eq!(s::posix_spawn_file_actions_adddup2(Some(&mut fa), root.raw(), 9), 0);
    assert_eq!(s::posix_spawn_file_actions_addfchdir_np(Some(&mut fa), 9), 0);
    assert_eq!(spawn_status("[ \"$(pwd)\" = / ]", Some(&fa), None), (0, 0));
    let _ = CSys.close(root);
    s::posix_spawn_file_actions_destroy(Some(&mut fa));
}

#[test]
fn sym_posix_spawn_file_actions_addopen() {
    exported("posix_spawn_file_actions_addopen");
    let mut fa = new_actions();
    assert_eq!(s::posix_spawn_file_actions_addopen(Some(&mut fa), -1, cs("/dev/null").as_ptr(), 0, 0), EBADF);
    assert_eq!(s::posix_spawn_file_actions_addopen(Some(&mut fa), 6, cs("/dev/null").as_ptr(), 0, 0), 0);
    assert_eq!(spawn_status("[ -e /proc/$$/fd/6 ]", Some(&fa), None), (0, 0));
    s::posix_spawn_file_actions_destroy(Some(&mut fa));
}

#[test]
fn sym_posix_spawn_file_actions_addtcsetpgrp_np() {
    exported("posix_spawn_file_actions_addtcsetpgrp_np");
    let mut fa = new_actions();
    assert_eq!(s::posix_spawn_file_actions_addtcsetpgrp_np(Some(&mut fa), -1), EBADF);
    let null = CSys.openat(Fd::CWD, liella_kernel::KStr::from_nul_terminated(b"/dev/null\0").unwrap(), 0o2000000, 0).unwrap();
    assert_eq!(s::posix_spawn_file_actions_addtcsetpgrp_np(Some(&mut fa), null.raw()), 0);
    assert_eq!(spawn_status("exit 0", Some(&fa), None).0, 25, "ENOTTY from the child, as the result");
    let _ = CSys.close(null);
    s::posix_spawn_file_actions_destroy(Some(&mut fa));
}

#[test]
fn sym_posix_spawn_file_actions_destroy() {
    exported("posix_spawn_file_actions_destroy");
    let mut fa = new_actions();
    s::posix_spawn_file_actions_addopen(Some(&mut fa), 5, cs("/dev/null").as_ptr(), 0, 0);
    assert_ne!(fa.block().0, 0, "the list has a block");
    assert_eq!(s::posix_spawn_file_actions_destroy(Some(&mut fa)), 0);
    assert_eq!(*fa, CFileActions::new(), "released and cleared");
    assert_eq!(s::posix_spawn_file_actions_destroy(None), EINVAL);
}

#[test]
fn sym_posix_spawn_file_actions_init() {
    exported("posix_spawn_file_actions_init");
    assert_eq!((size_of::<CFileActions>(), align_of::<CFileActions>()), (80, 8), "Appendix C");
    assert_eq!((offset_of!(CFileActions, allocated), offset_of!(CFileActions, used), offset_of!(CFileActions, actions)), (0, 4, 8));
    let mut fa = CFileActions::new();
    assert_eq!(s::posix_spawn_file_actions_init(Some(&mut fa)), 0);
    assert_eq!(fa.block(), (0, 0), "empty");
    assert_eq!(s::posix_spawn_file_actions_init(None), EINVAL);
}

#[test]
fn sym_posix_spawnattr_destroy() {
    exported("posix_spawnattr_destroy");
    let mut at = new_attr();
    assert_eq!(s::posix_spawnattr_destroy(Some(&mut at)), 0);
    assert_eq!(s::posix_spawnattr_destroy(None), EINVAL);
}

#[test]
fn sym_posix_spawnattr_getcgroup_np() {
    exported("posix_spawnattr_getcgroup_np");
    let at = new_attr();
    let mut cg = -5;
    assert_eq!(s::posix_spawnattr_getcgroup_np(Some(&at), Some(&mut cg)), 0);
    assert_eq!(cg, 0, "the default");
}

#[test]
fn sym_posix_spawnattr_getflags() {
    exported("posix_spawnattr_getflags");
    let at = new_attr();
    let mut fl = -1i16;
    assert_eq!(s::posix_spawnattr_getflags(Some(&at), Some(&mut fl)), 0);
    assert_eq!(fl, 0);
    assert_eq!(s::posix_spawnattr_getflags(None, Some(&mut fl)), EINVAL);
}

#[test]
fn sym_posix_spawnattr_getpgroup() {
    exported("posix_spawnattr_getpgroup");
    let at = new_attr();
    let mut pg = -1;
    assert_eq!(s::posix_spawnattr_getpgroup(Some(&at), Some(&mut pg)), 0);
    assert_eq!(pg, 0);
}

#[test]
fn sym_posix_spawnattr_getschedparam() {
    exported("posix_spawnattr_getschedparam");
    let at = new_attr();
    let mut p = SchedParam { priority: -1 };
    assert_eq!(s::posix_spawnattr_getschedparam(Some(&at), Some(&mut p)), 0);
    assert_eq!(p.priority, 0);
}

#[test]
fn sym_posix_spawnattr_getschedpolicy() {
    exported("posix_spawnattr_getschedpolicy");
    let at = new_attr();
    let mut p = -1;
    assert_eq!(s::posix_spawnattr_getschedpolicy(Some(&at), Some(&mut p)), 0);
    assert_eq!(p, 0);
}

#[test]
fn sym_posix_spawnattr_getsigdefault() {
    exported("posix_spawnattr_getsigdefault");
    let at = new_attr();
    let mut set = sigset_t { __val: [u64::MAX; 16] };
    assert_eq!(s::posix_spawnattr_getsigdefault(Some(&at), Some(&mut set)), 0);
    assert_eq!(set.__val, [0; 16], "empty");
}

#[test]
fn sym_posix_spawnattr_getsigmask() {
    exported("posix_spawnattr_getsigmask");
    let at = new_attr();
    let mut set = sigset_t { __val: [u64::MAX; 16] };
    assert_eq!(s::posix_spawnattr_getsigmask(Some(&at), Some(&mut set)), 0);
    assert_eq!(set.__val, [0; 16], "empty");
}

#[test]
fn sym_posix_spawnattr_init() {
    exported("posix_spawnattr_init");
    assert_eq!((size_of::<CSpawnAttr>(), align_of::<CSpawnAttr>()), (336, 8), "Appendix C");
    let offsets = (offset_of!(CSpawnAttr, flags), offset_of!(CSpawnAttr, pgrp), offset_of!(CSpawnAttr, sd), offset_of!(CSpawnAttr, ss), offset_of!(CSpawnAttr, priority), offset_of!(CSpawnAttr, policy), offset_of!(CSpawnAttr, cgroup));
    assert_eq!(offsets, (0, 4, 8, 136, 264, 268, 272), "Appendix C");
    let mut at = CSpawnAttr::new();
    at.pgrp = 9;
    assert_eq!(s::posix_spawnattr_init(Some(&mut at)), 0);
    assert_eq!((at.pgrp, at.flags, at.policy, at.priority, at.cgroup), (0, 0, 0, 0, 0), "every value reset");
    assert_eq!(s::posix_spawnattr_init(None), EINVAL);
}

#[test]
fn sym_posix_spawnattr_setcgroup_np() {
    exported("posix_spawnattr_setcgroup_np");
    let mut at = new_attr();
    assert_eq!(s::posix_spawnattr_setcgroup_np(Some(&mut at), 42), 0);
    let mut cg = 0;
    s::posix_spawnattr_getcgroup_np(Some(&at), Some(&mut cg));
    assert_eq!(cg, 42);
}

#[test]
fn sym_posix_spawnattr_setflags() {
    exported("posix_spawnattr_setflags");
    let mut at = new_attr();
    assert_eq!(s::posix_spawnattr_setflags(Some(&mut at), 0x1ff), 0);
    assert_eq!(s::posix_spawnattr_setflags(Some(&mut at), 0x200), EINVAL, "an error number");
    let mut fl = 0i16;
    s::posix_spawnattr_getflags(Some(&at), Some(&mut fl));
    assert_eq!(fl, 0x1ff);
}

#[test]
fn sym_posix_spawnattr_setpgroup() {
    exported("posix_spawnattr_setpgroup");
    let mut at = new_attr();
    assert_eq!(s::posix_spawnattr_setpgroup(Some(&mut at), 1234), 0);
    let mut pg = 0;
    s::posix_spawnattr_getpgroup(Some(&at), Some(&mut pg));
    assert_eq!(pg, 1234);
    s::posix_spawnattr_setflags(Some(&mut at), 0x02);
    s::posix_spawnattr_setpgroup(Some(&mut at), 0);
    assert_eq!(spawn_status("[ $(cut -d' ' -f5 /proc/$$/stat) = $$ ]", None, Some(&at)), (0, 0), "a group of its own");
}

#[test]
fn sym_posix_spawnattr_setschedparam() {
    exported("posix_spawnattr_setschedparam");
    let mut at = new_attr();
    assert_eq!(s::posix_spawnattr_setschedparam(Some(&mut at), Some(&SchedParam { priority: 1000 })), 0, "not checked until the spawn");
    let mut p = SchedParam { priority: 0 };
    s::posix_spawnattr_getschedparam(Some(&at), Some(&mut p));
    assert_eq!(p.priority, 1000);
    assert_eq!(s::posix_spawnattr_setschedparam(Some(&mut at), None), EINVAL);
}

#[test]
fn sym_posix_spawnattr_setschedpolicy() {
    exported("posix_spawnattr_setschedpolicy");
    let mut at = new_attr();
    for (policy, want) in [(0, 0), (1, 0), (2, 0), (3, 0), (5, 0), (6, EINVAL), (-1, EINVAL), (99, EINVAL)] {
        assert_eq!(s::posix_spawnattr_setschedpolicy(Some(&mut at), policy), want, "policy {policy}");
    }
    let mut p = -1;
    s::posix_spawnattr_getschedpolicy(Some(&at), Some(&mut p));
    assert_eq!(p, 5, "the last accepted value");
}

#[test]
fn sym_posix_spawnattr_setsigdefault() {
    exported("posix_spawnattr_setsigdefault");
    let mut at = new_attr();
    assert_eq!(s::posix_spawnattr_setsigdefault(Some(&mut at), Some(&one_set(9))), 0, "SIGKILL is accepted");
    let mut got = sigset_t { __val: [0; 16] };
    s::posix_spawnattr_getsigdefault(Some(&at), Some(&mut got));
    assert_eq!(got.__val[0], 1 << 8);
    let mut full = sigset_t { __val: [u64::MAX; 16] };
    full.__val[0] = u64::MAX;
    s::posix_spawnattr_setsigdefault(Some(&mut at), Some(&full));
    s::posix_spawnattr_getsigdefault(Some(&at), Some(&mut got));
    assert_eq!(got.__val[0] & (1 << 31), 0, "the library's signal is never stored");
    assert_eq!(s::posix_spawnattr_setsigdefault(Some(&mut at), None), EINVAL);
}

#[test]
fn sym_posix_spawnattr_setsigmask() {
    exported("posix_spawnattr_setsigmask");
    let mut at = new_attr();
    assert_eq!(s::posix_spawnattr_setsigmask(Some(&mut at), Some(&one_set(10))), 0);
    let mut got = sigset_t { __val: [0; 16] };
    s::posix_spawnattr_getsigmask(Some(&at), Some(&mut got));
    assert_eq!(got.__val[0], 1 << 9);
    s::posix_spawnattr_setflags(Some(&mut at), 0x08);
    let argv = Args::new(&["grep", "-q", "SigBlk:.0000000000000200", "/proc/self/status"]);
    let mut pid = -1;
    assert_eq!(s::posix_spawn(Some(&mut pid), cs("/bin/grep").as_ptr(), None, Some(&at), argv.p(), environ()), 0);
    let mut st = -1;
    assert_eq!(s::waitpid(pid, Some(&mut st), 0), pid);
    assert_eq!(st, 0, "the program starts with SIGUSR1 blocked");
}

#[test]
fn sym_posix_spawnp() {
    exported("posix_spawnp");
    let argv = Args::new(&["sh", "-c", "exit 2"]);
    let mut pid = -1;
    assert_eq!(s::posix_spawnp(Some(&mut pid), cs("sh").as_ptr(), None, None, argv.p(), environ()), 0);
    let mut st = 0;
    assert_eq!(s::waitpid(pid, Some(&mut st), 0), pid);
    assert_eq!(st, 2 << 8);
    assert_eq!(s::posix_spawnp(Some(&mut pid), cs("liella-no-such-program-xyz").as_ptr(), None, None, argv.p(), environ()), ENOENT);
}

#[test]
fn sym_prctl() {
    exported("prctl");
    assert_eq!(s::liella_prctl5(3, 0, 0, 0, 0), 1, "PR_GET_DUMPABLE: the value is the result");
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::liella_prctl5(0x7fff_1234, 0, 0, 0, 0))), (-1, EINVAL));
    conformance(&["s18_exec"]);
}

#[test]
fn sym_ptrace() {
    exported("ptrace");
    errno::set_raw(0);
    assert_eq!(fails(s::liella_ptrace4(2, 0x3fff_fff0, 0, 0)), (-1, ESRCH), "PEEKDATA of no process: -1 and errno");
    conformance(&["s18_exec"]);
}

#[test]
fn sym___register_atfork() {
    exported("__register_atfork");
    conformance(&["s18_fork_aliases"]);
}

#[test]
fn sym___setpgid() {
    exported("__setpgid");
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::__setpgid(-1, 0))), (-1, EINVAL));
    assert_eq!(fails(i64::from(s::__setpgid(1, 1))), (-1, ESRCH), "not a child");
}

#[test]
fn sym_setpgid() {
    exported("setpgid");
    let c = liella_boundary::selftest::spawn_pausing().unwrap();
    assert_eq!(s::setpgid(c, 0), 0);
    assert_eq!(s::getpgid(c), c);
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::setpgid(c, -1))), (-1, EINVAL));
    let _ = CSys.kill(c, 9);
    let _ = CSys.wait4(c, 0, None);
}

#[test]
fn sym_setpgrp() {
    exported("setpgrp");
    fn body(k: &CSys) -> i32 {
        i32::from(s::setpgrp() != 0 || s::getpgrp() != k.getpid())
    }
    assert_eq!(liella_boundary::selftest::in_child(body).unwrap(), liella_boundary::selftest::ChildEnd::Exited(0));
}

#[test]
fn sym_setsid() {
    exported("setsid");
    fn body(k: &CSys) -> i32 {
        if s::setsid() != k.getpid() {
            return 1;
        }
        errno::set_raw(0);
        i32::from(s::setsid() != -1 || errno::get() != EPERM)
    }
    assert_eq!(liella_boundary::selftest::in_child(body).unwrap(), liella_boundary::selftest::ChildEnd::Exited(0));
}

#[test]
fn sym_system() {
    exported("system");
    assert_eq!(s::system(core::ptr::null()), 1, "a shell is available");
    assert_eq!(s::system(cs("exit 5").as_ptr()), 5 << 8, "the wait status");
}

#[test]
fn sym___vfork() {
    exported("__vfork");
    conformance(&["s18_fork_aliases"]);
}
#[test]
fn sym_vfork() {
    exported("vfork");
    conformance(&["s18_fork"]);
}

// ------------------------------------------------------------------------------ §18.6 waiting

#[test]
fn sym___wait() {
    exported("__wait");
    // In a child: here `wait` could collect another test's child.
    fn body(_: &CSys) -> i32 {
        let pid = liella_boundary::selftest::spawn_exiting(3).unwrap_or(-1);
        let mut st = 0;
        i32::from(s::__wait(Some(&mut st)) != pid || st != 3 << 8)
    }
    assert_eq!(liella_boundary::selftest::in_child(body).unwrap(), liella_boundary::selftest::ChildEnd::Exited(0));
}

#[test]
fn sym_wait() {
    exported("wait");
    // Any child of this (multi-threaded) test process may be collected by `wait`: only the
    // convention with none of its own is checked, in a child process.
    fn body(_: &CSys) -> i32 {
        errno::set_raw(0);
        i32::from(s::wait(None) != -1 || errno::get() != ECHILD)
    }
    assert_eq!(liella_boundary::selftest::in_child(body).unwrap(), liella_boundary::selftest::ChildEnd::Exited(0));
}

#[test]
fn sym_wait3() {
    exported("wait3");
    fn body(_: &CSys) -> i32 {
        let mut ru = Rusage::default();
        errno::set_raw(0);
        i32::from(s::wait3(None, 0, Some(&mut ru)) != -1 || errno::get() != ECHILD)
    }
    assert_eq!(liella_boundary::selftest::in_child(body).unwrap(), liella_boundary::selftest::ChildEnd::Exited(0));
}

#[test]
fn sym_wait4() {
    exported("wait4");
    let pid = liella_boundary::selftest::spawn_exiting(4).unwrap();
    let mut st = 0;
    let mut ru = Rusage::default();
    assert_eq!(s::wait4(pid, Some(&mut st), 0, Some(&mut ru)), pid);
    assert_eq!(st, 4 << 8);
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::wait4(pid, None, 0, None))), (-1, ECHILD));
}

#[test]
fn sym_waitid() {
    exported("waitid");
    let pid = liella_boundary::selftest::spawn_exiting(5).unwrap();
    let mut si = SigInfo::default();
    assert_eq!(s::waitid(1, pid as u32, Some(&mut si), 4), 0, "P_PID, WEXITED");
    assert_eq!((si.code, si.status(), si.pid()), (1, 5, pid));
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::waitid(1, pid as u32, None, 4))), (-1, ECHILD), "NULL info is accepted");
    assert_eq!(fails(i64::from(s::waitid(1, pid as u32, None, 0))), (-1, EINVAL));
}

#[test]
fn sym___waitpid() {
    exported("__waitpid");
    let pid = liella_boundary::selftest::spawn_exiting(6).unwrap();
    let mut st = 0;
    assert_eq!(s::__waitpid(pid, Some(&mut st), 0), pid);
    assert_eq!(st, 6 << 8);
}

#[test]
fn sym_waitpid() {
    exported("waitpid");
    let pid = liella_boundary::selftest::spawn_pausing().unwrap();
    let mut st = -9;
    assert_eq!(s::waitpid(pid, Some(&mut st), 1), 0, "WNOHANG: nothing yet");
    assert_eq!(st, -9, "status untouched when no child is reported");
    let _ = CSys.kill(pid, 9);
    assert_eq!(s::waitpid(pid, Some(&mut st), 0), pid);
    assert_eq!(st, 9, "killed by SIGKILL");
    errno::set_raw(0);
    assert_eq!(fails(i64::from(s::waitpid(pid, None, 0x1000))), (-1, EINVAL));
}
