//! The matcher: a breadth-first simulation of the automaton over the subject (D-14.4).
//!
//! Threads advance position by position. At each position the states holding threads are
//! visited in the program's topological order, so every epsilon successor is visited after all
//! its predecessors; consuming instructions hand their threads to the position after the
//! character (a small ring of frontiers), back-references to the position after the referenced
//! text.
//!
//! **Dominance.** At one state and position, threads whose futures are equal are merged: a
//! thread with a worse start is dropped (leftmost wins; for a backward search, the latest start
//! wins), and among threads with the same start the submatch [`Policy`] keeps the best thread of
//! each comparison class. Without back-references the future of a thread depends only on its
//! state, so a state holds at most `classes` threads; with back-references the texts that the
//! referenced groups hold are part of the future, so threads are merged only when those groups
//! are equal (one *bucket* per distinct value).
//!
//! **Time.** Without back-references every position does work bounded by the size of the
//! automaton (states × classes × groups), whatever the subject: the matcher is linear in the
//! subject for a fixed pattern. With back-references the number of buckets can grow with the
//! subject; the work is then counted and limited by the caller's budget (`re_max_failures`),
//! beyond which the search fails with `REG_ESPACE` rather than run unbounded.
#![allow(clippy::arithmetic_side_effects)] // positions are bounded by the subject's length, indices by the arenas' lengths

use crate::locale::Locale;
use crate::mem::TryVec;

use super::policy::Policy;
use super::program::{ProgRef, NONE, OP_ASSERT, OP_BACKREF, OP_BYTE, OP_CLOSE, OP_ENTER, OP_EXIT, OP_JMP, OP_MATCH, OP_OPEN, OP_SET, OP_SPLIT};
use super::scratch::{self, Buf, Grow, Pool};
use super::text::{self, Mode, Unit};
use super::ReError;

/// Positions ahead that the largest ring of frontiers covers. A search uses fewer when the
/// subject's characters are shorter (two for single bytes); a thread sent beyond the ring waits
/// on the pending list ([`Matcher::run`]) or makes the search run again with this ring
/// ([`Matcher::run_fast`]).
const RING: usize = 16;

/// Words per node of [`State::info`]: instruction index, chain index, best slots, their offset
/// (with back-references: the closure-walk word of [`Matcher::mark_inline`] instead).
const INFO: usize = 4;

/// Closure walk (BACKREF-EPS), patterns with back-references: an epsilon node is followed
/// directly, without a bucket or thread of its own, when it has one predecessor (the walk is then
/// a tree, each node at most once per expansion) or when its own walk takes at most this many
/// hops (a join: copied per arrival, at most this much).
const EPS_JOIN: u32 = 4;
/// The flag of an inlined node in its info word 3; the low bits hold its walk's hops.
const EPS_INLINE: u32 = 1 << 31;
/// Split branches followed on a copy of the captures, nested at most this deep, for threads of at
/// most [`EPS_LOCAL`] elements; past either the branch takes a bucket as before.
const EPS_DEPTH: u32 = 16;
const EPS_LOCAL: usize = 32;

/// The base of the polynomial hash of back-referenced texts ([`Matcher::key_hash`]).
const HASH_BASE: u64 = 0x0100_0000_01b3;

/// Subjects up to this length hash each back-referenced text directly (at most this many
/// steps) instead of building prefix tables (two heap blocks per search).
const DIRECT_HASH: usize = 64;

/// The largest starting room per frontier (threads, buckets, best slots).
const ROOM_CAP: usize = 4096;

/// How the subject's edges are seen.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Ctx {
    /// `^` does not match at the start of the subject.
    pub(crate) not_bol: bool,
    /// `$` does not match at its end.
    pub(crate) not_eol: bool,
    /// `^` and `$` also match after and before a newline.
    pub(crate) newline_anchor: bool,
}

/// One search.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Run {
    /// Track subexpressions (otherwise only the overall match is found).
    pub(crate) caps: bool,
    /// The first and last start positions to try.
    pub(crate) first_start: usize,
    pub(crate) last_start: usize,
    /// The latest start wins (a backward search) instead of the earliest.
    pub(crate) late: bool,
    /// Accept only a match ending here.
    pub(crate) end: Option<usize>,
    /// Work limit (thread insertions), for patterns with back-references.
    pub(crate) budget: Option<u64>,
}

/// A match: its start and end, and the subexpressions (`so`, `eo` per group, -1 unset; empty
/// without `Run::caps`), in the search's scratch.
#[derive(Debug)]
pub(crate) struct Found<'s> {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) caps: &'s [i32],
}

/// Why a search stopped without an answer: an error, or a scratch buffer too small (run again).
#[derive(Clone, Copy, Debug)]
pub(crate) enum Fail {
    Re(ReError),
    Grow(Dim),
}

/// Which part of a [`Room`] was too small (only that part grows: growing every part when one
/// needs more multiplied the scratch of a pattern with many buckets until it ran out of memory).
#[derive(Clone, Copy, Debug)]
pub(crate) enum Dim {
    Ring,
    Threads,
    Buckets,
    Bests,
    All,
    /// Several of threads, buckets and bests ([`State::widen`]).
    Many { threads: bool, buckets: bool, bests: bool },
}

impl From<ReError> for Fail {
    fn from(e: ReError) -> Self {
        Fail::Re(e)
    }
}

impl From<Grow> for Fail {
    fn from(_: Grow) -> Self {
        Fail::Grow(Dim::All)
    }
}

/// The room a search's buffers get: frontiers in the ring, and per frontier, threads, buckets
/// and per-class best slots. Sized before the search from the program; doubled when a search
/// needs more ([`Grow`]).
#[derive(Clone, Copy, Debug)]
pub(crate) struct Room {
    ring: usize,
    threads: usize,
    buckets: usize,
    bests: usize,
}

/// Elements per pool that a search with some [`Room`] uses.
#[derive(Clone, Copy, Debug, Default)]
struct Sizes {
    u32s: usize,
    u64s: usize,
    i32s: usize,
    usizes: usize,
    buckets: usize,
}

impl Sizes {
    fn max(self, o: Sizes) -> Sizes {
        Sizes { u32s: self.u32s.max(o.u32s), u64s: self.u64s.max(o.u64s), i32s: self.i32s.max(o.i32s), usizes: self.usizes.max(o.usizes), buckets: self.buckets.max(o.buckets) }
    }

    fn within(&self, cap: &Sizes) -> bool {
        self.u32s <= cap.u32s && self.u64s <= cap.u64s && self.i32s <= cap.i32s && self.usizes <= cap.usizes && self.buckets <= cap.buckets
    }

    #[cfg(test)]
    fn fits_inline(&self) -> bool {
        self.within(&Small::CAP)
    }

    /// The same sizes as one caller-supplied block per element type (buckets in the words).
    fn external(&self) -> super::ScratchNeed {
        super::ScratchNeed { u32s: self.u32s, u64s: self.u64s.saturating_add(self.buckets.saturating_mul(4)), i32s: self.i32s, usizes: self.usizes }
    }
}

impl Room {
    fn grown(self, d: Dim) -> Room {
        let two = |v: usize| v.saturating_mul(2);
        match d {
            Dim::Ring => Room { ring: RING, ..self },
            Dim::Threads => Room { threads: two(self.threads), ..self },
            Dim::Buckets => Room { buckets: two(self.buckets), ..self },
            Dim::Bests => Room { bests: two(self.bests), ..self },
            Dim::All => Room { ring: RING, threads: two(self.threads), buckets: two(self.buckets), bests: two(self.bests) },
            Dim::Many { threads, buckets, bests } => {
                // Four times: each rerun repeats the search up to the overflow, so fewer and
                // larger steps waste less (a third of the last search at most, against all of it
                // when doubling), for at most four times the room a part needs.
                let four_if = |on: bool, v: usize| if on { v.saturating_mul(4) } else { v };
                Room { threads: four_if(threads, self.threads), buckets: four_if(buckets, self.buckets), bests: four_if(bests, self.bests), ..self }
            }
        }
    }
}

impl<const A: usize, const B: usize, const C: usize, const D: usize, const E: usize> Inline<A, B, C, D, E> {
    const CAP: Sizes = Sizes { u32s: A, u64s: B, i32s: C, usizes: D, buckets: E / 4 };

    fn new() -> Self {
        Inline { u32s: [0; A], u64s: [0; B], i32s: [0; C], usizes: [0; D], buckets: [0; E] }
    }

    fn mem(&mut self) -> Mem<'_> {
        Mem { u32s: &mut self.u32s, u64s: &mut self.u64s, i32s: &mut self.i32s, usizes: &mut self.usizes, buckets: &mut self.buckets }
    }
}

/// Heap storage for the programs whose buffers do not fit [`Small`] when the caller has no
/// scratch of its own: one block per pool.
struct Heap {
    u32s: TryVec<u32>,
    u64s: TryVec<u64>,
    i32s: TryVec<i32>,
    usizes: TryVec<usize>,
    buckets: TryVec<u64>,
}

impl Heap {
    fn new(n: &Sizes) -> Result<Self, ReError> {
        let e = |()| ReError::ESpace;
        Ok(Heap { u32s: scratch::heap(n.u32s).map_err(e)?, u64s: scratch::heap(n.u64s).map_err(e)?, i32s: scratch::heap(n.i32s).map_err(e)?, usizes: scratch::heap(n.usizes).map_err(e)?, buckets: scratch::heap(n.buckets.saturating_mul(4)).map_err(e)? })
    }

    fn mem(&mut self) -> Mem<'_> {
        Mem { u32s: self.u32s.as_mut_slice(), u64s: self.u64s.as_mut_slice(), i32s: self.i32s.as_mut_slice(), usizes: self.usizes.as_mut_slice(), buckets: self.buckets.as_mut_slice() }
    }
}

/// The storage of one search's pools (buckets packed as four words, [`Bucket::pack`]).
struct Mem<'m> {
    u32s: &'m mut [u32],
    u64s: &'m mut [u64],
    i32s: &'m mut [i32],
    usizes: &'m mut [usize],
    buckets: &'m mut [u64],
}

/// The pools of one pass of a search, carved from a [`Mem`].
struct Pools<'s> {
    u32s: Pool<'s, u32>,
    u64s: Pool<'s, u64>,
    i32s: Pool<'s, i32>,
    usizes: Pool<'s, usize>,
    buckets: Pool<'s, u64>,
}

impl<'m> Mem<'m> {
    /// A caller-supplied scratch as the pools of a search needing `need` (`None` when a block
    /// is too small).
    fn external(m: super::ScratchMem<'m>, need: &Sizes) -> Option<Mem<'m>> {
        let (u64s, rest) = m.u64s.split_at_mut_checked(need.u64s)?;
        let buckets = rest.get_mut(..need.buckets.checked_mul(4)?)?;
        if m.u32s.len() < need.u32s || m.i32s.len() < need.i32s || m.usizes.len() < need.usizes {
            return None;
        }
        Some(Mem { u32s: m.u32s, u64s, i32s: m.i32s, usizes: m.usizes, buckets })
    }

    fn pools(&mut self) -> Pools<'_> {
        Pools { u32s: Pool::new(self.u32s), u64s: Pool::new(self.u64s), i32s: Pool::new(self.i32s), usizes: Pool::new(self.usizes), buckets: Pool::new(self.buckets) }
    }
}

