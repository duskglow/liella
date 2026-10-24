//! Hot kernels (owner ruling 2026-10-08, plan S2 category *hot-kernel*, D-4.22): the small-size
//! copy and fill behind `memcpy`, `memmove`, `mempcpy`, `memset` and their fortified and
//! internal forms; and the small-size compare behind `memcmp`, `bcmp` and `__memcmpeq` (D-4.23,
//! with its own contract at the end of the file).
//!
//! Division of labour. The entry points here validate once, outside any loop: the size class
//! first (`1..=SMALL_MAX` takes the kernel, everything else the checked path in `conv`), then
//! one combined NULL test for the kernel's sizes. The kernel itself checks nothing.
//!
//! KERNEL CONTRACT (the one statement every `UNSAFE(hot-kernel)` site below relies on): called
//! with `1 <= n <= SMALL_MAX`, `d` valid for writing `n` bytes and `s` valid for reading `n`
//! bytes (the regions may overlap); it reads only `[s, s + n)`, writes only `[d, d + n)`, and
//! loads every source byte into registers before its first store, so overlapping regions are
//! moved correctly (`memcpy` is `memmove`, D-4.2) with no overlap test.
//!
//! Size classes (each one or two pairs of loads that may overlap each other, like the stores):
//! 16 to 32 bytes one 16-byte block from each end, 33 to 64 two, 65 to 128 four; 8 to 15 two
//! 8-byte words, 4 to 7 two 4-byte words, 2 to 3 two 2-byte words, 1 one byte. These are SSE2 (the
//! x86-64 baseline). On a CPU with AVX2 the 65 to 128 byte copy class runs its AVX2 build instead
//! (`avx2` below, D-4.25: two 32-byte blocks from each end), chosen by one flag test (`dispatch`)
//! on that class only. AVX2 copies at 33 to 64 bytes and AVX2 fills measured slower on Haswell
//! than these 16-byte blocks (`vzeroupper` and the tail jump cost more than the stores saved),
//! so the other classes and the fill kernel have no feature test.
//!
//! No value wider than two words is built (D-4.13): 16-byte vectors live in registers.
//!
//! With NULL folding on, the exported entries run the 1-64 byte classes in assembly instead
//! (`csys/src/machine/fold.S`, D-4.22 MEM-K4b): the same classes, but a NULL there must reach
//! the faulting access, which Rust cannot express (a NULL access is UB). The Rust 1-64 classes
//! below serve the checked entry and the internal entries. Every Rust path tests NULL first: the
//! 65-128 classes keep the explicit S6 checks with folding on (owner ruling 2026-10-08, option B).

// ARCH(x86_64): AVX2 intrinsics (hot-kernel); aarch64 needs a NEON or portable path (A-8)
use core::arch::x86_64::{
    __m128i, _mm_and_si128, _mm_cmpeq_epi8, _mm_cvtsi32_si128, _mm_movemask_epi8, _mm_shuffle_epi32, _mm_storeu_si128,
};

use liella_abi::ctypes::{c_int, c_void};
use liella_kernel::errno::Errno;

use crate::conv;
use crate::ret;

/// The largest size the kernel takes: four 16-byte blocks from each end.
pub const SMALL_MAX: usize = 128;

/// Whether `n` is in the kernel's range `1..=SMALL_MAX`: one compare (`n - 1` wraps for 0).
#[inline(always)]
fn small(n: usize) -> bool {
    n.wrapping_sub(1) < SMALL_MAX
}

/// The C contract's NULL outcome for a non-empty region (S6): the `EFAULT` abort, out of line.
#[cold]
#[inline(never)]
fn null_region() -> ! {
    ret::fatal(Errno::EFAULT)
}

/// 16 bytes at `p` into a register.
///
/// # Safety
/// `p` is valid for reading 16 bytes.
// UNSAFE(hot-kernel): an unaligned 16-byte load inside a region the kernel contract covers.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn ld16(p: *const u8) -> __m128i {
    // UNSAFE(hot-kernel) SAFETY: the function's contract; an unaligned read (`movups`). Not
    // `_mm_loadu_si128`: in a debug build its precondition check aborts on NULL before the
    // folded entries' page-zero fault (D-4.22) could happen.
    #[allow(unsafe_code)]
    unsafe {
        p.cast::<__m128i>().read_unaligned()
    }
}

/// A register's 16 bytes to `p`.
///
/// # Safety
/// `p` is valid for writing 16 bytes.
// UNSAFE(hot-kernel): an unaligned 16-byte store inside a region the kernel contract covers.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn st16(p: *mut u8, v: __m128i) {
    // UNSAFE(hot-kernel) SAFETY: the function's contract; `storeu` has no alignment requirement.
    #[allow(unsafe_code)]
    unsafe {
        _mm_storeu_si128(p.cast(), v);
    }
}

/// `K` (1, 2 or 4) 16-byte blocks from the start and `K` ending at byte `n`, all loaded before
/// any store: `16 * K <= n <= 32 * K`, so the blocks cover `[0, n)`. The blocks are separate
/// locals, never an array: a 64-byte array would be a value wider than two words, which an
/// unoptimised build moves with a call to `memcpy`, this function's own caller (D-4.13).
///
/// # Safety
/// The kernel contract, with `n` in that range.
// UNSAFE(hot-kernel): the block class of the kernel.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn move_blocks<const K: usize>(d: *mut u8, s: *const u8, n: usize) {
    let t = n.wrapping_sub(K.wrapping_mul(16));
    // Block `i` from the start (`ld!(i)`) or of the tail (`ld!(t + i)`): every offset + 16 <= n
    // by the class range, so every access is inside the regions the kernel contract covers.
    macro_rules! ld {
        ($off:expr) => {{
            // UNSAFE(hot-kernel) SAFETY: offset + 16 <= n (above): inside the source region.
            #[allow(unsafe_code)]
            let v = unsafe { ld16(s.wrapping_add($off)) };
            v
        }};
    }
    macro_rules! st {
        ($off:expr, $v:expr) => {{
            // UNSAFE(hot-kernel) SAFETY: offset + 16 <= n (above): inside the destination region.
            #[allow(unsafe_code)]
            unsafe { st16(d.wrapping_add($off), $v) };
        }};
    }
    let t16 = t.wrapping_add(16);
    if K == 1 {
        let (a, b) = (ld!(0), ld!(t));
        st!(0, a);
        st!(t, b);
    } else if K == 2 {
        let (a0, a1, b0, b1) = (ld!(0), ld!(16), ld!(t), ld!(t16));
        st!(0, a0);
        st!(16, a1);
        st!(t, b0);
        st!(t16, b1);
    } else {
        let (t32, t48) = (t.wrapping_add(32), t.wrapping_add(48));
        let (a0, a1, a2, a3) = (ld!(0), ld!(16), ld!(32), ld!(48));
        let (b0, b1, b2, b3) = (ld!(t), ld!(t16), ld!(t32), ld!(t48));
        st!(0, a0);
        st!(16, a1);
        st!(32, a2);
        st!(48, a3);
        st!(t, b0);
        st!(t16, b1);
        st!(t32, b2);
        st!(t48, b3);
    }
}

/// One `T` from the start and one ending at byte `n`, both loaded before either store:
/// `size_of::<T>() <= n <= 2 * size_of::<T>()`.
///
/// # Safety
/// The kernel contract, with `n` in that range.
// UNSAFE(hot-kernel): the word classes of the kernel.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn move_pair<T: Copy>(d: *mut u8, s: *const u8, n: usize) {
    let tail = n.wrapping_sub(size_of::<T>());
    // UNSAFE(hot-kernel) SAFETY: size_of::<T>() <= n: inside the source region; unaligned read.
    #[allow(unsafe_code)]
    let a = unsafe { s.cast::<T>().read_unaligned() };
    // UNSAFE(hot-kernel) SAFETY: tail + size_of::<T>() == n: inside the source region.
    #[allow(unsafe_code)]
    let b = unsafe { s.wrapping_add(tail).cast::<T>().read_unaligned() };
    // UNSAFE(hot-kernel) SAFETY: as the first load, in the destination region.
    #[allow(unsafe_code)]
    unsafe { d.cast::<T>().write_unaligned(a) };
    // UNSAFE(hot-kernel) SAFETY: as the second load, in the destination region.
    #[allow(unsafe_code)]
    unsafe { d.wrapping_add(tail).cast::<T>().write_unaligned(b) };
}

/// The copy kernel's classes up to 32 bytes (`1 <= n <= 32`).
///
/// # Safety
/// The kernel contract (module documentation), with `n <= 32`.
// UNSAFE(hot-kernel): dispatches to the size classes under the kernel contract.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn move_upto32(d: *mut u8, s: *const u8, n: usize) {
    if n >= 16 {
        // UNSAFE(hot-kernel) SAFETY: the kernel contract, 16 <= n <= 32.
        #[allow(unsafe_code)]
        unsafe { move_blocks::<1>(d, s, n) };
    } else if n >= 8 {
        // UNSAFE(hot-kernel) SAFETY: the kernel contract, 8 <= n < 16.
        #[allow(unsafe_code)]
        unsafe { move_pair::<u64>(d, s, n) };
    } else if n >= 4 {
        // UNSAFE(hot-kernel) SAFETY: the kernel contract, 4 <= n < 8.
        #[allow(unsafe_code)]
        unsafe { move_pair::<u32>(d, s, n) };
    } else if n >= 2 {
        // UNSAFE(hot-kernel) SAFETY: the kernel contract, 2 <= n < 4.
        #[allow(unsafe_code)]
        unsafe { move_pair::<u16>(d, s, n) };
    } else {
        // UNSAFE(hot-kernel) SAFETY: the kernel contract, n == 1.
        #[allow(unsafe_code)]
        unsafe { move_pair::<u8>(d, s, n) };
    }
}

/// The copy kernel's folded classes (`1 <= n <= FOLD_MAX` = 64, D-4.22): the 33-64 class is
/// tested first, so a 64-byte copy takes one class test after the entry's size test.
///
/// # Safety
/// The kernel contract (module documentation), with `n <= 64`.
// UNSAFE(hot-kernel): dispatches to the size classes under the kernel contract.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn move_upto64(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    if n > 32 {
        // UNSAFE(hot-kernel) SAFETY: the kernel contract, 32 < n <= 64.
        #[allow(unsafe_code)]
        unsafe { move_blocks::<2>(d, s, n) };
    } else {
        // UNSAFE(hot-kernel) SAFETY: the kernel contract, 1 <= n <= 32.
        #[allow(unsafe_code)]
        unsafe { move_upto32(d, s, n) };
    }
    d
}

/// The copy kernel: `n` bytes from `s` to `d`, overlap allowed; returns `d` (so the AVX2 build
/// is a tail call).
///
/// # Safety
/// The kernel contract (module documentation).
// UNSAFE(hot-kernel): dispatches to the size classes under the kernel contract.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn move_small(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
    if n <= 64 {
        // UNSAFE(hot-kernel) SAFETY: the kernel contract, 1 <= n <= 64.
        #[allow(unsafe_code)]
        return unsafe { move_upto64(d, s, n) };
    }
    if crate::dispatch::avx2() {
        // UNSAFE(hot-kernel) SAFETY: the kernel contract, 64 < n <= SMALL_MAX = 128; the flag
        // is set only on a CPU with AVX2 whose YMM state the OS saves (`dispatch`).
        #[allow(unsafe_code)]
        return unsafe { avx2::move_blocks(d, s, n) };
    }
    // UNSAFE(hot-kernel) SAFETY: the kernel contract, 64 < n <= SMALL_MAX = 128.
    #[allow(unsafe_code)]
    unsafe { move_blocks::<4>(d, s, n) };
    d
}

/// `c` in all 16 bytes of a register: one multiply, `movd`, `pshufd` (four instructions; the
/// byte splat LLVM picks for `_mm_set1_epi8` takes five).
///
/// # Safety
/// None beyond SSE2, which is part of the x86-64 baseline; no memory is touched.
// UNSAFE(hot-kernel): register-only SSE2 intrinsics.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn splat(c: u8) -> __m128i {
    let w = u32::from(c).wrapping_mul(0x0101_0101);
    // UNSAFE(hot-kernel) SAFETY: SSE2 is part of the x86-64 baseline; no memory is touched.
    #[allow(unsafe_code)]
    let x = unsafe { _mm_cvtsi32_si128(i32::from_ne_bytes(w.to_ne_bytes())) };
    // UNSAFE(hot-kernel) SAFETY: as above.
    #[allow(unsafe_code)]
    unsafe {
        _mm_shuffle_epi32::<0>(x)
    }
}

/// `K` 16-byte stores of `v` from the start and `K` ending at byte `n`: `16 * K <= n <= 32 * K`.
///
/// # Safety
/// `d` is valid for writing `n` bytes, `n` in that range.
// UNSAFE(hot-kernel): the block class of the fill kernel.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn fill_blocks<const K: usize>(d: *mut u8, v: __m128i, n: usize) {
    let tail = n.wrapping_sub(K.wrapping_mul(16));
    for i in 0..K {
        let at = i.wrapping_mul(16);
        // UNSAFE(hot-kernel) SAFETY: at + 16 <= 16 * K <= n: inside the region.
        #[allow(unsafe_code)]
        unsafe { st16(d.wrapping_add(at), v) };
        // UNSAFE(hot-kernel) SAFETY: tail + at + 16 <= n: inside the region.
        #[allow(unsafe_code)]
        unsafe { st16(d.wrapping_add(tail.wrapping_add(at)), v) };
    }
}

/// One `T` of `v` at the start and one ending at byte `n`: `size_of::<T>() <= n <= 2 * size_of::<T>()`.
///
/// # Safety
/// `d` is valid for writing `n` bytes, `n` in that range.
// UNSAFE(hot-kernel): the word classes of the fill kernel.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn fill_pair<T: Copy>(d: *mut u8, v: T, n: usize) {
    let tail = n.wrapping_sub(size_of::<T>());
    // UNSAFE(hot-kernel) SAFETY: size_of::<T>() <= n: inside the region; unaligned write.
    #[allow(unsafe_code)]
    unsafe { d.cast::<T>().write_unaligned(v) };
    // UNSAFE(hot-kernel) SAFETY: tail + size_of::<T>() == n: inside the region.
    #[allow(unsafe_code)]
    unsafe { d.wrapping_add(tail).cast::<T>().write_unaligned(v) };
}

/// The fill kernel's folded classes (`1 <= n <= FOLD_MAX` = 64, D-4.22), 33-64 tested first.
///
/// # Safety
/// `1 <= n <= 64` and `d` valid for writing `n` bytes; writes only `[d, d + n)`.
// UNSAFE(hot-kernel): dispatches to the fill size classes.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn fill_upto64(d: *mut u8, c: u8, n: usize) -> *mut u8 {
    if n > 32 {
        // UNSAFE(hot-kernel) SAFETY: SSE2 is part of the x86-64 baseline; no memory is touched.
        #[allow(unsafe_code)]
        let v = unsafe { splat(c) };
        // UNSAFE(hot-kernel) SAFETY: the function's contract, 32 < n <= 64.
        #[allow(unsafe_code)]
        unsafe { fill_blocks::<2>(d, v, n) };
    } else if n >= 16 {
        // UNSAFE(hot-kernel) SAFETY: SSE2 is part of the x86-64 baseline; no memory is touched.
        #[allow(unsafe_code)]
        let v = unsafe { splat(c) };
        // UNSAFE(hot-kernel) SAFETY: the function's contract, 16 <= n <= 32.
        #[allow(unsafe_code)]
        unsafe { fill_blocks::<1>(d, v, n) };
    } else if n >= 8 {
        // The byte broadcasts below multiply: an unoptimised build turns `[c; 8]` into a call to
        // `memset`, which in a test executable is this library's own and recursed (MEM-K4b gate).
        // UNSAFE(hot-kernel) SAFETY: the function's contract, 8 <= n < 16.
        #[allow(unsafe_code)]
        unsafe { fill_pair::<u64>(d, u64::from(c).wrapping_mul(0x0101_0101_0101_0101), n) };
    } else if n >= 4 {
        // UNSAFE(hot-kernel) SAFETY: the function's contract, 4 <= n < 8.
        #[allow(unsafe_code)]
        unsafe { fill_pair::<u32>(d, u32::from(c).wrapping_mul(0x0101_0101), n) };
    } else if n >= 2 {
        // UNSAFE(hot-kernel) SAFETY: the function's contract, 2 <= n < 4.
        #[allow(unsafe_code)]
        unsafe { fill_pair::<u16>(d, u16::from(c).wrapping_mul(0x0101), n) };
    } else {
        // UNSAFE(hot-kernel) SAFETY: the function's contract, n == 1.
        #[allow(unsafe_code)]
        unsafe { fill_pair::<u8>(d, c, n) };
    }
    d
}

