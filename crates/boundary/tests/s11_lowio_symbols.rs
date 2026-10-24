// Specified from spec §11 (phase 5; the asynchronous I/O symbols, §11.11, phase 13).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Translation-layer tests for the exported symbols of spec §11 (141 entries).
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! Files live under the temporary directory with unique names; buffers the library writes are
//! Rust vectors passed by pointer. The variadic C entry points (`open`, `openat`, `fcntl`,
//! `ioctl`, `mremap` and their aliases) are tested through their fixed-arity targets here and
//! through real C varargs in `conformance/c/s11_variadic.c`. Aborts run in re-executed copies
//! of this binary.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(non_snake_case)]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

#[path = "../../core/tests/support/kernel.rs"]
mod kernel;
#[path = "../../core/tests/support/sync_trap.rs"]
mod sync_trap;

use core::ptr::{null, null_mut};
use std::ffi::{CStr, CString};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::ExitStatusExt;
use std::sync::atomic::{AtomicU32, Ordering};

use liella_abi::ctypes::{c_int, c_void};
use liella_abi::layout::sigset_t;
use liella_boundary::errno;
use liella_boundary::posix::s11_lowio as s;
use liella_boundary::sys::CSys;
use liella_kernel::mem::{IoVec, IoVecMut, RemoteIoVec};
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{EpollEvent, FdSet, PollFd, Timespec, Timeval};

const EBADF: i32 = 9;
const EAGAIN: i32 = 11;
const EINVAL: i32 = 22;
const ESPIPE: i32 = 29;
const EEXIST: i32 = 17;
const ENOENT: i32 = 2;
const EOVERFLOW: i32 = 75;
const O_RDONLY: c_int = 0;
const O_RDWR: c_int = 2;
const O_CREAT: c_int = 0o100;
const O_EXCL: c_int = 0o200;
const O_TRUNC: c_int = 0o1000;
const O_NONBLOCK: c_int = 0o4000;
const O_CLOEXEC: c_int = 0o2000000;
const AT_FDCWD: c_int = -100;
const PROBE: &str = "LIELLA_S11_PROBE";

static SEQ: AtomicU32 = AtomicU32::new(0);

/// Moves `fd` to the private number `n`: tests that close a number twice, or a range, must
/// not touch numbers a concurrently running test may have been given (descriptor numbering is
/// process-wide). Numbers 900 and up are reserved for this, one per test.
fn private(fd: c_int, n: c_int) -> c_int {
    assert_eq!(s::dup2(fd, n), n);
    s::close(fd);
    n
}

/// A scratch path under the target directory, for a file or directory a test creates; it is
/// removed with whatever it holds when dropped, so a failing test leaves nothing behind either.
#[derive(Debug)]
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

