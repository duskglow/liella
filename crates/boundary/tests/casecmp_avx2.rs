//! `strcasecmp`/`strncasecmp` with the AVX2 and SSE2 kernels (CASECMP-K1, CASECMP, D-4.14, D-5.6): byte-identical to
//! the path of before (both strings measured, `ctype::compare_folded`) on every input.
//!
//! Each test runs on every path: the AVX2 kernel (where the CPU has it), the SSE2 kernel (CASECMP)
//! and the measured one (forced through `selftest::set_casecmp_kernel`). The reference is the pre-change export body, written out
//! here, and an independent naive loop; both must agree with the export. Inputs cover every
//! alignment of the first string in a 32-byte window against eight of the second's, every length up to 80, every position of the first
//! difference, bytes just outside `A`-`Z` (`@ [ \` {`), bytes of 128 and above, a second string
//! that is shorter, embedded bound `n` values around the difference, and strings that end at the
//! last byte of a page whose next page is unmapped.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use std::cmp::Ordering;
use std::sync::{Mutex, MutexGuard};

use liella_abi::ctypes::{c_char, c_int, c_void};
use liella_boundary::locale;
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

/// The kernels to run (`selftest::set_casecmp_kernel`): 0 = none (the measured path), 1 = SSE2
/// (CASECMP), and 2 = AVX2 where the CPU has it.
fn paths() -> Vec<u32> {
    let mut v = vec![0, 1];
    if selftest::set_casecmp_kernel(2) {
        v.push(2);
    }
    v
}

/// The text of `v` as `strz_n` measures it: up to the first NUL, at most `n` bytes.
fn measured(v: &[u8], n: usize) -> &[u8] {
    let end = v.iter().position(|&c| c == 0).unwrap_or(v.len()).min(n);
    &v[..end]
}

