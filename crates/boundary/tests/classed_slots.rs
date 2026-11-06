//! The classed entries (D-4.53, CLASSED-SLOT-INFRA): the SSE2 page and the AVX2 page of the
//! classed section, both exercised on every AVX2 host. Each case runs in a forked child
//! (`selftest::in_child`, one thread), which performs the start-up's page swap on itself and
//! then sweeps `memset` (the first classed slot) against a byte loop; the parent process never
//! swaps. The live page is read back from the code (`classed_id` slot), not from a flag.
#![no_builtins] // calls to libc names reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

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
            assert_eq!(file_backed(classed::page0()), Some(true), "the live slot page was written: it is no longer the file's page");
        } else {
            eprintln!("classed_slots: no AVX2 on this CPU: the SSE2 page only");
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
            eprintln!("classed_slots: no AVX2 on this CPU: no swap to guard");
            return SKIP;
        }
        use std::os::unix::fs::FileExt;
        let Ok(mem) = std::fs::OpenOptions::new().read(true).write(true).open("/proc/self/mem") else {
            eprintln!("classed_slots: /proc/self/mem cannot be opened for writing");
            return SKIP;
        };
        let at = classed::page0() as u64 + 16;
        let mut b = [0u8; 1];
        if mem.read_at(&mut b, at) != Ok(1) || mem.write_at(&b, at).map_err(|e| e.raw_os_error()) != Ok(1) {
            eprintln!("classed_slots: /proc/self/mem refuses a write to text (proc_mem.force_override)");
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
            eprintln!("classed_slots: no AVX2 on this CPU: no swap under MDWE to test");
            return SKIP;
        }
        if selftest::set_mdwe_refuse_exec_gain().is_err() {
            eprintln!("classed_slots: this kernel has no PR_SET_MDWE (Linux 6.3 and later)");
            return SKIP;
        }
        assert_eq!(selftest::classed_install(true, false), Outcome::Installed, "the swap under MDWE");
        assert_eq!(classed::live_page(), 1);
        sweep();
        0
    });
}
