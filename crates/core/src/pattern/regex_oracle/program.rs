//! The compiled form: a Thompson automaton with explicit repetition scopes, its serialised
//! words (what `regex_t` holds), and the tables the matcher derives from it.
//!
//! Instructions consume one byte (`Byte`), one character of a set (`Set`), or nothing (all
//! others). A repetition `X{m,n}` whose body can match the empty string or holds groups is
//! compiled with a *scope*: each iteration runs between `Enter` and `Exit`. `Enter` lets the
//! submatch policy reset the groups of the body (regexec's rule that a group is reported within
//! the last iteration); `Exit` has two successors, taken when the iteration was empty or not,
//! which implements the iteration rule of spec §14.9 (XBD 9.1, 9.3.6): an empty iteration is
//! allowed, but no iteration follows an empty one beyond the minimum count. Bounded
//! repetitions are expanded into copies; unbounded tails are loops.
//!
//! The matcher's states are pairs (instruction, `j`), where `j` counts the innermost enclosing
//! scopes whose current iteration began at the current position (so `Exit` knows whether its
//! iteration was empty without any per-thread state). Within one position the epsilon edges
//! between these states form an acyclic graph: a loop can only be re-entered after a non-empty
//! iteration, that is, after consuming. The topological order of that graph, computed here,
//! is the order in which the matcher processes the states of a position.
#![allow(clippy::arithmetic_side_effects)] // instruction counts are bounded by MAX_INSTS; group numbers by the pattern's length

use crate::locale::Locale;
use crate::mem::TryVec;

use super::parse::{Assert, Node, Parsed};
use super::text::{CharSet, Mode};
use super::ReError;

/// The largest automaton (instructions) a pattern may expand to; beyond it, `REG_ESPACE`.
const MAX_INSTS: usize = 1 << 20;
/// No instruction / no node.
pub(crate) const NONE: u32 = u32::MAX;

pub(crate) const OP_BYTE: u32 = 1;
pub(crate) const OP_SET: u32 = 2;
pub(crate) const OP_SPLIT: u32 = 3;
pub(crate) const OP_JMP: u32 = 4;
pub(crate) const OP_OPEN: u32 = 5;
pub(crate) const OP_CLOSE: u32 = 6;
pub(crate) const OP_ASSERT: u32 = 7;
pub(crate) const OP_BACKREF: u32 = 8;
pub(crate) const OP_ENTER: u32 = 9;
pub(crate) const OP_EXIT: u32 = 10;
pub(crate) const OP_MATCH: u32 = 11;

/// One instruction: an operation and three operands (see the `OP_*` uses in `compile`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Inst {
    pub(crate) op: u32,
    pub(crate) a: u32,
    pub(crate) b: u32,
    pub(crate) c: u32,
}

/// A chain of enclosing scopes: the innermost scope and the enclosing chain.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Chain {
    pub(crate) parent: u32,
    pub(crate) scope: u32,
    pub(crate) depth: u32,
}