/// A fresh path under the target directory (in a re-executed child, in the directory its
/// parent made for it).
fn tmp(tag: &str) -> Tmp {
    let n = SEQ.fetch_add(1, Ordering::Relaxed);
    let base = std::env::var_os(CHILD_TMP).map_or_else(|| std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")), std::path::PathBuf::from);
    let p = base.join(format!("s11-{}-{tag}-{n}", std::process::id()));
    let _ = std::fs::remove_file(&p);
    let _ = std::fs::remove_dir_all(&p);
    Tmp(CString::new(p.into_os_string().into_encoded_bytes()).unwrap())
}

/// A new file opened read-write, with `content`.
fn file(tag: &str, content: &[u8]) -> (Tmp, c_int) {
    let p = tmp(tag);
    let fd = s::liella_open3(p.as_ptr(), O_RDWR | O_CREAT | O_TRUNC, 0o600);
    assert!(fd >= 0, "open {p:?}: errno {}", errno::get());
    assert_eq!(s::write(fd, content.as_ptr().cast(), content.len()), content.len() as isize);
    (p, fd)
}

/// A memory file (memfd) holding `content`, for the durability calls (`fsync`, `fdatasync`,
/// `syncfs`, `msync`, `sync_file_range`): on the target file system they wait for the journal and
/// with it every dirty page of a parallel build (GATE-FSYNC: ~100 s each under gate load). What
/// the tests check - the call reaches the kernel, its result and its errors - is the same on any
/// regular file; a memfd's sync returns at once.
fn mem_file(tag: &CStr, content: &[u8]) -> c_int {
    let fd = s::memfd_create(tag.as_ptr(), 1);
    assert!(fd >= 0, "memfd_create: errno {}", errno::get());
    assert_eq!(s::write(fd, content.as_ptr().cast(), content.len()), content.len() as isize);
    fd
}

fn pipe() -> (c_int, c_int) {
    let [r, w] = CSys.pipe2(0).unwrap();
    (r.raw(), w.raw())
}

fn is_open(fd: c_int) -> bool {
    CSys.fcntl_int(liella_kernel::Fd::new(fd), liella_kernel::ops::FcntlInt::GetFd, 0).is_ok()
}

fn closed(fd: c_int) -> bool {
    errno::set_raw(0);
    s::close(fd) == -1 && errno::get() == EBADF
}

fn fails(r: isize, e: i32) -> bool {
    r == -1 && errno::get() == e
}

fn probe() -> Option<String> {
    std::env::var(PROBE).ok()
}

fn run_probe(test: &str, name: &str) -> std::process::Output {
    // The child's scratch files go into a directory of this process's own, removed here with
    // whatever the child left (a fortify child aborts and drops nothing).
    let scratch = tmp("child");
    std::fs::create_dir(scratch.to_str().unwrap()).unwrap();
    std::process::Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact", "--test-threads=1", "--nocapture"])
        .env(PROBE, name)
        .env(CHILD_TMP, scratch.to_str().unwrap())
        .output()
        .unwrap()
}

/// The variable naming a re-executed child's scratch directory ([`run_probe`]).
const CHILD_TMP: &str = "LIELLA_S11_TMP";

fn conformance(names: &[&str]) {
    static RUNNER: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = RUNNER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert!(root.join("target/liella/libc.a").exists(), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let o = std::process::Command::new("sh").arg(root.join("conformance/run.sh")).args(names).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

fn sigset(bits: u64) -> sigset_t {
    let mut s = sigset_t { __val: [0; 16] };
    s.__val[0] = bits;
    s
}

/// Map `len` anonymous read-write bytes.
fn anon(len: usize) -> *mut c_void {
    let p = s::mmap(null_mut(), len, 3, 0x22, -1, 0);
    assert_ne!(p as isize, -1);
    p
}

// ------------------------------------------------------------------------------ asynchronous I/O (§11.11)
//
// The exports run on the process's engine (io_uring where the kernel permits it, else the
// workers; the clause tests run both). Every request a test makes is complete before the test
// returns, so the kernel never writes into a buffer that is gone.

/// A `struct aiocb` (168 bytes): the library writes its status words concurrently, so a
/// submitted block is reached only through a raw pointer.
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

const EINPROGRESS: i32 = 115;
const ECANCELED: i32 = 125;
const EIO: i32 = 5;
const SIGEV_NONE: c_int = 1;
const O_SYNC: c_int = 0o4010000;
const O_DSYNC: c_int = 0o10000;

impl Cb {
    fn new(fd: c_int, buf: *const u8, n: usize, off: i64) -> Cb {
        Cb { fildes: fd, buf: buf.addr(), nbytes: n, offset: off, notify: SIGEV_NONE, ..Cb::default() }
    }
}

/// Waits (at most [`SETTLE_CAP`]) until the request is no longer in progress. The cap only ends a
/// wait that would otherwise hang: under gate load an fsync stalled for ~16 s (b27), so it is far
/// above that. On timeout the failure message carries the elapsed time.
const SETTLE_CAP: std::time::Duration = std::time::Duration::from_secs(60);
fn settled(p: *mut Cb) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < SETTLE_CAP {
        if s::aio_error(p.cast()) != EINPROGRESS {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
    assert!(
        s::aio_error(p.cast()) != EINPROGRESS,
        "aio request still in progress after {:?} (cap {SETTLE_CAP:?})",
        start.elapsed()
    );
    true
}

fn gone(path: &CString) {
    let _ = std::fs::remove_file(std::str::from_utf8(path.as_bytes()).unwrap());
}

fn feed(fd: c_int, b: &[u8]) {
    assert_eq!(s::write(fd, b.as_ptr().cast(), b.len()), b.len() as isize);
}

/// `aio_read`/`aio_read64`: 0 when queued; the request reads at `aio_offset`; `EINVAL` at
/// submission for an invalid priority or no block.
fn check_read(read: extern "C" fn(*mut c_void) -> c_int) {
    let (path, fd) = file("aio_read", b"0123456789");
    let mut buf = [0u8; 8];
    let mut cb = Cb::new(fd, buf.as_mut_ptr(), 5, 3);
    let p = &raw mut cb;
    assert_eq!(read(p.cast()), 0);
    assert!(settled(p));
    assert_eq!(s::aio_return(p.cast()), 5);
    assert_eq!(&buf[..5], b"34567");
    let mut bad = Cb::new(fd, buf.as_mut_ptr(), 5, 0);
    bad.reqprio = 21;
    errno::set_raw(0);
    assert!(fails(read((&raw mut bad).cast()) as isize, EINVAL));
    assert!(fails(read(null_mut()) as isize, EINVAL));
    s::close(fd);
    gone(&path);
}

/// `aio_write`/`aio_write64` at an offset past 4 GiB (the block's 64-bit `aio_offset`).
fn check_write(write: extern "C" fn(*mut c_void) -> c_int) {
    let (path, fd) = file("aio_write", b"");
    let off = 5i64 << 30;
    let mut cb = Cb::new(fd, b"large".as_ptr(), 5, off);
    let p = &raw mut cb;
    assert_eq!(write(p.cast()), 0);
    assert!(settled(p));
    assert_eq!((s::aio_error(p.cast()), s::aio_return(p.cast())), (0, 5));
    let mut back = [0u8; 5];
    assert_eq!(s::pread(fd, back.as_mut_ptr().cast(), 5, off), 5);
    assert_eq!(&back, b"large");
    assert!(fails(write(null_mut()) as isize, EINVAL));
    s::close(fd);
    gone(&path);
}

/// `aio_fsync`/`aio_fsync64`: `O_SYNC` and `O_DSYNC` only (`EINVAL`, checked first), then
/// `EBADF` for a descriptor that is not open, both at submission.
fn check_fsync(fsync: extern "C" fn(c_int, *mut c_void) -> c_int) {
    // A memory file, not one under the target directory: on ext4 (data=ordered) an fsync of a
    // 4-byte file waits for the journal commit and with it every other dirty file on the disk.
    // Measured on keke (FLAKES-6): 8 s with 4 GB dirty, 39 s under a 12 GB write, and in gates
    // over 60 s (sym_fsync and sym_fdatasync took 100 s in the same run). The library's part -
    // queue, run, status words - is the same on any descriptor; a memfd's fsync returns at once.
    let fd = s::memfd_create(c"aio_fsync".as_ptr(), 1);
    assert!(fd >= 0, "memfd_create: errno {}", errno::get());
    assert_eq!(s::write(fd, b"data".as_ptr().cast(), 4), 4);
    for op in [O_SYNC, O_DSYNC] {
        let mut cb = Cb::new(fd, core::ptr::null(), 0, 0);
        let p = &raw mut cb;
        assert_eq!(fsync(op, p.cast()), 0);
        assert!(settled(p));
        assert_eq!((s::aio_error(p.cast()), s::aio_return(p.cast())), (0, 0));
    }
    let mut cb = Cb::new(-1, core::ptr::null(), 0, 0);
    errno::set_raw(0);
    assert!(fails(fsync(0, (&raw mut cb).cast()) as isize, EINVAL), "the operation first");
    assert!(fails(fsync(O_SYNC, (&raw mut cb).cast()) as isize, EBADF));
    s::close(fd);
}

/// `aio_error`/`aio_error64`: the status word; `EINPROGRESS` while running; -1 `EINVAL` for no
/// block.
fn check_error(error: extern "C" fn(*const c_void) -> c_int) {
    let mut zero = Cb::default();
    assert_eq!(error((&raw mut zero).cast()), 0, "a zeroed block never submitted");
    let (r, w) = pipe();
    let mut buf = [0u8; 4];
    let mut cb = Cb::new(r, buf.as_mut_ptr(), 2, 0);
    let p = &raw mut cb;
    assert_eq!(s::aio_read(p.cast()), 0);
    assert_eq!(error(p.cast()), EINPROGRESS);
    feed(w, b"hi");
    assert!(settled(p));
    assert_eq!(error(p.cast()), 0);
    errno::set_raw(0);
    assert!(fails(error(core::ptr::null()) as isize, EINVAL));
    s::close(r);
    s::close(w);
}

/// `aio_return`/`aio_return64`: -1 without `errno` while running or after a failure; the count
/// after a success, as often as asked; -1 `EINVAL` for no block.
fn check_return(ret: extern "C" fn(*mut c_void) -> isize) {
    let (r, w) = pipe();
    let mut buf = [0u8; 4];
    let mut cb = Cb::new(r, buf.as_mut_ptr(), 3, 0);
    let p = &raw mut cb;
    assert_eq!(s::aio_read(p.cast()), 0);
    errno::set_raw(0);
    assert_eq!(ret(p.cast()), -1);
    assert_eq!(errno::get(), 0);
    feed(w, b"abc");
    assert!(settled(p));
    assert_eq!(ret(p.cast()), 3);
    assert_eq!(ret(p.cast()), 3);
    let mut bad = Cb::new(-1, buf.as_mut_ptr(), 3, 0);
    let bp = &raw mut bad;
    assert_eq!(s::aio_read(bp.cast()), 0);
    assert!(settled(bp));
    errno::set_raw(0);
    assert_eq!(ret(bp.cast()), -1);
    assert_eq!(errno::get(), 0, "the error is aio_error's");
    assert!(fails(ret(null_mut()), EINVAL));
    s::close(r);
    s::close(w);
}

/// `aio_suspend`/`aio_suspend64`: `EAGAIN` when the timeout passes, 0 for a completed entry (or
/// none in progress), `EINVAL` for a negative count.
fn check_suspend(suspend: extern "C" fn(*const *const c_void, c_int, Option<&Timespec>) -> c_int) {
    let (r, w) = pipe();
    let mut buf = [0u8; 4];
    let mut cb = Cb::new(r, buf.as_mut_ptr(), 1, 0);
    let p = &raw mut cb;
    assert_eq!(s::aio_read(p.cast()), 0);
    let l: [*const c_void; 2] = [core::ptr::null(), p.cast()];
    let t = Timespec { sec: 0, nsec: 20_000_000 };
    errno::set_raw(0);
    assert!(fails(suspend(l.as_ptr(), 2, Some(&t)) as isize, EAGAIN));
    feed(w, b"x");
    assert_eq!(suspend(l.as_ptr(), 2, None), 0);
    assert_eq!(s::aio_return(p.cast()), 1);
    assert_eq!(suspend(l.as_ptr(), 1, None), 0, "only a NULL entry: nothing can complete (the reference)");
    assert_eq!(suspend(core::ptr::null(), 0, None), 0);
    assert!(fails(suspend(l.as_ptr(), -1, None) as isize, EINVAL));
    s::close(r);
    s::close(w);
}

/// `aio_cancel`/`aio_cancel64`: `EBADF` for a closed descriptor, `EINVAL` for a block naming
/// another descriptor, `AIO_CANCELED` for a queued request, `AIO_NOTCANCELED` for a running one,
/// `AIO_ALLDONE` when nothing is left.
fn check_cancel(cancel: extern "C" fn(c_int, *mut c_void) -> c_int) {
    let (r, w) = pipe();
    let (mut a, mut b) = ([0u8; 2], [0u8; 2]);
    let mut first = Cb::new(r, a.as_mut_ptr(), 1, 0);
    let mut second = Cb::new(r, b.as_mut_ptr(), 1, 0);
    let (p1, p2) = (&raw mut first, &raw mut second);
    assert_eq!(s::aio_read(p1.cast()), 0);
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert_eq!(s::aio_read(p2.cast()), 0);
    errno::set_raw(0);
    assert!(fails(cancel(-1, null_mut()) as isize, EBADF));
    assert!(fails(cancel(w, p2.cast()) as isize, EINVAL), "the block names another descriptor");
    assert_eq!(cancel(r, p2.cast()), 0, "AIO_CANCELED");
    assert_eq!((s::aio_error(p2.cast()), s::aio_return(p2.cast())), (ECANCELED, -1));
    assert_eq!(cancel(r, p1.cast()), 1, "AIO_NOTCANCELED");
    feed(w, b"z");
    assert!(settled(p1));
    assert_eq!(cancel(r, null_mut()), 2, "AIO_ALLDONE");
    s::close(r);
    s::close(w);
}

/// `lio_listio`/`lio_listio64`: `EINVAL` for a mode; `LIO_WAIT` returns when every entry is
/// done (0, or `EIO` when one failed); NULL and `LIO_NOP` entries are skipped.
fn check_listio(listio: extern "C" fn(c_int, *const *mut c_void, c_int, Option<&liella_kernel::uapi::SigEvent>) -> c_int) {
    let (path, fd) = file("lio_listio", b"abcdef");
    let mut buf = [0u8; 4];
    let mut rd = Cb::new(fd, buf.as_mut_ptr(), 3, 1);
    rd.opcode = 0;
    let mut wr = Cb::new(fd, b"XY".as_ptr(), 2, 6);
    wr.opcode = 1;
    let mut nop = Cb::new(-1, core::ptr::null(), 0, 0);
    nop.opcode = 2;
    let (pr, pw, pn) = (&raw mut rd, &raw mut wr, &raw mut nop);
    let l: [*mut c_void; 4] = [pr.cast(), null_mut(), pw.cast(), pn.cast()];
    errno::set_raw(0);
    assert!(fails(listio(5, l.as_ptr(), 4, None) as isize, EINVAL));
    assert_eq!(listio(0, l.as_ptr(), 4, None), 0, "LIO_WAIT");
    assert_eq!((s::aio_return(pr.cast()), s::aio_return(pw.cast())), (3, 2));
    assert_eq!(&buf[..3], b"bcd");
    assert_eq!(s::aio_error(pn.cast()), 0, "the NOP entry is untouched");
    let mut bad = Cb::new(-1, buf.as_mut_ptr(), 1, 0);
    let pb = &raw mut bad;
    let l2: [*mut c_void; 2] = [pr.cast(), pb.cast()];
    assert!(fails(listio(0, l2.as_ptr(), 2, None) as isize, EIO));
    assert_eq!(s::aio_error(pb.cast()), EBADF);
    assert_eq!(listio(1, l2.as_ptr(), 2, None), 0, "LIO_NOWAIT: queued");
    assert!(settled(pr) && settled(pb));
    assert_eq!(listio(0, l.as_ptr(), -1, None), 0, "a negative count is an empty list");
    s::close(fd);
    gone(&path);
}

#[test]
fn sym_aio_read() {
    check_read(s::aio_read);
}

#[test]
fn sym_aio_read64() {
    check_read(s::aio_read64);
}

#[test]
fn sym_aio_write() {
    check_write(s::aio_write);
}

#[test]
fn sym_aio_write64() {
    check_write(s::aio_write64);
}

#[test]
fn sym_aio_fsync() {
    check_fsync(s::aio_fsync);
}

#[test]
fn sym_aio_fsync64() {
    check_fsync(s::aio_fsync64);
}

#[test]
fn sym_aio_error() {
    check_error(s::aio_error);
}

#[test]
fn sym_aio_error64() {
    check_error(s::aio_error64);
}

#[test]
fn sym_aio_return() {
    check_return(s::aio_return);
}

#[test]
fn sym_aio_return64() {
    check_return(s::aio_return64);
}

#[test]
fn sym_aio_suspend() {
    check_suspend(s::aio_suspend);
}

#[test]
fn sym_aio_suspend64() {
    check_suspend(s::aio_suspend64);
}

#[test]
fn sym_aio_cancel() {
    if probe().as_deref() == Some("aio-close-first") {
        // Fresh process: the first asynchronous I/O this process ever starts is the aio_read below,
        // so the direct `close` entry has been told about it by the submission alone (D-22.11
        // "Direct points"; in a process where an earlier test already started aio the mark is set
        // for it and cannot be told apart).
        close_cancels_queued();
        CSys.exit_group(0);
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["sym_aio_cancel", "--exact", "--test-threads=1", "--nocapture"])
        .env(PROBE, "aio-close-first")
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "close cancels queued aio requests when it follows the process's first aio_read: {}", String::from_utf8_lossy(&o.stderr));
    check_cancel(s::aio_cancel);
    close_cancels_queued();
}

/// close cancels the requests on the descriptor that have not started (D-11.8).
fn close_cancels_queued() {
    let (r, w) = pipe();
    let (mut a, mut b) = ([0u8; 2], [0u8; 2]);
    let mut first = Cb::new(r, a.as_mut_ptr(), 1, 0);
    let mut second = Cb::new(r, b.as_mut_ptr(), 1, 0);
    let (p1, p2) = (&raw mut first, &raw mut second);
    assert_eq!(s::aio_read(p1.cast()), 0);
    std::thread::sleep(std::time::Duration::from_millis(100));
    assert_eq!(s::aio_read(p2.cast()), 0);
    assert_eq!(s::close(r), 0);
    assert_eq!(s::aio_error(p2.cast()), ECANCELED);
    assert_eq!(s::aio_error(p1.cast()), EINPROGRESS, "a running request completes as if open");
    feed(w, b"q");
    assert!(settled(p1));
    assert_eq!(s::aio_return(p1.cast()), 1);
    s::close(w);
}

#[test]
fn sym_aio_cancel64() {
    check_cancel(s::aio_cancel64);
}

#[test]
fn sym_lio_listio() {
    check_listio(s::lio_listio);
}

#[test]
fn sym_lio_listio64() {
    check_listio(s::lio_listio64);
}

#[test]
fn sym_aio_init() {
    // aio_init changes the process's worker pool: in a re-executed copy of this binary, on
    // the worker engine, where one worker makes a request on another descriptor wait.
    if probe().as_deref() == Some("aio_init") {
        liella_boundary::selftest::aio_force_workers(true);
        s::aio_init(None);
        let init = liella_abi::layout::struct_aioinit { aio_threads: 1, aio_num: 4, aio_locks: 0, aio_usedba: 0, aio_debug: 0, aio_numusers: 0, aio_idle_time: 1, aio_reserved: 0 };
        s::aio_init(Some(&init));
        let (r, w) = pipe();
        let mut buf = [0u8; 4];
        let mut held = Cb::new(r, buf.as_mut_ptr(), 1, 0);
        let hp = &raw mut held;
        assert_eq!(s::aio_read(hp.cast()), 0);
        std::thread::sleep(std::time::Duration::from_millis(100));
        let z = s::liella_open3(c"/dev/zero".as_ptr(), O_RDONLY, 0);
        let mut zb = [1u8; 4];
        let mut zc = Cb::new(z, zb.as_mut_ptr(), 4, 0);
        let zp = &raw mut zc;
        assert_eq!(s::aio_read(zp.cast()), 0);
        std::thread::sleep(std::time::Duration::from_millis(300));
        assert_eq!(s::aio_error(zp.cast()), EINPROGRESS, "one worker: the second request waits");
        feed(w, b"!");
        assert!(settled(hp) && settled(zp));
        assert_eq!(zb, [0; 4]);
        return;
    }
    let o = run_probe("sym_aio_init", "aio_init");
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}
#[test]
fn sym___close() {
    let fd = private(file("__close", b"").1, 921);
    assert_eq!(s::__close(fd), 0);
    errno::set_raw(0);
    assert!(fails(s::__close(fd) as isize, EBADF), "released: a second close fails");
}

#[test]
fn sym_close() {
    let fd = private(file("close", b"").1, 920);
    assert_eq!(s::close(fd), 0);
    errno::set_raw(0);
    assert!(fails(s::close(fd) as isize, EBADF), "released: a second close fails");
}

#[test]
fn sym_close_range() {
    let (_f, a) = file("cr", b"");
    let (b, c) = (s::dup2(a, 922), s::dup2(a, 923));
    let (lo, hi) = (922, 923);
    if kernel::before(5, 11) {
        kernel::note("CLOSE_RANGE_CLOEXEC", "5.11");
        errno::set_raw(0);
        assert!(fails(s::close_range(lo, hi, 4) as isize, EINVAL), "an unknown flag before 5.11");
    } else {
        assert_eq!(s::close_range(lo, hi, 4), 0, "CLOSE_RANGE_CLOEXEC marks");
        assert_eq!(CSys.fcntl_int(liella_kernel::Fd::new(b), liella_kernel::ops::FcntlInt::GetFd, 0).unwrap(), 1);
    }
    assert_eq!(s::close_range(lo, hi, 0), 0);
    assert!(closed(b) && closed(c));
    s::close(a);
}

#[test]
fn sym_closefrom() {
    if probe().as_deref() == Some("from") {
        let fds: Vec<c_int> = (0..5).map(|_| s::dup(2)).collect();
        let lo = fds[2];
        s::closefrom(lo);
        let ok = fds[..2].iter().all(|&f| is_open(f)) && fds[2..].iter().all(|&f| !is_open(f));
        CSys.exit_group(if ok { 0 } else { 1 });
    }
    let o = run_probe("sym_closefrom", "from");
    assert_eq!(o.status.code(), Some(0));
}

#[test]
fn sym_copy_file_range() {
    let (_f, a) = file("cfr_a", b"copy me");
    let (_f, b) = file("cfr_b", b"");
    let (mut ia, mut ob) = (0i64, 0i64);
    assert_eq!(s::copy_file_range(a, Some(&mut ia), b, Some(&mut ob), 7, 0), 7);
    let mut buf = [0u8; 7];
    s::pread(b, buf.as_mut_ptr().cast(), 7, 0);
    assert_eq!(&buf, b"copy me");
    errno::set_raw(0);
    assert!(fails(s::copy_file_range(a, None, b, None, 1, 1), EINVAL), "flags must be 0");
    s::close(a);
    s::close(b);
}

#[test]
fn sym_creat() {
    let (p, fd0) = file("creat", b"old contents");
    s::close(fd0);
    let fd = s::creat(p.as_ptr(), 0o600);
    assert!(fd >= 0);
    let mut b = [0u8; 4];
    errno::set_raw(0);
    assert!(fails(s::read(fd, b.as_mut_ptr().cast(), 4), EBADF), "write-only");
    assert_eq!(s::lseek(fd, 0, 2), 0, "truncated");
    s::close(fd);
}

#[test]
fn sym_creat64() {
    let (p, fd0) = file("creat64", b"old contents");
    s::close(fd0);
    let fd = s::creat64(p.as_ptr(), 0o600);
    assert!(fd >= 0);
    let mut b = [0u8; 4];
    errno::set_raw(0);
    assert!(fails(s::read(fd, b.as_mut_ptr().cast(), 4), EBADF), "write-only");
    assert_eq!(s::lseek(fd, 0, 2), 0, "truncated");
    s::close(fd);
}

#[test]
fn sym_dup() {
    let (_f, fd) = file("dup", b"abc");
    let d = s::dup(fd);
    assert!(d >= 0 && d != fd, "a new descriptor");
    assert_eq!(s::lseek(d, 1, 0), 1);
    assert_eq!(s::lseek(fd, 0, 1), 1, "the open file description is shared");
    s::close(d);
    s::close(fd);
}

#[test]
fn sym___dup2() {
    let (_f, fd) = file("__dup2", b"");
    let (_f, other) = file("__dup2b", b"");
    assert_eq!(s::__dup2(fd, other), other, "closes other first");
    assert_eq!(s::__dup2(fd, fd), fd, "same: validated and returned");
    errno::set_raw(0);
    assert!(fails(s::__dup2(-1, fd) as isize, EBADF));
    s::close(other);
    s::close(fd);
}

#[test]
fn sym_dup2() {
    let (_f, fd) = file("dup2", b"");
    let (_f, other) = file("dup2b", b"");
    assert_eq!(s::dup2(fd, other), other, "closes other first");
    assert_eq!(s::dup2(fd, fd), fd, "same: validated and returned");
    errno::set_raw(0);
    assert!(fails(s::dup2(-1, fd) as isize, EBADF));
    s::close(other);
    s::close(fd);
}

#[test]
fn sym_dup3() {
    let (_f, fd) = file("dup3", b"");
    let (_f, other) = file("dup3b", b"");
    assert_eq!(s::dup3(fd, other, O_CLOEXEC), other);
    assert_eq!(CSys.fcntl_int(liella_kernel::Fd::new(other), liella_kernel::ops::FcntlInt::GetFd, 0).unwrap(), 1);
    errno::set_raw(0);
    assert!(fails(s::dup3(fd, fd, 0) as isize, EINVAL), "the same descriptor");
    s::close(other);
    s::close(fd);
}

#[test]
fn sym_epoll_create1() {
    let ep = s::epoll_create1(O_CLOEXEC);
    assert!(ep >= 0);
    errno::set_raw(0);
    assert!(fails(s::epoll_create1(1) as isize, EINVAL));
    s::close(ep);
}

#[test]
fn sym_epoll_create() {
    errno::set_raw(0);
    assert!(fails(s::epoll_create(0) as isize, EINVAL), "size must be > 0");
    let ep = s::epoll_create(1);
    assert!(ep >= 0);
    s::close(ep);
}

#[test]
fn sym_epoll_ctl() {
    let (r, w) = pipe();
    let ep = s::epoll_create1(0);
    let e = EpollEvent { events: 1, data: 0xC0FFEE };
    assert_eq!(s::epoll_ctl(ep, 1, r, Some(&e)), 0);
    s::write(w, b"x".as_ptr().cast(), 1);
    let mut ev = [EpollEvent::default(); 4];
    assert_eq!(s::epoll_wait(ep, ev.as_mut_ptr().cast(), 4, 1000), 1);
    let (events, data) = (ev[0].events, ev[0].data);
    assert_eq!((events, data), (1, 0xC0FFEE), "the caller's 64-bit cookie");
    errno::set_raw(0);
    assert!(fails(s::epoll_ctl(ep, 1, r, Some(&e)) as isize, EEXIST));
    for f in [ep, r, w] {
        s::close(f);
    }
}

#[test]
fn sym_epoll_pwait2() {
    let (r, w) = pipe();
    let ep = s::epoll_create1(0);
    let e = EpollEvent { events: 1, data: 0xC0FFEE };
    assert_eq!(s::epoll_ctl(ep, 1, r, Some(&e)), 0);
    s::write(w, b"x".as_ptr().cast(), 1);
    let mut ev = [EpollEvent::default(); 4];
    if kernel::before(5, 11) {
        kernel::note("epoll_pwait2", "5.11");
        errno::set_raw(0);
        assert!(fails(s::epoll_pwait2(ep, ev.as_mut_ptr().cast(), 4, Some(&Timespec { sec: 1, nsec: 0 }), None) as isize, 38), "ENOSYS");
        for f in [ep, r, w] {
            s::close(f);
        }
        return;
    }
    assert_eq!(s::epoll_pwait2(ep, ev.as_mut_ptr().cast(), 4, Some(&Timespec { sec: 1, nsec: 0 }), None), 1);
    let (events, data) = (ev[0].events, ev[0].data);
    assert_eq!((events, data), (1, 0xC0FFEE), "the caller's 64-bit cookie");
    errno::set_raw(0);
    assert!(fails(s::epoll_ctl(ep, 1, r, Some(&e)) as isize, EEXIST));
    for f in [ep, r, w] {
        s::close(f);
    }
}

#[test]
fn sym_epoll_pwait() {
    let (r, w) = pipe();
    let ep = s::epoll_create1(0);
    let e = EpollEvent { events: 1, data: 0xC0FFEE };
    assert_eq!(s::epoll_ctl(ep, 1, r, Some(&e)), 0);
    s::write(w, b"x".as_ptr().cast(), 1);
    let mut ev = [EpollEvent::default(); 4];
    assert_eq!(s::epoll_pwait(ep, ev.as_mut_ptr().cast(), 4, 1000, Some(&sigset(0))), 1);
    let (events, data) = (ev[0].events, ev[0].data);
    assert_eq!((events, data), (1, 0xC0FFEE), "the caller's 64-bit cookie");
    errno::set_raw(0);
    assert!(fails(s::epoll_ctl(ep, 1, r, Some(&e)) as isize, EEXIST));
    for f in [ep, r, w] {
        s::close(f);
    }
}

#[test]
fn sym_epoll_wait() {
    let (r, w) = pipe();
    let ep = s::epoll_create1(0);
    let e = EpollEvent { events: 1, data: 0xC0FFEE };
    assert_eq!(s::epoll_ctl(ep, 1, r, Some(&e)), 0);
    s::write(w, b"x".as_ptr().cast(), 1);
    let mut ev = [EpollEvent::default(); 4];
    assert_eq!(s::epoll_wait(ep, ev.as_mut_ptr().cast(), 4, 1000), 1);
    let (events, data) = (ev[0].events, ev[0].data);
    assert_eq!((events, data), (1, 0xC0FFEE), "the caller's 64-bit cookie");
    errno::set_raw(0);
    assert!(fails(s::epoll_ctl(ep, 1, r, Some(&e)) as isize, EEXIST));
    for f in [ep, r, w] {
        s::close(f);
    }
}

#[test]
fn sym_eventfd_read() {
    let fd = s::eventfd(0, O_NONBLOCK);
    s::eventfd_write(fd, 3);
    s::eventfd_write(fd, 4);
    let mut v = 0u64;
    assert_eq!(s::eventfd_read(fd, Some(&mut v)), 0);
    assert_eq!(v, 7, "the counter, reset");
    errno::set_raw(0);
    assert_eq!((s::eventfd_read(fd, Some(&mut v)), errno::get()), (-1, EAGAIN));
    s::close(fd);
}

#[test]
fn sym_eventfd_write() {
    let fd = s::eventfd(0, 0);
    assert_eq!(s::eventfd_write(fd, 42), 0);
    errno::set_raw(0);
    assert_eq!((s::eventfd_write(fd, u64::MAX), errno::get()), (-1, EINVAL));
    s::close(fd);
}

#[test]
fn sym_eventfd() {
    let fd = s::eventfd(5, 1);
    assert!(fd >= 0);
    let mut v = 0u64;
    assert_eq!(s::eventfd_read(fd, Some(&mut v)), 0);
    assert_eq!(v, 1, "EFD_SEMAPHORE decrements by one");
    s::close(fd);
}

#[test]
fn sym_fallocate64() {
    let (_f, fd) = file("fallocate64", b"ab");
    assert_eq!(s::fallocate64(fd, 1, 0, 8192), 0, "FALLOC_FL_KEEP_SIZE");
    assert_eq!(s::lseek(fd, 0, 2), 2);
    let r = s::fallocate64(fd, 0, 0, 8192);
    assert!(r == 0 || errno::get() == 95);
    s::close(fd);
}

#[test]
fn sym_fallocate() {
    let (_f, fd) = file("fallocate", b"ab");
    assert_eq!(s::fallocate(fd, 1, 0, 8192), 0, "FALLOC_FL_KEEP_SIZE");
    assert_eq!(s::lseek(fd, 0, 2), 2);
    let r = s::fallocate(fd, 0, 0, 8192);
    assert!(r == 0 || errno::get() == 95);
    s::close(fd);
}

#[test]
fn sym_fanotify_init() {
    let fd = s::fanotify_init(0, 0);
    if fd >= 0 {
        s::close(fd);
        return;
    }
    assert!([1, 38].contains(&errno::get()), "needs CAP_SYS_ADMIN: {}", errno::get());
}

#[test]
fn sym_fanotify_mark() {
    errno::set_raw(0);
    assert_eq!(s::fanotify_mark(-1, 1, 1, AT_FDCWD, c"/tmp".as_ptr()), -1);
    assert!([EBADF, 38].contains(&errno::get()));
    assert_eq!(s::fanotify_mark(-1, 1, 1, AT_FDCWD, null()), -1, "a NULL path is the directory itself");
}

#[test]
fn sym___fcntl() {
    let (_f, fd) = file("__fcntl", b"");
    assert_eq!(s::liella_fcntl3(fd, 2, 1), 0, "F_SETFD FD_CLOEXEC (an int argument)");
    assert_eq!(s::liella_fcntl3(fd, 1, 0x1234_5678_0000_0000), 1, "F_GETFD: an absent argument is ignored");
    let mut fl = liella_kernel::uapi::Flock::new(1, 0, 0, 10);
    assert_eq!(s::liella_fcntl3(fd, 6, &raw mut fl as usize), 0, "F_SETLK (a struct flock)");
    errno::set_raw(0);
    assert!(fails(s::liella_fcntl3(fd, 6, 0) as isize, 14), "a NULL structure: EFAULT");
    assert!(fails(s::liella_fcntl3(fd, 0x7fff, 0) as isize, EINVAL), "an unknown command");
    s::close(fd);
    conformance(&["s11_variadic"]);
}

#[test]
fn sym_fcntl() {
    let (_f, fd) = file("fcntl", b"");
    assert_eq!(s::liella_fcntl3(fd, 2, 1), 0, "F_SETFD FD_CLOEXEC (an int argument)");
    assert_eq!(s::liella_fcntl3(fd, 1, 0x1234_5678_0000_0000), 1, "F_GETFD: an absent argument is ignored");
    let mut fl = liella_kernel::uapi::Flock::new(1, 0, 0, 10);
    assert_eq!(s::liella_fcntl3(fd, 6, &raw mut fl as usize), 0, "F_SETLK (a struct flock)");
    errno::set_raw(0);
    assert!(fails(s::liella_fcntl3(fd, 6, 0) as isize, 14), "a NULL structure: EFAULT");
    assert!(fails(s::liella_fcntl3(fd, 0x7fff, 0) as isize, EINVAL), "an unknown command");
    s::close(fd);
    conformance(&["s11_variadic"]);
}

#[test]
fn sym_fcntl64() {
    let (_f, fd) = file("fcntl64", b"");
    assert_eq!(s::liella_fcntl3(fd, 2, 1), 0, "F_SETFD FD_CLOEXEC (an int argument)");
    assert_eq!(s::liella_fcntl3(fd, 1, 0x1234_5678_0000_0000), 1, "F_GETFD: an absent argument is ignored");
    let mut fl = liella_kernel::uapi::Flock::new(1, 0, 0, 10);
    assert_eq!(s::liella_fcntl3(fd, 6, &raw mut fl as usize), 0, "F_SETLK (a struct flock)");
    errno::set_raw(0);
    assert!(fails(s::liella_fcntl3(fd, 6, 0) as isize, 14), "a NULL structure: EFAULT");
    assert!(fails(s::liella_fcntl3(fd, 0x7fff, 0) as isize, EINVAL), "an unknown command");
    s::close(fd);
    conformance(&["s11_variadic"]);
}

#[test]
fn sym_fdatasync() {
    let fd = mem_file(c"fdatasync", b"data");
    assert_eq!(s::fdatasync(fd), 0);
    errno::set_raw(0);
    assert!(fails(s::fdatasync(-1) as isize, EBADF));
    let (r, w) = pipe();
    errno::set_raw(0);
    assert!(fails(s::fdatasync(r) as isize, EINVAL), "a pipe: the kernel's EINVAL");
    for x in [fd, r, w] {
        s::close(x);
    }
}

#[test]
fn sym___fdelt_chk() {
    assert_eq!(s::__fdelt_chk(0), 0);
    assert_eq!(s::__fdelt_chk(1023), 15);
    if probe().as_deref() == Some("big") {
        liella_boundary::selftest::expect_crash();
        s::__fdelt_chk(1024);
        return;
    }
    let o = run_probe("sym___fdelt_chk", "big");
    assert_eq!(o.status.signal(), Some(6), "FD_SETSIZE or more aborts");
}

#[test]
fn sym___fdelt_warn() {
    assert_eq!(s::__fdelt_warn(0), 0);
    assert_eq!(s::__fdelt_warn(1023), 15);
    if probe().as_deref() == Some("big") {
        liella_boundary::selftest::expect_crash();
        s::__fdelt_warn(1024);
        return;
    }
    let o = run_probe("sym___fdelt_warn", "big");
    assert_eq!(o.status.signal(), Some(6), "FD_SETSIZE or more aborts");
}

#[test]
fn sym_flock() {
    let (p, a) = file("flock", b"");
    let b = s::liella_open3(p.as_ptr(), O_RDWR, 0);
    assert_eq!(s::flock(a, 2 | 4), 0, "LOCK_EX|LOCK_NB");
    errno::set_raw(0);
    assert!(fails(s::flock(b, 2 | 4) as isize, EAGAIN), "another open file description conflicts");
    assert_eq!(s::flock(a, 8), 0);
    assert_eq!(s::flock(b, 1 | 4), 0, "LOCK_SH once released");
    s::close(a);
    s::close(b);
}

#[test]
fn sym_fsync() {
    let fd = mem_file(c"fsync", b"data");
    assert_eq!(s::fsync(fd), 0);
    errno::set_raw(0);
    assert!(fails(s::fsync(-1) as isize, EBADF));
    let (r, w) = pipe();
    errno::set_raw(0);
    assert!(fails(s::fsync(r) as isize, EINVAL), "a pipe: the kernel's EINVAL");
    for x in [fd, r, w] {
        s::close(x);
    }
}

#[test]
fn sym_inotify_add_watch() {
    let cdir = tmp("ino-add");
    let dir = std::path::PathBuf::from(cdir.to_str().unwrap());
    std::fs::create_dir(&dir).unwrap();
    let fd = s::inotify_init1(O_NONBLOCK);
    let wd = s::inotify_add_watch(fd, cdir.as_ptr(), 0x100);
    assert!(wd >= 0);
    std::fs::write(dir.join("new"), b"x").unwrap();
    let mut buf = [0u8; 4096];
    assert!(s::read(fd, buf.as_mut_ptr().cast(), buf.len()) >= 16, "an IN_CREATE record");
    assert_eq!(u32::from_ne_bytes(buf[4..8].try_into().unwrap()) & 0x100, 0x100);
    assert_eq!(s::inotify_rm_watch(fd, wd), 0);
    errno::set_raw(0);
    assert!(fails(s::inotify_rm_watch(fd, wd) as isize, EINVAL));
    s::close(fd);
}

#[test]
fn sym_inotify_init1() {
    let fd = s::inotify_init1(O_NONBLOCK | O_CLOEXEC);
    assert!(fd >= 0);
    s::close(fd);
}

#[test]
fn sym_inotify_init() {
    let fd = s::inotify_init();
    assert!(fd >= 0);
    s::close(fd);
}

#[test]
fn sym_inotify_rm_watch() {
    let cdir = tmp("ino-rm");
    let dir = std::path::PathBuf::from(cdir.to_str().unwrap());
    std::fs::create_dir(&dir).unwrap();
    let fd = s::inotify_init1(O_NONBLOCK);
    let wd = s::inotify_add_watch(fd, cdir.as_ptr(), 0x100);
    assert!(wd >= 0);
    std::fs::write(dir.join("new"), b"x").unwrap();
    let mut buf = [0u8; 4096];
    assert!(s::read(fd, buf.as_mut_ptr().cast(), buf.len()) >= 16, "an IN_CREATE record");
    assert_eq!(u32::from_ne_bytes(buf[4..8].try_into().unwrap()) & 0x100, 0x100);
    assert_eq!(s::inotify_rm_watch(fd, wd), 0);
    errno::set_raw(0);
    assert!(fails(s::inotify_rm_watch(fd, wd) as isize, EINVAL));
    s::close(fd);
}

#[test]
fn sym_ioctl() {
    let (r, w) = pipe();
    s::write(w, b"abc".as_ptr().cast(), 3);
    let mut avail: c_int = 0;
    assert_eq!(s::liella_ioctl3(r, 0x541B, &raw mut avail as usize), 0, "FIONREAD");
    assert_eq!(avail, 3);
    errno::set_raw(0);
    assert!(fails(s::liella_ioctl3(r, 0x5401, 0) as isize, 25), "TCGETS on a pipe: ENOTTY");
    s::close(r);
    s::close(w);
    conformance(&["s11_variadic"]);
}

#[test]
fn sym_llseek() {
    let (_f, fd) = file("llseek", b"0123456789");
    assert_eq!(s::llseek(fd, 2, 0), 2);
    assert_eq!(s::llseek(fd, 3, 1), 5);
    assert_eq!(s::llseek(fd, -1, 2), 9);
    errno::set_raw(0);
    assert_eq!((s::llseek(fd, -20, 0), errno::get()), (-1, EINVAL));
    assert_eq!((s::llseek(fd, 0, 9), errno::get()), (-1, EINVAL));
    let (r, w) = pipe();
    assert_eq!((s::llseek(r, 0, 0), errno::get()), (-1, ESPIPE));
    for f in [fd, r, w] {
        s::close(f);
    }
}

#[test]
fn sym_lockf() {
    if let Some(path) = probe() {
        let p = CString::new(path).unwrap();
        let fd = s::liella_open3(p.as_ptr(), O_RDWR, 0);
        errno::set_raw(0);
        let test = s::lockf(fd, 3, 0);
        let tlock = s::lockf(fd, 2, 0);
        let e = errno::get();
        CSys.exit_group(if test == -1 && tlock == -1 && [EAGAIN, 13].contains(&e) { 0 } else { 1 });
    }
    let (p, fd) = file("lockf", b"locked");
    s::lseek(fd, 0, 0);
    assert_eq!(s::lockf(fd, 2, 0), 0, "F_TLOCK");
    assert_eq!(s::lockf(fd, 3, 0), 0, "F_TEST: our own lock does not conflict");
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["sym_lockf", "--exact", "--test-threads=1"])
        .env(PROBE, p.to_str().unwrap())
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "another process: F_TEST and F_TLOCK fail");
    assert_eq!(s::lockf(fd, 0, 0), 0, "F_ULOCK");
    errno::set_raw(0);
    assert!(fails(s::lockf(fd, 9, 0) as isize, EINVAL));
    s::close(fd);
}

#[test]
fn sym_lockf64() {
    if let Some(path) = probe() {
        let p = CString::new(path).unwrap();
        let fd = s::liella_open3(p.as_ptr(), O_RDWR, 0);
        errno::set_raw(0);
        let test = s::lockf64(fd, 3, 0);
        let tlock = s::lockf64(fd, 2, 0);
        let e = errno::get();
        CSys.exit_group(if test == -1 && tlock == -1 && [EAGAIN, 13].contains(&e) { 0 } else { 1 });
    }
    let (p, fd) = file("lockf64", b"locked");
    s::lseek(fd, 0, 0);
    assert_eq!(s::lockf64(fd, 2, 0), 0, "F_TLOCK");
    assert_eq!(s::lockf64(fd, 3, 0), 0, "F_TEST: our own lock does not conflict");
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["sym_lockf64", "--exact", "--test-threads=1"])
        .env(PROBE, p.to_str().unwrap())
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "another process: F_TEST and F_TLOCK fail");
    assert_eq!(s::lockf64(fd, 0, 0), 0, "F_ULOCK");
    errno::set_raw(0);
    assert!(fails(s::lockf64(fd, 9, 0) as isize, EINVAL));
    s::close(fd);
}

