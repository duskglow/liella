//! Allocator debugging for the C `malloc` family (spec §3.2 `M_PERTURB`, §3.3 `mcheck`,
//! `mcheck_pedantic`, `mcheck_check_all`, `mprobe`, `mtrace`, `muntrace`; D-3.5 as decided in
//! phase 13): the semantics glibc provides with `libc_malloc_debug.so` preloaded, provided by
//! Liella's own C library when the program asks for them, since its heap can do it without a
//! second library. Written from the spec, mcheck(3), mtrace(3), mallopt(3), the glibc manual's
//! "Heap Consistency Checking" and "Allocation Debugging", and `spec/behaviour/mallocdbg.md`.
//!
//! **One word decides.** [`Debug`]'s flags are zero while no debugging is on, so the boundary's
//! allocation entry points test one atomic word ([`Debug::active`]) and take the plain path.
//! The flags start as [`VIRGIN`] in the products: the process's first allocation through the C
//! family then takes the slow path once, which clears it, so that [`Debug::mcheck`] can tell
//! whether a block was handed out yet at no cost to the fast path.
//!
//! **Checked blocks** (after a successful `mcheck` or `mcheck_pedantic`, which must come before
//! the first allocation): every block of the C family gets 16 guard bytes before it and
//! [`TAIL`] after its requested size, and is recorded in a table the library owns (its address,
//! size and the offset of the heap block that holds it), so a damaged guard never misleads the
//! bookkeeping. `free`, `realloc` and `mprobe` check the block's guards; `mcheck_check_all`
//! checks every live block, and `mcheck_pedantic` does that before every operation. A problem
//! is reported as an `mcheck_status` to the program's `abortfn`, or to the default handler (a
//! message and abort); when `abortfn` returns, the operation goes on where it safely can: a
//! block with damaged guards is still released (the table, not the guards, says where it is),
//! a block already freed is left alone. Freed blocks stay in the table, marked, until their
//! address is handed out again or the table is rebuilt, so a second `free` reports
//! [`MCHECK_FREE`].
//!
//! **Perturbation** (`M_PERTURB`, `MALLOC_PERTURB_`, `glibc.malloc.perturb`): new blocks other
//! than `calloc`'s are filled with the complement of the value's low byte (the whole usable
//! size, so bytes a later `realloc` copies are filled too), and released blocks with the byte.
//! A secure block (D-X.11) is never filled: its release zeroes it, and nothing else may show.
//! A released block of at most [`QUARANTINE_MAX`] bytes is then held back in a FIFO of
//! [`QUARANTINE`] blocks before the heap gets it (FREE-POISON-TESTS-2), so a read after `free`
//! sees the fill even when the next allocation would have reused the block at once; a second
//! `free` of a held block is a double free. Holding is O(1) and takes no lock (FREE-POISON-TESTS-4):
//! the ring's next slot is one `fetch_add`, and a held block keeps the fill in every byte. `malloc_trim`, the statistics and `mallopt` release
//! the held blocks first ([`Debug::drain`]). With perturbation off nothing is held.
//!
//! **Tracing** (`mtrace` with `MALLOC_TRACE` naming a file): the file is created (mode 0666
//! less the umask) and holds `= Start`, one line per event, and `= End` after `muntrace`:
//! `@ <caller> + <block> <size>` for an allocation, `@ <caller> - <block>` for a release,
//! `@ <caller> < <old>` then `@ <caller> > <new> <size>` for a `realloc`; addresses and sizes in
//! hexadecimal with `0x`. `free(NULL)` and failed allocations are not logged. Each line is one
//! `write`.
//!
//! Every block access goes through [`Mem`], which the boundary implements; the table lives in
//! heap blocks the library allocates for itself (not through the C family), viewed as atomic
//! words and changed only under [`Debug`]'s lock.

use core::sync::atomic::AtomicI32;
use core::sync::atomic::AtomicU32;
use core::sync::atomic::AtomicUsize;
use core::sync::atomic::Ordering::{AcqRel, Acquire, Relaxed, Release};

use liella::error::{Errno, Result};
use liella::num::format::{format_u64, Bounded, Case, Sink};
use liella_kernel::fd::Fd;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{MAP_ANONYMOUS, MAP_PRIVATE, O_CLOEXEC, O_CREAT, O_TRUNC, O_WRONLY, PROT_READ, PROT_WRITE};
use liella_kernel::KStr;
use liella_rt::heap::{Misuse, Pages, ALIGN};
use liella_rt::sync::RawMutex;

use crate::malloc_compat::Ctx;

/// `MCHECK_DISABLED`: checking is off.
pub const MCHECK_DISABLED: i32 = -1;
/// `MCHECK_OK`.
pub const MCHECK_OK: i32 = 0;
/// `MCHECK_FREE`: the block was already freed.
pub const MCHECK_FREE: i32 = 1;
/// `MCHECK_HEAD`: the bytes before the block were changed, or the address is not a block.
pub const MCHECK_HEAD: i32 = 2;
/// `MCHECK_TAIL`: the bytes after the block were changed.
pub const MCHECK_TAIL: i32 = 3;

/// No block has been handed out through the C family yet.
pub const VIRGIN: u32 = 1;
/// Perturbation is on.
pub const PERTURB: u32 = 2;
/// Blocks are checked (`mcheck`).
pub const CHECK: u32 = 4;
/// Every operation checks every block first (`mcheck_pedantic`).
pub const PEDANTIC: u32 = 8;
/// Events are logged (`mtrace`).
pub const TRACE: u32 = 16;
/// A test build has still to take its debugging settings from the environment: the first
/// operation through the C family does ([`Debug::take_environ`]), then this flag is gone and
/// the fast paths are plain again (the products take theirs at start instead).
pub const ENVIRON: u32 = 32;

