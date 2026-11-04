//! §14.1 `qsort` / `qsort_r` when the temporary position buffer cannot be allocated: the test
//! build's allocation-failure hook (ALLOC-FAIL-INJECT) fails the buffer, which lives in the global
//! allocator's domain (`TryVec`), and the in-place fallback must still sort the whole array and
//! keep every element (a permutation). Order of equal keys is not checked: `qsort` promises none.

#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use liella_abi::ctypes::c_void;
use liella_boundary::posix::s14_search as sx;
use liella_boundary::selftest::failinject::{self, FailAlloc};
use std::sync::atomic::{AtomicUsize, Ordering};

#[global_allocator]
static ALLOC: FailAlloc = FailAlloc;

#[derive(Clone, Copy)]
#[repr(C)]
struct Rec {
    key: u32,
    seq: u32,
    pad: [u32; 2],
}

/// Read memory of this process at `addr` through `/proc/self/mem` (no allocation once opened;
/// the tests may not use `unsafe`).
fn peek32(addr: usize) -> u32 {
    static MEM: std::sync::OnceLock<std::fs::File> = std::sync::OnceLock::new();
    let f = MEM.get_or_init(|| std::fs::File::open("/proc/self/mem").unwrap());
    let mut b = [0u8; 4];
    std::os::unix::fs::FileExt::read_exact_at(f, &mut b, addr as u64).unwrap();
    u32::from_ne_bytes(b)
}

fn by_key_inner(a: *const c_void, b: *const c_void) -> i32 {
    let (x, y) = (peek32(a as usize), peek32(b as usize));
    i32::from(x > y) - i32::from(x < y)
}

extern "C" fn by_key(a: *const c_void, b: *const c_void) -> i32 {
    by_key_inner(a, b)
}

static CTX_CALLS: AtomicUsize = AtomicUsize::new(0);

extern "C" fn by_key_ctx(a: *const c_void, b: *const c_void, ctx: *mut c_void) -> i32 {
    CTX_CALLS.fetch_add(1, Ordering::Relaxed);
    if ctx.is_null() {
        return 0;
    }
    by_key_inner(a, b)
}

fn make(n: usize) -> Vec<Rec> {
    let mut s = 7u32;
    (0..n)
        .map(|i| {
            s = s.wrapping_mul(1_103_515_245).wrapping_add(12345);
            Rec { key: (s >> 16) % 50, seq: i as u32, pad: [0; 2] }
        })
        .collect()
}

/// Sorted by key, and every sequence number present exactly once.
fn check(v: &[Rec], n: usize) {
    assert!(v.windows(2).all(|w| w[0].key <= w[1].key), "sorted by key (n = {n})");
    let mut seen = vec![false; n];
    for r in v {
        assert!(!seen[r.seq as usize], "no element duplicated (n = {n})");
        seen[r.seq as usize] = true;
    }
    assert!(seen.iter().all(|&s| s), "no element lost (n = {n})");
}

fn run(n: usize, with_ctx: bool, expect_injected: bool) {
    let mut v = make(n);
    let mut marker = 1u8;
    peek32(std::ptr::addr_of!(marker) as usize); // open /proc/self/mem before arming
    let before = failinject::injected();
    failinject::fail_at_least(1);
    if with_ctx {
        sx::qsort_r(v.as_mut_ptr().cast(), n, 16, Some(by_key_ctx), std::ptr::addr_of_mut!(marker).cast());
    } else {
        sx::qsort(v.as_mut_ptr().cast(), n, 16, Some(by_key));
    }
    failinject::off();
    let hits = failinject::injected() - before;
    if expect_injected {
        assert!(hits >= 1, "the hook failed the buffer allocation (n = {n})");
    } else {
        assert_eq!(hits, 0, "no heap buffer is wanted (n = {n})");
    }
    check(&v, n);
}

#[test]
fn qsort_buffer_allocation_fails_array_still_sorted() {
    // Trace: D-14.1; Clauses: s14_1_g_1 (whole elements move).
    // 0..=100 need no heap buffer (insertion runs or the stack buffer); larger ones do.
    for n in [0, 1, 2, 8, 9, 50, 85] {
        run(n, false, false);
    }
    for n in [86, 100, 129, 1000, 4097] {
        run(n, false, true);
    }
}

#[test]
fn qsort_r_buffer_allocation_fails_array_still_sorted() {
    // Trace: D-14.1; Clauses: s14_1_g_1 (whole elements move).
    CTX_CALLS.store(0, Ordering::Relaxed);
    for n in [2, 9, 85] {
        run(n, true, false);
    }
    for n in [86, 100, 1000, 4097] {
        run(n, true, true);
    }
    assert!(CTX_CALLS.load(Ordering::Relaxed) > 0, "the context comparator ran");
}
