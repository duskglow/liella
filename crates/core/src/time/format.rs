//! `strftime` and `wcsftime` (spec §23.7; D-23.7): broken-down time to text, in bytes or in wide
//! characters, by one engine generic over the output unit.
//!
//! **Directives.** `%` then flags (`_` pad with spaces, `-` no padding, `0` pad with zeros, `^`
//! upper case, `#` swap case: names upper, `%Z` and `%p` lower; the last of `_ - 0` wins), a
//! width (a minimum, in output units), an `E` or `O` modifier, and the conversion. `E` applies
//! to `c C x X y Y` (and `z`), `O` to `d e H I m M S u U V w W y` (POSIX), to `C j g k l`, to
//! `b B h` (the nominative month names) and to `z`. Anything else is not a directive: its text,
//! from the `%` through the character that ended it, is copied (padded to the width).
//!
//! **Numbers** have a natural width (`%d` 2, `%j` 3, `%Y` 1, ...) padded with zeros (spaces for
//! `%e %k %l %s`); zeros go after a minus sign, spaces before it, and the width includes the
//! sign. `%C` has at least two digits (ISO C: 00–99; POSIX: "the number of digits ... or two").
//! With `O` and a locale with alternative digits, 0–99 print as the locale's symbols (padded
//! with spaces). `%z` is the sign and `hhmm` of `tm_gmtoff`, empty when `tm_isdst < 0`; the
//! flags pad it as a whole (`hh` and `mm` stay two digits each).
//!
//! **Names and formats** come from `LC_TIME` ([`super::lctime`]); for wide output they are
//! decoded from the category's codeset. Case changes are per character (`LC_CTYPE`'s case
//! maps), so `^` upper-cases non-ASCII letters too.
//!
//! **Result.** The text is written while each directive's whole output fits, with a terminator;
//! when something does not fit, writing stops (no terminator) and the result is
//! [`Written::Truncated`] with the length that would have been needed (D-23.7). An empty result
//! fits in one unit.
#![allow(clippy::arithmetic_side_effects)] // struct tm fields are i32 widened to i64/i128 (no overflow); lengths saturate

use crate::ctype::{wtrans, Trans};
use crate::error::Result;
use crate::locale::Locale;
use crate::mem::InlineVec;

/// A field's units while it is built: inline up to 64 (names, numbers, zones), on the heap
/// beyond (a long composite format).
type Buf<U> = InlineVec<U, 64>;
use crate::text::{self, Decoded, Encoding, MbState};

use super::civil;
use super::lctime::{self, Time};

/// A broken-down time as `strftime` reads it: `struct tm`'s fields as given (`year` is
/// `tm_year`, the year − 1900; `mon` is 0-based) and the zone abbreviation (`None`: NULL).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Tm<'a> {
    /// `tm_sec`.
    pub sec: i32,
    /// `tm_min`.
    pub min: i32,
    /// `tm_hour`.
    pub hour: i32,
    /// `tm_mday`.
    pub mday: i32,
    /// `tm_mon`.
    pub mon: i32,
    /// `tm_year`.
    pub year: i32,
    /// `tm_wday`.
    pub wday: i32,
    /// `tm_yday`.
    pub yday: i32,
    /// `tm_isdst`.
    pub isdst: i32,
    /// `tm_gmtoff`.
    pub gmtoff: i64,
    /// `tm_zone`'s text.
    pub zone: Option<&'a [u8]>,
}

impl Tm<'_> {
    /// The full year.
    pub fn full_year(&self) -> i64 {
        i64::from(self.year) + 1900
    }
}

/// What the formatter asks of the process's time zone (§23.6: `strftime` uses the zone as if it
/// had called `tzset`). Called only by the directives that need it.
pub trait Env {
    /// The current zone's abbreviation for standard (`false`) or daylight time: `%Z` when
    /// `tm_zone` is NULL or empty.
    fn zone_name(&self, dst: bool) -> &[u8];
    /// `%s`: the seconds since the epoch of `tm` read as local time (`mktime`).
    fn seconds(&self, tm: &Tm<'_>) -> i128;
}

/// An output unit: a byte (`strftime`) or a `wchar_t` (`wcsftime`).
pub trait Unit: Copy + Default + PartialEq + core::fmt::Debug {
    /// Wide output.
    const WIDE: bool;
    /// The unit's value (a byte or a code point).
    fn code(self) -> u32;
    /// A unit of value `c` (an ASCII character, or for wide output any character).
    fn of(c: u32) -> Self;
    /// Multibyte text as units when that needs no decoding: bytes as they are (byte output),
    /// `None` for wide output.
    fn bytes(s: &[u8]) -> Option<&[Self]>;
    /// Copy `src` over the start of `dst` (the shorter's length). A field is a few units, so byte
    /// output stores it with a couple of wide loads and stores, not a call to the library's
    /// `memmove` (LV-4).
    #[inline]
    fn copy_into(dst: &mut [Self], src: &[Self]) {
        for (d, &s) in dst.iter_mut().zip(src) {
            *d = s;
        }
    }
}

impl Unit for u8 {
    const WIDE: bool = false;

    fn code(self) -> u32 {
        u32::from(self)
    }

    fn of(c: u32) -> u8 {
        c.to_le_bytes()[0]
    }

    fn bytes(s: &[u8]) -> Option<&[u8]> {
        Some(s)
    }

    #[inline]
    fn copy_into(dst: &mut [u8], src: &[u8]) {
        let n = dst.len().min(src.len());
        let (Some(d), Some(s)) = (dst.get_mut(..n), src.get(..n)) else { return };
        // Up to 16 bytes: two possibly overlapping stores of the widest size that fits (the
        // overlap rewrites bytes with the same values).
        match n {
            0 => {}
            1 => {
                if let (Some(a), Some(b)) = (d.first_mut(), s.first()) {
                    *a = *b;
                }
            }
            2..=3 => {
                if let (Some(a), Some(b)) = (d.first_chunk_mut::<2>(), s.first_chunk::<2>()) {
                    *a = *b;
                }
                if let (Some(c), Some(e)) = (d.last_chunk_mut::<2>(), s.last_chunk::<2>()) {
                    *c = *e;
                }
            }
            4..=7 => {
                if let (Some(a), Some(b)) = (d.first_chunk_mut::<4>(), s.first_chunk::<4>()) {
                    *a = *b;
                }
                if let (Some(c), Some(e)) = (d.last_chunk_mut::<4>(), s.last_chunk::<4>()) {
                    *c = *e;
                }
            }
            8..=16 => {
                if let (Some(a), Some(b)) = (d.first_chunk_mut::<8>(), s.first_chunk::<8>()) {
                    *a = *b;
                }
                if let (Some(c), Some(e)) = (d.last_chunk_mut::<8>(), s.last_chunk::<8>()) {
                    *c = *e;
                }
            }
            _ => d.copy_from_slice(s),
        }
    }
}

impl Unit for i32 {
    const WIDE: bool = true;

    fn code(self) -> u32 {
        u32::from_ne_bytes(self.to_ne_bytes())
    }

    fn of(c: u32) -> i32 {
        i32::from_ne_bytes(c.to_ne_bytes())
    }

    fn bytes(_: &[u8]) -> Option<&[i32]> {
        None
    }
}

/// The outcome of a call that fits or not (D-23.7: a full buffer is distinct from an empty
/// result).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Written {
    /// The text and its terminator were written; its length in units.
    Fits(usize),
    /// It did not fit; the length the text has.
    Truncated {
        /// Units needed, without the terminator.
        needed: usize,
    },
}

/// Padding (the last of `_`, `-`, `0`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Pad {
    Default,
    Zero,
    Space,
    None,
}

/// A directive's flags and width.
#[derive(Clone, Copy, Debug)]
struct Spec {
    pad: Pad,
    upper: bool,
    swap: bool,
    width: Option<usize>,
}

/// How `#` changes a field's case.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Case {
    /// Names: `#` upper-cases.
    Name,
    /// `%Z`, `%p`: `#` lower-cases (and wins over `^`).
    Lower,
    /// `%P`: always lower case unless `^`.
    AlwaysLower,
    /// Everything else: `#` does nothing.
    Other,
}

/// Where fields go: the caller's buffer, or the text of a composite conversion.
trait Out<U: Unit> {
    /// One field: `lead`, then `n` units of `fill`, then `body`, all or nothing.
    fn field(&mut self, lead: &[U], n: usize, fill: U, body: &[U]) -> Result<()>;
    /// One field of `body` alone (the direct path's whole fields).
    fn put(&mut self, body: &[U]) -> Result<()> {
        self.field(&[], 0, U::default(), body)
    }
    /// Ordinary text: each unit is a field of its own.
    fn literals(&mut self, s: &[U]) -> Result<()> {
        for u in s {
            self.field(&[], 0, *u, core::slice::from_ref(u))?;
        }
        Ok(())
    }
}

