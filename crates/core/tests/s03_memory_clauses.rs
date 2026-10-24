// Specified from glibc-functional-spec.md §3. Generated stubs were replaced by real tests of the
// SAME NAME (see crates/spectest); `cargo xtask stubs` now only checks coverage.

//! Clause tests for spec §3 (46 clauses).
//!
//! The heap clauses run through the exports of `liella-boundary` (the heap is a C-interface
//! service); the arena clauses run against `liella::mem::Arena` (D-3.6), with the C failure
//! protocol covered by the obstack symbol tests. Memory the library hands out is written and read
//! through the exported `memset`/`memcpy`. Aborts, exits and resource limits run in a
//! re-executed copy of this binary (`LIELLA_S3_PROBE`).
//!
//! The debugging semantics of §3.3 (`mcheck` before the first allocation, `mtrace`) and
//! `M_PERTURB` from the environment need a process Liella's heap served from its start, so they
//! also run as C conformance programs against the products (`conformance/c/s03_*.c`, D-3.5 as
//! decided in phase 13).
//!
//! Pending: `alloca` (§3.4: a compiler builtin, D-3.8, with no library behaviour to test).

#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

#[path = "support/scratch.rs"]
mod scratch;

use core::ptr::null_mut;
use core::sync::atomic::Ordering::Relaxed;
use std::os::unix::process::ExitStatusExt;
use std::sync::{Mutex, MutexGuard};

use liella::error::Errno;
use liella::mem::{Arena, TryBox, TryVec};
use liella_abi::ctypes::{c_int, c_void};
use liella_boundary::data;
use liella_boundary::errno;
use liella_boundary::posix::s03_memory as m;
use liella_boundary::posix::s04_strings as st;
use liella_boundary::posix::s12_stdio as x;
use liella_boundary::selftest::{self, opaque, ChildEnd};
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Rlimit, RLIMIT_AS, RLIM_INFINITY};
use spectest::clause;

const ENOMEM: i32 = 12;
const EINVAL: i32 = 22;
const PAGE: usize = 4096;
const PROBE: &str = "LIELLA_S3_PROBE";

static GLOBAL: Mutex<()> = Mutex::new(());

fn global() -> MutexGuard<'static, ()> {
    GLOBAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Build and run C conformance programs against the products (`conformance/run.sh`).
