//! Locale objects (spec §7). Every locale-sensitive function takes a `&Locale` (D-7.1).
//!
//! A [`Locale`] is twelve categories, each the built-in `"C"`, the built-in `"C.UTF-8"`, or a
//! compiled category file (D-7.2, Appendix D.1), with its name. Locale data is never unmapped
//! (D-7.7), so a `Locale` is a small `Copy` value over `'static` data; the process-wide and
//! per-thread selections live in `liella-legacy` (D-7.8). Loading reaches the file system only
//! through a [`Source`], which the POSIX layer implements with read-only mappings.

pub mod archive;
mod builtin_items;
pub mod collate;
pub mod ctype;
pub mod file;
pub mod name;
pub mod table3;
pub mod view;

use collate::{Collate, Collation};
use ctype::CtypeFile;
use file::{Cat, Category};

/// One `nl_langinfo` item: a word (returned as a value) or bytes (returned by address).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Item {
    /// A word item (Appendix D.1.5).
    Word(u32),
    /// The item's data.
    Bytes(&'static [u8]),
}

/// One category's item table: the built-in locales' pointer-free records (no dynamic relocations,
/// LOCALE-ITEMS-RELOC), or `Item`s built from a compiled category.
#[derive(Clone, Copy, Debug)]
pub enum Tab {
    /// The generated records of "C" / "C.UTF-8".
    Packed(&'static [builtin_items::R]),
    /// Decoded items.
    Items(&'static [Item]),
}

impl Tab {
    /// No items.
    pub const EMPTY: Tab = Tab::Items(&[]);

    /// Item `i`, `None` past the end.
    #[inline]
    pub fn get(self, i: usize) -> Option<Item> {
        match self {
            Tab::Items(t) => t.get(i).copied(),
            Tab::Packed(t) => t.get(i).map(|r| r.item()),
        }
    }

    /// The number of items.
    #[inline]
    pub fn len(self) -> usize {
        match self {
            Tab::Items(t) => t.len(),
            Tab::Packed(t) => t.len(),
        }
    }

    /// Whether the table has no items.
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// Whether both are the very same table.
    pub fn same(self, o: Tab) -> bool {
        match (self, o) {
            (Tab::Items(a), Tab::Items(b)) => core::ptr::eq(a, b),
            (Tab::Packed(a), Tab::Packed(b)) => core::ptr::eq(a, b),
            _ => false,
        }
    }
}

/// One category's data.
#[derive(Clone, Copy, Debug)]
pub enum Data {
    /// The built-in `"C"` (and `"POSIX"`).
    C,
    /// The built-in `"C.UTF-8"`.
    CUtf8,
    /// A compiled category.
    File(Category),
}

impl Data {
    /// Whether both name the same category data (the same built-in, or the same mapped file).
    pub fn same(&self, other: &Data) -> bool {
        match (self, other) {
            (Data::C, Data::C) | (Data::CUtf8, Data::CUtf8) => true,
            (Data::File(x), Data::File(y)) => core::ptr::eq(x.bytes(), y.bytes()),
            _ => false,
        }
    }
}

/// The `LC_CTYPE` of a locale.
#[derive(Clone, Copy, Debug)]
pub enum Ctype {
    /// ASCII (D-6.7).
    C,
    /// The built-in Unicode tables (D-5.4).
    CUtf8,
    /// A compiled `LC_CTYPE`.
    File(CtypeFile),
}

/// The name `"C"`, followed by a NUL in memory like every name a [`Locale`] holds (the others
/// are interned with one), so that the C interface can return it as a string.
pub const C_NAME: &[u8] = b"C\0".split_at(1).0;

/// A locale.
#[derive(Clone, Copy, Debug)]
pub struct Locale {
    names: [&'static [u8]; 12],
    data: [Data; 12],
    ctype: Ctype,
    collation: Collation,
    /// The `LC_CTYPE` codeset's charmap, when it is not a built-in encoding (D-6.8).
    charmap: Option<&'static crate::charmap::Charmap>,
    /// Decoded once when the locale is built (LV-1), so the hot paths read fields: the
    /// `LC_CTYPE` encoding (with its charmap attached), `MB_CUR_MAX`, and the `LC_NUMERIC`
    /// radix, thousands separator and grouping (NUL stripped).
    encoding: crate::text::Encoding,
    mb_cur_max: u8,
    radix_str: &'static [u8],
    thousands: &'static [u8],
    grouping: &'static [u8],
    /// The item tables of this locale's categories (LV-3, D-7.13): set for the built-in locales
    /// and when the locale is interned ([`view::LocaleView::new`]); until then `None` and the
    /// reads parse the category files per call.
    tables: Option<&'static Tables>,
}

/// The item tables of a locale's twelve categories, by [`Cat::index`].
#[derive(Debug)]
pub struct Tables(pub(crate) [Tab; 12]);

/// The built-in `"C"` locale's item tables.
pub(crate) const C_CATS: [Tab; 12] = [
    Tab::Packed(&builtin_items::C_0), Tab::Packed(&builtin_items::C_1), Tab::Packed(&builtin_items::C_2), Tab::Packed(&builtin_items::C_3),
    Tab::Packed(&builtin_items::C_4), Tab::Packed(&builtin_items::C_5), Tab::Packed(&builtin_items::C_7), Tab::Packed(&builtin_items::C_8),
    Tab::Packed(&builtin_items::C_9), Tab::Packed(&builtin_items::C_10), Tab::Packed(&builtin_items::C_11), Tab::Packed(&builtin_items::C_12),
];

/// The built-in `"C.UTF-8"` locale's item tables.
pub(crate) const C_UTF8_CATS: [Tab; 12] = [
    Tab::Packed(&builtin_items::C_UTF8_0), Tab::Packed(&builtin_items::C_UTF8_1), Tab::Packed(&builtin_items::C_UTF8_2), Tab::Packed(&builtin_items::C_UTF8_3),
    Tab::Packed(&builtin_items::C_UTF8_4), Tab::Packed(&builtin_items::C_UTF8_5), Tab::Packed(&builtin_items::C_UTF8_7), Tab::Packed(&builtin_items::C_UTF8_8),
    Tab::Packed(&builtin_items::C_UTF8_9), Tab::Packed(&builtin_items::C_UTF8_10), Tab::Packed(&builtin_items::C_UTF8_11), Tab::Packed(&builtin_items::C_UTF8_12),
];

static C_SET: Tables = Tables(C_CATS);
static C_UTF8_SET: Tables = Tables(C_UTF8_CATS);

/// The index of the first NUL in `b` (`b.len()` if none): eight bytes at a time, as the names
/// of `strftime` are a handful of bytes each (LV-4). The lowest flagged byte of the word test is
/// exact (a borrow can only flag bytes above a real zero byte).
#[inline]
pub(crate) fn nul_at(b: &[u8]) -> usize {
    const LOW: u64 = 0x0101_0101_0101_0101;
    let mut i = 0usize;
    while let Some(chunk) = b.get(i..).and_then(<[u8]>::first_chunk::<8>) {
        let w = u64::from_le_bytes(*chunk);
        let zero = w.wrapping_sub(LOW) & !w & (LOW << 7);
        if zero != 0 {
            return i.saturating_add(usize::try_from(zero.trailing_zeros() / 8).unwrap_or(0));
        }
        i = i.saturating_add(8);
    }
    let rest = b.get(i..).unwrap_or(&[]);
    i.saturating_add(rest.iter().position(|&x| x == 0).unwrap_or(rest.len()))
}

impl PartialEq for Locale {
    /// Locales are equal when every category has the same name and the same data.
    fn eq(&self, other: &Locale) -> bool {
        self.names == other.names && self.data.iter().zip(other.data.iter()).all(|(a, b)| a.same(b))
    }
}

impl Eq for Locale {}

impl Locale {
    /// The `"C"` (and `"POSIX"`) locale.
    pub const C: Locale = Locale { names: [C_NAME; 12], data: [Data::C; 12], ctype: Ctype::C, collation: Collation::CodePoint, charmap: None, encoding: crate::text::Encoding::Ascii, mb_cur_max: 1, radix_str: b".", thousands: b"", grouping: b"", tables: Some(&C_SET) };

    /// The built-in `"C.UTF-8"` locale, named as given (`C.UTF-8` or `C.utf8`).
    pub const fn c_utf8(name: &'static [u8]) -> Locale {
        Locale { names: [name; 12], data: [Data::CUtf8; 12], ctype: Ctype::CUtf8, collation: Collation::CodePoint, charmap: None, encoding: crate::text::Encoding::Utf8, mb_cur_max: 6, radix_str: b".", thousands: b"", grouping: b"", tables: Some(&C_UTF8_SET) }
    }

    /// The name of category `cat`.
    pub fn name(&self, cat: Cat) -> &'static [u8] {
        self.names.get(cat.index()).copied().unwrap_or(C_NAME)
    }

    /// The names of all categories, in [`Cat::ALL`] order.
    pub fn names(&self) -> &[&'static [u8]; 12] {
        &self.names
    }

    /// The data of category `cat`.
    pub fn data(&self, cat: Cat) -> Data {
        self.data.get(cat.index()).copied().unwrap_or(Data::C)
    }

    /// This locale with category `cat` replaced. `None` if the data does not suit the
    /// category (a malformed `LC_CTYPE` or `LC_COLLATE`).
    pub fn with(&self, cat: Cat, name: &'static [u8], data: Data) -> Option<Locale> {
        let mut l = *self;
        match cat {
            Cat::Ctype => {
                l.ctype = match data {
                    Data::C => Ctype::C,
                    Data::CUtf8 => Ctype::CUtf8,
                    Data::File(f) => Ctype::File(CtypeFile::parse(f)?),
                };
                l.charmap = None;
            }
            Cat::Collate => {
                l.collation = match data {
                    Data::C | Data::CUtf8 => Collation::CodePoint,
                    Data::File(f) => Collate::parse(f)?,
                };
            }
            _ => {}
        }
        *l.names.get_mut(cat.index())? = name;
        *l.data.get_mut(cat.index())? = data;
        l.tables = None;
        match cat {
            Cat::Ctype => {
                l.encoding = crate::text::Encoding::derive(&l);
                l.mb_cur_max = u8::try_from(l.slow_mb_cur_max()).unwrap_or(u8::MAX);
            }
            Cat::Numeric => {
                l.radix_str = l.string(Cat::Numeric, 0);
                l.thousands = l.string(Cat::Numeric, 1);
                l.grouping = l.string(Cat::Numeric, 2);
            }
            _ => {}
        }
        Some(l)
    }

    /// The `LC_CTYPE`.
    pub fn ctype(&self) -> Ctype {
        self.ctype
    }

    /// The charmap of the `LC_CTYPE` codeset, if it is not a built-in encoding.
    pub fn charmap(&self) -> Option<&'static crate::charmap::Charmap> {
        self.charmap
    }

    /// This locale with the charmap of its `LC_CTYPE` codeset attached (loading does this).
    pub fn with_charmap(&self, cm: &'static crate::charmap::Charmap) -> Locale {
        let mut l = Locale { charmap: Some(cm), ..*self };
        l.encoding = crate::text::Encoding::derive(&l);
        l
    }

    /// The `LC_CTYPE` encoding, decoded when the locale was built (what `Encoding::of` returns).
    pub fn encoding(&self) -> crate::text::Encoding {
        self.encoding
    }

    /// The `LC_NUMERIC` grouping (`GROUPING`), decoded when the locale was built.
    pub fn grouping(&self) -> &'static [u8] {
        self.grouping
    }

