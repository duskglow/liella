//! The lazy DFA of the existence scan (D-14.13): does any match start in the window?
//!
//! A state is a *kernel* (the automaton nodes that hold threads after a character was consumed,
//! before their epsilon closure) and the *context* of the character before it (start of the
//! subject, newline, word character, other). The closure is taken when a transition is built,
//! when the next character is known, so the look-ahead assertions (`$`, `\b`, `\>` ...) are
//! decided exactly; the start node is added to every closure (a match may start at every
//! position of a window that reaches the end of the subject). A closure that reaches `Match`
//! answers "yes" at once; the end of the subject takes one more closure without a character.
//!
//! The same nodes are reached as by [`super::exec`]'s fast pass, which keeps one thread per
//! node and position: only the threads' starts differ, and existence needs none. Transitions are
//! built lazily over byte classes ([`ProgRef::class_of`]) and kept in the calling thread's store
//! ([`super::super::regex::ScratchSource::dfa_store`]) between calls, keyed by the per-regcomp
//! serial (D-14.12) and `newline_anchor`.
//!
//! **Falling back.** `None` sends the search to the NFA, which then answers from scratch (exact):
//! back-references, more than [`NODE_CAP`] nodes (the D-14.8 stacked repetitions), a backward or
//! partial window, encodings other than single bytes and UTF-8, a UTF-8 byte `>= 0x80` (the
//! LEAVE class), a kernel larger than [`KMAX`], or a store too small. A full entry is flushed;
//! a third flush in one call, or a second one after fewer than four bytes per state built since
//! the last, falls back (the pattern needs more states than the store holds:
//! `(a|b)*a(a|b){12}`). Each built transition costs about one NFA step and at most one is built
//! per byte, so attempt + fallback cost at most about twice the NFA alone, plus O(subject).
//!
//! **Spans (D-14.16).** For a search that reports positions, [`Dfa::span`] finds the overall
//! match with two more kinds of states in the same entry: a *reverse* pass from the end of the
//! subject back to the window's first start, over the program's predecessor lists
//! ([`ProgRef::preds`], derived at regcomp), whose closures are seeded with the match nodes at
//! every position (a match may end anywhere) and accept where they reach the start node; the
//! leftmost accepting position is the earliest start. An *anchored* forward pass from there
//! (no seeding) keeps the last position whose closure reaches the match, until no thread is
//! left: the longest match. The capture search then runs on that span alone.
#![allow(clippy::arithmetic_side_effects)] // offsets are bounded by the store's length, positions by the subject's

use crate::locale::Locale;

use super::exec::{eps_of, Ctx};
use super::program::{ProgRef, NONE, OP_ASSERT, OP_BACKREF, OP_BYTE, OP_CLOSE, OP_ENTER, OP_MATCH, OP_OPEN, OP_SET};
use super::text::{self, Mode, Unit};

/// The most automaton nodes a program the DFA takes may have.
pub(crate) const NODE_CAP: usize = 1024;
/// The most nodes in one kernel.
pub(crate) const KMAX: usize = 256;

// The store: a global head, the closure scratch, then two entries (LRU).
const G_LRU: usize = 0;
const VIS: usize = 8;
const VIS_WORDS: usize = NODE_CAP / 32;
const WL: usize = VIS + VIS_WORDS;
const NK: usize = WL + NODE_CAP;
/// Words before the entries.
pub(crate) const SCRATCH: usize = NK + KMAX;

// An entry: key, flags, counts, start states (existence, anchored, reverse), canonical contexts
// (forward, reverse), the match nodes, byte -> class, class info, the state hash table, then the
// states (row of `ncls` targets, meta word, kernel length, kernel).
const E_FLAGS: usize = 2;
const E_NCLS: usize = 3;
const E_ALLOC: usize = 4;
const E_NSTATES: usize = 5;
const E_START: usize = 6;
const E_CANON: usize = 11;
const E_ASTART: usize = 16;
const E_RSTART: usize = 21;
const E_RCANON: usize = 26;
/// The number of match nodes (`u32::MAX`: more than [`MNODES`]), then the nodes.
const E_MNODE: usize = 31;
const MNODES: usize = 3;
const E_CMAP: usize = E_MNODE + 1 + MNODES;
const E_CINFO: usize = E_CMAP + 256;
const E_HT: usize = E_CINFO + 256;
const HT: usize = 256;
const E_STATES: usize = E_HT + HT;
const MAX_STATES: u32 = 192; // 3/4 of HT

/// Row values: 0 = not built yet; a state offset with flags; or `MATCH` / `LEAVE`.
const MATCH: u32 = 1 << 31;
const LEAVE: u32 = 1 << 30;
/// Existence rows: state offsets are below this.
const SPECIAL: u32 = 1 << 29;
/// Span rows: the source position accepts (anchored pass: a match ends there; reverse pass: a
/// match starts there).
const ACC: u32 = 1 << 28;
/// Anchored rows: no thread survives the character (the pass stops).
const DEAD: u32 = 1 << 27;
const OFF: u32 = DEAD - 1;

// Contexts. Forward states: the character before the position. Reverse states: the character
// after it (END, END_NE: the end of the subject, without / with NOTEOL).
const BOT: u32 = 0;
const BOT_NB: u32 = 1;
const END: u32 = 0;
const END_NE: u32 = 1;
const NL: u32 = 2;
const WORD: u32 = 3;
const OTHER: u32 = 4;

// State kinds (meta bits 4-5): the existence scan (the start node joins every closure), the
// anchored forward pass (no seeding), the reverse pass (the match nodes join every closure).
const K_EXIST: u32 = 0;
const K_ANCH: u32 = 1 << 4;
const K_REV: u32 = 2 << 4;
const M_KIND: u32 = 3 << 4;
const M_CX: u32 = 0xf;

// Class info bits: representative byte, newline, word, LEAVE, the context after it.
const CI_LEAVE: u32 = 1 << 10;
const CI_CTX_SHIFT: u32 = 12;

// Meta word: context and kind, the end-of-subject answers (computed, result) without / with
// NOTEOL, the reverse pass's start-of-subject answers without / with NOTBOL.
const M_CTX: u32 = 0xff;
const M_EOT: [u32; 2] = [1 << 8, 1 << 10];
const M_BEG: [u32; 2] = [1 << 12, 1 << 14];