/// The compiled pattern.
#[derive(Debug, Default)]
pub struct Program {
    pub(crate) insts: TryVec<Inst>,
    /// The scope chain of each instruction.
    pub(crate) inst_chain: TryVec<u32>,
    pub(crate) chains: TryVec<Chain>,
    /// Per scope: the groups of the repeated body, `(first, last)` (`first > last`: none).
    pub(crate) scopes: TryVec<(u32, u32)>,
    pub(crate) sets: TryVec<CharSet>,
    pub(crate) start: u32,
    /// The groups: the subexpressions and the hidden groups of other subpatterns, in the order
    /// of their opening (`parse::hide_subpatterns`).
    pub(crate) ngroups: u32,
    /// Bit `n`: group `n` is the target of a back-reference.
    pub(crate) brefs: u32,
    /// The subexpressions (`re_nsub`).
    pub(crate) nsub: u32,
    /// The group of each subexpression (empty: subexpression `k` is group `k`).
    pub(crate) report: TryVec<u32>,
    pub(crate) icase: bool,
    pub(crate) multibyte: bool,
    pub(crate) can_be_null: bool,
    // Derived (not serialised): the matcher's state numbering and order.
    /// Node of (instruction, 0); the nodes of an instruction are `base..=base + depth`.
    pub(crate) node_base: TryVec<u32>,
    /// The instruction and `j` of each node.
    pub(crate) owner: TryVec<(u32, u32)>,
    pub(crate) nodes: u32,
    /// Topological rank of each node.
    pub(crate) rank: TryVec<u32>,
    /// Node of each rank.
    pub(crate) by_rank: TryVec<u32>,
    /// Per chain: the distinct non-empty group ranges of its scopes, innermost first, as an
    /// offset and length into `masks`.
    pub(crate) chain_masks: TryVec<(u32, u32)>,
    pub(crate) masks: TryVec<(u32, u32)>,
    /// The bytes a match can begin with (a match that consumes something begins with one of
    /// them; conservative for multibyte sets and back-references).
    pub(crate) first: [u64; 4],
    /// A match can be empty (the start reaches the end without consuming): any position may
    /// begin one.
    pub(crate) empty_ok: bool,
}

fn nomem<T>(r: crate::error::Result<T>) -> Result<T, ReError> {
    r.map_err(|_| ReError::ESpace)
}

fn nullable(n: &Node) -> bool {
    match n {
        Node::Byte(_) | Node::Set(_) => false,
        Node::Empty | Node::Assert(_) | Node::Backref(_) => true,
        Node::Cat(v) => v.iter().all(nullable),
        Node::Alt(v) => v.iter().any(nullable),
        Node::Group(_, x) => nullable(x),
        Node::Rep(x, m, _) => *m == 0 || nullable(x),
    }
}

/// The first and last group inside `n` (`(NONE, 0)` when it has none).
fn group_range(n: &Node) -> (u32, u32) {
    fn walk(n: &Node, r: &mut (u32, u32)) {
        match n {
            Node::Group(g, x) => {
                r.0 = r.0.min(*g);
                r.1 = r.1.max(*g);
                walk(x, r);
            }
            Node::Cat(v) | Node::Alt(v) => v.iter().for_each(|c| walk(c, r)),
            Node::Rep(x, _, _) => walk(x, r),
            _ => {}
        }
    }
    let mut r = (NONE, 0);
    walk(n, &mut r);
    r
}

struct Builder {
    insts: TryVec<Inst>,
    inst_chain: TryVec<u32>,
    chains: TryVec<Chain>,
    scopes: TryVec<(u32, u32)>,
}

impl Builder {
    fn emit(&mut self, op: u32, a: u32, b: u32, c: u32, chain: u32) -> Result<u32, ReError> {
        if self.insts.len() >= MAX_INSTS {
            return Err(ReError::ESpace);
        }
        let pc = u32::try_from(self.insts.len()).map_err(|_| ReError::ESpace)?;
        nomem(self.insts.push(Inst { op, a, b, c }))?;
        nomem(self.inst_chain.push(chain))?;
        Ok(pc)
    }

    fn patch(&mut self, pc: u32, inst: Inst) {
        if let Some(i) = usize::try_from(pc).ok().and_then(|i| self.insts.get_mut(i)) {
            *i = inst;
        }
    }

    fn chain(&mut self, parent: u32, scope: u32) -> Result<u32, ReError> {
        let depth = if parent == NONE { 1 } else { self.chains.get(usize::try_from(parent).unwrap_or(usize::MAX)).map_or(1, |c| c.depth + 1) };
        let id = u32::try_from(self.chains.len()).map_err(|_| ReError::ESpace)?;
        nomem(self.chains.push(Chain { parent, scope, depth }))?;
        Ok(id)
    }

