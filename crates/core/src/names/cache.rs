//! The table of the integrated name-service cache (D-16.10): one format for the per-process
//! table and for the shared region of a user (`/dev/shm/liella-nscache-<euid>`), as 64-bit
//! atomic words. The process-wide logic (when to look, what to watch, who may write) is
//! `liella_legacy::nscache`; this module only reads and writes a table, and treats every word of
//! it as untrusted input: another process of the same user may have written anything, died in the
//! middle of a change, or truncated the file.
//!
//! **Layout** (version [`LAYOUT`]; word indices, little-endian bytes in the payloads):
//!
//! | Words | Content |
//! |---|---|
//! | 0 | [`MAGIC`] (`"LIELLANC"`) |
//! | 1 | [`LAYOUT`] |
//! | 2 | the table's size in words (the mapping's size) |
//! | 3 | the owner's user ID |
//! | 4, 5 | the boot ID (128 bits) |
//! | 6 | the account's validity stamp (a hash of its user-database name and its UID) |
//! | 7 | the sequence word of the seqlock (odd while a writer changes the table) |
//! | 8 | the generation of every category |
//! | 9–14 | one generation per [`Category`] |
//! | 15 | the number of hash slots (a power of two, [`geometry`]) |
//! | 16 | the arena's next free word (the head of the ring) |
//! | 17 | the last entry serial number given out |
//! | 20–69 | the identity of each watched [`File`] as last validated (5 words each, [`FileId`]) |
//! | 70–79 | the identities of `passwd` and `nsswitch.conf` the account stamp was verified against |
//! | 80 … | the hash slots, then the entry arena up to the end |
//!
//! **Slots.** A slot is 0 (empty) or an entry's serial number (40 bits) and its arena offset
//! (24 bits). A key's slot is found by linear probing over [`PROBE`] slots from its hash.
//!
//! **Entries** (the arena is a ring: new entries are written at the head, wrapping to the start,
//! so the oldest entries are overwritten first): a header of [`ENTRY_HEAD`] words (a magic
//! with the category and the entry's length, the serial, the key's hash, the two generations
//! the answer was obtained under, the time it was stored and its lifetime, the key's and the
//! value's lengths, a checksum of all of it) and the key and value bytes.
//!
//! **Readers** never trust the table. They copy what they use under the seqlock (a sequence
//! word that changed, or is odd, means the copy is thrown away: after a few tries the lookup goes
//! to the sources), and check every offset and length against the table's bounds, the entry's
//! magic, serial, hash and checksum, and the key itself. An entry is used only if both of its
//! generations are current and it has not expired. Writers (one at a time: the caller holds the
//! writers' lock) make the sequence odd, change the table, and make it even again; a writer that
//! finds it odd (one died in the middle of a change) reinitialises the table.

use core::sync::atomic::{fence, AtomicU64, Ordering::{Acquire, Relaxed, Release}};

use crate::error::Result;
use crate::mem::TryVec;

/// The first word of a table: `"LIELLANC"`.
pub const MAGIC: u64 = u64::from_le_bytes(*b"LIELLANC");
/// The layout version this module reads and writes.
pub const LAYOUT: u64 = 1;
/// The largest table (8 MiB): the shared region's size cap.
pub const CAP_BYTES: usize = 8 << 20;
/// The largest table in words.
pub const CAP_WORDS: usize = CAP_BYTES / 8;
/// The smallest table in words (32 KiB): a smaller one is not a table.
pub const MIN_WORDS: usize = 4096;
/// The header's size in words.
pub const HEADER: usize = 80;
/// Slots probed per key.
pub const PROBE: usize = 8;
/// The words of an entry's header.
pub const ENTRY_HEAD: usize = 9;
/// The longest entry in words (64 KiB): a longer answer is not cached.
pub const MAX_ENTRY_WORDS: usize = 8192;
/// An entry's lifetime when it has none (answers of the `files` source: valid until a watched
/// file changes).
pub const FOREVER: u64 = u64::MAX;

const H_MAGIC: usize = 0;
const H_LAYOUT: usize = 1;
const H_TOTAL: usize = 2;
const H_OWNER: usize = 3;
const H_BOOT: usize = 4;
const H_ACCOUNT: usize = 6;
const H_SEQ: usize = 7;
const H_GEN: usize = 8;
const H_CATGEN: usize = 9;
const H_SLOTS: usize = 15;
const H_HEAD: usize = 16;
const H_SERIAL: usize = 17;
const H_FILES: usize = 20;
const H_STAMP_FILES: usize = 70;

const E_HEAD: usize = 0;
const E_SERIAL: usize = 1;
const E_HASH: usize = 2;
const E_GEN: usize = 3;
const E_CGEN: usize = 4;
const E_TIME: usize = 5;
const E_TTL: usize = 6;
const E_LENS: usize = 7;
const E_SUM: usize = 8;

