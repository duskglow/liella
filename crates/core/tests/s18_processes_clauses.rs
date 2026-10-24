// Specified from spec §18: §18.1 in phase 5, §18.2 (fork) in phase 6, §18.3–§18.9 in phase 11.
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §18. Process-wide state (dispositions, the environment, the working
//! directory, waiting for any child) is changed only in a re-executed child (`alone`); the
//! spawned programs are the host's `/bin/sh` scripts, which report what they see on a pipe.
//! The C-only forms (the variadic `l` forms, `daemon` and cancellation, which need a process
//! Liella started) run in the conformance program `s18_exec`. The whole surface is also
//! compared with the reference by `process_products.rs`.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::unwrap_used, clippy::panic, clippy::as_conversions, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]

use std::ffi::CString;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};

use liella::process::ChildStatus;
use liella_abi::ctypes::c_char;
use liella_abi::layout::sigset_t;
use liella_boundary::errno;
use liella_boundary::posix::s12_stdio as io;
use liella_boundary::posix::s18_processes as x;
use liella_boundary::posix::s20_signals as sg;
use liella_boundary::selftest;
use liella_boundary::sys::CSys;
use liella_kernel::fd::Fd;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Rusage, SchedParam, SigInfo};
use liella_legacy::spawn_compat::{CFileActions, CSpawnAttr};

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

const EPERM: i32 = 1;
const ENOENT: i32 = 2;
const ESRCH: i32 = 3;
const EINTR: i32 = 4;
const EIO: i32 = 5;
const E2BIG: i32 = 7;
const ENOEXEC: i32 = 8;
const EBADF: i32 = 9;
const ECHILD: i32 = 10;
const EAGAIN: i32 = 11;
const EACCES: i32 = 13;
const EEXIST: i32 = 17;
const ENOTDIR: i32 = 20;
const EINVAL: i32 = 22;
const ENOTTY: i32 = 25;
const ELOOP: i32 = 40;

const SIGINT: i32 = 2;
const SIGQUIT: i32 = 3;
const SIGKILL: i32 = 9;
const SIGUSR1: i32 = 10;
const SIGUSR2: i32 = 12;
const SIGALRM: i32 = 14;
const SIGTERM: i32 = 15;
const SIGCHLD: i32 = 17;
const SIGCONT: i32 = 18;
const SIGSTOP: i32 = 19;

const O_RDONLY: i32 = 0;
const O_WRONLY: i32 = 1;
const O_CREAT: i32 = 0o100;
const O_EXCL: i32 = 0o200;
const O_NONBLOCK: i32 = 0o4000;
const O_DIRECTORY: i32 = 0o200000;
const O_CLOEXEC: u32 = 0o2000000;
const AT_EMPTY_PATH: i32 = 0x1000;

const WNOHANG: i32 = 1;
const WUNTRACED: i32 = 2;
const WEXITED: i32 = 4;
const WCONTINUED: i32 = 8;
const WNOWAIT: i32 = 0x0100_0000;
const P_ALL: i32 = 0;
const P_PID: i32 = 1;
const P_PGID: i32 = 2;
const P_PIDFD: i32 = 3;

const RESETIDS: i16 = 0x01;
const SETPGROUP: i16 = 0x02;
const SETSIGDEF: i16 = 0x04;
const SETSIGMASK: i16 = 0x08;
const SETSCHEDPARAM: i16 = 0x10;
const SETSCHEDULER: i16 = 0x20;
const USEVFORK: i16 = 0x40;
const SETSID: i16 = 0x80;
const SETCGROUP: i16 = 0x100;

fn cs(s: &str) -> CString {
    CString::new(s).unwrap()
}

/// A C argument (or environment) array that owns its strings.
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

/// The process's `environ` (the host's, or Liella's in the export pass).
fn environ() -> *const *mut c_char {
    core::ptr::with_exposed_provenance(liella_boundary::spawn::environ())
}

/// The test process's ID (the parent's, in a re-executed copy): scratch names are the same in
/// both, since a copy runs its test function again from the start.
fn base() -> String {
    std::env::var(BASE).unwrap_or_else(|_| std::process::id().to_string())
}
const BASE: &str = "LIELLA_C18_BASE";
/// Set in a re-executed copy: the case it runs.
const ALONE: &str = "LIELLA_C18_ALONE";

/// A fresh scratch directory; `tag` is unique per call site. The test process removes it when
/// the guard goes (a re-executed copy leaves it to the test process).
fn scratch(tag: &str) -> Scratch {
    let p = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("c18-{}-{tag}", base()));
    let _ = std::fs::remove_dir_all(&p);
    std::fs::create_dir_all(&p).unwrap();
    Scratch(p)
}

/// See [`scratch`].
struct Scratch(PathBuf);

impl std::ops::Deref for Scratch {
    type Target = PathBuf;
    fn deref(&self) -> &PathBuf {
        &self.0
    }
}

impl AsRef<std::path::Path> for Scratch {
    fn as_ref(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if std::env::var(ALONE).is_err() {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}

/// Write `text` to `path` with `mode` through a separate `/bin/sh`, so that this process never
/// holds a descriptor open for writing on a file a test then executes: a child another test
/// thread forks meanwhile would inherit it until its own exec, and executing the file during that
/// window fails with `ETXTBSY` (seen in the gate on 2026-10-04).
fn write_file(path: &std::path::Path, text: &str, mode: u32) {
    let st = std::process::Command::new("/bin/sh")
        .args(["-c", "printf '%s' \"$1\" > \"$2\" && chmod \"$3\" \"$2\"", "sh", text])
        .arg(path)
        .arg(format!("{mode:o}"))
        .status()
        .unwrap();
    assert!(st.success(), "writing {}", path.display());
}

/// `setenv(name, value, 1)` through the library (the spawns read the same `environ`).
fn set_env(name: &str, value: &str) {
    assert_eq!(liella_boundary::posix::s01_startup::setenv(cs(name).as_ptr(), cs(value).as_ptr(), 1), 0);
}

/// `unsetenv(name)` through the library.
fn unset_env(name: &str) {
    assert_eq!(liella_boundary::posix::s01_startup::unsetenv(cs(name).as_ptr()), 0);
}

fn path_c(p: &std::path::Path) -> CString {
    cs(p.to_str().unwrap())
}

fn kpath(c: &CString) -> liella_kernel::KStr<'_> {
    liella_kernel::KStr::from_nul_terminated(c.as_bytes_with_nul()).unwrap()
}

/// Run `body` alone in a re-executed copy of this test binary (test `test`, case `case`); the
/// copy exits 0 when `body` returns true, and `body` may also replace it with `exec`. In the
/// parent: the copy's standard output and exit status. In a copy made for another case of the
/// same test: `None`, and `body` does not run.
fn alone(test: &str, case: &str, body: impl FnOnce() -> bool) -> Option<(String, Option<i32>)> {
    const VAR: &str = ALONE;
    match std::env::var(VAR) {
        Ok(v) if v == case => {
            let ok = body();
            CSys.exit_group(if ok { 0 } else { 1 })
        }
        Ok(_) => return None,
        Err(_) => {}
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap()).args([test, "--exact", "--test-threads=1", "--nocapture"]).env(VAR, case).env(BASE, base()).output().unwrap();
    let mut text = String::from_utf8_lossy(&o.stdout).into_owned();
    if o.status.code() != Some(0) {
        text.push_str(&String::from_utf8_lossy(&o.stderr));
    }
    Some((text, o.status.code()))
}

/// [`alone`] whose case must succeed (exit 0): its output (empty in another case's copy).
fn alone_ok(test: &str, case: &str, body: impl FnOnce() -> bool) -> String {
    match alone(test, case, body) {
        Some((out, code)) => {
            assert_eq!(code, Some(0), "case {case} failed:\n{out}");
            out
        }
        None => String::new(),
    }
}

/// The reports in `out`: what follows `tag` on each line that has it (the harness may print
/// `test name ... ` before the first one on the same line).
fn tagged(out: &str, tag: &str) -> Vec<String> {
    out.lines().filter_map(|l| l.split_once(tag)).map(|(_, r)| r.to_string()).collect()
}

/// Write `text` to standard output now (before an exec replaces the process).
fn say(text: &str) {
    let _ = CSys.write(Fd::STDOUT, text.as_bytes());
}

/// A new, initialised file-actions object.
fn actions() -> Box<CFileActions> {
    let mut fa = Box::new(CFileActions::new());
    assert_eq!(x::posix_spawn_file_actions_init(Some(&mut fa)), 0);
    fa
}

/// A new, initialised attributes object with `flags`.
fn attributes(flags: i16) -> Box<CSpawnAttr> {
    let mut at = Box::new(CSpawnAttr::new());
    assert_eq!(x::posix_spawnattr_init(Some(&mut at)), 0);
    assert_eq!(x::posix_spawnattr_setflags(Some(&mut at), flags), 0);
    at
}

/// `posix_spawn("/bin/sh", ["sh", "-c", script])` with `fa` and `attr`: the result and the
/// child's PID.
fn spawn_sh(script: &str, fa: Option<&CFileActions>, attr: Option<&CSpawnAttr>) -> (i32, i32) {
    let argv = Args::new(&["sh", "-c", script]);
    let path = cs("/bin/sh");
    let mut pid = -1;
    let r = x::posix_spawn(Some(&mut pid), path.as_ptr(), fa, attr, argv.p(), environ());
    (r, pid)
}

/// Read until end of file, then close.
fn read_all(fd: Fd) -> String {
    String::from_utf8_lossy(&read_bytes(fd)).into_owned()
}

/// Read until end of file, then close: the bytes.
fn read_bytes(fd: Fd) -> Vec<u8> {
    let mut out = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        match CSys.read(fd, &mut buf) {
            Ok(0) => break,
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(liella::error::Errno::EINTR) => {}
            Err(_) => break,
        }
    }
    let _ = CSys.close(fd);
    out
}

/// Wait for `pid`: its status word.
fn wait_for(pid: i32) -> i32 {
    let mut st = -1;
    assert_eq!(x::waitpid(pid, Some(&mut st), 0), pid);
    st
}

fn kill_and_wait(pid: i32) {
    sg::kill(pid, SIGKILL);
    let _ = x::waitpid(pid, None, 0);
}

/// Run `script` with `sh -c` through `posix_spawn`, its standard output on a pipe; `setup`
/// adds file actions after the redirection. The spawn's result, the output and the status.
fn run_sh(script: &str, setup: impl FnOnce(&mut CFileActions), attr: Option<&CSpawnAttr>) -> (i32, String, i32) {
    run_prog("/bin/sh", &["sh", "-c", script], setup, attr)
}

/// [`run_sh`] for the program at `path` with `argv`.
fn run_prog(path: &str, argv: &[&str], setup: impl FnOnce(&mut CFileActions), attr: Option<&CSpawnAttr>) -> (i32, String, i32) {
    let [rd, wr] = CSys.pipe2(O_CLOEXEC).unwrap();
    let mut fa = actions();
    assert_eq!(x::posix_spawn_file_actions_adddup2(Some(&mut fa), wr.raw(), 1), 0);
    setup(&mut fa);
    let a = Args::new(argv);
    let mut pid = -1;
    let r = x::posix_spawn(Some(&mut pid), cs(path).as_ptr(), Some(&fa), attr, a.p(), environ());
    let _ = CSys.close(wr);
    let out = read_all(rd);
    x::posix_spawn_file_actions_destroy(Some(&mut fa));
    let st = if r == 0 { wait_for(pid) } else { -1 };
    (r, out, st)
}

/// The value of a `/proc/<pid>/status` line (`SigBlk:` ...) in a report.
fn status_field(out: &str, key: &str) -> u64 {
    let (_, v) = out.lines().find_map(|l| l.split_once(key)).unwrap_or_else(|| panic!("no {key} in {out}"));
    u64::from_str_radix(v.trim(), 16).unwrap()
}

fn bit(sig: i32) -> u64 {
    1u64 << (sig - 1)
}

/// A shell command that reports the shell's signal state (in a pipeline: dash resets its mask
/// before it executes a last simple command itself).
const REPORT_SIGNALS: &str = "grep -E '^(SigBlk|SigIgn|SigCgt|SigPnd|ShdPnd):' /proc/$$/status | cat";

