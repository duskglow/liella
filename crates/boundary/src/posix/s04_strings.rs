//! Translation for spec §4: byte arrays and strings.
//!
//! Every symbol of §4 except the three `string2.h` copy helpers `__mempcpy_small`,
//! `__stpcpy_small` and `__strcpy_small`, whose parameter lists the ABI tables do not record
//! (ERRATA E6; they are compat-only, so no program can link against them anew).
//!
//! * Fortified forms (`__*_chk`) take the destination size in bytes for narrow functions and in
//!   `wchar_t` units for wide ones (`bits/wchar2.h` passes `objsize / sizeof(wchar_t)`).
//! * The `_l` forms ignore the locale object: every locale is the C locale (D-6.7).
//! * Tokenizer cursors and `strtok`'s hidden position are addresses (`string_compat`).

use liella::bytes;
use liella::random;
use liella_abi::ctypes::{c_char, c_int, c_uint, c_void, locale_t, size_t, ssize_t, wchar_t};
use liella_kernel::errno::Errno;
use liella_legacy::cstring;
use liella_legacy::string_compat;

use crate::caller_alloc;
use crate::conv;
use crate::hot;
use crate::locale;
use crate::export;
use crate::native::secure;
use crate::ret;
use crate::strz;
use crate::sys::CSys;

export! { asm "GLIBC_2.14";
pub extern "C" fn memcpy(d: *mut c_void, s: *const c_void, n: size_t) -> *mut c_void {
    // UNSAFE(slice) SAFETY: d and s are valid for n bytes (§4.2; overlap is moved correctly,
    // D-4.2); sizes up to 128 take the hot kernel (D-4.22).
    unsafe { hot::move_bytes(d, s, n) }
}}

export! { "GLIBC_2.3.4";
pub extern "C" fn __memcpy_chk(d: *mut c_void, s: *const c_void, n: size_t, destlen: size_t) -> *mut c_void {
    // UNSAFE(slice) SAFETY: as memmove, after the fortify check of n against destlen.
    unsafe { hot::move_bytes(d, s, conv::chk(n, destlen)) }
}}

