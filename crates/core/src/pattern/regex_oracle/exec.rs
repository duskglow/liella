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
use super::program::{Program, NONE, OP_ASSERT, OP_BACKREF, OP_BYTE, OP_CLOSE, OP_ENTER, OP_MATCH, OP_OPEN, OP_SET};
use super::text::{self, Mode, Unit};
use super::ReError;

/// Positions ahead that the ring of frontiers covers (a character is shorter than this).
const RING: usize = 16;

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
/// without `Run::caps`).
#[derive(Debug)]
pub(crate) struct Found {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) caps: TryVec<i32>,
}

#[derive(Clone, Copy, Debug, Default)]
struct Bucket {
    next: u32,
    start: usize,
    /// Offset of the bucket's per-class best threads in `Frontier::bests`.
    best: u32,
    /// Its node and the hash of its back-referenced texts (patterns with back-references).
    node: u32,
    key: u64,
}

/// The threads of one position.
#[derive(Default)]
struct Frontier {
    epoch: u32,
    node_gen: TryVec<u32>,
    head: TryVec<u32>,
    active: TryVec<u64>,
    live: bool,
    buckets: TryVec<Bucket>,
    bests: TryVec<u32>,
    caps: TryVec<i32>,
    threads: u32,
    /// Patterns with back-references: the buckets by node and key hash (open addressing, a
    /// bucket's index plus one per slot, 0 empty; at most half full), so that finding a thread's
    /// bucket does not walk every bucket of its node (which made `\(a*\)\1...` cubic).
    table: TryVec<u32>,
}

fn nomem<T>(r: crate::error::Result<T>) -> Result<T, ReError> {
    r.map_err(|_| ReError::ESpace)
}

fn idx(v: u32) -> usize {
    usize::try_from(v).unwrap_or(usize::MAX)
}

impl Frontier {
    fn ready(&mut self, nodes: usize) -> Result<(), ReError> {
        if self.node_gen.len() != nodes {
            self.node_gen.clear();
            nomem(self.node_gen.resize(nodes, 0))?;
            self.head.clear();
            nomem(self.head.resize(nodes, NONE))?;
            self.active.clear();
            nomem(self.active.resize(nodes.div_ceil(64), 0))?;
            self.epoch = 1;
        }
        Ok(())
    }

    fn clear(&mut self) {
        if !self.live {
            return;
        }
        self.epoch = self.epoch.wrapping_add(1);
        if self.epoch == 0 {
            self.node_gen.iter_mut().for_each(|g| *g = 0);
            self.epoch = 1;
        }
        self.active.iter_mut().for_each(|w| *w = 0);
        self.live = false;
        self.buckets.clear();
        self.bests.clear();
        self.caps.clear();
        self.threads = 0;
        self.table.iter_mut().for_each(|t| *t = 0);
    }

    /// The slot of `(node, key)` in a table of `len` slots (a power of two).
    fn slot(node: u32, key: u64, len: usize) -> usize {
        let h = (key ^ u64::from(node).wrapping_mul(0x9e37_79b9_7f4a_7c15)).wrapping_mul(0xff51_afd7_ed55_8ccd);
        usize::try_from(h >> 32).unwrap_or(0) & (len - 1)
    }

    /// Enter bucket `b` in the table, growing it (and entering every bucket again) when it
    /// would be more than half full.
    fn index(&mut self, b: u32) -> Result<(), ReError> {
        let want = (self.buckets.len() * 2).next_power_of_two().max(64);
        if self.table.len() < want {
            self.table.clear();
            nomem(self.table.resize(want, 0))?;
            for i in 0..self.buckets.len() {
                let Some(bk) = self.buckets.get(i).copied() else { break };
                self.place(bk.node, bk.key, u32::try_from(i).map_err(|_| ReError::ESpace)?);
            }
            return Ok(());
        }
        let Some(bk) = self.buckets.get(idx(b)).copied() else { return Ok(()) };
        self.place(bk.node, bk.key, b);
        Ok(())
    }

    fn place(&mut self, node: u32, key: u64, b: u32) {
        let len = self.table.len();
        let mut i = Self::slot(node, key, len);
        for _ in 0..len {
            match self.table.get_mut(i) {
                Some(t) if *t == 0 => {
                    *t = b + 1;
                    return;
                }
                _ => i = (i + 1) & (len - 1),
            }
        }
    }

