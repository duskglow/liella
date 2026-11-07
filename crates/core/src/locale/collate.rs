//! Collation over a compiled `LC_COLLATE` (spec Appendix D.1.4, D-4.18).
//!
//! A string becomes a sequence of *elements* (one or more characters, contractions first);
//! each element has weights per level and a *ruleset* giving each level's rule. The key is the
//! levels in turn, each the elements' weights (backward runs reversed, position separators
//! counting skipped elements) ended by a separator; the key's final separator is dropped.
//! [`key`] yields the key's units lazily, so `strcoll` compares two keys without building
//! them, and `strxfrm` writes the same units.
#![allow(clippy::arithmetic_side_effects)] // offsets are u32 file values or slice lengths: sums of a few stay far below usize::MAX

use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU8, AtomicUsize, Ordering};

use super::file::{i32_at, u32_at, Category};
use super::table3::Table3;
use crate::mem::InlineVec;

const NRULES: usize = 0;
const RULESETS: usize = 1;
const TABLEMB: usize = 2;
const WEIGHTMB: usize = 3;
const EXTRAMB: usize = 4;
const INDIRECTMB: usize = 5;
const TABLEWC: usize = 9;
const WEIGHTWC: usize = 10;
const EXTRAWC: usize = 11;
const INDIRECTWC: usize = 12;
const COLLSEQMB: usize = 16;
const COLLSEQWC: usize = 17;

/// The `LC_COLLATE` magic word.
pub const MAGIC: u32 = 0x2005_1017;

const BACKWARD: u8 = 2;
const POSITION: u8 = 4;
/// The most levels a key can have (the rules are bytes per level; tables use 4).
const MAX_LEVELS: usize = 16;

/// [`Collate::ascii1`]: the byte's element needs the general path (a contraction may start
/// with it, or its first-level rule is backward or by position).
const ASCII_SLOW: u32 = u32::MAX;

/// How many catalogs keep a precomputed ASCII table (a full pool leaves later ones on the general
/// path; locales loaded in one process are few).
const ASCII_SLOTS: usize = 8;
static ASCII_KEYS: [AtomicUsize; ASCII_SLOTS] = [const { AtomicUsize::new(0) }; ASCII_SLOTS];
static ASCII_POOL: [AsciiSlot; ASCII_SLOTS] = [const {
    AsciiSlot {
        lane: [const { AtomicU32::new(ASCII_SLOW) }; 128],
        unit: [const { AtomicU32::new(UNIT_SLOW) }; 256],
        ascii_fast: AtomicBool::new(false),
        rank: [const { AtomicU16::new(0) }; 256],
        ready: AtomicBool::new(false),
        lead_map: [const { AtomicU8::new(0) }; 256],
        lead_next: AtomicUsize::new(0),
        lead: [const { LeadIndex { at: [const { AtomicU16::new(LEAD_NONE) }; 257] } }; LEAD_SLOTS],
    }
}; ASCII_SLOTS];

/// Lead bytes per catalog whose `extramb` list gets an index (PERF-STRCOLL-ELEMENTS, D-4.51).
const LEAD_SLOTS: usize = 16;
/// The shortest list worth an index: a shorter one is walked.
const LEAD_MIN: usize = 8;
/// [`AsciiSlot::lead_map`]: the list is walked from its start (short, malformed, or no slot).
const LEAD_WALK: u8 = 0xff;
/// [`LeadIndex::at`]: no entry.
const LEAD_NONE: u16 = u16::MAX;

/// Where to start the walk of one lead byte's `extramb` list, by the byte after the lead.
#[derive(Debug)]
struct LeadIndex {
    /// Per next byte (index 256: the string ends after the lead), the byte offset from the
    /// list's start of the first entry that can match a rest starting so: a plain entry whose
    /// sequence starts with that byte, a range whose low and high ends' first bytes bracket it,
    /// or the always-matching entry (length 0). Every entry before it cannot match, so the
    /// walk from there finds the same element as the walk from the start.
    at: [AtomicU16; 257],
}

/// One catalog's precomputed tables (STRCOLL-1 D-4.30, STRCOLL-CTEXT D-4.37).
#[derive(Debug)]
pub struct AsciiSlot {
    /// Per ASCII byte: `start << 8 | count` of its first-level weights in `weightmb`, or
    /// [`ASCII_SLOW`].
    lane: [AtomicU32; 128],
    /// Per byte, for [`compare_stream`]: its element's first-level units, at most three,
    /// left-aligned in bits 8..32 with their count in bits 0..8 (1..=3); or [`UNIT_IGNORE`] (no
    /// first-level unit), [`UNIT_END`] (the terminator) or [`UNIT_SLOW`] (the general path: more
    /// units, a contraction, a backward or positional rule, a non-ASCII byte, one level only).
    /// [`UNIT_LOOK`] marks a byte whose contractions all continue with a non-ASCII byte
    /// (STRCOLL-2, D-4.37): its units hold when the next byte is ASCII.
    unit: [AtomicU32; 256],
    /// No ASCII byte's `unit` is [`UNIT_SLOW`] (set once the table is filled): a shared prefix
    /// then needs the general path only if it holds a non-ASCII byte.
    ascii_fast: AtomicBool,
    /// Per byte, for [`decide_first_flag`] (STRCOLL-4, D-4.37): `0x100 | u` where the byte's
    /// `unit` entry is plain (one to [`UNIT_MAX`] units, no flag) with first unit `u` and
    /// `ascii_fast` holds; 0 (no decision at this byte) otherwise. Written after `ascii_fast`,
    /// so a reader meets 0 or the final value: both correct.
    rank: [AtomicU16; 256],
    /// The tables above are filled (STRCOLL-START, D-4.37): they are filled at the first
    /// comparison that needs them ([`Collate::warm`]), not when the catalog is loaded, so a
    /// process that never collates (`setlocale(LC_ALL, "")` at every program start) pays
    /// only the slot claim.
    ready: AtomicBool,
    /// Per lead byte with an `extramb` list (PERF-STRCOLL-ELEMENTS, D-4.51): 0 = not looked at
    /// yet, `k + 1` = indexed in `lead[k]`, [`LEAD_WALK`] = walked from the start. Built at the
    /// first element of that lead byte, published after its index (Release).
    lead_map: [AtomicU8; 256],
    /// The next free `lead` slot.
    lead_next: AtomicUsize,
    lead: [LeadIndex; LEAD_SLOTS],
}

/// [`AsciiSlot::unit`]: a one-byte element without first-level units.
const UNIT_IGNORE: u32 = 0;
/// [`AsciiSlot::unit`]: the string's terminator.
const UNIT_END: u32 = 0xfe;
/// [`AsciiSlot::unit`]: the general path.
const UNIT_SLOW: u32 = 0xff;
/// [`AsciiSlot::unit`] flag (with a count of 1..=3 below it): the units hold only when the next
/// byte is ASCII (below 0x80); otherwise the general path. The special values above have bits
/// 6 and 7 both set, so `v & 0xc0 == UNIT_LOOK` tells the flag apart.
const UNIT_LOOK: u32 = 0x40;
/// The units of a catalog without a pool slot: every byte takes the general path.
static NO_UNITS: [AtomicU32; 256] = [const { AtomicU32::new(UNIT_SLOW) }; 256];
/// The most first-level units an [`AsciiSlot::unit`] entry holds.
const UNIT_MAX: u32 = 3;

/// What a compiled `LC_COLLATE` holds.
#[derive(Clone, Copy, Debug)]
#[allow(clippy::large_enum_variant)] // the ASCII table lives in a pool (STRCMP-FIXED)
pub enum Collation {
    /// No rules: strings order by code point (bytes in the multibyte form).
    CodePoint,
    /// Multi-level rules.
    Rules(Collate),
}

