// Specified from spec §6 and its Appendix B list. Generated stubs were replaced by real tests of
// the SAME NAME (see crates/spectest); `cargo xtask stubs` now only checks coverage.

//! Translation-layer tests for the exported symbols of spec §6 (37 entries).
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! The tests run in the C locale (D-6.7): ASCII, `MB_CUR_MAX` 1, every byte from 0x80
//! `EILSEQ`. `iconv` is tested with built-in codecs here; `iconv_products.rs` compares it with
//! the reference library, charmaps included.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(non_snake_case)]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use core::ptr::{null, null_mut};

use liella_abi::ctypes::{c_char, mbstate_t, wchar_t};
use liella_boundary::errno;
use liella_boundary::posix::s06_multibyte as s;
use liella_boundary::selftest::{self, ChildEnd};

const EILSEQ: i32 = 84;
const NEG1: usize = usize::MAX;
const NEG2: usize = usize::MAX - 1;
const WEOF: u32 = u32::MAX;
const SIGABRT: i32 = 6;

fn c(t: &[u8]) -> Vec<c_char> {
    t.iter().map(|&b| i8::from_le_bytes([b])).chain([0]).collect()
}

fn w(t: &str) -> Vec<wchar_t> {
    t.chars().map(|ch| ch as wchar_t).chain([0]).collect()
}

/// `f` fails with `EILSEQ`, returning `bad`.
fn eilseq<T: PartialEq + core::fmt::Debug>(bad: T, f: impl FnOnce() -> T) {
    errno::set_raw(0);
    assert_eq!(f(), bad);
    assert_eq!(errno::get(), EILSEQ);
}

fn aborts(body: fn(&liella_boundary::sys::CSys) -> i32) {
    assert_eq!(selftest::in_child_quiet(body).unwrap(), ChildEnd::Signaled(SIGABRT));
}

// ------------------------------------------------------------------------------ one character

#[test]
fn sym_mbrtowc() {
    let mut st = mbstate_t::default();
    let mut out: wchar_t = -7;
    assert_eq!(s::mbrtowc(Some(&mut out), c(b"Az").as_ptr(), 2, Some(&mut st)), 1);
    assert_eq!(out, 65);
    assert_eq!(s::mbrtowc(Some(&mut out), c(b"").as_ptr(), 1, Some(&mut st)), 0, "the null character");
    assert_eq!(out, 0);
    assert_eq!(s::mbrtowc(None, c(b"x").as_ptr(), 0, Some(&mut st)), NEG2, "n = 0: incomplete");
    eilseq(NEG1, || s::mbrtowc(Some(&mut out), c(&[0xC3, 0xA9]).as_ptr(), 2, Some(&mut st)));
    assert_eq!(s::mbrtowc(None, null(), 0, Some(&mut st)), 0, "s NULL resets");
    // C17 7.29.6.3.2: with s NULL the call is mbrtowc(NULL, "", 1, ps); pwc is ignored.
    out = -7;
    assert_eq!(s::mbrtowc(Some(&mut out), null(), 5, Some(&mut st)), 0);
    assert_eq!(out, -7, "nothing is stored through pwc for a NULL s");
    assert_eq!(s::mbrtowc(None, c(b"q").as_ptr(), 1, None), 1, "NULL state: the hidden one");
}

