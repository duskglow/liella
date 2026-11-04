//! Category *slice*: the only place where an address becomes a Rust slice or reference.
//! Finding F2.
//!
//! Phase 1 needs two conversions, both for memory the kernel created and the boundary
//! tracks:
//! * [`kernel_image`]: the vDSO image, mapped read-only by the kernel for the life of the
//!   process;
//! * [`region_bytes`]: a [`Region`] that is plain private anonymous memory.
//!
//! The conversions of C callers' pointers start here (`buf`, `buf_mut`, `fd`); the exports that
//! use them arrive from phase 3. Rule S6: NULL with a non-zero length is `EFAULT`; NULL with
//! length 0 is an empty slice; a length no object can have (above `isize::MAX`) is `EFAULT`.

use core::ffi::{c_int, c_void};
use core::sync::atomic::AtomicI32;

use liella_kernel::errno::{Errno, KResult};
use liella_kernel::fd::Fd;
use liella::bytes::{self, Unit};

use crate::ffi;
use liella_kernel::mem::Region;
use liella_legacy::cstring::Dest;

/// The kernel-provided image at `addr`, `len` bytes long, for the rest of the process.
///
/// Proof obligation (discharged by the caller in `crate::sys`): `addr` is the
/// `AT_SYSINFO_EHDR` value from the kernel, and `len` is at most the image length its own ELF
/// header declares (`liella_kernel::vdso::image_len`, first called on the
/// `vdso::HEADER_LEN`-byte header, which lies within the first mapped page). The kernel maps
/// the whole image read-only and never unmaps it; only a C caller's `munmap` through `raw`
/// could, which is outside safe code.
pub(crate) fn kernel_image(addr: usize, len: usize) -> &'static [u8] {
    let ptr = core::ptr::with_exposed_provenance::<u8>(addr);
    // UNSAFE(slice): the vDSO is memory Rust did not allocate; turning its address into a slice
    // needs `from_raw_parts`. Validity: see the proof obligation above (mapped, readable,
    // immutable, 'static, `len` within the image).
    #[allow(unsafe_code)]
    // SAFETY: `ptr..ptr+len` is the kernel's read-only, never-unmapped vDSO image (see above).
    unsafe {
        core::slice::from_raw_parts(ptr, len)
    }
}

/// The bytes of `region`, for as long as it is borrowed exclusively. `None` unless the region
/// is plain memory (private anonymous, readable and writable, default protection key).
///
/// Proof: a `Region` is only created by `crate::sys::CSys` from a successful `mmap` (the
/// sealed constructor, enforced by `cargo xtask unsafe-audit`), so `addr..addr+len` is mapped
/// and owned by this value. Every call that could unmap, move or protect it takes the
/// `Region` by value or `&mut`, which the returned borrow excludes. Private anonymous memory
/// is changed by nothing else (no other process shares it, no file backs it).
pub(crate) fn region_bytes(region: &mut Region) -> Option<&mut [u8]> {
    if !region.is_plain_memory() {
        return None;
    }
    let ptr = core::ptr::with_exposed_provenance_mut::<u8>(region.addr());
    // UNSAFE(slice): a kernel mapping is memory Rust did not allocate; viewing it as a slice
    // needs `from_raw_parts_mut`. Validity: see the proof above (mapped, readable, writable,
    // exclusively borrowed for the slice's lifetime, initialised: fresh anonymous pages are
    // zero).
    #[allow(unsafe_code)]
    // SAFETY: the region is mapped, owned, plain memory, exclusively borrowed (see above).
    unsafe {
        Some(core::slice::from_raw_parts_mut(ptr, region.len()))
    }
}

/// The words of `region` as atomics, for as long as it is borrowed. `None` unless
/// [`Region::is_shared_words`].
///
/// Proof: as for [`region_bytes`], the range is mapped and owned by `region`, and every call
/// that could unmap, move or protect it needs the `Region` by value or `&mut`, which the shared
/// borrow excludes. Other processes may change shared memory at any time; atomics are the only
/// sound view of such memory, and `AtomicU32` has the size and alignment of `u32` (mappings are
/// page-aligned).
pub(crate) fn region_words(region: &Region) -> Option<&[core::sync::atomic::AtomicU32]> {
    if !region.is_shared_words() {
        return None;
    }
    let ptr = core::ptr::with_exposed_provenance::<core::sync::atomic::AtomicU32>(region.addr());
    // UNSAFE(slice): kernel-mapped memory becomes a slice only through `from_raw_parts`.
    // Validity: see the proof above (mapped, aligned, initialised, only atomic access).
    #[allow(unsafe_code)]
    // SAFETY: the region is mapped and owned; atomics are the only access (see above).
    unsafe {
        Some(core::slice::from_raw_parts(ptr, region.len() / 4))
    }
}

/// The first `n` words at `addr` of a name-service cache table (D-16.10) as atomics: empty for
/// NULL or a misaligned address.
///
/// Contract (discharged by `crate::nscache::Memory` and `liella_legacy::nscache`): `addr` is a
/// mapping `crate::nscache` made (`MAP_SHARED` of the user's region file, or private anonymous
/// memory), read-write, of at least `n` words, and the caller uses the slice only while it holds
/// the cache's reader count, which keeps the mapping from being unmapped. Other processes of the
/// user may change the words at any time, so atomics are the only view; nothing is assumed about
/// their values (the table format checks every one). A same-user process that truncates the
/// region file makes an access fault (`SIGBUS`), as with any shared file mapping: that process
/// could equally signal or trace this one, so it is outside what the cache defends against.
pub(crate) fn shared_words(addr: usize, n: usize) -> &'static [core::sync::atomic::AtomicU64] {
    if addr == 0 || addr & 7 != 0 || n == 0 {
        return &[];
    }
    let ptr = core::ptr::with_exposed_provenance::<core::sync::atomic::AtomicU64>(addr);
    // UNSAFE(slice): a mapping the cache made becomes atomic words only through
    // `from_raw_parts`. Validity: see the contract above (mapped, aligned, `n` words, kept
    // mapped while used, only atomic access).
    #[allow(unsafe_code)]
    // SAFETY: the contract above.
    unsafe {
        core::slice::from_raw_parts(ptr, n)
    }
}

/// The descriptor number a C caller passed (any value; the kernel judges it).
#[allow(dead_code)] // used by the exports from phase 3
pub(crate) fn fd(n: i32) -> Fd {
    Fd::new(n)
}

/// The entry of a spawn child that shares the caller's memory (D-18.3): `arg` is the address of
/// the caller's `spawn::ChildCtx`, which `spawn::spawn_with` made and keeps alive: the caller is
/// suspended (`CLONE_VFORK`) until this child executes a program or exits, and no other thread
/// has the context. Hands the context to `spawn::child_main`, which never returns.
pub(crate) extern "C" fn spawn_entry(arg: usize) -> ! {
    let ctx = core::ptr::with_exposed_provenance::<crate::spawn::ChildCtx<'static>>(arg);
    // UNSAFE(slice): a context made on the suspended caller's stack becomes a reference in the
    // child (same address space) only through a raw pointer.
    // Validity: the address was made from a live `&ChildCtx` in `spawn_with`, which does not
    // return, move or mutate it before the child has executed or exited, and the child only reads.
    #[allow(unsafe_code)]
    // SAFETY: see above.
    let ctx = unsafe { &*ctx };
    crate::spawn::child_main(ctx)
}

fn check(p: *const c_void, n: usize) -> KResult<bool> {
    match (p.is_null(), n) {
        (_, 0) => Ok(false),
        (true, _) => Err(Errno::EFAULT),
        (false, n) if isize::try_from(n).is_err() => Err(Errno::EFAULT),
        (false, _) => Ok(true),
    }
}

/// `n` readable bytes at `p`.
///
/// # Safety
/// The caller (an export, on behalf of its C caller) guarantees what that function's C
/// contract requires: `p` points to `n` readable, initialised bytes that nothing writes for
/// the returned lifetime.
// UNSAFE(slice): the exports receive C pointers; viewing caller memory as a slice is the one
// place the C caller's contract becomes Rust memory.
#[allow(unsafe_code)]
#[allow(dead_code)] // used by the exports from phase 3
pub(crate) unsafe fn buf<'a>(p: *const c_void, n: usize) -> KResult<&'a [u8]> {
    if !check(p, n)? {
        return Ok(&[]);
    }
    // UNSAFE(slice): see the function's safety contract.
    #[allow(unsafe_code)]
    // SAFETY: non-NULL, n <= isize::MAX (checked); validity is the caller's contract.
    unsafe {
        Ok(core::slice::from_raw_parts(p.cast::<u8>(), n))
    }
}

/// `n` writable bytes at `p`.
///
/// # Safety
/// As for [`buf`], and the bytes are writable and not otherwise referenced for the returned
/// lifetime (C contracts require distinct output buffers unless they say otherwise).
// UNSAFE(slice): as for `buf`, for memory the call writes.
#[allow(unsafe_code)]
#[allow(dead_code)] // used by the exports from phase 3
pub(crate) unsafe fn buf_mut<'a>(p: *mut c_void, n: usize) -> KResult<&'a mut [u8]> {
    if !check(p.cast_const(), n)? {
        return Ok(&mut []);
    }
    // UNSAFE(slice): see the function's safety contract.
    #[allow(unsafe_code)]
    // SAFETY: non-NULL, n <= isize::MAX (checked); validity is the caller's contract.
    unsafe {
        Ok(core::slice::from_raw_parts_mut(p.cast::<u8>(), n))
    }
}

/// `n` writable `wchar_t` units at `p` (a wide output buffer). A misaligned address is
/// `EFAULT` (no `wchar_t` array can start there).
///
/// # Safety
/// As for [`buf_mut`], for `n` units of 4 bytes.
// UNSAFE(slice): as for `buf_mut`, for wide output.
#[allow(unsafe_code)]
pub(crate) unsafe fn wide_mut<'a>(p: *mut i32, n: usize) -> KResult<&'a mut [i32]> {
    let bytes = n.checked_mul(4).ok_or(Errno::EFAULT)?;
    if !check(p.cast_const().cast(), bytes)? {
        return Ok(&mut []);
    }
    if !p.is_aligned() {
        return Err(Errno::EFAULT);
    }
    // UNSAFE(slice): see the function's safety contract.
    #[allow(unsafe_code)]
    // SAFETY: non-NULL, aligned, n * 4 <= isize::MAX (checked); validity is the caller's contract.
    unsafe {
        Ok(core::slice::from_raw_parts_mut(p, n))
    }
}

/// Freshly `calloc`ed memory (`n` zeroed units of `T`), or `None` for a NULL result.
///
/// Proof: `p` came from `calloc(n, size_of::<T>())`, so it is aligned for any fundamental type,
/// holds `n` initialised (zero) units, and nothing else references it yet.
pub(crate) fn fresh<'a, T: Copy + Default>(p: *mut c_void, n: usize) -> Option<&'a mut [T]> {
    if p.is_null() || n == 0 || isize::try_from(n.checked_mul(size_of::<T>())?).is_err() {
        return None;
    }
    // UNSAFE(slice): fresh allocator memory becomes a slice only through `from_raw_parts_mut`.
    #[allow(unsafe_code)]
    // SAFETY: see the proof above (zeroed is a valid value of every `T` used here: integers).
    unsafe {
        Some(core::slice::from_raw_parts_mut(p.cast::<T>(), n))
    }
}

/// Fill freshly `malloc`ed memory `p` (`text.len() + 1` units of `T`, not NULL) with `text` and a
/// terminating zero unit, writing every unit (`strdup`'s copy, without `calloc`'s zeroing).
///
/// Proof: `p` came from `malloc((text.len() + 1) * size_of::<T>())` and is not NULL: aligned for
/// any fundamental type, referenced by nothing else (so it cannot overlap `text`), and only
/// written here through raw pointers, never read before every unit is initialised.
pub(crate) fn fresh_copy_z<T: Copy + Default>(p: *mut T, text: &[T]) {
    let k = text.len();
    let nb = k.wrapping_mul(size_of::<T>());
    if nb <= 16 {
        // UNSAFE(slice): the short copy (STRDUP-FAST): the common short string takes no call into
        // the copy kernel (~17 instructions of call and size dispatch for 0 to 16 bytes).
        #[allow(unsafe_code)]
        // SAFETY: `nb` bytes at `text` and at `p` (proof above); every access below lies inside
        // `0..nb` and the pairs overlap only each other in the same region, with equal bytes.
        unsafe {
            small_copy(p.cast::<u8>(), text.as_ptr().cast::<u8>(), nb);
        }
    } else {
        // UNSAFE(slice): fresh allocator memory is filled only through raw writes.
        #[allow(unsafe_code)]
        // SAFETY: units 0..k of `p` (proof above).
        unsafe {
            core::ptr::copy_nonoverlapping(text.as_ptr(), p, k);
        }
    }
    // UNSAFE(slice): as above, the terminator unit.
    #[allow(unsafe_code)]
    // SAFETY: unit k is inside the `k + 1` allocated units (proof above).
    unsafe {
        p.wrapping_add(k).write(T::default());
    }
}

/// Fill freshly `malloc`ed memory `p` (not NULL; `head` plus the parts' total plus 1 bytes)
/// with `head` zero bytes, then `parts` one after another, then a NUL: every byte is written
/// once, through raw pointers only (FTS-PATH-ZERO: an `FTSENT` block's path is not zeroed
/// before it is copied in).
///
/// Proof: `p` came from `malloc(head + total + 1)` and is not NULL: referenced by nothing else
/// (so it cannot overlap a part), and only written here through raw pointers, never read. The
/// writes cover `0..head`, each part's range in turn, then the last byte: all inside the block.
pub(crate) fn fresh_fill_z(p: *mut u8, head: usize, parts: &[&[u8]]) {
    // UNSAFE(slice): fresh allocator memory is filled only through raw writes.
    #[allow(unsafe_code)]
    // SAFETY: bytes 0..head of `p` (proof above).
    unsafe {
        core::ptr::write_bytes(p, 0, head);
    }
    let mut at = head;
    for part in parts {
        // UNSAFE(slice): as above, the next part.
        #[allow(unsafe_code)]
        // SAFETY: bytes at..at + part.len() of `p`, inside the block (proof above).
        unsafe {
            core::ptr::copy_nonoverlapping(part.as_ptr(), p.wrapping_add(at), part.len());
        }
        at = at.wrapping_add(part.len());
    }
    // UNSAFE(slice): as above, the terminator.
    #[allow(unsafe_code)]
    // SAFETY: byte `at` is the block's last (proof above).
    unsafe {
        p.wrapping_add(at).write(0);
    }
}

/// Move one `T` from `s + off` to `d + off` (unaligned).
///
/// # Safety
/// `s + off` readable and `d + off` writable for `size_of::<T>()` bytes, not overlapping.
// UNSAFE(slice): one raw read and one raw write inside the regions of `fresh_copy_z`.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn mv<T: Copy>(d: *mut u8, s: *const u8, off: usize) {
    // UNSAFE(slice): a short raw copy of the fresh duplicate (STRDUP-FAST).
    // SAFETY: the function's contract.
    let v = unsafe { s.wrapping_add(off).cast::<T>().read_unaligned() };
    // UNSAFE(slice): a short raw copy of the fresh duplicate (STRDUP-FAST).
    // SAFETY: the function's contract.
    unsafe { d.wrapping_add(off).cast::<T>().write_unaligned(v) }
}

/// Copy `n <= 16` bytes with at most three (overlapping) moves of one width.
///
/// # Safety
/// `s` readable and `d` writable for `n` bytes, not overlapping each other; `n <= 16`.
// UNSAFE(slice): the moves stay inside `0..n` of the two regions of `fresh_copy_z`.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn small_copy(d: *mut u8, s: *const u8, n: usize) {
    if n >= 8 {
        // UNSAFE(slice): a short raw copy of the fresh duplicate (STRDUP-FAST).
        // SAFETY: n >= 8: both 8-byte windows lie in 0..n.
        unsafe { mv::<u64>(d, s, 0) };
        // UNSAFE(slice): a short raw copy of the fresh duplicate (STRDUP-FAST).
        // SAFETY: as above.
        unsafe { mv::<u64>(d, s, n.wrapping_sub(8)) };
    } else if n >= 4 {
        // UNSAFE(slice): a short raw copy of the fresh duplicate (STRDUP-FAST).
        // SAFETY: 4 <= n < 8: both 4-byte windows lie in 0..n.
        unsafe { mv::<u32>(d, s, 0) };
        // UNSAFE(slice): a short raw copy of the fresh duplicate (STRDUP-FAST).
        // SAFETY: as above.
        unsafe { mv::<u32>(d, s, n.wrapping_sub(4)) };
    } else if n != 0 {
        // UNSAFE(slice): a short raw copy of the fresh duplicate (STRDUP-FAST).
        // SAFETY: 1 <= n < 4: offsets 0, n / 2 and n - 1 lie in 0..n.
        unsafe { mv::<u8>(d, s, 0) };
        // UNSAFE(slice): a short raw copy of the fresh duplicate (STRDUP-FAST).
        // SAFETY: as above.
        unsafe { mv::<u8>(d, s, n >> 1) };
        // UNSAFE(slice): a short raw copy of the fresh duplicate (STRDUP-FAST).
        // SAFETY: as above.
        unsafe { mv::<u8>(d, s, n.wrapping_sub(1)) };
    }
}

