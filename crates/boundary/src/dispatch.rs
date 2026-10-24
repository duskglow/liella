//! CPU-variant selection for the hot kernels (owner ruling 2026-10-08 "CPU-variant kernels",
//! D-4.25): one decision at startup, read by the kernels as one byte.
//!
//! Mechanism: a flag byte in the library's data (machine/fold.S), `false` (the SSE2 baseline, valid on every x86-64
//! CPU) until [`crate::cpu::init_with`] decides the CPU paths at startup. A kernel that has an
//! AVX2 build tests the flag only on the sizes where the AVX2 build differs (one load and one
//! test-and-branch) and jumps to it directly. No relocation, pointer or loader feature is
//! involved, so it is correct in `libc.so.6` and in `libc.a` static programs from their first
//! instruction (before the static startup relocates itself, every kernel runs its baseline
//! build). Why not `STT_GNU_IFUNC` / `R_X86_64_IRELATIVE` or a resolved-once pointer: see D-4.25.
//!
//! Proof obligation (every `UNSAFE(hot-kernel)` call to an AVX2 build relies on it): the flag is
//! `true` only while the CPU has AVX2 and the OS saves the YMM state, which [`select`]'s callers
//! establish (`cpu::init_with` from `liella_rt::cpu::Features`; `selftest` refuses otherwise).

use core::sync::atomic::{AtomicBool, Ordering::Relaxed};

use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{O_CLOEXEC, O_RDONLY};

// The flag itself lives in machine/fold.S (`liella_machine_avx2`, the fold word's line), where the
// direct copy entry's 65-128 class reads it (COPY-128).
use crate::ffi::liella_machine_avx2 as AVX2;

/// Whether the kernels run their AVX2 builds.
#[inline(always)]
pub(crate) fn avx2() -> bool {
    AVX2.load(Relaxed)
}

/// Select the kernels' builds. `true` only when the CPU has AVX2 and the OS saves the YMM state
/// (module proof obligation).
pub(crate) fn select(avx2: bool) {
    if !avx2 {
        // The 65-128 and 129-2048 class words drop before the flag (never above 64 while the flag
        // is false).
        CMP_LONG_WORD.store(0, Relaxed);
        CMP_MID.store(0, Relaxed);
        store_cmp_gates(FOLD.load(Relaxed) != 0, false);
    }
    AVX2.store(avx2, Relaxed);
    update_cmp_gates();
    if avx2 {
        CMP_MID.store(128, Relaxed);
        CMP_LONG_WORD.store(crate::hot::CMP_LONG, Relaxed);
    }
}

// The compare entries' 65-128 class word (machine/fold.S, MEMCMP-MID-3): one `cmp`/`ja` against it
// sends sizes above it to the checked entries, replacing a range test and a flag test. It depends
// on the AVX2 flag alone (not on folding: option B keeps the explicit NULL tests above 64).
use crate::ffi::liella_machine_cmp_mid as CMP_MID;
// The compare entries' 129-2048 class word (machine/fold.S, MEMCMP-MID-4): the same rule as the
// 65-128 word. The order of the two stores does not matter for the class (it re-tests n <= 128);
// only the flag rule does (both are zero whenever the flag is false).
use crate::ffi::liella_machine_cmp_long as CMP_LONG_WORD;

// The compare entries' class words (machine/fold.S, MEMCMP-EQ64 + SEQ-REGRESS): the entries add
// them in turn to `n - 65`; a carry from WIDE sends 33-64 to the folded AVX2 blocks, a carry from
// LOW the other folded sizes to the head probe.
use crate::ffi::liella_machine_cmp_low as CMP_LOW;
use crate::ffi::liella_machine_cmp_wide as CMP_WIDE;

