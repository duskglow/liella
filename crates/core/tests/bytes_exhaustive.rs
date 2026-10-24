//! Exhaustive tests of the `bytes` primitives behind `mem*`/`str*` (implementation-plan.md,
//! phase 2 test surface): every length 0–256 at every alignment 0–15, every byte value as the
//! searched or differing byte, and overlapping moves in both directions (D-4.2). The reference
//! for each primitive is a plain loop written here.

#![allow(clippy::arithmetic_side_effects, clippy::indexing_slicing, clippy::as_conversions, clippy::unwrap_used, missing_docs)]

use std::cmp::Ordering;

use liella::bytes::{self, ByteSet, Move};

fn pattern(n: usize, seed: u8) -> Vec<u8> {
    (0..n).map(|i| (i as u8).wrapping_mul(37).wrapping_add(seed) | 1).collect()
}

#[test]
fn copy_and_fill_every_length_and_alignment() {
    // Clauses: s4_1_c_1, s4_1_g_0, s4_2_c_1 (D-4.11).
    let mut src_buf = vec![0u8; 256 + 16];
    let mut dst_buf = vec![0u8; 256 + 32];
    for len in 0..=256 {
        for sa in 0..16 {
            for da in 0..16 {
                src_buf[sa..sa + len].copy_from_slice(&pattern(len, sa as u8));
                dst_buf.fill(0xEE);
                assert_eq!(bytes::copy(&mut dst_buf[da..da + len], &src_buf[sa..sa + len]), len);
                assert_eq!(dst_buf[da..da + len], src_buf[sa..sa + len], "len {len} sa {sa} da {da}");
                assert!(dst_buf[..da].iter().chain(&dst_buf[da + len..]).all(|&b| b == 0xEE), "no write outside");
            }
        }
        for da in 0..16 {
            dst_buf.fill(0xEE);
            bytes::fill(&mut dst_buf[da..da + len], 0x5A);
            assert!(dst_buf[da..da + len].iter().all(|&b| b == 0x5A));
            assert!(dst_buf[..da].iter().chain(&dst_buf[da + len..]).all(|&b| b == 0xEE));
        }
    }
}

#[test]
fn overlapping_moves_both_directions() {
    // Clauses: s4_1_c_2, s4_1_c_1, s4_1_g_0 (D-4.2).
    for len in 0..=128 {
        for src in 0..20 {
            for dst in 0..20 {
                let mut a: Vec<u8> = pattern(160, 3);
                let mut b = a.clone();
                assert!(bytes::move_within(&mut a, src, dst, len));
                b.copy_within(src..src + len, dst);
                assert_eq!(a, b, "len {len} {src}->{dst}");
                let mut c = pattern(160, 3);
                let span = len + src.max(dst);
                let lo = src.min(dst);
                let moved = bytes::apply_move(Move::Within(&mut c[lo..span], src - lo, dst - lo));
                assert_eq!((moved, c), (len, b.clone()), "apply_move len {len} {src}->{dst}");
            }
        }
    }
}

#[test]
fn compare_every_position_and_byte_value() {
    // Clauses: s4_3_c_1, s4_3_g_0 (D-4.11).
    for len in [0usize, 1, 2, 7, 8, 9, 15, 16, 17, 31, 32, 33, 63, 64, 65, 255, 256] {
        let a = pattern(len, 9);
        assert_eq!(bytes::compare(&a, &a), Ordering::Equal);
        for pos in 0..len {
            for v in 0..=255u8 {
                let mut b = a.clone();
                b[pos] = v;
                assert_eq!(bytes::compare(&a, &b), a[pos].cmp(&v), "len {len} pos {pos} byte {v:#x}");
            }
        }
    }
}

#[test]
fn search_every_byte_value_length_and_alignment() {
    // Clauses: s4_5_c_1, s4_5_c_2, s4_7_i_0.
    let hay: Vec<u8> = (0..=255u8).chain(0..=255u8).collect();
    for off in 0..16 {
        for len in 0..=256 {
            let s = &hay[off..off + len];
            for v in 0..=255u8 {
                assert_eq!(bytes::find(s, v), s.iter().position(|&b| b == v), "find {v:#x} len {len} off {off}");
                assert_eq!(bytes::rfind(s, v), s.iter().rposition(|&b| b == v), "rfind");
            }
        }
    }
    for v in 0..=255u8 {
        let set = ByteSet::new(&[v, v.wrapping_add(1)]);
        for b in 0..=255u8 {
            assert_eq!(set.contains(b), b != 0 && (b == v || b == v.wrapping_add(1)), "set {v:#x} has {b:#x}");
        }
    }
}

#[test]
fn text_compare_and_sequence_search() {
    // Clauses: s4_3_c_1, s4_6_c_0.
    for len in 0..64 {
        let a = pattern(len, 1);
        for pos in 0..len {
            let mut b = a.clone();
            b.truncate(pos);
            assert_eq!(bytes::compare_text(&a, &b), Ordering::Greater, "a proper prefix is smaller");
        }
        for nlen in 0..=len.min(8) {
            for start in 0..=len - nlen {
                let needle = &a[start..start + nlen];
                let want = (0..=len - nlen).find(|&i| &a[i..i + nlen] == needle);
                assert_eq!(bytes::find_seq(&a, needle, core::convert::identity), want, "len {len} needle {start}+{nlen}");
            }
        }
    }
}

#[test]
fn ascii_folded_compare_equals_the_per_byte_fold() {
    // Clauses: s4_3_c_1 (PJ-23): the eight-byte path gives compare_text_by's answer for a fold that
    // is ASCII's on 0..127, at every length, every position of a difference, and both cases of
    // every byte, including the neighbours of the letter ranges and bytes of 128 and more (which
    // a non-identity fold maps elsewhere).
    let fold = |c: u8| if c.is_ascii() { c.to_ascii_lowercase() } else { c ^ 0x20 };
    let base: Vec<u8> = (0..40u8).map(|i| b"Hello, World! [ABC] `xyz` {@} 0123456789"[usize::from(i)]).collect();
    for len in 0..=base.len() {
        let a = &base[..len];
        let upper: Vec<u8> = a.iter().map(u8::to_ascii_uppercase).collect();
        assert_eq!(bytes::compare_text_ascii_folded(a, &upper, fold), Ordering::Equal);
        for b in [&base[..len.saturating_sub(1)], &base[..(len + 1).min(base.len())]] {
            assert_eq!(bytes::compare_text_ascii_folded(a, b, fold), bytes::compare_text_by(a, b, fold), "len {len}");
        }
        for pos in 0..len {
            for v in 1..=255u8 {
                let mut b = upper.clone();
                b[pos] = v;
                assert_eq!(bytes::compare_text_ascii_folded(a, &b, fold), bytes::compare_text_by(a, &b, fold), "len {len} pos {pos} byte {v:#x}");
                let mut c = a.to_vec();
                c[pos] = v;
                assert_eq!(bytes::compare_text_ascii_folded(&c, &upper, fold), bytes::compare_text_by(&c, &upper, fold), "len {len} pos {pos} byte {v:#x}");
            }
        }
    }
}
