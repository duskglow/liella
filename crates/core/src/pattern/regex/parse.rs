//! The pattern parser: basic, extended and GNU syntaxes, selected by the syntax bits
//! (`RE_*`, `<regex.h>`), into a tree. Errors are the `regcomp` codes.
//!
//! The tree is an arena ([`Parsed::nodes`]) in which every node comes after its children, and
//! no pass over it recurses (D-14 pending record REGCOMP-DEEP-NESTING-1): the parser keeps its
//! open groups on an explicit stack, the facts the compiler needs about a node (whether it can
//! match the empty string, the groups it holds) are computed as the node is added, and the
//! top-down passes walk with explicit stacks. How deep a pattern nests is bounded by memory
//! (`REG_ESPACE` when an allocation fails), never by the native stack.
#![allow(clippy::arithmetic_side_effects)] // pattern offsets are bounded by the pattern's length; node and stack counts by the arena's length

use crate::ctype::{self, WClass};
use crate::locale::collate::Collation;
use crate::locale::Locale;
use crate::mem::TryVec;

use super::text::{self, CharSet, Mode, Order, Unit};
use super::{syntax as s, ReError};

/// `RE_DUP_MAX`.
pub(crate) const DUP_MAX: u32 = 0x7fff;

/// A zero-width assertion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Assert {
    /// `^`.
    Bol,
    /// `$`.
    Eol,
    /// `` \` ``.
    BufStart,
    /// `\'`.
    BufEnd,
    /// `\b`.
    WordBoundary,
    /// `\B`.
    NotWordBoundary,
    /// `\<`.
    WordStart,
    /// `\>`.
    WordEnd,
}

/// A node of the tree. Children are indices into [`Parsed::nodes`], always smaller than the
/// node's own; a concatenation's or alternation's are `Parsed::kids[start..start + len]`.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Node {
    Empty,
    /// One literal byte.
    Byte(u8),
    /// One character of a set.
    Set(u32),
    /// `(start, len)` in [`Parsed::kids`].
    Cat(u32, u32),
    /// `(start, len)` in [`Parsed::kids`].
    Alt(u32, u32),
    /// A subexpression (numbered from 1) and its body.
    Group(u32, u32),
    /// A repetition of a node `{min,max}` (`max` `None`: unbounded).
    Rep(u32, u32, Option<u32>),
    Assert(Assert),
    /// A back-reference.
    Backref(u32),
}

/// The groups of a node that holds none.
pub(crate) const NO_GROUPS: (u32, u32) = (u32::MAX, 0);

/// A node with what the compiler needs to know about it.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Item {
    pub(crate) node: Node,
    /// The node can match the empty string.
    pub(crate) nullable: bool,
    /// The first and last group in the node, its hidden group included ([`NO_GROUPS`]: none).
    pub(crate) groups: (u32, u32),
    /// The hidden group around the node ([`hide_subpatterns`]; 0: none).
    pub(crate) wrap: u32,
}

const EMPTY: Item = Item { node: Node::Empty, nullable: true, groups: NO_GROUPS, wrap: 0 };

fn join(a: (u32, u32), b: (u32, u32)) -> (u32, u32) {
    (a.0.min(b.0), a.1.max(b.1))
}

fn at(nodes: &[Item], id: u32) -> Item {
    usize::try_from(id).ok().and_then(|i| nodes.get(i)).copied().unwrap_or(EMPTY)
}

fn kids_of(kids: &[u32], start: u32, len: u32) -> &[u32] {
    let s = usize::try_from(start).unwrap_or(usize::MAX);
    let e = s.saturating_add(usize::try_from(len).unwrap_or(0));
    kids.get(s..e).unwrap_or(&[])
}

/// `node` with its facts, from its children's (all in `nodes`).
fn facts(nodes: &[Item], kids: &[u32], node: Node, wrap: u32) -> Item {
    let (nullable, mut groups) = match node {
        Node::Byte(_) | Node::Set(_) => (false, NO_GROUPS),
        Node::Empty | Node::Assert(_) | Node::Backref(_) => (true, NO_GROUPS),
        Node::Cat(st, len) | Node::Alt(st, len) => {
            let all = matches!(node, Node::Cat(..));
            let mut null = all;
            let mut g = NO_GROUPS;
            for &k in kids_of(kids, st, len) {
                let it = at(nodes, k);
                null = if all { null && it.nullable } else { null || it.nullable };
                g = join(g, it.groups);
            }
            (null, g)
        }
        Node::Group(n, x) => {
            let it = at(nodes, x);
            (it.nullable, join((n, n), it.groups))
        }
        Node::Rep(x, m, _) => {
            let it = at(nodes, x);
            (m == 0 || it.nullable, it.groups)
        }
    };
    if wrap != 0 {
        groups = join(groups, (wrap, wrap));
    }
    Item { node, nullable, groups, wrap }
}

/// A stack whose first `N` entries live inline: the passes over the tree allocate nothing for
/// ordinary patterns, and a deep one grows on the heap (`REG_ESPACE` when that fails).
pub(crate) struct Stack<T: Copy, const N: usize> {
    inline: [T; N],
    len: usize,
    heap: TryVec<T>,
}

