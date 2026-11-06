//! The classed entries (D-4.53, CLASSED-SLOT-INFRA): the SSE2 page and the AVX2 page of the
//! classed section, both exercised on every AVX2 host. Each case runs in a forked child
//! (`selftest::in_child`, one thread), which performs the start-up's page swap on itself and
//! then sweeps `memset` (the first classed slot) against a byte loop; the parent process never
//! swaps. The live page is read back from the code (`classed_id` slot), not from a flag.
#![no_builtins] // calls to libc names reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use liella_boundary::classed::{self, Outcome};
use liella_boundary::selftest::{self, opaque, ChildEnd};
use liella_boundary::sys::CSys;

/// A child's "the capability is absent" exit status (the parent prints why and passes).
const SKIP: i32 = 77;
const MAX: usize = 320;
const PAD: usize = 64;

/// Every size 0 to 320 (each class of the slot, the >256 paths included), every destination
/// alignment in a 32-byte window, values whose int has other bytes set; sentinels on both sides.
/// Stack buffers only: the child of a multi-threaded test process must not allocate.
fn sweep() {
    for n in 0..=MAX {
        for da in 0..32 {
            for c in [0i32, 0x7F, 0x80, 0xFF, 0x1_23, -1] {
                let mut dst = [0xA5u8; MAX + 2 * PAD];
                let dp = dst[PAD + da..].as_mut_ptr();
                let r = opaque::memset(dp.cast(), c, n);
                assert_eq!(r, dp.cast(), "memset {n} return");
                for (i, &b) in dst.iter().enumerate() {
                    let want = if (PAD + da..PAD + da + n).contains(&i) { c as u8 } else { 0xA5 };
                    assert_eq!(b, want, "memset n {n} +{da} c {c:#x}: byte {i}");
                }
            }
        }
    }
}

/// Whether the page at `addr` is present and still a file page (`/proc/self/pagemap` bits 63 and
/// 61), or `None` when the pagemap cannot be read. The test touches the page first.
fn file_backed(addr: usize) -> Option<bool> {
    use std::os::unix::fs::FileExt;
    let _ = classed::live_page(); // runs the page's code, so it is present
    let pm = std::fs::File::open("/proc/self/pagemap").ok()?;
    let mut e = [0u8; 8];
    pm.read_exact_at(&mut e, (addr / classed::PAGE * 8) as u64).ok()?;
    let e = u64::from_le_bytes(e);
    Some(e & (1 << 63) != 0 && e & (1 << 61) != 0)
}

/// A line on stderr from a forked child, by one `write` (no allocation, no output capture).
fn say(msg: &[u8]) {
    use liella_kernel::sys::Sys;
    let _ = CSys.write(liella_kernel::fd::Fd::STDERR, msg);
}

fn run(name: &str, body: fn(&CSys) -> i32) {
    match selftest::in_child(body).expect("fork") {
        ChildEnd::Exited(0) => {}
        ChildEnd::Exited(SKIP) => eprintln!("classed_slots: {name}: skipped (reason printed by the child above)"),
        end => panic!("{name}: child ended with {end:?}"),
    }
}

#[test]
fn the_avx2_page_after_the_swap_fills_like_a_byte_loop() {
    // Clauses: s4_1_c_1 (memset) on the AVX2 page; D-4.53 start-up swap (no spec clause).
    run("avx2 page", |_| {
        let o = selftest::classed_install(true, false);
        if liella_boundary::cpu::avx2() {
            assert_eq!(o, Outcome::Installed, "an AVX2 CPU with an unmodified section swaps");
            assert_eq!(classed::live_page(), 1, "the AVX2 page is live after the swap");
            assert_eq!(file_backed(classed::page0_addr()), Some(true), "the live slot page was written: it is no longer the file's page");
        } else {
            say(b"classed_slots: no AVX2 on this CPU: the SSE2 page only\n");
            assert_eq!(o, Outcome::Sse2Class);
            assert_eq!(classed::live_page(), 0);
        }
        sweep();
        0
    });
}

#[test]
fn the_sse2_page_forced_on_any_host_fills_like_a_byte_loop() {
    // Clauses: s4_1_c_1 (memset) on the SSE2 page; D-4.53 (no spec clause). The test override:
    // the class decision forced to SSE2, as `glibc.cpu.hwcaps=-AVX2` makes it.
    assert_eq!(classed::live_page(), 0, "a test binary never swaps at start");
    run("sse2 page", |_| {
        assert_eq!(selftest::classed_install(false, false), Outcome::Sse2Class);
        assert_eq!(classed::live_page(), 0);
        sweep();
        0
    });
}

#[test]
fn the_compat_flag_keeps_the_sse2_page() {
    // Clauses: no spec clause (COMPAT-20, `liella.compat.no_classed_swap=1`).
    run("compat flag", |_| {
        let o = selftest::classed_install(true, true);
        assert_eq!(o, if liella_boundary::cpu::avx2() { Outcome::Disabled } else { Outcome::Sse2Class });
        assert_eq!(classed::live_page(), 0);
        0
    });
}

