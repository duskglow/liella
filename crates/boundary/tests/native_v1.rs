//! The native C ABI, version node `LIELLA_NATIVE_1` (D-X.10): every export called in process,
//! the structures' layouts and the constants against `spec/abi/native.tsv`, and the products
//! (the C conformance program `native_v1` and `libc.so.6`'s version node).

#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::indexing_slicing, clippy::arithmetic_side_effects, missing_docs)]

use std::collections::BTreeMap;
use std::mem::{align_of, offset_of, size_of};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};

use liella_boundary::native::aio::*;
use liella_boundary::native::clock::*;
use liella_boundary::native::consts::*;
use liella_boundary::native::env::*;
use liella_boundary::native::fd::*;
use liella_boundary::native::fs::*;
use liella_boundary::native::getpass::*;
use liella_boundary::native::heap::*;
use liella_boundary::native::locale::*;
use liella_boundary::native::process::*;
use liella_boundary::native::secure::*;
use liella_boundary::native::nscache::*;
use liella_boundary::native::types::{LiellaNscacheInfo, LiellaStat, LiellaTimespec, LiellaTm};
use liella_boundary::native::tz::*;
use liella_boundary::posix::s03_memory as s03;
use liella_boundary::posix::s04_strings as s04;
use liella_boundary::selftest::{self, ChildEnd};
use liella_boundary::sys::CSys;
use liella_kernel::Errno;

mod products;

const TABLE: &str = include_str!("../../../spec/abi/native.tsv");

/// Rows of the table of one kind: name -> (node, type, detail).
fn rows(kind: &str) -> BTreeMap<&'static str, (&'static str, &'static str, &'static str)> {
    TABLE
        .lines()
        .skip(1)
        .filter_map(|l| {
            let f: Vec<&str> = l.split('\t').collect();
            (f[0] == kind).then(|| (f[1], (f[2], f[3], f[4])))
        })
        .collect()
}

fn neg(e: Errno) -> i64 {
    -i64::from(e.get())
}

/// Tests that change the environment (and with it the time zone) run one at a time, and apart
/// from the one that starts processes: Rust's `Command` reads `environ` under its own lock,
/// which the library's `setenv` does not take (the library replaces and frees arrays it owns).
static ENV: Mutex<()> = Mutex::new(());

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV.lock().unwrap_or_else(PoisonError::into_inner)
}

// ------------------------------------------------------------------------------ layouts

macro_rules! layout {
    ($c:literal, $t:ty, [$($f:ident),*]) => {{
        let mut v: Vec<(String, usize, usize)> = vec![(String::from($c), size_of::<$t>(), align_of::<$t>())];
        $( v.push((format!("{}.{}", $c, stringify!($f)), offset_of!($t, $f), 0)); )*
        v
    }};
}

#[test]
fn layouts_match_the_table() {
    // Trace: no spec clause (LIELLA_NATIVE_1 structure layouts, spec/abi/native.tsv); D-X.10.
    let mut rust = Vec::new();
    rust.extend(layout!("liella_timespec", LiellaTimespec, [sec, nsec]));
    rust.extend(layout!("liella_stat", LiellaStat, [dev, ino, nlink, rdev, size, blksize, blocks, atime, mtime, ctime, mode, uid, gid, reserved]));
    rust.extend(layout!("liella_tm", LiellaTm, [year, month, day, hour, minute, second, weekday, yearday, isdst, utc_offset, abbr_len, abbr]));
    rust.extend(layout!("liella_nscache_info", LiellaNscacheInfo, [enabled, categories, mode, reason, owner, generation, hits, misses, stats, stores]));
    let structs = rows("struct");
    let fields = rows("field");
    assert_eq!(rust.iter().filter(|r| !r.0.contains('.')).count(), structs.len(), "every table structure has a Rust layout");
    assert_eq!(rust.iter().filter(|r| r.0.contains('.')).count(), fields.len(), "every table field has a Rust field");
    for (name, a, b) in &rust {
        match name.split_once('.') {
            None => {
                let (node, size, align) = structs[name.as_str()];
                assert_eq!(node, "LIELLA_NATIVE_1");
                assert_eq!((a.to_string(), b.to_string()), (size.to_string(), align.to_string()), "struct {name}: size and alignment");
            }
            Some(_) => {
                let (_, _, off) = fields[name.as_str()];
                assert_eq!(a.to_string(), off, "{name}: offset");
            }
        }
    }
    // The documented sizes.
    assert_eq!((size_of::<LiellaTimespec>(), align_of::<LiellaTimespec>()), (16, 8));
    assert_eq!((size_of::<LiellaStat>(), align_of::<LiellaStat>()), (120, 8));
    assert_eq!((size_of::<LiellaTm>(), align_of::<LiellaTm>()), (64, 8));
    assert_eq!((size_of::<LiellaNscacheInfo>(), align_of::<LiellaNscacheInfo>()), (64, 8));
}

fn parse_c(v: &str) -> i64 {
    if let Some(h) = v.strip_prefix("0x") {
        i64::from_str_radix(h, 16).unwrap()
    } else if v.len() > 1 && v.starts_with('0') {
        i64::from_str_radix(&v[1..], 8).unwrap()
    } else {
        v.parse().unwrap()
    }
}