/// A compiled `LC_COLLATE` with at least one rule.
#[derive(Clone, Copy, Debug)]
pub struct Collate {
    nrules: usize,
    rulesets: &'static [u8],
    tablemb: &'static [u8],
    weightmb: &'static [u8],
    extramb: &'static [u8],
    indirectmb: &'static [u8],
    tablewc: Option<Table3>,
    weightwc: &'static [u8],
    extrawc: &'static [u8],
    indirectwc: &'static [u8],
    /// The first-level weights of each ASCII byte as `first_level` would find them, precomputed
    /// once per catalog in [`ASCII_POOL`] (STRCOLL-1, D-4.30): `start << 8 | count` in `weightmb`,
    /// or [`ASCII_SLOW`]; `None` (every byte takes the general path) when the pool is full. A
    /// reference, not an inline table, so that copying a `Locale` (every regcomp does) costs 8
    /// bytes instead of 512 (STRCMP-FIXED).
    ascii1: Option<&'static AsciiSlot>,
    /// The category, for the items read rarely: `_NL_COLLATE_COLLSEQMB` (the collation-sequence
    /// value of each byte, `u8[256]`) and `_NL_COLLATE_COLLSEQWC` (that of each code point, a
    /// value table of `u32`, Appendix D.1.2).
    cat: Category,
}

impl Collate {
    /// Parse the category; `None` if it is malformed.
    pub fn parse(cat: Category) -> Option<Collation> {
        if cat.magic() != MAGIC || cat.len() <= INDIRECTWC {
            return None;
        }
        let nrules = usize::try_from(cat.word(NRULES)).ok()?;
        if nrules == 0 {
            return Some(Collation::CodePoint);
        }
        if nrules > MAX_LEVELS || cat.item(TABLEMB).len() < 1024 {
            return None;
        }
        let mut c = Collate {
            nrules,
            rulesets: cat.item(RULESETS),
            tablemb: cat.item(TABLEMB),
            weightmb: cat.item(WEIGHTMB),
            extramb: cat.item(EXTRAMB),
            indirectmb: cat.item(INDIRECTMB),
            tablewc: Table3::parse(cat.item(TABLEWC)),
            weightwc: cat.item(WEIGHTWC),
            extrawc: cat.item(EXTRAWC),
            indirectwc: cat.item(INDIRECTWC),
            ascii1: None,
            cat,
        };
        c.ascii1 = c.ascii_slot();
        Some(Collation::Rules(c))
    }

    /// How many ASCII bytes take the precomputed first level (D-4.30; for tests).
    #[doc(hidden)]
    pub fn ascii_lane_bytes(&self) -> usize {
        self.warm();
        self.ascii1.map_or(0, |t| t.lane.iter().filter(|u| u.load(Ordering::Relaxed) != ASCII_SLOW).count())
    }

