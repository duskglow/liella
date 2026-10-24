//! The small-size copy and fill kernels (`hot`, D-4.22, owner ruling 2026-10-08) against a byte
//! loop, through the exports and the library's internal copy: every size 0 to 257 (the kernel's classes, the 65-256 assembly classes and
//! the first size above them), every alignment of source and destination in a 32-byte window, and every
//! overlap shift up to 40 bytes in both directions plus whole-length shifts. Sentinels around the
//! destination catch a write outside it; guard pages catch a read or write past either end.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use liella_abi::ctypes::c_void;
use liella_boundary::posix::s04_strings as s;
use liella_boundary::selftest::{self, opaque, ChildEnd};
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{MAP_ANONYMOUS, MAP_PRIVATE, PROT_NONE, PROT_READ, PROT_WRITE};

const PAGE: usize = 4096;
const MAX: usize = 257;
const PAD: usize = 64;

/// The kernel builds are one process-wide flag: the tests that force one take turns.
static BUILD: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Run `test` with each build of the kernels forced (D-4.25): the SSE2 baseline, then AVX2 where
/// the CPU has it (every gate machine does; a CPU without it runs the baseline only and says so),
/// each with the explicit NULL checks (the startup guard's "page zero may be mapped" build) and
/// with NULL folding (D-4.22). The startup decisions are put back afterwards.
fn each_build(test: fn(&str)) {
    let _turn = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let avx2 = liella_boundary::cpu::avx2();
    let (before, fold_before) = (selftest::kernel_avx2(), selftest::null_fold());
    let mut builds = vec![false];
    if avx2 {
        builds.push(true);
    } else {
        eprintln!("small_copy: no AVX2 on this CPU: the SSE2 build only");
    }
    for b in builds {
        assert!(selftest::set_kernel_avx2(b), "the CPU has AVX2 but its build cannot be forced");
        assert_eq!(selftest::kernel_avx2(), b);
        // The checked entries once (the AVX2 flag changes only the 65-256 copy classes, which the
        // checked and folded entries reach through the same assembly).
        if !b {
            assert!(selftest::set_null_fold(false));
            test("sse2 checked");
        }
        // Every gate machine has vm.mmap_min_addr >= 64 KiB (the Linux default): folding must be
        // allowed there, so this is an assertion, not a skip.
        assert!(selftest::set_null_fold(true), "vm.mmap_min_addr below 64 KiB or unreadable: NULL folding refused");
        test(if b { "avx2 folded" } else { "sse2 folded" });
    }
    assert!(selftest::set_kernel_avx2(before));
    assert!(selftest::set_null_fold(fold_before));
}

fn oracle_move(buf: &mut [u8], d: usize, s: usize, n: usize) {
    let tmp: Vec<u8> = (0..n).map(|i| buf[s + i]).collect();
    for (i, b) in tmp.into_iter().enumerate() {
        buf[d + i] = b;
    }
}

fn pattern(len: usize, seed: usize) -> Vec<u8> {
    (0..len).map(|i| (i.wrapping_mul(131).wrapping_add(seed * 7 + i / 255) % 251) as u8 + 1).collect()
}

type MoveFn = fn(*mut c_void, *const c_void, usize) -> *mut c_void;