/// A program that reports its own signal state: `grep` itself, no shell in between.
const GREP: &str = "/bin/grep";
const GREP_SIGNALS: [&str; 4] = ["grep", "-E", "^(SigBlk|SigIgn|SigCgt|SigPnd|ShdPnd):", "/proc/self/status"];

extern "C" fn count_signal(_sig: i32) {
    HITS.fetch_add(1, Ordering::SeqCst);
}
static HITS: AtomicU32 = AtomicU32::new(0);

fn handler(f: extern "C" fn(i32)) -> usize {
    f as usize
}

/// The lowest descriptor not open.
fn lowest_free() -> i32 {
    let fd = CSys.openat(Fd::CWD, liella_kernel::KStr::from_nul_terminated(b"/dev/null\0").unwrap(), O_CLOEXEC, 0).unwrap();
    let _ = CSys.close(fd);
    fd.raw()
}

fn fd_flags(fd: i32) -> i64 {
    CSys.fcntl_int(Fd::new(fd), liella_kernel::ops::FcntlInt::GetFd, 0).unwrap()
}

// ------------------------------------------------------------------------------ §18.1, §18.2

#[test]
fn s18_1_c_0() {
    errno::set_raw(0);
    let (pid, ppid, tid) = (x::getpid(), x::getppid(), x::gettid());
    assert_eq!(errno::get(), 0, "never fail");
    assert!(pid > 0 && ppid >= 0 && tid > 0);
    let (opid, otid) = std::thread::spawn(|| (x::getpid(), x::gettid())).join().unwrap();
    assert_eq!(opid, pid, "the thread-group ID");
    assert_ne!(otid, tid, "the calling thread's ID");
    assert_ne!(otid, opid, "a second thread's ID is not the group's");
}

/// §18.2 Contract: **`fork()`** creates a child process that is a copy of the caller. Copied: memory (copy-on-write), descriptors (sharing descriptions), the signal mask and dispositions, the cwd, the umask, limits, and
#[test]
fn s18_2_c_1() {
    conformance(&["s18_fork", "s18_fork_aliases"]);
}
/// §18.2 Contract: Only the calling thread exists in the child.
#[test]
fn s18_2_c_1_1() {
    conformance(&["s18_fork", "s18_fork_aliases"]);
}
/// §18.2 Contract: `pthread_atfork` handlers run: *prepare* in reverse registration order before the fork, and *parent* and *child* in registration order after it.
#[test]
fn s18_2_c_1_2() {
    conformance(&["s18_fork", "s18_fork_aliases"]);
}
/// §18.2 Contract: glibc makes its own internal state (malloc, stdio lists, dynamic-linker locks) consistent in the child.
#[test]
fn s18_2_c_1_3() {
    conformance(&["s18_fork", "s18_fork_aliases"]);
    // popen's and system's locks too (D-18.6): another thread holds them when fork starts and
    // lets go only once fork waits for them; the child's popen and system then work.
    fn child(_: &CSys) -> i32 {
        let fp = x::popen(c"echo ok".as_ptr(), c"r".as_ptr());
        if fp.is_null() || line_of(fp) != b"ok
" || x::pclose(fp) != 0 {
            return 1;
        }
        if x::system(c"exit 4".as_ptr()) != 4 << 8 { 2 } else { 0 }
    }
    let (end, waited) = selftest::fork_while_spawn_locks_held(child).unwrap();
    assert!(waited, "fork waits for the popen lock another thread holds");
    assert_eq!(end, selftest::ChildEnd::Exited(0), "popen and system work in the child (SIGALRM: deadlocked)");
}
/// §18.2 Contract: **`_Fork()`** (2.34, POSIX 2024) forks without running `atfork` handlers and without internal lock handling. It is async-signal-safe.
#[test]
fn s18_2_c_2() {
    conformance(&["s18_fork"]);
}
/// §18.2 Contract: **`vfork()`** creates a child that shares the parent's memory. The parent is suspended until the child calls `execve` or `_exit`. The child may do nothing else. ⚑ HIST: `vfork` is an optimization that
#[test]
fn s18_2_c_3() {
    // vfork shares the parent's memory and suspends it (D-18.1): s18_vfork.
    conformance(&["s18_fork", "s18_vfork"]);
}
/// §18.2 Contract: **`clone(fn, stack, flags, arg, [ptid, tls, ctid])`** is the Linux primitive. It creates a task that shares selected resources (`CLONE_VM`, `CLONE_FS`, `CLONE_FILES`, `CLONE_SIGHAND`, `CLONE_THREAD`,
#[test]
fn s18_2_c_4() {
    conformance(&["s18_fork"]);
}
/// §18.2 Failure: -1 with `EAGAIN` (`RLIMIT_NPROC`, or the kernel limit) or `ENOMEM`.
#[test]
fn s18_2_f_0() {
    // Memory limits: s22_more provokes them for threads; fork maps them likewise.
    conformance(&["s18_fork", "s18_vfork", "s22_more"]);
}
/// §18.2 Guarantees: The child may call only async-signal-safe functions until it calls `exec`, per POSIX. glibc goes further: `malloc` works in the child.
#[test]
fn s18_2_g_0() {
    conformance(&["s18_fork"]);
}

// ------------------------------------------------------------------------------ §18.3 exec

/// The exec fixture: `noexec/prog` (not executable), `ok/prog` and `ok2/prog` (scripts that
/// report their `$0`, arguments and `X`), `plain/prog` (executable without `#!`), `loop/prog`
/// (a symbolic-link loop), `dir/prog` (a directory), `notdir` (a file).
fn exec_fixture(tag: &str) -> Scratch {
    let d = scratch(tag);
    for sub in ["noexec", "ok", "ok2", "plain", "loop", "dir/prog"] {
        std::fs::create_dir_all(d.join(sub)).unwrap();
    }
    write_file(&d.join("noexec/prog"), "#!/bin/sh\necho R:noexec\n", 0o644);
    write_file(&d.join("ok/prog"), "#!/bin/sh\necho \"R:ok $0|$*|${X-unset}\"\n", 0o755);
    write_file(&d.join("ok2/prog"), "#!/bin/sh\necho \"R:ok2 $0|$*\"\n", 0o755);
    write_file(&d.join("plain/prog"), "echo \"R:plain $0|$#|$*|${X-unset}\"\n", 0o755);
    write_file(&d.join("notdir"), "x\n", 0o644);
    std::os::unix::fs::symlink("prog", d.join("loop/prog")).unwrap();
    d
}

fn path_of(d: &std::path::Path, elems: &[&str]) -> String {
    elems.iter().map(|e| if e.starts_with('/') { (*e).to_string() } else { d.join(e).to_str().unwrap().to_string() }).collect::<Vec<_>>().join(":")
}

/// §18.3 Contract: what exec keeps (the PID, descriptors without close-on-exec, the mask, a
/// pending blocked signal, ignored dispositions, the cwd) and what it resets (caught
/// dispositions become default; close-on-exec descriptors close).
#[test]
fn s18_3_c_0() {
    let d = scratch("keep");
    let dc = d.clone();
    let out = alone_ok("s18_3_c_0", "keep", move || {
        sg::signal(SIGUSR1, 1);
        sg::signal(SIGUSR2, handler(count_signal));
        let mut set = sigset_t { __val: [0; 16] };
        sg::sigaddset(Some(&mut set), SIGTERM);
        sg::sigprocmask(0, Some(&set), None);
        sg::raise(SIGTERM);
        let f = CSys.openat(Fd::CWD, liella_kernel::KStr::from_nul_terminated(b"/dev/null\0").unwrap(), 0, 0).unwrap();
        CSys.dup2(f, Fd::new(7)).unwrap();
        CSys.dup3(f, Fd::new(8), O_CLOEXEC).unwrap();
        std::env::set_current_dir(&dc).unwrap();
        say(&format!("R:pid {}\n", x::getpid()));
        let argv = Args::new(&["sh", "-c", &format!("echo R:shpid $$; {REPORT_SIGNALS} | sed 's/^/R:/'; for f in 7 8; do [ -e /proc/$$/fd/$f ] && echo R:open $f; done; echo R:cwd $(pwd -P)")]);
        x::execv(cs("/bin/sh").as_ptr(), argv.p());
        false
    });
    if out.is_empty() {
        return;
    }
    let r = tagged(&out, "R:");
    let pid = r.iter().find_map(|l| l.strip_prefix("pid ")).unwrap();
    assert_eq!(r.iter().find_map(|l| l.strip_prefix("shpid ")), Some(pid), "the PID is kept: {out}");
    let rep = r.join("\n");
    assert_ne!(status_field(&rep, "SigBlk:") & bit(SIGTERM), 0, "the mask is kept");
    assert_ne!((status_field(&rep, "SigPnd:") | status_field(&rep, "ShdPnd:")) & bit(SIGTERM), 0, "the pending blocked signal stays pending");
    assert_ne!(status_field(&rep, "SigIgn:") & bit(SIGUSR1), 0, "an ignored signal stays ignored");
    assert_eq!(status_field(&rep, "SigCgt:") & bit(SIGUSR2), 0, "a caught signal is reset to its default action");
    assert!(r.iter().any(|l| l == "open 7") && !r.iter().any(|l| l == "open 8"), "close-on-exec descriptors close, others stay: {out}");
    let cwd = std::fs::canonicalize(&d).unwrap();
    assert!(r.iter().any(|l| *l == format!("cwd {}", cwd.display())), "the cwd is kept: {out}");
}

/// §18.3 Contract: the `p` forms search `PATH` (default `/bin:/usr/bin`), continue past
/// `EACCES` and report it only when nothing later runs, run an `ENOEXEC` file with the shell.
#[test]
fn s18_3_c_1() {
    let d = exec_fixture("exec-c1");
    let runs: [(&str, Option<String>, &str, &[&str]); 9] = [
        ("eacces-then-ok", Some(path_of(&d, &["noexec", "ok"])), "prog", &["prog", "a1"]),
        ("eacces-only", Some(path_of(&d, &["noexec", "nonexistent"])), "prog", &["prog"]),
        ("missing-then-eacces", Some(path_of(&d, &["nonexistent", "notdir", "noexec"])), "prog", &["prog"]),
        ("notdir-only", Some(path_of(&d, &["notdir"])), "prog", &["prog"]),
        ("enoexec-shell", Some(path_of(&d, &["plain", "ok"])), "prog", &["alias", "a", "b c"]),
        ("enoexec-empty-argv", Some(path_of(&d, &["plain"])), "prog", &[]),
        ("eloop-stops", Some(path_of(&d, &["loop", "ok"])), "prog", &["prog"]),
        ("default-path", None, "sh", &["sh", "-c", "echo R:default"]),
        ("empty-name", Some(path_of(&d, &["ok"])), "", &["x"]),
    ];
    let mut outs = Vec::new();
    for (name, path, file, argv) in runs {
        let r = alone("s18_3_c_1", name, move || {
            match &path {
                Some(v) => set_env("PATH", v),
                None => unset_env("PATH"),
            }
            unset_env("X");
            let a = Args::new(argv);
            errno::set_raw(0);
            x::execvp(cs(file).as_ptr(), a.p());
            say(&format!("R:errno {}\n", errno::get()));
            true
        });
        if let Some((out, _)) = r {
            outs.push((name, tagged(&out, "R:")));
        }
    }
    if outs.is_empty() {
        return;
    }
    let get = |n: &str| outs.iter().find(|o| o.0 == n).unwrap().1.clone();
    assert_eq!(get("eacces-then-ok"), vec![format!("ok {}|a1|unset", d.join("ok/prog").display())], "EACCES does not stop the search");
    assert_eq!(get("eacces-only"), vec![format!("errno {EACCES}")], "EACCES is reported when nothing later runs");
    assert_eq!(get("missing-then-eacces"), vec![format!("errno {EACCES}")], "EACCES wins over ENOENT and ENOTDIR");
    assert_eq!(get("notdir-only"), vec![format!("errno {ENOTDIR}")], "otherwise the last error");
    assert_eq!(get("enoexec-shell"), vec![format!("plain {}|2|a b c|unset", d.join("plain/prog").display())], "ENOEXEC: /bin/sh with the found path as $0 and argv[1..]");
    assert_eq!(get("enoexec-empty-argv"), vec![format!("plain {}|0||unset", d.join("plain/prog").display())]);
    assert_eq!(get("eloop-stops"), vec![format!("errno {ELOOP}")], "ELOOP ends the search");
    assert_eq!(get("default-path"), vec!["default".to_string()], "the default list finds sh");
    assert_eq!(get("empty-name"), vec![format!("errno {ENOENT}")]);
}

