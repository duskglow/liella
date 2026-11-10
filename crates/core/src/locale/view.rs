//! Pre-decoded locale views (D-7.13): what a locale-sensitive call needs, decoded once when the
//! locale is interned. A view is immutable and lives as long as its interned locale (for the
//! rest of the process, D-7.8, D-X.13), so a thread that holds one always reads consistent data.
//!
//! The view holds the [`Locale`] itself (whose `LC_CTYPE` encoding, `MB_CUR_MAX` and
//! `LC_NUMERIC` items are already decoded fields, LV-1), its interned handle, and for a compiled
//! `LC_CTYPE` the wide class, case-map and width tables with their headers checked
//! ([`Wide`]), which the wide-character functions otherwise parse on every call.

use super::ctype::Wide;
use super::file::{Cat, Category};
use super::{is_word_item, Ctype, Data, Item, Locale, Tab};
use crate::ctype::{self, Class, Trans};
use crate::error::Errno;
use crate::mem::{TryBox, TryVec};
use super::classes::{self, ClassTable};
use super::{Tables, C_CATS, C_UTF8_CATS};
use core::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize, Ordering::{Acquire, Relaxed, Release}};

/// The item table of one compiled category: word items decoded, string items as their data.
/// Item `i` equals `Locale::item_parsed` for every `i < len`; past the end the reader answers
/// the empty string.
///
/// The tables of one view share one allocation (SETLOCALE-START: a dozen allocations of a dozen
/// sizes cost more than the items): `out` (`f.len()` slots of it) gets the items of `f`.
///
/// One pass over the offset array (SETLOCALE-2): item `i` runs from offset `i` to offset `i + 1`
/// (the last to the end of the file), which [`Category::parse`] checked to be in order and within
/// the file, so this is [`Category::item`] / [`Category::word`] without re-reading two offsets and
/// re-checking bounds per item; the word items come from a per-category bit mask.
fn fill_table(out: &mut [Item], cat: Cat, f: &Category) {
    let data = f.bytes();
    let n = f.len();
    let words = WORD_ITEMS.get(cat.index()).copied().unwrap_or(0);
    let offsets = data.get(8..n.saturating_mul(4).saturating_add(8)).unwrap_or(&[]);
    let mut starts = offsets.chunks_exact(4).map(|w| w.try_into().map_or(0, |b: [u8; 4]| usize::try_from(u32::from_le_bytes(b)).unwrap_or(0)));
    let Some(mut start) = starts.next() else { return };
    for (i, slot) in out.iter_mut().take(n).enumerate() {
        let end = starts.next().unwrap_or(data.len());
        let item = data.get(start..end).unwrap_or(&[]);
        let word = u32::try_from(i).ok().and_then(|s| words.checked_shr(s)).is_some_and(|w| w & 1 != 0);
        *slot = if word { Item::Word(super::file::u32_at(item, 0).unwrap_or(0)) } else { Item::Bytes(item) };
        start = end;
    }
}

/// [`is_word_item`] of every category ([`Cat::index`] order) as a bit mask of item indexes (every
/// word item index is below 128).
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)] // const evaluation (no `get`): `c < 12`, `i < 128`
const WORD_ITEMS: [u128; 12] = {
    let mut m = [0u128; 12];
    let mut c = 0;
    while c < 12 {
        let mut i = 0;
        while i < 128 {
            if is_word_item(Cat::ALL[c], i) {
                m[c] |= 1u128 << i;
            }
            i += 1;
        }
        c += 1;
    }
    m
};

/// The category tables by [`super::Cat::index`] laid out by C category number (slot 6, `LC_ALL`, empty).
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)] // a const fn (no `get`, no overflow): `n < 13`, the source index is below 12
const fn by_number(t: [Tab; 12]) -> [Tab; 13] {
    let mut o: [Tab; 13] = [Tab::EMPTY; 13];
    let mut n = 0;
    while n < 13 {
        if n != 6 {
            o[n] = t[if n < 6 { n } else { n - 1 }];
        }
        n += 1;
    }
    o
}

