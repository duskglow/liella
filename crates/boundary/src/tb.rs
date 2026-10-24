//! The calling thread's block (IP-8; `liella_rt::tls`), reached through the thread pointer.
//!
//! In a process Liella started, the runtime made the block (`crate::runtime`). Under glibc's
//! `ld.so`, or in a test process the host C library started, the thread pointer names glibc's
//! thread descriptor, whose `tcbhead_t` has the same prefix and keeps 0x80–0x280 reserved and
//! zero: Liella's fields fall in that range, and the descriptor is larger than the block.

use core::ffi::c_int;

use core::sync::atomic::{AtomicU32, AtomicU8, AtomicUsize};

use liella_rt::thread::{Thread, KEYS, SLOT_SIZE};
use liella_rt::tls::{ThreadBlock, ERRNO_OFFSET};

/// The thread pointer (`%fs:0`): the block's self word (`ThreadBlock::tcb`, which holds the
/// thread pointer in every block, Liella's and glibc's), read with one `%fs`-relative load
/// instead of a call to `liella_machine_tp` (TINY-HOT; `__errno_location` is this plus an add).
#[inline]
pub fn tp() -> usize {
    read::<Tcb>()
}

/// A field of the calling thread's block that [`read`] loads with one `%fs`-relative `mov`
/// (plan S2 category *tb-field*, owner ruling 2026-10-06). Sealed: only the markers `field!`
/// makes (and the two [`Field32`] words) implement it, so a caller names a field, never an
/// offset or a type.
pub trait Field: sealed::Sealed {
    /// The field's offset from the thread pointer (`offset_of!`).
    const OFFSET: usize;
}

mod sealed {
    pub trait Sealed {}
}

/// The length of the array field a projection names (for `field!`'s index check).
const fn words<const N: usize>(_: fn(&ThreadBlock) -> &[AtomicUsize; N]) -> usize {
    N
}