/// A closure's result: whether it accepts (existence and anchored: reaches the match; reverse:
/// reaches the start node), and the next kernel's length (in the scratch).
struct Cl {
    hit: bool,
    nk: usize,
}

/// Flushes in one pass (the blow-up bound).
struct Flush {
    n: u32,
    since: usize,
}

/// One search's view (the matcher's inputs).
pub(crate) struct Dfa<'a> {
    pub(crate) prog: &'a ProgRef<'a>,
    pub(crate) mode: Mode,
    pub(crate) loc: &'a Locale,
    pub(crate) ctx: Ctx,
}

#[cfg(test)]
pub(crate) mod counters {
    std::thread_local! {
        /// Transitions built (closures over a character), flushes, fallbacks.
        pub(crate) static BUILT: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
        pub(crate) static FLUSHES: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
        pub(crate) static FALLBACKS: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
        /// Span searches whose forward pass found no match from the reverse pass's start (a
        /// bug: the NFA then answers, so only this counter shows it).
        pub(crate) static DISAGREE: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
        /// Subexpressions collected by the one-pass walk on a span the DFA found (RE-SEARCH).
        pub(crate) static SPAN_ONE_PASS: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
    }
    pub(crate) fn bump(c: &'static std::thread::LocalKey<core::cell::Cell<u64>>) {
        c.with(|v| v.set(v.get() + 1));
    }
}

/// Test-only planted bugs (D-14.16 red-before-green): the reverse pass ignores NOTBOL, or stops
/// at its first accepting position.
#[cfg(test)]
pub(crate) mod plant {
    std::thread_local! {
        pub(crate) static IGNORE_NOTBOL: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
        pub(crate) static FIRST_ACCEPT: core::cell::Cell<bool> = const { core::cell::Cell::new(false) };
    }
    pub(crate) fn on(c: &'static std::thread::LocalKey<core::cell::Cell<bool>>) -> bool {
        c.with(core::cell::Cell::get)
    }
}

fn u(v: u32) -> usize {
    usize::try_from(v).unwrap_or(usize::MAX)
}

fn w(v: usize) -> u32 {
    u32::try_from(v).unwrap_or(u32::MAX)
}

fn get(e: &[u32], i: usize) -> u32 {
    e.get(i).copied().unwrap_or(0)
}

fn set(e: &mut [u32], i: usize, v: u32) {
    if let Some(x) = e.get_mut(i) {
        *x = v;
    }
}