fn conformance(names: &[&str]) {
    static RUNNER: Mutex<()> = Mutex::new(());
    let _g = RUNNER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let products = ["target/liella/libc.a", "target/liella/liella-crt1.o"];
    assert!(products.iter().all(|p| root.join(p).exists()), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let o = std::process::Command::new("sh").arg(root.join("conformance/run.sh")).args(names).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

fn fill(p: *mut c_void, byte: u8, n: usize) {
    st::memset(p, c_int::from(byte), n);
}

fn bytes(p: *const c_void, n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    st::memcpy(v.as_mut_ptr().cast(), p, n);
    v
}

fn addr(p: *const c_void) -> usize {
    p as usize
}

fn probe() -> Option<String> {
    std::env::var(PROBE).ok()
}

fn run_probe(test: &str, name: &str) -> std::process::Output {
    std::process::Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact", "--test-threads=1", "--nocapture"])
        .env(PROBE, name)
        .output()
        .unwrap()
}

/// Run `body` alone in a re-executed copy of this binary (one test, one thread), so that heap
/// statistics see no other test's allocations. In the copy, `body` runs directly.
fn isolated(test: &str, body: impl FnOnce()) {
    if probe().as_deref() == Some(test) {
        body();
        return;
    }
    let o = run_probe(test, test);
    assert!(o.status.success(), "{test} in isolation: {}", String::from_utf8_lossy(&o.stdout));
}

/// A deterministic generator for the trace tests.
struct Gen(u64);
impl Gen {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }
}

// ------------------------------------------------------------------------------ §3.1

#[test]
fn s3_1_c_0() {
    // Dynamically sized, aligned, writable blocks whose lifetime the caller controls.
    let blocks: Vec<_> = (0..64).map(|i| (opaque::malloc(i * 37 + 1), i * 37 + 1)).collect();
    for (k, &(p, n)) in blocks.iter().enumerate() {
        fill(p, k as u8, n);
    }
    for (k, &(p, n)) in blocks.iter().enumerate() {
        assert_eq!(bytes(p, n), vec![k as u8; n], "block {k} kept its contents until freed");
        opaque::free(p);
    }
}

#[test]
fn s3_1_c_1() {
    for n in [0usize, 1, 15, 16, 17, 255, 4096, 65_536, 1 << 20] {
        let p = opaque::malloc(n);
        assert!(!p.is_null() && addr(p) % 16 == 0, "malloc({n}) aligned to alignof(max_align_t)");
        assert!(m::malloc_usable_size(p) >= n);
        opaque::free(p);
    }
}

#[test]
fn s3_1_c_2() {
    let p = opaque::malloc(4096);
    fill(p, 0x5A, 4096);
    opaque::free(p);
    let z = opaque::calloc(64, 64);
    assert_eq!(bytes(z, 4096), vec![0; 4096], "zero-filled, even reusing a dirty block");
    opaque::free(z);
    errno::set_raw(0);
    assert!(opaque::calloc(1 << 33, 1 << 33).is_null());
    assert_eq!(errno::get(), ENOMEM, "k*n overflowing size_t");
}

#[test]
fn s3_1_c_3() {
    let p = opaque::malloc(50);
    let data: Vec<u8> = (0..50).collect();
    st::memcpy(p, data.as_ptr().cast(), 50);
    let q = opaque::realloc(p, 200_000);
    assert_eq!(bytes(q, 50), data, "min(old, n) bytes preserved when growing (it may move)");
    let r = opaque::realloc(q, 20);
    assert_eq!(bytes(r, 20), data[..20], "and when shrinking");
    opaque::free(r);
}

#[test]
fn s3_1_c_3_1() {
    let p = opaque::realloc(null_mut(), 33);
    assert!(!p.is_null() && m::malloc_usable_size(p) >= 33, "realloc(NULL, n) is malloc(n)");
    opaque::free(p);
}

#[test]
fn s3_1_c_3_2() {
    let p = opaque::malloc(10);
    errno::set_raw(0);
    assert!(opaque::realloc(p, 0).is_null(), "frees and returns NULL (the glibc choice, D-3.2)");
    assert_eq!(errno::get(), 0);
}

#[test]
fn s3_1_c_4() {
    let p = m::reallocarray(null_mut(), 8, 8);
    assert!(m::malloc_usable_size(p) >= 64);
    errno::set_raw(0);
    assert!(m::reallocarray(p, 1 << 33, 1 << 33).is_null());
    assert_eq!(errno::get(), ENOMEM, "overflow-checked product");
    let q = m::reallocarray(p, 100, 8);
    assert!(m::malloc_usable_size(q) >= 800, "the block survived the failed call");
    opaque::free(q);
}

#[test]
fn s3_1_c_5() {
    opaque::free(null_mut());
    for n in [8usize, 100_000, 1 << 21] {
        let p = opaque::malloc(n);
        errno::set_raw(1234);
        opaque::free(p);
        assert_eq!(errno::get(), 1234, "free preserves errno");
    }
}

#[test]
fn s3_1_c_6() {
    // The aligned forms as a group: every one returns memory aligned as asked.
    let mut out = null_mut();
    let ps = [opaque::aligned_alloc(256, 10), m::memalign(256, 10), m::valloc(10), m::pvalloc(10), {
        assert_eq!(opaque::posix_memalign(Some(&mut out), 256, 10), 0);
        out
    }];
    for p in ps {
        assert_eq!(addr(p) % 256, 0);
        opaque::free(p);
    }
}

#[test]
fn s3_1_c_6_1() {
    errno::set_raw(0);
    assert!(opaque::aligned_alloc(3, 9).is_null());
    assert_eq!(errno::get(), EINVAL, "power-of-two alignment required");
    let p = opaque::aligned_alloc(64, 65);
    assert!(!p.is_null() && addr(p) % 64 == 0, "any size (not a multiple of the alignment) is accepted");
    opaque::free(p);
}

#[test]
fn s3_1_c_6_2() {
    let before = 0x77usize as *mut c_void;
    let mut out = before;
    errno::set_raw(0);
    assert_eq!(opaque::posix_memalign(Some(&mut out), 2, 1), EINVAL, "below sizeof(void *)");
    assert_eq!(opaque::posix_memalign(Some(&mut out), 12, 1), EINVAL);
    assert_eq!(opaque::posix_memalign(Some(&mut out), 8, usize::MAX / 2), ENOMEM);
    assert_eq!(out, before, "*p unmodified on failure");
    assert_eq!(errno::get(), 0, "the error number is returned directly");
    assert_eq!(opaque::posix_memalign(Some(&mut out), 8, 1), 0);
    opaque::free(out);
}

#[test]
fn s3_1_c_6_3() {
    for (asked, got) in [(3usize, 4usize), (100, 128), (4097, 8192)] {
        let p = m::memalign(asked, 1);
        assert_eq!(addr(p) % got, 0, "{asked} rounds up to {got}");
        opaque::free(p);
    }
}

#[test]
fn s3_1_c_6_4() {
    for n in [1usize, PAGE, 3 * PAGE + 1] {
        let p = m::valloc(n);
        assert_eq!(addr(p) % PAGE, 0);
        opaque::free(p);
    }
}

#[test]
fn s3_1_c_6_5() {
    let p = m::pvalloc(PAGE + 1);
    assert_eq!(addr(p) % PAGE, 0);
    let u = m::malloc_usable_size(p);
    assert!(u >= 2 * PAGE, "n rounded up to whole pages");
    fill(p, 1, 2 * PAGE);
    opaque::free(p);
}

#[test]
fn s3_1_c_7() {
    let mut g = Gen(7);
    for _ in 0..500 {
        let n = g.below(200_000) as usize;
        let p = opaque::malloc(n);
        let u = m::malloc_usable_size(p);
        assert!(u >= n);
        fill(p, 0xEE, u);
        opaque::free(p);
    }
}

#[test]
fn s3_1_c_8() {
    let p = opaque::malloc(10);
    m::cfree(p);
    let q = opaque::malloc(10);
    m::cfree(q);
}

#[test]
fn s3_1_i_0() {
    for n in [isize::MAX as usize + 1, usize::MAX, usize::MAX - 4095] {
        errno::set_raw(0);
        assert!(opaque::malloc(n).is_null(), "{n:#x} is above PTRDIFF_MAX");
        assert_eq!(errno::get(), ENOMEM);
        assert!(opaque::aligned_alloc(PAGE, n).is_null());
        assert!(opaque::realloc(null_mut(), n).is_null());
    }
}

/// A random trace of allocations and frees from the exports; checks that live blocks never
/// overlap and keep their contents.
fn trace(seed: u64, steps: usize) {
    let mut g = Gen(seed);
    let mut live: Vec<(usize, usize, u8)> = Vec::new();
    for step in 0..steps {
        if live.is_empty() || g.below(3) != 0 {
            let n = match g.below(8) {
                0 => g.below(300_000) as usize,
                1..=2 => g.below(8192) as usize,
                _ => g.below(300) as usize,
            };
            let p = opaque::malloc(n);
            let tag = (step % 251) as u8;
            fill(p, tag, n);
            live.push((addr(p), n, tag));
        } else {
            let (p, n, tag) = live.swap_remove(g.below(live.len() as u64) as usize);
            assert_eq!(bytes(p as *const c_void, n), vec![tag; n], "a live block was overwritten");
            opaque::free(p as *mut c_void);
        }
    }
    live.sort_unstable();
    for w in live.windows(2) {
        assert!(w[0].0 + w[0].1 <= w[1].0, "live blocks overlap");
    }
    for (p, n, tag) in live {
        assert_eq!(bytes(p as *const c_void, n), vec![tag; n]);
        opaque::free(p as *mut c_void);
    }
}

#[test]
fn s3_1_g_1() {
    trace(1, 4000);
}

#[test]
fn s3_1_g_2() {
    // calloc is zero whatever the block held before; malloc growth is merely indeterminate
    // (nothing to assert about it beyond being writable).
    for n in [24usize, 2000, 90_000] {
        let p = opaque::malloc(n);
        fill(p, 0xCC, n);
        opaque::free(p);
        let z = opaque::calloc(n, 1);
        assert_eq!(bytes(z, n), vec![0; n]);
        let g = opaque::realloc(z, n * 2);
        fill(g, 1, n * 2);
        opaque::free(g);
    }
}

#[test]
fn s3_1_g_3() {
    for a in [16usize, 32, 512, PAGE, 1 << 14, 1 << 20] {
        for n in [1usize, 100, 70_000] {
            let p = opaque::aligned_alloc(a, n);
            assert_eq!(addr(p) % a, 0, "requested alignment {a} for {n} bytes");
            opaque::free(p);
        }
    }
}

#[test]
fn s3_1_g_4() {
    // Filling every usable byte of many blocks must not damage the heap's own structures:
    // every block keeps its contents and every free and statistic stays consistent.
    let _g = global();
    let ps: Vec<_> = (0..300).map(|i| opaque::malloc(1 + i * 211 % 70_000)).collect();
    for (i, &p) in ps.iter().enumerate() {
        fill(p, i as u8, m::malloc_usable_size(p));
    }
    for (i, &p) in ps.iter().enumerate() {
        let u = m::malloc_usable_size(p);
        assert_eq!(bytes(p, u), vec![i as u8; u]);
        opaque::free(p);
    }
}

#[test]
fn s3_1_f_1() {
    if probe().as_deref() == Some("limit") {
        let statm = std::fs::read_to_string("/proc/self/statm").unwrap();
        let pages: u64 = statm.split_whitespace().next().unwrap().parse().unwrap();
        CSys.prlimit(0, RLIMIT_AS, Some(&Rlimit { cur: pages * 4096 + (32 << 20), max: RLIM_INFINITY })).unwrap();
        let calls: [fn() -> *mut c_void; 3] = [|| opaque::malloc(256 << 20), || opaque::calloc(1, 256 << 20), || opaque::aligned_alloc(64, 256 << 20)];
        for (k, call) in calls.iter().enumerate() {
            errno::set_raw(0);
            if !call().is_null() || errno::get() != ENOMEM {
                CSys.exit_group(10 + k as i32);
            }
        }
        errno::set_raw(0);
        let mut out = null_mut();
        if opaque::posix_memalign(Some(&mut out), 64, 256 << 20) != ENOMEM || errno::get() != 0 {
            CSys.exit_group(11);
        }
        CSys.exit_group(0);
    }
    let o = run_probe("s3_1_f_1", "limit");
    assert_eq!(o.status.code(), Some(0), "a real exhaustion is NULL with ENOMEM (posix_memalign returns it)");
}

#[test]
fn s3_1_f_2() {
    let a = opaque::malloc(0);
    let b = opaque::malloc(0);
    assert!(!a.is_null() && !b.is_null() && a != b, "unique and non-NULL");
    opaque::free(a);
    opaque::free(b);
}

#[test]
fn s3_1_f_3() {
    match probe().as_deref() {
        Some("double") => {
            let p = opaque::malloc(100);
            opaque::free(p);
            liella_boundary::selftest::expect_crash();
            opaque::free(p);
            CSys.exit_group(0);
        }
        Some("foreign") => {
            let x = 5u64;
            liella_boundary::selftest::expect_crash();
            opaque::free((&raw const x).cast_mut().cast());
            CSys.exit_group(0);
        }
        _ => {}
    }
    for (name, text) in [("double", "double free detected"), ("foreign", "invalid pointer")] {
        let o = run_probe("s3_1_f_3", name);
        assert_eq!(o.status.signal(), Some(6), "{name}: a defined abort (S4)");
        assert!(String::from_utf8_lossy(&o.stderr).contains(text));
    }
}

#[test]
fn s3_1_f_4() {
    let p = opaque::malloc(16);
    st::memcpy(p, b"still here......".as_ptr().cast(), 16);
    assert!(opaque::realloc(p, usize::MAX / 2).is_null());
    assert_eq!(&bytes(p, 10), b"still here");
    opaque::free(p);
}

#[test]
fn s3_1_t_0() {
    // Blocks allocated on one thread are freed on another, under contention. (`fork` after
    // allocation in a threaded parent needs the real `fork` with its atfork handling, which
    // arrives in phase 6, F54.)
    let (tx, rx) = std::sync::mpsc::channel::<(usize, usize)>();
    let consumer = std::thread::spawn(move || {
        for (p, n) in rx {
            assert_eq!(bytes(p as *const c_void, n), vec![(n % 251) as u8; n]);
            opaque::free(p as *mut c_void);
        }
    });
    let producers: Vec<_> = (0..4)
        .map(|t| {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut g = Gen(100 + t);
                for _ in 0..5000 {
                    let n = 1 + g.below(5000) as usize;
                    let p = opaque::malloc(n);
                    fill(p, (n % 251) as u8, n);
                    tx.send((addr(p), n)).unwrap();
                }
            })
        })
        .collect();
    drop(tx);
    let traces: Vec<_> = (0..2).map(|t| std::thread::spawn(move || trace(500 + t, 3000))).collect();
    for h in producers.into_iter().chain(traces) {
        h.join().unwrap();
    }
    consumer.join().unwrap();
}

