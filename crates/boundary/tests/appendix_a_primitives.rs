// Specified from spec Appendix A (kernel primitive catalogue). Generated stubs were replaced by
// real tests of the SAME NAME (see crates/spectest); `cargo xtask stubs` now only checks coverage.

//! Kernel-boundary tests, one per primitive of spec Appendix A (281 primitives), plus the
//! io_uring calls of the AIO engine (D-11.11), run against the real kernel through `CSys` (the
//! io_uring calls through `selftest`) and therefore through the C syscall layer.
//!
//! Conventions:
//! * Each test covers the success case, the errors reachable without privilege, and the
//!   argument layout where the kernel reads or writes a structure.
//! * Anything process-wide (signal dispositions, credentials, sessions, namespaces, timers,
//!   the umask, the working directory) runs in a forked child (`selftest::in_child`), so tests
//!   stay independent while the harness runs them in parallel threads.
//! * Privileged success paths run as root of a fresh user namespace in such a child. A host that
//!   denies unprivileged user namespaces (Ubuntu 24.04 and later: `kernel.apparmor_restrict_unprivileged_userns=1`,
//!   or `user.max_user_namespaces=0`) makes those tests print one `skipped:` line naming the cause and
//!   pass without running (`skip_without_userns`); `LIELLA_FORCE_NO_USERNS=1` forces that path.
//! * The parts that need a full subordinate ID mapping (`k_setuid`, `k_setgroups`) also need the setuid
//!   helpers `newuidmap`/`newgidmap` (package `uidmap`) on `PATH`. Without them that part prints one
//!   `skipped:` line naming the package and the test passes after its other parts (`skip_without_uidmap`);
//!   `LIELLA_FORCE_NO_UIDMAP=1` forces that path. With the helpers present nothing is skipped.
//! * Destructive primitives (`reboot`, module loading, `swapon`, `acct`, `iopl`, ...) are tested
//!   only on their error paths, with arguments chosen to fail before any effect.
//! * Where the running kernel predates a call (`fchmodat2`: 6.6, `map_shadow_stack`: 6.6), the
//!   test accepts `ENOSYS` and says so in its name of the outcome; nothing else is excused.

#![allow(clippy::arithmetic_side_effects, clippy::identity_op, clippy::drop_non_drop, clippy::bool_assert_comparison, clippy::erasing_op, clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing)]
#![allow(clippy::as_conversions, missing_docs)]

#[path = "../../core/tests/support/kernel.rs"]
mod kernel;
#[path = "../../core/tests/support/sync_trap.rs"]
mod sync_trap;
use sync_trap::skip_note;

use std::os::unix::fs::DirBuilderExt;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::time::Duration;

use liella_boundary::selftest::{self, ChildEnd};
use liella_boundary::sys::CSys;
use liella_kernel::errno::Errno;
use liella_kernel::fd::Fd;
use liella_kernel::machine::Machine;
use liella_kernel::mem::{IoVec, IoVecMut, KPtrArray, MMsgHdr, MMsgHdrMut, MsgHdr, MsgHdrMut, RegionKind, RemoteIoVec};
use liella_kernel::ops::*;
use liella_kernel::path::{KPathBuf, KStr};
use liella_kernel::sys::{Disposition, FsValue, FutexArg, Sys};
use liella_kernel::uapi::*;
use liella_kernel::{auxv, vdso};

const K: CSys = CSys;

// ------------------------------------------------------------------------------ helpers
//
// Two hazards of running kernel tests as threads of one process, both handled below:
// * a forked child copies every open descriptor, so "the last reference was closed" checks
//   (end of file, POLLHUP, EPIPE, ECONNREFUSED) run inside single-threaded children;
// * a closed descriptor number can be reused at once by another thread, so "closed
//   descriptor" checks also run in children, or use numbers beyond any table (999,999).

/// A NUL-terminated path.
fn kp(s: &str) -> KPathBuf {
    KPathBuf::new(s.as_bytes()).unwrap()
}

/// A scratch directory, removed on drop.
struct Tmp(String);