/// The fill kernel: `n` bytes at `d` set to `c`; returns `d`.
///
/// # Safety
/// `1 <= n <= SMALL_MAX` and `d` valid for writing `n` bytes; writes only `[d, d + n)`.
// UNSAFE(hot-kernel): dispatches to the fill size classes.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn fill_small(d: *mut u8, c: u8, n: usize) -> *mut u8 {
    if n <= 64 {
        // UNSAFE(hot-kernel) SAFETY: the function's contract, 1 <= n <= 64.
        #[allow(unsafe_code)]
        return unsafe { fill_upto64(d, c, n) };
    }
    // UNSAFE(hot-kernel) SAFETY: SSE2 is part of the x86-64 baseline; no memory is touched.
    #[allow(unsafe_code)]
    let v = unsafe { splat(c) };
    // UNSAFE(hot-kernel) SAFETY: the function's contract, 64 < n <= SMALL_MAX = 128.
    #[allow(unsafe_code)]
    unsafe { fill_blocks::<4>(d, v, n) };
    d
}

/// The large copies and moves: from 129 bytes, the kernel [`bulk_move`] picks (MEM-BULK,
/// MEM-FALLBACK). Out of line, so the small path carries none of its register saves; the entry
/// tail-calls it. Every arm is a tail call, so this function keeps no frame (an inline checked
/// path made LLVM save five registers here on the AVX2 path too: +11 instructions per call).
///
/// # Safety
/// The `memmove` contract for `n` bytes.
// UNSAFE(hot-kernel): forwards the caller's contract to the kernels and the checked path.
#[allow(unsafe_code)]
#[inline(never)]
unsafe fn move_large(d: *mut c_void, s: *const c_void, n: usize) -> *mut c_void {
    match bulk_move(n, crate::dispatch::avx2()) {
        // 129 bytes to 64 KiB with AVX2, overlapping or not (MEM-BULK): straight to the assembly
        // copy, which holds its first and last 32 bytes until the end and picks the direction from
        // the overlap, so `memmove` takes the same path as `memcpy` (it ran the generic loops before).
        Bulk::Avx2 => {
            if d.is_null() || s.is_null() {
                null_region();
            }
            // Proof obligations of the declarations (`ffi`): both non-NULL (tested above), the
            // caller's contract; disjoint regions for `rep movsb` (`avx2_erms`); AVX2 with the YMM
            // state saved (the kernel flag, `dispatch`) and 64 <= 129 <= n for the AVX2 move.
            if avx2_erms(d.addr(), s.addr(), n) {
                return crate::ffi::liella_machine_copy_erms_a64(d.cast(), s.cast(), n).cast();
            }
            crate::ffi::liella_machine_move_avx2(d.cast(), s.cast(), n).cast()
        }
        // UNSAFE(hot-kernel) SAFETY: the function's contract, 129 <= n <= 64 KiB (`bulk_move`).
        #[allow(unsafe_code)]
        Bulk::Fallback => unsafe { move_fallback(d, s, n) },
        // UNSAFE(hot-kernel) SAFETY: the function's contract, unchanged.
        #[allow(unsafe_code)]
        _ => unsafe { move_checked(d, s, n) },
    }
}

/// 129 bytes to 64 KiB without AVX2 (MEM-FALLBACK): the kernel [`fallback_move`] picks.
///
/// # Safety
/// The `memmove` contract for `n` bytes, `129 <= n <= 64 KiB`.
// UNSAFE(hot-kernel): forwards the caller's contract to the kernels.
#[allow(unsafe_code)]
#[inline(never)]
unsafe fn move_fallback(d: *mut c_void, s: *const c_void, n: usize) -> *mut c_void {
    if d.is_null() || s.is_null() {
        null_region();
    }
    // Proof obligations of the declarations (`ffi`): 64 <= 129 <= n, both non-NULL (tested
    // above), the caller's contract; disjoint regions for `rep movsb` (`fallback_move`).
    match fallback_move(d.addr(), s.addr(), n) {
        Bulk::Erms => {
            crate::ffi::liella_machine_copy_erms(d.cast(), s.cast(), n);
            d
        }
        Bulk::ErmsA64 => crate::ffi::liella_machine_copy_erms_a64(d.cast(), s.cast(), n).cast(),
        _ => crate::ffi::liella_machine_move_sse2(d.cast(), s.cast(), n).cast(),
    }
}

/// The checked path for sizes outside the kernels' range (and 0): `conv::move_units`.
///
/// # Safety
/// The `memmove` contract for `n` bytes.
// UNSAFE(hot-kernel): forwards the caller's contract to the checked path.
#[allow(unsafe_code)]
#[inline(never)]
unsafe fn move_checked(d: *mut c_void, s: *const c_void, n: usize) -> *mut c_void {
    // UNSAFE(hot-kernel) SAFETY: the function's contract, unchanged.
    #[allow(unsafe_code)]
    let r = unsafe { conv::move_units::<u8>(d, s, n) };
    // `black_box`: were the result visibly `d`, the entry would keep `d` in a saved register across
    // the call on every path (push, mov, pop on the small copies too) instead of tail-calling.
    ret::same(core::hint::black_box(d), r)
}

/// The large fills: from 129 bytes, the kernel [`bulk_fill`] picks; every arm a tail call (as
/// [`move_large`]).
///
/// # Safety
/// `d` is valid for writing `n` bytes (the `memset` contract).
// UNSAFE(hot-kernel): forwards the caller's contract to the kernels and the checked path.
#[allow(unsafe_code)]
#[inline(never)]
unsafe fn fill_large(d: *mut c_void, c: u8, n: usize) -> *mut c_void {
    // The flags as decided (both false before the startup decision: the checked path, which
    // decides them).
    match bulk_fill(n, crate::dispatch::avx2(), crate::cpu::erms_decided(), crate::cpu::fill_erms_min()) {
        // UNSAFE(hot-kernel) SAFETY: the function's contract, SMALL_MAX < n (`bulk_fill`).
        #[allow(unsafe_code)]
        Bulk::Erms => unsafe { fill_rep(d, c, n) },
        // Without `rep stosb` (no ERMS, e.g. Zen 1, or below its threshold), MEM-FALLBACK: the
        // AVX2 fill (aligned 32-byte stores, four per step) instead of the checked path's loop.
        Bulk::Avx2 => {
            if d.is_null() {
                null_region();
            }
            // Proof obligation of the declaration (`ffi`): AVX2 with the YMM state saved (the
            // kernel flag, `dispatch`), 128 < n, `d` non-NULL (tested above), the caller's contract.
            crate::ffi::liella_machine_fill_avx2(d.cast(), i32::from(c), n).cast()
        }
        Bulk::Sse2 => {
            if d.is_null() {
                null_region();
            }
            // Proof obligation of the declaration (`ffi`): SMALL_MAX (128) < n, `d` non-NULL
            // (tested above), the caller's contract.
            crate::ffi::liella_machine_fill_sse2(d.cast(), i32::from(c), n).cast()
        }
        // UNSAFE(hot-kernel) SAFETY: the function's contract, unchanged.
        #[allow(unsafe_code)]
        _ => unsafe { fill_checked(d, c, n) },
    }
}

/// From the `rep stosb` threshold on ERMS CPUs (MEM-BULK): the first 64 bytes by the 64-byte
/// class, then `rep stosb` from the first 64-byte boundary above `d` (aligned `rep stosb`:
/// measured, `conv::fill`), without the checked path's layers.
///
/// # Safety
/// `d` is valid for writing `n` bytes (the `memset` contract), `SMALL_MAX < n`.
// UNSAFE(hot-kernel): forwards the caller's contract to the kernels.
#[allow(unsafe_code)]
#[inline(never)]
unsafe fn fill_rep(d: *mut c_void, c: u8, n: usize) -> *mut c_void {
    if d.is_null() {
        null_region();
    }
    let head = d.addr().wrapping_neg() & 63;
    // UNSAFE(hot-kernel) SAFETY: 64 < SMALL_MAX < n (whatever the tunable threshold), `d` non-NULL
    // (tested above), the caller's contract.
    #[allow(unsafe_code)]
    unsafe { fill_upto64(d.cast(), c, 64) };
    // Proof obligation of the declaration (`ffi`): `[d + head, d + n)` lies inside the caller's
    // region (head <= 63 < n).
    crate::ffi::liella_machine_fill_erms(d.cast::<u8>().wrapping_add(head), i32::from(c), n.wrapping_sub(head));
    d
}

/// The checked fill for sizes outside the kernels' range (and 0).
///
/// # Safety
/// `d` is valid for writing `n` bytes (the `memset` contract).
// UNSAFE(hot-kernel): forwards the caller's contract to the checked path.
#[allow(unsafe_code)]
#[inline(never)]
unsafe fn fill_checked(d: *mut c_void, c: u8, n: usize) -> *mut c_void {
    // UNSAFE(hot-kernel) SAFETY: the function's contract, unchanged.
    #[allow(unsafe_code)]
    let r = unsafe { conv::buf_mut(d, n) };
    // `black_box`: as in `move_checked`.
    ret::same(core::hint::black_box(d), r.map(|b| conv::fill(b, c)))
}

/// The copy entry's other sizes (from the direct entry, machine/fold.S): 65-128 always, every kernel size without NULL folding (both
/// with the explicit S6 NULL checks), 0 and the large sizes (the checked path). Out of line, so
/// the folded classes in the entry carry none of this. No folded NULL reaches this Rust code
/// (owner ruling 2026-10-08, option B, D-4.22 MEM-K4c): folding covers only the 1-64 classes,
/// which run in assembly.
///
/// # Safety
/// The `memmove` contract for `n` bytes.
// UNSAFE(hot-kernel): the validated entry to the copy kernel for the sizes the entry passes on.
#[allow(unsafe_code)]
// Inlined into its C-named caller `posix::s04_strings::hot_move_rest` (the direct entry in
// machine/fold.S tail-jumps there): one function, no extra hop.
#[inline(always)]
pub(crate) unsafe fn move_rest(d: *mut c_void, s: *const c_void, n: usize) -> *mut c_void {
    // The first small call decides NULL folding (STARTUP-1: lazy `mmap_min_addr` read).
    crate::dispatch::resolve_fold(n);
    if small(n) {
        // The NULL tests, folding on or off: two fused test-and-branch uops, never taken in a
        // correct program. A NULL access is UB in Rust, so this path never relies on the fault.
        if d.is_null() || s.is_null() {
            null_region();
        }
        // UNSAFE(hot-kernel) SAFETY: 1 <= n <= SMALL_MAX, both non-NULL (tested above), the
        // caller's contract.
        #[allow(unsafe_code)]
        return unsafe { move_small(d.cast(), s.cast(), n) }.cast();
    }
    // UNSAFE(hot-kernel) SAFETY: the function's contract.
    #[allow(unsafe_code)]
    unsafe { move_large(d, s, n) }
}

/// The number of sizes `move_large` takes straight to an assembly kernel: `SMALL_MAX + 1` up to, not including,
/// `cpu::AVX2_COPY_END` (above it `rep movsb` wins for disjoint regions, D-4.14).
const BULK_SPAN: usize = crate::cpu::AVX2_COPY_END.wrapping_sub(SMALL_MAX.wrapping_add(1));

/// The kernel a large copy, move or fill takes from its entry (MEM-FALLBACK); `Checked` is the
/// checked path (`conv`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Bulk {
    /// The AVX2 copy or fill (`dispatch::avx2` only).
    Avx2,
    /// A copy without AVX2: [`fallback_move`] decides between the next two.
    Fallback,
    /// The SSE2 move (the x86-64 baseline).
    Sse2,
    /// `rep movsb` / `rep stosb` (ERMS CPUs, from their thresholds).
    Erms,
    /// One 64-byte head store, then `rep movsb` from the aligned destination
    /// (`liella_machine_copy_erms_a64`; copies without the AVX2 copy, [`erms_kernel`]).
    ErmsA64,
    /// The checked path.
    Checked,
}

/// The kernel for a `memmove`/`memcpy` of `n` bytes with the kernels' AVX2 build on or off:
/// 129 bytes to 64 KiB, AVX2 when it may run (MEM-BULK), otherwise the fallback (MEM-FALLBACK);
/// other sizes the checked path.
#[inline(always)]
pub(crate) fn bulk_move(n: usize, avx2: bool) -> Bulk {
    if n.wrapping_sub(SMALL_MAX.wrapping_add(1)) >= BULK_SPAN {
        Bulk::Checked
    } else if avx2 {
        Bulk::Avx2
    } else {
        Bulk::Fallback
    }
}

/// [`Bulk::Fallback`] for `n` bytes to address `d` from `s`: overlapping regions take the SSE2
/// move, disjoint ones on ERMS CPUs the choice of `conv::copy_large` ([`erms_kernel`]), the SSE2
/// move otherwise, also before the startup decision.
#[inline(always)]
pub(crate) fn fallback_move(d: usize, s: usize, n: usize) -> Bulk {
    if d.wrapping_sub(s) >= n && s.wrapping_sub(d) >= n && crate::cpu::erms_decided() { erms_kernel(d, s, n) } else { Bulk::Sse2 }
}

/// The kernel for a disjoint copy of `n` bytes to address `d` from `s` that the AVX2 copy does
/// not take, on an ERMS CPU after the startup decision (MEMCPY-NONAVX2-THRESHOLD, D-4.14):
/// below `cpu::copy_erms_min()` [`Bulk::Sse2`]; above the AVX2 copy's range [`Bulk::Erms`] from
/// `cpu::erms_large_from()` (below it `Sse2`: the caller takes the AVX2 copy); otherwise, with a
/// 64-byte-aligned destination, `Erms` where `conv::erms_placement_ok` accepts the placement
/// (Ivy Bridge: the head-aligned kernel was 1-4% slower there, it only adds a head store);
/// [`Bulk::ErmsA64`] when the addresses agree modulo 64, `Sse2` at a placement
/// `erms_placement_ok` rejects, `ErmsA64` below `cpu::ERMS_A64_MISALIGNED_END` and
/// [`Bulk::Erms`] from it.
#[inline(always)]
pub(crate) fn erms_kernel(d: usize, s: usize, n: usize) -> Bulk {
    if n < crate::cpu::copy_erms_min() {
        Bulk::Sse2
    } else if n >= crate::cpu::AVX2_COPY_END {
        if n >= crate::cpu::erms_large_from() { Bulk::Erms } else { Bulk::Sse2 }
    } else if d & 63 == 0 {
        if conv::erms_placement_ok(d, s, n) { Bulk::Erms } else { Bulk::Sse2 }
    } else if d.wrapping_sub(s) & 63 == 0 {
        Bulk::ErmsA64
    } else if !conv::erms_placement_ok(d, s, n) {
        Bulk::Sse2
    } else if n < crate::cpu::ERMS_A64_MISALIGNED_END {
        Bulk::ErmsA64
    } else {
        Bulk::Erms
    }
}

/// Whether a copy the AVX2 move would take ([`Bulk::Avx2`]) takes `rep movsb`
/// (`liella_machine_copy_erms_a64`) instead: disjoint regions whose addresses agree modulo 64,
/// in the CPU class's window `cpu::avx2_erms_from()..cpu::avx2_erms_to()` (empty without one
/// and before the startup decision; MEMCPY-POLICY-2, D-4.14). Overlapping moves never: `rep
/// movsb` runs forward only.
#[inline(always)]
pub(crate) fn avx2_erms(d: usize, s: usize, n: usize) -> bool {
    n >= crate::cpu::avx2_erms_from() && n < crate::cpu::avx2_erms_to() && d.wrapping_sub(s) & 63 == 0 && d.wrapping_sub(s) >= n && s.wrapping_sub(d) >= n
}