/// §18.3 Contract: the `l` forms (variadic): C programs only (`s18_exec`).
#[test]
fn s18_3_c_2() {
    conformance(&["s18_exec"]);
}

/// §18.3 Contract: the `e` forms take their environment; the others use `environ`; `execvpe`
/// searches the caller's `PATH`, not `envp`'s.
#[test]
fn s18_3_c_3() {
    let d = exec_fixture("exec-c3");
    let ok = d.join("ok").to_str().unwrap().to_string();
    let ok2 = d.join("ok2").to_str().unwrap().to_string();
    let (o1, o2) = (ok.clone(), ok.clone());
    let a = alone_ok("s18_3_c_3", "execve", move || {
        let env = Args::new(&["X=from-envp"]);
        let argv = Args::new(&["prog"]);
        x::execve(cs(&format!("{o1}/prog")).as_ptr(), argv.p(), env.p());
        false
    });
    let b = alone_ok("s18_3_c_3", "execv", move || {
        set_env("X", "from-environ");
        let argv = Args::new(&["prog"]);
        x::execv(cs(&format!("{o2}/prog")).as_ptr(), argv.p());
        false
    });
    let c = alone_ok("s18_3_c_3", "execvpe", move || {
        set_env("PATH", &ok);
        let env = Args::new(&[&format!("PATH={ok2}"), "X=e"]);
        let argv = Args::new(&["prog"]);
        x::execvpe(cs("prog").as_ptr(), argv.p(), env.p());
        false
    });
    if a.is_empty() {
        return;
    }
    assert!(tagged(&a, "R:")[0].ends_with("|from-envp"), "{a}");
    assert!(tagged(&b, "R:")[0].ends_with("|from-environ"), "{b}");
    let c = &tagged(&c, "R:")[0];
    assert!(c.starts_with("ok ") && c.ends_with("|e"), "the caller's PATH, envp's environment: {c}");
}

/// §18.3 Contract: `fexecve` runs an open file (`execveat(fd, "", AT_EMPTY_PATH)`).
#[test]
fn s18_3_c_4() {
    errno::set_raw(0);
    let argv = Args::new(&["x"]);
    assert_eq!((x::fexecve(-1, argv.p(), environ()), errno::get()), (-1, EINVAL), "a negative descriptor (fexecve(3))");
    assert_eq!((x::fexecve(1000, argv.p(), environ()), errno::get()), (-1, EBADF));
    assert_eq!((x::fexecve(0, core::ptr::null(), environ()), errno::get()), (-1, EINVAL), "a NULL argv");
    let d = exec_fixture("exec-c4");
    let plain = path_c(&d.join("plain/prog"));
    let fd = CSys.openat(Fd::CWD, kpath(&plain), O_CLOEXEC, 0).unwrap();
    assert_eq!((x::fexecve(fd.raw(), argv.p(), environ()), errno::get()), (-1, ENOEXEC), "no shell fallback");
    let _ = CSys.close(fd);
    let out = alone_ok("s18_3_c_4", "run", || {
        let fd = CSys.openat(Fd::CWD, liella_kernel::KStr::from_nul_terminated(b"/bin/sh\0").unwrap(), 0, 0).unwrap();
        let env = Args::new(&["X=fx"]);
        let argv = Args::new(&["sh", "-c", "echo R:fexecve $X"]);
        x::fexecve(fd.raw(), argv.p(), env.p());
        false
    });
    if !out.is_empty() {
        assert_eq!(tagged(&out, "R:"), vec!["fexecve fx"]);
    }
}

/// §18.3 Contract: `execveat` is thin: a path relative to the directory descriptor,
/// `AT_EMPTY_PATH`, the kernel's errors.
#[test]
fn s18_3_c_5() {
    let argv = Args::new(&["x"]);
    errno::set_raw(0);
    assert_eq!((x::execveat(-100, cs("/bin/sh").as_ptr(), argv.p(), environ(), 0x1), errno::get()), (-1, EINVAL), "an unknown flag");
    assert_eq!((x::execveat(-100, cs("").as_ptr(), argv.p(), environ(), 0), errno::get()), (-1, ENOENT), "an empty path without AT_EMPTY_PATH");
    assert_eq!((x::execveat(-5, cs("sh").as_ptr(), argv.p(), environ(), 0), errno::get()), (-1, EBADF), "a bad directory with a relative path");
    let out = alone_ok("s18_3_c_5", "dirfd", || {
        let dir = CSys.openat(Fd::CWD, liella_kernel::KStr::from_nul_terminated(b"/bin\0").unwrap(), O_DIRECTORY as u32, 0).unwrap();
        let argv = Args::new(&["sh", "-c", "echo R:at"]);
        x::execveat(dir.raw(), cs("sh").as_ptr(), argv.p(), environ(), 0);
        false
    });
    if !out.is_empty() {
        assert_eq!(tagged(&out, "R:"), vec!["at"]);
    }
    let out = alone_ok("s18_3_c_5", "empty", || {
        let sh = CSys.openat(Fd::CWD, liella_kernel::KStr::from_nul_terminated(b"/bin/sh\0").unwrap(), 0, 0).unwrap();
        let argv = Args::new(&["sh", "-c", "echo R:empty"]);
        x::execveat(sh.raw(), cs("").as_ptr(), argv.p(), environ(), AT_EMPTY_PATH);
        false
    });
    if !out.is_empty() {
        assert_eq!(tagged(&out, "R:"), vec!["empty"]);
    }
}

/// §18.3 Guarantees: a successful exec does not return (the replaced process's status is the
/// program's).
#[test]
fn s18_3_g_0() {
    if let Some((_, code)) = alone("s18_3_g_0", "run", || {
        let argv = Args::new(&["sh", "-c", "exit 7"]);
        x::execv(cs("/bin/sh").as_ptr(), argv.p());
        false
    }) {
        assert_eq!(code, Some(7));
    }
}

/// §18.3 Failure: the kernel's errors come back unchanged; `E2BIG` at `MAX_ARG_STRLEN` (one
/// string of 32 pages with its NUL); a failed exec leaves the process as it was.
#[test]
fn s18_3_f_0() {
    let d = exec_fixture("exec-f0");
    let argv = Args::new(&["x"]);
    let try_exec = |p: &std::path::Path| {
        errno::set_raw(0);
        let r = x::execv(path_c(p).as_ptr(), argv.p());
        (r, errno::get())
    };
    assert_eq!(try_exec(&d.join("missing")), (-1, ENOENT));
    assert_eq!(try_exec(&d.join("noexec/prog")), (-1, EACCES));
    assert_eq!(try_exec(&d.join("dir/prog")), (-1, EACCES), "a directory");
    assert_eq!(try_exec(&d.join("plain/prog")), (-1, ENOEXEC), "no shell fallback without p");
    assert_eq!(try_exec(&d.join("loop/prog")), (-1, ELOOP));
    assert_eq!(try_exec(&d.join("notdir/prog")), (-1, ENOTDIR));
    let big = "a".repeat(131_072);
    let a = Args::new(&["sh", "-c", "exit 0", &big]);
    errno::set_raw(0);
    assert_eq!((x::execv(cs("/bin/sh").as_ptr(), a.p()), errno::get()), (-1, E2BIG), "MAX_ARG_STRLEN = 32 pages per string, NUL included");
    let fits = "a".repeat(131_071);
    let fits_args = Args::new(&["sh", "-c", "exit 0", &fits]);
    let mut pid = -1;
    assert_eq!(x::posix_spawn(Some(&mut pid), cs("/bin/sh").as_ptr(), None, None, fits_args.p(), environ()), 0, "one byte less passes");
    assert_eq!(wait_for(pid), 0);
}

// ------------------------------------------------------------------------------ §18.4 spawn

/// §18.4 Contract: a child running the program, reported by PID (`posix_spawn`) or pidfd
/// (`pidfd_spawn`).
#[test]
fn s18_4_c_0() {
    let (r, pid) = spawn_sh("exit 3", None, None);
    assert_eq!(r, 0);
    assert_eq!(ChildStatus::from_word(wait_for(pid)), Some(ChildStatus::Exited(3)));
    let argv = Args::new(&["sh", "-c", "exit 4"]);
    let mut pfd = -1;
    assert_eq!(x::pidfd_spawn(Some(&mut pfd), cs("/bin/sh").as_ptr(), None, None, argv.p(), environ()), 0);
    assert!(pfd > 2);
    assert_eq!(fd_flags(pfd), 1, "close-on-exec");
    let pid = x::pidfd_getpid(pfd);
    let mut si = SigInfo::default();
    assert_eq!(x::waitid(P_PIDFD, pfd as u32, Some(&mut si), WEXITED), 0);
    assert_eq!((si.code, si.status(), si.pid()), (1, 4, pid), "CLD_EXITED 4 from the pidfd's process");
    let _ = CSys.close(Fd::new(pfd));
    let argv = Args::new(&["sh", "-c", "exit 5"]);
    let mut pid = -1;
    assert_eq!(x::posix_spawnp(Some(&mut pid), cs("sh").as_ptr(), None, None, argv.p(), environ()), 0, "posix_spawnp searches PATH");
    assert_eq!(wait_for(pid), 5 << 8);
}

/// §18.4 Contract: file actions apply in order: open, close, dup2 (on itself it clears
/// close-on-exec, in the child only), chdir, fchdir, closefrom; relative paths resolve in the
/// directory an earlier chdir set.
#[test]
fn s18_4_c_1() {
    let d = scratch("actions");
    std::fs::create_dir_all(d.join("sub")).unwrap();
    write_file(&d.join("sub/insub"), "insub\n", 0o644);
    let sub = d.join("sub");
    let canon = std::fs::canonicalize(&sub).unwrap();
    let fds = "for f in 5 6 7 8 9 12; do [ -e /proc/$$/fd/$f ] && echo R:open $f; done; echo R:cwd $(pwd -P)";
    let (r, out, st) = run_sh(
        fds,
        |fa| {
            x::posix_spawn_file_actions_addchdir_np(Some(fa), path_c(&sub).as_ptr());
            x::posix_spawn_file_actions_addopen(Some(fa), 5, cs("insub").as_ptr(), O_RDONLY, 0);
            x::posix_spawn_file_actions_adddup2(Some(fa), 5, 6);
            x::posix_spawn_file_actions_addclose(Some(fa), 5);
        },
        None,
    );
    assert_eq!((r, st), (0, 0));
    assert_eq!(tagged(&out, "R:"), vec!["open 6".to_string(), format!("cwd {}", canon.display())], "chdir, open, dup2, close in order");
    let (r, _, _) = run_sh(
        fds,
        |fa| {
            x::posix_spawn_file_actions_addopen(Some(fa), 5, cs("insub").as_ptr(), O_RDONLY, 0);
            x::posix_spawn_file_actions_addchdir_np(Some(fa), path_c(&sub).as_ptr());
        },
        None,
    );
    assert_eq!(r, ENOENT, "the open before the chdir resolves in the caller's directory");
    let subc = path_c(&sub);
    let dirfd = CSys.openat(Fd::CWD, kpath(&subc), (O_DIRECTORY as u32) | O_CLOEXEC, 0).unwrap();
    let (r, out, _) = run_sh(
        fds,
        |fa| {
            x::posix_spawn_file_actions_adddup2(Some(fa), dirfd.raw(), 9);
            x::posix_spawn_file_actions_adddup2(Some(fa), 9, 9);
            x::posix_spawn_file_actions_adddup2(Some(fa), 9, 12);
            x::posix_spawn_file_actions_addfchdir_np(Some(fa), 9);
            x::posix_spawn_file_actions_addclosefrom_np(Some(fa), 10);
        },
        None,
    );
    assert_eq!(r, 0);
    assert_eq!(tagged(&out, "R:"), vec!["open 9".to_string(), format!("cwd {}", canon.display())], "9 kept (close-on-exec cleared), 12 closed by closefrom, fchdir");
    assert_eq!(fd_flags(dirfd.raw()), 1, "the caller's descriptor keeps close-on-exec");
    let _ = CSys.close(dirfd);
    let mut fa = actions();
    x::posix_spawn_file_actions_addclose(Some(&mut fa), 0);
    for _ in 0..2 {
        let (r, pid) = spawn_sh("[ -e /proc/$$/fd/0 ] && exit 1; exit 0", Some(&fa), None);
        assert_eq!((r, wait_for(pid)), (0, 0), "one list serves several spawns");
    }
    x::posix_spawn_file_actions_destroy(Some(&mut fa));
}

