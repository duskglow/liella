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
use super::text::{is_word, set_matches, CharSet, Mode, SetDef};
use super::ReError;

/// The largest automaton (instructions) a pattern may expand to; beyond it, `REG_ESPACE`.
const MAX_INSTS: usize = 1 << 20;
/// The most matcher states (nodes: an instruction counts once per enclosing scope, plus one) a
/// pattern may expand to; beyond it, `REG_ESPACE`. Stacked repetitions of a nullable body
/// (`b*++++`) double the copies at each level and deepen the scopes, so the states grow faster
/// than the instructions; the matcher's tables are per state (about 200 bytes each), so this
/// bounds a search's memory to a few hundred MiB, as `MAX_INSTS` did before scopes nested.
const MAX_NODES: usize = 1 << 21;
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

/// The compiled pattern as regcomp builds it (owned tables), before serialisation.
#[derive(Debug, Default)]
pub(crate) struct Built {
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
    // Derived at regcomp (serialised too): the matcher's state numbering and order.
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
    /// The class id of each byte (D-14.12): two bytes share a class when the matcher treats them
    /// alike (every `Byte` literal, every `Set`, word-ness and newline-ness). Computed at
    /// regcomp; the lazy DFA reads it. 256 entries (empty until [`Built::byte_classes`] runs).
    pub(crate) classes: TryVec<u8>,
    /// The number of byte classes.
    pub(crate) nclasses: u32,
    /// The LEAVE class id (a multibyte locale's bytes `>= 0x80`, on which a search restarts in
    /// the NFA), or [`NONE`] in a single-byte locale.
    pub(crate) leave: u32,
    /// The program is one-pass (D-14.15, [`Built::one_pass_check`]): a known match's captures
    /// come from a single walk ([`super::exec`]'s `one_pass`), not the thread-set pass.
    pub(crate) one_pass: bool,
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
    /// The nodes of the instructions emitted so far (see [`MAX_NODES`]).
    nodes: usize,
}