/// A marker for the `AtomicUsize` field `$name`, or for word `$i` of the `AtomicUsize` array
/// field `$name`. The checks are compile-time: the projection does not type-check unless the
/// field (element) is an `AtomicUsize`, an index past the array fails an assertion, and the
/// assertions keep the load one aligned 8-byte `mov` inside the block.
macro_rules! field {
    ($(#[$doc:meta])* $marker:ident = $name:ident[$i:expr]) => {
        field!(@impl $(#[$doc])* $marker, core::mem::offset_of!(ThreadBlock, $name).wrapping_add(($i).wrapping_mul(8)));
        const _: () = assert!($i < words(|b| &b.$name));
    };
    ($(#[$doc:meta])* $marker:ident = $name:ident) => {
        field!(@impl $(#[$doc])* $marker, core::mem::offset_of!(ThreadBlock, $name));
        const _: fn(&ThreadBlock) -> &AtomicUsize = |b| &b.$name;
    };
    (@impl $(#[$doc:meta])* $marker:ident, $offset:expr) => {
        $(#[$doc])*
        pub enum $marker {}
        impl sealed::Sealed for $marker {}
        impl Field for $marker {
            const OFFSET: usize = $offset;
        }
        const _: () = {
            assert!(size_of::<AtomicUsize>() == 8 && align_of::<AtomicUsize>() == 8);
            assert!(<$marker as Field>::OFFSET % 8 == 0);
            assert!(<$marker as Field>::OFFSET + 8 <= liella_rt::tls::BLOCK_SIZE);
        };
    };
}

field! {
    /// `ThreadBlock::locale`: the thread's `uselocale` choice (D-7.8).
    Locale = locale
}

field! {
    /// `ThreadBlock::ctype_b`: the current class table at character 0 (D-5.5).
    CtypeB = ctype_b
}

field! {
    /// `ThreadBlock::ctype_toupper`: the current `toupper` table at character 0 (D-5.5).
    CtypeToupper = ctype_toupper
}

field! {
    /// `ThreadBlock::ctype_tolower`: the current `tolower` table at character 0 (D-5.5).
    CtypeTolower = ctype_tolower
}

field! {
    /// `ThreadBlock::cache[CACHE_TZ_PERIOD]`: word 0 of the thread's `localtime` period (P7).
    TzPeriod0 = cache[liella_rt::tls::CACHE_TZ_PERIOD]
}

field! {
    /// `ThreadBlock::cache[CACHE_TZ_PERIOD + 1]`: word 1 of the thread's `localtime` period.
    TzPeriod1 = cache[liella_rt::tls::CACHE_TZ_PERIOD.wrapping_add(1)]
}

field! {
    /// `ThreadBlock::cache[CACHE_TZ_PERIOD + 2]`: word 2 of the thread's `localtime` period.
    TzPeriod2 = cache[liella_rt::tls::CACHE_TZ_PERIOD.wrapping_add(2)]
}

field! {
    /// `ThreadBlock::tcb`: the block's own address, the thread pointer (`%fs:0`).
    Tcb = tcb
}

field! {
    /// `ThreadBlock::pointer_guard`: the process's pointer-mangling secret (the same in every
    /// thread), which also seals stream records to their `FILE` (D-12.19).
    PointerGuard = pointer_guard
}

field! {
    /// `ThreadBlock::view`: the current locale view (D-7.13).
    View = view
}

field! {
    /// `ThreadBlock::heap_cache`: the address of the thread's heap cache, 0 for none yet (D-3.11).
    HeapCache = heap_cache
}

/// The calling thread's `F` field: `current().<field>.load(Relaxed)` as one `%fs`-relative
/// load, without the call to the thread-pointer primitive. A `field!` marker's offset is an
/// `AtomicUsize` field's, 8-aligned (an aligned 8-byte load: exactly that field's atomic load);
/// a [`Field32`] word's is a 4-aligned `AtomicU32` field's, its 8 bytes inside the block and
/// inside one 64-byte line (assertions at `Field32`), and [`read32`] keeps the low 4 bytes (the
/// field, read whole) and discards the neighbour's.
#[inline]
pub fn read<F: Field>() -> usize {
    let v: usize;
    // UNSAFE(tb-field): Rust has no safe way to address `%fs` (plan S2, owner ruling 2026-10-06).
    #[allow(unsafe_code)]
    // UNSAFE(tb-field) SAFETY: `%fs` names the calling thread's block, at least `BLOCK_SIZE` bytes,
    // live while the thread runs (module documentation). `F::OFFSET`'s 8 bytes are inside the
    // block and one cache line (`field!`; `Field32` words: see `read`'s documentation), so the
    // `mov` is one load that never splits: a `Relaxed` atomic load. No write, stack, flags.
    unsafe {
        core::arch::asm!("mov {v}, qword ptr fs:[{off}]", v = lateout(reg) v, off = const F::OFFSET, options(nostack, preserves_flags, readonly)); // ARCH(x86_64): %fs-relative thread-block load; aarch64 reads TPIDR_EL0 (A-4)
    }
    v
}

/// An `AtomicU32` field of the calling thread's block that [`read32`] reads (as [`Field`];
/// sealed, markers below only). `Word` is a [`Field`] marker for the 8 bytes starting at the
/// field (the field in the low 4, `SHIFT` = 0; the bytes stay inside the block and inside one
/// cache line): [`read32`] is that word's one `%fs`-relative load (the single *tb-field* site,
/// [`read`]) truncated, so no second inline-assembly site exists and no shift is needed (MUTEX-4).
pub trait Field32: sealed::Sealed {
    /// The 8-byte word starting at the field.
    type Word: Field;
    /// The field's bit position inside `Word` (0: the low 4 bytes, little-endian).
    const SHIFT: u32;
}

/// The 8 bytes at `ThreadBlock::tid` (0x84; the low 4 are [`Tid`]).
pub enum TidWord {}
impl sealed::Sealed for TidWord {}
impl Field for TidWord {
    const OFFSET: usize = core::mem::offset_of!(ThreadBlock, tid);
}

/// The 8 bytes at `ThreadBlock::flags` (0x9c; the low 4 are [`Flags`]).
pub enum FlagsWord {}
impl sealed::Sealed for FlagsWord {}
impl Field for FlagsWord {
    const OFFSET: usize = core::mem::offset_of!(ThreadBlock, flags);
}

/// `ThreadBlock::tid`: the kernel thread ID (MUTEX-3: read without the `%fs:0` chain).
pub enum Tid {}
impl sealed::Sealed for Tid {}
impl Field32 for Tid {
    type Word = TidWord;
    const SHIFT: u32 = 0;
}

/// `ThreadBlock::flags`: Liella's flags (`FLAG_OWN`).
pub enum Flags {}
impl sealed::Sealed for Flags {}
impl Field32 for Flags {
    type Word = FlagsWord;
    const SHIFT: u32 = 0;
}

// The words' layout: each `Field32` a 4-aligned `AtomicU32` at the start of its word (`SHIFT`
// 0), the word's 8 bytes inside the block and inside one 64-byte line of the 64-aligned block.
const _: () = {
    let _: fn(&ThreadBlock) -> &AtomicU32 = |b| &b.tid;
    let _: fn(&ThreadBlock) -> &AtomicU32 = |b| &b.flags;
    assert!(size_of::<AtomicU32>() == 4 && align_of::<AtomicU32>() == 4);
    assert!(align_of::<ThreadBlock>() % 64 == 0);
    assert!(<TidWord as Field>::OFFSET % 4 == 0 && <TidWord as Field>::OFFSET % 64 + 8 <= 64);
    assert!(<FlagsWord as Field>::OFFSET % 4 == 0 && <FlagsWord as Field>::OFFSET % 64 + 8 <= 64);
    assert!(<TidWord as Field>::OFFSET + 8 <= liella_rt::tls::BLOCK_SIZE);
    assert!(<FlagsWord as Field>::OFFSET + 8 <= liella_rt::tls::BLOCK_SIZE);
    assert!(<Tid as Field32>::SHIFT == 0 && <Flags as Field32>::SHIFT == 0);
};

/// The calling thread's `F` field: `current().<field>.load(Relaxed)` as one `%fs`-relative
/// load of the word starting at it (no dependent load of the thread pointer first).
#[inline]
pub fn read32<F: Field32>() -> u32 {
    // The field is the word's `SHIFT`..`SHIFT + 32` bits; the mask makes the conversion
    // infallible, so it folds to the shift (no branch).
    u32::try_from((read::<F::Word>() >> F::SHIFT) & 0xffff_ffff).unwrap_or(0)
}

/// The thread block at `tp`.
///
/// # Safety
/// `tp` is the thread pointer of a live thread (a block of at least `BLOCK_SIZE` bytes, see the
/// module documentation), or the address of a fresh block the caller set up.
// UNSAFE(slice): the thread block becomes a reference.
#[allow(unsafe_code)]
pub(crate) unsafe fn at(tp: usize) -> &'static ThreadBlock {
    // UNSAFE(slice): see the function's contract; every field is atomic, so shared references
    // from several places (and the thread's own `%fs`-relative accesses) are sound.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract; the block is 64-aligned (it is placed so) and lives as
    // long as its thread.
    unsafe {
        &*core::ptr::with_exposed_provenance::<ThreadBlock>(tp)
    }
}

/// The calling thread's block.
pub fn current() -> &'static ThreadBlock {
    // UNSAFE(slice): the calling thread's own block.
    #[allow(unsafe_code)]
    // SAFETY: the thread pointer of the running thread names its block (module documentation).
    unsafe {
        at(tp())
    }
}

/// `__errno_location()`: the address of this thread's `errno` (spec §2.1, D-2.1).
pub fn errno_location() -> *mut c_int {
    core::ptr::with_exposed_provenance_mut(tp().wrapping_add(ERRNO_OFFSET))
}

/// The descriptor at `addr` (D-22.9). Crate-private contract (as `crate::raw`): `addr` comes
/// from a thread block's descriptor word or the thread runtime's lists, so it is the 64-aligned
/// address of a descriptor the runtime placed in a mapping it made, which stays mapped while
/// the result is used (a mapping is recycled only once no thread or joiner can reach it).
// UNSAFE(slice): a descriptor in the runtime's own mapping becomes a reference.
#[allow(unsafe_code)]
pub(crate) fn desc_at(addr: usize) -> &'static Thread {
    // UNSAFE(slice): see the function's contract; every field is atomic.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    unsafe {
        &*core::ptr::with_exposed_provenance::<Thread>(addr)
    }
}

/// The result slot at `addr` (`SLOT_SIZE` bytes inside a descriptor's slot area). Crate-private
/// contract as [`desc_at`]: an address `result_slot` computed from a descriptor.
// UNSAFE(slice): a thread's result slot becomes atomic bytes.
#[allow(unsafe_code)]
pub(crate) fn slot_at(addr: usize) -> &'static liella_legacy::result_slots::Buf<SLOT_SIZE> {
    // SAFETY: the caller's contract; `Buf` is a transparent array of atomic bytes, and the slot
    // area lives as long as the thread's mapping.
    unsafe { &*core::ptr::with_exposed_provenance::<liella_legacy::result_slots::Buf<SLOT_SIZE>>(addr) }
}

/// `N` bytes of result slots at `addr`. Crate-private contract as [`slot_at`]: an address
/// `result_area` computed from a descriptor, with the `N` bytes inside its slot area.
// UNSAFE(slice): a thread's result slots become atomic bytes.
#[allow(unsafe_code)]
pub(crate) fn area_at<const N: usize>(addr: usize) -> &'static liella_legacy::result_slots::Buf<N> {
    // SAFETY: the caller's contract; `Buf` is a transparent array of atomic bytes (alignment 1),
    // and the slot area lives as long as the thread's mapping.
    unsafe { &*core::ptr::with_exposed_provenance::<liella_legacy::result_slots::Buf<N>>(addr) }
}

/// The `SLOT_RECORDS` result slot as typed atomics (D-C0.2, D-14.12): word 0 the address of the
/// thread's result table, byte 8 the regex scratch busy flag. Every access to either location
/// goes through this view, so each is only ever accessed at one size (a word and a byte).
#[repr(C)]
pub(crate) struct RecordsSlot {
    /// The thread's result table address (0: none yet).
    pub(crate) table: AtomicUsize,
    /// The regex scratch busy flag (0 free, 1 claimed).
    pub(crate) busy: AtomicU8,
}

/// The records slot at `addr`, or `None` when `addr` is not word aligned. Crate-private contract
/// as [`area_at`]: an address `result_area` would compute from a descriptor for slot
/// `SLOT_RECORDS`, with `size_of::<RecordsSlot>()` bytes inside its slot area.
// UNSAFE(slice): a thread's result slot becomes an atomic word and an atomic byte.
#[allow(unsafe_code)]
pub(crate) fn records_at(addr: usize) -> Option<&'static RecordsSlot> {
    if addr == 0 || addr.checked_rem(align_of::<RecordsSlot>()) != Some(0) {
        return None;
    }
    // UNSAFE(slice): the checked slot address becomes an atomic word and an atomic byte.
    // SAFETY: the caller's contract (the bytes lie in the thread's slot area, which lives as long
    // as the thread's mapping); `addr` was checked above to be non-null and aligned for the word.
    Some(unsafe { &*core::ptr::with_exposed_provenance::<RecordsSlot>(addr) })
}

/// `N` bytes of the calling thread's result area at `addr`. Crate-private contract: `addr` is
/// what `users::block_addr` returned for a kind whose blocks are at least `N` bytes: memory of
/// the library's own, used only by the calling thread and freed only at its exit.
// UNSAFE(slice): a thread's result block becomes atomic bytes.
#[allow(unsafe_code)]
pub(crate) fn block_buf<const N: usize>(addr: usize) -> &'static liella_legacy::result_slots::Buf<N> {
    // SAFETY: the caller's contract; `Buf` is a transparent array of atomic bytes (alignment 1).
    unsafe { &*core::ptr::with_exposed_provenance::<liella_legacy::result_slots::Buf<N>>(addr) }
}