// ------------------------------------------------------------------------------ §3.2

#[test]
fn s3_2_c_1() {
    assert_eq!(m::mallopt(-3, 1 << 20), 1, "M_MMAP_THRESHOLD");
    assert_eq!(m::mallopt(-1, 1 << 20), 1, "M_TRIM_THRESHOLD");
    assert_eq!(m::mallopt(-2, 0), 1, "M_TOP_PAD");
    assert_eq!(m::mallopt(-8, 4), 1, "M_ARENA_MAX");
    assert_eq!(m::mallopt(-6, 0xA5), 1, "M_PERTURB");
    // D-3.4 (#412): every parameter is accepted as glibc does; only M_MXFAST outside 0..=160 is refused.
    assert_eq!(m::mallopt(-4, 0), 1, "M_MMAP_MAX");
    assert_eq!(m::mallopt(1, 64), 1, "M_MXFAST in range");
    assert_eq!(m::mallopt(1, 161), 0, "M_MXFAST above 160");
    assert_eq!(m::mallopt(1, -1), 0, "M_MXFAST below 0");
    assert_eq!(m::mallopt(-5, 1), 1, "M_CHECK_ACTION");
    assert_eq!(m::mallopt(9999, 1), 1, "unknown: accepted, as glibc does");
}

#[test]
fn s3_2_c_2() {
    isolated("s3_2_c_2", || {
        // The test harness's own threads may allocate or free between two readings (in the
        // export pass the heap is Liella's), so the whole sequence is retried until one run sees
        // no outside change: every relation must hold in the same run, and a heap that broke
        // one would break it in every run. Each run starts from a trimmed heap, so the 8 MiB
        // block of an earlier run is not kept for reuse (D-3.13).
        let run = || -> Result<(), String> {
            let check = |ok: bool, what: &str| if ok { Ok(()) } else { Err(what.to_string()) };
            m::malloc_trim(0);
            let a = m::mallinfo2();
            let small: Vec<_> = (0..100).map(|_| opaque::malloc(1000)).collect();
            let big = opaque::malloc(8 << 20);
            let b = m::mallinfo2();
            for p in small {
                opaque::free(p);
            }
            opaque::free(big);
            let c = m::mallinfo2();
            check(b.uordblks >= a.uordblks + 100_000 + (8 << 20), "in-use bytes")?;
            check(b.arena >= 100_000, "bytes held for small blocks")?;
            check(b.hblks == a.hblks + 1, "one more large block")?;
            check(b.hblkhd >= a.hblkhd + (8 << 20), "mmapped bytes")?;
            check(b.fordblks <= b.arena + b.hblkhd, "free bytes held")?;
            check(c.uordblks == a.uordblks, &format!("in-use bytes back: {} then {}", a.uordblks, c.uordblks))
        };
        let mut last = Err(String::new());
        for _ in 0..100 {
            last = run();
            if last.is_ok() {
                break;
            }
        }
        assert_eq!(last, Ok(()), "no run without outside allocations satisfied every relation");
    });
}