#[test]
fn constants_match_the_table() {
    // Trace: no spec clause (LIELLA_NATIVE_1 constants, spec/abi/native.tsv); D-X.10, D-16.10.
    let rust: Vec<(&str, i64)> = vec![
        ("LIELLA_NATIVE_VERSION", i64::from(LIELLA_NATIVE_VERSION)),
        ("LIELLA_HEAP_MIN_ALIGN", LIELLA_HEAP_MIN_ALIGN as i64),
        ("LIELLA_O_RDONLY", i64::from(LIELLA_O_RDONLY)),
        ("LIELLA_O_WRONLY", i64::from(LIELLA_O_WRONLY)),
        ("LIELLA_O_RDWR", i64::from(LIELLA_O_RDWR)),
        ("LIELLA_O_CREAT", i64::from(LIELLA_O_CREAT)),
        ("LIELLA_O_EXCL", i64::from(LIELLA_O_EXCL)),
        ("LIELLA_O_TRUNC", i64::from(LIELLA_O_TRUNC)),
        ("LIELLA_O_APPEND", i64::from(LIELLA_O_APPEND)),
        ("LIELLA_O_NONBLOCK", i64::from(LIELLA_O_NONBLOCK)),
        ("LIELLA_O_DIRECTORY", i64::from(LIELLA_O_DIRECTORY)),
        ("LIELLA_O_NOFOLLOW", i64::from(LIELLA_O_NOFOLLOW)),
        ("LIELLA_O_CLOEXEC", i64::from(LIELLA_O_CLOEXEC)),
        ("LIELLA_SEEK_SET", i64::from(LIELLA_SEEK_SET)),
        ("LIELLA_SEEK_CUR", i64::from(LIELLA_SEEK_CUR)),
        ("LIELLA_SEEK_END", i64::from(LIELLA_SEEK_END)),
        ("LIELLA_FS_NOFOLLOW", i64::from(LIELLA_FS_NOFOLLOW)),
        ("LIELLA_S_IFMT", i64::from(LIELLA_S_IFMT)),
        ("LIELLA_S_IFSOCK", i64::from(LIELLA_S_IFSOCK)),
        ("LIELLA_S_IFLNK", i64::from(LIELLA_S_IFLNK)),
        ("LIELLA_S_IFREG", i64::from(LIELLA_S_IFREG)),
        ("LIELLA_S_IFBLK", i64::from(LIELLA_S_IFBLK)),
        ("LIELLA_S_IFDIR", i64::from(LIELLA_S_IFDIR)),
        ("LIELLA_S_IFCHR", i64::from(LIELLA_S_IFCHR)),
        ("LIELLA_S_IFIFO", i64::from(LIELLA_S_IFIFO)),
        ("LIELLA_LC_CTYPE", i64::from(LIELLA_LC_CTYPE)),
        ("LIELLA_LC_NUMERIC", i64::from(LIELLA_LC_NUMERIC)),
        ("LIELLA_LC_TIME", i64::from(LIELLA_LC_TIME)),
        ("LIELLA_LC_COLLATE", i64::from(LIELLA_LC_COLLATE)),
        ("LIELLA_LC_MONETARY", i64::from(LIELLA_LC_MONETARY)),
        ("LIELLA_LC_MESSAGES", i64::from(LIELLA_LC_MESSAGES)),
        ("LIELLA_LC_ALL", i64::from(LIELLA_LC_ALL)),
        ("LIELLA_LC_PAPER", i64::from(LIELLA_LC_PAPER)),
        ("LIELLA_LC_NAME", i64::from(LIELLA_LC_NAME)),
        ("LIELLA_LC_ADDRESS", i64::from(LIELLA_LC_ADDRESS)),
        ("LIELLA_LC_TELEPHONE", i64::from(LIELLA_LC_TELEPHONE)),
        ("LIELLA_LC_MEASUREMENT", i64::from(LIELLA_LC_MEASUREMENT)),
        ("LIELLA_LC_IDENTIFICATION", i64::from(LIELLA_LC_IDENTIFICATION)),
        ("LIELLA_TZ_ABBR_CAP", LIELLA_TZ_ABBR_CAP as i64),
        ("LIELLA_NSCACHE_HOSTS", i64::from(LIELLA_NSCACHE_HOSTS)),
        ("LIELLA_NSCACHE_PASSWD", i64::from(LIELLA_NSCACHE_PASSWD)),
        ("LIELLA_NSCACHE_GROUP", i64::from(LIELLA_NSCACHE_GROUP)),
        ("LIELLA_NSCACHE_SERVICES", i64::from(LIELLA_NSCACHE_SERVICES)),
        ("LIELLA_NSCACHE_PROTOCOLS", i64::from(LIELLA_NSCACHE_PROTOCOLS)),
        ("LIELLA_NSCACHE_NETWORKS", i64::from(LIELLA_NSCACHE_NETWORKS)),
        ("LIELLA_NSCACHE_ALL", i64::from(LIELLA_NSCACHE_ALL)),
        ("LIELLA_NSCACHE_MODE_OFF", i64::from(LIELLA_NSCACHE_MODE_OFF)),
        ("LIELLA_NSCACHE_MODE_IDLE", i64::from(LIELLA_NSCACHE_MODE_IDLE)),
        ("LIELLA_NSCACHE_MODE_PRIVATE", i64::from(LIELLA_NSCACHE_MODE_PRIVATE)),
        ("LIELLA_NSCACHE_MODE_SHARED", i64::from(LIELLA_NSCACHE_MODE_SHARED)),
        ("LIELLA_NSCACHE_MODE_UNAVAILABLE", i64::from(LIELLA_NSCACHE_MODE_UNAVAILABLE)),
        ("LIELLA_NSCACHE_REASON_NONE", i64::from(LIELLA_NSCACHE_REASON_NONE)),
        ("LIELLA_NSCACHE_REASON_NO_REGION", i64::from(LIELLA_NSCACHE_REASON_NO_REGION)),
        ("LIELLA_NSCACHE_REASON_SECURE", i64::from(LIELLA_NSCACHE_REASON_SECURE)),
        ("LIELLA_NSCACHE_REASON_REFUSED", i64::from(LIELLA_NSCACHE_REASON_REFUSED)),
        ("LIELLA_NSCACHE_REASON_UNAVAILABLE", i64::from(LIELLA_NSCACHE_REASON_UNAVAILABLE)),
        ("LIELLA_GETPASS_DEFAULT_LIMIT", LIELLA_GETPASS_DEFAULT_LIMIT as i64),
    ];
    // The category numbers are the cache's.
    for c in liella::names::cache::Category::ALL {
        assert_eq!(liella::names::cache::Category::from_index(c.index()), Some(c));
    }
    assert_eq!(liella::names::cache::Category::Networks.index() as u32, LIELLA_NSCACHE_NETWORKS);
    let table = rows("const");
    assert_eq!(rust.len(), table.len(), "every table constant has a Rust constant");
    for (name, v) in rust {
        assert!(table.contains_key(name), "{name} not in the table");
        assert_eq!(parse_c(table[name].2), v, "{name}");
    }
    // The category numbers are the C library's.
    for (n, cat) in [(LIELLA_LC_CTYPE, liella::locale::file::Cat::Ctype), (LIELLA_LC_IDENTIFICATION, liella::locale::file::Cat::Identification)] {
        assert_eq!(cat.number(), n);
    }
}

