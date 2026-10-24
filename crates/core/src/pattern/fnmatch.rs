//! Shell-style wildcard matching (spec §14.7; XCU 2.13): `*`, `?`, bracket expressions with
//! ranges, classes, equivalence classes and collating symbols, `\` escapes, the flags
//! `FNM_NOESCAPE`, `FNM_PATHNAME`, `FNM_PERIOD`, `FNM_LEADING_DIR`, `FNM_CASEFOLD` and the ksh
//! extended patterns of `FNM_EXTMATCH`. `glob` (§14.8) and `wordexp` (§14.10) match with it.
//!
//! # Text
//!
//! In a locale whose characters take one byte, every byte is a character. In a multibyte locale
//! the pattern and the string are decoded into characters; if either is not valid text in the
//! locale's encoding, both are matched byte by byte instead (each byte one character, a byte that
//! is not a character of the locale in no class), so invalid text never makes the call fail
//! (observed behaviour; pending an owner ruling, see the phase report).
//!
//! # Brackets
//!
//! A `[` begins a bracket expression only when a valid one follows (XCU 2.13.1; XBD 9.3.5): a
//! closing `]` (a `]` right after `[` or `[!` is a member), every `[:name:]` naming a class of
//! the locale, every `[=x=]` and `[.x.]` naming one character (the collating-element machinery
//! shared with the regular expressions, `regex::text::collating_element`). Otherwise the `[` is
//! an ordinary character. `!` and `^` negate. A range runs over code points without collation
//! rules and over collation-sequence values with them (the regular expressions' order); a
//! reversed range matches nothing, and so does a range whose end is a class or an equivalence
//! class. Inside brackets `\` escapes unless `FNM_NOESCAPE`.
//!
//! # Matching
//!
//! The pattern becomes a list of items; matching computes, item by item, the set of string
//! positions that can be reached (a bit set), so the time is polynomial in the lengths
//! whatever the pattern (no backtracking): `*` and the extended groups never make it
//! exponential. `!(...)` is the complement, among the strings starting at a position, of what
//! its alternatives match there.

#![allow(clippy::arithmetic_side_effects)] // positions and counts are bounded by the lengths of the pattern and the string

use crate::ctype::{self, WClass};
use crate::error::{Errno, Result};
use crate::locale::Locale;
use crate::mem::TryVec;

use super::regex::text::{self, Mode, Order, Unit};

/// `FNM_PATHNAME`: wildcards and bracket expressions never match `/`.
pub const PATHNAME: i32 = 1;
/// `FNM_NOESCAPE`: `\` is an ordinary character.
pub const NOESCAPE: i32 = 2;
/// `FNM_PERIOD`: a leading `.` must be matched by a literal `.`.
pub const PERIOD: i32 = 4;
/// `FNM_LEADING_DIR`: the pattern may match a prefix of the string that a `/` follows.
pub const LEADING_DIR: i32 = 8;
/// `FNM_CASEFOLD`: characters and ranges are compared without case.
pub const CASEFOLD: i32 = 16;
/// `FNM_EXTMATCH`: the ksh extended patterns `?(..)`, `*(..)`, `+(..)`, `@(..)`, `!(..)`.
pub const EXTMATCH: i32 = 32;
/// `FNM_NOMATCH`.
pub const NOMATCH: i32 = 1;

/// Nesting of extended groups beyond this is refused (an error, never a wrong answer).
const MAX_DEPTH: usize = 64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Op {
    /// `?(...)`.
    Optional,
    /// `*(...)`.
    Many,
    /// `+(...)`.
    Some,
    /// `@(...)`.
    One,
    /// `!(...)`.
    Not,
}

#[derive(Debug)]
enum Member {
    Char(u32),
    /// A collating symbol `[.x.]`: the character itself, never folded (observed).
    Symbol(u32),
    Range(u32, u32),
    Class(WClass),
    Equiv(u32),
    /// A range whose end is a class or an equivalence class: matches nothing.
    Nothing,
}

#[derive(Debug)]
struct Set {
    negated: bool,
    members: TryVec<Member>,
}

#[derive(Debug)]
enum Item {
    Lit(u32),
    Any,
    Star,
    Set(Set),
    Group(Op, TryVec<TryVec<Item>>),
    /// A trailing `\`: nothing matches.
    Never,
}

/// A text decoded into characters: the characters and the byte offset of each (and of the end).
struct Units {
    chars: TryVec<u32>,
    offs: TryVec<usize>,
}

/// `text` as characters in `mode`; `None` if it is not valid text there. The byte offsets are
/// kept only `with_offs` (the parser needs them, the matcher does not).
fn decode(mode: Mode, bytes: &[u8], with_offs: bool) -> Result<Option<Units>> {
    // At most one character per byte: one allocation each.
    let mut chars = TryVec::with_capacity(bytes.len())?;
    let mut offs = if with_offs { TryVec::with_capacity(bytes.len() + 1)? } else { TryVec::new() };
    let mut i = 0;
    loop {
        if with_offs {
            offs.push(i)?;
        }
        match text::unit_at(mode, bytes, i) {
            Unit::Char(c, n) => {
                chars.push(c)?;
                i += n.max(1);
            }
            Unit::Invalid(_) => return Ok(None),
            Unit::End => return Ok(Some(Units { chars, offs })),
        }
    }
}

/// `\`, `/` and `.` as characters.
const BACKSLASH: u32 = 0x5c;
const SLASH: u32 = 0x2f;
const DOT: u32 = 0x2e;

fn ch(b: u8) -> u32 {
    u32::from(b)
}

