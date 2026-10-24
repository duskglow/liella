//! The small-size compare kernel (`hot`, D-4.23, owner ruling 2026-10-08) against a byte loop,
//! through the exports `memcmp`, `bcmp` and `__memcmpeq`: every size 0 to 256 (the kernel's
//! classes and the checked path above them), alignments of both operands in a 16-byte window,
//! every first-difference position, and byte pairs on both sides of 0x80 (the order is that of
//! `unsigned char`). Every byte after the first difference differs the opposite way, so a kernel
//! that lets a later byte decide gets the sign wrong; and a lone difference catches a skipped byte. Guard pages catch a read past either end.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use liella_abi::ctypes::{c_int, c_void};
use liella_boundary::posix::s04_strings as s;
use liella_boundary::selftest::{self, ChildEnd};
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{MAP_ANONYMOUS, MAP_PRIVATE, PROT_NONE, PROT_READ, PROT_WRITE};

const PAGE: usize = 4096;
const MAX: usize = 256;
const WIN: usize = 16;

/// The kernel builds are one process-wide flag: the tests that force one take turns.
static BUILD: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Run `test` with each build of the kernels forced (D-4.25): the SSE2 baseline, then AVX2 where
/// the CPU has it (every gate machine does; a CPU without it runs the baseline only and says so).
/// The build is printed before each run, so a failure's captured output names it. The startup
/// decision is put back afterwards. Nothing in this binary changes text during a call, so no
/// compare may take the D-4.21 rescan: a kernel that reports a first difference too low (on an
/// equal byte) would be answered right by the rescan, and only this count shows it.
fn each_build(test: impl Fn()) {
    let _turn = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let rescans = selftest::rescans();
    let avx2 = liella_boundary::cpu::avx2();
    let before = selftest::kernel_avx2();
    // Each build with NULL folding on (the direct entries' head probe in fold.S, MEMCMP-FOLD) and
    // off (every size through the checked entry); folding on is then left as the gate machines'
    // startup guard decides it for a process (vm.mmap_min_addr >= 64 KiB).
    for fold in [true, false] {
        assert!(selftest::set_null_fold(fold), "NULL folding cannot be forced to {fold}");
        assert!(selftest::set_kernel_avx2(false));
        eprintln!("build sse2 fold {fold}");
        test();
        if avx2 {
            assert!(selftest::set_kernel_avx2(true), "the CPU has AVX2 but its build cannot be forced");
            assert!(selftest::kernel_avx2());
            eprintln!("build avx2 fold {fold}");
            test();
        } else {
            eprintln!("small_compare: no AVX2 on this CPU: the SSE2 build only");
        }
    }
    assert!(selftest::set_null_fold(true), "NULL folding cannot be put back on");
    assert!(selftest::set_kernel_avx2(before));
    assert_eq!(selftest::rescans(), rescans, "a compare took the D-4.21 rescan on stable text: a first difference reported too low");
}

/// The oracle: the first differing byte as `unsigned char` decides; -1, 0 or 1.
fn oracle(a: &[u8], b: &[u8]) -> c_int {
    for (x, y) in a.iter().zip(b) {
        if x != y {
            return if x > y { 1 } else { -1 };
        }
    }
    0
}

type CmpFn = fn(*const c_void, *const c_void, usize) -> c_int;