/// The caller's buffer: writing stops at the first field that does not fit with a terminator.
struct Sink<'b, U> {
    buf: &'b mut [U],
    pos: usize,
    total: usize,
    full: bool,
}

impl<U: Unit> Out<U> for Sink<'_, U> {
    fn field(&mut self, lead: &[U], n: usize, fill: U, body: &[U]) -> Result<()> {
        let len = lead.len().saturating_add(n).saturating_add(body.len());
        self.total = self.total.saturating_add(len);
        if self.full {
            return Ok(());
        }
        // Room for the field and a terminator after it.
        let end = self.pos.saturating_add(len);
        let room = end < self.buf.len();
        let Some(dst) = self.buf.get_mut(self.pos..end).filter(|_| room) else {
            self.full = true;
            return Ok(());
        };
        // Fields are a few units: they are stored one by one, not through calls to the
        // library's `memcpy` and `memset`.
        for (d, &s) in dst.iter_mut().zip(lead.iter().chain(core::iter::repeat_n(&fill, n)).chain(body)) {
            *d = s;
        }
        self.pos = end;
        Ok(())
    }

    #[inline]
    fn put(&mut self, body: &[U]) -> Result<()> {
        self.total = self.total.saturating_add(body.len());
        if self.full {
            return Ok(());
        }
        let end = self.pos.saturating_add(body.len());
        let room = end < self.buf.len();
        match self.buf.get_mut(self.pos..end).filter(|_| room) {
            Some(dst) => {
                U::copy_into(dst, body);
                self.pos = end;
            }
            None => self.full = true,
        }
        Ok(())
    }

    /// The units that fit with a terminator after them, as unit-by-unit fields would store them.
    #[inline]
    fn literals(&mut self, s: &[U]) -> Result<()> {
        self.total = self.total.saturating_add(s.len());
        if self.full {
            return Ok(());
        }
        let k = s.len().min(self.buf.len().saturating_sub(self.pos.saturating_add(1)));
        let end = self.pos.saturating_add(k);
        if let Some(dst) = self.buf.get_mut(self.pos..end) {
            U::copy_into(dst, s);
        }
        self.pos = end;
        if k < s.len() {
            self.full = true;
        }
        Ok(())
    }
}

impl<U: Unit> Out<U> for Buf<U> {
    fn field(&mut self, lead: &[U], n: usize, fill: U, body: &[U]) -> Result<()> {
        self.extend_from_slice(lead)?;
        for _ in 0..n {
            self.push(fill)?;
        }
        self.extend_from_slice(body)
    }

    fn put(&mut self, body: &[U]) -> Result<()> {
        self.extend_from_slice(body)
    }

    fn literals(&mut self, s: &[U]) -> Result<()> {
        self.extend_from_slice(s)
    }
}

/// The deepest nesting of locale formats (a format naming itself must not recurse forever).
pub(super) const DEPTH: u8 = 8;

struct Ctx<'a> {
    tm: &'a Tm<'a>,
    loc: &'a Locale,
    time: Time<'a>,
    env: &'a dyn Env,
    /// The encodings, found on first need (numeric formats never need them).
    encodings: core::cell::OnceCell<(Encoding, Encoding)>,
    /// Take the direct path for the common directives ([`direct`]); off only in the tests that
    /// compare it with the general one.
    direct: bool,
}

impl Ctx<'_> {
    fn encodings(&self) -> (Encoding, Encoding) {
        *self.encodings.get_or_init(|| {
            let ctype = Encoding::of(self.loc);
            (self.time.encoding().unwrap_or(ctype), ctype)
        })
    }

    /// The encoding of `LC_TIME`'s strings (decoded for wide output).
    fn text_enc(&self) -> Encoding {
        self.encodings().0
    }

    /// `LC_CTYPE`'s encoding (case changes of byte output, `tm_zone`).
    fn ctype_enc(&self) -> Encoding {
        self.encodings().1
    }
}

/// Format `tm` by `fmt` into `out` (`strftime`, `wcsftime`): see the module documentation.
/// Only memory exhaustion fails.
pub fn strftime<U: Unit>(out: &mut [U], fmt: &[U], tm: &Tm<'_>, loc: &Locale, env: &dyn Env) -> Result<Written> {
    format(out, fmt, tm, loc, env, true)
}

/// [`strftime`], with or without the direct path.
fn format<U: Unit>(out: &mut [U], fmt: &[U], tm: &Tm<'_>, loc: &Locale, env: &dyn Env, direct: bool) -> Result<Written> {
    let cx = Ctx { tm, loc, time: Time::of(loc), env, encodings: core::cell::OnceCell::new(), direct };
    let mut sink = Sink { buf: out, pos: 0, total: 0, full: false };
    run(&cx, fmt, &mut sink, 0, None)?;
    if sink.full {
        return Ok(Written::Truncated { needed: sink.total });
    }
    // Every field fitted with room for the terminator after it (an empty text needs one unit).
    match sink.buf.get_mut(sink.pos) {
        Some(t) => {
            *t = U::of(0);
            Ok(Written::Fits(sink.total))
        }
        None => Ok(Written::Truncated { needed: sink.total }),
    }
}

fn ascii<U: Unit>(s: &[u8], out: &mut Buf<U>) -> Result<()> {
    for &b in s {
        out.push(U::of(u32::from(b)))?;
    }
    Ok(())
}

/// Multibyte text in encoding `enc` as units: bytes as they are, or decoded characters (a byte
/// that does not decode stands for itself). The encoding is worked out only for wide output.
fn text_units<U: Unit>(s: &[u8], enc: impl FnOnce() -> Encoding, out: &mut Buf<U>) -> Result<()> {
    if !U::WIDE {
        for &b in s {
            out.push(U::of(u32::from(b)))?;
        }
        return Ok(());
    }
    let enc = enc();
    let mut rest = s;
    while let Some(&b) = rest.first() {
        let (c, n) = match text::decode(&mut { MbState::INITIAL }, rest, enc) {
            Decoded::Char(c, n) => (c, n.max(1)),
            _ => (u32::from(b), 1),
        };
        out.push(U::of(c))?;
        rest = rest.get(n..).unwrap_or(&[]);
    }
    Ok(())
}

/// Change the case of `body` per character (`LC_CTYPE`'s maps).
fn recase<U: Unit>(cx: &Ctx<'_>, body: &mut Buf<U>, t: Trans) -> Result<()> {
    if U::WIDE {
        for u in body.iter_mut() {
            *u = U::of(wtrans(u.code(), t, cx.loc));
        }
        return Ok(());
    }
    let mut bytes: InlineVec<u8, 64> = InlineVec::new();
    for u in body.iter() {
        bytes.push(u.code().to_le_bytes()[0])?;
    }
    body.clear();
    let mut rest = &bytes[..];
    while let Some(&b) = rest.first() {
        let mut enc = [0u8; 16];
        match text::decode(&mut { MbState::INITIAL }, rest, cx.ctype_enc()) {
            Decoded::Char(c, n) => {
                let k = text::encode(wtrans(c, t, cx.loc), cx.ctype_enc(), &mut enc);
                match (k, rest.get(..n.max(1))) {
                    (Some(k), _) => ascii_bytes(enc.get(..k).unwrap_or(&[]), body)?,
                    (None, Some(orig)) => ascii_bytes(orig, body)?,
                    (None, None) => {}
                }
                rest = rest.get(n.max(1)..).unwrap_or(&[]);
            }
            _ => {
                body.push(U::of(u32::from(b)))?;
                rest = rest.get(1..).unwrap_or(&[]);
            }
        }
    }
    Ok(())
}

fn ascii_bytes<U: Unit>(s: &[u8], out: &mut Buf<U>) -> Result<()> {
    for &b in s {
        out.push(U::of(u32::from(b)))?;
    }
    Ok(())
}

