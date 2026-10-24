//! The bytes every match of a program contains (`regexec`'s prefilter, D-14.10): the longest
//! chain of `Byte` instructions that begins at a dominator of `Match` in the instruction graph.
//!
//! The graph's edges are every successor an instruction can take at any scope depth (`Exit`
//! has both of its successors), a superset of the moves the matcher makes, so an instruction
//! that every path from the start to a `Match` visits in it (a dominator; Cooper, Harvey and
//! Kennedy's iterative algorithm) is visited by every match. From a dominating `Byte`, the
//! matcher can only go on through zero-width instructions with a single successor (`Open`,
//! `Close`, `Assert`, `Enter`, `Jmp`) until the next consuming one; when that is a `Byte`, the
//! two bytes are adjacent in every match, and so on. `Byte` compares the subject's byte as it
//! is in every mode, so the string is exact in every locale and with `REG_ICASE` (which compiles
//! the letters to sets, so they yield nothing). A match lies in the searched window, so a
//! window without the string has no match.
#![allow(clippy::arithmetic_side_effects)] // indices and counts are bounded by the instruction count (MAX_INSTS)

use crate::mem::TryVec;

use super::program::{Inst, ProgRef, NONE, OP_ASSERT, OP_BACKREF, OP_BYTE, OP_CLOSE, OP_ENTER, OP_EXIT, OP_JMP, OP_MATCH, OP_OPEN, OP_SET, OP_SPLIT};

/// The longest string kept (bytes): a longer one's first bytes are as certain, and the search
/// costs the same.
pub(crate) const MAX_LEN: usize = 32;
/// Larger programs are not analysed (about 24 bytes of regcomp memory per instruction).
const MAX_INSTS: usize = 1 << 18;
/// The iteration converges in a few passes on the graphs regcomp builds; past this many the
/// analysis gives up (no string), which bounds regcomp time.
const MAX_PASSES: usize = 64;

fn get(p: &ProgRef<'_>, pc: u32) -> Option<Inst> {
    usize::try_from(pc).ok().filter(|&k| k < p.ninsts()).map(|_| p.inst(pc))
}

fn ix(pc: u32) -> usize {
    usize::try_from(pc).unwrap_or(usize::MAX)
}

/// Every successor of `pc` (`NONE` for none).
fn succ(p: &ProgRef<'_>, pc: u32) -> [u32; 2] {
    let Some(i) = get(p, pc) else { return [NONE, NONE] };
    let s = match i.op {
        OP_BYTE | OP_SET | OP_OPEN | OP_CLOSE | OP_ASSERT | OP_BACKREF | OP_ENTER => [i.b, NONE],
        OP_SPLIT => [i.a, i.b],
        OP_JMP => [i.a, NONE],
        OP_EXIT => [i.b, i.c],
        _ => [NONE, NONE],
    };
    s.map(|t| if get(p, t).is_some() { t } else { NONE })
}

/// The dominator tree of the instructions the start reaches.
struct Doms {
    /// Immediate dominator (the start's is itself; `NONE`: unreachable).
    idom: TryVec<u32>,
    /// Postorder number (`NONE`: unreachable).
    po: TryVec<u32>,
}

impl Doms {
    fn at(v: &TryVec<u32>, k: u32) -> u32 {
        v.get(ix(k)).copied().unwrap_or(NONE)
    }

    /// The nearest common dominator of `a` and `b` (both reachable).
    fn meet(&self, mut a: u32, mut b: u32) -> u32 {
        while a != b {
            while Self::at(&self.po, a) < Self::at(&self.po, b) {
                a = Self::at(&self.idom, a);
            }
            while Self::at(&self.po, b) < Self::at(&self.po, a) {
                b = Self::at(&self.idom, b);
            }
        }
        a
    }