/// The calling thread's block table at `addr` as `N` words, or `None` when `addr` is not word
/// aligned (a `calloc` result always is). Crate-private contract: `addr` is the table
/// `users::table` allocated (at least `N` words), used only by the calling thread and freed only
/// at its exit.
// UNSAFE(slice): a thread's block table becomes atomic words.
#[allow(unsafe_code)]
pub(crate) fn table_words<const N: usize>(addr: usize) -> Option<&'static liella_legacy::result_slots::Words<N>> {
    if addr == 0 || addr.checked_rem(align_of::<liella_legacy::result_slots::Words<N>>()) != Some(0) {
        return None;
    }
    // UNSAFE(slice): the checked table address becomes atomic words.
    // SAFETY: the caller's contract: the allocation at `addr` holds at least `N` words
    // (`users::TABLE` is defined as `TABLE_WORDS` words), and the `'static` view is never used
    // after `users::thread_ending` releases the table (it clears the slot word first, so no later
    // lookup on this thread reaches the freed table). `Words` is a transparent array of atomic
    // words and `addr` was checked above to be non-null and aligned for it.
    Some(unsafe { &*core::ptr::with_exposed_provenance::<liella_legacy::result_slots::Words<N>>(addr) })
}

/// A selected time zone (D-23.6). Crate-private contract: `addr` is an address
/// `liella_legacy::time_compat` recorded for an entry it leaked; such values are never freed or
/// modified.
pub(crate) fn zone_entry_at(addr: usize) -> &'static liella_legacy::time_compat::Entry {
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: a leaked entry, immutable but for its atomic link, that lives for the rest of the
    // process.
    unsafe {
        &*core::ptr::with_exposed_provenance::<liella_legacy::time_compat::Entry>(addr)
    }
}

