// Specified from spec §5 and its Appendix B list. Generated stubs were replaced by real tests of
// the SAME NAME (see crates/spectest); `cargo xtask stubs` now only checks coverage.

//! Translation-layer tests for the exported symbols of spec §5 (115 entries).
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! The reference classification is the C locale's (D-6.7): ASCII only, every byte from 0x80 and
//! `EOF` in no class. Tables are read through the `memcpy` export, so the tests need no
//! `unsafe` to look at memory the library owns.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(non_snake_case)]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use core::ptr::{null, null_mut};
use core::sync::atomic::Ordering::Relaxed;

use liella_abi::ctypes::{c_char, c_int, wint_t};
use liella_boundary::data;
use liella_boundary::posix::s04_strings as mem;
use liella_boundary::posix::s05_ctype as s;

const WEOF: wint_t = u32::MAX;

// `_ISbit` values (spec Appendix C.9).
const fn isbit(b: u16) -> u16 {
    if b < 8 { (1 << b) << 8 } else { (1 << b) >> 8 }
}
const ALPHA: u16 = isbit(2);
const DIGIT: u16 = isbit(3);
const BLANK: u16 = isbit(8);

fn bit(name: &str) -> u16 {
    let order = ["upper", "lower", "alpha", "digit", "xdigit", "space", "print", "graph", "blank", "cntrl", "punct", "alnum"];
    isbit(order.iter().position(|&n| n == name).unwrap() as u16)
}

fn iswbit(b: usize) -> u32 {
    let v = 1u32 << b;
    match b {
        0..=7 => v << 24,
        8..=15 => v << 8,
        16..=23 => v >> 8,
        _ => v >> 24,
    }
}

/// The C locale's classes (D-6.7).
fn reference(name: &str, ch: i32) -> bool {
    let Ok(b) = u8::try_from(ch) else { return false };
    match name {
        "alnum" => b.is_ascii_alphanumeric(),
        "alpha" => b.is_ascii_alphabetic(),
        "blank" => b == b' ' || b == b'\t',
        "cntrl" => b < 0x20 || b == 0x7f,
        "digit" => b.is_ascii_digit(),
        "graph" => (0x21..=0x7e).contains(&b),
        "lower" => b.is_ascii_lowercase(),
        "print" => (0x20..=0x7e).contains(&b),
        "punct" => b.is_ascii_punctuation(),
        "space" => b" \t\n\x0b\x0c\r".contains(&b),
        "upper" => b.is_ascii_uppercase(),
        "xdigit" => b.is_ascii_hexdigit(),
        _ => unreachable!(),
    }
}

fn narrow(name: &str, f: impl Fn(c_int) -> c_int) {
    for ch in -128..256 {
        assert_eq!(f(ch) != 0, reference(name, ch), "{name}({ch})");
    }
    assert_eq!(f(-1), 0, "{name}(EOF)");
}

fn wide(name: &str, f: impl Fn(wint_t) -> c_int) {
    for wc in 0..0x3000u32 {
        assert_eq!(f(wc) != 0, reference(name, i32::try_from(wc).unwrap()), "{name}({wc:#x})");
    }
    assert_eq!(f(WEOF), 0, "{name}(WEOF)");
}

fn case(f: impl Fn(c_int) -> c_int, r: fn(&u8) -> u8) {
    for ch in -128..256 {
        // A negative `char` other than EOF maps to the byte it aliases (c + 256), as in the
        // reference library's C tables; EOF (-1) maps to itself.
        let want = match ch {
            -1 => -1,
            c if c < 0 => c + 256,
            c => u8::try_from(c).map_or(c, |b| i32::from(r(&b))),
        };
        assert_eq!(f(ch), want, "{ch}");
    }
}

fn wcase(f: impl Fn(wint_t) -> wint_t, r: fn(&u8) -> u8) {
    for wc in 0..0x3000u32 {
        let want = u8::try_from(wc).ok().filter(u8::is_ascii).map_or(wc, |b| u32::from(r(&b)));
        assert_eq!(f(wc), want, "{wc:#x}");
    }
    assert_eq!(f(WEOF), WEOF);
}