const ENTRY_MAGIC: u64 = 0x4e43;
const SERIAL_BITS: u32 = 40;
const SERIAL_MASK: u64 = (1 << SERIAL_BITS) - 1;
const OFFSET_MASK: u64 = (1 << 24) - 1;

// ------------------------------------------------------------------------------ categories and files

/// What the cache holds answers for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    /// `hosts`: names and addresses (`gethostbyname*`, `gethostbyaddr`, `getaddrinfo`,
    /// `getnameinfo`).
    Hosts,
    /// `passwd` by name and by UID.
    Passwd,
    /// `group` by name and by GID, and group lists (`getgrouplist`, `initgroups`).
    Group,
    /// `services` by name and by port.
    Services,
    /// `protocols` by name and by number.
    Protocols,
    /// `networks` by name and by number.
    Networks,
}

/// The number of categories.
pub const CATEGORIES: usize = 6;

impl Category {
    /// Every category, in the order of [`Category::index`] (the native API's numbers).
    pub const ALL: [Category; CATEGORIES] = [Category::Hosts, Category::Passwd, Category::Group, Category::Services, Category::Protocols, Category::Networks];

    /// The category's number (0 to 5).
    pub fn index(self) -> usize {
        match self {
            Category::Hosts => 0,
            Category::Passwd => 1,
            Category::Group => 2,
            Category::Services => 3,
            Category::Protocols => 4,
            Category::Networks => 5,
        }
    }

    /// The category numbered `n`.
    pub fn from_index(n: usize) -> Option<Category> {
        Category::ALL.get(n).copied()
    }

    /// The category's bit in a mask.
    pub fn bit(self) -> u32 {
        1u32.checked_shl(u32::try_from(self.index()).unwrap_or(0)).unwrap_or(0)
    }

    /// The database file whose changes invalidate the category.
    pub fn file(self) -> File {
        match self {
            Category::Hosts => File::Hosts,
            Category::Passwd => File::Passwd,
            Category::Group => File::Group,
            Category::Services => File::Services,
            Category::Protocols => File::Protocols,
            Category::Networks => File::Networks,
        }
    }
}

/// Every category's bit.
pub const ALL_CATEGORIES: u32 = (1 << CATEGORIES) - 1;

/// The watched files (D-16.10): a change of the first four invalidates every category, a change
/// of a database file its category.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum File {
    /// `nsswitch.conf`.
    Nsswitch,
    /// `host.conf`.
    HostConf,
    /// `resolv.conf`.
    ResolvConf,
    /// `gai.conf`.
    GaiConf,
    /// `hosts`.
    Hosts,
    /// `passwd`.
    Passwd,
    /// `group`.
    Group,
    /// `services`.
    Services,
    /// `protocols`.
    Protocols,
    /// `networks`.
    Networks,
}

/// The number of watched files.
pub const FILES: usize = 10;

impl File {
    /// Every file, in the order of [`File::index`].
    pub const ALL: [File; FILES] =
        [File::Nsswitch, File::HostConf, File::ResolvConf, File::GaiConf, File::Hosts, File::Passwd, File::Group, File::Services, File::Protocols, File::Networks];

    /// The files whose change invalidates every category.
    pub const GLOBAL: [File; 4] = [File::Nsswitch, File::HostConf, File::ResolvConf, File::GaiConf];

    /// The file's position in the header's record.
    pub fn index(self) -> usize {
        File::ALL.iter().position(|&f| f == self).unwrap_or(0)
    }

    /// The category the file invalidates (`None`: every category).
    pub fn category(self) -> Option<Category> {
        Category::ALL.iter().copied().find(|c| c.file() == self)
    }
}

/// A file's identity as validation compares it: device and presence, inode, size, modification
/// and change times (D-16.10: an editor replacing a file by rename changes the inode even when
/// the times collide). All zero: never validated.
pub type FileId = [u64; 5];

/// The identity of a file `stat` described.
pub fn file_id(dev: u64, ino: u64, size: i64, mtime: (i64, i64), ctime: (i64, i64)) -> FileId {
    let ns = |(s, n): (i64, i64)| u64::from_ne_bytes(s.wrapping_mul(1_000_000_000).wrapping_add(n).to_ne_bytes());
    [dev.wrapping_shl(1) | 1, ino, u64::from_ne_bytes(size.to_ne_bytes()), ns(mtime), ns(ctime)]
}

/// The identity of a file that does not exist (or cannot be examined).
pub const MISSING: FileId = [2, 0, 0, 0, 0];

// ------------------------------------------------------------------------------ hashing