impl<T: Copy, const N: usize> Stack<T, N> {
    pub(crate) fn new(fill: T) -> Self {
        Stack { inline: [fill; N], len: 0, heap: TryVec::new() }
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn push(&mut self, v: T) -> Result<(), ReError> {
        if let Some(slot) = self.inline.get_mut(self.len) {
            *slot = v;
        } else {
            nomem(self.heap.push(v))?;
        }
        self.len += 1;
        Ok(())
    }

    pub(crate) fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        self.len -= 1;
        if self.len < N {
            self.inline.get(self.len).copied()
        } else {
            self.heap.pop()
        }
    }

    /// Append the entries from `from` up to `out`, in order.
    pub(crate) fn copy_from(&self, from: usize, out: &mut TryVec<T>) -> Result<(), ReError> {
        let inline = self.inline.get(from.min(N)..self.len.min(N)).unwrap_or(&[]);
        nomem(out.extend_from_slice(inline))?;
        if self.len > N {
            nomem(out.extend_from_slice(self.heap.as_slice().get(from.saturating_sub(N)..).unwrap_or(&[])))?;
        }
        Ok(())
    }

    pub(crate) fn truncate(&mut self, len: usize) {
        if len < self.len {
            self.heap.truncate(len.saturating_sub(N));
            self.len = len;
        }
    }
}

/// An open group while the parser reads its body: the group's number and the state of the
/// level around it.
#[derive(Clone, Copy)]
struct Level {
    n: u32,
    /// The visible groups when the enclosing alternation began.
    level_visible: u32,
    /// Where the enclosing alternation's branches and its current branch's items begin on
    /// [`Parser::work`].
    alts: usize,
    items: usize,
}

/// What the parser produces.
pub(crate) struct Parsed {
    /// The tree; every node after its children.
    pub(crate) nodes: TryVec<Item>,
    pub(crate) kids: TryVec<u32>,
    pub(crate) root: u32,
    pub(crate) sets: TryVec<CharSet>,
    /// The groups of the tree: the subexpressions, then (after [`hide_subpatterns`]) the hidden
    /// groups among them, all numbered in the order of their opening.
    pub(crate) ngroups: u32,
    /// Bit `n` set: group `n` is the target of a back-reference.
    pub(crate) brefs: u32,
    /// The pattern's subexpressions (`re_nsub`).
    pub(crate) nsub: u32,
    /// The group that reports subexpression `k + 1` (empty: subexpression `k + 1` is group
    /// `k + 1`).
    pub(crate) report: TryVec<u32>,
}

impl Parsed {
    /// Node `id` with its facts.
    pub(crate) fn item(&self, id: u32) -> Item {
        at(&self.nodes, id)
    }

    /// The children of a concatenation or an alternation.
    pub(crate) fn kids(&self, start: u32, len: u32) -> &[u32] {
        kids_of(&self.kids, start, len)
    }
}

fn nomem<T>(r: crate::error::Result<T>) -> Result<T, ReError> {
    r.map_err(|_| ReError::ESpace)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Tok {
    End,
    /// A literal character: its bytes in the pattern (`start..end`) and its value (`None` for a
    /// byte that is not a valid character).
    Lit(usize, usize, Option<u32>),
    Any,
    Bracket,
    Open,
    Close,
    Alt,
    Star,
    Plus,
    Question,
    /// `{` or `\{`.
    Interval,
    Caret,
    Dollar,
    Backref(u32),
    /// `\w`, `\W`, `\s`, `\S`.
    Class(u8),
    Assert(Assert),
}

struct Parser<'a> {
    p: &'a [u8],
    i: usize,
    syn: u64,
    mode: Mode,
    loc: &'a Locale,
    icase: bool,
    sets: TryVec<CharSet>,
    ngroups: u32,
    /// Groups closed so far and visible to a back-reference here.
    visible: u32,
    /// Per open nesting level: the visible groups hidden by an alternation at that level.
    hidden: TryVec<u32>,
    brefs: u32,
    open: usize,
    nodes: TryVec<Item>,
    kids: TryVec<u32>,
    /// The branches of the open alternations and the items of their current branches, the
    /// innermost on top.
    work: Stack<u32, 32>,
    /// An interval brace that `RE_INVALID_INTERVAL_ORD` made an ordinary character.
    lit_brace: Option<usize>,
}

impl<'a> Parser<'a> {
    fn has(&self, bit: u64) -> bool {
        self.syn & bit != 0
    }

    /// The character at `i` of the pattern: its end and value.
    fn char_at(&self, i: usize) -> (usize, Option<u32>) {
        match text::unit_at(self.mode, self.p, i) {
            Unit::Char(c, n) => (i + n, Some(c)),
            Unit::Invalid(_) => (i + 1, None),
            Unit::End => (i, None),
        }
    }