struct Parser<'a> {
    bytes: &'a [u8],
    u: &'a Units,
    flags: i32,
    mode: Mode,
    loc: &'a Locale,
    /// One past the index of the last `]` in the pattern (0: none). A bracket expression is closed by a
    /// `]` after its first member, so one starting at or after the last `]` never closes: an unclosed
    /// run of `[` is then rejected in O(1) instead of rescanned to the end from every `[` (quadratic).
    last_close: usize,
}

impl Parser<'_> {
    fn at(&self, i: usize) -> Option<u32> {
        self.u.chars.get(i).copied()
    }

    fn escapes(&self) -> bool {
        self.flags & NOESCAPE == 0
    }

    /// The bytes of characters `a..b`.
    fn bytes_of(&self, a: usize, b: usize) -> &[u8] {
        let (Some(&x), Some(&y)) = (self.u.offs.get(a), self.u.offs.get(b)) else { return &[] };
        self.bytes.get(x..y).unwrap_or(&[])
    }

    /// For a `[X` item at `j` (`X` one of `:=.`), the index of the closing `X]`; the name has at
    /// least one character (so `[.].]` names `]`).
    fn item_close(&self, j: usize, to: usize, delim: u32) -> Option<usize> {
        let mut k = j + 3;
        while k + 1 < to {
            if self.at(k) == Some(delim) && self.at(k + 1) == Some(ch(b']')) {
                return Some(k);
            }
            k += 1;
        }
        None
    }

    /// A `[:`, `[=` or `[.` item at `j`: its kind, the value (a class, or a character), and the
    /// index after it. `None`: not a valid item (the bracket expression is then invalid).
    fn special(&self, j: usize, to: usize) -> Option<(u32, Special, usize)> {
        let d = self.at(j + 1)?;
        let k = self.item_close(j, to, d)?;
        let name = self.bytes_of(j + 2, k);
        let v = if d == ch(b':') {
            Special::Class(ctype::wctype(name, self.loc)?)
        } else {
            Special::Char(text::collating_element(self.mode, name)?)
        };
        Some((d, v, k + 2))
    }

    fn is_special_start(&self, j: usize, to: usize) -> bool {
        self.at(j) == Some(ch(b'[')) && j + 1 < to && matches!(self.at(j + 1), Some(c) if c == ch(b':') || c == ch(b'=') || c == ch(b'.'))
    }

    /// The bracket expression whose `[` is just before `i`: the set and the index after its `]`,
    /// or `None` when no valid bracket expression follows.
    fn bracket(&self, i: usize, to: usize, build: bool) -> Result<Option<(Set, usize)>> {
        if self.last_close < i + 2 {
            return Ok(None);
        }
        let mut j = i;
        let negated = matches!(self.at(j), Some(c) if j < to && (c == ch(b'!') || c == ch(b'^')));
        if negated {
            j += 1;
        }
        let mut set = Set { negated, members: TryVec::new() };
        let mut first = true;
        let add = |set: &mut Set, m: Member| -> Result<()> { if build { set.members.push(m) } else { Ok(()) } };
        loop {
            if j >= to {
                return Ok(None);
            }
            let Some(c) = self.at(j) else { return Ok(None) };
            if c == ch(b']') && !first {
                return Ok(Some((set, j + 1)));
            }
            first = false;
            let start: u32;
            let mut symbol = false;
            if self.is_special_start(j, to) {
                let Some((d, v, after)) = self.special(j, to) else { return Ok(None) };
                j = after;
                match v {
                    Special::Class(w) => {
                        add(&mut set, Member::Class(w))?;
                        continue;
                    }
                    Special::Char(e) if d == ch(b'=') => {
                        add(&mut set, Member::Equiv(e))?;
                        continue;
                    }
                    Special::Char(e) => {
                        start = e;
                        symbol = true;
                    }
                }
            } else if c == BACKSLASH && self.escapes() {
                let Some(e) = self.at(j + 1).filter(|_| j + 1 < to) else { return Ok(None) };
                start = e;
                j += 2;
            } else {
                start = c;
                j += 1;
            }
            // A range?
            if self.at(j) == Some(ch(b'-')) && j + 1 < to && self.at(j + 1) != Some(ch(b']')) {
                let mut k = j + 1;
                let end = if self.is_special_start(k, to) {
                    let Some((d, v, after)) = self.special(k, to) else { return Ok(None) };
                    k = after;
                    match v {
                        Special::Char(e) if d == ch(b'.') => Some(e),
                        _ => None,
                    }
                } else if self.at(k) == Some(BACKSLASH) && self.escapes() {
                    let Some(e) = self.at(k + 1).filter(|_| k + 1 < to) else { return Ok(None) };
                    k += 2;
                    Some(e)
                } else {
                    let e = self.at(k);
                    k += 1;
                    e
                };
                add(&mut set, end.map_or(Member::Nothing, |hi| Member::Range(start, hi)))?;
                j = k;
                continue;
            }
            add(&mut set, if symbol { Member::Symbol(start) } else { Member::Char(start) })?;
        }
    }

    /// For an extended group whose `(` is just before `i`: the index of its `)`.
    fn group_close(&self, i: usize, to: usize) -> Result<Option<usize>> {
        let mut depth = 1usize;
        let mut j = i;
        while j < to {
            let Some(c) = self.at(j) else { break };
            if c == ch(b'[') {
                if let Some((_, after)) = self.bracket(j + 1, to, false)? {
                    j = after;
                    continue;
                }
            } else if c == ch(b'(') {
                depth += 1;
            } else if c == ch(b')') {
                depth -= 1;
                if depth == 0 {
                    return Ok(Some(j));
                }
            }
            j += 1;
        }
        Ok(None)
    }

    /// The alternatives of a group's text `i..close`, split at its top-level `|`.
    fn alternatives(&self, i: usize, close: usize, depth: usize) -> Result<TryVec<TryVec<Item>>> {
        let mut alts = TryVec::new();
        let mut start = i;
        let mut level = 0usize;
        let mut j = i;
        while j < close {
            let Some(c) = self.at(j) else { break };
            if c == ch(b'[') {
                if let Some((_, after)) = self.bracket(j + 1, close, false)? {
                    j = after;
                    continue;
                }
            } else if c == ch(b'(') {
                level += 1;
            } else if c == ch(b')') {
                level = level.saturating_sub(1);
            } else if c == ch(b'|') && level == 0 {
                alts.push(self.seq(start, j, depth + 1)?)?;
                start = j + 1;
            }
            j += 1;
        }
        alts.push(self.seq(start, close, depth + 1)?)?;
        Ok(alts)
    }

    fn seq(&self, from: usize, to: usize, depth: usize) -> Result<TryVec<Item>> {
        if depth > MAX_DEPTH {
            return Err(Errno::EINVAL);
        }
        // At most one item per character.
        let mut items = TryVec::with_capacity(to.saturating_sub(from))?;
        let mut i = from;
        while i < to {
            let Some(c) = self.at(i) else { break };
            if c == BACKSLASH && self.escapes() {
                match self.at(i + 1).filter(|_| i + 1 < to) {
                    Some(e) => items.push(Item::Lit(e))?,
                    None => {
                        items.push(Item::Never)?;
                        break;
                    }
                }
                i += 2;
                continue;
            }
            if self.flags & EXTMATCH != 0 && self.at(i + 1) == Some(ch(b'(')) && i + 1 < to {
                let op = match u8::try_from(c) {
                    Ok(b'?') => Some(Op::Optional),
                    Ok(b'*') => Some(Op::Many),
                    Ok(b'+') => Some(Op::Some),
                    Ok(b'@') => Some(Op::One),
                    Ok(b'!') => Some(Op::Not),
                    _ => None,
                };
                if let Some(op) = op {
                    if let Some(close) = self.group_close(i + 2, to)? {
                        let alts = self.alternatives(i + 2, close, depth)?;
                        items.push(Item::Group(op, alts))?;
                        i = close + 1;
                        continue;
                    }
                }
            }
            if c == ch(b'?') {
                items.push(Item::Any)?;
            } else if c == ch(b'*') {
                items.push(Item::Star)?;
            } else if c == ch(b'[') {
                if let Some((set, after)) = self.bracket(i + 1, to, true)? {
                    items.push(Item::Set(set))?;
                    i = after;
                    continue;
                }
                items.push(Item::Lit(c))?;
            } else {
                items.push(Item::Lit(c))?;
            }
            i += 1;
        }
        Ok(items)
    }
}

