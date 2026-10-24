//! The allocator algorithm (IP-7; spec §3.1; D-3.2, D-3.3, D-3.7, D-3.10–D-3.13). It manages
//! *addresses* of memory obtained through [`Pages`] (backed by `Sys::mmap`, never `brk`) and
//! keeps every piece of its state in atomic words: in this struct, or in metadata pages the
//! boundary views as `&[AtomicU64]`. It never reads or writes the blocks it hands out;
//! zero-filling and copying are callbacks the boundary supplies.
//!
//! **Shards (D-3.10).** The heap is split into [`SHARDS`] shards, one per NUMA node. Each has
//! its own lock, its free slabs, its current segment and its pool of parked caches, on its own
//! cache lines. A shard's segments are mapped with its node preferred.
//!
//! **Small blocks** (up to [`MAX_SMALL`] bytes) come from 256 KiB slabs, each serving one of
//! 44 size classes, inside 16 MiB segments aligned to 16 MiB. A segment starts with a header
//! (its shard) and the slab records; the slabs follow. There is no per-block header (D-3.3).
//! `free` finds the segment from the address through a lock-free, insert-only registry
//! (segments are never unmapped, only their pages discarded), the slab by offset, and the block
//! by its bit.
//!
//! **Caches (D-3.11).** Every thread allocates through its *cache*, found through
//! [`Pages::cache_slot`] (the thread block's word 0x90). A cache owns slabs: per class a
//! current slab and a list of partial ones. The owner allocates and frees in its own slabs
//! with `Relaxed` loads and stores of words only it writes (the *local* bitmap, the use count,
//! the lists): no lock and no atomic read-modify-write. Any other thread frees into a slab it
//! does not own through the slab's *remote* bitmap and remote word, and the free that finds
//! the slab not queued pushes it onto the owner's pending list, which the owner takes whole.
//! The owner collects pending slabs when its current slab runs out. A slab emptied while not
//! current goes back to its shard. Caches are never unmapped: a thread's cache is parked in
//! its shard's pool when the thread exits and taken over whole by a later thread, so a cache
//! identifier always names a live cache.
//!
//! **Large blocks** get a mapping each, recorded in an open-addressing table. Lookup is
//! lock-free (insert-only probing: a live block's probe chain never holds an empty slot);
//! inserting and removing take the large lock, and the `mmap`/`munmap` calls happen outside it.
//! A freed large block is kept in its thread's cache (D-3.13), marked cached in the table.
//!
//! **Secure blocks** ([`Heap::alloc_secure`], D-X.11) are large blocks marked secure in the
//! large table: each has a mapping of its own, whatever its size. Releasing one (by
//! [`Heap::free`], or by [`Heap::resize`] moving it) zeroes its whole mapping through
//! [`Pages::wipe`] while the releasing call alone owns it, and then unmaps it: a secure block
//! is never kept in a cache, never handed to another block, never moved by `remap`, so no other
//! path (thread exit, `trim`, a `fork` child's parking) ever holds its memory. Small blocks are
//! never secure, so their paths carry no test at all.
//!
//! Lock order: a shard's lock, then the segment-registry lock (`grow`). The large-block lock
//! is never held together with another.

#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)] // bounded index arithmetic on fixed tables

use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering, Ordering::{AcqRel, Acquire, Relaxed, Release}};

use liella_kernel::sys::Sys;

use crate::sync::RawMutex;

/// Memory for the heap: page-granular mappings, never `brk` (D-3.7), and the calling thread's
/// identity as the heap needs it.
pub trait Pages {
    /// A new zero-filled mapping of `len` bytes at an address that is a multiple of `align` (a
    /// power of two, at least [`PAGE`]), its pages preferably on NUMA node `node` (`None`: the
    /// kernel's default policy, first touch). `None` without memory.
    fn map(&self, len: usize, align: usize, node: Option<u32>) -> Option<usize>;
    /// Unmap a whole mapping returned by [`Pages::map`] or [`Pages::remap`].
    fn unmap(&self, addr: usize, len: usize);
    /// Give the pages of `[addr, addr + len)` (inside one mapping) back to the kernel; they read
    /// as zero afterwards.
    fn discard(&self, addr: usize, len: usize);
    /// Resize a whole mapping, possibly moving it; `None` if it cannot (the mapping is then
    /// unchanged). The node preference moves with it.
    fn remap(&self, addr: usize, old_len: usize, new_len: usize) -> Option<usize>;
    /// Zero `[addr, addr + len)`, the whole mapping of a secure block being released, so that
    /// the stores cannot be elided (D-X.11: the boundary passes the bytes through the
    /// explicit-zeroing barrier). The mapping is still in place and owned by the releasing
    /// call alone; it is unmapped right after. Required: a memory source that cannot zero must
    /// say so here, not by default.
    fn wipe(&self, addr: usize, len: usize);
    /// Keep the pages of `[addr, addr + len)` (a new secure block's mapping) out of core dumps
    /// (`MADV_DONTDUMP`); best effort, nothing changes if the kernel refuses.
    fn exclude_from_dumps(&self, _addr: usize, _len: usize) {}
    /// The metadata words at `addr` (inside a mapping the heap made and never hands out).
    fn words(&self, addr: usize, n: usize) -> &[AtomicU64];
    /// The shard serving the calling thread: its NUMA node when that is known cheaply (D-3.12).
    /// Asked only on slow paths (a cache or a slab is being chosen).
    fn shard(&self) -> usize {
        0
    }
    /// The calling thread's cache word (the address of its cache, 0 for none yet). Each
    /// thread has its own; the heap is the only writer.
    fn cache_slot(&self) -> &AtomicUsize;
    /// `cache_slot().load(Relaxed)`: the fast paths' read of the cache word. An implementation
    /// may read it more directly (the boundary: one thread-pointer-relative load, HEAP-CUTS2 C4).
    #[inline(always)]
    fn cache_word(&self) -> usize {
        self.cache_slot().load(Relaxed)
    }
    /// The process's address-space limit in bytes (`RLIMIT_AS`), `None` for none. Asked once,
    /// before the heap's first reservation: its registries are sized to fit (T14).
    fn address_limit(&self) -> Option<usize> {
        None
    }
    /// Called by the heap at `at`, an interleaving point of a cross-thread protocol, so that
    /// a test can run another "thread's" heap calls there and force the interleaving
    /// deterministically. The boundary keeps this no-op default (inlined away).
    #[inline(always)]
    fn race_point(&self, _at: RacePoint) {}
}

/// Interleaving points of the heap's cross-thread protocols ([`Pages::race_point`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RacePoint {
    /// A remote free has pinned its slab (in-flight count raised) and not yet set its remote bit.
    RemotePinned,
}

/// The fundamental alignment (`alignof(max_align_t)`).
pub const ALIGN: usize = 16;
/// The page size.
pub const PAGE: usize = 4096;
/// The largest size served from slabs.
pub const MAX_SMALL: usize = 65536;
/// Largest request (2.29+: requests above `PTRDIFF_MAX` fail).
pub const MAX_REQUEST: usize = isize::MAX.unsigned_abs();
/// Shards: one per NUMA node; node `n` uses shard `n % SHARDS`.
pub const SHARDS: usize = 64;
/// Segment size and alignment.
pub const SEG: usize = 1 << SEG_SHIFT;
/// Segments the registry can hold at most: 16 TiB of small-block memory (fewer under an
/// address-space limit, [`Pages::address_limit`]).
pub const MAX_SEGS: usize = 1 << 20;
/// Large-block table slots of generation 0 at most (the table grows by generations, T14).
pub const LARGE_CAP: usize = 1 << 20;
/// Caches (threads alive at once, plus parked caches) the heap can hold.
pub const MAX_CACHES: usize = 1 << 20;
/// Large blocks a thread keeps for reuse (D-3.13).
pub const LARGE_KEEP: usize = 4;
/// Bytes of large blocks a thread keeps for reuse (D-3.13).
pub const LARGE_KEEP_BYTES: usize = 64 << 20;

const SEG_SHIFT: u32 = 24;
const SLAB_SHIFT: u32 = 18;
const SLAB: usize = 1 << SLAB_SHIFT;
const SLAB64: u64 = 1 << SLAB_SHIFT;
const BITMAP_WORDS: usize = SLAB / ALIGN / 64;
// Slab head (one cache line per slab): word 0 class + 1 (bits 0..8), home shard (8..24),
// shard-list link (32..64, slab id + 1); word 1 the owner cache (0: its shard holds it); word 2
// blocks in use (owner only; kept for slabs that are not current, since a current slab's bitmap
// says it already); word 3 the remote word (bits 0..63 remote frees in flight, bit 63 QUEUED: the
// slab is on its owner's pending list or about to be; non-zero pins the slab); word 4 the pending-list link (slab id + 1); word 5
// the owner's partial-list links (next 0..31, prev 31..62, listed 63); word 6 the descriptor (0
// while no cache owns the slab): class + 1 (bits 0..8), the class's division magic (8..45), its
// size (45..62). `free` learns all three from one load.
const R_HEAD: usize = 0;
const R_OWNER: usize = 1;
const R_USED: usize = 2;
const R_REMOTE: usize = 3;
const R_PENDING: usize = 4;
const R_LINKS: usize = 5;
const R_DESC: usize = 6;
const HEAD_WORDS: usize = 8;
/// R_REMOTE bit 63: the slab is queued for its owner's collection (D-3.15 amendment).
const QUEUED: u64 = 1 << 63;
const SLABS: usize = 63;
// Segment metadata, dense (D-3.15): line 0 the shard + 1 (word 0); line 1 + i slab i's head; from
// LOCALS the slabs' local bitmaps, 2 KiB each, then their remote bitmaps (REMOTE_OFF after the
// local one); slab data from META. The heads of all slabs share one page and a bitmap is written
// only as far as its class uses it, so a process using a dozen classes touches a few metadata
// pages instead of one record page per class.
// Line 0 word 1 (SEG_REMOTE): non-zero once any block of the segment was freed remotely, set
// before the remote bit. While it reads zero every remote bitmap of the segment is all zero, so
// the double-free and liveness checks skip the remote word: a process that never frees across
// threads never reads (and faults in) the remote bitmap pages.
const SEG_REMOTE: usize = 8;
const HEADS: usize = 64;
const LOCALS: usize = PAGE;
const BITMAP_BYTES: usize = BITMAP_WORDS * 8;
const REMOTE_OFF: usize = SLABS * BITMAP_BYTES;
const META: usize = LOCALS + 2 * REMOTE_OFF;
const _: () = assert!(HEADS + SLABS * HEAD_WORDS * 8 <= LOCALS && META % PAGE == 0);
const _: () = assert!(META + SLABS * SLAB <= SEG);
const IDX_BITS: u32 = 21;
const IDX_MASK: u64 = (1 << IDX_BITS) - 1;
const _: () = assert!(MAX_SEGS.is_power_of_two() && MAX_SEGS < (1 << IDX_BITS) && LARGE_CAP.is_power_of_two() && MAX_CACHES.is_power_of_two());
const TOMBSTONE: u64 = 1;
/// Generations a registry can grow by (T14): generation 0 holds the capacity chosen at the first
/// reservation, generation g ≥ 1 twice the one before it; never moved, never unmapped.
const GENS: usize = 24;

/// The generation and offset of entry `i` of an index registry whose generation 0 holds `c`
/// entries (generation g ≥ 1 holds `c << (g - 1)`, starting at entry `c << (g - 1)`).
fn gen_of(i: usize, c: usize) -> (usize, usize) {
    if i < c {
        return (0, i);
    }
    let q = i / c;
    let g = usize::try_from(usize::BITS - 1 - q.leading_zeros()).unwrap_or(0) + 1;
    (g, i - (c << (g - 1)))
}

/// The entries of generation `g` of an index registry whose generation 0 holds `c`.
fn gen_len(g: usize, c: usize) -> usize {
    if g == 0 { c } else { c << (g - 1) }
}
/// Length-word flag: the large block sits in a thread's cache (D-3.13).
const CACHED: u64 = 1 << 63;
/// Length-word flag: the large block is secure (D-X.11): wiped and unmapped when released,
/// never cached or remapped. Lengths stay below `2^47`, so the bit is free.
const SECURE: u64 = 1 << 62;
/// The length in a length word.
fn len_of(lw: u64) -> usize {
    uz(lw & !(CACHED | SECURE))
}
const LINK_MASK: u64 = (1 << 31) - 1;
const LISTED: u64 = 1 << 63;