    /// Whether the `LC_CTYPE` codeset needs a charmap (it is not UTF-8, ASCII or ISO-8859-1).
    pub fn needs_charmap(&self) -> bool {
        match self.ctype {
            Ctype::File(f) => crate::text::builtin_charset(f.codeset()).is_none(),
            _ => false,
        }
    }

    /// The `LC_COLLATE` rules.
    pub fn collation(&self) -> Collation {
        self.collation
    }

    /// `nl_langinfo` item `index` of category `cat` (D-7.10); an unknown item is `""`. Reads the
    /// category's item table when it has one (LV-3), else [`Locale::item_parsed`].
    #[inline]
    pub fn item(&self, cat: Cat, index: usize) -> Item {
        match self.table(cat) {
            Some(t) => t.get(index).unwrap_or(Item::Bytes(b"\0")),
            None => self.item_parsed(cat, index),
        }
    }

    /// The category's item table, if the locale has them (built-in, or after [`view::LocaleView::new`]).
    #[inline]
    pub fn table(&self, cat: Cat) -> Option<Tab> {
        self.tables?.0.get(cat.index()).copied()
    }

    /// [`Locale::item`] read from the category itself, per call: what the item tables are
    /// built from, and the oracle that tests them.
    pub fn item_parsed(&self, cat: Cat, index: usize) -> Item {
        let empty = Item::Bytes(b"\0");
        match self.data(cat) {
            Data::C => builtin_items::items(false, cat.number()).get(index).unwrap_or(empty),
            Data::CUtf8 => builtin_items::items(true, cat.number()).get(index).unwrap_or(empty),
            Data::File(f) if index >= f.len() => empty,
            Data::File(f) if is_word_item(cat, index) => Item::Word(f.word(index)),
            Data::File(f) => Item::Bytes(f.item(index)),
        }
    }

