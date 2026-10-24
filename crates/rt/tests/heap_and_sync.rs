//! Internal tests of IP-5 (`sync`) and IP-7 (`heap`): the heap against a fake page source with
//! fault injection at every page request, a randomised allocation-trace fuzzer checking the
//! block invariants, misuse detection, shard ownership and NUMA placement (D-3.10), and the
//! futex primitives under real threads.

#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU32, AtomicU64, AtomicUsize, Ordering};
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use liella_boundary::sys::CSys;
use liella_rt::futex::Scope;
use liella_rt::heap::{class_for, Heap, Misuse, Pages, RacePoint, ALIGN, CLASSES, MAX_SMALL, MID_GROWTH, PAGE, SEG};
use liella_rt::sync::{RawCondvar, RawMutex, RawOnce, RawRwLock};

/// A mapping of the fake address space.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Mapping {
    len: usize,
    /// Its metadata storage in `FakePages::store`.
    slot: usize,
    node: Option<u32>,
}

/// A fake address space: mappings get synthetic aligned addresses and remember the node they
/// asked for; metadata words are real atomics, allocated the first time the heap reads a
/// mapping and freed with the fake. The heap never touches blocks, so nothing else is needed.
struct FakePages {
    next: RefCell<usize>,
    maps: RefCell<BTreeMap<usize, Mapping>>,
    store: Vec<OnceLock<Box<[AtomicU64]>>>,
    calls: RefCell<usize>,
    fail_at: RefCell<Option<usize>>,
    discarded: RefCell<usize>,
    /// Every `wipe` (address, length), in order, and every range kept out of core dumps.
    wiped: RefCell<Vec<(usize, usize)>>,
    undumped: RefCell<Vec<(usize, usize)>>,
    /// The shard the "calling thread" is on.
    shard: Cell<usize>,
    /// The "calling thread" (each has its own cache word).
    thread: Cell<usize>,
    slots: [AtomicUsize; 8],
    /// The address-space limit the heap is told about (T14).
    limit: Cell<Option<usize>>,
    /// Run once at the heap's next [`RacePoint::RemotePinned`] (another "thread's" calls there).
    hook: RefCell<Option<Hook>>,
}

type Hook = Box<dyn FnOnce(&FakePages)>;

/// Fails the whole test process with a message when a test body has not finished in `secs`
/// seconds: a broken remote-free protocol can loop inside the heap, and a hang costs a gate.
struct Watchdog(Arc<AtomicU32>);