    /// The token at `i` and the offset after it.
    fn token_at(&self, i: usize) -> Result<(Tok, usize), ReError> {
        let Some(&b) = self.p.get(i) else { return Ok((Tok::End, i)) };
        if self.lit_brace == Some(i) {
            let k = if b == b'\\' { i + 1 } else { i };
            return Ok((Tok::Lit(k, k + 1, Some(u32::from(b'{'))), k + 1));
        }
        let ops = !self.has(s::LIMITED_OPS);
        if b == b'\\' {
            let Some(&c) = self.p.get(i + 1) else { return Err(ReError::EEscape) };
            let j = i + 2;
            let gnu = !self.has(s::NO_GNU_OPS);
            let t = match c {
                b'(' if !self.has(s::NO_BK_PARENS) => Tok::Open,
                b')' if !self.has(s::NO_BK_PARENS) => Tok::Close,
                b'|' if !self.has(s::NO_BK_VBAR) && ops => Tok::Alt,
                b'{' if self.has(s::INTERVALS) && !self.has(s::NO_BK_BRACES) => Tok::Interval,
                b'+' if self.has(s::BK_PLUS_QM) && ops => Tok::Plus,
                b'?' if self.has(s::BK_PLUS_QM) && ops => Tok::Question,
                b'1'..=b'9' if !self.has(s::NO_BK_REFS) => Tok::Backref(u32::from(c - b'0')),
                b'w' if gnu => Tok::Class(b'w'),
                b'W' if gnu => Tok::Class(b'W'),
                b's' if gnu => Tok::Class(b's'),
                b'S' if gnu => Tok::Class(b'S'),
                b'b' if gnu => Tok::Assert(Assert::WordBoundary),
                b'B' if gnu => Tok::Assert(Assert::NotWordBoundary),
                b'<' if gnu => Tok::Assert(Assert::WordStart),
                b'>' if gnu => Tok::Assert(Assert::WordEnd),
                b'`' if gnu => Tok::Assert(Assert::BufStart),
                b'\'' if gnu => Tok::Assert(Assert::BufEnd),
                _ => {
                    let (end, v) = self.char_at(i + 1);
                    return Ok((Tok::Lit(i + 1, end, v), end));
                }
            };
            return Ok((t, j));
        }
        let j = i + 1;
        let t = match b {
            b'(' if self.has(s::NO_BK_PARENS) => Tok::Open,
            b')' if self.has(s::NO_BK_PARENS) => Tok::Close,
            b'|' if self.has(s::NO_BK_VBAR) && ops => Tok::Alt,
            b'\n' if self.has(s::NEWLINE_ALT) => Tok::Alt,
            b'{' if self.has(s::INTERVALS) && self.has(s::NO_BK_BRACES) => Tok::Interval,
            b'*' => Tok::Star,
            b'+' if !self.has(s::BK_PLUS_QM) && ops => Tok::Plus,
            b'?' if !self.has(s::BK_PLUS_QM) && ops => Tok::Question,
            b'.' => Tok::Any,
            b'[' => Tok::Bracket,
            b'^' => Tok::Caret,
            b'$' => Tok::Dollar,
            _ => {
                let (end, v) = self.char_at(i);
                return Ok((Tok::Lit(i, end, v), end));
            }
        };
        Ok((t, j))
    }

    fn set(&mut self, cs: CharSet) -> Result<Node, ReError> {
        let mut cs = cs;
        cs.finish(self.mode, self.loc);
        let k = u32::try_from(self.sets.len()).map_err(|_| ReError::ESpace)?;
        nomem(self.sets.push(cs))?;
        Ok(Node::Set(k))
    }

    /// A literal character as a node: its bytes, or a case-folding set under `RE_ICASE`.
    fn literal(&mut self, start: usize, end: usize, v: Option<u32>) -> Result<Node, ReError> {
        let bytes = self.p.get(start..end).unwrap_or(&[]);
        if let (true, Some(c)) = (self.icase, v) {
            if text::lower(self.mode, self.loc, c) != c || text::upper(self.mode, self.loc, c) != c {
                let mut cs = CharSet::new(text::SET_ICASE);
                self.push_char(&mut cs, c)?;
                return self.set(cs);
            }
        }
        if let [b] = bytes {
            return Ok(Node::Byte(*b));
        }
        let start = u32::try_from(self.kids.len()).map_err(|_| ReError::ESpace)?;
        for &b in bytes {
            let id = self.add(Node::Byte(b))?;
            nomem(self.kids.push(id))?;
        }
        Ok(Node::Cat(start, u32::try_from(bytes.len()).map_err(|_| ReError::ESpace)?))
    }

    /// Add `node` (its children already added) to the tree.
    fn add(&mut self, node: Node) -> Result<u32, ReError> {
        let id = u32::try_from(self.nodes.len()).map_err(|_| ReError::ESpace)?;
        // A leaf (most nodes) has its facts without a look at children: [`facts`]'s first arms.
        let it = match node {
            Node::Byte(_) | Node::Set(_) => Item { node, nullable: false, groups: NO_GROUPS, wrap: 0 },
            Node::Empty | Node::Assert(_) | Node::Backref(_) => Item { node, nullable: true, groups: NO_GROUPS, wrap: 0 },
            Node::Cat(..) | Node::Alt(..) | Node::Group(..) | Node::Rep(..) => facts(&self.nodes, &self.kids, node, 0),
        };
        nomem(self.nodes.push(it))?;
        Ok(id)
    }

    /// The items on [`Self::work`] from `base` up as one node (a concatenation or an
    /// alternation of several, the item itself, or an empty node), popped.
    fn collect(&mut self, base: usize, alt: bool) -> Result<u32, ReError> {
        let len = self.work.len().saturating_sub(base);
        if len == 0 {
            return self.add(Node::Empty);
        }
        if len == 1 {
            return self.work.pop().ok_or(ReError::ESpace);
        }
        let start = u32::try_from(self.kids.len()).map_err(|_| ReError::ESpace)?;
        self.work.copy_from(base, &mut self.kids)?;
        self.work.truncate(base);
        let n = u32::try_from(len).map_err(|_| ReError::ESpace)?;
        self.add(if alt { Node::Alt(start, n) } else { Node::Cat(start, n) })
    }