impl Tmp {
    fn new(tag: &str) -> Tmp {
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        // Under CARGO_TARGET_TMPDIR (the project's rule for test scratch), not /tmp: the
        // isolated gate gives /tmp its own tmpfs. Any file system works (k_preadv2 names the one
        // capability that differs, docs/testing-filesystems.md).
        let base = std::env::var("LIELLA_TEST_TMP").unwrap_or_else(|_| env!("CARGO_TARGET_TMPDIR").into());
        let dir = format!("{base}/liella-{}-{tag}-{n}", std::process::id());
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Tmp(dir)
    }
    /// A scratch directory with a SHORT path, for Unix-domain sockets: a socket's path must fit
    /// `sun_path` (108 bytes) and the checkout (so `CARGO_TARGET_TMPDIR`) may be anywhere.
    fn sock(tag: &str) -> Tmp {
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = format!("/tmp/liella-{}-{tag}-{n}", std::process::id());
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
        assert!(dir.len() < 80, "socket directory {dir} leaves too little of sun_path");
        Tmp(dir)
    }
    fn path(&self, name: &str) -> KPathBuf {
        kp(&format!("{}/{name}", self.0))
    }
    fn dir(&self) -> KPathBuf {
        kp(&self.0)
    }
    fn s(&self, name: &str) -> String {
        format!("{}/{name}", self.0)
    }
    /// A file with `content`, opened read-write.
    fn file(&self, name: &str, content: &[u8]) -> Fd {
        let fd = K.openat(Fd::CWD, self.path(name).as_kstr(), O_RDWR | O_CREAT | O_TRUNC | O_CLOEXEC, 0o600).unwrap();
        assert_eq!(K.write(fd, content).unwrap(), content.len());
        assert_eq!(K.lseek(fd, 0, SEEK_SET).unwrap(), 0);
        fd
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn close(fd: Fd) {
    K.close(fd).unwrap();
}

/// A memory file (memfd) with `content`, opened read-write at offset 0. For the durability calls
/// (`fsync`, `fdatasync`, `syncfs`, `msync`, `sync_file_range`): on the target file system they wait
/// for the journal and with it every dirty page of a parallel build (GATE-FSYNC: ~80 s each under
/// gate load). What these tests check - the call reaches the kernel, its result and its errors - is
/// the same on any regular file; a memfd's sync returns at once.
fn mem_file(name: &str, content: &[u8]) -> Fd {
    let fd = K.memfd_create(kp(name).as_kstr(), MFD_CLOEXEC).unwrap();
    assert_eq!(K.write(fd, content).unwrap(), content.len());
    assert_eq!(K.lseek(fd, 0, SEEK_SET).unwrap(), 0);
    fd
}

/// The status of a forked child whose call failed where the test needed it to succeed.
const CHILD_FAILED: i32 = 120;

/// In a forked child: the value, or the end of the child with [`CHILD_FAILED`]. Never a panic:
/// the child of this multithreaded harness may hold a copy of a lock another test thread had at
/// the fork (the allocator's, standard error's), and a panic formats and prints through them,
/// which hung a child for 28 minutes once (2026-10-04).
trait OrExit<T> {
    fn or_exit(self) -> T;
}

impl<T, E> OrExit<T> for Result<T, E> {
    fn or_exit(self) -> T {
        match self {
            Ok(v) => v,
            Err(_) => CSys.exit_group(CHILD_FAILED),
        }
    }
}

impl<T> OrExit<T> for Option<T> {
    fn or_exit(self) -> T {
        match self {
            Some(v) => v,
            None => CSys.exit_group(CHILD_FAILED),
        }
    }
}

/// Run `body` in a forked child and require exit status 0. The child has only the forking
/// thread, and other test threads may have held allocator or stdio locks at the fork: bodies
/// must not allocate (stack buffers, statics and `KPathBuf` only) and must not panic (`or_exit`).
fn in_child(body: fn(&CSys) -> i32) {
    assert_eq!(selftest::in_child(body).unwrap(), ChildEnd::Exited(0));
}

fn err<T: std::fmt::Debug>(r: Result<T, Errno>) -> Errno {
    r.expect_err("expected the call to fail")
}

/// Write a whole file with `Sys` (no allocation: usable in forked children).
fn put(sys: &CSys, path: &str, data: &[u8]) -> Result<(), Errno> {
    let p = KPathBuf::new(path.as_bytes())?;
    let fd = sys.openat(Fd::CWD, p.as_kstr(), O_WRONLY | O_CLOEXEC, 0)?;
    let r = sys.write(fd, data);
    let _ = sys.close(fd);
    r.map(|_| ())
}

/// Format `n` in decimal into `buf`, returning the used prefix (no allocation).
fn dec(mut n: u32, buf: &mut [u8; 16]) -> &[u8] {
    let mut i = buf.len();
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    &buf[i..]
}

/// In a forked child: become root of a new user namespace (plus the extra namespaces in
/// `flags`), mapping the current uid/gid to 0.
fn become_ns_root(sys: &CSys, flags: u64) -> Result<(), Errno> {
    let (uid, gid) = (sys.getuid(), sys.getgid());
    sys.unshare(CLONE_NEWUSER | flags)?;
    put(sys, "/proc/self/setgroups", b"deny")?;
    let mut line = [0u8; 48];
    for (file, id) in [("/proc/self/uid_map", uid), ("/proc/self/gid_map", gid)] {
        let mut d = [0u8; 16];
        let digits = dec(id, &mut d);
        let n = 2 + digits.len() + 3;
        line[..2].copy_from_slice(b"0 ");
        line[2..2 + digits.len()].copy_from_slice(digits);
        line[2 + digits.len()..n].copy_from_slice(b" 1\n");
        put(sys, file, &line[..n])?;
    }
    Ok(())
}

/// Whether this host lets an unprivileged process make a user namespace and write its uid and gid
/// maps (the privileged success paths of these tests run as root of such a namespace). Probed
/// once, in a forked child. Hosts that restrict it (Ubuntu 24.04 and later:
/// `sysctl kernel.apparmor_restrict_unprivileged_userns=1`, or `user.max_user_namespaces=0`) deny the
/// `unshare`. `LIELLA_FORCE_NO_USERNS=1` makes the probe report "denied" (to exercise the skip path
/// on a host where namespaces work).
fn userns_usable() -> bool {
    static USABLE: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *USABLE.get_or_init(|| {
        if std::env::var_os("LIELLA_FORCE_NO_USERNS").is_some_and(|v| v == "1") {
            return false;
        }
        matches!(selftest::in_child(|k| i32::from(become_ns_root(k, 0).is_err())), Ok(ChildEnd::Exited(0)))
    })
}

/// True (after one line naming the cause) when `test` needs a user namespace this host denies;
/// the test then returns without running. A namespace that works never skips, and a failure
/// inside a working one still fails the test.
fn skip_without_userns(test: &str) -> bool {
    if userns_usable() {
        return false;
    }
    skip_note::skipped(&format!(
        "skipped: {test} needs an unprivileged user namespace and this host denies it (on Ubuntu 24.04 and later: sysctl kernel.apparmor_restrict_unprivileged_userns=1; also user.max_user_namespaces=0)"
    ));
    true
}

/// Whether `name` is an executable file in a directory of `PATH`.
fn on_path(name: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::env::var_os("PATH").is_some_and(|p| {
        std::env::split_paths(&p).any(|d| {
            std::fs::metadata(d.join(name)).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
    })
}

/// True (after one line naming the package) when the setuid helpers `in_mapped_userns` needs are
/// missing from this host; the caller then returns without running the mapped part. Helpers that
/// exist never skip, and a failure while they run still fails the test. `LIELLA_FORCE_NO_UIDMAP=1`
/// reports them missing (to exercise the skip path on a host that has them).
fn skip_without_uidmap(test: &str) -> bool {
    let forced = std::env::var_os("LIELLA_FORCE_NO_UIDMAP").is_some_and(|v| v == "1");
    if !forced && on_path("newuidmap") && on_path("newgidmap") {
        return false;
    }
    skip_note::skipped(&format!(
        "skipped: {test} (mapped-ID part) needs the setuid helpers newuidmap and newgidmap (install the uidmap package)"
    ));
    true
}

/// The probe a re-executed test binary was started for (`LIELLA_PROBE`), if any.
fn probe() -> Option<String> {
    std::env::var("LIELLA_PROBE").ok()
}

/// Re-execute this test binary to run only `test`, with `LIELLA_PROBE=probe` and extra
/// environment, and return how it ended. The probe side ends with `exit_group`.
fn run_probe(test: &str, probe: &str, extra_env: &[&str], argv0: &str) -> ChildEnd {
    let exe = std::fs::read_link("/proc/self/exe").unwrap();
    let exe = kp(exe.to_str().unwrap());
    let args: Vec<KPathBuf> = [argv0, test, "--exact", "--test-threads=1", "-q"].iter().map(|s| kp(s)).collect();
    let mut env: Vec<KPathBuf> = vec![kp(&format!("LIELLA_PROBE={probe}"))];
    env.extend(extra_env.iter().map(|s| kp(s)));
    if let Ok(path) = std::env::var("PATH") {
        env.push(kp(&format!("PATH={path}")));
    }
    let a: Vec<KStr<'_>> = args.iter().map(|p| p.as_kstr()).collect();
    let e: Vec<KStr<'_>> = env.iter().map(|p| p.as_kstr()).collect();
    let (mut sa, mut se) = ([0usize; 16], [0usize; 16]);
    let argv = KPtrArray::build(&a, &mut sa).unwrap();
    let envp = KPtrArray::build(&e, &mut se).unwrap();
    selftest::exec_child(exe.as_kstr(), &argv, &envp).unwrap()
}

/// A timespec of `ms` milliseconds.
fn ms(ms: i64) -> Timespec {
    Timespec { sec: ms / 1000, nsec: (ms % 1000) * 1_000_000 }
}

fn sigset(sigs: &[i32]) -> SigSet {
    sigs.iter().fold(SigSet::empty(), |s, &g| s.with(g))
}

/// Block `sigs` in the calling thread (for tests that direct signals at themselves).
fn block(sys: &CSys, sigs: &[i32]) {
    sys.sigprocmask(SIG_BLOCK, Some(&sigset(sigs))).unwrap();
}

/// An `AF_UNIX` address for a path (sun_family + path + NUL).
fn unix_addr(path: &str) -> Vec<u8> {
    let mut v = (AF_UNIX as u16).to_ne_bytes().to_vec();
    v.extend_from_slice(path.as_bytes());
    v.push(0);
    v
}

/// An `AF_INET` loopback address with port `port` (network byte order).
fn inet_loopback(port: u16) -> Vec<u8> {
    let mut v = (AF_INET as u16).to_ne_bytes().to_vec();
    v.extend_from_slice(&port.to_be_bytes());
    v.extend_from_slice(&[127, 0, 0, 1]);
    v.extend_from_slice(&[0; 8]);
    v
}

/// Run `post` in a forked child that is root of a new user namespace whose uid and gid maps
/// were written by the setuid helpers `newuidmap`/`newgidmap` (package uidmap): 0 maps to the
/// caller, 1..=65536 to the caller's subordinate range, and `setgroups` stays allowed.
fn in_mapped_userns(post: fn(&CSys) -> i32) -> ChildEnd {
    let (uid, gid) = (K.getuid().to_string(), K.getgid().to_string());
    let sub = |file: &str| -> String {
        let me = std::env::var("USER").unwrap_or_default();
        let text = std::fs::read_to_string(file).unwrap_or_default();
        text.lines()
            .find_map(|l| {
                let f: Vec<&str> = l.split(':').collect();
                (f.len() == 3 && (f[0] == me || f[0] == K.getuid().to_string())).then(|| f[1].to_string())
            })
            .expect("no subordinate ID range for this user (see /etc/subuid, /etc/subgid)")
    };
    let (subu, subg) = (sub("/etc/subuid"), sub("/etc/subgid"));
    selftest::in_child_synced(
        |k| i32::from(k.unshare(CLONE_NEWUSER).is_err()),
        post,
        |pid| {
            for (tool, id, base) in [("newuidmap", &uid, &subu), ("newgidmap", &gid, &subg)] {
                let st = std::process::Command::new(tool)
                    .args([pid.to_string().as_str(), "0", id, "1", "1", base, "65536"])
                    .status()
                    .unwrap_or_else(|e| panic!("{tool}: {e} (install the uidmap package)"));
                assert!(st.success(), "{tool} failed");
            }
        },
    )
    .unwrap()
}

// ------------------------------------------------------------------------------ process lifecycle

#[test]
fn k_execve() {
    if probe().as_deref() == Some("execve") {
        // Probe side: argv[0] and the environment arrived exactly as passed.
        let ok = std::env::args().next().as_deref() == Some("liella-argv0")
            && std::env::var("LIELLA_EXTRA").as_deref() == Ok("x=y z");
        K.exit_group(if ok { 42 } else { 1 });
    }
    assert_eq!(run_probe("k_execve", "execve", &["LIELLA_EXTRA=x=y z"], "liella-argv0"), ChildEnd::Exited(42));
    // Errors: a missing file, and a directory (not executable).
    let (mut sa, mut se) = ([0usize; 2], [0usize; 1]);
    let a0 = kp("x");
    let argv = KPtrArray::build(&[a0.as_kstr()], &mut sa).unwrap();
    let envp = KPtrArray::build(&[], &mut se).unwrap();
    assert_eq!(err(K.execve(kp("/nonexistent/liella").as_kstr(), &argv, &envp)), Errno::ENOENT);
    assert_eq!(err(K.execve(kp("/").as_kstr(), &argv, &envp)), Errno::EACCES);
    // KPtrArray needs room for the terminator.
    let mut tiny = [0usize; 1];
    assert_eq!(KPtrArray::build(&[a0.as_kstr()], &mut tiny).unwrap_err(), Errno::E2BIG);
}

#[test]
fn k_execveat() {
    if probe().as_deref() == Some("execveat") {
        K.exit_group(if std::env::args().next().as_deref() == Some("via-execveat") { 43 } else { 1 });
    }
    // Success through an O_PATH descriptor and AT_EMPTY_PATH, in a child. The descriptor is
    // opened before the fork so the child needs no allocation.
    static EXE_FD: AtomicU32 = AtomicU32::new(0);
    let exe = std::fs::read_link("/proc/self/exe").unwrap();
    let fd = K.openat(Fd::CWD, kp(exe.to_str().unwrap()).as_kstr(), O_PATH | O_CLOEXEC, 0).unwrap();
    EXE_FD.store(fd.raw() as u32, Ordering::SeqCst);
    assert_eq!(selftest::in_child(|k| {
        let args = [kp("via-execveat"), kp("k_execveat"), kp("--exact"), kp("--test-threads=1"), kp("-q")];
        let env = [kp("LIELLA_PROBE=execveat")];
        let a = [args[0].as_kstr(), args[1].as_kstr(), args[2].as_kstr(), args[3].as_kstr(), args[4].as_kstr()];
        let e = [env[0].as_kstr()];
        let (mut sa, mut se) = ([0usize; 8], [0usize; 4]);
        let argv = KPtrArray::build(&a, &mut sa).or_exit();
        let envp = KPtrArray::build(&e, &mut se).or_exit();
        let null = k.openat(Fd::CWD, kp("/dev/null").as_kstr(), O_WRONLY, 0).or_exit();
        k.dup2(null, Fd::STDOUT).or_exit();
        let fd = Fd::new(EXE_FD.load(Ordering::SeqCst) as i32);
        let _ = k.execveat(fd, kp("").as_kstr(), &argv, &envp, AT_EMPTY_PATH);
        2
    }).unwrap(), ChildEnd::Exited(43));
    close(fd);
    // Errors: a bad descriptor with a relative path; an empty path without AT_EMPTY_PATH.
    let (mut sa, mut se) = ([0usize; 1], [0usize; 1]);
    let argv = KPtrArray::build(&[], &mut sa).unwrap();
    let envp = KPtrArray::build(&[], &mut se).unwrap();
    assert_eq!(err(K.execveat(Fd::new(-1), kp("rel").as_kstr(), &argv, &envp, 0)), Errno::EBADF);
    assert_eq!(err(K.execveat(Fd::CWD, kp("").as_kstr(), &argv, &envp, 0)), Errno::ENOENT);
}

#[test]
fn k_exit() {
    // `exit` ends the calling thread; in a single-threaded child that ends the process with
    // the given status (low 8 bits).
    assert_eq!(selftest::in_child_thread_exit(7).unwrap(), ChildEnd::Exited(7));
    assert_eq!(selftest::in_child_thread_exit(0x1ff).unwrap(), ChildEnd::Exited(0xff));
}

#[test]
fn k_exit_group() {
    assert_eq!(selftest::in_child(|k| k.exit_group(3)).unwrap(), ChildEnd::Exited(3));
    assert_eq!(selftest::in_child(|k| k.exit_group(256 + 9)).or_exit(), ChildEnd::Exited(9));
}

#[test]
fn k_clone() {
    // Fork-style clone: the child is a new process whose parent is us.
    assert_eq!(selftest::in_child(|k| if k.getppid() > 0 && k.getpid() != k.getppid() { 0 } else { 1 }).unwrap(), ChildEnd::Exited(0));
    let pid = selftest::spawn_exiting(5).unwrap();
    assert!(pid > 0 && pid != K.getpid());
    assert_eq!(K.wait4(pid, 0, None).unwrap(), (pid, 5 << 8));
}

#[test]
fn k_clone3() {
    let (pid, pidfd) = selftest::spawn_exiting_pidfd(6).unwrap();
    assert!(pid > 0);
    assert!(pidfd >= 0, "CLONE_PIDFD returned a descriptor");
    let pidfd = Fd::new(pidfd);
    // The pidfd becomes readable when the child exits.
    let mut fds = [PollFd { fd: pidfd.raw(), events: POLLIN, revents: 0 }];
    assert_eq!(K.ppoll(&mut fds, Some(&mut ms(5000)), None).unwrap(), 1);
    let mut info = SigInfo::default();
    K.waitid(P_PIDFD, pidfd.raw(), &mut info, WEXITED, None).unwrap();
    assert_eq!((info.signo, info.code, info.pid(), info.status()), (SIGCHLD, CLD_EXITED, pid, 6));
    close(pidfd);
}

#[test]
fn k_wait4() {
    let pid = selftest::spawn_exiting(11).unwrap();
    let mut ru = Rusage::default();
    let (got, status) = K.wait4(pid, 0, Some(&mut ru)).unwrap();
    assert_eq!((got, selftest::decode_status(status)), (pid, ChildEnd::Exited(11)));
    // A child killed by a signal.
    let p2 = selftest::spawn_pausing().unwrap();
    K.kill(p2, SIGKILL).unwrap();
    let (_, st2) = K.wait4(p2, 0, None).unwrap();
    assert_eq!(selftest::decode_status(st2), ChildEnd::Signaled(SIGKILL));
    // WNOHANG with a live child: 0. No such child: ECHILD.
    let p3 = selftest::spawn_pausing().unwrap();
    assert_eq!(K.wait4(p3, WNOHANG, None).unwrap().0, 0);
    K.kill(p3, SIGKILL).unwrap();
    K.wait4(p3, 0, None).unwrap();
    assert_eq!(err(K.wait4(p3, 0, None)), Errno::ECHILD);
    assert_eq!(err(K.wait4(-1, WNOWAIT, None)), Errno::EINVAL, "WNOWAIT is waitid-only");
}

#[test]
fn k_waitid() {
    let pid = selftest::spawn_exiting(12).unwrap();
    let mut info = SigInfo::default();
    // WNOWAIT leaves the child waitable.
    K.waitid(P_PID, pid, &mut info, WEXITED | WNOWAIT, None).unwrap();
    assert_eq!((info.pid(), info.status(), info.code), (pid, 12, CLD_EXITED));
    let mut info2 = SigInfo::default();
    K.waitid(P_PID, pid, &mut info2, WEXITED, None).unwrap();
    assert_eq!(info2.pid(), pid);
    assert_eq!(err(K.waitid(P_PID, pid, &mut info2, WEXITED, None)), Errno::ECHILD);
    assert_eq!(err(K.waitid(P_PID, pid, &mut info2, 0, None)), Errno::EINVAL);
    assert_eq!(err(K.waitid(99, 0, &mut info2, WEXITED, None)), Errno::EINVAL);
}

#[test]
fn k_getpid() {
    assert_eq!(K.getpid() as u32, std::process::id());
    // A forked child has its own PID, distinct from its parent's.
    in_child(|k| i32::from(k.getpid() == k.getppid() || k.getpid() <= 0));
}

#[test]
fn k_getppid() {
    let me = K.getpid();
    assert!(K.getppid() > 0);
    // The child's parent is us: the child reports whether getppid() equals the pid it saw
    // before forking (inherited through its copy of `EXPECT`).
    static EXPECT: AtomicU32 = AtomicU32::new(0);
    EXPECT.store(me as u32, Ordering::SeqCst);
    in_child(|k| i32::from(k.getppid() as u32 != EXPECT.load(Ordering::SeqCst)));
}

#[test]
fn k_gettid() {
    let main = K.gettid();
    assert!(main > 0);
    let other = std::thread::spawn(|| K.gettid()).join().unwrap();
    assert_ne!(main, other);
    // Every thread of the process shares the PID.
    assert_eq!(std::thread::spawn(|| K.getpid()).join().unwrap(), K.getpid());
}

#[test]
fn k_set_tid_address() {
    // In a child (it replaces the clear-child-tid word of the calling thread): returns the TID.
    static WORD: AtomicU32 = AtomicU32::new(0);
    in_child(|k| i32::from(k.set_tid_address(&WORD) != Ok(k.gettid())));
}

// ------------------------------------------------------------------------------ process descriptors

#[test]
fn k_pidfd_open() {
    let pid = selftest::spawn_pausing().unwrap();
    let pidfd = K.pidfd_open(pid, 0).unwrap();
    assert_eq!(K.fcntl_int(pidfd, FcntlInt::GetFd, 0).unwrap() & FD_CLOEXEC, FD_CLOEXEC, "pidfds are close-on-exec");
    K.kill(pid, SIGKILL).unwrap();
    K.wait4(pid, 0, None).unwrap();
    close(pidfd);
    // The reaped PID no longer exists; flags are validated.
    assert_eq!(err(K.pidfd_open(pid, 0)), Errno::ESRCH);
    assert_eq!(err(K.pidfd_open(K.getpid(), 0x1)), Errno::EINVAL);
    assert_eq!(err(K.pidfd_open(-1, 0)), Errno::EINVAL);
}

#[test]
fn k_pidfd_getfd() {
    // Take a duplicate of one of our own descriptors through our own pidfd.
    let t = Tmp::new("pidfd_getfd");
    let fd = t.file("f", b"abc");
    let me = K.pidfd_open(K.getpid(), 0).unwrap();
    let dup = K.pidfd_getfd(me, fd.raw(), 0).unwrap();
    assert_ne!(dup, fd);
    let mut b = [0u8; 3];
    assert_eq!(K.read(dup, &mut b).unwrap(), 3);
    assert_eq!(&b, b"abc");
    assert_eq!(err(K.pidfd_getfd(me, 999_999, 0)), Errno::EBADF);
    assert_eq!(err(K.pidfd_getfd(me, fd.raw(), 1)), Errno::EINVAL);
    for f in [dup, me, fd] {
        close(f);
    }
}

#[test]
fn k_pidfd_send_signal() {
    let pid = selftest::spawn_pausing().unwrap();
    let pidfd = K.pidfd_open(pid, 0).unwrap();
    K.pidfd_send_signal(pidfd, SIGTERM, None, 0).unwrap();
    let (_, status) = K.wait4(pid, 0, None).unwrap();
    assert_eq!(selftest::decode_status(status), ChildEnd::Signaled(SIGTERM));
    // The process is gone (reaped): ESRCH. Bad flags: EINVAL. Not a pidfd: EBADF.
    assert_eq!(err(K.pidfd_send_signal(pidfd, SIGTERM, None, 0)), Errno::ESRCH);
    assert_eq!(err(K.pidfd_send_signal(pidfd, SIGTERM, None, 1)), Errno::EINVAL);
    close(pidfd);
    assert_eq!(err(K.pidfd_send_signal(Fd::STDIN, 0, None, 0)), Errno::EBADF);
}

// ------------------------------------------------------------------------------ credentials

#[test]
fn k_getuid() {
    if skip_without_userns("k_getuid") {
        return;
    }
    let (r, _, _) = K.getresuid().unwrap();
    assert_eq!(K.getuid(), r);
    in_child(|k| {
        become_ns_root(k, 0).or_exit();
        i32::from(k.getuid() != 0)
    });
}

#[test]
fn k_geteuid() {
    let (_, e, _) = K.getresuid().unwrap();
    assert_eq!(K.geteuid(), e);
}

#[test]
fn k_getgid() {
    let (r, _, _) = K.getresgid().unwrap();
    assert_eq!(K.getgid(), r);
}

#[test]
fn k_getegid() {
    let (_, e, _) = K.getresgid().unwrap();
    assert_eq!(K.getegid(), e);
}

#[test]
fn k_getresuid() {
    let (r, e, s) = K.getresuid().unwrap();
    assert_eq!((r, e), (K.getuid(), K.geteuid()));
    assert_eq!(s, e, "saved set-user-ID equals the effective ID of a non-setuid program");
}

#[test]
fn k_getresgid() {
    let (r, e, s) = K.getresgid().unwrap();
    assert_eq!((r, e, s), (K.getgid(), K.getegid(), K.getegid()));
}

#[test]
fn k_setuid() {
    if skip_without_userns("k_setuid") {
        return;
    }
    // Unprivileged: only the current IDs are allowed.
    in_child(|k| {
        let me = k.getuid();
        if k.setuid(me).is_err() || k.setuid(me.wrapping_add(1)) != Err(Errno::EPERM) {
            return 1;
        }
        // As namespace root: any mapped ID; unmapped IDs are EINVAL.
        become_ns_root(k, 0).or_exit();
        if k.setuid(0).is_err() || k.getresuid() != Ok((0, 0, 0)) {
            return 2;
        }
        i32::from(k.setuid(12345) != Err(Errno::EINVAL)) * 3
    });
    // With a full subordinate mapping: root may become any mapped ID, then cannot return.
    if skip_without_uidmap("k_setuid") {
        return;
    }
    assert_eq!(in_mapped_userns(|k| {
        if k.setuid(1000).is_err() || k.getresuid() != Ok((1000, 1000, 1000)) {
            return 1;
        }
        i32::from(k.setuid(0) != Err(Errno::EPERM)) * 2
    }), ChildEnd::Exited(0));
}

#[test]
fn k_setgid() {
    if skip_without_userns("k_setgid") {
        return;
    }
    in_child(|k| {
        let me = k.getgid();
        if k.setgid(me).is_err() || k.setgid(me.wrapping_add(1)) != Err(Errno::EPERM) {
            return 1;
        }
        become_ns_root(k, 0).or_exit();
        if k.setgid(0).is_err() || k.getresgid() != Ok((0, 0, 0)) {
            return 2;
        }
        i32::from(k.setgid(12345) != Err(Errno::EINVAL)) * 3
    });
}

#[test]
fn k_setreuid() {
    if skip_without_userns("k_setreuid") {
        return;
    }
    in_child(|k| {
        let me = k.getuid();
        if k.setreuid(u32::MAX, me).is_err() || k.setreuid(me.wrapping_add(7), u32::MAX) != Err(Errno::EPERM) {
            return 1;
        }
        become_ns_root(k, 0).or_exit();
        i32::from(k.setreuid(0, 0).is_err() || k.getresuid() != Ok((0, 0, 0))) * 2
    });
}

#[test]
fn k_setregid() {
    if skip_without_userns("k_setregid") {
        return;
    }
    in_child(|k| {
        let me = k.getgid();
        if k.setregid(u32::MAX, me).is_err() || k.setregid(me.wrapping_add(7), u32::MAX) != Err(Errno::EPERM) {
            return 1;
        }
        become_ns_root(k, 0).or_exit();
        i32::from(k.setregid(0, 0).is_err() || k.getresgid() != Ok((0, 0, 0))) * 2
    });
}

#[test]
fn k_setresuid() {
    if skip_without_userns("k_setresuid") {
        return;
    }
    in_child(|k| {
        let me = k.getuid();
        if k.setresuid(u32::MAX, u32::MAX, u32::MAX).is_err() || k.setresuid(me, me, me.wrapping_add(1)) != Err(Errno::EPERM) {
            return 1;
        }
        become_ns_root(k, 0).or_exit();
        i32::from(k.setresuid(0, 0, 0).is_err() || k.getresuid() != Ok((0, 0, 0))) * 2
    });
}

#[test]
fn k_setresgid() {
    if skip_without_userns("k_setresgid") {
        return;
    }
    in_child(|k| {
        let me = k.getgid();
        if k.setresgid(u32::MAX, u32::MAX, u32::MAX).is_err() || k.setresgid(me, me, me.wrapping_add(1)) != Err(Errno::EPERM) {
            return 1;
        }
        become_ns_root(k, 0).or_exit();
        i32::from(k.setresgid(0, 0, 0).is_err() || k.getresgid() != Ok((0, 0, 0))) * 2
    });
}

#[test]
fn k_setfsuid() {
    // Returns the previous fsuid and never fails; an invalid request leaves it unchanged
    // (u32::MAX is a pure query, ⚑ KERN).
    in_child(|k| {
        let me = k.geteuid();
        let prev = k.setfsuid(u32::MAX);
        let again = k.setfsuid(me.wrapping_add(1)); // not permitted: unchanged
        i32::from(prev != me || again != me || k.setfsuid(u32::MAX) != me)
    });
}

#[test]
fn k_setfsgid() {
    in_child(|k| {
        let me = k.getegid();
        let prev = k.setfsgid(u32::MAX);
        let again = k.setfsgid(me.wrapping_add(1));
        i32::from(prev != me || again != me || k.setfsgid(u32::MAX) != me)
    });
}

#[test]
fn k_getgroups() {
    let n = K.getgroups(&mut []).unwrap();
    let mut list = vec![0u32; n];
    assert_eq!(K.getgroups(&mut list).unwrap(), n);
    if n > 1 {
        let mut short = vec![0u32; n - 1];
        assert_eq!(err(K.getgroups(&mut short)), Errno::EINVAL);
    }
}

#[test]
fn k_setgroups() {
    if skip_without_userns("k_setgroups") {
        return;
    }
    // Unprivileged: EPERM, even for the current list.
    let n = K.getgroups(&mut []).unwrap();
    let mut list = vec![0u32; n];
    K.getgroups(&mut list).unwrap();
    assert_eq!(err(K.setgroups(&list)), Errno::EPERM);
    // In a user namespace whose setgroups is denied (all an unprivileged process can map on
    // its own): still EPERM.
    in_child(|k| {
        become_ns_root(k, 0).or_exit();
        i32::from(k.setgroups(&[0]) != Err(Errno::EPERM))
    });
    // With a full subordinate mapping written by the setuid helpers (uidmap): success, and the
    // list reads back.
    if skip_without_uidmap("k_setgroups") {
        return;
    }
    assert_eq!(in_mapped_userns(|k| {
        if k.setgroups(&[0, 5, 7]).is_err() {
            return 1;
        }
        let mut back = [0u32; 8];
        i32::from(k.getgroups(&mut back) != Ok(3) || back[..3] != [0, 5, 7]) * 2
    }), ChildEnd::Exited(0));
    // Too many groups: EINVAL (NGROUPS_MAX is 65,536, ⚑ KERN), checked after the privilege
    // check, so only where setgroups is permitted.
    static BIG: [u32; 65_537] = [0; 65_537];
    assert_eq!(in_mapped_userns(|k| i32::from(k.setgroups(&BIG) != Err(Errno::EINVAL))), ChildEnd::Exited(0));
}

#[test]
fn k_capget() {
    if skip_without_userns("k_capget") {
        return;
    }
    let mut hdr = CapHeader { version: LINUX_CAPABILITY_VERSION_3, pid: 0 };
    let mut data = [CapData::default(); 2];
    K.capget(&mut hdr, Some(&mut data)).unwrap();
    // An unprivileged process has no effective capabilities in the initial namespace.
    assert_eq!(data[0].effective & data[0].permitted, data[0].effective);
    // Version probe: an unknown version is rewritten to the kernel's preferred one.
    // With no data buffer it is a pure probe and succeeds (⚑ KERN); with one, EINVAL.
    let mut probe = CapHeader { version: 0, pid: 0 };
    K.capget(&mut probe, None).unwrap();
    assert_eq!(probe.version, LINUX_CAPABILITY_VERSION_3);
    let mut probe = CapHeader { version: 0, pid: 0 };
    assert_eq!(err(K.capget(&mut probe, Some(&mut data))), Errno::EINVAL);
    assert_eq!(probe.version, LINUX_CAPABILITY_VERSION_3);
    // Root of a user namespace has every capability there.
    in_child(|k| {
        become_ns_root(k, 0).or_exit();
        let mut hdr = CapHeader { version: LINUX_CAPABILITY_VERSION_3, pid: 0 };
        let mut data = [CapData::default(); 2];
        k.capget(&mut hdr, Some(&mut data)).or_exit();
        i32::from(data[0].effective & (1 << 21) == 0) // CAP_SYS_ADMIN
    });
}

#[test]
fn k_capset() {
    if skip_without_userns("k_capset") {
        return;
    }
    in_child(|k| {
        become_ns_root(k, 0).or_exit();
        let mut hdr = CapHeader { version: LINUX_CAPABILITY_VERSION_3, pid: 0 };
        let mut data = [CapData::default(); 2];
        k.capget(&mut hdr, Some(&mut data)).or_exit();
        // Drop CAP_SYS_ADMIN from the effective set, then check it is gone.
        data[0].effective &= !(1 << 21);
        k.capset(&mut hdr, &data).or_exit();
        let mut after = [CapData::default(); 2];
        k.capget(&mut hdr, Some(&mut after)).or_exit();
        if after[0].effective & (1 << 21) != 0 {
            return 1;
        }
        // Raising a capability outside the permitted set: EPERM.
        data[0].permitted = 0;
        data[0].effective = 1;
        i32::from(k.capset(&mut hdr, &data) != Err(Errno::EPERM)) * 2
    });
    // Another process's capabilities cannot be set: EPERM.
    let mut hdr = CapHeader { version: LINUX_CAPABILITY_VERSION_3, pid: K.getppid() };
    assert_eq!(err(K.capset(&mut hdr, &[CapData::default(); 2])), Errno::EPERM);
}

// ------------------------------------------------------------------------------ sessions and groups

#[test]
fn k_setsid() {
    in_child(|k| {
        // A fresh child is not a group leader: it may start a session.
        let sid = k.setsid().or_exit();
        if sid != k.getpid() || k.getsid(0) != Ok(sid) || k.getpgrp() != sid {
            return 1;
        }
        // Now it is a group leader: EPERM.
        i32::from(k.setsid() != Err(Errno::EPERM)) * 2
    });
}

#[test]
fn k_getsid() {
    assert!(K.getsid(0).unwrap() > 0);
    assert_eq!(K.getsid(0).unwrap(), K.getsid(K.getpid()).unwrap());
    assert_eq!(err(K.getsid(i32::MAX)), Errno::ESRCH);
}

#[test]
fn k_setpgid() {
    in_child(|k| {
        // Become our own group leader, then move back is not allowed across sessions only.
        if k.setpgid(0, 0).is_err() || k.getpgrp() != k.getpid() {
            return 1;
        }
        if k.setpgid(-1, 0) != Err(Errno::EINVAL) {
            return 2;
        }
        i32::from(k.setpgid(i32::MAX, 0) != Err(Errno::ESRCH)) * 3
    });
}

#[test]
fn k_getpgid() {
    assert_eq!(K.getpgid(0).unwrap(), K.getpgrp());
    assert_eq!(err(K.getpgid(i32::MAX)), Errno::ESRCH);
}

#[test]
fn k_getpgrp() {
    assert_eq!(K.getpgrp(), K.getpgid(K.getpid()).unwrap());
}

// ------------------------------------------------------------------------------ signals

#[test]
fn k_rt_sigaction() {
    in_child(|k| {
        // Query, set SIG_IGN with a mask and flags, and read the same action back (layout of
        // the kernel's struct sigaction: handler, flags, restorer, mask).
        let old = k.sigaction_get(SIGUSR1).or_exit();
        if old.handler != SIG_DFL {
            return 1;
        }
        let mask = sigset(&[SIGUSR2, SIGHUP]);
        k.sigaction_set(SIGUSR1, Disposition::Ignore, SA_RESTART, mask).or_exit();
        let now = k.sigaction_get(SIGUSR1).or_exit();
        if now.handler != SIG_IGN || now.flags & SA_RESTART == 0 || now.mask != mask {
            return 2;
        }
        // An ignored signal is discarded.
        k.kill(k.getpid(), SIGUSR1).or_exit();
        if k.sigpending().or_exit().contains(SIGUSR1) {
            return 3;
        }
        // SIGKILL/SIGSTOP cannot be changed; 0 and 65 are not signals.
        for bad in [SIGKILL, SIGSTOP, 0, 65] {
            if k.sigaction_set(bad, Disposition::Ignore, 0, SigSet::empty()) != Err(Errno::EINVAL) {
                return 4;
            }
        }
        0
    });
}

#[test]
fn k_rt_sigprocmask() {
    // Per thread: safe to run in the test thread.
    let old = K.sigprocmask(SIG_BLOCK, Some(&sigset(&[SIGUSR2]))).unwrap();
    let now = K.sigprocmask(SIG_BLOCK, None).unwrap();
    assert!(now.contains(SIGUSR2));
    // SIGKILL and SIGSTOP are silently not blockable.
    K.sigprocmask(SIG_BLOCK, Some(&sigset(&[SIGKILL, SIGSTOP]))).unwrap();
    let now = K.sigprocmask(SIG_BLOCK, None).unwrap();
    assert!(!now.contains(SIGKILL) && !now.contains(SIGSTOP));
    K.sigprocmask(SIG_SETMASK, Some(&old)).unwrap();
    assert_eq!(K.sigprocmask(SIG_BLOCK, None).unwrap(), old);
    assert_eq!(err(K.sigprocmask(7, Some(&old))), Errno::EINVAL);
    // The mask is per thread: another thread starts with the creator's mask, then diverges.
    let t = std::thread::spawn(|| {
        K.sigprocmask(SIG_BLOCK, Some(&sigset(&[SIGWINCH]))).unwrap();
        K.sigprocmask(SIG_BLOCK, None).unwrap().contains(SIGWINCH)
    });
    assert!(t.join().unwrap());
    assert!(!K.sigprocmask(SIG_BLOCK, None).unwrap().contains(SIGWINCH) || old.contains(SIGWINCH));
}

#[test]
fn k_rt_sigreturn() {
    // rt_sigreturn is reached only from the restorer at the end of a handler: returning from
    // the round trip at all, with the handler run once, proves it restored the frame.
    in_child(|_| {
        let (count, sig) = selftest::signal_roundtrip(SIGUSR1).or_exit();
        i32::from(count != 1 || sig != SIGUSR1)
    });
}

#[test]
fn k_rt_sigsuspend() {
    in_child(|k| {
        // SIGUSR1 is blocked and made pending; suspending with an empty mask delivers it to
        // the handler, and the call fails with EINTR. On return the old mask is back.
        block(k, &[SIGUSR1]);
        let (r, count) = selftest::with_counting_handler(SIGUSR1, || {
            k.kill(k.getpid(), SIGUSR1).or_exit();
            k.sigsuspend(&SigSet::empty())
        })
        .or_exit();
        i32::from(r != Err(Errno::EINTR) || count != 1 || !k.sigprocmask(SIG_BLOCK, None).or_exit().contains(SIGUSR1))
    });
}

#[test]
fn k_rt_sigpending() {
    in_child(|k| {
        block(k, &[SIGUSR2]);
        if k.sigpending().or_exit().contains(SIGUSR2) {
            return 1;
        }
        k.tgkill(k.getpid(), k.gettid(), SIGUSR2).or_exit();
        if !k.sigpending().or_exit().contains(SIGUSR2) {
            return 2;
        }
        let got = k.sigtimedwait(&sigset(&[SIGUSR2]), None, Some(&ms(0))).or_exit();
        i32::from(got != SIGUSR2 || k.sigpending().or_exit().contains(SIGUSR2)) * 3
    });
}

#[test]
fn k_rt_sigtimedwait() {
    in_child(|k| {
        block(k, &[SIGUSR1]);
        // Timeout with nothing pending: EAGAIN.
        if k.sigtimedwait(&sigset(&[SIGUSR1]), None, Some(&ms(10))) != Err(Errno::EAGAIN) {
            return 1;
        }
        // A queued signal carries its value and sender.
        let info = SigInfo::queued(SIGUSR1, k.getpid(), k.getuid(), 0x5eed);
        k.sigqueueinfo(k.getpid(), SIGUSR1, &info).or_exit();
        let mut got = SigInfo::default();
        let sig = k.sigtimedwait(&sigset(&[SIGUSR1]), Some(&mut got), None).or_exit();
        if sig != SIGUSR1 || got.code != SI_QUEUE || got.value() != 0x5eed || got.pid() != k.getpid() {
            return 2;
        }
        // An invalid timeout: EINVAL.
        let bad = Timespec { sec: 0, nsec: 1_000_000_000 };
        i32::from(k.sigtimedwait(&sigset(&[SIGUSR1]), None, Some(&bad)) != Err(Errno::EINVAL)) * 3
    });
}

#[test]
fn k_rt_sigqueueinfo() {
    in_child(|k| {
        block(k, &[SIGRTMIN_KERNEL + 2]);
        // Real-time signals queue: two sends, two receipts, in order of sending.
        for v in [1u64, 2] {
            k.sigqueueinfo(k.getpid(), SIGRTMIN_KERNEL + 2, &SigInfo::queued(SIGRTMIN_KERNEL + 2, k.getpid(), k.getuid(), v)).or_exit();
        }
        let mut a = SigInfo::default();
        let mut b = SigInfo::default();
        k.sigtimedwait(&sigset(&[SIGRTMIN_KERNEL + 2]), Some(&mut a), Some(&ms(0))).or_exit();
        k.sigtimedwait(&sigset(&[SIGRTMIN_KERNEL + 2]), Some(&mut b), Some(&ms(0))).or_exit();
        if (a.value(), b.value()) != (1, 2) {
            return 1;
        }
        // Sending to another process with a kernel-reserved si_code (>= 0): EPERM.
        let mut fake = SigInfo::queued(SIGUSR1, 1, 0, 0);
        fake.code = SI_USER;
        if k.sigqueueinfo(k.getppid(), SIGUSR1, &fake) != Err(Errno::EPERM) {
            return 2;
        }
        i32::from(k.sigqueueinfo(i32::MAX, SIGUSR1, &SigInfo::queued(SIGUSR1, 0, 0, 0)) != Err(Errno::ESRCH)) * 3
    });
}

#[test]
fn k_rt_tgsigqueueinfo() {
    in_child(|k| {
        block(k, &[SIGUSR1]);
        let info = SigInfo::queued(SIGUSR1, k.getpid(), k.getuid(), 77);
        k.tgsigqueueinfo(k.getpid(), k.gettid(), SIGUSR1, &info).or_exit();
        let mut got = SigInfo::default();
        k.sigtimedwait(&sigset(&[SIGUSR1]), Some(&mut got), Some(&ms(0))).or_exit();
        if got.value() != 77 {
            return 1;
        }
        // A TID outside the thread group: ESRCH.
        i32::from(k.tgsigqueueinfo(k.getpid(), k.getppid(), SIGUSR1, &info) != Err(Errno::ESRCH)) * 2
    });
}

#[test]
fn k_kill() {
    let pid = selftest::spawn_pausing().unwrap();
    K.kill(pid, 0).unwrap(); // existence check
    K.kill(pid, SIGTERM).unwrap();
    let (_, st) = K.wait4(pid, 0, None).unwrap();
    assert_eq!(selftest::decode_status(st), ChildEnd::Signaled(SIGTERM));
    assert_eq!(err(K.kill(pid, 0)), Errno::ESRCH);
    assert_eq!(err(K.kill(K.getpid(), 65)), Errno::EINVAL);
    assert_eq!(err(K.kill(1, 0)), Errno::EPERM, "signalling init as a normal user");
}

#[test]
fn k_tgkill() {
    in_child(|k| {
        block(k, &[SIGUSR2]);
        k.tgkill(k.getpid(), k.gettid(), SIGUSR2).or_exit();
        if k.sigtimedwait(&sigset(&[SIGUSR2]), None, Some(&ms(0))) != Ok(SIGUSR2) {
            return 1;
        }
        if k.tgkill(k.getpid(), -1, SIGUSR2) != Err(Errno::EINVAL) {
            return 2;
        }
        i32::from(k.tgkill(k.getpid(), k.getppid(), 0) != Err(Errno::ESRCH)) * 3
    });
}

#[test]
fn k_sigaltstack() {
    in_child(|k| {
        // The query reports whatever stack is installed (the Rust runtime installs one per
        // thread); disabling returns that previous state, after which SS_DISABLE is reported,
        // and disabling again is a no-op success.
        let cur = k.sigaltstack_get().or_exit();
        let prev = k.sigaltstack_disable().or_exit();
        if prev != cur {
            return 1;
        }
        if k.sigaltstack_get().or_exit().flags & SS_DISABLE == 0 {
            return 2;
        }
        i32::from(k.sigaltstack_disable().or_exit().flags & SS_DISABLE == 0) * 3
    });
    // Installing a stack is `raw` (it names memory the kernel will write): exercised through
    // the handler round trip on an alternate stack.
    in_child(|_| i32::from(selftest::altstack_roundtrip(SIGUSR1) != Ok(true)));
}

#[test]
fn k_signalfd4() {
    in_child(|k| {
        block(k, &[SIGUSR1]);
        let sfd = k.signalfd(None, &sigset(&[SIGUSR1]), SFD_NONBLOCK_CLOEXEC).or_exit();
        let mut rec = [0u8; 128];
        if k.read(sfd, &mut rec) != Err(Errno::EAGAIN) {
            return 1;
        }
        k.tgkill(k.getpid(), k.gettid(), SIGUSR1).or_exit();
        if k.read(sfd, &mut rec) != Ok(128) {
            return 2;
        }
        let signo = u32::from_ne_bytes([rec[0], rec[1], rec[2], rec[3]]);
        let pid = u32::from_ne_bytes([rec[12], rec[13], rec[14], rec[15]]);
        if signo != SIGUSR1 as u32 || pid != k.getpid() as u32 {
            return 3;
        }
        // Updating the mask of an existing descriptor; a non-signalfd descriptor: EINVAL.
        if k.signalfd(Some(sfd), &sigset(&[SIGUSR2]), 0) != Ok(sfd) {
            return 4;
        }
        i32::from(k.signalfd(Some(Fd::STDIN), &sigset(&[SIGUSR2]), 0) != Err(Errno::EINVAL)) * 5
    });
}

const SFD_NONBLOCK_CLOEXEC: u32 = O_NONBLOCK | O_CLOEXEC;

#[test]
fn k_pause() {
    // pause returns only when a handler runs: EINTR. The alarm fires into a counting handler.
    in_child(|k| {
        let (r, count) = selftest::with_counting_handler(SIGALRM, || {
            k.alarm(1).or_exit();
            k.pause()
        })
        .or_exit();
        i32::from(r != Err(Errno::EINTR) || count != 1)
    });
}


// ------------------------------------------------------------------------------ memory

const RW: u32 = PROT_READ | PROT_WRITE;

#[test]
fn k_brk() {
    // `brk` is `raw` only (it moves memory safe code may not own; the runtime never uses it,
    // D-3.7). Its kernel behaviour is checked through the raw wrapper in a child: a query
    // returns the current break, and an impossible request leaves it unchanged (⚑ KERN).
    in_child(|_| {
        let cur = selftest::brk_query();
        i32::from(cur == 0 || selftest::brk_try(usize::MAX - 4095) != cur)
    });
}

#[test]
fn k_mmap() {
    let mut r = K.mmap(0, 10_000, RW, MAP_PRIVATE, None, 0).unwrap();
    assert_eq!(r.len(), 12_288, "length rounded to pages");
    assert_eq!(r.addr() % 4096, 0);
    assert_eq!(r.kind(), RegionKind::AnonPrivate);
    {
        let bytes = K.region_bytes(&mut r).unwrap();
        assert!(bytes.iter().all(|&b| b == 0), "anonymous memory is zero-filled");
        bytes[12_287] = 7;
        assert_eq!(bytes[12_287], 7);
    }
    K.munmap(r).unwrap();
    // A file mapping sees the file's bytes (through shared words; never as a byte slice).
    let t = Tmp::new("mmap");
    let fd = t.file("f", &[1, 2, 3, 4, 5, 6, 7, 8]);
    let fr = K.mmap(0, 8, PROT_READ, MAP_SHARED, Some(fd), 0).unwrap();
    assert_eq!(fr.kind(), RegionKind::File);
    assert!(K.region_words(&fr).is_none() && fr.prot() == PROT_READ);
    K.munmap(fr).unwrap();
    // Errors: MAP_FIXED refused by the boundary; zero length; bad descriptor; neither
    // private nor shared; unaligned offset.
    assert_eq!(err(K.mmap(0x1000_0000, 4096, RW, MAP_PRIVATE | MAP_FIXED, None, 0)), Errno::EINVAL);
    assert_eq!(err(K.mmap(0, 0, RW, MAP_PRIVATE, None, 0)), Errno::EINVAL);
    assert_eq!(err(K.mmap(0, 4096, PROT_READ, MAP_PRIVATE, Some(Fd::new(-1)), 0)), Errno::EBADF);
    assert_eq!(err(K.mmap(0, 4096, RW, 0, None, 0)), Errno::EINVAL);
    assert_eq!(err(K.mmap(0, 4096, PROT_READ, MAP_PRIVATE, Some(fd), 1)), Errno::EINVAL);
    // Writable shared mapping of a read-only descriptor: EACCES.
    let ro = K.openat(Fd::CWD, t.path("f").as_kstr(), O_RDONLY | O_CLOEXEC, 0).unwrap();
    assert_eq!(err(K.mmap(0, 4096, RW, MAP_SHARED, Some(ro), 0)), Errno::EACCES);
    // MAP_FIXED_NOREPLACE over an existing mapping: EEXIST.
    let a = K.mmap(0, 4096, RW, MAP_PRIVATE, None, 0).unwrap();
    assert_eq!(err(K.mmap(a.addr(), 4096, RW, MAP_PRIVATE | MAP_FIXED_NOREPLACE, None, 0)), Errno::EEXIST);
    K.munmap(a).unwrap();
    close(ro);
    close(fd);
}

#[test]
fn k_munmap() {
    let r = K.mmap(0, 8192, RW, MAP_PRIVATE, None, 0).unwrap();
    let (lo, hi) = r.split_at(4096).unwrap();
    let hi_addr = hi.addr();
    K.munmap(lo).unwrap();
    // The upper half is still mapped: mincore succeeds on it and fails where the lower was.
    let mut v = [0u8; 1];
    K.mincore(&hi, &mut v).unwrap();
    assert_eq!(hi.addr(), hi_addr);
    K.munmap(hi).unwrap();
    // Unmapping a range that is no longer mapped is not an error (⚑ KERN); unaligned is.
    let again = K.mmap(0, 4096, RW, MAP_PRIVATE, None, 0).unwrap();
    K.munmap(again).unwrap();
    assert!(K.mmap(0, 4096, RW, MAP_PRIVATE, None, 0).unwrap().split_at(100).is_err());
}

#[test]
fn k_mprotect() {
    let mut r = K.mmap(0, 4096, RW, MAP_PRIVATE, None, 0).unwrap();
    K.region_bytes(&mut r).unwrap()[0] = 1;
    K.mprotect(&mut r, PROT_READ).unwrap();
    assert_eq!(r.prot(), PROT_READ);
    assert!(K.region_bytes(&mut r).is_none(), "read-only memory is not handed out as &mut");
    K.mprotect(&mut r, RW).unwrap();
    assert_eq!(K.region_bytes(&mut r).unwrap()[0], 1, "contents survive protection changes");
    // Unknown protection bits (PROT_GROWSDOWN would reach outside the region): EINVAL.
    assert_eq!(err(K.mprotect(&mut r, RW | 0x0100_0000)), Errno::EINVAL);
    assert_eq!(r.prot(), RW, "a failed change leaves the bookkeeping alone");
    // Write permission on a read-only file mapping: EACCES.
    let t = Tmp::new("mprotect");
    let fd = t.file("f", &[0; 16]);
    let ro = K.openat(Fd::CWD, t.path("f").as_kstr(), O_RDONLY | O_CLOEXEC, 0).unwrap();
    let mut fr = K.mmap(0, 16, PROT_READ, MAP_SHARED, Some(ro), 0).unwrap();
    assert_eq!(err(K.mprotect(&mut fr, RW)), Errno::EACCES);
    K.munmap(fr).unwrap();
    K.munmap(r).unwrap();
    close(ro);
    close(fd);
}

#[test]
fn k_mremap() {
    let mut r = K.mmap(0, 4096, RW, MAP_PRIVATE, None, 0).unwrap();
    K.region_bytes(&mut r).unwrap()[100] = 42;
    K.mremap(&mut r, 3 * 4096, true).unwrap();
    assert_eq!(r.len(), 3 * 4096);
    let b = K.region_bytes(&mut r).unwrap();
    assert_eq!((b[100], b[3 * 4096 - 1]), (42, 0), "contents move with the region; new pages are zero");
    K.mremap(&mut r, 4096, false).unwrap();
    assert_eq!(r.len(), 4096);
    // Growing in place into an occupied range without may_move: ENOMEM.
    let next = K.mmap(r.addr() + 4096, 4096, RW, MAP_PRIVATE | MAP_FIXED_NOREPLACE, None, 0);
    if let Ok(n) = next {
        assert_eq!(err(K.mremap(&mut r, 2 * 4096, false)), Errno::ENOMEM);
        K.munmap(n).unwrap();
    }
    assert_eq!(err(K.mremap(&mut r, 0, true)), Errno::EINVAL);
    K.munmap(r).unwrap();
}

#[test]
fn k_madvise() {
    let mut r = K.mmap(0, 8192, RW, MAP_PRIVATE, None, 0).unwrap();
    K.region_bytes(&mut r).unwrap()[5] = 9;
    for a in [Advice::Normal, Advice::Random, Advice::Sequential, Advice::WillNeed, Advice::DontDump, Advice::DoDump, Advice::KeepOnFork] {
        K.madvise(&mut r, a).unwrap();
    }
    // MADV_DONTNEED on private anonymous memory: the next read sees zero pages (⚑ KERN).
    K.madvise(&mut r, Advice::DontNeed).unwrap();
    assert_eq!(K.region_bytes(&mut r).unwrap()[5], 0);
    // MADV_WIPEONFORK is for private anonymous memory only; on a file mapping: EINVAL.
    let t = Tmp::new("madvise");
    let fd = t.file("f", &[0; 16]);
    let mut fr = K.mmap(0, 16, PROT_READ, MAP_SHARED, Some(fd), 0).unwrap();
    assert_eq!(err(K.madvise(&mut fr, Advice::WipeOnFork)), Errno::EINVAL);
    K.munmap(fr).unwrap();
    K.munmap(r).unwrap();
    close(fd);
}

#[test]
fn k_msync() {
    let fd = mem_file("msync", &[0; 4096]);
    let fr = K.mmap(0, 4096, RW, MAP_SHARED, Some(fd), 0).unwrap();
    K.msync(&fr, MS_SYNC).unwrap();
    K.msync(&fr, MS_ASYNC).unwrap();
    assert_eq!(err(K.msync(&fr, MS_SYNC | MS_ASYNC)), Errno::EINVAL);
    assert_eq!(err(K.msync(&fr, 0x100)), Errno::EINVAL);
    K.munmap(fr).unwrap();
    close(fd);
}

#[test]
fn k_mincore() {
    let mut r = K.mmap(0, 3 * 4096, RW, MAP_PRIVATE, None, 0).unwrap();
    K.region_bytes(&mut r).unwrap()[0] = 1; // fault in page 0 only
    let mut v = [0xffu8; 3];
    K.mincore(&r, &mut v).unwrap();
    assert_eq!(v[0] & 1, 1, "touched page is resident");
    // The vector needs one byte per page: the boundary refuses a short one.
    let mut short = [0u8; 2];
    assert_eq!(err(K.mincore(&r, &mut short)), Errno::EINVAL);
    K.munmap(r).unwrap();
}

#[test]
fn k_mlock() {
    let r = K.mmap(0, 4096, RW, MAP_PRIVATE, None, 0).unwrap();
    K.mlock(&r).unwrap();
    K.munlock(&r).unwrap();
    // Beyond RLIMIT_MEMLOCK (checked in a child with a zero limit): ENOMEM/EPERM (⚑ KERN).
    in_child(|k| {
        k.prlimit(0, RLIMIT_MEMLOCK, Some(&Rlimit { cur: 0, max: 0 })).or_exit();
        let r = k.mmap(0, 4096, RW, MAP_PRIVATE, None, 0).or_exit();
        i32::from(!matches!(k.mlock(&r), Err(Errno::ENOMEM) | Err(Errno::EPERM)))
    });
    K.munmap(r).unwrap();
}

#[test]
fn k_mlock2() {
    let r = K.mmap(0, 8192, RW, MAP_PRIVATE, None, 0).unwrap();
    K.mlock2(&r, MLOCK_ONFAULT).unwrap();
    K.munlock(&r).unwrap();
    assert_eq!(err(K.mlock2(&r, 0x80)), Errno::EINVAL);
    K.munmap(r).unwrap();
}

#[test]
fn k_munlock() {
    let r = K.mmap(0, 4096, RW, MAP_PRIVATE, None, 0).unwrap();
    // Unlocking memory that is not locked succeeds (⚑ KERN).
    K.munlock(&r).unwrap();
    K.mlock(&r).unwrap();
    K.munlock(&r).unwrap();
    K.munmap(r).unwrap();
}

#[test]
fn k_mlockall() {
    in_child(|k| {
        if k.mlockall(MCL_FUTURE | MCL_ONFAULT).is_err() {
            return 1;
        }
        if k.munlockall().is_err() {
            return 2;
        }
        // No flags, or MCL_ONFAULT alone: EINVAL.
        i32::from(k.mlockall(0) != Err(Errno::EINVAL) || k.mlockall(MCL_ONFAULT) != Err(Errno::EINVAL)) * 3
    });
}

#[test]
fn k_munlockall() {
    in_child(|k| i32::from(k.munlockall().is_err()));
}

#[test]
fn k_remap_file_pages() {
    // Deprecated; still accepted on shared file mappings (emulated by the kernel).
    let t = Tmp::new("remap");
    let fd = t.file("f", &[0u8; 8192]);
    let fr = K.mmap(0, 8192, RW, MAP_SHARED, Some(fd), 0).unwrap();
    K.remap_file_pages(&fr, 0).unwrap();
    // On anonymous memory the boundary refuses (the kernel would too).
    let r = K.mmap(0, 4096, RW, MAP_PRIVATE, None, 0).unwrap();
    assert_eq!(err(K.remap_file_pages(&r, 0)), Errno::EINVAL);
    K.munmap(r).unwrap();
    K.munmap(fr).unwrap();
    close(fd);
}

#[test]
fn k_memfd_create() {
    let fd = K.memfd_create(kp("liella-test").as_kstr(), MFD_CLOEXEC | MFD_ALLOW_SEALING).unwrap();
    assert_eq!(K.write(fd, b"hello").unwrap(), 5);
    K.fcntl_int(fd, FcntlInt::AddSeals, F_SEAL_WRITE).unwrap();
    assert_eq!(err(K.write(fd, b"x")), Errno::EPERM);
    assert_eq!(K.fcntl_int(fd, FcntlInt::GetSeals, 0).unwrap() & F_SEAL_WRITE, F_SEAL_WRITE);
    close(fd);
    // Name longer than 249 bytes: EINVAL. Unknown flag: EINVAL.
    assert_eq!(err(K.memfd_create(kp(&"n".repeat(250)).as_kstr(), 0)), Errno::EINVAL);
    assert_eq!(err(K.memfd_create(kp("x").as_kstr(), 0x100)), Errno::EINVAL);
}

/// True if this CPU and kernel support protection keys.
fn have_pkeys() -> bool {
    K.pkru().is_some()
}

#[test]
fn k_pkey_alloc() {
    match K.pkey_alloc(0, 0) {
        Ok(key) => {
            assert!(key > 0);
            K.pkey_free(key).unwrap();
        }
        // No PKU on this CPU (or it is disabled): the kernel says ENOSPC or EINVAL.
        Err(e) => {
            assert!(!have_pkeys());
            assert!(e == Errno::ENOSPC || e == Errno::EINVAL, "{e:?}");
        }
    }
    assert_eq!(err(K.pkey_alloc(1, 0)), Errno::EINVAL);
}

#[test]
fn k_pkey_free() {
    // Freeing a key that was never allocated: EINVAL.
    assert_eq!(err(K.pkey_free(15)), Errno::EINVAL);
    assert_eq!(err(K.pkey_free(-1)), Errno::EINVAL);
}

#[test]
fn k_pkey_mprotect() {
    let mut r = K.mmap(0, 4096, RW, MAP_PRIVATE, None, 0).unwrap();
    // Key -1 means "keep the key": behaves as mprotect.
    K.pkey_mprotect(&mut r, PROT_READ, -1).unwrap();
    assert_eq!((r.prot(), r.pkey()), (PROT_READ, 0));
    if let Ok(key) = K.pkey_alloc(0, PKEY_DISABLE_WRITE) {
        K.pkey_mprotect(&mut r, RW, key).unwrap();
        assert_eq!(r.pkey(), key);
        assert!(K.region_bytes(&mut r).is_none(), "keyed memory is not plain memory");
        K.pkey_mprotect(&mut r, RW, 0).unwrap();
        K.pkey_free(key).unwrap();
    }
    // An unallocated key: EINVAL.
    assert_eq!(err(K.pkey_mprotect(&mut r, RW, 9)), Errno::EINVAL);
    K.munmap(r).unwrap();
}

#[test]
fn k_process_madvise() {
    let r = K.mmap(0, 4096, RW, MAP_PRIVATE, None, 0).unwrap();
    let me = K.pidfd_open(K.getpid(), 0).unwrap();
    let ranges = [RemoteIoVec { base: r.addr(), len: 4096 }];
    // MADV_COLD on our own memory: permitted without privilege for the calling process
    // (Linux 5.10+); older rules require CAP_SYS_NICE (EPERM).
    match K.process_madvise(me, &ranges, RemoteAdvice::Cold, 0) {
        Ok(n) => assert_eq!(n, 4096),
        Err(e) => assert_eq!(e, Errno::EPERM),
    }
    assert_eq!(err(K.process_madvise(me, &ranges, RemoteAdvice::Cold, 1)), Errno::EINVAL);
    assert_eq!(err(K.process_madvise(Fd::STDIN, &ranges, RemoteAdvice::Cold, 0)), Errno::EBADF);
    close(me);
    K.munmap(r).unwrap();
}

#[test]
fn k_process_mrelease() {
    // Only for a process that is exiting after SIGKILL; a live one: EINVAL.
    let pid = selftest::spawn_pausing().unwrap();
    let pidfd = K.pidfd_open(pid, 0).unwrap();
    if kernel::before(5, 15) {
        kernel::note("process_mrelease", "5.15");
        assert_eq!(err(K.process_mrelease(pidfd, 0)), Errno::ENOSYS);
        K.kill(pid, SIGKILL).unwrap();
        K.wait4(pid, 0, None).unwrap();
        close(pidfd);
        return;
    }
    assert_eq!(err(K.process_mrelease(pidfd, 0)), Errno::EINVAL);
    assert_eq!(err(K.process_mrelease(pidfd, 1)), Errno::EINVAL);
    K.kill(pid, SIGKILL).unwrap();
    // After the kill it may succeed or find the memory already gone (ESRCH).
    match K.process_mrelease(pidfd, 0) {
        Ok(()) | Err(Errno::ESRCH) | Err(Errno::EINVAL) => {}
        Err(e) => panic!("{e:?}"),
    }
    K.wait4(pid, 0, None).unwrap();
    close(pidfd);
}

#[test]
fn selftest_children_end_with_their_thread() {
    // Trace: no spec clause (test infrastructure: no test can leave an orphan); milestone 5.
    // A pausing child forked by a thread that ends without reaping it is killed (SIGKILL), or,
    // when the thread ended before the child armed its death signal, exits at once.
    // A watchdog so a hang (once seen: 600 s, the child's own alarm) fails fast and says where.
    let done = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let progress = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let dog = {
        let (done, progress) = (done.clone(), progress.clone());
        std::thread::spawn(move || {
            for _ in 0..600 {
                std::thread::sleep(Duration::from_millis(100));
                if done.load(std::sync::atomic::Ordering::Relaxed) {
                    return;
                }
            }
            eprintln!("WATCHDOG: selftest_children_end_with_their_thread stalled for 60 s in round {}", progress.load(std::sync::atomic::Ordering::Relaxed));
            std::process::abort();
        })
    };
    for round in 0..20 {
        progress.store(round, std::sync::atomic::Ordering::Relaxed);
        let pid = std::thread::spawn(|| selftest::spawn_pausing().unwrap()).join().unwrap();
        let (_, status) = K.wait4(pid, 0, None).unwrap();
        let end = selftest::decode_status(status);
        assert!(end == ChildEnd::Signaled(SIGKILL) || end == ChildEnd::Exited(selftest::PANICKED), "{end:?}");
    }
    done.store(true, std::sync::atomic::Ordering::Relaxed);
    dog.join().unwrap();
}

#[test]
fn selftest_synced_child_ends_when_between_fails() {
    // Trace: no spec clause (test infrastructure: no test can leave an orphan); SETID-TEST-ORPHANS.
    // A panic in `between` (newuidmap failing, say) never sends the go-ahead: the child must see
    // end of file at once, which it cannot while it keeps the pipe's write end itself; it would
    // then end only at its SYNCED_GO_TIMEOUT_MS bound. Re-executed alone, so no fork of another
    // test copies the pipe.
    if probe().as_deref() == Some("synced-between-fails") {
        selftest::mark_own_process();
        std::panic::set_hook(Box::new(|_| {}));
        let start = std::time::Instant::now();
        let r = std::panic::catch_unwind(|| selftest::in_child_synced(|_| 0, |_| 0, |_| panic!("between fails")));
        let quick = start.elapsed() < Duration::from_millis(u64::try_from(selftest::SYNCED_GO_TIMEOUT_MS / 2).unwrap());
        K.exit_group(match (r.is_err(), quick) {
            (true, true) => 0,
            (false, _) => 1,
            (true, false) => 2,
        });
    }
    assert_eq!(run_probe("selftest_synced_child_ends_when_between_fails", "synced-between-fails", &[], "x"), ChildEnd::Exited(0));
}

/// The write end of the parent's pipe for `selftest_synced_child_dies_with_its_parent`.
static SYNCED_REPORT_W: AtomicU32 = AtomicU32::new(0);

#[test]
fn selftest_synced_child_dies_with_its_parent() {
    // Trace: no spec clause (test infrastructure: no test can leave an orphan); SETID-TEST-ORPHANS.
    // A process P (made a subreaper, so orphans come back to it) forks M, which calls
    // in_child_synced; while `between` runs, M forks a holder H (no death signal) that keeps
    // a copy of the go-ahead pipe's write end, reports the child G's and H's IDs, and blocks.
    // P kills M. G gets no end of file (H holds the write end): only its death signal ends it,
    // and it must end within seconds (without it, after SYNCED_GO_TIMEOUT_MS). The incident
    // (k_setgroups/k_setuid orphans, 2026-10-22) had the same shape once the test process died.
    if probe().as_deref() == Some("synced-parent-dies") {
        use liella_kernel::ops::PrctlInt;
        use liella_kernel::uapi::WNOHANG;
        selftest::mark_own_process();
        K.prctl_int(PrctlInt::SetChildSubreaper, 1, 0).unwrap();
        let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
        SYNCED_REPORT_W.store(w.raw() as u32, Ordering::SeqCst);
        let m = selftest::spawn_child(|_| {
            let end = selftest::in_child_synced(|_| 0, |_| 0, |g| {
                let k = CSys;
                let h = selftest::spawn_child(|k| {
                    let _ = k.alarm(300);
                    loop {
                        let _ = k.pause();
                    }
                })
                .or_exit();
                let mut ids = [0u8; 8];
                ids[..4].copy_from_slice(&g.to_ne_bytes());
                ids[4..].copy_from_slice(&h.to_ne_bytes());
                let _ = k.write(Fd::new(SYNCED_REPORT_W.load(Ordering::SeqCst) as i32), &ids);
                loop {
                    let _ = k.pause();
                }
            });
            i32::from(end.is_err())
        })
        .unwrap();
        close(w);
        let mut ids = [0u8; 8];
        let got = K.read(r, &mut ids).unwrap();
        K.kill(m, SIGKILL).unwrap();
        K.wait4(m, 0, None).unwrap();
        assert_eq!(got, 8, "M ended before it reported");
        let g = i32::from_ne_bytes(ids[..4].try_into().unwrap());
        let h = i32::from_ne_bytes(ids[4..].try_into().unwrap());
        let mut g_end = None;
        for _ in 0..500 {
            if let Ok((pid, status)) = K.wait4(g, WNOHANG, None) {
                if pid == g {
                    g_end = Some(selftest::decode_status(status));
                    break;
                }
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        for pid in [g, h] {
            let _ = K.kill(pid, SIGKILL);
            let _ = K.wait4(pid, 0, None);
        }
        K.exit_group(match g_end {
            Some(ChildEnd::Signaled(SIGKILL)) | Some(ChildEnd::Exited(selftest::PANICKED)) => 0,
            Some(_) => 1,
            None => 2,
        });
    }
    assert_eq!(run_probe("selftest_synced_child_dies_with_its_parent", "synced-parent-dies", &[], "x"), ChildEnd::Exited(0));
}

#[test]
fn k_process_vm_readv() {
    // Read from a child's copy of our memory: a forked child sees the same addresses.
    static SECRET: [u8; 8] = *b"liella!!";
    let pid = selftest::spawn_pausing().unwrap();
    let mut buf = [0u8; 8];
    let remote = [RemoteIoVec { base: SECRET.as_ptr().addr(), len: 8 }];
    let n = {
        let mut local = [IoVecMut::new(&mut buf)];
        K.process_vm_readv(pid, &mut local, &remote, 0)
    };
    K.kill(pid, SIGKILL).unwrap();
    K.wait4(pid, 0, None).unwrap();
    assert_eq!(n.unwrap(), 8);
    assert_eq!(&buf, b"liella!!");
    // An unmapped remote address: EFAULT. Flags must be 0.
    let mut local = [IoVecMut::new(&mut buf)];
    assert_eq!(err(K.process_vm_readv(K.getpid(), &mut local, &[RemoteIoVec { base: 8, len: 8 }], 0)), Errno::EFAULT);
    assert_eq!(err(K.process_vm_readv(K.getpid(), &mut local, &remote, 1)), Errno::EINVAL);
}

#[test]
fn k_process_vm_writev() {
    // `raw` only (it writes another process's memory). Through the selftest sequence: a
    // forked child's copy of a word is overwritten and the child reports what it sees.
    assert_eq!(selftest::vm_writev_roundtrip().unwrap(), true);
}

#[test]
fn k_map_shadow_stack() {
    // Linux 6.6+ with CET user shadow stacks. On older kernels: ENOSYS; without CET: EOPNOTSUPP
    // or EINVAL.
    match K.map_shadow_stack(0, 4096, 0) {
        Ok(r) => {
            assert_eq!(r.kind(), RegionKind::ShadowStack);
            K.munmap(r).unwrap();
        }
        Err(e) => assert!([Errno::ENOSYS, Errno::EOPNOTSUPP, Errno::EINVAL].contains(&e), "{e:?}"),
    }
}

// ------------------------------------------------------------------------------ thread support

const PRIV: u32 = FUTEX_PRIVATE_FLAG;

/// Spawn a thread running `f` after `delay_ms`, for waking a waiter.
fn later(delay_ms: u64, f: impl FnOnce() + Send + 'static) -> std::thread::JoinHandle<()> {
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(delay_ms));
        f();
    })
}

#[test]
fn k_futex() {
    // The generic call: an unknown operation is ENOSYS; a misaligned word cannot be built in
    // safe Rust (AtomicU32 is aligned), so the kernel's EINVAL for it is unreachable here.
    let w = AtomicU32::new(0);
    assert_eq!(err(K.futex(&w, 99, 0, FutexArg::None, None, 0)), Errno::ENOSYS);
    // FUTEX_WAKE_OP: atomically set word2 and wake waiters on both words.
    let w2 = AtomicU32::new(5);
    const OP_SET_7_CMP_EQ_5: u32 = (0 << 28) | (0 << 24) | (7 << 12) | 5; // SET 7; wake2 if old == 5
    assert_eq!(K.futex(&w, 5 | PRIV, 1, FutexArg::Val2(1), Some(&w2), OP_SET_7_CMP_EQ_5).unwrap(), 0);
    assert_eq!(w2.load(Ordering::SeqCst), 7);
}

#[test]
fn k_wait() {
    static W: AtomicU32 = AtomicU32::new(0);
    // Value mismatch: EAGAIN at once. Timeout: ETIMEDOUT.
    assert_eq!(err(K.futex(&W, FUTEX_WAIT | PRIV, 1, FutexArg::None, None, 0)), Errno::EAGAIN);
    assert_eq!(err(K.futex(&W, FUTEX_WAIT | PRIV, 0, FutexArg::Timeout(&ms(20)), None, 0)), Errno::ETIMEDOUT);
    // Woken by another thread.
    let h = later(50, || {
        W.store(1, Ordering::SeqCst);
        while K.futex(&W, FUTEX_WAKE | PRIV, 1, FutexArg::None, None, 0).unwrap() == 0 {
            std::thread::sleep(Duration::from_millis(5));
        }
    });
    let r = K.futex(&W, FUTEX_WAIT | PRIV, 0, FutexArg::Timeout(&ms(5000)), None, 0);
    h.join().unwrap();
    assert!(matches!(r, Ok(0) | Err(Errno::EAGAIN)), "{r:?}");
    // Invalid timeout: EINVAL.
    let bad = Timespec { sec: 0, nsec: -1 };
    let z = AtomicU32::new(0);
    assert_eq!(err(K.futex(&z, FUTEX_WAIT | PRIV, 0, FutexArg::Timeout(&bad), None, 0)), Errno::EINVAL);
}

#[test]
fn k_wake() {
    let w = AtomicU32::new(0);
    assert_eq!(K.futex(&w, FUTEX_WAKE | PRIV, 10, FutexArg::None, None, 0).unwrap(), 0, "no waiters");
    // A shared (non-private) wake on the same word also finds nobody.
    assert_eq!(K.futex(&w, FUTEX_WAKE, 1, FutexArg::None, None, 0).unwrap(), 0);
}

#[test]
fn k_wait_bitset() {
    static W: AtomicU32 = AtomicU32::new(0);
    // An absolute CLOCK_MONOTONIC deadline in the past: ETIMEDOUT at once.
    let past = Timespec { sec: 0, nsec: 1 };
    assert_eq!(err(K.futex(&W, FUTEX_WAIT_BITSET | PRIV, 0, FutexArg::Timeout(&past), None, FUTEX_BITSET_MATCH_ANY)), Errno::ETIMEDOUT);
    // A zero bitset: EINVAL.
    assert_eq!(err(K.futex(&W, FUTEX_WAIT_BITSET | PRIV, 0, FutexArg::None, None, 0)), Errno::EINVAL);
    // Selective wake: a waiter on bit 0x2 is not woken by bit 0x1, only by 0x2.
    let h = std::thread::spawn(|| {
        let deadline = {
            let now = K.clock_gettime(CLOCK_MONOTONIC).unwrap();
            Timespec { sec: now.sec + 5, nsec: now.nsec }
        };
        K.futex(&W, FUTEX_WAIT_BITSET | PRIV, 0, FutexArg::Timeout(&deadline), None, 0x2)
    });
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(K.futex(&W, FUTEX_WAKE_BITSET | PRIV, 1, FutexArg::None, None, 0x1).unwrap(), 0);
    let mut woke = 0;
    for _ in 0..100 {
        woke = K.futex(&W, FUTEX_WAKE_BITSET | PRIV, 1, FutexArg::None, None, 0x2).unwrap();
        if woke == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(woke, 1);
    assert_eq!(h.join().unwrap(), Ok(0));
}

/// Start a thread waiting on `word` (value 0) and wait until it is blocked in the kernel.
fn parked_waiter(word: &'static AtomicU32) -> std::thread::JoinHandle<Result<i64, Errno>> {
    let h = std::thread::spawn(move || K.futex(word, FUTEX_WAIT | PRIV, 0, FutexArg::Timeout(&ms(5000)), None, 0));
    std::thread::sleep(Duration::from_millis(100));
    h
}

#[test]
fn k_requeue() {
    static A: AtomicU32 = AtomicU32::new(0);
    static B: AtomicU32 = AtomicU32::new(0);
    let h = parked_waiter(&A);
    // Wake none, move one waiter from A to B: returns the number woken + requeued.
    assert_eq!(K.futex(&A, FUTEX_REQUEUE | PRIV, 0, FutexArg::Val2(1), Some(&B), 0).unwrap(), 1);
    assert_eq!(K.futex(&A, FUTEX_WAKE | PRIV, 1, FutexArg::None, None, 0).unwrap(), 0, "no longer on A");
    assert_eq!(K.futex(&B, FUTEX_WAKE | PRIV, 1, FutexArg::None, None, 0).unwrap(), 1, "now on B");
    assert_eq!(h.join().unwrap(), Ok(0));
}

#[test]
fn k_cmp_requeue() {
    static A: AtomicU32 = AtomicU32::new(0);
    static B: AtomicU32 = AtomicU32::new(0);
    // The expected value (val3) does not match: EAGAIN.
    assert_eq!(err(K.futex(&A, FUTEX_CMP_REQUEUE | PRIV, 0, FutexArg::Val2(1), Some(&B), 1)), Errno::EAGAIN);
    let h = parked_waiter(&A);
    assert_eq!(K.futex(&A, FUTEX_CMP_REQUEUE | PRIV, 1, FutexArg::Val2(0), Some(&B), 0).unwrap(), 1, "woke one");
    assert_eq!(h.join().unwrap(), Ok(0));
}

#[test]
fn k_lock_pi() {
    let w = AtomicU32::new(0);
    // Uncontended: the kernel stores our TID.
    K.futex(&w, FUTEX_LOCK_PI | PRIV, 0, FutexArg::None, None, 0).unwrap();
    assert_eq!(w.load(Ordering::SeqCst) & FUTEX_TID_MASK, K.gettid() as u32);
    // Locking it again ourselves: EDEADLK.
    assert_eq!(err(K.futex(&w, FUTEX_LOCK_PI | PRIV, 0, FutexArg::None, None, 0)), Errno::EDEADLK);
    K.futex(&w, FUTEX_UNLOCK_PI | PRIV, 0, FutexArg::None, None, 0).unwrap();
    assert_eq!(w.load(Ordering::SeqCst), 0);
    // A word naming a TID that does not exist: ESRCH (the owner is gone without OWNER_DIED).
    let dead = AtomicU32::new(0x3fff_fff0);
    assert!(matches!(K.futex(&dead, FUTEX_LOCK_PI | PRIV, 0, FutexArg::Timeout(&Timespec { sec: 0, nsec: 0 }), None, 0), Err(Errno::ESRCH) | Err(Errno::ETIMEDOUT)));
}

#[test]
fn k_lock_pi2() {
    // Linux 5.14: like LOCK_PI, but an absolute timeout is on CLOCK_MONOTONIC.
    let w = AtomicU32::new(0);
    if kernel::before(5, 14) {
        kernel::note("FUTEX_LOCK_PI2", "5.14");
        assert_eq!(err(K.futex(&w, FUTEX_LOCK_PI2 | PRIV, 0, FutexArg::None, None, 0)), Errno::ENOSYS);
        return;
    }
    K.futex(&w, FUTEX_LOCK_PI2 | PRIV, 0, FutexArg::None, None, 0).unwrap();
    assert_eq!(w.load(Ordering::SeqCst) & FUTEX_TID_MASK, K.gettid() as u32);
    // Contended by another thread with a past monotonic deadline: ETIMEDOUT.
    let res = std::thread::scope(|s| {
        s.spawn(|| K.futex(&w, FUTEX_LOCK_PI2 | PRIV, 0, FutexArg::Timeout(&Timespec { sec: 0, nsec: 1 }), None, 0)).join().unwrap()
    });
    assert_eq!(res, Err(Errno::ETIMEDOUT));
    K.futex(&w, FUTEX_UNLOCK_PI | PRIV, 0, FutexArg::None, None, 0).unwrap();
}

#[test]
fn k_unlock_pi() {
    let w = AtomicU32::new(0);
    // Not the owner: EPERM.
    assert_eq!(err(K.futex(&w, FUTEX_UNLOCK_PI | PRIV, 0, FutexArg::None, None, 0)), Errno::EPERM);
    K.futex(&w, FUTEX_LOCK_PI | PRIV, 0, FutexArg::None, None, 0).unwrap();
    // Another thread cannot unlock it.
    let r = std::thread::scope(|s| s.spawn(|| K.futex(&w, FUTEX_UNLOCK_PI | PRIV, 0, FutexArg::None, None, 0)).join().unwrap());
    assert_eq!(r, Err(Errno::EPERM));
    K.futex(&w, FUTEX_UNLOCK_PI | PRIV, 0, FutexArg::None, None, 0).unwrap();
}

#[test]
fn k_trylock_pi() {
    let w = AtomicU32::new(0);
    K.futex(&w, FUTEX_TRYLOCK_PI | PRIV, 0, FutexArg::None, None, 0).unwrap();
    // Held by us: another thread's trylock fails with EAGAIN (or EWOULDBLOCK, the same number).
    let r = std::thread::scope(|s| s.spawn(|| K.futex(&w, FUTEX_TRYLOCK_PI | PRIV, 0, FutexArg::None, None, 0)).join().unwrap());
    assert_eq!(r, Err(Errno::EAGAIN));
    K.futex(&w, FUTEX_UNLOCK_PI | PRIV, 0, FutexArg::None, None, 0).unwrap();
}

#[test]
fn k_wait_requeue_pi() {
    static COND: AtomicU32 = AtomicU32::new(0);
    static MUTEX: AtomicU32 = AtomicU32::new(0);
    // uaddr == uaddr2: EINVAL.
    assert_eq!(err(K.futex(&COND, FUTEX_WAIT_REQUEUE_PI | PRIV, 0, FutexArg::None, Some(&COND), 0)), Errno::EINVAL);
    // A waiter is requeued onto the PI mutex and returns owning it.
    let h = std::thread::spawn(|| {
        let r = K.futex(&COND, FUTEX_WAIT_REQUEUE_PI | PRIV, 0, FutexArg::None, Some(&MUTEX), 0);
        let owner = MUTEX.load(Ordering::SeqCst) & FUTEX_TID_MASK;
        let me = K.gettid() as u32;
        K.futex(&MUTEX, FUTEX_UNLOCK_PI | PRIV, 0, FutexArg::None, None, 0).unwrap();
        (r, owner == me)
    });
    let mut n = 0;
    for _ in 0..200 {
        n = K.futex(&COND, FUTEX_CMP_REQUEUE_PI | PRIV, 1, FutexArg::Val2(0), Some(&MUTEX), 0).unwrap();
        if n > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(n, 1);
    assert_eq!(h.join().unwrap(), (Ok(0), true));
}

#[test]
fn k_cmp_requeue_pi() {
    let cond = AtomicU32::new(0);
    let mutex = AtomicU32::new(0);
    // nr_wake must be 1: EINVAL otherwise. Mismatched value: EAGAIN.
    assert_eq!(err(K.futex(&cond, FUTEX_CMP_REQUEUE_PI | PRIV, 2, FutexArg::Val2(0), Some(&mutex), 0)), Errno::EINVAL);
    assert_eq!(err(K.futex(&cond, FUTEX_CMP_REQUEUE_PI | PRIV, 1, FutexArg::Val2(0), Some(&mutex), 9)), Errno::EAGAIN);
    // No waiters: nothing to do.
    assert_eq!(K.futex(&cond, FUTEX_CMP_REQUEUE_PI | PRIV, 1, FutexArg::Val2(0), Some(&mutex), 0).unwrap(), 0);
}

#[test]
fn k_set_robust_list() {
    // `raw` only (the list names words the kernel writes at thread exit). The selftest
    // sequence checks the protocol: the kernel marks a held robust futex FUTEX_OWNER_DIED.
    let word = selftest::robust_list_owner_died().unwrap();
    assert_eq!(word & FUTEX_OWNER_DIED, FUTEX_OWNER_DIED);
    assert_eq!(word & FUTEX_TID_MASK, 0);
    assert_eq!(selftest::set_robust_list_bad_len(), Err(Errno::EINVAL));
}

static RSEQ_AREA: Rseq = Rseq::new();

#[test]
fn k_rseq() {
    if probe().as_deref() == Some("rseq") {
        // Probe side, started with glibc's own registration disabled.
        let ok = K.rseq(&RSEQ_AREA, 0, RSEQ_SIG).is_ok() && RSEQ_AREA.cpu_id() != RSEQ_CPU_ID_UNINITIALIZED;
        let again = K.rseq(&RSEQ_AREA, 0, RSEQ_SIG) == Err(Errno::EBUSY);
        let bad_sig = K.rseq(&RSEQ_AREA, RSEQ_FLAG_UNREGISTER, RSEQ_SIG ^ 1) == Err(Errno::EPERM);
        let off = K.rseq(&RSEQ_AREA, RSEQ_FLAG_UNREGISTER, RSEQ_SIG).is_ok();
        K.exit_group(if ok && again && bad_sig && off { 44 } else { 1 });
    }
    // In this process the host C library may already have registered an area for the thread
    // (EBUSY); a fresh process without that registration shows the whole protocol.
    // A different area while one is registered: EINVAL (⚑ KERN).
    match K.rseq(&RSEQ_AREA, 0, RSEQ_SIG) {
        Err(Errno::EINVAL) => {}
        Ok(()) => K.rseq(&RSEQ_AREA, RSEQ_FLAG_UNREGISTER, RSEQ_SIG).unwrap(),
        Err(e) => panic!("{e:?}"),
    }
    assert_eq!(run_probe("k_rseq", "rseq", &["GLIBC_TUNABLES=glibc.pthread.rseq=0"], "rseq-probe"), ChildEnd::Exited(44));
    assert_eq!(err(K.rseq(&RSEQ_AREA, 2, RSEQ_SIG)), Errno::EINVAL);
}

#[test]
fn k_arch_prctl() {
    // FS base is the thread pointer (x86_64 TLS ABI).
    assert_eq!(K.arch_prctl_get(ArchGet::Fs).unwrap() as usize, K.thread_pointer());
    let _gs = K.arch_prctl_get(ArchGet::Gs).unwrap();
    // CPUID faulting: query (1 = CPUID enabled). Setting may be unsupported (ENODEV).
    assert_eq!(K.arch_prctl_get_cpuid().unwrap(), 1);
    match K.arch_prctl_set_cpuid(true) {
        Ok(()) | Err(Errno::ENODEV) => {}
        Err(e) => panic!("{e:?}"),
    }
}

#[test]
fn k_sched_yield() {
    for _ in 0..3 {
        K.sched_yield();
    }
}

// ------------------------------------------------------------------------------ file descriptors

#[test]
fn k_openat() {
    let t = Tmp::new("openat");
    let dir = K.openat(Fd::CWD, t.dir().as_kstr(), O_RDONLY | O_DIRECTORY | O_CLOEXEC, 0).unwrap();
    let f = K.openat(dir, kp("new").as_kstr(), O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0o640).unwrap();
    let st = K.fstat(f).unwrap();
    assert_eq!(st.mode & 0o777, 0o640 & !current_umask());
    close(f);
    assert_eq!(err(K.openat(dir, kp("new").as_kstr(), O_WRONLY | O_CREAT | O_EXCL, 0o600)), Errno::EEXIST);
    assert_eq!(err(K.openat(dir, kp("missing").as_kstr(), O_RDONLY, 0)), Errno::ENOENT);
    assert_eq!(err(K.openat(dir, kp("new").as_kstr(), O_RDONLY | O_DIRECTORY, 0)), Errno::ENOTDIR);
    assert_eq!(err(K.openat(Fd::new(-1), kp("rel").as_kstr(), O_RDONLY, 0)), Errno::EBADF);
    // A component longer than NAME_MAX: ENAMETOOLONG.
    assert_eq!(err(K.openat(dir, kp(&"x".repeat(256)).as_kstr(), O_RDONLY, 0)), Errno::ENAMETOOLONG);
    // O_NOFOLLOW on a symlink: ELOOP.
    K.symlinkat(kp("new").as_kstr(), dir, kp("ln").as_kstr()).unwrap();
    assert_eq!(err(K.openat(dir, kp("ln").as_kstr(), O_RDONLY | O_NOFOLLOW, 0)), Errno::ELOOP);
    // openat2 (same family): RESOLVE_BENEATH refuses "..".
    let how = OpenHow { flags: u64::from(O_RDONLY), mode: 0, resolve: RESOLVE_BENEATH };
    assert_eq!(err(K.openat2(dir, kp("../").as_kstr(), &how)), Errno::EXDEV);
    let ok = K.openat2(dir, kp("new").as_kstr(), &how).unwrap();
    close(ok);
    close(dir);
}

/// The process umask, read from /proc without changing it.
fn current_umask() -> u32 {
    let status = std::fs::read_to_string("/proc/self/status").unwrap();
    let line = status.lines().find(|l| l.starts_with("Umask:")).unwrap();
    u32::from_str_radix(line[6..].trim(), 8).unwrap()
}

#[test]
fn k_open_by_handle_at() {
    let t = Tmp::new("handle");
    let f = t.file("f", b"x");
    let mut h = FileHandle::default();
    let _mount_id = K.name_to_handle_at(Fd::CWD, t.path("f").as_kstr(), &mut h, 0).unwrap();
    // Opening by handle needs CAP_DAC_READ_SEARCH in the initial namespace: EPERM here.
    let mnt = K.openat(Fd::CWD, t.dir().as_kstr(), O_RDONLY | O_DIRECTORY, 0).unwrap();
    assert_eq!(err(K.open_by_handle_at(mnt, &h, O_RDONLY)), Errno::EPERM);
    // The capability check comes first, so even a bad mount descriptor reports EPERM.
    assert_eq!(err(K.open_by_handle_at(Fd::new(-1), &h, O_RDONLY)), Errno::EPERM);
    close(mnt);
    close(f);
}

#[test]
fn k_name_to_handle_at() {
    let t = Tmp::new("n2h");
    let f = t.file("f", b"x");
    let mut h = FileHandle::default();
    K.name_to_handle_at(Fd::CWD, t.path("f").as_kstr(), &mut h, 0).unwrap();
    assert!(h.handle_bytes > 0 && h.handle_bytes <= 128);
    // Too small a buffer: EOVERFLOW, with the needed size written back.
    let need = h.handle_bytes;
    let mut small = FileHandle { handle_bytes: 0, ..FileHandle::default() };
    assert_eq!(err(K.name_to_handle_at(Fd::CWD, t.path("f").as_kstr(), &mut small, 0)), Errno::EOVERFLOW);
    assert_eq!(small.handle_bytes, need);
    // More than MAX_HANDLE_SZ: EINVAL. Missing: ENOENT.
    let mut big = FileHandle { handle_bytes: 129, ..FileHandle::default() };
    assert_eq!(err(K.name_to_handle_at(Fd::CWD, t.path("f").as_kstr(), &mut big, 0)), Errno::EINVAL);
    assert_eq!(err(K.name_to_handle_at(Fd::CWD, t.path("none").as_kstr(), &mut h, 0)), Errno::ENOENT);
    close(f);
}

#[test]
fn k_close() {
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    close(r);
    // Closing twice is EBADF; checked in a single-threaded child, where the number cannot
    // have been reused by another thread meanwhile.
    in_child(|k| {
        let [a, b] = k.pipe2(0).or_exit();
        k.close(a).or_exit();
        i32::from(k.close(a) != Err(Errno::EBADF) || k.close(b).is_err())
    });
    close(w);
    // A write end without readers: EPIPE (in a child, for the same fork reason; SIGPIPE is
    // ignored first so the signal does not end it).
    in_child(|k| {
        k.sigaction_set(SIGPIPE, Disposition::Ignore, 0, SigSet::empty()).or_exit();
        let [r, w] = k.pipe2(0).or_exit();
        k.close(r).or_exit();
        i32::from(k.write(w, b"x") != Err(Errno::EPIPE))
    });
    assert_eq!(err(K.close(Fd::new(-1))), Errno::EBADF);
}

#[test]
fn k_close_range() {
    if kernel::before(5, 11) {
        // CLOSE_RANGE_CLOEXEC is Linux 5.11: an unknown flag before (EINVAL); closing works.
        kernel::note("CLOSE_RANGE_CLOEXEC", "5.11");
        in_child(|k| {
            let [a, b] = k.pipe2(0).unwrap();
            let lo = a.raw().min(b.raw()) as u32;
            let hi = a.raw().max(b.raw()) as u32;
            if k.close_range(lo, hi, CLOSE_RANGE_CLOEXEC) != Err(Errno::EINVAL) {
                return 1;
            }
            k.close_range(lo, hi, 0).unwrap();
            i32::from(k.fcntl_int(a, FcntlInt::GetFd, 0) != Err(Errno::EBADF)) * 2
        });
        return;
    }
    in_child(|k| {
        let [a, b] = k.pipe2(0).or_exit();
        let lo = a.raw().min(b.raw()) as u32;
        let hi = a.raw().max(b.raw()) as u32;
        // CLOSE_RANGE_CLOEXEC marks without closing.
        k.close_range(lo, hi, CLOSE_RANGE_CLOEXEC).or_exit();
        if k.fcntl_int(a, FcntlInt::GetFd, 0) != Ok(FD_CLOEXEC) {
            return 1;
        }
        k.close_range(lo, hi, 0).or_exit();
        if k.fcntl_int(a, FcntlInt::GetFd, 0) != Err(Errno::EBADF) {
            return 2;
        }
        // first > last: EINVAL; unknown flag: EINVAL.
        i32::from(k.close_range(10, 9, 0) != Err(Errno::EINVAL) || k.close_range(0, 0, 0x80) != Err(Errno::EINVAL)) * 3
    });
}

#[test]
fn k_dup() {
    let t = Tmp::new("dup");
    let f = t.file("f", b"abcdef");
    let d = K.dup(f).unwrap();
    assert_ne!(d, f);
    // Shared file offset.
    let mut b = [0u8; 2];
    K.read(f, &mut b).unwrap();
    K.read(d, &mut b).unwrap();
    assert_eq!(&b, b"cd");
    // dup clears FD_CLOEXEC.
    assert_eq!(K.fcntl_int(d, FcntlInt::GetFd, 0).unwrap(), 0);
    close(d);
    close(f);
    assert_eq!(err(K.dup(Fd::new(-1))), Errno::EBADF);
}

#[test]
fn k_dup2() {
    let t = Tmp::new("dup2");
    let f = t.file("f", b"z");
    let spare = K.dup(f).unwrap();
    // Onto an open descriptor: it is closed first, silently.
    assert_eq!(K.dup2(f, spare).unwrap(), spare);
    // Same descriptor: returns it unchanged (and validates it).
    assert_eq!(K.dup2(f, f).unwrap(), f);
    assert_eq!(err(K.dup2(Fd::new(-1), spare)), Errno::EBADF);
    assert_eq!(err(K.dup2(f, Fd::new(-1))), Errno::EBADF);
    close(spare);
    close(f);
}

#[test]
fn k_dup3() {
    let t = Tmp::new("dup3");
    let f = t.file("f", b"z");
    let spare = K.dup(f).unwrap();
    assert_eq!(K.dup3(f, spare, O_CLOEXEC).unwrap(), spare);
    assert_eq!(K.fcntl_int(spare, FcntlInt::GetFd, 0).unwrap(), FD_CLOEXEC);
    // Unlike dup2: the same descriptor is EINVAL, and so is an unknown flag.
    assert_eq!(err(K.dup3(f, f, 0)), Errno::EINVAL);
    assert_eq!(err(K.dup3(f, spare, O_NONBLOCK)), Errno::EINVAL);
    close(spare);
    close(f);
}

#[test]
fn k_fcntl() {
    let t = Tmp::new("fcntl");
    let f = t.file("f", &[0u8; 100]);
    // Integer commands.
    let d = K.fcntl_int(f, FcntlInt::DupFdCloexec, 100).unwrap();
    assert!(d >= 100);
    assert_eq!(K.fcntl_int(Fd::new(d as i32), FcntlInt::GetFd, 0).unwrap(), FD_CLOEXEC);
    close(Fd::new(d as i32));
    K.fcntl_int(f, FcntlInt::SetFl, i64::from(O_APPEND)).unwrap();
    assert_eq!(K.fcntl_int(f, FcntlInt::GetFl, 0).unwrap() & i64::from(O_APPEND), i64::from(O_APPEND));
    // Record locks: set a write lock, query from a child (another process sees the conflict).
    let mut lk = Flock::new(F_WRLCK, SEEK_SET as i16, 10, 5);
    K.fcntl_lock(f, FcntlLock::SetLk, &mut lk).unwrap();
    static LOCKFD: AtomicU32 = AtomicU32::new(0);
    LOCKFD.store(f.raw() as u32, Ordering::SeqCst);
    in_child(|k| {
        let f = Fd::new(LOCKFD.load(Ordering::SeqCst) as i32);
        let mut q = Flock::new(F_WRLCK, SEEK_SET as i16, 0, 0);
        k.fcntl_lock(f, FcntlLock::GetLk, &mut q).or_exit();
        // The conflicting lock is described back, with its owner's PID.
        if q.ltype != F_WRLCK || q.start != 10 || q.len != 5 || q.pid != k.getppid() {
            return 1;
        }
        let mut w = Flock::new(F_WRLCK, SEEK_SET as i16, 12, 1);
        i32::from(!matches!(k.fcntl_lock(f, FcntlLock::SetLk, &mut w), Err(Errno::EAGAIN) | Err(Errno::EACCES))) * 2
    });
    // Open-file-description locks: pid must be 0.
    let mut ofd = Flock::new(F_RDLCK, SEEK_SET as i16, 50, 1);
    K.fcntl_lock(f, FcntlLock::OfdSetLk, &mut ofd).unwrap();
    // Owner and hints.
    let mut own = FOwnerEx { otype: 0, pid: K.getpid() };
    K.fcntl_owner_ex(f, true, &mut own).unwrap();
    let mut back = FOwnerEx::default();
    K.fcntl_owner_ex(f, false, &mut back).unwrap();
    assert_eq!(back.pid, K.getpid());
    let mut hint = 2u64; // RWH_WRITE_LIFE_SHORT
    match K.fcntl_hint(f, FcntlHint::SetRwHint, &mut hint) {
        Ok(()) => {
            let mut got = 0u64;
            K.fcntl_hint(f, FcntlHint::GetRwHint, &mut got).unwrap();
            assert_eq!(got, 2);
        }
        Err(e) => assert_eq!(e, Errno::EINVAL),
    }
    // Pipe size.
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    assert!(K.fcntl_int(r, FcntlInt::GetPipeSz, 0).unwrap() >= 4096);
    assert_eq!(err(K.fcntl_int(f, FcntlInt::GetPipeSz, 0)), Errno::EBADF);
    assert_eq!(err(K.fcntl_int(Fd::new(-1), FcntlInt::GetFd, 0)), Errno::EBADF);
    let mut bad = Flock::new(9, SEEK_SET as i16, 0, 0);
    assert_eq!(err(K.fcntl_lock(f, FcntlLock::SetLk, &mut bad)), Errno::EINVAL);
    for x in [r, w, f] {
        close(x);
    }
}

#[test]
fn k_ioctl() {
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    K.write(w, b"12345").unwrap();
    assert_eq!(K.ioctl_get_int(r, IoctlIntOut::Fionread).unwrap(), 5);
    K.ioctl_none(r, IoctlNone::Fionclex).unwrap();
    assert_eq!(K.fcntl_int(r, FcntlInt::GetFd, 0).unwrap(), 0);
    K.ioctl_none(r, IoctlNone::Fioclex).unwrap();
    K.ioctl_set_int(r, IoctlIntIn::Fionbio, 1).unwrap();
    assert_ne!(K.fcntl_int(r, FcntlInt::GetFl, 0).unwrap() & i64::from(O_NONBLOCK), 0);
    // Terminal requests on a pipe: ENOTTY.
    assert_eq!(err(K.tcgets(r)), Errno::ENOTTY);
    assert_eq!(err(K.tiocgwinsz(r)), Errno::ENOTTY);
    // A pseudo-terminal: termios and window size round-trip (kernel struct termios layout).
    let m = K.openat(Fd::CWD, kp("/dev/ptmx").as_kstr(), O_RDWR | O_NOCTTY | O_CLOEXEC, 0).unwrap();
    K.ioctl_set_int(m, IoctlIntIn::Tiocsptlck, 0).unwrap();
    let n = K.ioctl_get_int(m, IoctlIntOut::Tiocgptn).unwrap();
    assert!(n >= 0);
    let mut t = K.tcgets(m).unwrap();
    t.lflag ^= 0o10; // ECHO
    K.tcsets(m, TcSet::Now, &t).unwrap();
    assert_eq!(K.tcgets(m).unwrap().lflag, t.lflag);
    // termios2 (D-19.1): the same words, and integer speeds; BOTHER carries any rate.
    let mut t2 = K.tcgets2(m).unwrap();
    assert_eq!((t2.iflag, t2.oflag, t2.cflag, t2.lflag, t2.cc), (t.iflag, t.oflag, t.cflag, t.lflag, t.cc));
    assert_eq!((t2.ispeed, t2.ospeed), (38400, 38400), "a new pty's B38400");
    t2.cflag = (t2.cflag & !0o10017) | 0o10000; // BOTHER
    t2.ospeed = 12345;
    K.tcsets2(m, TcSet2::Flush, &t2).unwrap();
    let back = K.tcgets2(m).unwrap();
    assert_eq!((back.cflag & 0o10017, back.ospeed, back.ispeed), (0o10000, 12345, 12345));
    assert_eq!(err(K.tcgets2(r)), Errno::ENOTTY);
    assert_eq!(err(K.tcsets2(r, TcSet2::Now, &t2)), Errno::ENOTTY);
    let ws = Winsize { row: 24, col: 80, xpixel: 0, ypixel: 0 };
    K.tiocswinsz(m, &ws).unwrap();
    assert_eq!(K.tiocgwinsz(m).unwrap(), ws);
    let peer = K.ioctl_int(m, IoctlIntArg::Tiocgptpeer, i64::from(O_RDWR | O_NOCTTY | O_CLOEXEC)).unwrap();
    close(Fd::new(peer as i32));
    for x in [m, r, w] {
        close(x);
    }
    assert_eq!(err(K.ioctl_none(Fd::new(-1), IoctlNone::Fioclex)), Errno::EBADF);
}

#[test]
fn k_pipe2() {
    let [r, w] = K.pipe2(O_CLOEXEC | O_NONBLOCK).unwrap();
    let mut b = [0u8; 4];
    assert_eq!(err(K.read(r, &mut b)), Errno::EAGAIN);
    K.write(w, b"ping").unwrap();
    assert_eq!(K.read(r, &mut b).unwrap(), 4);
    assert_eq!(&b, b"ping");
    close(w);
    close(r);
    // End of file once the last writer is gone: in a child, because a concurrent fork in
    // another test would hold a copy of the write end here.
    in_child(|k| {
        let [r, w] = k.pipe2(0).or_exit();
        k.close(w).or_exit();
        let mut b = [0u8; 4];
        i32::from(k.read(r, &mut b) != Ok(0))
    });
    assert_eq!(err(K.pipe2(0x1)), Errno::EINVAL);
}

// ------------------------------------------------------------------------------ data transfer

#[test]
fn k_read() {
    let t = Tmp::new("read");
    let f = t.file("f", b"hello world");
    let mut b = [0u8; 5];
    assert_eq!(K.read(f, &mut b).unwrap(), 5);
    assert_eq!(&b, b"hello");
    let mut rest = [0u8; 64];
    assert_eq!(K.read(f, &mut rest).unwrap(), 6, "short read at end of file");
    assert_eq!(K.read(f, &mut rest).unwrap(), 0);
    assert_eq!(K.read(f, &mut []).unwrap(), 0);
    let dir = K.openat(Fd::CWD, t.dir().as_kstr(), O_RDONLY | O_DIRECTORY, 0).unwrap();
    assert_eq!(err(K.read(dir, &mut b)), Errno::EISDIR);
    let wo = K.openat(Fd::CWD, t.path("f").as_kstr(), O_WRONLY, 0).unwrap();
    assert_eq!(err(K.read(wo, &mut b)), Errno::EBADF);
    for x in [wo, dir, f] {
        close(x);
    }
}

#[test]
fn k_write() {
    let t = Tmp::new("write");
    let f = t.file("f", b"");
    assert_eq!(K.write(f, b"abc").unwrap(), 3);
    assert_eq!(K.write(f, b"").unwrap(), 0);
    assert_eq!(K.fstat(f).unwrap().size, 3);
    let ro = K.openat(Fd::CWD, t.path("f").as_kstr(), O_RDONLY, 0).unwrap();
    assert_eq!(err(K.write(ro, b"x")), Errno::EBADF);
    // RLIMIT_FSIZE exceeded: EFBIG (SIGXFSZ ignored first), in a child.
    static FD: AtomicU32 = AtomicU32::new(0);
    FD.store(f.raw() as u32, Ordering::SeqCst);
    in_child(|k| {
        k.sigaction_set(25, Disposition::Ignore, 0, SigSet::empty()).or_exit(); // SIGXFSZ
        k.prlimit(0, RLIMIT_FSIZE, Some(&Rlimit { cur: 3, max: RLIM_INFINITY })).or_exit();
        i32::from(k.write(Fd::new(FD.load(Ordering::SeqCst) as i32), b"more") != Err(Errno::EFBIG))
    });
    close(ro);
    close(f);
}

#[test]
fn k_pread64() {
    let t = Tmp::new("pread");
    let f = t.file("f", b"0123456789");
    let mut b = [0u8; 3];
    assert_eq!(K.pread(f, &mut b, 4).unwrap(), 3);
    assert_eq!(&b, b"456");
    assert_eq!(K.lseek(f, 0, SEEK_CUR).unwrap(), 0, "offset unchanged");
    assert_eq!(K.pread(f, &mut b, 100).unwrap(), 0);
    assert_eq!(err(K.pread(f, &mut b, -1)), Errno::EINVAL);
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    assert_eq!(err(K.pread(r, &mut b, 0)), Errno::ESPIPE);
    for x in [r, w, f] {
        close(x);
    }
}

#[test]
fn k_pwrite64() {
    let t = Tmp::new("pwrite");
    let f = t.file("f", b"0123456789");
    assert_eq!(K.pwrite(f, b"ab", 3).unwrap(), 2);
    let mut b = [0u8; 10];
    K.pread(f, &mut b, 0).unwrap();
    assert_eq!(&b, b"012ab56789");
    // Writing past the end leaves a hole that reads as zeros.
    K.pwrite(f, b"z", 20).unwrap();
    let mut h = [9u8; 2];
    K.pread(f, &mut h, 12).unwrap();
    assert_eq!(h, [0, 0]);
    assert_eq!(err(K.pwrite(f, b"x", -5)), Errno::EINVAL);
    close(f);
}

#[test]
fn k_readv() {
    let t = Tmp::new("readv");
    let f = t.file("f", b"abcdefgh");
    let (mut a, mut b) = ([0u8; 3], [0u8; 10]);
    let n = {
        let mut iov = [IoVecMut::new(&mut a), IoVecMut::new(&mut b)];
        K.readv(f, &mut iov).unwrap()
    };
    assert_eq!((n, &a, &b[..5]), (8, b"abc", &b"defgh"[..]));
    // More than IOV_MAX (1,024) entries: EINVAL.
    let mut bufs: Vec<[u8; 1]> = vec![[0]; 1025];
    let mut iov: Vec<IoVecMut<'_>> = bufs.iter_mut().map(|x| IoVecMut::new(x)).collect();
    assert_eq!(err(K.readv(f, &mut iov)), Errno::EINVAL);
    close(f);
}

#[test]
fn k_writev() {
    let t = Tmp::new("writev");
    let f = t.file("f", b"");
    assert_eq!(K.writev(f, &[IoVec::new(b"ab"), IoVec::new(b""), IoVec::new(b"cd")]).unwrap(), 4);
    let mut b = [0u8; 4];
    K.pread(f, &mut b, 0).unwrap();
    assert_eq!(&b, b"abcd");
    assert_eq!(K.writev(f, &[]).unwrap(), 0);
    close(f);
}

#[test]
fn k_preadv() {
    let t = Tmp::new("preadv");
    let f = t.file("f", b"0123456789");
    let (mut a, mut b) = ([0u8; 2], [0u8; 2]);
    {
        let mut iov = [IoVecMut::new(&mut a), IoVecMut::new(&mut b)];
        assert_eq!(K.preadv(f, &mut iov, 6).unwrap(), 4);
    }
    assert_eq!((&a, &b), (b"67", b"89"));
    let mut iov = [IoVecMut::new(&mut a)];
    assert_eq!(err(K.preadv(f, &mut iov, -1)), Errno::EINVAL);
    close(f);
}

#[test]
fn k_pwritev() {
    let t = Tmp::new("pwritev");
    let f = t.file("f", b"0123456789");
    assert_eq!(K.pwritev(f, &[IoVec::new(b"x"), IoVec::new(b"y")], 8).unwrap(), 2);
    let mut b = [0u8; 10];
    K.pread(f, &mut b, 0).unwrap();
    assert_eq!(&b, b"01234567xy");
    close(f);
}

#[test]
fn k_preadv2() {
    let t = Tmp::new("preadv2");
    let f = t.file("f", b"abcdef");
    let mut a = [0u8; 3];
    {
        // Offset -1: use and advance the file offset.
        let mut iov = [IoVecMut::new(&mut a)];
        assert_eq!(K.preadv2(f, &mut iov, -1, 0).unwrap(), 3);
    }
    assert_eq!(K.lseek(f, 0, SEEK_CUR).unwrap(), 3);
    let mut iov = [IoVecMut::new(&mut a)];
    assert_eq!(K.preadv2(f, &mut iov, 0, RWF_HIPRI).unwrap(), 3);
    assert_eq!(err(K.preadv2(f, &mut iov, 0, 0x8000_0000)), Errno::EOPNOTSUPP);
    // RWF_NOWAIT on data already in the page cache succeeds where the file's file system takes
    // non-blocking reads: ext2/3/4, xfs and btrfs always do, and there it must succeed. Others
    // (tmpfs on Linux 6.8, for one) refuse the flag with EOPNOTSUPP (docs/testing-filesystems.md).
    let fs = K.fstatfs(f).unwrap().ftype;
    match K.preadv2(f, &mut iov, 0, RWF_NOWAIT) {
        Ok(n) => assert_eq!(n, 3),
        Err(e) => {
            const NOWAIT_FS: [i64; 3] = [0xef53, 0x5846_5342, 0x9123_683e]; // ext2/3/4, xfs, btrfs
            assert!(e == Errno::EOPNOTSUPP && !NOWAIT_FS.contains(&fs), "{e:?} on file system {fs:#x}");
        }
    }
    // On an empty pipe: EAGAIN where pipes support non-blocking flags, EOPNOTSUPP on kernels
    // whose pipes lack FMODE_NOWAIT (5.15 is one).
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    let e = err(K.preadv2(r, &mut iov, -1, RWF_NOWAIT));
    assert!(e == Errno::EAGAIN || e == Errno::EOPNOTSUPP, "{e:?}");
    for x in [r, w, f] {
        close(x);
    }
}

#[test]
fn k_pwritev2() {
    let t = Tmp::new("pwritev2");
    let f = t.file("f", b"0123");
    // RWF_APPEND writes at the end regardless of the offset given.
    assert_eq!(K.pwritev2(f, &[IoVec::new(b"zz")], 0, RWF_APPEND).unwrap(), 2);
    let mut b = [0u8; 6];
    K.pread(f, &mut b, 0).unwrap();
    assert_eq!(&b, b"0123zz");
    K.pwritev2(f, &[IoVec::new(b"s")], 0, RWF_DSYNC).unwrap();
    assert_eq!(err(K.pwritev2(f, &[IoVec::new(b"s")], 0, 0x8000_0000)), Errno::EOPNOTSUPP);
    close(f);
}

#[test]
fn k_lseek() {
    let t = Tmp::new("lseek");
    let f = t.file("f", b"0123456789");
    assert_eq!(K.lseek(f, 4, SEEK_SET).unwrap(), 4);
    assert_eq!(K.lseek(f, 2, SEEK_CUR).unwrap(), 6);
    assert_eq!(K.lseek(f, -1, SEEK_END).unwrap(), 9);
    // Beyond the end is allowed; before 0 is EINVAL.
    assert_eq!(K.lseek(f, 1000, SEEK_SET).unwrap(), 1000);
    assert_eq!(err(K.lseek(f, -1, SEEK_SET)), Errno::EINVAL);
    assert_eq!(err(K.lseek(f, 0, 9)), Errno::EINVAL);
    // SEEK_DATA / SEEK_HOLE: data at 0; the end is an implicit hole.
    assert_eq!(K.lseek(f, 0, SEEK_DATA).unwrap(), 0);
    assert_eq!(K.lseek(f, 0, SEEK_HOLE).unwrap(), 10);
    assert_eq!(err(K.lseek(f, 10, SEEK_DATA)), Errno::ENXIO);
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    assert_eq!(err(K.lseek(r, 0, SEEK_CUR)), Errno::ESPIPE);
    for x in [r, w, f] {
        close(x);
    }
}

#[test]
fn k_sendfile() {
    let t = Tmp::new("sendfile");
    let src = t.file("src", b"abcdefgh");
    let dst = t.file("dst", b"");
    let mut off = 2i64;
    assert_eq!(K.sendfile(dst, src, Some(&mut off), 4).unwrap(), 4);
    assert_eq!(off, 6, "offset advanced; the file's own offset untouched");
    assert_eq!(K.lseek(src, 0, SEEK_CUR).unwrap(), 0);
    let mut b = [0u8; 4];
    K.pread(dst, &mut b, 0).unwrap();
    assert_eq!(&b, b"cdef");
    assert_eq!(K.sendfile(dst, src, None, 100).unwrap(), 8);
    let ro = K.openat(Fd::CWD, t.path("dst").as_kstr(), O_RDONLY, 0).unwrap();
    assert_eq!(err(K.sendfile(ro, src, None, 1)), Errno::EBADF);
    for x in [ro, dst, src] {
        close(x);
    }
}

#[test]
fn k_copy_file_range() {
    let t = Tmp::new("cfr");
    let src = t.file("src", b"0123456789");
    let dst = t.file("dst", b"");
    let (mut i, mut o) = (3i64, 0i64);
    assert_eq!(K.copy_file_range(src, Some(&mut i), dst, Some(&mut o), 4, 0).unwrap(), 4);
    assert_eq!((i, o), (7, 4));
    let mut b = [0u8; 4];
    K.pread(dst, &mut b, 0).unwrap();
    assert_eq!(&b, b"3456");
    assert_eq!(err(K.copy_file_range(src, None, dst, None, 1, 1)), Errno::EINVAL);
    // Overlapping ranges within one file: EINVAL.
    let (mut a, mut c) = (0i64, 2i64);
    assert_eq!(err(K.copy_file_range(src, Some(&mut a), src, Some(&mut c), 5, 0)), Errno::EINVAL);
    close(src);
    close(dst);
}

#[test]
fn k_splice() {
    let t = Tmp::new("splice");
    let src = t.file("src", b"spliced!");
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    let mut off = 0i64;
    assert_eq!(K.splice(src, Some(&mut off), w, None, 8, 0).unwrap(), 8);
    assert_eq!(off, 8);
    let mut b = [0u8; 8];
    K.read(r, &mut b).unwrap();
    assert_eq!(&b, b"spliced!");
    // Neither end a pipe: EINVAL. An offset for a pipe end: ESPIPE.
    let dst = t.file("dst", b"");
    assert_eq!(err(K.splice(src, None, dst, None, 1, 0)), Errno::EINVAL);
    let mut po = 0i64;
    assert_eq!(err(K.splice(src, None, w, Some(&mut po), 1, 0)), Errno::ESPIPE);
    for x in [r, w, src, dst] {
        close(x);
    }
}

#[test]
fn k_tee() {
    let [r1, w1] = K.pipe2(O_CLOEXEC).unwrap();
    let [r2, w2] = K.pipe2(O_CLOEXEC).unwrap();
    K.write(w1, b"dup me").unwrap();
    assert_eq!(K.tee(r1, w2, 6, 0).unwrap(), 6);
    let mut a = [0u8; 6];
    let mut b = [0u8; 6];
    K.read(r1, &mut a).unwrap();
    K.read(r2, &mut b).unwrap();
    assert_eq!((&a, &b), (b"dup me", b"dup me"), "tee copies without consuming");
    // Same pipe on both sides: EINVAL. Nothing to copy, non-blocking: EAGAIN.
    assert_eq!(err(K.tee(r1, w1, 1, 0)), Errno::EINVAL);
    assert_eq!(err(K.tee(r1, w2, 1, SPLICE_F_NONBLOCK)), Errno::EAGAIN);
    for x in [r1, w1, r2, w2] {
        close(x);
    }
}

#[test]
fn k_vmsplice() {
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    let mut data = *b"user pages";
    {
        let mut iov = [IoVecMut::new(&mut data)];
        assert_eq!(K.vmsplice(w, &mut iov, 0).unwrap(), 10);
    }
    // From the read end, vmsplice copies into the buffers.
    let mut out = [0u8; 10];
    {
        let mut iov = [IoVecMut::new(&mut out)];
        assert_eq!(K.vmsplice(r, &mut iov, 0).unwrap(), 10);
    }
    assert_eq!(&out, b"user pages");
    // Not a pipe: EBADF.
    let t = Tmp::new("vmsplice");
    let f = t.file("f", b"");
    let mut iov = [IoVecMut::new(&mut out)];
    assert_eq!(err(K.vmsplice(f, &mut iov, 0)), Errno::EBADF);
    for x in [r, w, f] {
        close(x);
    }
}

#[test]
fn k_readahead() {
    let t = Tmp::new("readahead");
    let f = t.file("f", &[1u8; 8192]);
    K.readahead(f, 0, 8192).unwrap();
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    assert_eq!(err(K.readahead(r, 0, 1)), Errno::EINVAL);
    assert_eq!(err(K.readahead(Fd::new(-1), 0, 1)), Errno::EBADF);
    for x in [r, w, f] {
        close(x);
    }
}

// The io_uring calls are not in Appendix A.1 (the spec lists them under H17); the POSIX AIO
// engine uses them (D-11.8, D-11.11), so they are in the C layer's table and tested here. Where
// the kernel refuses io_uring (`kernel.io_uring_disabled`: EPERM; compiled out: ENOSYS), AIO runs
// on its worker pool, and these tests say so and check only the refusal.

/// A new ring of `entries` with `flags` (and `cq` completion entries with `IORING_SETUP_CQSIZE`):
/// its descriptor and the parameters the kernel filled in, or the error.
fn uring(entries: u32, flags: u32, cq: u32) -> Result<(Fd, [u32; 30]), Errno> {
    let mut p = [0u32; 30];
    p[1] = cq;
    p[2] = flags;
    selftest::io_uring_setup(entries, &mut p).map(|fd| (Fd::new(fd), p))
}

/// io_uring is usable here; otherwise a note, after checking that the refusal is one the AIO
/// engine handles.
fn uring_usable() -> bool {
    match uring(8, 0, 0) {
        Ok((fd, _)) => {
            close(fd);
            true
        }
        Err(e) => {
            assert!(e == Errno::EPERM || e == Errno::ENOSYS, "io_uring_setup: {e:?}");
            eprintln!("note: io_uring is refused here ({e:?}); only the refusal is checked");
            false
        }
    }
}

#[test]
fn k_io_uring_setup() {
    if !uring_usable() {
        return;
    }
    // The kernel fills in the sizes (completion queue twice the submission queue by default),
    // the features and the ring offsets; the descriptor is a new one.
    let (fd, p) = uring(8, 0, 0).unwrap();
    assert!(fd.raw() >= 0);
    assert_eq!((p[0], p[1]), (8, 16), "sq_entries, cq_entries");
    assert_ne!(p[5], 0, "features");
    assert!(p[16] > 0 && p[25] > 0, "sq_off.array, cq_off.cqes");
    close(fd);
    // IORING_SETUP_CQSIZE takes the completion queue's size from the parameters.
    let (fd, p) = uring(8, 1 << 3, 64).unwrap();
    assert_eq!((p[0], p[1]), (8, 64));
    close(fd);
    // Refusals: no entries; CQSIZE with a zero size; a non-zero reserved word.
    assert_eq!(uring(0, 0, 0).unwrap_err(), Errno::EINVAL);
    assert_eq!(uring(8, 1 << 3, 0).unwrap_err(), Errno::EINVAL);
    let mut p = [0u32; 30];
    p[7] = 1;
    assert_eq!(selftest::io_uring_setup(8, &mut p), Err(Errno::EINVAL));
}

#[test]
fn k_io_uring_enter() {
    if !uring_usable() {
        assert!(matches!(selftest::io_uring_enter(-1, 0, 0), Err(Errno::EBADF | Errno::ENOSYS)));
        return;
    }
    let (fd, _) = uring(8, 0, 0).unwrap();
    // Nothing to submit or wait for: 0 at once, with or without IORING_ENTER_GETEVENTS.
    assert_eq!(selftest::io_uring_enter(fd.raw(), 0, 0), Ok(0));
    assert_eq!(selftest::io_uring_enter(fd.raw(), 0, 1), Ok(0));
    // Not a descriptor; not a ring.
    assert_eq!(selftest::io_uring_enter(999_999, 0, 0), Err(Errno::EBADF));
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    assert_eq!(selftest::io_uring_enter(r.raw(), 0, 0), Err(Errno::EOPNOTSUPP));
    for x in [fd, r, w] {
        close(x);
    }
}

#[test]
fn k_io_uring_register() {
    if !uring_usable() {
        return;
    }
    let (fd, _) = uring(8, 0, 0).unwrap();
    // IORING_REGISTER_PROBE: the last operation code, then one entry per operation with its
    // code and flags; IORING_OP_NOP (0) is always supported (IO_URING_OP_SUPPORTED).
    let mut probe = [0u8; 16 + 8 * 32];
    selftest::io_uring_probe(fd.raw(), &mut probe, 32).unwrap();
    assert!(probe[0] > 0, "last_op");
    assert_eq!(probe[16], 0, "ops[0].op");
    assert_eq!(probe[18] & 1, 1, "ops[0].flags");
    // Not a descriptor; not a ring; no room for the entries asked for (refused before the call).
    let mut fresh = [0u8; 16 + 8 * 32];
    assert_eq!(selftest::io_uring_probe(999_999, &mut fresh, 32), Err(Errno::EBADF));
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    assert_eq!(selftest::io_uring_probe(r.raw(), &mut fresh, 32), Err(Errno::EOPNOTSUPP));
    assert_eq!(selftest::io_uring_probe(fd.raw(), &mut fresh, 33), Err(Errno::EINVAL));
    for x in [fd, r, w] {
        close(x);
    }
}

// ------------------------------------------------------------------------------ multiplexing and events

#[test]
fn k_pselect6() {
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    let nfds = (r.raw().max(w.raw()) + 1) as u32;
    // Nothing readable, zero timeout: 0, and the set is cleared.
    let mut rs = FdSet::default();
    rs.set(r.raw());
    assert_eq!(K.pselect6(nfds, Some(&mut rs), None, None, Some(&mut ms(0)), None).unwrap(), 0);
    assert!(!rs.is_set(r.raw()));
    // Writable at once; readable after a write. The remaining time is written back.
    let mut ws = FdSet::default();
    ws.set(w.raw());
    assert_eq!(K.pselect6(nfds, None, Some(&mut ws), None, None, None).unwrap(), 1);
    K.write(w, b"x").unwrap();
    let mut rs = FdSet::default();
    rs.set(r.raw());
    let mut left = ms(5000);
    assert_eq!(K.pselect6(nfds, Some(&mut rs), None, None, Some(&mut left), Some(&SigSet::empty())).unwrap(), 1);
    assert!(rs.is_set(r.raw()));
    assert!(left.sec <= 5 && left.sec >= 4, "remaining time written back: {left:?}");
    // A closed descriptor in a set: EBADF (in a child, so the number cannot be reused).
    in_child(|k| {
        let [a, b] = k.pipe2(0).or_exit();
        k.close(a).or_exit();
        let mut bad = FdSet::default();
        bad.set(a.raw());
        let n = (a.raw().max(b.raw()) + 1) as u32;
        i32::from(k.pselect6(n, Some(&mut bad), None, None, Some(&mut ms(0)), None) != Err(Errno::EBADF))
    });
    // Bits beyond the descriptor table are ignored, not errors (⚑ KERN).
    let mut far = FdSet::default();
    far.set(1000);
    assert_eq!(K.pselect6(1001, Some(&mut far), None, None, Some(&mut ms(0)), None).unwrap(), 0);
    // Negative timeout: EINVAL. nfds > 1,024: refused by the boundary.
    assert_eq!(err(K.pselect6(nfds, None, None, None, Some(&mut Timespec { sec: -1, nsec: 0 }), None)), Errno::EINVAL);
    assert_eq!(err(K.pselect6(1025, None, None, None, Some(&mut ms(0)), None)), Errno::EINVAL);
    close(r);
    close(w);
}

#[test]
fn k_ppoll() {
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    let mut fds = [PollFd { fd: r.raw(), events: POLLIN, revents: 0 }, PollFd { fd: -1, events: POLLIN, revents: 7 }];
    assert_eq!(K.ppoll(&mut fds, Some(&mut ms(10)), None).unwrap(), 0);
    assert_eq!(fds[1].revents, 0, "negative descriptors are ignored and their revents cleared");
    K.write(w, b"x").unwrap();
    assert_eq!(K.ppoll(&mut fds, None, Some(&SigSet::empty())).unwrap(), 1);
    assert_eq!(fds[0].revents & POLLIN, POLLIN);
    close(w);
    // POLLHUP once the last writer is gone (in a child: concurrent forks copy descriptors).
    in_child(|k| {
        let [r, w] = k.pipe2(0).or_exit();
        k.close(w).or_exit();
        let mut fds = [PollFd { fd: r.raw(), events: POLLIN, revents: 0 }];
        k.ppoll(&mut fds, Some(&mut ms(0)), None).or_exit();
        i32::from(fds[0].revents & POLLHUP == 0)
    });
    close(r);
    // A descriptor that is not open reports POLLNVAL (not an error).
    let mut fds = [PollFd { fd: 999_999, events: POLLIN, revents: 0 }];
    assert_eq!(K.ppoll(&mut fds, Some(&mut ms(0)), None).unwrap(), 1);
    assert_eq!(fds[0].revents, POLLNVAL);
    // More descriptors than RLIMIT_NOFILE: EINVAL (checked in a child with a low limit).
    in_child(|k| {
        k.prlimit(0, RLIMIT_NOFILE, Some(&Rlimit { cur: 4, max: 4 })).or_exit();
        let mut many = [PollFd { fd: -1, events: 0, revents: 0 }; 5];
        i32::from(k.ppoll(&mut many, Some(&mut ms(0)), None) != Err(Errno::EINVAL))
    });
}

#[test]
fn k_epoll_create1() {
    let ep = K.epoll_create1(EPOLL_CLOEXEC).unwrap();
    assert_eq!(K.fcntl_int(ep, FcntlInt::GetFd, 0).unwrap(), FD_CLOEXEC);
    close(ep);
    assert_eq!(err(K.epoll_create1(1)), Errno::EINVAL);
}

#[test]
fn k_epoll_ctl() {
    let ep = K.epoll_create1(EPOLL_CLOEXEC).unwrap();
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    let ev = EpollEvent { events: EPOLLIN, data: 0xdead_beef };
    K.epoll_ctl(ep, EPOLL_CTL_ADD, r, Some(&ev)).unwrap();
    assert_eq!(err(K.epoll_ctl(ep, EPOLL_CTL_ADD, r, Some(&ev))), Errno::EEXIST);
    K.epoll_ctl(ep, EPOLL_CTL_MOD, r, Some(&EpollEvent { events: EPOLLIN, data: 1 })).unwrap();
    K.epoll_ctl(ep, EPOLL_CTL_DEL, r, None).unwrap();
    assert_eq!(err(K.epoll_ctl(ep, EPOLL_CTL_DEL, r, None)), Errno::ENOENT);
    assert_eq!(err(K.epoll_ctl(ep, EPOLL_CTL_ADD, ep, Some(&ev))), Errno::EINVAL, "an epoll set cannot watch itself");
    // Regular files are not pollable: EPERM.
    let t = Tmp::new("epoll");
    let f = t.file("f", b"");
    assert_eq!(err(K.epoll_ctl(ep, EPOLL_CTL_ADD, f, Some(&ev))), Errno::EPERM);
    for x in [f, r, w, ep] {
        close(x);
    }
}

#[test]
fn k_epoll_pwait() {
    let ep = K.epoll_create1(EPOLL_CLOEXEC).unwrap();
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    K.epoll_ctl(ep, EPOLL_CTL_ADD, r, Some(&EpollEvent { events: EPOLLIN, data: 42 })).unwrap();
    let mut evs = [EpollEvent::default(); 4];
    assert_eq!(K.epoll_pwait(ep, &mut evs, 0, None).unwrap(), 0);
    K.write(w, b"x").unwrap();
    assert_eq!(K.epoll_pwait(ep, &mut evs, 1000, Some(&SigSet::empty())).unwrap(), 1);
    let (events, data) = (evs[0].events, evs[0].data);
    assert_eq!((events & EPOLLIN, data), (EPOLLIN, 42), "packed epoll_event layout");
    assert_eq!(err(K.epoll_pwait(ep, &mut [], 0, None)), Errno::EINVAL);
    assert_eq!(err(K.epoll_pwait(r, &mut evs, 0, None)), Errno::EINVAL);
    for x in [r, w, ep] {
        close(x);
    }
}

#[test]
fn k_epoll_pwait2() {
    let ep = K.epoll_create1(EPOLL_CLOEXEC).unwrap();
    if kernel::before(5, 11) {
        kernel::note("epoll_pwait2", "5.11");
        let mut evs = [EpollEvent::default(); 2];
        assert_eq!(err(K.epoll_pwait2(ep, &mut evs, Some(&ms(10)), None)), Errno::ENOSYS);
        close(ep);
        return;
    }
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    K.epoll_ctl(ep, EPOLL_CTL_ADD, r, Some(&EpollEvent { events: EPOLLIN, data: 7 })).unwrap();
    let mut evs = [EpollEvent::default(); 2];
    assert_eq!(K.epoll_pwait2(ep, &mut evs, Some(&ms(10)), None).unwrap(), 0);
    K.write(w, b"x").unwrap();
    assert_eq!(K.epoll_pwait2(ep, &mut evs, None, None).unwrap(), 1);
    assert_eq!(err(K.epoll_pwait2(ep, &mut evs, Some(&Timespec { sec: 0, nsec: -1 }), None)), Errno::EINVAL);
    for x in [r, w, ep] {
        close(x);
    }
}

#[test]
fn k_eventfd2() {
    let e = K.eventfd2(3, EFD_CLOEXEC | EFD_NONBLOCK).unwrap();
    let mut v = [0u8; 8];
    assert_eq!(K.read(e, &mut v).unwrap(), 8);
    assert_eq!(u64::from_ne_bytes(v), 3);
    assert_eq!(err(K.read(e, &mut v)), Errno::EAGAIN);
    K.write(e, &5u64.to_ne_bytes()).unwrap();
    assert_eq!(err(K.write(e, &u64::MAX.to_ne_bytes())), Errno::EINVAL);
    assert_eq!(err(K.read(e, &mut [0u8; 4])), Errno::EINVAL, "reads need 8 bytes");
    close(e);
    let s = K.eventfd2(0, EFD_SEMAPHORE | EFD_NONBLOCK | EFD_CLOEXEC).unwrap();
    K.write(s, &2u64.to_ne_bytes()).unwrap();
    K.read(s, &mut v).unwrap();
    assert_eq!(u64::from_ne_bytes(v), 1, "semaphore mode reads 1 at a time");
    close(s);
    assert_eq!(err(K.eventfd2(0, 0x10)), Errno::EINVAL);
}

#[test]
fn k_inotify_init1() {
    let i = K.inotify_init1(IN_CLOEXEC | IN_NONBLOCK).unwrap();
    let mut b = [0u8; 256];
    assert_eq!(err(K.read(i, &mut b)), Errno::EAGAIN);
    close(i);
    assert_eq!(err(K.inotify_init1(0x1)), Errno::EINVAL);
}

#[test]
fn k_inotify_add_watch() {
    let t = Tmp::new("inotify");
    let i = K.inotify_init1(IN_CLOEXEC | IN_NONBLOCK).unwrap();
    let wd = K.inotify_add_watch(i, t.dir().as_kstr(), IN_CREATE).unwrap();
    assert!(wd > 0);
    close(t.file("child", b""));
    let mut b = [0u8; 256];
    let n = K.read(i, &mut b).unwrap();
    assert!(n >= 16);
    let got_wd = i32::from_ne_bytes([b[0], b[1], b[2], b[3]]);
    let mask = u32::from_ne_bytes([b[4], b[5], b[6], b[7]]);
    assert_eq!((got_wd, mask & IN_CREATE), (wd, IN_CREATE));
    assert_eq!(err(K.inotify_add_watch(i, t.path("none").as_kstr(), IN_CREATE)), Errno::ENOENT);
    assert_eq!(err(K.inotify_add_watch(i, t.dir().as_kstr(), 0)), Errno::EINVAL);
    close(i);
}

#[test]
fn k_inotify_rm_watch() {
    let t = Tmp::new("inotify_rm");
    let i = K.inotify_init1(IN_CLOEXEC).unwrap();
    let wd = K.inotify_add_watch(i, t.dir().as_kstr(), IN_DELETE).unwrap();
    K.inotify_rm_watch(i, wd).unwrap();
    assert_eq!(err(K.inotify_rm_watch(i, wd)), Errno::EINVAL);
    close(i);
}

#[test]
fn k_fanotify_init() {
    // Unprivileged use (Linux 5.13+) needs FAN_REPORT_FID and no permission events.
    match K.fanotify_init(FAN_CLASS_NOTIF | FAN_REPORT_FID | FAN_CLOEXEC, O_RDONLY) {
        Ok(fd) => close(fd),
        Err(e) => assert_eq!(e, Errno::EPERM),
    }
    // Without FAN_REPORT_FID an unprivileged caller is refused.
    assert_eq!(err(K.fanotify_init(FAN_CLASS_NOTIF | FAN_CLOEXEC, O_RDONLY)), Errno::EPERM);
}

#[test]
fn k_fanotify_mark() {
    let t = Tmp::new("fanotify");
    let fan = match K.fanotify_init(FAN_CLASS_NOTIF | FAN_REPORT_FID | FAN_CLOEXEC, O_RDONLY) {
        Ok(fan) => fan,
        // Unprivileged fanotify is Linux 5.13: EPERM before.
        Err(Errno::EPERM) if kernel::before(5, 13) => {
            kernel::note("unprivileged fanotify", "5.13");
            return;
        }
        Err(e) => panic!("unprivileged fanotify unavailable on this kernel: {e:?}"),
    };
    K.fanotify_mark(fan, FAN_MARK_ADD, FAN_CLOSE_WRITE, Fd::CWD, Some(t.dir().as_kstr())).unwrap();
    // Unprivileged groups may not mark whole mounts: EPERM. Unknown flags: EINVAL.
    assert_eq!(err(K.fanotify_mark(fan, FAN_MARK_ADD | 0x10, FAN_MODIFY, Fd::CWD, Some(t.dir().as_kstr()))), Errno::EPERM);
    assert_eq!(err(K.fanotify_mark(fan, 0x8000_0000, FAN_MODIFY, Fd::CWD, Some(t.dir().as_kstr()))), Errno::EINVAL);
    close(fan);
}

// ------------------------------------------------------------------------------ durability and space

#[test]
fn k_fsync() {
    let f = mem_file("fsync", b"data");
    K.fsync(f).unwrap();
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    assert_eq!(err(K.fsync(r)), Errno::EINVAL);
    assert_eq!(err(K.fsync(Fd::new(-1))), Errno::EBADF);
    for x in [r, w, f] {
        close(x);
    }
}

#[test]
fn k_fdatasync() {
    let f = mem_file("fdatasync", b"data");
    K.fdatasync(f).unwrap();
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    assert_eq!(err(K.fdatasync(r)), Errno::EINVAL);
    for x in [r, w, f] {
        close(x);
    }
}

#[test]
fn k_sync() {
    if sync_trap::child() {
        K.sync();
        println!("{}", sync_trap::RETURNED);
        return;
    }
    // SYS_sync issued once and success returned, under a seccomp trap instead of the machine-wide
    // writeback (GATE-FSYNC-2); no test makes a real sync().
    sync_trap::check("k_sync");
}

#[test]
fn k_syncfs() {
    let f = mem_file("syncfs", b"");
    K.syncfs(f).unwrap();
    assert_eq!(err(K.syncfs(Fd::new(-1))), Errno::EBADF);
    close(f);
}

#[test]
fn k_sync_file_range() {
    let f = mem_file("sfr", &[0u8; 8192]);
    K.sync_file_range(f, 0, 0, SYNC_FILE_RANGE_WRITE).unwrap();
    assert_eq!(err(K.sync_file_range(f, -1, 0, 0)), Errno::EINVAL);
    assert_eq!(err(K.sync_file_range(f, 0, 0, 0x100)), Errno::EINVAL);
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    assert_eq!(err(K.sync_file_range(r, 0, 0, SYNC_FILE_RANGE_WRITE)), Errno::ESPIPE);
    for x in [r, w, f] {
        close(x);
    }
}

#[test]
fn k_fadvise64() {
    let t = Tmp::new("fadvise");
    let f = t.file("f", &[0u8; 4096]);
    K.fadvise(f, 0, 0, POSIX_FADV_SEQUENTIAL).unwrap();
    K.fadvise(f, 0, 4096, POSIX_FADV_DONTNEED).unwrap();
    assert_eq!(err(K.fadvise(f, 0, 0, 99)), Errno::EINVAL);
    assert_eq!(err(K.fadvise(f, 0, -1, POSIX_FADV_NORMAL)), Errno::EINVAL);
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    assert_eq!(err(K.fadvise(r, 0, 0, POSIX_FADV_NORMAL)), Errno::ESPIPE);
    for x in [r, w, f] {
        close(x);
    }
}

#[test]
fn k_fallocate() {
    let t = Tmp::new("fallocate");
    let f = t.file("f", b"");
    K.fallocate(f, 0, 0, 10_000).unwrap();
    assert_eq!(K.fstat(f).unwrap().size, 10_000);
    K.fallocate(f, FALLOC_FL_KEEP_SIZE, 10_000, 4096).unwrap();
    assert_eq!(K.fstat(f).unwrap().size, 10_000, "KEEP_SIZE does not extend");
    // Punching a hole needs KEEP_SIZE; the hole reads as zeros.
    K.pwrite(f, &[1u8; 4096], 4096).unwrap();
    match K.fallocate(f, FALLOC_FL_PUNCH_HOLE | FALLOC_FL_KEEP_SIZE, 4096, 4096) {
        Ok(()) => {
            let mut b = [9u8; 4];
            K.pread(f, &mut b, 4096).unwrap();
            assert_eq!(b, [0; 4]);
        }
        Err(e) => assert_eq!(e, Errno::EOPNOTSUPP, "filesystem without hole punching"),
    }
    // PUNCH_HOLE without KEEP_SIZE: EOPNOTSUPP (⚑ KERN).
    assert_eq!(err(K.fallocate(f, FALLOC_FL_PUNCH_HOLE, 0, 1)), Errno::EOPNOTSUPP);
    assert_eq!(err(K.fallocate(f, 0, 0, 0)), Errno::EINVAL);
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    assert_eq!(err(K.fallocate(w, 0, 0, 1)), Errno::ESPIPE);
    assert_eq!(err(K.fallocate(r, 0, 0, 1)), Errno::EBADF, "not open for writing: checked first");
    for x in [r, w, f] {
        close(x);
    }
}

#[test]
fn k_ftruncate() {
    let t = Tmp::new("ftruncate");
    let f = t.file("f", b"0123456789");
    K.ftruncate(f, 4).unwrap();
    assert_eq!(K.fstat(f).unwrap().size, 4);
    K.ftruncate(f, 100).unwrap();
    let mut b = [9u8; 2];
    K.pread(f, &mut b, 50).unwrap();
    assert_eq!(b, [0, 0], "extension reads as zeros");
    assert_eq!(err(K.ftruncate(f, -1)), Errno::EINVAL);
    let ro = K.openat(Fd::CWD, t.path("f").as_kstr(), O_RDONLY, 0).unwrap();
    assert_eq!(err(K.ftruncate(ro, 0)), Errno::EINVAL);
    close(ro);
    close(f);
}

#[test]
fn k_truncate() {
    let t = Tmp::new("truncate");
    let f = t.file("f", b"0123456789");
    K.truncate(t.path("f").as_kstr(), 3).unwrap();
    assert_eq!(K.fstat(f).unwrap().size, 3);
    assert_eq!(err(K.truncate(t.path("none").as_kstr(), 0)), Errno::ENOENT);
    assert_eq!(err(K.truncate(t.dir().as_kstr(), 0)), Errno::EISDIR);
    assert_eq!(err(K.truncate(t.path("f").as_kstr(), -1)), Errno::EINVAL);
    close(f);
}

// ------------------------------------------------------------------------------ locks

#[test]
fn k_flock() {
    let t = Tmp::new("flock");
    let f = t.file("f", b"");
    K.flock(f, LOCK_EX).unwrap();
    // A separate open file description conflicts, even in the same process.
    let g = K.openat(Fd::CWD, t.path("f").as_kstr(), O_RDONLY | O_CLOEXEC, 0).unwrap();
    assert_eq!(err(K.flock(g, LOCK_SH | LOCK_NB)), Errno::EAGAIN);
    K.flock(f, LOCK_UN).unwrap();
    K.flock(g, LOCK_SH | LOCK_NB).unwrap();
    assert_eq!(err(K.flock(g, 0)), Errno::EINVAL);
    assert_eq!(err(K.flock(Fd::new(-1), LOCK_SH)), Errno::EBADF);
    close(g);
    close(f);
}

// ------------------------------------------------------------------------------ namespace and metadata

#[test]
fn k_newfstatat() {
    let t = Tmp::new("fstatat");
    let f = t.file("f", b"12345");
    let st = K.fstatat(Fd::CWD, t.path("f").as_kstr(), 0).unwrap();
    assert_eq!((st.size, st.mode & S_IFMT, st.nlink), (5, S_IFREG, 1));
    assert_eq!(st, K.fstat(f).unwrap(), "same object, same struct stat");
    K.symlinkat(kp("f").as_kstr(), Fd::CWD, t.path("ln").as_kstr()).unwrap();
    assert_eq!(K.fstatat(Fd::CWD, t.path("ln").as_kstr(), AT_SYMLINK_NOFOLLOW).unwrap().mode & S_IFMT, S_IFLNK);
    assert_eq!(K.fstatat(Fd::CWD, t.path("ln").as_kstr(), 0).unwrap().ino, st.ino);
    // AT_EMPTY_PATH operates on the descriptor itself.
    assert_eq!(K.fstatat(f, kp("").as_kstr(), AT_EMPTY_PATH).unwrap().ino, st.ino);
    assert_eq!(err(K.fstatat(f, kp("").as_kstr(), 0)), Errno::ENOENT);
    assert_eq!(err(K.fstatat(Fd::CWD, t.path("f/x").as_kstr(), 0)), Errno::ENOTDIR);
    assert_eq!(err(K.fstatat(Fd::CWD, t.path("f").as_kstr(), 0x1)), Errno::EINVAL);
    close(f);
}

#[test]
fn k_fstat() {
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    assert_eq!(K.fstat(r).unwrap().mode & S_IFMT, S_IFIFO);
    let null = K.openat(Fd::CWD, kp("/dev/null").as_kstr(), O_RDONLY, 0).unwrap();
    let st = K.fstat(null).unwrap();
    assert_eq!(st.mode & S_IFMT, S_IFCHR);
    assert_eq!(st.rdev, (1 << 8) | 3, "/dev/null is 1:3 (old-style dev_t encoding for small numbers)");
    assert_eq!(err(K.fstat(Fd::new(-1))), Errno::EBADF);
    for x in [null, r, w] {
        close(x);
    }
}

#[test]
fn k_statx() {
    let t = Tmp::new("statx");
    let f = t.file("f", b"abc");
    let sx = K.statx(Fd::CWD, t.path("f").as_kstr(), 0, STATX_BASIC_STATS | STATX_BTIME).unwrap();
    let st = K.fstat(f).unwrap();
    assert_eq!(sx.mask & STATX_BASIC_STATS, STATX_BASIC_STATS);
    assert_eq!((sx.size, sx.ino, u32::from(sx.mode)), (3, st.ino, st.mode));
    assert_eq!((sx.mtime.sec, i64::from(sx.mtime.nsec)), (st.mtime.sec, st.mtime.nsec), "statx round trip");
    assert_eq!(err(K.statx(Fd::CWD, t.path("f").as_kstr(), 0, 0x8000_0000)), Errno::EINVAL);
    assert_eq!(err(K.statx(Fd::CWD, t.path("f").as_kstr(), 0x7000_0000, 0)), Errno::EINVAL);
    assert_eq!(err(K.statx(Fd::CWD, t.path("none").as_kstr(), 0, STATX_BASIC_STATS)), Errno::ENOENT);
    close(f);
}

#[test]
fn k_statfs() {
    let s = K.statfs(kp("/proc").as_kstr()).unwrap();
    assert_eq!(s.ftype, 0x9fa0, "PROC_SUPER_MAGIC");
    assert!(s.bsize > 0 && s.namelen > 0);
    assert_eq!(err(K.statfs(kp("/nonexistent/x").as_kstr())), Errno::ENOENT);
}

#[test]
fn k_fstatfs() {
    let fd = K.openat(Fd::CWD, kp("/proc").as_kstr(), O_RDONLY | O_DIRECTORY, 0).unwrap();
    assert_eq!(K.fstatfs(fd).unwrap().ftype, 0x9fa0);
    close(fd);
    assert_eq!(err(K.fstatfs(Fd::new(-1))), Errno::EBADF);
}

#[test]
fn k_getdents64() {
    let t = Tmp::new("getdents");
    for n in ["a", "bb", "ccc"] {
        close(t.file(n, b""));
    }
    let d = K.openat(Fd::CWD, t.dir().as_kstr(), O_RDONLY | O_DIRECTORY | O_CLOEXEC, 0).unwrap();
    let mut buf = [0u8; 4096];
    let mut names = Vec::new();
    loop {
        let n = K.getdents64(d, &mut buf).unwrap();
        if n == 0 {
            break;
        }
        // struct linux_dirent64: ino u64, off i64, reclen u16, type u8, name.
        let mut i = 0;
        while i < n {
            let reclen = u16::from_ne_bytes([buf[i + 16], buf[i + 17]]) as usize;
            let name = &buf[i + 19..i + reclen];
            let name = &name[..name.iter().position(|&b| b == 0).unwrap()];
            names.push(String::from_utf8(name.to_vec()).unwrap());
            i += reclen;
        }
    }
    names.sort();
    assert_eq!(names, [".", "..", "a", "bb", "ccc"]);
    // A buffer too small for one record: EINVAL. Not a directory: ENOTDIR.
    assert_eq!(K.lseek(d, 0, SEEK_SET).unwrap(), 0);
    assert_eq!(err(K.getdents64(d, &mut [0u8; 8])), Errno::EINVAL);
    close(d);
    let f = t.file("f", b"");
    assert_eq!(err(K.getdents64(f, &mut buf)), Errno::ENOTDIR);
    close(f);
}

#[test]
fn k_getcwd() {
    let mut buf = [0u8; 4096];
    let n = K.getcwd(&mut buf).unwrap();
    assert_eq!(buf[n - 1], 0, "the length includes the terminator");
    assert_eq!(std::str::from_utf8(&buf[..n - 1]).unwrap(), std::env::current_dir().unwrap().to_str().unwrap());
    assert_eq!(err(K.getcwd(&mut buf[..1])), Errno::ERANGE);
    // A deleted working directory: ENOENT (in a child).
    let _gone = ChildPath::new("getcwd");
    in_child(|k| {
        let path = child_path(k, "getcwd");
        k.mkdirat(Fd::CWD, path.as_kstr(), 0o700).or_exit();
        k.chdir(path.as_kstr()).or_exit();
        k.unlinkat(Fd::CWD, path.as_kstr(), AT_REMOVEDIR).or_exit();
        let mut buf = [0u8; 256];
        i32::from(k.getcwd(&mut buf) != Err(Errno::ENOENT))
    });
}

#[test]
fn k_chdir() {
    in_child(|k| {
        k.chdir(kp("/proc").as_kstr()).or_exit();
        let mut buf = [0u8; 64];
        let n = k.getcwd(&mut buf).or_exit();
        if &buf[..n] != b"/proc\0" {
            return 1;
        }
        if k.chdir(kp("/proc/self/status").as_kstr()) != Err(Errno::ENOTDIR) {
            return 2;
        }
        i32::from(k.chdir(kp("/nonexistent").as_kstr()) != Err(Errno::ENOENT)) * 3
    });
}

#[test]
fn k_fchdir() {
    in_child(|k| {
        let d = k.openat(Fd::CWD, kp("/sys").as_kstr(), O_RDONLY | O_DIRECTORY, 0).or_exit();
        k.fchdir(d).or_exit();
        let mut buf = [0u8; 64];
        let n = k.getcwd(&mut buf).or_exit();
        if &buf[..n] != b"/sys\0" {
            return 1;
        }
        let f = k.openat(Fd::CWD, kp("/proc/self/status").as_kstr(), O_RDONLY, 0).or_exit();
        i32::from(k.fchdir(f) != Err(Errno::ENOTDIR) || k.fchdir(Fd::new(-1)) != Err(Errno::EBADF)) * 2
    });
}

#[test]
fn k_chroot() {
    if skip_without_userns("k_chroot") {
        return;
    }
    assert_eq!(err(K.chroot(kp("/tmp").as_kstr())), Errno::EPERM);
    in_child(|k| {
        become_ns_root(k, 0).or_exit();
        if k.chroot(kp("/proc/self/status").as_kstr()) != Err(Errno::ENOTDIR) {
            return 1;
        }
        k.chroot(kp("/proc").as_kstr()).or_exit();
        // Inside the new root, "/self" is the old "/proc/self".
        i32::from(k.fstatat(Fd::CWD, kp("/self/status").as_kstr(), 0).is_err()) * 2
    });
}

#[test]
fn k_mkdirat() {
    let t = Tmp::new("mkdirat");
    K.mkdirat(Fd::CWD, t.path("d").as_kstr(), 0o755).unwrap();
    assert_eq!(K.fstatat(Fd::CWD, t.path("d").as_kstr(), 0).unwrap().mode & S_IFMT, S_IFDIR);
    assert_eq!(err(K.mkdirat(Fd::CWD, t.path("d").as_kstr(), 0o755)), Errno::EEXIST);
    assert_eq!(err(K.mkdirat(Fd::CWD, t.path("x/y").as_kstr(), 0o755)), Errno::ENOENT);
    assert_eq!(err(K.mkdirat(Fd::CWD, kp("/proc/liella").as_kstr(), 0o755)), Errno::ENOENT);
}

#[test]
fn k_mknodat() {
    let t = Tmp::new("mknodat");
    K.mknodat(Fd::CWD, t.path("fifo").as_kstr(), S_IFIFO | 0o600, 0).unwrap();
    assert_eq!(K.fstatat(Fd::CWD, t.path("fifo").as_kstr(), 0).unwrap().mode & S_IFMT, S_IFIFO);
    K.mknodat(Fd::CWD, t.path("reg").as_kstr(), S_IFREG | 0o600, 0).unwrap();
    // Device nodes need CAP_MKNOD: EPERM.
    assert_eq!(err(K.mknodat(Fd::CWD, t.path("null").as_kstr(), S_IFCHR | 0o600, (1 << 8) | 3)), Errno::EPERM);
    assert_eq!(err(K.mknodat(Fd::CWD, t.path("dir").as_kstr(), S_IFDIR | 0o700, 0)), Errno::EPERM, "mknod never makes directories");
    assert_eq!(err(K.mknodat(Fd::CWD, t.path("bad").as_kstr(), 0o170000 | 0o600, 0)), Errno::EINVAL);
}

#[test]
fn k_linkat() {
    let t = Tmp::new("linkat");
    let f = t.file("f", b"x");
    K.linkat(Fd::CWD, t.path("f").as_kstr(), Fd::CWD, t.path("g").as_kstr(), 0).unwrap();
    assert_eq!(K.fstat(f).unwrap().nlink, 2);
    assert_eq!(err(K.linkat(Fd::CWD, t.path("f").as_kstr(), Fd::CWD, t.path("g").as_kstr(), 0)), Errno::EEXIST);
    assert_eq!(err(K.linkat(Fd::CWD, t.dir().as_kstr(), Fd::CWD, t.path("d2").as_kstr(), 0)), Errno::EPERM);
    assert_eq!(err(K.linkat(Fd::CWD, t.path("f").as_kstr(), Fd::CWD, t.path("h").as_kstr(), 0x1)), Errno::EINVAL);
    // Across filesystems: EXDEV. The other side is found, not assumed: the first of a few
    // directories on a different device than the scratch directory (the scratch directory may be
    // on tmpfs or in /dev/shm itself, where a link would succeed), under a name of this process.
    // The kernel checks the mounts after the new name's lookup and before any permission check,
    // so a directory this user cannot write to still answers EXDEV; a read-only one is skipped.
    use std::os::unix::fs::MetadataExt;
    let here = std::fs::metadata(&t.0).unwrap().dev();
    let mut tried = false;
    for other in ["/dev/shm", "/tmp", "/var/tmp", "/run", "/dev"] {
        if !std::fs::metadata(other).is_ok_and(|m| m.dev() != here && m.is_dir()) {
            continue;
        }
        let name = kp(&format!("{other}/liella-linkat-{}", std::process::id()));
        match K.linkat(Fd::CWD, t.path("f").as_kstr(), Fd::CWD, name.as_kstr(), 0) {
            Err(Errno::EXDEV) => tried = true,
            Err(Errno::EROFS) => continue,
            r => {
                let _ = K.unlinkat(Fd::CWD, name.as_kstr(), 0);
                panic!("link into {other} (another device than {here:#x}): {r:?}, expected EXDEV");
            }
        }
        break;
    }
    if !tried {
        skip_note::skipped("skipped: k_linkat (EXDEV part) found no writable directory on another device than the scratch directory");
    }
    close(f);
}

#[test]
fn k_symlinkat() {
    let t = Tmp::new("symlinkat");
    K.symlinkat(kp("target/that/need/not/exist").as_kstr(), Fd::CWD, t.path("l").as_kstr()).unwrap();
    let mut buf = [0u8; 64];
    let n = K.readlinkat(Fd::CWD, t.path("l").as_kstr(), &mut buf).unwrap();
    assert_eq!(&buf[..n], b"target/that/need/not/exist");
    assert_eq!(err(K.symlinkat(kp("x").as_kstr(), Fd::CWD, t.path("l").as_kstr())), Errno::EEXIST);
    assert_eq!(err(K.symlinkat(kp("").as_kstr(), Fd::CWD, t.path("m").as_kstr())), Errno::ENOENT);
}

#[test]
fn k_readlinkat() {
    let t = Tmp::new("readlinkat");
    K.symlinkat(kp("0123456789").as_kstr(), Fd::CWD, t.path("l").as_kstr()).unwrap();
    // Silent truncation, no terminator (⚑ KERN).
    let mut small = [0u8; 4];
    assert_eq!(K.readlinkat(Fd::CWD, t.path("l").as_kstr(), &mut small).unwrap(), 4);
    assert_eq!(&small, b"0123");
    let f = t.file("f", b"");
    assert_eq!(err(K.readlinkat(Fd::CWD, t.path("f").as_kstr(), &mut small)), Errno::EINVAL);
    assert_eq!(err(K.readlinkat(Fd::CWD, t.path("l").as_kstr(), &mut [])), Errno::EINVAL);
    close(f);
}

#[test]
fn k_unlinkat() {
    let t = Tmp::new("unlinkat");
    close(t.file("f", b""));
    K.mkdirat(Fd::CWD, t.path("d").as_kstr(), 0o700).unwrap();
    assert_eq!(err(K.unlinkat(Fd::CWD, t.path("d").as_kstr(), 0)), Errno::EISDIR);
    assert_eq!(err(K.unlinkat(Fd::CWD, t.path("f").as_kstr(), AT_REMOVEDIR)), Errno::ENOTDIR);
    K.unlinkat(Fd::CWD, t.path("f").as_kstr(), 0).unwrap();
    close(t.file("d/inner", b""));
    assert_eq!(err(K.unlinkat(Fd::CWD, t.path("d").as_kstr(), AT_REMOVEDIR)), Errno::ENOTEMPTY);
    K.unlinkat(Fd::CWD, t.path("d/inner").as_kstr(), 0).unwrap();
    K.unlinkat(Fd::CWD, t.path("d").as_kstr(), AT_REMOVEDIR).unwrap();
    assert_eq!(err(K.unlinkat(Fd::CWD, t.path("f").as_kstr(), 0)), Errno::ENOENT);
    assert_eq!(err(K.unlinkat(Fd::CWD, t.path("f").as_kstr(), 0x1)), Errno::EINVAL);
}

#[test]
fn k_renameat2() {
    let t = Tmp::new("renameat2");
    close(t.file("a", b"A"));
    close(t.file("b", b"B"));
    assert_eq!(err(K.renameat2(Fd::CWD, t.path("a").as_kstr(), Fd::CWD, t.path("b").as_kstr(), RENAME_NOREPLACE)), Errno::EEXIST);
    K.renameat2(Fd::CWD, t.path("a").as_kstr(), Fd::CWD, t.path("b").as_kstr(), RENAME_EXCHANGE).unwrap();
    assert_eq!(std::fs::read(t.s("a")).unwrap(), b"B");
    K.renameat2(Fd::CWD, t.path("a").as_kstr(), Fd::CWD, t.path("c").as_kstr(), 0).unwrap();
    assert_eq!(err(K.renameat2(Fd::CWD, t.path("a").as_kstr(), Fd::CWD, t.path("d").as_kstr(), 0)), Errno::ENOENT);
    assert_eq!(err(K.renameat2(Fd::CWD, t.path("b").as_kstr(), Fd::CWD, t.path("c").as_kstr(), RENAME_NOREPLACE | RENAME_EXCHANGE)), Errno::EINVAL);
    // A directory into its own subtree: EINVAL.
    K.mkdirat(Fd::CWD, t.path("d").as_kstr(), 0o700).unwrap();
    assert_eq!(err(K.renameat2(Fd::CWD, t.path("d").as_kstr(), Fd::CWD, t.path("d/sub").as_kstr(), 0)), Errno::EINVAL);
}

#[test]
fn k_fchmodat() {
    let t = Tmp::new("fchmodat");
    close(t.file("f", b""));
    K.fchmodat(Fd::CWD, t.path("f").as_kstr(), 0o604).unwrap();
    assert_eq!(K.fstatat(Fd::CWD, t.path("f").as_kstr(), 0).unwrap().mode & 0o7777, 0o604);
    assert_eq!(err(K.fchmodat(Fd::CWD, t.path("none").as_kstr(), 0o600)), Errno::ENOENT);
    assert_eq!(err(K.fchmodat(Fd::CWD, kp("/").as_kstr(), 0o755)), Errno::EPERM);
}

#[test]
fn k_fchmodat2() {
    // Linux 6.6+. On older kernels (this suite's reference: 5.15) the call is ENOSYS.
    let t = Tmp::new("fchmodat2");
    close(t.file("f", b""));
    K.symlinkat(kp("f").as_kstr(), Fd::CWD, t.path("l").as_kstr()).unwrap();
    match K.fchmodat2(Fd::CWD, t.path("f").as_kstr(), 0o640, 0) {
        Ok(()) => {
            assert_eq!(K.fstatat(Fd::CWD, t.path("f").as_kstr(), 0).unwrap().mode & 0o777, 0o640);
            // Changing a symlink's own mode is not supported by Linux filesystems.
            assert_eq!(err(K.fchmodat2(Fd::CWD, t.path("l").as_kstr(), 0o600, AT_SYMLINK_NOFOLLOW)), Errno::EOPNOTSUPP);
        }
        Err(e) => assert_eq!(e, Errno::ENOSYS),
    }
}

#[test]
fn k_fchmod() {
    let t = Tmp::new("fchmod");
    let f = t.file("f", b"");
    K.fchmod(f, 0o4700).unwrap();
    assert_eq!(K.fstat(f).unwrap().mode & 0o7777, 0o4700);
    assert_eq!(err(K.fchmod(Fd::new(-1), 0o600)), Errno::EBADF);
    let p = K.openat(Fd::CWD, t.path("f").as_kstr(), O_PATH, 0).unwrap();
    assert_eq!(err(K.fchmod(p, 0o600)), Errno::EBADF, "O_PATH descriptors cannot change modes");
    close(p);
    close(f);
}

#[test]
fn k_fchownat() {
    let t = Tmp::new("fchownat");
    close(t.file("f", b""));
    // To one's own IDs (or "unchanged"): allowed.
    K.fchownat(Fd::CWD, t.path("f").as_kstr(), u32::MAX, K.getegid(), 0).unwrap();
    K.fchownat(Fd::CWD, t.path("f").as_kstr(), K.geteuid(), u32::MAX, AT_SYMLINK_NOFOLLOW).unwrap();
    assert_eq!(err(K.fchownat(Fd::CWD, t.path("f").as_kstr(), 0, u32::MAX, 0)), Errno::EPERM);
    assert_eq!(err(K.fchownat(Fd::CWD, t.path("f").as_kstr(), u32::MAX, u32::MAX, 0x1)), Errno::EINVAL);
}

#[test]
fn k_fchown() {
    let t = Tmp::new("fchown");
    let f = t.file("f", b"");
    K.fchown(f, u32::MAX, u32::MAX).unwrap();
    assert_eq!(err(K.fchown(f, 0, 0)), Errno::EPERM);
    assert_eq!(err(K.fchown(Fd::new(-1), 0, 0)), Errno::EBADF);
    close(f);
}

#[test]
fn k_umask() {
    in_child(|k| {
        let first = k.umask(0o027);
        let second = k.umask(first);
        // Only the permission bits are kept.
        let third = k.umask(0o7777);
        let fourth = k.umask(first);
        i32::from(second != 0o027 || third != first || fourth != 0o777)
    });
}

#[test]
fn k_faccessat() {
    let t = Tmp::new("faccessat");
    let f = t.file("f", b"");
    K.fchmod(f, 0o400).unwrap();
    K.faccessat(Fd::CWD, t.path("f").as_kstr(), R_OK).unwrap();
    assert_eq!(err(K.faccessat(Fd::CWD, t.path("f").as_kstr(), W_OK)), Errno::EACCES);
    assert_eq!(err(K.faccessat(Fd::CWD, t.path("none").as_kstr(), F_OK)), Errno::ENOENT);
    assert_eq!(err(K.faccessat(Fd::CWD, t.path("f").as_kstr(), 0x10)), Errno::EINVAL);
    close(f);
}

#[test]
fn k_faccessat2() {
    let t = Tmp::new("faccessat2");
    let f = t.file("f", b"");
    K.faccessat2(Fd::CWD, t.path("f").as_kstr(), R_OK | W_OK, AT_EACCESS).unwrap();
    K.faccessat2(f, kp("").as_kstr(), R_OK, AT_EMPTY_PATH).unwrap();
    assert_eq!(err(K.faccessat2(Fd::CWD, t.path("f").as_kstr(), R_OK, 0x1)), Errno::EINVAL);
    close(f);
}

#[test]
fn k_utimensat() {
    let t = Tmp::new("utimensat");
    let f = t.file("f", b"");
    let times = [Timespec { sec: 1_000_000_000, nsec: 5 }, Timespec { sec: 1_234_567_890, nsec: 123 }];
    K.utimensat(Fd::CWD, Some(t.path("f").as_kstr()), Some(&times), 0).unwrap();
    let st = K.fstat(f).unwrap();
    assert_eq!((st.atime, st.mtime), (times[0], times[1]));
    // UTIME_OMIT leaves a time alone; a NULL path works on the descriptor itself.
    let keep = [Timespec { sec: 0, nsec: UTIME_OMIT }, Timespec { sec: 0, nsec: UTIME_NOW }];
    K.utimensat(f, None, Some(&keep), 0).unwrap();
    let st2 = K.fstat(f).unwrap();
    assert_eq!(st2.atime, times[0]);
    assert!(st2.mtime.sec > times[1].sec);
    let bad = [Timespec { sec: 0, nsec: 1_000_000_000 }, Timespec::default()];
    assert_eq!(err(K.utimensat(f, None, Some(&bad), 0)), Errno::EINVAL);
    // Another user's file with explicit times: EPERM.
    assert_eq!(err(K.utimensat(Fd::CWD, Some(kp("/").as_kstr()), Some(&times), 0)), Errno::EPERM);
    close(f);
}

#[test]
fn k_xattr() {
    // user.* attributes on a tmpfs or ext4 scratch file (tmpfs supports user xattrs from 6.6;
    // /tmp on this system decides). Falls back to the ENOTSUP/EOPNOTSUPP answer honestly.
    let t = Tmp::new("xattr");
    let f = t.file("f", b"");
    let path = t.path("f");
    let name = kp("user.liella");
    match K.setxattr(path.as_kstr(), false, name.as_kstr(), b"v1", XATTR_CREATE) {
        Ok(()) => {
            let mut buf = [0u8; 16];
            assert_eq!(K.getxattr(path.as_kstr(), false, name.as_kstr(), &mut []).unwrap(), 2, "size query");
            assert_eq!(K.getxattr(path.as_kstr(), true, name.as_kstr(), &mut buf).unwrap(), 2);
            assert_eq!(&buf[..2], b"v1");
            assert_eq!(err(K.setxattr(path.as_kstr(), false, name.as_kstr(), b"v2", XATTR_CREATE)), Errno::EEXIST);
            K.fsetxattr(f, name.as_kstr(), b"v22", XATTR_REPLACE).unwrap();
            assert_eq!(K.fgetxattr(f, name.as_kstr(), &mut buf).unwrap(), 3);
            assert_eq!(err(K.fgetxattr(f, name.as_kstr(), &mut buf[..1])), Errno::ERANGE);
            let n = K.listxattr(path.as_kstr(), false, &mut buf).unwrap();
            assert_eq!(&buf[..n], b"user.liella\0");
            assert_eq!(K.flistxattr(f, &mut buf).unwrap(), n);
            assert_eq!(K.listxattr(path.as_kstr(), true, &mut buf).unwrap(), n);
            K.removexattr(path.as_kstr(), true, name.as_kstr()).unwrap();
            assert_eq!(err(K.fremovexattr(f, name.as_kstr())), Errno::ENODATA);
            assert_eq!(err(K.getxattr(path.as_kstr(), false, name.as_kstr(), &mut buf)), Errno::ENODATA);
            K.fsetxattr(f, name.as_kstr(), b"", 0).unwrap();
            K.removexattr(path.as_kstr(), false, name.as_kstr()).unwrap();
            K.setxattr(path.as_kstr(), true, name.as_kstr(), b"x", 0).unwrap();
            K.fremovexattr(f, name.as_kstr()).unwrap();
        }
        Err(e) => assert_eq!(e, Errno::EOPNOTSUPP, "filesystem without user xattrs"),
    }
    // Invalid namespace prefix: EOPNOTSUPP. Too long a name: ERANGE.
    assert_eq!(err(K.setxattr(path.as_kstr(), false, kp("bogus.x").as_kstr(), b"", 0)), Errno::EOPNOTSUPP);
    assert_eq!(err(K.getxattr(path.as_kstr(), false, kp(&format!("user.{}", "n".repeat(300))).as_kstr(), &mut [])), Errno::ERANGE);
    // Unknown flags: EINVAL.
    assert_eq!(err(K.fsetxattr(f, name.as_kstr(), b"", 0x10)), Errno::EINVAL);
    close(f);
}

// ------------------------------------------------------------------------------ mounting

/// A path a forked child creates ([`child_path`]): under the target directory, named after this
/// process (the child's parent) and `tag` (unique per test). This process removes whatever is
/// there when the guard goes, so a child that fails, or cannot clean up (a mount point in its own
/// namespace), leaves nothing behind.
struct ChildPath(String);

impl ChildPath {
    fn new(tag: &str) -> ChildPath {
        let p = format!("{}/liella-child-{}-{tag}", env!("CARGO_TARGET_TMPDIR"), std::process::id());
        let _ = std::fs::remove_dir_all(&p);
        ChildPath(p)
    }
}

impl Drop for ChildPath {
    fn drop(&mut self) {
        if std::fs::remove_dir_all(&self.0).is_err() {
            let _ = std::fs::remove_file(&self.0);
        }
    }
}

/// In a forked child: the path its parent's [`ChildPath`] for `tag` names (no allocation).
fn child_path(k: &CSys, tag: &str) -> KPathBuf {
    let mut d = [0u8; 16];
    let digits = dec(k.getppid() as u32, &mut d);
    let parts: [&[u8]; 5] = [env!("CARGO_TARGET_TMPDIR").as_bytes(), b"/liella-child-", digits, b"-", tag.as_bytes()];
    let mut name = [0u8; 512];
    let mut n = 0;
    for part in parts {
        name[n..n + part.len()].copy_from_slice(part);
        n += part.len();
    }
    KPathBuf::new(&name[..n]).unwrap()
}

/// In a child: become root of new user + mount namespaces and mount a tmpfs at a fresh
/// directory (the parent's [`ChildPath`] for `tag`), returning its path.
fn ns_tmpfs(k: &CSys, tag: &str) -> KPathBuf {
    become_ns_root(k, CLONE_NEWNS).unwrap();
    let path = child_path(k, tag);
    let _ = k.mkdirat(Fd::CWD, path.as_kstr(), 0o700);
    // Make every mount private so nothing propagates out of the namespace.
    k.mount(None, kp("/").as_kstr(), None, MS_REC | MS_PRIVATE, None).unwrap();
    k.mount(Some(kp("none").as_kstr()), path.as_kstr(), Some(kp("tmpfs").as_kstr()), 0, None).unwrap();
    path
}

/// A file inside `dir` (no allocation: used in forked children).
fn under(dir: &KPathBuf, name: &str) -> KPathBuf {
    let d = dir.as_kstr().bytes();
    let mut buf = [0u8; 512];
    let n = d.len() + 1 + name.len();
    buf[..d.len()].copy_from_slice(d);
    buf[d.len()] = b'/';
    buf[d.len() + 1..n].copy_from_slice(name.as_bytes());
    KPathBuf::new(&buf[..n]).unwrap()
}

#[test]
fn k_mount() {
    if skip_without_userns("k_mount") {
        return;
    }
    assert_eq!(err(K.mount(Some(kp("none").as_kstr()), kp("/tmp").as_kstr(), Some(kp("tmpfs").as_kstr()), 0, None)), Errno::EPERM);
    let _mnt = ChildPath::new("mount");
    in_child(|k| {
        let m = ns_tmpfs(k, "mount");
        let s = k.statfs(m.as_kstr()).or_exit();
        if s.ftype != 0x0102_1994 {
            return 1; // TMPFS_MAGIC
        }
        // Read-only remount, then a write fails with EROFS.
        k.mount(None, m.as_kstr(), None, MS_REMOUNT | MS_RDONLY, None).or_exit();
        if k.mkdirat(Fd::CWD, under(&m, "x").as_kstr(), 0o700) != Err(Errno::EROFS) {
            return 2;
        }
        // An unknown filesystem type: ENODEV.
        i32::from(k.mount(Some(kp("none").as_kstr()), m.as_kstr(), Some(kp("liellafs").as_kstr()), 0, None) != Err(Errno::ENODEV)) * 3
    });
}

#[test]
fn k_umount2() {
    if skip_without_userns("k_umount2") {
        return;
    }
    assert_eq!(err(K.umount2(kp("/proc").as_kstr(), 0)), Errno::EPERM);
    let _mnt = ChildPath::new("umount2");
    in_child(|k| {
        let m = ns_tmpfs(k, "umount2");
        k.umount2(m.as_kstr(), 0).or_exit();
        if k.umount2(m.as_kstr(), 0) != Err(Errno::EINVAL) {
            return 1; // no longer a mount point
        }
        i32::from(k.umount2(m.as_kstr(), 0x100) != Err(Errno::EINVAL)) * 2
    });
}

#[test]
fn k_pivot_root() {
    if skip_without_userns("k_pivot_root") {
        return;
    }
    let _mnt = ChildPath::new("pivot_root");
    in_child(|k| {
        let m = ns_tmpfs(k, "pivot_root");
        let old = under(&m, "old");
        k.mkdirat(Fd::CWD, old.as_kstr(), 0o700).or_exit();
        // put_old on the current root's own mount: EBUSY. /etc, not /tmp: /tmp may be a mount
        // of its own (the isolated gate's tmpfs).
        if k.pivot_root(m.as_kstr(), kp("/etc").as_kstr()) != Err(Errno::EBUSY) {
            return 1;
        }
        k.pivot_root(m.as_kstr(), old.as_kstr()).or_exit();
        k.chdir(kp("/").as_kstr()).or_exit();
        // The old root is now at /old.
        if k.fstatat(Fd::CWD, kp("/old/proc").as_kstr(), 0).is_err() {
            return 2;
        }
        i32::from(k.umount2(kp("/old").as_kstr(), MNT_DETACH).is_err()) * 3
    });
}

#[test]
fn k_fsopen() {
    if skip_without_userns("k_fsopen") {
        return;
    }
    assert_eq!(err(K.fsopen(kp("tmpfs").as_kstr(), FSOPEN_CLOEXEC)), Errno::EPERM);
    in_child(|k| {
        become_ns_root(k, CLONE_NEWNS).or_exit();
        let fs = k.fsopen(kp("tmpfs").as_kstr(), FSOPEN_CLOEXEC).or_exit();
        let _ = k.close(fs);
        if k.fsopen(kp("liellafs").as_kstr(), 0) != Err(Errno::ENODEV) {
            return 1;
        }
        i32::from(k.fsopen(kp("tmpfs").as_kstr(), 0x2) != Err(Errno::EINVAL)) * 2
    });
}

#[test]
fn k_fsconfig() {
    if skip_without_userns("k_fsconfig") {
        return;
    }
    in_child(|k| {
        become_ns_root(k, CLONE_NEWNS).or_exit();
        let fs = k.fsopen(kp("tmpfs").as_kstr(), FSOPEN_CLOEXEC).or_exit();
        k.fsconfig(fs, FsconfigCmd::SetString, Some(kp("size").as_kstr()), FsValue::Str(kp("1m").as_kstr()), 0).or_exit();
        k.fsconfig(fs, FsconfigCmd::SetFlag, Some(kp("ro").as_kstr()), FsValue::None, 0).or_exit();
        if k.fsconfig(fs, FsconfigCmd::SetString, Some(kp("nosuchoption").as_kstr()), FsValue::Str(kp("1").as_kstr()), 0) != Err(Errno::EINVAL) {
            return 1;
        }
        // A string value for a flag-type command: EINVAL (value must be NULL).
        if k.fsconfig(fs, FsconfigCmd::SetFlag, Some(kp("ro").as_kstr()), FsValue::Binary(b"x"), 0) != Err(Errno::EINVAL) {
            return 2;
        }
        i32::from(k.fsconfig(fs, FsconfigCmd::CmdCreate, None, FsValue::None, 0).is_err()) * 3
    });
}

#[test]
fn k_fsmount() {
    if skip_without_userns("k_fsmount") {
        return;
    }
    let _mnt = ChildPath::new("fsmount");
    in_child(|k| {
        let m = ns_tmpfs(k, "fsmount");
        let fs = k.fsopen(kp("tmpfs").as_kstr(), FSOPEN_CLOEXEC).or_exit();
        // Before CMD_CREATE the context is not ready to mount: EINVAL.
        if k.fsmount(fs, FSMOUNT_CLOEXEC, 0) != Err(Errno::EINVAL) {
            return 1;
        }
        k.fsconfig(fs, FsconfigCmd::CmdCreate, None, FsValue::None, 0).or_exit();
        let mnt = k.fsmount(fs, FSMOUNT_CLOEXEC, 0).or_exit();
        // Attach it with move_mount and see a fresh tmpfs there.
        let target = under(&m, "sub");
        k.mkdirat(Fd::CWD, target.as_kstr(), 0o700).or_exit();
        k.move_mount(mnt, kp("").as_kstr(), Fd::CWD, target.as_kstr(), MOVE_MOUNT_F_EMPTY_PATH).or_exit();
        i32::from(k.fsmount(fs, 0x10, 0) != Err(Errno::EINVAL)) * 2
    });
}

#[test]
fn k_fspick() {
    if skip_without_userns("k_fspick") {
        return;
    }
    let _mnt = ChildPath::new("fspick");
    in_child(|k| {
        let m = ns_tmpfs(k, "fspick");
        let fc = k.fspick(Fd::CWD, m.as_kstr(), FSPICK_CLOEXEC).or_exit();
        k.fsconfig(fc, FsconfigCmd::SetFlag, Some(kp("ro").as_kstr()), FsValue::None, 0).or_exit();
        k.fsconfig(fc, FsconfigCmd::CmdReconfigure, None, FsValue::None, 0).or_exit();
        if k.mkdirat(Fd::CWD, under(&m, "x").as_kstr(), 0o700) != Err(Errno::EROFS) {
            return 1;
        }
        // Not a mount root: EINVAL.
        k.mount(None, m.as_kstr(), None, MS_REMOUNT, None).or_exit();
        k.mkdirat(Fd::CWD, under(&m, "d").as_kstr(), 0o700).or_exit();
        i32::from(k.fspick(Fd::CWD, under(&m, "d").as_kstr(), 0) != Err(Errno::EINVAL)) * 2
    });
}

#[test]
fn k_move_mount() {
    if skip_without_userns("k_move_mount") {
        return;
    }
    let _mnt = ChildPath::new("move_mount");
    in_child(|k| {
        let m = ns_tmpfs(k, "move_mount");
        let a = under(&m, "a");
        let b = under(&m, "b");
        k.mkdirat(Fd::CWD, a.as_kstr(), 0o700).or_exit();
        k.mkdirat(Fd::CWD, b.as_kstr(), 0o700).or_exit();
        k.mount(Some(kp("none").as_kstr()), a.as_kstr(), Some(kp("tmpfs").as_kstr()), 0, None).or_exit();
        let marker = under(&a, "marker");
        let f = k.openat(Fd::CWD, marker.as_kstr(), O_CREAT | O_WRONLY, 0o600).or_exit();
        let _ = k.close(f);
        k.move_mount(Fd::CWD, a.as_kstr(), Fd::CWD, b.as_kstr(), 0).or_exit();
        if k.fstatat(Fd::CWD, under(&b, "marker").as_kstr(), 0).is_err() {
            return 1;
        }
        i32::from(k.move_mount(Fd::CWD, a.as_kstr(), Fd::CWD, b.as_kstr(), 0x1000_0000) != Err(Errno::EINVAL)) * 2
    });
}

#[test]
fn k_open_tree() {
    if skip_without_userns("k_open_tree") {
        return;
    }
    // Without OPEN_TREE_CLONE: an O_PATH-like descriptor, allowed unprivileged.
    let fd = K.open_tree(Fd::CWD, kp("/proc").as_kstr(), OPEN_TREE_CLOEXEC).unwrap();
    assert_eq!(K.fstatfs(fd).unwrap().ftype, 0x9fa0);
    close(fd);
    // Cloning a tree needs CAP_SYS_ADMIN: EPERM here, success as namespace root.
    assert_eq!(err(K.open_tree(Fd::CWD, kp("/proc").as_kstr(), OPEN_TREE_CLONE)), Errno::EPERM);
    let _mnt = ChildPath::new("open_tree");
    in_child(|k| {
        let m = ns_tmpfs(k, "open_tree");
        let t = k.open_tree(Fd::CWD, m.as_kstr(), OPEN_TREE_CLONE | OPEN_TREE_CLOEXEC).or_exit();
        let target = under(&m, "clone");
        k.mkdirat(Fd::CWD, target.as_kstr(), 0o700).or_exit();
        i32::from(k.move_mount(t, kp("").as_kstr(), Fd::CWD, target.as_kstr(), MOVE_MOUNT_F_EMPTY_PATH).is_err())
    });
    assert_eq!(err(K.open_tree(Fd::CWD, kp("/proc").as_kstr(), 0x2)), Errno::EINVAL);
}

#[test]
fn k_mount_setattr() {
    if skip_without_userns("k_mount_setattr") {
        return;
    }
    if kernel::before(5, 12) {
        kernel::note("mount_setattr", "5.12");
        let attr = MountAttr { attr_set: MOUNT_ATTR_RDONLY, attr_clr: 0, propagation: 0, userns_fd: 0 };
        assert_eq!(err(K.mount_setattr(Fd::CWD, kp("/").as_kstr(), 0, &attr)), Errno::ENOSYS);
        return;
    }
    let _mnt = ChildPath::new("mount_setattr");
    in_child(|k| {
        let m = ns_tmpfs(k, "mount_setattr");
        let attr = MountAttr { attr_set: MOUNT_ATTR_RDONLY, attr_clr: 0, propagation: 0, userns_fd: 0 };
        k.mount_setattr(Fd::CWD, m.as_kstr(), 0, &attr).or_exit();
        if k.mkdirat(Fd::CWD, under(&m, "x").as_kstr(), 0o700) != Err(Errno::EROFS) {
            return 1;
        }
        let bad = MountAttr { attr_set: 1 << 40, ..attr };
        i32::from(k.mount_setattr(Fd::CWD, m.as_kstr(), 0, &bad) != Err(Errno::EINVAL)) * 2
    });
}

#[test]
fn k_swapon() {
    // Destructive: error paths only. Unprivileged: EPERM (checked before the path).
    assert_eq!(err(K.swapon(kp("/nonexistent").as_kstr(), 0)), Errno::EPERM);
}

#[test]
fn k_swapoff() {
    assert_eq!(err(K.swapoff(kp("/nonexistent").as_kstr())), Errno::EPERM);
}

#[test]
fn k_quotactl() {
    // Q_SYNC with no device syncs every filesystem's quota: allowed unprivileged.
    K.quotactl(QuotaCmd::Sync, USRQUOTA, None, 0, &mut []).unwrap();
    // A missing block device: ENOENT. The boundary refuses a buffer shorter than the command
    // needs.
    let mut dq = [0u8; 72];
    assert_eq!(err(K.quotactl(QuotaCmd::GetQuota, USRQUOTA, Some(kp("/dev/liella-none").as_kstr()), 0, &mut dq)), Errno::ENOENT);
    assert_eq!(err(K.quotactl(QuotaCmd::GetQuota, USRQUOTA, Some(kp("/dev/null").as_kstr()), 0, &mut dq[..10])), Errno::EINVAL);
    // Q_QUOTAON: the device is resolved first, so a character device is ENOTBLK.
    assert_eq!(err(K.quotactl_on(USRQUOTA, kp("/dev/null").as_kstr(), 2, kp("/aquota.user").as_kstr())), Errno::ENOTBLK);
}

// ------------------------------------------------------------------------------ sockets

fn int_bytes(v: i32) -> [u8; 4] {
    v.to_ne_bytes()
}

/// A listening loopback TCP socket on an ephemeral port: (socket, encoded address).
fn tcp_listener() -> (Fd, Vec<u8>) {
    let s = K.socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0).unwrap();
    K.bind(s, &inet_loopback(0)).unwrap();
    K.listen(s, 8).unwrap();
    let mut a = SockAddrBuf::new();
    K.getsockname(s, &mut a).unwrap();
    (s, a.bytes().to_vec())
}

#[test]
fn k_socket() {
    let s = K.socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0).unwrap();
    assert_ne!(K.fcntl_int(s, FcntlInt::GetFl, 0).unwrap() & i64::from(O_NONBLOCK), 0);
    let mut ty = [0u8; 4];
    assert_eq!(K.getsockopt(s, SOL_SOCKET, SO_TYPE, &mut ty).unwrap(), 4);
    assert_eq!(i32::from_ne_bytes(ty), SOCK_STREAM as i32);
    close(s);
    assert_eq!(err(K.socket(9999, SOCK_STREAM, 0)), Errno::EAFNOSUPPORT);
    assert_eq!(err(K.socket(AF_INET, 99, 0)), Errno::EINVAL);
    assert_eq!(err(K.socket(AF_INET, SOCK_STREAM, 17)), Errno::EPROTONOSUPPORT);
    // Raw sockets need CAP_NET_RAW: EPERM.
    assert_eq!(err(K.socket(AF_INET, SOCK_RAW, 1)), Errno::EPERM);
}

#[test]
fn k_socketpair() {
    let [a, b] = K.socketpair(AF_UNIX, SOCK_SEQPACKET | SOCK_CLOEXEC, 0).unwrap();
    K.write(a, b"one").unwrap();
    K.write(a, b"two!").unwrap();
    let mut buf = [0u8; 16];
    assert_eq!(K.read(b, &mut buf).unwrap(), 3, "record boundaries kept");
    assert_eq!(K.read(b, &mut buf).unwrap(), 4);
    close(a);
    close(b);
    assert_eq!(err(K.socketpair(AF_INET, SOCK_STREAM, 0)), Errno::EOPNOTSUPP);
}

#[test]
fn k_bind() {
    let t = Tmp::sock("bind");
    let s = K.socket(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC, 0).unwrap();
    K.bind(s, &unix_addr(&t.s("sock"))).unwrap();
    assert_eq!(K.fstatat(Fd::CWD, t.path("sock").as_kstr(), 0).unwrap().mode & S_IFMT, S_IFSOCK);
    assert_eq!(err(K.bind(s, &unix_addr(&t.s("other")))), Errno::EINVAL, "already bound");
    let s2 = K.socket(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC, 0).unwrap();
    assert_eq!(err(K.bind(s2, &unix_addr(&t.s("sock")))), Errno::EADDRINUSE);
    assert_eq!(err(K.bind(s2, &[1])), Errno::EINVAL);
    // A privileged port: EACCES.
    let tcp = K.socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0).unwrap();
    assert_eq!(err(K.bind(tcp, &inet_loopback(1))), Errno::EACCES);
    for x in [tcp, s2, s] {
        close(x);
    }
}

#[test]
fn k_listen() {
    let (s, _) = tcp_listener();
    close(s);
    let u = K.socket(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC, 0).unwrap();
    assert_eq!(err(K.listen(u, 1)), Errno::EOPNOTSUPP);
    close(u);
    assert_eq!(err(K.listen(Fd::new(-1), 1)), Errno::EBADF);
}

#[test]
fn k_accept4() {
    let (l, addr) = tcp_listener();
    let c = K.socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0).unwrap();
    K.connect(c, &addr).unwrap();
    let mut peer = SockAddrBuf::new();
    let a = K.accept4(l, Some(&mut peer), SOCK_CLOEXEC).unwrap();
    assert_eq!(peer.reported_len(), 16);
    assert_eq!(&peer.bytes()[4..8], &[127, 0, 0, 1]);
    assert_eq!(K.fcntl_int(a, FcntlInt::GetFd, 0).unwrap(), FD_CLOEXEC);
    // Non-blocking with nothing pending: EAGAIN. Not listening: EINVAL. Bad flags: EINVAL.
    K.fcntl_int(l, FcntlInt::SetFl, i64::from(O_NONBLOCK)).unwrap();
    assert_eq!(err(K.accept4(l, None, 0)), Errno::EAGAIN);
    assert_eq!(err(K.accept4(c, None, 0)), Errno::EINVAL);
    assert_eq!(err(K.accept4(l, None, 0x1)), Errno::EINVAL);
    for x in [a, c, l] {
        close(x);
    }
}

#[test]
fn k_connect() {
    // A port that is bound but not listening (kept bound, so no other test can take it):
    // ECONNREFUSED.
    let b = K.socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0).unwrap();
    K.bind(b, &inet_loopback(0)).unwrap();
    let mut a = SockAddrBuf::new();
    K.getsockname(b, &mut a).unwrap();
    let c = K.socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0).unwrap();
    assert_eq!(err(K.connect(c, a.bytes())), Errno::ECONNREFUSED);
    close(c);
    close(b);
    let u = K.socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0).unwrap();
    assert_eq!(err(K.connect(u, &unix_addr("/nonexistent/sock"))), Errno::ENOENT);
    assert_eq!(err(K.connect(u, &inet_loopback(1))), Errno::EINVAL);
    close(u);
}