fn movers() -> [(&'static str, MoveFn); 7] {
    [
        ("memcpy", |d, s_, n| opaque::memcpy(d, s_, n)),
        // The library's own copies (D-X.15, COPY-INTERNAL): no C-contract checks, the same kernels.
        ("internal", selftest::internal_memmove),
        ("memmove", |d, s_, n| opaque::memmove(d, s_, n)),
        ("__memcpy_chk", |d, s_, n| opaque::__memcpy_chk(d, s_, n, n)),
        ("__memmove_chk", |d, s_, n| s::__memmove_chk(d, s_, n, n + 1)),
        // mempcpy's direct entry (WL-HOT-SEQ2) returns d + n: taken back to d, so the callers'
        // return check holds it to exactly d + n.
        ("mempcpy", |d, s_, n| s::mempcpy(d, s_, n).cast::<u8>().wrapping_sub(n).cast()),
        ("__mempcpy_chk", |d, s_, n| s::__mempcpy_chk(d, s_, n, n).cast::<u8>().wrapping_sub(n).cast()),
    ]
}

fn copies_match_a_byte_loop_at_every_size_alignment_pair_and_disjoint_placement(build: &str) {
    // Clauses: s4_1_c_1, s4_2_c_1. Two disjoint buffers with sentinels around the destination.
    let src = pattern(MAX + 2 * PAD, 1);
    for (name, f) in movers() {
        for n in 0..=MAX {
            for sa in 0..32 {
                for da in 0..32 {
                    let mut dst = vec![0xA5u8; MAX + 2 * PAD];
                    let mut want = dst.clone();
                    want[PAD + da..PAD + da + n].copy_from_slice(&src[PAD + sa..PAD + sa + n]);
                    let dp = dst[PAD + da..].as_mut_ptr();
                    let r = f(dp.cast(), src[PAD + sa..].as_ptr().cast(), n);
                    assert_eq!(r, dp.cast(), "{build} {name} {n} return");
                    assert!(dst == want, "{build} {name} n {n} src +{sa} dst +{da}");
                }
            }
        }
    }
}

fn moves_match_a_byte_loop_at_every_size_and_overlap(build: &str) {
    // Clause: s4_2_c_1 (memmove), D-4.2 (memcpy moves overlapping regions correctly too).
    for (name, f) in movers() {
        for n in 0..=MAX {
            let mut shifts: Vec<isize> = (-40..=40).collect();
            for k in [n as isize - 1, n as isize, n as isize + 1] {
                shifts.extend([k, -k]);
            }
            for &shift in &shifts {
                if shift.unsigned_abs() > MAX + PAD {
                    continue;
                }
                for base in [PAD, PAD + 3, PAD + 17] {
                    let len = 4 * MAX + 4 * PAD;
                    let mut buf = pattern(len, n);
                    let mut want = buf.clone();
                    let s_at = base + MAX + PAD;
                    let d_at = (s_at as isize + shift) as usize;
                    oracle_move(&mut want, d_at, s_at, n);
                    let p = buf.as_mut_ptr();
                    let r = f(p.wrapping_add(d_at).cast(), p.wrapping_add(s_at).cast_const().cast(), n);
                    assert_eq!(r, p.wrapping_add(d_at).cast(), "{build} {name} {n} return");
                    assert!(buf == want, "{build} {name} n {n} shift {shift} base {base}");
                }
            }
        }
    }
}

fn fills_match_a_byte_loop_at_every_size_alignment_and_value(build: &str) {
    // Clauses: s4_1_c_1 (memset, __memset_chk). The value is taken as unsigned char.
    for n in 0..=MAX {
        for da in 0..32 {
            for c in [0i32, 0x7F, 0x80, 0xFF, 0x1_23, -1] {
                for chk in [false, true] {
                    let mut dst = vec![0xA5u8; MAX + 2 * PAD];
                    let mut want = dst.clone();
                    want[PAD + da..PAD + da + n].fill(c as u8);
                    let dp = dst[PAD + da..].as_mut_ptr();
                    let r = if chk { opaque::__memset_chk(dp.cast(), c, n, n) } else { opaque::memset(dp.cast(), c, n) };
                    assert_eq!(r, dp.cast(), "{build} memset {n} return");
                    assert!(dst == want, "{build} memset n {n} dst +{da} c {c} chk {chk}");
                }
            }
        }
    }
}

fn small_copies_and_fills_stay_inside_regions_that_touch_unmapped_pages(build: &str) {
    // Clauses: s4_1_c_1, s4_2_c_1.
    // A read or write one byte past either end faults: the regions sit at the very start and at
    // the very end of a mapping fenced by PROT_NONE pages.
    let r = CSys.mmap(0, 4 * PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, None, 0).unwrap();
    let (mut lo, rest) = r.split_at(PAGE).ok().unwrap();
    let (mid, mut hi) = rest.split_at(2 * PAGE).ok().unwrap();
    CSys.mprotect(&mut lo, PROT_NONE).unwrap();
    CSys.mprotect(&mut hi, PROT_NONE).unwrap();
    let (a, b) = (mid.addr(), mid.addr() + PAGE);
    let src = pattern(MAX, 3);
    for n in 0..=MAX {
        for (d_at, s_at) in [(a, b + PAGE - n), (b + PAGE - n, a), (a + PAGE - n, b)] {
            opaque::memcpy(s_at as *mut c_void, src.as_ptr().cast(), n);
            for (name, f) in movers() {
                opaque::memset(d_at as *mut c_void, 0, n);
                f(d_at as *mut c_void, s_at as *const c_void, n);
                assert_eq!(s::memcmp(d_at as *const c_void, src.as_ptr().cast(), n), 0, "{build} {name} {n} at the fence");
            }
            opaque::memset(d_at as *mut c_void, 0x3C, n);
            let want = vec![0x3Cu8; n];
            assert_eq!(s::memcmp(d_at as *const c_void, want.as_ptr().cast(), n), 0, "{build} memset {n} at the fence");
        }
    }
    for r in [lo, mid, hi] {
        CSys.munmap(r).unwrap();
    }
}

#[test]
fn zero_length_calls_accept_null_and_touch_nothing() {
    // Clause: s4_1_g_0 (n == 0 copies nothing; NULL is then accepted, as glibc does).
    let null = core::ptr::null_mut::<c_void>();
    assert_eq!(opaque::memcpy(null, null, 0), null);
    assert_eq!(opaque::memmove(null, null, 0), null);
    assert_eq!(opaque::memset(null, 1, 0), null);
}

#[test]
fn copies_match_a_byte_loop_at_every_size_alignment_pair_and_disjoint_placement_in_each_build() {
    // Clauses: s4_1_c_1, s4_2_c_1 (each build of the kernels, D-4.25).
    each_build(copies_match_a_byte_loop_at_every_size_alignment_pair_and_disjoint_placement);
}

#[test]
fn moves_match_a_byte_loop_at_every_size_and_overlap_in_each_build() {
    // Clauses: s4_2_c_1 (each build of the kernels, D-4.25).
    each_build(moves_match_a_byte_loop_at_every_size_and_overlap);
}

#[test]
fn fills_match_a_byte_loop_at_every_size_alignment_and_value_in_each_build() {
    // Clauses: s4_1_c_1 (each build of the kernels, D-4.25).
    each_build(fills_match_a_byte_loop_at_every_size_alignment_and_value);
}

#[test]
fn small_copies_and_fills_stay_inside_regions_that_touch_unmapped_pages_in_each_build() {
    // Clauses: s4_1_c_1, s4_2_c_1 (each build of the kernels, D-4.25).
    each_build(small_copies_and_fills_stay_inside_regions_that_touch_unmapped_pages);
}

/// The internal copy (`liella_internal_memcpy`, COPY-INTERNAL) against the exported `memcpy` and
/// a byte loop: every size 0 to 300 and the 4 KiB and 64 KiB boundaries (the kernels' range ends
/// at 64 KiB), every alignment of source and destination mod 32, disjoint buffers with sentinels.
fn internal_copies_match_memcpy_at_every_size_and_alignment(build: &str) {
    // Clauses: s4_1_c_1 (the copy the products' own memcpy calls reach, D-X.15).
    let sizes: Vec<usize> = (0..=300).chain([4095, 4096, 4097, 65_535, 65_536, 65_537]).collect();
    let big = 65_537 + 2 * PAD;
    let src = pattern(big, 5);
    let (mut a, mut b) = (vec![0u8; big], vec![0u8; big]);
    for &n in &sizes {
        for sa in 0..32 {
            for da in 0..32 {
                a.fill(0xA5);
                b.fill(0xA5);
                let (ap, bp) = (a[PAD + da..].as_mut_ptr(), b[PAD + da..].as_mut_ptr());
                let sp = src[PAD + sa..].as_ptr().cast();
                assert_eq!(selftest::internal_memmove(ap.cast(), sp, n), ap.cast(), "{build} internal {n} return");
                opaque::memcpy(bp.cast(), sp, n);
                // The cheap discriminating probes first: the tail, then the head, then all.
                let tail = PAD + da + n;
                assert!(a[tail.saturating_sub(40)..tail + 8] == b[tail.saturating_sub(40)..tail + 8], "{build} n {n} src +{sa} dst +{da}: tail");
                assert!(a == b, "{build} n {n} src +{sa} dst +{da}: internal differs from memcpy");
                if sa == da {
                    assert!(a[PAD + da..tail] == src[PAD + sa..PAD + sa + n], "{build} n {n} +{da}: not the source");
                }
            }
        }
    }
}

#[test]
fn internal_copies_match_memcpy_at_every_size_and_alignment_in_each_build() {
    // Clauses: s4_1_c_1 (each build of the kernels, D-4.25; rep movsb forced on and off).
    each_build(|build| {
        for erms in [false, true] {
            selftest::set_erms(erms);
            internal_copies_match_memcpy_at_every_size_and_alignment(&format!("{build} erms {erms}"));
        }
        // Put the CPUID decision back (ERMS), then the build `each_build` forced.
        let _ = liella_boundary::cpu::init();
        assert!(selftest::set_kernel_avx2(build.starts_with("avx2")));
    });
}

const SIGSEGV: i32 = 11;
const SIGABRT: i32 = 6;

/// The call the NULL child makes (set before the fork; the child gets its own copy).
static NULL_CASE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static NULL_FOLD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static NULL_AVX2: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
const NULL_CALLS: usize = 7;

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
    let mut buf = [0x5Au8; 256];
    let (d, src) = (buf.as_mut_ptr().cast::<c_void>(), [0xA5u8; 256]);
    let null = core::ptr::null_mut::<c_void>();
    match call {
        0 => opaque::memcpy(null, src.as_ptr().cast(), n),
        1 => opaque::memcpy(d, null, n),
        2 => opaque::memmove(null, src.as_ptr().cast(), n),
        3 => opaque::memmove(d, null, n),
        4 => opaque::memset(null, 7, n),
        5 => opaque::__memcpy_chk(null, src.as_ptr().cast(), n, usize::MAX),
        _ => opaque::__memset_chk(null, 7, n, usize::MAX),
    };
    0
}