    /// A string item without its terminator (empty for words and unknown items).
    pub fn string(&self, cat: Cat, index: usize) -> &'static [u8] {
        match self.item(cat, index) {
            Item::Bytes(b) => b.get(..nul_at(b)).unwrap_or(&[]),
            Item::Word(_) => &[],
        }
    }

    /// The codeset of the multibyte encoding (`CODESET`).
    pub fn codeset(&self) -> &'static [u8] {
        self.string(Cat::Ctype, 14)
    }

    /// The radix character (`RADIXCHAR`, `LC_NUMERIC`), as a string.
    pub fn decimal_point(&self) -> &'static [u8] {
        self.radix_str
    }

    /// The radix character's first byte (the common single-byte case).
    pub fn radix(&self) -> u8 {
        self.radix_str.first().copied().unwrap_or(b'.')
    }

    /// The thousands separator (`THOUSEP`).
    pub fn thousands_sep(&self) -> &'static [u8] {
        self.thousands
    }

    /// The largest number of bytes one character takes in this locale's multibyte encoding
    /// (`MB_CUR_MAX`).
    pub fn mb_cur_max(&self) -> usize {
        usize::from(self.mb_cur_max)
    }

    /// `MB_CUR_MAX` read from the `LC_CTYPE` (computed once, at build).
    fn slow_mb_cur_max(&self) -> usize {
        match self.ctype {
            Ctype::C => 1,
            Ctype::CUtf8 => 6,
            Ctype::File(f) => f.mb_cur_max(),
        }
    }
}

impl Locale {
    /// Whether strings collate in code-point order here (no `LC_COLLATE` rules): `strcoll`
    /// is then a byte comparison.
    pub fn code_point_order(&self) -> bool {
        matches!(self.collation, Collation::CodePoint)
    }

    /// `strcoll`'s single pass over two C strings that share `prefix` (no terminator in it), the
    /// rest read byte by byte (D-4.37; see
    /// [`collate::compare_stream`]): `None` where [`Self::compare_bytes`] must decide.
    #[inline]
    pub fn compare_stream(&self, prefix: &[u8], a: impl FnMut() -> u8, b: impl FnMut() -> u8) -> Option<core::cmp::Ordering> {
        match &self.collation {
            Collation::CodePoint => None,
            Collation::Rules(c) => collate::compare_stream(c, prefix, a, b),
        }
    }

    /// `strcoll`'s decision at the first bytes `x` and `y` after a shared `prefix` where two C
    /// strings differ (or both end), when that settles it (STRCOLL-3; see
    /// [`collate::decide_first`]): `None` where [`Self::compare_stream`] must go on.
    #[inline]
    pub fn decide_first(&self, prefix: &[u8], x: u8, y: u8) -> Option<core::cmp::Ordering> {
        match &self.collation {
            Collation::CodePoint => None,
            Collation::Rules(c) => collate::decide_first(c, prefix, x, y),
        }
    }

    /// [`Self::decide_first`] with the prefix's test made by the caller (STRCOLL-4; see
    /// [`collate::decide_first_flag`]): `high` is whether a shared byte or `x` is non-ASCII.
    #[inline]
    pub fn decide_first_flag(&self, high: bool, x: u8, y: u8) -> Option<core::cmp::Ordering> {
        match &self.collation {
            Collation::CodePoint => None,
            Collation::Rules(c) => collate::decide_first_flag(c, high, x, y),
        }
    }

    /// Compare byte strings as `strcoll` does in this locale (D-4.18).
    pub fn compare_bytes(&self, a: &[u8], b: &[u8]) -> core::cmp::Ordering {
        match &self.collation {
            Collation::CodePoint => a.cmp(b),
            Collation::Rules(c) => collate::compare(c, a, b),
        }
    }