    /// The pool slot of this catalog's ASCII table, claimed at load and filled at the first
    /// general-path comparison ([`Self::warm`]). Entries start as [`ASCII_SLOW`] / [`UNIT_SLOW`] / rank 0 and are
    /// written with the same value by every filler, so a reader that meets a table not yet filled,
    /// or still being filled, sees, per byte, either the right value or the general path: both
    /// correct. The slot is keyed by the address of the catalog's `tablemb` (catalogs are mapped
    /// for the life of the process, so the address names one catalog).
    fn ascii_slot(&self) -> Option<&'static AsciiSlot> {
        let key = self.tablemb.as_ptr().addr();
        let slot = ASCII_KEYS.iter().position(|k| {
            match k.compare_exchange(0, key, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => true,
                Err(have) => have == key,
            }
        })?;
        ASCII_POOL.get(slot)
    }

    /// Fill this catalog's ASCII table if it is not filled yet (one load when it is). Called only
    /// on the general path, at the entry of [`first_level`]: the fast readers ([`decide_first_flag`],
    /// [`compare_stream`]) stay check-free and, until the table is filled, decide nothing (rank 0,
    /// every unit SLOW), so the first `strcoll` of a differing pair reaches the general path,
    /// which fills; later calls take the fast paths.
    #[inline]
    fn warm(&self) {
        if let Some(table) = self.ascii1 {
            if !table.ready.load(Ordering::Acquire) {
                self.fill(table);
            }
        }
    }

    #[cold]
    #[inline(never)]
    fn fill(&self, table: &AsciiSlot) {
        for (b, u) in table.lane.iter().enumerate() {
            let e = self.ascii_entry(b);
            u.store(e, Ordering::Relaxed);
            if let Some(w) = table.unit.get(b) {
                w.store(self.unit_entry(b, e), Ordering::Relaxed);
            }
        }
        // Every filler stores the same entries before this, so each sees the final ones.
        let fast = table.unit.iter().take(128).all(|u| u.load(Ordering::Relaxed) != UNIT_SLOW);
        table.ascii_fast.store(fast, Ordering::Relaxed);
        if fast {
            for (r, u) in table.rank.iter().zip(&table.unit) {
                let u = u.load(Ordering::Relaxed);
                if plain_unit(u) {
                    // The first unit is the top byte (left-aligned), so this fits u16.
                    r.store(u16::try_from(0x100 | u >> 24).unwrap_or(0), Ordering::Relaxed);
                }
            }
        }
        table.ready.store(true, Ordering::Release);
    }

    /// The first level of ASCII byte `b` from the same lookups [`FirstLevel`] makes per element:
    /// a byte whose `tablemb` entry is direct, or a list that matches at its first entry (no
    /// contraction), is a one-byte element; its first-level rule must be forward without
    /// positions; its first-level weights are kept as found. [`ASCII_SLOW`] otherwise.
    fn ascii_entry(&self, b: usize) -> u32 {
        let Some(mut e) = i32_at(self.tablemb, 4 * b) else { return ASCII_SLOW };
        if e < 0 {
            // A list whose first entry is a plain one of length 0 matches at once, whatever
            // follows (as `element` reads it); any other list may hold a contraction.
            let p = usize::try_from(e.unsigned_abs()).unwrap_or(usize::MAX);
            match (i32_at(self.extramb, p), self.extramb.get(p.saturating_add(4))) {
                (Some(idx), Some(0)) if idx >= 0 => e = idx,
                _ => return ASCII_SLOW,
            }
        }
        self.direct_entry(e)
    }

    /// [`Self::ascii_entry`] for the one-byte element at table index `e` (not a list).
    fn direct_entry(&self, e: i32) -> u32 {
        let (ruleset, at) = entry(e);
        if self.rule(ruleset, 0) & (BACKWARD | POSITION) != 0 {
            return ASCII_SLOW;
        }
        let w = <&[u8] as Text>::weights(self, at, 0);
        if let (Ok(start), Ok(count)) = (u32::try_from(w.start), u8::try_from(w.count)) {
            if start < 1 << 24 {
                return start << 8 | u32::from(count);
            }
        }
        ASCII_SLOW
    }

    /// The [`AsciiSlot::unit`] entry of ASCII byte `b` whose lane entry is `e`: only with more
    /// than one level (the separator rule of [`compare_stream`] needs one), and only for a
    /// byte with at most one first-level unit.
    fn unit_entry(&self, b: usize, e: u32) -> u32 {
        if self.nrules < 2 {
            return UNIT_SLOW;
        }
        if b == 0 {
            return UNIT_END;
        }
        if e == ASCII_SLOW {
            // A contraction may start here: when every one continues with a non-ASCII byte, an
            // ASCII next byte leaves the byte alone (its list's always-matching entry), so its
            // units hold after a look at the next byte (D-4.37).
            let Some(idx) = self.look_index(b) else { return UNIT_SLOW };
            let u = self.units(self.direct_entry(idx));
            return if u == UNIT_SLOW || u == UNIT_IGNORE { UNIT_SLOW } else { u | UNIT_LOOK };
        }
        self.units(e)
    }

    /// The table index of byte `b`'s one-byte element when `b` starts a list whose every entry
    /// before the always-matching one (a plain entry of length 0) needs a first following byte
    /// of 0x80 or more (a plain sequence starting there, or a range whose low end does: a
    /// lower byte orders below it); `None` otherwise (also for a direct entry or a malformed list).
    fn look_index(&self, b: usize) -> Option<i32> {
        let e = i32_at(self.tablemb, 4 * b)?;
        if e >= 0 {
            return None;
        }
        let mut p = usize::try_from(e.unsigned_abs()).ok()?;
        loop {
            let idx = i32_at(self.extramb, p)?;
            let len = usize::from(*self.extramb.get(p.checked_add(4)?)?);
            let body = p.checked_add(5)?;
            if len == 0 {
                return if idx >= 0 { Some(idx) } else { None };
            }
            if *self.extramb.get(body)? < 0x80 {
                return None;
            }
            p = body.checked_add(if idx >= 0 { len } else { 2 * len })?;
            p = p.checked_add(3)? & !3;
        }
    }

    /// Where the walk of lead byte `lead`'s list (starting at `start` in `extramb`) begins for
    /// a rest whose first byte is `next` (`None`: the string ends): [`LeadIndex::at`], or
    /// `start` without an index.
    #[inline]
    fn list_from(&self, slot: &AsciiSlot, lead: u8, start: usize, next: Option<&u8>) -> usize {
        let m = slot.lead_map.get(usize::from(lead)).map_or(LEAD_WALK, |m| m.load(Ordering::Acquire));
        let m = if m == 0 { self.index_list(slot, lead, start) } else { m };
        let Some(ix) = usize::from(m).checked_sub(1).and_then(|k| slot.lead.get(k)) else { return start };
        let r = ix.at.get(next.map_or(256, |b| usize::from(*b))).map_or(LEAD_NONE, |v| v.load(Ordering::Relaxed));
        if r == LEAD_NONE { start } else { start + usize::from(r) }
    }

    /// Build lead byte `lead`'s [`LeadIndex`] from its list at `start` and publish it; returns
    /// the [`AsciiSlot::lead_map`] value. Two threads may both build one (each its own slot):
    /// both indexes are the same and either serves.
    #[cold]
    #[inline(never)]
    fn index_list(&self, slot: &AsciiSlot, lead: u8, start: usize) -> u8 {
        let mut at = [LEAD_NONE; 257];
        let (mut p, mut n, mut closed) = (start, 0usize, false);
        while let (Some(idx), Some(&len)) = (i32_at(self.extramb, p), self.extramb.get(p + 4)) {
            let Ok(rel) = u16::try_from(p - start) else { break };
            if rel == LEAD_NONE {
                break;
            }
            n += 1;
            let len = usize::from(len);
            let body = p + 5;
            if len == 0 {
                for v in at.iter_mut().filter(|v| **v == LEAD_NONE) {
                    *v = rel;
                }
                closed = true;
                break;
            }
            let (lo, hi) = if idx >= 0 {
                p = body + len;
                (self.extramb.get(body), self.extramb.get(body))
            } else {
                p = body + 2 * len;
                (self.extramb.get(body), self.extramb.get(body + len))
            };
            let (Some(&lo), Some(&hi)) = (lo, hi) else { break };
            for v in at.iter_mut().take(usize::from(hi) + 1).skip(usize::from(lo)).filter(|v| **v == LEAD_NONE) {
                *v = rel;
            }
            p = (p + 3) & !3;
        }
        let k = if closed && n >= LEAD_MIN { slot.lead_next.fetch_add(1, Ordering::Relaxed) } else { LEAD_SLOTS };
        let m = match (slot.lead.get(k), u8::try_from(k + 1)) {
            (Some(ix), Ok(m)) => {
                for (d, v) in ix.at.iter().zip(at) {
                    d.store(v, Ordering::Relaxed);
                }
                m
            }
            _ => LEAD_WALK,
        };
        if let Some(e) = slot.lead_map.get(usize::from(lead)) {
            e.store(m, Ordering::Release);
        }
        m
    }

    /// The [`AsciiSlot::unit`] entry of a lane entry `e` (`start << 8 | count`, or
    /// [`ASCII_SLOW`]).
    fn units(&self, e: u32) -> u32 {
        let count = e & 0xff;
        if e == ASCII_SLOW || count > UNIT_MAX {
            return UNIT_SLOW;
        }
        let start = usize::try_from(e >> 8).unwrap_or(usize::MAX);
        let mut v = 0;
        for k in 0..count {
            let Some(w) = self.weightmb.get(start.saturating_add(usize::try_from(k).unwrap_or(usize::MAX))) else { return UNIT_SLOW };
            v |= u32::from(*w) << (24 - 8 * k);
        }
        v | count
    }

    /// The collation-sequence value of byte `b` (the order of bracket-expression ranges in a
    /// single-byte locale; spec Appendix D.1.4); `None` when the table does not cover it.
    pub fn collseq_byte(&self, b: u8) -> Option<u32> {
        self.cat.item(COLLSEQMB).get(usize::from(b)).map(|v| u32::from(*v))
    }

    /// The collation-sequence value of code point `wc` (bracket-expression ranges in a
    /// multibyte locale); `None` when the table has no entry for it.
    pub fn collseq_wide(&self, wc: u32) -> Option<u32> {
        Table3::parse(self.cat.item(COLLSEQWC)).and_then(|t| t.value_u32(wc))
    }

    fn rule(&self, ruleset: usize, level: usize) -> u8 {
        ruleset.checked_mul(self.nrules).and_then(|i| i.checked_add(level)).and_then(|i| self.rulesets.get(i)).copied().unwrap_or(1)
    }

    /// The rule at `level` shared by every ruleset in `mask` (bit `r` = ruleset `r`); `None` if
    /// two of them differ (or the mask is empty).
    #[inline(always)]
    fn common_rule(&self, mask: u64, level: usize) -> Option<u8> {
        let mut m = mask;
        let first = self.rule(usize::try_from(m.trailing_zeros()).ok()?, level);
        m &= m.wrapping_sub(1);
        while m != 0 {
            if self.rule(usize::try_from(m.trailing_zeros()).ok()?, level) != first {
                return None;
            }
            m &= m.wrapping_sub(1);
        }
        (mask != 0).then_some(first)
    }
}

/// One element: where it starts and how long it is (in units of the string), its ruleset,
/// and where its weights start.
#[derive(Clone, Copy, Debug, Default)]
struct Element {
    len: usize,
    ruleset: usize,
    weights: usize,
}

fn entry(e: i32) -> (usize, usize) {
    let u = u32::from_le_bytes(e.to_le_bytes());
    (usize::try_from(u >> 24).unwrap_or(0), usize::try_from(u & 0x00ff_ffff).unwrap_or(0))
}

/// A string the engine can collate: bytes (multibyte tables) or code points (wide tables).
pub trait Text: Copy {
    /// The number of units.
    fn len(&self) -> usize;
    /// Whether there are no units.
    fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// The element at `pos`.
    #[doc(hidden)]
    fn element(&self, c: &Collate, pos: usize) -> ElementRef;
    /// The weight units of the element whose weights start at `at`, at `level`.
    #[doc(hidden)]
    fn weights(c: &Collate, at: usize, level: usize) -> Weights;
    /// The weight units of the level that starts at `at` (`weights(c, at, 0)`), and where the
    /// next level starts (`weights(c, at, 1)` is `weights(c, next, 0)`): a walk over the levels
    /// in order advances one cursor instead of counting from level 0 each time.
    #[doc(hidden)]
    fn step(c: &Collate, at: usize) -> (Weights, usize);
    /// Whether the two strings hold the same units.
    #[doc(hidden)]
    fn same(&self, other: &Self) -> bool;
    /// The precomputed first level of the unit at `pos` ([`Collate::ascii1`]), or
    /// [`ASCII_SLOW`] (the default: no shortcut for this text).
    #[doc(hidden)]
    fn ascii1(&self, _c: &Collate, _pos: usize) -> u32 {
        ASCII_SLOW
    }
}

/// An element, opaque outside this module.
#[derive(Clone, Copy, Debug, Default)]
pub struct ElementRef(Element);

/// The weight units of one element at one level.
#[derive(Clone, Copy, Debug)]
pub struct Weights {
    data: &'static [u8],
    start: usize,
    count: usize,
    wide: bool,
}

impl Weights {
    fn get(&self, i: usize) -> u32 {
        if self.wide {
            u32_at(self.data, 4 * (self.start + i)).unwrap_or(0)
        } else {
            self.data.get(self.start + i).copied().map_or(0, u32::from)
        }
    }
}

