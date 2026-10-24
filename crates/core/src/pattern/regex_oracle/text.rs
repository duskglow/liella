//! Characters for the regular-expression engine: how the subject and the pattern are read in a
//! locale, and the character predicates of bracket expressions (spec §14.9, multibyte awareness).
//!
//! A locale whose `MB_CUR_MAX` is 1 is read byte by byte: every byte is a character, classified
//! and case-mapped through the locale's byte tables. Otherwise the subject is decoded with the
//! locale's encoding at each byte offset: a valid character is one unit, and a byte that does not
//! begin a valid character is an *invalid unit* that only a literal byte of the pattern matches
//! (`.`, bracket expressions and the word operators never match it). Offsets stay byte offsets,
//! and a match may begin at any byte (a pattern byte `\xa9` matches the second byte of `é`).
#![allow(clippy::arithmetic_side_effects)] // offsets are bounded by slice lengths and small constants

use crate::ctype::{self, Class, Trans, WClass};
use crate::locale::collate::{self, Collation};
use crate::locale::Locale;
use crate::mem::TryVec;
use crate::text::{self, Decoded, Encoding, MbState};

/// How a locale's text is read.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Mode {
    /// One byte, one character.
    Bytes,
    /// A multibyte encoding.
    Multi(Encoding),
}

impl Mode {
    pub(crate) fn of(loc: &Locale) -> Mode {
        if loc.mb_cur_max() <= 1 { Mode::Bytes } else { Mode::Multi(Encoding::of(loc)) }
    }

    pub(crate) fn multibyte(self) -> bool {
        matches!(self, Mode::Multi(_))
    }
}

/// One unit of text at a byte offset.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Unit {
    /// A character (code point, or the byte in [`Mode::Bytes`]) and its length in bytes.
    Char(u32, usize),
    /// A byte that does not begin a valid character.
    Invalid(u8),
    /// The end of the text.
    End,
}

/// The unit at `p` of `bytes`.
pub(crate) fn unit_at(mode: Mode, bytes: &[u8], p: usize) -> Unit {
    let Some(rest) = bytes.get(p..).filter(|r| !r.is_empty()) else { return Unit::End };
    let first = rest.first().copied().unwrap_or(0);
    match mode {
        Mode::Bytes => Unit::Char(u32::from(first), 1),
        Mode::Multi(enc) => {
            if first < 0x80 && !matches!(enc, Encoding::Charmap(_)) {
                return Unit::Char(u32::from(first), 1);
            }
            let mut st = MbState::INITIAL;
            match text::decode(&mut st, rest, enc) {
                Decoded::Char(c, n) if n >= 1 => Unit::Char(c, n),
                Decoded::Null => Unit::Char(0, 1),
                _ => Unit::Invalid(first),
            }
        }
    }
}

/// The character that ends exactly at `p` (`None` at the start, or when the bytes before `p` do
/// not end a valid character).
pub(crate) fn char_before(mode: Mode, bytes: &[u8], p: usize) -> Option<u32> {
    if p == 0 {
        return None;
    }
    match mode {
        Mode::Bytes => bytes.get(p - 1).map(|b| u32::from(*b)),
        Mode::Multi(_) => {
            for len in 1..=p.min(8) {
                if let Unit::Char(c, n) = unit_at(mode, bytes.get(..p).unwrap_or(&[]), p - len) {
                    if n == len {
                        return Some(c);
                    }
                }
            }
            None
        }
    }
}

/// Whether character `c` is in class `w`.
pub(crate) fn in_class(mode: Mode, loc: &Locale, c: u32, w: WClass) -> bool {
    match (mode, w) {
        (Mode::Bytes, WClass::Standard(m)) => u8::try_from(c).is_ok_and(|b| ctype::classes(b, loc).intersects(m)),
        _ => ctype::is_wclass(c, w, loc),
    }
}

/// A word character for `\w`, `\b`, `\<`, `\>`: alphanumeric or `_`.
pub(crate) fn is_word(mode: Mode, loc: &Locale, c: u32) -> bool {
    c == u32::from(b'_') || in_class(mode, loc, c, WClass::Standard(Class::ALNUM))
}

pub(crate) fn lower(mode: Mode, loc: &Locale, c: u32) -> u32 {
    match mode {
        Mode::Bytes => u8::try_from(c).map_or(c, |b| u32::from(ctype::to_lower(b, loc))),
        Mode::Multi(_) => ctype::wtrans(c, Trans::Lower, loc),
    }
}