    /// Compare `wchar_t` strings as `wcscoll` does in this locale.
    pub fn compare_wide(&self, a: &[i32], b: &[i32]) -> core::cmp::Ordering {
        match &self.collation {
            Collation::CodePoint => a.cmp(b),
            Collation::Rules(c) => collate::compare(c, a, b),
        }
    }

    /// The `strxfrm` key of `s`, byte by byte (cloneable: measured, then written).
    pub fn xfrm_bytes<'a>(&'a self, s: &'a [u8]) -> XfrmKey<'a, &'a [u8]> {
        match &self.collation {
            Collation::CodePoint => XfrmKey::Identity(s, 0),
            Collation::Rules(c) => XfrmKey::Rules(collate::key(c, s)),
        }
    }

    /// The `strxfrm` key of `s` in one pass (STRXFRM-EN): its leading bytes into `out`, as many
    /// as fit (perhaps one byte more, past the length); returns the key's length, or `None`
    /// (nothing written) when the one-pass engine cannot run - then use [`Self::xfrm_bytes`].
    pub fn xfrm_bytes_into(&self, s: &[u8], out: &mut [u8]) -> Option<usize> {
        match &self.collation {
            Collation::CodePoint => {
                let k = s.len().min(out.len());
                out.get_mut(..k)?.copy_from_slice(s.get(..k)?);
                Some(s.len())
            }
            Collation::Rules(c) => collate::key_bytes(c, s, out),
        }
    }

    /// The `wcsxfrm` key of `s` in one pass, as [`Self::xfrm_bytes_into`].
    pub fn xfrm_wide_into(&self, s: &[i32], out: &mut [i32]) -> Option<usize> {
        match &self.collation {
            Collation::CodePoint => {
                let k = s.len().min(out.len());
                out.get_mut(..k)?.copy_from_slice(s.get(..k)?);
                Some(s.len())
            }
            Collation::Rules(c) => collate::key_each(c, s, |i, u| {
                if let Some(o) = out.get_mut(i) {
                    *o = i32::from_ne_bytes(u.to_ne_bytes());
                }
            }),
        }
    }

    /// The `wcsxfrm` key of `s`, unit by unit.
    pub fn xfrm_wide<'a>(&'a self, s: &'a [i32]) -> XfrmKey<'a, &'a [i32]> {
        match &self.collation {
            Collation::CodePoint => XfrmKey::Identity(s, 0),
            Collation::Rules(c) => XfrmKey::Rules(collate::key(c, s)),
        }
    }
}

/// A collation key: the string itself (no rules) or the lazily built key.
#[derive(Clone)]
pub enum XfrmKey<'a, T: collate::Text> {
    /// Code-point order: the units themselves, from a position.
    Identity(T, usize),
    /// The key of the rules.
    Rules(collate::Key<'a, T>),
}

impl<'a> Iterator for XfrmKey<'a, &'a [u8]> {
    type Item = u8;
    fn next(&mut self) -> Option<u8> {
        match self {
            XfrmKey::Identity(s, pos) => {
                let b = s.get(*pos).copied()?;
                *pos = pos.saturating_add(1);
                Some(b)
            }
            XfrmKey::Rules(k) => k.next().map(|u| u8::try_from(u).unwrap_or(u8::MAX)),
        }
    }
}

impl<'a> Iterator for XfrmKey<'a, &'a [i32]> {
    type Item = i32;
    fn next(&mut self) -> Option<i32> {
        match self {
            XfrmKey::Identity(s, pos) => {
                let w = s.get(*pos).copied()?;
                *pos = pos.saturating_add(1);
                Some(w)
            }
            XfrmKey::Rules(k) => k.next().map(|u| i32::from_ne_bytes(u.to_ne_bytes())),
        }
    }
}

/// The word items (Appendix D.1.5).
const fn is_word_item(cat: Cat, index: usize) -> bool {
    match cat {
        Cat::Ctype => matches!(index, 13 | 17 | 18 | 19 | 30 | 51..=61 | 66 | 68 | 70 | 71),
        Cat::Numeric => matches!(index, 3 | 4),
        Cat::Time => matches!(index, 50 | 102),
        Cat::Collate => matches!(index, 0 | 13),
        Cat::Monetary => matches!(index, 38..=41 | 43 | 44),
        Cat::Paper => matches!(index, 0 | 1),
        Cat::Address => index == 6,
        _ => false,
    }
}

/// Where locale data is found (implemented by the POSIX layer over the file system).
pub trait Source {
    /// The bytes of the file at `path` (no terminator), mapped or read once and kept for the
    /// life of the process; `None` if it cannot be read.
    fn file(&self, path: &[u8]) -> Option<&'static [u8]>;
    /// `LOCPATH`, unless unset or in secure mode.
    fn locpath(&self) -> Option<&[u8]>;
    /// The charmap of the charset named `name` (D-6.8), loaded once.
    fn charmap(&self, name: &[u8]) -> Option<&'static crate::charmap::Charmap>;
}

/// The default directory of compiled locales and the archive.
pub const LOCALE_DIR: &[u8] = b"/usr/lib/locale";
/// The locale archive.
pub const ARCHIVE: &[u8] = b"/usr/lib/locale/locale-archive";
/// The locale alias file.
pub const ALIAS_FILE: &[u8] = b"/usr/share/locale/locale.alias";

/// The built-in locale named `name` and the name it reports, if it is one.
pub fn builtin(name: &'static [u8]) -> Option<(Data, &'static [u8])> {
    match name {
        b"C" | b"POSIX" => Some((Data::C, C_NAME)),
        b"C.UTF-8" | b"C.utf8" => Some((Data::CUtf8, name)),
        _ => None,
    }
}

/// `dir/name/file` in a fixed buffer.
fn path<'b>(buf: &'b mut [u8; 3 * name::MAX_NAME], parts: &[&[u8]]) -> Option<&'b [u8]> {
    let mut n = 0usize;
    for p in parts {
        buf.get_mut(n..n.checked_add(p.len())?)?.copy_from_slice(p);
        n = n.checked_add(p.len())?;
    }
    buf.get(..n)
}