enum Special {
    Class(WClass),
    Char(u32),
}

/// Words of [`Bits`] held in place: strings of up to 255 characters allocate nothing.
const INLINE_WORDS: usize = 4;

/// A set of string positions `0..=n`: in place up to `64 * INLINE_WORDS` positions, on the
/// heap beyond.
struct Bits {
    words: usize,
    inline: [u64; INLINE_WORDS],
    heap: TryVec<u64>,
}

impl Bits {
    fn new(n: usize) -> Result<Bits> {
        let words = n / 64 + 1;
        let mut heap = TryVec::new();
        if words > INLINE_WORDS {
            heap.resize(words, 0u64)?;
        }
        Ok(Bits { words, inline: [0; INLINE_WORDS], heap })
    }
    fn w(&self) -> &[u64] {
        if self.words > INLINE_WORDS { self.heap.as_slice() } else { self.inline.get(..self.words).unwrap_or(&[]) }
    }
    fn w_mut(&mut self) -> &mut [u64] {
        if self.words > INLINE_WORDS { self.heap.as_mut_slice() } else { self.inline.get_mut(..self.words).unwrap_or(&mut []) }
    }
    fn set(&mut self, i: usize) {
        if let Some(x) = self.w_mut().get_mut(i / 64) {
            *x |= 1 << (i % 64);
        }
    }
    fn get(&self, i: usize) -> bool {
        self.w().get(i / 64).is_some_and(|x| (x >> (i % 64)) & 1 != 0)
    }
    /// The set positions below `n`, in order; a zero word is skipped whole, so a sparse set costs
    /// its size, not `n` (a run of literal items over a long string was quadratic).
    fn ones(&self, n: usize) -> impl Iterator<Item = usize> + '_ {
        self.w().iter().enumerate().filter(|(_, x)| **x != 0).flat_map(|(k, x)| {
            let mut bits = *x;
            core::iter::from_fn(move || {
                if bits == 0 {
                    return None;
                }
                let t = usize::try_from(bits.trailing_zeros()).ok()?;
                bits &= bits - 1;
                Some(k * 64 + t)
            })
        }).take_while(move |&p| p < n)
    }
    fn or(&mut self, o: &Bits) {
        for (a, b) in self.w_mut().iter_mut().zip(o.w().iter()) {
            *a |= *b;
        }
    }
    fn try_clone(&self) -> Result<Bits> {
        Ok(Bits { words: self.words, inline: self.inline, heap: self.heap.try_clone()? })
    }
    fn any(&self) -> bool {
        self.w().iter().any(|x| *x != 0)
    }
    fn same(&self, o: &Bits) -> bool {
        self.w() == o.w()
    }
}

struct Matcher<'a> {
    s: &'a [u32],
    flags: i32,
    mode: Mode,
    loc: &'a Locale,
    work: &'a core::cell::Cell<u64>,
}