/// AUD-M4: the single-character forms read only the bytes of the character, not all of `n`
/// (the reference returns 1 for an "A" that ends a page before an inaccessible one, `n`
/// 100000; probe 551e4d076fae). Scanning `n` faulted, and made a loop over a buffer O(N^2).
#[test]
fn single_character_forms_read_no_byte_past_the_character() {
    // Trace: s6_2_c_1 ("at most `n` bytes": only the character's bytes are read); D-6.10.
    use liella_boundary::sys::CSys;
    use liella_kernel::sys::Sys;
    use liella_kernel::uapi::{MAP_ANONYMOUS, MAP_PRIVATE, PROT_NONE, PROT_READ, PROT_WRITE};
    const PAGE: usize = 4096;
    let end = selftest::in_child_quiet(|_| {
        let r = CSys.mmap(0, 2 * PAGE, PROT_READ | PROT_WRITE, MAP_PRIVATE | MAP_ANONYMOUS, None, 0).unwrap();
        let (lo, mut hi) = r.split_at(PAGE).ok().unwrap();
        CSys.mprotect(&mut hi, PROT_NONE).unwrap();
        let a = (lo.addr() + PAGE - 1) as *mut c_char;
        liella_boundary::posix::s04_strings::memset(a.cast(), i32::from(b'A'), 1);
        let n = 100_000;
        let mut st = mbstate_t::default();
        let mut wc: wchar_t = 0;
        let mut c16 = 0u16;
        let mut c32 = 0u32;
        let mut c8 = 0u8;
        let got = [
            s::mbrtowc(Some(&mut wc), a, n, Some(&mut st)),
            s::mbrlen(a, n, Some(&mut st)),
            s::mbrtoc32(Some(&mut c32), a, n, Some(&mut st)),
            s::mbrtoc16(Some(&mut c16), a, n, Some(&mut st)),
            s::mbrtoc8(Some(&mut c8), a, n, Some(&mut st)),
            s::mbtowc(Some(&mut wc), a, n) as usize,
            s::mblen(a, n) as usize,
        ];
        i32::from(got != [1; 7] || wc != 65 || c32 != 65 || c16 != 65 || c8 != 65)
    })
    .unwrap();
    assert_eq!(end, ChildEnd::Exited(0), "every form returns 1 and stores 'A' without touching the next page");
}

#[test]
fn sym___mbrtowc() {
    let mut out: wchar_t = 0;
    assert_eq!(s::__mbrtowc(Some(&mut out), c(b"~").as_ptr(), 1, None), 1);
    assert_eq!(out, 0x7e);
}

#[test]
fn sym_mbrtoc32() {
    let mut out = 0u32;
    assert_eq!(s::mbrtoc32(Some(&mut out), c(b"k").as_ptr(), 4, None), 1);
    assert_eq!(out, u32::from(b'k'));
    eilseq(NEG1, || s::mbrtoc32(Some(&mut out), c(&[0x80]).as_ptr(), 1, None));
}

#[test]
fn sym_mbrtoc16() {
    let mut st = mbstate_t::default();
    let mut out = 0u16;
    assert_eq!(s::mbrtoc16(Some(&mut out), c(b"Z").as_ptr(), 1, Some(&mut st)), 1);
    assert_eq!(out, u16::from(b'Z'));
    assert_eq!(s::mbrtoc16(Some(&mut out), c(b"").as_ptr(), 1, Some(&mut st)), 0);
    eilseq(NEG1, || s::mbrtoc16(Some(&mut out), c(&[0xF0, 0x9F, 0x98, 0x80]).as_ptr(), 4, Some(&mut st)));
}

#[test]
fn sym_mbrtoc8() {
    let mut st = mbstate_t::default();
    let mut out = 0u8;
    assert_eq!(s::mbrtoc8(Some(&mut out), c(b"r").as_ptr(), 1, Some(&mut st)), 1);
    assert_eq!(out, b'r');
    eilseq(NEG1, || s::mbrtoc8(Some(&mut out), c(&[0xE2]).as_ptr(), 1, Some(&mut st)));
}

#[test]
fn sym_mbrlen() {
    let mut st = mbstate_t::default();
    assert_eq!(s::mbrlen(c(b"ab").as_ptr(), 2, Some(&mut st)), 1);
    assert_eq!(s::mbrlen(c(b"").as_ptr(), 1, Some(&mut st)), 0);
    eilseq(NEG1, || s::mbrlen(c(&[0xFF]).as_ptr(), 1, Some(&mut st)));
}

