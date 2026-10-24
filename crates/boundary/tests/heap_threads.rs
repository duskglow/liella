//! The heap under real threads (D-3.11–D-3.13), through the exported `malloc` family: thread
//! caches, remote frees handed between threads, collection, parking and taking over a cache,
//! and reuse of large blocks. Each block is filled with its owner's tag through the exported
//! `memset` and checked through `memchr` before it is freed, so two live blocks that overlapped
//! would be caught. A binary of its own: the statistics are process-wide.
#![cfg(feature = "selftest")]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use std::sync::{Arc, Barrier, Mutex, MutexGuard};

use liella_abi::ctypes::{c_int, c_void};
use liella_boundary::posix::s03_memory as m;
use liella_boundary::heap::{rust_alloc, rust_alloc_zeroed, rust_dealloc, rust_realloc};
use liella_boundary::posix::s04_strings as s;
use liella_rt::heap::{ALIGN, CLASSES};
use liella_boundary::selftest;

struct Gen(u64);
impl Gen {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
}

/// A live block: address, length, fill byte.
#[derive(Clone, Copy)]
struct Live(usize, usize, u8);

fn fill(b: Live) {
    s::memset(b.0 as *mut c_void, c_int::from(b.2), b.1);
}

/// Every byte still holds the tag (no other block was handed out over it).
fn check(b: Live) {
    let p = b.0 as *const c_void;
    for x in 0..=255u8 {
        if x != b.2 && b.1 > 0 {
            assert!(s::memchr(p, c_int::from(x), b.1).is_null(), "block {:#x} ({} bytes, tag {}) holds a foreign byte {x}", b.0, b.1, b.2);
        }
    }
}

fn quick_check(b: Live) {
    // The first, middle and last bytes (the full check runs on a sample).
    for off in [0, b.1 / 2, b.1.saturating_sub(1)] {
        if b.1 > 0 {
            assert!(!s::memchr((b.0 + off) as *const c_void, c_int::from(b.2), 1).is_null(), "block {:#x} byte {off}", b.0);
        }
    }
}

fn size(g: &mut Gen) -> usize {
    match g.below(20) {
        0..=13 => g.below(200) as usize,
        14..=17 => g.below(5000) as usize,
        18 => g.below(70_000) as usize,
        _ => 70_000 + g.below(3_000_000) as usize,
    }
}

/// The pools and statistics are process-wide: one test at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn in_use() -> u64 {
    m::mallinfo2().uordblks
}

