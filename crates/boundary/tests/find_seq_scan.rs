//! The pair scanner behind `memmem` (and the regex literal prefilter), FIND-SEQ-AVX2 (D-4.28):
//! `memmem` against a naive search for every needle length 1..=64, every haystack length 0..=300,
//! the match at the edges of every 16- and 32-byte block (and no match), at six alignments, with
//! decoys that match the first byte, the last byte, or both without the middle; and with the
//! haystack ending on the last byte of a page whose next page is unmapped (a load across the page
//! would fault). Both paths (SSE2, and AVX2 where the CPU has it).
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use std::sync::{Mutex, MutexGuard};

use liella_abi::ctypes::c_void;
use liella_boundary::posix::s04_strings as s;
use liella_boundary::selftest;
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{MAP_ANONYMOUS, MAP_PRIVATE, PROT_NONE, PROT_READ, PROT_WRITE};

const PAGE: usize = 4096;

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

/// `a` ... `c`, the middle varying: the first and last byte are the scanner's pair.
fn needle(m: usize) -> Vec<u8> {
    (0..m).map(|i| if i == 0 { b'a' } else if i + 1 == m { b'c' } else { b'b' + (i % 2) as u8 }).collect()
}

/// `n` bytes of `x`, decoys of the first byte, the last byte, and both (middle wrong), then the
/// needle planted at `pos`.
fn haystack(n: usize, nd: &[u8], pos: Option<usize>, seed: usize) -> Vec<u8> {
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
            h[p] = b'a';
            h[p + m - 1] = b'c';
        }
    }
    if let Some(p) = pos {
        h[p..p + m].copy_from_slice(nd);
    }
    h
}

fn naive(h: &[u8], nd: &[u8]) -> Option<usize> {
    if nd.len() > h.len() {
        return None;
    }
    (0..=h.len() - nd.len()).find(|&i| &h[i..i + nd.len()] == nd)
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

fn run(base: *const u8, n: usize, nd: &[u8], want: Option<usize>, ctx: &str) {
    let r = s::memmem(base.cast::<c_void>(), n, nd.as_ptr().cast::<c_void>(), nd.len());
    let got = if r.is_null() { None } else { Some(r as usize - base as usize) };
    assert_eq!(got, want, "{ctx}");
}

#[test]
fn memmem_matches_a_naive_search_at_every_length_position_and_alignment() {
    // Clauses: s4_6_c_0 (D-4.28).
    let _g = level();
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        let mut buf = vec![0xEEu8; 64 + 300 + 64];
        for m in 1..=64usize {
            let nd = needle(m);
            for n in 0..=300usize {
                for (k, pos) in positions(n, m).into_iter().enumerate() {
                    let h = haystack(n, &nd, pos, m + n + k);
                    let want = naive(&h, &nd);
                    for align in [0usize, 1, 17, 31, 32, 63] {
                        buf.fill(0xEE);
                        buf[align..align + n].copy_from_slice(&h);
                        run(buf[align..].as_ptr(), n, &nd, want, &format!("avx2={avx2} m={m} n={n} pos={pos:?} align={align}"));
                    }
                }
            }
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
}

#[test]
fn memmem_never_reads_past_the_end_of_the_haystack() {
    // Clauses: s4_6_c_0 (D-4.28).
    let _g = level();
    let r = CSys.mmap(0, 2 * PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, None, 0).unwrap();
    let (lo, mut hi) = r.split_at(PAGE).ok().unwrap();
    CSys.mprotect(&mut hi, PROT_NONE).unwrap();
    let base = lo.addr();
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        for m in 1..=64usize {
            let nd = needle(m);
            for n in 0..=300usize {
                for (k, pos) in positions(n, m).into_iter().enumerate() {
                    let h = haystack(n, &nd, pos, m + n + k);
                    let want = naive(&h, &nd);
                    let at = base + PAGE - n;
                    s::memcpy(at as *mut c_void, h.as_ptr().cast::<c_void>(), n);
                    run(at as *const u8, n, &nd, want, &format!("page end avx2={avx2} m={m} n={n} pos={pos:?}"));
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
fn memmem_with_many_failed_candidates_stays_correct() {
    // Clauses: s4_6_c_0 (D-4.28). The needle `ab..ba` in `a^n`: every position is a candidate that
    // fails at the end, the linear bound switches to the two-way search, the answer is the same.
    let _g = level();
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        for m in [2usize, 3, 17, 40, 64, 200] {
            for n in [m, m + 15, m + 16, m + 40, 1000, 5000] {
                let mut h = vec![b'a'; n];
                let mut nd = vec![b'b'; m];
                nd[0] = b'a';
                nd[m - 1] = b'a';
                run(h.as_ptr(), n, &nd, naive(&h, &nd), &format!("avx2={avx2} m={m} n={n} absent"));
                if n >= m {
                    h[n - m..].copy_from_slice(&nd);
                    run(h.as_ptr(), n, &nd, naive(&h, &nd), &format!("avx2={avx2} m={m} n={n} at the end"));
                }
            }
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
}
