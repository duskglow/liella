//! Translation for spec §2.1 to §2.5: `errno`, error text, the `err`/`error` families,
//! assertions, fatal diagnostics; and §2.6 to §2.7, `fmtmsg` and `backtrace`.
//!
//! `__errno_location` is a `runtime` export (only the products define it; test processes keep
//! the host's, D-2.1). The variadic `err`/`warn`/`error` names are C adapters forwarding to
//! `liella_report7`.

use core::ffi::c_void;

use liella_abi::ctypes::{c_char, c_int, c_long, c_uint, locale_t, size_t};

use crate::backtrace as bt;
use crate::conv;
use crate::export;
use crate::fmtmsg;
use crate::process;
use crate::report::{self, Kind};
use crate::ret;
use crate::strz;
use crate::tb;

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn __errno_location() -> *mut c_int {
    tb::errno_location()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __assert_fail(assertion: *const c_char, file: *const c_char, line: c_uint, function: *const c_char) {
    // UNSAFE(slice) SAFETY: the compiler-generated arguments are NUL-terminated strings; function may be NULL (§2.4).
    process::assert_fail(unsafe { strz::strz(assertion) }.unwrap_or(b"?"), unsafe { strz::strz(file) }.unwrap_or(b"?"), line, unsafe { strz::strz_opt(function) }.unwrap_or(None))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __assert_perror_fail(errnum: c_int, file: *const c_char, line: c_uint, function: *const c_char) {
    // UNSAFE(slice) SAFETY: as __assert_fail.
    process::assert_perror_fail(errnum, unsafe { strz::strz(file) }.unwrap_or(b"?"), line, unsafe { strz::strz_opt(function) }.unwrap_or(None))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __assert(assertion: *const c_char, file: *const c_char, line: c_int) {
    // UNSAFE(slice) SAFETY: as __assert_fail (BSD form, no function name).
    process::assert_fail(unsafe { strz::strz(assertion) }.unwrap_or(b"?"), unsafe { strz::strz(file) }.unwrap_or(b"?"), ret::line(line), None)
}}

export! { "GLIBC_2.4";
pub extern "C" fn __stack_chk_fail() {
    process::stack_chk_fail()
}}

export! { "GLIBC_2.3.4";
pub extern "C" fn __chk_fail() {
    ret::chk_fail()
}}

// ------------------------------------------------------------------------------ §2.2

export! { "GLIBC_2.2.5";
pub extern "C" fn strerror(e: c_int) -> *mut c_char {
    ret::ptr(report::strerror(e))
}}

export! { "GLIBC_2.6";
pub extern "C" fn strerror_l(e: c_int, _loc: locale_t) -> *mut c_char {
    ret::ptr(report::strerror(e))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strerror_r(e: c_int, buf: *mut c_char, n: size_t) -> *mut c_char {
    // UNSAFE(slice) SAFETY: buf is n writable bytes (§2.2); NULL with n = 0 is no buffer.
    ret::ptr(report::strerror_r(e, unsafe { conv::buf_mut(buf.cast(), n) }, conv::addr(buf)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strerror_r(e: c_int, buf: *mut c_char, n: size_t) -> *mut c_char {
    // UNSAFE(slice) SAFETY: buf is n writable bytes (§2.2); NULL with n = 0 is no buffer.
    ret::ptr(report::strerror_r(e, unsafe { conv::buf_mut(buf.cast(), n) }, conv::addr(buf)))
}}

export! { "GLIBC_2.3.4";
pub extern "C" fn __xpg_strerror_r(e: c_int, buf: *mut c_char, n: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: buf is n writable bytes (§2.2); NULL with n = 0 is no buffer.
    report::xpg_strerror_r(e, unsafe { conv::buf_mut(buf.cast(), n) })
}}

export! { "GLIBC_2.32";
pub extern "C" fn strerrorname_np(e: c_int) -> *const c_char {
    ret::ptr(report::strerrorname(e))
}}

export! { "GLIBC_2.32";
pub extern "C" fn strerrordesc_np(e: c_int) -> *const c_char {
    ret::ptr(report::strerrordesc(e))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn perror(s: *const c_char) {
    // UNSAFE(slice) SAFETY: s is NULL or a string (§2.2).
    report::perror(unsafe { strz::strz_opt(s) })
}}

// ------------------------------------------------------------------------------ §2.3

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_report7(kind: c_int, status: c_int, errnum: c_int, file: *const c_char, line: c_uint, fmt: *const c_char, ap: *mut c_void) {
    // UNSAFE(slice) SAFETY: file and fmt are NULL or strings; ap is the va_list of the arguments fmt names (§2.3).
    report::report(Kind::of(kind), status, errnum, unsafe { strz::strz_opt(file) }, line, unsafe { strz::strz_opt(fmt) }, conv::addr(ap))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn vwarn(fmt: *const c_char, ap: *mut c_void) {
    // UNSAFE(slice) SAFETY: fmt is NULL or a string; ap is the va_list of the arguments it names (§2.3).
    report::report(Kind::Warn, 0, 0, Ok(None), 0, unsafe { strz::strz_opt(fmt) }, conv::addr(ap))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn vwarnx(fmt: *const c_char, ap: *mut c_void) {
    // UNSAFE(slice) SAFETY: fmt is NULL or a string; ap is the va_list of the arguments it names (§2.3).
    report::report(Kind::Warnx, 0, 0, Ok(None), 0, unsafe { strz::strz_opt(fmt) }, conv::addr(ap))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn verr(status: c_int, fmt: *const c_char, ap: *mut c_void) {
    // UNSAFE(slice) SAFETY: fmt is NULL or a string; ap is the va_list of the arguments it names (§2.3).
    report::report(Kind::Err, status, 0, Ok(None), 0, unsafe { strz::strz_opt(fmt) }, conv::addr(ap))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn verrx(status: c_int, fmt: *const c_char, ap: *mut c_void) {
    // UNSAFE(slice) SAFETY: fmt is NULL or a string; ap is the va_list of the arguments it names (§2.3).
    report::report(Kind::Errx, status, 0, Ok(None), 0, unsafe { strz::strz_opt(fmt) }, conv::addr(ap))
}}

// ------------------------------------------------------------------------------ §2.6

export! { "GLIBC_2.2.5";
pub extern "C" fn fmtmsg(classification: c_long, label: *const c_char, severity: c_int, text: *const c_char, action: *const c_char, tag: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: label, text, action and tag are NULL or strings (§2.6).
    let (label, text) = (unsafe { strz::strz_opt(label) }, unsafe { strz::strz_opt(text) });
    // UNSAFE(slice) SAFETY: as above.
    let (action, tag) = (unsafe { strz::strz_opt(action) }, unsafe { strz::strz_opt(tag) });
    fmtmsg::fmtmsg(classification, label.unwrap_or(None), severity, text.unwrap_or(None), action.unwrap_or(None), tag.unwrap_or(None))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn addseverity(severity: c_int, text: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: text is NULL or a string (§2.6); it is copied before the call returns.
    fmtmsg::addseverity(severity, unsafe { strz::strz_opt(text) }.unwrap_or(None))
}}

// ------------------------------------------------------------------------------ §2.7

export! { "GLIBC_2.2.5";
pub extern "C" fn backtrace(buffer: *mut *mut c_void, size: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: buffer has room for size pointers (§2.7); a size of 0 or less is no buffer.
    bt::capture(unsafe { conv::buf_mut(buffer.cast(), bt::bytes(size)) }.unwrap_or_default(), bt::entry(backtrace))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __backtrace(buffer: *mut *mut c_void, size: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: as backtrace.
    bt::capture(unsafe { conv::buf_mut(buffer.cast(), bt::bytes(size)) }.unwrap_or_default(), bt::entry(__backtrace))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn backtrace_symbols(array: *const *mut c_void, size: c_int) -> *mut *mut c_char {
    // UNSAFE(slice) SAFETY: array holds size addresses (§2.7); a size of 0 or less is none.
    ret::null(unsafe { conv::buf(array.cast(), bt::bytes(size)) }.and_then(bt::symbols))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __backtrace_symbols(array: *const *mut c_void, size: c_int) -> *mut *mut c_char {
    // UNSAFE(slice) SAFETY: as backtrace_symbols.
    ret::null(unsafe { conv::buf(array.cast(), bt::bytes(size)) }.and_then(bt::symbols))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn backtrace_symbols_fd(array: *const *mut c_void, size: c_int, fd: c_int) {
    // UNSAFE(slice) SAFETY: as backtrace_symbols.
    ret::errno_only(unsafe { conv::buf(array.cast(), bt::bytes(size)) }.and_then(|a| bt::symbols_fd(a, conv::fd(fd))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __backtrace_symbols_fd(array: *const *mut c_void, size: c_int, fd: c_int) {
    // UNSAFE(slice) SAFETY: as backtrace_symbols.
    ret::errno_only(unsafe { conv::buf(array.cast(), bt::bytes(size)) }.and_then(|a| bt::symbols_fd(a, conv::fd(fd))))
}}