// Cache words: identifier, pending-list head (slab id + 1), the remote flag, home shard, pool link (cache identifier), kept large blocks, parked flag; then per class eight words (current
// slab id + 1, its record address, its first block address, the bitmap word to take from,
// the partial-list head, the bitmap groups that may have a free block, the bitmap length, one
// unused), then the
// large-block entries (address, length), then the slab hints.
const C_ID: usize = 0;
const C_PENDING: usize = 1;
/// Non-zero once a block of a slab this cache owns may have been freed remotely (HEAP-HINT,
/// D-3.16): set by `free_remote` for the slab's owner after the remote counter pins the slab and
/// before the remote bit, and by `adopt` when the slab's segment has had a remote free. Never
/// cleared. While it reads zero no slab of the cache has a remote bit, so the free fast paths
/// skip the remote check with one compare against the cache (word 2 held the latest slab before
/// HEAP-CUTS2 S-C).
const C_REMOTE: usize = 2;
const C_SHARD: usize = 3;
const C_POOL: usize = 4;
const C_LARGE_N: usize = 5;
const C_PARKED: usize = 6;
/// The segment of the cache's latest slab: base | (registry index + 1). A free into it skips
/// the registry lookup.
const C_SEG: usize = 7;
const C_CLASS: usize = 8;
const K_CUR: usize = 0;
const K_REC: usize = 1;
const K_BASE: usize = 2;
const K_HINT: usize = 3;
const K_PARTIAL: usize = 4;
/// The groups of the current slab's bitmap that may have a free block (HEAP-HINT-2, D-3.16): bit
/// `g` covers words `4g..4g+4`; a clear bit means every word of its group is full. `K_HINT` is
/// where the fast path takes from (the word of the latest free, or of the latest search); when
/// that word is full, both paths take the lowest free block of the lowest set group.
const K_FREE: usize = 5;
/// The current slab's bitmap word count (the words past it hold no blocks and are never read).
const K_END: usize = 6;
/// Bitmap words per `K_FREE` bit: 64 bits cover a whole bitmap.
const GROUP_SHIFT: u32 = 2;
const _: () = assert!(BITMAP_WORDS >> GROUP_SHIFT <= 64);
/// A copy of the `K_HINT` word's value, written by the fast malloc and free (PERF-HEAP-PAIR-IVB-2).
/// Only a shortcut: the fast malloc uses it only while it equals the bitmap word, so a stale copy
/// (another path wrote the word or moved `K_HINT`) costs one pass through the plain path.
const K_HINTV: usize = 7;
/// Eight words (one cache line) per class: the index is a shift, and a class's words never
/// straddle two lines.
const CLASS_WORDS: usize = 8;
const C_LARGE: usize = C_CLASS + 44 * CLASS_WORDS;
const CACHE_WORDS: usize = 384;
/// The free fast path's slab hints (HEAP-CUTS2 S-C): a small table, indexed by the slab's address
/// bits above `SLAB_SHIFT`, of slabs that are the current slab of their class: base | class, 0
/// for none. Every allocation from a current slab records it; `refill` removes a class's entry
/// before its slab stops being current, so an entry always names a current slab and a free
/// inside it needs no lookup. (One word, the latest slab only, missed every free of a block of
/// another class than the latest allocation's.)
const C_SLABS: usize = C_LARGE + 2 * LARGE_KEEP;
const SLAB_HINTS: usize = 16;
/// The largest size `resize`'s fused path and growth forecast serve (REALLOC-FUSE).
pub const FORECAST_CAP: usize = 1024;
/// The smallest block whose growth by `realloc` the general path forecasts up to
/// [`MAX_SMALL`] (HEAP-REUSE).
pub const MID_FORECAST: usize = 2048;
/// How much larger than the request [`Heap::forecast_mid`] makes a mid-size block that grows
/// (REALLOC-GROW).
pub const MID_GROWTH: usize = 4;
// The fused path reads `SMALL_CLASS` up to the cap and masks the class with 31.
const _: () = assert!(FORECAST_CAP >> 4 == SMALL_CLASS.len() - 1 && CLASSES[31] >= FORECAST_CAP);
const _: () = assert!(C_SLABS + SLAB_HINTS <= CACHE_WORDS && SLAB_HINTS.is_power_of_two());
const CACHE_BYTES: usize = CACHE_WORDS * 8;
const CACHE_CHUNK: usize = 64 << 10;

/// The size classes: 16-byte steps to 128, then four per doubling up to 64 KiB.
pub const CLASSES: [usize; 44] = [
    16, 32, 48, 64, 80, 96, 112, 128,
    160, 192, 224, 256, 320, 384, 448, 512, 640, 768, 896, 1024,
    1280, 1536, 1792, 2048, 2560, 3072, 3584, 4096, 5120, 6144, 7168, 8192,
    10240, 12288, 14336, 16384, 20480, 24576, 28672, 32768, 40960, 49152, 57344, 65536,
];

/// `floor(2^40 / size) + 1` per class: `(rel * m) >> 40` is `rel / size` exactly for every
/// offset in a slab (`rel < 2^18`, `size <= 2^16`: the error stays below `2^34 / 2^40`).
const MAGIC: [u64; 44] = {
    let mut m = [0u64; 44];
    let mut i = 0;
    while i < 44 {
        // A const context: `as` is the only conversion there; every class fits in 17 bits.
        #[allow(clippy::as_conversions)]
        let size = CLASSES[i] as u64;
        m[i] = (1u64 << 40) / size + 1;
        i += 1;
    }
    m
};

/// The free fast path's view of a class (HEAP-CUTS C5): its block size, `MAGIC` and blocks per
/// slab side by side, padded to 64 classes so that a class index taken from a cache word is
/// reduced by a mask instead of a division and needs no bounds check. A padding entry is all
/// zero: no offset passes `q * size == off && q < blocks` with it, so a corrupt index can only
/// send the free to the slow path.
const FREE_VIEW: [[u64; 3]; 64] = {
    let mut t = [[0u64; 3]; 64];
    let mut i = 0;
    while i < 44 {
        // A const context: `as` is the only conversion there; both fit in 64 bits.
        #[allow(clippy::as_conversions)]
        {
            t[i] = [CLASSES[i] as u64, MAGIC[i], (SLAB / CLASSES[i]) as u64];
        }
        i += 1;
    }
    t
};
const _: () = assert!(CLASSES.len() <= 64);

/// A block handed out.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Block {
    /// Its address.
    pub addr: usize,
    /// Usable bytes (at least the request).
    pub usable: usize,
    /// True if the memory is known to read as zero (a fresh mapping).
    pub zeroed: bool,
}

/// A misuse the caller made, which C leaves undefined and the boundary turns into an abort.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Misuse {
    /// Not the address of a block this heap handed out.
    InvalidPointer,
    /// The block is already free.
    DoubleFree,
}

/// Statistics in implementation-neutral terms (D-3.4).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Stats {
    /// Bytes in live blocks (usable sizes).
    pub in_use: usize,
    /// Bytes mapped for small blocks (segments).
    pub small_mapped: usize,
    /// Bytes mapped for large blocks.
    pub large_mapped: usize,
    /// Live large blocks.
    pub large_blocks: usize,
    /// Bytes in empty slabs that [`Heap::trim`] could release (or released already: an empty
    /// slab keeps its address range).
    pub releasable: usize,
    /// Those empty slabs.
    pub empty_slabs: usize,
    /// Bytes in live small blocks (part of `in_use`).
    pub small_in_use: usize,
    /// The most bytes mapped at once (small and large blocks together).
    pub peak_mapped: usize,
    /// The most large blocks mapped at once.
    pub peak_large_blocks: usize,
    /// The most bytes mapped for large blocks at once.
    pub peak_large_mapped: usize,
}

/// The segment registry's lookup state: read by every `free` (`find_seg`) and written only
/// when a segment or a generation is added, so it has cache lines of its own (128 bytes, the
/// adjacent-line prefetch pair): nothing written per operation shares them (a precaution taken
/// with the generations of milestone 6; `threads16_64` measured the same either way, within its
/// wide run-to-run spread).
#[repr(align(128))]
struct SegRegistry {
    /// Generation 0's capacity (0 until chosen; T14).
    cap: AtomicUsize,
    /// Published generations.
    ngens: AtomicUsize,
    /// Segment index → base address, by generation ([`gen_of`]).
    bases: [AtomicUsize; GENS],
    /// Base >> 24 → index (open addressing, insert-only), one table per generation of `bases`
    /// (twice its entries) holding that generation's segments.
    hash: [AtomicUsize; GENS],
}

/// One shard's state, alone on its cache lines.
#[repr(align(64))]
struct Shard {
    lock: RawMutex,
    /// Empty slabs this shard holds (slab id + 1; linked through record word 0).
    empty: AtomicU64,
    /// How many.
    empties: AtomicUsize,
    /// The segment new slabs come from (registry index + 1; 0 = none).
    seg: AtomicUsize,
    /// The next never-used slab of `seg`.
    next_slab: AtomicUsize,
    /// Parked caches (cache identifier; linked through the cache's pool word).
    pool: AtomicU64,
}

impl Shard {
    const fn new() -> Shard {
        Shard {
            lock: RawMutex::new(),
            empty: AtomicU64::new(0),
            empties: AtomicUsize::new(0),
            seg: AtomicUsize::new(0),
            next_slab: AtomicUsize::new(0),
            pool: AtomicU64::new(0),
        }
    }
}

/// The heap. All state is atomic; [`Heap::new`] is `const`, so it can be a `static`.
pub struct Heap {
    shards: [Shard; SHARDS],
    /// Whether segments are placed on their shard's node (more than one node exists).
    numa: AtomicBool,
    /// Serialises segment and cache creation (the registries are read without it).
    grow: RawMutex,
    nsegs: AtomicUsize,
    /// The segment registry (its generation 0's capacity is `seg.cap`).
    seg: SegRegistry,
    /// Caches: identifier - 1 → address, by generation as `seg_bases`.
    cache_tab: [AtomicUsize; GENS],
    cache_ngens: AtomicUsize,
    /// The other registries' generation-0 capacities (powers of two; 0 until chosen, then
    /// fixed; T14): caches, large blocks. The registries grow by generations when an
    /// allocation needs more and the address-space limit (asked again then) allows it.
    caches: AtomicUsize,
    large_cap: AtomicUsize,
    /// No segments at all: the address-space limit is below four segments, so a segment would
    /// take most of it; every block then gets a mapping of its own (T14).
    no_segments: AtomicBool,
    ncaches: AtomicUsize,
    /// Unused room for caches in the current chunk: (address, bytes left).
    cache_room: AtomicUsize,
    cache_left: AtomicUsize,
    large: RawMutex,
    /// (address, length) pairs, one hash table per generation (generation g has
    /// `large_cap << g` slots); a block is entered in the newest; lookups search all.
    large_table: [AtomicUsize; GENS],
    /// Slots of each generation ever filled (tombstones included).
    large_used: [AtomicUsize; GENS],
    large_ngens: AtomicUsize,
    large_count: AtomicUsize,
    large_mapped: AtomicUsize,
    /// Bytes of large blocks kept in caches (mapped, not in use).
    large_kept: AtomicUsize,
    /// High-water marks for the statistics, raised where memory is mapped (slow paths only).
    peak_mapped: AtomicUsize,
    peak_large_blocks: AtomicUsize,
    peak_large_mapped: AtomicUsize,
}

impl Default for Heap {
    fn default() -> Heap {
        Heap::new()
    }
}

/// The class of every size up to 1024 bytes at the heap's own alignment, by `(size + 15) / 16`
/// (HEAP-CUTS C3): every class boundary up to there is a multiple of 16, so the smallest class
/// holding the rounded-up size holds the size. A table read instead of the arithmetic below.
const SMALL_CLASS: [u8; 65] = {
    let mut t = [0u8; 65];
    let mut i = 0;
    while i < 65 {
        let size = if i == 0 { 1 } else { i * 16 };
        let mut c = 0;
        while CLASSES[c] < size {
            c += 1;
        }
        // A const context: `as` is the only conversion there; a class index fits in a byte.
        #[allow(clippy::as_conversions, clippy::cast_possible_truncation)]
        {
            t[i] = c as u8;
        }
        i += 1;
    }
    t
};

/// The class serving `size` bytes at `align` (a power of two), or `None` for the large path.
#[inline]
pub fn class_for(size: usize, align: usize) -> Option<usize> {
    if align <= ALIGN && size <= 1024 {
        // `size <= 1024`: the index is at most 64.
        return SMALL_CLASS.get((size + 15) >> 4).map(|&c| usize::from(c));
    }
    let size = size.max(1);
    if size > MAX_SMALL || align > PAGE {
        return None;
    }
    let first = if size <= 128 {
        size.div_ceil(16) - 1
    } else {
        // Four classes per doubling: (size - 1) in [2^e, 2^(e+1)) picks one of four steps.
        let s = size - 1;
        let e = uz(u64::from(s.ilog2()));
        8 + (e - 7) * 4 + ((s >> (e - 2)) - 4)
    };
    // A power of two: a mask, not a division.
    let mask = align.max(ALIGN) - 1;
    (first..CLASSES.len()).find(|&c| CLASSES[c] & mask == 0)
}