#[test]
fn sym___mbrlen() {
    assert_eq!(s::__mbrlen(c(b"x").as_ptr(), 1, None), 1);
}

#[test]
fn sym_mblen() {
    assert_eq!(s::mblen(null(), 0), 0, "NULL: no shift states");
    assert_eq!(s::mblen(c(b"a").as_ptr(), 1), 1);
    assert_eq!(s::mblen(c(b"").as_ptr(), 1), 0);
    eilseq(-1, || s::mblen(c(&[0xC3]).as_ptr(), 1));
}

#[test]
fn sym_mbtowc() {
    let mut out: wchar_t = 0;
    assert_eq!(s::mbtowc(None, null(), 0), 0);
    assert_eq!(s::mbtowc(Some(&mut out), c(b"Q").as_ptr(), 1), 1);
    assert_eq!(out, 81);
    eilseq(-1, || s::mbtowc(Some(&mut out), c(&[0x90]).as_ptr(), 1));
}

#[test]
fn sym_wcrtomb() {
    let mut st = mbstate_t::default();
    let mut b = [0 as c_char; 4];
    assert_eq!(s::wcrtomb(b.as_mut_ptr(), 0x41, Some(&mut st)), 1);
    assert_eq!(b[0], 0x41);
    assert_eq!(s::wcrtomb(null_mut(), 0x41, Some(&mut st)), 1, "NULL s: as if encoding L'\\0'");
    eilseq(NEG1, || s::wcrtomb(b.as_mut_ptr(), 0xE9, Some(&mut st)));
    eilseq(NEG1, || s::wcrtomb(b.as_mut_ptr(), -5, None));
}

#[test]
fn sym___wcrtomb_chk() {
    let mut b = [0 as c_char; 1];
    assert_eq!(s::__wcrtomb_chk(b.as_mut_ptr(), 0x42, None, 1), 1);
    aborts(|_| {
        let mut b = [0 as c_char; 1];
        s::__wcrtomb_chk(b.as_mut_ptr(), 0x42, None, 0);
        0
    });
}

#[test]
fn sym_c32rtomb() {
    let mut b = [0 as c_char; 4];
    assert_eq!(s::c32rtomb(b.as_mut_ptr(), 0x7a, None), 1);
    assert_eq!(b[0], 0x7a);
    eilseq(NEG1, || s::c32rtomb(b.as_mut_ptr(), 0x110000, None));
}

#[test]
fn sym_c16rtomb() {
    let mut st = mbstate_t::default();
    let mut b = [0 as c_char; 4];
    assert_eq!(s::c16rtomb(b.as_mut_ptr(), 0x61, Some(&mut st)), 1);
    assert_eq!(s::c16rtomb(b.as_mut_ptr(), 0xD83D, Some(&mut st)), 0, "a high surrogate is stored");
    assert_eq!(s::mbsinit(Some(&st)), 0);
    eilseq(NEG1, || s::c16rtomb(b.as_mut_ptr(), 0xDE00, Some(&mut st)));
    eilseq(NEG1, || s::c16rtomb(b.as_mut_ptr(), 0xDC00, Some(&mut st)));
}

#[test]
fn sym_c8rtomb() {
    let mut st = mbstate_t::default();
    let mut b = [0 as c_char; 4];
    assert_eq!(s::c8rtomb(b.as_mut_ptr(), b'h', Some(&mut st)), 1);
    assert_eq!(s::c8rtomb(b.as_mut_ptr(), 0xC3, Some(&mut st)), 0, "a lead unit is stored");
    eilseq(NEG1, || s::c8rtomb(b.as_mut_ptr(), 0xA9, Some(&mut st)));
}

#[test]
fn sym_wctomb() {
    let mut b = [0 as c_char; 2];
    assert_eq!(s::wctomb(null_mut(), 0), 0, "NULL: no shift states");
    assert_eq!(s::wctomb(b.as_mut_ptr(), 0x5a), 1);
    assert_eq!(b[0], 0x5a);
    eilseq(-1, || s::wctomb(b.as_mut_ptr(), 0x263A));
}