/// The kernel for a `memset` of `n` bytes: from 129 bytes, `rep stosb` from its threshold on
/// ERMS CPUs (MEM-BULK), otherwise the AVX2 fill when it may run (MEM-FALLBACK); else the
/// checked path.
#[inline(always)]
pub(crate) fn bulk_fill(n: usize, avx2: bool, erms: bool, erms_min: usize) -> Bulk {
    if n <= SMALL_MAX {
        Bulk::Checked
    } else if erms && n >= erms_min {
        Bulk::Erms
    } else if avx2 {
        Bulk::Avx2
    } else {
        // MEMSET-4K-SSE2: the SSE2 fill (the baseline; also the level before the startup decision).
        Bulk::Sse2
    }
}

/// `memmove` (and `memcpy`, D-4.2): `n` bytes from `s` to `d`; returns `d`. NULL with `n > 0`
/// is the `EFAULT` abort (S6), or with NULL folding (D-4.22, `dispatch`) for `1 <= n <= 64` the
/// page-zero fault in assembly (SIGSEGV), as glibc observably does.
///
/// # Safety
/// `d` is valid for writing and `s` for reading `n` bytes (they may overlap).
// UNSAFE(hot-kernel): the validated entry to the copy kernel.
#[allow(unsafe_code)]
#[inline(always)]
pub unsafe fn move_bytes(d: *mut c_void, s: *const c_void, n: usize) -> *mut c_void {
    // The direct entry (machine/fold.S, D-4.22 COPY-FILL): in the products it is `memcpy` and
    // `memmove` themselves; the folded classes run there (a NULL `d` or `s` must reach the load or
    // store that faults on page zero, UB in Rust) and every other size comes back to
    // [`move_rest`]. Proof obligation of the declaration (`ffi`): the caller's contract.
    crate::ffi::liella_machine_memmove(d.cast(), s.cast(), n).cast()
}

/// [`move_bytes`] returning `d + n` (`mempcpy`): the direct entry `liella_machine_mempcpy`
/// (machine/fold.S, WL-HOT-SEQ2), which runs the same folded classes and calls the copy entry for
/// every other size.
///
/// # Safety
/// `d` is valid for writing and `s` for reading `n` bytes (they may overlap).
// UNSAFE(hot-kernel): the validated entry to the copy kernel.
#[allow(unsafe_code)]
#[inline(always)]
pub unsafe fn move_bytes_past(d: *mut c_void, s: *const c_void, n: usize) -> *mut c_void {
    // Proof obligation of the declaration (`ffi`): the caller's contract.
    crate::ffi::liella_machine_mempcpy(d.cast(), s.cast(), n).cast()
}

/// The fill entry's other sizes, as [`move_rest`].
///
/// # Safety
/// `d` is valid for writing `n` bytes (the `memset` contract).
// UNSAFE(hot-kernel): the validated entry to the fill kernel for the sizes the entry passes on.
#[allow(unsafe_code)]
// As `move_rest` (caller `posix::s04_strings::hot_fill_rest`). `c` is C's `int`, passed on
// unchanged by the assembly.
#[inline(always)]
pub(crate) unsafe fn fill_rest(d: *mut c_void, c: c_int, n: usize) -> *mut c_void {
    let c = conv::byte(c);
    crate::dispatch::resolve_fold(n);
    if small(n) {
        // The NULL test, folding on or off (as in `move_rest`).
        if d.is_null() {
            null_region();
        }
        // UNSAFE(hot-kernel) SAFETY: 1 <= n <= SMALL_MAX, non-NULL (tested above), the caller's
        // contract.
        #[allow(unsafe_code)]
        return unsafe { fill_small(d.cast(), c, n) }.cast();
    }
    // UNSAFE(hot-kernel) SAFETY: the function's contract.
    #[allow(unsafe_code)]
    unsafe { fill_large(d, c, n) }
}

/// `memset`: `n` bytes at `d` set to `c`; returns `d`. NULL with `n > 0` is the `EFAULT` abort,
/// or with NULL folding for `1 <= n <= 64` the page-zero fault, as for [`move_bytes`].
///
/// # Safety
/// `d` is valid for writing `n` bytes.
// UNSAFE(hot-kernel): the validated entry to the fill kernel.
#[allow(unsafe_code)]
#[inline(always)]
pub unsafe fn set_bytes(d: *mut c_void, c: c_int, n: usize) -> *mut c_void {
    // The direct entry, as in `move_bytes` (`memset` itself in the products); it takes `c` as C's
    // `int` and uses its low byte (`(unsigned char)c`). Other sizes come back to [`fill_rest`].
    crate::ffi::liella_machine_memset(d.cast(), c, n).cast()
}

/// The library's internal copy (D-X.15): as [`move_bytes`] without the C-contract checks.
///
/// # Safety
/// `n == 0`, or `d` valid for writing and `s` for reading `n` bytes (`n <= isize::MAX`).
// UNSAFE(hot-kernel): the internal entry to the copy kernel.
#[allow(unsafe_code)]
#[inline(always)]
pub(crate) unsafe fn internal_move(d: *mut c_void, s: *const c_void, n: usize) -> bool {
    if n.wrapping_sub(1) < 64 {
        // UNSAFE(hot-kernel) SAFETY: 1 <= n <= 64 <= SMALL_MAX, the function's contract.
        #[allow(unsafe_code)]
        unsafe { move_small(d.cast(), s.cast(), n) };
        return true;
    }
    if n.wrapping_sub(65) < 192 {
        // 65-256: the export's straight-line classes without their NULL tests (COPY-256; 144 B,
        // the fstatat struct, took 85 instructions through `internal_large`). Proof obligation
        // of the declaration (`ffi`): 65 <= n <= 256 (tested here), both regions valid and
        // non-NULL (the function's contract with n > 0).
        crate::ffi::liella_machine_move_mid(d.cast(), s.cast(), n);
        return true;
    }
    // 257 bytes to 64 KiB: the export's kernels (COPY-INTERNAL; the slice path cost 45
    // instructions more per call).
    if bulk_move(n, false) != Bulk::Checked {
        // UNSAFE(hot-kernel) SAFETY: the function's contract, 129 <= n <= 64 KiB (`bulk_move`).
        #[allow(unsafe_code)]
        unsafe { internal_large(d, s, n) };
        return true;
    }
    false
}

/// [`internal_move`] from 257 bytes to 64 KiB: the kernel [`move_large`] picks, without its NULL
/// tests (the internal contract: `n > 0`, so both regions are valid and non-NULL). Before the
/// startup decision the kernel flag is off and the SSE2 move (the x86-64 baseline) runs, as for
/// the export. Out of line, so the small classes carry none of it.
///
/// # Safety
/// `d` valid for writing and `s` for reading `n` bytes, `257 <= n <= 64 KiB`.
// UNSAFE(hot-kernel): forwards the caller's contract to the kernels.
#[allow(unsafe_code)]
#[inline(never)]
unsafe fn internal_large(d: *mut c_void, s: *const c_void, n: usize) {
    // Proof obligations of the declarations (`ffi`): AVX2 with the YMM state saved (the kernel
    // flag, `dispatch`) for the AVX2 move; 64 <= 257 <= n, both non-NULL and valid (the function's
    // contract); disjoint regions for `rep movsb` (`fallback_move`).
    if crate::dispatch::avx2() && avx2_erms(d.addr(), s.addr(), n) {
        let _ = crate::ffi::liella_machine_copy_erms_a64(d.cast(), s.cast(), n);
    } else if crate::dispatch::avx2() {
        let _ = crate::ffi::liella_machine_move_avx2(d.cast(), s.cast(), n);
    } else {
        match fallback_move(d.addr(), s.addr(), n) {
            Bulk::Erms => {
                crate::ffi::liella_machine_copy_erms(d.cast(), s.cast(), n);
            }
            Bulk::ErmsA64 => {
                let _ = crate::ffi::liella_machine_copy_erms_a64(d.cast(), s.cast(), n);
            }
            _ => {
                let _ = crate::ffi::liella_machine_move_sse2(d.cast(), s.cast(), n);
            }
        }
    }
}

/// The library's internal fill (D-X.15): the kernel for `1..=SMALL_MAX` bytes; `false` otherwise.
///
/// # Safety
/// `n == 0`, or `d` valid for writing `n` bytes (`n <= isize::MAX`).
// UNSAFE(hot-kernel): the internal entry to the fill kernel.
#[allow(unsafe_code)]
#[inline(always)]
pub(crate) unsafe fn internal_fill(d: *mut c_void, c: u8, n: usize) -> bool {
    if small(n) {
        // UNSAFE(hot-kernel) SAFETY: 1 <= n <= SMALL_MAX, the function's contract.
        #[allow(unsafe_code)]
        unsafe { fill_small(d.cast(), c, n) };
        return true;
    }
    false
}

// ---------------------------------------------------------------------------------------------
// The small-size compare (MEM-K3, D-4.23) behind `memcmp`, `bcmp` and `__memcmpeq`.
//
// COMPARE KERNEL CONTRACT (the one statement every `UNSAFE(hot-kernel)` site of the compare relies
// on): called with `1 <= n <= CMP_MAX` and `a`, `b` each valid for reading `n` bytes; it reads only
// `[a, a + n)` and `[b, b + n)`, writes nothing, and returns the order of the first differing
// byte pair as `unsigned char` (-1, 0 or 1, the checked path's values). "Equal" is decided from
// one load of every byte; a difference found in the block classes is located by reading those
// blocks again, and should a concurrent writer (D-4.21) have made them equal meanwhile, the checked
// path compares the whole range once more and its own reads decide, as D-4.21 settles it there.
//
// Size classes (loads from both ends may overlap, like the copy's): 65 to 128 bytes four 16-byte
// blocks from each end (MEM-K5; AVX2: two 32-byte blocks), 33 to 64
// bytes two 16-byte blocks from each end, 16 to 32 one; 8 to 15 two big-endian 8-byte words as one
// 128-bit number, 4 to 7 two 4-byte words as one 64-bit number, 1 to 3 the bytes 0, n/2, n-1 as
// one 24-bit number. SSE2 only, like the copy.
// ---------------------------------------------------------------------------------------------

/// The largest size the compare kernel takes: four 16-byte blocks from each end (MEM-K5).
pub const CMP_MAX: usize = 128;

/// The end of the first size range the entry tests (1 to 64); 65 to [`CMP_MAX`] is the second.
const CMP_MID: usize = 64;

/// The largest size the compare kernel's long class takes (MEMCMP-ORDER): 64-byte (SSE2) or
/// 128-byte (AVX2) chunks from the start; larger sizes go to the checked path's scanner.
pub(crate) const CMP_LONG: usize = 2048;

/// -1, 0 or 1 for the order of two unsigned numbers: no branch.
#[inline(always)]
fn sign<T: PartialOrd>(x: T, y: T) -> c_int {
    c_int::from(x > y).wrapping_sub(c_int::from(x < y))
}

/// -1 or 1 for the order of two different bytes `p != q` (each below 256): the sign of `p - q`,
/// which fits an `i32`, ORed with 1 (MEM-ENTRY-2: `sub`, `sar`, `or` instead of [`sign`]'s compare
/// and two flag reads; the caller has already compared them).
#[inline(always)]
fn order_ne(p: u32, q: u32) -> c_int {
    (i32::from_ne_bytes(p.wrapping_sub(q).to_ne_bytes()) >> 31) | 1
}

/// The first 8 bytes at `p` as a native word (MEMCMP-HEAD's head probe).
///
/// # Safety
/// `p` is valid for reading 8 bytes.
// UNSAFE(hot-kernel): the head probe of the compare kernel.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn head8(p: *const u8) -> u64 {
    // UNSAFE(hot-kernel) SAFETY: the function's contract; an unaligned read.
    #[allow(unsafe_code)]
    unsafe {
        p.cast::<u64>().read_unaligned()
    }
}

/// -1 or 1 for the order of two different 8-byte words `x != y` read from memory: the order of
/// their bytes as big-endian numbers, i.e. of the first byte pair that differs (MEMCMP-HEAD:
/// `bswap` x2, `cmp`, `sbb`, `or`; no index search, no second read of the bytes).
#[inline(always)]
fn order_words(x: u64, y: u64) -> c_int {
    c_int::from(u64::from_be(x) < u64::from_be(y)).wrapping_neg() | 1
}

/// MEMCMP-HEAD's head probe: the order of the first 8 bytes at `a` and `b` when they differ (that
/// decides the whole compare), else `None`.
///
/// # Safety
/// `a` and `b` are each valid for reading 8 bytes.
// UNSAFE(hot-kernel): the head probe of the compare kernel.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn head_order(a: *const u8, b: *const u8) -> Option<c_int> {
    // UNSAFE(hot-kernel) SAFETY: the function's contract (this and the next).
    #[allow(unsafe_code)]
    let x = unsafe { head8(a) };
    // UNSAFE(hot-kernel) SAFETY: as above.
    #[allow(unsafe_code)]
    let y = unsafe { head8(b) };
    if x != y { Some(order_words(x, y)) } else { None }
}

/// 0xFF where the bytes of `x` and `y` are equal, 0 where they differ.
#[inline(always)]
fn eq_bytes(x: __m128i, y: __m128i) -> __m128i {
    // UNSAFE(hot-kernel) SAFETY: a register-only SSE2 operation (x86-64 baseline), no memory access.
    #[allow(unsafe_code)]
    unsafe {
        _mm_cmpeq_epi8(x, y)
    }
}

/// Both of `x` and `y` (byte-wise AND).
#[inline(always)]
fn both(x: __m128i, y: __m128i) -> __m128i {
    // UNSAFE(hot-kernel) SAFETY: a register-only SSE2 operation, no memory access.
    #[allow(unsafe_code)]
    unsafe {
        _mm_and_si128(x, y)
    }
}

/// The top bit of every byte of `e`: bit `i` from byte `i` (0xFFFF when all of `e` is set).
#[inline(always)]
fn mask(e: __m128i) -> i32 {
    // UNSAFE(hot-kernel) SAFETY: a register-only SSE2 operation, no memory access.
    #[allow(unsafe_code)]
    unsafe {
        _mm_movemask_epi8(e)
    }
}

/// The equality vector of the 16-byte blocks at offset `off` of `a` and `b`.
///
/// # Safety
/// `off + 16 <= n` for regions of `n` bytes at `a` and `b`.
// UNSAFE(hot-kernel): one block of the SSE2 compare classes.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn eq16(a: *const u8, b: *const u8, off: usize) -> __m128i {
    // UNSAFE(hot-kernel) SAFETY: the function's contract: inside the region at `a`.
    #[allow(unsafe_code)]
    let x = unsafe { ld16(a.wrapping_add(off)) };
    // UNSAFE(hot-kernel) SAFETY: as `x`, for the region at `b`.
    #[allow(unsafe_code)]
    let y = unsafe { ld16(b.wrapping_add(off)) };
    eq_bytes(x, y)
}

/// The differing bytes of a 16-byte equality vector as bits: bit `i` set when byte `i` differs.
#[inline(always)]
fn ne_bits16(e: __m128i) -> u64 {
    u64::from(u32::from_ne_bytes(mask(e).to_ne_bytes()) ^ 0xFFFF)
}

/// The index of the lowest set bit of `bits` (64 for none).
#[inline(always)]
fn low_bit(bits: u64) -> usize {
    usize::try_from(bits.trailing_zeros()).unwrap_or(0)
}

