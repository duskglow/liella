//! Sorting, searching, ordered sets, hash tables and queues (spec §14.1–14.6; D-14.1–D-14.3).
//!
//! Arrays are byte slices of fixed-size elements compared by a caller function. Trees, hash
//! tables and queues live in memory the C caller observes (node pointers are returned to it),
//! so their algorithms are written over small storage traits; the boundary implements the
//! traits over C memory and this module stays memory-safe and allocation-free.
//!
//! The design decisions of this module are recorded in D-14.18.
#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)] // element indices are bounded by the slice's element count; path depth by MAX_DEPTH
#![allow(clippy::as_conversions)] // the hash's 64x64 -> 128-bit range reduction

use core::cmp::Ordering;

use crate::mem::{self, TryVec};

// ------------------------------------------------------------------------------ sorting

/// Words of index space kept on the stack: sorts whose indices fit never allocate.
const STACK_WORDS: usize = 128;

/// Largest piece of an element (bytes) moved through a stack temporary; bigger elements are
/// rotated in place by insertion and moved in pieces by the final permutation.
const TEMP: usize = 64;

/// Elements an index sort places by insertion before merging starts.
const INDEX_RUN: usize = 8;

/// One comparison of two elements of the array being sorted, named by their byte offsets
/// (QSORT-COMPARE-TRAIT). The boundary implements it for the C comparators of `qsort` and
/// `qsort_r`: it forms the element pointers from `base` and the offsets without slicing, which
/// spares the merge loop two bounds checks per comparison.
///
/// Contract (an implementation may rely on it for memory safety): every sort in this module
/// passes as `base` a slice of whole elements of the array (`base.len()` a multiple of the
/// element size), and as `a` and `b` multiples of the element size below `base.len()`, so each
/// names a whole element of `base`. This holds whatever `gt` answers: the offsets are computed
/// from element indices below the element count, or are taken from a table of positions that
/// starts as `0, size, 2 * size, ...` and is only ever permuted (every merge, insertion,
/// reversal and rotation step writes back a position it read from the table), so an
/// inconsistent comparator changes the order but never an offset.
pub trait SortCompare {
    /// Whether the element at byte offset `a` of `base` goes after the element at offset `b`
    /// (it compares greater).
    fn gt(&mut self, base: &[u8], a: usize, b: usize) -> bool;
}

impl<C: SortCompare + ?Sized> SortCompare for &mut C {
    #[inline]
    fn gt(&mut self, base: &[u8], a: usize, b: usize) -> bool {
        (**self).gt(base, a, b)
    }
}

/// A comparator over element slices as a [`SortCompare`] (checked slicing; the safe callers).
struct BySlices<'a, F: ?Sized> {
    size: usize,
    cmp: &'a mut F,
}

impl<F: FnMut(&[u8], &[u8]) -> Ordering + ?Sized> SortCompare for BySlices<'_, F> {
    #[inline]
    fn gt(&mut self, base: &[u8], a: usize, b: usize) -> bool {
        (self.cmp)(&base[a..a + self.size], &base[b..b + self.size]) == Ordering::Greater
    }
}

/// Stable sort of `n = base.len() / size` elements (D-14.1, D-14.18): a merge sort of the
/// elements' positions that compares the elements where they lie, then one pass that moves
/// every element to its place (QSORT-C-COMPARATOR). The comparator therefore only ever sees
/// elements of `base`, and `base` is unchanged until the last comparison has returned. Without
/// memory for the positions, an in-place stable merge (O(n log² n)). Never fails, and stays
/// within `base` whatever the comparator returns.
pub fn sort_stable<F: FnMut(&[u8], &[u8]) -> Ordering + ?Sized>(base: &mut [u8], size: usize, cmp: &mut F) {
    sort_stable_tracked(base, size, BySlices { size, cmp }, &mut SortScratch::default());
}

/// Where a [`sort_stable_tracked`] stands, for a caller that must recover when a comparator
/// never returns (a C++ exception, OD-6 revised): the heap buffer of positions. The array needs
/// no repair: it is only written after the last comparison.
#[derive(Default)]
pub struct SortScratch {
    /// The positions and merge buffer, when they are on the heap.
    pub heap: Option<TryVec<usize>>,
}

