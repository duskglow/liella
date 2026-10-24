//! Character classification and case mapping over explicit locales (spec §5).
//!
//! Classes are the twelve of `<ctype.h>`; their bit values are the ABI masks of the table
//! protocol (spec Appendix C.9: `_ISbit(0..11)` for upper, lower, alpha, digit, xdigit, space,
//! print, graph, blank, cntrl, punct, alnum), so the same masks serve the exported tables and
//! `isctype`. In the `"C"` locale only ASCII has classes (D-6.7).

use crate::locale::ctype::{Map, Wide};
use crate::locale::{Ctype, Locale};
use crate::unicode;

/// A set of character classes, as the 16-bit ABI mask.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Class(pub u16);

const fn isbit(b: u16) -> u16 {
    if b < 8 { (1 << b) << 8 } else { (1 << b) >> 8 }
}

impl Class {
    /// `_ISupper`.
    pub const UPPER: Class = Class(isbit(0));
    /// `_ISlower`.
    pub const LOWER: Class = Class(isbit(1));
    /// `_ISalpha`.
    pub const ALPHA: Class = Class(isbit(2));
    /// `_ISdigit`.
    pub const DIGIT: Class = Class(isbit(3));
    /// `_ISxdigit`.
    pub const XDIGIT: Class = Class(isbit(4));
    /// `_ISspace`.
    pub const SPACE: Class = Class(isbit(5));
    /// `_ISprint`.
    pub const PRINT: Class = Class(isbit(6));
    /// `_ISgraph`.
    pub const GRAPH: Class = Class(isbit(7));
    /// `_ISblank`.
    pub const BLANK: Class = Class(isbit(8));
    /// `_IScntrl`.
    pub const CNTRL: Class = Class(isbit(9));
    /// `_ISpunct`.
    pub const PUNCT: Class = Class(isbit(10));
    /// `_ISalnum`.
    pub const ALNUM: Class = Class(isbit(11));

    /// True if any class of `other` is in `self`.
    pub const fn intersects(self, other: Class) -> bool {
        self.0 & other.0 != 0
    }

    /// The union.
    pub const fn with(self, other: Class) -> Class {
        Class(self.0 | other.0)
    }

    /// Look up a class by its `wctype` name; `None` for an unknown name.
    pub fn by_name(name: &[u8]) -> Option<Class> {
        Some(match name {
            b"alnum" => Class::ALNUM,
            b"alpha" => Class::ALPHA,
            b"blank" => Class::BLANK,
            b"cntrl" => Class::CNTRL,
            b"digit" => Class::DIGIT,
            b"graph" => Class::GRAPH,
            b"lower" => Class::LOWER,
            b"print" => Class::PRINT,
            b"punct" => Class::PUNCT,
            b"space" => Class::SPACE,
            b"upper" => Class::UPPER,
            b"xdigit" => Class::XDIGIT,
            _ => return None,
        })
    }
}

/// The classes of an ASCII byte (the `"C"` locale; all other bytes have none).
pub const fn ascii_classes(b: u8) -> Class {
    let c = match b {
        b'A'..=b'Z' => {
            let base = Class::UPPER.0 | Class::ALPHA.0 | Class::ALNUM.0 | Class::PRINT.0 | Class::GRAPH.0;
            if b <= b'F' { base | Class::XDIGIT.0 } else { base }
        }
        b'a'..=b'z' => {
            let base = Class::LOWER.0 | Class::ALPHA.0 | Class::ALNUM.0 | Class::PRINT.0 | Class::GRAPH.0;
            if b <= b'f' { base | Class::XDIGIT.0 } else { base }
        }
        b'0'..=b'9' => Class::DIGIT.0 | Class::XDIGIT.0 | Class::ALNUM.0 | Class::PRINT.0 | Class::GRAPH.0,
        b' ' => Class::SPACE.0 | Class::PRINT.0 | Class::BLANK.0,
        b'\t' => Class::SPACE.0 | Class::BLANK.0 | Class::CNTRL.0,
        b'\n' | 0x0b | 0x0c | b'\r' => Class::SPACE.0 | Class::CNTRL.0,
        0..=0x1f | 0x7f => Class::CNTRL.0,
        0x21..=0x7e => Class::PUNCT.0 | Class::PRINT.0 | Class::GRAPH.0,
        _ => 0,
    };
    Class(c)
}

/// The 16-bit masks of the standard classes in name order (`STANDARD_CLASSES`).
pub(crate) const STANDARD_MASKS: [u16; 12] = [
    Class::UPPER.0, Class::LOWER.0, Class::ALPHA.0, Class::DIGIT.0, Class::XDIGIT.0, Class::SPACE.0,
    Class::PRINT.0, Class::GRAPH.0, Class::BLANK.0, Class::CNTRL.0, Class::PUNCT.0, Class::ALNUM.0,
];