/// The first differing byte of `n` bytes at `a` and `b` in the class `16 * K <= n <= 32 * K`
/// (K = 1, 2 or 4), or `None` when they are equal. The blocks at 0, 16, ... `16 * (K - 1)` and
/// the K ending at `n` are all loaded and compared, their equality vectors ANDed and tested once
/// (equal inputs pay nothing more); only an unequal outcome turns the same vectors into bitmaps
/// of differing bytes: the head's lowest set bit (one `tzcnt`) or, when the head is equal, the
/// tail's plus its offset `t = n - 16 * K` (MEMCMP-ORDER: no second pass over the bytes; -3: a
/// branch instead of a variable shift). Bytes the head and tail blocks share are equal in the
/// tail when the head is, so that is the first difference. No separate
/// head probe: it cost equal inputs a cycle on shiki (MEMCMP-ORDER-3 bench) while the loads are
/// issued together anyway.
///
/// # Safety
/// The compare kernel contract, `n` in that range.
// UNSAFE(hot-kernel): the SSE2 block classes of the compare kernel.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn first_ne<const K: usize>(a: *const u8, b: *const u8, n: usize) -> Option<usize> {
    let t = n.wrapping_sub(K.wrapping_mul(16));
    // UNSAFE(hot-kernel) SAFETY: every offset below + 16 <= n by the class range (this and the
    // next seven): offsets 0, 16, 32, 48 below 16 * K and t, t + 16, ... ending at n.
    #[allow(unsafe_code)]
    let e0 = unsafe { eq16(a, b, 0) };
    // UNSAFE(hot-kernel) SAFETY: as above.
    #[allow(unsafe_code)]
    let f0 = unsafe { eq16(a, b, t) };
    if K == 1 {
        if mask(both(e0, f0)) == 0xFFFF {
            return None;
        }
        // Head block first, else the tail's lowest bit at its offset t (MEMCMP-ORDER-3: a branch,
        // not a variable shift - `shl %cl` is 3 uops on Intel; bytes the blocks share are equal
        // when the head is).
        let head = ne_bits16(e0);
        if head != 0 {
            return Some(low_bit(head));
        }
        return Some(t.wrapping_add(low_bit(ne_bits16(f0))));
    }
    // UNSAFE(hot-kernel) SAFETY: as above, 32 <= n.
    #[allow(unsafe_code)]
    let e1 = unsafe { eq16(a, b, 16) };
    // UNSAFE(hot-kernel) SAFETY: as above.
    #[allow(unsafe_code)]
    let f1 = unsafe { eq16(a, b, t.wrapping_add(16)) };
    if K == 2 {
        if mask(both(both(e0, e1), both(f0, f1))) == 0xFFFF {
            return None;
        }
        let head = ne_bits16(e0) | (ne_bits16(e1) << 16);
        if head != 0 {
            return Some(low_bit(head));
        }
        // As K = 1: the tail's lowest bit at its offset t.
        return Some(t.wrapping_add(low_bit(ne_bits16(f0) | (ne_bits16(f1) << 16))));
    }
    // UNSAFE(hot-kernel) SAFETY: as above, 64 <= n.
    #[allow(unsafe_code)]
    let e2 = unsafe { eq16(a, b, 32) };
    // UNSAFE(hot-kernel) SAFETY: as above.
    #[allow(unsafe_code)]
    let f2 = unsafe { eq16(a, b, t.wrapping_add(32)) };
    // UNSAFE(hot-kernel) SAFETY: as above, 64 <= n.
    #[allow(unsafe_code)]
    let e3 = unsafe { eq16(a, b, 48) };
    // UNSAFE(hot-kernel) SAFETY: as above.
    #[allow(unsafe_code)]
    let f3 = unsafe { eq16(a, b, t.wrapping_add(48)) };
    let all = both(both(both(e0, e1), both(e2, e3)), both(both(f0, f1), both(f2, f3)));
    if mask(all) == 0xFFFF {
        return None;
    }
    let head = ne_bits16(e0) | (ne_bits16(e1) << 16) | (ne_bits16(e2) << 32) | (ne_bits16(e3) << 48);
    if head != 0 {
        return Some(low_bit(head));
    }
    let tail = ne_bits16(f0) | (ne_bits16(f1) << 16) | (ne_bits16(f2) << 32) | (ne_bits16(f3) << 48);
    Some(t.wrapping_add(low_bit(tail)))
}

/// The SSE2 long class of the compare kernel, `CMP_MAX < n <= CMP_LONG`: 64-byte chunks from the
/// start (the last one ending at `n`), each tested once as [`first_ne`]'s 64-byte class; the
/// first unequal chunk's first differing byte decides (MEMCMP-ORDER).
///
/// # Safety
/// The compare kernel contract, `64 <= n`.
// UNSAFE(hot-kernel): the SSE2 long class of the compare kernel.
#[allow(unsafe_code)]
#[inline(never)]
unsafe fn cmp_long(a: *const u8, b: *const u8, n: usize) -> c_int {
    // Head-first probe (MEMCMP-ORDER-3), once before the loop: a difference in the first 16
    // bytes is decided without the chunk's other blocks.
    // UNSAFE(hot-kernel) SAFETY: 64 <= n: the block at 0 is inside both regions.
    #[allow(unsafe_code)]
    let e0 = unsafe { eq16(a, b, 0) };
    if mask(e0) != 0xFFFF {
        // UNSAFE(hot-kernel) SAFETY: the kernel contract, the index is below 16 <= n.
        #[allow(unsafe_code)]
        return unsafe { order_at(a, b, n, low_bit(ne_bits16(e0))) };
    }
    let last = n.wrapping_sub(64);
    let mut off = 0;
    loop {
        let at = off.min(last);
        // UNSAFE(hot-kernel) SAFETY: at + 64 <= n: the chunk is inside both regions.
        #[allow(unsafe_code)]
        if let Some(i) = unsafe { first_ne::<2>(a.wrapping_add(at), b.wrapping_add(at), 64) } {
            // UNSAFE(hot-kernel) SAFETY: the kernel contract, at + i < at + 64 <= n.
            #[allow(unsafe_code)]
            return unsafe { order_at(a, b, n, at.wrapping_add(i)) };
        }
        if at == last {
            return 0;
        }
        off = off.wrapping_add(64);
    }
}

/// One `T` from the start and one ending at byte `n` of `p`: `size_of::<T>() <= n`.
///
/// # Safety
/// `p` is valid for reading `n` bytes, `n` in that range.
// UNSAFE(hot-kernel): the word classes of the compare kernel.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn ends<T: Copy>(p: *const u8, n: usize) -> (T, T) {
    // UNSAFE(hot-kernel) SAFETY: size_of::<T>() <= n: inside the region; unaligned read.
    #[allow(unsafe_code)]
    let x = unsafe { p.cast::<T>().read_unaligned() };
    // UNSAFE(hot-kernel) SAFETY: offset n - size_of::<T>() ends at byte n: inside the region.
    #[allow(unsafe_code)]
    let y = unsafe { p.wrapping_add(n.wrapping_sub(size_of::<T>())).cast::<T>().read_unaligned() };
    (x, y)
}

/// The order of `n` bytes at `a` and `b` whose first difference a block's equality mask put at
/// byte `i`: that byte pair decides; equal on this second read (a writer changed the text during
/// the call, D-4.21), [`rescan_order`] decides.
///
/// # Safety
/// The compare kernel contract, `i < n`.
// UNSAFE(hot-kernel): the unequal outcome of a head-block probe.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn order_at(a: *const u8, b: *const u8, n: usize, i: usize) -> c_int {
    // UNSAFE(hot-kernel) SAFETY: i < n: inside both regions (this and the next).
    #[allow(unsafe_code)]
    let p = unsafe { byte_at(a, i) };
    // UNSAFE(hot-kernel) SAFETY: as `p`.
    #[allow(unsafe_code)]
    let q = unsafe { byte_at(b, i) };
    if p != q {
        return order_ne(p, q);
    }
    // UNSAFE(hot-kernel) SAFETY: the kernel contract, i < n.
    #[allow(unsafe_code)]
    unsafe { rescan_order(a, b, n, i) }
}

/// [`order_at`]'s rescan (D-4.21): the reported byte pair is equal on the second read, so a writer
/// changed the text during the call. Counted (self-tests), then one pass byte by byte from `i + 1`
/// whose own reads decide: the first pair that differs, else equal. On stable text a kernel never
/// gets here (a reported index too low lands here; the self-tests require the count to stay 0).
///
/// # Safety
/// The compare kernel contract, `i < n`.
// UNSAFE(hot-kernel): the cold rescan of the compare kernel's block classes.
#[allow(unsafe_code)]
#[cold]
#[inline(never)]
unsafe fn rescan_order(a: *const u8, b: *const u8, n: usize, i: usize) -> c_int {
    crate::strz::note_rescan();
    let mut j = i.wrapping_add(1);
    while j < n {
        // UNSAFE(hot-kernel) SAFETY: j < n: inside both regions (this and the next).
        #[allow(unsafe_code)]
        let p = unsafe { byte_at(a, j) };
        // UNSAFE(hot-kernel) SAFETY: as `p`.
        #[allow(unsafe_code)]
        let q = unsafe { byte_at(b, j) };
        if p != q {
            return order_ne(p, q);
        }
        j = j.wrapping_add(1);
    }
    0
}

/// The direct entry's rescan (machine/fold.S `.Lcmp_byte`/`.Lcmp_big_byte`, MEMCMP-MID-2; 17-64 since
/// MEMCMP-HEAD-2): the byte pair at `i`
/// that a 17-2048-byte block class reported as the first difference was equal on its re-read, so
/// [`rescan_order`] decides, counted as the Rust classes' rescans are (D-4.21). A position from the
/// assembly that is too low therefore shows in the self-tests' count instead of being answered
/// right by a silent recompute.
///
/// # Safety
/// The compare kernel contract, `i < n`, `a` and `b` non-NULL (the entry has read both at `i`).
// UNSAFE(hot-kernel): the cold rescan of the direct entry's block classes.
#[allow(unsafe_code)]
#[cold]
pub(crate) unsafe fn compare_rescan(a: *const c_void, b: *const c_void, n: usize, i: usize) -> c_int {
    debug_assert!(!a.is_null() && !b.is_null() && i < n);
    // UNSAFE(hot-kernel) SAFETY: the contract, i < n.
    #[allow(unsafe_code)]
    unsafe { rescan_order(a.cast(), b.cast(), n, i) }
}

/// The SSE2 compare classes `16 * K <= n <= 32 * K` (K = 1, 2 or 4) behind the head probe, whose
/// equal outcome settled bytes 0-7 (MEMCMP-EQ, D-4.47): the probe's word stands in for the block at
/// 0, so the class compares the word at 8 (bytes 8-15; one fused test-and-branch, cheaper than
/// ORing it into the vector test) and the blocks at 16, ... `16 * (K - 1)` and the K ending at `n` - the
/// same loads as before, without the block at 0's `pcmpeqb`/`pand` and register copy. Unequal: a
/// difference in bytes 8-15 is ordered from the two words already in registers (no index search,
/// no second read); else the head blocks' lowest differing bit, else the tail's at its offset
/// `t = n - 16 * K` (bytes before `t` lie below `16 * K`, all equal when the head is).
///
/// # Safety
/// The compare kernel contract, `n` in that range, and the first 8 bytes of `a` and `b` read equal.
// UNSAFE(hot-kernel): the block classes of the compare kernel.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn cmp_blocks<const K: usize>(a: *const u8, b: *const u8, n: usize) -> c_int {
    let t = n.wrapping_sub(K.wrapping_mul(16));
    // UNSAFE(hot-kernel) SAFETY: 16 <= n: bytes 8-15 are inside both regions (this and the next).
    #[allow(unsafe_code)]
    let x = unsafe { head8(a.wrapping_add(8)) };
    // UNSAFE(hot-kernel) SAFETY: as above.
    #[allow(unsafe_code)]
    let y = unsafe { head8(b.wrapping_add(8)) };
    if x != y {
        return order_words(x, y);
    }
    // UNSAFE(hot-kernel) SAFETY: every block offset below + 16 <= n by the class range (this and
    // the next six): 16, 32, 48 below 16 * K and t, t + 16, ... ending at n.
    #[allow(unsafe_code)]
    let f0 = unsafe { eq16(a, b, t) };
    let (head, tail) = if K == 1 {
        if mask(f0) == 0xFFFF {
            return 0;
        }
        (0, ne_bits16(f0))
    } else {
        // UNSAFE(hot-kernel) SAFETY: as above, 32 <= n.
        #[allow(unsafe_code)]
        let e1 = unsafe { eq16(a, b, 16) };
        // UNSAFE(hot-kernel) SAFETY: as above.
        #[allow(unsafe_code)]
        let f1 = unsafe { eq16(a, b, t.wrapping_add(16)) };
        if K == 2 {
            let all = both(e1, both(f0, f1));
            if mask(all) == 0xFFFF {
                return 0;
            }
            (ne_bits16(e1), ne_bits16(f0) | (ne_bits16(f1) << 16))
        } else {
            // UNSAFE(hot-kernel) SAFETY: as above, 64 <= n.
            #[allow(unsafe_code)]
            let e2 = unsafe { eq16(a, b, 32) };
            // UNSAFE(hot-kernel) SAFETY: as above.
            #[allow(unsafe_code)]
            let e3 = unsafe { eq16(a, b, 48) };
            // UNSAFE(hot-kernel) SAFETY: as above.
            #[allow(unsafe_code)]
            let f2 = unsafe { eq16(a, b, t.wrapping_add(32)) };
            // UNSAFE(hot-kernel) SAFETY: as above.
            #[allow(unsafe_code)]
            let f3 = unsafe { eq16(a, b, t.wrapping_add(48)) };
            let all = both(both(e1, both(e2, e3)), both(both(f0, f1), both(f2, f3)));
            if mask(all) == 0xFFFF {
                return 0;
            }
            (
                ne_bits16(e1) | (ne_bits16(e2) << 16) | (ne_bits16(e3) << 32),
                ne_bits16(f0) | (ne_bits16(f1) << 16) | (ne_bits16(f2) << 32) | (ne_bits16(f3) << 48),
            )
        }
    };
    let i = if head != 0 { low_bit(head).wrapping_add(16) } else { t.wrapping_add(low_bit(tail)) };
    // UNSAFE(hot-kernel) SAFETY: the kernel contract; a head bit is below 16 * (K - 1), so the
    // index is below 16 * K <= n; a tail bit is below 16 * K, so t + bit < n.
    #[allow(unsafe_code)]
    unsafe { order_at(a, b, n, i) }
}

/// One byte of `p` as a number.
///
/// # Safety
/// `p` is valid for reading byte `i`.
// UNSAFE(hot-kernel): the byte class of the compare kernel.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn byte_at(p: *const u8, i: usize) -> u32 {
    // UNSAFE(hot-kernel) SAFETY: the function's contract.
    #[allow(unsafe_code)]
    let v = unsafe { p.wrapping_add(i).read() };
    u32::from(v)
}

/// Bytes 0, `h` and `l` of `p` as one big-endian 24-bit number.
///
/// # Safety
/// `p` is valid for reading bytes 0, `h` and `l`.
// UNSAFE(hot-kernel): the byte class of the compare kernel.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn bytes3(p: *const u8, h: usize, l: usize) -> u32 {
    // UNSAFE(hot-kernel) SAFETY: the function's contract (this and the next two).
    #[allow(unsafe_code)]
    let x = unsafe { byte_at(p, 0) };
    // UNSAFE(hot-kernel) SAFETY: as above.
    #[allow(unsafe_code)]
    let y = unsafe { byte_at(p, h) };
    // UNSAFE(hot-kernel) SAFETY: as above.
    #[allow(unsafe_code)]
    let z = unsafe { byte_at(p, l) };
    (x << 16) | (y << 8) | z
}

/// The compare kernel's 65 to 128 byte class (MEM-K5): the AVX2 build where the CPU has it, else
/// four 16-byte blocks from each end.
///
/// # Safety
/// The compare kernel contract, `CMP_MID < n <= CMP_MAX`, the first 8 bytes read equal (the head probe).
// UNSAFE(hot-kernel): dispatches the 65 to 128 byte class under the compare kernel contract.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn cmp_upto128(a: *const u8, b: *const u8, n: usize) -> c_int {
    if crate::dispatch::avx2() {
        // UNSAFE(hot-kernel) SAFETY: the compare kernel contract, 64 < n <= 128; the flag is set
        // only on a CPU with AVX2 whose YMM state the OS saves (`dispatch`, D-4.25).
        #[allow(unsafe_code)]
        return unsafe { avx2::cmp_blocks4(a, b, n) };
    }
    // UNSAFE(hot-kernel) SAFETY: the compare kernel contract, 64 < n <= 128, bytes 0-7 equal
    // (this function's contract).
    #[allow(unsafe_code)]
    unsafe { cmp_blocks::<4>(a, b, n) }
}

/// The compare kernel's 33 to 64 byte class: the AVX2 build where the CPU has it, else two
/// 16-byte blocks from each end.
///
/// # Safety
/// The compare kernel contract, `32 < n <= CMP_MID`, the first 8 bytes read equal (the head probe).
// UNSAFE(hot-kernel): dispatches the 33 to 64 byte class under the compare kernel contract.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn cmp_upto64(a: *const u8, b: *const u8, n: usize) -> c_int {
    if crate::dispatch::avx2() {
        // UNSAFE(hot-kernel) SAFETY: the compare kernel contract, 32 < n <= CMP_MID = 64; the flag
        // is set only on a CPU with AVX2 whose YMM state the OS saves (`dispatch`, D-4.25).
        #[allow(unsafe_code)]
        return unsafe { avx2::cmp_blocks(a, b, n) };
    }
    // UNSAFE(hot-kernel) SAFETY: the compare kernel contract, 32 < n <= CMP_MID = 64, bytes 0-7
    // equal (this function's contract).
    #[allow(unsafe_code)]
    unsafe { cmp_blocks::<2>(a, b, n) }
}