#[test]
fn sym___wctomb_chk() {
    let mut b = [0 as c_char; 1];
    assert_eq!(s::__wctomb_chk(b.as_mut_ptr(), 0x30, 1), 1);
    aborts(|_| {
        let mut b = [0 as c_char; 1];
        s::__wctomb_chk(b.as_mut_ptr(), 0x30, 0);
        0
    });
}

#[test]
fn sym_btowc() {
    assert_eq!(s::btowc(0x41), 0x41);
    assert_eq!(s::btowc(0), 0);
    assert_eq!(s::btowc(0xC3), WEOF, "not a character on its own");
    assert_eq!(s::btowc(-1), WEOF, "EOF");
}

#[test]
fn sym_wctob() {
    assert_eq!(s::wctob(0x41), 0x41);
    assert_eq!(s::wctob(0xE9), -1);
    assert_eq!(s::wctob(WEOF), -1);
}

#[test]
fn sym_mbsinit() {
    assert_ne!(s::mbsinit(None), 0, "NULL counts as initial");
    assert_ne!(s::mbsinit(Some(&mbstate_t::default())), 0, "zero-initialised is initial");
    // A state a program wrote itself (gnulib's replacement converters): `__count`, the first 4
    // bytes of the header's layout, decides; `__value` does not (glibc 2.39 probe, T18).
    for k in 0..8 {
        let mut st = mbstate_t::default();
        st.bytes[k] = 0x80;
        assert_eq!(s::mbsinit(Some(&st)) == 0, k < 4, "byte {k} set");
    }
    // A pending state of this library's own keeps `__count` non-zero.
    let mut st = mbstate_t::default();
    let mut b = [0 as c_char; 4];
    assert_eq!(s::c16rtomb(b.as_mut_ptr(), 0xD83D, Some(&mut st)), 0, "a high surrogate is stored");
    assert_ne!(st.bytes[..4], [0; 4]);
    assert_eq!(s::mbsinit(Some(&st)), 0);
}

#[test]
fn sym___ctype_get_mb_cur_max() {
    assert_eq!(s::__ctype_get_mb_cur_max(), 1, "the C locale");
}

#[test]
fn sym_wcwidth() {
    assert_eq!(s::wcwidth(0x41), 1);
    assert_eq!(s::wcwidth(0), 0);
    assert_eq!(s::wcwidth(0x07), -1, "not printable");
    assert_eq!(s::wcwidth(0x4E00), -1, "not printable in the C locale");
}

#[test]
fn sym_wcswidth() {
    assert_eq!(s::wcswidth(w("abc").as_ptr(), 3), 3);
    assert_eq!(s::wcswidth(w("abc").as_ptr(), 2), 2, "the first n characters");
    assert_eq!(s::wcswidth(w("ab").as_ptr(), 9), 2, "stops at the terminator");
    assert_eq!(s::wcswidth(w("a\tb").as_ptr(), 3), -1);
}

// ------------------------------------------------------------------------------ strings

#[test]
fn sym_mbsrtowcs() {
    let text = c(b"hey");
    let mut src = text.as_ptr();
    let mut out = [9 as wchar_t; 5];
    assert_eq!(s::mbsrtowcs(out.as_mut_ptr(), Some(&mut src), 5, None), 3);
    assert_eq!(out[..4], [104, 101, 121, 0]);
    assert!(src.is_null(), "the terminator was converted");
    let mut src = text.as_ptr();
    assert_eq!(s::mbsrtowcs(out.as_mut_ptr(), Some(&mut src), 2, None), 2, "at most len units");
    assert_eq!(src, text.as_ptr().wrapping_add(2), "*src advances");
    let mut src = text.as_ptr();
    assert_eq!(s::mbsrtowcs(null_mut(), Some(&mut src), 0, None), 3, "NULL dst measures");
    assert_eq!(src, text.as_ptr(), "measuring leaves *src");
    let bad = c(b"a\x80b");
    let mut src = bad.as_ptr();
    eilseq(NEG1, || s::mbsrtowcs(out.as_mut_ptr(), Some(&mut src), 5, None));
    assert_eq!(src, bad.as_ptr().wrapping_add(1), "left at the invalid sequence");
}

