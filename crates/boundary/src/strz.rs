//! Category *slice*: NUL-terminated C text read into Rust views. Rust may not read past the
//! terminator (finding F1), so byte text is measured by the C layer's scanners (OD-C2, rule P7,
//! D-4.14), which read aligned vectors that never cross a page. Rust checks what it can in
//! constant time: the reported byte is the terminator, and a bounded result is within its
//! bound. An answer outside the bound is a library bug and aborts; a reported byte that no
//! longer matches only proves that a writer (another thread, a signal handler, another process
//! sharing the page) changed the text during the call, so the text is rescanned once from that
//! index (D-4.21: abort only on answers no writer can cause). Wide text is still scanned unit by
//! unit.
//!
//! Every view excludes the terminator. A NULL pointer is `EFAULT` (rule S6: callers without an
//! error channel turn it into a diagnostic).

use liella_kernel::errno::{Errno, KResult};
use liella_kernel::fd::Fd;
use liella_kernel::sys::Sys;

use crate::ffi;
use crate::sys::CSys;

/// A scanner gave an answer no concurrent writer can cause (an index past its bound): report and
/// abort (never expected; tested exhaustively). D-4.21.
#[cold]
pub(crate) fn scanner_fault() -> ! {
    let _ = CSys.write(Fd::STDERR, b"liella: string scanner postcondition failed\n");
    crate::ret::abort()
}

/// Rescans taken because the text changed under a scanner (self-tests only).
#[cfg(feature = "selftest")]
pub(crate) static RESCANS: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// `strcasecmp`/`strncasecmp` calls decided by the AVX2 kernel and calls that took the measured
/// path (self-tests only).
#[cfg(feature = "selftest")]
pub(crate) static CASECMP_KERNEL: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);
/// See [`CASECMP_KERNEL`].
#[cfg(feature = "selftest")]
pub(crate) static CASECMP_MEASURED: core::sync::atomic::AtomicUsize = core::sync::atomic::AtomicUsize::new(0);

/// Count a casecmp decision: `kernel` for the AVX2 kernel's, else the measured path's (self-tests only).
#[inline]
pub(crate) fn note_casecmp(kernel: bool) {
    #[cfg(feature = "selftest")]
    {
        let counter = if kernel { &CASECMP_KERNEL } else { &CASECMP_MEASURED };
        counter.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
    }
    #[cfg(not(feature = "selftest"))]
    let _ = kernel;
}

/// Count a rescan (self-tests only; nothing in the library build).
#[inline]
pub(crate) fn note_rescan() {
    #[cfg(feature = "selftest")]
    RESCANS.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
}

/// Bytes before the first NUL at `p`, at most `limit`, measured by the C layer's scanners.
///
/// # Safety
/// `p` is non-NULL and points to readable bytes up to and including a NUL, or `limit` bytes.
// UNSAFE(slice): checking a scanner's answer: the byte at the reported index, then a rescan after it if a writer changed it (D-4.21).
#[allow(unsafe_code)]
unsafe fn byte_len(p: *const u8, limit: usize) -> usize {
    crate::cpu::ready();
    let n = if limit == usize::MAX { ffi::liella_machine_strlen(p) } else { ffi::liella_machine_strnlen(p, limit) };
    // UNSAFE(slice): checking a scanner's answer: the byte at the reported index, then a rescan after it if a writer changed it (D-4.21).
    // SAFETY: the function's contract; `n` is the scanner's answer for `p` and `limit`.
    unsafe { settle_len(p, limit, n) }
}

/// Check the answer `n` a length scanner gave for `p` and `limit` (D-4.21): past the limit is a
/// scanner bug and aborts; a byte at `n` that is no longer NUL was changed by a writer during the
/// call, so the rest is rescanned once, unit by unit from `n + 1`, and that pass's own reads
/// decide the answer.
///
/// # Safety
/// As for `byte_len`; `n` is what a scanner reported for this string.
#[inline]
// UNSAFE(slice): the postcondition check reads the one byte the scanner reported.
#[allow(unsafe_code)]
pub(crate) unsafe fn settle_len(p: *const u8, limit: usize, n: usize) -> usize {
    if n > limit {
        scanner_fault();
    }
    if n < limit {
        // UNSAFE(slice): one byte at the reported terminator.
        #[allow(unsafe_code)]
        // SAFETY: the string is readable through its terminator (contract); the scanner
        // reported the terminator at `n`.
        let b = unsafe { byte_at(p, n) };
        if b != 0 {
            // UNSAFE(slice): rescanning C text after the reported index, whose byte a writer changed (D-4.21).
            // SAFETY: byte `n` is not NUL, so the string goes on past it (contract).
            return unsafe { rescan_len(p, limit, n) };
        }
    }
    n
}

/// `settle_len`'s rescan: `n` plus the units after it up to the first NUL, within `limit`.
///
/// # Safety
/// Byte `n < limit` of the string at `p` was just read as not NUL.
#[cold]
#[inline(never)]
#[allow(clippy::arithmetic_side_effects)] // n < limit
// UNSAFE(slice): measuring the rest of the text unit by unit.
#[allow(unsafe_code)]
unsafe fn rescan_len(p: *const u8, limit: usize, n: usize) -> usize {
    note_rescan();
    let next = n + 1;
    // UNSAFE(slice): rescanning C text after the reported index, whose byte a writer changed (D-4.21).
    // SAFETY: the string goes on past byte `n` (contract), so `p + n + 1` is its next unit or
    // its terminator; `scan` reads no further than the first NUL or `limit - n - 1` units.
    next + unsafe { scan(p.wrapping_add(next), limit - next) }
}

/// The first index `>= from` of a byte equal to `c`, read unit by unit (`rawmemchr`'s rescan,
/// D-4.21; each byte is read once, so the answer agrees with the bytes read).
///
/// # Safety
/// Bytes `from..` at `p` are readable up to and including one equal to `c`.
#[cold]
#[inline(never)]
#[allow(clippy::arithmetic_side_effects)] // the occurrence lies within the address space
// UNSAFE(slice): reading C text one unit at a time.
#[allow(unsafe_code)]
unsafe fn rescan_until(p: *const u8, from: usize, c: u8) -> usize {
    note_rescan();
    let mut i = from;
    // SAFETY: the function's contract: no earlier byte was `c`, so byte `i` is readable.
    while unsafe { byte_at(p, i) } != c {
        i += 1;
    }
    i
}

/// The bytes of the C string at `p`, one per call, then its terminator (0) on every later
/// call without reading again: a cursor never moves past the terminator it read.
///
/// # Safety
/// `p` points to a readable NUL-terminated string for the cursor's life.
// UNSAFE(slice): reading C text one unit at a time, up to the first NUL read.
#[allow(unsafe_code)]
unsafe fn cursor(mut p: *const u8) -> impl FnMut() -> u8 {
    static END: u8 = 0;
    move || {
        // UNSAFE(slice) SAFETY: no byte before `p` was NUL (each one read moved `p` on; the
        // NUL read moves it to `END`), so `p` is a byte of the string, its terminator, or `END`.
        let x = unsafe { byte_at(p, 0) };
        p = if x == 0 { &raw const END } else { p.wrapping_add(1) };
        x
    }
}

/// `strcoll` in `l` (D-4.37) as an `int`: byte order (as [`order`]) without collation rules;
/// otherwise most pairs are decided by the two bytes at the first difference (STRCOLL-3), the
/// rest by the single pass, and what neither decides (NULL included) by [`coll_general`]. The shared
/// prefix is found first by the C layer's kernel (`liella_machine_strdiff`, D-4.37), which also
/// reports whether a byte of it is non-ASCII (STRCOLL-4); the bytes at its index are read here
/// and the single pass reads on from it itself (the answer never comes from the kernel's scan).
///
/// # Safety
/// `a` and `b` are NULL or point to readable NUL-terminated strings (the C contract of the
/// export calling this).
#[inline(always)]
// UNSAFE(slice): reading the deciding bytes of C text; the single pass reads on through `cursor`.
#[allow(unsafe_code)]
pub unsafe fn coll(a: *const i8, b: *const i8, l: &liella::locale::Locale) -> i32 {
    if l.code_point_order() {
        // UNSAFE(slice) SAFETY: the caller's contract. The direct entry takes NULL in its kernel's
        // slow path (STRCMP-HEADNULL), which aborts through `order`: no NULL tests here, so a
        // code-point-order strcoll pays the kernel's head only (SORT-UTF8-DRIFT).
        return unsafe { order_direct(a, b) };
    }
    if a.is_null() || b.is_null() {
        // UNSAFE(slice) SAFETY: the caller's contract.
        return unsafe { coll_general(a, b, l) };
    }
    let (a, b) = (a.cast::<u8>(), b.cast::<u8>());
    let d = ffi::liella_machine_strdiff(a, b);
    // UNSAFE(slice) SAFETY: a + index is a byte of the string or its terminator (the kernel's
    // index: the bytes before it are not terminators).
    let x = unsafe { byte_at(a, d.index) };
    // UNSAFE(slice) SAFETY: b + index likewise (its bytes before are a's, none a terminator).
    let y = unsafe { byte_at(b, d.index) };
    if let Some(o) = l.decide_first_flag(d.high != 0, x, y) {
        return crate::ret::order(Ok(o));
    }
    // UNSAFE(slice) SAFETY: the bytes before index are shared and not terminators (the
    // kernel's index), so a and b with it satisfy coll_stream_rest's contract.
    match unsafe { coll_stream_rest(a, b, d.index, l) } {
        Some(o) => crate::ret::order(Ok(o)),
        // UNSAFE(slice) SAFETY: the caller's contract (a and b are its strings, cast back).
        None => unsafe { coll_general(a.cast(), b.cast(), l) },
    }
}

/// `strcoll`'s general path (D-4.18): the whole strings' collation keys compared; NULL aborts
/// (`ret::order` on the error). Out of line and cold: the common pairs never reach it (D-4.37).
///
/// # Safety
/// `a` and `b` are NULL or point to readable NUL-terminated strings.
#[cold]
#[inline(never)]
// UNSAFE(slice): measuring C text through the scanners.
#[allow(unsafe_code)]
unsafe fn coll_general(a: *const i8, b: *const i8, l: &liella::locale::Locale) -> i32 {
    // UNSAFE(slice) SAFETY: the caller's contract: a and b are NULL or strings (§4.3; D-4.18).
    crate::ret::order(unsafe { strz(a) }.and_then(|x| unsafe { strz(b) }.map(|y| l.compare_bytes(x, y))))
}

