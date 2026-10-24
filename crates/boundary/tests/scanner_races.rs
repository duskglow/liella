//! D-4.21 (owner ruling 2026-10-07, review finding F1): a scanner whose reported byte no longer
//! matches when Rust re-reads it rescans instead of aborting, because a writer (another thread, a
//! signal handler, another process sharing the page) can cause that; only an answer past its bound
//! (which no writer can cause) still aborts.
//!
//! The race tests put the text in a `MAP_SHARED` page of a memfd and let a writer flip one byte
//! through `pwrite` while the test thread calls each scanner's export in a loop: until the library
//! reports a rescan (so a test cannot pass without the race having happened), at most 20 s per
//! scanner. Every answer is checked against the ones the two versions of the text allow.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

#[path = "../../core/tests/support/timeout.rs"]
mod timeout;

use core::cmp::Ordering;
use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, AtomicUsize, Ordering::SeqCst};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use liella_abi::ctypes::{c_char, c_int, c_void};
use liella_boundary::posix::s01_startup as e;
use liella_boundary::posix::s04_strings as s;
use liella_boundary::selftest::{self, ChildEnd};
use liella_boundary::sys::CSys;
use liella_kernel::fd::Fd;
use liella_kernel::mem::Region;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{MAP_SHARED, MFD_CLOEXEC, PROT_READ, PROT_WRITE, SIGABRT, SIGKILL, SIGUSR1};

const PAGE: usize = 4096;
/// Offsets in the page: the first text, the second text (comparisons), the environment entry.
const A: usize = 0;
const B: usize = 256;
const ENV: usize = 512;
/// The byte the writer flips, and the text's own end (or match) after it.
const AT: usize = 15;
const END: usize = 40;
const NAME: &core::ffi::CStr = c"LIELLA_SCANNER_RACE";

/// The rescan counter and the environment are process-wide: every test holds this.
static LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Set in the process `alone` starts, to the test it runs.
const ALONE: &str = "LIELLA_SCANNER_RACES_ALONE";