#[test]
fn sym_mbsnrtowcs() {
    let text = c(b"abcd");
    let mut src = text.as_ptr();
    let mut out = [0 as wchar_t; 8];
    assert_eq!(s::mbsnrtowcs(out.as_mut_ptr(), Some(&mut src), 2, 8, None), 2, "nms limits the input");
    assert_eq!(src, text.as_ptr().wrapping_add(2));
    let mut src = text.as_ptr();
    assert_eq!(s::mbsnrtowcs(out.as_mut_ptr(), Some(&mut src), 99, 8, None), 4);
    assert!(src.is_null());
}

#[test]
fn sym___mbsrtowcs_chk() {
    let text = c(b"ok");
    let mut src = text.as_ptr();
    let mut out = [0 as wchar_t; 3];
    assert_eq!(s::__mbsrtowcs_chk(out.as_mut_ptr(), Some(&mut src), 3, None, 3), 2);
    aborts(|_| {
        let mut out = [0 as wchar_t; 2];
        let mut src: *const c_char = c"ok".as_ptr();
        s::__mbsrtowcs_chk(out.as_mut_ptr(), Some(&mut src), 3, None, 2);
        0
    });
}

#[test]
fn sym___mbsnrtowcs_chk() {
    let text = c(b"ok");
    let mut src = text.as_ptr();
    let mut out = [0 as wchar_t; 3];
    assert_eq!(s::__mbsnrtowcs_chk(out.as_mut_ptr(), Some(&mut src), 1, 3, None, 3), 1);
    aborts(|_| {
        let mut out = [0 as wchar_t; 1];
        let mut src: *const c_char = c"ok".as_ptr();
        s::__mbsnrtowcs_chk(out.as_mut_ptr(), Some(&mut src), 2, 2, None, 1);
        0
    });
}

#[test]
fn sym_mbstowcs() {
    let mut out = [9 as wchar_t; 4];
    assert_eq!(s::mbstowcs(out.as_mut_ptr(), c(b"ab").as_ptr(), 4), 2);
    assert_eq!(out[..3], [97, 98, 0]);
    assert_eq!(s::mbstowcs(null_mut(), c(b"abc").as_ptr(), 0), 3, "measures");
    eilseq(NEG1, || s::mbstowcs(out.as_mut_ptr(), c(&[0xC3, 0xA9]).as_ptr(), 4));
}

#[test]
fn sym___mbstowcs_chk() {
    let mut out = [0 as wchar_t; 4];
    assert_eq!(s::__mbstowcs_chk(out.as_mut_ptr(), c(b"ab").as_ptr(), 4, 4), 2);
    aborts(|_| {
        let mut out = [0 as wchar_t; 2];
        s::__mbstowcs_chk(out.as_mut_ptr(), c"ab".as_ptr(), 4, 2);
        0
    });
}

#[test]
fn sym_wcsrtombs() {
    let text = w("hi");
    let mut src = text.as_ptr();
    let mut out = [9 as c_char; 4];
    assert_eq!(s::wcsrtombs(out.as_mut_ptr(), Some(&mut src), 4, None), 2);
    assert_eq!(out[..3], [104, 105, 0]);
    assert!(src.is_null());
    let mut src = text.as_ptr();
    assert_eq!(s::wcsrtombs(out.as_mut_ptr(), Some(&mut src), 1, None), 1);
    assert_eq!(src, text.as_ptr().wrapping_add(1));
    let mut src = text.as_ptr();
    assert_eq!(s::wcsrtombs(null_mut(), Some(&mut src), 0, None), 2, "measures");
    let bad = w("a\u{e9}");
    let mut src = bad.as_ptr();
    eilseq(NEG1, || s::wcsrtombs(out.as_mut_ptr(), Some(&mut src), 4, None));
    assert_eq!(src, bad.as_ptr().wrapping_add(1));
}

