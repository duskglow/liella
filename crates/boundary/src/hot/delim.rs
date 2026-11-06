//! The `getdelim` fused scan and copy (GETDELIM-FUSE, D-12.19), under the `hot` module's
//! hot-kernel category (plan S2).

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