/// The `LC_TIME` names `strftime` copies (STRFTIME-NAMES, D-23.12): items 0..40 of the category
/// (`ABDAY_1..7`, `DAY_1..7`, `ABMON_1..12`, `MON_1..12`, `AM_STR`, `PM_STR`) as ready slices
/// (pointer and length, the terminator already cut off), so that a name is one indexed load
/// and no item decoding. An item the table lacks, or a word, is empty (as `Locale::string`).
#[derive(Debug, Clone, Copy)]
pub struct TimeNames([&'static [u8]; TimeNames::N]);

#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)] // `of` is a const fn (no `get`): every index is checked against `N` or the length
impl TimeNames {
    /// The items covered (`AM_STR` and `PM_STR` are the last two).
    pub const N: usize = 40;

    /// The names of the `LC_TIME` item table `t`.
    const fn of(t: Tab) -> TimeNames {
        let mut o: [&'static [u8]; TimeNames::N] = [&[]; TimeNames::N];
        let mut i = 0;
        while i < TimeNames::N {
            let item = match t {
                Tab::Items(t) if i < t.len() => t[i],
                Tab::Packed(t) if i < t.len() => t[i].item(),
                _ => break,
            };
            if let Item::Bytes(b) = item {
                let mut n = 0;
                while n < b.len() && b[n] != 0 {
                    n += 1;
                }
                o[i] = b.split_at(n).0;
            }
            i += 1;
        }
        TimeNames(o)
    }

    /// The name of `LC_TIME` item `i` (`< N`), else `None`.
    #[inline]
    pub fn item(&self, i: usize) -> Option<&'static [u8]> {
        self.0.get(i).copied()
    }

    /// The abbreviated name of weekday `w` (0 = Sunday), `None` out of range.
    #[inline]
    pub fn abday(&self, w: i64) -> Option<&'static [u8]> {
        self.item(usize::try_from(w).ok().filter(|&w| w < 7)?)
    }

    /// The full name of weekday `w`.
    #[inline]
    pub fn day(&self, w: i64) -> Option<&'static [u8]> {
        self.item(7 + usize::try_from(w).ok().filter(|&w| w < 7)?)
    }

    /// The abbreviated name of month `m` (0 = January).
    #[inline]
    pub fn abmon(&self, m: i64) -> Option<&'static [u8]> {
        self.item(14 + usize::try_from(m).ok().filter(|&m| m < 12)?)
    }

    /// The full name of month `m`.
    #[inline]
    pub fn mon(&self, m: i64) -> Option<&'static [u8]> {
        self.item(26 + usize::try_from(m).ok().filter(|&m| m < 12)?)
    }

    /// `AM_STR` (`pm` false) or `PM_STR`.
    #[inline]
    pub fn ampm(&self, pm: bool) -> &'static [u8] {
        self.item(if pm { 39 } else { 38 }).unwrap_or(&[])
    }
}

/// One interned locale, pre-decoded.
///
/// `repr(C)` keeps `locale` at offset 0 (SORT-ENUS-INSTR-3): the hot calls (`strcoll` first)
/// take `&view.locale`, and at offset 0 that is the view pointer itself. With rustc's field
/// order the 312-byte `cats` went first, and `strcoll` then held `view + 0x138` in one more
/// callee-saved register across its kernel call: +5 instructions per call, +93M in sort-enUS.
#[derive(Debug)]
#[repr(C)]
pub struct LocaleView {
    locale: Locale,
    handle: usize,
    /// The wide tables of a compiled `LC_CTYPE`, and its ASCII case flag; `None` for the
    /// built-in ones.
    wide: Option<(Wide, bool)>,
    /// The class-word table of the `LC_CTYPE` (D-5.9, D-5.10), which answers every wide class
    /// test: complete for the built-in locales, resolved block by block for a compiled one.
    classes: &'static ClassTable,
    /// The `wctrans_t` of `toupper` and `tolower` in a compiled `LC_CTYPE`
    /// ([`super::ctype::CtypeFile::case_desc`]), worked out once so that `towctrans` compares instead of
    /// deriving every map's descriptor per call (D-5.8); unused (0) for the built-in ones.
    case_descs: [usize; 2],
    /// The slot in [`LATIN`] holding the answers of `wide` for U+0000..U+00FF (ASCII and
    /// Latin-1 text is most of what wide-character calls see, WCTYPE-LOCALE): 0 until the first
    /// Latin-1 call takes one, `usize::MAX` when none was left (every call then walks `wide`).
    latin: AtomicUsize,
    /// The item tables of the locale's categories, inline so that `nl_langinfo` reaches an
    /// entry through the view alone (the same slices as `locale.tables`), indexed by the C
    /// category number (`item >> 16`: `LC_ALL`'s slot 6 is empty), so that the lookup is one
    /// bounds check and no arithmetic (LV-4).
    cats: [Tab; 13],
    /// The `LC_TIME` names, ready to copy (STRFTIME-NAMES).
    names: TimeNames,
    /// The words of this locale's `struct lconv`, worked out by the first `localeconv` that
    /// asks (the same values every time, so a racing second fill stores the same words).
    lconv: LconvWords,
    /// `nl_langinfo`'s answer table ([`Answers`]): [`NO_ANSWERS`] until the view's
    /// [`ANSWERS_AFTER`]th decoded call builds it (PARITY-SMALL-ENTRY-2), then a leaked table
    /// that is never freed or changed. Never built when the view is: start-up and `setlocale`
    /// do not pay for it (INSTR-REGRESS, +4.4k instructions per start-up when it was eager).
    answers: AtomicPtr<Answers>,
    /// The calls that decoded an entry while `answers` was [`NO_ANSWERS`] (a racy count: it
    /// only paces the build).
    decoded: AtomicU32,
}