#[test]
fn sym_wcsnrtombs() {
    let text = w("abc");
    let mut src = text.as_ptr();
    let mut out = [0 as c_char; 8];
    assert_eq!(s::wcsnrtombs(out.as_mut_ptr(), Some(&mut src), 2, 8, None), 2, "nwc limits the input");
    assert_eq!(src, text.as_ptr().wrapping_add(2));
}

#[test]
fn sym___wcsrtombs_chk() {
    let text = w("x");
    let mut src = text.as_ptr();
    let mut out = [0 as c_char; 2];
    assert_eq!(s::__wcsrtombs_chk(out.as_mut_ptr(), Some(&mut src), 2, None, 2), 1);
    aborts(|_| {
        let mut out = [0 as c_char; 1];
        let text = [120, 0];
        let mut src = text.as_ptr();
        s::__wcsrtombs_chk(out.as_mut_ptr(), Some(&mut src), 2, None, 1);
        0
    });
}

#[test]
fn sym___wcsnrtombs_chk() {
    let text = w("xy");
    let mut src = text.as_ptr();
    let mut out = [0 as c_char; 2];
    assert_eq!(s::__wcsnrtombs_chk(out.as_mut_ptr(), Some(&mut src), 1, 2, None, 2), 1);
    aborts(|_| {
        let mut out = [0 as c_char; 1];
        let text = [120, 0];
        let mut src = text.as_ptr();
        s::__wcsnrtombs_chk(out.as_mut_ptr(), Some(&mut src), 1, 2, None, 1);
        0
    });
}

#[test]
fn sym_wcstombs() {
    let mut out = [9 as c_char; 4];
    assert_eq!(s::wcstombs(out.as_mut_ptr(), w("ok").as_ptr(), 4), 2);
    assert_eq!(out[..3], [111, 107, 0]);
    assert_eq!(s::wcstombs(null_mut(), w("abc").as_ptr(), 0), 3);
    eilseq(NEG1, || s::wcstombs(out.as_mut_ptr(), w("\u{263a}").as_ptr(), 4));
}

#[test]
fn sym___wcstombs_chk() {
    let mut out = [0 as c_char; 3];
    assert_eq!(s::__wcstombs_chk(out.as_mut_ptr(), w("ok").as_ptr(), 3, 3), 2);
    aborts(|_| {
        let mut out = [0 as c_char; 2];
        let text = [111, 107, 0];
        s::__wcstombs_chk(out.as_mut_ptr(), text.as_ptr(), 3, 2);
        0
    });
}

/// `iconv` over `input` into `room` bytes: the result, errno, the input and output advanced.
fn iconv_run(cd: *mut core::ffi::c_void, input: &[u8], room: usize) -> (usize, i32, usize, Vec<u8>) {
    let mut inb = input.to_vec();
    let mut out = vec![0u8; room];
    let (mut ip, mut il) = (inb.as_mut_ptr().cast::<c_char>(), inb.len());
    let (mut op, mut ol) = (out.as_mut_ptr().cast::<c_char>(), room);
    errno::set_raw(0);
    let r = s::iconv(cd, Some(&mut ip), Some(&mut il), Some(&mut op), Some(&mut ol));
    let e = errno::get();
    assert_eq!(ip as usize - inb.as_ptr() as usize, input.len() - il, "*inbuf advanced with *inbytesleft");
    assert_eq!(op as usize - out.as_ptr() as usize, room - ol, "*outbuf advanced with *outbytesleft");
    out.truncate(room - ol);
    (r, e, input.len() - il, out)
}

