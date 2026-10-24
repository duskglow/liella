//! `strstr` and `strcasestr` on the pair scanner (STRSTR-FIND): against a naive search for every
//! needle length 0..=70, every haystack length 0..=300, a match at the edges of every 16- and
//! 32-byte block (and none), six alignments, with decoys, with a copy of the needle placed AFTER
//! the haystack's terminator (a search that reads past it finds that copy) and bytes after the
//! needle's terminator (a search that ignores the needle's NUL sees a longer needle); the
//! haystack ending on the last byte of a page whose next page is unmapped; `strcasestr` with the
//! case varied, with non-ASCII bytes (0x80..0xFF fold to nothing in the C and C.UTF-8 locales) in
//! both strings. Both scan paths (SSE2, and AVX2 where the CPU has it).
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use std::sync::{Mutex, MutexGuard};

use core::ffi::c_char;
use liella_abi::ctypes::c_void;
use liella_boundary::posix::s04_strings as s;
use liella_boundary::posix::s07_locale as loc;
use liella_boundary::selftest;
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{MAP_ANONYMOUS, MAP_PRIVATE, PROT_NONE, PROT_READ, PROT_WRITE};

const PAGE: usize = 4096;
const LC_CTYPE: i32 = 0;

static LEVEL: Mutex<()> = Mutex::new(());

fn level() -> MutexGuard<'static, ()> {
    LEVEL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn paths() -> Vec<bool> {
    let mut v = vec![false];
    if selftest::set_scan_avx2(true) {
        v.push(true);
    }
    selftest::set_scan_avx2(false);
    v
}

fn set_ctype(name: &[u8]) {
    assert!(!loc::setlocale(LC_CTYPE, name.as_ptr().cast::<c_char>()).is_null());
}

/// The needle: `a` ... `c`, the middle varying; `upper` flips the case of every letter, `high`
/// puts non-ASCII bytes in the middle.
fn needle(m: usize, upper: bool, high: bool) -> Vec<u8> {
    (0..m)
        .map(|i| {
            let c = if i == 0 {
                b'a'
            } else if i + 1 == m {
                b'c'
            } else if high && i % 3 == 0 {
                0xC0 + (i % 16) as u8
            } else {
                b'b' + (i % 2) as u8
            };
            if upper { c.to_ascii_uppercase() } else { c }
        })
        .collect()
}

/// `n` bytes of `x`, decoys of the first byte, the last byte and both (middle wrong), the needle
/// (case flipped when `flip`) planted at `pos`.
fn haystack(n: usize, nd: &[u8], pos: Option<usize>, seed: usize, flip: bool) -> Vec<u8> {
    let m = nd.len();
    let mut h = vec![b'x'; n];
    for p in 0..n {
        if (p + seed) % 7 == 0 {
            h[p] = b'a';
        }
        if (p + seed) % 11 == 0 {
            h[p] = b'c';
        }
        if (p + seed) % 5 == 0 && p + m <= n && m > 2 {
            h[p] = b'A';
            h[p + m - 1] = b'C';
        }
    }
    if let Some(p) = pos {
        for (k, &c) in nd.iter().enumerate() {
            h[p + k] = if flip { if c.is_ascii_uppercase() { c.to_ascii_lowercase() } else { c.to_ascii_uppercase() } } else { c };
        }
    }
    h
}

fn naive(h: &[u8], nd: &[u8], ci: bool) -> Option<usize> {
    if nd.len() > h.len() {
        return None;
    }
    (0..=h.len() - nd.len()).find(|&i| if ci { h[i..i + nd.len()].eq_ignore_ascii_case(nd) } else { &h[i..i + nd.len()] == nd })
}

fn positions(n: usize, m: usize) -> Vec<Option<usize>> {
    let mut v = vec![None];
    if n >= m {
        let last = n - m;
        let mut c = vec![0, 1, 2, 15, 16, 17, 31, 32, 33, 47, 48, 63, 64, 65, last, last.saturating_sub(1), last.saturating_sub(15), last.saturating_sub(16), last.saturating_sub(31)];
        c.retain(|&p| p <= last);
        c.sort_unstable();
        c.dedup();
        v.extend(c.into_iter().map(Some));
    }
    v
}

/// Writes `h`, NUL, then a copy of the needle (tail bait) at `at`; the needle with its NUL and
/// bait bytes into `nbuf`.
fn call(at: *mut u8, h: &[u8], nd: &[u8], nbuf: &mut Vec<u8>, ci: bool, bait: bool) -> Option<usize> {
    // `at` has room for h.len() + 1 (+ the bait when `bait`) bytes in every caller.
    let mut img = h.to_vec();
    img.push(0);
    if bait {
        img.extend_from_slice(nd);
        img.push(0);
    }
    s::memcpy(at.cast::<c_void>(), img.as_ptr().cast::<c_void>(), img.len());
    nbuf.clear();
    nbuf.extend_from_slice(nd);
    nbuf.push(0);
    nbuf.extend_from_slice(b"zzzzzzzz");
    let r = if ci { s::strcasestr(at.cast::<c_char>(), nbuf.as_ptr().cast::<c_char>()) } else { s::strstr(at.cast::<c_char>(), nbuf.as_ptr().cast::<c_char>()) };
    if r.is_null() { None } else { Some(r as usize - at as usize) }
}