/// Format `fmt` into `out`. `inherit` is the padding of an enclosing `%EY`, given to the
/// directives of the era's format that have none of their own.
fn run<U: Unit, O: Out<U>>(cx: &Ctx<'_>, fmt: &[U], out: &mut O, depth: u8, inherit: Option<Pad>) -> Result<()> {
    let pct = U::of(u32::from(b'%'));
    let mut i = 0usize;
    while let Some(&u) = fmt.get(i) {
        if u != pct {
            if !cx.direct {
                out.field(&[], 0, u, core::slice::from_ref(&u))?;
                i += 1;
                continue;
            }
            let rest = fmt.get(i..).unwrap_or(&[]);
            let n = rest.iter().position(|&c| c == pct).unwrap_or(rest.len());
            out.literals(rest.get(..n).unwrap_or(&[]))?;
            i += n;
            continue;
        }
        // A conversion right after the `%` (no flag, width or modifier, no inherited padding).
        if cx.direct && inherit.is_none() {
            if let Some(c) = fmt.get(i + 1).and_then(|u| u8::try_from(u.code()).ok()) {
                if direct(cx, c, out, depth)? {
                    i += 2;
                    continue;
                }
            }
        }
        let start = i;
        let mut j = i + 1;
        let mut spec = Spec { pad: Pad::Default, upper: false, swap: false, width: None };
        while let Some(c) = fmt.get(j).map(|u| u.code()) {
            match u8::try_from(c) {
                Ok(b'_') => spec.pad = Pad::Space,
                Ok(b'-') => spec.pad = Pad::None,
                Ok(b'0') => spec.pad = Pad::Zero,
                Ok(b'^') => spec.upper = true,
                Ok(b'#') => spec.swap = true,
                _ => break,
            }
            j += 1;
        }
        while let Some(d) = fmt.get(j).map(|u| u.code()).filter(|c| (0x30..=0x39).contains(c)) {
            let w = spec.width.unwrap_or(0);
            spec.width = Some(w.saturating_mul(10).saturating_add(usize::try_from(d - 0x30).unwrap_or(0)));
            j += 1;
        }
        if spec.pad == Pad::Default {
            spec.pad = inherit.unwrap_or(Pad::Default);
        }
        let mut modifier = None;
        if let Some(m) = fmt.get(j).map(|u| u.code()).filter(|&c| c == u32::from(b'E') || c == u32::from(b'O')) {
            modifier = u8::try_from(m).ok();
            j += 1;
        }
        let conv = fmt.get(j).map(|u| u.code()).and_then(|c| u8::try_from(c).ok());
        let done = match conv {
            Some(c) if valid(c, modifier) => convert(cx, c, modifier, spec, out, depth)?,
            _ => false,
        };
        let end = (j + 1).min(fmt.len());
        if !done {
            // Not a directive: its text, padded to the width.
            let text = fmt.get(start..end).unwrap_or(&[]);
            let fill = U::of(u32::from(if spec.pad == Pad::Zero { b'0' } else { b' ' }));
            out.field(&[], spec.width.unwrap_or(0).saturating_sub(text.len()), fill, text)?;
        }
        i = end;
    }
    Ok(())
}

/// A table of the bytes in `set` (one load per lookup instead of a scan).
#[allow(clippy::indexing_slicing, clippy::as_conversions)] // a const fn (no `get_mut`); `i < set.len()` and `b < 128` are checked
const fn table(set: &[u8]) -> [bool; 128] {
    let mut t = [false; 128];
    let mut i = 0;
    while i < set.len() {
        let b = set[i] as usize;
        if b < 128 {
            t[b] = true;
        }
        i += 1;
    }
    t
}

const PLAIN: [bool; 128] = table(b"aAbBcCdDeFgGhHIjklmMnpPrRsStTuUVwWxXyYzZ%");
const WITH_E: [bool; 128] = table(b"cCxXyYz");
const WITH_O: [bool; 128] = table(b"deHImMSuUVwWyCjgklbBhz");

/// Whether `c` with `modifier` is a directive.
fn valid(c: u8, modifier: Option<u8>) -> bool {
    let t = match modifier {
        None => &PLAIN,
        Some(b'E') => &WITH_E,
        Some(_) => &WITH_O,
    };
    t.get(usize::from(c)).copied().unwrap_or(false)
}

/// A field of text units with the directive's case and width; spaces (zeros with `0`) pad it.
fn text_field<U: Unit>(cx: &Ctx<'_>, body: &mut Buf<U>, case: Case, spec: Spec, out: &mut dyn Out<U>) -> Result<()> {
    let t = match case {
        Case::Lower if spec.swap => Some(Trans::Lower),
        Case::Name if spec.swap => Some(Trans::Upper),
        _ if spec.upper => Some(Trans::Upper),
        Case::AlwaysLower => Some(Trans::Lower),
        _ => None,
    };
    if let Some(t) = t {
        recase(cx, body, t)?;
    }
    let fill = U::of(u32::from(if spec.pad == Pad::Zero { b'0' } else { b' ' }));
    out.field(&[], spec.width.unwrap_or(0).saturating_sub(body.len()), fill, body)
}

/// A number with its natural width and default padding (module documentation); `alt` uses the
/// locale's alternative digits when it has a symbol for `v`.
fn number<U: Unit>(cx: &Ctx<'_>, v: i128, natural: usize, default: Pad, spec: Spec, alt: bool, out: &mut dyn Out<U>) -> Result<()> {
    if alt {
        if let Some(s) = i64::try_from(v).ok().and_then(|n| cx.time.alt_digit(n)) {
            let mut body = Buf::new();
            text_units(s, || cx.text_enc(), &mut body)?;
            let fill = U::of(u32::from(if spec.pad == Pad::Zero { b'0' } else { b' ' }));
            return out.field(&[], spec.width.unwrap_or(0).saturating_sub(body.len()), fill, &body);
        }
    }
    // Most fields are below 10000 (all but years past 9999 and `%s`): four digits at most,
    // worked out directly.
    if let Ok(small) = u16::try_from(v) {
        if small < 10_000 {
            let all = [small / 1000, small / 100 % 10, small / 10 % 10, small % 10].map(|d| U::of(u32::from(d) + u32::from(b'0')));
            let skip = match small {
                0..=9 => 3,
                10..=99 => 2,
                100..=999 => 1,
                _ => 0,
            };
            return signed_field(all.get(skip..).unwrap_or(&[]), false, natural, default, spec, out);
        }
    }
    let mut digits = [0u8; 40];
    let mut n = 0usize;
    // 64-bit division for every value that fits (all but `%s` beyond `time_t`).
    if let Ok(mut a) = u64::try_from(v.unsigned_abs()) {
        loop {
            if let Some(d) = digits.get_mut(n) {
                *d = b'0' + (a % 10).to_le_bytes()[0];
            }
            n += 1;
            a /= 10;
            if a == 0 {
                break;
            }
        }
    } else {
        let mut a = v.unsigned_abs();
        loop {
            if let Some(d) = digits.get_mut(n) {
                *d = b'0' + (a % 10).to_le_bytes()[0];
            }
            n += 1;
            a /= 10;
            if a == 0 {
                break;
            }
        }
    }
    // At most 40 digits (a 128-bit value): no allocation.
    let mut body = [U::of(0); 40];
    for (slot, k) in body.iter_mut().zip((0..n).rev()) {
        *slot = U::of(u32::from(digits.get(k).copied().unwrap_or(b'0')));
    }
    signed_field(body.get(..n).unwrap_or(&[]), v < 0, natural, default, spec, out)
}

/// Digits with a sign, padded as numbers are (the width includes the sign).
fn signed_field<U: Unit>(digits: &[U], negative: bool, natural: usize, default: Pad, spec: Spec, out: &mut dyn Out<U>) -> Result<()> {
    let pad = if spec.pad == Pad::Default { default } else { spec.pad };
    let target = match (spec.width, pad) {
        (Some(w), _) => w,
        (None, Pad::None) => 0,
        (None, _) => natural,
    };
    let len = digits.len() + usize::from(negative);
    let n = target.saturating_sub(len);
    let minus = [U::of(u32::from(b'-'))];
    let sign: &[U] = if negative { &minus } else { &[] };
    match pad {
        Pad::Zero | Pad::Default => out.field(sign, n, U::of(u32::from(b'0')), digits),
        Pad::Space | Pad::None if len <= 41 => {
            let mut body = [U::of(0); 41];
            for (slot, &u) in body.iter_mut().zip(sign.iter().chain(digits)) {
                *slot = u;
            }
            out.field(&[], n, U::of(u32::from(b' ')), body.get(..len).unwrap_or(&[]))
        }
        Pad::Space | Pad::None => {
            let mut body: Buf<U> = InlineVec::new();
            body.extend_from_slice(sign)?;
            body.extend_from_slice(digits)?;
            out.field(&[], n, U::of(u32::from(b' ')), &body)
        }
    }
}