/// The `errno` cell `__errno_location` returned.
///
/// Proof: the C ABI (spec Appendix C.9) guarantees a valid, aligned `int` for the calling
/// thread's lifetime; it is only ever accessed as an atomic here, and the returned reference
/// is used at once by the caller (never stored).
pub(crate) fn errno_cell<'a>(p: *mut i32) -> Option<&'a AtomicI32> {
    if p.is_null() {
        return None;
    }
    // UNSAFE(slice): the C library's per-thread cell becomes a reference.
    #[allow(unsafe_code)]
    // SAFETY: see the proof above; `AtomicI32` has the layout of `i32`.
    unsafe {
        Some(AtomicI32::from_ptr(p))
    }
}

/// Copy `src` into the start of `dst`; returns the units copied (the shorter length). The two
/// are disjoint by construction (a `&mut` and a `&`). In the measured size range the AVX2 copy
/// runs when the CPU and OS support it (owner ruling 2026-10-05, PJ-7); otherwise large copies
/// use `rep movsb` when the CPU has ERMS (OD-C2 b, P7), except where it is slow
/// (`erms_placement_ok`); the other copies from 128 bytes take the AVX2 or SSE2 copy (any size,
/// COPY-INTERNAL); the rest use the `liella-mem` loops, in descending address order when the
/// destination sits a short distance above the source modulo 4 KiB (`back_better`).
pub fn copy_units<T: Unit>(dst: &mut [T], src: &[T]) -> usize {
    let n = dst.len().min(src.len());
    let bytes = n.saturating_mul(size_of::<T>());
    if bytes < LOOPS_ONLY_BELOW {
        return bytes::copy(dst, src);
    }
    copy_large(dst, src, n, bytes)
}

/// [`copy_units`] from [`LOOPS_ONLY_BELOW`] bytes on: kept out of line so that small copies
/// pay none of the path choice (no flag load, no saved registers; PJ-7 measured 1.5 ns at 64
/// bytes on shiki with it inline).
#[inline(never)]
fn copy_large<T: Unit>(dst: &mut [T], src: &[T], n: usize, bytes: usize) -> usize {
    let (da, sa) = (dst.as_ptr().addr(), src.as_ptr().addr());
    if bytes < crate::cpu::AVX2_COPY_END && crate::cpu::copy_avx2() && crate::dispatch::avx2() && bytes >= crate::cpu::AVX2_COPY_MIN {
        // The CPU class's `rep movsb` window (MEMCPY-POLICY-2), else the AVX2 copy. Proof
        // obligations of the declarations (`ffi`): both regions valid and disjoint (a `&mut`
        // and a `&`), 64 <= 128 <= bytes, AVX2 (the flags above) for the AVX2 copy.
        if crate::hot::avx2_erms(da, sa, bytes) {
            let _ = ffi::liella_machine_copy_erms_a64(dst.as_mut_ptr().cast(), src.as_ptr().cast(), bytes);
        } else {
            ffi::liella_machine_copy_avx2(dst.as_mut_ptr().cast(), src.as_ptr().cast(), bytes);
        }
        return n;
    }
    // `rep movsb`, plain or head-aligned, on ERMS CPUs (`hot::erms_kernel`,
    // MEMCPY-NONAVX2-THRESHOLD). Proof obligations of the declarations (`ffi`): both regions
    // valid and disjoint (a `&mut` and a `&`), 64 <= 128 <= bytes.
    let k = if bytes >= crate::cpu::copy_erms_min() && crate::cpu::erms() { crate::hot::erms_kernel(da, sa, bytes) } else { crate::hot::Bulk::Sse2 };
    if k == crate::hot::Bulk::ErmsA64 {
        let _ = ffi::liella_machine_copy_erms_a64(dst.as_mut_ptr().cast(), src.as_ptr().cast(), bytes);
        n
    } else if k == crate::hot::Bulk::Erms {
        ffi::liella_machine_copy_erms(dst.as_mut_ptr().cast(), src.as_ptr().cast(), bytes);
        n
    } else if bytes >= crate::cpu::AVX2_COPY_MIN && crate::cpu::copy_avx2() && crate::dispatch::avx2() {
        // Above 64 KiB where `rep movsb` is not used (no ERMS, e.g. Zen 1): the AVX2 copy, not the
        // loops (COPY-INTERNAL: 64 KiB + 1 took 39,100 instructions in the loops on Zen 1, the
        // kernel picks the order from the placement itself). Proof obligation of the declaration
        // (`ffi`): AVX2 (the flags above), 64 <= 128 <= bytes, both regions valid and disjoint.
        ffi::liella_machine_copy_avx2(dst.as_mut_ptr().cast(), src.as_ptr().cast(), bytes);
        n
    } else if bytes >= crate::cpu::AVX2_COPY_MIN {
        // Without AVX2 (MEM-FALLBACK): the SSE2 build of the same copy, which picks the order
        // from the placement as the AVX2 one does; above 64 KiB too where `rep movsb` is not used
        // (COPY-INTERNAL). Proof obligation of the declaration (`ffi`): 64 <= 128 <= bytes, both
        // regions valid (a `&mut [T]` and a `&[T]` of `n` units).
        let _ = ffi::liella_machine_move_sse2(dst.as_mut_ptr().cast(), src.as_ptr().cast(), bytes);
        n
    } else if back_better(da, sa) {
        bytes::copy_back(dst, src)
    } else {
        bytes::copy(dst, src)
    }
}

/// Below this many bytes every copy takes the forward loops: the placement makes no measurable
/// difference there and the checks would cost more than they save (measured, PJ-7).
const LOOPS_ONLY_BELOW: usize = 128;

/// Whether the loops should copy from the end: the destination starts 1 to 255 bytes after the
/// source modulo 4 KiB, where an ascending copy's loads share their low 12 address bits with
/// stores issued shortly before (4 KiB aliasing). Measured (PJ-7), 4 KiB at +100 bytes: 161
/// against 85 ns on Haswell (shiki), 116 against 98 on Zen 1 (keke); at +16 and +511 the two
/// orders are within 6% on both, the descending one the slower at +511 on Zen 1.
fn back_better(dst: usize, src: usize) -> bool {
    dst.wrapping_sub(src).wrapping_sub(1) & 4095 < 255
}

/// Bytes above which `rep movsb` is used whatever the placement: beyond the largest size
/// measured. On Haswell (shiki, PJ-7) the slow placements stay slow up to 64 KiB: 16 KiB at
/// +16 bytes took 1045 ns against 344 for the loops.
const ERMS_ANY_PLACEMENT: usize = 65_537;

/// Whether `rep movsb` is fast for `n` bytes to `dst` from `src`. It is not when the destination
/// starts less than 512 bytes after the source, modulo 4 KiB, at a distance that is not a
/// multiple of 32, and the copy is 64 KiB or shorter: measured on Haswell (shiki, Xeon
/// E5-2697 v3), 4 KiB at +16 bytes took 345 ns against 83 ns for the loops (glibc: 49), 2 KiB
/// 194 against 47, 512 bytes 50 against 14, 16 KiB 1045 against 344 (PJ-7); at multiples of 32
/// (0 included) the reference library and `rep movsb` agree. Such copies use the loops.
pub(crate) fn erms_placement_ok(dst: usize, src: usize, n: usize) -> bool {
    let d = dst.wrapping_sub(src) & 4095;
    n >= ERMS_ANY_PLACEMENT || d >= 512 || d & 31 == 0
}

/// The native interface's text output (D-X.10): `src` is copied into the start of `out` only if
/// it fits whole, and its length is returned either way, so a caller whose buffer was too small
/// learns the size it needs and nothing partial is ever delivered.
pub(crate) fn copy_out(src: &[u8], out: &mut [u8]) -> usize {
    if let Some(dst) = out.get_mut(..src.len()) {
        copy_units(dst, src);
    }
    src.len()
}

/// Set every byte of `dst` to `v` (`memset`): `rep stosb` for large fills on ERMS CPUs. The
/// bytes before the first 64-byte boundary are set by the loops first, so `rep stosb` always
/// starts aligned: measured on Haswell (shiki), 4 KiB at 16 bytes past a boundary took 86 ns
/// against 42 aligned (glibc: 82 there).
pub fn fill(dst: &mut [u8], v: u8) {
    if dst.len() >= crate::cpu::fill_erms_min() && crate::cpu::erms() {
        let head = dst.as_ptr().addr().wrapping_neg() & 63;
        let (lead, rest) = dst.split_at_mut(head.min(dst.len()));
        bytes::fill(lead, v);
        ffi::liella_machine_fill_erms(rest.as_mut_ptr(), i32::from(v), rest.len());
    } else if dst.len() > 128 && crate::dispatch::avx2() {
        // Without `rep stosb` (MEM-FALLBACK): the AVX2 fill. Proof obligation of the declaration
        // (`ffi`): AVX2 with the YMM state saved (the kernel flag; before the startup decision it
        // is false and the loop runs), 128 < len, `dst` a `&mut [u8]`.
        let _ = ffi::liella_machine_fill_avx2(dst.as_mut_ptr(), i32::from(v), dst.len());
    } else {
        bytes::fill(dst, v);
    }
}

/// How common a byte is in text, high = common (the pair scanner prefers the rarest bytes of the
/// needle after its first and last have produced a false candidate).
pub(crate) fn commonness(b: u8) -> u8 {
    match b {
        b' ' => 255,
        b'e' => 250,
        b't' => 245,
        b'a' | b'o' | b'i' | b'n' => 235,
        b's' | b'h' | b'r' => 225,
        b'd' | b'l' | b'u' | b'c' | b'm' => 210,
        b'j' | b'k' | b'q' | b'x' | b'z' | b'v' | b'b' | b'p' | b'y' | b'g' | b'f' | b'w' => 160,
        b'.' | b',' | b'-' | b'_' | b'/' | b'\n' | b'\t' | b'=' | b'(' | b')' | b':' | b'\'' | b'"' | 0 => 150,
        b'0'..=b'9' => 130,
        b'A'..=b'Z' => 120,
        _ => 40,
    }
}

/// The offsets `o0 < o1` of the two rarest bytes of `needle` (at least two bytes), each byte
/// ranked after `key` (the fold of a case-insensitive search).
fn rare_pair(needle: &[u8], key: impl Fn(u8) -> u8) -> (usize, usize) {
    rare_pair_by(needle, |c| commonness(key(c)))
}

/// The positions (ascending) of the two units of `needle` (two or more units) with the lowest
/// `rank` (ties to the earlier unit): `rare_pair`, and `strz::find_str` / `find_wcs` (STRSTR-VEC).
pub(crate) fn rare_pair_by<T: Copy>(needle: &[T], rank: impl Fn(T) -> u8) -> (usize, usize) {
    let (mut a, mut b) = ((u16::MAX, 0usize), (u16::MAX, 1usize));
    for (i, &c) in needle.iter().enumerate() {
        let r = u16::from(rank(c));
        if r < a.0 {
            b = a;
            a = (r, i);
        } else if r < b.0 {
            b = (r, i);
        }
    }
    (a.1.min(b.1), a.1.max(b.1))
}

/// The first occurrence of `needle` in `hay` (`memmem`, `strstr`, the regex literal prefilter):
/// the same answer as `bytes::find_seq` with the identity fold (an empty needle matches at 0),
/// found by the C layer's pair scanner (FIND-SEQ-AVX2, D-4.28): it names each position where two
/// bytes of the needle (first and last; after a false candidate its two rarest) both match (32
/// positions per step on AVX2, 16 on SSE2) and Rust compares the whole needle there, so a wrong
/// answer needs both a scanner bug and a missed candidate. Fewer than 16 positions are checked by
/// `memchr` for the first byte and a compare. Work spent on candidates that fail is counted, and
/// past a linear bound the rest goes to `bytes::find_seq` (the worst case stays linear).
pub fn find_slice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    find_core(hay, needle, false)
}

/// `commonness` of a wide unit: a unit above 255 counts as rare (0), as for `wcsstr`'s candidates.
pub(crate) fn unit_commonness(u: i32) -> u8 {
    u8::try_from(u).map_or(0, commonness)
}

/// Whether `l`'s byte `tolower` is exactly the ASCII fold (A-Z to a-z, every other byte itself):
/// true for the C, POSIX and UTF-8 locales (a byte of 0x80 and up is no character alone). The
/// answer for the last locale asked is cached (a pointer, low bit = answer), so a call costs one
/// load and compare; a racing store of another locale only costs a recomputation.
fn ascii_fold(l: &'static liella::locale::Locale) -> bool {
    use core::sync::atomic::{AtomicUsize, Ordering};
    static LAST: AtomicUsize = AtomicUsize::new(0);
    let key = core::ptr::from_ref(l).addr();
    let seen = LAST.load(Ordering::Relaxed);
    if seen != 0 && (seen & !1) == (key & !1) {
        return (seen & 1) == 1;
    }
    let ok = (0..=255u8).all(|b| liella::ctype::to_lower(b, l) == b.to_ascii_lowercase());
    LAST.store((key & !1) | usize::from(ok), Ordering::Relaxed);
    ok
}

/// `strcasestr`'s search: the first position where `needle` matches `hay` under the locale's
/// byte fold (`locale::lower`). In a locale whose fold is the ASCII fold the pair scanner finds
/// candidates (an ASCII letter of the pair matches both cases: `liella_machine_pairscan_ci`) and
/// the compare at each is `eq_ignore_ascii_case`; any other locale takes `bytes::find_seq` with
/// the locale's fold. Same candidate policy, verification and linear bound as `find_slice`.
pub fn find_slice_ci(hay: &[u8], needle: &[u8]) -> Option<usize> {
    let l = crate::locale::current();
    if ascii_fold(l) {
        find_core(hay, needle, true)
    } else {
        bytes::find_seq(hay, needle, crate::locale::lower(l))
    }
}

/// `find_slice` (`ci` false) and `find_slice_ci` under the ASCII fold (`ci` true).
fn find_core(hay: &[u8], needle: &[u8], ci: bool) -> Option<usize> {
    let (Some(&b0), Some(&b1)) = (needle.first(), needle.last()) else { return Some(0) };
    let m = needle.len();
    let count = hay.len().checked_sub(m)?.saturating_add(1);
    let fold = |c: u8| if ci { c.to_ascii_lowercase() } else { c };
    if m == 1 {
        return if ci { hay.iter().position(|&c| fold(c) == fold(b0)) } else { crate::strz::find_in(hay, b0) };
    }
    let last = m.saturating_sub(1);
    // The candidate at `at`: the whole needle there (a writer may have changed the scanned bytes).
    let hit = |at: usize| match hay.get(at..at.saturating_add(m)) {
        Some(w) => {
            if ci {
                w.eq_ignore_ascii_case(needle)
            } else {
                w == needle
            }
        }
        None => false,
    };
    let mut from = 0usize;
    if count >= 16 {
        crate::cpu::ready();
        let (mut o0, mut o1) = (0usize, last);
        let mut tuned = false;
        let mut wasted = 0usize;
        while count.saturating_sub(from) >= 16 {
            let rest = count.saturating_sub(from);
            let h = hay.get(from.saturating_add(o0)..)?;
            let (c0, c1) = (needle.get(o0).copied()?, needle.get(o1).copied()?);
            let off = o1.saturating_sub(o0);
            let i = if ci {
                let flags = match (c0.is_ascii_alphabetic(), c1.is_ascii_alphabetic()) {
                    (false, false) => 0,
                    (true, false) => 1,
                    (false, true) => 2,
                    (true, true) => 3,
                };
                ffi::liella_machine_pairscan_ci(h.as_ptr(), rest, off, i32::from(fold(c0)), i32::from(fold(c1)), flags)
            } else {
                ffi::liella_machine_pairscan(h.as_ptr(), rest, off, i32::from(c0), i32::from(c1))
            };
            if i >= rest {
                if i == rest {
                    return None;
                }
                crate::strz::scanner_fault();
            }
            let at = from.saturating_add(i);
            if hit(at) {
                return Some(at);
            }
            from = at.saturating_add(1);
            wasted = wasted.saturating_add(m);
            if wasted > from.saturating_mul(4).saturating_add(256) {
                return hay.get(from..).and_then(|w| bytes::find_seq(w, needle, fold)).map(|k| from.saturating_add(k));
            }
            if !tuned {
                tuned = true;
                (o0, o1) = rare_pair(needle, fold);
            }
        }
    }
    // Fewer than 16 positions left: the first byte by the bounded `memchr` scanner, then the rest.
    let mut at = from;
    while at < count {
        let w = hay.get(at..count)?;
        let p = at.saturating_add(if ci { w.iter().position(|&c| fold(c) == fold(b0))? } else { crate::strz::find_in(w, b0)? });
        if hay.get(p.saturating_add(last)).is_some_and(|&c| fold(c) == fold(b1)) && hit(p) {
            return Some(p);
        }
        at = p.saturating_add(1);
    }
    None
}

/// `memcmp`'s order of `a` and `b` (equal lengths in every caller; a longer slice is compared
/// over the shorter length, then by length). The C layer finds the first difference; the check
/// here confirms the bytes there differ (`settle_compare`), and the order comes from those two
/// bytes.
pub fn compare(a: &[u8], b: &[u8]) -> core::cmp::Ordering {
    crate::cpu::ready();
    let n = a.len().min(b.len());
    let i = ffi::liella_machine_memcmp(a.as_ptr(), b.as_ptr(), n);
    settle_compare(a, b, i)
}

/// Check the answer `i` the `memcmp` scanner gave for `a` and `b` (D-4.21): past the shorter
/// length is a scanner bug and aborts; equal bytes at `i` were changed by a writer during the
/// call, so the rest is compared once more and that pass's own reads decide the order.
#[inline]
pub(crate) fn settle_compare(a: &[u8], b: &[u8], i: usize) -> core::cmp::Ordering {
    let n = a.len().min(b.len());
    if i == n {
        return a.len().cmp(&b.len());
    }
    match (a.get(i), b.get(i)) {
        (Some(x), Some(y)) if i < n => {
            if x != y {
                x.cmp(y)
            } else {
                rescan_compare(a, b, i)
            }
        }
        _ => crate::strz::scanner_fault(),
    }
}