fn c(t: &str) -> Vec<c_char> {
    t.bytes().map(|b| i8::from_le_bytes([b])).chain([0]).collect()
}

/// The pointer stored at `p` (library memory), copied out with `memcpy`.
fn read_ptr<T>(p: *mut *const T) -> *const T {
    let mut out: *const T = null();
    mem::memcpy((&raw mut out).cast(), p.cast_const().cast(), size_of::<*const T>());
    out
}

/// Entry `ch` of a table indexed from -128 (library memory), copied out with `memcpy`.
fn data_at<T: Default>(table: *const T, ch: i32) -> T {
    let mut out = T::default();
    let at = table.wrapping_offset(ch as isize);
    mem::memcpy((&raw mut out).cast(), at.cast(), size_of::<T>());
    out
}

#[test]
fn sym___ctype32_b() {
    // The wide table uses the _ISwbit encoding (<wctype.h>): bit b of the class list, bytes reversed.
    let table = data::__ctype32_b.load(Relaxed).cast_const();
    for ch in 0..256 {
        let mask = data_at(table, ch);
        for (b, name) in ["upper", "lower", "alpha", "digit", "xdigit", "space", "print", "graph", "blank", "cntrl", "punct", "alnum"].into_iter().enumerate() {
            assert_eq!(mask & iswbit(b) != 0, reference(name, ch), "{name} {ch}");
        }
    }
}

#[test]
fn sym___ctype32_tolower() {
    let table = data::__ctype32_tolower.load(Relaxed).cast_const();
    assert_eq!((data_at(table, i32::from(b'A')), data_at(table, i32::from(b'a'))), (i32::from(b'a'), i32::from(b'a')));
}

#[test]
fn sym___ctype32_toupper() {
    let table = data::__ctype32_toupper.load(Relaxed).cast_const();
    assert_eq!(data_at(table, i32::from(b'z')), i32::from(b'Z'));
}

#[test]
fn sym___ctype_b() {
    assert_eq!(data::__ctype_b.load(Relaxed).cast_const(), read_ptr(s::__ctype_b_loc()));
}

#[test]
fn sym___ctype_b_loc() {
    let p = s::__ctype_b_loc();
    assert_eq!(p, s::__ctype_b_loc(), "a stable location");
    let table = read_ptr(p);
    for ch in -128..256 {
        let mask = data_at(table, ch);
        for name in ["alnum", "alpha", "blank", "cntrl", "digit", "graph", "lower", "print", "punct", "space", "upper", "xdigit"] {
            assert_eq!(mask & bit(name) != 0, reference(name, ch), "{name} {ch}");
        }
    }
}

#[test]
fn sym___ctype_tolower() {
    assert_eq!(data::__ctype_tolower.load(Relaxed).cast_const(), read_ptr(s::__ctype_tolower_loc()));
}

#[test]
fn sym___ctype_tolower_loc() {
    let table = read_ptr(s::__ctype_tolower_loc());
    for ch in -128..256 {
        assert_eq!(data_at(table, ch), s::tolower(ch), "{ch}");
    }
}

#[test]
fn sym___ctype_toupper() {
    assert_eq!(data::__ctype_toupper.load(Relaxed).cast_const(), read_ptr(s::__ctype_toupper_loc()));
}

#[test]
fn sym___ctype_toupper_loc() {
    let table = read_ptr(s::__ctype_toupper_loc());
    for ch in -128..256 {
        assert_eq!(data_at(table, ch), s::toupper(ch), "{ch}");
    }
}

#[test]
fn sym_isalnum() {
    narrow("alnum", |c| s::isalnum(c));
}

#[test]
fn sym___isalnum_l() {
    narrow("alnum", |c| s::__isalnum_l(c, null_mut()));
}