#[test]
fn null_faults_in_the_folded_sizes_and_aborts_in_every_other() {
    // Clause: s4_1_c_1 with D-4.22's NULL folding (owner rulings 2026-10-08, option B): NULL with
    // 1 <= n <= 64 dies by SIGSEGV on page zero with folding on (the fault in fold.S; glibc's
    // observable outcome, it has no NULL check of its own) and by the S6 abort with folding off.
    // Every other size, 65-256 included, takes the explicit checks either way (the abort): no Rust
    // path performs a deliberate NULL access.
    use std::sync::atomic::Ordering::SeqCst;
    let _turn = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // (fold, AVX2 build): the AVX2 flag changes only the 65-256 copy and 129-256 fill classes.
    let mut runs = vec![(false, false), (true, false)];
    if liella_boundary::cpu::avx2() {
        runs.push((true, true));
    }
    for (fold, avx2) in runs {
        NULL_FOLD.store(fold, SeqCst);
        NULL_AVX2.store(avx2, SeqCst);
        // Folded: every folded size, then the class bounds above it. Checked: the class bounds
        // (each abort prints a line).
        let sizes: Vec<usize> = match (fold, avx2) {
            (true, false) => (1..=66).chain([96, 127, 128, 129, 144, 200, 256, 257, 4096]).collect(),
            (true, true) => (60..=70).chain([96, 127, 128, 129, 144, 256, 257]).collect(),
            _ => vec![1, 2, 3, 4, 7, 8, 15, 16, 17, 32, 33, 63, 64, 65, 96, 127, 128, 129, 144, 200, 256, 257, 4096],
        };
        for n in sizes {
            for call in 0..NULL_CALLS {
                NULL_CASE.store(n * NULL_CALLS + call, SeqCst);
                let want = if fold && n <= 64 { SIGSEGV } else { SIGABRT };
                let end = selftest::in_child_quiet(null_call).unwrap();
                assert_eq!(end, ChildEnd::Signaled(want), "fold {fold} avx2 {avx2} call {call} n {n}");
            }
        }
    }
}