impl Builder {
    fn emit(&mut self, op: u32, a: u32, b: u32, c: u32, chain: u32) -> Result<u32, ReError> {
        if self.insts.len() >= MAX_INSTS {
            return Err(ReError::ESpace);
        }
        let depth = if chain == NONE { 0 } else { self.chains.get(usize::try_from(chain).unwrap_or(usize::MAX)).map_or(0, |c| c.depth) };
        self.nodes += usize::try_from(depth).map_err(|_| ReError::ESpace)? + 1;
        if self.nodes > MAX_NODES {
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
                    if m >= 1 {
                        // `X{m,}`: the loop's body is the last mandatory copy too (D-14.8): entered
                        // directly, it is exactly that copy followed by the loop, so `X+` keeps one
                        // copy of `X` and stacked `+` grow linearly, not by doubling.
                        let mut entry = body;
                        for _ in 1..m {
                            entry = self.node(x, entry, chain)?;
                        }
                        return Ok(entry);
                    }
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
        // `X{m,}` with `m >= 1`: the loop's iteration is also the last mandatory copy (D-14.8).
        // That copy would be `ENTER X EXIT(empty: next, consumed: loop)`, the very instructions of
        // the loop's iteration, so it is entered directly instead of compiled again: `X+` keeps
        // one copy of `X`, and stacked `+` (`b*++++`, `((a)+)+`) grow linearly, not by doubling.
        let mut last = m;
        let opt = match n {
            None => {
                let l = self.emit(OP_SPLIT, NONE, next, 0, chain)?;
                let inner = self.chain(chain, scope)?;
                let ex = self.emit(OP_EXIT, scope, next, l, inner)?;
                let body = self.node(x, ex, inner)?;
                let en = self.emit(OP_ENTER, scope, body, 0, chain)?;
                self.patch(l, Inst { op: OP_SPLIT, a: en, b: next, c: 0 });
                if m >= 1 {
                    last = m - 1;
                    en
                } else {
                    l
                }
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
        for t in (1..=last).rev() {
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

impl Built {
    /// Build the automaton of a parsed pattern.
    fn build(parsed: Parsed, loc: &Locale, icase: bool) -> Result<Built, ReError> {
        let mut b = Builder { insts: TryVec::new(), inst_chain: TryVec::new(), chains: TryVec::new(), scopes: TryVec::new(), nodes: 0 };
        let m = b.emit(OP_MATCH, 0, 0, 0, NONE)?;
        let start = b.node(&parsed.root, m, NONE)?;
        let mut p = Built {
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
            ..Built::default()
        };
        p.derive()?;
        p.byte_classes(loc)?;
        p.one_pass = p.one_pass_check()?;
        Ok(p)
    }

    /// Fill [`Built::classes`], `nclasses` and `leave` (D-14.12): group the bytes the matcher
    /// cannot tell apart. Two bytes are equivalent when they are the same literal (or neither a
    /// literal), have the same newline-ness, the same word-ness when the program has a word
    /// assertion, and belong to exactly the same sets.
    /// In a multibyte (UTF-8) locale every byte `>= 0x80` is folded into one LEAVE class, since a
    /// search that meets one restarts in the NFA. Pure groundwork: nothing reads the classes yet.
    ///
    /// Partition refinement on 256-bit byte masks (RX-T2-0c): literal singletons, newline, word
    /// (only when asked for), the rest; then each set splits every class it cuts (`class & set`,
    /// `class & !set`), a few word operations per class instead of a pass over every byte. The
    /// classes are numbered in order of their first byte.
    #[allow(clippy::indexing_slicing)] // masks are `[u64; 4]` indexed by `byte >> 6` (< 4); `classes` has 256 entries indexed by bytes
    fn byte_classes(&mut self, loc: &Locale) -> Result<(), ReError> {
        type Mask = [u64; 4];
        fn and(a: Mask, b: Mask) -> Mask {
            [a[0] & b[0], a[1] & b[1], a[2] & b[2], a[3] & b[3]]
        }
        fn and_not(a: Mask, b: Mask) -> Mask {
            [a[0] & !b[0], a[1] & !b[1], a[2] & !b[2], a[3] & !b[3]]
        }
        fn empty(a: Mask) -> bool {
            (a[0] | a[1] | a[2] | a[3]) == 0
        }
        // Word compares (`==` on arrays becomes a `bcmp` call).
        fn same(a: Mask, b: Mask) -> bool {
            ((a[0] ^ b[0]) | (a[1] ^ b[1]) | (a[2] ^ b[2]) | (a[3] ^ b[3])) == 0
        }
        fn bit(v: usize) -> Mask {
            let mut m = [0u64; 4];
            m[(v >> 6) & 3] = 1u64 << (v & 63);
            m
        }
        // The first byte of a non-empty mask (256 for an empty one).
        fn first(a: Mask) -> u32 {
            let mut base = 0u32;
            for w in a {
                if w != 0 {
                    return base | w.trailing_zeros();
                }
                base = base.saturating_add(64);
            }
            base
        }
        let mode = Mode::of(loc);
        let multibyte = mode.multibyte();
        // The bytes classified one by one: all of them, or the ASCII ones in a multibyte locale.
        let all: Mask = if multibyte { [!0, !0, 0, 0] } else { [!0; 4] };
        // Word-ness splits classes only when an instruction asks for it (`\b`, `\B`, `\<`, `\>`; a
        // `\w` set carries its own table): in a multibyte locale each `is_word` is a wide-class
        // lookup, and 128 of them per regcomp cost more than the rest of the compile (RX-T2-0c).
        let (word_lo, word_hi) = (assert_code(Assert::WordBoundary), assert_code(Assert::WordEnd));
        let word = self.insts.iter().any(|i| i.op == OP_ASSERT && (word_lo..=word_hi).contains(&i.a));
        let mut lits = [0u64; 4];
        for i in self.insts.iter() {
            if i.op == OP_BYTE {
                if let Ok(b) = u8::try_from(i.a) {
                    let m = bit(usize::from(b));
                    lits = [lits[0] | m[0], lits[1] | m[1], lits[2] | m[2], lits[3] | m[3]];
                }
            }
        }
        lits = and(lits, all);
        let mut parts: TryVec<Mask> = TryVec::new();
        nomem(parts.reserve(8))?;
        for (w, &lw) in lits.iter().enumerate() {
            let mut x = lw;
            while x != 0 {
                let low = x & x.wrapping_neg();
                let mut m = [0u64; 4];
                m[w] = low;
                nomem(parts.push(m))?;
                x &= !low;
            }
        }
        let mut rest = and_not(all, lits);
        let nl = and(rest, bit(usize::from(b'\n')));
        rest = and_not(rest, nl);
        let mut wd = [0u64; 4];
        if word {
            for v in 0..256usize {
                if !empty(and(rest, bit(v))) && is_word(mode, loc, u32::try_from(v).unwrap_or(0)) {
                    wd[v >> 6] |= 1u64 << (v & 63);
                }
            }
        }
        rest = and_not(rest, wd);
        for m in [nl, wd, rest] {
            if !empty(m) {
                nomem(parts.push(m))?;
            }
        }
        // Below `real` a set's answer is its table bit (`set_matches` reads the table for every
        // byte below 128, or 256 in a single-byte locale), so each set is its four table words.
        // A set equal to the one before splits nothing (a repetition copies its sets).
        let real = if multibyte { 0x80 } else { 256 };
        let mut prev: Option<Mask> = None;
        for s in self.sets.iter() {
            if parts.len() == real {
                break; // every byte is its own class already
            }
            let t = [s.table_word(0), s.table_word(1), s.table_word(2), s.table_word(3)];
            if prev.is_some_and(|p| same(p, t)) {
                continue;
            }
            prev = Some(t);
            for k in 0..parts.len() {
                let c = parts[k];
                let inside = and(c, t);
                if !empty(inside) && !same(inside, c) {
                    parts[k] = inside;
                    nomem(parts.push(and_not(c, t)))?;
                }
            }
        }
        parts.as_mut_slice().sort_unstable_by_key(|m| first(*m));
        let n = u32::try_from(parts.len()).map_err(|_| ReError::ESpace)?;
        let leave = if multibyte { n } else { NONE };
        let mut classes: TryVec<u8> = TryVec::new();
        nomem(classes.resize(256, u8::try_from(leave).unwrap_or(0)))?;
        for (id, m) in parts.iter().enumerate() {
            let id = u8::try_from(id).map_err(|_| ReError::ESpace)?;
            for (w, &mw) in m.iter().enumerate() {
                let mut x = mw;
                while x != 0 {
                    let v = (w << 6) | usize::try_from(x.trailing_zeros()).unwrap_or(0);
                    classes[v & 255] = id;
                    x &= x.wrapping_sub(1);
                }
            }
        }
        if multibyte {
            u8::try_from(leave).map_err(|_| ReError::ESpace)?;
        }
        self.nclasses = n.saturating_add(u32::from(multibyte));
        self.leave = leave;
        self.classes = classes;
        Ok(())
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
        if usize::try_from(total).unwrap_or(usize::MAX) > MAX_NODES {
            return Err(ReError::ESpace);
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

    /// Fill [`Built::first`] and [`Built::empty_ok`]: walk the epsilon closure of the start
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

    /// Whether the program is *one-pass* (D-14.15, RX-ONEPASS). An *entry* is the state a walk
    /// starts a position in: (start, 0), or (`b`, 0) after a `Byte`/`Set` instruction. From each
    /// entry, follow every epsilon edge ([`Built::eps`], assertions taken as passable) to its
    /// *ends*: `Byte`, `Set` or `Match`. The program is one-pass when, for every entry, no state
    /// is reached twice (the paths form a tree, so each end has exactly one path and one capture
    /// effect), the bytes the consuming ends can begin with ([`Built::first_bytes`]' superset:
    /// a set's table, every byte `>= 0x80` in a multibyte locale) are pairwise disjoint, at most
    /// one end is `Match`, and there is no back-reference. Then at any position at most one path
    /// consumes the next character and at most one accepts, so a match with a known start and
    /// end has exactly one accepting path: the single walk's captures are the ones every
    /// selection rule must report. Conservative: assertions may only remove paths, and a state
    /// reached twice is refused even when both paths would die. Programs over 4096 states are
    /// not analysed (regcomp stays linear in the program for the common sizes).
    fn one_pass_check(&self) -> Result<bool, ReError> {
        const LIMIT: usize = 4096;
        let nodes = usize::try_from(self.nodes).map_err(|_| ReError::ESpace)?;
        if self.brefs != 0 || nodes > LIMIT {
            return Ok(false);
        }
        let ninst = u32::try_from(self.insts.len()).map_err(|_| ReError::ESpace)?;
        let mut seen: TryVec<u32> = TryVec::new();
        nomem(seen.resize(nodes, 0))?;
        let mut stack: TryVec<(u32, u32)> = TryVec::new();
        let mut generation = 0u32;
        for e in 0..=ninst {
            let entry = if e == ninst {
                self.start
            } else {
                let i = self.inst(e);
                if i.op != OP_BYTE && i.op != OP_SET {
                    continue;
                }
                i.b
            };
            generation += 1;
            let (mut taken, mut matched) = ([0u64; 4], false);
            stack.clear();
            nomem(stack.push((entry, 0)))?;
            while let Some((pc, j)) = stack.pop() {
                if pc == NONE {
                    continue;
                }
                let Some(s) = seen.get_mut(usize::try_from(self.node(pc, j)).unwrap_or(usize::MAX)) else { return Ok(false) };
                if *s == generation {
                    return Ok(false);
                }
                *s = generation;
                let i = self.inst(pc);
                let mut bytes = [0u64; 4];
                match i.op {
                    OP_BYTE => {
                        if let Some(w) = bytes.get_mut(usize::try_from(i.a >> 6).unwrap_or(usize::MAX)) {
                            *w |= 1 << (i.a & 63);
                        } else {
                            return Ok(false);
                        }
                    }
                    OP_SET => match self.sets.get(usize::try_from(i.a).unwrap_or(usize::MAX)) {
                        Some(set) => {
                            bytes = set.table;
                            if self.multibyte {
                                bytes.iter_mut().skip(2).for_each(|w| *w = u64::MAX);
                            }
                        }
                        None => return Ok(false),
                    },
                    OP_MATCH => {
                        if matched {
                            return Ok(false);
                        }
                        matched = true;
                        continue;
                    }
                    OP_BACKREF => return Ok(false),
                    _ => {
                        for (t, tj) in self.eps(pc, j) {
                            if t != NONE {
                                nomem(stack.push((t, tj)))?;
                            }
                        }
                        continue;
                    }
                }
                if taken.iter().zip(bytes).any(|(t, b)| t & b != 0) {
                    return Ok(false);
                }
                taken.iter_mut().zip(bytes).for_each(|(t, b)| *t |= b);
            }
        }
        Ok(true)
    }

    // ---------------------------------------------------------------------------- bytes

    /// The successors of node `n` (owner `pc`, `j`): the node a consumed character leads to, or
    /// the epsilon successors, as the matchers follow them (`NONE` for none).
    fn succ(&self, n: usize) -> [u32; 2] {
        let (pc, j) = self.owner.get(n).copied().unwrap_or((0, 0));
        let i = self.inst(pc);
        match i.op {
            OP_BYTE | OP_SET => [self.node(i.b, 0), NONE],
            OP_MATCH => [NONE, NONE],
            _ => self.eps(pc, j).map(|(t, tj)| if t == NONE { NONE } else { self.node(t, tj) }),
        }
    }

    /// The predecessor section (D-14.16, the lazy DFA's reverse pass): a presence word, then,
    /// when present (at most [`PRED_CAP`] nodes, the programs the DFA takes), `nodes + 1`
    /// offsets and the predecessor lists, node by node (each edge once).
    fn push_preds(&self, w: &mut TryVec<u32>) -> Result<(), ReError> {
        let nodes = usize::try_from(self.nodes).unwrap_or(usize::MAX);
        if nodes > PRED_CAP {
            return nomem(w.push(0));
        }
        // One pass over the nodes: their successors go to a temporary tail (two words per node)
        // while the offsets count the edges per target; the lists are then filled from the
        // offsets' ends backwards and moved down over the tail.
        nomem(w.reserve(2 + 5 * nodes))?;
        nomem(w.push(1))?;
        let off = w.len();
        nomem(w.resize(off + nodes + 1, 0))?;
        let tail = w.len();
        for n in 0..nodes {
            for t in self.succ(n) {
                nomem(w.push(t))?;
                if let Some(c) = usize::try_from(t).ok().filter(|&t| t < nodes).and_then(|t| w.get_mut(off + t)) {
                    *c += 1;
                }
            }
        }
        let mut total = 0u32;
        for c in w.get_mut(off..off + nodes + 1).unwrap_or(&mut []) {
            total += *c;
            *c = total;
        }
        let list = w.len();
        nomem(w.resize(list + usize::try_from(total).unwrap_or(0), 0))?;
        for k in 0..2 * nodes {
            let t = w.get(tail + k).copied().unwrap_or(NONE);
            let Some(t) = usize::try_from(t).ok().filter(|&t| t < nodes) else { continue };
            let Some(c) = w.get_mut(off + t) else { continue };
            *c -= 1;
            let at = list + usize::try_from(*c).unwrap_or(usize::MAX);
            if let Some(x) = w.get_mut(at) {
                *x = u32::try_from(k / 2).unwrap_or(NONE);
            }
        }
        w.copy_within(list.., tail);
        w.truncate(tail + usize::try_from(total).unwrap_or(0));
        Ok(())
    }

    /// The serialised form, derived tables included (what a `regex_t` block holds;
    /// [`ProgRef::from_bytes`] reads it in place).
    fn to_bytes(&self) -> Result<TryVec<u8>, ReError> {
        let len32 = |n: usize| u32::try_from(n).map_err(|_| ReError::ESpace);
        let mut w: TryVec<u32> = TryVec::new();
        let flags = u32::from(self.icase) | (u32::from(self.multibyte) << 1) | (u32::from(self.can_be_null) << 2) | (u32::from(self.empty_ok) << 3) | (u32::from(self.one_pass) << 4);
        for v in [MAGIC, flags, self.start, self.ngroups, self.brefs, self.nsub, self.nodes] {
            nomem(w.push(v))?;
        }
        for f in self.first {
            nomem(w.push(u32::try_from(f & 0xffff_ffff).unwrap_or(0)))?;
            nomem(w.push(u32::try_from(f >> 32).unwrap_or(0)))?;
        }
        // The set lists: per set, its four counts and then the lists.
        let mut data: TryVec<u32> = TryVec::new();
        let mut offs: TryVec<u32> = TryVec::new();
        for s in self.sets.iter() {
            nomem(offs.push(len32(data.len())?))?;
            for v in [len32(s.ranges.len())?, len32(s.seq.len())?, len32(s.classes.len())?, len32(s.equiv.len())?] {
                nomem(data.push(v))?;
            }
            for &(a, b) in s.ranges.iter().chain(s.seq.iter()) {
                nomem(data.push(a))?;
                nomem(data.push(b))?;
            }
            for &v in s.classes.iter().chain(s.equiv.iter()) {
                nomem(data.push(v))?;
            }
        }
        // One allocation for the whole program (growth by doubling copied it several times).
        let total = [
            HEADER,
            self.report.len(),
            self.insts.len().saturating_mul(5),
            self.chains.len().saturating_mul(3),
            self.scopes.len().saturating_mul(2),
            self.sets.len().saturating_mul(SET_WORDS),
            data.len(),
            self.node_base.len(),
            self.owner.len().saturating_mul(2),
            self.rank.len(),
            self.by_rank.len(),
            self.chain_masks.len().saturating_add(self.masks.len()).saturating_mul(2),
            CLASS_WORDS,
        ]
        .iter()
        .fold(0usize, |a, &b| a.saturating_add(b));
        nomem(w.reserve(total.saturating_sub(w.len())))?;
        for n in [self.report.len(), self.insts.len(), self.chains.len(), self.scopes.len(), self.sets.len(), data.len(), self.masks.len()] {
            nomem(w.push(len32(n)?))?;
        }
        debug_assert_eq!(w.len(), HEADER);
        for &g in self.report.iter() {
            nomem(w.push(g))?;
        }
        for (i, c) in self.insts.iter().zip(self.inst_chain.iter()) {
            for v in [i.op, i.a, i.b, i.c, *c] {
                nomem(w.push(v))?;
            }
        }
        for c in self.chains.iter() {
            for v in [c.parent, c.scope, c.depth] {
                nomem(w.push(v))?;
            }
        }
        for &(a, b) in self.scopes.iter() {
            nomem(w.push(a))?;
            nomem(w.push(b))?;
        }
        for (s, &off) in self.sets.iter().zip(offs.iter()) {
            nomem(w.push(s.flags))?;
            for t in s.table {
                nomem(w.push(u32::try_from(t & 0xffff_ffff).unwrap_or(0)))?;
                nomem(w.push(u32::try_from(t >> 32).unwrap_or(0)))?;
            }
            nomem(w.push(off))?;
        }
        for &v in data.iter().chain(self.node_base.iter()) {
            nomem(w.push(v))?;
        }
        for &(a, b) in self.owner.iter() {
            nomem(w.push(a))?;
            nomem(w.push(b))?;
        }
        for &v in self.rank.iter().chain(self.by_rank.iter()) {
            nomem(w.push(v))?;
        }
        for &(a, b) in self.chain_masks.iter().chain(self.masks.iter()) {
            nomem(w.push(a))?;
            nomem(w.push(b))?;
        }
        // The byte-class section (D-14.12): `nclasses`, `leave`, then the 256 class ids packed
        // four to a word, little byte first.
        nomem(w.push(self.nclasses))?;
        nomem(w.push(self.leave))?;
        let mut ids = [0u8; 256];
        if let Some(c) = self.classes.get(..256) {
            ids.copy_from_slice(c);
        }
        for q in ids.chunks_exact(4) {
            nomem(w.push(u32::from_le_bytes(q.try_into().unwrap_or([0; 4]))))?;
        }
        self.push_preds(&mut w)?;
        let mut bytes = TryVec::new();
        nomem(bytes.resize(w.len().checked_mul(4).ok_or(ReError::ESpace)?, 0))?;
        for (q, v) in bytes.chunks_exact_mut(4).zip(w.iter()) {
            q.copy_from_slice(&v.to_le_bytes());
        }
        Ok(bytes)
    }
}

/// The first word of a serialised program ("Lrx5": derived tables, the byte-class map and the
/// predecessor section included). Bumped from "Lrx3" when the fixed byte-class section was added
/// (D-14.12), from "Lrx4" for the predecessor section (D-14.16); a block never outlives the
/// process, so a format bump costs nothing.
const MAGIC: u32 = 0x3578_724c;
/// The most nodes a program gets a predecessor section for (the lazy DFA's node cap).
pub(crate) const PRED_CAP: usize = super::dfa::NODE_CAP;
/// Header words: magic, flags, start, groups, back-references, subexpressions, nodes, the
/// first-byte map (8 words) and the seven section counts. The byte-class section is a fixed
/// [`CLASS_WORDS`] words appended after the counted sections, so it needs no header count.
pub(crate) const HEADER: usize = 22;
/// Words per set record: flags, the table (8 words), the offset of its lists.
const SET_WORDS: usize = 10;
/// The byte-class section (D-14.12), fixed size: `nclasses`, `leave`, then the 256 class ids
/// packed four per word.
const CLASS_WORDS: usize = 2 + 64;

/// The compiled pattern: its serialised form (header, automaton and derived tables), built once
/// by [`crate::pattern::regex::compile`] and matched through [`Program::view`].
#[derive(Debug, Default)]
pub struct Program {
    bytes: TryVec<u8>,
}

impl Program {
    /// Build the automaton of a parsed pattern, derive the matcher's tables and serialise.
    pub(crate) fn build(parsed: Parsed, loc: &Locale, icase: bool) -> Result<Program, ReError> {
        let bytes = Built::build(parsed, loc, icase)?.to_bytes()?;
        if ProgRef::from_bytes(&bytes).is_none() {
            return Err(ReError::ESpace);
        }
        Ok(Program { bytes })
    }

    /// The serialised form, as a `regex_t` block holds it.
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// The program, read in place.
    pub fn view(&self) -> ProgRef<'_> {
        ProgRef::from_bytes(&self.bytes).unwrap_or_default()
    }

    /// The number of subexpressions.
    pub fn nsub(&self) -> usize {
        self.view().nsub()
    }

    /// Whether the pattern can match the empty string (`can_be_null`).
    pub fn can_be_null(&self) -> bool {
        self.view().can_be_null()
    }

    /// Whether the pattern has back-references.
    pub fn has_backrefs(&self) -> bool {
        self.view().has_backrefs()
    }
}

/// The `n`th little-endian word of `b`.
#[inline]
fn word(b: &[u8], n: usize) -> Option<u32> {
    let at = n.checked_mul(4)?;
    let w: [u8; 4] = b.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(w))
}

/// A section of records of `W` little-endian words, read in place.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Recs<'a, const W: usize>(&'a [u8]);

impl<'a, const W: usize> Recs<'a, W> {
    pub(crate) fn len(&self) -> usize {
        self.0.len() / (4 * W)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.0.len() < 4 * W
    }

    /// The bytes of record `i`.
    #[inline]
    fn rec(&self, i: usize) -> Option<&'a [u8]> {
        let at = i.checked_mul(4 * W)?;
        self.0.get(at..at.checked_add(4 * W)?)
    }

    /// Records `off..off + len`.
    fn sub(&self, off: usize, len: usize) -> Recs<'a, W> {
        let lo = off.saturating_mul(4 * W);
        Recs(self.0.get(lo..lo.saturating_add(len.saturating_mul(4 * W))).unwrap_or(&[]))
    }
}

impl Recs<'_, 1> {
    #[inline]
    pub(crate) fn get(&self, i: usize) -> Option<u32> {
        word(self.0, i)
    }
}

impl Recs<'_, 2> {
    #[inline]
    pub(crate) fn get(&self, i: usize) -> Option<(u32, u32)> {
        let r = self.rec(i)?;
        Some((word(r, 0)?, word(r, 1)?))
    }
}

/// A set of a compiled block: its fixed record (flags, table, lists offset) and the block's lists.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SetRef<'a> {
    rec: &'a [u8],
    data: Recs<'a, 1>,
}

impl<'a> SetRef<'a> {
    /// The set's lists: ranges, collation-sequence ranges, classes, equivalence classes.
    fn lists(&self) -> (Recs<'a, 2>, Recs<'a, 2>, Recs<'a, 1>, Recs<'a, 1>) {
        let off = word(self.rec, SET_WORDS - 1).and_then(|o| usize::try_from(o).ok()).unwrap_or(usize::MAX);
        let n = |k: usize| self.data.get(off.saturating_add(k)).and_then(|v| usize::try_from(v).ok()).unwrap_or(0);
        let (nr, ns, nc, ne) = (n(0), n(1), n(2), n(3));
        // In words, from the first list.
        let w = Recs::<'a, 1>(self.data.0.get(off.saturating_add(4).saturating_mul(4)..).unwrap_or(&[]));
        let ranges = Recs(w.sub(0, nr.saturating_mul(2)).0);
        let seq = Recs(w.sub(nr.saturating_mul(2), ns.saturating_mul(2)).0);
        let base = nr.saturating_add(ns).saturating_mul(2);
        (ranges, seq, w.sub(base, nc), w.sub(base.saturating_add(nc), ne))
    }
}

impl SetDef for SetRef<'_> {
    fn flags(&self) -> u32 {
        word(self.rec, 0).unwrap_or(0)
    }

