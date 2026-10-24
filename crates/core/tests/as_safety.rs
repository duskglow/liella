//! Phase 5's exit criterion (implementation-plan.md §6): the async-signal-safe functions take no
//! lock and allocate nothing. A child holds every lock of the library (the heap's included,
//! `selftest::with_all_locks`) and raises a signal; its handler calls each listed function
//! that is implemented so far. A function that took a lock, or allocated (which takes a heap
//! lock), would block for good: an alarm then ends the child, and the parent names the
//! function from the progress the handler wrote before each call.
//!
//! The list is POSIX.1-2017's async-signal-safe functions (XSH 2.4.3) as far as phases 1–5
//! implement them; `abort`, `_exit` and `_Exit` end the process and are covered by their own
//! tests, and the blocking waits (`pause`, `sigsuspend`, `sigpause`) are left out. The rest
//! (`fork`, `exec*`, `wait*`, the terminal and thread calls) arrive in later phases. The POSIX
//! timer calls (phase 9) run on a `SIGEV_NONE` kernel timer and on a `SIGEV_THREAD` timer (the
//! library's own: a `timerfd` and its helper, #544) made before the locks are taken;
//! `alarm` is left out because the child's own alarm is the harness's watchdog.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::arithmetic_side_effects, missing_docs)]

#[path = "support/timeout.rs"]
mod timeout;
use core::ptr::{null, null_mut};
use core::sync::atomic::{AtomicBool, AtomicI32, AtomicUsize, Ordering};

use liella_boundary::posix::s04_strings as st;
use liella_boundary::posix::s08_numeric as nm;
use liella_boundary::posix::s11_lowio as io;
use liella_boundary::posix::s13_filesystem as fs;
use liella_boundary::posix::s15_sockets as sk;
use liella_boundary::posix::s17_users as us;
use liella_boundary::posix::s18_processes as pr;
use liella_boundary::posix::s20_signals as sg;
use liella_boundary::posix::s23_time as tm;
use liella_boundary::posix::s25_ipc as ip;
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Itimerspec, PollFd, SigEvent, Timespec, Timeval, Tms};

static FILE: AtomicI32 = AtomicI32::new(-1);
static DIR: AtomicI32 = AtomicI32::new(-1);
static NULL_FD: AtomicI32 = AtomicI32::new(-1);
static ZERO_FD: AtomicI32 = AtomicI32::new(-1);
static DGRAM: [AtomicI32; 2] = [AtomicI32::new(-1), AtomicI32::new(-1)];
static LISTENER: AtomicI32 = AtomicI32::new(-1);
/// A `SIGEV_NONE` POSIX timer (its `timer_t`), made during setup.
static TIMER: AtomicUsize = AtomicUsize::new(0);
/// A `SIGEV_THREAD` POSIX timer, disarmed, made during setup (its notification never runs).
static THREAD_TIMER: AtomicUsize = AtomicUsize::new(0);
static DONE: AtomicBool = AtomicBool::new(false);
/// The control's request: a LARGE block (above `MAX_SMALL`, 64 KiB). A small request is served from
/// the calling thread's cache (D-3.12) without any lock once an earlier allocation in the child
/// (std's own, via the exported `malloc`) has given that class a slab, so `malloc(64)` returned
/// at once and the control saw no blocked lock (b21 gates, keke and the 5.10 guest). A large
/// block always needs its own mapping and an entry in the large-block table, taken under the
/// large lock, which `with_all_locks` holds; the child never frees a large block first, so no
/// kept block (D-3.13) can answer it either.
const CONTROL_SIZE: usize = 1 << 20;

/// The negative control: the handler calls `malloc`, which must block on the held heap lock.
static CONTROL: AtomicBool = AtomicBool::new(false);

fn f(a: &AtomicI32) -> i32 {
    a.load(Ordering::Relaxed)
}