// ------------------------------------------------------------------------------ heap

/// Tests may not use `unsafe`: heap blocks are written and read through the POSIX layer's
/// `memset` and `memcpy`.
fn fill(p: *mut core::ffi::c_void, v: u8, n: usize) {
    s04::memset(p, i32::from(v), n);
}

fn contents(p: *const core::ffi::c_void, n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    s04::memcpy(v.as_mut_ptr().cast(), p, n);
    v
}

#[test]
fn heap_alloc_honours_size_and_alignment() {
    // Trace: no spec clause (native heap interface, LIELLA_NATIVE_1); D-X.10, S8.
    for align in [1usize, 8, 16, 64, 4096, 1 << 16] {
        for size in [0usize, 1, 24, 1000, 100_000] {
            let p = liella_heap_alloc(size, align);
            assert!(!p.is_null(), "size {size} align {align}");
            assert_eq!(p as usize % align.max(LIELLA_HEAP_MIN_ALIGN), 0, "size {size} align {align}");
            assert!(liella_heap_usable_size(p) >= size);
            fill(p, 0xa5, size);
            assert!(contents(p, size).iter().all(|&b| b == 0xa5));
            liella_heap_free(p);
        }
    }
    assert!(liella_heap_alloc(16, 3).is_null(), "an alignment that is not a power of two");
    assert!(liella_heap_alloc(16, 0).is_null());
    assert!(liella_heap_alloc(usize::MAX - 64, 16).is_null(), "exhaustion is NULL, not an abort");
    assert!(liella_heap_alloc_zeroed(16, 24).is_null());
    liella_heap_free(std::ptr::null_mut());
    assert_eq!(liella_heap_usable_size(std::ptr::null()), 0);
}

#[test]
fn heap_zeroed_and_realloc() {
    // Trace: no spec clause (native heap interface, LIELLA_NATIVE_1, one heap with malloc); D-X.10.
    // Dirty blocks, release them, then ask for zeroed ones of the same classes.
    for align in [16usize, 64, 4096] {
        for n in [24usize, 3000, 200_000] {
            for _ in 0..4 {
                let p = liella_heap_alloc(n, align);
                fill(p, 0x5a, n);
                liella_heap_free(p);
                let z = liella_heap_alloc_zeroed(n, align);
                assert_eq!(z as usize % align, 0);
                assert!(contents(z, n).iter().all(|&b| b == 0), "size {n} align {align}");
                fill(z, 0x5a, n);
                liella_heap_free(z);
            }
        }
    }
    // realloc keeps the contents, NULL allocates, size 0 frees.
    let p = liella_heap_alloc(10, 16);
    s04::memcpy(p, b"0123456789".as_ptr().cast(), 10);
    let q = liella_heap_realloc(p, 100_000);
    assert!(!q.is_null() && liella_heap_usable_size(q) >= 100_000);
    assert_eq!(contents(q, 10), b"0123456789");
    assert!(liella_heap_realloc(q, 0).is_null(), "size 0 frees");
    let r = liella_heap_realloc(std::ptr::null_mut(), 32);
    assert!(!r.is_null());
    assert!(liella_heap_realloc(r, usize::MAX - 64).is_null(), "a failure leaves the block");
    assert!(liella_heap_usable_size(r) >= 32);
    // Blocks are interchangeable with the POSIX functions.
    liella_boundary::posix::s03_memory::free(r);
    let m = liella_boundary::posix::s03_memory::malloc(40);
    assert!(liella_heap_usable_size(m) >= 40);
    liella_heap_free(m);
}

// ------------------------------------------------------------------------------ secure memory (D-X.11)

#[test]
fn secure_zero_zeroes_exactly_its_slice() {
    // Clauses: s4_2_c_3 (D-X.11, D-X.10).
    for len in [0usize, 1, 7, 64, 4097] {
        let mut v = vec![0xa5u8; len + 2];
        assert_eq!(liella_secure_zero(v[1..].as_mut_ptr().cast(), len), 0);
        assert_eq!((v[0], v[len + 1]), (0xa5, 0xa5), "length {len}: nothing outside");
        assert!(v[1..=len].iter().all(|&b| b == 0), "length {len}: every byte inside");
    }
    assert_eq!(liella_secure_zero(std::ptr::null_mut(), 0), 0, "NULL with length 0");
    assert_eq!(liella_secure_zero(std::ptr::null_mut(), 1), neg(Errno::EFAULT));
    let mut b = [1u8; 4];
    assert_eq!(liella_secure_zero(b.as_mut_ptr().cast(), usize::MAX), neg(Errno::EFAULT), "beyond PTRDIFF_MAX");
    assert_eq!(b, [1; 4], "nothing written on failure");
    // explicit_bzero is the same function.
    let mut k = [0x5au8; 9];
    s04::explicit_bzero(k.as_mut_ptr().cast(), 9);
    assert_eq!(k, [0; 9]);
}

/// Watching a block's wipe is process-wide, and a released secure block's address can be
/// mapped again at once: the tests that use secure blocks run one at a time.
static WIPES: Mutex<()> = Mutex::new(());

fn wipe_lock() -> std::sync::MutexGuard<'static, ()> {
    WIPES.lock().unwrap_or_else(PoisonError::into_inner)
}

