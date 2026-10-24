//! `LC_CTYPE` of a compiled locale (spec Appendix D.1.3): byte tables, wide classes and maps,
//! widths, the codeset, and the transliteration table.
#![allow(clippy::arithmetic_side_effects)] // offsets are u32 file values or slice lengths: sums of a few stay far below usize::MAX

use super::file::{u32_at, Category};
use super::table3::Table3;

// Item numbers (`_NL_CTYPE_*` in <langinfo.h>).
const CLASS: usize = 0;
const TOUPPER: usize = 1;
const TOLOWER: usize = 3;
const CLASS32: usize = 5;
const CLASS_NAMES: usize = 10;
const MAP_NAMES: usize = 11;
const WIDTH: usize = 12;
const MB_CUR_MAX: usize = 13;
const CODESET: usize = 14;
const TOUPPER32: usize = 15;
const TOLOWER32: usize = 16;
const CLASS_OFFSET: usize = 17;
const MAP_OFFSET: usize = 18;
const TRANSLIT_TAB_SIZE: usize = 61;
const TRANSLIT_FROM_IDX: usize = 62;
const TRANSLIT_FROM_TBL: usize = 63;
const TRANSLIT_TO_IDX: usize = 64;
const TRANSLIT_TO_TBL: usize = 65;
const TRANSLIT_DEFAULT_MISSING: usize = 67;

/// The standard class names, in the order of the `<ctype.h>` bits (Appendix C.9).
pub const STANDARD_CLASSES: [&[u8]; 12] = [b"upper", b"lower", b"alpha", b"digit", b"xdigit", b"space", b"print", b"graph", b"blank", b"cntrl", b"punct", b"alnum"];
/// The standard map names.
pub const STANDARD_MAPS: [&[u8]; 2] = [b"toupper", b"tolower"];

/// The `LC_CTYPE` magic word.
pub const MAGIC: u32 = 0x2009_0720;

/// A compiled `LC_CTYPE`.
#[derive(Clone, Copy, Debug)]
pub struct CtypeFile {
    cat: Category,
    /// Whether the case maps of characters 0…127 (the byte maps and the wide maps) are ASCII's:
    /// computed once when the file is checked (PJ-23).
    ascii_case: bool,
}

/// Which wide-character map.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Map {
    /// `toupper`.
    Upper,
    /// `tolower`.
    Lower,
}

/// ASCII's case map of bytes 0…127 as little-endian `i32` words (the layout of the byte maps and
/// of a wide map's entries): the mapped byte (`upper`: `toupper`, else `tolower`), or with
/// `delta` the wide map's entry, mapped minus original.
#[allow(clippy::indexing_slicing)] // a const fn (no `get`): `4 * b + 3 < 512` for `b < 128`
const fn ascii_words(upper: bool, delta: bool) -> [u8; 512] {
    let mut o = [0u8; 512];
    let (mut b, mut at) = (0u8, 0usize);
    while b < 128 {
        let m = if upper { b.to_ascii_uppercase() } else { b.to_ascii_lowercase() };
        let word: [u8; 4] = if !delta {
            [m, 0, 0, 0]
        } else if m > b {
            [m - b, 0, 0, 0]
        } else if m < b {
            // −(b − m), a difference below 128, in two's complement.
            [0u8.wrapping_sub(b - m), 0xff, 0xff, 0xff]
        } else {
            [0; 4]
        };
        o[at] = word[0];
        o[at + 1] = word[1];
        o[at + 2] = word[2];
        o[at + 3] = word[3];
        b += 1;
        at += 4;
    }
    o
}

