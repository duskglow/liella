//! §14 searching and sorting under a real address-space limit (`RLIMIT_AS`).
//!
//! Each test re-executes itself as a child, because the limit is process-wide and permanent. The
//! child lowers `RLIMIT_AS` to a little above its size, then drives the exports until memory runs
//! out:
//! - `qsort`/`qsort_r` on an array whose scratch copy cannot be had still sort, and stably (a
//!   probe of the host glibc under the same limit sorts but is NOT stable there; Liella keeps its
//!   stable order on both paths, D-14.1, which gains the caller a defined result);
//! - `hcreate`/`hcreate_r` that cannot get their table fail with 0 and leave the table state
//!   unchanged; `hsearch(ENTER)` that cannot grow fails with 0 and every earlier entry is still found;
//! - `tsearch` that cannot get a node returns NULL, the tree is unchanged and `twalk` still visits
//!   exactly the nodes inserted so far, in order.
//!
//! Needs only `setrlimit` (RLIMIT_AS) and `/proc/self/statm`.
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use std::ffi::CString;
use std::sync::atomic::{AtomicUsize, Ordering};

use liella_abi::ctypes::c_void;
use liella_abi::layout::{struct_hsearch_data, ENTRY};
use liella_boundary::posix::s14_search as sx;
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Rlimit, RLIMIT_AS, RLIM_INFINITY};

/// Address space the child may add to its size when the limit goes on.
const HEADROOM: u64 = 1 << 20;
const FIND: i32 = 0;
const ENTER: i32 = 1;

fn size_bytes() -> u64 {
    let statm = std::fs::read_to_string("/proc/self/statm").unwrap();
    statm.split_whitespace().next().unwrap().parse::<u64>().unwrap() * 4096
}

/// Read memory of this process at `addr` through `/proc/self/mem` (the tests may not use `unsafe`).
fn peek<const N: usize>(addr: usize) -> [u8; N] {
    static MEM: std::sync::OnceLock<std::fs::File> = std::sync::OnceLock::new();
    let f = MEM.get_or_init(|| std::fs::File::open("/proc/self/mem").unwrap());
    let mut b = [0u8; N];
    std::os::unix::fs::FileExt::read_exact_at(f, &mut b, addr as u64).unwrap();
    b
}

fn peek32(addr: usize) -> u32 {
    u32::from_ne_bytes(peek(addr))
}

fn peek64(addr: usize) -> u64 {
    u64::from_ne_bytes(peek(addr))
}

fn limit_now() {
    static ANCHOR: u32 = 7;
    assert_eq!(peek32(std::ptr::addr_of!(ANCHOR) as usize), 7); // opens the file before the limit goes on
    let base = size_bytes();
    CSys.prlimit(0, RLIMIT_AS, Some(&Rlimit { cur: base + HEADROOM, max: RLIM_INFINITY })).unwrap();
    // The allocator behind the exports may hold spare address space. Take what is left in
    // blocks, then give back 64 pages, so the calls under test meet the limit soon and surely.
    let mut ballast: Vec<Vec<u8>> = Vec::with_capacity(1 << 12);
    for len in [1usize << 20, 1 << 12] {
        loop {
            let mut b: Vec<u8> = Vec::new();
            if b.try_reserve_exact(len).is_err() || ballast.len() == ballast.capacity() {
                break;
            }
            ballast.push(b);
        }
    }
    for _ in 0..64 {
        ballast.pop();
    }
    let _ = ballast.leak();
}

