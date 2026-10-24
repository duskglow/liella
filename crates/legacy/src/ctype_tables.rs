//! The ABI tables behind `__ctype_b_loc`, `__ctype_tolower_loc` and `__ctype_toupper_loc`
//! (spec §5.1 ⚑ COMPAT, Appendix C.9): arrays indexable from -128 to 255, built at compile time
//! from `liella::ctype` for the built-in `"C"` locale (D-5.3; other locales arrive in phase 7).
//! Index `i` of an array holds the entry for character `i - 128`.

#![allow(clippy::as_conversions, clippy::indexing_slicing, clippy::arithmetic_side_effects)] // const-evaluated table construction

use liella::ctype::{self, ascii_classes, Class, Trans, WClass};
use liella::locale::ctype::Map;
use liella::locale::view::LocaleView;
use liella::locale::{Ctype, Locale};

/// Entries in each table (characters -128..=255).
pub const LEN: usize = 384;
/// Offset of character 0 within the arrays.
pub const ZERO: usize = 128;

const fn class_table() -> [u16; LEN] {
    let mut t = [0u16; LEN];
    let mut i = 0;
    while i < 256 {
        t[ZERO + i] = ascii_classes(i as u8).0;
        i += 1;
    }
    t
}

/// `_ISbit(b)`: the 16-bit mask of class bit `b` (bytes swapped, as `<ctype.h>` defines it).
const fn isbit(b: u32) -> u16 {
    if b < 8 { (1u16 << b) << 8 } else { (1u16 << b) >> 8 }
}

/// `_ISwbit(b)`: the 32-bit mask of class bit `b` in the wide tables (`<wctype.h>`: the four
/// bytes reversed).
const fn iswbit(b: u32) -> u32 {
    let v = 1u32 << b;
    if b < 8 {
        v << 24
    } else if b < 16 {
        v << 8
    } else if b < 24 {
        v >> 8
    } else {
        v >> 24
    }
}

/// A 16-bit `_ISbit` mask re-encoded with `_ISwbit`.
const fn wide_mask(m: u16) -> u32 {
    let mut out = 0u32;
    let mut b = 0;
    while b < 12 {
        if m & isbit(b) != 0 {
            out |= iswbit(b);
        }
        b += 1;
    }
    out
}

const fn class32_table() -> [u32; LEN] {
    let c = class_table();
    let mut t = [0u32; LEN];
    let mut i = 0;
    while i < LEN {
        t[i] = wide_mask(c[i]);
        i += 1;
    }
    t
}

const fn case_table(upper: bool) -> [i32; LEN] {
    let mut t = [0i32; LEN];
    let mut i = 0;
    while i < LEN {
        let c = i as i32 - ZERO as i32;
        // Negative `char` values other than EOF map to the byte they alias (c + 256), as in
        // the reference library's tables; EOF (−1) maps to itself.
        t[i] = if c >= 0 && c < 128 {
            let b = c as u8;
            (if upper { b.to_ascii_uppercase() } else { b.to_ascii_lowercase() }) as i32
        } else if c < -1 {
            c + 256
        } else {
            c
        };
        i += 1;
    }
    t
}

/// The `"C"` class table (`unsigned short`, the `_IS*` masks).
pub static C_CLASS: [u16; LEN] = class_table();
/// The `"C"` 32-bit class table (`__ctype32_b`, compat), in the `_ISwbit` encoding.
pub static C_CLASS32: [u32; LEN] = class32_table();
/// The `"C"` `tolower` table.
pub static C_TOLOWER: [i32; LEN] = case_table(false);
/// The `"C"` `toupper` table.
pub static C_TOUPPER: [i32; LEN] = case_table(true);

// ------------------------------------------------------------------------------ C-shaped calls

