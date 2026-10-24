//! Internal tests of `liella::search` (spec §14.1–14.6): stability, comparator robustness, the
//! AVL tree against a reference set, hash-table growth with stable entries, and queues.

#![allow(clippy::type_complexity, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::arithmetic_side_effects, missing_docs)]

use std::cmp::Ordering;
use std::collections::{BTreeSet, HashMap};

use liella::search::*;

struct Gen(u64);
impl Gen {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }
}

fn pairs(n: usize, g: &mut Gen) -> Vec<u8> {
    // Elements of 8 bytes: (key u32, original index u32).
    let mut v = Vec::new();
    for i in 0..n as u32 {
        v.extend_from_slice(&((g.next() % 16) as u32).to_le_bytes());
        v.extend_from_slice(&i.to_le_bytes());
    }
    v
}

fn key(e: &[u8]) -> u32 {
    u32::from_le_bytes(e[..4].try_into().unwrap())
}

#[test]
fn sorts_are_stable_on_both_paths() {
    // Clauses: s14_1_c_0, s14_1_g_1, s14_1_g_2, s14_1_g_3 (D-14.1).
    let mut g = Gen(7);
    for n in [0usize, 1, 2, 3, 7, 8, 9, 16, 100, 1000] {
        for in_place in [false, true] {
            let mut v = pairs(n, &mut g);
            let mut c = |a: &[u8], b: &[u8]| key(a).cmp(&key(b));
            if in_place {
                sort_stable_in_place(&mut v, 8, &mut c)
            } else {
                sort_stable(&mut v, 8, &mut c)
            }
            for w in v.chunks_exact(8).collect::<Vec<_>>().windows(2) {
                let (a, b) = (w[0], w[1]);
                let idx = |e: &[u8]| u32::from_le_bytes(e[4..].try_into().unwrap());
                assert!(key(a) < key(b) || (key(a) == key(b) && idx(a) < idx(b)), "n={n} in_place={in_place}");
            }
            let mut idx: Vec<u32> = v.chunks_exact(8).map(|e| u32::from_le_bytes(e[4..].try_into().unwrap())).collect();
            idx.sort();
            assert_eq!(idx, (0..n as u32).collect::<Vec<_>>(), "a permutation");
        }
    }
}

#[test]
fn inconsistent_comparators_keep_a_permutation() {
    // Clauses: s14_1_i_0, s14_1_g_1 (D-14.1).
    let mut g = Gen(9);
    let mut v = pairs(500, &mut g);
    let mut r = Gen(1);
    sort_stable(&mut v, 8, &mut |_, _| match r.next() % 3 {
        0 => Ordering::Less,
        1 => Ordering::Equal,
        _ => Ordering::Greater,
    });
    let mut idx: Vec<u32> = v.chunks_exact(8).map(|e| u32::from_le_bytes(e[4..].try_into().unwrap())).collect();
    idx.sort();
    assert_eq!(idx, (0..500).collect::<Vec<_>>());
}

#[test]
fn binary_and_linear_search() {
    // Clauses: s14_2_c_0, s14_3_c_0.
    let v: Vec<u8> = (0u32..100).flat_map(|i| (i * 2).to_le_bytes()).collect();
    let find = |k: u32| binary_search(&v, 4, &mut |e| k.cmp(&u32::from_le_bytes(e.try_into().unwrap())));
    assert_eq!(find(50), Some(25));
    assert_eq!(find(51), None);
    assert_eq!(find(0), Some(0));
    assert_eq!(find(198), Some(99));
    assert_eq!(linear_find(&v, 4, &mut |e| u32::from_le_bytes(e.try_into().unwrap()) == 8), Some(4));
    assert_eq!(binary_search(&[], 4, &mut |_| Ordering::Equal), None);
}

/// A tree whose nodes live in a map (addresses are map keys).
#[derive(Default)]
struct MapTree {
    root: usize,
    nodes: HashMap<usize, (usize, usize, usize, u32)>,
    next: usize,
    fail: bool,
}

