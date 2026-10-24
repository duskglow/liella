// Specified from spec §11 (phase 5; the cancellation clauses phase 6; the asynchronous I/O
// clauses, §11.11, phase 13).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §11 (114 clauses).
//!
//! The thin calls are tested through the exports (`liella_boundary::posix::s11_lowio`), which is
//! the contract a C caller sees; what glibc adds is also tested where it lives
//! (`liella_legacy::io_compat`), with the kernel's failures injected by `Faulty`. The
//! asynchronous I/O clauses run on both of its engines (io_uring and the worker pool, D-11.8).
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

#[path = "support/kernel.rs"]
mod kernel;
#[path = "support/sync_trap.rs"]
mod sync_trap;

use core::ptr::null_mut;
use std::ffi::{CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::sync::atomic::{AtomicU32, Ordering};

use liella::error::Errno;
use liella_abi::ctypes::c_int;
use liella_boundary::errno;
use liella_boundary::posix::s11_lowio as x;
use liella_boundary::sys::CSys;
use liella_kernel::fd::Fd;
use liella_kernel::mem::{IoVec, IoVecMut};
use liella_kernel::ops::FcntlInt;
use liella_kernel::sys::Sys;
use liella_kernel::testing::Faulty;
use liella_kernel::uapi::{EpollEvent, FdSet, PollFd, Timespec, Timeval};
use liella_legacy::io_compat;

const O_RDONLY: c_int = 0;
const O_RDWR: c_int = 2;
const O_CREAT: c_int = 0o100;
const O_EXCL: c_int = 0o200;
const O_TRUNC: c_int = 0o1000;
const O_APPEND: c_int = 0o2000;
const O_NONBLOCK: c_int = 0o4000;
const O_DIRECTORY: c_int = 0o200000;
const O_NOFOLLOW: c_int = 0o400000;
const O_CLOEXEC: c_int = 0o2000000;
const O_TMPFILE: c_int = 0o20200000;
const O_PATH: c_int = 0o10000000;

static SEQ: AtomicU32 = AtomicU32::new(0);

/// A scratch path under the target directory, for a file, directory or link a test creates; it is
/// removed with whatever it holds when dropped, so a failing test leaves nothing behind either.
struct Tmp(CString);

impl core::ops::Deref for Tmp {
    type Target = CString;
    fn deref(&self) -> &CString {
        &self.0
    }
}

impl Drop for Tmp {
    fn drop(&mut self) {
        let p = std::path::Path::new(std::ffi::OsStr::from_bytes(self.0.as_bytes()));
        if std::fs::remove_file(p).is_err() {
            let _ = std::fs::remove_dir_all(p);
        }
    }
}

/// A fresh scratch path (in a re-executed child, in the directory its parent made for it).
fn tmp(tag: &str) -> Tmp {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let base = std::env::var_os(CHILD_TMP).map_or_else(|| std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")), std::path::PathBuf::from);
    let p = base.join(format!("c11-{}-{tag}-{n}", std::process::id()));
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_dir_all(&p);
    Tmp(CString::new(p.into_os_string().into_encoded_bytes()).unwrap())
}

fn file(tag: &str, content: &[u8]) -> (Tmp, c_int) {
    let p = tmp(tag);
    let fd = x::liella_open3(p.as_ptr(), O_RDWR | O_CREAT | O_TRUNC, 0o600);
    assert!(fd >= 0);
    assert_eq!(x::write(fd, content.as_ptr().cast(), content.len()), content.len() as isize);
    (p, fd)
}

/// A memory file (memfd) holding `content`, for the durability calls (`fsync`, `fdatasync`,
/// `syncfs`, `msync`, `sync_file_range`): on the target file system they wait for the journal and
/// with it every dirty page of a parallel build (GATE-FSYNC: up to 80 s each under gate load).
/// The library's part - the call reaches the kernel, its result - is the same on any regular file;
/// durability itself is the kernel's (§11.8 Kernel) and no test can observe it. A memfd's sync
/// returns at once.
fn mem_file(tag: &CStr, content: &[u8]) -> c_int {
    let fd = x::memfd_create(tag.as_ptr(), 1);
    assert!(fd >= 0, "memfd_create: errno {}", errno::get());
    assert_eq!(x::write(fd, content.as_ptr().cast(), content.len()), content.len() as isize);
    fd
}

fn pipe() -> (c_int, c_int) {
    let [r, w] = CSys.pipe2(0).unwrap();
    (r.raw(), w.raw())
}

/// -1 with `errno == e`.
fn err(r: isize, e: Errno) -> bool {
    r == -1 && errno::get() == e.get()
}

fn fdflags(fd: c_int) -> i64 {
    CSys.fcntl_int(Fd::new(fd), FcntlInt::GetFd, 0).unwrap()
}

/// Runs `body` in a single-threaded child (this test re-executed alone): descriptor
/// numbering is process-wide, so "the lowest free number" holds only without concurrent tests.
fn alone(name: &str, body: impl FnOnce() -> bool) {
    if std::env::var("LIELLA_C11_ALONE").is_ok_and(|v| v == name) {
        CSys.exit_group(if body() { 0 } else { 1 });
    }
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args([name, "--exact", "--test-threads=1"]).env("LIELLA_C11_ALONE", name);
    let _scratch = child_scratch(&mut cmd);
    let o = cmd.output().unwrap();
    assert_eq!(o.status.code(), Some(0), "{name} failed in its child: {}", child_report(&o));
}

/// The variable naming a re-executed child's scratch directory ([`child_scratch`]).
const CHILD_TMP: &str = "LIELLA_C11_TMP";

/// A directory for the scratch files of the child `cmd` runs ([`tmp`] uses it there), removed by
/// this process with whatever the child left when the value goes: a child that ends with
/// `exit_group` drops nothing.
fn child_scratch(cmd: &mut std::process::Command) -> Tmp {
    let d = tmp("child");
    std::fs::create_dir(d.to_str().unwrap()).unwrap();
    cmd.env(CHILD_TMP, d.to_str().unwrap());
    d
}

fn conformance(names: &[&str]) {
    static RUNNER: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = RUNNER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert!(root.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let o = std::process::Command::new("sh").arg(root.join("conformance/run.sh")).args(names).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

#[test]
fn s11_n_1() {
    // Thin wrappers: the kernel's contract, plus the errno convention (the failure is -1 with errno).
    errno::set_raw(0);
    assert!(err(x::close(-1) as isize, Errno::EBADF));
    assert!(err(x::read(-1, null_mut(), 0), Errno::EBADF));
}

#[test]
fn s11_n_1_1() {
    errno::set_raw(0);
    let mut b = [0u8; 1];
    assert_eq!(x::read(-1, b.as_mut_ptr().cast(), 1), -1);
    assert_eq!(errno::get(), Errno::EBADF.get(), "the kernel's -errno, transferred to errno (C0.1)");
    let (_f, fd) = file("n11", b"ok");
    errno::set_raw(77);
    assert_eq!(x::lseek(fd, 0, 0), 0);
    assert_eq!(errno::get(), 77, "success leaves errno alone");
    x::close(fd);
}

/// §11 Notes: thread cancellation (C0.2), for blocking calls;
#[test]
fn s11_n_1_2() {
    conformance(&["s22_cancel_points", "s22_cancel"]);
}
#[test]
fn s11_n_1_3() {
    // The 64-bit aliases are the same functions (one off_t on this ABI, C0.4).
    let (_f, fd) = file("n13", b"0123456789");
    let mut a = [0u8; 4];
    let mut b = [0u8; 4];
    assert_eq!(x::pread(fd, a.as_mut_ptr().cast(), 4, 3), x::pread64(fd, b.as_mut_ptr().cast(), 4, 3));
    assert_eq!(a, b);
    assert_eq!(x::lseek(fd, 1 << 40, 0), x::lseek64(fd, 1 << 40, 0), "offsets beyond 32 bits");
    x::close(fd);
}

#[test]
fn s11_n_1_4() {
    // Emulation: posix_fallocate on a file system without fallocate (injected EOPNOTSUPP).
    let (_f, fd) = file("n14", b"keep");
    let k = Faulty::new(CSys);
    assert!(k.fail("fallocate", Errno::EOPNOTSUPP, 0, 1));
    io_compat::posix_fallocate(&k, Fd::new(fd), 0, 20000).unwrap();
    assert_eq!(k.injected(), 1, "the kernel refused; the emulation ran");
    assert_eq!(x::lseek(fd, 0, 2), 20000);
    let mut b = [0u8; 4];
    x::pread(fd, b.as_mut_ptr().cast(), 4, 0);
    assert_eq!(&b, b"keep");
    x::close(fd);
}

#[test]
fn s11_n_2() {
    // [thin]: the kernel's documented behaviour, unaltered: a short read from a pipe returns
    // what was there.
    let (r, w) = pipe();
    x::write(w, b"ab".as_ptr().cast(), 2);
    let mut b = [0u8; 10];
    assert_eq!(x::read(r, b.as_mut_ptr().cast(), 10), 2);
    x::close(r);
    x::close(w);
}

#[test]
fn s11_n_3() {
    // Descriptor numbering and description sharing are the kernel's.
    alone("s11_n_3", || {
        let (_f, fd) = file("n3", b"0123");
        let d = x::dup(fd);
        x::lseek(d, 2, 0);
        let shared = x::lseek(fd, 0, 1) == 2; // one offset per open file description
        x::close(d);
        let lowest = x::dup(fd) == d; // the lowest free number again
        shared && lowest
    });
}

#[test]
fn s11_1_c_0() {
    let (p, fd) = file("c0", b"");
    x::close(fd);
    let fd = x::liella_open3(p.as_ptr(), O_RDONLY, 0);
    assert!(fd >= 0, "relative to the current directory (or absolute)");
    let parent = CString::new(std::path::Path::new(p.to_str().unwrap()).parent().unwrap().to_str().unwrap()).unwrap();
    let dir = x::liella_open3(parent.as_ptr(), O_RDONLY | O_DIRECTORY, 0);
    let name = CString::new(std::path::Path::new(p.to_str().unwrap()).file_name().unwrap().to_str().unwrap()).unwrap();
    let at = x::liella_openat4(dir, name.as_ptr(), O_RDONLY, 0);
    assert!(at >= 0 && at != fd, "relative to a directory descriptor: a new description");
    for f in [fd, dir, at] {
        x::close(f);
    }
}

#[test]
fn s11_1_i_1() {
    let (p, fd) = file("i1", b"x");
    x::close(fd);
    let ro = x::liella_open3(p.as_ptr(), O_RDONLY, 0);
    errno::set_raw(0);
    assert!(err(x::write(ro, b"y".as_ptr().cast(), 1), Errno::EBADF), "O_RDONLY");
    let path = x::liella_open3(p.as_ptr(), O_PATH, 0);
    let mut b = [0u8; 1];
    assert!(err(x::read(path, b.as_mut_ptr().cast(), 1), Errno::EBADF), "O_PATH: no I/O");
    x::close(ro);
    x::close(path);
}

#[test]
fn s11_1_i_2() {
    // The creation flags as a group: create, exclusive, truncate.
    let p = tmp("i2");
    let fd = x::liella_open3(p.as_ptr(), O_RDWR | O_CREAT | O_EXCL, 0o600);
    assert!(fd >= 0);
    x::write(fd, b"data".as_ptr().cast(), 4);
    x::close(fd);
    let fd = x::liella_open3(p.as_ptr(), O_RDWR | O_TRUNC, 0);
    assert_eq!(x::lseek(fd, 0, 2), 0);
    x::close(fd);
}

#[test]
fn s11_1_i_2_1() {
    let p = tmp("i21");
    let fd = x::liella_open3(p.as_ptr(), O_RDWR | O_CREAT | O_EXCL, 0o600);
    errno::set_raw(0);
    assert!(err(x::liella_open3(p.as_ptr(), O_RDWR | O_CREAT | O_EXCL, 0o600) as isize, Errno::EEXIST));
    x::write(fd, b"abc".as_ptr().cast(), 3);
    let t = x::liella_open3(p.as_ptr(), O_RDWR | O_TRUNC, 0);
    assert_eq!(x::lseek(t, 0, 2), 0, "O_TRUNC");
    x::close(fd);
    x::close(t);
}

#[test]
fn s11_1_i_2_2() {
    let (target, fd) = file("i22", b"");
    x::close(fd);
    let link = tmp("i22l");
    std::os::unix::fs::symlink(target.to_str().unwrap(), link.to_str().unwrap()).unwrap();
    errno::set_raw(0);
    assert!(err(x::liella_open3(link.as_ptr(), O_RDONLY | O_NOFOLLOW, 0) as isize, Errno::ELOOP), "O_NOFOLLOW");
    assert!(err(x::liella_open3(target.as_ptr(), O_RDONLY | O_DIRECTORY, 0) as isize, Errno::ENOTDIR), "O_DIRECTORY");
    let nc = x::liella_open3(target.as_ptr(), O_RDONLY | 0o400, 0);
    assert!(nc >= 0, "O_NOCTTY on a regular file");
    x::close(nc);
}

#[test]
fn s11_1_i_2_3() {
    let (p, fd) = file("i23", b"");
    x::close(fd);
    let ce = x::liella_open3(p.as_ptr(), O_RDONLY | O_CLOEXEC, 0);
    assert_eq!(fdflags(ce), 1, "O_CLOEXEC: FD_CLOEXEC set");
    let t = x::liella_open3(c"/tmp".as_ptr(), O_TMPFILE | O_RDWR, 0o600);
    assert!(t >= 0 || [Errno::EOPNOTSUPP.get(), Errno::EISDIR.get()].contains(&errno::get()), "O_TMPFILE: an unnamed file");
    x::close(ce);
    x::close(t);
}

#[test]
fn s11_1_i_3() {
    let (p, fd) = file("i3", b"start");
    x::close(fd);
    let a = x::liella_open3(p.as_ptr(), O_RDWR | O_APPEND | O_NONBLOCK, 0);
    x::lseek(a, 0, 0);
    x::write(a, b"!".as_ptr().cast(), 1);
    assert_eq!(x::lseek(a, 0, 1), 6, "O_APPEND writes at the end");
    let fl = CSys.fcntl_int(Fd::new(a), FcntlInt::GetFl, 0).unwrap() as c_int;
    assert_eq!(fl & (O_APPEND | O_NONBLOCK), O_APPEND | O_NONBLOCK, "status flags are kept");
    x::close(a);
}

#[test]
fn s11_1_i_4() {
    // A mode is required iff O_CREAT/O_TMPFILE: the fortify form checks it (conformance and
    // symbol tests cover the abort), and a mode passed without O_CREAT is ignored.
    assert!(io_compat::needs_mode(O_CREAT) && io_compat::needs_mode(O_TMPFILE | O_RDWR));
    assert!(!io_compat::needs_mode(O_RDWR | O_DIRECTORY));
    let (p, fd) = file("i4", b"");
    x::close(fd);
    let r = x::liella_open3(p.as_ptr(), O_RDONLY, 0o777);
    assert!(r >= 0);
    x::close(r);
}

#[test]
fn s11_1_g_1() {
    // The lowest free descriptor: close one in a run of three, the next open reuses it.
    alone("s11_1_g_1", || {
        let (p, a) = file("g1", b"");
        let b = x::dup(a);
        let _c = x::dup(a);
        x::close(b);
        x::liella_open3(p.as_ptr(), O_RDONLY, 0) == b // open takes the lowest free number
    });
}

#[test]
fn s11_1_g_2() {
    // O_CREAT|O_EXCL is atomic: of many threads racing to create, exactly one wins.
    let p = tmp("g2");
    let winners: usize = (0..8)
        .map(|_| {
            let p = p.clone();
            std::thread::spawn(move || {
                let fd = x::liella_open3(p.as_ptr(), O_RDWR | O_CREAT | O_EXCL, 0o600);
                if fd >= 0 {
                    x::close(fd);
                    1
                } else {
                    0
                }
            })
        })
        .collect::<Vec<_>>()
        .into_iter()
        .map(|h| h.join().unwrap())
        .sum();
    assert_eq!(winners, 1);
}

#[test]
fn s11_1_g_3() {
    // The mode is masked by umask (here 022 in a child, so the test process is unaffected).
    if std::env::var("LIELLA_C11_UMASK").is_ok() {
        let _ = CSys.umask(0o022);
        let p = tmp("g3");
        let fd = x::liella_open3(p.as_ptr(), O_RDWR | O_CREAT, 0o666);
        let mode = CSys.fstat(Fd::new(fd)).unwrap().mode & 0o777;
        CSys.exit_group(if mode == 0o644 { 0 } else { 1 });
    }
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args(["s11_1_g_3", "--exact", "--test-threads=1"]).env("LIELLA_C11_UMASK", "1");
    let _scratch = child_scratch(&mut cmd);
    let o = cmd.output().unwrap();
    assert_eq!(o.status.code(), Some(0));
}

#[test]
fn s11_1_g_4() {
    let (p, fd) = file("g4", b"old");
    x::close(fd);
    let c = x::creat(p.as_ptr(), 0o600);
    assert_eq!(x::lseek(c, 0, 2), 0, "O_TRUNC");
    let fl = CSys.fcntl_int(Fd::new(c), FcntlInt::GetFl, 0).unwrap() as c_int;
    assert_eq!(fl & 3, 1, "O_WRONLY");
    assert_eq!(io_compat::CREAT_FLAGS, (O_CREAT | 1 | O_TRUNC) as u32);
    x::close(c);
}

#[test]
fn s11_1_f_0() {
    errno::set_raw(0);
    assert!(err(x::liella_open3(tmp("missing").as_ptr(), O_RDONLY, 0) as isize, Errno::ENOENT));
    assert!(err(x::liella_open3(c"/etc/passwd/x".as_ptr(), O_RDONLY, 0) as isize, Errno::ENOTDIR));
    assert!(err(x::liella_open3(c"/".as_ptr(), O_RDWR, 0) as isize, Errno::EISDIR));
    let long = CString::new(vec![b'a'; 300]).unwrap();
    assert!(err(x::liella_open3(long.as_ptr(), O_RDONLY, 0) as isize, Errno::ENAMETOOLONG));
}

/// §11.1 Cancellation: Yes.
#[test]
fn s11_1_x_0() {
    conformance(&["s22_cancel_points"]);
}
#[test]
fn s11_2_c_0() {
    let (_f, a) = file("c20", b"");
    let b = x::dup(a);
    assert_eq!(x::close(a), 0);
    assert_eq!(x::write(b, b"x".as_ptr().cast(), 1), 1, "the description survives while a descriptor remains");
    // Private high numbers: closing numbers another test may hold would race with it.
    let (c, d) = (x::dup2(b, 901), x::dup2(b, 902));
    assert_eq!((c, d), (901, 902));
    assert_eq!(x::close_range(901, 902, 0), 0);
    errno::set_raw(0);
    assert!(err(x::close(c) as isize, Errno::EBADF) && err(x::close(d) as isize, Errno::EBADF));
    x::close(b);
}

#[test]
fn s11_2_g_0() {
    // Released even on error: a close that reports EIO/EINTR still frees the number; a second
    // close of it is EBADF.
    let (_f, f) = file("g20", b"");
    let fd = x::dup2(f, 903); // a private number nothing else will be given meanwhile
    x::close(f);
    x::close(fd);
    errno::set_raw(0);
    assert!(err(x::close(fd) as isize, Errno::EBADF));
}

#[test]
fn s11_2_f_0() {
    errno::set_raw(0);
    assert!(err(x::close(-5) as isize, Errno::EBADF));
}

#[test]
fn s11_3_c_1() {
    let (_f, fd) = file("c31", b"abcdef");
    x::lseek(fd, 0, 0);
    let mut b = [0u8; 4];
    assert_eq!(x::read(fd, b.as_mut_ptr().cast(), 4), 4);
    assert_eq!(x::lseek(fd, 0, 1), 4, "the offset advances");
    assert_eq!(x::read(fd, b.as_mut_ptr().cast(), 4), 2, "short at end of file");
    assert_eq!(x::read(fd, b.as_mut_ptr().cast(), 4), 0, "0 at end of file");
    x::close(fd);
}

#[test]
fn s11_3_c_2() {
    let (r, w) = pipe();
    x::liella_fcntl3(w, 4, O_NONBLOCK as usize);
    let big = vec![1u8; 1 << 20];
    let n = x::write(w, big.as_ptr().cast(), big.len());
    assert!(n > 0 && (n as usize) < big.len(), "short: the pipe's capacity");
    x::close(r);
    x::close(w);
}

#[test]
fn s11_3_c_3() {
    let (p, fd) = file("c33", b"0123456789");
    let mut b = [0u8; 2];
    x::lseek(fd, 1, 0);
    x::pread(fd, b.as_mut_ptr().cast(), 2, 5);
    assert_eq!((&b, x::lseek(fd, 0, 1)), (b"56", 1), "explicit offset, the offset unmoved");
    x::close(fd);
    let a = x::liella_open3(p.as_ptr(), O_RDWR | O_APPEND, 0);
    x::pwrite(a, b"X".as_ptr().cast(), 1, 0);
    let mut all = [0u8; 11];
    assert_eq!(x::pread(a, all.as_mut_ptr().cast(), 11, 0), 11, "pwrite on O_APPEND appends (Linux)");
    assert_eq!(all[10], b'X');
    x::close(a);
}

#[test]
fn s11_3_c_4() {
    let (_f, fd) = file("c34", b"");
    let v = [IoVec::new(b"one,"), IoVec::new(b""), IoVec::new(b"two")];
    assert_eq!(x::writev(fd, v.as_ptr().cast(), 3), 7);
    let (mut a, mut b) = ([0u8; 2], [0u8; 5]);
    let mut r = [IoVecMut::new(&mut a), IoVecMut::new(&mut b)];
    assert_eq!(x::preadv(fd, r.as_mut_ptr().cast(), 2, 0), 7);
    assert_eq!((&a, &b), (b"on", b"e,two"));
    errno::set_raw(0);
    assert!(err(x::writev(fd, v.as_ptr().cast(), 1025), Errno::EINVAL), "more than IOV_MAX");
    x::close(fd);
}

#[test]
fn s11_3_c_5() {
    let (_f, fd) = file("c35", b"abc");
    x::lseek(fd, 1, 0);
    let v = [IoVec::new(b"Z")];
    assert_eq!(x::pwritev2(fd, v.as_ptr().cast(), 1, -1, 0), 1, "-1: the current offset");
    assert_eq!(x::lseek(fd, 0, 1), 2);
    assert_eq!(x::pwritev2(fd, v.as_ptr().cast(), 1, 0, 0x2), 1, "RWF_DSYNC");
    errno::set_raw(0);
    assert!(err(x::pwritev2(fd, v.as_ptr().cast(), 1, 0, 0x4000_0000), Errno::EOPNOTSUPP), "unknown flag");
    x::close(fd);
}

#[test]
fn s11_3_i_1() {
    // n up to SSIZE_MAX is accepted; Linux caps one transfer (0x7ffff000), so a larger request
    // is a short transfer, not an error. Above SSIZE_MAX the buffer cannot exist (EFAULT).
    let fd = x::liella_open3(c"/dev/zero".as_ptr(), O_RDONLY, 0);
    let mut b = vec![0u8; 1 << 16];
    assert_eq!(x::read(fd, b.as_mut_ptr().cast(), b.len()), 1 << 16);
    errno::set_raw(0);
    assert!(err(x::read(fd, b.as_mut_ptr().cast(), usize::MAX), Errno::EFAULT));
    x::close(fd);
}

#[test]
fn s11_3_i_2() {
    // Writes of at most PIPE_BUF (4096) bytes to a pipe are atomic: concurrent writers never
    // interleave within one.
    let (r, w) = pipe();
    let writers: Vec<_> = (0..4u8)
        .map(|k| {
            std::thread::spawn(move || {
                let block = [b'a' + k; 4096];
                for _ in 0..8 {
                    assert_eq!(x::write(w, block.as_ptr().cast(), 4096), 4096);
                }
            })
        })
        .collect();
    let mut total = Vec::new();
    let mut b = [0u8; 4096];
    while total.len() < 4 * 8 * 4096 {
        let n = x::read(r, b.as_mut_ptr().cast(), 4096);
        total.extend_from_slice(&b[..n as usize]);
    }
    for h in writers {
        h.join().unwrap();
    }
    assert!(total.chunks(4096).all(|c| c.iter().all(|&v| v == c[0])), "each 4096-byte write arrived whole");
    x::close(r);
    x::close(w);
}

#[test]
fn s11_3_g_0() {
    let (_f, fd) = file("g30", b"");
    let data = [9u8; 10000];
    assert_eq!(x::write(fd, data.as_ptr().cast(), data.len()), 10000, "an exact count");
    let mut b = vec![0u8; 10000];
    assert_eq!(x::pread(fd, b.as_mut_ptr().cast(), 10000, 0), 10000, "in the page cache: readable at once");
    assert!(b.iter().all(|&v| v == 9));
    x::close(fd);
}

#[test]
fn s11_3_f_1() {
    let (r, w) = pipe();
    x::liella_fcntl3(r, 4, O_NONBLOCK as usize);
    let mut b = [0u8; 1];
    errno::set_raw(0);
    assert!(err(x::read(r, b.as_mut_ptr().cast(), 1), Errno::EAGAIN));
    x::close(r);
    x::close(w);
}

#[test]
fn s11_3_f_2() {
    // EINTR before any transfer: a blocked read interrupted by a signal (SIGALRM via a timer).
    if std::env::var("LIELLA_C11_EINTR").is_ok() {
        let (r, _w) = pipe();
        let (res, _) = liella_boundary::selftest::with_counting_handler(14, || {
            // Aimed at this thread: a process-directed signal may go to another (libtest's).
            let (pid, tid) = (CSys.getpid(), CSys.gettid());
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(100));
                let _ = CSys.tgkill(pid, tid, 14);
            });
            let mut b = [0u8; 1];
            (x::read(r, b.as_mut_ptr().cast(), 1), errno::get())
        })
        .unwrap();
        CSys.exit_group(if res == (-1, Errno::EINTR.get()) { 0 } else { 1 });
    }
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args(["s11_3_f_2", "--exact", "--test-threads=1"]).env("LIELLA_C11_EINTR", "1");
    let _scratch = child_scratch(&mut cmd);
    let o = cmd.output().unwrap();
    assert_eq!(o.status.code(), Some(0));
}

#[test]
fn s11_3_f_3() {
    // EPIPE (and SIGPIPE, here ignored in a child so the error is observable).
    if std::env::var("LIELLA_C11_EPIPE").is_ok() {
        let _ = CSys.sigaction_set(13, liella_kernel::sys::Disposition::Ignore, 0, liella_kernel::uapi::SigSet::empty());
        let (r, w) = pipe();
        x::close(r);
        errno::set_raw(0);
        let ok = err(x::write(w, b"x".as_ptr().cast(), 1), Errno::EPIPE);
        CSys.exit_group(if ok { 0 } else { 1 });
    }
    let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
    cmd.args(["s11_3_f_3", "--exact", "--test-threads=1"]).env("LIELLA_C11_EPIPE", "1");
    let _scratch = child_scratch(&mut cmd);
    let o = cmd.output().unwrap();
    assert_eq!(o.status.code(), Some(0));
}

#[test]
fn s11_3_f_4() {
    let d = x::liella_open3(c"/tmp".as_ptr(), O_RDONLY | O_DIRECTORY, 0);
    let mut b = [0u8; 1];
    errno::set_raw(0);
    assert!(err(x::read(d, b.as_mut_ptr().cast(), 1), Errno::EISDIR));
    assert!(err(x::read(-1, b.as_mut_ptr().cast(), 1), Errno::EBADF));
    assert!(err(x::read(d, core::ptr::null_mut(), 1), Errno::EFAULT));
    x::close(d);
}

/// §11.3 Cancellation: Yes.
#[test]
fn s11_3_x_0() {
    conformance(&["s22_cancel_points", "s22_cancel"]);
}
#[test]
fn s11_4_c_0() {
    let (_f, fd) = file("c40", b"0123456789");
    assert_eq!(x::lseek(fd, 4, 0), 4);
    assert_eq!(x::lseek(fd, 2, 1), 6);
    assert_eq!(x::lseek(fd, -3, 2), 7);
    assert_eq!(x::lseek(fd, 0, 3), 0, "SEEK_DATA: data starts at 0");
    assert_eq!(x::lseek(fd, 0, 4), 10, "SEEK_HOLE: the implicit hole at end of file");
    x::close(fd);
}

#[test]
fn s11_4_f_1() {
    let (r, w) = pipe();
    errno::set_raw(0);
    assert_eq!((x::lseek(r, 0, 0), errno::get()), (-1, Errno::ESPIPE.get()));
    x::close(r);
    x::close(w);
}

#[test]
fn s11_4_f_2() {
    let (_f, fd) = file("f42", b"abc");
    errno::set_raw(0);
    assert_eq!((x::lseek(fd, -1, 0), errno::get()), (-1, Errno::EINVAL.get()), "negative result");
    assert_eq!((x::lseek(fd, 0, 7), errno::get()), (-1, Errno::EINVAL.get()), "bad whence");
    x::close(fd);
}

#[test]
fn s11_4_f_3() {
    let (_f, fd) = file("f43", b"abc");
    errno::set_raw(0);
    assert_eq!((x::lseek(fd, 100, 3), errno::get()), (-1, Errno::ENXIO.get()));
    assert_eq!((x::lseek(fd, 100, 4), errno::get()), (-1, Errno::ENXIO.get()));
    x::close(fd);
}

#[test]
fn s11_4_f_4() {
    // A result beyond off_t (SEEK_CUR past i64::MAX) fails; so does one beyond the file
    // system's maximum file size, which on most file systems is reached first (EINVAL).
    let (_f, fd) = file("f44", b"");
    errno::set_raw(0);
    if x::lseek(fd, i64::MAX - 10, 0) == -1 {
        assert_eq!(errno::get(), Errno::EINVAL.get(), "beyond the file system's maximum");
    } else {
        assert_eq!(x::lseek(fd, 100, 1), -1);
        assert!([Errno::EOVERFLOW.get(), Errno::EINVAL.get()].contains(&errno::get()));
    }
    x::close(fd);
}

#[test]
fn s11_5_c_1() {
    let (_f, fd) = file("c51", b"xy");
    let d = x::dup(fd);
    assert!(d >= 0 && d != fd);
    x::lseek(fd, 1, 0);
    assert_eq!(x::lseek(d, 0, 1), 1, "shared description");
    assert_eq!(fdflags(d), 0, "the new descriptor's FD_CLOEXEC is clear");
    x::close(d);
    x::close(fd);
}

#[test]
fn s11_5_c_2() {
    let (_f, a) = file("c52a", b"A");
    let (_f, b) = file("c52b", b"B");
    assert_eq!(x::dup2(a, b), b);
    let mut c = [0u8; 1];
    x::pread(b, c.as_mut_ptr().cast(), 1, 0);
    assert_eq!(c[0], b'A', "b now refers to a's file");
    assert_eq!(x::dup2(a, a), a);
    errno::set_raw(0);
    assert!(err(x::dup2(-1, -1) as isize, Errno::EBADF), "old == new: old is validated");
    x::close(a);
    x::close(b);
}

#[test]
fn s11_5_c_3() {
    let (_f, a) = file("c53a", b"");
    let (_f, b) = file("c53b", b"");
    assert_eq!(x::dup3(a, b, O_CLOEXEC), b);
    assert_eq!(fdflags(b), 1);
    errno::set_raw(0);
    assert!(err(x::dup3(a, a, 0) as isize, Errno::EINVAL));
    assert!(err(x::dup3(a, b, 0o1) as isize, Errno::EINVAL), "flags other than O_CLOEXEC");
    x::close(a);
    x::close(b);
}

#[test]
fn s11_5_c_4() {
    // The command set: each kind of argument reaches the kernel typed.
    use io_compat::FcntlKind;
    assert!(matches!(io_compat::fcntl_kind(0), FcntlKind::Int(_)));
    assert!(matches!(io_compat::fcntl_kind(7), FcntlKind::Lock(_)));
    assert!(matches!(io_compat::fcntl_kind(15), FcntlKind::OwnerEx(true)));
    assert!(matches!(io_compat::fcntl_kind(1035), FcntlKind::Hint(_)));
    assert_eq!(io_compat::fcntl_kind(4242), FcntlKind::Unknown);
}

#[test]
fn s11_5_c_4_1() {
    let (_f, fd) = file("c541", b"");
    assert_eq!(x::liella_fcntl3(fd, 0, 200), 200, "F_DUPFD: the lowest at or above 200");
    let d = x::liella_fcntl3(fd, 1030, 200);
    assert!(d > 200 && fdflags(d) == 1, "F_DUPFD_CLOEXEC");
    for f in [200, d, fd] {
        x::close(f);
    }
}

#[test]
fn s11_5_c_4_2() {
    let (_f, fd) = file("c542", b"");
    assert_eq!(x::liella_fcntl3(fd, 2, 1), 0);
    assert_eq!(x::liella_fcntl3(fd, 1, 0), 1);
    assert_eq!(x::liella_fcntl3(fd, 2, 0), 0);
    assert_eq!(x::liella_fcntl3(fd, 1, 0), 0);
    x::close(fd);
}

#[test]
fn s11_5_c_4_3() {
    let (_f, fd) = file("c543", b"");
    assert_eq!(x::liella_fcntl3(fd, 4, (O_APPEND | O_NONBLOCK) as usize), 0);
    let fl = x::liella_fcntl3(fd, 3, 0);
    assert_eq!(fl & (O_APPEND | O_NONBLOCK), O_APPEND | O_NONBLOCK);
    assert_eq!(fl & 3, O_RDWR, "the access mode cannot change");
    x::close(fd);
}

#[test]
fn s11_5_c_4_4() {
    let (_f, fd) = file("c544", b"0123456789");
    let mut l = liella_kernel::uapi::Flock::new(1, 0, 0, 5);
    assert_eq!(x::liella_fcntl3(fd, 6, &raw mut l as usize), 0, "F_SETLK");
    let mut q = liella_kernel::uapi::Flock::new(1, 0, 0, 5);
    assert_eq!(x::liella_fcntl3(fd, 5, &raw mut q as usize), 0, "F_GETLK");
    assert_eq!(q.ltype, 2, "our own lock: no conflict (F_UNLCK)");
    // OFD locks conflict with the process's own record locks, so they use another range.
    let mut o = liella_kernel::uapi::Flock::new(1, 0, 6, 2);
    assert_eq!(x::liella_fcntl3(fd, 37, &raw mut o as usize), 0, "F_OFD_SETLK");
    let mut og = liella_kernel::uapi::Flock::new(1, 0, 6, 2);
    assert_eq!(x::liella_fcntl3(fd, 36, &raw mut og as usize), 0, "F_OFD_GETLK");
    x::close(fd);
}

#[test]
fn s11_5_c_4_5() {
    let (r, w) = pipe();
    let me = CSys.getpid();
    assert_eq!(x::liella_fcntl3(r, 8, me as usize), 0, "F_SETOWN");
    assert_eq!(x::liella_fcntl3(r, 9, 0), me, "F_GETOWN");
    let mut ex = liella_kernel::uapi::FOwnerEx { otype: 1, pid: me };
    assert_eq!(x::liella_fcntl3(r, 15, &raw mut ex as usize), 0, "F_SETOWN_EX");
    let mut got = liella_kernel::uapi::FOwnerEx::default();
    assert_eq!(x::liella_fcntl3(r, 16, &raw mut got as usize), 0, "F_GETOWN_EX");
    assert_eq!(got.pid, me);
    assert_eq!(x::liella_fcntl3(r, 10, 10), 0, "F_SETSIG");
    assert_eq!(x::liella_fcntl3(r, 11, 0), 10, "F_GETSIG");
    x::close(r);
    x::close(w);
}

#[test]
fn s11_5_c_4_6() {
    let (_f, fd) = file("c546", b"");
    assert_eq!(x::liella_fcntl3(fd, 1025, 0), 2, "F_GETLEASE: F_UNLCK");
    let r = x::liella_fcntl3(fd, 1024, 0);
    assert!(r == 0 || [Errno::EAGAIN.get(), Errno::EACCES.get(), Errno::EINVAL.get()].contains(&errno::get()), "F_SETLEASE (F_RDLCK)");
    // A directory of this test's own: an access to a shared one (/tmp) by any other process
    // would send this process SIGIO, whose default action ends it.
    let dir = tmp("c546-dir");
    std::fs::create_dir(dir.to_str().unwrap()).unwrap();
    let d = x::liella_open3(dir.as_ptr(), O_RDONLY | O_DIRECTORY, 0);
    let n = x::liella_fcntl3(d, 1026, 0x1);
    assert!(n == 0 || errno::get() == Errno::EINVAL.get(), "F_NOTIFY on a directory");
    x::close(fd);
    x::close(d);
    let _ = std::fs::remove_dir(dir.to_str().unwrap());
}

#[test]
fn s11_5_c_4_7() {
    let (r, w) = pipe();
    let size = x::liella_fcntl3(r, 1032, 0);
    assert!(size >= 4096, "F_GETPIPE_SZ");
    assert!(x::liella_fcntl3(r, 1031, 1 << 17) >= 1 << 17, "F_SETPIPE_SZ rounds up");
    x::close(r);
    x::close(w);
}

#[test]
fn s11_5_c_4_8() {
    let fd = x::memfd_create(c"seals".as_ptr(), 2);
    assert_eq!(x::liella_fcntl3(fd, 1034, 0), 0, "F_GET_SEALS: none yet");
    assert_eq!(x::liella_fcntl3(fd, 1033, 0x8), 0, "F_ADD_SEALS: F_SEAL_WRITE");
    errno::set_raw(0);
    assert!(err(x::write(fd, b"x".as_ptr().cast(), 1), Errno::EPERM), "sealed");
    x::close(fd);
}

#[test]
fn s11_5_c_4_9() {
    let (_f, fd) = file("c549", b"");
    let mut hint = 2u64;
    let r = x::liella_fcntl3(fd, 1036, &raw mut hint as usize);
    assert!(r == 0 || errno::get() == Errno::EINVAL.get(), "F_SET_RW_HINT");
    let mut got = 0u64;
    let g = x::liella_fcntl3(fd, 1035, &raw mut got as usize);
    assert!(g == 0 || errno::get() == Errno::EINVAL.get(), "F_GET_RW_HINT");
    x::close(fd);
}

#[test]
fn s11_5_c_5() {
    let mut l = liella_kernel::uapi::Rlimit { cur: 0, max: 0 };
    liella_boundary::posix::s24_system::getrlimit(7, Some(&mut l));
    assert_eq!(liella_boundary::posix::s24_system::getdtablesize() as u64, l.cur.min(i32::MAX as u64), "RLIMIT_NOFILE's soft limit");
}

#[test]
fn s11_6_c_0() {
    let (r, w) = pipe();
    x::write(w, b"four".as_ptr().cast(), 4);
    let mut n: c_int = 0;
    assert_eq!(x::liella_ioctl3(r, 0x541B, &raw mut n as usize), 0);
    assert_eq!(n, 4, "FIONREAD");
    x::close(r);
    x::close(w);
    conformance(&["s11_variadic"]);
}

#[test]
fn s11_7_c_0() {
    let (r, w) = pipe();
    let mut fds = [PollFd { fd: r, events: 1, revents: 0 }];
    assert_eq!(x::poll(fds.as_mut_ptr().cast(), 1, 20), 0, "until a timeout");
    x::close(w);
    assert_eq!(x::poll(fds.as_mut_ptr().cast(), 1, -1), 1, "hang-up makes it ready");
    assert_ne!(fds[0].revents & 0x10, 0, "POLLHUP");
    x::close(r);
}

#[test]
fn s11_7_c_1() {
    let (r, w) = pipe();
    x::write(w, b"x".as_ptr().cast(), 1);
    let (mut rs, mut ws) = (FdSet::default(), FdSet::default());
    rs.set(r);
    rs.set(w);
    ws.set(w);
    let mut tv = Timeval { sec: 1, usec: 0 };
    assert_eq!(x::select(r.max(w) + 1, Some(&mut rs), Some(&mut ws), None, Some(&mut tv)), 2);
    assert!(rs.bits[(r / 64) as usize] >> (r % 64) & 1 == 1 && rs.bits[(w / 64) as usize] >> (w % 64) & 1 == 0, "rewritten");
    assert!(tv.sec < 1 || tv.usec < 1_000_000, "the remaining time");
    x::close(r);
    x::close(w);
}

#[test]
fn s11_7_c_2() {
    // pselect swaps the mask atomically: a pending, blocked signal does not interrupt it, and
    // its timespec is not modified.
    let (r, w) = pipe();
    let mut rs = FdSet::default();
    rs.set(r);
    let ts = Timespec { sec: 0, nsec: 5_000_000 };
    let mut m = liella_abi::layout::sigset_t { __val: [0; 16] };
    m.__val[0] = 1 << 9;
    assert_eq!(x::pselect(r + 1, Some(&mut rs), None, None, Some(&ts), Some(&m)), 0);
    assert_eq!(ts.nsec, 5_000_000);
    x::close(r);
    x::close(w);
}

#[test]
fn s11_7_c_3() {
    let (r, w) = pipe();
    let mut fds = [PollFd { fd: w, events: 4, revents: 0 }];
    assert_eq!(x::poll(fds.as_mut_ptr().cast(), 1, 0), 1);
    assert_eq!(fds[0].revents, 4, "POLLOUT");
    let ts = Timespec { sec: 0, nsec: 0 };
    assert_eq!(x::ppoll(fds.as_mut_ptr().cast(), 1, Some(&ts), None), 1);
    x::close(r);
    x::close(w);
}

#[test]
fn s11_7_c_4() {
    let ep = x::epoll_create1(0);
    let (r, w) = pipe();
    let e = EpollEvent { events: 1, data: 1 };
    assert_eq!(x::epoll_ctl(ep, 1, r, Some(&e)), 0, "kernel-held interest set");
    let mut out = [EpollEvent::default(); 2];
    assert_eq!(x::epoll_wait(ep, out.as_mut_ptr().cast(), 2, 0), 0);
    x::write(w, b"x".as_ptr().cast(), 1);
    assert_eq!(x::epoll_wait(ep, out.as_mut_ptr().cast(), 2, 0), 1);
    for f in [ep, r, w] {
        x::close(f);
    }
}

#[test]
fn s11_7_c_4_1() {
    let ep = x::epoll_create1(0);
    let (r, w) = pipe();
    let et = EpollEvent { events: 1 | (1 << 31), data: 7 };
    assert_eq!(x::epoll_ctl(ep, 1, r, Some(&et)), 0, "EPOLLET");
    x::write(w, b"ab".as_ptr().cast(), 2);
    let mut out = [EpollEvent::default(); 1];
    assert_eq!(x::epoll_wait(ep, out.as_mut_ptr().cast(), 1, 0), 1);
    assert_eq!(x::epoll_wait(ep, out.as_mut_ptr().cast(), 1, 0), 0, "edge-triggered: no new edge");
    let one = EpollEvent { events: 1 | (1 << 30), data: 8 };
    assert_eq!(x::epoll_ctl(ep, 3, r, Some(&one)), 0, "EPOLL_CTL_MOD to EPOLLONESHOT");
    assert_eq!(x::epoll_wait(ep, out.as_mut_ptr().cast(), 1, 0), 1);
    assert_eq!(x::epoll_wait(ep, out.as_mut_ptr().cast(), 1, 0), 0, "one shot");
    assert_eq!(x::epoll_ctl(ep, 2, r, None), 0, "EPOLL_CTL_DEL");
    for f in [ep, r, w] {
        x::close(f);
    }
}

#[test]
fn s11_7_c_4_2() {
    let ep = x::epoll_create1(0);
    let (r, w) = pipe();
    let e = EpollEvent { events: 1, data: u64::MAX - 1 };
    x::epoll_ctl(ep, 1, r, Some(&e));
    x::write(w, b"x".as_ptr().cast(), 1);
    let mut out = [EpollEvent::default(); 1];
    x::epoll_wait(ep, out.as_mut_ptr().cast(), 1, -1);
    let d = out[0].data;
    assert_eq!(d, u64::MAX - 1, "all 64 bits of the cookie");
    for f in [ep, r, w] {
        x::close(f);
    }
}

#[test]
fn s11_7_c_4_3() {
    let ep = x::epoll_create1(0);
    let mut out = [EpollEvent::default(); 1];
    let ts = Timespec { sec: 0, nsec: 2_000_000 };
    if kernel::before(5, 11) {
        kernel::note("epoll_pwait2", "5.11");
        assert_eq!(x::epoll_pwait2(ep, out.as_mut_ptr().cast(), 1, Some(&ts), None), -1, "ENOSYS");
        x::close(ep);
        return;
    }
    assert_eq!(x::epoll_pwait2(ep, out.as_mut_ptr().cast(), 1, Some(&ts), None), 0, "a timespec timeout");
    x::close(ep);
}

#[test]
fn s11_7_c_4_4() {
    errno::set_raw(0);
    assert!(err(x::epoll_create(0) as isize, Errno::EINVAL));
    assert!(err(x::epoll_create(-1) as isize, Errno::EINVAL));
    let ep = x::epoll_create(1_000_000);
    assert!(ep >= 0, "the size is ignored");
    x::close(ep);
}

#[test]
fn s11_7_i_0() {
    assert_eq!(io_compat::fdelt(1023), Some(15));
    assert_eq!(io_compat::fdelt(1024), None, "FD_SETSIZE: the fortify check aborts");
    let mut s = FdSet::default();
    s.set(2000);
    assert_eq!(s, FdSet::default(), "a descriptor beyond the set is not representable");
}

#[test]
fn s11_7_g_0() {
    let (r, w) = pipe();
    let mut fds = [PollFd { fd: -3, events: 1, revents: 9 }, PollFd { fd: w, events: 4, revents: 0 }];
    assert_eq!(x::poll(fds.as_mut_ptr().cast(), 2, 0), 1, "the number of ready descriptors");
    assert_eq!(fds[0].revents, 0, "a negative fd is ignored");
    let mut none = [PollFd { fd: r, events: 1, revents: 0 }];
    assert_eq!(x::poll(none.as_mut_ptr().cast(), 1, 0), 0, "0 on timeout");
    x::close(r);
    x::close(w);
}

#[test]
fn s11_7_f_0() {
    let mut fds = [PollFd { fd: 0, events: 1, revents: 0 }];
    errno::set_raw(0);
    assert!(err(x::poll(fds.as_mut_ptr().cast(), 1 << 30, 0) as isize, Errno::EINVAL), "nfds beyond RLIMIT_NOFILE");
    // A closed descriptor inside the table (the kernel caps nfds at the table's size): a
    // private number, so no concurrently running test is given it meanwhile.
    let (r, w0) = pipe();
    let w = x::dup2(w0, 905);
    x::close(w0);
    x::close(w);
    let mut rs = FdSet::default();
    rs.set(w);
    assert!(err(x::select(w + 1, Some(&mut rs), None, None, Some(&mut Timeval { sec: 0, usec: 0 })) as isize, Errno::EBADF), "not open");
    x::close(r);
}

/// §11.7 Cancellation: Yes.
#[test]
fn s11_7_x_0() {
    conformance(&["s22_cancel_points"]);
}
#[test]
fn s11_8_c_1() {
    let fd = mem_file(c"c81", b"durable");
    assert_eq!(x::fsync(fd), 0);
    assert_eq!(x::fdatasync(fd), 0);
    let (r, w) = pipe();
    errno::set_raw(0);
    assert!(err(x::fsync(r) as isize, Errno::EINVAL), "a pipe: the kernel's EINVAL");
    errno::set_raw(0);
    assert!(err(x::fdatasync(r) as isize, Errno::EINVAL), "a pipe: the kernel's EINVAL");
    for f in [fd, r, w] {
        x::close(f);
    }
}

#[test]
fn s11_8_c_2() {
    if sync_trap::child() {
        x::sync();
        println!("{}", sync_trap::RETURNED);
        return;
    }
    // sync() flushes every file system and waits on Linux: a real call stalls the whole host. The
    // clause's contract (one sync request reaches the kernel, the call returns) is checked under a
    // seccomp trap instead (GATE-FSYNC-2, SYNC-LAST); the kernel flushes nothing.
    sync_trap::check("s11_8_c_2");
    // syncfs() needs only a descriptor on some file system; a memfd's is cheap to flush.
    let fd = mem_file(c"c82", b"x");
    assert_eq!(x::syncfs(fd), 0);
    errno::set_raw(0);
    assert!(err(x::syncfs(-1) as isize, Errno::EBADF), "not open");
    x::close(fd);
}

#[test]
fn s11_8_c_3() {
    let fd = mem_file(c"c83", &[1u8; 8192]);
    assert_eq!(x::sync_file_range(fd, 0, 8192, 1 | 2 | 4), 0, "WAIT_BEFORE | WRITE | WAIT_AFTER");
    x::close(fd);
}

#[test]
fn s11_8_c_4() {
    let (_f, fd) = file("c84", b"x");
    for advice in 0..=5 {
        assert_eq!(x::posix_fadvise(fd, 0, 0, advice), 0, "advice {advice}");
    }
    errno::set_raw(0);
    assert_eq!(x::posix_fadvise(fd, 0, 0, 6), Errno::EINVAL.get(), "the error number, returned");
    assert_eq!(errno::get(), 0);
    x::close(fd);
}

#[test]
fn s11_8_c_5() {
    let (_f, fd) = file("c85", &[0u8; 65536]);
    assert_eq!(x::readahead(fd, 0, 65536), 0);
    x::close(fd);
}

#[test]
fn s11_8_c_6() {
    let (_f, fd) = file("c86", b"ab");
    assert_eq!(x::posix_fallocate(fd, 4096, 4096), 0);
    assert_eq!(x::lseek(fd, 0, 2), 8192, "extended");
    // The emulation (D-11.5): zeros only into holes, existing data untouched, O_APPEND refused.
    let (_f, e) = file("c86e", b"abcdefgh");
    let k = Faulty::new(CSys);
    k.fail("fallocate", Errno::EOPNOTSUPP, 0, 3);
    io_compat::posix_fallocate(&k, Fd::new(e), 2, 100_000).unwrap();
    let mut b = [0u8; 8];
    x::pread(e, b.as_mut_ptr().cast(), 8, 0);
    assert_eq!(&b, b"abcdefgh");
    assert_eq!(x::lseek(e, 0, 2), 100_002);
    x::liella_fcntl3(e, 4, O_APPEND as usize);
    assert_eq!(io_compat::posix_fallocate(&k, Fd::new(e), 0, 10), Err(Errno::EBADF));
    x::close(fd);
    x::close(e);
}

#[test]
fn s11_8_c_7() {
    let (_f, fd) = file("c87", &[7u8; 16384]);
    assert_eq!(x::fallocate(fd, 1, 16384, 4096), 0, "FALLOC_FL_KEEP_SIZE");
    assert_eq!(x::lseek(fd, 0, 2), 16384);
    let r = x::fallocate(fd, 1 | 2, 4096, 4096);
    if r == 0 {
        let mut b = [1u8; 1];
        x::pread(fd, b.as_mut_ptr().cast(), 1, 4096);
        assert_eq!(b[0], 0, "FALLOC_FL_PUNCH_HOLE");
    } else {
        assert_eq!(errno::get(), Errno::EOPNOTSUPP.get());
    }
    x::close(fd);
}

#[test]
fn s11_8_c_8() {
    let (p, fd) = file("c88", b"abc");
    assert_eq!(liella_boundary::posix::s13_filesystem::ftruncate(fd, 6), 0);
    let mut b = [9u8; 6];
    x::pread(fd, b.as_mut_ptr().cast(), 6, 0);
    assert_eq!(&b, b"abc\0\0\0", "extension reads as zeros");
    assert_eq!(liella_boundary::posix::s13_filesystem::truncate(p.as_ptr(), 1), 0);
    assert_eq!(x::lseek(fd, 0, 2), 1);
    x::close(fd);
}

/// Linux 5.10 refuses `sendfile` from a regular file into a pipe (EINVAL; the reference on that
/// kernel too, probed 2026-10-05); later kernels transfer. Whether this kernel refuses it: one
/// byte of `fd` into a pipe of its own.
fn sendfile_to_pipe_refused(fd: i32) -> bool {
    let (r, w) = pipe();
    let n = x::sendfile(w, fd, Some(&mut 0), 1);
    x::close(r);
    x::close(w);
    if n == -1 && kernel::before(5, 12) {
        kernel::note("sendfile into a pipe", "5.11/5.12");
        return true;
    }
    assert_eq!(n, 1, "one byte into a pipe");
    false
}

#[test]
fn s11_9_c_0() {
    let (_f, a) = file("c90", b"in-kernel");
    x::lseek(a, 0, 0);
    let (r, w) = pipe();
    if sendfile_to_pipe_refused(a) {
        return;
    }
    assert_eq!(x::sendfile(w, a, None, 9), 9);
    let mut b = [0u8; 9];
    x::read(r, b.as_mut_ptr().cast(), 9);
    assert_eq!(&b, b"in-kernel");
    for f in [a, r, w] {
        x::close(f);
    }
}

#[test]
fn s11_9_c_1() {
    let (_f, a) = file("c91a", b"0123456789");
    x::lseek(a, 0, 0);
    let (_f, b) = file("c91b", b"");
    assert_eq!(x::sendfile(b, a, None, 10), 10, "out may be a regular file");
    assert_eq!(x::lseek(a, 0, 1), 10, "without an offset variable, the input offset advances");
    x::close(a);
    x::close(b);
}

#[test]
fn s11_9_c_2() {
    let (_f, a) = file("c92a", &[5u8; 100_000]);
    x::lseek(a, 0, 0);
    let (_f, b) = file("c92b", b"");
    let mut total = 0;
    while total < 100_000 {
        let n = x::copy_file_range(a, None, b, None, 100_000 - total, 0);
        assert!(n > 0);
        total += n as usize;
    }
    assert_eq!(x::lseek(b, 0, 2), 100_000);
    x::close(a);
    x::close(b);
}

#[test]
fn s11_9_c_3() {
    let (r, w) = pipe();
    let (r2, w2) = pipe();
    let mut data = *b"vm";
    let mut v = [IoVecMut::new(&mut data)];
    assert_eq!(x::vmsplice(w, v.as_mut_ptr().cast(), 1, 0), 2);
    assert_eq!(x::tee(r, w2, 2, 0), 2);
    assert_eq!(x::splice(r, None, w2, None, 2, 0), 2);
    let mut b = [0u8; 4];
    assert_eq!(x::read(r2, b.as_mut_ptr().cast(), 4), 4);
    assert_eq!(&b, b"vmvm");
    for f in [r, w, r2, w2] {
        x::close(f);
    }
}

#[test]
fn s11_9_g_0() {
    let (_f, a) = file("g90", b"short");
    let (r, w) = pipe();
    if sendfile_to_pipe_refused(a) {
        return;
    }
    assert_eq!(x::sendfile(w, a, Some(&mut 0), 1000), 5, "short: the file's end");
    for f in [a, r, w] {
        x::close(f);
    }
}

#[test]
fn s11_9_f_0() {
    let (_f, a) = file("f90", b"x");
    errno::set_raw(0);
    assert!(err(x::splice(a, None, a, None, 1, 0), Errno::EINVAL), "neither end a pipe");
    let (r, w) = pipe();
    assert!(err(x::copy_file_range(r, None, a, None, 1, 0), Errno::EINVAL), "not regular files: no emulation");
    for f in [a, r, w] {
        x::close(f);
    }
}

#[test]
fn s11_10_c_1() {
    let (_f, fd) = file("c101", b"shared");
    let sh = x::mmap(null_mut(), 4096, 3, 1, fd, 0);
    liella_boundary::posix::s04_strings::memcpy(sh, b"SH".as_ptr().cast(), 2);
    let mut b = [0u8; 2];
    x::pread(fd, b.as_mut_ptr().cast(), 2, 0);
    assert_eq!(&b, b"SH", "MAP_SHARED writes reach the file");
    let pr = x::mmap(null_mut(), 4096, 3, 2, fd, 0);
    liella_boundary::posix::s04_strings::memcpy(pr, b"pr".as_ptr().cast(), 2);
    x::pread(fd, b.as_mut_ptr().cast(), 2, 0);
    assert_eq!(&b, b"SH", "MAP_PRIVATE writes do not");
    let v = x::mmap(null_mut(), 4096, 1, 3, fd, 0);
    assert_ne!(v as isize, -1, "MAP_SHARED_VALIDATE");
    for (p, n) in [(sh, 4096), (pr, 4096), (v, 4096)] {
        x::munmap(p, n);
    }
    x::close(fd);
}

#[test]
fn s11_10_c_1_1() {
    let p = x::mmap(null_mut(), 8192, 3, 0x22, -1, 0);
    let f = x::mmap(p, 4096, 3, 0x22 | 0x10, -1, 0);
    assert_eq!(f, p, "MAP_FIXED at the address");
    errno::set_raw(0);
    let n = x::mmap(p, 4096, 3, 0x22 | 0x100000, -1, 0);
    assert_eq!(n as isize, -1);
    assert_eq!(errno::get(), Errno::EEXIST.get(), "MAP_FIXED_NOREPLACE over a mapping");
    x::munmap(p, 8192);
}

#[test]
fn s11_10_c_1_2() {
    let p = x::mmap(null_mut(), 8192, 3, 0x22 | 0x8000 | 0x4000, -1, 0);
    assert_ne!(p as isize, -1, "MAP_POPULATE | MAP_NORESERVE");
    let mut v = [0u8; 2];
    x::mincore(p, 8192, v.as_mut_ptr());
    assert_eq!(v, [1, 1], "populated: resident");
    x::munmap(p, 8192);
}

#[test]
fn s11_10_c_1_3() {
    let p = x::mmap(null_mut(), 2 << 20, 3, 0x22 | 0x40000 | (21 << 26), -1, 0);
    if p as isize == -1 {
        assert!([Errno::ENOMEM.get(), Errno::EINVAL.get()].contains(&errno::get()), "no huge pages reserved");
    } else {
        x::munmap(p, 2 << 20);
    }
}

#[test]
fn s11_10_c_1_4() {
    let p = x::mmap(null_mut(), 65536, 3, 0x22 | 0x20000, -1, 0);
    assert_ne!(p as isize, -1, "MAP_STACK");
    x::munmap(p, 65536);
    let g = x::mmap(null_mut(), 65536, 3, 0x22 | 0x100, -1, 0);
    assert_ne!(g as isize, -1, "MAP_GROWSDOWN");
    x::munmap(g, 65536);
}

#[test]
fn s11_10_c_1_5() {
    let (_f, fd) = file("c1015", &[0u8; 4096]);
    errno::set_raw(0);
    let p = x::mmap(null_mut(), 4096, 3, 3 | 0x80000, fd, 0);
    assert_eq!(p as isize, -1, "MAP_SYNC needs DAX storage");
    assert_eq!(errno::get(), Errno::EOPNOTSUPP.get());
    errno::set_raw(0);
    assert_eq!(x::mmap(null_mut(), 0, 3, 0x22, -1, 0) as isize, -1, "MAP_FAILED is -1, not NULL");
    x::close(fd);
}

#[test]
fn s11_10_c_2() {
    let p = x::mmap(null_mut(), 12288, 3, 0x22, -1, 0);
    assert_eq!(x::munmap(p.cast::<u8>().wrapping_add(4096).cast(), 4096), 0, "a range inside a mapping");
    assert_eq!(x::mprotect(p, 4096, 1), 0);
    let r = x::pkey_mprotect(p, 4096, 3, 0);
    assert!(r == 0 || [Errno::EINVAL.get(), Errno::ENOSYS.get()].contains(&errno::get()));
    // Only the two pages still owned: a concurrent test's mapping may have landed in the hole.
    x::munmap(p, 4096);
    x::munmap(p.cast::<u8>().wrapping_add(8192).cast(), 4096);
}

#[test]
fn s11_10_c_3() {
    let k = x::pkey_alloc(0, 0);
    if k < 0 {
        // No protection keys: pkey_get/pkey_set report it rather than fault.
        assert_eq!(x::pkey_get(1), -1);
        return;
    }
    assert_eq!(x::pkey_set(k, 1), 0, "PKEY_DISABLE_ACCESS");
    assert_eq!(x::pkey_get(k), 1, "the per-thread register, no system call");
    assert_eq!(x::pkey_set(k, 0), 0);
    assert_eq!(x::pkey_free(k), 0);
}

#[test]
fn s11_10_c_4() {
    let p = x::mmap(null_mut(), 4096, 3, 0x22, -1, 0);
    liella_boundary::posix::s04_strings::memset(p, 9, 4096);
    assert_eq!(x::madvise(p, 4096, 4), 0, "MADV_DONTNEED (errno {})", errno::get());
    let mut b = [1u8; 1];
    liella_boundary::posix::s04_strings::memcpy(b.as_mut_ptr().cast(), p, 1);
    assert_eq!(b[0], 0, "MADV_DONTNEED discards");
    assert_eq!(x::madvise(p, 4096, 0), 0, "MADV_NORMAL");
    x::munmap(p, 4096);
}

#[test]
fn s11_10_c_5() {
    let p = x::mmap(null_mut(), 4096, 3, 0x22, -1, 0);
    liella_boundary::posix::s04_strings::memset(p, 9, 4096);
    assert_eq!(x::posix_madvise(p, 4096, 4), 0);
    let mut b = [0u8; 1];
    liella_boundary::posix::s04_strings::memcpy(b.as_mut_ptr().cast(), p, 1);
    assert_eq!(b[0], 9, "POSIX_MADV_DONTNEED never discards");
    errno::set_raw(0);
    assert_eq!(x::posix_madvise(p, 4096, 99), Errno::EINVAL.get(), "returned, not in errno");
    assert_eq!(errno::get(), 0);
    x::munmap(p, 4096);
}

#[test]
fn s11_10_c_6() {
    let pidfd = CSys.pidfd_open(CSys.getpid(), 0).unwrap();
    errno::set_raw(0);
    assert_eq!(x::process_mrelease(pidfd.raw(), 0), -1, "reaping a live process is refused");
    let p = x::mmap(null_mut(), 4096, 3, 0x22, -1, 0);
    let v = [liella_kernel::mem::RemoteIoVec { base: p as usize, len: 4096 }];
    let r = x::process_madvise(pidfd.raw(), v.as_ptr().cast(), 1, 21, 0);
    assert!(r == 4096 || [Errno::EPERM.get(), Errno::EINVAL.get(), Errno::ENOSYS.get()].contains(&errno::get()), "MADV_PAGEOUT through a pidfd");
    x::munmap(p, 4096);
    x::close(pidfd.raw());
}

#[test]
fn s11_10_c_7() {
    let p = x::mmap(null_mut(), 4096, 3, 0x22, -1, 0);
    let s = x::liella_mremap5(p, 4096, 2048, 0, null_mut());
    assert_eq!(s, p, "shrink in place");
    let g = x::liella_mremap5(s, 2048, 1 << 20, 1, null_mut());
    assert_ne!(g as isize, -1, "grow, moving if needed");
    x::munmap(g, 1 << 20);
    conformance(&["s11_variadic"]);
}

#[test]
fn s11_10_c_8() {
    let (_f, fd) = file("c108", &[0u8; 8192]);
    let m = x::mmap(null_mut(), 8192, 3, 1, fd, 0);
    let r = x::remap_file_pages(m, 4096, 0, 1, 0);
    assert!(r == 0 || [Errno::EINVAL.get(), Errno::EPERM.get()].contains(&errno::get()));
    x::munmap(m, 8192);
    x::close(fd);
}

#[test]
fn s11_10_c_9() {
    let fd = mem_file(c"c109", &[0u8; 4096]);
    let m = x::mmap(null_mut(), 4096, 3, 1, fd, 0);
    liella_boundary::posix::s04_strings::memset(m, 1, 10);
    assert_eq!(x::msync(m, 4096, 4), 0, "MS_SYNC");
    assert_eq!(x::msync(m, 4096, 1), 0, "MS_ASYNC");
    assert_eq!(x::msync(m, 4096, 2), 0, "MS_INVALIDATE");
    x::munmap(m, 4096);
    x::close(fd);
}

#[test]
fn s11_10_c_10() {
    // Between PROT_NONE guard pages. An adjacent read-write anonymous mapping (a new thread's
    // stack) would otherwise merge with this one, and with transparent huge pages "always" the
    // kernel can back the merged range with a huge page whose 2 MiB span takes in this
    // mapping's untouched pages, which mincore then rightly reports resident.
    let g = x::mmap(null_mut(), 6 * 4096, 0, 0x22, -1, 0);
    let p = g.cast::<u8>().wrapping_add(4096).cast();
    assert_eq!(x::mprotect(p, 4 * 4096, 3), 0);
    liella_boundary::posix::s04_strings::memset(p.cast::<u8>().wrapping_add(2 * 4096).cast(), 1, 1);
    let mut v = [9u8; 4];
    assert_eq!(x::mincore(p, 4 * 4096, v.as_mut_ptr()), 0);
    assert_eq!(v.map(|b| b & 1), [0, 0, 1, 0], "one byte per page");
    x::munmap(g, 6 * 4096);
}

#[test]
fn s11_10_c_11() {
    let p = x::mmap(null_mut(), 4096, 3, 0x22, -1, 0);
    let r = x::mlock(p, 4096);
    assert!(r == 0 || [Errno::EPERM.get(), Errno::ENOMEM.get(), Errno::EAGAIN.get()].contains(&errno::get()));
    assert_eq!(x::munlock(p, 4096), 0);
    let r2 = x::mlock2(p, 4096, 1);
    assert!(r2 == 0 || [Errno::EPERM.get(), Errno::ENOMEM.get(), Errno::EAGAIN.get()].contains(&errno::get()), "MLOCK_ONFAULT");
    x::munlock(p, 4096);
    x::munmap(p, 4096);
}

#[test]
fn s11_10_c_12() {
    let fd = x::memfd_create(c"sealable".as_ptr(), 2);
    assert!(fd >= 0, "MFD_ALLOW_SEALING");
    assert_eq!(x::liella_fcntl3(fd, 1033, 1), 0, "F_SEAL_SEAL accepted");
    x::close(fd);
    let h = x::memfd_create(c"huge".as_ptr(), 4);
    if h >= 0 {
        x::close(h);
    }
}

#[test]
fn s11_10_c_13() {
    let page = liella_boundary::process::auxval(6).unwrap();
    assert_eq!(liella_boundary::posix::s24_system::getpagesize() as u64, page, "AT_PAGESZ");
    assert_eq!(liella_boundary::posix::s24_system::sysconf(30) as u64, page, "the POSIX form agrees");
}

#[test]
fn s11_10_c_14() {
    let src = *b"between";
    let mut dst = [0u8; 7];
    let mut l = [IoVecMut::new(&mut dst)];
    let r = [liella_kernel::mem::RemoteIoVec { base: src.as_ptr() as usize, len: 7 }];
    let n = x::process_vm_readv(CSys.getpid(), l.as_mut_ptr().cast(), 1, r.as_ptr().cast(), 1, 0);
    if n == -1 {
        assert_eq!(errno::get(), Errno::EPERM.get(), "ptrace permission");
        return;
    }
    assert_eq!((n, &dst), (7, b"between"));
}

#[test]
fn s11_10_i_0() {
    let p = x::mmap(null_mut(), 8192, 3, 0x22, -1, 0);
    errno::set_raw(0);
    assert!(err(x::mprotect(p.cast::<u8>().wrapping_add(1).cast(), 10, 1) as isize, Errno::EINVAL), "unaligned address");
    assert_eq!(x::mprotect(p, 10, 1), 0, "a length rounds up to whole pages");
    let (_f, fd) = file("i100", &[0u8; 8192]);
    assert_eq!(x::mmap(null_mut(), 4096, 1, 2, fd, 100) as isize, -1, "unaligned offset");
    x::munmap(p, 8192);
    x::close(fd);
}

#[test]
fn s11_10_f_0() {
    errno::set_raw(0);
    assert_eq!(x::mmap(null_mut(), 1 << 62, 3, 0x22, -1, 0) as isize, -1);
    assert_eq!(errno::get(), Errno::ENOMEM.get());
    let d = x::liella_open3(c"/tmp".as_ptr(), O_RDONLY | O_DIRECTORY, 0);
    assert_eq!(x::mmap(null_mut(), 4096, 1, 2, d, 0) as isize, -1);
    assert_eq!(errno::get(), Errno::ENODEV.get(), "not mappable");
    x::close(d);
}

// ------------------------------------------------------------------------------ §11.11 asynchronous I/O
//
// Every AIO clause runs twice, each time in this test binary re-executed alone: on the io_uring
// engine and on the worker engine (`selftest::aio_force_workers`, D-11.8), since the engine is
// chosen once per process. A machine without io_uring runs the workers both times and says so.
// No test returns while a request it made may still write into its buffers.

/// A `struct aiocb` (168 bytes, Appendix C) with the fields the tests set. The library writes
/// its status words concurrently, so a submitted block is reached only through a raw pointer.
#[repr(C, align(8))]
#[derive(Clone, Copy, Default)]
struct Cb {
    fildes: c_int,
    opcode: c_int,
    reqprio: c_int,
    _p0: c_int,
    buf: usize,
    nbytes: usize,
    value: u64,
    signo: c_int,
    notify: c_int,
    function: usize,
    attrs: usize,
    _sev: [u64; 4],
    _next: usize,
    _abs: [c_int; 2],
    error: c_int,
    _p1: c_int,
    ret: isize,
    offset: i64,
    _res: [u64; 4],
}
const _: () = assert!(size_of::<Cb>() == 168);

const EINPROGRESS: c_int = 115;
const ECANCELED: c_int = 125;
const SIGEV_SIGNAL: c_int = 0;
const SIGEV_NONE: c_int = 1;
const SIGEV_THREAD: c_int = 2;
const O_SYNC: c_int = 0o4010000;
const O_DSYNC: c_int = 0o10000;
const LIO_READ: c_int = 0;
const LIO_WRITE: c_int = 1;
const LIO_NOP: c_int = 2;
const LIO_WAIT: c_int = 0;
const LIO_NOWAIT: c_int = 1;
const AIO_CANCELED: c_int = 0;
const AIO_NOTCANCELED: c_int = 1;
const AIO_ALLDONE: c_int = 2;
const AIO_ENGINE: &str = "LIELLA_TEST_AIO";

impl Cb {
    fn new(fd: c_int, buf: *const u8, n: usize, off: i64) -> Cb {
        Cb { fildes: fd, buf: buf.addr(), nbytes: n, offset: off, notify: SIGEV_NONE, ..Cb::default() }
    }
}

fn aio_err(p: *mut Cb) -> c_int {
    x::aio_error(p.cast())
}

fn aio_ret(p: *mut Cb) -> isize {
    x::aio_return(p.cast())
}

/// Waits (at most 10 s) until the request is no longer in progress.
/// Whether the request completes. The bound only ends a wait that would otherwise hang: on a
/// loaded machine with a slow disk an fsync can take well over 10 s (seen on the timing VM).
fn settled(p: *mut Cb) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < std::time::Duration::from_secs(300) {
        if aio_err(p) != EINPROGRESS {
            return true;
        }
        ms(1);
    }
    aio_err(p) != EINPROGRESS
}