/// The byte table's classes for a C `int` argument (−128…255, D-5.1); none outside.
fn class_of(c: i32, loc: &Locale) -> Class {
    match loc.ctype() {
        // A compiled table covers −128…255 itself (its entry for −1, `EOF`, included).
        Ctype::File(f) if (-128..=255).contains(&c) => Class(f.byte_class(c)),
        // The built-ins read the "C" table header-inline calls read (one load).
        Ctype::C | Ctype::CUtf8 => usize::try_from(c.saturating_add(128)).ok().and_then(|i| C_CLASS.get(i)).map_or(Class(0), |&m| Class(m)),
        Ctype::File(_) => u8::try_from(c).map_or(Class(0), |b| ctype::classes(b, loc)),
    }
}

/// The byte classes of a C `int` argument in `loc` (none outside -128...255).
pub fn class(c: i32, loc: &Locale) -> Class {
    class_of(c, loc)
}

/// A class test of a C `int` argument (`isalpha`, ...) in `loc`.
pub fn is(c: i32, mask: Class, loc: &Locale) -> bool {
    class_of(c, loc).intersects(mask)
}

/// `isctype(c, mask)`: `mask` is any combination of `_IS*` bits.
pub fn isctype(c: i32, mask: i32, loc: &Locale) -> bool {
    u16::try_from(mask).is_ok_and(|m| is(c, Class(m), loc))
}

/// `isascii`.
pub fn isascii(c: i32) -> bool {
    (0..=0x7f).contains(&c)
}

/// `toascii`: the low seven bits.
pub fn toascii(c: i32) -> i32 {
    c & 0x7f
}

/// `tolower` of a C `int` argument in `loc`: values outside `unsigned char` (`EOF`) map to
/// themselves.
pub fn tolower(c: i32, loc: &Locale) -> i32 {
    match loc.ctype() {
        Ctype::File(f) if (-128..=255).contains(&c) => f.byte_map(c, Map::Lower),
        _ => table_entry(&C_TOLOWER, c),
    }
}

/// A built-in case table's entry for `c` (−128…255; other values map to themselves).
fn table_entry(t: &[i32; LEN], c: i32) -> i32 {
    usize::try_from(c.wrapping_add(128)).ok().and_then(|i| t.get(i)).copied().unwrap_or(c)
}

/// `toupper`, as [`tolower`].
pub fn toupper(c: i32, loc: &Locale) -> i32 {
    match loc.ctype() {
        Ctype::File(f) if (-128..=255).contains(&c) => f.byte_map(c, Map::Upper),
        _ => table_entry(&C_TOUPPER, c),
    }
}

/// A class test of a `wint_t` (`iswalpha`, ...) in `loc`; `WEOF` belongs to no class.
pub fn isw(wc: u32, mask: Class, loc: &Locale) -> bool {
    ctype::is_w(wc, mask, loc)
}

/// The `wctype_t` of a locale-defined class `k`: above every standard mask.
const DEFINED: u64 = 0x1_0000;

/// `wctype(name)` in `loc`: a standard class as its mask, a locale-defined class as
/// `0x10000 + k`; 0 for an unknown name.
pub fn wctype(name: &[u8], loc: &Locale) -> u64 {
    match ctype::wctype(name, loc) {
        Some(WClass::Standard(c)) => u64::from(c.0),
        Some(WClass::Defined(k)) => DEFINED.saturating_add(u64::try_from(k).unwrap_or(0)),
        None => 0,
    }
}

/// [`wctype`] of a converted name (the conversion's error passes through).
pub fn wctype_of(name: liella::error::KResult<&[u8]>, loc: &Locale) -> liella::error::KResult<u64> {
    name.map(|n| wctype(n, loc))
}

/// [`wctrans`] of a converted name.
pub fn wctrans_of(name: liella::error::KResult<&[u8]>, loc: &Locale) -> liella::error::KResult<usize> {
    name.map(|n| wctrans(n, loc))
}