/// A 64-bit hash of `bytes` with `seed` (keys, checksums and fingerprints; not cryptographic:
/// the table's writers are processes of the same user, D-16.10).
pub fn hash(seed: u64, bytes: &[u8]) -> u64 {
    let mut h = seed ^ 0x243f_6a88_85a3_08d3 ^ u64::try_from(bytes.len()).unwrap_or(0).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let mut chunks = bytes.chunks_exact(8);
    for c in &mut chunks {
        let mut w = [0u8; 8];
        w.copy_from_slice(c);
        h = mix(h, u64::from_le_bytes(w));
    }
    let rest = chunks.remainder();
    if !rest.is_empty() {
        let mut w = [0u8; 8];
        if let Some(d) = w.get_mut(..rest.len()) {
            d.copy_from_slice(rest);
        }
        h = mix(h, u64::from_le_bytes(w));
    }
    finish(h)
}

/// One word into a running hash.
pub fn mix(h: u64, w: u64) -> u64 {
    (h ^ w).wrapping_mul(0x9e37_79b9_7f4a_7c15).rotate_left(29) ^ (h >> 31)
}

/// The final avalanche (SplitMix64's).
pub fn finish(h: u64) -> u64 {
    let h = (h ^ (h >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    let h = (h ^ (h >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    h ^ (h >> 31)
}

// ------------------------------------------------------------------------------ the table

/// The validity stamp of a table: whose it is and since when (D-16.10).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Stamp {
    /// The owner's user ID (the process's own view of its effective UID).
    pub owner: u64,
    /// The boot ID.
    pub boot: [u64; 2],
    /// A hash of the owner's user-database name and UID ([`account`]).
    pub account: u64,
}

/// The account part of a stamp: `name` (empty when the UID has no entry) and `uid`.
pub fn account(name: &[u8], uid: u32) -> u64 {
    hash(u64::from(uid), name)
}

/// The slots and the arena of a table of `total` words: `(slots, arena start, arena words)`.
pub fn geometry(total: usize) -> Option<(usize, usize, usize)> {
    if !(MIN_WORDS..=CAP_WORDS).contains(&total) {
        return None;
    }
    // About one slot per 16 words, a power of two.
    let target = total >> 4;
    let mut slots = 1usize;
    while slots.checked_mul(2).is_some_and(|s| s <= target) {
        slots = slots.checked_mul(2)?;
    }
    let start = HEADER.checked_add(slots)?;
    let len = total.checked_sub(start)?;
    (len >= MAX_ENTRY_WORDS.min(total >> 1)).then_some((slots, start, len))
}

/// A probe's outcome.
#[derive(Debug, PartialEq, Eq)]
pub enum Found {
    /// A current entry: its value was copied out.
    Hit,
    /// No current entry for the key.
    Miss,
    /// The table changed under every try, or is inconsistent: ask the sources.
    Busy,
}

/// What an entry was stored under.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Stored {
    /// The category.
    pub category: Category,
    /// The generations (every category's, the category's) the answer was obtained under.
    pub gens: (u64, u64),
    /// When it was stored (seconds of the real-time clock).
    pub time: u64,
    /// How long it is valid, in seconds ([`FOREVER`]: until a generation moves).
    pub ttl: u64,
}

/// A table: words another process may change at any time.
#[derive(Clone, Copy, Debug)]
pub struct Table<'a> {
    w: &'a [AtomicU64],
    slots: usize,
    arena: usize,
    arena_len: usize,
}

impl<'a> Table<'a> {
    /// The table over `words` (`None`: a size no table has).
    pub fn new(words: &'a [AtomicU64]) -> Option<Table<'a>> {
        let (slots, arena, arena_len) = geometry(words.len())?;
        Some(Table { w: words, slots, arena, arena_len })
    }

    fn ld(&self, i: usize) -> u64 {
        self.w.get(i).map_or(0, |a| a.load(Relaxed))
    }

    fn st(&self, i: usize, v: u64) {
        if let Some(a) = self.w.get(i) {
            a.store(v, Relaxed);
        }
    }

    /// `f` run under the seqlock: its result if no writer changed the table meanwhile (`None`
    /// when every try overlapped a change, or one is unfinished).
    fn read<T>(&self, mut f: impl FnMut() -> T) -> Option<T> {
        for _ in 0..4 {
            let s1 = self.w.get(H_SEQ)?.load(Acquire);
            if s1 & 1 != 0 {
                core::hint::spin_loop();
                continue;
            }
            let v = f();
            fence(Acquire);
            if self.ld(H_SEQ) == s1 {
                return Some(v);
            }
        }
        None
    }

    /// Whether the header is a table of this layout, size and `stamp` (the words a reader
    /// checks before trusting anything else).
    pub fn valid(&self, stamp: &Stamp) -> bool {
        self.read(|| {
            self.ld(H_MAGIC) == MAGIC
                && self.ld(H_LAYOUT) == LAYOUT
                && usize::try_from(self.ld(H_TOTAL)).ok() == Some(self.w.len())
                && self.ld(H_OWNER) == stamp.owner
                && [self.ld(H_BOOT), self.ld(H_BOOT.saturating_add(1))] == stamp.boot
                && self.ld(H_ACCOUNT) == stamp.account
                && usize::try_from(self.ld(H_SLOTS)).ok() == Some(self.slots)
                && usize::try_from(self.ld(H_HEAD)).is_ok_and(|h| h <= self.arena_len)
        })
        .unwrap_or(false)
    }

    // -------------------------------------------------------------- writers

    /// Start a change (the caller holds the writers' lock and calls [`Table::end`] after):
    /// whether the table was consistent. `false`: a writer died in the middle of a change, so
    /// the caller reinitialises it.
    pub fn begin(&self) -> bool {
        let s = self.ld(H_SEQ);
        if s & 1 != 0 {
            return false;
        }
        self.st(H_SEQ, s.wrapping_add(1));
        fence(Release);
        true
    }

    /// End a change.
    pub fn end(&self) {
        let s = self.ld(H_SEQ);
        if let Some(a) = self.w.get(H_SEQ) {
            a.store(if s & 1 != 0 { s.wrapping_add(1) } else { s.wrapping_add(2) }, Release);
        }
    }

    /// Make the table empty, with `stamp` (inside a change). Every generation moves, so that a
    /// process that remembers the old ones sees the change; the file record is cleared, so the
    /// next validation records the files. `zero`: clear the slots (a fresh mapping is zero).
    pub fn init(&self, stamp: &Stamp, zero: bool) {
        let next = self.ld(H_GEN).wrapping_add(1);
        let serial = self.ld(H_SERIAL) & SERIAL_MASK;
        for i in 0..HEADER {
            if i != H_SEQ {
                self.st(i, 0);
            }
        }
        self.st(H_MAGIC, MAGIC);
        self.st(H_LAYOUT, LAYOUT);
        self.st(H_TOTAL, u64::try_from(self.w.len()).unwrap_or(0));
        self.st(H_OWNER, stamp.owner);
        self.st(H_BOOT, stamp.boot[0]);
        self.st(H_BOOT.saturating_add(1), stamp.boot[1]);
        self.st(H_ACCOUNT, stamp.account);
        self.st(H_GEN, next);
        for c in 0..CATEGORIES {
            self.st(H_CATGEN.saturating_add(c), next);
        }
        self.st(H_SLOTS, u64::try_from(self.slots).unwrap_or(0));
        self.st(H_SERIAL, serial);
        if zero {
            for i in HEADER..self.arena {
                self.st(i, 0);
            }
        }
    }

    /// Move the generation of every category (`all`) and of the categories in `mask` (inside a
    /// change).
    pub fn bump(&self, all: bool, mask: u32) {
        if all {
            self.st(H_GEN, self.ld(H_GEN).wrapping_add(1));
        }
        for c in Category::ALL {
            if mask & c.bit() != 0 {
                let i = H_CATGEN.saturating_add(c.index());
                self.st(i, self.ld(i).wrapping_add(1));
            }
        }
    }

    /// The account stamp the header holds (`None`: busy).
    pub fn account(&self) -> Option<u64> {
        self.read(|| self.ld(H_ACCOUNT))
    }

    /// The identities of `passwd` and `nsswitch.conf` the stamp was verified against (`None`:
    /// busy).
    pub fn stamp_files(&self) -> Option<[FileId; 2]> {
        self.read(|| core::array::from_fn(|f| core::array::from_fn(|k| self.ld(H_STAMP_FILES.saturating_add(f.saturating_mul(5)).saturating_add(k)))))
    }

    /// Record the identities the stamp was verified against (inside a change).
    pub fn set_stamp_files(&self, ids: &[FileId; 2]) {
        for (f, id) in ids.iter().enumerate() {
            for (k, &v) in id.iter().enumerate() {
                self.st(H_STAMP_FILES.saturating_add(f.saturating_mul(5)).saturating_add(k), v);
            }
        }
    }

    /// `file`'s recorded identity, read inside a change (the writer's own view; readers use
    /// [`Table::file`]).
    pub fn file_in_change(&self, file: File) -> FileId {
        let base = H_FILES.saturating_add(file.index().saturating_mul(5));
        core::array::from_fn(|k| self.ld(base.saturating_add(k)))
    }

    /// Record `id` as `file`'s validated identity (inside a change).
    pub fn set_file(&self, file: File, id: &FileId) {
        let base = H_FILES.saturating_add(file.index().saturating_mul(5));
        for (k, &v) in id.iter().enumerate() {
            self.st(base.saturating_add(k), v);
        }
    }

    // -------------------------------------------------------------- readers

    /// The generations of every category and of `c` (`None`: the table is busy).
    pub fn gens(&self, c: Category) -> Option<(u64, u64)> {
        self.read(|| (self.ld(H_GEN), self.ld(H_CATGEN.saturating_add(c.index()))))
    }

    /// Every category's generation, then each category's (`None`: busy).
    pub fn all_gens(&self) -> Option<[u64; CATEGORIES + 1]> {
        self.read(|| core::array::from_fn(|i| if i == 0 { self.ld(H_GEN) } else { self.ld(H_CATGEN.saturating_add(i).saturating_sub(1)) }))
    }

    /// The validated identity of `file` (`None`: busy).
    pub fn file(&self, file: File) -> Option<FileId> {
        let base = H_FILES.saturating_add(file.index().saturating_mul(5));
        self.read(|| core::array::from_fn(|k| self.ld(base.saturating_add(k))))
    }

    fn slot_index(&self, hash: u64, k: usize) -> usize {
        let mask = self.slots.saturating_sub(1);
        let h = usize::try_from(hash & u64::try_from(mask).unwrap_or(0)).unwrap_or(0);
        HEADER.saturating_add(h.wrapping_add(k) & mask)
    }

    /// The arena offset of a slot's entry, if the slot points to a whole entry of its serial.
    fn entry_at(&self, slot: u64) -> Option<(usize, usize, u64)> {
        if slot == 0 {
            return None;
        }
        let serial = slot >> 24;
        let off = usize::try_from(slot & OFFSET_MASK).ok()?;
        let base = self.arena.checked_add(off)?;
        let head = self.ld(base.checked_add(E_HEAD)?);
        let words = usize::try_from(head & 0xffff_ffff).ok()?;
        let fits = off.checked_add(words).is_some_and(|e| e <= self.arena_len);
        if head >> 48 != ENTRY_MAGIC || !(ENTRY_HEAD..=MAX_ENTRY_WORDS).contains(&words) || !fits {
            return None;
        }
        (self.ld(base.checked_add(E_SERIAL)?) & SERIAL_MASK == serial).then_some((base, words, serial))
    }

    /// Look `key` (hash `hash`) up: on a [`Found::Hit`] its value is in `out` and the entry was
    /// stored under `gens` and is valid `now` (seconds of the real-time clock). `buf` is scratch
    /// space for the entry's words.
    pub fn get(&self, key: &[u8], hash: u64, gens: (u64, u64), now: u64, buf: &mut TryVec<u64>, out: &mut TryVec<u8>) -> Result<Found> {
        let mut err = None;
        let r = self.read(|| {
            // The newest entry for the key among the probed slots.
            let mut best: Option<(u64, Stored)> = None;
            out.clear();
            for k in 0..PROBE {
                let Some((base, words, serial)) = self.entry_at(self.ld(self.slot_index(hash, k))) else { continue };
                if self.ld(base.saturating_add(E_HASH)) != hash || best.is_some_and(|(s, _)| s > serial) {
                    continue;
                }
                buf.clear();
                for i in 0..words {
                    if let Err(e) = buf.push(self.ld(base.saturating_add(i))) {
                        err = Some(e);
                        return None;
                    }
                }
                let Some((stored, vlen)) = check(buf, key) else { continue };
                if let Err(e) = value_of(buf, key.len(), vlen, out) {
                    err = Some(e);
                    return None;
                }
                best = Some((serial, stored));
            }
            best.map(|(_, s)| s)
        });
        if let Some(e) = err {
            out.clear();
            return Err(e);
        }
        Ok(match r {
            None => {
                out.clear();
                Found::Busy
            }
            Some(Some(s)) if s.gens == gens && live(s.time, s.ttl, now) => Found::Hit,
            Some(_) => {
                out.clear();
                Found::Miss
            }
        })
    }

    /// Store `value` for `key` (inside a change): the oldest entries give way when the arena is
    /// full. `false` when the entry is too long to be cached.
    pub fn put(&self, key: &[u8], hash: u64, s: &Stored, value: &[u8]) -> bool {
        let payload = key.len().saturating_add(value.len());
        let Some(words) = payload.div_ceil(8).checked_add(ENTRY_HEAD) else { return false };
        if words > MAX_ENTRY_WORDS || words > self.arena_len || u32::try_from(key.len()).is_err() || u32::try_from(value.len()).is_err() {
            return false;
        }
        let mut head = usize::try_from(self.ld(H_HEAD)).unwrap_or(usize::MAX);
        if head.checked_add(words).is_none_or(|e| e > self.arena_len) {
            head = 0;
        }
        let serial = (self.ld(H_SERIAL).wrapping_add(1)) & SERIAL_MASK;
        let serial = if serial == 0 { 1 } else { serial };
        let base = self.arena.saturating_add(head);
        let lens = u64::try_from(key.len()).unwrap_or(0) << 32 | u64::try_from(value.len()).unwrap_or(0);
        let headw = ENTRY_MAGIC << 48 | u64::try_from(s.category.index()).unwrap_or(0) << 40 | u64::try_from(words).unwrap_or(0);
        let fields = [headw, serial, hash, s.gens.0, s.gens.1, s.time, s.ttl, lens];
        let mut sum = 0u64;
        for (i, &v) in fields.iter().enumerate() {
            self.st(base.saturating_add(i), v);
            sum = mix(sum, v);
        }
        let mut at = base.saturating_add(ENTRY_HEAD);
        for w in Payload::new(key, value) {
            self.st(at, w);
            sum = mix(sum, w);
            at = at.saturating_add(1);
        }
        self.st(base.saturating_add(E_SUM), finish(sum));
        self.st(H_HEAD, u64::try_from(head.saturating_add(words)).unwrap_or(0));
        self.st(H_SERIAL, serial);
        // The slot: one holding this hash, else a free or stale one, else the oldest.
        let mut choice = None;
        let mut oldest: Option<(u64, usize)> = None;
        for k in 0..PROBE {
            let i = self.slot_index(hash, k);
            match self.entry_at(self.ld(i)) {
                None => {
                    choice = Some(i);
                    break;
                }
                Some((b, _, es)) => {
                    let current = self.ld(b.saturating_add(E_GEN)) == self.ld(H_GEN)
                        && self.ld(b.saturating_add(E_CGEN)) == self.ld(H_CATGEN.saturating_add(usize::try_from(self.ld(b.saturating_add(E_HEAD)) >> 40 & 0xff).unwrap_or(0)));
                    if self.ld(b.saturating_add(E_HASH)) == hash || !current {
                        choice = Some(i);
                        break;
                    }
                    if oldest.is_none_or(|(o, _)| es < o) {
                        oldest = Some((es, i));
                    }
                }
            }
        }
        let i = choice.or(oldest.map(|(_, i)| i)).unwrap_or_else(|| self.slot_index(hash, 0));
        self.st(i, serial << 24 | u64::try_from(head).unwrap_or(0) & OFFSET_MASK);
        true
    }
}

/// Whether an entry stored at `time` with lifetime `ttl` is valid at `now` (a clock that went
/// back before `time` makes it invalid).
fn live(time: u64, ttl: u64, now: u64) -> bool {
    ttl == FOREVER || (now >= time && now.saturating_sub(time) < ttl)
}

/// The words of an entry's key and value, packed.
struct Payload<'a> {
    bytes: core::iter::Chain<core::slice::Iter<'a, u8>, core::slice::Iter<'a, u8>>,
}

impl<'a> Payload<'a> {
    fn new(key: &'a [u8], value: &'a [u8]) -> Payload<'a> {
        Payload { bytes: key.iter().chain(value.iter()) }
    }
}