/// Stack storage for the small programs that most searches use (zeroed per call): [`Small`].
/// Larger needs use the caller's [`super::ScratchSource`] (`regexec`: the thread's own
/// blocks, kept between calls) or else the heap.
struct Inline<const A: usize, const B: usize, const C: usize, const D: usize, const E: usize> {
    u32s: [u32; A],
    u64s: [u64; B],
    i32s: [i32; C],
    usizes: [usize; D],
    buckets: [u64; E],
}

/// The only stack storage, for both passes of a search together: 3,712 bytes (256 x 4 +
/// 16 x 8 + 128 x 4 + 192 x 8 + 16 x 32), so that a thread with a `PTHREAD_STACK_MIN` (16 KiB)
/// stack can call `regexec` (D-14.11). Every REG_NOSUB pattern of the benchmarks fits.
type Small = Inline<256, 16, 128, 192, 64>;

/// The stack storage of a fast pass alone (REG_NOSUB, no back-references) on a small program:
/// 800 bytes, so that such a call does not zero all of [`Small`] (RX-CAP2: zeroing it was
/// about 2,000 instructions of a 39,000-instruction search).
type Tiny = Inline<64, 4, 0, 64, 0>;

/// Packed as four words in the scratch: `next | best << 32`, `start`, `node | slot << 32`, `key`.
#[derive(Clone, Copy, Debug, Default)]
struct Bucket {
    next: u32,
    start: usize,
    /// Offset of the bucket's per-class best threads in `Frontier::bests`.
    best: u32,
    /// Its node and the hash of its back-referenced texts (patterns with back-references).
    node: u32,
    key: u64,
    /// Its slot in `Frontier::table`, so that emptying the table does not search for it.
    slot: u32,
}

/// Bucket `i` of a frontier's packed buckets.
fn bucket_at(b: &Buf<'_, u64>, i: usize) -> Option<Bucket> {
    let at = i.checked_mul(4)?;
    let w = b.as_slice().get(at..at.checked_add(4)?)?;
    Some(Bucket::unpack([*w.first()?, *w.get(1)?, *w.get(2)?, *w.get(3)?]))
}

impl Bucket {
    fn pack(self) -> [u64; 4] {
        [u64::from(self.next) | u64::from(self.best) << 32, u64::try_from(self.start).unwrap_or(u64::MAX), u64::from(self.node) | u64::from(self.slot) << 32, self.key]
    }

    fn unpack(w: [u64; 4]) -> Bucket {
        let [a, start, node, key] = w;
        let lo = |v: u64| u32::try_from(v & 0xffff_ffff).unwrap_or(0);
        Bucket { next: lo(a), best: lo(a >> 32), start: usize::try_from(start).unwrap_or(usize::MAX), node: lo(node), key, slot: lo(node >> 32) }
    }
}

/// The threads of one position.
#[derive(Default)]
struct Frontier<'s> {
    epoch: u32,
    node_gen: Buf<'s, u32>,
    head: Buf<'s, u32>,
    active: Buf<'s, u64>,
    live: bool,
    buckets: Buf<'s, u64>,
    bests: Buf<'s, u32>,
    caps: Buf<'s, i32>,
    threads: u32,
    /// Patterns with back-references (buckets, `head`, `table`) or without (one bucket per
    /// node: its start in `start`, its best threads at the node's fixed offset in `bests`).
    indexed: bool,
    start: Buf<'s, usize>,
    /// Patterns with back-references: the buckets by node and key hash (open addressing, a
    /// bucket's index plus one per slot, 0 empty; at most half full, since its size is fixed
    /// at twice the buckets' room), so that finding a thread's bucket does not walk every
    /// bucket of its node (which made `\(a*\)\1...` cubic).
    table: Buf<'s, u32>,
    /// Its states, and whether `node_gen`, `active` and `table` are initialised: a frontier is
    /// set up at its first insertion, not per search ([`Frontier::prepare`]; BACKREF-ARCH: a
    /// short subject touches a few of the 16 frontiers of a back-reference search).
    nodes: usize,
    ready: bool,
}

/// The groups set in a back-reference mask (bit `g` for group `g`), lowest first: visits only the
/// set bits, where a scan of all 31 group numbers cost every thread insertion ~100 instructions.
fn bref_groups(brefs: u32) -> impl Iterator<Item = u32> {
    let mut m = brefs & !1;
    core::iter::from_fn(move || {
        let g = m.trailing_zeros();
        m &= m.wrapping_sub(1);
        (g < 32).then_some(g)
    })
}

fn nomem<T>(r: crate::error::Result<T>) -> Result<T, ReError> {
    r.map_err(|_| ReError::ESpace)
}

fn idx(v: u32) -> usize {
    usize::try_from(v).unwrap_or(usize::MAX)
}

/// The elements a thread's captures take in the scratch: `so` and `eo` per group; without
/// back-references padded to a multiple of four (unset, -1) so that [`copy_caps`] moves whole
/// 16-byte blocks (with them, the longer captures would cost more in the bucket comparisons).
fn stride_of(ngroups: usize, backrefs: bool) -> usize {
    if backrefs { ngroups * 2 } else { (ngroups * 2).next_multiple_of(4) }
}

/// Copy a thread's captures (`dst` and `src` of the same length) in 16-byte blocks: a slice copy
/// of a run-time length is a library call, and threads are copied at every step of a search.
fn copy_caps(dst: &mut [i32], src: &[i32]) {
    let (mut d, mut s) = (dst.chunks_exact_mut(4), src.chunks_exact(4));
    for (d, s) in (&mut d).zip(&mut s) {
        if let (Ok(d), Ok(s)) = (<&mut [i32; 4]>::try_from(d), <&[i32; 4]>::try_from(s)) {
            *d = *s;
        }
    }
    for (d, s) in d.into_remainder().iter_mut().zip(s.remainder()) {
        *d = *s;
    }
}

/// The table size of a frontier with room for `buckets` buckets (patterns with back-references).
fn table_len(buckets: usize) -> usize {
    buckets.saturating_mul(2).next_power_of_two().max(64)
}

impl<'s> Frontier<'s> {
    /// An empty frontier for `nodes` states, with the buffers `room` gives it.
    /// Without back-references, `classes` is the number of best slots of all nodes together
    /// (each node's slots are at a fixed offset, [`Matcher::node_info`]).
    fn new(pools: &mut Pools<'s>, room: &Room, nodes: usize, stride: usize, indexed: bool, classes: usize) -> Result<Self, Fail> {
        let mut f = Frontier { epoch: 1, indexed, nodes, ..Frontier::default() };
        f.node_gen = pools.u32s.take(nodes)?;
        f.active = pools.u64s.take(nodes.div_ceil(64))?;
        f.caps = pools.i32s.take(room.threads.saturating_mul(stride))?;
        f.bests = pools.u32s.take(room.bests)?;
        if indexed {
            // Read only where `node_gen` marks the node present, so never filled.
            f.head = pools.u32s.take(nodes)?;
            f.head.expose(nodes)?;
            f.buckets = pools.buckets.take(room.buckets.saturating_mul(4))?;
            f.table = pools.u32s.take(table_len(room.buckets))?;
        } else {
            // Read only where `node_gen` marks the node present, so never written here.
            f.start = pools.usizes.take(nodes)?;
            f.start.expose(nodes)?;
            f.bests.expose(classes).map_err(|_| Fail::Grow(Dim::Bests))?;
        }
        Ok(f)
    }

    /// Initialise the frontier before its first insertion (its buffers were taken in
    /// [`Frontier::new`] with exactly these lengths, so the fills cannot fail).
    fn prepare(&mut self) {
        if self.ready {
            return;
        }
        self.ready = true;
        let n = self.nodes;
        let ok = self.node_gen.fill(n, 0).is_ok() && self.active.fill(n.div_ceil(64), 0).is_ok();
        let len = self.table.capacity();
        let ok = ok && (!self.indexed || self.table.fill(len, 0).is_ok());
        debug_assert!(ok, "regex frontier buffers shorter than taken");
    }

    fn clear(&mut self) {
        if !self.live {
            return;
        }
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.node_gen.as_mut_slice().fill(0);
            self.epoch = 1;
        }
        self.active.as_mut_slice().fill(0);
        self.live = false;
        self.caps.clear();
        self.threads = 0;
        if self.indexed {
            // Before the buckets are emptied: the table is reset through them.
            self.clear_table();
            self.buckets.clear();
            self.bests.clear();
        }
    }

    /// Empty the bucket table. With few buckets (the common case: a short subject) only their
    /// slots are reset, not the whole table (at least 64 words, at every position).
    fn clear_table(&mut self) {
        let len = self.table.len();
        let nb = self.buckets.len() / 4;
        if nb.saturating_mul(8) >= len {
            self.table.as_mut_slice().fill(0);
            return;
        }
        // Each bucket remembers its slot (BACKREF-ARCH: no probe per bucket).
        for b in 0..nb {
            let Some(bk) = bucket_at(&self.buckets, b) else { continue };
            if let Some(t) = self.table.get_mut(idx(bk.slot)) {
                *t = 0;
            }
        }
        // A slot left set is never wrong, only slow (lookups and inserts probe past it), so no
        // answer shows it: check it here in test builds.
        debug_assert!(self.table.as_mut_slice().iter().all(|&t| t == 0), "regex bucket table not empty after clear");
    }

    /// The slot of `(node, key)` in a table of `len` slots (a power of two).
    fn slot(node: u32, key: u64, len: usize) -> usize {
        let h = (key ^ u64::from(node).wrapping_mul(0x9e37_79b9_7f4a_7c15)).wrapping_mul(0xff51_afd7_ed55_8ccd);
        usize::try_from(h >> 32).unwrap_or(0) & (len - 1)
    }

    /// Enter bucket `b` in the table (at most half full: the table has twice the buckets' room),
    /// from slot `i` on: the empty slot the lookup that missed it ended at, so that the insertion
    /// does not probe the same chain twice (BACKREF-COST).
    /// Returns the slot used.
    fn index(&mut self, b: u32, mut i: usize) -> u32 {
        let len = self.table.len();
        if len == 0 {
            return 0;
        }
        i &= len - 1;
        for _ in 0..len {
            match self.table.get_mut(i) {
                Some(t) if *t == 0 => {
                    *t = b + 1;
                    return u32::try_from(i).unwrap_or(0);
                }
                _ => i = (i + 1) & (len - 1),
            }
        }
        0
    }

    #[inline]
    fn first_bucket(&self, node: u32) -> u32 {
        let i = idx(node);
        if self.node_gen.get(i) == Some(&self.epoch) { self.head.get(i).copied().unwrap_or(NONE) } else { NONE }
    }

    fn thread(&self, t: u32, stride: usize) -> &[i32] {
        let at = idx(t) * stride;
        self.caps.as_slice().get(at..at + stride).unwrap_or(&[])
    }

    fn add_thread(&mut self, caps: &[i32]) -> Result<u32, Fail> {
        // A block copy for both layouts: a slice extend of a run-time length is a library call.
        let r = self.caps.extend_with(caps.len(), |d| copy_caps(d, caps));
        r.map_err(|_| Fail::Grow(Dim::Threads))?;
        let t = self.threads;
        self.threads += 1;
        Ok(t)
    }
}