fn sweep(ci: bool, high: bool, flip: bool, ms: core::ops::Range<usize>) {
    let mut buf = vec![0xEEu8; 64 + 300 + 80 + 8];
    let mut nbuf = Vec::new();
    // strstr (STRSTR-VEC's fused scan): every offset in a 32-byte block; strcasestr: six.
    let aligns: Vec<usize> = if ci { vec![0, 1, 17, 31, 32, 63] } else { (0..32).collect() };
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        for m in ms.clone() {
            for upper in [false, true] {
                // The needle is a letter pattern with the case chosen here; with ci false only the
                // exact case is searched, so only `flip == false` plants a matching haystack.
                let nd = needle(m, upper, high);
                for n in 0..=300usize {
                    for (k, pos) in positions(n, m).into_iter().enumerate() {
                        // Bytes of NUL in the haystack would end it: the generators never make one.
                        let h = haystack(n, &nd, pos, m + n + k, flip);
                        let want = if m == 0 { Some(0) } else { naive(&h, &nd, ci) };
                        for &align in &aligns {
                            buf.fill(0xEE);
                            let got = call(buf[align..].as_mut_ptr(), &h, &nd, &mut nbuf, ci, true);
                            assert_eq!(got, want, "ci={ci} high={high} flip={flip} avx2={avx2} m={m} upper={upper} n={n} pos={pos:?} align={align}");
                        }
                    }
                }
            }
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
}

/// GATE-SPEED: each sweep test runs in needle-length ranges (one test each); `M_SHARDS` tiles
/// 0..=70 (checked by `the_needle_length_shards_tile_0_to_70`), so the coverage is the whole sweep's.
const M_SHARDS: [core::ops::Range<usize>; 4] = [0..16, 16..32, 32..48, 48..71];

/// A module of one test per needle-length range, each running `$body(range)`.
macro_rules! shards {
    ($name:ident, $setup:expr, $body:expr) => {
        mod $name {
            use super::*;
            fn run(i: usize) {
                // Clauses: s4_6_c_0 (D-4.28).
                let _g = level();
                let () = $setup;
                $body(M_SHARDS[i].clone());
            }
            #[test]
            fn m_0_to_15() {
    // no spec clause: a shard of the strstr engine sweep, an internal engine.
                run(0);
            }
            #[test]
            fn m_16_to_31() {
    // no spec clause: a shard of the strstr engine sweep, an internal engine.
                run(1);
            }
            #[test]
            fn m_32_to_47() {
    // no spec clause: a shard of the strstr engine sweep, an internal engine.
                run(2);
            }
            #[test]
            fn m_48_to_70() {
    // no spec clause: a shard of the strstr engine sweep, an internal engine.
                run(3);
            }
        }
    };
}

#[test]
fn the_needle_length_shards_tile_0_to_70() {
    // no spec clause: a shard of the strstr engine sweep, an internal engine.
    let mut next = 0;
    for r in M_SHARDS {
        assert_eq!(r.start, next);
        assert!(r.end > r.start);
        next = r.end;
    }
    assert_eq!(next, 71);
}

shards!(strstr_matches_a_naive_search_at_every_length_position_and_alignment, (), |ms: core::ops::Range<usize>| {
    sweep(false, false, false, ms.clone());
    sweep(false, true, false, ms);
});

// Bytes 0x80..0xFF are no characters alone in a UTF-8 locale: they fold to nothing.
shards!(strcasestr_matches_a_naive_search_in_the_c_locale, set_ctype(b"C\0"), |ms: core::ops::Range<usize>| {
    sweep(true, false, false, ms.clone());
    sweep(true, false, true, ms.clone());
    sweep(true, true, true, ms);
});

shards!(strcasestr_matches_a_naive_search_in_a_utf8_locale, set_ctype(b"C.UTF-8\0"), |ms: core::ops::Range<usize>| {
    sweep(true, false, true, ms.clone());
    sweep(true, true, true, ms);
    set_ctype(b"C\0");
});

#[test]
fn strstr_and_strcasestr_never_read_past_the_terminator() {
    // Clauses: s4_6_c_0 (D-4.28). The haystack's NUL is the last mapped byte.
    let _g = level();
    set_ctype(b"C\0");
    let r = CSys.mmap(0, 2 * PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, None, 0).unwrap();
    let (lo, mut hi) = r.split_at(PAGE).ok().unwrap();
    CSys.mprotect(&mut hi, PROT_NONE).unwrap();
    let base = lo.addr();
    let mut nbuf = Vec::new();
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        for m in 0..=70usize {
            let nd = needle(m, false, false);
            for n in 0..=300usize {
                for (k, pos) in positions(n, m).into_iter().enumerate() {
                    for ci in [false, true] {
                        let h = haystack(n, &nd, pos, m + n + k, ci);
                        let want = if m == 0 { Some(0) } else { naive(&h, &nd, ci) };
                        let at = (base + PAGE - n - 1) as *mut u8;
                        let got = call(at, &h, &nd, &mut nbuf, ci, false);
                        assert_eq!(got, want, "page end ci={ci} avx2={avx2} m={m} n={n} pos={pos:?}");
                    }
                }
            }
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
    CSys.munmap(hi).unwrap();
    CSys.munmap(lo).unwrap();
}

#[test]
fn strstr_and_strcasestr_with_many_failed_candidates_stay_correct() {
    // Clauses: s4_6_c_0 (D-4.28). Needle `aB..Ba` in `a^n`: every position is a candidate that
    // fails at the end, the linear bound switches to the two-way search.
    let _g = level();
    set_ctype(b"C\0");
    let mut nbuf = Vec::new();
    let mut buf = vec![0u8; 5100];
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        for m in [2usize, 3, 17, 40, 64, 200] {
            for n in [m, m + 15, m + 16, m + 40, 1000, 5000] {
                for ci in [false, true] {
                    let mut h = vec![b'a'; n];
                    let mut nd = vec![b'B'; m];
                    nd[0] = b'a';
                    nd[m - 1] = b'A';
                    assert_eq!(call(buf.as_mut_ptr(), &h, &nd, &mut nbuf, ci, false), naive(&h, &nd, ci), "ci={ci} avx2={avx2} m={m} n={n} absent");
                    if n >= m {
                        h[n - m..].copy_from_slice(&nd);
                        assert_eq!(call(buf.as_mut_ptr(), &h, &nd, &mut nbuf, ci, false), naive(&h, &nd, ci), "ci={ci} avx2={avx2} m={m} n={n} at the end");
                    }
                }
            }
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
}

#[test]
fn strstr_across_a_page_boundary_from_the_last_bytes_of_a_page() {
    // Clauses: s4_6_c_0 (D-4.28). STRSTR-VEC: a haystack starting in the last 40 bytes of a page
    // and running into the next, so the fused scan starts (and resumes after false candidates)
    // within 32 bytes of a page end; the page after the second is unmapped.
    let _g = level();
    set_ctype(b"C\0");
    let r = CSys.mmap(0, 3 * PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, None, 0).unwrap();
    let (lo, mut hi) = r.split_at(2 * PAGE).ok().unwrap();
    CSys.mprotect(&mut hi, PROT_NONE).unwrap();
    let base = lo.addr();
    let mut nbuf = Vec::new();
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        for m in [1usize, 2, 3, 5, 17, 32, 33, 70] {
            for upper in [false, true] {
                let nd = needle(m, upper, false);
                for n in [m, m + 1, 40, 100, 300] {
                    for (k, pos) in positions(n, m).into_iter().enumerate() {
                        let h = haystack(n, &nd, pos, m + n + k, false);
                        let want = naive(&h, &nd, false);
                        for back in 1..=40usize {
                            let at = (base + PAGE - back) as *mut u8;
                            let got = call(at, &h, &nd, &mut nbuf, false, true);
                            assert_eq!(got, want, "page cross avx2={avx2} m={m} n={n} pos={pos:?} back={back}");
                        }
                    }
                }
            }
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
    CSys.munmap(hi).unwrap();
    CSys.munmap(lo).unwrap();
}

/// A candidate that differs from the needle in one byte only (the scanner's pair bytes may
/// match) is rejected, for every needle length 2..=40 and every differing position
/// (STRSTR-SHORT-SETUP: the short candidate compare's overlapping word loads).
#[test]
fn strstr_rejects_candidates_differing_in_one_byte() {
    // Clauses: s4_6_c_0 (D-4.28).
    let _g = level();
    for m in 2..=40usize {
        let nd: Vec<u8> = (0..m).map(|i| b'a' + (i % 26) as u8).collect();
        let mut n0 = nd.clone();
        n0.push(0);
        for j in 0..m {
            let mut h = Vec::new();
            for _ in 0..3 {
                let mut d = nd.clone();
                d[j] = b'#';
                h.extend_from_slice(&d);
                h.push(b' ');
            }
            let at = h.len();
            h.extend_from_slice(&nd);
            h.push(0);
            let r = s::strstr(h.as_ptr().cast(), n0.as_ptr().cast());
            assert_eq!(r as usize, h.as_ptr() as usize + at, "needle length {m}, differing byte {j}");
        }
    }
}