#[test]
fn threads_allocate_free_locally_and_remotely_without_overlap() {
    // Clauses: s3_1_t_0, s3_1_g_1, s3_1_g_4, s3_1_c_1, s3_1_c_3, s3_1_c_5, s3_1_c_7, s3_2_c_6,
    // s3_2_c_2 (D-3.11).
    let _g = serial();
    const THREADS: usize = 8;
    const STEPS: usize = 60_000;
    let base = in_use();
    // Each thread's inbox: blocks other threads hand it to free (remote frees).
    let inboxes: Arc<Vec<Mutex<Vec<Live>>>> = Arc::new((0..THREADS).map(|_| Mutex::new(Vec::new())).collect());
    let start = Arc::new(Barrier::new(THREADS));
    let handles: Vec<_> = (0..THREADS)
        .map(|t| {
            let inboxes = Arc::clone(&inboxes);
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                let mut g = Gen(0x9E37 + t as u64 * 7919);
                let mut live: Vec<Live> = Vec::new();
                start.wait();
                for step in 0..STEPS {
                    match g.below(10) {
                        0..=4 => {
                            let n = size(&mut g);
                            let p = m::malloc(n) as usize;
                            assert_ne!(p, 0);
                            assert_eq!(p % 16, 0);
                            assert!(m::malloc_usable_size(p as *mut c_void) >= n);
                            let b = Live(p, n, (t * 31 + step) as u8 | 1);
                            fill(b);
                            live.push(b);
                        }
                        5..=6 if !live.is_empty() => {
                            let b = live.swap_remove(g.below(live.len() as u64) as usize);
                            if step % 64 == 0 { check(b) } else { quick_check(b) }
                            m::free(b.0 as *mut c_void);
                        }
                        7 if !live.is_empty() => {
                            // Hand a block to another thread, which frees it.
                            let b = live.swap_remove(g.below(live.len() as u64) as usize);
                            let to = (t + 1 + g.below(THREADS as u64 - 1) as usize) % THREADS;
                            inboxes[to].lock().unwrap().push(b);
                        }
                        8 => {
                            let mine: Vec<Live> = std::mem::take(&mut *inboxes[t].lock().unwrap());
                            for b in mine {
                                quick_check(b);
                                m::free(b.0 as *mut c_void);
                            }
                        }
                        9 if !live.is_empty() => {
                            let i = g.below(live.len() as u64) as usize;
                            let b = live[i];
                            quick_check(b);
                            let n = size(&mut g);
                            let q = m::realloc(b.0 as *mut c_void, n.max(1)) as usize;
                            assert_ne!(q, 0);
                            let nb = Live(q, n.max(1).min(b.1), b.2);
                            quick_check(nb);
                            live[i] = Live(q, n.max(1), b.2);
                            fill(live[i]);
                        }
                        _ => {}
                    }
                }
                start.wait();
                // Drain: own blocks, then whatever was handed over.
                for b in live {
                    check(b);
                    m::free(b.0 as *mut c_void);
                }
                start.wait();
                let mine: Vec<Live> = std::mem::take(&mut *inboxes[t].lock().unwrap());
                for b in mine {
                    check(b);
                    m::free(b.0 as *mut c_void);
                }
                // As a Liella thread's exit does.
                selftest::park_heap_cache();
            })
        })
        .collect();
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(m::malloc_trim(0), 1, "kept and empty memory goes back");
    // With `export`, this binary's own `malloc` is Liella's: the test harness and the standard
    // library allocate from the same heap, so only their small residue may differ.
    if cfg!(feature = "export") {
        assert!(in_use().abs_diff(base) < 64 << 10, "every block came back (within the harness's own allocations)");
    } else {
        assert_eq!(in_use(), base, "every block came back, including remote frees to parked caches");
    }
}

/// Run the calling thread on CPU 0 only.
fn pin_to_cpu0() {
    let mut mask = [0u64; 16];
    mask[0] = 1;
    let r = liella_boundary::posix::s24_system::sched_setaffinity(0, size_of_val(&mask), mask.as_ptr().cast());
    assert_eq!(r, 0, "sched_setaffinity");
}

#[test]
fn a_new_thread_takes_over_a_parked_cache() {
    // Trace: no spec clause (taking over a parked thread cache is internal); D-3.11, D-3.12.
    let _g = serial();
    // The blocks must go back to the thread's cache at once: perturbation (the test runner's
    // poison) would hold them in its quarantine and free them later from another thread.
    assert_eq!(m::mallopt(-6, 0), 1, "M_PERTURB off");
    // A thread allocates, parks its cache; the next thread on its node continues with the
    // same slabs, so its first block of the class comes from where the first thread left off.
    // (A class the standard library's thread start does not use: with `export` it allocates
    // through Liella too.) Both threads run on CPU 0: caches are parked per NUMA node (D-3.11),
    // and on a machine with several nodes the second thread could otherwise start on another.
    // The test harness's own threads start and exit on that node too, parking or adopting a
    // cache between the two, so the check does not compare addresses alone: right before the
    // second thread starts the test lists the caches parked at that moment, and the second
    // thread must have taken over one of them (it did not make a new cache). When it took the
    // first thread's own cache, its block must also be the first thread's (same slab).
    let first = || {
        std::thread::spawn(|| {
            pin_to_cpu0();
            let p = m::malloc(20_000) as usize;
            m::free(p as *mut c_void);
            let id = selftest::heap_cache_id();
            selftest::park_heap_cache();
            (p, id)
        })
        .join()
        .unwrap()
    };
    let second = |parked: Vec<u64>| {
        std::thread::spawn(move || {
            pin_to_cpu0();
            let p = m::malloc(20_000) as usize;
            let id = selftest::heap_cache_id();
            m::free(p as *mut c_void);
            selftest::park_heap_cache();
            (p, id, parked)
        })
        .join()
        .unwrap()
    };
    let mut seen = Vec::new();
    let mut taken = false;
    for _ in 0..5 {
        let (p, id) = first();
        let mut ids = [0u64; 4096];
        let n = selftest::parked_heap_cache_ids(&mut ids);
        assert!(n <= ids.len(), "{n} parked caches");
        let (q, id2, parked) = second(ids[..n].to_vec());
        // A cache that was parked when the thread started and is not the first thread's own
        // was parked by another thread meanwhile: taking it over is still a takeover.
        if parked.contains(&id2) && (id2 != id || p == q) {
            taken = true;
            break;
        }
        seen.push((p, id, q, id2, parked.len()));
    }
    assert!(taken, "the second thread never took over a cache parked before it started: (block, cache) pairs {seen:x?}");
}