    fn first_bucket(&self, node: u32) -> u32 {
        let i = idx(node);
        if self.node_gen.get(i) == Some(&self.epoch) { self.head.get(i).copied().unwrap_or(NONE) } else { NONE }
    }

    fn thread(&self, t: u32, stride: usize) -> &[i32] {
        let at = idx(t) * stride;
        self.caps.get(at..at + stride).unwrap_or(&[])
    }

    fn add_thread(&mut self, caps: &[i32]) -> Result<u32, ReError> {
        nomem(self.caps.extend_from_slice(caps))?;
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
struct State<'r> {
    run: &'r Run,
    /// Patterns with back-references: polynomial prefix hashes of the subject and the powers of
    /// the base, so that a text's hash costs O(1) (16 bytes per subject byte, for this call).
    prefix: TryVec<u64>,
    pows: TryVec<u64>,
    stride: usize,
    /// The position being processed.
    cur: usize,
    ring: TryVec<Frontier>,
    /// Threads beyond the ring, with their captures; per subject position the newest of its
    /// list (made at the first such thread: 4 bytes per subject byte, for this call), and how
    /// many are still waiting. Taking a position's threads costs only their number (a scan of
    /// every waiting thread at every position made `\(a*\)\1...` cubic).
    pending: TryVec<Pending>,
    pending_caps: TryVec<i32>,
    pending_head: TryVec<u32>,
    pending_live: usize,
    best: Option<Found>,
    work: u64,
}

/// The matcher's view of one subject.
pub(crate) struct Matcher<'a, P: Policy> {
    pub(crate) prog: &'a Program,
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
        class.checked_sub(1).and_then(|k| self.prog.masks_of(pc).get(k).copied())
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
        (1u32..32).filter(|g| brefs & (1 << g) != 0).all(|g| {
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
    fn key_hash(&self, st: &State<'_>, caps: &[i32]) -> u64 {
        let brefs = self.prog.brefs;
        let mut h = 0x243f_6a88_85a3_08d3u64;
        for g in (1u32..32).filter(|g| brefs & (1 << g) != 0) {
            let at = idx(g - 1) * 2;
            let (so, eo) = (caps.get(at).copied().unwrap_or(-1), caps.get(at + 1).copied().unwrap_or(-1));
            let v = match (usize::try_from(so), usize::try_from(eo)) {
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

    /// Offer a thread to `node` of the frontier for position `pos`.
    fn insert(&self, st: &mut State<'_>, pos: usize, node: u32, start: usize, caps: &[i32]) -> Result<(), ReError> {
        if node == NONE || idx(node) >= idx(self.prog.nodes) {
            return Ok(());
        }
        if let (false, Some(b)) = (st.run.late, &st.best) {
            if start > b.start {
                return Ok(());
            }
        }
        st.work += 1;
        if st.run.budget.is_some_and(|b| st.work > b) {
            return Err(ReError::ESpace);
        }
        if pos - st.cur >= RING {
            if st.pending_head.is_empty() {
                nomem(st.pending_head.resize(self.subject.len() + 1, NONE))?;
            }
            let Some(head) = st.pending_head.get(pos).copied() else { return Ok(()) };
            let off = st.pending_caps.len();
            nomem(st.pending_caps.extend_from_slice(caps))?;
            let q = u32::try_from(st.pending.len()).map_err(|_| ReError::ESpace)?;
            nomem(st.pending.push(Pending { node, start, caps: off, next: head }))?;
            if let Some(h) = st.pending_head.get_mut(pos) {
                *h = q;
            }
            st.pending_live += 1;
            return Ok(());
        }
        let stride = st.stride;
        let (pc, _) = self.prog.owner.get(idx(node)).copied().unwrap_or((0, 0));
        let classes = self.classes(st.run, pc);
        let nodes = idx(self.prog.nodes);
        let run = st.run;
        let indexed = self.prog.brefs != 0;
        let key = if indexed { self.key_hash(st, caps) } else { 0 };
        let Some(f) = st.ring.get_mut(pos % RING) else { return Ok(()) };
        f.ready(nodes)?;
        // The bucket with the same back-referenced texts.
        let mut b = NONE;
        if indexed {
            let len = f.table.len();
            if len > 0 {
                let mut i = Frontier::slot(node, key, len);
                for _ in 0..len {
                    let t = f.table.get(i).copied().unwrap_or(0);
                    if t == 0 {
                        break;
                    }
                    if let Some(bk) = f.buckets.get(idx(t - 1)).copied() {
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
        if b == NONE {
            let t = f.add_thread(caps)?;
            let best = u32::try_from(f.bests.len()).map_err(|_| ReError::ESpace)?;
            for _ in 0..classes {
                nomem(f.bests.push(t))?;
            }
            let i = idx(node);
            let next = f.first_bucket(node);
            let nb = u32::try_from(f.buckets.len()).map_err(|_| ReError::ESpace)?;
            nomem(f.buckets.push(Bucket { next, start, best, node, key }))?;
            if let (Some(g), Some(h)) = (f.node_gen.get_mut(i), f.head.get_mut(i)) {
                *g = f.epoch;
                *h = nb;
            }
            if indexed {
                f.index(nb)?;
            }
            let r = idx(self.prog.rank.get(i).copied().unwrap_or(0));
            if let Some(w) = f.active.get_mut(r / 64) {
                *w |= 1 << (r % 64);
            }
            f.live = true;
            return Ok(());
        }
        let Some(bk) = f.buckets.get(idx(b)).copied() else { return Ok(()) };
        if Self::start_better(run, start, bk.start) {
            let t = f.add_thread(caps)?;
            for k in 0..classes {
                if let Some(s) = f.bests.get_mut(idx(bk.best) + k) {
                    *s = t;
                }
            }
            if let Some(m) = f.buckets.get_mut(idx(b)) {
                m.start = start;
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

    fn accept(&self, st: &mut State<'_>, start: usize, p: usize, caps: &[i32]) -> Result<(), ReError> {
        if st.run.end.is_some_and(|e| e != p) {
            return Ok(());
        }
        let better = match &st.best {
            None => true,
            Some(b) => {
                Self::start_better(st.run, start, b.start)
                    || (start == b.start && (p > b.end || (p == b.end && self.policy.prefer(None, caps, &b.caps))))
            }
        };
        if better {
            st.best = Some(Found { start, end: p, caps: nomem(TryVec::from_slice(caps))? });
        }
        Ok(())
    }

    /// Advance the threads of node `node` at position `p`.
    fn visit(&self, st: &mut State<'_>, p: usize, node: u32, scratch: &mut TryVec<i32>) -> Result<(), ReError> {
        let (pc, j) = self.prog.owner.get(idx(node)).copied().unwrap_or((0, 0));
        let inst = self.prog.inst(pc);
        let classes = self.classes(st.run, pc);
        let stride = st.stride;
        let mut b = st.ring.get(p % RING).map_or(NONE, |f| f.first_bucket(node));
        while b != NONE {
            let Some(bk) = st.ring.get(p % RING).and_then(|f| f.buckets.get(idx(b)).copied()) else { break };
            for k in 0..classes {
                let Some(f) = st.ring.get(p % RING) else { break };
                let t = f.bests.get(idx(bk.best) + k).copied().unwrap_or(NONE);
                // The same thread can be the best of several classes: advance it once.
                if (0..k).any(|e| f.bests.get(idx(bk.best) + e).copied() == Some(t)) {
                    continue;
                }
                scratch.clear();
                nomem(scratch.extend_from_slice(f.thread(t, stride)))?;
                self.advance(st, p, inst, j, bk.start, scratch)?;
            }
            b = bk.next;
        }
        Ok(())
    }

    fn set_cap(caps: &mut [i32], at: usize, v: i32) {
        if let Some(c) = caps.get_mut(at) {
            *c = v;
        }
    }

    fn advance(&self, st: &mut State<'_>, p: usize, inst: super::program::Inst, j: u32, start: usize, caps: &mut [i32]) -> Result<(), ReError> {
        let prog = self.prog;
        let pos = i32::try_from(p).map_err(|_| ReError::ESpace)?;
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
                if st.run.caps && self.policy.resets_on_iteration() {
                    let (lo, hi) = prog.scopes.get(idx(inst.a)).copied().unwrap_or((1, 0));
                    for g in lo..=hi {
                        let at = idx(g.saturating_sub(1)) * 2;
                        Self::set_cap(caps, at, -1);
                        Self::set_cap(caps, at + 1, -1);
                    }
                }
                self.insert(st, p, prog.node(inst.b, j + 1), start, caps)?;
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
    fn step(&self, st: &mut State<'_>, p: usize, scratch: &mut TryVec<i32>) -> Result<(), ReError> {
        let words = st.ring.get(p % RING).map_or(0, |f| f.active.len());
        let mut w = 0usize;
        while w < words {
            let word = st.ring.get(p % RING).and_then(|f| f.active.get(w).copied()).unwrap_or(0);
            if word == 0 {
                w += 1;
                continue;
            }
            let bit = word.trailing_zeros();
            if let Some(a) = st.ring.get_mut(p % RING).and_then(|f| f.active.get_mut(w)) {
                *a &= !(1u64 << bit);
            }
            let rank = w * 64 + idx(bit);
            let node = self.prog.by_rank.get(rank).copied().unwrap_or(NONE);
            if node != NONE {
                self.visit(st, p, node, scratch)?;
            }
        }
        Ok(())
    }

    /// Run the search.
    pub(crate) fn run(&self, run: &Run) -> Result<Option<Found>, ReError> {
        let len = self.subject.len();
        if run.first_start > len || run.first_start > run.last_start {
            return Ok(None);
        }
        let ng = if run.caps { idx(self.prog.ngroups) } else { 0 };
        let stride = ng * 2;
        let mut ring = TryVec::new();
        for _ in 0..RING {
            nomem(ring.push(Frontier::default()))?;
        }
        let (mut prefix, mut pows) = (TryVec::new(), TryVec::new());
        if self.prog.brefs != 0 {
            const BASE: u64 = 0x0100_0000_01b3;
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
            stride,
            cur: run.first_start,
            ring,
            pending: TryVec::new(),
            pending_caps: TryVec::new(),
            pending_head: TryVec::new(),
            pending_live: 0,
            best: None,
            work: 0,
            prefix,
            pows,
        };
        let mut arrivals: TryVec<u32> = TryVec::new();
        let mut unset: TryVec<i32> = TryVec::new();
        nomem(unset.resize(stride, -1))?;
        let mut scratch: TryVec<i32> = TryVec::new();
        let mut p = run.first_start;
        loop {
            // With no thread alive, skip to the next byte a match can begin with.
            if !self.prog.empty_ok && st.pending_live == 0 && !st.ring.iter().any(|f| f.live) {
                let limit = run.last_start.saturating_add(1).min(len);
                match self.subject.get(p..limit).and_then(|r| r.iter().position(|&b| self.prog.can_start_with(b))) {
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
                nomem(scratch.extend_from_slice(st.pending_caps.get(at..at + stride).unwrap_or(&[])))?;
                let caps = core::mem::take(&mut scratch);
                st.pending_live -= 1;
                self.insert(&mut st, p, node, start, &caps)?;
                scratch = caps;
            }
            if st.pending_live == 0 {
                st.pending.clear();
                st.pending_caps.clear();
            }
            if p <= run.last_start && (run.late || st.best.is_none()) {
                self.insert(&mut st, p, self.prog.node(self.prog.start, 0), p, &unset)?;
            }
            self.step(&mut st, p, &mut scratch)?;
            if let Some(f) = st.ring.get_mut(p % RING) {
                f.clear();
            }
            if run.end == Some(p) || p >= len {
                break;
            }
            let ahead = st.ring.iter().any(|f| f.live) || st.pending_live > 0;
            let more_seeds = p < run.last_start && (run.late || st.best.is_none());
            if !ahead && !more_seeds {
                break;
            }
            p += 1;
        }
        Ok(st.best)
    }
}

/// The epsilon successors of an instruction other than those `advance` handles itself.
fn eps_of(inst: super::program::Inst, j: u32) -> impl Iterator<Item = (u32, u32)> {
    let none = (NONE, 0);
    let pair = match inst.op {
        super::program::OP_SPLIT => [(inst.a, j), (inst.b, j)],
        super::program::OP_JMP => [(inst.a, j), none],
        super::program::OP_EXIT if j >= 1 => [(inst.b, j - 1), none],
        super::program::OP_EXIT => [(inst.c, 0), none],
        _ => [none, none],
    };
    pair.into_iter()
}
