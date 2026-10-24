//! Category *allocator*: the process heap and `unsafe impl GlobalAlloc`, delegating every
//! decision to the safe algorithm in `liella_rt::heap` (IP-7).
//!
//! * [`HEAP`] is the one heap behind the exported `malloc` family and, in the products, Rust's
//!   global allocator (both allocation domains of D-3.1 meet here unless a C program
//!   interposes its own `malloc`).
//! * [`KernelPages`] supplies memory: `mmap` through `CSys` (never `brk`, D-3.7); unmapping,
//!   discarding and remapping the heap's own mappings by address through `crate::raw`; and
//!   the metadata views and the zeroing of a released secure block (`conv::secure_zero`,
//!   D-X.11), the two places this module converts addresses to references.
//! * The `c_*` functions are the C `malloc` family's way into the heap, through the
//!   debugging state [`DEBUG`] (`M_PERTURB`, `mcheck`, `mtrace`; D-3.5 as decided in phase 13),
//!   whose block accesses ([`BlockMem`]) are this module's third kind of conversion.

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering::Relaxed};

use liella_kernel::errno::{Errno, KResult};
use liella_kernel::fd::Fd;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{MAP_ANONYMOUS, MAP_NORESERVE, MAP_PRIVATE, PROT_READ, PROT_WRITE};
use liella_legacy::malloc_compat::{self, Ctx};
use liella_legacy::malloc_debug::{self, Call, Debug, Mem};
use liella_rt::heap::{Heap, Misuse, Pages, ALIGN, PAGE};

use crate::raw;
use crate::sys::CSys;

/// The process heap.
pub static HEAP: Heap = Heap::new();

/// The heap's memory source.
pub struct KernelPages;

impl Pages for KernelPages {
    fn map(&self, len: usize, align: usize, node: Option<u32>) -> Option<usize> {
        // Over-map by the alignment and cut the ends off, so that the range is aligned.
        let slack = if align > PAGE { align } else { 0 };
        let total = len.checked_add(slack)?;
        // The `Region` is dropped, which leaks the mapping on purpose: the heap owns it by
        // address from here on and releases it through `raw`.
        let Ok(region) = CSys.mmap(0, total, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE, None, 0) else {
            // The over-mapping may not fit under an address-space limit (T14): an aligned range
            // without the slack, found from where the kernel would place `len` bytes.
            return if slack > 0 { map_aligned_exactly(len, align, node) } else { None };
        };
        let base = region.addr();
        let addr = base.checked_next_multiple_of(align.max(PAGE))?;
        let head = addr.saturating_sub(base);
        if head > 0 {
            let _ = raw::munmap(base, head);
        }
        let tail = total.saturating_sub(head).saturating_sub(len);
        if tail > 0 {
            let _ = raw::munmap(addr.saturating_add(len), tail);
        }
        // A preference only; without `CONFIG_NUMA` (`ENOSYS`) the default policy stands.
        if let Some(n) = node {
            let _ = raw::mbind_preferred(addr, len, n);
        }
        Some(addr)
    }
    fn unmap(&self, addr: usize, len: usize) {
        let _ = raw::munmap(addr, len);
    }
    fn discard(&self, addr: usize, len: usize) {
        let _ = raw::madvise_dontneed(addr, len);
    }
    fn remap(&self, addr: usize, old_len: usize, new_len: usize) -> Option<usize> {
        raw::mremap(addr, old_len, new_len).ok()
    }
    fn wipe(&self, addr: usize, len: usize) {
        // The whole mapping of a secure block that `free` (or `realloc` moving it) has just
        // claimed (D-X.11): mapped read-write by `map`, owned by that call alone, unmapped
        // right after; nothing references it.
        // UNSAFE(slice): the secure block's mapping becomes the slice that is zeroed.
        #[allow(unsafe_code)]
        // SAFETY: see above.
        let block = unsafe { crate::conv::buf_mut(core::ptr::with_exposed_provenance_mut(addr), len) };
        if let Ok(b) = block {
            #[cfg(feature = "selftest")]
            let watched = crate::selftest::wipe_before(addr, b);
            crate::conv::secure_zero(b);
            #[cfg(feature = "selftest")]
            crate::selftest::wipe_after(watched, b);
        }
    }
    fn exclude_from_dumps(&self, addr: usize, len: usize) {
        let _ = raw::madvise_dontdump(addr, len);
    }
    fn shard(&self) -> usize {
        node_hint()
    }
    fn cache_slot(&self) -> &AtomicUsize {
        &crate::tb::current().heap_cache
    }
    #[inline(always)]
    fn cache_word(&self) -> usize {
        // HEAP-CUTS2 C4: one `%fs`-relative load (plan S2 category tb-field) instead of the
        // out-of-line thread-pointer call and the load through it.
        crate::tb::read::<crate::tb::HeapCache>()
    }
    fn address_limit(&self) -> Option<usize> {
        // `RLIMIT_AS` (9); `RLIM_INFINITY` is none.
        CSys.prlimit(0, 9, None).ok().map(|l| l.cur).filter(|&c| c != u64::MAX).and_then(|c| usize::try_from(c).ok())
    }
    fn words(&self, addr: usize, n: usize) -> &[AtomicU64] {
        // The heap passes only its metadata ranges (segment records, the large table): parts
        // of private anonymous read-write mappings it made with `map` and never unmaps or hands
        // out, 8-aligned, with `n` words inside; every access is atomic.
        // UNSAFE(allocator): the allocator's own metadata becomes atomic words.
        #[allow(unsafe_code)]
        // SAFETY: see above.
        unsafe {
            core::slice::from_raw_parts(core::ptr::with_exposed_provenance::<AtomicU64>(addr), n)
        }
    }
}