/// The empty string every empty `nl_langinfo` item answers with.
pub static EMPTY: [u8; 1] = [0];

/// `nl_langinfo`'s answer for every entry of a view's `cats`, by the same category number and
/// index (PARITY-SMALL-ENTRY): a word item's value, else the address of the item's bytes
/// ([`EMPTY`] for an empty one). The call is then one bounds check and one load instead of
/// decoding a `Tab` and an `Item`.
#[derive(Debug)]
pub struct Answers {
    tabs: [&'static [usize]; 13],
}

impl Answers {
    /// The answer for entry `index` of category number `number` ([`LocaleView::entry`] worked
    /// out ahead; `None` where `entry` is `None`, and always in [`NO_ANSWERS`]).
    #[inline]
    pub fn get(&self, number: u32, index: usize) -> Option<usize> {
        self.tabs.get(usize::try_from(number).ok()?)?.get(index).copied()
    }
}

/// The table of a view whose answers are not built: every lookup misses.
pub static NO_ANSWERS: Answers = Answers { tabs: [&[]; 13] };

/// How many decoded `nl_langinfo` calls a view answers before it builds its [`Answers`]: a
/// program that asks a few items (`CODESET` after `setlocale`) never pays for the table, one
/// that asks in a loop pays once (the build is a few hundred items, about the decode surplus
/// of a few hundred calls).
pub const ANSWERS_AFTER: u32 = 256;

/// [`Answers`]' word for one item.
fn answer(item: Item) -> usize {
    match item {
        Item::Word(v) => usize::try_from(v).unwrap_or(0),
        Item::Bytes([]) => EMPTY.as_ptr().expose_provenance(),
        Item::Bytes(b) => b.as_ptr().expose_provenance(),
    }
}

/// The address of [`NO_ANSWERS`] as the views' `answers` word holds it.
const fn no_answers_ptr() -> *mut Answers {
    core::ptr::from_ref(&NO_ANSWERS).cast_mut()
}

/// A view's `answers` word before its table is built.
const fn no_answers() -> AtomicPtr<Answers> {
    AtomicPtr::new(no_answers_ptr())
}

/// The answer tables of `cats` in one allocation, leaked with the view (views are never freed).
fn answers_of(cats: &[Tab; 13]) -> Result<[&'static [usize]; 13], Errno> {
    let total = cats.iter().fold(0usize, |t, c| t.saturating_add(c.len()));
    let mut v = TryVec::with_capacity(total)?;
    for tab in cats {
        for i in 0..tab.len() {
            v.push(tab.get(i).map_or(0, answer))?;
        }
    }
    let mut rest: &'static [usize] = v.leak();
    let mut out: [&'static [usize]; 13] = [&[]; 13];
    for (slot, tab) in out.iter_mut().zip(cats) {
        let (head, tail) = rest.split_at(tab.len().min(rest.len()));
        (*slot, rest) = (head, tail);
    }
    Ok(out)
}

/// The twelve words of a `struct lconv` (ten string pointers, two words of packed chars).
#[derive(Debug)]
struct LconvWords {
    ready: AtomicBool,
    words: [AtomicUsize; 12],
}

/// A compiled `LC_CTYPE`'s case maps and widths for U+0000..U+00FF ([`LocaleView::wtrans`],
/// [`LocaleView::width`] read them instead of walking the tables; the classes are in the
/// view's class-word table, D-5.10).
/// Each entry is worked out by the first call that asks for it, not when the view is built
/// (WL-REGRESS: filling all 256 characters eagerly cost ~194k instructions in every
/// `setlocale(LC_ALL, "")`, 2.9x the whole of a short `printf` run). An entry holds 0 until
/// filled; a racing second fill stores the same value.
#[derive(Debug)]
struct Latin {
    /// `towupper`, `towlower` of each character ([`ctype::wtrans_file`]) + 1 (wrapping; a
    /// value of `u32::MAX` stays 0 and is worked out on every call).
    map: [[AtomicU32; 256]; 2],
    /// `wcwidth` of each character (-1, or a byte of the width table) + 2.
    width: [AtomicU32; 256],
}

/// The Latin-1 caches of the views with a compiled `LC_CTYPE`, one per view, taken by the view's
/// first Latin-1 wide call (WL-REGRESS-2). They live in `.bss`, not in the view: a 4 KiB cache
/// in every view cost 3 heap pages per process (and an image page for the static `"C"` view)
/// even in programs that never ask a wide question; a page here is touched only by a process
/// that does. Views live for the rest of the process, so a slot is never handed back.
static LATIN: [Latin; LATIN_SLOTS] = [const { Latin::zero() }; LATIN_SLOTS];

/// How many views get a Latin-1 cache (later views walk the tables on every call).
const LATIN_SLOTS: usize = 8;

/// The next free slot of [`LATIN`].
static LATIN_NEXT: AtomicUsize = AtomicUsize::new(0);

impl Latin {
    const fn zero() -> Latin {
        Latin {
            map: [const { [const { AtomicU32::new(0) }; 256] }; 2],
            width: [const { AtomicU32::new(0) }; 256],
        }
    }

    /// `towupper` (`upper`) or `towlower` of `b`, filled from `w` on first use.
    #[inline]
    fn map(&self, b: u8, upper: bool, ascii: bool, w: &Wide) -> u32 {
        let Some(slot) = self.map.get(usize::from(!upper)).and_then(|m| m.get(usize::from(b))) else {
            return ctype::wtrans_file(u32::from(b), upper, ascii, w);
        };
        let v = slot.load(Relaxed);
        if v != 0 {
            return v.wrapping_sub(1);
        }
        Latin::fill_map(slot, b, upper, ascii, w)
    }

    /// The first-use fill of a `map` entry (outlined, see `fill_class`).
    #[cold]
    #[inline(never)]
    fn fill_map(slot: &AtomicU32, b: u8, upper: bool, ascii: bool, w: &Wide) -> u32 {
        let r = ctype::wtrans_file(u32::from(b), upper, ascii, w);
        slot.store(r.wrapping_add(1), Relaxed);
        r
    }

    /// `wcwidth` of `b`, filled from `w` on first use.
    #[inline]
    fn width(&self, b: u8, w: &Wide) -> i32 {
        let Some(slot) = self.width.get(usize::from(b)) else { return w.width(u32::from(b)) };
        let v = slot.load(Relaxed);
        if v != 0 {
            return i32::try_from(v).unwrap_or(1).wrapping_sub(2);
        }
        Latin::fill_width(slot, b, w)
    }

    /// The first-use fill of a `width` entry (outlined, see `fill_class`).
    #[cold]
    #[inline(never)]
    fn fill_width(slot: &AtomicU32, b: u8, w: &Wide) -> i32 {
        // -1 or one byte of the width table.
        let r = w.width(u32::from(b));
        // Outside -1..=255 (never, from a width table) the 0 stored makes every call work it out.
        slot.store(u32::try_from(r.wrapping_add(2)).unwrap_or(0), Relaxed);
        r
    }
}

impl LconvWords {
    const fn new() -> LconvWords {
        LconvWords { ready: AtomicBool::new(false), words: [const { AtomicUsize::new(0) }; 12] }
    }
}

impl LocaleView {
    /// The view of `"C"` (handle 0).
    #[allow(clippy::indexing_slicing)] // a const fn (no `get`): `Cat::Time.index()` is 2, below the 12 tables
    pub const fn c() -> LocaleView {
        LocaleView { locale: Locale::C, handle: 0, wide: None, classes: &classes::C_TABLE, case_descs: [0; 2], latin: AtomicUsize::new(0), cats: by_number(C_CATS), names: TimeNames::of(C_CATS[Cat::Time.index()]), lconv: LconvWords::new(), answers: no_answers(), decoded: AtomicU32::new(0) }
    }

    /// The words of this locale's `struct lconv`: `fill` works them out on the first call, later
    /// calls read them back (D-7.13; they point into data that lives as long as the view).
    #[inline]
    pub fn lconv(&self, fill: impl FnOnce(&Locale) -> [usize; 12]) -> [usize; 12] {
        if self.lconv.ready.load(Acquire) {
            return core::array::from_fn(|i| self.lconv.words.get(i).map_or(0, |w| w.load(Relaxed)));
        }
        let words = fill(&self.locale);
        for (slot, w) in self.lconv.words.iter().zip(words) {
            slot.store(w, Relaxed);
        }
        self.lconv.ready.store(true, Release);
        words
    }

    /// The view of `locale`, interned as `handle`. Every compiled category without an item table
    /// gets one here, built once and leaked with the view (LV-3): `reuse` returns a table an
    /// earlier view already built for the same category file, so composite locales share them.
    pub fn new(
        mut locale: Locale,
        handle: usize,
        reuse: &dyn Fn(Cat, &Data) -> Option<Tab>,
    ) -> Result<LocaleView, Errno> {
        if locale.tables.is_none() {
            let mut cats: [Tab; 12] = [Tab::EMPTY; 12];
            // The compiled categories no earlier view has a table for: built into one allocation.
            let mut build: [Option<Category>; 12] = [None; 12];
            let mut total = 0usize;
            for cat in Cat::ALL {
                let i = cat.index();
                let table = match locale.data(cat) {
                    Data::C => C_CATS.get(i).copied(),
                    Data::CUtf8 => C_UTF8_CATS.get(i).copied(),
                    Data::File(f) => match reuse(cat, &Data::File(f)) {
                        Some(t) => Some(t),
                        None => {
                            if let Some(b) = build.get_mut(i) {
                                *b = Some(f);
                                total = total.saturating_add(f.len());
                            }
                            None
                        }
                    },
                };
                if let (Some(slot), Some(t)) = (cats.get_mut(i), table) {
                    *slot = t;
                }
            }
            if total > 0 {
                let mut v = TryVec::with_capacity(total)?;
                v.resize(total, Item::Word(0))?;
                let mut rest: &'static mut [Item] = v.leak();
                for cat in Cat::ALL {
                    if let (Some(Some(f)), Some(slot)) = (build.get(cat.index()), cats.get_mut(cat.index())) {
                        let k = f.len().min(rest.len());
                        let (table, tail) = core::mem::take(&mut rest).split_at_mut(k);
                        fill_table(table, cat, f);
                        (*slot, rest) = (Tab::Items(table), tail);
                    }
                }
            }
            locale.tables = Some(TryBox::new(Tables(cats))?.leak());
        }
        let cats = by_number(locale.tables.map_or([Tab::EMPTY; 12], |t| t.0));
        let names = TimeNames::of(cats.get(2).copied().unwrap_or(Tab::EMPTY)); // LC_TIME is C category 2
        let (wide, classes, case_descs) = match locale.ctype() {
            Ctype::File(f) => (Some((f.wide(), f.ascii_case())), ClassTable::for_file(), [f.case_desc(true), f.case_desc(false)]),
            Ctype::C => (None, &classes::C_TABLE, [0; 2]),
            Ctype::CUtf8 => (None, &classes::C_UTF8_TABLE, [0; 2]),
        };
        Ok(LocaleView { locale, handle, wide, classes, case_descs, latin: AtomicUsize::new(0), cats, names, lconv: LconvWords::new(), answers: no_answers(), decoded: AtomicU32::new(0) })
    }

    /// The address of this view's [`Answers`] (Acquire: a built table is seen whole): the
    /// address of [`NO_ANSWERS`], or of a leaked table that is never freed or changed.
    #[inline]
    pub fn answers(&self) -> *const Answers {
        self.answers.load(Acquire)
    }

    /// Note one `nl_langinfo` call that decoded an entry; the [`ANSWERS_AFTER`]th builds the
    /// view's [`Answers`] and publishes it (Release). Two threads may both build: the first
    /// store wins and the other table stays unused (leaked, once). Out of memory: the count
    /// starts again and the calls go on decoding.
    #[cold]
    #[inline(never)]
    pub fn note_decoded(&self) {
        let n = self.decoded.load(Relaxed);
        if n < ANSWERS_AFTER {
            self.decoded.store(n.wrapping_add(1), Relaxed);
            return;
        }
        if !core::ptr::eq(self.answers(), &NO_ANSWERS) {
            return;
        }
        let built = answers_of(&self.cats).and_then(|tabs| TryBox::new(Answers { tabs }));
        let Ok(b) = built else {
            self.decoded.store(0, Relaxed);
            return;
        };
        let new = core::ptr::from_mut(b.leak());
        let _ = self.answers.compare_exchange(no_answers_ptr(), new, Release, Relaxed);
    }

    /// The entry `index` of the table of category number `number` (`nl_langinfo`'s fast path):
    /// `None` for `LC_ALL`, a number that is no category (negative as a `u32`) and an index past
    /// the table (`LC_ALL`'s slot is an empty table).
    #[inline]
    pub fn entry(&self, number: u32, index: usize) -> Option<Item> {
        self.cats.get(usize::try_from(number).ok()?)?.get(index)
    }

    /// [`crate::ctype::ascii_case`] of the locale, read from the view itself (one load, no walk
    /// through the locale's `LC_CTYPE`): the built-in `LC_CTYPE`s (no wide tables) map 0…127 as
    /// ASCII does; a compiled one carries its flag next to its wide tables (CASECMP3).
    #[inline]
    pub fn ascii_case(&self) -> bool {
        self.wide.as_ref().is_none_or(|w| w.1)
    }

    /// The `LC_TIME` names (`strftime`'s fast lane).
    #[inline]
    pub fn names(&self) -> &TimeNames {
        &self.names
    }

    /// The locale.
    #[inline]
    pub fn locale(&self) -> &Locale {
        &self.locale
    }

    /// This view's Latin-1 cache in [`LATIN`], taking a slot on first use; `None` when all
    /// slots are taken.
    #[inline]
    fn latin(&self) -> Option<&'static Latin> {
        match self.latin.load(Relaxed) {
            0 => self.take_latin(),
            s => LATIN.get(s.wrapping_sub(1)),
        }
    }

    /// Takes a free [`LATIN`] slot for this view (a thread that loses the race to another
    /// thread of the same view uses the winner's slot; its own stays unused).
    #[cold]
    #[inline(never)]
    fn take_latin(&self) -> Option<&'static Latin> {
        // A view without a slot stores `usize::MAX` and never comes back here, so the counter
        // grows by at most one per view (and per losing racer): it cannot wrap.
        let k = LATIN_NEXT.fetch_add(1, Relaxed);
        let want = if k < LATIN_SLOTS { k.saturating_add(1) } else { usize::MAX };
        let s = match self.latin.compare_exchange(0, want, Relaxed, Relaxed) {
            Ok(_) => want,
            Err(cur) => cur,
        };
        LATIN.get(s.wrapping_sub(1))
    }

