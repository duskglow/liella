//! The class-word table of an `LC_CTYPE` (D-5.9, D-5.10): stage 1 by block (`c >> 8`), a
//! 256-word leaf by `c & 255`, every word the `Class` (`_IS*`) bits of one code point.
//!
//! One type serves every locale, so that the wide class tests read one shape (D-5.11: a thread
//! block holds the active table's address). A built-in locale's stage 1 is complete when the
//! program starts (the `cargo xtask unicode-gen` tables, D-5.9). A compiled locale's starts at
//! 0 (unresolved) in every block: the first test in a block works out its 256 words from the
//! file's class tables and publishes the block's leaf, one of the built-in leaves when they are
//! equal (every block of `en_US.UTF-8`), else a leaf of the shared [`POOL`]. Building every block
//! when a locale is loaded cost 160-270 ms per locale (WCTYPE-RETHINK), so nothing is built
//! before it is asked for.

use super::ctype::Wide;
use crate::ctype::w_classes_file;
use crate::unicode::class_words::{self, LEAVES, STAGE1_LEN};
use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicUsize, Ordering::{Acquire, Relaxed, Release}};

/// A stage-1 value: 0 = not resolved yet; `1..=LEAVES.len()`: built-in leaf `v - 1`;
/// `POOL_BASE + k`: [`POOL`] leaf `k`; [`WALK`]: no leaf (every test walks the file's tables).
/// The values fit in 16 bits; a stage-1 cell is 32 bits wide because a 16-bit atomic load
/// left the compiler a separate zero extension before the leaf index on every test (one
/// instruction per call), where a 32-bit load extends for free (WCTYPE-TRIE-FILE-2).
const POOL_BASE: u32 = 0x8000;

/// The stage-1 value of a block whose leaf did not fit in [`POOL`].
const WALK: u32 = 0xffff;

/// How many leaves the shared pool holds (128 KiB of `.bss`, touched only by a process that
/// resolves a block no built-in leaf matches).
const POOL_LEAVES: usize = 256;

/// How many compiled `LC_CTYPE` views get a table (later ones walk the file's tables).
const FILE_SLOTS: usize = 16;

/// What a missing block does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    /// A built-in locale: every block is resolved from the start.
    Builtin,
    /// A compiled locale: resolve on first use; `share` looks for an equal built-in leaf first
    /// (always, but for the tests that exercise the pool).
    File { share: bool },
    /// A compiled locale left without a table: never written, every test walks.
    Walk,
}

/// A two-stage class-word table (see the module documentation).
#[derive(Debug)]
pub struct ClassTable {
    stage1: [AtomicU32; STAGE1_LEN],
    mode: Mode,
}

/// The leaves shared by every compiled locale's table: a leaf is a pure function of its 256
/// words, so two locales with the same block use one leaf. A leaf is written whole before its
/// `ready` flag (Release) and before any stage 1 names it (Release).
#[derive(Debug)]
struct Pool {
    leaves: [[AtomicU16; 256]; POOL_LEAVES],
    ready: [AtomicBool; POOL_LEAVES],
    next: AtomicUsize,
}

static POOL: Pool = Pool {
    leaves: [const { [const { AtomicU16::new(0) }; 256] }; POOL_LEAVES],
    ready: [const { AtomicBool::new(false) }; POOL_LEAVES],
    next: AtomicUsize::new(0),
};

/// The `"C"` locale's table.
pub static C_TABLE: ClassTable = ClassTable::builtin(&class_words::C_STAGE1);

/// The built-in `C.UTF-8` locale's table.
pub static C_UTF8_TABLE: ClassTable = ClassTable::builtin(&class_words::C_UTF8_STAGE1);

/// The tables of compiled `LC_CTYPE` views, one per view, taken when the view is built. In
/// `.bss`: a page is touched only by a process that asks a wide class question there.
static FILE_TABLES: [ClassTable; FILE_SLOTS] = [const { ClassTable::new(Mode::File { share: true }) }; FILE_SLOTS];

/// The next free slot of [`FILE_TABLES`].
static FILE_NEXT: AtomicUsize = AtomicUsize::new(0);