/// Whether a candidate's `LC_CTYPE` names the requested codeset (Appendix D.1).
fn codeset_matches(ctype: Option<&'static [u8]>, codeset: Option<&[u8]>) -> bool {
    let Some(cs) = codeset else { return true };
    let Some(f) = ctype.and_then(Category::parse).and_then(CtypeFile::parse) else { return false };
    name::same_codeset(f.codeset(), cs)
}

/// The most archive candidates of a name [`FindCache`] keeps (a name has at most a dozen).
const CACHED_CANDIDATES: usize = 16;

/// What [`find_cached`] keeps between the categories of one selection (`setlocale` or
/// `newlocale` asks for every category of the same name): the parsed archive, the archive's
/// answer for each candidate of the name, and, once a category was not found under the name,
/// the name's alias and the same answers for the alias's candidates. The alias file is scanned
/// and the archive searched once per name instead of once per category. It lives on the
/// caller's stack for one selection: nothing is kept across calls.
#[derive(Default)]
pub struct FindCache {
    /// The name the rest belongs to.
    name: Option<&'static [u8]>,
    archive: Option<Option<archive::Archive>>,
    /// The archive's answers for the name's own candidates.
    named: Walk,
    /// The alias's value, once looked up (`Some(None)`: the name has none, or its value is the
    /// name itself), and the archive's answers for the value's candidates.
    alias: Option<Option<&'static [u8]>>,
    aliased: Walk,
}

/// The archive's answers for the candidates of one name (part of [`FindCache`]).
#[derive(Default)]
struct Walk {
    /// Per candidate, in search order: looked up, the record, and whether its `LC_CTYPE`
    /// names the requested codeset (worked out at the first category that needs it).
    records: [(bool, Option<archive::Record>, Option<bool>); CACHED_CANDIDATES],
    /// The record of the first candidate whose answer no longer depends on the category: every
    /// earlier candidate has no record or the wrong codeset, and this one has the right codeset
    /// (SETLOCALE-START). A later category it holds answers from it without walking the names.
    live: Option<archive::Record>,
}

impl FindCache {
    /// Whether later categories are answered from one record without walking the names
    /// (for the tests).
    #[doc(hidden)]
    pub fn settled(&self) -> bool {
        self.named.live.is_some() || self.aliased.live.is_some()
    }

    /// Whether the alias file was consulted for the current name (for the tests).
    #[doc(hidden)]
    pub fn alias_consulted(&self) -> bool {
        self.alias.is_some()
    }
}

impl Walk {
    /// The record whose categories answer every later lookup that it holds ([`Walk::live`]).
    fn settle(&mut self) {
        for r in &self.records {
            match r {
                (true, None, _) | (true, Some(_), Some(false)) => continue,
                (true, Some(rec), Some(true)) => self.live = Some(*rec),
                _ => {}
            }
            return;
        }
    }
}

/// Find category `cat` of the locale named `requested` (Appendix D.1, D-7.7): built-ins, then
/// each `LOCPATH` directory, or, when `LOCPATH` is unset, the archive and then the default
/// directory. Only when the name is not found there is it looked up in the alias file, and its
/// value searched the same way (owner ruling 2026-10-11: as the reference does).
pub fn find(src: &impl Source, cat: Cat, requested: &'static [u8]) -> Option<Data> {
    find_cached(src, cat, requested, &mut FindCache::default())
}

/// [`find`], keeping in `cache` what does not depend on the category (the answers are
/// [`find`]'s).
pub fn find_cached(src: &impl Source, cat: Cat, requested: &'static [u8], cache: &mut FindCache) -> Option<Data> {
    if let Some((data, _)) = builtin(requested) {
        return Some(data);
    }
    // A name is checked once per selection: `cache.name` only ever holds an acceptable one.
    if cache.name != Some(requested) {
        if !name::acceptable(requested) {
            return None;
        }
        *cache = FindCache::default();
        cache.name = Some(requested);
    }
    if let Some(found) = find_named(src, cat, requested, &mut cache.archive, &mut cache.named) {
        return Some(found);
    }
    let alias = *cache.alias.get_or_insert_with(|| {
        src.file(ALIAS_FILE)
            .and_then(|t| name::lookup_alias(t, requested))
            .filter(|&v| v != requested && name::acceptable(v))
    });
    find_named(src, cat, alias?, &mut cache.archive, &mut cache.aliased)
}