/// The compare kernel up to 64 bytes: the order of `n` bytes at `a` against `n` bytes at `b`.
///
/// # Safety
/// The compare kernel contract, `n <= CMP_MID`.
// UNSAFE(hot-kernel): dispatches to the size classes under the compare kernel contract.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn cmp_small(a: *const u8, b: *const u8, n: usize) -> c_int {
    // Head probe first in every class from 8 bytes (MEMCMP-HEAD): most unequal inputs differ in
    // their first 8 bytes (sorted text, keys), and one scalar word compare settles them without the
    // vector unit. Inside each class's branch, so that it costs no size test of its own.
    if n > 32 {
        // UNSAFE(hot-kernel) SAFETY: the compare kernel contract, 8 <= n.
        #[allow(unsafe_code)]
        if let Some(c) = unsafe { head_order(a, b) } {
            return c;
        }
        // UNSAFE(hot-kernel) SAFETY: the compare kernel contract, 32 < n <= CMP_MID.
        #[allow(unsafe_code)]
        unsafe { cmp_upto64(a, b, n) }
    } else if n >= 16 {
        // UNSAFE(hot-kernel) SAFETY: the compare kernel contract, 8 <= n.
        #[allow(unsafe_code)]
        if let Some(c) = unsafe { head_order(a, b) } {
            return c;
        }
        // UNSAFE(hot-kernel) SAFETY: the compare kernel contract, 16 <= n <= 32, bytes 0-7 equal.
        #[allow(unsafe_code)]
        unsafe { cmp_blocks::<1>(a, b, n) }
    } else if n >= 8 {
        // UNSAFE(hot-kernel) SAFETY: the compare kernel contract, 8 <= n.
        #[allow(unsafe_code)]
        if let Some(c) = unsafe { head_order(a, b) } {
            return c;
        }
        // Equal heads: the last word decides (the bytes it shares with the head are equal).
        // UNSAFE(hot-kernel) SAFETY: offset n - 8 with 8 <= n: inside the region (this and the next).
        #[allow(unsafe_code)]
        let x = unsafe { head8(a.wrapping_add(n.wrapping_sub(8))) };
        // UNSAFE(hot-kernel) SAFETY: as above.
        #[allow(unsafe_code)]
        let y = unsafe { head8(b.wrapping_add(n.wrapping_sub(8))) };
        sign(u64::from_be(x), u64::from_be(y))
    } else if n >= 4 {
        // UNSAFE(hot-kernel) SAFETY: the compare kernel contract, 4 <= n < 8 (this and the next).
        #[allow(unsafe_code)]
        let (x0, x1) = unsafe { ends::<u32>(a, n) };
        // UNSAFE(hot-kernel) SAFETY: as above.
        #[allow(unsafe_code)]
        let (y0, y1) = unsafe { ends::<u32>(b, n) };
        let (x, y) = if x0 == y0 { (x1, y1) } else { (x0, y0) };
        sign(u32::from_be(x), u32::from_be(y))
    } else {
        // Bytes 0, n/2 and n-1: (a0,a0,a0), (a0,a1,a1), (a0,a1,a2) for n = 1, 2, 3 order exactly
        // as the bytes do.
        let (h, l) = (n >> 1, n.wrapping_sub(1));
        // UNSAFE(hot-kernel) SAFETY: 0, n/2 and n-1 are below n (1 <= n <= 3): inside the region.
        #[allow(unsafe_code)]
        let x = unsafe { bytes3(a, h, l) };
        // UNSAFE(hot-kernel) SAFETY: as `x`, for the region at `b`.
        #[allow(unsafe_code)]
        let y = unsafe { bytes3(b, h, l) };
        sign(x, y)
    }
}

/// The checked compare for sizes outside the kernel's range (and 0).
///
/// # Safety
/// `a` and `b` are readable for `n` bytes (the `memcmp` contract).
// UNSAFE(hot-kernel): forwards the caller's contract to the checked path.
#[allow(unsafe_code)]
#[inline(never)]
unsafe fn compare_large(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    // UNSAFE(hot-kernel) SAFETY: the function's contract, unchanged.
    #[allow(unsafe_code)]
    let x = unsafe { conv::buf(a, n) };
    // UNSAFE(hot-kernel) SAFETY: the function's contract, unchanged.
    #[allow(unsafe_code)]
    let y = unsafe { conv::buf(b, n) };
    ret::order(x.and_then(|x| y.map(|y| conv::compare(x, y))))
}

/// The 65 to [`CMP_MAX`] byte range: the explicit NULL tests (S6 abort, option B on D-4.22), then
/// the class. Out of line, as [`compare_mid`]; its own entry so that the class pays no second range
/// test (MEM-ENTRY-2).
///
/// # Safety
/// `a` and `b` are readable for `n` bytes, `CMP_MID < n <= CMP_MAX`.
// UNSAFE(hot-kernel): the validated entry to the compare kernel's 65 to 128 byte class.
#[allow(unsafe_code)]
#[inline(never)]
unsafe fn compare_upto128(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    if a.is_null() || b.is_null() {
        null_region();
    }
    let (a, b) = (a.cast::<u8>(), b.cast::<u8>());
    // UNSAFE(hot-kernel) SAFETY: 8 <= n, both non-NULL, the caller's contract.
    #[allow(unsafe_code)]
    if let Some(c) = unsafe { head_order(a, b) } {
        return c;
    }
    // UNSAFE(hot-kernel) SAFETY: CMP_MID < n <= CMP_MAX, both non-NULL, the caller's contract.
    #[allow(unsafe_code)]
    unsafe { cmp_upto128(a, b, n) }
}

/// The long class's range, [`CMP_MAX`] to [`CMP_LONG`] bytes: the explicit NULL tests (S6 abort; only
/// the 1-64 copy and fill classes fold a NULL, owner ruling option B on D-4.22, the compare never
/// does), then the long class. Out of line: it carries a frame the entry must not pay.
///
/// # Safety
/// `a` and `b` are readable for `n` bytes, `CMP_MAX < n <= CMP_LONG`.
// UNSAFE(hot-kernel): the validated entry to the compare kernel's long class.
#[allow(unsafe_code)]
#[inline(never)]
unsafe fn compare_mid(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    if a.is_null() || b.is_null() {
        null_region();
    }
    let (a, b) = (a.cast::<u8>(), b.cast::<u8>());
    // UNSAFE(hot-kernel) SAFETY: 8 <= n, both non-NULL, the caller's contract.
    #[allow(unsafe_code)]
    if let Some(c) = unsafe { head_order(a, b) } {
        return c;
    }
    if crate::dispatch::avx2() {
        // UNSAFE(hot-kernel) SAFETY: CMP_MAX < n, both non-NULL, the caller's contract; AVX2 with
        // the YMM state saved by the OS (`dispatch`, D-4.25).
        #[allow(unsafe_code)]
        return unsafe { avx2::cmp_long(a, b, n) };
    }
    // UNSAFE(hot-kernel) SAFETY: CMP_MAX < n, both non-NULL, the caller's contract.
    #[allow(unsafe_code)]
    unsafe { cmp_long(a, b, n) }
}

/// `memcmp`, `bcmp` and `__memcmpeq`: -1, 0 or 1 for the order of `n` bytes at `a` against `n`
/// bytes at `b` as `unsigned char`. NULL with `n > 0` is the `EFAULT` abort (S6), as on the
/// checked path.
///
/// # Safety
/// `a` and `b` are readable for `n` bytes.
// UNSAFE(hot-kernel): the validated entry to the compare kernel.
#[allow(unsafe_code)]
#[inline(always)]
pub unsafe fn compare_bytes(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    // One range test per class, all on the one register `n - 1` (MEM-ENTRY-2; 0 wraps to the
    // largest value, the checked path). 1 to 64 first so that its classes pay nothing for the rest
    // (MEM-K5); the only inline NULL site is its own (a second one cost every size a frame).
    let m = n.wrapping_sub(1);
    if m < CMP_MID {
        // Two fused test-and-branch, never taken in a correct program (as the copy).
        if a.is_null() || b.is_null() {
            null_region();
        }
        // UNSAFE(hot-kernel) SAFETY: 1 <= n <= CMP_MID, both non-NULL, the caller's contract.
        #[allow(unsafe_code)]
        return unsafe { cmp_small(a.cast(), b.cast(), n) };
    }
    if m < CMP_MAX {
        // UNSAFE(hot-kernel) SAFETY: CMP_MID < n <= CMP_MAX, the caller's contract.
        #[allow(unsafe_code)]
        return unsafe { compare_upto128(a, b, n) };
    }
    if m < CMP_LONG {
        // UNSAFE(hot-kernel) SAFETY: CMP_MAX < n <= CMP_LONG, the caller's contract.
        #[allow(unsafe_code)]
        return unsafe { compare_mid(a, b, n) };
    }
    // UNSAFE(hot-kernel) SAFETY: the function's contract.
    #[allow(unsafe_code)]
    unsafe { compare_large(a, b, n) }
}

// ---------------------------------------------------------------------------------------------
// The equality kernel (MEMCMP-EQ, D-4.41) behind `__memcmpeq`: 0 when `n` bytes at `a` and `b`
// are equal, else 1. No first difference is ever located (the equality-only contract of §4.3).
//
// EQUALITY KERNEL CONTRACT: called with `1 <= n` and `a`, `b` each valid for reading `n` bytes;
// it reads only `[a, a + n)` and `[b, b + n)`, writes nothing, and decides from one load of every
// byte (under a concurrent writer, D-4.21, either answer is one the bytes held). Classes as the
// compare kernel's up to 128 bytes (overlapping loads from both ends, every load issued before the
// one test; above 64 the first block is tested alone first, the head probe of D-4.44); above 128
// the long class: the head probe, the last chunk (early reject), then the chunks from
// the start, one test per 64 bytes (SSE2) or 128 bytes (AVX2).
// ---------------------------------------------------------------------------------------------

/// The equality kernel's block classes, `16 * K <= n <= 32 * K` (K = 1, 2 or 4): the blocks at 0,
/// 16, ... `16 * (K - 1)` and the K blocks ending at `n`, compared, ANDed and turned into one byte
/// mask; 0xFFFF when all are equal.
///
/// # Safety
/// The equality kernel contract, `n` in that range.
// UNSAFE(hot-kernel): the block classes of the equality kernel.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn eq_mask<const K: usize>(a: *const u8, b: *const u8, n: usize) -> i32 {
    let t = n.wrapping_sub(K.wrapping_mul(16));
    macro_rules! eq {
        ($off:expr) => {{
            // UNSAFE(hot-kernel) SAFETY: offset + 16 <= n by the class range: inside the region.
            #[allow(unsafe_code)]
            let x = unsafe { ld16(a.wrapping_add($off)) };
            // UNSAFE(hot-kernel) SAFETY: as `x`, for the region at `b`.
            #[allow(unsafe_code)]
            let y = unsafe { ld16(b.wrapping_add($off)) };
            eq_bytes(x, y)
        }};
    }
    let e = if K == 1 {
        both(eq!(0), eq!(t))
    } else if K == 2 {
        both(both(eq!(0), eq!(16)), both(eq!(t), eq!(t.wrapping_add(16))))
    } else {
        let tail = both(both(eq!(t), eq!(t.wrapping_add(16))), both(eq!(t.wrapping_add(32)), eq!(t.wrapping_add(48))));
        both(both(both(eq!(0), eq!(16)), both(eq!(32), eq!(48))), tail)
    };
    mask(e)
}

/// The equality kernel's head probe above 64 bytes (D-4.44): the first block of each region is
/// compared and tested on its own before the tail-first scan, so a difference near the start (the
/// common way unequal keys differ) is rejected after one load per side. `false` only for the
/// planted-regression run of D-4.44 (head rows must regress); the answer never depends on it.
const EQ_HEAD_PROBE: bool = true;

/// Whether the first 16 bytes at `a` and `b` differ (the SSE2 head probe, D-4.44).
///
/// # Safety
/// Both regions are readable for 16 bytes.
// UNSAFE(hot-kernel): one block per region under the equality kernel contract.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn head_ne16(a: *const u8, b: *const u8) -> bool {
    // UNSAFE(hot-kernel) SAFETY: the function's contract: 16 bytes at `a`.
    #[allow(unsafe_code)]
    let x = unsafe { ld16(a) };
    // UNSAFE(hot-kernel) SAFETY: as `x`, for the region at `b`.
    #[allow(unsafe_code)]
    let y = unsafe { ld16(b) };
    mask(eq_bytes(x, y)) != 0xFFFF
}

/// The equality kernel's SSE2 long class, `64 <= n`: 1 if the bytes differ, else 0.
///
/// # Safety
/// The equality kernel contract, `64 <= n`.
// UNSAFE(hot-kernel): the SSE2 long class of the equality kernel.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn ne_long(a: *const u8, b: *const u8, n: usize) -> c_int {
    // UNSAFE(hot-kernel) SAFETY: the contract, 64 <= n: the first 16 bytes are inside both regions.
    #[allow(unsafe_code)]
    if EQ_HEAD_PROBE && unsafe { head_ne16(a, b) } {
        return 1;
    }
    let last = n.wrapping_sub(64);
    // UNSAFE(hot-kernel) SAFETY: the 64-byte chunk at `last` ends at n: inside both regions.
    #[allow(unsafe_code)]
    if unsafe { eq_mask::<2>(a.wrapping_add(last), b.wrapping_add(last), 64) } != 0xFFFF {
        return 1;
    }
    let mut off = 0;
    while off < last {
        // UNSAFE(hot-kernel) SAFETY: off < last = n - 64: the chunk ends before n.
        #[allow(unsafe_code)]
        if unsafe { eq_mask::<2>(a.wrapping_add(off), b.wrapping_add(off), 64) } != 0xFFFF {
            return 1;
        }
        off = off.wrapping_add(64);
    }
    0
}

/// The equality kernel up to 64 bytes: 1 if `n` bytes at `a` and `b` differ, else 0.
///
/// # Safety
/// The equality kernel contract, `n <= CMP_MID`.
// UNSAFE(hot-kernel): dispatches to the size classes under the equality kernel contract.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn ne_small(a: *const u8, b: *const u8, n: usize) -> c_int {
    if n > 32 && crate::dispatch::avx2() {
        // UNSAFE(hot-kernel) SAFETY: the contract, 32 < n <= 64; AVX2 per `dispatch` (D-4.25).
        #[allow(unsafe_code)]
        return unsafe { avx2::ne_blocks(a, b, n) };
    }
    let ne = if n > 32 {
        // UNSAFE(hot-kernel) SAFETY: the contract, 32 < n <= 64.
        #[allow(unsafe_code)]
        let m = unsafe { eq_mask::<2>(a, b, n) };
        m != 0xFFFF
    } else if n >= 16 {
        // UNSAFE(hot-kernel) SAFETY: the contract, 16 <= n <= 32.
        #[allow(unsafe_code)]
        let m = unsafe { eq_mask::<1>(a, b, n) };
        m != 0xFFFF
    } else if n >= 8 {
        // UNSAFE(hot-kernel) SAFETY: the contract, 8 <= n < 16 (this and the next).
        #[allow(unsafe_code)]
        let (x0, x1) = unsafe { ends::<u64>(a, n) };
        // UNSAFE(hot-kernel) SAFETY: as above.
        #[allow(unsafe_code)]
        let (y0, y1) = unsafe { ends::<u64>(b, n) };
        ((x0 ^ y0) | (x1 ^ y1)) != 0
    } else if n >= 4 {
        // UNSAFE(hot-kernel) SAFETY: the contract, 4 <= n < 8 (this and the next).
        #[allow(unsafe_code)]
        let (x0, x1) = unsafe { ends::<u32>(a, n) };
        // UNSAFE(hot-kernel) SAFETY: as above.
        #[allow(unsafe_code)]
        let (y0, y1) = unsafe { ends::<u32>(b, n) };
        ((x0 ^ y0) | (x1 ^ y1)) != 0
    } else {
        let (h, l) = (n >> 1, n.wrapping_sub(1));
        // UNSAFE(hot-kernel) SAFETY: 0, n/2 and n-1 are below n (1 <= n <= 3).
        #[allow(unsafe_code)]
        let x = unsafe { bytes3(a, h, l) };
        // UNSAFE(hot-kernel) SAFETY: as `x`, for the region at `b`.
        #[allow(unsafe_code)]
        let y = unsafe { bytes3(b, h, l) };
        x != y
    };
    c_int::from(ne)
}

