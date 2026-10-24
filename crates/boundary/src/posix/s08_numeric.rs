//! Translation for spec §8: numeric conversion and integer arithmetic.
//!
//! `long double` and `_Float128` cross the C ABI in registers Rust cannot name, so their
//! exports are single-statement C forwarders (`csys/src/adapter/float.c`, OD-C4) around the
//! `liella_*_bits` functions here, which take and return the values as bit structs
//! (`LdBits`, `F128Bits`). Those carry the private version node `LIELLA_PRIVATE` (not
//! exported from the product, phase 4).
//!
//! Conversions use the current rounding mode (MXCSR). The radix is `.` and there is no
//! grouping: every locale is the C locale until phase 7 (D-6.7). The `ecvt`/`fcvt`/`l64a`
//! buffers are process-wide until the phase-4 thread blocks.

use liella::num::{self, Float, Format, StdBit};
use liella::num::parse::Dialect;
use liella_abi::ctypes::{c_char, c_int, c_long, c_uchar, c_uint, c_ulong, c_ulonglong, c_ushort, locale_t, size_t, wchar_t, F128Bits, LdBits};
use liella_abi::layout::{div_t, imaxdiv_t, ldiv_t, lldiv_t};
use liella_abi::pack;
use liella_kernel::machine::Machine;
use liella_legacy::num_compat;

use crate::conv;
use crate::locale;
use crate::export;
use crate::ret;
use crate::strz;
use crate::sys::CSys;

/// Bytes the `strtol` fast path looks at first: a sign, 18 digits and the terminator.
const STRTOL_HEAD: usize = 20;

