//! `wcsstr` and `wcswcs` on the unit candidate scan (STRSTR-HIT): against a naive search for
//! every needle length 0..=20, haystack lengths 0..=90, a match at every position (and none),
//! units below and above 256 (the rare-unit choice ranks only units below 256), decoys that
//! repeat the needle's rarest unit (the failed-candidate path and its fallback to the two-way
//! search), and a copy of the needle after the haystack's terminator (a search that reads past
//! it finds that copy). STRSTR-VEC: both scan levels, needle 0..=70 x haystack 0..=300 at every
//! unit offset in a 32-byte block, units above U+FFFF and negative, page end and page crossing.
#![no_builtins] // calls to libc names reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use std::sync::{Mutex, MutexGuard};

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

/// The scan levels to run: SSE2, and AVX2 where the CPU has it.
fn paths() -> Vec<bool> {
    let mut v = vec![false];
    if selftest::set_scan_avx2(true) {
        v.push(true);
    }
    selftest::set_scan_avx2(false);
    v
}

/// Unit `i` of class `cls`: 0 small letters, 1 units above U+FFFF and negative `wchar_t` values
/// (equal only to themselves), mixed with letters.
fn unit2(i: usize, seed: usize, cls: usize) -> i32 {
    let x = (i.wrapping_mul(2_654_435_761).wrapping_add(seed.wrapping_mul(40_503))) >> 7;
    let tbl: [i32; 4] = if cls == 0 { [97, 98, 99, 100] } else { [0x1F600, -2, 0x10FFFF, 0x0061] };
    tbl[x % 4]
}

/// Match positions to try: none, the edges of the first 8-unit blocks, the last few.
fn positions(n: usize, m: usize) -> Vec<Option<usize>> {
    let mut v = vec![None];
    if n >= m {
        let last = n - m;
        let mut c = vec![0, 1, 7, 8, 9, 15, 16, 17, last, last.saturating_sub(1), last.saturating_sub(7), last.saturating_sub(8)];
        c.retain(|&p| p <= last);
        c.sort_unstable();
        c.dedup();
        v.extend(c.into_iter().map(Some));
    }
    v
}

/// `wcsstr` with the haystack copied to `at` (room for it, its terminator and the bait), a copy
/// of the needle after the terminator.
fn call_at(at: *mut i32, h: &[i32], nd: &[i32], bait: bool) -> Option<usize> {
    let mut img = h.to_vec();
    img.push(0);
    if bait {
        img.extend_from_slice(nd);
        img.push(0);
    }
    s::memcpy(at.cast::<liella_abi::ctypes::c_void>(), img.as_ptr().cast::<liella_abi::ctypes::c_void>(), img.len() * 4);
    let mut nz = nd.to_vec();
    nz.push(0);
    nz.extend_from_slice(&[0x1F600; 4]);
    let r = s::wcsstr(at, nz.as_ptr());
    if r.is_null() { None } else { Some((r as usize - at as usize) / 4) }
}

fn naive(h: &[i32], nd: &[i32]) -> Option<usize> {
    if nd.is_empty() {
        return Some(0);
    }
    (0..=h.len().checked_sub(nd.len())?).find(|&i| &h[i..i + nd.len()] == nd)
}

/// Unit `i` of a pseudo-random text over a small alphabet (plenty of partial matches).
fn unit(i: usize, seed: usize, wide: bool) -> i32 {
    let x = (i.wrapping_mul(2_654_435_761).wrapping_add(seed.wrapping_mul(40_503))) >> 7;
    let base = if wide { 0x3b1 } else { i32::from(b'a') };
    base + (x % 4) as i32
}

fn check(h: &[i32], nd: &[i32]) {
    let want = naive(h, nd);
    let mut hz = h.to_vec();
    hz.push(0);
    hz.extend_from_slice(nd); // a copy after the terminator
    hz.push(0);
    let mut nz = nd.to_vec();
    nz.push(0);
    for f in [s::wcsstr as extern "C" fn(*const i32, *const i32) -> *mut i32, s::wcswcs] {
        let r = f(hz.as_ptr(), nz.as_ptr());
        let got = if r.is_null() { None } else { Some((r as usize - hz.as_ptr() as usize) / 4) };
        assert_eq!(got, want, "hay {h:?} needle {nd:?}");
    }
}