#[test]
fn s3_2_c_3() {
    isolated("s3_2_c_3", || {
        // The test harness's own threads may allocate between two calls (in the export pass the
        // heap is Liella's), so compare only a `mallinfo` bracketed by two equal `mallinfo2`s.
        let sat = |v: u64| i32::try_from(v).unwrap_or(i32::MAX);
        let key = |s: &liella_abi::layout::struct_mallinfo2| (s.uordblks, s.hblks, s.arena);
        let still = (0..1000).find_map(|_| {
            let before = m::mallinfo2();
            let i = m::mallinfo();
            let after = m::mallinfo2();
            (key(&before) == key(&after)).then_some((i, after))
        });
        let (i, s) = still.unwrap();
        assert_eq!((i.uordblks, i.hblks, i.arena), (sat(s.uordblks), sat(s.hblks), sat(s.arena)), "the same statistics as int (saturated)");
    });
}

#[test]
fn s3_2_c_4() {
    if probe().as_deref() == Some("stats") {
        m::malloc_stats();
        CSys.exit_group(0);
    }
    let o = run_probe("s3_2_c_4", "stats");
    assert!(o.status.success());
    assert!(!String::from_utf8_lossy(&o.stdout).contains("system bytes"), "nothing on stdout");
    assert!(String::from_utf8_lossy(&o.stderr).contains("system bytes"), "statistics on stderr");
}

