//! The memory primitives behind `memcpy`, `memmove`, `memset`, `memcmp` and `bcmp` (and their
//! `wmem*` forms), as plain loops in a `#![no_builtins]` crate (decision D-4.11).
//!
//! Rust lowers `copy_from_slice`, `fill`, `copy_within` and slice comparison to calls to the C
//! runtime symbols `memcpy`, `memset`, `memmove`, `memcmp` and `bcmp`. In the shipped library
//! those symbols are Liella's own exports, so the code *implementing* them must not lower to
//! them, or `memcpy` would call itself. `#![no_builtins]` stops LLVM from recognising these
//! loops as the C idioms; the functions are not generic, so they are compiled here, under that
//! attribute, and never instantiated in another crate. Everything else in the library may use
//! the standard slice operations freely: they resolve to these loops through the exports.

#![no_std]
#![no_builtins]
#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::disallowed_methods))] // S8: fallible allocation only

use core::cmp::Ordering;

/// Bytes moved per step of the copy loops (64: the measured best on SSE2 and AVX2 builds).
const BLOCK: usize = 64;

/// Bytes examined per step of the search and compare loops (32 measured best).
const SCAN: usize = 32;

/// Words per scan block.
const WORDS: usize = SCAN / 8;

/// The little-endian words of one block.
fn words(b: &[u8]) -> [u64; WORDS] {
    core::array::from_fn(|i| b.get(i.saturating_mul(8)..i.saturating_mul(8).saturating_add(8)).and_then(|s| <[u8; 8]>::try_from(s).ok()).map_or(0, u64::from_le_bytes))
}

const LO: u64 = 0x0101_0101_0101_0101;
const HI: u64 = 0x8080_8080_8080_8080;

/// Nonzero where a byte of `v` is zero (the classic bit trick; exact for the lowest one).
fn zero_bytes(v: u64) -> u64 {
    v.wrapping_sub(LO) & !v & HI
}

/// `memchr` over bytes: the index of the first `x`, 32 bytes per step as four words read from
/// the slice (never beyond it).
pub fn find_u8(s: &[u8], x: u8) -> Option<usize> {
    let pat = LO.wrapping_mul(u64::from(x));
    let mut base = 0usize;
    let mut blocks = s.chunks_exact(SCAN);
    for b in &mut blocks {
        if words(b).iter().fold(0, |acc, &w| acc | zero_bytes(w ^ pat)) != 0 {
            return b.iter().position(|&v| v == x).map(|i| base.wrapping_add(i));
        }
        base = base.wrapping_add(SCAN);
    }
    blocks.remainder().iter().position(|&v| v == x).map(|i| base.wrapping_add(i))
}

/// Lexicographic order of bytes: equal blocks are skipped a word at a time.
pub fn compare_u8(a: &[u8], b: &[u8]) -> Ordering {
    let n = a.len().min(b.len());
    let mut skip = 0usize;
    for (x, y) in a.chunks_exact(SCAN).zip(b.chunks_exact(SCAN)) {
        let (p, q) = (words(x), words(y));
        if p.iter().zip(&q).fold(0, |acc, (a, b)| acc | (a ^ b)) != 0 {
            break;
        }
        skip = skip.wrapping_add(SCAN);
    }
    let (a, b) = (a.get(skip.min(n)..).unwrap_or(&[]), b.get(skip.min(n)..).unwrap_or(&[]));
    for (x, y) in a.iter().zip(b) {
        if x != y {
            return if x < y { Ordering::Less } else { Ordering::Greater };
        }
    }
    a.len().cmp(&b.len())
}