/// `len` bytes at a multiple of `align` without mapping `len + align` first (T14): the kernel
/// picks where `len` bytes go; if that is not aligned, the aligned addresses just above and just
/// below are tried with `MAP_FIXED_NOREPLACE` (never replacing a mapping).
fn map_aligned_exactly(len: usize, align: usize, node: Option<u32>) -> Option<usize> {
    use liella_kernel::uapi::MAP_FIXED_NOREPLACE;
    let flags = MAP_PRIVATE | MAP_ANONYMOUS | MAP_NORESERVE;
    let probe = CSys.mmap(0, len, PROT_READ | PROT_WRITE, flags, None, 0).ok()?.addr();
    let addr = if probe.checked_rem(align) == Some(0) {
        probe
    } else {
        let _ = raw::munmap(probe, len);
        let up = probe.checked_next_multiple_of(align)?;
        let down = up.checked_sub(align)?;
        [up, down].into_iter().find_map(|a| {
            let got = CSys.mmap(a, len, PROT_READ | PROT_WRITE, flags | MAP_FIXED_NOREPLACE, None, 0).ok()?.addr();
            if got == a {
                Some(a)
            } else {
                let _ = raw::munmap(got, len);
                None
            }
        })?
    };
    if let Some(n) = node {
        let _ = raw::mbind_preferred(addr, len, n);
    }
    Some(addr)
}

/// The calling thread's NUMA node (D-3.12), asked only when the heap chooses a cache or a
/// slab: the vDSO `getcpu` (no system call), remembered in the thread block as node + 1 for
/// the next slow path. A thread that migrates is noticed at its next slow path.
fn node_hint() -> usize {
    let tb = crate::tb::current();
    match CSys.getcpu() {
        Ok((_, node)) => {
            tb.node.store(node.saturating_add(1), Relaxed);
            usize::try_from(node).unwrap_or(0)
        }
        Err(_) => usize::try_from(tb.node.load(Relaxed).saturating_sub(1)).unwrap_or(0),
    }
}

/// The heap with its kernel and memory source, as the C-shaped calls take it.
pub static CTX: Ctx<'static, CSys, KernelPages> = Ctx { heap: &HEAP, sys: &CSys, pages: &KernelPages };

/// The block-level misuse C leaves undefined: a diagnostic and abort (S4), in glibc's words.
pub fn misuse(what: &str, m: Misuse) -> ! {
    let msg: &[u8] = match m {
        Misuse::InvalidPointer => b"(): invalid pointer\n",
        Misuse::DoubleFree => b"(): double free detected\n",
    };
    let _ = CSys.write(Fd::STDERR, what.as_bytes());
    let _ = CSys.write(Fd::STDERR, msg);
    crate::ret::abort()
}

/// The value of a heap call, or [`misuse`].
pub fn checked<T>(what: &str, r: Result<T, Misuse>) -> T {
    match r {
        Ok(v) => v,
        Err(m) => misuse(what, m),
    }
}

/// Make room for `len` more bytes in the obstack's current object (`_obstack_newchunk`'s
/// growth: the object moves to a new chunk).
pub(crate) fn obstack_grow_room(o: &mut liella_abi::layout::struct_obstack, len: usize) {
    // UNSAFE(slice): the obstack's chunks are memory its own functions handed to it.
    #[allow(unsafe_code)]
    // SAFETY: the obstack's chunk and object addresses are its own allocations (§3.4).
    let mem = unsafe { crate::conv::ctext() };
    liella_legacy::obstack::obstack_newchunk(
        o,
        len,
        mem,
        crate::conv::CCalls,
        |f, t, n| {
            // UNSAFE(slice): copying the object between the obstack's chunks.
            #[allow(unsafe_code)]
            // SAFETY: both ranges are inside the obstack's chunks.
            unsafe {
                crate::conv::copy_at(f, t, n)
            }
        },
        obstack_handler(),
    );
}

/// The current `obstack_alloc_failed_handler`.
pub fn obstack_handler() -> usize {
    crate::data::obstack_alloc_failed_handler.load(core::sync::atomic::Ordering::Relaxed).expose_provenance()
}

/// The default `obstack_alloc_failed_handler`: report and exit with `obstack_exit_failure`.
/// (`exit` runs no `atexit` handlers until phase 4.)
pub extern "C" fn default_obstack_failure() {
    let _ = CSys.write(Fd::STDERR, liella_legacy::obstack::EXHAUSTED);
    CSys.exit_group(crate::data::obstack_exit_failure.load(core::sync::atomic::Ordering::Relaxed))
}