/// `struct sockaddr_in` for 127.0.0.1:`port`.
#[repr(C)]
struct In4 {
    family: u16,
    port: u16,
    addr: [u8; 4],
    zero: [u8; 8],
}
static LOOP0: In4 = In4 { family: 2, port: 0, addr: [127, 0, 0, 1], zero: [0; 8] };
static LOOP1: In4 = In4 { family: 2, port: 1u16.to_be(), addr: [127, 0, 0, 1], zero: [0; 8] };

#[repr(C)]
struct Iov {
    base: *mut core::ffi::c_void,
    len: usize,
}
#[repr(C)]
struct Msg {
    name: *mut core::ffi::c_void,
    namelen: u32,
    iov: *mut Iov,
    iovlen: usize,
    control: *mut core::ffi::c_void,
    controllen: usize,
    flags: i32,
}

fn empty_set() -> liella_abi::layout::sigset_t {
    liella_abi::layout::sigset_t { __val: [0; 16] }
}

type Call = (&'static str, fn());

/// The calls, each on arguments set up before the locks were taken.
const CALLS: &[Call] = &[
    // §4 strings
    ("memcpy", || { let (mut d, s) = ([0u8; 8], [1u8; 8]); st::memcpy(d.as_mut_ptr().cast(), s.as_ptr().cast(), 8); }),
    ("memmove", || { let mut d = [1u8; 8]; st::memmove(d.as_mut_ptr().cast(), d.as_ptr().wrapping_add(1).cast(), 4); }),
    ("memset", || { let mut d = [0u8; 8]; st::memset(d.as_mut_ptr().cast(), 7, 8); }),
    ("memcmp", || { st::memcmp(b"ab".as_ptr().cast(), b"ac".as_ptr().cast(), 2); }),
    ("memchr", || { st::memchr(b"abc".as_ptr().cast(), 99, 3); }),
    ("memccpy", || { let mut d = [0u8; 8]; st::memccpy(d.as_mut_ptr().cast(), b"ab:cd".as_ptr().cast(), 58, 5); }),
    ("strlen", || { st::strlen(c"hello".as_ptr()); }),
    ("strnlen", || { st::strnlen(c"hello".as_ptr(), 3); }),
    ("strcpy", || { let mut d = [0i8; 8]; st::strcpy(d.as_mut_ptr(), c"ab".as_ptr()); }),
    ("stpcpy", || { let mut d = [0i8; 8]; st::stpcpy(d.as_mut_ptr(), c"ab".as_ptr()); }),
    ("strncpy", || { let mut d = [0i8; 8]; st::strncpy(d.as_mut_ptr(), c"ab".as_ptr(), 8); }),
    ("stpncpy", || { let mut d = [0i8; 8]; st::stpncpy(d.as_mut_ptr(), c"ab".as_ptr(), 8); }),
    ("strcat", || { let mut d = [0i8; 8]; st::strcat(d.as_mut_ptr(), c"ab".as_ptr()); }),
    ("strncat", || { let mut d = [0i8; 8]; st::strncat(d.as_mut_ptr(), c"ab".as_ptr(), 1); }),
    ("strcmp", || { st::strcmp(c"a".as_ptr(), c"b".as_ptr()); }),
    ("strncmp", || { st::strncmp(c"a".as_ptr(), c"b".as_ptr(), 1); }),
    ("strchr", || { st::strchr(c"abc".as_ptr(), 98); }),
    ("strrchr", || { st::strrchr(c"abcb".as_ptr(), 98); }),
    ("strspn", || { st::strspn(c"aab".as_ptr(), c"a".as_ptr()); }),
    ("strcspn", || { st::strcspn(c"aab".as_ptr(), c"b".as_ptr()); }),
    ("strpbrk", || { st::strpbrk(c"aab".as_ptr(), c"b".as_ptr()); }),
    ("strstr", || { st::strstr(c"hello".as_ptr(), c"ll".as_ptr()); }),
    ("strtok_r", || { let mut s = *b"a,b\0"; let mut save = null_mut(); st::strtok_r(s.as_mut_ptr().cast(), c",".as_ptr(), Some(&mut save)); }),
    ("wcslen", || { st::wcslen([97, 0].as_ptr()); }),
    ("wcsnlen", || { st::wcsnlen([97, 0].as_ptr(), 1); }),
    ("wcscpy", || { let mut d = [0i32; 4]; st::wcscpy(d.as_mut_ptr(), [97, 0].as_ptr()); }),
    ("wcpcpy", || { let mut d = [0i32; 4]; st::wcpcpy(d.as_mut_ptr(), [97, 0].as_ptr()); }),
    ("wcsncpy", || { let mut d = [0i32; 4]; st::wcsncpy(d.as_mut_ptr(), [97, 0].as_ptr(), 4); }),
    ("wcpncpy", || { let mut d = [0i32; 4]; st::wcpncpy(d.as_mut_ptr(), [97, 0].as_ptr(), 4); }),
    ("wcscat", || { let mut d = [0i32; 4]; st::wcscat(d.as_mut_ptr(), [97, 0].as_ptr()); }),
    ("wcsncat", || { let mut d = [0i32; 4]; st::wcsncat(d.as_mut_ptr(), [97, 0].as_ptr(), 1); }),
    ("wcscmp", || { st::wcscmp([97, 0].as_ptr(), [98, 0].as_ptr()); }),
    ("wcsncmp", || { st::wcsncmp([97, 0].as_ptr(), [98, 0].as_ptr(), 1); }),
    ("wcschr", || { st::wcschr([97, 98, 0].as_ptr(), 98); }),
    ("wcsrchr", || { st::wcsrchr([97, 98, 0].as_ptr(), 98); }),
    ("wcsspn", || { st::wcsspn([97, 98, 0].as_ptr(), [97, 0].as_ptr()); }),
    ("wcscspn", || { st::wcscspn([97, 98, 0].as_ptr(), [98, 0].as_ptr()); }),
    ("wcspbrk", || { st::wcspbrk([97, 98, 0].as_ptr(), [98, 0].as_ptr()); }),
    ("wcsstr", || { st::wcsstr([97, 98, 0].as_ptr(), [98, 0].as_ptr()); }),
    ("wcstok", || { let mut s = [97, 44, 98, 0]; let mut save = null_mut(); st::wcstok(s.as_mut_ptr(), [44, 0].as_ptr(), Some(&mut save)); }),
    ("ffs", || { nm::ffs(8); }),
    // §11 descriptors
    ("write", || { io::write(f(&NULL_FD), b"x".as_ptr().cast(), 1); }),
    ("read", || { let mut b = [0u8; 4]; io::read(f(&ZERO_FD), b.as_mut_ptr().cast(), 4); }),
    ("open", || { let d = io::liella_open3(c"f".as_ptr(), 2, 0); io::close(d); }),
    ("openat", || { let d = io::liella_openat4(f(&DIR), c"f".as_ptr(), 0, 0); io::close(d); }),
    ("creat", || { let d = io::creat(c"c".as_ptr(), 0o600); io::close(d); fs::unlink(c"c".as_ptr()); }),
    ("close", || { let d = io::dup(f(&NULL_FD)); io::close(d); }),
    ("dup", || { let d = io::dup(f(&NULL_FD)); io::close(d); }),
    ("dup2", || { io::dup2(f(&NULL_FD), 950); io::close(950); }),
    ("fcntl", || { io::liella_fcntl3(f(&FILE), 1, 0); }),
    ("lseek", || { io::lseek(f(&FILE), 0, 0); }),
    ("fsync", || { io::fsync(f(&FILE)); }),
    ("fdatasync", || { io::fdatasync(f(&FILE)); }),
    ("pipe", || { let mut p = [-1; 2]; ip::pipe(Some(&mut p)); io::close(p[0]); io::close(p[1]); }),
    ("poll", || { let mut p = [PollFd { fd: f(&NULL_FD), events: 4, revents: 0 }]; io::poll(p.as_mut_ptr().cast(), 1, 0); }),
    ("select", || { io::select(0, None, None, None, Some(&mut Timeval { sec: 0, usec: 0 })); }),
    ("pselect", || { io::pselect(0, None, None, None, Some(&Timespec { sec: 0, nsec: 0 }), None); }),
    // §13 file system
    ("access", || { fs::access(c"f".as_ptr(), 4); }),
    ("faccessat", || { fs::faccessat(f(&DIR), c"f".as_ptr(), 4, 0x200); }),
    ("chmod", || { fs::chmod(c"f".as_ptr(), 0o600); }),
    ("fchmod", || { fs::fchmod(f(&FILE), 0o600); }),
    ("fchmodat", || { fs::fchmodat(f(&DIR), c"f".as_ptr(), 0o600, 0); }),
    ("chown", || { fs::chown(c"f".as_ptr(), u32::MAX, u32::MAX); }),
    ("fchown", || { fs::fchown(f(&FILE), u32::MAX, u32::MAX); }),
    ("fchownat", || { fs::fchownat(f(&DIR), c"f".as_ptr(), u32::MAX, u32::MAX, 0); }),
    ("link", || { fs::link(c"f".as_ptr(), c"h".as_ptr()); fs::unlink(c"h".as_ptr()); }),
    ("linkat", || { fs::linkat(f(&DIR), c"f".as_ptr(), f(&DIR), c"h".as_ptr(), 0); fs::unlinkat(f(&DIR), c"h".as_ptr(), 0); }),
    ("unlink", || { fs::mkfifo(c"u".as_ptr(), 0o600); fs::unlink(c"u".as_ptr()); }),
    ("unlinkat", || { fs::mkfifo(c"u".as_ptr(), 0o600); fs::unlinkat(f(&DIR), c"u".as_ptr(), 0); }),
    ("mkdir", || { fs::mkdir(c"d".as_ptr(), 0o700); fs::rmdir(c"d".as_ptr()); }),
    ("mkdirat", || { fs::mkdirat(f(&DIR), c"d".as_ptr(), 0o700); fs::unlinkat(f(&DIR), c"d".as_ptr(), 0x200); }),
    ("rmdir", || { fs::mkdir(c"d".as_ptr(), 0o700); fs::rmdir(c"d".as_ptr()); }),
    ("mkfifo", || { fs::mkfifo(c"p".as_ptr(), 0o600); fs::unlink(c"p".as_ptr()); }),
    ("mkfifoat", || { fs::mkfifoat(f(&DIR), c"p".as_ptr(), 0o600); fs::unlink(c"p".as_ptr()); }),
    ("mknod", || { fs::mknod(c"n".as_ptr(), 0o010600, 0); fs::unlink(c"n".as_ptr()); }),
    ("mknodat", || { fs::mknodat(f(&DIR), c"n".as_ptr(), 0o010600, 0); fs::unlink(c"n".as_ptr()); }),
    ("readlink", || { let mut b = [0i8; 16]; fs::readlink(c"l".as_ptr(), b.as_mut_ptr(), 16); }),
    ("readlinkat", || { let mut b = [0i8; 16]; fs::readlinkat(f(&DIR), c"l".as_ptr(), b.as_mut_ptr(), 16); }),
    ("rename", || { fs::rename(c"f".as_ptr(), c"g".as_ptr()); fs::rename(c"g".as_ptr(), c"f".as_ptr()); }),
    ("renameat", || { fs::renameat(f(&DIR), c"f".as_ptr(), f(&DIR), c"g".as_ptr()); fs::renameat(f(&DIR), c"g".as_ptr(), f(&DIR), c"f".as_ptr()); }),
    ("stat", || { fs::stat(c"f".as_ptr(), Some(&mut Default::default())); }),
    ("lstat", || { fs::lstat(c"l".as_ptr(), Some(&mut Default::default())); }),
    ("fstat", || { fs::fstat(f(&FILE), Some(&mut Default::default())); }),
    ("fstatat", || { fs::fstatat(f(&DIR), c"f".as_ptr(), Some(&mut Default::default()), 0); }),
    ("symlink", || { fs::symlink(c"f".as_ptr(), c"s".as_ptr()); fs::unlink(c"s".as_ptr()); }),
    ("symlinkat", || { fs::symlinkat(c"f".as_ptr(), f(&DIR), c"s".as_ptr()); fs::unlink(c"s".as_ptr()); }),
    ("umask", || { let old = fs::umask(0o022); fs::umask(old); }),
    ("utime", || { fs::utime(c"f".as_ptr(), None); }),
    ("utimes", || { fs::utimes(c"f".as_ptr(), None); }),
    ("utimensat", || { fs::utimensat(f(&DIR), c"f".as_ptr(), None, 0); }),
    ("futimens", || { fs::futimens(f(&FILE), None); }),
    ("ftruncate", || { fs::ftruncate(f(&FILE), 16); }),
    // §15 sockets
    ("socket", || { let s = sk::socket(1, 2, 0); io::close(s); }),
    ("socketpair", || { let mut v = [-1; 2]; sk::socketpair(1, 1, 0, Some(&mut v)); io::close(v[0]); io::close(v[1]); }),
    ("bind", || { let s = sk::socket(2, 2, 0); sk::bind(s, (&raw const LOOP0).cast(), 16); io::close(s); }),
    ("listen", || { let s = sk::socket(2, 1, 0); sk::listen(s, 1); io::close(s); }),
    ("accept", || { sk::accept(f(&LISTENER), null_mut(), None); }),
    ("connect", || { let s = sk::socket(2, 1, 0); sk::connect(s, (&raw const LOOP1).cast(), 16); io::close(s); }),
    ("getpeername", || { let mut a = [0u8; 16]; let mut l = 16u32; sk::getpeername(f(&LISTENER), a.as_mut_ptr().cast(), Some(&mut l)); }),
    ("getsockname", || { let mut a = [0u8; 16]; let mut l = 16u32; sk::getsockname(f(&LISTENER), a.as_mut_ptr().cast(), Some(&mut l)); }),
    ("getsockopt", || { let mut v = 0i32; let mut l = 4u32; sk::getsockopt(f(&DGRAM[0]), 1, 3, (&raw mut v).cast(), Some(&mut l)); }),
    ("setsockopt", || { let v = 1i32; sk::setsockopt(f(&DGRAM[0]), 1, 2, (&raw const v).cast(), 4); }),
    ("send", || { sk::send(f(&DGRAM[0]), b"x".as_ptr().cast(), 1, 0x40); }),
    ("recv", || { let mut b = [0u8; 4]; sk::recv(f(&DGRAM[1]), b.as_mut_ptr().cast(), 4, 0x40); }),
    ("sendto", || { sk::sendto(f(&DGRAM[0]), b"x".as_ptr().cast(), 1, 0x40, null(), 0); }),
    ("recvfrom", || { let mut b = [0u8; 4]; sk::recvfrom(f(&DGRAM[1]), b.as_mut_ptr().cast(), 4, 0x40, null_mut(), None); }),
    ("sendmsg", || {
        let mut d = *b"m";
        let mut iov = [Iov { base: d.as_mut_ptr().cast(), len: 1 }];
        let m = Msg { name: null_mut(), namelen: 0, iov: iov.as_mut_ptr(), iovlen: 1, control: null_mut(), controllen: 0, flags: 0 };
        sk::sendmsg(f(&DGRAM[0]), (&raw const m).cast(), 0x40);
    }),
    ("recvmsg", || {
        let mut d = [0u8; 4];
        let mut iov = [Iov { base: d.as_mut_ptr().cast(), len: 4 }];
        let mut m = Msg { name: null_mut(), namelen: 0, iov: iov.as_mut_ptr(), iovlen: 1, control: null_mut(), controllen: 0, flags: 0 };
        sk::recvmsg(f(&DGRAM[1]), (&raw mut m).cast(), 0x40);
    }),
    ("shutdown", || { let mut v = [-1; 2]; sk::socketpair(1, 1, 0, Some(&mut v)); sk::shutdown(v[0], 2); io::close(v[0]); io::close(v[1]); }),
    ("sockatmark", || { sk::sockatmark(f(&LISTENER)); }),
    // §17, §18 identity
    ("getuid", || { us::getuid(); }),
    ("geteuid", || { us::geteuid(); }),
    ("getgid", || { us::getgid(); }),
    ("getegid", || { us::getegid(); }),
    ("getgroups", || { let mut g = [0u32; 64]; us::getgroups(64, g.as_mut_ptr()); }),
    ("getpid", || { pr::getpid(); }),
    ("getppid", || { pr::getppid(); }),
    // §20 signals
    ("kill", || { sg::kill(pr::getpid(), 0); }),
    ("raise", || { sg::raise(0); }),
    ("sigaction", || { let mut a = liella_legacy::signal_compat::CSigaction::new(0, liella_kernel::uapi::SigSet::empty(), 0); sg::sigaction(12, None, Some(&mut a)); }),
    ("signal", || { sg::signal(12, 0); }),
    ("sigset", || { sg::sigset(12, 0); }),
    ("sigaddset", || { let mut s = empty_set(); sg::sigaddset(Some(&mut s), 2); }),
    ("sigdelset", || { let mut s = empty_set(); sg::sigdelset(Some(&mut s), 2); }),
    ("sigemptyset", || { let mut s = empty_set(); sg::sigemptyset(Some(&mut s)); }),
    ("sigfillset", || { let mut s = empty_set(); sg::sigfillset(Some(&mut s)); }),
    ("sigismember", || { sg::sigismember(Some(&empty_set()), 2); }),
    ("sigpending", || { let mut s = empty_set(); sg::sigpending(Some(&mut s)); }),
    ("sigprocmask", || { let mut s = empty_set(); sg::sigprocmask(0, None, Some(&mut s)); }),
    ("pthread_sigmask", || { let mut s = empty_set(); sg::pthread_sigmask(0, None, Some(&mut s)); }),
    ("sigqueue", || { sg::sigqueue(pr::getpid(), 0, 0); }),
    // §23 time
    ("clock_gettime", || { tm::clock_gettime(1, Some(&mut Timespec::default())); }),
    ("time", || { tm::time(None); }),
    ("times", || { tm::times(Some(&mut Tms { utime: 0, stime: 0, cutime: 0, cstime: 0 })); }),
    ("sleep", || { tm::sleep(0); }),
    ("timer_settime", || { tm::timer_settime(timer(), 0, Some(&Itimerspec::default()), None); }),
    ("timer_gettime", || { tm::timer_gettime(timer(), Some(&mut Itimerspec::default())); }),
    ("timer_getoverrun", || { tm::timer_getoverrun(timer()); }),
    ("timer_settime (SIGEV_THREAD)", || { tm::timer_settime(thread_timer(), 0, Some(&Itimerspec::default()), None); }),
    ("timer_gettime (SIGEV_THREAD)", || { tm::timer_gettime(thread_timer(), Some(&mut Itimerspec::default())); }),
    ("timer_getoverrun (SIGEV_THREAD)", || { tm::timer_getoverrun(thread_timer()); }),
];

fn thread_timer() -> *mut core::ffi::c_void {
    core::ptr::without_provenance_mut(THREAD_TIMER.load(Ordering::SeqCst))
}

extern "C" fn never_notified(_: usize) {}

fn timer() -> *mut core::ffi::c_void {
    core::ptr::without_provenance_mut(TIMER.load(Ordering::SeqCst))
}

extern "C" fn run_all(_sig: i32) {
    if CONTROL.load(Ordering::SeqCst) {
        io::write(1, b"malloc
".as_ptr().cast(), 7);
        liella_boundary::selftest::opaque::malloc(CONTROL_SIZE);
        DONE.store(true, Ordering::SeqCst);
        return;
    }
    for (i, (_, call)) in CALLS.iter().enumerate() {
        // Progress before each call: three digits and a newline, no formatting machinery.
        let n = i as u32;
        let line = [b'0' + (n / 100 % 10) as u8, b'0' + (n / 10 % 10) as u8, b'0' + (n % 10) as u8, b'\n'];
        io::write(1, line.as_ptr().cast(), 4);
        call();
    }
    DONE.store(true, Ordering::SeqCst);
}

/// Sets up the files and sockets, then runs the handler under every lock. Exits the child.
fn child(alarm: u32) -> ! {
    {
        // The parent's scratch directory (`Scratch`): the control child ends by its alarm, so
        // it could not remove one of its own.
        let dir = std::path::PathBuf::from(std::env::var_os(SCRATCH).unwrap());
        std::fs::write(dir.join("f"), b"contents").unwrap();
        std::os::unix::fs::symlink("f", dir.join("l")).unwrap();
        std::env::set_current_dir(&dir).unwrap();
        FILE.store(io::liella_open3(c"f".as_ptr(), 2, 0), Ordering::SeqCst);
        DIR.store(io::liella_open3(c".".as_ptr(), 0o200000, 0), Ordering::SeqCst);
        NULL_FD.store(io::liella_open3(c"/dev/null".as_ptr(), 2, 0), Ordering::SeqCst);
        ZERO_FD.store(io::liella_open3(c"/dev/zero".as_ptr(), 0, 0), Ordering::SeqCst);
        let mut v = [-1; 2];
        sk::socketpair(1, 2, 0, Some(&mut v));
        DGRAM[0].store(v[0], Ordering::SeqCst);
        DGRAM[1].store(v[1], Ordering::SeqCst);
        let l = sk::socket(2, 1 | 0o4000, 0);
        sk::bind(l, (&raw const LOOP0).cast(), 16);
        sk::listen(l, 1);
        LISTENER.store(l, Ordering::SeqCst);
        let mut t = null_mut();
        assert_eq!(tm::timer_create(1, Some(&SigEvent::new(1, 0, 0, 0)), Some(&mut t)), 0, "a SIGEV_NONE timer");
        TIMER.store(t.addr(), Ordering::SeqCst);
        let mut t = null_mut();
        let ev = SigEvent::thread(0, never_notified as extern "C" fn(usize) as usize as u64, 0);
        assert_eq!(tm::timer_create(1, Some(&ev), Some(&mut t)), 0, "a SIGEV_THREAD timer");
        THREAD_TIMER.store(t.addr(), Ordering::SeqCst);
        sg::signal(10, run_all as extern "C" fn(i32) as usize);
        let _ = CSys.alarm(alarm);
        liella_boundary::selftest::with_all_locks(|| {
            sg::raise(10);
        });
        CSys.exit_group(if DONE.load(Ordering::SeqCst) { 0 } else { 1 });
    }
}

/// The variable naming the child's scratch directory.
const SCRATCH: &str = "LIELLA_AS_DIR";

/// A scratch directory, removed on drop (also when the test fails). It lives on tmpfs (`/dev/shm`) when there is one:
/// the harness calls `fsync` on its file, and on a busy disk (a host that also runs other VMs) that waited over 20 s
/// (FL-11). The call stays in the list; it is still proven lock- and allocation-free, on a file that never waits for a disk.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let shm = std::path::Path::new("/dev/shm");
        let base = if shm.is_dir() { shm.to_path_buf() } else { std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")) };
        let d = base.join(format!("liella-as-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Scratch(d)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Runs the test `name` alone in a child with `var` set and a scratch directory of its own.
/// Also returns how long it took and the load scale the child's alarm was computed with: the scale is fixed here once
/// and handed to the child (the load may change while it runs, and the report must name the limit the child had).
fn run_child(name: &str, var: &str) -> (std::process::Output, std::time::Duration, u64) {
    let dir = Scratch::new(name);
    let scale = timeout::scale();
    let t = std::time::Instant::now();
    let o = std::process::Command::new(std::env::current_exe().unwrap()).args([name, "--exact", "--test-threads=1"]).env(var, "1").env(SCRATCH, &dir.0).env("LIELLA_TEST_TIMEOUT_SCALE", scale.to_string()).output().unwrap();
    (o, t.elapsed(), scale)
}

#[test]
fn async_signal_safe_functions_take_no_lock_and_allocate_nothing() {
    // Clauses: s20_9_c_0, s0_6_c0_2_3, s4_n_2.
    if std::env::var("LIELLA_AS_SAFETY").is_ok() {
        child(10 * u32::try_from(timeout::scale()).unwrap());
    }
    let (o, took, scale) = run_child("async_signal_safe_functions_take_no_lock_and_allocate_nothing", "LIELLA_AS_SAFETY");
    let out = String::from_utf8_lossy(&o.stdout);
    let last = out.lines().filter_map(|l| l.trim().parse::<usize>().ok()).next_back();
    let name = last.and_then(|i| CALLS.get(i)).map_or("(before the first call)", |(n, _)| n);
    // A slow machine is not a hang: the limit is scaled by the load, and a kill by it says so.
    let watchdog = if std::os::unix::process::ExitStatusExt::signal(&o.status) == Some(14) { timeout::expired_at("async-signal-safety harness", 10, scale, took) } else { String::new() };
    assert_eq!(o.status.code(), Some(0), "the harness stopped in `{name}` (status {:?}): it took a lock or allocated, or hung. {watchdog}", o.status);
    assert_eq!(last, Some(CALLS.len() - 1), "every listed function ran");
}

#[test]
fn the_harness_catches_an_allocation() {
    // Trace: no spec clause (negative control of the AS-safety harness); plan §6, phase 5 exit criterion.
    // The negative control: `malloc` from the handler blocks on the heap lock the harness
    // holds, the alarm ends the child, and the harness names the call.
    if std::env::var("LIELLA_AS_CONTROL").is_ok() {
        CONTROL.store(true, Ordering::SeqCst);
        child(u32::try_from(timeout::scale()).unwrap());
    }
    let (o, took, _) = run_child("the_harness_catches_an_allocation", "LIELLA_AS_CONTROL");
    assert_eq!(std::os::unix::process::ExitStatusExt::signal(&o.status), Some(14), "ended by the alarm (SIGALRM): status {:?} after {took:?}, stdout {:?}, stderr {:?}", o.status, String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("malloc"), "the progress names the call");
}

#[test]
fn the_watchdog_scale_follows_the_load() {
    // Trace: no spec clause (test-harness helper, task FL-9; formula in support/timeout.rs).
    let s = timeout::scale_from;
    assert_eq!(s("0.00 0.00 0.00 1/300 99", 4), 2, "an idle machine: the floor");
    assert_eq!(s("4.00 1.00 1.00 1/300 99", 4), 2, "load equal to the CPUs");
    assert_eq!(s("4.01 1.00 1.00 1/300 99", 4), 3, "a little over");
    assert_eq!(s("16.50 1.00 1.00 1/300 99", 4), 6, "four times oversubscribed");
    assert_eq!(s("0.10 1.00 1.00 33/300 99", 4), 9, "many runnable tasks right now, load average still low (self excluded)");
    assert_eq!(s("900.00 1.00 1.00 1/300 99", 4), 10, "capped");
    assert_eq!(s("garbage", 0), 2, "unreadable input is the floor, zero CPUs counts as one");
}

#[test]
fn a_wall_clock_bound_scales_and_its_failure_names_bound_scale_and_time() {
    // Trace: no spec clause (test-harness helper, task FL-13; support/timeout.rs `assert_within`).
    use std::time::Duration;
    timeout::assert_within_at("fits", Duration::from_secs(10), 3, Duration::from_secs(29));
    timeout::assert_within_at("exactly", Duration::from_secs(10), 3, Duration::from_secs(30));
    let r = std::panic::catch_unwind(|| timeout::assert_within_at("slow work", Duration::from_secs(10), 3, Duration::from_millis(30_500)));
    let msg = r.err().and_then(|e| e.downcast_ref::<String>().cloned()).unwrap_or_default();
    assert!(msg.contains("slow work") && msg.contains("took 30.50 s") && msg.contains("bound 30.00 s") && msg.contains("10.00 s x load scale 3"), "{msg}");
    assert!(timeout::limit(Duration::from_secs(1)) >= Duration::from_secs(2), "the scale floor is 2");
    assert!(timeout::deadline(Duration::from_secs(1)) > std::time::Instant::now());
}
