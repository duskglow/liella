//! Translation for spec §26: system logging (phase 13; D-26.1).
//!
//! The process-wide logger is `liella_legacy::log_compat` over `liella::log`, with the glue in
//! `crate::syslog`. `syslog` and `__syslog_chk` are variadic: their C entry points are adapters
//! (`csys/src/adapter/variadic.c`) forwarding to `liella_vsyslog4` with the `va_list` (OD-C1).
//! `openlog` keeps the identifier by pointer (§26.1 ⚑ HIST). No entry point reports failure or
//! changes `errno`.

use liella_abi::ctypes::{c_char, c_int, c_void};

use crate::conv;
use crate::export;
use crate::strz;
use crate::syslog;

export! { "GLIBC_2.2.5";
pub extern "C" fn openlog(ident: *const c_char, option: c_int, facility: c_int) {
    syslog::openlog(conv::addr(ident), option, facility)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn closelog() {
    syslog::closelog()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setlogmask(mask: c_int) -> c_int {
    syslog::setlogmask(mask)
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_vsyslog4(pri: c_int, flag: c_int, fmt: *const c_char, ap: *mut c_void) {
    // UNSAFE(slice) SAFETY: fmt is NULL or a NUL-terminated string (§26.1); ap is the caller's va_list.
    syslog::vsyslog(pri, flag, unsafe { strz::strz(fmt) }, conv::addr(ap))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn vsyslog(pri: c_int, fmt: *const c_char, ap: *mut c_void) {
    // UNSAFE(slice) SAFETY: fmt is NULL or a NUL-terminated string (§26.1); ap is the caller's va_list.
    syslog::vsyslog(pri, 0, unsafe { strz::strz(fmt) }, conv::addr(ap))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __vsyslog_chk(pri: c_int, flag: c_int, fmt: *const c_char, ap: *mut c_void) {
    // UNSAFE(slice) SAFETY: fmt is NULL or a NUL-terminated string (§26.1); ap is the caller's va_list.
    syslog::vsyslog(pri, flag, unsafe { strz::strz(fmt) }, conv::addr(ap))
}}