/// Blocks perturbation holds back after their release (the quarantine's length).
pub const QUARANTINE: usize = 32;
/// The largest block the quarantine holds (larger ones go back to the heap at once): at most
/// `QUARANTINE * QUARANTINE_MAX` bytes (2 MiB) are held.
pub const QUARANTINE_MAX: usize = 64 * 1024;

/// Guard bytes after a checked block's requested size (nonzero, so a stray terminator is seen).
pub const TAIL: [u8; 8] = [0xA5, 0xB4, 0x87, 0x96, 0xE1, 0xF0, 0xC3, 0xD2];
/// Guard bytes before a checked block.
pub const HEAD: usize = 16;

const HEAD_A: u64 = 0x6d63_6865_636b_2d61;
const HEAD_B: u64 = 0x9e37_79b9_7f4a_7c15;
const SLOT_WORDS: usize = 3;
const EMPTY: usize = 0;
const TOMB: usize = 1;
const FREED: usize = 1;
const MIN_SLOTS: usize = 1024;

/// Access to the memory of blocks and of the table (the boundary's conversions).
pub trait Mem {
    /// `n` atomic words at `addr` (a heap block the table owns).
    fn words(&self, addr: usize, n: usize) -> &[AtomicUsize];
    /// Copy `out.len()` bytes from `addr` (inside a live block).
    fn read(&self, addr: usize, out: &mut [u8]);
    /// Store `bytes` at `addr` (inside a live block).
    fn write(&self, addr: usize, bytes: &[u8]);
    /// Store `len` copies of `byte` at `addr` (inside a live block).
    fn fill(&self, addr: usize, len: usize, byte: u8);
    /// Copy `len` bytes from `from` to `to` (two live blocks).
    fn copy(&self, from: usize, to: usize, len: usize);
}

/// One operation's context: the flags [`Debug::begin`] gave, and the caller's text for a trace
/// line (empty when not tracing).
#[derive(Clone, Copy)]
pub struct Call<'a> {
    /// The flags.
    pub flags: u32,
    /// The caller.
    pub caller: &'a [u8],
}

/// The debugging state of one heap's C family.
pub struct Debug {
    flags: AtomicU32,
    perturb: AtomicU32,
    abortfn: AtomicUsize,
    lock: RawMutex,
    /// The table: address of `cap` slots of three words (key, size, offset | freed bit).
    table: AtomicUsize,
    cap: AtomicUsize,
    /// Slots not empty (live, freed or tombstone), and live ones.
    filled: AtomicUsize,
    live: AtomicUsize,
    trace: AtomicI32,
    /// The address of perturbation's quarantine rings (an anonymous mapping of [`HELD_WORDS`]
    /// words), 0 until a release first holds a block.
    held: AtomicUsize,
}

/// Perturbation's quarantine: rings of block addresses that the releases fill in turn, so a
/// block is held until [`QUARANTINE`] later releases of the same thread (a FIFO per thread).
/// Lock-free and O(1): one `fetch_add` picks the slot and one `swap` takes the block it held
/// (FREE-POISON-TESTS-4: the locked scan of every slot per release serialised the threads). Each
/// thread claims its own ring ([`Debug::ring`]), so a block leaves the quarantine on the thread
/// that released it (FREE-POISON-TESTS-6, ruling #295: one global ring handed nearly every block
/// to the heap from another thread, a remote free, and bounced the slot lines between threads;
/// `special_ld_products` ran 7.3x slower). A held block keeps the fill in every byte
/// (FREE-POISON-TESTS-5: run 4 wrote the slot index into its first byte, so a stale read of that
/// byte saw a small plausible number). Whether a block is held: its first byte against the fill,
/// and only when they match a lock-free look at the claimed rings (see [`Debug::is_held`]). The
/// rings are an anonymous mapping of their own, made by the first release that holds a block
/// ([`Debug::held`] keeps its address; FREE-POISON-TESTS-7): with perturbation off nothing of
/// them exists, so no page of .data or .bss and no startup work is added (run 6's 20 KiB static
/// moved the rest of .bss and cost the short products one page fault at start).
///
/// One ring is [`RING_WORDS`] words, on cache lines of its own: the claiming thread's key at
/// [`OWNER`] (the address of its heap cache word, 0 while unclaimed), the next slot's counter at
/// [`NEXT`], then [`QUARANTINE`] slots from [`SLOTS`].
const RING_WORDS: usize = (SLOTS + QUARANTINE).next_multiple_of(8);
const OWNER: usize = 0;
const NEXT: usize = 1;
const SLOTS: usize = 2;
/// Rings in the quarantine (a power of two): threads beyond this many share rings, which stays
/// correct (the slots are swapped atomically) and only costs remote frees.
const RINGS: usize = 64;
/// Words in the rings' mapping.
const HELD_WORDS: usize = RINGS * RING_WORDS;
/// Bytes in the rings' mapping (20 KiB).
const HELD_BYTES: usize = HELD_WORDS * size_of::<usize>();
/// Rings a thread tries for its own before it shares the one its key hashes to.
const PROBE: usize = 8;

enum Found {
    Live(usize),
    Freed(usize),
    Absent,
}

