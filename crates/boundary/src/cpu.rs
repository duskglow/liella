//! The CPU paths of the C layer's machine primitives (rule P7, D-4.14), chosen once.
//!
//! The scanners pick AVX2 or SSE2 from a flag in the C layer (SSE2, valid on every x86-64 CPU,
//! until [`init`] runs); `rep movsb`/`rep stosb` are used only when CPUID reports ERMS. The
//! decision comes from `liella_rt::cpu::Features`, which counts AVX2 only when the OS saves
//! the YMM state (XCR0).

use core::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering::{Acquire, Relaxed, Release}};

use liella::config::Tunables;

use liella_rt::cpu::Features;

use crate::ffi;
use crate::sys::CSys;

const READY: u8 = 1;
const ERMS: u8 = 2;
const AVX2: u8 = 4;

static STATE: AtomicU8 = AtomicU8::new(0);

/// The decoded CPUID result, kept after the first detection (STARTUP-1): `init` (a scanner's cold
/// trampoline, possibly before startup) and `init_with` (startup, with the tunables) both decide
/// from it, and a CPUID instruction traps in a VM (0.8-1.3 us each, six per detection). CPUID
/// does not change within a process; the tunables are applied on top of it by every `init_with`.
static FEATURES_READY: AtomicBool = AtomicBool::new(false);
static FEATURES: [AtomicU64; 4] = [AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0)];

/// The low and high 32 bits of a stored word.
fn halves(w: u64) -> (u32, u32) {
    (u32::try_from(w & 0xffff_ffff).unwrap_or(0), u32::try_from(w >> 32).unwrap_or(0))
}

/// `Features::detect`, executed once per process (racing first callers each detect and store
/// the same words).
fn features() -> Features {
    if FEATURES_READY.load(Acquire) {
        let (leaf1_ecx, leaf1_edx) = halves(FEATURES[0].load(Relaxed));
        let (leaf7_ebx, leaf7_ecx) = halves(FEATURES[1].load(Relaxed));
        let (ext1_ecx, leaf7_edx) = halves(FEATURES[2].load(Relaxed));
        return Features { leaf1_ecx, leaf1_edx, leaf7_ebx, leaf7_ecx, leaf7_edx, ext1_ecx, xcr0: FEATURES[3].load(Relaxed) };
    }
    DETECTIONS.fetch_add(1, Relaxed);
    let f = Features::detect(&CSys);
    FEATURES[0].store(u64::from(f.leaf1_ecx) | u64::from(f.leaf1_edx) << 32, Relaxed);
    FEATURES[1].store(u64::from(f.leaf7_ebx) | u64::from(f.leaf7_ecx) << 32, Relaxed);
    FEATURES[2].store(u64::from(f.ext1_ecx) | u64::from(f.leaf7_edx) << 32, Relaxed);
    FEATURES[3].store(f.xcr0, Relaxed);
    FEATURES_READY.store(true, Release);
    f
}

/// How many times this process executed the CPUID detection (1 once any path is decided; a racing
/// first call may make it 2). For the test that the detection is shared.
pub fn detections() -> usize {
    DETECTIONS.load(Relaxed)
}

/// The decoded features, detecting them on first use.
pub fn features_cached() -> Features {
    features()
}

static DETECTIONS: AtomicUsize = AtomicUsize::new(0);

/// Bytes from which a disjoint copy that the AVX2 copy does not take uses `rep movsb` on an ERMS
/// CPU (in practice a CPU without AVX2; `liella.cpu.rep_movsb_threshold` overrides it). Measured
/// (MEMCPY-NONAVX2-THRESHOLD, D-4.14) on Ivy Bridge (nico, kotori; ERMS, no AVX2) against the
/// SSE2 move, cycles per call with the kernels interleaved in one pinned process
/// (tools/probe/memcpy_cycles.c): at 2-4 KiB `rep movsb` lost at every placement (1.03-1.07
/// mutually aligned, 1.28-1.47 otherwise); from 6 KiB the head-aligned kernel won at mutually
/// aligned placements (0.82-0.98 up to 64 KiB). The old 1,280 lost from 2 KiB on. Which kernel
/// from here: `hot::erms_kernel`.
pub const COPY_ERMS_MIN: usize = 6144;
/// Below this many bytes a copy whose destination and source differ modulo 64 (at a placement
/// `conv::erms_placement_ok` accepts) takes the head-aligned `rep movsb`
/// (`liella_machine_copy_erms_a64`), from it the plain one. Ivy Bridge, against the SSE2 move:
/// head-aligned 0.78-0.98 at 6-16 KiB (source +7, destination 600 bytes after the source),
/// plain 1.01-1.31 there; from 24 KiB head-aligned 1.04-1.05 at +600, plain 0.98-1.00.
pub const ERMS_A64_MISALIGNED_END: usize = 16_385;
/// Bytes from which `rep movsb` takes copies above the AVX2 copy's range on an ERMS CPU, at any
/// placement (`conv::ERMS_ANY_PLACEMENT`). With FSRM and AVX2 (N100, nozomi) it starts above
/// 1 MiB, the largest size measured: there `rep movsb` against the AVX2 copy was 0.99-1.01 at
/// mutually aligned placements and 1.08-1.73 at others (destination 600 bytes after the source
/// 1.73, plain or head-aligned); larger copies keep `rep movsb` (not measured). Ivy Bridge and
/// Broadwell keep it from 64 KiB (1 MiB: 0.80-0.89).
pub const ERMS_LARGE_FSRM_FROM: usize = 1_048_577;
/// Bytes from which `rep stosb` beats the fill loop (measured, D-4.14);
/// `liella.cpu.rep_stosb_threshold` overrides it.
pub const FILL_ERMS_MIN: usize = 1280;