    fn any(&mut self) -> Result<Node, ReError> {
        let mut f = text::SET_ANY;
        if !self.has(s::DOT_NEWLINE) {
            f |= text::SET_NOT_NEWLINE;
        }
        if self.has(s::DOT_NOT_NULL) {
            f |= text::SET_NOT_NUL;
        }
        self.set(CharSet::new(f))
    }

    fn gnu_class(&mut self, k: u8) -> Result<Node, ReError> {
        let neg = if k.is_ascii_uppercase() { text::SET_NEGATED } else { 0 };
        let mut cs = CharSet::new(neg);
        if k.eq_ignore_ascii_case(&b'w') {
            cs.flags |= text::SET_WORD;
        } else {
            nomem(cs.classes.push(text::class_code(WClass::Standard(ctype::Class::SPACE))))?;
        }
        self.set(cs)
    }

    // ---------------------------------------------------------------------------- brackets

    /// For a `[X...X]` item whose name starts at `from`: the name's end and the offset after the
    /// closing `X]`; `None` if it is not terminated. The name has at least one byte, so `[[.].]]`
    /// names `]`.
    fn bracket_item_end(&self, from: usize, delim: u8) -> Option<(usize, usize)> {
        let mut k = from.checked_add(1)?;
        while k + 1 < self.p.len() {
            if self.p.get(k) == Some(&delim) && self.p.get(k + 1) == Some(&b']') {
                return Some((k, k + 2));
            }
            k += 1;
        }
        None
    }

    /// A one-character collating element `[.x.]` or the representative of an equivalence class
    /// `[=x=]`: the character, or `REG_ECOLLATE`. Any single character of the locale is a
    /// collating element, multibyte ones included (XBD 9.3.5; D-14.4, three-way rule: glibc
    /// refuses `[[.é.]]` in UTF-8 locales).
    fn element_name(&self, start: usize, end: usize) -> Result<u32, ReError> {
        text::collating_element(self.mode, self.p.get(start..end).unwrap_or(&[])).ok_or(ReError::ECollate)
    }

    /// The position of a range end point in the range order.
    fn range_value(&self, c: u32) -> Result<u32, ReError> {
        let c = if self.icase { text::lower(self.mode, self.loc, c) } else { c };
        if let (Collation::CodePoint, true) = (self.loc.collation(), self.mode.multibyte()) {
            // Without collation rules a multibyte locale orders only the single-byte characters
            // (observed in C.UTF-8: a range with a non-ASCII end point is REG_ECOLLATE).
            if c >= 0x80 {
                return Err(ReError::ECollate);
            }
        }
        match text::order(self.mode, self.loc, c) {
            Order::At(v) => Ok(v),
            Order::None => Err(ReError::ECollate),
        }
    }