    #[inline]
    fn table_word(&self, k: usize) -> u64 {
        let lo = word(self.rec, 1 + 2 * k).unwrap_or(0);
        let hi = word(self.rec, 2 + 2 * k).unwrap_or(0);
        u64::from(lo) | (u64::from(hi) << 32)
    }

    fn ranges(&self) -> impl Iterator<Item = (u32, u32)> {
        let r = self.lists().0;
        (0..r.len()).filter_map(move |i| r.get(i))
    }

    fn seq(&self) -> impl Iterator<Item = (u32, u32)> {
        let r = self.lists().1;
        (0..r.len()).filter_map(move |i| r.get(i))
    }

    fn classes(&self) -> impl Iterator<Item = u32> {
        let r = self.lists().2;
        (0..r.len()).filter_map(move |i| r.get(i))
    }

    fn equiv(&self) -> impl Iterator<Item = u32> {
        let r = self.lists().3;
        (0..r.len()).filter_map(move |i| r.get(i))
    }
}

impl SetRef<'_> {
    /// Whether character `c` matches.
    #[inline]
    pub(crate) fn matches(&self, mode: Mode, loc: &Locale, c: u32) -> bool {
        set_matches(self, mode, loc, c)
    }
}

/// The sets of a compiled block.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Sets<'a> {
    fixed: Recs<'a, SET_WORDS>,
    data: Recs<'a, 1>,
}