    /// Compile `n` followed by `next`, inside scope chain `chain`; the entry instruction.
    fn node(&mut self, n: &Node, next: u32, chain: u32) -> Result<u32, ReError> {
        Ok(match n {
            Node::Empty => next,
            Node::Byte(b) => self.emit(OP_BYTE, u32::from(*b), next, 0, chain)?,
            Node::Set(k) => self.emit(OP_SET, *k, next, 0, chain)?,
            Node::Assert(a) => self.emit(OP_ASSERT, assert_code(*a), next, 0, chain)?,
            Node::Backref(g) => self.emit(OP_BACKREF, *g, next, 0, chain)?,
            Node::Cat(items) => {
                let mut pc = next;
                for item in items.iter().rev() {
                    pc = self.node(item, pc, chain)?;
                }
                pc
            }
            Node::Alt(branches) => {
                let mut entries: TryVec<u32> = TryVec::new();
                for b in branches.iter() {
                    let e = self.node(b, next, chain)?;
                    nomem(entries.push(e))?;
                }
                let mut pc = entries.pop().unwrap_or(next);
                while let Some(e) = entries.pop() {
                    pc = self.emit(OP_SPLIT, e, pc, 0, chain)?;
                }
                pc
            }
            Node::Group(g, x) => {
                let close = self.emit(OP_CLOSE, *g, next, 0, chain)?;
                let body = self.node(x, close, chain)?;
                self.emit(OP_OPEN, *g, body, 0, chain)?
            }
            Node::Rep(x, m, n) => self.rep(x, *m, *n, next, chain)?,
        })
    }

    fn rep(&mut self, x: &Node, m: u32, n: Option<u32>, next: u32, chain: u32) -> Result<u32, ReError> {
        if n == Some(0) {
            return Ok(next);
        }
        if m == 1 && n == Some(1) {
            return self.node(x, next, chain);
        }
        let groups = group_range(x);
        let scoped = nullable(x) || groups.0 != NONE;
        if m == 0 && n == Some(1) {
            // `X?`: one optional pass; with no later iteration there is nothing to gate or to
            // reset (an enclosing repetition resets the groups at its own iterations).
            let body = self.node(x, next, chain)?;
            return self.emit(OP_SPLIT, body, next, 0, chain);
        }
        if !scoped {
            // A body that consumes and has no groups: plain copies and a plain loop.
            let opt = match n {
                None => {
                    let l = self.emit(OP_SPLIT, NONE, next, 0, chain)?;
                    let body = self.node(x, l, chain)?;
                    self.patch(l, Inst { op: OP_SPLIT, a: body, b: next, c: 0 });
                    l
                }
                Some(n) => {
                    let mut cont = next;
                    for _ in m..n {
                        let body = self.node(x, cont, chain)?;
                        cont = self.emit(OP_SPLIT, body, next, 0, chain)?;
                    }
                    cont
                }
            };
            let mut entry = opt;
            for _ in 0..m {
                entry = self.node(x, entry, chain)?;
            }
            return Ok(entry);
        }
        let scope = u32::try_from(self.scopes.len()).map_err(|_| ReError::ESpace)?;
        nomem(self.scopes.push(if groups.0 == NONE { (1, 0) } else { groups }))?;
        // The optional part: after the mandatory copies, iterations that may stop.
        let opt = match n {
            None => {
                let l = self.emit(OP_SPLIT, NONE, next, 0, chain)?;
                let inner = self.chain(chain, scope)?;
                let ex = self.emit(OP_EXIT, scope, next, l, inner)?;
                let body = self.node(x, ex, inner)?;
                let en = self.emit(OP_ENTER, scope, body, 0, chain)?;
                self.patch(l, Inst { op: OP_SPLIT, a: en, b: next, c: 0 });
                l
            }
            Some(n) => {
                let mut cont = next;
                for _ in m..n {
                    let inner = self.chain(chain, scope)?;
                    let ex = self.emit(OP_EXIT, scope, next, cont, inner)?;
                    let body = self.node(x, ex, inner)?;
                    let en = self.emit(OP_ENTER, scope, body, 0, chain)?;
                    cont = self.emit(OP_SPLIT, en, next, 0, chain)?;
                }
                cont
            }
        };
        // The mandatory copies, last first. After the last one an empty iteration stops the
        // repetition; before it, the next copy follows whatever the iteration matched.
        let mut entry = opt;
        for t in (1..=m).rev() {
            let inner = self.chain(chain, scope)?;
            let ex = if t == m { self.emit(OP_EXIT, scope, next, opt, inner)? } else { self.emit(OP_EXIT, scope, entry, entry, inner)? };
            let body = self.node(x, ex, inner)?;
            entry = self.emit(OP_ENTER, scope, body, 0, chain)?;
        }
        Ok(entry)
    }
}

