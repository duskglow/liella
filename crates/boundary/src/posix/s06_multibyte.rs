//! Translation for spec §6: multibyte text and wide characters.
//!
//! The functions use the calling thread's current locale (phase 7); `iconv` converts through
//! `crate::iconv` (D-6.8, D-6.9). Hidden states (the non-restartable forms, a `NULL` `mbstate_t *`) are process-wide until
//! the phase-4 thread blocks.

use liella::text::{Encoding, MbState};
use core::ffi::c_void;

use liella_abi::ctypes::{c_char, c_int, char16_t, char32_t, char8_t, mbstate_t, size_t, wchar_t, wint_t};
use liella_legacy::mb_compat;
use liella_legacy::mb_state::{self, Slot};

use crate::conv;
use crate::iconv;
use crate::locale;
use crate::export;
use crate::ret;
use crate::strz;

/// The common single-character case of `mbrtowc`/`mbrtoc32`/`mbrlen` (WL-HOT-SEQ3, D-6.10):
/// `s` non-NULL, `n >= 1`, the state all zero bytes (caller's or hidden), and either a byte below
/// 0x80 in a UTF-8 or ASCII locale, or a complete well-formed two-byte character (lead
/// 0xC2..=0xDF, `n >= 2`) in a UTF-8 locale. The answer (return value, character) is the general
/// decoder's, and the state stays all zero, which is what it stores after a complete character
/// from the initial state - so nothing is written to it. Everything else (`NULL` `s`, `n == 0`,
/// a pending or non-zero state, other bytes or charsets) is `None`: the general path decides.
#[inline(always)]
fn quick(s: *const c_char, n: size_t, ps: Option<&mbstate_t>, slot: Slot) -> Option<(size_t, u32)> {
    if s.is_null() || n == 0 || !ps.map_or_else(|| mb_state::is_zero(slot), |m| m.bytes == [0; 8]) {
        return None;
    }
    let enc = locale::current().encoding();
    // UNSAFE(slice): the first byte of the caller's text.
    #[allow(unsafe_code)]
    // SAFETY: s is not NULL and n >= 1, so byte 0 lies within the caller's limit (§6.2).
    let b0 = unsafe { s.cast::<u8>().read() };
    if b0 < 0x80 {
        return matches!(enc, Encoding::Utf8 | Encoding::Ascii).then_some((size_t::from(b0 != 0), u32::from(b0)));
    }
    if enc != Encoding::Utf8 || n < 2 || !(0xc2..=0xdf).contains(&b0) {
        return None;
    }
    // UNSAFE(slice): the second byte of the caller's text.
    #[allow(unsafe_code)]
    // SAFETY: n >= 2 and byte 0 is not the terminator, so byte 1 lies within the caller's limit.
    let b1 = unsafe { s.cast::<u8>().wrapping_add(1).read() };
    (b1 & 0xc0 == 0x80).then_some((2, (u32::from(b0 & 0x1f) << 6) | u32::from(b1 & 0x3f)))
}

/// `mbrtowc`/`__mbrtowc`/`mbrtoc32`: [`quick`], else the general decoder.
#[inline(always)]
fn to_wide<W: TryFrom<u32>>(pwc: Option<&mut W>, s: *const c_char, n: size_t, ps: Option<&mut mbstate_t>, slot: Slot) -> size_t {
    if let Some((r, c)) = quick(s, n, ps.as_deref(), slot) {
        if let (Some(o), Ok(w)) = (pwc, W::try_from(c)) {
            *o = w;
        }
        return r;
    }
    // UNSAFE(slice): the caller's text, read lazily.
    #[allow(unsafe_code)]
    // SAFETY: s is NULL or readable up to its terminator or n bytes; the outputs and ps are NULL or valid (§6.2).
    let text = unsafe { strz::text_n_lazy(s, n) };
    ret::size_or_neg1(mb_compat::mbrtowc(pwc, text, mb_compat::State::of(conv::state(ps), slot), locale::current()))
}