impl Matcher<'_> {
    fn n(&self) -> usize {
        self.s.len()
    }

    fn leading(&self, p: usize) -> bool {
        p == 0 || (self.flags & PATHNAME != 0 && self.s.get(p - 1) == Some(&SLASH))
    }

    /// A leading period under `FNM_PERIOD` at `p`.
    fn period_at(&self, p: usize) -> bool {
        self.flags & PERIOD != 0 && self.s.get(p) == Some(&DOT) && self.leading(p)
    }

    /// Whether a wildcard or a bracket expression may consume the character at `p`.
    fn wild(&self, p: usize) -> bool {
        match self.s.get(p) {
            None => false,
            Some(&c) if c == SLASH && self.flags & PATHNAME != 0 => false,
            Some(_) if self.period_at(p) => false,
            Some(_) => true,
        }
    }

    fn fold(&self, c: u32) -> u32 {
        if self.flags & CASEFOLD != 0 { text::lower(self.mode, self.loc, c) } else { c }
    }

    fn same(&self, a: u32, b: u32) -> bool {
        a == b || (self.flags & CASEFOLD != 0 && self.fold(a) == self.fold(b))
    }

    fn in_set(&self, set: &Set, c: u32) -> bool {
        let hit = set.members.iter().any(|m| self.member_has(m, c));
        hit != set.negated
    }

    /// Whether the bracket member `m` matches the character `c`.
    fn member_has(&self, m: &Member, c: u32) -> bool {
        let order = |x: u32| match text::order(self.mode, self.loc, x) {
            Order::At(v) => Some(v),
            Order::None => None,
        };
        match m {
            Member::Char(x) => self.same(*x, c),
            Member::Symbol(x) => *x == c,
            Member::Range(lo, hi) => match (order(self.fold(*lo)), order(self.fold(*hi)), order(self.fold(c))) {
                (Some(a), Some(b), Some(v)) => a <= v && v <= b,
                _ => false,
            },
            Member::Class(w) => text::in_class(self.mode, self.loc, c, *w),
            Member::Equiv(e) => {
                let (pc, nc) = text::primary(self.mode, self.loc, c);
                let (pe, ne) = text::primary(self.mode, self.loc, *e);
                pc.get(..nc) == pe.get(..ne)
            }
            Member::Nothing => false,
        }
    }

    fn run(&self, items: &[Item], from: Bits) -> Result<Bits> {
        let mut cur = from;
        for it in items {
            if !cur.any() {
                break;
            }
            cur = self.item(it, &cur)?;
        }
        Ok(cur)
    }

    /// The mirror of [`Self::run`]: the positions from which the whole of `items` can match up
    /// to the end of the string (`end` = the positions it may stop at, here just the end).
    /// Plain items only (`None` for a group or a dead item: the caller matches each suffix
    /// itself); no period or pathname rule applies, which the caller has excluded.
    fn run_back(&self, items: &[Item], end: Bits) -> Result<Option<Bits>> {
        let n = self.n();
        let mut cur = end;
        for it in items.iter().rev() {
            if !cur.any() {
                break;
            }
            let mut out = Bits::new(n)?;
            self.work.set(self.work.get().saturating_add(u64::try_from(n + 1).unwrap_or(u64::MAX)));
            match it {
                Item::Lit(x) => {
                    for p in 0..n {
                        if cur.get(p + 1) && self.s.get(p).is_some_and(|&c| self.same(*x, c)) {
                            out.set(p);
                        }
                    }
                }
                Item::Any => {
                    for p in 0..n {
                        if cur.get(p + 1) && self.wild(p) {
                            out.set(p);
                        }
                    }
                }
                Item::Set(set) => {
                    for p in 0..n {
                        if cur.get(p + 1) && self.wild(p) && self.s.get(p).is_some_and(|&c| self.in_set(set, c)) {
                            out.set(p);
                        }
                    }
                }
                Item::Star => {
                    let mut active = false;
                    for q in (0..=n).rev() {
                        if cur.get(q) {
                            active = true;
                        }
                        if active {
                            out.set(q);
                            if q == 0 || !self.wild(q - 1) {
                                active = false;
                            }
                        }
                    }
                }
                // A trailing `\`: nothing matches, so no position is reached (WORDEXP-TIMEOUT-2: giving
                // up here sent `${x%...\}` to one match per suffix, cubic in the value).
                Item::Never => {}
                Item::Group(..) => return Ok(None),
            }
            cur = out;
        }
        Ok(Some(cur))
    }

    /// The positions reached by one of `alts` from the positions `from`.
    fn one_of(&self, alts: &[TryVec<Item>], from: &Bits) -> Result<Bits> {
        let mut out = Bits::new(self.n())?;
        for a in alts {
            let r = self.run(a, from.try_clone()?)?;
            out.or(&r);
        }
        Ok(out)
    }

    fn closure(&self, alts: &[TryVec<Item>], from: Bits) -> Result<Bits> {
        let mut acc = from;
        loop {
            let mut next = self.one_of(alts, &acc)?;
            next.or(&acc);
            if next.same(&acc) {
                return Ok(acc);
            }
            acc = next;
        }
    }

    fn item(&self, it: &Item, from: &Bits) -> Result<Bits> {
        let n = self.n();
        self.work.set(self.work.get().saturating_add(u64::try_from(n / 64 + 1).unwrap_or(u64::MAX)));
        let mut out = Bits::new(n)?;
        match it {
            Item::Lit(x) => {
                for p in from.ones(n) {
                    if self.s.get(p).is_some_and(|&c| self.same(*x, c)) {
                        out.set(p + 1);
                    }
                }
            }
            Item::Any => {
                for p in from.ones(n).filter(|&p| self.wild(p)) {
                    out.set(p + 1);
                }
            }
            Item::Set(set) => {
                for p in from.ones(n).filter(|&p| self.wild(p)) {
                    if self.s.get(p).is_some_and(|&c| self.in_set(set, c)) {
                        out.set(p + 1);
                    }
                }
            }
            Item::Star => {
                // One sweep: a span that starts at a reached position goes on while the
                // character under it may be consumed by a wildcard.
                let mut active = false;
                for q in 0..=n {
                    // A leading period is never matched by `*`, not even by its empty match
                    // (XCU 2.13.3: the period must be the pattern's own; observed).
                    if from.get(q) && !self.period_at(q) {
                        active = true;
                    }
                    if active {
                        out.set(q);
                        if !self.wild(q) {
                            active = false;
                        }
                    }
                }
            }
            Item::Never => {}
            Item::Group(op, alts) => {
                out = match op {
                    Op::One => self.one_of(alts, from)?,
                    Op::Optional => {
                        let mut r = self.one_of(alts, from)?;
                        r.or(from);
                        r
                    }
                    Op::Many => self.closure(alts, from.try_clone()?)?,
                    Op::Some => {
                        let first = self.one_of(alts, from)?;
                        self.closure(alts, first)?
                    }
                    Op::Not => {
                        for p in (0..=n).filter(|&p| from.get(p)) {
                            let mut single = Bits::new(n)?;
                            single.set(p);
                            let ends = self.one_of(alts, &single)?;
                            for q in (p..=n).filter(|&q| !ends.get(q)) {
                                out.set(q);
                            }
                        }
                        out
                    }
                };
            }
        }
        Ok(out)
    }
}