#[test]
fn k_shutdown() {
    let [a, b] = K.socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0).unwrap();
    K.shutdown(a, SHUT_WR).unwrap();
    let mut buf = [0u8; 4];
    assert_eq!(K.read(b, &mut buf).unwrap(), 0, "peer sees end of file");
    assert_eq!(err(K.write(a, b"x")), Errno::EPIPE);
    assert_eq!(err(K.shutdown(a, 9)), Errno::EINVAL);
    let (l, _) = tcp_listener();
    close(l);
    let t = K.socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0).unwrap();
    assert_eq!(err(K.shutdown(t, SHUT_RDWR)), Errno::ENOTCONN);
    for x in [t, a, b] {
        close(x);
    }
}

#[test]
fn k_getsockname() {
    let (l, addr) = tcp_listener();
    let mut a = SockAddrBuf::new();
    K.getsockname(l, &mut a).unwrap();
    assert_eq!(a.bytes(), &addr[..]);
    assert_ne!(u16::from_be_bytes([addr[2], addr[3]]), 0, "an ephemeral port was assigned");
    close(l);
    let t = Tmp::new("getsockname");
    let f = t.file("f", b"");
    assert_eq!(err(K.getsockname(f, &mut a)), Errno::ENOTSOCK);
    close(f);
}

