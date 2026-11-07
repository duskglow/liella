//! LOCALE-CTYPE-L: the `_l` ctype family reads the locale object's own view and tables. Each class
//! function, each case map and `iswctype_l` answer for a `locale_t` what the plain function
//! answers when that locale is the thread's (`uselocale`), in `"C"`, `"C.UTF-8"` and (when the
//! host has it) `en_US.UTF-8`, both through the `_l` form and its `__*_l` alias, while the thread
//! runs in a locale whose classes differ (so an entry that reads the thread's locale instead of its
//! argument fails); the object follows `newlocale` over a base, `duplocale` and the global locale
//! (`LC_GLOBAL_LOCALE`, null).
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

/// The name, the plain function, its `_l` form and the `__*_l` alias (a separate entry).
type Narrow = (
    &'static str,
    extern "C" fn(c_int) -> c_int,
    extern "C" fn(c_int, locale_t) -> c_int,
    extern "C" fn(c_int, locale_t) -> c_int,
);
type Wide = (
    &'static str,
    extern "C" fn(wint_t) -> c_int,
    extern "C" fn(wint_t, locale_t) -> c_int,
    extern "C" fn(wint_t, locale_t) -> c_int,
);

fn narrow_pairs() -> Vec<Narrow> {
    vec![
        ("alnum", s::isalnum, s::isalnum_l, s::__isalnum_l),
        ("alpha", s::isalpha, s::isalpha_l, s::__isalpha_l),
        ("blank", s::isblank, s::isblank_l, s::__isblank_l),
        ("cntrl", s::iscntrl, s::iscntrl_l, s::__iscntrl_l),
        ("digit", s::isdigit, s::isdigit_l, s::__isdigit_l),
        ("graph", s::isgraph, s::isgraph_l, s::__isgraph_l),
        ("lower", s::islower, s::islower_l, s::__islower_l),
        ("print", s::isprint, s::isprint_l, s::__isprint_l),
        ("punct", s::ispunct, s::ispunct_l, s::__ispunct_l),
        ("space", s::isspace, s::isspace_l, s::__isspace_l),
        ("upper", s::isupper, s::isupper_l, s::__isupper_l),
        ("xdigit", s::isxdigit, s::isxdigit_l, s::__isxdigit_l),
        ("tolower", s::tolower, s::tolower_l, s::__tolower_l),
        ("toupper", s::toupper, s::toupper_l, s::__toupper_l),
    ]
}

fn wide_pairs() -> Vec<Wide> {
    vec![
        ("iswalnum", s::iswalnum, s::iswalnum_l, s::__iswalnum_l),
        ("iswalpha", s::iswalpha, s::iswalpha_l, s::__iswalpha_l),
        ("iswblank", s::iswblank, s::iswblank_l, s::__iswblank_l),
        ("iswcntrl", s::iswcntrl, s::iswcntrl_l, s::__iswcntrl_l),
        ("iswdigit", s::iswdigit, s::iswdigit_l, s::__iswdigit_l),
        ("iswgraph", s::iswgraph, s::iswgraph_l, s::__iswgraph_l),
        ("iswlower", s::iswlower, s::iswlower_l, s::__iswlower_l),
        ("iswprint", s::iswprint, s::iswprint_l, s::__iswprint_l),
        ("iswpunct", s::iswpunct, s::iswpunct_l, s::__iswpunct_l),
        ("iswspace", s::iswspace, s::iswspace_l, s::__iswspace_l),
        ("iswupper", s::iswupper, s::iswupper_l, s::__iswupper_l),
        ("iswxdigit", s::iswxdigit, s::iswxdigit_l, s::__iswxdigit_l),
    ]
}

const WIDE_SAMPLE: [wint_t; 12] = [0x20, 0x41, 0x7a, 0xe0, 0xff, 0x100, 0x3b1, 0x416, 0x4e00, 0x20ac, 0x1f600, 0x10ffff];

fn wide_points() -> impl Iterator<Item = wint_t> {
    (0..0x500u32).chain(WIDE_SAMPLE).chain([u32::MAX])
}