/// Blocks per slab, per class (a table: a division here would cost more than the rest of the
/// fast path).
const BLOCKS: [usize; 44] = {
    let mut b = [0usize; 44];
    let mut i = 0;
    while i < 44 {
        b[i] = SLAB / CLASSES[i];
        i += 1;
    }
    b
};

fn blocks(class: usize) -> usize {
    BLOCKS[class]
}

fn rec(class: usize, shard: usize, next: u64) -> u64 {
    (w64(class) + 1) | (w64(shard) << 8) | (next << 32)
}
fn rec_class(w: u64) -> Option<usize> {
    usize::try_from(w & 0xff).ok().and_then(|c| c.checked_sub(1))
}
fn rec_shard(w: u64) -> usize {
    uz((w >> 8) & 0xffff)
}
fn rec_next(w: u64) -> u64 {
    w >> 32
}
/// Record word 0 of an empty slab: its pages went back to the kernel ([`Heap::trim`]) since it
/// became empty. `release_slab` writes the word afresh, which clears it.
const DISCARDED: u64 = 1 << 24;
/// A `usize` as a metadata word (lossless on the 64-bit targets).
fn w64(x: usize) -> u64 {
    u64::try_from(x).unwrap_or(u64::MAX)
}
/// A metadata word as a `usize` (lossless on the 64-bit targets).
fn uz(x: u64) -> usize {
    usize::try_from(x).unwrap_or(usize::MAX)
}
fn seg_slot(key: u64, len: usize) -> usize {
    uz(key.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - IDX_BITS)) & (len - 1)
}
/// Bit `i % 64` of word `i / 64`.
fn bit(i: usize) -> (usize, u64) {
    (i / 64, 1u64 << (i % 64))
}

/// The cache word of the slab hint for the slab holding `addr` (`C_SLABS`).
#[inline(always)]
fn slab_hint(addr: usize) -> usize {
    C_SLABS + ((addr >> SLAB_SHIFT) & (SLAB_HINTS - 1))
}

/// Where a live block sits.
enum Loc {
    /// In a slab: (slab id, its record address, class, index in the slab).
    Small(u64, usize, usize, usize),
    /// Not in a segment: a large block, if any.
    Large,
}

/// A thread's cache: its words. Its identifier is read when needed, not on every fast-path
/// call (HEAP-CUTS C7).
/// The words are a fixed-size array (HEAP-CUTS2 C6), so an index known to be in range needs
/// no bounds check.
/// A slab record (D-3.15): its head line and its local and remote bitmaps, apart in the segment.
#[derive(Clone, Copy)]
struct Rec<'p> {
    head: &'p [AtomicU64],
    local: &'p [AtomicU64],
    remote: &'p [AtomicU64],
    /// The segment's SEG_REMOTE word.
    any: &'p AtomicU64,
}

impl Rec<'_> {
    /// Whether block bit `b` of word `j` is set in the remote bitmap (not read while the
    /// segment has had no remote free).
    #[inline(always)]
    fn remote_has(&self, j: usize, b: u64, o: Ordering) -> bool {
        self.any.load(o) != 0 && self.remote[j].load(o) & b != 0
    }
}

/// The bitmaps of a slab record as fixed-size arrays, for the fast paths (no bounds checks).
struct Bits<'p> {
    local: &'p [AtomicU64; BITMAP_WORDS],
    remote: &'p [AtomicU64; BITMAP_WORDS],
    any: &'p AtomicU64,
}

impl Bits<'_> {
    /// [`Rec::remote_has`] for the fast paths (relaxed: the owner's own check).
    #[inline(always)]
    fn remote_has(&self, j: usize, b: u64) -> bool {
        self.any.load(Relaxed) != 0 && self.remote[j].load(Relaxed) & b != 0
    }
}

struct Cache<'p> {
    w: &'p [AtomicU64; CACHE_WORDS],
}

impl Cache<'_> {
    /// The cache's identifier (constant from its creation on).
    fn id(&self) -> u64 {
        self.get(C_ID)
    }
    fn get(&self, i: usize) -> u64 {
        self.w[i].load(Relaxed)
    }
    fn set(&self, i: usize, v: u64) {
        self.w[i].store(v, Relaxed);
    }
    fn class(&self, c: usize, k: usize) -> u64 {
        self.get(C_CLASS + c * CLASS_WORDS + k)
    }
    fn set_class(&self, c: usize, k: usize, v: u64) {
        self.set(C_CLASS + c * CLASS_WORDS + k, v);
    }
    /// A block of class `c` was freed into bitmap word `j` of the current slab (D-3.16): the
    /// next allocation takes from that word (it has a free block now, and the block is likely
    /// still in cache), and its group may have a free block again. Moving only a search start
    /// made free/malloc/malloc patterns leave the fast path: frees into three words, then
    /// mallocs (gawk's regex sets, two slow mallocs per input line), found the hint word and the
    /// lowest word full while a higher one had the freed blocks.
    #[inline(always)]
    fn freed_into(&self, c: usize, j: usize) {
        self.set_class(c, K_HINT, w64(j));
        self.set_class(c, K_FREE, self.class(c, K_FREE) | 1u64 << ((j >> GROUP_SHIFT) & 63));
    }
    /// Every group of the current slab of class `c` may have a free block again (a new current
    /// slab, or collected remote frees): the search restarts at word 0, so the lowest free block
    /// wins and the slab stays compact.
    fn all_free(&self, c: usize) {
        self.set_class(c, K_HINT, 0);
        let groups = blocks(c).div_ceil(64).div_ceil(1 << GROUP_SHIFT);
        self.set_class(c, K_FREE, if groups >= 64 { u64::MAX } else { (1u64 << groups) - 1 });
    }
    /// The lowest bitmap word with a free block (and its value) of the current slab of class
    /// `c`, whose local bitmap is `local`: the lowest set `K_FREE` group first. Groups found full
    /// are cleared, so each set bit costs at most one scan of its words. `None`: the slab is full.
    /// The owner alone writes the local bitmap and `K_FREE`, so the scan needs no ordering.
    #[inline(always)]
    fn free_word(&self, c: usize, local: &[AtomicU64]) -> Option<(usize, u64)> {
        let mut m = self.class(c, K_FREE);
        let end = uz(self.class(c, K_END));
        while m != 0 {
            let mut j = uz(u64::from(m.trailing_zeros())) << GROUP_SHIFT;
            let stop = (j + (1 << GROUP_SHIFT)).min(end);
            while j < stop {
                let Some(w) = local.get(j) else { break };
                let v = w.load(Relaxed);
                if v != u64::MAX {
                    self.set_class(c, K_FREE, m);
                    return Some((j, v));
                }
                j += 1;
            }
            m &= m - 1;
        }
        self.set_class(c, K_FREE, 0);
        None
    }
    /// Whether a slab of this cache may have a remote bit ([`C_REMOTE`]).
    #[inline(always)]
    fn remote_any(&self) -> bool {
        self.get(C_REMOTE) != 0
    }
}

impl Heap {
    /// An empty heap (all zero).
    pub const fn new() -> Heap {
        Heap {
            shards: [const { Shard::new() }; SHARDS],
            numa: AtomicBool::new(false),
            grow: RawMutex::new(),
            nsegs: AtomicUsize::new(0),
            seg: SegRegistry {
                cap: AtomicUsize::new(0),
                ngens: AtomicUsize::new(0),
                bases: [const { AtomicUsize::new(0) }; GENS],
                hash: [const { AtomicUsize::new(0) }; GENS],
            },
            cache_tab: [const { AtomicUsize::new(0) }; GENS],
            cache_ngens: AtomicUsize::new(0),
            caches: AtomicUsize::new(0),
            large_cap: AtomicUsize::new(0),
            no_segments: AtomicBool::new(false),
            ncaches: AtomicUsize::new(0),
            cache_room: AtomicUsize::new(0),
            cache_left: AtomicUsize::new(0),
            large: RawMutex::new(),
            large_table: [const { AtomicUsize::new(0) }; GENS],
            large_used: [const { AtomicUsize::new(0) }; GENS],
            large_ngens: AtomicUsize::new(0),
            large_count: AtomicUsize::new(0),
            large_mapped: AtomicUsize::new(0),
            large_kept: AtomicUsize::new(0),
            peak_mapped: AtomicUsize::new(0),
            peak_large_blocks: AtomicUsize::new(0),
            peak_large_mapped: AtomicUsize::new(0),
        }
    }

    /// Take every lock of the heap in the lock order: the shards, the registries', the
    /// large-block table's. For `fork` (the child must not inherit a lock some other thread
    /// held) and the AS-safety harness (an allocation attempted meanwhile blocks).
    pub fn lock_all<S: Sys>(&self, sys: &S) {
        for sh in &self.shards {
            sh.lock.lock_raw(sys, crate::futex::Scope::Private);
        }
        self.grow.lock_raw(sys, crate::futex::Scope::Private);
        self.large.lock_raw(sys, crate::futex::Scope::Private);
    }

    /// Release what [`Heap::lock_all`] took, in reverse order.
    pub fn unlock_all<S: Sys>(&self, sys: &S) {
        self.large.unlock(sys, crate::futex::Scope::Private);
        self.grow.unlock(sys, crate::futex::Scope::Private);
        for sh in self.shards.iter().rev() {
            sh.lock.unlock(sys, crate::futex::Scope::Private);
        }
    }

    /// Place each shard's new memory on its own node (the runtime calls this when the machine
    /// has more than one NUMA node).
    pub fn set_numa(&self, on: bool) {
        self.numa.store(on, Relaxed);
    }

    fn node_for(&self, shard: usize) -> Option<u32> {
        if self.numa.load(Relaxed) { u32::try_from(shard).ok() } else { None }
    }

    /// Choose the registries' capacities once (T14): the maxima, or under an address-space limit
    /// `L` registries sized to it (segments for `L`, a cache per MiB of `L`, a large block per
    /// page of `L`), so that their reservations, which count against `L`, stay small.
    fn choose_capacities<P: Pages>(&self, pages: &P) {
        if self.seg.cap.load(Acquire) != 0 {
            return;
        }
        let limit = pages.address_limit();
        if limit.is_some_and(|l| l < 4 * SEG) {
            self.no_segments.store(true, Relaxed);
        }
        let (segs, caches, large) = match limit {
            Some(l) if l < (64 << 30) => (
                (l / SEG).max(1).next_power_of_two().clamp(2, MAX_SEGS),
                (l >> 20).max(1).next_power_of_two().clamp(64, MAX_CACHES),
                (l / PAGE).max(1).next_power_of_two().clamp(256, LARGE_CAP),
            ),
            _ => (MAX_SEGS, MAX_CACHES, LARGE_CAP),
        };
        let _ = self.caches.compare_exchange(0, caches, AcqRel, Acquire);
        let _ = self.large_cap.compare_exchange(0, large, AcqRel, Acquire);
        let _ = self.seg.cap.compare_exchange(0, segs, AcqRel, Acquire);
    }

    fn seg_base<P: Pages>(&self, pages: &P, idx: usize) -> usize {
        let c = self.seg.cap.load(Relaxed);
        let (g, o) = gen_of(idx, c);
        uz(pages.words(self.seg.bases[g].load(Acquire), gen_len(g, c))[o].load(Acquire))
    }

    /// After the address-space limit may have been raised (T14): segments again when it allows
    /// four. Asked when a registry needs a new generation (rare).
    fn recheck_limit<P: Pages>(&self, pages: &P) {
        if self.no_segments.load(Relaxed) && pages.address_limit().is_none_or(|l| l >= 4 * SEG) {
            self.no_segments.store(false, Relaxed);
        }
    }

    /// The address naming slab `slab`'s record: its local bitmap's.
    fn record_addr<P: Pages>(&self, pages: &P, slab: u64) -> usize {
        let (seg, i) = (uz(slab / w64(SLABS)), uz(slab % w64(SLABS)));
        self.seg_base(pages, seg) + LOCALS + i * BITMAP_BYTES
    }

