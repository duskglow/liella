// Specified from spec §14.1–§14.6 (phase 2) and §14.7–§14.10 (phase 12).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §14 (56 clauses).
//!
//! §14.1–14.6 run against `liella::search` and `liella_legacy::search_c` over an in-memory
//! arena with an allocation budget (for the out-of-memory clauses). §14.9 runs against
//! `liella::pattern::regex` and the exports; §14.7, §14.8 and §14.10 against
//! `liella::pattern::{fnmatch, glob, wordexp}` (with a virtual tree and a shell in the test) and
//! the exports.

#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::bool_assert_comparison, missing_docs)]


// ------------------------------------------------------------------------------ §14.1–14.6 (phase 2)

#[path = "support/timeout.rs"]
mod timeout;

use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::HashMap;

use liella::search::{self, sort_stable, sort_stable_in_place};
use liella_abi::layout::{struct_hsearch_data, ENTRY};
use liella_kernel::Errno;
use liella_legacy::mb_compat::Local;
use liella_legacy::search_c::{self, CWords, Table};

/// Caller memory as a map from addresses to words, with an allocation budget (for the
/// out-of-memory clauses).
struct Arena {
    words: RefCell<HashMap<usize, usize>>,
    text: HashMap<usize, Vec<u8>>,
    next: RefCell<usize>,
    budget: RefCell<usize>,
}

impl Arena {
    fn new(budget: usize) -> Arena {
        Arena { words: RefCell::new(HashMap::new()), text: HashMap::new(), next: RefCell::new(0x10_0000), budget: RefCell::new(budget) }
    }
}

impl CWords for &Arena {
    fn word(&self, addr: usize, i: usize) -> usize {
        self.words.borrow().get(&(addr + 8 * i)).copied().unwrap_or(0)
    }
    fn set_word(&mut self, addr: usize, i: usize, v: usize) {
        self.words.borrow_mut().insert(addr + 8 * i, v);
    }
    fn alloc(&mut self, words: usize) -> Option<usize> {
        let mut b = self.budget.borrow_mut();
        *b = b.checked_sub(1)?;
        let mut n = self.next.borrow_mut();
        let a = *n;
        *n += 8 * words + 64;
        Some(a)
    }
    fn free(&mut self, _addr: usize) {}
    fn text(&self, addr: usize) -> &[u8] {
        self.text.get(&addr).map_or(&[], Vec::as_slice)
    }
}

extern "C" fn by_addr(a: *const core::ffi::c_void, b: *const core::ffi::c_void) -> i32 {
    a.addr().cmp(&b.addr()) as i32
}

fn ints(v: &[u32]) -> Vec<u8> {
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

fn back(v: &[u8]) -> Vec<u32> {
    v.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect()
}

fn cmp4(a: &[u8], b: &[u8]) -> Ordering {
    u32::from_le_bytes(a.try_into().unwrap()).cmp(&u32::from_le_bytes(b.try_into().unwrap()))
}

#[test]
fn s14_1_c_0() {
    let mut v = ints(&[3, 1, 2]);
    sort_stable(&mut v, 4, &mut cmp4);
    assert_eq!(back(&v), [1, 2, 3]);
    let mut w = ints(&[1, 2, 3]);
    let mut ctx_seen = 0;
    sort_stable(&mut w, 4, &mut |a, b| {
        ctx_seen += 1;
        cmp4(b, a)
    });
    assert_eq!((back(&w), ctx_seen > 0), (vec![3, 2, 1], true), "the comparator alone decides the order");
}

#[test]
fn s14_1_i_0() {
    let mut v = ints(&(0..300).collect::<Vec<u32>>());
    let mut flip = false;
    sort_stable(&mut v, 4, &mut |_, _| {
        flip = !flip;
        if flip { Ordering::Less } else { Ordering::Greater }
    });
    let mut got = back(&v);
    got.sort_unstable();
    assert_eq!(got, (0..300).collect::<Vec<u32>>(), "an inconsistent comparator: still within the array, still a permutation");
    // Elements above 64 bytes move in pieces.
    let mut w: Vec<u8> = (0u8..200).flat_map(|i| [i; 100]).collect();
    sort_stable(&mut w, 100, &mut |_, _| {
        flip = !flip;
        if flip { Ordering::Less } else { Ordering::Greater }
    });
    let mut firsts: Vec<u8> = w.chunks_exact(100).map(|e| {
        assert!(e.iter().all(|&b| b == e[0]), "an element stays whole");
        e[0]
    }).collect();
    firsts.sort_unstable();
    assert_eq!(firsts, (0u8..200).collect::<Vec<u8>>(), "still a permutation");
    offsets_whole_elements();
}

/// The `SortCompare` contract the boundary relies on for memory safety: with any answers, every
/// offset names a whole element of the slice passed with it. Insertion-only, stack and heap
/// sizes, word and multi-piece elements.
fn offsets_whole_elements() {
    struct Wild {
        size: usize,
        x: u32,
        bad: usize,
        calls: usize,
    }
    impl search::SortCompare for Wild {
        fn gt(&mut self, base: &[u8], a: usize, b: usize) -> bool {
            let whole = |o: usize| o % self.size == 0 && o + self.size <= base.len() && base.len() % self.size == 0;
            self.bad += usize::from(!whole(a) || !whole(b));
            self.calls += 1;
            self.x = self.x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            self.x >> 31 == 1
        }
    }
    for (size, n) in [(8usize, 7usize), (8, 85), (8, 3000), (24, 500), (100, 3), (100, 300)] {
        let mut v: Vec<u8> = (0..n).flat_map(|i| vec![(i % 251) as u8; size]).collect();
        let orig = v.clone();
        let mut w = Wild { size, x: n as u32, bad: 0, calls: 0 };
        search::sort_stable_tracked(&mut v, size, &mut w, &mut search::SortScratch::default());
        assert!(w.calls > 0, "size {size} n {n}: compared");
        assert_eq!(w.bad, 0, "size {size} n {n}: every offset names a whole element");
        let mut got: Vec<&[u8]> = v.chunks_exact(size).collect();
        let mut want: Vec<&[u8]> = orig.chunks_exact(size).collect();
        got.sort_unstable();
        want.sort_unstable();
        assert_eq!(got, want, "size {size} n {n}: still a permutation of whole elements");
    }
}

#[test]
fn s14_1_g_1() {
    let mut x = 12345u32;
    let input: Vec<u32> = (0..1000).map(|_| {
        x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
        x >> 20
    }).collect();
    let mut v = ints(&input);
    sort_stable(&mut v, 4, &mut cmp4);
    let mut want = input.clone();
    want.sort_unstable();
    assert_eq!(back(&v), want);
    elements_in_place();
}

/// The comparator gets elements of the array, where they started and unchanged (C17 7.22.5),
/// for word, two-word and multi-piece (above 64 bytes) elements, on the stack and heap paths.
fn elements_in_place() {
    for (size, n) in [(8usize, 9usize), (8, 85), (8, 1000), (16, 300), (100, 70), (100, 500)] {
        let mut x = 777u32;
        let mut v: Vec<u8> = (0..n)
            .flat_map(|i| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let mut e = vec![(i % 251) as u8; size];
                e[..4].copy_from_slice(&(x >> 22).to_le_bytes());
                e
            })
            .collect();
        let orig = v.clone();
        let (lo, hi) = (v.as_ptr().addr(), v.as_ptr().addr() + v.len());
        let mut bad = 0usize;
        let mut check = |e: &[u8]| {
            let at = e.as_ptr().addr();
            let ok = at >= lo && at + size <= hi && (at - lo) % size == 0 && e == &orig[at - lo..at - lo + size];
            bad += usize::from(!ok);
        };
        sort_stable(&mut v, size, &mut |a, b| {
            check(a);
            check(b);
            cmp4(&a[..4], &b[..4])
        });
        assert_eq!(bad, 0, "size {size} n {n}: only elements of the array, in place");
        let keys: Vec<u32> = v.chunks_exact(size).map(|e| u32::from_le_bytes(e[..4].try_into().unwrap())).collect();
        assert!(keys.windows(2).all(|p| p[0] <= p[1]), "size {size} n {n}: sorted");
        let mut got: Vec<&[u8]> = v.chunks_exact(size).collect();
        let mut want: Vec<&[u8]> = orig.chunks_exact(size).collect();
        got.sort_unstable();
        want.sort_unstable();
        assert_eq!(got, want, "size {size} n {n}: whole elements moved");
    }
}

#[test]
fn s14_1_g_2() {
    // Stability is not promised; Liella sorts stably on both paths (D-14.1).
    let mut v: Vec<u8> = (0u32..100).flat_map(|i| [(i % 3).to_le_bytes(), i.to_le_bytes()].concat()).collect();
    let mut w = v.clone();
    let mut by_key = |a: &[u8], b: &[u8]| cmp4(&a[..4], &b[..4]);
    sort_stable(&mut v, 8, &mut by_key);
    sort_stable_in_place(&mut w, 8, &mut by_key);
    for s in [&v, &w] {
        let idx: Vec<u32> = s.chunks_exact(8).map(|e| u32::from_le_bytes(e[4..].try_into().unwrap())).collect();
        for k in 0..3 {
            let run: Vec<u32> = idx.iter().copied().filter(|i| i % 3 == k).collect();
            assert!(run.windows(2).all(|p| p[0] < p[1]), "equal keys keep their order");
        }
    }
}

#[test]
fn s14_1_g_3() {
    let mut v = ints(&(0..2000).rev().collect::<Vec<u32>>());
    sort_stable_in_place(&mut v, 4, &mut cmp4);
    assert_eq!(back(&v), (0..2000).collect::<Vec<u32>>(), "the allocation-free path also sorts: never fails");
}

#[test]
fn k_sort_stable_ordered_input() {
    // Trace: D-14.1
    // Ordered input costs one comparison per element on both paths (WL-HOT-MAKE: the comparator
    // is the caller's function, so comparisons are the cost).
    for n in [2u32, 20, 32, 33, 1000] {
        let mut v = ints(&(0..n).collect::<Vec<u32>>());
        let mut w = v.clone();
        let mut calls = 0u32;
        sort_stable(&mut v, 4, &mut |a, b| {
            calls += 1;
            cmp4(a, b)
        });
        assert_eq!((back(&v), calls), ((0..n).collect::<Vec<u32>>(), n - 1), "sort_stable, n = {n}");
        let mut calls = 0u32;
        sort_stable_in_place(&mut w, 4, &mut |a, b| {
            calls += 1;
            cmp4(a, b)
        });
        assert_eq!((back(&w), calls), ((0..n).collect::<Vec<u32>>(), n - 1), "sort_stable_in_place, n = {n}");
    }
}

#[test]
fn k_sort_stable_descending_input() {
    // Trace: D-14.1
    // Strictly descending input costs one comparison per element (QSORT-MERGE-INDEX), on the
    // stack and heap paths; descending runs with ties and descending prefixes still sort stably.
    for n in [9u32, 16, 17, 20, 33, 85, 86, 1000, 5000] {
        let mut v = ints(&(0..n).rev().collect::<Vec<u32>>());
        let mut calls = 0u32;
        sort_stable(&mut v, 4, &mut |a, b| {
            calls += 1;
            cmp4(a, b)
        });
        assert_eq!((back(&v), calls), ((0..n).collect::<Vec<u32>>(), n - 1), "descending, n = {n}");
    }
    let mut x = 4242u32;
    for n in [9usize, 10, 17, 40, 85, 300, 2000] {
        for shape in 0..4 {
            // 0: descending with ties, 1: descending blocks, 2: descending prefix then random,
            // 3: ascending blocks each descending inside.
            let input: Vec<(u32, u32)> = (0..n as u32).map(|i| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
                let k = match shape {
                    0 => (n as u32 - i) / 3,
                    1 => (n as u32 - i) % 7 + ((x >> 16) % 2),
                    2 if i < n as u32 / 2 => n as u32 - i,
                    2 => (x >> 16) % 50,
                    _ => (i / 5) * 10 + (4 - i % 5),
                };
                (k, i)
            }).collect();
            let mut v: Vec<u8> = input.iter().flat_map(|&(k, i)| [k.to_le_bytes(), i.to_le_bytes()].concat()).collect();
            sort_stable(&mut v, 8, &mut |a, b| cmp4(&a[..4], &b[..4]));
            let mut want = input.clone();
            want.sort_by_key(|&(k, _)| k);
            let got: Vec<(u32, u32)> = v.chunks_exact(8).map(|e| {
                (u32::from_le_bytes(e[..4].try_into().unwrap()), u32::from_le_bytes(e[4..].try_into().unwrap()))
            }).collect();
            assert_eq!(got, want, "shape {shape}, n {n}: sorted, equal keys in their order");
        }
    }
}

#[test]
fn k_sort_stable_element_sizes() {
    // Trace: D-14.1
    // Element sizes around the insertion pass's in-register limit (64 bytes) and run lengths
    // around one run: sorted by a 4-byte key with many duplicates, stably, the payload intact.
    let mut x = 777u32;
    for size in [8usize, 32, 63, 64, 65, 100, 200] {
        for n in [3usize, 9, 20, 31, 32, 33, 64, 65, 300] {
            let input: Vec<(u32, u32)> = (0..n as u32).map(|i| {
                x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
                ((x >> 16) % 7, i)
            }).collect();
            let mut v: Vec<u8> = Vec::new();
            for &(k, i) in &input {
                let mut e = vec![(i % 251) as u8; size];
                e[..4].copy_from_slice(&k.to_le_bytes());
                e[size - 4..].copy_from_slice(&i.to_le_bytes());
                v.extend_from_slice(&e);
            }
            let mut w = v.clone();
            let mut by_key = |a: &[u8], b: &[u8]| cmp4(&a[..4], &b[..4]);
            sort_stable(&mut v, size, &mut by_key);
            sort_stable_in_place(&mut w, size, &mut by_key);
            let mut want = input.clone();
            want.sort_by_key(|&(k, _)| k);
            for s in [&v, &w] {
                let got: Vec<(u32, u32)> = s.chunks_exact(size).map(|e| {
                    let i = u32::from_le_bytes(e[size - 4..].try_into().unwrap());
                    assert!(e[4..size - 4].iter().all(|&b| b == (i % 251) as u8), "payload of {i} intact (size {size})");
                    (u32::from_le_bytes(e[..4].try_into().unwrap()), i)
                }).collect();
                assert_eq!(got, want, "size {size}, n {n}");
            }
        }
    }
}

#[test]
fn s14_2_c_0() {
    let v = ints(&[1, 3, 3, 3, 9]);
    let i = search::binary_search(&v, 4, &mut |e| 3u32.cmp(&u32::from_le_bytes(e.try_into().unwrap())));
    assert!(matches!(i, Some(1..=3)), "an equal element, whichever");
    assert_eq!(search::binary_search(&v, 4, &mut |e| 4u32.cmp(&u32::from_le_bytes(e.try_into().unwrap()))), None);
}

#[test]
fn s14_3_c_0() {
    extern "C" fn eq(a: *const core::ffi::c_void, b: *const core::ffi::c_void) -> i32 {
        let (mut x, mut y) = (0u32, 0u32);
        liella_boundary::posix::s04_strings::memcpy((&raw mut x).cast(), a, 4);
        liella_boundary::posix::s04_strings::memcpy((&raw mut y).cast(), b, 4);
        x.cmp(&y) as i32
    }
    let mut v = ints(&[5, 6, 0]);
    let mut n = 2usize;
    assert_eq!(search_c::lsearch(&7u32.to_le_bytes(), &mut v, &mut n, 4, eq), 8);
    assert_eq!((n, back(&v)), (3, vec![5, 6, 7]), "appended, *nmemb incremented");
    assert_eq!(search_c::lfind(6u32.to_le_bytes().as_ptr().cast(), &v, 4, eq), Some(4));
}

#[test]
fn s14_4_c_0() {
    let a = Arena::new(100);
    let mut root = Local(0);
    for k in [30, 10, 20] {
        search_c::tsearch(k, &mut root, &a, by_addr).unwrap();
    }
    assert_ne!(root.0, 0, "the root lives in the caller's variable");
    let mut keys = Vec::new();
    collect(&a, root.0, &mut keys);
    assert_eq!(keys, [10, 20, 30], "an ordered set");
}

fn collect(a: &Arena, n: usize, out: &mut Vec<usize>) {
    if n == 0 {
        return;
    }
    collect(a, (&a).word(n, 1), out);
    out.push((&a).word(n, 0));
    collect(a, (&a).word(n, 2), out);
}

#[test]
fn s14_4_c_1() {
    let a = Arena::new(10);
    let mut root = Local(0);
    let n = search_c::tsearch(77, &mut root, &a, by_addr).unwrap();
    assert_eq!((&a).word(n, 0), 77, "the node's first member is the key");
    assert_eq!(search_c::tsearch(77, &mut root, &a, by_addr).unwrap(), n);
}

#[test]
fn s14_4_c_2() {
    let a = Arena::new(10);
    let mut root = Local(0);
    let n = search_c::tsearch(5, &mut root, &a, by_addr).unwrap();
    assert_eq!(search_c::tfind(5, root.0, &a, by_addr), n);
    assert_eq!(search_c::tfind(6, root.0, &a, by_addr), 0);
}