/// `settle_compare`'s rescan: the order of the first bytes after `i` that differ, else by length.
#[cold]
#[inline(never)]
fn rescan_compare(a: &[u8], b: &[u8], i: usize) -> core::cmp::Ordering {
    crate::strz::note_rescan();
    a.iter().zip(b).skip(i.saturating_add(1)).find(|(x, y)| x != y).map_or_else(|| a.len().cmp(&b.len()), |(x, y)| x.cmp(y))
}

/// Move `n` units from `s` to `d` (`memmove`, and `memcpy` per D-4.2): the units moved.
///
/// No value wider than two words is built on this path (D-4.13): the compiler lowers wider
/// copies to calls to `memcpy`, which in the products is this function's own caller. So the
/// regions go to `liella::bytes` as slices (passed in registers), never as a `Move`.
///
/// # Safety
/// `d` is valid for writing and `s` for reading `n` units, and nothing else references them
/// during the call (the `memmove` contract).
// UNSAFE(slice): two C regions become slices; overlap decides between two views and one.
#[allow(unsafe_code)]
pub(crate) unsafe fn move_units<T: Unit>(d: *mut c_void, s: *const c_void, n: usize) -> KResult<usize> {
    let bytes = n.checked_mul(size_of::<T>()).filter(|b| isize::try_from(*b).is_ok()).ok_or(Errno::EFAULT)?;
    if n == 0 {
        return Ok(0);
    }
    if d.is_null() || s.is_null() {
        return Err(Errno::EFAULT);
    }
    // UNSAFE(slice): the regions after the checks.
    // SAFETY: the caller's contract; n > 0, both non-NULL, `bytes` = n units, checked above.
    Ok(unsafe { move_regions::<T>(d, s, n, bytes) })
}

/// [`move_units`] after its checks, also the library's internal `memcpy` and `memmove`
/// (D-X.15): `n` units (`bytes` bytes) from `s` to `d`, correct under overlap; the units moved.
///
/// # Safety
/// `n > 0`, `bytes == n * size_of::<T>() <= isize::MAX`, `d` and `s` non-NULL, and the
/// [`move_units`] contract.
// UNSAFE(slice): two C regions become slices; overlap decides between two views and one.
#[allow(unsafe_code)]
pub(crate) unsafe fn move_regions<T: Unit>(d: *mut c_void, s: *const c_void, n: usize, bytes: usize) -> usize {
    let (da, sa) = (d.addr(), s.cast::<u8>().addr());
    let overlap = da < sa.wrapping_add(bytes) && sa < da.wrapping_add(bytes);
    if !overlap {
        // UNSAFE(slice): two disjoint C regions become slices.
        #[allow(unsafe_code)]
        // SAFETY: the caller's contract; the regions are disjoint, so the views do not alias.
        let dst = unsafe { core::slice::from_raw_parts_mut(d.cast::<T>(), n) };
        // UNSAFE(slice): the source region.
        #[allow(unsafe_code)]
        // SAFETY: as above.
        let src = unsafe { core::slice::from_raw_parts(s.cast::<T>(), n) };
        return copy_units(dst, src);
    }
    // Overlapping, from 128 bytes on with AVX2, any size: the assembly copy, which picks the
    // direction from the overlap (MEM-BULK; the loops below took 4x its instructions).
    if bytes >= crate::cpu::AVX2_COPY_MIN && crate::cpu::copy_avx2() && crate::dispatch::avx2() {
        // Proof obligation of the declaration (`ffi`): AVX2 (the flags above), 64 <= 128 <= bytes,
        // both regions valid for `bytes` bytes and non-NULL (this function's contract).
        let _ = ffi::liella_machine_move_avx2(d.cast(), s.cast(), bytes);
        return n;
    }
    // Without AVX2 (MEM-FALLBACK): the SSE2 build of the same copy, any size from 128 bytes.
    if bytes >= crate::cpu::AVX2_COPY_MIN {
        // Proof obligation of the declaration (`ffi`): 64 <= 128 <= bytes, both regions valid for
        // `bytes` bytes and non-NULL (this function's contract).
        let _ = ffi::liella_machine_move_sse2(d.cast(), s.cast(), bytes);
        return n;
    }
    // One view from the lower start to the higher end (contiguous because the regions overlap).
    let (lo, lo_ptr) = if da <= sa { (da, d.cast::<T>()) } else { (sa, s.cast_mut().cast::<T>()) };
    let unit = size_of::<T>().max(1);
    let span = da.max(sa).wrapping_sub(lo).wrapping_add(bytes).checked_div(unit).unwrap_or(0);
    let (so, dof) = (sa.wrapping_sub(lo).checked_div(unit).unwrap_or(0), da.wrapping_sub(lo).checked_div(unit).unwrap_or(0));
    // UNSAFE(slice): the union of two overlapping C regions becomes one slice.
    #[allow(unsafe_code)]
    // SAFETY: both regions are valid (caller's contract) and they overlap, so their union is
    // one contiguous valid region; it is the only view created.
    let buf = unsafe { core::slice::from_raw_parts_mut(lo_ptr, span) };
    if bytes::move_within(buf, so, dof, n) { n } else { 0 }
}

// ------------------------------------------------------------------------- internal copies

// The library's own memory copies and fills (D-X.15). rustc lowers slice copies, moves and
// fills to calls to `memcpy`, `memmove` and `memset`, and the C layer calls them too; those
// callers are library code whose bounds are already checked, so they need neither the exported
// functions' C-contract checks nor an interposable entry. `cargo xtask so` (`xtask/src/copies.rs`)
// points those calls at these entries and makes them direct; the exports are unchanged. The
// bodies are the exports' after their checks, so they never lower to the names they replace
// (D-4.11, D-4.13), which the pass checks.

/// `memcpy`/`memmove` for the library itself: `n` bytes from `s` to `d`, correct under overlap
/// (LLVM may copy a value onto itself); returns `d`.
///
/// # Safety
/// `n == 0`, or `d` is valid for writing and `s` for reading `n` bytes (`n <= isize::MAX`, as
/// for any region Rust or the C layer holds), not otherwise referenced during the call.
// UNSAFE(slice): the regions of a library-internal copy become slices.
#[allow(unsafe_code)]
pub(crate) unsafe fn internal_move(d: *mut c_void, s: *const c_void, n: usize) -> *mut c_void {
    // SAFETY: the function's contract; sizes 1 to 128 take the hot kernel (D-4.22), 129 to
    // 64 KiB the export's large kernels (COPY-INTERNAL).
    if unsafe { crate::hot::internal_move(d, s, n) } {
        return d;
    }
    if n != 0 {
        // UNSAFE(slice) SAFETY: the function's contract; n > 0, so both are non-NULL.
        unsafe { move_regions::<u8>(d, s, n, n) };
    }
    d
}

/// `memset` for the library itself: `n` bytes at `d` set to `c` (as `unsigned char`); returns `d`.
///
/// # Safety
/// `n == 0`, or `d` is valid for writing `n` bytes (`n <= isize::MAX`), not otherwise
/// referenced during the call.
// UNSAFE(slice): the region of a library-internal fill becomes a slice.
#[allow(unsafe_code)]
pub(crate) unsafe fn internal_fill(d: *mut c_void, c: c_int, n: usize) -> *mut c_void {
    // SAFETY: the function's contract; sizes 1 to 128 take the hot kernel (D-4.22).
    if unsafe { crate::hot::internal_fill(d, byte(c), n) } {
        return d;
    }
    if n != 0 {
        // UNSAFE(slice) SAFETY: the function's contract; n > 0, so d is non-NULL.
        fill(unsafe { core::slice::from_raw_parts_mut(d.cast::<u8>(), n) }, byte(c));
    }
    d
}

crate::internal_entry! {
/// The library's internal `memcpy` (D-X.15).
pub extern "C" fn liella_internal_memcpy(d: *mut c_void, s: *const c_void, n: usize) -> *mut c_void {
    // UNSAFE(slice): a copy the library's own code makes.
    // SAFETY: its callers pass regions their bounds checks or types guarantee.
    unsafe { internal_move(d, s, n) }
}}

crate::internal_entry! {
/// The library's internal `memmove` (D-X.15).
pub extern "C" fn liella_internal_memmove(d: *mut c_void, s: *const c_void, n: usize) -> *mut c_void {
    // UNSAFE(slice): a move the library's own code makes.
    // SAFETY: as for `liella_internal_memcpy`.
    unsafe { internal_move(d, s, n) }
}}

crate::internal_entry! {
/// The library's internal `memset` (D-X.15).
pub extern "C" fn liella_internal_memset(d: *mut c_void, c: c_int, n: usize) -> *mut c_void {
    // UNSAFE(slice): a fill the library's own code makes.
    // SAFETY: as for `liella_internal_memcpy`.
    unsafe { internal_fill(d, c, n) }
}}

/// `n` readable `wchar_t` units at `p`.
///
/// # Safety
/// As for [`buf`], in units of four bytes.
// UNSAFE(slice): a C wide array becomes a slice.
#[allow(unsafe_code)]
#[allow(dead_code)]
pub(crate) unsafe fn buf_w<'a>(p: *const i32, n: usize) -> KResult<&'a [i32]> {
    if n == 0 {
        return Ok(&[]);
    }
    if p.is_null() || n.checked_mul(4).is_none_or(|b| isize::try_from(b).is_err()) {
        return Err(Errno::EFAULT);
    }
    // UNSAFE(slice): see the function's safety contract.
    #[allow(unsafe_code)]
    // SAFETY: non-NULL, size checked; validity is the caller's contract.
    unsafe {
        Ok(core::slice::from_raw_parts(p, n))
    }
}

/// `n` writable `wchar_t` units at `p`.
///
/// # Safety
/// As for [`buf_mut`], in units of four bytes.
// UNSAFE(slice): a C wide array becomes a mutable slice.
#[allow(unsafe_code)]
#[allow(dead_code)]
pub(crate) unsafe fn buf_w_mut<'a>(p: *mut i32, n: usize) -> KResult<&'a mut [i32]> {
    if n == 0 {
        return Ok(&mut []);
    }
    if p.is_null() || n.checked_mul(4).is_none_or(|b| isize::try_from(b).is_err()) {
        return Err(Errno::EFAULT);
    }
    // UNSAFE(slice): see the function's safety contract.
    #[allow(unsafe_code)]
    // SAFETY: non-NULL, size checked; validity is the caller's contract.
    unsafe {
        Ok(core::slice::from_raw_parts_mut(p, n))
    }
}

/// `n` elements of `size` bytes at `p` as one byte slice (`qsort`, `bsearch`, `lfind`).
///
/// # Safety
/// `p` is valid for reading and writing `n * size` bytes, not otherwise referenced.
// UNSAFE(slice): a C array of opaque elements becomes a byte slice.
#[allow(unsafe_code)]
#[allow(dead_code)]
pub(crate) unsafe fn elements_mut<'a>(p: *mut c_void, n: usize, size: usize) -> KResult<&'a mut [u8]> {
    let bytes = n.checked_mul(size).ok_or(Errno::EINVAL)?;
    // SAFETY: the caller's contract, for `bytes` bytes.
    unsafe { buf_mut(p, bytes) }
}

/// A C address as an integer (for the storage traits of `liella::search`).
pub(crate) fn addr<T>(p: *const T) -> usize {
    p.expose_provenance()
}

/// `(unsigned char)c`: the byte a C `int` argument denotes (`memchr`, `memset`, `strchr`).
pub(crate) fn byte(c: i32) -> u8 {
    c.to_le_bytes()[0]
}

/// A C destination whose length the operation decides ([`Dest`]), optionally limited by a
/// fortify size (D-C0.5): a request beyond the limit calls `__chk_fail`.
pub(crate) struct CDest<T> {
    p: *mut T,
    limit: Option<usize>,
}

/// A destination at `p`, in units of `T` (`char` pointers are viewed as bytes).
///
/// # Safety
/// `p` is valid for writing every length the export's contract lets the operation request,
/// and nothing else references those units meanwhile.
// UNSAFE(slice): constructing a destination whose later views rely on the C contract.
#[allow(unsafe_code)]
pub(crate) unsafe fn dest<T, P>(p: *mut P) -> CDest<T> {
    CDest { p: p.cast::<T>(), limit: None }
}

/// A fortified destination: at most `destlen_units` units (`usize::MAX`: unknown size).
///
/// # Safety
/// As for [`dest`], within `destlen_units`.
// UNSAFE(slice): as for `dest`, bounded by the compiler-known object size.
#[allow(unsafe_code)]
pub(crate) unsafe fn dest_chk<T, P>(p: *mut P, destlen_units: usize) -> CDest<T> {
    CDest { p: p.cast::<T>(), limit: (destlen_units != usize::MAX).then_some(destlen_units) }
}

impl<T> Dest<T> for CDest<T> {
    fn get(&mut self, n: usize) -> KResult<&mut [T]> {
        if self.limit.is_some_and(|l| n > l) {
            crate::ret::chk_fail();
        }
        if n == 0 {
            return Ok(&mut []);
        }
        if self.p.is_null() || n.checked_mul(size_of::<T>()).is_none_or(|b| isize::try_from(b).is_err()) {
            return Err(Errno::EFAULT);
        }
        // UNSAFE(slice): the destination's units become a slice when the operation asks.
        #[allow(unsafe_code)]
        // SAFETY: the constructor's contract covers every length the operation requests.
        unsafe {
            Ok(core::slice::from_raw_parts_mut(self.p, n))
        }
    }
}

/// A fortify check for a count-delimited call (`__memcpy_chk`): `n` units must fit in
/// `destlen_units`; otherwise `__chk_fail`.
#[inline(always)]
pub(crate) fn chk(n: usize, destlen_units: usize) -> usize {
    // One compare for the fits(n, destlen, 1) rule: `usize::MAX` (size unknown) always fits.
    if n > destlen_units {
        crate::ret::chk_fail();
    }
    n
}

/// Text at C addresses, for operations that hold positions as addresses across calls (the
/// tokenizers' cursors).
pub(crate) struct CText;

/// Access to C text by address.
///
/// # Safety
/// Every address the operation passes is one the export's C contract makes a valid, writable,
/// NUL-terminated string (the argument, or a position inside it saved by an earlier call).
// UNSAFE(slice): text reached through saved addresses relies on the C contract.
#[allow(unsafe_code)]
pub(crate) unsafe fn ctext() -> CText {
    CText
}

impl liella_legacy::string_compat::TextAt<u8> for CText {
    fn text_at(&mut self, addr: usize) -> Option<&mut [u8]> {
        // UNSAFE(slice): see `ctext`.
        #[allow(unsafe_code)]
        // SAFETY: the constructor's contract.
        let t = unsafe { crate::strz::strz_mut(core::ptr::with_exposed_provenance_mut(addr)) };
        t.ok()
    }
}

impl liella_legacy::string_compat::TextAt<i32> for CText {
    fn text_at(&mut self, addr: usize) -> Option<&mut [i32]> {
        // UNSAFE(slice): see `ctext`.
        #[allow(unsafe_code)]
        // SAFETY: the constructor's contract.
        let t = unsafe { crate::strz::wcsz_mut(core::ptr::with_exposed_provenance_mut(addr)) };
        t.ok()
    }
}

/// The address of a C variable the library reads and updates later through it (`getline`'s
/// `*lineptr` and `*n`, `open_memstream`'s `*ptr` and `*size`); NULL is `EINVAL`.
pub(crate) fn var<T>(v: Option<&mut T>) -> KResult<usize> {
    v.map(|r| core::ptr::from_mut(r).expose_provenance()).ok_or(Errno::EINVAL)
}

/// A C pointer variable (`char **`) as an address cell (`strtok_r`'s `saveptr`, `strsep`'s
/// `stringp`); NULL is `EFAULT` (D-X.8: the variable arrives as `Option<&mut *mut T>`).
pub(crate) fn cell<T>(v: Option<&mut *mut T>) -> KResult<PtrCell<'_, T>> {
    v.map(PtrCell).ok_or(Errno::EFAULT)
}

/// An optional end pointer (`strtol`'s `endptr`) as an address cell.
pub(crate) fn end<T>(v: Option<&mut *mut T>) -> Option<PtrCell<'_, T>> {
    v.map(PtrCell)
}

/// A C `const T *` variable (`mbsrtowcs`'s `src`) as an address cell; NULL is `EFAULT`.
pub(crate) fn const_cell<T>(v: Option<&mut *const T>) -> KResult<ConstCell<'_, T>> {
    v.map(ConstCell).ok_or(Errno::EFAULT)
}

/// See [`const_cell`].
pub(crate) struct ConstCell<'a, T>(&'a mut *const T);

impl<T> liella_legacy::string_compat::Cursor for ConstCell<'_, T> {
    fn get(&self) -> usize {
        self.0.expose_provenance()
    }
    fn set(&mut self, addr: usize) {
        *self.0 = core::ptr::with_exposed_provenance(addr);
    }
}

/// See [`cell`].
pub(crate) struct PtrCell<'a, T>(&'a mut *mut T);

impl<T> PtrCell<'_, T> {
    /// The address of the C variable itself (`tdelete` reports it when the tree empties).
    pub(crate) fn var_addr(&self) -> usize {
        core::ptr::from_ref::<*mut T>(self.0).expose_provenance()
    }
}