/// [`sort_stable`] comparing through `cmp` (see [`SortCompare`] for the offsets it is given),
/// with its state in `scratch` (the heap buffer stays there). `cmp` is taken by value so the
/// merge loop holds its fields itself rather than behind a reference.
pub fn sort_stable_tracked<C: SortCompare>(base: &mut [u8], size: usize, mut cmp: C, scratch: &mut SortScratch) {
    if size == 0 || base.len() / size < 2 {
        return;
    }
    let n = base.len() / size;
    let base = &mut base[..n * size];
    if n <= run_len(size) {
        insertion_sort(base, size, &mut cmp);
        return;
    }
    // The positions, then room for the left half of the biggest merge. n <= isize::MAX, so
    // n + n / 2 fits.
    let words = n + n / 2;
    if words <= STACK_WORDS {
        let mut buf = [0usize; STACK_WORDS];
        sort_by_index(base, size, cmp, &mut buf[..words]);
    } else {
        scratch.heap = scratch_buf(words);
        match &mut scratch.heap {
            Some(buf) => sort_by_index(base, size, cmp, buf.as_mut_slice()),
            None => merge_sort(base, size, run_len(size), &mut cmp),
        }
    }
}

/// A zeroed heap buffer of `len` words, or `None` when memory is short.
fn scratch_buf(len: usize) -> Option<TryVec<usize>> {
    let mut v = TryVec::<usize>::with_capacity(len).ok()?;
    v.resize(len, 0).ok()?;
    Some(v)
}

/// The allocation-free path of [`sort_stable`] (also used directly when no buffer is wanted).
pub fn sort_stable_in_place<F: FnMut(&[u8], &[u8]) -> Ordering + ?Sized>(base: &mut [u8], size: usize, cmp: &mut F) {
    if size == 0 || base.len() / size < 2 {
        return;
    }
    let n = base.len() / size;
    merge_sort(&mut base[..n * size], size, run_len(size), &mut BySlices { size, cmp });
}

/// Elements sorted by insertion before merging starts in place. Binary insertion costs about
/// as many comparisons as merging does for such short runs and moves fewer, shorter distances;
/// moves get dearer with the element size, so big elements get shorter runs.
fn run_len(size: usize) -> usize {
    if size <= 64 { 8 } else { 4 }
}

/// Element `i` of `base`.
fn elem(base: &[u8], size: usize, i: usize) -> &[u8] {
    &base[i * size..(i + 1) * size]
}

/// Sort `base` through `buf` (`n` positions, then `n / 2` words of merge buffer): moving a
/// position costs one word whatever the element size, and the elements move once at the end.
/// A position is the element's byte offset in `base`, so a comparison costs no multiply.
fn sort_by_index<C: SortCompare>(base: &mut [u8], size: usize, mut cmp: C, buf: &mut [usize]) {
    let n = base.len() / size;
    let (pos, aux) = buf.split_at_mut(n);
    let mut off = 0;
    for slot in pos.iter_mut() {
        *slot = off;
        off += size;
    }
    {
        // Every position is a multiple of `size` below `n * size == elems.len()` (the
        // `SortCompare` contract): `merge_sort_index` only permutes them.
        let elems: &[u8] = base;
        let mut gt = move |a: usize, b: usize| cmp.gt(elems, a, b);
        merge_sort_index(pos, aux, &mut gt);
    }
    permute(base, size, pos);
}

/// Top-down merge sort of positions; `gt(a, b)` says element `a` goes after element `b`.
/// Recursing on halves keeps each merge inside the cache that the sorts of its halves warmed.
/// Every step writes a position still to be placed, so `idx` stays a permutation whatever `gt`
/// answers. Returns whether the range was strictly descending (each element greater than the
/// next); it is then sorted by reversal, which is stable because no two of its elements are
/// equal, and costs one comparison per element (QSORT-MERGE-INDEX).
fn merge_sort_index<G: FnMut(usize, usize) -> bool>(idx: &mut [usize], aux: &mut [usize], gt: &mut G) -> bool {
    let n = idx.len();
    if n <= INDEX_RUN {
        return insertion_index(idx, gt);
    }
    let mid = n / 2;
    let both_down = {
        let (left, right) = idx.split_at_mut(mid);
        let down = merge_sort_index(left, aux, gt);
        merge_sort_index(right, aux, gt) && down
    };
    // Two descending halves, reversed: the range descends when the left half's last element (now
    // its first) is greater than the right half's first (now its last). Random input almost
    // never gets here, so it pays nothing for the check.
    if both_down && gt(idx[0], idx[n - 1]) {
        idx.rotate_left(mid);
        return true;
    }
    // One comparison settles two runs that are already in order.
    if !gt(idx[mid - 1], idx[mid]) {
        return false;
    }
    // The left run moves out; the output fills `idx` from the front and never passes the read
    // position of the right run (k = i + j - mid < j). (Stepping through cell slices of `idx`
    // instead measured 7% more instructions, QSORT-MERGE-INDEX.)
    let left = &mut aux[..mid];
    left.copy_from_slice(&idx[..mid]);
    let (mut i, mut j, mut k) = (0, mid, 0);
    while i < mid && j < n {
        let (a, b) = (left[i], idx[j]);
        let right_first = gt(a, b);
        idx[k] = if right_first { b } else { a };
        j += usize::from(right_first);
        i += usize::from(!right_first);
        k += 1;
    }
    // A left remainder fills the gap before the rest of the right run, which is in place.
    idx[k..k + (mid - i)].copy_from_slice(&left[i..]);
    false
}