/// The single pass after the shared prefix of `i` bytes (D-4.37), out of line so that the
/// decision at the first difference above keeps a small frame (STRCOLL-3: inlined, its
/// register saves cost every call).
///
/// # Safety
/// `a` and `b` point to strings whose first `i` bytes are shared and not terminators.
#[inline(never)]
// UNSAFE(slice): reading C text one unit at a time through `cursor`.
#[allow(unsafe_code)]
unsafe fn coll_stream_rest(a: *const u8, b: *const u8, i: usize, l: &liella::locale::Locale) -> Option<core::cmp::Ordering> {
    // UNSAFE(slice) SAFETY: the caller's contract: the i bytes at `a` precede a's terminator
    // (none of them is one), so they are readable string bytes; read during this call only.
    let prefix = unsafe { core::slice::from_raw_parts(a, i) };
    // UNSAFE(slice) SAFETY: a + i and b + i are bytes of the strings or their terminators (the
    // i bytes before each are shared and not terminators), read during this call only.
    l.compare_stream(prefix, unsafe { cursor(a.wrapping_add(i)) }, unsafe { cursor(b.wrapping_add(i)) })
}

/// The byte at `p + i`.
///
/// # Safety
/// `p + i` is readable: a byte of a string at `p` that no earlier byte terminates, or its
/// terminator.
// UNSAFE(slice): the scanners' postcondition checks read the one byte a scanner reported.
#[allow(unsafe_code)]
unsafe fn byte_at(p: *const u8, i: usize) -> u8 {
    // SAFETY: the caller's contract.
    unsafe { p.wrapping_add(i).read() }
}

/// The order of the strings at `a` and `b` as `strcmp` defines it (unsigned bytes): -1, 0 or 1,
/// from the C layer's one-pass kernel (`liella_machine_strcmp`, PJ-8; STRCOLL-1, D-4.29), returned
/// directly (a tail call: the kernel itself reads the two deciding bytes again and rescans after
/// a writer's change, so the answer always comes from bytes the call read). No CPU decision is
/// needed first: the runtime decides the scan level at startup, and before that the kernel's
/// SSE2 steps (correct on every x86-64) run. NULL aborts (S6).
///
/// # Safety
/// `a` and `b` are NULL or point to readable NUL-terminated strings (the C contract of the
/// export calling this).
#[inline]
// UNSAFE(slice): the kernel reads C text up to the deciding bytes (D-4.29).
#[allow(unsafe_code)]
pub unsafe fn order(a: *const i8, b: *const i8) -> i32 {
    if a.is_null() || b.is_null() {
        null_fault();
    }
    ffi::liella_machine_strcmp(a.cast(), b.cast())
}

/// [`order`] through the direct entry `liella_machine_strcmp_entry` (STRCMP-ENTRY): `strcmp` in
/// the products; its NULL tests (in the kernel's slow path, which the head's page test sends
/// NULL to: STRCMP-HEADNULL) jump to `liella_hot_strcmp_null`, which aborts through [`order`].
///
/// # Safety
/// As [`order`].
#[inline]
// UNSAFE(slice): the kernel reads both strings to their first difference (§4.3).
#[allow(unsafe_code)]
pub unsafe fn order_direct(a: *const i8, b: *const i8) -> i32 {
    ffi::liella_machine_strcmp_entry(a.cast(), b.cast())
}

/// Rescans `liella_machine_strcmp` made after a writer's change (self-tests only).
#[cfg(feature = "selftest")]
pub(crate) fn strcmp_rescans() -> usize {
    usize::try_from(ffi::liella_machine_strcmp_rescan_count()).unwrap_or(usize::MAX)
}

/// Check an answer `i` a `strcmp` scanner gave for `a` and `b` (D-4.21) and order the bytes
/// there (selftest only since D-4.29: the kernel now settles its own answer the same way). Bytes that are now equal and not NUL were changed by a writer during the call, so the
/// rest is compared once more, unit by unit from `i + 1`, and that pass's own reads decide the
/// order. No index is impossible here: the strings bound it, not a limit Rust knows.
///
/// # Safety
/// As for `compare`, with non-NULL `a` and `b`; `i` is what the scanner reported for them.
#[cfg(feature = "selftest")]
#[inline]
// UNSAFE(slice): reading the two bytes at the reported index of C text.
#[allow(unsafe_code)]
pub(crate) unsafe fn settle_compare(a: *const u8, b: *const u8, i: usize) -> core::cmp::Ordering {
    // UNSAFE(slice): `a`'s byte at the reported index.
    #[allow(unsafe_code)]
    // SAFETY: the scanner reported the first index where the strings differ or `a` ends, so
    // bytes 0..i of both are equal and not NUL: neither string ended before i.
    let x = unsafe { byte_at(a, i) };
    // UNSAFE(slice): `b`'s byte at the reported index.
    #[allow(unsafe_code)]
    // SAFETY: as for `x`.
    let y = unsafe { byte_at(b, i) };
    if x == y && x != 0 {
        // SAFETY: both strings go on past byte `i`, which was just read as equal and not NUL.
        return unsafe { rescan_compare(a, b, i) };
    }
    x.cmp(&y)
}

/// `settle_compare`'s rescan: the order of the first bytes after `i` that differ or are NUL.
///
/// # Safety
/// Byte `i` of both strings was just read as equal and not NUL.
#[cfg(feature = "selftest")]
#[cold]
#[inline(never)]
#[allow(clippy::arithmetic_side_effects)] // a C string cannot exceed the address space
// UNSAFE(slice): comparing C text one unit at a time.
#[allow(unsafe_code)]
unsafe fn rescan_compare(a: *const u8, b: *const u8, i: usize) -> core::cmp::Ordering {
    note_rescan();
    let mut k = i + 1;
    loop {
        // UNSAFE(slice): rescanning C text after the reported index, whose byte a writer changed (D-4.21).
        // SAFETY: bytes 0..k of both strings were read as equal and not NUL, so byte `k` of each
        // is a further byte or its terminator.
        let x = unsafe { byte_at(a, k) };
        // SAFETY: as for `x`.
        let y = unsafe { byte_at(b, k) };
        if x != y || x == 0 {
            return x.cmp(&y);
        }
        k += 1;
    }
}

/// The order of the first `n` bytes of the strings at `a` and `b` after `tolower` in `loc`
/// (`strcasecmp` with `n` = `usize::MAX`, `strncasecmp`). Where the locale's case maps of 0..127
/// are ASCII's (`ctype::ascii_case`), one pass of the C layer's `liella_machine_casecmp_avx2`
/// (CPUs with AVX2) or `liella_machine_casecmp_sse2` (the others, CASECMP) finds the first index where the ASCII-folded bytes differ or `a`
/// ends (CASECMP-K1), and the `liella_machine_casecmp_ord_*` wrapper around it orders the two bytes there (STRCASECMP-SHORT). Everything else, and every case the
/// kernel leaves open (a byte of 128 or more at the index, whose fold the locale decides; a
/// concurrent writer), takes the path of before: both strings measured, `ctype::compare_folded`.
///
/// # Safety
/// `a` and `b` are NULL or point to readable bytes up to a NUL or `n` bytes (the C contract of
/// the export calling this), not written for the call except by another thread (D-4.21).
#[inline]
// UNSAFE(slice): reading the two bytes at the index the kernel reported.
#[allow(unsafe_code)]
pub unsafe fn casecmp(a: *const i8, b: *const i8, n: usize, loc: &liella::locale::Locale) -> KResult<core::cmp::Ordering> {
    // UNSAFE(slice): the same call with the locale's ASCII case flag worked out here.
    // SAFETY: the function's contract.
    unsafe { casecmp_in(a, b, n, liella::ctype::ascii_case(loc), loc) }
}

/// [`casecmp`] with `ascii` = `ctype::ascii_case(loc)` given by the caller, so that the exports
/// on the thread's current locale read it from the locale view in one load (CASECMP3).
///
/// # Safety
/// As for [`casecmp`]; `ascii` is `ctype::ascii_case(loc)`.
#[inline]
// UNSAFE(slice): the kernel reads C text under the function's contract.
#[allow(unsafe_code)]
pub unsafe fn casecmp_in(a: *const i8, b: *const i8, n: usize, ascii: bool, loc: &liella::locale::Locale) -> KResult<core::cmp::Ordering> {
    let kernel = crate::cpu::casecmp_kernel();
    if ascii && n != 0 && !a.is_null() && !b.is_null() && kernel != crate::cpu::CASECMP_MEASURED {
        // UNSAFE(slice): the kernel call; its obligations are checked just above.
        // SAFETY: the function's contract; `ascii`, `n != 0`, both non-NULL and a kernel chosen.
        return Ok(unsafe { casecmp_kernel_call(a, b, n, loc, kernel) }.cmp(&0));
    }
    // UNSAFE(slice): the measured path under the same contract.
    // SAFETY: the function's contract.
    unsafe { casecmp_measured(a, b, n, loc) }
}

/// [`casecmp_in`] as the `int` the exports return (-1, 0 or 1): the kernel's answer goes straight
/// back as a tail call (STRCASECMP-SHORT-2: no registers kept across it, no `KResult` between).
///
/// # Safety
/// As for [`casecmp_in`].
#[inline]
// UNSAFE(slice): the kernel reads C text under the function's contract.
#[allow(unsafe_code)]
pub unsafe fn casecmp_int(a: *const i8, b: *const i8, n: usize, ascii: bool, loc: &liella::locale::Locale) -> i32 {
    let kernel = crate::cpu::casecmp_kernel_decided();
    // The default kernel first (PERF-STRCASECMP-AVX2): the AVX2 test is one bit of the CPU
    // state, and the AVX2 kernel tests NULL itself (it sends NULL to the measured path), so this
    // branch tests only `ascii` (`n != 0` is a constant for `strcasecmp`).
    if kernel == crate::cpu::CASECMP_AVX2 && ascii && n != 0 {
        // UNSAFE(slice): the kernel call; its obligations are checked just above or by the kernel.
        // SAFETY: the function's contract; `ascii`, `n != 0` and AVX2 chosen; NULL is the kernel's.
        return unsafe { casecmp_kernel_call(a, b, n, loc, crate::cpu::CASECMP_AVX2) };
    }
    if kernel == crate::cpu::CASECMP_SSE2 && ascii && n != 0 && !a.is_null() && !b.is_null() {
        // UNSAFE(slice): the kernel call; its obligations are checked just above.
        // SAFETY: the function's contract; `ascii`, `n != 0`, both non-NULL and SSE2 chosen.
        return unsafe { casecmp_kernel_call(a, b, n, loc, crate::cpu::CASECMP_SSE2) };
    }
    // UNSAFE(slice): the same call out of line.
    // SAFETY: the function's contract.
    unsafe { casecmp_int_slow(a, b, n, ascii, loc) }
}

