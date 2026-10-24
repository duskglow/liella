//! The C layer's scanners, ERMS copies and AVX2 copy (OD-C2, rule P7, D-4.14), held to exhaustive tests:
//! rule P7 makes these the evidence for the one postcondition Rust cannot check in constant
//! time (that a scanner found the *first* terminator).
//!
//! Each scanner runs on both paths (SSE2, and AVX2 where the CPU has it) at every alignment of
//! a 128-byte window (from a 128-aligned base) and every length up to 300, with zero bytes planted before the string (so
//! the masking of an aligned block's leading bytes is exercised), and with strings that end on
//! the last byte of a page followed by an unmapped page (a load across the page would fault).
//! The bounded scanners (`memchr`, `memcmp`) run at every alignment, length and position of the
//! first match or difference, with decoys after it and everywhere outside the region, and in
//! regions touching an unmapped page on either side.
//!
//! The scanners are reached through the exports (`strlen`, `strnlen`, `rawmemchr`, `memchr`,
//! `memcmp`, `bcmp`, `__memcmpeq`), so the Rust
//! postcondition checks run too.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use std::sync::{Mutex, MutexGuard};

use liella_abi::ctypes::{c_char, c_int, c_void};
use liella_boundary::posix::s04_strings as s;
use liella_boundary::selftest;
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{MAP_ANONYMOUS, MAP_PRIVATE, PROT_NONE, PROT_READ, PROT_WRITE};

const PAGE: usize = 4096;

/// The scan level is process-wide: tests that switch it hold this.
static LEVEL: Mutex<()> = Mutex::new(());

fn level() -> MutexGuard<'static, ()> {
    LEVEL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The paths this CPU can run: SSE2, and AVX2 if present.
fn paths() -> Vec<bool> {
    let mut v = vec![false];
    if selftest::set_scan_avx2(true) {
        v.push(true);
    }
    selftest::set_scan_avx2(false);
    v
}

fn at(buf: &mut [u8], i: usize) -> *mut u8 {
    buf[i..].as_mut_ptr()
}