/// §18.4 Contract: the attribute flags: `setflags` takes the nine defined bits only.
#[test]
fn s18_4_c_2() {
    let mut at = attributes(0);
    let mut fl = -1i16;
    assert_eq!(x::posix_spawnattr_getflags(Some(&at), Some(&mut fl)), 0);
    assert_eq!(fl, 0, "no flags in a new object");
    assert_eq!(x::posix_spawnattr_setflags(Some(&mut at), 0x1ff), 0);
    assert_eq!(x::posix_spawnattr_setflags(Some(&mut at), 0x200), EINVAL);
    assert_eq!(x::posix_spawnattr_setflags(Some(&mut at), -1), EINVAL);
    x::posix_spawnattr_getflags(Some(&at), Some(&mut fl));
    assert_eq!(fl, 0x1ff, "a refused value changes nothing");
    assert_eq!(x::posix_spawnattr_destroy(Some(&mut at)), 0);
}

/// §18.4 Contract: `RESETIDS` takes the real IDs. Unprivileged the IDs agree, so the child's
/// stay the caller's; the privileged case (real 65534, effective 0) is in process_products.rs.
#[test]
fn s18_4_c_2_1() {
    let at = attributes(RESETIDS);
    let (r, out, st) = run_sh("echo R:$(id -u) $(id -ru) $(id -g) $(id -rg)", |_| {}, Some(&at));
    assert_eq!((r, st), (0, 0));
    assert_eq!(tagged(&out, "R:"), vec![format!("{} {} {} {}", CSys.geteuid(), CSys.getuid(), CSys.getegid(), CSys.getgid())]);
}

/// §18.4 Contract: `SETPGROUP` (0: the child's own group; an existing group; a missing one is
/// `EPERM`) and `SETSID` (a new session); both together is `EPERM`.
#[test]
fn s18_4_c_2_2() {
    let mut at = attributes(SETPGROUP);
    x::posix_spawnattr_setpgroup(Some(&mut at), 0);
    // The caller resumes after the child executed its program, so the group is already set.
    let (r, leader) = spawn_sh("exec sleep 5", None, Some(&at));
    assert_eq!(r, 0);
    assert_eq!(x::getpgid(leader), leader, "a group of its own");
    x::posix_spawnattr_setpgroup(Some(&mut at), leader);
    let (r, member) = spawn_sh("exec sleep 5", None, Some(&at));
    assert_eq!(r, 0);
    assert_eq!(x::getpgid(member), leader, "joins the existing group");
    kill_and_wait(member);
    kill_and_wait(leader);
    assert_eq!(spawn_sh("exit 0", None, Some(&at)).0, EPERM, "a group that no longer exists");
    let at = attributes(SETSID);
    let (r, pid) = spawn_sh("exec sleep 5", None, Some(&at));
    assert_eq!(r, 0);
    assert_eq!((x::getsid(pid), x::getpgid(pid)), (pid, pid), "leader of a new session and group");
    kill_and_wait(pid);
    let mut at = attributes(SETSID | SETPGROUP);
    x::posix_spawnattr_setpgroup(Some(&mut at), 0);
    assert_eq!(spawn_sh("exit 0", None, Some(&at)).0, EPERM, "a session leader cannot change its group");
}

/// §18.4 Contract: `SETSIGMASK` replaces the mask (the caller's otherwise); `SETSIGDEF`
/// resets the listed signals; ignored ones stay ignored, caught ones become default.
#[test]
fn s18_4_c_2_3() {
    let out = alone_ok("s18_4_c_2_3", "run", || {
        sg::signal(SIGUSR1, 1);
        sg::signal(SIGTERM, 1);
        sg::signal(SIGUSR2, handler(count_signal));
        let mut hup = sigset_t { __val: [0; 16] };
        sg::sigaddset(Some(&mut hup), 1);
        sg::sigprocmask(0, Some(&hup), None);
        let report = |tag: &str, at: Option<&CSpawnAttr>| {
            let (r, out, _) = run_prog(GREP, &GREP_SIGNALS, |_| {}, at);
            let o = out.lines().map(|l| format!("R:{tag} {l}\n")).collect::<String>();
            say(&format!("R:{tag} r={r}\n{o}"));
        };
        report("none", None);
        let mut at = attributes(SETSIGMASK | SETSIGDEF);
        let mut m = sigset_t { __val: [0; 16] };
        sg::sigaddset(Some(&mut m), SIGUSR2);
        x::posix_spawnattr_setsigmask(Some(&mut at), Some(&m));
        let mut def = sigset_t { __val: [0; 16] };
        sg::sigaddset(Some(&mut def), SIGUSR1);
        x::posix_spawnattr_setsigdefault(Some(&mut at), Some(&def));
        report("set", Some(&at));
        true
    });
    if out.is_empty() {
        return;
    }
    let pick = |tag: &str| tagged(&out, &format!("R:{tag} ")).join("\n");
    let none = pick("none");
    assert_eq!(status_field(&none, "SigBlk:"), bit(1), "the caller's mask");
    // The harness itself ignores SIGPIPE: only the signals set here are looked at.
    let ours = bit(SIGUSR1) | bit(SIGTERM);
    assert_eq!(status_field(&none, "SigIgn:") & ours, ours, "ignored signals stay ignored");
    assert_eq!(status_field(&none, "SigCgt:") & bit(SIGUSR2), 0, "the caught signal is at its default");
    let set = pick("set");
    assert_eq!(status_field(&set, "SigBlk:"), bit(SIGUSR2), "SETSIGMASK");
    assert_eq!(status_field(&set, "SigIgn:") & ours, bit(SIGTERM), "SETSIGDEF reset SIGUSR1");
}

/// §18.4 Contract: `SETSCHEDULER` sets policy and priority, `SETSCHEDPARAM` alone the
/// priority under the caller's policy; the kernel's refusals are the spawn's errors.
#[test]
fn s18_4_c_2_4() {
    let mut at = attributes(SETSCHEDULER);
    assert_eq!(x::posix_spawnattr_setschedpolicy(Some(&mut at), 3), 0, "SCHED_BATCH");
    assert_eq!(x::posix_spawnattr_setschedparam(Some(&mut at), Some(&SchedParam { priority: 0 })), 0);
    let (r, pid) = spawn_sh("exec sleep 5", None, Some(&at));
    assert_eq!(r, 0);
    assert_eq!(CSys.sched_getscheduler(pid), Ok(3), "the child runs SCHED_BATCH");
    kill_and_wait(pid);
    x::posix_spawnattr_setschedpolicy(Some(&mut at), 0);
    x::posix_spawnattr_setschedparam(Some(&mut at), Some(&SchedParam { priority: 5 }));
    assert_eq!(spawn_sh("exit 0", None, Some(&at)).0, EINVAL, "SCHED_OTHER takes priority 0 only");
    let mut at = attributes(SETSCHEDPARAM);
    x::posix_spawnattr_setschedparam(Some(&mut at), Some(&SchedParam { priority: 0 }));
    let (r, pid) = spawn_sh("exit 0", None, Some(&at));
    assert_eq!((r, wait_for(pid)), (0, 0));
    if CSys.geteuid() != 0 {
        let mut at = attributes(SETSCHEDULER);
        x::posix_spawnattr_setschedpolicy(Some(&mut at), 1);
        x::posix_spawnattr_setschedparam(Some(&mut at), Some(&SchedParam { priority: 1 }));
        assert_eq!(spawn_sh("exit 0", None, Some(&at)).0, EPERM, "SCHED_FIFO unprivileged");
    }
}

/// §18.4 Contract: `USEVFORK` changes nothing.
#[test]
fn s18_4_c_2_5() {
    let at = attributes(USEVFORK);
    let (r, out, st) = run_sh("echo R:ran", |_| {}, Some(&at));
    assert_eq!((r, out.trim(), st), (0, "R:ran", 0));
}

/// §18.4 Contract: `SETCGROUP` places the child in the cgroup of a directory descriptor. Only
/// the refusals are tested (no writable cgroup is created on the test machine): -1 is
/// `EINVAL`, a directory that is not a cgroup `EBADF`, both when the child is created.
#[test]
fn s18_4_c_2_6() {
    let mut at = attributes(SETCGROUP);
    let mut cg = 7;
    x::posix_spawnattr_getcgroup_np(Some(&at), Some(&mut cg));
    assert_eq!(cg, 0, "the default");
    x::posix_spawnattr_setcgroup_np(Some(&mut at), -1);
    assert_eq!(spawn_sh("exit 0", None, Some(&at)).0, EINVAL);
    let dir = scratch("cgroup");
    let d = path_c(&dir);
    let fd = CSys.openat(Fd::CWD, kpath(&d), O_DIRECTORY as u32 | O_CLOEXEC, 0).unwrap();
    x::posix_spawnattr_setcgroup_np(Some(&mut at), fd.raw());
    assert_eq!(spawn_sh("exit 0", None, Some(&at)).0, EBADF);
    let _ = CSys.close(fd);
}

