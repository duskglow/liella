//! Translation for spec §5: character classification and case mapping.
//!
//! The plain forms use the calling thread's current locale, the `_l` forms their locale
//! object (D-7.8). `__ctype_*_loc` return the address of a per-thread pointer that always
//! holds the current locale's table, and the plain byte forms read it (D-5.5). `wctype_t` is a standard class's mask, or 0x10000 plus a
//! locale-defined class's number; `wctrans_t` is the address of the mapping's table, as in
//! glibc.

use liella::ctype::Class;
use liella_abi::ctypes::{c_char, c_int, c_ushort, locale_t, wctrans_t, wctype_t, wint_t};
use liella_legacy::ctype_tables;

use crate::conv;
use crate::locale;
use crate::export;
use crate::ret;
use crate::strz;

export! { "GLIBC_2.2.5";
pub extern "C" fn isalnum(c: c_int) -> c_int {
    ret::truth(locale::byte_class(c).intersects(Class::ALNUM))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __isalnum_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::ALNUM))
}}

export! { "GLIBC_2.3";
pub extern "C" fn isalnum_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::ALNUM))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn iswalnum(wc: wint_t) -> c_int {
    ret::truth(locale::view().isw(wc, Class::ALNUM))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __iswalnum_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::ALNUM))
}}

export! { "GLIBC_2.3";
pub extern "C" fn iswalnum_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::ALNUM))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn isalpha(c: c_int) -> c_int {
    ret::truth(locale::byte_class(c).intersects(Class::ALPHA))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __isalpha_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::ALPHA))
}}

export! { "GLIBC_2.3";
pub extern "C" fn isalpha_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::ALPHA))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn iswalpha(wc: wint_t) -> c_int {
    ret::truth(locale::view().isw(wc, Class::ALPHA))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __iswalpha_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::ALPHA))
}}

export! { "GLIBC_2.3";
pub extern "C" fn iswalpha_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::ALPHA))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn isblank(c: c_int) -> c_int {
    ret::truth(locale::byte_class(c).intersects(Class::BLANK))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __isblank_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::BLANK))
}}

export! { "GLIBC_2.3";
pub extern "C" fn isblank_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::BLANK))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn iswblank(wc: wint_t) -> c_int {
    ret::truth(locale::view().isw(wc, Class::BLANK))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __iswblank_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::BLANK))
}}

export! { "GLIBC_2.3";
pub extern "C" fn iswblank_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::BLANK))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn iscntrl(c: c_int) -> c_int {
    ret::truth(locale::byte_class(c).intersects(Class::CNTRL))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __iscntrl_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::CNTRL))
}}

export! { "GLIBC_2.3";
pub extern "C" fn iscntrl_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::CNTRL))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn iswcntrl(wc: wint_t) -> c_int {
    ret::truth(locale::view().isw(wc, Class::CNTRL))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __iswcntrl_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::CNTRL))
}}

export! { "GLIBC_2.3";
pub extern "C" fn iswcntrl_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::CNTRL))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn isdigit(c: c_int) -> c_int {
    ret::truth(locale::byte_class(c).intersects(Class::DIGIT))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __isdigit_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::DIGIT))
}}

export! { "GLIBC_2.3";
pub extern "C" fn isdigit_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::DIGIT))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn iswdigit(wc: wint_t) -> c_int {
    ret::truth(locale::view().isw(wc, Class::DIGIT))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __iswdigit_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::DIGIT))
}}

export! { "GLIBC_2.3";
pub extern "C" fn iswdigit_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::DIGIT))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn isgraph(c: c_int) -> c_int {
    ret::truth(locale::byte_class(c).intersects(Class::GRAPH))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __isgraph_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::GRAPH))
}}

export! { "GLIBC_2.3";
pub extern "C" fn isgraph_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::GRAPH))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn iswgraph(wc: wint_t) -> c_int {
    ret::truth(locale::view().isw(wc, Class::GRAPH))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __iswgraph_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::GRAPH))
}}

export! { "GLIBC_2.3";
pub extern "C" fn iswgraph_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::GRAPH))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn islower(c: c_int) -> c_int {
    ret::truth(locale::byte_class(c).intersects(Class::LOWER))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __islower_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::LOWER))
}}

export! { "GLIBC_2.3";
pub extern "C" fn islower_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::LOWER))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn iswlower(wc: wint_t) -> c_int {
    ret::truth(locale::view().isw(wc, Class::LOWER))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __iswlower_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::LOWER))
}}

export! { "GLIBC_2.3";
pub extern "C" fn iswlower_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::LOWER))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn isprint(c: c_int) -> c_int {
    ret::truth(locale::byte_class(c).intersects(Class::PRINT))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __isprint_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::PRINT))
}}

