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
use liella_kernel::uapi::{MAP_ANONYMOUS, MAP_FIXED_NOREPLACE, MAP_PRIVATE, MAP_SHARED, PROT_NONE, PROT_READ, PROT_WRITE};

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
/// `rep stosb` forced on in the child (the `memset` entry's ERMS path on every machine).
static NULL_ERMS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
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
    if NULL_ERMS.load(SeqCst) {
        selftest::set_erms(true);
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
    // The copies (memcpy, memmove, __memcpy_chk) also fold 129-256 (owner ruling #3304, D-4.55);
    // memset and __memset_chk fold 65-128 (D-4.60, owner ruling 2026-11-06): SIGSEGV up to 128
    // with folding on; their 129-256 class keeps its test. Every other size and call, the copies' 65-128 included, takes the explicit
    // checks either way (the abort), n > 64 KiB included: no Rust path performs a deliberate NULL
    // access.
    use std::sync::atomic::Ordering::SeqCst;
    let _turn = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    // (fold, AVX2 build): the AVX2 flag changes only the 65-256 copy and 129-256 fill classes.
    // Third: `rep stosb` forced on, so the `memset` entry's ERMS path (MEMSET-4K-4: NULL is
    // 64-byte aligned and takes its aligned branch) is checked without ERMS hardware too.
    let mut runs = vec![(false, false, false), (true, false, false), (true, false, true)];
    if liella_boundary::cpu::avx2() {
        runs.push((true, true, false));
    }
    for (fold, avx2, erms) in runs {
        NULL_FOLD.store(fold, SeqCst);
        NULL_AVX2.store(avx2, SeqCst);
        NULL_ERMS.store(erms, SeqCst);
        // Folded: every folded size, then the class bounds above it. Checked: the class bounds
        // (each abort prints a line).
        let sizes: Vec<usize> = match (fold, avx2, erms) {
            (_, _, true) => vec![1280, 4096, 65_536, 65_537, 1 << 20],
            (true, false, _) => (1..=66).chain([96, 127, 128, 129, 130, 144, 200, 255, 256, 257, 4096, 65_537]).collect(),
            (true, true, _) => (60..=70).chain([96, 127, 128, 129, 144, 256, 257]).collect(),
            _ => vec![1, 2, 3, 4, 7, 8, 15, 16, 17, 32, 33, 63, 64, 65, 96, 127, 128, 129, 144, 200, 256, 257, 4096],
        };
        for n in sizes {
            for call in 0..NULL_CALLS {
                NULL_CASE.store(n * NULL_CALLS + call, SeqCst);
                let fill = call == 4 || call == NULL_CALLS - 1; // memset, __memset_chk
                let copy = matches!(call, 0 | 1 | 2 | 3 | 5);
                let folded = n <= 64 || (fill && n <= 128) || (copy && (129..=256).contains(&n));
                let want = if fold && folded { SIGSEGV } else { SIGABRT };
                let end = selftest::in_child_quiet(null_call).unwrap();
                assert_eq!(end, ChildEnd::Signaled(want), "fold {fold} avx2 {avx2} call {call} n {n}");
            }
        }
    }
}

/// The 129-256 NULL children's call (`case % MID_CALLS`) and size (`case / MID_CALLS`), the build
/// and, for the shared-mapping test, the destination (set before the fork).
static MID_CASE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static MID_FOLD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static MID_AVX2: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static MID_DEST: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
const MID_CALLS: usize = 8;

/// The builds of the 129-256 NULL tests: (folding, AVX2 build); AVX2 only where the CPU has it.
fn mid_builds() -> Vec<(bool, bool)> {
    let mut runs = vec![(false, false), (true, false)];
    if liella_boundary::cpu::avx2() {
        runs.extend([(false, true), (true, true)]);
    } else {
        println!("no AVX2 on this CPU: the 129-256 NULL tests run the SSE2 build only");
    }
    runs
}