#[test]
fn heap_alloc_secure_honours_size_and_alignment() {
    // Trace: no spec clause (native secure heap blocks, LIELLA_NATIVE_1); D-X.11, D-X.10.
    // Its blocks may take a watched block's address once that is released.
    let _g = wipe_lock();
    for align in [1usize, 16, 4096, 1 << 16] {
        for size in [0usize, 1, 24, 5000, 100_000] {
            let p = liella_heap_alloc_secure(size, align);
            assert!(!p.is_null(), "size {size} align {align}");
            assert_eq!(p as usize % align.max(LIELLA_HEAP_MIN_ALIGN), 0, "size {size} align {align}");
            assert!(liella_heap_usable_size(p) >= size);
            assert!(contents(p, size).iter().all(|&b| b == 0), "a secure block reads as zero");
            fill(p, 0xa5, size);
            liella_heap_free(p);
        }
    }
    assert!(liella_heap_alloc_secure(16, 3).is_null(), "an alignment that is not a power of two");
    assert!(liella_heap_alloc_secure(16, 0).is_null());
    assert!(liella_heap_alloc_secure(usize::MAX - 64, 16).is_null(), "exhaustion is NULL, not an abort");
}

/// A secure block of `n` bytes holding `pattern`, watched (`selftest::watch_wipe`).
fn watched_secure(n: usize, pattern: u8) -> *mut core::ffi::c_void {
    let p = liella_heap_alloc_secure(n, 16);
    assert!(!p.is_null());
    fill(p, pattern, n);
    selftest::watch_wipe(p as usize);
    p
}

/// The watched block was zeroed once by its release: the whole mapping, holding the `n` bytes
/// of the pattern just before, and no non-zero byte just after.
fn assert_wiped(n: usize, how: &str) {
    let w = selftest::watched_wipe();
    assert_eq!(w.count, 1, "{how}: zeroed once ({w:?})");
    assert!(w.len >= n.max(1) && w.len % 4096 == 0, "{how}: the whole mapping ({w:?})");
    assert_eq!(w.dirty_before, n, "{how}: the pattern was there just before ({w:?})");
    assert_eq!(w.dirty_after, 0, "{how}: every byte zero just after ({w:?})");
}

#[test]
fn secure_blocks_are_zeroed_by_every_free() {
    // Clauses: s3_1_c_5, s3_1_c_8, s3_1_c_3_2, s3_1_t_0 (D-X.11).
    let _g = wipe_lock();
    type Free = fn(*mut core::ffi::c_void);
    let frees: [(&str, Free); 5] = [
        ("liella_heap_free", |p| liella_heap_free(p)),
        ("free", |p| s03::free(p)),
        ("cfree", |p| s03::cfree(p)),
        ("__libc_free", |p| s03::__libc_free(p)),
        ("realloc(p, 0)", |p| assert!(s03::realloc(p, 0).is_null())),
    ];
    for (how, free) in frees {
        for n in [1usize, 100, 4096, 70_000] {
            let p = watched_secure(n, 0x5a);
            free(p);
            assert_wiped(n, how);
        }
    }
    let p = watched_secure(100, 0x5a);
    assert!(liella_heap_realloc(p, 0).is_null());
    assert_wiped(100, "liella_heap_realloc(p, 0)");
    // An ordinary block is never zeroed by the heap: its free records nothing.
    let o = liella_heap_alloc(100_000, 16);
    selftest::watch_wipe(o as usize);
    liella_heap_free(o);
    assert_eq!(selftest::watched_wipe().count, 0, "an ordinary block is not wiped");
    // A free by another thread than the allocating one.
    let p = watched_secure(300, 0x11) as usize;
    std::thread::spawn(move || liella_heap_free(p as *mut core::ffi::c_void)).join().unwrap();
    assert_wiped(300, "a free on another thread");
}

#[test]
fn secure_blocks_stay_secure_through_realloc() {
    // Clauses: s3_1_c_3, s3_1_c_4, s3_1_f_4 (D-X.11).
    let _g = wipe_lock();
    type Realloc = fn(*mut core::ffi::c_void, usize) -> *mut core::ffi::c_void;
    let reallocs: [(&str, Realloc); 4] = [
        ("liella_heap_realloc", |p, n| liella_heap_realloc(p, n)),
        ("realloc", |p, n| s03::realloc(p, n)),
        ("__libc_realloc", |p, n| s03::__libc_realloc(p, n)),
        ("reallocarray", |p, n| s03::reallocarray(p, n / 4, 4)),
    ];
    for (how, realloc) in reallocs {
        // Within the block's mapping: in place, nothing zeroed.
        let p = watched_secure(100, 0x5a);
        assert_eq!(realloc(p, 4000), p, "{how}: in place");
        assert_eq!(selftest::watched_wipe().count, 0, "{how}: no release in place");
        // Beyond it: moved, the old block zeroed, the contents kept, the rest zero.
        let q = realloc(p, 100_000);
        assert!(!q.is_null() && q != p, "{how}: moved");
        assert_wiped(100, &format!("{how}, the block it left"));
        let c = contents(q, 100_000);
        assert!(c[..100].iter().all(|&b| b == 0x5a) && c[100..].iter().all(|&b| b == 0), "{how}: contents kept, the rest zero");
        // The new block is secure: its own free zeroes it.
        selftest::watch_wipe(q as usize);
        liella_heap_free(q);
        assert_wiped(100, &format!("{how}, the new block"));
    }
    // A failure leaves the block, still secure.
    let p = watched_secure(64, 0x22);
    assert!(liella_heap_realloc(p, usize::MAX - 64).is_null());
    assert_eq!(selftest::watched_wipe().count, 0);
    liella_heap_free(p);
    assert_wiped(64, "liella_heap_free after a failed realloc");
}

/// The block a `fork` child frees (its copy of the parent's).
static FORKED: AtomicUsize = AtomicUsize::new(0);

fn child_frees_its_copy(_: &CSys) -> i32 {
    let p = FORKED.load(Ordering::SeqCst);
    selftest::watch_wipe(p);
    liella_heap_free(p as *mut core::ffi::c_void);
    let w = selftest::watched_wipe();
    i32::from(!(w.count == 1 && w.dirty_before == 64 && w.dirty_after == 0))
}