pub(crate) fn assert_code(a: Assert) -> u32 {
    match a {
        Assert::Bol => 0,
        Assert::Eol => 1,
        Assert::BufStart => 2,
        Assert::BufEnd => 3,
        Assert::WordBoundary => 4,
        Assert::NotWordBoundary => 5,
        Assert::WordStart => 6,
        Assert::WordEnd => 7,
    }
}

impl Program {
    /// Build the automaton of a parsed pattern.
    pub(crate) fn build(parsed: Parsed, loc: &Locale, icase: bool) -> Result<Program, ReError> {
        let mut b = Builder { insts: TryVec::new(), inst_chain: TryVec::new(), chains: TryVec::new(), scopes: TryVec::new() };
        let m = b.emit(OP_MATCH, 0, 0, 0, NONE)?;
        let start = b.node(&parsed.root, m, NONE)?;
        let mut p = Program {
            insts: b.insts,
            inst_chain: b.inst_chain,
            chains: b.chains,
            scopes: b.scopes,
            sets: parsed.sets,
            start,
            ngroups: parsed.ngroups,
            brefs: parsed.brefs,
            nsub: parsed.nsub,
            report: parsed.report,
            icase,
            multibyte: Mode::of(loc).multibyte(),
            can_be_null: nullable(&parsed.root),
            ..Program::default()
        };
        p.derive()?;
        Ok(p)
    }

    pub(crate) fn inst(&self, pc: u32) -> Inst {
        usize::try_from(pc).ok().and_then(|i| self.insts.get(i)).copied().unwrap_or(Inst { op: OP_MATCH, a: 0, b: 0, c: 0 })
    }

    fn chain_of(&self, pc: u32) -> u32 {
        usize::try_from(pc).ok().and_then(|i| self.inst_chain.get(i)).copied().unwrap_or(NONE)
    }

    pub(crate) fn depth(&self, pc: u32) -> u32 {
        let c = self.chain_of(pc);
        if c == NONE { 0 } else { self.chains.get(usize::try_from(c).unwrap_or(usize::MAX)).map_or(0, |c| c.depth) }
    }

    /// The node of (`pc`, `j`).
    pub(crate) fn node(&self, pc: u32, j: u32) -> u32 {
        usize::try_from(pc).ok().and_then(|i| self.node_base.get(i)).map_or(NONE, |b| b + j)
    }

    /// The group ranges a state's future iterations may overwrite (distinct, non-empty,
    /// innermost first).
    pub(crate) fn masks_of(&self, pc: u32) -> &[(u32, u32)] {
        let c = self.chain_of(pc);
        let Some(&(off, len)) = usize::try_from(c).ok().and_then(|i| self.chain_masks.get(i)) else { return &[] };
        let (off, len) = (usize::try_from(off).unwrap_or(0), usize::try_from(len).unwrap_or(0));
        self.masks.get(off..off + len).unwrap_or(&[])
    }