    fn build(p: &ProgRef<'_>) -> Option<Self> {
        let n = p.ninsts();
        if n == 0 || n > MAX_INSTS || get(p, p.start).is_none() {
            return None;
        }
        // Postorder by an explicit depth-first walk.
        let mut po = TryVec::new();
        po.resize(n, NONE).ok()?;
        let mut seen = TryVec::new();
        seen.resize(n, false).ok()?;
        let mut post: TryVec<u32> = TryVec::new();
        post.reserve(n).ok()?;
        let mut stack: TryVec<(u32, u8)> = TryVec::new();
        stack.push((p.start, 0)).ok()?;
        *seen.get_mut(ix(p.start))? = true;
        while let Some(top) = stack.last_mut() {
            let (pc, k) = *top;
            match succ(p, pc).get(usize::from(k)).copied() {
                Some(t) => {
                    top.1 += 1;
                    if t != NONE && !seen.get(ix(t)).copied().unwrap_or(true) {
                        *seen.get_mut(ix(t))? = true;
                        stack.push((t, 0)).ok()?;
                    }
                }
                None => {
                    *po.get_mut(ix(pc))? = u32::try_from(post.len()).ok()?;
                    post.push(pc).ok()?;
                    stack.pop();
                }
            }
        }
        // Predecessors (of reachable instructions), compressed.
        let mut off = TryVec::new();
        off.resize(n + 1, 0u32).ok()?;
        for &pc in post.iter() {
            for t in succ(p, pc) {
                if t != NONE {
                    *off.get_mut(ix(t) + 1)? += 1;
                }
            }
        }
        for k in 1..=n {
            let prev = *off.get(k - 1)?;
            *off.get_mut(k)? += prev;
        }
        let mut fill = off.try_clone().ok()?;
        let mut preds = TryVec::new();
        preds.resize(ix(*off.get(n)?), NONE).ok()?;
        for &pc in post.iter() {
            for t in succ(p, pc) {
                if t != NONE {
                    let slot = fill.get_mut(ix(t))?;
                    *preds.get_mut(ix(*slot))? = pc;
                    *slot += 1;
                }
            }
        }
        let mut idom = TryVec::new();
        idom.resize(n, NONE).ok()?;
        *idom.get_mut(ix(p.start))? = p.start;
        let mut d = Self { idom, po };
        for pass in 0.. {
            if pass == MAX_PASSES {
                return None;
            }
            let mut changed = false;
            for &pc in post.iter().rev() {
                if pc == p.start {
                    continue;
                }
                let (lo, hi) = (ix(*off.get(ix(pc))?), ix(*off.get(ix(pc) + 1)?));
                let mut new = NONE;
                for &q in preds.get(lo..hi)? {
                    if Self::at(&d.idom, q) != NONE {
                        new = if new == NONE { q } else { d.meet(q, new) };
                    }
                }
                if new != Self::at(&d.idom, pc) {
                    *d.idom.get_mut(ix(pc))? = new;
                    changed = true;
                }
            }
            if !changed {
                break;
            }
        }
        Some(d)
    }
}

/// The bytes of the chain that begins at the `Byte` instruction `pc`.
fn chain(p: &ProgRef<'_>, mut pc: u32) -> Option<TryVec<u8>> {
    let mut bytes = TryVec::new();
    for _ in 0..=p.ninsts() {
        let Some(i) = get(p, pc) else { break };
        match i.op {
            OP_BYTE => {
                let Ok(b) = u8::try_from(i.a) else { break };
                bytes.push(b).ok()?;
                if bytes.len() == MAX_LEN {
                    break;
                }
                pc = i.b;
            }
            OP_OPEN | OP_CLOSE | OP_ASSERT | OP_ENTER => pc = i.b,
            OP_JMP => pc = i.a,
            _ => break,
        }
    }
    Some(bytes)
}

/// How common a byte is in text (higher: more common): the tie-break between strings of the
/// same length prefers the rarer first byte, which the search finds fewer false starts of.
fn commonness(b: u8) -> u8 {
    match b {
        b' ' | b'e' | b't' | b'a' | b'o' | b'i' | b'n' | b's' | b'r' => 6,
        b'a'..=b'z' => 5,
        b'0'..=b'9' => 4,
        b'A'..=b'Z' | b'\n' | b'\t' | b'_' | b'.' | b',' | b'-' | b'/' => 3,
        0x21..=0x7e => 2,
        _ => 1,
    }
}

/// The longest string of bytes every match of `p` holds contiguously (at most [`MAX_LEN`]);
/// `None` when there is none, or the program is too large to analyse.
pub(crate) fn required(p: &ProgRef<'_>) -> Option<TryVec<u8>> {
    let d = Doms::build(p)?;
    // The common dominator of every reachable `Match`.
    let mut m = NONE;
    for k in 0..p.ninsts() {
        let pc = u32::try_from(k).ok()?;
        if p.inst(pc).op == OP_MATCH && Doms::at(&d.po, pc) != NONE {
            m = if m == NONE { pc } else { d.meet(pc, m) };
        }
    }
    let mut best: Option<TryVec<u8>> = None;
    let mut x = m;
    for _ in 0..=p.ninsts() {
        if x == NONE {
            break;
        }
        if get(p, x).is_some_and(|i| i.op == OP_BYTE) {
            let s = chain(p, x)?;
            let better = match &best {
                None => !s.is_empty(),
                Some(b) => s.len() > b.len() || (s.len() == b.len() && commonness(s.first().copied().unwrap_or(0)) < commonness(b.first().copied().unwrap_or(0))),
            };
            if better {
                best = Some(s);
            }
        }
        if x == p.start {
            break;
        }
        x = Doms::at(&d.idom, x);
    }
    best
}