/// §18.4 Guarantees: errors before the program runs are the return value, for every failing
/// action, attribute and exec; no child is left and no descriptor leaks; earlier actions took
/// effect.
#[test]
fn s18_4_g_1() {
    let d = scratch("childerr");
    write_file(&d.join("afile"), "a\n", 0o644);
    write_file(&d.join("mode000"), "x\n", 0o000);
    let dc = d.clone();
    let out = alone_ok("s18_4_g_1", "run", move || {
        let p = |n: &str| path_c(&dc.join(n));
        let a = p("afile");
        let file = CSys.openat(Fd::CWD, kpath(&a), 0, 0).unwrap();
        CSys.dup2(file, Fd::new(20)).unwrap();
        let _ = CSys.close(file);
        type Setup<'a> = Box<dyn Fn(&mut CFileActions) + 'a>;
        let cases: Vec<(&str, Setup<'_>, &str)> = vec![
            ("open missing", Box::new(|fa| { x::posix_spawn_file_actions_addopen(Some(fa), 5, p("missing").as_ptr(), O_RDONLY, 0); }), "/bin/sh"),
            ("open mode 000", Box::new(|fa| { x::posix_spawn_file_actions_addopen(Some(fa), 5, p("mode000").as_ptr(), O_RDONLY, 0); }), "/bin/sh"),
            ("open O_EXCL existing", Box::new(|fa| { x::posix_spawn_file_actions_addopen(Some(fa), 5, p("afile").as_ptr(), O_WRONLY | O_CREAT | O_EXCL, 0o644); }), "/bin/sh"),
            ("dup2 closed", Box::new(|fa| { x::posix_spawn_file_actions_adddup2(Some(fa), 40, 5); }), "/bin/sh"),
            ("dup2 same closed", Box::new(|fa| { x::posix_spawn_file_actions_adddup2(Some(fa), 40, 40); }), "/bin/sh"),
            ("chdir missing", Box::new(|fa| { x::posix_spawn_file_actions_addchdir_np(Some(fa), p("missing").as_ptr()); }), "/bin/sh"),
            ("chdir file", Box::new(|fa| { x::posix_spawn_file_actions_addchdir_np(Some(fa), p("afile").as_ptr()); }), "/bin/sh"),
            ("fchdir closed", Box::new(|fa| { x::posix_spawn_file_actions_addfchdir_np(Some(fa), 40); }), "/bin/sh"),
            ("fchdir file", Box::new(|fa| { x::posix_spawn_file_actions_addfchdir_np(Some(fa), 20); }), "/bin/sh"),
            ("tcsetpgrp not a tty", Box::new(|fa| { x::posix_spawn_file_actions_addtcsetpgrp_np(Some(fa), 20); }), "/bin/sh"),
            ("closefrom then dup2", Box::new(|fa| { x::posix_spawn_file_actions_addclosefrom_np(Some(fa), 3); x::posix_spawn_file_actions_adddup2(Some(fa), 20, 6); }), "/bin/sh"),
            ("create then exec missing", Box::new(|fa| { x::posix_spawn_file_actions_addopen(Some(fa), 5, p("created").as_ptr(), O_WRONLY | O_CREAT, 0o644); }), "missing-program"),
            ("exec not executable", Box::new(|_| {}), "afile"),
        ];
        for (label, setup, prog) in &cases {
            let before = lowest_free();
            let mut fa = actions();
            setup(&mut fa);
            let prog = if prog.starts_with('/') { cs(prog) } else { p(prog) };
            let argv = Args::new(&["x"]);
            let mut pid = -1;
            let r = x::posix_spawn(Some(&mut pid), prog.as_ptr(), Some(&fa), None, argv.p(), environ());
            errno::set_raw(0);
            let left = x::waitpid(-1, None, WNOHANG);
            say(&format!("R:{label}: r={r} left={left}/{} leak={}\n", errno::get(), lowest_free() != before));
            x::posix_spawn_file_actions_destroy(Some(&mut fa));
        }
        say(&format!("R:created {}\n", dc.join("created").exists()));
        let mut at = attributes(SETPGROUP);
        x::posix_spawnattr_setpgroup(Some(&mut at), 0x3fff_fff0);
        let before = lowest_free();
        let r = spawn_sh("exit 0", None, Some(&at)).0;
        say(&format!("R:setpgroup missing: r={r} left={} leak={}\n", x::waitpid(-1, None, WNOHANG), lowest_free() != before));
        true
    });
    if out.is_empty() {
        return;
    }
    let want = [
        ("open missing", ENOENT),
        ("open mode 000", EACCES),
        ("open O_EXCL existing", EEXIST),
        ("dup2 closed", EBADF),
        ("dup2 same closed", EBADF),
        ("chdir missing", ENOENT),
        ("chdir file", ENOTDIR),
        ("fchdir closed", EBADF),
        ("fchdir file", ENOTDIR),
        ("tcsetpgrp not a tty", ENOTTY),
        ("closefrom then dup2", EBADF),
        ("create then exec missing", ENOENT),
        ("exec not executable", EACCES),
    ];
    let r = tagged(&out, "R:");
    for (label, e) in want {
        assert!(r.contains(&format!("{label}: r={e} left=-1/{ECHILD} leak=false")), "{label}: {out}");
    }
    assert!(r.contains(&"created true".to_string()), "actions before the failing exec took effect");
    assert!(r.contains(&format!("setpgroup missing: r={EPERM} left=-1 leak=false")), "{out}");
}

/// The write end of the pipe `record_pid` writes to.
static SIG_PIPE: AtomicU32 = AtomicU32::new(0);
/// Writes the PID it runs in.
extern "C" fn record_pid(_sig: i32) {
    let me = x::getpid().to_ne_bytes();
    let _ = CSys.write(Fd::new(SIG_PIPE.load(Ordering::SeqCst) as i32), &me);
}

/// §18.4 Guarantees: no handler runs in the child between clone and exec: a signal sent to the
/// child while a file action blocks it (an open of a FIFO) runs the handler only in the
/// caller, and reaches the child once its mask is set (killing it, or pending under
/// `SETSIGMASK`).
#[test]
fn s18_4_g_2() {
    let d = scratch("window");
    let fifo = d.join("fifo");
    assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());
    let run = |case: &'static str, masked: bool| {
        let fifo = fifo.clone();
        alone_ok("s18_4_g_2", case, move || {
            x::setpgid(0, 0);
            let [rd, wr] = CSys.pipe2(O_CLOEXEC).unwrap();
            SIG_PIPE.store(wr.raw() as u32, Ordering::SeqCst);
            sg::signal(SIGUSR2, handler(record_pid));
            let f2 = fifo.clone();
            let t = std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                sg::kill(0, SIGUSR2);
                std::thread::sleep(std::time::Duration::from_millis(200));
                let _ = std::fs::OpenOptions::new().write(true).open(&f2);
            });
            let mut fa = actions();
            x::posix_spawn_file_actions_addopen(Some(&mut fa), 5, path_c(&fifo).as_ptr(), O_RDONLY, 0);
            let mut at = attributes(if masked { SETSIGMASK } else { 0 });
            if masked {
                let mut m = sigset_t { __val: [0; 16] };
                sg::sigaddset(Some(&mut m), SIGUSR2);
                x::posix_spawnattr_setsigmask(Some(&mut at), Some(&m));
            }
            let argv = Args::new(&["grep", "-E", "-e", "^(SigBlk|SigIgn|SigCgt|SigPnd|ShdPnd):", "/proc/self/status"]);
            let mut pid = -1;
            let r = x::posix_spawn(Some(&mut pid), cs(GREP).as_ptr(), Some(&fa), Some(&at), argv.p(), environ());
            let st = if r == 0 { wait_for(pid) } else { -1 };
            t.join().unwrap();
            let _ = CSys.close(wr);
            let pids = read_bytes(rd);
            let me = x::getpid();
            let (mut here, mut elsewhere) = (0, 0);
            for c in pids.chunks(4) {
                if i32::from_ne_bytes(c.try_into().unwrap()) == me {
                    here += 1;
                } else {
                    elsewhere += 1;
                }
            }
            say(&format!("R:r={r} status={st:#x} handler_here={here} handler_elsewhere={elsewhere}\n"));
            true
        })
    };
    let plain = run("plain", false);
    let masked = run("masked", true);
    if plain.is_empty() {
        return;
    }
    assert!(tagged(&plain, "R:").contains(&format!("r=0 status={SIGUSR2:#x} handler_here=1 handler_elsewhere=0")), "killed by the signal after the actions: {plain}");
    let m = tagged(&masked, "R:");
    assert!(m.contains(&"r=0 status=0x0 handler_here=1 handler_elsewhere=0".to_string()), "{masked}");
    assert_ne!((status_field(&masked, "SigPnd:") | status_field(&masked, "ShdPnd:")) & bit(SIGUSR2), 0, "pending in the new program");
}

/// §18.4 Guarantees: safe in multi-threaded programs: 4 threads × 25 spawns, every child
/// reports its own status.
#[test]
fn s18_4_g_3() {
    let ok = AtomicU32::new(0);
    std::thread::scope(|s| {
        for t in 0..4 {
            let ok = &ok;
            let work = move || {
                for i in 0..25 {
                    let code = t * 25 + i;
                    let (r, pid) = spawn_sh(&format!("exit {code}"), None, None);
                    if r == 0 && wait_for(pid) == code << 8 {
                        ok.fetch_add(1, Ordering::SeqCst);
                    }
                }
            };
            // FL-3/FL-4: the kernel refuses clone3 transiently with EAGAIN in this busy process
            // (also without Liella; it clears in 4-23 ms). The clause tests posix_spawn, not thread
            // creation, so retry EAGAIN only, for at most ~1 s; any other error fails at once.
            let started = std::time::Instant::now();
            let mut retries = 0;
            let mut spawned = std::thread::Builder::new().spawn_scoped(s, work);
            while matches!(&spawned, Err(e) if e.raw_os_error() == Some(11)) && started.elapsed() < std::time::Duration::from_secs(1) {
                retries += 1;
                std::thread::sleep(std::time::Duration::from_millis(5));
                spawned = std::thread::Builder::new().spawn_scoped(s, work);
            }
            if retries > 0 {
                eprintln!("s18_4_g_3: thread spawn retried {retries}x after EAGAIN");
            }
            if let Err(e) = spawned {
                // FL-1: this spawn failed with EAGAIN in the gate's context only, with no limit
                // reached by the numbers seen afterwards. Record the state, then fail as before.
                eprintln!("{}", thread_spawn_state(&e));
                panic!("failed to spawn thread: {e}");
            }
        }
    });
    assert_eq!(ok.load(Ordering::SeqCst), 100);
}