#[test]
fn thread_exit_trim_and_fork_release_no_live_secure_block() {
    // Trace: no spec clause (secure blocks across thread exit, malloc_trim and fork); D-X.11, D-3.11, D-18.6.
    let _g = wipe_lock();
    let p = watched_secure(64, 0x33);
    // Another thread works with the heap and exits (its cache parked, D-3.11); then malloc_trim.
    std::thread::spawn(|| {
        let q = liella_heap_alloc(1 << 20, 16);
        liella_heap_free(q);
        selftest::park_heap_cache();
    })
    .join()
    .unwrap();
    s03::malloc_trim(0);
    // A child of the library's fork (the other threads' caches parked, D-18.6) frees its copy.
    FORKED.store(p as usize, Ordering::SeqCst);
    assert_eq!(selftest::in_library_child(child_frees_its_copy).unwrap(), ChildEnd::Exited(0), "the child's free zeroes its copy");
    assert_eq!(selftest::watched_wipe().count, 0, "nothing released the live block here");
    assert!(contents(p, 64).iter().all(|&b| b == 0x33), "the block is intact");
    // Freed by a thread that then exits: zeroed at the free, and by nothing after.
    let addr = p as usize;
    std::thread::spawn(move || {
        liella_heap_free(addr as *mut core::ffi::c_void);
        selftest::park_heap_cache();
    })
    .join()
    .unwrap();
    s03::malloc_trim(0);
    assert_wiped(64, "freed by an exiting thread");
}

// ------------------------------------------------------------------------------ environment

fn get(name: &[u8]) -> Result<Vec<u8>, i64> {
    let mut buf = vec![0u8; 8];
    loop {
        let n = liella_env_get(name.as_ptr(), name.len(), buf.as_mut_ptr(), buf.len());
        if n < 0 {
            return Err(n);
        }
        let n = n as usize;
        if n <= buf.len() {
            buf.truncate(n);
            return Ok(buf);
        }
        buf.resize(n, 0);
    }
}

#[test]
fn env_set_get_unset() {
    // Trace: no spec clause (native environment interface, LIELLA_NATIVE_1); D-X.10.
    let _g = env_lock();
    let (name, value) = (b"LIELLA_NATIVE_V1_A", b"a value longer than eight bytes");
    assert_eq!(liella_env_set(name.as_ptr(), name.len(), value.as_ptr(), value.len(), 1), 0);
    assert_eq!(get(name).unwrap(), value);
    // A buffer too small: the length, the buffer untouched.
    let mut small = [b'#'; 4];
    assert_eq!(liella_env_get(name.as_ptr(), name.len(), small.as_mut_ptr(), small.len()), value.len() as i64);
    assert_eq!(&small, b"####");
    assert_eq!(liella_env_get(name.as_ptr(), name.len(), std::ptr::null_mut(), 0), value.len() as i64, "NULL with length 0 asks for the length");
    // overwrite 0 keeps the value.
    assert_eq!(liella_env_set(name.as_ptr(), name.len(), b"x".as_ptr(), 1, 0), 0);
    assert_eq!(get(name).unwrap(), value);
    assert_eq!(liella_env_set(name.as_ptr(), name.len(), b"".as_ptr(), 0, 7), 0);
    assert_eq!(get(name).unwrap(), b"");
    // The POSIX view is the same environment.
    assert_eq!(liella_boundary::process::getenv(name).map(|_| ()), Some(()));
    assert_eq!(liella_env_unset(name.as_ptr(), name.len()), 0);
    assert_eq!(get(name), Err(neg(Errno::ENOENT)));
    assert_eq!(liella_env_unset(name.as_ptr(), name.len()), 0, "unsetting an unset name succeeds");
    // Invalid names and text.
    for bad in [&b""[..], b"A=B", b"A\0B"] {
        assert_eq!(liella_env_set(bad.as_ptr(), bad.len(), b"v".as_ptr(), 1, 1), neg(Errno::EINVAL), "{bad:?}");
    }
    assert_eq!(liella_env_set(name.as_ptr(), name.len(), b"v\0w".as_ptr(), 3, 1), neg(Errno::EINVAL), "a NUL in the value");
    assert_eq!(liella_env_unset(b"".as_ptr(), 0), neg(Errno::EINVAL));
    assert_eq!(liella_env_get(std::ptr::null(), 3, std::ptr::null_mut(), 0), neg(Errno::EFAULT));
    assert_eq!(liella_env_get(name.as_ptr(), name.len(), std::ptr::null_mut(), 5), neg(Errno::EFAULT));
}

#[test]
fn env_entries_are_enumerable() {
    // Trace: no spec clause (native environment interface, LIELLA_NATIVE_1); D-X.10.
    let _g = env_lock();
    let name = b"LIELLA_NATIVE_V1_ENUM";
    assert_eq!(liella_env_set(name.as_ptr(), name.len(), b"1".as_ptr(), 1, 1), 0);
    let n = liella_env_count();
    assert!(n >= 1);
    let mut found = 0;
    for i in 0..n as usize {
        let mut buf = vec![0u8; 4096];
        let len = liella_env_entry(i, buf.as_mut_ptr(), buf.len());
        assert!(len >= 0 && (len as usize) <= buf.len(), "entry {i}");
        buf.truncate(len as usize);
        assert!(buf.contains(&b'=') || buf.is_empty(), "entry {i}");
        if buf == b"LIELLA_NATIVE_V1_ENUM=1" {
            found += 1;
        }
    }
    assert_eq!(found, 1);
    assert_eq!(liella_env_entry(n as usize, std::ptr::null_mut(), 0), neg(Errno::ENOENT));
    assert_eq!(liella_env_unset(name.as_ptr(), name.len()), 0);
    assert_eq!(liella_env_count(), n - 1);
}

// ------------------------------------------------------------------------------ files

/// Removes a test's directory, with whatever it holds, when it goes: the tests remove it
/// themselves, and this covers a test that fails first.
struct Scratch(String);

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A fresh directory for one test, under the target directory, and its guard.
fn scratch(tag: &str) -> (Scratch, String) {
    let dir = format!("{}/native-v1-{}-{tag}", env!("CARGO_TARGET_TMPDIR"), std::process::id());
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(liella_fs_mkdir(dir.as_ptr(), dir.len(), 0o700), 0);
    (Scratch(dir.clone()), dir)
}