/// The compiled items of a pattern: for the locale's characters (when the pattern is valid
/// text there) and for bytes.
struct Compiled {
    multi: Option<TryVec<Item>>,
    bytes: TryVec<Item>,
}

/// A pattern for repeated matching: the text, compiled to items on first need. Matching a
/// string [`direct`] needs no items, so the many names of a directory scan (glob) never pay
/// for compiling a plain pattern.
pub struct Pattern {
    flags: i32,
    mode: Mode,
    raw: TryVec<u8>,
    compiled: core::cell::OnceCell<Compiled>,
    work: core::cell::Cell<u64>,
}

impl Pattern {
    /// Compile `pattern` under `flags` in `loc`. An error only without memory, or for extended
    /// groups nested deeper than 64.
    pub fn new(pattern: &[u8], flags: i32, loc: &Locale) -> Result<Pattern> {
        let p = Pattern { flags, mode: Mode::of(loc), raw: TryVec::from_slice(pattern)?, compiled: core::cell::OnceCell::new(), work: core::cell::Cell::new(0) };
        // Extended groups nested too deeply are an error here, as they were when every
        // pattern was compiled at once; a plain pattern is compiled only when a match needs it.
        if flags & EXTMATCH != 0 {
            p.items(loc)?;
        }
        Ok(p)
    }

    /// The items, compiled now if they were not yet.
    fn items(&self, loc: &Locale) -> Result<&Compiled> {
        if let Some(c) = self.compiled.get() {
            return Ok(c);
        }
        let (pattern, flags, mode) = (self.raw.as_slice(), self.flags, self.mode);
        let compile = |m: Mode| -> Result<Option<TryVec<Item>>> {
            let Some(u) = decode(m, pattern, true)? else { return Ok(None) };
            let last_close = u.chars.iter().rposition(|&c| c == ch(b']')).map_or(0, |k| k + 1);
            let p = Parser { bytes: pattern, u: &u, flags, mode: m, loc, last_close };
            Ok(Some(p.seq(0, u.chars.len(), 0)?))
        };
        let multi = if mode.multibyte() { compile(mode)? } else { None };
        // Every byte string is valid text byte by byte.
        let bytes = compile(Mode::Bytes)?.unwrap_or_else(TryVec::new);
        Ok(self.compiled.get_or_init(|| Compiled { multi, bytes }))
    }

    /// The general matcher's work in this pattern's calls so far (WALLCLOCK-ASSERT-SWEEP): one unit per
    /// 64 string positions per pattern item advanced forward (one bit-set pass), one per position per item
    /// advanced backward (a byte loop). A count for tests
    /// to bound complexity deterministically instead of by a clock; the direct fast paths do not count.
    #[must_use]
    pub fn work(&self) -> u64 {
        self.work.get()
    }

    /// Whether `string` matches (in `loc`, the locale the pattern was compiled in).
    pub fn matches(&self, string: &[u8], loc: &Locale) -> Result<bool> {
        if let Some(m) = direct(&self.raw, string, self.flags, loc) {
            return Ok(m);
        }
        let c = self.items(loc)?;
        let mode = Mode::of(loc);
        let as_bytes = || -> Result<Units> { Ok(decode(Mode::Bytes, string, false)?.unwrap_or(Units { chars: TryVec::new(), offs: TryVec::new() })) };
        let (items, s, m) = match (&c.multi, mode.multibyte()) {
            (Some(items), true) => match decode(mode, string, false)? {
                Some(u) => (items, u, mode),
                None => (&c.bytes, as_bytes()?, Mode::Bytes),
            },
            _ => (&c.bytes, as_bytes()?, Mode::Bytes),
        };
        let mt = Matcher { s: &s.chars, flags: self.flags, mode: m, loc, work: &self.work };
        let n = s.chars.len();
        let mut start = Bits::new(n)?;
        start.set(0);
        let end = mt.run(items, start)?;
        if end.get(n) {
            return Ok(true);
        }
        if self.flags & LEADING_DIR != 0 {
            return Ok((0..n).any(|q| end.get(q) && s.chars.get(q) == Some(&SLASH)));
        }
        Ok(false)
    }