#[test]
fn k_getpeername() {
    let (l, addr) = tcp_listener();
    let c = K.socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0).unwrap();
    let mut p = SockAddrBuf::new();
    assert_eq!(err(K.getpeername(c, &mut p)), Errno::ENOTCONN);
    K.connect(c, &addr).unwrap();
    K.getpeername(c, &mut p).unwrap();
    assert_eq!(p.bytes(), &addr[..]);
    close(c);
    close(l);
}

#[test]
fn k_getsockopt() {
    let s = K.socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0).unwrap();
    let mut v = [0u8; 4];
    assert_eq!(K.getsockopt(s, SOL_SOCKET, SO_ERROR, &mut v).unwrap(), 4);
    assert_eq!(i32::from_ne_bytes(v), 0);
    // A short buffer is truncated, not an error, for integer options (⚑ KERN).
    let mut one = [0u8; 1];
    assert!(K.getsockopt(s, SOL_SOCKET, SO_TYPE, &mut one).unwrap() <= 4);
    assert_eq!(err(K.getsockopt(s, SOL_SOCKET, 9999, &mut v)), Errno::ENOPROTOOPT);
    // SO_PEERCRED on a socketpair: our own credentials (struct ucred).
    let [a, b] = K.socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0).unwrap();
    let mut cred = [0u8; 12];
    assert_eq!(K.getsockopt(a, SOL_SOCKET, SO_PEERCRED, &mut cred).unwrap(), 12);
    assert_eq!(i32::from_ne_bytes([cred[0], cred[1], cred[2], cred[3]]), K.getpid());
    for x in [a, b, s] {
        close(x);
    }
}