/// The size range, in bytes, of the AVX2 copy: from `AVX2_COPY_MIN` up to, not including,
/// `AVX2_COPY_END` (64 KiB, the largest size measured, included). Measured (D-4.14, PJ-7) on
/// Haswell (shiki) and Zen 1 (keke), worst placement: at 128 bytes 12.2 against 12.8 ns for
/// the loops (keke 10.2 against 14.9), at 64 bytes the loops win (7.2 against 12.8); up to
/// 64 KiB it is the fastest path at every placement on shiki (16 KiB: 273 ns against 387 for
/// the loops and 1045 for `rep movsb`) and within 5% of the best one on keke.
pub const AVX2_COPY_MIN: usize = 128;
/// See [`AVX2_COPY_MIN`].
pub const AVX2_COPY_END: usize = 65_537;

/// The `rep movsb` window inside the AVX2 copy's range (MEMCPY-POLICY-2, D-4.14), per CPU class:
/// a disjoint copy of `from..end` bytes whose destination and source agree modulo 64 takes
/// `liella_machine_copy_erms_a64` (one 64-byte head store, then `rep movsb` from the aligned
/// destination) instead of the AVX2 copy. Decided by cycles:u per call, the kernels interleaved
/// in one pinned process (tools/probe/memcpy_cycles.c; paired per-round ratios, 10-90% range):
/// - ERMS, no FSRM, AVX-512F (Skylake-SP, sumire, booked): rep movsb / AVX2 0.90 at 4 KiB, 0.66
///   at 10 KiB, 0.43-0.46 at 16 KiB, 0.93-0.98 at 20-64 KiB; a loss at 2 KiB (1.21): [`AVX2_ERMS_WIDE`].
/// - ERMS, no FSRM, no AVX-512 (Broadwell, kanon): a 1-4% loss at 6-12 KiB, a win only at 16 KiB
///   (0.72-0.82; at 14 KiB 0.77 at one placement, a 1% loss at another), a tie from 20 KiB:
///   [`AVX2_ERMS_NARROW`]. Haswell (same class) not measured.
/// - FSRM (N100, nozomi): rep movsb loses 1.4-2x up to 16 KiB and ties at best above: no window.
/// - No ERMS (Zen 1, keke): no window.
///
/// Destinations that differ from the source modulo 64 keep the AVX2 copy: there rep movsb
/// (head-aligned or not) lost on every class (dst 600 bytes after the source modulo 4 KiB:
/// Skylake-SP 1.07-1.19, Broadwell 1.08-1.30, N100 1.3-2.0). `liella.cpu.rep_movsb_threshold`
/// sets the start and widens the end to [`AVX2_COPY_END`].
pub const AVX2_ERMS_WIDE: (usize, usize) = (4096, AVX2_COPY_END);
/// See [`AVX2_ERMS_WIDE`].
pub const AVX2_ERMS_NARROW: (usize, usize) = (16_384, 20_480);
static COPY_MIN: AtomicUsize = AtomicUsize::new(COPY_ERMS_MIN);
/// The start of the `rep movsb` window ([`AVX2_ERMS_WIDE`]): `usize::MAX` (no window) without
/// a window class and before the startup decision.
static AVX2_ERMS_FROM: AtomicUsize = AtomicUsize::new(usize::MAX);
/// The end (exclusive) of the `rep movsb` window: 0 without a window.
static AVX2_ERMS_TO: AtomicUsize = AtomicUsize::new(0);
static FILL_MIN: AtomicUsize = AtomicUsize::new(FILL_ERMS_MIN);
/// See [`erms_large_from`].
static ERMS_LARGE_FROM: AtomicUsize = AtomicUsize::new(AVX2_COPY_END);