/// `M_PERTURB` acts: from `mallopt`, `MALLOC_PERTURB_` and the tunable (C programs, so that the
/// environment is read at start).
#[test]
fn s3_2_c_1_perturb() {
    // Clauses: s3_2_c_1, s1_8_c_0 (D-3.4).
    conformance(&["s03_perturb", "s03_perturb_tunable"]);
}

/// With `M_PERTURB` on, a released block keeps its fill while the next allocations are served
/// elsewhere (perturbation's quarantine), so a read after `free` sees the fill byte. An explicit
/// `mallopt` wins over the test runner's switch.
#[test]
fn s3_2_c_1_perturb_holds_released_blocks() {
    // Clauses: s3_2_c_1 (D-3.4; the quarantine, FREE-POISON-TESTS-2).
    isolated("s3_2_c_1_perturb_holds_released_blocks", || {
        assert_eq!(m::mallopt(-6, 0x5a), 1, "M_PERTURB");
        let p = opaque::malloc(48);
        fill(p, 0x11, 48);
        opaque::free(p);
        let next: Vec<_> = (0..16).map(|_| opaque::malloc(48)).collect();
        assert!(!next.contains(&p), "the released block is held back");
        // Read into the stack: with the exports interposed a `Vec` would be a block of this heap.
        let mut held = [0u8; 48];
        st::memcpy(held.as_mut_ptr().cast(), p, 48);
        assert!(held.iter().all(|&b| b == 0x5a), "and keeps the fill");
        for &q in &next {
            opaque::free(q);
        }
        assert_eq!(m::mallopt(-6, 0), 1, "M_PERTURB off");
        // One thread, no other allocation: the heap serves the next block from those just given
        // back.
        let q = opaque::malloc(48);
        assert!(q == p || next.contains(&q), "the held blocks went back to the heap");
        opaque::free(q);
    });
}