/// Recompute the compare entries' class words from the fold limit and the AVX2 flag. Called after
/// every write of either; before either is switched off, [`store_cmp_gates`] first stores the words
/// of the off state, so WIDE is never nonzero with folding or AVX2 off and LOW never with folding
/// off. Production writes the flag once at startup and the fold limit once (0 to 64); the selftest
/// toggles run serialized by its lock.
fn update_cmp_gates() {
    store_cmp_gates(FOLD.load(Relaxed) != 0, AVX2.load(Relaxed));
}

/// Store the class words for `fold` / `avx2`: WIDE 32 only with both (33-64 carry), LOW 25 with
/// both (8-32 carry), 57 with folding alone (8-64), 0 with folding off. Their sum never exceeds
/// 57, so `n - 65 + WIDE + LOW` carries only for `8..=64`, also between the two stores (a racing
/// call may see one old word): a smaller WIDE is stored before LOW grows, LOW shrinks before WIDE
/// grows.
fn store_cmp_gates(fold: bool, avx2: bool) {
    let wide = if fold && avx2 { 32 } else { 0 };
    let low = if !fold { 0 } else if avx2 { 25 } else { 57 };
    if wide < CMP_WIDE.load(Relaxed) {
        CMP_WIDE.store(wide, Relaxed);
        CMP_LOW.store(low, Relaxed);
    } else {
        CMP_LOW.store(low, Relaxed);
        CMP_WIDE.store(wide, Relaxed);
    }
}

/// NULL folding (owner rulings 2026-10-08, D-4.22): the copy and fill entries take sizes
/// `1..=FOLD` straight to their assembly classes (`csys/src/machine/fold.S`) with no NULL test,
/// where a NULL faults on page zero (SIGSEGV, what glibc observably does: it has no NULL check of
/// its own). Every other size, 65-128 included, keeps the explicit S6 checks in Rust (option B:
/// no Rust code performs a deliberate NULL access). `0` (the start value, and the value whenever
/// page zero could be mapped) keeps the explicit checks for every size.
///
/// Proof obligation (every fold relies on it): `FOLD` is nonzero only while no access in
/// `[0, 64 KiB)` can succeed, which [`page_zero_unmappable`] establishes at startup
/// (`runtime`; `selftest` forces it in tests). A folded kernel touches at most
/// `[d, d + FOLD_MAX)` with `FOLD_MAX` = 64 <= 64 KiB, so a NULL `d` or `s` faults.
// The word itself lives in machine/fold.S (`liella_machine_fold_limit`), which reads it on every
// call (D-4.22 COPY-FILL).
use crate::ffi::liella_machine_fold_limit as FOLD;
// The memchr entry's gate (machine/fold.S, MEMCHR-FOLD): `isize::MAX` with folding on, else 0. Its
// folded calls touch `[s & -64, s + 64)` first, page zero for a NULL `s`, so the same guard covers
// them for every `n` (the module proof obligation).
use crate::ffi::liella_machine_chr_limit as FOLD_CHR;

/// The folded sizes: `1..=FOLD_MAX`, the classes the entries keep inline (D-4.22).
pub(crate) const FOLD_MAX: usize = 64;

/// Whether NULL folding is on (the explicit S6 checks are off for the kernel sizes).
#[inline(always)]
pub(crate) fn null_folded() -> bool {
    FOLD.load(Relaxed) != 0
}

/// Whether the folding decision has been taken (STARTUP-1: the `vm.mmap_min_addr` read is lazy).
/// Until it is, `FOLD` is 0 and every size takes the explicit S6 checks, which is always correct.
static FOLD_RESOLVED: AtomicBool = AtomicBool::new(false);

/// Turn NULL folding on or off. On only when page zero cannot be mapped (module proof obligation).
/// Also marks the decision as taken, so a forced setting is never overridden by [`resolve_fold`].
pub(crate) fn select_null_fold(fold: bool) {
    if !fold {
        store_cmp_gates(false, false);
    }
    FOLD.store(if fold { FOLD_MAX } else { 0 }, Relaxed);
    FOLD_CHR.store(if fold { isize::MAX.unsigned_abs() } else { 0 }, Relaxed);
    update_cmp_gates();
    FOLD_RESOLVED.store(true, Relaxed);
}