/// Mixed-radix offset of `cand` within `[lo, hi]` per unit (Appendix D.1.4): `None` unless
/// `lo <= cand <= hi` as strings. The offset is signed arithmetic over the units, as the
/// reference model computes it.
fn range_offset(lo: impl Fn(usize) -> u32, hi: impl Fn(usize) -> u32, cand: impl Fn(usize) -> u32, len: usize) -> Option<i64> {
    let lo_seq = (0..len).map(&lo);
    let hi_seq = (0..len).map(&hi);
    let cand_seq = || (0..len).map(&cand);
    if cand_seq().lt(lo_seq) || cand_seq().gt(hi_seq) {
        return None;
    }
    let mut off: i64 = 0;
    for j in 0..len {
        let span = i64::from(hi(j)) - i64::from(lo(j)) + 1;
        off = off.checked_mul(span)?.checked_add(i64::from(cand(j)) - i64::from(lo(j)))?;
    }
    Some(off)
}

/// The indirect entry `base + off` (`base` = −i of a range entry).
fn indirect(table: &[u8], idx: i32, off: i64) -> i32 {
    let i = i64::from(idx.unsigned_abs()).checked_add(off).and_then(|i| usize::try_from(i).ok());
    i.and_then(|i| i.checked_mul(4)).and_then(|at| i32_at(table, at)).unwrap_or(0)
}

impl Text for &[u8] {
    fn len(&self) -> usize {
        <[u8]>::len(self)
    }

    fn element(&self, c: &Collate, pos: usize) -> ElementRef {
        let first = self.get(pos).copied().unwrap_or(0);
        let t = i32_at(c.tablemb, 4 * usize::from(first)).unwrap_or(0);
        if t >= 0 {
            let (ruleset, weights) = entry(t);
            return ElementRef(Element { len: 1, ruleset, weights });
        }
        let rest = self.get(pos + 1..).unwrap_or(&[]);
        let mut p = usize::try_from(t.unsigned_abs()).unwrap_or(usize::MAX);
        if let Some(slot) = c.ascii1 {
            p = c.list_from(slot, first, p, rest.first());
        }
        // Each list ends with an entry that always matches; a malformed list ends at the
        // table's end, and the element is then one byte without weights.
        while let (Some(idx), Some(&len)) = (i32_at(c.extramb, p), c.extramb.get(p + 4)) {
            let len = usize::from(len);
            let body = p + 5;
            if idx >= 0 {
                let seq = c.extramb.get(body..body + len).unwrap_or(&[0xff; 0]);
                if seq.len() == len && rest.starts_with(seq) {
                    let (ruleset, weights) = entry(idx);
                    return ElementRef(Element { len: 1 + len, ruleset, weights });
                }
                p = body + len;
            } else {
                let at = |base: usize| move |j: usize| c.extramb.get(base + j).copied().map_or(0, u32::from);
                if rest.len() >= len {
                    let cand = |j: usize| rest.get(j).copied().map_or(0, u32::from);
                    if let Some(off) = range_offset(at(body), at(body + len), cand, len) {
                        let v = indirect(c.indirectmb, idx, off);
                        let (ruleset, weights) = entry(v);
                        return ElementRef(Element { len: 1 + len, ruleset, weights });
                    }
                }
                p = body + 2 * len;
            }
            p = (p + 3) & !3;
        }
        ElementRef(Element { len: 1, ruleset: 0, weights: usize::MAX })
    }

    fn same(&self, other: &Self) -> bool {
        self == other
    }

    #[inline]
    fn ascii1(&self, c: &Collate, pos: usize) -> u32 {
        match self.get(pos) {
            Some(&b) => c.ascii1.and_then(|t| t.lane.get(usize::from(b))).map_or(ASCII_SLOW, |u| u.load(Ordering::Relaxed)),
            _ => ASCII_SLOW,
        }
    }

    fn weights(c: &Collate, at: usize, level: usize) -> Weights {
        let mut at = at;
        for _ in 0..level {
            let n = c.weightmb.get(at).copied().map_or(0, usize::from);
            at = at.saturating_add(1 + n);
        }
        let count = c.weightmb.get(at).copied().map_or(0, usize::from);
        let count = if c.weightmb.get(at + 1..at + 1 + count).is_some() { count } else { 0 };
        Weights { data: c.weightmb, start: at + 1, count, wide: false }
    }

    #[inline(always)]
    fn step(c: &Collate, at: usize) -> (Weights, usize) {
        let raw = c.weightmb.get(at).copied().map_or(0, usize::from);
        let start = at.saturating_add(1);
        let count = if c.weightmb.get(start..start.saturating_add(raw)).is_some() { raw } else { 0 };
        (Weights { data: c.weightmb, start, count, wide: false }, start.saturating_add(raw))
    }
}

/// The element at `pos` of a wide string of `len` units read through `at`.
fn wide_element(c: &Collate, len: usize, at: impl Fn(usize) -> Option<u32>, pos: usize) -> ElementRef {
    let first = at(pos).unwrap_or(0);
    let t = c.tablewc.map_or(0, |tab| tab.value_i32(first));
    if t >= 0 {
        let (ruleset, weights) = entry(t);
        return ElementRef(Element { len: 1, ruleset, weights });
    }
    let rest_len = len.saturating_sub(pos + 1);
let rest = |j: usize| at(pos + 1 + j);
    let word = |i: usize| i32_at(c.extrawc, 4 * i);
    let mut p = usize::try_from(t.unsigned_abs()).unwrap_or(usize::MAX);
    while let (Some(idx), Some(len)) = (word(p), word(p + 1)) {
        let Ok(len) = usize::try_from(len) else { break };
        let body = p + 2;
        let unit = |base: usize| move |j: usize| word(base + j).map_or(u32::MAX, |v| u32::from_le_bytes(v.to_le_bytes()));
        if idx >= 0 {
            if rest_len >= len && (0..len).all(|j| rest(j) == Some(unit(body)(j))) {
                let (ruleset, weights) = entry(idx);
                return ElementRef(Element { len: 1 + len, ruleset, weights });
            }
            p = body + len;
        } else {
            if rest_len >= len {
                let cand = |j: usize| rest(j).unwrap_or(0);
                if let Some(off) = range_offset(unit(body), unit(body + len), cand, len) {
                    let v = indirect(c.indirectwc, idx, off);
                    let (ruleset, weights) = entry(v);
                    return ElementRef(Element { len: 1 + len, ruleset, weights });
                }
            }
            p = body + 2 * len;
        }
    }
    ElementRef(Element { len: 1, ruleset: 0, weights: usize::MAX })
}

impl Text for &[u32] {
    fn len(&self) -> usize {
        <[u32]>::len(self)
    }

    fn element(&self, c: &Collate, pos: usize) -> ElementRef {
        wide_element(c, self.len(), |i| self.get(i).copied(), pos)
    }

    fn same(&self, other: &Self) -> bool {
        self == other
    }

    fn weights(c: &Collate, at: usize, level: usize) -> Weights {
        let w = |i: usize| u32_at(c.weightwc, 4 * i).map_or(0, |v| usize::try_from(v).unwrap_or(0));
        let mut at = at;
        for _ in 0..level {
            at = at.saturating_add(1 + w(at));
        }
        let count = w(at);
        let count = if c.weightwc.get(4 * (at + 1)..4 * (at + 1 + count)).is_some() { count } else { 0 };
        Weights { data: c.weightwc, start: at + 1, count, wide: true }
    }

    #[inline(always)]
    fn step(c: &Collate, at: usize) -> (Weights, usize) {
        let word = |i: usize| i.checked_mul(4).and_then(|b| u32_at(c.weightwc, b));
        let raw = word(at).map_or(0, |v| usize::try_from(v).unwrap_or(0));
        let start = at.saturating_add(1);
        let end = start.saturating_add(raw);
        let fits = start.checked_mul(4).zip(end.checked_mul(4)).is_some_and(|(a, b)| c.weightwc.get(a..b).is_some());
        (Weights { data: c.weightwc, start, count: if fits { raw } else { 0 }, wide: true }, end)
    }
}