#[test]
fn s3_2_c_5() {
    let mut buf = vec![0u8; 8192];
    let fp = x::fmemopen(buf.as_mut_ptr().cast(), buf.len(), c"w".as_ptr());
    assert!(!fp.is_null());
    assert_eq!(m::malloc_info(0, fp), 0);
    assert_eq!(x::fflush(fp), 0);
    let text = String::from_utf8_lossy(&buf[..buf.iter().position(|&b| b == 0).unwrap()]).into_owned();
    assert!(text.starts_with("<malloc version=\"1\">\n<heap nr=\"0\">\n<sizes>\n</sizes>\n"), "{text}");
    assert!(text.ends_with("</malloc>\n"), "{text}");
    for element in ["<total type=\"rest\" count=\"", "<total type=\"mmap\" count=\"", "<system type=\"current\" size=\"", "<system type=\"max\" size=\"", "<aspace type=\"total\" size=\"", "<aspace type=\"mprotect\" size=\""] {
        assert!(text.contains(element), "{element} in {text}");
    }
    errno::set_raw(0);
    assert_eq!(m::malloc_info(1, fp), -1, "options other than 0");
    assert_eq!(errno::get(), EINVAL);
    errno::set_raw(0);
    assert_eq!(m::malloc_info(0, null_mut()), -1, "a NULL stream");
    assert_eq!(errno::get(), EINVAL);
    assert_eq!(x::fclose(fp), 0);
}

#[test]
fn s3_2_c_6() {
    isolated("s3_2_c_6", || {
        let ps: Vec<_> = (0..400).map(|_| opaque::malloc(30_000)).collect();
        for p in ps {
            opaque::free(p);
        }
        assert_eq!(m::malloc_trim(0), 1, "1 when memory was released");
        let p = opaque::malloc(30_000);
        fill(p, 3, 30_000);
        opaque::free(p);
    });
}