/// A locale format (bytes in `LC_TIME`'s codeset) run as a composite field.
fn composite<U: Unit>(cx: &Ctx<'_>, fmt: &[u8], case: Case, spec: Spec, out: &mut dyn Out<U>, depth: u8, inherit: Option<Pad>) -> Result<()> {
    let mut body: Buf<U> = InlineVec::new();
    if depth < DEPTH {
        match U::bytes(fmt).filter(|_| cx.direct) {
            // Byte output reads the format in place.
            Some(f) => run(cx, f, &mut body, depth + 1, inherit)?,
            None => {
                let mut f: Buf<U> = InlineVec::new();
                text_units(fmt, || cx.text_enc(), &mut f)?;
                run(cx, &f, &mut body, depth + 1, inherit)?;
            }
        }
    }
    text_field(cx, &mut body, case, spec, out)
}

/// `"00"` to `"99"`: two digits per lookup.
#[allow(clippy::indexing_slicing, clippy::as_conversions)] // a const block (no `get_mut`); `i < 100`
pub(super) const PAIRS: [[u8; 2]; 100] = {
    let mut t = [[0u8; 2]; 100];
    let mut i = 0u8;
    while i < 100 {
        t[i as usize] = [b'0' + i / 10, b'0' + i % 10];
        i += 1;
    }
    t
};

/// A direct-path field: up to 16 units on the stack (`%F`'s widest is 14).
pub(super) struct Small<U> {
    u: [U; 16],
    n: usize,
}

impl<U: Unit> Small<U> {
    pub(super) fn new() -> Small<U> {
        Small { u: [U::default(); 16], n: 0 }
    }

    /// Append `c`; always `true` (to chain with [`Small::num`]).
    #[inline(always)]
    pub(super) fn ch(&mut self, c: u8) -> bool {
        if let Some(slot) = self.u.get_mut(self.n) {
            *slot = U::of(u32::from(c));
            self.n += 1;
        }
        true
    }

    /// Append `v` as the general path writes a number with no flags, width or modifier: at least
    /// `natural` (at most 3) units, `fill` before the digits. `false` (nothing appended) unless
    /// `v` is in `0..=9999`.
    #[inline(always)]
    pub(super) fn num(&mut self, v: i64, natural: usize, fill: u8) -> bool {
        let Some(v) = u16::try_from(v).ok().filter(|&v| v < 10_000) else {
            return false;
        };
        let (Some(&[a, b]), Some(&[c, d])) = (PAIRS.get(usize::from(v / 100)), PAIRS.get(usize::from(v % 100))) else {
            return false;
        };
        let digits = match v {
            0..=9 => 1,
            10..=99 => 2,
            100..=999 => 3,
            _ => 4,
        };
        let len = natural.max(digits);
        let all = [a, b, c, d];
        // The last `len` of: fill up to 4 - digits, then the digits.
        for k in (4 - len)..4 {
            let unit = if k < 4 - digits { fill } else { all.get(k).copied().unwrap_or(b'0') };
            self.ch(unit);
        }
        true
    }

    pub(super) fn units(&self) -> &[U] {
        self.u.get(..self.n).unwrap_or(&[])
    }
}

/// The direct path: conversion `c` with no flag, width or modifier, written as the general path
/// writes it (the tests compare the two). `false` (nothing written) for every other conversion and
/// whenever a value or the output needs the general path: numbers outside `0..=9999`, wide names,
/// composites at the nesting limit.
#[inline]
fn direct<U: Unit, O: Out<U>>(cx: &Ctx<'_>, c: u8, out: &mut O, depth: u8) -> Result<bool> {
    let tm = cx.tm;
    let year = tm.full_year();
    let mut f = Small::<U>::new();
    let hour12 = || match i64::from(tm.hour).rem_euclid(12) {
        0 => 12,
        h => h,
    };
    let (mday, mon, hour, min, sec) = (i64::from(tm.mday), i64::from(tm.mon) + 1, i64::from(tm.hour), i64::from(tm.min), i64::from(tm.sec));
    let nested = depth < DEPTH;
    let done = match c {
        b'Y' => f.num(year, 1, b'0'),
        b'C' => f.num(year.div_euclid(100), 2, b'0'),
        b'y' => f.num(year.rem_euclid(100), 2, b'0'),
        b'm' => f.num(mon, 2, b'0'),
        b'd' => f.num(mday, 2, b'0'),
        b'e' => f.num(mday, 2, b' '),
        b'H' => f.num(hour, 2, b'0'),
        b'k' => f.num(hour, 2, b' '),
        b'I' => f.num(hour12(), 2, b'0'),
        b'l' => f.num(hour12(), 2, b' '),
        b'M' => f.num(min, 2, b'0'),
        b'S' => f.num(sec, 2, b'0'),
        b'j' => f.num(i64::from(tm.yday) + 1, 3, b'0'),
        b'u' => f.num((i64::from(tm.wday) - 1).rem_euclid(7) + 1, 1, b'0'),
        b'w' => f.num(i64::from(tm.wday), 1, b'0'),
        b'U' => f.num((i64::from(tm.yday) + 7 - i64::from(tm.wday).rem_euclid(7)).div_euclid(7), 2, b'0'),
        b'W' => f.num((i64::from(tm.yday) + 7 - (i64::from(tm.wday) + 6).rem_euclid(7)).div_euclid(7), 2, b'0'),
        // The fixed composites as one field, as `composite` makes them.
        b'T' => nested && f.num(hour, 2, b'0') && f.ch(b':') && f.num(min, 2, b'0') && f.ch(b':') && f.num(sec, 2, b'0'),
        b'R' => nested && f.num(hour, 2, b'0') && f.ch(b':') && f.num(min, 2, b'0'),
        b'D' => nested && f.num(mon, 2, b'0') && f.ch(b'/') && f.num(mday, 2, b'0') && f.ch(b'/') && f.num(year.rem_euclid(100), 2, b'0'),
        b'F' => nested && f.num(year, 1, b'0') && f.ch(b'-') && f.num(mon, 2, b'0') && f.ch(b'-') && f.num(mday, 2, b'0'),
        b'z' if tm.isdst < 0 => true,
        b'z' => {
            let a = tm.gmtoff.unsigned_abs();
            let (hh, mm) = (a / 3600, a / 60 % 60);
            // Below 100 hours, `hh` is the general path's two digits (`mm` is below 60).
            hh < 100
                && f.ch(if tm.gmtoff < 0 { b'-' } else { b'+' })
                && f.num(i64::try_from(hh).unwrap_or(-1), 2, b'0')
                && f.num(i64::try_from(mm).unwrap_or(-1), 2, b'0')
        }
        b'n' => f.ch(b'\n'),
        b't' => f.ch(b'\t'),
        b'%' => f.ch(b'%'),
        // Names: byte output only (wide output decodes them).
        b'a' => return named(cx.time.abday(i64::from(tm.wday)), out),
        b'A' => return named(cx.time.day(i64::from(tm.wday)), out),
        b'b' | b'h' => return named(cx.time.month(lctime::ABMON, i64::from(tm.mon)), out),
        b'B' => return named(cx.time.month(lctime::MON, i64::from(tm.mon)), out),
        b'p' => return named(Some(cx.time.string(if tm.hour >= 12 { lctime::PM } else { lctime::AM })), out),
        b'Z' => {
            let z = match tm.zone.filter(|z| !z.is_empty()) {
                Some(z) => z,
                None if tm.isdst < 0 => &[],
                None => cx.env.zone_name(tm.isdst > 0),
            };
            return named(Some(z), out);
        }
        _ => false,
    };
    if done {
        out.put(f.units())?;
    }
    Ok(done)
}

/// A name as one field (`"?"` when out of range), for byte output; `false` for wide output.
#[inline]
fn named<U: Unit, O: Out<U>>(s: Option<&[u8]>, out: &mut O) -> Result<bool> {
    match U::bytes(s.unwrap_or(b"?")) {
        Some(units) => {
            out.put(units)?;
            Ok(true)
        }
        None => Ok(false),
    }
}

/// ISO 8601 week-based year and week of `tm` (`%G`, `%g`, `%V`).
fn iso_week(tm: &Tm<'_>) -> (i64, i64) {
    let year = tm.full_year();
    let wd = (i64::from(tm.wday) + 6).rem_euclid(7); // Monday 0
    let mut thu = i64::from(tm.yday) - wd + 3;
    let mut iso = year;
    if thu < 0 {
        iso = year - 1;
        thu += civil::year_days(iso);
    } else if thu >= civil::year_days(year) {
        thu -= civil::year_days(year);
        iso = year + 1;
    }
    (iso, thu.div_euclid(7) + 1)
}

