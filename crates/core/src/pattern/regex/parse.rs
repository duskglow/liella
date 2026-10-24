//! The pattern parser: basic, extended and GNU syntaxes, selected by the syntax bits
//! (`RE_*`, `<regex.h>`), into a tree. Errors are the `regcomp` codes.
#![allow(clippy::arithmetic_side_effects)] // pattern offsets are bounded by the pattern's length; counts are bounded by RE_DUP_MAX and MAX_DEPTH

use crate::ctype::{self, WClass};
use crate::locale::collate::Collation;
use crate::locale::Locale;
use crate::mem::{TryBox, TryVec};

use super::text::{self, CharSet, Mode, Order, Unit};
use super::{syntax as s, ReError};

/// A tree deeper than this (nested groups, alternatives and repetitions) is refused with
/// `REG_ESPACE`, so that the recursive passes over it stay within a small stack.
const MAX_DEPTH: usize = 256;
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

/// The tree.
#[derive(Debug)]
pub(crate) enum Node {
    Empty,
    /// One literal byte.
    Byte(u8),
    /// One character of a set.
    Set(u32),
    Cat(TryVec<Node>),
    Alt(TryVec<Node>),
    /// A subexpression (numbered from 1).
    Group(u32, TryBox<Node>),
    /// A repetition `{min,max}` (`max` `None`: unbounded).
    Rep(TryBox<Node>, u32, Option<u32>),
    Assert(Assert),
    /// A back-reference.
    Backref(u32),
}

/// What the parser produces.
pub(crate) struct Parsed {
    pub(crate) root: Node,
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

fn nomem<T>(r: crate::error::Result<T>) -> Result<T, ReError> {
    r.map_err(|_| ReError::ESpace)
}

fn boxed(n: Node) -> Result<TryBox<Node>, ReError> {
    nomem(TryBox::new(n))
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
    /// The depth of the subtree the last parsing step produced.
    last_depth: usize,
    open: usize,
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
        let mut items = TryVec::new();
        for &b in bytes {
            nomem(items.push(Node::Byte(b)))?;
        }
        Ok(Node::Cat(items))
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

    fn alternation(&mut self) -> Result<Node, ReError> {
        let mut branches = TryVec::new();
        let level_visible = self.visible;
        let mut deepest = 0;
        loop {
            let b = self.branch()?;
            deepest = deepest.max(self.last_depth);
            nomem(branches.push(b))?;
            let (t, j) = self.token_at(self.i)?;
            if t != Tok::Alt {
                break;
            }
            // Groups closed in this branch are not visible from the next one (GNU: a
            // back-reference to a group of another alternative is REG_ESUBREG).
            let hide = self.visible & !level_visible;
            if let Some(h) = self.hidden.last_mut() {
                *h |= hide;
            }
            self.visible = level_visible;
            self.i = j;
        }
        if branches.len() == 1 {
            self.last_depth = deepest;
            return Ok(branches.pop().unwrap_or(Node::Empty));
        }
        self.last_depth = deepest + 1;
        Ok(Node::Alt(branches))
    }

    fn branch(&mut self) -> Result<Node, ReError> {
        let mut items: TryVec<Node> = TryVec::new();
        let mut deepest = 1;
        // No atom yet in this branch (or only a leading `^`): the repetition operators are
        // context dependent here.
        let mut start = true;
        loop {
            let (t, j) = self.token_at(self.i)?;
            match t {
                Tok::End | Tok::Alt => break,
                Tok::Close if self.open > 0 => break,
                Tok::Close if !self.has(s::UNMATCHED_RIGHT_PAREN_ORD) => return Err(ReError::ERParen),
                _ => {}
            }
            let atom = if Self::is_dup(t) {
                if start {
                    if self.has(s::CONTEXT_INVALID_OPS) || (t == Tok::Interval && self.has(s::CONTEXT_INVALID_DUP)) {
                        return Err(ReError::BadRpt);
                    }
                    if self.has(s::CONTEXT_INDEP_OPS) {
                        // A repetition of nothing; the operators follow.
                        Node::Empty
                    } else {
                        // The operator is an ordinary character.
                        let c = self.p.get(j.saturating_sub(1)).copied().unwrap_or(b'*');
                        self.i = j;
                        Node::Byte(c)
                    }
                } else {
                    // After a `^` anchor in the middle of the pattern: it is the operand.
                    items.pop().unwrap_or(Node::Empty)
                }
            } else {
                self.i = j;
                match t {
                    Tok::Caret if self.has(s::CONTEXT_INDEP_ANCHORS) || start => {
                        nomem(items.push(Node::Assert(Assert::Bol)))?;
                        // A repetition right after a leading `^` is still in a start context.
                        continue;
                    }
                    Tok::Caret => self.literal_char(b'^')?,
                    Tok::Dollar if self.dollar_anchor(j) => Node::Assert(Assert::Eol),
                    Tok::Dollar => self.literal_char(b'$')?,
                    Tok::Lit(a, b, v) => self.literal(a, b, v)?,
                    Tok::Any => self.any()?,
                    Tok::Bracket => self.bracket()?,
                    Tok::Open => self.group()?,
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
                }
            };
            start = false;
            // The operand's depth: a group's was measured; a literal of several bytes is two levels.
            let depth = if t == Tok::Open { self.last_depth } else if matches!(atom, Node::Cat(_)) { 2 } else { 1 };
            let node = self.dups(atom, depth)?;
            deepest = deepest.max(self.last_depth);
            nomem(items.push(node))?;
        }
        if items.len() <= 1 {
            self.last_depth = deepest;
            return Ok(items.pop().unwrap_or(Node::Empty));
        }
        self.last_depth = deepest + 1;
        Ok(Node::Cat(items))
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
    fn dups(&mut self, atom: Node, depth: usize) -> Result<Node, ReError> {
        let mut node = atom;
        let mut last_dup = false;
        self.last_depth = depth;
        loop {
            let at = self.i;
            let (t, j) = self.token_at(at)?;
            if !Self::is_dup(t) {
                return Ok(node);
            }
            // A repetition of an anchor (ERE `^*`, `x$+`, `(^){2}` written `^{2}`) is REG_BADRPT where the
            // syntax rejects repetition operators without an operand (#1783).
            if matches!(node, Node::Assert(Assert::Bol | Assert::Eol)) && self.has(s::CONTEXT_INVALID_OPS) {
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
                        self.i = at;
                        self.lit_brace = Some(at);
                        return Ok(node);
                    }
                },
            };
            last_dup = true;
        }
    }