/// Stable binary insertion of positions (a short run). A strictly descending prefix is found
/// first and reversed (one comparison per element; it is the whole run for descending input).
/// The rest is inserted: first against the predecessor (ordered input costs one comparison per
/// element), then after every element not greater. Returns whether the whole run descended.
fn insertion_index<G: FnMut(usize, usize) -> bool>(idx: &mut [usize], gt: &mut G) -> bool {
    let n = idx.len();
    let mut d = 1;
    while d < n && gt(idx[d - 1], idx[d]) {
        d += 1;
    }
    if d >= n {
        idx.reverse();
        return true;
    }
    idx[..d].reverse();
    // The comparison that ended the prefix found idx[d] not before the prefix's last element,
    // its least: with d == 1 that is insertion's own first comparison; with d > 1 that element
    // is now idx[0], so idx[d] goes after it.
    let first = if d == 1 { 2 } else { d };
    for i in first..n {
        let x = idx[i];
        if !gt(idx[i - 1], x) {
            continue;
        }
        let (mut lo, mut hi) = (usize::from(i == d), i - 1);
        while lo < hi {
            let m = lo + (hi - lo) / 2;
            if gt(idx[m], x) {
                hi = m;
            } else {
                lo = m + 1;
            }
        }
        let mut j = i;
        while j > lo {
            idx[j] = idx[j - 1];
            j -= 1;
        }
        idx[lo] = x;
    }
    false
}

/// Move the elements to their places: place `k` (offset `k * size`) takes the element now at
/// offset `pos[k]`. Each cycle of the permutation is walked once per piece of at most [`TEMP`]
/// bytes, the piece at its start held in a stack temporary; the last walk marks its places done
/// (`pos[k] = k * size`).
fn permute(base: &mut [u8], size: usize, pos: &mut [usize]) {
    let mut tmp = [0u8; TEMP];
    // Offset to place: a shift for the common power-of-two sizes, else one division per element.
    let shift = if size.is_power_of_two() { Some(size.trailing_zeros()) } else { None };
    let place = |o: usize| match shift {
        Some(s) => o >> s,
        None => o / size,
    };
    for start in 0..pos.len() {
        let so = start * size;
        if pos[start] == so {
            continue;
        }
        let mut c = 0;
        while c < size {
            let len = (size - c).min(TEMP);
            let last = c + len == size;
            mem::copy(&mut tmp[..len], &base[so + c..so + c + len]);
            let (mut j, mut jo) = (start, so);
            loop {
                let from = pos[j];
                if last {
                    pos[j] = jo;
                }
                if from == so {
                    mem::copy(&mut base[jo + c..jo + c + len], &tmp[..len]);
                    break;
                }
                copy_piece(base, from + c, jo + c, len);
                (j, jo) = (place(from), from);
            }
            c += len;
        }
    }
}

/// Copy `len` bytes at offset `from` to offset `to` (pieces of two different elements, so they
/// do not overlap).
fn copy_piece(base: &mut [u8], from: usize, to: usize, len: usize) {
    if from < to {
        let (a, b) = base.split_at_mut(to);
        mem::copy(&mut b[..len], &a[from..from + len]);
    } else {
        let (a, b) = base.split_at_mut(from);
        mem::copy(&mut a[to..to + len], &b[..len]);
    }
}