/// One conversion; `false` if it is not a directive after all.
fn convert<U: Unit>(cx: &Ctx<'_>, c: u8, modifier: Option<u8>, spec: Spec, out: &mut dyn Out<U>, depth: u8) -> Result<bool> {
    let tm = cx.tm;
    let alt = modifier == Some(b'O');
    let era_mod = modifier == Some(b'E');
    let year = tm.full_year();
    let era = || if era_mod { cx.time.era_of((year, i64::from(tm.mon) + 1, i64::from(tm.mday))) } else { None };
    let num = |v: i64, natural: usize, default: Pad, out: &mut dyn Out<U>| number(cx, i128::from(v), natural, default, spec, alt, out);
    let name = |s: Option<&[u8]>, case: Case, out: &mut dyn Out<U>| -> Result<()> {
        let mut body = Buf::new();
        text_units(s.unwrap_or(b"?"), || cx.text_enc(), &mut body)?;
        text_field(cx, &mut body, case, spec, out)
    };
    let lit = |s: &[u8], out: &mut dyn Out<U>| -> Result<()> {
        let mut body = Buf::new();
        ascii(s, &mut body)?;
        text_field(cx, &mut body, Case::Other, spec, out)
    };
    let fmt = |plain: usize, era_item: Option<usize>, out: &mut dyn Out<U>| composite(cx, cx.time.format(plain, era_item), Case::Other, spec, out, depth, None);
    let hour12 = || match i64::from(tm.hour).rem_euclid(12) {
        0 => 12,
        h => h,
    };
    let months = if alt { (lctime::ABALTMON, lctime::ALTMON) } else { (lctime::ABMON, lctime::MON) };
    let month = |base: usize, plain: usize| {
        let m = i64::from(tm.mon);
        cx.time.month(base, m).filter(|s| !s.is_empty()).or_else(|| cx.time.month(plain, m))
    };
    match c {
        b'a' => name(cx.time.abday(i64::from(tm.wday)), Case::Name, out)?,
        b'A' => name(cx.time.day(i64::from(tm.wday)), Case::Name, out)?,
        b'b' | b'h' => name(month(months.0, lctime::ABMON), Case::Name, out)?,
        b'B' => name(month(months.1, lctime::MON), Case::Name, out)?,
        b'c' => fmt(lctime::D_T_FMT, era_mod.then_some(lctime::ERA_D_T_FMT), out)?,
        b'x' => fmt(lctime::D_FMT, era_mod.then_some(lctime::ERA_D_FMT), out)?,
        b'X' => fmt(lctime::T_FMT, era_mod.then_some(lctime::ERA_T_FMT), out)?,
        b'r' => fmt(lctime::T_FMT_AMPM, None, out)?,
        b'D' => composite(cx, b"%m/%d/%y", Case::Other, spec, out, depth, None)?,
        b'F' => composite(cx, b"%Y-%m-%d", Case::Other, spec, out, depth, None)?,
        b'R' => composite(cx, b"%H:%M", Case::Other, spec, out, depth, None)?,
        b'T' => composite(cx, b"%H:%M:%S", Case::Other, spec, out, depth, None)?,
        b'C' => match era() {
            Some(e) => name(Some(e.name), Case::Other, out)?,
            None => num(year.div_euclid(100), 2, Pad::Zero, out)?,
        },
        b'y' => match era() {
            Some(e) => number(cx, i128::from(e.year_of(year)), 2, Pad::Zero, spec, false, out)?,
            None => num(year.rem_euclid(100), 2, Pad::Zero, out)?,
        },
        b'Y' => match era() {
            Some(e) if !e.format.is_empty() => composite(cx, e.format, Case::Other, Spec { pad: Pad::Default, ..spec }, out, depth, Some(spec.pad).filter(|p| *p != Pad::Default))?,
            _ => num(year, 1, Pad::Zero, out)?,
        },
        b'd' => num(i64::from(tm.mday), 2, Pad::Zero, out)?,
        b'e' => num(i64::from(tm.mday), 2, Pad::Space, out)?,
        b'H' => num(i64::from(tm.hour), 2, Pad::Zero, out)?,
        b'k' => num(i64::from(tm.hour), 2, Pad::Space, out)?,
        b'I' => num(hour12(), 2, Pad::Zero, out)?,
        b'l' => num(hour12(), 2, Pad::Space, out)?,
        b'j' => num(i64::from(tm.yday) + 1, 3, Pad::Zero, out)?,
        b'm' => num(i64::from(tm.mon) + 1, 2, Pad::Zero, out)?,
        b'M' => num(i64::from(tm.min), 2, Pad::Zero, out)?,
        b'S' => num(i64::from(tm.sec), 2, Pad::Zero, out)?,
        b'u' => num((i64::from(tm.wday) - 1).rem_euclid(7) + 1, 1, Pad::Zero, out)?,
        b'w' => num(i64::from(tm.wday), 1, Pad::Zero, out)?,
        b'U' => num((i64::from(tm.yday) + 7 - i64::from(tm.wday).rem_euclid(7)).div_euclid(7), 2, Pad::Zero, out)?,
        b'W' => num((i64::from(tm.yday) + 7 - (i64::from(tm.wday) + 6).rem_euclid(7)).div_euclid(7), 2, Pad::Zero, out)?,
        b'V' => num(iso_week(tm).1, 2, Pad::Zero, out)?,
        b'G' => num(iso_week(tm).0, 1, Pad::Zero, out)?,
        b'g' => num(iso_week(tm).0.rem_euclid(100), 2, Pad::Zero, out)?,
        b's' => number(cx, cx.env.seconds(tm), 1, Pad::Space, spec, false, out)?,
        b'p' | b'P' => {
            let s = cx.time.string(if tm.hour >= 12 { lctime::PM } else { lctime::AM });
            name(Some(s), if c == b'p' { Case::Lower } else { Case::AlwaysLower }, out)?;
        }
        b'z' => {
            if tm.isdst < 0 {
                lit(b"", out)?;
            } else {
                let a = tm.gmtoff.unsigned_abs();
                let (hh, mm) = (a / 3600, a / 60 % 60);
                let mut digits = Buf::new();
                let mut buf = [0u8; 24];
                let k = decimal(hh, &mut buf);
                if k < 2 {
                    ascii(b"0", &mut digits)?;
                }
                ascii(buf.get(..k).unwrap_or(&[]), &mut digits)?;
                ascii(&[b'0' + (mm / 10).to_le_bytes()[0], b'0' + (mm % 10).to_le_bytes()[0]], &mut digits)?;
                // The sign is always shown.
                let pad = if spec.pad == Pad::Default { Pad::Zero } else { spec.pad };
                let target = spec.width.unwrap_or(0);
                let n = target.saturating_sub(digits.len() + 1);
                let sign = [U::of(u32::from(if tm.gmtoff < 0 { b'-' } else { b'+' }))];
                match pad {
                    Pad::Zero | Pad::Default => out.field(&sign, n, U::of(u32::from(b'0')), &digits)?,
                    Pad::Space | Pad::None => {
                        let mut body: Buf<U> = InlineVec::new();
                        body.extend_from_slice(&sign)?;
                        body.extend_from_slice(&digits)?;
                        out.field(&[], n, U::of(u32::from(b' ')), &body)?;
                    }
                }
            }
        }
        b'Z' => {
            let mut body = Buf::new();
            match tm.zone.filter(|z| !z.is_empty()) {
                Some(z) => text_units(z, || cx.ctype_enc(), &mut body)?,
                None if tm.isdst < 0 => {}
                None => text_units(cx.env.zone_name(tm.isdst > 0), || cx.ctype_enc(), &mut body)?,
            }
            text_field(cx, &mut body, Case::Lower, spec, out)?;
        }
        b'n' => lit(b"\n", out)?,
        b't' => lit(b"\t", out)?,
        b'%' => lit(b"%", out)?,
        _ => return Ok(false),
    }
    Ok(true)
}

/// The decimal digits of `v` into `buf`: their count.
fn decimal(mut v: u64, buf: &mut [u8; 24]) -> usize {
    let mut tmp = [0u8; 24];
    let mut n = 0usize;
    loop {
        if let Some(d) = tmp.get_mut(n) {
            *d = b'0' + (v % 10).to_le_bytes()[0];
        }
        n += 1;
        v /= 10;
        if v == 0 || n >= 24 {
            break;
        }
    }
    for (k, slot) in buf.iter_mut().take(n).enumerate() {
        *slot = tmp.get(n - 1 - k).copied().unwrap_or(b'0');
    }
    n
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::as_conversions)]
mod tests {
    use super::*;

    struct Utc;

    impl Env for Utc {
        fn zone_name(&self, dst: bool) -> &[u8] {
            if dst { b"UDT" } else { b"UTC" }
        }