impl Watchdog {
    fn new(name: &'static str, secs: u64) -> Watchdog {
        let done = Arc::new(AtomicU32::new(0));
        let d = done.clone();
        std::thread::spawn(move || {
            let end = std::time::Instant::now() + std::time::Duration::from_secs(secs);
            while std::time::Instant::now() < end {
                if d.load(Ordering::Relaxed) != 0 {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
            eprintln!("WATCHDOG: {name} still running after {secs} s (the heap looped: a corrupt pending list?)");
            std::process::exit(101);
        });
        Watchdog(done)
    }
}

impl Drop for Watchdog {
    fn drop(&mut self) {
        self.0.store(1, Ordering::Relaxed);
    }
}

impl FakePages {
    fn new() -> FakePages {
        FakePages {
            next: RefCell::new(0x1000_0000),
            maps: RefCell::new(BTreeMap::new()),
            store: (0..1 << 16).map(|_| OnceLock::new()).collect(),
            calls: RefCell::new(0),
            fail_at: RefCell::new(None),
            discarded: RefCell::new(0),
            wiped: RefCell::new(Vec::new()),
            undumped: RefCell::new(Vec::new()),
            shard: Cell::new(0),
            thread: Cell::new(0),
            slots: Default::default(),
            limit: Cell::new(None),
            hook: RefCell::new(None),
        }
    }
    fn mapping_of(&self, addr: usize) -> (usize, Mapping) {
        let maps = self.maps.borrow();
        let (&base, &m) = maps.range(..=addr).next_back().unwrap();
        assert!(addr < base + m.len, "{addr:#x} is mapped");
        (base, m)
    }
}

impl Pages for FakePages {
    fn address_limit(&self) -> Option<usize> {
        self.limit.get()
    }
    fn map(&self, len: usize, align: usize, node: Option<u32>) -> Option<usize> {
        assert!(align.is_power_of_two() && align >= PAGE);
        let n = *self.calls.borrow();
        *self.calls.borrow_mut() += 1;
        if *self.fail_at.borrow() == Some(n) {
            return None;
        }
        // An address-space limit counts every mapping, as RLIMIT_AS does.
        let mapped: usize = self.maps.borrow().values().map(|m| m.len).sum();
        if self.limit.get().is_some_and(|l| mapped + len > l) {
            return None;
        }
        let mut next = self.next.borrow_mut();
        let addr = next.next_multiple_of(align);
        *next = addr + len.next_multiple_of(PAGE) + 16 * PAGE;
        self.maps.borrow_mut().insert(addr, Mapping { len, slot: n, node });
        Some(addr)
    }
    fn unmap(&self, addr: usize, len: usize) {
        assert_eq!(self.maps.borrow_mut().remove(&addr).map(|m| m.len), Some(len), "unmap of exactly a mapping");
    }
    fn discard(&self, addr: usize, len: usize) {
        let (base, m) = self.mapping_of(addr);
        assert!(addr + len <= base + m.len, "discard inside one mapping");
        *self.discarded.borrow_mut() += len;
    }
    fn remap(&self, addr: usize, old_len: usize, new_len: usize) -> Option<usize> {
        let m = *self.maps.borrow().get(&addr).unwrap();
        assert_eq!(m.len, old_len);
        let nb = self.map(new_len, PAGE, m.node)?;
        self.unmap(addr, old_len);
        Some(nb)
    }
    fn wipe(&self, addr: usize, len: usize) {
        assert_eq!(self.maps.borrow().get(&addr).map(|m| m.len), Some(len), "a wipe covers exactly a live mapping");
        self.wiped.borrow_mut().push((addr, len));
    }
    fn exclude_from_dumps(&self, addr: usize, len: usize) {
        assert_eq!(self.maps.borrow().get(&addr).map(|m| m.len), Some(len), "exactly a mapping");
        self.undumped.borrow_mut().push((addr, len));
    }
    fn words(&self, addr: usize, n: usize) -> &[AtomicU64] {
        let (base, m) = self.mapping_of(addr);
        assert!(addr % 8 == 0 && addr + 8 * n <= base + m.len, "metadata inside the mapping");
        let all = self.store[m.slot].get_or_init(|| (0..m.len / 8).map(|_| AtomicU64::new(0)).collect());
        let off = (addr - base) / 8;
        &all[off..off + n]
    }
    fn shard(&self) -> usize {
        self.shard.get()
    }
    fn cache_slot(&self) -> &AtomicUsize {
        &self.slots[self.thread.get()]
    }
    fn race_point(&self, at: RacePoint) {
        assert_eq!(at, RacePoint::RemotePinned);
        let hook = self.hook.borrow_mut().take();
        if let Some(f) = hook {
            f(self);
        }
    }
}

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

fn size_mix(g: &mut Gen) -> usize {
    match g.below(10) {
        0..=5 => g.below(256) as usize,
        6..=7 => g.below(8192) as usize,
        8 => g.below(MAX_SMALL as u64 + 1) as usize,
        _ => MAX_SMALL + g.below(300_000) as usize,
    }
}

#[test]
fn classes_cover_every_small_size_with_the_fundamental_alignment() {
    // Clauses: s3_1_c_1, s3_1_g_3 (D-3.10).
    for size in 0..=MAX_SMALL {
        let c = class_for(size, ALIGN).unwrap();
        assert!(CLASSES[c] >= size.max(1) && CLASSES[c] % ALIGN == 0);
        assert!(c == 0 || CLASSES[c - 1] < size.max(1), "the smallest fitting class");
    }
    // Smaller alignments pick the same class (the table path for sizes up to 1024, HEAP-CUTS C3).
    for align in [1, 2, 4, 8] {
        for size in 0..=1100 {
            assert_eq!(class_for(size, align), class_for(size, ALIGN), "size {size} align {align}");
        }
    }
    assert_eq!(class_for(MAX_SMALL + 1, ALIGN), None);
    assert_eq!(class_for(100, 2 * PAGE), None, "over-page alignment goes large");
    for align in [32, 64, 128, 256, 512, 1024, 2048, 4096] {
        let c = class_for(100, align).unwrap();
        assert_eq!(CLASSES[c] % align, 0, "every block of the class is aligned");
    }
}

#[test]
fn the_allocation_trace_fuzzer_keeps_every_invariant() {
    let _wd = Watchdog::new("the_allocation_trace_fuzzer_keeps_every_invariant", 120);
    // Clauses: s3_1_g_1, s3_1_g_3, s3_1_c_7, s3_1_c_3, s3_1_t_0 (D-3.10, D-3.11).
    let heap = Heap::new();
    let pages = FakePages::new();
    let mut g = Gen(0x5EED);
    let mut live: BTreeMap<usize, (usize, usize)> = BTreeMap::new();
    for step in 0..60_000 {
        // Three simulated threads: frees by a thread that does not own the block take the
        // remote path, and owners collect them later (D-3.11).
        pages.thread.set(g.below(3) as usize);
        match g.below(10) {
            0..=4 => {
                let size = size_mix(&mut g);
                let align = 1usize << g.below(13);
                let b = heap.alloc(&CSys, &pages, size, align).unwrap();
                assert_eq!(b.addr % align.max(ALIGN), 0, "step {step}: alignment");
                assert!(b.usable >= size, "step {step}: usable");
                if let Some((&a, &(u, _))) = live.range(..=b.addr).next_back() {
                    assert!(a + u <= b.addr, "step {step}: overlaps the block below");
                }
                if let Some((&a, _)) = live.range(b.addr..).next() {
                    assert!(b.addr + b.usable <= a, "step {step}: overlaps the block above");
                }
                live.insert(b.addr, (b.usable, size));
            }
            5..=8 if !live.is_empty() => {
                let k = *live.keys().nth(g.below(live.len() as u64) as usize).unwrap();
                assert_eq!(heap.usable(&CSys, &pages, k), Ok(live[&k].0));
                heap.free(&CSys, &pages, k).unwrap();
                live.remove(&k);
            }
            9 if !live.is_empty() => {
                let k = *live.keys().nth(g.below(live.len() as u64) as usize).unwrap();
                let new_size = size_mix(&mut g).max(1);
                let mut copied = None;
                let b = heap.resize(&CSys, &pages, k, new_size, |f, t, len| copied = Some((f, t, len))).unwrap().unwrap();
                let (old_usable, _) = live.remove(&k).unwrap();
                if let Some((f, t, len)) = copied {
                    assert_eq!((f, t, len), (k, b.addr, old_usable.min(new_size)), "step {step}: moved contents");
                }
                assert!(b.usable >= new_size);
                live.insert(b.addr, (b.usable, new_size));
            }
            _ => {}
        }
    }
    // Remote frees count as free before their owner collects them (D-3.4); trimming collects.
    let collect_all = |pages: &FakePages| {
        for t in 0..3 {
            pages.thread.set(t);
            heap.trim(&CSys, pages);
        }
    };
    let in_use: usize = live.values().map(|v| v.0).sum();
    assert_eq!(heap.stats(&CSys, &pages).in_use, in_use, "the in-use count matches the live blocks before collection");
    collect_all(&pages);
    assert_eq!(heap.stats(&CSys, &pages).in_use, in_use, "the in-use count matches the live blocks");
    for (i, k) in live.keys().copied().enumerate() {
        pages.thread.set(i % 3);
        heap.free(&CSys, &pages, k).unwrap();
    }
    collect_all(&pages);
    let s = heap.stats(&CSys, &pages);
    assert_eq!((s.in_use, s.large_blocks, s.large_mapped), (0, 0, 0), "everything returned");
    assert!(s.releasable > 0, "emptied slabs went back to their shards");
    assert!(*pages.discarded.borrow() > 0, "and trimming released their pages");
}

#[test]
fn misuse_is_detected() {
    // Clauses: s3_1_f_3 (D-3.11, D-3.13).
    let heap = Heap::new();
    let pages = FakePages::new();
    let a = heap.alloc(&CSys, &pages, 40, ALIGN).unwrap().addr;
    assert_eq!(heap.free(&CSys, &pages, a + 8), Err(Misuse::InvalidPointer), "inside a block");
    assert_eq!(heap.free(&CSys, &pages, 0x10), Err(Misuse::InvalidPointer), "not heap memory");
    heap.free(&CSys, &pages, a).unwrap();
    assert_eq!(heap.free(&CSys, &pages, a), Err(Misuse::DoubleFree));
    let big = heap.alloc(&CSys, &pages, 1 << 20, ALIGN).unwrap().addr;
    heap.free(&CSys, &pages, big).unwrap();
    assert_eq!(heap.free(&CSys, &pages, big), Err(Misuse::DoubleFree), "a kept large block is free (D-3.13)");
    assert_eq!(heap.usable(&CSys, &pages, big), Err(Misuse::DoubleFree));
    assert_eq!(heap.resize(&CSys, &pages, big, 10, |_, _, _| {}), Err(Misuse::DoubleFree));
    heap.trim(&CSys, &pages);
    assert_eq!(heap.free(&CSys, &pages, big), Err(Misuse::InvalidPointer), "an unmapped large block is unknown");
    assert_eq!(heap.usable(&CSys, &pages, a), Err(Misuse::DoubleFree));
    // A block freed by another thread and then by its owner is a double free too.
    let x = heap.alloc(&CSys, &pages, 40, ALIGN).unwrap().addr;
    pages.thread.set(1);
    heap.free(&CSys, &pages, x).unwrap();
    assert_eq!(heap.free(&CSys, &pages, x), Err(Misuse::DoubleFree), "twice remotely");
    pages.thread.set(0);
    assert_eq!(heap.free(&CSys, &pages, x), Err(Misuse::DoubleFree), "remotely, then by the owner");
    assert_eq!(heap.usable(&CSys, &pages, x), Err(Misuse::DoubleFree));
}

#[test]
fn every_page_request_can_fail_without_damage() {
    // Clauses: s3_1_f_1 (S8, X5).
    let probe = FakePages::new();
    let heap = Heap::new();
    let plan: Vec<(usize, usize)> = (0..40).map(|i| (16 + i * 3000, [ALIGN, 64, 8192][i % 3])).collect();
    for &(s, a) in &plan {
        heap.alloc(&CSys, &probe, s, a).unwrap();
    }
    let requests = *probe.calls.borrow();
    assert!(requests >= 3);
    for fail in 0..requests {
        let heap = Heap::new();
        let pages = FakePages::new();
        *pages.fail_at.borrow_mut() = Some(fail);
        let mut got = Vec::new();
        let mut failures = 0;
        for &(s, a) in &plan {
            match heap.alloc(&CSys, &pages, s, a) {
                Some(b) => got.push(b.addr),
                None => failures += 1,
            }
        }
        // The failed request's allocation fails, or (a segment that could not be mapped) is
        // served by a mapping of its own (T14); no other allocation is affected.
        assert!(failures <= 1, "request {fail}: at most the failed request's allocation fails");
        for p in got {
            heap.free(&CSys, &pages, p).unwrap();
        }
        assert_eq!(heap.stats(&CSys, &pages).in_use, 0);
    }
}

#[test]
fn an_address_space_limit_sizes_the_registries() {
    // Clauses: s3_1_c_1, s3_1_f_1 (T14). Under a limit of 20 MiB the registries are small, no
    // 16 MiB segment is mapped (below four segments' worth), and small blocks get mappings of
    // their own; under 1 GiB segments are used again, with small registries.
    let pages = FakePages::new();
    pages.limit.set(Some(20 << 20));
    let heap = Heap::new();
    let blocks: Vec<usize> = (0..100).map(|i| heap.alloc(&CSys, &pages, 24 + i, ALIGN).unwrap().addr).collect();
    let biggest = pages.maps.borrow().values().map(|m| m.len).max().unwrap();
    assert!(biggest <= 1 << 20, "no segment and no large registry: largest mapping {biggest}");
    for b in blocks {
        heap.free(&CSys, &pages, b).unwrap();
    }
    assert_eq!(heap.stats(&CSys, &pages).in_use, 0);
    let pages = FakePages::new();
    pages.limit.set(Some(1 << 30));
    let heap = Heap::new();
    let a = heap.alloc(&CSys, &pages, 40, ALIGN).unwrap().addr;
    let lens: Vec<usize> = pages.maps.borrow().values().map(|m| m.len).collect();
    assert!(lens.contains(&(16 << 20)), "a segment: {lens:?}");
    assert!(lens.iter().all(|&l| l <= 16 << 20), "registries sized to 1 GiB: {lens:?}");
    heap.free(&CSys, &pages, a).unwrap();
}

#[test]
fn a_raised_address_space_limit_lets_the_heap_grow() {
    // Clauses: s3_1_c_1, s3_1_f_1 (T14, owner requirement 2026-10-05): a process that raises
    // its address-space limit after the heap's first reservations can grow the heap past the
    // earlier limit: the registries chosen for 20 MiB grow by generations. Without a limit the
    // sizes are the maxima: `without_an_address_space_limit_the_registries_keep_their_maxima`.
    let pages = FakePages::new();
    pages.limit.set(Some(20 << 20));
    let heap = Heap::new();
    let mut blocks = Vec::new();
    while let Some(b) = heap.alloc(&CSys, &pages, 1024, ALIGN) {
        blocks.push(b.addr);
        assert!(blocks.len() < 1 << 20, "the limit must stop it");
    }
    let under = blocks.len();
    assert!(under > 1000, "{under} blocks under 20 MiB");
    pages.limit.set(Some(4 << 30));
    for _ in 0..50_000 {
        blocks.push(heap.alloc(&CSys, &pages, 1024, ALIGN).unwrap().addr);
    }
    // Raised again, to no limit at all (the case of setrlimit(RLIM_INFINITY)).
    pages.limit.set(None);
    for i in 0..40 * 1024 {
        let b = heap.alloc(&CSys, &pages, 1024, ALIGN);
        assert!(b.is_some(), "1 KiB block {i} after the limit was removed");
        blocks.push(b.unwrap().addr);
    }
    let big = heap.alloc(&CSys, &pages, 1 << 20, ALIGN).unwrap();
    blocks.push(big.addr);
    let mut sorted = blocks.clone();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), blocks.len(), "no block handed out twice");
    for b in blocks {
        heap.free(&CSys, &pages, b).unwrap();
    }
    assert_eq!(heap.stats(&CSys, &pages).in_use, 0);
}