    /// The interned handle (0: `"C"`).
    #[inline]
    pub fn handle(&self) -> usize {
        self.handle
    }

    /// Whether wide character `c` has a class of `mask` (`iswalpha`, ...): [`ctype::is_w`].
    ///
    /// Always inlined (PERF-WCTYPE-ENUS-2): with the inline hit paths below, a plain `#[inline]`
    /// body was left out of line, and every export (the built-in locales' too) paid a call:
    /// `iswalpha` in C.UTF-8 went from 68 to 76 instructions.
    #[inline(always)]
    pub fn isw(&self, c: u32, mask: Class) -> bool {
        self.isw_int(c, mask) != 0
    }

    /// The `wctrans_t` of `toupper` and `tolower` (in that order) of a compiled `LC_CTYPE`,
    /// stored when the view was built; `None` for the built-in locales, whose descriptors are
    /// the built-in tables' addresses (D-5.8).
    #[inline(always)]
    pub fn case_descs(&self) -> Option<[usize; 2]> {
        self.wide.as_ref().map(|_| self.case_descs)
    }

    /// Whether the view has a compiled locale's checked wide tables (the built-in locales do
    /// not): the arm [`LocaleView::isw_int`] answers inline.
    #[inline(always)]
    pub fn has_wide(&self) -> bool {
        self.wide.is_some()
    }