impl Text for &[i32] {
    fn len(&self) -> usize {
        <[i32]>::len(self)
    }

    fn element(&self, c: &Collate, pos: usize) -> ElementRef {
        // `wchar_t` units hold code points; the conversion reinterprets the bits.
        wide_element(c, self.len(), |i| self.get(i).map(|w| u32::from_ne_bytes(w.to_ne_bytes())), pos)
    }

    fn same(&self, other: &Self) -> bool {
        self == other
    }

    fn weights(c: &Collate, at: usize, level: usize) -> Weights {
        <&[u32] as Text>::weights(c, at, level)
    }

    #[inline(always)]
    fn step(c: &Collate, at: usize) -> (Weights, usize) {
        <&[u32] as Text>::step(c, at)
    }
}

/// Where a key finds the elements of its string at a level. Opaque outside this module.
#[doc(hidden)]
pub trait Source<T: Text> {
    /// The element at position `p`, or `None` past the end.
    fn at(&mut self, c: &Collate, s: T, level: usize, p: usize) -> Option<ElementRef>;
    /// The position after the element `e` at `p`.
    fn after(&self, level: usize, p: usize, e: &ElementRef) -> usize;
    /// The `k`-th element (0-based) from position `start`.
    fn nth(&mut self, c: &Collate, s: T, level: usize, start: usize, k: usize) -> ElementRef {
        let mut p = start;
        let mut e = self.at(c, s, level, p).unwrap_or_default();
        for _ in 0..k {
            p = self.after(level, p, &e);
            e = self.at(c, s, level, p).unwrap_or_default();
        }
        e
    }
}

/// Elements found in the string at each visit (positions are unit offsets): nothing stored.
#[derive(Clone, Copy, Debug, Default)]
pub struct Lazy;

impl<T: Text> Source<T> for Lazy {
    fn at(&mut self, c: &Collate, s: T, _: usize, p: usize) -> Option<ElementRef> {
        (p < s.len()).then(|| s.element(c, p))
    }

    fn after(&self, _: usize, p: usize, e: &ElementRef) -> usize {
        p + e.0.len.max(1)
    }
}

/// Elements found at each visit on the first level, as [`Lazy`] (most comparisons end there),
/// then all found once and kept for the later levels (positions are then element indexes):
/// inline up to 32 (768 bytes), 24 bytes per element on the heap beyond.
#[derive(Debug, Default)]
struct Memo {
    elems: Option<InlineVec<Element, 32>>,
    /// No memory for the elements: the key is then unusable.
    failed: bool,
}

impl<T: Text> Source<T> for &mut Memo {
    fn at(&mut self, c: &Collate, s: T, level: usize, p: usize) -> Option<ElementRef> {
        if level == 0 {
            return Lazy.at(c, s, level, p);
        }
        let elems = self.elems.get_or_insert_with(|| {
            let mut v = InlineVec::new();
            let mut q = 0;
            while q < s.len() {
                let e = s.element(c, q).0;
                if v.push(e).is_err() {
                    self.failed = true;
                    break;
                }
                q += e.len.max(1);
            }
            v
        });
        elems.get(p).copied().map(ElementRef)
    }

    fn after(&self, level: usize, p: usize, e: &ElementRef) -> usize {
        if level == 0 { <Lazy as Source<T>>::after(&Lazy, level, p, e) } else { p + 1 }
    }

    fn nth(&mut self, c: &Collate, s: T, level: usize, start: usize, k: usize) -> ElementRef {
        if level == 0 {
            return Lazy.nth(c, s, level, start, k);
        }
        self.at(c, s, level, start + k).unwrap_or_default()
    }
}

/// The lazy key of a string (see the module documentation).
#[derive(Clone)]
pub struct Key<'a, T: Text, S: Source<T> = Lazy> {
    c: &'a Collate,
    s: T,
    src: S,
    level: usize,
    /// Position of the next element to visit in string order (forward walk).
    pos: usize,
    /// The current backward run, if any: its start, its end, and how many of its elements
    /// remain to be emitted (emitted from the last).
    run: Option<(usize, usize, usize)>,
    cur: Weights,
    cur_i: usize,
    skipped: u32,
    position: bool,
    level_emitted: bool,
    level_done: bool,
    /// One unit of lookahead: the final unit is dropped if it is a separator.
    held: Option<u32>,
    done: bool,
}

/// The key of `s` under `c`, unit by unit.
pub fn key<T: Text>(c: &Collate, s: T) -> Key<'_, T> {
    key_with(c, s, Lazy)
}

fn key_with<T: Text, S: Source<T>>(c: &Collate, s: T, src: S) -> Key<'_, T, S> {
    Key {
        c,
        s,
        src,
        level: 0,
        pos: 0,
        run: None,
        cur: Weights { data: &[], start: 0, count: 0, wide: false },
        cur_i: 0,
        skipped: 0,
        position: false,
        level_emitted: false,
        level_done: false,
        held: None,
        done: s.len() == 0,
    }
}

impl<T: Text, S: Source<T>> Key<'_, T, S> {
    fn backward(&self, e: &Element) -> bool {
        self.c.rule(e.ruleset, self.level) & BACKWARD != 0
    }

    /// The element at position `p`, or `None` past the end.
    fn element_at(&mut self, p: usize) -> Option<Element> {
        self.src.at(self.c, self.s, self.level, p).map(|e| e.0)
    }

    /// The position after the element `e` at `p`.
    fn after(&self, p: usize, e: &Element) -> usize {
        self.src.after(self.level, p, &ElementRef(*e))
    }

    /// The `k`-th element (0-based) of the run starting at `start`.
    fn run_element(&mut self, start: usize, k: usize) -> Element {
        self.src.nth(self.c, self.s, self.level, start, k).0
    }

    /// The next element of this level in visiting order, or `None` at the level's end.
    fn next_element(&mut self) -> Option<Element> {
        if let Some((start, end, left)) = self.run {
            if left > 0 {
                self.run = Some((start, end, left - 1));
                return Some(self.run_element(start, left - 1));
            }
            self.run = None;
            self.pos = end;
        }
        let e = self.element_at(self.pos)?;
        if !self.backward(&e) {
            self.pos = self.after(self.pos, &e);
            return Some(e);
        }
        // A backward run: find its end and length, then emit it from the last element.
        let (start, mut p, mut n) = (self.pos, self.pos, 0usize);
        while let Some(x) = self.element_at(p) {
            if !self.backward(&x) {
                break;
            }
            p = self.after(p, &x);
            n += 1;
        }
        self.run = Some((start, p, n - 1));
        Some(self.run_element(start, n - 1))
    }

    /// The next unit before the final-separator rule.
    fn raw_next(&mut self) -> Option<u32> {
        loop {
            if self.level >= self.c.nrules {
                return None;
            }
            if self.cur_i < self.cur.count {
                self.cur_i += 1;
                self.level_emitted = true;
                return Some(self.cur.get(self.cur_i - 1));
            }
            if self.level_done {
                // The level's end: a separator unless it is an empty position level.
                let sep = !(self.position && !self.level_emitted);
                self.level += 1;
                self.pos = 0;
                self.run = None;
                self.skipped = 0;
                self.position = false;
                self.level_emitted = false;
                self.level_done = false;
                if sep {
                    return Some(1);
                }
                continue;
            }
            match self.next_element() {
                None => self.level_done = true,
                Some(e) => {
                    let w = if e.weights == usize::MAX {
                        Weights { data: &[], start: 0, count: 0, wide: false }
                    } else {
                        T::weights(self.c, e.weights, self.level)
                    };
                    self.cur = w;
                    self.cur_i = 0;
                    if self.c.rule(e.ruleset, self.level) & POSITION != 0 {
                        self.position = true;
                        if w.count == 0 {
                            self.skipped = self.skipped.saturating_add(1);
                            continue;
                        }
                        let sep = self.skipped.saturating_add(1);
                        self.skipped = 0;
                        self.level_emitted = true;
                        return Some(sep);
                    }
                }
            }
        }
    }
}