impl TreeNodes for MapTree {
    fn root(&self) -> usize {
        self.root
    }
    fn set_root(&mut self, n: usize) {
        self.root = n
    }
    fn key(&self, n: usize) -> usize {
        self.nodes[&n].0
    }
    fn link(&self, n: usize, right: bool) -> usize {
        if right { self.nodes[&n].2 } else { self.nodes[&n].1 }
    }
    fn set_link(&mut self, n: usize, right: bool, c: usize) {
        let e = self.nodes.get_mut(&n).unwrap();
        if right { e.2 = c } else { e.1 = c }
    }
    fn height(&self, n: usize) -> u32 {
        self.nodes[&n].3
    }
    fn set_height(&mut self, n: usize, h: u32) {
        self.nodes.get_mut(&n).unwrap().3 = h
    }
    fn alloc(&mut self, key: usize) -> Option<usize> {
        if self.fail {
            return None;
        }
        self.next += 16;
        self.nodes.insert(self.next, (key, 0, 0, 1));
        Some(self.next)
    }
    fn free(&mut self, n: usize) {
        self.nodes.remove(&n).unwrap();
    }
}

fn check(t: &MapTree, n: usize, out: &mut Vec<usize>) -> u32 {
    if n == 0 {
        return 0;
    }
    let hl = check(t, t.link(n, false), out);
    out.push(t.key(n));
    let hr = check(t, t.link(n, true), out);
    assert!(hl.abs_diff(hr) <= 1, "AVL balance");
    assert_eq!(t.height(n), hl.max(hr) + 1, "height bookkeeping");
    hl.max(hr) + 1
}

#[test]
fn avl_tree_matches_a_reference_set() {
    // Clauses: s14_4_c_0, s14_4_c_1, s14_4_c_2, s14_4_c_3, s14_4_c_4, s14_4_g_0, s14_4_f_0 (D-14.2).
    let mut t = MapTree::default();
    let mut reference = BTreeSet::new();
    let mut g = Gen(11);
    let mut cmp = |a: usize, b: usize| a.cmp(&b);
    let mut node_of: HashMap<usize, usize> = HashMap::new();
    for _ in 0..20_000 {
        let k = (g.next() % 2000) as usize + 1;
        if g.next() % 3 == 0 {
            let r = tree_delete(&mut t, k, &mut cmp);
            assert_eq!(r != Deleted::Absent, reference.remove(&k));
            node_of.remove(&k);
        } else {
            let n = tree_search(&mut t, k, true, &mut cmp).unwrap().unwrap();
            if !reference.insert(k) {
                assert_eq!(node_of[&k], n, "an existing key returns its node");
            }
            node_of.insert(k, n);
        }
    }
    // Node identity survives other deletions (keys never move between nodes).
    for (&k, &n) in &node_of {
        assert_eq!(t.key(n), k);
        assert_eq!(tree_search(&mut t, k, false, &mut cmp).unwrap(), Some(n));
    }
    let mut keys = Vec::new();
    check(&t, t.root, &mut keys);
    assert_eq!(keys, reference.iter().copied().collect::<Vec<_>>());
    assert_eq!(t.nodes.len(), reference.len());
    // Walk order and kinds: in-order visits (postorder or leaf) give the sorted keys.
    let mut inorder = Vec::new();
    tree_walk(&t, t.root, 0, &mut |n, v, _| {
        if matches!(v, Visit::Postorder | Visit::Leaf) {
            inorder.push(t.key(n))
        }
    });
    assert_eq!(inorder, keys);
    // Allocation failure: Err, tree unchanged.
    t.fail = true;
    assert_eq!(tree_search(&mut t, 5000, true, &mut cmp), Err(()));
    t.fail = false;
    let mut freed = 0;
    let root = t.root;
    tree_destroy(&mut t, root, &mut |_| freed += 1);
    assert_eq!(freed, reference.len());
    assert!(t.nodes.is_empty());
}