/// The narrow signed `strtol` family (§8.1): the common shapes from the first
/// [`STRTOL_HEAD`] bytes without measuring the text (STRTOIMAX), the rest by the full parse.
// UNSAFE(slice): the export's C string is read through `strz`.
#[allow(unsafe_code)]
fn strtol_narrow(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, dialect: Dialect) -> c_long {
    // UNSAFE(slice) SAFETY: s is a string; at most STRTOL_HEAD bytes of it are read (§8.1).
    let head = ret::must(unsafe { strz::strz_n(s, STRTOL_HEAD) });
    let mut e = e;
    if let Some(v) = num_compat::strtol_short(head, conv::addr(s), conv::end(e.as_deref_mut()), base, head.len() < STRTOL_HEAD) {
        return v;
    }
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, dialect))
}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtol(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_long {
    strtol_narrow(s, e, base, Dialect::C99)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtoll(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_long {
    strtol_narrow(s, e, base, Dialect::C99)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtoq(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_long {
    strtol_narrow(s, e, base, Dialect::C99)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtoimax(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_long {
    strtol_narrow(s, e, base, Dialect::C99)
}}

export! { "GLIBC_2.3";
pub extern "C" fn strtol_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_long {
    strtol_narrow(s, e, base, Dialect::C99)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtol_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_long {
    strtol_narrow(s, e, base, Dialect::C99)
}}

export! { "GLIBC_2.3.3";
pub extern "C" fn strtoll_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_long {
    strtol_narrow(s, e, base, Dialect::C99)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtoll_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_long {
    strtol_narrow(s, e, base, Dialect::C99)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtol_internal(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _group: c_int) -> c_long {
    strtol_narrow(s, e, base, Dialect::C99)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtoll_internal(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _group: c_int) -> c_long {
    strtol_narrow(s, e, base, Dialect::C99)
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_strtol(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_long {
    strtol_narrow(s, e, base, Dialect::C23)
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_strtoll(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_long {
    strtol_narrow(s, e, base, Dialect::C23)
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_strtoimax(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_long {
    strtol_narrow(s, e, base, Dialect::C23)
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_strtol_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_long {
    strtol_narrow(s, e, base, Dialect::C23)
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_strtoll_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_long {
    strtol_narrow(s, e, base, Dialect::C23)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtoul(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtoull(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtouq(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtoumax(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.3";
pub extern "C" fn strtoul_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtoul_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.3.3";
pub extern "C" fn strtoull_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtoull_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtoul_internal(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _group: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtoull_internal(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _group: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_strtoul(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_strtoull(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_strtoumax(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_strtoul_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_strtoull_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstol(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstoll(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstoq(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstoimax(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.3";
pub extern "C" fn wcstol_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstol_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.3";
pub extern "C" fn wcstoll_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstoll_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstol_internal(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _group: c_int) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstoll_internal(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _group: c_int) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_wcstol(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_wcstoll(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_wcstoimax(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_wcstol_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_wcstoll_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstoul(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstoull(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstouq(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstoumax(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.3";
pub extern "C" fn wcstoul_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstoul_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.3";
pub extern "C" fn wcstoull_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstoull_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstoul_internal(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _group: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstoull_internal(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _group: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_wcstoul(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_wcstoull(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_wcstoumax(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_wcstoul_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_wcstoull_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn atoi(s: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: s is a string (§8.1).
    num_compat::atoi(ret::must(unsafe { strz::strz(s) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn atol(s: *const c_char) -> c_long {
    // UNSAFE(slice) SAFETY: s is a string (§8.1).
    num_compat::atol(ret::must(unsafe { strz::strz(s) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn atoll(s: *const c_char) -> c_long {
    // UNSAFE(slice) SAFETY: s is a string (§8.1).
    num_compat::atol(ret::must(unsafe { strz::strz(s) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn atof(s: *const c_char) -> f64 {
    // UNSAFE(slice) SAFETY: s is a string (§8.2).
    num_compat::atof(ret::must(unsafe { strz::strz(s) }), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::current()))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtod(s: *const c_char, e: Option<&mut *mut c_char>) -> f64 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::current())))
}}

export! { "GLIBC_2.27";
pub extern "C" fn strtof64(s: *const c_char, e: Option<&mut *mut c_char>) -> f64 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::current())))
}}

export! { "GLIBC_2.27";
pub extern "C" fn strtof32x(s: *const c_char, e: Option<&mut *mut c_char>) -> f64 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::current())))
}}

export! { "GLIBC_2.3";
pub extern "C" fn strtod_l(s: *const c_char, e: Option<&mut *mut c_char>, loc: locale_t) -> f64 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtod_l(s: *const c_char, e: Option<&mut *mut c_char>, loc: locale_t) -> f64 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.27";
pub extern "C" fn strtof64_l(s: *const c_char, e: Option<&mut *mut c_char>, loc: locale_t) -> f64 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.27";
pub extern "C" fn strtof32x_l(s: *const c_char, e: Option<&mut *mut c_char>, loc: locale_t) -> f64 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtod_internal(s: *const c_char, e: Option<&mut *mut c_char>, _group: c_int) -> f64 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::current())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtof(s: *const c_char, e: Option<&mut *mut c_char>) -> f32 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::current())))
}}

export! { "GLIBC_2.27";
pub extern "C" fn strtof32(s: *const c_char, e: Option<&mut *mut c_char>) -> f32 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::current())))
}}

export! { "GLIBC_2.3";
pub extern "C" fn strtof_l(s: *const c_char, e: Option<&mut *mut c_char>, loc: locale_t) -> f32 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtof_l(s: *const c_char, e: Option<&mut *mut c_char>, loc: locale_t) -> f32 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.27";
pub extern "C" fn strtof32_l(s: *const c_char, e: Option<&mut *mut c_char>, loc: locale_t) -> f32 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtof_internal(s: *const c_char, e: Option<&mut *mut c_char>, _group: c_int) -> f32 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::current())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstod(s: *const wchar_t, e: Option<&mut *mut wchar_t>) -> f64 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::current())))
}}

export! { "GLIBC_2.27";
pub extern "C" fn wcstof64(s: *const wchar_t, e: Option<&mut *mut wchar_t>) -> f64 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::current())))
}}

export! { "GLIBC_2.27";
pub extern "C" fn wcstof32x(s: *const wchar_t, e: Option<&mut *mut wchar_t>) -> f64 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::current())))
}}

export! { "GLIBC_2.3";
pub extern "C" fn wcstod_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, loc: locale_t) -> f64 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstod_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, loc: locale_t) -> f64 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.27";
pub extern "C" fn wcstof64_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, loc: locale_t) -> f64 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.27";
pub extern "C" fn wcstof32x_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, loc: locale_t) -> f64 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstod_internal(s: *const wchar_t, e: Option<&mut *mut wchar_t>, _group: c_int) -> f64 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::current())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstof(s: *const wchar_t, e: Option<&mut *mut wchar_t>) -> f32 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::current())))
}}

export! { "GLIBC_2.27";
pub extern "C" fn wcstof32(s: *const wchar_t, e: Option<&mut *mut wchar_t>) -> f32 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::current())))
}}

export! { "GLIBC_2.3";
pub extern "C" fn wcstof_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, loc: locale_t) -> f32 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstof_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, loc: locale_t) -> f32 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.27";
pub extern "C" fn wcstof32_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, loc: locale_t) -> f32 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstof_internal(s: *const wchar_t, e: Option<&mut *mut wchar_t>, _group: c_int) -> f32 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::current())))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_strtold_bits(s: *const c_char, e: Option<&mut *mut c_char>) -> LdBits {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    LdBits::from_bits(ret::with_errno(num_compat::strto_float(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), Format::F80, num_compat::rounding(CSys.mxcsr()), locale::radix(locale::current()))))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_strtof128_bits(s: *const c_char, e: Option<&mut *mut c_char>) -> F128Bits {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    F128Bits::from_bits(ret::with_errno(num_compat::strto_float(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), Format::F128, num_compat::rounding(CSys.mxcsr()), locale::radix(locale::current()))))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_wcstold_bits(s: *const wchar_t, e: Option<&mut *mut wchar_t>) -> LdBits {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    LdBits::from_bits(ret::with_errno(num_compat::strto_float(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), Format::F80, num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::current()))))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_wcstof128_bits(s: *const wchar_t, e: Option<&mut *mut wchar_t>) -> F128Bits {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    F128Bits::from_bits(ret::with_errno(num_compat::strto_float(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), Format::F128, num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::current()))))
}}