/// `iswctype(wc, desc)`: membership in a class [`wctype`] returned (0: none).
pub fn iswctype(wc: u32, desc: u64, loc: &Locale) -> bool {
    if desc >= DEFINED {
        return usize::try_from(desc - DEFINED).is_ok_and(|k| ctype::is_wclass(wc, WClass::Defined(k), loc));
    }
    u16::try_from(desc).is_ok_and(|m| m != 0 && isw(wc, Class(m), loc))
}

/// [`iswctype`] in the locale of view `v` (D-7.13): a standard class reads the view's checked
/// wide tables, as `iswalpha` does; a locale-defined class takes the general path.
#[inline]
pub fn iswctype_view(wc: u32, desc: u64, v: &LocaleView) -> bool {
    match u16::try_from(desc) {
        Ok(m) => v.isw(wc, Class(m)),
        Err(_) => iswctype_defined(wc, desc, v),
    }
}

/// [`iswctype_view`] for a descriptor that is not a standard class's mask: a locale-defined
/// class (the general path), or a value that names no class.
#[cold]
#[inline(never)]
fn iswctype_defined(wc: u32, desc: u64, v: &LocaleView) -> bool {
    desc >= DEFINED && iswctype(wc, desc, v.locale())
}

/// A marker whose address names the built-in `C.UTF-8` `totitle` mapping.
static UTF8_TOTITLE: i32 = 0;

/// The `wctrans_t` of mapping `t` in `loc`: the address of its table, as in glibc (the byte
/// table at character 0 for `toupper`/`tolower`, the map's own table otherwise; 0 for none).
pub fn trans_desc(t: Trans, loc: &Locale) -> usize {
    match (loc.ctype(), t) {
        (Ctype::File(f), Trans::Upper) => f.toupper_table().as_ptr().wrapping_add(4 * ZERO).expose_provenance(),
        (Ctype::File(f), Trans::Lower) => f.tolower_table().as_ptr().wrapping_add(4 * ZERO).expose_provenance(),
        (Ctype::File(f), Trans::Defined(k)) => f.map_table(k).as_ptr().expose_provenance(),
        (_, Trans::Upper) => C_TOUPPER.as_ptr().wrapping_add(ZERO).expose_provenance(),
        (_, Trans::Lower) => C_TOLOWER.as_ptr().wrapping_add(ZERO).expose_provenance(),
        (Ctype::CUtf8, Trans::Defined(2)) => core::ptr::from_ref(&UTF8_TOTITLE).expose_provenance(),
        (_, Trans::Defined(_)) => 0,
    }
}

/// `wctrans(name)` in `loc`: the descriptor, 0 for an unknown name.
pub fn wctrans(name: &[u8], loc: &Locale) -> usize {
    Trans::in_locale(name, loc).map_or(0, |t| trans_desc(t, loc))
}

/// The mapping a descriptor of `loc` names.
fn trans_of(desc: usize, loc: &Locale) -> Option<Trans> {
    if desc == 0 {
        return None;
    }
    let maps = match loc.ctype() {
        Ctype::File(f) => f.map_count(),
        Ctype::CUtf8 => 3,
        Ctype::C => 2,
    };
    let mut candidates = [Trans::Upper, Trans::Lower].into_iter().chain((2..maps).map(Trans::Defined));
    candidates.find(|&t| trans_desc(t, loc) == desc)
}

/// `towctrans(wc, desc)`: apply the mapping a descriptor names in `loc`; a descriptor of
/// another locale or of none maps `wc` to itself.
pub fn towctrans(wc: u32, desc: usize, loc: &Locale) -> u32 {
    trans_of(desc, loc).map_or(wc, |t| ctype::wtrans(wc, t, loc))
}

/// `towlower`.
pub fn towlower(wc: u32, loc: &Locale) -> u32 {
    ctype::wtrans(wc, Trans::Lower, loc)
}

/// `towupper`.
pub fn towupper(wc: u32, loc: &Locale) -> u32 {
    ctype::wtrans(wc, Trans::Upper, loc)
}