    /// For every byte offset `e` that is a character boundary of `string` (as
    /// [`text::unit_at`] segments it, invalid bytes one each), whether `matches(&string[..e])`:
    /// the result is indexed by offset (`string.len() + 1` entries; offsets inside a character
    /// are `false`). One pass over the whole string instead of one match per prefix, so the
    /// work stays linear in the string for each pattern item (the prefix-removal operators of
    /// `wordexp` were quadratic). A prefix is read as text exactly when it is valid text, as
    /// in `matches`: the characters up to the first invalid byte are matched as characters, the
    /// longer prefixes byte by byte.
    ///
    /// `None` when the flags make a prefix's match depend on what follows it (`FNM_PERIOD`,
    /// `FNM_LEADING_DIR`): the caller then matches each prefix itself.
    pub fn prefix_matches(&self, string: &[u8], loc: &Locale) -> Result<Option<TryVec<bool>>> {
        if self.flags & (PERIOD | LEADING_DIR) != 0 {
            return Ok(None);
        }
        let mode = Mode::of(loc);
        let len = string.len();
        let mut out = TryVec::new();
        out.resize(len + 1, false)?;
        // Where the first invalid byte is (`len` when the string is all valid text).
        let mut valid_to = 0;
        let c = self.items(loc)?;
        let text_items = match (&c.multi, mode.multibyte()) {
            (Some(items), true) => Some(items),
            _ => None,
        };
        if text_items.is_some() {
            while !matches!(text::unit_at(mode, string, valid_to), Unit::End | Unit::Invalid(_)) {
                if let Unit::Char(_, n) = text::unit_at(mode, string, valid_to) {
                    valid_to += n.max(1);
                }
            }
        }
        // Prefixes that are not valid text (all of them without text items): bytes.
        let first_bytes = if text_items.is_some() { valid_to + 1 } else { 0 };
        if first_bytes <= len {
            let s = decode(Mode::Bytes, string, false)?.unwrap_or(Units { chars: TryVec::new(), offs: TryVec::new() });
            let mt = Matcher { s: &s.chars, flags: self.flags, mode: Mode::Bytes, loc, work: &self.work };
            let mut start = Bits::new(len)?;
            start.set(0);
            let end = mt.run(&c.bytes, start)?;
            for (e, slot) in out.as_mut_slice().iter_mut().enumerate().skip(first_bytes) {
                *slot = end.get(e);
            }
        }
        if let Some(items) = text_items {
            let head = string.get(..valid_to).unwrap_or(&[]);
            if let Some(u) = decode(mode, head, true)? {
                let mt = Matcher { s: &u.chars, flags: self.flags, mode, loc, work: &self.work };
                let mut start = Bits::new(u.chars.len())?;
                start.set(0);
                let end = mt.run(items, start)?;
                for (i, &off) in u.offs.iter().enumerate() {
                    if let Some(slot) = out.as_mut_slice().get_mut(off) {
                        *slot = end.get(i);
                    }
                }
            }
        }
        Ok(Some(out))
    }

    /// The mirror of [`Self::prefix_matches`]: for every byte offset `s` that is a character
    /// boundary of `string`, whether `matches(&string[s..])` (indexed by offset, with one entry
    /// more than `string` has bytes; offsets inside a character are `false`). One backward pass over the string
    /// per pattern item, so the work is linear in the string for each item (the suffix-removal
    /// operators of `wordexp` were cubic). A suffix is read as text exactly when it is valid
    /// text, as in `matches`: those starting after the last invalid byte are matched as
    /// characters, the longer ones byte by byte.
    ///
    /// `None` when the flags make a suffix's match depend on what precedes it (`FNM_PERIOD`,
    /// `FNM_LEADING_DIR`) or the pattern has extended groups: the
    /// caller then matches each suffix itself.
    pub fn suffix_matches(&self, string: &[u8], loc: &Locale) -> Result<Option<TryVec<bool>>> {
        if self.flags & (PERIOD | LEADING_DIR | PATHNAME | EXTMATCH) != 0 {
            return Ok(None);
        }
        let mode = Mode::of(loc);
        let len = string.len();
        let mut out = TryVec::new();
        out.resize(len + 1, false)?;
        let c = self.items(loc)?;
        let text_items = match (&c.multi, mode.multibyte()) {
            (Some(items), true) => Some(items),
            _ => None,
        };
        // Suffixes starting before `text_from` contain an invalid byte: bytes. Those from it on
        // are valid text (all of them without text items: only bytes then).
        let mut text_from = len + 1;
        if text_items.is_some() {
            text_from = 0;
            let mut i = 0;
            loop {
                match text::unit_at(mode, string, i) {
                    Unit::Char(_, n) => i += n.max(1),
                    Unit::Invalid(_) => {
                        i += 1;
                        text_from = i;
                    }
                    Unit::End => break,
                }
            }
        }
        if text_from > 0 {
            let s = decode(Mode::Bytes, string, false)?.unwrap_or(Units { chars: TryVec::new(), offs: TryVec::new() });
            let mt = Matcher { s: &s.chars, flags: self.flags, mode: Mode::Bytes, loc, work: &self.work };
            let mut end = Bits::new(len)?;
            end.set(len);
            let Some(start) = mt.run_back(&c.bytes, end)? else { return Ok(None) };
            for (b, slot) in out.as_mut_slice().iter_mut().enumerate().take(text_from) {
                *slot = start.get(b);
            }
        }
        if let (Some(items), true) = (text_items, text_from <= len) {
            let tail = string.get(text_from..).unwrap_or(&[]);
            if let Some(u) = decode(mode, tail, true)? {
                let mt = Matcher { s: &u.chars, flags: self.flags, mode, loc, work: &self.work };
                let mut end = Bits::new(u.chars.len())?;
                end.set(u.chars.len());
                let Some(start) = mt.run_back(items, end)? else { return Ok(None) };
                for (i, &off) in u.offs.iter().enumerate() {
                    if let Some(slot) = out.as_mut_slice().get_mut(text_from + off) {
                        *slot = start.get(i);
                    }
                }
            }
        }
        Ok(Some(out))
    }
}