/// The export body before CASECMP-K1 on the bytes `va`, `vb` the strings hold (C locale).
fn before(va: &[u8], vb: &[u8], n: usize) -> c_int {
    match liella::ctype::compare_folded(measured(va, n), measured(vb, n), locale::current()) {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

/// An independent byte loop (C locale).
fn naive(va: &[u8], vb: &[u8], n: usize) -> c_int {
    for i in 0..n {
        let (x, y) = (va.get(i).copied().unwrap_or(0).to_ascii_lowercase(), vb.get(i).copied().unwrap_or(0).to_ascii_lowercase());
        if x != y {
            return if x < y { -1 } else { 1 };
        }
        if x == 0 {
            return 0;
        }
    }
    0
}

/// Check `strcasecmp` (when `n` is `None`) or `strncasecmp` on the strings at `pa`, `pb`, which
/// hold the bytes `va`, `vb` (to the NUL, or as far as the bound reaches), against both references.
fn check(pa: usize, va: &[u8], pb: usize, vb: &[u8], n: Option<usize>, what: &dyn Fn() -> String) {
    let (a, b) = (pa as *const c_char, pb as *const c_char);
    let lim = n.unwrap_or(usize::MAX);
    let want = before(va, vb, lim);
    assert_eq!(want, naive(va, vb, lim), "references disagree: {}", what());
    let got = match n {
        None => s::strcasecmp(a, b),
        Some(n) => s::strncasecmp(a, b, n),
    };
    assert_eq!(got, want, "{}", what());
    if n.is_none() {
        assert_eq!(s::strcasecmp(b, a), -want, "swapped: {}", what());
    }
}

/// Bytes planted at the difference position: (a, b).
const PAIRS: [(u8, u8); 16] = [
    (b'a', b'A'), (b'Z', b'z'), (b'@', b'`'), (b'[', b'{'), (b'A', b'@'), (b'Z', b'['), (b'a', b'`'), (b'z', b'{'),
    (b'm', b'n'), (b'N', b'm'), (0x7F, 0x80), (0xC1, b'a'), (0xE1, 0xC1), (0xFF, 0xFE), (b'1', b'!'), (b' ', 0x01),
];

#[test]
fn casecmp_equals_the_path_of_before_at_every_alignment_pair_length_and_difference() {
    // Clauses: s4_3_c_1 (CASECMP-K1).
    let _g = level();
    for kernel in paths() {
        assert!(selftest::set_casecmp_kernel(kernel));
        let mut a = vec![0u8; 32 + 150 + 64];
        let mut b = a.clone();
        for aa in 0..32 {
            for ba in [0, 1, 3, 8, 15, 16, 17, 31] {
                for len in (0..80).chain([95, 111, 128, 129, 144, 150]) {
                    for (buf, al) in [(&mut a, aa), (&mut b, ba)] {
                        buf.fill(b'y');
                        for i in 0..len {
                            const TEXT: &[u8] = b"AbCdEfGhIjKlMnOpQrStUvWxYz0123 .-_";
                            let v = TEXT[(i * 7) % TEXT.len()];
                            buf[al + i] = if (i + al) % 3 == 0 { v.to_ascii_lowercase() } else { v.to_ascii_uppercase() };
                        }
                        buf[al + len] = 0;
                    }
                    // Equal up to case: the swapped case on every other byte of b.
                    for i in 0..len {
                        if i % 2 == 0 {
                            b[ba + i] = b[ba + i].to_ascii_uppercase();
                        } else {
                            b[ba + i] = b[ba + i].to_ascii_lowercase();
                        }
                    }
                    let (pa, pb) = (a.as_ptr() as usize + aa, b.as_ptr() as usize + ba);
                    let w = || format!("equal, align {aa}/{ba}, len {len}, kernel {kernel}");
                    check(pa, &a[aa..], pb, &b[ba..], None, &w);
                    if len % 7 == 0 || len < 4 {
                        for n in [0, 1, len.saturating_sub(1), len, len + 1, len + 40] {
                            check(pa, &a[aa..], pb, &b[ba..], Some(n), &|| format!("{}, n {n}", w()));
                        }
                    }
                    for d in (0..len).filter(|d| len < 40 || d % 5 == 0 || *d + 3 > len || *d < 3) {
                        let keep = (a[aa + d], b[ba + d]);
                        for (k, (x, y)) in PAIRS.into_iter().enumerate() {
                            if len >= 12 && (k + d) % 4 != 0 {
                                continue;
                            }
                            (a[aa + d], b[ba + d]) = (x, y);
                            let w = || format!("diff {x:#x}/{y:#x} at {d}, align {aa}/{ba}, len {len}, kernel {kernel}");
                            check(pa, &a[aa..], pb, &b[ba..], None, &w);
                            for n in [d, d + 1, d + 2, d + 33] {
                                check(pa, &a[aa..], pb, &b[ba..], Some(n), &|| format!("{}, n {n}", w()));
                            }
                        }
                        // b ends at d (shorter), then a ends at d.
                        b[ba + d] = 0;
                        check(pa, &a[aa..], pb, &b[ba..], None, &|| format!("b ends at {d}, align {aa}/{ba}, len {len}, kernel {kernel}"));
                        check(pa, &a[aa..], pb, &b[ba..], Some(d + 1), &|| format!("b ends at {d}, n {}, align {aa}/{ba}, len {len}", d + 1));
                        (a[aa + d], b[ba + d]) = keep;
                    }
                }
            }
        }
    }
    selftest::set_scan_avx2(false);
}

/// xorshift64*.
fn next(state: &mut u64) -> u64 {
    *state ^= *state >> 12;
    *state ^= *state << 25;
    *state ^= *state >> 27;
    state.wrapping_mul(0x2545_F491_4F6C_DD1D)
}

#[test]
fn casecmp_equals_the_path_of_before_on_random_mixed_text() {
    // Clauses: s4_3_c_1 (CASECMP-K1). Seeded; letters, the bytes beside them, digits, and bytes
    // of 128 and above, in pairs that differ only in case at random bytes and in random ones.
    let _g = level();
    const ALPHA: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ@[`{ 0189_";
    for kernel in paths() {
        assert!(selftest::set_casecmp_kernel(kernel));
        let mut st = 0x9E37_79B9_7F4A_7C15_u64;
        let mut a = vec![0u8; 400];
        let mut b = vec![0u8; 400];
        for round in 0..20_000u32 {
            let len = (next(&mut st) % 150) as usize;
            let (aa, ba) = ((next(&mut st) % 32) as usize, (next(&mut st) % 32) as usize);
            let high = round % 4 == 0;
            for i in 0..len {
                let r = next(&mut st);
                let c = if high && r % 11 == 0 { 0x80 + (r >> 8) as u8 % 128 } else { ALPHA[(r >> 8) as usize % ALPHA.len()] };
                a[aa + i] = c;
                b[ba + i] = match (r >> 40) % 8 {
                    0 => c ^ 0x20,
                    1 if round % 3 == 0 => ALPHA[(r >> 20) as usize % ALPHA.len()],
                    _ => c.to_ascii_uppercase(),
                };
                // A NUL inside the data would end both: keep the strings NUL-free until `len`.
            }
            a[aa + len] = 0;
            b[ba + len + usize::from(round % 5 == 0)] = 0;
            if round % 5 == 0 {
                b[ba + len] = b'q';
            }
            let (pa, pb) = (a.as_ptr() as usize + aa, b.as_ptr() as usize + ba);
            let n = (next(&mut st) % 170) as usize;
            check(pa, &a[aa..], pb, &b[ba..], None, &|| format!("random {round}, a {:x?} b {:x?}", &a[aa..aa + len + 1], &b[ba..ba + len + 2]));
            check(pa, &a[aa..], pb, &b[ba..], Some(n), &|| format!("random {round}, n {n}, a {:x?} b {:x?}", &a[aa..aa + len + 1], &b[ba..ba + len + 2]));
        }
    }
    selftest::set_scan_avx2(false);
}

/// Two pages; the second unmapped (no access). Returns the mapping and the first page's base.
fn guarded() -> (liella_kernel::mem::Region, liella_kernel::mem::Region, usize) {
    let r = CSys.mmap(0, 2 * PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, None, 0).unwrap();
    let (lo, mut hi) = r.split_at(PAGE).ok().unwrap();
    CSys.mprotect(&mut hi, PROT_NONE).unwrap();
    let base = lo.addr();
    (lo, hi, base)
}

/// Make the page at `base` hold `v` (PAGE bytes).
fn put(base: usize, v: &[u8]) {
    s::memcpy(base as *mut c_void, v.as_ptr().cast(), PAGE);
}

#[test]
fn casecmp_never_reads_past_the_pages_holding_the_strings_or_the_bound() {
    // Clauses: s4_3_c_1 (CASECMP-K1). Both strings end on the last byte of a page whose next page
    // is unmapped, at every pair of lengths (every pair of page offsets): equal up to case,
    // differing in the last byte, one a prefix of the other; and strncasecmp on text with NO
    // terminator whose bound is the page end.
    let _g = level();
    for kernel in paths() {
        assert!(selftest::set_casecmp_kernel(kernel));
        let (lo, hi, base) = guarded();
        let (lo2, hi2, base2) = guarded();
        let mut ma: Vec<u8> = (0..PAGE).map(|i| b'A' + (i % 26) as u8).collect();
        let mut mb: Vec<u8> = ma.iter().map(u8::to_ascii_lowercase).collect();
        ma[PAGE - 1] = 0;
        mb[PAGE - 1] = 0;
        put(base, &ma);
        put(base2, &mb);
        let at = |len: usize| PAGE - 1 - len;
        for la in 0..200usize {
            for lb in [la.saturating_sub(17), la.saturating_sub(1), la, la + 1, la + 16, la + 64] {
                let (pa, pb) = (base + at(la), base2 + at(lb));
                check(pa, &ma[at(la)..], pb, &mb[at(lb)..], None, &|| format!("page end, lengths {la}/{lb}, kernel {kernel}"));
                check(pb, &mb[at(lb)..], pa, &ma[at(la)..], None, &|| format!("page end swapped, lengths {lb}/{la}, kernel {kernel}"));
                check(pa, &ma[at(la)..], pb, &mb[at(lb)..], Some(la.min(lb) + 3), &|| format!("page end, n, lengths {la}/{lb}"));
            }
            if la > 0 {
                let keep = ma[PAGE - 2];
                ma[PAGE - 2] = b'#';
                put(base, &ma);
                check(base + at(la), &ma[at(la)..], base2 + at(la), &mb[at(la)..], None, &|| format!("last byte differs, length {la}, kernel {kernel}"));
                ma[PAGE - 2] = keep;
                put(base, &ma);
            }
            // Unterminated text to the page end, bounded by it: n bytes remain, none beyond.
            ma[PAGE - 1] = b'k';
            mb[PAGE - 1] = b'K';
            put(base, &ma);
            put(base2, &mb);
            check(base + at(la), &ma[at(la)..], base2 + at(la), &mb[at(la)..], Some(la + 1), &|| format!("unterminated to the page end, n {}, kernel {kernel}", la + 1));
            ma[PAGE - 1] = 0;
            mb[PAGE - 1] = 0;
            put(base, &ma);
            put(base2, &mb);
        }
        CSys.munmap(hi2).unwrap();
        CSys.munmap(lo2).unwrap();
        CSys.munmap(hi).unwrap();
        CSys.munmap(lo).unwrap();
    }
    selftest::set_scan_avx2(false);
}

#[test]
fn casecmp_is_right_on_long_text_across_a_page_boundary_inside_the_strings() {
    // Clauses: s4_3_c_1 (CASECMP-K1). The 64-byte steps and the 32-byte and byte steps next to a
    // page end: strings of 300 bytes starting just before the end of the first of two mapped pages
    // (the third is unmapped), against copies at other offsets, with a difference at every position.
    let _g = level();
    for kernel in paths() {
        assert!(selftest::set_casecmp_kernel(kernel));
        let map = |_: u8| {
            let r = CSys.mmap(0, 3 * PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, None, 0).unwrap();
            let (lo, mut hi) = r.split_at(2 * PAGE).ok().unwrap();
            CSys.mprotect(&mut hi, PROT_NONE).unwrap();
            let base = lo.addr();
            (lo, hi, base)
        };
        let (lo, hi, base) = map(0);
        let (lo2, hi2, base2) = map(1);
        let len = 300usize;
        for (sa, sb) in [(1usize, 1usize), (3, 40), (37, 5), (64, 64), (100, 99), (129, 1), (130, 31), (299, 2)] {
            for d in (0..len).step_by(1) {
                let mut ma = vec![b'x'; 2 * PAGE];
                let mut mb = vec![b'X'; 2 * PAGE];
                let (oa, ob) = (PAGE - sa, PAGE - sb);
                for i in 0..len {
                    ma[oa + i] = b'a' + (i % 26) as u8;
                    mb[ob + i] = b'A' + (i % 26) as u8;
                }
                ma[oa + len] = 0;
                mb[ob + len] = 0;
                for (x, y) in [(b'm', b'm'), (b'@', b'`'), (b'z', b'{'), (0xC9, b'a')] {
                    let (ka, kb) = (ma[oa + d], mb[ob + d]);
                    if (x, y) != (b'm', b'm') {
                        ma[oa + d] = x;
                        mb[ob + d] = y;
                    }
                    s::memcpy(base as *mut c_void, ma.as_ptr().cast(), 2 * PAGE);
                    s::memcpy(base2 as *mut c_void, mb.as_ptr().cast(), 2 * PAGE);
                    let w = || format!("starts -{sa}/-{sb}, diff {x:#x}/{y:#x} at {d}, kernel {kernel}");
                    check(base + oa, &ma[oa..], base2 + ob, &mb[ob..], None, &w);
                    check(base + oa, &ma[oa..], base2 + ob, &mb[ob..], Some(d + 1), &|| format!("{}, n {}", w(), d + 1));
                    ma[oa + d] = ka;
                    mb[ob + d] = kb;
                }
            }
        }
        CSys.munmap(hi2).unwrap();
        CSys.munmap(lo2).unwrap();
        CSys.munmap(hi).unwrap();
        CSys.munmap(lo).unwrap();
    }
    selftest::set_scan_avx2(false);
}