#[test]
fn without_an_address_space_limit_the_registries_keep_their_maxima() {
    // Clauses: s3_1_c_1 (T14, D-3.14). No limit (or 64 GiB and more): the registries and the
    // segments have the sizes they had before limits were considered: 16 MiB segments, 8 MiB of
    // segment bases and 16 MiB of segment hash (MAX_SEGS).
    for limit in [None, Some(64usize << 30)] {
        let pages = FakePages::new();
        pages.limit.set(limit);
        let heap = Heap::new();
        let blocks: Vec<usize> = (0..100).map(|i| heap.alloc(&CSys, &pages, 24 + i, ALIGN).unwrap().addr).collect();
        let mut lens: Vec<usize> = pages.maps.borrow().values().map(|m| m.len).collect();
        lens.sort_unstable();
        // A cache chunk; the cache table and the segment bases (8 MiB each); the segment hash
        // and one segment (16 MiB each).
        assert_eq!(lens, [64 << 10, 8 << 20, 8 << 20, 16 << 20, SEG], "{limit:?}");
        for b in blocks {
            heap.free(&CSys, &pages, b).unwrap();
        }
    }
}

#[test]
fn large_blocks_map_and_unmap_and_remap() {
    // Clauses: s3_1_c_3, s3_1_g_3, s3_1_i_0, s3_2_c_6 (D-3.13).
    let heap = Heap::new();
    let pages = FakePages::new();
    // Warm up: the first large block also maps the heap's large-block table.
    let w = heap.alloc(&CSys, &pages, 1_000_000, ALIGN).unwrap();
    assert!(w.zeroed && w.usable >= 1_000_000 && w.addr % PAGE == 0, "a fresh mapping");
    heap.free(&CSys, &pages, w.addr).unwrap();
    let before = pages.maps.borrow().clone();
    let b = heap.alloc(&CSys, &pages, 1_000_000, ALIGN).unwrap();
    assert_eq!((b.addr, b.zeroed), (w.addr, false), "a kept block is reused, not known to be zero (D-3.13)");
    // A kept block more than twice the request plus 1 MiB is not taken (D-3.13).
    let huge = heap.alloc(&CSys, &pages, 3_200_000, ALIGN).unwrap();
    heap.free(&CSys, &pages, huge.addr).unwrap();
    let small = heap.alloc(&CSys, &pages, 1_000_000, ALIGN).unwrap();
    assert_ne!(small.addr, huge.addr);
    heap.free(&CSys, &pages, small.addr).unwrap();
    let again = heap.alloc(&CSys, &pages, 1_200_000, ALIGN).unwrap();
    assert_eq!(again.addr, huge.addr, "within the bound it is");
    heap.free(&CSys, &pages, again.addr).unwrap();
    let mut copied = false;
    let grown = heap.resize(&CSys, &pages, b.addr, 5_000_000, |_, _, _| copied = true).unwrap().unwrap();
    assert!(!copied, "a large block grows by remapping, without a copy");
    assert!(grown.usable >= 5_000_000);
    let al = heap.alloc(&CSys, &pages, 100, 1 << 16).unwrap();
    assert_eq!(al.addr % (1 << 16), 0, "over-page alignment");
    heap.free(&CSys, &pages, grown.addr).unwrap();
    heap.free(&CSys, &pages, al.addr).unwrap();
    assert_eq!(heap.stats(&CSys, &pages).in_use, 0, "kept blocks are not in use");
    assert!(heap.trim(&CSys, &pages));
    let s = heap.stats(&CSys, &pages);
    assert_eq!((s.large_blocks, s.large_mapped, s.in_use), (0, 0, 0), "large mappings are gone after a trim");
    let after = pages.maps.borrow().clone();
    assert!(after.iter().all(|(a, l)| before.get(a) == Some(l) || l.len < 1_000_000), "no large block's mapping survives");
    assert!(heap.alloc(&CSys, &pages, usize::MAX / 2 + 1, ALIGN).is_none(), "above PTRDIFF_MAX");
}