#[test]
fn sym_isalnum_l() {
    narrow("alnum", |c| s::isalnum_l(c, null_mut()));
}

#[test]
fn sym_isalpha() {
    narrow("alpha", |c| s::isalpha(c));
}

#[test]
fn sym___isalpha_l() {
    narrow("alpha", |c| s::__isalpha_l(c, null_mut()));
}

#[test]
fn sym_isalpha_l() {
    narrow("alpha", |c| s::isalpha_l(c, null_mut()));
}

#[test]
fn sym_isascii() {
    for c in -200..400 {
        assert_eq!(s::isascii(c) != 0, (0..=127).contains(&c), "{c}");
    }
}

#[test]
fn sym___isascii_l() {
    assert_ne!(s::__isascii_l(127, null_mut()), 0);
    assert_eq!(s::__isascii_l(128, null_mut()), 0);
    assert_eq!(s::__isascii_l(-1, null_mut()), 0);
}

#[test]
fn sym_isblank() {
    narrow("blank", |c| s::isblank(c));
}

#[test]
fn sym___isblank_l() {
    narrow("blank", |c| s::__isblank_l(c, null_mut()));
}

#[test]
fn sym_isblank_l() {
    narrow("blank", |c| s::isblank_l(c, null_mut()));
}

#[test]
fn sym_iscntrl() {
    narrow("cntrl", |c| s::iscntrl(c));
}

#[test]
fn sym___iscntrl_l() {
    narrow("cntrl", |c| s::__iscntrl_l(c, null_mut()));
}

#[test]
fn sym_iscntrl_l() {
    narrow("cntrl", |c| s::iscntrl_l(c, null_mut()));
}

#[test]
fn sym___isctype() {
    assert_ne!(s::__isctype(c_int::from(b' '), c_int::from(BLANK)), 0);
    assert_eq!(s::__isctype(c_int::from(b'\n'), c_int::from(BLANK)), 0);
}

#[test]
fn sym_isctype() {
    let alpha_or_digit = c_int::from(ALPHA | DIGIT);
    assert_ne!(s::isctype(c_int::from(b'a'), alpha_or_digit), 0);
    assert_ne!(s::isctype(c_int::from(b'7'), alpha_or_digit), 0);
    assert_eq!(s::isctype(c_int::from(b'-'), alpha_or_digit), 0);
    assert_eq!(s::isctype(-1, alpha_or_digit), 0, "EOF");
}

#[test]
fn sym_isdigit() {
    narrow("digit", |c| s::isdigit(c));
}

#[test]
fn sym___isdigit_l() {
    narrow("digit", |c| s::__isdigit_l(c, null_mut()));
}

#[test]
fn sym_isdigit_l() {
    narrow("digit", |c| s::isdigit_l(c, null_mut()));
}

#[test]
fn sym_isgraph() {
    narrow("graph", |c| s::isgraph(c));
}

#[test]
fn sym___isgraph_l() {
    narrow("graph", |c| s::__isgraph_l(c, null_mut()));
}

#[test]
fn sym_isgraph_l() {
    narrow("graph", |c| s::isgraph_l(c, null_mut()));
}

#[test]
fn sym_islower() {
    narrow("lower", |c| s::islower(c));
}

#[test]
fn sym___islower_l() {
    narrow("lower", |c| s::__islower_l(c, null_mut()));
}

#[test]
fn sym_islower_l() {
    narrow("lower", |c| s::islower_l(c, null_mut()));
}

#[test]
fn sym_isprint() {
    narrow("print", |c| s::isprint(c));
}

#[test]
fn sym___isprint_l() {
    narrow("print", |c| s::__isprint_l(c, null_mut()));
}

#[test]
fn sym_isprint_l() {
    narrow("print", |c| s::isprint_l(c, null_mut()));
}

#[test]
fn sym_ispunct() {
    narrow("punct", |c| s::ispunct(c));
}