/// The table of a compiled view that found no free slot.
static WALK_TABLE: ClassTable = ClassTable::new(Mode::Walk);

impl ClassTable {
    const fn new(mode: Mode) -> ClassTable {
        ClassTable { stage1: [const { AtomicU32::new(0) }; STAGE1_LEN], mode }
    }

    /// A built-in locale's table from its generated stage 1 (leaf `i` stored as `i + 1`).
    #[allow(clippy::indexing_slicing)] // a const fn (no `get`): `i < STAGE1_LEN`, both arrays' length
    const fn builtin(s1: &[u8; STAGE1_LEN]) -> ClassTable {
        let mut t = ClassTable::new(Mode::Builtin);
        let mut i = 0;
        while i < STAGE1_LEN {
            t.stage1[i] = AtomicU32::new(u32::from_le_bytes([s1[i], 0, 0, 0]).wrapping_add(1));
            i = i.wrapping_add(1);
        }
        t
    }

    /// A table for a newly built compiled view: a free slot, else the walking table.
    pub fn for_file() -> &'static ClassTable {
        // A view without a slot never comes back here, so the counter grows by one per view.
        let k = FILE_NEXT.fetch_add(1, Relaxed);
        FILE_TABLES.get(k).unwrap_or(&WALK_TABLE)
    }

    /// A fresh compiled-locale table outside the slots (tests: `share` false sends every block
    /// to the pool).
    #[cfg(test)]
    pub(crate) fn file_for_test(share: bool) -> ClassTable {
        ClassTable::new(Mode::File { share })
    }

    /// The class word of `c` when its block is resolved: one stage-1 load and one leaf load.
    /// `None`: not resolved yet, no leaf ([`WALK`]), or `c` above U+10FFFF; [`ClassTable::word`]
    /// then answers.
    #[inline(always)]
    pub fn hit(&self, c: u32) -> Option<u16> {
        let (v, low) = self.stage1_of(c)?;
        match LEAVES.get(leaf_index(v)) {
            Some(l) => l.get(low).copied(),
            None => pool_word(v, low),
        }
    }

    /// Whether `c`'s class word has a bit of `mask`, as the C `int`, when its block is
    /// resolved: [`ClassTable::hit`] with the mask test.
    ///
    /// The test is made in each leaf's arm, not on a merged word: on a word merged from the two
    /// arms the compiler zero-extended again before a test of bit 15 (`iswgraph`, one
    /// instruction per call; WCTYPE-TRIE-FILE-2).
    #[inline(always)]
    pub fn test(&self, c: u32, mask: u16) -> Option<i32> {
        let (v, low) = self.stage1_of(c)?;
        match LEAVES.get(leaf_index(v)) {
            Some(l) => l.get(low).map(|w| i32::from(w & mask != 0)),
            None => pool_word(v, low).map(|w| i32::from(w & mask != 0)),
        }
    }

    /// `c`'s stage-1 value and its index in the leaf; `None` above U+10FFFF.
    #[inline(always)]
    fn stage1_of(&self, c: u32) -> Option<(u32, usize)> {
        let block = usize::try_from(c >> 8).unwrap_or(usize::MAX);
        // Acquire: a pool leaf named here was written before the name (`resolve`, Release).
        let v = self.stage1.get(block)?.load(Acquire);
        Some((v, usize::try_from(c & 0xff).unwrap_or(0)))
    }

    /// The class word of `c` (the standard classes only), `w` being this table's locale's wide
    /// tables (unused by a built-in table): [`ClassTable::hit`], else the block is resolved
    /// now, or the file's tables are walked.
    #[inline(never)]
    pub fn word(&self, c: u32, w: Option<&Wide>) -> u16 {
        if let Some(v) = self.hit(c) {
            return v;
        }
        match (self.mode, w) {
            (Mode::File { share }, Some(w)) => self.resolve(c, w, share),
            (_, Some(w)) => w_classes_file(c, w).0,
            // A built-in table misses only above U+10FFFF, where nothing has a class.
            (_, None) => 0,
        }
    }

    /// The miss of a compiled locale's table: resolve `c`'s block (or walk when it has none).
    #[cold]
    fn resolve(&self, c: u32, w: &Wide, share: bool) -> u16 {
        let block = usize::try_from(c >> 8).unwrap_or(usize::MAX);
        let Some(slot) = self.stage1.get(block) else { return w_classes_file(c, w).0 };
        if slot.load(Acquire) != 0 {
            // Resolved without a leaf (WALK); a block another thread just published is a hit.
            return self.hit(c).unwrap_or_else(|| w_classes_file(c, w).0);
        }
        let first = c & !0xff;
        let words: [u16; 256] = core::array::from_fn(|i| w_classes_file(first | u32::try_from(i).unwrap_or(0), w).0);
        let builtin = if share { LEAVES.iter().position(|l| *l == words) } else { None };
        let v = match builtin {
            Some(i) => u32::try_from(i).map_or(WALK, |i| i.wrapping_add(1)),
            None => pool_add(&words),
        };
        // Release: the pool leaf's words are seen by every thread that reads `v` (Acquire).
        // A thread that lost the race keeps the winner's value: the same words either way.
        let _ = slot.compare_exchange(0, v, Release, Relaxed);
        words.get(usize::try_from(c & 0xff).unwrap_or(0)).copied().unwrap_or(0)
    }

    /// A table as a compiled view without a slot has it (tests).
    #[cfg(test)]
    pub(crate) fn walk_for_test() -> ClassTable {
        ClassTable::new(Mode::Walk)
    }

    /// Marks `block` resolved without a leaf, as a full pool leaves it (tests).
    #[cfg(test)]
    pub(crate) fn mark_walk_for_test(&self, block: usize) {
        if let Some(s) = self.stage1.get(block) {
            s.store(WALK, Release);
        }
    }

    /// The number of blocks resolved to a built-in leaf, to a pool leaf, and to none (tests).
    #[cfg(test)]
    pub(crate) fn census(&self) -> [usize; 4] {
        let mut n = [0usize; 4];
        for s in &self.stage1 {
            let v = s.load(Acquire);
            let k = match v {
                0 => 0,
                WALK => 3,
                v if v >= POOL_BASE => 2,
                _ => 1,
            };
            if let Some(x) = n.get_mut(k) {
                *x = x.saturating_add(1);
            }
        }
        n
    }
}