impl<'a> Sets<'a> {
    #[inline]
    pub(crate) fn get(&self, i: usize) -> Option<SetRef<'a>> {
        Some(SetRef { rec: self.fixed.rec(i)?, data: self.data })
    }
}

/// A compiled program read in place from its serialised bytes (a `regex_t` block or a
/// [`Program`]): nothing is copied or derived per search, and the bytes are only read, so
/// any number of threads may search with one block at once.
#[derive(Clone, Copy, Debug, Default)]
pub struct ProgRef<'a> {
    pub(crate) start: u32,
    pub(crate) ngroups: u32,
    pub(crate) brefs: u32,
    pub(crate) nsub: u32,
    pub(crate) nodes: u32,
    pub(crate) icase: bool,
    pub(crate) multibyte: bool,
    can_be_null: bool,
    pub(crate) empty_ok: bool,
    /// [`Built::one_pass`].
    pub(crate) one_pass: bool,
    first: [u64; 4],
    report: Recs<'a, 1>,
    insts: Recs<'a, 5>,
    pub(crate) scopes: Recs<'a, 2>,
    pub(crate) sets: Sets<'a>,
    node_base: Recs<'a, 1>,
    pub(crate) owner: Recs<'a, 2>,
    pub(crate) rank: Recs<'a, 1>,
    pub(crate) by_rank: Recs<'a, 1>,
    chain_masks: Recs<'a, 2>,
    masks: Recs<'a, 2>,
    /// The byte-class section (D-14.12): `nclasses`, `leave`, 256 packed class ids. Read by the
    /// lazy DFA (RX-T2-1) and the class tests; no non-test code reads it in this groundwork task.
    #[allow(dead_code)]
    class_sec: &'a [u8],
    /// The predecessor section (D-14.16): offsets per node, then the lists; empty when absent.
    pred_offs: Recs<'a, 1>,
    pred_list: Recs<'a, 1>,
}