/// Run `body` in a re-executed child (exit code 0 passes; each failure has its own code).
fn in_child(name: &str, body: fn() -> i32) {
    if std::env::var("LIELLA_SEARCH_NOMEM").is_ok_and(|v| v == name) {
        CSys.exit_group(body());
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--test-threads=1", "--nocapture"])
        .env("LIELLA_SEARCH_NOMEM", name)
        .output()
        .unwrap();
    assert_eq!(o.status.code(), Some(0), "the child failed: {:?}\n{}", o.status, String::from_utf8_lossy(&o.stderr));
}

// ------------------------------------------------------------------------------ qsort

#[derive(Clone, Copy)]
#[repr(C)]
struct Rec {
    key: u32,
    seq: u32,
    pad: [u32; 2],
}

const RECS: usize = 128 << 10; // 2 MiB: a scratch copy cannot fit in what the ballast leaves

fn fill(v: &mut [Rec]) {
    let mut s = 1u32;
    for (i, r) in v.iter_mut().enumerate() {
        s = s.wrapping_mul(1_103_515_245).wrapping_add(12345);
        *r = Rec { key: (s >> 16) % 7, seq: i as u32, pad: [0; 2] };
    }
}

/// Sorted by key, equal keys in their original order (the sequence number is unique).
fn ordered(v: &[Rec]) -> bool {
    v.windows(2).all(|w| (w[0].key, w[0].seq) < (w[1].key, w[1].seq))
}

extern "C" fn by_key(a: *const c_void, b: *const c_void) -> i32 {
    let (x, y) = (peek32(a as usize), peek32(b as usize));
    i32::from(x > y) - i32::from(x < y)
}

static CTX_CALLS: AtomicUsize = AtomicUsize::new(0);

extern "C" fn by_key_ctx(a: *const c_void, b: *const c_void, ctx: *mut c_void) -> i32 {
    CTX_CALLS.fetch_add(1, Ordering::Relaxed);
    if ctx.is_null() {
        return 0;
    }
    by_key(a, b)
}

fn qsort_body() -> i32 {
    let mut v = vec![Rec { key: 0, seq: 0, pad: [0; 2] }; RECS];
    fill(&mut v);
    let mut warm = [Rec { key: 2, seq: 0, pad: [0; 2] }, Rec { key: 1, seq: 1, pad: [0; 2] }];
    sx::qsort(warm.as_mut_ptr().cast(), 2, 16, Some(by_key));
    limit_now();
    sx::qsort(v.as_mut_ptr().cast(), RECS, 16, Some(by_key));
    if !ordered(&v) {
        return 10;
    }
    fill(&mut v);
    let mut marker = 1u8;
    sx::qsort_r(v.as_mut_ptr().cast(), RECS, 16, Some(by_key_ctx), std::ptr::addr_of_mut!(marker).cast());
    if !ordered(&v) {
        return 11;
    }
    if CTX_CALLS.load(Ordering::Relaxed) == 0 {
        return 12;
    }
    0
}

#[test]
fn qsort_without_scratch_memory_still_sorts_stably() {
    // Trace: D-14.1
    // Clauses: s14_1_g_1 (whole elements move), s14_1_g_2 (stable on both paths).
    in_child("qsort_without_scratch_memory_still_sorts_stably", qsort_body);
}

// ------------------------------------------------------------------------------ hsearch

const KEYS: usize = 40_000;

fn hash_body() -> i32 {
    let names: Vec<CString> = (0..KEYS).map(|i| CString::new(format!("key{i}")).unwrap()).collect();
    let mut warm = struct_hsearch_data { table: 0, size: 0, filled: 0 };
    if sx::hcreate_r(4, Some(&mut warm)) == 0 {
        return 1;
    }
    sx::hdestroy_r(Some(&mut warm));
    limit_now();

    // A table too big for the headroom: failure, and the caller's state is not touched.
    let mut t = struct_hsearch_data { table: 0, size: 0, filled: 0 };
    if sx::hcreate_r(1 << 40, Some(&mut t)) != 0 {
        return 10;
    }
    if (t.table, t.size, t.filled) != (0, 0, 0) {
        return 11;
    }
    // The global table fails the same way, and a small one still works afterwards.
    if sx::hcreate(1 << 40) != 0 {
        return 12;
    }

    // Enter until memory runs out: that ENTER fails with 0, and everything before it is intact.
    if sx::hcreate_r(4, Some(&mut t)) == 0 {
        return 13;
    }
    let mut ret: *mut ENTRY = std::ptr::null_mut();
    let mut entered = 0usize;
    let mut failed = false;
    for (i, n) in names.iter().enumerate() {
        let item = ENTRY { key: n.as_ptr() as usize, data: i + 1 };
        if sx::hsearch_r(item, ENTER, Some(&mut ret), Some(&mut t)) == 0 {
            failed = true;
            break;
        }
        entered += 1;
    }
    if !failed {
        return 14; // the limit never bit
    }
    if entered == 0 {
        return 15;
    }
    for (i, n) in names.iter().enumerate().take(entered) {
        let item = ENTRY { key: n.as_ptr() as usize, data: 0 };
        if sx::hsearch_r(item, FIND, Some(&mut ret), Some(&mut t)) == 0 || ret.is_null() {
            return 16;
        }
        if peek64(ret as usize + 8) != (i + 1) as u64 {
            return 17;
        }
    }
    // The key that failed is absent, and the table still takes lookups for it without harm.
    let item = ENTRY { key: names[entered].as_ptr() as usize, data: 0 };
    if sx::hsearch_r(item, FIND, Some(&mut ret), Some(&mut t)) != 0 {
        return 18;
    }
    sx::hdestroy_r(Some(&mut t));

    // The global table: the same, through hcreate / hsearch.
    if sx::hcreate(4) == 0 {
        return 20;
    }
    let mut global = 0usize;
    for (i, n) in names.iter().enumerate() {
        let r = sx::hsearch(ENTRY { key: n.as_ptr() as usize, data: i + 1 }, ENTER);
        if r.is_null() {
            break;
        }
        global += 1;
    }
    if global == 0 || global == KEYS {
        return 21;
    }
    for (i, n) in names.iter().enumerate().take(global) {
        let r = sx::hsearch(ENTRY { key: n.as_ptr() as usize, data: 0 }, FIND);
        if r.is_null() || peek64(r as usize + 8) != (i + 1) as u64 {
            return 22;
        }
    }
    sx::hdestroy();
    0
}

#[test]
fn hcreate_and_hsearch_fail_with_the_table_intact() {
    // Trace: D-14.3
    // Clauses: s14_5_f_1 (full and no memory to grow: failure), s14_5_f_2 (growth).
    in_child("hcreate_and_hsearch_fail_with_the_table_intact", hash_body);
}

// ------------------------------------------------------------------------------ tsearch

static WALKED: AtomicUsize = AtomicUsize::new(0);
static LAST: AtomicUsize = AtomicUsize::new(0);
static DISORDER: AtomicUsize = AtomicUsize::new(0);

extern "C" fn by_value(a: *const c_void, b: *const c_void) -> i32 {
    let (x, y) = (peek64(a as usize), peek64(b as usize));
    i32::from(x > y) - i32::from(x < y)
}

extern "C" fn visit(node: *const c_void, which: i32, _depth: i32) {
    // postorder (1) and leaf (3) visit each node once, in key order.
    if which == 1 || which == 3 {
        let k = peek64(peek64(node as usize) as usize) as usize;
        if WALKED.fetch_add(1, Ordering::Relaxed) > 0 && k <= LAST.load(Ordering::Relaxed) {
            DISORDER.fetch_add(1, Ordering::Relaxed);
        }
        LAST.store(k, Ordering::Relaxed);
    }
}

fn walk() -> (usize, usize) {
    WALKED.store(0, Ordering::Relaxed);
    LAST.store(0, Ordering::Relaxed);
    DISORDER.store(0, Ordering::Relaxed);
    sx::twalk(ROOT.load(Ordering::Relaxed) as *const c_void, Some(visit));
    (WALKED.load(Ordering::Relaxed), DISORDER.load(Ordering::Relaxed))
}

static ROOT: AtomicUsize = AtomicUsize::new(0);

fn tree_body() -> i32 {
    let keys: Vec<u64> = (1..=KEYS as u64).collect();
    let mut warm: *mut c_void = std::ptr::null_mut();
    sx::tsearch(std::ptr::addr_of!(keys[0]).cast(), Some(&mut warm), Some(by_value));
    limit_now();

    let mut root: *mut c_void = std::ptr::null_mut();
    let mut inserted = 0usize;
    let mut failed = false;
    for k in &keys {
        let r = sx::tsearch(std::ptr::from_ref(k).cast(), Some(&mut root), Some(by_value));
        if r.is_null() {
            failed = true;
            break;
        }
        inserted += 1;
    }
    if !failed {
        return 10; // the limit never bit
    }
    if inserted == 0 {
        return 11;
    }
    ROOT.store(root as usize, Ordering::Relaxed);
    let before = root;
    // Another failing insert changes nothing either.
    if !sx::tsearch(std::ptr::addr_of!(keys[inserted]).cast(), Some(&mut root), Some(by_value)).is_null() {
        return 12;
    }
    if root != before {
        return 13;
    }
    let (walked, disorder) = walk();
    if walked != inserted || disorder != 0 {
        return 14;
    }
    for k in keys.iter().take(inserted) {
        if sx::tfind(std::ptr::from_ref(k).cast(), Some(&root), Some(by_value)).is_null() {
            return 15;
        }
    }
    if !sx::tfind(std::ptr::addr_of!(keys[inserted]).cast(), Some(&root), Some(by_value)).is_null() {
        return 16;
    }
    0
}

#[test]
fn tsearch_returns_null_and_leaves_the_tree_walkable() {
    // Trace: D-14.4
    // Clauses: s14_4_f_0 (no memory: NULL, the tree is intact), s14_4_c_5 (twalk order).
    in_child("tsearch_returns_null_and_leaves_the_tree_walkable", tree_body);
}