/// Take the folding decision on first use (STARTUP-1): called from the explicit-check paths of the
/// copy and fill entries for sizes `1..=FOLD_MAX`, the only sizes folding could speed up. The
/// first such call reads `vm.mmap_min_addr` (3 system calls) and later calls cost one load. Racing
/// callers read the same value; the stores agree. Until then the explicit checks run, so a call
/// that arrives before the decision is correct, only not folded.
#[inline(always)]
pub(crate) fn resolve_fold(n: usize) {
    if n <= FOLD_MAX && !FOLD_RESOLVED.load(Relaxed) {
        resolve_fold_slow();
    }
}

/// [`resolve_fold`] for an entry whose folded sizes are not bounded by `FOLD_MAX` (`memchr`,
/// MEMCHR-FOLD): the decision is taken on the first checked call of any size.
#[inline(always)]
pub(crate) fn resolve_fold_any() {
    if !FOLD_RESOLVED.load(Relaxed) {
        resolve_fold_slow();
    }
}

/// Set while one caller reads `vm.mmap_min_addr`. The read's own system-call wrappers may call
/// `memset` (a debug build zero-fills its argument array that way), which lands back in
/// [`resolve_fold`]; that nested call, and any racing thread, returns at once and takes the explicit
/// checks, which is always correct. Without it the nested call recursed until the stack overflowed.
static FOLD_RESOLVING: AtomicBool = AtomicBool::new(false);

#[cold]
#[inline(never)]
fn resolve_fold_slow() {
    if FOLD_RESOLVING.swap(true, Relaxed) {
        return;
    }
    select_null_fold(page_zero_unmappable(&crate::sys::CSys));
    FOLD_RESOLVING.store(false, Relaxed);
}

/// Test-only: forget the folding decision (folding off, undecided), as at process start.
#[cfg(feature = "selftest")]
pub(crate) fn unresolve_null_fold() {
    store_cmp_gates(false, false);
    FOLD.store(0, Relaxed);
    FOLD_CHR.store(0, Relaxed);
    FOLD_RESOLVED.store(false, Relaxed);
    FOLD_RESOLVING.store(false, Relaxed);
}

/// Whether the folding decision has been taken.
pub(crate) fn fold_resolved() -> bool {
    FOLD_RESOLVED.load(Relaxed)
}

/// The startup guard: `true` only when `vm.mmap_min_addr` reads as at least 64 KiB. Unreadable
/// (no `/proc`, a sandbox) or malformed is `false`: the explicit checks stay. Three system calls,
/// once per process.
pub(crate) fn page_zero_unmappable<S: Sys>(sys: &S) -> bool {
    let Ok(path) = KStr::from_nul_terminated(b"/proc/sys/vm/mmap_min_addr\0") else { return false };
    let Ok(fd) = sys.openat(Fd::CWD, path, O_RDONLY | O_CLOEXEC, 0) else { return false };
    let mut buf = [0u8; 32];
    let r = sys.read(fd, &mut buf);
    let _ = sys.close(fd);
    match r {
        Ok(n) => min_addr_allows_fold(buf.get(..n).unwrap_or(&[])),
        Err(_) => false,
    }
}

/// Whether the text of `vm.mmap_min_addr` (decimal digits, an optional final newline) is at
/// least 64 KiB. Anything else (empty, other bytes, overflow, a full buffer that may be cut) is
/// `false`.
pub(crate) fn min_addr_allows_fold(text: &[u8]) -> bool {
    let digits = text.strip_suffix(b"\n").unwrap_or(text);
    if digits.is_empty() || text.len() >= 32 {
        return false;
    }
    let mut v: u64 = 0;
    for &c in digits {
        if !c.is_ascii_digit() {
            return false;
        }
        let Some(next) = v.checked_mul(10).and_then(|x| x.checked_add(u64::from(c.wrapping_sub(b'0')))) else { return false };
        v = next;
    }
    v >= 0x1_0000
}