// ------------------------------------------------------------------------------ §3.3

#[test]
fn s3_3_c_1() {
    let _g = global();
    for hook in [&data::__malloc_hook, &data::__free_hook, &data::__realloc_hook, &data::__memalign_hook] {
        assert_eq!(hook.load(Relaxed), 0);
        hook.store(1, Relaxed); // a bogus function address: calling it would crash
    }
    let p = opaque::malloc(10);
    let q = opaque::realloc(p, 20);
    opaque::free(m::memalign(32, 1));
    opaque::free(q);
    for hook in [&data::__malloc_hook, &data::__free_hook, &data::__realloc_hook, &data::__memalign_hook] {
        hook.store(0, Relaxed);
    }
}

extern "C" fn abort_fn(_: c_int) {
    CSys.exit_group(99);
}

#[test]
fn s3_3_c_2() {
    // After the first allocation (a test process has allocated before any test runs): checking
    // cannot be enabled any more and every probe reports MCHECK_DISABLED.
    assert_eq!(m::mcheck(Some(abort_fn)), -1);
    assert_eq!(m::mcheck_pedantic(Some(abort_fn)), -1);
    let p = opaque::malloc(4);
    assert_eq!(m::mprobe(p), -1, "MCHECK_DISABLED");
    m::mcheck_check_all();
    opaque::free(p);
    // Before it, in a program Liella started (D-3.5 as decided in phase 13): checking with
    // abortfn, the default handler's abort, and mcheck_pedantic.
    conformance(&["s03_mcheck", "s03_mcheck_default", "s03_mcheck_pedantic"]);
}

#[test]
fn s3_3_c_3() {
    if probe().as_deref() == Some("trace") {
        m::mtrace();
        opaque::free(opaque::malloc(100));
        m::muntrace();
        CSys.exit_group(0);
    }
    let path = scratch::Scratch::path("mtrace");
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["s3_3_c_3", "--exact", "--test-threads=1"])
        .env(PROBE, "trace")
        .env("MALLOC_TRACE", &*path)
        .output()
        .unwrap();
    assert!(o.status.success());
    // Liella's C library traces itself (D-3.5 as decided in phase 13; the reference needs
    // libc_malloc_debug.so preloaded): the file holds the start, the 100-byte block's
    // allocation and release, and the end.
    let log = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(log.starts_with("= Start\n") && log.ends_with("= End\n"), "{log}");
    assert!(log.lines().any(|l| l.starts_with("@ ") && l.contains(" + 0x") && l.ends_with(" 0x64")), "{log}");
    assert!(log.lines().any(|l| l.starts_with("@ ") && l.contains(" - 0x")), "{log}");
    // Without MALLOC_TRACE nothing is written, and a secure process never writes a file its
    // environment names.
    conformance(&["s03_mtrace", "s03_mtrace_secure"]);
}

#[test]
fn s3_3_c_4() {
    errno::set_raw(0);
    assert!(m::malloc_get_state().is_null());
    assert_ne!(errno::get(), 0);
    assert_eq!(m::malloc_set_state(null_mut()), -1, "they fail");
}

// ------------------------------------------------------------------------------ §3.4