/// The word `low` of the pool leaf a stage-1 value `v` names; `None` for 0, [`WALK`] and a
/// leaf not in the pool.
#[inline(always)]
fn pool_word(v: u32, low: usize) -> Option<u16> {
    let l = POOL.leaves.get(usize::try_from(v.wrapping_sub(POOL_BASE)).unwrap_or(usize::MAX))?;
    Some(l.get(low)?.load(Relaxed))
}

/// The [`LEAVES`] index a stage-1 value `v` names (past the end for 0, a pool leaf and [`WALK`]).
#[inline(always)]
fn leaf_index(v: u32) -> usize {
    usize::try_from(v).unwrap_or(usize::MAX).wrapping_sub(1)
}

/// The pool leaf equal to `words` (an existing one, else a new one): its stage-1 value, or
/// [`WALK`] when the pool is full.
fn pool_add(words: &[u16; 256]) -> u32 {
    let same = |k: usize| {
        POOL.ready.get(k).is_some_and(|r| r.load(Acquire))
            && POOL.leaves.get(k).is_some_and(|l| l.iter().zip(words).all(|(a, b)| a.load(Relaxed) == *b))
    };
    let named = |k: usize| u32::try_from(k).map_or(WALK, |k| POOL_BASE.wrapping_add(k));
    let filled = POOL.next.load(Acquire).min(POOL_LEAVES);
    if let Some(k) = (0..filled).find(|&k| same(k)) {
        return named(k);
    }
    // A racing thread with the same words may add a second copy: both are correct.
    let k = POOL.next.fetch_add(1, Relaxed);
    let Some(leaf) = POOL.leaves.get(k) else {
        POOL.next.store(POOL_LEAVES, Relaxed);
        return WALK;
    };
    for (a, b) in leaf.iter().zip(words) {
        a.store(*b, Relaxed);
    }
    if let Some(r) = POOL.ready.get(k) {
        r.store(true, Release);
    }
    named(k)
}