/// Rust's global allocator in the products (`#[global_allocator]` is set by the product
/// crate, phase 4): the same heap, with sizes and alignments from `Layout`.
pub struct Liella;

// UNSAFE(allocator): implementing `GlobalAlloc` is the allocator category itself.
// SAFETY: blocks come from `HEAP`, which never hands out overlapping live blocks, honours the
// layout's size and alignment, and returns null on exhaustion.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for Liella {
    // UNSAFE(allocator): the trait's required method.
    #[allow(unsafe_code)]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        core::ptr::with_exposed_provenance_mut(rust_alloc(layout.size(), layout.align()))
    }

    // UNSAFE(allocator): the trait's required method.
    #[allow(unsafe_code)]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        rust_dealloc(ptr.expose_provenance(), layout.size());
    }

    // UNSAFE(allocator): the trait's provided method, overridden (HEAP-CUTS C2).
    #[allow(unsafe_code)]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        core::ptr::with_exposed_provenance_mut(rust_alloc_zeroed(layout.size(), layout.align()))
    }

    // UNSAFE(allocator): the trait's provided method, overridden (HEAP-CUTS C2).
    #[allow(unsafe_code)]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        core::ptr::with_exposed_provenance_mut(rust_realloc(ptr.expose_provenance(), layout.size(), layout.align(), new_size))
    }
}

/// [`Liella`]'s `alloc`: `size` bytes at `align` (a power of two) from [`HEAP`], else from the
/// reserve; 0 without memory.
pub fn rust_alloc(size: usize, align: usize) -> usize {
    HEAP.alloc(&CSys, &KernelPages, size, align).map_or_else(|| reserve_alloc(size, align), |b| b.addr)
}

/// [`Liella`]'s `alloc_zeroed`: as [`rust_alloc`], zeroed; a block the heap reports as already
/// zero (fresh pages) is not written again, as `calloc` does.
pub fn rust_alloc_zeroed(size: usize, align: usize) -> usize {
    match HEAP.alloc(&CSys, &KernelPages, size, align) {
        Some(b) => {
            if !b.zeroed {
                zero_fresh(b.addr, size);
            }
            b.addr
        }
        None => {
            let a = reserve_alloc(size, align);
            if a != 0 {
                zero_fresh(a, size);
            }
            a
        }
    }
}

/// [`Liella`]'s `dealloc` of the block at `addr` that was allocated with `size` bytes.
pub fn rust_dealloc(addr: usize, size: usize) {
    if !reserve_free(addr, size) {
        checked("free", HEAP.free(&CSys, &KernelPages, addr));
    }
}

/// [`Liella`]'s `realloc` (HEAP-CUTS C2): the live block at `addr`, allocated with `size` bytes
/// at `align`, resized to `new_size` (> 0) bytes; its new address, or 0 without memory (the
/// block is then unchanged). A heap block at the heap's own alignment goes through
/// [`Heap::resize`], so a growth inside its size class stays in place and a large block moves by
/// `mremap`, instead of the trait's default allocate + copy + free. A block of the reserve, or
/// one with a larger alignment (`Heap::resize` moves at [`ALIGN`]), takes that default path;
/// so does a resize the heap has no memory for, so that the reserve still serves it.
pub fn rust_realloc(addr: usize, size: usize, align: usize, new_size: usize) -> usize {
    if align <= ALIGN && !in_reserve(addr) {
        match HEAP.resize(&CSys, &KernelPages, addr, new_size, copy_blocks) {
            Ok(Some(b)) => return b.addr,
            Ok(None) => {}
            Err(m) => misuse("realloc", m),
        }
    }
    let new = rust_alloc(new_size, align);
    if new != 0 {
        copy_blocks(addr, new, size.min(new_size));
        rust_dealloc(addr, size);
    }
    new
}

// ------------------------------------------------------------------------------ the reserve

/// Bytes of the reserve.
const RESERVE: usize = 64 << 10;

/// The library's own allocations when the heap is exhausted (T22): the short-lived vectors a
/// diagnostic needs (`printf`'s parsed format and arguments, `perror`'s line) come from here, so
/// that a program out of memory can still say so; so does a standard stream's slot when the
/// stream is first used then (`stdio`; kept for good, at most three). A stack: a block freed at
/// the top lowers it, and the whole reserve is free again when no block is live. Never used
/// while the heap serves.
static RESERVE_MEM: [AtomicU64; RESERVE / 8] = [const { AtomicU64::new(0) }; RESERVE / 8];
static RESERVE_LOCK: liella_rt::sync::RawMutex = liella_rt::sync::RawMutex::new();
/// The first free byte (an offset) and the number of live blocks.
static RESERVE_TOP: AtomicUsize = AtomicUsize::new(0);
static RESERVE_LIVE: AtomicUsize = AtomicUsize::new(0);

fn reserve_base() -> usize {
    core::ptr::from_ref(&RESERVE_MEM).expose_provenance()
}

/// Whether `addr` is in the reserve.
fn in_reserve(addr: usize) -> bool {
    let base = reserve_base();
    addr >= base && addr < base.saturating_add(RESERVE)
}