impl Iterator for Payload<'_> {
    type Item = u64;
    fn next(&mut self) -> Option<u64> {
        let mut w = [0u8; 8];
        let mut any = false;
        for b in w.iter_mut() {
            match self.bytes.next() {
                Some(&x) => {
                    *b = x;
                    any = true;
                }
                None => break,
            }
        }
        any.then(|| u64::from_le_bytes(w))
    }
}

/// Byte `i` of an entry's payload words.
fn byte(payload: &[u64], i: usize) -> Option<u8> {
    payload.get(i >> 3).and_then(|w| w.to_le_bytes().get(i & 7).copied())
}

/// Check a copied entry (`e`, its words) against `key`: what it was stored under and the value's
/// length. `None`: not a whole, intact entry for `key`.
fn check(e: &[u64], key: &[u8]) -> Option<(Stored, usize)> {
    let head = *e.get(E_HEAD)?;
    let category = Category::from_index(usize::try_from(head >> 40 & 0xff).ok()?)?;
    let lens = *e.get(E_LENS)?;
    let (klen, vlen) = (usize::try_from(lens >> 32).ok()?, usize::try_from(lens & 0xffff_ffff).ok()?);
    let payload = e.get(ENTRY_HEAD..)?;
    if klen != key.len() || klen.checked_add(vlen)?.div_ceil(8) != payload.len() {
        return None;
    }
    let mut sum = 0u64;
    for (i, &v) in e.iter().enumerate() {
        if i != E_SUM {
            sum = mix(sum, v);
        }
    }
    if finish(sum) != *e.get(E_SUM)? {
        return None;
    }
    for (i, &k) in key.iter().enumerate() {
        if byte(payload, i)? != k {
            return None;
        }
    }
    Some((Stored { category, gens: (*e.get(E_GEN)?, *e.get(E_CGEN)?), time: *e.get(E_TIME)?, ttl: *e.get(E_TTL)? }, vlen))
}

