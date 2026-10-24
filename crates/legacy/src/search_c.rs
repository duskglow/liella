//! The C shapes of spec §14.1–§14.6 over `liella::search`: comparator callbacks, trees, hash
//! tables and queues whose nodes live in memory the C caller sees (D-14.2, D-14.3).
//!
//! Memory is reached through [`CWords`], which the boundary implements over C addresses
//! (category *slice*), with the caller's allocator for nodes and segments.
//!
//! * Tree nodes are `{key, left, right, height}` words; only the leading key is ABI.
//! * Hash segments are `{next, capacity, used}` words followed by `capacity` `ENTRY`s
//!   (`{key, data}`); `hsearch` returns entry addresses, which stay valid as the table grows.
//! * Queue elements start with their forward and backward pointers.

use core::cmp::Ordering;
use core::ffi::c_void;
use core::sync::atomic::Ordering as AtomicOrdering;

use liella::error::{Errno, Result};
use liella::search::{self, Deleted, HashMem, Links, TreeNodes, Visit};
use liella_abi::callback::{ActionCtxFn, ActionFn, CompareFn, FreeFn};
use liella_abi::layout::{struct_hsearch_data, ENTRY};

use crate::search_global::{FILLED, SIZE, TABLE};
use crate::string_compat::Cursor;

/// Caller-visible memory: machine words at addresses, the caller's allocator, and C text.
pub trait CWords {
    /// Word `i` at `addr` (0 if unreadable).
    fn word(&self, addr: usize, i: usize) -> usize;
    /// Store word `i` at `addr`.
    fn set_word(&mut self, addr: usize, i: usize, v: usize);
    /// `words` zeroed words from the caller's allocator (`calloc`); `None` without memory.
    fn alloc(&mut self, words: usize) -> Option<usize>;
    /// Return memory to the caller's allocator (`free`).
    fn free(&mut self, addr: usize);
    /// The NUL-terminated text at `addr` (empty if unreadable).
    fn text(&self, addr: usize) -> &[u8];
    /// At most `limit` bytes of the text at `addr`, read no further than that (the environment's
    /// name comparisons). The default measures the whole text; an implementation over real C
    /// memory overrides it with a bounded read.
    fn text_prefix(&self, addr: usize, limit: usize) -> &[u8] {
        let t = self.text(addr);
        t.get(..limit.min(t.len())).unwrap_or(t)
    }
    /// The index and address of the first entry of the NULL-terminated array at `env` (0: none)
    /// that is `name` followed by `=` (`setenv`'s and `putenv`'s lookup). The default compares
    /// entry by entry ([`crate::environ::find_scan`]); an implementation over real C memory
    /// overrides it with a faster scanner giving the same answer (ENVIRON-PUTENV-SLOPE).
    fn env_find(&self, env: usize, name: &[u8]) -> Option<(usize, usize)>
    where
        Self: Sized,
    {
        crate::environ::find_scan(env, name, self)
    }
    /// [`CWords::env_find`], answering on a miss the array's entry count instead of `None`
    /// (`setenv`'s and `putenv`'s append; ENVIRON-SMALL-CYCLE). The default scans, then counts.
    fn env_find_end(&self, env: usize, name: &[u8]) -> core::result::Result<(usize, usize), usize>
    where
        Self: Sized,
    {
        self.env_find(env, name).ok_or_else(|| crate::environ::count(env, self))
    }
}

fn ptr(addr: usize) -> *const c_void {
    core::ptr::with_exposed_provenance(addr)
}

fn order(v: i32) -> Ordering {
    v.cmp(&0)
}

// ------------------------------------------------------------------------------ arrays

pub use liella::search::{SortCompare, SortScratch};

/// `qsort` and `qsort_r`: stable, never fails (D-14.1). `cmp` is the C comparator as the
/// boundary calls it, on elements named by offset ([`SortCompare`]); `scratch` shows where the
/// sort stands if the comparator throws (OD-6 revised).
pub fn qsort<C: SortCompare>(base: &mut [u8], size: usize, cmp: C, scratch: &mut SortScratch) {
    search::sort_stable_tracked(base, size, cmp, scratch);
}