/// `mbrlen`/`__mbrlen`: [`quick`], else the general decoder.
#[inline(always)]
fn length(s: *const c_char, n: size_t, ps: Option<&mut mbstate_t>) -> size_t {
    if let Some((r, _)) = quick(s, n, ps.as_deref(), Slot::Mbrlen) {
        return r;
    }
    // UNSAFE(slice): the caller's text, read lazily.
    #[allow(unsafe_code)]
    // SAFETY: s is NULL or readable up to its terminator or n bytes; ps is NULL or valid (§6.2).
    let text = unsafe { strz::text_n_lazy(s, n) };
    ret::size_or_neg1(mb_compat::mbrlen(text, mb_compat::State::of(conv::state(ps), Slot::Mbrlen), locale::current()))
}

export! { "GLIBC_2.2.5";
pub extern "C" fn mbrtowc(pwc: Option<&mut wchar_t>, s: *const c_char, n: size_t, ps: Option<&mut mbstate_t>) -> size_t {
    to_wide(pwc, s, n, ps, Slot::Mbrtowc)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __mbrtowc(pwc: Option<&mut wchar_t>, s: *const c_char, n: size_t, ps: Option<&mut mbstate_t>) -> size_t {
    to_wide(pwc, s, n, ps, Slot::Mbrtowc)
}}

export! { "GLIBC_2.16";
pub extern "C" fn mbrtoc32(pc32: Option<&mut char32_t>, s: *const c_char, n: size_t, ps: Option<&mut mbstate_t>) -> size_t {
    to_wide(pc32, s, n, ps, Slot::C32)
}}

export! { "GLIBC_2.16";
pub extern "C" fn mbrtoc16(pc16: Option<&mut char16_t>, s: *const c_char, n: size_t, ps: Option<&mut mbstate_t>) -> size_t {
    // UNSAFE(slice) SAFETY: s is NULL or readable up to its terminator or n bytes; the outputs and ps are NULL or valid (§6.2).
    ret::size_or_neg1(mb_compat::mbrtoc16(pc16, unsafe { strz::text_n_lazy(s, n) }, mb_compat::State::of(conv::state(ps), Slot::C16), locale::current()))
}}

export! { "GLIBC_2.36";
pub extern "C" fn mbrtoc8(pc8: Option<&mut char8_t>, s: *const c_char, n: size_t, ps: Option<&mut mbstate_t>) -> size_t {
    // UNSAFE(slice) SAFETY: s is NULL or readable up to its terminator or n bytes; the outputs and ps are NULL or valid (§6.2).
    ret::size_or_neg1(mb_compat::mbrtoc8(pc8, unsafe { strz::text_n_lazy(s, n) }, mb_compat::State::of(conv::state(ps), Slot::C8), locale::current()))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mbrlen(s: *const c_char, n: size_t, ps: Option<&mut mbstate_t>) -> size_t {
    length(s, n, ps)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __mbrlen(s: *const c_char, n: size_t, ps: Option<&mut mbstate_t>) -> size_t {
    length(s, n, ps)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mblen(s: *const c_char, n: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: s is NULL or readable up to its terminator or n bytes (§6.2).
    ret::int(mb_compat::mbtowc::<i32>(None, unsafe { strz::text_n_lazy(s, n) }, Slot::Mblen, locale::current()))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mbtowc(pwc: Option<&mut wchar_t>, s: *const c_char, n: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: s is NULL or readable up to its terminator or n bytes; pwc is NULL or valid (§6.2).
    ret::int(mb_compat::mbtowc(pwc, unsafe { strz::text_n_lazy(s, n) }, Slot::Mbtowc, locale::current()))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcrtomb(s: *mut c_char, wc: wchar_t, ps: Option<&mut mbstate_t>) -> size_t {
    // UNSAFE(slice) SAFETY: s is NULL or has room for MB_CUR_MAX bytes; ps is NULL or valid (§6.2).
    ret::size_or_neg1(mb_compat::wcrtomb(unsafe { conv::opt_dest::<u8, _>(s) }, conv::wc(wc), mb_compat::State::of(conv::state(ps), Slot::Wcrtomb), locale::current()))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __wcrtomb_chk(s: *mut c_char, wc: wchar_t, ps: Option<&mut mbstate_t>, buflen: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: as wcrtomb; a buffer smaller than MB_CUR_MAX fails the fortify check.
    ret::size_or_neg1(mb_compat::wcrtomb(unsafe { conv::opt_dest_chk::<u8, _>(s, conv::chk(mb_compat::mb_cur_max(locale::current()), buflen)) }, conv::wc(wc), mb_compat::State::of(conv::state(ps), Slot::Wcrtomb), locale::current()))
}}

export! { "GLIBC_2.16";
pub extern "C" fn c32rtomb(s: *mut c_char, c32: char32_t, ps: Option<&mut mbstate_t>) -> size_t {
    // UNSAFE(slice) SAFETY: s is NULL or has room for MB_CUR_MAX bytes; ps is NULL or valid (§6.2).
    ret::size_or_neg1(mb_compat::wcrtomb(unsafe { conv::opt_dest::<u8, _>(s) }, c32, mb_compat::State::of(conv::state(ps), Slot::C32), locale::current()))
}}

export! { "GLIBC_2.16";
pub extern "C" fn c16rtomb(s: *mut c_char, c16: char16_t, ps: Option<&mut mbstate_t>) -> size_t {
    // UNSAFE(slice) SAFETY: s is NULL or has room for MB_CUR_MAX bytes; ps is NULL or valid (§6.2).
    ret::size_or_neg1(mb_compat::c16rtomb(unsafe { conv::opt_dest::<u8, _>(s) }, c16, mb_compat::State::of(conv::state(ps), Slot::C16), locale::current()))
}}

export! { "GLIBC_2.36";
pub extern "C" fn c8rtomb(s: *mut c_char, c8: char8_t, ps: Option<&mut mbstate_t>) -> size_t {
    // UNSAFE(slice) SAFETY: s is NULL or has room for MB_CUR_MAX bytes; ps is NULL or valid (§6.2).
    ret::size_or_neg1(mb_compat::c8rtomb(unsafe { conv::opt_dest::<u8, _>(s) }, c8, mb_compat::State::of(conv::state(ps), Slot::C8), locale::current()))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wctomb(s: *mut c_char, wc: wchar_t) -> c_int {
    // UNSAFE(slice) SAFETY: s is NULL or has room for MB_CUR_MAX bytes (§6.2).
    ret::int(mb_compat::wctomb(unsafe { conv::opt_dest::<u8, _>(s) }, conv::wc(wc), locale::current()))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __wctomb_chk(s: *mut c_char, wc: wchar_t, buflen: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: as wctomb; a buffer smaller than MB_CUR_MAX fails the fortify check.
    ret::int(mb_compat::wctomb(unsafe { conv::opt_dest_chk::<u8, _>(s, conv::chk(mb_compat::mb_cur_max(locale::current()), buflen)) }, conv::wc(wc), locale::current()))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn btowc(c: c_int) -> wint_t {
    mb_compat::btowc(c, locale::current())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wctob(wc: wint_t) -> c_int {
    mb_compat::wctob(wc, locale::current())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mbsinit(ps: Option<&mbstate_t>) -> c_int {
    ret::truth(mb_compat::mbsinit(conv::state_ref(ps)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __ctype_get_mb_cur_max() -> size_t {
    mb_compat::mb_cur_max(locale::current())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcwidth(wc: wchar_t) -> c_int {
    u32::try_from(wc).map_or(-1, |c| locale::view().width(c))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcswidth(s: *const wchar_t, n: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: s is readable up to its terminator or n units (§6.5).
    ret::must(unsafe { strz::wcsz_n(s, n) }.map(|t| mb_compat::wcswidth(t, locale::current())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mbsrtowcs(dst: *mut wchar_t, src: Option<&mut *const c_char>, len: size_t, ps: Option<&mut mbstate_t>) -> size_t {
    // UNSAFE(slice) SAFETY: *src is a string, dst is NULL or has room for len units, src and ps are valid or ps NULL (§6.3).
    ret::size_or_neg1(conv::const_cell(src).and_then(|c| unsafe { conv::opt_buf_w_mut(dst, len) }.and_then(|o| mb_compat::mbsrtowcs(c, usize::MAX, o, mb_compat::State::of(conv::state(ps), Slot::Mbsrtowcs), unsafe { conv::ctext() }, locale::current()))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mbsnrtowcs(dst: *mut wchar_t, src: Option<&mut *const c_char>, nms: size_t, len: size_t, ps: Option<&mut mbstate_t>) -> size_t {
    // UNSAFE(slice) SAFETY: *src is a string, dst is NULL or has room for len units, src and ps are valid or ps NULL (§6.3).
    ret::size_or_neg1(conv::const_cell(src).and_then(|c| unsafe { conv::opt_buf_w_mut(dst, len) }.and_then(|o| mb_compat::mbsrtowcs(c, nms, o, mb_compat::State::of(conv::state(ps), Slot::Mbsrtowcs), unsafe { conv::ctext() }, locale::current()))))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __mbsrtowcs_chk(dst: *mut wchar_t, src: Option<&mut *const c_char>, len: size_t, ps: Option<&mut mbstate_t>, dstlen: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: as mbsrtowcs, after the fortify check (dstlen in wchar_t units).
    ret::size_or_neg1(conv::const_cell(src).and_then(|c| unsafe { conv::opt_buf_w_mut(dst, conv::chk(len, dstlen)) }.and_then(|o| mb_compat::mbsrtowcs(c, usize::MAX, o, mb_compat::State::of(conv::state(ps), Slot::Mbsrtowcs), unsafe { conv::ctext() }, locale::current()))))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __mbsnrtowcs_chk(dst: *mut wchar_t, src: Option<&mut *const c_char>, nms: size_t, len: size_t, ps: Option<&mut mbstate_t>, dstlen: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: as mbsnrtowcs, after the fortify check (dstlen in wchar_t units).
    ret::size_or_neg1(conv::const_cell(src).and_then(|c| unsafe { conv::opt_buf_w_mut(dst, conv::chk(len, dstlen)) }.and_then(|o| mb_compat::mbsrtowcs(c, nms, o, mb_compat::State::of(conv::state(ps), Slot::Mbsrtowcs), unsafe { conv::ctext() }, locale::current()))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mbstowcs(dst: *mut wchar_t, src: *const c_char, len: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: src is a string, dst is NULL or has room for len units (§6.3).
    ret::size_or_neg1(unsafe { conv::opt_buf_w_mut(dst, len) }.and_then(|o| mb_compat::mbsrtowcs(mb_compat::Local(conv::addr(src)), usize::MAX, o, mb_compat::State::Fresh(MbState::INITIAL), unsafe { conv::ctext() }, locale::current())))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __mbstowcs_chk(dst: *mut wchar_t, src: *const c_char, len: size_t, dstlen: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: as mbstowcs, after the fortify check (dstlen in wchar_t units).
    ret::size_or_neg1(unsafe { conv::opt_buf_w_mut(dst, conv::chk(len, dstlen)) }.and_then(|o| mb_compat::mbsrtowcs(mb_compat::Local(conv::addr(src)), usize::MAX, o, mb_compat::State::Fresh(MbState::INITIAL), unsafe { conv::ctext() }, locale::current())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcsrtombs(dst: *mut c_char, src: Option<&mut *const wchar_t>, len: size_t, ps: Option<&mut mbstate_t>) -> size_t {
    // UNSAFE(slice) SAFETY: *src is a wide string, dst is NULL or has room for len bytes, src and ps are valid or ps NULL (§6.3).
    ret::size_or_neg1(conv::const_cell(src).and_then(|c| unsafe { conv::opt_buf_mut(dst, len) }.and_then(|o| mb_compat::wcsrtombs(c, usize::MAX, o, mb_compat::State::of(conv::state(ps), Slot::Wcsrtombs), unsafe { conv::ctext() }, locale::current()))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcsnrtombs(dst: *mut c_char, src: Option<&mut *const wchar_t>, nwc: size_t, len: size_t, ps: Option<&mut mbstate_t>) -> size_t {
    // UNSAFE(slice) SAFETY: *src is a wide string, dst is NULL or has room for len bytes, src and ps are valid or ps NULL (§6.3).
    ret::size_or_neg1(conv::const_cell(src).and_then(|c| unsafe { conv::opt_buf_mut(dst, len) }.and_then(|o| mb_compat::wcsrtombs(c, nwc, o, mb_compat::State::of(conv::state(ps), Slot::Wcsrtombs), unsafe { conv::ctext() }, locale::current()))))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __wcsrtombs_chk(dst: *mut c_char, src: Option<&mut *const wchar_t>, len: size_t, ps: Option<&mut mbstate_t>, dstlen: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: as wcsrtombs, after the fortify check (dstlen in bytes).
    ret::size_or_neg1(conv::const_cell(src).and_then(|c| unsafe { conv::opt_buf_mut(dst, conv::chk(len, dstlen)) }.and_then(|o| mb_compat::wcsrtombs(c, usize::MAX, o, mb_compat::State::of(conv::state(ps), Slot::Wcsrtombs), unsafe { conv::ctext() }, locale::current()))))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __wcsnrtombs_chk(dst: *mut c_char, src: Option<&mut *const wchar_t>, nwc: size_t, len: size_t, ps: Option<&mut mbstate_t>, dstlen: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: as wcsnrtombs, after the fortify check (dstlen in bytes).
    ret::size_or_neg1(conv::const_cell(src).and_then(|c| unsafe { conv::opt_buf_mut(dst, conv::chk(len, dstlen)) }.and_then(|o| mb_compat::wcsrtombs(c, nwc, o, mb_compat::State::of(conv::state(ps), Slot::Wcsrtombs), unsafe { conv::ctext() }, locale::current()))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstombs(dst: *mut c_char, src: *const wchar_t, len: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: src is a wide string, dst is NULL or has room for len bytes (§6.3).
    ret::size_or_neg1(unsafe { conv::opt_buf_mut(dst, len) }.and_then(|o| mb_compat::wcsrtombs(mb_compat::Local(conv::addr(src)), usize::MAX, o, mb_compat::State::Fresh(MbState::INITIAL), unsafe { conv::ctext() }, locale::current())))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __wcstombs_chk(dst: *mut c_char, src: *const wchar_t, len: size_t, dstlen: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: as wcstombs, after the fortify check (dstlen in bytes).
    ret::size_or_neg1(unsafe { conv::opt_buf_mut(dst, conv::chk(len, dstlen)) }.and_then(|o| mb_compat::wcsrtombs(mb_compat::Local(conv::addr(src)), usize::MAX, o, mb_compat::State::Fresh(MbState::INITIAL), unsafe { conv::ctext() }, locale::current())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn iconv_open(tocode: *const c_char, fromcode: *const c_char) -> *mut c_void {
    // UNSAFE(slice) SAFETY: tocode and fromcode are strings (§6.4).
    ret::addr_or_neg1(iconv::open(unsafe { strz::strz(tocode) }, unsafe { strz::strz(fromcode) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn iconv(cd: *mut c_void, inbuf: Option<&mut *mut c_char>, inbytesleft: Option<&mut size_t>, outbuf: Option<&mut *mut c_char>, outbytesleft: Option<&mut size_t>) -> size_t {
    // UNSAFE(slice) SAFETY: *inbuf has *inbytesleft readable bytes and *outbuf *outbytesleft writable ones, distinct (§6.4).
    ret::size_or_neg1(iconv::convert(conv::addr(cd), unsafe { conv::iconv_args(inbuf, inbytesleft, outbuf, outbytesleft) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn iconv_close(cd: *mut c_void) -> c_int {
    ret::zero(iconv::close(conv::addr(cd)))
}}