/// The key area at `addr` (`2 * KEYS` words). Crate-private contract as [`desc_at`]: the key
/// area a descriptor names.
// UNSAFE(slice): a thread's key area becomes atomic words.
#[allow(unsafe_code)]
pub(crate) fn keys_at(addr: usize) -> &'static [AtomicUsize] {
    // UNSAFE(slice): see the function's contract; every access is atomic.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    unsafe {
        core::slice::from_raw_parts(core::ptr::with_exposed_provenance::<AtomicUsize>(addr), KEYS.saturating_mul(2))
    }
}

/// The `n` 32-bit words at `addr` as atomics: a C synchronisation object (`pthread_mutex_t`
/// and friends, D-22.1), viewed only through atomic operations.
///
/// # Safety
/// `addr` is 4-aligned and `n` words there are readable and writable while the result is used.
// UNSAFE(slice): a C object's words become atomics.
#[allow(unsafe_code)]
pub(crate) unsafe fn words32_at<'a>(addr: usize, n: usize) -> &'a [AtomicU32] {
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    unsafe {
        core::slice::from_raw_parts(core::ptr::with_exposed_provenance::<AtomicU32>(addr), n)
    }
}

/// The `n` 64-bit words at `addr` as atomics (thread-runtime memory and C objects).
///
/// # Safety
/// `addr` is 8-aligned and `n` words there are readable and writable while the result is used.
// UNSAFE(slice): memory becomes atomic words.
#[allow(unsafe_code)]
pub(crate) unsafe fn words64_at<'a>(addr: usize, n: usize) -> &'a [AtomicUsize] {
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    unsafe {
        core::slice::from_raw_parts(core::ptr::with_exposed_provenance::<AtomicUsize>(addr), n)
    }
}