impl<T> liella_legacy::string_compat::Cursor for PtrCell<'_, T> {
    fn get(&self) -> usize {
        self.0.expose_provenance()
    }
    fn set(&mut self, addr: usize) {
        *self.0 = core::ptr::with_exposed_provenance_mut(addr);
    }
}

/// `iconv`'s arguments, by what the call does.
pub(crate) enum IconvArgs<'a> {
    /// No input and no output buffer: back to the initial state.
    Reset,
    /// No input, an output buffer: write the shift sequence (none) and reset.
    Flush,
    /// Convert the input into the output.
    Convert(IconvIn<'a>, IconvOut<'a>),
}

/// `iconv`'s input: `*inbuf` and `*inbytesleft`.
pub(crate) struct IconvIn<'a> {
    ptr: &'a mut *mut i8,
    left: &'a mut usize,
    data: &'a [u8],
}

impl IconvIn<'_> {
    /// The bytes left to convert.
    pub(crate) fn bytes(&self) -> &[u8] {
        self.data
    }

    /// Record that `n` bytes were consumed.
    pub(crate) fn advance(&mut self, n: usize) {
        *self.ptr = self.ptr.wrapping_add(n);
        *self.left = self.left.saturating_sub(n);
    }
}

/// `iconv`'s output: `*outbuf` and `*outbytesleft` (absent: no room).
pub(crate) struct IconvOut<'a> {
    cells: Option<(&'a mut *mut i8, &'a mut usize)>,
    data: &'a mut [u8],
}

impl IconvOut<'_> {
    /// The room left.
    pub(crate) fn room(&mut self) -> &mut [u8] {
        self.data
    }

    /// Record that `n` bytes were written.
    pub(crate) fn advance(&mut self, n: usize) {
        if let Some((p, left)) = self.cells.as_mut() {
            **p = p.wrapping_add(n);
            **left = left.saturating_sub(n);
        }
    }
}

/// The buffers `iconv`'s pointer and count variables describe (spec §6.4): a NULL `inbuf` or
/// `*inbuf` is a reset, or a flush when an output buffer is given.
///
/// # Safety
/// `*inbuf` points to `*inbytesleft` readable bytes and `*outbuf` to `*outbytesleft` writable
/// bytes, distinct, not otherwise referenced during the call (the iconv contract).
// UNSAFE(slice): the caller's conversion buffers become slices.
#[allow(unsafe_code)]
pub(crate) unsafe fn iconv_args<'a>(inbuf: Option<&'a mut *mut i8>, inleft: Option<&'a mut usize>, outbuf: Option<&'a mut *mut i8>, outleft: Option<&'a mut usize>) -> KResult<IconvArgs<'a>> {
    let out_given = outbuf.as_ref().is_some_and(|p| !p.is_null());
    let input = match inbuf {
        Some(p) if !p.is_null() => p,
        _ => return Ok(if out_given { IconvArgs::Flush } else { IconvArgs::Reset }),
    };
    let left = inleft.ok_or(Errno::EFAULT)?;
    // UNSAFE(slice): see the function's safety contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    let data = unsafe { buf((*input).cast_const().cast::<c_void>(), *left)? };
    let out = match (outbuf, outleft) {
        (Some(p), Some(n)) if !p.is_null() => {
            // UNSAFE(slice): see the function's safety contract.
            #[allow(unsafe_code)]
            // SAFETY: the caller's contract.
            let data = unsafe { buf_mut((*p).cast::<c_void>(), *n)? };
            IconvOut { cells: Some((p, n)), data }
        }
        _ => IconvOut { cells: None, data: &mut [] },
    };
    Ok(IconvArgs::Convert(IconvIn { ptr: input, left, data }, out))
}

/// An argz vector held in a caller's `(char *argz, size_t len)` variables.
pub(crate) struct ArgzVar<'a> {
    argz: &'a mut *mut i8,
    len: &'a mut usize,
}

/// The argz vector in `*argz`/`*len`.
///
/// # Safety
/// `*argz` is NULL with `*len == 0`, or a buffer of `*len` readable bytes from this allocator
/// (the argz contract), not otherwise referenced meanwhile. The variables themselves arrive as
/// references (D-X.8); the buffer they describe is what this view relies on.
// UNSAFE(slice): the caller's argz buffer becomes a slice; replacing it frees the old one.
#[allow(unsafe_code)]
pub(crate) unsafe fn argz<'a>(argz: Option<&'a mut *mut i8>, len: Option<&'a mut usize>) -> KResult<ArgzVar<'a>> {
    match (argz, len) {
        (Some(argz), Some(len)) => Ok(ArgzVar { argz, len }),
        _ => Err(Errno::EFAULT),
    }
}

/// The output variables of `argz_create`/`argz_create_sep` (their old contents are ignored).
pub(crate) struct ArgzOut<'a> {
    argz: &'a mut *mut i8,
    len: &'a mut usize,
}

/// See [`ArgzOut`] (the variables arrive as references, D-X.8).
pub(crate) fn argz_out<'a>(argz: Option<&'a mut *mut i8>, len: Option<&'a mut usize>) -> KResult<ArgzOut<'a>> {
    match (argz, len) {
        (Some(argz), Some(len)) => Ok(ArgzOut { argz, len }),
        _ => Err(Errno::EFAULT),
    }
}

impl ArgzOut<'_> {
    /// Store a new vector (a fresh caller-owned buffer; NULL when empty).
    pub(crate) fn set(self, new: &[u8]) -> KResult<()> {
        let fresh = if new.is_empty() { 0 } else { crate::caller_alloc::dup_bytes(new)? };
        *self.argz = core::ptr::with_exposed_provenance_mut(fresh);
        *self.len = new.len();
        Ok(())
    }
}

/// A caller's `mbstate_t` as its ABI bytes; `None` for NULL (the hidden-state forms).
pub(crate) fn state(ps: Option<&mut liella_abi::ctypes::mbstate_t>) -> Option<&mut [u8; 8]> {
    ps.map(|m| &mut m.bytes)
}

/// A read-only `mbstate_t` (`mbsinit`).
pub(crate) fn state_ref(ps: Option<&liella_abi::ctypes::mbstate_t>) -> Option<&[u8; 8]> {
    ps.map(|m| &m.bytes)
}

/// A required fixed-size C object (D-X.8: `Option<&mut T>`); NULL is `EFAULT`, which the
/// calls without an error channel turn into a diagnostic (S6).
pub(crate) fn req<T>(o: Option<T>) -> KResult<T> {
    o.ok_or(Errno::EFAULT)
}

/// A C truth value.
pub(crate) fn flag(v: i32) -> bool {
    v != 0
}

/// A `ssize_t` count; negative counts are empty (`swab`).
pub(crate) fn count(n: isize) -> usize {
    usize::try_from(n).unwrap_or(0)
}

/// `if_set` when the address is not NULL, else `if_null` (`setbuf`: a buffer means fully
/// buffered, NULL unbuffered).
pub(crate) fn flag_mode(addr: usize, if_set: i32, if_null: i32) -> i32 {
    if addr != 0 { if_set } else { if_null }
}

/// A fortify check for an element transfer (`__fread_chk`): `n` elements of `size` bytes must
/// fit in `destlen` bytes; otherwise `__chk_fail`.
pub(crate) fn chk_elements(n: usize, size: usize, destlen: usize) -> usize {
    match n.checked_mul(size) {
        Some(b) if b <= destlen => n,
        _ => crate::ret::chk_fail(),
    }
}

/// A C `int` count; negative counts are empty.
pub(crate) fn nonneg(n: i32) -> usize {
    usize::try_from(n).unwrap_or(0)
}

/// A `char` argument as a byte.
pub(crate) fn byte_c(c: i8) -> u8 {
    c.to_le_bytes()[0]
}

impl ArgzVar<'_> {
    /// The current bytes.
    pub(crate) fn bytes(&self) -> &[u8] {
        if self.argz.is_null() || *self.len == 0 {
            return &[];
        }
        // UNSAFE(slice): see `argz`.
        #[allow(unsafe_code)]
        // SAFETY: the constructor's contract.
        unsafe {
            core::slice::from_raw_parts(self.argz.cast::<u8>(), *self.len)
        }
    }

    /// The buffer's address.
    pub(crate) fn base(&self) -> usize {
        self.argz.expose_provenance()
    }

    /// Replace the vector with `new` (a fresh caller-owned buffer; the old one is freed).
    pub(crate) fn replace(self, new: &[u8]) -> KResult<()> {
        let fresh = if new.is_empty() { 0 } else { crate::caller_alloc::dup_bytes(new)? };
        crate::caller_alloc::release(self.argz.expose_provenance());
        *self.argz = core::ptr::with_exposed_provenance_mut(fresh);
        *self.len = new.len();
        Ok(())
    }

    /// Replace with a vector built from the current bytes and base address.
    pub(crate) fn update(self, f: impl FnOnce(&[u8], usize) -> KResult<alloc::vec::Vec<u8>>) -> KResult<()> {
        let new = f(self.bytes(), self.base())?;
        self.replace(&new)
    }

}

/// A NULL-terminated array of C strings (`argz_create`'s `argv`) as texts.
///
/// # Safety
/// `argv` points to a NULL-terminated array of valid NUL-terminated strings.
// UNSAFE(slice): a C `char *const *` array and its strings become slices.
#[allow(unsafe_code)]
pub(crate) unsafe fn argv<'a>(argv: *const *mut i8) -> KResult<alloc::vec::Vec<&'a [u8]>> {
    let mut v = alloc::vec::Vec::new();
    if argv.is_null() {
        return Err(Errno::EFAULT);
    }
    let mut i = 0usize;
    loop {
        // UNSAFE(slice): reading the array one entry at a time.
        #[allow(unsafe_code)]
        // SAFETY: entries up to the NULL terminator are readable (contract).
        let p = unsafe { argv.wrapping_add(i).read() };
        if p.is_null() {
            return Ok(v);
        }
        v.try_reserve(1).map_err(|_| Errno::ENOMEM)?;
        // UNSAFE(slice): each entry's text.
        #[allow(unsafe_code)]
        // SAFETY: each entry is a valid string (contract).
        let text = unsafe { crate::strz::strz(p.cast_const())? };
        // Capacity was reserved above, so this cannot allocate (S8).
        #[allow(clippy::disallowed_methods)]
        v.push(text);
        i = i.saturating_add(1);
    }
}

/// A writable array of `n` pointers (`argz_extract`'s `argv`).
///
/// # Safety
/// `p` is valid for writing `n` pointers.
// UNSAFE(slice): a C pointer array becomes a slice of addresses.
#[allow(unsafe_code)]
pub(crate) unsafe fn ptrs_mut(p: *mut *mut i8) -> CDest<usize> {
    CDest { p: p.cast::<usize>(), limit: None }
}

/// `n` writable `wchar_t` units at `p`, `None` for NULL (a measuring call).
///
/// # Safety
/// As for [`buf_w_mut`] when `p` is not NULL.
// UNSAFE(slice): as for `buf_w_mut`.
#[allow(unsafe_code)]
pub(crate) unsafe fn opt_buf_w_mut<'a>(p: *mut i32, n: usize) -> KResult<Option<&'a mut [i32]>> {
    if p.is_null() {
        return Ok(None);
    }
    // UNSAFE(slice): the caller's C contract covers this view.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    unsafe { buf_w_mut(p, n) }.map(Some)
}

/// `n` writable bytes at `p`, `None` for NULL (a measuring call).
///
/// # Safety
/// As for [`buf_mut`] when `p` is not NULL.
// UNSAFE(slice): as for `buf_mut`.
#[allow(unsafe_code)]
pub(crate) unsafe fn opt_buf_mut<'a>(p: *mut i8, n: usize) -> KResult<Option<&'a mut [u8]>> {
    if p.is_null() {
        return Ok(None);
    }
    // UNSAFE(slice): the caller's C contract covers this view.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    unsafe { buf_mut(p.cast(), n) }.map(Some)
}

/// A destination at `p`, `None` for NULL (the multibyte encoders' reset form).
///
/// # Safety
/// As for [`dest`].
// UNSAFE(slice): as for `dest`.
#[allow(unsafe_code)]
pub(crate) unsafe fn opt_dest<T, P>(p: *mut P) -> Option<CDest<T>> {
    (!p.is_null()).then_some(CDest { p: p.cast::<T>(), limit: None })
}

/// A fortified destination at `p` (at most `limit` units), `None` for NULL.
///
/// # Safety
/// As for [`dest_chk`].
// UNSAFE(slice): as for `dest_chk`.
#[allow(unsafe_code)]
pub(crate) unsafe fn opt_dest_chk<T, P>(p: *mut P, limit: usize) -> Option<CDest<T>> {
    (!p.is_null()).then_some(CDest { p: p.cast::<T>(), limit: Some(limit) })
}

/// A `wchar_t` argument as a code point (negative values become an invalid one).
pub(crate) fn wc(c: i32) -> u32 {
    u32::try_from(c).unwrap_or(u32::MAX)
}

impl liella_legacy::mb_compat::ReadAt<u8> for CText {
    fn read_at(&mut self, addr: usize, limit: usize) -> Option<&[u8]> {
        // UNSAFE(slice): see `ctext`.
        #[allow(unsafe_code)]
        // SAFETY: the constructor's contract (read-only here).
        let t = unsafe { crate::strz::strz_n(core::ptr::with_exposed_provenance(addr), limit) };
        t.ok()
    }
}

impl liella_legacy::mb_compat::ReadAt<i32> for CText {
    fn read_at(&mut self, addr: usize, limit: usize) -> Option<&[i32]> {
        // UNSAFE(slice): see `ctext`.
        #[allow(unsafe_code)]
        // SAFETY: the constructor's contract (read-only here).
        let t = unsafe { crate::strz::wcsz_n(core::ptr::with_exposed_provenance(addr), limit) };
        t.ok()
    }
}

impl liella_legacy::prng_global::CMem for CText {
    fn bytes(&mut self, addr: usize, len: usize) -> Option<&mut [u8]> {
        // UNSAFE(slice): see `ctext`.
        #[allow(unsafe_code)]
        // SAFETY: the constructor's contract (a state buffer the caller handed over).
        let b = unsafe { buf_mut(core::ptr::with_exposed_provenance_mut(addr), len) };
        b.ok()
    }
}

impl liella_legacy::search_c::CWords for CText {
    fn word(&self, addr: usize, i: usize) -> usize {
        let at = addr.wrapping_add(i.wrapping_mul(size_of::<usize>()));
        // UNSAFE(slice): see `ctext`.
        #[allow(unsafe_code)]
        // SAFETY: the constructor's contract (a node, segment or element the caller handed over).
        let b = unsafe { buf(core::ptr::with_exposed_provenance(at), size_of::<usize>()) };
        b.ok().and_then(|b| <[u8; 8]>::try_from(b).ok()).map_or(0, usize::from_ne_bytes)
    }
    fn set_word(&mut self, addr: usize, i: usize, v: usize) {
        let at = addr.wrapping_add(i.wrapping_mul(size_of::<usize>()));
        // UNSAFE(slice): see `ctext`.
        #[allow(unsafe_code)]
        // SAFETY: the constructor's contract.
        let b = unsafe { buf_mut(core::ptr::with_exposed_provenance_mut(at), size_of::<usize>()) };
        if let Ok(b) = b {
            b.copy_from_slice(&v.to_ne_bytes());
        }
    }
    fn alloc(&mut self, words: usize) -> Option<usize> {
        let p = crate::caller_alloc::calloc(words, size_of::<usize>());
        (!p.is_null()).then(|| p.expose_provenance())
    }
    fn free(&mut self, addr: usize) {
        crate::caller_alloc::release(addr);
    }
    fn text(&self, addr: usize) -> &[u8] {
        // UNSAFE(slice): see `ctext`.
        #[allow(unsafe_code)]
        // SAFETY: the constructor's contract (a key the caller stored).
        let t = unsafe { crate::strz::strz(core::ptr::with_exposed_provenance(addr)) };
        t.unwrap_or(&[])
    }
    fn text_prefix(&self, addr: usize, limit: usize) -> &[u8] {
        // UNSAFE(slice): see `ctext`.
        #[allow(unsafe_code)]
        // SAFETY: the constructor's contract (an entry the caller stored; at most `limit` bytes).
        let t = unsafe { crate::strz::strz_n(core::ptr::with_exposed_provenance(addr), limit) };
        t.unwrap_or(&[])
    }
    fn env_find(&self, env: usize, name: &[u8]) -> Option<(usize, usize)> {
        self.env_find_end(env, name).ok()
    }
    /// ENVIRON-SMALL-CYCLE: the C layer's one-pass scanner (`getenv`'s, in the copy that also
    /// answers the slot it stopped at) gives the entry's index, or on a miss the entry count, with
    /// no second pass over the pointers (ENVIRON-PUTENV-SLOPE's pointer pass and re-compare cost
    /// a quarter of a small environment's `putenv`). The scanner compared `name=` itself.
    fn env_find_end(&self, env: usize, name: &[u8]) -> Result<(usize, usize), usize> {
        if env == 0 || name.is_empty() || name.contains(&0) {
            return liella_legacy::environ::find_scan(env, name, self).ok_or_else(|| liella_legacy::environ::count(env, self));
        }
        // The scanner's contract: `env` is a NULL-terminated array of strings (the constructor's,
        // as for `word`); `name` is `name.len()` readable bytes holding no NUL (checked above).
        let r = ffi::liella_machine_env_slot(env, name.as_ptr(), name.len());
        // The stop slot is in the array at or after `env`, a whole number of pointers on.
        let i = r.slot.wrapping_sub(env).checked_div(size_of::<usize>()).unwrap_or(0);
        if r.entry == 0 { Err(i) } else { Ok((i, r.entry)) }
    }
}