/// [`casecmp_int`] after its kernel path declined: the CPU flags decided here if they were not,
/// then the kernel or the measured path. Out of line, so the export's kernel path is a tail call
/// with no frame.
///
/// # Safety
/// As for [`casecmp_in`].
#[inline(never)]
// UNSAFE(slice): the kernel reads C text under the function's contract.
#[allow(unsafe_code)]
unsafe fn casecmp_int_slow(a: *const i8, b: *const i8, n: usize, ascii: bool, loc: &liella::locale::Locale) -> i32 {
    let kernel = crate::cpu::casecmp_kernel();
    if ascii && n != 0 && !a.is_null() && !b.is_null() && kernel != crate::cpu::CASECMP_MEASURED {
        // UNSAFE(slice): the kernel call; its obligations are checked just above.
        // SAFETY: the function's contract; `ascii`, `n != 0`, both non-NULL and a kernel chosen.
        return unsafe { casecmp_kernel_call(a, b, n, loc, kernel) };
    }
    // UNSAFE(slice): the measured path under the same contract.
    // SAFETY: the function's contract.
    crate::ret::order(unsafe { casecmp_measured(a, b, n, loc) })
}

/// The ordering kernel `kernel` (`liella_machine_casecmp_ord_*`) on `a` and `b`: -1, 0 or 1
/// (D-4.27). When the bytes that decide hold one of 128 or more (its fold is `loc`'s), the kernel
/// itself tail-jumps to [`casecmp_rest`] with `loc`, so every answer comes back from this call.
///
/// # Safety
/// `a` and `b` are non-NULL and point to readable bytes up to a NUL or `n` bytes (the C contract
/// of the export calling this), not written for the call except by another thread (D-4.21);
/// `n != 0`; `loc`'s case map below 128 is A-Z to a-z (`ctype::ascii_case`); `kernel` is
/// `CASECMP_AVX2` (only where `cpu::casecmp_kernel` chose it) or `CASECMP_SSE2`.
#[inline(always)]
// UNSAFE(slice): the kernels read C text up to the first folded difference.
#[allow(unsafe_code)]
unsafe fn casecmp_kernel_call(a: *const i8, b: *const i8, n: usize, loc: &liella::locale::Locale, kernel: u8) -> i32 {
    let (pa, pb) = (a.cast::<u8>(), b.cast::<u8>());
    let pl = core::ptr::from_ref(loc).cast::<core::ffi::c_void>();
    // Self-tests count the kernel's decisions: a call the rest entry did not measure.
    #[cfg(feature = "selftest")]
    let measured = CASECMP_MEASURED.load(core::sync::atomic::Ordering::Relaxed);
    // The kernels' contract (`ffi`): the function's contract gives it; they read only inside pages
    // that hold an unended byte below `n`, and they order the deciding bytes themselves.
    let r = if kernel == crate::cpu::CASECMP_AVX2 {
        ffi::liella_machine_casecmp_ord_avx2(pa, pb, n, pl)
    } else {
        ffi::liella_machine_casecmp_ord_sse2(pa, pb, n, pl)
    };
    #[cfg(feature = "selftest")]
    if CASECMP_MEASURED.load(core::sync::atomic::Ordering::Relaxed) == measured {
        note_casecmp(true);
    }
    r
}

/// The ordering kernels' way out (`liella_hot_casecmp_rest`, STRCASECMP-SHORT-2): `code` 2 = the
/// bytes that decide hold one of 128 or more, whose fold `loc` decides, or the kernel's
/// re-read at the stop found the text changed: the measured path; 3 = an index above `n` (never
/// sent by the current kernels; kept as an abort, D-4.21).
///
/// # Safety
/// As for [`casecmp_kernel_call`], with `loc` the pointer it passed (a `&Locale` live for the call).
// UNSAFE(slice): the measured path reads C text under the caller's contract.
#[allow(unsafe_code)]
pub unsafe fn casecmp_rest(a: *const i8, b: *const i8, n: usize, loc: *const core::ffi::c_void, code: i32) -> i32 {
    if code == 3 {
        scanner_fault();
    }
    // UNSAFE(slice): the locale the kernel passed through unchanged.
    // SAFETY: `loc` came from a `&Locale` the caller holds for the call (the function's contract).
    let loc = unsafe { &*loc.cast::<liella::locale::Locale>() };
    // UNSAFE(slice): the measured path under the same contract.
    // SAFETY: the function's contract.
    crate::ret::order(unsafe { casecmp_measured(a, b, n, loc) })
}

/// [`casecmp`] without the kernel: both strings measured, then `ctype::compare_folded` (the path
/// before CASECMP-K1). Out of line, so the kernel path keeps a small frame.
///
/// # Safety
/// As for [`casecmp`].
#[inline(never)]
// UNSAFE(slice): measuring C text under the caller's contract.
#[allow(unsafe_code)]
unsafe fn casecmp_measured(a: *const i8, b: *const i8, n: usize, loc: &liella::locale::Locale) -> KResult<core::cmp::Ordering> {
    note_casecmp(false);
    // UNSAFE(slice): measuring `a` under the function's contract.
    // SAFETY: the function's contract.
    let x = unsafe { strz_n(a, n) }?;
    // UNSAFE(slice): measuring `b` under the function's contract.
    // SAFETY: as for `x`.
    let y = unsafe { strz_n(b, n) }?;
    Ok(liella::ctype::compare_folded(x, y, loc))
}

/// Units before the terminator at `p`.
///
/// # Safety
/// `p` is non-NULL and points to readable units up to and including a zero unit.
#[allow(clippy::arithmetic_side_effects)] // a C string cannot exceed the address space
// UNSAFE(slice): measuring C text needs raw reads.
#[allow(unsafe_code)]
unsafe fn scan<T: Copy + PartialEq + Default>(p: *const T, limit: usize) -> usize {
    let zero = T::default();
    let mut n = 0usize;
    while n < limit {
        // UNSAFE(slice): reading the C string one unit at a time is the only way to find its
        // end without reading past it.
        #[allow(unsafe_code)]
        // SAFETY: units 0..=n lie within the string (no terminator was seen before n), per the
        // function's contract.
        let unit = unsafe { p.wrapping_add(n).read() };
        if unit == zero {
            break;
        }
        n += 1;
    }
    n
}

/// The value of an environment variable at `addr` (an address `process::getenv` returned), for
/// use before the environment next changes. Crate-private contract: `addr` is such an address.
pub(crate) fn env_value(addr: usize) -> &'static [u8] {
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: a NUL-terminated environment string that stays put while it is used.
    unsafe {
        strz(core::ptr::with_exposed_provenance::<i8>(addr)).unwrap_or(&[])
    }
}

/// The abbreviation a `struct tm`'s `tm_zone` points to (spec §23.5), `None` for NULL, for the
/// duration of the call that received the structure. Crate-private contract: `addr` is the
/// `tm_zone` of a `struct tm` the C caller passed, which is NULL or points to a string (the
/// library's own results always do; a caller's are the caller's contract, as for any pointer
/// in a structure it passes).
pub(crate) fn tm_zone<'a>(addr: usize) -> Option<&'a [u8]> {
    // UNSAFE(slice): the contract above.
    #[allow(unsafe_code)]
    // SAFETY: NULL (handled by `strz`) or a NUL-terminated string that the call does not outlive.
    unsafe {
        strz(core::ptr::with_exposed_provenance::<i8>(addr)).ok()
    }
}

/// The text at `p` (without its terminator).
///
/// # Safety
/// `p` is NULL or points to a readable NUL-terminated string not written for the returned
/// lifetime (the C contract of the export calling this).
// UNSAFE(slice): C text becomes a Rust slice only through raw reads and `from_raw_parts`.
#[allow(unsafe_code)]
pub unsafe fn strz<'a>(p: *const i8) -> KResult<&'a [u8]> {
    // SAFETY: the caller's contract (above); `text` handles NULL.
    unsafe { text(p.cast(), usize::MAX) }
}

/// # Safety
/// `p` is NULL or points to readable bytes up to a NUL or `limit` bytes, not written for the
/// returned lifetime.
// UNSAFE(slice): the shared implementation of the byte readers.
#[allow(unsafe_code)]
unsafe fn text<'a>(p: *const u8, limit: usize) -> KResult<&'a [u8]> {
    if p.is_null() {
        return Err(Errno::EFAULT);
    }
    // UNSAFE(slice): measuring C text under the caller's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    let n = unsafe { byte_len(p, limit) };
    // UNSAFE(slice): the measured text becomes a slice.
    #[allow(unsafe_code)]
    // SAFETY: `n` readable bytes at `p`, not written for the returned lifetime.
    unsafe {
        Ok(core::slice::from_raw_parts(p, n))
    }
}

/// A Path argument (spec §0.4): the C text at `p` with its terminator, as the kernel takes it.
///
/// The text is not read here: the result holds the address, and measures the text only when a
/// caller asks for its bytes. A call that just hands the path to the kernel therefore gives an
/// unreadable pointer to the kernel, which answers `EFAULT` as it does for the reference
/// library; measuring first would fault in user space instead.
///
/// # Safety
/// `p` is NULL or a readable NUL-terminated string, not written for the returned lifetime
/// (or, for a call that only forwards it, any pointer: the kernel validates it).
// UNSAFE(slice): a C path becomes a KStr whose bytes are read only on demand.
#[allow(unsafe_code)]
pub unsafe fn path<'a>(p: *const i8) -> KResult<liella_kernel::path::KStr<'a>> {
    if p.is_null() {
        return Err(Errno::EFAULT);
    }
    Ok(liella_kernel::path::KStr::from_user(p.cast(), path_bytes))
}

/// The measuring function of [`path`]'s result: the text at `p` and its terminator.
///
/// Reached only through a `KStr` that `path` built from the caller's pointer, so `p` is what the
/// export's C contract says: a readable NUL-terminated string, not written for `'a`.
// UNSAFE(slice): the measured path and its terminator become a slice.
#[allow(unsafe_code)]
fn path_bytes<'a>(p: *const u8) -> &'a [u8] {
    // UNSAFE(slice): measuring the path under the caller's contract.
    #[allow(unsafe_code)]
    // SAFETY: the contract above.
    let n = unsafe { byte_len(p, usize::MAX) };
    // UNSAFE(slice): the measured path and its terminator become a slice.
    #[allow(unsafe_code)]
    // SAFETY: `n + 1` readable bytes at `p`: the text and the NUL `byte_len` checked.
    unsafe {
        core::slice::from_raw_parts(p, n.saturating_add(1))
    }
}

