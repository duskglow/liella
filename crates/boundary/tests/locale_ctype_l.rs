//! LOCALE-CTYPE-L: the `_l` ctype family reads the locale object's own view and tables. Each class
//! function, each case map and `iswctype_l` answer for a `locale_t` what the plain function
//! answers when that locale is the thread's (`uselocale`), in `"C"`, `"C.UTF-8"` and (when the
//! host has it) `en_US.UTF-8`; the object follows `newlocale` over a base, `duplocale` and the
//! global locale (`LC_GLOBAL_LOCALE`, null).
#![no_builtins]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use core::ptr::null_mut;

#[path = "../../core/tests/support/skip_note.rs"]
mod skip_note;

use liella_abi::ctypes::{c_char, c_int, locale_t, wint_t};
use liella_boundary::posix::s05_ctype as s;
use liella_boundary::posix::s07_locale as l7;

const LC_CTYPE_MASK: c_int = 1;
const LC_ALL_MASK: c_int = 0x1fbf;
const GLOBAL: locale_t = usize::MAX as locale_t;

fn name(n: &str) -> Vec<u8> {
    let mut v = n.as_bytes().to_vec();
    v.push(0);
    v
}

/// The object for `n`, or `None` where the host does not have the locale.
fn object(n: &str) -> Option<locale_t> {
    let o = l7::newlocale(LC_ALL_MASK, name(n).as_ptr().cast::<c_char>(), null_mut());
    (!o.is_null()).then_some(o)
}