/// The thread block of a thread the runtime lists (`pthread_t` values, after
/// `crate::thread::find` matched them against the thread list). Crate-private contract: `tp`
/// is the thread pointer of a descriptor in the list, whose mapping is live.
pub(crate) fn listed(tp: usize) -> &'static ThreadBlock {
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: a listed thread's block lives in its mapping, which outlives its listing.
    unsafe {
        at(tp)
    }
}

/// The thread block of a thread being created, in the mapping `crate::thread::create` just
/// made or reused. Crate-private contract: `tp` lies in that mapping with `BLOCK_SIZE` bytes,
/// 64-aligned.
pub(crate) fn fresh(tp: usize) -> &'static ThreadBlock {
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: the creator's own mapping, laid out by `liella_rt::thread::Layout`.
    unsafe {
        at(tp)
    }
}

/// An interned locale's view (D-7.8, D-7.13). Crate-private contract: `addr` is an address
/// `liella_legacy::global_locale` recorded for a view it leaked (also as stored in a thread
/// block's `view` word); such values are never freed or modified.
pub(crate) fn view_at(addr: usize) -> &'static liella::locale::view::LocaleView {
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: a leaked, immutable `LocaleView` that lives for the rest of the process.
    unsafe {
        &*core::ptr::with_exposed_provenance::<liella::locale::view::LocaleView>(addr)
    }
}