/// `size` bytes at `align` from the reserve: their address, or 0.
fn reserve_alloc(size: usize, align: usize) -> usize {
    let _g = RESERVE_LOCK.lock(&CSys);
    let base = reserve_base();
    let top = RESERVE_TOP.load(Relaxed);
    let Some(start) = base.checked_add(top).and_then(|a| a.checked_next_multiple_of(align.max(8))) else { return 0 };
    let Some(end) = start.checked_add(size.max(1)) else { return 0 };
    if end > base.saturating_add(RESERVE) {
        return 0;
    }
    RESERVE_TOP.store(end.saturating_sub(base), Relaxed);
    RESERVE_LIVE.fetch_add(1, Relaxed);
    start
}

/// Free a block of the reserve (`false`: `addr` is not in it).
fn reserve_free(addr: usize, size: usize) -> bool {
    if !in_reserve(addr) {
        return false;
    }
    let base = reserve_base();
    let _g = RESERVE_LOCK.lock(&CSys);
    if RESERVE_LIVE.fetch_sub(1, Relaxed) == 1 {
        RESERVE_TOP.store(0, Relaxed);
    } else if addr.saturating_add(size.max(1)).saturating_sub(base) == RESERVE_TOP.load(Relaxed) {
        RESERVE_TOP.store(addr.saturating_sub(base), Relaxed);
    }
    true
}

// ------------------------------------------------------------------------------ the C family

/// The C `malloc` family's debugging state (`liella_legacy::malloc_debug`; D-3.5 as decided
/// in phase 13): `M_PERTURB`, `mcheck`, `mtrace`. In the products (feature `export`) Liella's
/// heap serves the process from its start, so the state starts as `VIRGIN` and `mcheck` can
/// succeed before the first allocation; in a test build the host's allocator served the
/// process first, so it is always too late there. A test build (`selftest` without `runtime`:
/// the host started the process, so [`perturb_from_start`] never ran) starts with `ENVIRON`
/// as well: its first call through the C family reads [`TEST_PERTURB`] ([`begin`]).
pub(crate) static DEBUG: Debug = Debug::new(
    (if cfg!(feature = "export") { malloc_debug::VIRGIN } else { 0 })
        | if cfg!(all(feature = "selftest", not(feature = "runtime"))) { malloc_debug::ENVIRON } else { 0 },
);

/// The test builds' switch for `M_PERTURB` (FREE-POISON-TESTS): decimal, as `MALLOC_PERTURB_`.
/// The test runner sets it so that every test poisons the blocks it frees and a read after
/// `free` sees the fill byte on every host, instead of whatever a reuse left there. Its own name,
/// not `MALLOC_PERTURB_`, so that the host's allocator (cargo, rustc) does not perturb too.
pub const TEST_PERTURB: &[u8] = b"LIELLA_TEST_MALLOC_PERTURB";

/// [`Debug::begin`] for the debugging paths; in a test build the first one takes its settings
/// from the environment ([`malloc_debug::ENVIRON`]).
/// In the products (which never set `ENVIRON`) it is [`Debug::begin`] itself, so the
/// environment step is not compiled into them (FREE-POISON-TESTS-8: `hot_text_products`).
#[inline(always)]
fn begin(allocating: bool) -> u32 {
    let f = DEBUG.begin(allocating);
    #[cfg(all(feature = "selftest", not(feature = "runtime")))]
    if f & malloc_debug::ENVIRON != 0 {
        return begin_environ(allocating);
    }
    f
}

/// [`begin`] the first time in a test build: [`TEST_PERTURB`] becomes `M_PERTURB`, then the
/// flags as they now are (`getenv` does not allocate; a call racing this one sees the flag
/// already taken and goes on with the settings as they are).
#[cfg(all(feature = "selftest", not(feature = "runtime")))]
#[cold]
#[inline(never)]
fn begin_environ(allocating: bool) -> u32 {
    if DEBUG.take_environ() {
        if let Some(v) = crate::process::getenv(TEST_PERTURB).map(crate::strz::env_value).and_then(malloc_debug::perturb_value) {
            DEBUG.set_perturb(i32::from(v));
        }
    }
    DEBUG.begin(allocating)
}

/// The memory [`DEBUG`] reads and writes: the guard, user and table bytes of heap blocks.
pub struct BlockMem;

