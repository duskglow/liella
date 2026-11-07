//! TOWCTRANS-DESC (D-5.8): `towctrans` with the `toupper` or `tolower` descriptor of the
//! thread's locale (or of the `locale_t` it is given) answers exactly what `towupper` and
//! `towlower` answer, for every code point, in `"C"`, `"C.UTF-8"` and (when the host has it)
//! `en_US.UTF-8`, through `towctrans`, `__towctrans`, `towctrans_l` and `__towctrans_l`; a
//! descriptor stays bound to its locale across `uselocale` (another locale's descriptor maps a
//! character to itself, a descriptor taken again after the switch is the new locale's).
#![no_builtins]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use core::ptr::{null, null_mut};

#[path = "../../core/tests/support/skip_note.rs"]
mod skip_note;

use liella_abi::ctypes::{c_char, c_int, locale_t, wctrans_t, wint_t};
use liella_boundary::posix::s05_ctype as s;
use liella_boundary::posix::s07_locale as l7;

const LC_ALL_MASK: c_int = 0x1fbf;
const WEOF: wint_t = u32::MAX;

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

/// Every code point, `WEOF` and a few values beyond Unicode.
fn points() -> impl Iterator<Item = wint_t> {
    (0..=0x10_ffffu32).chain([0x11_0000, 0x7fff_ffff, 0x8000_0000, WEOF])
}

/// The first code point whose answer differs, with both answers.
fn first_mismatch(got: &[wint_t], want: &[wint_t]) -> Option<(wint_t, wint_t, wint_t)> {
    points().zip(got.iter().zip(want)).find(|(_, (g, w))| g != w).map(|(c, (g, w))| (c, *g, *w))
}

fn wctrans(n: &str) -> wctrans_t {
    s::wctrans(name(n).as_ptr().cast())
}

/// Every form of `towctrans` with the `toupper`/`tolower` descriptors of `obj` equals
/// `towupper`/`towlower` in `obj`. The `_l` forms run while the thread is in `other`, so an
/// entry that read the thread's locale instead of its argument answers differently.
fn agree(label: &str, obj: locale_t, other: locale_t) {
    let prev = l7::uselocale(obj);
    let (up, low) = (wctrans("toupper"), wctrans("tolower"));
    assert!(!up.is_null() && !low.is_null() && up != low, "{label}: wctrans");
    let want_up: Vec<wint_t> = points().map(|c| s::towupper(c)).collect();
    let want_low: Vec<wint_t> = points().map(|c| s::towlower(c)).collect();
    for (form, f) in [("towctrans", s::towctrans as extern "C" fn(wint_t, wctrans_t) -> wint_t), ("__towctrans", s::__towctrans)] {
        for (map, d, want) in [("toupper", up, &want_up), ("tolower", low, &want_low)] {
            let got: Vec<wint_t> = points().map(|c| f(c, d)).collect();
            assert_eq!(first_mismatch(&got, want), None, "{label}: {form}(c, {map}): (c, got, want)");
        }
    }
    l7::uselocale(other);
    let (up_l, low_l) = (s::wctrans_l(name("toupper").as_ptr().cast(), obj), s::__wctrans_l(name("tolower").as_ptr().cast(), obj));
    assert_eq!((up_l, low_l), (up, low), "{label}: wctrans_l names the same descriptors");
    for (form, f) in [("towctrans_l", s::towctrans_l as extern "C" fn(wint_t, wctrans_t, locale_t) -> wint_t), ("__towctrans_l", s::__towctrans_l)] {
        for (map, d, want) in [("toupper", up_l, &want_up), ("tolower", low_l, &want_low)] {
            let got: Vec<wint_t> = points().map(|c| f(c, d, obj)).collect();
            assert_eq!(first_mismatch(&got, want), None, "{label}: {form}(c, {map}): (c, got, want)");
            // The `_l` answers are towupper_l/towlower_l's too.
            let u = if map == "toupper" { s::towupper_l(0xe9, obj) } else { s::towlower_l(0xc9, obj) };
            assert_eq!(f(if map == "toupper" { 0xe9 } else { 0xc9 }, d, obj), u, "{label}: {form} vs to*_l");
        }
    }
    // A null descriptor names no map: every entry maps the character to itself.
    for c in [0x61, 0xe9, 0x3b1] {
        assert_eq!((s::towctrans(c, null()), s::towctrans_l(c, null(), obj)), (c, c), "{label}: null descriptor");
    }
    l7::uselocale(prev);
}