/// A loaded charmap (D-6.8). Crate-private contract: `addr` is an address
/// `liella_legacy::global_locale` recorded for a charmap it leaked; such values are never freed
/// or modified.
pub(crate) fn charmap_at(addr: usize) -> &'static liella::charmap::Charmap {
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: a leaked, immutable `Charmap` that lives for the rest of the process.
    unsafe {
        &*core::ptr::with_exposed_provenance::<liella::charmap::Charmap>(addr)
    }
}

/// The words of a `locale_t` object (`struct __locale_struct`, 29 words, D-7.8). Crate-private
/// contract: `addr` is a value `newlocale` or `duplocale` returned and `freelocale` has not yet
/// released (spec §7.3: any other value is undefined behaviour of the caller), and an 8-aligned
/// heap block the library allocated.
pub(crate) fn locale_object(addr: usize) -> Option<&'static [AtomicUsize]> {
    if addr == 0 || addr % 8 != 0 {
        return None;
    }
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: a live locale object of 29 words.
    let w = unsafe { words64_at(addr, LOCALE_WORDS) };
    Some(w)
}

/// The words of a `FILE` (§12; D-12.11). Crate-private contract: `addr` is a `FILE *` a C
/// caller passed (spec §12: anything but a stream the library opened, or a standard stream,
/// is undefined behaviour of the caller) or one from the open-streams list: an 8-aligned object
/// of `FILE_WORDS` words, a library-allocated stream or an exported standard stream. Such
/// memory is never unmapped (closed streams are recycled, not freed).
pub(crate) fn file_words(addr: usize) -> Option<&'static [AtomicUsize]> {
    if addr == 0 || addr % 8 != 0 {
        return None;
    }
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: a live FILE object of FILE_WORDS words.
    let w = unsafe { words64_at(addr, liella_legacy::stdio_compat::FILE_WORDS) };
    Some(w)
}

/// A stream's lock. Crate-private contract: `addr` is a `_lock` word `crate::stdio` stored,
/// the address of a lock in a slot it leaked (never freed).
pub(crate) fn stream_lock_at(addr: usize) -> &'static liella_legacy::stdio_compat::StreamLock {
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: a leaked StreamLock that lives for the rest of the process; shared access only.
    unsafe {
        &*core::ptr::with_exposed_provenance::<liella_legacy::stdio_compat::StreamLock>(addr)
    }
}