impl Mem for BlockMem {
    fn words(&self, addr: usize, n: usize) -> &[AtomicUsize] {
        // The table: a heap block the debugging state allocated for itself and owns; or the
        // quarantine rings: an anonymous read-write mapping it made and never unmaps.
        crate::tb::words64(addr, n)
    }
    fn read(&self, addr: usize, out: &mut [u8]) {
        // UNSAFE(slice): bytes of a heap block become a slice to read.
        #[allow(unsafe_code)]
        // SAFETY: `malloc_debug` reads only the guard and user bytes of a block its table
        // records as live, or of a block the calling operation owns; both are mapped heap
        // memory of that length.
        let b = unsafe { crate::conv::buf(core::ptr::with_exposed_provenance(addr), out.len()) };
        if let Ok(b) = b {
            liella::bytes::copy(out, b);
        }
    }
    fn write(&self, addr: usize, bytes: &[u8]) {
        // UNSAFE(slice): bytes of a heap block become a slice to write.
        #[allow(unsafe_code)]
        // SAFETY: as `read`: the guards of a block the calling operation just allocated.
        let b = unsafe { crate::conv::buf_mut(core::ptr::with_exposed_provenance_mut(addr), bytes.len()) };
        if let Ok(b) = b {
            liella::bytes::copy(b, bytes);
        }
    }
    fn fill(&self, addr: usize, len: usize, byte: u8) {
        // UNSAFE(slice): bytes of a heap block become a slice to fill.
        #[allow(unsafe_code)]
        // SAFETY: as `write`: a block the calling operation allocated or is releasing.
        let b = unsafe { crate::conv::buf_mut(core::ptr::with_exposed_provenance_mut(addr), len) };
        if let Ok(b) = b {
            liella::bytes::fill(b, byte);
        }
    }
    fn copy(&self, from: usize, to: usize, len: usize) {
        copy_blocks(from, to, len);
    }
}

/// The largest `calloc` size zeroed by the inlined slice fill rather than the fill kernel.
const ZERO_INLINE: usize = 256;

fn zero_fresh(addr: usize, len: usize) {
    if len <= ZERO_INLINE {
        // UNSAFE(slice): a fresh heap block is zeroed.
        #[allow(unsafe_code)]
        // SAFETY: the heap hands a fresh block to the zeroing callback (§3.1).
        unsafe {
            crate::conv::zero_at(addr, len);
        }
        return;
    }
    // Larger blocks: the fill kernel `memset` uses (WL-HOT-MAKE: make callocs 4 KiB per
    // target, where the inlined slice fill took about twice the kernel's instructions; below
    // ZERO_INLINE the inlined fill is cheaper than the kernel call).
    // UNSAFE(slice): a fresh heap block is zeroed.
    #[allow(unsafe_code)]
    // SAFETY: the heap hands a live block of at least `len` bytes at a nonzero address, which
    // nothing references yet, to the zeroing callback (§3.1): valid for writing `len` bytes.
    unsafe {
        crate::hot::set_bytes(core::ptr::with_exposed_provenance_mut(addr), 0, len);
    }
}

/// `realloc`'s move: the copy kernel `memcpy` uses (REALLOC-CHAIN: the generic slice copy took
/// 226 instructions for a 128-byte block, about two thirds of a growing move).
fn copy_blocks(from: usize, to: usize, len: usize) {
    // UNSAFE(slice): bytes move between two heap blocks.
    #[allow(unsafe_code)]
    // SAFETY: the heap (or `realloc` checking blocks) copies between its own live old and new
    // blocks (§3.1): both nonzero and valid for `len` bytes.
    unsafe {
        crate::hot::move_bytes(core::ptr::with_exposed_provenance_mut(to), core::ptr::with_exposed_provenance(from), len);
    }
}

/// `malloc`. Each `c_*` function is the one way its entry points reach the heap: one test of
/// [`DEBUG`]'s word, then the plain path (P6: the hot paths pay nothing else for debugging).
#[inline(always)]
#[cfg_attr(feature = "selftest", allow(dead_code))] // only `caller_alloc` without `selftest` calls it
pub(crate) fn c_malloc(n: usize) -> KResult<usize> {
    if DEBUG.active() {
        return debug_alloc(n, ALIGN, false);
    }
    malloc_compat::malloc(&CTX, n)
}

/// `malloc`'s entry point (TARC-ALLOC): the heap's fast path inline, everything else (the
/// debugging state, the slow path, `ENOMEM`) in [`malloc_general`], which the export tail-calls,
/// so the fast path keeps no stack frame (two of a warm `malloc`'s instructions; tar -c makes
/// about four `malloc` calls per file). The same result as `ret::null(c_malloc(n))`.
#[inline(always)]
pub(crate) fn malloc_entry(n: usize) -> *mut core::ffi::c_void {
    if !DEBUG.active() {
        if let Some(b) = Heap::alloc_fast(&KernelPages, n, ALIGN) {
            return core::ptr::with_exposed_provenance_mut(b.addr);
        }
    }
    malloc_general(n)
}

/// [`malloc_entry`] past the fast path: the debugging allocation, the lowest free block of the
/// current slab when its hint word was full (D-3.16; here, so the fast path stays frameless), or
/// the heap's slow path.
#[inline(never)]
fn malloc_general(n: usize) -> *mut core::ffi::c_void {
    if DEBUG.active() {
        return crate::ret::null(debug_alloc(n, ALIGN, false));
    }
    if let Some(b) = Heap::alloc_group(&KernelPages, n) {
        return core::ptr::with_exposed_provenance_mut(b.addr);
    }
    crate::ret::null(HEAP.alloc_slow(&CSys, &KernelPages, n, ALIGN).map(|b| b.addr).ok_or(Errno::ENOMEM))
}

/// `free`'s entry point (TARC-ALLOC): `free(NULL)` and the heap's fast path inline, the rest in
/// [`free_general`], tail-called, so neither keeps a stack frame (tar -c: eleven `free(NULL)`
/// and four real frees per file). The same effect as `checked("free", c_free(p))`.
#[inline(always)]
pub(crate) fn free_entry(p: usize) {
    if p == 0 || (!DEBUG.active() && Heap::free_hint(&KernelPages, p)) {
        return;
    }
    free_general(p);
}