/// An optional Path argument: `None` for NULL (`fanotify_mark`, `utimensat`).
///
/// # Safety
/// As for [`path`].
// UNSAFE(slice): as for `path`.
#[allow(unsafe_code)]
pub unsafe fn opt_path<'a>(p: *const i8) -> KResult<Option<liella_kernel::path::KStr<'a>>> {
    if p.is_null() {
        return Ok(None);
    }
    // UNSAFE(slice): as for `path`.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    unsafe { path(p).map(Some) }
}

/// At most `n` bytes of the text at `p` (`strn*`): reads no further than `n` bytes. With `n`
/// 0 nothing is read, so any `p`, NULL included, is the empty text (the reference's answer for
/// `strnlen`, `strncmp`, `stpncpy`, ... with NULL and 0, which gnulib's tests require; S6 is
/// about pointers the call reads through; validation T18, as T11 for `strndup`).
///
/// # Safety
/// `p` is NULL or points to `min(n, strlen+1)` readable bytes.
// UNSAFE(slice): as for `strz`, bounded.
#[allow(unsafe_code)]
pub unsafe fn strz_n<'a>(p: *const i8, n: usize) -> KResult<&'a [u8]> {
    if n == 0 {
        return Ok(&[]);
    }
    // UNSAFE(slice): at most `n` bytes of C text under the caller's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract (above).
    unsafe { text(p.cast(), n) }
}

/// The text at `p` as a mutable view (the destructive tokenizers, XPG `basename`).
///
/// # Safety
/// As for [`strz`], and the bytes are writable and not otherwise referenced.
// UNSAFE(slice): as for `strz`, writable.
#[allow(unsafe_code)]
pub unsafe fn strz_mut<'a>(p: *mut i8) -> KResult<&'a mut [u8]> {
    if p.is_null() {
        return Err(Errno::EFAULT);
    }
    // UNSAFE(slice): measuring or viewing C text under the caller's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    let n = unsafe { byte_len(p.cast_const().cast::<u8>(), usize::MAX) };
    // UNSAFE(slice): the measured text becomes a mutable slice.
    #[allow(unsafe_code)]
    // SAFETY: `n` readable and (per contract) writable bytes at `p`.
    unsafe {
        Ok(core::slice::from_raw_parts_mut(p.cast::<u8>(), n))
    }
}

/// The text at `p` with its terminator as a mutable view (the temporary-name templates, which
/// the kernel then reads as a path).
///
/// # Safety
/// As for [`strz_mut`].
// UNSAFE(slice): as for `strz_mut`, one byte longer.
#[allow(unsafe_code)]
pub unsafe fn strz_mut_z<'a>(p: *mut i8) -> KResult<&'a mut [u8]> {
    if p.is_null() {
        return Err(Errno::EFAULT);
    }
    // UNSAFE(slice): measuring C text under the caller's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    let n = unsafe { byte_len(p.cast_const().cast::<u8>(), usize::MAX) };
    // UNSAFE(slice): the measured text and its terminator become a mutable slice.
    #[allow(unsafe_code)]
    // SAFETY: `n + 1` readable and (per contract) writable bytes at `p`.
    unsafe {
        Ok(core::slice::from_raw_parts_mut(p.cast::<u8>(), n.saturating_add(1)))
    }
}

/// The address of the `d_name` of the `struct dirent` that `*pp` points to (`alphasort`,
/// `versionsort`); `EFAULT` for a NULL `pp` or `*pp`.
///
/// # Safety
/// `pp` is NULL or points to a readable pointer variable.
// UNSAFE(slice): reading the C pointer variable the caller passed the address of.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn dirent_name_at(pp: *const *const core::ffi::c_void) -> KResult<*const i8> {
    if pp.is_null() {
        return Err(Errno::EFAULT);
    }
    // UNSAFE(slice): reading the pointer the caller passed the address of.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract: `pp` is a readable pointer variable.
    let d = unsafe { pp.read() };
    if d.is_null() {
        return Err(Errno::EFAULT);
    }
    // The entry's name, 19 bytes into the record.
    Ok(d.cast::<i8>().wrapping_add(19))
}

/// The `d_name` of the `struct dirent` that `*pp` points to (`versionsort`).
///
/// # Safety
/// `pp` is NULL or points to a readable pointer to a `struct dirent` whose `d_name` is a
/// NUL-terminated string, not written for the returned lifetime.
// UNSAFE(slice): reading the C pointer variable, then the name it leads to.
#[allow(unsafe_code)]
pub unsafe fn dirent_name<'a>(pp: *const *const core::ffi::c_void) -> KResult<&'a [u8]> {
    // SAFETY: the caller's contract: `pp` is NULL or a readable pointer variable.
    let p = unsafe { dirent_name_at(pp) }?;
    // SAFETY: the caller's contract: `p` is the entry's NUL-terminated `d_name`.
    unsafe { strz(p) }
}

/// `alphasort(a, b)` (§13.3): the entries' `d_name`s compared as `strcoll` compares them in
/// locale `l`, through [`coll`]'s fast paths (one shared-prefix scan, no length scans:
/// SCANDIR-ALPHASORT-DEEP). A NULL `a`, `b`, `*a` or `*b` aborts as the other compare callbacks do.
///
/// # Safety
/// `a` and `b` are NULL or point to readable pointers to `struct dirent`s whose `d_name`s are
/// NUL-terminated strings.
// UNSAFE(slice): reading the C pointer variables; the names are read by `coll`.
#[allow(unsafe_code)]
pub unsafe fn dirent_coll(a: *const *const core::ffi::c_void, b: *const *const core::ffi::c_void, l: &liella::locale::Locale) -> i32 {
    // SAFETY: the caller's contract: `a` is NULL or a readable pointer variable.
    let x = unsafe { dirent_name_at(a) };
    // SAFETY: the caller's contract: `b` is NULL or a readable pointer variable.
    let y = unsafe { dirent_name_at(b) };
    let (x, y) = match (x, y) {
        (Ok(x), Ok(y)) => (x, y),
        (Err(e), _) | (_, Err(e)) => return crate::ret::order(Err(e)),
    };
    // UNSAFE(slice): comparing the two entries' names as C text.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract: both are the entries' NUL-terminated names.
    unsafe {
        coll(x, y, l)
    }
}

/// The wide text at `p` (`wchar_t` units), at most `n` units.
///
/// # Safety
/// `p` is NULL or points to readable units up to a zero unit or `n` units.
// UNSAFE(slice): wide C text into a Rust slice.
#[allow(unsafe_code)]
pub unsafe fn wcsz_n<'a>(p: *const i32, n: usize) -> KResult<&'a [i32]> {
    // As for `strz_n`: 0 units read need no pointer.
    if n == 0 {
        return Ok(&[]);
    }
    // UNSAFE(slice): at most `n` units of wide C text under the caller's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    unsafe { wide::<i32>(p, n) }
}

/// The wide text at `p`.
///
/// # Safety
/// As for [`wcsz_n`] without a bound.
// UNSAFE(slice): wide C text into a Rust slice.
#[allow(unsafe_code)]
pub unsafe fn wcsz<'a>(p: *const i32) -> KResult<&'a [i32]> {
    // SAFETY: the caller's contract.
    unsafe { wide::<i32>(p, usize::MAX) }
}

/// `strstr`'s search (STRSTR-VEC): the first index where `needle` occurs in the string at `h`
/// (an empty needle at 0), the same answer as `bytes::find_seq` over the measured string, found
/// without measuring the haystack first: the C layer's fused scanner
/// (`liella_machine_strpair`) names the first candidate (two needle bytes in place: the first
/// and last, after the first false candidate the two rarest) or the terminator, whichever comes
/// first, and the needle is compared at each candidate. Work spent on failed candidates is
/// counted; past 4 x progress + 256 units the rest is measured and searched by
/// `bytes::find_seq` (two-way), so the worst case stays linear.
///
/// # Safety
/// `h` is NULL or a readable NUL-terminated string, not written during the call.
// UNSAFE(slice): the haystack is read through its terminator by the fused scanner and the compares.
#[allow(unsafe_code)]
pub unsafe fn find_str(h: *const i8, needle: &[u8]) -> KResult<Option<usize>> {
    // UNSAFE(slice): the shared search over the caller's string.
    // SAFETY: the caller's contract; `strpair` is the byte instance of the fused scanner.
    unsafe { find_z(h.cast::<u8>(), needle, ffi::liella_machine_strpair, crate::conv::commonness) }
}

/// Needle bytes that [`find_str_n`] measures in its fused loop before it hands the rest to the
/// length scanner.
const SHORT_NEEDLE: usize = 16;

/// `strstr` from both strings (STRSTR-SHORT-SETUP): [`find_str`] with the needle measured here.
/// The first [`SHORT_NEEDLE`] needle bytes are measured in one loop with the haystack prefix
/// that `find_z` needs as text (units [0, m - 1)), so a short needle costs no length-scanner
/// call and no second pass over the prefix (a short call paid 11 + 20 instructions for them,
/// of 209). A longer needle has the rest measured by the length scanner and the rest of the
/// prefix checked after it. A haystack shorter than the needle stops the needle's reads early.
///
/// # Safety
/// `h` and `n` are NULL or readable NUL-terminated strings, not written during the call.
// UNSAFE(slice): both strings are read unit by unit, never past a terminator, then as for `find_str`.
#[allow(unsafe_code)]
pub unsafe fn find_str_n(h: *const i8, n: *const i8) -> KResult<Option<usize>> {
    if h.is_null() || n.is_null() {
        return Err(Errno::EFAULT);
    }
    let (p, q) = (h.cast::<u8>(), n.cast::<u8>());
    let mut i = 0usize;
    let m = loop {
        if i == SHORT_NEEDLE {
            // UNSAFE(slice): measuring the needle from byte i, after i text bytes.
            // SAFETY: bytes [0, i) of the needle were not NUL, so byte i lies in the string.
            let rest = unsafe { strz(q.wrapping_add(i).cast()) }?.len();
            let m = i.saturating_add(rest);
            let (done, left) = (i.saturating_sub(1), m.saturating_sub(i));
            // UNSAFE(slice): the rest of the haystack prefix, from byte i - 1.
            // SAFETY: haystack bytes [0, i - 1) were not NUL, so byte i - 1 lies in the string.
            if unsafe { scan(p.wrapping_add(done), left) } < left {
                return Ok(None);
            }
            break m;
        }
        // UNSAFE(slice): needle byte i, read after bytes [0, i) were not NUL.
        // SAFETY: the caller's contract (the loop stops at the terminator).
        if unsafe { q.wrapping_add(i).read() } == 0 {
            break i;
        }
        // UNSAFE(slice): haystack byte i - 1, read after bytes [0, i - 1) were not NUL.
        // SAFETY: the caller's contract (the loop stops at the terminator).
        if i > 0 && unsafe { p.wrapping_add(i.wrapping_sub(1)).read() } == 0 {
            // The needle has more than i bytes, the haystack i - 1.
            return Ok(None);
        }
        i = i.wrapping_add(1);
    };
    if m == 0 {
        return Ok(Some(0));
    }
    // UNSAFE(slice): the measured needle becomes a slice.
    // SAFETY: `m` readable non-NUL bytes at `q`, not written during the call (contract).
    let needle = unsafe { core::slice::from_raw_parts(q, m) };
    crate::cpu::ready();
    // UNSAFE(slice): the search after the prefix check.
    // SAFETY: haystack bytes [0, m - 1) are text (checked above); `strpair` keeps the contract.
    unsafe { find_known(p, needle, ffi::liella_machine_strpair, crate::conv::commonness) }
}