/// The process state at a failed thread creation (evidence for FL-1's s18_4_g_3): the error,
/// limits, thread and memory counts, the number of mappings and the cgroup's event counters.
fn thread_spawn_state(e: &std::io::Error) -> String {
    let read = |p: &str| std::fs::read_to_string(p).unwrap_or_else(|err| format!("<{p}: {err}>"));
    let status = read("/proc/self/status");
    let keep: Vec<&str> = status.lines().filter(|l| ["Threads:", "VmSize:", "VmRSS:", "VmData:", "VmStk:", "Cpus_allowed_list:"].iter().any(|k| l.starts_with(k))).collect();
    let cgroup = read("/proc/self/cgroup");
    let path = cgroup.lines().find_map(|l| l.strip_prefix("0::")).unwrap_or("/");
    let events = |f: &str| read(&format!("/sys/fs/cgroup{path}/{f}")).split_whitespace().collect::<Vec<_>>().join(" ");
    format!(
        "thread spawn failed: {e} (errno {:?})
{}
maps lines: {}
limits:
{}cgroup {path}
 memory.events: {}
 pids.events: {}
 pids.current: {}
 pids.max: {}
",
        e.raw_os_error(),
        keep.join("
"),
        read("/proc/self/maps").lines().count(),
        read("/proc/self/limits"),
        events("memory.events"),
        events("pids.events"),
        events("pids.current"),
        events("pids.max"),
    )
}

/// §18.4 Failure: the failing action's, the creation's or the exec's error number;
/// `posix_spawnp` returns `ENOEXEC` (no shell, unlike `execvp`); `EAGAIN` when no process may
/// be created.
#[test]
fn s18_4_f_0() {
    let d = exec_fixture("exec-spawn-f0");
    let argv = Args::new(&["prog"]);
    let mut pid = -1;
    assert_eq!(x::posix_spawn(Some(&mut pid), path_c(&d.join("plain/prog")).as_ptr(), None, None, argv.p(), environ()), ENOEXEC);
    assert_eq!(x::posix_spawn(Some(&mut pid), path_c(&d.join("loop/prog")).as_ptr(), None, None, argv.p(), environ()), ELOOP);
    assert_eq!(x::posix_spawn(Some(&mut pid), cs("").as_ptr(), None, None, argv.p(), environ()), ENOENT);
    let big = "b".repeat(131_072);
    assert_eq!(x::posix_spawn(Some(&mut pid), cs("/bin/sh").as_ptr(), None, None, Args::new(&["sh", &big]).p(), environ()), E2BIG);
    let cases = [("p-eacces-ok", path_of(&d, &["noexec", "ok"]), 0), ("p-eacces", path_of(&d, &["noexec"]), EACCES), ("p-enoexec", path_of(&d, &["plain", "ok"]), ENOEXEC), ("p-eloop", path_of(&d, &["loop", "ok"]), ELOOP)];
    for (case, path, want) in cases {
        let p = path.clone();
        if let Some((out, _)) = alone("s18_4_f_0", case, move || {
            set_env("PATH", &p);
            let mut pid = -1;
            let r = x::posix_spawnp(Some(&mut pid), cs("prog").as_ptr(), None, None, Args::new(&["prog"]).p(), environ());
            if r == 0 {
                wait_for(pid);
            }
            say(&format!("R:{r}\n"));
            true
        }) {
            assert_eq!(tagged(&out, "R:").last(), Some(&want.to_string()), "posix_spawnp with PATH={path}");
        }
    }
    if CSys.geteuid() != 0 {
        if let Some((out, _)) = alone("s18_4_f_0", "nproc", || {
            CSys.prlimit(0, 6, Some(&liella_kernel::uapi::Rlimit { cur: 0, max: 0 })).unwrap();
            say(&format!("R:{}\n", spawn_sh("exit 0", None, None).0));
            true
        }) {
            assert_eq!(tagged(&out, "R:"), vec![EAGAIN.to_string()], "creation refused (RLIMIT_NPROC 0)");
        }
    }
}

// ------------------------------------------------------------------------------ §18.5 pidfd

/// A child that runs until its standard input closes, with `/dev/null` on its descriptor 9:
/// its PID and the pipe's write end.
fn held_child() -> (i32, Fd) {
    let [rd, wr] = CSys.pipe2(O_CLOEXEC).unwrap();
    let mut fa = actions();
    x::posix_spawn_file_actions_adddup2(Some(&mut fa), rd.raw(), 0);
    let file = CSys.openat(Fd::CWD, liella_kernel::KStr::from_nul_terminated(b"/dev/null\0").unwrap(), O_CLOEXEC, 0).unwrap();
    x::posix_spawn_file_actions_adddup2(Some(&mut fa), file.raw(), 9);
    let (r, pid) = spawn_sh("read x; exit 0", Some(&fa), None);
    assert_eq!(r, 0);
    let _ = CSys.close(rd);
    let _ = CSys.close(file);
    x::posix_spawn_file_actions_destroy(Some(&mut fa));
    (pid, wr)
}

fn poll_in(fd: i32, ms: i64) -> usize {
    let mut p = [liella_kernel::uapi::PollFd { fd, events: 1, revents: 0 }];
    let mut t = liella_kernel::uapi::Timespec { sec: ms / 1000, nsec: (ms % 1000) * 1_000_000 };
    CSys.ppoll(&mut p, Some(&mut t), None).unwrap()
}

/// §18.5 Contract: `pidfd_open` refers to the process, is readable once it exits (before it is
/// reaped), and takes `PIDFD_NONBLOCK`.
#[test]
fn s18_5_c_1() {
    let (pid, hold) = held_child();
    let pfd = x::pidfd_open(pid, 0);
    assert!(pfd >= 0);
    assert_eq!(fd_flags(pfd), 1, "close-on-exec");
    assert_eq!(poll_in(pfd, 0), 0, "running");
    let nb = x::pidfd_open(pid, O_NONBLOCK as u32);
    assert_ne!(CSys.fcntl_int(Fd::new(nb), liella_kernel::ops::FcntlInt::GetFl, 0).unwrap() & i64::from(O_NONBLOCK), 0);
    let mut si = SigInfo::default();
    errno::set_raw(0);
    assert_eq!((x::waitid(P_PIDFD, nb as u32, Some(&mut si), WEXITED), errno::get()), (-1, EAGAIN), "non-blocking wait on a running child");
    let _ = CSys.close(hold);
    assert_eq!(poll_in(pfd, 5000), 1, "readable once it exits");
    assert_eq!(x::pidfd_getpid(pfd), pid, "still there as a zombie");
    assert_eq!(wait_for(pid), 0);
    for fd in [pfd, nb] {
        let _ = CSys.close(Fd::new(fd));
    }
}

/// §18.5 Contract: `pidfd_getfd` duplicates a descriptor of the other process.
#[test]
fn s18_5_c_2() {
    let (pid, hold) = held_child();
    let pfd = x::pidfd_open(pid, 0);
    let got = x::pidfd_getfd(pfd, 9, 0);
    assert!(got >= 0, "errno {}", errno::get());
    let null = CSys.fstatat(Fd::CWD, liella_kernel::KStr::from_nul_terminated(b"/dev/null\0").unwrap(), 0).unwrap();
    assert_eq!(CSys.fstat(Fd::new(got)).unwrap().rdev, null.rdev, "the child's /dev/null");
    assert_eq!(fd_flags(got), 1, "close-on-exec");
    errno::set_raw(0);
    assert_eq!((x::pidfd_getfd(pfd, 50, 0), errno::get()), (-1, EBADF), "not open in the target");
    assert_eq!((x::pidfd_getfd(pfd, 9, 1), errno::get()), (-1, EINVAL), "flags");
    for fd in [got, pfd] {
        let _ = CSys.close(Fd::new(fd));
    }
    let _ = CSys.close(hold);
    wait_for(pid);
}

/// §18.5 Contract: `pidfd_send_signal` signals the process the pidfd refers to, never another
/// one that reuses its PID.
#[test]
fn s18_5_c_3() {
    let (pid, hold) = held_child();
    let pfd = x::pidfd_open(pid, 0);
    assert_eq!(x::pidfd_send_signal(pfd, 0, None, 0), 0, "signal 0 checks");
    assert_eq!(x::pidfd_send_signal(pfd, SIGTERM, None, 0), 0);
    assert_eq!(ChildStatus::from_word(wait_for(pid)), Some(ChildStatus::Killed { signal: SIGTERM as u8, core: false }));
    errno::set_raw(0);
    assert_eq!((x::pidfd_send_signal(pfd, 0, None, 0), errno::get()), (-1, ESRCH), "reaped");
    let _ = CSys.close(Fd::new(pfd));
    let _ = CSys.close(hold);
}

/// §18.5 Contract: `pidfd_getpid` gives the PID from `/proc/self/fdinfo`.
#[test]
fn s18_5_c_4() {
    let (pid, hold) = held_child();
    let pfd = x::pidfd_open(pid, 0);
    assert_eq!(x::pidfd_getpid(pfd), pid);
    let _ = CSys.close(hold);
    wait_for(pid);
    errno::set_raw(0);
    assert_eq!((x::pidfd_getpid(pfd), errno::get()), (-1, ESRCH), "reaped");
    let _ = CSys.close(Fd::new(pfd));
    let [rd, wr] = CSys.pipe2(O_CLOEXEC).unwrap();
    assert_eq!((x::pidfd_getpid(rd.raw()), errno::get()), (-1, EBADF), "not a pidfd");
    for f in [rd, wr] {
        let _ = CSys.close(f);
    }
    assert_eq!((x::pidfd_getpid(-1), errno::get()), (-1, EBADF));
    let me = x::pidfd_open(x::getpid(), 0);
    assert_eq!(x::pidfd_getpid(me), x::getpid());
    let _ = CSys.close(Fd::new(me));
}

/// §18.5 Failure: `ESRCH`, `EPERM`, `EINVAL`.
#[test]
fn s18_5_f_0() {
    errno::set_raw(0);
    assert_eq!((x::pidfd_open(x::getpid(), 1), errno::get()), (-1, EINVAL), "flags");
    assert_eq!((x::pidfd_open(0, 0), errno::get()), (-1, EINVAL));
    assert_eq!((x::pidfd_open(0x3fff_fff0, 0), errno::get()), (-1, ESRCH), "no such process");
    let (pid, hold) = held_child();
    let pfd = x::pidfd_open(pid, 0);
    let mut info = SigInfo::default();
    info.signo = SIGTERM;
    info.code = 0;
    info.fields[0] = u64::from(x::getpid() as u32) | (u64::from(CSys.getuid()) << 32);
    assert_eq!((x::pidfd_send_signal(pfd, SIGTERM, Some(&info), 0), errno::get()), (-1, EPERM), "SI_USER information for another process");
    assert_eq!((x::pidfd_send_signal(pfd, 99, None, 0), errno::get()), (-1, EINVAL));
    let _ = CSys.close(hold);
    wait_for(pid);
    let _ = CSys.close(Fd::new(pfd));
}

// ------------------------------------------------------------------------------ §18.6 waiting

/// §18.6 Contract: wait for and collect a child's state change.
#[test]
fn s18_6_c_0() {
    let (_, pid) = spawn_sh("exit 9", None, None);
    let mut st = 0;
    assert_eq!(x::wait4(pid, Some(&mut st), 0, None), pid);
    assert_eq!(st, 9 << 8);
    errno::set_raw(0);
    assert_eq!((x::waitpid(pid, Some(&mut st), 0), errno::get()), (-1, ECHILD), "collected once");
}

/// §18.6 Contract: exited, killed, stopped (`WUNTRACED`), continued (`WCONTINUED`).
#[test]
fn s18_6_c_1() {
    // The child sleeps after the continue until it is killed, so that its exit cannot overtake
    // the continued report under load (a waitpid with WCONTINUED alone fails once it has exited).
    let (_, pid) = spawn_sh("kill -STOP $$; exec sleep 60", None, None);
    let mut st = 0;
    assert_eq!(x::waitpid(pid, Some(&mut st), WUNTRACED), pid);
    assert_eq!((ChildStatus::from_word(st), st), (Some(ChildStatus::Stopped(SIGSTOP as u8)), 0x137f));
    assert_eq!(x::waitpid(pid, Some(&mut st), WUNTRACED | WNOHANG), 0, "a stop is reported once");
    sg::kill(pid, SIGCONT);
    assert_eq!(x::waitpid(pid, Some(&mut st), WCONTINUED), pid);
    assert_eq!((ChildStatus::from_word(st), st), (Some(ChildStatus::Continued), 0xffff));
    sg::kill(pid, SIGKILL);
    assert_eq!(ChildStatus::from_word(wait_for(pid)), Some(ChildStatus::Killed { signal: SIGKILL as u8, core: false }));
    let (_, pid) = spawn_sh("exit 5", None, None);
    assert_eq!(x::waitpid(pid, Some(&mut st), 0), pid);
    assert_eq!(ChildStatus::from_word(st), Some(ChildStatus::Exited(5)));
    let (_, pid) = spawn_sh("kill -KILL $$", None, None);
    assert_eq!(ChildStatus::from_word(wait_for(pid)), Some(ChildStatus::Killed { signal: SIGKILL as u8, core: false }));
}

/// §18.6 Contract: `waitpid` selects by PID, by group (`-pgid`, 0 for the caller's) or any
/// child (-1). Alone: -1 and 0 would collect other tests' children.
#[test]
fn s18_6_c_2() {
    let out = alone_ok("s18_6_c_2", "run", || {
        let (_, a) = spawn_sh("exit 1", None, None);
        let mut at = attributes(SETPGROUP);
        x::posix_spawnattr_setpgroup(Some(&mut at), 0);
        let (_, b) = spawn_sh("exit 2", None, Some(&at));
        let mut st = 0;
        let same = x::waitpid(0, Some(&mut st), 0);
        let other = x::waitpid(-b, Some(&mut st), 0);
        let (_, c) = spawn_sh("exit 3", None, None);
        let any = x::waitpid(-1, Some(&mut st), 0);
        say(&format!("R:{} {} {}\n", same == a, other == b, any == c && st == 3 << 8));
        true
    });
    if !out.is_empty() {
        assert_eq!(tagged(&out, "R:"), vec!["true true true"]);
    }
}

/// §18.6 Contract: `waitid` with `P_PID`, `P_PGID`, `P_ALL`, `P_PIDFD`, and `WEXITED`,
/// `WSTOPPED`, `WCONTINUED`, `WNOWAIT`: the `siginfo_t` describes the change.
#[test]
fn s18_6_c_3() {
    let (_, pid) = spawn_sh("exit 300", None, None);
    let mut si = SigInfo::default();
    assert_eq!(x::waitid(P_PID, pid as u32, Some(&mut si), WEXITED | WNOWAIT), 0);
    assert_eq!((si.signo, si.code, si.pid(), si.uid(), si.status()), (SIGCHLD, 1, pid, CSys.getuid(), 44), "CLD_EXITED with the low 8 bits (the kernel's)");
    assert_eq!(wait_for(pid), 44 << 8, "WNOWAIT left it waitable");
    // The continued child sleeps until it is killed, so that its exit cannot overtake the
    // continued report (waitid with WCONTINUED alone fails once the child has exited).
    let (_, pid) = spawn_sh("kill -STOP $$; exec sleep 60", None, None);
    assert_eq!(x::waitid(P_PID, pid as u32, Some(&mut si), 2), 0, "WSTOPPED");
    assert_eq!((si.code, si.status()), (5, SIGSTOP), "CLD_STOPPED");
    sg::kill(pid, SIGCONT);
    assert_eq!(x::waitid(P_PID, pid as u32, Some(&mut si), WCONTINUED), 0);
    assert_eq!((si.code, si.status()), (6, SIGCONT), "CLD_CONTINUED");
    sg::kill(pid, SIGKILL);
    assert_eq!(wait_for(pid), SIGKILL);
    let mut at = attributes(SETPGROUP);
    x::posix_spawnattr_setpgroup(Some(&mut at), 0);
    let (_, pid) = spawn_sh("kill -TERM $$", None, Some(&at));
    assert_eq!(x::waitid(P_PGID, pid as u32, Some(&mut si), WEXITED), 0);
    assert_eq!((si.code, si.status(), si.pid()), (2, SIGTERM, pid), "CLD_KILLED, by group");
    errno::set_raw(0);
    assert_eq!((x::waitid(99, 0, Some(&mut si), WEXITED), errno::get()), (-1, EINVAL), "an unknown id type");
    let out = alone_ok("s18_6_c_3", "all", || {
        let (_, pid) = spawn_sh("exit 8", None, None);
        let mut si = SigInfo::default();
        let r = x::waitid(P_ALL, 12345, Some(&mut si), WEXITED);
        say(&format!("R:{r} {} {}\n", si.pid() == pid, si.status()));
        true
    });
    if !out.is_empty() {
        assert_eq!(tagged(&out, "R:"), vec!["0 true 8"], "P_ALL ignores the id");
    }
}

/// §18.6 Contract: `wait3`/`wait4` report the child's resource usage.
#[test]
fn s18_6_c_4() {
    let (_, pid) = spawn_sh("i=0; while [ $i -lt 30000 ]; do i=$((i+1)); done", None, None);
    let mut ru = Rusage::default();
    let mut st = -1;
    assert_eq!(x::wait4(pid, Some(&mut st), 0, Some(&mut ru)), pid);
    assert_eq!(st, 0);
    let us = (ru.utime.sec + ru.stime.sec) * 1_000_000 + ru.utime.usec + ru.stime.usec;
    assert!(us > 0 && ru.maxrss > 0, "the child's CPU time and memory: {ru:?}");
    let out = alone_ok("s18_6_c_4", "wait3", || {
        let (_, pid) = spawn_sh("exit 2", None, None);
        let mut ru = Rusage::default();
        let mut st = 0;
        let r = x::wait3(Some(&mut st), 0, Some(&mut ru));
        say(&format!("R:{} {st} {}\n", r == pid, ru.maxrss > 0));
        true
    });
    if !out.is_empty() {
        assert_eq!(tagged(&out, "R:"), vec![format!("true {} true", 2 << 8)]);
    }
}

/// §18.6 Guarantees: the status word decodes (the macros' layout, D-18.4): an 8-bit exit code,
/// the signal, a stop, a continue; a reaped child is gone.
#[test]
fn s18_6_g_0() {
    for code in 0..=255u8 {
        assert_eq!(ChildStatus::from_word(i32::from(code) << 8), Some(ChildStatus::Exited(code)));
        assert_eq!(ChildStatus::Exited(code).word(), i32::from(code) << 8);
    }
    for sig in 1..=64u8 {
        assert_eq!(ChildStatus::from_word(i32::from(sig)), Some(ChildStatus::Killed { signal: sig, core: false }));
        assert_eq!(ChildStatus::from_word(i32::from(sig) | 0x80), Some(ChildStatus::Killed { signal: sig, core: true }));
        assert_eq!(ChildStatus::Killed { signal: sig, core: false }.word(), i32::from(sig));
    }
    assert_eq!((ChildStatus::Stopped(19).word(), ChildStatus::from_word(0x137f)), (0x137f, Some(ChildStatus::Stopped(19))));
    assert_eq!(ChildStatus::from_word(0xffff), Some(ChildStatus::Continued));
    let (_, pid) = spawn_sh("exit 256", None, None);
    assert_eq!(wait_for(pid), 0, "only 8 bits of the exit code");
    errno::set_raw(0);
    assert_eq!((sg::kill(pid, 0), errno::get()), (-1, ESRCH), "no zombie left");
}

static ALARMS: AtomicU32 = AtomicU32::new(0);
extern "C" fn on_alarm(_sig: i32) {
    ALARMS.fetch_add(1, Ordering::SeqCst);
}

/// §18.6 Failure: `ECHILD` (no such child; `SIGCHLD` ignored), `EINTR`, `EINVAL`.
#[test]
fn s18_6_f_0() {
    errno::set_raw(0);
    assert_eq!((x::waitpid(1, None, 0), errno::get()), (-1, ECHILD), "not a child");
    assert_eq!((x::waitpid(-0x3fff_fff0, None, 0), errno::get()), (-1, ECHILD), "no such group");
    assert_eq!((x::waitpid(x::getpid(), None, 0x1000), errno::get()), (-1, EINVAL), "unknown option bits");
    let mut si = SigInfo::default();
    assert_eq!((x::waitid(P_PID, 1, Some(&mut si), 0), errno::get()), (-1, EINVAL), "no state option");
    assert_eq!((x::waitid(P_PID, 1, Some(&mut si), WNOHANG), errno::get()), (-1, EINVAL), "WNOHANG alone");
    let out = alone_ok("s18_6_f_0", "run", || {
        sg::signal(SIGCHLD, 1);
        let (_, pid) = spawn_sh("exit 3", None, None);
        errno::set_raw(0);
        let r = x::waitpid(pid, None, 0);
        say(&format!("R:ignored {r} {}\n", errno::get()));
        sg::signal(SIGCHLD, 0);
        let sa = liella_legacy::signal_compat::CSigaction::new(handler(on_alarm), liella_kernel::uapi::SigSet::empty(), 0);
        sg::sigaction(SIGALRM, Some(&sa), None);
        let (_, pid) = spawn_sh("sleep 1", None, None);
        // The signal goes to this thread (the harness has others).
        let (me, tid) = (x::getpid(), x::gettid());
        let t = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let _ = CSys.tgkill(me, tid, SIGALRM);
        });
        errno::set_raw(0);
        let r = x::waitpid(pid, None, 0);
        t.join().unwrap();
        say(&format!("R:interrupted {r} {} {}\n", errno::get(), ALARMS.load(Ordering::SeqCst)));
        wait_for(pid);
        true
    });
    if !out.is_empty() {
        assert_eq!(tagged(&out, "R:"), vec![format!("ignored -1 {ECHILD}"), format!("interrupted -1 {EINTR} 1")]);
    }
}