fn mid_null_call(k: &CSys) -> i32 {
    use std::sync::atomic::Ordering::SeqCst;
    if k.prctl_int(liella_kernel::ops::PrctlInt::SetDumpable, 0, 0).is_err() {
        return 4;
    }
    // The production order: the build at startup, then the (lazy) folding decision, so the
    // decision's own word store is the one the call reads.
    if !selftest::set_kernel_avx2(MID_AVX2.load(SeqCst)) || !selftest::set_null_fold(MID_FOLD.load(SeqCst)) {
        return 3;
    }
    let case = MID_CASE.load(SeqCst);
    let (call, n) = (case % MID_CALLS, case / MID_CALLS);
    let src = [0xA5u8; 256];
    let dest = MID_DEST.load(SeqCst);
    let mut buf = [0x5Au8; 256];
    let d = if dest == 0 { buf.as_mut_ptr().cast::<c_void>() } else { dest as *mut c_void };
    let (s_ok, null) = (src.as_ptr().cast::<c_void>(), core::ptr::null_mut::<c_void>());
    let move_chk = core::hint::black_box(s::__memmove_chk as extern "C" fn(*mut c_void, *const c_void, usize, usize) -> *mut c_void);
    match call {
        0 => opaque::memcpy(null, s_ok, n),
        1 => opaque::memcpy(d, null, n),
        2 => opaque::memcpy(null, null, n),
        3 => opaque::memmove(null, s_ok, n),
        4 => opaque::memmove(d, null, n),
        5 => opaque::memmove(null, null, n),
        6 => opaque::__memcpy_chk(d, null, n, usize::MAX),
        _ => move_chk(null, s_ok, n, usize::MAX),
    };
    0
}

#[test]
fn copies_of_129_to_256_bytes_fault_on_a_null_with_folding_and_abort_without() {
    // Clause: s4_1_c_1 with D-4.55's NULL folding of the 129-256 copy class (owner ruling #3304):
    // a NULL source, destination or both, at every size 129-256, through memcpy, memmove,
    // __memcpy_chk and __memmove_chk, dies by SIGSEGV on page zero with folding on and by the S6
    // abort with folding off, in the SSE2 and the AVX2 build.
    use std::sync::atomic::Ordering::SeqCst;
    let _turn = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    MID_DEST.store(0, SeqCst);
    for (fold, avx2) in mid_builds() {
        MID_FOLD.store(fold, SeqCst);
        MID_AVX2.store(avx2, SeqCst);
        let want = if fold { SIGSEGV } else { SIGABRT };
        for n in 129..=256 {
            for call in 0..MID_CALLS {
                MID_CASE.store(n * MID_CALLS + call, SeqCst);
                let end = selftest::in_child_quiet(mid_null_call).unwrap();
                assert_eq!(end, ChildEnd::Signaled(want), "fold {fold} avx2 {avx2} call {call} n {n}");
            }
        }
    }
}

#[test]
fn a_null_source_of_129_to_256_bytes_writes_nothing_into_a_shared_file_mapping() {
    // Clause: s4_1_c_1 with D-4.55's ordering requirement: the folded 129-256 copy loads the
    // source's first block before any store, so a NULL source crashes before one byte reaches a
    // valid destination. The destination is a MAP_SHARED page of a memfd (the parent sees what the
    // child wrote, and so does the file); after each crash (SIGSEGV folded, the abort otherwise)
    // the mapping and the file still hold their pattern.
    use liella_kernel::path::KStr;
    use liella_kernel::uapi::{MAP_SHARED, MFD_CLOEXEC};
    use std::sync::atomic::Ordering::SeqCst;
    let _turn = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let fd = CSys.memfd_create(KStr::from_nul_terminated(b"liella-nullfold\0").unwrap(), MFD_CLOEXEC).unwrap();
    let want_bytes: Vec<u8> = (0..PAGE).map(|i| (i % 251) as u8 | 1).collect();
    CSys.pwrite(fd, &want_bytes, 0).unwrap();
    let region = CSys.mmap(0, PAGE, PROT_READ | PROT_WRITE, MAP_SHARED, Some(fd), 0).unwrap();
    // The destination at the start of the page, and one ending at its end.
    for (fold, avx2) in mid_builds() {
        MID_FOLD.store(fold, SeqCst);
        MID_AVX2.store(avx2, SeqCst);
        let want = if fold { SIGSEGV } else { SIGABRT };
        for n in 129..=256 {
            for (call, at) in [(1, 0), (4, 0), (6, 0), (1, PAGE - n), (4, PAGE - n)] {
                MID_DEST.store(region.addr() + at, SeqCst);
                MID_CASE.store(n * MID_CALLS + call, SeqCst);
                let end = selftest::in_child_quiet(mid_null_call).unwrap();
                assert_eq!(end, ChildEnd::Signaled(want), "fold {fold} avx2 {avx2} call {call} n {n} at {at}");
                let mut file = vec![0u8; PAGE];
                assert_eq!(CSys.pread(fd, &mut file, 0).unwrap(), PAGE);
                assert!(file == want_bytes, "fold {fold} avx2 {avx2} call {call} n {n} at {at}: the file changed");
                let mapped = s::memcmp(region.addr() as *const c_void, want_bytes.as_ptr().cast(), PAGE);
                assert_eq!(mapped, 0, "fold {fold} avx2 {avx2} call {call} n {n} at {at}: the mapping changed");
            }
        }
    }
    MID_DEST.store(0, SeqCst);
    CSys.munmap(region).unwrap();
    CSys.close(fd).unwrap();
}