#[test]
fn the_startup_guard_folds_only_from_64_kib() {
    // no spec clause (D-4.22 startup guard): vm.mmap_min_addr >= 65536 allows folding; below it,
    // or unreadable text, does not.
    for (text, ok) in [
        (&b"65536\n"[..], true),
        (b"65536", true),
        (b"4096\n", false),
        (b"0\n", false),
        (b"65535\n", false),
        (b"1048576\n", true),
        (b"", false),
        (b"\n", false),
        (b"-65536\n", false),
        (b" 65536\n", false),
        (b"65536\n\n", false),
        (b"99999999999999999999999\n", false),
        (b"0000000000000000000000000000065536", false),
    ] {
        assert_eq!(selftest::null_fold_allowed_by(text), ok, "{:?}", String::from_utf8_lossy(text));
    }
}

#[test]
fn the_folding_decision_is_taken_by_the_first_small_call_not_at_startup() {
    // no spec clause (STARTUP-1, D-4.22): the vm.mmap_min_addr read is lazy. After a reset
    // (process start) folding is off and undecided; a large call leaves it undecided; the first
    // copy or fill of 1-64 bytes decides it, and the decision equals the startup guard's.
    let _turn = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (fold_before, resolved_before) = (selftest::null_fold(), selftest::null_fold_resolved());
    let (mut a, mut b) = ([1u8; 256], [2u8; 256]);
    let (pa, pb) = (a.as_mut_ptr().cast::<c_void>(), b.as_mut_ptr().cast::<c_void>());
    let want = selftest::set_null_fold(true); // what the guard allows on this machine
    for which in 0..3 {
        selftest::unresolve_null_fold();
        assert!(!selftest::null_fold_resolved() && !selftest::null_fold());
        opaque::memmove(pa, pb, 200);
        opaque::memset(pa, 1, 200);
        assert!(!selftest::null_fold_resolved(), "a size above 64 must not read mmap_min_addr");
        match which {
            0 => drop(opaque::memcpy(pa, pb, 8)),
            1 => drop(opaque::memmove(pa, pb, 64)),
            _ => drop(opaque::memset(pa, 7, 1)),
        }
        assert!(selftest::null_fold_resolved(), "call {which}: the first small call must decide");
        assert_eq!(selftest::null_fold(), want, "call {which}");
    }
    // A forced setting is never overridden by a later small call.
    assert!(selftest::set_null_fold(false));
    opaque::memcpy(pa, pb, 8);
    assert!(!selftest::null_fold());
    if resolved_before {
        assert!(selftest::set_null_fold(fold_before));
    } else {
        selftest::unresolve_null_fold();
    }
}