pub(crate) fn upper(mode: Mode, loc: &Locale, c: u32) -> u32 {
    match mode {
        Mode::Bytes => u8::try_from(c).map_or(c, |b| u32::from(ctype::to_upper(b, loc))),
        Mode::Multi(_) => ctype::wtrans(c, Trans::Upper, loc),
    }
}

/// The character a collating-element name denotes in a bracket expression (`[.x.]`, and the
/// representative of `[=x=]`): the name must be exactly one character of the locale, which is
/// then the collating element (XBD 9.3.5; a single character is always a collating element of
/// the locale, multibyte or not). `None` for a multi-character, empty or invalid name (no
/// multi-character collating elements are defined). Shared by the regular expressions, `fnmatch`
/// and `glob`, so the three agree (D-14.4).
pub(crate) fn collating_element(mode: Mode, name: &[u8]) -> Option<u32> {
    match unit_at(mode, name, 0) {
        Unit::Char(c, n) if n == name.len() => Some(c),
        _ => None,
    }
}

/// Characters equal when case is ignored (`REG_ICASE`, back-references).
pub(crate) fn same_folded(mode: Mode, loc: &Locale, a: u32, b: u32) -> bool {
    a == b || lower(mode, loc, a) == lower(mode, loc, b) || upper(mode, loc, a) == upper(mode, loc, b)
}

/// The position of character `c` in the order bracket-expression ranges use: the code point (or
/// byte) without collation rules, the locale's collation-sequence value with them.
pub(crate) enum Order {
    /// The value.
    At(u32),
    /// The character has no place in the order (`REG_ECOLLATE` as a range end point).
    None,
}

pub(crate) fn order(mode: Mode, loc: &Locale, c: u32) -> Order {
    match loc.collation() {
        Collation::CodePoint => Order::At(c),
        Collation::Rules(r) => {
            let v = match mode {
                Mode::Bytes => u8::try_from(c).ok().and_then(|b| r.collseq_byte(b)),
                Mode::Multi(_) => r.collseq_wide(c),
            };
            v.map_or(Order::None, Order::At)
        }
    }
}

/// The primary (first-level) collation weights of `c`, for equivalence classes: two characters
/// are equivalent when these are equal. Without collation rules a character is equivalent only to
/// itself (the weights are then the character). At most 16 units are kept.
pub(crate) fn primary(mode: Mode, loc: &Locale, c: u32) -> ([u32; 16], usize) {
    let mut out = [0u32; 16];
    match loc.collation() {
        Collation::CodePoint => {
            if let Some(o) = out.first_mut() {
                *o = c;
            }
            (out, 1)
        }
        Collation::Rules(r) => {
            let mut n = 0usize;
            let mut push = |u: u32| {
                if let Some(o) = out.get_mut(n) {
                    *o = u;
                    n += 1;
                }
            };
            match mode {
                Mode::Bytes => {
                    let b = [u8::try_from(c).unwrap_or(0)];
                    for u in collate::key(&r, &b[..]).take_while(|u| *u != 1) {
                        push(u);
                    }
                }
                Mode::Multi(_) => {
                    let w = [c];
                    for u in collate::key(&r, &w[..]).take_while(|u| *u != 1) {
                        push(u);
                    }
                }
            }
            (out, n)
        }
    }
}

// ------------------------------------------------------------------------------ character sets

/// The set matches the characters it lists (else the complement).
pub(crate) const SET_NEGATED: u32 = 1;
/// Case is ignored: a character matches if it, its lowercase or its uppercase is listed.
pub(crate) const SET_ICASE: u32 = 2;
/// A newline never matches (`.` without `RE_DOT_NEWLINE`, `[^...]` with
/// `RE_HAT_LISTS_NOT_NEWLINE`).
pub(crate) const SET_NOT_NEWLINE: u32 = 4;
/// NUL never matches (`.` with `RE_DOT_NOT_NULL`).
pub(crate) const SET_NOT_NUL: u32 = 8;
/// Every character is listed (`.`).
pub(crate) const SET_ANY: u32 = 16;
/// The word characters are listed (`\w`).
pub(crate) const SET_WORD: u32 = 32;