fn names(item: &'static [u8]) -> impl Iterator<Item = &'static [u8]> {
    item.split(|&b| b == 0).take_while(|n| !n.is_empty())
}

impl CtypeFile {
    /// Check the file: the magic, the items this module reads, and the tables' headers.
    pub fn parse(cat: Category) -> Option<CtypeFile> {
        if cat.magic() != MAGIC || cat.len() <= MAP_OFFSET {
            return None;
        }
        let mut f = CtypeFile { cat, ascii_case: false };
        let byte_tables = [(CLASS, 768), (TOUPPER, 1536), (TOLOWER, 1536), (CLASS32, 1024), (TOUPPER32, 1024), (TOLOWER32, 1024)];
        if byte_tables.iter().any(|&(i, len)| cat.item(i).len() < len) {
            return None;
        }
        let nclasses = names(cat.item(CLASS_NAMES)).count();
        let nmaps = names(cat.item(MAP_NAMES)).count();
        // The standard classes and maps come first, in this order, so lookups by class can
        // use fixed positions.
        if !names(cat.item(CLASS_NAMES)).zip(STANDARD_CLASSES).all(|(a, b)| a == b) || !names(cat.item(MAP_NAMES)).zip(STANDARD_MAPS).all(|(a, b)| a == b) {
            return None;
        }
        let co = usize::try_from(cat.word(CLASS_OFFSET)).ok()?;
        let mo = usize::try_from(cat.word(MAP_OFFSET)).ok()?;
        if nclasses < 12 || nmaps < 2 || co.checked_add(nclasses)? > cat.len() || mo.checked_add(nmaps)? > cat.len() {
            return None;
        }
        let tables_ok = (0..nclasses).all(|k| Table3::parse(cat.item(co + k)).is_some())
            && (0..nmaps).all(|k| Table3::parse(cat.item(mo + k)).is_some())
            && Table3::parse(cat.item(WIDTH)).is_some();
        if !tables_ok {
            return None;
        }
        f.ascii_case = f.ascii_case_runs() || f.ascii_case_each();
        Some(f)
    }

    /// The ASCII case check proven by whole-run compares (SETLOCALE-START): the byte maps of
    /// bytes 0…127 are one 512-byte compare each, the wide maps one compare per level-3 run.
    /// `false` when not proven ([`CtypeFile::ascii_case_each`] then decides).
    fn ascii_case_runs(&self) -> bool {
        const UPPER: [u8; 512] = ascii_words(true, false);
        const LOWER: [u8; 512] = ascii_words(false, false);
        const UPPER_DELTA: [u8; 512] = ascii_words(true, true);
        const LOWER_DELTA: [u8; 512] = ascii_words(false, true);
        // Entry `b + 128` of the byte maps is byte `b`.
        let bytes = self.toupper_table().get(512..1024) == Some(&UPPER[..]) && self.tolower_table().get(512..1024) == Some(&LOWER[..]);
        let mo = usize::try_from(self.cat.word(MAP_OFFSET)).unwrap_or(usize::MAX);
        let wide = |k: usize, want: &[u8]| mo.checked_add(k).and_then(|i| Table3::parse(self.cat.item(i))).is_some_and(|t| t.runs_equal(want));
        bytes && wide(0, &UPPER_DELTA) && wide(1, &LOWER_DELTA)
    }

    /// The ASCII case check, one character at a time (the definition of [`CtypeFile::ascii_case`]).
    fn ascii_case_each(&self) -> bool {
        (0..128u8).all(|b| {
            let (lower, upper) = (b.to_ascii_lowercase(), b.to_ascii_uppercase());
            let c = i32::from(b);
            self.byte_map(c, Map::Lower) == i32::from(lower)
                && self.byte_map(c, Map::Upper) == i32::from(upper)
                && self.map(1, u32::from(b)) == u32::from(lower)
                && self.map(0, u32::from(b)) == u32::from(upper)
        })
    }

    /// Whether `tolower`, `toupper`, `towlower` and `towupper` map every character 0…127 as in
    /// ASCII (true of most locales; not of `tr_TR`, whose `I` and `i` map to the dotless and
    /// dotted forms). Where it holds, case folding of those characters needs no table (PJ-23).
    pub fn ascii_case(&self) -> bool {
        self.ascii_case
    }

    /// The underlying category file (for `nl_langinfo`).
    pub fn category(&self) -> Category {
        self.cat
    }

    /// The 384 16-bit class masks of bytes −128…255, as raw item bytes (`__ctype_b`).
    pub fn class_table(&self) -> &'static [u8] {
        self.cat.item(CLASS)
    }

    /// The 384 `i32` uppercase mappings of bytes −128…255 (`__ctype_toupper`).
    pub fn toupper_table(&self) -> &'static [u8] {
        self.cat.item(TOUPPER)
    }

    /// The 384 `i32` lowercase mappings (`__ctype_tolower`).
    pub fn tolower_table(&self) -> &'static [u8] {
        self.cat.item(TOLOWER)
    }

    /// The 256 `u32` class words of bytes 0…255 (`__ctype32_b`).
    pub fn class32_table(&self) -> &'static [u8] {
        self.cat.item(CLASS32)
    }

    /// The class mask of byte `b` (−128…255).
    pub fn byte_class(&self, b: i32) -> u16 {
        let i = usize::try_from(b + 128).unwrap_or(usize::MAX);
        let t = self.class_table();
        t.get(2 * i..2 * i + 2).and_then(|w| <[u8; 2]>::try_from(w).ok()).map_or(0, u16::from_le_bytes)
    }

    /// The byte case mapping of `b` (−128…255).
    pub fn byte_map(&self, b: i32, map: Map) -> i32 {
        let i = usize::try_from(b + 128).unwrap_or(usize::MAX);
        let t = if map == Map::Upper { self.toupper_table() } else { self.tolower_table() };
        super::file::i32_at(t, 4 * i).unwrap_or(b)
    }

    /// The index of the class named `name`.
    pub fn class_index(&self, name: &[u8]) -> Option<usize> {
        names(self.cat.item(CLASS_NAMES)).position(|n| n == name)
    }

    /// The index of the map named `name`.
    pub fn map_index(&self, name: &[u8]) -> Option<usize> {
        names(self.cat.item(MAP_NAMES)).position(|n| n == name)
    }

    /// The number of maps.
    pub fn map_count(&self) -> usize {
        names(self.cat.item(MAP_NAMES)).count()
    }

    /// Whether `wc` is in class number `k`.
    pub fn in_class(&self, k: usize, wc: u32) -> bool {
        let co = usize::try_from(self.cat.word(CLASS_OFFSET)).unwrap_or(usize::MAX);
        co.checked_add(k).and_then(|i| Table3::parse(self.cat.item(i))).is_some_and(|t| t.bit(wc))
    }

    /// The table of map number `k` (its item's bytes).
    pub fn map_table(&self, k: usize) -> &'static [u8] {
        let mo = usize::try_from(self.cat.word(MAP_OFFSET)).unwrap_or(usize::MAX);
        mo.checked_add(k).map_or(&[], |i| self.cat.item(i))
    }

    /// Map `wc` through map number `k`.
    pub fn map(&self, k: usize, wc: u32) -> u32 {
        let mo = usize::try_from(self.cat.word(MAP_OFFSET)).unwrap_or(usize::MAX);
        let delta = mo.checked_add(k).and_then(|i| Table3::parse(self.cat.item(i))).map_or(0, |t| t.value_i32(wc));
        wc.checked_add_signed(delta).unwrap_or(wc)
    }

    /// `wcwidth`: −1 when the table has no entry or 0xff.
    pub fn width(&self, wc: u32) -> i32 {
        match Table3::parse(self.cat.item(WIDTH)).and_then(|t| t.value_u8(wc)) {
            None | Some(0xff) => -1,
            Some(w) => i32::from(w),
        }
    }

    /// The wide tables of the standard classes, the two standard maps and the widths, each
    /// header checked once (D-7.13): what [`CtypeFile::in_class`], [`CtypeFile::map`] and
    /// [`CtypeFile::width`] parse on every call.
    pub fn wide(&self) -> Wide {
        let co = usize::try_from(self.cat.word(CLASS_OFFSET)).unwrap_or(usize::MAX);
        let mo = usize::try_from(self.cat.word(MAP_OFFSET)).unwrap_or(usize::MAX);
        let table = |base: usize, k: usize| base.checked_add(k).and_then(|i| Table3::parse(self.cat.item(i)));
        Wide {
            class: {
                let mut by_bit = [None; 16];
                for (k, m) in crate::ctype::STANDARD_MASKS.iter().enumerate() {
                    if let Some(slot) = by_bit.get_mut(usize::try_from(m.trailing_zeros()).unwrap_or(16)) {
                        *slot = table(co, k);
                    }
                }
                by_bit
            },
            map: [table(mo, 0), table(mo, 1)],
            width: Table3::parse(self.cat.item(WIDTH)),
        }
    }

    /// `MB_CUR_MAX`.
    pub fn mb_cur_max(&self) -> usize {
        usize::try_from(self.cat.word(MB_CUR_MAX)).unwrap_or(1).max(1)
    }

    /// The codeset name.
    pub fn codeset(&self) -> &'static [u8] {
        self.cat.string(CODESET)
    }

    /// The transliteration alternatives for the wide string `from` (an exact entry), as a
    /// sequence of NUL-terminated wide strings ended by an empty one, in `u32` little-endian
    /// units; `None` if the table has no entry.
    pub fn translit(&self, from: &[u32]) -> Option<Translit> {
        let n = usize::try_from(self.cat.word(TRANSLIT_TAB_SIZE)).ok()?;
        let fidx = self.cat.item(TRANSLIT_FROM_IDX);
        let ftbl = self.cat.item(TRANSLIT_FROM_TBL);
        let wide_at = |tbl: &'static [u8], word: usize| -> WideStr { WideStr { data: tbl, at: word } };
        let (mut lo, mut hi) = (0usize, n);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            let w = usize::try_from(u32_at(fidx, 4 * mid)?).ok()?;
            match wide_at(ftbl, w).cmp_with(from) {
                core::cmp::Ordering::Less => lo = mid + 1,
                core::cmp::Ordering::Greater => hi = mid,
                core::cmp::Ordering::Equal => {
                    let t = usize::try_from(u32_at(self.cat.item(TRANSLIT_TO_IDX), 4 * mid)?).ok()?;
                    return Some(Translit { data: self.cat.item(TRANSLIT_TO_TBL), at: t });
                }
            }
        }
        None
    }

    /// The default replacement for untranslatable characters (wide units, may be empty).
    pub fn default_missing(&self) -> WideStr {
        WideStr { data: self.cat.item(TRANSLIT_DEFAULT_MISSING), at: 0 }
    }
}