/// [`free_entry`] past the slab-hint path: the debugging free, or the heap's segment fast path
/// and then its slow path; misuse aborts.
#[inline(never)]
fn free_general(p: usize) {
    let r = if DEBUG.active() {
        debug_free(p)
    } else if Heap::free_seg(&KernelPages, p) {
        return;
    } else {
        HEAP.free_slow(&CSys, &KernelPages, p)
    };
    checked("free", r);
}

/// `calloc`.
#[inline(always)]
pub(crate) fn c_calloc(k: usize, n: usize) -> KResult<usize> {
    if DEBUG.active() {
        return k.checked_mul(n).ok_or(Errno::ENOMEM).and_then(|t| debug_alloc(t, ALIGN, true));
    }
    malloc_compat::calloc(&CTX, k, n, zero_fresh)
}

/// `realloc`.
#[inline(always)]
pub(crate) fn c_realloc(p: usize, n: usize) -> Result<KResult<usize>, Misuse> {
    if DEBUG.active() {
        return debug_realloc(p, n);
    }
    // `realloc(NULL, n)` is `malloc(n)` (s3_1_c_3_1): taken here, before the resize path's
    // larger frame (REALLOC-FUSE; about a fifth of real reallocs).
    if p == 0 {
        return Ok(malloc_compat::malloc(&CTX, n));
    }
    malloc_compat::realloc(&CTX, p, n, copy_blocks)
}

/// `reallocarray`.
#[inline(always)]
#[cfg_attr(feature = "export", allow(dead_code))] // only `caller_alloc` without `export` calls it
pub(crate) fn c_reallocarray(p: usize, k: usize, n: usize) -> Result<KResult<usize>, Misuse> {
    match k.checked_mul(n) {
        Some(t) => c_realloc(p, t),
        None => Ok(Err(Errno::ENOMEM)),
    }
}

/// `free`.
#[inline(always)]
pub(crate) fn c_free(p: usize) -> Result<(), Misuse> {
    // `free(NULL)` does nothing in every debugging mode too (`debug_free` returns before any
    // check or trace), so it returns before the debugging word is read (MALLOC-FAST; about half
    // of the `free` calls of `ls -lR`).
    if p == 0 {
        return Ok(());
    }
    if DEBUG.active() {
        return debug_free(p);
    }
    malloc_compat::free(&CTX, p)
}

/// `aligned_alloc` (and the native interface's allocation).
#[inline(always)]
pub(crate) fn c_aligned_alloc(align: usize, n: usize) -> KResult<usize> {
    if DEBUG.active() {
        return malloc_compat::aligned_request(align).and_then(|a| debug_alloc(n, a, false));
    }
    malloc_compat::aligned_alloc(&CTX, align, n)
}

/// `aligned_alloc` with the block zeroed (the native interface).
#[inline(always)]
pub(crate) fn c_aligned_calloc(align: usize, n: usize) -> KResult<usize> {
    if DEBUG.active() {
        return malloc_compat::aligned_request(align).and_then(|a| debug_alloc(n, a, true));
    }
    malloc_compat::aligned_calloc(&CTX, align, n, zero_fresh)
}

/// `memalign`.
#[inline(always)]
pub(crate) fn c_memalign(align: usize, n: usize) -> KResult<usize> {
    if DEBUG.active() {
        return malloc_compat::memalign_request(align).and_then(|a| debug_alloc(n, a, false));
    }
    malloc_compat::memalign(&CTX, align, n)
}

/// `posix_memalign`.
#[inline(always)]
pub(crate) fn c_posix_memalign(mut out: impl liella_legacy::string_compat::Cursor, align: usize, n: usize) -> KResult<()> {
    if DEBUG.active() {
        let p = malloc_compat::posix_memalign_request(align).and_then(|a| debug_alloc(n, a, false))?;
        out.set(p);
        return Ok(());
    }
    malloc_compat::posix_memalign(&CTX, out, align, n)
}

/// `valloc`.
#[inline(always)]
pub(crate) fn c_valloc(n: usize) -> KResult<usize> {
    if DEBUG.active() {
        return debug_alloc(n, PAGE, false);
    }
    malloc_compat::valloc(&CTX, n)
}

/// `pvalloc`.
#[inline(always)]
pub(crate) fn c_pvalloc(n: usize) -> KResult<usize> {
    if DEBUG.active() {
        return malloc_compat::pvalloc_size(n).and_then(|s| debug_alloc(s, PAGE, false));
    }
    malloc_compat::pvalloc(&CTX, n)
}

/// `malloc_usable_size`: a checked block's requested size.
#[inline(always)]
pub(crate) fn c_usable_size(p: usize) -> Result<usize, Misuse> {
    if DEBUG.active() && p != 0 {
        return DEBUG.usable(&CTX, &BlockMem, begin(false), p);
    }
    malloc_compat::usable_size(&CTX, p)
}

/// The text naming the caller in a trace line (`MALLOC_TRACE`), when tracing.
struct Caller {
    buf: [u8; 256],
    len: usize,
}