/// §18.6 Cancellation: the waits are cancellation points; in a thread Liella runs (`s18_exec`).
#[test]
fn s18_6_x_0() {
    conformance(&["s18_exec"]);
}

// ------------------------------------------------------------------------------ §18.7 shell

/// §18.7 Contract: `system` runs `/bin/sh -c`, blocks `SIGCHLD` and ignores `SIGINT`/`SIGQUIT`
/// in the caller while it waits, returns the wait status (127's when the shell cannot run);
/// `system(NULL)` says whether a shell runs.
#[test]
fn s18_7_c_1() {
    let out = alone_ok("s18_7_c_1", "run", || {
        sg::signal(SIGINT, handler(count_signal));
        sg::signal(SIGQUIT, handler(count_signal));
        let c = |s: &str| x::system(cs(s).as_ptr());
        say(&format!("R:null {}\n", x::system(core::ptr::null())));
        say(&format!("R:exit {:#x} {:#x} {:#x}\n", c("exit 3"), c("kill -TERM $$"), c("no_such_command_xyz 2>/dev/null")));
        say(&format!("R:argv {}\n", c("[ \"$0\" = sh ] && [ $# = 0 ] && exit 0; exit 1")));
        // The calling thread's mask (system blocks SIGCHLD in it alone) and the shell's state.
        let script = format!("grep -E '^Sig(Blk|Ign):' /proc/{}/task/{}/status | sed 's/^/R:caller /'; {REPORT_SIGNALS} | sed 's/^/R:shell /'", x::getpid(), x::gettid());
        say(&format!("R:status {}\n", c(&script)));
        let mut sa = liella_legacy::signal_compat::CSigaction::new(0, liella_kernel::uapi::SigSet::empty(), 0);
        sg::sigaction(SIGINT, None, Some(&mut sa));
        say(&format!("R:restored {}\n", sa.handler == handler(count_signal)));
        true
    });
    if out.is_empty() {
        return;
    }
    let r = tagged(&out, "R:");
    assert!(r.contains(&"null 1".to_string()));
    assert!(r.contains(&format!("exit {:#x} {:#x} {:#x}", 3 << 8, SIGTERM, 127 << 8)), "{out}");
    assert!(r.contains(&"argv 0".to_string()), "$0 is sh, no arguments");
    let caller = tagged(&out, "R:caller ").join("\n");
    assert_eq!(status_field(&caller, "SigBlk:") & bit(SIGCHLD), bit(SIGCHLD), "SIGCHLD blocked while it waits");
    assert_eq!(status_field(&caller, "SigIgn:") & (bit(SIGINT) | bit(SIGQUIT)), bit(SIGINT) | bit(SIGQUIT), "SIGINT and SIGQUIT ignored");
    let shell = tagged(&out, "R:shell ").join("\n");
    let int_quit = bit(SIGINT) | bit(SIGQUIT);
    assert_eq!((status_field(&shell, "SigBlk:"), status_field(&shell, "SigIgn:") & int_quit), (0, 0), "the shell has the caller's original state");
    assert!(r.contains(&"restored true".to_string()));
}

fn line_of(fp: *mut liella_abi::layout::FILE) -> Vec<u8> {
    let mut line = [0 as c_char; 128];
    if io::fgets(line.as_mut_ptr(), 128, fp).is_null() {
        return Vec::new();
    }
    line.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect()
}

/// §18.7 Contract: `popen` (`r`, `w`, `re`, `we`) and `pclose`.
#[test]
fn s18_7_c_2() {
    let fp = x::popen(cs("echo hi; exit 4").as_ptr(), cs("r").as_ptr());
    assert!(!fp.is_null());
    assert_eq!(line_of(fp), b"hi\n");
    assert_eq!(fd_flags(io::fileno(fp)), 0, "without e: inheritable");
    assert_eq!(x::pclose(fp), 4 << 8);
    let dir = scratch("popen");
    let target = dir.join("out");
    let fp = x::popen(cs(&format!("cat > '{}'", target.display())).as_ptr(), cs("we").as_ptr());
    assert_eq!(fd_flags(io::fileno(fp)), 1, "e: close-on-exec");
    io::fputs(cs("written\n").as_ptr(), fp);
    assert_eq!(x::pclose(fp), 0);
    assert_eq!(std::fs::read_to_string(&target).unwrap(), "written\n");
    for (mode, ok) in [("re", true), ("er", true), ("w", true), ("", false), ("rw", false), ("rb", false), ("ee", false), ("rew", false), ("x", false)] {
        errno::set_raw(0);
        let fp = x::popen(cs("exit 0").as_ptr(), cs(mode).as_ptr());
        assert_eq!(!fp.is_null(), ok, "mode {mode:?}");
        if fp.is_null() {
            assert_eq!(errno::get(), EINVAL);
        } else {
            x::pclose(fp);
        }
    }
    let first = x::popen(cs("cat >/dev/null").as_ptr(), cs("w").as_ptr());
    let fd = io::fileno(first);
    let probe = x::popen(cs(&format!("[ -e /proc/$$/fd/{fd} ] && echo inherited || echo clean")).as_ptr(), cs("r").as_ptr());
    assert_eq!(line_of(probe), b"clean\n", "a popen child has no other popen stream's pipe");
    assert_eq!(x::pclose(probe), 0);
    assert_eq!(x::pclose(first), 0);
    // FL-4: close-on-exec ("e"), and closed again: a plain descriptor kept open here leaks into
    // a concurrent test's posix_spawn child (s18_4_c_1 saw an extra "open 8").
    let other = io::fopen(cs("/dev/null").as_ptr(), cs("re").as_ptr());
    errno::set_raw(0);
    assert_eq!((x::pclose(other), errno::get()), (-1, ECHILD), "a stream popen did not return");
    io::fclose(other);
}