#[derive(Default)]
struct MapHash {
    head: usize,
    segs: HashMap<usize, (usize, usize, Vec<(usize, usize)>)>,
    keys: HashMap<usize, Vec<u8>>,
    next: usize,
}

impl HashMem for MapHash {
    fn head(&self) -> usize {
        self.head
    }
    fn set_head(&mut self, s: usize) {
        self.head = s
    }
    fn next(&self, s: usize) -> usize {
        self.segs[&s].0
    }
    fn capacity(&self, s: usize) -> usize {
        self.segs[&s].2.len()
    }
    fn used(&self, s: usize) -> usize {
        self.segs[&s].1
    }
    fn set_used(&mut self, s: usize, n: usize) {
        self.segs.get_mut(&s).unwrap().1 = n
    }
    fn key(&self, s: usize, i: usize) -> usize {
        self.segs[&s].2[i].0
    }
    fn set_entry(&mut self, s: usize, i: usize, k: usize, d: usize) {
        self.segs.get_mut(&s).unwrap().2[i] = (k, d)
    }
    fn entry_addr(&self, s: usize, i: usize) -> usize {
        s * 100_000 + i
    }
    fn alloc_segment(&mut self, cap: usize, next: usize) -> Option<usize> {
        self.next += 1;
        self.segs.insert(self.next, (next, 0, vec![(0, 0); cap]));
        Some(self.next)
    }
    fn free_segment(&mut self, s: usize) {
        self.segs.remove(&s);
    }
    fn text(&self, k: usize) -> &[u8] {
        &self.keys[&k]
    }
}

#[test]
fn hash_table_grows_with_stable_entries() {
    // Clauses: s14_5_c_0, s14_5_i_0, s14_5_f_1, s14_5_f_2, s14_5_f_3 (D-14.3).
    let mut m = MapHash::default();
    let mut addr = HashMap::new();
    for i in 1..=5000usize {
        m.keys.insert(i, format!("key{i}").into_bytes());
        let a = hash_search(&mut m, i, i * 10, true).unwrap().unwrap();
        addr.insert(i, a);
    }
    for i in 1..=5000usize {
        assert_eq!(hash_search(&mut m, i, 0, false).unwrap(), Some(addr[&i]), "entries never move");
    }
    m.keys.insert(99_999, b"absent".to_vec());
    assert_eq!(hash_search(&mut m, 99_999, 0, false).unwrap(), None);
    // An equal text under a different key pointer finds the same entry.
    m.keys.insert(100_000, b"key42".to_vec());
    assert_eq!(hash_search(&mut m, 100_000, 0, true).unwrap(), Some(addr[&42]));
    hash_destroy(&mut m);
    assert!(m.segs.is_empty());
    assert_eq!(prime_at_least(100), 101);
}

#[derive(Default)]
struct MapLinks(HashMap<usize, (usize, usize)>);
impl Links for MapLinks {
    fn next(&self, e: usize) -> usize {
        self.0[&e].0
    }
    fn prev(&self, e: usize) -> usize {
        self.0[&e].1
    }
    fn set_next(&mut self, e: usize, v: usize) {
        self.0.entry(e).or_default().0 = v
    }
    fn set_prev(&mut self, e: usize, v: usize) {
        self.0.entry(e).or_default().1 = v
    }
}

#[test]
fn queues_link_and_unlink() {
    // Clauses: s14_6_c_0.
    let mut l = MapLinks::default();
    queue_insert(&mut l, 1, 0);
    queue_insert(&mut l, 2, 1);
    queue_insert(&mut l, 3, 1);
    assert_eq!((l.next(1), l.next(3), l.next(2)), (3, 2, 0));
    assert_eq!((l.prev(2), l.prev(3), l.prev(1)), (3, 1, 0));
    queue_remove(&mut l, 3);
    assert_eq!((l.next(1), l.prev(2)), (2, 1));
}