export! { "GLIBC_2.3.4";
pub extern "C" fn __memmove_chk(d: *mut c_void, s: *const c_void, n: size_t, destlen: size_t) -> *mut c_void {
    // UNSAFE(slice) SAFETY: as memmove, after the fortify check of n against destlen.
    unsafe { hot::move_bytes(d, s, conv::chk(n, destlen)) }
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn memmove(d: *mut c_void, s: *const c_void, n: size_t) -> *mut c_void {
    // UNSAFE(slice) SAFETY: d and s are valid for n bytes and may overlap (§4.2); sizes up to
    // 128 take the hot kernel (D-4.22).
    unsafe { hot::move_bytes(d, s, n) }
}}

crate::internal_entry! {
/// `memcpy@GLIBC_2.2.5`: the old contract is overlap-safe, `memmove`'s (probe-settled, LG-2);
/// exported only under that compat version (`csys/src/machine/compat_aliases.S`).
pub extern "C" fn liella_compat_memcpy(d: *mut c_void, s: *const c_void, n: size_t) -> *mut c_void {
    // UNSAFE(slice) SAFETY: d and s are valid for n bytes and may overlap (memcpy@GLIBC_2.2.5).
    unsafe { hot::move_bytes(d, s, n) }
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn mempcpy(d: *mut c_void, s: *const c_void, n: size_t) -> *mut c_void {
    // UNSAFE(slice) SAFETY: d and s are valid for n bytes (§4.2).
    unsafe { copy_past(d, s, n) }
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn __mempcpy(d: *mut c_void, s: *const c_void, n: size_t) -> *mut c_void {
    // UNSAFE(slice) SAFETY: d and s are valid for n bytes (§4.2).
    unsafe { copy_past(d, s, n) }
}}

export! { "GLIBC_2.3.4";
pub extern "C" fn __mempcpy_chk(d: *mut c_void, s: *const c_void, n: size_t, destlen: size_t) -> *mut c_void {
    // UNSAFE(slice) SAFETY: as mempcpy, after the fortify check.
    unsafe { copy_past(d, s, conv::chk(n, destlen)) }
}}

/// `mempcpy`'s copy: the same hot kernel as `memcpy` (D-4.22), returning `d + n` (WL-HOT-SEQ: the
/// generic unit copy it used before cost 3x memcpy's instructions, a third of `seq`'s libc time).
/// In the products `mempcpy` and `__mempcpy` are the direct entry itself (machine/fold.S,
/// WL-HOT-SEQ2); this is their Rust twin and `__mempcpy_chk`'s copy.
///
/// # Safety
/// `d` and `s` are valid for `n` bytes (the caller's contract, §4.2).
// UNSAFE(slice): the caller's two C regions go to the hot copy kernel, as memmove's do.
#[allow(unsafe_code)]
unsafe fn copy_past(d: *mut c_void, s: *const c_void, n: size_t) -> *mut c_void {
    // UNSAFE(slice) SAFETY: the function's contract.
    unsafe { hot::move_bytes_past(d, s, n) }
}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn memset(d: *mut c_void, c: c_int, n: size_t) -> *mut c_void {
    // UNSAFE(slice) SAFETY: d is valid for writing n bytes (§4.2); sizes up to 128 take the hot
    // kernel (D-4.22).
    unsafe { hot::set_bytes(d, c, n) }
}}

export! { "GLIBC_2.3.4";
pub extern "C" fn __memset_chk(d: *mut c_void, c: c_int, n: size_t, destlen: size_t) -> *mut c_void {
    // UNSAFE(slice) SAFETY: as memset, after the fortify check.
    unsafe { hot::set_bytes(d, c, conv::chk(n, destlen)) }
}}

export! { local "liella_hot_move_rest";
/// The copy entry's other sizes, reached by name from the direct entry `memcpy`/`memmove`
/// (machine/fold.S, D-4.22 COPY-FILL).
pub extern "C" fn hot_move_rest(d: *mut c_void, s: *const c_void, n: size_t) -> *mut c_void {
    // UNSAFE(slice) SAFETY: the memmove contract, passed on unchanged by the direct entry.
    unsafe { hot::move_rest(d, s, n) }
}}

export! { local "liella_hot_fill_rest";
/// The fill entry's other sizes, as `hot_move_rest` (from the direct entry `memset`).
pub extern "C" fn hot_fill_rest(d: *mut c_void, c: c_int, n: size_t) -> *mut c_void {
    // UNSAFE(slice) SAFETY: the memset contract, passed on unchanged by the direct entry.
    unsafe { hot::fill_rest(d, c, n) }
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn bzero(d: *mut c_void, n: size_t) {
    // UNSAFE(slice) SAFETY: d is valid for writing n bytes (§4.2).
    ret::unit(unsafe { conv::buf_mut(d, n) }.map(|b| conv::fill(b, 0)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __bzero(d: *mut c_void, n: size_t) {
    // UNSAFE(slice) SAFETY: d is valid for writing n bytes (§4.2).
    ret::unit(unsafe { conv::buf_mut(d, n) }.map(|b| conv::fill(b, 0)))
}}

export! { "GLIBC_2.25";
pub extern "C" fn explicit_bzero(d: *mut c_void, n: size_t) {
    // The native interface's secure zeroing, unchanged (D-X.11); NULL with n > 0 aborts (S6).
    ret::unit(Errno::decode(secure::liella_secure_zero(d, n)).map(drop))
}}

export! { "GLIBC_2.25";
pub extern "C" fn __explicit_bzero_chk(d: *mut c_void, n: size_t, destlen: size_t) {
    // As explicit_bzero, after the fortify check.
    ret::unit(Errno::decode(secure::liella_secure_zero(d, conv::chk(n, destlen))).map(drop))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn bcopy(s: *const c_void, d: *mut c_void, n: size_t) {
    // UNSAFE(slice) SAFETY: d and s are valid for n bytes and may overlap (§4.2).
    ret::unit(unsafe { conv::move_units::<u8>(d, s, n) }.map(drop))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn memccpy(d: *mut c_void, s: *const c_void, c: c_int, n: size_t) -> *mut c_void {
    // UNSAFE(slice) SAFETY: d and s are valid, non-overlapping regions of n bytes (§4.2).
    ret::found(d.cast::<u8>(), unsafe { conv::buf(s, n) }.and_then(|src| unsafe { conv::buf_mut(d, n) }.map(|dst| bytes::copy_until(dst, src, conv::byte(c))))).cast()
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn memchr(s: *const c_void, c: c_int, n: size_t) -> *mut c_void {
    // UNSAFE(slice) SAFETY: s is readable in order up to the first c or n bytes (§4.5, C17 7.24.5.1p2); the direct entry (machine/scan.S, MEMCHR-FOLD, D-4.48).
    unsafe { strz::find_n_direct(s.cast(), c, n) }.cast_mut().cast()
}}

export! { local "liella_hot_memchr_rest";
/// The memchr entry's other cases (n == 0, n > isize::MAX, folding off), reached by name from the
/// direct entry `memchr` (machine/scan.S, MEMCHR-FOLD): the checked entry, which also takes the
/// lazy folding decision.
pub extern "C" fn hot_memchr_rest(s: *const c_void, c: c_int, n: size_t) -> *mut c_void {
    crate::dispatch::resolve_fold_any();
    // UNSAFE(slice) SAFETY: the memchr contract, passed on unchanged by the direct entry; NULL with n > 0 aborts. STR-K3.
    unsafe { strz::find_n(s.cast(), c, n) }.cast_mut().cast()
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn memrchr(s: *const c_void, c: c_int, n: size_t) -> *mut c_void {
    // UNSAFE(slice) SAFETY: s is readable for n bytes (§4.5); NULL with n > 0 aborts. One kernel scan from the end (SHORT-1, D-4.35); the direct entry (machine/scan.S, SCAN-RAW).
    unsafe { strz::rfind_n_direct(s.cast(), c, n) }.cast_mut().cast()
}}

export! { local "liella_hot_memrchr_rest";
/// The memrchr entry's other cases (n == 0, n > isize::MAX, NULL s), reached by name from the
/// direct entry `memrchr` (machine/scan.S, SCAN-RAW): the checked entry (NULL for n == 0, else
/// the S6 abort).
pub extern "C" fn hot_memrchr_rest(s: *const c_void, c: c_int, n: size_t) -> *mut c_void {
    // UNSAFE(slice) SAFETY: the memrchr contract, passed on unchanged by the direct entry; NULL with n > 0 aborts. S6.
    unsafe { strz::rfind_n(s.cast(), c, n) }.cast_mut().cast()
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn rawmemchr(s: *const c_void, c: c_int) -> *mut c_void {
    // UNSAFE(slice) SAFETY: the caller asserts that c occurs in s (§4.5); NULL aborts. The direct entry (machine/scan.S, SCAN-TAIL, D-4.14 as amended).
    unsafe { strz::find_direct(s.cast(), c) }.cast_mut().cast()
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn __rawmemchr(s: *const c_void, c: c_int) -> *mut c_void {
    // UNSAFE(slice) SAFETY: the caller asserts that c occurs in s (§4.5); NULL aborts. The direct entry (machine/scan.S, SCAN-TAIL, D-4.14 as amended).
    unsafe { strz::find_direct(s.cast(), c) }.cast_mut().cast()
}}

export! { local "liella_hot_rawmemchr_rest";
/// The rawmemchr entry's NULL case, reached by name from the direct entry `rawmemchr`
/// (machine/scan.S, SCAN-TAIL): the checked entry, which aborts (S6).
pub extern "C" fn hot_rawmemchr_rest(s: *const c_void, c: c_int) -> *mut c_void {
    // UNSAFE(slice) SAFETY: the rawmemchr contract, passed on unchanged by the direct entry; NULL aborts. S6.
    unsafe { strz::find(s.cast(), conv::byte(c)) }.cast_mut().cast()
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn memcmp(a: *const c_void, b: *const c_void, n: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are readable for n bytes (§4.3); the direct entry (machine/fold.S,
    // MEMCMP-FOLD, D-4.23).
    unsafe { hot::compare_direct(a, b, n) }
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn bcmp(a: *const c_void, b: *const c_void, n: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are readable for n bytes (§4.3); the equality entry, 0 iff equal
    // (the spec asks for no order; owner ruling 2026-10-10; D-4.43; docs/compat-flags.md COMPAT-1).
    unsafe { hot::equal_direct(a, b, n) }
}}

export! { asm "GLIBC_2.35";
pub extern "C" fn __memcmpeq(a: *const c_void, b: *const c_void, n: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are readable for n bytes (§4.3); the equality entry (D-4.41).
    unsafe { hot::equal_direct(a, b, n) }
}}

export! { local "liella_hot_compare_rest";
/// The compare entry's other cases (above 64, folding off, NULL with 0-7 bytes, a 2-7 byte word that
/// re-reads equal under a concurrent writer), reached by name from the
/// direct entry `memcmp` (machine/fold.S, MEMCMP-FOLD): the checked entry.
pub extern "C" fn hot_compare_rest(a: *const c_void, b: *const c_void, n: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: the memcmp contract, passed on unchanged by the direct entry.
    unsafe { hot::compare_rest(a, b, n) }
}}

export! { local "liella_hot_compare_tail";
/// The compare entry's classes after an equal head probe (machine/fold.S, MEMCMP-FOLD).
pub extern "C" fn hot_compare_tail(a: *const c_void, b: *const c_void, n: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: 8 <= n <= 64, a and b just read by the direct entry (non-NULL), bytes 0-7 equal.
    unsafe { hot::compare_tail(a, b, n) }
}}

export! { local "liella_hot_compare_big";
/// The compare entry's sizes above 64 (machine/fold.S, MEMCMP-SHORT): the checked classes without
/// the small-size range test and the folding decision (which only 1-64 bytes take).
pub extern "C" fn hot_compare_big(a: *const c_void, b: *const c_void, n: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: the memcmp contract with n > 64, passed on unchanged by the direct entry.
    unsafe { hot::compare_big(a, b, n) }
}}

export! { local "liella_hot_compare_rescan";
/// The compare entry's 65-2048-byte classes when the reported first difference re-reads equal
/// (machine/fold.S `.Lcmp_big_byte`, MEMCMP-MID-2): the counted D-4.21 rescan from `i + 1`.
pub extern "C" fn hot_compare_rescan(a: *const c_void, b: *const c_void, n: size_t, i: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: the memcmp contract, i < n, a and b just read at i by the direct entry.
    unsafe { hot::compare_rescan(a, b, n, i) }
}}

export! { local "liella_hot_equal_big";
/// The equality entry's sizes above 64, as `hot_compare_big` (from `bcmp`/`__memcmpeq`).
pub extern "C" fn hot_equal_big(a: *const c_void, b: *const c_void, n: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: the bcmp contract with n > 64, passed on unchanged by the direct entry.
    unsafe { hot::equal_big(a, b, n) }
}}

export! { local "liella_hot_equal_rest";
/// The equality entry's other cases, as `hot_compare_rest` (from `bcmp`/`__memcmpeq`).
pub extern "C" fn hot_equal_rest(a: *const c_void, b: *const c_void, n: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: the bcmp contract, passed on unchanged by the direct entry.
    unsafe { hot::equal_rest_entry(a, b, n) }
}}

export! { local "liella_hot_equal_tail";
/// The equality entry's classes after an equal head probe (machine/fold.S, MEMCMP-FOLD).
pub extern "C" fn hot_equal_tail(a: *const c_void, b: *const c_void, n: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: 8 <= n <= 64, a and b just read by the direct entry (non-NULL), bytes 0-7 equal.
    unsafe { hot::equal_tail(a, b, n) }
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn memmem(h: *const c_void, hl: size_t, n: *const c_void, nl: size_t) -> *mut c_void {
    // UNSAFE(slice) SAFETY: h and n are readable for hl and nl bytes (§4.6).
    ret::found(h.cast::<u8>(), unsafe { conv::buf(h, hl) }.and_then(|x| unsafe { conv::buf(n, nl) }.map(|y| conv::find_slice(x, y)))).cast()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn memfrob(s: *mut c_void, n: size_t) -> *mut c_void {
    // UNSAFE(slice) SAFETY: s is writable for n bytes (§4.12).
    ret::same(s, unsafe { conv::buf_mut(s, n) }.map(bytes::frob))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn swab(from: *const c_void, to: *mut c_void, n: ssize_t) {
    // UNSAFE(slice) SAFETY: from and to are distinct regions of n bytes (§4.12).
    ret::unit(unsafe { conv::buf(from, conv::count(n)) }.and_then(|s| unsafe { conv::buf_mut(to, conv::count(n)) }.map(|d| bytes::swap_pairs(d, s))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strlen(s: *const c_char) -> size_t {
    // UNSAFE(slice) SAFETY: s is a string (§4.8); NULL aborts (S6). D-4.14 as amended 2026-10-08.
    unsafe { strz::len(s) }
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strnlen(s: *const c_char, n: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: s is readable up to its terminator or n bytes (§4.8). D-4.14 as amended.
    unsafe { strz::len_n(s, n) }
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn strcmp(a: *const c_char, b: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are strings (§4.3; collation in the C locale is strcmp); the direct entry (machine/scan.S, STRCMP-ENTRY): NULL tests, then the one-pass kernel (PJ-8, D-4.29).
    unsafe { strz::order_direct(a, b) }
}}

export! { local "liella_hot_strcmp_null";
/// The strcmp entry's NULL case, reached by name from the direct entry `strcmp` (machine/scan.S,
/// STRCMP-ENTRY): the checked entry, which aborts (S6).
pub extern "C" fn hot_strcmp_null(a: *const c_char, b: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: a or b is NULL here; `strz::order` aborts before reading (S6, §4.3).
    unsafe { strz::order(a, b) }
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strcoll(a: *const c_char, b: *const c_char) -> c_int {
    let l = locale::current();
    // UNSAFE(slice) SAFETY: a and b are NULL or strings (§4.3; D-4.18): without collation rules
    // byte order (as strcmp, PJ-8, D-4.29); else the first difference or one pass decides most
    // pairs (D-4.37), the general path the rest.
    unsafe { strz::coll(a, b, l) }
}}

export! { "GLIBC_2.3";
pub extern "C" fn strcoll_l(a: *const c_char, b: *const c_char, loc: locale_t) -> c_int {
    let l = locale::of(conv::addr(loc));
    // UNSAFE(slice) SAFETY: a and b are NULL or strings (§4.3; D-4.18): without collation rules
    // byte order (as strcmp, PJ-8, D-4.29); else the first difference or one pass decides most
    // pairs (D-4.37), the general path the rest.
    unsafe { strz::coll(a, b, l) }
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strcoll_l(a: *const c_char, b: *const c_char, loc: locale_t) -> c_int {
    let l = locale::of(conv::addr(loc));
    // UNSAFE(slice) SAFETY: a and b are NULL or strings (§4.3; D-4.18): without collation rules
    // byte order (as strcmp, PJ-8, D-4.29); else the first difference or one pass decides most
    // pairs (D-4.37), the general path the rest.
    unsafe { strz::coll(a, b, l) }
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn strncmp(a: *const c_char, b: *const c_char, n: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are readable up to their terminators or n bytes (§4.3).
    // The direct entry (machine/scan.S, STR-CHR-CMP-SHORT): one kernel pass (SHORT-1, D-4.35); n == 0 reads nothing, NULL with n > 0 aborts.
    unsafe { strz::order_n(a, b, n) }
}}

export! { local "liella_hot_strncmp_null";
/// The strncmp entry's NULL case (n > 0), reached by name from the direct entry `strncmp`
/// (machine/scan.S, STR-CHR-CMP-SHORT): the S6 abort, before any read.
pub extern "C" fn hot_strncmp_null(a: *const c_char, b: *const c_char, n: size_t) -> c_int {
    let _ = (a, b, n);
    strz::null_abort()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strcasecmp(a: *const c_char, b: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are strings (§4.3).
    unsafe { let v = locale::view(); strz::casecmp_int(a, b, usize::MAX, v.ascii_case(), v.locale()) }
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strcasecmp(a: *const c_char, b: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are strings (§4.3).
    unsafe { let v = locale::view(); strz::casecmp_int(a, b, usize::MAX, v.ascii_case(), v.locale()) }
}}

export! { local "liella_hot_casecmp_rest";
/// The casecmp ordering kernels' way out (machine/scan.S, STRCASECMP-SHORT-2), reached by name
/// with the arguments the kernel got: the deciding bytes hold one of 128 or more (`code` 2: the
/// measured path for `loc`), or an index above `n` (`code` 3: abort, D-4.21).
pub extern "C" fn hot_casecmp_rest(a: *const c_char, b: *const c_char, n: size_t, loc: *const c_void, code: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: the strcasecmp/strncasecmp contract and the kernel's `loc`, passed on
    // unchanged by the kernel (`strz::casecmp_rest`).
    unsafe { strz::casecmp_rest(a, b, n, loc, code) }
}}

export! { "GLIBC_2.3";
pub extern "C" fn strcasecmp_l(a: *const c_char, b: *const c_char, loc: locale_t) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are strings (§4.3).
    ret::order(unsafe { strz::casecmp(a, b, usize::MAX, locale::of(conv::addr(loc))) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strcasecmp_l(a: *const c_char, b: *const c_char, loc: locale_t) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are strings (§4.3).
    ret::order(unsafe { strz::casecmp(a, b, usize::MAX, locale::of(conv::addr(loc))) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strncasecmp(a: *const c_char, b: *const c_char, n: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are readable up to their terminators or n bytes (§4.3).
    unsafe { let v = locale::view(); strz::casecmp_int(a, b, n, v.ascii_case(), v.locale()) }
}}

export! { "GLIBC_2.3";
pub extern "C" fn strncasecmp_l(a: *const c_char, b: *const c_char, n: size_t, loc: locale_t) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are readable up to their terminators or n bytes (§4.3).
    ret::order(unsafe { strz::casecmp(a, b, n, locale::of(conv::addr(loc))) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strncasecmp_l(a: *const c_char, b: *const c_char, n: size_t, loc: locale_t) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are readable up to their terminators or n bytes (§4.3).
    ret::order(unsafe { strz::casecmp(a, b, n, locale::of(conv::addr(loc))) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strverscmp(a: *const c_char, b: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are strings (§4.3).
    ret::order(unsafe { strz::strz(a) }.and_then(|x| unsafe { strz::strz(b) }.map(|y| bytes::compare_versions(x, y))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strverscmp(a: *const c_char, b: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are strings (§4.3).
    ret::order(unsafe { strz::strz(a) }.and_then(|x| unsafe { strz::strz(b) }.map(|y| bytes::compare_versions(x, y))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strxfrm(d: *mut c_char, s: *const c_char, n: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: s is a string, d has room for n bytes (§4.4).
    ret::must(unsafe { strz::strz(s) }.and_then(|t| cstring::xfrm_fill(n, unsafe { conv::dest(d) }, |o| locale::current().xfrm_bytes_into(t, o), || locale::current().xfrm_bytes(t))))
}}

export! { "GLIBC_2.3";
pub extern "C" fn strxfrm_l(d: *mut c_char, s: *const c_char, n: size_t, loc: locale_t) -> size_t {
    // UNSAFE(slice) SAFETY: s is a string, d has room for n bytes (§4.4).
    ret::must(unsafe { strz::strz(s) }.and_then(|t| cstring::xfrm_fill(n, unsafe { conv::dest(d) }, |o| locale::of(conv::addr(loc)).xfrm_bytes_into(t, o), || locale::of(conv::addr(loc)).xfrm_bytes(t))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strxfrm_l(d: *mut c_char, s: *const c_char, n: size_t, loc: locale_t) -> size_t {
    // UNSAFE(slice) SAFETY: s is a string, d has room for n bytes (§4.4).
    ret::must(unsafe { strz::strz(s) }.and_then(|t| cstring::xfrm_fill(n, unsafe { conv::dest(d) }, |o| locale::of(conv::addr(loc)).xfrm_bytes_into(t, o), || locale::of(conv::addr(loc)).xfrm_bytes(t))))
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn strchr(s: *const c_char, c: c_int) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s is a string (§4.5); NULL aborts. The direct entry (machine/scan.S, STR-CHR-CMP-SHORT): one kernel pass (SHORT-1, D-4.35).
    unsafe { strz::chr(s, c) }.cast_mut().cast()
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn index(s: *const c_char, c: c_int) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s is a string (§4.5); NULL aborts. The direct entry (machine/scan.S, STR-CHR-CMP-SHORT): one kernel pass (SHORT-1, D-4.35).
    unsafe { strz::chr(s, c) }.cast_mut().cast()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strrchr(s: *const c_char, c: c_int) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s is a string (§4.5).
    ret::found(s, unsafe { strz::strz(s) }.map(|t| bytes::rfind_z(t, conv::byte(c))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn rindex(s: *const c_char, c: c_int) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s is a string (§4.5).
    ret::found(s, unsafe { strz::strz(s) }.map(|t| bytes::rfind_z(t, conv::byte(c))))
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn strchrnul(s: *const c_char, c: c_int) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s is a string (§4.5); NULL aborts. The direct entry (machine/scan.S, STR-CHR-CMP-SHORT): one kernel pass (SHORT-1, D-4.35).
    unsafe { strz::chr_or_end(s, c) }.cast_mut().cast()
}}

export! { local "liella_hot_strchr_null";
/// The NULL case of the strchr, index and strchrnul entries, reached by name from the direct
/// entries (machine/scan.S, STR-CHR-CMP-SHORT): the S6 abort, before any read.
pub extern "C" fn hot_strchr_null(s: *const c_char, c: c_int) -> *mut c_char {
    let _ = (s, c);
    strz::null_abort()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strstr(h: *const c_char, n: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: h and n are strings (§4.6). The needle is measured with the haystack
    // prefix (`strz::find_str_n`, STRSTR-SHORT-SETUP); the haystack is read by the fused candidate
    // and terminator scan (STRSTR-VEC).
    ret::found(h, unsafe { strz::find_str_n(h, n) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strcasestr(h: *const c_char, n: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: h and n are strings (§4.6).
    ret::found(h, unsafe { strz::strz(h) }.and_then(|x| unsafe { strz::strz(n) }.map(|y| conv::find_slice_ci(x, y))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strcasestr(h: *const c_char, n: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: h and n are strings (§4.6).
    ret::found(h, unsafe { strz::strz(h) }.and_then(|x| unsafe { strz::strz(n) }.map(|y| conv::find_slice_ci(x, y))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strspn(s: *const c_char, set: *const c_char) -> size_t {
    // UNSAFE(slice) SAFETY: s and set are strings (§4.7).
    ret::must(unsafe { strz::strz(s) }.and_then(|t| unsafe { strz::strz(set) }.map(|u| bytes::span(t, bytes::ByteSet::new(u).predicate()))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strcspn(s: *const c_char, set: *const c_char) -> size_t {
    // UNSAFE(slice) SAFETY: s and set are strings (§4.7).
    ret::must(unsafe { strz::strz(s) }.and_then(|t| unsafe { strz::strz(set) }.map(|u| bytes::span(t, bytes::ByteSet::new(u).absent()))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strpbrk(s: *const c_char, set: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s and set are strings (§4.7).
    ret::found(s, unsafe { strz::strz(s) }.and_then(|t| unsafe { strz::strz(set) }.map(|u| bytes::find_any(t, bytes::ByteSet::new(u).predicate()))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strspn_c1(s: *const c_char, a: c_int) -> size_t {
    // UNSAFE(slice) SAFETY: s is a string (§4.7, compat).
    ret::must(unsafe { strz::strz(s) }.map(|t| bytes::span(t, bytes::ByteSet::new(&[conv::byte(a)]).predicate())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strspn_c2(s: *const c_char, a1: c_int, a2: c_int) -> size_t {
    // UNSAFE(slice) SAFETY: s is a string (§4.7, compat).
    ret::must(unsafe { strz::strz(s) }.map(|t| bytes::span(t, bytes::ByteSet::new(&[conv::byte(a1), conv::byte(a2)]).predicate())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strspn_c3(s: *const c_char, a1: c_int, a2: c_int, a3: c_int) -> size_t {
    // UNSAFE(slice) SAFETY: s is a string (§4.7, compat).
    ret::must(unsafe { strz::strz(s) }.map(|t| bytes::span(t, bytes::ByteSet::new(&[conv::byte(a1), conv::byte(a2), conv::byte(a3)]).predicate())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strcspn_c1(s: *const c_char, r: c_int) -> size_t {
    // UNSAFE(slice) SAFETY: s is a string (§4.7, compat).
    ret::must(unsafe { strz::strz(s) }.map(|t| bytes::span(t, bytes::ByteSet::new(&[conv::byte(r)]).absent())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strcspn_c2(s: *const c_char, r1: c_int, r2: c_int) -> size_t {
    // UNSAFE(slice) SAFETY: s is a string (§4.7, compat).
    ret::must(unsafe { strz::strz(s) }.map(|t| bytes::span(t, bytes::ByteSet::new(&[conv::byte(r1), conv::byte(r2)]).absent())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strcspn_c3(s: *const c_char, r1: c_int, r2: c_int, r3: c_int) -> size_t {
    // UNSAFE(slice) SAFETY: s is a string (§4.7, compat).
    ret::must(unsafe { strz::strz(s) }.map(|t| bytes::span(t, bytes::ByteSet::new(&[conv::byte(r1), conv::byte(r2), conv::byte(r3)]).absent())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strpbrk_c2(s: *const c_char, a1: c_int, a2: c_int) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s is a string (§4.7, compat).
    ret::found(s, unsafe { strz::strz(s) }.map(|t| bytes::find_any(t, bytes::ByteSet::new(&[conv::byte(a1), conv::byte(a2)]).predicate())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strpbrk_c3(s: *const c_char, a1: c_int, a2: c_int, a3: c_int) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s is a string (§4.7, compat).
    ret::found(s, unsafe { strz::strz(s) }.map(|t| bytes::find_any(t, bytes::ByteSet::new(&[conv::byte(a1), conv::byte(a2), conv::byte(a3)]).predicate())))
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn strcpy(d: *mut c_char, s: *const c_char) -> *mut c_char {
    // The direct entry (machine/scan.S, STRCPY-SHORT-2): short strings in one pass, the rest and
    // NULLs in `hot_strcpy_rest`. Proof obligation: this function's contract (§4.2).
    crate::ffi::liella_machine_strcpy(d.cast(), s.cast()).cast()
}}

export! { local "liella_hot_strcpy_rest";
/// `strcpy`'s general path, reached by name from the direct entry with nothing written: a NULL
/// `d` or `s` (reported, S6), a source whose first 64 bytes leave its page (`skip` 0) or hold no
/// NUL (`skip` 64: that scan is not repeated). `_room` is not set by that entry.
pub extern "C" fn hot_strcpy_rest(d: *mut c_char, s: *const c_char, _room: size_t, skip: size_t) -> *mut c_char {
    // UNSAFE(slice) SAFETY: the strcpy contract, passed on unchanged by the direct entry; skip
    // as documented (the scan of the entry, D-4.22).
    match unsafe { copy_str_rest(d, s, skip) } { Some(_) => d, None => ret::same(d, unsafe { strz::strz(s) }.and_then(|t| cstring::copy_z(t, unsafe { conv::dest(d) }))) }
}}

/// `strcpy`'s copy after the direct entry: `None` for a NULL `d` or `s`, else the text's length.
///
/// # Safety
/// As `copy_str`; when `d` and `s` are non-NULL, `skip` is 0 or 64 and `s`'s first `skip` bytes
/// hold no NUL.
// UNSAFE(slice): the caller's C string and destination go to the hot copy.
#[allow(unsafe_code)]
unsafe fn copy_str_rest(d: *mut c_char, s: *const c_char, skip: usize) -> Option<usize> {
    if d.is_null() || s.is_null() {
        return None;
    }
    // The safety chain for the skipped scan (thread rule RESULT CHECKS): asserted in the
    // selftest/debug build; the sweeps and planted bugs of D-4.22 cover the shipped one.
    // UNSAFE(slice) SAFETY: s is a string; at most skip bytes of it are viewed.
    debug_assert!((skip == 0 || skip == 64) && unsafe { strz::strz_n(s, skip) }.is_ok_and(|t| t.len() == skip));
    // UNSAFE(slice) SAFETY: s[..skip] holds no NUL, so s + skip is still inside the string.
    let len = unsafe { strz::strz(s.wrapping_add(skip)) }.ok()?.len().checked_add(skip)?;
    let total = len.checked_add(1)?;
    // UNSAFE(slice) SAFETY: s is readable for len + 1 bytes; d has room for them (contract).
    unsafe { hot::move_bytes(d.cast(), s.cast(), total) };
    Some(len)
}

export! { asm "GLIBC_2.3.4";
pub extern "C" fn __strcpy_chk(d: *mut c_char, s: *const c_char, destlen: size_t) -> *mut c_char {
    // The direct entry (machine/scan.S, STRCPY-FAMILY): room checked before any store; the rest,
    // NULLs and failed checks in `hot_strcpy_chk_rest`. Proof obligation: the fortify contract.
    crate::ffi::liella_machine_strcpy_chk(d.cast(), s.cast(), destlen).cast()
}}

export! { local "liella_hot_strcpy_chk_rest";
/// `__strcpy_chk`'s general path, reached by name from the direct entry with nothing written: a
/// NULL `d` or `s` (reported, S6), a source whose first 64 bytes leave its page, or a string that
/// does not fit `destlen` (fortify failure).
pub extern "C" fn hot_strcpy_chk_rest(d: *mut c_char, s: *const c_char, destlen: size_t) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s is a string, d has room for destlen bytes (fortify contract).
    match unsafe { copy_str_chk(d, s, destlen) } { Some(_) => d, None => ret::same(d, unsafe { strz::strz(s) }.and_then(|t| cstring::copy_z(t, unsafe { conv::dest_chk(d, destlen) }))) }
}}

/// The text and its terminator as one memmove-contract copy (D-4.22), `len + 1` checked against
/// `destlen` before any store; `None` for a NULL `d` or `s` (the caller reports it).
///
/// # Safety
/// `s` is NULL or a string; `d` is NULL or has room for `destlen` bytes (`usize::MAX`: unknown).
// UNSAFE(slice): the caller's C string and destination go to the hot copy.
#[allow(unsafe_code)]
unsafe fn copy_str_chk(d: *mut c_char, s: *const c_char, destlen: usize) -> Option<usize> {
    if d.is_null() || s.is_null() {
        return None;
    }
    // UNSAFE(slice) SAFETY: s is a string (§4.2); only its length is kept.
    let len = unsafe { strz::strz(s) }.ok()?.len();
    if len >= isize::MAX.unsigned_abs() {
        return None;
    }
    // UNSAFE(slice) SAFETY: s is valid for len + 1 bytes; d is, once conv::chk has checked that
    // total against destlen (writes beyond it fail the fortify check).
    unsafe { hot::move_bytes(d.cast(), s.cast(), conv::chk(len.wrapping_add(1), destlen)) };
    Some(len)
}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn stpcpy(d: *mut c_char, s: *const c_char) -> *mut c_char {
    // The direct entry (machine/scan.S, STRCPY-FAMILY). Proof obligation: the contract (§4.2).
    crate::ffi::liella_machine_stpcpy(d.cast(), s.cast()).cast()
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn __stpcpy(d: *mut c_char, s: *const c_char) -> *mut c_char {
    // The direct entry (machine/scan.S, STRCPY-FAMILY): the same code as stpcpy.
    crate::ffi::liella_machine_stpcpy(d.cast(), s.cast()).cast()
}}

export! { asm "GLIBC_2.3.4";
pub extern "C" fn __stpcpy_chk(d: *mut c_char, s: *const c_char, destlen: size_t) -> *mut c_char {
    // The direct entry (machine/scan.S, STRCPY-FAMILY): room checked before any store.
    // Proof obligation: the fortify contract.
    crate::ffi::liella_machine_stpcpy_chk(d.cast(), s.cast(), destlen).cast()
}}

export! { local "liella_hot_stpcpy_rest";
/// The stpcpy family's general path (`room` = `usize::MAX` for stpcpy/__stpcpy), reached by name
/// from the direct entry with nothing written: NULLs (S6), a page-leaving head, a failed check.
pub extern "C" fn hot_stpcpy_rest(d: *mut c_char, s: *const c_char, room: size_t) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s is a string, d has room for room bytes (§4.2, fortify contract).
    match unsafe { copy_str_chk(d, s, room) } { Some(len) => d.wrapping_add(len), None => ret::done(d, unsafe { strz::strz(s) }.and_then(|t| cstring::copy_z(t, unsafe { conv::dest_chk(d, room) }))) }
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn strncpy(d: *mut c_char, s: *const c_char, n: size_t) -> *mut c_char {
    // The direct entry (machine/scan.S, STRNCPY-SHORT): 1 <= n <= 64 in one pass, the rest and
    // NULLs in `hot_strncpy_rest`. Proof obligation: this function's contract (§4.2).
    crate::ffi::liella_machine_strncpy(d.cast(), s.cast(), n).cast()
}}

export! { local "liella_hot_strncpy_rest";
/// `strncpy`'s general path, reached by name from the direct entry with the arguments unchanged
/// and nothing written: a NULL `d` or `s` (reported, S6; with `n` 0 nothing is read or written),
/// `n` 0 or above 64, or a source whose first 64 bytes leave its page.
pub extern "C" fn hot_strncpy_rest(d: *mut c_char, s: *const c_char, n: size_t) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s is readable up to its terminator or n bytes, d has room for n bytes (§4.2).
    ret::same(d, unsafe { strz::strz_n(s, n) }.and_then(|t| cstring::copy_n_padded(t, n, unsafe { conv::dest(d) })))
}}

export! { asm "GLIBC_2.3.4";
pub extern "C" fn __strncpy_chk(d: *mut c_char, s: *const c_char, n: size_t, destlen: size_t) -> *mut c_char {
    // The direct entry (machine/scan.S, STRNCPY-FAMILY): n <= destlen checked before any store,
    // then the strncpy body; n > destlen and NULLs in `hot_strncpy_chk_rest`. Proof obligation:
    // this function's contract (fortify, §4.2).
    crate::ffi::liella_machine_strncpy_chk(d.cast(), s.cast(), n, destlen).cast()
}}

export! { local "liella_hot_strncpy_chk_rest";
/// `__strncpy_chk`'s general path, reached by name from the direct entry with the arguments
/// unchanged and nothing written: `n` above `destlen` (fails the fortify check) or a NULL `d` or `s`.
pub extern "C" fn hot_strncpy_chk_rest(d: *mut c_char, s: *const c_char, n: size_t, destlen: size_t) -> *mut c_char {
    // UNSAFE(slice) SAFETY: as strncpy; n beyond destlen fails the fortify check.
    ret::same(d, unsafe { strz::strz_n(s, n) }.and_then(|t| cstring::copy_n_padded(t, n, unsafe { conv::dest_chk(d, destlen) })))
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn stpncpy(d: *mut c_char, s: *const c_char, n: size_t) -> *mut c_char {
    // The direct entry (machine/scan.S, STRNCPY-FAMILY): the strncpy body returning the end;
    // the rest and NULLs in `hot_stpncpy_rest`. Proof obligation: this function's contract (§4.2).
    crate::ffi::liella_machine_stpncpy(d.cast(), s.cast(), n).cast()
}}

export! { local "liella_hot_stpncpy_rest";
/// `stpncpy`'s general path, reached by name from the direct entry with the arguments unchanged
/// and nothing written: a NULL `d` or `s`, `n` above 64, or a source whose first 64 bytes leave its page.
pub extern "C" fn hot_stpncpy_rest(d: *mut c_char, s: *const c_char, n: size_t) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s is readable up to its terminator or n bytes, d has room for n bytes (§4.2).
    ret::done(d, unsafe { strz::strz_n(s, n) }.and_then(|t| cstring::copy_n_padded(t, n, unsafe { conv::dest(d) })))
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn __stpncpy(d: *mut c_char, s: *const c_char, n: size_t) -> *mut c_char {
    // STPNCPY-ALIAS: in the products a second label on the direct stpncpy entry (machine/scan.S).
    // Proof obligation: this function's contract (§4.2), as stpncpy.
    crate::ffi::liella_machine_stpncpy(d.cast(), s.cast(), n).cast()
}}

export! { asm "GLIBC_2.4";
pub extern "C" fn __stpncpy_chk(d: *mut c_char, s: *const c_char, n: size_t, destlen: size_t) -> *mut c_char {
    // The direct entry (machine/scan.S, STPNCPY-ALIAS): n <= destlen checked before any store,
    // then the stpncpy body; n > destlen and NULLs in `hot_stpncpy_chk_rest`. Proof obligation:
    // this function's contract (fortify, §4.2).
    crate::ffi::liella_machine_stpncpy_chk(d.cast(), s.cast(), n, destlen).cast()
}}

export! { local "liella_hot_stpncpy_chk_rest";
/// `__stpncpy_chk`'s general path, reached by name from the direct entry with the arguments
/// unchanged and nothing written: `n` above `destlen` (fails the fortify check) or a NULL `d` or `s`.
pub extern "C" fn hot_stpncpy_chk_rest(d: *mut c_char, s: *const c_char, n: size_t, destlen: size_t) -> *mut c_char {
    // UNSAFE(slice) SAFETY: as stpncpy; n beyond destlen fails the fortify check.
    ret::done(d, unsafe { strz::strz_n(s, n) }.and_then(|t| cstring::copy_n_padded(t, n, unsafe { conv::dest_chk(d, destlen) })))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strcat(d: *mut c_char, s: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: d and s are strings, d has room for both (§4.9).
    ret::same(d, unsafe { strz::strz(d.cast_const()) }.map(<[u8]>::len).and_then(|k| unsafe { strz::strz(s) }.and_then(|t| cstring::cat_z(k, t, unsafe { conv::dest(d) }))))
}}

export! { "GLIBC_2.3.4";
pub extern "C" fn __strcat_chk(d: *mut c_char, s: *const c_char, destlen: size_t) -> *mut c_char {
    // UNSAFE(slice) SAFETY: as strcat; writes beyond destlen fail the fortify check.
    ret::same(d, unsafe { strz::strz(d.cast_const()) }.map(<[u8]>::len).and_then(|k| unsafe { strz::strz(s) }.and_then(|t| cstring::cat_z(k, t, unsafe { conv::dest_chk(d, destlen) }))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strncat(d: *mut c_char, s: *const c_char, n: size_t) -> *mut c_char {
    // UNSAFE(slice) SAFETY: d is a string, s readable up to its terminator or n bytes, d has room (§4.9).
    ret::same(d, unsafe { strz::strz(d.cast_const()) }.map(<[u8]>::len).and_then(|k| unsafe { strz::strz_n(s, n) }.and_then(|t| cstring::cat_z(k, t, unsafe { conv::dest(d) }))))
}}

export! { "GLIBC_2.3.4";
pub extern "C" fn __strncat_chk(d: *mut c_char, s: *const c_char, n: size_t, destlen: size_t) -> *mut c_char {
    // UNSAFE(slice) SAFETY: as strncat; writes beyond destlen fail the fortify check.
    ret::same(d, unsafe { strz::strz(d.cast_const()) }.map(<[u8]>::len).and_then(|k| unsafe { strz::strz_n(s, n) }.and_then(|t| cstring::cat_z(k, t, unsafe { conv::dest_chk(d, destlen) }))))
}}

export! { "GLIBC_2.38";
pub extern "C" fn strlcpy(d: *mut c_char, s: *const c_char, n: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: s is a string, d has room for n bytes (§4.2).
    ret::must(unsafe { strz::strz(s) }.and_then(|t| cstring::lcpy(t, n, unsafe { conv::dest(d) })))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __strlcpy_chk(d: *mut c_char, s: *const c_char, n: size_t, destlen: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: as strlcpy, after the fortify check of n against destlen.
    ret::must(unsafe { strz::strz(s) }.and_then(|t| cstring::lcpy(t, conv::chk(n, destlen), unsafe { conv::dest(d) })))
}}

export! { "GLIBC_2.38";
pub extern "C" fn strlcat(d: *mut c_char, s: *const c_char, n: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: d is readable up to its terminator or n bytes and has room for n bytes, s is a string (§4.9).
    ret::must(unsafe { strz::strz_n(d.cast_const(), n) }.map(<[u8]>::len).and_then(|k| unsafe { strz::strz(s) }.and_then(|t| cstring::lcat(k, t, n, unsafe { conv::dest(d) }))))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __strlcat_chk(d: *mut c_char, s: *const c_char, n: size_t, destlen: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: as strlcat, after the fortify check of n against destlen.
    ret::must(unsafe { strz::strz_n(d.cast_const(), conv::chk(n, destlen)) }.map(<[u8]>::len).and_then(|k| unsafe { strz::strz(s) }.and_then(|t| cstring::lcat(k, t, n, unsafe { conv::dest(d) }))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strdup(s: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s is a string (§4.10).
    ret::null(unsafe { strz::strz(s) }.and_then(caller_alloc::dup_z))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strdup(s: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s is a string (§4.10).
    ret::null(unsafe { strz::strz(s) }.and_then(caller_alloc::dup_z))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strndup(s: *const c_char, n: size_t) -> *mut c_char {
    // At most 0 bytes of `s` are read, so any `s` gives an empty copy (the reference; T11).
    // UNSAFE(slice) SAFETY: s is readable up to its terminator or n bytes (§4.10).
    ret::null(if n == 0 { Ok(&[][..]) } else { unsafe { strz::strz_n(s, n) } }.and_then(caller_alloc::dup_z))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strndup(s: *const c_char, n: size_t) -> *mut c_char {
    // As strndup.
    // UNSAFE(slice) SAFETY: s is readable up to its terminator or n bytes (§4.10).
    ret::null(if n == 0 { Ok(&[][..]) } else { unsafe { strz::strz_n(s, n) } }.and_then(caller_alloc::dup_z))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strfry(s: *mut c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s is a writable string (§4.12).
    ret::same(s, unsafe { strz::strz_mut(s) }.map(|t| bytes::shuffle(t, |b| random::uniform(&CSys, b).unwrap_or(0))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtok(s: *mut c_char, delim: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s (or the saved position) is a writable string, delim a string (§4.11).
    ret::ptr(ret::must(unsafe { strz::strz(delim) }.map(|u| string_compat::strtok(conv::addr(s), bytes::ByteSet::new(u).predicate(), string_compat::GlobalCursor, unsafe { conv::ctext() }))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtok_r(s: *mut c_char, delim: *const c_char, save: Option<&mut *mut c_char>) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s (or *save) is a writable string, delim a string, save a pointer variable (§4.11).
    ret::ptr(ret::must(unsafe { strz::strz(delim) }.and_then(|u| conv::cell(save).map(|c| string_compat::strtok(conv::addr(s), bytes::ByteSet::new(u).predicate(), c, unsafe { conv::ctext() })))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtok_r(s: *mut c_char, delim: *const c_char, save: Option<&mut *mut c_char>) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s (or *save) is a writable string, delim a string, save a pointer variable (§4.11).
    ret::ptr(ret::must(unsafe { strz::strz(delim) }.and_then(|u| conv::cell(save).map(|c| string_compat::strtok(conv::addr(s), bytes::ByteSet::new(u).predicate(), c, unsafe { conv::ctext() })))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtok_r_1c(s: *mut c_char, sep: c_char, save: Option<&mut *mut c_char>) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s (or *save) is a writable string, save a pointer variable (§4.11, compat).
    ret::ptr(ret::must(conv::cell(save).map(|c| string_compat::strtok(conv::addr(s), bytes::ByteSet::new(&[conv::byte_c(sep)]).predicate(), c, unsafe { conv::ctext() }))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strsep(sp: Option<&mut *mut c_char>, delim: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: sp is a pointer variable holding NULL or a writable string, delim a string (§4.11).
    ret::ptr(ret::must(unsafe { strz::strz(delim) }.and_then(|u| conv::cell(sp).map(|c| string_compat::strsep(c, bytes::ByteSet::new(u).predicate(), unsafe { conv::ctext() })))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strsep_g(sp: Option<&mut *mut c_char>, delim: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: sp is a pointer variable holding NULL or a writable string, delim a string (§4.11).
    ret::ptr(ret::must(unsafe { strz::strz(delim) }.and_then(|u| conv::cell(sp).map(|c| string_compat::strsep(c, bytes::ByteSet::new(u).predicate(), unsafe { conv::ctext() })))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strsep_1c(sp: Option<&mut *mut c_char>, r: c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: sp is a pointer variable holding NULL or a writable string (§4.11, compat).
    ret::ptr(ret::must(conv::cell(sp).map(|c| string_compat::strsep(c, bytes::ByteSet::new(&[conv::byte_c(r)]).predicate(), unsafe { conv::ctext() }))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strsep_2c(sp: Option<&mut *mut c_char>, r1: c_char, r2: c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: sp is a pointer variable holding NULL or a writable string (§4.11, compat).
    ret::ptr(ret::must(conv::cell(sp).map(|c| string_compat::strsep(c, bytes::ByteSet::new(&[conv::byte_c(r1), conv::byte_c(r2)]).predicate(), unsafe { conv::ctext() }))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strsep_3c(sp: Option<&mut *mut c_char>, r1: c_char, r2: c_char, r3: c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: sp is a pointer variable holding NULL or a writable string (§4.11, compat).
    ret::ptr(ret::must(conv::cell(sp).map(|c| string_compat::strsep(c, bytes::ByteSet::new(&[conv::byte_c(r1), conv::byte_c(r2), conv::byte_c(r3)]).predicate(), unsafe { conv::ctext() }))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn basename(path: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: path is a string (§4.12, GNU form).
    ret::at(path, unsafe { strz::strz(path) }.map(bytes::basename_gnu))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __xpg_basename(path: *mut c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: path is NULL or a writable string (§4.12, XPG form).
    ret::path_part(path, unsafe { strz::strz_mut_or_empty(path) }.map(bytes::basename_xpg))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn dirname(path: *mut c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: path is NULL or a writable string (§4.12).
    ret::path_part(path, unsafe { strz::strz_mut_or_empty(path) }.map(bytes::dirname))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn argz_create(argv: *const *mut c_char, argz: Option<&mut *mut c_char>, len: Option<&mut size_t>) -> c_int {
    // UNSAFE(slice) SAFETY: argv is a NULL-terminated array of strings; argz and len are output variables (§4.13).
    ret::errnum(unsafe { conv::argv(argv) }.and_then(|v| cstring::argz_from_list(v.into_iter())).and_then(|b| conv::argz_out(argz, len).and_then(|o| o.set(&b))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn argz_create_sep(s: *const c_char, sep: c_int, argz: Option<&mut *mut c_char>, len: Option<&mut size_t>) -> c_int {
    // UNSAFE(slice) SAFETY: s is a string; argz and len are output variables (§4.13).
    ret::errnum(unsafe { strz::strz(s) }.and_then(|t| cstring::argz_from_sep(t, conv::byte(sep))).and_then(|b| conv::argz_out(argz, len).and_then(|o| o.set(&b))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn argz_count(argz: *const c_char, len: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: argz is readable for len bytes (§4.13).
    ret::must(unsafe { conv::buf(argz.cast(), len) }.map(cstring::argz_count))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __argz_count(argz: *const c_char, len: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: argz is readable for len bytes (§4.13).
    ret::must(unsafe { conv::buf(argz.cast(), len) }.map(cstring::argz_count))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn argz_extract(argz: *const c_char, len: size_t, argv: *mut *mut c_char) {
    // UNSAFE(slice) SAFETY: argz is readable for len bytes; argv has room for count + 1 pointers (§4.13).
    ret::unit(unsafe { conv::buf(argz.cast(), len) }.and_then(|b| cstring::argz_extract(b, conv::addr(argz), unsafe { conv::ptrs_mut(argv) })))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn argz_stringify(argz: *mut c_char, len: size_t, sep: c_int) {
    // UNSAFE(slice) SAFETY: argz is writable for len bytes (§4.13).
    ret::unit(unsafe { conv::buf_mut(argz.cast(), len) }.map(|b| cstring::argz_stringify(b, conv::byte(sep))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __argz_stringify(argz: *mut c_char, len: size_t, sep: c_int) {
    // UNSAFE(slice) SAFETY: argz is writable for len bytes (§4.13).
    ret::unit(unsafe { conv::buf_mut(argz.cast(), len) }.map(|b| cstring::argz_stringify(b, conv::byte(sep))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn argz_add(argz: Option<&mut *mut c_char>, len: Option<&mut size_t>, s: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: *argz/*len describe an argz vector; s is a string (§4.13).
    ret::errnum(unsafe { strz::strz(s) }.and_then(|t| unsafe { conv::argz(argz, len) }.and_then(|a| a.update(|b, _| cstring::argz_add(b, t)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn argz_add_sep(argz: Option<&mut *mut c_char>, len: Option<&mut size_t>, s: *const c_char, sep: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: *argz/*len describe an argz vector; s is a string (§4.13).
    ret::errnum(unsafe { strz::strz(s) }.and_then(|t| unsafe { conv::argz(argz, len) }.and_then(|a| a.update(|b, _| cstring::argz_add_sep(b, t, conv::byte(sep))))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn argz_append(argz: Option<&mut *mut c_char>, len: Option<&mut size_t>, more: *const c_char, more_len: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: *argz/*len describe an argz vector; more is readable for more_len bytes (§4.13).
    ret::errnum(unsafe { conv::buf(more.cast(), more_len) }.and_then(|m| unsafe { conv::argz(argz, len) }.and_then(|a| a.update(|b, _| cstring::argz_concat(b, m)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn argz_delete(argz: Option<&mut *mut c_char>, len: Option<&mut size_t>, entry: *mut c_char) {
    // UNSAFE(slice) SAFETY: *argz/*len describe an argz vector (§4.13).
    ret::unit(unsafe { conv::argz(argz, len) }.and_then(|a| a.update(|b, base| cstring::argz_delete_addr(b, base, conv::addr(entry)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn argz_insert(argz: Option<&mut *mut c_char>, len: Option<&mut size_t>, before: *mut c_char, entry: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: *argz/*len describe an argz vector; entry is a string (§4.13).
    ret::errnum(unsafe { strz::strz(entry) }.and_then(|t| unsafe { conv::argz(argz, len) }.and_then(|a| a.update(|b, base| cstring::argz_insert_addr(b, base, conv::addr(before), t)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn argz_next(argz: *const c_char, len: size_t, entry: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: argz is readable for len bytes (§4.13).
    ret::found(argz, unsafe { conv::buf(argz.cast(), len) }.map(|b| cstring::argz_next_addr(b, conv::addr(argz), conv::addr(entry))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __argz_next(argz: *const c_char, len: size_t, entry: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: argz is readable for len bytes (§4.13).
    ret::found(argz, unsafe { conv::buf(argz.cast(), len) }.map(|b| cstring::argz_next_addr(b, conv::addr(argz), conv::addr(entry))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn argz_replace(argz: Option<&mut *mut c_char>, len: Option<&mut size_t>, from: *const c_char, to: *const c_char, count: Option<&mut c_uint>) -> c_int {
    // UNSAFE(slice) SAFETY: *argz/*len describe an argz vector; from and to are strings; count is NULL or a variable (§4.13).
    ret::errnum(unsafe { strz::strz(from) }.and_then(|f| unsafe { strz::strz(to) }.and_then(|t| unsafe { conv::argz(argz, len) }.and_then(|a| a.update(|b, _| cstring::argz_replace_counted(b, f, t, count))))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn envz_entry(envz: *const c_char, len: size_t, name: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: envz is readable for len bytes, name is a string (§4.14).
    ret::found(envz, unsafe { conv::buf(envz.cast(), len) }.and_then(|b| unsafe { strz::strz(name) }.map(|n| cstring::envz_entry(b, n))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn envz_get(envz: *const c_char, len: size_t, name: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: envz is readable for len bytes, name is a string (§4.14).
    ret::found(envz, unsafe { conv::buf(envz.cast(), len) }.and_then(|b| unsafe { strz::strz(name) }.map(|n| cstring::envz_get(b, n))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn envz_add(envz: Option<&mut *mut c_char>, len: Option<&mut size_t>, name: *const c_char, value: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: *envz/*len describe an envz vector; name is a string, value NULL or a string (§4.14).
    ret::errnum(unsafe { strz::strz(name) }.and_then(|n| unsafe { strz::strz_opt(value) }.and_then(|v| unsafe { conv::argz(envz, len) }.and_then(|a| a.update(|b, _| cstring::envz_add(b, n, v))))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn envz_merge(envz: Option<&mut *mut c_char>, len: Option<&mut size_t>, other: *const c_char, other_len: size_t, override_: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: *envz/*len describe an envz vector; other is readable for other_len bytes (§4.14).
    ret::errnum(unsafe { conv::buf(other.cast(), other_len) }.and_then(|o| unsafe { conv::argz(envz, len) }.and_then(|a| a.update(|b, _| cstring::envz_merge(b, o, conv::flag(override_))))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn envz_remove(envz: Option<&mut *mut c_char>, len: Option<&mut size_t>, name: *const c_char) {
    // UNSAFE(slice) SAFETY: *envz/*len describe an envz vector; name is a string (§4.14).
    ret::unit(unsafe { strz::strz(name) }.and_then(|n| unsafe { conv::argz(envz, len) }.and_then(|a| a.update(|b, _| cstring::envz_remove(b, n)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn envz_strip(envz: Option<&mut *mut c_char>, len: Option<&mut size_t>) {
    // UNSAFE(slice) SAFETY: *envz/*len describe an envz vector (§4.14).
    ret::unit(unsafe { conv::argz(envz, len) }.and_then(|a| a.update(|b, _| cstring::envz_strip(b))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcslen(s: *const wchar_t) -> size_t {
    // UNSAFE(slice) SAFETY: s is a wide string (§4.8).
    ret::must(unsafe { strz::wcsz(s) }.map(<[i32]>::len))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcsnlen(s: *const wchar_t, n: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: s is readable up to its terminator or n units (§4.8).
    ret::must(unsafe { strz::wcsz_n(s, n) }.map(<[i32]>::len))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcscmp(a: *const wchar_t, b: *const wchar_t) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are wide strings (§4.3).
    ret::order(unsafe { strz::wcsz(a) }.and_then(|x| unsafe { strz::wcsz(b) }.map(|y| bytes::compare_text(x, y))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcscoll(a: *const wchar_t, b: *const wchar_t) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are wide strings (§4.3).
    ret::order(unsafe { strz::wcsz(a) }.and_then(|x| unsafe { strz::wcsz(b) }.map(|y| locale::current().compare_wide(x, y))))
}}

export! { "GLIBC_2.3";
pub extern "C" fn wcscoll_l(a: *const wchar_t, b: *const wchar_t, loc: locale_t) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are wide strings (§4.3).
    ret::order(unsafe { strz::wcsz(a) }.and_then(|x| unsafe { strz::wcsz(b) }.map(|y| locale::of(conv::addr(loc)).compare_wide(x, y))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcscoll_l(a: *const wchar_t, b: *const wchar_t, loc: locale_t) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are wide strings (§4.3).
    ret::order(unsafe { strz::wcsz(a) }.and_then(|x| unsafe { strz::wcsz(b) }.map(|y| locale::of(conv::addr(loc)).compare_wide(x, y))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcsncmp(a: *const wchar_t, b: *const wchar_t, n: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are readable up to their terminators or n units (§4.3).
    ret::order(unsafe { strz::wcsz_n(a, n) }.and_then(|x| unsafe { strz::wcsz_n(b, n) }.map(|y| bytes::compare_text(x, y))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcscasecmp(a: *const wchar_t, b: *const wchar_t) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are wide strings (§4.3).
    ret::order(unsafe { strz::wcsz(a) }.and_then(|x| unsafe { strz::wcsz(b) }.map(|y| bytes::compare_text_by(x, y, locale::wlower(locale::current())))))
}}

export! { "GLIBC_2.3";
pub extern "C" fn wcscasecmp_l(a: *const wchar_t, b: *const wchar_t, loc: locale_t) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are wide strings (§4.3).
    ret::order(unsafe { strz::wcsz(a) }.and_then(|x| unsafe { strz::wcsz(b) }.map(|y| bytes::compare_text_by(x, y, locale::wlower(locale::of(conv::addr(loc)))))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcscasecmp_l(a: *const wchar_t, b: *const wchar_t, loc: locale_t) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are wide strings (§4.3).
    ret::order(unsafe { strz::wcsz(a) }.and_then(|x| unsafe { strz::wcsz(b) }.map(|y| bytes::compare_text_by(x, y, locale::wlower(locale::of(conv::addr(loc)))))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcsncasecmp(a: *const wchar_t, b: *const wchar_t, n: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are readable up to their terminators or n units (§4.3).
    ret::order(unsafe { strz::wcsz_n(a, n) }.and_then(|x| unsafe { strz::wcsz_n(b, n) }.map(|y| bytes::compare_text_by(x, y, locale::wlower(locale::current())))))
}}

export! { "GLIBC_2.3";
pub extern "C" fn wcsncasecmp_l(a: *const wchar_t, b: *const wchar_t, n: size_t, loc: locale_t) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are readable up to their terminators or n units (§4.3).
    ret::order(unsafe { strz::wcsz_n(a, n) }.and_then(|x| unsafe { strz::wcsz_n(b, n) }.map(|y| bytes::compare_text_by(x, y, locale::wlower(locale::of(conv::addr(loc)))))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcsncasecmp_l(a: *const wchar_t, b: *const wchar_t, n: size_t, loc: locale_t) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are readable up to their terminators or n units (§4.3).
    ret::order(unsafe { strz::wcsz_n(a, n) }.and_then(|x| unsafe { strz::wcsz_n(b, n) }.map(|y| bytes::compare_text_by(x, y, locale::wlower(locale::of(conv::addr(loc)))))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcsxfrm(d: *mut wchar_t, s: *const wchar_t, n: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: s is a wide string, d has room for n units (§4.4).
    ret::must(unsafe { strz::wcsz(s) }.and_then(|t| cstring::xfrm_fill(n, unsafe { conv::dest(d) }, |o| locale::current().xfrm_wide_into(t, o), || locale::current().xfrm_wide(t))))
}}

export! { "GLIBC_2.3";
pub extern "C" fn wcsxfrm_l(d: *mut wchar_t, s: *const wchar_t, n: size_t, loc: locale_t) -> size_t {
    // UNSAFE(slice) SAFETY: s is a wide string, d has room for n units (§4.4).
    ret::must(unsafe { strz::wcsz(s) }.and_then(|t| cstring::xfrm_fill(n, unsafe { conv::dest(d) }, |o| locale::of(conv::addr(loc)).xfrm_wide_into(t, o), || locale::of(conv::addr(loc)).xfrm_wide(t))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcsxfrm_l(d: *mut wchar_t, s: *const wchar_t, n: size_t, loc: locale_t) -> size_t {
    // UNSAFE(slice) SAFETY: s is a wide string, d has room for n units (§4.4).
    ret::must(unsafe { strz::wcsz(s) }.and_then(|t| cstring::xfrm_fill(n, unsafe { conv::dest(d) }, |o| locale::of(conv::addr(loc)).xfrm_wide_into(t, o), || locale::of(conv::addr(loc)).xfrm_wide(t))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcschr(s: *const wchar_t, c: wchar_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: s is a wide string (§4.5).
    ret::found(s, unsafe { strz::wcsz(s) }.map(|t| bytes::find_z(t, c)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcsrchr(s: *const wchar_t, c: wchar_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: s is a wide string (§4.5).
    ret::found(s, unsafe { strz::wcsz(s) }.map(|t| bytes::rfind_z(t, c)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcschrnul(s: *const wchar_t, c: wchar_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: s is a wide string (§4.5).
    ret::at(s, unsafe { strz::wcsz(s) }.map(|t| bytes::find_z_or_end(t, c)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcsstr(h: *const wchar_t, n: *const wchar_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: h and n are wide strings (§4.6). The needle is measured; the haystack
    // is read by the fused unit scan (`strz::find_wcs`, STRSTR-VEC).
    ret::found(h, unsafe { strz::wcsz(n) }.and_then(|y| unsafe { strz::find_wcs(h, y) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcswcs(h: *const wchar_t, n: *const wchar_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: h and n are wide strings (§4.6). The needle is measured; the haystack
    // is read by the fused unit scan (`strz::find_wcs`, STRSTR-VEC).
    ret::found(h, unsafe { strz::wcsz(n) }.and_then(|y| unsafe { strz::find_wcs(h, y) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcsspn(s: *const wchar_t, set: *const wchar_t) -> size_t {
    // UNSAFE(slice) SAFETY: s and set are wide strings (§4.7).
    ret::must(unsafe { strz::wcsz(s) }.and_then(|t| unsafe { strz::wcsz(set) }.map(|u| bytes::span(t, bytes::member_of(u)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcscspn(s: *const wchar_t, set: *const wchar_t) -> size_t {
    // UNSAFE(slice) SAFETY: s and set are wide strings (§4.7).
    ret::must(unsafe { strz::wcsz(s) }.and_then(|t| unsafe { strz::wcsz(set) }.map(|u| bytes::span(t, bytes::not_member_of(u)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcspbrk(s: *const wchar_t, set: *const wchar_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: s and set are wide strings (§4.7).
    ret::found(s, unsafe { strz::wcsz(s) }.and_then(|t| unsafe { strz::wcsz(set) }.map(|u| bytes::find_any(t, bytes::member_of(u)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcscpy(d: *mut wchar_t, s: *const wchar_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: s is a wide string, d has room for it and its terminator (§4.2).
    ret::same(d, unsafe { strz::wcsz(s) }.and_then(|t| cstring::copy_z(t, unsafe { conv::dest(d) })))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __wcscpy_chk(d: *mut wchar_t, s: *const wchar_t, destlen: size_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: as wcscpy; destlen counts wchar_t units (bits/wchar2.h).
    ret::same(d, unsafe { strz::wcsz(s) }.and_then(|t| cstring::copy_z(t, unsafe { conv::dest_chk(d, destlen) })))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcpcpy(d: *mut wchar_t, s: *const wchar_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: s is a wide string, d has room for it and its terminator (§4.2).
    ret::done(d, unsafe { strz::wcsz(s) }.and_then(|t| cstring::copy_z(t, unsafe { conv::dest(d) })))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __wcpcpy_chk(d: *mut wchar_t, s: *const wchar_t, destlen: size_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: as wcpcpy; destlen counts wchar_t units.
    ret::done(d, unsafe { strz::wcsz(s) }.and_then(|t| cstring::copy_z(t, unsafe { conv::dest_chk(d, destlen) })))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcsncpy(d: *mut wchar_t, s: *const wchar_t, n: size_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: s is readable up to its terminator or n units, d has room for n (§4.2).
    ret::same(d, unsafe { strz::wcsz_n(s, n) }.and_then(|t| cstring::copy_n_padded(t, n, unsafe { conv::dest(d) })))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __wcsncpy_chk(d: *mut wchar_t, s: *const wchar_t, n: size_t, destlen: size_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: as wcsncpy; destlen counts wchar_t units.
    ret::same(d, unsafe { strz::wcsz_n(s, n) }.and_then(|t| cstring::copy_n_padded(t, n, unsafe { conv::dest_chk(d, destlen) })))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcpncpy(d: *mut wchar_t, s: *const wchar_t, n: size_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: s is readable up to its terminator or n units, d has room for n (§4.2).
    ret::done(d, unsafe { strz::wcsz_n(s, n) }.and_then(|t| cstring::copy_n_padded(t, n, unsafe { conv::dest(d) })))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __wcpncpy_chk(d: *mut wchar_t, s: *const wchar_t, n: size_t, destlen: size_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: as wcpncpy; destlen counts wchar_t units.
    ret::done(d, unsafe { strz::wcsz_n(s, n) }.and_then(|t| cstring::copy_n_padded(t, n, unsafe { conv::dest_chk(d, destlen) })))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcscat(d: *mut wchar_t, s: *const wchar_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: d and s are wide strings, d has room for both (§4.9).
    ret::same(d, unsafe { strz::wcsz(d.cast_const()) }.map(<[i32]>::len).and_then(|k| unsafe { strz::wcsz(s) }.and_then(|t| cstring::cat_z(k, t, unsafe { conv::dest(d) }))))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __wcscat_chk(d: *mut wchar_t, s: *const wchar_t, destlen: size_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: as wcscat; destlen counts wchar_t units.
    ret::same(d, unsafe { strz::wcsz(d.cast_const()) }.map(<[i32]>::len).and_then(|k| unsafe { strz::wcsz(s) }.and_then(|t| cstring::cat_z(k, t, unsafe { conv::dest_chk(d, destlen) }))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcsncat(d: *mut wchar_t, s: *const wchar_t, n: size_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: d is a wide string, s readable up to its terminator or n units, d has room (§4.9).
    ret::same(d, unsafe { strz::wcsz(d.cast_const()) }.map(<[i32]>::len).and_then(|k| unsafe { strz::wcsz_n(s, n) }.and_then(|t| cstring::cat_z(k, t, unsafe { conv::dest(d) }))))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __wcsncat_chk(d: *mut wchar_t, s: *const wchar_t, n: size_t, destlen: size_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: as wcsncat; destlen counts wchar_t units.
    ret::same(d, unsafe { strz::wcsz(d.cast_const()) }.map(<[i32]>::len).and_then(|k| unsafe { strz::wcsz_n(s, n) }.and_then(|t| cstring::cat_z(k, t, unsafe { conv::dest_chk(d, destlen) }))))
}}

export! { "GLIBC_2.38";
pub extern "C" fn wcslcpy(d: *mut wchar_t, s: *const wchar_t, n: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: s is a wide string, d has room for n units (§4.2).
    ret::must(unsafe { strz::wcsz(s) }.and_then(|t| cstring::lcpy(t, n, unsafe { conv::dest(d) })))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __wcslcpy_chk(d: *mut wchar_t, s: *const wchar_t, n: size_t, destlen: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: as wcslcpy, after the fortify check (destlen in wchar_t units).
    ret::must(unsafe { strz::wcsz(s) }.and_then(|t| cstring::lcpy(t, conv::chk(n, destlen), unsafe { conv::dest(d) })))
}}

export! { "GLIBC_2.38";
pub extern "C" fn wcslcat(d: *mut wchar_t, s: *const wchar_t, n: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: d is readable up to its terminator or n units and has room for n, s a wide string (§4.9).
    ret::must(unsafe { strz::wcsz_n(d.cast_const(), n) }.map(<[i32]>::len).and_then(|k| unsafe { strz::wcsz(s) }.and_then(|t| cstring::lcat(k, t, n, unsafe { conv::dest(d) }))))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __wcslcat_chk(d: *mut wchar_t, s: *const wchar_t, n: size_t, destlen: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: as wcslcat, after the fortify check (destlen in wchar_t units).
    ret::must(unsafe { strz::wcsz_n(d.cast_const(), conv::chk(n, destlen)) }.map(<[i32]>::len).and_then(|k| unsafe { strz::wcsz(s) }.and_then(|t| cstring::lcat(k, t, n, unsafe { conv::dest(d) }))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcsdup(s: *const wchar_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: s is a wide string (§4.10).
    ret::null(unsafe { strz::wcsz(s) }.and_then(caller_alloc::dup_z))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstok(s: *mut wchar_t, delim: *const wchar_t, save: Option<&mut *mut wchar_t>) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: s (or *save) is a writable wide string, delim a wide string, save a pointer variable (§4.11).
    ret::ptr(ret::must(unsafe { strz::wcsz(delim) }.and_then(|u| conv::cell(save).map(|c| string_compat::strtok(conv::addr(s), bytes::member_of(u), c, unsafe { conv::ctext() })))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wmemchr(s: *const wchar_t, c: wchar_t, n: size_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: s is readable for n units (§4.5).
    ret::found(s, unsafe { conv::buf_w(s, n) }.map(|b| bytes::find(b, c)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wmemcmp(a: *const wchar_t, b: *const wchar_t, n: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are readable for n units (§4.3).
    ret::order(unsafe { conv::buf_w(a, n) }.and_then(|x| unsafe { conv::buf_w(b, n) }.map(|y| bytes::compare(x, y))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wmemcpy(d: *mut wchar_t, s: *const wchar_t, n: size_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: d and s are valid for n units (§4.2).
    ret::same(d, unsafe { conv::move_units::<i32>(d.cast(), s.cast(), n) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wmemmove(d: *mut wchar_t, s: *const wchar_t, n: size_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: d and s are valid for n units (§4.2).
    ret::same(d, unsafe { conv::move_units::<i32>(d.cast(), s.cast(), n) })
}}

export! { "GLIBC_2.4";
pub extern "C" fn __wmemcpy_chk(d: *mut wchar_t, s: *const wchar_t, n: size_t, destlen: size_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: as wmemmove, after the fortify check (destlen in wchar_t units).
    ret::same(d, unsafe { conv::move_units::<i32>(d.cast(), s.cast(), conv::chk(n, destlen)) })
}}

export! { "GLIBC_2.4";
pub extern "C" fn __wmemmove_chk(d: *mut wchar_t, s: *const wchar_t, n: size_t, destlen: size_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: as wmemmove, after the fortify check (destlen in wchar_t units).
    ret::same(d, unsafe { conv::move_units::<i32>(d.cast(), s.cast(), conv::chk(n, destlen)) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wmempcpy(d: *mut wchar_t, s: *const wchar_t, n: size_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: d and s are valid for n units (§4.2).
    ret::done(d, unsafe { conv::move_units::<i32>(d.cast(), s.cast(), n) })
}}

export! { "GLIBC_2.4";
pub extern "C" fn __wmempcpy_chk(d: *mut wchar_t, s: *const wchar_t, n: size_t, destlen: size_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: as wmempcpy, after the fortify check (destlen in wchar_t units).
    ret::done(d, unsafe { conv::move_units::<i32>(d.cast(), s.cast(), conv::chk(n, destlen)) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wmemset(d: *mut wchar_t, c: wchar_t, n: size_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: d is writable for n units (§4.2).
    ret::same(d, unsafe { conv::buf_w_mut(d, n) }.map(|b| bytes::fill(b, c)))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __wmemset_chk(d: *mut wchar_t, c: wchar_t, n: size_t, destlen: size_t) -> *mut wchar_t {
    // UNSAFE(slice) SAFETY: as wmemset, after the fortify check (destlen in wchar_t units).
    ret::same(d, unsafe { conv::buf_w_mut(d, conv::chk(n, destlen)) }.map(|b| bytes::fill(b, c)))
}}
