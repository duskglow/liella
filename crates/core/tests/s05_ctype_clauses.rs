// Specified from glibc-functional-spec.md §5. Generated stubs were replaced by real tests of the
// SAME NAME (see crates/spectest); `cargo xtask stubs` now only checks coverage.

//! Clause tests for spec §5 (7 clauses).
//!
//! The C locale is ASCII (D-6.7); `C.UTF-8` carries the built-in Unicode tables (D-5.4).

#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, missing_docs)]

use liella::ctype::{self, Class, Trans};
use liella::locale::Locale;
use liella_legacy::ctype_tables as t;

const EOF: i32 = -1;
const WEOF: u32 = u32::MAX;

const ALL: [Class; 12] = [
    Class::UPPER, Class::LOWER, Class::ALPHA, Class::DIGIT, Class::XDIGIT, Class::SPACE,
    Class::PRINT, Class::GRAPH, Class::BLANK, Class::CNTRL, Class::PUNCT, Class::ALNUM,
];

#[test]
fn s5_1_c_0() {
    assert!(t::is(i32::from(b'a'), Class::LOWER, &Locale::C));
    assert!(!t::is(i32::from(b'A'), Class::LOWER, &Locale::C));
    assert!(t::is(i32::from(b'\t'), Class::BLANK, &Locale::C));
    assert!(ctype::classes(b'7', &Locale::C).intersects(Class::DIGIT), "the given locale");
}

#[test]
fn s5_1_i_0() {
    for class in ALL {
        assert!(!t::is(EOF, class, &Locale::C), "EOF is in the domain and in no class");
    }
    for c in 0..=255 {
        let _ = ALL.map(|k| t::is(c, k, &Locale::C));
    }
    assert!(!t::is(-2, Class::PRINT, &Locale::C) && !t::is(256, Class::PRINT, &Locale::C), "outside the domain: no class, no fault");
}

#[test]
fn s5_1_g_0() {
    for c in -128..256 {
        assert_eq!(t::is(c, Class::ALNUM, &Locale::C), t::is(c, Class::ALPHA, &Locale::C) || t::is(c, Class::DIGIT, &Locale::C), "{c}");
        assert_eq!(t::is(c, Class::ALPHA, &Locale::C), t::is(c, Class::UPPER, &Locale::C) || t::is(c, Class::LOWER, &Locale::C), "{c}");
        assert_eq!(t::is(c, Class::GRAPH, &Locale::C), t::is(c, Class::ALNUM, &Locale::C) || t::is(c, Class::PUNCT, &Locale::C), "{c}");
        assert_eq!(t::is(c, Class::PRINT, &Locale::C), t::is(c, Class::GRAPH, &Locale::C) || c == 0x20, "{c}");
        assert_eq!(t::is(c, Class::DIGIT, &Locale::C), (0x30..=0x39).contains(&c), "{c}");
        assert_eq!(t::is(c, Class::XDIGIT, &Locale::C), u8::try_from(c).is_ok_and(|b| b.is_ascii_hexdigit()), "{c}");
        assert_eq!(t::isascii(c), (0..=127).contains(&c), "{c}");
    }
    assert_eq!(t::toascii(0xE9), 0x69, "masks to 7 bits");
}

#[test]
fn s5_2_c_0() {
    let alpha = t::wctype(b"alpha", &Locale::C);
    assert!(t::iswctype(u32::from('z'), alpha, &Locale::C));
    assert!(!t::iswctype(u32::from('1'), alpha, &Locale::C));
    assert!(!t::iswctype(WEOF, alpha, &Locale::C), "WEOF is in no class");
    for k in ALL {
        assert!(!t::isw(WEOF, k, &Locale::C));
    }
    assert!(t::isw(u32::from(' '), Class::SPACE, &Locale::C));
}

#[test]
fn s5_2_g_0() {
    // C.UTF-8's classes come from the Unicode 15.1.0 database (D-5.4); the named UTF-8 locales'
    // from their compiled LC_CTYPE (compared with the reference library in locale_products.rs).
    let u = Locale::c_utf8(b"C.UTF-8");
    let new_in_15_1 = 0x2EBF0; // CJK UNIFIED IDEOGRAPH-2EBF0 (Extension I)
    for k in [Class::ALPHA, Class::GRAPH, Class::PRINT, Class::ALNUM] {
        assert!(t::isw(new_in_15_1, k, &u), "{k:?}");
    }
    assert!(!t::isw(new_in_15_1, Class::ALPHA, &Locale::C), "the C locale is ASCII");
    assert!(!t::isw(0x2EE5E, Class::PRINT, &u), "unassigned after Extension I");
    assert!(t::isw(0x391, Class::UPPER, &u));
    assert_eq!(ctype::wtrans(0x391, Trans::Lower, &u), 0x3B1, "GREEK CAPITAL ALPHA");
    assert!(t::isw(0x3000, Class::SPACE, &u), "IDEOGRAPHIC SPACE");
}

#[test]
fn s5_2_f_0() {
    assert_eq!(t::wctype(b"jspace", &Locale::C), 0);
    assert_eq!(t::wctype(b"", &Locale::C), 0);
    assert!(!t::iswctype(u32::from('a'), 0, &Locale::C));
}

#[test]
fn s5_3_c_0() {
    assert_eq!((t::tolower(i32::from(b'Q'), &Locale::C), t::toupper(i32::from(b'q'), &Locale::C)), (i32::from(b'q'), i32::from(b'Q')));
    assert_eq!(t::tolower(i32::from(b'5'), &Locale::C), i32::from(b'5'), "no counterpart: itself");
    assert_eq!(t::tolower(EOF, &Locale::C), EOF);
    let up = t::wctrans(b"toupper", &Locale::C);
    assert_ne!(up, 0);
    assert_eq!(t::towctrans(u32::from('k'), up, &Locale::C), u32::from('K'));
    assert_eq!(t::wctrans(b"tojhira", &Locale::C), 0, "not defined by the C locale");
    assert_eq!(ctype::wtrans(u32::from('ß'), Trans::Upper, &Locale::C), u32::from('ß'), "1:1 only");
}

// ------------------------------------------------------------------------------ clauses after a flag line (2026-10-04)

#[test]
fn s5_1_i_p1() {
    // isctype/__isctype test an arbitrary combination of the class bits (_IS*: upper 0x100,
    // lower 0x200, alpha 0x400, digit 0x800, space 0x2000, punct 0x4).
    use liella_boundary::posix::s05_ctype as cx;
    assert_ne!(cx::isctype(i32::from(b'A'), 0x100), 0);
    assert_eq!(cx::isctype(i32::from(b'a'), 0x100), 0);
    assert_ne!(cx::isctype(i32::from(b'a'), 0x100 | 0x200), 0, "any bit of the combination");
    assert_ne!(cx::isctype(i32::from(b'5'), 0x800), 0);
    assert_eq!(cx::isctype(i32::from(b'5'), 0x400 | 0x2000), 0);
    assert_eq!(cx::__isctype(i32::from(b'!'), 0x4), cx::isctype(i32::from(b'!'), 0x4));
}