/// `wcsstr`'s search (STRSTR-VEC): [`find_str`] over 4-byte units (`liella_machine_wcspair`; a
/// unit above 255 ranks as rare). A haystack address that is not 4-byte aligned is measured
/// and searched by `bytes::find_seq` instead.
///
/// # Safety
/// `h` is NULL or a readable wide string ending in a zero unit, not written during the call.
// UNSAFE(slice): as for `find_str`, in units.
#[allow(unsafe_code)]
pub unsafe fn find_wcs(h: *const i32, needle: &[i32]) -> KResult<Option<usize>> {
    if h.addr() % 4 != 0 {
        // UNSAFE(slice): measuring the wide string under the caller's contract.
        // SAFETY: the caller's contract.
        let t = unsafe { wcsz(h) }?;
        return Ok(liella::bytes::find_seq(t, needle, core::convert::identity));
    }
    // UNSAFE(slice): the shared search over the caller's aligned wide string.
    // SAFETY: the caller's contract; `h` is 4-byte aligned (checked above).
    unsafe { find_z(h, needle, ffi::liella_machine_wcspair, crate::conv::unit_commonness) }
}

/// The search of [`find_str`] and [`find_wcs`] over units `T`, with the fused scanner `kernel`
/// and the rarity rank `rank`. Loop invariant: the units before `from + o1` are text (none is
/// the terminator), and unit `from + o1` is at or before the terminator, which is the
/// scanner's proof obligation for the start `p + from + o1` and the distance `o1 - o0`.
///
/// # Safety
/// As for [`find_str`] in units `T`; `kernel` keeps `liella_machine_strpair`'s contract for `T`.
// UNSAFE(slice): reading the haystack through its terminator via the scanner's answers.
#[allow(unsafe_code)]
unsafe fn find_z<T: Copy + PartialEq + Default + Into<i32> + liella::bytes::Unit>(
    p: *const T,
    needle: &[T],
    kernel: extern "C" fn(*const T, usize, i32, i32) -> usize,
    rank: fn(T) -> u8,
) -> KResult<Option<usize>> {
    if p.is_null() {
        return Err(Errno::EFAULT);
    }
    let m = needle.len();
    let Some(last) = m.checked_sub(1) else { return Ok(Some(0)) };
    // UNSAFE(slice): the first `last` units, each read only after the one before was text.
    // SAFETY: the caller's contract (the scan stops at the terminator).
    if unsafe { scan(p, last) } < last {
        return Ok(None);
    }
    crate::cpu::ready();
    // UNSAFE(slice): the search after the prefix check.
    // SAFETY: units [0, last) at `p` are text (checked above); the caller's contract for `kernel`.
    unsafe { find_known(p, needle, kernel, rank) }
}

/// [`find_z`] after its checks: `needle` is not empty, units [0, needle.len() - 1) at `p` are
/// text, and the scan paths are decided (`cpu::ready`).
///
/// # Safety
/// As for [`find_z`], with those units known to be text.
// UNSAFE(slice): reading the haystack through its terminator via the scanner's answers.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn find_known<T: Copy + PartialEq + Default + Into<i32> + liella::bytes::Unit>(
    p: *const T,
    needle: &[T],
    kernel: extern "C" fn(*const T, usize, i32, i32) -> usize,
    rank: fn(T) -> u8,
) -> KResult<Option<usize>> {
    let m = needle.len();
    let last = m.saturating_sub(1);
    let (mut o0, mut o1) = (0usize, last);
    let mut tuned = m < 2;
    let (mut from, mut wasted) = (0usize, 0usize);
    loop {
        let (Some(&c0), Some(&c1)) = (needle.get(o0), needle.get(o1)) else { return Ok(None) };
        let q0 = from.saturating_add(o1);
        let r = kernel(p.wrapping_add(q0), o1.saturating_sub(o0), c0.into(), c1.into());
        let k = r >> 1;
        if r & 1 == 1 {
            // UNSAFE(slice): the one unit the scanner reported as the terminator.
            // SAFETY: the string is readable through its terminator, reported at q0 + k.
            if unsafe { p.wrapping_add(q0.saturating_add(k)).read() } == T::default() {
                return Ok(None);
            }
            // A writer changed that unit: this pass's own reads decide the rest (D-4.21).
            // UNSAFE(slice): measuring the rest of the string from `from`.
            // SAFETY: unit `from` is at or before the terminator (loop invariant).
            return unsafe { find_rest(p, from, needle) };
        }
        let at = from.saturating_add(k);
        // UNSAFE(slice): comparing the needle at the candidate.
        // SAFETY: units [at, at + o1] are text: the scanner saw no terminator in [q0, at + o1],
        // and the units before q0 are text (loop invariant).
        if unsafe { matches_at(p, at, o1, needle) } {
            return Ok(Some(at));
        }
        from = at.saturating_add(1);
        wasted = wasted.saturating_add(m);
        if wasted > from.saturating_mul(4).saturating_add(256) {
            // UNSAFE(slice): measuring the rest of the string from `from`.
            // SAFETY: unit `from` = at + 1 <= at + o1 + 1, at or before the terminator.
            return unsafe { find_rest(p, from, needle) };
        }
        if !tuned {
            // o1 stays at most `last`, so the invariant holds for the new pair.
            tuned = true;
            (o0, o1) = crate::conv::rare_pair_by(needle, rank);
        }
    }
}

/// Whether `needle` is at unit `at` of the string at `p`, of which units [at, at + known] are
/// known text: those are compared as a slice, the rest one unit at a time, each read only after
/// the one before matched a needle unit (none is zero, so it was not the terminator).
///
/// # Safety
/// Units [at, at + known] at `p` are readable text, `known < needle.len()`, needle units are
/// non-zero.
// UNSAFE(slice): a slice of the known units, then reads that never pass the terminator.
#[allow(unsafe_code)]
unsafe fn matches_at<T: Copy + PartialEq>(p: *const T, at: usize, known: usize, needle: &[T]) -> bool {
    let Some((head, tail)) = needle.split_at_checked(known.saturating_add(1)) else { return false };
    let s = p.wrapping_add(at);
    // UNSAFE(slice): the known text units become a slice.
    // SAFETY: the function's contract (`head.len()` = known + 1 units at `s`).
    let bytes = core::mem::size_of_val(head);
    // UNSAFE(slice): the known text units compared byte for byte.
    // SAFETY: the function's contract (`bytes` readable bytes at `s` and at `head`).
    if !unsafe { eq_bytes(s.cast::<u8>(), head.as_ptr().cast::<u8>(), bytes) } {
        return false;
    }
    let s = s.wrapping_add(head.len());
    for (j, &u) in tail.iter().enumerate() {
        // UNSAFE(slice): one unit past the units already matched.
        // SAFETY: every unit before it matched a non-zero needle unit, so it lies in the string.
        if unsafe { s.wrapping_add(j).read() } != u {
            return false;
        }
    }
    true
}

/// Whether the `n` bytes at `a` and `b` are equal (STRSTR-SHORT-SETUP): up to 16 bytes by two
/// overlapping word loads from each side (the first and the last word, both inside the `n`
/// bytes), longer as slices. A short candidate compare called `__memcmpeq` (22 instructions plus
/// the call, of a 209-instruction short `strstr`).
///
/// # Safety
/// `n` readable bytes at `a` and at `b`.
// UNSAFE(slice): unaligned word loads inside the two ranges.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn eq_bytes(a: *const u8, b: *const u8, n: usize) -> bool {
    if n > 16 {
        // UNSAFE(slice): the first range as a slice.
        // SAFETY: the function's contract.
        let x = unsafe { core::slice::from_raw_parts(a, n) };
        // UNSAFE(slice): the second range as a slice.
        // SAFETY: the function's contract.
        let y = unsafe { core::slice::from_raw_parts(b, n) };
        x == y
    } else if n >= 8 {
        // UNSAFE(slice): first and last 8-byte words of the ranges.
        // SAFETY: the function's contract, n >= 8.
        unsafe { ends_eq::<u64>(a, b, n) }
    } else if n >= 4 {
        // UNSAFE(slice): first and last 4-byte words of the ranges.
        // SAFETY: the function's contract, n >= 4.
        unsafe { ends_eq::<u32>(a, b, n) }
    } else if n >= 2 {
        // UNSAFE(slice): first and last 2-byte words of the ranges.
        // SAFETY: the function's contract, n >= 2.
        unsafe { ends_eq::<u16>(a, b, n) }
    } else {
        // UNSAFE(slice): the one byte of each range.
        // SAFETY: the function's contract, n = 1 when read.
        n == 0 || unsafe { word_diff::<u8>(a, b, 0) } == 0
    }
}

/// Whether the first and the last `W`-sized words of the `n` bytes at `a` and `b` are equal (all
/// `n` bytes when `n` <= 2 x the word size).
///
/// # Safety
/// `n` readable bytes at `a` and at `b`, `n` >= the size of `W`.
// UNSAFE(slice): two word loads per range, inside it.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn ends_eq<W: Copy + Into<u64>>(a: *const u8, b: *const u8, n: usize) -> bool {
    let t = n.wrapping_sub(core::mem::size_of::<W>());
    // UNSAFE(slice): the first word of each range.
    // SAFETY: the function's contract (n >= the word size).
    let d0 = unsafe { word_diff::<W>(a, b, 0) };
    // UNSAFE(slice): the last word of each range, at n - size.
    // SAFETY: the function's contract (n >= the word size, so t + size = n).
    let d1 = unsafe { word_diff::<W>(a, b, t) };
    d0 | d1 == 0
}