#[test]
fn sym___ispunct_l() {
    narrow("punct", |c| s::__ispunct_l(c, null_mut()));
}

#[test]
fn sym_ispunct_l() {
    narrow("punct", |c| s::ispunct_l(c, null_mut()));
}

#[test]
fn sym_isspace() {
    narrow("space", |c| s::isspace(c));
}

#[test]
fn sym___isspace_l() {
    narrow("space", |c| s::__isspace_l(c, null_mut()));
}

#[test]
fn sym_isspace_l() {
    narrow("space", |c| s::isspace_l(c, null_mut()));
}

#[test]
fn sym_isupper() {
    narrow("upper", |c| s::isupper(c));
}

#[test]
fn sym___isupper_l() {
    narrow("upper", |c| s::__isupper_l(c, null_mut()));
}

#[test]
fn sym_isupper_l() {
    narrow("upper", |c| s::isupper_l(c, null_mut()));
}

#[test]
fn sym_iswalnum() {
    wide("alnum", |c| s::iswalnum(c));
}

#[test]
fn sym___iswalnum_l() {
    wide("alnum", |c| s::__iswalnum_l(c, null_mut()));
}

#[test]
fn sym_iswalnum_l() {
    wide("alnum", |c| s::iswalnum_l(c, null_mut()));
}

#[test]
fn sym_iswalpha() {
    wide("alpha", |c| s::iswalpha(c));
}

#[test]
fn sym___iswalpha_l() {
    wide("alpha", |c| s::__iswalpha_l(c, null_mut()));
}

#[test]
fn sym_iswalpha_l() {
    wide("alpha", |c| s::iswalpha_l(c, null_mut()));
}

#[test]
fn sym_iswblank() {
    wide("blank", |c| s::iswblank(c));
}

#[test]
fn sym___iswblank_l() {
    wide("blank", |c| s::__iswblank_l(c, null_mut()));
}

#[test]
fn sym_iswblank_l() {
    wide("blank", |c| s::iswblank_l(c, null_mut()));
}

#[test]
fn sym_iswcntrl() {
    wide("cntrl", |c| s::iswcntrl(c));
}

#[test]
fn sym___iswcntrl_l() {
    wide("cntrl", |c| s::__iswcntrl_l(c, null_mut()));
}

#[test]
fn sym_iswcntrl_l() {
    wide("cntrl", |c| s::iswcntrl_l(c, null_mut()));
}

#[test]
fn sym___iswctype() {
    assert_ne!(s::__iswctype(u32::from('5'), s::wctype(c("xdigit").as_ptr())), 0);
}

#[test]
fn sym_iswctype() {
    let t = s::wctype(c("upper").as_ptr());
    assert_ne!(s::iswctype(u32::from('Q'), t), 0);
    assert_eq!(s::iswctype(u32::from('q'), t), 0);
    assert_eq!(s::iswctype(WEOF, t), 0, "WEOF is in no class");
    assert_eq!(s::iswctype(u32::from('Q'), 0), 0, "descriptor 0: no class");
}

#[test]
fn sym___iswctype_l() {
    assert_eq!(s::__iswctype_l(0xE9, s::wctype(c("alpha").as_ptr()), null_mut()), 0, "C locale: ASCII only");
}

#[test]
fn sym_iswctype_l() {
    assert_ne!(s::iswctype_l(u32::from(' '), s::wctype(c("space").as_ptr()), null_mut()), 0);
}

#[test]
fn sym_iswdigit() {
    wide("digit", |c| s::iswdigit(c));
}

#[test]
fn sym___iswdigit_l() {
    wide("digit", |c| s::__iswdigit_l(c, null_mut()));
}

#[test]
fn sym_iswdigit_l() {
    wide("digit", |c| s::iswdigit_l(c, null_mut()));
}

#[test]
fn sym_iswgraph() {
    wide("graph", |c| s::iswgraph(c));
}