export! { "GLIBC_2.3";
pub extern "C" fn isprint_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::PRINT))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn iswprint(wc: wint_t) -> c_int {
    ret::truth(locale::view().isw(wc, Class::PRINT))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __iswprint_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::PRINT))
}}

export! { "GLIBC_2.3";
pub extern "C" fn iswprint_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::PRINT))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ispunct(c: c_int) -> c_int {
    ret::truth(locale::byte_class(c).intersects(Class::PUNCT))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __ispunct_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::PUNCT))
}}

export! { "GLIBC_2.3";
pub extern "C" fn ispunct_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::PUNCT))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn iswpunct(wc: wint_t) -> c_int {
    ret::truth(locale::view().isw(wc, Class::PUNCT))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __iswpunct_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::PUNCT))
}}

export! { "GLIBC_2.3";
pub extern "C" fn iswpunct_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::PUNCT))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn isspace(c: c_int) -> c_int {
    ret::truth(locale::byte_class(c).intersects(Class::SPACE))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __isspace_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::SPACE))
}}

export! { "GLIBC_2.3";
pub extern "C" fn isspace_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::SPACE))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn iswspace(wc: wint_t) -> c_int {
    ret::truth(locale::view().isw(wc, Class::SPACE))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __iswspace_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::SPACE))
}}

export! { "GLIBC_2.3";
pub extern "C" fn iswspace_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::SPACE))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn isupper(c: c_int) -> c_int {
    ret::truth(locale::byte_class(c).intersects(Class::UPPER))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __isupper_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::UPPER))
}}

export! { "GLIBC_2.3";
pub extern "C" fn isupper_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::UPPER))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn iswupper(wc: wint_t) -> c_int {
    ret::truth(locale::view().isw(wc, Class::UPPER))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __iswupper_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::UPPER))
}}

export! { "GLIBC_2.3";
pub extern "C" fn iswupper_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::UPPER))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn isxdigit(c: c_int) -> c_int {
    ret::truth(locale::byte_class(c).intersects(Class::XDIGIT))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __isxdigit_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::XDIGIT))
}}

export! { "GLIBC_2.3";
pub extern "C" fn isxdigit_l(c: c_int, loc: locale_t) -> c_int {
    ret::truth(locale::byte_class_l(c, conv::addr(loc)).intersects(Class::XDIGIT))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn iswxdigit(wc: wint_t) -> c_int {
    ret::truth(locale::view().isw(wc, Class::XDIGIT))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __iswxdigit_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::XDIGIT))
}}