/// A stream's record. Crate-private contract: `addr` is an `_codecvt` word `crate::stdio`
/// stored, the address of the record in a slot it leaked (never freed), and the caller holds
/// that stream's lock (or the caller of the C function does, for the `_unlocked` forms, as
/// their contract requires), so no other reference to it exists meanwhile.
#[allow(clippy::mut_from_ref)]
pub(crate) fn stream_ext_at(addr: usize) -> &'static mut liella_legacy::stdio_compat::Ext {
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: a leaked Ext that lives for the rest of the process, used under its stream's lock.
    unsafe {
        &mut *core::ptr::with_exposed_provenance_mut::<liella_legacy::stdio_compat::Ext>(addr)
    }
}

/// A value a stream's slot keeps at a leaked address, e.g. its `fprintf` plan cache (D-12.21).
/// Crate-private contract: `addr` is the address of a `T` the caller's module leaked (never
/// freed: it stays with the stream's slot), and the caller holds that stream's lock, so no other
/// reference to it exists meanwhile. Generic so that this module names no caller (the affected
/// test narrowing follows module names).
#[allow(clippy::mut_from_ref)]
pub(crate) fn leaked_at<T>(addr: usize) -> &'static mut T {
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: a leaked T that lives for the rest of the process, used under its stream's lock.
    unsafe {
        &mut *core::ptr::with_exposed_provenance_mut::<T>(addr)
    }
}

/// Memory a stream owns or a caller gave it (a buffer, an `fmemopen` buffer, a growing
/// buffer). Crate-private contract: `len` bytes at `addr` stay valid and writable while the
/// stream uses them (the stream's own allocation, or the caller's under §12.3), and the caller
/// holds the stream's lock.
pub(crate) fn stream_bytes(addr: usize, len: usize) -> &'static mut [u8] {
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: the contract above.
    unsafe {
        crate::conv::bytes_at_mut(addr, len)
    }
}

/// A directory-stream or `fts` slot (§13.2, §13.3; `crate::dirtree`). Crate-private contract:
/// `addr` is a `DIR *` or `FTS *` a C caller passed (spec §13: anything but a stream the
/// library opened is undefined behaviour of the caller) and not NULL: the address of a slot
/// `crate::dirtree` leaked (never freed; closed slots are recycled). Shared access only.
pub(crate) fn dir_slot_at<T>(addr: usize) -> Option<&'static T> {
    if addr == 0 || addr.checked_rem(align_of::<T>()) != Some(0) {
        return None;
    }
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: a leaked slot that lives for the rest of the process; shared access only.
    unsafe {
        Some(&*core::ptr::with_exposed_provenance::<T>(addr))
    }
}

/// A slot's state. Crate-private contract: `addr` is the state address a slot of
/// `crate::dirtree` stored (a leaked value, never freed), and the caller holds that slot's lock,
/// so no other reference to it exists meanwhile.
#[allow(clippy::mut_from_ref)]
pub(crate) fn dir_state_at<T>(addr: usize) -> &'static mut T {
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: a leaked value that lives for the rest of the process, used under its slot's lock.
    unsafe {
        &mut *core::ptr::with_exposed_provenance_mut::<T>(addr)
    }
}

/// An `FTSENT` block `crate::dirtree` allocated from the caller's allocator. Crate-private
/// contract: `len` bytes at `addr` belong to a live block of that size the stream owns, and the
/// caller holds the stream's lock (the C caller reads the block only between calls).
pub(crate) fn walk_block(addr: usize, len: usize) -> &'static mut [u8] {
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: the contract above.
    unsafe {
        crate::conv::bytes_at_mut(addr, len)
    }
}

/// A C variable of one word (`getline`'s `*lineptr` and `*n`, `open_memstream`'s `*ptr` and
/// `*size`), or a pointer field of an `argp` tree or state (spec §1.7). Crate-private
/// contract: `addr` is the 8-aligned address of such a variable that the caller's contract keeps
/// valid while the stream (or the argp call) uses it.
pub(crate) fn c_word(addr: usize) -> Option<&'static AtomicUsize> {
    if addr == 0 || addr % 8 != 0 {
        return None;
    }
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: a live, aligned word of the caller's.
    unsafe { words64_at(addr, 1) }.first()
}