// ------------------------------------------------------------------------------ secure blocks (D-X.11)

#[test]
fn secure_blocks_are_wiped_on_every_release_and_never_kept() {
    // Trace: no spec clause (secure heap blocks, the heap engine); D-X.11.
    let heap = Heap::new();
    let pages = FakePages::new();
    // Ordinary blocks, small and large, are never wiped, and a large one is kept (D-3.13).
    let s = heap.alloc(&CSys, &pages, 40, ALIGN).unwrap();
    let l = heap.alloc(&CSys, &pages, 1 << 20, ALIGN).unwrap();
    heap.free(&CSys, &pages, s.addr).unwrap();
    heap.free(&CSys, &pages, l.addr).unwrap();
    assert!(pages.wiped.borrow().is_empty() && pages.undumped.borrow().is_empty(), "ordinary blocks are never wiped");

    // A secure block: a fresh mapping of its own (zero, page-aligned), kept out of core dumps,
    // never the kept ordinary block.
    let a = heap.alloc_secure(&CSys, &pages, 32, ALIGN).unwrap();
    assert!(a.zeroed && a.usable == PAGE && a.addr % PAGE == 0);
    assert_ne!(a.addr, l.addr, "a kept ordinary block is not reused for a secure one");
    assert_eq!(*pages.undumped.borrow(), [(a.addr, PAGE)]);
    assert_eq!(heap.usable(&CSys, &pages, a.addr), Ok(PAGE));
    // `free`: wiped while still mapped (FakePages checks), then unmapped, not kept.
    heap.free(&CSys, &pages, a.addr).unwrap();
    assert_eq!(*pages.wiped.borrow(), [(a.addr, PAGE)]);
    assert!(!pages.maps.borrow().contains_key(&a.addr), "unmapped at once");
    assert_eq!(heap.free(&CSys, &pages, a.addr), Err(Misuse::InvalidPointer), "a released secure block is unknown");

    // Alignment and size as `alloc`'s.
    let al = heap.alloc_secure(&CSys, &pages, 100, 1 << 16).unwrap();
    assert_eq!(al.addr % (1 << 16), 0, "over-page alignment");
    let zero = heap.alloc_secure(&CSys, &pages, 0, ALIGN).unwrap();
    assert!(zero.usable == PAGE && zero.addr != al.addr, "size 0 is a unique block");
    assert!(heap.alloc_secure(&CSys, &pages, 16, 3).is_none(), "not a power of two");
    assert!(heap.alloc_secure(&CSys, &pages, 16, 0).is_none());
    assert!(heap.alloc_secure(&CSys, &pages, usize::MAX / 2 + 1, ALIGN).is_none(), "above PTRDIFF_MAX");

    // `resize`: within the mapping in place, without a wipe; beyond it into a new secure block,
    // the old one wiped (never remapped, even when large).
    let b = heap.alloc_secure(&CSys, &pages, 3000, ALIGN).unwrap();
    let wipes = pages.wiped.borrow().len();
    let mut copied = None;
    for size in [10, PAGE] {
        let same = heap.resize(&CSys, &pages, b.addr, size, |f, t, n| copied = Some((f, t, n))).unwrap().unwrap();
        assert_eq!(same.addr, b.addr, "in place");
    }
    assert_eq!((copied, pages.wiped.borrow().len()), (None, wipes), "no copy, no wipe in place");
    let moved = heap.resize(&CSys, &pages, b.addr, 100_000, |f, t, n| copied = Some((f, t, n))).unwrap().unwrap();
    assert_eq!(copied, Some((b.addr, moved.addr, PAGE)), "the contents moved");
    assert_eq!(pages.wiped.borrow()[wipes..], [(b.addr, PAGE)], "the old block wiped");
    assert!(!pages.maps.borrow().contains_key(&b.addr));
    assert!(pages.undumped.borrow().contains(&(moved.addr, moved.usable)), "the new block is secure");
    let big = heap.resize(&CSys, &pages, moved.addr, 400_000, |f, t, n| copied = Some((f, t, n))).unwrap().unwrap();
    assert_eq!(copied, Some((moved.addr, big.addr, moved.usable)), "a large secure block is copied, not remapped");
    assert_eq!(pages.wiped.borrow().last(), Some(&(moved.addr, moved.usable)));
    copied = None;
    let shrunk = heap.resize(&CSys, &pages, big.addr, 70_000, |f, t, n| copied = Some((f, t, n))).unwrap().unwrap();
    assert_eq!((shrunk.addr, copied), (big.addr, None), "a secure block shrinks in place");
    assert!(heap.resize(&CSys, &pages, big.addr, usize::MAX / 2 + 1, |_, _, _| {}).unwrap().is_none(), "a failure leaves it");

    // A free from another thread (large blocks have no owner) wipes too.
    pages.thread.set(1);
    heap.free(&CSys, &pages, big.addr).unwrap();
    pages.thread.set(0);
    assert_eq!(pages.wiped.borrow().last(), Some(&(big.addr, big.usable)));

    // Thread exit (park), a fork child's parking of the other threads, and `trim` never hold a
    // secure block: a live one stays live and mapped, a released one was wiped at its release.
    let c = heap.alloc_secure(&CSys, &pages, 3 * PAGE, ALIGN).unwrap();
    let wipes = pages.wiped.borrow().len();
    pages.thread.set(1);
    heap.park(&CSys, &pages);
    pages.thread.set(0);
    heap.park_others(&CSys, &pages);
    heap.park(&CSys, &pages);
    heap.trim(&CSys, &pages);
    assert_eq!(pages.wiped.borrow().len(), wipes, "nothing released a live secure block");
    assert!(pages.maps.borrow().contains_key(&c.addr));
    for blk in [c, al, zero] {
        heap.free(&CSys, &pages, blk.addr).unwrap();
    }
    assert_eq!(pages.wiped.borrow()[wipes..], [(c.addr, c.usable), (al.addr, al.usable), (zero.addr, zero.usable)]);
    heap.trim(&CSys, &pages);
    assert_eq!(pages.wiped.borrow().len(), wipes + 3, "and nothing wipes them again");
    let st = heap.stats(&CSys, &pages);
    assert_eq!((st.in_use, st.large_blocks), (0, 0), "every secure mapping is gone");
}