impl<T: Text, S: Source<T>> Iterator for Key<'_, T, S> {
    type Item = u32;

    fn next(&mut self) -> Option<u32> {
        if self.done {
            return None;
        }
        if self.held.is_none() {
            self.held = self.raw_next();
        }
        let cur = self.held?;
        let next = self.raw_next();
        match next {
            None => {
                self.done = true;
                self.held = None;
                if cur == 1 { None } else { Some(cur) }
            }
            Some(n) => {
                self.held = Some(n);
                Some(cur)
            }
        }
    }
}

/// Where [`key_walk`] puts a key: an element's weights at a level, and single units (the
/// separators).
trait KeySink {
    fn weights(&mut self, w: &Weights);
    fn unit(&mut self, u: u32);
}

/// The whole key of `s` under `c` in one pass (STRXFRM-EN), into `sink`; its last unit is
/// returned (`None`: no units). The units are exactly those of [`key`] before the final-separator
/// rule; the difference is the cost: the string's elements are found once (not once per level
/// and visit) and each level is a plain walk over them. `Err(())` (nothing put) if the elements
/// cannot be kept (no memory beyond the inline 32): use [`key`] then.
fn key_walk<T: Text>(c: &Collate, s: T, sink: &mut impl KeySink) -> Result<Option<u32>, ()> {
    if s.is_empty() {
        return Ok(None);
    }
    let mut found: InlineVec<Element, 32> = InlineVec::new();
    // The rulesets the string uses, one bit each (`None`: one beyond 63).
    let mut seen: Option<u64> = Some(0);
    let mut q = 0;
    while q < s.len() {
        let e = s.element(c, q).0;
        found.push(e).map_err(|_| ())?;
        seen = seen.and_then(|m| Some(m | 1u64.checked_shl(u32::try_from(e.ruleset).ok()?)?));
        q += e.len.max(1);
    }
    // Each element's `weights` is from here on a cursor at the current level's weights: every
    // level visits every element once, so `T::step` advances it (no walk from level 0).
    let elems = found.as_mut_slice();
    let mut last = None;
    for level in 0..c.nrules {
        // The level's rule when every ruleset the string uses agrees on it (the common case: a
        // locale's sections share their directions): read once per level, not per element.
        let fixed = seen.and_then(|m| c.common_rule(m, level));
        let mut st = LevelState { position: false, emitted: false, skipped: 0 };
        match fixed {
            Some(r) if r & (BACKWARD | POSITION) == 0 => {
                // Forward without positions: each element's weights in order, then the
                // separator (which is then the last unit so far).
                for e in elems.iter_mut() {
                    let w = advance::<T>(c, &mut e.weights);
                    if w.count > 0 {
                        sink.weights(&w);
                    }
                }
                sink.unit(1);
                last = Some(1);
                continue;
            }
            Some(r) if r & BACKWARD == 0 => {
                for e in elems.iter_mut() {
                    st.visit::<T>(c, e, r, sink, &mut last);
                }
            }
            _ => {
                let rule = |e: &Element| fixed.unwrap_or_else(|| c.rule(e.ruleset, level));
                let mut i = 0;
                while let Some(first) = elems.get(i) {
                    // A run of backward elements is visited from its last element; a forward
                    // element is a run of one.
                    let mut j = i + 1;
                    if rule(first) & BACKWARD != 0 {
                        while elems.get(j).is_some_and(|x| rule(x) & BACKWARD != 0) {
                            j += 1;
                        }
                    }
                    for e in elems.get_mut(i..j).unwrap_or_default().iter_mut().rev() {
                        let r = rule(e);
                        st.visit::<T>(c, e, r, sink, &mut last);
                    }
                    i = j;
                }
            }
        }
        // The level's end: a separator unless it is an empty position level.
        if !(st.position && !st.emitted) {
            sink.unit(1);
            last = Some(1);
        }
    }
    Ok(last)
}

/// One level of [`key_walk`] in progress: whether a position rule was seen, whether a unit was
/// put, and how many weightless position elements wait for the next separator.
struct LevelState {
    position: bool,
    emitted: bool,
    skipped: u32,
}

impl LevelState {
    /// Puts element `e` (rule `r` at this level) and moves its cursor to the next level.
    #[inline(always)]
    fn visit<T: Text>(&mut self, c: &Collate, e: &mut Element, r: u8, sink: &mut impl KeySink, last: &mut Option<u32>) {
        let w = advance::<T>(c, &mut e.weights);
        if r & POSITION != 0 {
            self.position = true;
            if w.count == 0 {
                self.skipped = self.skipped.saturating_add(1);
                return;
            }
            let sep = self.skipped.saturating_add(1);
            sink.unit(sep);
            *last = Some(sep);
            self.skipped = 0;
            self.emitted = true;
        }
        if w.count > 0 {
            sink.weights(&w);
            *last = Some(w.get(w.count - 1));
            self.emitted = true;
        }
    }
}

/// The weights at cursor `cur` (an element's weights at the current level; `usize::MAX` = an
/// element without weights, empty at every level), moving `cur` to the next level.
#[inline(always)]
fn advance<T: Text>(c: &Collate, cur: &mut usize) -> Weights {
    if *cur == usize::MAX {
        return Weights { data: &[], start: 0, count: 0, wide: false };
    }
    let (w, next) = T::step(c, *cur);
    *cur = next;
    w
}

/// The whole key of `s` under `c` in one pass (STRXFRM-EN): unit `i` of the key goes to
/// `put(i, unit)`, and the key's length is returned. The units are exactly those of [`key`],
/// except that a dropped final separator is put too (at the returned length). `None` (nothing
/// put) if the elements cannot be kept: use [`key`] then.
pub fn key_each<T: Text>(c: &Collate, s: T, put: impl FnMut(usize, u32)) -> Option<usize> {
    struct Each<P>(P, usize);
    impl<P: FnMut(usize, u32)> KeySink for Each<P> {
        #[inline(always)]
        fn weights(&mut self, w: &Weights) {
            for x in 0..w.count {
                self.unit(w.get(x));
            }
        }
        #[inline(always)]
        fn unit(&mut self, u: u32) {
            (self.0)(self.1, u);
            self.1 += 1;
        }
    }
    let mut sink = Each(put, 0);
    let last = key_walk(c, s, &mut sink).ok()?;
    Some(sink.1 - usize::from(last == Some(1)))
}

/// The bytes the reference's `strxfrm` writes for one wide weight unit (COLLATE-STRXFRM-DA-WEIGHT):
/// the UTF-8 form of a unit from 0x80 on (so 0x97 is `c2 97`, as in da_DK.UTF-8's long-string
/// keys), the byte itself below that; a unit that is not a scalar value stays one byte, 255.
#[inline(always)]
fn unit_bytes(u: u32, buf: &mut [u8; 4]) -> &[u8] {
    if u < 0x80 {
        buf[0] = u.to_le_bytes()[0];
        return &buf[..1];
    }
    match char::from_u32(u) {
        Some(ch) => ch.encode_utf8(buf).as_bytes(),
        None => {
            buf[0] = u8::MAX;
            &buf[..1]
        }
    }
}