fn ms(n: u64) {
    std::thread::sleep(std::time::Duration::from_millis(n));
}

fn put(fd: c_int, b: &[u8]) {
    assert_eq!(x::write(fd, b.as_ptr().cast(), b.len()), b.len() as isize);
}

fn list(cbs: &[*mut Cb]) -> Vec<*mut core::ffi::c_void> {
    cbs.iter().map(|p| p.cast()).collect()
}

/// Runs `body` in this test re-executed on each engine.
fn on_both_engines(test: &str, body: fn()) {
    if let Ok(engine) = std::env::var(AIO_ENGINE) {
        liella_boundary::selftest::aio_force_workers(engine == "workers");
        body();
        let used = liella_boundary::selftest::aio_engine();
        if engine == "workers" {
            assert_eq!(used, "workers");
        } else if used != "ring" {
            eprintln!("note: io_uring is not available here; the ring run used the {used} engine");
        }
        return;
    }
    for engine in ["ring", "workers"] {
        let mut cmd = std::process::Command::new(std::env::current_exe().unwrap());
        cmd.args([test, "--exact", "--test-threads=1", "--nocapture"]).env(AIO_ENGINE, engine);
        let _scratch = child_scratch(&mut cmd);
        let o = cmd.output().unwrap();
        assert!(o.status.success(), "{test} on the {engine} engine:\n{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    }
}

static NOTE_CALLS: AtomicU32 = AtomicU32::new(0);
static NOTE_VALUE: AtomicU32 = AtomicU32::new(0);
static NOTE_TID: AtomicU32 = AtomicU32::new(0);

/// A `SIGEV_THREAD` function: `void f(union sigval)`, the value in the first register.
extern "C" fn note(v: u64) {
    NOTE_VALUE.store(v as u32, Ordering::SeqCst);
    NOTE_TID.store(CSys.gettid() as u32, Ordering::SeqCst);
    NOTE_CALLS.fetch_add(1, Ordering::SeqCst);
}

fn note_addr() -> usize {
    note as extern "C" fn(u64) as usize
}

fn calls_reach(n: u32) -> bool {
    for _ in 0..5000 {
        if NOTE_CALLS.load(Ordering::SeqCst) >= n {
            return true;
        }
        ms(1);
    }
    false
}

#[test]
fn s11_11_c_0() {
    on_both_engines("s11_11_c_0", || {
        // The engine in use decides the expectation below, not the variable that asked for one: where
        // io_uring is unavailable or refused (a 5.10 kernel with a small memlock) the ring run falls
        // back to the workers. It is read after the first request, which is what chooses it (below).
        // aio_init tunes the worker pool: with one worker, a request on another descriptor waits
        // behind a blocked one; the ring has no pool and is not held up.
        let init = liella_abi::layout::struct_aioinit { aio_threads: 1, aio_num: 1, aio_locks: 0, aio_usedba: 0, aio_debug: 0, aio_numusers: 0, aio_idle_time: 0, aio_reserved: 0 };
        x::aio_init(Some(&init));
        let (r, w) = pipe();
        let mut pb = [0u8; 8];
        let mut held = Cb::new(r, pb.as_mut_ptr(), 4, 0);
        let hp = &raw mut held;
        assert_eq!(x::aio_read(hp.cast()), 0);
        let workers = liella_boundary::selftest::aio_engine() == "workers";
        if std::env::var(AIO_ENGINE).is_ok_and(|e| e == "workers") {
            assert!(workers, "the workers run uses the worker engine");
        }
        ms(100);
        let z = x::liella_open3(c"/dev/zero".as_ptr(), O_RDONLY, 0);
        let mut zb = [1u8; 8];
        let mut zc = Cb::new(z, zb.as_mut_ptr(), 8, 0);
        let zp = &raw mut zc;
        assert_eq!(x::aio_read(zp.cast()), 0);
        ms(300);
        assert_eq!(aio_err(zp) == EINPROGRESS, workers, "aio_threads=1 holds the second request only on the worker engine");
        put(w, b"feed");
        assert!(settled(hp) && settled(zp));
        assert_eq!((aio_ret(hp), aio_ret(zp)), (4, 8));
        assert_eq!(zb, [0; 8]);
        let init = liella_abi::layout::struct_aioinit { aio_threads: 64, ..init };
        x::aio_init(Some(&init));
        x::aio_init(None);

        // A read proceeds concurrently; polling aio_error sees the end; aio_return the count.
        let content: Vec<u8> = (0..200u8).collect();
        // A memfd (GATE-FSYNC-2): the aio_fsync below on a file under target/ waited for the
        // journal and a parallel build's dirty pages (the test took up to 72 s under gate load). What this clause
        // checks - queue order, results, notification - is the same on any regular file.
        let fd = mem_file(c"aio-c", &content);
        let mut buf = [0u8; 64];
        let mut cb = Cb::new(fd, buf.as_mut_ptr(), 50, 100);
        let p = &raw mut cb;
        assert_eq!(x::aio_read(p.cast()), 0);
        assert!(settled(p));
        assert_eq!(aio_err(p), 0);
        assert_eq!(aio_ret(p), 50);
        assert_eq!(&buf[..50], &content[100..150]);
        assert_eq!(buf[50], 0, "nothing beyond the count");
        assert_eq!(aio_ret(p), 50, "the final status can be read again (the reference: no EINVAL)");

        // In progress: a read from an empty pipe. aio_return then is -1 without errno.
        let mut cb = Cb::new(r, pb.as_mut_ptr(), 4, 0);
        let p = &raw mut cb;
        assert_eq!(x::aio_read(p.cast()), 0);
        ms(50);
        assert_eq!(aio_err(p), EINPROGRESS);
        errno::set_raw(0);
        assert_eq!(aio_ret(p), -1);
        assert_eq!(errno::get(), 0);
        // Waiting with a timeout: it passes (EAGAIN); without one, a completion ends the wait.
        let l = list(&[p]);
        let t = Timespec { sec: 0, nsec: 50_000_000 };
        let t0 = std::time::Instant::now();
        assert!(err(x::aio_suspend(l.as_ptr().cast(), 1, Some(&t)) as isize, Errno::EAGAIN));
        assert!(t0.elapsed() >= std::time::Duration::from_millis(45));
        let feeder = std::thread::spawn(move || {
            ms(100);
            put(w, b"abcd");
        });
        assert_eq!(x::aio_suspend(l.as_ptr().cast(), 1, None), 0);
        feeder.join().unwrap();
        assert_eq!((aio_err(p), aio_ret(p)), (0, 4));
        assert_eq!(&pb[..4], b"abcd");

        // A write, then a sync covering it.
        let mut cb = Cb::new(fd, b"XYZ".as_ptr(), 3, 10);
        let p = &raw mut cb;
        assert_eq!(x::aio_write(p.cast()), 0);
        let mut sc = Cb::new(fd, core::ptr::null(), 0, 0);
        let sp = &raw mut sc;
        assert_eq!(x::aio_fsync(O_DSYNC, sp.cast()), 0);
        assert!(settled(sp));
        assert_eq!(aio_err(p), 0, "the sync completes after the write queued before it");
        assert_eq!((aio_ret(p), aio_ret(sp)), (3, 0));
        let mut back = [0u8; 3];
        assert_eq!(x::pread(fd, back.as_mut_ptr().cast(), 3, 10), 3);
        assert_eq!(&back, b"XYZ");
        assert_eq!(x::aio_fsync(O_SYNC, sp.cast()), 0);
        assert!(settled(sp) && aio_err(sp) == 0);
        // A write to a pipe is a blocking write: all of it, as the reader drains the pipe.
        let (r3, w3) = pipe();
        let big = vec![7u8; 100_000];
        let mut wc = Cb::new(w3, big.as_ptr(), big.len(), 0);
        let wp = &raw mut wc;
        assert_eq!(x::aio_write(wp.cast()), 0);
        let (mut got, mut chunk) = (0usize, [0u8; 8192]);
        while got < big.len() {
            let n = x::read(r3, chunk.as_mut_ptr().cast(), chunk.len());
            assert!(n > 0);
            got += n as usize;
        }
        assert!(settled(wp));
        assert_eq!(aio_ret(wp), 100_000);
        x::close(r3);
        x::close(w3);

        // Notification by a signal: exactly one, after the request completed.
        let sig = 40;
        let (ok, count) = liella_boundary::selftest::with_counting_handler(sig, || {
            let mut cb = Cb::new(fd, buf.as_mut_ptr(), 5, 0);
            cb.notify = SIGEV_SIGNAL;
            cb.signo = sig;
            cb.value = 7;
            let p = &raw mut cb;
            assert_eq!(x::aio_read(p.cast()), 0);
            assert!(settled(p));
            ms(200);
            aio_ret(p) == 5
        })
        .unwrap();
        assert!(ok);
        assert_eq!(count, 1, "one SIGEV_SIGNAL");
        // Notification by a thread: the function with the value, once, on another thread.
        NOTE_CALLS.store(0, Ordering::SeqCst);
        let mut cb = Cb::new(fd, buf.as_mut_ptr(), 5, 0);
        cb.notify = SIGEV_THREAD;
        cb.function = note_addr();
        cb.value = 77;
        let p = &raw mut cb;
        assert_eq!(x::aio_read(p.cast()), 0);
        assert!(calls_reach(1));
        ms(100);
        assert_eq!(NOTE_CALLS.load(Ordering::SeqCst), 1);
        assert_eq!(NOTE_VALUE.load(Ordering::SeqCst), 77);
        assert_ne!(NOTE_TID.load(Ordering::SeqCst), CSys.gettid() as u32);
        assert_eq!((aio_err(p), aio_ret(p)), (0, 5));

        // lio_listio: LIO_WAIT returns with every entry done; LIO_NOWAIT notifies the list once.
        let mut a = Cb::new(fd, b"lio".as_ptr(), 3, 20);
        a.opcode = LIO_WRITE;
        let mut b2 = [0u8; 8];
        let mut b = Cb::new(fd, b2.as_mut_ptr(), 8, 0);
        b.opcode = LIO_READ;
        let mut n = Cb::new(-1, core::ptr::null(), 0, 0);
        n.opcode = LIO_NOP;
        let (pa, pbb, pn) = (&raw mut a, &raw mut b, &raw mut n);
        let l = list(&[pa, core::ptr::null_mut(), pbb, pn]);
        assert_eq!(x::lio_listio(LIO_WAIT, l.as_ptr().cast(), 4, None), 0);
        assert_eq!((aio_err(pa), aio_ret(pa), aio_err(pbb), aio_ret(pbb)), (0, 3, 0, 8));
        NOTE_CALLS.store(0, Ordering::SeqCst);
        let ev = liella_kernel::uapi::SigEvent::thread(99, note_addr() as u64, 0);
        assert_eq!(x::lio_listio(LIO_NOWAIT, l.as_ptr().cast(), 4, Some(&ev)), 0);
        assert!(calls_reach(1));
        ms(100);
        assert_eq!(NOTE_CALLS.load(Ordering::SeqCst), 1, "one notification for the list");
        assert_eq!(NOTE_VALUE.load(Ordering::SeqCst), 99);
        assert!(aio_err(pa) == 0 && aio_err(pbb) == 0, "every entry is done at the notification");

        // aio_cancel: a queued request is cancelled; the one already running is not.
        let (r2, w2) = pipe();
        let (mut x1, mut x2) = ([0u8; 4], [0u8; 4]);
        let mut first = Cb::new(r2, x1.as_mut_ptr(), 2, 0);
        let mut second = Cb::new(r2, x2.as_mut_ptr(), 2, 0);
        let (p1, p2) = (&raw mut first, &raw mut second);
        assert_eq!(x::aio_read(p1.cast()), 0);
        ms(100);
        assert_eq!(x::aio_read(p2.cast()), 0);
        assert_eq!(x::aio_cancel(r2, p2.cast()), AIO_CANCELED);
        assert_eq!((aio_err(p2), aio_ret(p2)), (ECANCELED, -1));
        assert_eq!(x::aio_cancel(r2, p2.cast()), AIO_ALLDONE);
        assert_eq!(x::aio_cancel(r2, p1.cast()), AIO_NOTCANCELED);
        assert_eq!(x::aio_cancel(r2, core::ptr::null_mut()), AIO_NOTCANCELED);
        put(w2, b"ok");
        assert!(settled(p1));
        assert_eq!(aio_ret(p1), 2);
        assert_eq!(x::aio_cancel(r2, core::ptr::null_mut()), AIO_ALLDONE);
        for d in [r, w, r2, w2, z, fd] {
            x::close(d);
        }
    });
}

#[test]
fn s11_11_g_0() {
    on_both_engines("s11_11_g_0", || {
        // Writes to one offset: the last submitted is the byte left; overlapping writes of 8, 6,
        // 4 and 2 bytes leave each in turn.
        let (path, fd) = file("aio-g", b"");
        let bytes = *b"abcdefgh";
        for _ in 0..30 {
            let mut cbs: Vec<Cb> = (0..8).map(|k| Cb::new(fd, bytes[k..].as_ptr(), 1, 0)).collect();
            let ps: Vec<*mut Cb> = cbs.iter_mut().map(|c| c as *mut Cb).collect();
            for &p in &ps {
                assert_eq!(x::aio_write(p.cast()), 0);
            }
            assert!(ps.iter().all(|&p| settled(p)));
            let mut b = [0u8; 1];
            assert_eq!(x::pread(fd, b.as_mut_ptr().cast(), 1, 0), 1);
            assert_eq!(b[0], b'h', "the last request on the descriptor is the last to complete");
            let data = [b"AAAAAAAA".as_ptr(), b"BBBBBB".as_ptr(), b"CCCC".as_ptr(), b"DD".as_ptr()];
            let mut cbs: Vec<Cb> = data.iter().zip([8, 6, 4, 2]).map(|(&d, n)| Cb::new(fd, d, n, 0)).collect();
            let ps: Vec<*mut Cb> = cbs.iter_mut().map(|c| c as *mut Cb).collect();
            for &p in &ps {
                assert_eq!(x::aio_write(p.cast()), 0);
            }
            assert!(ps.iter().all(|&p| settled(p)));
            let mut b = [0u8; 8];
            assert_eq!(x::pread(fd, b.as_mut_ptr().cast(), 8, 0), 8);
            assert_eq!(&b, b"DDCCBBAA");
        }
        // Reads queued on an empty pipe receive the bytes in submission order.
        let (r, w) = pipe();
        let mut got = [0u8; 4];
        let mut cbs: Vec<Cb> = (0..4).map(|k| Cb::new(r, got[k..].as_mut_ptr(), 1, 0)).collect();
        let ps: Vec<*mut Cb> = cbs.iter_mut().map(|c| c as *mut Cb).collect();
        for &p in &ps {
            assert_eq!(x::aio_read(p.cast()), 0);
        }
        for (k, &p) in ps.iter().enumerate() {
            put(w, &[b'1' + k as u8]);
            assert!(settled(p));
        }
        assert_eq!(&got, b"1234");
        // Priorities: among queued requests, a lower aio_reqprio runs first, then the older.
        let mut got = [0u8; 4];
        let mut cbs: Vec<Cb> = (0..4).map(|k| Cb::new(r, got[k..].as_mut_ptr(), 1, 0)).collect();
        for (c, prio) in cbs.iter_mut().zip([0, 5, 0, 2]) {
            c.reqprio = prio;
        }
        let ps: Vec<*mut Cb> = cbs.iter_mut().map(|c| c as *mut Cb).collect();
        assert_eq!(x::aio_read(ps[0].cast()), 0);
        ms(100);
        for &p in &ps[1..] {
            assert_eq!(x::aio_read(p.cast()), 0);
        }
        for k in 0..4u8 {
            put(w, &[b'1' + k]);
            for _ in 0..5000 {
                if ps.iter().filter(|&&p| aio_err(p) != EINPROGRESS).count() > usize::from(k) {
                    break;
                }
                ms(1);
            }
        }
        assert!(ps.iter().all(|&p| settled(p)));
        assert_eq!(got, [b'1', b'4', b'2', b'3'], "priority 0 (held), then 0, 2, 5");
        // aio_fsync is a barrier: it waits for the read queued before it, and a request queued
        // after it waits for the sync.
        let mut rb = [0u8; 2];
        let mut before = Cb::new(r, rb.as_mut_ptr(), 2, 0);
        let mut sync = Cb::new(r, core::ptr::null(), 0, 0);
        let mut after = Cb::new(r, rb[1..].as_mut_ptr(), 1, 0);
        let (pb, ps2, pa) = (&raw mut before, &raw mut sync, &raw mut after);
        assert_eq!(x::aio_read(pb.cast()), 0);
        assert_eq!(x::aio_fsync(O_SYNC, ps2.cast()), 0);
        assert_eq!(x::aio_read(pa.cast()), 0);
        ms(200);
        assert_eq!((aio_err(pb), aio_err(ps2), aio_err(pa)), (EINPROGRESS, EINPROGRESS, EINPROGRESS));
        put(w, b"xy");
        assert!(settled(pb) && settled(ps2));
        assert_eq!(aio_err(ps2), 22, "a pipe cannot be synced (EINVAL), after the read before it");
        assert_eq!(aio_err(pa), EINPROGRESS, "the read after the sync waits");
        put(w, b"z");
        assert!(settled(pa));
        assert_eq!(aio_ret(pa), 1);
        // A request on another descriptor is not held up by a blocked one.
        let mut hb = [0u8; 1];
        let mut held = Cb::new(r, hb.as_mut_ptr(), 1, 0);
        let hp = &raw mut held;
        assert_eq!(x::aio_read(hp.cast()), 0);
        let z = x::liella_open3(c"/dev/zero".as_ptr(), O_RDONLY, 0);
        let mut zb = [9u8; 4];
        let mut zc = Cb::new(z, zb.as_mut_ptr(), 4, 0);
        let zp = &raw mut zc;
        assert_eq!(x::aio_read(zp.cast()), 0);
        assert!(settled(zp));
        assert_eq!(aio_err(hp), EINPROGRESS);
        put(w, b"!");
        assert!(settled(hp));
        assert_eq!(zb, [0; 4]);
        for d in [r, w, z, fd] {
            x::close(d);
        }
        let _ = std::fs::remove_file(std::str::from_utf8(path.as_bytes()).unwrap());
    });
}

#[test]
fn s11_11_f_0() {
    on_both_engines("s11_11_f_0", || {
        // EAGAIN: no engine can start (no thread may be created: RLIMIT_NPROC 1, the user
        // having other processes); this is the process's first request.
        let old = CSys.prlimit(0, liella_kernel::uapi::RLIMIT_NPROC, None).unwrap();
        let low = liella_kernel::uapi::Rlimit { cur: 1, max: old.max };
        CSys.prlimit(0, liella_kernel::uapi::RLIMIT_NPROC, Some(&low)).unwrap();
        let z = x::liella_open3(c"/dev/zero".as_ptr(), O_RDONLY, 0);
        let mut zb = [0u8; 4];
        let mut zc = Cb::new(z, zb.as_mut_ptr(), 4, 0);
        let zp = &raw mut zc;
        errno::set_raw(0);
        let r = x::aio_read(zp.cast());
        CSys.prlimit(0, liella_kernel::uapi::RLIMIT_NPROC, Some(&old)).unwrap();
        if CSys.geteuid() == 0 {
            eprintln!("note: root is not held to RLIMIT_NPROC; EAGAIN not checked");
            if r == 0 {
                assert!(settled(zp));
            }
        } else {
            assert!(err(r as isize, Errno::EAGAIN), "no thread can be made: EAGAIN at submission");
            assert_ne!(liella_boundary::selftest::aio_engine(), "ring", "nothing was set up");
        }
        // Once threads can be made again, requests run.
        assert_eq!(x::aio_read(zp.cast()), 0);
        assert!(settled(zp));
        assert_eq!(aio_ret(zp), 4);

        // EINVAL at submission.
        let (path, fd) = file("aio-f", b"0123456789");
        let mut buf = [0u8; 16];
        for prio in [-1, 21, i32::MAX, i32::MIN] {
            let mut cb = Cb::new(fd, buf.as_mut_ptr(), 4, 0);
            cb.reqprio = prio;
            errno::set_raw(0);
            assert!(err(x::aio_read((&raw mut cb).cast()) as isize, Errno::EINVAL), "aio_reqprio {prio}");
        }
        for op in [0, 2, O_SYNC | O_APPEND, -1] {
            let mut cb = Cb::new(-1, core::ptr::null(), 0, 0);
            errno::set_raw(0);
            assert!(err(x::aio_fsync(op, (&raw mut cb).cast()) as isize, Errno::EINVAL), "aio_fsync operation {op}, checked before the descriptor");
        }
        let mut cb = Cb::new(fd, buf.as_mut_ptr(), 4, 0);
        cb.notify = SIGEV_THREAD;
        assert!(err(x::aio_read((&raw mut cb).cast()) as isize, Errno::EINVAL), "SIGEV_THREAD without a function");
        assert!(err(x::aio_read(null_mut()) as isize, Errno::EINVAL), "no aiocb");
        assert!(err(x::lio_listio(5, core::ptr::null(), 0, None) as isize, Errno::EINVAL), "lio_listio mode");
        assert!(err(x::aio_suspend(core::ptr::null(), -1, None) as isize, Errno::EINVAL), "negative nent");
        // EBADF at submission where Liella reports it there: aio_fsync and aio_cancel.
        let mut cb = Cb::new(-1, core::ptr::null(), 0, 0);
        assert!(err(x::aio_fsync(O_SYNC, (&raw mut cb).cast()) as isize, Errno::EBADF));
        assert!(err(x::aio_cancel(-1, null_mut()) as isize, Errno::EBADF));

        // The operation's own error is available through aio_error (aio_return -1, errno
        // untouched).
        let wo = x::liella_open3(c"/dev/null".as_ptr(), 1, 0);
        let dir = x::liella_open3(c"/".as_ptr(), O_RDONLY | O_DIRECTORY, 0);
        for (cbfd, n, off, want) in [(wo, 4, 0, Errno::EBADF), (-1, 4, 0, Errno::EBADF), (dir, 4, 0, Errno::EISDIR), (fd, 4, -1, Errno::EINVAL), (fd, (isize::MAX as usize) + 1, 0, Errno::EFAULT)] {
            let mut cb = Cb::new(cbfd, buf.as_mut_ptr(), n, off);
            let p = &raw mut cb;
            assert_eq!(x::aio_read(p.cast()), 0, "accepted: the error is the request's");
            assert!(settled(p));
            assert_eq!(aio_err(p), want.get());
            errno::set_raw(0);
            assert_eq!(aio_ret(p), -1);
            assert_eq!(errno::get(), 0);
        }
        // A list with a failing entry: LIO_WAIT reports EIO, the entry its own error.
        let mut a = Cb::new(fd, buf.as_mut_ptr(), 4, 0);
        a.opcode = LIO_READ;
        let mut b = Cb::new(wo, buf[8..].as_mut_ptr(), 4, 0);
        b.opcode = LIO_READ;
        let (pa, pb) = (&raw mut a, &raw mut b);
        let l = list(&[pa, pb]);
        assert!(err(x::lio_listio(LIO_WAIT, l.as_ptr().cast(), 2, None) as isize, Errno::EIO));
        assert_eq!((aio_err(pa), aio_ret(pa), aio_err(pb)), (0, 4, Errno::EBADF.get()));
        for d in [z, fd, wo, dir] {
            x::close(d);
        }
        let _ = std::fs::remove_file(std::str::from_utf8(path.as_bytes()).unwrap());
    });
}
#[test]
fn s11_12_c_1() {
    let fd = x::eventfd(2, 0);
    x::eventfd_write(fd, 3);
    let mut v = 0u64;
    assert_eq!(x::eventfd_read(fd, Some(&mut v)), 0);
    assert_eq!(v, 5, "the sum, then reset");
    let sem = x::eventfd(2, 1 | O_NONBLOCK);
    assert_eq!(x::eventfd_read(sem, Some(&mut v)), 0);
    assert_eq!(v, 1, "EFD_SEMAPHORE: one at a time");
    assert_eq!(x::eventfd_read(sem, Some(&mut v)), 0);
    errno::set_raw(0);
    assert_eq!((x::eventfd_read(sem, Some(&mut v)), errno::get()), (-1, Errno::EAGAIN.get()));
    x::close(fd);
    x::close(sem);
}

#[test]
fn s11_12_c_2() {
    let cdir = tmp("ino");
    let dir = std::path::PathBuf::from(cdir.to_str().unwrap());
    std::fs::create_dir(&dir).unwrap();
    let fd = x::inotify_init1(O_NONBLOCK);
    let wd = x::inotify_add_watch(fd, cdir.as_ptr(), 0x100 | 0x200);
    std::fs::write(dir.join("f"), b"x").unwrap();
    std::fs::remove_file(dir.join("f")).unwrap();
    let mut buf = [0u8; 4096];
    let n = x::read(fd, buf.as_mut_ptr().cast(), 4096) as usize;
    let mut masks = Vec::new();
    let mut off = 0;
    while off < n {
        let len = u32::from_ne_bytes(buf[off + 12..off + 16].try_into().unwrap()) as usize;
        masks.push(u32::from_ne_bytes(buf[off + 4..off + 8].try_into().unwrap()));
        off += 16 + len;
    }
    assert!(masks.contains(&0x100) && masks.contains(&0x200), "variable-length create and delete records: {masks:x?}");
    x::inotify_rm_watch(fd, wd);
    x::close(fd);
}

#[test]
fn s11_12_c_3() {
    let fd = x::fanotify_init(0, 0);
    if fd < 0 {
        assert_eq!(errno::get(), Errno::EPERM.get(), "needs CAP_SYS_ADMIN");
    } else {
        x::close(fd);
    }
}

#[test]
fn s11_13_c_1() {
    // Close-on-exec descriptors only: a concurrent test spawning a process would otherwise
    // inherit one and hold the lock past the closes below.
    let (p, f) = file("c131", b"");
    let a = x::liella_open3(p.as_ptr(), O_RDWR | 0o2000000, 0);
    x::close(f);
    let b = x::liella_open3(p.as_ptr(), O_RDWR | 0o2000000, 0);
    let d = x::liella_fcntl3(a, 1030, 0); // F_DUPFD_CLOEXEC: a duplicate, like dup
    assert_eq!(x::flock(a, 2), 0);
    assert_eq!(x::flock(d, 2 | 4), 0, "the same open file description owns it");
    errno::set_raw(0);
    assert!(err(x::flock(b, 1 | 4) as isize, Errno::EAGAIN), "another description conflicts");
    x::close(a);
    assert!(err(x::flock(b, 1 | 4) as isize, Errno::EAGAIN), "still held through the dup");
    x::close(d);
    // Released with the last descriptor. A process another test spawns at this moment holds a
    // copy of the descriptor table until its exec, so the release may lag by that window.
    let released = (0..200).any(|_| {
        let ok = x::flock(b, 1 | 4) == 0;
        if !ok {
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        ok
    });
    assert!(released, "released with the last descriptor");
    x::close(b);
}

#[test]
fn s11_13_c_2() {
    let (_f, fd) = file("c132", b"0123456789");
    x::lseek(fd, 4, 0);
    assert_eq!(x::lockf(fd, 2, 3), 0, "F_TLOCK on [4, 7)");
    let mut q = liella_kernel::uapi::Flock::new(1, 0, 0, 0);
    x::liella_fcntl3(fd, 5, &raw mut q as usize);
    assert_eq!(q.ltype, 2, "fcntl sees no conflict with our own record lock");
    assert_eq!(x::lockf(fd, 3, 3), 0, "F_TEST");
    assert_eq!(x::lockf(fd, 0, 3), 0, "F_ULOCK");
    x::close(fd);
}

#[test]
fn s11_13_f_0() {
    let (_f, fd) = file("f130", b"");
    errno::set_raw(0);
    assert!(err(x::lockf(fd, 42, 0) as isize, Errno::EINVAL));
    assert!(err(x::flock(fd, 0) as isize, Errno::EINVAL));
    x::close(fd);
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