impl Dfa<'_> {
    /// Whether a match starts at or after `first` in `subject` (to its end): `Some(answer)`, or
    /// `None` when the NFA must answer (see the module notes).
    pub(crate) fn exists(&self, subject: &[u8], first: usize, store: &mut [u32], serial: u64) -> Option<bool> {
        let r = self.exists_in(subject, first, store, serial);
        #[cfg(test)]
        if r.is_none() {
            counters::bump(&counters::FALLBACKS);
        }
        r
    }

    /// The match that starts in `first..=last` (the earliest start, then the longest; D-14.16):
    /// `Some(Some((start, end)))`, `Some(None)` for none, or `None` when the NFA must answer.
    /// The reverse pass runs from the end of the subject to `first` and keeps the last (the
    /// leftmost) accepting position; the anchored forward pass from there keeps the last
    /// position where the match is reached, until no thread survives. A window of one start
    /// (`re_match`) takes the forward pass alone.
    pub(crate) fn span(&self, subject: &[u8], first: usize, last: usize, store: &mut [u32], serial: u64) -> Option<Option<(usize, usize)>> {
        let r = self.span_in(subject, first, last, store, serial);
        #[cfg(test)]
        if r.is_none() {
            counters::bump(&counters::FALLBACKS);
        }
        r
    }

    fn span_in(&self, subject: &[u8], first: usize, last: usize, store: &mut [u32], serial: u64) -> Option<Option<(usize, usize)>> {
        if first > subject.len() || first > last || !self.prog.has_preds() {
            return None;
        }
        // No match starts before the first byte that can begin one (any position, when the
        // match can be empty): an independent scan per byte, no dependent chain; no such byte
        // in the window is no match, and the reverse pass stops there.
        let (len, hi) = (subject.len(), last.min(subject.len()));
        let lo = if self.prog.empty_ok {
            first
        } else {
            match self.prog.first_start(subject.get(first..(hi + 1).min(len))?) {
                Some(k) => first + k,
                None => return Some(None),
            }
        };
        let (scr, e, ncls) = self.entry(subject, lo, store, serial)?;
        let start = if lo == hi {
            lo
        } else {
            match self.reverse(e, scr, ncls, subject, lo)? {
                Some(p) if p <= hi => p,
                _ => return Some(None),
            }
        };
        match self.longest(e, scr, ncls, subject, start)? {
            Some(end) => Some(Some((start, end))),
            // The reverse pass accepted `start`: a forward pass that disagrees hands the
            // search to the NFA rather than answer.
            None if lo == hi => Some(None),
            None => {
                #[cfg(test)]
                counters::bump(&counters::DISAGREE);
                None
            }
        }
    }

    /// The store entry for this program (found, or set up in the least recently used one), the
    /// closure scratch, and the number of classes; `None` when the DFA cannot take the search.
    fn entry<'s>(&self, subject: &[u8], first: usize, store: &'s mut [u32], serial: u64) -> Option<(&'s mut [u32], &'s mut [u32], usize)> {
        let prog = self.prog;
        if prog.brefs != 0 || u(prog.nodes) > NODE_CAP || first > subject.len() || serial == 0 {
            return None;
        }
        match self.mode {
            Mode::Bytes => {}
            Mode::Multi(crate::text::Encoding::Utf8) if prog.leave_class().is_some() => {}
            Mode::Multi(_) => return None,
        }
        let ncls = u(prog.nclasses());
        if ncls == 0 || ncls > 256 {
            return None;
        }
        let (scr, entries) = store.split_at_mut_checked(SCRATCH)?;
        let esize = (entries.len() / 2).min(u(OFF));
        // Room for two small states at least (a larger kernel that does not fit falls back).
        if esize < E_STATES + 2 * (ncls + 2) + 16 {
            return None;
        }
        let key = [w(usize::try_from(serial & 0xffff_ffff).unwrap_or(0)), w(usize::try_from(serial >> 32).unwrap_or(0)), 1 | (u32::from(self.ctx.newline_anchor) << 1)];
        let lru = u(get(scr, G_LRU) & 1);
        let hit = (0..2).find(|&i| entries.get(i * esize..i * esize + 3) == Some(&key[..]));
        let i = hit.unwrap_or(1 - lru);
        set(scr, G_LRU, w(i));
        let e = entries.get_mut(i * esize..(i + 1) * esize)?;
        if hit.is_none() {
            self.init(e, key, ncls);
        }
        Some((scr, e, ncls))
    }

    /// The forward context before position `p` (canonical): the start of the subject, or the
    /// character before; `None` for a LEAVE byte.
    fn before(&self, e: &[u32], subject: &[u8], p: usize) -> Option<u32> {
        let c = if p == 0 {
            if self.ctx.not_bol { BOT_NB } else { BOT }
        } else {
            let info = get(e, E_CINFO + u(get(e, E_CMAP + usize::from(*subject.get(p - 1)?))));
            if info & CI_LEAVE != 0 {
                return None;
            }
            (info >> CI_CTX_SHIFT) & 0xf
        };
        Some(get(e, E_CANON + u(c)))
    }

    /// The start state of `kind` for canonical context `ctx` (slot `at`), built on first use.
    fn start(&self, e: &mut [u32], scr: &mut [u32], at: usize, kind: u32, ctx: u32) -> Option<usize> {
        match u(get(e, at + u(ctx))) {
            0 => {
                let nk = if kind == K_ANCH {
                    set(scr, NK, self.prog.node(self.prog.start, 0));
                    1
                } else {
                    0
                };
                let s = self.add(e, scr, nk, kind | ctx).ok()?;
                set(e, at + u(ctx), w(s));
                Some(s)
            }
            s => Some(s),
        }
    }

    fn exists_in(&self, subject: &[u8], first: usize, store: &mut [u32], serial: u64) -> Option<bool> {
        let (scr, e, ncls) = self.entry(subject, first, store, serial)?;
        let c0 = self.before(e, subject, first)?;
        let mut s = self.start(e, scr, E_START, K_EXIST, c0)?;
        let len = subject.len();
        let mut p = first;
        let mut fl = Flush { n: 0, since: first };
        loop {
            // The hot loop: one class load and one row load per byte.
            let mut t = 0;
            while let Some(&b) = subject.get(p) {
                t = get(e, s + u(get(e, E_CMAP + usize::from(b))));
                if t.wrapping_sub(1) >= SPECIAL - 1 {
                    break;
                }
                s = u(t);
                p += 1;
            }
            if p >= len {
                return Some(self.at_end(e, scr, s, ncls));
            }
            if t == MATCH {
                return Some(true);
            }
            if t != 0 {
                return None; // LEAVE: a multibyte character
            }
            let c = u(get(e, E_CMAP + usize::from(*subject.get(p)?)));
            match self.step(e, scr, s, ncls, c, &mut fl, p)? {
                MATCH => return Some(true),
                LEAVE => return None,
                t => s = u(t),
            }
            p += 1;
        }
    }

    /// The anchored forward pass from `from`: the end of the longest match starting there.
    fn longest(&self, e: &mut [u32], scr: &mut [u32], ncls: usize, subject: &[u8], from: usize) -> Option<Option<usize>> {
        let c0 = self.before(e, subject, from)?;
        let mut s = self.start(e, scr, E_ASTART, K_ANCH, c0)?;
        let len = subject.len();
        let (mut p, mut end) = (from, None);
        let mut fl = Flush { n: 0, since: from };
        loop {
            let mut t = 0;
            while let Some(&b) = subject.get(p) {
                t = get(e, s + u(get(e, E_CMAP + usize::from(b))));
                if t.wrapping_sub(1) >= DEAD - 1 {
                    break;
                }
                s = u(t);
                p += 1;
            }
            if p >= len {
                if self.at_end(e, scr, s, ncls) {
                    end = Some(len);
                }
                return Some(end);
            }
            if t == 0 {
                let c = u(get(e, E_CMAP + usize::from(*subject.get(p)?)));
                t = self.step(e, scr, s, ncls, c, &mut fl, p)?;
            }
            if t >= LEAVE {
                return None;
            }
            if t & ACC != 0 {
                end = Some(p);
            }
            if t & DEAD != 0 {
                return Some(end);
            }
            s = u(t & OFF);
            p += 1;
        }
    }

    /// The reverse pass from the end of the subject to `first`: the leftmost position where a
    /// match starts (reading every byte: a later accepting position never ends the pass).
    fn reverse(&self, e: &mut [u32], scr: &mut [u32], ncls: usize, subject: &[u8], first: usize) -> Option<Option<usize>> {
        let c0 = get(e, E_RCANON + u(if self.ctx.not_eol { END_NE } else { END }));
        let mut s = self.start(e, scr, E_RSTART, K_REV, c0)?;
        let (mut p, mut acc) = (subject.len(), None);
        let mut fl = Flush { n: 0, since: p };
        loop {
            let mut t = 0;
            while p > first {
                let b = *subject.get(p - 1)?;
                t = get(e, s + u(get(e, E_CMAP + usize::from(b))));
                if t.wrapping_sub(1) >= LEAVE - 1 {
                    break;
                }
                if t & ACC != 0 {
                    acc = Some(p);
                    #[cfg(test)]
                    if plant::on(&plant::FIRST_ACCEPT) {
                        return Some(acc);
                    }
                }
                s = u(t & OFF);
                p -= 1;
            }
            if p == first {
                // Does a match start at `first`? Its look-behind is the subject's start or the
                // character before.
                let hit = if first == 0 {
                    self.at_begin(e, scr, s, ncls)
                } else {
                    let c = u(get(e, E_CMAP + usize::from(*subject.get(first - 1)?)));
                    let t = match get(e, s + c) {
                        0 => self.step(e, scr, s, ncls, c, &mut fl, first)?,
                        t => t,
                    };
                    if t >= LEAVE {
                        return None;
                    }
                    t & ACC != 0
                };
                return Some(if hit { Some(first) } else { acc });
            }
            if t != 0 {
                return None; // LEAVE: a multibyte character
            }
            let c = u(get(e, E_CMAP + usize::from(*subject.get(p - 1)?)));
            let t = self.step(e, scr, s, ncls, c, &mut fl, p)?;
            if t >= LEAVE {
                return None;
            }
            if t & ACC != 0 {
                acc = Some(p);
                #[cfg(test)]
                if plant::on(&plant::FIRST_ACCEPT) {
                    return Some(acc);
                }
            }
            s = u(t & OFF);
            p -= 1;
        }
    }

    /// Build state `s`'s transition on class `c` (at position `p`, for the blow-up bound): the
    /// row value, stored in the row unless a flush dropped `s`. `None`: fall back.
    #[allow(clippy::too_many_arguments)] // one transition's inputs
    fn step(&self, e: &mut [u32], scr: &mut [u32], s: usize, ncls: usize, c: usize, fl: &mut Flush, p: usize) -> Option<u32> {
        let info = get(e, E_CINFO + c);
        if info & CI_LEAVE != 0 {
            return Some(LEAVE);
        }
        #[cfg(test)]
        counters::bump(&counters::BUILT);
        let meta = get(e, s + ncls);
        let (kind, sctx) = (meta & M_KIND, meta & M_CX);
        let raw = (info >> CI_CTX_SHIFT) & 0xf;
        let (before, after) = if kind == K_REV { (raw, sctx) } else { (sctx, raw) };
        let cl = self.closure(e, scr, s, ncls, before, after, Some(u8::try_from(info & 0xff).unwrap_or(0)))?;
        if kind == K_EXIST && cl.hit {
            set(e, s + c, MATCH);
            return Some(MATCH);
        }
        let flag = if kind != K_EXIST && cl.hit { ACC } else { 0 };
        if kind == K_ANCH && cl.nk == 0 {
            set(e, s + c, flag | DEAD);
            return Some(flag | DEAD);
        }
        let nctx = kind | get(e, if kind == K_REV { E_RCANON } else { E_CANON } + u(raw));
        match self.add(e, scr, cl.nk, nctx) {
            Ok(t) => {
                set(e, s + c, w(t) | flag);
                Some(w(t) | flag)
            }
            Err(states) => {
                // Full: flush the entry, keep going from the new state.
                #[cfg(test)]
                counters::bump(&counters::FLUSHES);
                fl.n += 1;
                if fl.n > 2 || (fl.n > 1 && p.abs_diff(fl.since) < 4 * u(states)) {
                    return None;
                }
                fl.since = p;
                Self::flush(e);
                Some(w(self.add(e, scr, cl.nk, nctx).ok()?) | flag)
            }
        }
    }

    /// Whether a forward state's closure reaches the match at the end of the subject (cached
    /// per NOTEOL).
    fn at_end(&self, e: &mut [u32], scr: &mut [u32], s: usize, ncls: usize) -> bool {
        let after = if self.ctx.not_eol { END_NE } else { END };
        self.cached(e, scr, s, ncls, M_EOT.get(usize::from(self.ctx.not_eol)).copied().unwrap_or(0), None, after)
    }

    /// Whether a reverse state's closure reaches the start node at the start of the subject
    /// (cached per NOTBOL).
    fn at_begin(&self, e: &mut [u32], scr: &mut [u32], s: usize, ncls: usize) -> bool {
        #[allow(unused_mut)]
        let mut nb = self.ctx.not_bol;
        #[cfg(test)]
        if plant::on(&plant::IGNORE_NOTBOL) {
            nb = false;
        }
        let before = if nb { BOT_NB } else { BOT };
        self.cached(e, scr, s, ncls, M_BEG.get(usize::from(nb)).copied().unwrap_or(0), Some(before), 0)
    }

    /// A closure at an edge of the subject, its answer cached in the meta word under `bits`
    /// (computed, then the answer one bit up). `before`: a reverse state's look-behind; else the
    /// forward state's own context, with `after`.
    #[allow(clippy::too_many_arguments)] // one closure's inputs
    fn cached(&self, e: &mut [u32], scr: &mut [u32], s: usize, ncls: usize, bits: u32, before: Option<u32>, after: u32) -> bool {
        let meta = get(e, s + ncls);
        if meta & bits != 0 {
            return meta & (bits << 1) != 0;
        }
        let (b, a) = match before {
            Some(b) => (b, meta & M_CX),
            None => (meta & M_CX, after),
        };
        let m = self.closure(e, scr, s, ncls, b, a, None).is_some_and(|cl| cl.hit);
        set(e, s + ncls, meta | bits | if m { bits << 1 } else { 0 });
        m
    }

    /// Set up entry `e` for this program.
    fn init(&self, e: &mut [u32], key: [u32; 3], ncls: usize) {
        let prog = self.prog;
        let (mut bol, mut eol, mut word) = (false, false, false);
        for pc in 0..prog.ninsts() {
            let i = prog.inst(w(pc));
            if i.op == OP_ASSERT {
                match i.a {
                    0 | 2 => bol = true,
                    1 | 3 => eol = true,
                    4..=7 => word = true,
                    _ => {}
                }
            }
        }
        // Contexts the program cannot tell apart are one (forward: before; reverse: after).
        let canon = |edge: bool| match (edge, word) {
            (false, false) => [OTHER; 5],
            (false, true) => [OTHER, OTHER, OTHER, WORD, OTHER],
            (true, false) => [BOT, BOT_NB, NL, OTHER, OTHER],
            (true, true) => [BOT, BOT_NB, NL, WORD, OTHER],
        };
        for (k, (f, r)) in canon(bol).iter().zip(canon(eol).iter()).enumerate() {
            set(e, E_CANON + k, *f);
            set(e, E_RCANON + k, *r);
        }
        // The match nodes (the reverse pass's seeds).
        let mut m = 0usize;
        for n in 0..u(prog.nodes) {
            let (pc, _) = prog.owner.get(n).unwrap_or((0, 0));
            if prog.inst(pc).op == OP_MATCH {
                if m < MNODES {
                    set(e, E_MNODE + 1 + m, w(n));
                }
                m += 1;
            }
        }
        set(e, E_MNODE, if m > MNODES { u32::MAX } else { w(m) });
        let leave = prog.leave_class();
        let mut seen = [false; 256];
        for b in 0..=255u8 {
            let c = prog.class_of(b);
            set(e, E_CMAP + usize::from(b), c);
            let Some(slot) = seen.get_mut(u(c)) else { continue };
            if *slot {
                continue;
            }
            *slot = true;
            let mut info = u32::from(b);
            if Some(c) == leave {
                info |= CI_LEAVE;
            } else {
                let ch = match text::unit_at(self.mode, &[b], 0) {
                    Unit::Char(ch, _) => Some(ch),
                    _ => None,
                };
                let nl = b == b'\n';
                let wd = word && ch.is_some_and(|ch| text::is_word(self.mode, self.loc, ch));
                info |= (if nl { NL } else if wd { WORD } else { OTHER }) << CI_CTX_SHIFT;
            }
            set(e, E_CINFO + u(c), info);
        }
        set(e, E_NCLS, w(ncls));
        Self::flush(e);
        // The key last: an entry is valid only once it is complete.
        for (k, v) in key.iter().enumerate() {
            set(e, k, *v);
        }
        debug_assert_eq!(get(e, E_FLAGS), key[2]);
    }

    /// Drop every state of entry `e`.
    fn flush(e: &mut [u32]) {
        set(e, E_ALLOC, w(E_STATES));
        set(e, E_NSTATES, 0);
        for at in [E_START, E_ASTART, E_RSTART] {
            if let Some(h) = e.get_mut(at..at + 5) {
                h.fill(0);
            }
        }
        if let Some(h) = e.get_mut(E_HT..E_HT + HT) {
            h.fill(0);
        }
    }

    /// The state with kernel `scr[NK..NK + nk]` (sorted, distinct) and context and kind `ctx`:
    /// found, or added. `Err(states)` when the entry is full.
    fn add(&self, e: &mut [u32], scr: &[u32], nk: usize, ctx: u32) -> Result<usize, u32> {
        let ncls = u(get(e, E_NCLS));
        let kernel = scr.get(NK..NK + nk).unwrap_or(&[]);
        let mut h: u32 = 0x811c_9dc5 ^ ctx;
        for &n in kernel {
            h = (h ^ n).wrapping_mul(0x0100_0193);
        }
        let mut slot = u(h ^ (h >> 16)) % HT;
        loop {
            let v = u(get(e, E_HT + slot));
            if v == 0 {
                break;
            }
            if get(e, v + ncls) & M_CTX == ctx && u(get(e, v + ncls + 1)) == nk && e.get(v + ncls + 2..v + ncls + 2 + nk) == Some(kernel) {
                return Ok(v);
            }
            slot = (slot + 1) % HT;
        }
        let states = get(e, E_NSTATES);
        let at = u(get(e, E_ALLOC));
        let end = at + ncls + 2 + nk;
        if states >= MAX_STATES || end > e.len() {
            return Err(states);
        }
        if let Some(row) = e.get_mut(at..at + ncls) {
            row.fill(0);
        }
        if let Some(l) = self.prog.leave_class() {
            set(e, at + u(l), LEAVE);
        }
        set(e, at + ncls, ctx);
        set(e, at + ncls + 1, w(nk));
        if let Some(k) = e.get_mut(at + ncls + 2..end) {
            k.copy_from_slice(kernel);
        }
        set(e, E_HT + slot, w(at));
        set(e, E_ALLOC, w(end));
        set(e, E_NSTATES, states + 1);
        Ok(at)
    }

    /// Whether assertion `kind` holds between the character before (`before`: BOT, BOT_NB, NL,
    /// WORD, OTHER) and the one after (`after`: END, END_NE, NL, WORD, OTHER).
    fn holds(&self, kind: u32, before: u32, after: u32) -> bool {
        let nla = self.ctx.newline_anchor;
        match kind {
            0 => before == BOT || (nla && before == NL),
            1 => after == END || (nla && after == NL),
            2 => before == BOT || before == BOT_NB,
            3 => after == END || after == END_NE,
            _ => {
                let (b, a) = (before == WORD, after == WORD);
                match kind {
                    4 => b != a,
                    5 => b == a,
                    6 => !b && a,
                    _ => b && !a,
                }
            }
        }
    }

    /// Whether consuming instruction `inst` accepts the class whose representative is `rep`.
    fn consumes(&self, inst: super::program::Inst, rep: Option<u8>) -> bool {
        let Some(b) = rep else { return false };
        match inst.op {
            OP_BYTE => u32::from(b) == inst.a,
            OP_SET => match text::unit_at(self.mode, &[b], 0) {
                Unit::Char(ch, _) => self.prog.sets.get(u(inst.a)).is_some_and(|st| st.matches(self.mode, self.loc, ch)),
                _ => false,
            },
            _ => false,
        }
    }

    /// The closure of state `s` at a position between `before` and `after`, consuming `rep`'s
    /// class next (`None`: an edge of the subject). Forward states: the kernel (and the start
    /// node for the existence scan) and its epsilon successors; `hit` = the match is reached
    /// (the existence scan stops there); the next kernel = the targets of the consuming nodes
    /// that accept `rep`. Reverse states: the kernel and the match nodes, and their epsilon
    /// predecessors ([`ProgRef::preds`]); `hit` = the start node is reached; the next kernel =
    /// the consuming predecessors that accept `rep` (the character before). The next kernel is
    /// in `scr[NK..]`, sorted and distinct. `None`: too many nodes (fall back).
    #[allow(clippy::too_many_arguments)] // one closure's inputs
    fn closure(&self, e: &[u32], scr: &mut [u32], s: usize, ncls: usize, before: u32, after: u32, rep: Option<u8>) -> Option<Cl> {
        let prog = self.prog;
        let nodes = u(prog.nodes);
        let kind = get(e, s + ncls) & M_KIND;
        let klen = u(get(e, s + ncls + 1));
        scr.get_mut(VIS..VIS + VIS_WORDS)?.fill(0);
        let (mut top, mut nk, mut hit) = (0usize, 0usize, false);
        let push = |scr: &mut [u32], n: u32, top: &mut usize| {
            let k = u(n);
            if n == NONE || k >= nodes {
                return;
            }
            let (wi, bit) = (VIS + k / 32, 1u32 << (k % 32));
            let v = get(scr, wi);
            if v & bit == 0 {
                set(scr, wi, v | bit);
                set(scr, WL + *top, n);
                *top += 1;
            }
        };
        let start = prog.node(prog.start, 0);
        match kind {
            K_EXIST => push(scr, start, &mut top),
            K_REV => {
                for k in 0..u(get(e, E_MNODE)).min(MNODES) {
                    push(scr, get(e, E_MNODE + 1 + k), &mut top);
                }
            }
            _ => {}
        }
        for i in 0..klen {
            push(scr, get(e, s + ncls + 2 + i), &mut top);
        }
        while top > 0 {
            top -= 1;
            let n = get(scr, WL + top);
            if kind == K_REV {
                hit |= n == start;
                let preds = prog.preds(n);
                for i in 0..preds.len() {
                    let m = preds.get(i).unwrap_or(NONE);
                    let (pc, _) = prog.owner.get(u(m)).unwrap_or((0, 0));
                    let inst = prog.inst(pc);
                    match inst.op {
                        OP_BYTE | OP_SET => {
                            if self.consumes(inst, rep) && u(m) < nodes {
                                if nk >= KMAX {
                                    return None;
                                }
                                set(scr, NK + nk, m);
                                nk += 1;
                            }
                        }
                        OP_BACKREF => return None,
                        OP_ASSERT => {
                            if self.holds(inst.a, before, after) {
                                push(scr, m, &mut top);
                            }
                        }
                        _ => push(scr, m, &mut top),
                    }
                }
                continue;
            }
            let (pc, j) = prog.owner.get(u(n)).unwrap_or((0, 0));
            let inst = prog.inst(pc);
            let target = match inst.op {
                OP_MATCH => {
                    if kind == K_EXIST {
                        return Some(Cl { hit: true, nk: 0 });
                    }
                    hit = true;
                    None
                }
                OP_BYTE | OP_SET => self.consumes(inst, rep).then(|| prog.node(inst.b, 0)),
                OP_BACKREF => return None,
                OP_OPEN | OP_CLOSE => {
                    push(scr, prog.node(inst.b, j), &mut top);
                    None
                }
                OP_ASSERT => {
                    if self.holds(inst.a, before, after) {
                        push(scr, prog.node(inst.b, j), &mut top);
                    }
                    None
                }
                OP_ENTER => {
                    push(scr, prog.node(inst.b, super::program::enter_j(inst, j)), &mut top);
                    None
                }
                _ => {
                    for (t, tj) in eps_of(inst, j) {
                        if t != NONE {
                            push(scr, prog.node(t, tj), &mut top);
                        }
                    }
                    None
                }
            };
            if let Some(t) = target.filter(|&t| t != NONE && u(t) < nodes) {
                if nk >= KMAX {
                    return None;
                }
                set(scr, NK + nk, t);
                nk += 1;
            }
        }
        let k = scr.get_mut(NK..NK + nk)?;
        k.sort_unstable();
        let mut d = 0;
        for i in 0..nk {
            let v = get(k, i);
            if d == 0 || get(k, d - 1) != v {
                set(k, d, v);
                d += 1;
            }
        }
        Some(Cl { hit, nk: d })
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions)]
    use super::{counters, plant, E_STATES, NODE_CAP, SCRATCH};
    use crate::locale::Locale;
    use crate::pattern::regex::{cflags, compile, exec, exists_with, regcomp_syntax, search_with, Accel, Options, Program, ScratchMem, ScratchNeed, ScratchSource, Window};
    use std::vec::Vec;

    struct Store(Vec<u32>);

    impl ScratchSource for Store {
        fn scratch(&mut self, _: &ScratchNeed) -> Option<ScratchMem<'_>> {
            None
        }
        fn dfa_store(&mut self) -> Option<&mut [u32]> {
            Some(&mut self.0)
        }
    }

    fn get(c: &'static std::thread::LocalKey<core::cell::Cell<u64>>) -> u64 {
        c.with(core::cell::Cell::get)
    }

    /// The matcher's answer (no DFA, no prefilter).
    fn nfa(prog: &Program, loc: &Locale, s: &[u8], first: usize, opts: &Options) -> bool {
        let opts = Options { accel: Accel::OFF, ..*opts };
        exec(prog, loc, s, Window { first, last: s.len(), backward: false }, &opts, false).unwrap().is_some()
    }

    fn subjects() -> Vec<Vec<u8>> {
        let mut all: Vec<Vec<u8>> = std::vec![Vec::new()];
        let mut level = all.clone();
        for _ in 0..3 {
            let mut next = Vec::new();
            for s in &level {
                for &c in b"ab\nx1_" {
                    let mut t = s.clone();
                    t.push(c);
                    next.push(t);
                }
            }
            all.extend(next.iter().cloned());
            level = next;
        }
        for s in [&b"abc cd ab"[..], b"xa\nbx\n", b"A B Cab", b"aab_ba\n\nb", b"\xc3\xa9a", b"a\xc3\xa9", b"12.3 a", b"ba\xe9b"] {
            all.push(s.to_vec());
        }
        all
    }

    const ERE: &[&[u8]] = &[
        b"a", b"ab|cd", b"^a", b"a$", b"^$", b"x*", b"(a|b)*c", b"a.b", b"[a-c]+x", b"\\bab\\b", b"\\<a", b"b\\>", b"\\Bb", b"a\\B",
        b"^ab$|b\\b", b"(^|x)a", b"a($|b)", b"[[:digit:]]+\\.?[0-9]", b"(a|ab)(c|bcd)", b"a{2,3}", b"(a*)*b", b"(|a)+b", b".*x", b"[^a\n]b",
        b"\\`a", b"a\\'", b"(a|^b)c", b"a\n?^b", b"\xc3\xa9", b"\xc3\xa9?a", b"_\\w", b"[[:space:]]b$", b"\\<\\>",
    ];
    const BRE: &[&[u8]] = &[b"a\\{2\\}", b"\\(ab\\)*c", b"^a*$", b"a\\|b", b"\\(a\\)\\1", b"x\\(^a\\)", b"a$b"];

    /// Every answer the DFA gives (a full store, and a tiny one that flushes and falls back) is
    /// the matcher's, over BRE and ERE, REG_ICASE, REG_NEWLINE (and `newline_anchor` flipped on
    /// the same program: part of the store's key), NOTBOL/NOTEOL, starts after the subject's
    /// start, C and UTF-8.
    #[test]
    fn the_dfa_answers_as_the_matcher() {
        // Trace: D-14.13
        let utf8 = Locale::c_utf8(b"C.UTF-8");
        let subjects = subjects();
        let tiny = SCRATCH + 2 * (E_STATES + 64);
        let mut stores = [Store(std::vec![0; 8192]), Store(std::vec![0; tiny])];
        let mut serial = 0u64;
        let (mut decided, mut total) = (0u64, 0u64);
        for (pats, ext) in [(ERE, cflags::EXTENDED), (BRE, 0)] {
            for pat in pats {
                for loc in [&Locale::C, &utf8] {
                    for extra in [0, cflags::ICASE, cflags::NEWLINE, cflags::ICASE | cflags::NEWLINE] {
                        let Ok(prog) = compile(pat, regcomp_syntax(ext | extra), loc) else { continue };
                        serial += 1;
                        for s in &subjects {
                            for first in 0..=s.len().min(2) {
                                for (not_bol, not_eol) in [(false, false), (true, false), (false, true)] {
                                    for nl in [extra & cflags::NEWLINE != 0, extra & cflags::NEWLINE == 0] {
                                        let opts = Options { not_bol, not_eol, newline_anchor: nl, budget: 1 << 20, accel: Accel { prefilter: false, dfa: true, cap: 0 } };
                                        let want = nfa(&prog, loc, s, first, &opts);
                                        for st in &mut stores {
                                            let f0 = get(&counters::FALLBACKS);
                                            let got = exists_with(&prog.view(), loc, s, Window { first, last: s.len(), backward: false }, &opts, serial, st).unwrap();
                                            assert_eq!(got, want, "{:?} flags {} on {s:?} from {first}, notbol {not_bol} noteol {not_eol} nl {nl} store {}", std::string::String::from_utf8_lossy(pat), ext | extra, st.0.len());
                                            total += 1;
                                            decided += u64::from(get(&counters::FALLBACKS) == f0);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        // Most answers come from the DFA itself (only back-references, LEAVE bytes and the tiny
        // store's overflows fall back).
        assert!(decided * 4 > total * 3, "decided {decided} of {total}");
        assert!(get(&counters::FLUSHES) > 0, "the tiny store flushed");
    }

    /// A pattern needing more states than the store holds (`(a|b)*a(a|b){12}c`: 2^13) falls back after flushes, with at
    /// most one transition built per byte: the attempt is linear, and the answer exact.
    #[test]
    fn a_state_blowup_falls_back_linearly() {
        // Trace: D-14.13
        let prog = compile(b"(a|b)*a(a|b){12}c", regcomp_syntax(cflags::EXTENDED), &Locale::C).unwrap();
        let mut x = 0x2545_f491_u32;
        let s: Vec<u8> = (0..4000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 17;
                x ^= x << 5;
                if x & 1 == 0 { b'a' } else { b'b' }
            })
            .collect();
        let mut hit = s.clone();
        hit.extend_from_slice(b"abbbbbbbbbbbbc");
        for (subject, want) in [(&s[..], false), (&hit[..], true)] {
            let mut store = Store(std::vec![0; 8192]);
            let opts = Options { budget: 1 << 20, ..Options::default() };
            let (b0, f0) = (get(&counters::BUILT), get(&counters::FALLBACKS));
            let got = exists_with(&prog.view(), &Locale::C, subject, Window { first: 0, last: subject.len(), backward: false }, &opts, 7, &mut store).unwrap();
            assert_eq!(got, want);
            assert!(get(&counters::BUILT) - b0 <= subject.len() as u64 + 1, "at most one transition per byte");
            assert_eq!(get(&counters::FALLBACKS) - f0, 1, "the blow-up falls back");
            assert!(get(&counters::BUILT) - b0 < 2000, "it falls back early: {}", get(&counters::BUILT) - b0);
        }
    }

    /// The D-14.8 stacked repetitions: their nodes grow linearly in the stack (the shared
    /// empty-iteration level, FUZZ-REGEX-TIMEOUTS-3), so a 60-deep stack fits the node cap and
    /// the DFA answers as the matcher; a stack past the cap takes the matcher with zero DFA work.
    #[test]
    fn stacked_repetitions_take_the_matcher() {
        // Trace: D-14.13, D-14.8
        let stack = |depth: usize| {
            let mut pat = b"((|)a".to_vec();
            pat.extend(core::iter::repeat_n(b'*', depth));
            pat.extend_from_slice(b".****?*(b))+");
            compile(&pat, regcomp_syntax(cflags::EXTENDED), &Locale::C).unwrap()
        };
        let opts = Options { budget: 1 << 20, ..Options::default() };
        let fits = stack(60);
        assert!(fits.view().nodes as usize <= NODE_CAP, "{} nodes", fits.view().nodes);
        for s in [&b"aab"[..], b"b", b"a.b", b"xaaa.bab", b"aaaa"] {
            let mut store = Store(std::vec![0; 8192]);
            let w = Window { first: 0, last: s.len(), backward: false };
            let got = exists_with(&fits.view(), &Locale::C, s, w, &opts, 9, &mut store).unwrap();
            assert_eq!(got, nfa(&fits, &Locale::C, s, 0, &opts), "{s:?}");
        }
        let prog = stack(150);
        assert!(prog.view().nodes as usize > NODE_CAP, "{} nodes", prog.view().nodes);
        let mut store = Store(std::vec![0; 8192]);
        let (b0, f0) = (get(&counters::BUILT), get(&counters::FALLBACKS));
        let s = b"aab";
        let got = exists_with(&prog.view(), &Locale::C, s, Window { first: 0, last: 3, backward: false }, &opts, 9, &mut store).unwrap();
        assert_eq!(got, nfa(&prog, &Locale::C, s, 0, &opts));
        assert_eq!((get(&counters::BUILT) - b0, get(&counters::FALLBACKS) - f0), (0, 1));
    }

    /// Span patterns: the earliest start must not be the first one the reverse pass sees, empty
    /// iterations, empty matches, matches at the end.
    const SPAN_ERE: &[&[u8]] = &[b"ab|xabc", b"xabc", b"(a?)*", b"(a*)*b", b"(|a)+", b"x*", b"(a|b)*b$", b"a|.b", b"(ab|a)(bx|x)?", b"\\b(x|1)", b"[^\n]*$"];
    const SPAN_BRE: &[&[u8]] = &[b"\\(a*\\)\\{2,3\\}", b"\\(a\\|ab\\)\\(c\\|bcd\\)*", b"^\\(x*\\)"];

    type Got = Option<(usize, usize, Vec<(i64, i64)>)>;

    /// Span searches (D-14.16) over every pattern, flag set, subject and window: the
    /// mismatches with the matcher (described), the searches the DFA decided, and the total.
    fn span_check(only: Option<&[&[u8]]>) -> (Vec<std::string::String>, u64, u64) {
        let utf8 = Locale::c_utf8(b"C.UTF-8");
        // Every subject of up to two characters, every third of three, and the span cases.
        let mut subjects: Vec<Vec<u8>> = subjects().into_iter().enumerate().filter(|(i, s)| s.len() < 3 || i % 3 == 0 || s.len() > 3).map(|(_, s)| s).collect();
        for s in [&b"xabc"[..], b"zxabcz", b"aaab", b"c xabc ab", b"aabcd abcd", b"x1 1x"] {
            subjects.push(s.to_vec());
        }
        let tiny = SCRATCH + 2 * (E_STATES + 64);
        let mut stores = [Store(std::vec![0; 8192]), Store(std::vec![0; tiny])];
        let mut serial = 1000u64;
        let (mut bad, mut decided, mut total) = (Vec::new(), 0u64, 0u64);
        let sets: [(&[&[u8]], i32); 4] = [(ERE, cflags::EXTENDED), (SPAN_ERE, cflags::EXTENDED), (BRE, 0), (SPAN_BRE, 0)];
        for (pats, ext) in sets {
            for pat in pats {
                if only.is_some_and(|o| !o.contains(pat)) {
                    continue;
                }
                for loc in [&Locale::C, &utf8] {
                    for extra in [0, cflags::ICASE | cflags::NEWLINE] {
                        if extra != 0 && !core::ptr::eq(loc, &Locale::C) {
                            continue;
                        }
                        let Ok(prog) = compile(pat, regcomp_syntax(ext | extra), loc) else { continue };
                        serial += 1;
                        for s in &subjects {
                            for (first, last) in [(0, s.len()), (0, 0), (1, s.len()), (1, 3)] {
                                if first > s.len() {
                                    continue;
                                }
                                let last = last.min(s.len());
                                {
                                    for (not_bol, not_eol) in [(false, false), (true, false), (false, true)] {
                                        let window = Window { first, last, backward: false };
                                        let opts = Options { not_bol, not_eol, newline_anchor: extra & cflags::NEWLINE != 0, budget: 1 << 20, accel: Accel { prefilter: false, dfa: true, cap: 0 } };
                                        for caps in [false, true] {
                                            let off = Options { accel: Accel::OFF, ..opts };
                                            let want: Got = exec(&prog, loc, s, window, &off, caps).unwrap().map(|m| (m.start, m.end, m.groups.to_vec()));
                                            for st in stores.iter_mut().take(if caps { 1 } else { 2 }) {
                                                let f0 = get(&counters::FALLBACKS);
                                                let got: Got = search_with(&prog.view(), loc, s, window, &opts, caps, serial, st).unwrap().map(|m| (m.start, m.end, m.groups.to_vec()));
                                                total += 1;
                                                decided += u64::from(get(&counters::FALLBACKS) == f0);
                                                if got != want && bad.len() < 8 {
                                                    bad.push(std::format!("{:?} flags {} on {s:?} [{first},{last}] notbol {not_bol} noteol {not_eol} caps {caps} store {}: {got:?} != {want:?}", std::string::String::from_utf8_lossy(pat), ext | extra, st.0.len()));
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        (bad, decided, total)
    }

    /// Every span the DFA finds is the matcher's (start, end and subexpressions), over BRE and
    /// ERE, REG_ICASE with REG_NEWLINE, NOTBOL/NOTEOL, windows of one start (`re_match`), a
    /// range (`re_search`) and to the end, nmatch 1 and full, C and UTF-8, a full and a tiny
    /// store; and the reverse pass never accepts a start the forward pass cannot match from.
    #[test]
    fn the_spans_answer_as_the_matcher() {
        // Trace: D-14.16
        let d0 = get(&counters::DISAGREE);
        let (bad, decided, total) = span_check(None);
        assert!(bad.is_empty(), "{bad:#?}");
        assert_eq!(get(&counters::DISAGREE), d0, "the reverse pass accepted a start the forward pass refused");
        // Most come from the DFA (back-references, LEAVE bytes, tiny-store overflows fall back).
        assert!(decided * 3 > total * 2, "decided {decided} of {total}");
    }

    /// Planted bugs (D-14.16): the reverse pass ignoring NOTBOL, or stopping at its first
    /// accepting position (the rightmost start), is caught by the span check.
    #[test]
    fn planted_span_bugs_are_caught() {
        // Trace: D-14.16
        for (bug, pats) in [(&plant::IGNORE_NOTBOL, &[&b"^a"[..], b"(^|x)a"][..]), (&plant::FIRST_ACCEPT, &[&b"a"[..], b"ab|xabc"][..])] {
            bug.with(|c| c.set(true));
            let d0 = get(&counters::DISAGREE);
            let (bad, _, _) = span_check(Some(pats));
            let caught = !bad.is_empty() || get(&counters::DISAGREE) > d0;
            bug.with(|c| c.set(false));
            assert!(caught, "a planted bug went unnoticed");
        }
    }
    /// RE-SEARCH red-before-green: a one-pass program whose span the DFA found gets its
    /// subexpressions from the one-pass walk (the merge of RX-T3 and RX-ONEPASS ran the
    /// thread-set pass there), with the matcher's answer.
    #[test]
    fn a_dfa_span_takes_the_one_pass_walk() {
        let mut st = Store(std::vec![0; 8192]);
        for (k, (pat, s)) in [(&b"([a-z]+) ([0-9]+)"[..], &b"word 100037 /usr/lib/word/1.so"[..]), (b"(a|b)c", b"xxbc")].into_iter().enumerate() {
            let prog = compile(pat, regcomp_syntax(cflags::EXTENDED), &Locale::C).unwrap();
            assert!(prog.view().one_pass, "{k}: one-pass program expected");
            let window = Window { first: 0, last: s.len(), backward: false };
            let opts = Options { budget: 1 << 20, accel: Accel { prefilter: false, dfa: true, cap: 0 }, ..Options::default() };
            let off = Options { accel: Accel::OFF, ..opts };
            let want = exec(&prog, &Locale::C, s, window, &off, true).unwrap().map(|m| (m.start, m.end, m.groups.to_vec()));
            let n0 = get(&counters::SPAN_ONE_PASS);
            let got = search_with(&prog.view(), &Locale::C, s, window, &opts, true, 7 + k as u64, &mut st).unwrap().map(|m| (m.start, m.end, m.groups.to_vec()));
            assert_eq!(got, want, "{k}");
            assert_eq!(get(&counters::SPAN_ONE_PASS), n0 + 1, "{k}: the span's subexpressions did not come from the one-pass walk");
        }
    }
}