export! { "GLIBC_2.25";
pub extern "C" fn strfromd(buf: *mut c_char, n: size_t, f: *const c_char, x: f64) -> c_int {
    // UNSAFE(slice) SAFETY: buf has room for n bytes (or n is 0); fmt is a string (§8.4).
    ret::int(unsafe { strz::strz(f) }.and_then(|t| unsafe { conv::buf_mut(buf.cast(), n) }.and_then(|b| num_compat::strfrom(b, t, num_compat::from_f64(x), Format::F64, num_compat::rounding(CSys.mxcsr())))))
}}

export! { "GLIBC_2.27";
pub extern "C" fn strfromf64(buf: *mut c_char, n: size_t, f: *const c_char, x: f64) -> c_int {
    // UNSAFE(slice) SAFETY: buf has room for n bytes (or n is 0); fmt is a string (§8.4).
    ret::int(unsafe { strz::strz(f) }.and_then(|t| unsafe { conv::buf_mut(buf.cast(), n) }.and_then(|b| num_compat::strfrom(b, t, num_compat::from_f64(x), Format::F64, num_compat::rounding(CSys.mxcsr())))))
}}

export! { "GLIBC_2.27";
pub extern "C" fn strfromf32x(buf: *mut c_char, n: size_t, f: *const c_char, x: f64) -> c_int {
    // UNSAFE(slice) SAFETY: buf has room for n bytes (or n is 0); fmt is a string (§8.4).
    ret::int(unsafe { strz::strz(f) }.and_then(|t| unsafe { conv::buf_mut(buf.cast(), n) }.and_then(|b| num_compat::strfrom(b, t, num_compat::from_f64(x), Format::F64, num_compat::rounding(CSys.mxcsr())))))
}}

