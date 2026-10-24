//! Translation for spec §8: numeric conversion and integer arithmetic.
//!
//! `long double` and `_Float128` cross the C ABI in registers Rust cannot name, so their
//! exports are single-statement C forwarders (`csys/src/adapter/float.c`, OD-C4) around the
//! `liella_*_bits` functions here, which take and return the values as bit structs
//! (`LdBits`, `F128Bits`). Those carry the private version node `LIELLA_PRIVATE` (not
//! exported from the product, phase 4).
//!
//! Conversions use the current rounding mode (MXCSR) and the current locale's radix. Only the
//! compat `__strto*_internal` / `__wcsto*_internal` entry points group, when their `group`
//! argument is set: integers and decimal floats read the longest prefix grouped as `LC_NUMERIC`
//! says ([`grouped`], STRTO-INTERNAL-GROUP-2; hex floats are not grouped yet). The `ecvt`/`fcvt`/`l64a`
//! buffers are process-wide until the phase-4 thread blocks.

use liella::bytes::Unit;
use liella::error::Errno;
use liella::mem::TryVec;
use liella::num::{self, Float, Format, StdBit};
use liella::num::parse::Dialect;
use liella_abi::ctypes::{c_char, c_int, c_long, c_uchar, c_uint, c_ulong, c_ulonglong, c_ushort, locale_t, size_t, wchar_t, F128Bits, LdBits};
use liella_abi::layout::{div_t, imaxdiv_t, ldiv_t, lldiv_t};
use liella_abi::pack;
use liella_kernel::machine::Machine;
use liella_legacy::num_compat;

use crate::conv;
use crate::locale;
use crate::export;
use crate::ret;
use crate::strz;
use crate::sys::CSys;

/// Bytes the `strtol` fast path looks at first: a sign, 18 digits and the terminator.
const STRTOL_HEAD: usize = 20;

// ------------------------------------------------------------------ the compat `group` flag

/// What the grouping scan reads: an integer in `base`, or a decimal float.
#[derive(Clone, Copy)]
enum Grouped {
    Int(c_int),
    Float,
}

/// The end a `num_compat` parser reports, as an offset from address 0 (`None`: not set).
struct Rec(Option<usize>);

impl liella_legacy::string_compat::Cursor for Rec {
    fn get(&self) -> usize {
        self.0.unwrap_or(0)
    }
    fn set(&mut self, addr: usize) {
        self.0 = Some(addr);
    }
}

/// The `LC_NUMERIC` group sizes from the right (`GROUPING`): the sizes before the first 0 or
/// `CHAR_MAX` byte, and whether such a `CHAR_MAX` byte ends grouping (otherwise the last size
/// repeats). `n == 0`: no grouping.
struct Sizes<'a> {
    g: &'a [u8],
    stopped: bool,
}

impl Sizes<'_> {
    fn new(g: &[u8]) -> Sizes<'_> {
        let n = g.iter().position(|&b| b == 0 || b >= 127).unwrap_or(g.len());
        Sizes { g: g.get(..n).unwrap_or(&[]), stopped: g.get(n).is_some_and(|&b| b != 0) }
    }
    /// The size of the group `t` places left of the rightmost one; `None`: not grouped.
    fn at(&self, t: usize) -> Option<usize> {
        match self.g.get(t) {
            Some(&b) => Some(usize::from(b)),
            None if self.stopped => None,
            None => self.g.last().map(|&b| usize::from(b)),
        }
    }
}

fn is_digit<T: Unit>(u: T) -> bool {
    u.as_byte().is_some_and(|b| b.is_ascii_digit())
}

/// Digits of `s` that end at `end`, counted back up to `limit + 1`.
fn digits_back<T: Unit>(s: &[T], end: usize, limit: usize) -> usize {
    s.get(..end).unwrap_or(&[]).iter().rev().take(limit.saturating_add(1)).take_while(|&&u| is_digit(u)).count()
}

/// A separator `sep` at `i` that a digit follows.
fn sep_at<T: Unit>(s: &[T], i: usize, sep: &[T]) -> bool {
    s.get(i..).is_some_and(|r| r.starts_with(sep)) && s.get(i + sep.len()).is_some_and(|&u| is_digit(u))
}