#[test]
fn k_setsockopt() {
    let s = K.socket(AF_INET, SOCK_STREAM | SOCK_CLOEXEC, 0).unwrap();
    K.setsockopt(s, SockLevel::Socket, SO_REUSEADDR, &int_bytes(1)).unwrap();
    let mut v = [0u8; 4];
    K.getsockopt(s, SOL_SOCKET, SO_REUSEADDR, &mut v).unwrap();
    assert_eq!(i32::from_ne_bytes(v), 1);
    K.setsockopt(s, SockLevel::Socket, SO_SNDBUF, &int_bytes(8192)).unwrap();
    K.getsockopt(s, SOL_SOCKET, SO_SNDBUF, &mut v).unwrap();
    assert_eq!(i32::from_ne_bytes(v), 16_384, "the kernel doubles SO_SNDBUF (⚑ KERN)");
    assert_eq!(err(K.setsockopt(s, SockLevel::Socket, SO_REUSEADDR, &[1])), Errno::EINVAL);
    assert_eq!(err(K.setsockopt(s, SockLevel::Tcp, 9999, &int_bytes(1))), Errno::ENOPROTOOPT);
    close(s);
}

#[test]
fn k_sendto() {
    let t = Tmp::sock("sendto");
    let r = K.socket(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC, 0).unwrap();
    K.bind(r, &unix_addr(&t.s("r"))).unwrap();
    let s = K.socket(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC, 0).unwrap();
    assert_eq!(K.sendto(s, b"dgram", 0, Some(&unix_addr(&t.s("r")))).unwrap(), 5);
    let mut b = [0u8; 8];
    assert_eq!(K.read(r, &mut b).unwrap(), 5);
    // Unconnected without an address: ENOTCONN. Missing peer: ENOENT/ECONNREFUSED.
    assert_eq!(err(K.sendto(s, b"x", 0, None)), Errno::ENOTCONN);
    assert_eq!(err(K.sendto(s, b"x", 0, Some(&unix_addr(&t.s("nobody"))))), Errno::ENOENT);
    // MSG_DONTWAIT on a full stream socket: EAGAIN.
    let [a, c] = K.socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0).unwrap();
    let chunk = [0u8; 65536];
    let mut full = false;
    for _ in 0..1000 {
        if K.sendto(a, &chunk, MSG_DONTWAIT, None) == Err(Errno::EAGAIN) {
            full = true;
            break;
        }
    }
    assert!(full);
    for x in [a, c, s, r] {
        close(x);
    }
}