    /// The epsilon successors of node (`pc`, `j`): up to two (`NONE` for none).
    pub(crate) fn eps(&self, pc: u32, j: u32) -> [(u32, u32); 2] {
        let i = self.inst(pc);
        let none = (NONE, 0);
        match i.op {
            OP_SPLIT => [(i.a, j), (i.b, j)],
            OP_JMP => [(i.a, j), none],
            OP_OPEN | OP_CLOSE | OP_ASSERT | OP_BACKREF => [(i.b, j), none],
            OP_ENTER => [(i.b, j + 1), none],
            OP_EXIT if j >= 1 => [(i.b, j - 1), none],
            OP_EXIT => [(i.c, 0), none],
            _ => [none, none],
        }
    }

    /// Number the nodes, order them topologically, and collect the comparison masks.
    pub(crate) fn derive(&mut self) -> Result<(), ReError> {
        let n = self.insts.len();
        let mut base = TryVec::new();
        nomem(base.reserve(n))?;
        let mut total: u32 = 0;
        for pc in 0..n {
            nomem(base.push(total))?;
            let d = self.depth(u32::try_from(pc).map_err(|_| ReError::ESpace)?);
            total = total.checked_add(d + 1).ok_or(ReError::ESpace)?;
        }
        self.node_base = base;
        self.nodes = total;
        let nodes = usize::try_from(total).map_err(|_| ReError::ESpace)?;
        // Which instruction each node belongs to, and its `j`.
        let mut owner: TryVec<(u32, u32)> = TryVec::new();
        nomem(owner.resize(nodes, (0, 0)))?;
        for pc in 0..n {
            let pcu = u32::try_from(pc).map_err(|_| ReError::ESpace)?;
            for j in 0..=self.depth(pcu) {
                if let Some(o) = owner.get_mut(usize::try_from(self.node(pcu, j)).unwrap_or(usize::MAX)) {
                    *o = (pcu, j);
                }
            }
        }
        // Kahn's algorithm over the epsilon edges.
        let mut indeg: TryVec<u32> = TryVec::new();
        nomem(indeg.resize(nodes, 0))?;
        for &(pc, j) in owner.iter() {
            for (t, tj) in self.eps(pc, j) {
                if t != NONE {
                    if let Some(d) = indeg.get_mut(usize::try_from(self.node(t, tj)).unwrap_or(usize::MAX)) {
                        *d += 1;
                    }
                }
            }
        }
        let mut queue: TryVec<u32> = TryVec::new();
        nomem(queue.reserve(nodes))?;
        for (k, d) in indeg.iter().enumerate() {
            if *d == 0 {
                nomem(queue.push(u32::try_from(k).map_err(|_| ReError::ESpace)?))?;
            }
        }
        let mut rank: TryVec<u32> = TryVec::new();
        nomem(rank.resize(nodes, NONE))?;
        let mut head = 0usize;
        while let Some(&v) = queue.get(head) {
            if let Some(r) = rank.get_mut(usize::try_from(v).unwrap_or(usize::MAX)) {
                *r = u32::try_from(head).map_err(|_| ReError::ESpace)?;
            }
            head += 1;
            let (pc, j) = owner.get(usize::try_from(v).unwrap_or(usize::MAX)).copied().unwrap_or((0, 0));
            for (t, tj) in self.eps(pc, j) {
                if t == NONE {
                    continue;
                }
                let w = self.node(t, tj);
                if let Some(d) = indeg.get_mut(usize::try_from(w).unwrap_or(usize::MAX)) {
                    *d -= 1;
                    if *d == 0 {
                        nomem(queue.push(w))?;
                    }
                }
            }
        }
        if head != nodes {
            // An epsilon cycle: the construction above never makes one.
            return Err(ReError::ESpace);
        }
        self.rank = rank;
        self.by_rank = queue;
        self.owner = owner;
        // Masks per chain.
        let mut chain_masks = TryVec::new();
        let mut masks: TryVec<(u32, u32)> = TryVec::new();
        for c in 0..self.chains.len() {
            let off = u32::try_from(masks.len()).map_err(|_| ReError::ESpace)?;
            let mut len = 0u32;
            let mut cur = u32::try_from(c).map_err(|_| ReError::ESpace)?;
            while cur != NONE {
                let Some(ch) = self.chains.get(usize::try_from(cur).unwrap_or(usize::MAX)).copied() else { break };
                let r = self.scopes.get(usize::try_from(ch.scope).unwrap_or(usize::MAX)).copied().unwrap_or((1, 0));
                let last = if len > 0 { masks.last().copied() } else { None };
                if r.0 <= r.1 && last != Some(r) {
                    nomem(masks.push(r))?;
                    len += 1;
                }
                cur = ch.parent;
            }
            nomem(chain_masks.push((off, len)))?;
        }
        self.chain_masks = chain_masks;
        self.masks = masks;
        self.first_bytes()
    }

