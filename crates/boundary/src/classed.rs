//! The classed entries (D-4.53, CLASSED-SLOT-INFRA; design `docs/design/loadtime-entry.md`
//! sections 4-5): one entry per CPU class, chosen once at start-up, never written.
//!
//! `machine/fold.S` holds one page-aligned section of two 4096-byte pages with the same slot
//! layout: page 0 the SSE2 bodies at the exported labels, page 1 the AVX2 bodies assembled for
//! page 0's address. Both are mapped read+exec from the file and stay that way. On the AVX2 class
//! the start-up (static step 4, after `cpu::init_with`, one thread, before the program's IFUNC
//! resolvers and constructors) moves page 1 over page 0 with one `mremap`, after the
//! `/proc/self/pagemap` guard shows that neither page was modified since it was mapped (a
//! debugger breakpoint or a uprobe written before start-up makes it anonymous). No write to code,
//! no gain of exec permission, no private memory, no code pointer, nothing per call.
//!
//! Every refusal keeps page 0, which is correct on every x86-64: no `/proc` (RULING 3, #3140),
//! a modified page, an `mremap` refused by a policy. After a failed `mremap` the start-up checks
//! that page 0 is still mapped (`mremap(2)` does not say what `MREMAP_FIXED` leaves at the
//! destination when the move fails); if it is gone it tries once more, and if that fails too the
//! process ends with a message instead of faulting at its next `memset`.
//!
//! `liella.compat.no_classed_swap=1` (COMPAT-20) keeps the file's page 0, so debuggers,
//! profilers and core files see the code that runs.
use core::sync::atomic::{AtomicU8, Ordering::Relaxed};

use liella_kernel::errno::Errno;
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{MREMAP_FIXED, MREMAP_MAYMOVE, O_CLOEXEC, O_RDONLY};

use crate::ffi;
use crate::sys::CSys;

/// The size of each page of the classed section (machine/fold.S `LIELLA_CLASSED_PAGE`).
pub const PAGE: usize = 4096;
/// `/proc/<pid>/pagemap` (kernel `admin-guide/mm/pagemap.rst`): bit 63, the page is present.
const PM_PRESENT: u64 = 1 << 63;
/// Bit 61: the page is a file page or shared anonymous (clear once a private page was written).
const PM_FILE: u64 = 1 << 61;

/// What the start-up did with the classed pages ([`outcome`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Outcome {
    /// Not decided yet (before start-up step 4, or a program started by another `ld.so`).
    NotRun = 0,
    /// The CPU class is SSE2 (no AVX2, no saved YMM state, or `glibc.cpu.hwcaps=-AVX2`): no system call.
    Sse2Class = 1,
    /// `liella.compat.no_classed_swap=1` (COMPAT-20): page 0 kept.
    Disabled = 2,
    /// `/proc/self/pagemap` could not be opened or read: page 0 kept.
    NoPagemap = 3,
    /// A page was modified (a breakpoint or a uprobe written before start-up): page 0 kept.
    Modified = 4,
    /// The `mremap` was refused and page 0 is still mapped: page 0 kept.
    Refused = 5,
    /// Page 1 is live at page 0's address.
    Installed = 6,
    /// The first `mremap` failed and left page 0 unmapped; the second one put page 1 there.
    InstalledOnRetry = 7,
}

impl Outcome {
    /// The stored code ([`outcome`] maps it back).
    const fn code(self) -> u8 {
        match self {
            Outcome::NotRun => 0,
            Outcome::Sse2Class => 1,
            Outcome::Disabled => 2,
            Outcome::NoPagemap => 3,
            Outcome::Modified => 4,
            Outcome::Refused => 5,
            Outcome::Installed => 6,
            Outcome::InstalledOnRetry => 7,
        }
    }
}

static OUTCOME: AtomicU8 = AtomicU8::new(Outcome::NotRun.code());

/// What the start-up did (for the tests and the selftest).
pub fn outcome() -> Outcome {
    match OUTCOME.load(Relaxed) {
        1 => Outcome::Sse2Class,
        2 => Outcome::Disabled,
        3 => Outcome::NoPagemap,
        4 => Outcome::Modified,
        5 => Outcome::Refused,
        6 => Outcome::Installed,
        7 => Outcome::InstalledOnRetry,
        _ => Outcome::NotRun,
    }
}

/// Which page is live at page 0's address, read from the code itself (the `classed_id` slot):
/// 0 the SSE2 page, 1 the AVX2 page.
pub fn live_page() -> u32 {
    ffi::liella_classed_id()
}

/// Page 0's address (page 1 is at `+ PAGE`). Not named `page0`: the functions below take a
/// `page0` argument, and `cargo xtask affected` follows a csys symbol's users by name, so a
/// function of that name made every one of them (the `SwapOps` trait methods too) a user of
/// fold.S and escalated a change to fold.S or scan.S to FULL.
pub fn page0_addr() -> usize {
    core::ptr::from_ref(&ffi::liella_classed_page0).addr()
}