/// The equality kernel above 64 bytes, and `n == 0`: out of line, with the explicit NULL tests
/// (S6 abort; the compare never folds a NULL, owner ruling option B on D-4.22).
///
/// # Safety
/// `a` and `b` are readable for `n` bytes, `n == 0 || CMP_MID < n`.
// UNSAFE(hot-kernel): the validated entry to the equality kernel's larger classes.
#[allow(unsafe_code)]
#[inline(never)]
unsafe fn equal_rest(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    if n == 0 {
        return 0;
    }
    if a.is_null() || b.is_null() {
        null_region();
    }
    let (a, b) = (a.cast::<u8>(), b.cast::<u8>());
    let avx2 = crate::dispatch::avx2();
    if n <= CMP_MAX {
        if avx2 {
            // UNSAFE(hot-kernel) SAFETY: the contract, 64 < n <= 128; AVX2 per `dispatch`.
            #[allow(unsafe_code)]
            return unsafe { avx2::ne_blocks4_head(a, b, n) };
        }
        // UNSAFE(hot-kernel) SAFETY: the contract, 64 < n: the first 16 bytes are inside both regions.
        #[allow(unsafe_code)]
        if EQ_HEAD_PROBE && unsafe { head_ne16(a, b) } {
            return 1;
        }
        // UNSAFE(hot-kernel) SAFETY: the contract, 64 < n <= 128.
        #[allow(unsafe_code)]
        let m = unsafe { eq_mask::<4>(a, b, n) };
        return c_int::from(m != 0xFFFF);
    }
    if avx2 {
        // UNSAFE(hot-kernel) SAFETY: the contract, 128 < n; AVX2 per `dispatch`.
        #[allow(unsafe_code)]
        return unsafe { avx2::ne_long(a, b, n) };
    }
    // UNSAFE(hot-kernel) SAFETY: the contract, 128 < n.
    #[allow(unsafe_code)]
    unsafe { ne_long(a, b, n) }
}

/// `__memcmpeq`: 0 when `n` bytes at `a` and `b` are equal, else 1 (§4.3: equality only; glibc's
/// own non-zero values carry no order either, probe 8bcfded6924a). NULL with `n > 0` is the
/// `EFAULT` abort (S6).
///
/// # Safety
/// `a` and `b` are readable for `n` bytes.
// UNSAFE(hot-kernel): the validated entry to the equality kernel.
#[allow(unsafe_code)]
#[inline(always)]
pub unsafe fn equal_bytes(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    if n.wrapping_sub(1) < CMP_MID {
        if a.is_null() || b.is_null() {
            null_region();
        }
        // UNSAFE(hot-kernel) SAFETY: 1 <= n <= CMP_MID, both non-NULL, the caller's contract.
        #[allow(unsafe_code)]
        return unsafe { ne_small(a.cast(), b.cast(), n) };
    }
    // UNSAFE(hot-kernel) SAFETY: the function's contract.
    #[allow(unsafe_code)]
    unsafe { equal_rest(a, b, n) }
}

// ---------------------------------------------------------------------------------------------
// The direct compare entries (MEMCMP-FOLD; owner ruling 2026-10-10 extending the S6 NULL-folding
// exception to memcmp, bcmp and __memcmpeq for 8 <= n <= 64; D-4.23 and D-4.41 amendments). In the
// products `memcmp` and `bcmp`/`__memcmpeq` ARE the assembly entries (machine/fold.S): with folding
// on, 8 to 64 bytes run the head probe there (a NULL faults on page zero), and only equal heads come
// back to Rust, to the `*_tail` functions below, with both pointers already read (so non-NULL).
// Every other case comes back to the `*_rest` functions: the checked entries with the explicit S6
// NULL tests. No Rust code here performs a deliberate NULL access.
// ---------------------------------------------------------------------------------------------

/// `memcmp` through its direct entry (the products' `memcmp`; the Rust twin calls it so that the
/// tests run the assembly).
///
/// # Safety
/// `a` and `b` are readable for `n` bytes (the `memcmp` contract).
// UNSAFE(hot-kernel): the validated entry to the direct compare entry.
#[allow(unsafe_code)]
#[inline(always)]
pub unsafe fn compare_direct(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    // Proof obligation of the declaration (`ffi`): the caller's contract.
    crate::ffi::liella_machine_compare(a.cast(), b.cast(), n)
}

/// `bcmp`/`__memcmpeq` through their direct entry, as [`compare_direct`].
///
/// # Safety
/// `a` and `b` are readable for `n` bytes.
// UNSAFE(hot-kernel): the validated entry to the direct equality entry.
#[allow(unsafe_code)]
#[inline(always)]
pub unsafe fn equal_direct(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    // Proof obligation of the declaration (`ffi`): the caller's contract.
    crate::ffi::liella_machine_equal(a.cast(), b.cast(), n)
}

/// The compare entry's other cases (0-7 bytes, above 64, folding off): the checked entry with the
/// explicit S6 NULL tests. A small call also takes the folding decision (STARTUP-1), as the copy's.
///
/// # Safety
/// The `memcmp` contract for `n` bytes.
// UNSAFE(hot-kernel): forwards the caller's contract to the checked entry.
#[allow(unsafe_code)]
#[inline(always)]
pub(crate) unsafe fn compare_rest(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    crate::dispatch::resolve_fold(n);
    // UNSAFE(hot-kernel) SAFETY: the function's contract, unchanged.
    #[allow(unsafe_code)]
    unsafe { compare_bytes(a, b, n) }
}

/// The compare entry's sizes above [`CMP_MID`] (MEMCMP-SHORT): [`compare_bytes`] without its 1-64
/// range test and without the folding decision, which only 1-64 bytes take ([`compare_rest`]).
/// The classes keep their explicit S6 NULL tests.
///
/// # Safety
/// The `memcmp` contract for `n` bytes, `CMP_MID < n`.
// UNSAFE(hot-kernel): forwards the caller's contract to the checked classes above 64 bytes.
#[allow(unsafe_code)]
#[inline(always)]
pub(crate) unsafe fn compare_big(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    debug_assert!(n > CMP_MID);
    let m = n.wrapping_sub(1);
    if m < CMP_MAX {
        // UNSAFE(hot-kernel) SAFETY: CMP_MID < n <= CMP_MAX, the caller's contract.
        #[allow(unsafe_code)]
        return unsafe { compare_upto128(a, b, n) };
    }
    if m < CMP_LONG {
        // UNSAFE(hot-kernel) SAFETY: CMP_MAX < n <= CMP_LONG, the caller's contract.
        #[allow(unsafe_code)]
        return unsafe { compare_mid(a, b, n) };
    }
    // UNSAFE(hot-kernel) SAFETY: the function's contract.
    #[allow(unsafe_code)]
    unsafe { compare_large(a, b, n) }
}

/// The equality entry's sizes above [`CMP_MID`], as [`compare_big`].
///
/// # Safety
/// `a` and `b` are readable for `n` bytes, `CMP_MID < n`.
// UNSAFE(hot-kernel): forwards the caller's contract to the checked classes above 64 bytes.
#[allow(unsafe_code)]
#[inline(always)]
pub(crate) unsafe fn equal_big(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    debug_assert!(n > CMP_MID);
    // UNSAFE(hot-kernel) SAFETY: the function's contract.
    #[allow(unsafe_code)]
    unsafe { equal_rest(a, b, n) }
}

/// The equality entry's other cases, as [`compare_rest`].
///
/// # Safety
/// `a` and `b` are readable for `n` bytes.
// UNSAFE(hot-kernel): forwards the caller's contract to the checked entry.
#[allow(unsafe_code)]
#[inline(always)]
pub(crate) unsafe fn equal_rest_entry(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    crate::dispatch::resolve_fold(n);
    // UNSAFE(hot-kernel) SAFETY: the function's contract, unchanged.
    #[allow(unsafe_code)]
    unsafe { equal_bytes(a, b, n) }
}

/// The compare entry's classes after an equal head probe: the order of `n` bytes at `a` and `b`.
/// Reached only from the assembly entry, which has just read 8 bytes at each (so neither is NULL;
/// the debug assertions keep that link in the chain, RESULT CHECKS).
///
/// # Safety
/// The compare kernel contract, `8 <= n <= CMP_MID`, `a` and `b` non-NULL, bytes 0-7 equal.
// UNSAFE(hot-kernel): the classes of the compare kernel after the direct entry's head probe.
#[allow(unsafe_code)]
#[inline(always)]
pub(crate) unsafe fn compare_tail(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    debug_assert!(!a.is_null() && !b.is_null() && (8..=CMP_MID).contains(&n));
    let (a, b) = (a.cast::<u8>(), b.cast::<u8>());
    if n > 32 {
        // UNSAFE(hot-kernel) SAFETY: the contract, 32 < n <= CMP_MID, bytes 0-7 equal.
        #[allow(unsafe_code)]
        return unsafe { cmp_upto64(a, b, n) };
    }
    if n >= 16 {
        // UNSAFE(hot-kernel) SAFETY: the contract, 16 <= n <= 32, bytes 0-7 equal.
        #[allow(unsafe_code)]
        return unsafe { cmp_blocks::<1>(a, b, n) };
    }
    // Equal heads, 8 <= n < 16: the last word decides (the bytes it shares with the head are equal).
    // UNSAFE(hot-kernel) SAFETY: offset n - 8 with 8 <= n: inside the region (this and the next).
    #[allow(unsafe_code)]
    let x = unsafe { head8(a.wrapping_add(n.wrapping_sub(8))) };
    // UNSAFE(hot-kernel) SAFETY: as above.
    #[allow(unsafe_code)]
    let y = unsafe { head8(b.wrapping_add(n.wrapping_sub(8))) };
    sign(u64::from_be(x), u64::from_be(y))
}

/// The equality entry's classes after an equal head probe: 1 if the bytes differ, else 0. As
/// [`compare_tail`].
///
/// # Safety
/// The equality kernel contract, `8 <= n <= CMP_MID`, `a` and `b` non-NULL, bytes 0-7 equal.
// UNSAFE(hot-kernel): the classes of the equality kernel after the direct entry's head probe.
#[allow(unsafe_code)]
#[inline(always)]
pub(crate) unsafe fn equal_tail(a: *const c_void, b: *const c_void, n: usize) -> c_int {
    debug_assert!(!a.is_null() && !b.is_null() && (8..=CMP_MID).contains(&n));
    let (a, b) = (a.cast::<u8>(), b.cast::<u8>());
    if n >= 16 {
        // UNSAFE(hot-kernel) SAFETY: the contract, 16 <= n <= CMP_MID.
        #[allow(unsafe_code)]
        return unsafe { ne_small(a, b, n) };
    }
    // Equal heads, 8 <= n < 16: the last word decides.
    // UNSAFE(hot-kernel) SAFETY: offset n - 8 with 8 <= n: inside the region (this and the next).
    #[allow(unsafe_code)]
    let x = unsafe { head8(a.wrapping_add(n.wrapping_sub(8))) };
    // UNSAFE(hot-kernel) SAFETY: as above.
    #[allow(unsafe_code)]
    let y = unsafe { head8(b.wrapping_add(n.wrapping_sub(8))) };
    c_int::from(x != y)
}

/// The AVX2 builds of the hot copy and compare kernels' block classes (D-4.25). Copy: 65 to 128
/// bytes as two 32-byte blocks from each end, against four 16-byte blocks in the SSE2 build: half
/// the loads and stores. Measured on Haswell (shiki, MEM-IFUNC size sweep): faster at 65 and 96
/// bytes, even at 128; an AVX2 copy at 33 to 64 bytes and an AVX2 fill at 33 to 128 bytes were
/// slower than SSE2 at every size, so they have none.
///
/// Reached only through `hot` after [`crate::dispatch::avx2`] (the module's proof obligation: the
/// CPU has AVX2 and the OS saves the YMM state) and under the `hot` KERNEL CONTRACT with
/// `64 < n <= hot::SMALL_MAX` (copy) or `32 < n <= 128` (compare). Every source byte is loaded before the first store, as in the
/// SSE2 build, so overlapping regions move correctly. The compiler ends each function with
/// `vzeroupper` (it uses YMM registers), so the caller's SSE code pays no transition penalty.
///
/// The compare kernel (D-4.23) has two AVX2 classes: 33 to 64 bytes, two 32-byte blocks (at 0 and
/// `n - 32`) per region, and 65 to 128 bytes (MEM-K5), four (at 0, 32, `n - 64`, `n - 32`), against
/// four and eight 16-byte blocks per region; the blocks are compared, ANDed and tested once, and
/// an unequal result reads the first difference from the same vectors (MEMCMP-ORDER); the long
/// class (129 to `CMP_LONG`) tests 128-byte chunks the same way.
///
/// No value wider than 32 bytes is built; each block is its own local (D-4.13, the debug-build
/// recursion trap of MEM-K1).
// In this file because plan S2 permits the hot-kernel category in `src/hot.rs` only.
pub(crate) mod avx2 {
    // ARCH(x86_64): AVX2 intrinsics (hot-kernel); aarch64 needs a NEON or portable path (A-8)
    use core::arch::x86_64::{__m256i, _mm256_and_si256, _mm256_cmpeq_epi8, _mm256_loadu_si256, _mm256_movemask_epi8, _mm256_storeu_si256};

    use liella_abi::ctypes::c_int;

    /// The copy kernel's AVX2 block class: `n` bytes from `s` to `d`, `64 < n <= 128`; returns `d`.
    ///
    /// # Safety
    /// The `hot` KERNEL CONTRACT with `64 < n <= 128`, and the CPU has AVX2 with the YMM state saved
    /// by the OS (`dispatch` proof obligation).
    // UNSAFE(hot-kernel): the AVX2 block class of the copy kernel.
    #[allow(unsafe_code)]
    #[target_feature(enable = "avx2")]
    pub(crate) unsafe fn move_blocks(d: *mut u8, s: *const u8, n: usize) -> *mut u8 {
        let t = n.wrapping_sub(32);
        macro_rules! ld {
            ($off:expr) => {{
                // UNSAFE(hot-kernel) SAFETY: offset + 32 <= n (class range): inside the source region.
                #[allow(unsafe_code)]
                let v = unsafe { s.wrapping_add($off).cast::<__m256i>().read_unaligned() };
                v
            }};
        }
        macro_rules! st {
            ($off:expr, $v:expr) => {{
                // UNSAFE(hot-kernel) SAFETY: offset + 32 <= n (class range): inside the destination.
                #[allow(unsafe_code)]
                unsafe { _mm256_storeu_si256(d.wrapping_add($off).cast(), $v) };
            }};
        }
        // Blocks at 0, 32, n - 64 and n - 32: every offset + 32 <= n as 64 < n.
        let u = n.wrapping_sub(64);
        let (a0, a1, b0, b1) = (ld!(0), ld!(32), ld!(u), ld!(t));
        st!(0, a0);
        st!(32, a1);
        st!(u, b0);
        st!(t, b1);
        // `black_box`: were the result visibly `d`, the caller would keep `d` in a saved register across
        // a call (push and pop on every small copy, MEM-IFUNC measured it) instead of tail-calling here.
        core::hint::black_box(d)
    }

    /// The 32-byte equality vector of the blocks at offset `off` of `a` and `b`.
    ///
    /// # Safety
    /// `off + 32 <= n` for regions of `n` bytes at `a` and `b`; the CPU has AVX2 (`dispatch`).
    // UNSAFE(hot-kernel): one block of the AVX2 compare and equality classes.
    #[allow(unsafe_code)]
    #[target_feature(enable = "avx2")]
    #[inline]
    unsafe fn eq32(a: *const u8, b: *const u8, off: usize) -> __m256i {
        // UNSAFE(hot-kernel) SAFETY: the function's contract: inside the region at `a`.
        #[allow(unsafe_code)]
        let x = unsafe { _mm256_loadu_si256(a.wrapping_add(off).cast()) };
        // UNSAFE(hot-kernel) SAFETY: as `x`, for the region at `b`.
        #[allow(unsafe_code)]
        let y = unsafe { _mm256_loadu_si256(b.wrapping_add(off).cast()) };
        _mm256_cmpeq_epi8(x, y)
    }