/// Run `body` in a fresh process of this binary that runs only `test` (one test thread): the
/// environment writes (`putenv`/`setenv`/`unsetenv` of the test entries) then share the process
/// with no test that starts processes (FL-1). Its own writer child is only `fork`ed (no `exec`),
/// so nothing reads `environ` to start a program.
fn alone(test: &str, body: impl FnOnce()) {
    if std::env::var(ALONE).is_ok_and(|v| v == test) {
        body();
        return;
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap()).args([test, "--exact", "--test-threads=1"]).env(ALONE, test).output().unwrap();
    assert!(o.status.success(), "{test}: {:?}
{}{}", o.status, String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
    assert!(String::from_utf8_lossy(&o.stdout).contains("1 passed"), "{test}: the child ran no test");
}

/// What the writer flips: `pwrite` of `X` or `Y` (alternately) at `OFF` of the memfd `FD`.
static FD: AtomicI32 = AtomicI32::new(-1);
static OFF: AtomicUsize = AtomicUsize::new(0);
static X: AtomicU8 = AtomicU8::new(0);
static Y: AtomicU8 = AtomicU8::new(0);
static STATE: AtomicBool = AtomicBool::new(false);

/// One flip (async-signal-safe: atomics and one system call).
fn flip_once() {
    let v = if STATE.fetch_xor(true, SeqCst) { X.load(SeqCst) } else { Y.load(SeqCst) };
    let _ = CSys.pwrite(Fd::new(FD.load(SeqCst)), &[v], OFF.load(SeqCst) as i64);
}

/// A memfd page mapped `MAP_SHARED`, with an environment entry for `NAME` in it (`putenv`).
struct Shared {
    fd: Fd,
    addr: usize,
    region: Option<Region>,
}

impl Shared {
    fn map_with_entry() -> Shared {
        let fd = CSys.memfd_create(KStr::from_nul_terminated(b"liella-race\0").unwrap(), MFD_CLOEXEC).unwrap();
        CSys.pwrite(fd, &[0u8; PAGE], 0).unwrap();
        let region = CSys.mmap(0, PAGE, PROT_READ | PROT_WRITE, MAP_SHARED, Some(fd), 0).unwrap();
        let p = Shared { fd, addr: region.addr(), region: Some(region) };
        let mut entry = NAME.to_bytes().to_vec();
        entry.extend_from_slice(b"=v\0");
        p.put(ENV, &entry);
        assert_eq!(e::putenv(p.at(ENV).cast_mut().cast()), 0);
        FD.store(fd.raw(), SeqCst);
        p
    }

    fn at(&self, off: usize) -> *const u8 {
        core::ptr::with_exposed_provenance(self.addr + off)
    }

    fn put(&self, off: usize, bytes: &[u8]) {
        assert_eq!(CSys.pwrite(self.fd, bytes, off as i64).unwrap(), bytes.len());
    }
}

impl Drop for Shared {
    fn drop(&mut self) {
        // The writers have stopped; the entry's name may have been left flipped, and `unsetenv`
        // must find it, or `environ` would keep a pointer into the unmapped page.
        let _ = CSys.pwrite(self.fd, NAME.to_bytes(), ENV as i64);
        let gone = e::unsetenv(NAME.as_ptr()) == 0 && e::getenv(NAME.as_ptr()).is_null();
        if !std::thread::panicking() {
            assert!(gone, "the test entry is still in the environment");
        }
        if let Some(r) = self.region.take() {
            let _ = CSys.munmap(r);
        }
        let _ = CSys.close(self.fd);
    }
}

#[derive(Clone, Copy, Debug)]
enum Case {
    Strlen,
    Strnlen,
    Rawmemchr,
    Memchr,
    Memcmp,
    Strcmp,
    Strcoll,
    Getenv,
}

const CASES: [Case; 8] = [Case::Strlen, Case::Strnlen, Case::Rawmemchr, Case::Memchr, Case::Memcmp, Case::Strcmp, Case::Strcoll, Case::Getenv];

/// Write the texts for `c` and aim the writer: byte `AT` of the first text flips between a
/// value that ends (or matches, or differs) there and one that does not; for `getenv`, a
/// byte of the entry's name flips.
fn prepare(p: &Shared, c: Case) {
    let mut text = [b'a'; 64];
    text[END] = 0;
    let (x, y) = match c {
        Case::Strlen | Case::Strnlen => (b'x', 0),
        Case::Rawmemchr | Case::Memchr => {
            text[END] = b'c';
            (b'a', b'c')
        }
        Case::Memcmp | Case::Strcmp | Case::Strcoll => (b'a', b'b'),
        Case::Getenv => (b'L', b'M'),
    };
    p.put(A, &text);
    p.put(B, &text);
    let off = if matches!(c, Case::Getenv) { ENV + 3 } else { A + AT };
    p.put(off, &[x]);
    OFF.store(off, SeqCst);
    X.store(x, SeqCst);
    Y.store(y, SeqCst);
    STATE.store(false, SeqCst);
}

/// One call of `c`'s export, its answer checked against the two versions of the text.
fn check(p: &Shared, c: Case) {
    let (a, b) = (p.at(A), p.at(B));
    let off = |r: *mut c_void| r.addr().wrapping_sub(p.addr);
    match c {
        Case::Strlen => assert!(matches!(s::strlen(a.cast()), AT | END)),
        Case::Strnlen => assert!(matches!(s::strnlen(a.cast(), 100), AT | END)),
        Case::Rawmemchr => assert!(matches!(off(s::rawmemchr(a.cast(), c_int::from(b'c'))), AT | END)),
        Case::Memchr => assert!(matches!(off(s::memchr(a.cast(), c_int::from(b'c'), 64)), AT | END)),
        // 200 bytes: past the D-4.23 register kernel (up to 128 bytes, MEM-K5), which decides "equal" from
        // one load of each byte and has no scanner to rescan; the 129-2048 class (fold.S, AVX2) re-reads its
        // first difference and takes the counted rescan (MEMCMP-MID-2), the checked path's scanner likewise.
        Case::Memcmp => assert!(s::memcmp(a.cast(), b.cast(), 200) >= 0),
        Case::Strcmp => assert!(s::strcmp(a.cast::<c_char>(), b.cast()) >= 0),
        Case::Strcoll => assert!(s::strcoll(a.cast::<c_char>(), b.cast()) >= 0),
        Case::Getenv => {
            let r = e::getenv(NAME.as_ptr());
            assert!(r.is_null() || r.addr() == p.addr + ENV + NAME.to_bytes().len() + 1);
        }
    }
}

/// Call `c` until the library reports a rescan (the race happened and was survived).
fn hammer(p: &Shared, c: Case) {
    let before = selftest::rescans();
    let deadline = timeout::deadline(Duration::from_secs(20));
    let mut calls = 0u64;
    while selftest::rescans() == before {
        assert!(Instant::now() < deadline, "{c:?}: no rescan within 20 s ({calls} calls)");
        for _ in 0..1000 {
            check(p, c);
        }
        calls += 1000;
    }
}

/// Sets the flag on drop (also when a check panics), so a writer thread always stops.
struct Stop<'a>(&'a AtomicBool);