#[test]
fn large_blocks_are_reused_within_a_thread() {
    // Clauses: s3_1_c_2, s3_1_g_2 (D-3.13).
    let _g = serial();
    std::thread::spawn(|| {
        let a = m::malloc(1 << 20) as usize;
        s::memset(a as *mut c_void, 0x5A, 1 << 20);
        m::free(a as *mut c_void);
        let b = m::malloc(1 << 20) as usize;
        assert_eq!(a, b, "the kept block is reused");
        // calloc of a reused block is zeroed by the boundary.
        m::free(b as *mut c_void);
        let c = m::calloc(1, 1 << 20) as usize;
        assert_eq!(c, a);
        assert!(s::memchr(c as *const c_void, 0x5A, 1 << 20).is_null(), "calloc cleared the kept block");
        m::free(c as *mut c_void);
        selftest::park_heap_cache();
    })
    .join()
    .unwrap();
}

// ------------------------------------------------------------------ Rust's `realloc` (HEAP-CUTS C2)

/// `n` bytes of a pattern particular to `seed`.
fn pattern(seed: u8, n: usize) -> Vec<u8> {
    (0..n).map(|i| (i as u8).wrapping_mul(31).wrapping_add(seed) ^ ((i >> 8) as u8)).collect()
}

fn put(addr: usize, bytes: &[u8]) {
    s::memcpy(addr as *mut c_void, bytes.as_ptr().cast(), bytes.len());
}

fn get(addr: usize, n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    s::memcpy(v.as_mut_ptr().cast(), addr as *const c_void, n);
    v
}

/// The sizes around every class boundary, and some large ones.
fn boundary_sizes() -> Vec<usize> {
    let mut v = vec![1, 2, 15];
    for &c in &CLASSES {
        v.extend([c - 1, c, c + 1]);
    }
    v.extend([100_000, 300_000, 1 << 20]);
    v
}

#[test]
fn rust_realloc_keeps_the_contents_across_every_class_boundary() {
    // Trace: no spec clause (Rust's internal allocations, D-3.1); HEAP-CUTS C2.
    let _g = serial();
    let sizes = boundary_sizes();
    let (mut in_place, mut moved) = (0, 0);
    for (n, &from) in sizes.iter().enumerate() {
        // Grow to each of a few larger sizes and shrink to each of a few smaller ones.
        for &to in sizes.iter().skip(n + 1).take(4).chain(sizes.iter().take(n).rev().take(4)) {
            let seed = (from ^ to) as u8;
            let a = rust_alloc(from, ALIGN);
            assert_ne!(a, 0);
            put(a, &pattern(seed, from));
            let b = rust_realloc(a, from, ALIGN, to);
            assert_ne!(b, 0, "{from} -> {to}");
            assert_eq!(b % ALIGN, 0);
            if a == b { in_place += 1 } else { moved += 1 }
            let keep = from.min(to);
            assert!(get(b, keep) == pattern(seed, keep), "{from} -> {to}: contents kept");
            // The whole new size is the block's: writing it all is checked by the neighbour test.
            put(b, &pattern(seed.wrapping_add(1), to));
            assert!(get(b, to) == pattern(seed.wrapping_add(1), to));
            rust_dealloc(b, to);
        }
    }
    assert!(in_place > 0 && moved > 0, "both paths ran: {in_place} in place, {moved} moved");
}