/// Top-down merge sort in place (no memory for positions). Merges by rotation.
fn merge_sort<C: SortCompare + ?Sized>(base: &mut [u8], size: usize, run: usize, cmp: &mut C) {
    let n = base.len() / size;
    if n <= run {
        insertion_sort(base, size, cmp);
        return;
    }
    let mid = n / 2;
    {
        let (left, right) = base.split_at_mut(mid * size);
        merge_sort(left, size, run, cmp);
        merge_sort(right, size, run, cmp);
    }
    // One comparison settles two runs that are already in order.
    if !cmp.gt(base, (mid - 1) * size, mid * size) {
        return;
    }
    merge_rotating(base, size, 0, mid, n, cmp);
}

/// Stable binary insertion sort of `base` (a short run). An element is first compared with its
/// predecessor (ordered input costs one comparison per element), then placed after every
/// element not greater than it, which keeps the sort stable.
fn insertion_sort<C: SortCompare + ?Sized>(base: &mut [u8], size: usize, cmp: &mut C) {
    let n = base.len() / size;
    let mut tmp = [0u8; TEMP];
    for i in 1..n {
        if !cmp.gt(base, (i - 1) * size, i * size) {
            continue;
        }
        // elem(i) < elem(i - 1): the slot is in [0, i - 1].
        let (mut lo, mut hi) = (0, i - 1);
        while lo < hi {
            let m = lo + (hi - lo) / 2;
            if cmp.gt(base, m * size, i * size) {
                hi = m;
            } else {
                lo = m + 1;
            }
        }
        if size <= TEMP {
            mem::copy(&mut tmp[..size], elem(base, size, i));
            base.copy_within(lo * size..i * size, (lo + 1) * size);
            mem::copy(&mut base[lo * size..(lo + 1) * size], &tmp[..size]);
        } else {
            base[lo * size..(i + 1) * size].rotate_right(size);
        }
    }
}

/// Stable merge of `[lo, mid)` and `[mid, hi)` without a buffer: split the longer run at its
/// middle, find where that element belongs in the other run, rotate the two inner pieces past
/// each other, and merge the two halves that result.
fn merge_rotating<C: SortCompare + ?Sized>(base: &mut [u8], size: usize, lo: usize, mid: usize, hi: usize, cmp: &mut C) {
    if lo == mid || mid == hi {
        return;
    }
    if hi - lo == 2 {
        if cmp.gt(base, lo * size, (lo + 1) * size) {
            let (a, b) = base[lo * size..(lo + 2) * size].split_at_mut(size);
            a.swap_with_slice(b);
        }
        return;
    }
    let (cut1, cut2);
    if mid - lo >= hi - mid {
        cut1 = lo + (mid - lo) / 2;
        // Right-run elements strictly below the pivot go before it (equal ones stay after).
        let (mut a, mut b) = (mid, hi);
        while a < b {
            let m = a + (b - a) / 2;
            if cmp.gt(base, cut1 * size, m * size) {
                a = m + 1;
            } else {
                b = m;
            }
        }
        cut2 = a;
    } else {
        cut2 = mid + (hi - mid) / 2;
        // Left-run elements not above the pivot stay before it.
        let (mut a, mut b) = (lo, mid);
        while a < b {
            let m = a + (b - a) / 2;
            if cmp.gt(base, m * size, cut2 * size) {
                b = m;
            } else {
                a = m + 1;
            }
        }
        cut1 = a;
    }
    base[cut1 * size..cut2 * size].rotate_left((mid - cut1) * size);
    let new_mid = cut1 + (cut2 - mid);
    merge_rotating(base, size, lo, cut1, new_mid, cmp);
    merge_rotating(base, size, new_mid, cut2, hi, cmp);
}

/// `bsearch`: the index of an element comparing equal to the key (`cmp(element)` returns the
/// key's order relative to the element), or `None`.
pub fn binary_search(base: &[u8], size: usize, cmp: &mut dyn FnMut(&[u8]) -> Ordering) -> Option<usize> {
    if size == 0 {
        return None;
    }
    let (mut lo, mut n) = (0, base.len() / size);
    while n > 0 {
        let half = n / 2;
        let m = lo + half;
        match cmp(elem(base, size, m)) {
            Ordering::Equal => return Some(m),
            Ordering::Less => n = half,
            Ordering::Greater => {
                lo = m + 1;
                n -= half + 1;
            }
        }
    }
    None
}