#[test]
fn sym___lseek() {
    let (_f, fd) = file("__lseek", b"0123456789");
    assert_eq!(s::__lseek(fd, 2, 0), 2);
    assert_eq!(s::__lseek(fd, 3, 1), 5);
    assert_eq!(s::__lseek(fd, -1, 2), 9);
    errno::set_raw(0);
    assert_eq!((s::__lseek(fd, -20, 0), errno::get()), (-1, EINVAL));
    assert_eq!((s::__lseek(fd, 0, 9), errno::get()), (-1, EINVAL));
    let (r, w) = pipe();
    assert_eq!((s::__lseek(r, 0, 0), errno::get()), (-1, ESPIPE));
    for f in [fd, r, w] {
        s::close(f);
    }
}

#[test]
fn sym_lseek() {
    let (_f, fd) = file("lseek", b"0123456789");
    assert_eq!(s::lseek(fd, 2, 0), 2);
    assert_eq!(s::lseek(fd, 3, 1), 5);
    assert_eq!(s::lseek(fd, -1, 2), 9);
    errno::set_raw(0);
    assert_eq!((s::lseek(fd, -20, 0), errno::get()), (-1, EINVAL));
    assert_eq!((s::lseek(fd, 0, 9), errno::get()), (-1, EINVAL));
    let (r, w) = pipe();
    assert_eq!((s::lseek(r, 0, 0), errno::get()), (-1, ESPIPE));
    for f in [fd, r, w] {
        s::close(f);
    }
}