/// Copy the value of a checked entry (key `klen` bytes, value `vlen`) into `out`.
fn value_of(e: &[u64], klen: usize, vlen: usize, out: &mut TryVec<u8>) -> Result<()> {
    out.clear();
    out.reserve(vlen)?;
    let payload = e.get(ENTRY_HEAD..).unwrap_or(&[]);
    for i in 0..vlen {
        out.push(byte(payload, klen.saturating_add(i)).unwrap_or(0))?;
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions)]
mod tests {
    use super::*;

    fn words(n: usize) -> alloc::vec::Vec<AtomicU64> {
        (0..n).map(|_| AtomicU64::new(0)).collect()
    }

    const STAMP: Stamp = Stamp { owner: 1000, boot: [1, 2], account: 3 };

    fn stored(c: Category, gens: (u64, u64), time: u64, ttl: u64) -> Stored {
        Stored { category: c, gens, time, ttl }
    }

    fn get(t: &Table<'_>, key: &[u8], gens: (u64, u64), now: u64) -> (Found, alloc::vec::Vec<u8>) {
        let (mut buf, mut out) = (TryVec::new(), TryVec::new());
        let f = t.get(key, hash(0, key), gens, now, &mut buf, &mut out).unwrap();
        (f, out.as_slice().to_vec())
    }