#[test]
fn sym___iswgraph_l() {
    wide("graph", |c| s::__iswgraph_l(c, null_mut()));
}

#[test]
fn sym_iswgraph_l() {
    wide("graph", |c| s::iswgraph_l(c, null_mut()));
}

#[test]
fn sym_iswlower() {
    wide("lower", |c| s::iswlower(c));
}

#[test]
fn sym___iswlower_l() {
    wide("lower", |c| s::__iswlower_l(c, null_mut()));
}

#[test]
fn sym_iswlower_l() {
    wide("lower", |c| s::iswlower_l(c, null_mut()));
}

#[test]
fn sym_iswprint() {
    wide("print", |c| s::iswprint(c));
}

#[test]
fn sym___iswprint_l() {
    wide("print", |c| s::__iswprint_l(c, null_mut()));
}

#[test]
fn sym_iswprint_l() {
    wide("print", |c| s::iswprint_l(c, null_mut()));
}

#[test]
fn sym_iswpunct() {
    wide("punct", |c| s::iswpunct(c));
}

#[test]
fn sym___iswpunct_l() {
    wide("punct", |c| s::__iswpunct_l(c, null_mut()));
}

#[test]
fn sym_iswpunct_l() {
    wide("punct", |c| s::iswpunct_l(c, null_mut()));
}

#[test]
fn sym_iswspace() {
    wide("space", |c| s::iswspace(c));
}

#[test]
fn sym___iswspace_l() {
    wide("space", |c| s::__iswspace_l(c, null_mut()));
}

#[test]
fn sym_iswspace_l() {
    wide("space", |c| s::iswspace_l(c, null_mut()));
}

#[test]
fn sym_iswupper() {
    wide("upper", |c| s::iswupper(c));
}

#[test]
fn sym___iswupper_l() {
    wide("upper", |c| s::__iswupper_l(c, null_mut()));
}

#[test]
fn sym_iswupper_l() {
    wide("upper", |c| s::iswupper_l(c, null_mut()));
}

#[test]
fn sym_iswxdigit() {
    wide("xdigit", |c| s::iswxdigit(c));
}

#[test]
fn sym___iswxdigit_l() {
    wide("xdigit", |c| s::__iswxdigit_l(c, null_mut()));
}

#[test]
fn sym_iswxdigit_l() {
    wide("xdigit", |c| s::iswxdigit_l(c, null_mut()));
}

#[test]
fn sym_isxdigit() {
    narrow("xdigit", |c| s::isxdigit(c));
}

#[test]
fn sym___isxdigit_l() {
    narrow("xdigit", |c| s::__isxdigit_l(c, null_mut()));
}

#[test]
fn sym_isxdigit_l() {
    narrow("xdigit", |c| s::isxdigit_l(c, null_mut()));
}

#[test]
fn sym_toascii() {
    assert_eq!(s::toascii(0xC1), 0x41, "masks to 7 bits");
    assert_eq!(s::toascii(-1), 0x7f);
}

#[test]
fn sym___toascii_l() {
    assert_eq!(s::__toascii_l(0x1FF, null_mut()), 0x7f);
}

#[test]
fn sym__tolower() {
    case(|c| s::_tolower(c), u8::to_ascii_lowercase);
}

#[test]
fn sym_tolower() {
    case(|c| s::tolower(c), u8::to_ascii_lowercase);
}

#[test]
fn sym___tolower_l() {
    case(|c| s::__tolower_l(c, null_mut()), u8::to_ascii_lowercase);
}

#[test]
fn sym_tolower_l() {
    case(|c| s::tolower_l(c, null_mut()), u8::to_ascii_lowercase);
}

#[test]
fn sym__toupper() {
    case(|c| s::_toupper(c), u8::to_ascii_uppercase);
}

#[test]
fn sym_toupper() {
    case(|c| s::toupper(c), u8::to_ascii_uppercase);
}

#[test]
fn sym___toupper_l() {
    case(|c| s::__toupper_l(c, null_mut()), u8::to_ascii_uppercase);
}