        fn seconds(&self, tm: &Tm<'_>) -> i128 {
            let f = civil::Fields {
                year: tm.full_year(),
                month: i64::from(tm.mon) + 1,
                day: i64::from(tm.mday),
                hour: i64::from(tm.hour),
                minute: i64::from(tm.min),
                second: i64::from(tm.sec),
            };
            f.seconds()
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn tm(y: i32, mon: i32, d: i32, h: i32, mi: i32, s: i32, wday: i32, yday: i32) -> Tm<'static> {
        Tm { sec: s, min: mi, hour: h, mday: d, mon: mon - 1, year: y - 1900, wday, yday, isdst: 0, gmtoff: 0, zone: Some(b"UTC") }
    }

    fn f(fmt: &str, t: &Tm<'_>) -> String {
        let mut buf = [0u8; 256];
        match strftime(&mut buf, fmt.as_bytes(), t, &Locale::C, &Utc).unwrap() {
            Written::Fits(n) => {
                assert_eq!(buf[n], 0, "terminated");
                String::from_utf8(buf[..n].to_vec()).unwrap()
            }
            Written::Truncated { .. } => panic!("{fmt} does not fit"),
        }
    }

    #[test]
    fn directives() {
        let t = tm(2024, 1, 1, 0, 0, 0, 1, 0);
        assert_eq!(f("%a %A %b %B %h", &t), "Mon Monday Jan January Jan");
        assert_eq!(f("%c|%x|%X|%r|%D|%F|%R|%T", &t), "Mon Jan  1 00:00:00 2024|01/01/24|00:00:00|12:00:00 AM|01/01/24|2024-01-01|00:00|00:00:00");
        assert_eq!(f("%C %d %e %H %I %j %k %l %m %M %S", &t), "20 01  1 00 12 001  0 12 01 00 00");
        assert_eq!(f("%u %w %U %W %V %G %g %y %Y", &t), "1 1 00 01 01 2024 24 24 2024");
        assert_eq!(f("%p %P %z %Z %s %% %n%t", &t), "AM am +0000 UTC 1704067200 % \n\t");
        // ISO weeks at the year's edges.
        assert_eq!(f("%G-%V %g", &tm(2021, 1, 3, 0, 0, 0, 0, 2)), "2020-53 20");
        assert_eq!(f("%G-%V", &tm(2024, 12, 30, 0, 0, 0, 1, 364)), "2025-01");
    }

    #[test]
    fn flags_and_widths() {
        let t = tm(2024, 1, 5, 7, 0, 0, 5, 4);
        assert_eq!(f("%_d|%-d|%0e|%-H|%_H|%3d|%10Y|%-10Y|%_10Y", &t), " 5|5|05|7| 7|005|0000002024|      2024|      2024");
        assert_eq!(f("%^a|%#a|%^B|%#Z|%#p|%^p|%^#Z|%^P", &t), "FRI|FRI|JANUARY|utc|am|AM|utc|AM");
        assert_eq!(f("%010a|%_10b|%5%|%02%|%5n", &t), "0000000Fri|       Jan|    %|0%|    \n");
        assert_eq!(f("%10D|%010R", &t), "  01/05/24|0000007:00");
        // Negative years: zeros after the sign, spaces before it, the width includes it.
        let b = tm(-1, 12, 31, 0, 0, 0, 5, 364);
        assert_eq!(f("%Y|%C|%y|%4C|%10Y|%_10Y|%G|%g", &b), "-1|-1|99|-001|-000000001|        -1|-1|99");
        // %C has two digits at least.
        assert_eq!(f("%C|%C|%-C", &tm(512, 1, 1, 0, 0, 0, 0, 0)), "05|05|5");
        assert_eq!(f("%Y|%2Y|%C", &tm(0, 3, 1, 0, 0, 0, 3, 60)), "0|00|00");
    }

    #[test]
    fn zones() {
        let mut t = tm(2024, 1, 1, 0, 0, 0, 1, 0);
        t.gmtoff = 19_800;
        assert_eq!(f("%z|%Ez|%Oz|%_z|%-z|%10z|%_10z", &t), "+0530|+0530|+0530|+0530|+0530|+000000530|     +0530");
        t.gmtoff = -12_600;
        assert_eq!(f("%z", &t), "-0330");
        t.gmtoff = -1;
        assert_eq!(f("%z", &t), "-0000", "the sign of the offset");
        t.isdst = -1;
        assert_eq!(f("[%z]", &t), "[]", "no zone determinable");
        t.zone = None;
        assert_eq!(f("[%Z]", &t), "[]");
        t.isdst = 1;
        assert_eq!(f("%Z", &t), "UDT", "the current zone's names");
        t.zone = Some(b"");
        t.isdst = 0;
        assert_eq!(f("%Z", &t), "UTC");
    }

    #[test]
    fn not_directives() {
        let t = tm(2024, 1, 1, 0, 0, 0, 1, 0);
        assert_eq!(f("%E|%Ea|%OY|%+6Y|%:z|%Oq|%E5Y", &t), "%E|%Ea|%OY|%+6Y|%:z|%Oq|%E5Y");
        assert_eq!(f("%5", &t), "   %5");
        assert_eq!(f("a%", &t), "a%");
        // E and O without eras or alternative digits are the plain conversions.
        assert_eq!(f("%Ec|%EC|%Ey|%EY|%Od|%Oe|%OH|%Ob|%OB", &t), "Mon Jan  1 00:00:00 2024|20|24|2024|01| 1|00|Jan|January");
    }

    #[test]
    fn sizes() {
        let t = tm(2024, 1, 1, 0, 0, 0, 1, 0);
        let mut buf = [b'x'; 16];
        assert_eq!(strftime(&mut buf[..11], b"%Y-%m-%d", &t, &Locale::C, &Utc).unwrap(), Written::Fits(10));
        let mut buf = [b'x'; 16];
        assert_eq!(strftime(&mut buf[..10], b"%Y-%m-%d", &t, &Locale::C, &Utc).unwrap(), Written::Truncated { needed: 10 });
        assert_eq!(&buf[..9], b"2024-01-x", "whole fields only, no terminator");
        let mut buf = [b'x'; 4];
        assert_eq!(strftime(&mut buf[..1], b"", &t, &Locale::C, &Utc).unwrap(), Written::Fits(0));
        assert_eq!(buf[0], 0);
        assert_eq!(strftime(&mut buf[..0], b"", &t, &Locale::C, &Utc).unwrap(), Written::Truncated { needed: 0 });
        let mut buf = [b'x'; 4];
        assert_eq!(strftime(&mut buf, b"%9999999999d", &t, &Locale::C, &Utc).unwrap(), Written::Truncated { needed: 9_999_999_999 });
        assert_eq!(buf, [b'x'; 4]);
        // Wide output.
        let mut w = [0i32; 32];
        let fmt: Vec<i32> = "%a %d|%Y".bytes().map(i32::from).collect();
        assert_eq!(strftime(&mut w, &fmt, &t, &Locale::C, &Utc).unwrap(), Written::Fits(11));
        assert_eq!(w[..11].iter().map(|&c| c as u8 as char).collect::<String>(), "Mon 01|2024");
    }

    /// The reference locales' `LC_TIME` (compiled with `localedef` into a directory of this test's
    /// own, so that no other test binary writes a file while it is read), after `C` and `C.UTF-8`.
    fn reference_locales() -> Vec<Locale> {
        use crate::locale::file::{Cat, Category};
        use crate::locale::Data;
        // Several tests of this binary call this at once; on a fresh target they would run
        // localedef into the same directory together (one fails, or another reads a half-written
        // LC_TIME), so the compile-and-read is one at a time.
        static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _one = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-locales-format");
        std::fs::create_dir_all(&dir).unwrap();
        let mut all = vec![Locale::C, Locale::c_utf8(b"C.UTF-8")];
        for name in ["en_US.UTF-8", "de_DE.UTF-8", "ja_JP.UTF-8", "th_TH.UTF-8", "fa_IR.UTF-8"] {
            let path = dir.join(name);
            if !path.join("LC_TIME").exists() {
                let (lang, charset) = name.split_once('.').unwrap();
                let st = std::process::Command::new("localedef").args(["-i", lang, "-f", charset]).arg(&path).status();
                // localedef reports warnings with status 1 and still writes the locale.
                assert!(st.is_ok_and(|s| s.code().is_some_and(|c| c <= 1)), "localedef {name}");
            }
            let bytes: &'static [u8] = Box::leak(std::fs::read(path.join("LC_TIME")).unwrap().into_boxed_slice());
            let cat = Category::parse(bytes).expect("LC_TIME parses");
            all.push(Locale::C.with(Cat::Time, name.as_bytes(), Data::File(cat)).expect("LC_TIME applies"));
        }
        all
    }