/// The value behind a reference (`lsearch` reads `*nmemb` before updating it).
pub(crate) fn copy<T: Copy>(r: &T) -> T {
    *r
}

/// `n` readable elements of `size` bytes at `p` (`bsearch`, `lfind`).
///
/// # Safety
/// `p` is valid for reading `n * size` bytes.
// UNSAFE(slice): a C array of opaque elements becomes a byte slice.
#[allow(unsafe_code)]
pub(crate) unsafe fn elements<'a>(p: *const c_void, n: usize, size: usize) -> KResult<&'a [u8]> {
    let bytes = n.checked_mul(size).ok_or(Errno::EINVAL)?;
    // SAFETY: the caller's contract, for `bytes` bytes.
    unsafe { buf(p, bytes) }
}

/// `n + 1` writable elements of `size` bytes at `p` (`lsearch`: room for one more).
///
/// # Safety
/// `p` is valid for writing `(n + 1) * size` bytes, not otherwise referenced.
// UNSAFE(slice): as for `elements_mut`, one element longer.
#[allow(unsafe_code)]
pub(crate) unsafe fn elements_room<'a>(p: *mut c_void, n: usize, size: usize) -> KResult<&'a mut [u8]> {
    let n = n.checked_add(1).ok_or(Errno::EINVAL)?;
    // SAFETY: the caller's contract.
    unsafe { elements_mut(p, n, size) }
}

/// Zero `len` bytes at `addr` (a block the heap just handed out: `calloc`).
///
/// # Safety
/// `addr` is a live heap block of at least `len` bytes that nothing references.
// UNSAFE(slice): a fresh heap block becomes a byte slice to clear.
#[allow(unsafe_code)]
pub(crate) unsafe fn zero_at(addr: usize, len: usize) {
    // SAFETY: the caller's contract.
    if let Ok(b) = unsafe { buf_mut(core::ptr::with_exposed_provenance_mut(addr), len) } {
        liella::bytes::fill(b, 0);
    }
}

/// Zero `buf` so that the stores survive every optimisation (D-X.11, D-4.9): the one
/// implementation of `liella_secure_zero` (which `explicit_bzero` and `__explicit_bzero_chk`
/// call) and of the heap's wiping of secure blocks. The bytes are stored as zero in Rust, then
/// their address goes to the C layer's barrier (`csys/src/machine/barrier.S`, OD-C1): an
/// assembly function, so no optimiser sees into it, not even under LTO. To the compiler the
/// call may read the whole object the pointer reaches, so every zero store before it is live
/// and is performed, in full, before the call; the barrier then returns without touching
/// memory.
pub(crate) fn secure_zero(buf: &mut [u8]) {
    bytes::wipe(buf, |w| ffi::liella_machine_barrier(w.as_ptr()));
}

/// Copy `len` bytes between two distinct memory blocks (an obstack object moving to a new
/// chunk) with the copy kernel `memcpy` uses (COPY-AT: the generic slice copy cost about 226
/// instructions for a 128-byte move, as REALLOC-CHAIN measured for `realloc`).
///
/// # Safety
/// Both are live blocks of at least `len` bytes, `from` readable and `to` writable, that
/// nothing else references during the copy (the kernel would also handle an overlap).
// UNSAFE(hot-kernel): the validated entry to the copy kernel.
#[allow(unsafe_code)]
pub(crate) unsafe fn copy_at(from: usize, to: usize, len: usize) {
    if len == 0 {
        // Nothing moves; neither address is touched (as the slice copy did).
        return;
    }
    // SAFETY: the caller's contract: `to` valid for writing and `from` for reading `len` bytes.
    unsafe {
        crate::hot::move_bytes(core::ptr::with_exposed_provenance_mut(to), core::ptr::with_exposed_provenance(from), len);
    }
}

/// Calls into C functions the C caller supplied as data (obstack callbacks). See the proof
/// obligations of `ffi::liella_machine_call0`/`call3`.
pub(crate) struct CCalls;

impl liella_legacy::obstack::CCall for CCalls {
    fn call3(&mut self, f: usize, a: usize, b: usize, c: usize) -> usize {
        let w = |v: usize| i64::from_ne_bytes(v.to_ne_bytes());
        usize::from_ne_bytes(crate::ffi::liella_machine_call3(f, w(a), w(b), w(c)).to_ne_bytes())
    }
    fn call0(&mut self, f: usize) {
        let _ = crate::ffi::liella_machine_call0(f);
    }
    fn fatal(&mut self) -> ! {
        crate::ret::abort()
    }
}


/// The `n` words at `addr`, which live for the rest of the process: the initial stack's
/// argument, environment and auxiliary-vector words, and the program headers.
///
/// # Safety
/// `addr` is 8-aligned and `n` words there are readable and never unmapped.
// UNSAFE(slice): memory the kernel set up at `execve` becomes a slice.
#[allow(unsafe_code)]
pub(crate) unsafe fn words_at(addr: usize, n: usize) -> &'static [u64] {
    if n == 0 || addr == 0 {
        return &[];
    }
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    unsafe {
        core::slice::from_raw_parts(core::ptr::with_exposed_provenance::<u64>(addr), n)
    }
}

/// The `n` bytes at `addr` for the rest of the process (`AT_RANDOM`, a TLS image, C text the
/// startup publishes).
///
/// # Safety
/// `n` bytes at `addr` are readable and never unmapped or written.
// UNSAFE(slice): process-lifetime memory becomes a slice.
#[allow(unsafe_code)]
#[cfg_attr(not(feature = "runtime"), allow(dead_code))] // the products' startup
pub(crate) unsafe fn bytes_at(addr: usize, n: usize) -> &'static [u8] {
    if n == 0 || addr == 0 {
        return &[];
    }
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    unsafe {
        core::slice::from_raw_parts(core::ptr::with_exposed_provenance::<u8>(addr), n)
    }
}

/// The `n` writable bytes at `addr` (memory the runtime itself just mapped).
///
/// # Safety
/// `n` bytes at `addr` are writable and referenced by nothing else.
// UNSAFE(slice): fresh memory becomes a mutable slice.
#[allow(unsafe_code)]
pub(crate) unsafe fn bytes_at_mut(addr: usize, n: usize) -> &'static mut [u8] {
    if n == 0 || addr == 0 {
        return &mut [];
    }
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    unsafe {
        core::slice::from_raw_parts_mut(core::ptr::with_exposed_provenance_mut::<u8>(addr), n)
    }
}

/// The C text named by the environment array, the program-name objects and the argument vector
/// the runtime recorded, with the C allocator for new entries (spec §1.3, §1.4).
pub(crate) fn env_text() -> CText {
    // UNSAFE(slice): the addresses `environ` and its entries hold, the program names, and the
    // `argc` entries of `argv` are C text by the contract of those objects (spec §1.3: a
    // NULL-terminated array of NUL-terminated strings, which C programs may replace but must
    // keep valid; spec §1.1: `argv` as the kernel placed it on the initial stack).
    #[allow(unsafe_code)]
    // SAFETY: as above; writers of the array are serialised by `crate::process`.
    unsafe {
        ctext()
    }
}

/// The address of the first entry of the current `environ` array (spec §1.3) that is `name`
/// followed by `=`, found by the C layer in one pass over the array (PJ-8; `getenv`).
pub(crate) fn env_find(name: &[u8]) -> Option<usize> {
    // A name holding a NUL matches no entry; the scanner's contract excludes it (its word
    // compare would read past an entry's terminator as a name byte, TINY-HOT).
    if name.contains(&0) {
        return None;
    }
    env_find_raw(name.as_ptr(), name.len())
}

/// [`env_find`] from slot `at` of the `environ` array `env` on: the first entry at or after that
/// slot that is `name` followed by `=` (the [`crate::process::EnvMemo`] hit, GETTEXT-MEMO-HIT-COST:
/// one scanner call that compares the remembered slot first, instead of two bounded measures).
#[inline]
pub(crate) fn env_find_from(env: usize, at: usize, name: &[u8]) -> Option<usize> {
    // A name holding a NUL matches no entry (as `env_find`); for a constant name this folds away.
    if env == 0 || !name.iter().all(|&b| b != 0) {
        return None;
    }
    // The proof obligation of `liella_machine_env_find`: slots `at..` of a NULL-terminated
    // `environ` array are themselves one, for `at` at most its length. The caller's `at` is a
    // slot it found in this same array (`EnvMemo`: the same `environ`, which this library's
    // writers have not shortened since); `name` is a slice without NUL.
    // Slot `at` of that array: its address does not wrap (the array is in memory).
    let from = env.wrapping_add(at.wrapping_mul(size_of::<usize>()));
    let p = ffi::liella_machine_env_find(from, name.as_ptr(), name.len());
    (p != 0).then_some(p)
}

/// [`env_find`] for `len` bytes at `name` (the caller measured a readable C string).
#[inline]
pub(crate) fn env_find_raw(name: *const u8, len: usize) -> Option<usize> {
    // The proof obligation of `liella_machine_env_find`: `environ` is 0 or a NULL-terminated
    // array of strings (its contract, as for `env_text`); `name` is `len` readable bytes, none
    // of them NUL (a measured C string, or a slice `env_find` checked).
    let env = ffi::environ.load(core::sync::atomic::Ordering::Acquire);
    let p = ffi::liella_machine_env_find(env, name, len);
    (p != 0).then_some(p)
}

/// The C text at `addr` (empty for NULL), for objects the C contract keeps valid for the
/// process: the program name (spec §1.4), `argp_program_version` and `argp_program_bug_address`
/// (spec §1.7).
pub(crate) fn process_text(addr: usize) -> &'static [u8] {
    // UNSAFE(slice): see the function's documentation.
    #[allow(unsafe_code)]
    // SAFETY: the program-name objects are NULL or NUL-terminated strings that live for the
    // process (set at startup from argv[0], which lives on the initial stack); argp's
    // program-wide texts are the program's own strings, set for the process.
    unsafe { crate::strz::strz(core::ptr::with_exposed_provenance(addr)) }.unwrap_or(&[])
}

/// The C text at `addr` (empty for NULL) that a caller handed to the running entry point by
/// address inside one of its arguments: `getopt`'s argument-vector entries and long-option
/// names, `getsubopt`'s tokens, the texts of an `argp` tree and a help filter's result (spec
/// §1.6, §1.6a, §1.7). Crate-private contract: the calling entry point's C contract keeps the
/// text valid and unwritten while it runs, and the result is used only during that call.
pub(crate) fn caller_text<'a>(addr: usize) -> &'a [u8] {
    // UNSAFE(slice): see the function's documentation.
    #[allow(unsafe_code)]
    // SAFETY: NULL or a NUL-terminated string the running call's contract keeps valid and
    // unwritten (the documentation above); the result does not outlive the call.
    unsafe { crate::strz::strz(core::ptr::with_exposed_provenance(addr)) }.unwrap_or(&[])
}

/// The C text at `addr` (without its NUL; empty for NULL), writable: `getsubopt`'s option text,
/// which the caller hands over for modification in place (spec §1.6a). Crate-private
/// contract: as for [`caller_text`], the bytes are writable, and nothing else refers to them
/// while the result is used.
pub(crate) fn caller_text_mut<'a>(addr: usize) -> &'a mut [u8] {
    // UNSAFE(slice): see the function's documentation.
    #[allow(unsafe_code)]
    // SAFETY: NULL (an error, mapped to empty) or a writable NUL-terminated string the running
    // call's contract hands over (the documentation above); the result does not outlive the call.
    unsafe { crate::strz::strz_mut(core::ptr::with_exposed_provenance_mut(addr)) }.unwrap_or(&mut [])
}

/// The identifier `openlog` keeps by pointer (spec §26.1 ⚑ HIST): the C text at `addr` (empty
/// for NULL), read again for each message. Crate-private contract: the C contract makes the
/// caller keep the string valid for the whole logging session, and the result is used only
/// while one message is delivered.
pub(crate) fn session_text<'a>(addr: usize) -> &'a [u8] {
    // UNSAFE(slice): see the function's documentation.
    #[allow(unsafe_code)]
    // SAFETY: NULL or a NUL-terminated string that `openlog`'s contract keeps valid while the
    // logging session lasts (the documentation above); the result does not outlive the message.
    unsafe { crate::strz::strz(core::ptr::with_exposed_provenance(addr)) }.unwrap_or(&[])
}

/// The executable's program headers (`AT_PHDR`, `AT_PHNUM`), mapped by the kernel with it for
/// the life of the process: `n` words at `addr`.
pub(crate) fn program_headers(addr: usize, n: usize) -> &'static [u64] {
    if addr == 0 || addr % 8 != 0 {
        return &[];
    }
    // UNSAFE(slice): the loaded program headers.
    #[allow(unsafe_code)]
    // SAFETY: `AT_PHDR`/`AT_PHNUM` describe headers the kernel mapped with the executable,
    // which stay mapped (⚑ KERN).
    unsafe {
        words_at(addr, n)
    }
}

/// The `len` bytes at `addr` of a loaded object's read-only memory (D-2.7, IP-27): a
/// non-writable `PT_LOAD` segment, or the ELF header at the object's start. Crate-private
/// contract (kept by `crate::backtrace`): `addr` and `len` come from the program headers of the
/// executable (`AT_PHDR`) or of the image this library is linked into (`__ehdr_start`), with the
/// load bias applied, or are the 64 bytes at `__ehdr_start`. The kernel (the executable) or the
/// dynamic linker (this library's own image) mapped these bytes readable when the object was
/// loaded, and they stay mapped for the life of the process: neither object is ever unloaded.
/// A segment without `PF_W`, and the ELF header in the first segment, are never written (only a
/// C caller's `mprotect` could change that, outside safe code).
pub(crate) fn loaded(addr: usize, len: usize) -> &'static [u8] {
    if addr == 0 || isize::try_from(len).is_err() || addr.checked_add(len).is_none() {
        return &[];
    }
    // UNSAFE(slice): an object's loaded image becomes a slice.
    #[allow(unsafe_code)]
    // SAFETY: mapped, read-only and never unmapped (see the documentation above).
    unsafe {
        bytes_at(addr, len)
    }
}

/// The program headers of the image this library is linked into: `n` words at `addr`
/// (`__ehdr_start` plus the header's `e_phoff`). Crate-private contract (kept by
/// `crate::backtrace`): as for [`loaded`]; the linker defines `__ehdr_start` only when the ELF
/// header, and with it the program headers that follow it, lie in a loaded segment.
pub(crate) fn image_program_headers(addr: usize, n: usize) -> &'static [u64] {
    if addr == 0 || addr % 8 != 0 {
        return &[];
    }
    // UNSAFE(slice): the loaded program headers.
    #[allow(unsafe_code)]
    // SAFETY: see the contract above: mapped with the image for the process's life, read-only.
    unsafe {
        words_at(addr, n)
    }
}

/// The initial auxiliary vector's words, as the runtime measured them at startup.
pub(crate) fn startup_auxv(addr: usize, words: usize) -> &'static [u64] {
    // UNSAFE(slice): the auxiliary vector on the initial stack.
    #[allow(unsafe_code)]
    // SAFETY: `crate::runtime` stored this address and length after measuring the vector
    // (through its `AT_NULL` pair) on the initial stack, which lives for the process; nothing
    // else sets them.
    unsafe {
        words_at(addr, words)
    }
}

// ------------------------------------------------------------------------------ phase 5: §11

/// Kernel structures a C array of which may be viewed in place: plain data whose every bit
/// pattern is a value, with the C layout (`liella_kernel::uapi`, Appendix C).
pub(crate) trait CPod {}
impl CPod for liella_kernel::uapi::PollFd {}
impl CPod for liella_kernel::uapi::EpollEvent {}
impl CPod for liella_kernel::mem::RemoteIoVec {}
impl CPod for liella_kernel::mem::IoVec<'_> {}
impl CPod for liella_kernel::mem::IoVecMut<'_> {}
impl CPod for liella_kernel::uapi::Sembuf {}
impl CPod for liella_kernel::uapi::MqAttr {}
impl CPod for u16 {}
impl CPod for u32 {}
impl CPod for liella_kernel::mem::MMsgHdr<'_> {}
impl CPod for liella_kernel::mem::MMsgHdrMut<'_> {}
impl CPod for liella_abi::layout::struct_passwd {}
impl CPod for liella_abi::layout::struct_group {}
impl CPod for liella_abi::layout::struct_spwd {}
impl CPod for liella_abi::layout::struct_sgrp {}
impl CPod for liella_abi::layout::struct_utmp {}
impl CPod for liella_abi::layout::struct_hostent {}
impl CPod for liella_abi::layout::struct_netent {}
impl CPod for liella_abi::layout::struct_servent {}
impl CPod for liella_abi::layout::struct_protoent {}
impl CPod for liella_abi::layout::struct_rpcent {}
impl CPod for liella_abi::layout::struct_aliasent {}
impl CPod for liella_abi::layout::struct_addrinfo {}
impl CPod for liella_abi::layout::struct_mntent {}
impl CPod for liella_abi::layout::struct_fstab {}
impl CPod for u8 {}