    /// Fill [`Program::first`] and [`Program::empty_ok`]: walk the epsilon closure of the start
    /// state.
    fn first_bytes(&mut self) -> Result<(), ReError> {
        let mut first = [0u64; 4];
        let mut empty_ok = false;
        let mut seen: TryVec<bool> = TryVec::new();
        nomem(seen.resize(usize::try_from(self.nodes).map_err(|_| ReError::ESpace)?, false))?;
        let mut stack: TryVec<(u32, u32)> = TryVec::new();
        nomem(stack.push((self.start, 0)))?;
        let all = |f: &mut [u64; 4]| f.iter_mut().for_each(|w| *w = u64::MAX);
        while let Some((pc, j)) = stack.pop() {
            let Some(s) = seen.get_mut(usize::try_from(self.node(pc, j)).unwrap_or(usize::MAX)) else { continue };
            if *s {
                continue;
            }
            *s = true;
            let i = self.inst(pc);
            match i.op {
                OP_BYTE => {
                    if let Some(w) = first.get_mut(usize::try_from(i.a >> 6).unwrap_or(0)) {
                        *w |= 1 << (i.a & 63);
                    }
                }
                OP_SET => match self.sets.get(usize::try_from(i.a).unwrap_or(usize::MAX)) {
                    Some(set) => {
                        for (f, t) in first.iter_mut().zip(set.table) {
                            *f |= t;
                        }
                        if self.multibyte {
                            // Any byte of 0x80 and up may begin a character the set holds.
                            first.iter_mut().skip(2).for_each(|w| *w = u64::MAX);
                        }
                    }
                    None => all(&mut first),
                },
                OP_MATCH => empty_ok = true,
                _ => {
                    if i.op == OP_BACKREF {
                        all(&mut first);
                    }
                    for (t, tj) in self.eps(pc, j) {
                        if t != NONE {
                            nomem(stack.push((t, tj)))?;
                        }
                    }
                }
            }
        }
        self.first = first;
        self.empty_ok = empty_ok;
        Ok(())
    }

    /// Whether a match can begin with byte `b` (always, when a match can be empty).
    pub fn can_start_with(&self, b: u8) -> bool {
        self.empty_ok || self.first.get(usize::from(b >> 6)).is_some_and(|w| (w >> (b & 63)) & 1 != 0)
    }

    // ---------------------------------------------------------------------------- words