    /// Times for the comparison: years around every boundary of the direct path (negative,
    /// 0, 9999, 10000, the extremes of `tm_year`) and each field at and beyond its range.
    fn odd_times() -> Vec<Tm<'static>> {
        let base = Tm { sec: 9, min: 5, hour: 13, mday: 4, mon: 6, year: 124, wday: 4, yday: 185, isdst: 0, gmtoff: 7200, zone: Some(b"CEST") };
        let mut v = Vec::new();
        for year in [i32::MIN, -3900, -1901, -1900, -1890, -1801, -1800, -901, -900, -1, 0, 99, 124, 8099, 8100, i32::MAX - 1900, i32::MAX] {
            v.push(Tm { year, ..base });
        }
        let big = [i32::MIN, -1, i32::MAX];
        for x in [0, 1, 9, 10, 31, 40, 100, 9999, 10_000].into_iter().chain(big) {
            v.push(Tm { mday: x, ..base });
        }
        for x in [0, 11, 12, 13, 9998, 9999].into_iter().chain(big) {
            v.push(Tm { mon: x, ..base });
        }
        for x in [0, 1, 11, 12, 13, 23, 24, 99, 100, 150, 9999, 10_000].into_iter().chain(big) {
            v.push(Tm { hour: x, ..base });
        }
        for x in [0, 59, 60, 99, 100, 9999, 10_000].into_iter().chain(big) {
            v.push(Tm { min: x, sec: x, ..base });
        }
        for x in [0, 6, 7, -7].into_iter().chain(big) {
            v.push(Tm { wday: x, ..base });
        }
        for x in [0, 364, 365, 366, 9998, 9999, 10_000].into_iter().chain(big) {
            v.push(Tm { yday: x, ..base });
        }
        for x in [-1, 1, i32::MIN, i32::MAX] {
            v.push(Tm { isdst: x, ..base });
            v.push(Tm { isdst: x, zone: None, ..base });
            v.push(Tm { isdst: x, zone: Some(b""), ..base });
        }
        for x in [0, 59, -59, 19_800, -12_600, 359_999, 360_000, -360_000, i64::MIN, i64::MAX] {
            v.push(Tm { gmtoff: x, ..base });
        }
        // Everything at once, at both ends.
        v.push(Tm { sec: i32::MIN, min: i32::MIN, hour: i32::MIN, mday: i32::MIN, mon: i32::MIN, year: i32::MIN, wday: i32::MIN, yday: i32::MIN, isdst: i32::MIN, gmtoff: i64::MIN, zone: None });
        v.push(Tm { sec: i32::MAX, min: i32::MAX, hour: i32::MAX, mday: i32::MAX, mon: i32::MAX, year: i32::MAX, wday: i32::MAX, yday: i32::MAX, isdst: i32::MAX, gmtoff: i64::MAX, zone: None });
        v.push(Tm { sec: 0, min: 0, hour: 0, mday: 1, mon: 0, year: -1900, wday: 6, yday: 0, isdst: 0, gmtoff: 0, zone: Some(b"UTC") });
        v
    }

    /// Both paths give the same result and the same buffer contents: at every buffer length from 0
    /// to the length the text needs plus one with `sweep`, else at 0, that length, one more and 512.
    fn same_both_ways<U: Unit>(fmt: &[U], t: &Tm<'_>, loc: &Locale, sentinel: U, sweep: bool, what: &dyn Fn() -> String) -> usize {
        let mut big = vec![sentinel; 512];
        let needed = match format(&mut big, fmt, t, loc, &Utc, false).unwrap() {
            Written::Fits(n) | Written::Truncated { needed: n } => n,
        };
        let mut calls = 0;
        let lengths: Vec<usize> = if sweep { (0..=needed.saturating_add(1)).collect() } else { vec![0, needed, needed.saturating_add(1), 512] };
        for len in lengths.into_iter().filter(|&l| l <= 512) {
            let (mut a, mut b) = (vec![sentinel; len + 2], vec![sentinel; len + 2]);
            let general = format(&mut a[..len], fmt, t, loc, &Utc, false).unwrap();
            let direct = format(&mut b[..len], fmt, t, loc, &Utc, true).unwrap();
            assert_eq!(direct, general, "result, length {len}: {}", what());
            assert_eq!(b, a, "buffer, length {len}: {}", what());
            calls += 1;
        }
        calls
    }

    /// `Unit::copy_into` stores exactly what the unit-by-unit copy stores, for every length and
    /// both unit types, and touches nothing past the shorter slice (LV-4).
    #[test]
    fn copy_into_is_the_unit_copy() {
        for n in 0..70usize {
            for extra in [0usize, 1, 9] {
                let src8: Vec<u8> = (0..n).map(|k| u8::try_from(k + 1).unwrap()).collect();
                let src32: Vec<i32> = (0..n).map(|k| i32::try_from(k + 1).unwrap() * 1000).collect();
                // The destination is longer than the source by `extra`, and shorter by it.
                for (dn, sn) in [(n + extra, n), (n, n + extra)] {
                    let (s8, s32) = (src8.iter().copied().cycle().take(sn).collect::<Vec<u8>>(), src32.iter().copied().cycle().take(sn).collect::<Vec<i32>>());
                    let (mut d8, mut w8) = (vec![0xEEu8; dn], vec![0xEEu8; dn]);
                    u8::copy_into(&mut d8, &s8);
                    for (d, &s) in w8.iter_mut().zip(&s8) {
                        *d = s;
                    }
                    assert_eq!(d8, w8, "bytes: dst {dn}, src {sn}");
                    let (mut d32, mut w32) = (vec![-7i32; dn], vec![-7i32; dn]);
                    i32::copy_into(&mut d32, &s32);
                    for (d, &s) in w32.iter_mut().zip(&s32) {
                        *d = s;
                    }
                    assert_eq!(d32, w32, "wide: dst {dn}, src {sn}");
                }
            }
        }
    }

    #[test]
    fn direct_path_is_the_general_path() {
        // Every directive alone (`%s` aside: `mktime` of the extremes is not this engine's), the
        // benchmarks' formats, the locale composites, and flags, widths and modifiers mixed in
        // (those take the general path, around the direct one).
        let mut formats: Vec<String> = "aAbBcCdDeFgGhHIjklmMnpPrRStTuUVwWxXyYzZ%".chars().map(|c| format!("%{c}")).collect();
        formats.extend(
            [
                "%Y-%m-%d %H:%M",
                "%b %e %H:%M",
                "%a %b %e %H:%M:%S %Z %Y",
                "%Y-%m-%dT%H:%M:%S%z",
                "%d/%b/%Y:%H:%M:%S %z",
                "%x %X %r|%c",
                "%T%R%D%F%%",
                "a%%b%nc%td",
                "%I %l %p %P %j %u %w %U %W %C %y %G %g %V",
                "%EY|%Ey|%EC|%Ec|%Ex|%EX|%Ez",
                "%Od|%Oe|%OH|%Om|%Oy|%OB|%Ob|%OS|%Oz",
                "%_d|%-m|%05Y|%^a|%#b|%10T|%-e|%_H|%^p|%#Z|%3z|%_8F|%-y|%0e|%^B|%#A|%1Y",
                "%",
                "x%",
                "%5",
                "%Q|%Ea|%OY|%+Y|%E",
                "plain text",
                "",
                "%%%Y%%%",
            ]
            .map(String::from),
        );
        let (mut calls, mut cases) = (0usize, 0usize);
        for loc in &reference_locales() {
            let times = odd_times();
            for (k, t) in times.iter().enumerate() {
                // Every length for the years (where the fields' widths change) and the extremes; the
                // edges for the rest (a debug build runs about 2 us a call).
                let sweep = k < 14 || k + 3 >= times.len();
                for fmt in &formats {
                    let what = || format!("{fmt:?} in {:?} at {t:?}", core::str::from_utf8(loc.name(crate::locale::file::Cat::Time)));
                    calls += same_both_ways(fmt.as_bytes(), t, loc, 0x7f, sweep, &what);
                    let wide: Vec<i32> = fmt.chars().map(|c| c as i32).collect();
                    calls += same_both_ways(&wide, t, loc, 0x7f7f, sweep, &what);
                    cases += 2;
                }
            }
        }
        // The cases are all there (7 locales; a lost list would pass vacuously).
        assert!(cases > 7 * 100 * 50 && calls > cases * 4, "{cases} cases, {calls} calls");
    }