impl Drop for Stop<'_> {
    fn drop(&mut self) {
        self.0.store(true, SeqCst);
    }
}

#[test]
fn scanners_rescan_when_another_thread_changes_the_text() {
    // Trace: no spec clause (scanner postcondition: rescan, abort only on answers no writer can cause); D-4.21.
    alone("scanners_rescan_when_another_thread_changes_the_text", scanners_rescan_when_another_thread_changes_the_text_alone);
}

fn scanners_rescan_when_another_thread_changes_the_text_alone() {
    let _g = lock();
    let p = Shared::map_with_entry();
    for c in CASES {
        prepare(&p, c);
        let done = AtomicBool::new(false);
        std::thread::scope(|sc| {
            sc.spawn(|| {
                while !done.load(SeqCst) {
                    flip_once();
                }
            });
            let _stop = Stop(&done);
            hammer(&p, c);
        });
    }
}

extern "C" fn on_signal(_: c_int) {
    flip_once();
}

#[test]
fn scanners_rescan_when_a_signal_handler_changes_the_text() {
    // Trace: no spec clause (scanner postcondition: rescan, abort only on answers no writer can cause); D-4.21.
    alone("scanners_rescan_when_a_signal_handler_changes_the_text", scanners_rescan_when_a_signal_handler_changes_the_text_alone);
}

fn scanners_rescan_when_a_signal_handler_changes_the_text_alone() {
    let _g = lock();
    let p = Shared::map_with_entry();
    let (pid, tid) = (CSys.getpid(), CSys.gettid());
    selftest::with_handler(SIGUSR1, on_signal, || {
        for c in CASES {
            prepare(&p, c);
            let done = AtomicBool::new(false);
            std::thread::scope(|sc| {
                sc.spawn(|| {
                    while !done.load(SeqCst) {
                        let _ = CSys.tgkill(pid, tid, SIGUSR1);
                        for _ in 0..200 {
                            std::hint::spin_loop();
                        }
                    }
                });
                let _stop = Stop(&done);
                hammer(&p, c);
            });
            // A system call: any signal still pending is handled before the handler goes.
            let _ = CSys.getpid();
        }
    })
    .unwrap();
}

/// The test process, which the writer outlives by at most 1024 flips.
static PARENT: AtomicI32 = AtomicI32::new(0);

fn writer_child(_: &CSys) -> i32 {
    loop {
        for _ in 0..1024 {
            flip_once();
        }
        if CSys.getppid() != PARENT.load(SeqCst) {
            return 0;
        }
    }
}

/// Kills and reaps the writer process on drop.
struct Child(i32);

impl Drop for Child {
    fn drop(&mut self) {
        let _ = CSys.kill(self.0, SIGKILL);
        let _ = CSys.wait4(self.0, 0, None);
    }
}

#[test]
fn scanners_rescan_when_another_process_changes_a_shared_page() {
    // Trace: no spec clause (scanner postcondition: rescan, abort only on answers no writer can cause); D-4.21.
    alone("scanners_rescan_when_another_process_changes_a_shared_page", scanners_rescan_when_another_process_changes_a_shared_page_alone);
}

fn scanners_rescan_when_another_process_changes_a_shared_page_alone() {
    let _g = lock();
    let p = Shared::map_with_entry();
    PARENT.store(CSys.getpid(), SeqCst);
    for c in CASES {
        prepare(&p, c);
        let _child = Child(selftest::spawn_child(writer_child).unwrap());
        hammer(&p, c);
    }
}

