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
use super::{Tables, C_CATS, C_UTF8_CATS};
use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering::{Acquire, Relaxed, Release}};

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
}

/// The twelve words of a `struct lconv` (ten string pointers, two words of packed chars).
#[derive(Debug)]
struct LconvWords {
    ready: AtomicBool,
    words: [AtomicUsize; 12],
}

/// A compiled `LC_CTYPE`'s wide answers for U+0000..U+00FF ([`LocaleView::isw`],
/// [`LocaleView::wtrans`], [`LocaleView::width`] read them instead of walking the tables).
/// Each entry is worked out by the first call that asks for it, not when the view is built
/// (WL-REGRESS: filling all 256 characters eagerly cost ~194k instructions in every
/// `setlocale(LC_ALL, "")`, 2.9x the whole of a short `printf` run). An entry holds 0 until
/// filled; a racing second fill stores the same value.
#[derive(Debug)]
struct Latin {
    /// The standard classes of each character ([`ctype::w_classes_file`]) | `FILLED`.
    class: [AtomicU32; 256],
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

/// The marker bit of a filled `Latin::class` entry (classes are 16 bits).
const FILLED: u32 = 1 << 16;

impl Latin {
    const fn zero() -> Latin {
        Latin {
            class: [const { AtomicU32::new(0) }; 256],
            map: [const { [const { AtomicU32::new(0) }; 256] }; 2],
            width: [const { AtomicU32::new(0) }; 256],
        }
    }

    /// The classes of `b`, filled from `w` on first use.
    #[inline]
    fn class(&self, b: u8, w: &Wide) -> u32 {
        let Some(slot) = self.class.get(usize::from(b)) else { return 0 };
        let v = slot.load(Relaxed);
        if v != 0 {
            return v;
        }
        Latin::fill_class(slot, b, w)
    }

    /// The first-use fill of a `class` entry, outlined so the hit path keeps no fill registers.
    #[cold]
    #[inline(never)]
    fn fill_class(slot: &AtomicU32, b: u8, w: &Wide) -> u32 {
        let v = u32::from(ctype::w_classes_file(u32::from(b), w).0) | FILLED;
        slot.store(v, Relaxed);
        v
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
        LocaleView { locale: Locale::C, handle: 0, wide: None, latin: AtomicUsize::new(0), cats: by_number(C_CATS), names: TimeNames::of(C_CATS[Cat::Time.index()]), lconv: LconvWords::new() }
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
        let wide = match locale.ctype() {
            Ctype::File(f) => Some((f.wide(), f.ascii_case())),
            Ctype::C | Ctype::CUtf8 => None,
        };
        Ok(LocaleView { locale, handle, wide, latin: AtomicUsize::new(0), cats, names, lconv: LconvWords::new() })
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
    #[inline]
    pub fn isw(&self, c: u32, mask: Class) -> bool {
        // One compare in front of the table walk (WCTYPE-LOCALE-3: a tuple match here kept the
        // walk from being inlined and made the non-Latin path longer).
        match &self.wide {
            Some((w, _)) => match u8::try_from(c) {
                Ok(b) => match self.latin() {
                    Some(l) => l.class(b, w) & u32::from(mask.0) != 0,
                    None => ctype::is_w_file(c, mask, w),
                },
                Err(_) => ctype::is_w_file(c, mask, w),
            },
            None => ctype::wclasses(c, &self.locale).intersects(mask),
        }
    }

    /// `towupper` (`upper`) or `towlower` of `c`: [`ctype::wtrans`].
    #[inline]
    pub fn wtrans(&self, c: u32, upper: bool) -> u32 {
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