    fn bracket(&mut self) -> Result<Node, ReError> {
        // `self.i` is just after `[`.
        if self.i >= self.p.len() {
            return Err(ReError::BadPat);
        }
        let mut cs = CharSet::new(0);
        if self.p.get(self.i) == Some(&b'^') {
            cs.flags |= text::SET_NEGATED;
            if self.has(s::HAT_LISTS_NOT_NEWLINE) {
                cs.flags |= text::SET_NOT_NEWLINE;
            }
            self.i += 1;
        }
        if self.icase {
            cs.flags |= text::SET_ICASE;
        }
        let seq = matches!(self.loc.collation(), Collation::Rules(_));
        let mut first = true;
        loop {
            let Some(&b) = self.p.get(self.i) else { return Err(ReError::EBrack) };
            if b == b']' && !first {
                self.i += 1;
                break;
            }
            first = false;
            // A start element: a character, a collating symbol, or a class / equivalence class.
            let start: Option<u32>;
            let next = self.p.get(self.i + 1).copied();
            if b == b'[' && next == Some(b':') && self.has(s::CHAR_CLASSES) {
                let (name_end, after) = self.bracket_item_end(self.i + 2, b':').ok_or(ReError::EBrack)?;
                let name = self.p.get(self.i + 2..name_end).unwrap_or(&[]);
                let w = ctype::wctype(name, self.loc).ok_or(ReError::ECtype)?;
                nomem(cs.classes.push(text::class_code(w)))?;
                self.i = after;
                if self.p.get(self.i) == Some(&b'-') && self.p.get(self.i + 1).is_some_and(|c| *c != b']') {
                    return Err(ReError::ERange);
                }
                continue;
            } else if b == b'[' && next == Some(b'=') {
                let (name_end, after) = self.bracket_item_end(self.i + 2, b'=').ok_or(ReError::EBrack)?;
                let c = self.element_name(self.i + 2, name_end)?;
                nomem(cs.equiv.push(c))?;
                self.i = after;
                if self.p.get(self.i) == Some(&b'-') && self.p.get(self.i + 1).is_some_and(|c| *c != b']') {
                    return Err(ReError::ERange);
                }
                continue;
            } else if b == b'[' && next == Some(b'.') {
                let (name_end, after) = self.bracket_item_end(self.i + 2, b'.').ok_or(ReError::EBrack)?;
                start = Some(self.element_name(self.i + 2, name_end)?);
                self.i = after;
            } else if b == b'\\' && self.has(s::BACKSLASH_ESCAPE_IN_LISTS) {
                let (end, v) = self.char_at(self.i + 1);
                if end == self.i + 1 {
                    return Err(ReError::EBrack);
                }
                start = v;
                self.i = end;
            } else {
                let (end, v) = self.char_at(self.i);
                start = v;
                self.i = end;
            }
            // A range?
            if self.p.get(self.i) == Some(&b'-') && self.p.get(self.i + 1).is_some_and(|c| *c != b']') {
                self.i += 1;
                let Some(&e) = self.p.get(self.i) else { return Err(ReError::EBrack) };
                let enext = self.p.get(self.i + 1).copied();
                let end: Option<u32> = if e == b'[' && (enext == Some(b':') && self.has(s::CHAR_CLASSES) || enext == Some(b'=')) {
                    return Err(ReError::ERange);
                } else if e == b'[' && enext == Some(b'.') {
                    let (name_end, after) = self.bracket_item_end(self.i + 2, b'.').ok_or(ReError::EBrack)?;
                    let c = self.element_name(self.i + 2, name_end)?;
                    self.i = after;
                    Some(c)
                } else if e == b'\\' && self.has(s::BACKSLASH_ESCAPE_IN_LISTS) {
                    let (end, v) = self.char_at(self.i + 1);
                    if end == self.i + 1 {
                        return Err(ReError::EBrack);
                    }
                    self.i = end;
                    v
                } else {
                    let (end, v) = self.char_at(self.i);
                    self.i = end;
                    v
                };
                let (Some(lo), Some(hi)) = (start, end) else { return Err(ReError::ECollate) };
                let (vlo, vhi) = (self.range_value(lo)?, self.range_value(hi)?);
                if vlo > vhi {
                    if self.has(s::NO_EMPTY_RANGES) {
                        return Err(ReError::ERange);
                    }
                } else if seq {
                    nomem(cs.seq.push((vlo, vhi)))?;
                } else {
                    nomem(cs.ranges.push((vlo, vhi)))?;
                }
                // `[a-c-e]`: a range end point cannot start another range.
                if self.p.get(self.i) == Some(&b'-') && self.p.get(self.i + 1).is_some_and(|c| *c != b']') {
                    return Err(ReError::ERange);
                }
                continue;
            }
            // A single character (an invalid byte in a multibyte locale matches nothing).
            if let Some(c) = start {
                self.push_char(&mut cs, c)?;
            }
        }
        self.set(cs)
    }

    // ---------------------------------------------------------------------------- structure

    /// Parse an interval after `{` / `\{`: `Some((min, max))`, or `None` when the interval is
    /// not well formed and `RE_INVALID_INTERVAL_ORD` makes it literal (the parser is then
    /// positioned after the brace).
    fn interval(&mut self) -> Result<Option<(u32, Option<u32>)>, ReError> {
        let open = self.i;
        let close: &[u8] = if self.has(s::NO_BK_BRACES) { b"}" } else { b"\\}" };
        let digits = |p: &[u8], mut k: usize| -> (usize, Option<u64>) {
            let mut v: Option<u64> = None;
            while let Some(d) = p.get(k).filter(|d| d.is_ascii_digit()) {
                v = Some(v.unwrap_or(0).saturating_mul(10).saturating_add(u64::from(d - b'0')));
                k += 1;
            }
            (k, v)
        };
        let (k, lo) = digits(self.p, self.i);
        let (k, comma, hi) = if self.p.get(k) == Some(&b',') {
            let (k2, hi) = digits(self.p, k + 1);
            (k2, true, hi)
        } else {
            (k, false, None)
        };
        let well_formed = self.p.get(k..).is_some_and(|r| r.starts_with(close)) && (lo.is_some() || comma);
        // An empty interval `{}` is REG_BADBR under every syntax, RE_INVALID_INTERVAL_ORD included
        // (T28, probed: the other malformed intervals are literal under that bit).
        if lo.is_none() && !comma && k == open && !well_formed && self.p.get(k..).is_some_and(|r| r.starts_with(close)) {
            return Err(ReError::BadBr);
        }
        if !well_formed {
            if self.has(s::INVALID_INTERVAL_ORD) {
                self.i = open;
                return Ok(None);
            }
            // Unterminated when no closing brace follows anywhere; otherwise bad contents.
            let rest = self.p.get(open..).unwrap_or(&[]);
            let closed = rest.windows(close.len()).any(|w| w == close);
            return Err(if closed { ReError::BadBr } else { ReError::EBrace });
        }
        self.i = k + close.len();
        let lo = lo.unwrap_or(0);
        let hi = if comma { hi } else { Some(lo) };
        // A count above RE_DUP_MAX: "number too large" is REG_BADBR (POSIX regcomp; spec §14.9
        // lists no other code for it).
        if lo > u64::from(DUP_MAX) || hi.is_some_and(|h| h > u64::from(DUP_MAX)) {
            return Err(ReError::BadBr);
        }
        if hi.is_some_and(|h| h < lo) {
            return Err(ReError::BadBr);
        }
        let lo = u32::try_from(lo).map_err(|_| ReError::BadBr)?;
        let hi = hi.map(|h| u32::try_from(h).unwrap_or(DUP_MAX));
        Ok(Some((lo, hi)))
    }