/// `n` elements of a C array of kernel structures (negative `n` is `EINVAL`).
///
/// # Safety
/// `p` is NULL with `n == 0`, or points to `n` readable elements not written for the returned
/// lifetime. For `IoVec`, each element describes memory readable for that lifetime.
// UNSAFE(slice): a C array of kernel structures becomes a slice.
#[allow(unsafe_code)]
pub(crate) unsafe fn array<'a, T: CPod>(p: *const c_void, n: i64) -> KResult<&'a [T]> {
    let n = usize::try_from(n).map_err(|_| Errno::EINVAL)?;
    if n == 0 {
        return Ok(&[]);
    }
    if p.is_null() || n.checked_mul(size_of::<T>()).is_none_or(|b| isize::try_from(b).is_err()) || !p.cast::<T>().is_aligned() {
        return Err(Errno::EFAULT);
    }
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: non-NULL, aligned, size checked; validity is the caller's contract.
    unsafe {
        Ok(core::slice::from_raw_parts(p.cast::<T>(), n))
    }
}

/// As [`array`], writable (`poll`'s `revents`, `epoll_wait`'s events, `readv`'s buffers).
///
/// # Safety
/// As for [`array`], and the elements (and for `IoVecMut`, the memory each describes) are
/// writable and referenced by nothing else.
// UNSAFE(slice): a writable C array of kernel structures becomes a slice.
#[allow(unsafe_code)]
pub(crate) unsafe fn array_mut<'a, T: CPod>(p: *mut c_void, n: i64) -> KResult<&'a mut [T]> {
    let n = usize::try_from(n).map_err(|_| Errno::EINVAL)?;
    if n == 0 {
        return Ok(&mut []);
    }
    if p.is_null() || n.checked_mul(size_of::<T>()).is_none_or(|b| isize::try_from(b).is_err()) || !p.cast::<T>().is_aligned() {
        return Err(Errno::EFAULT);
    }
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: as above, and exclusive per the contract.
    unsafe {
        Ok(core::slice::from_raw_parts_mut(p.cast::<T>(), n))
    }
}

/// The `iovec` count of the vector calls: at most `IOV_MAX` (1024), else `EINVAL` (§11.3).
pub(crate) fn iovcnt(n: c_int) -> i64 {
    if (0..=1024).contains(&n) { i64::from(n) } else { -1 }
}

/// `fcntl`'s third argument, typed by the command (T7): an integer (the low 32 bits for the
/// commands that take an `int`), or a pointer to the command's structure.
///
/// # Safety
/// For a pointer command, `arg` is NULL or the address of that command's structure, writable
/// and referenced by nothing else (the contract of `fcntl`, §11.5).
// UNSAFE(slice): a pointer argument becomes a reference to its structure.
#[allow(unsafe_code)]
pub(crate) unsafe fn fcntl_arg<'a>(cmd: c_int, arg: usize) -> liella_legacy::io_compat::FcntlArg<'a> {
    use liella_legacy::io_compat::{FcntlArg, FcntlKind};
    let p = core::ptr::with_exposed_provenance_mut::<u8>(arg);
    match liella_legacy::io_compat::fcntl_kind(cmd) {
        // An `int` argument arrives in a 64-bit register whose upper half is unspecified.
        FcntlKind::Int(c) => FcntlArg::Int(c, i64::from(i32::from_ne_bytes(u32::try_from(arg & 0xffff_ffff).unwrap_or(0).to_ne_bytes()))),
        // UNSAFE(slice): the command's `struct flock`.
        #[allow(unsafe_code)]
        // SAFETY: the caller's contract; `as_mut` checks NULL, and a C object of the type is
        // aligned for it.
        FcntlKind::Lock(c) => FcntlArg::Lock(c, unsafe { p.cast::<liella_kernel::uapi::Flock>().as_mut() }),
        // UNSAFE(slice): the command's `struct f_owner_ex`.
        #[allow(unsafe_code)]
        // SAFETY: as above.
        FcntlKind::OwnerEx(set) => FcntlArg::OwnerEx(set, unsafe { p.cast::<liella_kernel::uapi::FOwnerEx>().as_mut() }),
        // UNSAFE(slice): the command's `uint64_t`.
        #[allow(unsafe_code)]
        // SAFETY: as above.
        FcntlKind::Hint(c) => FcntlArg::Hint(c, unsafe { p.cast::<u64>().as_mut() }),
        FcntlKind::Unknown => FcntlArg::Unknown,
    }
}

/// A `sigset_t` argument as the kernel's 64-bit mask (D-20.3: bits beyond 64 are ignored), with
/// the library's internal signal (32, D-20.1) never blocked.
pub(crate) fn sigmask(set: Option<&liella_abi::layout::sigset_t>) -> Option<liella_kernel::uapi::SigSet> {
    set.map(|s| liella_kernel::uapi::SigSet(s.__val[0] & !(1u64 << 31)))
}

/// `__open_2`/`__openat_2`: `O_CREAT` or `O_TMPFILE` without a mode is a program bug, which
/// glibc reports with a fortify message and an abort (§11.1).
pub(crate) fn open_without_mode(flags: c_int) -> c_int {
    if liella_legacy::io_compat::needs_mode(flags) {
        crate::process::fortify_fail(b"invalid open call: O_CREAT or O_TMPFILE without mode")
    }
    flags
}

/// A fortified element count: `n` elements of `size` bytes must fit in `bytes` (`__poll_chk`).
pub(crate) fn chk_elems(n: u64, bytes: usize, size: usize) -> u64 {
    match bytes.checked_div(size) {
        Some(room) if u64::try_from(room).is_ok_and(|r| n <= r) => n,
        _ => crate::ret::chk_fail(),
    }
}

/// The result vector of `mincore`: one byte per page of `len` bytes.
///
/// # Safety
/// `vec` holds `ceil(len / page)` writable bytes (the contract of `mincore`, §11.10).
// UNSAFE(slice): the caller's result vector becomes a slice.
#[allow(unsafe_code)]
pub(crate) unsafe fn mincore_vec<'a>(vec: *mut u8, len: usize) -> KResult<&'a mut [u8]> {
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    unsafe { buf_mut(vec.cast(), len.div_ceil(4096)) }
}

/// A C `int` of flags or a command as the kernel's unsigned value (the same bits).
pub(crate) fn bits(n: c_int) -> u32 {
    u32::from_ne_bytes(n.to_ne_bytes())
}

/// A C `nfds_t`/`unsigned long` count for [`array`] (saturated; `array` rejects what cannot be
/// a size).
pub(crate) fn count_ul(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// The advice of `process_madvise`, typed (`EINVAL` for advice the call does not take).
pub(crate) fn remote_advice(advice: c_int) -> KResult<liella_kernel::ops::RemoteAdvice> {
    liella_kernel::ops::RemoteAdvice::from_code(bits(advice)).ok_or(Errno::EINVAL)
}

/// A `socklen_t` length as a byte count.
pub(crate) fn len32(n: u32) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

/// A `size_t` count for [`array`] (saturated, as [`count_ul`]).
pub(crate) fn count_sz(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// A C `unsigned long` of flags as the kernel's 32-bit value (`process_vm_readv`).
pub(crate) fn flags_ul(n: u64) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

// ------------------------------------------------------------------------------ phase 5: §13, §25

/// A C structure pointer as a reference, `EFAULT` for NULL or misaligned.
///
/// # Safety
/// `p` is NULL or points to a `T`, writable and referenced by nothing else for `'a`.
// UNSAFE(slice): the caller's structure becomes a reference.
#[allow(unsafe_code)]
unsafe fn object_mut<'a, T>(p: *mut c_void) -> KResult<&'a mut T> {
    if !p.cast::<T>().is_aligned() {
        return Err(Errno::EFAULT);
    }
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: aligned (checked); `as_mut` checks NULL; validity is the caller's contract.
    unsafe { p.cast::<T>().as_mut() }.ok_or(Errno::EFAULT)
}

/// A `msgctl`/`shmctl` argument in the shape its command uses (`ipc_compat::*_shape`; an
/// unknown command is `EINVAL`).
///
/// # Safety
/// `p` is what the command takes: ignored, NULL, or the caller's structure of the planned
/// type or size, writable and referenced by nothing else (the contract of `*ctl`, §25.2).
// UNSAFE(slice): a structure pointer becomes a reference or a byte slice.
#[allow(unsafe_code)]
pub(crate) unsafe fn ipc_ctl_arg<'a, T>(shape: Option<liella_legacy::ipc_compat::CtlShape>, p: *mut c_void) -> KResult<liella_legacy::ipc_compat::CtlArg<'a, T>> {
    use liella_legacy::ipc_compat::{CtlArg, CtlShape};
    match shape.ok_or(Errno::EINVAL)? {
        // UNSAFE(slice): the command's structure.
        #[allow(unsafe_code)]
        // SAFETY: the caller's contract.
        CtlShape::Ds => unsafe { object_mut::<T>(p) }.map(CtlArg::Ds),
        // UNSAFE(slice): the command's smaller statistics structure.
        #[allow(unsafe_code)]
        // SAFETY: the caller's contract.
        CtlShape::Info(n) => unsafe { buf_mut(p, n) }.map(CtlArg::Info),
        CtlShape::None | CtlShape::Val | CtlShape::Values => Ok(CtlArg::None),
    }
}

/// `semctl`'s `union semun` in the shape its command uses (`ipc_compat::sem_shape`). The
/// `GETALL`/`SETALL` array has one entry per semaphore, counted by `nsems` when needed.
///
/// # Safety
/// `arg` is what the command takes: an `int`, or the address of the caller's `semid_ds`,
/// `seminfo` or `unsigned short` array, writable and referenced by nothing else (§25.2).
// UNSAFE(slice): the union's pointer members become references or slices.
#[allow(unsafe_code)]
pub(crate) unsafe fn semctl_arg<'a>(cmd: c_int, arg: usize, nsems: impl FnOnce() -> KResult<usize>) -> KResult<liella_legacy::ipc_compat::SemArg<'a>> {
    use liella_legacy::ipc_compat::{sem_shape, CtlShape, SemArg};
    let p = core::ptr::with_exposed_provenance_mut::<c_void>(arg);
    match sem_shape(cmd).ok_or(Errno::EINVAL)? {
        CtlShape::None => Ok(SemArg::None),
        // An `int` member arrives in a 64-bit register whose upper half is unspecified.
        CtlShape::Val => Ok(SemArg::Val(i32::from_ne_bytes(u32::try_from(arg & 0xffff_ffff).unwrap_or(0).to_ne_bytes()))),
        // UNSAFE(slice): `buf`.
        #[allow(unsafe_code)]
        // SAFETY: the caller's contract.
        CtlShape::Ds => unsafe { object_mut(p) }.map(SemArg::Ds),
        // UNSAFE(slice): `__buf`.
        #[allow(unsafe_code)]
        // SAFETY: the caller's contract.
        CtlShape::Info(n) => unsafe { buf_mut(p, n) }.map(SemArg::Info),
        // UNSAFE(slice): `array`, one entry per semaphore.
        #[allow(unsafe_code)]
        // SAFETY: the caller's contract.
        CtlShape::Values => unsafe { array_mut::<u16>(p, count_sz(nsems()?)) }.map(SemArg::Values),
    }
}

/// `fsconfig`'s value, typed by the command: a string (`SET_STRING`, `SET_PATH`,
/// `SET_PATH_EMPTY`), `aux` bytes (`SET_BINARY`), or none.
///
/// # Safety
/// `value` is what the command takes: NULL, a NUL-terminated string, or `aux` readable bytes.
// UNSAFE(slice): the value pointer becomes a string or a byte slice.
#[allow(unsafe_code)]
pub(crate) unsafe fn fs_value<'a>(cmd: liella_kernel::ops::FsconfigCmd, value: *const c_void, aux: c_int) -> KResult<liella_kernel::sys::FsValue<'a>> {
    use liella_kernel::ops::FsconfigCmd as C;
    use liella_kernel::sys::FsValue;
    match cmd {
        // UNSAFE(slice): a string value.
        #[allow(unsafe_code)]
        // SAFETY: the caller's contract.
        C::SetString | C::SetPath | C::SetPathEmpty => unsafe { crate::strz::path(value.cast()) }.map(FsValue::Str),
        // UNSAFE(slice): a binary value of `aux` bytes.
        #[allow(unsafe_code)]
        // SAFETY: the caller's contract.
        C::SetBinary => unsafe { buf(value, usize::try_from(aux).map_err(|_| Errno::EINVAL)?) }.map(FsValue::Binary),
        _ => Ok(FsValue::None),
    }
}

/// `fsconfig`'s command, typed.
pub(crate) fn fsconfig_cmd(cmd: u32) -> KResult<liella_kernel::ops::FsconfigCmd> {
    liella_kernel::ops::FsconfigCmd::from_code(cmd).ok_or(Errno::EINVAL)
}

/// `mount_setattr`'s attribute structure of `size` bytes: at least the first version (32
/// bytes); a longer one is accepted when the bytes this ABI does not know are zero (`E2BIG`
/// otherwise), as the kernel's extensible-structure rule.
///
/// # Safety
/// `p` points to `size` readable bytes.
// UNSAFE(slice): the caller's structure becomes a reference.
#[allow(unsafe_code)]
pub(crate) unsafe fn mount_attr<'a>(p: *const c_void, size: usize) -> KResult<&'a liella_kernel::uapi::MountAttr> {
    const V0: usize = size_of::<liella_kernel::uapi::MountAttr>();
    if size < V0 {
        return Err(Errno::EINVAL);
    }
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    let bytes = unsafe { buf(p, size) }?;
    if bytes.get(V0..).is_some_and(|tail| tail.iter().any(|&b| b != 0)) {
        return Err(Errno::E2BIG);
    }
    if !p.cast::<liella_kernel::uapi::MountAttr>().is_aligned() {
        return Err(Errno::EFAULT);
    }
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: non-NULL (`buf` checked), aligned, at least V0 readable bytes.
    Ok(unsafe { &*p.cast::<liella_kernel::uapi::MountAttr>() })
}

/// A message buffer (`struct msgbuf`: `long mtype` then `n` text bytes), read-only.
///
/// # Safety
/// `p` points to `8 + n` readable bytes.
// UNSAFE(slice): the caller's message becomes a slice.
#[allow(unsafe_code)]
pub(crate) unsafe fn msg<'a>(p: *const c_void, n: usize) -> KResult<&'a [u8]> {
    let len = liella_legacy::ipc_compat::msg_bytes(n)?;
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    unsafe { buf(p, len) }
}

/// As [`msg`], writable (`msgrcv`).
///
/// # Safety
/// `p` points to `8 + n` writable bytes referenced by nothing else.
// UNSAFE(slice): the caller's message buffer becomes a slice.
#[allow(unsafe_code)]
pub(crate) unsafe fn msg_mut<'a>(p: *mut c_void, n: usize) -> KResult<&'a mut [u8]> {
    let len = liella_legacy::ipc_compat::msg_bytes(n)?;
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    unsafe { buf_mut(p, len) }
}

// ------------------------------------------------------------------------------ phase 5: §15

/// A `socklen_t` as a byte count. The kernel treats it as an `int`: beyond `INT_MAX` is
/// `EINVAL`.
fn socklen(n: u32) -> KResult<usize> {
    i32::try_from(n).ok().and_then(|v| usize::try_from(v).ok()).ok_or(Errno::EINVAL)
}

/// A socket address (or an option value) of `len` bytes.
///
/// # Safety
/// `p` points to `len` readable bytes (or is NULL with `len` 0).
// UNSAFE(slice): the caller's address becomes a slice.
#[allow(unsafe_code)]
pub(crate) unsafe fn sockaddr<'a>(p: *const c_void, len: u32) -> KResult<&'a [u8]> {
    let n = socklen(len)?;
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    unsafe { buf(p, n) }
}

/// As [`sockaddr`], with NULL meaning none (`sendto` on a connected socket).
///
/// # Safety
/// As for [`sockaddr`] when `p` is not NULL.
// UNSAFE(slice): the caller's address becomes a slice.
#[allow(unsafe_code)]
pub(crate) unsafe fn opt_sockaddr<'a>(p: *const c_void, len: u32) -> KResult<Option<&'a [u8]>> {
    if p.is_null() {
        return Ok(None);
    }
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    unsafe { sockaddr(p, len) }.map(Some)
}

/// A result buffer with its in-out `socklen_t` (`accept`, `getsockname`, `recvfrom`,
/// `getsockopt`): `*len` bytes at `p`. NULL `p` means no result wanted; a buffer without a
/// length cell is `EFAULT`.
///
/// # Safety
/// `p` is NULL or points to `*len` writable bytes referenced by nothing else.
// UNSAFE(slice): the caller's result buffer becomes a slice.
#[allow(unsafe_code)]
pub(crate) unsafe fn addr_out<'a>(p: *mut c_void, len: Option<&'a mut u32>) -> KResult<Option<liella_legacy::net_compat::AddrOut<'a>>> {
    if p.is_null() {
        return Ok(None);
    }
    let len = len.ok_or(Errno::EFAULT)?;
    let n = socklen(*len)?;
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    let b = unsafe { buf_mut(p, n) }?;
    Ok(Some(liella_legacy::net_compat::AddrOut { buf: b, len }))
}

/// A C `struct msghdr` for sending, viewed in place: its layout is the kernel's, and the
/// kernel reads the buffers it names (validating their addresses itself).
///
/// # Safety
/// `p` points to a `struct msghdr` whose name, iovec array, buffers and control data are
/// readable for `'a` (the contract of `sendmsg`, §15.2).
// UNSAFE(slice): the caller's message header becomes a reference.
#[allow(unsafe_code)]
pub(crate) unsafe fn msghdr<'a>(p: *const c_void) -> KResult<&'a liella_kernel::mem::MsgHdr<'a>> {
    if !p.cast::<liella_kernel::mem::MsgHdr<'_>>().is_aligned() {
        return Err(Errno::EFAULT);
    }
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: aligned (checked); `as_ref` checks NULL; the contents are the caller's contract.
    unsafe { p.cast::<liella_kernel::mem::MsgHdr<'a>>().as_ref() }.ok_or(Errno::EFAULT)
}