#[test]
fn fd_and_fs_round_trip() {
    // Trace: no spec clause (native descriptor and file-system interface, LIELLA_NATIVE_1); D-X.10.
    let (_scratch, dir) = scratch("io");
    let file = format!("{dir}/f");
    let fd = liella_fs_open(file.as_ptr(), file.len(), LIELLA_O_RDWR | LIELLA_O_CREAT | LIELLA_O_EXCL | LIELLA_O_CLOEXEC, 0o640);
    assert!(fd >= 0, "{fd}");
    let fd = fd as i32;
    assert_eq!(liella_fs_open(file.as_ptr(), file.len(), LIELLA_O_RDWR | LIELLA_O_CREAT | LIELLA_O_EXCL, 0o640), neg(Errno::EEXIST));
    assert_eq!(liella_fd_write(fd, b"hello world".as_ptr(), 11), 11);
    assert_eq!(liella_fd_pwrite(fd, b"W".as_ptr(), 1, 6), 1);
    let mut buf = [0u8; 11];
    assert_eq!(liella_fd_pread(fd, buf.as_mut_ptr(), 11, 0), 11);
    assert_eq!(&buf, b"hello World");
    assert_eq!(liella_fd_seek(fd, 0, LIELLA_SEEK_CUR), 11, "pread and pwrite leave the offset");
    assert_eq!(liella_fd_seek(fd, 6, LIELLA_SEEK_SET), 6);
    let mut rest = [0u8; 16];
    assert_eq!(liella_fd_read(fd, rest.as_mut_ptr(), rest.len()), 5);
    assert_eq!(&rest[..5], b"World");
    assert_eq!(liella_fd_read(fd, rest.as_mut_ptr(), rest.len()), 0, "end of file");
    assert_eq!(liella_fd_seek(fd, -1, LIELLA_SEEK_SET), neg(Errno::EINVAL));
    assert_eq!(liella_fd_seek(fd, 0, LIELLA_SEEK_END), 11);
    let mut st = LiellaStat::default();
    assert_eq!(liella_fd_stat(fd, Some(&mut st)), 0);
    assert_eq!((st.size, st.mode & LIELLA_S_IFMT, st.mode & 0o700, st.nlink, st.reserved), (11, LIELLA_S_IFREG, 0o600, 1, 0));
    assert!(st.mtime.sec > 1_600_000_000 && (0..1_000_000_000).contains(&st.mtime.nsec));
    assert_eq!(liella_fd_stat(fd, None), neg(Errno::EFAULT));
    assert_eq!(liella_fd_read(fd, std::ptr::null_mut(), 1), neg(Errno::EFAULT));
    assert_eq!(liella_fd_close(fd), 0);
    assert_eq!(liella_fd_close(fd), neg(Errno::EBADF));
    assert_eq!(liella_fd_write(-1, b"x".as_ptr(), 1), neg(Errno::EBADF));

    // Paths: stat, a symbolic link with and without following it, rename, unlink, rmdir.
    let mut by_path = LiellaStat::default();
    assert_eq!(liella_fs_stat(file.as_ptr(), file.len(), 0, Some(&mut by_path)), 0);
    assert_eq!((by_path.ino, by_path.dev, by_path.size), (st.ino, st.dev, 11));
    let link = format!("{dir}/l");
    std::os::unix::fs::symlink("f", &link).unwrap();
    let mut ls = LiellaStat::default();
    assert_eq!(liella_fs_stat(link.as_ptr(), link.len(), LIELLA_FS_NOFOLLOW, Some(&mut ls)), 0);
    assert_eq!(ls.mode & LIELLA_S_IFMT, LIELLA_S_IFLNK);
    assert_eq!(liella_fs_stat(link.as_ptr(), link.len(), 0, Some(&mut ls)), 0);
    assert_eq!(ls.ino, st.ino);
    let moved = format!("{dir}/g");
    assert_eq!(liella_fs_rename(file.as_ptr(), file.len(), moved.as_ptr(), moved.len()), 0);
    assert_eq!(liella_fs_stat(file.as_ptr(), file.len(), 0, Some(&mut ls)), neg(Errno::ENOENT));
    assert_eq!(liella_fs_unlink(moved.as_ptr(), moved.len()), 0);
    assert_eq!(liella_fs_unlink(link.as_ptr(), link.len()), 0);
    let sub = format!("{dir}/d");
    assert_eq!(liella_fs_mkdir(sub.as_ptr(), sub.len(), 0o755), 0);
    assert_eq!(liella_fs_mkdir(sub.as_ptr(), sub.len(), 0o755), neg(Errno::EEXIST));
    assert_eq!(liella_fs_stat(sub.as_ptr(), sub.len(), 0, Some(&mut ls)), 0);
    assert_eq!(ls.mode & LIELLA_S_IFMT, LIELLA_S_IFDIR);
    assert_eq!(liella_fs_unlink(sub.as_ptr(), sub.len()), neg(Errno::EISDIR));
    let d = liella_fs_open(sub.as_ptr(), sub.len(), LIELLA_O_RDONLY | LIELLA_O_DIRECTORY, 0);
    assert!(d >= 0);
    assert_eq!(liella_fd_close(d as i32), 0);
    assert_eq!(liella_fs_rmdir(sub.as_ptr(), sub.len()), 0);
    assert_eq!(liella_fs_rmdir(dir.as_ptr(), dir.len()), 0);
    assert_eq!(liella_fs_rmdir(dir.as_ptr(), dir.len()), neg(Errno::ENOENT));
}