    fn put(t: &Table<'_>, key: &[u8], s: &Stored, v: &[u8]) -> bool {
        assert!(t.begin());
        let r = t.put(key, hash(0, key), s, v);
        t.end();
        r
    }

    fn fresh(w: &[AtomicU64]) -> Table<'_> {
        let t = Table::new(w).unwrap();
        assert!(!t.valid(&STAMP));
        assert!(t.begin());
        t.init(&STAMP, true);
        t.end();
        assert!(t.valid(&STAMP));
        t
    }

    #[test]
    fn geometry_bounds() {
        assert!(geometry(MIN_WORDS - 1).is_none());
        assert!(geometry(CAP_WORDS + 1).is_none());
        let (slots, start, len) = geometry(CAP_WORDS).unwrap();
        assert_eq!(slots, 65536);
        assert_eq!(start + len, CAP_WORDS);
        let (slots, _, len) = geometry(MIN_WORDS).unwrap();
        assert!(slots.is_power_of_two() && len >= MIN_WORDS / 2);
    }

    #[test]
    fn stores_and_finds() {
        let w = words(MIN_WORDS * 4);
        let t = fresh(&w);
        let g = t.gens(Category::Passwd).unwrap();
        assert!(put(&t, b"k1", &stored(Category::Passwd, g, 10, FOREVER), b"root:x:0:0::/root:/bin/sh"));
        assert_eq!(get(&t, b"k1", g, 99), (Found::Hit, b"root:x:0:0::/root:/bin/sh".to_vec()));
        assert_eq!(get(&t, b"k2", g, 99).0, Found::Miss);
        // A newer answer for the same key replaces the older one.
        assert!(put(&t, b"k1", &stored(Category::Passwd, g, 11, FOREVER), b"v2"));
        assert_eq!(get(&t, b"k1", g, 99), (Found::Hit, b"v2".to_vec()));
        // Generations: any move makes the entry stale.
        assert!(t.begin());
        t.bump(false, Category::Passwd.bit());
        t.end();
        let g2 = t.gens(Category::Passwd).unwrap();
        assert_ne!(g, g2);
        assert_eq!(get(&t, b"k1", g2, 99).0, Found::Miss);
        assert_eq!(t.gens(Category::Hosts).unwrap(), (g.0, g.0));
        assert!(t.begin());
        t.bump(true, 0);
        t.end();
        assert_ne!(t.gens(Category::Hosts).unwrap().0, g.0);
        // Files.
        assert_eq!(t.file(File::Passwd).unwrap(), [0; 5]);
        assert!(t.begin());
        t.set_file(File::Passwd, &file_id(1, 2, 3, (4, 5), (6, 7)));
        t.end();
        assert_eq!(t.file(File::Passwd).unwrap(), file_id(1, 2, 3, (4, 5), (6, 7)));
        assert_eq!(t.file(File::Group).unwrap(), [0; 5]);
    }

    #[test]
    fn lifetimes() {
        let w = words(MIN_WORDS * 4);
        let t = fresh(&w);
        let g = t.gens(Category::Hosts).unwrap();
        assert!(put(&t, b"ttl", &stored(Category::Hosts, g, 1000, 60), b"a"));
        assert_eq!(get(&t, b"ttl", g, 1000).0, Found::Hit);
        assert_eq!(get(&t, b"ttl", g, 1059).0, Found::Hit);
        assert_eq!(get(&t, b"ttl", g, 1060).0, Found::Miss);
        // A clock that went back before the store.
        assert_eq!(get(&t, b"ttl", g, 999).0, Found::Miss);
        assert!(put(&t, b"zero", &stored(Category::Hosts, g, 1000, 0), b"a"));
        assert_eq!(get(&t, b"zero", g, 1000).0, Found::Miss);
    }

    #[test]
    fn evicts_the_oldest_and_keeps_its_size() {
        let w = words(MIN_WORDS);
        let t = fresh(&w);
        let g = t.gens(Category::Group).unwrap();
        let value = [7u8; 200];
        for i in 0..2000u32 {
            assert!(put(&t, &i.to_le_bytes(), &stored(Category::Group, g, 0, FOREVER), &value));
        }
        assert_eq!(w.len(), MIN_WORDS);
        assert_eq!(get(&t, &0u32.to_le_bytes(), g, 0).0, Found::Miss);
        assert_eq!(get(&t, &1999u32.to_le_bytes(), g, 0), (Found::Hit, value.to_vec()));
        // Too long to cache.
        assert!(!put(&t, b"big", &stored(Category::Group, g, 0, FOREVER), &[0u8; MAX_ENTRY_WORDS * 8]));
    }

    #[test]
    fn a_writer_that_died_leaves_it_busy_until_reinitialised() {
        let w = words(MIN_WORDS * 2);
        let t = fresh(&w);
        let g = t.gens(Category::Passwd).unwrap();
        assert!(put(&t, b"k", &stored(Category::Passwd, g, 0, FOREVER), b"v"));
        assert!(t.begin());
        // ... killed here: the sequence stays odd.
        assert_eq!(get(&t, b"k", g, 0).0, Found::Busy);
        assert!(t.gens(Category::Passwd).is_none());
        assert!(!t.valid(&STAMP));
        // The next writer finds it inconsistent and starts afresh.
        assert!(!t.begin());
        t.init(&STAMP, true);
        t.end();
        assert!(t.valid(&STAMP));
        let g2 = t.gens(Category::Passwd).unwrap();
        assert_ne!(g, g2);
        assert_eq!(get(&t, b"k", g2, 0).0, Found::Miss);
    }

    #[test]
    fn damage_is_never_an_answer() {
        let w = words(MIN_WORDS * 2);
        let t = fresh(&w);
        let g = t.gens(Category::Services).unwrap();
        assert!(put(&t, b"http/tcp", &stored(Category::Services, g, 0, FOREVER), b"http 80/tcp"));
        let (_, start, _) = geometry(w.len()).unwrap();
        // Find the entry and flip a payload bit: the checksum fails.
        let slot = (HEADER..start).find(|&i| w[i].load(Relaxed) != 0).unwrap();
        let off = (w[slot].load(Relaxed) & OFFSET_MASK) as usize;
        let at = start + off + ENTRY_HEAD + 1;
        w[at].fetch_xor(1 << 3, Relaxed);
        assert_eq!(get(&t, b"http/tcp", g, 0).0, Found::Miss);
        w[at].fetch_xor(1 << 3, Relaxed);
        assert_eq!(get(&t, b"http/tcp", g, 0).0, Found::Hit);
        // Slots pointing anywhere: past the arena, at a wrong serial, at the arena's start.
        let keep = w[slot].load(Relaxed);
        for bad in [OFFSET_MASK, keep ^ (1 << 30), (keep & !OFFSET_MASK) | 3, u64::MAX, 1] {
            w[slot].store(bad, Relaxed);
            assert_eq!(get(&t, b"http/tcp", g, 0).0, Found::Miss, "{bad:#x}");
        }
        w[slot].store(keep, Relaxed);
        // An entry length beyond the arena.
        let head = start + off;
        let h = w[head].load(Relaxed);
        w[head].store((h & !0xffff_ffff) | 0xffff_fff0, Relaxed);
        assert_eq!(get(&t, b"http/tcp", g, 0).0, Found::Miss);
        w[head].store(h, Relaxed);
        assert_eq!(get(&t, b"http/tcp", g, 0).0, Found::Hit);
        // Random words everywhere: no panic, no answer, not a valid header.
        let mut x = 0x1234_5678_9abc_def0u64;
        for round in 0..20 {
            for a in w.iter() {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                a.store(if round % 2 == 0 { x } else { x & 0xff }, Relaxed);
            }
            w[H_SEQ].store(0, Relaxed);
            assert!(!t.valid(&STAMP));
            for k in [&b"http/tcp"[..], b"", b"x"] {
                let (f, _) = get(&t, k, g, 0);
                assert_ne!(f, Found::Hit);
            }
            // A writer reinitialises it.
            let _ = t.begin();
            t.init(&STAMP, true);
            t.end();
            assert!(t.valid(&STAMP));
            assert_eq!(get(&t, b"http/tcp", t.gens(Category::Services).unwrap(), 0).0, Found::Miss);
        }
    }

    #[test]
    fn stamps_and_sizes_must_match() {
        let w = words(MIN_WORDS * 2);
        let t = fresh(&w);
        assert!(!t.valid(&Stamp { owner: 1001, ..STAMP }));
        assert!(!t.valid(&Stamp { boot: [9, 9], ..STAMP }));
        assert!(!t.valid(&Stamp { account: account(b"bob", 1000), ..STAMP }));
        // The same words seen as a shorter table (a truncated file): not a table of that size.
        let short = Table::new(&w[..MIN_WORDS]).unwrap();
        assert!(!short.valid(&STAMP));
        assert!(Table::new(&w[..MIN_WORDS - 8]).is_none());
        assert_ne!(account(b"alice", 2000), account(b"bob", 2000));
        assert_ne!(account(b"alice", 2000), account(b"alice", 2001));
    }
}