/// A C `struct msghdr` for receiving, viewed in place: the kernel writes the buffers it names
/// and the length and flag fields.
///
/// # Safety
/// `p` points to a writable `struct msghdr` whose name, buffers and control data are
/// writable and referenced by nothing else for `'a` (the contract of `recvmsg`, §15.2).
// UNSAFE(slice): the caller's message header becomes a reference.
#[allow(unsafe_code)]
pub(crate) unsafe fn msghdr_mut<'a>(p: *mut c_void) -> KResult<&'a mut liella_kernel::mem::MsgHdrMut<'a>> {
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    unsafe { object_mut(p) }
}

/// `__cmsg_nxthdr`'s operands: the message's control buffer and the offset of `cmsg` in it
/// (`None` when `cmsg` is not inside it; the walk then ends).
///
/// # Safety
/// `mhdr` points to a `struct msghdr` whose control buffer is readable, and `cmsg` is a
/// header obtained from it (the contract of `CMSG_NXTHDR`, §15.2).
// UNSAFE(slice): the caller's control buffer becomes a slice.
#[allow(unsafe_code)]
pub(crate) unsafe fn cmsg_area<'a>(mhdr: *const c_void, cmsg: *const c_void) -> Option<(&'a [u8], usize)> {
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    let (control, len) = unsafe { msghdr(mhdr) }.ok()?.control_area();
    let off = cmsg.addr().checked_sub(control).filter(|&o| o < len)?;
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract: `len` readable bytes at `control`.
    let area = unsafe { buf(core::ptr::with_exposed_provenance(control), len) }.ok()?;
    Some((area, off))
}

// ------------------------------------------------------------------------------ phase 5: §17

/// `__getgroups_chk`'s count: `size` entries of `gid_t` must fit in `listlen` bytes, else the
/// fortify failure (a negative size too: glibc multiplies it as a `size_t`).
pub(crate) fn groups_chk(size: c_int, listlen: usize) -> i64 {
    // A negative size names no array: it is `getgroups`' `EINVAL`, not an overflow (the
    // reference, probed 2026-10-04: -1 `EINVAL`; validation T10).
    let Ok(n) = u64::try_from(size) else { return i64::from(size) };
    i64::try_from(chk_elems(n, listlen, 4)).unwrap_or(-1)
}

/// A descriptor argument that must name an open file (`futimens`, `futimes`): a negative number
/// is `EBADF` (POSIX), never `AT_FDCWD`'s meaning in the system call beneath (validation T9).
pub(crate) fn file_fd(fd: c_int) -> KResult<Fd> {
    if fd < 0 { Err(Errno::EBADF) } else { Ok(Fd::new(fd)) }
}

// ------------------------------------------------------------------------------ phase 5: §20

/// A descriptor argument where -1 asks for a new one (`signalfd`).
pub(crate) fn opt_fd(n: c_int) -> Option<Fd> {
    (n != -1).then(|| Fd::new(n))
}


// ------------------------------------------------------------------------------ thread memory

/// Zero `len` bytes at `addr`, memory the thread runtime mapped and owns (a recycled
/// thread's TLS block, descriptor and key entries). Crate-private contract: nothing else
/// references those bytes while this runs (the thread that used them has exited).
pub(crate) fn zero_mapped(addr: usize, len: usize) {
    // UNSAFE(slice): the runtime's own memory, unreferenced (contract above).
    #[allow(unsafe_code)]
    // SAFETY: the contract above.
    let b = unsafe { bytes_at_mut(addr, len) };
    fill(b, 0);
}

/// Copy `len` bytes from the executable's TLS image at `from` into a new thread's TLS block
/// at `to`. Crate-private contract: `from` is the loaded `PT_TLS` image; `to` is in the
/// creator's fresh mapping.
pub(crate) fn copy_mapped(from: usize, to: usize, len: usize) {
    // UNSAFE(slice): the image, mapped with the executable.
    #[allow(unsafe_code)]
    // SAFETY: the contract above.
    let src = unsafe { bytes_at(from, len) };
    // UNSAFE(slice): the new TLS block.
    #[allow(unsafe_code)]
    // SAFETY: the contract above.
    let dst = unsafe { bytes_at_mut(to, len) };
    let _ = copy_units(dst, src);
}

/// Entry `i` of the NULL-terminated array of C strings at `addr` that an exec or spawn call
/// received (`argv`, spec §18.3–18.4). Crate-private contract: `addr` is the caller's 8-aligned
/// array, readable up to and including its NULL entry for the call, and `i` is at most that
/// entry's index (callers stop at the first 0).
pub(crate) fn exec_array_entry(addr: usize, i: usize) -> usize {
    let Some(at) = i.checked_mul(8).and_then(|o| addr.checked_add(o)) else { return 0 };
    if addr == 0 || at % 8 != 0 {
        return 0;
    }
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: an 8-aligned word of the caller's array, readable for the call (the contract).
    let w = unsafe { core::slice::from_raw_parts(core::ptr::with_exposed_provenance::<usize>(at), 1) };
    w.first().copied().unwrap_or(0)
}

/// Store `words` at `addr` (a new thread's start words, on its fresh stack). Crate-private
/// contract: `addr` is 8-aligned in the creator's fresh mapping.
pub(crate) fn store_words(addr: usize, words: &[usize]) {
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: the contract above.
    let w = unsafe { crate::tb::words64_at(addr, words.len()) };
    for (slot, &v) in w.iter().zip(words) {
        slot.store(v, core::sync::atomic::Ordering::Relaxed);
    }
}

/// Store `words` at `addr` in a stack a C caller gave `clone` (spec §18.2: the child runs on
/// that stack). Crate-private contract: `addr` is 8-aligned, just below the top of the stack the
/// caller passed, which `clone`'s contract makes writable memory the caller owns.
pub(crate) fn store_words_user(addr: usize, words: &[usize]) -> KResult<()> {
    if addr == 0 || addr % 8 != 0 {
        return Err(Errno::EINVAL);
    }
    store_words(addr, words);
    Ok(())
}

/// A block the library allocated for its own results (a thread's record buffer, §17.3–17.6,
/// D-17.3): the `T` at its start and the bytes after it up to `len`, with their address.
/// `None` for an address or size that cannot hold a `T`.
///
/// Crate-private contract: `addr` came from `caller_alloc::calloc` with at least `len` bytes
/// (so it is aligned for any C type and initialised), the library still owns it, and nothing
/// else references it while the views are used (only the thread that owns it uses it). `T` is
/// plain data, valid for every bit pattern (`CPod`).
pub(crate) fn own_record<T: CPod>(addr: usize, len: usize) -> Option<(&'static mut T, &'static mut [u8], usize)> {
    let rest = len.checked_sub(size_of::<T>())?;
    let after = addr.checked_add(size_of::<T>())?;
    if addr == 0 || addr.checked_rem(align_of::<T>()) != Some(0) || isize::try_from(len).is_err() {
        return None;
    }
    // UNSAFE(slice): the library's own block becomes its record.
    #[allow(unsafe_code)]
    // SAFETY: the contract above; aligned and non-NULL (checked), `size_of::<T>()` bytes in it.
    let rec = unsafe { &mut *core::ptr::with_exposed_provenance_mut::<T>(addr) };
    // UNSAFE(slice): the rest of the block, disjoint from the record.
    #[allow(unsafe_code)]
    // SAFETY: the contract above; `rest` bytes follow the record inside the block.
    let bytes = unsafe { core::slice::from_raw_parts_mut(core::ptr::with_exposed_provenance_mut::<u8>(after), rest) };
    Some((rec, bytes, after))
}

/// The first `len` bytes of a block the library allocated for its own results (`getlogin`'s
/// name, `getpass`'s line). Crate-private contract: as [`own_record`].
pub(crate) fn own_bytes(addr: usize, len: usize) -> Option<&'static mut [u8]> {
    if addr == 0 || isize::try_from(len).is_err() {
        return None;
    }
    // UNSAFE(slice): the library's own block becomes a byte slice.
    #[allow(unsafe_code)]
    // SAFETY: the contract above; non-NULL (checked), `len` bytes in the block.
    Some(unsafe { core::slice::from_raw_parts_mut(core::ptr::with_exposed_provenance_mut::<u8>(addr), len) })
}

/// The `len` bytes at `addr` that a descriptor records (a copied CPU mask in the thread's
/// mapping). Crate-private contract: as [`zero_mapped`], read-only.
pub(crate) fn mapped_bytes(addr: usize, len: usize) -> &'static [u8] {
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: the contract above.
    unsafe {
        bytes_at(addr, len)
    }
}

// ------------------------------------------------------------------------------ C objects

/// A C object of type `T` (`pthread_attr_t`, `pthread_mutex_t`, ...) as atomic 64-bit words:
/// `EINVAL` for NULL or a misaligned address (§22: those objects have no `EFAULT`).
///
/// # Safety
/// A non-NULL `p` points to a live `T` the caller owns for the call; the object is viewed only
/// atomically (D-22.1: the objects hold no pointers the library follows without checks).
// UNSAFE(slice): a C object's bytes become atomic words.
#[allow(unsafe_code)]
pub(crate) unsafe fn obj64<'a, T>(p: *const T) -> KResult<&'a [core::sync::atomic::AtomicUsize]> {
    let a = p.expose_provenance();
    if a == 0 || a % 8 != 0 {
        return Err(Errno::EINVAL);
    }
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract; 8-aligned, `size_of::<T>() / 8` whole words.
    let w = unsafe { crate::tb::words64_at(a, size_of::<T>() / 8) };
    Ok(w)
}

/// [`obj64`] where NULL means "none" (`pthread_create`'s attribute).
///
/// # Safety
/// As [`obj64`].
// UNSAFE(slice): see `obj64`.
#[allow(unsafe_code)]
pub(crate) unsafe fn opt_obj64<'a, T>(p: *const T) -> KResult<Option<&'a [core::sync::atomic::AtomicUsize]>> {
    if p.is_null() {
        return Ok(None);
    }
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    let w = unsafe { obj64(p) };
    w.map(Some)
}

/// A C object of type `T` as atomic 32-bit words (`pthread_mutex_t`, `sem_t`, `pthread_once_t`):
/// `EINVAL` for NULL or a misaligned address.
///
/// # Safety
/// As [`obj64`].
// UNSAFE(slice): a C object's bytes become atomic words.
#[allow(unsafe_code)]
pub(crate) unsafe fn obj32<'a, T>(p: *const T) -> KResult<&'a [core::sync::atomic::AtomicU32]> {
    let a = p.expose_provenance();
    if a == 0 || a % 4 != 0 {
        return Err(Errno::EINVAL);
    }
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract; 4-aligned, `size_of::<T>() / 4` whole words.
    let w = unsafe { crate::tb::words32_at(a, size_of::<T>() / 4) };
    Ok(w)
}

/// The stack a `ucontext_t` describes (`uc_stack.ss_sp`, `ss_size`): its lowest 8-aligned
/// address and its words, for `makecontext` to place a return address and arguments in.
///
/// # Safety
/// `uc` points to a `ucontext_t` whose `uc_stack` describes memory the caller owns and gives to
/// the context (spec §21.2: "on the stack in `uc.uc_stack`").
// UNSAFE(slice): the caller's context stack becomes atomic words.
#[allow(unsafe_code)]
pub(crate) unsafe fn ucontext_stack<'a>(uc: *const liella_abi::layout::ucontext_t) -> KResult<(usize, &'a [core::sync::atomic::AtomicUsize])> {
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    let w = unsafe { obj64(uc) }?;
    let load = |i: usize| w.get(i).map_or(0, |x| x.load(core::sync::atomic::Ordering::Relaxed));
    let (sp, size) = (load(2), load(4));
    let base = sp.checked_next_multiple_of(8).ok_or(Errno::EINVAL)?;
    let end = sp.checked_add(size).ok_or(Errno::EINVAL)?;
    if base == 0 || end <= base {
        return Err(Errno::EINVAL);
    }
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract; whole words inside the stack.
    let words = unsafe { crate::tb::words64_at(base, end.saturating_sub(base) / 8) };
    Ok((base, words))
}

/// A mutex (`pthread_mutex_t`, `mtx_t`) as its parts (spec §22.4): the six 32-bit words
/// (`__lock` ... `__elision`), the robust-list links (`__list.__prev`, `__list.__next`, 64-bit,
/// at offset 24), and the address of `__list.__next` (its robust-list entry). The two views
/// cover disjoint bytes. `EINVAL` for NULL or a misaligned address.
///
/// # Safety
/// As [`obj64`]: a live 40-byte object the caller owns for the call.
// UNSAFE(slice): a C mutex's bytes become atomic words.
#[allow(unsafe_code)]
pub(crate) unsafe fn mutex<'a, T>(p: *const T) -> KResult<(&'a [core::sync::atomic::AtomicU32], &'a [core::sync::atomic::AtomicUsize], usize)> {
    let a = p.expose_provenance();
    // MUTEX-5: two predicted branches to a cold EINVAL (LLVM had merged the tests into a
    // setcc/or chain on every lock and unlock).
    if a == 0 || size_of::<T>() < 40 {
        cold();
        return Err(Errno::EINVAL);
    }
    if a % 8 != 0 {
        cold();
        return Err(Errno::EINVAL);
    }
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract; bytes 0..24.
    let words = unsafe { crate::tb::words32_at(a, 6) };
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract; bytes 24..40, disjoint from the words above.
    let links = unsafe { crate::tb::words64_at(a.wrapping_add(24), 2) };
    Ok((words, links, a.wrapping_add(32)))
}

/// Marks the path that calls it unlikely (an empty cold function: the branch weight without
/// moving the result through memory).
#[cold]
#[inline(never)]
fn cold() {}

/// [`obj32`] where NULL means "none" (an attribute argument).
///
/// # Safety
/// As [`obj64`].
// UNSAFE(slice): see `obj32`.
#[allow(unsafe_code)]
pub(crate) unsafe fn opt_obj32<'a, T>(p: *const T) -> KResult<Option<&'a [core::sync::atomic::AtomicU32]>> {
    if p.is_null() {
        return Ok(None);
    }
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    let w = unsafe { obj32(p) };
    w.map(Some)
}

// ------------------------------------------------------------------------------ formatted I/O

/// The text a `%s` argument points at (§12.6): up to its NUL, at most `max` bytes. Crate-
/// private contract: `addr` is the non-NULL pointer the format's argument gave, which the C
/// caller's contract makes a string, or `max` readable bytes when a precision is given.
pub(crate) fn printf_text(addr: usize, max: Option<usize>) -> &'static [u8] {
    let p = core::ptr::with_exposed_provenance::<i8>(addr);
    let r = match max {
        // UNSAFE(slice): the contract above.
        #[allow(unsafe_code)]
        // SAFETY: the C caller's contract for the argument.
        Some(n) => unsafe { crate::strz::strz_n(p, n) },
        // UNSAFE(slice): the contract above.
        #[allow(unsafe_code)]
        // SAFETY: the C caller's contract for the argument.
        None => unsafe { crate::strz::strz(p) },
    };
    r.unwrap_or(&[])
}

/// The wide string a `%ls` argument points at, as [`printf_text`] for `wchar_t`.
pub(crate) fn printf_wide(addr: usize, max: Option<usize>) -> &'static [i32] {
    let p = core::ptr::with_exposed_provenance::<i32>(addr);
    let r = match max {
        // UNSAFE(slice): the contract of `printf_text`, for a wide string.
        #[allow(unsafe_code)]
        // SAFETY: the C caller's contract for the argument.
        Some(n) => unsafe { crate::strz::wcsz_n(p, n) },
        // UNSAFE(slice): the contract of `printf_text`, for a wide string.
        #[allow(unsafe_code)]
        // SAFETY: the C caller's contract for the argument.
        None => unsafe { crate::strz::wcsz(p) },
    };
    r.unwrap_or(&[])
}

/// Store `value` in the `size`-byte integer at `addr` (`%n`, scanf's conversions). Crate-
/// private contract: `addr` is the pointer the format's argument gave, which the C caller's
/// contract makes a writable object of that size.
pub(crate) fn store_int(addr: usize, size: usize, value: i64) {
    if addr == 0 {
        return;
    }
    let bytes = value.to_ne_bytes();
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: the C caller's contract for the argument.
    let dst = unsafe { bytes_at_mut(addr, size.min(8)) };
    dst.copy_from_slice(bytes.get(..dst.len()).unwrap_or(&[]));
}

/// Bytes the formatted-output functions write into a caller's buffer at `addr` (`sprintf`).
/// Crate-private contract: the C caller's contract makes `len` bytes at `addr` writable (for
/// `sprintf`, however long the result is: the caller vouches for it, §12.6, ⚑ HIST).
pub(crate) fn out_bytes(addr: usize, len: usize) -> &'static mut [u8] {
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: the C caller's contract for the destination.
    unsafe {
        bytes_at_mut(addr, len)
    }
}

// ------------------------------------------------------------------------------ resolver (§16.4)
//
// The resolver's C structures hold pointers the library follows: `struct __res_state`'s
// `dnsrch` strings and `_u._ext.nsaddrs` socket addresses, `ns_msg`'s message bounds, a
// `dnptrs` array's message start, `struct res_sym`'s names. By the C contract of each structure
// those pointers are NULL or name valid memory of the given kind; the helpers below make them
// slices under that contract (crate-private: the callers pass only pointers read from such a
// structure, or from a C argument the function's contract gives the same meaning).