#[test]
fn the_129_to_256_copy_word_follows_the_folding_and_build_settings_in_either_order() {
    // no spec clause (D-4.55, NULLFOLD-COPY-MID-3): the copy entry reads one word for the 129-256
    // class; its bound is 256 only with folding on, and its SSE2 mark is set exactly when folding
    // is on with the SSE2 build, whichever setting changed last. Both bodies copy the same bytes,
    // so only the word itself shows a wrong build choice on a CPU with AVX2.
    let _turn = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (avx2_before, fold_before) = (selftest::kernel_avx2(), selftest::null_fold());
    let builds = mid_builds();
    for &(fold, avx2) in &builds {
        for (from_fold, from_avx2) in &builds {
            // From every other state, set folding first, then the build; then the other order.
            for fold_first in [true, false] {
                assert!(selftest::set_null_fold(*from_fold) && selftest::set_kernel_avx2(*from_avx2));
                if fold_first {
                    assert!(selftest::set_null_fold(fold) && selftest::set_kernel_avx2(avx2));
                } else {
                    assert!(selftest::set_kernel_avx2(avx2) && selftest::set_null_fold(fold));
                }
                let want = (if fold { 256 } else { 0 }, fold && !avx2);
                assert_eq!(
                    selftest::copy_class_word(),
                    want,
                    "copy word: fold {fold} avx2 {avx2} from fold {from_fold} avx2 {from_avx2} fold first {fold_first}"
                );
            }
        }
    }
    assert!(selftest::set_kernel_avx2(avx2_before));
    assert!(selftest::set_null_fold(fold_before));
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
    // copy of 1-256 bytes (D-4.55) or fill of 1-64 bytes decides it, and the decision equals the
    // startup guard's.
    let _turn = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let (fold_before, resolved_before) = (selftest::null_fold(), selftest::null_fold_resolved());
    let (mut a, mut b) = ([1u8; 256], [2u8; 256]);
    let (pa, pb) = (a.as_mut_ptr().cast::<c_void>(), b.as_mut_ptr().cast::<c_void>());
    let want = selftest::set_null_fold(true); // what the guard allows on this machine
    let (mut c, mut e) = (vec![1u8; 300], vec![2u8; 300]);
    let (pc, pe) = (c.as_mut_ptr().cast::<c_void>(), e.as_mut_ptr().cast::<c_void>());
    for which in 0..5 {
        selftest::unresolve_null_fold();
        assert!(!selftest::null_fold_resolved() && !selftest::null_fold());
        opaque::memmove(pc, pe, 257);
        opaque::memcpy(pc, pe, 300);
        opaque::memset(pa, 1, 200);
        assert!(!selftest::null_fold_resolved(), "a copy above 256 or a fill above 64 must not read mmap_min_addr");
        match which {
            0 => drop(opaque::memcpy(pa, pb, 8)),
            1 => drop(opaque::memmove(pa, pb, 64)),
            2 => drop(opaque::memcpy(pa, pb, 129)),
            3 => drop(opaque::memmove(pa, pb, 256)),
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

/// The fill a store-order or 64 KiB child makes (set before each fork; the child gets its own
/// copy): `d` (0 for NULL), `n`, `__memset_chk` instead of `memset`, and the kernel forced in the
/// child (0 as decided, 1 `rep stosb`, 2 the AVX2 build).
static FILL_D: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static FILL_N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static FILL_CHK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
static FILL_MODE: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// The fill child: folding on, the kernel `FILL_MODE` names, then one fill of 0x3C.
fn fill_call(k: &CSys) -> i32 {
    use std::sync::atomic::Ordering::SeqCst;
    if k.prctl_int(liella_kernel::ops::PrctlInt::SetDumpable, 0, 0).is_err() {
        return 4;
    }
    if !selftest::set_null_fold(true) {
        return 3;
    }
    match FILL_MODE.load(SeqCst) {
        1 => selftest::set_erms(true),
        2 if !selftest::set_kernel_avx2(true) => return 5,
        _ => {}
    }
    let (d, n) = (FILL_D.load(SeqCst) as *mut c_void, FILL_N.load(SeqCst));
    if FILL_CHK.load(SeqCst) {
        opaque::__memset_chk(d, 0x3C, n, usize::MAX);
    } else {
        opaque::memset(d, 0x3C, n);
    }
    0
}

#[test]
fn folded_fills_store_their_head_block_first() {
    // Clause: s4_1_c_1 (memset, __memset_chk) with D-4.60's head-first obligation: the first store
    // of each fill class without a NULL test is its head block at d, before any store further out.
    // Here d sits 16 bytes below a writable page, with a PROT_NONE page under it: the head store
    // faults (SIGSEGV), and the writable page, shared with the child, must be left untouched. A
    // class that stored a later block first would have written it before the fault.
    use std::sync::atomic::Ordering::SeqCst;
    let _turn = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let r = CSys.mmap(0, 2 * PAGE, PROT_READ | PROT_WRITE, MAP_SHARED | MAP_ANONYMOUS, None, 0).unwrap();
    let (mut lo, hi) = r.split_at(PAGE).ok().unwrap();
    CSys.mprotect(&mut lo, PROT_NONE).unwrap();
    let (edge, clean) = (hi.addr(), vec![0xA5u8; PAGE]);
    FILL_MODE.store(0, SeqCst);
    for n in 17..=256 {
        for chk in [false, true] {
            opaque::memset(edge as *mut c_void, 0xA5, PAGE);
            FILL_D.store(edge - 16, SeqCst);
            FILL_N.store(n, SeqCst);
            FILL_CHK.store(chk, SeqCst);
            let end = selftest::in_child_quiet(fill_call).unwrap();
            assert_eq!(end, ChildEnd::Signaled(SIGSEGV), "n {n} chk {chk}: the head store must fault");
            let same = s::memcmp(edge as *const c_void, clean.as_ptr().cast(), PAGE) == 0;
            assert!(same, "n {n} chk {chk}: a store past the head block came before it (D-4.60 head-first)");
        }
    }
    CSys.munmap(lo).unwrap();
    CSys.munmap(hi).unwrap();
}

/// The 64 KiB child: maps a shared page at 64 KiB (the lowest address the guard allows), then for
/// each kernel and size above 64 KiB a grandchild fills from NULL; 0 when every grandchild aborts
/// and the page stays untouched, 77 when the page cannot be mapped (said on stderr).
fn null_fill_beside_a_page_at_64_kib(k: &CSys) -> i32 {
    use std::sync::atomic::Ordering::SeqCst;
    const AT: usize = 0x1_0000;
    let flags = MAP_SHARED | MAP_ANONYMOUS | MAP_FIXED_NOREPLACE;
    let page = match k.mmap(AT, PAGE, PROT_READ | PROT_WRITE, flags, None, 0) {
        Ok(r) if r.addr() == AT => r,
        _ => {
            let _ = k.write(liella_kernel::fd::Fd::STDERR, b"small_copy: no page can be mapped at 64 KiB here: that check skipped\n");
            return 77;
        }
    };
    let clean = [0xA5u8; PAGE];
    let modes: &[usize] = if liella_boundary::cpu::avx2() { &[0, 1, 2] } else { &[0, 1] };
    for &mode in modes {
        for n in [65_537, AT + PAGE, 1 << 20] {
            for chk in [false, true] {
                opaque::memset(AT as *mut c_void, 0xA5, PAGE);
                FILL_D.store(0, SeqCst);
                FILL_N.store(n, SeqCst);
                FILL_CHK.store(chk, SeqCst);
                FILL_MODE.store(mode, SeqCst);
                if !matches!(selftest::in_child_quiet(fill_call), Ok(ChildEnd::Signaled(SIGABRT))) {
                    return 1;
                }
                if s::memcmp(AT as *const c_void, clean.as_ptr().cast(), PAGE) != 0 {
                    return 2;
                }
            }
        }
    }
    let _ = k.munmap(page);
    0
}

#[test]
fn null_fills_above_64_kib_abort_and_leave_a_page_at_64_kib_untouched() {
    // Clause: s4_1_c_1 (the S6 abort) with D-4.60: the fill classes without a NULL test end at 128
    // bytes (129-256 keeps its test too); every larger fill (`rep stosb`, the AVX2 fill, the Rust path) keeps the explicit test,
    // so a NULL d with n > 64 KiB aborts before any store and never reaches a page mapped at 64 KiB.
    // In a child: the test process keeps its low addresses free.
    let _turn = BUILD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let end = selftest::in_child(null_fill_beside_a_page_at_64_kib).unwrap();
    if end == ChildEnd::Exited(77) {
        eprintln!("small_copy: 64 KiB check skipped (reason printed above)");
        return;
    }
    assert_eq!(end, ChildEnd::Exited(0), "NULL fill above 64 KiB (1: a grandchild did not abort, 2: the page at 64 KiB was written)");
}