impl Caller {
    fn new(f: u32) -> Caller {
        let mut c = Caller { buf: [0; 256], len: 0 };
        if f & malloc_debug::TRACE != 0 {
            c.len = crate::backtrace::caller_text(&entry_points(), &mut c.buf);
        }
        c
    }
    fn text(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or(&[])
    }
}

/// The functions that may stand between a C caller and the capture of its address: the entry
/// points of the C family and the debugging paths below them.
#[allow(clippy::as_conversions, clippy::fn_to_numeric_cast_any)] // a function's address is its pointer value
fn entry_points() -> [usize; 25] {
    use core::ffi::{c_int, c_void};

    use crate::native::heap as n;
    use crate::posix::s03_memory as m;
    type Size = extern "C" fn(usize) -> *mut c_void;
    type Two = extern "C" fn(usize, usize) -> *mut c_void;
    type Free = extern "C" fn(*mut c_void);
    type Resize = extern "C" fn(*mut c_void, usize) -> *mut c_void;
    let size: [Size; 6] = [m::malloc, m::__libc_malloc, m::valloc, m::__libc_valloc, m::pvalloc, m::__libc_pvalloc];
    let two: [Two; 7] = [m::calloc, m::__libc_calloc, m::aligned_alloc, m::memalign, m::__libc_memalign, n::liella_heap_alloc, n::liella_heap_alloc_zeroed];
    let free: [Free; 4] = [m::free, m::__libc_free, m::cfree, n::liella_heap_free];
    let resize: [Resize; 3] = [m::realloc, m::__libc_realloc, n::liella_heap_realloc];
    let array: extern "C" fn(*mut c_void, usize, usize) -> *mut c_void = m::reallocarray;
    let posix: extern "C" fn(Option<&mut *mut c_void>, usize, usize) -> c_int = m::posix_memalign;
    let alloc: fn(usize, usize, bool) -> KResult<usize> = debug_alloc;
    let release: fn(usize) -> Result<(), Misuse> = debug_free;
    let realloc: fn(usize, usize) -> Result<KResult<usize>, Misuse> = debug_realloc;
    let mut out = [0usize; 25];
    let all = size.iter().map(|&f| f as usize).chain(two.iter().map(|&f| f as usize)).chain(free.iter().map(|&f| f as usize)).chain(resize.iter().map(|&f| f as usize));
    let all = all.chain([array as usize, posix as usize, alloc as usize, release as usize, realloc as usize]);
    for (slot, a) in out.iter_mut().zip(all) {
        *slot = a;
    }
    out
}

/// Report a problem [`DEBUG`] found (any status but `MCHECK_OK` and `MCHECK_DISABLED`) to the
/// program's `abortfn`, or print mcheck(3)'s message for it and abort (the default handler).
/// Never called with the debugging lock held, so `abortfn` may allocate.
fn report(status: i32) {
    if status == malloc_debug::MCHECK_OK || status == malloc_debug::MCHECK_DISABLED {
        return;
    }
    match DEBUG.abortfn() {
        0 => {
            let _ = CSys.write(Fd::STDERR, malloc_debug::status_message(status));
            crate::ret::abort()
        }
        f => {
            let _ = crate::ffi::liella_machine_call3(f, i64::from(status), 0, 0);
        }
    }
}

/// `mcheck_pedantic`'s check before an operation.
fn pedantic(f: u32) {
    if f & malloc_debug::PEDANTIC != 0 {
        check_all();
    }
}

#[cold]
#[inline(never)]
fn debug_alloc(n: usize, align: usize, zero: bool) -> KResult<usize> {
    let f = begin(true);
    let caller = Caller::new(f);
    pedantic(f);
    DEBUG.alloc(&CTX, &BlockMem, Call { flags: f, caller: caller.text() }, n, align, zero)
}

#[cold]
#[inline(never)]
fn debug_free(p: usize) -> Result<(), Misuse> {
    let f = begin(false);
    if p == 0 {
        return Ok(());
    }
    let caller = Caller::new(f);
    pedantic(f);
    let (r, status) = DEBUG.free(&CTX, &BlockMem, Call { flags: f, caller: caller.text() }, p);
    report(status);
    r
}

#[cold]
#[inline(never)]
fn debug_realloc(p: usize, n: usize) -> Result<KResult<usize>, Misuse> {
    if p == 0 {
        return Ok(debug_alloc(n, ALIGN, false));
    }
    if n == 0 {
        return debug_free(p).map(|()| Ok(0));
    }
    let f = begin(true);
    let caller = Caller::new(f);
    pedantic(f);
    let (r, status) = DEBUG.realloc(&CTX, &BlockMem, Call { flags: f, caller: caller.text() }, p, n, copy_blocks);
    report(status);
    r
}

/// `mcheck(abortfn)` / `mcheck_pedantic(abortfn)`: 0 when checking is now on, -1 when it is too
/// late (a block was handed out) or it was on already.
pub fn mcheck(abortfn: Option<extern "C" fn(core::ffi::c_int)>, pedantic: bool) -> core::ffi::c_int {
    #[allow(clippy::as_conversions, clippy::fn_to_numeric_cast_any)] // a function's address
    let f = abortfn.map_or(0, |f| f as usize);
    if DEBUG.mcheck(&CSys, f, pedantic) { 0 } else { -1 }
}