/// The C string at `addr` that a resolver structure names (see above); `None` for NULL.
pub(crate) fn named_text(addr: usize) -> Option<&'static [u8]> {
    if addr == 0 {
        return None;
    }
    // UNSAFE(slice): the contract of the structure that names the string (see above).
    #[allow(unsafe_code)]
    // SAFETY: a non-NULL pointer of a resolver structure names a C string (see above).
    unsafe { crate::strz::strz(core::ptr::with_exposed_provenance::<i8>(addr)) }.ok()
}

/// The `n` bytes at `addr` that a resolver structure or argument names (see above).
pub(crate) fn named_bytes(addr: usize, n: usize) -> Option<&'static [u8]> {
    if addr == 0 || isize::try_from(n).is_err() {
        return None;
    }
    // UNSAFE(slice): the contract of the structure or argument (see above).
    #[allow(unsafe_code)]
    // SAFETY: the C contract: `n` readable bytes at `addr`.
    Some(unsafe { bytes_at(addr, n) })
}

/// The `n` writable bytes at `addr` that a resolver argument names (`dn_comp`'s message from
/// `dnptrs[0]` to the end of the output; see above).
pub(crate) fn named_bytes_mut(addr: usize, n: usize) -> Option<&'static mut [u8]> {
    if addr == 0 || isize::try_from(n).is_err() {
        return None;
    }
    // UNSAFE(slice): the contract of the argument (see above).
    #[allow(unsafe_code)]
    // SAFETY: the C contract: `n` writable bytes at `addr`, used only for the call.
    Some(unsafe { bytes_at_mut(addr, n) })
}

/// The environment contract table's `n` slots at `addr` as atomics (D-1.12). Crate-private
/// contract: `addr` is 0 (no slots: an empty view) or a block of `n` words that
/// `crate::process` allocated from this library's own heap for the table and never frees, so
/// the view stays valid for the life of the process; lock-free lookups and the writers (under
/// the environment lock) share it through atomic accesses only.
pub(crate) fn env_table_words(addr: usize, n: usize) -> &'static [core::sync::atomic::AtomicUsize] {
    if addr == 0 || addr % 8 != 0 {
        return &[];
    }
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: an 8-aligned block of `n` words, allocated for the table and never freed (above).
    unsafe {
        crate::tb::words64_at(addr, n)
    }
}

/// The `n` 64-bit words at `addr` as atomics: a `struct gaicb` of a `getaddrinfo_a` list, or a
/// `struct aiocb` of an asynchronous I/O request and the `pthread_attr_t` its `sigevent` names
/// (§11.11) (their contracts keep the object valid, and the caller does not change it, until the
/// request is done; the library and the caller then share it through atomic accesses only).
/// `None` for NULL or a misaligned address.
pub(crate) fn named_words(addr: usize, n: usize) -> Option<&'static [core::sync::atomic::AtomicUsize]> {
    if addr == 0 || addr % 8 != 0 {
        return None;
    }
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: an 8-aligned object of `n` words, valid while the request is pending (above).
    Some(unsafe { crate::tb::words64_at(addr, n) })
}

/// The `n` 32-bit words at `addr` as atomics: an io_uring instance's rings or submission entries
/// (D-11.8), which the kernel and the library write concurrently, so atomics are the only sound
/// view (io_uring(7): the rings are shared memory ordered by acquire and release on their head
/// and tail words). Crate-private contract: `addr..addr + 4 * n` lies in a `MAP_SHARED` mapping
/// of an io_uring descriptor that `crate::aio` made and keeps mapped while any thread may hold a
/// view: it unmaps only when the setup fails, before a view exists, and in a `fork` child, where
/// no other thread (and so no view) exists. `None` for 0 or a misaligned address.
pub(crate) fn ring_words(addr: usize, n: usize) -> Option<&'static [core::sync::atomic::AtomicU32]> {
    if addr == 0 || addr % 4 != 0 {
        return None;
    }
    // UNSAFE(slice): a kernel-shared mapping becomes atomic words (the contract above).
    #[allow(unsafe_code)]
    // SAFETY: a live, 4-aligned shared mapping of `n` words, viewed only atomically (above).
    Some(unsafe { crate::tb::words32_at(addr, n) })
}

/// The wire name at `addr` (labels up to the zero byte; at most 256 bytes read), for the
/// `ns_name_*` functions whose argument is a name without a length.
pub(crate) fn named_wire(addr: usize) -> Option<&'static [u8]> {
    if addr == 0 {
        return None;
    }
    let mut at = 0usize;
    loop {
        let b = *named_bytes(addr.checked_add(at)?, 1)?.first()?;
        if b == 0 || b & 0xc0 != 0 {
            // The end, or not a plain label (the caller reports the error).
            return named_bytes(addr, at.checked_add(1)?);
        }
        at = at.checked_add(usize::from(b))?.checked_add(1)?;
        if at > 255 {
            return named_bytes(addr, 256);
        }
    }
}

/// The floating value a printf handler's first argument points at (`printf_size`,
/// `__printf_fp`, spec §12.11): a `double`'s bits, or a `long double`'s 80 bits. Crate-private
/// contract: `args` is the argument array a handler receives (`const void *const *`), whose
/// first entry points at a value of that type; NULL reads as 0.
pub(crate) fn printf_float_arg(args: usize, long_double: bool) -> u128 {
    let word = |b: &[u8]| b.iter().rev().fold(0u128, |v, &x| (v << 8) | u128::from(x));
    // UNSAFE(slice): the contract above: the array's first entry.
    #[allow(unsafe_code)]
    // SAFETY: the C caller's contract for the handler arguments.
    let p = word(unsafe { bytes_at(args, 8) });
    let at = usize::try_from(p).unwrap_or(0);
    // UNSAFE(slice): the contract above: the value it points at.
    #[allow(unsafe_code)]
    // SAFETY: the C caller's contract for the handler arguments.
    let v = unsafe { bytes_at(at, if long_double { 10 } else { 8 }) };
    word(v)
}

// ------------------------------------------------------------------------------ regular expressions (§14.9)
//
// The regex functions receive the addresses the C caller passed (`crate::regex`): the
// compiled block a `regex_t` names, the subject, the `pmatch` array, the arrays of a
// `struct re_registers`, a fastmap. Crate-private contract of each helper below: `addr` is
// what the C caller passed for that argument (or what `regcomp`/`re_compile_pattern`/
// `re_search` stored in the structure), and the function's C contract makes `n` elements there
// valid for the call (POSIX `regexec`: `pmatch` has `nmatch` elements; the GNU interface:
// `string` has `length` bytes, the register arrays `num_regs` entries, a fastmap 256 bytes);
// NULL gives `None`.

impl CPod for liella_abi::layout::regmatch_t {}
impl CPod for i32 {}
impl CPod for u64 {}
impl CPod for usize {}

/// The `n` bytes of a compiled regex block or of a subject of known length.
pub(crate) fn regex_bytes(addr: usize, n: usize) -> Option<&'static [u8]> {
    if addr == 0 || isize::try_from(n).is_err() {
        return None;
    }
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: the C contract: `n` readable bytes at `addr`, not written during the call.
    Some(unsafe { bytes_at(addr, n) })
}

/// The NUL-terminated subject at `addr` (without its terminator).
pub(crate) fn regex_subject_z(addr: usize) -> Option<&'static [u8]> {
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: the C contract: a NUL-terminated string (NULL is refused by `strz`).
    unsafe { crate::strz::strz(core::ptr::with_exposed_provenance::<i8>(addr)) }.ok()
}

/// The `n` writable bytes of a fastmap.
pub(crate) fn regex_bytes_mut(addr: usize, n: usize) -> Option<&'static mut [u8]> {
    if addr == 0 || isize::try_from(n).is_err() {
        return None;
    }
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: the C contract: `n` writable bytes at `addr`, used only by this call.
    Some(unsafe { bytes_at_mut(addr, n) })
}

/// The `n` elements of a block the library allocated for the calling thread (`regexec`'s
/// scratch, D-14.11): at least `n * size_of::<T>()` bytes at `addr`, calloc'ed (aligned and
/// initialised), used by one call at a time.
pub(crate) fn own_array<T: CPod>(addr: usize, n: usize) -> Option<&'static mut [T]> {
    let n = i64::try_from(n).ok()?;
    if addr == 0 {
        return None;
    }
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: the calling thread's own block of at least `n` elements (calloc'ed, so aligned and
    // initialised), used only by the current call: `regexec` does not call back into the
    // application and is not async-signal-safe, so nothing else on this thread uses it meanwhile.
    unsafe { array_mut(core::ptr::with_exposed_provenance_mut::<c_void>(addr), n) }.ok()
}

/// The `n` entries of a `pmatch` array.
pub(crate) fn regmatch_array(addr: usize, n: usize) -> Option<&'static mut [liella_abi::layout::regmatch_t]> {
    let n = i64::try_from(n).ok()?;
    if addr == 0 {
        return None;
    }
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: the C contract: `n` writable `regmatch_t` at `addr` (NULL, misalignment and size are checked).
    unsafe { array_mut(core::ptr::with_exposed_provenance_mut::<c_void>(addr), n) }.ok()
}

/// The `n` entries of a register array (`regoff_t`).
pub(crate) fn regoff_array(addr: usize, n: usize) -> Option<&'static mut [i32]> {
    let n = i64::try_from(n).ok()?;
    if addr == 0 {
        return None;
    }
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: the C contract: `n` writable `regoff_t` at `addr` (NULL, misalignment and size are checked).
    unsafe { array_mut(core::ptr::with_exposed_provenance_mut::<c_void>(addr), n) }.ok()
}

// ------------------------------------------------------------------------------ glob and wordexp (§14.8, §14.10)
//
// `glob` and `wordexp` keep their results in vectors of string pointers they allocated from the
// caller's heap (`crate::caller_alloc`), which the caller's `glob_t`/`wordexp_t` names until
// `globfree`/`wordfree` (POSIX: the structure is passed back as the earlier call left it); with
// `GLOB_ALTDIRFUNC`, `gl_readdir` returns the caller's `struct dirent`. Crate-private contract of
// the helpers below: `addr` is what such a structure or callback holds or returned.

/// The `n` pointer words of a result vector an earlier `glob` or `wordexp` call allocated
/// (`gl_offs + gl_pathc`, or `we_offs + we_wordc`, entries). `None` for NULL or a misaligned
/// address.
pub(crate) fn result_vector(addr: usize, n: usize) -> Option<&'static [u64]> {
    if addr == 0 || addr % 8 != 0 || n.checked_mul(8).is_none_or(|b| isize::try_from(b).is_err()) {
        return None;
    }
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: a vector this library allocated with at least `n` 8-aligned words, not freed while
    // the caller's structure names it.
    Some(unsafe { words_at(addr, n) })
}

/// The `d_type` and name of the `struct dirent` a `gl_readdir` callback returned (its contract:
/// a valid entry, its name NUL-terminated, until the next call). `None` for NULL.
pub(crate) fn alt_dirent(addr: usize) -> Option<(u8, &'static [u8])> {
    let (kind_at, name_at) = (addr.checked_add(18)?, addr.checked_add(19)?);
    if addr == 0 {
        return None;
    }
    // UNSAFE(slice): the contract above (`d_type` at offset 18, spec Appendix C).
    #[allow(unsafe_code)]
    // SAFETY: the callback's contract: a readable `struct dirent` at `addr`.
    let kind = unsafe { bytes_at(kind_at, 1) }.first().copied()?;
    // UNSAFE(slice): the contract above (`d_name` at offset 19).
    #[allow(unsafe_code)]
    // SAFETY: the callback's contract: `d_name` is a NUL-terminated string.
    let name = unsafe { crate::strz::strz(core::ptr::with_exposed_provenance::<i8>(name_at)) }.ok()?;
    Some((kind, name))
}

/// A `const fenv_t *` or `const femode_t *` argument (spec §10.6): an object, or one of the
/// predefined environments the header encodes as the addresses −1 (`FE_DFL_ENV`,
/// `FE_DFL_MODE`) and −2 (`FE_NOMASK_ENV`).
pub(crate) enum FeArg<'a, T> {
    /// The caller's object.
    Object(&'a T),
    /// The default environment or modes.
    Default,
    /// All exception traps enabled (`FE_NOMASK_ENV`).
    NoMask,
    /// NULL.
    Null,
}

/// The `fenv_t`/`femode_t` argument at `p`.
///
/// # Safety
/// `p` is NULL, one of the predefined addresses, or the address of a readable object of type
/// `T` (the contract of `fesetenv`, `feupdateenv`, `fesetmode`, §10.6).
// UNSAFE(slice): a pointer argument becomes a reference to its object.
#[allow(unsafe_code)]
pub(crate) unsafe fn fe_arg<'a, T>(p: *const T) -> FeArg<'a, T> {
    match p.addr() {
        usize::MAX => FeArg::Default,
        a if a == usize::MAX - 1 => FeArg::NoMask,
        // UNSAFE(slice): the caller's object.
        #[allow(unsafe_code)]
        // SAFETY: the caller's contract; `as_ref` checks NULL, and a C object of the type is
        // aligned for it.
        _ => unsafe { p.as_ref() }.map_or(FeArg::Null, FeArg::Object),
    }
}

/// An old condition-variable block (`pthread_cond_*@GLIBC_2.2.5`, sync.rs) of `n` words as
/// (reference word, condition variable). Only sync.rs calls it, with an address its own
/// `old_block` allocated (`n` words, heap-aligned) and has not freed: the block is freed only
/// once the caller's word no longer names it and no waiter is inside.
pub(crate) fn old_cond_block<'a>(p: usize, n: usize) -> KResult<(&'a core::sync::atomic::AtomicU32, &'a [core::sync::atomic::AtomicU32])> {
    if p == 0 || p % 4 != 0 {
        return Err(Errno::EINVAL);
    }
    // UNSAFE(slice): see the function's contract.
    #[allow(unsafe_code)]
    // SAFETY: a live block of n words that sync.rs allocated; viewed only atomically.
    let b = unsafe { crate::tb::words32_at(p, n) };
    b.split_last().ok_or(Errno::EINVAL)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, clippy::byte_char_slices)]
mod tests {
    use super::*;

    #[test]
    fn internal_entries_copy_move_and_fill() {
        let mut a = [0u8; 300];
        let b: [u8; 300] = core::array::from_fn(|i| i as u8);
        let p: *mut c_void = a.as_mut_ptr().cast();
        let _ = liella_internal_memcpy(p, b.as_ptr().cast(), 300);
        assert_eq!(a, b);
        let p = a.as_mut_ptr();
        let _ = liella_internal_memmove(p.wrapping_add(1).cast(), p.cast_const().cast(), 299);
        assert_eq!(a[0], 0);
        assert_eq!(a[1..], b[..299]);
        let p: *mut c_void = a.as_mut_ptr().cast();
        let _ = liella_internal_memcpy(p, p.cast_const(), 300); // a self-copy, as LLVM may emit
        assert_eq!(liella_internal_memset(p, 0x1ab, 300), p);
        assert!(a.iter().all(|&v| v == 0xab));
        // Zero lengths accept NULL.
        assert!(liella_internal_memcpy(core::ptr::null_mut(), core::ptr::null(), 0).is_null());
        assert!(liella_internal_memset(core::ptr::null_mut(), 0, 0).is_null());
    }

    /// Every length 0..=64 through the internal entries, disjoint and overlapping both ways
    /// (the `liella-mem` pairs below 64 bytes and the loops above, PJ-20), against a copy
    /// through a separate buffer; bytes outside the destination are never touched.
    #[test]
    fn internal_entries_every_short_length_and_overlap() {
        let fresh: [u8; 200] = core::array::from_fn(|i| (i * 7 + 3) as u8);
        for n in 0..=64usize {
            for (s, d) in [(0usize, 100usize), (100, 0), (40, 40), (40, 41), (41, 40), (40, 47), (47, 40), (40, 56), (56, 40), (40, 40 + n / 2), (40 + n / 2, 40), (40, 40 + n), (40 + n, 40)] {
                let mut want = fresh;
                let tmp = fresh[s..s + n].to_vec();
                want[d..d + n].copy_from_slice(&tmp);
                for entry in [liella_internal_memmove as extern "C" fn(*mut c_void, *const c_void, usize) -> *mut c_void, liella_internal_memcpy] {
                    let mut a = fresh;
                    let base = a.as_mut_ptr();
                    let r = entry(base.wrapping_add(d).cast(), base.wrapping_add(s).cast_const().cast(), n);
                    assert_eq!(r, base.wrapping_add(d).cast(), "n {n} s {s} d {d}");
                    assert_eq!(a, want, "n {n} s {s} d {d}");
                }
            }
            for at in 0..8usize {
                let mut a = fresh;
                let p = a.as_mut_ptr().wrapping_add(at);
                assert_eq!(liella_internal_memset(p.cast(), 0x15a, n), p.cast());
                let mut want = fresh;
                want[at..at + n].fill(0x5a);
                assert_eq!(a, want, "fill n {n} at {at}");
            }
        }
    }

    #[test]
    fn null_and_lengths_follow_s6() {
        assert_eq!(check(core::ptr::null(), 0), Ok(false));
        assert_eq!(check(core::ptr::null(), 1), Err(Errno::EFAULT));
        let x = 0u8;
        let p = core::ptr::from_ref(&x).cast::<c_void>();
        assert_eq!(check(p, usize::MAX), Err(Errno::EFAULT));
        assert_eq!(check(p, 1), Ok(true));
        assert_eq!(fd(-100), Fd::CWD);
    }
}