#[test]
fn a_page_written_before_the_swap_keeps_the_sse2_page() {
    // Clauses: no spec clause (D-4.53 pagemap guard). A tracer's breakpoint, as gdb inserts it:
    // one byte of page 0 written back unchanged through /proc/self/mem, which makes the page a
    // private anonymous copy with the same content.
    run("guard", |_| {
        if !liella_boundary::cpu::avx2() {
            say(b"classed_slots: no AVX2 on this CPU: no swap to guard\n");
            return SKIP;
        }
        use std::os::unix::fs::FileExt;
        let Ok(mem) = std::fs::OpenOptions::new().read(true).write(true).open("/proc/self/mem") else {
            say(b"classed_slots: /proc/self/mem cannot be opened for writing\n");
            return SKIP;
        };
        let at = classed::page0_addr() as u64 + 16;
        let mut b = [0u8; 1];
        if mem.read_at(&mut b, at).ok() != Some(1) || mem.write_at(&b, at).ok() != Some(1) {
            say(b"classed_slots: /proc/self/mem refuses a write to text (proc_mem.force_override)\n");
            return SKIP;
        }
        assert_eq!(selftest::classed_install(true, false), Outcome::Modified, "a written page 0 must keep the SSE2 page");
        assert_eq!(classed::live_page(), 0);
        sweep();
        0
    });
}

#[test]
fn the_swap_passes_under_mdwe() {
    // Clauses: no spec clause (D-4.53, design 5.2: PR_SET_MDWE refuses an exec gain; the swap
    // gains none). The policy is set in the child before the swap, as a parent would before execve.
    run("mdwe", |_| {
        if !liella_boundary::cpu::avx2() {
            say(b"classed_slots: no AVX2 on this CPU: no swap under MDWE to test\n");
            return SKIP;
        }
        if selftest::set_mdwe_refuse_exec_gain().is_err() {
            say(b"classed_slots: this kernel has no PR_SET_MDWE (Linux 6.3 and later)\n");
            return SKIP;
        }
        assert_eq!(selftest::classed_install(true, false), Outcome::Installed, "the swap under MDWE");
        assert_eq!(classed::live_page(), 1);
        sweep();
        0
    });
}

const SIGABRT: i32 = 6;
const SIGSEGV: i32 = 11;

/// The fill the grandchild makes (set by the child before each fork): `d` (0 for NULL), `n`,
/// `__memset_chk` instead of `memset`.
static FILL_D: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static FILL_N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
static FILL_CHK: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn fill_call(k: &CSys) -> i32 {
    use std::sync::atomic::Ordering::SeqCst;
    if k.prctl_int(liella_kernel::ops::PrctlInt::SetDumpable, 0, 0).is_err() {
        return 4;
    }
    let (d, n) = (FILL_D.load(SeqCst) as *mut liella_abi::ctypes::c_void, FILL_N.load(SeqCst));
    if FILL_CHK.load(SeqCst) {
        opaque::__memset_chk(d, 0x3C, n, usize::MAX);
    } else {
        opaque::memset(d, 0x3C, n);
    }
    0
}

/// One grandchild fill; whether it ended by `want`.
fn fill_ends_by(d: usize, n: usize, chk: bool, want: i32) -> bool {
    use std::sync::atomic::Ordering::SeqCst;
    FILL_D.store(d, SeqCst);
    FILL_N.store(n, SeqCst);
    FILL_CHK.store(chk, SeqCst);
    matches!(selftest::in_child_quiet(fill_call), Ok(ChildEnd::Signaled(s)) if s == want)
}

#[test]
fn the_avx2_page_folds_null_up_to_256_and_stores_head_first() {
    // Clauses: s4_1_c_1 (memset, __memset_chk) on the AVX2 page after the swap (D-4.53, the swap
    // flag on), with D-4.60: with folding on a NULL d faults on page zero (SIGSEGV) for every size
    // up to 256 and aborts (S6) above; with folding off every size aborts. Head-first: d 16 bytes
    // below a writable shared page over a PROT_NONE page, the head store faults and the writable
    // page stays untouched for every size of the 32-byte-block 129-256 class and the 65-128 class.
    run("avx2 page NULL", |k| {
        use liella_kernel::sys::Sys;
        use liella_kernel::uapi::{MAP_ANONYMOUS, MAP_SHARED, PROT_NONE, PROT_READ, PROT_WRITE};
        if !liella_boundary::cpu::avx2() {
            say(b"classed_slots: no AVX2 on this CPU: no AVX2 page to check\n");
            return SKIP;
        }
        assert_eq!(selftest::classed_install(true, false), Outcome::Installed);
        assert_eq!(classed::live_page(), 1, "the AVX2 page is live after the swap");
        for fold in [false, true] {
            assert!(selftest::set_null_fold(fold), "vm.mmap_min_addr below 64 KiB or unreadable: NULL folding refused");
            for n in [1, 64, 65, 96, 127, 128, 129, 130, 144, 200, 255, 256, 257, 4096, 65_537] {
                for chk in [false, true] {
                    let want = if fold && n <= 256 { SIGSEGV } else { SIGABRT };
                    assert!(fill_ends_by(0, n, chk, want), "fold {fold} n {n} chk {chk}: NULL ended otherwise than by {want}");
                }
            }
        }
        let r = k.mmap(0, 2 * classed::PAGE, PROT_READ | PROT_WRITE, MAP_SHARED | MAP_ANONYMOUS, None, 0).unwrap();
        let (mut lo, hi) = r.split_at(classed::PAGE).ok().unwrap();
        k.mprotect(&mut lo, PROT_NONE).unwrap();
        let (edge, clean) = (hi.addr(), [0xA5u8; classed::PAGE]);
        for n in 65..=256 {
            opaque::memset(edge as *mut liella_abi::ctypes::c_void, 0xA5, classed::PAGE);
            assert!(fill_ends_by(edge - 16, n, false, SIGSEGV), "n {n}: the head store must fault");
            let same = liella_boundary::posix::s04_strings::memcmp(edge as *const liella_abi::ctypes::c_void, clean.as_ptr().cast(), classed::PAGE) == 0;
            assert!(same, "n {n}: a store past the head block came before it (D-4.60 head-first)");
        }
        0
    });
}