    /// The serialised form (what a `regex_t` holds; [`Program::from_words`] reads it back).
    pub fn to_words(&self) -> crate::error::Result<TryVec<u32>> {
        let mut w = TryVec::new();
        let flags = u32::from(self.icase) | (u32::from(self.multibyte) << 1) | (u32::from(self.can_be_null) << 2);
        let len32 = |n: usize| u32::try_from(n).unwrap_or(u32::MAX);
        for v in [MAGIC, flags, self.start, self.ngroups, self.brefs, self.nsub, len32(self.report.len()), len32(self.insts.len()), len32(self.chains.len()), len32(self.scopes.len()), len32(self.sets.len())] {
            w.push(v)?;
        }
        for &g in self.report.iter() {
            w.push(g)?;
        }
        for (i, c) in self.insts.iter().zip(self.inst_chain.iter()) {
            for v in [i.op, i.a, i.b, i.c, *c] {
                w.push(v)?;
            }
        }
        for c in self.chains.iter() {
            for v in [c.parent, c.scope, c.depth] {
                w.push(v)?;
            }
        }
        for s in self.scopes.iter() {
            w.push(s.0)?;
            w.push(s.1)?;
        }
        for s in self.sets.iter() {
            for v in [s.flags, len32(s.ranges.len()), len32(s.seq.len()), len32(s.classes.len()), len32(s.equiv.len())] {
                w.push(v)?;
            }
            for t in s.table {
                w.push(u32::try_from(t & 0xffff_ffff).unwrap_or(0))?;
                w.push(u32::try_from(t >> 32).unwrap_or(0))?;
            }
            for &(a, b) in s.ranges.iter().chain(s.seq.iter()) {
                w.push(a)?;
                w.push(b)?;
            }
            for &v in s.classes.iter().chain(s.equiv.iter()) {
                w.push(v)?;
            }
        }
        Ok(w)
    }

    /// Read the serialised form back; `None` if it is not one (a damaged or foreign
    /// `regex_t`), or `REG_ESPACE` when memory is exhausted.
    pub fn from_words(words: &[u32]) -> Result<Option<Program>, ReError> {
        let mut r = Reader { w: words, i: 0 };
        let Some(p) = r.program()? else { return Ok(None) };
        Ok(Some(p))
    }

    /// The number of subexpressions.
    pub fn nsub(&self) -> usize {
        usize::try_from(self.nsub).unwrap_or(0)
    }

    /// The group that holds subexpression `k` (from 1).
    pub(crate) fn group_of(&self, k: u32) -> u32 {
        if self.report.is_empty() { k } else { self.report.get(usize::try_from(k).unwrap_or(0).wrapping_sub(1)).copied().unwrap_or(0) }
    }

    /// Whether the pattern can match the empty string (`can_be_null`).
    pub fn can_be_null(&self) -> bool {
        self.can_be_null
    }

    /// Whether the pattern has back-references.
    pub fn has_backrefs(&self) -> bool {
        self.brefs != 0
    }
}

/// The first word of a serialised program ("Lrx2": with the subexpression map).
const MAGIC: u32 = 0x3278_724c;

struct Reader<'a> {
    w: &'a [u32],
    i: usize,
}