/// `lfind`: the index of the first element equal to the key (`eq`), or `None`.
pub fn linear_find(base: &[u8], size: usize, eq: &mut dyn FnMut(&[u8]) -> bool) -> Option<usize> {
    if size == 0 {
        return None;
    }
    base.chunks_exact(size).position(&mut *eq)
}

// ------------------------------------------------------------------------------ ordered set

/// Node storage of a binary search tree whose nodes live in caller-visible memory. Nodes are
/// nonzero addresses; 0 is "none". Each node holds a key (a caller pointer), two links and a
/// height (Liella's node layout `{key, left, right, height}`; only the leading key member is
/// ABI, D-14.2).
pub trait TreeNodes {
    /// The root (0 when empty).
    fn root(&self) -> usize;
    /// Set the root.
    fn set_root(&mut self, n: usize);
    /// The node's key.
    fn key(&self, n: usize) -> usize;
    /// A child link.
    fn link(&self, n: usize, right: bool) -> usize;
    /// Set a child link.
    fn set_link(&mut self, n: usize, right: bool, child: usize);
    /// The node's height (1 for a leaf).
    fn height(&self, n: usize) -> u32;
    /// Set the node's height.
    fn set_height(&mut self, n: usize, h: u32);
    /// A new node with `key`, no children, height 1; `None` if memory is exhausted.
    fn alloc(&mut self, key: usize) -> Option<usize>;
    /// Release a node.
    fn free(&mut self, n: usize);
}

/// Height of the subtree at `n` (0 for none).
fn height_of<T: TreeNodes + ?Sized>(t: &T, n: usize) -> u32 {
    if n == 0 { 0 } else { t.height(n) }
}

/// Rotate `n` down to the right (`right`) or left; its child on the other side takes its
/// place. Returns the new subtree root, with both heights recomputed.
fn rotate<T: TreeNodes + ?Sized>(t: &mut T, n: usize, right: bool) -> usize {
    let pivot = t.link(n, !right);
    let inner = t.link(pivot, right);
    t.set_link(n, !right, inner);
    t.set_link(pivot, right, n);
    for x in [n, pivot] {
        let h = 1 + height_of(t, t.link(x, false)).max(height_of(t, t.link(x, true)));
        t.set_height(x, h);
    }
    pivot
}

/// Restore the AVL condition at `n`, whose subtrees are valid, and recompute its height.
/// Returns the root of the repaired subtree (`n` itself unless a rotation was needed).
fn repair<T: TreeNodes + ?Sized>(t: &mut T, n: usize) -> usize {
    let (l, r) = (t.link(n, false), t.link(n, true));
    let (hl, hr) = (height_of(t, l), height_of(t, r));
    if hl > hr + 1 || hr > hl + 1 {
        let heavy_right = hr > hl;
        let child = if heavy_right { r } else { l };
        // An inner-heavy child needs the opposite rotation first (the double rotation).
        if height_of(t, t.link(child, !heavy_right)) > height_of(t, t.link(child, heavy_right)) {
            let c = rotate(t, child, heavy_right);
            t.set_link(n, heavy_right, c);
        }
        return rotate(t, n, !heavy_right);
    }
    t.set_height(n, 1 + hl.max(hr));
    n
}

/// Depth bound of the path stacks: an AVL tree of height 96 would hold more than 2^64 nodes.
const MAX_DEPTH: usize = 96;

/// Repair every ancestor in `path` (each the node and the side its path continued on), deepest
/// first, re-linking a rotated subtree into its parent, and stop at the first whose height is
/// the same as before: nothing above it can have changed.
fn retrace<T: TreeNodes + ?Sized>(t: &mut T, path: &[(usize, bool)]) {
    for i in (0..path.len()).rev() {
        let n = path[i].0;
        let before = t.height(n);
        let top = repair(t, n);
        if top != n {
            match i.checked_sub(1) {
                Some(p) => t.set_link(path[p].0, path[p].1, top),
                None => t.set_root(top),
            }
        }
        if t.height(top) == before {
            return;
        }
    }
}