#[test]
fn paths_are_byte_slices() {
    // Trace: no spec clause (native paths as byte slices, LIELLA_NATIVE_1); D-X.10, D-H1.
    // Not NUL-terminated: only `len` bytes are the path.
    let (_scratch, dir) = scratch("paths");
    let padded = format!("{dir}/x/trailing");
    assert_eq!(liella_fs_mkdir(padded.as_ptr(), dir.len() + 2, 0o700), 0, "the first len bytes only");
    let x = format!("{dir}/x");
    let mut st = LiellaStat::default();
    assert_eq!(liella_fs_stat(x.as_ptr(), x.len(), 0, Some(&mut st)), 0);
    assert_eq!(liella_fs_stat(b"a\0b".as_ptr(), 3, 0, Some(&mut st)), neg(Errno::EINVAL), "an interior NUL");
    let long = vec![b'a'; 4096];
    assert_eq!(liella_fs_stat(long.as_ptr(), long.len(), 0, Some(&mut st)), neg(Errno::ENAMETOOLONG));
    assert_eq!(liella_fs_stat(std::ptr::null(), 1, 0, Some(&mut st)), neg(Errno::EFAULT));
    assert_eq!(liella_fs_stat(std::ptr::null(), 0, 0, Some(&mut st)), neg(Errno::ENOENT), "the empty path");
    assert_eq!(liella_fs_stat(x.as_ptr(), x.len(), 0, None), neg(Errno::EFAULT));
    assert_eq!(liella_fs_rename(x.as_ptr(), x.len(), std::ptr::null(), 2), neg(Errno::EFAULT));
    assert_eq!(liella_fs_rmdir(x.as_ptr(), x.len()), 0);
    assert_eq!(liella_fs_rmdir(dir.as_ptr(), dir.len()), 0);
}

// ------------------------------------------------------------------------------ clocks

#[test]
fn clocks_read_the_time() {
    // Trace: no spec clause (native clock interface, LIELLA_NATIVE_1); D-X.10.
    let mut a = LiellaTimespec::default();
    let mut b = LiellaTimespec::default();
    assert_eq!(liella_clock_monotonic(Some(&mut a)), 0);
    assert_eq!(liella_clock_monotonic(Some(&mut b)), 0);
    assert!((b.sec, b.nsec) >= (a.sec, a.nsec), "monotonic");
    let mut now = LiellaTimespec::default();
    assert_eq!(liella_clock_realtime(Some(&mut now)), 0);
    let std_now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64;
    assert!((now.sec - std_now).abs() <= 2 && (0..1_000_000_000).contains(&now.nsec));
    assert_eq!(liella_clock_realtime(None), neg(Errno::EFAULT));
    assert_eq!(liella_clock_monotonic(None), neg(Errno::EFAULT));
}

// ------------------------------------------------------------------------------ time zone

fn set_tz(tz: &str) {
    assert_eq!(liella_env_set(b"TZ".as_ptr(), 2, tz.as_ptr(), tz.len(), 1), 0);
}

#[test]
fn tz_converts_both_ways() {
    // Trace: no spec clause (native time-zone interface, LIELLA_NATIVE_1); D-X.10.
    let _g = env_lock();
    set_tz("CET-1CEST,M3.5.0,M10.5.0/3");
    let mut tm = LiellaTm::default();
    assert_eq!(liella_tz_to_local(0, Some(&mut tm)), 0);
    assert_eq!(
        (tm.year, tm.month, tm.day, tm.hour, tm.minute, tm.second, tm.weekday, tm.yearday, tm.isdst, tm.utc_offset),
        (1970, 1, 1, 1, 0, 0, 4, 0, 0, 3600)
    );
    assert_eq!((tm.abbr_len, &tm.abbr[..3], tm.abbr[3]), (3, &b"CET"[..], 0));
    // 2026-07-01 12:00:00 UTC is 14:00 CEST.
    assert_eq!(liella_tz_to_local(1_782_907_200, Some(&mut tm)), 0);
    assert_eq!((tm.year, tm.month, tm.day, tm.hour, tm.isdst, tm.utc_offset, tm.abbr_len), (2026, 7, 1, 14, 1, 7200, 4));
    assert_eq!(&tm.abbr[..4], b"CEST");
    assert_eq!(tm.yearday, 181);
    // Back again, and with fields that carry (month 13 of 2025 is January 2026).
    let mut t = 0i64;
    tm.weekday = 99;
    assert_eq!(liella_tz_from_local(Some(&mut tm), Some(&mut t)), 0);
    assert_eq!((t, tm.weekday), (1_782_907_200, 3));
    let mut carry = LiellaTm { year: 2025, month: 13, day: 1, hour: 0, minute: 0, second: 0, isdst: -1, ..LiellaTm::default() };
    assert_eq!(liella_tz_from_local(Some(&mut carry), Some(&mut t)), 0);
    assert_eq!((carry.year, carry.month, carry.day, carry.utc_offset, t), (2026, 1, 1, 3600, 1_767_222_000));
    // Far years (the zone conversion's range is `struct tm`'s: years that fit an `int`).
    assert_eq!(liella_tz_to_local(200_000_000 * 31_556_952, Some(&mut tm)), 0);
    assert!(tm.year > 200_000_000 && tm.year < i64::from(i32::MAX));
    assert_eq!(liella_tz_to_local(i64::MAX, Some(&mut tm)), neg(Errno::EOVERFLOW));
    // NULL out-parameters, and nothing written on failure.
    assert_eq!(liella_tz_to_local(0, None), neg(Errno::EFAULT));
    let before = tm;
    assert_eq!(liella_tz_from_local(Some(&mut tm), None), neg(Errno::EFAULT));
    assert_eq!(tm, before);
    // The zone follows TZ.
    set_tz("UTC0");
    assert_eq!(liella_tz_to_local(0, Some(&mut tm)), 0);
    assert_eq!((tm.hour, tm.utc_offset, tm.abbr_len, &tm.abbr[..3]), (0, 0, 3, &b"UTC"[..]));
    assert_eq!(liella_env_unset(b"TZ".as_ptr(), 2), 0);
}

// ------------------------------------------------------------------------------ locale