/// Every `_l` answer for `obj` (both the `_l` form and its `__*_l` alias) equals the plain answer
/// with `obj` as the thread's locale, while `thread` is the thread's locale during the `_l` calls.
/// The callers pass a `thread` whose classes differ from `obj`'s, so an entry that read the thread's
/// locale instead of its argument answers differently.
fn agree(label: &str, obj: locale_t, thread: locale_t) {
    let prev = l7::uselocale(obj);
    let mut plain_n = Vec::new();
    for (_, p, _, _) in narrow_pairs() {
        plain_n.push((-128..256).map(|c| p(c)).collect::<Vec<_>>());
    }
    let mut plain_w = Vec::new();
    for (_, p, _, _) in wide_pairs() {
        plain_w.push(wide_points().map(|w| p(w)).collect::<Vec<_>>());
    }
    let up: Vec<wint_t> = wide_points().map(|w| s::towupper(w)).collect();
    let low: Vec<wint_t> = wide_points().map(|w| s::towlower(w)).collect();
    l7::uselocale(thread);

    for (i, (n, _, f, g)) in narrow_pairs().into_iter().enumerate() {
        for (form, h) in [("", f), ("__", g)] {
            let got: Vec<c_int> = (-128..256).map(|c| h(c, obj)).collect();
            // A class answer is a truth value; a case map is the character.
            if n.starts_with("to") {
                assert_eq!(got, plain_n[i], "{label}: {form}{n}_l");
            } else {
                let t = |v: &[c_int]| v.iter().map(|&x| x != 0).collect::<Vec<_>>();
                assert_eq!(t(&got), t(&plain_n[i]), "{label}: {form}{n}_l");
            }
        }
    }
    for (i, (n, _, f, g)) in wide_pairs().into_iter().enumerate() {
        let want: Vec<bool> = plain_w[i].iter().map(|&x| x != 0).collect();
        for (form, h) in [("", f), ("__", g)] {
            let got: Vec<bool> = wide_points().map(|w| h(w, obj) != 0).collect();
            assert_eq!(got, want, "{label}: {form}{n}_l");
        }
    }
    for (form, h) in [("", s::towupper_l as extern "C" fn(wint_t, locale_t) -> wint_t), ("__", s::__towupper_l)] {
        let got: Vec<wint_t> = wide_points().map(|w| h(w, obj)).collect();
        assert_eq!(got, up, "{label}: {form}towupper_l");
    }
    for (form, h) in [("", s::towlower_l as extern "C" fn(wint_t, locale_t) -> wint_t), ("__", s::__towlower_l)] {
        let got: Vec<wint_t> = wide_points().map(|w| h(w, obj)).collect();
        assert_eq!(got, low, "{label}: {form}towlower_l");
    }
    // `iswctype_l` of each standard class equals the plain class function under `obj`, whichever
    // way the class is named and whichever entry is called.
    for (i, (n, _, _, _)) in wide_pairs().into_iter().enumerate() {
        let want: Vec<bool> = plain_w[i].iter().map(|&x| x != 0).collect();
        let d = s::wctype_l(name(&n[3..]).as_ptr().cast(), obj);
        let d2 = s::__wctype_l(name(&n[3..]).as_ptr().cast(), obj);
        for (form, desc, h) in [
            ("", d, s::iswctype_l as extern "C" fn(wint_t, _, locale_t) -> c_int),
            ("__", d2, s::__iswctype_l),
        ] {
            let got: Vec<bool> = wide_points().map(|w| h(w, desc, obj) != 0).collect();
            assert_eq!(got, want, "{label}: {form}iswctype_l({n})");
        }
    }
    l7::uselocale(prev);
}

#[test]
fn c_object_agrees_with_the_plain_functions() {
    // Trace: no spec clause (the `_l` lookup of a locale object's view and tables, equal to the plain functions); D-7.8, D-7.13.
    let o = object("C").unwrap();
    // The thread runs in C.UTF-8 meanwhile: Greek and Latin-1 letters tell the two apart.
    let t = object("C.UTF-8").unwrap();
    agree("C", o, t);
    l7::freelocale(t);
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
    let t = object("C").unwrap();
    agree("C.UTF-8", o, t);
    l7::freelocale(t);
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
    let t = object("C").unwrap();
    agree("en_US.UTF-8", o, t);
    l7::freelocale(t);
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