// ------------------------------------------------------------------------------ shards (D-3.10)

/// The node each segment (a 16 MiB mapping at a 16 MiB boundary) asked for, by base.
fn segment_nodes(pages: &FakePages) -> BTreeMap<usize, Option<u32>> {
    pages.maps.borrow().iter().filter(|(b, m)| m.len == SEG && *b % SEG == 0).map(|(&b, m)| (b, m.node)).collect()
}

#[test]
fn shards_place_their_memory_on_their_node() {
    // Trace: no spec clause (NUMA placement of the heap's shards); D-3.10, D-3.12, M3.
    let heap = Heap::new();
    let pages = FakePages::new();
    heap.set_numa(true);
    pages.shard.set(5);
    let a = heap.alloc(&CSys, &pages, 100, ALIGN).unwrap();
    // Another thread, on node 9.
    pages.thread.set(1);
    pages.shard.set(9);
    let b = heap.alloc(&CSys, &pages, 100, ALIGN).unwrap();
    let segs = segment_nodes(&pages);
    assert_eq!(segs.values().copied().collect::<Vec<_>>(), [Some(5), Some(9)], "one segment per shard, on its node");
    assert_ne!(a.addr / SEG, b.addr / SEG, "shards never share a segment");
    assert_eq!(segs.get(&(a.addr / SEG * SEG)), Some(&Some(5)));
    let big = heap.alloc(&CSys, &pages, 1 << 20, ALIGN).unwrap();
    assert_eq!(pages.maps.borrow()[&big.addr].node, Some(9), "large blocks too");
    heap.set_numa(false);
    pages.thread.set(2);
    pages.shard.set(11);
    heap.alloc(&CSys, &pages, 100, ALIGN).unwrap();
    assert_eq!(segment_nodes(&pages).values().filter(|n| n.is_none()).count(), 1, "without NUMA: the default policy");
}

#[test]
fn blocks_return_to_their_owner_whoever_frees_them() {
    let _wd = Watchdog::new("blocks_return_to_their_owner_whoever_frees_them", 30);
    // Clauses: s3_1_t_0, s3_1_f_3 (D-3.11, M4).
    let heap = Heap::new();
    let pages = FakePages::new();
    // Thread 0 on node 3 allocates; thread 1 on node 4 frees everything (remote frees).
    pages.shard.set(3);
    let mine: Vec<_> = (0..1000).map(|_| heap.alloc(&CSys, &pages, 48, ALIGN).unwrap().addr).collect();
    pages.thread.set(1);
    pages.shard.set(4);
    for &p in &mine {
        heap.free(&CSys, &pages, p).unwrap();
    }
    assert_eq!(heap.free(&CSys, &pages, mine[0]), Err(Misuse::DoubleFree), "detected across threads");
    // Thread 1 allocates from its own slab, on its own node's segment.
    let again = heap.alloc(&CSys, &pages, 48, ALIGN).unwrap();
    assert!(!mine.contains(&again.addr), "thread 1 allocates from its own slab");
    assert_ne!(again.addr / SEG, mine[0] / SEG);
    // The owner collects the remote frees and reuses the blocks.
    pages.thread.set(0);
    assert_eq!(heap.stats(&CSys, &pages).in_use, 48, "uncollected remote frees count as free (D-3.4)");
    heap.trim(&CSys, &pages);
    let reused = heap.alloc(&CSys, &pages, 48, ALIGN).unwrap();
    assert_eq!(reused.addr, mine[0], "thread 0 gets its first block back");
    // realloc moves a block into the calling thread's cache.
    pages.thread.set(1);
    let moved = heap.resize(&CSys, &pages, reused.addr, 5000, |_, _, _| {}).unwrap().unwrap();
    assert_eq!(moved.addr / SEG, again.addr / SEG);
    heap.free(&CSys, &pages, moved.addr).unwrap();
    heap.free(&CSys, &pages, again.addr).unwrap();
    pages.thread.set(0);
    heap.trim(&CSys, &pages);
    assert_eq!(heap.stats(&CSys, &pages).in_use, 0);
}