#[test]
fn scanners_find_the_first_terminator_at_every_alignment_and_length() {
    // Clauses: s4_8_c_0, s4_5_c_3 (D-4.14, P7).
    let _g = level();
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        // Every offset from a 128-aligned base (0..128): the AVX2 loops step in 128-byte
        // groups, so a group that starts 128-aligned past the head is exercised at some offset.
        let mut buf = vec![0u8; 128 + 128 + 300 + 256];
        let base = buf.as_ptr().align_offset(128);
        for off in 0..128 {
            let align = base + off;
            for len in 0..300 {
                buf.fill(0); // zero bytes before the string: the scanners must mask them off
                buf[align..align + len].fill(b'x');
                buf[align + len] = 0;
                buf[align + len + 1..].fill(b'y');
                let p = at(&mut buf, align).cast::<c_char>();
                assert_eq!(s::strlen(p), len, "strlen, avx2={avx2}, align {align}, len {len}");
                for max in [0, len.saturating_sub(1), len, len + 1, len + 200, usize::MAX] {
                    assert_eq!(s::strnlen(p, max), len.min(max), "strnlen, avx2={avx2}, align {align}, len {len}, max {max}");
                }
                buf[align + len] = b'#';
                let r = s::rawmemchr(at(&mut buf, align).cast::<c_void>(), c_int::from(b'#'));
                assert_eq!(r as usize - p as usize, len, "rawmemchr, avx2={avx2}, align {align}, len {len}");
            }
        }
        // strnlen when the bound ends before the terminator, at every alignment.
        for off in 0..128 {
            let align = base + off;
            buf.fill(b'z');
            for max in 0..200 {
                assert_eq!(s::strnlen(at(&mut buf, align).cast(), max), max);
            }
        }
        // rawmemchr for every byte value (the broadcast), including 0 and 0xFF.
        for c in 0..=255u8 {
            buf.fill(c.wrapping_add(1));
            buf[100] = c;
            let r = s::rawmemchr(at(&mut buf, 3).cast(), c_int::from(c));
            assert_eq!(r as usize - at(&mut buf, 3) as usize, 97, "byte {c}");
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
}

/// Two pages; the second unmapped (no access). Returns the mapping and the first page's base.
fn guarded() -> (liella_kernel::mem::Region, liella_kernel::mem::Region, usize) {
    let r = CSys.mmap(0, 2 * PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, None, 0).unwrap();
    let (lo, mut hi) = r.split_at(PAGE).ok().unwrap();
    CSys.mprotect(&mut hi, PROT_NONE).unwrap();
    let base = lo.addr();
    (lo, hi, base)
}

#[test]
fn strcpy_never_reads_past_the_page_holding_the_terminator() {
    // Clauses: s4_1_c_6 (D-4.22). STRCPY-SHORT-3: the direct strcpy entry's 64-byte head and its
    // aligned long scan stay in the pages the string occupies; the terminator is the last byte.
    let (lo, hi, base) = guarded();
    for len in 0..200usize {
        let at = base + PAGE - 1 - len;
        // [at, base + PAGE) lies in the mapped, writable first page (the crate forbids `unsafe`,
        // so the text goes in through memcpy).
        let mut want: Vec<u8> = (0..len).map(|i| b'a' + (i % 26) as u8).collect();
        want.push(0);
        s::memcpy(at as *mut c_void, want.as_ptr().cast(), len + 1);
        let mut d = vec![0x5au8; len + 2];
        let r = s::strcpy(d.as_mut_ptr().cast::<c_char>(), at as *const c_char);
        assert_eq!(r, d.as_mut_ptr().cast::<c_char>(), "len {len}: return value");
        assert_eq!(&d[..=len], &want[..], "len {len}: text and terminator");
        assert_eq!(d[len + 1], 0x5a, "len {len}: wrote outside");
    }
    CSys.munmap(hi).unwrap();
    CSys.munmap(lo).unwrap();
}

/// The source text of `strcpy_family_sweep_on_level`: `len` letters then the NUL at page offset `off` of a
/// 3-page buffer.
fn copy_source(len: usize, off: usize) -> (Vec<u8>, usize) {
    let mut src = vec![0u8; 3 * PAGE];
    let at = fill_copy_source(&mut src, len, off);
    (src, at)
}

/// [`copy_source`] in a caller's buffer of `3 * PAGE` bytes (a forked child's, on its stack): the text's start.
fn fill_copy_source(src: &mut [u8], len: usize, off: usize) -> usize {
    let at = (PAGE - (src.as_ptr() as usize) % PAGE) % PAGE + off;
    src[at..at + len].iter_mut().enumerate().for_each(|(i, b)| *b = b'a' + (i % 26) as u8);
    src[at + len] = 0;
    at
}

/// The aborting child's call (set before the fork; the child gets its own copy): length, page offset, entry.
static SHORT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn short_room_child(_: &CSys) -> i32 {
    let v = SHORT.load(std::sync::atomic::Ordering::Relaxed);
    let (len, off, f) = (v & 0xff, (v >> 8) & 0xfff, v >> 20);
    let mut src = [0u8; 3 * PAGE];
    let at = fill_copy_source(&mut src, len, off);
    let sp = src[at..].as_ptr().cast::<c_char>();
    // Room one byte short of len + 1; the buffer itself has space, so a missing check returns instead of aborting.
    // On the stack (len <= 0xff): a forked child must not allocate (FORK-CHILD-ALLOC-LINT).
    let mut d = [0x5au8; 0xff + 8];
    let dp = d.as_mut_ptr().cast::<c_char>();
    if f == 0 {
        s::__strcpy_chk(dp, sp, len);
    } else {
        s::__stpcpy_chk(dp, sp, len);
    }
    0
}

#[test]
fn strcpy_family_sweep_on_level() {
    // Clauses: s4_1_c_6 (D-4.22, D-4.21). STRCPY-AVX2: both scan levels (the AVX2 head with its 16-31 and 32-63
    // classes, and the SSE2 ladder), every length 0..=80 at every source page offset 4000..=4095 (4032 is the
    // last AVX2 head offset), destination offsets 0..=2: strcpy/stpcpy/__strcpy_chk/__stpcpy_chk (exact room)
    // copy the text and its terminator, return the right pointer and write nothing outside; one byte short of
    // room aborts the _chk entries.
    let _g = level();
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        for len in 0..=80usize {
            for off in 4000..PAGE {
                let (src, at) = copy_source(len, off);
                let sp = src[at..].as_ptr().cast::<c_char>();
                for doff in 0..3usize {
                    for f in 0..4 {
                        let mut d = vec![0x5au8; len + 8];
                        let dp = d[doff..].as_mut_ptr().cast::<c_char>();
                        let r = match f {
                            0 => s::strcpy(dp, sp),
                            1 => s::stpcpy(dp, sp),
                            2 => s::__strcpy_chk(dp, sp, len + 1),
                            _ => s::__stpcpy_chk(dp, sp, len + 1),
                        };
                        let want = if f % 2 == 0 { dp } else { dp.wrapping_add(len) };
                        let what = format!("avx2 {avx2} f {f} len {len} off {off} doff {doff}");
                        assert_eq!(r, want, "{what}: return value");
                        assert_eq!(&d[doff..doff + len], &src[at..at + len], "{what}: text");
                        assert_eq!(d[doff + len], 0, "{what}: terminator");
                        assert!(d[..doff].iter().chain(&d[doff + len + 1..]).all(|&b| b == 0x5a), "{what}: wrote outside");
                    }
                }
                if [4000, 4032, 4033].contains(&off) {
                    for f in 0..2usize {
                        SHORT.store(len | off << 8 | f << 20, std::sync::atomic::Ordering::Relaxed);
                        let end = selftest::in_child_quiet(short_room_child).unwrap();
                        assert_eq!(end, selftest::ChildEnd::Signaled(6), "avx2 {avx2} f {f} len {len} off {off}: room one byte short must abort");
                    }
                }
            }
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
}

#[test]
fn scanners_never_read_past_the_page_holding_the_terminator() {
    // Clauses: s4_8_c_0, s4_5_c_3 (D-4.14, P7).
    let _g = level();
    let (lo, hi, base) = guarded();
    let page = base as *mut c_void;
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        for len in 0..400usize {
            // The terminator is the page's last byte; the next byte would fault.
            s::memset(page, c_int::from(b'a'), PAGE - 1);
            s::memset((base + PAGE - 1) as *mut c_void, 0, 1);
            let p = (base + PAGE - 1 - len) as *const c_char;
            assert_eq!(s::strlen(p), len, "strlen, avx2={avx2}, len {len}");
            assert_eq!(s::strnlen(p, usize::MAX), len);
            // No terminator at all, but the bound ends exactly at the page end.
            s::memset((base + PAGE - 1) as *mut c_void, c_int::from(b'a'), 1);
            assert_eq!(s::strnlen(p, len + 1), len + 1, "strnlen to the page end, avx2={avx2}, len {len}");
            // The searched byte is the page's last.
            s::memset((base + PAGE - 1) as *mut c_void, c_int::from(b'!'), 1);
            let r = s::rawmemchr(p.cast(), c_int::from(b'!'));
            assert_eq!(r as usize, base + PAGE - 1, "rawmemchr, avx2={avx2}, len {len}");
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
    CSys.munmap(hi).unwrap();
    CSys.munmap(lo).unwrap();
}

#[test]
fn scanners_never_read_below_the_page_holding_the_start() {
    // Clauses: s4_8_c_0 (D-4.14, P7).
    let _g = level();
    // The first page unmapped, the second readable: strings start at the second page's
    // first bytes, so an aligned-down load must stay in the second page.
    let r = CSys.mmap(0, 2 * PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, None, 0).unwrap();
    let (mut lo, hi) = r.split_at(PAGE).ok().unwrap();
    CSys.mprotect(&mut lo, PROT_NONE).unwrap();
    let base = hi.addr();
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        for off in 0..128usize {
            s::memset(base as *mut c_void, c_int::from(b'q'), PAGE);
            s::memset((base + off + 10) as *mut c_void, 0, 1);
            assert_eq!(s::strlen((base + off) as *const c_char), 10, "avx2={avx2}, offset {off}");
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
    CSys.munmap(hi).unwrap();
    CSys.munmap(lo).unwrap();
}

#[test]
fn erms_copy_and_fill_match_the_loops_at_every_size_and_alignment() {
    // Clauses: s4_1_c_1, s4_1_g_0, s4_2_c_1 (D-4.14, P7).
    for n in (0..600).chain([1279, 1280, 1281, 4096, 4097, 65_536 + 3]) {
        for off in [0usize, 1, 7, 31, 63] {
            let src: Vec<u8> = (0..n + 64).map(|i| (i * 7 + 3) as u8).collect();
            let mut a = vec![0xEEu8; n + 128];
            let mut b = a.clone();
            assert_eq!(selftest::copy_erms(&mut a[off..off + n], &src[off..off + n]), n);
            b[off..off + n].copy_from_slice(&src[off..off + n]);
            assert_eq!(a, b, "copy {n} at {off}: nothing outside the region changes");
            selftest::fill_erms(&mut a[off..off + n], 0x5C);
            b[off..off + n].fill(0x5C);
            assert_eq!(a, b, "fill {n} at {off}");
            // The exports, across the threshold, choose either path and agree.
            s::memcpy(a[off..].as_mut_ptr().cast(), src[off..].as_ptr().cast(), n);
            b[off..off + n].copy_from_slice(&src[off..off + n]);
            assert_eq!(a, b, "memcpy {n} at {off}");
            s::memset(a[off..].as_mut_ptr().cast(), 0x11, n);
            b[off..off + n].fill(0x11);
            assert_eq!(a, b, "memset {n} at {off}");
        }
    }
}

/// Sizes around every boundary of the copy paths: the AVX2 copy's 32-byte head and tail and
/// 128-byte steps, the dispatch thresholds, and 4 KiB.
const COPY_SIZES: [usize; 24] = [64, 65, 95, 96, 97, 127, 128, 129, 159, 160, 161, 255, 256, 257, 383, 511, 512, 513, 1279, 1280, 1281, 4095, 4096, 4097];

#[test]
fn avx2_copy_matches_the_source_at_every_alignment_pair_and_size() {
    // Clauses: s4_1_c_1, s4_1_g_0 (D-4.14, PJ-7). The routine itself, also below the dispatch
    // range, nothing outside the destination written.
    let _g = level();
    liella_boundary::cpu::init();
    // Source and destination in one buffer, the destination 8 KiB + `gap` above the source, so
    // (dst - src) mod 4 KiB is gap + da - sa: both of the routine's directions run (counted).
    const DST: usize = 8192;
    let pattern: Vec<u8> = (0..DST + 3000 + 4097 + 128).map(|i| (i * 7 + 3 + i / 251) as u8).collect();
    let mut buf = pattern.clone();
    let mut dirs = [0usize; 2];
    for gap in [64usize, 3000] {
        for n in COPY_SIZES {
            for sa in 0..64 {
                for da in 0..64 {
                    let at = DST + gap + da;
                    let (lo, hi) = buf.split_at_mut(DST);
                    let Some(m) = selftest::copy_avx2(&mut hi[gap + da..gap + da + n], &lo[sa..sa + n]) else {
                        assert!(!liella_boundary::cpu::avx2(), "the AVX2 copy refused on an AVX2 CPU");
                        return;
                    };
                    assert_eq!(m, n);
                    assert!(buf[at..at + n] == pattern[sa..sa + n], "avx2 copy {n}, src +{sa}, dst +{gap}+{da}");
                    assert!(buf[..at] == pattern[..at] && buf[at + n..] == pattern[at + n..], "avx2 copy {n}, src +{sa}, dst +{gap}+{da}: outside written");
                    buf[at..at + n].copy_from_slice(&pattern[at..at + n]);
                    let d = (DST + gap + da - sa) & 4095;
                    dirs[usize::from((1..2048).contains(&d))] += 1;
                }
            }
        }
    }
    assert!(dirs[0] > 0 && dirs[1] > 0, "both directions of the routine reached: {dirs:?}");
}

/// A mapping of `pages` read-write pages between two unmapped ones: (the three regions, the
/// first read-write byte's address).
fn fenced(pages: usize) -> ([liella_kernel::mem::Region; 3], usize) {
    let r = CSys.mmap(0, (pages + 2) * PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, None, 0).unwrap();
    let (mut lo, rest) = r.split_at(PAGE).ok().unwrap();
    let (mid, mut hi) = rest.split_at(pages * PAGE).ok().unwrap();
    CSys.mprotect(&mut lo, PROT_NONE).unwrap();
    CSys.mprotect(&mut hi, PROT_NONE).unwrap();
    let base = mid.addr();
    ([lo, mid, hi], base)
}

#[test]
fn memcpy_on_every_path_stays_inside_both_regions_at_every_alignment_pair() {
    // Clauses: s4_1_c_1, s4_1_g_0, s4_2_c_1 (D-4.14, PJ-7). Source and destination touch an
    // unmapped page (a read or write past either end faults): the source at the start of its
    // mapping and the destination at the end of its own, then the other way round, at every
    // alignment pair of a 64-byte window. Each path the dispatch can take: the default (AVX2
    // where present), without AVX2 (`rep movsb` where ERMS, else the loops), and the loops
    // alone. The destination's distance from the source modulo 4 KiB varies with the size and
    // the offsets, so both copy directions are reached (counted).
    use liella::config::Tunables;
    let _g = level();
    let pages = 5;
    let top = pages * PAGE;
    let (sm, sb) = fenced(pages);
    let (dm, db) = fenced(pages);
    let pattern: Vec<u8> = (0..top).map(|i| (i * 31 + i / 251) as u8).collect();
    let clean = vec![0xEEu8; top];
    s::memcpy(sb as *mut c_void, pattern.as_ptr().cast(), top);
    s::memcpy(db as *mut c_void, clean.as_ptr().cast(), top);
    assert_eq!(s::memcmp(sb as *const c_void, pattern.as_ptr().cast(), top), 0);
    let configs = [
        Tunables::default(),
        Tunables { avx2: Some(false), ..Tunables::default() },
        Tunables { avx2: Some(false), erms: Some(false), ..Tunables::default() },
    ];
    let mut dirs = [0usize; 2];
    for (c, t) in configs.iter().enumerate() {
        liella_boundary::cpu::init_with(t);
        for n in COPY_SIZES.iter().copied().chain([16_383, 16_384, 16_385]) {
            // Every alignment pair below 16 KiB; at 16 KiB a few (they share the code paths).
            let w = if n < 16_384 { 64 } else { 3 };
            for sa in 0..w {
                for da in 0..w {
                    for flip in [false, true] {
                        let (s_at, d_at) = if flip { (sb + top - n - sa, db + da) } else { (sb + sa, db + top - n - da) };
                        let r = s::memcpy(d_at as *mut c_void, s_at as *const c_void, n);
                        assert_eq!(r as usize, d_at);
                        let what = format!("memcpy {n}, src +{sa}, dst +{da}, flip {flip}, config {c}");
                        assert_eq!(s::memcmp(d_at as *const c_void, s_at as *const c_void, n), 0, "{what}");
                        let (below, above) = (d_at - db, db + top - d_at - n);
                        assert!(s::memcmp(db as *const c_void, clean.as_ptr().cast(), below) == 0
                            && s::memcmp((d_at + n) as *const c_void, clean.as_ptr().cast(), above) == 0, "{what}: outside written");
                        s::memset(d_at as *mut c_void, 0xEE, n);
                        let d = d_at.wrapping_sub(s_at) & 4095;
                        dirs[usize::from((1..2048).contains(&d))] += 1;
                    }
                }
            }
        }
    }
    assert!(dirs[0] > 0 && dirs[1] > 0, "both copy directions reached: {dirs:?}");
    // memmove with overlap at the same sizes (from 129 bytes it takes the AVX2 copy, MEM-BULK).
    for n in COPY_SIZES {
        for shift in [1usize, 31, 32, 33, 64, 200] {
            let mut v: Vec<u8> = (0..n + 256).map(|i| (i * 11 + 1) as u8).collect();
            let mut w = v.clone();
            s::memmove(v[shift..].as_mut_ptr().cast(), v.as_ptr().cast(), n);
            w.copy_within(0..n, shift);
            assert_eq!(v, w, "memmove up {n} by {shift}");
            s::memmove(v.as_mut_ptr().cast(), v[shift..].as_ptr().cast(), n);
            w.copy_within(shift..shift + n, 0);
            assert_eq!(v, w, "memmove down {n} by {shift}");
        }
    }
    liella_boundary::cpu::init();
    for r in sm.into_iter().chain(dm) {
        CSys.munmap(r).unwrap();
    }
}

/// `memchr` for `#` in `buf[align..align + n]`: the offset found.
fn memchr_at(buf: &mut [u8], align: usize, n: usize) -> Option<usize> {
    let p = at(buf, align).cast::<c_void>();
    let r = s::memchr(p, c_int::from(b'#'), n);
    if r.is_null() { None } else { Some(r as usize - p as usize) }
}

const BOUNDED_LENGTHS: [usize; 7] = [383, 384, 385, 511, 512, 513, 640];

#[test]
fn memchr_finds_the_first_match_inside_the_bound_at_every_alignment_length_and_position() {
    // Clauses: s4_5_c_1 (D-4.14, P7).
    let _g = level();
    // D-4.21's rescan heals an answer that is EARLY (the byte there does not match, so the rest is
    // searched again): without this count an early-by-one scanner passes every assertion (STR-K3).
    let rescans = selftest::rescans();
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        let mut buf = vec![0u8; 64 + 700 + 64];
        for align in 0..64 {
            for n in (0..=300).chain(BOUNDED_LENGTHS) {
                // The target byte everywhere outside the region: a load outside it would match.
                buf.fill(b'#');
                buf[align..align + n].fill(b'.');
                assert_eq!(memchr_at(&mut buf, align, n), None, "none, avx2={avx2}, align {align}, n {n}");
                // Matches planted from the end down: every byte after `pos` matches as well,
                // so only the first may be reported.
                for pos in (0..n).rev() {
                    buf[align + pos] = b'#';
                    assert_eq!(memchr_at(&mut buf, align, n), Some(pos), "avx2={avx2}, align {align}, n {n}, pos {pos}");
                }
            }
        }
        // Every byte value (the broadcast), in each part of a 200-byte region.
        for c in 0..=255u8 {
            buf.fill(c.wrapping_add(1));
            let p = at(&mut buf, 3).cast::<c_void>();
            for pos in [0usize, 15, 16, 31, 32, 63, 64, 127, 128, 199] {
                s::memset(p.wrapping_byte_add(pos), c_int::from(c), 1);
                assert_eq!(s::memchr(p, c_int::from(c), 200) as usize - p as usize, pos, "byte {c}, pos {pos}");
                assert!(s::memchr(p, c_int::from(c), pos).is_null(), "byte {c} before {pos}");
                s::memset(p.wrapping_byte_add(pos), c_int::from(c.wrapping_add(1)), 1);
            }
            // Only the low byte of `c` counts.
            s::memset(p.wrapping_byte_add(50), c_int::from(c), 1);
            assert_eq!(s::memchr(p, c_int::from(c) | 0x7F00, 200) as usize - p as usize, 50, "byte {c} with high bits");
        }
        // Large regions: matches near the end, and none (the byte just past the end matches).
        let mut big = vec![b'.'; 65_536 + 200];
        for n in [4096usize, 65_536, 65_539] {
            for off in [0usize, 1, 31] {
                for back in [1usize, 2, 17, 33, 129] {
                    big[off + n - back] = b'#';
                    assert_eq!(memchr_at(&mut big, off, n), Some(n - back), "n {n}, off {off}, back {back}");
                    big[off + n - back] = b'.';
                }
                big[off + n] = b'#';
                assert_eq!(memchr_at(&mut big, off, n), None, "n {n}, off {off}");
                big[off + n] = b'.';
            }
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
    assert_eq!(selftest::rescans(), rescans, "a single-threaded memchr sweep took a rescan");
}

/// `memchr` for the byte values whose broadcast or sign matters (0, 0x80, 0xFF): every length to
/// 256 at every alignment of a 64-byte window, with no match, a single match at each position
/// (first, last and every one between) and every byte matching; the byte outside the region
/// matches, so a load past either end would be reported. No call may take a rescan (STR-K3).
#[test]
fn memchr_finds_each_byte_value_at_every_position_without_a_rescan() {
    // Clauses: s4_5_c_1 (D-4.14, D-4.21, P7).
    let _g = level();
    let rescans = selftest::rescans();
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        let mut buf = vec![0u8; 64 + 256 + 64];
        for c in [0u8, 0x80, 0xFF] {
            let other = c ^ 1;
            for align in 0..64 {
                for n in 0..=256 {
                    buf.fill(c);
                    buf[align..align + n].fill(other);
                    let p = at(&mut buf, align).cast::<c_void>();
                    assert!(s::memchr(p, c_int::from(c), n).is_null(), "none, byte {c}, avx2={avx2}, align {align}, n {n}");
                    for pos in 0..n {
                        buf[align + pos] = c;
                        let r = s::memchr(p, c_int::from(c), n);
                        assert_eq!(r as usize, p as usize + pos, "byte {c}, avx2={avx2}, align {align}, n {n}, pos {pos}");
                        buf[align + pos] = other;
                    }
                    buf[align..align + n].fill(c);
                    if n > 0 {
                        assert_eq!(s::memchr(p, c_int::from(c), n), p, "all, byte {c}, avx2={avx2}, align {align}, n {n}");
                    }
                }
            }
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
    assert_eq!(selftest::rescans(), rescans, "a single-threaded memchr sweep took a rescan");
}

/// `memcmp`, `bcmp` and `__memcmpeq` on `a[ia..ia + n]` and `b[ib..ib + n]`.
fn cmp3(a: &mut [u8], ia: usize, b: &mut [u8], ib: usize, n: usize) -> (c_int, c_int, c_int) {
    let x = at(a, ia).cast::<c_void>().cast_const();
    let y = at(b, ib).cast::<c_void>().cast_const();
    (s::memcmp(x, y, n), s::bcmp(x, y, n), s::__memcmpeq(x, y, n))
}

#[test]
fn memcmp_finds_the_first_difference_at_every_alignment_length_and_position() {
    // Clauses: s4_3_c_1, s4_3_c_2, s4_3_c_3 (D-4.14, P7).
    let _g = level();
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        let mut a = vec![0u8; 64 + 700 + 64];
        let mut b = a.clone();
        for ia in 0..32 {
            for ib in [0usize, 5, 31, 32] {
                for n in (0..=300).chain(BOUNDED_LENGTHS) {
                    // 0x7F against 0x80 checks the unsigned order; `first` below `later`.
                    for (first, later) in [((0x7Fu8, 0x80u8), (0x80u8, 0x7Fu8)), ((0x80, 0x7F), (0x7F, 0x80))] {
                        // Equal inside the region, different everywhere outside it.
                        a.fill(1);
                        b.fill(2);
                        for i in 0..n {
                            a[ia + i] = (i * 13) as u8;
                            b[ib + i] = (i * 13) as u8;
                        }
                        assert_eq!(cmp3(&mut a, ia, &mut b, ib, n), (0, 0, 0), "equal, avx2={avx2}, ia {ia}, ib {ib}, n {n}");
                        // Differences planted from the end down: every byte after `pos` differs
                        // in the opposite direction, so only the first may decide.
                        for pos in (0..n).rev() {
                            (a[ia + pos], b[ib + pos]) = first;
                            let (m, bc, eq) = cmp3(&mut a, ia, &mut b, ib, n);
                            let want = first.0.cmp(&first.1) as c_int;
                            assert_eq!(m.signum(), want, "memcmp, avx2={avx2}, ia {ia}, ib {ib}, n {n}, pos {pos}");
                            assert!(bc == 1 && eq == 1, "bcmp, avx2={avx2}, ia {ia}, ib {ib}, n {n}, pos {pos}");
                            (a[ia + pos], b[ib + pos]) = later;
                        }
                    }
                }
            }
        }
        // Every pair of byte values, in each part of a 200-byte region.
        let mut a = vec![9u8; 256];
        let mut b = a.clone();
        for x in 0..=255u8 {
            for y in 0..=255u8 {
                for pos in [3usize, 40, 199] {
                    (a[pos], b[pos]) = (x, y);
                    let (m, bc, eq) = cmp3(&mut a, 0, &mut b, 0, 200);
                    assert_eq!(m.signum(), x.cmp(&y) as c_int, "{x} against {y} at {pos}");
                    assert_eq!((bc != 0, eq != 0), (x != y, x != y), "{x} against {y} at {pos}");
                    (a[pos], b[pos]) = (9, 9);
                }
            }
        }
        // Large regions: a difference near the end.
        let mut a = vec![7u8; 65_536 + 200];
        let mut b = a.clone();
        for n in [4096usize, 65_536, 65_539] {
            for (ia, ib) in [(0usize, 0usize), (1, 0), (31, 2)] {
                assert_eq!(cmp3(&mut a, ia, &mut b, ib, n).0, 0);
                for back in [1usize, 2, 17, 33, 129] {
                    b[ib + n - back] = 8;
                    assert!(cmp3(&mut a, ia, &mut b, ib, n).0 < 0, "n {n}, ia {ia}, ib {ib}, back {back}");
                    b[ib + n - back] = 7;
                }
            }
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
}

/// C17 7.24.5.1p2: `memchr` behaves as if it read the bytes in order and stopped at the first
/// match, so a bound reaching past the readable memory is fine when the byte comes first. The
/// match is planted at every distance before an unmapped page, the region starting at every
/// alignment before it, with bounds ending just past the page, far past it, and at the largest
/// value a region may have.
#[test]
fn memchr_stops_at_the_match_before_an_unmapped_page() {
    // Clauses: s4_5_c_1 (D-4.14, P7; validation T7).
    let _g = level();
    let (lo, hi, base) = guarded();
    let end = base + PAGE;
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        s::memset(base as *mut c_void, c_int::from(b'.'), PAGE);
        for back in 1..=300usize {
            let m = end - back;
            s::memset(m as *mut c_void, c_int::from(b'#'), 1);
            for before in (0..=300usize).step_by(7).chain([0, 1, 15, 16, 31, 32, 63, 64, 127, 128]) {
                let p = (m - before) as *const c_void;
                let to_end = before + back;
                for n in [to_end + 1, to_end + 31, to_end + 4096, (isize::MAX as usize) - p as usize] {
                    assert_eq!(s::memchr(p, c_int::from(b'#'), n) as usize, m, "avx2={avx2}, back {back}, before {before}, n {n}");
                }
            }
            s::memset(m as *mut c_void, c_int::from(b'.'), 1);
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
    CSys.munmap(hi).unwrap();
    CSys.munmap(lo).unwrap();
}

#[test]
fn bounded_scanners_never_read_outside_the_region() {
    // Clauses: s4_5_c_1, s4_3_c_1, s4_3_c_2 (D-4.14, P7).
    let _g = level();
    // A region ending at the last byte before an unmapped page, and one starting at the first
    // byte after an unmapped page: any load outside [s, s + n) faults.
    let (lo, hi, base) = guarded();
    let r = CSys.mmap(0, 2 * PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, None, 0).unwrap();
    let (mut lo2, hi2) = r.split_at(PAGE).ok().unwrap();
    CSys.mprotect(&mut lo2, PROT_NONE).unwrap();
    let start = hi2.addr() as *const c_void;
    let end = base + PAGE;
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        s::memset(base as *mut c_void, c_int::from(b'.'), PAGE);
        s::memset(start.cast_mut(), c_int::from(b'.'), PAGE);
        for n in 0..=600usize {
            let p = (end - n) as *const c_void;
            assert!(s::memchr(p, c_int::from(b'#'), n).is_null(), "memchr to the page end, avx2={avx2}, n {n}");
            assert!(s::memchr(start, c_int::from(b'#'), n).is_null(), "memchr from the page start, avx2={avx2}, n {n}");
            assert_eq!(s::memcmp(p, start, n), 0, "memcmp, avx2={avx2}, n {n}");
            assert_eq!(s::memcmp(start, p, n), 0, "memcmp, avx2={avx2}, n {n}");
            assert_eq!(s::bcmp(p, start, n), 0, "bcmp, avx2={avx2}, n {n}");
            if n > 0 {
                s::memset((end - 1) as *mut c_void, c_int::from(b'#'), 1);
                assert_eq!(s::memchr(p, c_int::from(b'#'), n) as usize, end - 1, "last byte, avx2={avx2}, n {n}");
                assert!(s::memcmp(p, start, n) < 0, "last byte differs (`#` sorts below `.`), avx2={avx2}, n {n}");
                s::memset((end - 1) as *mut c_void, c_int::from(b'.'), 1);
            }
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
    CSys.munmap(hi2).unwrap();
    CSys.munmap(lo2).unwrap();
    CSys.munmap(hi).unwrap();
    CSys.munmap(lo).unwrap();
}

/// The sign of `strcmp` on two buffers' strings at `pa` and `pb`.
fn cmp_at(pa: usize, pb: usize) -> c_int {
    s::strcmp(pa as *const c_char, pb as *const c_char).signum()
}

#[test]
fn strcmp_scanner_finds_the_first_difference_at_every_alignment_pair() {
    // Clauses: s4_3_c_1 (D-4.14, P7; PJ-8: the one-pass C-layer compare, SSE2 on both levels). Both scan levels (D-4.29).
    let _g = level();
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        strcmp_scanner_finds_the_first_difference_at_every_alignment_pair_on_level();
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
}

fn strcmp_scanner_finds_the_first_difference_at_every_alignment_pair_on_level() {
    // Clauses: s4_3_c_1 (D-4.14, P7; PJ-8: the one-pass C-layer compare, SSE2 on both levels).
    // Every pair of alignments in a 32-byte window, every length up to 80 (the 64-byte step,
    // the 16-byte step and the byte step), every position of the first difference, with
    // unsigned bytes above 0x7F, a shorter second string, and equal decoys after the end.
    let mut a = vec![0u8; 32 + 80 + 64];
    let mut b = a.clone();
    for aa in 0..32 {
        for ba in 0..32 {
            for len in 0..80 {
                for (buf, al) in [(&mut a, aa), (&mut b, ba)] {
                    buf.fill(b'y');
                    for i in 0..len {
                        buf[al + i] = 1 + ((i * 37) % 255) as u8;
                    }
                    buf[al + len] = 0;
                }
                let (pa, pb) = (a.as_ptr() as usize + aa, b.as_ptr() as usize + ba);
                assert_eq!(cmp_at(pa, pb), 0, "equal, align {aa}/{ba}, len {len}");
                for d in 0..len {
                    let keep = (a[aa + d], b[ba + d]);
                    (a[aa + d], b[ba + d]) = (0x7F, 0x80);
                    assert_eq!(cmp_at(pa, pb), -1, "0x7F < 0x80 at {d}, align {aa}/{ba}, len {len}");
                    assert_eq!(cmp_at(pb, pa), 1, "swapped, at {d}, align {aa}/{ba}, len {len}");
                    b[ba + d] = 0;
                    assert_eq!(cmp_at(pa, pb), 1, "b ends at {d}, align {aa}/{ba}, len {len}");
                    (a[aa + d], b[ba + d]) = keep;
                }
            }
        }
    }
}

#[test]
fn strcmp_kernel_folded_groups_find_every_difference_up_to_length_300() {
    // Clauses: s4_3_c_1 (D-4.29, STRCOLL-1: the kernel returns the order and tests a 64-byte Both scan levels (D-4.29).
    let _g = level();
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        strcmp_kernel_folded_groups_find_every_difference_up_to_length_300_on_level();
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
}

fn strcmp_kernel_folded_groups_find_every_difference_up_to_length_300_on_level() {
    // Clauses: s4_3_c_1 (D-4.29, STRCOLL-1: the kernel returns the order and tests a 64-byte
    // group once, with the four blocks' masks folded). Lengths past several groups, at
    // alignments around the block and group edges, every position of the first difference
    // (each block of a group, each lane), unsigned bytes above 0x7F, a terminator in b first.
    let mut a = vec![0u8; 64 + 300 + 64];
    let mut b = a.clone();
    let aligns = [0usize, 1, 7, 15, 16, 17, 31, 48, 63];
    for &aa in &aligns {
        for &ba in &aligns {
            for len in 0..300 {
                for (buf, al) in [(&mut a, aa), (&mut b, ba)] {
                    buf.fill(b'z');
                    for i in 0..len {
                        buf[al + i] = 1 + ((i * 53) % 255) as u8;
                    }
                    buf[al + len] = 0;
                }
                let (pa, pb) = (a.as_ptr() as usize + aa, b.as_ptr() as usize + ba);
                assert_eq!(cmp_at(pa, pb), 0, "equal, align {aa}/{ba}, len {len}");
                for d in 0..len {
                    let keep = (a[aa + d], b[ba + d]);
                    (a[aa + d], b[ba + d]) = (0x80, 0x7F);
                    assert_eq!(cmp_at(pa, pb), 1, "0x80 > 0x7F at {d}, align {aa}/{ba}, len {len}");
                    assert_eq!(cmp_at(pb, pa), -1, "swapped, at {d}, align {aa}/{ba}, len {len}");
                    a[aa + d] = 0;
                    assert_eq!(cmp_at(pa, pb), -1, "a ends at {d}, align {aa}/{ba}, len {len}");
                    (a[aa + d], b[ba + d]) = keep;
                }
            }
        }
    }
}

#[test]
fn strcmp_scanner_never_reads_past_the_pages_holding_the_terminators() {
    // Clauses: s4_3_c_1 (D-4.14, P7; PJ-8). Both strings end on the last byte of a page whose Both scan levels (D-4.29).
    let _g = level();
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        strcmp_scanner_never_reads_past_the_pages_holding_the_terminators_on_level();
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
}

fn strcmp_scanner_never_reads_past_the_pages_holding_the_terminators_on_level() {
    // Clauses: s4_3_c_1 (D-4.14, P7; PJ-8). Both strings end on the last byte of a page whose
    // next page is unmapped, at every pair of lengths (so every pair of page offsets), equal,
    // differing in their last byte, or one a prefix of the other ending at its page end.
    let (lo, hi, base) = guarded();
    let (lo2, hi2, base2) = guarded();
    s::memset(base as *mut c_void, c_int::from(b'a'), PAGE - 1);
    s::memset((base + PAGE - 1) as *mut c_void, 0, 1);
    s::memset(base2 as *mut c_void, c_int::from(b'a'), PAGE - 1);
    s::memset((base2 + PAGE - 1) as *mut c_void, 0, 1);
    let end = |b: usize, len: usize| b + PAGE - 1 - len;
    for la in 0..200usize {
        for lb in [la.saturating_sub(17), la.saturating_sub(1), la, la + 1, la + 16, la + 64] {
            let want = (la.cmp(&lb)) as c_int;
            assert_eq!(cmp_at(end(base, la), end(base2, lb)), want, "lengths {la}/{lb}");
            assert_eq!(cmp_at(end(base2, lb), end(base, la)), -want, "lengths {lb}/{la}");
        }
        if la > 0 {
            s::memset((base + PAGE - 2) as *mut c_void, c_int::from(b'b'), 1);
            assert_eq!(cmp_at(end(base, la), end(base2, la)), 1, "last byte differs, length {la}");
            s::memset((base + PAGE - 2) as *mut c_void, c_int::from(b'a'), 1);
        }
        // A string in the middle of a page against one ending at a page end.
        assert_eq!(cmp_at(end(base, la), base2 + 1000 - la.min(900)), (la.cmp(&(PAGE - 1 - 1000 + la.min(900)))) as c_int, "mid-page, length {la}");
    }
    CSys.munmap(hi2).unwrap();
    CSys.munmap(lo2).unwrap();
    CSys.munmap(hi).unwrap();
    CSys.munmap(lo).unwrap();
}

#[test]
fn strcmp_scanner_crosses_page_boundaries_inside_the_strings() {
    // Clauses: s4_3_c_1 (D-4.14, P7; PJ-8). Strings of 150 bytes that start 1..80 bytes before Both scan levels (D-4.29).
    let _g = level();
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        strcmp_scanner_crosses_page_boundaries_inside_the_strings_on_level();
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
}

fn strcmp_scanner_crosses_page_boundaries_inside_the_strings_on_level() {
    // Clauses: s4_3_c_1 (D-4.14, P7; PJ-8). Strings of 150 bytes that start 1..80 bytes before
    // a page boundary (the step switches to 16 bytes and to single bytes near it), at
    // different offsets in the two buffers, equal or differing just before, at or after it.
    let mut a = vec![b'q'; 3 * PAGE];
    let mut b = a.clone();
    let ia = PAGE - (a.as_ptr() as usize % PAGE);
    let ib = PAGE - (b.as_ptr() as usize % PAGE);
    for oa in 1..80usize {
        for ob in [1usize, 15, 16, 17, 63, 64, 65, 79, oa] {
            let (sa, sb) = (ia - oa, ib - ob);
            a[sa + 150] = 0;
            b[sb + 150] = 0;
            let (pa, pb) = (a.as_ptr() as usize + sa, b.as_ptr() as usize + sb);
            assert_eq!(cmp_at(pa, pb), 0, "equal, offsets {oa}/{ob}");
            for d in [oa.saturating_sub(1), oa, oa + 1, ob.saturating_sub(1), ob, ob + 1, 149] {
                b[sb + d] = b'r';
                assert_eq!(cmp_at(pa, pb), -1, "differ at {d}, offsets {oa}/{ob}");
                b[sb + d] = b'q';
            }
            a[sa + 150] = b'q';
            b[sb + 150] = b'q';
        }
    }
}

/// The return labels of `liella_machine_strcmp` (`.Lstrcmp_ret_<path>` on the SSE2 level,
/// `.Lstrcmp_v_ret_<path>` on AVX2), read from the kernel's source. Each `ret` must sit in a block
/// opened by such a label, so a new return path cannot appear without a name.
fn strcmp_return_labels() -> Vec<String> {
    let src = include_str!("../../../csys/src/machine/scan.S");
    let start = src.find("\nliella_machine_strcmp:\n");
    assert!(start.is_some(), "no strcmp kernel in scan.S");
    let start = start.unwrap_or(0);
    let end = src[start..].find(".size liella_machine_strcmp,");
    assert!(end.is_some(), "no .size line for the strcmp kernel in scan.S");
    let end = end.unwrap_or(0) + start;
    let mut labels = Vec::new();
    let mut last_label = String::new();
    for line in src[start..end].lines() {
        let t = line.trim();
        if let Some(l) = t.strip_suffix(':').filter(|l| l.starts_with(".L")) {
            last_label = l.to_string();
            if l.starts_with(".Lstrcmp_ret_") || l.starts_with(".Lstrcmp_v_ret_") {
                labels.push(l.to_string());
            }
        } else if t == "ret" {
            assert!(
                last_label.starts_with(".Lstrcmp_ret_") || last_label.starts_with(".Lstrcmp_v_ret_"),
                "a ret in liella_machine_strcmp after {last_label}, not after a .Lstrcmp_[v_]ret_ label"
            );
        }
    }
    labels
}

#[test]
fn strcmp_leaves_no_upper_register_state_behind_on_any_return_path() {
    // Clauses: s4_3_c_1 (D-4.29, D-4.38: a caller that enters with clean upper halves leaves clean;
    // vzeroupper only where a 256-bit instruction ran). One group of cases per return label, the
    // labels read from scan.S, so a return path without a case fails here.
    let _g = level();
    if !selftest::set_scan_avx2(true) {
        selftest::set_scan_avx2(false);
        return; // no AVX2 on this CPU: no upper halves to read (and none the SSE2 path could dirty)
    }
    assert_ne!(selftest::dirty_upper_probe(), 0, "the check itself sees a dirty register");
    // (label suffix, route, a's page offset, b's page offset, length, difference position or -1).
    // Routes: head (16-byte head), blocks (first 64 bytes), group (64-byte groups), page step (one
    // block stepped back to end at a page end), byte (single bytes while i < k).
    let cases: &[(&str, &str, usize, usize, usize, i32)] = &[
        ("head_equal", "head", 0, 0, 5, -1),
        ("head_equal", "head", 3, 5, 15, -1),
        ("head_differ", "head", 0, 0, 20, 3),
        ("head_differ", "head", 3, 5, 15, 0),
        ("head_equal", "head, second block", 3, 5, 50, -1),
        ("head_differ", "head, second block", 0, 0, 100, 40),
        ("differ", "blocks after the head", 0, 0, 100, 70),
        ("equal", "blocks", 0, 0, 40, -1),
        ("equal", "group", 0, 32, 300, -1),
        ("equal", "page step", 4040, 100, 100, -1),
        ("equal", "byte", 4090, 4092, 3, -1),
        ("equal", "bytes, then blocks", 4090, 7, 40, -1),
        ("differ", "blocks", 0, 0, 40, 20),
        ("differ", "group", 0, 32, 300, 250),
        ("differ", "page step", 4040, 100, 100, 60),
        ("differ", "byte", 4090, 4092, 3, 1),
        ("differ", "bytes, then blocks", 4090, 7, 40, 30),
    ];
    let labels = strcmp_return_labels();
    for l in &labels {
        let suffix = l.trim_start_matches(".Lstrcmp_v_ret_").trim_start_matches(".Lstrcmp_ret_");
        assert!(cases.iter().any(|c| c.0 == suffix), "return label {l} has no case in this test");
    }
    assert!(labels.iter().any(|l| l.starts_with(".Lstrcmp_v_ret_")), "no AVX2 return labels found in scan.S");
    // Three pages; the strings start at page offsets of the middle one (the next page is mapped,
    // the kernel's own page tests keep every read inside the page holding a string byte).
    let mut a = vec![b'x'; 4 * 4096];
    let mut b = a.clone();
    let pa = (4096 - a.as_ptr().addr() % 4096) % 4096 + 4096;
    let pb = (4096 - b.as_ptr().addr() % 4096) % 4096 + 4096;
    for avx2 in [true, false] {
        assert!(selftest::set_scan_avx2(avx2));
        for &(label, route, oa, ob, len, d) in cases {
            a.fill(b'x');
            b.fill(b'x');
            a[pa + oa + len] = 0;
            b[pb + ob + len] = 0;
            let want = if d >= 0 {
                b[pb + ob + d as usize] = b'y';
                -1
            } else {
                0
            };
            let (got, upper) = selftest::strcmp_clean_upper(&a[pa + oa..], &b[pb + ob..]);
            assert_eq!(got, want, "avx2 {avx2}, {label} via {route}: offsets {oa}/{ob}, length {len}, difference {d}");
            assert_eq!(upper, 0, "upper state left behind: avx2 {avx2}, {label} via {route}, offsets {oa}/{ob}");
            // The sign the other way round: b's byte is the smaller one.
            let (got, upper) = selftest::strcmp_clean_upper(&b[pb + ob..], &a[pa + oa..]);
            assert_eq!(got, -want, "avx2 {avx2}, {label} via {route} swapped");
            assert_eq!(upper, 0, "upper state left behind: avx2 {avx2}, {label} via {route} swapped");
        }
        // Every short length and difference position around the head (0..160 B: every head block and
        // the steps after it, three offset pairs).
        for &(oa, ob) in &[(0, 0), (3, 5), (4090, 4092)] {
            for len in 0..160usize {
                for d in [-1i32, 0, (len / 2) as i32, len as i32 - 1] {
                    a.fill(b'x');
                    b.fill(b'x');
                    a[pa + oa + len] = 0;
                    b[pb + ob + len] = 0;
                    let want = if d >= 0 && (d as usize) < len {
                        b[pb + ob + d as usize] = b'y';
                        -1
                    } else {
                        0
                    };
                    let (got, upper) = selftest::strcmp_clean_upper(&a[pa + oa..], &b[pb + ob..]);
                    assert_eq!(got, want, "short: avx2 {avx2}, offsets {oa}/{ob}, length {len}, difference {d}");
                    assert_eq!(upper, 0, "upper state left behind: short, avx2 {avx2}, offsets {oa}/{ob}, length {len}, difference {d}");
                }
            }
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
}

/// The byte-loop oracle: the index of the first byte equal to `c` in `buf[from..]`, or `None`.
fn first(buf: &[u8], from: usize, c: u8) -> Option<usize> {
    buf[from..].iter().position(|&b| b == c)
}

#[test]
fn terminator_scanners_match_a_byte_loop_for_every_length_to_512_and_alignment() {
    // Clauses: s4_8_c_0, s4_5_c_3 (D-4.14, STR-K1). Every byte value 1..=255 appears in the
    // strings (the SSE2 loop folds blocks with an unsigned minimum: high bytes must not hide a
    // zero), zero bytes sit before the start (the head must mask them), and the bounds cover
    // every group edge around the terminator.
    let _g = level();
    let rescans = selftest::rescans();
    const N: usize = 512;
    let mut buf = vec![0u8; 64 + N + 128];
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        for align in 0..64 {
            for len in 0..=N {
                buf.fill(0);
                for (i, b) in buf[align..align + len].iter_mut().enumerate() {
                    *b = ((i * 37 + align * 11) % 255 + 1) as u8;
                }
                for (i, b) in buf.iter_mut().enumerate().skip(align + len + 1) {
                    *b = (i % 255 + 1) as u8;
                }
                let want = first(&buf, align, 0).unwrap();
                assert_eq!(want, len);
                let p = at(&mut buf, align).cast::<c_char>();
                assert_eq!(s::strlen(p), want, "strlen, avx2={avx2}, align {align}, len {len}");
                for max in [0, 1, len.saturating_sub(64), len.saturating_sub(1), len, len + 1, len + 64, usize::MAX - align] {
                    assert_eq!(s::strnlen(p, max), want.min(max), "strnlen, avx2={avx2}, align {align}, len {len}, max {max}");
                }
                // rawmemchr for a byte that first occurs at len (the terminator replaced).
                let c = 0xFFu8;
                for b in &mut buf[align..align + len] {
                    if *b == c {
                        *b = 0xFE;
                    }
                }
                buf[align + len] = c;
                let r = s::rawmemchr(at(&mut buf, align).cast::<c_void>(), c_int::from(c));
                assert_eq!(r as usize - p as usize, first(&buf, align, c).unwrap(), "rawmemchr, avx2={avx2}, align {align}, len {len}");
            }
        }
    }
    // No writer runs here: a rescan would mean a scanner answered wrong (D-4.21, STR-K1).
    assert_eq!(selftest::rescans(), rescans, "a single-threaded sweep took a rescan");
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
}

#[test]
fn terminator_scanners_stop_at_a_page_end_for_every_length_to_512_and_alignment() {
    // Clauses: s4_8_c_0, s4_5_c_3 (D-4.14, STR-K1). The terminator (or the bound, or the
    // searched byte) is the page's last byte and the next page is inaccessible; start
    // offsets PAGE - 1 - len for len 0..=512 + 63 give every alignment modulo 64 at least eight
    // times, short strings included.
    let _g = level();
    let rescans = selftest::rescans();
    let (lo, hi, base) = guarded();
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        for len in 0..=512 + 63usize {
            let start = base + PAGE - 1 - len;
            for i in 0..len {
                s::memset((start + i) as *mut c_void, c_int::try_from((i * 37) % 255 + 1).unwrap(), 1);
            }
            s::memset((base + PAGE - 1) as *mut c_void, 0, 1);
            let p = start as *const c_char;
            assert_eq!(s::strlen(p), len, "strlen, avx2={avx2}, len {len}");
            assert_eq!(s::strnlen(p, usize::MAX), len, "strnlen max, avx2={avx2}, len {len}");
            assert_eq!(s::strnlen(p, len + 1), len, "strnlen len+1, avx2={avx2}, len {len}");
            s::memset((base + PAGE - 1) as *mut c_void, c_int::from(b'a'), 1);
            assert_eq!(s::strnlen(p, len + 1), len + 1, "strnlen to the page end, avx2={avx2}, len {len}");
            s::memset((base + PAGE - 1) as *mut c_void, 0xFF, 1);
            for i in 0..len {
                s::memset((start + i) as *mut c_void, c_int::from(b'b'), 1);
            }
            let r = s::rawmemchr(p.cast(), 0xFF);
            assert_eq!(r as usize, base + PAGE - 1, "rawmemchr, avx2={avx2}, len {len}");
        }
    }
    // No writer runs here: a rescan would mean a scanner answered wrong (D-4.21, STR-K1).
    assert_eq!(selftest::rescans(), rescans, "a single-threaded sweep took a rescan");
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
    CSys.munmap(hi).unwrap();
    CSys.munmap(lo).unwrap();
}

/// The index `liella_machine_strdiff` must return: the first index at which the strings differ
/// or `a`'s byte is the terminator.
fn strdiff_ref(a: &[u8], b: &[u8]) -> usize {
    (0..).find(|&i| a[i] != b[i] || a[i] == 0).unwrap()
}

#[test]
fn strdiff_finds_the_first_difference_at_every_alignment_pair_length_and_position() {
    // Clauses: s4_4_c_0 (D-4.37: strcoll's shared-prefix kernel; D-4.14 page rule).
    let (mut a, mut b) = (vec![0u8; 256], vec![0u8; 256]);
    for ia in 0..16usize {
        for ib in 0..16usize {
            for len in 0..160usize {
                for d in 0..=len {
                    for (kind, other) in [("byte", b'Z'), ("high", 0xe9u8), ("end", 0u8)] {
                        a.fill(0x55);
                        b.fill(0x55);
                        for k in 0..len {
                            let v = b'a' + u8::try_from(k % 26).unwrap();
                            a[ia + k] = v;
                            b[ib + k] = v;
                        }
                        a[ia + len] = 0;
                        b[ib + len] = 0;
                        if d < len {
                            b[ib + d] = other;
                        }
                        let want = strdiff_ref(&a[ia..], &b[ib..]);
                        assert_eq!(selftest::strdiff(at(&mut a, ia), at(&mut b, ib)), want, "{kind}: align {ia}/{ib}, len {len}, d {d}");
                    }
                }
            }
        }
    }
}

#[test]
fn strdiff_never_reads_past_the_pages_holding_the_terminators() {
    // Clauses: s4_4_c_0 (D-4.37, D-4.14: loads stay in the page of an unended byte).
    let (lo_a, hi_a, base_a) = guarded();
    let (lo_b, hi_b, base_b) = guarded();
    for len in 0..300usize {
        for shift in [0usize, 1, 7, 15, 16, 33] {
            s::memset(base_a as *mut c_void, c_int::from(b'q'), PAGE - 1);
            s::memset(base_b as *mut c_void, c_int::from(b'q'), PAGE - 1);
            s::memset((base_a + PAGE - 1) as *mut c_void, 0, 1);
            s::memset((base_b + PAGE - 1) as *mut c_void, 0, 1);
            // a's terminator is its page's last byte; b's is `shift` bytes earlier in its page.
            let pa = (base_a + PAGE - 1 - len) as *const u8;
            let pb = (base_b + PAGE - 1 - len - shift) as *const u8;
            s::memset((base_b + PAGE - 1 - shift) as *mut c_void, 0, 1);
            assert_eq!(selftest::strdiff(pa, pb), len, "same strings, len {len}, shift {shift}");
            assert_eq!(selftest::strdiff(pb, pa), len, "same strings swapped, len {len}, shift {shift}");
            if len > 0 {
                // The last byte before a's terminator differs.
                s::memset((base_a + PAGE - 2) as *mut c_void, c_int::from(b'r'), 1);
                assert_eq!(selftest::strdiff(pa, pb), len - 1, "last byte differs, len {len}, shift {shift}");
            }
        }
    }
    CSys.munmap(hi_a).unwrap();
    CSys.munmap(lo_a).unwrap();
    CSys.munmap(hi_b).unwrap();
    CSys.munmap(lo_b).unwrap();
}

/// Fill `a` and `b` with a shared word of `len` letters ending in a terminator, `b` differing at
/// `d` (none if `d == len`) and a non-ASCII byte at `h` in `a` (and in `b` too before `d`), then
/// check the kernel's index and its high flag: set iff a byte of `a` up to the index has bit 7.
fn strdiff_high_case(pa: *mut u8, pb: *mut u8, len: usize, d: usize, h: Option<usize>, what: &str) {
    let (mut a, mut b) = (vec![0u8; len + 1], vec![0u8; len + 1]);
    for k in 0..len {
        let v = b'a' + u8::try_from(k % 26).unwrap();
        a[k] = v;
        b[k] = v;
    }
    a[len] = 0;
    b[len] = 0;
    if d < len {
        b[d] = b'Z';
    }
    if let Some(h) = h.filter(|&h| h < len) {
        a[h] = 0xc3;
        if h < d {
            b[h] = 0xc3;
        }
    }
    s::memcpy(pa.cast(), a.as_ptr().cast(), len + 1);
    s::memcpy(pb.cast(), b.as_ptr().cast(), len + 1);
    let want = strdiff_ref(&a, &b);
    let high = a[..=want].iter().any(|&x| x >= 0x80);
    assert_eq!(selftest::strdiff(pa, pb), want, "{what}: index, len {len}, d {d}, h {h:?}");
    assert_eq!(selftest::strdiff_high(pa, pb), high, "{what}: high flag, len {len}, d {d}, h {h:?}");
}

#[test]
fn strdiff_reports_a_non_ascii_byte_up_to_the_index_in_blocks_and_near_page_ends() {
    // Clauses: s4_4_c_0 (D-4.37 as amended by STRCOLL-4: the kernel's second word is strcoll's
    // non-ASCII prefix test; whole blocks, the block holding the index, the bytes near a page end).
    let (mut a, mut b) = (vec![0u8; 128], vec![0u8; 128]);
    for ia in 0..16usize {
        for len in 0..70usize {
            for d in 0..=len {
                for h in [None, Some(0), d.checked_sub(1), Some(d), Some(d + 1), len.checked_sub(1), Some(17), Some(33)] {
                    strdiff_high_case(at(&mut a, ia), at(&mut b, 3), len, d, h, "heap");
                }
            }
        }
    }
    let (lo_a, hi_a, base_a) = guarded();
    let (lo_b, hi_b, base_b) = guarded();
    for len in 0..40usize {
        for d in 0..=len {
            for h in (0..=len).map(Some).chain([None]) {
                // a ends at its page's last byte (the byte steps); b a few bytes before its own.
                let pa = (base_a + PAGE - 1 - len) as *mut u8;
                let pb = (base_b + PAGE - 6 - len) as *mut u8;
                strdiff_high_case(pa, pb, len, d, h, "page end");
            }
        }
    }
    CSys.munmap(hi_a).unwrap();
    CSys.munmap(lo_a).unwrap();
    CSys.munmap(hi_b).unwrap();
    CSys.munmap(lo_b).unwrap();
}

#[test]
fn cpuid_is_executed_once_and_every_decision_follows_from_it() {
    // no spec clause (STARTUP-1): `init` and `init_with` share one CPUID detection; each tunable
    // set still decides the paths from it, as the uncached code did.
    use liella::config::Tunables;
    use liella_boundary::cpu;
    let _g = level();
    let direct = liella_rt::cpu::Features::detect(&liella_boundary::sys::CSys);
    assert_eq!(cpu::features_cached(), direct, "the cached features equal a fresh detection");
    let a = cpu::init_with(&Tunables::default());
    let b = cpu::init_with(&Tunables { avx2: Some(false), ..Tunables::default() });
    let c = cpu::init();
    let d = cpu::init_with(&Tunables::default());
    assert_eq!(a, d, "the same tunables decide the same paths");
    assert_eq!(a, c, "init() is the default-tunables decision");
    assert_eq!(b & 4, 0, "avx2 switched off by the tunable");
    assert_eq!(a & 4 != 0, direct.avx2(), "the default decision follows the CPU");
    assert_eq!(a & 2 != 0, direct.erms());
    // Once per process; a race between two first callers on different threads may detect twice.
    // Without the cache this call sequence alone detects four times.
    assert!((1..=2).contains(&cpu::detections()), "detections: {}", cpu::detections());
}

/// `strchr` and `strchrnul` at `p` for `c`, as offsets (`None` = NULL).
fn chr2(p: *const c_char, c: c_int) -> (Option<usize>, usize) {
    let r = s::strchr(p, c);
    let r = if r.is_null() { None } else { Some(r as usize - p as usize) };
    (r, s::strchrnul(p, c) as usize - p as usize)
}

#[test]
fn strchr_kernel_finds_the_first_byte_or_the_terminator_at_every_alignment_length_and_position() {
    // Clauses: s4_5_c_4, s4_5_c_5, s4_5_c_7 (SHORT-1, D-4.35).
    let _g = level();
    let mut buf = vec![0u8; 64 + 300 + 64];
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        for align in 0..64 {
            for len in 0..=200usize {
                // Zeros before the string (an unmasked head would find them) and the searched
                // byte after the terminator (decoys).
                for (c, x) in [(b'#', c_int::from(b'#')), (0xE9u8, 0x1E9), (0x80u8, -128)] {
                    buf.fill(c);
                    buf[..align].fill(0);
                    buf[align..align + len].fill(b'.');
                    buf[align + len] = 0;
                    let p = at(&mut buf, align).cast::<c_char>();
                    assert_eq!(chr2(p, x), (None, len), "miss, avx2={avx2}, align {align}, len {len}, c {c:#x}");
                    assert_eq!(chr2(p, 0), (Some(len), len), "c = 0, avx2={avx2}, align {align}, len {len}");
                    assert_eq!(s::index(p, x), s::strchr(p, x));
                    for pos in (0..len).rev() {
                        buf[align + pos] = c;
                        assert_eq!(chr2(p, x), (Some(pos), pos), "avx2={avx2}, align {align}, len {len}, pos {pos}, c {c:#x}");
                    }
                }
            }
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
}

#[test]
fn strchr_kernel_never_reads_past_the_page_holding_the_terminator() {
    // Clauses: s4_5_c_4, s4_5_c_5 (SHORT-1, D-4.35; D-4.14 page rule).
    // Both scan levels (PERF-STRCHR-NONAVX2: the SSE2 64-byte head reaches the page end too).
    let _g = level();
    let (lo, hi, base) = guarded();
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        for len in 0..400usize {
            s::memset(base as *mut c_void, c_int::from(b'.'), PAGE - 1);
            s::memset((base + PAGE - 1) as *mut c_void, 0, 1);
            let p = (base + PAGE - 1 - len) as *const c_char;
            assert_eq!(chr2(p, c_int::from(b'#')), (None, len), "avx2={avx2}, len {len}");
            if len > 0 {
                // The searched byte just before the terminator, the page's last byte.
                s::memset((base + PAGE - 2) as *mut c_void, c_int::from(b'#'), 1);
                assert_eq!(chr2(p, c_int::from(b'#')), (Some(len - 1), len - 1), "last byte, avx2={avx2}, len {len}");
            }
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
    CSys.munmap(hi).unwrap();
    CSys.munmap(lo).unwrap();
}

/// `memrchr` at `buf[align..]` for `#` over `n` bytes, as an offset.
fn memrchr_at(buf: &mut [u8], align: usize, n: usize) -> Option<usize> {
    let p = at(buf, align).cast::<c_void>();
    let r = s::memrchr(p, c_int::from(b'#'), n);
    if r.is_null() { None } else { Some(r as usize - p as usize) }
}

#[test]
fn memrchr_kernel_finds_the_last_match_inside_the_region_at_every_alignment_length_and_position() {
    // Clauses: s4_5_c_2 (SHORT-1, D-4.35).
    let _g = level();
    let mut buf = vec![0u8; 64 + 700 + 64];
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        for align in 0..64 {
            for n in (0..=300).chain(BOUNDED_LENGTHS) {
                // The target byte everywhere outside the region: a match outside it is wrong.
                buf.fill(b'#');
                buf[align..align + n].fill(b'.');
                assert_eq!(memrchr_at(&mut buf, align, n), None, "none, avx2={avx2}, align {align}, n {n}");
                // Matches planted from the start up: every byte before `pos` matches as well,
                // so only the last may be reported.
                for pos in 0..n {
                    buf[align + pos] = b'#';
                    assert_eq!(memrchr_at(&mut buf, align, n), Some(pos), "avx2={avx2}, align {align}, n {n}, pos {pos}");
                }
            }
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
}

#[test]
fn memrchr_kernel_never_reads_outside_the_region() {
    // Clauses: s4_5_c_2 (SHORT-1, D-4.35).
    let _g = level();
    let (lo, hi, base) = guarded();
    let r = CSys.mmap(0, 2 * PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, None, 0).unwrap();
    let (mut lo2, hi2) = r.split_at(PAGE).ok().unwrap();
    CSys.mprotect(&mut lo2, PROT_NONE).unwrap();
    let start = hi2.addr();
    let end = base + PAGE;
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        s::memset(base as *mut c_void, c_int::from(b'.'), PAGE);
        s::memset(start as *mut c_void, c_int::from(b'.'), PAGE);
        for n in 0..=PAGE {
            let p = (end - n) as *const c_void;
            assert!(s::memrchr(p, c_int::from(b'#'), n).is_null(), "to the page end, avx2={avx2}, n {n}");
            assert!(s::memrchr(start as *const c_void, c_int::from(b'#'), n).is_null(), "from the page start, avx2={avx2}, n {n}");
            if n > 0 {
                s::memset((end - n) as *mut c_void, c_int::from(b'#'), 1);
                s::memset(start as *mut c_void, c_int::from(b'#'), 1);
                assert_eq!(s::memrchr(p, c_int::from(b'#'), n) as usize, end - n, "first byte, avx2={avx2}, n {n}");
                assert_eq!(s::memrchr(start as *const c_void, c_int::from(b'#'), n) as usize, start, "page start, avx2={avx2}, n {n}");
                s::memset((end - n) as *mut c_void, c_int::from(b'.'), 1);
                s::memset(start as *mut c_void, c_int::from(b'.'), 1);
            }
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
    CSys.munmap(hi2).unwrap();
    CSys.munmap(lo2).unwrap();
    CSys.munmap(hi).unwrap();
    CSys.munmap(lo).unwrap();
}

#[test]
fn strncmp_kernel_finds_the_first_difference_inside_the_bound_at_every_alignment_pair() {
    // Clauses: s4_3_c_1 (SHORT-1, D-4.35).
    let mut a = vec![0u8; 32 + 100 + 32];
    let mut b = vec![0u8; 32 + 100 + 32];
    let _g = level();
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        for ia in 0..32 {
            for ib in (0..32).step_by(3) {
                for len in 0..=80usize {
                    a.fill(0x7f);
                    b.fill(0x81);
                    a[ia..ia + len].fill(b'x');
                    b[ib..ib + len].fill(b'x');
                    a[ia + len] = 0;
                    b[ib + len] = 0;
                    let pa = at(&mut a, ia).cast::<c_char>();
                    let pb = at(&mut b, ib).cast::<c_char>();
                    for n in [0, 1, 2, 7, 15, 16, 17, len, len + 1, len + 2, usize::MAX] {
                        assert_eq!(s::strncmp(pa, pb, n), 0, "equal, ia {ia}, ib {ib}, len {len}, n {n}");
                    }
                    for pos in 0..len {
                        // The byte of a above b's (0xE9 > 'x': unsigned order), then below it.
                        for (x, sign) in [(0xE9u8, 1), (b'a', -1), (0u8, -1)] {
                            a[ia + pos] = x;
                            for n in [pos, pos + 1, pos + 9, len, usize::MAX] {
                                let want = if n > pos { sign } else { 0 };
                                assert_eq!(s::strncmp(pa, pb, n), want, "ia {ia}, ib {ib}, len {len}, pos {pos}, x {x:#x}, n {n}");
                                assert_eq!(s::strncmp(pb, pa, n), -want, "swapped, ia {ia}, ib {ib}, len {len}, pos {pos}, x {x:#x}, n {n}");
                            }
                        }
                        a[ia + pos] = b'x';
                    }
                }
            }
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
}

#[test]
fn strncmp_kernel_never_reads_past_the_bound_at_a_page_end() {
    // Clauses: s4_3_c_1 (SHORT-1, D-4.35).
    // Texts without a terminator ending at the last byte before an unmapped page: n bounds the
    // read, and a load past it faults.
    let (lo, hi, base) = guarded();
    let (lo2, hi2, base2) = guarded();
    s::memset(base as *mut c_void, c_int::from(b'q'), PAGE);
    s::memset(base2 as *mut c_void, c_int::from(b'q'), PAGE);
    let _g = level();
    for avx2 in paths() {
        assert!(selftest::set_scan_avx2(avx2));
        for n in 1..=300usize {
            for skew in [0usize, 1, 5, 16, 17, 40] {
                let pa = (base + PAGE - n) as *const c_char;
                let pb = (base2 + PAGE - n - skew) as *const c_char;
                assert_eq!(s::strncmp(pa, pb, n), 0, "n {n}, skew {skew}");
                assert_eq!(s::strncmp(pb, pa, n), 0, "swapped, n {n}, skew {skew}");
            }
            // A difference at the last byte before the unmapped page.
            s::memset((base + PAGE - 1) as *mut c_void, c_int::from(b'r'), 1);
            assert_eq!(s::strncmp((base + PAGE - n) as *const c_char, (base2 + PAGE - n) as *const c_char, n), 1, "last byte, n {n}");
            s::memset((base + PAGE - 1) as *mut c_void, c_int::from(b'q'), 1);
        }
    }
    selftest::set_scan_avx2(false);
    liella_boundary::cpu::init();
    CSys.munmap(hi2).unwrap();
    CSys.munmap(lo2).unwrap();
    CSys.munmap(hi).unwrap();
    CSys.munmap(lo).unwrap();
}

/// The scanning exports make no per-call "paths decided?" test (D-4.39): they run whatever path the
/// kernels' scan level names. So every startup decision must set that level to the decided path:
/// AVX2 exactly when the CPU has it and no tunable switched it off.
#[test]
fn the_startup_decision_sets_the_scanners_path() {
    // Clauses: s4_8_c_0 (D-4.39, D-4.14).
    use liella::config::Tunables;
    let _g = level();
    let cpu_avx2 = std::arch::is_x86_feature_detected!("avx2");
    let configs = [
        (Tunables::default(), cpu_avx2),
        (Tunables { avx2: Some(false), ..Tunables::default() }, false),
        (Tunables { avx2: Some(true), ..Tunables::default() }, cpu_avx2),
        (Tunables { erms: Some(false), ..Tunables::default() }, cpu_avx2),
    ];
    for (c, (t, want)) in configs.iter().enumerate() {
        liella_boundary::cpu::init_with(t);
        assert_eq!(liella_boundary::cpu::scan_level(), u32::from(*want), "config {c}: scan level");
        assert_eq!(liella_boundary::cpu::avx2(), *want, "config {c}: decided flag");
    }
    liella_boundary::cpu::init();
    assert_eq!(liella_boundary::cpu::scan_level(), u32::from(cpu_avx2), "init()");
}

/// A byte-loop move within `buf` (the oracle: never the library's own `memmove`).
fn oracle_move(buf: &mut [u8], d: usize, s_at: usize, n: usize) {
    let tmp = buf[s_at..s_at + n].to_vec();
    for (i, b) in tmp.into_iter().enumerate() {
        buf[d + i] = b;
    }
}

#[test]
fn avx2_move_matches_a_byte_loop_at_every_overlap_alignment_and_size() {
    // Clauses: s4_2_c_1, s4_1_g_0 (memmove; memcpy too, D-4.2). MEM-BULK: overlapping regions
    // take the AVX2 copy, the direction forced by the overlap (both counted); nothing outside the
    // destination written.
    let _g = level();
    liella_boundary::cpu::init();
    let mut dirs = [0usize; 2];
    for n in COPY_SIZES {
        let mut shifts: Vec<isize> = (1..=40).chain([63, 64, 65, 96, 127, 128, 129, 200, 2047, 2048, 2049]).collect();
        shifts.extend([n - 33, n - 32, n - 31, n - 1].into_iter().filter(|&k| k > 0).map(|k| k as isize));
        let room = n.max(2049) + 64;
        let pattern: Vec<u8> = (0..n + 2 * room).map(|i| (i * 7 + 3 + i / 251) as u8).collect();
        for &k in &shifts {
            for shift in [k, -k] {
                for a in 0..32 {
                    let s_at = room + a;
                    let d_at = (s_at as isize + shift) as usize;
                    let mut buf = pattern.clone();
                    let Some(m) = selftest::move_avx2(&mut buf, s_at, d_at, n) else {
                        assert!(!liella_boundary::cpu::avx2(), "the AVX2 move refused on an AVX2 CPU");
                        return;
                    };
                    assert_eq!(m, n);
                    let mut want = pattern.clone();
                    oracle_move(&mut want, d_at, s_at, n);
                    assert!(buf == want, "avx2 move {n} by {shift}, src +{a}");
                    dirs[usize::from(shift > 0)] += 1;
                }
            }
        }
    }
    assert!(dirs[0] > 0 && dirs[1] > 0, "both overlap directions reached: {dirs:?}");
}

#[test]
fn memmove_matches_a_byte_loop_around_the_64_kib_edge_and_above() {
    // Clause: s4_2_c_1. The overlap path at and above the AVX2 copy's range (the entry's direct
    // call ends at 64 KiB, the checked path takes the rest), both directions, odd sizes.
    let _g = level();
    liella_boundary::cpu::init();
    for n in [65_535usize, 65_536, 65_537, 100_003] {
        for shift in [1isize, 8, 31, 4096, -1, -8, -31, -4096] {
            for a in [0usize, 5, 32] {
                let room = 4096 + 64;
                let pattern: Vec<u8> = (0..n + 2 * room).map(|i| (i * 13 + 1 + i / 251) as u8).collect();
                let (s_at, mut buf, mut want) = (room + a, pattern.clone(), pattern);
                let d_at = (s_at as isize + shift) as usize;
                oracle_move(&mut want, d_at, s_at, n);
                let p = buf.as_mut_ptr();
                let r = s::memmove(p.wrapping_add(d_at).cast(), p.wrapping_add(s_at).cast_const().cast(), n);
                assert_eq!(r, p.wrapping_add(d_at).cast(), "memmove {n} by {shift} return");
                assert!(buf == want, "memmove {n} by {shift}, src +{a}");
            }
        }
    }
}

#[test]
fn memset_matches_a_byte_loop_around_the_rep_stosb_threshold_at_every_alignment() {
    // Clause: s4_2_c_1 (memset). MEM-BULK: from the threshold the entry sets the first 64 bytes
    // and runs `rep stosb` from the next 64-byte boundary; every start offset modulo 64, sizes
    // around the threshold and 4 KiB, nothing outside the region written.
    // Both paths on every machine: ERMS forced off (the loops) and on (`rep stosb`, correct
    // without ERMS too; keke's Zen 1 has none).
    let _g = level();
    liella_boundary::cpu::init();
    for (erms, n) in [false, true].into_iter().flat_map(|e| [129usize, 1279, 1280, 1281, 4095, 4096, 4097, 65_537].map(|n| (e, n))) {
        selftest::set_erms(erms);
        for a in 0..64 {
            let mut buf = vec![0xA5u8; n + 192];
            let p = buf[64 + a..].as_mut_ptr();
            let r = s::memset(p.cast(), 0x15A, n);
            assert_eq!(r, p.cast(), "memset {n} at +{a} return");
            assert!(buf[..64 + a].iter().all(|&b| b == 0xA5), "memset {n} at +{a} erms {erms}: below written");
            assert!(buf[64 + a..64 + a + n].iter().all(|&b| b == 0x5A), "memset {n} at +{a} erms {erms}: region");
            assert!(buf[64 + a + n..].iter().all(|&b| b == 0xA5), "memset {n} at +{a} erms {erms}: above written");
        }
    }
    liella_boundary::cpu::init();
}

#[test]
fn sse2_move_matches_a_byte_loop_at_every_overlap_alignment_and_size() {
    // Clauses: s4_2_c_1, s4_1_g_0 (memmove; memcpy too, D-4.2). MEM-FALLBACK: the SSE2 build of
    // the move (CPUs without AVX2), overlapping both ways and disjoint, every source alignment
    // modulo 32; nothing outside the destination written.
    let mut dirs = [0usize; 2];
    for n in COPY_SIZES {
        let mut shifts: Vec<isize> = (1..=40).chain([63, 64, 65, 96, 127, 128, 129, 200, 2047, 2048, 2049]).collect();
        shifts.extend([n - 17, n - 16, n - 15, n - 1, n, n + 1, n + 4095].into_iter().filter(|&k| k > 0).map(|k| k as isize));
        let room = n.max(2049) + n + 4096;
        let pattern: Vec<u8> = (0..n + 2 * room).map(|i| (i * 7 + 3 + i / 251) as u8).collect();
        for &k in &shifts {
            for shift in [k, -k] {
                for a in 0..32 {
                    let s_at = room + a;
                    let d_at = (s_at as isize + shift) as usize;
                    let mut buf = pattern.clone();
                    assert_eq!(selftest::move_sse2(&mut buf, s_at, d_at, n), Some(n));
                    let mut want = pattern.clone();
                    oracle_move(&mut want, d_at, s_at, n);
                    assert!(buf == want, "sse2 move {n} by {shift}, src +{a}");
                    if (k as usize) < n {
                        dirs[usize::from(shift > 0)] += 1;
                    }
                }
            }
        }
    }
    assert!(dirs[0] > 0 && dirs[1] > 0, "both overlap directions reached: {dirs:?}");
}

#[test]
fn avx2_fill_sets_exactly_the_region_at_every_alignment_and_size() {
    // Clause: s4_2_c_1 (memset). MEM-FALLBACK: the AVX2 fill, every start offset modulo 64, sizes
    // around its step (128) and the vector (32) edges; nothing outside the region written.
    for n in [128usize, 129, 159, 160, 161, 191, 192, 193, 255, 256, 257, 383, 384, 385, 1279, 4095, 4096, 4097, 65_537] {
        for a in 0..64 {
            let mut buf = vec![0xA5u8; n + 192];
            let Some(m) = selftest::fill_avx2(&mut buf[64 + a..64 + a + n], (n + a) as u8 | 1) else {
                assert!(!liella_boundary::cpu::avx2(), "the AVX2 fill refused on an AVX2 CPU");
                return;
            };
            assert_eq!(m, n);
            let v = (n + a) as u8 | 1;
            assert!(buf[..64 + a].iter().all(|&b| b == 0xA5), "fill {n} at +{a}: below written");
            assert!(buf[64 + a..64 + a + n].iter().all(|&b| b == v), "fill {n} at +{a}: region");
            assert!(buf[64 + a + n..].iter().all(|&b| b == 0xA5), "fill {n} at +{a}: above written");
        }
    }
}

#[test]
fn bulk_kernel_choice_follows_the_cpu_flags() {
    // Clause: s4_2_c_1. MEM-FALLBACK, D-4.25: with the kernels' AVX2 build off (a CPU without
    // AVX2) no copy or fill picks an AVX2 kernel; overlapping moves take SSE2, disjoint copies
    // `rep movsb` only with ERMS from its threshold at a fast placement, fills `rep stosb` with
    // ERMS and the AVX2 fill only when AVX2 is on.
    let _g = level();
    liella_boundary::cpu::init();
    let has_avx2 = selftest::set_kernel_avx2(true);
    selftest::set_kernel_avx2(false);
    let base = 1usize << 20;
    for erms in [false, true] {
        selftest::set_erms(erms);
        // MEMCPY-NONAVX2-THRESHOLD (D-4.14): `rep movsb` from 6144 bytes (`COPY_ERMS_MIN`); plain
        // to a 64-byte-aligned destination; else the head-aligned kernel when the addresses
        // agree modulo 64, and below 16385 bytes (`ERMS_A64_MISALIGNED_END`) at other accepted
        // placements; plain from there. `base` is 64-byte aligned.
        assert_eq!(liella_boundary::cpu::COPY_ERMS_MIN, 6144);
        assert_eq!(liella_boundary::cpu::ERMS_A64_MISALIGNED_END, 16_385);
        for n in [129usize, 1279, 1280, 4096, 6143, 6144, 16_384, 16_385, 65_536] {
            assert_eq!(selftest::bulk_move_kernel(base + 8, base, n), "sse2", "overlap up {n} erms {erms}");
            assert_eq!(selftest::bulk_move_kernel(base, base + 8, n), "sse2", "overlap down {n} erms {erms}");
            let on = erms && n >= 6144;
            let dst_aligned = if on { "erms" } else { "sse2" };
            let aligned = if on { "erms-a64" } else { "sse2" };
            let misaligned = if !on { "sse2" } else if n < 16_385 { "erms-a64" } else { "erms" };
            assert_eq!(selftest::bulk_move_kernel(base + (1 << 18), base, n), dst_aligned, "disjoint {n} erms {erms}");
            assert_eq!(selftest::bulk_move_kernel(base + (1 << 18) + 64, base, n), dst_aligned, "disjoint +64 {n} erms {erms}");
            assert_eq!(selftest::bulk_move_kernel(base + (1 << 18), base + 7, n), dst_aligned, "disjoint src +7 {n} erms {erms}");
            assert_eq!(selftest::bulk_move_kernel(base + (1 << 18) + 64, base + 48, n), "sse2", "disjoint dst aligned +16 {n} erms {erms}");
            assert_eq!(selftest::bulk_move_kernel(base + (1 << 18) + 7, base + 7, n), aligned, "disjoint both +7 {n} erms {erms}");
            assert_eq!(selftest::bulk_move_kernel(base + (1 << 18) + 600, base, n), misaligned, "disjoint +600 {n} erms {erms}");
            assert_eq!(selftest::bulk_move_kernel(base + (1 << 18) - 7, base, n), misaligned, "disjoint -7 {n} erms {erms}");
            assert_eq!(selftest::bulk_move_kernel(base + (1 << 18) + 16, base, n), "sse2", "disjoint +16 {n} erms {erms}");
            assert_eq!(selftest::bulk_move_kernel(base + (1 << 18) + 32, base, n), misaligned, "disjoint +32 {n} erms {erms}");
            assert_eq!(selftest::bulk_fill_kernel(n), if erms && n >= 1280 { "erms" } else { "checked" }, "fill {n} erms {erms}");
        }
        // FSRM with the AVX2 copy keeps the AVX2 copy up to 1 MiB (N100); other classes `rep movsb`
        // from 64 KiB + 1; a tunable threshold keeps 64 KiB + 1.
        assert_eq!(liella_boundary::cpu::erms_large_window(true, true, None), 1_048_577);
        assert_eq!(liella_boundary::cpu::erms_large_window(true, true, Some(4096)), 65_537);
        assert_eq!(liella_boundary::cpu::erms_large_window(true, false, None), 65_537);
        assert_eq!(liella_boundary::cpu::erms_large_window(false, true, None), 65_537);
        for n in [0usize, 1, 128, 65_537, 1 << 20] {
            assert_ne!(selftest::bulk_move_kernel(base + 8, base, n), "avx2", "move {n} erms {erms}");
            assert_eq!(selftest::bulk_move_kernel(base + 8, base, n), "checked", "move {n} erms {erms}");
        }
        assert_eq!(selftest::bulk_fill_kernel(128), "checked");
        if has_avx2 {
            selftest::set_kernel_avx2(true);
            assert_eq!(selftest::bulk_move_kernel(base + 8, base, 4096), "avx2");
            // MEMCPY-POLICY-2: the `rep movsb` window of the AVX2 range (forced ERMS: the narrow
            // class window, 16 KiB up to 20 KiB), disjoint copies whose addresses agree modulo 64.
            for n in [16_383usize, 16_384, 16_448, 20_416, 20_479, 20_480] {
                let win = erms && (16_384..20_480).contains(&n);
                let k = |w: bool| if w { "erms" } else { "avx2" };
                assert_eq!(selftest::bulk_move_kernel(base + (1 << 18), base, n), k(win), "avx2 disjoint {n} erms {erms}");
                assert_eq!(selftest::bulk_move_kernel(base, base + (1 << 18), n), k(win), "avx2 disjoint below {n} erms {erms}");
                assert_eq!(selftest::bulk_move_kernel(base + (1 << 18) + 7, base + 7, n), k(win), "avx2 disjoint both +7 {n} erms {erms}");
                assert_eq!(selftest::bulk_move_kernel(base + (1 << 18) + 64, base, n), k(win), "avx2 disjoint +64 {n} erms {erms}");
                for off in [1usize, 7, 16, 32, 600, 4095] {
                    assert_eq!(selftest::bulk_move_kernel(base + (1 << 18) + off, base, n), "avx2", "avx2 disjoint +{off} {n} erms {erms}");
                }
                assert_eq!(selftest::bulk_move_kernel(base + 64, base, n), "avx2", "avx2 overlap up 64 {n} erms {erms}");
                assert_eq!(selftest::bulk_move_kernel(base, base + 64, n), "avx2", "avx2 overlap down 64 {n} erms {erms}");
                assert_eq!(selftest::bulk_move_kernel(base + 8, base, n), "avx2", "avx2 overlap up {n} erms {erms}");
                assert_eq!(selftest::bulk_move_kernel(base, base + 8, n), "avx2", "avx2 overlap down {n} erms {erms}");
                assert_eq!(selftest::bulk_move_kernel(base + n, base, n), k(win && n % 64 == 0), "avx2 adjacent {n} erms {erms}");
                assert_eq!(selftest::bulk_move_kernel(base + n - 64, base, n), "avx2", "avx2 overlap by 64 {n} erms {erms}");
            }
            assert_eq!(selftest::bulk_fill_kernel(4096), if erms { "erms" } else { "avx2" }, "fill 4096 avx2 erms {erms}");
            assert_eq!(selftest::bulk_fill_kernel(129), "avx2");
            assert_eq!(selftest::bulk_fill_kernel(1 << 20), if erms { "erms" } else { "avx2" });
            selftest::set_kernel_avx2(false);
        }
    }
    liella_boundary::cpu::init();
}

#[test]
fn memmove_and_memset_match_a_byte_loop_on_every_bulk_path() {
    // Clauses: s4_2_c_1, s4_1_g_0. MEM-FALLBACK: the exported entries with the kernels' AVX2
    // build off and on (where the CPU has it) and ERMS forced off and on, so the SSE2 move, the
    // AVX2 fill, `rep movsb`/`rep stosb` and the checked paths all run on a gate machine: sizes
    // around every class edge, overlap both ways and disjoint, odd alignments.
    let _g = level();
    liella_boundary::cpu::init();
    let mut builds = vec![false];
    if selftest::set_kernel_avx2(true) {
        builds.push(true);
    }
    for avx2 in builds {
        for erms in [false, true] {
            selftest::set_kernel_avx2(avx2);
            selftest::set_erms(erms);
            for n in [129usize, 130, 143, 144, 145, 255, 256, 257, 1279, 1280, 1281, 4095, 4096, 4097, 6143, 6144, 6145, 16_384, 16_385, 65_536, 65_537, 70_001] {
                let room = n + 4096 + 128;
                let pattern: Vec<u8> = (0..n + 2 * room).map(|i| (i * 13 + 1 + i / 251) as u8).collect();
                let far = ((n + 127) & !63) as isize; // disjoint, the addresses agreeing modulo 64
                for shift in [1isize, 8, 15, 16, 17, 100, 4096, (n + 32) as isize, (n + 600) as isize, far, -1, -8, -15, -16, -17, -100, -4096, -((n + 32) as isize), -far] {
                    for a in [0usize, 3, 16, 31] {
                        let (s_at, mut buf, mut want) = (room + a, pattern.clone(), pattern.clone());
                        let d_at = (s_at as isize + shift) as usize;
                        oracle_move(&mut want, d_at, s_at, n);
                        let p = buf.as_mut_ptr();
                        let r = s::memmove(p.wrapping_add(d_at).cast(), p.wrapping_add(s_at).cast_const().cast(), n);
                        assert_eq!(r, p.wrapping_add(d_at).cast(), "memmove {n} by {shift} return");
                        assert!(buf == want, "memmove {n} by {shift}, src +{a}, avx2 {avx2} erms {erms}");
                    }
                }
                for a in [0usize, 1, 31, 32, 33, 63] {
                    let mut buf = vec![0xA5u8; n + 192];
                    let p = buf[64 + a..].as_mut_ptr();
                    let r = s::memset(p.cast(), 0x13C, n);
                    assert_eq!(r, p.cast(), "memset {n} at +{a} return");
                    assert!(buf[..64 + a].iter().all(|&b| b == 0xA5), "memset {n} at +{a} avx2 {avx2} erms {erms}: below");
                    assert!(buf[64 + a..64 + a + n].iter().all(|&b| b == 0x3C), "memset {n} at +{a} avx2 {avx2} erms {erms}: region");
                    assert!(buf[64 + a + n..].iter().all(|&b| b == 0xA5), "memset {n} at +{a} avx2 {avx2} erms {erms}: above");
                }
            }
        }
    }
    liella_boundary::cpu::init();
}

/// The NULL child's memchr call (set before the fork; the child gets its own copy).
static CHR_CASE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static CHR_FOLD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static CHR_AVX2: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn null_memchr(_k: &CSys) -> i32 {
    use std::sync::atomic::Ordering::SeqCst;
    // No core dump: the caller runs this through `in_child_quiet` (CORE-STORM's expect_crash).
    if !selftest::set_null_fold(CHR_FOLD.load(SeqCst)) || !selftest::set_scan_avx2(CHR_AVX2.load(SeqCst)) {
        return 3;
    }
    let r = s::memchr(core::ptr::null(), c_int::from(b'#'), CHR_CASE.load(SeqCst));
    std::hint::black_box(r);
    0
}

#[test]
fn null_faults_in_the_folded_memchr_sizes_and_aborts_in_every_other() {
    // Clause: s4_5_c_1 with S6 (NULL with n > 0 is the EFAULT abort) and the memchr fold
    // (MEMCHR-FOLD, owner ruling 2026-10-11): with folding on, NULL with 1 <= n <= isize::MAX dies by
    // SIGSEGV on page zero (the kernel's first load); n > isize::MAX and every n with folding off take
    // the S6 abort; n == 0 returns NULL without a read, in each path.
    use std::sync::atomic::Ordering::SeqCst;
    let _g = level();
    let big = isize::MAX.unsigned_abs();
    for fold in [false, true] {
        for avx2 in paths() {
            CHR_FOLD.store(fold, SeqCst);
            CHR_AVX2.store(avx2, SeqCst);
            for n in [1, 2, 16, 17, 32, 33, 64, 65, 4096, big - 1, big, big + 1, usize::MAX] {
                CHR_CASE.store(n, SeqCst);
                let end = selftest::in_child_quiet(null_memchr).unwrap();
                let want = if fold && n <= big { 11 } else { 6 };
                assert_eq!(end, selftest::ChildEnd::Signaled(want), "fold {fold} avx2 {avx2} n {n}");
            }
            assert!(selftest::set_null_fold(fold));
            assert!(s::memchr(core::ptr::null(), c_int::from(b'#'), 0).is_null(), "fold {fold} avx2 {avx2} n 0");
        }
    }
    selftest::set_scan_avx2(false);
}

/// The NULL child's rawmemchr path (set before the fork; the child gets its own copy).
static RAW_AVX2: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn null_rawmemchr(_k: &CSys) -> i32 {
    use std::sync::atomic::Ordering::SeqCst;
    // No core dump: the caller runs this through `in_child_quiet` (CORE-STORM's expect_crash).
    if !selftest::set_null_fold(true) || !selftest::set_scan_avx2(RAW_AVX2.load(SeqCst)) {
        return 3;
    }
    let r = s::rawmemchr(core::ptr::null(), c_int::from(b'#'));
    std::hint::black_box(r);
    0
}

#[test]
fn null_rawmemchr_aborts_through_the_direct_entry_even_with_folding_on() {
    // Clause: s4_5_c_1 with S6 (NULL is the EFAULT abort). SCAN-TAIL: rawmemchr is a direct
    // assembly entry but is not folded (D-4.48), so its NULL test must take the S6 abort (SIGABRT),
    // never a page-zero fault, on both paths and with NULL folding on.
    use std::sync::atomic::Ordering::SeqCst;
    let _g = level();
    for avx2 in paths() {
        RAW_AVX2.store(avx2, SeqCst);
        let end = selftest::in_child_quiet(null_rawmemchr).unwrap();
        assert_eq!(end, selftest::ChildEnd::Signaled(6), "rawmemchr NULL, avx2 {avx2}");
    }
    selftest::set_scan_avx2(false);
}

static MRC_AVX2: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static MRC_N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn null_memrchr(_k: &CSys) -> i32 {
    use std::sync::atomic::Ordering::SeqCst;
    // No core dump: the caller runs this through `in_child_quiet` (CORE-STORM's expect_crash).
    if !selftest::set_null_fold(true) || !selftest::set_scan_avx2(MRC_AVX2.load(SeqCst)) {
        return 3;
    }
    let r = s::memrchr(core::ptr::null(), c_int::from(b'#'), MRC_N.load(SeqCst));
    std::hint::black_box(r);
    0
}

#[test]
fn null_memrchr_aborts_through_the_direct_entry_and_n_zero_reads_nothing() {
    // Clause: s4_5_c_1 with S6 (NULL with n > 0 is the EFAULT abort). SCAN-RAW: memrchr is a
    // direct assembly entry but is not folded (D-4.48): NULL with n = 1, 64 or 200 and n beyond
    // isize::MAX must take the S6 abort (SIGABRT), never a page-zero fault, on both paths and
    // with NULL folding on; n = 0 returns NULL for any pointer and reads nothing.
    use std::sync::atomic::Ordering::SeqCst;
    let _g = level();
    for avx2 in paths() {
        MRC_AVX2.store(avx2, SeqCst);
        for n in [1usize, 64, 200, 1usize << 63] {
            MRC_N.store(n, SeqCst);
            let end = selftest::in_child_quiet(null_memrchr).unwrap();
            assert_eq!(end, selftest::ChildEnd::Signaled(6), "memrchr NULL, n {n}, avx2 {avx2}");
        }
        assert!(selftest::set_scan_avx2(avx2));
        assert!(s::memrchr(core::ptr::null(), c_int::from(b'#'), 0).is_null(), "NULL, n 0, avx2 {avx2}");
        assert!(s::memrchr(core::ptr::dangling::<c_void>(), c_int::from(b'#'), 0).is_null(), "bad pointer, n 0, avx2 {avx2}");
    }
    selftest::set_scan_avx2(false);
}

/// The NULL child's strcmp argument order (set before the fork; the child gets its own copy).
static CMP_NULL_FIRST: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static CMP_AVX2: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn null_strcmp(_k: &CSys) -> i32 {
    use std::sync::atomic::Ordering::SeqCst;
    // No core dump: the caller runs this through `in_child_quiet` (CORE-STORM's expect_crash).
    if !selftest::set_scan_avx2(CMP_AVX2.load(SeqCst)) {
        return 3;
    }
    let text = b"liella\0";
    let p = text.as_ptr().cast::<c_char>();
    let (a, b) = if CMP_NULL_FIRST.load(SeqCst) { (core::ptr::null(), p) } else { (p, core::ptr::null()) };
    std::hint::black_box(s::strcmp(a, b));
    0
}

#[test]
fn strcmp_aborts_on_a_null_string_in_either_argument() {
    // Clause: s4_3_c_1 with S6 (a NULL string is the EFAULT abort; strcmp never folds, owner ruling
    // 2026-10-08 option B). Through the direct entry (STRCMP-ENTRY): each of its two NULL tests must
    // reach `liella_hot_strcmp_null` and abort (SIGABRT), not fall into the kernel (SIGSEGV on page zero).
    use std::sync::atomic::Ordering::SeqCst;
    let _g = level();
    for avx2 in paths() {
        CMP_AVX2.store(avx2, SeqCst);
        for null_first in [true, false] {
            CMP_NULL_FIRST.store(null_first, SeqCst);
            let end = selftest::in_child_quiet(null_strcmp).unwrap();
            assert_eq!(end, selftest::ChildEnd::Signaled(6), "avx2 {avx2} null first {null_first}");
        }
    }
    selftest::set_scan_avx2(false);
}

/// The NULL child's scanner (0 strchr, 1 index, 2 strchrnul, 3 strncmp with a NULL first, 4 with
/// a NULL second) and level, set before the fork.
static CHR_NULL_FN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn null_chr_cmp(_k: &CSys) -> i32 {
    use std::sync::atomic::Ordering::SeqCst;
    // No core dump: the caller runs this through `in_child_quiet` (CORE-STORM's expect_crash).
    if !selftest::set_scan_avx2(CMP_AVX2.load(SeqCst)) {
        return 3;
    }
    let text = b"liella\0";
    let p = text.as_ptr().cast::<c_char>();
    let z = core::ptr::null::<c_char>();
    match CHR_NULL_FN.load(SeqCst) {
        0 => { std::hint::black_box(s::strchr(z, c_int::from(b'l'))); }
        1 => { std::hint::black_box(s::index(z, 0)); }
        2 => { std::hint::black_box(s::strchrnul(z, c_int::from(b'#'))); }
        3 => { std::hint::black_box(s::strncmp(z, p, 1)); }
        _ => { std::hint::black_box(s::strncmp(p, z, 64)); }
    }
    0
}

#[test]
fn strchr_strchrnul_and_strncmp_abort_on_null_through_their_direct_entries() {
    // Clauses: s4_5_c_1 and s4_3_c_1 with S6 (a NULL string is the EFAULT abort). STR-CHR-CMP-SHORT:
    // strchr, index, strchrnul and strncmp are direct assembly entries whose NULL tests sit in the
    // slow path (the head's (s - 1) page test never passes NULL): each must reach its
    // `liella_hot_*_null` and abort (SIGABRT), never fault on page zero, on both levels; strncmp with
    // n == 0 reads nothing and returns 0 for NULL or a dangling pointer.
    use std::sync::atomic::Ordering::SeqCst;
    let _g = level();
    for avx2 in paths() {
        CMP_AVX2.store(avx2, SeqCst);
        for f in 0..5 {
            CHR_NULL_FN.store(f, SeqCst);
            let end = selftest::in_child_quiet(null_chr_cmp).unwrap();
            assert_eq!(end, selftest::ChildEnd::Signaled(6), "avx2 {avx2} scanner {f}");
        }
        assert!(selftest::set_scan_avx2(avx2));
        assert_eq!(s::strncmp(core::ptr::null(), core::ptr::null(), 0), 0, "NULL, n 0, avx2 {avx2}");
        assert_eq!(s::strncmp(core::ptr::dangling(), core::ptr::dangling(), 0), 0, "dangling, n 0, avx2 {avx2}");
    }
    selftest::set_scan_avx2(false);
}

fn null_strcoll(_k: &CSys) -> i32 {
    use std::sync::atomic::Ordering::SeqCst;
    if !selftest::set_scan_avx2(CMP_AVX2.load(SeqCst)) {
        return 3;
    }
    let text = b"liella\0";
    let p = text.as_ptr().cast::<c_char>();
    let (a, b) = if CMP_NULL_FIRST.load(SeqCst) { (core::ptr::null(), p) } else { (p, core::ptr::null()) };
    std::hint::black_box(s::strcoll(a, b));
    0
}

#[test]
fn strcoll_aborts_on_a_null_string_in_either_argument_in_the_c_locale() {
    // Clause: s4_3_c_1 with S6 (a NULL string is the EFAULT abort). The C locale orders by code point,
    // so strcoll takes the direct strcmp entry (SORT-UTF8-DRIFT): its NULL tests, which sit in the
    // kernel's slow path (STRCMP-HEADNULL), must still reach `liella_hot_strcmp_null` and abort (SIGABRT).
    use std::sync::atomic::Ordering::SeqCst;
    let _g = level();
    for avx2 in paths() {
        CMP_AVX2.store(avx2, SeqCst);
        for null_first in [true, false] {
            CMP_NULL_FIRST.store(null_first, SeqCst);
            let end = selftest::in_child_quiet(null_strcoll).unwrap();
            assert_eq!(end, selftest::ChildEnd::Signaled(6), "avx2 {avx2} null first {null_first}");
        }
    }
    selftest::set_scan_avx2(false);
}

#[test]
fn memchr_answers_alike_with_null_folding_on_and_off_and_takes_the_folding_decision() {
    // Clauses: s4_5_c_1. The direct entry's gate (MEMCHR-FOLD): folding off sends every n through the
    // checked entry, on enters the kernel; the answers are the same at every alignment, n and
    // position. An undecided process takes the decision on its first checked memchr, of any n.
    let _g = level();
    let mut buf = vec![b'.'; 64 + 300 + 64];
    for fold in [false, true] {
        assert!(selftest::set_null_fold(fold));
        for avx2 in paths() {
            assert!(selftest::set_scan_avx2(avx2));
            for align in 0..64 {
                for n in 0..=300 {
                    assert_eq!(memchr_at(&mut buf, align, n), None, "none, fold {fold} avx2 {avx2} align {align} n {n}");
                    for pos in [0, n / 2, n.wrapping_sub(1), n] {
                        if pos > n || pos >= 300 {
                            continue;
                        }
                        buf[align + pos] = b'#';
                        let want = if pos < n { Some(pos) } else { None };
                        assert_eq!(memchr_at(&mut buf, align, n), want, "fold {fold} avx2 {avx2} align {align} n {n} pos {pos}");
                        buf[align + pos] = b'.';
                    }
                }
            }
        }
    }
    selftest::set_scan_avx2(false);
    selftest::unresolve_null_fold();
    assert!(!selftest::null_fold_resolved());
    let p = buf.as_ptr().cast::<c_void>();
    assert!(s::memchr(p, c_int::from(b'#'), 0).is_null());
    assert!(selftest::null_fold_resolved(), "the checked memchr did not take the folding decision");
}

#[test]
fn memmove_matches_a_byte_loop_around_the_rep_movsb_window_of_the_avx2_range() {
    // Clause: s4_2_c_1. MEMCPY-POLICY-2, D-4.14: with the AVX2 build on and ERMS forced on and
    // off, disjoint copies at the forced window's edges (16 KiB, 20 KiB) whose addresses agree
    // modulo 64 (the 64-byte head store at three destination alignments) and overlapping moves in
    // both directions through the exported `memmove`/`memcpy` equal a byte-loop reference;
    // nothing outside the destination changes.
    let _g = level();
    liella_boundary::cpu::init();
    if !selftest::set_kernel_avx2(true) {
        liella_boundary::cpu::init();
        return;
    }
    for erms in [false, true] {
        selftest::set_erms(erms);
        for n in [16_383usize, 16_384, 16_385, 18_432, 20_479, 20_480] {
            let ni = n as isize;
            for (shift, o) in [-4096isize, -64, -1, 1, 64, 4096, ni, -ni, ni + 64, -ni - 64, ni + 128].into_iter().flat_map(|s| [0usize, 7, 33].map(|o| (s, o))) {
                let pad = n + 4224;
                let mut buf: Vec<u8> = (0..n + 2 * pad + 64).map(|i| (i * 7 + i / 251) as u8).collect();
                let mut want = buf.clone();
                let src = pad + o;
                let dst = (pad as isize + shift) as usize;
                let tmp = want[src..src + n].to_vec();
                want[dst..dst + n].copy_from_slice(&tmp);
                let p = buf.as_mut_ptr();
                let r = s::memmove(p.wrapping_add(dst).cast(), p.wrapping_add(src).cast(), n);
                assert_eq!(r, p.wrapping_add(dst).cast(), "memmove {n} shift {shift} erms {erms}: return");
                assert!(buf == want, "memmove {n} shift {shift} erms {erms}: bytes differ");
            }
            let srcv: Vec<u8> = (0..n + 64).map(|i| (i * 13 + 5) as u8).collect();
            for (sa, da) in [(0usize, 0usize), (7, 0), (0, 7), (0, 16), (32, 0)] {
                let mut dstv = vec![0xA5u8; n + 128];
                let r = s::memcpy(dstv[32 + da..].as_mut_ptr().cast(), srcv[sa..].as_ptr().cast(), n);
                assert_eq!(r, dstv[32 + da..].as_mut_ptr().cast(), "memcpy {n} return");
                assert!(dstv[32 + da..32 + da + n] == srcv[sa..sa + n], "memcpy {n} +{sa}/+{da} erms {erms}: region");
                assert!(dstv[..32 + da].iter().chain(&dstv[32 + da + n..]).all(|&b| b == 0xA5), "memcpy {n} +{sa}/+{da} erms {erms}: outside written");
            }
        }
    }
    liella_boundary::cpu::init();
}