/// `tsearch`/`tfind`: find the node whose key compares equal (`cmp(node_key)` gives the search
/// key's order relative to it); with `insert`, add a node when absent. Returns `Ok(Some(node))`,
/// `Ok(None)` when absent and not inserting, `Err(())` when the node cannot be allocated.
#[allow(clippy::result_unit_err)]
pub fn tree_search<T: TreeNodes + ?Sized>(t: &mut T, key: usize, insert: bool, cmp: &mut dyn FnMut(usize, usize) -> Ordering) -> Result<Option<usize>, ()> {
    let mut n = t.root();
    if !insert {
        while n != 0 {
            match cmp(key, t.key(n)) {
                Ordering::Equal => return Ok(Some(n)),
                o => n = t.link(n, o == Ordering::Greater),
            }
        }
        return Ok(None);
    }
    let mut path = [(0usize, false); MAX_DEPTH];
    let mut depth = 0;
    while n != 0 {
        match cmp(key, t.key(n)) {
            Ordering::Equal => return Ok(Some(n)),
            o => {
                if depth == MAX_DEPTH {
                    return Err(());
                }
                let right = o == Ordering::Greater;
                path[depth] = (n, right);
                depth += 1;
                n = t.link(n, right);
            }
        }
    }
    let node = t.alloc(key).ok_or(())?;
    match depth.checked_sub(1) {
        None => t.set_root(node),
        Some(last) => {
            t.set_link(path[last].0, path[last].1, node);
            retrace(t, &path[..depth]);
        }
    }
    Ok(Some(node))
}

/// What `tdelete` removed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Deleted {
    /// The key was not in the tree.
    Absent,
    /// Removed; the removed node's parent before rebalancing.
    Parent(usize),
    /// Removed the root; the tree still has this new root.
    NewRoot(usize),
    /// Removed the last node.
    Empty,
}

/// `tdelete`: remove the node whose key compares equal, relinking nodes (never moving keys
/// between nodes, so node pointers the caller holds stay valid) and freeing it.
pub fn tree_delete<T: TreeNodes + ?Sized>(t: &mut T, key: usize, cmp: &mut dyn FnMut(usize, usize) -> Ordering) -> Deleted {
    let mut path = [(0usize, false); MAX_DEPTH + 1];
    let mut depth = 0;
    let mut target = t.root();
    while target != 0 {
        match cmp(key, t.key(target)) {
            Ordering::Equal => break,
            o => {
                if depth == MAX_DEPTH {
                    return Deleted::Absent;
                }
                let right = o == Ordering::Greater;
                path[depth] = (target, right);
                depth += 1;
                target = t.link(target, right);
            }
        }
    }
    if target == 0 {
        return Deleted::Absent;
    }
    let parent = if depth == 0 { 0 } else { path[depth - 1].0 };
    let (l, r) = (t.link(target, false), t.link(target, true));
    if l == 0 || r == 0 {
        // At most one child: it takes the node's place.
        let child = l.max(r);
        match depth.checked_sub(1) {
            Some(p) => t.set_link(path[p].0, path[p].1, child),
            None => t.set_root(child),
        }
    } else {
        // Two children: the nearest key of the taller side takes the node's place, which
        // shortens the taller side and so needs the fewest repairs.
        let from_left = height_of(t, l) > height_of(t, r);
        let slot = depth;
        path[slot] = (target, !from_left);
        depth += 1;
        let mut m = if from_left { l } else { r };
        // Walk to the extreme of that side: right-most of the left, left-most of the right.
        while t.link(m, from_left) != 0 && depth < MAX_DEPTH {
            path[depth] = (m, from_left);
            depth += 1;
            m = t.link(m, from_left);
        }
        let keep = t.link(m, !from_left); // m's only possible child
        if depth != slot + 1 {
            // Deeper down: m's parent adopts m's child, and m adopts the node's near side.
            t.set_link(path[depth - 1].0, from_left, keep);
            t.set_link(m, !from_left, t.link(target, !from_left));
        } // else m is the node's direct child and keeps its own subtree on that side
        t.set_link(m, from_left, t.link(target, from_left));
        t.set_height(m, t.height(target));
        path[slot].0 = m;
        match slot.checked_sub(1) {
            Some(p) => t.set_link(path[p].0, path[p].1, m),
            None => t.set_root(m),
        }
    }
    t.free(target);
    retrace(t, &path[..depth]);
    if parent != 0 {
        Deleted::Parent(parent)
    } else if t.root() == 0 {
        Deleted::Empty
    } else {
        Deleted::NewRoot(t.root())
    }
}

/// The SVID visit kinds of `twalk` (their C values).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Visit {
    /// Before the children (`preorder`, 0).
    Preorder = 0,
    /// Between the children: in-order (`postorder`, 1; ⚑ HIST naming).
    Postorder = 1,
    /// After the children (`endorder`, 2).
    Endorder = 2,
    /// A node without children, visited once (`leaf`, 3).
    Leaf = 3,
}