/// Whether the AVX2 page is wanted: the CPU reports AVX2 with the YMM state saved by the OS
/// (`Features::avx2` reads XCR0) and the tunables do not say `-AVX2`. The same rule as the
/// kernels' AVX2 flag (`cpu::init_with`).
pub fn avx2_class(f: &liella_rt::cpu::Features, t: &liella::config::Tunables) -> bool {
    f.avx2() && t.avx2.unwrap_or(true)
}

/// The pagemap guard: `true` when neither entry shows a present page that is no longer a file
/// page. A page not yet faulted in (present clear) is still the file's.
pub fn pages_unmodified(entries: [u64; 2]) -> bool {
    entries.iter().all(|&e| e & PM_PRESENT == 0 || e & PM_FILE != 0)
}

/// The system calls the swap makes, so the failure handling can be tested with injected errors.
pub trait SwapOps {
    /// The two pagemap entries of the pages at `page0` and `page0 + PAGE`, or `None` when
    /// `/proc/self/pagemap` cannot be opened or read.
    fn pagemap(&self, page0: usize) -> Option<[u64; 2]>;
    /// `mremap(from, PAGE, PAGE, MREMAP_MAYMOVE | MREMAP_FIXED, to)`.
    fn move_page(&self, from: usize, to: usize) -> Result<(), Errno>;
    /// Whether the page at `addr` is mapped (`mincore` does not fail with `ENOMEM`).
    fn mapped(&self, addr: usize) -> bool;
    /// End the process with `msg` on stderr.
    fn fatal(&self, msg: &[u8]) -> !;
}

/// The real system calls.
pub struct Kernel;

impl SwapOps for Kernel {
    fn pagemap(&self, page0: usize) -> Option<[u64; 2]> {
        let fd = CSys.openat(Fd::CWD, KStr::from_nul_terminated(b"/proc/self/pagemap\0").ok()?, O_RDONLY | O_CLOEXEC, 0).ok()?;
        let mut buf = [0u8; 16];
        let offset = i64::try_from((page0 / PAGE).saturating_mul(8)).ok();
        let read = offset.and_then(|o| CSys.pread(fd, &mut buf, o).ok());
        let _ = CSys.close(fd);
        if read != Some(buf.len()) {
            return None;
        }
        let (a, b) = buf.split_at(8);
        Some([u64::from_le_bytes(a.try_into().ok()?), u64::from_le_bytes(b.try_into().ok()?)])
    }
    fn move_page(&self, from: usize, to: usize) -> Result<(), Errno> {
        // The move replaces the live page 0 with page 1's file page, both read+exec. Sound only
        // while no other thread runs and no handler exists (start-up step 4, or a test's
        // single-threaded child): code in page 0 that is executing elsewhere would continue in
        // the other page's bytes.
        #[allow(clippy::as_conversions)] // flag bits of the uapi constants
        let flags = (MREMAP_MAYMOVE | MREMAP_FIXED) as i32;
        crate::raw::mremap_c(from, PAGE, PAGE, flags, to).map(|_| ())
    }
    fn mapped(&self, addr: usize) -> bool {
        let mut v = [0u8; 1];
        crate::raw::mincore(addr, PAGE, &mut v) != Err(Errno::ENOMEM)
    }
    fn fatal(&self, msg: &[u8]) -> ! {
        let _ = CSys.write(Fd::STDERR, msg);
        CSys.exit_group(127)
    }
}

/// The swap at start-up: `want` is [`avx2_class`] and `disabled` the COMPAT-20 tunable.
/// Records and returns the outcome.
pub fn install_with(ops: &impl SwapOps, page0: usize, want: bool, disabled: bool) -> Outcome {
    let o = decide(ops, page0, want, disabled);
    OUTCOME.store(o.code(), Relaxed);
    o
}

fn decide(ops: &impl SwapOps, page0: usize, want: bool, disabled: bool) -> Outcome {
    if !want {
        return Outcome::Sse2Class;
    }
    if disabled {
        return Outcome::Disabled;
    }
    let Some(entries) = ops.pagemap(page0) else {
        return Outcome::NoPagemap;
    };
    if !pages_unmodified(entries) {
        return Outcome::Modified;
    }
    let page1 = page0.wrapping_add(PAGE);
    if ops.move_page(page1, page0).is_ok() {
        return Outcome::Installed;
    }
    if ops.mapped(page0) {
        return Outcome::Refused;
    }
    // Page 0 is gone; page 1 is still at its own address.
    if ops.move_page(page1, page0).is_ok() {
        return Outcome::InstalledOnRetry;
    }
    ops.fatal(b"liella: start-up: the classed entry page is unmapped after a failed mremap\n")
}