/// Category `cat` of the locale named `target` (no alias), with the archive's answers for
/// `target`'s candidates kept in `walk`. `target` is [`name::acceptable`] (the caller checks it
/// once per selection, SETLOCALE-5).
fn find_named(
    src: &impl Source,
    cat: Cat,
    target: &'static [u8],
    archive_slot: &mut Option<Option<archive::Archive>>,
    walk: &mut Walk,
) -> Option<Data> {
    let parts = name::parse(target);
    let in_dir = |dir: &[u8]| -> Option<Data> {
        name::candidates(&parts, |cand| {
            let mut buf = [0u8; 3 * name::MAX_NAME];
            let data = src.file(path(&mut buf, &[dir, b"/", cand, b"/", cat.file_name()])?)?;
            let f = Category::parse(data)?;
            let ctype = if cat == Cat::Ctype { Some(data) } else { src.file(path(&mut buf, &[dir, b"/", cand, b"/LC_CTYPE"])?) };
            codeset_matches(ctype, parts.codeset).then_some(Data::File(f))
        })
    };
    if let Some(lp) = src.locpath() {
        return lp.split(|&b| b == b':').filter(|d| !d.is_empty()).find_map(in_dir);
    }
    let archive = *archive_slot.get_or_insert_with(|| src.file(ARCHIVE).and_then(archive::Archive::parse));
    // The name walk below would skip every candidate before the live one whatever the category,
    // and answer from the live one when it holds the category.
    if let Some(f) = walk.live.and_then(|rec| rec.category(cat)).and_then(Category::parse) {
        return Some(Data::File(f));
    }
    let records = &mut walk.records;
    let archived = archive.and_then(|a| {
        let mut k = 0usize;
        name::candidates(&parts, |cand| {
            let mut slot = records.get_mut(k);
            k = k.saturating_add(1);
            let rec = match slot.as_deref_mut() {
                Some((true, r, _)) => *r,
                Some(s) => {
                    let r = a.lookup(name::archive_form(&name::parse(cand)).as_bytes());
                    *s = (true, r, None);
                    r
                }
                None => a.lookup(name::archive_form(&name::parse(cand)).as_bytes()),
            }?;
            let f = Category::parse(rec.category(cat)?)?;
            let matches = match slot {
                Some((_, _, Some(m))) => *m,
                Some((_, _, m)) => *m.insert(codeset_matches(rec.category(Cat::Ctype), parts.codeset)),
                None => codeset_matches(rec.category(Cat::Ctype), parts.codeset),
            };
            matches.then_some(Data::File(f))
        })
    });
    if archived.is_some() && walk.live.is_none() {
        walk.settle();
    }
    archived.or_else(|| in_dir(LOCALE_DIR))
}

/// `l` with its `LC_CTYPE` charmap attached when the codeset needs one; `None` when that
/// charmap is not installed (the category cannot be used, D-6.8).
pub fn attach_charmap(src: &impl Source, l: Locale) -> Option<Locale> {
    if !l.needs_charmap() || l.charmap.is_some() {
        return Some(l);
    }
    let Ctype::File(f) = l.ctype else { return Some(l) };
    src.charmap(f.codeset()).map(|cm| l.with_charmap(cm))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::as_conversions)]
mod tests {
    use super::file::{Cat, Category};
    use super::*;
    use crate::text::Encoding;