/// A trace line in a fixed buffer (callers' texts are bounded by the boundary).
struct Line {
    buf: [u8; 384],
    len: usize,
}

impl Sink for Line {
    fn put(&mut self, bytes: &[u8]) {
        let mut b = Bounded::new(self.buf.get_mut(self.len..).unwrap_or(&mut []));
        b.put(bytes);
        self.len = self.len.saturating_add(b.stored());
    }
}

impl Line {
    fn new() -> Line {
        Line { buf: [0; 384], len: 0 }
    }
    fn hex(&mut self, v: usize) {
        self.put(b"0x");
        format_u64(self, u64::try_from(v).unwrap_or(u64::MAX), 16, Case::Lower);
    }
    fn bytes(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or(&[])
    }
}

fn head_guard(user: usize, size: usize) -> [u8; HEAD] {
    let u = u64::try_from(user).unwrap_or(0);
    let s = u64::try_from(size).unwrap_or(0);
    let mut g = [0u8; HEAD];
    let (a, b) = g.split_at_mut(8);
    a.copy_from_slice(&(HEAD_A ^ u).to_le_bytes());
    b.copy_from_slice(&(HEAD_B ^ u.rotate_left(29) ^ s).to_le_bytes());
    g
}

impl Debug {
    /// The state with `flags` set (0, or [`VIRGIN`] when this heap serves the process's C
    /// family from its start).
    pub const fn new(flags: u32) -> Debug {
        Debug {
            flags: AtomicU32::new(flags),
            perturb: AtomicU32::new(0),
            abortfn: AtomicUsize::new(0),
            lock: RawMutex::new(),
            table: AtomicUsize::new(0),
            cap: AtomicUsize::new(0),
            filled: AtomicUsize::new(0),
            live: AtomicUsize::new(0),
            trace: AtomicI32::new(-1),
            held: AtomicUsize::new(0),
        }
    }

    /// Whether any debugging (or the first-allocation note) is pending: the one test the fast
    /// paths make.
    #[inline(always)]
    pub fn active(&self) -> bool {
        self.flags.load(Relaxed) != 0
    }

    /// The flags for an operation; an allocating one clears [`VIRGIN`].
    pub fn begin(&self, allocating: bool) -> u32 {
        if allocating { self.flags.fetch_and(!VIRGIN, AcqRel) & !VIRGIN } else { self.flags.load(Acquire) }
    }

    /// The lock serialising checked operations and tracing (`fork` takes it in its prepare
    /// step: [`Debug::lock`]).
    pub fn lock(&self) -> &RawMutex {
        &self.lock
    }

    /// Whether the caller is the one to take the settings from the environment ([`ENVIRON`]
    /// was set; it is clear afterwards, so exactly one caller sees `true`).
    pub fn take_environ(&self) -> bool {
        self.flags.fetch_and(!ENVIRON, AcqRel) & ENVIRON != 0
    }

    /// The registered `abortfn` (0: the default handler).
    pub fn abortfn(&self) -> usize {
        self.abortfn.load(Acquire)
    }

    /// `M_PERTURB`: perturb with the low byte of `value`; 0 turns it off.
    pub fn set_perturb(&self, value: i32) {
        let byte = value.to_le_bytes()[0];
        self.perturb.store(u32::from(byte), Relaxed);
        // An explicit setting wins over a test build's environment ([`ENVIRON`]).
        if byte == 0 {
            self.flags.fetch_and(!(PERTURB | ENVIRON), AcqRel);
        } else {
            self.flags.fetch_or(PERTURB, AcqRel);
            self.flags.fetch_and(!ENVIRON, AcqRel);
        }
    }

    fn perturb_byte(&self) -> u8 {
        self.perturb.load(Relaxed).to_le_bytes()[0]
    }

    /// Whether the quarantine holds `p` (a live block of the heap: released, so a second
    /// `free`, a `realloc` or a `malloc_usable_size` of it is a double free, as it is once the
    /// heap has it): one load when nothing was ever held, else its first byte against the fill.
    /// A held block is the fill in every byte, and a live block's first byte is the fill only
    /// when its owner wrote that value there (perturbation fills new blocks with the
    /// complement), so only then are the slots of the claimed rings looked at (lock-free loads).
    /// Exact because the slots hold exactly the held blocks, unless a write after `free` changed a held
    /// block's first byte (or `mallopt` changed the byte while a release was in flight); then a
    /// second release of it reaches the heap when the block leaves the quarantine.
    #[inline(always)]
    fn is_held<M: Mem>(&self, m: &M, p: usize) -> bool {
        self.held.load(Relaxed) != 0 && self.is_held_slow(m, p)
    }

    /// [`Debug::is_held`] once a release has held a block: out of line and cold (the paths
    /// with perturbation off never come here).
    #[cold]
    #[inline(never)]
    fn is_held_slow<M: Mem>(&self, m: &M, p: usize) -> bool {
        let Some(w) = self.held(m) else { return false };
        let mut first = [0u8; 1];
        m.read(p, &mut first);
        first[0] == self.perturb_byte()
            && w.chunks(RING_WORDS).any(|r| {
                r.get(OWNER).is_some_and(|o| o.load(Acquire) != 0)
                    && r.get(SLOTS..SLOTS + QUARANTINE).is_some_and(|ss| ss.iter().any(|s| s.load(Acquire) == p))
            })
    }