/// `fnmatch(pattern, string, flags)` in `loc`: `Ok(true)` for a match. Unknown flag bits are
/// ignored. An error only without memory (or for extended groups nested deeper than 64).
pub fn fnmatch(pattern: &[u8], string: &[u8], flags: i32, loc: &Locale) -> Result<bool> {
    if let Some(m) = direct(pattern, string, flags, loc) {
        return Ok(m);
    }
    Pattern::new(pattern, flags, loc)?.matches(string, loc)
}

/// The match decided directly on the bytes, without compiling the pattern, when it can be: no
/// `FNM_EXTMATCH`, every byte ASCII in a multibyte locale (so bytes are the characters), and
/// bracket expressions made of characters, ranges and `[:class:]` only (`None` otherwise, and
/// the general matcher decides). The rules are the general matcher's, item for item (module
/// documentation): `*` is matched by the classic method that remembers only the last star
/// (a star never consumes a `/` under `FNM_PATHNAME` or a leading period, so where the
/// segments between stars lie never depends on an earlier star), linear for `*` and `?`.
fn direct(p: &[u8], s: &[u8], flags: i32, loc: &Locale) -> Option<bool> {
    let mode = Mode::of(loc);
    if flags & EXTMATCH != 0 || (mode.multibyte() && (!p.is_ascii() || !s.is_ascii())) {
        return None;
    }
    let work = core::cell::Cell::new(0);
    let mt = Matcher { s: &[], flags, mode, loc, work: &work };
    let n = s.len();
    let at = |i: usize| s.get(i).map(|&b| u32::from(b));
    let leading = |q: usize| q == 0 || (flags & PATHNAME != 0 && s.get(q - 1) == Some(&b'/'));
    let period_at = |q: usize| flags & PERIOD != 0 && s.get(q) == Some(&b'.') && leading(q);
    let wild = |q: usize| match s.get(q) {
        None => false,
        Some(&b'/') if flags & PATHNAME != 0 => false,
        Some(_) => !period_at(q),
    };
    let escapes = flags & NOESCAPE == 0;
    // A `[` with no `]` after it can never close: it is a literal, decided in O(1) (C1-TRIAGE:
    // looking for the `]` from every `[` of a long unclosed run was quadratic).
    let last_close = p.iter().rposition(|&c| c == b']');
    let (mut pi, mut si) = (0usize, 0usize);
    // The last star: the pattern after it, and where its match ends now.
    let mut star: Option<(usize, usize)> = None;
    loop {
        // One step of the pattern at `pi` against the string at `si`: the new positions, or
        // `None` for a mismatch.
        let step: Option<(usize, usize)> = match p.get(pi) {
            None => {
                if si == n || (flags & LEADING_DIR != 0 && s.get(si) == Some(&b'/')) {
                    return Some(true);
                }
                None
            }
            // A star never starts at a leading period, not even empty.
            Some(b'*') if period_at(si) => None,
            Some(b'*') => {
                star = Some((pi + 1, si));
                Some((pi + 1, si))
            }
            Some(b'?') => wild(si).then_some((pi + 1, si + 1)),
            Some(b'\\') if escapes => match p.get(pi + 1) {
                Some(&e) => at(si).is_some_and(|c| mt.same(u32::from(e), c)).then_some((pi + 2, si + 1)),
                // A trailing `\`: nothing matches.
                None => return Some(false),
            },
            Some(b'[') if last_close.is_none_or(|k| k <= pi) => at(si).is_some_and(|c| mt.same(u32::from(b'['), c)).then_some((pi + 1, si + 1)),
            Some(b'[') => match bracket_has(p, pi + 1, escapes, &mt, at(si).filter(|_| wild(si)))? {
                Some((hit, after)) => hit.then_some((after, si + 1)),
                None => at(si).is_some_and(|c| mt.same(u32::from(b'['), c)).then_some((pi + 1, si + 1)),
            },
            Some(&c) => at(si).is_some_and(|x| mt.same(u32::from(c), x)).then_some((pi + 1, si + 1)),
        };
        match step {
            Some((np, ns)) => {
                pi = np;
                si = ns;
            }
            None => {
                // Give the last star one more character, if it may take it.
                let Some((sp, ss)) = star else { return Some(false) };
                if !wild(ss) {
                    return Some(false);
                }
                let mut q = ss + 1;
                // When an ordinary character follows the star (compared exactly), the star takes
                // every character up to its next occurrence at once. Those characters are never
                // a leading period (the one before each is not a `/`), so each may be taken
                // unless it is a `/` under `FNM_PATHNAME`.
                if let Some(&l) = p.get(sp).filter(|&&l| flags & CASEFOLD == 0 && !matches!(l, b'*' | b'?' | b'[' | b'\\')) {
                    loop {
                        match s.get(q) {
                            None => return Some(false),
                            Some(&x) if x == l => break,
                            Some(b'/') if flags & PATHNAME != 0 => return Some(false),
                            Some(_) => q += 1,
                        }
                    }
                }
                star = Some((sp, q));
                pi = sp;
                si = q;
            }
        }
    }
}