    /// [`LocaleView::isw`] as the C `int` the exports return (1 or 0). Always inlined.
    ///
    /// Every locale reads its class-word table (D-5.9, D-5.10): a resolved block is the
    /// stage-1 load, the leaf load and the mask test, inline and call-free. A block not resolved
    /// yet, a compiled locale's block without a leaf and a character above U+10FFFF take
    /// [`LocaleView::isw_wide`], which returns the `int` itself, so an export reaches it with a
    /// tail jump and keeps no frame on its inline path (TIMING-COV-WCTYPE-L).
    #[inline(always)]
    pub fn isw_int(&self, c: u32, mask: Class) -> i32 {
        if let Some(word) = self.classes.hit(c) {
            return i32::from(word & mask.0 != 0);
        }
        LocaleView::isw_wide(c, mask, self)
    }

    /// The view's class-word table (D-5.11: a thread block holds the current view's, so that
    /// the wide class tests read it with one `%fs` load).
    #[inline(always)]
    pub fn class_table(&self) -> &'static ClassTable {
        self.classes
    }

    /// The class word of `c` (the standard classes): resolves its block when needed.
    pub fn class_word(&self, c: u32) -> u16 {
        self.classes.word(c, self.wide.as_ref().map(|(w, _)| w))
    }

    /// The filled [`LATIN`] `towupper` (`upper`) or `towlower` entry of `b` for this view;
    /// `None` when the view has no slot or the entry is not filled (`wtrans_wide` takes the
    /// general path).
    #[inline]
    fn latin_map_hit(&self, b: u8, upper: bool) -> Option<u32> {
        let l = LATIN.get(self.latin.load(Relaxed).wrapping_sub(1))?;
        let v = l.map.get(usize::from(!upper))?.get(usize::from(b))?.load(Relaxed);
        (v != 0).then(|| v.wrapping_sub(1))
    }

    /// The out-of-line part of [`LocaleView::isw_int`], with its `int` answer: the block's
    /// resolution, or the walk of the file's tables.
    ///
    /// The character comes first and the view last (WCTYPE-TRIE-BUILTIN-2): an export receives
    /// `c` in the first argument register, so this order lets the tail jump keep it there.
    #[inline(never)]
    fn isw_wide(c: u32, mask: Class, v: &LocaleView) -> i32 {
        i32::from(v.class_word(c) & mask.0 != 0)
    }

    /// `towupper` (`upper`) or `towlower` of `c`: [`ctype::wtrans`]. Always inlined, as
    /// [`LocaleView::isw`] (`towupper` in C.UTF-8 went from 53 to 68 instructions as a call).
    #[inline(always)]
    pub fn wtrans(&self, c: u32, upper: bool) -> u32 {
        // The file-backed arm's common answers are inline and call-free, as in `isw`
        // (PERF-WCTYPE-ENUS): a filled Latin-1 entry, and the case table's walk above U+00FF.
        // The rest stays out of line so this body saves no registers for the built-in locales
        // (PERF-WCTYPE-UTF8-REST-3).
        if let Some((w, ascii)) = &self.wide {
            if let Ok(b) = u8::try_from(c) {
                if let Some(r) = self.latin_map_hit(b, upper) {
                    return r;
                }
            } else {
                return ctype::wtrans_file(c, upper, *ascii, w);
            }
            return self.wtrans_wide(c, upper);
        }
        ctype::wtrans(c, if upper { Trans::Upper } else { Trans::Lower }, &self.locale)
    }

    #[inline(never)]
    fn wtrans_wide(&self, c: u32, upper: bool) -> u32 {
        match &self.wide {
            Some((w, ascii)) => match u8::try_from(c) {
                Ok(b) => match self.latin() {
                    Some(l) => l.map(b, upper, *ascii, w),
                    None => ctype::wtrans_file(c, upper, *ascii, w),
                },
                Err(_) => ctype::wtrans_file(c, upper, *ascii, w),
            },
            None => ctype::wtrans(c, if upper { Trans::Upper } else { Trans::Lower }, &self.locale),
        }
    }

    /// `wcwidth` of `c`: [`crate::text::display_width`].
    #[inline]
    pub fn width(&self, c: u32) -> i32 {
        // The file-backed arm is out of line (PERF-WCTYPE-UTF8-REST-3, as `wtrans`).
        if self.wide.is_some() {
            return self.width_wide(c);
        }
        crate::text::display_width(c, &self.locale)
    }

    #[inline(never)]
    fn width_wide(&self, c: u32) -> i32 {
        match &self.wide {
            Some((w, _)) => match u8::try_from(c) {
                Ok(b) => match self.latin() {
                    Some(l) => l.width(b, w),
                    None => w.width(c),
                },
                Err(_) => w.width(c),
            },
            None => crate::text::display_width(c, &self.locale),
        }
    }
}