impl<'a> ProgRef<'a> {
    /// Read a serialised program in place; `None` if `b` is not one (a damaged or foreign
    /// `regex_t`: wrong magic, or sections that do not fill the bytes exactly). Only the
    /// header is checked, in constant time; every later read is bounds-checked, so damage
    /// inside the sections can change answers but never reads outside `b`.
    pub fn from_bytes(b: &'a [u8]) -> Option<ProgRef<'a>> {
        let h = |n: usize| word(b, n);
        if h(0)? != MAGIC || b.len() % 4 != 0 {
            return None;
        }
        let flags = h(1)?;
        let mut first = [0u64; 4];
        for (k, f) in first.iter_mut().enumerate() {
            *f = u64::from(h(7 + 2 * k)?) | (u64::from(h(8 + 2 * k)?) << 32);
        }
        let c = |n: usize| h(n).and_then(|v| usize::try_from(v).ok());
        let (nreport, ninst, nchains, nscopes, nsets, ndata, nmasks) = (c(15)?, c(16)?, c(17)?, c(18)?, c(19)?, c(20)?, c(21)?);
        let nodes = h(6)?;
        let n = usize::try_from(nodes).ok().filter(|&n| n <= MAX_NODES)?;
        // Section sizes in words, in order.
        // The 12 counted sections, then the fixed byte-class section (D-14.12).
        let sizes = [nreport, ninst.checked_mul(5)?, nchains.checked_mul(3)?, nscopes.checked_mul(2)?, nsets.checked_mul(SET_WORDS)?, ndata, ninst, n.checked_mul(2)?, n, n, nchains.checked_mul(2)?, nmasks.checked_mul(2)?, CLASS_WORDS];
        let mut at = HEADER.checked_mul(4)?;
        let mut sec: [&[u8]; 13] = [&[]; 13];
        for (s, &words) in sec.iter_mut().zip(sizes.iter()) {
            let end = at.checked_add(words.checked_mul(4)?)?;
            *s = b.get(at..end)?;
            at = end;
        }
        // The predecessor section (D-14.16): a presence word, then `n + 1` offsets and the lists.
        let (pred_offs, pred_list) = match h(at / 4)? {
            0 => (&[][..], &[][..]),
            1 => {
                let offs = b.get(at + 4..at.checked_add(4)?.checked_add(n.checked_add(1)?.checked_mul(4)?)?)?;
                let edges = usize::try_from(word(offs, n)?).ok()?;
                let lo = at + 4 + offs.len();
                let list = b.get(lo..lo.checked_add(edges.checked_mul(4)?)?)?;
                (offs, list)
            }
            _ => return None,
        };
        at = at.checked_add(4)?.checked_add(pred_offs.len())?.checked_add(pred_list.len())?;
        if at != b.len() {
            return None;
        }
        let [report, insts, _chains, scopes, sets, data, node_base, owner, rank, by_rank, chain_masks, masks, class_sec] = sec;
        Some(ProgRef {
            start: h(2)?,
            ngroups: h(3)?,
            brefs: h(4)?,
            nsub: h(5)?,
            nodes,
            icase: flags & 1 != 0,
            multibyte: flags & 2 != 0,
            can_be_null: flags & 4 != 0,
            empty_ok: flags & 8 != 0,
            one_pass: flags & 16 != 0,
            first,
            report: Recs(report),
            insts: Recs(insts),
            scopes: Recs(scopes),
            sets: Sets { fixed: Recs(sets), data: Recs(data) },
            node_base: Recs(node_base),
            owner: Recs(owner),
            rank: Recs(rank),
            by_rank: Recs(by_rank),
            chain_masks: Recs(chain_masks),
            masks: Recs(masks),
            class_sec,
            pred_offs: Recs(pred_offs),
            pred_list: Recs(pred_list),
        })
    }

    /// The class of byte `b` (D-14.12): bytes that share a class are indistinguishable to the
    /// matcher. 0 if the section is damaged or absent. Read by the lazy DFA (RX-T2-1) and the
    /// class tests; no non-test code reads these accessors in this groundwork task.
    #[inline]
    #[allow(dead_code)]
    pub(crate) fn class_of(&self, b: u8) -> u32 {
        word(self.class_sec, 2 + usize::from(b) / 4).map_or(0, |w| (w >> (8 * (usize::from(b) % 4))) & 0xff)
    }

    /// The number of byte classes.
    #[allow(dead_code)]
    pub(crate) fn nclasses(&self) -> u32 {
        word(self.class_sec, 0).unwrap_or(0)
    }

    /// The LEAVE class id (a multibyte locale's `>= 0x80` bytes), or `None`.
    #[allow(dead_code)]
    pub(crate) fn leave_class(&self) -> Option<u32> {
        word(self.class_sec, 1).filter(|&l| l != NONE)
    }

    /// Whether the program has its predecessor section (at most [`PRED_CAP`] nodes).
    pub(crate) fn has_preds(&self) -> bool {
        !self.pred_offs.is_empty()
    }

    /// The nodes with an edge to node `n` (a consumed character or an epsilon move), for the
    /// lazy DFA's reverse pass (D-14.16); empty when the section is absent or damaged.
    #[inline]
    pub(crate) fn preds(&self, n: u32) -> Recs<'a, 1> {
        let i = usize::try_from(n).unwrap_or(usize::MAX);
        match (self.pred_offs.get(i), self.pred_offs.get(i.saturating_add(1))) {
            (Some(lo), Some(hi)) if lo <= hi => self.pred_list.sub(usize::try_from(lo).unwrap_or(usize::MAX), usize::try_from(hi - lo).unwrap_or(0)),
            _ => Recs::default(),
        }
    }