/// The text the parse sees when `group` is set (§8.1-§8.3, D-8.1 compat; STRTO-INTERNAL-GROUP-2):
/// the longest correctly grouped prefix of the leading digit run, with its separators removed.
/// `Ok(None)`: parse `s` itself; `Ok(Some((text, head, digits, cut)))`: parse `text`, made of
/// `s[..head]`, the `digits` digits of `s[head..cut]` and, for a float grouped to the end of its
/// run, the rest of `s` from `cut`; `Err`: no memory for the copy.
///
/// Integers group in base 10 (base 0 unless the number starts with `0`), floats unless hex
/// (hex floats are STRTO-INTERNAL-GROUP-3's). One pass forward and one back, each candidate end
/// checked in time bounded by the locale's sizes, so the cost is linear in the run.
#[allow(clippy::type_complexity)]
fn grouped<T: Unit>(s: &[T], sep: &[T], radix: &[T], g: &[u8], kind: Grouped) -> Result<Option<(TryVec<T>, usize, usize, usize)>, ()> {
    let sizes = Sizes::new(g);
    let Some(first) = sizes.at(0) else { return Ok(None) };
    if sep.is_empty() || sep == radix || first == 0 {
        return Ok(None);
    }
    let byte = |i: usize| s.get(i).and_then(|u| u.as_byte());
    let mut head = s.iter().take_while(|u| matches!(u.as_byte(), Some(b' ' | b'\t'..=b'\r'))).count();
    if matches!(byte(head), Some(b'+' | b'-')) {
        head += 1;
    }
    let lead = byte(head);
    let hex = lead == Some(b'0') && matches!(byte(head + 1), Some(b'x' | b'X'));
    let on = match kind {
        Grouped::Int(base) => base == 10 || (base == 0 && lead != Some(b'0')),
        Grouped::Float => !hex,
    };
    if !on {
        return Ok(None);
    }
    // Forward: the first group, the separator count and the first later group whose size is
    // not the repeating size (`bad`).
    let g0 = s.get(head..).unwrap_or(&[]).iter().take_while(|&&u| is_digit(u)).count();
    if g0 == 0 {
        return Ok(None);
    }
    let repeat = if sizes.stopped { None } else { sizes.g.last().map(|&b| usize::from(b)) };
    let (mut i, mut k, mut bad) = (head + g0, 0usize, None);
    while sep_at(s, i, sep) {
        i += sep.len();
        let len = s.get(i..).unwrap_or(&[]).iter().take_while(|&&u| is_digit(u)).count();
        k += 1;
        if bad.is_none() && Some(len) != repeat {
            bad = Some(k);
        }
        i += len;
    }
    let run_end = i;
    if k == 0 {
        return Ok(None);
    }
    let bad = bad.unwrap_or(k + 1);
    let n = sizes.g.len();
    // Back: the candidate ends right of each separator, the rightmost valid one first.
    let (mut cut, mut pos, mut j) = (head + g0, run_end, k);
    while j >= 1 {
        let len_j = digits_back(s, pos, usize::MAX);
        let sep_pos = pos - len_j - sep.len();
        let ok = len_j >= first && {
            // The groups between: the near ones one by one, the far ones by `bad`.
            let mut e = sep_pos;
            let mut fine = true;
            for t in 1..j.min(n) {
                let want = sizes.at(t).unwrap_or(0);
                if digits_back(s, e, want) != want {
                    fine = false;
                    break;
                }
                e -= want + sep.len();
            }
            fine && (j <= n || (!sizes.stopped && j - n < bad)) && sizes.at(j).is_none_or(|c| g0 <= c)
        };
        if ok {
            cut = sep_pos + sep.len() + first;
            break;
        }
        pos = sep_pos;
        j -= 1;
    }
    if j == 0 {
        // No separator survives: parse the first group alone.
        return Ok(Some((TryVec::from_slice(s.get(..cut).unwrap_or(&[])).map_err(|_| ())?, cut, 0, cut)));
    }
    let mut text = TryVec::with_capacity(run_end + 1).map_err(|_| ())?;
    text.extend_from_slice(s.get(..head).unwrap_or(&[])).map_err(|_| ())?;
    let mut digits = 0;
    for &u in s.get(head..cut).unwrap_or(&[]) {
        if is_digit(u) {
            text.push(u).map_err(|_| ())?;
            digits += 1;
        }
    }
    if matches!(kind, Grouped::Float) && cut == run_end {
        text.extend_from_slice(s.get(cut..).unwrap_or(&[])).map_err(|_| ())?;
    }
    Ok(Some((text, head, digits, cut)))
}

/// Parse `s` (at `addr`, end cursor `e`) with `parse` honouring the compat `group` flag: the
/// separator `sep` and `LC_NUMERIC` grouping `g` of the current locale. No memory for the copy:
/// no conversion and `ENOMEM`.
#[allow(clippy::too_many_arguments)]
fn with_group<T: Unit, R>(s: &[T], addr: usize, e: Option<impl liella_legacy::string_compat::Cursor>, sep: &[T], radix: &[T], g: &[u8], kind: Grouped, parse: impl Fn(&[T], Option<&mut Rec>) -> (R, Option<Errno>)) -> (R, Option<Errno>) {
    use liella_legacy::string_compat::Cursor as _;
    let unit = core::mem::size_of::<T>();
    let mut rec = Rec(None);
    let (r, at) = match grouped(s, sep, radix, g, kind) {
        Ok(None) => {
            let r = parse(s, Some(&mut rec));
            (r, rec.0.map(|o| o / unit))
        }
        Ok(Some((text, head, digits, cut))) => {
            let r = parse(text.as_slice(), Some(&mut rec));
            let at = rec.0.map(|o| o / unit).map(|u| if u <= head { u } else if u >= head + digits { cut + (u - head - digits) } else {
                // Inside the digits (not reached by a decimal parse): the source of digit `u - head`.
                s.iter().enumerate().skip(head).filter(|&(_, &c)| is_digit(c)).nth(u - head).map_or(cut, |(i, _)| i)
            });
            (r, at)
        }
        Err(()) => {
            let r = parse(&[], Some(&mut rec));
            ((r.0, Some(Errno::ENOMEM)), Some(0))
        }
    };
    if let (Some(at), Some(mut e)) = (at, e) {
        e.set(addr.saturating_add(at.saturating_mul(unit)));
    }
    r
}