export! { "GLIBC_2.3";
pub extern "C" fn iswxdigit_l(wc: wint_t, loc: locale_t) -> c_int {
    ret::truth(locale::view_of(conv::addr(loc)).isw(wc, Class::XDIGIT))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn isascii(c: c_int) -> c_int {
    ret::truth(ctype_tables::isascii(c))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __isascii_l(c: c_int, _loc: locale_t) -> c_int {
    ret::truth(ctype_tables::isascii(c))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn toascii(c: c_int) -> c_int {
    ctype_tables::toascii(c)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __toascii_l(c: c_int, _loc: locale_t) -> c_int {
    ctype_tables::toascii(c)
}}

export! { "GLIBC_2.3";
pub extern "C" fn isctype(c: c_int, mask: c_int) -> c_int {
    ret::truth(u16::try_from(mask).is_ok_and(|m| locale::byte_class(c).intersects(Class(m))))
}}

export! { "GLIBC_2.3";
pub extern "C" fn __isctype(c: c_int, mask: c_int) -> c_int {
    ret::truth(u16::try_from(mask).is_ok_and(|m| locale::byte_class(c).intersects(Class(m))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn tolower(c: c_int) -> c_int {
    locale::tolower(c)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _tolower(c: c_int) -> c_int {
    locale::tolower(c)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __tolower_l(c: c_int, loc: locale_t) -> c_int {
    locale::case_map_l(c, conv::addr(loc), false)
}}

export! { "GLIBC_2.3";
pub extern "C" fn tolower_l(c: c_int, loc: locale_t) -> c_int {
    locale::case_map_l(c, conv::addr(loc), false)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn towlower(wc: wint_t) -> wint_t {
    locale::view().wtrans(wc, false)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __towlower_l(wc: wint_t, loc: locale_t) -> wint_t {
    locale::view_of(conv::addr(loc)).wtrans(wc, false)
}}

export! { "GLIBC_2.3";
pub extern "C" fn towlower_l(wc: wint_t, loc: locale_t) -> wint_t {
    locale::view_of(conv::addr(loc)).wtrans(wc, false)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn toupper(c: c_int) -> c_int {
    locale::toupper(c)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _toupper(c: c_int) -> c_int {
    locale::toupper(c)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __toupper_l(c: c_int, loc: locale_t) -> c_int {
    locale::case_map_l(c, conv::addr(loc), true)
}}

export! { "GLIBC_2.3";
pub extern "C" fn toupper_l(c: c_int, loc: locale_t) -> c_int {
    locale::case_map_l(c, conv::addr(loc), true)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn towupper(wc: wint_t) -> wint_t {
    locale::view().wtrans(wc, true)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __towupper_l(wc: wint_t, loc: locale_t) -> wint_t {
    locale::view_of(conv::addr(loc)).wtrans(wc, true)
}}

export! { "GLIBC_2.3";
pub extern "C" fn towupper_l(wc: wint_t, loc: locale_t) -> wint_t {
    locale::view_of(conv::addr(loc)).wtrans(wc, true)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn towctrans(wc: wint_t, desc: wctrans_t) -> wint_t {
    ctype_tables::towctrans(wc, conv::addr(desc), locale::current())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __towctrans(wc: wint_t, desc: wctrans_t) -> wint_t {
    ctype_tables::towctrans(wc, conv::addr(desc), locale::current())
}}

export! { "GLIBC_2.3";
pub extern "C" fn towctrans_l(wc: wint_t, desc: wctrans_t, loc: locale_t) -> wint_t {
    ctype_tables::towctrans(wc, conv::addr(desc), locale::of(conv::addr(loc)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __towctrans_l(wc: wint_t, desc: wctrans_t, loc: locale_t) -> wint_t {
    ctype_tables::towctrans(wc, conv::addr(desc), locale::of(conv::addr(loc)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wctrans(name: *const c_char) -> wctrans_t {
    // UNSAFE(slice) SAFETY: name is a string (§5.3).
    ret::ptr::<i32>(ret::must(ctype_tables::wctrans_of(unsafe { strz::strz(name) }, locale::current()))).cast_const()
}}

export! { "GLIBC_2.3";
pub extern "C" fn wctrans_l(name: *const c_char, loc: locale_t) -> wctrans_t {
    // UNSAFE(slice) SAFETY: name is a string (§5.3).
    ret::ptr::<i32>(ret::must(ctype_tables::wctrans_of(unsafe { strz::strz(name) }, locale::of(conv::addr(loc))))).cast_const()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wctrans_l(name: *const c_char, loc: locale_t) -> wctrans_t {
    // UNSAFE(slice) SAFETY: name is a string (§5.3).
    ret::ptr::<i32>(ret::must(ctype_tables::wctrans_of(unsafe { strz::strz(name) }, locale::of(conv::addr(loc))))).cast_const()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wctype(name: *const c_char) -> wctype_t {
    // UNSAFE(slice) SAFETY: name is a string (§5.2).
    ret::must(ctype_tables::wctype_of(unsafe { strz::strz(name) }, locale::current()))
}}

export! { "GLIBC_2.3";
pub extern "C" fn wctype_l(name: *const c_char, loc: locale_t) -> wctype_t {
    // UNSAFE(slice) SAFETY: name is a string (§5.2).
    ret::must(ctype_tables::wctype_of(unsafe { strz::strz(name) }, locale::of(conv::addr(loc))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wctype_l(name: *const c_char, loc: locale_t) -> wctype_t {
    // UNSAFE(slice) SAFETY: name is a string (§5.2).
    ret::must(ctype_tables::wctype_of(unsafe { strz::strz(name) }, locale::of(conv::addr(loc))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn iswctype(wc: wint_t, desc: wctype_t) -> c_int {
    ret::truth(ctype_tables::iswctype_view(wc, desc, locale::view()))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __iswctype(wc: wint_t, desc: wctype_t) -> c_int {
    ret::truth(ctype_tables::iswctype_view(wc, desc, locale::view()))
}}

export! { "GLIBC_2.3";
pub extern "C" fn iswctype_l(wc: wint_t, desc: wctype_t, loc: locale_t) -> c_int {
    ret::truth(ctype_tables::iswctype_view(wc, desc, locale::view_of(conv::addr(loc))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __iswctype_l(wc: wint_t, desc: wctype_t, loc: locale_t) -> c_int {
    ret::truth(ctype_tables::iswctype_view(wc, desc, locale::view_of(conv::addr(loc))))
}}

export! { "GLIBC_2.3";
pub extern "C" fn __ctype_b_loc() -> *mut *const c_ushort {
    ret::ptr(locale::ctype_slot(0))
}}

export! { "GLIBC_2.3";
pub extern "C" fn __ctype_tolower_loc() -> *mut *const i32 {
    ret::ptr(locale::ctype_slot(1))
}}

export! { "GLIBC_2.3";
pub extern "C" fn __ctype_toupper_loc() -> *mut *const i32 {
    ret::ptr(locale::ctype_slot(2))
}}