/// `bsearch`: the byte offset of an element equal to `key`.
pub fn bsearch(key: *const c_void, base: &[u8], size: usize, cmp: CompareFn) -> Option<usize> {
    search::binary_search(base, size, &mut |e| order(cmp(key, e.as_ptr().cast()))).map(|i| i.saturating_mul(size))
}

/// `lfind`: the byte offset of the first element equal to `key`.
pub fn lfind(key: *const c_void, base: &[u8], size: usize, cmp: CompareFn) -> Option<usize> {
    search::linear_find(base, size, &mut |e| cmp(key, e.as_ptr().cast()) == 0).map(|i| i.saturating_mul(size))
}

/// `lsearch`: as [`lfind`], appending `key` (and counting it in `*n`) when absent. `base` has
/// room for one element more than `*n`.
pub fn lsearch(key: &[u8], base: &mut [u8], n: &mut usize, size: usize, cmp: CompareFn) -> usize {
    let used = n.saturating_mul(size);
    let found = lfind(key.as_ptr().cast(), base.get(..used).unwrap_or(&[]), size, cmp);
    found.unwrap_or_else(|| {
        if let Some(slot) = base.get_mut(used..used.saturating_add(size)) {
            liella::bytes::copy(slot, key);
        }
        *n = n.saturating_add(1);
        used
    })
}

// ------------------------------------------------------------------------------ trees

/// A tree in C memory, rooted at the caller's `void *` variable.
pub struct CTree<R: Cursor, W: CWords> {
    root: R,
    mem: W,
}

/// A root held in a local variable (`twalk`, `tdestroy`, `tfind` receive the root itself).
struct LocalRoot(usize);

impl Cursor for LocalRoot {
    fn get(&self) -> usize {
        self.0
    }
    fn set(&mut self, addr: usize) {
        self.0 = addr;
    }
}

impl<R: Cursor, W: CWords> TreeNodes for CTree<R, W> {
    fn root(&self) -> usize {
        self.root.get()
    }
    fn set_root(&mut self, n: usize) {
        self.root.set(n);
    }
    fn key(&self, n: usize) -> usize {
        self.mem.word(n, 0)
    }
    fn link(&self, n: usize, right: bool) -> usize {
        self.mem.word(n, if right { 2 } else { 1 })
    }
    fn set_link(&mut self, n: usize, right: bool, child: usize) {
        self.mem.set_word(n, if right { 2 } else { 1 }, child);
    }
    fn height(&self, n: usize) -> u32 {
        u32::try_from(self.mem.word(n, 3)).unwrap_or(0)
    }
    fn set_height(&mut self, n: usize, h: u32) {
        self.mem.set_word(n, 3, usize::try_from(h).unwrap_or(0));
    }
    fn alloc(&mut self, key: usize) -> Option<usize> {
        let n = self.mem.alloc(4)?;
        self.mem.set_word(n, 0, key);
        self.mem.set_word(n, 3, 1);
        Some(n)
    }
    fn free(&mut self, n: usize) {
        self.mem.free(n);
    }
}

/// `tsearch`: the node holding a key equal to `key`, inserting one; `ENOMEM` if a node cannot
/// be allocated.
pub fn tsearch(key: usize, root: impl Cursor, mem: impl CWords, cmp: CompareFn) -> Result<usize> {
    let mut t = CTree { root, mem };
    match search::tree_search(&mut t, key, true, &mut |a, b| order(cmp(ptr(a), ptr(b)))) {
        Ok(n) => Ok(n.unwrap_or(0)),
        Err(()) => Err(Errno::ENOMEM),
    }
}

/// `tfind`: the node holding a key equal to `key`, 0 if none (the root is not changed).
pub fn tfind(key: usize, root: usize, mem: impl CWords, cmp: CompareFn) -> usize {
    let mut t = CTree { root: LocalRoot(root), mem };
    search::tree_search(&mut t, key, false, &mut |a, b| order(cmp(ptr(a), ptr(b)))).ok().flatten().unwrap_or(0)
}