/// The current locale's thousands separator in wide units (`_NL_NUMERIC_THOUSANDS_SEP_WC`),
/// empty when it has none.
fn wthousands(l: &liella::locale::Locale) -> ([i32; 1], usize) {
    match l.item(liella::locale::file::Cat::Numeric, 4) {
        liella::locale::Item::Word(w) if w != 0 => ([i32::try_from(w).unwrap_or(0)], 1),
        _ => ([0], 0),
    }
}

/// `__strto*_internal` integers: `group == 0` is the plain parse (§8.1).
// UNSAFE(slice): the export's C string is read through `strz`.
#[allow(unsafe_code)]
fn strtol_group<R>(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, parse: impl Fn(&[u8], Option<&mut Rec>) -> (R, Option<Errno>)) -> R {
    let l = locale::current();
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(with_group(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), l.thousands_sep(), locale::radix(l), l.grouping(), Grouped::Int(base), parse))
}

/// `__wcsto*_internal` integers (§8.3).
// UNSAFE(slice): the export's wide string is read through `strz`.
#[allow(unsafe_code)]
fn wcstol_group<R>(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, parse: impl Fn(&[wchar_t], Option<&mut Rec>) -> (R, Option<Errno>)) -> R {
    let l = locale::current();
    let (sep, n) = wthousands(l);
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(with_group(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), sep.get(..n).unwrap_or(&[]), &locale::wradix(l), l.grouping(), Grouped::Int(base), parse))
}

/// `__strto{d,f,ld}_internal` with `group` set (§8.2): the float bits in `fmt`.
// UNSAFE(slice): the export's C string is read through `strz`.
#[allow(unsafe_code)]
fn strtof_group(s: *const c_char, e: Option<&mut *mut c_char>, fmt: Format) -> u128 {
    let l = locale::current();
    let radix = locale::radix(l);
    let mode = num_compat::rounding(CSys.mxcsr());
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.2).
    ret::with_errno(with_group(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), l.thousands_sep(), radix, l.grouping(), Grouped::Float, |t, c| num_compat::strto_float(t, 0, c, fmt, mode, radix)))
}

/// `__wcsto{d,f,ld}_internal` with `group` set (§8.3).
// UNSAFE(slice): the export's wide string is read through `strz`.
#[allow(unsafe_code)]
fn wcstof_group(s: *const wchar_t, e: Option<&mut *mut wchar_t>, fmt: Format) -> u128 {
    let l = locale::current();
    let radix = locale::wradix(l);
    let (sep, n) = wthousands(l);
    let mode = num_compat::rounding(CSys.mxcsr());
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(with_group(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), sep.get(..n).unwrap_or(&[]), &radix, l.grouping(), Grouped::Float, |t, c| num_compat::strto_float(t, 0, c, fmt, mode, &radix)))
}