macro_rules! unit_fns {
    ($t:ty, $copy:ident, $back:ident, $mv:ident, $fill:ident $(, $cmp:ident)?) => {
        /// Copy `src` into the start of `dst`; returns the units copied (the shorter length).
        /// Whole blocks of [`BLOCK`] bytes move as fixed-size arrays, which the compiler turns
        /// into vector loads and stores (never a call: their size is constant). The tail is one
        /// more block ending at the end, overlapping the last whole one; below one block, two
        /// overlapping blocks of the largest power of two that fits. No unit is copied one at a
        /// time unless the region is a single unit.
        pub fn $copy(dst: &mut [$t], src: &[$t]) -> usize {
            const UNITS: usize = BLOCK / size_of::<$t>();
            /// `K` units at `at`.
            fn put<const K: usize>(d: &mut [$t], s: &[$t], at: usize) {
                let end = at.wrapping_add(K);
                if let (Some(a), Some(b)) = (d.get_mut(at..end), s.get(at..end)) {
                    if let (Ok(a), Ok(b)) = (<&mut [$t; K]>::try_from(a), <&[$t; K]>::try_from(b)) {
                        *a = *b;
                    }
                }
            }
            let n = dst.len().min(src.len());
            let (Some(d), Some(s)) = (dst.get_mut(..n), src.get(..n)) else { return 0 };
            if n >= UNITS && n < UNITS.wrapping_mul(2) {
                // One to two blocks: two overlapping blocks, no loop (the most frequent large
                // size, 64 bytes, stays straight-line code, PJ-7).
                put::<UNITS>(d, s, 0);
                put::<UNITS>(d, s, n.wrapping_sub(UNITS));
                return n;
            }
            if n >= UNITS {
                // From two blocks on, the first block is copied alone and the whole blocks start
                // at the first block boundary of the destination after it, so no store crosses a
                // cache line (measured on Zen 1: 4 KiB to a destination 7 bytes past a line took
                // 368 ns unaligned, D-4.14, PJ-7).
                let head = if n >= UNITS.wrapping_mul(2) {
                    put::<UNITS>(d, s, 0);
                    (d.as_ptr().addr().wrapping_neg() & (BLOCK - 1)).checked_div(size_of::<$t>()).unwrap_or(0)
                } else {
                    0
                };
                let (Some(dh), Some(sh)) = (d.get_mut(head..), s.get(head..)) else { return n };
                for (a, b) in dh.chunks_exact_mut(UNITS).zip(sh.chunks_exact(UNITS)) {
                    if let (Ok(a), Ok(b)) = (<&mut [$t; UNITS]>::try_from(a), <&[$t; UNITS]>::try_from(b)) {
                        *a = *b;
                    }
                }
                if n.wrapping_sub(head) % UNITS != 0 {
                    put::<UNITS>(d, s, n.wrapping_sub(UNITS));
                }
                return n;
            }
            macro_rules! pair {
                ($k:expr) => {
                    if n >= $k {
                        put::<{ $k }>(d, s, 0);
                        put::<{ $k }>(d, s, n.wrapping_sub($k));
                        return n;
                    }
                };
            }
            pair!(32);
            pair!(16);
            pair!(8);
            pair!(4);
            pair!(2);
            pair!(1);
            n
        }

        /// As the copy, with the whole blocks taken from the end towards the start (the first
        /// block then overlaps the lowest whole one): a destination a short distance above the
        /// source modulo 4 KiB makes each forward load wait on a store whose address it shares
        /// in the low 12 bits, which this order avoids (D-4.14, PJ-7).
        pub fn $back(dst: &mut [$t], src: &[$t]) -> usize {
            const UNITS: usize = BLOCK / size_of::<$t>();
            let n = dst.len().min(src.len());
            if n < UNITS {
                return $copy(dst, src);
            }
            let (Some(d), Some(s)) = (dst.get_mut(..n), src.get(..n)) else { return 0 };
            // As in the forward copy: the last block alone, then whole blocks ending at the
            // destination's last block boundary before it.
            let tail = if n >= UNITS.wrapping_mul(2) {
                if let (Some(a), Some(b)) = (d.get_mut(n.wrapping_sub(UNITS)..), s.get(n.wrapping_sub(UNITS)..)) {
                    if let (Ok(a), Ok(b)) = (<&mut [$t; UNITS]>::try_from(a), <&[$t; UNITS]>::try_from(b)) {
                        *a = *b;
                    }
                }
                (d.as_ptr().addr().wrapping_add(n.wrapping_mul(size_of::<$t>())) & (BLOCK - 1)).checked_div(size_of::<$t>()).unwrap_or(0)
            } else {
                0
            };
            let m = n.wrapping_sub(tail);
            let (Some(d), Some(s)) = (d.get_mut(..m), s.get(..m)) else { return n };
            for (a, b) in d.rchunks_exact_mut(UNITS).zip(s.rchunks_exact(UNITS)) {
                if let (Ok(a), Ok(b)) = (<&mut [$t; UNITS]>::try_from(a), <&[$t; UNITS]>::try_from(b)) {
                    *a = *b;
                }
            }
            if m % UNITS != 0 {
                if let (Some(a), Some(b)) = (d.get_mut(..UNITS), s.get(..UNITS)) {
                    if let (Ok(a), Ok(b)) = (<&mut [$t; UNITS]>::try_from(a), <&[$t; UNITS]>::try_from(b)) {
                        *a = *b;
                    }
                }
            }
            n
        }

        /// Move `len` units from offset `src` to offset `dst` within `buf`, correct under
        /// overlap. Returns `false` (and moves nothing) if either range leaves the buffer.
        /// Blocks are read whole before they are written: forward when `dst <= src` (the
        /// tail in decreasing powers of two, at increasing addresses), backward otherwise
        /// (whole blocks from the end, then the head in increasing powers of two, at decreasing
        /// addresses), so no unit is read after it was overwritten.
        pub fn $mv(buf: &mut [$t], src: usize, dst: usize, len: usize) -> bool {
            const UNITS: usize = BLOCK / size_of::<$t>();
            /// `K` units from `from` to `to`.
            fn mv<const K: usize>(buf: &mut [$t], from: usize, to: usize) {
                let tmp = buf.get(from..from.wrapping_add(K)).and_then(|x| <[$t; K]>::try_from(x).ok());
                if let (Some(tmp), Some(d)) = (tmp, buf.get_mut(to..to.wrapping_add(K))) {
                    if let Ok(d) = <&mut [$t; K]>::try_from(d) {
                        *d = tmp;
                    }
                }
            }
            let fits = |o: usize| o.checked_add(len).is_some_and(|e| e <= buf.len());
            if !(fits(src) && fits(dst)) {
                return false;
            }
            if dst == src || len == 0 {
                return true;
            }
            macro_rules! piece {
                ($k:expr, $at:expr) => {
                    mv::<{ $k }>(buf, src.wrapping_add($at), dst.wrapping_add($at))
                };
            }
            if dst < src {
                let mut i = 0usize;
                while len.wrapping_sub(i) >= UNITS {
                    piece!(UNITS, i);
                    i = i.wrapping_add(UNITS);
                }
                for k in [32usize, 16, 8, 4, 2, 1] {
                    if k < UNITS && len.wrapping_sub(i) >= k {
                        match k {
                            32 => piece!(32, i),
                            16 => piece!(16, i),
                            8 => piece!(8, i),
                            4 => piece!(4, i),
                            2 => piece!(2, i),
                            _ => piece!(1, i),
                        }
                        i = i.wrapping_add(k);
                    }
                }
            } else {
                let mut i = len;
                while i >= UNITS {
                    i = i.wrapping_sub(UNITS);
                    piece!(UNITS, i);
                }
                for k in [1usize, 2, 4, 8, 16, 32] {
                    if i & k != 0 {
                        i = i.wrapping_sub(k);
                        match k {
                            32 => piece!(32, i),
                            16 => piece!(16, i),
                            8 => piece!(8, i),
                            4 => piece!(4, i),
                            2 => piece!(2, i),
                            _ => piece!(1, i),
                        }
                    }
                }
            }
            true
        }

        /// Set every unit of `dst` to `v`, a block at a time; the tail as in the copy.
        pub fn $fill(dst: &mut [$t], v: $t) {
            const UNITS: usize = BLOCK / size_of::<$t>();
            fn set<const K: usize>(d: &mut [$t], at: usize, v: $t) {
                if let Some(a) = d.get_mut(at..at.wrapping_add(K)) {
                    if let Ok(a) = <&mut [$t; K]>::try_from(a) {
                        *a = [v; K];
                    }
                }
            }
            let n = dst.len();
            if n >= UNITS {
                let block = [v; UNITS];
                for c in dst.chunks_exact_mut(UNITS) {
                    if let Ok(c) = <&mut [$t; UNITS]>::try_from(c) {
                        *c = block;
                    }
                }
                if n % UNITS != 0 {
                    set::<UNITS>(dst, n.wrapping_sub(UNITS), v);
                }
                return;
            }
            macro_rules! pair {
                ($k:expr) => {
                    if n >= $k {
                        set::<{ $k }>(dst, 0, v);
                        set::<{ $k }>(dst, n.wrapping_sub($k), v);
                        return;
                    }
                };
            }
            pair!(32);
            pair!(16);
            pair!(8);
            pair!(4);
            pair!(2);
            pair!(1);
        }

        $(
        /// Lexicographic order (a proper prefix is smaller).
        pub fn $cmp(a: &[$t], b: &[$t]) -> Ordering {
            for (x, y) in a.iter().zip(b) {
                if x != y {
                    return if x < y { Ordering::Less } else { Ordering::Greater };
                }
            }
            a.len().cmp(&b.len())
        }
        )?
    };
}