/// `tdelete`: the deleted node's parent (the new root, or `root_addr`, the root variable's
/// address, when the tree became empty: non-NULL on success); 0 if absent.
pub fn tdelete(key: usize, root_addr: usize, root: impl Cursor, mem: impl CWords, cmp: CompareFn) -> usize {
    let mut t = CTree { root, mem };
    match search::tree_delete(&mut t, key, &mut |a, b| order(cmp(ptr(a), ptr(b)))) {
        Deleted::Absent => 0,
        Deleted::Parent(p) | Deleted::NewRoot(p) => p,
        Deleted::Empty => root_addr,
    }
}

fn visit_value(v: Visit) -> i32 {
    match v {
        Visit::Preorder => 0,
        Visit::Postorder => 1,
        Visit::Endorder => 2,
        Visit::Leaf => 3,
    }
}

/// `twalk`; a NULL action visits nothing.
pub fn twalk(root: usize, mem: impl CWords, action: Option<ActionFn>) {
    let Some(action) = action else { return };
    let t = CTree { root: LocalRoot(root), mem };
    search::tree_walk(&t, root, 0, &mut |n, v, d| action(ptr(n), visit_value(v), i32::try_from(d).unwrap_or(i32::MAX)));
}

/// `twalk_r`; a NULL action visits nothing.
pub fn twalk_r(root: usize, mem: impl CWords, action: Option<ActionCtxFn>, ctx: *mut c_void) {
    let Some(action) = action else { return };
    let t = CTree { root: LocalRoot(root), mem };
    search::tree_walk(&t, root, 0, &mut |n, v, _| action(ptr(n), visit_value(v), ctx));
}

/// `tdestroy`: free every node, calling `free_key` on each key (none for a NULL function).
pub fn tdestroy(root: usize, mem: impl CWords, free_key: Option<FreeFn>) {
    let mut t = CTree { root: LocalRoot(root), mem };
    search::tree_destroy(&mut t, root, &mut |k| {
        if let Some(f) = free_key {
            f(core::ptr::with_exposed_provenance_mut(k));
        }
    });
}

// ------------------------------------------------------------------------------ hash tables

/// Where a table's head and counters live.
pub enum Table<'a> {
    /// The process-global table of `hsearch` (⚑ HIST).
    Global,
    /// A caller `struct hsearch_data`.
    Caller(&'a mut struct_hsearch_data),
}

/// A hash table in C memory.
pub struct CHash<'a, W: CWords> {
    table: Table<'a>,
    mem: W,
}

const HDR: usize = 3;

impl<W: CWords> CHash<'_, W> {
    fn add_size(&mut self, cap: usize) {
        let c = u32::try_from(cap).unwrap_or(u32::MAX);
        match &mut self.table {
            Table::Global => {
                SIZE.fetch_add(c, AtomicOrdering::Relaxed);
            }
            Table::Caller(d) => d.size = d.size.saturating_add(c),
        }
    }
}