#[test]
fn rust_realloc_in_place_never_reaches_the_neighbour() {
    // Trace: no spec clause (Rust's internal allocations, D-3.1); HEAP-CUTS C2.
    let _g = serial();
    for (ci, &class) in CLASSES.iter().enumerate().take(24) {
        // Consecutive blocks of one class are neighbours in its slab.
        let size = if ci == 0 { 1 } else { CLASSES[ci - 1] + 1 };
        let blocks: Vec<usize> = (0..8).map(|_| rust_alloc(size, ALIGN)).collect();
        for (i, &b) in blocks.iter().enumerate() {
            put(b, &pattern(i as u8, size));
        }
        let mid = blocks[4];
        // Grow within the class (in place), then just past it (must move).
        let same = rust_realloc(mid, size, ALIGN, class);
        assert_eq!(same, mid, "class {class}: a growth inside the class stays in place");
        put(same, &pattern(0xEE, class));
        let grown = rust_realloc(same, class, ALIGN, class + 1);
        assert_ne!(grown, same, "class {class}: a growth past the class moves");
        assert!(get(grown, class) == pattern(0xEE, class), "class {class}: moved contents");
        put(grown, &pattern(0xDD, class + 1));
        for (i, &b) in blocks.iter().enumerate() {
            if i != 4 {
                assert!(get(b, size) == pattern(i as u8, size), "class {class}: neighbour {i} intact");
            }
        }
        assert!(get(grown, class + 1) == pattern(0xDD, class + 1), "class {class}: the moved block holds its new size");
        rust_dealloc(grown, class + 1);
        for (i, &b) in blocks.iter().enumerate() {
            if i != 4 {
                rust_dealloc(b, size);
            }
        }
    }
}

#[test]
fn rust_realloc_of_a_block_from_another_thread_and_at_a_large_alignment() {
    // Trace: no spec clause (Rust's internal allocations, D-3.1); HEAP-CUTS C2.
    let _g = serial();
    let sizes = [24usize, 200, 5000, 70_000];
    let theirs: Vec<usize> = std::thread::spawn(move || {
        sizes.iter().map(|&n| {
            let a = rust_alloc(n, ALIGN);
            put(a, &pattern(n as u8, n));
            a
        }).collect()
    })
    .join()
    .unwrap();
    for (&a, &n) in theirs.iter().zip(&sizes) {
        // A chain on this thread: shrink, grow a little, grow a lot; the contents follow.
        let (mut cur, mut len) = (a, n);
        for to in [n / 3 + 1, n + 7, 3 * n] {
            cur = rust_realloc(cur, len, ALIGN, to);
            let keep = len.min(to);
            assert!(get(cur, keep) == pattern(n as u8, keep), "{n}: -> {to}: another thread's block");
            put(cur, &pattern(n as u8, to));
            len = to;
        }
        rust_dealloc(cur, len);
    }
    // A larger alignment than the heap's keeps it across the move.
    let a = rust_alloc(100, 256);
    put(a, &pattern(7, 100));
    let b = rust_realloc(a, 100, 256, 3000);
    assert_eq!(b % 256, 0, "the alignment survives a realloc");
    assert!(get(b, 100) == pattern(7, 100));
    rust_dealloc(b, 3000);
}

#[test]
fn rust_alloc_zeroed_is_zero_after_a_dirty_free() {
    // Trace: no spec clause (Rust's internal allocations, D-3.1); HEAP-CUTS C2.
    let _g = serial();
    for n in [1usize, 16, 100, 1000, 5000, 70_000] {
        // Dirty blocks of the class, free them, then ask for zeroed ones of the same size.
        let dirty: Vec<usize> = (0..4).map(|_| rust_alloc(n, ALIGN)).collect();
        for &d in &dirty {
            put(d, &vec![0xA5; n]);
        }
        for &d in &dirty {
            rust_dealloc(d, n);
        }
        let z: Vec<usize> = (0..4).map(|_| rust_alloc_zeroed(n, ALIGN)).collect();
        for &b in &z {
            assert!(get(b, n).iter().all(|&x| x == 0), "{n}: zeroed");
            rust_dealloc(b, n);
        }
    }
}

// opaque-ok: audited in GATE-SPEED-4/5: the direct calls here use their results and pass at opt-level 1 and 2 in both pass types; any case LLVM folds goes through selftest::opaque.