fn comparers() -> [(&'static str, CmpFn); 3] {
    [("memcmp", |a, b, n| s::memcmp(a, b, n)), ("bcmp", |a, b, n| s::bcmp(a, b, n)), ("__memcmpeq", |a, b, n| s::__memcmpeq(a, b, n))]
}

/// Byte pairs (a, b) at the first difference: both orders, across 0x80 and at the extremes.
const PAIRS: [(u8, u8); 8] = [(0x00, 0x01), (0x01, 0x00), (0x7F, 0x80), (0x80, 0x7F), (0x00, 0xFF), (0xFF, 0x00), (0x80, 0x81), (0xFE, 0xFF)];

/// Fills `a[ao..ao+n]` and `b[bo..bo+n]`: equal before `p`, the pair at `p`, and every byte after
/// `p` different the other way round if `opposite`, else equal (`None`: all equal).
fn build(a: &mut [u8], b: &mut [u8], ao: usize, bo: usize, n: usize, diff: Option<(usize, u8, u8)>, opposite: bool) {
    for i in 0..n {
        let v = (i.wrapping_mul(37).wrapping_add(11) % 256) as u8;
        a[ao + i] = v;
        b[bo + i] = v;
    }
    if let Some((p, x, y)) = diff {
        a[ao + p] = x;
        b[bo + p] = y;
        let (lo, hi) = if x > y { (0x00, 0xFF) } else { (0xFF, 0x00) };
        for i in (p + 1..n).filter(|_| opposite) {
            a[ao + i] = lo;
            b[bo + i] = hi;
        }
    }
}

fn check(a: &[u8], b: &[u8], ao: usize, bo: usize, n: usize, what: &str) {
    let want = oracle(&a[ao..ao + n], &b[bo..bo + n]);
    for (name, f) in comparers() {
        let got = f(a[ao..].as_ptr().cast(), b[bo..].as_ptr().cast(), n);
        // bcmp and __memcmpeq report equality only (§4.3; D-4.41, D-4.43): 0 or 1, never an order.
        let want = if name != "memcmp" { c_int::from(want != 0) } else { want };
        assert_eq!(got, want, "{name} n={n} a+{ao} b+{bo} {what}");
    }
}

/// The first-difference positions tried at every alignment pair (all of them at a few).
fn edge_positions(n: usize) -> Vec<usize> {
    let mut v: Vec<usize> = [0, 1, 2, 3, 4, 7, 8, 15, 16, 17, 31, 32, 33, 47, 48, 63, 64, 65, 79, 80, 95, 96, 111, 112, 127, 128, n / 2]
        .into_iter()
        .chain((1..=17).map(|k| n.wrapping_sub(k)))
        .filter(|&p| p < n)
        .collect();
    v.sort_unstable();
    v.dedup();
    v
}

fn compares_match_a_byte_loop_at_every_size_alignment_and_difference(sizes: core::ops::RangeInclusive<usize>) {
    // Clauses: s4_3_c_1, s4_3_c_2, s4_3_c_3.
    let mut a = vec![0u8; MAX + WIN];
    let mut b = vec![0u8; MAX + WIN];
    for n in sizes {
        let all_aligns = n <= 130; // every alignment pair up to past the 65-128 class (MEM-K5)
        for ao in 0..WIN {
            for bo in 0..WIN {
                let every_position = matches!((ao, bo), (0, 0) | (3, 9) | (15, 1));
                if !all_aligns && !every_position && (ao + bo) % 7 != 0 {
                    continue;
                }
                build(&mut a, &mut b, ao, bo, n, None, false);
                check(&a, &b, ao, bo, n, "equal");
                let positions: Vec<usize> = if every_position { (0..n).collect() } else { edge_positions(n) };
                for &p in &positions {
                    for &(x, y) in &PAIRS {
                        build(&mut a, &mut b, ao, bo, n, Some((p, x, y)), true);
                        check(&a, &b, ao, bo, n, &format!("first difference at {p}: {x:#04x} vs {y:#04x}"));
                    }
                    // The only difference: a kernel that skips a byte calls these equal.
                    for (x, y) in [(0x7F, 0x80), (0x80, 0x7F)] {
                        build(&mut a, &mut b, ao, bo, n, Some((p, x, y)), false);
                        check(&a, &b, ao, bo, n, &format!("only difference at {p}: {x:#04x} vs {y:#04x}"));
                    }
                }
            }
        }
    }
}

fn compares_stay_inside_regions_that_touch_unmapped_pages() {
    // Clause: s4_3_c_1.
    // A read one byte past either end faults: the operands sit at the very start and at the very
    // end of a mapping fenced by PROT_NONE pages.
    let r = CSys.mmap(0, 4 * PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, None, 0).unwrap();
    let (mut lo, rest) = r.split_at(PAGE).ok().unwrap();
    let (mid, mut hi) = rest.split_at(2 * PAGE).ok().unwrap();
    CSys.mprotect(&mut lo, PROT_NONE).unwrap();
    CSys.mprotect(&mut hi, PROT_NONE).unwrap();
    let (first, second) = (mid.addr(), mid.addr() + PAGE);
    for n in 0..=MAX {
        for (a_at, b_at) in [(first, second + PAGE - n), (second + PAGE - n, first), (first + PAGE - n, second)] {
            for diff in [None, Some(0), n.checked_sub(1)].into_iter().filter(|d| d.is_none_or(|i| i < n)) {
                s::memset(a_at as *mut c_void, 0x41, n);
                s::memset(b_at as *mut c_void, 0x41, n);
                if let Some(i) = diff {
                    s::memset((b_at + i) as *mut c_void, 0xC1, 1);
                }
                let want = if diff.is_some() { -1 } else { 0 };
                for (name, f) in comparers() {
                    // bcmp and __memcmpeq report equality only (section 4.3; D-4.41, D-4.43): 0 or 1.
                    let want = if name != "memcmp" { c_int::from(want != 0) } else { want };
                    assert_eq!(f(a_at as *const c_void, b_at as *const c_void, n), want, "{name} {n} {diff:?} at the fence");
                }
            }
        }
    }
    for r in [lo, mid, hi] {
        CSys.munmap(r).unwrap();
    }
}

#[test]
fn zero_length_compares_accept_null_and_are_equal() {
    // Clause: s4_3_c_1 (n == 0 compares nothing; NULL is then accepted, as glibc does).
    let null = core::ptr::null::<c_void>();
    for (name, f) in comparers() {
        assert_eq!(f(null, null, 0), 0, "{name}");
    }
}

/// GATE-SPEED: the sweep runs in size ranges (one test each, below); `SIZE_SHARDS` tiles 0..=MAX
/// (checked by `the_size_shards_tile_0_to_max`), so the coverage is the whole sweep's.
const SIZE_SHARDS: [(usize, usize); 4] = [(0, 64), (65, 130), (131, 190), (191, MAX)];

fn sweep_shard(i: usize) {
    // Clauses: s4_3_c_1, s4_3_c_2, s4_3_c_3 (each build of the kernels, D-4.25).
    let (lo, hi) = SIZE_SHARDS[i];
    each_build(|| compares_match_a_byte_loop_at_every_size_alignment_and_difference(lo..=hi));
}

#[test]
fn compares_match_a_byte_loop_at_every_size_alignment_and_difference_in_each_build_0() {
    // no spec clause: a shard of the compare kernels byte-loop oracle sweep (D-4 kernels), not a spec clause.
    sweep_shard(0);
}

#[test]
fn compares_match_a_byte_loop_at_every_size_alignment_and_difference_in_each_build_1() {
    // no spec clause: a shard of the compare kernels byte-loop oracle sweep (D-4 kernels), not a spec clause.
    sweep_shard(1);
}

#[test]
fn compares_match_a_byte_loop_at_every_size_alignment_and_difference_in_each_build_2() {
    // no spec clause: a shard of the compare kernels byte-loop oracle sweep (D-4 kernels), not a spec clause.
    sweep_shard(2);
}

#[test]
fn compares_match_a_byte_loop_at_every_size_alignment_and_difference_in_each_build_3() {
    // no spec clause: a shard of the compare kernels byte-loop oracle sweep (D-4 kernels), not a spec clause.
    sweep_shard(3);
}

#[test]
fn the_size_shards_tile_0_to_max() {
    // no spec clause: a shard of the compare kernels byte-loop oracle sweep (D-4 kernels), not a spec clause.
    let mut next = 0;
    for (lo, hi) in SIZE_SHARDS {
        assert_eq!(lo, next);
        assert!(hi >= lo);
        next = hi + 1;
    }
    assert_eq!(next, MAX + 1);
}

#[test]
fn compares_stay_inside_regions_that_touch_unmapped_pages_in_each_build() {
    // Clauses: s4_3_c_1 (each build of the kernels, D-4.25).
    each_build(compares_stay_inside_regions_that_touch_unmapped_pages);
}

/// Sparse differences for the two-block classes (17-32: 16-byte blocks, 33-64: 32-byte blocks):
/// the first at `p`, an opposite one at `r` (both before the second block starts), and one in the
/// second block at the same offset as `r` in the first. A kernel that took the first difference
/// from the second block's mask would read the pair at `r` and return its (wrong) order.
fn compares_take_the_first_difference_from_the_first_block() {
    let mut a = vec![0u8; MAX + WIN];
    let mut b = vec![0u8; MAX + WIN];
    for n in 17..=64 {
        let w = if n > 32 { 32 } else { 16 };
        for (ao, bo) in [(0, 0), (3, 9)] {
            for p in 8..n - w {
                for r in p + 1..n - w {
                    for (x, y) in [(0x10, 0x20), (0x20, 0x10)] {
                        build(&mut a, &mut b, ao, bo, n, Some((p, x, y)), false);
                        for i in [r, n - w + r] {
                            a[ao + i] = y;
                            b[bo + i] = x;
                        }
                        check(&a, &b, ao, bo, n, &format!("differences at {p}, {r} and {}", n - w + r));
                    }
                }
            }
        }
    }
}

#[test]
fn compares_take_the_first_difference_from_the_first_block_in_each_build() {
    // Clauses: s4_3_c_1, s4_3_c_2 (each build of the kernels, D-4.25; MEMCMP-EQ64 block masks).
    each_build(compares_take_the_first_difference_from_the_first_block);
}

/// The call the NULL child makes (set before the fork; the child gets its own copy).
static NULL_CASE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static NULL_FOLD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static NULL_AVX2: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// memcmp, bcmp, __memcmpeq, each with the NULL first or second.
const NULL_CALLS: usize = 6;

fn null_call(k: &CSys) -> i32 {
    use std::sync::atomic::Ordering::SeqCst;
    // No core dump (a crash reporter such as apport took about a second per child).
    if k.prctl_int(liella_kernel::ops::PrctlInt::SetDumpable, 0, 0).is_err() {
        return 4;
    }
    let (case, fold) = (NULL_CASE.load(SeqCst), NULL_FOLD.load(SeqCst));
    if !selftest::set_null_fold(fold) || !selftest::set_kernel_avx2(NULL_AVX2.load(SeqCst)) {
        return 3;
    }
    let (call, n) = (case % NULL_CALLS, case / NULL_CALLS);
    let buf = [0x5Au8; 256];
    let (p, null) = (buf.as_ptr().cast::<c_void>(), core::ptr::null::<c_void>());
    let (_, f) = comparers()[call / 2];
    let r = if call % 2 == 0 { f(null, p, n) } else { f(p, null, n) };
    std::hint::black_box(r);
    0
}

#[test]
fn null_faults_in_the_folded_compare_sizes_and_aborts_in_every_other() {
    // Clause: s4_3_c_1 with S6 (NULL with n > 0 is the EFAULT abort) and the compare fold
    // (MEMCMP-FOLD, owner ruling 2026-10-10): with folding on, NULL with 8 <= n <= 64 dies by SIGSEGV
    // on page zero (the head probe's load in fold.S); 1-7, 65 and up (the 65-128 class of MEM-K5
    // included) and every size with folding off take the S6 abort, in each build.
    use std::sync::atomic::Ordering::SeqCst;
    let _turn = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut runs = vec![(false, false), (true, false)];
    if liella_boundary::cpu::avx2() {
        // Folding off with AVX2 on too: the 33-64 gate (liella_machine_cmp_wide) must stay shut.
        runs.extend([(true, true), (false, true)]);
    }
    for (fold, avx2) in runs {
        NULL_FOLD.store(fold, SeqCst);
        NULL_AVX2.store(avx2, SeqCst);
        for n in [1, 3, 7, 8, 15, 16, 32, 33, 63, 64, 65, 96, 127, 128, 129, 4096] {
            for call in 0..NULL_CALLS {
                NULL_CASE.store(n * NULL_CALLS + call, SeqCst);
                let end = selftest::in_child_quiet(null_call).unwrap();
                let want = if fold && (8..=64).contains(&n) { 11 } else { 6 };
                assert_eq!(end, ChildEnd::Signaled(want), "fold {fold} avx2 {avx2} call {call} n {n}");
            }
        }
    }
}

#[test]
fn ordered_long_class_orders_the_first_difference_at_every_position_in_each_build() {
    // Clauses: s4_3_c_1, s4_3_c_2. The compare kernel's long class (MEMCMP-ORDER, 129 to 2048 bytes:
    // chunks from the start, the first difference read from the chunk's bitmaps) and the scanner
    // past it: at every position the first difference decides, the bytes after it ordered the
    // other way round; a lone difference is never missed.
    each_build(|| {
        let mut a = vec![0u8; 2100 + WIN];
        let mut b = vec![0u8; 2100 + WIN];
        for n in (129..=400).chain([511, 512, 513, 700, 1023, 1024, 1025, 2047, 2048, 2049, 2100]) {
            for (ao, bo) in [(0, 0), (3, 9), (15, 1)] {
                build(&mut a, &mut b, ao, bo, n, None, false);
                check(&a, &b, ao, bo, n, "equal");
                for p in 0..n {
                    for (x, y, opposite) in [(0x7F, 0x80, true), (0x80, 0x7F, true), (0x01, 0x00, false)] {
                        build(&mut a, &mut b, ao, bo, n, Some((p, x, y)), opposite);
                        check(&a, &b, ao, bo, n, &format!("first difference at {p}: {x:#04x} vs {y:#04x}"));
                    }
                }
            }
        }
    });
}

#[test]
fn memcmpeq_long_class_finds_a_lone_difference_at_every_position_in_each_build() {
    // Clause: s4_3_c_3. The equality kernel's long class (D-4.41): tail chunk first, then chunks
    // from the start; one differing byte anywhere, at three alignment pairs, must be seen.
    each_build(|| {
        let mut a = vec![0u8; 1100 + WIN];
        let mut b = vec![0u8; 1100 + WIN];
        for n in (120..=700).chain([1023, 1024, 1025, 1100]) {
            for (ao, bo) in [(0, 0), (3, 9), (15, 1)] {
                build(&mut a, &mut b, ao, bo, n, None, false);
                assert_eq!(s::__memcmpeq(a[ao..].as_ptr().cast(), b[bo..].as_ptr().cast(), n), 0, "equal n={n} a+{ao} b+{bo}");
                for p in 0..n {
                    build(&mut a, &mut b, ao, bo, n, Some((p, 0x7F, 0x80)), false);
                    let got = s::__memcmpeq(a[ao..].as_ptr().cast(), b[bo..].as_ptr().cast(), n);
                    assert_eq!(got, 1, "n={n} a+{ao} b+{bo} lone difference at {p}");
                }
            }
        }
    });
}

#[test]
fn mid_and_long_classes_find_the_first_difference_at_every_size_to_2049_in_each_build() {
    // Clauses: s4_3_c_1, s4_3_c_2, s4_3_c_3. The direct entries' AVX2 classes above 128 bytes
    // (MEMCMP-MID-4: the 32-byte head block, 128-byte chunks from byte 32, the last chunk at n - 128
    // overlapping the one before; equality: the last chunk first) and the checked path past 2048:
    // every size, the first difference on both sides of every 32-byte block boundary, in the head,
    // in and around the last chunk; memcmp with every later byte ordered the other way round,
    // bcmp/__memcmpeq with a lone difference. Both operands also end at a PROT_NONE page.
    each_build(|| {
        let r = CSys.mmap(0, 4 * PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, None, 0).unwrap();
        let (mut lo, rest) = r.split_at(PAGE).ok().unwrap();
        let (mid, mut hi) = rest.split_at(2 * PAGE).ok().unwrap();
        CSys.mprotect(&mut lo, PROT_NONE).unwrap();
        CSys.mprotect(&mut hi, PROT_NONE).unwrap();
        let (first, end) = (mid.addr(), mid.addr() + 2 * PAGE);
        let mut a = vec![0u8; 2049 + WIN];
        let mut b = vec![0u8; 2049 + WIN];
        for n in 129..=2049 {
            let (ao, bo) = [(0, 0), (3, 9), (15, 1), (1, 0)][n % 4];
            build(&mut a, &mut b, ao, bo, n, None, false);
            check(&a, &b, ao, bo, n, "equal");
            let mut ps: Vec<usize> = (0..n / 32 + 1).flat_map(|k| [32 * k, 32 * k + 1, (32 * k).wrapping_sub(1)]).collect();
            ps.extend((0..9).chain((1..=34).map(|k| n.wrapping_sub(k))).chain((0..5).map(|k| n.wrapping_sub(126 + k))));
            ps.retain(|&p| p < n);
            ps.sort_unstable();
            ps.dedup();
            for &p in &ps {
                for (x, y) in [(0x7F, 0x80), (0x80, 0x7F)] {
                    build(&mut a, &mut b, ao, bo, n, Some((p, x, y)), true);
                    check(&a, &b, ao, bo, n, &format!("first difference at {p}: {x:#04x} vs {y:#04x}"));
                    build(&mut a, &mut b, ao, bo, n, Some((p, x, y)), false);
                    check(&a, &b, ao, bo, n, &format!("only difference at {p}: {x:#04x} vs {y:#04x}"));
                }
            }
            for (a_at, b_at) in [(first, end - n), (end - n, first)] {
                for diff in [None, Some(0), Some(n / 2), Some(n - 1)] {
                    s::memset(a_at as *mut c_void, 0x41, n);
                    s::memset(b_at as *mut c_void, 0x41, n);
                    if let Some(i) = diff {
                        s::memset((b_at + i) as *mut c_void, 0xC1, 1);
                    }
                    let want = if diff.is_some() { -1 } else { 0 };
                    for (name, f) in comparers() {
                        let want = if name != "memcmp" { c_int::from(want != 0) } else { want };
                        assert_eq!(f(a_at as *const c_void, b_at as *const c_void, n), want, "{name} {n} {diff:?} at the fence");
                    }
                }
            }
        }
        for r in [lo, mid, hi] {
            CSys.munmap(r).unwrap();
        }
    });
}