/// `twalk`: visit every node with its depth.
pub fn tree_walk<T: TreeNodes + ?Sized>(t: &T, node: usize, depth: u32, visit: &mut dyn FnMut(usize, Visit, u32)) {
    if node == 0 {
        return;
    }
    let (l, r) = (t.link(node, false), t.link(node, true));
    if l == 0 && r == 0 {
        visit(node, Visit::Leaf, depth);
        return;
    }
    visit(node, Visit::Preorder, depth);
    tree_walk(t, l, depth + 1, visit);
    visit(node, Visit::Postorder, depth);
    tree_walk(t, r, depth + 1, visit);
    visit(node, Visit::Endorder, depth);
}

/// `tdestroy`: free every node, calling `free_key` on each key (children before parents).
pub fn tree_destroy<T: TreeNodes + ?Sized>(t: &mut T, node: usize, free_key: &mut dyn FnMut(usize)) {
    if node == 0 {
        return;
    }
    let (l, r) = (t.link(node, false), t.link(node, true));
    tree_destroy(t, l, free_key);
    tree_destroy(t, r, free_key);
    free_key(t.key(node));
    t.free(node);
}

// ------------------------------------------------------------------------------ hash table

/// Storage of a growable hash table of `(key text, data)` entries with stable entry addresses
/// (D-14.3). The table is a chain of segments; each segment is an open-addressed array.
pub trait HashMem {
    /// The newest segment (0 if none).
    fn head(&self) -> usize;
    /// Set the newest segment.
    fn set_head(&mut self, seg: usize);
    /// The segment created before `seg` (0 if none).
    fn next(&self, seg: usize) -> usize;
    /// Entries in `seg`.
    fn capacity(&self, seg: usize) -> usize;
    /// Entries used in `seg`.
    fn used(&self, seg: usize) -> usize;
    /// Set the used count of `seg`.
    fn set_used(&mut self, seg: usize, n: usize);
    /// Entry `i`'s key pointer (0 = empty slot).
    fn key(&self, seg: usize, i: usize) -> usize;
    /// Store entry `i`.
    fn set_entry(&mut self, seg: usize, i: usize, key: usize, data: usize);
    /// The address of entry `i` (what `hsearch` returns).
    fn entry_addr(&self, seg: usize, i: usize) -> usize;
    /// A zeroed segment with `capacity` entries linked to `next`; `None` without memory.
    fn alloc_segment(&mut self, capacity: usize, next: usize) -> Option<usize>;
    /// Release a segment.
    fn free_segment(&mut self, seg: usize);
    /// The text of a key pointer.
    fn text(&self, key: usize) -> &[u8];
}

/// A 64-bit hash of the key text: eight bytes per multiply-fold step, the length mixed into the
/// seed so texts differing only by trailing zero bytes still differ, and a final avalanche
/// because the slot index is taken from the high bits.
fn hash_text(s: &[u8]) -> u64 {
    const K1: u64 = 0x9e37_79b9_7f4a_7c15;
    const K2: u64 = 0xff51_afd7_ed55_8ccd;
    let mut h = K1 ^ (s.len() as u64).wrapping_mul(K2);
    let mut words = s.chunks_exact(8);
    for w in &mut words {
        let mut b = [0u8; 8];
        mem::copy(&mut b, w);
        h = (h ^ u64::from_le_bytes(b)).wrapping_mul(K2);
        h ^= h >> 32;
    }
    let tail = words.remainder();
    if !tail.is_empty() {
        let mut b = [0u8; 8];
        mem::copy(&mut b[..tail.len()], tail);
        h = (h ^ u64::from_le_bytes(b)).wrapping_mul(K2);
        h ^= h >> 32;
    }
    h = (h ^ (h >> 29)).wrapping_mul(K1);
    h ^ (h >> 32)
}

/// The home slot of hash `h` in a segment of `cap` slots (multiply-shift range reduction: any
/// capacity works, no division).
fn home(h: u64, cap: usize) -> usize {
    ((u128::from(h) * cap as u128) >> 64) as usize
}

/// The smallest prime at or above `n` (at least 3), used as the first segment's capacity.
pub fn prime_at_least(n: usize) -> usize {
    let mut c = n.max(3) | 1;
    loop {
        let mut d = 3;
        let mut prime = true;
        while d <= c / d {
            if c % d == 0 {
                prime = false;
                break;
            }
            d += 2;
        }
        if prime {
            return c;
        }
        c += 2;
    }
}