#[test]
fn a_remote_free_landing_after_the_owners_scan_is_collected_later() {
    let _wd = Watchdog::new("a_remote_free_landing_after_the_owners_scan_is_collected_later", 30);
    // Clauses: s3_1_f_3 (D-3.15: a late remote free is seen by this collection or queues the slab again).
    let heap = Rc::new(Heap::new());
    let pages = FakePages::new();
    let x = heap.alloc(&CSys, &pages, 48, ALIGN).unwrap().addr;
    let y = heap.alloc(&CSys, &pages, 48, ALIGN).unwrap().addr;
    // Thread 1 frees x: the slab goes on thread 0's pending list.
    pages.thread.set(1);
    heap.free(&CSys, &pages, x).unwrap();
    // Thread 1 frees y; once it has pinned the slab, and before its remote bit is set, thread 0
    // collects (trim): it takes the slab off its pending list and scans past y's word.
    let h = Rc::clone(&heap);
    *pages.hook.borrow_mut() = Some(Box::new(move |p: &FakePages| {
        p.thread.set(0);
        h.trim(&CSys, p);
        p.thread.set(1);
    }));
    heap.free(&CSys, &pages, y).unwrap();
    assert!(pages.hook.borrow().is_none(), "the race point was reached");
    // The next collection must see y: both blocks come back (lowest free block first).
    pages.thread.set(0);
    heap.trim(&CSys, &pages);
    let mut again = [0; 2].map(|_| heap.alloc(&CSys, &pages, 48, ALIGN).unwrap().addr);
    again.sort_unstable();
    let mut want = [x, y];
    want.sort_unstable();
    assert_eq!(again, want, "a remote free after the owner's scan was never collected (leak)");
}

#[test]
fn a_refused_remote_double_free_leaves_no_pin_behind() {
    // Clauses: s3_1_f_3 (D-3.15: a refused remote free gives its pin back, or the slab never goes home).
    let _wd = Watchdog::new("a_refused_remote_double_free_leaves_no_pin_behind", 30);
    let run = |double: bool| {
        let heap = Heap::new();
        let pages = FakePages::new();
        let mine: Vec<_> = (0..20_000).map(|_| heap.alloc(&CSys, &pages, 48, ALIGN).unwrap().addr).collect();
        pages.thread.set(1);
        for &p in &mine {
            heap.free(&CSys, &pages, p).unwrap();
        }
        if double {
            assert_eq!(heap.free(&CSys, &pages, mine[0]), Err(Misuse::DoubleFree));
        }
        pages.thread.set(0);
        heap.trim(&CSys, &pages);
        heap.trim(&CSys, &pages);
        heap.stats(&CSys, &pages).releasable
    };
    let clean = run(false);
    assert!(clean > 0, "emptied slabs went back to their shard");
    assert_eq!(run(true), clean, "a refused double free pinned its slab: it was never released");
}

#[test]
fn realloc_takes_one_fused_path_and_forecasts_growth() {
    // Trace: no spec clause beyond s3_1_c_7 (REALLOC-FUSE: fused small resize, growth forecast); D-3.11, S6.
    let heap = Heap::new();
    let pages = FakePages::new();
    let a = |n: usize| heap.alloc(&CSys, &pages, n, ALIGN).unwrap();
    let resize = |p: usize, n: usize| {
        let mut copied = None;
        let r = heap.resize(&CSys, &pages, p, n, |f, t, len| copied = Some((f, t, len)));
        (r, copied)
    };
    // Current slabs for the target classes below, so that the moves take the fused path.
    for n in [16, 80, 192] {
        let w = a(n).addr;
        heap.free(&CSys, &pages, w).unwrap();
    }
    // `malloc`'d blocks keep their own class: no forecast outside `resize`.
    let b = a(20);
    assert_eq!((b.usable, heap.usable(&CSys, &pages, b.addr)), (32, Ok(32)), "malloc(20) is not forecast");
    // Hint words: fill two bitmap words of the class-32 slab, so the cache searches a later word.
    let fill: Vec<usize> = (0..130).map(|_| a(32).addr).collect();
    let old = fill[5];
    // Growth past the class moves, copying the whole old slot, into the class for twice the request.
    let (r, copied) = resize(old, 40);
    let grown = r.unwrap().unwrap();
    assert_eq!(copied, Some((old, grown.addr, 32)), "the old block's contents moved");
    assert_eq!(grown.usable, CLASSES[class_for(80, ALIGN).unwrap()], "the forecast class (2 x 40)");
    assert_eq!(heap.usable(&CSys, &pages, grown.addr), Ok(grown.usable), "the new block is live at its class");
    assert_eq!(heap.usable(&CSys, &pages, old), Err(Misuse::DoubleFree), "the old block is free");
    // The old block is free and searched first again: the next allocation of its class gets it.
    assert_eq!(a(32).addr, old, "the freed slot is the cache's next block of its class");
    let old2 = fill[6];
    let g2 = resize(old2, 33).0.unwrap().unwrap();
    // A freed block is a double free for `resize`, in the fused path as in the general one.
    assert_eq!(resize(old2, 50).0, Err(Misuse::DoubleFree), "resize after a fused move");
    assert_eq!(heap.free(&CSys, &pages, old2), Err(Misuse::DoubleFree), "free after a fused move");
    heap.free(&CSys, &pages, fill[7]).unwrap();
    assert_eq!(resize(fill[7], 10).0, Err(Misuse::DoubleFree), "resize after free");
    // The caller's next doubling stays in place, with no copy, and the class does not go stale.
    let (r, copied) = resize(grown.addr, 80);
    assert_eq!((r.unwrap().unwrap().addr, copied), (grown.addr, None), "in place");
    assert_eq!(heap.usable(&CSys, &pages, grown.addr), Ok(80));
    let (r, copied) = resize(grown.addr, 81);
    let g3 = r.unwrap().unwrap();
    assert_eq!(copied, Some((grown.addr, g3.addr, 80)));
    assert_eq!((g3.usable, heap.usable(&CSys, &pages, g3.addr)), (192, Ok(192)), "forecast 162 -> class 192");
    // A shrink below half the next class down moves without a forecast, copying the request.
    let (r, copied) = resize(g3.addr, 10);
    let small = r.unwrap().unwrap();
    assert_eq!((small.usable, copied), (16, Some((g3.addr, small.addr, 10))));
    assert_eq!(heap.usable(&CSys, &pages, g3.addr), Err(Misuse::DoubleFree));
    // A block another thread freed is not live for its owner's `resize`.
    let x = a(40).addr;
    pages.thread.set(1);
    heap.free(&CSys, &pages, x).unwrap();
    pages.thread.set(0);
    assert_eq!(resize(x, 90).0, Err(Misuse::DoubleFree), "remotely freed, then resized by the owner (to class 192)");
    // Above the cap: no forecast.
    let y = a(1024).addr;
    let big = resize(y, 1500).0.unwrap().unwrap();
    assert_eq!(big.usable, CLASSES[class_for(1500, ALIGN).unwrap()], "no forecast above FORECAST_CAP");
    for p in [g2.addr, small.addr, big.addr] {
        heap.free(&CSys, &pages, p).unwrap();
    }
}