#[test]
fn casecmp_takes_the_kernel_for_ascii_and_the_measured_path_for_bytes_of_128_and_up() {
    // Clauses: s4_3_c_1 (CASECMP-K1, CASECMP). With a kernel (SSE2 always, AVX2 where present) the
    // kernel decides plain-ASCII calls (a counter, self-test builds); a byte of 128 or more at the
    // difference sends the call down the measured path; with no kernel forced nothing reaches one.
    // The kernel also decides when the deciding bytes are letters (STRCASECMP-SHORT-2): a kernel
    // that sent those to the measured path would give the same answers, only slower.
    let _g = level();
    let (a, b) = (b"Hello World, this is a longer ASCII line\0", b"hELLO wORLD, THIS IS A LONGER ascii line\0");
    let (c, d) = (b"Hello \xC9 World\0", b"hello \xE9 world\0");
    let (e, f) = (b"Hello Worle\0", b"hELLO wORLD\0");
    let (g, h) = (b"abc[\0", b"ABCz\0");
    let run = |x: &[u8], y: &[u8]| s::strcasecmp(x.as_ptr() as *const c_char, y.as_ptr() as *const c_char);
    for kernel in paths() {
        assert!(selftest::set_casecmp_kernel(kernel));
        let (k0, m0) = selftest::casecmp_paths();
        assert_eq!(run(a, b), 0);
        assert_eq!(run(c, d), before(c, d, usize::MAX));
        assert_eq!(run(e, f), before(e, f, usize::MAX));
        assert_eq!(run(g, h), before(g, h, usize::MAX));
        let (k1, m1) = selftest::casecmp_paths();
        if kernel == 0 {
            assert_eq!((k1 - k0, m1 - m0), (0, 4), "no kernel forced: every call takes the measured path");
        } else {
            assert_eq!((k1 - k0, m1 - m0), (3, 1), "kernel {kernel} for ASCII (letters deciding too), measured path for 0xC9 vs 0xE9");
            // An ASCII difference or end in any block of any step is decided by the kernel itself
            // (PERF-STRCASECMP-SSE2): a stop index placed too early lands on equal bytes, which the
            // kernel hands to the measured path: right answers, but slow, so it is counted here.
            for p in 0..140 {
                let mut x = vec![b'q'; p];
                let mut y = vec![b'Q'; p];
                let (mut u, mut w) = (x.clone(), y.clone());
                x.extend_from_slice(b"a\0");
                y.extend_from_slice(b"B\0");
                u.push(0);
                w.push(0);
                let (k2, m2) = selftest::casecmp_paths();
                assert_eq!(run(&x, &y), -1, "kernel {kernel}, difference at {p}");
                assert_eq!(run(&u, &w), 0, "kernel {kernel}, both end at {p}");
                assert_eq!(selftest::casecmp_paths(), (k2 + 2, m2), "kernel {kernel} decides an ASCII stop at {p}");
            }
        }
    }
    // The CPU's own choice (the startup decision, no AVX2 needed) is a kernel, never the measured path.
    assert!(selftest::set_scan_avx2(false));
    let (k2, m2) = selftest::casecmp_paths();
    assert_eq!(run(a, b), 0);
    assert_eq!(selftest::casecmp_paths(), (k2 + 1, m2), "the baseline build takes the SSE2 kernel");
}