    /// The number of instructions.
    pub(crate) fn ninsts(&self) -> usize {
        self.insts.len()
    }

    #[inline]
    pub(crate) fn inst(&self, pc: u32) -> Inst {
        let r = usize::try_from(pc).ok().and_then(|i| self.insts.rec(i)).unwrap_or(&[]);
        match (word(r, 0), word(r, 1), word(r, 2), word(r, 3)) {
            (Some(op), Some(a), Some(b), Some(c)) => Inst { op, a, b, c },
            _ => Inst { op: OP_MATCH, a: 0, b: 0, c: 0 },
        }
    }

    #[inline]
    fn chain_of(&self, pc: u32) -> u32 {
        usize::try_from(pc).ok().and_then(|i| self.insts.rec(i)).and_then(|r| word(r, 4)).unwrap_or(NONE)
    }

    /// The node of (`pc`, `j`).
    #[inline]
    pub(crate) fn node(&self, pc: u32, j: u32) -> u32 {
        usize::try_from(pc).ok().and_then(|i| self.node_base.get(i)).map_or(NONE, |b| b.wrapping_add(j))
    }

    /// The group ranges a state's future iterations may overwrite (distinct, non-empty,
    /// innermost first).
    pub(crate) fn masks_of(&self, pc: u32) -> Recs<'a, 2> {
        let c = self.chain_of(pc);
        let Some((off, len)) = usize::try_from(c).ok().and_then(|i| self.chain_masks.get(i)) else { return Recs::default() };
        self.masks.sub(usize::try_from(off).unwrap_or(usize::MAX), usize::try_from(len).unwrap_or(0))
    }

    /// Whether a match can begin with byte `b` (always, when a match can be empty).
    #[inline]
    pub fn can_start_with(&self, b: u8) -> bool {
        self.empty_ok || self.first.get(usize::from(b >> 6)).is_some_and(|w| (w >> (b & 63)) & 1 != 0)
    }

    /// The index of the first byte of `r` that a match can begin with (every byte, when a match
    /// can be empty). Four bytes are tested per branch: the scan is the whole cost of a search
    /// that finds nothing (PERF-REGEXEC-BACKREF).
    #[inline]
    pub fn first_start(&self, r: &[u8]) -> Option<usize> {
        if self.empty_ok {
            return if r.is_empty() { None } else { Some(0) };
        }
        let f = self.first;
        let hit = |b: u8| f.get(usize::from(b >> 6)).is_some_and(|w| (w >> (b & 63)) & 1 != 0);
        let mut base = 0usize;
        let mut chunks = r.chunks_exact(4);
        for c in &mut chunks {
            if let &[a, b, d, e] = c {
                if hit(a) | hit(b) | hit(d) | hit(e) {
                    break;
                }
            }
            base += 4;
        }
        r.get(base..)?.iter().position(|&b| hit(b)).map(|k| base + k)
    }

    /// The number of subexpressions.
    pub fn nsub(&self) -> usize {
        usize::try_from(self.nsub).unwrap_or(0)
    }

    /// The group that holds subexpression `k` (from 1).
    pub(crate) fn group_of(&self, k: u32) -> u32 {
        if self.report.is_empty() { k } else { self.report.get(usize::try_from(k).unwrap_or(0).wrapping_sub(1)).unwrap_or(0) }
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::type_complexity)]
mod class_tests {
    use super::*;
    use crate::locale::Locale;
    use std::vec::Vec;

