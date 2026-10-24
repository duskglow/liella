//! Translation for spec §7: locales (§7.1–7.4), `strfmon` (§7.5) and the message catalogs
//! (§7.7, §7.8). Locale objects, the global locale and the per-thread choice are `crate::locale`
//! (D-7.7, D-7.8); the catalogs are `crate::catalog` (D-7.5, D-7.6). `rpmatch` (§7.6) matches with
//! the regular expressions of phase 12 (D-7.9; missed there, added in the validation stage, T2).

use liella_abi::ctypes::{c_char, c_int, c_ulong, c_void, locale_t, size_t, ssize_t};

use crate::catalog;
use crate::conv;
use crate::export;
use crate::locale;
use crate::ret;
use crate::strz;

export! { "GLIBC_2.2.5";
pub extern "C" fn setlocale(category: c_int, name: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: name is NULL or a string (§7.2).
    ret::ptr(locale::setlocale_c(category, unsafe { strz::strz_opt(name) }))
}}

export! { "GLIBC_2.3";
pub extern "C" fn newlocale(mask: c_int, name: *const c_char, base: locale_t) -> locale_t {
    // UNSAFE(slice) SAFETY: name is NULL or a string (§7.3).
    ret::null(locale::newlocale_c(mask, unsafe { strz::strz(name) }, conv::addr(base)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __newlocale(mask: c_int, name: *const c_char, base: locale_t) -> locale_t {
    // UNSAFE(slice) SAFETY: as newlocale.
    ret::null(locale::newlocale_c(mask, unsafe { strz::strz(name) }, conv::addr(base)))
}}

export! { "GLIBC_2.3";
pub extern "C" fn duplocale(loc: locale_t) -> locale_t {
    ret::null(locale::duplocale(conv::addr(loc)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __duplocale(loc: locale_t) -> locale_t {
    ret::null(locale::duplocale(conv::addr(loc)))
}}

export! { "GLIBC_2.3";
pub extern "C" fn freelocale(loc: locale_t) {
    locale::freelocale(conv::addr(loc))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __freelocale(loc: locale_t) {
    locale::freelocale(conv::addr(loc))
}}

export! { "GLIBC_2.3";
pub extern "C" fn uselocale(loc: locale_t) -> locale_t {
    ret::ptr::<c_void>(locale::uselocale(conv::addr(loc)))
}}

export! { "GLIBC_2.3";
pub extern "C" fn __uselocale(loc: locale_t) -> locale_t {
    ret::ptr::<c_void>(locale::uselocale(conv::addr(loc)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn localeconv() -> *mut liella_abi::layout::struct_lconv {
    ret::ptr(locale::localeconv())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn nl_langinfo(item: c_int) -> *mut c_char {
    ret::ptr(locale::langinfo(item, locale::view()))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn rpmatch(response: *const c_char) -> c_int {
    locale::rpmatch(conv::addr(response))
}}

export! { "GLIBC_2.3";
pub extern "C" fn nl_langinfo_l(item: c_int, loc: locale_t) -> *mut c_char {
    ret::ptr(locale::langinfo(item, locale::view_of(conv::addr(loc))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __nl_langinfo_l(item: c_int, loc: locale_t) -> *mut c_char {
    ret::ptr(locale::langinfo(item, locale::view_of(conv::addr(loc))))
}}

// ------------------------------------------------------------------------------ §7.7
//
// The gettext family never fails visibly and preserves `errno` (§7.7): a lookup returns the
// translation or the argument it was given (`msgid`, or by `n` `msgid`/`msgid_plural`).

export! { "GLIBC_2.2.5";
pub extern "C" fn gettext(msgid: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: msgid is a string (§7.7; NULL is returned as given).
    ret::ptr(catalog::gettext(conv::addr(msgid), unsafe { strz::strz_opt(msgid) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn dgettext(domain: *const c_char, msgid: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: domain is NULL or a string; msgid is a string (§7.7).
    ret::ptr(catalog::dcgettext(unsafe { strz::strz_opt(domain) }, conv::addr(msgid), unsafe { strz::strz_opt(msgid) }, 5))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __dgettext(domain: *const c_char, msgid: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: as dgettext.
    ret::ptr(catalog::dcgettext(unsafe { strz::strz_opt(domain) }, conv::addr(msgid), unsafe { strz::strz_opt(msgid) }, 5))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn dcgettext(domain: *const c_char, msgid: *const c_char, category: c_int) -> *mut c_char {
    // UNSAFE(slice) SAFETY: domain is NULL or a string; msgid is a string (§7.7).
    ret::ptr(catalog::dcgettext(unsafe { strz::strz_opt(domain) }, conv::addr(msgid), unsafe { strz::strz_opt(msgid) }, category))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __dcgettext(domain: *const c_char, msgid: *const c_char, category: c_int) -> *mut c_char {
    // UNSAFE(slice) SAFETY: as dcgettext.
    ret::ptr(catalog::dcgettext(unsafe { strz::strz_opt(domain) }, conv::addr(msgid), unsafe { strz::strz_opt(msgid) }, category))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ngettext(msgid: *const c_char, msgid_plural: *const c_char, n: c_ulong) -> *mut c_char {
    // UNSAFE(slice) SAFETY: msgid is a string; msgid_plural is only returned (§7.7).
    ret::ptr(catalog::dcngettext(Ok(None), conv::addr(msgid), unsafe { strz::strz_opt(msgid) }, conv::addr(msgid_plural), n, 5))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn dngettext(domain: *const c_char, msgid: *const c_char, msgid_plural: *const c_char, n: c_ulong) -> *mut c_char {
    // UNSAFE(slice) SAFETY: domain is NULL or a string; msgid is a string (§7.7).
    ret::ptr(catalog::dcngettext(unsafe { strz::strz_opt(domain) }, conv::addr(msgid), unsafe { strz::strz_opt(msgid) }, conv::addr(msgid_plural), n, 5))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn dcngettext(domain: *const c_char, msgid: *const c_char, msgid_plural: *const c_char, n: c_ulong, category: c_int) -> *mut c_char {
    // UNSAFE(slice) SAFETY: domain is NULL or a string; msgid is a string (§7.7).
    ret::ptr(catalog::dcngettext(unsafe { strz::strz_opt(domain) }, conv::addr(msgid), unsafe { strz::strz_opt(msgid) }, conv::addr(msgid_plural), n, category))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn textdomain(name: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: name is NULL or a string (§7.7).
    ret::ptr(catalog::textdomain(unsafe { strz::strz_opt(name) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn bindtextdomain(domain: *const c_char, dir: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: domain and dir are NULL or strings (§7.7).
    ret::ptr(catalog::bindtextdomain(unsafe { strz::strz_opt(domain) }, unsafe { strz::strz_opt(dir) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn bind_textdomain_codeset(domain: *const c_char, codeset: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: domain and codeset are NULL or strings (§7.7).
    ret::ptr(catalog::bind_textdomain_codeset(unsafe { strz::strz_opt(domain) }, unsafe { strz::strz_opt(codeset) }))
}}

// ------------------------------------------------------------------------------ §7.8

export! { "GLIBC_2.2.5";
pub extern "C" fn catopen(name: *const c_char, flag: c_int) -> *mut c_void {
    // UNSAFE(slice) SAFETY: name is a string (§7.8; NULL is EINVAL).
    ret::addr_or_neg1(catalog::catopen(unsafe { strz::strz(name) }, flag))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn catgets(catd: *mut c_void, set: c_int, msg: c_int, default: *const c_char) -> *mut c_char {
    ret::ptr(ret::with_errno(catalog::catgets(conv::addr(catd), set, msg, conv::addr(default))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn catclose(catd: *mut c_void) -> c_int {
    ret::zero(catalog::catclose(conv::addr(catd)))
}}

// ------------------------------------------------------------------------------ §7.5

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_vstrfmon5(s: *mut c_char, max: size_t, loc: locale_t, fmt: *const c_char, ap: *mut c_void) -> ssize_t {
    // UNSAFE(slice) SAFETY: s has room for max bytes; fmt is a string; ap is the va_list of the amounts it names (§7.5).
    ret::ssize(crate::monetary::vstrfmon(unsafe { conv::buf_mut(s.cast(), max) }, conv::addr(loc), unsafe { strz::strz(fmt) }, conv::addr(ap)))
}}