#[test]
fn casecmp_keeps_the_error_and_zero_bound_behaviour() {
    // Clauses: s4_3_c_1 (CASECMP-K1). n = 0 compares nothing, so even NULL is equal; a bound
    // shorter than the difference sees only the equal prefix.
    let _g = level();
    for kernel in paths() {
        assert!(selftest::set_casecmp_kernel(kernel));
        assert_eq!(s::strncasecmp(std::ptr::null(), std::ptr::null(), 0), 0);
        assert_eq!(s::strncasecmp(c"abc".as_ptr(), c"ABD".as_ptr(), 2), 0);
        assert_eq!(s::strncasecmp(c"abc".as_ptr(), c"ABD".as_ptr(), 3), -1);
        assert_eq!(s::strcasecmp(c"abc".as_ptr(), c"AB".as_ptr()), 1);
        assert_eq!(s::strcasecmp(c"".as_ptr(), c"".as_ptr()), 0);
        assert_eq!(s::strcasecmp(c"Z".as_ptr(), c"[".as_ptr()), 1);
        assert_eq!(s::strcasecmp(c"z".as_ptr(), c"{".as_ptr()), -1);
        assert_eq!(s::strcasecmp(c"@".as_ptr(), c"a".as_ptr()), -1);
        assert_eq!(s::strcasecmp(c"`".as_ptr(), c"A".as_ptr()), -1);
    }
    selftest::set_scan_avx2(false);
}