    /// The matcher-visible signature of byte `v`: its word-ness, newline-ness, literal identity
    /// (if any) and membership of every set, recomputed straight from the serialised program.
    /// Word-ness counts only when the program has a word assertion (`word`).
    fn sig(prog: &ProgRef<'_>, mode: Mode, loc: &Locale, v: u8, is_lit: &[bool; 256], word: bool) -> (bool, bool, Option<u8>, Vec<bool>) {
        let mut sets = Vec::new();
        let mut i = 0;
        while let Some(s) = prog.sets.get(i) {
            sets.push(s.matches(mode, loc, u32::from(v)));
            i += 1;
        }
        (word && is_word(mode, loc, u32::from(v)), v == b'\n', is_lit[usize::from(v)].then_some(v), sets)
    }

    /// D-14.12 invariant: every pair of bytes in one class is indistinguishable to the matcher,
    /// and the class count is exactly the number of distinct behaviours (LEAVE aside).
    fn check(pat: &[u8], flags: i32, loc: &Locale) {
        let prog = super::super::compile(pat, super::super::regcomp_syntax(flags), loc).unwrap();
        let p = prog.view();
        let mode = Mode::of(loc);
        let mut is_lit = [false; 256];
        let mut word = false;
        for pc in 0..p.ninsts() {
            let i = p.inst(u32::try_from(pc).unwrap());
            word |= i.op == OP_ASSERT && (4..=7).contains(&i.a);
            if i.op == OP_BYTE {
                if let Ok(b) = u8::try_from(i.a) {
                    is_lit[usize::from(b)] = true;
                }
            }
        }
        let leave = p.leave_class();
        // In a multibyte locale every byte >= 0x80 is the LEAVE class; in a single-byte locale
        // there is no LEAVE class.
        for v in 0..=255u8 {
            if mode.multibyte() && v >= 0x80 {
                assert_eq!(p.class_of(v), leave.unwrap(), "{pat:?}: byte {v:#x} should be LEAVE");
            } else {
                assert_ne!(Some(p.class_of(v)), leave, "{pat:?}: byte {v:#x} wrongly LEAVE");
            }
        }
        // Forward invariant, and class count = distinct non-LEAVE signatures (+1 for LEAVE).
        let mut reps: Vec<((bool, bool, Option<u8>, Vec<bool>), u32)> = Vec::new();
        for a in 0..=255u8 {
            if Some(p.class_of(a)) == leave {
                continue;
            }
            let sa = sig(&p, mode, loc, a, &is_lit, word);
            for &(ref sr, id) in &reps {
                if *sr == sa {
                    assert_eq!(id, p.class_of(a), "{pat:?}: same signature, different class for {a:#x}");
                }
            }
            if !reps.iter().any(|(sr, _)| *sr == sa) {
                reps.push((sa, p.class_of(a)));
            }
        }
        let expected = u32::try_from(reps.len()).unwrap() + u32::from(leave.is_some());
        assert_eq!(p.nclasses(), expected, "{pat:?}: class count");
        // Within a class the signatures match (the strong direction of the invariant).
        for a in 0..=255u8 {
            for b in a..=255u8 {
                if p.class_of(a) == p.class_of(b) && Some(p.class_of(a)) != leave {
                    assert_eq!(sig(&p, mode, loc, a, &is_lit, word), sig(&p, mode, loc, b, &is_lit, word), "{pat:?}: class {} merges {a:#x} and {b:#x}", p.class_of(a));
                }
            }
        }
    }

    #[test]
    fn classes_group_only_indistinguishable_bytes() {
        use super::super::cflags::{EXTENDED, ICASE, NEWLINE};
        let utf8 = Locale::c_utf8(b"C.UTF-8");
        let cases: &[(&[u8], i32)] = &[
            (b"foobar", 0),
            (br"[a-z]+", EXTENDED),
            (br"\bword\b", 0),
            (br"\<a\w*\>", 0),
            (br"^abc$", 0),
            (b".*x", EXTENDED),
            (b"(cat|dog)", EXTENDED),
            (br"[0-9]+\.[0-9]+", EXTENDED),
            (br"[[:digit:]]+", EXTENDED),
            (b"HELLO", ICASE),
            (b"^a.c$", NEWLINE | EXTENDED),
            (b"x", 0),
        ];
        for &(pat, flags) in cases {
            check(pat, flags, &Locale::C);
            check(pat, flags, &utf8);
        }
    }
}