#[test]
fn a_stale_answer_is_rescanned_from_the_reported_index() {
    // Trace: no spec clause (scanner postcondition: rescan, abort only on answers no writer can cause); D-4.21.
    alone("a_stale_answer_is_rescanned_from_the_reported_index", a_stale_answer_is_rescanned_from_the_reported_index_alone);
}

fn a_stale_answer_is_rescanned_from_the_reported_index_alone() {
    let _g = lock();
    let before = selftest::rescans();
    // Consistent answers are taken as they are.
    assert_eq!(selftest::settle_strnlen(b"abc\0", usize::MAX, 3), Some(3));
    assert_eq!(selftest::settle_strnlen(b"abcdef\0", 4, 4), Some(4));
    assert_eq!(selftest::settle_strcmp(b"abcx\0", b"abcy\0", 3), Some(Ordering::Less));
    assert_eq!(selftest::settle_rawmemchr(b"abcabc", b'c', 2), Some(2));
    assert_eq!(selftest::settle_memchr(b"abcabc", b'c', 2), Some(2));
    assert_eq!(selftest::settle_memchr(b"abab", b'c', 4), None);
    assert_eq!(selftest::settle_memcmp(b"abcx", b"abcy", 3), Ordering::Less);
    assert_eq!(selftest::settle_memcmp(b"abc", b"abc", 3), Ordering::Equal);
    assert_eq!(selftest::rescans(), before);
    // Answers the text no longer supports are rescanned from the reported index.
    assert_eq!(selftest::settle_strnlen(b"abc\0", usize::MAX, 1), Some(3));
    assert_eq!(selftest::settle_strnlen(b"abcdef\0", 4, 1), Some(4));
    assert_eq!(selftest::settle_strcmp(b"abcx\0", b"abcy\0", 1), Some(Ordering::Less));
    assert_eq!(selftest::settle_strcmp(b"abc\0", b"abc\0", 0), Some(Ordering::Equal));
    assert_eq!(selftest::settle_strcmp(b"ab\0", b"abc\0", 1), Some(Ordering::Less));
    assert_eq!(selftest::settle_rawmemchr(b"abcabc", b'c', 0), Some(2));
    assert_eq!(selftest::settle_memchr(b"abcabc", b'c', 0), Some(2));
    assert_eq!(selftest::settle_memchr(b"abab", b'c', 1), None);
    assert_eq!(selftest::settle_memcmp(b"abcx", b"abcy", 0), Ordering::Less);
    assert_eq!(selftest::settle_memcmp(b"abc", b"abc", 1), Ordering::Equal);
    assert_eq!(selftest::settle_memcmp(b"abc", b"abcd", 1), Ordering::Less);
    // getenv: an entry that is not `name=` is searched for again in the environment.
    assert_eq!(e::setenv(c"LIELLA_SETTLE".as_ptr(), c"1".as_ptr(), 1), 0);
    let want = e::getenv(c"LIELLA_SETTLE".as_ptr()).addr();
    assert_eq!(selftest::settle_getenv(b"LIELLA_SETTLE", b"LIELLA_SETTLX=1\0"), Some(want));
    assert_eq!(selftest::settle_getenv(b"LIELLA_UNSET_NAME", b"OTHER=1\0"), None);
    assert_eq!(e::unsetenv(c"LIELLA_SETTLE".as_ptr()), 0);
    assert_eq!(selftest::rescans(), before + 13);
}

fn past_strnlen_limit(_: &CSys) -> i32 {
    selftest::settle_strnlen(b"abc\0", 2, 3).map_or(1, |_| 0)
}

fn past_memchr_length(_: &CSys) -> i32 {
    selftest::settle_memchr(b"abc", b'x', 4).map_or(0, |_| 1)
}

fn past_memcmp_length(_: &CSys) -> i32 {
    i32::from(selftest::settle_memcmp(b"abc", b"abd", 4) == Ordering::Equal)
}

#[test]
fn an_answer_past_the_bound_still_aborts() {
    // Trace: no spec clause (scanner postcondition: rescan, abort only on answers no writer can cause); D-4.21.
    let _g = lock();
    for f in [past_strnlen_limit, past_memchr_length, past_memcmp_length] {
        assert_eq!(selftest::in_child_quiet(f).unwrap(), ChildEnd::Signaled(SIGABRT));
    }
}