/// A thread a back-reference sent beyond the ring: kept on the list of its position.
struct Pending {
    node: u32,
    start: usize,
    caps: usize,
    /// The next thread sent to the same position (`NONE`: none).
    next: u32,
}

/// The mutable state of one search.
struct State<'r, 's> {
    run: &'r Run,
    /// Frontiers in use (the first `r` of `ring`).
    r: usize,
    /// Patterns with back-references: polynomial prefix hashes of the subject and the powers of
    /// the base, so that a text's hash costs O(1) (16 bytes per subject byte, for this call).
    prefix: TryVec<u64>,
    pows: TryVec<u64>,
    stride: usize,
    /// The position being processed.
    cur: usize,
    ring: [Frontier<'s>; RING],
    /// Threads beyond the ring, with their captures; per subject position the newest of its
    /// list (made at the first such thread: 4 bytes per subject byte, for this call), and how
    /// many are still waiting. Taking a position's threads costs only their number (a scan of
    /// every waiting thread at every position made `\(a*\)\1...` cubic).
    pending: TryVec<Pending>,
    pending_caps: TryVec<i32>,
    pending_head: TryVec<u32>,
    pending_live: usize,
    best: Option<(usize, usize)>,
    /// The captures of `best`.
    best_caps: Buf<'s, i32>,
    work: u64,
    /// The key of the thread being advanced, while its captures are those it was stored with
    /// (BACKREF-LONG): [`Matcher::insert`] then reuses it instead of hashing the texts again.
    /// Set by [`Matcher::visit`]; cleared by every step that changes captures.
    carry: Option<u64>,
    /// Patterns without back-references: per node, its instruction index, chain index, number
    /// of best slots and their offset ([`INFO`] words; computed once per search, not per
    /// thread).
    info: &'s [u32],
}

/// The matcher's view of one subject.
pub(crate) struct Matcher<'a, P: Policy> {
    pub(crate) prog: &'a ProgRef<'a>,
    pub(crate) mode: Mode,
    pub(crate) loc: &'a Locale,
    pub(crate) subject: &'a [u8],
    pub(crate) ctx: Ctx,
    pub(crate) policy: &'a P,
}