#[test]
fn sym_lseek64() {
    let (_f, fd) = file("lseek64", b"0123456789");
    assert_eq!(s::lseek64(fd, 2, 0), 2);
    assert_eq!(s::lseek64(fd, 3, 1), 5);
    assert_eq!(s::lseek64(fd, -1, 2), 9);
    errno::set_raw(0);
    assert_eq!((s::lseek64(fd, -20, 0), errno::get()), (-1, EINVAL));
    assert_eq!((s::lseek64(fd, 0, 9), errno::get()), (-1, EINVAL));
    let (r, w) = pipe();
    assert_eq!((s::lseek64(r, 0, 0), errno::get()), (-1, ESPIPE));
    for f in [fd, r, w] {
        s::close(f);
    }
}

#[test]
fn sym_madvise() {
    let p = anon(4096);
    liella_boundary::posix::s04_strings::memset(p, 7, 4096);
    assert_eq!(s::madvise(p, 4096, 4), 0, "MADV_DONTNEED");
    let mut b = [1u8; 1];
    liella_boundary::posix::s04_strings::memcpy(b.as_mut_ptr().cast(), p, 1);
    assert_eq!(b[0], 0, "anonymous contents discarded");
    s::munmap(p, 4096);
}

#[test]
fn sym_memfd_create() {
    let fd = s::memfd_create(c"liella".as_ptr(), 1);
    assert!(fd >= 0);
    assert_eq!(s::write(fd, b"anon".as_ptr().cast(), 4), 4);
    assert_eq!(s::lseek(fd, 0, 2), 4);
    errno::set_raw(0);
    assert!(fails(s::memfd_create(c"x".as_ptr(), 0x7000_0000) as isize, EINVAL));
    s::close(fd);
}