/// `wcwidth` through the width table `t`: −1 when there is no table, no entry, or 0xff.
fn width_of(t: Option<Table3>, wc: u32) -> i32 {
    match t.and_then(|t| t.value_u8(wc)) {
        None | Some(0xff) => -1,
        Some(w) => i32::from(w),
    }
}

/// A compiled `LC_CTYPE`'s wide tables with their headers already checked ([`CtypeFile::wide`]):
/// a class test, case map or width is one table walk, as the file-reading methods give.
#[derive(Clone, Copy, Debug)]
pub struct Wide {
    /// The standard classes by the bit number of their `Class` mask (`None`: the bit of no
    /// standard class, or no valid table), so that a one-class test indexes it directly.
    class: [Option<Table3>; 16],
    /// `toupper`, `tolower`.
    map: [Option<Table3>; 2],
    width: Option<Table3>,
}

impl Wide {
    /// [`CtypeFile::in_class`] for a standard class `k` (0…11); false for any other `k`.
    #[inline]
    pub fn in_class(&self, k: usize, wc: u32) -> bool {
        crate::ctype::STANDARD_MASKS.get(k).is_some_and(|m| self.in_class_bit(m.trailing_zeros(), wc))
    }

    /// Whether `wc` is in the standard class whose `Class` mask is bit `bit` (mod 16); false
    /// for a bit of no standard class.
    #[inline]
    pub fn in_class_bit(&self, bit: u32, wc: u32) -> bool {
        self.class.get(usize::try_from(bit & 15).unwrap_or(0)).copied().flatten().is_some_and(|t| t.bit(wc))
    }