unit_fns!(u8, copy_u8, copy_back_u8, move_u8, fill_u8);
unit_fns!(i32, copy_i32, copy_back_i32, move_i32, fill_i32, compare_i32);

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, clippy::byte_char_slices)]
mod tests {
    extern crate std;
    use super::*;
    use std::vec;
    use std::vec::Vec;

    #[test]
    fn moves_agree_with_copy_within_in_both_directions() {
        for (src, dst) in [(0, 2), (2, 0), (1, 1), (0, 5)] {
            let mut a = *b"abcdefgh";
            let mut b = a;
            assert!(move_u8(&mut a, src, dst, 3));
            b.copy_within(src..src + 3, dst);
            assert_eq!(a, b, "{src}->{dst}");
        }
        assert!(!move_u8(&mut [0u8; 4], 2, 0, 3), "out of range: refused");
    }

    /// The word-at-a-time block tests must neither cancel nor miss a match or a difference
    /// (MUTATION-CAMPAIGN-2 survivors; no spec clause: kernel internals behind memchr/memcmp).
    #[test]
    fn block_tests_see_every_match_and_difference_in_a_block() {
        // Two matches with the same in-word position in two words of one block.
        let mut s = [b'a'; 64];
        s[0] = b'b';
        s[8] = b'b';
        assert_eq!(find_u8(&s, b'b'), Some(0));
        // A NUL past the first block: the block test must not see zero padding as a match.
        let mut z = [1u8; 64];
        z[40] = 0;
        assert_eq!(find_u8(&z, 0), Some(40));
        // Two equal differences in two words of the first block.
        let a = [0u8; 64];
        let mut b = [0u8; 64];
        b[0] = 1;
        b[8] = 1;
        assert_eq!(compare_u8(&a, &b), Ordering::Less);
        // Differences with no common bits.
        assert_eq!(compare_u8(&[1u8; 64], &[2u8; 64]), Ordering::Less);
    }