#[test]
fn sym_mincore() {
    let p = anon(8192);
    liella_boundary::posix::s04_strings::memset(p, 1, 4096);
    let mut v = [0u8; 2];
    assert_eq!(s::mincore(p, 8192, v.as_mut_ptr()), 0);
    assert_eq!(v[0] & 1, 1, "the touched page is resident");
    s::munmap(p, 8192);
}

#[test]
fn sym_mlock() {
    let p = anon(4096);
    let r = s::mlock(p, 4096);
    assert!(r == 0 || [1, 12, EAGAIN].contains(&errno::get()), "within RLIMIT_MEMLOCK, or refused");
    assert_eq!(s::munlock(p, 4096), 0);
    s::munmap(p, 4096);
}

#[test]
fn sym_mlock2() {
    let p = anon(4096);
    let r = s::mlock2(p, 4096, 1);
    assert!(r == 0 || [1, 12, EAGAIN].contains(&errno::get()), "MLOCK_ONFAULT");
    errno::set_raw(0);
    assert!(fails(s::mlock2(p, 4096, 0x100) as isize, EINVAL));
    s::munlock(p, 4096);
    s::munmap(p, 4096);
}

#[test]
fn sym_mlockall() {
    if probe().as_deref() == Some("all") {
        let r = s::mlockall(2);
        let ok = r == 0 || [1, 12, EAGAIN].contains(&errno::get());
        s::munlockall();
        CSys.exit_group(if ok { 0 } else { 1 });
    }
    assert_eq!(run_probe("sym_mlockall", "all").status.code(), Some(0));
}

#[test]
fn sym_mmap() {
    let p = s::mmap(null_mut(), 8192, 3, 0x22, -1, 0);
    assert_ne!(p as isize, -1);
    liella_boundary::posix::s04_strings::memset(p, 0x5A, 8192);
    let (_f, fd) = file("mmap", b"file contents");
    let f = s::mmap(null_mut(), 4096, 1, 1, fd, 0);
    let mut b = [0u8; 4];
    liella_boundary::posix::s04_strings::memcpy(b.as_mut_ptr().cast(), f, 4);
    assert_eq!(&b, b"file");
    errno::set_raw(0);
    assert_eq!(s::mmap(null_mut(), 4096, 1, 0x20, -1, 0) as isize, -1, "MAP_FAILED, not NULL (neither MAP_PRIVATE nor MAP_SHARED)");
    assert_eq!(errno::get(), EINVAL);
    s::munmap(p, 8192);
    s::munmap(f, 4096);
    s::close(fd);
}

#[test]
fn sym_mmap64() {
    let p = s::mmap64(null_mut(), 8192, 3, 0x22, -1, 0);
    assert_ne!(p as isize, -1);
    liella_boundary::posix::s04_strings::memset(p, 0x5A, 8192);
    let (_f, fd) = file("mmap64", b"file contents");
    let f = s::mmap64(null_mut(), 4096, 1, 1, fd, 0);
    let mut b = [0u8; 4];
    liella_boundary::posix::s04_strings::memcpy(b.as_mut_ptr().cast(), f, 4);
    assert_eq!(&b, b"file");
    errno::set_raw(0);
    assert_eq!(s::mmap64(null_mut(), 4096, 1, 0x20, -1, 0) as isize, -1, "MAP_FAILED, not NULL (neither MAP_PRIVATE nor MAP_SHARED)");
    assert_eq!(errno::get(), EINVAL);
    s::munmap(p, 8192);
    s::munmap(f, 4096);
    s::close(fd);
}

#[test]
fn sym_mprotect() {
    let p = anon(4096);
    assert_eq!(s::mprotect(p, 4096, 1), 0);
    errno::set_raw(0);
    assert!(fails(s::mprotect(p.cast::<u8>().wrapping_add(1).cast(), 4096, 1) as isize, EINVAL));
    s::munmap(p, 4096);
}

#[test]
fn sym_mremap() {
    let p = anon(4096);
    liella_boundary::posix::s04_strings::memset(p, 3, 4096);
    let g = s::liella_mremap5(p, 4096, 16384, 1, null_mut());
    assert_ne!(g as isize, -1);
    let mut b = [0u8; 1];
    liella_boundary::posix::s04_strings::memcpy(b.as_mut_ptr().cast(), g, 1);
    assert_eq!(b[0], 3, "contents kept");
    errno::set_raw(0);
    assert_eq!(s::liella_mremap5(g, 16384, 32768, 0x8, null_mut()) as isize, -1, "MREMAP_FIXED without MAYMOVE");
    s::munmap(g, 16384);
    conformance(&["s11_variadic"]);
}

#[test]
fn sym_msync() {
    let fd = mem_file(c"msync", &[0u8; 4096]);
    let m = s::mmap(null_mut(), 4096, 3, 1, fd, 0);
    liella_boundary::posix::s04_strings::memset(m, 9, 1);
    assert_eq!(s::msync(m, 4096, 4), 0, "MS_SYNC");
    errno::set_raw(0);
    assert!(fails(s::msync(m, 4096, 5) as isize, EINVAL), "MS_SYNC with MS_ASYNC");
    s::munmap(m, 4096);
    s::close(fd);
}

#[test]
fn sym_munlock() {
    let p = anon(4096);
    let r = s::mlock(p, 4096);
    assert!(r == 0 || [1, 12, EAGAIN].contains(&errno::get()), "within RLIMIT_MEMLOCK, or refused");
    assert_eq!(s::munlock(p, 4096), 0);
    s::munmap(p, 4096);
}

#[test]
fn sym_munlockall() {
    if probe().as_deref() == Some("all") {
        let r = s::munlockall();
        let ok = r == 0 || [1, 12, EAGAIN].contains(&errno::get());
        s::munlockall();
        CSys.exit_group(if ok { 0 } else { 1 });
    }
    assert_eq!(run_probe("sym_munlockall", "all").status.code(), Some(0));
}

#[test]
fn sym_munmap() {
    let p = anon(8192);
    assert_eq!(s::munmap(p, 8192), 0);
    errno::set_raw(0);
    assert!(fails(s::munmap(p.cast::<u8>().wrapping_add(1).cast(), 4096) as isize, EINVAL), "unaligned");
}

#[test]
fn sym_name_to_handle_at() {
    let (p, fd) = file("handle", b"x");
    s::close(fd);
    // A struct file_handle with no room: EOVERFLOW, and the size needed is reported.
    let mut h = vec![0u32; 2 + 32];
    let mut mount_id = 0;
    errno::set_raw(0);
    let r = s::name_to_handle_at(AT_FDCWD, p.as_ptr(), h.as_mut_ptr().cast(), &raw mut mount_id, 0);
    if r == -1 && errno::get() == 95 {
        return; // EOPNOTSUPP: the file system has no handles
    }
    assert!(fails(r as isize, EOVERFLOW), "{}", errno::get());
    assert!(h[0] > 0 && h[0] <= 128);
    assert_eq!(s::name_to_handle_at(AT_FDCWD, p.as_ptr(), h.as_mut_ptr().cast(), &raw mut mount_id, 0), 0);
}