/// `mcheck_check_all()`: every damaged live block reported.
pub fn check_all() {
    let mut from = 0;
    while let Some((next, status)) = DEBUG.check_from(&CSys, &BlockMem, from) {
        report(status);
        from = next;
    }
}

/// `mprobe(p)`: the block's status, reported first when it is a problem.
pub fn mprobe(p: usize) -> core::ffi::c_int {
    let status = DEBUG.mprobe(&CTX, &BlockMem, p);
    report(status);
    status
}

/// `mtrace()`: `MALLOC_TRACE` through `secure_getenv`, so a secure process never creates or
/// appends to a file its caller's environment names.
pub fn mtrace() {
    let path = crate::process::secure_getenv(b"MALLOC_TRACE").map(crate::strz::env_value);
    DEBUG.mtrace(&CSys, path);
}

/// `muntrace()`.
pub fn muntrace() {
    DEBUG.muntrace(&CSys);
}

/// [`CTX`] once perturbation's quarantine has given its blocks to the heap ([`Debug::drain`]):
/// for `malloc_trim`, the statistics and `mallopt`, so they see every released block as free.
/// One load when nothing is held.
pub fn drained() -> &'static Ctx<'static, CSys, KernelPages> {
    DEBUG.drain(&CTX, &BlockMem);
    &CTX
}

/// `M_PERTURB` from the environment at start (`MALLOC_PERTURB_`, which the tunable
/// `glibc.malloc.perturb` overrides; both removed in a secure process).
pub fn perturb_from_start(tunable: Option<u8>) {
    let env = crate::process::getenv(crate::startup_names::MALLOC_PERTURB).map(crate::strz::env_value).and_then(malloc_debug::perturb_value);
    if let Some(v) = tunable.or(env) {
        DEBUG.set_perturb(i32::from(v));
    }
}

/// Text written to a stream a line at a time (`malloc_stats`, `malloc_info`), without
/// allocating; the first failure is kept.
struct StreamLines {
    fp: usize,
    buf: [u8; 160],
    len: usize,
    error: Option<Errno>,
}

impl StreamLines {
    fn new(fp: usize) -> StreamLines {
        StreamLines { fp, buf: [0; 160], len: 0, error: None }
    }
    fn flush(&mut self) {
        if self.len > 0 && self.error.is_none() {
            let line = self.buf.get(..self.len).unwrap_or(&[]);
            if let Err(e) = crate::stdio::fputs(Ok(line), self.fp, crate::stdio::Locking::Locked) {
                self.error = Some(e);
            }
        }
        self.len = 0;
    }
}

impl liella::num::format::Sink for StreamLines {
    fn put(&mut self, mut bytes: &[u8]) {
        // A run at a time: up to the next newline (which ends the line) or the buffer's end.
        while !bytes.is_empty() {
            if self.len >= self.buf.len() {
                self.flush();
            }
            let room = self.buf.len().saturating_sub(self.len);
            let take = bytes.iter().take(room).position(|&b| b == b'\n').map_or(room.min(bytes.len()), |i| i.saturating_add(1));
            let Some((run, rest)) = bytes.split_at_checked(take) else { return };
            if let Some(dst) = self.buf.get_mut(self.len..self.len.saturating_add(take)) {
                dst.copy_from_slice(run);
            }
            self.len = self.len.saturating_add(take);
            if run.last() == Some(&b'\n') {
                self.flush();
            }
            bytes = rest;
        }
    }
}

/// `malloc_stats()`: the statistics through the `stderr` stream, a line at a time; `errno` is
/// left alone (there is no error channel).
pub fn malloc_stats() {
    let saved = crate::errno::get();
    let mut out = StreamLines::new(crate::stdio::stderr());
    malloc_compat::stats_text(&malloc_compat::stats(drained()), &mut out);
    out.flush();
    crate::errno::set_raw(saved);
}

/// `malloc_info(options, fp)`: `EINVAL` for options other than 0 or a NULL stream; the
/// stream's error if a write fails.
pub fn malloc_info(options: core::ffi::c_int, fp: usize) -> KResult<()> {
    malloc_compat::info_options(options)?;
    if fp == 0 {
        return Err(Errno::EINVAL);
    }
    let mut out = StreamLines::new(fp);
    malloc_compat::info_text(&malloc_compat::stats(drained()), &mut out);
    out.flush();
    out.error.map_or(Ok(()), Err)
}

/// A heap block of `n` 8-byte words for an object the library hands to C (a `locale_t`):
/// its address, or `None` when memory is exhausted.
pub(crate) fn alloc_words(n: usize) -> Option<usize> {
    liella_legacy::malloc_compat::malloc(&CTX, n.checked_mul(8)?).ok().filter(|a| *a != 0)
}

/// Release a block [`alloc_words`] returned.
pub(crate) fn free_words(addr: usize) {
    checked("freelocale", liella_legacy::malloc_compat::free(&CTX, addr));
}