    fn is_dup(t: Tok) -> bool {
        matches!(t, Tok::Star | Tok::Plus | Tok::Question | Tok::Interval)
    }

    /// Whether `$` followed by the token at `after` is an anchor.
    fn dollar_anchor(&self, after: usize) -> bool {
        if self.has(s::CONTEXT_INDEP_ANCHORS) {
            return true;
        }
        match self.token_at(after) {
            Ok((Tok::End | Tok::Alt, _)) => true,
            Ok((Tok::Close, _)) => self.open > 0,
            _ => false,
        }
    }

    /// The whole pattern: alternations of branches of items, groups nesting alternations. No
    /// recursion: an open group's level waits on `levels`, and the branches and items of every
    /// open level on [`Self::work`].
    fn tree(&mut self) -> Result<u32, ReError> {
        let mut levels: Stack<Level, 16> = Stack::new(Level { n: 0, level_visible: 0, alts: 0, items: 0 });
        // The innermost level: the visible groups when its alternation began, and where its
        // branches and its current branch's items begin on `work`.
        let mut level_visible = self.visible;
        let mut alts = self.work.len();
        let mut items = alts;
        // No atom yet in this branch (or only a leading `^`): the repetition operators are
        // context dependent here.
        let mut start = true;
        loop {
            let (t, j) = self.token_at(self.i)?;
            let ends = match t {
                Tok::End | Tok::Alt => true,
                Tok::Close if self.open > 0 => true,
                Tok::Close if !self.has(s::UNMATCHED_RIGHT_PAREN_ORD) => return Err(ReError::ERParen),
                _ => false,
            };
            if ends {
                // The branch ends: one node on `work` for it.
                let b = self.collect(items, false)?;
                self.work.push(b)?;
                let (t, j) = self.token_at(self.i)?;
                if t == Tok::Alt {
                    // Groups closed in this branch are not visible from the next one (GNU: a
                    // back-reference to a group of another alternative is REG_ESUBREG).
                    let hide = self.visible & !level_visible;
                    if let Some(h) = self.hidden.last_mut() {
                        *h |= hide;
                    }
                    self.visible = level_visible;
                    self.i = j;
                    items = self.work.len();
                    start = true;
                    continue;
                }
                // The alternation ends: its level is the whole pattern or a group's body.
                let body = self.collect(alts, true)?;
                let Some(lv) = levels.pop() else { return Ok(body) };
                if t != Tok::Close {
                    return Err(ReError::EParen);
                }
                self.i = j;
                self.open -= 1;
                let hidden = self.hidden.pop().unwrap_or(0);
                // Leaving the group: the groups its alternatives hid are visible again, and the
                // group itself becomes visible (back-references count groups up to 31; later ones
                // are never referenced, `\1`-`\9`).
                self.visible |= hidden;
                if lv.n < 32 {
                    self.visible |= 1 << lv.n;
                }
                let atom = self.add(Node::Group(lv.n, body))?;
                level_visible = lv.level_visible;
                alts = lv.alts;
                items = lv.items;
                start = false;
                let node = self.dups(atom)?;
                self.work.push(node)?;
                continue;
            }
            let atom = if Self::is_dup(t) {
                if start {
                    if self.has(s::CONTEXT_INVALID_OPS) || (t == Tok::Interval && self.has(s::CONTEXT_INVALID_DUP)) {
                        return Err(ReError::BadRpt);
                    }
                    if self.has(s::CONTEXT_INDEP_OPS) {
                        // A repetition of nothing; the operators follow.
                        self.add(Node::Empty)?
                    } else {
                        // The operator is an ordinary character.
                        let c = self.p.get(j.saturating_sub(1)).copied().unwrap_or(b'*');
                        self.i = j;
                        self.add(Node::Byte(c))?
                    }
                } else if self.work.len() > items {
                    // After a `^` anchor in the middle of the pattern: it is the operand.
                    self.work.pop().unwrap_or(u32::MAX)
                } else {
                    self.add(Node::Empty)?
                }
            } else {
                self.i = j;
                let node = match t {
                    Tok::Caret if self.has(s::CONTEXT_INDEP_ANCHORS) || start => {
                        let id = self.add(Node::Assert(Assert::Bol))?;
                        self.work.push(id)?;
                        // A repetition right after a leading `^` is still in a start context.
                        continue;
                    }
                    Tok::Open => {
                        // A group opens: its body is a new level.
                        self.ngroups = self.ngroups.checked_add(1).ok_or(ReError::ESpace)?;
                        self.open += 1;
                        nomem(self.hidden.push(0))?;
                        levels.push(Level { n: self.ngroups, level_visible, alts, items })?;
                        level_visible = self.visible;
                        alts = self.work.len();
                        items = alts;
                        start = true;
                        continue;
                    }
                    Tok::Caret => self.literal_char(b'^')?,
                    Tok::Dollar if self.dollar_anchor(j) => Node::Assert(Assert::Eol),
                    Tok::Dollar => self.literal_char(b'$')?,
                    Tok::Lit(a, b, v) => self.literal(a, b, v)?,
                    Tok::Any => self.any()?,
                    Tok::Bracket => self.bracket()?,
                    Tok::Close => self.literal_char(b')')?,
                    Tok::Backref(n) => {
                        if n > self.ngroups || self.visible & (1 << n) == 0 {
                            return Err(ReError::ESubreg);
                        }
                        self.brefs |= 1 << n;
                        Node::Backref(n)
                    }
                    Tok::Class(k) => self.gnu_class(k)?,
                    Tok::Assert(a) => Node::Assert(a),
                    _ => Node::Empty,
                };
                self.add(node)?
            };
            start = false;
            let node = self.dups(atom)?;
            self.work.push(node)?;
        }
    }