#[test]
fn s14_4_c_3() {
    let a = Arena::new(10);
    let mut root = Local(0);
    for k in [2, 1, 3] {
        search_c::tsearch(k, &mut root, &a, by_addr).unwrap();
    }
    let parent = search_c::tdelete(1, 0x1, &mut root, &a, by_addr);
    assert_eq!((&a).word(parent, 0), 2, "the parent node");
    assert_eq!(search_c::tdelete(42, 0x1, &mut root, &a, by_addr), 0);
    search_c::tdelete(2, 0x1, &mut root, &a, by_addr);
    assert_ne!(search_c::tdelete(3, 0x1, &mut root, &a, by_addr), 0, "non-NULL when the tree empties");
    assert_eq!(root.0, 0);
}

static VISITS: std::sync::Mutex<Vec<(usize, i32, i32)>> = std::sync::Mutex::new(Vec::new());

extern "C" fn note(node: *const core::ffi::c_void, v: i32, d: i32) {
    VISITS.lock().unwrap().push((node.addr(), v, d));
}

static VISITS_R: std::sync::Mutex<Vec<(usize, i32, i32)>> = std::sync::Mutex::new(Vec::new());

extern "C" fn note_r(node: *const core::ffi::c_void, v: i32, ctx: *mut core::ffi::c_void) {
    VISITS_R.lock().unwrap().push((node.addr(), v, ctx.addr() as i32));
}

#[test]
fn s14_4_c_4() {
    let a = Arena::new(10);
    let mut root = Local(0);
    for k in [2, 1, 3] {
        search_c::tsearch(k, &mut root, &a, by_addr).unwrap();
    }
    VISITS.lock().unwrap().clear();
    search_c::twalk(root.0, &a, Some(note));
    let v = VISITS.lock().unwrap().clone();
    let kinds: Vec<i32> = v.iter().filter(|e| e.0 == root.0).map(|e| e.1).collect();
    assert_eq!(kinds, [0, 1, 2], "preorder, postorder (in-order) and endorder for an inner node");
    assert_eq!(v.iter().filter(|e| e.1 == 3).count(), 2, "each leaf once, as leaf");
    assert!(v.iter().filter(|e| e.1 == 3).all(|e| e.2 == 1), "at depth 1");
}

#[test]
fn s14_4_c_5() {
    let a = Arena::new(10);
    let mut root = Local(0);
    search_c::tsearch(9, &mut root, &a, by_addr).unwrap();
    search_c::twalk_r(root.0, &a, Some(note_r), core::ptr::without_provenance_mut(123));
    assert!(VISITS_R.lock().unwrap().contains(&(root.0, 3, 123)));
}

static KEYS_FREED: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());

extern "C" fn free_k(k: *mut core::ffi::c_void) {
    KEYS_FREED.lock().unwrap().push(k.addr());
}

#[test]
fn s14_4_c_6() {
    let a = Arena::new(10);
    let mut root = Local(0);
    for k in [501, 502, 503] {
        search_c::tsearch(k, &mut root, &a, by_addr).unwrap();
    }
    search_c::tdestroy(root.0, &a, Some(free_k));
    let mut f: Vec<usize> = KEYS_FREED.lock().unwrap().iter().copied().filter(|&k| k > 500).collect();
    f.sort_unstable();
    assert_eq!(f, [501, 502, 503]);
}

#[test]
fn s14_4_g_0() {
    let a = Arena::new(5000);
    let mut root = Local(0);
    for k in 1..=4096usize {
        search_c::tsearch(k, &mut root, &a, by_addr).unwrap();
    }
    fn depth(a: &Arena, n: usize) -> usize {
        if n == 0 { 0 } else { 1 + depth(a, (&a).word(n, 1)).max(depth(a, (&a).word(n, 2))) }
    }
    assert!(depth(&a, root.0) <= 18, "balanced: depth {} for 4096 ordered inserts", depth(&a, root.0));
}

#[test]
fn s14_4_f_0() {
    let a = Arena::new(1);
    let mut root = Local(0);
    search_c::tsearch(1, &mut root, &a, by_addr).unwrap();
    assert_eq!(search_c::tsearch(2, &mut root, &a, by_addr), Err(Errno::ENOMEM), "no memory: NULL (ENOMEM)");
    assert_eq!(search_c::tfind(1, root.0, &a, by_addr) != 0, true, "the tree is intact");
}

fn arena_with_keys(budget: usize, keys: &[&str]) -> Arena {
    let mut a = Arena::new(budget);
    for (i, k) in keys.iter().enumerate() {
        a.text.insert(0x9000 + i * 16, k.as_bytes().to_vec());
    }
    a
}

#[test]
fn s14_5_c_0() {
    let a = arena_with_keys(10, &["red", "red"]);
    let mut t = struct_hsearch_data { table: 0, size: 0, filled: 0 };
    search_c::hcreate(Table::Caller(&mut t), 5, &a).unwrap();
    let e = search_c::hsearch(Table::Caller(&mut t), ENTRY { key: 0x9000, data: 1 }, true, &a).unwrap();
    assert_eq!(search_c::hsearch(Table::Caller(&mut t), ENTRY { key: 0x9010, data: 0 }, false, &a), Ok(e), "keys are compared as text");
    assert_eq!((&a).word(e, 1), 1, "the data");
}

#[test]
fn s14_5_i_0() {
    let a = Arena::new(10);
    let mut t = struct_hsearch_data { table: 0, size: 0, filled: 0 };
    search_c::hcreate(Table::Caller(&mut t), 100, &a).unwrap();
    assert_eq!(t.size, 101, "rounded up to a prime");
    assert_eq!(search::prime_at_least(8), 11);
}