#[test]
fn sym_iconv_open() {
    let cd = s::iconv_open(c(b"UTF-16LE").as_ptr(), c(b"utf-8").as_ptr());
    assert_ne!(cd as usize, usize::MAX);
    assert_eq!(s::iconv_close(cd), 0);
    errno::set_raw(0);
    assert_eq!(s::iconv_open(c(b"NO-SUCH-CHARSET").as_ptr(), c(b"UTF-8").as_ptr()) as usize, usize::MAX);
    assert_eq!(errno::get(), 22, "EINVAL");
    let cd = s::iconv_open(c(b"").as_ptr(), c(b"UTF-8").as_ptr());
    assert_ne!(cd as usize, usize::MAX, "the empty name: the locale's codeset (ASCII in C)");
    let (r, e, _, _) = iconv_run(cd, "é".as_bytes(), 8);
    assert_eq!((r, e), (NEG1, EILSEQ));
    assert_eq!(s::iconv_close(cd), 0);
}

#[test]
fn sym_iconv() {
    let cd = s::iconv_open(c(b"UTF-16").as_ptr(), c(b"UTF-8").as_ptr());
    assert_eq!(iconv_run(cd, b"AB", 16), (0, 0, 2, vec![0xFF, 0xFE, 0x41, 0, 0x42, 0]), "BOM, then the text");
    assert_eq!(iconv_run(cd, b"A", 16).3, vec![0x41, 0], "no BOM the second time");
    // The flush form writes no shift sequence and resets: the BOM comes again.
    let mut out = [0u8; 4];
    let (mut op, mut ol) = (out.as_mut_ptr().cast::<c_char>(), 4usize);
    assert_eq!(s::iconv(cd, None, None, Some(&mut op), Some(&mut ol)), 0);
    assert_eq!(ol, 4);
    assert_eq!(iconv_run(cd, b"A", 16).3, vec![0xFF, 0xFE, 0x41, 0]);
    assert_eq!(s::iconv(cd, None, None, None, None), 0, "reset");
    assert_eq!(iconv_run(cd, b"ABC", 5), (NEG1, 7, 1, vec![0xFF, 0xFE, 0x41, 0]), "E2BIG after the unit that fits");
    assert_eq!(s::iconv_close(cd), 0);
    let cd = s::iconv_open(c(b"ASCII//TRANSLIT").as_ptr(), c(b"UTF-8").as_ptr());
    assert_eq!(iconv_run(cd, "x€y".as_bytes(), 16), (1, 0, 5, b"xEURy".to_vec()), "one irreversible conversion");
    assert_eq!(iconv_run(cd, b"a\xc3", 16), (NEG1, 22, 1, b"a".to_vec()), "EINVAL: incomplete at the end");
    assert_eq!(s::iconv_close(cd), 0);
    errno::set_raw(0);
    assert_eq!(s::iconv(usize::MAX as *mut core::ffi::c_void, None, None, None, None), NEG1, "(iconv_t)-1");
    assert_eq!(errno::get(), 9, "EBADF");
}

#[test]
fn sym_iconv_close() {
    let cd = s::iconv_open(c(b"UTF-8").as_ptr(), c(b"ISO-8859-1").as_ptr());
    assert_eq!(s::iconv_close(cd), 0);
    errno::set_raw(0);
    assert_eq!(s::iconv_close(usize::MAX as *mut core::ffi::c_void), -1, "(iconv_t)-1 is not a descriptor");
    assert_eq!(errno::get(), 9, "EBADF");
}