/// Bytes from which a copy above the AVX2 copy's range takes `rep movsb` (ERMS CPUs; see
/// [`ERMS_LARGE_FSRM_FROM`]).
#[inline]
pub(crate) fn erms_large_from() -> usize {
    ERMS_LARGE_FROM.load(Relaxed)
}

/// [`erms_large_from`] for a CPU class: [`ERMS_LARGE_FSRM_FROM`] with FSRM and the AVX2 copy and
/// no tunable threshold, else [`AVX2_COPY_END`].
pub fn erms_large_window(fsrm: bool, avx2: bool, threshold: Option<usize>) -> usize {
    if fsrm && avx2 && threshold.is_none() { ERMS_LARGE_FSRM_FROM } else { AVX2_COPY_END }
}

/// The current `rep movsb` threshold.
#[inline]
pub(crate) fn copy_erms_min() -> usize {
    COPY_MIN.load(Relaxed)
}

/// The start of the `rep movsb` window of the AVX2 copy range (`usize::MAX`: none).
#[inline]
pub(crate) fn avx2_erms_from() -> usize {
    AVX2_ERMS_FROM.load(Relaxed)
}

/// The end (exclusive) of the `rep movsb` window of the AVX2 copy range (0: none).
#[inline]
pub(crate) fn avx2_erms_to() -> usize {
    AVX2_ERMS_TO.load(Relaxed)
}

/// The `rep movsb` window for a CPU class ([`AVX2_ERMS_WIDE`]): `(usize::MAX, 0)` for none.
pub fn avx2_erms_window(erms: bool, fsrm: bool, avx512f: bool, threshold: Option<usize>) -> (usize, usize) {
    match (erms, threshold) {
        (false, _) => (usize::MAX, 0),
        (true, Some(t)) => (t, AVX2_COPY_END),
        (true, None) if fsrm => (usize::MAX, 0),
        (true, None) if avx512f => AVX2_ERMS_WIDE,
        (true, None) => AVX2_ERMS_NARROW,
    }
}

fn set_avx2_erms_window((from, to): (usize, usize)) {
    AVX2_ERMS_FROM.store(from, Relaxed);
    AVX2_ERMS_TO.store(to, Relaxed);
}

/// The current `rep stosb` threshold.
#[inline]
pub(crate) fn fill_erms_min() -> usize {
    FILL_MIN.load(Relaxed)
}

/// The decided flags, deciding them on first use.
#[inline]
fn flags() -> u8 {
    match STATE.load(Acquire) {
        0 => init(),
        s => s,
    }
}

/// The scanners' path as the kernels see it (`liella_machine_scan_level`: 0 = SSE2, 1 = AVX2).
/// The scanning exports make no per-call "decided yet?" test (SHORT-3, D-4.39): the SSE2 default
/// is correct before [`init_with`] runs at startup. Tests compare it with [`avx2`].
#[cfg(feature = "selftest")]
pub fn scan_level() -> u32 {
    ffi::liella_machine_scan_level_get()
}

/// Decide the paths from CPUID (later calls repeat the same decision). Never inlined: the
/// scanning exports keep no frame for this one-time path (STR-K1).
#[cold]
#[inline(never)]
pub fn init() -> u8 {
    init_with(&Tunables::default())
}

/// Decide the paths from CPUID and the tunables (startup, spec §1.8): a tunable can switch a
/// feature off, or ask for it (honoured only if the CPU has it), and set the thresholds.
pub fn init_with(t: &Tunables) -> u8 {
    let f = features();
    let avx2 = f.avx2() && t.avx2.unwrap_or(true);
    let erms = f.erms() && t.erms.unwrap_or(true);
    ffi::liella_machine_set_scan_level(u32::from(avx2));
    // The hot kernels' builds (D-4.25): the same decision as the scanners'.
    crate::dispatch::select(avx2);
    if let Some(n) = t.rep_movsb_threshold {
        COPY_MIN.store(n, Relaxed);
    }
    if let Some(n) = t.rep_stosb_threshold {
        FILL_MIN.store(n, Relaxed);
    }
    set_avx2_erms_window(avx2_erms_window(erms, f.fsrm(), f.avx512f(), t.rep_movsb_threshold));
    ERMS_LARGE_FROM.store(erms_large_window(f.fsrm(), avx2, t.rep_movsb_threshold), Relaxed);
    let s = READY | if erms { ERMS } else { 0 } | if avx2 { AVX2 } else { 0 };
    STATE.store(s, Release);
    s
}