    /// [`CtypeFile::map`] for map `m` (`Map::Upper` is map 0, `Map::Lower` map 1).
    #[inline]
    pub fn map(&self, m: Map, wc: u32) -> u32 {
        let t = if m == Map::Upper { self.map[0] } else { self.map[1] };
        wc.checked_add_signed(t.map_or(0, |t| t.value_i32(wc))).unwrap_or(wc)
    }

    /// [`CtypeFile::width`].
    #[inline]
    pub fn width(&self, wc: u32) -> i32 {
        width_of(self.width, wc)
    }
}

/// A NUL-terminated wide string inside an item.
#[derive(Clone, Copy, Debug)]
pub struct WideStr {
    data: &'static [u8],
    at: usize,
}

impl WideStr {
    /// The code points, up to the terminator (or the end of the item).
    pub fn chars(&self) -> impl Iterator<Item = u32> + '_ {
        (self.at..).map_while(|i| u32_at(self.data, 4 * i).filter(|&c| c != 0))
    }

    fn cmp_with(&self, other: &[u32]) -> core::cmp::Ordering {
        self.chars().cmp(other.iter().copied())
    }

    /// The number of code points.
    pub fn len(&self) -> usize {
        self.chars().count()
    }

    /// Whether the string is empty.
    pub fn is_empty(&self) -> bool {
        self.chars().next().is_none()
    }
}

/// The alternatives of one transliteration entry.
#[derive(Clone, Copy, Debug)]
pub struct Translit {
    data: &'static [u8],
    at: usize,
}

impl Translit {
    /// The alternatives in preference order. The first may be empty (a combining mark that
    /// transliterates to nothing); an empty string after it ends the list.
    pub fn alternatives(&self) -> impl Iterator<Item = WideStr> + '_ {
        let (mut at, mut first) = (self.at, true);
        core::iter::from_fn(move || {
            let s = WideStr { data: self.data, at };
            if s.is_empty() && !core::mem::replace(&mut first, false) {
                return None;
            }
            first = false;
            at += s.len() + 1;
            Some(s)
        })
    }
}