    /// An ASCII character that the syntax made ordinary.
    fn literal_char(&mut self, b: u8) -> Result<Node, ReError> {
        let c = u32::from(b);
        if self.icase && (text::lower(self.mode, self.loc, c) != c || text::upper(self.mode, self.loc, c) != c) {
            let mut cs = CharSet::new(text::SET_ICASE);
            nomem(cs.ranges.push((c, c)))?;
            return self.set(cs);
        }
        Ok(Node::Byte(b))
    }

    /// Apply the repetition operators that follow an operand.
    fn dups(&mut self, atom: u32) -> Result<u32, ReError> {
        let mut node = atom;
        let mut last_dup = false;
        loop {
            let pos = self.i;
            let (t, j) = self.token_at(pos)?;
            if !Self::is_dup(t) {
                return Ok(node);
            }
            // A repetition of an anchor (ERE `^*`, `x$+`, `(^){2}` written `^{2}`) is REG_BADRPT where the
            // syntax rejects repetition operators without an operand (#1783).
            if matches!(at(&self.nodes, node).node, Node::Assert(Assert::Bol | Assert::Eol)) && self.has(s::CONTEXT_INVALID_OPS) {
                return Err(ReError::BadRpt);
            }
            // RE_CONTEXT_INVALID_DUP: `*` or an interval cannot follow another repetition.
            if last_dup && self.has(s::CONTEXT_INVALID_DUP) && matches!(t, Tok::Star | Tok::Interval) {
                return Err(ReError::BadRpt);
            }
            self.i = j;
            node = match t {
                Tok::Star => self.rep(node, 0, None)?,
                Tok::Plus => self.rep(node, 1, None)?,
                Tok::Question => self.rep(node, 0, Some(1))?,
                _ => match self.interval()? {
                    Some((lo, hi)) => self.rep(node, lo, hi)?,
                    None => {
                        // RE_INVALID_INTERVAL_ORD: the brace is an ordinary character.
                        self.i = pos;
                        self.lit_brace = Some(pos);
                        return Ok(node);
                    }
                },
            };
            last_dup = true;
        }
    }

    fn rep(&mut self, atom: u32, lo: u32, hi: Option<u32>) -> Result<u32, ReError> {
        self.add(Node::Rep(atom, lo, hi))
    }

    /// List character `c` in `cs`; under `RE_ICASE` also its upper case when that does not fold
    /// back to `c` (T29, [`text::icase_extra`]).
    fn push_char(&self, cs: &mut CharSet, c: u32) -> Result<(), ReError> {
        nomem(cs.ranges.push((c, c)))?;
        if self.icase {
            if let Some(u) = text::icase_extra(self.mode, self.loc, c) {
                nomem(cs.ranges.push((u, u)))?;
            }
        }
        Ok(())
    }

}

/// Parse `pattern` under `syntax` in `loc`.
pub(crate) fn parse(pattern: &[u8], syntax: u64, loc: &Locale) -> Result<Parsed, ReError> {
    let mut p = Parser {
        p: pattern,
        i: 0,
        syn: syntax,
        mode: Mode::of(loc),
        loc,
        icase: syntax & s::ICASE != 0,
        sets: TryVec::new(),
        ngroups: 0,
        visible: 0,
        hidden: TryVec::new(),
        brefs: 0,
        open: 0,
        // A node per pattern byte at most, short of multibyte literals and empty operands.
        nodes: nomem(TryVec::with_capacity(pattern.len().saturating_add(1)))?,
        kids: TryVec::new(),
        work: Stack::new(0),
        lit_brace: None,
    };
    let root = p.tree()?;
    let (t, _) = p.token_at(p.i)?;
    if t != Tok::End {
        // Only an unmatched close parenthesis stops the top level early.
        return Err(ReError::ERParen);
    }
    let mut parsed =
        Parsed { nodes: p.nodes, kids: p.kids, root, sets: p.sets, ngroups: p.ngroups, brefs: p.brefs, nsub: p.ngroups, report: TryVec::new() };
    hide_subpatterns(&mut parsed)?;
    Ok(parsed)
}

// ------------------------------------------------------------------------------ subpatterns