/// The narrow signed `strtol` family (§8.1): the common shapes from the first
/// [`STRTOL_HEAD`] bytes without measuring the text (STRTOIMAX), the rest by the full parse.
// UNSAFE(slice): the export's C string is read through `strz`.
#[allow(unsafe_code)]
fn strtol_narrow(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, dialect: Dialect) -> c_long {
    // UNSAFE(slice) SAFETY: s is a string; at most STRTOL_HEAD bytes of it are read (§8.1).
    let head = ret::must(unsafe { strz::strz_n(s, STRTOL_HEAD) });
    let mut e = e;
    if let Some(v) = num_compat::strtol_short(head, conv::addr(s), conv::end(e.as_deref_mut()), base, head.len() < STRTOL_HEAD) {
        return v;
    }
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, dialect))
}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtol(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_long {
    strtol_narrow(s, e, base, Dialect::C99)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtoll(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_long {
    strtol_narrow(s, e, base, Dialect::C99)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtoq(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_long {
    strtol_narrow(s, e, base, Dialect::C99)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtoimax(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_long {
    strtol_narrow(s, e, base, Dialect::C99)
}}

export! { "GLIBC_2.3";
pub extern "C" fn strtol_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_long {
    strtol_narrow(s, e, base, Dialect::C99)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtol_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_long {
    strtol_narrow(s, e, base, Dialect::C99)
}}

export! { "GLIBC_2.3.3";
pub extern "C" fn strtoll_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_long {
    strtol_narrow(s, e, base, Dialect::C99)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtoll_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_long {
    strtol_narrow(s, e, base, Dialect::C99)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtol_internal(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, group: c_int) -> c_long {
    if group != 0 {
        return strtol_group(s, e, base, |t, c| num_compat::strtol(t, 0, c, base, Dialect::C99));
    }
    strtol_narrow(s, e, base, Dialect::C99)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtoll_internal(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, group: c_int) -> c_long {
    if group != 0 {
        return strtol_group(s, e, base, |t, c| num_compat::strtol(t, 0, c, base, Dialect::C99));
    }
    strtol_narrow(s, e, base, Dialect::C99)
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_strtol(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_long {
    strtol_narrow(s, e, base, Dialect::C23)
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_strtoll(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_long {
    strtol_narrow(s, e, base, Dialect::C23)
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_strtoimax(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_long {
    strtol_narrow(s, e, base, Dialect::C23)
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_strtol_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_long {
    strtol_narrow(s, e, base, Dialect::C23)
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_strtoll_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_long {
    strtol_narrow(s, e, base, Dialect::C23)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtoul(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtoull(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtouq(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtoumax(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.3";
pub extern "C" fn strtoul_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtoul_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.3.3";
pub extern "C" fn strtoull_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtoull_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtoul_internal(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, group: c_int) -> c_ulong {
    if group != 0 {
        return strtol_group(s, e, base, |t, c| num_compat::strtoul(t, 0, c, base, Dialect::C99));
    }
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtoull_internal(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, group: c_int) -> c_ulong {
    if group != 0 {
        return strtol_group(s, e, base, |t, c| num_compat::strtoul(t, 0, c, base, Dialect::C99));
    }
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_strtoul(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_strtoull(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_strtoumax(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_strtoul_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_strtoull_l(s: *const c_char, e: Option<&mut *mut c_char>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstol(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstoll(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstoq(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstoimax(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.3";
pub extern "C" fn wcstol_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstol_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.3";
pub extern "C" fn wcstoll_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstoll_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstol_internal(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, group: c_int) -> c_long {
    if group != 0 {
        return wcstol_group(s, e, base, |t, c| num_compat::strtol(t, 0, c, base, Dialect::C99));
    }
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstoll_internal(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, group: c_int) -> c_long {
    if group != 0 {
        return wcstol_group(s, e, base, |t, c| num_compat::strtol(t, 0, c, base, Dialect::C99));
    }
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_wcstol(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_wcstoll(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_wcstoimax(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_wcstol_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_wcstoll_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_long {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtol(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstoul(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstoull(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstouq(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstoumax(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.3";
pub extern "C" fn wcstoul_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstoul_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.3";
pub extern "C" fn wcstoull_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstoull_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstoul_internal(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, group: c_int) -> c_ulong {
    if group != 0 {
        return wcstol_group(s, e, base, |t, c| num_compat::strtoul(t, 0, c, base, Dialect::C99));
    }
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstoull_internal(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, group: c_int) -> c_ulong {
    if group != 0 {
        return wcstol_group(s, e, base, |t, c| num_compat::strtoul(t, 0, c, base, Dialect::C99));
    }
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C99))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_wcstoul(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_wcstoull(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_wcstoumax(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_wcstoul_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.38";
pub extern "C" fn __isoc23_wcstoull_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, base: c_int, _loc: locale_t) -> c_ulong {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtoul(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), base, Dialect::C23))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn atoi(s: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: s is a string (§8.1).
    num_compat::atoi(ret::must(unsafe { strz::strz(s) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn atol(s: *const c_char) -> c_long {
    // UNSAFE(slice) SAFETY: s is a string (§8.1).
    num_compat::atol(ret::must(unsafe { strz::strz(s) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn atoll(s: *const c_char) -> c_long {
    // UNSAFE(slice) SAFETY: s is a string (§8.1).
    num_compat::atol(ret::must(unsafe { strz::strz(s) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn atof(s: *const c_char) -> f64 {
    // UNSAFE(slice) SAFETY: s is a string (§8.2).
    num_compat::atof(ret::must(unsafe { strz::strz(s) }), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::current()))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtod(s: *const c_char, e: Option<&mut *mut c_char>) -> f64 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::current())))
}}

export! { "GLIBC_2.27";
pub extern "C" fn strtof64(s: *const c_char, e: Option<&mut *mut c_char>) -> f64 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::current())))
}}

export! { "GLIBC_2.27";
pub extern "C" fn strtof32x(s: *const c_char, e: Option<&mut *mut c_char>) -> f64 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::current())))
}}

export! { "GLIBC_2.3";
pub extern "C" fn strtod_l(s: *const c_char, e: Option<&mut *mut c_char>, loc: locale_t) -> f64 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtod_l(s: *const c_char, e: Option<&mut *mut c_char>, loc: locale_t) -> f64 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.27";
pub extern "C" fn strtof64_l(s: *const c_char, e: Option<&mut *mut c_char>, loc: locale_t) -> f64 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.27";
pub extern "C" fn strtof32x_l(s: *const c_char, e: Option<&mut *mut c_char>, loc: locale_t) -> f64 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtod_internal(s: *const c_char, e: Option<&mut *mut c_char>, group: c_int) -> f64 {
    if group != 0 {
        return f64::from_bits(strtof_group(s, e, Format::F64) as u64);
    }
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::current())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn strtof(s: *const c_char, e: Option<&mut *mut c_char>) -> f32 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::current())))
}}

export! { "GLIBC_2.27";
pub extern "C" fn strtof32(s: *const c_char, e: Option<&mut *mut c_char>) -> f32 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::current())))
}}

export! { "GLIBC_2.3";
pub extern "C" fn strtof_l(s: *const c_char, e: Option<&mut *mut c_char>, loc: locale_t) -> f32 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtof_l(s: *const c_char, e: Option<&mut *mut c_char>, loc: locale_t) -> f32 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.27";
pub extern "C" fn strtof32_l(s: *const c_char, e: Option<&mut *mut c_char>, loc: locale_t) -> f32 {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __strtof_internal(s: *const c_char, e: Option<&mut *mut c_char>, group: c_int) -> f32 {
    if group != 0 {
        return f32::from_bits(strtof_group(s, e, Format::F32) as u32);
    }
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), locale::radix(locale::current())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstod(s: *const wchar_t, e: Option<&mut *mut wchar_t>) -> f64 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::current())))
}}

export! { "GLIBC_2.27";
pub extern "C" fn wcstof64(s: *const wchar_t, e: Option<&mut *mut wchar_t>) -> f64 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::current())))
}}

export! { "GLIBC_2.27";
pub extern "C" fn wcstof32x(s: *const wchar_t, e: Option<&mut *mut wchar_t>) -> f64 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::current())))
}}

export! { "GLIBC_2.3";
pub extern "C" fn wcstod_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, loc: locale_t) -> f64 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstod_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, loc: locale_t) -> f64 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.27";
pub extern "C" fn wcstof64_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, loc: locale_t) -> f64 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.27";
pub extern "C" fn wcstof32x_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, loc: locale_t) -> f64 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstod_internal(s: *const wchar_t, e: Option<&mut *mut wchar_t>, group: c_int) -> f64 {
    if group != 0 {
        return f64::from_bits(wcstof_group(s, e, Format::F64) as u64);
    }
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtod(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::current())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wcstof(s: *const wchar_t, e: Option<&mut *mut wchar_t>) -> f32 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::current())))
}}

export! { "GLIBC_2.27";
pub extern "C" fn wcstof32(s: *const wchar_t, e: Option<&mut *mut wchar_t>) -> f32 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::current())))
}}

export! { "GLIBC_2.3";
pub extern "C" fn wcstof_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, loc: locale_t) -> f32 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstof_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, loc: locale_t) -> f32 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.27";
pub extern "C" fn wcstof32_l(s: *const wchar_t, e: Option<&mut *mut wchar_t>, loc: locale_t) -> f32 {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::of(conv::addr(loc)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __wcstof_internal(s: *const wchar_t, e: Option<&mut *mut wchar_t>, group: c_int) -> f32 {
    if group != 0 {
        return f32::from_bits(wcstof_group(s, e, Format::F32) as u32);
    }
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    ret::with_errno(num_compat::strtof(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::current())))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_strtold_internal_bits(s: *const c_char, e: Option<&mut *mut c_char>, group: c_int) -> LdBits {
    if group != 0 {
        return LdBits::from_bits(strtof_group(s, e, Format::F80));
    }
    liella_strtold_bits(s, e)
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_wcstold_internal_bits(s: *const wchar_t, e: Option<&mut *mut wchar_t>, group: c_int) -> LdBits {
    if group != 0 {
        return LdBits::from_bits(wcstof_group(s, e, Format::F80));
    }
    liella_wcstold_bits(s, e)
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_strtold_bits(s: *const c_char, e: Option<&mut *mut c_char>) -> LdBits {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    LdBits::from_bits(ret::with_errno(num_compat::strto_float(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), Format::F80, num_compat::rounding(CSys.mxcsr()), locale::radix(locale::current()))))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_strtof128_bits(s: *const c_char, e: Option<&mut *mut c_char>) -> F128Bits {
    // UNSAFE(slice) SAFETY: s is a string; e is NULL or a pointer variable (§8.1).
    F128Bits::from_bits(ret::with_errno(num_compat::strto_float(ret::must(unsafe { strz::strz(s) }), conv::addr(s), conv::end(e), Format::F128, num_compat::rounding(CSys.mxcsr()), locale::radix(locale::current()))))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_wcstold_bits(s: *const wchar_t, e: Option<&mut *mut wchar_t>) -> LdBits {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    LdBits::from_bits(ret::with_errno(num_compat::strto_float(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), Format::F80, num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::current()))))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_wcstof128_bits(s: *const wchar_t, e: Option<&mut *mut wchar_t>) -> F128Bits {
    // UNSAFE(slice) SAFETY: s is a wide string; e is NULL or a pointer variable (§8.3).
    F128Bits::from_bits(ret::with_errno(num_compat::strto_float(ret::must(unsafe { strz::wcsz(s) }), conv::addr(s), conv::end(e), Format::F128, num_compat::rounding(CSys.mxcsr()), &locale::wradix(locale::current()))))
}}

export! { "GLIBC_2.25";
pub extern "C" fn strfromd(buf: *mut c_char, n: size_t, f: *const c_char, x: f64) -> c_int {
    // UNSAFE(slice) SAFETY: buf has room for n bytes (or n is 0); fmt is a string (§8.4).
    ret::int(unsafe { strz::strz(f) }.and_then(|t| unsafe { conv::buf_mut(buf.cast(), n) }.and_then(|b| num_compat::strfrom(b, t, num_compat::from_f64(x), Format::F64, num_compat::rounding(CSys.mxcsr())))))
}}

export! { "GLIBC_2.27";
pub extern "C" fn strfromf64(buf: *mut c_char, n: size_t, f: *const c_char, x: f64) -> c_int {
    // UNSAFE(slice) SAFETY: buf has room for n bytes (or n is 0); fmt is a string (§8.4).
    ret::int(unsafe { strz::strz(f) }.and_then(|t| unsafe { conv::buf_mut(buf.cast(), n) }.and_then(|b| num_compat::strfrom(b, t, num_compat::from_f64(x), Format::F64, num_compat::rounding(CSys.mxcsr())))))
}}

export! { "GLIBC_2.27";
pub extern "C" fn strfromf32x(buf: *mut c_char, n: size_t, f: *const c_char, x: f64) -> c_int {
    // UNSAFE(slice) SAFETY: buf has room for n bytes (or n is 0); fmt is a string (§8.4).
    ret::int(unsafe { strz::strz(f) }.and_then(|t| unsafe { conv::buf_mut(buf.cast(), n) }.and_then(|b| num_compat::strfrom(b, t, num_compat::from_f64(x), Format::F64, num_compat::rounding(CSys.mxcsr())))))
}}

export! { "GLIBC_2.25";
pub extern "C" fn strfromf(buf: *mut c_char, n: size_t, f: *const c_char, x: f32) -> c_int {
    // UNSAFE(slice) SAFETY: buf has room for n bytes (or n is 0); fmt is a string (§8.4).
    ret::int(unsafe { strz::strz(f) }.and_then(|t| unsafe { conv::buf_mut(buf.cast(), n) }.and_then(|b| num_compat::strfrom(b, t, num_compat::from_f32(x), Format::F32, num_compat::rounding(CSys.mxcsr())))))
}}

export! { "GLIBC_2.27";
pub extern "C" fn strfromf32(buf: *mut c_char, n: size_t, f: *const c_char, x: f32) -> c_int {
    // UNSAFE(slice) SAFETY: buf has room for n bytes (or n is 0); fmt is a string (§8.4).
    ret::int(unsafe { strz::strz(f) }.and_then(|t| unsafe { conv::buf_mut(buf.cast(), n) }.and_then(|b| num_compat::strfrom(b, t, num_compat::from_f32(x), Format::F32, num_compat::rounding(CSys.mxcsr())))))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_strfroml_bits(buf: *mut c_char, n: size_t, f: *const c_char, x: LdBits) -> c_int {
    // UNSAFE(slice) SAFETY: buf has room for n bytes (or n is 0); fmt is a string (§8.4).
    ret::int(unsafe { strz::strz(f) }.and_then(|t| unsafe { conv::buf_mut(buf.cast(), n) }.and_then(|b| num_compat::strfrom(b, t, Float::unpack(x.to_bits(), Format::F80), Format::F80, num_compat::rounding(CSys.mxcsr())))))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_strfromf128_bits(buf: *mut c_char, n: size_t, f: *const c_char, x: F128Bits) -> c_int {
    // UNSAFE(slice) SAFETY: buf has room for n bytes (or n is 0); fmt is a string (§8.4).
    ret::int(unsafe { strz::strz(f) }.and_then(|t| unsafe { conv::buf_mut(buf.cast(), n) }.and_then(|b| num_compat::strfrom(b, t, Float::unpack(x.to_bits(), Format::F128), Format::F128, num_compat::rounding(CSys.mxcsr())))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ecvt(x: f64, ndigit: c_int, decpt: Option<&mut c_int>, sign: Option<&mut c_int>) -> *mut c_char {
    ret::null(crate::users::result_buf::<512>(crate::users::ECVT).map(|b| ret::must(conv::req(decpt).and_then(|d| conv::req(sign).map(|g| num_compat::cvt_static(b, num_compat::from_f64(x), ndigit.min(num_compat::DOUBLE_DIGITS), num_compat::rounding(CSys.mxcsr()), d, g, false))))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fcvt(x: f64, ndigit: c_int, decpt: Option<&mut c_int>, sign: Option<&mut c_int>) -> *mut c_char {
    ret::null(crate::users::result_buf::<512>(crate::users::FCVT).map(|b| ret::must(conv::req(decpt).and_then(|d| conv::req(sign).map(|g| num_compat::cvt_static(b, num_compat::from_f64(x), ndigit.min(num_compat::DOUBLE_DIGITS), num_compat::rounding(CSys.mxcsr()), d, g, true))))))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_qecvt_bits(x: LdBits, ndigit: c_int, decpt: Option<&mut c_int>, sign: Option<&mut c_int>) -> *mut c_char {
    ret::null(crate::users::result_buf::<5120>(crate::users::QECVT).map(|b| ret::must(conv::req(decpt).and_then(|d| conv::req(sign).map(|g| num_compat::cvt_static(b, Float::unpack(x.to_bits(), Format::F80), ndigit.min(num_compat::LONG_DOUBLE_DIGITS), num_compat::rounding(CSys.mxcsr()), d, g, false))))))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_qfcvt_bits(x: LdBits, ndigit: c_int, decpt: Option<&mut c_int>, sign: Option<&mut c_int>) -> *mut c_char {
    ret::null(crate::users::result_buf::<5120>(crate::users::QFCVT).map(|b| ret::must(conv::req(decpt).and_then(|d| conv::req(sign).map(|g| num_compat::cvt_static(b, Float::unpack(x.to_bits(), Format::F80), ndigit.min(num_compat::LONG_DOUBLE_DIGITS), num_compat::rounding(CSys.mxcsr()), d, g, true))))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ecvt_r(x: f64, ndigit: c_int, decpt: Option<&mut c_int>, sign: Option<&mut c_int>, buf: *mut c_char, len: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: decpt and sign are valid int variables; buf has room for len bytes (§8.4).
    ret::int(conv::req(decpt).and_then(|d| conv::req(sign).and_then(|g| unsafe { conv::buf_mut(buf.cast(), len) }.map(|b| num_compat::cvt_r(num_compat::from_f64(x), ndigit.min(num_compat::DOUBLE_DIGITS), num_compat::rounding(CSys.mxcsr()), d, g, b, false)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn fcvt_r(x: f64, ndigit: c_int, decpt: Option<&mut c_int>, sign: Option<&mut c_int>, buf: *mut c_char, len: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: decpt and sign are valid int variables; buf has room for len bytes (§8.4).
    ret::int(conv::req(decpt).and_then(|d| conv::req(sign).and_then(|g| unsafe { conv::buf_mut(buf.cast(), len) }.map(|b| num_compat::cvt_r(num_compat::from_f64(x), ndigit.min(num_compat::DOUBLE_DIGITS), num_compat::rounding(CSys.mxcsr()), d, g, b, true)))))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_qecvt_r_bits(x: LdBits, ndigit: c_int, decpt: Option<&mut c_int>, sign: Option<&mut c_int>, buf: *mut c_char, len: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: decpt and sign are valid int variables; buf has room for len bytes (§8.4).
    ret::int(conv::req(decpt).and_then(|d| conv::req(sign).and_then(|g| unsafe { conv::buf_mut(buf.cast(), len) }.map(|b| num_compat::cvt_r(Float::unpack(x.to_bits(), Format::F80), ndigit.min(num_compat::LONG_DOUBLE_DIGITS), num_compat::rounding(CSys.mxcsr()), d, g, b, false)))))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_qfcvt_r_bits(x: LdBits, ndigit: c_int, decpt: Option<&mut c_int>, sign: Option<&mut c_int>, buf: *mut c_char, len: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: decpt and sign are valid int variables; buf has room for len bytes (§8.4).
    ret::int(conv::req(decpt).and_then(|d| conv::req(sign).and_then(|g| unsafe { conv::buf_mut(buf.cast(), len) }.map(|b| num_compat::cvt_r(Float::unpack(x.to_bits(), Format::F80), ndigit.min(num_compat::LONG_DOUBLE_DIGITS), num_compat::rounding(CSys.mxcsr()), d, g, b, true)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn gcvt(x: f64, ndigit: c_int, buf: *mut c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: buf has room for the %.*g result (§8.4).
    ret::same(buf, num_compat::gcvt_into(unsafe { conv::dest::<u8, _>(buf) }, num_compat::from_f64(x), Format::F64, ndigit, num_compat::rounding(CSys.mxcsr())))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_qgcvt_bits(x: LdBits, ndigit: c_int, buf: *mut c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: buf has room for the %.*g result (§8.4).
    ret::same(buf, num_compat::gcvt_into(unsafe { conv::dest::<u8, _>(buf) }, Float::unpack(x.to_bits(), Format::F80), Format::F80, ndigit, num_compat::rounding(CSys.mxcsr())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn abs(n: c_int) -> c_int {
    num_compat::abs32(n)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn labs(n: c_long) -> c_long {
    num_compat::abs64(n)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn llabs(n: c_long) -> c_long {
    num_compat::abs64(n)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn imaxabs(n: c_long) -> c_long {
    num_compat::abs64(n)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn div(n: c_int, d: c_int) -> div_t {
    pack::div(ret::must(num_compat::div32(n, d)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ldiv(n: c_long, d: c_long) -> ldiv_t {
    pack::ldiv(ret::must(num_compat::div64(n, d)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn lldiv(n: c_long, d: c_long) -> lldiv_t {
    pack::lldiv(ret::must(num_compat::div64(n, d)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn imaxdiv(n: c_long, d: c_long) -> imaxdiv_t {
    pack::imaxdiv(ret::must(num_compat::div64(n, d)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ffs(n: c_int) -> c_int {
    num_compat::ffs32(n)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __ffs(n: c_int) -> c_int {
    num_compat::ffs32(n)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ffsl(n: c_long) -> c_int {
    num_compat::ffs64(n)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ffsll(n: c_long) -> c_int {
    num_compat::ffs64(n)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn l64a(n: c_long) -> *mut c_char {
    ret::null(crate::users::result_buf::<7>(crate::users::L64A).map(|b| num_compat::l64a_into(b, n)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn a64l(s: *const c_char) -> c_long {
    // UNSAFE(slice) SAFETY: s is a string (§8.10).
    ret::must(unsafe { strz::strz(s) }.map(num::a64l))
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_leading_zeros_uc(x: c_uchar) -> c_uint {
    StdBit::leading_zeros_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_leading_zeros_us(x: c_ushort) -> c_uint {
    StdBit::leading_zeros_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_leading_zeros_ui(x: c_uint) -> c_uint {
    StdBit::leading_zeros_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_leading_zeros_ul(x: c_ulong) -> c_uint {
    StdBit::leading_zeros_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_leading_zeros_ull(x: c_ulonglong) -> c_uint {
    StdBit::leading_zeros_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_leading_ones_uc(x: c_uchar) -> c_uint {
    StdBit::leading_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_leading_ones_us(x: c_ushort) -> c_uint {
    StdBit::leading_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_leading_ones_ui(x: c_uint) -> c_uint {
    StdBit::leading_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_leading_ones_ul(x: c_ulong) -> c_uint {
    StdBit::leading_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_leading_ones_ull(x: c_ulonglong) -> c_uint {
    StdBit::leading_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_trailing_zeros_uc(x: c_uchar) -> c_uint {
    StdBit::trailing_zeros_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_trailing_zeros_us(x: c_ushort) -> c_uint {
    StdBit::trailing_zeros_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_trailing_zeros_ui(x: c_uint) -> c_uint {
    StdBit::trailing_zeros_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_trailing_zeros_ul(x: c_ulong) -> c_uint {
    StdBit::trailing_zeros_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_trailing_zeros_ull(x: c_ulonglong) -> c_uint {
    StdBit::trailing_zeros_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_trailing_ones_uc(x: c_uchar) -> c_uint {
    StdBit::trailing_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_trailing_ones_us(x: c_ushort) -> c_uint {
    StdBit::trailing_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_trailing_ones_ui(x: c_uint) -> c_uint {
    StdBit::trailing_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_trailing_ones_ul(x: c_ulong) -> c_uint {
    StdBit::trailing_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_trailing_ones_ull(x: c_ulonglong) -> c_uint {
    StdBit::trailing_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_leading_zero_uc(x: c_uchar) -> c_uint {
    num::first_leading_zero(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_leading_zero_us(x: c_ushort) -> c_uint {
    num::first_leading_zero(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_leading_zero_ui(x: c_uint) -> c_uint {
    num::first_leading_zero(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_leading_zero_ul(x: c_ulong) -> c_uint {
    num::first_leading_zero(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_leading_zero_ull(x: c_ulonglong) -> c_uint {
    num::first_leading_zero(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_leading_one_uc(x: c_uchar) -> c_uint {
    num::first_leading_one(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_leading_one_us(x: c_ushort) -> c_uint {
    num::first_leading_one(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_leading_one_ui(x: c_uint) -> c_uint {
    num::first_leading_one(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_leading_one_ul(x: c_ulong) -> c_uint {
    num::first_leading_one(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_leading_one_ull(x: c_ulonglong) -> c_uint {
    num::first_leading_one(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_trailing_zero_uc(x: c_uchar) -> c_uint {
    num::first_trailing_zero(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_trailing_zero_us(x: c_ushort) -> c_uint {
    num::first_trailing_zero(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_trailing_zero_ui(x: c_uint) -> c_uint {
    num::first_trailing_zero(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_trailing_zero_ul(x: c_ulong) -> c_uint {
    num::first_trailing_zero(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_trailing_zero_ull(x: c_ulonglong) -> c_uint {
    num::first_trailing_zero(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_trailing_one_uc(x: c_uchar) -> c_uint {
    num::first_trailing_one(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_trailing_one_us(x: c_ushort) -> c_uint {
    num::first_trailing_one(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_trailing_one_ui(x: c_uint) -> c_uint {
    num::first_trailing_one(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_trailing_one_ul(x: c_ulong) -> c_uint {
    num::first_trailing_one(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_first_trailing_one_ull(x: c_ulonglong) -> c_uint {
    num::first_trailing_one(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_count_zeros_uc(x: c_uchar) -> c_uint {
    num::count_zeros(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_count_zeros_us(x: c_ushort) -> c_uint {
    num::count_zeros(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_count_zeros_ui(x: c_uint) -> c_uint {
    num::count_zeros(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_count_zeros_ul(x: c_ulong) -> c_uint {
    num::count_zeros(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_count_zeros_ull(x: c_ulonglong) -> c_uint {
    num::count_zeros(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_count_ones_uc(x: c_uchar) -> c_uint {
    StdBit::count_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_count_ones_us(x: c_ushort) -> c_uint {
    StdBit::count_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_count_ones_ui(x: c_uint) -> c_uint {
    StdBit::count_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_count_ones_ul(x: c_ulong) -> c_uint {
    StdBit::count_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_count_ones_ull(x: c_ulonglong) -> c_uint {
    StdBit::count_ones_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_has_single_bit_uc(x: c_uchar) -> bool {
    num::has_single_bit(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_has_single_bit_us(x: c_ushort) -> bool {
    num::has_single_bit(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_has_single_bit_ui(x: c_uint) -> bool {
    num::has_single_bit(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_has_single_bit_ul(x: c_ulong) -> bool {
    num::has_single_bit(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_has_single_bit_ull(x: c_ulonglong) -> bool {
    num::has_single_bit(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_width_uc(x: c_uchar) -> c_uint {
    num::bit_width(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_width_us(x: c_ushort) -> c_uint {
    num::bit_width(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_width_ui(x: c_uint) -> c_uint {
    num::bit_width(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_width_ul(x: c_ulong) -> c_uint {
    num::bit_width(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_width_ull(x: c_ulonglong) -> c_uint {
    num::bit_width(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_floor_uc(x: c_uchar) -> c_uchar {
    StdBit::bit_floor_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_floor_us(x: c_ushort) -> c_ushort {
    StdBit::bit_floor_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_floor_ui(x: c_uint) -> c_uint {
    StdBit::bit_floor_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_floor_ul(x: c_ulong) -> c_ulong {
    StdBit::bit_floor_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_floor_ull(x: c_ulonglong) -> c_ulonglong {
    StdBit::bit_floor_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_ceil_uc(x: c_uchar) -> c_uchar {
    StdBit::bit_ceil_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_ceil_us(x: c_ushort) -> c_ushort {
    StdBit::bit_ceil_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_ceil_ui(x: c_uint) -> c_uint {
    StdBit::bit_ceil_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_ceil_ul(x: c_ulong) -> c_ulong {
    StdBit::bit_ceil_(x)
}}

export! { "GLIBC_2.39";
pub extern "C" fn stdc_bit_ceil_ull(x: c_ulonglong) -> c_ulonglong {
    StdBit::bit_ceil_(x)
}}