clause!(s3_4_c_0, "3.4", "Contract", r##"pending: alloca is a compiler builtin (D-3.8); no library behaviour"##);
clause!(s3_4_f_0, "3.4", "Failure", r##"pending: alloca is a compiler builtin (D-3.8); no library behaviour"##);

// ------------------------------------------------------------------------------ §3.5

#[test]
fn s3_5_c_0() {
    let mut a = Arena::new(128, 8).unwrap();
    let first = a.alloc(b"one").unwrap();
    a.grow(b"tw").unwrap();
    a.grow(b"o").unwrap();
    assert_eq!(a.growing(), b"two", "an object grows before it is finished");
    let second = a.finish().unwrap();
    let third = a.alloc(&[3; 500]).unwrap();
    assert_eq!(a.get(second).unwrap(), b"two");
    a.free_to(second).unwrap();
    assert_eq!(a.get(first).unwrap(), b"one", "objects before the freed one survive");
    assert!(a.get(third).is_none(), "everything after it is released");
    assert_eq!(a.free_to(third), Err(Errno::EINVAL), "an object no longer in the arena");
}

#[test]
fn s3_5_i_0() {
    let a = Arena::new(0, 0).unwrap();
    assert!(a.memory_used() >= liella::mem::ARENA_CHUNK, "0 selects the default chunk size");
    assert_eq!(Arena::new(64, 24).unwrap_err(), Errno::EINVAL, "the alignment is a power of two");
    let b = Arena::new(10_000, 64).unwrap();
    assert!(b.memory_used() >= 10_000, "the chunk-size hint");
    // The caller-supplied chunk functions are the C protocol: see the obstack symbol tests.
}

#[test]
fn s3_5_g_1() {
    let mut a = Arena::new(64, 8).unwrap();
    let objs: Vec<_> = (0..200u32).map(|i| (a.alloc(&i.to_le_bytes().repeat(i as usize % 50 + 1)).unwrap(), i)).collect();
    let places: Vec<_> = objs.iter().map(|&(r, _)| a.get(r).unwrap().as_ptr()).collect();
    for (k, &(r, i)) in objs.iter().enumerate() {
        assert_eq!(a.get(r).unwrap(), i.to_le_bytes().repeat(i as usize % 50 + 1), "contents");
        assert_eq!(a.get(r).unwrap().as_ptr(), places[k], "finished objects never move");
    }
}

#[test]
fn s3_5_g_2() {
    let mut a = Arena::new(64, 8).unwrap();
    a.grow(b"start").unwrap();
    let before = a.growing().as_ptr();
    a.grow(&[b'.'; 1000]).unwrap();
    assert_ne!(a.growing().as_ptr(), before, "a growing object may move");
    assert_eq!(&a.growing()[..5], b"start", "with its contents");
    let r = a.finish().unwrap();
    assert_eq!(a.get(r).unwrap().len(), 1005);
}

#[test]
fn s3_5_g_3() {
    for align in [1usize, 8, 16, 64] {
        let mut a = Arena::new(256, align).unwrap();
        let objs: Vec<_> = (1..60).map(|n| a.alloc(&vec![1; n]).unwrap()).collect();
        for r in objs {
            assert_eq!(a.get(r).unwrap().as_ptr().addr() % align, 0, "objects follow the alignment ({align})");
        }
    }
}

#[test]
fn s3_5_f_0() {
    // Internally failure is ENOMEM (D-3.6); the C handler protocol (default: message and
    // exit(obstack_exit_failure)) is the obstack symbol tests' business.
    assert_eq!(Arena::new(usize::MAX / 2, 16).unwrap_err(), Errno::ENOMEM);
    let mut a = Arena::new(64, 16).unwrap();
    assert_eq!(a.grow(&[]).map(|()| a.growing().len()), Ok(0));
    assert_eq!(TryVec::<u64>::with_capacity(usize::MAX / 4).unwrap_err(), Errno::ENOMEM, "fallible collections");
    assert_eq!(*TryBox::new(5u8).unwrap(), 5);
    assert_eq!(data::obstack_exit_failure.load(Relaxed), 1);
}

// ------------------------------------------------------------------------------ §3.7

fn break_moves(_: &CSys) -> i32 {
    let old = m::sbrk(0) as usize;
    if m::sbrk(PAGE as isize) as usize != old || m::sbrk(0) as usize != old + PAGE {
        return 1;
    }
    if m::brk(old as *mut c_void) != 0 || m::sbrk(0) as usize != old {
        return 2;
    }
    0
}

fn break_fails(_: &CSys) -> i32 {
    errno::set_raw(0);
    if m::brk(usize::MAX as *mut c_void) != -1 || errno::get() != ENOMEM {
        return 1;
    }
    errno::set_raw(0);
    if m::sbrk(isize::MAX) as isize != -1 || errno::get() != ENOMEM {
        return 2;
    }
    0
}

#[test]
fn s3_7_c_0() {
    assert_eq!(selftest::in_child(break_moves).unwrap(), ChildEnd::Exited(0));
}

#[test]
fn s3_7_f_0() {
    assert_eq!(selftest::in_child(break_fails).unwrap(), ChildEnd::Exited(0));
}