#[test]
fn towctrans_c_equals_towupper_towlower_everywhere() {
    // Trace: s5_3_c_0 (towctrans with wctrans("toupper"/"tolower") is towupper/towlower); D-5.8.
    let (o, t) = (object("C").unwrap(), object("C.UTF-8").unwrap());
    agree("C", o, t);
    l7::freelocale(t);
    l7::freelocale(o);
}

#[test]
fn towctrans_c_utf8_equals_towupper_towlower_everywhere() {
    // Trace: s5_3_c_0 (towctrans with wctrans("toupper"/"tolower") is towupper/towlower); D-5.8.
    let (o, t) = (object("C.UTF-8").unwrap(), object("C").unwrap());
    agree("C.UTF-8", o, t);
    // The locale's other map still answers through the general path.
    let prev = l7::uselocale(o);
    let title = wctrans("totitle");
    assert!(!title.is_null());
    assert_eq!(s::towctrans(u32::from('a'), title), u32::from('A'), "C.UTF-8 totitle");
    assert_eq!(s::towctrans_l(0x3b1, title, o), 0x391, "C.UTF-8 totitle (_l)");
    assert_eq!(s::towctrans(0x3b1, core::ptr::without_provenance(16)), 0x3b1, "a descriptor naming no map maps to itself");
    l7::uselocale(prev);
    l7::freelocale(t);
    l7::freelocale(o);
}

#[test]
fn towctrans_en_us_equals_towupper_towlower_everywhere() {
    // Trace: s5_3_c_0 (towctrans with wctrans("toupper"/"tolower") is towupper/towlower); D-5.8.
    let Some(o) = object("en_US.UTF-8") else {
        return skip_note::skipped("skipped: en_US.UTF-8 is not installed on this host (the C and C.UTF-8 tests ran)");
    };
    let t = object("C").unwrap();
    agree("en_US.UTF-8", o, t);
    l7::freelocale(t);
    l7::freelocale(o);
}

#[test]
fn towctrans_descriptor_stays_bound_to_its_locale_across_uselocale() {
    // Trace: s5_3_c_0 (a descriptor names a mapping of the locale it came from); D-5.8.
    let Some(en) = object("en_US.UTF-8") else {
        return skip_note::skipped("skipped: en_US.UTF-8 is not installed on this host (the C and C.UTF-8 tests ran)");
    };
    let c = object("C").unwrap();
    let prev = l7::uselocale(en);
    let en_up = wctrans("toupper");
    assert_eq!(s::towctrans(0xe9, en_up), 0xc9);
    // In C, en_US's descriptor names no map of the locale: the character maps to itself, and
    // C's own descriptor is C's map.
    l7::uselocale(c);
    let c_up = wctrans("toupper");
    assert_ne!(c_up, en_up);
    assert_eq!(s::towctrans(u32::from('a'), en_up), u32::from('a'), "en_US descriptor in C");
    assert_eq!(s::towctrans(u32::from('a'), c_up), u32::from('A'));
    assert_eq!(s::towctrans_l(0xe9, en_up, en), 0xc9, "en_US descriptor with the en_US object");
    // Back in en_US: its descriptor works again, C's maps to itself.
    l7::uselocale(en);
    assert_eq!(s::towctrans(0xe9, en_up), 0xc9);
    assert_eq!(s::towctrans(u32::from('a'), c_up), u32::from('a'), "C descriptor in en_US");
    assert_eq!(s::towctrans_l(u32::from('a'), en_up, c), u32::from('a'), "en_US descriptor with the C object");
    l7::uselocale(prev);
    l7::freelocale(c);
    l7::freelocale(en);
}