#[cfg(test)]
mod class_word_tests {
    use super::{Locale, LocaleView};
    use crate::ctype::{self, Class, STANDARD_MASKS};
    use crate::unicode::{self, class_words};

    /// Spec §5.2 (s5_2_g_0: the classes follow the locale's definition; s5_2_c_0: `isw*` and
    /// `iswctype` classify by them), D-5.9: the built-in locales' two-stage class-word tables
    /// equal the per-call derivation (`ctype::wclasses`: the C rule and the `C.UTF-8` Unicode
    /// properties) for every code point through U+10FFFF, 0x110000 and the edge values, as a
    /// whole word and through the view's `isw` for each of the 12 standard classes.
    #[test]
    fn builtin_class_tables_equal_wclasses() {
        let views = [(Locale::C, &class_words::C_STAGE1, "C"), (Locale::c_utf8(b"C.UTF-8"), &class_words::C_UTF8_STAGE1, "C.UTF-8")];
        let mut members = [0usize; 12];
        for (l, s1, what) in views {
            let built = LocaleView::new(l, 3, &|_, _| None);
            assert!(built.is_ok(), "{what}: the view is built");
            let Ok(v) = built else { return };
            for c in (0..=0x11_0000u32).chain([0x7fff_ffff, 0xffff_fffe, 0xffff_ffff]) {
                let want = ctype::wclasses(c, &l);
                assert_eq!(unicode::class_word(s1, c), want.0, "{what}: class word {c:#x}");
                for (k, &m) in STANDARD_MASKS.iter().enumerate() {
                    let has = want.intersects(Class(m));
                    assert_eq!(v.isw(c, Class(m)), has, "{what}: isw {c:#x} class {k}");
                    if let Some(n) = members.get_mut(k) {
                        *n += usize::from(has);
                    }
                }
            }
        }
        // Not vacuous: every class has members, and C.UTF-8's alphabetic ones are beyond ASCII.
        assert!(members.iter().all(|&n| n > 0), "{members:?}");
        assert!(members.get(2).is_some_and(|&n| n > 100_000), "{members:?}");
    }
}