    fn rep(&mut self, atom: Node, lo: u32, hi: Option<u32>) -> Result<Node, ReError> {
        self.last_depth += 1;
        if self.last_depth > MAX_DEPTH {
            return Err(ReError::ESpace);
        }
        Ok(Node::Rep(boxed(atom)?, lo, hi))
    }

    /// The error for a group nested deeper than `MAX_DEPTH` (T30): the depth stays bounded (the
    /// parser never recurses past it), but a pattern whose groups do not close is `REG_EPAREN`
    /// and one with an unterminated bracket is `REG_EBRACK`, as glibc reports them (probed at
    /// 257 to 10,000 levels); a balanced pattern that deep is `REG_ESPACE`. The rest of the
    /// pattern is only scanned by token, on this error path.
    fn too_deep(&self) -> ReError {
        let mut need = self.open + 1;
        let mut i = self.i;
        loop {
            let Ok((t, j)) = self.token_at(i) else { return ReError::ESpace };
            match t {
                Tok::End => return if need > 0 { ReError::EParen } else { ReError::ESpace },
                Tok::Open => need += 1,
                Tok::Close => need = need.saturating_sub(1),
                Tok::Bracket => match self.bracket_end(j) {
                    Some(k) => {
                        i = k;
                        continue;
                    }
                    None => return ReError::EBrack,
                },
                _ => {}
            }
            i = j;
        }
    }