/// A bracket expression or another one-character predicate, compiled.
#[derive(Debug, Default)]
pub(crate) struct CharSet {
    pub(crate) flags: u32,
    /// Ranges of characters (code points, or bytes), inclusive.
    pub(crate) ranges: TryVec<(u32, u32)>,
    /// Ranges of collation-sequence values, inclusive (ranges in a locale with collation rules).
    pub(crate) seq: TryVec<(u32, u32)>,
    /// Character classes, encoded by [`class_code`].
    pub(crate) classes: TryVec<u32>,
    /// Equivalence classes, by a representative character.
    pub(crate) equiv: TryVec<u32>,
    /// The result for each character below 128 (or each byte in [`Mode::Bytes`]), filled by
    /// [`CharSet::finish`].
    pub(crate) table: [u64; 4],
}

/// A class as one word: a standard class's mask, or `1 << 31 | k` for the locale's class `k`.
pub(crate) fn class_code(w: WClass) -> u32 {
    match w {
        WClass::Standard(m) => u32::from(m.0),
        WClass::Defined(k) => (1 << 31) | u32::try_from(k).unwrap_or(0x7fff_ffff),
    }
}

pub(crate) fn class_of(code: u32) -> WClass {
    if code & (1 << 31) != 0 {
        WClass::Defined(usize::try_from(code & 0x7fff_ffff).unwrap_or(0))
    } else {
        WClass::Standard(Class(u16::try_from(code).unwrap_or(0)))
    }
}

impl CharSet {
    pub(crate) fn new(flags: u32) -> CharSet {
        CharSet { flags, ..CharSet::default() }
    }

    /// Whether `c` is listed (case not considered).
    fn listed(&self, mode: Mode, loc: &Locale, c: u32) -> bool {
        if self.flags & SET_ANY != 0 {
            return true;
        }
        if self.flags & SET_WORD != 0 && is_word(mode, loc, c) {
            return true;
        }
        if self.ranges.iter().any(|&(lo, hi)| lo <= c && c <= hi) {
            return true;
        }
        if self.classes.iter().any(|&k| in_class(mode, loc, c, class_of(k))) {
            return true;
        }
        if !self.seq.is_empty() {
            if let Order::At(v) = order(mode, loc, c) {
                if self.seq.iter().any(|&(lo, hi)| lo <= v && v <= hi) {
                    return true;
                }
            }
        }
        if !self.equiv.is_empty() {
            let (pc, nc) = primary(mode, loc, c);
            for &e in self.equiv.iter() {
                let (pe, ne) = primary(mode, loc, e);
                if nc == ne && pc.get(..nc) == pe.get(..ne) {
                    return true;
                }
            }
        }
        false
    }

    /// Whether character `c` matches, computed from the definition.
    fn compute(&self, mode: Mode, loc: &Locale, c: u32) -> bool {
        if (self.flags & SET_NOT_NEWLINE != 0 && c == u32::from(b'\n')) || (self.flags & SET_NOT_NUL != 0 && c == 0) {
            return false;
        }
        let mut hit = self.listed(mode, loc, c);
        if !hit && self.flags & SET_ICASE != 0 {
            let (l, u) = (lower(mode, loc, c), upper(mode, loc, c));
            hit = (l != c && self.listed(mode, loc, l)) || (u != c && self.listed(mode, loc, u));
        }
        hit != (self.flags & SET_NEGATED != 0)
    }

    /// Precompute the answers for the bytes (single-byte locales) or for ASCII (multibyte).
    pub(crate) fn finish(&mut self, mode: Mode, loc: &Locale) {
        let n: u32 = if mode.multibyte() { 128 } else { 256 };
        let mut t = [0u64; 4];
        for c in 0..n {
            if self.compute(mode, loc, c) {
                if let Some(w) = t.get_mut(usize::try_from(c >> 6).unwrap_or(0)) {
                    *w |= 1 << (c & 63);
                }
            }
        }
        self.table = t;
    }

    /// Whether character `c` matches.
    pub(crate) fn matches(&self, mode: Mode, loc: &Locale, c: u32) -> bool {
        let limit = if mode.multibyte() { 128 } else { 256 };
        if c < limit {
            return self.table.get(usize::try_from(c >> 6).unwrap_or(0)).is_some_and(|w| (w >> (c & 63)) & 1 != 0);
        }
        self.compute(mode, loc, c)
    }
}