export! { "GLIBC_2.25";
pub extern "C" fn strfromf(buf: *mut c_char, n: size_t, f: *const c_char, x: f32) -> c_int {
    // UNSAFE(slice) SAFETY: buf has room for n bytes (or n is 0); fmt is a string (§8.4).
    ret::int(unsafe { strz::strz(f) }.and_then(|t| unsafe { conv::buf_mut(buf.cast(), n) }.and_then(|b| num_compat::strfrom(b, t, num_compat::from_f32(x), Format::F32, num_compat::rounding(CSys.mxcsr())))))
}}

export! { "GLIBC_2.27";
pub extern "C" fn strfromf32(buf: *mut c_char, n: size_t, f: *const c_char, x: f32) -> c_int {
    // UNSAFE(slice) SAFETY: buf has room for n bytes (or n is 0); fmt is a string (§8.4).
    ret::int(unsafe { strz::strz(f) }.and_then(|t| unsafe { conv::buf_mut(buf.cast(), n) }.and_then(|b| num_compat::strfrom(b, t, num_compat::from_f32(x), Format::F32, num_compat::rounding(CSys.mxcsr())))))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_strfroml_bits(buf: *mut c_char, n: size_t, f: *const c_char, x: LdBits) -> c_int {
    // UNSAFE(slice) SAFETY: buf has room for n bytes (or n is 0); fmt is a string (§8.4).
    ret::int(unsafe { strz::strz(f) }.and_then(|t| unsafe { conv::buf_mut(buf.cast(), n) }.and_then(|b| num_compat::strfrom(b, t, Float::unpack(x.to_bits(), Format::F80), Format::F80, num_compat::rounding(CSys.mxcsr())))))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_strfromf128_bits(buf: *mut c_char, n: size_t, f: *const c_char, x: F128Bits) -> c_int {
    // UNSAFE(slice) SAFETY: buf has room for n bytes (or n is 0); fmt is a string (§8.4).
    ret::int(unsafe { strz::strz(f) }.and_then(|t| unsafe { conv::buf_mut(buf.cast(), n) }.and_then(|b| num_compat::strfrom(b, t, Float::unpack(x.to_bits(), Format::F128), Format::F128, num_compat::rounding(CSys.mxcsr())))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ecvt(x: f64, ndigit: c_int, decpt: Option<&mut c_int>, sign: Option<&mut c_int>) -> *mut c_char {
    ret::null(crate::users::result_buf::<512>(crate::users::ECVT).map(|b| ret::must(conv::req(decpt).and_then(|d| conv::req(sign).map(|g| num_compat::cvt_static(b, num_compat::from_f64(x), ndigit.min(num_compat::DOUBLE_DIGITS), num_compat::rounding(CSys.mxcsr()), d, g, false))))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fcvt(x: f64, ndigit: c_int, decpt: Option<&mut c_int>, sign: Option<&mut c_int>) -> *mut c_char {
    ret::null(crate::users::result_buf::<512>(crate::users::FCVT).map(|b| ret::must(conv::req(decpt).and_then(|d| conv::req(sign).map(|g| num_compat::cvt_static(b, num_compat::from_f64(x), ndigit.min(num_compat::DOUBLE_DIGITS), num_compat::rounding(CSys.mxcsr()), d, g, true))))))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_qecvt_bits(x: LdBits, ndigit: c_int, decpt: Option<&mut c_int>, sign: Option<&mut c_int>) -> *mut c_char {
    ret::null(crate::users::result_buf::<5120>(crate::users::QECVT).map(|b| ret::must(conv::req(decpt).and_then(|d| conv::req(sign).map(|g| num_compat::cvt_static(b, Float::unpack(x.to_bits(), Format::F80), ndigit.min(num_compat::LONG_DOUBLE_DIGITS), num_compat::rounding(CSys.mxcsr()), d, g, false))))))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_qfcvt_bits(x: LdBits, ndigit: c_int, decpt: Option<&mut c_int>, sign: Option<&mut c_int>) -> *mut c_char {
    ret::null(crate::users::result_buf::<5120>(crate::users::QFCVT).map(|b| ret::must(conv::req(decpt).and_then(|d| conv::req(sign).map(|g| num_compat::cvt_static(b, Float::unpack(x.to_bits(), Format::F80), ndigit.min(num_compat::LONG_DOUBLE_DIGITS), num_compat::rounding(CSys.mxcsr()), d, g, true))))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ecvt_r(x: f64, ndigit: c_int, decpt: Option<&mut c_int>, sign: Option<&mut c_int>, buf: *mut c_char, len: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: decpt and sign are valid int variables; buf has room for len bytes (§8.4).
    ret::int(conv::req(decpt).and_then(|d| conv::req(sign).and_then(|g| unsafe { conv::buf_mut(buf.cast(), len) }.map(|b| num_compat::cvt_r(num_compat::from_f64(x), ndigit.min(num_compat::DOUBLE_DIGITS), num_compat::rounding(CSys.mxcsr()), d, g, b, false)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fcvt_r(x: f64, ndigit: c_int, decpt: Option<&mut c_int>, sign: Option<&mut c_int>, buf: *mut c_char, len: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: decpt and sign are valid int variables; buf has room for len bytes (§8.4).
    ret::int(conv::req(decpt).and_then(|d| conv::req(sign).and_then(|g| unsafe { conv::buf_mut(buf.cast(), len) }.map(|b| num_compat::cvt_r(num_compat::from_f64(x), ndigit.min(num_compat::DOUBLE_DIGITS), num_compat::rounding(CSys.mxcsr()), d, g, b, true)))))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_qecvt_r_bits(x: LdBits, ndigit: c_int, decpt: Option<&mut c_int>, sign: Option<&mut c_int>, buf: *mut c_char, len: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: decpt and sign are valid int variables; buf has room for len bytes (§8.4).
    ret::int(conv::req(decpt).and_then(|d| conv::req(sign).and_then(|g| unsafe { conv::buf_mut(buf.cast(), len) }.map(|b| num_compat::cvt_r(Float::unpack(x.to_bits(), Format::F80), ndigit.min(num_compat::LONG_DOUBLE_DIGITS), num_compat::rounding(CSys.mxcsr()), d, g, b, false)))))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_qfcvt_r_bits(x: LdBits, ndigit: c_int, decpt: Option<&mut c_int>, sign: Option<&mut c_int>, buf: *mut c_char, len: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: decpt and sign are valid int variables; buf has room for len bytes (§8.4).
    ret::int(conv::req(decpt).and_then(|d| conv::req(sign).and_then(|g| unsafe { conv::buf_mut(buf.cast(), len) }.map(|b| num_compat::cvt_r(Float::unpack(x.to_bits(), Format::F80), ndigit.min(num_compat::LONG_DOUBLE_DIGITS), num_compat::rounding(CSys.mxcsr()), d, g, b, true)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn gcvt(x: f64, ndigit: c_int, buf: *mut c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: buf has room for the %.*g result (§8.4).
    ret::same(buf, num_compat::gcvt_into(unsafe { conv::dest::<u8, _>(buf) }, num_compat::from_f64(x), Format::F64, ndigit, num_compat::rounding(CSys.mxcsr())))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_qgcvt_bits(x: LdBits, ndigit: c_int, buf: *mut c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: buf has room for the %.*g result (§8.4).
    ret::same(buf, num_compat::gcvt_into(unsafe { conv::dest::<u8, _>(buf) }, Float::unpack(x.to_bits(), Format::F80), Format::F80, ndigit, num_compat::rounding(CSys.mxcsr())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn abs(n: c_int) -> c_int {
    num_compat::abs32(n)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn labs(n: c_long) -> c_long {
    num_compat::abs64(n)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn llabs(n: c_long) -> c_long {
    num_compat::abs64(n)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn imaxabs(n: c_long) -> c_long {
    num_compat::abs64(n)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn div(n: c_int, d: c_int) -> div_t {
    pack::div(ret::must(num_compat::div32(n, d)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ldiv(n: c_long, d: c_long) -> ldiv_t {
    pack::ldiv(ret::must(num_compat::div64(n, d)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn lldiv(n: c_long, d: c_long) -> lldiv_t {
    pack::lldiv(ret::must(num_compat::div64(n, d)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn imaxdiv(n: c_long, d: c_long) -> imaxdiv_t {
    pack::imaxdiv(ret::must(num_compat::div64(n, d)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ffs(n: c_int) -> c_int {
    num_compat::ffs32(n)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __ffs(n: c_int) -> c_int {
    num_compat::ffs32(n)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ffsl(n: c_long) -> c_int {
    num_compat::ffs64(n)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ffsll(n: c_long) -> c_int {
    num_compat::ffs64(n)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn l64a(n: c_long) -> *mut c_char {
    ret::null(crate::users::result_buf::<7>(crate::users::L64A).map(|b| num_compat::l64a_into(b, n)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn a64l(s: *const c_char) -> c_long {
    // UNSAFE(slice) SAFETY: s is a string (§8.10).
    ret::must(unsafe { strz::strz(s) }.map(num::a64l))
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_leading_zeros_uc(x: c_uchar) -> c_uint {
    StdBit::leading_zeros_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_leading_zeros_us(x: c_ushort) -> c_uint {
    StdBit::leading_zeros_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_leading_zeros_ui(x: c_uint) -> c_uint {
    StdBit::leading_zeros_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_leading_zeros_ul(x: c_ulong) -> c_uint {
    StdBit::leading_zeros_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_leading_zeros_ull(x: c_ulonglong) -> c_uint {
    StdBit::leading_zeros_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_leading_ones_uc(x: c_uchar) -> c_uint {
    StdBit::leading_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_leading_ones_us(x: c_ushort) -> c_uint {
    StdBit::leading_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_leading_ones_ui(x: c_uint) -> c_uint {
    StdBit::leading_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_leading_ones_ul(x: c_ulong) -> c_uint {
    StdBit::leading_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_leading_ones_ull(x: c_ulonglong) -> c_uint {
    StdBit::leading_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_trailing_zeros_uc(x: c_uchar) -> c_uint {
    StdBit::trailing_zeros_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_trailing_zeros_us(x: c_ushort) -> c_uint {
    StdBit::trailing_zeros_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_trailing_zeros_ui(x: c_uint) -> c_uint {
    StdBit::trailing_zeros_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_trailing_zeros_ul(x: c_ulong) -> c_uint {
    StdBit::trailing_zeros_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_trailing_zeros_ull(x: c_ulonglong) -> c_uint {
    StdBit::trailing_zeros_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_trailing_ones_uc(x: c_uchar) -> c_uint {
    StdBit::trailing_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_trailing_ones_us(x: c_ushort) -> c_uint {
    StdBit::trailing_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_trailing_ones_ui(x: c_uint) -> c_uint {
    StdBit::trailing_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_trailing_ones_ul(x: c_ulong) -> c_uint {
    StdBit::trailing_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_trailing_ones_ull(x: c_ulonglong) -> c_uint {
    StdBit::trailing_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_leading_zero_uc(x: c_uchar) -> c_uint {
    num::first_leading_zero(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_leading_zero_us(x: c_ushort) -> c_uint {
    num::first_leading_zero(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_leading_zero_ui(x: c_uint) -> c_uint {
    num::first_leading_zero(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_leading_zero_ul(x: c_ulong) -> c_uint {
    num::first_leading_zero(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_leading_zero_ull(x: c_ulonglong) -> c_uint {
    num::first_leading_zero(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_leading_one_uc(x: c_uchar) -> c_uint {
    num::first_leading_one(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_leading_one_us(x: c_ushort) -> c_uint {
    num::first_leading_one(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_leading_one_ui(x: c_uint) -> c_uint {
    num::first_leading_one(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_leading_one_ul(x: c_ulong) -> c_uint {
    num::first_leading_one(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_leading_one_ull(x: c_ulonglong) -> c_uint {
    num::first_leading_one(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_trailing_zero_uc(x: c_uchar) -> c_uint {
    num::first_trailing_zero(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_trailing_zero_us(x: c_ushort) -> c_uint {
    num::first_trailing_zero(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_trailing_zero_ui(x: c_uint) -> c_uint {
    num::first_trailing_zero(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_trailing_zero_ul(x: c_ulong) -> c_uint {
    num::first_trailing_zero(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_trailing_zero_ull(x: c_ulonglong) -> c_uint {
    num::first_trailing_zero(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_trailing_one_uc(x: c_uchar) -> c_uint {
    num::first_trailing_one(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_trailing_one_us(x: c_ushort) -> c_uint {
    num::first_trailing_one(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_trailing_one_ui(x: c_uint) -> c_uint {
    num::first_trailing_one(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_trailing_one_ul(x: c_ulong) -> c_uint {
    num::first_trailing_one(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_trailing_one_ull(x: c_ulonglong) -> c_uint {
    num::first_trailing_one(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_count_zeros_uc(x: c_uchar) -> c_uint {
    num::count_zeros(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_count_zeros_us(x: c_ushort) -> c_uint {
    num::count_zeros(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_count_zeros_ui(x: c_uint) -> c_uint {
    num::count_zeros(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_count_zeros_ul(x: c_ulong) -> c_uint {
    num::count_zeros(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_count_zeros_ull(x: c_ulonglong) -> c_uint {
    num::count_zeros(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_count_ones_uc(x: c_uchar) -> c_uint {
    StdBit::count_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_count_ones_us(x: c_ushort) -> c_uint {
    StdBit::count_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_count_ones_ui(x: c_uint) -> c_uint {
    StdBit::count_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_count_ones_ul(x: c_ulong) -> c_uint {
    StdBit::count_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_count_ones_ull(x: c_ulonglong) -> c_uint {
    StdBit::count_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_has_single_bit_uc(x: c_uchar) -> bool {
    num::has_single_bit(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_has_single_bit_us(x: c_ushort) -> bool {
    num::has_single_bit(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_has_single_bit_ui(x: c_uint) -> bool {
    num::has_single_bit(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_has_single_bit_ul(x: c_ulong) -> bool {
    num::has_single_bit(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_has_single_bit_ull(x: c_ulonglong) -> bool {
    num::has_single_bit(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_width_uc(x: c_uchar) -> c_uint {
    num::bit_width(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_width_us(x: c_ushort) -> c_uint {
    num::bit_width(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_width_ui(x: c_uint) -> c_uint {
    num::bit_width(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_width_ul(x: c_ulong) -> c_uint {
    num::bit_width(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_width_ull(x: c_ulonglong) -> c_uint {
    num::bit_width(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_floor_uc(x: c_uchar) -> c_uchar {
    StdBit::bit_floor_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_floor_us(x: c_ushort) -> c_ushort {
    StdBit::bit_floor_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_floor_ui(x: c_uint) -> c_uint {
    StdBit::bit_floor_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_floor_ul(x: c_ulong) -> c_ulong {
    StdBit::bit_floor_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_floor_ull(x: c_ulonglong) -> c_ulonglong {
    StdBit::bit_floor_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_ceil_uc(x: c_uchar) -> c_uchar {
    StdBit::bit_ceil_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_ceil_us(x: c_ushort) -> c_ushort {
    StdBit::bit_ceil_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_ceil_ui(x: c_uint) -> c_uint {
    StdBit::bit_ceil_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_ceil_ul(x: c_ulong) -> c_ulong {
    StdBit::bit_ceil_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_ceil_ull(x: c_ulonglong) -> c_ulonglong {
    StdBit::bit_ceil_(x)
}}