    /// Formats for the fast lane: every letter alone, with each flag, a width and each modifier,
    /// mixed with text, the benchmarks' formats and the locale composites.
    fn lane_formats() -> Vec<String> {
        let mut v: Vec<String> = Vec::new();
        for c in ('a'..='z').chain('A'..='Z').chain(['%', '5', '-', '_', '0', '^', '#', '+', ' ']) {
            v.push(format!("%{c}"));
            v.push(format!("x%{c}y"));
            for flag in ["-", "_", "0", "^", "#", "5", "10", "-5", "E", "O", "_E", "^O"] {
                v.push(format!("%{flag}{c}"));
            }
        }
        v.extend(
            [
                "%A %B",
                "%Y-%m-%dT%H:%M:%S%z",
                "%c",
                "%x %X",
                "%x|%X|%r|%c|%D|%F|%R|%T",
                "%a %b %e %H:%M:%S %Z %Y",
                "%d/%b/%Y:%H:%M:%S %z",
                "%I:%M %p, %A %d %B %Y (day %j, week %U/%W, %u/%w) %C%y",
                "%H%M%S%n%t%%%k%l",
                "plain text",
                "",
                "%",
                "a%",
                "%%%%",
                "%Ec %Ex %EX %Er",
                "%Y %-m %_d %H",
                "%Y-%m-%d %H:%M",
                "%b %e %H:%M|%e%k%S|%m/%d/%Y",
                "\u{e4}\u{20ac}%Y\u{1f600}",
            ]
            .map(String::from),
        );
        v
    }

    /// A format with a flag, width or modifier right after a `%`, or a trailing `%`: never the
    /// fast lane's.
    fn is_decorated(fmt: &str) -> bool {
        let b = fmt.as_bytes();
        let mut i = 0;
        while i < b.len() {
            if b[i] != b'%' {
                i += 1;
                continue;
            }
            match b.get(i + 1) {
                None => return true,
                Some(n) if b"-_0123456789^#EO".contains(n) => return true,
                Some(_) => i += 2,
            }
        }
        false
    }

    #[test]
    fn fast_lane_is_the_general_path() {
        use super::super::fast;
        let (mut taken, mut bailed, mut calls) = (0usize, 0usize, 0usize);
        let formats = lane_formats();
        for loc in &reference_locales() {
            let view = crate::locale::view::LocaleView::new(*loc, 1, &|_, _| None).unwrap();
            let times = odd_times();
            for (k, t) in times.iter().enumerate() {
                let sweep = k < 14 || k + 3 >= times.len();
                for fmt in &formats {
                    let what = || format!("{fmt:?} in {:?} at {t:?}", core::str::from_utf8(loc.name(crate::locale::file::Cat::Time)));
                    let f = fmt.as_bytes();
                    let mut big = vec![0x7fu8; 512];
                    let needed = match format(&mut big, f, t, loc, &Utc, true).unwrap() {
                        Written::Fits(n) | Written::Truncated { needed: n } => n,
                    };
                    let lengths: Vec<usize> = if sweep { (0..=needed + 1).collect() } else { vec![0, needed, needed + 1, 512] };
                    for len in lengths.into_iter().filter(|&l| l <= 512) {
                        let (mut a, mut b) = (vec![0x7fu8; len + 2], vec![0x7fu8; len + 2]);
                        let general = format(&mut a[..len], f, t, loc, &Utc, true).unwrap();
                        match fast::strftime(&mut b[..len], f, t, &view) {
                            Some(n) => {
                                assert_eq!(general, Written::Fits(n), "length {len}: {}", what());
                                assert_eq!(b, a, "buffer, length {len}: {}", what());
                                assert!(!is_decorated(fmt), "took a decorated format, length {len}: {}", what());
                                taken += 1;
                            }
                            None => {
                                // The general engine runs on what the lane left.
                                assert_eq!(format(&mut b[..len], f, t, loc, &Utc, true).unwrap(), general, "after a bail, length {len}: {}", what());
                                assert_eq!(b, a, "buffer after a bail, length {len}: {}", what());
                                bailed += 1;
                            }
                        }
                        calls += 1;
                    }
                }
            }
        }
        // The lane took a large share of the plain cases (not vacuous) and bailed on others.
        assert!(taken > 50_000 && bailed > 50_000 && calls == taken + bailed, "{taken} taken, {bailed} bailed");
    }

    #[test]
    fn fast_lane_takes_the_plain_formats() {
        use super::super::fast;
        let locs = reference_locales();
        let vw = crate::locale::view::LocaleView::new(locs[0], 1, &|_, _| None).unwrap();
        let base = Tm { sec: 9, min: 5, hour: 13, mday: 4, mon: 6, year: 124, wday: 4, yday: 185, isdst: 0, gmtoff: 7200, zone: Some(b"CEST") };
        let mut out = [0u8; 200];
        for (fmt, want) in [("%A %B", "Thursday July"), ("%Y-%m-%dT%H:%M:%S%z", "2024-07-04T13:05:09+0200"), ("%F %T %Z|%e|%j", "2024-07-04 13:05:09 CEST| 4|186"), ("a%%b", "a%b"), ("", "")] {
            let n = fast::strftime(&mut out, fmt.as_bytes(), &base, &vw).unwrap_or_else(|| panic!("{fmt:?} bailed"));
            assert_eq!(core::str::from_utf8(&out[..n]).unwrap(), want);
            assert_eq!(out[n], 0);
        }
        for fmt in ["%Ey", "%Od", "%-d", "%_H", "%5Y", "%^a", "%#b", "%", "x%", "%s", "%P", "%V", "%G", "%Q"] {
            assert_eq!(fast::strftime(&mut out, fmt.as_bytes(), &base, &vw), None, "{fmt:?}");
        }
        // A zone name from the process (NULL or empty `tm_zone`) is the engine's.
        assert_eq!(fast::strftime(&mut out, b"%Z", &Tm { zone: None, ..base }, &vw), None);
        assert_eq!(fast::strftime(&mut out, b"%Z", &Tm { zone: Some(b""), ..base }, &vw), None);
        // Out of range numbers, and no room for the terminator.
        assert_eq!(fast::strftime(&mut out, b"%d", &Tm { mday: 10_000, ..base }, &vw), None);
        assert_eq!(fast::strftime(&mut out[..4], b"%A", &base, &vw), None);
        assert_eq!(fast::strftime(&mut out[..9], b"%A", &base, &vw), Some(8));
        assert_eq!(fast::strftime(&mut out[..8], b"%A", &base, &vw), None);
}

    /// The view's ready names equal the per-call reads of every `LC_TIME` name item in every
    /// reference locale (built-in C and C.UTF-8, and four compiled ones), and the accessors
    /// equal `lctime::Time` over in-range and out-of-range indices.
    #[test]
    fn time_names_equal_the_item_reads() {
        use crate::locale::file::Cat;
        use crate::locale::view::{LocaleView, TimeNames};
        use crate::locale::Item;
        let mut nonempty = 0;
        for loc in &reference_locales() {
            let v = LocaleView::new(*loc, 3, &|_, _| None).unwrap();
            let what = format!("{:?}", core::str::from_utf8(loc.name(Cat::Time)));
            let names = v.names();
            for i in 0..TimeNames::N {
                let want: &[u8] = match loc.item_parsed(Cat::Time, i) {
                    Item::Bytes(b) => &b[..b.iter().position(|&x| x == 0).unwrap_or(b.len())],
                    Item::Word(_) => &[],
                };
                assert_eq!(names.item(i), Some(want), "{what}: item {i}");
                nonempty += usize::from(!want.is_empty());
            }
            assert_eq!(names.item(TimeNames::N), None);
            let t = super::super::lctime::Time::of(loc);
            for w in -2..10 {
                assert_eq!(names.abday(w), t.abday(w), "{what}: abday {w}");
                assert_eq!(names.day(w), t.day(w), "{what}: day {w}");
                assert_eq!(names.abmon(w), t.month(super::super::lctime::ABMON, w), "{what}: abmon {w}");
                assert_eq!(names.mon(w), t.month(super::super::lctime::MON, w), "{what}: mon {w}");
            }
            for w in [i64::MIN, i64::MAX, -1, 12] {
                assert_eq!((names.abmon(w), names.mon(w)), (None, None), "{what}: month {w}");
            }
            for w in [i64::MIN, i64::MAX, -1, 7, 12] {
                assert_eq!((names.abday(w), names.day(w)), (None, None), "{what}: day {w}");
            }
            assert_eq!(names.ampm(false), t.string(super::super::lctime::AM), "{what}: am");
            assert_eq!(names.ampm(true), t.string(super::super::lctime::PM), "{what}: pm");
        }
        // Not vacuous: the names occur (C has 38 + the AM/PM strings; compiled ones differ).
        assert!(nonempty > 7 * 38, "{nonempty}");
    }
}
