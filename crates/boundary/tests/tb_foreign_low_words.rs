//! D-X.60: in a process the runtime did not start (this test process: another C library's
//! loader made every thread block), the words at 0x80-0x9b of a thread block (`errno`, TID,
//! `thread_local` destructor list, heap cache, NUMA hint) are that library's data. Liella reads
//! none of them as a pointer or a decision and writes none: with garbage in all of them (the
//! heap cache word aimed at page zero, so a use of it as a pointer faults) `errno` still
//! round-trips through one per-thread address outside the block, the heap allocates and frees
//! through its slow path, a destructor registers, and the words stay as they were. Nothing but
//! Liella runs on that thread while the garbage is in place, and the block is restored before
//! anything is asserted; the other test runs in a forked child, which has only its own thread.
// opaque-ok: the test is about Liella's own `malloc`/`free` in a process another library started.
#![no_builtins]
#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::arithmetic_side_effects, missing_docs)]

use core::sync::atomic::Ordering::Relaxed;

use liella_boundary::posix::s02_diagnostics as dg;
use liella_boundary::posix::s03_memory as mem;
use liella_boundary::{process, tb};
use liella_rt::tls::{ThreadBlock, BLOCK_SIZE};

/// The block's words at 0x80-0x9b: errno, TID, destructor list, heap cache, node.
fn low(b: &ThreadBlock) -> (i32, u32, usize, usize, u32) {
    (b.errno.load(Relaxed), b.tid.load(Relaxed), b.tls_dtors.load(Relaxed), b.heap_cache.load(Relaxed), b.node.load(Relaxed))
}

fn put(b: &ThreadBlock, v: (i32, u32, usize, usize, u32)) {
    b.errno.store(v.0, Relaxed);
    b.tid.store(v.1, Relaxed);
    b.tls_dtors.store(v.2, Relaxed);
    b.heap_cache.store(v.3, Relaxed);
    b.node.store(v.4, Relaxed);
}

extern "C" fn no_dtor(_: *mut core::ffi::c_void) {}

/// What the test asks Liella, with no allocation of the harness's.
fn ask() -> [usize; 6] {
    let tp = tb::tp();
    let e = dg::__errno_location();
    tb::set_own_errno(4242);
    let again = dg::__errno_location();
    let read = tb::own_errno();
    let p = mem::malloc(48);
    let q = mem::malloc(4000);
    let ok = usize::from(!p.is_null() && !q.is_null());
    mem::free(p);
    mem::free(q);
    let registered = usize::from(process::thread_atexit(no_dtor as extern "C" fn(*mut core::ffi::c_void) as usize, 0, 0));
    [usize::from(!(tp..tp + BLOCK_SIZE).contains(&(e as usize))), usize::from(e == again), read as usize, ok, registered, usize::from(dg::__errno_location() == e)]
}

#[test]
fn foreign_block_low_words_neither_trusted_nor_written() {
    // Trace: no spec clause (D-X.60: a thread block another library made).
    std::thread::spawn(|| {
        let b = tb::current();
        // The garbage first: the thread's first allocation (its cache, its node hint) and first
        // destructor happen with it in place.
        let saved = low(b);
        let planted = (0x5a5a_5a5a, 0x7fff_fff0, 0x18, 0x8, 0xa5a5);
        put(b, planted);
        let during = ask();
        let after = low(b);
        put(b, saved);
        let first = ask();
        let expect = [1, 1, 4242, 1, 1, 1];
        assert_eq!(first, expect, "the answers with the block as the other library left it");
        assert_eq!(during, expect, "the answers with garbage in the block's words 0x80-0x9b");
        assert_eq!(after, planted, "no word at 0x80-0x9b of the other library's block was written");
    })
    .join()
    .unwrap();
}

/// In a forked child (one thread; no allocation, so no lock another test thread held at the fork
/// is met): with no room in the registry the thread gets the shared spare words, which hold no
/// heap cache, and `errno` still round-trips through one address outside the block.
fn spare_child(_: &liella_boundary::sys::CSys) -> i32 {
    liella_boundary::selftest::starve_registry(true);
    if liella_boundary::selftest::heap_cache_word() != (false, 0) {
        return 1;
    }
    let tp = tb::tp();
    let e = dg::__errno_location();
    tb::set_own_errno(77);
    if tb::own_errno() != 77 || dg::__errno_location() != e {
        return 2;
    }
    if (tp..tp + BLOCK_SIZE).contains(&(e as usize)) {
        return 3;
    }
    0
}

#[test]
fn spare_words_hold_no_heap_cache() {
    // Trace: no spec clause (D-X.60, #4837: the spare words when the registry has no room).
    let end = liella_boundary::selftest::in_child(spare_child).unwrap();
    assert_eq!(end, liella_boundary::selftest::ChildEnd::Exited(0), "spare words: 1 = a heap cache slot, 2 = errno did not round-trip, 3 = errno in the block");
}