    /// The position after the `]` closing a bracket expression whose body starts at `i`, or
    /// `None` when it is unterminated (for [`Self::too_deep`]).
    fn bracket_end(&self, mut i: usize) -> Option<usize> {
        if self.p.get(i) == Some(&b'^') {
            i += 1;
        }
        if self.p.get(i) == Some(&b']') {
            i += 1;
        }
        loop {
            match (self.p.get(i)?, self.p.get(i + 1)) {
                (b']', _) => return Some(i + 1),
                (b'[', Some(&d @ (b':' | b'.' | b'='))) => {
                    let rest = self.p.get(i + 2..)?;
                    let k = rest.windows(2).position(|w| w == [d, b']'])?;
                    i += 2 + k + 2;
                }
                _ => i += 1,
            }
        }
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

    fn group(&mut self) -> Result<Node, ReError> {
        // The parser recurses once per open group: bound it before descending.
        if self.open >= MAX_DEPTH {
            return Err(self.too_deep());
        }
        self.ngroups = self.ngroups.checked_add(1).ok_or(ReError::ESpace)?;
        let n = self.ngroups;
        self.open += 1;
        nomem(self.hidden.push(0))?;
        let inner = self.alternation()?;
        let (t, j) = self.token_at(self.i)?;
        if t != Tok::Close {
            return Err(ReError::EParen);
        }
        self.i = j;
        self.open -= 1;
        let hidden = self.hidden.pop().unwrap_or(0);
        // Leaving the group: the groups its alternatives hid are visible again, and the group
        // itself becomes visible (back-references count groups up to 31; later ones are never
        // referenced, `\1`-`\9`).
        self.visible |= hidden;
        if n < 32 {
            self.visible |= 1 << n;
        }
        self.last_depth += 1;
        if self.last_depth > MAX_DEPTH {
            return Err(ReError::ESpace);
        }
        Ok(Node::Group(n, boxed(inner)?))
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
        last_depth: 0,
        open: 0,
        lit_brace: None,
    };
    let root = p.alternation()?;
    let (t, _) = p.token_at(p.i)?;
    if t != Tok::End {
        // Only an unmatched close parenthesis stops the top level early.
        return Err(ReError::ERParen);
    }
    let mut parsed = Parsed { root, sets: p.sets, ngroups: p.ngroups, brefs: p.brefs, nsub: p.ngroups, report: TryVec::new() };
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
/// group's whole body, not part of an alternative, and before the opening of some group) gets a
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
    /// Per pre-order position: whether the node holds a group; and the last group's position.
    fn survey(n: &Node, holds: &mut TryVec<bool>, last: &mut usize) -> Result<bool, ReError> {
        let me = holds.len();
        nomem(holds.push(false))?;
        let has = match n {
            Node::Group(_, x) => {
                *last = me;
                survey(x, holds, last)?;
                true
            }
            Node::Cat(v) | Node::Alt(v) => {
                let mut any = false;
                for c in v.iter() {
                    any |= survey(c, holds, last)?;
                }
                any
            }
            Node::Rep(x, _, _) => survey(x, holds, last)?,
            _ => false,
        };
        if let Some(h) = holds.get_mut(me) {
            *h = has;
        }
        Ok(has)
    }
    struct Plan {
        /// The pattern has back-references: a repetition holding groups is not weighed on its
        /// own (a back-reference sees the group as the subexpressions alone select it, the
        /// owner's ruling of 2026-10-04 and the POSIX reading of `(a*)*\1`).
        brefs: bool,
        holds: TryVec<bool>,
        last: usize,
        pre: usize,
        next: u32,
        /// Per pre-order position: the hidden group wrapping it (0: none).
        wrap: TryVec<u32>,
        /// Per subexpression: its group.
        map: TryVec<u32>,
    }
    // Decide, in pre-order, which nodes get a hidden group and every group's number. `body`: `n`
    // is a group's whole body; `alt`: `n` lies in an alternative (below the nearest group).
    fn plan(n: &Node, body: bool, alt: bool, st: &mut Plan) -> Result<(), ReError> {
        let me = st.pre;
        st.pre += 1;
        let has = st.holds.get(me).copied().unwrap_or(false);
        let wrap = match n {
            Node::Rep(..) => !has || !st.brefs,
            Node::Alt(_) | Node::Backref(_) => !has,
            _ => false,
        };
        let mut h = 0;
        if wrap && !body && !alt && me < st.last {
            st.next = st.next.checked_add(1).ok_or(ReError::ESpace)?;
            h = st.next;
        }
        nomem(st.wrap.push(h))?;
        match n {
            Node::Group(g, x) => {
                st.next = st.next.checked_add(1).ok_or(ReError::ESpace)?;
                if let Some(m) = st.map.get_mut(usize::try_from(*g).unwrap_or(0).saturating_sub(1)) {
                    *m = st.next;
                }
                plan(x, true, false, st)?;
            }
            Node::Cat(v) => {
                for c in v.iter() {
                    plan(c, false, alt, st)?;
                }
            }
            Node::Alt(v) => {
                for c in v.iter() {
                    plan(c, false, true, st)?;
                }
            }
            Node::Rep(x, _, _) => plan(x, false, alt, st)?,
            _ => {}
        }
        Ok(())
    }
    // Renumber the groups and back-references and wrap the planned nodes.
    fn apply(n: &mut Node, pre: &mut usize, st: &Plan) -> Result<(), ReError> {
        let me = *pre;
        *pre += 1;
        let of = |g: u32| st.map.get(usize::try_from(g).unwrap_or(0).saturating_sub(1)).copied().ok_or(ReError::ESubreg);
        match n {
            Node::Group(g, x) => {
                *g = of(*g)?;
                apply(x, pre, st)?;
            }
            Node::Cat(v) | Node::Alt(v) => {
                for c in v.as_mut_slice() {
                    apply(c, pre, st)?;
                }
            }
            Node::Rep(x, _, _) => apply(x, pre, st)?,
            Node::Backref(g) => *g = of(*g)?,
            _ => {}
        }
        let h = st.wrap.get(me).copied().unwrap_or(0);
        if h != 0 {
            let inner = core::mem::replace(n, Node::Empty);
            *n = Node::Group(h, boxed(inner)?);
        }
        Ok(())
    }
    let mut holds = TryVec::new();
    let mut last = 0usize;
    survey(&p.root, &mut holds, &mut last)?;
    let mut map = TryVec::new();
    nomem(map.resize(usize::try_from(p.ngroups).unwrap_or(0), 0u32))?;
    let mut st = Plan { brefs: p.brefs != 0, holds, last, pre: 0, next: 0, wrap: TryVec::new(), map };
    plan(&p.root, false, false, &mut st)?;
    if st.next == p.ngroups {
        return Ok(());
    }
    let mut brefs = 0u32;
    for g in 1..=p.ngroups.min(31) {
        if p.brefs & (1 << g) != 0 {
            let to = st.map.get(usize::try_from(g - 1).unwrap_or(usize::MAX)).copied().unwrap_or(u32::MAX);
            if to > 31 {
                return Ok(());
            }
            brefs |= 1 << to;
        }
    }
    apply(&mut p.root, &mut 0, &st)?;
    p.brefs = brefs;
    p.ngroups = st.next;
    p.report = st.map;
    Ok(())
}