/// Make sure the paths are decided (the scanners are correct either way; this is for speed).
#[inline]
pub(crate) fn ready() {
    flags();
}

/// Whether `rep movsb`/`rep stosb` are fast on this CPU.
#[inline]
pub(crate) fn erms() -> bool {
    flags() & ERMS != 0
}

/// [`erms`] without deciding the paths first: `false` before the startup decision. For the
/// large-fill entry (`hot::fill_large`), whose fallback is correct either way and decides them:
/// with a possible `init` call inline, LLVM saved five registers on every large fill.
#[inline]
pub(crate) fn erms_decided() -> bool {
    STATE.load(Relaxed) & ERMS != 0
}

/// Whether the AVX2 copy may run (the same decision as the scanners').
#[inline]
pub(crate) fn copy_avx2() -> bool {
    flags() & AVX2 != 0
}

/// The casecmp kernels (CASECMP): none (the measured path), SSE2 (every x86-64 CPU), AVX2.
pub(crate) const CASECMP_MEASURED: u8 = 0;
/// See [`CASECMP_MEASURED`].
pub(crate) const CASECMP_SSE2: u8 = 1;
/// See [`CASECMP_MEASURED`].
pub(crate) const CASECMP_AVX2: u8 = 2;
/// No self-test override: the CPU decides.
const CASECMP_AUTO: u8 = u8::MAX;

/// Set by the self-tests to force a casecmp kernel (never in the library).
#[cfg(feature = "selftest")]
static CASECMP_FORCE: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(CASECMP_AUTO);

/// The kernel `strcasecmp`/`strncasecmp` use (CASECMP-K1, CASECMP): AVX2 where the CPU and OS
/// have it, else SSE2, unless the self-tests forced one.
#[inline]
pub(crate) fn casecmp_kernel() -> u8 {
    // The override exists only for the self-tests: the library makes no per-call load of it.
    #[cfg(not(feature = "selftest"))]
    let forced = CASECMP_AUTO;
    #[cfg(feature = "selftest")]
    let forced = CASECMP_FORCE.load(Relaxed);
    match forced {
        CASECMP_AUTO => {
            if flags() & AVX2 != 0 {
                CASECMP_AVX2
            } else {
                CASECMP_SSE2
            }
        }
        k => k,
    }
}

/// [`casecmp_kernel`] without deciding the CPU flags: `CASECMP_MEASURED` until they are decided,
/// which sends the caller to its out-of-line path (that one calls [`casecmp_kernel`]), so the
/// exports' kernel path holds no call and keeps no registers (STRCASECMP-SHORT-2).
#[inline]
pub(crate) fn casecmp_kernel_decided() -> u8 {
    #[cfg(not(feature = "selftest"))]
    let forced = CASECMP_AUTO;
    #[cfg(feature = "selftest")]
    let forced = CASECMP_FORCE.load(Relaxed);
    match forced {
        CASECMP_AUTO => match STATE.load(Acquire) {
            0 => CASECMP_MEASURED,
            s if s & AVX2 != 0 => CASECMP_AVX2,
            _ => CASECMP_SSE2,
        },
        k => k,
    }
}

/// Force the casecmp kernel (self-tests): `None` puts the CPU decision back. The caller checks
/// that the CPU has a forced AVX2.
#[cfg(feature = "selftest")]
pub(crate) fn force_casecmp_kernel(kernel: Option<u8>) {
    CASECMP_FORCE.store(kernel.unwrap_or(CASECMP_AUTO), Relaxed);
}

/// Force the ERMS flag on or off (self-tests, MEM-BULK): `rep movsb`/`rep stosb` are correct on
/// every x86-64 CPU and the flag only chooses speed, so the `rep` paths can run on a gate machine
/// without ERMS (Zen 1). [`init`] puts the CPUID decision back.
#[cfg(feature = "selftest")]
pub(crate) fn force_erms(on: bool) {
    flags();
    set_avx2_erms_window(if on { AVX2_ERMS_NARROW } else { (usize::MAX, 0) });
    if on {
        STATE.fetch_or(ERMS, Release);
    } else {
        STATE.fetch_and(!ERMS, Release);
    }
}

/// Whether the scanners may use AVX2.
pub fn avx2() -> bool {
    flags() & AVX2 != 0
}