#[test]
fn k_recvfrom() {
    let t = Tmp::sock("recvfrom");
    let r = K.socket(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC, 0).unwrap();
    K.bind(r, &unix_addr(&t.s("r"))).unwrap();
    let s = K.socket(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC, 0).unwrap();
    K.bind(s, &unix_addr(&t.s("s"))).unwrap();
    K.sendto(s, b"0123456789", 0, Some(&unix_addr(&t.s("r")))).unwrap();
    let mut b = [0u8; 4];
    let mut from = SockAddrBuf::new();
    // MSG_TRUNC reports the real datagram length even when the buffer is smaller.
    assert_eq!(K.recvfrom(r, &mut b, MSG_TRUNC | MSG_PEEK, Some(&mut from)).unwrap(), 10);
    assert_eq!(&from.bytes()[2..], t.s("s").as_bytes().iter().copied().chain([0]).collect::<Vec<_>>().as_slice());
    assert_eq!(K.recvfrom(r, &mut b, 0, None).unwrap(), 4);
    assert_eq!(err(K.recvfrom(r, &mut b, MSG_DONTWAIT, None)), Errno::EAGAIN);
    close(s);
    close(r);
}

#[test]
fn k_sendmsg() {
    let [a, b] = K.socketpair(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0).unwrap();
    // Pass a descriptor with SCM_RIGHTS: cmsghdr { len: u64, level: i32, type: i32 } + int.
    let t = Tmp::new("sendmsg");
    let f = t.file("f", b"passed");
    let mut control = [0u8; 24];
    control[..8].copy_from_slice(&20u64.to_ne_bytes());
    control[8..12].copy_from_slice(&(SOL_SOCKET as i32).to_ne_bytes());
    control[12..16].copy_from_slice(&SCM_RIGHTS.to_ne_bytes());
    control[16..20].copy_from_slice(&f.raw().to_ne_bytes());
    let iov = [IoVec::new(b"m")];
    let msg = MsgHdr::new(None, &iov, &control).unwrap();
    assert_eq!(K.sendmsg(a, &msg, 0).unwrap(), 1);
    // Receive it and read through the new descriptor.
    let mut data = [0u8; 1];
    let mut ctl = [0u8; 64];
    let mut riov = [IoVecMut::new(&mut data)];
    let mut rmsg = MsgHdrMut::new(None, &mut riov, &mut ctl);
    assert_eq!(K.recvmsg(b, &mut rmsg, MSG_CMSG_CLOEXEC).unwrap(), 1);
    assert!(rmsg.control_len() >= 20);
    let got = Fd::new(i32::from_ne_bytes([ctl[16], ctl[17], ctl[18], ctl[19]]));
    let mut buf = [0u8; 6];
    assert_eq!(K.pread(got, &mut buf, 0).unwrap(), 6);
    assert_eq!(&buf, b"passed");
    // A bad descriptor in SCM_RIGHTS: EBADF.
    control[16..20].copy_from_slice(&(-1i32).to_ne_bytes());
    let bad = MsgHdr::new(None, &iov, &control).unwrap();
    assert_eq!(err(K.sendmsg(a, &bad, 0)), Errno::EBADF);
    for x in [got, f, a, b] {
        close(x);
    }
}

#[test]
fn k_recvmsg() {
    let t = Tmp::sock("recvmsg");
    let r = K.socket(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC, 0).unwrap();
    K.bind(r, &unix_addr(&t.s("r"))).unwrap();
    let s = K.socket(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC, 0).unwrap();
    K.bind(s, &unix_addr(&t.s("s"))).unwrap();
    K.sendto(s, b"abcdef", 0, Some(&unix_addr(&t.s("r")))).unwrap();
    let (mut x, mut y) = ([0u8; 2], [0u8; 2]);
    let mut name = SockAddrBuf::new();
    let mut iov = [IoVecMut::new(&mut x), IoVecMut::new(&mut y)];
    let mut ctl = [0u8; 0];
    let mut msg = MsgHdrMut::new(Some(&mut name), &mut iov, &mut ctl);
    assert_eq!(K.recvmsg(r, &mut msg, 0).unwrap(), 4, "scattered over two buffers");
    assert_ne!(msg.flags() & MSG_TRUNC as i32, 0, "the rest of the datagram was discarded");
    assert!(msg.name_len() as usize > 2);
    drop(msg);
    assert_eq!((&x, &y), (b"ab", b"cd"));
    let mut z = [0u8; 1];
    let mut iov = [IoVecMut::new(&mut z)];
    let mut ctl = [0u8; 0];
    let mut msg = MsgHdrMut::new(None, &mut iov, &mut ctl);
    assert_eq!(err(K.recvmsg(r, &mut msg, MSG_DONTWAIT)), Errno::EAGAIN);
    close(s);
    close(r);
}

#[test]
fn k_sendmmsg() {
    let [a, b] = K.socketpair(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC, 0).unwrap();
    let (i1, i2) = ([IoVec::new(b"first")], [IoVec::new(b"second")]);
    let mut msgs = [MMsgHdr::new(MsgHdr::new(None, &i1, &[]).unwrap()), MMsgHdr::new(MsgHdr::new(None, &i2, &[]).unwrap())];
    assert_eq!(K.sendmmsg(a, &mut msgs, 0).unwrap(), 2);
    assert_eq!((msgs[0].len(), msgs[1].len()), (5, 6), "per-message lengths written back");
    let mut buf = [0u8; 8];
    assert_eq!(K.read(b, &mut buf).unwrap(), 5);
    close(b);
    close(a);
    // The peer gone: ECONNREFUSED (in a child: concurrent forks copy descriptors).
    in_child(|k| {
        let [a, b] = k.socketpair(AF_UNIX, SOCK_DGRAM, 0).or_exit();
        k.close(b).or_exit();
        let iov = [IoVec::new(b"x")];
        let mut msgs = [MMsgHdr::new(MsgHdr::new(None, &iov, &[]).or_exit())];
        i32::from(k.sendmmsg(a, &mut msgs, 0) != Err(Errno::ECONNREFUSED))
    });
}

#[test]
fn k_recvmmsg() {
    let [a, b] = K.socketpair(AF_UNIX, SOCK_DGRAM | SOCK_CLOEXEC, 0).unwrap();
    K.write(a, b"one").unwrap();
    K.write(a, b"three").unwrap();
    let (mut x, mut y) = ([0u8; 8], [0u8; 8]);
    let (mut c1, mut c2) = ([0u8; 0], [0u8; 0]);
    let mut iov1 = [IoVecMut::new(&mut x)];
    let mut iov2 = [IoVecMut::new(&mut y)];
    let mut msgs = [
        MMsgHdrMut::new(MsgHdrMut::new(None, &mut iov1, &mut c1)),
        MMsgHdrMut::new(MsgHdrMut::new(None, &mut iov2, &mut c2)),
    ];
    let mut timeout = ms(1000);
    assert_eq!(K.recvmmsg(b, &mut msgs, MSG_DONTWAIT, Some(&mut timeout)).unwrap(), 2);
    assert_eq!((msgs[0].len(), msgs[1].len()), (3, 5));
    assert_eq!(err(K.recvmmsg(b, &mut msgs, MSG_DONTWAIT, None)), Errno::EAGAIN);
    drop(msgs);
    assert_eq!((&x[..3], &y[..5]), (&b"one"[..], &b"three"[..]));
    close(a);
    close(b);
}

// ------------------------------------------------------------------------------ time

fn ts_ns(t: Timespec) -> i128 {
    i128::from(t.sec) * 1_000_000_000 + i128::from(t.nsec)
}

#[test]
fn k_clock_gettime() {
    // vDSO and system call agree; monotonic clocks do not go backwards.
    for clock in [CLOCK_REALTIME, CLOCK_MONOTONIC, CLOCK_BOOTTIME, CLOCK_MONOTONIC_RAW, CLOCK_REALTIME_COARSE, CLOCK_MONOTONIC_COARSE, CLOCK_TAI, CLOCK_PROCESS_CPUTIME_ID, CLOCK_THREAD_CPUTIME_ID] {
        let a = K.clock_gettime(clock).unwrap();
        let b = K.clock_gettime_syscall(clock).unwrap();
        let c = K.clock_gettime(clock).unwrap();
        assert!((0..1_000_000_000).contains(&a.nsec));
        if clock != CLOCK_REALTIME && clock != CLOCK_REALTIME_COARSE && clock != CLOCK_TAI {
            assert!(ts_ns(a) <= ts_ns(b) && ts_ns(b) <= ts_ns(c), "clock {clock}");
        }
    }
    assert_eq!(err(K.clock_gettime(99)), Errno::EINVAL);
    assert_eq!(err(K.clock_gettime_syscall(99)), Errno::EINVAL);
    // CPU-time clock of another process (encoded clock ID ~pid << 3 | 2): ours via pid 0 form.
    let pid_clock = (!K.getpid() << 3) | 2;
    assert!(K.clock_gettime(pid_clock).is_ok());
}