    /// The quarantine rings ([`HELD_WORDS`] words), if a release ever held a block: one load.
    fn held<'m, M: Mem>(&self, m: &'m M) -> Option<&'m [AtomicUsize]> {
        let a = self.held.load(Acquire);
        (a != 0).then(|| m.words(a, HELD_WORDS))
    }

    /// The quarantine rings, mapped by the first caller (once: a caller that loses the race
    /// unmaps its own mapping and takes the winner's). `None` when the mapping fails; the
    /// release then gives its block to the heap at once.
    #[cold]
    #[inline(never)]
    fn map_held<S: Sys>(&self, sys: &S) -> Option<usize> {
        let r = sys
            .mmap(0, HELD_BYTES, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, None, 0)
            .ok()?;
        let a = r.addr();
        match self.held.compare_exchange(0, a, AcqRel, Acquire) {
            // Dropping the `Region` leaves it mapped: the rings live as long as the process.
            Ok(_) => Some(a),
            Err(winner) => {
                let _ = sys.munmap(r);
                Some(winner)
            }
        }
    }

    /// The calling thread's ring in the rings `w`: the one it claimed, else it claims a free one
    /// among the [`PROBE`] its key hashes to, else (all taken) it shares the first of them. O(1).
    fn ring<'w, P: Pages>(pages: &P, w: &'w [AtomicUsize]) -> Option<&'w [AtomicUsize]> {
        // The address of the thread's heap cache word: distinct for every live thread.
        let key = core::ptr::from_ref(pages.cache_slot()).addr();
        let h = (key >> 6).wrapping_mul(0x9e37_79b9_7f4a_7c15).wrapping_shr(usize::BITS.wrapping_sub(RINGS.trailing_zeros()));
        let at = |i: usize| {
            let k = i.wrapping_rem(RINGS).wrapping_mul(RING_WORDS);
            w.get(k..k.wrapping_add(RING_WORDS))
        };
        for i in 0..PROBE {
            let r = at(h.wrapping_add(i))?;
            let owner = r.get(OWNER)?;
            let o = owner.load(Relaxed);
            if o == key || (o == 0 && owner.compare_exchange(0, key, AcqRel, Relaxed).is_ok()) {
                return Some(r);
            }
        }
        at(h)
    }

    /// Hold the released (and filled) block `p` in the calling thread's ring; the block the heap
    /// is to get now (the one the slot held), if any. The first hold maps the rings.
    fn hold<S: Sys, P: Pages, M: Mem>(&self, c: &Ctx<'_, S, P>, m: &M, p: usize) -> Option<usize> {
        let a = match self.held.load(Acquire) {
            0 => match self.map_held(c.sys) {
                Some(a) => a,
                None => return Some(p),
            },
            a => a,
        };
        let Some(ring) = Self::ring(c.pages, m.words(a, HELD_WORDS)) else { return Some(p) };
        let Some(next) = ring.get(NEXT) else { return Some(p) };
        let k = next.fetch_add(1, Relaxed).wrapping_rem(QUARANTINE);
        let Some(slot) = ring.get(SLOTS.wrapping_add(k)) else { return Some(p) };
        let old = slot.swap(p, AcqRel);
        (old != 0).then_some(old)
    }

    /// Give every block the quarantine holds to the heap (`malloc_trim`, `mallinfo`,
    /// `malloc_stats`, `malloc_info`, `mallopt`): one load when nothing was ever held.
    #[inline(always)]
    pub fn drain<S: Sys, P: Pages, M: Mem>(&self, c: &Ctx<'_, S, P>, m: &M) {
        if self.held.load(Relaxed) != 0 {
            self.drain_held(c, m);
        }
    }

    /// [`Debug::drain`] once a release has held a block: out of line and cold.
    #[cold]
    #[inline(never)]
    fn drain_held<S: Sys, P: Pages, M: Mem>(&self, c: &Ctx<'_, S, P>, m: &M) {
        let Some(w) = self.held(m) else { return };
        for r in w.chunks(RING_WORDS) {
            for s in r.get(SLOTS..SLOTS + QUARANTINE).unwrap_or(&[]) {
                let q = s.swap(0, AcqRel);
                if q != 0 {
                    // Each was a live block when it was held, and a held block leaves only by a swap.
                    let _ = c.heap.free(c.sys, c.pages, q);
                }
            }
        }
    }

    /// `mcheck(abortfn)` / `mcheck_pedantic(abortfn)`: turn checking on, which succeeds only
    /// before the first allocation through the C family and only once.
    pub fn mcheck<S: Sys>(&self, sys: &S, abortfn: usize, pedantic: bool) -> bool {
        let _g = self.lock.lock(sys);
        let mut f = self.flags.load(Acquire);
        loop {
            if f & VIRGIN == 0 || f & CHECK != 0 {
                return false;
            }
            self.abortfn.store(abortfn, Release);
            let want = f | CHECK | if pedantic { PEDANTIC } else { 0 };
            match self.flags.compare_exchange(f, want, AcqRel, Acquire) {
                Ok(_) => return true,
                Err(now) => f = now,
            }
        }
    }

    // ------------------------------------------------------------------------- the table

    fn slots<'m, M: Mem>(&self, m: &'m M) -> Option<(&'m [AtomicUsize], usize)> {
        let (t, cap) = (self.table.load(Acquire), self.cap.load(Acquire));
        if t == 0 || cap == 0 {
            return None;
        }
        Some((m.words(t, cap.checked_mul(SLOT_WORDS)?), cap))
    }

    fn word(t: &[AtomicUsize], slot: usize, k: usize) -> usize {
        slot.checked_mul(SLOT_WORDS).and_then(|i| i.checked_add(k)).and_then(|i| t.get(i)).map_or(0, |w| w.load(Relaxed))
    }

    fn set_word(t: &[AtomicUsize], slot: usize, k: usize, v: usize) {
        if let Some(w) = slot.checked_mul(SLOT_WORDS).and_then(|i| i.checked_add(k)).and_then(|i| t.get(i)) {
            w.store(v, Relaxed);
        }
    }

    fn home(key: usize, cap: usize) -> usize {
        let h = u64::try_from(key).unwrap_or(0).wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 20;
        usize::try_from(h).unwrap_or(0) & cap.wrapping_sub(1)
    }

    /// Where `key` is (the lock held).
    fn find<M: Mem>(&self, m: &M, key: usize) -> Found {
        let Some((t, cap)) = self.slots(m) else { return Found::Absent };
        let mut i = Self::home(key, cap);
        for _ in 0..cap {
            match Self::word(t, i, 0) {
                EMPTY => return Found::Absent,
                k if k == key => {
                    return if Self::word(t, i, 2) & FREED != 0 { Found::Freed(i) } else { Found::Live(i) };
                }
                _ => {}
            }
            i = i.wrapping_add(1) & cap.wrapping_sub(1);
        }
        Found::Absent
    }

    /// Record a live block (the lock held); false without memory for the table.
    fn insert<S: Sys, P: Pages, M: Mem>(&self, c: &Ctx<'_, S, P>, m: &M, key: usize, size: usize, off: usize) -> bool {
        if let Found::Live(i) | Found::Freed(i) = self.find(m, key) {
            if let Some((t, _)) = self.slots(m) {
                if Self::word(t, i, 2) & FREED != 0 {
                    self.live.fetch_add(1, Relaxed);
                }
                Self::set_word(t, i, 1, size);
                Self::set_word(t, i, 2, off);
                return true;
            }
        }
        let (filled, cap) = (self.filled.load(Relaxed), self.cap.load(Relaxed));
        if filled.saturating_add(1).saturating_mul(4) > cap.saturating_mul(3) && !self.rebuild(c, m) {
            return false;
        }
        let Some((t, cap)) = self.slots(m) else { return false };
        let mut i = Self::home(key, cap);
        for _ in 0..cap {
            let k = Self::word(t, i, 0);
            if k == EMPTY || k == TOMB {
                if k == EMPTY {
                    self.filled.fetch_add(1, Relaxed);
                }
                Self::set_word(t, i, 1, size);
                Self::set_word(t, i, 2, off);
                Self::set_word(t, i, 0, key);
                self.live.fetch_add(1, Relaxed);
                return true;
            }
            i = i.wrapping_add(1) & cap.wrapping_sub(1);
        }
        false
    }

    /// A new table for the live blocks (freed marks and tombstones are dropped).
    fn rebuild<S: Sys, P: Pages, M: Mem>(&self, c: &Ctx<'_, S, P>, m: &M) -> bool {
        let want = self.live.load(Relaxed).saturating_add(1).saturating_mul(4).max(MIN_SLOTS);
        let Some(cap) = want.checked_next_power_of_two() else { return false };
        let Some(bytes) = cap.checked_mul(SLOT_WORDS * 8) else { return false };
        let Some(block) = c.heap.alloc(c.sys, c.pages, bytes, ALIGN) else { return false };
        let new = m.words(block.addr, cap.saturating_mul(SLOT_WORDS));
        for w in new {
            w.store(0, Relaxed);
        }
        let (mut filled, mut live) = (0usize, 0usize);
        if let Some((old, old_cap)) = self.slots(m) {
            for s in 0..old_cap {
                let k = Self::word(old, s, 0);
                if k == EMPTY || k == TOMB || Self::word(old, s, 2) & FREED != 0 {
                    continue;
                }
                let mut i = Self::home(k, cap);
                while Self::word(new, i, 0) != EMPTY {
                    i = i.wrapping_add(1) & cap.wrapping_sub(1);
                }
                Self::set_word(new, i, 0, k);
                Self::set_word(new, i, 1, Self::word(old, s, 1));
                Self::set_word(new, i, 2, Self::word(old, s, 2));
                filled = filled.saturating_add(1);
                live = live.saturating_add(1);
            }
        }
        let old_table = self.table.swap(block.addr, AcqRel);
        self.cap.store(cap, Release);
        self.filled.store(filled, Relaxed);
        self.live.store(live, Relaxed);
        if old_table != 0 {
            let _ = c.heap.free(c.sys, c.pages, old_table);
        }
        true
    }

    fn mark_freed<M: Mem>(&self, m: &M, slot: usize) {
        if let Some((t, _)) = self.slots(m) {
            Self::set_word(t, slot, 2, Self::word(t, slot, 2) | FREED);
            self.live.fetch_sub(1, Relaxed);
        }
    }

    fn entry<M: Mem>(&self, m: &M, slot: usize) -> (usize, usize) {
        self.slots(m).map_or((0, 0), |(t, _)| (Self::word(t, slot, 1), Self::word(t, slot, 2) & !FREED))
    }

    /// The status of the live checked block `user` of `size` bytes.
    fn guards<M: Mem>(m: &M, user: usize, size: usize) -> i32 {
        let mut head = [0u8; HEAD];
        m.read(user.wrapping_sub(HEAD), &mut head);
        if head != head_guard(user, size) {
            return MCHECK_HEAD;
        }
        let mut tail = [0u8; TAIL.len()];
        m.read(user.wrapping_add(size), &mut tail);
        if tail != TAIL {
            return MCHECK_TAIL;
        }
        MCHECK_OK
    }

    /// A new checked block (the lock held): its user address.
    fn alloc_checked<S: Sys, P: Pages, M: Mem>(&self, c: &Ctx<'_, S, P>, m: &M, n: usize, align: usize) -> Result<usize> {
        let off = align.max(HEAD);
        let total = off.checked_add(n).and_then(|t| t.checked_add(TAIL.len())).ok_or(Errno::ENOMEM)?;
        let b = c.heap.alloc(c.sys, c.pages, total, off).ok_or(Errno::ENOMEM)?;
        let user = b.addr.wrapping_add(off);
        if !self.insert(c, m, user, n, off) {
            let _ = c.heap.free(c.sys, c.pages, b.addr);
            return Err(Errno::ENOMEM);
        }
        m.write(user.wrapping_sub(HEAD), &head_guard(user, n));
        m.write(user.wrapping_add(n), &TAIL);
        Ok(user)
    }

    // ------------------------------------------------------------------------- operations

    /// An allocation of `n` bytes at `align` (zeroed for `calloc`) with the flags `f` of
    /// [`Debug::begin`]; `caller` names the caller in a trace line.
    pub fn alloc<S: Sys, P: Pages, M: Mem>(&self, c: &Ctx<'_, S, P>, m: &M, call: Call<'_>, n: usize, align: usize, zero: bool) -> Result<usize> {
        let (f, caller) = (call.flags, call.caller);
        let (user, usable) = if f & CHECK != 0 {
            let _g = self.lock.lock(c.sys);
            (self.alloc_checked(c, m, n, align)?, n)
        } else {
            let b = c.heap.alloc(c.sys, c.pages, n, align).ok_or(Errno::ENOMEM)?;
            (b.addr, if zero { n } else { b.usable })
        };
        if zero {
            m.fill(user, n, 0);
        } else if f & PERTURB != 0 {
            m.fill(user, usable, !self.perturb_byte());
        }
        if f & TRACE != 0 {
            self.log(c.sys, caller, b'+', user, Some(n));
        }
        Ok(user)
    }

    /// A release (`free`): the heap's verdict on a misuse when checking is off, and the status
    /// to report when it is on.
    pub fn free<S: Sys, P: Pages, M: Mem>(&self, c: &Ctx<'_, S, P>, m: &M, call: Call<'_>, p: usize) -> (core::result::Result<(), Misuse>, i32) {
        let (f, caller) = (call.flags, call.caller);
        if p == 0 {
            return (Ok(()), MCHECK_OK);
        }
        let (r, status) = if f & CHECK != 0 {
            let _g = self.lock.lock(c.sys);
            self.release_checked(c, m, f, p)
        } else if f & PERTURB != 0 {
            (self.release_perturbed(c, m, p), MCHECK_OK)
        } else {
            (c.heap.free(c.sys, c.pages, p), MCHECK_OK)
        };
        if f & TRACE != 0 && r.is_ok() && status != MCHECK_FREE && status != MCHECK_HEAD {
            self.log(c.sys, caller, b'-', p, None);
        }
        (r, status)
    }

    /// `free` of `p` with perturbation on and checking off: the block is filled with the byte
    /// and held in the quarantine, or given to the heap. Out of line and cold, so that the
    /// release paths a program takes with perturbation off keep their size and place in the
    /// library (FREE-POISON-TESTS-8: `hot_text_products`).
    #[cold]
    #[inline(never)]
    fn release_perturbed<S: Sys, P: Pages, M: Mem>(&self, c: &Ctx<'_, S, P>, m: &M, p: usize) -> core::result::Result<(), Misuse> {
        let mut give = Some(p);
        // A secure block's release zeroes it (D-X.11): never perturbed, never held.
        if !c.heap.is_secure(c.pages, p) {
            if let Ok(u) = c.heap.usable(c.sys, c.pages, p) {
                if self.is_held(m, p) {
                    return Err(Misuse::DoubleFree);
                }
                m.fill(p, u, self.perturb_byte());
                if u <= QUARANTINE_MAX && u > 0 {
                    give = self.hold(c, m, p);
                }
            }
        }
        give.map_or(Ok(()), |q| c.heap.free(c.sys, c.pages, q))
    }

    /// `free` of `p` with checking on (the lock held).
    fn release_checked<S: Sys, P: Pages, M: Mem>(&self, c: &Ctx<'_, S, P>, m: &M, f: u32, p: usize) -> (core::result::Result<(), Misuse>, i32) {
        match self.find(m, p) {
            Found::Live(i) => {
                let (size, off) = self.entry(m, i);
                let status = Self::guards(m, p, size);
                if f & PERTURB != 0 {
                    m.fill(p, size, self.perturb_byte());
                }
                self.mark_freed(m, i);
                (c.heap.free(c.sys, c.pages, p.wrapping_sub(off)), status)
            }
            Found::Freed(_) => (Ok(()), MCHECK_FREE),
            Found::Absent => match Self::unrecorded(c, p) {
                MCHECK_OK => (c.heap.free(c.sys, c.pages, p), MCHECK_OK),
                s => (Ok(()), s),
            },
        }
    }

    /// The status of an address the table does not know: [`MCHECK_OK`] for a live block
    /// allocated outside the C family (a secure block, D-X.11), which then gets the plain
    /// operation; otherwise the status to report.
    fn unrecorded<S: Sys, P: Pages>(c: &Ctx<'_, S, P>, p: usize) -> i32 {
        match c.heap.usable(c.sys, c.pages, p) {
            Ok(_) => MCHECK_OK,
            Err(Misuse::DoubleFree) => MCHECK_FREE,
            Err(Misuse::InvalidPointer) => MCHECK_HEAD,
        }
    }

    /// `realloc(p, n)` for `p` not NULL and `n` not 0 (the boundary handles those as
    /// `malloc` and `free`): the new block, or `EINVAL` after reporting a block that is not
    /// live; `copy` moves bytes between blocks.
    pub fn realloc<S: Sys, P: Pages, M: Mem>(&self, c: &Ctx<'_, S, P>, m: &M, call: Call<'_>, p: usize, n: usize, copy: impl FnOnce(usize, usize, usize)) -> (core::result::Result<Result<usize>, Misuse>, i32) {
        let (f, caller) = (call.flags, call.caller);
        let (r, status) = if f & CHECK != 0 {
            let _g = self.lock.lock(c.sys);
            match self.find(m, p) {
                Found::Live(i) => {
                    let (size, off) = self.entry(m, i);
                    let status = Self::guards(m, p, size);
                    let new = self.alloc_checked(c, m, n, ALIGN);
                    if let Ok(q) = new {
                        m.copy(p, q, size.min(n));
                        if f & PERTURB != 0 {
                            if n > size {
                                m.fill(q.wrapping_add(size), n.wrapping_sub(size), !self.perturb_byte());
                            }
                            m.fill(p, size, self.perturb_byte());
                        }
                        // The slot may have moved if the table was rebuilt.
                        if let Found::Live(j) = self.find(m, p) {
                            self.mark_freed(m, j);
                        }
                        let _ = c.heap.free(c.sys, c.pages, p.wrapping_sub(off));
                    }
                    (Ok(new), status)
                }
                Found::Freed(_) => (Ok(Err(Errno::EINVAL)), MCHECK_FREE),
                Found::Absent => match Self::unrecorded(c, p) {
                    MCHECK_OK => (c.heap.resize(c.sys, c.pages, p, n, copy).map(|b| b.map(|b| b.addr).ok_or(Errno::ENOMEM)), MCHECK_OK),
                    s => (Ok(Err(Errno::EINVAL)), s),
                },
            }
        } else {
            let old = match c.heap.usable(c.sys, c.pages, p) {
                Ok(u) => u,
                Err(e) => return (Err(e), MCHECK_OK),
            };
            if self.is_held(m, p) {
                return (Err(Misuse::DoubleFree), MCHECK_OK);
            }
            // A secure block's new bytes read as zero (D-X.11): never perturbed.
            let secure = c.heap.is_secure(c.pages, p);
            match c.heap.resize(c.sys, c.pages, p, n, copy) {
                Ok(Some(b)) => {
                    if f & PERTURB != 0 && b.usable > old && !secure {
                        m.fill(b.addr.wrapping_add(old), b.usable.wrapping_sub(old), !self.perturb_byte());
                    }
                    (Ok(Ok(b.addr)), MCHECK_OK)
                }
                Ok(None) => (Ok(Err(Errno::ENOMEM)), MCHECK_OK),
                Err(e) => (Err(e), MCHECK_OK),
            }
        };
        if f & TRACE != 0 {
            if let Ok(Ok(q)) = r {
                self.log(c.sys, caller, b'<', p, None);
                self.log(c.sys, caller, b'>', q, Some(n));
            }
        }
        (r, status)
    }

    /// `malloc_usable_size(p)` (`p` not NULL): a checked block's requested size.
    pub fn usable<S: Sys, P: Pages, M: Mem>(&self, c: &Ctx<'_, S, P>, m: &M, f: u32, p: usize) -> core::result::Result<usize, Misuse> {
        if f & CHECK != 0 {
            let _g = self.lock.lock(c.sys);
            match self.find(m, p) {
                Found::Live(i) => return Ok(self.entry(m, i).0),
                Found::Freed(_) => return Err(Misuse::DoubleFree),
                Found::Absent => {}
            }
        }
        let u = c.heap.usable(c.sys, c.pages, p)?;
        if f & CHECK == 0 && self.is_held(m, p) {
            return Err(Misuse::DoubleFree);
        }
        Ok(u)
    }

    /// `mprobe(p)`: the block's status, which the caller also reports when it is a problem.
    pub fn mprobe<S: Sys, P: Pages, M: Mem>(&self, c: &Ctx<'_, S, P>, m: &M, p: usize) -> i32 {
        if self.flags.load(Acquire) & CHECK == 0 {
            return MCHECK_DISABLED;
        }
        if p == 0 {
            return MCHECK_OK;
        }
        let _g = self.lock.lock(c.sys);
        match self.find(m, p) {
            Found::Live(i) => Self::guards(m, p, self.entry(m, i).0),
            Found::Freed(_) => MCHECK_FREE,
            Found::Absent => Self::unrecorded(c, p),
        }
    }

    /// One step of `mcheck_check_all`: the first damaged live block at or after slot `from`, as
    /// (the slot after it, its status); `None` when there is none (or checking is off).
    pub fn check_from<S: Sys, M: Mem>(&self, sys: &S, m: &M, from: usize) -> Option<(usize, i32)> {
        if self.flags.load(Acquire) & CHECK == 0 {
            return None;
        }
        let _g = self.lock.lock(sys);
        let (t, cap) = self.slots(m)?;
        (from..cap).find_map(|s| {
            let k = Self::word(t, s, 0);
            if k == EMPTY || k == TOMB || Self::word(t, s, 2) & FREED != 0 {
                return None;
            }
            let st = Self::guards(m, k, Self::word(t, s, 1));
            (st != MCHECK_OK).then_some((s.saturating_add(1), st))
        })
    }

    // ------------------------------------------------------------------------- tracing

    /// `mtrace()` with `path` (the value of `MALLOC_TRACE`, `None` when unset or in a secure
    /// process): start logging to a new file at `path`. Nothing happens when tracing is on
    /// already or the file cannot be created.
    pub fn mtrace<S: Sys>(&self, sys: &S, path: Option<&[u8]>) {
        let Some(path) = path.filter(|p| !p.is_empty()) else { return };
        let _g = self.lock.lock(sys);
        if self.trace.load(Relaxed) >= 0 {
            return;
        }
        let mut buf = [0u8; 4097];
        let Some(z) = buf.get_mut(..path.len()) else { return };
        z.copy_from_slice(path);
        let Some(z) = buf.get(..=path.len()) else { return };
        let Ok(k) = KStr::from_nul_terminated(z) else { return };
        let Ok(fd) = sys.openat(Fd::CWD, k, O_WRONLY | O_CREAT | O_TRUNC | O_CLOEXEC, 0o666) else { return };
        // Kept until `muntrace`: never a standard descriptor (validation T12).
        let Ok(fd) = liella::io::above_standard(sys, fd) else { return };
        let _ = sys.write(fd, b"= Start\n");
        self.trace.store(fd.raw(), Release);
        self.flags.fetch_or(TRACE, AcqRel);
    }

    /// `muntrace()`: stop logging, `= End` last.
    pub fn muntrace<S: Sys>(&self, sys: &S) {
        let _g = self.lock.lock(sys);
        let fd = self.trace.swap(-1, AcqRel);
        self.flags.fetch_and(!TRACE, AcqRel);
        if fd >= 0 {
            let _ = sys.write(Fd::new(fd), b"= End\n");
            let _ = sys.close(Fd::new(fd));
        }
    }

    /// `@ <caller> <op> <block>[ <size>]`, one write, under the lock (never held by the caller).
    fn log<S: Sys>(&self, sys: &S, caller: &[u8], op: u8, addr: usize, size: Option<usize>) {
        let mut l = Line::new();
        l.put(b"@ ");
        l.put(if caller.is_empty() { b"??" } else { caller });
        l.put(&[b' ', op, b' ']);
        l.hex(addr);
        if let Some(n) = size {
            l.put(b" ");
            l.hex(n);
        }
        l.put(b"\n");
        let _g = self.lock.lock(sys);
        let fd = self.trace.load(Acquire);
        if fd >= 0 {
            let _ = sys.write(Fd::new(fd), l.bytes());
        }
    }
}