impl Reader<'_> {
    fn next(&mut self) -> Option<u32> {
        let v = self.w.get(self.i).copied();
        self.i += 1;
        v
    }

    fn count(&mut self) -> Option<usize> {
        let n = usize::try_from(self.next()?).ok()?;
        // Each item takes at least one word: a count beyond the rest is damage.
        (n <= self.w.len()).then_some(n)
    }

    fn program(&mut self) -> Result<Option<Program>, ReError> {
        let Some(p) = self.fields()? else { return Ok(None) };
        let mut p = p;
        // Validate references before deriving.
        let ninst = u32::try_from(p.insts.len()).unwrap_or(0);
        let ok_pc = |pc: u32| pc < ninst;
        let nsets = u32::try_from(p.sets.len()).unwrap_or(0);
        let nscopes = u32::try_from(p.scopes.len()).unwrap_or(0);
        let nchains = u32::try_from(p.chains.len()).unwrap_or(0);
        if !ok_pc(p.start) || p.ngroups > 0x7fff_ffff || p.nsub > p.ngroups {
            return Ok(None);
        }
        let nreport = u32::try_from(p.report.len()).unwrap_or(u32::MAX);
        if (nreport != 0 && nreport != p.nsub) || p.report.iter().any(|&g| g == 0 || g > p.ngroups) {
            return Ok(None);
        }
        for (i, &c) in p.insts.iter().zip(p.inst_chain.iter()) {
            let valid = (c == NONE || c < nchains)
                && match i.op {
                    OP_BYTE => i.a < 256 && ok_pc(i.b),
                    OP_SET => i.a < nsets && ok_pc(i.b),
                    OP_SPLIT => ok_pc(i.a) && ok_pc(i.b),
                    OP_JMP => ok_pc(i.a),
                    OP_OPEN | OP_CLOSE | OP_BACKREF => i.a >= 1 && i.a <= p.ngroups && ok_pc(i.b),
                    OP_ASSERT => i.a < 8 && ok_pc(i.b),
                    OP_ENTER => i.a < nscopes && ok_pc(i.b),
                    OP_EXIT => i.a < nscopes && ok_pc(i.b) && ok_pc(i.c),
                    OP_MATCH => true,
                    _ => false,
                };
            if !valid {
                return Ok(None);
            }
        }
        for (k, c) in p.chains.iter().enumerate() {
            let k = u32::try_from(k).unwrap_or(u32::MAX);
            if (c.parent != NONE && c.parent >= k) || c.scope >= nscopes || c.depth == 0 || c.depth > 1_000_000 {
                return Ok(None);
            }
        }
        p.derive()?;
        Ok(Some(p))
    }

    fn fields(&mut self) -> Result<Option<Program>, ReError> {
        let mut p = Program::default();
        let header = (|| {
            if self.next()? != MAGIC {
                return None;
            }
            let flags = self.next()?;
            p.icase = flags & 1 != 0;
            p.multibyte = flags & 2 != 0;
            p.can_be_null = flags & 4 != 0;
            p.start = self.next()?;
            p.ngroups = self.next()?;
            p.brefs = self.next()?;
            p.nsub = self.next()?;
            Some((self.count()?, self.count()?, self.count()?, self.count()?, self.count()?))
        })();
        let Some((nreport, ninst, nchains, nscopes, nsets)) = header else { return Ok(None) };
        for _ in 0..nreport {
            let Some(g) = self.next() else { return Ok(None) };
            nomem(p.report.push(g))?;
        }
        for _ in 0..ninst {
            let (Some(op), Some(a), Some(b), Some(c), Some(ch)) = (self.next(), self.next(), self.next(), self.next(), self.next()) else { return Ok(None) };
            nomem(p.insts.push(Inst { op, a, b, c }))?;
            nomem(p.inst_chain.push(ch))?;
        }
        for _ in 0..nchains {
            let (Some(parent), Some(scope), Some(depth)) = (self.next(), self.next(), self.next()) else { return Ok(None) };
            nomem(p.chains.push(Chain { parent, scope, depth }))?;
        }
        for _ in 0..nscopes {
            let (Some(a), Some(b)) = (self.next(), self.next()) else { return Ok(None) };
            nomem(p.scopes.push((a, b)))?;
        }
        for _ in 0..nsets {
            let Some(flags) = self.next() else { return Ok(None) };
            let (Some(nr), Some(ns), Some(nc), Some(ne)) = (self.count(), self.count(), self.count(), self.count()) else { return Ok(None) };
            let mut s = CharSet::new(flags);
            for t in s.table.iter_mut() {
                let (Some(lo), Some(hi)) = (self.next(), self.next()) else { return Ok(None) };
                *t = u64::from(lo) | (u64::from(hi) << 32);
            }
            for _ in 0..nr {
                let (Some(a), Some(b)) = (self.next(), self.next()) else { return Ok(None) };
                nomem(s.ranges.push((a, b)))?;
            }
            for _ in 0..ns {
                let (Some(a), Some(b)) = (self.next(), self.next()) else { return Ok(None) };
                nomem(s.seq.push((a, b)))?;
            }
            for _ in 0..nc {
                let Some(v) = self.next() else { return Ok(None) };
                nomem(s.classes.push(v))?;
            }
            for _ in 0..ne {
                let Some(v) = self.next() else { return Ok(None) };
                nomem(s.equiv.push(v))?;
            }
            nomem(p.sets.push(s))?;
        }
        Ok(Some(p))
    }
}