    /// Compile `name` with `localedef` (once, into this test's own directory) and leak the
    /// bytes of one category file.
    fn category(name: &str, file: &str) -> Category {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-locales-lv1");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        if !path.join(file).exists() {
            // Built in a directory of this process's own and renamed into place: under nextest every test is a
            // process, and two tests compiling into the same directory at once failed in the 5.10 guest
            // (2026-10-08). The thread id is in the name too: under plain `cargo test` two tests of one process compile the
            // same locale at once and shared a pid-only directory ("cannot write output files ... File exists", 2026-10-21).
            // A rename that loses the race leaves the winner's identical locale.
            let (lang, charset) = name.split_once('.').unwrap();
            let tmp = dir.join(format!("{name}.tmp.{}.{:?}", std::process::id(), std::thread::current().id()));
            let _ = std::fs::remove_dir_all(&tmp);
            let st = std::process::Command::new("localedef").args(["-i", lang, "-f", charset]).arg(&tmp).status();
            // localedef reports warnings with status 1 and still writes the locale.
            assert!(st.is_ok_and(|s| s.code().is_some_and(|c| c <= 1)), "localedef {name}");
            if std::fs::rename(&tmp, &path).is_err() {
                let _ = std::fs::remove_dir_all(&tmp);
            }
        }
        let bytes: &'static [u8] = Box::leak(std::fs::read(path.join(file)).unwrap().into_boxed_slice());
        Category::parse(bytes).unwrap_or_else(|| panic!("{name} {file} parses"))
    }

    /// The old, per-call reads: the oracle for the fields decoded at build (LV-1).
    fn check(l: &Locale, what: &str) {
        assert_eq!(l.encoding(), Encoding::derive(l), "{what}: encoding");
        assert_eq!(l.mb_cur_max(), l.slow_mb_cur_max(), "{what}: MB_CUR_MAX");
        assert_eq!(l.decimal_point(), l.string(Cat::Numeric, 0), "{what}: radix");
        assert_eq!(l.thousands_sep(), l.string(Cat::Numeric, 1), "{what}: thousands");
        assert_eq!(l.grouping(), l.string(Cat::Numeric, 2), "{what}: grouping");
        assert_eq!(l.radix(), l.string(Cat::Numeric, 0).first().copied().unwrap_or(b'.'), "{what}: radix byte");
    }

    #[test]
    fn decoded_fields_equal_the_per_call_reads() {
        check(&Locale::C, "C");
        check(&Locale::c_utf8(b"C.UTF-8"), "C.UTF-8");
        // (ctype, numeric) pairs: a UTF-8, a Latin-1 and an unattached non-built-in codeset, with
        // radix/grouping that differ from "C" (de_DE: ',' and '.' grouped by 3).
        let mut seen_comma = false;
        let mut seen_group = false;
        for (ct, nu, enc) in [
            ("en_US.UTF-8", "en_US.UTF-8", Some(Encoding::Utf8)),
            ("de_DE.UTF-8", "de_DE.UTF-8", Some(Encoding::Utf8)),
            ("de_DE.ISO-8859-1", "de_DE.ISO-8859-1", Some(Encoding::Latin1)),
            ("de_DE.UTF-8", "en_US.UTF-8", Some(Encoding::Utf8)),
            ("ja_JP.EUC-JP", "ja_JP.EUC-JP", None),
        ] {
            // Both orders of building: numeric before ctype and after it.
            let a = Locale::C.with(Cat::Ctype, ct.as_bytes(), Data::File(category(ct, "LC_CTYPE"))).unwrap();
            let a = a.with(Cat::Numeric, nu.as_bytes(), Data::File(category(nu, "LC_NUMERIC"))).unwrap();
            let b = Locale::C.with(Cat::Numeric, nu.as_bytes(), Data::File(category(nu, "LC_NUMERIC"))).unwrap();
            let b = b.with(Cat::Ctype, ct.as_bytes(), Data::File(category(ct, "LC_CTYPE"))).unwrap();
            for (l, order) in [(a, "ctype,numeric"), (b, "numeric,ctype")] {
                check(&l, &format!("{ct}+{nu} {order}"));
                if let Some(e) = enc {
                    assert_eq!(l.encoding(), e, "{ct}");
                } else {
                    assert_eq!(l.encoding(), Encoding::Ascii, "{ct}: no charmap attached");
                }
                seen_comma |= l.decimal_point() == b",";
                seen_group |= !l.grouping().is_empty();
            }
            // Replacing the ctype again re-decodes it, and the numeric fields survive.
            let c = a.with(Cat::Ctype, b"C", Data::C).unwrap();
            check(&c, "ctype replaced by C");
            assert_eq!(c.encoding(), Encoding::Ascii);
            assert_eq!(c.mb_cur_max(), 1);
        }
        assert!(seen_comma && seen_group, "the oracle saw a non-C radix and a grouping");
    }

    /// LV-3: every item of every category, read through the item tables a view builds, equals the
    /// read from the category file (`item_parsed`, the oracle), in C, C.UTF-8 and compiled UTF-8,
    /// Latin-1 and EUC-JP locales with all twelve categories from files.
    /// `nul_at` is the first-NUL position for every length, NUL position and trailing content.
    #[test]
    fn nul_at_is_the_first_nul() {
        for n in 0..40usize {
            for z in 0..=n {
                for fill in [1u8, 0x80, 0xff, 0x01] {
                    let mut b = vec![fill; n];
                    if z < n {
                        b[z] = 0;
                        // Later NULs and bytes that mimic a borrow (0x01 above a zero) change nothing.
                        for later in b.iter_mut().skip(z + 1).step_by(3) {
                            *later = 0;
                        }
                    }
                    assert_eq!(nul_at(&b), b.iter().position(|&x| x == 0).unwrap_or(n), "{b:?}");
                }
            }
        }
    }

    /// The built-in `"C"` and `"C.UTF-8"` item tables equal the repository spec's rows
    /// (`spec/abi/langinfo.tsv`, D-7.10): every recorded row byte for byte (words by value),
    /// the table lengths per category, and what an unrecorded in-range item answers
    /// (COLD-TABLE-LOCALE-ITEMS: the equivalence oracle for the table redesign).
    #[test]
    fn builtin_items_equal_the_spec_rows() {
        const TSV: &str = include_str!("../../../../spec/abi/langinfo.tsv");
        // Table lengths by Cat::index (the same in both locales).
        const LENS: [usize; 12] = [72, 6, 159, 19, 46, 5, 3, 7, 13, 5, 2, 16];
        let hex = |h: &str| -> Vec<u8> { (0..h.len() / 2).map(|i| u8::from_str_radix(&h[2 * i..2 * i + 2], 16).unwrap()).collect() };
        let locales = [(Locale::C, 4usize, "C"), (Locale::c_utf8(b"C.UTF-8"), 5, "C.UTF-8")];
        for (l, col, what) in locales {
            let mut recorded = std::collections::HashSet::new();
            let mut mismatches = Vec::new();
            for row in TSV.lines().skip(1) {
                let f: Vec<&str> = row.split('\t').collect();
                let cat = Cat::from_number(f[0].parse().unwrap()).unwrap();
                let i: usize = f[1].parse().unwrap();
                assert!(recorded.insert((cat.index(), i)), "{what}: duplicate row {row}");
                let want = match f[3] {
                    "W" => Item::Word(f[col].parse().unwrap()),
                    "P" => {
                        // A one-byte value (the spec records `ff`, `04`) is stored with a NUL after
                        // it, so a reader that scans a C string stays inside the item.
                        let mut b = hex(f[col]);
                        if b.last().is_some_and(|&x| x != 0) {
                            b.push(0);
                        }
                        Item::Bytes(Box::leak(b.into_boxed_slice()))
                    }
                    k => panic!("kind {k}"),
                };
                if l.item(cat, i) != want {
                    mismatches.push(format!("{what}: {} ({cat:?} {i}): table {:?}, spec {want:?}", f[2], l.item(cat, i)));
                }
            }
            assert!(mismatches.is_empty(), "{mismatches:#?}");
            let mut unrecorded = std::collections::BTreeMap::new();
            for cat in Cat::ALL {
                assert_eq!(l.table(cat).unwrap().len(), LENS[cat.index()], "{what}: {cat:?} length");
                // Past the table: the empty string.
                for i in LENS[cat.index()]..LENS[cat.index()] + 4 {
                    assert_eq!(l.item(cat, i), Item::Bytes(b"\0"), "{what}: {cat:?} past the end");
                }
                for i in 0..LENS[cat.index()] {
                    if !recorded.contains(&(cat.index(), i)) {
                        *unrecorded.entry(format!("{:?}", l.item(cat, i))).or_insert(0usize) += 1;
                    }
                }
            }
            // The 37 items the spec does not record (glibc's own class and collation tables):
            // no data, the empty slice (not the NUL string).
            assert_eq!(format!("{unrecorded:?}"), r#"{"Bytes([])": 37}"#, "{what}: unrecorded");
        }
    }

    #[test]
    fn item_tables_equal_the_parsed_reads() {
        use view::LocaleView;
        let mut locales = vec![(Locale::C, "C".to_string()), (Locale::c_utf8(b"C.UTF-8"), "C.UTF-8".to_string())];
        for name in ["en_US.UTF-8", "de_DE.ISO-8859-1", "ja_JP.EUC-JP"] {
            let mut l = Locale::C;
            for cat in Cat::ALL {
                let file = String::from_utf8(cat.file_name().to_vec()).unwrap();
                l = l.with(cat, name.as_bytes(), Data::File(category(name, &file))).unwrap();
            }
            locales.push((l, name.to_string()));
        }
        let (mut words, mut strings, mut shared) = (0usize, 0usize, 0usize);
        for (l, what) in &locales {
            // Before the view: compiled categories have no table yet.
            assert_eq!(l.table(Cat::Time).is_some(), what.starts_with('C'), "{what}: tables before the view");
            let v = LocaleView::new(*l, 1, &|_, _| None).unwrap();
            // A second view reuses the first one's tables for the same files.
            let v2 = LocaleView::new(*l, 2, &|c, d| if v.locale().data(c).same(d) { v.locale().table(c) } else { None }).unwrap();
            let vl = v.locale();
            for cat in Cat::ALL {
                let t = vl.table(cat).unwrap_or_else(|| panic!("{what}: {cat:?} has a table"));
                assert!(t.same(v2.locale().table(cat).unwrap()), "{what}: {cat:?} table shared");
                shared += 1;
                let number = u32::try_from(cat.number()).unwrap();
                // The name item's index has no entry (`nl_langinfo` relies on it to take its slow path).
                assert!(v.entry(number, 0xffff).is_none() && t.len() < 0xffff, "{what}: {cat:?} name item");
                for i in 0..t.len() + 6 {
                    // The hot lookup (`nl_langinfo`): the same entry; none past the table.
                    match (v.entry(number, i), t.get(i)) {
                        (Some(e), Some(w)) => assert_eq!(e, w, "{what}: {cat:?} entry {i} by number"),
                        (None, None) => {}
                        (e, w) => panic!("{what}: {cat:?} entry {i}: {e:?} vs {w:?}"),
                    }
                    assert!([6u32, 13, 0x7fff, 0xffff].iter().all(|&n| v.entry(n, i).is_none()), "{what}: bad category numbers (entry)");
                    let want = l.item_parsed(cat, i);
                    assert_eq!(vl.item(cat, i), want, "{what}: {cat:?} item {i}");
                    assert_eq!(vl.string(cat, i), l.string(cat, i), "{what}: {cat:?} string {i}");
                    match want {
                        Item::Word(_) => words += 1,
                        Item::Bytes(b) => strings += usize::from(!b.is_empty()),
                    }
                }
            }
        }
        // Not vacuous: word and string items both occur in plenty.
        assert!(words > 100 && strings > 1000 && shared == 5 * 12, "{words} {strings} {shared}");
    }

    /// D-7.13: a locale view's wide class tests, case maps and widths equal the per-call reads
    /// (`wclasses`, `wtrans`, `display_width`: the oracle), and so does the masked `is_w`, for
    /// every code point below 0x30000 and the edge values, in C, C.UTF-8 and compiled UTF-8,
    /// Latin-1 and EUC-JP locales. This covers the view's U+0000..U+00FF table (WCTYPE-LOCALE)
    /// against the table walk for all 256 entries, every one-bit mask and mixed masks.
    #[test]
    fn view_equals_the_per_call_reads() {
        use crate::ctype::{self, Class, Trans};
        use view::LocaleView;
        let mut locales = vec![(Locale::C, "C".to_string()), (Locale::c_utf8(b"C.UTF-8"), "C.UTF-8".to_string())];
        for name in ["en_US.UTF-8", "de_DE.ISO-8859-1", "ja_JP.EUC-JP"] {
            let l = Locale::C.with(Cat::Ctype, name.as_bytes(), Data::File(category(name, "LC_CTYPE"))).unwrap();
            locales.push((l, name.to_string()));
        }
        let masks: Vec<Class> = (0..16).map(|b| Class(1 << b)).chain([Class::ALNUM, Class(Class::ALPHA.0 | Class::DIGIT.0), Class(0xffff), Class(0)]).collect();
        let (mut members, mut mapped, mut wide) = (0usize, 0usize, 0usize);
        for (l, what) in &locales {
            let v = LocaleView::new(*l, 7, &|_, _| None).unwrap();
            assert_eq!(v.handle(), 7);
            assert!(*v.locale() == *l, "{what}: locale");
            for c in (0..0x3_0000u32).chain([0x10_ffff, 0x11_0000, 0x7fff_ffff, 0xffff_ffff]) {
                let all = ctype::wclasses(c, l);
                for &m in &masks {
                    let want = all.intersects(m);
                    assert_eq!(v.isw(c, m), want, "{what}: isw {c:#x} {m:?}");
                    assert_eq!(ctype::is_w(c, m, l), want, "{what}: is_w {c:#x} {m:?}");
                    members += usize::from(want);
                }
                for (upper, t) in [(true, Trans::Upper), (false, Trans::Lower)] {
                    let want = ctype::wtrans(c, t, l);
                    // Twice: the first call fills the Latin-1 cache, the second reads it (WL-REGRESS-2).
                    assert_eq!(v.wtrans(c, upper), want, "{what}: wtrans {c:#x} {upper}");
                    assert_eq!(v.wtrans(c, upper), want, "{what}: wtrans {c:#x} {upper} (cached)");
                    mapped += usize::from(want != c);
                }
                let want = crate::text::display_width(c, l);
                assert_eq!(v.width(c), want, "{what}: width {c:#x}");
                assert_eq!(v.width(c), want, "{what}: width {c:#x} (cached)");
                wide += usize::from(want == 2);
            }
        }
        // The oracle is not vacuous: classes, case maps and double-width characters all occur.
        assert!(members > 100_000 && mapped > 1000 && wide > 1000, "{members} {mapped} {wide}");
        // The "C" view the global module keeps as a static.
        assert!(*LocaleView::c().locale() == Locale::C && LocaleView::c().handle() == 0);
    }
}