/// `MALLOC_PERTURB_`'s value: decimal digits, of which the low byte is used as `M_PERTURB`
/// uses its argument's (mallopt(3)); `None` for anything else.
pub fn perturb_value(text: &[u8]) -> Option<u8> {
    if text.is_empty() || !text.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(text.iter().fold(0u8, |acc, &d| acc.wrapping_mul(10).wrapping_add(d.wrapping_sub(b'0'))))
}

/// The default `abortfn`'s message for `status` (mcheck(3)'s descriptions).
pub fn status_message(status: i32) -> &'static [u8] {
    match status {
        MCHECK_HEAD => b"memory clobbered before allocated block\n",
        MCHECK_TAIL => b"memory clobbered past end of allocated block\n",
        MCHECK_FREE => b"block freed twice\n",
        _ => b"bogus mcheck_status, library is buggy\n",
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects)]
mod tests {
    use super::*;

    #[test]
    fn guards_depend_on_address_and_size() {
        assert_ne!(head_guard(0x1000, 10), head_guard(0x1010, 10));
        assert_ne!(head_guard(0x1000, 10), head_guard(0x1000, 11));
        assert!(TAIL.iter().all(|&b| b != 0));
    }

    #[test]
    fn statuses_have_messages() {
        assert!(status_message(MCHECK_TAIL).starts_with(b"memory clobbered past end"));
        assert!(status_message(MCHECK_HEAD).starts_with(b"memory clobbered before"));
    }

    #[test]
    fn perturb_values() {
        assert_eq!(perturb_value(b"165"), Some(165));
        assert_eq!(perturb_value(b"300"), Some(44), "the low byte");
        assert_eq!(perturb_value(b"0"), Some(0));
        assert_eq!(perturb_value(b"junk"), None);
        assert_eq!(perturb_value(b""), None);
    }

    #[test]
    fn debug_path_is_one_shot_without_debugging() {
        // HEAP-DBG: the first allocation takes the debug path once (it clears VIRGIN); with no
        // mcheck/perturb/mtrace the fast paths then see `active() == false` for good.
        let d = Debug::new(VIRGIN);
        assert!(d.active());
        assert_eq!(d.begin(true), 0);
        assert!(!d.active(), "plain runs must leave the debug path after the first allocation");
        assert_eq!(d.begin(true), 0);
        assert!(!d.active());
    }

    #[test]
    fn trace_line_layout() {
        let mut l = Line::new();
        l.put(b"@ p:[0x10] + ");
        l.hex(0xabc);
        assert_eq!(l.bytes(), b"@ p:[0x10] + 0xabc");
    }
}