/// The `strxfrm` key of `s` under `c` in one pass, as [`key_each`] with the units as bytes
/// (a unit from 0x80 on in its UTF-8 form, as the reference writes it): its leading bytes into `out` as
/// far as they fit (a dropped final separator may be written at the returned length); returns
/// the key's length.
pub fn key_bytes(c: &Collate, s: &[u8], out: &mut [u8]) -> Option<usize> {
    struct Bytes<'o>(&'o mut [u8], usize);
    impl KeySink for Bytes<'_> {
        #[inline(always)]
        fn weights(&mut self, w: &Weights) {
            if w.wide {
                for x in 0..w.count {
                    self.unit(w.get(x));
                }
                return;
            }
            // Weights are one to three bytes (en_US: two at the first level, one at the middle
            // ones, two or three at the position level): a checked store per byte beats a
            // memcpy call (measured, STRXFRM-EN). Bytes past `out` are counted, not written.
            for &b in w.data.get(w.start..w.start.saturating_add(w.count)).unwrap_or_default() {
                if let Some(o) = self.0.get_mut(self.1) {
                    *o = b;
                }
                self.1 += 1;
            }
        }
        #[inline(always)]
        fn unit(&mut self, u: u32) {
            let mut utf8 = [0u8; 4];
            for &b in unit_bytes(u, &mut utf8) {
                if let Some(o) = self.0.get_mut(self.1) {
                    *o = b;
                }
                self.1 += 1;
            }
        }
    }
    /// No room at all (`strxfrm(NULL, s, 0)`, the measuring call): the key's length only, each
    /// element's weights counted at once instead of stored byte by byte (STRXFRM-3).
    struct Count(usize);
    impl KeySink for Count {
        #[inline(always)]
        fn weights(&mut self, w: &Weights) {
            if w.wide {
                for x in 0..w.count {
                    self.unit(w.get(x));
                }
                return;
            }
            self.0 += w.count;
        }
        #[inline(always)]
        fn unit(&mut self, u: u32) {
            self.0 += unit_bytes(u, &mut [0u8; 4]).len();
        }
    }
    if out.is_empty() {
        let mut sink = Count(0);
        let last = key_walk(c, s, &mut sink).ok()?;
        return Some(sink.0 - usize::from(last == Some(1)));
    }
    let mut sink = Bytes(out, 0);
    let last = key_walk(c, s, &mut sink).ok()?;
    Some(sink.1 - usize::from(last == Some(1)))
}

/// The first level's weights of a string, in order, while every element's rule at that level
/// is forward without positions (the key's first level is then exactly these units).
struct FirstLevel<'a, T: Text> {
    c: &'a Collate,
    s: T,
    pos: usize,
    cur: Weights,
    i: usize,
}

impl<T: Text> FirstLevel<'_, T> {
    /// Whether the current element's units are all taken.
    fn between(&self) -> bool {
        self.i >= self.cur.count
    }

    /// The next element, `Ok(None)` at the string's end, `Err(())` if its rule is backward or
    /// by position.
    fn element(&mut self) -> Result<Option<Element>, ()> {
        if self.pos >= self.s.len() {
            return Ok(None);
        }
        let e = self.s.element(self.c, self.pos).0;
        if self.c.rule(e.ruleset, 0) & (BACKWARD | POSITION) != 0 {
            return Err(());
        }
        self.pos += e.len.max(1);
        Ok(Some(e))
    }

    /// Take the units of element `e` next.
    fn load(&mut self, e: &Element) {
        self.cur = if e.weights == usize::MAX { Weights { data: &[], start: 0, count: 0, wide: false } } else { T::weights(self.c, e.weights, 0) };
        self.i = 0;
    }

    /// Take the one-byte element at `pos` whose [`Collate::ascii1`] entry is `u` (not SLOW).
    #[inline]
    fn take_packed(&mut self, u: u32) {
        self.pos += 1;
        self.cur = Weights { data: self.c.weightmb, start: usize::try_from(u >> 8).unwrap_or(0), count: usize::from(u.to_le_bytes()[0]), wide: false };
        self.i = 0;
    }

    /// The next unit, `Ok(None)` at the level's end, `Err(())` at an element whose rule is
    /// backward or by position.
    fn next(&mut self) -> Result<Option<u32>, ()> {
        loop {
            if self.i < self.cur.count {
                self.i += 1;
                return Ok(Some(self.cur.get(self.i - 1)));
            }
            // ASCII lane (D-4.30): a one-byte element's weights straight from the table.
            let u = self.s.ascii1(self.c, self.pos);
            if u != ASCII_SLOW {
                self.take_packed(u);
                continue;
            }
            let Some(e) = self.element()? else { return Ok(None) };
            self.load(&e);
        }
    }
}

/// What the first levels of two keys show.
enum First {
    /// They decide the order.
    Order(core::cmp::Ordering),
    /// They are equal, and each is followed by a separator.
    Tied,
    /// Neither (a rule this shortcut leaves to the keys, or an end it cannot judge).
    Unknown,
}

/// The two keys' first levels compared without the key machinery. With more than one level,
/// each key's first level (forward, no positions) is followed by a separator (1), which ends
/// the key only when it is dropped as the final unit; so a first level that is a prefix of the
/// other's orders first when the other's next unit is above 1.
fn first_level<T: Text>(c: &Collate, a: T, b: T) -> First {
    use core::cmp::Ordering::{Greater, Less};
    if c.nrules < 2 {
        return First::Unknown;
    }
    c.warm();
    let empty = Weights { data: &[], start: 0, count: 0, wide: false };
    let mut la = FirstLevel { c, s: a, pos: 0, cur: empty, i: 0 };
    let mut lb = FirstLevel { c, s: b, pos: 0, cur: empty, i: 0 };
    loop {
        if la.between() && lb.between() {
            // Both at an element's start: elements with the same weights give the same units
            // (two one-byte ASCII elements with the same table entry give the same first-level
            // units; D-4.30).
            let (ua, ub) = (la.s.ascii1(c, la.pos), lb.s.ascii1(c, lb.pos));
            if ua != ASCII_SLOW && ub != ASCII_SLOW {
                if ua == ub {
                    la.pos += 1;
                    lb.pos += 1;
                    continue;
                }
                la.take_packed(ua);
                lb.take_packed(ub);
            } else {
                let (Ok(x), Ok(y)) = (la.element(), lb.element()) else { return First::Unknown };
                match (x, y) {
                    (Some(x), Some(y)) if x.weights == y.weights => continue,
                    (None, None) => return First::Tied,
                    (x, y) => {
                        if let Some(x) = x {
                            la.load(&x);
                        }
                        if let Some(y) = y {
                            lb.load(&y);
                        }
                    }
                }
            }
        }
        let (Ok(x), Ok(y)) = (la.next(), lb.next()) else { return First::Unknown };
        match (x, y) {
            (Some(x), Some(y)) if x != y => return First::Order(x.cmp(&y)),
            (Some(_), Some(_)) => {}
            (None, None) => return First::Tied,
            (None, Some(y)) if y > 1 => return First::Order(Less),
            (Some(x), None) if x > 1 => return First::Order(Greater),
            _ => return First::Unknown,
        }
    }
}

/// The [`AsciiSlot::unit`] entry of the first element with first-level units from byte `x`
/// on, reading on through `next` past bytes without any: [`UNIT_END`] at the terminator,
/// [`UNIT_SLOW`] where the general path is needed.
#[inline(always)]
fn stream_unit<F: FnMut() -> u8>(t: &[AtomicU32; 256], mut x: u8, next: &mut Peek<F>) -> u32 {
    loop {
        let v = unit_of(t, x);
        if v == UNIT_IGNORE {
            x = next.get();
            continue;
        }
        if v & 0xc0 == UNIT_LOOK {
            // The next byte decides whether a contraction starts here; it stays for the caller.
            let n = next.get();
            next.held = Some(n);
            return if n >= 0x80 { UNIT_SLOW } else { v & !UNIT_LOOK };
        }
        return v;
    }
}

/// [`AsciiSlot::unit`]'s entry for byte `x`.
#[inline(always)]
fn unit_of(t: &[AtomicU32; 256], x: u8) -> u32 {
    t.get(usize::from(x)).map_or(UNIT_SLOW, |v| v.load(Ordering::Relaxed))
}

/// A byte source with one byte of lookahead put back ([`UNIT_LOOK`]).
struct Peek<F> {
    next: F,
    held: Option<u8>,
}

impl<F: FnMut() -> u8> Peek<F> {
    #[inline(always)]
    fn get(&mut self) -> u8 {
        match self.held.take() {
            Some(x) => x,
            None => (self.next)(),
        }
    }
}

/// Whether a shared `prefix` (no terminator in it) holds a byte the table leaves to the general
/// path; with [`AsciiSlot::ascii_fast`] that is any non-ASCII byte (one OR over the bytes).
/// A [`UNIT_LOOK`] byte is not one: the byte after it is checked where it is read.
#[inline]
fn prefix_slow(slot: Option<&AsciiSlot>, t: &[AtomicU32; 256], prefix: &[u8]) -> bool {
    if slot.is_some_and(|s| s.ascii_fast.load(Ordering::Relaxed)) {
        any_high(prefix)
    } else {
        prefix.iter().any(|&x| unit_of(t, x) == UNIT_SLOW)
    }
}