/// The bits that differ between the `W`-sized words at byte `off` of `a` and of `b`.
///
/// # Safety
/// `off` + the size of `W` readable bytes at `a` and at `b`.
// UNSAFE(slice): one unaligned load from each range.
#[allow(unsafe_code)]
#[inline(always)]
unsafe fn word_diff<W: Copy + Into<u64>>(a: *const u8, b: *const u8, off: usize) -> u64 {
    // UNSAFE(slice): one word of the first range.
    // SAFETY: the function's contract.
    let x: u64 = unsafe { a.wrapping_add(off).cast::<W>().read_unaligned() }.into();
    // UNSAFE(slice): the same word of the second range.
    // SAFETY: the function's contract.
    let y: u64 = unsafe { b.wrapping_add(off).cast::<W>().read_unaligned() }.into();
    x ^ y
}

/// The needle's first occurrence at or after unit `from`, by measuring the rest and the two-way
/// search (`find_z`'s linear bound and writer fallback).
///
/// # Safety
/// Unit `from` at `p` is at or before the terminator of a readable string.
// UNSAFE(slice): measuring the rest of the caller's string.
#[allow(unsafe_code)]
unsafe fn find_rest<T: Copy + PartialEq + Default + liella::bytes::Unit>(p: *const T, from: usize, needle: &[T]) -> KResult<Option<usize>> {
    // UNSAFE(slice): the rest of the string becomes a slice.
    // SAFETY: the function's contract.
    let t = unsafe { wide::<T>(p.wrapping_add(from), usize::MAX) }?;
    Ok(liella::bytes::find_seq(t, needle, core::convert::identity).map(|k| from.saturating_add(k)))
}

/// The wide text at `p` as a mutable view (`wcstok`).
///
/// # Safety
/// As for [`wcsz`], and writable, not otherwise referenced.
// UNSAFE(slice): wide C text into a mutable slice.
#[allow(unsafe_code)]
pub unsafe fn wcsz_mut<'a>(p: *mut i32) -> KResult<&'a mut [i32]> {
    if p.is_null() {
        return Err(Errno::EFAULT);
    }
    // UNSAFE(slice): measuring or viewing C text under the caller's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    let n = unsafe { scan(p.cast_const(), usize::MAX) };
    // UNSAFE(slice): the measured text becomes a mutable slice.
    #[allow(unsafe_code)]
    // SAFETY: `n` readable, writable units at `p`.
    unsafe {
        Ok(core::slice::from_raw_parts_mut(p, n))
    }
}

/// # Safety
/// `p` is NULL or points to readable units up to a zero unit or `limit` units.
// UNSAFE(slice): the shared implementation of the readers above.
#[allow(unsafe_code)]
unsafe fn wide<'a, T: Copy + PartialEq + Default>(p: *const T, limit: usize) -> KResult<&'a [T]> {
    if p.is_null() {
        return Err(Errno::EFAULT);
    }
    // UNSAFE(slice): measuring or viewing C text under the caller's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    let n = unsafe { scan(p, limit) };
    // UNSAFE(slice): the measured text becomes a slice.
    #[allow(unsafe_code)]
    // SAFETY: `n` readable units at `p`, not written for the returned lifetime.
    unsafe {
        Ok(core::slice::from_raw_parts(p, n))
    }
}

/// The text at `p`, `None` for NULL (`envz_add`'s value).
///
/// # Safety
/// As for [`strz`].
// UNSAFE(slice): as for `strz`.
#[allow(unsafe_code)]
pub unsafe fn strz_opt<'a>(p: *const i8) -> KResult<Option<&'a [u8]>> {
    if p.is_null() {
        return Ok(None);
    }
    // UNSAFE(slice): measuring or viewing C text under the caller's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    unsafe { strz(p).map(Some) }
}

/// The writable text at `p`, empty for NULL (XPG `basename`, `dirname`: NULL means `"."`).
///
/// # Safety
/// As for [`strz_mut`].
// UNSAFE(slice): as for `strz_mut`.
#[allow(unsafe_code)]
pub unsafe fn strz_mut_or_empty<'a>(p: *mut i8) -> KResult<&'a mut [u8]> {
    if p.is_null() {
        return Ok(&mut []);
    }
    // UNSAFE(slice): measuring or viewing C text under the caller's contract.
    #[allow(unsafe_code)]
    // SAFETY: the caller's contract.
    unsafe { strz_mut(p) }
}

/// Check the answer `n` the `rawmemchr` scanner gave for `p` and `c` (D-4.21): a byte at `n`
/// that is no longer `c` was changed by a writer during the call, so the bytes after it are
/// rescanned once, unit by unit, for `c`. There is no bound, so no index is impossible.
///
/// # Safety
/// As for `until`; `n` is what the scanner reported for `p` and `c`.
#[inline]
// UNSAFE(slice): the postcondition check reads the one byte the scanner reported.
#[allow(unsafe_code)]
pub(crate) unsafe fn settle_until(p: *const u8, c: u8, n: usize) -> usize {
    // SAFETY: the bytes are readable through the asserted occurrence (contract), which the
    // scanner reported at `n`.
    if unsafe { byte_at(p, n) } == c {
        return n;
    }
    // `n` indexes readable memory, so `n + 1` fits.
    let next = n.saturating_add(1);
    // UNSAFE(slice): rescanning C text after the reported index, whose byte a writer changed (D-4.21).
    // SAFETY: byte `n` is not `c`, so the asserted occurrence lies after it (contract).
    unsafe { rescan_until(p, next, c) }
}

/// The bytes at `p` up to (excluding) the first `c` (`rawmemchr`: the caller asserts that `c`
/// occurs).
///
/// # Safety
/// `p` points to readable bytes up to and including one equal to `c`.
// UNSAFE(slice): an unbounded scan for a byte the caller asserts is present.
#[allow(unsafe_code)]
pub unsafe fn until<'a>(p: *const u8, c: u8) -> KResult<&'a [u8]> {
    if p.is_null() {
        return Err(Errno::EFAULT);
    }
    crate::cpu::ready();
    let n = ffi::liella_machine_rawmemchr(p, i32::from(c)).addr().wrapping_sub(p.addr());
    // UNSAFE(slice): checking a scanner's answer: the byte at the reported index, then a rescan after it if a writer changed it (D-4.21).
    // SAFETY: the function's contract; `n` is the scanner's answer for `p` and `c`.
    let n = unsafe { settle_until(p, c, n) };
    // UNSAFE(slice): the measured bytes become a slice.
    #[allow(unsafe_code)]
    // SAFETY: `n` readable bytes at `p`.
    unsafe {
        Ok(core::slice::from_raw_parts(p, n))
    }
}

/// The text at `p`, read lazily one byte at a time and no further than `n` bytes (the
/// single-character multibyte forms, which need at most a few bytes of a possibly huge `n`:
/// AUD-M4). Byte `i` is read only once bytes `0..i` were read and none was the terminator, so
/// no read leaves the caller's `min(n, strlen+1)` bytes whatever order `byte` is called in.
pub struct LazyText {
    p: *const u8,
    n: usize,
    /// Bytes `0..seen` were read and are not the terminator.
    seen: core::cell::Cell<usize>,
}

/// The lazy text at `p` (`None` for NULL: the multibyte functions treat a NULL input
/// specially). Nothing is read here.
///
/// # Safety
/// As for [`strz_n`], for as long as the result is used.
// UNSAFE(slice): the view reads only under the caller's contract, checked per byte.
#[allow(unsafe_code)]
pub unsafe fn text_n_lazy(p: *const i8, n: usize) -> Option<LazyText> {
    (!p.is_null()).then_some(LazyText { p: p.cast(), n, seen: core::cell::Cell::new(0) })
}

impl liella_legacy::mb_compat::MbInput for LazyText {
    fn byte(&self, i: usize) -> Option<u8> {
        if i >= self.n || i > self.seen.get() {
            return None;
        }
        // UNSAFE(slice): one byte of C text inside the caller's contract.
        #[allow(unsafe_code)]
        // SAFETY: `i < n` and bytes `0..i` are not the terminator (`i <= seen`), so byte `i`
        // lies within `min(n, strlen+1)`, readable under `text_n_lazy`'s contract.
        let b = unsafe { self.p.wrapping_add(i).read() };
        if b != 0 && i == self.seen.get() {
            self.seen.set(i.saturating_add(1));
        }
        Some(b)
    }
}

/// The length check of `strlen`/`strnlen` (D-4.21) applied to the answer `n` for `text` (which
/// must hold a NUL; `None` otherwise) and `limit`, as if a scanner had reported `n`.
#[cfg(feature = "selftest")]
pub(crate) fn settle_len_in(text: &[u8], limit: usize, n: usize) -> Option<usize> {
    if !text.contains(&0) {
        return None;
    }
    // UNSAFE(slice): `text` holds a NUL, so it is a readable C string.
    #[allow(unsafe_code)]
    // SAFETY: `text` is readable through its first NUL.
    Some(unsafe { settle_len(text.as_ptr(), limit, n) })
}

/// The check of `strcmp` (D-4.21) applied to the answer `i` for `a` and `b` (each must hold a
/// NUL after `i`; `None` otherwise).
#[cfg(feature = "selftest")]
pub(crate) fn settle_compare_in(a: &[u8], b: &[u8], i: usize) -> Option<core::cmp::Ordering> {
    let ends = |s: &[u8]| s.get(i..).is_some_and(|t| t.contains(&0));
    if !ends(a) || !ends(b) {
        return None;
    }
    // UNSAFE(slice): both are readable C strings with byte `i` inside them.
    #[allow(unsafe_code)]
    // SAFETY: both slices hold a NUL at or after `i`.
    Some(unsafe { settle_compare(a.as_ptr(), b.as_ptr(), i) })
}

/// The check of `rawmemchr` (D-4.21) applied to the answer `n` for `text` and `c` (`text` must
/// hold `c` at or after `n`; `None` otherwise).
#[cfg(feature = "selftest")]
pub(crate) fn settle_until_in(text: &[u8], c: u8, n: usize) -> Option<usize> {
    if !text.get(n..).is_some_and(|t| t.contains(&c)) {
        return None;
    }
    // UNSAFE(slice): `text` is readable through an occurrence of `c` at or after `n`.
    #[allow(unsafe_code)]
    // SAFETY: as above.
    Some(unsafe { settle_until(text.as_ptr(), c, n) })
}