/// The classes of byte `b` in `loc` (the byte table, D-5.3).
pub fn classes(b: u8, loc: &Locale) -> Class {
    match loc.ctype() {
        Ctype::C | Ctype::CUtf8 => ascii_classes(b),
        Ctype::File(f) => Class(f.byte_class(i32::from(b))),
    }
}

/// The classes of wide character (code point) `c` in `loc`. `WEOF` and every value outside the
/// locale's repertoire have none.
pub fn wclasses(c: u32, loc: &Locale) -> Class {
    match loc.ctype() {
        Ctype::C => u8::try_from(c).ok().filter(u8::is_ascii).map_or(Class(0), ascii_classes),
        Ctype::CUtf8 => unicode::classes(c),
        Ctype::File(f) => Class(STANDARD_MASKS.iter().enumerate().filter(|(k, _)| f.in_class(*k, c)).fold(0, |m, (_, b)| m | b)),
    }
}

/// Whether wide character `c` has a class of `mask` in `loc`: `wclasses(c, loc).intersects(mask)`,
/// reading in a compiled locale only the classes `mask` names (D-7.13; one table for one class,
/// where [`wclasses`] reads all twelve).
pub fn is_w(c: u32, mask: Class, loc: &Locale) -> bool {
    match loc.ctype() {
        Ctype::File(f) => STANDARD_MASKS.iter().enumerate().any(|(k, b)| b & mask.0 != 0 && f.in_class(k, c)),
        _ => wclasses(c, loc).intersects(mask),
    }
}

/// [`is_w`] in a compiled locale through its checked wide tables `w` (a locale view, D-7.13).
#[inline]
pub fn is_w_file(c: u32, mask: Class, w: &Wide) -> bool {
    // One class (every `wctype`/`iswalpha` descriptor): one table, found by its bit, instead of
    // testing all twelve masks in a loop (WCTYPE-LOCALE-3). A bit of no standard class has
    // no table, which `in_class_bit` answers `false`, as the loop would.
    if mask.0.is_power_of_two() {
        return w.in_class_bit(mask.0.trailing_zeros(), c);
    }
    STANDARD_MASKS.iter().enumerate().any(|(k, b)| b & mask.0 != 0 && w.in_class(k, c))
}

/// The classes of `c` among the standard ones in a compiled locale through its wide tables `w`
/// (a locale view's Latin-1 table, WCTYPE-LOCALE): `is_w_file(c, m, w)` is
/// `w_classes_file(c, w).intersects(m)` for every mask `m`.
pub fn w_classes_file(c: u32, w: &Wide) -> Class {
    Class(STANDARD_MASKS.iter().enumerate().filter(|(k, _)| w.in_class(*k, c)).fold(0, |m, (_, b)| m | b))
}

/// [`wtrans`] `toupper` (`upper`) or `tolower` in a compiled locale through its checked wide
/// tables `w`; `ascii_case` is the locale's flag (PJ-23).
#[inline]
pub fn wtrans_file(c: u32, upper: bool, ascii_case: bool, w: &Wide) -> u32 {
    if c < 128 && ascii_case {
        let b = u8::try_from(c).unwrap_or(0);
        return u32::from(if upper { b.to_ascii_uppercase() } else { b.to_ascii_lowercase() });
    }
    w.map(if upper { Map::Upper } else { Map::Lower }, c)
}

/// Lowercase counterpart of byte `b` (itself if none).
pub fn to_lower(b: u8, loc: &Locale) -> u8 {
    match loc.ctype() {
        Ctype::C | Ctype::CUtf8 => b.to_ascii_lowercase(),
        Ctype::File(f) if b.is_ascii() && f.ascii_case() => b.to_ascii_lowercase(),
        Ctype::File(f) => u8::try_from(f.byte_map(i32::from(b), Map::Lower)).unwrap_or(b),
    }
}

/// Whether `loc` maps every character 0…127 as ASCII does, in its byte and its wide case maps
/// (always in `"C"` and `"C.UTF-8"`; for a compiled locale, computed once when it is loaded).
pub fn ascii_case(loc: &Locale) -> bool {
    match loc.ctype() {
        Ctype::C | Ctype::CUtf8 => true,
        Ctype::File(f) => f.ascii_case(),
    }
}

/// `a` and `b` compared after `tolower` in `loc` (`strcasecmp`): [`bytes::compare_text_by`],
/// eight bytes at a time where the locale's case maps are ASCII's on 0…127 (PJ-23).
pub fn compare_folded(a: &[u8], b: &[u8], loc: &Locale) -> core::cmp::Ordering {
    let fold = |c| to_lower(c, loc);
    if ascii_case(loc) { crate::bytes::compare_text_ascii_folded(a, b, fold) } else { crate::bytes::compare_text_by(a, b, fold) }
}