#[test]
fn sym___open() {
    // A C variadic alias of open (the same adapter, conformance s11_variadic).
    conformance(&["s11_variadic"]);
}

#[test]
fn sym_open() {
    // The fixed-arity target of the variadic entry; C varargs: conformance s11_variadic.
    let p = tmp("open");
    let fd = s::liella_open3(p.as_ptr(), O_RDWR | O_CREAT | O_EXCL, 0o600);
    assert!(fd >= 0);
    errno::set_raw(0);
    assert!(fails(s::liella_open3(p.as_ptr(), O_RDWR | O_CREAT | O_EXCL, 0o600) as isize, EEXIST));
    assert!(fails(s::liella_open3(tmp("absent").as_ptr(), O_RDONLY, 0) as isize, ENOENT));
    assert!(fails(s::liella_open3(null(), O_RDONLY, 0) as isize, 14), "NULL path: EFAULT");
    s::close(fd);
    conformance(&["s11_variadic"]);
}

#[test]
fn sym___open64() {
    // A C variadic alias of open (the same adapter, conformance s11_variadic).
    conformance(&["s11_variadic"]);
}

#[test]
fn sym_open64() {
    // A C variadic alias of open (the same adapter, conformance s11_variadic).
    conformance(&["s11_variadic"]);
}

#[test]
fn sym___open64_2() {
    let (p, fd0) = file("__open64_2", b"x");
    s::close(fd0);
    if probe().as_deref() == Some("nomode") {
        liella_boundary::selftest::expect_crash();
        s::__open64_2(p.as_ptr(), O_RDWR | O_CREAT);
        return;
    }
    let fd = s::__open64_2(p.as_ptr(), O_RDONLY);
    assert!(fd >= 0);
    s::close(fd);
    let o = run_probe("sym___open64_2", "nomode");
    assert_eq!(o.status.signal(), Some(6), "O_CREAT without a mode aborts");
    assert!(String::from_utf8_lossy(&o.stderr).contains("*** invalid open call: O_CREAT or O_TMPFILE without mode ***: terminated"));
}

#[test]
fn sym___open_2() {
    let (p, fd0) = file("__open_2", b"x");
    s::close(fd0);
    if probe().as_deref() == Some("nomode") {
        liella_boundary::selftest::expect_crash();
        s::__open_2(p.as_ptr(), O_RDWR | O_CREAT);
        return;
    }
    let fd = s::__open_2(p.as_ptr(), O_RDONLY);
    assert!(fd >= 0);
    s::close(fd);
    let o = run_probe("sym___open_2", "nomode");
    assert_eq!(o.status.signal(), Some(6), "O_CREAT without a mode aborts");
    assert!(String::from_utf8_lossy(&o.stderr).contains("*** invalid open call: O_CREAT or O_TMPFILE without mode ***: terminated"));
}

#[test]
fn sym_open_by_handle_at() {
    // Needs CAP_DAC_READ_SEARCH: an unprivileged caller gets EPERM; a bad mount fd EBADF.
    let mut h = vec![0u32; 2 + 32];
    h[0] = 8;
    errno::set_raw(0);
    let r = s::open_by_handle_at(-1, h.as_mut_ptr().cast(), 0);
    assert_eq!(r, -1);
    assert!([EBADF, 1, 95].contains(&errno::get()), "{}", errno::get());
}

#[test]
fn sym___openat64_2() {
    let (p, fd0) = file("__openat64_2", b"x");
    s::close(fd0);
    if probe().as_deref() == Some("nomode") {
        liella_boundary::selftest::expect_crash();
        s::__openat64_2(AT_FDCWD, p.as_ptr(), O_RDWR | O_CREAT);
        return;
    }
    let fd = s::__openat64_2(AT_FDCWD, p.as_ptr(), O_RDONLY);
    assert!(fd >= 0);
    s::close(fd);
    let o = run_probe("sym___openat64_2", "nomode");
    assert_eq!(o.status.signal(), Some(6), "O_CREAT without a mode aborts");
    assert!(String::from_utf8_lossy(&o.stderr).contains("*** invalid open call: O_CREAT or O_TMPFILE without mode ***: terminated"));
}

#[test]
fn sym_openat64() {
    conformance(&["s11_variadic"]);
}

#[test]
fn sym___openat_2() {
    let (p, fd0) = file("__openat_2", b"x");
    s::close(fd0);
    if probe().as_deref() == Some("nomode") {
        liella_boundary::selftest::expect_crash();
        s::__openat_2(AT_FDCWD, p.as_ptr(), O_RDWR | O_CREAT);
        return;
    }
    let fd = s::__openat_2(AT_FDCWD, p.as_ptr(), O_RDONLY);
    assert!(fd >= 0);
    s::close(fd);
    let o = run_probe("sym___openat_2", "nomode");
    assert_eq!(o.status.signal(), Some(6), "O_CREAT without a mode aborts");
    assert!(String::from_utf8_lossy(&o.stderr).contains("*** invalid open call: O_CREAT or O_TMPFILE without mode ***: terminated"));
}

#[test]
fn sym_openat() {
    let p = tmp("openat");
    let dir = s::liella_open3(c"/".as_ptr(), O_RDONLY, 0);
    let fd = s::liella_openat4(dir, p.as_ptr(), O_RDWR | O_CREAT, 0o600);
    assert!(fd >= 0, "an absolute path ignores the directory");
    let rel = CString::new(p.to_bytes().strip_prefix(b"/").unwrap()).unwrap();
    let again = s::liella_openat4(dir, rel.as_ptr(), O_RDONLY, 0);
    assert!(again >= 0, "relative to the directory descriptor");
    let cwd = s::liella_openat4(AT_FDCWD, p.as_ptr(), O_RDONLY, 0);
    assert!(cwd >= 0);
    for f in [fd, again, cwd, dir] {
        s::close(f);
    }
    conformance(&["s11_variadic"]);
}

#[test]
fn sym_pkey_alloc() {
    // Protection keys may be absent (CPU or OS): EINVAL/ENOSPC/ENOSYS then.
    let k = s::pkey_alloc(0, 0);
    if k < 0 {
        assert!([EINVAL, 28, 38].contains(&errno::get()));
        return;
    }
    assert_eq!(s::pkey_free(k), 0);
}

#[test]
fn sym_pkey_free() {
    errno::set_raw(0);
    assert_eq!(s::pkey_free(15), -1, "not allocated");
    assert_eq!(errno::get(), EINVAL);
}

#[test]
fn sym_pkey_get() {
    errno::set_raw(0);
    assert_eq!((s::pkey_get(16), errno::get()), (-1, EINVAL), "out of range");
    let r = s::pkey_get(0);
    assert!(r == 0 || (r == -1 && errno::get() == 38), "key 0 is accessible, or no keys (ENOSYS)");
}

#[test]
fn sym_pkey_mprotect() {
    let p = anon(4096);
    let r = s::pkey_mprotect(p, 4096, 3, 0);
    assert!(r == 0 || [EINVAL, 38].contains(&errno::get()), "key 0 is always valid where keys exist");
    s::munmap(p, 4096);
}

#[test]
fn sym_pkey_set() {
    errno::set_raw(0);
    assert_eq!((s::pkey_set(1, 4), errno::get()), (-1, EINVAL), "rights beyond 3");
    let r = s::pkey_set(0, 0);
    assert!(r == 0 || errno::get() == 38);
}

#[test]
fn sym___poll() {
    let (r, w) = pipe();
    let mut fds = [PollFd { fd: w, events: 4, revents: 0 }];
    assert_eq!(s::__poll(fds.as_mut_ptr().cast(), 1, 100), 1, "POLLOUT");
    s::close(r);
    s::close(w);
}

#[test]
fn sym_poll() {
    let (r, w) = pipe();
    let mut fds = [PollFd { fd: r, events: 1, revents: 0 }, PollFd { fd: -1, events: 1, revents: 7 }];
    assert_eq!(s::poll(fds.as_mut_ptr().cast(), 2, 0), 0, "timeout");
    s::write(w, b"x".as_ptr().cast(), 1);
    assert_eq!(s::poll(fds.as_mut_ptr().cast(), 2, -1), 1);
    assert_eq!(fds[0].revents, 1, "POLLIN");
    assert_eq!(fds[1].revents, 0, "a negative fd is ignored");
    s::close(r);
    s::close(w);
}

#[test]
fn sym___poll_chk() {
    let (r, w) = pipe();
    let mut fds = [PollFd { fd: w, events: 4, revents: 0 }];
    if probe().as_deref() == Some("over") {
        liella_boundary::selftest::expect_crash();
        s::__poll_chk(fds.as_mut_ptr().cast(), 2, 0, 8);
        return;
    }
    assert_eq!(s::__poll_chk(fds.as_mut_ptr().cast(), 1, 0, 8), 1);
    assert_eq!(run_probe("sym___poll_chk", "over").status.signal(), Some(6));
    s::close(r);
    s::close(w);
}

#[test]
fn sym_posix_fadvise() {
    let (_f, fd) = file("posix_fadvise", b"data");
    assert_eq!(s::posix_fadvise(fd, 0, 0, 2), 0);
    errno::set_raw(0);
    assert_eq!(s::posix_fadvise(-1, 0, 0, 0), EBADF, "the error number is returned");
    assert_eq!(errno::get(), 0, "errno is not set");
    assert_eq!(s::posix_fadvise(fd, 0, 0, 99), EINVAL);
    s::close(fd);
}

#[test]
fn sym_posix_fadvise64() {
    let (_f, fd) = file("posix_fadvise64", b"data");
    assert_eq!(s::posix_fadvise64(fd, 0, 0, 2), 0);
    errno::set_raw(0);
    assert_eq!(s::posix_fadvise64(-1, 0, 0, 0), EBADF, "the error number is returned");
    assert_eq!(errno::get(), 0, "errno is not set");
    assert_eq!(s::posix_fadvise64(fd, 0, 0, 99), EINVAL);
    s::close(fd);
}

#[test]
fn sym_posix_fallocate() {
    let (_f, fd) = file("posix_fallocate", b"ab");
    assert_eq!(s::posix_fallocate(fd, 0, 10000), 0);
    assert_eq!(s::lseek(fd, 0, 2), 10000, "extended");
    let mut b = [9u8; 2];
    s::pread(fd, b.as_mut_ptr().cast(), 2, 0);
    assert_eq!(&b, b"ab", "existing data untouched");
    assert_eq!(s::posix_fallocate(fd, 0, 0), EINVAL, "returned, not in errno");
    s::close(fd);
}

#[test]
fn sym_posix_fallocate64() {
    let (_f, fd) = file("posix_fallocate64", b"ab");
    assert_eq!(s::posix_fallocate64(fd, 0, 10000), 0);
    assert_eq!(s::lseek(fd, 0, 2), 10000, "extended");
    let mut b = [9u8; 2];
    s::pread(fd, b.as_mut_ptr().cast(), 2, 0);
    assert_eq!(&b, b"ab", "existing data untouched");
    assert_eq!(s::posix_fallocate64(fd, 0, 0), EINVAL, "returned, not in errno");
    s::close(fd);
}