#[test]
fn the_free_fast_path_finds_each_blocks_own_slab() {
    // Trace: no spec clause (the free fast path's slab hints, HEAP-CUTS2 S-C); D-3.11, S6.
    const SLAB: usize = 1 << 18;
    let heap = Heap::new();
    let pages = FakePages::new();
    let a = |n: usize| heap.alloc(&CSys, &pages, n, ALIGN).unwrap().addr;
    // An older slab: fill a 32-byte slab, so the next block opens a new current slab; a free
    // into the old one must not be applied to the new one's bitmap.
    let per = SLAB / 32;
    let old: Vec<usize> = (0..per).map(|_| a(32)).collect();
    let new = a(32);
    assert_eq!(new & (SLAB - 1), old[0] & (SLAB - 1), "block 0 of the next slab");
    assert_ne!((new / SLAB) % 16, (old[0] / SLAB) % 16, "the test needs the two slabs in different hint words");
    heap.free(&CSys, &pages, old[0]).unwrap();
    assert_eq!(heap.free(&CSys, &pages, new), Ok(()), "the new slab's block 0 is still live");
    assert_eq!(heap.free(&CSys, &pages, old[0]), Err(Misuse::DoubleFree));
    assert_eq!(heap.free(&CSys, &pages, new), Err(Misuse::DoubleFree));
    assert_eq!(heap.stats(&CSys, &pages).in_use, (per - 1) * 32);
    for &p in &old[1..] {
        heap.free(&CSys, &pages, p).unwrap();
    }
    // Mixed classes, at their boundaries: every free reaches its own block, in any order.
    let sizes = [16, 17, 32, 33, 128, 129, 1024, 1025, 4096, 4097, 65536];
    for round in 0..3 {
        let live: Vec<(usize, usize)> = sizes.iter().map(|&n| (a(n), n)).collect();
        let class_size = |n: usize| CLASSES[class_for(n, ALIGN).unwrap()];
        assert_eq!(heap.stats(&CSys, &pages).in_use, sizes.iter().map(|&n| class_size(n)).sum::<usize>());
        for &(p, n) in &live {
            assert_eq!(heap.usable(&CSys, &pages, p), Ok(class_size(n)));
            assert_eq!(heap.free(&CSys, &pages, p + 8), Err(Misuse::InvalidPointer), "inside a block ({n} B)");
        }
        // Free in an order that differs from the allocation order each round.
        let mut order: Vec<usize> = (0..live.len()).collect();
        order.rotate_left(round * 4 + 1);
        if round == 1 {
            order.reverse();
        }
        for (k, &i) in order.iter().enumerate() {
            heap.free(&CSys, &pages, live[i].0).unwrap();
            assert_eq!(heap.free(&CSys, &pages, live[i].0), Err(Misuse::DoubleFree), "{} B", live[i].1);
            for &j in &order[k + 1..] {
                assert_eq!(heap.usable(&CSys, &pages, live[j].0), Ok(class_size(live[j].1)), "a neighbour stays live");
            }
        }
        assert_eq!(heap.stats(&CSys, &pages).in_use, 0);
    }
    // Another thread's free of a block of a class it also has a current slab of: a remote free.
    let x = a(48);
    pages.thread.set(1);
    let y = a(48);
    heap.free(&CSys, &pages, x).unwrap();
    assert_eq!(heap.free(&CSys, &pages, x), Err(Misuse::DoubleFree), "twice remotely");
    heap.free(&CSys, &pages, y).unwrap();
    pages.thread.set(0);
    assert_eq!(heap.free(&CSys, &pages, x), Err(Misuse::DoubleFree), "remotely, then by the owner");
    heap.trim(&CSys, &pages);
    assert_eq!(heap.stats(&CSys, &pages).in_use, 0);
}

#[test]
fn malloc_reuses_blocks_freed_into_several_words() {
    // Trace: no spec clause (the allocation hint's free groups, HEAP-HINT-2); D-3.16.
    let heap = Heap::new();
    let pages = FakePages::new();
    // Five bitmap words of 16-byte blocks in use: the class's first slab, from block 0.
    let live: Vec<usize> = (0..320).map(|_| heap.alloc(&CSys, &pages, 8, ALIGN).unwrap().addr).collect();
    for (i, &p) in live.iter().enumerate() {
        assert_eq!(p, live[0] + i * 16, "block {i} of one slab");
    }
    for round in 0..3 {
        // gawk's regex sets: frees into words 4, 3, 3 and 2, then as many mallocs: the hint
        // word first, then the lowest free block of the free groups - malloc's way (the fast
        // path, then `alloc_group` at the top of `malloc_general`), never the slow path.
        let freed = [live[4 * 64 + 5], live[3 * 64 + 9], live[3 * 64 + 10], live[2 * 64 + 7]];
        for &p in &freed {
            heap.free(&CSys, &pages, p).unwrap();
        }
        let malloc = |m: usize| {
            let b = Heap::alloc_fast(&pages, 8, ALIGN).or_else(|| Heap::alloc_group(&pages, 8));
            assert!(b.is_some(), "round {round}: malloc {m} left the fast path and alloc_group");
            b.map_or(0, |b| b.addr)
        };
        let mut got: Vec<usize> = (0..4).map(malloc).collect();
        got.sort_unstable();
        let mut want = freed.to_vec();
        want.sort_unstable();
        assert_eq!(got, want, "round {round}: the freed blocks come back");
    }
    // Words 0-4 are full again: the next block is the lowest free one, block 320.
    let next = heap.alloc(&CSys, &pages, 8, ALIGN).unwrap().addr;
    assert_eq!(next, live[0] + 320 * 16, "word 5's first block");
    for p in live.into_iter().chain([next]) {
        heap.free(&CSys, &pages, p).unwrap();
    }
    assert_eq!(heap.stats(&CSys, &pages).in_use, 0);
}

#[test]
fn segments_are_aligned_and_found_from_any_address() {
    // Trace: no spec clause (the heap's segment registry); D-3.10, M5.
    let heap = Heap::new();
    let pages = FakePages::new();
    // 64 KiB blocks: four per slab, 252 per segment, so this spans four segments.
    let blocks: Vec<_> = (0..900).map(|_| heap.alloc(&CSys, &pages, 65536, ALIGN).unwrap().addr).collect();
    let segs = segment_nodes(&pages);
    assert_eq!(segs.len(), 4);
    for &p in &blocks {
        assert!(segs.contains_key(&(p / SEG * SEG)), "every block lies in a registered, aligned segment");
        assert_eq!(heap.usable(&CSys, &pages, p), Ok(65536));
    }
    for &p in blocks.iter().rev() {
        heap.free(&CSys, &pages, p).unwrap();
    }
    assert_eq!(heap.stats(&CSys, &pages).small_mapped, 4 * SEG);
}