/// The bracket expression whose `[` is just before `i` in the ASCII pattern `p`, tested against
/// the character `ch` (`None`: no character a bracket may consume there): `Some(Some((matched,
/// after)))` for a valid one, `Some(None)` when no valid one follows (the `[` is then an
/// ordinary character), `None` when it holds what [`direct`] leaves to the general matcher
/// (`[=x=]`, `[.x.]`). The rules are [`Parser::bracket`]'s; the members are tested as they are
/// read, so nothing is stored.
#[allow(clippy::option_option)]
fn bracket_has(p: &[u8], i: usize, escapes: bool, mt: &Matcher<'_>, ch: Option<u32>) -> Option<Option<(bool, usize)>> {
    let mut j = i;
    let negated = matches!(p.get(j), Some(b'!' | b'^'));
    if negated {
        j += 1;
    }
    let mut hit = false;
    let mut add = |m: Member| {
        if !hit {
            hit = ch.is_some_and(|c| mt.member_has(&m, c));
        }
    };
    let mut first = true;
    loop {
        let Some(&c) = p.get(j) else { return Some(None) };
        if c == b']' && !first {
            return Some(Some((ch.is_some() && hit != negated, j + 1)));
        }
        first = false;
        let start: u32;
        if c == b'[' && matches!(p.get(j + 1), Some(b':' | b'=' | b'.')) {
            let d = *p.get(j + 1)?;
            if d != b':' {
                return None;
            }
            // `[:name:]`: the name has at least one character.
            let mut k = j + 3;
            let close = loop {
                match (p.get(k), p.get(k + 1)) {
                    (Some(b':'), Some(b']')) => break k,
                    (Some(_), Some(_)) => k += 1,
                    _ => return Some(None),
                }
            };
            let w = ctype::wctype(p.get(j + 2..close)?, mt.loc);
            let Some(w) = w else { return Some(None) };
            add(Member::Class(w));
            j = close + 2;
            continue;
        } else if c == b'\\' && escapes {
            let Some(&e) = p.get(j + 1) else { return Some(None) };
            start = u32::from(e);
            j += 2;
        } else {
            start = u32::from(c);
            j += 1;
        }
        if p.get(j) == Some(&b'-') && p.get(j + 1).is_some_and(|&x| x != b']') {
            let mut k = j + 1;
            let end = match p.get(k) {
                Some(b'[') if matches!(p.get(k + 1), Some(b':' | b'=' | b'.')) => return None,
                Some(b'\\') if escapes => {
                    let Some(&e) = p.get(k + 1) else { return Some(None) };
                    k += 2;
                    u32::from(e)
                }
                Some(&e) => {
                    k += 1;
                    u32::from(e)
                }
                None => return Some(None),
            };
            add(Member::Range(start, end));
            j = k;
            continue;
        }
        add(Member::Char(start));
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects)]
mod tests {
    use super::*;

    extern crate std;
    use std::vec::Vec;

    /// The direct matcher agrees with the general one on every pattern of up to three tokens
    /// over every string of up to three characters of `a b . / A`, under every combination of
    /// the flags it takes, in the C and C.UTF-8 locales.
    #[test]
    fn the_direct_matcher_agrees_with_the_general_one() {
        let toks: [&[u8]; 17] = [b"a", b"b", b".", b"/", b"*", b"?", b"[ab]", b"[!a]", b"[a-c]", b"[[:upper:]]", b"\\*", b"[", b"]", b"\\", b"[]a]", b"[!]]", b"[Z-a]"];
        let alphabet = *b"ab./A";
        let mut strings: Vec<Vec<u8>> = std::vec![Vec::new()];
        let mut frontier = strings.clone();
        for _ in 0..3 {
            let mut next = Vec::new();
            for s in &frontier {
                for &c in &alphabet {
                    let mut t = s.clone();
                    t.push(c);
                    next.push(t);
                }
            }
            strings.extend(next.iter().cloned());
            frontier = next;
        }
        let mut patterns: Vec<Vec<u8>> = std::vec![Vec::new()];
        let mut level = patterns.clone();
        for _ in 0..3 {
            let mut next = Vec::new();
            for pfx in &level {
                for t in toks {
                    let mut q = pfx.clone();
                    q.extend_from_slice(t);
                    next.push(q);
                }
            }
            patterns.extend(next.iter().cloned());
            level = next;
        }
        let locales = [Locale::C, Locale::c_utf8(b"C.UTF-8")];
        let mut decided = 0usize;
        for loc in &locales {
            for flags in 0..32 {
                for pat in &patterns {
                    let general = Pattern::new(pat, flags, loc).unwrap();
                    for s in &strings {
                        if let Some(d) = direct(pat, s, flags, loc) {
                            decided += 1;
                            assert_eq!(d, general.matches(s, loc).unwrap(), "{:?} {:?} flags {flags}", std::string::String::from_utf8_lossy(pat), std::string::String::from_utf8_lossy(s));
                        }
                    }
                }
            }
        }
        assert!(decided > 100_000, "the direct matcher decides most cases ({decided})");
    }

    /// Every ASCII character has the same classes in `C.UTF-8` through the wide path (which
    /// reads the ASCII table for 0..128, and the code point table from 128) as through the code point table, and every class name
    /// is found by its name and no other.
    // no spec clause: speed-up of the class lookup; behaviour is covered by the test above.
    #[test]
    fn ascii_classes_agree_in_c_utf8_and_names_resolve() {
        let loc = Locale::c_utf8(b"C.UTF-8");
        for c in 0..=130u32 {
            assert_eq!(ctype::wclasses(c, &loc), crate::unicode::classes(c), "{c}");
        }
        let names: [&[u8]; 12] = [b"alnum", b"alpha", b"blank", b"cntrl", b"digit", b"graph", b"lower", b"print", b"punct", b"space", b"upper", b"xdigit"];
        for (k, n) in names.iter().enumerate() {
            assert!(ctype::Class::by_name(n).is_some(), "{k}");
        }
        for n in [&b""[..], b"alph", b"Alpha", b"alphaa", b"uppe", b"xdigits", b"combining"] {
            assert!(ctype::Class::by_name(n).is_none());
        }
    }
}