type Narrow = (&'static str, extern "C" fn(c_int) -> c_int, extern "C" fn(c_int, locale_t) -> c_int);
type Wide = (&'static str, extern "C" fn(wint_t) -> c_int, extern "C" fn(wint_t, locale_t) -> c_int);

fn narrow_pairs() -> Vec<Narrow> {
    vec![
        ("alnum", s::isalnum, s::isalnum_l),
        ("alpha", s::isalpha, s::isalpha_l),
        ("blank", s::isblank, s::isblank_l),
        ("cntrl", s::iscntrl, s::iscntrl_l),
        ("digit", s::isdigit, s::isdigit_l),
        ("graph", s::isgraph, s::isgraph_l),
        ("lower", s::islower, s::islower_l),
        ("print", s::isprint, s::isprint_l),
        ("punct", s::ispunct, s::ispunct_l),
        ("space", s::isspace, s::isspace_l),
        ("upper", s::isupper, s::isupper_l),
        ("xdigit", s::isxdigit, s::isxdigit_l),
        ("tolower", s::tolower, s::tolower_l),
        ("toupper", s::toupper, s::toupper_l),
    ]
}

fn wide_pairs() -> Vec<Wide> {
    vec![
        ("iswalnum", s::iswalnum, s::iswalnum_l),
        ("iswalpha", s::iswalpha, s::iswalpha_l),
        ("iswblank", s::iswblank, s::iswblank_l),
        ("iswcntrl", s::iswcntrl, s::iswcntrl_l),
        ("iswdigit", s::iswdigit, s::iswdigit_l),
        ("iswgraph", s::iswgraph, s::iswgraph_l),
        ("iswlower", s::iswlower, s::iswlower_l),
        ("iswprint", s::iswprint, s::iswprint_l),
        ("iswpunct", s::iswpunct, s::iswpunct_l),
        ("iswspace", s::iswspace, s::iswspace_l),
        ("iswupper", s::iswupper, s::iswupper_l),
        ("iswxdigit", s::iswxdigit, s::iswxdigit_l),
    ]
}

const WIDE_SAMPLE: [wint_t; 12] = [0x20, 0x41, 0x7a, 0xe0, 0xff, 0x100, 0x3b1, 0x416, 0x4e00, 0x20ac, 0x1f600, 0x10ffff];

fn wide_points() -> impl Iterator<Item = wint_t> {
    (0..0x500u32).chain(WIDE_SAMPLE).chain([u32::MAX])
}

/// Every `_l` answer for `obj` equals the plain answer with `obj` as the thread's locale.
fn agree(label: &str, obj: locale_t) {
    let prev = l7::uselocale(obj);
    let mut plain_n = Vec::new();
    for (_, p, _) in narrow_pairs() {
        plain_n.push((-128..256).map(|c| p(c)).collect::<Vec<_>>());
    }
    let mut plain_w = Vec::new();
    for (_, p, _) in wide_pairs() {
        plain_w.push(wide_points().map(|w| p(w)).collect::<Vec<_>>());
    }
    let up: Vec<wint_t> = wide_points().map(|w| s::towupper(w)).collect();
    let low: Vec<wint_t> = wide_points().map(|w| s::towlower(w)).collect();
    l7::uselocale(prev);

    for (i, (n, _, f)) in narrow_pairs().into_iter().enumerate() {
        let got: Vec<c_int> = (-128..256).map(|c| f(c, obj)).collect();
        // A class answer is a truth value; a case map is the character.
        if n.starts_with("to") {
            assert_eq!(got, plain_n[i], "{label}: {n}_l");
        } else {
            let t = |v: &[c_int]| v.iter().map(|&x| x != 0).collect::<Vec<_>>();
            assert_eq!(t(&got), t(&plain_n[i]), "{label}: {n}_l");
        }
    }
    for (i, (n, _, f)) in wide_pairs().into_iter().enumerate() {
        let got: Vec<bool> = wide_points().map(|w| f(w, obj) != 0).collect();
        let want: Vec<bool> = plain_w[i].iter().map(|&x| x != 0).collect();
        assert_eq!(got, want, "{label}: {n}_l");
    }
    let got: Vec<wint_t> = wide_points().map(|w| s::towupper_l(w, obj)).collect();
    assert_eq!(got, up, "{label}: towupper_l");
    let got: Vec<wint_t> = wide_points().map(|w| s::towlower_l(w, obj)).collect();
    assert_eq!(got, low, "{label}: towlower_l");
    // `iswctype_l` of each standard class equals the class function of the same object (and so, by
    // the loop above, the plain function), whichever way the class is named.
    for (n, _, f) in wide_pairs() {
        let d = s::wctype_l(name(&n[3..]).as_ptr().cast(), obj);
        let got: Vec<bool> = wide_points().map(|w| s::iswctype_l(w, d, obj) != 0).collect();
        let want: Vec<bool> = wide_points().map(|w| f(w, obj) != 0).collect();
        assert_eq!(got, want, "{label}: iswctype_l({n})");
    }
}

#[test]
fn c_object_agrees_with_the_plain_functions() {
    // Trace: no spec clause (the `_l` lookup of a locale object's view and tables, equal to the plain functions); D-7.8, D-7.13.
    let o = object("C").unwrap();
    agree("C", o);
    // The C locale: ASCII classes only, whatever the thread's.
    assert_ne!(s::isalpha_l(i32::from(b'a'), o), 0);
    assert_eq!(s::isalpha_l(0xe9, o), 0);
    assert_eq!(s::iswalpha_l(0xe9, o), 0);
    assert_eq!(s::toupper_l(i32::from(b'a'), o), i32::from(b'A'));
    l7::freelocale(o);
}

#[test]
fn c_utf8_object_agrees_with_the_plain_functions() {
    // Trace: no spec clause (the `_l` lookup of a locale object's view and tables, equal to the plain functions); D-7.8, D-7.13.
    let o = object("C.UTF-8").unwrap();
    agree("C.UTF-8", o);
    assert_ne!(s::iswalpha_l(0x3b1, o), 0, "Greek alpha is a letter in C.UTF-8");
    assert_eq!(s::towupper_l(0x3b1, o), 0x391);
    l7::freelocale(o);
}

#[test]
fn en_us_object_agrees_with_the_plain_functions() {
    // Trace: no spec clause (the `_l` lookup of a locale object's view and tables, equal to the plain functions); D-7.8, D-7.13.
    let Some(o) = object("en_US.UTF-8") else {
        return skip_note::skipped("skipped: en_US.UTF-8 is not installed on this host (the C and C.UTF-8 tests ran)");
    };
    agree("en_US.UTF-8", o);
    assert_ne!(s::iswalpha_l(0x4e00, o), 0, "a CJK ideograph is alphabetic");
    assert_ne!(s::iswalpha_l(0xe9, o), 0);
    assert_eq!(s::towupper_l(0xe9, o), 0xc9);
    assert_ne!(s::iswctype_l(0x4e00, s::wctype_l(name("alpha").as_ptr().cast(), o), o), 0);
    assert_eq!(s::iswctype_l(0x20, s::wctype_l(name("alpha").as_ptr().cast(), o), o), 0);
    l7::freelocale(o);
}

#[test]
fn object_follows_duplocale_and_newlocale_over_a_base() {
    // Trace: no spec clause (the `_l` lookup of a locale object's view and tables, equal to the plain functions); D-7.8, D-7.13.
    let c = object("C").unwrap();
    let u = object("C.UTF-8").unwrap();
    assert_eq!(s::iswalpha_l(0x3b1, c), 0);
    let d = l7::duplocale(u);
    l7::freelocale(u);
    assert_ne!(s::iswalpha_l(0x3b1, d), 0, "the duplicate holds the locale after the original is freed");
    assert_eq!(s::towupper_l(0x3b1, d), 0x391);
    // In place over a base: the same object now answers for the new locale.
    let same = l7::newlocale(LC_CTYPE_MASK, name("C").as_ptr().cast(), c);
    assert_eq!(same, c, "newlocale reuses the base object");
    assert_eq!(s::iswalpha_l(0x3b1, same), 0);
    let back = l7::newlocale(LC_CTYPE_MASK, name("C.UTF-8").as_ptr().cast(), same);
    assert_ne!(s::iswalpha_l(0x3b1, back), 0);
    assert_eq!(s::towupper_l(0x3b1, back), 0x391);
    assert_ne!(s::isalpha_l(i32::from(b'z'), back), 0);
    l7::freelocale(back);
    l7::freelocale(d);
}

#[test]
fn global_and_null_locale_name_the_global_locale() {
    // Trace: no spec clause (the `_l` lookup of a locale object's view and tables, equal to the plain functions); D-7.8, D-7.13.
    // The global locale is "C" here (nothing in this process calls setlocale).
    for loc in [null_mut(), GLOBAL] {
        assert_ne!(s::isalpha_l(i32::from(b'a'), loc), 0);
        assert_eq!(s::isalpha_l(0xe9, loc), 0);
        assert_eq!(s::toupper_l(i32::from(b'a'), loc), i32::from(b'A'));
        assert_ne!(s::iswalpha_l(u32::from(b'a'), loc), 0);
        assert_eq!(s::iswalpha_l(0xe9, loc), 0);
    }
}