/// Uppercase counterpart of byte `b` (itself if none).
pub fn to_upper(b: u8, loc: &Locale) -> u8 {
    match loc.ctype() {
        Ctype::C | Ctype::CUtf8 => b.to_ascii_uppercase(),
        Ctype::File(f) if b.is_ascii() && f.ascii_case() => b.to_ascii_uppercase(),
        Ctype::File(f) => u8::try_from(f.byte_map(i32::from(b), Map::Upper)).unwrap_or(b),
    }
}

/// A class that `wctype` named: a standard class (its mask) or a locale-defined one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WClass {
    /// One of the twelve standard classes.
    Standard(Class),
    /// The locale's class number `k` (12 and up).
    Defined(usize),
}

/// The built-in `C.UTF-8` locale-defined classes, numbered after the standard twelve.
const UTF8_CLASSES: [&[u8]; 2] = [b"combining", b"combining_level3"];

/// Look up a class by name in `loc` (`wctype`); `None` for an unknown name.
pub fn wctype(name: &[u8], loc: &Locale) -> Option<WClass> {
    if let Some(c) = Class::by_name(name) {
        return Some(WClass::Standard(c));
    }
    match loc.ctype() {
        Ctype::C => None,
        Ctype::CUtf8 => UTF8_CLASSES.iter().position(|n| *n == name).map(|k| WClass::Defined(k.saturating_add(12))),
        Ctype::File(f) => f.class_index(name).filter(|k| *k >= 12).map(WClass::Defined),
    }
}

/// Whether `c` is in class `w` (`iswctype`).
pub fn is_wclass(c: u32, w: WClass, loc: &Locale) -> bool {
    match w {
        // Only the table of the class asked for (a compiled locale would otherwise read all twelve).
        WClass::Standard(m) => is_w(c, m, loc),
        WClass::Defined(k) => match loc.ctype() {
            Ctype::C => false,
            Ctype::CUtf8 => match k {
                12 => unicode::is_combining(c),
                13 => unicode::is_combining_level3(c),
                _ => false,
            },
            Ctype::File(f) => f.in_class(k, c),
        },
    }
}

/// A named case mapping (`wctrans`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Trans {
    /// `"tolower"`.
    Lower,
    /// `"toupper"`.
    Upper,
    /// A locale-defined mapping: the locale's map number `k` (2 and up; `"totitle"` is 2 in
    /// `C.UTF-8`).
    Defined(usize),
}

impl Trans {
    /// Look up a standard mapping by name; `None` for any other name.
    pub fn by_name(name: &[u8]) -> Option<Trans> {
        match name {
            b"tolower" => Some(Trans::Lower),
            b"toupper" => Some(Trans::Upper),
            _ => None,
        }
    }

    /// Look up a mapping by name in `loc` (`wctrans`).
    pub fn in_locale(name: &[u8], loc: &Locale) -> Option<Trans> {
        Trans::by_name(name).or_else(|| match loc.ctype() {
            Ctype::C => None,
            Ctype::CUtf8 => (name == b"totitle").then_some(Trans::Defined(2)),
            Ctype::File(f) => f.map_index(name).filter(|k| *k >= 2).map(Trans::Defined),
        })
    }
}

/// Apply a case mapping to wide character `c` (simple 1:1 mapping, D-5.2).
pub fn wtrans(c: u32, t: Trans, loc: &Locale) -> u32 {
    match loc.ctype() {
        Ctype::C => match u8::try_from(c) {
            Ok(b) if b.is_ascii() => u32::from(match t {
                Trans::Lower => b.to_ascii_lowercase(),
                Trans::Upper => b.to_ascii_uppercase(),
                Trans::Defined(_) => b,
            }),
            _ => c,
        },
        // ASCII characters map as in ASCII in Unicode, and in a locale whose flag says so (PJ-23).
        Ctype::CUtf8 | Ctype::File(_) if c < 128 && ascii_case(loc) && matches!(t, Trans::Lower | Trans::Upper) => {
            let b = u8::try_from(c).unwrap_or(0);
            u32::from(if t == Trans::Lower { b.to_ascii_lowercase() } else { b.to_ascii_uppercase() })
        }
        Ctype::CUtf8 => match t {
            Trans::Lower => unicode::to_lower(c),
            Trans::Upper => unicode::to_upper(c),
            Trans::Defined(2) => unicode::to_title(c),
            Trans::Defined(_) => c,
        },
        Ctype::File(f) => match t {
            Trans::Upper => f.map(0, c),
            Trans::Lower => f.map(1, c),
            Trans::Defined(k) => f.map(k, c),
        },
    }
}

/// `tolower` of the `"C"` locale, as the fold of the `*casecmp`/`strcasestr` functions.
pub fn lower_c(b: u8) -> u8 {
    to_lower(b, &Locale::C)
}