    #[test]
    fn compare_is_lexicographic_and_signed_for_wide_units() {
        assert_eq!(compare_u8(b"ab", b"abc"), Ordering::Less);
        assert_eq!(compare_u8(&[0x80], &[0x7f]), Ordering::Greater);
        assert_eq!(compare_i32(&[-1], &[1]), Ordering::Less);
        let mut d = [0u8; 3];
        assert_eq!(copy_u8(&mut d, b"xyzw"), 3);
        fill_i32(&mut [0; 2], 7);
    }

    #[test]
    fn copy_move_and_fill_agree_with_the_slice_operations_at_every_length_and_overlap() {
        let src: Vec<u8> = (0..400u32).map(|i| (i.wrapping_mul(37) ^ 0x5A) as u8).collect();
        for (n, o) in (0..300usize).map(|n| (n, 3)).chain((0..64).flat_map(|o| [128, 129, 191, 200, 255].map(|n| (n, o)))) {
            // Every destination offset in a block (the loops align their stores, D-4.14).
            let mut d = vec![0xEEu8; 400];
            assert_eq!(copy_u8(&mut d[o..o + n], &src[..n]), n);
            assert_eq!(&d[o..o + n], &src[..n], "copy {n} at +{o}");
            assert!(d[..o].iter().chain(&d[o + n..]).all(|&b| b == 0xEE), "copy {n} at +{o}: outside untouched");
            let mut d = vec![0xEEu8; 400];
            assert_eq!(copy_back_u8(&mut d[o..o + n], &src[..n]), n);
            assert_eq!(&d[o..o + n], &src[..n], "copy_back {n} at +{o}");
            assert!(d[..o].iter().chain(&d[o + n..]).all(|&b| b == 0xEE), "copy_back {n} at +{o}: outside untouched");
            let mut d = vec![0xEEu8; 310];
            assert_eq!(copy_u8(&mut d[3..3 + n], &src[..n]), n);
            assert_eq!(&d[3..3 + n], &src[..n], "copy {n}");
            assert!(d[..3].iter().chain(&d[3 + n..]).all(|&b| b == 0xEE), "copy {n}: outside untouched");
            let mut d = vec![0xEEu8; 310];
            assert_eq!(copy_back_u8(&mut d[3..3 + n], &src[..n]), n);
            assert_eq!(&d[3..3 + n], &src[..n], "copy_back {n}");
            assert!(d[..3].iter().chain(&d[3 + n..]).all(|&b| b == 0xEE), "copy_back {n}: outside untouched");
            let wide: Vec<i32> = src[..n].iter().map(|&b| i32::from(b) - 100).collect();
            let mut w = vec![7i32; n + 2];
            assert_eq!(copy_back_i32(&mut w[1..1 + n], &wide), n);
            assert!(w[1..1 + n] == wide[..] && w[0] == 7 && w[1 + n] == 7, "copy_back_i32 {n}");
            let mut f = vec![0xEEu8; 310];
            fill_u8(&mut f[1..1 + n], 0x11);
            assert!(f[1..1 + n].iter().all(|&b| b == 0x11) && f[0] == 0xEE && f[1 + n..].iter().all(|&b| b == 0xEE), "fill {n}");
            for (s_off, d_off) in [(0usize, 0usize), (0, 1), (1, 0), (0, 17), (17, 0), (5, 70), (70, 5), (0, 64), (64, 0), (3, 100), (100, 3)] {
                let mut a = src.clone();
                let mut b = src.clone();
                assert!(move_u8(&mut a, s_off, d_off, n));
                b.copy_within(s_off..s_off + n, d_off);
                assert_eq!(a, b, "move {n} from {s_off} to {d_off}");
            }
        }
        for n in 0..80usize {
            let w: Vec<i32> = (0..100).map(|i| i * 3 - 7).collect();
            let mut d = vec![99i32; 100];
            assert_eq!(copy_i32(&mut d[..n], &w[..n]), n);
            assert_eq!(&d[..n], &w[..n]);
            fill_i32(&mut d[..n], -5);
            assert!(d[..n].iter().all(|&x| x == -5) && d[n..].iter().all(|&x| x == 99));
            for (s_off, d_off) in [(0usize, 1usize), (1, 0), (0, 9), (9, 0), (2, 17), (17, 2)] {
                if s_off.max(d_off) + n <= 100 {
                    let mut a = w.clone();
                    let mut b = w.clone();
                    assert!(move_i32(&mut a, s_off, d_off, n));
                    b.copy_within(s_off..s_off + n, d_off);
                    assert_eq!(a, b, "wide move {n} from {s_off} to {d_off}");
                }
            }
        }
    }

    #[test]
    fn block_paths_agree_with_the_simple_loops() {
        for n in 0..200usize {
            let src: [u8; 200] = core::array::from_fn(|i| (i as u8).wrapping_mul(7));
            let mut dst = [0u8; 200];
            assert_eq!(copy_u8(&mut dst[..n], &src[..n]), n);
            assert_eq!(dst[..n], src[..n]);
            for pos in 0..n {
                let mut other = src;
                other[pos] ^= 0x80;
                assert_eq!(compare_u8(&src[..n], &other[..n]), src[pos].cmp(&other[pos]), "n {n} pos {pos}");
                assert_eq!(find_u8(&src[..n], src[pos]), src[..n].iter().position(|&b| b == src[pos]));
            }
            assert_eq!(find_u8(&src[..n], 0xFF), src[..n].iter().position(|&b| b == 0xFF));
        }
    }
}