/// The slot holding `text` in `seg`, if it is there. Linear probing stops at the first empty
/// slot (entries are never removed) or after one lap of a full segment.
fn find_in<H: HashMem + ?Sized>(m: &H, seg: usize, text: &[u8], h: u64) -> Option<usize> {
    let cap = m.capacity(seg);
    let mut i = home(h, cap);
    for _ in 0..cap {
        let k = m.key(seg, i);
        if k == 0 {
            return None;
        }
        if m.text(k) == text {
            return Some(i);
        }
        i += 1;
        if i == cap {
            i = 0;
        }
    }
    None
}

/// `hsearch`: find the entry for `key`; with `enter`, insert `(key, data)` when absent,
/// growing by a new, twice as large segment when the newest is three-quarters full (older
/// segments stay, so entry addresses never change). Returns the entry address, `Ok(None)` when
/// absent and not entering, `Err(())` when memory is exhausted.
#[allow(clippy::result_unit_err)]
pub fn hash_search<H: HashMem + ?Sized>(m: &mut H, key: usize, data: usize, enter: bool) -> Result<Option<usize>, ()> {
    let mut head = m.head();
    if head == 0 {
        // No table yet: a lookup finds nothing; an entry creates the first segment.
        if !enter {
            return Ok(None);
        }
        head = m.alloc_segment(prime_at_least(16), 0).ok_or(())?;
        m.set_head(head);
    }
    let h = {
        let text = m.text(key);
        let h = hash_text(text);
        // Newest first: it holds most of the entries.
        let mut seg = head;
        while seg != 0 {
            if let Some(i) = find_in(m, seg, text, h) {
                return Ok(Some(m.entry_addr(seg, i)));
            }
            seg = m.next(seg);
        }
        h
    };
    if !enter {
        return Ok(None);
    }
    let cap = m.capacity(head);
    if (m.used(head) + 1) * 4 > cap * 3 {
        match m.alloc_segment(cap.saturating_mul(2).saturating_add(1), head) {
            Some(seg) => {
                m.set_head(seg);
                head = seg;
            }
            // No memory to grow: still usable while a slot is free.
            None if m.used(head) >= cap => return Err(()),
            None => {}
        }
    }
    let cap = m.capacity(head);
    let mut i = home(h, cap);
    while m.key(head, i) != 0 {
        i += 1;
        if i == cap {
            i = 0;
        }
    }
    m.set_entry(head, i, key, data);
    m.set_used(head, m.used(head) + 1);
    Ok(Some(m.entry_addr(head, i)))
}

/// `hdestroy`: free every segment (keys are not freed, per the contract).
pub fn hash_destroy<H: HashMem + ?Sized>(m: &mut H) {
    let mut seg = m.head();
    m.set_head(0);
    while seg != 0 {
        let next = m.next(seg);
        m.free_segment(seg);
        seg = next;
    }
}

// ------------------------------------------------------------------------------ queues

/// Elements of a caller-managed doubly linked list (`insque`/`remque`): addresses whose first
/// two members are the forward and backward pointers.
pub trait Links {
    /// The forward pointer.
    fn next(&self, e: usize) -> usize;
    /// The backward pointer.
    fn prev(&self, e: usize) -> usize;
    /// Set the forward pointer.
    fn set_next(&mut self, e: usize, v: usize);
    /// Set the backward pointer.
    fn set_prev(&mut self, e: usize, v: usize);
}

/// `insque(elem, pred)`: insert after `pred`, or start a new linear list when `pred` is 0.
pub fn queue_insert<L: Links + ?Sized>(l: &mut L, elem: usize, pred: usize) {
    if pred == 0 {
        l.set_next(elem, 0);
        l.set_prev(elem, 0);
        return;
    }
    let after = l.next(pred);
    l.set_next(elem, after);
    l.set_prev(elem, pred);
    if after != 0 {
        l.set_prev(after, elem);
    }
    l.set_next(pred, elem);
}

/// `remque(elem)`: unlink `elem` from its neighbours.
pub fn queue_remove<L: Links + ?Sized>(l: &mut L, elem: usize) {
    let (before, after) = (l.prev(elem), l.next(elem));
    if before != 0 {
        l.set_next(before, after);
    }
    if after != 0 {
        l.set_prev(after, before);
    }
}