// ------------------------------------------------------------------------------ sync

#[test]
fn mutex_excludes_and_wakes() {
    // Clauses: s22_4_c_0, s22_n_3 (IP-5, D-22.1).
    let m = Arc::new(RawMutex::new());
    let n = Arc::new(AtomicUsize::new(0));
    let racy = Arc::new(AtomicU32::new(0));
    let hs: Vec<_> = (0..8).map(|_| {
        let (m, n, racy) = (m.clone(), n.clone(), racy.clone());
        std::thread::spawn(move || {
            for _ in 0..20_000 {
                let _g = m.lock(&CSys);
                // A non-atomic read-modify-write made safe only by the lock.
                let v = racy.load(Ordering::Relaxed);
                racy.store(v + 1, Ordering::Relaxed);
                n.fetch_add(1, Ordering::Relaxed);
            }
        })
    }).collect();
    for h in hs {
        h.join().unwrap();
    }
    assert_eq!(racy.load(Ordering::Relaxed), 160_000);
    assert!(!m.is_locked());
    assert!(m.try_lock() && !m.try_lock());
    m.unlock(&CSys, Scope::Private);
}

#[test]
fn once_runs_exactly_once() {
    // Clauses: s22_5_c_5 (IP-5).
    let once = Arc::new(RawOnce::new());
    let runs = Arc::new(AtomicUsize::new(0));
    let hs: Vec<_> = (0..8).map(|_| {
        let (once, runs) = (once.clone(), runs.clone());
        std::thread::spawn(move || {
            once.call_once(&CSys, || {
                std::thread::sleep(std::time::Duration::from_millis(20));
                runs.fetch_add(1, Ordering::SeqCst);
            });
            assert!(once.is_done(), "every caller returns after it finished");
        })
    }).collect();
    for h in hs {
        h.join().unwrap();
    }
    assert_eq!(runs.load(Ordering::SeqCst), 1);
}

#[test]
fn rwlock_shares_readers_and_excludes_writers() {
    // Clauses: s22_5_c_2 (IP-5).
    let l = Arc::new(RawRwLock::new());
    assert!(l.try_read() && l.try_read(), "readers share");
    assert!(!l.try_write());
    l.unlock(&CSys, Scope::Private);
    l.unlock(&CSys, Scope::Private);
    assert!(l.try_write() && !l.try_read());
    l.unlock(&CSys, Scope::Private);
    let v = Arc::new(AtomicU64::new(0));
    let hs: Vec<_> = (0..6).map(|i| {
        let (l, v) = (l.clone(), v.clone());
        std::thread::spawn(move || {
            for _ in 0..5_000 {
                if i % 2 == 0 {
                    l.write(&CSys, Scope::Private);
                    let x = v.load(Ordering::Relaxed);
                    v.store(x + 1, Ordering::Relaxed);
                } else {
                    l.read(&CSys, Scope::Private);
                    let _ = v.load(Ordering::Relaxed);
                }
                l.unlock(&CSys, Scope::Private);
            }
        })
    }).collect();
    for h in hs {
        h.join().unwrap();
    }
    assert_eq!(v.load(Ordering::Relaxed), 15_000);
}

#[test]
fn condvar_hands_off() {
    // Clauses: s22_5_c_1 (IP-5).
    let m = Arc::new(RawMutex::new());
    let cv = Arc::new(RawCondvar::new());
    let ready = Arc::new(AtomicU32::new(0));
    let (m2, cv2, r2) = (m.clone(), cv.clone(), ready.clone());
    let waiter = std::thread::spawn(move || {
        m2.lock_raw(&CSys, Scope::Private);
        while r2.load(Ordering::Relaxed) == 0 {
            cv2.wait(&CSys, &m2, Scope::Private);
        }
        m2.unlock(&CSys, Scope::Private);
    });
    std::thread::sleep(std::time::Duration::from_millis(20));
    m.lock_raw(&CSys, Scope::Private);
    ready.store(1, Ordering::Relaxed);
    cv.notify_all(&CSys, Scope::Private);
    m.unlock(&CSys, Scope::Private);
    waiter.join().unwrap();
}

#[test]
fn a_mid_size_buffer_grown_by_half_skips_classes() {
    // Trace: no spec clause beyond s3_1_c_7 (HEAP-REUSE: mid-size growth forecast up to MAX_SMALL); D-3.11.
    let heap = Heap::new();
    let pages = FakePages::new();
    let mut p = heap.alloc(&CSys, &pages, 2187, ALIGN).unwrap().addr;
    // tar's directory buffer: each step half again the last, 2 KiB to 56 KiB.
    let (mut moves, mut usable) = (0, 0);
    for n in [3280, 4920, 7380, 11070, 16605, 24907, 37360, 56040] {
        let mut copied = false;
        let b = heap.resize(&CSys, &pages, p, n, |_, _, _| copied = true).unwrap().unwrap();
        assert!(b.usable >= n, "a resized block holds its request");
        if copied {
            moves += 1;
            assert!(b.usable >= (MID_GROWTH * n).min(MAX_SMALL), "a block moved upward gets about four times the request: {n} -> {}", b.usable);
            // Resized again to the request it was forecast for, it stays where it is.
            let mut back = false;
            let same = heap.resize(&CSys, &pages, b.addr, n, |_, _, _| back = true).unwrap().unwrap();
            assert!(!back && same.addr == b.addr, "a forecast block moved back down: {n}");
        }
        (p, usable) = (b.addr, b.usable);
    }
    assert_eq!(moves, 2, "two moves: 3280 and 16605 (REALLOC-GROW)");
    assert_eq!(usable, MAX_SMALL, "the forecast stops at the largest small class");
    // Above MAX_SMALL the request is exact: a large block is mapped for its own length.
    let big = heap.resize(&CSys, &pages, p, MAX_SMALL + 1, |_, _, _| {}).unwrap().unwrap();
    assert!(big.usable < 2 * MAX_SMALL, "no forecast for a large block");
    // A large block whose remap fails is copied into a new mapping, never kept at its old
    // length while reported grown.
    *pages.fail_at.borrow_mut() = Some(*pages.calls.borrow());
    let mut copied = None;
    let moved = heap.resize(&CSys, &pages, big.addr, 4 * MAX_SMALL, |f, t, len| copied = Some((f, t, len))).unwrap().unwrap();
    assert_eq!(copied, Some((big.addr, moved.addr, big.usable)), "a failed remap falls back to a copy");
    assert!(moved.usable >= 4 * MAX_SMALL && pages.maps.borrow().get(&moved.addr).is_some_and(|m| m.len >= moved.usable), "the new block is mapped for its length");
    heap.free(&CSys, &pages, moved.addr).unwrap();
}