#[test]
fn k_clock_getres() {
    let r = K.clock_getres(CLOCK_MONOTONIC).unwrap();
    assert_eq!(r, K.clock_getres_syscall(CLOCK_MONOTONIC).unwrap());
    assert!(r.sec == 0 && r.nsec > 0 && r.nsec <= 1_000_000);
    assert_eq!(err(K.clock_getres(99)), Errno::EINVAL);
}

#[test]
fn k_clock_settime() {
    let now = K.clock_gettime(CLOCK_REALTIME).unwrap();
    assert_eq!(err(K.clock_settime(CLOCK_REALTIME, &now)), Errno::EPERM);
    assert_eq!(err(K.clock_settime(CLOCK_MONOTONIC, &now)), Errno::EINVAL);
    assert_eq!(err(K.clock_settime(CLOCK_REALTIME, &Timespec { sec: 0, nsec: -1 })), Errno::EINVAL);
}

#[test]
fn k_clock_adjtime() {
    // modes = 0 is a read-only query, allowed unprivileged; returns the clock state.
    let mut tx = Timex::default();
    let state = K.clock_adjtime(CLOCK_REALTIME, &mut tx).unwrap();
    assert!((0..=5).contains(&state), "TIME_OK..TIME_ERROR");
    assert!(tx.tick > 0, "struct timex filled");
    let mut set = Timex::default();
    set.modes = 0x0001; // ADJ_OFFSET
    assert_eq!(err(K.clock_adjtime(CLOCK_REALTIME, &mut set)), Errno::EPERM);
    assert_eq!(err(K.clock_adjtime(99, &mut tx)), Errno::EINVAL);
}

#[test]
fn k_clock_nanosleep() {
    let t0 = K.clock_gettime(CLOCK_MONOTONIC).unwrap();
    K.clock_nanosleep(CLOCK_MONOTONIC, 0, &ms(20), None).unwrap();
    let t1 = K.clock_gettime(CLOCK_MONOTONIC).unwrap();
    assert!(ts_ns(t1) - ts_ns(t0) >= 20_000_000);
    // Absolute deadline in the past: returns at once.
    K.clock_nanosleep(CLOCK_MONOTONIC, TIMER_ABSTIME as u32, &t0, None).unwrap();
    assert_eq!(err(K.clock_nanosleep(CLOCK_MONOTONIC, 0, &Timespec { sec: 0, nsec: 2_000_000_000 }, None)), Errno::EINVAL);
    assert_eq!(err(K.clock_nanosleep(CLOCK_THREAD_CPUTIME_ID, 0, &ms(1), None)), Errno::EOPNOTSUPP);
    assert_eq!(err(K.clock_nanosleep(99, 0, &ms(1), None)), Errno::EINVAL);
    // Interrupted by a handler: EINTR with the remainder written.
    in_child(|k| {
        let (r, n) = selftest::with_counting_handler(SIGALRM, || {
            k.setitimer(ITIMER_REAL as u32, &Itimerval { interval: Timeval::default(), value: Timeval { sec: 0, usec: 50_000 } }).or_exit();
            let mut rem = Timespec::default();
            let r = k.clock_nanosleep(CLOCK_MONOTONIC, 0, &Timespec { sec: 5, nsec: 0 }, Some(&mut rem));
            (r, rem)
        })
        .or_exit();
        i32::from(r.0 != Err(Errno::EINTR) || n != 1 || !(r.1.sec >= 3 && r.1.sec <= 5))
    });
}

#[test]
fn k_gettimeofday() {
    let (tv, _tz) = K.gettimeofday().unwrap();
    let (tv2, _) = K.gettimeofday_syscall().unwrap();
    assert!((0..1_000_000).contains(&tv.usec));
    assert!((tv2.sec - tv.sec).abs() <= 1);
    let rt = K.clock_gettime(CLOCK_REALTIME).unwrap();
    assert!((rt.sec - tv.sec).abs() <= 1);
}

#[test]
fn k_settimeofday() {
    let (tv, _) = K.gettimeofday().unwrap();
    assert_eq!(err(K.settimeofday(Some(&tv), None)), Errno::EPERM);
    assert_eq!(err(K.settimeofday(Some(&Timeval { sec: 0, usec: 1_000_000 }), None)), Errno::EINVAL);
}

#[test]
fn k_time() {
    let a = K.time().unwrap();
    let b = K.time_syscall().unwrap();
    assert!(b - a <= 1 && b >= a);
    assert!(a > 1_700_000_000, "after 2023");
}

#[test]
fn k_times() {
    let mut t = Tms::default();
    let before = K.times(&mut t).unwrap();
    let mut x = 0u64;
    for i in 0..5_000_000u64 {
        x = x.wrapping_mul(31).wrapping_add(i);
    }
    std::hint::black_box(x);
    let after = K.times(&mut t).unwrap();
    assert!(after >= before, "clock ticks are monotonic");
    assert!(t.utime >= 0 && t.stime >= 0);
}

#[test]
fn k_setitimer() {
    in_child(|k| {
        let one = Itimerval { interval: Timeval { sec: 0, usec: 0 }, value: Timeval { sec: 10, usec: 0 } };
        let old = k.setitimer(ITIMER_REAL as u32, &one).or_exit();
        if old.value != Timeval::default() {
            return 1;
        }
        let cur = k.getitimer(ITIMER_REAL as u32).or_exit();
        if !(cur.value.sec >= 9 && cur.value.sec <= 10) {
            return 2;
        }
        let off = k.setitimer(ITIMER_REAL as u32, &Itimerval::default()).or_exit();
        if off.value.sec < 9 {
            return 3;
        }
        if k.setitimer(9, &one) != Err(Errno::EINVAL) {
            return 4;
        }
        let bad = Itimerval { interval: Timeval::default(), value: Timeval { sec: 0, usec: 1_000_000 } };
        i32::from(k.setitimer(ITIMER_REAL as u32, &bad) != Err(Errno::EINVAL)) * 5
    });
}

#[test]
fn k_getitimer() {
    in_child(|k| {
        for which in [ITIMER_REAL, ITIMER_VIRTUAL, ITIMER_PROF] {
            if k.getitimer(which as u32) != Ok(Itimerval::default()) {
                return 1;
            }
        }
        i32::from(k.getitimer(9) != Err(Errno::EINVAL)) * 2
    });
}

#[test]
fn k_alarm() {
    in_child(|k| {
        if k.alarm(100) != Ok(0) {
            return 1;
        }
        let left = k.alarm(0).or_exit();
        i32::from(!(99..=100).contains(&left)) * 2
    });
}

#[test]
fn k_timer_create() {
    in_child(|k| {
        block(k, &[SIGUSR1]);
        let ev = SigEvent::new(SIGEV_SIGNAL, SIGUSR1, 0x77, 0);
        let id = k.timer_create(CLOCK_MONOTONIC, Some(&ev)).or_exit();
        let spec = Itimerspec { interval: Timespec::default(), value: ms(10) };
        k.timer_settime(id, 0, &spec).or_exit();
        let mut info = SigInfo::default();
        if k.sigtimedwait(&sigset(&[SIGUSR1]), Some(&mut info), Some(&ms(2000))) != Ok(SIGUSR1) {
            return 1;
        }
        if info.code != -2 {
            return 2; // SI_TIMER
        }
        k.timer_delete(id).or_exit();
        if k.timer_create(99, None) != Err(Errno::EINVAL) {
            return 3;
        }
        let bad = SigEvent::new(SIGEV_SIGNAL, 99, 0, 0);
        i32::from(k.timer_create(CLOCK_MONOTONIC, Some(&bad)) != Err(Errno::EINVAL)) * 4
    });
}

#[test]
fn k_timer_settime() {
    in_child(|k| {
        let ev = SigEvent::new(SIGEV_NONE, 0, 0, 0);
        let id = k.timer_create(CLOCK_MONOTONIC, Some(&ev)).or_exit();
        let spec = Itimerspec { interval: ms(500), value: Timespec { sec: 30, nsec: 0 } };
        let old = k.timer_settime(id, 0, &spec).or_exit();
        if old != Itimerspec::default() {
            return 1;
        }
        let prev = k.timer_settime(id, 0, &Itimerspec::default()).or_exit();
        if prev.interval != ms(500) || prev.value.sec < 29 {
            return 2;
        }
        let bad = Itimerspec { interval: Timespec::default(), value: Timespec { sec: 0, nsec: -1 } };
        i32::from(k.timer_settime(id, 0, &bad) != Err(Errno::EINVAL) || k.timer_settime(9999, 0, &spec) != Err(Errno::EINVAL)) * 3
    });
}

#[test]
fn k_timer_gettime() {
    in_child(|k| {
        let id = k.timer_create(CLOCK_REALTIME, Some(&SigEvent::new(SIGEV_NONE, 0, 0, 0))).or_exit();
        if k.timer_gettime(id) != Ok(Itimerspec::default()) {
            return 1;
        }
        k.timer_settime(id, 0, &Itimerspec { interval: Timespec::default(), value: Timespec { sec: 5, nsec: 0 } }).or_exit();
        let cur = k.timer_gettime(id).or_exit();
        i32::from(!(cur.value.sec >= 4 && cur.value.sec <= 5) || k.timer_gettime(9999) != Err(Errno::EINVAL)) * 2
    });
}

#[test]
fn k_timer_getoverrun() {
    in_child(|k| {
        block(k, &[SIGUSR2]);
        let id = k.timer_create(CLOCK_MONOTONIC, Some(&SigEvent::new(SIGEV_SIGNAL, SIGUSR2, 0, 0))).or_exit();
        // A 1 ms periodic timer whose signal stays blocked for 50 ms overruns.
        k.timer_settime(id, 0, &Itimerspec { interval: ms(1), value: ms(1) }).or_exit();
        k.clock_nanosleep(CLOCK_MONOTONIC, 0, &ms(50), None).or_exit();
        k.sigtimedwait(&sigset(&[SIGUSR2]), None, Some(&ms(1000))).or_exit();
        let over = k.timer_getoverrun(id).or_exit();
        i32::from(over < 10 || k.timer_getoverrun(9999) != Err(Errno::EINVAL)) * 1
    });
}

#[test]
fn k_timer_delete() {
    in_child(|k| {
        let id = k.timer_create(CLOCK_MONOTONIC, Some(&SigEvent::new(SIGEV_NONE, 0, 0, 0))).or_exit();
        k.timer_delete(id).or_exit();
        i32::from(k.timer_delete(id) != Err(Errno::EINVAL))
    });
}

#[test]
fn k_timerfd_create() {
    let t = K.timerfd_create(CLOCK_MONOTONIC, (TFD_CLOEXEC | TFD_NONBLOCK) as u32).unwrap();
    let mut b = [0u8; 8];
    assert_eq!(err(K.read(t, &mut b)), Errno::EAGAIN);
    close(t);
    assert_eq!(err(K.timerfd_create(CLOCK_PROCESS_CPUTIME_ID, 0)), Errno::EINVAL);
    assert_eq!(err(K.timerfd_create(CLOCK_MONOTONIC, 0x1)), Errno::EINVAL);
}

#[test]
fn k_timerfd_settime() {
    let t = K.timerfd_create(CLOCK_MONOTONIC, TFD_CLOEXEC as u32).unwrap();
    let old = K.timerfd_settime(t, 0, &Itimerspec { interval: Timespec::default(), value: ms(10) }).unwrap();
    assert_eq!(old, Itimerspec::default());
    let mut b = [0u8; 8];
    assert_eq!(K.read(t, &mut b).unwrap(), 8);
    assert_eq!(u64::from_ne_bytes(b), 1, "one expiration");
    // Absolute time in the past: expires at once.
    let now = K.clock_gettime(CLOCK_MONOTONIC).unwrap();
    K.timerfd_settime(t, TFD_TIMER_ABSTIME as u32, &Itimerspec { interval: Timespec::default(), value: now }).unwrap();
    assert_eq!(K.read(t, &mut b).unwrap(), 8);
    assert_eq!(err(K.timerfd_settime(t, 0x10, &Itimerspec::default())), Errno::EINVAL);
    assert_eq!(err(K.timerfd_settime(t, 0, &Itimerspec { interval: Timespec::default(), value: Timespec { sec: 0, nsec: -5 } })), Errno::EINVAL);
    close(t);
}

#[test]
fn k_timerfd_gettime() {
    let t = K.timerfd_create(CLOCK_REALTIME, TFD_CLOEXEC as u32).unwrap();
    assert_eq!(K.timerfd_gettime(t).unwrap(), Itimerspec::default());
    K.timerfd_settime(t, 0, &Itimerspec { interval: ms(250), value: Timespec { sec: 60, nsec: 0 } }).unwrap();
    let cur = K.timerfd_gettime(t).unwrap();
    assert_eq!(cur.interval, ms(250));
    assert!(cur.value.sec >= 59);
    close(t);
    let [r, w] = K.pipe2(O_CLOEXEC).unwrap();
    assert_eq!(err(K.timerfd_gettime(r)), Errno::EINVAL);
    close(r);
    close(w);
}

// ------------------------------------------------------------------------------ scheduling and resources

#[test]
fn k_sched_setscheduler() {
    // Real-time policies need privilege: EPERM. SCHED_BATCH/IDLE are allowed (in a child).
    assert_eq!(err(K.sched_setscheduler(0, SCHED_FIFO, &SchedParam { priority: 1 })), Errno::EPERM);
    in_child(|k| {
        k.sched_setscheduler(0, SCHED_BATCH, &SchedParam { priority: 0 }).or_exit();
        if k.sched_getscheduler(0) != Ok(SCHED_BATCH) {
            return 1;
        }
        i32::from(k.sched_setscheduler(0, SCHED_OTHER, &SchedParam { priority: 5 }) != Err(Errno::EINVAL)) * 2
    });
    assert_eq!(err(K.sched_setscheduler(0, 77, &SchedParam { priority: 0 })), Errno::EINVAL);
}

#[test]
fn k_sched_getscheduler() {
    assert_eq!(K.sched_getscheduler(0).unwrap(), SCHED_OTHER);
    assert_eq!(err(K.sched_getscheduler(i32::MAX)), Errno::ESRCH);
    assert_eq!(err(K.sched_getscheduler(-1)), Errno::EINVAL);
}

#[test]
fn k_sched_setparam() {
    K.sched_setparam(0, &SchedParam { priority: 0 }).unwrap();
    assert_eq!(err(K.sched_setparam(0, &SchedParam { priority: 50 })), Errno::EINVAL);
    assert_eq!(err(K.sched_setparam(i32::MAX, &SchedParam { priority: 0 })), Errno::ESRCH);
}

#[test]
fn k_sched_getparam() {
    assert_eq!(K.sched_getparam(0).unwrap(), SchedParam { priority: 0 });
    assert_eq!(err(K.sched_getparam(-1)), Errno::EINVAL);
}

#[test]
fn k_sched_get_priority_min() {
    assert_eq!(K.sched_get_priority_min(SCHED_FIFO).unwrap(), 1);
    assert_eq!(K.sched_get_priority_min(SCHED_OTHER).unwrap(), 0);
    assert_eq!(err(K.sched_get_priority_min(77)), Errno::EINVAL);
}

#[test]
fn k_sched_get_priority_max() {
    assert_eq!(K.sched_get_priority_max(SCHED_RR).unwrap(), 99);
    assert_eq!(K.sched_get_priority_max(SCHED_BATCH).unwrap(), 0);
    assert_eq!(err(K.sched_get_priority_max(77)), Errno::EINVAL);
}

#[test]
fn k_sched_rr_get_interval() {
    let ts = K.sched_rr_get_interval(0).unwrap();
    assert!(ts.sec >= 0 && (0..1_000_000_000).contains(&ts.nsec));
    assert_eq!(err(K.sched_rr_get_interval(i32::MAX)), Errno::ESRCH);
}

#[test]
fn k_sched_setaffinity() {
    in_child(|k| {
        let mut mask = [0u8; 128];
        let n = k.sched_getaffinity(0, &mut mask).or_exit();
        let first = mask[..n].iter().position(|&b| b != 0).or_exit();
        let bit = mask[first].trailing_zeros();
        let mut one = [0u8; 128];
        one[first] = 1 << bit;
        k.sched_setaffinity(0, &one[..n]).or_exit();
        let mut back = [0u8; 128];
        k.sched_getaffinity(0, &mut back).or_exit();
        if back != one {
            return 1;
        }
        // An empty mask: EINVAL.
        i32::from(k.sched_setaffinity(0, &[0u8; 128][..n]) != Err(Errno::EINVAL)) * 2
    });
}

#[test]
fn k_sched_getaffinity() {
    let mut mask = [0u8; 128];
    let n = K.sched_getaffinity(0, &mut mask).unwrap();
    assert!(n >= 8 && n % 8 == 0);
    let cpus: u32 = mask[..n].iter().map(|b| b.count_ones()).sum();
    assert_eq!(cpus as usize, std::thread::available_parallelism().unwrap().get());
    // A buffer smaller than the kernel's mask: EINVAL.
    assert_eq!(err(K.sched_getaffinity(0, &mut [0u8; 1])), Errno::EINVAL);
}

#[test]
fn k_getcpu() {
    let (cpu, node) = K.getcpu().unwrap();
    let (cpu2, _) = K.getcpu_syscall().unwrap();
    let ncpu = std::thread::available_parallelism().unwrap().get() as u32;
    assert!(cpu < 1024 && cpu2 < 1024 && node < 64);
    let _ = ncpu;
}

#[test]
fn k_getpriority() {
    // The kernel returns 20 - nice (1..=40) so no valid value looks like an error (⚑ KERN).
    let p = K.getpriority(PRIO_PROCESS, 0).unwrap();
    assert!((1..=40).contains(&p));
    assert_eq!(err(K.getpriority(9, 0)), Errno::EINVAL);
    assert_eq!(err(K.getpriority(PRIO_PROCESS, i32::MAX as u32)), Errno::ESRCH);
}

#[test]
fn k_setpriority() {
    in_child(|k| {
        k.setpriority(PRIO_PROCESS, 0, 5).or_exit();
        if k.getpriority(PRIO_PROCESS, 0) != Ok(15) {
            return 1;
        }
        // Raising priority again needs privilege.
        i32::from(!matches!(k.setpriority(PRIO_PROCESS, 0, -5), Err(Errno::EACCES) | Err(Errno::EPERM))) * 2
    });
    assert_eq!(err(K.setpriority(9, 0, 0)), Errno::EINVAL);
}

#[test]
fn k_prlimit64() {
    let cur = K.prlimit(0, RLIMIT_NOFILE, None).unwrap();
    assert!(cur.cur <= cur.max);
    in_child(|k| {
        let cur = k.prlimit(0, RLIMIT_CORE, None).or_exit();
        let old = k.prlimit(0, RLIMIT_CORE, Some(&Rlimit { cur: 0, max: cur.max })).or_exit();
        if old != cur || k.prlimit(0, RLIMIT_CORE, None).or_exit().cur != 0 {
            return 1;
        }
        // soft > hard: EINVAL. Raising the hard limit: EPERM.
        if k.prlimit(0, RLIMIT_CORE, Some(&Rlimit { cur: 10, max: 5 })) != Err(Errno::EINVAL) {
            return 2;
        }
        let nofile = k.prlimit(0, RLIMIT_NOFILE, None).or_exit();
        i32::from(nofile.max != RLIM_INFINITY && k.prlimit(0, RLIMIT_NOFILE, Some(&Rlimit { cur: nofile.cur, max: RLIM_INFINITY })) != Err(Errno::EPERM)) * 3
    });
    assert_eq!(err(K.prlimit(0, 99, None)), Errno::EINVAL);
    assert_eq!(err(K.prlimit(i32::MAX, RLIMIT_CORE, None)), Errno::ESRCH);
}

#[test]
fn k_getrusage() {
    let ru = K.getrusage(RUSAGE_SELF).unwrap();
    assert!(ru.maxrss > 0, "struct rusage filled");
    let _ = K.getrusage(RUSAGE_CHILDREN).unwrap();
    let _ = K.getrusage(RUSAGE_THREAD).unwrap();
    assert_eq!(err(K.getrusage(7)), Errno::EINVAL);
}

#[test]
fn k_sysinfo() {
    let si = K.sysinfo().unwrap();
    assert!(si.uptime > 0 && si.totalram > 0 && si.mem_unit >= 1 && si.procs > 0);
    assert!(si.freeram <= si.totalram);
}

// ------------------------------------------------------------------------------ IPC

/// A System V IPC object removed on drop.
struct Ipc(i32, fn(i32));
impl Drop for Ipc {
    fn drop(&mut self) {
        (self.1)(self.0);
    }
}
fn rm_msg(id: i32) {
    let _ = K.msgctl(id, MsgCtl::Rmid, &mut MsqidDs::default());
}
fn rm_sem(id: i32) {
    let _ = K.semctl_int(id, 0, SemCtlInt::Rmid, 0);
}
fn rm_shm(id: i32) {
    let _ = K.shmctl(id, ShmCtl::Rmid, &mut ShmidDs::default());
}

fn message(mtype: i64, text: &[u8]) -> Vec<u8> {
    let mut v = mtype.to_ne_bytes().to_vec();
    v.extend_from_slice(text);
    v
}

#[test]
fn k_msgget() {
    let q = Ipc(K.msgget(IPC_PRIVATE, IPC_CREAT | 0o600).unwrap(), rm_msg);
    assert!(q.0 >= 0);
    let key = 0x4c49_0000 | (K.getpid() & 0xffff);
    let a = Ipc(K.msgget(key, IPC_CREAT | IPC_EXCL | 0o600).unwrap(), rm_msg);
    assert_eq!(K.msgget(key, 0).unwrap(), a.0, "the same key finds the same queue");
    assert_eq!(err(K.msgget(key, IPC_CREAT | IPC_EXCL | 0o600)), Errno::EEXIST);
    assert_eq!(err(K.msgget(key ^ 0x0100_0000, 0)), Errno::ENOENT);
}

#[test]
fn k_msgsnd() {
    let q = Ipc(K.msgget(IPC_PRIVATE, IPC_CREAT | 0o600).unwrap(), rm_msg);
    K.msgsnd(q.0, &message(1, b"hi"), 0).unwrap();
    let mut ds = MsqidDs::default();
    K.msgctl(q.0, MsgCtl::Stat, &mut ds).unwrap();
    assert_eq!(ds.qnum(), 1);
    // mtype must be positive; the message must hold the 8-byte type.
    assert_eq!(err(K.msgsnd(q.0, &message(0, b"x"), 0)), Errno::EINVAL);
    assert_eq!(err(K.msgsnd(q.0, b"short", 0)), Errno::EINVAL);
    assert_eq!(err(K.msgsnd(-1, &message(1, b"x"), 0)), Errno::EINVAL);
}

#[test]
fn k_msgrcv() {
    let q = Ipc(K.msgget(IPC_PRIVATE, IPC_CREAT | 0o600).unwrap(), rm_msg);
    K.msgsnd(q.0, &message(2, b"two"), 0).unwrap();
    K.msgsnd(q.0, &message(1, b"one!"), 0).unwrap();
    let mut buf = [0u8; 32];
    // By type: the type-1 message, though it was sent second.
    assert_eq!(K.msgrcv(q.0, &mut buf, 1, 0).unwrap(), 4);
    assert_eq!((i64::from_ne_bytes(buf[..8].try_into().unwrap()), &buf[8..12]), (1, &b"one!"[..]));
    // Too small without MSG_NOERROR: E2BIG; with it: truncated.
    let mut small = [0u8; 9];
    assert_eq!(err(K.msgrcv(q.0, &mut small, 0, IPC_NOWAIT)), Errno::E2BIG);
    assert_eq!(K.msgrcv(q.0, &mut small, 0, IPC_NOWAIT | MSG_NOERROR).unwrap(), 1);
    assert_eq!(err(K.msgrcv(q.0, &mut buf, 0, IPC_NOWAIT)), Errno::ENOMSG);
}

#[test]
fn k_msgctl() {
    let q = Ipc(K.msgget(IPC_PRIVATE, IPC_CREAT | 0o640).unwrap(), rm_msg);
    let mut ds = MsqidDs::default();
    K.msgctl(q.0, MsgCtl::Stat, &mut ds).unwrap();
    K.msgctl(q.0, MsgCtl::Set, &mut ds).unwrap();
    let mut info = MsqidDs::default();
    assert!(K.msgctl(0, MsgCtl::Info, &mut info).unwrap() >= 0, "IPC_INFO returns the highest index");
    K.msgctl(q.0, MsgCtl::Rmid, &mut ds).unwrap();
    assert_eq!(err(K.msgctl(q.0, MsgCtl::Stat, &mut ds)), Errno::EINVAL);
}

#[test]
fn k_semget() {
    let s = Ipc(K.semget(IPC_PRIVATE, 3, IPC_CREAT | 0o600).unwrap(), rm_sem);
    let mut ds = SemidDs::default();
    K.semctl_ds(s.0, SemCtlDs::Stat, &mut ds).unwrap();
    assert_eq!(ds.nsems(), 3);
    assert_eq!(err(K.semget(IPC_PRIVATE, -1, IPC_CREAT | 0o600)), Errno::EINVAL);
    assert_eq!(err(K.semget(IPC_PRIVATE, 1_000_000, IPC_CREAT | 0o600)), Errno::EINVAL);
}

#[test]
fn k_semtimedop() {
    let s = Ipc(K.semget(IPC_PRIVATE, 1, IPC_CREAT | 0o600).unwrap(), rm_sem);
    K.semtimedop(s.0, &[Sembuf { num: 0, op: 2, flg: 0 }], None).unwrap();
    K.semtimedop(s.0, &[Sembuf { num: 0, op: -2, flg: 0 }], None).unwrap();
    // Would block: EAGAIN with IPC_NOWAIT; a timeout: EAGAIN after it expires.
    assert_eq!(err(K.semtimedop(s.0, &[Sembuf { num: 0, op: -1, flg: IPC_NOWAIT as i16 }], None)), Errno::EAGAIN);
    assert_eq!(err(K.semtimedop(s.0, &[Sembuf { num: 0, op: -1, flg: 0 }], Some(&ms(20)))), Errno::EAGAIN);
    assert_eq!(err(K.semtimedop(s.0, &[Sembuf { num: 5, op: 1, flg: 0 }], None)), Errno::EFBIG);
    assert_eq!(err(K.semtimedop(s.0, &[], None)), Errno::EINVAL);
}

#[test]
fn k_semctl() {
    let s = Ipc(K.semget(IPC_PRIVATE, 2, IPC_CREAT | 0o600).unwrap(), rm_sem);
    K.semctl_int(s.0, 1, SemCtlInt::SetVal, 7).unwrap();
    assert_eq!(K.semctl_int(s.0, 1, SemCtlInt::GetVal, 0).unwrap(), 7);
    K.semctl_setall(s.0, &[3, 4]).unwrap();
    let mut all = [0u16; 2];
    K.semctl_getall(s.0, &mut all).unwrap();
    assert_eq!(all, [3, 4]);
    // The boundary refuses arrays shorter than the set (the kernel would write past them).
    assert_eq!(err(K.semctl_getall(s.0, &mut [0u16; 1])), Errno::EINVAL);
    assert_eq!(err(K.semctl_int(s.0, 1, SemCtlInt::SetVal, 40_000)), Errno::ERANGE);
    assert_eq!(err(K.semctl_int(s.0, 9, SemCtlInt::GetVal, 0)), Errno::EINVAL);
    assert_eq!(K.semctl_int(s.0, 0, SemCtlInt::GetPid, 0).unwrap(), K.getpid());
}

#[test]
fn k_shmget() {
    let s = Ipc(K.shmget(IPC_PRIVATE, 10_000, IPC_CREAT | 0o600).unwrap(), rm_shm);
    let mut ds = ShmidDs::default();
    K.shmctl(s.0, ShmCtl::Stat, &mut ds).unwrap();
    assert_eq!(ds.segsz(), 10_000);
    assert_eq!(err(K.shmget(IPC_PRIVATE, 0, IPC_CREAT | 0o600)), Errno::EINVAL);
    assert_eq!(err(K.shmget(0x4c49_ffff, 4096, 0)), Errno::ENOENT);
}

#[test]
fn k_shmat() {
    let s = Ipc(K.shmget(IPC_PRIVATE, 5000, IPC_CREAT | 0o600).unwrap(), rm_shm);
    let a = K.shmat(s.0, 0).unwrap();
    let b = K.shmat(s.0, 0).unwrap();
    assert_eq!((a.kind(), a.len()), (RegionKind::Shm, 8192));
    // Both attachments see the same words.
    K.region_words(&a).unwrap()[3].store(0xabcd, Ordering::SeqCst);
    assert_eq!(K.region_words(&b).unwrap()[3].load(Ordering::SeqCst), 0xabcd);
    let ro = K.shmat(s.0, SHM_RDONLY).unwrap();
    assert!(K.region_words(&ro).is_none(), "read-only attachments are not handed out as atomics");
    for r in [a, b, ro] {
        K.shmdt(r).unwrap();
    }
    // Attachment counting, in a child: in this process, other tests' forks copy attachments.
    in_child(|k| {
        let id = k.shmget(IPC_PRIVATE, 4096, IPC_CREAT | 0o600).or_exit();
        let (a, b) = (k.shmat(id, 0).or_exit(), k.shmat(id, SHM_RDONLY).or_exit());
        let mut ds = ShmidDs::default();
        k.shmctl(id, ShmCtl::Stat, &mut ds).or_exit();
        let two = ds.nattch() == 2;
        k.shmdt(a).or_exit();
        k.shmdt(b).or_exit();
        k.shmctl(id, ShmCtl::Stat, &mut ds).or_exit();
        let zero = ds.nattch() == 0;
        k.shmctl(id, ShmCtl::Rmid, &mut ds).or_exit();
        i32::from(!(two && zero))
    });
    assert_eq!(err(K.shmat(s.0, SHM_REMAP)), Errno::EINVAL);
    assert_eq!(err(K.shmat(-1, 0)), Errno::EINVAL);
}

#[test]
fn k_shmdt() {
    let s = Ipc(K.shmget(IPC_PRIVATE, 4096, IPC_CREAT | 0o600).unwrap(), rm_shm);
    let a = K.shmat(s.0, 0).unwrap();
    K.shmdt(a).unwrap();
    // Not a shared-memory region: refused.
    let r = K.mmap(0, 4096, PROT_READ | PROT_WRITE, MAP_PRIVATE, None, 0).unwrap();
    assert_eq!(err(K.shmdt(r)), Errno::EINVAL);
}

#[test]
fn k_shmctl() {
    let s = Ipc(K.shmget(IPC_PRIVATE, 4096, IPC_CREAT | 0o600).unwrap(), rm_shm);
    let mut ds = ShmidDs::default();
    K.shmctl(s.0, ShmCtl::Stat, &mut ds).unwrap();
    K.shmctl(s.0, ShmCtl::Set, &mut ds).unwrap();
    let mut info = ShmidDs::default();
    assert!(K.shmctl(0, ShmCtl::Info, &mut info).unwrap() >= 0);
    // SHM_LOCK needs CAP_IPC_LOCK or a sufficient RLIMIT_MEMLOCK: success or EPERM.
    match K.shmctl(s.0, ShmCtl::Lock, &mut ds) {
        Ok(_) => {
            K.shmctl(s.0, ShmCtl::Unlock, &mut ds).unwrap();
        }
        Err(e) => assert_eq!(e, Errno::EPERM),
    }
    K.shmctl(s.0, ShmCtl::Rmid, &mut ds).unwrap();
    assert_eq!(err(K.shmctl(s.0, ShmCtl::Stat, &mut ds)), Errno::EINVAL);
}

fn mq_name(tag: &str) -> KPathBuf {
    kp(&format!("liella-{}-{tag}", K.getpid()))
}

#[test]
fn k_mq_open() {
    let name = mq_name("open");
    let attr = MqAttr::new(4, 64);
    let q = K.mq_open(name.as_kstr(), O_RDWR | O_CREAT | O_EXCL | O_CLOEXEC, 0o600, Some(&attr)).unwrap();
    assert_eq!(err(K.mq_open(name.as_kstr(), O_RDWR | O_CREAT | O_EXCL, 0o600, Some(&attr))), Errno::EEXIST);
    let a = K.mq_getsetattr(q, None).unwrap();
    assert_eq!((a.maxmsg, a.msgsize), (4, 64));
    close(q);
    K.mq_unlink(name.as_kstr()).unwrap();
    assert_eq!(err(K.mq_open(name.as_kstr(), O_RDONLY, 0, None)), Errno::ENOENT);
    // A name with a slash: EACCES (⚑ KERN; the C library strips the leading one).
    assert_eq!(err(K.mq_open(kp("a/b").as_kstr(), O_RDWR | O_CREAT, 0o600, None)), Errno::EACCES);
    let bad = MqAttr::new(0, 64);
    assert_eq!(err(K.mq_open(mq_name("bad").as_kstr(), O_RDWR | O_CREAT, 0o600, Some(&bad))), Errno::EINVAL);
}

#[test]
fn k_mq_unlink() {
    let name = mq_name("unlink");
    close(K.mq_open(name.as_kstr(), O_RDWR | O_CREAT | O_CLOEXEC, 0o600, None).unwrap());
    K.mq_unlink(name.as_kstr()).unwrap();
    assert_eq!(err(K.mq_unlink(name.as_kstr())), Errno::ENOENT);
}

#[test]
fn k_mq_timedsend() {
    let name = mq_name("send");
    let q = K.mq_open(name.as_kstr(), O_RDWR | O_CREAT | O_NONBLOCK | O_CLOEXEC, 0o600, Some(&MqAttr::new(1, 8))).unwrap();
    K.mq_unlink(name.as_kstr()).unwrap();
    K.mq_timedsend(q, b"12345678", 3, None).unwrap();
    assert_eq!(err(K.mq_timedsend(q, b"x", 0, None)), Errno::EAGAIN, "queue full");
    assert_eq!(err(K.mq_timedsend(q, b"123456789", 0, None)), Errno::EMSGSIZE);
    close(q);
}