    /// The differing bytes of a 32-byte equality vector as bits: bit `i` set when byte `i` differs.
    #[target_feature(enable = "avx2")]
    #[inline]
    fn ne_bits32(e: __m256i) -> u64 {
        u64::from(!u32::from_ne_bytes(_mm256_movemask_epi8(e).to_ne_bytes()))
    }

    /// The first differing byte of the 64 bytes at `head` (blocks `e0`, `e1`) and, when those are
    /// equal, of the 64 at offset `u` (blocks `f0`, `f1`): `u` plus its lowest differing bit.
    #[target_feature(enable = "avx2")]
    #[inline]
    fn first_of4(e0: __m256i, e1: __m256i, f0: __m256i, f1: __m256i, u: usize) -> usize {
        let head = ne_bits32(e0) | (ne_bits32(e1) << 32);
        if head != 0 {
            return super::low_bit(head);
        }
        u.wrapping_add(super::low_bit(ne_bits32(f0) | (ne_bits32(f1) << 32)))
    }

    /// The compare kernel's AVX2 block classes, `32 < n <= 128`: the blocks at 0 and `n - 32`
    /// (33 to 64 bytes) or at 0, 32, `n - 64` and `n - 32` (65 to 128, MEM-K5) ANDed and tested
    /// once (equal inputs pay nothing more); unequal: the same vectors as bitmaps of differing
    /// bytes, the first set bit read with one `tzcnt`, and that byte pair ordered (MEMCMP-ORDER:
    /// no second pass over the bytes).
    ///
    /// # Safety
    /// The `hot` compare kernel contract (`a` and `b` valid for reading `n` bytes) with
    /// `32 < n <= 64` (`FOUR` false) or `64 < n <= 128` (`FOUR` true), and the CPU has AVX2 with
    /// the YMM state saved by the OS (`dispatch`).
    // UNSAFE(hot-kernel): the AVX2 block classes of the compare kernel.
    #[allow(unsafe_code)]
    #[target_feature(enable = "avx2")]
    #[inline]
    unsafe fn cmp_head_first<const FOUR: bool>(a: *const u8, b: *const u8, n: usize) -> c_int {
        let t = n.wrapping_sub(32);
        // UNSAFE(hot-kernel) SAFETY: 32 < n: the block at 0 is inside both regions.
        #[allow(unsafe_code)]
        let e0 = unsafe { eq32(a, b, 0) };
        // UNSAFE(hot-kernel) SAFETY: offset n - 32: offset + 32 = n, inside both regions.
        #[allow(unsafe_code)]
        let f1 = unsafe { eq32(a, b, t) };
        if !FOUR {
            if _mm256_movemask_epi8(_mm256_and_si256(e0, f1)) == -1 {
                return 0;
            }
            // The head block's lowest differing bit, else the tail's at its offset t (bytes the
            // blocks share are equal in the tail when the head is; MEMCMP-ORDER-3: a branch, not a
            // variable shift).
            let h = ne_bits32(e0);
            let i = if h != 0 { super::low_bit(h) } else { t.wrapping_add(super::low_bit(ne_bits32(f1))) };
            // UNSAFE(hot-kernel) SAFETY: the kernel contract, i < n.
            #[allow(unsafe_code)]
            return unsafe { super::order_at(a, b, n, i) };
        }
        let u = n.wrapping_sub(64);
        // UNSAFE(hot-kernel) SAFETY: with FOUR 64 < n: offset 32 + 32 <= n.
        #[allow(unsafe_code)]
        let e1 = unsafe { eq32(a, b, 32) };
        // UNSAFE(hot-kernel) SAFETY: offset n - 64 with 64 < n: offset + 32 <= n.
        #[allow(unsafe_code)]
        let f0 = unsafe { eq32(a, b, u) };
        if _mm256_movemask_epi8(_mm256_and_si256(_mm256_and_si256(e0, e1), _mm256_and_si256(f0, f1))) == -1 {
            return 0;
        }
        let i = first_of4(e0, e1, f0, f1, u);
        // UNSAFE(hot-kernel) SAFETY: the kernel contract, i < n.
        #[allow(unsafe_code)]
        unsafe { super::order_at(a, b, n, i) }
    }

    /// The compare kernel's AVX2 long class, `128 < n <= CMP_LONG` (MEMCMP-ORDER): 128-byte chunks
    /// from the start (the last one ending at `n`), each tested once; the first unequal chunk's
    /// first differing byte decides.
    ///
    /// # Safety
    /// The `hot` compare kernel contract with `128 <= n`; AVX2 (`dispatch`).
    // UNSAFE(hot-kernel): the AVX2 long class of the compare kernel.
    #[allow(unsafe_code)]
    #[target_feature(enable = "avx2")]
    pub(crate) unsafe fn cmp_long(a: *const u8, b: *const u8, n: usize) -> c_int {
        // Head-first probe (MEMCMP-ORDER-3), once before the loop.
        // UNSAFE(hot-kernel) SAFETY: 128 <= n: the block at 0 is inside both regions.
        #[allow(unsafe_code)]
        let h = ne_bits32(unsafe { eq32(a, b, 0) });
        if h != 0 {
            // UNSAFE(hot-kernel) SAFETY: the kernel contract, the index is below 32 < n.
            #[allow(unsafe_code)]
            return unsafe { super::order_at(a, b, n, super::low_bit(h)) };
        }
        let last = n.wrapping_sub(128);
        let mut off = 0;
        loop {
            let at = off.min(last);
            // UNSAFE(hot-kernel) SAFETY: at + 128 <= n: the four blocks at at, at + 32, at + 64 and
            // at + 96 are inside both regions (this and the next three).
            #[allow(unsafe_code)]
            let e0 = unsafe { eq32(a, b, at) };
            // UNSAFE(hot-kernel) SAFETY: as above.
            #[allow(unsafe_code)]
            let e1 = unsafe { eq32(a, b, at.wrapping_add(32)) };
            // UNSAFE(hot-kernel) SAFETY: as above.
            #[allow(unsafe_code)]
            let f0 = unsafe { eq32(a, b, at.wrapping_add(64)) };
            // UNSAFE(hot-kernel) SAFETY: as above.
            #[allow(unsafe_code)]
            let f1 = unsafe { eq32(a, b, at.wrapping_add(96)) };
            if _mm256_movemask_epi8(_mm256_and_si256(_mm256_and_si256(e0, e1), _mm256_and_si256(f0, f1))) != -1 {
                let i = at.wrapping_add(first_of4(e0, e1, f0, f1, 64));
                // UNSAFE(hot-kernel) SAFETY: the kernel contract, i < at + 128 <= n.
                #[allow(unsafe_code)]
                return unsafe { super::order_at(a, b, n, i) };
            }
            if at == last {
                return 0;
            }
            off = off.wrapping_add(128);
        }
    }

    /// The compare kernel's AVX2 block class: the order of `32 < n <= 64` bytes at `a` and `b`.
    ///
    /// # Safety
    /// As `cmp_head_first` with `32 < n <= 64`.
    // UNSAFE(hot-kernel): the AVX2 block class of the compare kernel.
    #[allow(unsafe_code)]
    #[target_feature(enable = "avx2")]
    pub(crate) unsafe fn cmp_blocks(a: *const u8, b: *const u8, n: usize) -> c_int {
        // UNSAFE(hot-kernel) SAFETY: the function's contract.
        #[allow(unsafe_code)]
        unsafe { cmp_head_first::<false>(a, b, n) }
    }

    /// The compare kernel's AVX2 65 to 128 byte class: the order of `64 < n <= 128` bytes.
    ///
    /// # Safety
    /// As `cmp_head_first` with `64 < n <= 128`.
    // UNSAFE(hot-kernel): the AVX2 65 to 128 byte class of the compare kernel.
    #[allow(unsafe_code)]
    #[target_feature(enable = "avx2")]
    pub(crate) unsafe fn cmp_blocks4(a: *const u8, b: *const u8, n: usize) -> c_int {
        // UNSAFE(hot-kernel) SAFETY: the function's contract.
        #[allow(unsafe_code)]
        unsafe { cmp_head_first::<true>(a, b, n) }
    }

    /// The equality kernel's AVX2 class for `32 <= n <= 64`: 1 if the bytes differ, else 0.
    ///
    /// # Safety
    /// The `hot` equality kernel contract with `32 <= n <= 64`; AVX2 (`dispatch`).
    // UNSAFE(hot-kernel): the AVX2 33 to 64 byte class of the equality kernel.
    #[allow(unsafe_code)]
    #[target_feature(enable = "avx2")]
    pub(crate) unsafe fn ne_blocks(a: *const u8, b: *const u8, n: usize) -> c_int {
        // UNSAFE(hot-kernel) SAFETY: offset 0, 32 <= n: inside both regions.
        #[allow(unsafe_code)]
        let e0 = unsafe { eq32(a, b, 0) };
        // UNSAFE(hot-kernel) SAFETY: offset n - 32, 32 <= n: inside both regions.
        #[allow(unsafe_code)]
        let e1 = unsafe { eq32(a, b, n.wrapping_sub(32)) };
        let e = _mm256_and_si256(e0, e1);
        c_int::from(_mm256_movemask_epi8(e) != -1)
    }

    /// The equality kernel's AVX2 class for `64 <= n <= 128`: 1 if the bytes differ, else 0.
    ///
    /// # Safety
    /// The `hot` equality kernel contract with `64 <= n <= 128`; AVX2 (`dispatch`).
    // UNSAFE(hot-kernel): the AVX2 65 to 128 byte class of the equality kernel.
    #[allow(unsafe_code)]
    #[target_feature(enable = "avx2")]
    #[inline]
    pub(crate) unsafe fn ne_blocks4(a: *const u8, b: *const u8, n: usize) -> c_int {
        let (u, t) = (n.wrapping_sub(64), n.wrapping_sub(32));
        // UNSAFE(hot-kernel) SAFETY: offsets 0, 32, n - 64 and n - 32 with 64 <= n: each offset
        // + 32 <= n, inside both regions (this and the next three).
        #[allow(unsafe_code)]
        let e0 = unsafe { eq32(a, b, 0) };
        // UNSAFE(hot-kernel) SAFETY: as above.
        #[allow(unsafe_code)]
        let e1 = unsafe { eq32(a, b, 32) };
        // UNSAFE(hot-kernel) SAFETY: as above.
        #[allow(unsafe_code)]
        let e2 = unsafe { eq32(a, b, u) };
        // UNSAFE(hot-kernel) SAFETY: as above.
        #[allow(unsafe_code)]
        let e3 = unsafe { eq32(a, b, t) };
        let e = _mm256_and_si256(_mm256_and_si256(e0, e1), _mm256_and_si256(e2, e3));
        c_int::from(_mm256_movemask_epi8(e) != -1)
    }

    /// [`ne_blocks4`] with the head probe (D-4.44): the first block is tested on its own before the
    /// other three are loaded, so a difference in the first 32 bytes is rejected after one load per
    /// side. The 65 to 128 byte entry of `equal_rest`; the long class's chunks keep one test each.
    ///
    /// # Safety
    /// The `hot` equality kernel contract with `64 <= n <= 128`; AVX2 (`dispatch`).
    // UNSAFE(hot-kernel): the AVX2 65 to 128 byte class of the equality kernel, head first.
    #[allow(unsafe_code)]
    #[target_feature(enable = "avx2")]
    pub(crate) unsafe fn ne_blocks4_head(a: *const u8, b: *const u8, n: usize) -> c_int {
        let (u, t) = (n.wrapping_sub(64), n.wrapping_sub(32));
        // UNSAFE(hot-kernel) SAFETY: offsets 0, 32, n - 64 and n - 32 with 64 <= n: each offset
        // + 32 <= n, inside both regions (this and the next three).
        #[allow(unsafe_code)]
        let e0 = unsafe { eq32(a, b, 0) };
        if super::EQ_HEAD_PROBE && _mm256_movemask_epi8(e0) != -1 {
            return 1;
        }
        // UNSAFE(hot-kernel) SAFETY: as above.
        #[allow(unsafe_code)]
        let e1 = unsafe { eq32(a, b, 32) };
        // UNSAFE(hot-kernel) SAFETY: as above.
        #[allow(unsafe_code)]
        let e2 = unsafe { eq32(a, b, u) };
        // UNSAFE(hot-kernel) SAFETY: as above.
        #[allow(unsafe_code)]
        let e3 = unsafe { eq32(a, b, t) };
        let e = _mm256_and_si256(_mm256_and_si256(e0, e1), _mm256_and_si256(e2, e3));
        c_int::from(_mm256_movemask_epi8(e) != -1)
    }

    /// The equality kernel's AVX2 long class, `128 <= n`: the last 128 bytes first (an early
    /// reject where only the tail differs), then 128-byte chunks from the start, four blocks
    /// ANDed and one test per chunk; no first difference is located. 1 if the bytes differ.
    ///
    /// # Safety
    /// The `hot` equality kernel contract with `128 <= n`; AVX2 (`dispatch`).
    // UNSAFE(hot-kernel): the AVX2 long class of the equality kernel.
    #[allow(unsafe_code)]
    #[target_feature(enable = "avx2")]
    pub(crate) unsafe fn ne_long(a: *const u8, b: *const u8, n: usize) -> c_int {
        // UNSAFE(hot-kernel) SAFETY: offset 0, 128 <= n: inside both regions (the head probe, D-4.44).
        #[allow(unsafe_code)]
        if super::EQ_HEAD_PROBE && _mm256_movemask_epi8(unsafe { eq32(a, b, 0) }) != -1 {
            return 1;
        }
        let last = n.wrapping_sub(128);
        // UNSAFE(hot-kernel) SAFETY: the chunk at `last` ends at n: inside both regions.
        #[allow(unsafe_code)]
        if unsafe { ne_blocks4(a.wrapping_add(last), b.wrapping_add(last), 128) } != 0 {
            return 1;
        }
        let mut off = 0;
        while off < last {
            // UNSAFE(hot-kernel) SAFETY: off < last = n - 128: the chunk ends before n.
            #[allow(unsafe_code)]
            if unsafe { ne_blocks4(a.wrapping_add(off), b.wrapping_add(off), 128) } != 0 {
                return 1;
            }
            off = off.wrapping_add(128);
        }
        0
    }
}

/// The `getdelim` front path's scan and copy in one pass (GETDELIM-FUSE, D-12.19): for the first
/// `c` in `s` at `i < min(s.len(), d.len() - 1)`, `s[..=i]` and a NUL to `d[..=i + 1]`, returning
/// `i + 1`; nothing past the NUL is written. `None` when there is none (a prefix of the record
/// may then be in `d`). The caller has checked for at least 32 bytes to scan and AVX2; checked
/// here again (`stdio_compat::find_copy` otherwise).
#[inline(always)]
pub(crate) fn delim_copy(s: &[u8], d: &mut [u8], c: u8) -> Option<usize> {
    let lim = d.len().checked_sub(1)?.min(s.len());
    if lim < 32 || !crate::dispatch::avx2() {
        return liella_legacy::stdio_compat::find_copy(&crate::stdio::Glue, s, d, c);
    }
    // UNSAFE(hot-kernel): the fused scan and copy (KERNEL CONTRACT of `fuse::delim_copy`).
    #[allow(unsafe_code)]
    // SAFETY: `s` is readable for `lim <= s.len()` bytes and `d` writable for `lim + 1 <=
    // d.len()` (slices, distinct: `d` is a `&mut`); `lim >= 32`; AVX2 with the YMM state saved.
    let n = unsafe { fuse::delim_copy(d.as_mut_ptr(), s.as_ptr(), c, lim) };
    if n == 0 { None } else { Some(n) }
}