/// A NULL argument to a scanning export: abort as `ret::must` does (S6), out of line so the
/// export keeps no frame.
#[cold]
#[inline(never)]
fn null_fault() -> ! {
    crate::ret::fatal(Errno::EFAULT)
}

/// The S6 NULL abort (`EFAULT`) of the direct assembly entries' NULL cases (`liella_hot_strchr_null`,
/// `liella_hot_strncmp_null`; STR-CHR-CMP-SHORT).
pub fn null_abort() -> ! {
    null_fault()
}

// SMALL-STR (D-4.45): the scanners need no first-call CPU decision. Their level and short-head
// limit start at the SSE2 baseline (valid on every x86-64 CPU) and startup (`cpu::init_with`)
// switches them once, so the exports below call the kernels directly (was STR-K2's `is_ready`
// test and `*_cold` trampolines: two instructions per call).

// The exports below (owner ruling 2026-10-08, D-4.14 and D-4.21 as amended) return the
// scanner's answer without re-reading the byte at it in shipped builds: that answer is the index
// of a byte that WAS the terminator (or `c`) when the kernel read it, which is a correct answer
// for a call racing a writer, so nothing aborts. The selftest build (every `cargo xtask test`
// build of this crate) keeps the full D-4.21 check: a past-bound answer aborts, a changed byte
// is rescanned and counted in `RESCANS`, which the single-threaded sweeps require to stay 0.
//
// No CPU-path test either (SHORT-3, D-4.39): the scanners are correct on their SSE2 default
// (scan level 0) before `cpu::init_with` runs, and startup runs it (step 4) before any user code,
// so a per-call "decided yet?" test only bought speed for calls made before startup.

/// `strlen`: the scanner's answer, returned directly (a tail call in shipped builds; no slice
/// is formed, so nothing in Rust depends on the index).
///
/// # Safety
/// `p` is NULL (aborts, S6) or a readable NUL-terminated string.
#[inline]
// UNSAFE(slice): the selftest build checks the scanner's answer (D-4.21).
#[allow(unsafe_code)]
pub unsafe fn len(p: *const i8) -> usize {
    if p.is_null() {
        null_fault();
    }
    let n = ffi::liella_machine_strlen(p.cast());
    if cfg!(feature = "selftest") {
        // UNSAFE(slice): checking a scanner's answer: the byte at the reported index, then a rescan after it if a writer changed it (D-4.21).
        // SAFETY: the caller's contract; the string is readable through its terminator.
        unsafe { settle_len(p.cast(), usize::MAX, n) }
    } else {
        n
    }
}

/// `getenv` for the export: `name` is NULL (no entry) or a NUL-terminated string. The name is
/// measured once ([`len`]) and handed to the environment scanner as it is, with its length;
/// the answer is the scanner's (ENV-SCAN; the selftest build re-reads the entry, D-4.21).
///
/// # Safety
/// `name` is NULL or a readable NUL-terminated string.
#[inline]
// UNSAFE(slice): measuring the caller's string; the selftest build forms the slice.
#[allow(unsafe_code)]
pub unsafe fn getenv_c(name: *const i8) -> usize {
    if name.is_null() {
        return 0;
    }
    // UNSAFE(slice): measuring the caller's string.
    // SAFETY: the caller's contract.
    let n = unsafe { len(name) };
    if cfg!(feature = "selftest") {
        // UNSAFE(slice): the measured text as a slice (selftest build only).
        // SAFETY: `n` readable bytes at `name`, up to the terminator.
        return crate::process::getenv(unsafe { core::slice::from_raw_parts(name.cast::<u8>(), n) }).unwrap_or(0);
    }
    crate::process::getenv_raw(name.cast(), n)
}

/// `secure_getenv` for the export: [`getenv_c`] unless the process is secure.
///
/// # Safety
/// As [`getenv_c`].
#[inline]
// UNSAFE(slice): passes the caller's contract on.
#[allow(unsafe_code)]
pub unsafe fn secure_getenv_c(name: *const i8) -> usize {
    if crate::process::secure() {
        return 0;
    }
    // UNSAFE(slice): passes the caller's contract on.
    // SAFETY: the caller's contract, unchanged.
    unsafe { getenv_c(name) }
}

/// `strnlen`: as [`len`], at most `max`; `max == 0` reads nothing (NULL allowed, see [`strz_n`]).
///
/// # Safety
/// `p` is NULL or points to `min(max, strlen + 1)` readable bytes.
#[inline]
// UNSAFE(slice): the selftest build checks the scanner's answer (D-4.21).
#[allow(unsafe_code)]
pub unsafe fn len_n(p: *const i8, max: usize) -> usize {
    if max == 0 {
        return 0;
    }
    if p.is_null() {
        null_fault();
    }
    let n = ffi::liella_machine_strnlen(p.cast(), max);
    if cfg!(feature = "selftest") {
        // UNSAFE(slice): checking a scanner's answer: the byte at the reported index, then a rescan after it if a writer changed it (D-4.21).
        // SAFETY: the caller's contract; a reported index below `max` is a terminator inside it.
        unsafe { settle_len(p.cast(), max, n) }
    } else {
        n
    }
}

/// `rawmemchr`: a pointer to the first `c` (the caller asserts one occurs). Shipped builds
/// return the scanner's pointer unread, so the export tail-calls it (STR-K2, as `len`).
///
/// # Safety
/// `p` is NULL (aborts) or points to readable bytes up to and including one equal to `c`.
#[inline]
// UNSAFE(slice): the selftest build checks the scanner's answer (D-4.21).
#[allow(unsafe_code)]
pub unsafe fn find(p: *const u8, c: u8) -> *const u8 {
    if p.is_null() {
        null_fault();
    }
    let r = ffi::liella_machine_rawmemchr(p, i32::from(c));
    if cfg!(feature = "selftest") {
        // UNSAFE(slice): checking a scanner's answer: the byte at the reported index, then a rescan after it if a writer changed it (D-4.21).
        // SAFETY: the caller's contract; the bytes are readable through the asserted occurrence.
        p.wrapping_add(unsafe { settle_until(p, c, r.addr().wrapping_sub(p.addr())) })
    } else {
        r
    }
}

/// `memchr`: a pointer to the first `c` in `[p, p + n)`, or NULL. Shipped builds return the
/// scanner's pointer unread, so the export tail-calls it (STR-K3, as [`find`]); the selftest
/// build checks it (D-4.21, RESULT CHECKS ruling). `n == 0` reads nothing and accepts NULL; a
/// NULL `p` with `n > 0`, or `n > isize::MAX`, aborts (`EFAULT`, as before STR-K3).
///
/// # Safety
/// `p` is NULL (aborts unless `n == 0`) or its bytes are readable in order up to the first `c`
/// or `n` bytes, whichever comes first (C17 7.24.5.1p2).
#[inline]
// UNSAFE(slice): the selftest build checks the scanner's answer (D-4.21).
#[allow(unsafe_code)]
pub unsafe fn find_n(p: *const u8, c: i32, n: usize) -> *const u8 {
    // One test for both length edges (n == 0 wraps to usize::MAX); the edges and NULL leave
    // through a tail call, so the hot path keeps no frame (STR-K3).
    if n.wrapping_sub(1) >= usize::MAX >> 1 || p.is_null() {
        return find_n_edge(n);
    }
    // The scanner compares the low byte of `c` only (C17 7.24.5.1p2: converted to unsigned char).
    let r = ffi::liella_machine_memchr(p, c, n);
    if cfg!(feature = "selftest") {
        // UNSAFE(slice): checking a scanner's answer: the byte it points at, then a rescan after it if a writer changed it (D-4.21).
        // SAFETY: the caller's contract; the scanner reported `r` for `p`, `c` and `n`.
        unsafe { settle_find_n(p, crate::conv::byte(c), n, r) }
    } else {
        r
    }
}

/// `rawmemchr` through its direct entry (SCAN-TAIL; the products' `rawmemchr` is that entry, this
/// is its Rust twin so the tests run the assembly): a NULL `p` goes to [`find`] (abort) through
/// `liella_hot_rawmemchr_rest`; any other `p` enters the kernel with `c` as given (it reads the
/// low byte only). The selftest build checks the answer as [`find`] does (D-4.21).
///
/// # Safety
/// As [`find`].
#[inline]
// UNSAFE(slice): the selftest build checks the scanner's answer (D-4.21).
#[allow(unsafe_code)]
pub unsafe fn find_direct(p: *const u8, c: i32) -> *const u8 {
    let r = ffi::liella_machine_rawmemchr_entry(p, c);
    if cfg!(feature = "selftest") {
        debug_assert!(!p.is_null(), "the rawmemchr entry returned for a NULL pointer");
        let c = crate::conv::byte(c);
        // UNSAFE(slice): checking a scanner's answer, as find (D-4.21).
        // SAFETY: the entry returned, so p is non-NULL (a NULL aborts); the caller's contract.
        p.wrapping_add(unsafe { settle_until(p, c, r.addr().wrapping_sub(p.addr())) })
    } else {
        r
    }
}

/// `memchr` through its direct entry (MEMCHR-FOLD; the products' `memchr` is that entry, this is
/// its Rust twin so the tests run the assembly). With NULL folding on, `1 <= n <= isize::MAX`
/// enters the kernel with no NULL test (a NULL faults on page zero); every other case comes back
/// to [`find_n`] through `liella_hot_memchr_rest`. The selftest build checks the answer as
/// [`find_n`] does (D-4.21, RESULT CHECKS ruling).
///
/// # Safety
/// As [`find_n`].
#[inline]
// UNSAFE(slice): the selftest build checks the scanner's answer (D-4.21).
#[allow(unsafe_code)]
pub unsafe fn find_n_direct(p: *const u8, c: i32, n: usize) -> *const u8 {
    let r = ffi::liella_machine_memchr_entry(p, c, n);
    if cfg!(feature = "selftest") && n.wrapping_sub(1) < usize::MAX >> 1 {
        debug_assert!(!p.is_null(), "the memchr entry returned for a NULL region");
        // UNSAFE(slice): checking a scanner's answer, as find_n (D-4.21).
        // SAFETY: 1 <= n <= isize::MAX and the entry returned, so p was read (folded: a NULL
        // faults on page zero) or checked (find_n aborts on NULL); the caller's contract.
        unsafe { settle_find_n(p, crate::conv::byte(c), n, r) }
    } else {
        r
    }
}