impl<P: Policy> Matcher<'_, P> {
    fn classes(&self, run: &Run, pc: u32) -> usize {
        if run.caps { self.prog.masks_of(pc).len() + 1 } else { 1 }
    }

    fn mask(&self, pc: u32, class: usize) -> Option<(u32, u32)> {
        class.checked_sub(1).and_then(|k| self.prog.masks_of(pc).get(k))
    }

    /// Whether two threads hold the same back-referenced texts (so their futures are equal). The
    /// texts are compared, not their offsets: `\(a*\)\1` seeded at every position holds the same
    /// text at many offsets, and only the thread with the better start (or captures) among them
    /// can win. Unset or open groups compare by their offsets.
    fn same_key(&self, a: &[i32], b: &[i32]) -> bool {
        let brefs = self.prog.brefs;
        if brefs == 0 {
            return true;
        }
        bref_groups(brefs).all(|g| {
            let at = idx(g - 1) * 2;
            let (x, y) = (a.get(at..at + 2), b.get(at..at + 2));
            if x == y {
                return true;
            }
            match (x, y) {
                (Some(&[xs, xe]), Some(&[ys, ye])) => match (usize::try_from(xs), usize::try_from(xe), usize::try_from(ys), usize::try_from(ye)) {
                    (Ok(xs), Ok(xe), Ok(ys), Ok(ye)) => xe - xs == ye - ys && self.subject.get(xs..xe).is_some_and(|t| self.subject.get(ys..ye) == Some(t)),
                    _ => false,
                },
                _ => false,
            }
        })
    }

    /// A hash of the back-referenced texts of `caps` (equal texts give equal hashes, whatever
    /// their offsets; unset and open groups hash their offsets), consistent with [`Self::same_key`].
    fn key_hash(&self, st: &State<'_, '_>, caps: &[i32]) -> u64 {
        let brefs = self.prog.brefs;
        let mut h = 0x243f_6a88_85a3_08d3u64;
        for g in bref_groups(brefs) {
            let at = idx(g - 1) * 2;
            let (so, eo) = (caps.get(at).copied().unwrap_or(-1), caps.get(at + 1).copied().unwrap_or(-1));
            let v = match (usize::try_from(so), usize::try_from(eo)) {
                // A short subject has no prefix table: the same polynomial, over the text.
                (Ok(s), Ok(e)) if s <= e && st.prefix.is_empty() => self.subject.get(s..e).unwrap_or(&[]).iter().fold(0u64, |h, &c| h.wrapping_mul(HASH_BASE).wrapping_add(u64::from(c) + 1)) ^ u64::try_from(e - s).unwrap_or(0).rotate_left(48),
                (Ok(s), Ok(e)) if s <= e => {
                    let (ps, pe, pw) = (st.prefix.get(s).copied().unwrap_or(0), st.prefix.get(e).copied().unwrap_or(0), st.pows.get(e - s).copied().unwrap_or(0));
                    pe.wrapping_sub(ps.wrapping_mul(pw)) ^ u64::try_from(e - s).unwrap_or(0).rotate_left(48)
                }
                _ => (u64::from(u32::from_ne_bytes(so.to_ne_bytes())) << 32 | u64::from(u32::from_ne_bytes(eo.to_ne_bytes()))) ^ 0x5bd1_e995,
            };
            h = (h ^ v).wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(29);
        }
        h
    }

    fn start_better(run: &Run, a: usize, b: usize) -> bool {
        if run.late { a > b } else { a < b }
    }

    /// A node's instruction, chain index, best slots and their offset (patterns without
    /// back-references, [`State::info`]).
    fn node_info(info: &[u32], node: u32) -> (u32, u32, usize, usize) {
        let at = idx(node) * INFO;
        match info.get(at..at + 4) {
            Some(&[pc, j, k, off]) => (pc, j, idx(k), idx(off)),
            _ => (0, 0, 0, 0),
        }
    }

    /// [`Self::insert`] for patterns without back-references: one bucket per node. Not inlined:
    /// kept out of [`Self::insert`], whose body is the back-reference path.
    #[inline(never)]
    fn insert_plain(&self, st: &mut State<'_, '_>, pos: usize, node: u32, start: usize, caps: &[i32]) -> Result<(), Fail> {
        let (pc, _, classes, off) = Self::node_info(st.info, node);
        let run = st.run;
        let i = idx(node);
        let Some(f) = st.ring.get_mut(pos & (st.r - 1)) else { return Ok(()) };
        f.prepare();
        let present = f.node_gen.get(i) == Some(&f.epoch);
        let old = if present { f.start.get(i).copied() } else { None };
        let Some(old) = old.filter(|&o| !Self::start_better(run, start, o)) else {
            // A new bucket, or a better start replacing the node's threads.
            let t = f.add_thread(caps)?;
            if let Some(s) = f.bests.as_mut_slice().get_mut(off..off + classes) {
                s.fill(t);
            }
            if let Some(s) = f.start.get_mut(i) {
                *s = start;
            }
            if !present {
                if let Some(g) = f.node_gen.get_mut(i) {
                    *g = f.epoch;
                }
                let r = idx(self.prog.rank.get(i).unwrap_or(0));
                if let Some(w) = f.active.get_mut(r / 64) {
                    *w |= 1 << (r % 64);
                }
                f.live = true;
            }
            return Ok(());
        };
        if Self::start_better(run, old, start) {
            return Ok(());
        }
        let masks = self.prog.masks_of(pc);
        let stride = st.stride;
        let mut new_t = NONE;
        for k in 0..classes {
            let inc = f.bests.get(off + k).copied().unwrap_or(NONE);
            let mask = k.checked_sub(1).and_then(|m| masks.get(m));
            if self.policy.prefer(mask, caps, f.thread(inc, stride)) {
                if new_t == NONE {
                    new_t = f.add_thread(caps)?;
                }
                if let Some(s) = f.bests.get_mut(off + k) {
                    *s = new_t;
                }
            }
        }
        Ok(())
    }

    /// Offer a thread to `node` of the frontier for position `pos`.
    fn insert(&self, st: &mut State<'_, '_>, pos: usize, node: u32, start: usize, caps: &[i32]) -> Result<(), Fail> {
        if node == NONE || idx(node) >= idx(self.prog.nodes) {
            return Ok(());
        }
        if let (false, Some((bs, _))) = (st.run.late, st.best) {
            if start > bs {
                return Ok(());
            }
        }
        st.work += 1;
        if st.run.budget.is_some_and(|b| st.work > b) {
            return Err(Fail::Re(ReError::ESpace));
        }
        // A thread past the only accepted end cannot match (positions only increase); without
        // back-references (no budget to keep equal) it is not kept, so the ring need only cover
        // the characters up to that end.
        if self.prog.brefs == 0 && st.run.end.is_some_and(|e| pos > e) {
            return Ok(());
        }
        if pos - st.cur >= st.r {
            if st.pending_head.is_empty() {
                nomem(st.pending_head.resize(self.subject.len() + 1, NONE))?;
            }
            let Some(head) = st.pending_head.get(pos).copied() else { return Ok(()) };
            let off = st.pending_caps.len();
            nomem(st.pending_caps.extend_from_slice(caps))?;
            let q = u32::try_from(st.pending.len()).map_err(|_| Fail::Re(ReError::ESpace))?;
            nomem(st.pending.push(Pending { node, start, caps: off, next: head }))?;
            if let Some(h) = st.pending_head.get_mut(pos) {
                *h = q;
            }
            st.pending_live += 1;
            return Ok(());
        }
        if self.prog.brefs == 0 {
            return self.insert_plain(st, pos, node, start, caps);
        }
        let stride = st.stride;
        let (pc, _, classes, _) = Self::node_info(st.info, node);
        let run = st.run;
        let indexed = self.prog.brefs != 0;
        // Every thread of a bucket holds texts equal to its key's (`same_key`), so a thread
        // advanced without changing captures keeps the key of the bucket it came from.
        let key = match st.carry {
            Some(k) => {
                debug_assert!(!indexed || k == self.key_hash(st, caps), "regex carried key differs from the captures' key");
                k
            }
            None if indexed => self.key_hash(st, caps),
            None => 0,
        };
        let Some(f) = st.ring.get_mut(pos & (st.r - 1)) else { return Ok(()) };
        f.prepare();
        // The bucket with the same back-referenced texts.
        let mut b = NONE;
        let mut empty = 0usize;
        // The budget bounds TIME, not insertions (DFA-TIMEOUT): one unit for the insertion
        // (charged above), plus one per bucket probed and one per group compared in each class
        // below, so a pattern whose insertions are expensive (many classes and groups, long
        // probe chains) reaches `REG_ESPACE` as fast as one with many cheap insertions.
        let mut cost = u64::try_from(classes.saturating_mul(stride / 2)).unwrap_or(u64::MAX);
        if indexed {
            let len = f.table.len();
            if len > 0 {
                let mut i = Frontier::slot(node, key, len);
                for _ in 0..len {
                    cost += 1;
                    let t = f.table.get(i).copied().unwrap_or(0);
                    if t == 0 {
                        empty = i;
                        break;
                    }
                    if let Some(bk) = bucket_at(&f.buckets, idx(t - 1)) {
                        let rep = f.bests.get(idx(bk.best)).copied().unwrap_or(NONE);
                        if bk.node == node && bk.key == key && self.same_key(caps, f.thread(rep, stride)) {
                            b = t - 1;
                            break;
                        }
                    }
                    i = (i + 1) & (len - 1);
                }
            }
        } else {
            b = f.first_bucket(node);
        }
        st.work = st.work.saturating_add(cost);
        if run.budget.is_some_and(|lim| st.work > lim) {
            return Err(Fail::Re(ReError::ESpace));
        }
        if b == NONE {
            let t = f.add_thread(caps)?;
            let best = u32::try_from(f.bests.len()).map_err(|_| Fail::Re(ReError::ESpace))?;
            for _ in 0..classes {
                f.bests.push(t).map_err(|_| Fail::Grow(Dim::Bests))?;
            }
            let i = idx(node);
            let next = f.first_bucket(node);
            let nb = u32::try_from(f.buckets.len() / 4).map_err(|_| Fail::Re(ReError::ESpace))?;
            let slot = if indexed { f.index(nb, empty) } else { 0 };
            if f.buckets.extend_from_slice(&Bucket { next, start, best, node, key, slot }.pack()).is_err() {
                // Leave no slot naming the bucket that was not stored (the search reruns larger).
                if let Some(t) = f.table.get_mut(idx(slot)) {
                    *t = 0;
                }
                return Err(Fail::Grow(Dim::Buckets));
            }
            if let (Some(g), Some(h)) = (f.node_gen.get_mut(i), f.head.get_mut(i)) {
                *g = f.epoch;
                *h = nb;
            }
            let r = idx(self.prog.rank.get(i).unwrap_or(0));
            if let Some(w) = f.active.get_mut(r / 64) {
                *w |= 1 << (r % 64);
            }
            f.live = true;
            return Ok(());
        }
        let Some(bk) = bucket_at(&f.buckets, idx(b)) else { return Ok(()) };
        if Self::start_better(run, start, bk.start) {
            let t = f.add_thread(caps)?;
            for k in 0..classes {
                if let Some(s) = f.bests.get_mut(idx(bk.best) + k) {
                    *s = t;
                }
            }
            let at = idx(b) * 4;
            if let Some(w) = f.buckets.as_mut_slice().get_mut(at..at + 4) {
                w.copy_from_slice(&Bucket { start, ..bk }.pack());
            }
            return Ok(());
        }
        if Self::start_better(run, bk.start, start) {
            return Ok(());
        }
        let mut new_t = NONE;
        for k in 0..classes {
            let inc = f.bests.get(idx(bk.best) + k).copied().unwrap_or(NONE);
            if self.policy.prefer(self.mask(pc, k), caps, f.thread(inc, stride)) {
                if new_t == NONE {
                    new_t = f.add_thread(caps)?;
                }
                if let Some(s) = f.bests.get_mut(idx(bk.best) + k) {
                    *s = new_t;
                }
            }
        }
        Ok(())
    }

    fn assert_ok(&self, kind: u32, p: usize) -> bool {
        let s = self.subject;
        let len = s.len();
        match kind {
            0 => (p == 0 && !self.ctx.not_bol) || (self.ctx.newline_anchor && p > 0 && s.get(p - 1) == Some(&b'\n')),
            1 => (p == len && !self.ctx.not_eol) || (self.ctx.newline_anchor && p < len && s.get(p) == Some(&b'\n')),
            2 => p == 0,
            3 => p == len,
            _ => {
                let before = text::char_before(self.mode, s, p).is_some_and(|c| text::is_word(self.mode, self.loc, c));
                let after = matches!(text::unit_at(self.mode, s, p), Unit::Char(c, _) if text::is_word(self.mode, self.loc, c));
                match kind {
                    4 => before != after,
                    5 => before == after,
                    6 => !before && after,
                    _ => before && !after,
                }
            }
        }
    }

    /// The length of the subject at `p` that repeats `s[so..eo]`, if it does.
    fn backref_len(&self, so: usize, eo: usize, p: usize) -> Option<usize> {
        let s = self.subject;
        let text = s.get(so..eo)?;
        if !self.prog.icase {
            return s.get(p..)?.starts_with(text).then_some(text.len());
        }
        let (mut i, mut k) = (0usize, p);
        while i < text.len() {
            match (text::unit_at(self.mode, text, i), text::unit_at(self.mode, s, k)) {
                (Unit::Char(a, la), Unit::Char(b, lb)) if text::same_folded(self.mode, self.loc, a, b) => {
                    i += la;
                    k += lb;
                }
                (Unit::Invalid(a), Unit::Invalid(b)) if a == b => {
                    i += 1;
                    k += 1;
                }
                _ => return None,
            }
        }
        Some(k - p)
    }

    fn accept(&self, st: &mut State<'_, '_>, start: usize, p: usize, caps: &[i32]) -> Result<(), Fail> {
        if st.run.end.is_some_and(|e| e != p) {
            return Ok(());
        }
        let better = match st.best {
            None => true,
            Some((bs, be)) => Self::start_better(st.run, start, bs) || (start == bs && (p > be || (p == be && self.policy.prefer(None, caps, st.best_caps.as_slice())))),
        };
        if better {
            st.best = Some((start, p));
            st.best_caps.clear();
            st.best_caps.extend_from_slice(caps)?;
        }
        Ok(())
    }

    /// Advance the threads of node `node` at position `p`.
    fn visit(&self, st: &mut State<'_, '_>, p: usize, node: u32, scratch: &mut Buf<'_, i32>) -> Result<(), Fail> {
        if self.prog.brefs == 0 {
            return self.visit_plain(st, p, node, scratch);
        }
        let (pc, j, classes, _) = Self::node_info(st.info, node);
        let inst = self.prog.inst(pc);
        let stride = st.stride;
        let fi = p & (st.r - 1);
        let mut b = st.ring.get(fi).map_or(NONE, |f| f.first_bucket(node));
        while b != NONE {
            let Some(bk) = st.ring.get(fi).and_then(|f| bucket_at(&f.buckets, idx(b))) else { break };
            // A bucket's best slots are contiguous, one per class ([`Self::insert`]).
            let best = idx(bk.best);
            for k in 0..classes {
                let Some(f) = st.ring.get(fi) else { break };
                let bests = f.bests.as_slice().get(best..best + classes).unwrap_or(&[]);
                let Some(&t) = bests.get(k) else { break };
                // The same thread can be the best of several classes: advance it once.
                if bests.get(..k).is_some_and(|s| s.contains(&t)) {
                    continue;
                }
                // `scratch` holds `stride` elements, as every thread (no library copy per thread).
                copy_caps(scratch.as_mut_slice(), f.thread(t, stride));
                st.carry = Some(bk.key);
                let r = self.advance_brefs(st, p, inst, j, bk.start, scratch.as_mut_slice());
                st.carry = None;
                r?;
            }
            b = bk.next;
        }
        Ok(())
    }

    /// [`Self::visit`] for patterns without back-references (not inlined, as [`Self::insert_plain`]).
    #[inline(never)]
    fn visit_plain(&self, st: &mut State<'_, '_>, p: usize, node: u32, scratch: &mut Buf<'_, i32>) -> Result<(), Fail> {
        let (pc, j, classes, off) = Self::node_info(st.info, node);
        let inst = self.prog.inst(pc);
        let stride = st.stride;
        let i = idx(node);
        let Some(start) = st.ring.get(p & (st.r - 1)).and_then(|f| f.start.get(i).copied()) else { return Ok(()) };
        for k in 0..classes {
            let Some(f) = st.ring.get(p & (st.r - 1)) else { break };
            let bests = f.bests.as_slice().get(off..off + classes).unwrap_or(&[]);
            let Some(&t) = bests.get(k) else { break };
            // The same thread can be the best of several classes: advance it once.
            if bests.get(..k).is_some_and(|b| b.contains(&t)) {
                continue;
            }
            copy_caps(scratch.as_mut_slice(), f.thread(t, stride));
            self.advance_plain(st, p, inst, j, start, scratch.as_mut_slice())?;
        }
        Ok(())
    }

    fn set_cap(caps: &mut [i32], at: usize, v: i32) {
        if let Some(c) = caps.get_mut(at) {
            *c = v;
        }
    }

    /// The successors of node (`pc`, `j`), consuming edges included ([`Self::mark_inline`]).
    fn succs(&self, pc: u32, j: u32) -> [u32; 2] {
        let prog = self.prog;
        let i = prog.inst(pc);
        match i.op {
            OP_BYTE | OP_SET => [prog.node(i.b, 0), NONE],
            OP_BACKREF => [prog.node(i.b, j), prog.node(i.b, 0)],
            OP_OPEN | OP_CLOSE | OP_ASSERT => [prog.node(i.b, j), NONE],
            OP_ENTER => [prog.node(i.b, super::program::enter_j(i, j)), NONE],
            OP_MATCH => [NONE, NONE],
            _ => {
                let mut out = [NONE, NONE];
                for (o, (t, tj)) in out.iter_mut().zip(eps_of(i, j)) {
                    *o = if t == NONE { NONE } else { prog.node(t, tj) };
                }
                out
            }
        }
    }

    /// BACKREF-EPS: mark in word 3 of each node's [`State::info`] whether a thread's expansion
    /// follows it directly ([`Self::reach`]): an epsilon node with one predecessor (the seed counts
    /// as one), or whose own walk is at most [`EPS_JOIN`] hops. Hops are counted in reverse
    /// topological order over the inlined successors, so one expansion is bounded by the nodes of
    /// its tree times `1 + 2 * EPS_JOIN`; every hop is charged to the work budget too.
    fn mark_inline(&self, info: &mut [u32]) {
        let nodes = idx(self.prog.nodes);
        let word = |n: u32| idx(n).checked_mul(INFO).and_then(|a| a.checked_add(3));
        for n in 0..nodes {
            if let Some(w) = info.get_mut(n * INFO + 3) {
                *w = 0;
            }
        }
        // Predecessor counts first.
        let bump = |info: &mut [u32], t: u32| {
            if let Some(w) = word(t).and_then(|a| info.get_mut(a)) {
                *w = w.saturating_add(1);
            }
        };
        bump(info, self.prog.node(self.prog.start, 0));
        for n in 0..nodes {
            let (pc, j) = (info.get(n * INFO).copied().unwrap_or(0), info.get(n * INFO + 1).copied().unwrap_or(0));
            for t in self.succs(pc, j) {
                bump(info, t);
            }
        }
        // Then, from the last rank down, the hops of each node's walk (successors of higher rank
        // already hold theirs; anything else is a bucket, 0 hops).
        for r in (0..nodes).rev() {
            let Some(n) = self.prog.by_rank.get(r) else { continue };
            let Some(at) = word(n) else { continue };
            let (pc, j) = (info.get(at - 3).copied().unwrap_or(0), info.get(at - 2).copied().unwrap_or(0));
            let preds = info.get(at).copied().unwrap_or(0);
            let eps = matches!(self.prog.inst(pc).op, OP_SPLIT | OP_JMP | OP_OPEN | OP_CLOSE | OP_ENTER | OP_EXIT | OP_ASSERT);
            let mut hops = 1u32;
            for t in self.succs(pc, j) {
                let later = t != NONE && self.prog.rank.get(idx(t)).is_some_and(|tr| idx(tr) > r);
                let w = if later { word(t).and_then(|a| info.get(a)).copied().unwrap_or(0) } else { 0 };
                if w & EPS_INLINE != 0 {
                    hops = hops.saturating_add(w & !EPS_INLINE);
                }
            }
            let inline = eps && (preds <= 1 || hops <= EPS_JOIN);
            if let Some(w) = info.get_mut(at) {
                *w = if inline { EPS_INLINE | hops.min(EPS_INLINE - 1) } else { 0 };
            }
        }
    }

    fn inlined(st: &State<'_, '_>, node: u32) -> bool {
        idx(node).checked_mul(INFO).and_then(|a| st.info.get(a + 3)).is_some_and(|w| w & EPS_INLINE != 0)
    }

    /// Offer a thread to `node` at `pos` (patterns with back-references): an inlined epsilon node
    /// is walked now ([`Self::walk`]), anything else gets its bucket ([`Self::insert`]).
    fn reach(&self, st: &mut State<'_, '_>, pos: usize, node: u32, start: usize, caps: &mut [i32], depth: u32) -> Result<(), Fail> {
        if !Self::inlined(st, node) {
            return self.insert(st, pos, node, start, caps);
        }
        let (pc, j, _, _) = Self::node_info(st.info, node);
        self.walk(st, pos, self.prog.inst(pc), j, start, caps, depth)
    }

    /// Follow the epsilon instruction `inst` (chain index `j`) of a thread at `pos`, and its
    /// inlined successors, without buckets; the first non-inlined node reached is inserted. A
    /// split's first branch is walked on a copy of the captures (nested at most [`EPS_DEPTH`]),
    /// its second continues here. Each hop is one unit of work (DFA-TIMEOUT), as an insertion.
    #[allow(clippy::too_many_arguments)] // one thread's state, as `advance_plain`
    fn walk(&self, st: &mut State<'_, '_>, pos: usize, mut inst: super::program::Inst, mut j: u32, start: usize, caps: &mut [i32], depth: u32) -> Result<(), Fail> {
        let prog = self.prog;
        let at_pos = i32::try_from(pos).map_err(|_| Fail::Re(ReError::ESpace))?;
        let mut hops = 0usize;
        loop {
            if let (false, Some((bs, _))) = (st.run.late, st.best) {
                if start > bs {
                    return Ok(());
                }
            }
            st.work += 1;
            if st.run.budget.is_some_and(|b| st.work > b) {
                return Err(Fail::Re(ReError::ESpace));
            }
            let next = match inst.op {
                OP_OPEN | OP_CLOSE => {
                    st.carry = None;
                    if st.run.caps {
                        let at = idx(inst.a.saturating_sub(1)) * 2;
                        if inst.op == OP_OPEN {
                            Self::set_cap(caps, at, at_pos);
                            Self::set_cap(caps, at + 1, -1);
                        } else {
                            Self::set_cap(caps, at + 1, at_pos);
                        }
                    }
                    prog.node(inst.b, j)
                }
                OP_ENTER => {
                    st.carry = None;
                    if st.run.caps && self.policy.resets_on_iteration() {
                        let (lo, hi) = prog.scopes.get(idx(inst.a)).unwrap_or((1, 0));
                        for g in lo..=hi {
                            let at = idx(g.saturating_sub(1)) * 2;
                            Self::set_cap(caps, at, -1);
                            Self::set_cap(caps, at + 1, -1);
                        }
                    }
                    prog.node(inst.b, super::program::enter_j(inst, j))
                }
                OP_ASSERT if !self.assert_ok(inst.a, pos) => return Ok(()),
                OP_ASSERT => prog.node(inst.b, j),
                OP_SPLIT => {
                    let a = if inst.a == NONE { NONE } else { prog.node(inst.a, j) };
                    let mut copy = [0i32; EPS_LOCAL];
                    match copy.get_mut(..caps.len()) {
                        Some(c) if depth < EPS_DEPTH && Self::inlined(st, a) => {
                            copy_caps(c, caps);
                            let carry = st.carry;
                            let r = self.reach(st, pos, a, start, c, depth + 1);
                            st.carry = carry;
                            r?;
                        }
                        _ => self.insert(st, pos, a, start, caps)?,
                    }
                    if inst.b == NONE { NONE } else { prog.node(inst.b, j) }
                }
                OP_JMP | OP_EXIT => {
                    let (t, tj) = eps_of(inst, j).next().unwrap_or((NONE, 0));
                    if t == NONE { NONE } else { prog.node(t, tj) }
                }
                _ => return Ok(()),
            };
            hops += 1;
            // The hop guard only matters if the ranks were not topological (no cycle can loop).
            if !Self::inlined(st, next) || hops > idx(prog.nodes) {
                return self.insert(st, pos, next, start, caps);
            }
            let (pc, nj, _, _) = Self::node_info(st.info, next);
            inst = prog.inst(pc);
            j = nj;
        }
    }

    /// [`Self::advance_plain`] for patterns with back-references (chosen once per search, by
    /// [`Self::visit`]; BACKREF-BREFS-DISPATCH).
    #[allow(clippy::too_many_arguments)] // one thread's state, as `advance_plain`
    fn advance_brefs(&self, st: &mut State<'_, '_>, p: usize, inst: super::program::Inst, j: u32, start: usize, caps: &mut [i32]) -> Result<(), Fail> {
        let prog = self.prog;
        // BACKREF-EPS: consuming steps reach their target's closure directly; epsilon
        // instructions (a bucket that a walk's guard left) walk on.
        match inst.op {
            OP_BYTE => {
                if self.subject.get(p).is_some_and(|b| u32::from(*b) == inst.a) {
                    self.reach(st, p + 1, prog.node(inst.b, 0), start, caps, 0)?;
                }
            }
            OP_SET => {
                if let Unit::Char(c, n) = text::unit_at(self.mode, self.subject, p) {
                    if prog.sets.get(idx(inst.a)).is_some_and(|s| s.matches(self.mode, self.loc, c)) {
                        self.reach(st, p + n, prog.node(inst.b, 0), start, caps, 0)?;
                    }
                }
            }
            OP_BACKREF => {
                let at = idx(inst.a.saturating_sub(1)) * 2;
                let (so, eo) = (caps.get(at).copied().unwrap_or(-1), caps.get(at + 1).copied().unwrap_or(-1));
                if let (Ok(so), Ok(eo)) = (usize::try_from(so), usize::try_from(eo)) {
                    match self.backref_len(so, eo, p) {
                        Some(0) => self.reach(st, p, prog.node(inst.b, j), start, caps, 0)?,
                        Some(n) => self.reach(st, p + n, prog.node(inst.b, 0), start, caps, 0)?,
                        None => {}
                    }
                }
            }
            OP_MATCH => self.accept(st, start, p, caps)?,
            _ => {
                // This node's own unit was charged at its insertion.
                st.work = st.work.saturating_sub(1);
                self.walk(st, p, inst, j, start, caps, 0)?;
            }
        }
        Ok(())
    }

    /// Advance one thread of a pattern without back-references ([`Self::visit_plain`]); the
    /// back-reference variant is [`Self::advance_brefs`], so this pays nothing for it.
    fn advance_plain(&self, st: &mut State<'_, '_>, p: usize, inst: super::program::Inst, j: u32, start: usize, caps: &mut [i32]) -> Result<(), Fail> {
        let prog = self.prog;
        let pos = i32::try_from(p).map_err(|_| Fail::Re(ReError::ESpace))?;
        match inst.op {
            OP_BYTE => {
                if self.subject.get(p).is_some_and(|b| u32::from(*b) == inst.a) {
                    self.insert(st, p + 1, prog.node(inst.b, 0), start, caps)?;
                }
            }
            OP_SET => {
                if let Unit::Char(c, n) = text::unit_at(self.mode, self.subject, p) {
                    if prog.sets.get(idx(inst.a)).is_some_and(|s| s.matches(self.mode, self.loc, c)) {
                        self.insert(st, p + n, prog.node(inst.b, 0), start, caps)?;
                    }
                }
            }
            OP_OPEN | OP_CLOSE => {
                st.carry = None;
                if st.run.caps {
                    let at = idx(inst.a.saturating_sub(1)) * 2;
                    if inst.op == OP_OPEN {
                        Self::set_cap(caps, at, pos);
                        Self::set_cap(caps, at + 1, -1);
                    } else {
                        Self::set_cap(caps, at + 1, pos);
                    }
                }
                self.insert(st, p, prog.node(inst.b, j), start, caps)?;
            }
            OP_ENTER => {
                st.carry = None;
                if st.run.caps && self.policy.resets_on_iteration() {
                    let (lo, hi) = prog.scopes.get(idx(inst.a)).unwrap_or((1, 0));
                    for g in lo..=hi {
                        let at = idx(g.saturating_sub(1)) * 2;
                        Self::set_cap(caps, at, -1);
                        Self::set_cap(caps, at + 1, -1);
                    }
                }
                self.insert(st, p, prog.node(inst.b, super::program::enter_j(inst, j)), start, caps)?;
            }
            OP_ASSERT => {
                if self.assert_ok(inst.a, p) {
                    self.insert(st, p, prog.node(inst.b, j), start, caps)?;
                }
            }
            OP_BACKREF => {
                let at = idx(inst.a.saturating_sub(1)) * 2;
                let (so, eo) = (caps.get(at).copied().unwrap_or(-1), caps.get(at + 1).copied().unwrap_or(-1));
                if let (Ok(so), Ok(eo)) = (usize::try_from(so), usize::try_from(eo)) {
                    match self.backref_len(so, eo, p) {
                        Some(0) => self.insert(st, p, prog.node(inst.b, j), start, caps)?,
                        Some(n) => self.insert(st, p + n, prog.node(inst.b, 0), start, caps)?,
                        None => {}
                    }
                }
            }
            OP_MATCH => self.accept(st, start, p, caps)?,
            _ => {
                for (t, tj) in eps_of(inst, j) {
                    if t != NONE {
                        self.insert(st, p, prog.node(t, tj), start, caps)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Process every state of position `p`, in topological order.
    fn step(&self, st: &mut State<'_, '_>, p: usize, scratch: &mut Buf<'_, i32>) -> Result<(), Fail> {
        let words = st.ring.get(p & (st.r - 1)).map_or(0, |f| f.active.len());
        let mut w = 0usize;
        while w < words {
            let word = st.ring.get(p & (st.r - 1)).and_then(|f| f.active.get(w).copied()).unwrap_or(0);
            if word == 0 {
                w += 1;
                continue;
            }
            let bit = word.trailing_zeros();
            if let Some(a) = st.ring.get_mut(p & (st.r - 1)).and_then(|f| f.active.get_mut(w)) {
                *a &= !(1u64 << bit);
            }
            let rank = w * 64 + idx(bit);
            let node = self.prog.by_rank.get(rank).unwrap_or(NONE);
            if node != NONE {
                self.visit(st, p, node, scratch)?;
            }
        }
        Ok(())
    }

    /// Run the search.
    fn run<'s>(&self, run: &Run, room: &Room, pools: &mut Pools<'s>) -> Result<Option<Found<'s>>, Fail> {
        let len = self.subject.len();
        if run.first_start > len || run.first_start > run.last_start {
            return Ok(None);
        }
        let ng = if run.caps { idx(self.prog.ngroups) } else { 0 };
        let indexed = self.prog.brefs != 0;
        let stride = stride_of(ng, indexed);
        let nodes = idx(self.prog.nodes);
        let mut r = room.ring.clamp(1, RING).next_power_of_two();
        // Without back-references nothing past the accepted end is kept ([`Self::insert`]): a
        // span of single-byte characters needs two frontiers, whatever the ring's room.
        if !indexed && r > 2 && self.subject.get(run.first_start..run.end.unwrap_or(len)).is_some_and(|s| s.is_ascii()) {
            r = 2;
        }
        // Per node: instruction, chain index, best slots, their offset ([`State::info`]).
        let mut classes = 0usize;
        // With back-references too (BACKREF-LONG): every insertion and visit reads its node's
        // classes here instead of walking the program's mask chains.
        let mut info = pools.u32s.take(nodes * INFO)?;
        for n in 0..nodes {
            let (pc, j) = self.prog.owner.get(n).unwrap_or((0, 0));
            let k = self.classes(run, pc);
            let w = [pc, j, u32::try_from(k).unwrap_or(0), u32::try_from(classes).unwrap_or(0)];
            info.extend_with(INFO, |d| d.copy_from_slice(&w))?;
            classes += k;
        }
        if indexed {
            self.mark_inline(info.as_mut_slice());
        }
        let mut ring: [Frontier<'s>; RING] = Default::default();
        for f in ring.iter_mut().take(r) {
            *f = Frontier::new(pools, room, nodes, stride, indexed, classes)?;
        }
        let mut unset = pools.i32s.take(stride)?;
        unset.fill(stride, -1)?;
        let mut scratch = pools.i32s.take(stride)?;
        scratch.expose(stride)?;
        let best_caps = pools.i32s.take(stride)?;
        // Patterns with back-references on subjects longer than [`DIRECT_HASH`] only: heap
        // buffers sized by the subject (a short subject hashes each text directly, BACKREF-ARCH).
        let (mut prefix, mut pows) = (TryVec::new(), TryVec::new());
        if self.prog.brefs != 0 && len > DIRECT_HASH {
            const BASE: u64 = HASH_BASE;
            nomem(prefix.reserve(len + 1))?;
            nomem(pows.reserve(len + 1))?;
            let (mut h, mut w) = (0u64, 1u64);
            nomem(prefix.push(0))?;
            nomem(pows.push(1))?;
            for &c in self.subject {
                h = h.wrapping_mul(BASE).wrapping_add(u64::from(c) + 1);
                w = w.wrapping_mul(BASE);
                nomem(prefix.push(h))?;
                nomem(pows.push(w))?;
            }
        }
        let mut st = State {
            run,
            r,
            best_caps,
            stride,
            cur: run.first_start,
            ring,
            pending: TryVec::new(),
            pending_caps: TryVec::new(),
            pending_head: TryVec::new(),
            pending_live: 0,
            best: None,
            work: 0,
            carry: None,
            prefix,
            pows,
            info: info.into_slice(),
        };
        let mut arrivals: TryVec<u32> = TryVec::new();
        let mut p = run.first_start;
        let mut walk = || -> Result<(), Fail> { loop {
            // With no thread alive, skip to the next byte a match can begin with.
            if !self.prog.empty_ok && st.pending_live == 0 && !st.ring.iter().take(st.r).any(|f| f.live) {
                let limit = run.last_start.saturating_add(1).min(len);
                match self.subject.get(p..limit).and_then(|r| self.prog.first_start(r)) {
                    Some(k) => p += k,
                    None => break,
                }
            }
            st.cur = p;
            // Threads that back-references sent to this position.
            // In the order they were sent (the list holds the newest first).
            arrivals.clear();
            let mut q = st.pending_head.get(p).copied().unwrap_or(NONE);
            if let Some(h) = st.pending_head.get_mut(p) {
                *h = NONE;
            }
            while q != NONE {
                nomem(arrivals.push(q))?;
                q = st.pending.get(idx(q)).map_or(NONE, |e| e.next);
            }
            for &q in arrivals.iter().rev() {
                let Some(e) = st.pending.get(idx(q)) else { continue };
                let (node, start, at) = (e.node, e.start, e.caps);
                scratch.clear();
                scratch.extend_from_slice(st.pending_caps.get(at..at + stride).unwrap_or(&[]))?;
                st.pending_live -= 1;
                self.insert(&mut st, p, node, start, scratch.as_slice())?;
            }
            if st.pending_live == 0 {
                st.pending.clear();
                st.pending_caps.clear();
            }
            // A seed at a byte no match begins with dies at its first character test, after
            // several insertions (BACKREF-COST): seed only where a match can begin.
            let can_begin = self.prog.empty_ok || self.subject.get(p).is_some_and(|&b| self.prog.can_start_with(b));
            if p <= run.last_start && (run.late || st.best.is_none()) && can_begin {
                let seed = self.prog.node(self.prog.start, 0);
                if indexed {
                    scratch.clear();
                    scratch.extend_from_slice(unset.as_slice())?;
                    self.reach(&mut st, p, seed, p, scratch.as_mut_slice(), 0)?;
                } else {
                    self.insert(&mut st, p, seed, p, unset.as_slice())?;
                }
            }
            self.step(&mut st, p, &mut scratch)?;
            if let Some(f) = st.ring.get_mut(p & (st.r - 1)) {
                f.clear();
            }
            if run.end == Some(p) || p >= len {
                break;
            }
            let ahead = st.ring.iter().take(st.r).any(|f| f.live) || st.pending_live > 0;
            let more_seeds = p < run.last_start && (run.late || st.best.is_none());
            if !ahead && !more_seeds {
                break;
            }
            p += 1;
        }
        Ok(()) };
        if let Err(e) = walk() {
            return Err(match e {
                Fail::Grow(d) => Fail::Grow(st.widen(d)),
                e => e,
            });
        }
        let State { best, best_caps, .. } = st;
        Ok(best.map(|(start, end)| Found { start, end, caps: best_caps.into_slice().get(..ng * 2).unwrap_or(&[]) }))
    }
}

/// The epsilon successors of an instruction other than those `advance_plain` handles itself.
pub(super) fn eps_of(inst: super::program::Inst, j: u32) -> impl Iterator<Item = (u32, u32)> {
    let none = (NONE, 0);
    let pair = match inst.op {
        super::program::OP_SPLIT => [(inst.a, j), (inst.b, j)],
        super::program::OP_JMP => [(inst.a, j), none],
        super::program::OP_EXIT if j >= 1 => [(inst.b, super::program::exit_j(inst, j)), none],
        super::program::OP_EXIT => [(inst.c, 0), none],
        _ => [none, none],
    };
    pair.into_iter()
}

/// The state of one search without subexpressions or back-references ([`Matcher::run_fast`]):
/// per frontier of the ring and per node, the best start of the thread there (valid when the
/// node's stamp is the frontier's epoch), and the nodes holding threads as bits by rank.
struct Fast<'r, 's> {
    run: &'r Run,
    nodes: usize,
    words: usize,
    /// Frontiers in the ring, and the position being processed.
    r: usize,
    cur: usize,
    /// A thread was sent beyond the ring (a character longer than the ring assumed).
    beyond: bool,
    start: &'s mut [usize],
    stamp: &'s mut [u32],
    epoch: [u32; RING],
    active: &'s mut [u64],
    count: [u32; RING],
    best: Option<(usize, usize)>,
}

impl Fast<'_, '_> {
    fn live(&self) -> bool {
        self.count.iter().any(|&c| c != 0)
    }

    /// Empty frontier `f` (a new epoch; its active bits are already clear after its step).
    fn clear(&mut self, f: usize) {
        if let Some(e) = self.epoch.get_mut(f) {
            *e = e.wrapping_add(1);
            if *e == 0 {
                let base = f * self.nodes;
                if let Some(s) = self.stamp.get_mut(base..base + self.nodes) {
                    s.iter_mut().for_each(|x| *x = 0);
                }
                *e = 1;
            }
        }
        if let Some(c) = self.count.get_mut(f) {
            *c = 0;
        }
        let base = f * self.words;
        if let Some(a) = self.active.get_mut(base..base + self.words) {
            a.iter_mut().for_each(|w| *w = 0);
        }
    }
}

impl State<'_, '_> {
    /// The parts to grow after `d` overflowed: `d`, and each of threads, buckets and best slots
    /// that some frontier has half filled. They tend to fill together (a pattern with
    /// back-references needed 29 reruns growing one part at a time, and timed out in the fuzzer);
    /// growing only the parts in use keeps the memory near the need (growing all of them ran a
    /// pattern with many states out of memory).
    fn widen(&self, d: Dim) -> Dim {
        let (mut threads, mut buckets, mut bests) = match d {
            Dim::Threads => (true, false, false),
            Dim::Buckets => (false, true, false),
            Dim::Bests => (false, false, true),
            other => return other,
        };
        let half = |b: usize, cap: usize| cap != 0 && b.saturating_mul(2) >= cap;
        for f in self.ring.iter().take(self.r) {
            threads |= half(f.caps.len(), f.caps.capacity());
            buckets |= half(f.buckets.len(), f.buckets.capacity());
            bests |= half(f.bests.len(), f.bests.capacity());
        }
        Dim::Many { threads, buckets, bests }
    }
}

impl<P: Policy> Matcher<'_, P> {
    /// Offer a thread to `node` at position `pos` (fast search): kept if the node has none
    /// there, or replacing one with a worse start.
    fn fast_insert(&self, st: &mut Fast<'_, '_>, pos: usize, node: u32, start: usize) {
        let n = idx(node);
        if node == NONE || n >= st.nodes {
            return;
        }
        if let (false, Some((bs, _))) = (st.run.late, st.best) {
            if start > bs {
                return;
            }
        }
        if pos - st.cur >= st.r && st.r < RING {
            st.beyond = true;
            return;
        }
        let f = pos & (st.r - 1);
        let at = f * st.nodes + n;
        let epoch = st.epoch.get(f).copied().unwrap_or(0);
        let seen = st.stamp.get(at).copied() == Some(epoch);
        if seen {
            let old = st.start.get(at).copied().unwrap_or(start);
            if Self::start_better(st.run, start, old) {
                if let Some(s) = st.start.get_mut(at) {
                    *s = start;
                }
            }
            return;
        }
        if let (Some(s), Some(t)) = (st.start.get_mut(at), st.stamp.get_mut(at)) {
            *s = start;
            *t = epoch;
        }
        let r = idx(self.prog.rank.get(n).unwrap_or(0));
        if let Some(w) = st.active.get_mut(f * st.words + r / 64) {
            *w |= 1 << (r % 64);
        }
        if let Some(c) = st.count.get_mut(f) {
            *c += 1;
        }
    }

    /// Advance the thread at `node`, position `p`, started at `start` (fast search).
    fn fast_advance(&self, st: &mut Fast<'_, '_>, p: usize, node: u32, start: usize) {
        let prog = self.prog;
        let (pc, j) = prog.owner.get(idx(node)).unwrap_or((0, 0));
        let inst = prog.inst(pc);
        match inst.op {
            OP_BYTE => {
                if self.subject.get(p).is_some_and(|b| u32::from(*b) == inst.a) {
                    self.fast_insert(st, p + 1, prog.node(inst.b, 0), start);
                }
            }
            OP_SET => {
                if let Unit::Char(c, n) = text::unit_at(self.mode, self.subject, p) {
                    if prog.sets.get(idx(inst.a)).is_some_and(|s| s.matches(self.mode, self.loc, c)) {
                        self.fast_insert(st, p + n, prog.node(inst.b, 0), start);
                    }
                }
            }
            OP_OPEN | OP_CLOSE | OP_ASSERT if inst.op != OP_ASSERT || self.assert_ok(inst.a, p) => self.fast_insert(st, p, prog.node(inst.b, j), start),
            OP_ASSERT => {}
            OP_ENTER => self.fast_insert(st, p, prog.node(inst.b, super::program::enter_j(inst, j)), start),
            OP_MATCH => {
                if st.run.end.is_none_or(|e| e == p) {
                    let better = match st.best {
                        None => true,
                        Some((bs, be)) => Self::start_better(st.run, start, bs) || (start == bs && p > be),
                    };
                    if better {
                        st.best = Some((start, p));
                    }
                }
            }
            _ => {
                for (t, tj) in eps_of(inst, j) {
                    if t != NONE {
                        self.fast_insert(st, p, prog.node(t, tj), start);
                    }
                }
            }
        }
    }

    /// The overall match only, for a pattern without back-references: [`Matcher::run`]'s
    /// search with one thread per state (the best start; with no subexpressions to compare,
    /// nothing else tells threads apart), kept in flat arrays instead of buckets and capture
    /// vectors. The same positions are visited and the same threads kept, so the answer is
    /// [`Matcher::run`]'s. Memory: two words per state per ring position (16), for this call.
    fn run_fast(&self, run: &Run, room: &Room, pools: &mut Pools<'_>) -> Result<Option<(usize, usize)>, Fail> {
        let len = self.subject.len();
        if run.first_start > len || run.first_start > run.last_start {
            return Ok(None);
        }
        let nodes = idx(self.prog.nodes);
        let words = nodes.div_ceil(64).max(1);
        let r = room.ring.clamp(1, RING).next_power_of_two();
        let start = pools.usizes.take_filled(nodes * r, 0)?;
        let stamp = pools.u32s.take_filled(nodes * r, 0)?;
        let active = pools.u64s.take_filled(words * r, 0)?;
        let mut st = Fast { run, nodes, words, r, cur: run.first_start, beyond: false, start, stamp, epoch: [1; RING], active, count: [0; RING], best: None };
        let mut p = run.first_start;
        loop {
            if !self.prog.empty_ok && !st.live() {
                let limit = run.last_start.saturating_add(1).min(len);
                match self.subject.get(p..limit).and_then(|r| self.prog.first_start(r)) {
                    Some(k) => p += k,
                    None => break,
                }
            }
            st.cur = p;
            if p <= run.last_start && (run.late || st.best.is_none()) {
                self.fast_insert(&mut st, p, self.prog.node(self.prog.start, 0), p);
            }
            // Every state of position `p`, in topological order (new bits are higher).
            let f = p & (st.r - 1);
            let mut w = 0usize;
            while w < st.words {
                let at = f * st.words + w;
                let word = st.active.get(at).copied().unwrap_or(0);
                if word == 0 {
                    w += 1;
                    continue;
                }
                let bit = word.trailing_zeros();
                if let Some(a) = st.active.get_mut(at) {
                    *a &= !(1u64 << bit);
                }
                let node = self.prog.by_rank.get(w * 64 + idx(bit)).unwrap_or(NONE);
                if node != NONE {
                    let s = st.start.get(f * st.nodes + idx(node)).copied().unwrap_or(p);
                    self.fast_advance(&mut st, p, node, s);
                }
            }
            st.clear(f);
            if run.end == Some(p) || p >= len {
                break;
            }
            let more_seeds = p < run.last_start && (run.late || st.best.is_none());
            if !st.live() && !more_seeds {
                break;
            }
            p += 1;
        }
        if st.beyond {
            return Err(Fail::Grow(Dim::Ring));
        }
        Ok(st.best)
    }
}

impl<P: Policy> Matcher<'_, P> {
    /// The room a search starts with. Without back-references a frontier holds at most one
    /// bucket per state and one best thread per state and class (`classes`, the sum over the
    /// states); each insertion makes at most one thread, and a frontier receives the seed, at
    /// most two epsilon insertions per thread visited there and one per thread visited at the
    /// position before: `3 * classes + 1` threads when characters are single bytes (longer
    /// characters, or back-references, can need more: the search then runs again).
    fn room<const BR: bool>(&self, fast_only: bool) -> Room {
        #[cfg(test)]
        if test_room::TINY.with(core::cell::Cell::get) {
            return Room { ring: 1, threads: 1, buckets: 1, bests: 1 };
        }
        if fast_only {
            // Only the fast pass runs (no subexpressions, no back-references): it needs the
            // ring alone, so the per-state sums below are not paid on every REG_NOSUB call.
            let ring = if matches!(self.mode, Mode::Bytes) { 2 } else { 8 };
            return Room { ring, threads: 0, buckets: 0, bests: 0 };
        }
        let nodes = idx(self.prog.nodes);
        let (mut classes, mut widest) = (0usize, 1usize);
        for n in 0..nodes {
            let (pc, _) = self.prog.owner.get(n).unwrap_or((0, 0));
            let k = self.prog.masks_of(pc).len() + 1;
            classes += k;
            widest = widest.max(k);
        }
        let ring = if BR {
            RING
        } else if matches!(self.mode, Mode::Bytes) {
            2
        } else {
            // A UTF-8 character has at most four bytes; a longer one (other encodings) waits on
            // the pending list or reruns the fast search with the full ring. A power of two: the
            // frontier of a position is a mask, not a division, away.
            8
        };
        let buckets = if BR { nodes * 2 } else { nodes };
        let bests = if BR { buckets * widest } else { classes };
        // Capped, so that a huge program (stacked repetitions) does not reserve its worst case
        // up front: a search that needs more grows the part it needs ([`Dim`]).
        Room { ring, threads: (3 * classes + 1).min(ROOM_CAP), buckets: buckets.min(ROOM_CAP), bests: bests.min(ROOM_CAP) }
    }

    /// The pools' sizes for a search with `room`.
    fn sizes<const BR: bool>(&self, room: &Room, caps: bool) -> Sizes {
        let nodes = idx(self.prog.nodes);
        let r = room.ring.clamp(1, RING).next_power_of_two();
        let fast = Sizes { u32s: nodes * r, u64s: nodes.div_ceil(64).max(1) * r, usizes: nodes * r, ..Sizes::default() };
        let stride = stride_of(idx(self.prog.ngroups), BR);
        let full = if BR {
            Sizes { u32s: r * (2 * nodes + room.bests + table_len(room.buckets)) + INFO * nodes, u64s: r * nodes.div_ceil(64), i32s: r * room.threads * stride + 3 * stride, usizes: 0, buckets: r * room.buckets }
        } else {
            // One bucket per node: its start (`usizes`) and best slots; plus the node table.
            Sizes { u32s: r * (nodes + room.bests) + INFO * nodes, u64s: r * nodes.div_ceil(64), i32s: r * room.threads * stride + 3 * stride, usizes: r * nodes, buckets: 0 }
        };
        match (BR, caps) {
            (true, _) => full,
            (false, true) => fast.max(full),
            (false, false) => fast,
        }
    }

    /// The search of `regexec` and `re_search`: the match that starts in `base`'s window and,
    /// with `caps`, its captures (handed to `out`, `so` and `eo` per group). All the scratch is
    /// this call's own: on the stack when the program is small ([`Small`]), else in `src`'s blocks (no allocation once they are large enough), on
    /// the heap otherwise, and larger while a search needs more room ([`Grow`]).
    pub(crate) fn search(&self, base: &Run, caps: bool, budget: u64, src: &mut dyn super::ScratchSource, out: &mut dyn FnMut(&[i32]) -> Result<(), ReError>) -> Result<Option<(usize, usize)>, ReError> {
        // Decided once per search: `room` and `sizes` are compiled for each kind of pattern
        // (BACKREF-BREFS-DISPATCH), so a pattern without back-references does not test for them.
        if self.prog.has_backrefs() {
            self.search_as::<true>(base, caps, budget, src, out)
        } else {
            self.search_as::<false>(base, caps, budget, src, out)
        }
    }

    /// [`Self::search`] for a pattern that has (`BR`) or has not back-references.
    fn search_as<const BR: bool>(&self, base: &Run, caps: bool, budget: u64, src: &mut dyn super::ScratchSource, out: &mut dyn FnMut(&[i32]) -> Result<(), ReError>) -> Result<Option<(usize, usize)>, ReError> {
        let backrefs = BR;
        let mut room = self.room::<BR>(!caps && !backrefs);
        loop {
            let need = self.sizes::<BR>(&room, caps);
            let r = if need.within(&Tiny::CAP) {
                self.attempt_on::<64, 4, 0, 64, 0>(base, caps, budget, backrefs, &room, out)
            } else if need.within(&Small::CAP) {
                self.attempt_on::<256, 16, 128, 192, 64>(base, caps, budget, backrefs, &room, out)
            } else if let Some(mut mem) = src.scratch(&need.external()).and_then(|m| Mem::external(m, &need)) {
                self.attempt(base, caps, budget, backrefs, &room, &mut mem, out)
            } else {
                let mut heap = Heap::new(&need)?;
                self.attempt(base, caps, budget, backrefs, &room, &mut heap.mem(), out)
            };
            match r {
                Ok(v) => return Ok(v),
                Err(Fail::Re(e)) => return Err(e),
                Err(Fail::Grow(d)) => {
                    #[cfg(test)]
                    test_room::GROWN.with(|g| g.set(g.get() + 1));
                    room = room.grown(d);
                }
            }
        }
    }

    /// [`Matcher::attempt`] in stack storage of these sizes ([`Tiny`], [`Small`]); not inlined,
    /// so that the storage is in this frame only, while the search runs in it.
    #[inline(never)]
    #[allow(clippy::too_many_arguments)] // one search's inputs, passed through from `search`
    fn attempt_on<const A: usize, const B: usize, const C: usize, const D: usize, const E: usize>(&self, base: &Run, caps: bool, budget: u64, backrefs: bool, room: &Room, out: &mut dyn FnMut(&[i32]) -> Result<(), ReError>) -> Result<Option<(usize, usize)>, Fail> {
        let mut storage = Inline::<A, B, C, D, E>::new();
        self.attempt(base, caps, budget, backrefs, room, &mut storage.mem(), out)
    }

    #[allow(clippy::too_many_arguments)] // one search's inputs, passed through from `search`
    fn attempt(&self, base: &Run, caps: bool, budget: u64, backrefs: bool, room: &Room, mem: &mut Mem<'_>, out: &mut dyn FnMut(&[i32]) -> Result<(), ReError>) -> Result<Option<(usize, usize)>, Fail> {
        let run = if backrefs {
            #[cfg(feature = "selftest")]
            super::paths::hit(super::paths::Path::Backref);
            Run { caps: true, budget: Some(budget.max(1)), ..*base }
        } else if base.caps {
            // The caller found the span already (the lazy DFA, D-14.16): no overall pass. The
            // one-pass walk collects the subexpressions on it when the program allows (D-14.15
            // amendment RX-ONEPASS; RE-SEARCH: the merge of both left this arm without it, so
            // every regexec/re_search with a DFA span ran the thread-set pass).
            if let (true, true, Some(end)) = (caps, self.prog.one_pass, base.end) {
                if base.first_start == base.last_start {
                    if let Some(f) = self.one_pass(base.first_start, end, &mut mem.pools())? {
                        #[cfg(test)]
                        super::dfa::counters::bump(&super::dfa::counters::SPAN_ONE_PASS);
                        #[cfg(feature = "selftest")]
                        super::paths::hit(super::paths::Path::SpanOnePass);
                        out(f.caps)?;
                        return Ok(Some((f.start, f.end)));
                    }
                }
            }
            #[cfg(feature = "selftest")]
            super::paths::hit(super::paths::Path::ThreadSet);
            *base
        } else {
            let Some((start, end)) = self.run_fast(base, room, &mut mem.pools())? else { return Ok(None) };
            if !caps {
                #[cfg(feature = "selftest")]
                super::paths::hit(super::paths::Path::FastOnly);
                return Ok(Some((start, end)));
            }
            if self.prog.one_pass {
                if let Some(f) = self.one_pass(start, end, &mut mem.pools())? {
                    #[cfg(feature = "selftest")]
                    super::paths::hit(super::paths::Path::FastOnePass);
                    out(f.caps)?;
                    return Ok(Some((f.start, f.end)));
                }
            }
            #[cfg(feature = "selftest")]
            super::paths::hit(super::paths::Path::ThreadSet);
            Run { caps: true, first_start: start, last_start: start, late: false, end: Some(end), budget: base.budget }
        };
        let mut pools = mem.pools();
        let Some(f) = self.run(&run, room, &mut pools)? else { return Ok(None) };
        if caps {
            out(f.caps)?;
        }
        Ok(Some((f.start, f.end)))
    }
}


/// One position of the one-pass walk ([`Matcher::one_pass_step`]).
enum OneStep {
    /// The path consumed a character of this length; the walk continues at this instruction.
    Next(u32, usize),
    /// The path reached `Match` at the match's end.
    Done,
    /// No path, or a bound was reached: the thread-set pass decides.
    Fail,
}

/// Capture effects in the one-pass walk's log: the kind in the top two bits, the group or scope
/// below.
const LOG_OPEN: u32 = 1 << 30;
const LOG_CLOSE: u32 = 2 << 30;
const LOG_ENTER: u32 = 3 << 30;
const LOG_ARG: u32 = (1 << 30) - 1;

impl<P: Policy> Matcher<'_, P> {
    /// The captures of the match `[s, e]` of a one-pass program (D-14.15, regcomp's
    /// `one_pass_check`): at most one path consumes each character and at most one accepts, so
    /// the walk follows the only path from `s` to `Match` at `e` and records its capture
    /// effects, with no thread set. `None` (no such path, or a bound reached: a damaged
    /// `regex_t`) leaves the search to [`Self::run`].
    fn one_pass<'s>(&self, s: usize, e: usize, pools: &mut Pools<'s>) -> Result<Option<Found<'s>>, Fail> {
        let ng = idx(self.prog.ngroups);
        let nodes = idx(self.prog.nodes);
        let mut caps = pools.i32s.take(ng * 2)?;
        caps.fill(ng * 2, -1)?;
        let mut stack = pools.u32s.take(3 * nodes)?;
        stack.expose(3 * nodes)?;
        let mut log = pools.u32s.take(nodes)?;
        log.expose(nodes)?;
        let resets = self.policy.resets_on_iteration();
        let (mut pc, mut p) = (self.prog.start, s);
        loop {
            let (step, n) = self.one_pass_step(pc, p, e, stack.as_mut_slice(), log.as_mut_slice());
            if matches!(step, OneStep::Fail) {
                return Ok(None);
            }
            let v = i32::try_from(p).map_err(|_| Fail::Re(ReError::ESpace))?;
            let c = caps.as_mut_slice();
            for &w in log.as_slice().get(..n).unwrap_or(&[]) {
                let a = w & LOG_ARG;
                match w & !LOG_ARG {
                    LOG_OPEN => {
                        let at = idx(a.saturating_sub(1)) * 2;
                        Self::set_cap(c, at, v);
                        Self::set_cap(c, at + 1, -1);
                    }
                    LOG_CLOSE => Self::set_cap(c, idx(a.saturating_sub(1)) * 2 + 1, v),
                    _ => {
                        if resets {
                            let (lo, hi) = self.prog.scopes.get(idx(a)).unwrap_or((1, 0));
                            for g in lo..=hi {
                                let at = idx(g.saturating_sub(1)) * 2;
                                Self::set_cap(c, at, -1);
                                Self::set_cap(c, at + 1, -1);
                            }
                        }
                    }
                }
            }
            match step {
                OneStep::Next(t, len) => {
                    pc = t;
                    p += len;
                }
                _ => break,
            }
        }
        Ok(Some(Found { start: s, end: e, caps: caps.into_slice() }))
    }

    /// From instruction `entry` (`j` 0) at position `p`, find the path that consumes the
    /// character at `p` (`p < e`) or reaches `Match` (`p == e`): a depth-first walk of the
    /// epsilon edges, the second branch of each split on `stack` (instruction, `j`, log length)
    /// and the capture effects of the current path in `log` (its length is returned). In a
    /// one-pass program the paths form a tree and at most one of them succeeds, so the first one
    /// found is the only one. At most `stack.len()` instructions are taken per position: a
    /// damaged program cannot loop.
    fn one_pass_step(&self, entry: u32, p: usize, e: usize, stack: &mut [u32], log: &mut [u32]) -> (OneStep, usize) {
        let prog = self.prog;
        let (mut pc, mut j, mut sp, mut lp) = (entry, 0u32, 0usize, 0usize);
        let mut budget = stack.len();
        loop {
            if pc != NONE && budget > 0 {
                budget -= 1;
                let inst = prog.inst(pc);
                pc = match inst.op {
                    OP_BYTE => {
                        if p < e && self.subject.get(p).is_some_and(|b| u32::from(*b) == inst.a) {
                            return (OneStep::Next(inst.b, 1), lp);
                        }
                        NONE
                    }
                    OP_SET => {
                        if p < e {
                            if let Unit::Char(c, n) = text::unit_at(self.mode, self.subject, p) {
                                if p + n <= e && prog.sets.get(idx(inst.a)).is_some_and(|s| s.matches(self.mode, self.loc, c)) {
                                    return (OneStep::Next(inst.b, n), lp);
                                }
                            }
                        }
                        NONE
                    }
                    OP_MATCH => {
                        if p == e {
                            return (OneStep::Done, lp);
                        }
                        NONE
                    }
                    OP_OPEN | OP_CLOSE | OP_ENTER => {
                        let tag = match inst.op {
                            OP_OPEN => LOG_OPEN,
                            OP_CLOSE => LOG_CLOSE,
                            _ => LOG_ENTER,
                        };
                        let Some(slot) = log.get_mut(lp).filter(|_| inst.a <= LOG_ARG) else { return (OneStep::Fail, 0) };
                        *slot = tag | inst.a;
                        lp += 1;
                        if inst.op == OP_ENTER {
                            j = super::program::enter_j(inst, j);
                        }
                        inst.b
                    }
                    OP_ASSERT => {
                        if self.assert_ok(inst.a, p) { inst.b } else { NONE }
                    }
                    OP_BACKREF => return (OneStep::Fail, 0),
                    _ => {
                        let mut succ = eps_of(inst, j);
                        let (t, tj) = succ.next().unwrap_or((NONE, 0));
                        if let Some((u, uj)) = succ.next().filter(|&(u, _)| u != NONE) {
                            let Some(w) = stack.get_mut(sp..sp + 3) else { return (OneStep::Fail, 0) };
                            w.copy_from_slice(&[u, uj, u32::try_from(lp).unwrap_or(0)]);
                            sp += 3;
                        }
                        j = tj;
                        t
                    }
                };
                continue;
            }
            // A dead end: the latest branch not yet taken.
            let Some(&[u, uj, m]) = sp.checked_sub(3).and_then(|b| stack.get(b..sp)) else { return (OneStep::Fail, 0) };
            sp -= 3;
            (pc, j, lp) = (u, uj, idx(m));
        }
    }
}

#[cfg(test)]
impl<P: Policy> Matcher<'_, P> {
    /// Whether this program's searches start in the inline scratch (no allocation), and the
    /// sizes they need.
    pub(crate) fn inline_need(&self, caps: bool) -> (bool, std::string::String) {
        let backrefs = self.prog.has_backrefs();
        let n = if backrefs {
            self.sizes::<true>(&self.room::<true>(false), caps)
        } else {
            self.sizes::<false>(&self.room::<false>(!caps), caps)
        };
        (n.fits_inline(), std::format!("{n:?}"))
    }
}

/// Tests: start every search with the least room (one frontier, one thread, bucket and best
/// slot), so that every search takes the pending list and the [`Grow`] reruns; and count the
/// reruns.
#[cfg(test)]
pub(crate) mod test_room {
    std::thread_local! {
        pub(crate) static TINY: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
        pub(crate) static GROWN: core::cell::Cell<u32> = const { core::cell::Cell::new(0) };
    }
}