#[test]
fn sweep_lengths_positions_and_unit_ranges() {
    // no spec clause: the wcsstr engine (conv::find_wide), an internal engine.
    // Trace: STRSTR-HIT wcsstr candidate scan against a naive search.
    for wide in [false, true] {
        for m in 0..=20usize {
            for n in 0..=90usize {
                let seed = m * 131 + n;
                let nd: Vec<i32> = (0..m).map(|i| unit(i, seed ^ 0x55, wide)).collect();
                let base: Vec<i32> = (0..n).map(|i| unit(i, seed, wide)).collect();
                check(&base, &nd);
                if m > 0 && m <= n {
                    for p in 0..=n - m {
                        let mut h = base.clone();
                        h[p..p + m].copy_from_slice(&nd);
                        check(&h, &nd);
                    }
                }
            }
        }
    }
}

#[test]
fn decoys_take_the_fallback_and_still_find_the_match() {
    // no spec clause: the wcsstr engine (conv::find_wide), an internal engine.
    // Trace: STRSTR-HIT failed candidates past the linear bound go to the two-way search.
    for m in [2usize, 3, 9, 40] {
        let mut nd = vec![i32::from(b'a'); m];
        nd[m - 1] = 0x10ffff; // the rarest unit, last
        for n in [64usize, 300, 2000] {
            // Every unit the rare one except where the prefix fails: many false candidates.
            let mut h: Vec<i32> = (0..n).map(|i| if i % 2 == 0 { 0x10ffff } else { i32::from(b'a') }).collect();
            check(&h, &nd);
            if n > m {
                let p = n - m;
                h[p..].copy_from_slice(&nd);
                check(&h, &nd);
            }
        }
    }
}

#[test]
fn wcsstr_on_both_scan_paths_at_every_unit_alignment_with_wide_units() {
    // no spec clause: the wcsstr engine (strz::find_wcs, STRSTR-VEC), an internal engine.
    // Trace: STRSTR-VEC fused unit scan against a naive search, needle 0..=70, haystack 0..=300,
    // every 4-byte offset in a 32-byte block, units above U+FFFF and negative values.
    let _g = level();
    let mut buf = vec![0i32; 8 + 300 + 80];
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        for cls in 0..2usize {
            for m in 0..=70usize {
                let nd: Vec<i32> = (0..m).map(|i| unit2(i, m ^ 0x55, cls)).collect();
                for n in 0..=300usize {
                    for (k, pos) in positions(n, m).into_iter().enumerate() {
                        let mut h: Vec<i32> = (0..n).map(|i| unit2(i, m * 131 + n + k, cls)).collect();
                        if let Some(p) = pos {
                            h[p..p + m].copy_from_slice(&nd);
                        }
                        let want = naive(&h, &nd);
                        for align in 0..8usize {
                            let got = call_at(buf[align..].as_mut_ptr(), &h, &nd, true);
                            assert_eq!(got, want, "avx2={avx2} cls={cls} m={m} n={n} pos={pos:?} align={align}");
                        }
                    }
                }
            }
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
}

#[test]
fn wcsstr_never_reads_past_the_terminator_at_a_page_end_or_across_pages() {
    // no spec clause: the wcsstr engine (strz::find_wcs, STRSTR-VEC), an internal engine.
    // Trace: STRSTR-VEC the haystack's zero unit is the last mapped unit (next page unmapped);
    // and a haystack starting in the last 40 bytes of a page, running into the next.
    let _g = level();
    let r = CSys.mmap(0, 3 * PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, None, 0).unwrap();
    let (lo, mut hi) = r.split_at(2 * PAGE).ok().unwrap();
    CSys.mprotect(&mut hi, PROT_NONE).unwrap();
    let base = lo.addr();
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        for m in 0..=70usize {
            let nd: Vec<i32> = (0..m).map(|i| unit2(i, m, 1)).collect();
            for n in 0..=300usize {
                for (k, pos) in positions(n, m).into_iter().enumerate() {
                    let mut h: Vec<i32> = (0..n).map(|i| unit2(i, n + k, 1)).collect();
                    if let Some(p) = pos {
                        h[p..p + m].copy_from_slice(&nd);
                    }
                    let want = naive(&h, &nd);
                    let at = (base + 2 * PAGE - 4 * (n + 1)) as *mut i32;
                    assert_eq!(call_at(at, &h, &nd, false), want, "page end avx2={avx2} m={m} n={n} pos={pos:?}");
                    if m % 7 == 1 || m == 70 {
                        for back in 1..=10usize {
                            let at = (base + PAGE - 4 * back) as *mut i32;
                            assert_eq!(call_at(at, &h, &nd, true), want, "page cross avx2={avx2} m={m} n={n} pos={pos:?} back={back}");
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