    fn record<'p, P: Pages>(&self, pages: &'p P, slab: u64) -> Rec<'p> {
        Self::rec_at(pages, self.record_addr(pages, slab))
    }

    /// The record named by `a` (a [`Heap::record_addr`]): its head and both bitmaps.
    #[inline]
    fn rec_at<P: Pages>(pages: &P, a: usize) -> Rec<'_> {
        let seg = a & !(SEG - 1);
        let i = a.wrapping_sub(seg + LOCALS) / BITMAP_BYTES;
        Rec {
            head: pages.words(seg + HEADS + i * HEAD_WORDS * 8, HEAD_WORDS),
            local: pages.words(a, BITMAP_WORDS),
            remote: pages.words(a + REMOTE_OFF, BITMAP_WORDS),
            any: &pages.words(seg + SEG_REMOTE, 1)[0],
        }
    }

    fn slab_base<P: Pages>(&self, pages: &P, slab: u64) -> usize {
        let (seg, i) = (uz(slab / w64(SLABS)), uz(slab % w64(SLABS)));
        self.seg_base(pages, seg) + META + i * SLAB
    }

    // ------------------------------------------------------------------------- segments

    /// Map a new segment for `shard` and register it.
    fn new_segment<S: Sys, P: Pages>(&self, sys: &S, pages: &P, shard: usize) -> Option<usize> {
        let _g = self.grow.lock(sys);
        self.choose_capacities(pages);
        if self.no_segments.load(Relaxed) {
            return None;
        }
        let segs = self.seg.cap.load(Relaxed);
        let n = self.nsegs.load(Relaxed);
        let (g, o) = gen_of(n, segs);
        if n >= MAX_SEGS || g >= GENS {
            return None;
        }
        let len = gen_len(g, segs);
        if g >= self.seg.ngens.load(Relaxed) {
            if g > 0 {
                self.recheck_limit(pages);
            }
            let bases = pages.map(len * 8, PAGE, None)?;
            let Some(hash) = pages.map(len * 2 * 8, PAGE, None) else {
                pages.unmap(bases, len * 8);
                return None;
            };
            self.seg.bases[g].store(bases, Release);
            self.seg.hash[g].store(hash, Release);
            self.seg.ngens.store(g + 1, Release);
        }
        let base = pages.map(SEG, SEG, self.node_for(shard))?;
        pages.words(base, 1)[0].store(w64(shard) + 1, Release);
        pages.words(self.seg.bases[g].load(Relaxed), len)[o].store(w64(base), Release);
        let table = pages.words(self.seg.hash[g].load(Relaxed), len * 2);
        let key = w64(base >> SEG_SHIFT);
        let mut i = seg_slot(key, table.len());
        // Probe with a compare-exchange, a write access: a plain load on an untouched table page
        // maps the shared zero page and the store then faults a second time (FAULTS-1).
        let entry = (key << IDX_BITS) | (w64(n) + 1);
        while table[i].compare_exchange(0, entry, Release, Relaxed).is_err() {
            i = (i + 1) & (table.len() - 1);
        }
        self.nsegs.store(n + 1, Release);
        self.peak_mapped.fetch_max((n + 1) * SEG + self.large_mapped.load(Relaxed), Relaxed);
        Some(n)
    }

    /// The registry index and base of the segment containing `addr`. Lock-free.
    #[inline]
    fn find_seg<P: Pages>(&self, pages: &P, addr: usize) -> Option<(usize, usize)> {
        let key = w64(addr >> SEG_SHIFT);
        let c = self.seg.cap.load(Relaxed);
        for g in 0..self.seg.ngens.load(Acquire) {
            let table = pages.words(self.seg.hash[g].load(Acquire), gen_len(g, c) * 2);
            let mut i = seg_slot(key, table.len());
            for _ in 0..table.len() {
                let e = table[i].load(Acquire);
                if e == 0 {
                    break;
                }
                if e >> IDX_BITS == key {
                    return Some((uz((e & IDX_MASK) - 1), uz(key) << SEG_SHIFT));
                }
                i = (i + 1) & (table.len() - 1);
            }
        }
        None
    }

    /// An empty or never-used slab of `shard` (its lock held), mapping a new segment when the
    /// current one is spent.
    fn take_slab<S: Sys, P: Pages>(&self, sys: &S, pages: &P, shard: usize) -> Option<u64> {
        let sh = &self.shards[shard];
        if let Some(s) = sh.empty.load(Relaxed).checked_sub(1) {
            sh.empty.store(rec_next(self.record(pages, s).head[R_HEAD].load(Relaxed)), Relaxed);
            sh.empties.fetch_sub(1, Relaxed);
            return Some(s);
        }
        let (seg, next) = (sh.seg.load(Relaxed), sh.next_slab.load(Relaxed));
        if seg != 0 && next < SLABS {
            sh.next_slab.store(next + 1, Relaxed);
            return Some(w64((seg - 1) * SLABS + next));
        }
        let idx = self.new_segment(sys, pages, shard)?;
        sh.seg.store(idx + 1, Relaxed);
        sh.next_slab.store(1, Relaxed);
        Some(w64(idx * SLABS))
    }

    /// Give the empty slab `s` (no live block, nothing pending) back to its home shard.
    fn release_slab<S: Sys, P: Pages>(&self, sys: &S, pages: &P, s: u64) {
        let r = self.record(pages, s);
        let head = r.head[R_HEAD].load(Relaxed);
        // No live block and nothing pending: the only set local bits are the tail bits `adopt`
        // wrote. Clearing them keeps an unowned slab's bitmap all zero (D-3.15).
        if let Some(c) = rec_class(head) {
            let n = blocks(c);
            if n % 64 != 0 {
                r.local[n / 64].store(0, Relaxed);
            }
        }
        let home = rec_shard(head) % SHARDS;
        let sh = &self.shards[home];
        let _g = sh.lock.lock(sys);
        r.head[R_OWNER].store(0, Release);
        r.head[R_LINKS].store(0, Relaxed);
        self.descriptor(pages, s).store(0, Release);
        r.head[R_HEAD].store((w64(home) << 8) | (sh.empty.load(Relaxed) << 32), Release);
        sh.empty.store(s + 1, Relaxed);
        sh.empties.fetch_add(1, Relaxed);
    }

    // ------------------------------------------------------------------------- caches

    fn cache_addr<P: Pages>(&self, pages: &P, id: u64) -> usize {
        let c = self.caches.load(Relaxed);
        let Some(i) = uz(id).checked_sub(1).filter(|_| c != 0) else { return 0 };
        let (g, o) = gen_of(i, c);
        if g >= self.cache_ngens.load(Acquire) {
            return 0;
        }
        uz(pages.words(self.cache_tab[g].load(Acquire), gen_len(g, c))[o].load(Acquire))
    }

    #[inline]
    fn cache_at<'p, P: Pages>(&self, pages: &'p P, id: u64) -> Option<Cache<'p>> {
        let addr = self.cache_addr(pages, id);
        if addr == 0 {
            return None;
        }
        Self::cache_of(pages, addr)
    }

    /// The calling thread's cache, taking a parked one or making one on first use.
    #[inline]
    fn cache<'p, S: Sys, P: Pages>(&self, sys: &S, pages: &'p P) -> Option<Cache<'p>> {
        match pages.cache_slot().load(Relaxed) {
            0 => self.first_cache(sys, pages),
            addr => Self::cache_of(pages, addr),
        }
    }

    /// The cache at `addr` (a value of a thread's cache word).
    #[inline]
    fn cache_of<P: Pages>(pages: &P, addr: usize) -> Option<Cache<'_>> {
        // `words` returns exactly the words asked for; the check folds away when it is inlined.
        pages.words(addr, CACHE_WORDS).first_chunk().map(|w| Cache { w })
    }

    /// The bitmaps of the slab record at `addr`, as fixed-size arrays (C6, as [`Cache`]).
    #[inline(always)]
    fn record_words<P: Pages>(pages: &P, addr: usize) -> Option<Bits<'_>> {
        Some(Bits {
            local: pages.words(addr, BITMAP_WORDS).first_chunk()?,
            remote: pages.words(addr + REMOTE_OFF, BITMAP_WORDS).first_chunk()?,
            any: pages.words((addr & !(SEG - 1)) + SEG_REMOTE, 1).first()?,
        })
    }

    /// The calling thread's cache if it has one.
    fn own_cache<P: Pages>(pages: &P) -> Option<Cache<'_>> {
        match pages.cache_word() {
            0 => None,
            addr => Self::cache_of(pages, addr),
        }
    }

    #[cold]
    fn first_cache<'p, S: Sys, P: Pages>(&self, sys: &S, pages: &'p P) -> Option<Cache<'p>> {
        let shard = pages.shard() % SHARDS;
        let id = self.unpark(sys, pages, shard).or_else(|| self.new_cache(sys, pages, shard))?;
        let k = self.cache_at(pages, id)?;
        pages.cache_slot().store(self.cache_addr(pages, id), Relaxed);
        Some(k)
    }

    fn unpark<S: Sys, P: Pages>(&self, sys: &S, pages: &P, shard: usize) -> Option<u64> {
        let sh = &self.shards[shard];
        let _g = sh.lock.lock(sys);
        let id = sh.pool.load(Relaxed);
        let c = self.cache_at(pages, id)?;
        sh.pool.store(c.get(C_POOL), Relaxed);
        c.set(C_POOL, 0);
        c.set(C_PARKED, 0);
        Some(id)
    }

    fn new_cache<S: Sys, P: Pages>(&self, sys: &S, pages: &P, shard: usize) -> Option<u64> {
        let _g = self.grow.lock(sys);
        self.choose_capacities(pages);
        let cap = self.caches.load(Relaxed);
        let n = self.ncaches.load(Relaxed);
        let (g, o) = gen_of(n, cap);
        if n >= MAX_CACHES || g >= GENS {
            return None;
        }
        if g >= self.cache_ngens.load(Relaxed) {
            if g > 0 {
                self.recheck_limit(pages);
            }
            let tab = pages.map(gen_len(g, cap) * 8, PAGE, None)?;
            self.cache_tab[g].store(tab, Release);
            self.cache_ngens.store(g + 1, Release);
        }
        if self.cache_left.load(Relaxed) < CACHE_BYTES {
            let chunk = pages.map(CACHE_CHUNK, PAGE, self.node_for(shard))?;
            self.cache_room.store(chunk, Relaxed);
            self.cache_left.store(CACHE_CHUNK, Relaxed);
        }
        let addr = self.cache_room.load(Relaxed);
        self.cache_room.store(addr + CACHE_BYTES, Relaxed);
        self.cache_left.fetch_sub(CACHE_BYTES, Relaxed);
        let id = w64(n) + 1;
        let w = pages.words(addr, CACHE_WORDS);
        w[C_ID].store(id, Relaxed);
        w[C_SHARD].store(w64(shard), Relaxed);
        pages.words(self.cache_tab[g].load(Relaxed), gen_len(g, cap))[o].store(w64(addr), Release);
        self.ncaches.store(n + 1, Release);
        Some(id)
    }

    /// Park the calling thread's cache (its thread is exiting): its large blocks are unmapped,
    /// its slabs stay with it, and the next new thread on its shard takes it over (D-3.11).
    pub fn park<S: Sys, P: Pages>(&self, sys: &S, pages: &P) {
        if let Some(c) = Self::own_cache(pages) {
            pages.cache_slot().store(0, Relaxed);
            self.park_cache(sys, pages, &c);
        }
    }

    /// The identifier of the calling thread's cache (0 before its first allocation). Self-test
    /// only: it lets a test see which cache a thread took over (D-3.11).
    pub fn own_cache_id<P: Pages>(&self, pages: &P) -> u64 {
        Self::own_cache(pages).map_or(0, |c| c.id())
    }

    /// Write the identifiers of the caches parked right now (in any pool) into `out`; returns
    /// how many were found, counting those that did not fit. Self-test only.
    pub fn parked_ids<P: Pages>(&self, pages: &P, out: &mut [u64]) -> usize {
        let mut n = 0;
        for id in 1..=w64(self.ncaches.load(Acquire)) {
            if self.cache_at(pages, id).is_some_and(|c| c.get(C_PARKED) != 0) {
                if let Some(slot) = out.get_mut(n) {
                    *slot = id;
                }
                n += 1;
            }
        }
        n
    }

    /// Park every cache but `keep` (`fork`'s child: only the calling thread survives, D-18.6).
    /// The child is single-threaded, so the caches of the vanished threads are idle.
    pub fn park_others<S: Sys, P: Pages>(&self, sys: &S, pages: &P) {
        let keep = Self::own_cache(pages).map_or(0, |c| c.id());
        for i in 0..self.ncaches.load(Acquire) {
            let id = w64(i) + 1;
            if let Some(c) = self.cache_at(pages, id).filter(|c| id != keep && c.get(C_PARKED) == 0) {
                self.park_cache(sys, pages, &c);
            }
        }
    }

    fn park_cache<S: Sys, P: Pages>(&self, sys: &S, pages: &P, c: &Cache<'_>) {
        self.drop_large(sys, pages, c, 0);
        self.collect(sys, pages, c);
        let sh = &self.shards[uz(c.get(C_SHARD)) % SHARDS];
        let _g = sh.lock.lock(sys);
        c.set(C_POOL, sh.pool.load(Relaxed));
        c.set(C_PARKED, 1);
        sh.pool.store(c.id(), Relaxed);
    }

    // ------------------------------------------------------------------------- small blocks

    /// Make `s` the current slab of class `c` in cache `k`.
    fn make_current<P: Pages>(&self, pages: &P, k: &Cache<'_>, c: usize, s: u64) {
        k.set_class(c, K_CUR, s + 1);
        k.set_class(c, K_REC, w64(self.record_addr(pages, s)));
        k.set_class(c, K_BASE, w64(self.slab_base(pages, s)));
        k.set_class(c, K_END, w64(blocks(c).div_ceil(64)));
        k.all_free(c);
        let seg = uz(s / w64(SLABS));
        k.set(C_SEG, w64(self.seg_base(pages, seg) | (seg + 1)));
    }

    /// Adopt slab `s` (empty, from a shard) for class `c` of cache `k`.
    fn adopt<P: Pages>(&self, pages: &P, k: &Cache<'_>, c: usize, s: u64) {
        let r = self.record(pages, s);
        let n = blocks(c);
        // An unowned slab's local bitmap is all zero (`release_slab`), so only the word holding
        // the last block is written: its bits past the last block read as allocated, and the
        // words past it are never read (D-3.15). Untouched bitmap pages stay unfaulted.
        debug_assert!(r.local[..n.div_ceil(64)].iter().all(|w| w.load(Relaxed) == 0));
        if n % 64 != 0 {
            r.local[n / 64].store(u64::MAX << (n % 64), Relaxed);
        }
        // The first allocation reads word 0 before it sets a bit: written here first, its page
        // faults once (a write) instead of twice (zero page, then copy) (FAULTS-1).
        if n >= 64 {
            r.local[0].store(0, Relaxed);
        }
        r.head[R_USED].store(0, Relaxed);
        r.head[R_LINKS].store(0, Relaxed);
        r.head[R_OWNER].store(k.id(), Release);
        // A remote bit left from an earlier owner must stay visible to the free fast paths
        // (D-3.16): the segment's flag is set before any remote bit of its slabs.
        if r.any.load(Acquire) != 0 {
            k.set(C_REMOTE, 1);
        }
        // The home shard is the segment's, whichever shard handed the slab out.
        r.head[R_HEAD].store(rec(c, self.home_of(pages, s), 0), Release);
        self.descriptor(pages, s).store((w64(c) + 1) | (MAGIC[c] << 8) | (w64(CLASSES[c]) << 45), Release);
    }

    /// Slab `s`'s descriptor word in its segment's header.
    fn descriptor<'p, P: Pages>(&self, pages: &'p P, s: u64) -> &'p AtomicU64 {
        let (seg, i) = (uz(s / w64(SLABS)), uz(s % w64(SLABS)));
        &pages.words(self.seg_base(pages, seg) + HEADS + i * HEAD_WORDS * 8 + R_DESC * 8, 1)[0]
    }

    fn home_of<P: Pages>(&self, pages: &P, s: u64) -> usize {
        let base = self.seg_base(pages, uz(s / w64(SLABS)));
        uz(pages.words(base, 1)[0].load(Acquire)).saturating_sub(1)
    }

    /// Allocate a block of class `c` from cache `k`.
    fn alloc_small<S: Sys, P: Pages>(&self, sys: &S, pages: &P, k: &Cache<'_>, c: usize) -> Option<Block> {
        loop {
            if k.class(c, K_CUR) != 0 {
                let r = Self::rec_at(pages, uz(k.class(c, K_REC)));
                // The lowest free block wins, so the slab stays compact (D-3.16).
                if let Some((j, v)) = k.free_word(c, r.local) {
                    let b = (!v).trailing_zeros();
                    r.local[j].store(v | (1u64 << b), Relaxed);
                    k.set_class(c, K_HINT, w64(j));
                    let i = j * 64 + uz(u64::from(b));
                    k.set(slab_hint(uz(k.class(c, K_BASE))), k.class(c, K_BASE) | w64(c));
                    return Some(Block { addr: uz(k.class(c, K_BASE)) + i * CLASSES[c], usable: CLASSES[c], zeroed: false });
                }
            }
            self.refill(sys, pages, k, c)?;
        }
    }

    /// The current slab of class `c` is full (or absent): make another current. Pending
    /// slabs first, then the cache's partial slabs, then an empty slab from the shard.
    #[cold]
    fn refill<S: Sys, P: Pages>(&self, sys: &S, pages: &P, k: &Cache<'_>, c: usize) -> Option<()> {
        // A full current slab simply stops being current, with every block counted in its use
        // count (some may be pending remote frees, which stay counted there until collected); a
        // free will list it again.
        if k.class(c, K_CUR) != 0 {
            let r = Self::rec_at(pages, uz(k.class(c, K_REC)));
            r.head[R_USED].store(w64(Self::live_bits(r, c)), Relaxed);
        }
        for h in C_SLABS..C_SLABS + SLAB_HINTS {
            if k.get(h) != 0 && k.get(h) & (SLAB64 - 1) == w64(c) {
                k.set(h, 0);
            }
        }
        k.set_class(c, K_CUR, 0);
        self.collect(sys, pages, k);
        if let Some(s) = k.class(c, K_PARTIAL).checked_sub(1) {
            self.unlist(pages, k, c, s);
            self.make_current(pages, k, c, s);
            return Some(());
        }
        let shard = pages.shard() % SHARDS;
        let s = {
            let _g = self.shards[shard].lock.lock(sys);
            self.take_slab(sys, pages, shard)?
        };
        self.adopt(pages, k, c, s);
        self.make_current(pages, k, c, s);
        Some(())
    }

    /// Collect the remote frees of every pending slab of cache `k`.
    fn collect<S: Sys, P: Pages>(&self, sys: &S, pages: &P, k: &Cache<'_>) {
        let mut next = k.w[C_PENDING].swap(0, AcqRel);
        while let Some(s) = next.checked_sub(1) {
            let r = self.record(pages, s);
            next = r.head[R_PENDING].load(Acquire);
            // Dequeued before the scan (its link already read): a remote free whose bit this scan
            // misses sets QUEUED after its bit, finds it clear and queues the slab again; one
            // that set QUEUED before this clear set its bit before it too, and the scan sees it.
            r.head[R_REMOTE].fetch_and(!QUEUED, AcqRel);
            if r.head[R_OWNER].load(Relaxed) != k.id() {
                continue; // never expected: pushes go to the owner
            }
            let mut freed = 0u64;
            // Only the words holding blocks can have remote bits (D-3.15: untouched pages stay so).
            let words = rec_class(r.head[R_HEAD].load(Relaxed)).map_or(BITMAP_WORDS, |c| blocks(c).div_ceil(64).min(BITMAP_WORDS));
            for j in 0..words {
                let got = r.remote[j].load(Acquire);
                if got == 0 {
                    continue;
                }
                let local = r.local[j].load(Relaxed);
                // A bit set remotely for a block that is not live locally was a racing double
                // free; ignoring it keeps the counts right.
                let hit = got & local;
                r.local[j].store(local & !hit, Relaxed);
                // The local bits are cleared before the remote ones (a release): a reader of the
                // statistics that no longer sees a remote bit sees its local bit cleared, so a
                // collected block never reads as in use (`stats`). Bits set meanwhile stay for
                // the next collection (their free pushed the slab again).
                r.remote[j].fetch_and(!got, AcqRel);
                freed += u64::from(hit.count_ones());
            }
            let Some(c) = rec_class(r.head[R_HEAD].load(Relaxed)) else { continue };
            if k.class(c, K_CUR) == s + 1 {
                // Its bitmap is the count; every group may have a free block again.
                k.all_free(c);
                continue;
            }
            let used = r.head[R_USED].load(Relaxed) - freed.min(r.head[R_USED].load(Relaxed));
            r.head[R_USED].store(used, Relaxed);
            self.after_free(sys, pages, k, c, s, used);
        }
    }

    /// Bookkeeping after blocks of slab `s` (class `c`, owned by `k`) were freed, leaving `used`.
    fn after_free<S: Sys, P: Pages>(&self, sys: &S, pages: &P, k: &Cache<'_>, c: usize, s: u64, used: u64) {
        let r = self.record(pages, s);
        if k.class(c, K_CUR) == s + 1 {
            if used == 0 {
                k.all_free(c);
            }
            return;
        }
        let listed = r.head[R_LINKS].load(Relaxed) & LISTED != 0;
        if used == 0 && r.head[R_REMOTE].load(Acquire) == 0 {
            if listed {
                self.unlist(pages, k, c, s);
            }
            self.release_slab(sys, pages, s);
        } else if !listed {
            self.list(pages, k, c, s);
        }
    }

    fn list<P: Pages>(&self, pages: &P, k: &Cache<'_>, c: usize, s: u64) {
        let head = k.class(c, K_PARTIAL);
        self.record(pages, s).head[R_LINKS].store(LISTED | head, Relaxed);
        if let Some(h) = head.checked_sub(1) {
            let hr = self.record(pages, h);
            let l = hr.head[R_LINKS].load(Relaxed);
            hr.head[R_LINKS].store((l & !(LINK_MASK << 31)) | ((s + 1) << 31), Relaxed);
        }
        k.set_class(c, K_PARTIAL, s + 1);
    }

    fn unlist<P: Pages>(&self, pages: &P, k: &Cache<'_>, c: usize, s: u64) {
        let r = self.record(pages, s);
        let l = r.head[R_LINKS].load(Relaxed);
        let (next, prev) = (l & LINK_MASK, (l >> 31) & LINK_MASK);
        if let Some(p) = prev.checked_sub(1) {
            let pr = self.record(pages, p);
            let pl = pr.head[R_LINKS].load(Relaxed);
            pr.head[R_LINKS].store((pl & !LINK_MASK) | next, Relaxed);
        } else {
            k.set_class(c, K_PARTIAL, next);
        }
        if let Some(n) = next.checked_sub(1) {
            let nr = self.record(pages, n);
            let nl = nr.head[R_LINKS].load(Relaxed);
            nr.head[R_LINKS].store((nl & !(LINK_MASK << 31)) | (prev << 31), Relaxed);
        }
        r.head[R_LINKS].store(0, Relaxed);
    }

    // ------------------------------------------------------------------------- large blocks

    /// Generation `g` of the large table.
    fn large_gen<'p, P: Pages>(&self, pages: &'p P, g: usize) -> &'p [AtomicU64] {
        pages.words(self.large_table[g].load(Acquire), (self.large_cap.load(Relaxed) << g) * 2)
    }

    /// The generation a new large block is entered in (the large lock held): the newest, or a
    /// new one when the newest is three quarters full. Its index.
    fn large_insert_gen<P: Pages>(&self, pages: &P) -> Option<usize> {
        self.choose_capacities(pages);
        let cap = self.large_cap.load(Relaxed);
        let n = self.large_ngens.load(Relaxed);
        if n > 0 && self.large_used[n - 1].load(Relaxed) < (cap << (n - 1)) / 4 * 3 {
            return Some(n - 1);
        }
        if n >= GENS {
            return None;
        }
        if n > 0 {
            self.recheck_limit(pages);
        }
        let t = pages.map((cap << n) * 2 * 8, PAGE, None)?;
        self.large_table[n].store(t, Release);
        self.large_used[n].store(0, Relaxed);
        self.large_ngens.store(n + 1, Release);
        Some(n)
    }

    /// The table slot holding `addr`, or the first free slot for it.
    fn large_slot(t: &[AtomicU64], addr: usize, insert: bool) -> Option<usize> {
        let key = w64(addr);
        let cap = t.len() / 2;
        let mut i = uz((key >> 12).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 44) & (cap - 1);
        let mut tomb = None;
        for _ in 0..cap {
            let k = t[2 * i].load(Acquire);
            if k == key {
                return Some(i);
            }
            if k == 0 {
                return if insert { tomb.or(Some(i)) } else { None };
            }
            if k == TOMBSTONE && tomb.is_none() {
                tomb = Some(i);
            }
            i = (i + 1) & (cap - 1);
        }
        if insert { tomb } else { None }
    }

    /// The live (or cached) large block at `addr`: its table slot and length word. Lock-free.
    fn large_find<'p, P: Pages>(&self, pages: &'p P, addr: usize) -> Result<(&'p [AtomicU64], usize, u64), Misuse> {
        for g in (0..self.large_ngens.load(Acquire)).rev() {
            let t = self.large_gen(pages, g);
            if let Some(i) = Self::large_slot(t, addr, false) {
                return Ok((t, i, t[2 * i + 1].load(Acquire)));
            }
        }
        Err(Misuse::InvalidPointer)
    }

    fn alloc_large<S: Sys, P: Pages>(&self, sys: &S, pages: &P, k: Option<&Cache<'_>>, size: usize, align: usize) -> Option<Block> {
        let len = size.max(1).checked_next_multiple_of(PAGE)?;
        if let Some(b) = k.and_then(|k| self.reuse_large(pages, k, len, align)) {
            return Some(b);
        }
        let shard = k.map_or(0, |k| uz(k.get(C_SHARD)));
        self.map_large(sys, pages, len, align, shard, 0)
    }

    /// A new mapping of `len` bytes (a multiple of [`PAGE`]) at `align` for a large block of
    /// `shard`, recorded in the large table with the length-word `flags`.
    fn map_large<S: Sys, P: Pages>(&self, sys: &S, pages: &P, len: usize, align: usize, shard: usize, flags: u64) -> Option<Block> {
        let addr = pages.map(len, align.max(PAGE), self.node_for(shard))?;
        if flags & SECURE != 0 {
            pages.exclude_from_dumps(addr, len);
        }
        let recorded = {
            let _g = self.large.lock(sys);
            self.large_insert_gen(pages).and_then(|g| {
                let t = self.large_gen(pages, g);
                let i = Self::large_slot(t, addr, true)?;
                if t[2 * i].load(Relaxed) == 0 {
                    self.large_used[g].fetch_add(1, Relaxed);
                }
                t[2 * i + 1].store(w64(len) | flags, Relaxed);
                t[2 * i].store(w64(addr), Release);
                self.large_count.fetch_add(1, Relaxed);
                self.large_mapped.fetch_add(len, Relaxed);
                self.note_peaks();
                Some(())
            })
        };
        if recorded.is_none() {
            pages.unmap(addr, len);
            return None;
        }
        Some(Block { addr, usable: len, zeroed: true })
    }

    /// Raise the high-water marks to the current large-block counts (the large lock held).
    fn note_peaks(&self) {
        let (count, mapped) = (self.large_count.load(Relaxed), self.large_mapped.load(Relaxed));
        self.peak_large_blocks.fetch_max(count, Relaxed);
        self.peak_large_mapped.fetch_max(mapped, Relaxed);
        self.peak_mapped.fetch_max(mapped + self.nsegs.load(Acquire) * SEG, Relaxed);
    }

    /// A kept large block of at least `len` bytes (and at most twice that plus 1 MiB) at
    /// `align`, taken out of cache `k`.
    fn reuse_large<P: Pages>(&self, pages: &P, k: &Cache<'_>, len: usize, align: usize) -> Option<Block> {
        let n = uz(k.get(C_LARGE_N));
        let fit = (0..n).find(|&e| {
            let (a, l) = (uz(k.get(C_LARGE + 2 * e)), uz(k.get(C_LARGE + 2 * e + 1)));
            l >= len && l <= len.saturating_mul(2).saturating_add(1 << 20) && a % align.max(PAGE) == 0
        })?;
        let (addr, l) = (uz(k.get(C_LARGE + 2 * fit)), uz(k.get(C_LARGE + 2 * fit + 1)));
        for e in fit..n - 1 {
            k.set(C_LARGE + 2 * e, k.get(C_LARGE + 2 * e + 2));
            k.set(C_LARGE + 2 * e + 1, k.get(C_LARGE + 2 * e + 3));
        }
        k.set(C_LARGE_N, w64(n - 1));
        self.large_kept.fetch_sub(l, Relaxed);
        let (t, i, _) = self.large_find(pages, addr).ok()?;
        t[2 * i + 1].fetch_and(!CACHED, AcqRel);
        Some(Block { addr, usable: l, zeroed: false })
    }

    /// Unmap kept large blocks of cache `k` until at most `keep` remain.
    fn drop_large<S: Sys, P: Pages>(&self, sys: &S, pages: &P, k: &Cache<'_>, keep: usize) {
        while uz(k.get(C_LARGE_N)) > keep {
            let n = uz(k.get(C_LARGE_N));
            let (addr, len) = (uz(k.get(C_LARGE)), uz(k.get(C_LARGE + 1)));
            for e in 0..n - 1 {
                k.set(C_LARGE + 2 * e, k.get(C_LARGE + 2 * e + 2));
                k.set(C_LARGE + 2 * e + 1, k.get(C_LARGE + 2 * e + 3));
            }
            k.set(C_LARGE_N, w64(n - 1));
            self.large_kept.fetch_sub(len, Relaxed);
            self.unmap_large(sys, pages, addr, len);
        }
    }

    fn unmap_large<S: Sys, P: Pages>(&self, sys: &S, pages: &P, addr: usize, len: usize) {
        {
            let _g = self.large.lock(sys);
            if let Ok((t, i, _)) = self.large_find(pages, addr) {
                t[2 * i].store(TOMBSTONE, Release);
                self.large_count.fetch_sub(1, Relaxed);
                self.large_mapped.fetch_sub(len, Relaxed);
            }
        }
        pages.unmap(addr, len);
    }

    fn free_large<S: Sys, P: Pages>(&self, sys: &S, pages: &P, addr: usize) -> Result<(), Misuse> {
        let (t, i, lw) = self.large_find(pages, addr)?;
        if lw & CACHED != 0 || t[2 * i + 1].fetch_or(CACHED, AcqRel) & CACHED != 0 {
            return Err(Misuse::DoubleFree);
        }
        let len = len_of(lw);
        if lw & SECURE != 0 {
            // The CACHED bit just set makes this call the block's only owner: zeroed while
            // still mapped, then unmapped. Never kept, so nothing later releases it again.
            pages.wipe(addr, len);
            self.unmap_large(sys, pages, addr, len);
            return Ok(());
        }
        let kept: usize = match self.cache(sys, pages) {
            Some(k) => {
                let n = uz(k.get(C_LARGE_N));
                let bytes: usize = (0..n).map(|e| uz(k.get(C_LARGE + 2 * e + 1))).sum();
                if len <= LARGE_KEEP_BYTES {
                    // Make room: the oldest go first.
                    let mut keep = n.min(LARGE_KEEP - 1);
                    let mut total = bytes;
                    while keep > 0 && total + len > LARGE_KEEP_BYTES {
                        total -= uz(k.get(C_LARGE + 1));
                        keep -= 1;
                    }
                    if n > keep {
                        self.drop_large(sys, pages, &k, keep);
                    }
                    let n = uz(k.get(C_LARGE_N));
                    k.set(C_LARGE + 2 * n, w64(addr));
                    k.set(C_LARGE + 2 * n + 1, w64(len));
                    k.set(C_LARGE_N, w64(n + 1));
                    self.large_kept.fetch_add(len, Relaxed);
                    1
                } else {
                    0
                }
            }
            None => 0,
        };
        if kept == 0 {
            self.unmap_large(sys, pages, addr, len);
        }
        Ok(())
    }

    // ------------------------------------------------------------------------- the API

    /// Allocate `size` bytes aligned to `align` (a power of two; at least [`ALIGN`] is applied)
    /// through the calling thread's cache. `None` without memory or above `PTRDIFF_MAX`.
    /// Always inlined: the fast path is a few instructions, and the C entry points (which test
    /// the debugging word first) must keep it in their own body (P6).
    #[inline(always)]
    pub fn alloc<S: Sys, P: Pages>(&self, sys: &S, pages: &P, size: usize, align: usize) -> Option<Block> {
        if let Some(b) = Self::alloc_fast(pages, size, align) {
            return Some(b);
        }
        self.alloc_slow(sys, pages, size, align)
    }

    /// [`Heap::alloc`]'s fast path alone: a block from the current slab's hint word, or `None`
    /// with nothing changed (then [`Heap::alloc_slow`] decides). Separate (TARC-ALLOC) so that
    /// `malloc` can tail-call the slow path and its fast path needs no stack frame.
    #[inline(always)]
    pub fn alloc_fast<P: Pages>(pages: &P, size: usize, align: usize) -> Option<Block> {
        if align <= ALIGN && size <= FORECAST_CAP {
            // The common sizes (MALLOC-FAST): their classes are below 32 (asserted with
            // `FORECAST_CAP`), so with the mask the cache-word and class-table indexes of the
            // hint path need no bounds checks. The index is at most 64, inside `SMALL_CLASS`.
            let c = usize::from(SMALL_CLASS[(size + 15) >> 4]) & 31;
            if let Some(k) = Self::own_cache(pages) {
                if let Some(b) = Self::alloc_hint(pages, &k, c) {
                    return Some(b);
                }
            }
        } else if let Some(c) = class_for(size, align) {
            // The fast path: a free bit in the current slab's hint word.
            if let Some(k) = Self::own_cache(pages) {
                if let Some(b) = Self::alloc_hint(pages, &k, c) {
                    return Some(b);
                }
            }
        }
        None
    }

    /// The step after [`Heap::alloc_fast`] for a common size (`size <= FORECAST_CAP`, default
    /// alignment) whose hint word is full: the lowest free block of the current slab's groups
    /// that may have one (D-3.16), or `None` (the slab is full or absent: the slow path
    /// refills). Separate from the fast path (HEAP-HINT-3) so that `malloc`'s frameless fast
    /// path stays as it was: `malloc_general`, which the fast path already tail-calls, runs it
    /// first. Inlined into the slow path, the group search of gawk's free/malloc patterns cost
    /// the slow path's whole entry (cache lookup, class search) per miss.
    #[inline(always)]
    pub fn alloc_group<P: Pages>(pages: &P, size: usize) -> Option<Block> {
        if size > FORECAST_CAP {
            return None;
        }
        // As in `alloc_fast`: the class is below 32 and the index inside `SMALL_CLASS`.
        let c = usize::from(SMALL_CLASS[(size + 15) >> 4]) & 31;
        let k = Self::own_cache(pages)?;
        if k.class(c, K_CUR) == 0 {
            return None;
        }
        let r = Self::record_words(pages, uz(k.class(c, K_REC)))?;
        let (j, v) = k.free_word(c, r.local)?;
        let b = (!v).trailing_zeros();
        r.local[j].store(v | (1u64 << b), Relaxed);
        k.set_class(c, K_HINT, w64(j));
        let i = j * 64 + uz(u64::from(b));
        let base = k.class(c, K_BASE);
        k.set(slab_hint(uz(base)), base | w64(c));
        Some(Block { addr: uz(base) + i * CLASSES[c], usable: CLASSES[c], zeroed: false })
    }

    /// A block of class `c` from the word of the current slab that cache `k` searches first.
    /// Always inlined (HEAP-SMALL, 2026-10-09): as a call, the `Option<Block>` went back through
    /// memory and `malloc` re-tested its discriminant, about a tenth of a warm pair.
    #[inline(always)]
    fn alloc_hint<'p, P: Pages>(pages: &'p P, k: &Cache<'p>, c: usize) -> Option<Block> {
        if k.class(c, K_CUR) == 0 {
            return None;
        }
        let r = Self::record_words(pages, uz(k.class(c, K_REC)))?;
        // A valid hint is a bitmap word index; the mask only keeps a corrupt one in the bitmap.
        let j = uz(k.class(c, K_HINT)) & (BITMAP_WORDS - 1);
        // The hint word's value as the last fast malloc or free left it (PERF-HEAP-PAIR-IVB-2):
        // loaded beside `K_HINT`, not after it, so the block address does not wait for the
        // bitmap load that `K_HINT` addresses (a load-to-load chain carried from each free to the
        // next malloc). The bitmap word stays the truth: a copy that differs from it (any other
        // writer of the word) only sends this call down the path that uses the word itself.
        let vc = k.class(c, K_HINTV);
        let v = r.local[j].load(Relaxed);
        // A plain compare: `black_box` here spilled the flag to the stack (PERF-HEAP-PAIR-IVB-3).
        if v == vc && vc != u64::MAX {
            let b = (!vc).trailing_zeros();
            let n = vc | (1u64 << b);
            r.local[j].store(n, Relaxed);
            k.set_class(c, K_HINTV, n);
            let i = j * 64 + uz(u64::from(b));
            let base = k.class(c, K_BASE);
            k.set(slab_hint(uz(base)), base | w64(c));
            return Some(Block { addr: uz(base) + i * CLASSES[c], usable: CLASSES[c], zeroed: false });
        }
        if v == u64::MAX {
            // The hint word is full: `alloc_group` (malloc) or the slow path takes the lowest
            // free block of the groups that may have one (D-3.16). Not here: inlined, the group
            // loop made `malloc` save a register on every call (tar -c +0.4%); out of line, the
            // call did too (+0.7%).
            return None;
        }
        let b = (!v).trailing_zeros();
        let n = v | (1u64 << b);
        r.local[j].store(n, Relaxed);
        k.set_class(c, K_HINTV, n);
        let i = j * 64 + uz(u64::from(b));
        let base = k.class(c, K_BASE);
        k.set(slab_hint(uz(base)), base | w64(c));
        Some(Block { addr: uz(base) + i * CLASSES[c], usable: CLASSES[c], zeroed: false })
    }

    /// Allocate a *secure* block (D-X.11): `size` bytes aligned to `align` (a power of two), in
    /// a fresh mapping of its own (so at least page-aligned) that reads as zero and is kept out
    /// of core dumps where the kernel allows. When it is released, by [`Heap::free`] or by
    /// [`Heap::resize`] moving it, its whole mapping is zeroed through [`Pages::wipe`] and then
    /// unmapped; [`Heap::resize`] keeps it secure. `None` without memory, above
    /// `PTRDIFF_MAX`, or for an alignment that is not a power of two.
    pub fn alloc_secure<S: Sys, P: Pages>(&self, sys: &S, pages: &P, size: usize, align: usize) -> Option<Block> {
        if size > MAX_REQUEST || !align.is_power_of_two() {
            return None;
        }
        let len = size.max(1).checked_next_multiple_of(PAGE)?;
        let shard = Self::own_cache(pages).map_or(0, |k| uz(k.get(C_SHARD)));
        self.map_large(sys, pages, len, align, shard, SECURE)
    }

    /// [`Heap::alloc`] past its fast path.
    #[inline(never)]
    pub fn alloc_slow<S: Sys, P: Pages>(&self, sys: &S, pages: &P, size: usize, align: usize) -> Option<Block> {
        if size > MAX_REQUEST || !align.is_power_of_two() {
            return None;
        }
        match class_for(size, align) {
            Some(c) => {
                let k = self.cache(sys, pages);
                if let Some(b) = k.as_ref().and_then(|k| self.alloc_small(sys, pages, k, c)) {
                    return Some(b);
                }
                // No slab (a segment does not fit, as under a small address-space limit, T14):
                // the block gets a mapping of its own, as a large block does.
                self.alloc_large(sys, pages, k.as_ref(), size, align)
            }
            None => {
                let k = self.cache(sys, pages);
                self.alloc_large(sys, pages, k.as_ref(), size, align)
            }
        }
    }

    /// Where the block at `addr` would be. Lock-free: a slab's class does not change while it
    /// has a live block.
    #[inline]
    fn locate<P: Pages>(&self, pages: &P, addr: usize) -> Result<Loc, Misuse> {
        let Some((idx, base)) = self.find_seg(pages, addr) else { return Ok(Loc::Large) };
        Self::locate_in(pages, idx, base, addr)
    }

    /// [`Heap::locate`] for an address inside the registered segment `idx` at `base`.
    #[inline]
    fn locate_in<P: Pages>(pages: &P, idx: usize, base: usize, addr: usize) -> Result<Loc, Misuse> {
        let within = (addr - base).checked_sub(META).ok_or(Misuse::InvalidPointer)?;
        let i = within >> SLAB_SHIFT;
        if i >= SLABS {
            return Err(Misuse::InvalidPointer);
        }
        let slab = w64(idx * SLABS + i);
        let d = pages.words(base + HEADS + i * HEAD_WORDS * 8 + R_DESC * 8, 1)[0].load(Acquire);
        let class = uz(d & 0xff).checked_sub(1).ok_or(Misuse::InvalidPointer)?;
        let (magic, size) = ((d >> 8) & ((1 << 37) - 1), d >> 45);
        let rel = w64(within & (SLAB - 1));
        let q = (rel * magic) >> 40;
        if q * size != rel || class >= CLASSES.len() || uz(q) >= blocks(class) {
            return Err(Misuse::InvalidPointer);
        }
        let ra = base + LOCALS + i * BITMAP_BYTES;
        Ok(Loc::Small(slab, ra, class, uz(q)))
    }

    /// Blocks of a class-`c` slab marked allocated in its local bitmap (in use, or freed
    /// remotely and not yet collected).
    fn live_bits(r: Rec<'_>, c: usize) -> usize {
        // Bits past the last block are always set (`adopt`), so only the words holding blocks
        // are counted: a large class's slab has a word or two of its 256.
        let n = blocks(c);
        let words = n.div_ceil(64).min(BITMAP_WORDS);
        let set: usize = r.local[..words].iter().map(|w| uz(u64::from(w.load(Relaxed).count_ones()))).sum();
        set.saturating_sub(words * 64 - n)
    }

    fn is_live(r: Rec<'_>, i: usize) -> bool {
        let (j, b) = bit(i);
        r.local[j].load(Acquire) & b != 0 && !r.remote_has(j, b, Acquire)
    }

    /// Release the block at `addr`: into the calling thread's own slab directly, or as a remote
    /// free to the slab's owner. Always inlined, as [`Heap::alloc`].
    #[inline(always)]
    pub fn free<S: Sys, P: Pages>(&self, sys: &S, pages: &P, addr: usize) -> Result<(), Misuse> {
        if Self::free_fast(pages, addr) {
            return Ok(());
        }
        self.free_slow(sys, pages, addr)
    }

    /// [`Heap::free`]'s fast path alone: true if the block at `addr` was released (a live block
    /// of the calling thread's current slab, in the segment the cache last took a slab from);
    /// false leaves everything unchanged for [`Heap::free_slow`]. Separate (TARC-ALLOC) so that
    /// `free` can tail-call the slow path and its fast path needs no stack frame.
    #[inline(always)]
    pub fn free_fast<P: Pages>(pages: &P, addr: usize) -> bool {
        Self::free_hint(pages, addr) || Self::free_seg(pages, addr)
    }

    /// The first half of [`Heap::free_fast`]: a block of a current slab found through the
    /// cache's slab hint. Few registers (TARC-ALLOC: `free` inlines only this half, so its fast
    /// path saves no callee-saved register; [`Heap::free_seg`] runs out of line after it).
    #[inline(always)]
    pub fn free_hint<P: Pages>(pages: &P, addr: usize) -> bool {
        if let Some(k) = Self::own_cache(pages) {
            // A slab hint: a current slab of the cache, its class known.
            let last = k.get(slab_hint(addr));
            let off = w64(addr).wrapping_sub(last & !(SLAB64 - 1));
            if last != 0 && off < SLAB64 {
                let c = uz(last & 63);
                // A valid hint names a class; the test (instead of the cache words' bounds checks
                // and their panic call) keeps the fast path free of a stack frame.
                if c >= CLASSES.len() {
                    return false;
                }
                let [size, magic, n] = FREE_VIEW[c];
                let q = (off * magic) >> 40;
                if q * size == off && q < n {
                    let Some(r) = Self::record_words(pages, uz(k.class(c, K_REC))) else { return false };
                    let (j, b) = bit(uz(q));
                    // `q < n` keeps `j` in the bitmap; the mask lets the compiler see it.
                    let j = j & (BITMAP_WORDS - 1);
                    let v = r.local[j].load(Relaxed);
                    // The cache's remote flag first (D-3.16): one compare while no remote free
                    // ever reached a slab of this cache.
                    if v & b != 0 && !(k.remote_any() && r.remote_has(j, b)) {
                        r.local[j].store(v & !b, Relaxed);
                        k.freed_into(c, j);
                        k.set_class(c, K_HINTV, v & !b);
                        return true;
                    }
                }
            }
        }
        false
    }

    /// The second half of [`Heap::free_fast`]: a block of a current slab in the segment the
    /// cache last took a slab from, found through the segment (the slab hint missed).
    #[inline(always)]
    pub fn free_seg<P: Pages>(pages: &P, addr: usize) -> bool {
        if let Some(k) = Self::own_cache(pages) {
            let seg = uz(k.get(C_SEG));
            if seg != 0 && seg & !(SEG - 1) == addr & !(SEG - 1) {
                if let Ok(Loc::Small(s, ra, c, i)) = Self::locate_in(pages, (seg & (SEG - 1)) - 1, addr & !(SEG - 1), addr) {
                if k.class(c, K_CUR) == s + 1 {
                    let r = Self::rec_at(pages, ra);
                    let (j, b) = bit(i);
                    // `locate_in` keeps `i` below the slab's block count, so `j` is in the bitmap;
                    // the mask lets the compiler see it (no bounds check, no panic call).
                    let j = j & (BITMAP_WORDS - 1);
                    let v = r.local[j].load(Relaxed);
                    if v & b != 0 && !(k.remote_any() && r.remote_has(j, b, Relaxed)) {
                        r.local[j].store(v & !b, Relaxed);
                        k.freed_into(c, j);
                        k.set_class(c, K_HINTV, v & !b);
                        return true;
                    }
                }
                }
            }
        }
        false
    }

    /// [`Heap::free`] past its fast path (the registry lookup, other slabs, remote and large
    /// blocks, misuse).
    #[inline(never)]
    pub fn free_slow<S: Sys, P: Pages>(&self, sys: &S, pages: &P, addr: usize) -> Result<(), Misuse> {
        match self.locate(pages, addr)? {
            Loc::Small(s, ra, c, i) => {
                let r = Self::rec_at(pages, ra);
                let owner = r.head[R_OWNER].load(Acquire);
                let (j, b) = bit(i);
                let mine = Self::own_cache(pages);
                if let Some(k) = mine.filter(|k| owner != 0 && owner == k.id()) {
                    let v = r.local[j].load(Relaxed);
                    if v & b == 0 || r.remote_has(j, b, Relaxed) {
                        return Err(Misuse::DoubleFree);
                    }
                    r.local[j].store(v & !b, Relaxed);
                    if k.class(c, K_CUR) == s + 1 {
                        k.freed_into(c, j);
                        return Ok(());
                    }
                    let used = r.head[R_USED].load(Relaxed) - 1;
                    r.head[R_USED].store(used, Relaxed);
                    if used + 1 == w64(blocks(c)) || used == 0 {
                        self.after_free(sys, pages, &k, c, s, used);
                    }
                    Ok(())
                } else {
                    self.free_remote(pages, s, r, owner, i)
                }
            }
            Loc::Large => self.free_large(sys, pages, addr),
        }
    }

    fn free_remote<P: Pages>(&self, pages: &P, s: u64, r: Rec<'_>, owner: u64, i: usize) -> Result<(), Misuse> {
        let (j, b) = bit(i);
        if owner == 0 || r.local[j].load(Acquire) & b == 0 {
            return Err(Misuse::DoubleFree);
        }
        // The segment's flag before the bit: a remote bit is never set while it reads zero.
        if r.any.load(Relaxed) == 0 {
            r.any.store(1, Release);
        }
        // Pinned first: while the remote word is non-zero the owner cannot give the slab away.
        r.head[R_REMOTE].fetch_add(1, AcqRel);
        pages.race_point(RacePoint::RemotePinned);
        // The owner's cache flag before the bit (D-3.16), read again now that the count pins
        // the slab: the owner's free fast paths look at the remote bit only once it is set.
        if let Some(o) = self.cache_at(pages, r.head[R_OWNER].load(Acquire)) {
            if !o.remote_any() {
                o.w[C_REMOTE].store(1, Release);
            }
        }
        if r.remote[j].fetch_or(b, AcqRel) & b != 0 {
            r.head[R_REMOTE].fetch_sub(1, Release);
            return Err(Misuse::DoubleFree);
        }
        // Unpinned and queued in one step, after the bit (D-3.15 amendment): the free that
        // finds QUEUED clear pushes the slab; QUEUED then pins it until the owner dequeues it.
        let mut w = r.head[R_REMOTE].load(Relaxed);
        while let Err(now) = r.head[R_REMOTE].compare_exchange_weak(w, (w - 1) | QUEUED, AcqRel, Relaxed) {
            w = now;
        }
        if w & QUEUED == 0 {
            let k = self.cache_at(pages, owner).ok_or(Misuse::InvalidPointer)?;
            let head = &k.w[C_PENDING];
            let mut h = head.load(Relaxed);
            loop {
                r.head[R_PENDING].store(h, Relaxed);
                match head.compare_exchange_weak(h, s + 1, AcqRel, Relaxed) {
                    Ok(_) => break,
                    Err(now) => h = now,
                }
            }
        }
        Ok(())
    }

    /// Whether the block at `addr` is a secure block ([`Heap::alloc_secure`]).
    pub fn is_secure<P: Pages>(&self, pages: &P, addr: usize) -> bool {
        matches!(self.locate(pages, addr), Ok(Loc::Large)) && self.large_find(pages, addr).is_ok_and(|(_, _, lw)| lw & SECURE != 0)
    }

    /// Usable bytes of the live block at `addr` (`malloc_usable_size`).
    pub fn usable<S: Sys, P: Pages>(&self, _sys: &S, pages: &P, addr: usize) -> Result<usize, Misuse> {
        match self.locate(pages, addr)? {
            Loc::Small(_, ra, class, i) => {
                if Self::is_live(Self::rec_at(pages, ra), i) { Ok(CLASSES[class]) } else { Err(Misuse::DoubleFree) }
            }
            Loc::Large => {
                let (_, _, lw) = self.large_find(pages, addr)?;
                if lw & CACHED != 0 { Err(Misuse::DoubleFree) } else { Ok(len_of(lw)) }
            }
        }
    }

    /// Resize the block at `addr` to `size` bytes (`realloc`): in place when it fits, by
    /// `mremap` for large blocks, otherwise into a new block (`copy(from, to, len)` moves the
    /// contents; the boundary performs it). `Ok(None)` without memory: the block is unchanged.
    /// A secure block stays secure: it shrinks in place, and grows into a new secure block,
    /// the old one zeroed and unmapped as [`Heap::free`] does (never by `remap`, so every
    /// mapping a secure block gives up passes through [`Pages::wipe`]).
    ///
    /// A small block of a current slab of the calling thread's cache, resized to at most
    /// [`FORECAST_CAP`] bytes, takes one fused path (REALLOC-FUSE): the block is found once
    /// through the slab hint (as [`Heap::free`]'s fast path finds it), checked live, kept in
    /// place when its class still fits, or else moved into a block from the target class's hint
    /// word and its old bit cleared, without looking it up again. Anything else, and any miss on
    /// the way, takes the general path. A block that moves upward gets the class for about
    /// twice the request ([`Heap::forecast`]).
    #[inline(always)]
    pub fn resize<S: Sys, P: Pages>(&self, sys: &S, pages: &P, addr: usize, size: usize, copy: impl FnOnce(usize, usize, usize)) -> Result<Option<Block>, Misuse> {
        if size <= FORECAST_CAP {
            if let Some(k) = Self::own_cache(pages) {
                let last = k.get(slab_hint(addr));
                let off = w64(addr).wrapping_sub(last & !(SLAB64 - 1));
                if last != 0 && off < SLAB64 {
                    let c = uz(last & 63);
                    let [csize, magic, n] = FREE_VIEW[c];
                    let q = (off * magic) >> 40;
                    if q * csize == off && q < n {
                        if let Some(r) = Self::record_words(pages, uz(k.class(c, K_REC))) {
                            let (j, b) = bit(uz(q));
                            // `q < n` keeps `j` in the bitmap; the mask lets the compiler see it.
                            let j = j & (BITMAP_WORDS - 1);
                            let word = &r.local[j];
                            // Not live: the general path decides (and reports a double free).
                            if word.load(Relaxed) & b != 0 && !(k.remote_any() && r.remote_has(j, b)) {
                                let usable = uz(csize);
                                // `c` came from the masked hint: a padding entry has no blocks,
                                // so `c` is a class here; the `min` lets the compiler see it.
                                let below = CLASSES[c.min(CLASSES.len() - 1).saturating_sub(1)];
                                if size <= usable && (c == 0 || size > below / 2) {
                                    return Ok(Some(Block { addr, usable, zeroed: false }));
                                }
                                let target = if size > usable { Self::forecast(size) } else { size };
                                // `target <= FORECAST_CAP`: the table's class (at most 19, so the
                                // mask is the identity and spares the cache words' bounds checks).
                                let nc = usize::from(SMALL_CLASS[((target + 15) >> 4).min(FORECAST_CAP >> 4)]) & 31;
                                if let Some(new) = Self::alloc_hint(pages, &k, nc) {
                                    copy(addr, new.addr, usable.min(size));
                                    // The old block's bit, cleared as the free fast path does.
                                    let v = word.load(Relaxed);
                                    word.store(v & !b, Relaxed);
                                    k.freed_into(c, j);
                                    return Ok(Some(new));
                                }
                            }
                        }
                    }
                }
            }
        }
        self.resize_slow(sys, pages, addr, size, copy)
    }

    /// The size a small block that `resize` moves upward is allocated for (REALLOC-FUSE growth
    /// forecast): about twice the request, at most [`FORECAST_CAP`], so that a caller doubling
    /// its buffer finds the next step in place. Only `resize` uses it: `malloc`'d blocks keep
    /// their own class.
    #[inline(always)]
    fn forecast(size: usize) -> usize {
        if size < FORECAST_CAP { (size * 2).min(FORECAST_CAP) } else { size }
    }

    /// The general path's growth forecast (HEAP-REUSE, REALLOC-GROW): about four times the
    /// request up to [`MAX_SMALL`], so that a mid-size buffer grown step by step (tar's directory
    /// reads grow one by half each time, 2 KiB to 56 KiB) moves twice instead of four times: a
    /// slab block cannot grow past its class, so each move is a copy of the whole old block (two
    /// thirds of a moving call), and each class it skips is a fresh block it does not touch. A
    /// request already above `MAX_SMALL` stays exact: it is a large block, mapped for its own
    /// length and grown by `remap`. Only blocks of at least [`MID_FORECAST`] bytes: between
    /// `FORECAST_CAP` and it growth stays exact. [`Heap::keeps_in_place`] keeps such a block
    /// where it is when it is resized back down to the request it was forecast for.
    #[inline(always)]
    fn forecast_mid(size: usize) -> usize {
        if size <= MAX_SMALL { (size * MID_GROWTH).min(MAX_SMALL) } else { size }
    }

    /// Whether a live small block of `class` resized to `size` stays in place: its class
    /// still fits and is not more than twice the need, or four times for the classes a mid-size
    /// growth forecast lands in (larger than `MID_GROWTH * MID_FORECAST`), so that a block forecast
    /// for `size` is not moved back down by a resize to that same `size`.
    #[inline(always)]
    fn keeps_in_place(class: usize, size: usize) -> bool {
        if class == 0 {
            return size <= CLASSES[0];
        }
        let below = CLASSES[class - 1];
        let slack = if CLASSES[class] > MID_GROWTH * MID_FORECAST { MID_GROWTH } else { 2 };
        size <= CLASSES[class] && size > below / slack
    }

    #[inline(never)]
    fn resize_slow<S: Sys, P: Pages>(&self, sys: &S, pages: &P, addr: usize, size: usize, copy: impl FnOnce(usize, usize, usize)) -> Result<Option<Block>, Misuse> {
        if size > MAX_REQUEST {
            return Ok(None);
        }
        let (usable, secure) = match self.locate(pages, addr)? {
            Loc::Small(_, ra, class, i) => {
                if !Self::is_live(Self::rec_at(pages, ra), i) {
                    return Err(Misuse::DoubleFree);
                }
                let usable = CLASSES[class];
                if Self::keeps_in_place(class, size) {
                    return Ok(Some(Block { addr, usable, zeroed: false }));
                }
                (usable, false)
            }
            Loc::Large => {
                let (_, _, lw) = self.large_find(pages, addr)?;
                if lw & CACHED != 0 {
                    return Err(Misuse::DoubleFree);
                }
                let (len, secure) = (len_of(lw), lw & SECURE != 0);
                if size <= len && (size > MAX_SMALL || secure) {
                    return Ok(Some(Block { addr, usable: len, zeroed: false }));
                }
                if let Some(new_len) = size.checked_next_multiple_of(PAGE).filter(|_| size > MAX_SMALL && !secure) {
                    let _g = self.large.lock(sys);
                    if let Some(nb) = pages.remap(addr, len, new_len) {
                        if let Ok((t, i, _)) = self.large_find(pages, addr) {
                            t[2 * i].store(TOMBSTONE, Release);
                        }
                        if let Some(g) = self.large_insert_gen(pages) {
                            let t = self.large_gen(pages, g);
                            if let Some(j) = Self::large_slot(t, nb, true) {
                                if t[2 * j].load(Relaxed) == 0 {
                                    self.large_used[g].fetch_add(1, Relaxed);
                                }
                                t[2 * j + 1].store(w64(new_len), Relaxed);
                                t[2 * j].store(w64(nb), Release);
                            }
                        }
                        self.large_mapped.fetch_add(new_len, Relaxed);
                        self.large_mapped.fetch_sub(len, Relaxed);
                        self.note_peaks();
                        return Ok(Some(Block { addr: nb, usable: new_len, zeroed: false }));
                    }
                }
                (len, secure)
            }
        };
        // A small block growing: the forecast size (a large block's `usable` is above both caps).
        let want = if size > usable && usable < FORECAST_CAP {
            Self::forecast(size)
        } else if size > usable && (MID_FORECAST..MAX_SMALL).contains(&usable) {
            Self::forecast_mid(size)
        } else {
            size
        };
        let new = if secure { self.alloc_secure(sys, pages, size, ALIGN) } else { self.alloc(sys, pages, want, ALIGN) };
        let Some(new) = new else { return Ok(None) };
        copy(addr, new.addr, usable.min(size));
        self.free(sys, pages, addr)?;
        Ok(Some(Block { zeroed: false, ..new }))
    }

    /// Release memory to the kernel (`malloc_trim`): the calling thread's kept large blocks and
    /// pending slabs, then the pages of every empty slab the shards hold that an earlier trim
    /// did not discard already. True if any memory was released.
    pub fn trim<S: Sys, P: Pages>(&self, sys: &S, pages: &P) -> bool {
        let mut released = false;
        if let Some(k) = Self::own_cache(pages) {
            released |= k.get(C_LARGE_N) != 0;
            self.drop_large(sys, pages, &k, 0);
            self.collect(sys, pages, &k);
        }
        // Parked caches have no thread to collect their remote frees: take each pool out,
        // collect as their owner, and put it back.
        for sh in &self.shards {
            let mut id = {
                let _g = sh.lock.lock(sys);
                sh.pool.swap(0, Relaxed)
            };
            while let Some(k) = self.cache_at(pages, id) {
                id = k.get(C_POOL);
                self.collect(sys, pages, &k);
                let _g = sh.lock.lock(sys);
                k.set(C_POOL, sh.pool.load(Relaxed));
                sh.pool.store(k.id(), Relaxed);
            }
        }
        for shard in 0..SHARDS {
            let sh = &self.shards[shard];
            let _g = sh.lock.lock(sys);
            let mut s = sh.empty.load(Relaxed);
            while let Some(slab) = s.checked_sub(1) {
                let head = &self.record(pages, slab).head[R_HEAD];
                // A slab discarded by an earlier trim holds no pages to release.
                if head.fetch_or(DISCARDED, Relaxed) & DISCARDED == 0 {
                    pages.discard(self.slab_base(pages, slab), SLAB);
                    released = true;
                }
                s = rec_next(head.load(Relaxed));
            }
        }
        released
    }

    /// Blocks of a class-`c` slab in use: allocated in the local bitmap and not freed remotely.
    /// A block freed remotely counts as free at once, whether or not its owner has collected
    /// it (D-3.4). Each remote word is read before its local word: the collector clears the
    /// local bits before the remote ones (a release, `collect`), so a remote word read without
    /// a block's bit (the acquire) comes with its local bit cleared too.
    fn in_use_bits(r: Rec<'_>, c: usize) -> usize {
        let n = blocks(c);
        let words = n.div_ceil(64).min(BITMAP_WORDS);
        let any = r.any.load(Acquire) != 0;
        let set: usize = (0..words)
            .map(|j| {
                let remote = if any { r.remote[j].load(Acquire) } else { 0 };
                uz(u64::from((r.local[j].load(Relaxed) & !remote).count_ones()))
            })
            .sum();
        // Bits past the last block are set locally and never remotely.
        set.saturating_sub(words * 64 - n)
    }

    /// Current statistics (a snapshot). A block whose `free` completed before the call counts
    /// as free, including a remote free its owner has not collected yet; a block being freed or
    /// allocated during the call may count either way.
    pub fn stats<S: Sys, P: Pages>(&self, sys: &S, pages: &P) -> Stats {
        // In-use bytes come from the slab records, so the fast paths keep no counter.
        let mut in_use = 0;
        for seg in 0..self.nsegs.load(Acquire) {
            for i in 0..SLABS {
                let r = self.record(pages, w64(seg * SLABS + i));
                if r.head[R_OWNER].load(Acquire) != 0 {
                    if let Some(c) = rec_class(r.head[R_HEAD].load(Acquire)) {
                        in_use += Self::in_use_bits(r, c) * CLASSES[c];
                    }
                }
            }
        }
        let empty_slabs: usize = self.shards.iter().map(|sh| sh.empties.load(Relaxed)).sum();
        let _g = self.large.lock(sys);
        let large_mapped = self.large_mapped.load(Relaxed);
        Stats {
            in_use: in_use + large_mapped.saturating_sub(self.large_kept.load(Relaxed)),
            small_mapped: self.nsegs.load(Acquire) * SEG,
            large_mapped,
            large_blocks: self.large_count.load(Relaxed),
            releasable: empty_slabs * SLAB,
            empty_slabs,
            small_in_use: in_use,
            peak_mapped: self.peak_mapped.load(Relaxed),
            peak_large_blocks: self.peak_large_blocks.load(Relaxed),
            peak_large_mapped: self.peak_large_mapped.load(Relaxed),
        }
    }
}