#[test]
fn sym_toupper_l() {
    case(|c| s::toupper_l(c, null_mut()), u8::to_ascii_uppercase);
}

#[test]
fn sym___towctrans() {
    assert_eq!(s::__towctrans(u32::from('q'), s::wctrans(c("toupper").as_ptr())), u32::from('Q'));
}

#[test]
fn sym_towctrans() {
    let up = s::wctrans(c("toupper").as_ptr());
    let low = s::wctrans(c("tolower").as_ptr());
    assert_eq!((s::towctrans(u32::from('a'), up), s::towctrans(u32::from('A'), low)), (u32::from('A'), u32::from('a')));
    assert_eq!(s::towctrans(u32::from('a'), null()), u32::from('a'), "an unknown descriptor maps to itself");
}

#[test]
fn sym___towctrans_l() {
    assert_eq!(s::__towctrans_l(WEOF, s::wctrans(c("tolower").as_ptr()), null_mut()), WEOF);
}

#[test]
fn sym_towctrans_l() {
    assert_eq!(s::towctrans_l(u32::from('Q'), s::wctrans(c("tolower").as_ptr()), null_mut()), u32::from('q'));
}

#[test]
fn sym_towlower() {
    wcase(|c| s::towlower(c), u8::to_ascii_lowercase);
}

#[test]
fn sym___towlower_l() {
    wcase(|c| s::__towlower_l(c, null_mut()), u8::to_ascii_lowercase);
}

#[test]
fn sym_towlower_l() {
    wcase(|c| s::towlower_l(c, null_mut()), u8::to_ascii_lowercase);
}

#[test]
fn sym_towupper() {
    wcase(|c| s::towupper(c), u8::to_ascii_uppercase);
}

#[test]
fn sym___towupper_l() {
    wcase(|c| s::__towupper_l(c, null_mut()), u8::to_ascii_uppercase);
}

#[test]
fn sym_towupper_l() {
    wcase(|c| s::towupper_l(c, null_mut()), u8::to_ascii_uppercase);
}

#[test]
fn sym_wctrans() {
    let low = s::wctrans(c("tolower").as_ptr());
    let up = s::wctrans(c("toupper").as_ptr());
    assert!(!low.is_null() && !up.is_null() && low != up);
    assert!(s::wctrans(c("tojhira").as_ptr()).is_null(), "unknown in the C locale: 0");
    let lower_table = s::__ctype_tolower_loc();
    assert_eq!(low, read_ptr(lower_table), "the descriptor is the table, as in glibc");
}

#[test]
fn sym___wctrans_l() {
    assert_eq!(s::__wctrans_l(c("toupper").as_ptr(), null_mut()), s::wctrans(c("toupper").as_ptr()));
}

#[test]
fn sym_wctrans_l() {
    assert!(!s::wctrans_l(c("toupper").as_ptr(), null_mut()).is_null());
    assert!(s::wctrans_l(c("x").as_ptr(), null_mut()).is_null());
}

#[test]
fn sym_wctype() {
    for name in ["alnum", "alpha", "blank", "cntrl", "digit", "graph", "lower", "print", "punct", "space", "upper", "xdigit"] {
        let t = s::wctype(c(name).as_ptr());
        assert_ne!(t, 0, "{name}");
        for wc in 0..300u32 {
            assert_eq!(s::iswctype(wc, t) != 0, reference(name, i32::try_from(wc).unwrap()), "{name} {wc}");
        }
    }
    assert_eq!(s::wctype(c("jspace").as_ptr()), 0, "unknown: 0");
}

#[test]
fn sym___wctype_l() {
    assert_eq!(s::__wctype_l(c("nope").as_ptr(), null_mut()), 0);
}

#[test]
fn sym_wctype_l() {
    assert_eq!(s::wctype_l(c("digit").as_ptr(), null_mut()), s::wctype(c("digit").as_ptr()));
}