/// XBD 9.1 (spec §14.9): consistent with the whole match being the longest of the leftmost
/// matches, *each subpattern*, from left to right, matches the longest possible string. The
/// submatch policy compares groups in the order of their opening, so a subpattern that is not a
/// subexpression must be a group too when it can vary in length and a subexpression follows it:
/// in `.*/\(.*\)` on `a/b/c` the first `.*` takes `a/b/` and the group `c` (with only the group
/// compared, the earlier start won: `b/c`); in `x*(a|b)*(b*)` on `xxabb` the repetition takes
/// `abb`, so its group reports the last iteration `b` and `(b*)` the empty string. Such a
/// subpattern (a repetition (one holding groups only in a pattern without back-references), or
/// an alternation or a back-reference that holds no group; not a
/// group's whole body, not a group-free repetition that is another repetition's whole
/// operand, not part of an alternative, and before the opening of some group) gets a
/// *hidden* group: every group is renumbered in opening order, and [`Parsed::report`] maps each
/// subexpression to its group. A fixed-length subpattern needs none (where it lies follows from
/// the subpatterns before it). Parts of an alternative are not compared on their own: the
/// alternatives are weighed by their subexpressions, as before.
///
/// Back-reference targets keep the 31-group limit of [`Parsed::brefs`]; a pattern whose
/// renumbered targets would exceed it keeps its groups as they are.
fn hide_subpatterns(p: &mut Parsed) -> Result<(), ReError> {
    if p.ngroups == 0 {
        return Ok(());
    }
    // The pattern has back-references: a repetition holding groups is not weighed on its own (a
    // back-reference sees the group as the subexpressions alone select it, the owner's ruling of
    // 2026-10-04 and the POSIX reading of `(a*)*\1`).
    let brefs = p.brefs != 0;
    let ix = |id: u32| usize::try_from(id).unwrap_or(usize::MAX);
    // Per node: the hidden group wrapping it (0: none). Per subexpression: its group.
    let mut wrap = TryVec::new();
    nomem(wrap.resize(p.nodes.len(), 0u32))?;
    let mut map = TryVec::new();
    nomem(map.resize(usize::try_from(p.ngroups).unwrap_or(0), 0u32))?;
    let mut next = 0u32;
    // The groups met so far in pre-order: a node comes before the opening of some group while
    // fewer than all of them are met (every group of the pattern is a node of the tree).
    let mut opened = 0u32;
    // Decide, in pre-order, which nodes get a hidden group and every group's number. `BODY`: the
    // node is a group's whole body; `ALT`: it lies in an alternative (below the nearest group);
    // `REP`: it is a repetition's whole operand.
    const BODY: u8 = 1;
    const ALT: u8 = 2;
    const REP: u8 = 4;
    let mut stack: Stack<(u32, u8), 32> = Stack::new((0, 0));
    stack.push((p.root, 0))?;
    while let Some((id, how)) = stack.pop() {
        let it = p.item(id);
        // Whether the node holds a group (nothing is wrapped yet).
        let has = it.groups != NO_GROUPS;
        let (body, alt, rep) = (how & BODY != 0, how & ALT != 0, how & REP != 0);
        let wrapped = match it.node {
            // A group-free repetition that is another repetition's whole operand (`a**`, `a*?*`)
            // is not weighed on its own (FUZZ-REGEX-TIMEOUTS-3, D-14.8): the outer repetition's
            // hidden group fixes the stack's span, and how a group-free stack splits that span
            // into iterations changes no later subexpression. Left unwrapped, the stack's scopes
            // are group-free and share one empty-iteration level (program.rs `chain_shared`).
            Node::Rep(..) => !(rep && !has) && (!has || !brefs),
            Node::Alt(..) | Node::Backref(_) => !has,
            _ => false,
        };
        if wrapped && !body && !alt && opened < p.ngroups {
            next = next.checked_add(1).ok_or(ReError::ESpace)?;
            if let Some(w) = wrap.get_mut(ix(id)) {
                *w = next;
            }
        }
        let inherit = if alt { ALT } else { 0 };
        // The children go on the stack last first, so that they are planned first to last.
        match it.node {
            Node::Group(g, x) => {
                opened += 1;
                next = next.checked_add(1).ok_or(ReError::ESpace)?;
                if let Some(m) = map.get_mut(ix(g).saturating_sub(1)) {
                    *m = next;
                }
                stack.push((x, BODY))?;
            }
            Node::Cat(s, l) => {
                for &k in p.kids(s, l).iter().rev() {
                    stack.push((k, inherit))?;
                }
            }
            Node::Alt(s, l) => {
                for &k in p.kids(s, l).iter().rev() {
                    stack.push((k, ALT))?;
                }
            }
            Node::Rep(x, _, _) => stack.push((x, inherit | REP))?,
            _ => {}
        }
    }
    if next == p.ngroups {
        return Ok(());
    }
    let mut new_brefs = 0u32;
    for g in 1..=p.ngroups.min(31) {
        if p.brefs & (1 << g) != 0 {
            let to = map.get(usize::try_from(g - 1).unwrap_or(usize::MAX)).copied().unwrap_or(u32::MAX);
            if to > 31 {
                return Ok(());
            }
            new_brefs |= 1 << to;
        }
    }
    // Renumber the groups and back-references, mark the planned nodes and recompute every
    // node's facts, children first.
    let of = |g: u32| map.get(ix(g).saturating_sub(1)).copied().ok_or(ReError::ESubreg);
    let nodes = p.nodes.as_mut_slice();
    for i in 0..nodes.len() {
        let (done, rest) = nodes.split_at_mut(i);
        let Some(cur) = rest.first_mut() else { break };
        let node = match cur.node {
            Node::Group(g, x) => Node::Group(of(g)?, x),
            Node::Backref(g) => Node::Backref(of(g)?),
            other => other,
        };
        *cur = facts(done, &p.kids, node, wrap.get(i).copied().unwrap_or(0));
    }
    p.brefs = new_brefs;
    p.ngroups = next;
    p.report = map;
    Ok(())
}