/// [`delim_copy`] without AVX2 (PERF-GETLINE-SHORT, D-12.19): the first [`fuse::SSE2_HEAD`]
/// bytes in one SSE2 pass, the rest of a longer record in two (the delimiter search, then the
/// copy: fewer instructions per byte than the 16-byte fused loop on long records). The caller
/// has checked for at least 16 bytes to scan; checked here again.
#[inline(always)]
pub(crate) fn delim_copy_sse2(s: &[u8], d: &mut [u8], c: u8) -> Option<usize> {
    let lim = d.len().checked_sub(1)?.min(s.len());
    if lim < 16 {
        return liella_legacy::stdio_compat::find_copy(&crate::stdio::Glue, s, d, c);
    }
    let head = lim.min(fuse::SSE2_HEAD);
    // UNSAFE(hot-kernel): the fused scan and copy (KERNEL CONTRACT of `fuse::delim_copy_sse2`).
    #[allow(unsafe_code)]
    // SAFETY: `s` is readable for `head <= lim <= s.len()` bytes and `d` writable for `head + 1
    // <= d.len()` (slices, distinct: `d` is a `&mut`); `head >= 16`.
    let n = unsafe { fuse::delim_copy_sse2(d.as_mut_ptr(), s.as_ptr(), c, head) };
    if n != 0 {
        return Some(n);
    }
    if head == lim {
        return None;
    }
    // head == SSE2_HEAD, a multiple of 32: all of s[..head] is in d[..head].
    let n = crate::strz::find_in(s.get(head..lim)?, c)?.checked_add(head)?.checked_add(1)?;
    liella::mem::copy(d.get_mut(head..n)?, s.get(head..n)?);
    *d.get_mut(n)? = 0;
    Some(n)
}

/// The `getdelim` front path's fused scan and copy (GETDELIM-FUSE, D-12.19, HOT KERNELS ruling).
mod fuse {
    // ARCH(x86_64): AVX2 intrinsics (hot-kernel); aarch64 needs a NEON or portable path (A-8)
    use core::arch::x86_64::{__m256i, _mm256_cmpeq_epi8, _mm256_movemask_epi8, _mm256_or_si256, _mm256_set1_epi8, _mm256_storeu_si256};
    use core::arch::x86_64::{__m128i, _mm_cmpeq_epi8, _mm_movemask_epi8, _mm_or_si128, _mm_set1_epi8, _mm_storeu_si128};

    /// `s[..=i]` and a NUL to `d[..=i + 1]` for the first `c` in `s` at `i < lim`; returns
    /// `i + 1`, or 0 when there is none (a prefix of `s[..lim]` may then be in `d[..lim]`).
    /// 32-byte blocks are compared and stored in one pass; the record's last block is a store
    /// that ends at its last byte, then the NUL: nothing past the NUL is written.
    ///
    /// # Safety
    /// KERNEL CONTRACT: `s` readable for `lim` bytes, `d` writable for `lim + 1` bytes, the two
    /// regions distinct; `lim >= 32`; the CPU has AVX2 with the YMM state saved by the OS
    /// (`dispatch` proof obligation). Nothing outside `s[..lim]` is read or `d[..=lim]` written.
    // UNSAFE(hot-kernel): the fused scan and copy.
    #[allow(unsafe_code)]
    #[target_feature(enable = "avx2")]
    pub(super) unsafe fn delim_copy(d: *mut u8, s: *const u8, c: u8, lim: usize) -> usize {
        let pat = _mm256_set1_epi8(i8::from_ne_bytes([c]));
        let mut off = 0usize;
        // STRSTR-GETLINE: 128-byte steps first (four blocks, one test), about half the
        // instructions per byte of the 32-byte step on long records; a step holding `c` is left
        // to the 32-byte loop below, which compares its blocks again and finds the first `c`.
        while off.wrapping_add(128) <= lim {
            let mut v = [pat; 4];
            for (k, slot) in v.iter_mut().enumerate() {
                // UNSAFE(hot-kernel) SAFETY: off + 32 * k + 32 <= off + 128 <= lim: inside the source.
                #[allow(unsafe_code)]
                let b = unsafe { s.wrapping_add(off.wrapping_add(k.wrapping_mul(32))).cast::<__m256i>().read_unaligned() };
                *slot = b;
            }
            let [v0, v1, v2, v3] = v;
            let e = _mm256_or_si256(
                _mm256_or_si256(_mm256_cmpeq_epi8(v0, pat), _mm256_cmpeq_epi8(v1, pat)),
                _mm256_or_si256(_mm256_cmpeq_epi8(v2, pat), _mm256_cmpeq_epi8(v3, pat)),
            );
            if _mm256_movemask_epi8(e) != 0 {
                break;
            }
            for (k, b) in v.into_iter().enumerate() {
                // UNSAFE(hot-kernel) SAFETY: off + 32 * k + 32 <= off + 128 <= lim: inside the destination.
                #[allow(unsafe_code)]
                unsafe { _mm256_storeu_si256(d.wrapping_add(off.wrapping_add(k.wrapping_mul(32))).cast(), b) };
            }
            off = off.wrapping_add(128);
        }
        // Whole blocks [off, off + 32) inside [0, lim): compared, then stored when they hold no `c`.
        while off.wrapping_add(32) <= lim {
            // UNSAFE(hot-kernel) SAFETY: off + 32 <= lim: inside the source.
            #[allow(unsafe_code)]
            let v = unsafe { s.wrapping_add(off).cast::<__m256i>().read_unaligned() };
            let m = u32::from_ne_bytes(_mm256_movemask_epi8(_mm256_cmpeq_epi8(v, pat)).to_ne_bytes());
            if m != 0 {
                // UNSAFE(hot-kernel) SAFETY: the record ends at off + tz + 1 <= lim.
                #[allow(unsafe_code)]
                return unsafe { finish(d, s, off.wrapping_add(usize::try_from(m.trailing_zeros()).unwrap_or(32)).wrapping_add(1)) };
            }
            // UNSAFE(hot-kernel) SAFETY: off + 32 <= lim: inside the destination.
            #[allow(unsafe_code)]
            unsafe { _mm256_storeu_si256(d.wrapping_add(off).cast(), v) };
            off = off.wrapping_add(32);
        }
        if off < lim {
            // The rest [off, lim), shorter than 32: the block [lim - 32, lim) (lim >= 32), its
            // first off - (lim - 32) bytes (in 1..32) already compared.
            let t = lim.wrapping_sub(32);
            // UNSAFE(hot-kernel) SAFETY: [lim - 32, lim) is inside the source.
            #[allow(unsafe_code)]
            let v = unsafe { s.wrapping_add(t).cast::<__m256i>().read_unaligned() };
            let m = u32::from_ne_bytes(_mm256_movemask_epi8(_mm256_cmpeq_epi8(v, pat)).to_ne_bytes()) >> off.wrapping_sub(t);
            if m != 0 {
                // UNSAFE(hot-kernel) SAFETY: the record ends at off + tz + 1 <= lim.
                #[allow(unsafe_code)]
                return unsafe { finish(d, s, off.wrapping_add(usize::try_from(m.trailing_zeros()).unwrap_or(32)).wrapping_add(1)) };
            }
        }
        0
    }

    /// The record's bytes not yet stored (the scan stored every whole block before the one
    /// holding `c`) from `s` to `d`, ending at `n`, then `d[n] = 0`; returns `n`.
    ///
    /// # Safety
    /// `1 <= n <= lim` of [`delim_copy`]'s contract, and every byte below `n` that a store here
    /// does not cover already copied (the blocks before the record's last).
    // UNSAFE(hot-kernel): the record's last store and its NUL.
    #[allow(unsafe_code)]
    #[target_feature(enable = "avx2")]
    #[inline]
    unsafe fn finish(d: *mut u8, s: *const u8, n: usize) -> usize {
        // Overlapping pairs: one ending at n, one starting at 0 (a record shorter than 32 bytes
        // lies in the first block, which nothing has stored).
        macro_rules! pair {
            ($t:ty, $w:expr) => {{
                let e = n.wrapping_sub($w);
                // UNSAFE(hot-kernel) SAFETY: $w <= n <= lim: [0, $w) and [n - $w, n) are inside both regions.
                #[allow(unsafe_code)]
                unsafe {
                    d.cast::<$t>().write_unaligned(s.cast::<$t>().read_unaligned());
                    d.wrapping_add(e).cast::<$t>().write_unaligned(s.wrapping_add(e).cast::<$t>().read_unaligned());
                }
            }};
        }
        if n >= 32 {
            let e = n.wrapping_sub(32);
            // UNSAFE(hot-kernel) SAFETY: 32 <= n <= lim: [n - 32, n) inside both regions; the
            // blocks before it were stored by the scan.
            #[allow(unsafe_code)]
            let v = unsafe { s.wrapping_add(e).cast::<__m256i>().read_unaligned() };
            // UNSAFE(hot-kernel) SAFETY: as above, the destination.
            #[allow(unsafe_code)]
            unsafe { _mm256_storeu_si256(d.wrapping_add(e).cast(), v) };
        } else if n >= 16 {
            pair!(u128, 16);
        } else if n >= 8 {
            pair!(u64, 8);
        } else if n >= 4 {
            pair!(u32, 4);
        } else if n >= 2 {
            pair!(u16, 2);
        } else {
            // UNSAFE(hot-kernel) SAFETY: n == 1 <= lim.
            #[allow(unsafe_code)]
            let b = unsafe { s.read() };
            // UNSAFE(hot-kernel) SAFETY: n == 1 <= lim.
            #[allow(unsafe_code)]
            unsafe { d.write(b) };
        }
        // UNSAFE(hot-kernel) SAFETY: n <= lim: d[n] is inside the destination (lim + 1 bytes).
        #[allow(unsafe_code)]
        unsafe { d.wrapping_add(n).write(0) };
        n
    }

    /// The bytes [`delim_copy_sse2`] scans in one pass; a record longer than this is finished in
    /// two (`hot::delim_copy_sse2`). A multiple of 32 (the whole head is stored when it holds no `c`).
    pub(super) const SSE2_HEAD: usize = 128;

    /// [`delim_copy`] with 16-byte SSE2 blocks (PERF-GETLINE-SHORT): `s[..=i]` and a NUL to
    /// `d[..=i + 1]` for the first `c` in `s` at `i < lim`; returns `i + 1`, or 0 when there is
    /// none. On 0 with `lim` a multiple of 32, all of `s[..lim]` is in `d[..lim]` (every block was
    /// stored); otherwise a prefix may be.
    ///
    /// # Safety
    /// KERNEL CONTRACT: `s` readable for `lim` bytes, `d` writable for `lim + 1` bytes, the two
    /// regions distinct; `lim >= 16`. SSE2 is in every x86-64 CPU. Nothing outside `s[..lim]` is
    /// read or `d[..=lim]` written.
    // UNSAFE(hot-kernel): the fused scan and copy, SSE2 build.
    #[allow(unsafe_code)]
    #[target_feature(enable = "sse2")]
    #[inline]
    pub(super) unsafe fn delim_copy_sse2(d: *mut u8, s: *const u8, c: u8, lim: usize) -> usize {
        let pat = _mm_set1_epi8(i8::from_ne_bytes([c]));
        let mut off = 0usize;
        // 32-byte steps (two blocks, one test); a step holding `c` is left to the block loop.
        while off.wrapping_add(32) <= lim {
            // UNSAFE(hot-kernel) SAFETY: off + 32 <= lim: the first block is inside the source.
            #[allow(unsafe_code)]
            let v0 = unsafe { s.wrapping_add(off).cast::<__m128i>().read_unaligned() };
            // UNSAFE(hot-kernel) SAFETY: off + 32 <= lim: the second block is inside the source.
            #[allow(unsafe_code)]
            let v1 = unsafe { s.wrapping_add(off).wrapping_add(16).cast::<__m128i>().read_unaligned() };
            if _mm_movemask_epi8(_mm_or_si128(_mm_cmpeq_epi8(v0, pat), _mm_cmpeq_epi8(v1, pat))) != 0 {
                break;
            }
            // UNSAFE(hot-kernel) SAFETY: off + 32 <= lim: the first block is inside the destination.
            #[allow(unsafe_code)]
            unsafe { _mm_storeu_si128(d.wrapping_add(off).cast(), v0) };
            // UNSAFE(hot-kernel) SAFETY: off + 32 <= lim: the second block is inside the destination.
            #[allow(unsafe_code)]
            unsafe { _mm_storeu_si128(d.wrapping_add(off).wrapping_add(16).cast(), v1) };
            off = off.wrapping_add(32);
        }
        // Whole blocks [off, off + 16) inside [0, lim): compared, then stored when they hold no `c`.
        while off.wrapping_add(16) <= lim {
            // UNSAFE(hot-kernel) SAFETY: off + 16 <= lim: inside the source.
            #[allow(unsafe_code)]
            let v = unsafe { s.wrapping_add(off).cast::<__m128i>().read_unaligned() };
            let m = u32::from_ne_bytes(_mm_movemask_epi8(_mm_cmpeq_epi8(v, pat)).to_ne_bytes());
            if m != 0 {
                // UNSAFE(hot-kernel) SAFETY: the record ends at off + tz + 1 <= lim; [0, off) is stored.
                #[allow(unsafe_code)]
                return unsafe { finish16(d, s, off.wrapping_add(usize::try_from(m.trailing_zeros()).unwrap_or(16)).wrapping_add(1)) };
            }
            // UNSAFE(hot-kernel) SAFETY: off + 16 <= lim: inside the destination.
            #[allow(unsafe_code)]
            unsafe { _mm_storeu_si128(d.wrapping_add(off).cast(), v) };
            off = off.wrapping_add(16);
        }
        if off < lim {
            // The rest [off, lim), shorter than 16: the block [lim - 16, lim) (lim >= 16), its
            // first off - (lim - 16) bytes (in 1..16) already compared.
            let t = lim.wrapping_sub(16);
            // UNSAFE(hot-kernel) SAFETY: [lim - 16, lim) is inside the source.
            #[allow(unsafe_code)]
            let v = unsafe { s.wrapping_add(t).cast::<__m128i>().read_unaligned() };
            let m = u32::from_ne_bytes(_mm_movemask_epi8(_mm_cmpeq_epi8(v, pat)).to_ne_bytes()) >> off.wrapping_sub(t);
            if m != 0 {
                // UNSAFE(hot-kernel) SAFETY: the record ends at off + tz + 1 <= lim; [0, off) is stored.
                #[allow(unsafe_code)]
                return unsafe { finish16(d, s, off.wrapping_add(usize::try_from(m.trailing_zeros()).unwrap_or(16)).wrapping_add(1)) };
            }
        }
        0
    }

    /// [`finish`] for the SSE2 build: the record's last 16 bytes (or overlapping pairs below 16)
    /// from `s` to `d`, ending at `n`, then `d[n] = 0`; returns `n`.
    ///
    /// # Safety
    /// `1 <= n <= lim` of [`delim_copy_sse2`]'s contract, and every byte of `[0, n - 16)` already copied.
    // UNSAFE(hot-kernel): the record's last store and its NUL.
    #[allow(unsafe_code)]
    #[inline]
    unsafe fn finish16(d: *mut u8, s: *const u8, n: usize) -> usize {
        macro_rules! pair {
            ($t:ty, $w:expr) => {{
                let e = n.wrapping_sub($w);
                // UNSAFE(hot-kernel) SAFETY: $w <= n <= lim: [0, $w) and [n - $w, n) are inside both regions.
                #[allow(unsafe_code)]
                unsafe {
                    d.cast::<$t>().write_unaligned(s.cast::<$t>().read_unaligned());
                    d.wrapping_add(e).cast::<$t>().write_unaligned(s.wrapping_add(e).cast::<$t>().read_unaligned());
                }
            }};
        }
        if n >= 16 {
            let e = n.wrapping_sub(16);
            // UNSAFE(hot-kernel) SAFETY: 16 <= n <= lim: [n - 16, n) is inside the source.
            #[allow(unsafe_code)]
            let v = unsafe { s.wrapping_add(e).cast::<u128>().read_unaligned() };
            // UNSAFE(hot-kernel) SAFETY: 16 <= n <= lim: [n - 16, n) is inside the destination; the
            // bytes before it were stored by the scan.
            #[allow(unsafe_code)]
            unsafe { d.wrapping_add(e).cast::<u128>().write_unaligned(v) };
        } else if n >= 8 {
            pair!(u64, 8);
        } else if n >= 4 {
            pair!(u32, 4);
        } else if n >= 2 {
            pair!(u16, 2);
        } else {
            // UNSAFE(hot-kernel) SAFETY: n == 1 <= lim: s[0] is inside the source.
            #[allow(unsafe_code)]
            let b = unsafe { s.read() };
            // UNSAFE(hot-kernel) SAFETY: n == 1 <= lim: d[0] is inside the destination.
            #[allow(unsafe_code)]
            unsafe { d.write(b) };
        }
        // UNSAFE(hot-kernel) SAFETY: n <= lim: d[n] is inside the destination (lim + 1 bytes).
        #[allow(unsafe_code)]
        unsafe { d.wrapping_add(n).write(0) };
        n
    }
}