/// [`find_n`] for `n == 0` (NULL, nothing read), and for a NULL `p` with `n > 0` or
/// `n > isize::MAX` (both abort).
#[cold]
#[inline(never)]
fn find_n_edge(n: usize) -> *const u8 {
    if n == 0 { core::ptr::null() } else { null_fault() }
}

/// Check the pointer `r` the `memchr` scanner gave for `p`, `c` and `n` (D-4.21): outside
/// `[p, p + n)` is a scanner bug and aborts; a byte at `r` that is no longer `c` was changed by
/// a writer during the call, so the bytes after it are searched once more, in order, and that
/// pass's own reads decide the answer. NULL cannot be checked in constant time (the sweeps hold
/// it).
///
/// # Safety
/// As for [`find_n`] (`p` non-NULL, `0 < n <= isize::MAX`).
#[inline]
// UNSAFE(slice): the postcondition check reads the one byte the scanner reported.
#[allow(unsafe_code)]
unsafe fn settle_find_n(p: *const u8, c: u8, n: usize, r: *const u8) -> *const u8 {
    if r.is_null() {
        return r;
    }
    let i = r.addr().wrapping_sub(p.addr());
    if i >= n {
        scanner_fault();
    }
    // UNSAFE(slice): reading the one byte the scanner reported.
    // SAFETY: `i < n`, and the scanner read up to it (contract).
    if unsafe { byte_at(p, i) } == c {
        return r;
    }
    // UNSAFE(slice): rescanning after the reported byte, which a writer changed (D-4.21).
    // SAFETY: byte `i` is not `c`, so the bytes after it are readable in order up to `c` or `n`.
    unsafe { rescan_find_n(p, c, n, i) }
}

/// The index of the first `c` in `s` (`memchr` over a slice), by [`find_n`]: the same scanner
/// and the same shipped-build trust in its answer (D-4.21 RESULT CHECKS; the selftest build
/// checks it). Callers bound every use of the index by slice access.
#[inline]
pub(crate) fn find_in(s: &[u8], c: u8) -> Option<usize> {
    let p = s.as_ptr();
    // UNSAFE(slice): the scanner reads the bytes of a slice in order, inside its length.
    #[allow(unsafe_code)]
    // SAFETY: `s` is readable for its length (a slice).
    let r = unsafe { find_n(p, i32::from(c), s.len()) };
    if r.is_null() { None } else { Some(r.addr().wrapping_sub(p.addr())) }
}

/// [`settle_find_n`] applied to the answer `i` (an index; `s.len()` for none) for the slice `s`
/// (self-tests: the check the `memchr` export runs in the selftest build).
#[cfg(feature = "selftest")]
pub(crate) fn settle_find_in(s: &[u8], c: u8, i: usize) -> Option<usize> {
    let p = s.as_ptr();
    let r = if i == s.len() { core::ptr::null() } else { p.wrapping_add(i) };
    // UNSAFE(slice): the check reads bytes of a slice, in order, inside its length.
    #[allow(unsafe_code)]
    // SAFETY: `s` is readable for its length; `r` is NULL or `p + i`.
    let r = unsafe { settle_find_n(p, c, s.len(), r) };
    if r.is_null() { None } else { Some(r.addr().wrapping_sub(p.addr())) }
}

/// [`settle_find_n`]'s rescan: the first `c` in `(p + i, p + n)`, or NULL.
///
/// # Safety
/// As for [`settle_find_n`], `i < n`.
#[cold]
#[inline(never)]
// UNSAFE(slice): a byte-by-byte rescan under the caller's contract.
#[allow(unsafe_code)]
unsafe fn rescan_find_n(p: *const u8, c: u8, n: usize, i: usize) -> *const u8 {
    note_rescan();
    let mut k = i.wrapping_add(1);
    while k < n {
        // UNSAFE(slice): the next byte of the rescan.
        // SAFETY: `k < n`, read in order after bytes that were not `c` (contract).
        if unsafe { byte_at(p, k) } == c {
            return p.wrapping_add(k);
        }
        k = k.wrapping_add(1);
    }
    core::ptr::null()
}

// SHORT-1 (D-4.35): one-pass kernels for strchr, strchrnul, memrchr and strncmp. The exports
// tail-call them in shipped builds (RESULT CHECKS ruling: the kernels settle racing writers
// themselves, from the bytes they read); the selftest build compares every answer with the
// byte-loop reference over the measured text and aborts on a difference (the single-threaded
// sweeps then prove the kernels, planted bugs included).

/// `strchr`: the first `c` in the string at `p` (the terminator when `c` is 0), or NULL.
///
/// # Safety
/// `p` is NULL (aborts, S6) or a readable NUL-terminated string.
#[inline]
// UNSAFE(slice): the selftest build checks the scanner's answer (D-4.21).
#[allow(unsafe_code)]
pub unsafe fn chr(p: *const i8, c: i32) -> *const u8 {
    // The kernel is the direct entry: it takes a NULL p to the S6 abort itself (STR-CHR-CMP-SHORT).
    let r = ffi::liella_machine_strchr(p.cast(), c);
    if cfg!(feature = "selftest") {
        // UNSAFE(slice): measuring the caller's string for the reference answer.
        // SAFETY: the caller's contract.
        let want = unsafe { strz(p) }.ok().and_then(|t| liella::bytes::find_z(t, crate::conv::byte(c)));
        check_answer(p.cast(), r, want);
    }
    r
}

/// `strchrnul`: the first `c` in the string at `p`, or its terminator.
///
/// # Safety
/// As for [`chr`].
#[inline]
// UNSAFE(slice): the selftest build checks the scanner's answer (D-4.21).
#[allow(unsafe_code)]
pub unsafe fn chr_or_end(p: *const i8, c: i32) -> *const u8 {
    // The kernel is the direct entry: it takes a NULL p to the S6 abort itself (STR-CHR-CMP-SHORT).
    let r = ffi::liella_machine_strchrnul(p.cast(), c);
    if cfg!(feature = "selftest") {
        // UNSAFE(slice): measuring the caller's string for the reference answer.
        // SAFETY: the caller's contract.
        let want = unsafe { strz(p) }.ok().map(|t| liella::bytes::find_z_or_end(t, crate::conv::byte(c)));
        check_answer(p.cast(), r, want);
    }
    r
}

/// `memrchr`: the last `c` in `[p, p + n)`, or NULL. `n == 0` reads nothing and accepts NULL; a
/// NULL `p` with `n > 0`, or `n > isize::MAX`, aborts (`EFAULT`, as [`find_n`]).
///
/// # Safety
/// `p` is NULL (aborts unless `n == 0`) or `[p, p + n)` is readable.
#[inline]
// UNSAFE(slice): the selftest build checks the scanner's answer (D-4.21).
#[allow(unsafe_code)]
pub unsafe fn rfind_n(p: *const u8, c: i32, n: usize) -> *const u8 {
    if n.wrapping_sub(1) >= usize::MAX >> 1 || p.is_null() {
        return find_n_edge(n);
    }
    let r = ffi::liella_machine_memrchr(p, c, n);
    if cfg!(feature = "selftest") {
        // UNSAFE(slice): the caller's n readable bytes, for the reference answer.
        // SAFETY: p is non-NULL, n <= isize::MAX, and the caller's contract makes them readable.
        let want = liella::bytes::rfind(unsafe { core::slice::from_raw_parts(p, n) }, crate::conv::byte(c));
        check_answer(p, r, want);
    }
    r
}

/// `memrchr` through its direct entry (SCAN-RAW; the products' `memrchr` is that entry, this is
/// its Rust twin so the tests run the assembly). `1 <= n <= isize::MAX` with a non-NULL `p`
/// enters the kernel; every other case comes back to [`rfind_n`] through
/// `liella_hot_memrchr_rest`. The selftest build checks the answer as [`rfind_n`] does (D-4.21).
///
/// # Safety
/// As [`rfind_n`].
#[inline]
// UNSAFE(slice): the selftest build checks the scanner's answer (D-4.21).
#[allow(unsafe_code)]
pub unsafe fn rfind_n_direct(p: *const u8, c: i32, n: usize) -> *const u8 {
    let r = ffi::liella_machine_memrchr_entry(p, c, n);
    if cfg!(feature = "selftest") && n.wrapping_sub(1) < usize::MAX >> 1 {
        debug_assert!(!p.is_null(), "the memrchr entry returned for a NULL region");
        // UNSAFE(slice): the caller's n readable bytes, for the reference answer.
        // SAFETY: p is non-NULL (a NULL aborts), n <= isize::MAX, and the caller's contract.
        let want = liella::bytes::rfind(unsafe { core::slice::from_raw_parts(p, n) }, crate::conv::byte(c));
        check_answer(p, r, want);
    }
    r
}

/// `strncmp`: the order of at most `n` bytes of the strings at `a` and `b`, -1, 0 or 1. With
/// `n == 0` nothing is read and any pointer, NULL included, compares equal (as [`strz_n`]); a
/// NULL pointer with `n > 0` aborts (S6).
///
/// # Safety
/// `a` and `b` are NULL or readable up to their terminators or `n` bytes.
#[inline]
// UNSAFE(slice): the selftest build checks the kernel's answer (D-4.21).
#[allow(unsafe_code)]
pub unsafe fn order_n(a: *const i8, b: *const i8, n: usize) -> i32 {
    // The kernel is the direct entry: n == 0 gives 0 before any read, a NULL a or b with n > 0 the
    // S6 abort (STR-CHR-CMP-SHORT).
    let r = ffi::liella_machine_strncmp(a.cast(), b.cast(), n);
    if cfg!(feature = "selftest") {
        // UNSAFE(slice): measuring both texts within n for the reference answer.
        // SAFETY: the caller's contract.
        let want = unsafe { strz_n(a, n) }.and_then(|x| unsafe { strz_n(b, n) }.map(|y| crate::conv::compare(x, y)));
        if want.ok().map(|o| match o { core::cmp::Ordering::Less => -1, core::cmp::Ordering::Equal => 0, core::cmp::Ordering::Greater => 1 }) != Some(r) {
            scanner_fault();
        }
    }
    r
}

/// The selftest check of a pointer-returning scanner: `r` must be `p + want`, or NULL for
/// `None`; anything else aborts.
fn check_answer(p: *const u8, r: *const u8, want: Option<usize>) {
    if want.map_or(core::ptr::null(), |i| p.wrapping_add(i)) != r {
        scanner_fault();
    }
}