#[test]
fn k_mq_timedreceive() {
    let name = mq_name("recv");
    let q = K.mq_open(name.as_kstr(), O_RDWR | O_CREAT | O_CLOEXEC, 0o600, Some(&MqAttr::new(4, 16))).unwrap();
    K.mq_unlink(name.as_kstr()).unwrap();
    K.mq_timedsend(q, b"low", 1, None).unwrap();
    K.mq_timedsend(q, b"high", 9, None).unwrap();
    let mut buf = [0u8; 16];
    let mut prio = 0u32;
    assert_eq!(K.mq_timedreceive(q, &mut buf, Some(&mut prio), None).unwrap(), 4);
    assert_eq!((&buf[..4], prio), (&b"high"[..], 9), "highest priority first");
    // Buffer smaller than mq_msgsize: EMSGSIZE. Empty with a timeout: ETIMEDOUT.
    assert_eq!(err(K.mq_timedreceive(q, &mut buf[..8], None, None)), Errno::EMSGSIZE);
    K.mq_timedreceive(q, &mut buf, None, None).unwrap();
    let past = K.clock_gettime(CLOCK_REALTIME).unwrap();
    assert_eq!(err(K.mq_timedreceive(q, &mut buf, None, Some(&past))), Errno::ETIMEDOUT);
    close(q);
}

#[test]
fn k_mq_notify() {
    in_child(|k| {
        block(k, &[SIGUSR1]);
        let name = kp("liella-notify");
        let _ = k.mq_unlink(name.as_kstr());
        let q = k.mq_open(name.as_kstr(), O_RDWR | O_CREAT | O_CLOEXEC, 0o600, None).or_exit();
        k.mq_unlink(name.as_kstr()).or_exit();
        k.mq_notify(q, Some(&SigEvent::new(SIGEV_SIGNAL, SIGUSR1, 5, 0))).or_exit();
        // Only one registration per queue: EBUSY.
        if k.mq_notify(q, Some(&SigEvent::new(SIGEV_SIGNAL, SIGUSR1, 5, 0))) != Err(Errno::EBUSY) {
            return 1;
        }
        k.mq_timedsend(q, b"x", 0, None).or_exit();
        let mut info = SigInfo::default();
        if k.sigtimedwait(&sigset(&[SIGUSR1]), Some(&mut info), Some(&ms(2000))) != Ok(SIGUSR1) || info.code != -3 {
            return 2; // SI_MESGQ
        }
        // Unregistering when not registered is fine; a bad descriptor is EBADF.
        k.mq_notify(q, None).or_exit();
        i32::from(k.mq_notify(Fd::new(-1), None) != Err(Errno::EBADF)) * 3
    });
}

#[test]
fn k_mq_getsetattr() {
    let name = mq_name("attr");
    let q = K.mq_open(name.as_kstr(), O_RDWR | O_CREAT | O_CLOEXEC, 0o600, Some(&MqAttr::new(2, 32))).unwrap();
    K.mq_unlink(name.as_kstr()).unwrap();
    let mut nb = MqAttr::new(0, 0);
    nb.flags = i64::from(O_NONBLOCK);
    let old = K.mq_getsetattr(q, Some(&nb)).unwrap();
    assert_eq!(old.flags, 0);
    let now = K.mq_getsetattr(q, None).unwrap();
    assert_eq!((now.flags, now.maxmsg, now.msgsize, now.curmsgs), (i64::from(O_NONBLOCK), 2, 32, 0), "only O_NONBLOCK changes");
    close(q);
    assert_eq!(err(K.mq_getsetattr(Fd::new(-1), None)), Errno::EBADF);
}

// ------------------------------------------------------------------------------ randomness

#[test]
fn k_getrandom() {
    let mut a = [0u8; 64];
    let mut b = [0u8; 64];
    assert_eq!(K.getrandom(&mut a, 0).unwrap(), 64);
    assert_eq!(K.getrandom(&mut b, GRND_NONBLOCK).unwrap(), 64);
    assert_ne!(a, b);
    assert_eq!(K.getrandom(&mut [], 0).unwrap(), 0);
    assert_eq!(err(K.getrandom(&mut a, 0x100)), Errno::EINVAL);
    assert_eq!(err(K.getrandom(&mut a, GRND_RANDOM | GRND_INSECURE)), Errno::EINVAL);
}

// ------------------------------------------------------------------------------ system identity and control

fn cstr(field: &[u8]) -> &[u8] {
    &field[..field.iter().position(|&b| b == 0).unwrap_or(field.len())]
}

#[test]
fn k_uname() {
    let u = K.uname().unwrap();
    assert_eq!(cstr(&u.sysname), b"Linux");
    assert_eq!(cstr(&u.machine), b"x86_64");
    assert!(!cstr(&u.release).is_empty() && !cstr(&u.nodename).is_empty());
}

#[test]
fn k_sethostname() {
    if skip_without_userns("k_sethostname") {
        return;
    }
    assert_eq!(err(K.sethostname(b"x")), Errno::EPERM);
    in_child(|k| {
        become_ns_root(k, CLONE_NEWUTS).or_exit();
        k.sethostname(b"liella-host").or_exit();
        if cstr(&k.uname().or_exit().nodename) != b"liella-host" {
            return 1;
        }
        i32::from(k.sethostname(&[b'h'; 65]) != Err(Errno::EINVAL)) * 2
    });
}

#[test]
fn k_setdomainname() {
    if skip_without_userns("k_setdomainname") {
        return;
    }
    assert_eq!(err(K.setdomainname(b"x")), Errno::EPERM);
    in_child(|k| {
        become_ns_root(k, CLONE_NEWUTS).or_exit();
        k.setdomainname(b"liella.test").or_exit();
        if cstr(&k.uname().or_exit().domainname) != b"liella.test" {
            return 1;
        }
        i32::from(k.setdomainname(&[b'd'; 65]) != Err(Errno::EINVAL)) * 2
    });
}

#[test]
fn k_reboot() {
    // Destructive: error paths only, with the harmless CAD_OFF command. CAP_SYS_BOOT is checked
    // before the magic numbers, so both requests are EPERM unprivileged (⚑ KERN).
    assert_eq!(err(K.reboot(1, 2, LINUX_REBOOT_CMD_CAD_OFF, None)), Errno::EPERM);
    assert_eq!(err(K.reboot(LINUX_REBOOT_MAGIC1, LINUX_REBOOT_MAGIC2, LINUX_REBOOT_CMD_CAD_OFF, None)), Errno::EPERM);
}

#[test]
fn k_syslog() {
    // SIZE_BUFFER is allowed unprivileged unless dmesg_restrict; READ_ALL needs CAP_SYSLOG then.
    match K.syslog(SYSLOG_ACTION_SIZE_BUFFER, &mut []) {
        Ok(n) => assert!(n > 0),
        Err(e) => assert_eq!(e, Errno::EPERM),
    }
    // Every other action is restricted, so even an unknown one is EPERM unprivileged
    // (permission is checked before the action, ⚑ KERN).
    assert_eq!(err(K.syslog(99, &mut [])), Errno::EPERM);
}

#[test]
fn k_klogctl() {
    // klogctl is glibc's name for the syslog system call (same primitive): read the buffer if
    // permitted, and reject a negative length.
    let mut buf = vec![0u8; 1 << 16];
    match K.syslog(SYSLOG_ACTION_READ_ALL, &mut buf) {
        Ok(n) => assert!(n <= buf.len()),
        Err(e) => assert_eq!(e, Errno::EPERM),
    }
}

#[test]
fn k_acct() {
    // Destructive: unprivileged EPERM only.
    assert_eq!(err(K.acct(Some(kp("/tmp/liella-acct").as_kstr()))), Errno::EPERM);
    assert_eq!(err(K.acct(None)), Errno::EPERM);
}

#[test]
fn k_ioperm() {
    // Kernels built without CONFIG_X86_IOPL_IOPERM (this suite's WSL2 reference kernel is one)
    // answer ENOSYS to every request.
    if K.ioperm(0x80, 1, false) == Err(Errno::ENOSYS) {
        assert_eq!(err(K.ioperm(0x80, 1, true)), Errno::ENOSYS);
        return;
    }
    assert_eq!(err(K.ioperm(0x80, 1, true)), Errno::EPERM);
    assert_eq!(err(K.ioperm(0xffff, 2, true)), Errno::EINVAL);
}

#[test]
fn k_iopl() {
    // Same configuration dependency as ioperm.
    if K.iopl(0) == Err(Errno::ENOSYS) {
        assert_eq!(err(K.iopl(3)), Errno::ENOSYS);
        return;
    }
    assert_eq!(err(K.iopl(3)), Errno::EPERM);
    assert_eq!(err(K.iopl(4)), Errno::EINVAL);
}

#[test]
fn k_modify_ldt() {
    // Read the (empty) LDT: 0 bytes. Writing is `raw` only.
    let mut buf = [0u8; 64];
    assert_eq!(K.modify_ldt_read(0, &mut buf).unwrap(), 0);
    // Function 2 reads the default LDT (zeros) on kernels that keep it, else 0 bytes.
    let _ = K.modify_ldt_read(2, &mut buf).unwrap();
    assert_eq!(err(K.modify_ldt_read(1, &mut buf)), Errno::EINVAL);
}

#[test]
fn k_init_module() {
    // Destructive: an image that is not a module, without privilege: EPERM.
    assert_eq!(err(K.init_module(b"not an ELF module", kp("").as_kstr())), Errno::EPERM);
}

#[test]
fn k_finit_module() {
    let t = Tmp::new("finit");
    let f = t.file("m.ko", b"junk");
    assert_eq!(err(K.finit_module(f, kp("").as_kstr(), 0)), Errno::EPERM);
    close(f);
}

#[test]
fn k_delete_module() {
    assert_eq!(err(K.delete_module(kp("liella_no_such_module").as_kstr(), 0)), Errno::EPERM);
}

#[test]
fn k_vhangup() {
    assert_eq!(err(K.vhangup()), Errno::EPERM);
}

#[test]
fn k_personality() {
    in_child(|k| {
        let cur = k.personality(PERSONALITY_QUERY).or_exit();
        let prev = k.personality(cur | ADDR_NO_RANDOMIZE).or_exit();
        i32::from(prev != cur || k.personality(PERSONALITY_QUERY) != Ok(cur | ADDR_NO_RANDOMIZE))
    });
}

#[test]
fn k_prctl() {
    in_child(|k| {
        k.prctl_set_name(b"liella-prctl\0\0\0\0").or_exit();
        if &k.prctl_get_name().or_exit()[..13] != b"liella-prctl\0" {
            return 1;
        }
        k.prctl_int(PrctlInt::SetPdeathsig, SIGUSR1 as u64, 0).or_exit();
        if k.prctl_get_int(PrctlIntOut::GetPdeathsig) != Ok(SIGUSR1) {
            return 2;
        }
        if k.prctl_int(PrctlInt::GetDumpable, 0, 0) != Ok(1) {
            return 3;
        }
        k.prctl_int(PrctlInt::SetNoNewPrivs, 1, 0).or_exit();
        if k.prctl_int(PrctlInt::GetNoNewPrivs, 0, 0) != Ok(1) {
            return 4;
        }
        // NO_NEW_PRIVS cannot be cleared; a bad signal number: EINVAL.
        if k.prctl_int(PrctlInt::SetNoNewPrivs, 0, 0) != Err(Errno::EINVAL) {
            return 5;
        }
        i32::from(k.prctl_int(PrctlInt::SetPdeathsig, 999, 0) != Err(Errno::EINVAL)) * 6
    });
}

#[test]
fn k_unshare() {
    if skip_without_userns("k_unshare") {
        return;
    }
    in_child(|k| {
        let before = k.fstatat(Fd::CWD, kp("/proc/self/ns/uts").as_kstr(), 0).or_exit().ino;
        become_ns_root(k, CLONE_NEWUTS).or_exit();
        let after = k.fstatat(Fd::CWD, kp("/proc/self/ns/uts").as_kstr(), 0).or_exit().ino;
        if before == after {
            return 1;
        }
        i32::from(k.unshare(1 << 0) != Err(Errno::EINVAL)) * 2
    });
    // Without a user namespace, new namespaces need CAP_SYS_ADMIN: EPERM. A multithreaded
    // process cannot enter a new user namespace: EINVAL.
    assert_eq!(err(K.unshare(CLONE_NEWUTS)), Errno::EPERM);
    assert_eq!(err(K.unshare(CLONE_NEWUSER)), Errno::EINVAL);
}

#[test]
fn k_setns() {
    if skip_without_userns("k_setns") {
        return;
    }
    in_child(|k| {
        let mine = k.openat(Fd::CWD, kp("/proc/self/ns/uts").as_kstr(), O_RDONLY | O_CLOEXEC, 0).or_exit();
        become_ns_root(k, CLONE_NEWUTS).or_exit();
        // Back into the original UTS namespace: its owner (the initial user namespace) grants
        // us nothing, so EPERM; our own new one is fine.
        if k.setns(mine, CLONE_NEWUTS as u32) != Err(Errno::EPERM) {
            return 1;
        }
        let own = k.openat(Fd::CWD, kp("/proc/self/ns/uts").as_kstr(), O_RDONLY | O_CLOEXEC, 0).or_exit();
        if k.setns(own, CLONE_NEWUTS as u32).is_err() {
            return 2;
        }
        // Wrong type for the descriptor: EINVAL.
        i32::from(k.setns(own, CLONE_NEWNET as u32) != Err(Errno::EINVAL)) * 3
    });
}

#[test]
fn k_ptrace() {
    // Attach to our own child, peek a word we know, detach.
    static MARK: u64 = 0x4c49_454c_4c41_2121;
    let pid = selftest::spawn_pausing().unwrap();
    K.ptrace(PtraceOp::Seize, pid, 0, 0).unwrap();
    K.ptrace(PtraceOp::Interrupt, pid, 0, 0).unwrap();
    let (_, st) = K.wait4(pid, 0x4000_0000 | WUNTRACED, None).unwrap(); // __WALL
    assert_eq!(st & 0xff, 0x7f, "stopped");
    let word = K.ptrace_peek(PtracePeek::Data, pid, std::ptr::from_ref(&MARK).addr() as u64).unwrap();
    assert_eq!(word, MARK);
    assert_eq!(err(K.ptrace_peek(PtracePeek::Data, pid, 8)), Errno::EIO);
    K.ptrace(PtraceOp::Detach, pid, 0, 0).unwrap();
    K.kill(pid, SIGKILL).unwrap();
    K.wait4(pid, 0, None).unwrap();
    // Not traced: ESRCH. init: EPERM.
    assert_eq!(err(K.ptrace(PtraceOp::Cont, K.getppid(), 0, 0)), Errno::ESRCH);
    assert_eq!(err(K.ptrace(PtraceOp::Attach, 1, 0, 0)), Errno::EPERM);
}

// ------------------------------------------------------------------------------ A.2 interfaces

#[test]
fn k_initial_process_stack() {
    // The kernel builds argc/argv/envp/auxv on the new stack; a re-executed probe sees exactly
    // what execve passed (empty strings and spaces preserved, no environment added).
    if probe().as_deref() == Some("stack") {
        let args: Vec<String> = std::env::args().collect();
        let env: Vec<(String, String)> = std::env::vars().collect();
        let ok = args[0] == "argv zero" && args.len() == 5
            && env.iter().any(|(k, v)| k == "EMPTY" && v.is_empty())
            && env.iter().any(|(k, v)| k == "SPACED" && v == " a b ");
        K.exit_group(if ok { 45 } else { 1 });
    }
    assert_eq!(run_probe("k_initial_process_stack", "stack", &["EMPTY=", "SPACED= a b "], "argv zero"), ChildEnd::Exited(45));
}

/// This process's auxiliary vector, from /proc.
fn my_auxv() -> Vec<u64> {
    let bytes = std::fs::read("/proc/self/auxv").unwrap();
    let mut words = vec![0u64; bytes.len() / 8];
    let n = auxv::words_from_bytes(&bytes, &mut words);
    words.truncate(n);
    words
}

#[test]
fn k_auxiliary_vector_entries() {
    let words = my_auxv();
    let av = auxv::Auxv::from_words(&words);
    assert_eq!(av.get(AT_PAGESZ), Some(4096));
    assert_eq!(av.get(AT_UID), Some(u64::from(K.getuid())));
    assert_eq!(av.get(AT_EUID), Some(u64::from(K.geteuid())));
    assert_eq!(av.get(AT_GID), Some(u64::from(K.getgid())));
    assert_eq!(av.get(AT_EGID), Some(u64::from(K.getegid())));
    assert_eq!(av.get(AT_SECURE), Some(0));
    assert_eq!(av.get(AT_CLKTCK), Some(100));
    for key in [AT_RANDOM, AT_PHDR, AT_PHNUM, AT_ENTRY, AT_EXECFN, AT_PLATFORM, AT_HWCAP] {
        assert!(av.get(key).is_some_and(|v| v != 0), "AT_{key} present");
    }
    assert_eq!(av.get(9999), None, "absent keys are None, not 0 (D-1.5)");
    // Parsing synthetic vectors: stops at AT_NULL, tolerates a missing terminator and odd words.
    let synthetic = [AT_PAGESZ, 4096, AT_NULL, 0, AT_UID, 5];
    assert_eq!(auxv::Auxv::from_words(&synthetic).len(), 1);
    assert_eq!(auxv::Auxv::from_words(&[AT_UID, 7, AT_GID]).get(AT_UID), Some(7));
    assert!(auxv::Auxv::from_words(&[]).is_empty());
    assert_eq!(auxv::Auxv::from_words(&[AT_UID, 1, AT_UID, 2]).get(AT_UID), Some(1), "first entry wins");
    let mut out = [0u64; 2];
    assert_eq!(auxv::words_from_bytes(&[1, 0, 0, 0, 0, 0, 0, 0, 9], &mut out), 1);
}

#[test]
fn k_vdso() {
    assert!(K.vdso_clock_gettime_available(), "the x86_64 kernel provides a vDSO");
    let words = my_auxv();
    let base = auxv::Auxv::from_words(&words).get(AT_SYSINFO_EHDR).unwrap();
    assert_ne!(base, 0);
    // The parser over a copy of the image (read through /proc/self/mem is not needed: the
    // vDSO is also readable via process_vm_readv of our own address space).
    let mut header = [0u8; vdso::HEADER_LEN];
    {
        let mut local = [IoVecMut::new(&mut header)];
        K.process_vm_readv(K.getpid(), &mut local, &[RemoteIoVec { base: base as usize, len: vdso::HEADER_LEN }], 0).unwrap();
    }
    let len = vdso::image_len(&header).unwrap();
    let mut image = vec![0u8; len];
    {
        let mut local = [IoVecMut::new(&mut image)];
        K.process_vm_readv(K.getpid(), &mut local, &[RemoteIoVec { base: base as usize, len }], 0).unwrap();
    }
    let v = vdso::Vdso::parse(&image).unwrap();
    for name in [&b"__vdso_clock_gettime"[..], b"__vdso_gettimeofday", b"__vdso_time", b"__vdso_getcpu", b"__vdso_clock_getres"] {
        assert!(v.lookup(name, b"LINUX_2.6").is_some(), "{}", String::from_utf8_lossy(name));
    }
    assert_eq!(v.lookup(b"__vdso_clock_gettime", b"LINUX_9.9"), None, "version checked");
    assert_eq!(v.lookup(b"__vdso_nonexistent", b"LINUX_2.6"), None);
    // Absent or malformed images: no symbols, never a panic.
    assert!(vdso::Vdso::parse(&[]).is_none());
    assert!(vdso::image_len(&[0u8; 64]).is_none());
    let mut truncated = image.clone();
    truncated.truncate(200);
    assert!(vdso::Vdso::parse(&truncated).is_none());
    let mut corrupt = image.clone();
    for b in corrupt.iter_mut().skip(64).step_by(7) {
        *b ^= 0x5a;
    }
    let _ = vdso::Vdso::parse(&corrupt).map(|v| v.lookup(b"__vdso_time", b"LINUX_2.6"));
}

#[test]
fn k_vdso_lookup_many_answers_as_single_lookups() {
    // Clauses: s0_6_c0_8_3 (SETLOCALE-2: the five vDSO functions are resolved in one walk; every
    // answer must be the per-name walk's, on the real image and on every one-byte damage of it).
    let base = auxv::Auxv::from_words(&my_auxv()).get(AT_SYSINFO_EHDR).unwrap();
    let mut header = [0u8; vdso::HEADER_LEN];
    K.process_vm_readv(K.getpid(), &mut [IoVecMut::new(&mut header)], &[RemoteIoVec { base: base as usize, len: vdso::HEADER_LEN }], 0).unwrap();
    let len = vdso::image_len(&header).unwrap();
    let mut image = vec![0u8; len];
    K.process_vm_readv(K.getpid(), &mut [IoVecMut::new(&mut image)], &[RemoteIoVec { base: base as usize, len }], 0).unwrap();
    let names: [&[u8]; 9] = [
        b"__vdso_clock_gettime", b"__vdso_clock_getres", b"__vdso_gettimeofday", b"__vdso_time", b"__vdso_getcpu",
        b"clock_gettime", b"__vdso_time", b"__vdso_nonexistent", b"",
    ];
    let check = |img: &[u8]| -> Option<Vec<Option<usize>>> {
        let v = vdso::Vdso::parse(img)?;
        let mut all = Vec::new();
        for version in [&b"LINUX_2.6"[..], b"LINUX_9.9"] {
            let many = v.lookup_many(&names, version);
            let one: Vec<_> = names.iter().map(|n| v.lookup_one_by_one(n, version)).collect();
            assert_eq!(many.to_vec(), one, "{version:?}");
            all.extend(one);
        }
        Some(all)
    };
    let whole = check(&image).unwrap();
    assert!(whole[..5].iter().all(Option::is_some), "the five functions resolve: {whole:?}");
    // Every byte after the ELF header, damaged three ways: the answers still agree, and the damage
    // changes some answers (the comparison is not vacuous).
    let (mut parsed, mut changed) = (0usize, 0usize);
    for at in 64..image.len() {
        let orig = image[at];
        for b in [0u8, 0xff, orig ^ 1] {
            image[at] = b;
            if let Some(got) = check(&image) {
                parsed += 1;
                changed += usize::from(got != whole);
            }
        }
        image[at] = orig;
    }
    assert!(parsed > image.len() && changed > 10, "parsed {parsed}, changed {changed}");
    // A name defined twice, the first time with another version: the walk passes over the first
    // definition and finds the second (symbol i takes symbol j's name and version index 1, the
    // image's own name, which is never LINUX_2.6).
    let (symtab, versym, nsyms) = vdso::Vdso::parse(&image).unwrap().layout();
    let versym = versym.expect("the vDSO has version indexes");
    let mut second = 0usize;
    for j in 0..nsyms {
        for i in 0..j {
            let mut img = image.clone();
            let (si, sj) = (symtab + 24 * i, symtab + 24 * j);
            img.copy_within(sj..sj + 4, si);
            img[si + 4..si + 8].copy_from_slice(&image[sj + 4..sj + 8]);
            img[versym + 2 * i..versym + 2 * i + 2].copy_from_slice(&1u16.to_le_bytes());
            let v = vdso::Vdso::parse(&img).unwrap();
            let many = v.lookup_many(&names, b"LINUX_2.6");
            let one: Vec<_> = names.iter().map(|n| v.lookup_one_by_one(n, b"LINUX_2.6")).collect();
            assert_eq!(many.to_vec(), one, "symbol {i} renamed as {j}");
            // Nothing lost: symbol i's own name was not asked and symbol j's still resolves.
            second += usize::from(many.iter().filter(|m| m.is_some()).count() == whole[..names.len()].iter().filter(|m| m.is_some()).count());
        }
    }
    assert!(second > 0, "some duplicate still resolves through its second definition");
}

#[test]
fn k_signal_frame_and_restorer() {
    // A handler installed with SA_RESTORER and the boundary's trampoline runs once and
    // returns into the interrupted code, for an ordinary and a real-time signal, and on an
    // alternate stack.
    in_child(|_| {
        for sig in [SIGUSR1, SIGUSR2, SIGRTMIN_KERNEL + 1] {
            if selftest::signal_roundtrip(sig) != Ok((1, sig)) {
                return sig;
            }
        }
        if selftest::altstack_roundtrip(SIGUSR2) != Ok(true) {
            return 100;
        }
        // SIGKILL cannot be caught.
        i32::from(selftest::signal_roundtrip(SIGKILL) != Err(Errno::EINVAL)) * 101
    });
}

#[test]
fn k_thread_pointer_register() {
    // %fs:0 holds the thread pointer itself, and it differs per thread.
    let tp = K.thread_pointer();
    assert_ne!(tp, 0);
    assert_eq!(K.arch_prctl_get(ArchGet::Fs).unwrap() as usize, tp);
    let other = std::thread::spawn(|| K.thread_pointer()).join().unwrap();
    assert_ne!(other, tp);
    // The x86_64 TLS block lies below the thread pointer: a thread-local's address is too.
    thread_local!(static X: u8 = const { 0 });
    let addr = X.with(|x| std::ptr::from_ref(x).addr());
    assert!(addr < tp && tp - addr < (1 << 20));
}

#[test]
fn k_shared_memory_protocols() {
    // Futex words shared across processes (non-private futex on MAP_SHARED memory), the
    // robust list, clear_child_tid and rseq are each covered by their own primitives; this
    // checks the cross-process futex wake specifically.
    let region = K.mmap(0, 4096, PROT_READ | PROT_WRITE, MAP_SHARED, None, 0).unwrap();
    let words = K.region_words(&region).unwrap();
    let pid = selftest::spawn_waiter_on_shared_word(&words[0]).unwrap();
    std::thread::sleep(Duration::from_millis(100));
    words[0].store(1, Ordering::SeqCst);
    let mut woke = 0;
    for _ in 0..200 {
        woke = K.futex(&words[0], FUTEX_WAKE, 1, FutexArg::None, None, 0).unwrap();
        if woke == 1 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let (_, st) = K.wait4(pid, 0, None).unwrap();
    assert_eq!((woke, selftest::decode_status(st)), (1, ChildEnd::Exited(0)));
    K.munmap(region).unwrap();
}

#[test]
fn k_cpu_instructions_requiring_os_support() {
    // CPUID leaf 0: vendor and maximum leaf.
    let [max, ebx, ecx, edx] = K.cpuid(0, 0);
    assert!(max >= 1);
    let vendor: Vec<u8> = [ebx, edx, ecx].iter().flat_map(|w| w.to_le_bytes()).collect();
    assert!(vendor == b"GenuineIntel" || vendor == b"AuthenticAMD", "{}", String::from_utf8_lossy(&vendor));
    // XGETBV: x87 and SSE state always enabled by an x86_64 kernel.
    let xcr0 = K.xcr0().expect("OSXSAVE");
    assert_eq!(xcr0 & 0b11, 0b11);
    // MXCSR: default 0x1f80 (all exceptions masked, round to nearest); writable and restored.
    let m = K.mxcsr();
    K.set_mxcsr((m & !0x6000) | 0x6000); // round toward zero
    assert_eq!(K.mxcsr() & 0x6000, 0x6000);
    K.set_mxcsr(m);
    assert_eq!(K.mxcsr(), m);
    // x87 control word round trip, environment store/load, exception clearing.
    let cw = K.x87_control();
    K.set_x87_control(cw ^ 0x0c00);
    assert_eq!(K.x87_control(), cw ^ 0x0c00);
    let env = K.x87_store_env();
    assert_eq!(env.control & 0xffff, u32::from(cw ^ 0x0c00));
    K.set_x87_control(cw);
    K.x87_load_env(&env);
    assert_eq!(K.x87_control(), cw ^ 0x0c00);
    K.set_x87_control(cw);
    K.x87_clear_exceptions();
    assert_eq!(K.x87_status() & 0x3f, 0);
    // PKRU: only when the OS enabled protection keys (OSPKE); key 0 is never revoked.
    match K.pkru() {
        Some(p) => {
            assert!(K.set_pkru(p | 3));
            assert_eq!(K.pkru().unwrap() & 3, 0, "key 0 stays accessible");
            assert!(K.set_pkru(p));
        }
        None => assert!(!K.set_pkru(0)),
    }
    // The explicit-zeroing barrier accepts any buffer.
    K.barrier(&[1, 2, 3]);
    K.barrier(&[]);
}

#[test]
fn k_proc() {
    // The /proc files the spec relies on exist and have the expected shape.
    let mut buf = [0u8; 256];
    let n = K.readlinkat(Fd::CWD, kp("/proc/self/fd/0").as_kstr(), &mut buf);
    assert!(n.is_ok() || n == Err(Errno::ENOENT));
    for p in ["/proc/self/maps", "/proc/self/mounts", "/proc/stat", "/proc/loadavg", "/proc/sys/fs/pipe-max-size", "/proc/sys/kernel/ngroups_max", "/proc/self/status"] {
        let fd = K.openat(Fd::CWD, kp(p).as_kstr(), O_RDONLY | O_CLOEXEC, 0).unwrap_or_else(|e| panic!("{p}: {e:?}"));
        assert!(K.read(fd, &mut buf).unwrap() > 0, "{p}");
        close(fd);
    }
    let comm = format!("/proc/self/task/{}/comm", K.gettid());
    assert!(std::fs::read(comm).unwrap().ends_with(b"\n"));
    let fdinfo = K.pidfd_open(K.getpid(), 0).unwrap();
    let info = std::fs::read_to_string(format!("/proc/self/fdinfo/{}", fdinfo.raw())).unwrap();
    assert!(info.lines().any(|l| l == format!("Pid:\t{}", K.getpid())), "pidfd fdinfo reports the PID");
    close(fdinfo);
    assert_eq!(std::fs::read_to_string("/proc/sys/kernel/ngroups_max").unwrap().trim(), "65536");
}

#[test]
fn k_sys() {
    let online = std::fs::read_to_string("/sys/devices/system/cpu/online").unwrap();
    assert!(online.starts_with('0'));
    assert!(std::fs::metadata("/sys/devices/system/cpu/cpu0").unwrap().is_dir());
    let thp = std::fs::read_to_string("/sys/kernel/mm/transparent_hugepage/enabled");
    assert!(thp.map_or(true, |s| s.contains('[')));
}

#[test]
fn k_device_nodes() {
    for (path, major, minor) in [("/dev/null", 1, 3), ("/dev/zero", 1, 5), ("/dev/urandom", 1, 9), ("/dev/random", 1, 8), ("/dev/tty", 5, 0), ("/dev/ptmx", 5, 2)] {
        let st = K.fstatat(Fd::CWD, kp(path).as_kstr(), 0).unwrap_or_else(|e| panic!("{path}: {e:?}"));
        assert_eq!(st.mode & S_IFMT, S_IFCHR, "{path}");
        assert_eq!((st.rdev >> 8 & 0xfff, st.rdev & 0xff), (major, minor), "{path}");
    }
    assert!(std::fs::metadata("/dev/pts").unwrap().is_dir());
    let fd = K.openat(Fd::CWD, kp("/dev/urandom").as_kstr(), O_RDONLY | O_CLOEXEC, 0).unwrap();
    let mut b = [0u8; 16];
    assert_eq!(K.read(fd, &mut b).unwrap(), 16);
    close(fd);
}

#[test]
fn k_netlink_netlink_route() {
    // RTM_GETLINK dump: at least the loopback interface comes back, then NLMSG_DONE.
    let s = K.socket(AF_NETLINK, SOCK_RAW | SOCK_CLOEXEC, NETLINK_ROUTE).unwrap();
    let mut req = [0u8; 32]; // nlmsghdr (16) + ifinfomsg (16)
    req[..4].copy_from_slice(&32u32.to_ne_bytes());
    req[4..6].copy_from_slice(&RTM_GETLINK.to_ne_bytes());
    req[6..8].copy_from_slice(&(NLM_F_REQUEST | NLM_F_DUMP).to_ne_bytes());
    req[8..12].copy_from_slice(&1u32.to_ne_bytes());
    req[16] = AF_UNSPEC as u8;
    let mut kernel = vec![0u8; 12];
    kernel[..2].copy_from_slice(&(AF_NETLINK as u16).to_ne_bytes());
    assert_eq!(K.sendto(s, &req, 0, Some(&kernel)).unwrap(), 32);
    let mut links = 0;
    let mut done = false;
    let mut buf = vec![0u8; 1 << 16];
    while !done {
        let n = K.recvfrom(s, &mut buf, 0, None).unwrap();
        let mut i = 0;
        while i + 16 <= n {
            let len = u32::from_ne_bytes(buf[i..i + 4].try_into().unwrap()) as usize;
            let ty = u16::from_ne_bytes(buf[i + 4..i + 6].try_into().unwrap());
            match ty {
                RTM_NEWLINK => links += 1,
                NLMSG_DONE => done = true,
                NLMSG_ERROR => panic!("netlink error"),
                _ => {}
            }
            i += (len + 3) & !3;
            if len == 0 {
                break;
            }
        }
    }
    assert!(links >= 1);
    close(s);
}

#[test]
fn k_kernel_error_numbers() {
    // C0.1: -errno in [-4095, -1] is an error; everything else is a value.
    assert_eq!(Errno::decode(-2), Err(Errno::ENOENT));
    assert_eq!(Errno::decode(-4095).map_err(|e| e.get()), Err(22), "unknown numbers decode as errors (EINVAL)");
    assert_eq!(Errno::decode(-4096), Ok((-4096i64) as u64), "an address in the top page is a value");
    assert_eq!(Errno::decode(0), Ok(0));
    assert_eq!(Errno::decode(i64::MAX), Ok(i64::MAX as u64));
    // The numbering is the kernel's; names round-trip.
    assert_eq!((Errno::EPERM.get(), Errno::ENOENT.get(), Errno::EAGAIN.get(), Errno::EWOULDBLOCK.get()), (1, 2, 11, 11));
    assert_eq!(Errno::ENOENT.name(), Some("ENOENT"));
    assert_eq!(Errno::new(0), None);
    assert_eq!(Errno::new(i32::from(liella_kernel::errno::MAX_ERRNO) + 1), None);
    // And from the real kernel, through the C layer.
    assert_eq!(err(K.close(Fd::new(-1))), Errno::EBADF);
    assert_eq!(err(K.openat(Fd::CWD, kp("/nonexistent").as_kstr(), O_RDONLY, 0)), Errno::ENOENT);
}

#[test]
fn k_expect_crash_not_dumpable() {
    // CORE-STORM: expect_crash (and in_child_quiet) make only the child that is about to crash on
    // purpose non-dumpable, so no core handler starts for it; every other process stays dumpable,
    // so an unexpected crash still dumps.
    assert_eq!(selftest::in_child(|_| { selftest::expect_crash(); i32::from(selftest::dumpable() != Ok(0)) }).unwrap(), ChildEnd::Exited(0), "expect_crash: PR_GET_DUMPABLE 0");
    assert_eq!(selftest::in_child_quiet(|_| i32::from(selftest::dumpable() != Ok(0))).unwrap(), ChildEnd::Exited(0), "in_child_quiet: PR_GET_DUMPABLE 0");
    assert_eq!(selftest::in_child(|_| i32::from(selftest::dumpable() != Ok(1))).unwrap(), ChildEnd::Exited(0), "a normal child stays dumpable");
    assert_eq!(selftest::dumpable(), Ok(1), "the test process stays dumpable");
}