#[test]
fn sym_posix_madvise() {
    let p = anon(4096);
    liella_boundary::posix::s04_strings::memset(p, 7, 4096);
    assert_eq!(s::posix_madvise(p, 4096, 4), 0, "POSIX_MADV_DONTNEED");
    let mut b = [0u8; 1];
    liella_boundary::posix::s04_strings::memcpy(b.as_mut_ptr().cast(), p, 1);
    assert_eq!(b[0], 7, "never discards");
    assert_eq!(s::posix_madvise(p, 4096, 2), 0);
    assert_eq!(s::posix_madvise(p, 4096, 77), EINVAL, "returned directly");
    s::munmap(p, 4096);
}

#[test]
fn sym___ppoll_chk() {
    let (r, w) = pipe();
    let mut fds = [PollFd { fd: w, events: 4, revents: 0 }];
    assert_eq!(s::__ppoll_chk(fds.as_mut_ptr().cast(), 1, None, None, 8), 1);
    s::close(r);
    s::close(w);
}

#[test]
fn sym_ppoll() {
    let (r, w) = pipe();
    let mut fds = [PollFd { fd: r, events: 1, revents: 0 }];
    let ts = Timespec { sec: 0, nsec: 1000 };
    assert_eq!(s::ppoll(fds.as_mut_ptr().cast(), 1, Some(&ts), Some(&sigset(0))), 0);
    assert_eq!(ts.nsec, 1000, "not modified");
    s::close(r);
    s::close(w);
}

#[test]
fn sym_pread() {
    let (_f, fd) = file("pread", b"0123456789");
    let mut b = [0u8; 3];
    assert_eq!(s::pread(fd, b.as_mut_ptr().cast(), 3, 4), 3);
    assert_eq!(&b, b"456");
    assert_eq!(s::lseek(fd, 0, 1), 10, "the offset does not move");
    s::close(fd);
}

#[test]
fn sym___pread64() {
    let (_f, fd) = file("__pread64", b"0123456789");
    let mut b = [0u8; 3];
    assert_eq!(s::__pread64(fd, b.as_mut_ptr().cast(), 3, 4), 3);
    assert_eq!(&b, b"456");
    assert_eq!(s::lseek(fd, 0, 1), 10, "the offset does not move");
    s::close(fd);
}

#[test]
fn sym_pread64() {
    let (_f, fd) = file("pread64", b"0123456789");
    let mut b = [0u8; 3];
    assert_eq!(s::pread64(fd, b.as_mut_ptr().cast(), 3, 4), 3);
    assert_eq!(&b, b"456");
    assert_eq!(s::lseek(fd, 0, 1), 10, "the offset does not move");
    s::close(fd);
}

#[test]
fn sym___pread64_chk() {
    let (_f, fd) = file("__pread64_chk", b"abcd");
    if probe().as_deref() == Some("over") {
        let mut b = [0u8; 2];
        liella_boundary::selftest::expect_crash();
        s::__pread64_chk(fd, b.as_mut_ptr().cast(), 4, 0, 2);
        return;
    }
    let mut b = [0u8; 2];
    assert_eq!(s::__pread64_chk(fd, b.as_mut_ptr().cast(), 2, 2, 2), 2);
    assert_eq!(&b, b"cd");
    assert_eq!(run_probe("sym___pread64_chk", "over").status.signal(), Some(6));
    s::close(fd);
}

#[test]
fn sym___pread_chk() {
    let (_f, fd) = file("__pread_chk", b"abcd");
    if probe().as_deref() == Some("over") {
        let mut b = [0u8; 2];
        liella_boundary::selftest::expect_crash();
        s::__pread_chk(fd, b.as_mut_ptr().cast(), 4, 0, 2);
        return;
    }
    let mut b = [0u8; 2];
    assert_eq!(s::__pread_chk(fd, b.as_mut_ptr().cast(), 2, 2, 2), 2);
    assert_eq!(&b, b"cd");
    assert_eq!(run_probe("sym___pread_chk", "over").status.signal(), Some(6));
    s::close(fd);
}

#[test]
fn sym_preadv2() {
    let (_f, fd) = file("preadv2", b"abcdef");
    s::lseek(fd, 1, 0);
    let mut a = [0u8; 2];
    let mut v = [IoVecMut::new(&mut a)];
    assert_eq!(s::preadv2(fd, v.as_mut_ptr().cast(), 1, -1, 0), 2, "offset -1: the current offset");
    assert_eq!(&a, b"bc");
    assert_eq!(s::lseek(fd, 0, 1), 3, "and it advances");
    s::close(fd);
}

#[test]
fn sym_preadv64() {
    let (_f, fd) = file("preadv64", b"abcdef");
    let mut a = [0u8; 2];
    let mut v = [IoVecMut::new(&mut a)];
    assert_eq!(s::preadv64(fd, v.as_mut_ptr().cast(), 1, 3), 2);
    assert_eq!(&a, b"de");
    assert_eq!(s::lseek(fd, 0, 1), 6);
    s::close(fd);
}

#[test]
fn sym_preadv64v2() {
    let (_f, fd) = file("preadv64v2", b"abcdef");
    s::lseek(fd, 1, 0);
    let mut a = [0u8; 2];
    let mut v = [IoVecMut::new(&mut a)];
    assert_eq!(s::preadv64v2(fd, v.as_mut_ptr().cast(), 1, -1, 0), 2, "offset -1: the current offset");
    assert_eq!(&a, b"bc");
    assert_eq!(s::lseek(fd, 0, 1), 3, "and it advances");
    s::close(fd);
}

#[test]
fn sym_preadv() {
    let (_f, fd) = file("preadv", b"abcdef");
    let mut a = [0u8; 2];
    let mut v = [IoVecMut::new(&mut a)];
    assert_eq!(s::preadv(fd, v.as_mut_ptr().cast(), 1, 3), 2);
    assert_eq!(&a, b"de");
    assert_eq!(s::lseek(fd, 0, 1), 6);
    s::close(fd);
}

#[test]
fn sym_process_madvise() {
    let p = anon(8192);
    let pidfd = CSys.pidfd_open(CSys.getpid(), 0).unwrap();
    let v = [RemoteIoVec { base: p as usize, len: 8192 }];
    let r = s::process_madvise(pidfd.raw(), v.as_ptr().cast(), 1, 20, 0);
    assert!(r == 8192 || [1, EINVAL, 38].contains(&errno::get()), "MADV_COLD on our own memory: {}", errno::get());
    errno::set_raw(0);
    assert!(fails(s::process_madvise(pidfd.raw(), v.as_ptr().cast(), 1, 4, 0), EINVAL), "advice the call does not take");
    s::close(pidfd.raw());
    s::munmap(p, 8192);
}

#[test]
fn sym_process_mrelease() {
    let pidfd = CSys.pidfd_open(CSys.getpid(), 0).unwrap();
    errno::set_raw(0);
    assert_eq!(s::process_mrelease(pidfd.raw(), 0), -1, "the process is not exiting");
    assert!([EINVAL, 38].contains(&errno::get()));
    s::close(pidfd.raw());
}

#[test]
fn sym_process_vm_readv() {
    let src = *b"remote";
    let mut dst = [0u8; 6];
    let mut local = [IoVecMut::new(&mut dst)];
    let remote = [RemoteIoVec { base: src.as_ptr() as usize, len: 6 }];
    let r = s::process_vm_readv(CSys.getpid(), local.as_mut_ptr().cast(), 1, remote.as_ptr().cast(), 1, 0);
    if r == -1 && errno::get() == 1 {
        return; // ptrace restrictions
    }
    assert_eq!(r, 6);
    assert_eq!(&dst, b"remote");
}

#[test]
fn sym_process_vm_writev() {
    let mut dst = [0u8; 5];
    let local = [IoVec::new(b"wrote")];
    let remote = [RemoteIoVec { base: dst.as_mut_ptr() as usize, len: 5 }];
    let r = s::process_vm_writev(CSys.getpid(), local.as_ptr().cast(), 1, remote.as_ptr().cast(), 1, 0);
    if r == -1 && errno::get() == 1 {
        return;
    }
    assert_eq!(r, 5);
    assert_eq!(&dst, b"wrote");
}

#[test]
fn sym_pselect() {
    let (r, w) = pipe();
    let mut rs = FdSet::default();
    rs.set(r);
    let ts = Timespec { sec: 0, nsec: 1_000_000 };
    let mask = sigset(0);
    assert_eq!(s::pselect(r + 1, Some(&mut rs), None, None, Some(&ts), Some(&mask)), 0);
    assert_eq!(ts.nsec, 1_000_000, "the caller's timeout is not modified");
    s::write(w, b"x".as_ptr().cast(), 1);
    let mut rs = FdSet::default();
    rs.set(r);
    assert_eq!(s::pselect(r + 1, Some(&mut rs), None, None, None, None), 1);
    s::close(r);
    s::close(w);
}

#[test]
fn sym_pwrite() {
    let (_f, fd) = file("pwrite", b"0123456789");
    assert_eq!(s::pwrite(fd, b"ab".as_ptr().cast(), 2, 1), 2);
    assert_eq!(s::lseek(fd, 0, 1), 10, "the offset does not move");
    let mut b = [0u8; 4];
    s::pread(fd, b.as_mut_ptr().cast(), 4, 0);
    assert_eq!(&b, b"0ab3");
    s::close(fd);
}

#[test]
fn sym___pwrite64() {
    let (_f, fd) = file("__pwrite64", b"0123456789");
    assert_eq!(s::__pwrite64(fd, b"ab".as_ptr().cast(), 2, 1), 2);
    assert_eq!(s::lseek(fd, 0, 1), 10, "the offset does not move");
    let mut b = [0u8; 4];
    s::pread(fd, b.as_mut_ptr().cast(), 4, 0);
    assert_eq!(&b, b"0ab3");
    s::close(fd);
}

#[test]
fn sym_pwrite64() {
    let (_f, fd) = file("pwrite64", b"0123456789");
    assert_eq!(s::pwrite64(fd, b"ab".as_ptr().cast(), 2, 1), 2);
    assert_eq!(s::lseek(fd, 0, 1), 10, "the offset does not move");
    let mut b = [0u8; 4];
    s::pread(fd, b.as_mut_ptr().cast(), 4, 0);
    assert_eq!(&b, b"0ab3");
    s::close(fd);
}

#[test]
fn sym_pwritev2() {
    let (_f, fd) = file("pwritev2", b"abcdef");
    let v = [IoVec::new(b"Z")];
    assert_eq!(s::pwritev2(fd, v.as_ptr().cast(), 1, 0, 0), 1);
    errno::set_raw(0);
    assert!(fails(s::pwritev2(fd, v.as_ptr().cast(), 1, 0, 0x7000_0000), 95), "unknown flags: EOPNOTSUPP");
    s::close(fd);
}

#[test]
fn sym_pwritev64() {
    let (_f, fd) = file("pwritev64", b"abcdef");
    let v = [IoVec::new(b"XY")];
    assert_eq!(s::pwritev64(fd, v.as_ptr().cast(), 1, 1), 2);
    let mut b = [0u8; 6];
    s::pread(fd, b.as_mut_ptr().cast(), 6, 0);
    assert_eq!(&b, b"aXYdef");
    s::close(fd);
}

#[test]
fn sym_pwritev64v2() {
    let (_f, fd) = file("pwritev64v2", b"abcdef");
    let v = [IoVec::new(b"Z")];
    assert_eq!(s::pwritev64v2(fd, v.as_ptr().cast(), 1, 0, 0), 1);
    errno::set_raw(0);
    assert!(fails(s::pwritev64v2(fd, v.as_ptr().cast(), 1, 0, 0x7000_0000), 95), "unknown flags: EOPNOTSUPP");
    s::close(fd);
}