#[cfg(test)]
mod answer_tests {
    use super::{answer, answers_of, Locale, LocaleView, ANSWERS_AFTER, NO_ANSWERS};

    /// no spec clause (the `nl_langinfo` answers of §7, PARITY-SMALL-ENTRY): the answer table
    /// built for a view equals the `cats` decode for every category number and index, past the
    /// end of each table included; [`NO_ANSWERS`] answers nothing.
    #[test]
    fn answer_table_matches_entries() {
        let built = LocaleView::new(Locale::C, 0, &|_, _| None);
        assert!(built.is_ok(), "the view of \"C\" is built");
        let Ok(v) = built else { return };
        let tabs = answers_of(&v.cats);
        assert!(tabs.is_ok(), "the answer table is built");
        let Ok(tabs) = tabs else { return };
        let table = super::Answers { tabs };
        let mut seen = 0usize;
        for number in 0..15u32 {
            for index in 0..1100usize {
                let want = v.entry(number, index).map(answer);
                assert_eq!(table.get(number, index), want, "category {number} item {index}");
                assert_eq!(NO_ANSWERS.get(number, index), None, "no table: category {number} item {index}");
                seen += usize::from(want.is_some());
            }
        }
        assert!(seen > 100, "the C view answers its items ({seen})");
    }

    /// no spec clause (PARITY-SMALL-ENTRY-2: no eager work when a view is built): a new view,
    /// the const `"C"` view included, has no table; its first [`ANSWERS_AFTER`] decoded calls
    /// build none, the next one publishes one, and later calls keep it.
    #[test]
    fn answer_table_is_built_late() {
        let built = LocaleView::new(Locale::C, 0, &|_, _| None);
        assert!(built.is_ok(), "the view of \"C\" is built");
        let Ok(v) = built else { return };
        assert!(core::ptr::eq(LocaleView::c().answers(), &NO_ANSWERS), "the const view starts without a table");
        assert!(core::ptr::eq(v.answers(), &NO_ANSWERS), "a new view starts without a table");
        for n in 0..ANSWERS_AFTER {
            v.note_decoded();
            assert!(core::ptr::eq(v.answers(), &NO_ANSWERS), "no table after {} decoded calls", n + 1);
        }
        v.note_decoded();
        let first = v.answers();
        assert!(!core::ptr::eq(first, &NO_ANSWERS), "the table is built after {ANSWERS_AFTER} decoded calls");
        v.note_decoded();
        assert!(core::ptr::eq(v.answers(), first), "a later call keeps the published table");
    }
}