impl<W: CWords> HashMem for CHash<'_, W> {
    fn head(&self) -> usize {
        match &self.table {
            Table::Global => TABLE.load(AtomicOrdering::Relaxed),
            Table::Caller(d) => d.table,
        }
    }
    fn set_head(&mut self, seg: usize) {
        match &mut self.table {
            Table::Global => TABLE.store(seg, AtomicOrdering::Relaxed),
            Table::Caller(d) => d.table = seg,
        }
    }
    fn next(&self, seg: usize) -> usize {
        self.mem.word(seg, 0)
    }
    fn capacity(&self, seg: usize) -> usize {
        self.mem.word(seg, 1)
    }
    fn used(&self, seg: usize) -> usize {
        self.mem.word(seg, 2)
    }
    fn set_used(&mut self, seg: usize, n: usize) {
        let grew = n > self.mem.word(seg, 2);
        self.mem.set_word(seg, 2, n);
        if grew {
            match &mut self.table {
                Table::Global => {
                    FILLED.fetch_add(1, AtomicOrdering::Relaxed);
                }
                Table::Caller(d) => d.filled = d.filled.saturating_add(1),
            }
        }
    }
    fn key(&self, seg: usize, i: usize) -> usize {
        self.mem.word(seg, HDR.saturating_add(i.saturating_mul(2)))
    }
    fn set_entry(&mut self, seg: usize, i: usize, key: usize, data: usize) {
        let at = HDR.saturating_add(i.saturating_mul(2));
        self.mem.set_word(seg, at, key);
        self.mem.set_word(seg, at.saturating_add(1), data);
    }
    fn entry_addr(&self, seg: usize, i: usize) -> usize {
        seg.saturating_add(HDR.saturating_add(i.saturating_mul(2)).saturating_mul(core::mem::size_of::<usize>()))
    }
    fn alloc_segment(&mut self, capacity: usize, next: usize) -> Option<usize> {
        let seg = self.mem.alloc(HDR.checked_add(capacity.checked_mul(2)?)?)?;
        self.mem.set_word(seg, 0, next);
        self.mem.set_word(seg, 1, capacity);
        self.add_size(capacity);
        Some(seg)
    }
    fn free_segment(&mut self, seg: usize) {
        self.mem.free(seg);
    }
    fn text(&self, key: usize) -> &[u8] {
        self.mem.text(key)
    }
}

/// `hcreate`/`hcreate_r`: a first segment of at least `nel` entries (a prime). A table that
/// already exists is kept (it grows anyway, D-14.3).
pub fn hcreate(table: Table<'_>, nel: usize, mem: impl CWords) -> Result<()> {
    let mut h = CHash { table, mem };
    if h.head() != 0 {
        return Ok(());
    }
    let seg = h.alloc_segment(search::prime_at_least(nel), 0).ok_or(Errno::ENOMEM)?;
    h.set_head(seg);
    Ok(())
}

/// `hsearch`/`hsearch_r`: the entry for `item.key` (inserting `item` with `enter`); `ESRCH`
/// when absent, `ENOMEM` when the table cannot grow.
pub fn hsearch(table: Table<'_>, item: ENTRY, enter: bool, mem: impl CWords) -> Result<usize> {
    let mut h = CHash { table, mem };
    match search::hash_search(&mut h, item.key, item.data, enter) {
        Ok(Some(e)) => Ok(e),
        Ok(None) => Err(Errno::ESRCH),
        Err(()) => Err(Errno::ENOMEM),
    }
}

/// `hsearch_r`: as [`hsearch`], storing the entry (or NULL) in `*retval`.
pub fn hsearch_r(table: Table<'_>, item: ENTRY, enter: bool, mut retval: impl Cursor, mem: impl CWords) -> Result<()> {
    let r = hsearch(table, item, enter, mem);
    retval.set(*r.as_ref().unwrap_or(&0));
    r.map(drop)
}

/// `hdestroy`/`hdestroy_r` (keys are not freed).
pub fn hdestroy(table: Table<'_>, mem: impl CWords) {
    let mut h = CHash { table, mem };
    search::hash_destroy(&mut h);
    match &mut h.table {
        Table::Global => {
            SIZE.store(0, AtomicOrdering::Relaxed);
            FILLED.store(0, AtomicOrdering::Relaxed);
        }
        Table::Caller(d) => {
            d.size = 0;
            d.filled = 0;
        }
    }
}

// ------------------------------------------------------------------------------ queues

struct CLinks<W>(W);

impl<W: CWords> Links for CLinks<W> {
    fn next(&self, e: usize) -> usize {
        self.0.word(e, 0)
    }
    fn prev(&self, e: usize) -> usize {
        self.0.word(e, 1)
    }
    fn set_next(&mut self, e: usize, v: usize) {
        self.0.set_word(e, 0, v);
    }
    fn set_prev(&mut self, e: usize, v: usize) {
        self.0.set_word(e, 1, v);
    }
}

/// `insque`.
pub fn insque(elem: usize, pred: usize, mem: impl CWords) {
    search::queue_insert(&mut CLinks(mem), elem, pred);
}

/// `remque`.
pub fn remque(elem: usize, mem: impl CWords) {
    search::queue_remove(&mut CLinks(mem), elem);
}