#[test]
fn s14_5_f_1() {
    let names: Vec<String> = (0..10).map(|i| format!("k{i}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let a = arena_with_keys(1, &refs);
    let mut t = struct_hsearch_data { table: 0, size: 0, filled: 0 };
    search_c::hcreate(Table::Caller(&mut t), 3, &a).unwrap();
    let mut last = Ok(0);
    for i in 0..10 {
        last = search_c::hsearch(Table::Caller(&mut t), ENTRY { key: 0x9000 + i * 16, data: i }, true, &a);
        if last.is_err() {
            break;
        }
    }
    assert_eq!(last, Err(Errno::ENOMEM), "full, and no memory to grow: NULL with ENOMEM");
}

#[test]
fn s14_5_f_2() {
    // Liella grows the table instead of failing (D-14.3); entries never move.
    let names: Vec<String> = (0..50).map(|i| format!("n{i}")).collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let a = arena_with_keys(100, &refs);
    let mut t = struct_hsearch_data { table: 0, size: 0, filled: 0 };
    search_c::hcreate(Table::Caller(&mut t), 3, &a).unwrap();
    let first = search_c::hsearch(Table::Caller(&mut t), ENTRY { key: 0x9000, data: 0 }, true, &a).unwrap();
    for i in 1..50 {
        search_c::hsearch(Table::Caller(&mut t), ENTRY { key: 0x9000 + i * 16, data: i }, true, &a).unwrap();
    }
    assert_eq!((t.filled, t.size >= 50), (50, true));
    assert_eq!(search_c::hsearch(Table::Caller(&mut t), ENTRY { key: 0x9000, data: 0 }, false, &a), Ok(first));
}

#[test]
fn s14_5_f_3() {
    let a = arena_with_keys(10, &["kept"]);
    let mut t = struct_hsearch_data { table: 0, size: 0, filled: 0 };
    search_c::hcreate(Table::Caller(&mut t), 3, &a).unwrap();
    search_c::hsearch(Table::Caller(&mut t), ENTRY { key: 0x9000, data: 0 }, true, &a).unwrap();
    search_c::hdestroy(Table::Caller(&mut t), &a);
    assert_eq!((&a).text(0x9000), b"kept", "keys are not freed");
    assert_eq!(t.table, 0);
}

#[test]
fn s14_6_c_0() {
    let a = Arena::new(0);
    let (x, y, z) = (0x100, 0x200, 0x300);
    search_c::insque(x, 0, &a);
    search_c::insque(z, x, &a);
    search_c::insque(y, x, &a);
    let w = |e: usize, i: usize| (&a).word(e, i);
    assert_eq!((w(x, 0), w(x, 1), w(y, 0), w(y, 1), w(z, 0), w(z, 1)), (y, 0, z, x, 0, y), "after the given element; NULL starts a list");
    search_c::remque(y, &a);
    assert_eq!((w(x, 0), w(z, 1)), (z, x));
}

// ------------------------------------------------------------------------------ §14.9 (phase 12)
//
// The engine runs through `liella::pattern::regex`; the interface clauses through the exports
// (`liella_boundary::posix::s14_search`). The whole behaviour record is compared by
// `regex_record.rs` (library) and `patterns_products.rs` (the C interface against glibc).

mod rx {
    use std::ffi::CString;
    use std::sync::Mutex;

    pub(crate) use liella::pattern::regex::{self, cflags, Options, ReError, Window};
    pub(crate) use liella_abi::layout::{regex_t, regmatch_t, struct_re_registers};
    pub(crate) use liella_boundary::posix::s14_search as s;

    /// The GNU and BSD interfaces share process-wide state (`re_syntax_options`,
    /// `re_max_failures`, the `re_comp` pattern).
    pub(crate) static GLOBAL: Mutex<()> = Mutex::new(());

    pub(crate) fn z(s: &str) -> CString {
        CString::new(s).unwrap()
    }

    pub(crate) fn blank() -> regex_t {
        regex_t { buffer: 0, allocated: 0, used: 0, syntax: 0, fastmap: 0, translate: 0, re_nsub: 0, can_be_null: [0] }
    }

    pub(crate) fn comp(pat: &str, cf: i32) -> (i32, regex_t) {
        let mut re = blank();
        let c = s::regcomp(Some(&mut re), z(pat).as_ptr(), cf);
        (c, re)
    }

    /// `regexec` on `subj` with `n` entries: the result and the entries.
    pub(crate) fn exec(re: &regex_t, subj: &str, n: usize, ef: i32) -> (i32, Vec<(i32, i32)>) {
        let mut m = vec![regmatch_t { rm_so: -7, rm_eo: -7 }; n.max(1)];
        let r = s::regexec(Some(re), z(subj).as_ptr(), n, m.as_mut_ptr(), ef);
        (r, m.iter().take(n).map(|x| (x.rm_so, x.rm_eo)).collect())
    }

    /// Compile in "C" with `cf` and match `subj`, all subexpressions.
    pub(crate) fn m(pat: &str, cf: i32, subj: &str) -> Option<Vec<(i32, i32)>> {
        let (c, mut re) = comp(pat, cf);
        assert_eq!(c, 0, "regcomp {pat}");
        let n = usize::try_from(re.re_nsub).unwrap() + 1;
        let (r, v) = exec(&re, subj, n, 0);
        s::regfree(Some(&mut re));
        (r == 0).then_some(v)
    }

    /// A match: start, end, subexpressions.
    pub(crate) type Hit = (usize, usize, Vec<(i64, i64)>);

    /// The library engine directly: a match of `pat` (syntax `syn`) on `subj` in locale `loc`.
    pub(crate) fn lib(pat: &[u8], syn: u64, subj: &[u8], loc: &liella::locale::Locale) -> Option<Hit> {
        let p = regex::compile(pat, syn, loc).unwrap();
        let w = Window { first: 0, last: subj.len(), backward: false };
        let r = regex::exec(&p, loc, subj, w, &Options { budget: 1 << 26, ..Options::default() }, true).unwrap();
        r.map(|x| (x.start, x.end, x.groups.iter().copied().collect()))
    }

    pub(crate) const E: i32 = cflags::EXTENDED;
    pub(crate) const NOTBOL: i32 = 1;
    pub(crate) const NOTEOL: i32 = 2;
    pub(crate) const STARTEND: i32 = 4;
}

#[test]
fn s14_9_c_1() {
    // The POSIX interface: compile, execute, describe an error, release.
    use rx::*;
    let (c, mut re) = comp("b(c)", E);
    assert_eq!((c, re.re_nsub), (0, 1));
    assert_ne!(re.buffer, 0, "the compiled form lives in the regex_t");
    assert_eq!(exec(&re, "abcd", 2, 0), (0, vec![(1, 3), (2, 3)]));
    assert_eq!(exec(&re, "xyz", 2, 0).0, regex::NOMATCH);
    let mut buf = [0i8; 64];
    let n = s::regerror(regex::NOMATCH, Some(&re), buf.as_mut_ptr(), 64);
    assert_eq!((n, text_of(&buf)), (9, "No match".to_string()));
    s::regfree(Some(&mut re));
    assert_eq!((re.buffer, re.used), (0, 0));
}

fn text_of(b: &[i8]) -> String {
    b.iter().take_while(|&&c| c != 0).map(|&c| char::from(c as u8)).collect()
}

#[test]
fn s14_9_c_1_1() {
    use rx::*;
    // Basic and extended syntax.
    assert_eq!(m("a\\{2\\}", 0, "xaa"), Some(vec![(1, 3)]), "BRE interval");
    assert_eq!(m("a{2}", 0, "a{2}"), Some(vec![(0, 4)]), "BRE braces are literal");
    assert_eq!(m("a{2}", E, "xaa"), Some(vec![(1, 3)]), "ERE interval");
    assert_eq!(m("\\(a\\)|b", 0, "a|b"), Some(vec![(0, 3), (0, 1)]), "BRE | is literal");
    assert_eq!(m("(a)|b", E, "b"), Some(vec![(0, 1), (-1, -1)]), "ERE alternation");
    // REG_ICASE.
    assert_eq!(m("aBc", E | cflags::ICASE, "xAbC"), Some(vec![(1, 4)]));
    assert_eq!(m("[^a]", E | cflags::ICASE, "A"), None, "a negated set excludes both cases");
    // REG_NOSUB: re_nsub still counts the groups; regexec reports success only.
    let (c, mut re) = comp("(a)(b)?", E | cflags::NOSUB);
    assert_eq!((c, re.re_nsub), (0, 2));
    assert_eq!(exec(&re, "xab", 3, 0), (0, vec![(-7, -7); 3]), "nothing written");
    s::regfree(Some(&mut re));
    // REG_NEWLINE.
    assert_eq!(m("a.b", E | cflags::NEWLINE, "a\nb"), None, ". does not match a newline");
    assert_eq!(m("^b", E | cflags::NEWLINE, "a\nb"), Some(vec![(2, 3)]), "^ after a newline");
    assert_eq!(m("^b", E, "a\nb"), None);
    // An unknown flag bit is ignored.
    assert_eq!(comp("a", 0x100).0, 0);
}

#[test]
fn s14_9_c_1_2() {
    use rx::*;
    // Leftmost, then longest.
    assert_eq!(m("x*", E, "yxx"), Some(vec![(0, 0)]), "the leftmost (empty) match");
    assert_eq!(m("x*$", E, "yxx"), Some(vec![(1, 3)]));
    assert_eq!(m("a|ab|abc", E, "xabcd"), Some(vec![(1, 4)]), "the longest at the leftmost start");
    assert_eq!(m("(.*)c(.*)", E, "abcdcef"), Some(vec![(0, 7), (0, 4), (5, 7)]));
    // nmatch: exactly nmatch entries; beyond re_nsub, -1.
    let (_, mut re) = comp("(a)(b)(c)?", E);
    assert_eq!(exec(&re, "xab", 1, 0), (0, vec![(1, 3)]));
    assert_eq!(exec(&re, "xab", 6, 0), (0, vec![(1, 3), (1, 2), (2, 3), (-1, -1), (-1, -1), (-1, -1)]));
    assert_eq!(exec(&re, "xyz", 2, 0), (1, vec![(-7, -7); 2]), "no match: nothing written");
    s::regfree(Some(&mut re));
    // REG_NOTBOL, REG_NOTEOL.
    let (_, mut re) = comp("^a$", E);
    assert_eq!(exec(&re, "a", 1, NOTBOL).0, 1);
    assert_eq!(exec(&re, "a", 1, NOTEOL).0, 1);
    assert_eq!(exec(&re, "a", 1, 0).0, 0);
    s::regfree(Some(&mut re));
    // REG_STARTEND: pmatch[0] delimits the text, NUL included, offsets from the buffer start.
    let buf = b"xa\0ab\0c";
    let (_, mut re) = comp("(a)(b)", E);
    let mut pm = [regmatch_t { rm_so: 0, rm_eo: 7 }, regmatch_t { rm_so: -7, rm_eo: -7 }, regmatch_t { rm_so: -7, rm_eo: -7 }];
    assert_eq!(s::regexec(Some(&re), buf.as_ptr().cast(), 3, pm.as_mut_ptr(), STARTEND), 0);
    assert_eq!([(pm[0].rm_so, pm[0].rm_eo), (pm[1].rm_so, pm[1].rm_eo), (pm[2].rm_so, pm[2].rm_eo)], [(3, 5), (3, 4), (4, 5)]);
    let mut pm = [regmatch_t { rm_so: 4, rm_eo: 3 }];
    assert_eq!(s::regexec(Some(&re), buf.as_ptr().cast(), 1, pm.as_mut_ptr(), STARTEND), 1, "so > eo");
    s::regfree(Some(&mut re));
    let (_, mut re) = comp("a[^x]a", E);
    let mut pm = [regmatch_t { rm_so: 0, rm_eo: 7 }];
    assert_eq!(s::regexec(Some(&re), buf.as_ptr().cast(), 1, pm.as_mut_ptr(), STARTEND), 0, "a set matches NUL");
    assert_eq!((pm[0].rm_so, pm[0].rm_eo), (1, 4));
    s::regfree(Some(&mut re));
}

#[test]
fn s14_9_c_1_3() {
    use rx::*;
    let text = |code: i32, size: usize| {
        let mut b = [b'Z' as i8; 64];
        let n = s::regerror(code, None, b.as_mut_ptr(), size);
        (n, text_of(&b[..size.max(1)]))
    };
    assert_eq!(text(0, 64), (8, "Success".to_string()));
    assert_eq!(text(7, 64), (31, "Unmatched [, [^, [:, [., or [=".to_string()));
    assert_eq!(text(7, 5), (31, "Unma".to_string()), "truncated and terminated");
    assert_eq!(s::regerror(7, None, core::ptr::null_mut(), 0), 31, "the full length plus one, with no buffer");
    assert_eq!(text(16, 64), (18, "Unmatched ) or \\)".to_string()));
    assert_eq!(text(1000, 64), (14, "Unknown error".to_string()), "an unknown code has a message (no abort)");
    // regfree releases the compiled storage and a fastmap.
    let (_, mut re) = comp("a", 0);
    s::regfree(Some(&mut re));
    assert_eq!((re.buffer, re.allocated, re.used, re.fastmap), (0, 0, 0, 0));
    s::regfree(None);
}

#[test]
fn s14_9_c_2() {
    use rx::*;
    use liella_boundary::data;
    use std::sync::atomic::Ordering::Relaxed;
    let _g = GLOBAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let old = s::re_set_syntax(regex::syntax::POSIX_EXTENDED);
    let mut b = blank();
    // A length-delimited pattern: only the first 6 bytes are the pattern.
    let pat = b"a(b+)cXYZ";
    assert!(s::re_compile_pattern(pat.as_ptr().cast(), 6, Some(&mut b)).is_null());
    let mut regs = struct_re_registers { num_regs: 0, start: 0, end: 0 };
    let subj = b"zabbbc!";
    assert_eq!(s::re_search(Some(&mut b), subj.as_ptr().cast(), 6, 0, 6, Some(&mut regs)), 1);
    let reg = |r: &struct_re_registers, i: usize| reg_of(r, i);
    assert_eq!((regs.num_regs, reg(&regs, 0), reg(&regs, 1)), (3, (1, 6), (2, 5)));
    assert_eq!(s::re_search(Some(&mut b), subj.as_ptr().cast(), 6, 5, -5, None), 1, "backwards from 5");
    assert_eq!(s::re_search(Some(&mut b), subj.as_ptr().cast(), 6, 7, 1, None), -1, "a start outside the buffer");
    assert_eq!(s::re_match(Some(&mut b), subj.as_ptr().cast(), 6, 1, None), 5, "the length matched at start");
    assert_eq!(s::re_match(Some(&mut b), subj.as_ptr().cast(), 6, 0, None), -1);
    // Split buffers: one string.
    let (s1, s2) = (b"xab", b"bcy");
    assert_eq!(s::re_search_2(Some(&mut b), s1.as_ptr().cast(), 3, s2.as_ptr().cast(), 3, 0, 6, Some(&mut regs), 6), 1);
    assert_eq!((reg(&regs, 0), reg(&regs, 1)), ((1, 5), (2, 4)));
    assert_eq!(s::re_search_2(Some(&mut b), s1.as_ptr().cast(), 3, s2.as_ptr().cast(), 3, 0, 6, None, 4), -1, "stop limits the match");
    assert_eq!(s::re_match_2(Some(&mut b), s1.as_ptr().cast(), 3, s2.as_ptr().cast(), 3, 1, None, 6), 4);
    // The registers stay allocated (the caller frees them with free(); the test leaks them).
    s::regfree(Some(&mut b));
    // Selectable syntax, back-references, word and buffer anchors.
    s::re_set_syntax(regex::syntax::POSIX_BASIC);
    let mut b = blank();
    let pat = b"\\(a*\\)\\1";
    assert!(s::re_compile_pattern(pat.as_ptr().cast(), pat.len(), Some(&mut b)).is_null());
    assert_eq!(s::re_match(Some(&mut b), b"aaaa".as_ptr().cast(), 4, 0, None), 4, "a back-reference");
    s::regfree(Some(&mut b));
    s::re_set_syntax(regex::syntax::POSIX_EXTENDED);
    for (p, subj, at) in [("\\<b", "a b", 2), ("b\\>", "ab b", 1), ("\\bb", "ab b", 3), ("a\\'", "aa", 1), ("\\`a", "aa", 0)] {
        let mut b = blank();
        assert!(s::re_compile_pattern(p.as_ptr().cast(), p.len(), Some(&mut b)).is_null());
        let l = i32::try_from(subj.len()).unwrap();
        assert_eq!(s::re_search(Some(&mut b), subj.as_ptr().cast(), l, 0, l, None), at, "{p} on {subj}");
        s::regfree(Some(&mut b));
    }
    // The syntax in force is re_syntax_options; an error is the message.
    data::re_syntax_options.store(regex::syntax::POSIX_EXTENDED, Relaxed);
    let mut b = blank();
    let e = s::re_compile_pattern(b"(".as_ptr().cast(), 1, Some(&mut b));
    assert_eq!(cstr_of(e), "Unmatched ( or \\(");
    s::re_set_syntax(old);
}

fn reg_of(r: &rx::struct_re_registers, i: usize) -> (i32, i32) {
    let n = usize::try_from(r.num_regs).unwrap();
    assert!(i < n);
    let read = |addr: usize| {
        let mut v = 0i32;
        liella_boundary::posix::s04_strings::memcpy((&raw mut v).cast(), core::ptr::with_exposed_provenance::<core::ffi::c_void>(addr + 4 * i), 4);
        v
    };
    (read(r.start), read(r.end))
}

fn cstr_of(p: *const i8) -> String {
    let mut out = String::new();
    let mut k = 0usize;
    loop {
        let mut c = 0u8;
        liella_boundary::posix::s04_strings::memcpy((&raw mut c).cast(), p.wrapping_add(k).cast(), 1);
        if c == 0 {
            return out;
        }
        out.push(char::from(c));
        k += 1;
    }
}

#[test]
fn s14_9_c_3() {
    use rx::*;
    let _g = GLOBAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let old = s::re_set_syntax(0);
    // One global compiled pattern; Emacs syntax when re_syntax_options is 0.
    assert!(s::re_comp(z("ab*c").as_ptr()).is_null());
    assert_eq!((s::re_exec(z("xabbc").as_ptr()), s::re_exec(z("xb").as_ptr())), (1, 0));
    assert!(s::re_comp(core::ptr::null()).is_null(), "NULL keeps the pattern");
    assert_eq!(s::re_exec(z("ac").as_ptr()), 1);
    assert!(s::re_comp(z("a|b").as_ptr()).is_null());
    assert_eq!((s::re_exec(z("b").as_ptr()), s::re_exec(z("a|b").as_ptr())), (0, 1), "| is literal in Emacs syntax");
    let e = s::re_comp(z("\\(").as_ptr());
    assert_eq!(cstr_of(e), "Unmatched ( or \\(");
    assert_eq!(cstr_of(s::re_comp(core::ptr::null())), "No previous regular expression", "a failed re_comp leaves no pattern");
    assert_eq!(s::re_exec(z("abc").as_ptr()), 0, "no pattern: no match (never a crash)");
    s::re_set_syntax(old);
}

/// Spec §14.9 (Guarantees): POSIX leftmost-longest semantics, and multibyte (locale) awareness.
/// This includes subexpression selection as POSIX XBD 9.1 defines it: consistent with the whole
/// match being the longest of the leftmost matches, each subpattern, from left to right, matches
/// the longest possible string (a null string being longer than no match), and `regexec`
/// reports subexpressions by its rules (the last match of a repeated group; -1 for a group that
/// did not take part, including a group nested in a repetition whose last iteration did not take
/// it). Clarified 2026-10-03 (owner ruling, `spec/behaviour/regex-submatch-analysis.md`): glibc
/// selects subexpressions by alternation priority instead.
#[test]
fn s14_9_g_0() {
    use rx::*;
    use liella::locale::Locale;
    let ere = regex::syntax::POSIX_EXTENDED;
    let c = Locale::C;
    let g = |pat: &str, subj: &str| lib(pat.as_bytes(), ere, subj.as_bytes(), &c);
    // Each subexpression, left to right, the longest: alternation order does not matter (I1).
    assert_eq!(g("(a|ab)(c|bcd)(d*)", "abcd"), Some((0, 4, vec![(0, 2), (2, 3), (3, 4)])));
    assert_eq!(g("(ab|a)(c|bcd)(d*)", "abcd"), Some((0, 4, vec![(0, 2), (2, 3), (3, 4)])));
    assert_eq!(g("(a|ab)(b*)", "abb"), Some((0, 3, vec![(0, 2), (2, 3)])));
    // A null string is longer than no match (I2).
    assert_eq!(g("||()", ""), Some((0, 0, vec![(0, 0)])));
    // The last iteration; a group the last iteration did not take is -1 (I3).
    assert_eq!(g("((a)|b)+", "ab"), Some((0, 2, vec![(1, 2), (-1, -1)])));
    assert_eq!(g("(a|b)*", "abab"), Some((0, 4, vec![(3, 4)])));
    // The longest subexpression in a counted repetition (I4), and no unneeded empty iteration (E2).
    assert_eq!(g("(a*){2}", "aa"), Some((0, 2, vec![(0, 2)])));
    assert_eq!(g("(()|a)+", "a"), Some((0, 1, vec![(0, 1), (-1, -1)])));
    // The leftmost-longest match exists, so it is found (O1).
    assert_eq!(g("(){2}\\1", ""), Some((0, 0, vec![(0, 0)])));
    // Multibyte: characters, not bytes; offsets in bytes.
    let u = Locale::c_utf8(b"C.UTF-8");
    assert_eq!(lib("^.$".as_bytes(), ere, "é".as_bytes(), &u).map(|x| (x.0, x.1)), Some((0, 2)));
    assert_eq!(lib("^.$".as_bytes(), ere, "é".as_bytes(), &c), None, "two bytes in C");
    assert_eq!(lib("[[:alpha:]]+".as_bytes(), ere, "1étè2".as_bytes(), &u).map(|x| (x.0, x.1)), Some((1, 6)));
    assert_eq!(lib(b"\xa9", ere, "é".as_bytes(), &u).map(|x| (x.0, x.1)), Some((1, 2)), "a pattern byte matches inside a character");
    // Linear time without back-references (D-14.4): patterns that are exponential for a
    // backtracking matcher, on a long subject; quadrupling the subject roughly quadruples the time.
    for pat in ["(a|aa)*c", "(a*)*b", "(x+x+)+y", "((a|aa)*)*(b|c)"] {
        let time = |n: usize| {
            let subj = if pat.contains('x') { "x".repeat(n) } else { "a".repeat(n) };
            // CPU time of this thread (/proc/thread-self/schedstat, ns on a CPU), not wall time: under load a
            // run is descheduled for whole seconds (measured 7-15x "ratios" with 64 busy loops on 32 CPUs, FLAKES-4).
            // The fastest of 5 runs: a disturbed run only adds time (cache and SMT contention, steal).
            let on_cpu = || -> f64 {
                let st = std::fs::read_to_string("/proc/thread-self/schedstat").unwrap();
                st.split_whitespace().next().and_then(|v| v.parse::<f64>().ok()).unwrap() / 1e9
            };
            (0..5)
                .map(|_| {
                    let t = on_cpu();
                    assert_eq!(lib(pat.as_bytes(), ere, subj.as_bytes(), &c), None, "{pat}: no match");
                    on_cpu() - t
                })
                .fold(f64::MAX, f64::min)
        };
        // A subject 4 times as long: linear time is 4 times (HYGIENE-1: measured with doubling, a linear matcher
        // reads 2 and a quadratic one 4, so a bound of 4 caught nothing and load noise crossed it); quadratic is
        // 16 times. The bound of 9 sits between, with a floor for timer granularity on the short run.
        let (t1, t2) = (time(2000), time(8000));
        eprintln!("s14_9_g_0 {pat}: {t1:.4} s at 2000, {t2:.4} s at 8000 (x{:.1})", t2 / t1.max(1e-9));
        assert!(t2 < 60.0, "{pat}: {t2:.2} s on 8000 characters");
        assert!(t2 < t1 * 9.0 + 0.05, "{pat}: not linear ({t1:.3} s for 2000, {t2:.3} s for 8000; linear 4x, quadratic 16x)");
    }
    let subj = "a".repeat(20_000) + "c";
    assert_eq!(lib(b"((a|aa)*)c", ere, subj.as_bytes(), &c).map(|x| (x.0, x.1, x.2)), Some((0, 20_001, vec![(0, 20_000), (19_998, 20_000)])), "subexpressions on a long subject: the last iteration is the longer alternative, `aa`");
}

#[test]
fn s14_9_f_0() {
    use rx::*;
    let code = |p: &str, cf: i32| comp(p, cf).0;
    assert_eq!(code("[", E), 2, "REG_BADPAT");
    assert_eq!(code("[[.foo.]]", E), 3, "REG_ECOLLATE");
    assert_eq!(code("[[:foo:]]", E), 4, "REG_ECTYPE");
    assert_eq!(code("a\\", E), 5, "REG_EESCAPE");
    assert_eq!(code("(a)\\2", E), 6, "REG_ESUBREG");
    assert_eq!(code("[a", E), 7, "REG_EBRACK");
    assert_eq!(code("(a", E), 8, "REG_EPAREN");
    assert_eq!(code("a\\)", 0), 8, "REG_EPAREN for an unmatched BRE \\)");
    assert_eq!(code("a{1", E), 9, "REG_EBRACE");
    assert_eq!(code("a{2,1}", E), 10, "REG_BADBR");
    assert_eq!(code("a{32768}", E), 10, "REG_BADBR: a count above RE_DUP_MAX");
    assert_eq!(code("a{32767}", E), 0);
    assert_eq!(code("[b-a]", E), 11, "REG_ERANGE");
    assert_eq!(code("*a", E), 13, "REG_BADRPT");
    let nest = |n: usize| "(".repeat(n) + "a" + &")".repeat(n);
    assert_eq!(code(&nest(200), E), 0, "200 nested groups");
    assert_eq!(code(&nest(300), E), 12, "REG_ESPACE: a tree deeper than the limit (256)");
    assert_eq!(code(&("a".to_string() + &"*".repeat(300)), E), 12, "REG_ESPACE: 300 repetitions of one operand");
    // Stacked `+` levels are compiled as written but grow linearly, one copy of the operand per
    // level (D-14.8 amendment, COMPAT-4); glibc doubles per level and answers REG_ESPACE from 24
    // levels on `b*` (over 30 s on this 17-level pattern, FZ-1). Past 2^21 matcher states the
    // builder still stops with REG_ESPACE.
    assert_eq!(code(&("b*".to_string() + &"+".repeat(10)), E), 0, "10 stacked levels compile");
    assert_eq!(code(")++++b*+++++++++++++++++h", E), 0, "17 stacked levels compile (D-14.8 amendment)");
    assert_eq!(code(&("(b*)".to_string() + &"+".repeat(24)), E), 0, "24 stacked levels over a group compile (D-14.8 amendment)");
    // Stacked minimums above 1 still need distinct copies per level, so they reach the cap.
    assert_eq!(code(&("a".to_string() + &"{2,}".repeat(24)), E), 12, "REG_ESPACE: 24 stacked {{2,}} levels");
    // regexec: REG_NOMATCH, and REG_ESPACE when a search with back-references exceeds its budget.
    assert_eq!(m("abc", E, "xyz"), None);
    let c = liella::locale::Locale::C;
    let p = regex::compile(b"(a*)*(b*)*\\1\\2c", regex::syntax::POSIX_EXTENDED, &c).unwrap();
    let subj = "ab".repeat(200);
    let w = Window { first: 0, last: subj.len(), backward: false };
    assert_eq!(regex::exec(&p, &c, subj.as_bytes(), w, &Options { budget: 1000, ..Options::default() }, true).err(), Some(ReError::ESpace));
}

// ------------------------------------------------------------------------------ §14.7, §14.8, §14.10 (phase 12)
//
// `fnmatch` runs through `liella::pattern::fnmatch` (C and C.UTF-8) and the export; `glob` through
// `liella::pattern::glob` over a virtual tree (an `Access` in the test: no file system, any
// order) and through the exports on a scratch directory under `target/`; `wordexp` through
// `liella::pattern::wordexp` with a `Shell` in the test (no process state is read or changed)
// and through the exports for the C structure. The whole behaviour record is compared by
// `patterns_products.rs`.

mod pat {
    use std::cell::RefCell;
    use std::collections::HashMap;

    pub(crate) use liella::error::Errno;
    pub(crate) use liella::locale::Locale;
    pub(crate) use liella::mem::TryVec;
    pub(crate) use liella::pattern::fnmatch as fm;
    pub(crate) use liella::pattern::glob as gb;
    pub(crate) use liella::pattern::wordexp as we;

    pub(crate) fn c() -> Locale {
        Locale::C
    }

    pub(crate) fn u() -> Locale {
        Locale::c_utf8(b"C.UTF-8")
    }

    /// `fnmatch` in `loc`: 0 or 1 (`FNM_NOMATCH`), as the C function returns.
    pub(crate) fn f(p: &str, s: &str, flags: i32, loc: &Locale) -> i32 {
        fb(p.as_bytes(), s.as_bytes(), flags, loc)
    }

    pub(crate) fn fb(p: &[u8], s: &[u8], flags: i32, loc: &Locale) -> i32 {
        if fm::fnmatch(p, s, flags, loc).unwrap() { 0 } else { fm::NOMATCH }
    }

    /// A virtual tree for `glob`: entries `(dir, name, d_type)`, modes by path (`lstat`; `stat`
    /// follows `links`), directories that fail to open, and what happened.
    #[derive(Default)]
    pub(crate) struct Tree {
        pub(crate) entries: Vec<(&'static str, &'static str, u8)>,
        pub(crate) modes: HashMap<String, u32>,
        pub(crate) links: HashMap<String, String>,
        pub(crate) fail_open: HashMap<String, Errno>,
        pub(crate) fail_read: bool,
        pub(crate) abort_on_error: bool,
        pub(crate) home: Option<&'static str>,
        pub(crate) opened: RefCell<Vec<String>>,
        pub(crate) reported: RefCell<Vec<(String, Errno)>>,
        pub(crate) lstat_calls: RefCell<Vec<String>>,
    }

    pub(crate) const DIR: u32 = 0o40755;
    pub(crate) const REG: u32 = 0o100644;
    pub(crate) const LNK: u32 = 0o120777;

    impl Tree {
        pub(crate) fn sample() -> Tree {
            let mut t = Tree { entries: vec![("v", "b", 8), ("v", "a", 8), ("v", ".h", 8), ("v", "d", 4), ("v", "u", 0), ("v", "dang", 10), ("v", "ld", 10), ("v", "locked", 4), ("v/d", "x", 8), ("v/d", ".y", 8), ("v/locked", "z", 8), (".", "v", 4)], ..Tree::default() };
            for (p, m) in [("v", DIR), ("v/b", REG), ("v/a", REG), ("v/.h", REG), ("v/d", DIR), ("v/u", REG), ("v/dang", LNK), ("v/ld", LNK), ("v/locked", DIR), ("v/d/x", REG), ("v/d/.y", REG), ("v/locked/z", REG)] {
                t.modes.insert(p.to_string(), m);
            }
            t.links.insert("v/ld".to_string(), "v/d".to_string());
            t.links.insert("v/dang".to_string(), "v/nowhere".to_string());
            t.fail_open.insert("v/locked".to_string(), Errno::EACCES);
            t
        }

        fn key(path: &[u8]) -> String {
            let s = String::from_utf8(path.to_vec()).unwrap();
            let t = s.trim_end_matches('/');
            if t.is_empty() && !s.is_empty() { "/".to_string() } else { t.to_string() }
        }

        /// `k` with a link among its directories followed.
        fn parents(&self, k: String) -> String {
            for (from, to) in &self.links {
                if let Some(rest) = k.strip_prefix(&format!("{from}/")) {
                    return format!("{to}/{rest}");
                }
            }
            k
        }

        /// `k` with every link followed.
        fn follow(&self, k: String) -> String {
            let k = self.parents(k);
            self.links.get(&k).cloned().unwrap_or(k)
        }
    }

    impl gb::Access for Tree {
        type Dir = (String, usize);
        fn open(&mut self, path: &[u8]) -> liella::error::Result<(String, usize)> {
            let k = Self::key(path);
            self.opened.borrow_mut().push(k.clone());
            if let Some(e) = self.fail_open.get(&k) {
                return Err(*e);
            }
            let real = self.follow(k);
            match self.modes.get(&real) {
                Some(&m) if m == DIR => Ok((real, 0)),
                Some(_) => Err(Errno::ENOTDIR),
                None if real == "." => Ok((real, 0)),
                None => Err(Errno::ENOENT),
            }
        }
        fn read(&mut self, dir: &mut (String, usize), name: &mut TryVec<u8>) -> liella::error::Result<Option<u8>> {
            if self.fail_read {
                return Err(Errno::ENOMEM);
            }
            while let Some(&(d, n, k)) = self.entries.get(dir.1) {
                dir.1 += 1;
                if d == dir.0 {
                    name.clear();
                    name.extend_from_slice(n.as_bytes())?;
                    return Ok(Some(k));
                }
            }
            Ok(None)
        }
        fn close(&mut self, _dir: (String, usize)) {}
        fn lstat(&mut self, path: &[u8]) -> liella::error::Result<u32> {
            let k = self.parents(Self::key(path));
            self.lstat_calls.borrow_mut().push(k.clone());
            let m = *self.modes.get(&k).ok_or(Errno::ENOENT)?;
            if path.ends_with(b"/") && self.stat(path)? != DIR {
                return Err(Errno::ENOTDIR);
            }
            Ok(m)
        }
        fn stat(&mut self, path: &[u8]) -> liella::error::Result<u32> {
            let k = self.follow(Self::key(path));
            self.modes.get(&k).copied().ok_or(Errno::ENOENT)
        }
        fn error(&mut self, path: &[u8], e: Errno) -> bool {
            self.reported.borrow_mut().push((Self::key(path), e));
            self.abort_on_error
        }
        fn home(&mut self, user: Option<&[u8]>) -> liella::error::Result<Option<TryVec<u8>>> {
            match (user, self.home) {
                (None, Some(h)) => Ok(Some(TryVec::from_slice(h.as_bytes())?)),
                (Some(b"root"), _) => Ok(Some(TryVec::from_slice(b"/root")?)),
                _ => Ok(None),
            }
        }
    }

    /// `glob` over `t` in `loc`: the paths, or the failure code.
    pub(crate) fn g(t: &mut Tree, p: &str, flags: i32, loc: &Locale) -> Result<(Vec<String>, bool), i32> {
        gb::glob(p.as_bytes(), flags, loc, t).map(|f| (f.paths.iter().map(|x| String::from_utf8(x.to_vec()).unwrap()).collect(), f.magic)).map_err(gb::Failure::code)
    }

    pub(crate) fn paths(t: &mut Tree, p: &str, flags: i32) -> Vec<String> {
        g(t, p, flags, &c()).unwrap_or_default().0
    }

    /// A shell for `wordexp`: variables, positional parameters, homes, files for globbing, and
    /// what it was asked.
    #[derive(Default)]
    pub(crate) struct Sh {
        pub(crate) vars: HashMap<String, String>,
        pub(crate) args: Vec<String>,
        pub(crate) files: Vec<&'static str>,
        pub(crate) home: Option<&'static str>,
        pub(crate) ran: Vec<(String, bool)>,
        pub(crate) stderr: String,
        pub(crate) reads: usize,
        pub(crate) sets: Vec<(String, String)>,
    }

    impl Sh {
        pub(crate) fn sample() -> Sh {
            let mut s = Sh { args: vec!["prog".into(), "one".into(), "two words".into()], files: vec!["w1", "w2", ".w3", "x.c"], home: Some("/home/h"), ..Sh::default() };
            for (k, v) in [("X", "a b"), ("Y", ""), ("N", "3"), ("C", "x:y::z"), ("P", "path/to/file.tar.gz"), ("G", "w*"), ("BAD", "1x"), ("NEG", "-4"), ("HEX", "0x1f")] {
                s.vars.insert(k.into(), v.into());
            }
            s
        }
    }

    impl we::Shell for Sh {
        fn var(&mut self, name: &[u8], out: &mut we::ValBuf) -> liella::error::Result<bool> {
            self.reads += 1;
            out.clear();
            match self.vars.get(std::str::from_utf8(name).unwrap()) {
                Some(v) => {
                    out.extend_from_slice(v.as_bytes())?;
                    Ok(true)
                }
                None => Ok(false),
            }
        }
        fn set_var(&mut self, name: &[u8], value: &[u8]) -> liella::error::Result<()> {
            let (n, v) = (String::from_utf8(name.to_vec()).unwrap(), String::from_utf8(value.to_vec()).unwrap());
            self.sets.push((n.clone(), v.clone()));
            self.vars.insert(n, v);
            Ok(())
        }
        fn arg_count(&mut self) -> usize {
            self.args.len().saturating_sub(1)
        }
        fn arg(&mut self, i: usize, out: &mut we::ValBuf) -> liella::error::Result<bool> {
            out.clear();
            match self.args.get(i) {
                Some(a) => {
                    out.extend_from_slice(a.as_bytes())?;
                    Ok(true)
                }
                None => Ok(false),
            }
        }
        fn pid(&mut self) -> i32 {
            4242
        }
        fn home(&mut self, user: Option<&[u8]>, out: &mut we::ValBuf) -> liella::error::Result<bool> {
            out.clear();
            let h = match user {
                None => self.vars.get("HOME").cloned().or(self.home.map(str::to_string)),
                Some(b"root") => Some("/root".to_string()),
                Some(_) => None,
            };
            match h {
                Some(h) => {
                    out.extend_from_slice(h.as_bytes())?;
                    Ok(true)
                }
                None => Ok(false),
            }
        }
        fn run(&mut self, command: &[u8], show_err: bool, out: &mut TryVec<u8>) -> liella::error::Result<()> {
            let cmd = String::from_utf8(command.to_vec()).unwrap();
            self.ran.push((cmd.clone(), show_err));
            // `echo ARGS`, the only command the tests run.
            if let Some(rest) = cmd.strip_prefix("echo ") {
                out.extend_from_slice(rest.as_bytes())?;
            }
            out.extend_from_slice(b"\n\n")?;
            Ok(())
        }
        fn stderr(&mut self, text: &[u8]) {
            self.stderr.push_str(std::str::from_utf8(text).unwrap());
        }
        fn glob(&mut self, pattern: &[u8], out: &mut TryVec<TryVec<u8>>) -> liella::error::Result<()> {
            let mut m: Vec<&str> = self.files.iter().copied().filter(|f| fm::fnmatch(pattern, f.as_bytes(), fm::PERIOD, &Locale::C).unwrap()).collect();
            m.sort_unstable();
            for f in m {
                out.push(TryVec::from_slice(f.as_bytes())?)?;
            }
            Ok(())
        }
    }

    /// The fields, failing (as memory would) after `limit` of them.
    pub(crate) struct Out {
        pub(crate) words: Vec<String>,
        pub(crate) limit: usize,
    }

    impl we::Words for Out {
        fn push(&mut self, word: &[u8]) -> liella::error::Result<()> {
            if self.words.len() >= self.limit {
                return Err(Errno::ENOMEM);
            }
            self.words.push(String::from_utf8(word.to_vec()).unwrap());
            Ok(())
        }
    }

    /// `wordexp` with `sh`: the words, or the failure code.
    pub(crate) fn w_with(sh: &mut Sh, words: &str, flags: i32) -> Result<Vec<String>, i32> {
        let mut out = Out { words: Vec::new(), limit: usize::MAX };
        we::wordexp(words.as_bytes(), flags, &Locale::C, sh, &mut out).map(|()| out.words).map_err(we::WordError::code)
    }

    pub(crate) fn w(words: &str) -> Result<Vec<String>, i32> {
        w_with(&mut Sh::sample(), words, 0)
    }

    pub(crate) fn wf(words: &str, flags: i32) -> Result<Vec<String>, i32> {
        w_with(&mut Sh::sample(), words, flags)
    }

    pub(crate) fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    /// Arithmetic: the value, or the failure code.
    pub(crate) fn a(expr: &str) -> Result<i64, i32> {
        a_with(expr, 0)
    }

    pub(crate) fn a_with(expr: &str, flags: i32) -> Result<i64, i32> {
        wf(&format!("$(({expr}))"), flags).map(|r| r[0].parse().unwrap())
    }

    // The C structures, through the exports.

    pub(crate) fn word(addr: usize, i: usize) -> usize {
        let mut v = 0usize;
        liella_boundary::posix::s04_strings::memcpy((&raw mut v).cast(), core::ptr::with_exposed_provenance::<core::ffi::c_void>(addr + 8 * i), 8);
        v
    }

    pub(crate) fn text(p: usize) -> String {
        let mut out = Vec::new();
        loop {
            let mut ch = 0u8;
            liella_boundary::posix::s04_strings::memcpy((&raw mut ch).cast(), core::ptr::with_exposed_provenance::<core::ffi::c_void>(p + out.len()), 1);
            if ch == 0 {
                return String::from_utf8_lossy(&out).into_owned();
            }
            out.push(ch);
        }
    }

    /// A vector's `n` entries after `offs` slots: `None` for a null slot.
    pub(crate) fn vector(v: usize, n: usize) -> Vec<Option<String>> {
        (0..n).map(|i| match word(v, i) {
            0 => None,
            p => Some(text(p)),
        }).collect()
    }

    pub(crate) fn z(s: &str) -> std::ffi::CString {
        std::ffi::CString::new(s).unwrap()
    }

    /// A scratch directory under `target/` for this test (removed by the caller).
    pub(crate) fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch").join(format!("s14-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::canonicalize(&d).unwrap()
    }
}

// ---------------------------------------------------------------- §14.7 fnmatch

/// Empty, unknown and unterminated `[:name:]` class names in a bracket expression, in C and
/// C.UTF-8. The expected matches were taken from the reference C library (a black-box probe over
/// these patterns and texts, flags 0): a name that is empty, unknown, differently cased or
/// unterminated never matches as a class.
#[test]
fn fnmatch_class_names_empty_unknown_unterminated() {
    // Trace: no spec clause (regression guard for the class-name dispatch speed-up, PERF-FNMATCH-CLASS); D-14.5.
    use pat::*;
    let texts: [&str; 8] = ["a", "x", "-", ":", "1", "[", "]", ""];
    // (pattern, the texts among `texts` that match)
    let cases: [(&str, &[&str]); 13] = [
        ("[[::]]", &[]),
        ("[[:nosuch:]]", &[]),
        ("[[:alpha", &[]),
        ("[[:alpha:", &[]),
        ("[[:alpha:]", &[]),
        ("[[:alpha:]]", &["a", "x"]),
        ("[![:nosuch:]]", &[]),
        ("[[:Alpha:]]", &[]),
        ("[[:alpha:]x]", &["a", "x"]),
        ("[[:]]", &[]),
        ("[[:a:]]", &[]),
        ("[[:alpha:]-z]", &["a", "x", "-"]),
        ("[[:alpha:][:digit:]]", &["a", "x", "1"]),
    ];
    for loc in [c(), u()] {
        for (p, want) in cases {
            for t in texts {
                let got = f(p, t, 0, &loc) == 0;
                assert_eq!(got, want.contains(&t), "{p} vs {t:?}");
            }
        }
        // Unterminated forms match only the text that spells them (reference probe).
        assert_eq!(f("[[:alpha", "[[:alpha", 0, &loc), 0);
        assert_eq!(f("[[:alpha:", "[[:alpha:", 0, &loc), 0);
        assert_eq!(f("[[:alpha:]", "[:", 0, &loc), 0);
        for (p, t) in [("[[::]]", "[[::]]"), ("[[:nosuch:]]", "[[:nosuch:]]"), ("[[:]]", "[[:]]"), ("[[:a:]]", "[[:a:]]"), ("[[:alpha:]", "[[:alpha:]")] {
            assert_eq!(f(p, t, 0, &loc), fm::NOMATCH, "{p}");
        }
    }
}

/// Spec §14.7 (Contract): Decide whether a Text matches a shell pattern.
#[test]
fn s14_7_c_0() {
    use pat::*;
    let cl = c();
    assert_eq!(f("abc", "abc", 0, &cl), 0);
    assert_eq!(f("abc", "abd", 0, &cl), fm::NOMATCH);
    assert_eq!(f("", "", 0, &cl), 0);
    assert_eq!(f("a", "", 0, &cl), fm::NOMATCH);
    // The C interface, in the calling thread's locale.
    use liella_boundary::posix::s14_search as s;
    assert_eq!(s::fnmatch(z("a*c").as_ptr(), z("abbc").as_ptr(), 0), 0);
    assert_eq!(s::fnmatch(z("a*c").as_ptr(), z("abbd").as_ptr(), 0), 1);
}

/// Spec §14.7 (Contract): **Pattern syntax:** `*`, `?`, `[...]` bracket expressions (ranges,
/// `[:class:]`, `[=equiv=]`, `[.coll.]`), and `\` escapes.
#[test]
fn s14_7_c_1() {
    use pat::*;
    let cl = c();
    // `*` and `?`.
    assert_eq!((f("*", "", 0, &cl), f("?", "", 0, &cl), f("a*b*c", "aXbYbZc", 0, &cl), f("*?*?", "a", 0, &cl)), (0, 1, 0, 1));
    // Brackets: sets, negation with `!` or `^`, `]` first, `-` first or last, ranges.
    assert_eq!((f("[abc]", "b", 0, &cl), f("[!abc]", "a", 0, &cl), f("[^abc]", "d", 0, &cl)), (0, 1, 0));
    assert_eq!((f("[]a]", "]", 0, &cl), f("[!]a]", "]", 0, &cl), f("[a-]", "-", 0, &cl), f("[-a]", "-", 0, &cl)), (0, 1, 0, 0));
    assert_eq!((f("[a-c]", "b", 0, &cl), f("[c-a]", "b", 0, &cl), f("[a-c-e]", "-", 0, &cl), f("[a-c-e]", "d", 0, &cl)), (0, 1, 0, 1), "a reversed range is empty");
    // Classes; an unknown class makes the bracket expression invalid, so `[` is literal.
    assert_eq!((f("[[:alpha:]]", "a", 0, &cl), f("[[:digit:][:upper:]]", "Q", 0, &cl), f("[![:space:]]", " ", 0, &cl)), (0, 0, 1));
    assert_eq!((f("[[:foo:]]", "f", 0, &cl), f("[[:foo:]]", "[f]", 0, &cl)), (1, 0), "the first `[` is literal; `[:foo:]` is then a set");
    assert_eq!((f("[[:alpha:]-z]", "-", 0, &cl), f("[a-[:alpha:]]", "b", 0, &cl)), (0, 1), "after a class `-` is literal; a class as a range end matches nothing");
    // Equivalence classes and collating symbols: one character each (shared with the regular
    // expressions, D-14.4); a multi-character name is invalid, so the `[` is literal.
    assert_eq!((f("[[=a=]]", "a", 0, &cl), f("[[=a=]b]", "b", 0, &cl), f("[[.-.]]", "-", 0, &cl), f("[[.a.]-c]", "b", 0, &cl)), (0, 0, 0, 0));
    assert_eq!((f("[[.ab.]]", "a", 0, &cl), f("[[.].]]", "]", 0, &cl)), (1, 0));
    let ut = u();
    assert_eq!(f("[[.é.]]", "é", 0, &ut), 0, "a single-character collating element in a UTF-8 locale");
    assert_eq!((f("[[=é=]]", "é", 0, &ut), f("[[=é=]]", "e", 0, &ut)), (0, 1), "without collation rules a character is its own class");
    assert_eq!(f("[[.é.]]", "é", 0, &cl), 1, "in C é is two characters: invalid, `[` literal");
    // A `[` that does not begin a valid bracket expression matches itself (XCU 2.13.1).
    assert_eq!((f("[", "[", 0, &cl), f("[a", "[a", 0, &cl), f("[a-", "[a-", 0, &cl), f("[!", "[!", 0, &cl), f("*[", "x[", 0, &cl)), (0, 0, 0, 0, 0));
    assert_eq!((f("[[:alpha]", "a", 0, &cl), f("[[=]", "[", 0, &cl), f("[[.]", "[", 0, &cl)), (1, 1, 1));
    // Escapes, also inside brackets; a trailing backslash matches nothing.
    assert_eq!((f("\\*", "*", 0, &cl), f("\\*", "a", 0, &cl), f("[\\]]", "]", 0, &cl), f("[a\\-c]", "-", 0, &cl), f("[a\\-c]", "b", 0, &cl)), (0, 1, 0, 0, 1));
    assert_eq!((f("\\", "\\", 0, &cl), f("a\\", "a\\", 0, &cl), f("a\\", "a", 0, &cl)), (1, 1, 1));
}

/// Spec §14.7 (Contract): **Flags:**
#[test]
fn s14_7_c_2() {
    use pat::*;
    let cl = c();
    assert_eq!((fm::PATHNAME, fm::NOESCAPE, fm::PERIOD, fm::LEADING_DIR, fm::CASEFOLD, fm::EXTMATCH, fm::NOMATCH), (1, 2, 4, 8, 16, 32, 1), "<fnmatch.h>");
    assert_eq!(f("*", "a/.b", fm::PATHNAME | fm::PERIOD, &cl), 1, "flags combine");
    assert_eq!((f("a", "a", 0x400, &cl), f("a", "b", 0x400, &cl)), (0, 1), "unknown flag bits are ignored");
}

/// Spec §14.7 (Contract): `FNM_NOESCAPE`;
#[test]
fn s14_7_c_2_1() {
    use pat::*;
    let cl = c();
    assert_eq!((f("\\*", "*", fm::NOESCAPE, &cl), f("\\*", "\\x", fm::NOESCAPE, &cl), f("\\", "\\", fm::NOESCAPE, &cl)), (1, 0, 0));
    assert_eq!((f("[\\]]", "\\]", fm::NOESCAPE, &cl), f("[\\]]", "]", fm::NOESCAPE, &cl)), (0, 1), "a backslash is a member of the set");
}

/// Spec §14.7 (Contract): `FNM_PATHNAME` (wildcards do not match `/`);
#[test]
fn s14_7_c_2_2() {
    use pat::*;
    let cl = c();
    let p = fm::PATHNAME;
    assert_eq!((f("*", "a/b", 0, &cl), f("*", "a/b", p, &cl), f("a?b", "a/b", p, &cl), f("a[/]b", "a/b", p, &cl), f("a[!x]b", "a/b", p, &cl)), (0, 1, 1, 1, 1));
    assert_eq!((f("*/*", "a/b", p, &cl), f("a\\/b", "a/b", p, &cl), f("a//b", "a/b", p, &cl)), (0, 0, 1), "slashes correspond one to one");
}

/// Spec §14.7 (Contract): `FNM_PERIOD` (a leading `.` must be matched literally);
#[test]
fn s14_7_c_2_3() {
    use pat::*;
    let cl = c();
    let (pe, pp) = (fm::PERIOD, fm::PATHNAME | fm::PERIOD);
    assert_eq!((f("*", ".a", pe, &cl), f("?", ".", pe, &cl), f("[.]", ".", pe, &cl), f("[!a]", ".", pe, &cl), f(".*", ".a", pe, &cl), f("\\.a", ".a", pe, &cl)), (1, 1, 1, 1, 0, 0));
    assert_eq!((f("a/*", "a/.b", pe, &cl), f("a/*", "a/.b", pp, &cl), f("*/.b", "a/.b", pp, &cl)), (0, 1, 0), "after `/` only with FNM_PATHNAME");
    assert_eq!(f("*.", ".", pe, &cl), 1, "a `*` at a leading period matches nothing, not even empty (XCU 2.13.3)");
    assert_eq!((f("*?[!a]", "a.", pe, &cl), f("/*?[!a]", "/a.", pp, &cl)), (0, 0), "a period that does not lead is an ordinary character (fnmatch(3); glibc differs, patterns_products)");
}

/// Spec §14.7 (Contract): `FNM_LEADING_DIR`;
#[test]
fn s14_7_c_2_4() {
    use pat::*;
    let cl = c();
    let l = fm::LEADING_DIR;
    assert_eq!((f("a", "a/b", l, &cl), f("a", "a/b", 0, &cl), f("a", "ab", l, &cl), f("a/", "a/b", l, &cl), f("", "/a", l, &cl), f("a/b", "a/b/c/d", l, &cl)), (0, 1, 1, 1, 0, 0));
    assert_eq!((f("*b", "a/b/c", l, &cl), f("*b", "a/b/c", l | fm::PATHNAME, &cl)), (0, 1), "without FNM_PATHNAME `*` crosses slashes before the cut");
}

/// Spec §14.7 (Contract): `FNM_CASEFOLD`;
#[test]
fn s14_7_c_2_5() {
    use pat::*;
    let cl = c();
    let cf = fm::CASEFOLD;
    assert_eq!((f("abc", "ABC", cf, &cl), f("[a-c]", "B", cf, &cl), f("[A-C]", "b", cf, &cl), f("\\A", "a", cf, &cl), f("[!a]", "A", cf, &cl)), (0, 0, 0, 0, 1));
    assert_eq!((f("[[:upper:]]", "a", cf, &cl), f("[[=a=]]", "A", cf, &cl), f("[[.a.]]", "A", cf, &cl)), (1, 1, 1), "classes, equivalence classes and collating symbols do not fold");
    assert_eq!((f("[Z-a]", "_", 0, &cl), f("[Z-a]", "_", cf, &cl)), (0, 1), "range end points fold ([z-a] is empty)");
    assert_eq!(f("é", "É", cf, &u()), 0, "multibyte case folding");
}

/// Spec §14.7 (Contract): `FNM_EXTMATCH` (ksh extended patterns `?(...)`, `*(...)`, `+(...)`,
/// `@(...)`, `!(...)`).
#[test]
fn s14_7_c_2_6() {
    use pat::*;
    let cl = c();
    let x = fm::EXTMATCH;
    assert_eq!((f("?(a|b)", "", x, &cl), f("?(a|b)", "a", x, &cl), f("?(a|b)", "ab", x, &cl)), (0, 0, 1));
    assert_eq!((f("*(ab)", "", x, &cl), f("*(ab)", "abab", x, &cl), f("*(ab)", "aba", x, &cl)), (0, 0, 1));
    assert_eq!((f("+(ab)", "", x, &cl), f("+(ab)", "ababab", x, &cl)), (1, 0));
    assert_eq!((f("@(a|b)", "a", x, &cl), f("@(a|b)", "ab", x, &cl), f("@()", "", x, &cl), f("@(|a)", "", x, &cl)), (0, 1, 0, 0));
    assert_eq!((f("!(a)", "a", x, &cl), f("!(a)", "", x, &cl), f("!(a)", "aa", x, &cl), f("x!(y)z", "xyyz", x, &cl), f("!(*.c)", "x.h", x, &cl)), (1, 0, 0, 0, 0));
    assert_eq!((f("!(!(a))", "a", x, &cl), f("*(*(a)b)", "abaab", x, &cl), f("+([[:digit:]])", "123", x, &cl), f("@([)])", ")", x, &cl)), (0, 0, 0, 0), "nesting and brackets");
    assert_eq!((f("*(a)", "x(a)", 0, &cl), f("?(a", "?(a", x, &cl), f("@(a\\|b)", "a", x, &cl)), (0, 0, 1), "without the flag, or unterminated, the operators are ordinary; `|` is not escapable");
    assert_eq!((f("!(x)", "a/b", fm::PATHNAME | x, &cl), f("*(a/b)", "a/b", fm::PATHNAME | x, &cl), f("!(x)", ".a", fm::PERIOD | x, &cl), f("*(x)", ".a", fm::PERIOD | x, &cl)), (0, 0, 0, 1));
    // Polynomial time, whatever the pattern (no backtracking).
    let t = std::time::Instant::now();
    let s = "a".repeat(400) + "b";
    assert_eq!(f("*(a|aa|aaa)*(a|aa)c", &s, x, &cl), 1);
    assert_eq!(f("*a*a*a*a*a*a*a*a*a*a*c", &s, 0, &cl), 1);
    crate::timeout::assert_within("extglob polynomial-time matching", std::time::Duration::from_secs(20), t.elapsed());
}

/// Spec §14.7 (Guarantees): Returns 0 for a match and `FNM_NOMATCH` otherwise. Multibyte-aware
/// in the current locale.
#[test]
fn s14_7_g_0() {
    use pat::*;
    let (cl, ut) = (c(), u());
    assert_eq!((f("?", "é", 0, &ut), f("??", "é", 0, &ut), f("?", "é", 0, &cl), f("??", "é", 0, &cl)), (0, 1, 1, 0), "a character in UTF-8, a byte in C");
    assert_eq!((f("[[:alpha:]]", "é", 0, &ut), f("[à-ÿ]", "é", 0, &ut), f("[!a]", "é", 0, &ut)), (0, 0, 0));
    // Text that is not valid in the locale is matched byte by byte (no error).
    assert_eq!((fb(b"?", b"\xff", 0, &ut), fb(b"\xc3*", "é".as_bytes(), 0, &ut), fb("[é]".as_bytes(), b"\xc3", 0, &ut), fb(b"?", b"\xe2\x82", 0, &ut)), (0, 0, 0, 1));
}

/// Spec §14.7 (Failure): Returns another non-zero value on error (for example invalid multibyte
/// text, or `ENOMEM`).
#[test]
fn s14_7_f_0() {
    use liella_boundary::posix::s14_search as s;
    use pat::*;
    // Extended groups nested deeper than the parser allows: an error, never a wrong answer.
    let deep = "@(".repeat(100) + "a" + &")".repeat(100);
    assert!(fm::fnmatch(deep.as_bytes(), b"a", fm::EXTMATCH, &c()).is_err());
    let r = s::fnmatch(z(&deep).as_ptr(), z("a").as_ptr(), fm::EXTMATCH);
    assert!(r != 0 && r != fm::NOMATCH, "an error value: {r}");
    assert_eq!(s::fnmatch(core::ptr::null(), z("a").as_ptr(), 0), -1, "a NULL pattern is an error, not a crash");
    // Invalid multibyte text is not an error (it is matched byte by byte, s14_7_g_0).
    assert_eq!(fb(b"\xff", b"\xff", 0, &u()), 0);
}

// ---------------------------------------------------------------- §14.8 glob

/// Spec §14.8 (Contract): Find all Paths matching a shell pattern, and return them in a
/// `glob_t {gl_pathc, gl_pathv, gl_offs}`.
#[test]
fn s14_8_c_0() {
    use liella_abi::callback::GlobT;
    use liella_boundary::posix::s14_search as s;
    use pat::*;
    let mut t = Tree::sample();
    assert_eq!(g(&mut t, "v/*", 0, &c()), Ok((v(&["v/a", "v/b", "v/d", "v/dang", "v/ld", "v/locked", "v/u"]), true)));
    assert_eq!(paths(&mut t, "v/*/x", 0), v(&["v/d/x", "v/ld/x"]), "directories reached through a link");
    assert_eq!(paths(&mut t, "v/a", 0), v(&["v/a"]));
    // The C structure, on a real directory.
    let d = scratch("glob-c0");
    for n in ["f1", "f2", "g"] {
        std::fs::write(d.join(n), b"").unwrap();
    }
    let mut gl = GlobT { gl_pathc: 9, gl_pathv: 0, gl_offs: 5, gl_flags: 0, gl_closedir: None, gl_readdir: None, gl_opendir: None, gl_lstat: None, gl_stat: None };
    let pat = format!("{}/f*", d.display());
    assert_eq!(s::glob(z(&pat).as_ptr(), 0, None, Some(&mut gl)), 0);
    assert_eq!((gl.gl_pathc, gl.gl_offs), (2, 0), "gl_offs is reset without GLOB_DOOFFS");
    let want: Vec<Option<String>> = vec![Some(format!("{}/f1", d.display())), Some(format!("{}/f2", d.display())), None];
    assert_eq!(vector(gl.gl_pathv, 3), want, "sorted, then a null pointer");
    s::globfree(Some(&mut gl));
    assert_eq!(gl.gl_pathv, 0);
    std::fs::remove_dir_all(&d).unwrap();
}

/// Spec §14.8 (Contract): **Standard flags:** `GLOB_ERR`, `GLOB_MARK`, `GLOB_NOSORT`,
/// `GLOB_DOOFFS`, `GLOB_NOCHECK`, `GLOB_APPEND`, `GLOB_NOESCAPE`.
#[test]
fn s14_8_c_1() {
    use liella_abi::callback::GlobT;
    use liella_boundary::posix::s14_search as s;
    use pat::*;
    // GLOB_ERR: a directory that cannot be opened is reported, and the walk goes on or stops.
    let mut t = Tree::sample();
    assert_eq!(paths(&mut t, "v/*/*", 0), v(&["v/d/x", "v/ld/x"]));
    assert_eq!(t.reported.borrow().as_slice(), &[("v/locked".to_string(), Errno::EACCES)]);
    let mut t = Tree::sample();
    assert_eq!(g(&mut t, "v/*/*", gb::ERR, &c()), Err(2), "GLOB_ABORTED, no partial results");
    let mut t = Tree::sample();
    assert_eq!(g(&mut t, "v/a/*", gb::ERR, &c()), Err(3), "ENOTDIR is not a read error");
    // GLOB_MARK: directories (and links to them) get a slash; files and dangling links do not.
    let mut t = Tree::sample();
    assert_eq!(paths(&mut t, "v/[dl]*", gb::MARK), v(&["v/d/", "v/dang", "v/ld/", "v/locked/"]));
    assert_eq!(paths(&mut t, "v/d", gb::MARK), v(&["v/d/"]));
    // GLOB_NOSORT: directory order (the Access's order here).
    let mut t = Tree::sample();
    assert_eq!(paths(&mut t, "v/[ab]", gb::NOSORT), v(&["v/b", "v/a"]));
    assert_eq!(paths(&mut t, "v/[ab]", 0), v(&["v/a", "v/b"]));
    // GLOB_NOCHECK: the pattern as given; GLOB_NOESCAPE: `\` is ordinary.
    let mut t = Tree::sample();
    assert_eq!(g(&mut t, "v/no\\*", gb::NOCHECK, &c()), Ok((v(&["v/no\\*"]), false)));
    assert_eq!(paths(&mut t, "v/\\a", 0), v(&["v/a"]));
    assert_eq!(g(&mut t, "v/\\a", gb::NOESCAPE, &c()), Err(3));
    // GLOB_DOOFFS and GLOB_APPEND through the C structure.
    let d = scratch("glob-c1");
    for n in ["a1", "a2", "b1"] {
        std::fs::write(d.join(n), b"").unwrap();
    }
    let mut gl = GlobT { gl_pathc: 0, gl_pathv: 0, gl_offs: 2, gl_flags: 0, gl_closedir: None, gl_readdir: None, gl_opendir: None, gl_lstat: None, gl_stat: None };
    let p1 = z(&format!("{}/a*", d.display()));
    assert_eq!(s::glob(p1.as_ptr(), gb::DOOFFS, None, Some(&mut gl)), 0);
    let p2 = z(&format!("{}/b*", d.display()));
    assert_eq!(s::glob(p2.as_ptr(), gb::DOOFFS | gb::APPEND, None, Some(&mut gl)), 0);
    let p3 = z(&format!("{}/none*", d.display()));
    assert_eq!(s::glob(p3.as_ptr(), gb::DOOFFS | gb::APPEND, None, Some(&mut gl)), 3, "nothing to append: GLOB_NOMATCH, the results kept");
    let names: Vec<Option<String>> = vector(gl.gl_pathv, 6).into_iter().map(|e| e.map(|p| p.rsplit('/').next().unwrap().to_string())).collect();
    assert_eq!((gl.gl_pathc, gl.gl_offs, names), (3, 2, vec![None, None, Some("a1".into()), Some("a2".into()), Some("b1".into()), None]));
    s::globfree(Some(&mut gl));
    std::fs::remove_dir_all(&d).unwrap();
}

/// Spec §14.8 (Contract): **GNU flags:**
#[test]
fn s14_8_c_2() {
    use liella_abi::callback::GlobT;
    use liella_boundary::posix::s14_search as s;
    use pat::*;
    assert_eq!((gb::PERIOD, gb::MAGCHAR, gb::ALTDIRFUNC, gb::BRACE, gb::NOMAGIC, gb::TILDE, gb::ONLYDIR, gb::TILDE_CHECK), (128, 256, 512, 1024, 2048, 4096, 8192, 16384), "<glob.h>");
    // An unknown flag bit: -1 with EINVAL (an error, never a guess).
    let mut gl = GlobT { gl_pathc: 0, gl_pathv: 0, gl_offs: 0, gl_flags: 0, gl_closedir: None, gl_readdir: None, gl_opendir: None, gl_lstat: None, gl_stat: None };
    assert_eq!(s::glob(z("*").as_ptr(), 1 << 20, None, Some(&mut gl)), -1);
    assert_eq!(liella_boundary::errno::get(), 22);
}

/// Spec §14.8 (Contract): `GLOB_PERIOD`, `GLOB_BRACE` (`{a,b}`), `GLOB_NOMAGIC`, `GLOB_TILDE`,
/// `GLOB_TILDE_CHECK` (`~user` expansion);
#[test]
fn s14_8_c_2_1() {
    use pat::*;
    let mut t = Tree::sample();
    // GLOB_PERIOD: wildcards match a leading period, in the last component only.
    assert_eq!(paths(&mut t, "v/?h", 0), Vec::<String>::new());
    assert_eq!(paths(&mut t, "v/?h", gb::PERIOD), v(&["v/.h"]));
    assert_eq!(paths(&mut t, "v/*/?y", gb::PERIOD), v(&["v/d/.y", "v/ld/.y"]));
    // GLOB_BRACE: left to right, each group sorted on its own, nested, empty alternatives.
    assert_eq!(paths(&mut t, "v/{b,a}", gb::BRACE), v(&["v/b", "v/a"]));
    assert_eq!(paths(&mut t, "v/{d*,[ab]}", gb::BRACE), v(&["v/d", "v/dang", "v/a", "v/b"]));
    assert_eq!(paths(&mut t, "v/{a,{b,nomatch}}", gb::BRACE), v(&["v/a", "v/b"]));
    assert_eq!(g(&mut t, "v/{a", gb::BRACE, &c()), Err(3), "unbalanced");
    assert_eq!(g(&mut t, "v/{x,y}", gb::BRACE | gb::NOCHECK, &c()), Ok((v(&["v/{x,y}"]), false)));
    // GLOB_NOMAGIC: the pattern only when it has no metacharacter.
    assert_eq!(g(&mut t, "v/none", gb::NOMAGIC, &c()), Ok((v(&["v/none"]), false)));
    assert_eq!(g(&mut t, "v/none*", gb::NOMAGIC, &c()), Err(3));
    assert_eq!(g(&mut t, "v/no\\ne", gb::NOMAGIC, &c()), Ok((v(&["v/no\\ne"]), false)), "an escape is not a metacharacter");
    // GLOB_TILDE: the home directory; an unknown user leaves the pattern as written.
    let mut t = Tree::sample();
    t.home = Some("v");
    assert_eq!(paths(&mut t, "~/a", gb::TILDE), v(&["v/a"]));
    assert_eq!(paths(&mut t, "~", gb::TILDE), v(&["v"]));
    assert_eq!(paths(&mut t, "~/a", 0), Vec::<String>::new(), "without the flag `~` is ordinary");
    assert_eq!(g(&mut t, "~nouser/a", gb::TILDE | gb::NOCHECK, &c()), Ok((v(&["~nouser/a"]), false)));
    assert_eq!(g(&mut t, "~nouser", gb::TILDE, &c()), Err(3), "no substitution, and no such name");
    // GLOB_TILDE_CHECK: an unknown user is GLOB_NOMATCH, even with GLOB_NOCHECK.
    assert_eq!(g(&mut t, "~nouser", gb::TILDE_CHECK | gb::NOCHECK, &c()), Err(3));
    assert_eq!(paths(&mut t, "~/*", gb::TILDE_CHECK), v(&["v/a", "v/b", "v/d", "v/dang", "v/ld", "v/locked", "v/u"]));
}

/// Spec §14.8 (Contract): `GLOB_ONLYDIR`;
#[test]
fn s14_8_c_2_2() {
    use pat::*;
    let mut t = Tree::sample();
    assert_eq!(paths(&mut t, "v/*", gb::ONLYDIR), v(&["v/d", "v/ld", "v/locked"]), "d_type, and stat for links and unknown types");
    assert_eq!(paths(&mut t, "v/a", gb::ONLYDIR), v(&["v/a"]), "a hint (glob(3)): a name without wildcards is returned");
    assert_eq!(paths(&mut t, "v/*/", 0), v(&["v/d/", "v/ld/", "v/locked/"]), "a trailing slash: directories only");
}

/// Spec §14.8 (Contract): `GLOB_ALTDIRFUNC`, with caller directory-access callbacks.
#[test]
fn s14_8_c_2_3() {
    use alt::*;
    use liella_abi::callback::GlobT;
    use liella_boundary::posix::s14_search as s;
    use pat::*;
    let run = |p: &str, flags: i32| -> (i32, Vec<String>) {
        let mut gl = GlobT { gl_pathc: 0, gl_pathv: 0, gl_offs: 0, gl_flags: 0, gl_closedir: Some(v_closedir), gl_readdir: Some(v_readdir), gl_opendir: Some(v_opendir), gl_lstat: Some(v_lstat), gl_stat: Some(v_stat) };
        let r = s::glob(z(p).as_ptr(), flags | gb::ALTDIRFUNC, None, Some(&mut gl));
        let out = if r == 0 { vector(gl.gl_pathv, usize::try_from(gl.gl_pathc).unwrap()).into_iter().map(Option::unwrap).collect() } else { Vec::new() };
        s::globfree(Some(&mut gl));
        (r, out)
    };
    assert_eq!(run("v/*", 0), (0, v(&["v/dang", "v/x", "v/y"])), "results come only from the callbacks");
    assert_eq!(run("v/*", gb::MARK), (0, v(&["v/dang", "v/x", "v/y/"])));
    // The current contract (glob@@GLIBC_2.27): a name without wildcards exists when gl_lstat
    // finds it, so a dangling link is found. (The compat glob@GLIBC_2.2.5, which uses gl_stat
    // and does not find it, comes with phase 15: the plan's ledger.)
    STATS.with(|s| s.borrow_mut().clear());
    assert_eq!(run("v/dang", 0), (0, v(&["v/dang"])));
    assert!(STATS.with(|s| s.borrow().iter().any(|c| c == "lstat:v/dang")), "gl_lstat decides existence");
    assert_eq!(run("v/nomatch", 0).0, 3);
}

/// Spec §14.8 (Contract): Results are sorted with `strcoll` unless `GLOB_NOSORT`. An error
/// callback may abort the walk.
#[test]
fn s14_8_c_3() {
    use pat::*;
    let names = ["b", "B", "a", "_a", "A", "é", "e", "1"];
    let mut t = Tree::default();
    t.modes.insert("s".into(), DIR);
    for n in names {
        t.entries.push(("s", n, 8));
        t.modes.insert(format!("s/{n}"), REG);
    }
    // C and C.UTF-8 order by bytes (code points); a locale with collation rules orders by them
    // (the products test covers en_US.UTF-8).
    let want = v(&["s/1", "s/A", "s/B", "s/_a", "s/a", "s/b", "s/e", "s/é"]);
    assert_eq!(g(&mut t, "s/*", 0, &c()).unwrap().0, want);
    assert_eq!(g(&mut t, "s/*", 0, &u()).unwrap().0, want);
    assert_eq!(g(&mut t, "s/?", 0, &u()).unwrap().0.len(), 7, "`?` is one character in UTF-8 (é included)");
    assert_eq!(g(&mut t, "s/?", 0, &c()).unwrap().0.len(), 6, "`?` is one byte in C (é is two)");
    // The error callback: nonzero stops the walk with GLOB_ABORTED.
    let mut t = Tree::sample();
    t.abort_on_error = true;
    assert_eq!(g(&mut t, "v/*/*", 0, &c()), Err(2));
    assert_eq!(t.reported.borrow().len(), 1);
}

/// Spec §14.8 (Contract): **`glob_pattern_p`** reports whether a pattern contains wildcards.
#[test]
fn s14_8_c_4() {
    use liella_boundary::posix::s14_search as s;
    use pat::*;
    let p = |pat: &str, q: i32| s::glob_pattern_p(z(pat).as_ptr(), q);
    assert_eq!((p("abc", 0), p("a*", 1), p("a?", 1), p("a[b]", 1), p("a[", 1), p("a]", 1), p("{a,b}", 1), p("~", 1)), (0, 1, 1, 1, 0, 0, 0, 0));
    assert_eq!((p("a\\*", 0), p("a\\*", 1), p("\\", 1), p("*\\", 1), p("", 1)), (1, 0, 0, 1, 0), "quote: a backslash escapes");
    assert_eq!(s::glob_pattern_p(core::ptr::null(), 1), 0);
}

/// Spec §14.8 (Failure): `GLOB_NOSPACE`, `GLOB_ABORTED`, `GLOB_NOMATCH`.
#[test]
fn s14_8_f_0() {
    use pat::*;
    let mut t = Tree::sample();
    assert_eq!(g(&mut t, "v/none*", 0, &c()), Err(3), "GLOB_NOMATCH");
    assert_eq!(g(&mut t, "", 0, &c()), Err(3));
    let mut t = Tree::sample();
    t.fail_open.insert("v/d".into(), Errno::EIO);
    assert_eq!(g(&mut t, "v/d/*", gb::ERR, &c()), Err(2), "GLOB_ABORTED");
    let mut t = Tree::sample();
    t.fail_read = true;
    assert_eq!(g(&mut t, "v/*", 0, &c()), Err(1), "GLOB_NOSPACE (memory exhausted while reading)");
}

mod alt {
    //! A virtual tree behind `GLOB_ALTDIRFUNC` callbacks: `v` holds `x` (a file), `y` (a
    //! directory holding `z`) and `dang` (a dangling link: `gl_lstat` finds it, `gl_stat` does
    //! not). Handles are numbers (never dereferenced by `glob`); the state is the calling
    //! thread's, so parallel tests do not share it.
    use core::ffi::{c_char, c_int, c_void};
    use std::cell::RefCell;

    type Entries = Vec<(&'static str, u8)>;

    thread_local! {
        pub(crate) static STATS: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
        static DIRS: RefCell<Vec<(Entries, usize)>> = const { RefCell::new(Vec::new()) };
        static DIRENT: RefCell<Box<[u8; 280]>> = RefCell::new(Box::new([0; 280]));
    }

    const BASE: usize = 0x1000;

    fn path(p: *const c_char) -> String {
        super::pat::text(p.expose_provenance())
    }

    pub(crate) extern "C-unwind" fn v_opendir(p: *const c_char) -> *mut c_void {
        let entries: Entries = match path(p).as_str() {
            "v" => vec![("x", 8), ("y", 4), ("dang", 10)],
            "v/y" => vec![("z", 8)],
            _ => return core::ptr::null_mut(),
        };
        let i = DIRS.with(|d| {
            let mut d = d.borrow_mut();
            d.push((entries, 0));
            d.len() - 1
        });
        core::ptr::with_exposed_provenance_mut(BASE + i)
    }

    pub(crate) extern "C-unwind" fn v_readdir(h: *mut c_void) -> *mut c_void {
        let i = h.addr() - BASE;
        let next = DIRS.with(|d| {
            let mut d = d.borrow_mut();
            let (entries, pos) = &mut d[i];
            let e = entries.get(*pos).copied();
            *pos += 1;
            e
        });
        let Some((name, kind)) = next else { return core::ptr::null_mut() };
        DIRENT.with(|b| {
            let mut b = b.borrow_mut();
            b.fill(0);
            b[18] = kind;
            b[19..19 + name.len()].copy_from_slice(name.as_bytes());
            b.as_mut_ptr().cast()
        })
    }

    pub(crate) extern "C-unwind" fn v_closedir(_h: *mut c_void) {}

    fn stat_common(kind: &str, p: *const c_char, st: *mut c_void) -> c_int {
        let p = path(p);
        STATS.with(|s| s.borrow_mut().push(format!("{kind}:{p}")));
        let mode: u32 = match (kind, p.as_str()) {
            (_, "v" | "v/y") => 0o40755,
            (_, "v/x" | "v/y/z") => 0o100644,
            ("lstat", "v/dang") => 0o120777,
            _ => return -1,
        };
        let mut buf = [0u8; 144];
        buf[24..28].copy_from_slice(&mode.to_le_bytes());
        liella_boundary::posix::s04_strings::memcpy(st, buf.as_ptr().cast(), 144);
        0
    }

    pub(crate) extern "C-unwind" fn v_stat(p: *const c_char, st: *mut c_void) -> c_int {
        stat_common("stat", p, st)
    }

    pub(crate) extern "C-unwind" fn v_lstat(p: *const c_char, st: *mut c_void) -> c_int {
        stat_common("lstat", p, st)
    }
}

// ---------------------------------------------------------------- §14.10 wordexp

/// Spec §14.10 (Contract): Perform POSIX shell word expansion on a Text:
#[test]
fn s14_10_c_0() {
    use pat::*;
    assert_eq!(w(""), Ok(vec![]));
    assert_eq!(w("  a   b\tc "), Ok(v(&["a", "b", "c"])));
    assert_eq!(w(r#"'a b' "c d" e\ f ''"#), Ok(v(&["a b", "c d", "e f", ""])));
    assert_eq!(w(r#""a\"b" 'a\b' "a\b" a\\b"#), Ok(v(&["a\"b", "a\\b", "a\\b", "a\\b"])));
    assert_eq!(w("a\\\nb \"c\\\nd\""), Ok(v(&["ab", "cd"])), "backslash-newline is removed");
    assert_eq!(w("#a a#b $ a$"), Ok(v(&["#a", "a#b", "$", "a$"])), "`#` is not a comment; a lone `$` is literal");
    // The C interface: one call, the words in a vector the caller frees with wordfree.
    use liella_abi::layout::wordexp_t;
    use liella_boundary::posix::s14_search as s;
    let mut we = wordexp_t { we_wordc: 0, we_wordv: 0, we_offs: 0 };
    assert_eq!(s::wordexp(z("one 'two three'").as_ptr(), Some(&mut we), 0), 0);
    assert_eq!((we.we_wordc, vector(we.we_wordv, 3)), (2, vec![Some("one".into()), Some("two three".into()), None]));
    s::wordfree(Some(&mut we));
    assert_eq!(we.we_wordv, 0);
}

/// Spec §14.10 (Contract): tilde, parameter and arithmetic expansion;
#[test]
fn s14_10_c_1() {
    use pat::*;
    // Tilde: HOME (as set, even empty), else the passwd entry; ~user; unknown users unchanged.
    assert_eq!(w("~ ~/x ~root ~root/x ~nouser a~ '~' \\~ x=~/a ~:b"), Ok(v(&["/home/h", "/home/h/x", "/root", "/root/x", "~nouser", "a~", "~", "~", "x=/home/h/a", "/home/h:b"])));
    let mut sh = Sh::sample();
    sh.vars.insert("HOME".into(), String::new());
    assert_eq!(w_with(&mut sh, "~ ~/x", 0), Ok(v(&["", "/x"])), "HOME set and empty is the empty value (XCU 2.6.1)");
    assert_eq!(w("~$X"), Ok(v(&["~a", "b"])), "a `$` ends the tilde prefix's claim: `$X` is expanded");
    // Parameters: the POSIX forms.
    assert_eq!(w("$X ${X}y \"$X\" $U ${U} \"$U\" $Y"), Ok(v(&["a", "b", "a", "by", "a b", ""])));
    assert_eq!(w("${U:-d} ${U-d} ${Y:-d} ${Y-d} ${X:+p} ${Y:+p} ${Y+p} ${#X} ${#U}"), Ok(v(&["d", "d", "d", "p", "p", "3", "0"])));
    assert_eq!(w("${P%.*} ${P%%.*} ${P#*/} ${P##*/} ${P#\"path\"} ${X#} ${P%x}"), Ok(v(&["path/to/file.tar", "path/to/file", "to/file.tar.gz", "file.tar.gz", "/to/file.tar.gz", "a", "b", "path/to/file.tar.gz"])));
    assert_eq!(w("${U:-a b} \"${U:-a b}\" ${U:-'a b'} ${U:-~}"), Ok(v(&["a", "b", "a b", "a b", "/home/h"])), "quoted parts of the word are not split");
    assert_eq!(w("${U:-\\}} ${U:-{x}}"), Ok(v(&["}", "{x}"])), "an escaped brace; brace levels are counted");
    let mut sh = Sh::sample();
    assert_eq!(w_with(&mut sh, "${U:=set} $U", 0), Ok(v(&["set", "set"])));
    assert_eq!(sh.sets, vec![("U".to_string(), "set".to_string())], "${{U:=w}} assigns through the shell (the boundary keeps it local to the call, D-14.7)");
    // Special parameters: the program's arguments, the PID; $? $- $! stay literal.
    assert_eq!(w("$0 $1 ${2} $# $$ $? $- $! ${10}"), Ok(v(&["prog", "one", "two", "words", "2", "4242", "$?", "$-", "$!"])));
    assert_eq!(w("$@ \"$@\" \"$*\""), Ok(v(&["one", "two", "words", "one", "two words", "one two words"])));
    // Arithmetic: every POSIX operator on signed 64-bit long (owner ruling 2026-10-03).
    for (e, r) in [("1+2", 3), ("7-10", -3), ("6*7", 42), ("7/2", 3), ("-7/2", -3), ("7%3", 1), ("-3%2", -1), ("1<<4", 16), ("-16>>2", -4), ("1<2", 1), ("2<=2", 1), ("3>4", 0), ("4>=4", 1), ("1==1", 1), ("1!=1", 0), ("5&3", 1), ("5^3", 6), ("5|3", 7), ("1&&0", 0), ("0||2", 1), ("1?2:3", 2), ("0?2:3", 3), ("-(2+3)", -5), ("+4", 4), ("~0", -1), ("!0", 1), ("!5", 0), ("2*(3+4)", 14), ("010", 8), ("0x1F", 31), ("0X10", 16), ("1?0?4:5:6", 5), ("2+3*4", 14), ("1-1-1", -1)] {
        assert_eq!(a(e), Ok(r), "$(({e}))");
    }
    assert_eq!(a("N*2"), Ok(6), "a bare name");
    assert_eq!(a("$N*2"), Ok(6));
    assert_eq!(a("NEG+HEX"), Ok(27), "values with a sign or in hex");
    assert_eq!(a("U+1"), Ok(1), "unset is 0 without WRDE_UNDEF");
    assert_eq!(a("0&&1/0"), Ok(0), "short-circuit: the skipped side is not evaluated");
    assert_eq!(a("1||1/0"), Ok(1));
    assert_eq!(a("1?1:1/0"), Ok(1));
    assert_eq!(a("-9223372036854775807-1"), Ok(i64::MIN));
    assert_eq!(w("$[1+2] x$((1+2))y \"$((2*3))\""), Ok(v(&["3", "x3y", "6"])));
}

/// Spec §14.10 (Contract): command substitution (`$(...)`, backquotes) unless `WRDE_NOCMD`;
#[test]
fn s14_10_c_2() {
    use pat::*;
    let mut sh = Sh::sample();
    assert_eq!(w_with(&mut sh, "$(echo a b) \"$(echo c d)\" `echo e` x$(echo f)y", 0), Ok(v(&["a", "b", "c d", "e", "xfy"])));
    assert_eq!(sh.ran.len(), 4, "each substitution runs once");
    assert!(sh.ran.iter().all(|(_, show)| !show), "standard error to /dev/null without WRDE_SHOWERR");
    assert_eq!(w_with(&mut sh, "$(echo w*)", we::SHOWERR), Ok(v(&["w1", "w2"])), "the output is split and expanded");
    assert!(sh.ran.last().unwrap().1, "WRDE_SHOWERR");
    assert_eq!(w("$((1+$(echo 2)))"), Ok(v(&["3"])), "inside arithmetic");
    assert_eq!(w("${U:-`echo x`} \"${U:-`echo y`}\""), Ok(v(&["x", "y"])), "a backquote inside ${{...}} is a substitution (owner ruling)");
    let mut sh = Sh::sample();
    w_with(&mut sh, "$(echo ')') `echo \\`echo n\\``", 0).unwrap();
    let ran: Vec<&str> = sh.ran.iter().map(|(c, _)| c.as_str()).collect();
    assert_eq!(ran, ["echo ')'", "echo `echo n`"], "the command texts: a quoted `)` inside; backquotes nest with `\\``");
    // WRDE_NOCMD: syntactic, fail closed, before anything is expanded (owner ruling): every form.
    for words in ["$(echo x)", "`echo x`", "\"$(echo x)\"", "\"`echo x`\"", "$(echo $(echo x))", "$((1+$(echo 1)))", "$((`echo 1`+1))", "\"$(($(echo 1)))\"", "${U:-$(echo x)}", "${X:-$(echo x)}", "${X:+$(echo x)}", "${P#$(echo x)}", "${U:-`echo x`}", "\"${X:-`echo x`}\"", "$()", "``", "a $(echo x) b", "${#X}$(echo x)", "$X $(echo x)", "$U $(echo x)"] {
        let mut sh = Sh::sample();
        assert_eq!(w_with(&mut sh, words, we::NOCMD | we::UNDEF), Err(4), "WRDE_CMDSUB for {words}");
        assert_eq!((sh.reads, sh.ran.len()), (0, 0), "nothing expanded before the refusal: {words}");
    }
    assert_eq!(wf("'$(echo x)' \\`x\\` $((1+2)) $X ~", we::NOCMD), Ok(v(&["$(echo x)", "`x`", "3", "a", "b", "/home/h"])), "quoted forms are text; other expansions work");
    assert_eq!(wf("x|$(echo x)", we::NOCMD), Err(2), "a syntax error before the substitution comes first");
    assert_eq!(wf("$(echo x", we::NOCMD), Err(5));
}

/// Spec §14.10 (Contract): field splitting on `IFS`;
#[test]
fn s14_10_c_3() {
    use pat::*;
    let with_ifs = |ifs: Option<&str>, words: &str| {
        let mut sh = Sh::sample();
        sh.vars.insert("S".into(), "  lead  trail  ".into());
        sh.vars.insert("D".into(), " a : b :: c ".into());
        if let Some(i) = ifs {
            sh.vars.insert("IFS".into(), i.into());
        }
        w_with(&mut sh, words, 0)
    };
    assert_eq!(with_ifs(None, "$S $C"), Ok(v(&["lead", "trail", "x:y::z"])), "unset: blanks only");
    assert_eq!(with_ifs(Some(":"), "$C a$C \"$C\" $S"), Ok(v(&["x", "y", "", "z", "ax", "y", "", "z", "x:y::z", "  lead  trail  "])), "an empty field between two separators");
    assert_eq!(with_ifs(Some(" :"), "$D x${D}y"), Ok(v(&["a", "b", "", "c", "xa", "b", "", "cy"])), "white space around another separator is part of it");
    assert_eq!(with_ifs(Some(":x"), "$C"), Ok(v(&["", "", "y", "", "z"])));
    assert_eq!(with_ifs(Some(""), "$X $C"), Ok(v(&["a b", "x:y::z"])), "empty: no splitting");
    assert_eq!(with_ifs(Some(":"), "a:b c $(echo a:b c)"), Ok(v(&["a:b", "c", "a", "b c"])), "only expansion results are split");
    assert_eq!(with_ifs(None, "$Y$Y a$Y ''$Y \"$Y\""), Ok(v(&["a", "", ""])), "an empty unquoted expansion is no field");
}

/// Spec §14.10 (Contract): pathname expansion and quote removal. The result is a list of words.
/// Other flags are `WRDE_DOOFFS`, `WRDE_APPEND`, `WRDE_REUSE`, `WRDE_SHOWERR` and `WRDE_UNDEF`.
#[test]
fn s14_10_c_4() {
    use liella_abi::layout::wordexp_t;
    use liella_boundary::posix::s14_search as s;
    use pat::*;
    assert_eq!(w("w* .* w? x.[ch] nomatch* w\\* 'w*' \"w*\" [!w]*"), Ok(v(&["w1", "w2", ".w3", "w1", "w2", "x.c", "nomatch*", "w*", "w*", "w*", "x.c"])));
    assert_eq!(w("$G \"$G\" w*/"), Ok(v(&["w1", "w2", "w*", "w*/"])), "results of expansions are expanded; `w*/` names directories only");
    assert_eq!((we::DOOFFS, we::APPEND, we::NOCMD, we::REUSE, we::SHOWERR, we::UNDEF), (1, 2, 4, 8, 16, 32), "<wordexp.h>");
    // WRDE_UNDEF: an unset variable is an error, in every form; set-but-empty is fine.
    for words in ["$U", "${U}", "\"$U\"", "${#U}", "${U%x}", "${U:-x}", "${U-x}", "${U:+x}", "${U:=x}", "$((U))", "$((U+1))", "$(($U))", "$((1?U:2))"] {
        assert_eq!(wf(words, we::UNDEF), Err(3), "WRDE_BADVAL for {words}");
    }
    assert_eq!(wf("$Y $1 '$U' \\$U $((0?U:2))", we::UNDEF), Ok(v(&["one", "$U", "$U", "2"])));
    // WRDE_DOOFFS, WRDE_APPEND, WRDE_REUSE through the C structure.
    let mut wx = wordexp_t { we_wordc: 0, we_wordv: 0, we_offs: 2 };
    assert_eq!(s::wordexp(z("a b").as_ptr(), Some(&mut wx), we::DOOFFS), 0);
    assert_eq!(s::wordexp(z("c").as_ptr(), Some(&mut wx), we::DOOFFS | we::APPEND), 0);
    assert_eq!((wx.we_wordc, wx.we_offs, vector(wx.we_wordv, 6)), (3, 2, vec![None, None, Some("a".into()), Some("b".into()), Some("c".into()), None]));
    assert_eq!(s::wordexp(z("d").as_ptr(), Some(&mut wx), we::DOOFFS | we::REUSE), 0);
    assert_eq!((wx.we_wordc, vector(wx.we_wordv, 4)), (1, vec![None, None, Some("d".into()), None]), "WRDE_REUSE frees the earlier result first");
    s::wordfree(Some(&mut wx));
    s::wordfree(Some(&mut wx));
    assert_eq!((wx.we_wordv, wx.we_wordc), (0, 1), "wordfree leaves the count; twice is harmless");
    let mut stale = wordexp_t { we_wordc: 0, we_wordv: 0, we_offs: 1000 };
    assert_eq!(s::wordexp(z("a b").as_ptr(), Some(&mut stale), 0), 0);
    assert_eq!((stale.we_wordc, stale.we_offs, vector(stale.we_wordv, 3)), (2, 0, vec![Some("a".into()), Some("b".into()), None]), "without WRDE_DOOFFS, we_offs becomes 0");
    s::wordfree(Some(&mut stale));
    let mut zero = wordexp_t { we_wordc: 0, we_wordv: 0, we_offs: 0 };
    s::wordfree(Some(&mut zero));
    assert_eq!(s::wordexp(z("a").as_ptr(), Some(&mut zero), 0x400), 0, "unknown flag bits are ignored");
    s::wordfree(Some(&mut zero));
}

/// Spec §14.10 (Failure): `WRDE_BADCHAR` (an unquoted `|&;<>(){}` or newline), `WRDE_BADVAL` (an
/// undefined variable with `WRDE_UNDEF`), `WRDE_CMDSUB` (command substitution with
/// `WRDE_NOCMD`), `WRDE_NOSPACE`, `WRDE_SYNTAX`.
#[test]
fn s14_10_f_0() {
    use liella_abi::layout::wordexp_t;
    use liella_boundary::posix::s14_search as s;
    use pat::*;
    // WRDE_BADCHAR, unquoted only.
    for c in ["|", "&", ";", "<", ">", "(", ")", "{", "}", "\n"] {
        assert_eq!(w(&format!("a{c}b")), Err(2), "unquoted {c:?}");
        assert_eq!(w(&format!("'a{c}b' \"a{c}b\" a\\{c}b")).map(|r| r.len()), Ok(3), "quoted or escaped {c:?}");
    }
    assert_eq!(w("${U:-a|b;c}"), Ok(v(&["a|b;c"])), "inside ${{...}}");
    assert_eq!(w("${U:-}}"), Err(2), "the first `}}` closes");
    // WRDE_SYNTAX: unterminated forms, a trailing backslash, forms POSIX does not define.
    for words in ["'", "\"", "`", "a\\", "\"\\", "$(", "$((", "$((1+2)", "${", "${X", "${X:-", "${U:-a{b", "${U:-'}", "${X:2}", "${X/a/z}", "${!X}", "${}", "${ X}", "${X:}", "${#}", "${#X:-}"] {
        assert_eq!(w(words), Err(5), "WRDE_SYNTAX for {words:?}");
    }
    // Arithmetic errors (owner ruling): WRDE_SYNTAX, never a wrong value.
    for e in ["1/0", "1%0", "1<<64", "1<<-1", "1>>64", "1<<63", "9223372036854775807+1", "-9223372036854775807-2", "4294967296*4294967296", "-(-9223372036854775807-1)", "(-9223372036854775807-1)/-1", "(-9223372036854775807-1)%-1", "N=1", "N+=1", "N-=1", "N*=2", "N/=2", "N%=2", "N<<=1", "N>>=1", "N&=1", "N^=1", "N|=1", "N++", "N--", "2**3", "1,2", "08", "0x", "1+", "(1", "1)+(2", "a b", "X", "BAD", "Y"] {
        assert_eq!(a(e), Err(5), "$(({e}))");
    }
    assert_eq!(a_with("U", we::UNDEF), Err(3), "an unset variable with WRDE_UNDEF");
    // WRDE_NOSPACE: the fields expanded before the failure are kept (POSIX).
    let mut sh = Sh::sample();
    let mut out = Out { words: Vec::new(), limit: 2 };
    assert_eq!(we::wordexp(b"a b c d", 0, &Locale::C, &mut sh, &mut out), Err(we::WordError::NoSpace));
    assert_eq!(out.words, v(&["a", "b"]));
    // `${X:?word}`: the message, then WRDE_BADVAL.
    let mut sh = Sh::sample();
    assert_eq!(w_with(&mut sh, "${U:?} ${Y:?gone}", 0), Err(3));
    assert_eq!(sh.stderr, "U: parameter null or not set\n");
    let mut sh = Sh::sample();
    assert_eq!(w_with(&mut sh, "${Y:?gone}", 0), Err(3));
    assert_eq!(sh.stderr, "Y: gone\n");
    // Order: the whole text is parsed before anything is expanded.
    assert_eq!(wf("$U |", we::UNDEF), Err(2));
    assert_eq!(wf("| $U", we::UNDEF), Err(2));
    assert_eq!(wf("$U '", we::UNDEF), Err(5));
    // The C structure after each kind of error: untouched, except WRDE_NOSPACE (above).
    for (words, flags, code) in [("a b |", 0, 2), ("a b $(", 0, 5), ("a b $U", we::UNDEF, 3), ("a b $(echo x)", we::NOCMD, 4), ("a b $((1/0))", 0, 5)] {
        let mut wx = wordexp_t { we_wordc: 777, we_wordv: 0, we_offs: 5 };
        assert_eq!(s::wordexp(z(words).as_ptr(), Some(&mut wx), flags), code, "{words}");
        assert_eq!((wx.we_wordc, wx.we_wordv, wx.we_offs), (777, 0, 5), "{words}: nothing written");
    }
    let mut wx = wordexp_t { we_wordc: 0, we_wordv: 0, we_offs: 0 };
    assert_eq!(s::wordexp(z("one").as_ptr(), Some(&mut wx), 0), 0);
    assert_eq!(s::wordexp(z("two |").as_ptr(), Some(&mut wx), we::APPEND), 2);
    assert_eq!((wx.we_wordc, vector(wx.we_wordv, 2)), (1, vec![Some("one".into()), None]), "a failed WRDE_APPEND adds nothing");
    s::wordfree(Some(&mut wx));
    assert_eq!(s::wordexp(core::ptr::null(), Some(&mut wx), 0), 5, "a NULL string is an error, not a crash");
}

/// FZ-2: `Pattern::prefix_matches` (one pass over the string) agrees with `Pattern::matches` on
/// every prefix that ends at a character boundary, in a byte locale and in UTF-8, for values with
/// invalid bytes (a prefix is read as text exactly when it is valid text).
#[test]
fn fz2_prefix_matches_agree() {
    // Trace: no spec clause (internal one-pass matching used by the wordexp prefix-removal operators; results must equal the per-prefix match); D-14.5.
    use pat::*;
    let patterns: [&[u8]; 14] = [b"", b"a", b"b", b"a*", b"*b", b"a?b", b"[ab]*", b"*", b"?", b"\xc3\xa9*", b"\xe5", b"*\x85", b"\xc3\xa9?", b"@(a|ab)*"];
    let values: [&[u8]; 8] = [b"", b"aab", b"abab\xe5b", b"\xc3\xa9ab", b"\xc3\xa9\xe5\xc3\xa9b", b"\x85\xe5ab", b"ab\xc3", b"\xe2\x82\xac\xff\xe2\x82\xacb"];
    for (loc, bytes_only) in [(Locale::C, true), (Locale::c_utf8(b"C.UTF-8"), false)] {
        for p in patterns {
            for flags in [0, fm::EXTMATCH] {
                let Ok(pat) = fm::Pattern::new(p, flags, &loc) else { continue };
                for v in values {
                    // Boundaries as the matcher segments the value: a well-formed character is one unit, any other byte is one.
                    let mut bounds = vec![0];
                    let mut i = 0;
                    while i < v.len() {
                        let rest = &v[i..v.len().min(i + 4)];
                        let ok = match std::str::from_utf8(rest) {
                            Ok(s) => s.chars().next().map_or(1, char::len_utf8),
                            Err(e) => std::str::from_utf8(&rest[..e.valid_up_to()]).ok().and_then(|s| s.chars().next()).map_or(1, char::len_utf8),
                        };
                        i += if bytes_only { 1 } else { ok };
                        bounds.push(i);
                    }
                    let ends = pat.prefix_matches(v, &loc).unwrap().unwrap();
                    for &e in &bounds {
                        assert_eq!(ends.get(e).copied(), Some(pat.matches(&v[..e], &loc).unwrap()), "pattern {p:?} flags {flags} value {v:?} prefix {e}");
                    }
                }
            }
        }
    }
    let pat = fm::Pattern::new(b"a*", fm::PERIOD, &Locale::C).unwrap();
    assert!(pat.prefix_matches(b"ab", &Locale::C).unwrap().is_none(), "FNM_PERIOD is matched prefix by prefix");
}

/// FZ-2: the prefix-removal operators were quadratic in the value's length (a long value with a
/// pattern that fails at every prefix took seconds; a fuzz input ran past libFuzzer's timeout).
#[test]
fn fz2_prefix_removal_is_linear() {
    // Trace: no spec clause (an implementation bound: prefix removal is not quadratic in the value; fuzz FZ-2); D-14.5.
    use pat::*;
    let n = 150_000;
    let mut sh = Sh::sample();
    sh.vars.insert("V".into(), "a".repeat(n) + "\u{e9}b");
    let t = std::time::Instant::now();
    for (words, want) in [("${V#b}", n + 3), ("${V##b*}", n + 3), ("${V#a?b}", n + 3), ("${V#a*\u{e9}}", 1), ("${V##a*}", 0), ("${V#}", n + 3)] {
        let r = w_with(&mut sh, words, 0).unwrap();
        assert_eq!(r.first().map_or(0, String::len), want, "{words}");
    }
    crate::timeout::assert_within(&format!("prefix removal over {n} bytes"), std::time::Duration::from_secs(5), t.elapsed());
}

/// WORDEXP-SUFFIX: `Pattern::suffix_matches` (one backward pass) agrees with `Pattern::matches`
/// on every suffix that starts at a character boundary, in a byte locale and in UTF-8, for values
/// with invalid bytes (a suffix is read as text exactly when it is valid text).
#[test]
fn wordexp_suffix_matches_agree() {
    // Trace: no spec clause (internal one-pass matching used by the wordexp suffix-removal operators; results must equal the per-suffix match); D-14.5.
    use pat::*;
    let patterns: [&[u8]; 20] = [
        b"", b"a", b"b", b"a*", b"*b", b"a?b", b"[ab]*", b"*", b"?", b"\xc3\xa9*", b"\xe5", b"*\x85", b"\xc3\xa9?", b"@(a|ab)*", b"\\*", b"a\\*", b"[!a]*b", b"[[:alpha:]]", b"*a*b*a*", b"abababababababab",
    ];
    // Values ending in a multibyte character: their last suffixes are text, not bytes (MUTATION-CAMPAIGN-1B-4).
    let values: [&[u8]; 12] = [b"", b"aab", b"abab\xe5b", b"\xc3\xa9ab", b"\xc3\xa9\xe5\xc3\xa9b", b"\x85\xe5ab", b"ab\xc3", b"\xe2\x82\xac\xff\xe2\x82\xacb", b"a*b*", b"a\xc3\xa9", b"\xc3\xa9", b"a/b"];
    let mut some = 0;
    for (loc, bytes_only) in [(Locale::C, true), (Locale::c_utf8(b"C.UTF-8"), false)] {
        // The flags the one-pass path takes (NOESCAPE, CASEFOLD) and those it must decline (it may not answer wrongly).
        let flag_sets = [0, fm::NOESCAPE, fm::CASEFOLD, fm::PATHNAME, fm::LEADING_DIR, fm::EXTMATCH];
        for (p, flags) in patterns.iter().flat_map(|&p| flag_sets.map(|f| (p, f))) {
            let Ok(pat) = fm::Pattern::new(p, flags, &loc) else { continue };
            for v in values {
                // Boundaries as the matcher segments the value: a well-formed character is one unit, any other byte is one.
                let mut bounds = vec![0];
                let mut i = 0;
                while i < v.len() {
                    let rest = &v[i..v.len().min(i + 4)];
                    let ok = match std::str::from_utf8(rest) {
                        Ok(s) => s.chars().next().map_or(1, char::len_utf8),
                        Err(e) => std::str::from_utf8(&rest[..e.valid_up_to()]).ok().and_then(|s| s.chars().next()).map_or(1, char::len_utf8),
                    };
                    i += if bytes_only { 1 } else { ok };
                    bounds.push(i);
                }
                // A trailing backslash may be declined (None): the caller then matches each suffix itself.
                let Some(starts) = pat.suffix_matches(v, &loc).unwrap() else { continue };
                some += 1;
                for &s in &bounds {
                    assert_eq!(starts.get(s).copied(), Some(pat.matches(&v[s..], &loc).unwrap()), "pattern {p:?} flags {flags} value {v:?} suffix from {s}");
                }
            }
        }
    }
    assert!(some > 200, "the one-pass path is the one under test ({some})");
    // The documented contract: a suffix's match under these flags depends on what precedes it (or
    // the pattern may hold groups), so the caller matches each suffix itself.
    for flags in [fm::PERIOD, fm::LEADING_DIR, fm::PATHNAME, fm::EXTMATCH] {
        let pat = fm::Pattern::new(b"a*", flags, &Locale::C).unwrap();
        assert!(pat.suffix_matches(b"ab", &Locale::C).unwrap().is_none(), "flags {flags} are matched suffix by suffix");
    }
}

/// WORDEXP-SUFFIX: `${v%pat}` removes the shortest matching suffix, `${v%%pat}` the longest
/// (XCU 2.6.2), for `*`, `?`, brackets, escaped characters, an empty pattern, a pattern longer
/// than the value and multibyte values.
#[test]
fn wordexp_suffix_removal_shortest_longest() {
    // Trace: no spec clause (POSIX XCU 2.6.2 remove-suffix semantics, shortest and longest, of the wordexp operators); D-14.5.
    use pat::*;
    let mut sh = Sh::sample();
    sh.vars.insert("V".into(), "abcabc".into());
    sh.vars.insert("S".into(), "a*b*".into());
    sh.vars.insert("E".into(), "x".into());
    let cases: [(&str, &str); 22] = [
        ("${V%c}", "abcab"),
        ("${V%b*}", "abca"),
        ("${V%%b*}", "a"),
        ("${V%?}", "abcab"),
        ("${V%%?}", "abcab"),
        ("${V%a*}", "abc"),
        ("${V%%a*}", ""),
        ("${V%[bc]}", "abcab"),
        ("${V%[ab]*}", "abca"),
        ("${V%%[ab]*}", ""),
        ("${V%*c}", "abcab"),
        ("${V%%*c}", ""),
        ("${V%z}", "abcabc"),
        ("${V%}", "abcabc"),
        ("${V%%}", "abcabc"),
        ("${V%abcabcabc}", "abcabc"),
        ("${V%%abcabcabc*}", "abcabc"),
        ("${V%abcabc}", ""),
        ("${S%\\*}", "a*b"),
        ("${S%%\\**}", "a"),
        ("${S%?}", "a*b"),
        ("${E%%*}", ""),
    ];
    for (words, want) in cases {
        let r = w_with(&mut sh, words, 0).unwrap();
        assert_eq!(r.first().map_or("", String::as_str), want, "{words}");
    }
}

/// WORDEXP-SUFFIX: the suffix-removal operators were cubic in the value's length (a fuzz input of
/// about 2 KB took 20 s; the cloud1 artifact `${1%$*}` shape). Patterns that fail or succeed late at
/// every suffix start must now cost a pass per pattern item.
#[test]
fn wordexp_suffix_removal_is_bounded() {
    // Trace: no spec clause (an implementation bound: suffix removal is not cubic in the value; WORDEXP-SUFFIX); D-14.5.
    use pat::*;
    for n in [2_000usize, 8_000] {
        let mut sh = Sh::sample();
        sh.vars.insert("V".into(), "a".repeat(n) + "b");
        let t = std::time::Instant::now();
        for (words, want) in [("${V%a*c}", n + 1), ("${V%%a*c}", n + 1), ("${V%*a?b}", n - 2), ("${V%%*a?b}", 0), ("${V%?*?*?*c}", n + 1), ("${V%%b}", n), ("${V%}", n + 1)] {
            let r = w_with(&mut sh, words, 0).unwrap();
            assert_eq!(r.first().map_or(0, String::len), want, "{words} n={n}");
        }
        crate::timeout::assert_within(&format!("suffix removal over {n} bytes"), std::time::Duration::from_secs(5), t.elapsed());
    }
}

/// WORDEXP-1: the fast paths of the expansion (one segment list for all the words, runs of plain
/// bytes as one piece, words of text alone added straight to the field, the field's pattern kept
/// only when it differs from its bytes, IFS runs decided by the byte set, binary operators by
/// precedence climbing) give the same words as the general path.
#[test]
fn wordexp1_fast_paths_agree() {
    // Trace: no spec clause (implementation fast paths of §14.10 wordexp; WORDEXP-1); D-14.5.
    use pat::*;
    // Words never share text: an empty quoted segment ending one word is not extended by the next.
    assert_eq!(w(r#""" \b"#), Ok(v(&["", "b"])));
    assert_eq!(w(r#"'' "b""#), Ok(v(&["", "b"])));
    assert_eq!(w(r#"a"" "b" 'c'd e\ f"#), Ok(v(&["a", "b", "cd", "e f"])));
    assert_eq!(w("abc=def ~x=~ a=b=c"), Ok(v(&["abc=def", "~x=~", "a=b=c"])));
    // Text alone, with and without active pattern characters.
    assert_eq!(w(r#""w"* w"*" '[w]'* "[x]".* "x".* \[w\]* w\\*"#), Ok(v(&["w1", "w2", "w*", "[w]*", "[x].*", "x.c", "[w]*", "w\\*"])));
    let mut sh = Sh::sample();
    for (k, val) in [("B", "w\\*"), ("Q", "w]"), ("S", "  x  y\tz  ")] {
        sh.vars.insert(k.into(), val.into());
    }
    assert_eq!(w_with(&mut sh, "$B '['$Q \"x\".$G $S \"$S\"", 0), Ok(v(&["w\\*", "[w]", "x.w*", "x", "y", "z", "  x  y\tz  "])));
    // IFS other than white space: empty fields between two of them; white space then trimmed.
    sh.vars.insert("IFS".into(), ": ".into());
    assert_eq!(w_with(&mut sh, "$C $S", 0), Ok(v(&["x", "y", "", "z", "x", "y\tz"])));
    sh.vars.insert("IFS".into(), String::new());
    assert_eq!(w_with(&mut sh, "$S", 0), Ok(v(&["  x  y\tz  "])));
    // Lengths counted without a list of character boundaries (bytes in the C locale).
    sh.vars.insert("E".into(), "\u{e9}t\u{e9}".into());
    assert_eq!(w_with(&mut sh, "${#P} ${#E} ${#Y} ${#UNSET}", 0), Ok(v(&["19", "5", "0", "0"])));
    // Arithmetic: precedence and associativity, short-circuiting.
    for (e, want) in [
        ("8-2-1", "5"),
        ("2+3*4", "14"),
        ("2*3+4", "10"),
        ("10-2*3-1", "3"),
        ("7%4*2", "6"),
        ("1<2==1", "1"),
        ("1||0&&0", "1"),
        ("0&&1/0", "0"),
        ("1||1/0", "1"),
        ("1|2^3&1", "3"),
        ("1<<2+1", "8"),
        ("-2*-3", "6"),
        ("1?2:3+4", "2"),
        ("(1+2)*(3+4)", "21"),
        ("N*N-N", "6"),
        ("8>>1>=4", "1"),
        ("3!=4", "1"),
        ("5<=5&&2>1", "1"),
    ] {
        assert_eq!(w(&format!("$(({e}))")), Ok(v(&[want])), "{e}");
    }
    assert!(w("$((1+))").is_err());
    assert!(w("$((1/0))").is_err());
    assert!(w("$((1**2))").is_err());
    assert!(w("$((1<<=2))").is_err());
}

#[test]
fn wordexp2_input_text_agrees() {
    // Trace: no spec clause (implementation fast paths of §14.10 wordexp; WORDEXP-2); D-14.5.
    // Word text taken from the input as it stands (no copy), joined only where it is contiguous
    // there; a word of such text alone goes straight to the result unless it may glob.
    use pat::*;
    // Pieces apart in the input (a quote, an escaped newline between them) are not one range.
    assert_eq!(w("a'b'c 'a''b' a\\\nb \"x\"y\"z\" a\"\"b"), Ok(v(&["abc", "ab", "ab", "xyz", "ab"])));
    // Backslashes inside double quotes: kept before an ordinary byte, removed before `$ \ "`.
    assert_eq!(w(r#""a\b" "a\$b" "a\\b" "a\"b" "\x\y""#), Ok(v(&["a\\b", "a$b", "a\\b", "a\"b", "\\x\\y"])));
    // `~` and `=` from the input: a tilde prefix only at the start or after an assignment's `=`.
    assert_eq!(w("a~b =a ~ a=~ x=y=~ a=~/q"), Ok(v(&["a~b", "=a", "/home/h", "a=/home/h", "x=y=~", "a=/home/h/q"])));
    // Quoted text from the input never globs; unquoted text from it does.
    assert_eq!(w(r#""w*" 'w?' w* w?"#), Ok(v(&["w*", "w?", "w1", "w2", "w1", "w2"])));
    // Words of text alone around expansions; IFS is read for the expansions.
    let mut sh = Sh::sample();
    sh.vars.insert("IFS".into(), ":".into());
    sh.vars.insert("C".into(), "p:q".into());
    assert_eq!(w_with(&mut sh, "a:b 'c d' $C e\\ f \"${C}\"", 0), Ok(v(&["a:b", "c d", "p", "q", "e f", "p:q"])));
    // Quoted text inside a brace word.
    assert_eq!(w(r#"${UNSET_X:-"q r"} ${UNSET_X:-'s t'}x"#), Ok(v(&["q r", "s tx"])));
}

#[test]
fn wordexp3_plain_segments_agree() {
    // Trace: no spec clause (implementation: segments as plain data, inline value buffers; WORDEXP-3); D-14.5.
    use pat::*;
    // Nested words side by side and inside each other (each stored when complete).
    assert_eq!(w("${U1:-a${U2:-b c}d} ${U3:-x}${U4:-$((1+2))}y"), Ok(v(&["ab", "cd", "x3y"])));
    // Default IFS: tab and newline are white space (runs collapse, leading and trailing are dropped).
    let mut sh = Sh::sample();
    sh.vars.insert("T".into(), "		a		b

c	".into());
    assert_eq!(w_with(&mut sh, "$T", 0), Ok(v(&["a", "b", "c"])));
    // A non-white-space IFS character makes empty fields.
    sh.vars.insert("IFS".into(), ":".into());
    sh.vars.insert("C".into(), "a::b".into());
    assert_eq!(w_with(&mut sh, "$C", 0), Ok(v(&["a", "", "b"])));
    // Command text from the input and from backquotes reaches the shell as written.
    let mut sh = Sh::sample();
    assert!(w_with(&mut sh, "$(echo a b) `echo c`", 0).is_ok());
    let ran: Vec<&str> = sh.ran.iter().map(|(c, _)| c.as_str()).collect();
    assert_eq!(ran, ["echo a b", "echo c"]);
}

/// C1-TRIAGE: a pattern that is a long run of unclosed `[` (no `]` anywhere) was rescanned to its end from
/// every `[` while compiling (quadratic), and matching it walked every string position for every item. The
/// wordexp fuzz targets timed out on such patterns (built from expanded values, compiled over and over).
#[test]
fn c1_unclosed_bracket_run_is_linear_to_compile() {
    // Trace: no spec clause (internal compile cost; fnmatch results unchanged); D-14.5.
    use pat::*;
    let loc = Locale::c_utf8(b"C.UTF-8");
    let compile = |n: usize| {
        let p = vec![b'['; n];
        let t = std::time::Instant::now();
        let pt = fm::Pattern::new(&p, 0, &loc).unwrap();
        (t.elapsed().as_secs_f64(), pt, p)
    };
    let (t1, _, _) = compile(20_000);
    let (t4, pt, p) = compile(80_000);
    // Linear is about 4x; quadratic is 16x. The floor keeps timer noise on a fast run from deciding.
    assert!(t4 < t1 * 9.0 + 0.02, "compile not linear: {t1:.4} s for 20000, {t4:.4} s for 80000");
    let t = std::time::Instant::now();
    assert!(pt.matches(&p, &loc).unwrap(), "unclosed `[` is a literal");
    assert!(!pt.matches(b"[", &loc).unwrap());
    crate::timeout::assert_within("match of an unclosed bracket run", std::time::Duration::from_secs(5), t.elapsed());
    // With a `]` at the end the run is one bracket expression and must still compile and match.
    let mut q = p.clone();
    q.push(b']');
    let pt = fm::Pattern::new(&q, 0, &loc).unwrap();
    assert!(pt.matches(b"[", &loc).unwrap());
    assert!(!pt.matches(b"a", &loc).unwrap());
}

/// `no spec clause` (a regression guard for the field buffer of PERF-WORDEXP). The
/// field buffer keeps a pattern apart from its bytes once quoted text needs an escape, and plain
/// text added after that must reach the pattern as well (PERF-WORDEXP).
#[test]
fn wordexp_field_pattern_after_escape() {
    // Trace: no spec clause (internal field buffer; behaviour is §14.10 c_4 quote removal and globbing); PERF-WORDEXP.
    use pat::*;
    let run = |files: Vec<&'static str>, words: &str| {
        let mut sh = Sh::sample();
        sh.files = files;
        w_with(&mut sh, words, 0)
    };
    // `?` is active, the quoted `*` is escaped, and the plain `x` after it is part of the pattern.
    assert_eq!(run(vec!["a*x", "ab"], "?\"*\"x"), Ok(v(&["a*x"])));
    // A quoted `]` inside an active bracket expression is escaped; the text around it is not.
    assert_eq!(run(vec!["]", "ab]"], "[a\"]\"b]"), Ok(v(&["]"])));
    // A lone `*` is one field per match, never dropped.
    assert_eq!(run(vec!["w1", "w2", ".w3"], "*"), Ok(v(&["w1", "w2"])));
    assert_eq!(run(vec![], "*"), Ok(v(&["*"])));
}