/// §18.7 Threads: concurrent `system` calls share the `SIGINT`/`SIGQUIT` changes, reference
/// counted: they stay ignored until the last caller returns. Race-free: the long call is held
/// on a FIFO until the short one has returned and the state has been read.
#[test]
fn s18_7_t_0() {
    let dir = scratch("concurrent");
    let fifo = dir.join("fifo");
    assert!(std::process::Command::new("mkfifo").arg(&fifo).status().unwrap().success());
    let out = alone_ok("s18_7_t_0", "run", move || {
        sg::signal(SIGINT, handler(count_signal));
        sg::signal(SIGQUIT, handler(count_signal));
        let disp = |sig: i32| {
            let mut sa = liella_legacy::signal_compat::CSigaction::new(0, liella_kernel::uapi::SigSet::empty(), 0);
            sg::sigaction(sig, None, Some(&mut sa));
            match sa.handler {
                0 => "default",
                1 => "ignored",
                h if h == handler(count_signal) => "handler",
                _ => "other",
            }
        };
        let f = fifo.clone();
        let long = std::thread::spawn(move || x::system(cs(&format!("cat '{}' > /dev/null", f.display())).as_ptr()));
        // The long call is inside once the dispositions are ignored (set under its lock).
        while disp(SIGINT) != "ignored" {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        let short = x::system(cs("exit 6").as_ptr());
        let during = format!("{} {}", disp(SIGINT), disp(SIGQUIT));
        let mut m = sigset_t { __val: [0; 16] };
        sg::sigprocmask(0, None, Some(&mut m));
        let chld = sg::sigismember(Some(&m), SIGCHLD);
        drop(std::fs::OpenOptions::new().write(true).open(&fifo).unwrap());
        let first = long.join().unwrap();
        say(&format!("R:{short:#x} {during} {chld} {first} {} {}\n", disp(SIGINT), disp(SIGQUIT)));
        true
    });
    if !out.is_empty() {
        assert_eq!(tagged(&out, "R:"), vec![format!("{:#x} ignored ignored 0 0 handler handler", 6 << 8)], "{out}");
    }
}

// ------------------------------------------------------------------------------ §18.8 groups

/// §18.8 Contract: `setpgid` moves a child into its own or an existing group of the session;
/// the kernel's refusals.
#[test]
fn s18_8_c_1() {
    let c = selftest::spawn_pausing().unwrap();
    assert_eq!(x::setpgid(c, c), 0);
    assert_eq!(x::getpgid(c), c);
    assert_eq!(x::setpgid(c, x::getpgrp()), 0);
    assert_eq!(x::getpgid(c), x::getpgrp());
    assert_eq!(x::setpgid(c, 0), 0, "0: the process's own ID");
    assert_eq!(x::getpgid(c), c);
    errno::set_raw(0);
    assert_eq!((x::setpgid(c, -1), errno::get()), (-1, EINVAL));
    assert_eq!((x::setpgid(c, 0x3fff_fff0), errno::get()), (-1, EPERM), "no such group in the session");
    assert_eq!((x::setpgid(1, 1), errno::get()), (-1, ESRCH), "not a child");
    assert_eq!((x::__setpgid(-1, 0), errno::get()), (-1, EINVAL));
    kill_and_wait(c);
    // A child that has executed a program (and said so: the kernel marks the exec after the
    // spawn's caller has resumed).
    let [rd, wr] = CSys.pipe2(O_CLOEXEC).unwrap();
    let mut fa = actions();
    x::posix_spawn_file_actions_adddup2(Some(&mut fa), wr.raw(), 1);
    let (_, e) = spawn_sh("echo r; exec sleep 5", Some(&fa), None);
    let _ = CSys.close(wr);
    let mut b = [0u8; 1];
    assert_eq!(CSys.read(rd, &mut b), Ok(1));
    let _ = CSys.close(rd);
    errno::set_raw(0);
    assert_eq!((x::setpgid(e, 0), errno::get()), (-1, EACCES), "a child that has executed a program");
    kill_and_wait(e);
    x::posix_spawn_file_actions_destroy(Some(&mut fa));
}

/// §18.8 Contract: `setsid` makes a new session and group led by the caller; `EPERM` for a
/// group leader.
#[test]
fn s18_8_c_2() {
    fn body(k: &CSys) -> i32 {
        let me = k.getpid();
        if x::setsid() != me || x::getsid(0) != me || x::getpgrp() != me {
            return 1;
        }
        errno::set_raw(0);
        if x::setsid() != -1 || errno::get() != EPERM {
            return 2;
        }
        // A session leader cannot change its group.
        i32::from(x::setpgid(0, 0) != -1 || errno::get() != EPERM) * 3
    }
    assert_eq!(selftest::in_child(body).unwrap(), selftest::ChildEnd::Exited(0));
    fn leader(_: &CSys) -> i32 {
        x::setpgid(0, 0);
        errno::set_raw(0);
        i32::from(!(x::setsid() == -1 && errno::get() == EPERM))
    }
    assert_eq!(selftest::in_child(leader).unwrap(), selftest::ChildEnd::Exited(0));
}

/// §18.8 Contract: `getsid`, `getpgid` and `getpgrp` query (other sessions included).
#[test]
fn s18_8_c_3() {
    assert_eq!(x::getpgid(0), x::getpgrp());
    assert_eq!(x::__getpgid(x::getpid()), x::getpgrp());
    assert_eq!(x::getsid(0), x::getsid(x::getpid()));
    assert!(x::getsid(1) >= 0 && x::getpgid(1) >= 0, "process 1, another session");
    errno::set_raw(0);
    assert_eq!((x::getpgid(0x3fff_fff0), errno::get()), (-1, ESRCH));
    assert_eq!((x::getsid(0x3fff_fff0), errno::get()), (-1, ESRCH));
}

/// §18.8 Contract: `setpgrp` is `setpgid(0, 0)`; `__bsd_getpgrp(pid)` is `getpgid(pid)`.
#[test]
fn s18_8_c_4() {
    fn body(k: &CSys) -> i32 {
        if x::setpgrp() != 0 || x::getpgrp() != k.getpid() {
            return 1;
        }
        i32::from(x::__bsd_getpgrp(0) != k.getpid())
    }
    assert_eq!(selftest::in_child(body).unwrap(), selftest::ChildEnd::Exited(0));
    assert_eq!(x::__bsd_getpgrp(x::getpid()), x::getpgrp());
}

/// §18.8 Contract: `daemon` (a process Liella started: `s18_exec`).
#[test]
fn s18_8_c_5() {
    conformance(&["s18_exec"]);
}

// ------------------------------------------------------------------------------ §18.9 attributes

/// §18.9 Contract: `prctl` gets and sets attributes (results by value or through the pointer
/// argument); the variadic form is checked in `s18_exec`.
#[test]
fn s18_9_c_1() {
    const PR_SET_PDEATHSIG: i32 = 1;
    const PR_GET_PDEATHSIG: i32 = 2;
    const PR_GET_DUMPABLE: i32 = 3;
    const PR_SET_NAME: i32 = 15;
    const PR_GET_NAME: i32 = 16;
    std::thread::spawn(|| {
        let name = cs("abcdefghijklmnopqrstuvwxyz");
        assert_eq!(x::liella_prctl5(PR_SET_NAME, name.as_ptr() as u64, 0, 0, 0), 0);
        let mut got = [0u8; 16];
        assert_eq!(x::liella_prctl5(PR_GET_NAME, got.as_mut_ptr() as u64, 0, 0, 0), 0);
        assert_eq!(&got, b"abcdefghijklmno\0", "15 bytes and a NUL");
    })
    .join()
    .unwrap();
    fn pdeath(_: &CSys) -> i32 {
        let mut v = -1i32;
        if x::liella_prctl5(PR_SET_PDEATHSIG, 10, 0, 0, 0) != 0 || x::liella_prctl5(PR_GET_PDEATHSIG, core::ptr::from_mut(&mut v) as u64, 0, 0, 0) != 0 {
            return 1;
        }
        i32::from(v != 10)
    }
    assert_eq!(selftest::in_child(pdeath).unwrap(), selftest::ChildEnd::Exited(0));
    assert_eq!(x::liella_prctl5(PR_GET_DUMPABLE, 0, 0, 0, 0), 1, "the value is the result");
    errno::set_raw(0);
    assert_eq!((x::liella_prctl5(0x7fff_1234, 0, 0, 0, 0), errno::get()), (-1, EINVAL), "an unknown option");
    conformance(&["s18_exec"]);
}

/// §18.9 Contract: `personality` queries (`0xffffffff`) and sets, returning the previous persona.
#[test]
fn s18_9_c_2() {
    fn body(_: &CSys) -> i32 {
        const ADDR_NO_RANDOMIZE: u64 = 0x0040000;
        let before = x::personality(0xffff_ffff);
        let old = x::personality(ADDR_NO_RANDOMIZE);
        let now = x::personality(0xffff_ffff);
        i32::from(!(old == before && now as u64 == ADDR_NO_RANDOMIZE))
    }
    assert_eq!(selftest::in_child(body).unwrap(), selftest::ChildEnd::Exited(0));
}

/// §18.9 Contract: `unshare` and `setns`: thin; unprivileged, `CLONE_FILES` and the refusals.
#[test]
fn s18_9_c_3() {
    fn body(_: &CSys) -> i32 {
        const CLONE_FILES: i32 = 0x400;
        if x::unshare(CLONE_FILES) != 0 || x::unshare(0) != 0 {
            return 1;
        }
        errno::set_raw(0);
        i32::from(x::unshare(1) != -1 || errno::get() != EINVAL) * 2
    }
    assert_eq!(selftest::in_child(body).unwrap(), selftest::ChildEnd::Exited(0));
    errno::set_raw(0);
    assert_eq!((x::setns(-1, 0), errno::get()), (-1, EBADF));
    let null = CSys.openat(Fd::CWD, liella_kernel::KStr::from_nul_terminated(b"/dev/null\0").unwrap(), O_CLOEXEC, 0).unwrap();
    assert_eq!((x::setns(null.raw(), 0), errno::get()), (-1, EINVAL), "not a namespace");
    let _ = CSys.close(null);
}

/// §18.9 Contract: `arch_prctl` gets and sets x86 state.
#[test]
fn s18_9_c_4() {
    // ARCH(x86_64): arch_prctl is exported on x86_64 only (RL-8)
    const ARCH_GET_FS: i32 = 0x1003;
    const ARCH_GET_CPUID: i32 = 0x1011;
    let mut fs = 0u64;
    // ARCH(x86_64): arch_prctl is exported on x86_64 only (RL-8)
    assert_eq!(x::arch_prctl(ARCH_GET_FS, core::ptr::from_mut(&mut fs) as u64), 0);
    assert_eq!(fs as usize, liella_boundary::tb::tp(), "the FS base is the thread pointer");
    let mut again = 0u64;
    // ARCH(x86_64): arch_prctl is exported on x86_64 only (RL-8)
    assert_eq!(x::__arch_prctl(ARCH_GET_FS, core::ptr::from_mut(&mut again) as u64), 0);
    assert_eq!(again, fs);
    assert!(matches!(x::arch_prctl(ARCH_GET_CPUID, 0), 0 | 1));
    errno::set_raw(0);
    // ARCH(x86_64): arch_prctl is exported on x86_64 only (RL-8)
    assert_eq!((x::arch_prctl(0x7777, 0), errno::get()), (-1, EINVAL));
}

static PEEK_WORD: AtomicUsize = AtomicUsize::new(usize::MAX);

/// §18.9 Contract: `ptrace`'s `PEEK*` requests return the word: -1 is a valid word, told from
/// an error only by `errno`.
#[test]
fn s18_9_c_5() {
    const PTRACE_PEEKDATA: i32 = 2;
    const PTRACE_PEEKUSER: i32 = 3;
    const PTRACE_DETACH: i32 = 17;
    const PTRACE_SEIZE: i32 = 0x4206;
    const PTRACE_INTERRUPT: i32 = 0x4207;
    let c = selftest::spawn_pausing().unwrap();
    assert_eq!(x::liella_ptrace4(PTRACE_SEIZE, c, 0, 0), 0, "errno {}", errno::get());
    assert_eq!(x::liella_ptrace4(PTRACE_INTERRUPT, c, 0, 0), 0);
    let mut st = 0;
    assert_eq!(x::waitpid(c, Some(&mut st), 0), c);
    errno::set_raw(0);
    let w = x::liella_ptrace4(PTRACE_PEEKDATA, c, core::ptr::from_ref(&PEEK_WORD) as u64, 0);
    assert_eq!((w, errno::get()), (-1, 0), "the word is -1, errno untouched");
    let w = x::liella_ptrace4(PTRACE_PEEKDATA, c, 8, 0);
    assert_eq!((w, errno::get()), (-1, EIO), "an unmapped address");
    errno::set_raw(0);
    assert_eq!((x::liella_ptrace4(PTRACE_PEEKUSER, c, 1, 0), errno::get()), (-1, EIO), "misaligned");
    assert_eq!(x::liella_ptrace4(PTRACE_DETACH, c, 0, 0), 0);
    kill_and_wait(c);
}