/// The start-up step (static step 4, right after `cpu::init_with`).
pub(crate) fn start(f: &liella_rt::cpu::Features, t: &liella::config::Tunables) {
    let _ = install_with(&Kernel, page0_addr(), avx2_class(f, t), t.no_classed_swap == Some(true));
}

#[cfg(test)]
mod tests {
    // Clauses: no spec clause (D-4.53 start-up mechanism; the entries' behaviour is s4_1_c_1).
    #![allow(clippy::unwrap_used, clippy::panic, clippy::arithmetic_side_effects)]
    use super::*;
    use core::cell::Cell;

    struct Fake {
        entries: Option<[u64; 2]>,
        moves: Cell<u32>,
        fail_moves: u32,
        mapped_after_fail: bool,
    }

    impl SwapOps for Fake {
        fn pagemap(&self, _: usize) -> Option<[u64; 2]> {
            self.entries
        }
        fn move_page(&self, from: usize, to: usize) -> Result<(), Errno> {
            assert_eq!(from, to + PAGE, "page 1 moves over page 0");
            self.moves.set(self.moves.get() + 1);
            if self.moves.get() <= self.fail_moves { Err(Errno::ENOMEM) } else { Ok(()) }
        }
        fn mapped(&self, _: usize) -> bool {
            self.mapped_after_fail
        }
        fn fatal(&self, msg: &[u8]) -> ! {
            panic!("fatal: {}", String::from_utf8_lossy(msg))
        }
    }

    const FILE: [u64; 2] = [PM_PRESENT | PM_FILE, 0];

    fn fake(entries: Option<[u64; 2]>, fail_moves: u32, mapped_after_fail: bool) -> Fake {
        Fake { entries, moves: Cell::new(0), fail_moves, mapped_after_fail }
    }

    #[test]
    fn sse2_class_and_compat_flag_make_no_call() {
        let f = fake(None, 0, true);
        assert_eq!(install_with(&f, 0x40_0000, false, false), Outcome::Sse2Class);
        assert_eq!(install_with(&f, 0x40_0000, true, true), Outcome::Disabled);
        assert_eq!(f.moves.get(), 0);
    }

    #[test]
    fn the_guard_keeps_page0_without_pagemap_or_with_a_modified_page() {
        assert_eq!(install_with(&fake(None, 0, true), 0x40_0000, true, false), Outcome::NoPagemap);
        let anon = fake(Some([PM_PRESENT, PM_PRESENT | PM_FILE]), 0, true);
        assert_eq!(install_with(&anon, 0x40_0000, true, false), Outcome::Modified);
        let anon1 = fake(Some([PM_PRESENT | PM_FILE, PM_PRESENT]), 0, true);
        assert_eq!(install_with(&anon1, 0x40_0000, true, false), Outcome::Modified);
        assert_eq!(anon.moves.get() + anon1.moves.get(), 0);
        let clean = fake(Some(FILE), 0, true);
        assert_eq!(install_with(&clean, 0x40_0000, true, false), Outcome::Installed);
        assert_eq!(clean.moves.get(), 1);
    }

    #[test]
    fn a_refused_mremap_keeps_page0_and_a_lost_page0_is_moved_again() {
        let refused = fake(Some(FILE), 1, true);
        assert_eq!(install_with(&refused, 0x40_0000, true, false), Outcome::Refused);
        assert_eq!(refused.moves.get(), 1);
        let lost = fake(Some(FILE), 1, false);
        assert_eq!(install_with(&lost, 0x40_0000, true, false), Outcome::InstalledOnRetry);
        assert_eq!(lost.moves.get(), 2);
    }

    #[test]
    #[should_panic(expected = "unmapped after a failed mremap")]
    fn a_lost_page0_that_cannot_be_moved_again_ends_the_process() {
        let _ = install_with(&fake(Some(FILE), 2, false), 0x40_0000, true, false);
    }

    #[test]
    fn the_class_needs_avx2_with_the_ymm_state_saved_and_no_tunable() {
        let avx2_bit = 1 << 5;
        let osxsave_avx = (1 << 27) | (1 << 28);
        let mut f = liella_rt::cpu::Features { leaf1_ecx: osxsave_avx, leaf7_ebx: avx2_bit, xcr0: 0x7, ..Default::default() };
        let t = liella::config::Tunables::default();
        assert!(avx2_class(&f, &t));
        f.xcr0 = 0x3; // the OS does not save the YMM state
        assert!(!avx2_class(&f, &t));
        f.xcr0 = 0x7;
        assert!(!avx2_class(&f, &liella::config::Tunables::parse(b"glibc.cpu.hwcaps=-AVX2", false)));
        f.leaf7_ebx = 0;
        assert!(!avx2_class(&f, &t));
    }
}