#[test]
fn single_character_quick_path_matches_the_general_decoder() {
    // Trace: s6_2_c_1 (mbrtowc/mbrlen/mbrtoc32 results, stored character and state); D-6.10 (WL-HOT-SEQ3).
    // The quick path takes an all-zero state; a state whose __count is zero but whose __value is
    // not is the same initial state (mbsinit) yet goes the general way, so it is the oracle. Every
    // one- and two-byte input, n 0..=3, output and state NULL or not, in C and C.UTF-8.
    use liella_boundary::posix::s07_locale as l;
    std::thread::spawn(|| {
        for name in ["C", "C.UTF-8"] {
            let cname = c(name.as_bytes());
            let loc = l::newlocale(0x1FBF, cname.as_ptr(), null_mut());
            assert!(!loc.is_null(), "{name}");
            l::uselocale(loc);
            let oracle = [0, 0, 0, 0, 0x5a, 0, 0, 0];
            for b0 in 0..=255u8 {
                for b1 in 0..=255u8 {
                    if b0 < 0x80 && b1 != 0 {
                        continue; // one-byte characters: the second byte is never read
                    }
                    let text = [b0 as c_char, b1 as c_char, 0, 0];
                    for n in 0..=3usize {
                        let call = |init: Option<[u8; 8]>, out: bool| {
                            let mut st = init.map(|bytes| mbstate_t { bytes });
                            let mut wc: wchar_t = -7;
                            let mut c32 = 7u32;
                            let r = s::mbrtowc(out.then_some(&mut wc), text.as_ptr(), n, st.as_mut());
                            let after = st.map(|m| m.bytes);
                            let mut st2 = init.map(|bytes| mbstate_t { bytes });
                            let len = s::mbrlen(text.as_ptr(), n, st2.as_mut());
                            let mut st3 = init.map(|bytes| mbstate_t { bytes });
                            let r32 = s::mbrtoc32(out.then_some(&mut c32), text.as_ptr(), n, st3.as_mut());
                            if init.is_none() {
                                // reset the hidden states for the next case
                                s::mbrtowc(None, null(), 0, None);
                                s::mbrlen(null(), 0, None);
                                s::mbrtoc32(None, null(), 0, None);
                            }
                            (r, wc, after, len, r32, c32)
                        };
                        for out in [true, false] {
                            let want = call(Some(oracle), out);
                            let what = format!("{name} {b0:#04x} {b1:#04x} n={n} out={out}");
                            assert_eq!(call(Some([0; 8]), out), want, "zero state: {what}");
                            let (r, wc, _, len, r32, c32) = want;
                            let got = call(None, out);
                            assert_eq!((got.0, got.1, got.3, got.4, got.5), (r, wc, len, r32, c32), "hidden state: {what}");
                        }
                    }
                }
            }
            // A pending state (here the hidden ones) never takes the quick path: a lead byte, then
            // an ASCII byte, answers as with the caller's state.
            for second in [b'A', 0xa9, 0] {
                let (lead, next) = ([0xc3u8 as c_char, 0], [second as c_char, 0]);
                let mut st = mbstate_t::default();
                let mut wc: wchar_t = 0;
                let user = [s::mbrtowc(Some(&mut wc), lead.as_ptr(), 1, Some(&mut st)), s::mbrtowc(Some(&mut wc), next.as_ptr(), 1, Some(&mut st))];
                let mut st = mbstate_t::default();
                let user_len = [s::mbrlen(lead.as_ptr(), 1, Some(&mut st)), s::mbrlen(next.as_ptr(), 1, Some(&mut st))];
                let hidden = [s::mbrtowc(Some(&mut wc), lead.as_ptr(), 1, None), s::mbrtowc(Some(&mut wc), next.as_ptr(), 1, None)];
                let hidden_len = [s::mbrlen(lead.as_ptr(), 1, None), s::mbrlen(next.as_ptr(), 1, None)];
                let hidden32 = [s::mbrtoc32(None, lead.as_ptr(), 1, None), s::mbrtoc32(None, next.as_ptr(), 1, None)];
                s::mbrtowc(None, null(), 0, None);
                s::mbrlen(null(), 0, None);
                s::mbrtoc32(None, null(), 0, None);
                assert_eq!((hidden, hidden_len, hidden32), (user, user_len, user), "{name}: 0xc3 then {second:#04x}");
            }
            l::uselocale(usize::MAX as _);
            l::freelocale(loc);
        }
    })
    .join()
    .unwrap();
}