#[test]
fn sym_pwritev() {
    let (_f, fd) = file("pwritev", b"abcdef");
    let v = [IoVec::new(b"XY")];
    assert_eq!(s::pwritev(fd, v.as_ptr().cast(), 1, 1), 2);
    let mut b = [0u8; 6];
    s::pread(fd, b.as_mut_ptr().cast(), 6, 0);
    assert_eq!(&b, b"aXYdef");
    s::close(fd);
}

#[test]
fn sym___read() {
    let (_f, fd) = file("uread", b"abc");
    s::lseek(fd, 0, 0);
    let mut b = [0u8; 3];
    assert_eq!(s::__read(fd, b.as_mut_ptr().cast(), 3), 3);
    assert_eq!(&b, b"abc");
    s::close(fd);
}

#[test]
fn sym_read() {
    let (r, w) = pipe();
    assert_eq!(s::write(w, b"hello".as_ptr().cast(), 5), 5);
    let mut b = [0u8; 16];
    assert_eq!(s::read(r, b.as_mut_ptr().cast(), 16), 5, "short: what the pipe held");
    s::close(w);
    assert_eq!(s::read(r, b.as_mut_ptr().cast(), 16), 0, "end of file");
    errno::set_raw(0);
    assert!(fails(s::read(-1, b.as_mut_ptr().cast(), 1), EBADF));
    s::close(r);
}

#[test]
fn sym___read_chk() {
    let (_f, fd) = file("readchk", b"abcd");
    s::lseek(fd, 0, 0);
    if probe().as_deref() == Some("over") {
        let mut b = [0u8; 2];
        liella_boundary::selftest::expect_crash();
        s::__read_chk(fd, b.as_mut_ptr().cast(), 4, 2);
        return;
    }
    let mut b = [0u8; 4];
    assert_eq!(s::__read_chk(fd, b.as_mut_ptr().cast(), 4, 4), 4);
    let o = run_probe("sym___read_chk", "over");
    assert_eq!(o.status.signal(), Some(6));
    assert!(String::from_utf8_lossy(&o.stderr).contains("*** buffer overflow detected ***"));
    s::close(fd);
}

#[test]
fn sym_readahead() {
    let (_f, fd) = file("ra", b"data");
    assert_eq!(s::readahead(fd, 0, 4), 0);
    s::close(fd);
}

#[test]
fn sym_readv() {
    let (_f, fd) = file("readv", b"abcdefg");
    s::lseek(fd, 0, 0);
    let (mut a, mut b) = ([0u8; 3], [0u8; 4]);
    let mut v = [IoVecMut::new(&mut a), IoVecMut::new(&mut b)];
    assert_eq!(s::readv(fd, v.as_mut_ptr().cast(), 2), 7);
    assert_eq!((&a, &b), (b"abc", b"defg"));
    errno::set_raw(0);
    assert!(fails(s::readv(fd, null_mut(), 1025), EINVAL), "more than IOV_MAX");
    assert!(fails(s::readv(fd, null_mut(), -1), EINVAL));
    s::close(fd);
}

#[test]
fn sym_remap_file_pages() {
    let (_f, fd) = file("rfp", &[0u8; 8192]);
    let m = s::mmap(null_mut(), 8192, 3, 1, fd, 0);
    let r = s::remap_file_pages(m, 4096, 0, 1, 0);
    assert!(r == 0 || [EINVAL, 38, 1].contains(&errno::get()), "deprecated, kernel-emulated");
    s::munmap(m, 8192);
    s::close(fd);
}

#[test]
fn sym___select() {
    let (r, w) = pipe();
    let mut rs = FdSet::default();
    rs.set(r);
    let mut tv = Timeval { sec: 0, usec: 0 };
    assert_eq!(s::__select(r + 1, Some(&mut rs), None, None, Some(&mut tv)), 0, "timeout");
    s::write(w, b"x".as_ptr().cast(), 1);
    let mut rs = FdSet::default();
    rs.set(r);
    let mut tv = Timeval { sec: 5, usec: 0 };
    assert_eq!(s::__select(r + 1, Some(&mut rs), None, None, Some(&mut tv)), 1);
    assert!(rs.bits[(r / 64) as usize] >> (r % 64) & 1 == 1, "rewritten with the ready set");
    assert!(tv.sec <= 5, "the remaining time");
    errno::set_raw(0);
    let mut bad = Timeval { sec: 0, usec: 2_000_000 };
    assert_eq!((s::__select(0, None, None, None, Some(&mut bad)), errno::get()), (-1, EINVAL));
    s::close(r);
    s::close(w);
}

#[test]
fn sym_select() {
    let (r, w) = pipe();
    let mut rs = FdSet::default();
    rs.set(r);
    let mut tv = Timeval { sec: 0, usec: 0 };
    assert_eq!(s::select(r + 1, Some(&mut rs), None, None, Some(&mut tv)), 0, "timeout");
    s::write(w, b"x".as_ptr().cast(), 1);
    let mut rs = FdSet::default();
    rs.set(r);
    let mut tv = Timeval { sec: 5, usec: 0 };
    assert_eq!(s::select(r + 1, Some(&mut rs), None, None, Some(&mut tv)), 1);
    assert!(rs.bits[(r / 64) as usize] >> (r % 64) & 1 == 1, "rewritten with the ready set");
    assert!(tv.sec <= 5, "the remaining time");
    errno::set_raw(0);
    let mut bad = Timeval { sec: 0, usec: 2_000_000 };
    assert_eq!((s::select(0, None, None, None, Some(&mut bad)), errno::get()), (-1, EINVAL));
    s::close(r);
    s::close(w);
}

/// Linux 5.10 refuses `sendfile` from a regular file into a pipe (`EINVAL`; the reference on
/// that kernel gives the same, probed 2026-10-05); later kernels transfer. Whether this kernel
/// refuses it: one byte of `fd` into a pipe of its own.
fn sendfile_to_pipe_refused(fd: i32) -> bool {
    let (r, w) = pipe();
    errno::set_raw(0);
    let n = s::sendfile(w, fd, Some(&mut 0), 1);
    let e = errno::get();
    for f in [r, w] {
        s::close(f);
    }
    if n == -1 && e == EINVAL && kernel::before(5, 12) {
        kernel::note("sendfile into a pipe", "5.11/5.12");
        return true;
    }
    assert_eq!(n, 1, "one byte into a pipe");
    false
}

#[test]
fn sym_sendfile() {
    let (_f, fd) = file("sendfile", b"0123456789");
    let (r, w) = pipe();
    let mut off = 2i64;
    if sendfile_to_pipe_refused(fd) {
        for f in [fd, r, w] {
            s::close(f);
        }
        return;
    }
    assert_eq!(s::sendfile(w, fd, Some(&mut off), 5), 5);
    assert_eq!(off, 7, "the offset variable advances");
    let mut b = [0u8; 5];
    s::read(r, b.as_mut_ptr().cast(), 5);
    assert_eq!(&b, b"23456");
    for f in [fd, r, w] {
        s::close(f);
    }
}

#[test]
fn sym_sendfile64() {
    let (_f, fd) = file("sendfile64", b"0123456789");
    let (r, w) = pipe();
    let mut off = 2i64;
    if sendfile_to_pipe_refused(fd) {
        for f in [fd, r, w] {
            s::close(f);
        }
        return;
    }
    assert_eq!(s::sendfile64(w, fd, Some(&mut off), 5), 5);
    assert_eq!(off, 7, "the offset variable advances");
    let mut b = [0u8; 5];
    s::read(r, b.as_mut_ptr().cast(), 5);
    assert_eq!(&b, b"23456");
    for f in [fd, r, w] {
        s::close(f);
    }
}

#[test]
fn sym_splice() {
    let (_f, fd) = file("splice", b"spliced");
    let (r, w) = pipe();
    let mut off = 0i64;
    assert_eq!(s::splice(fd, Some(&mut off), w, None, 7, 0), 7);
    errno::set_raw(0);
    assert!(fails(s::splice(fd, None, fd, None, 1, 0), EINVAL), "one end must be a pipe");
    for f in [fd, r, w] {
        s::close(f);
    }
}

#[test]
fn sym_sync() {
    if sync_trap::child() {
        s::sync();
        println!("{}", sync_trap::RETURNED);
        return;
    }
    // SYS_sync issued once and success returned, under a seccomp trap instead of the machine-wide
    // writeback (GATE-FSYNC-2); no test makes a real sync().
    sync_trap::check("sym_sync");
}

#[test]
fn sym_sync_file_range() {
    let fd = mem_file(c"sfr", b"data");
    assert_eq!(s::sync_file_range(fd, 0, 4, 2), 0);
    errno::set_raw(0);
    assert!(fails(s::sync_file_range(fd, 0, 4, 0x100) as isize, EINVAL));
    s::close(fd);
}

#[test]
fn sym_syncfs() {
    let fd = mem_file(c"syncfs", b"data");
    assert_eq!(s::syncfs(fd), 0);
    errno::set_raw(0);
    assert!(fails(s::syncfs(-1) as isize, EBADF));
    s::close(fd);
}

#[test]
fn sym_tee() {
    let (r1, w1) = pipe();
    let (r2, w2) = pipe();
    s::write(w1, b"dup".as_ptr().cast(), 3);
    assert_eq!(s::tee(r1, w2, 3, 0), 3);
    let mut b = [0u8; 3];
    assert_eq!(s::read(r1, b.as_mut_ptr().cast(), 3), 3, "tee leaves the source");
    assert_eq!(s::read(r2, b.as_mut_ptr().cast(), 3), 3);
    for f in [r1, w1, r2, w2] {
        s::close(f);
    }
}

#[test]
fn sym_vmsplice() {
    let (r, w) = pipe();
    let mut data = *b"pages";
    let mut v = [IoVecMut::new(&mut data)];
    assert_eq!(s::vmsplice(w, v.as_mut_ptr().cast(), 1, 0), 5);
    let mut b = [0u8; 5];
    s::read(r, b.as_mut_ptr().cast(), 5);
    assert_eq!(&b, b"pages");
    s::close(r);
    s::close(w);
}

#[test]
fn sym___write() {
    let (r, w) = pipe();
    assert_eq!(s::__write(w, b"xy".as_ptr().cast(), 2), 2);
    s::close(r);
    s::close(w);
}

#[test]
fn sym_write() {
    let (r, w) = pipe();
    assert_eq!(CSys.fcntl_int(liella_kernel::Fd::new(w), liella_kernel::ops::FcntlInt::SetFl, i64::from(O_NONBLOCK)).unwrap(), 0);
    let big = vec![7u8; 1 << 20];
    let n = s::write(w, big.as_ptr().cast(), big.len());
    assert!(n > 0 && (n as usize) < big.len(), "short: the pipe's capacity");
    errno::set_raw(0);
    assert!(fails(s::write(w, big.as_ptr().cast(), 1), EAGAIN), "non-blocking and full");
    s::close(r);
    s::close(w);
}

#[test]
fn sym_writev() {
    let (_f, fd) = file("writev", b"");
    let v = [IoVec::new(b"gather "), IoVec::new(b"write")];
    assert_eq!(s::writev(fd, v.as_ptr().cast(), 2), 12);
    let mut b = [0u8; 12];
    s::pread(fd, b.as_mut_ptr().cast(), 12, 0);
    assert_eq!(&b, b"gather write");
    s::close(fd);
}