/// Whether any byte of `p` is non-ASCII (bit 7 set): whole words, the last one overlapping
/// the one before it (a short prefix costs one or two loads, STRCOLL-3).
#[inline]
fn any_high(p: &[u8]) -> bool {
    const HIGH: u64 = 0x8080_8080_8080_8080;
    if let Some(tail) = p.last_chunk::<8>() {
        let m = p.chunks_exact(8).fold(u64::from_ne_bytes(*tail), |m, w| {
            m | w.first_chunk::<8>().map_or(HIGH, |w| u64::from_ne_bytes(*w))
        });
        return m & HIGH != 0;
    }
    if let (Some(head), Some(tail)) = (p.first_chunk::<4>(), p.last_chunk::<4>()) {
        return (u32::from_ne_bytes(*head) | u32::from_ne_bytes(*tail)) & 0x8080_8080 != 0;
    }
    p.iter().fold(0u8, |m, &x| m | x) >= 0x80
}

/// `strcoll`'s decision at the first differing bytes, without reading further (STRCOLL-3,
/// D-4.37 amendment): the strings share `prefix` (no terminator in it) and then hold the bytes
/// `x` and `y`, the first where they differ or both terminators. Byte-identical strings are
/// equal. Otherwise the order is decided here only in the common case [`compare_stream`] would
/// decide at its first step: no shared byte the table leaves to the general path, and `x` and
/// `y` both plain one-byte elements (units, no flag) whose first units differ; that first
/// unit's order is the answer ([`unit_order`] of the two entries). `None`: [`compare_stream`]
/// decides (from the same prefix).
#[inline]
pub fn decide_first(c: &Collate, prefix: &[u8], x: u8, y: u8) -> Option<core::cmp::Ordering> {
    decide_first_flag(c, any_high(prefix), x, y)
}

/// [`decide_first`] with the prefix's test already made (STRCOLL-4): `high` is whether a
/// shared byte (or `x`) is non-ASCII - `strcoll`'s kernel reports it from the blocks it loaded.
/// Counting `x` changes nothing: a non-ASCII `x` has no [`AsciiSlot::rank`] (no decision).
/// The decision is [`decide_first`]'s, from [`AsciiSlot::rank`]: both bytes plain with
/// different first units, `ascii_fast` (else every rank is 0) and no non-ASCII shared byte.
#[inline]
pub fn decide_first_flag(c: &Collate, high: bool, x: u8, y: u8) -> Option<core::cmp::Ordering> {
    if x == y {
        return (x == 0).then_some(core::cmp::Ordering::Equal);
    }
    let slot = c.ascii1?;
    let rank = |b: u8| slot.rank.get(usize::from(b)).map_or(0, |r| r.load(Ordering::Relaxed));
    let (rx, ry) = (rank(x), rank(y));
    if high | (rx == 0) | (ry == 0) | (rx == ry) {
        return None;
    }
    Some(rx.cmp(&ry))
}

/// Whether an [`AsciiSlot::unit`] entry is plain: its low byte is its unit count
/// (1..=[`UNIT_MAX`]); IGNORE (0), END, SLOW and [`UNIT_LOOK`] entries all fall outside that range.
#[inline(always)]
fn plain_unit(v: u32) -> bool {
    (v & 0xff).wrapping_sub(1) < UNIT_MAX
}

/// How two [`AsciiSlot::unit`] entries with units start the rest of the first levels: their
/// units' order where they differ within the shorter run (left-aligned, so one comparison of
/// the bits both runs hold), `Some(Equal)` when they are the same run (go on), `None` when one
/// run is a proper prefix of the other (the next element decides: the general path).
#[inline(always)]
fn unit_order(x: u32, y: u32) -> Option<core::cmp::Ordering> {
    if x == y {
        return Some(core::cmp::Ordering::Equal);
    }
    let n = (x & 0xff).min(y & 0xff);
    let shift = 32 - 8 * n;
    let (a, b) = (x >> shift, y >> shift);
    if a == b {
        None
    } else {
        Some(a.cmp(&b))
    }
}

/// `strcoll`'s single pass over two C strings (STRCOLL-CTEXT, STRCOLL-2; D-4.37): both start with
/// `prefix` (no terminator in it; found by a word-wise kernel), and `a` and `b` give each
/// string's next byte after it (0 = the terminator, after which neither is called again). Byte-identical
/// strings are equal (read to the end only while equal). Otherwise the first levels are compared unit by unit from
/// [`AsciiSlot::unit`] as [`first_level`] compares them (one-byte elements only, the same
/// separator rule at an end), and the first difference decides; `None` (the caller takes the
/// general path from the start) at equal first levels, at an end the rule cannot judge, and at
/// any byte the table leaves to the general path. Equal bytes give equal units (a contraction
/// cannot start at a byte the table covers, except a [`UNIT_LOOK`] byte followed by a non-ASCII
/// byte, which the table leaves to the general path), so they are skipped without a lookup.
pub fn compare_stream(c: &Collate, prefix: &[u8], a: impl FnMut() -> u8, b: impl FnMut() -> u8) -> Option<core::cmp::Ordering> {
    use core::cmp::Ordering::{Equal, Greater, Less};
    let t = c.ascii1.map_or(&NO_UNITS, |slot| &slot.unit);
    let (mut a, mut b) = (Peek { next: a, held: None }, Peek { next: b, held: None });
    let (mut differ, mut slow) = (false, false);
    // The shared prefix is checked only once the strings differ (identical strings never).
    let mut pending = Some(prefix);
    loop {
        let (x, y) = (a.get(), b.get());
        if x == y {
            if x == 0 {
                return if differ { None } else { Some(Equal) };
            }
            // After a byte the table leaves to the general path, only "the same bytes" can
            // still be decided here.
            slow |= unit_of(t, x) == UNIT_SLOW;
            continue;
        }
        if let Some(p) = pending.take() {
            slow |= prefix_slow(c.ascii1, t, p);
        }
        if slow {
            return None;
        }
        differ = true;
        let (ux, uy) = (stream_unit(t, x, &mut a), stream_unit(t, y, &mut b));
        if ux == UNIT_SLOW || uy == UNIT_SLOW {
            return None;
        }
        // At an end the other's first unit decides when it is above the separator (1).
        let o = match (ux == UNIT_END, uy == UNIT_END) {
            (false, false) => unit_order(ux, uy)?,
            (true, false) if uy >> 24 > 1 => return Some(Less),
            (false, true) if ux >> 24 > 1 => return Some(Greater),
            _ => return None,
        };
        if o.is_ne() {
            return Some(o);
        }
    }
}

/// Compare two strings as their keys compare (`strcoll`, `wcscoll`). Equal strings have equal
/// keys. Most other pairs differ in their first level, which is compared directly. Otherwise
/// the keys are compared as they are produced, and each string's elements are found once, as
/// the second level reaches them, and kept for the later levels (memory per string:
/// [`Memo`]'s, freed on return). Without memory for the elements, the comparison is made
/// again with keys that find the elements at each visit.
pub fn compare<T: Text>(c: &Collate, a: T, b: T) -> core::cmp::Ordering {
    if a.same(&b) {
        return core::cmp::Ordering::Equal;
    }
    let first = first_level(c, a, b);
    if let First::Order(o) = first {
        return o;
    }
    let (mut ma, mut mb) = (Memo::default(), Memo::default());
    let (mut ka, mut kb) = (key_with(c, a, &mut ma), key_with(c, b, &mut mb));
    if matches!(first, First::Tied) && !a.is_empty() && !b.is_empty() {
        // Equal first levels: compare from the separator after them on (the final-unit rule
        // still applies to the whole key, as it ends the same).
        for k in [&mut ka, &mut kb] {
            k.level = 1;
            k.held = Some(1);
        }
    }
    let ord = ka.cmp(kb);
    if ma.failed || mb.failed {
        return key(c, a).cmp(key(c, b));
    }
    ord
}