#[test]
fn locale_name_and_decimal_point() {
    // Trace: no spec clause (native locale interface, LIELLA_NATIVE_1); D-X.10.
    let mut buf = [0u8; 64];
    // A test process never calls setlocale: the C locale.
    assert_eq!(liella_locale_name(LIELLA_LC_ALL, buf.as_mut_ptr(), buf.len()), 1);
    assert_eq!(buf[0], b'C');
    assert_eq!(liella_locale_name(LIELLA_LC_NUMERIC, buf.as_mut_ptr(), buf.len()), 1);
    let mut tiny = [b'#'; 0];
    assert_eq!(liella_locale_name(LIELLA_LC_CTYPE, tiny.as_mut_ptr(), 0), 1, "the length without a buffer");
    assert_eq!(liella_locale_name(99, buf.as_mut_ptr(), buf.len()), neg(Errno::EINVAL));
    assert_eq!(liella_locale_name(-1, buf.as_mut_ptr(), buf.len()), neg(Errno::EINVAL));
    assert_eq!(liella_locale_name(LIELLA_LC_ALL, std::ptr::null_mut(), 4), neg(Errno::EFAULT));
    assert_eq!(liella_locale_decimal_point(buf.as_mut_ptr(), buf.len()), 1);
    assert_eq!(buf[0], b'.');
    assert_eq!(liella_locale_decimal_point(std::ptr::null_mut(), 0), 1);
}

// ------------------------------------------------------------------------------ process

#[test]
fn process_identity_and_arguments() {
    // Trace: no spec clause (native process interface, LIELLA_NATIVE_1); D-X.10.
    assert_eq!(liella_process_id(), i64::from(std::process::id() as i32));
    // The host C library started this process: Liella's runtime recorded no arguments.
    assert_eq!(liella_process_arg_count(), 0);
    let mut buf = [0u8; 8];
    assert_eq!(liella_process_arg(0, buf.as_mut_ptr(), buf.len()), neg(Errno::ENOENT));
    assert_eq!(liella_process_arg(0, std::ptr::null_mut(), 8), neg(Errno::EFAULT));
}

extern "C" fn exit_handler() {
    liella_kernel::sys::Sys::exit_group(&liella_boundary::sys::CSys, 9)
}

#[test]
fn process_exit_runs_the_exit_handlers() {
    // Trace: no spec clause (native process exit, LIELLA_NATIVE_1); D-X.10.
    assert_eq!(selftest::in_child(|_| liella_process_exit(7)).unwrap(), ChildEnd::Exited(7));
    // A handler registered as `atexit` registers it: it ends the child with 9 instead. The
    // parent's registry is never run (the host C library ends the test process).
    assert!(liella_boundary::process::register(liella_boundary::process::entry(liella_legacy::exit_handlers::ATEXIT, exit_handler as *const () as usize, 0, 0)));
    assert_eq!(selftest::in_child(|_| liella_process_exit(7)).unwrap(), ChildEnd::Exited(9));
}

// ------------------------------------------------------------------------------ name-service cache

#[test]
fn nscache_calls_follow_the_conventions() {
    // Trace: no spec clause (native name-service cache calls, LIELLA_NATIVE_1); D-X.10, D-16.10.
    // The semantics are tests/nscache.rs and the products' crates/core/tests/nscache_products.rs.
    let mut s = LiellaNscacheInfo::default();
    assert_eq!(liella_nscache_status(Some(&mut s)), 0);
    assert_eq!(liella_nscache_status(None), neg(Errno::EFAULT));
    assert!(s.enabled == 1 && s.categories == 0x3f, "enabled with every category in a test process");
    assert_eq!(liella_nscache_category(LIELLA_NSCACHE_NETWORKS + 1, 1), neg(Errno::EINVAL));
    assert_eq!(liella_nscache_invalidate(LIELLA_NSCACHE_NETWORKS + 1), neg(Errno::EINVAL));
    assert_eq!(liella_nscache_invalidate(LIELLA_NSCACHE_ALL), 0);
    assert_eq!(liella_nscache_category(LIELLA_NSCACHE_HOSTS, 1), 1);
}

// ------------------------------------------------------------------------------ asynchronous I/O

#[test]
fn aio_use_threads_follows_the_conventions() {
    // Trace: no spec clause (native AIO engine switch, LIELLA_NATIVE_1); D-X.10, D-11.11.
    // In a child: the choice holds for the process. The engine selection itself is
    // crates/core/tests/aio_engine.rs.
    let child = |_: &CSys| -> i32 { i32::from(!(liella_aio_use_threads() == 0 && liella_aio_use_threads() == 0)) };
    assert_eq!(selftest::in_child(child).unwrap(), ChildEnd::Exited(0), "0 before the first request, and again");
}

// ------------------------------------------------------------------------------ getpass

#[test]
fn getpass_set_limit_follows_the_conventions() {
    // Trace: no spec clause (native getpass bound, LIELLA_NATIVE_1); D-X.10, D-17.8.
    // In a child: the bound is process-wide. getpass itself under the bound is
    // tests/s17_users_symbols.rs (sym_getpass_bound).
    let child = |_: &CSys| -> i32 {
        let ok = liella_getpass_set_limit(10) == LIELLA_GETPASS_DEFAULT_LIMIT as i64
            && liella_getpass_set_limit(isize::MAX as usize) == 10
            && liella_getpass_set_limit(isize::MAX as usize + 1) == neg(Errno::EINVAL)
            && liella_getpass_set_limit(0) == isize::MAX as i64
            && liella_getpass_set_limit(0) == LIELLA_GETPASS_DEFAULT_LIMIT as i64;
        i32::from(!ok)
    };
    assert_eq!(selftest::in_child(child).unwrap(), ChildEnd::Exited(0), "the previous bound; EINVAL above PTRDIFF_MAX; 0 restores the default");
    assert_eq!(LIELLA_GETPASS_DEFAULT_LIMIT, 1 << 20, "1 MiB (D-17.8)");
}

// ------------------------------------------------------------------------------ products

#[test]
fn products_export_the_node_and_pass_the_c_program() {
    // Trace: no spec clause (the LIELLA_NATIVE_1 version node in the products); D-X.10.
    let _g = env_lock();
    for (name, (node, _, _)) in rows("fn") {
        assert_eq!(node, "LIELLA_NATIVE_1");
        products::archived(name);
        products::exported_at(name, node);
    }
    products::conformance(&["native_v1"]);
}

// opaque-ok: audited in GATE-SPEED-4/5: the direct calls here use their results and pass at opt-level 1 and 2 in both pass types; any case LLVM folds goes through selftest::opaque.