/// A C `int` of the caller's: a field of `getopt_long`'s option table (`has_arg`, `val`) or of
/// an `argp` tree or state (spec §1.7), or a variable one of their pointers names (`*longind`,
/// an entry's `*flag`, `argp_parse`'s `*arg_index`). Crate-private contract: `addr` is the
/// 4-aligned address of an `int` the running call's C contract keeps valid.
pub(crate) fn c_int(addr: usize) -> Option<&'static AtomicU32> {
    if addr == 0 || addr % 4 != 0 {
        return None;
    }
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: a live, aligned `int` of the caller's.
    let w = unsafe { words32_at(addr, 1) };
    w.first()
}

/// The words of an `iconv_t` descriptor. Crate-private contract: as for [`locale_object`], for a
/// value `iconv_open` returned and `iconv_close` has not yet released (spec §6.4: any other
/// value is undefined behaviour of the caller), an 8-aligned heap block of [`ICONV_WORDS`].
pub(crate) fn iconv_object(addr: usize) -> Option<&'static [AtomicUsize]> {
    if addr == 0 || addr == usize::MAX || addr % 8 != 0 {
        return None;
    }
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: a live descriptor of ICONV_WORDS words.
    let w = unsafe { words64_at(addr, ICONV_WORDS) };
    Some(w)
}

/// The words of an `iconv_t` descriptor: a tag, then `liella::iconv::Conv::to_words`.
pub(crate) const ICONV_WORDS: usize = 7;

/// The words in `struct __locale_struct` (13 + 3 + 13).
pub(crate) const LOCALE_WORDS: usize = 29;

/// The `n` words at `addr` that the runtime or the heap owns (a heap block holding an
/// affinity copy, an attribute's default). Crate-private contract: `addr` is an 8-aligned
/// address the library itself allocated and still owns.
pub(crate) fn words64(addr: usize, n: usize) -> &'static [AtomicUsize] {
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: the library's own live memory.
    unsafe {
        words64_at(addr, n)
    }
}

/// The `n` words of a cleanup buffer the calling thread registered (`__pthread_register_cancel`,
/// `_pthread_cleanup_push`): the thread unwinds through them while their frames are live.
/// Crate-private contract: `addr` is a value the C caller registered on this thread and has not
/// yet unregistered (§22.3: a buffer lives until its pop).
pub(crate) fn registered(addr: usize, n: usize) -> &'static [AtomicUsize] {
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: a registered buffer lives in a live frame of this thread.
    unsafe {
        words64_at(addr, n)
    }
}

/// The 32-bit word at `addr`: a once object whose routine is running on this thread (its
/// cleanup record names it). Crate-private contract: `addr` is the address of a live,
/// 4-aligned `pthread_once_t` the calling thread's `pthread_once` is running.
pub(crate) fn words32_of(addr: usize) -> Option<&'static AtomicU32> {
    if addr == 0 || addr % 4 != 0 {
        return None;
    }
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: a live once object (the routine it runs has not returned).
    let w = unsafe { words32_at(addr, 1) };
    w.first()
}

/// The saved program counter (`uc_mcontext.gregs[REG_RIP]`, offset 168) of the `ucontext_t`
/// the kernel passed to a handler of the library's internal signal. Crate-private contract:
/// `ctx` is that handler's third argument, during the handler (the kernel's signal frame on
/// this thread's stack).
pub(crate) fn saved_pc(ctx: usize) -> Option<&'static AtomicUsize> {
    if ctx == 0 || ctx % 8 != 0 {
        return None;
    }
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: the kernel's signal frame, live while the handler runs.
    let w = unsafe { words64_at(ctx.wrapping_add(168), 1) };
    w.first()
}
