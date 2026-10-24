//! General character-set conversion (spec §6.4; D-6.8, D-6.9): codecs, name parsing and the
//! conversion loop of `iconv`.
//!
//! A conversion decodes one character of the source to a code point and encodes it in the
//! target (pivot UCS-4). Every step either completes or leaves input and output untouched, so a
//! conversion stops cleanly at any point (`E2BIG`, `EINVAL`, `EILSEQ`) and resumes on the next
//! call. Incomplete input is never buffered: it stays in the caller's input. The descriptor
//! keeps only the byte order a BOM selected and whether the target's BOM was written.
//!
//! The built-in Unicode codecs follow the reference library, as observed (D-6.9): `UTF-16` and
//! `UTF-32` read an optional BOM and otherwise assume little-endian, and write a little-endian
//! BOM once; the forms that name a byte order never read or write one. `UCS-2` is
//! little-endian, `UCS-4` big-endian, `WCHAR_T` little-endian (the host); the `UCS-4` forms carry
//! values up to `0x7FFFFFFF`, which `UTF-8` writes in the original 5- and 6-byte forms.

#![allow(clippy::arithmetic_side_effects)] // byte counts below the slice lengths, code points below 2^31

use crate::charmap::Charmap;
use crate::error::Errno;
mod fast;

use crate::locale::{Ctype, Locale};
use crate::text::{self, CharmapRef};

/// A byte order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Order {
    /// Little-endian.
    Le,
    /// Big-endian.
    Be,
}

/// A codec of the pivot conversion.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Codec {
    /// UTF-8; decodes RFC 3629, encodes code points up to `0x7FFFFFFF`.
    Utf8,
    /// UTF-16; `None` reads and writes a BOM.
    Utf16(Option<Order>),
    /// UTF-32; `None` reads and writes a BOM.
    Utf32(Option<Order>),
    /// UCS-2: the BMP without surrogates.
    Ucs2(Order),
    /// UCS-4: 31-bit values without surrogates.
    Ucs4(Order),
    /// ASCII.
    Ascii,
    /// ISO-8859-1.
    Latin1,
    /// A charset read from the installed charmaps.
    Charmap(CharmapRef),
}

/// The built-in names (compared without regard to case). The reference library's aliases of
/// ASCII and ISO-8859-1 are the ones their charmaps list.
const NAMES: [(&[u8], Codec); 52] = [
    (b"UTF-8", Codec::Utf8),
    (b"UTF8", Codec::Utf8),
    (b"ISO-10646/UTF8", Codec::Utf8),
    (b"ISO-10646/UTF-8", Codec::Utf8),
    (b"ISO-IR-193", Codec::Utf8),
    (b"UTF-16", Codec::Utf16(None)),
    (b"UTF16", Codec::Utf16(None)),
    (b"UTF-16LE", Codec::Utf16(Some(Order::Le))),
    (b"UTF16LE", Codec::Utf16(Some(Order::Le))),
    (b"UTF-16BE", Codec::Utf16(Some(Order::Be))),
    (b"UTF16BE", Codec::Utf16(Some(Order::Be))),
    (b"UTF-32", Codec::Utf32(None)),
    (b"UTF32", Codec::Utf32(None)),
    (b"UTF-32LE", Codec::Utf32(Some(Order::Le))),
    (b"UTF32LE", Codec::Utf32(Some(Order::Le))),
    (b"UTF-32BE", Codec::Utf32(Some(Order::Be))),
    (b"UTF32BE", Codec::Utf32(Some(Order::Be))),
    (b"UCS-2", Codec::Ucs2(Order::Le)),
    (b"UCS2", Codec::Ucs2(Order::Le)),
    (b"ISO-10646/UCS2", Codec::Ucs2(Order::Le)),
    (b"CSUNICODE", Codec::Ucs2(Order::Le)),
    (b"UCS-2LE", Codec::Ucs2(Order::Le)),
    (b"UNICODELITTLE", Codec::Ucs2(Order::Le)),
    (b"UCS-2BE", Codec::Ucs2(Order::Be)),
    (b"UNICODEBIG", Codec::Ucs2(Order::Be)),
    (b"UCS-4", Codec::Ucs4(Order::Be)),
    (b"UCS4", Codec::Ucs4(Order::Be)),
    (b"ISO-10646", Codec::Ucs4(Order::Be)),
    (b"ISO-10646/UCS4", Codec::Ucs4(Order::Be)),
    (b"10646-1:1993", Codec::Ucs4(Order::Be)),
    (b"10646-1:1993/UCS4", Codec::Ucs4(Order::Be)),
    (b"CSUCS4", Codec::Ucs4(Order::Be)),
    (b"UCS-4BE", Codec::Ucs4(Order::Be)),
    (b"UCS-4LE", Codec::Ucs4(Order::Le)),
    (b"WCHAR_T", Codec::Ucs4(Order::Le)),
    (b"ANSI_X3.4-1968", Codec::Ascii),
    (b"ANSI_X3.4-1986", Codec::Ascii),
    (b"ISO-IR-6", Codec::Ascii),
    (b"ISO_646.IRV:1991", Codec::Ascii),
    (b"ASCII", Codec::Ascii),
    (b"ISO646-US", Codec::Ascii),
    (b"US-ASCII", Codec::Ascii),
    (b"US", Codec::Ascii),
    (b"IBM367", Codec::Ascii),
    (b"CP367", Codec::Ascii),
    (b"CSASCII", Codec::Ascii),
    (b"ISO-8859-1", Codec::Latin1),
    (b"ISO_8859-1", Codec::Latin1),
    (b"ISO8859-1", Codec::Latin1),
    (b"LATIN1", Codec::Latin1),
    (b"L1", Codec::Latin1),
    (b"CP819", Codec::Latin1),
];

/// More names of ISO-8859-1 (its charmap's aliases).
const LATIN1_MORE: [&[u8]; 6] = [b"ISO-IR-100", b"ISO_8859-1:1987", b"IBM819", b"CSISOLATIN1", b"8859_1", b"OSF00010001"];

/// The built-in codec `name` denotes (without suffixes), if any.
pub fn builtin(name: &[u8]) -> Option<Codec> {
    if let Some((_, c)) = NAMES.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)) {
        return Some(*c);
    }
    LATIN1_MORE.iter().any(|n| n.eq_ignore_ascii_case(name)).then_some(Codec::Latin1)
}

/// The codec of `loc`'s `LC_CTYPE` codeset (the empty name of `iconv_open`).
pub fn of_locale(loc: &Locale) -> Codec {
    match text::Encoding::of(loc) {
        text::Encoding::Ascii => Codec::Ascii,
        text::Encoding::Utf8 => Codec::Utf8,
        text::Encoding::Latin1 => Codec::Latin1,
        text::Encoding::Charmap(c) => Codec::Charmap(c),
    }
}

/// A charset name and its suffixes: the name ends at the first `/` that begins `//`; the
/// suffixes after it are separated by `/` or `,` and matched without regard to case. Unknown
/// suffixes are ignored, as by the reference library.
pub struct Spec<'a> {
    /// The charset name.
    pub name: &'a [u8],
    /// `//TRANSLIT`.
    pub translit: bool,
    /// `//IGNORE`.
    pub ignore: bool,
}

/// Split `full` into its name and suffixes.
pub fn parse_name(full: &[u8]) -> Spec<'_> {
    let cut = full.windows(2).position(|w| w == b"//").unwrap_or(full.len());
    let (name, rest) = full.split_at(cut);
    let mut s = Spec { name, translit: false, ignore: false };
    for word in rest.split(|&b| b == b'/' || b == b',') {
        if word.eq_ignore_ascii_case(b"TRANSLIT") {
            s.translit = true;
        } else if word.eq_ignore_ascii_case(b"IGNORE") {
            s.ignore = true;
        }
    }
    s
}

/// One step of decoding.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Dec {
    /// A character and the bytes it took.
    Char(u32, usize),
    /// A byte-order mark the codec consumes (no character).
    Bom(usize, Order),
    /// The input ends inside a character.
    Incomplete,
    /// An invalid sequence; `//IGNORE` skips this many bytes.
    Invalid(usize),
}

fn unit(input: &[u8], n: usize, order: Order) -> Option<u32> {
    match (n, order) {
        (2, Order::Le) => input.first_chunk::<2>().map(|b| u32::from(u16::from_le_bytes(*b))),
        (2, Order::Be) => input.first_chunk::<2>().map(|b| u32::from(u16::from_be_bytes(*b))),
        (_, Order::Le) => input.first_chunk::<4>().map(|b| u32::from_le_bytes(*b)),
        (_, Order::Be) => input.first_chunk::<4>().map(|b| u32::from_be_bytes(*b)),
    }
}

/// One UTF-8 character (RFC 3629: no overlongs, no surrogates, nothing above U+10FFFF) at the
/// start of `input`, whose first byte is not ASCII: the character, `Incomplete` when the input
/// ends inside a valid prefix, `Invalid(1)` otherwise (as `text::decode` in UTF-8).
fn decode_utf8(input: &[u8]) -> Dec {
    let Some(&b0) = input.first() else { return Dec::Incomplete };
    // The sequence length and the range of the second byte (the others are 0x80..=0xBF).
    let (n, lo, hi) = match b0 {
        0xC2..=0xDF => (2, 0x80, 0xBF),
        0xE0 => (3, 0xA0, 0xBF),
        0xE1..=0xEC | 0xEE..=0xEF => (3, 0x80, 0xBF),
        0xED => (3, 0x80, 0x9F),
        0xF0 => (4, 0x90, 0xBF),
        0xF1..=0xF3 => (4, 0x80, 0xBF),
        0xF4 => (4, 0x80, 0x8F),
        _ => return Dec::Invalid(1),
    };
    let mut c = u32::from(b0) & (0x7F >> n);
    for k in 1..n {
        let Some(&b) = input.get(k) else { return Dec::Incomplete };
        let (l, h) = if k == 1 { (lo, hi) } else { (0x80, 0xBF) };
        if !(l..=h).contains(&b) {
            return Dec::Invalid(1);
        }
        c = (c << 6) | u32::from(b & 0x3F);
    }
    Dec::Char(c, n)
}

fn is_surrogate(c: u32) -> bool {
    (0xD800..0xE000).contains(&c)
}

impl Codec {
    /// Decode one character. `order` is the order a BOM selected (`None` before the first unit).
    fn decode(self, input: &[u8], order: Option<Order>) -> Dec {
        let need = |n: usize| input.len() < n;
        // ASCII is itself in the byte codecs that contain it.
        if let (Some(&b), Codec::Utf8 | Codec::Ascii | Codec::Latin1) = (input.first(), self) {
            if b < 0x80 {
                return Dec::Char(u32::from(b), 1);
            }
        }
        match self {
            Codec::Utf8 => decode_utf8(input),
            Codec::Ascii => match input.first() {
                None => Dec::Incomplete,
                Some(&b) if b < 0x80 => Dec::Char(u32::from(b), 1),
                Some(_) => Dec::Invalid(1),
            },
            Codec::Latin1 => input.first().map_or(Dec::Incomplete, |&b| Dec::Char(u32::from(b), 1)),
            Codec::Charmap(cm) => match cm.0.decode(input) {
                crate::charmap::Step::Char(c, n) => Dec::Char(c, n),
                crate::charmap::Step::Incomplete => Dec::Incomplete,
                crate::charmap::Step::Invalid => Dec::Invalid(1),
            },
            Codec::Ucs2(o) => match unit(input, 2, o) {
                None => Dec::Incomplete,
                Some(c) if is_surrogate(c) => Dec::Invalid(2),
                Some(c) => Dec::Char(c, 2),
            },
            Codec::Utf16(fixed) => {
                let o = fixed.or(order).unwrap_or(Order::Le);
                let Some(c) = unit(input, 2, o) else { return Dec::Incomplete };
                if fixed.is_none() && order.is_none() && (c == 0xFEFF || c == 0xFFFE) {
                    return Dec::Bom(2, if c == 0xFEFF { o } else { Order::Be });
                }
                match c {
                    0xD800..=0xDBFF => {
                        if need(4) {
                            return Dec::Incomplete;
                        }
                        match unit(input.get(2..).unwrap_or(&[]), 2, o) {
                            Some(lo @ 0xDC00..=0xDFFF) => Dec::Char(0x10000 + ((c - 0xD800) << 10) + (lo - 0xDC00), 4),
                            _ => Dec::Invalid(2),
                        }
                    }
                    0xDC00..=0xDFFF => Dec::Invalid(2),
                    _ => Dec::Char(c, 2),
                }
            }
            Codec::Utf32(fixed) => {
                let o = fixed.or(order).unwrap_or(Order::Le);
                let Some(c) = unit(input, 4, o) else { return Dec::Incomplete };
                if fixed.is_none() && order.is_none() && (c == 0xFEFF || c == 0xFFFE_0000) {
                    return Dec::Bom(4, if c == 0xFEFF { o } else { Order::Be });
                }
                if c > 0x10_FFFF || is_surrogate(c) { Dec::Invalid(4) } else { Dec::Char(c, 4) }
            }
            Codec::Ucs4(o) => match unit(input, 4, o) {
                None => Dec::Incomplete,
                Some(c) if c > 0x7FFF_FFFF || is_surrogate(c) => Dec::Invalid(4),
                Some(c) => Dec::Char(c, 4),
            },
        }
    }

    /// Encode `c` into `out` (at most 8 bytes needed); `None` if the codec cannot represent it.
    fn encode(self, c: u32, out: &mut [u8; 8]) -> Option<usize> {
        let put = |out: &mut [u8; 8], v: u32, n: usize, o: Order| {
            for (i, slot) in out.iter_mut().take(n).enumerate() {
                let shift = match o {
                    Order::Le => 8 * i,
                    Order::Be => 8 * (n - 1 - i),
                };
                *slot = u8::try_from((v >> shift) & 0xFF).unwrap_or(0);
            }
            n
        };
        if is_surrogate(c) {
            return None;
        }
        match self {
            Codec::Utf8 => encode_utf8_31(c, out),
            Codec::Ascii => (c < 0x80).then(|| put(out, c, 1, Order::Le)),
            Codec::Latin1 => (c < 0x100).then(|| put(out, c, 1, Order::Le)),
            Codec::Charmap(cm) => cm.0.encode(c, out),
            Codec::Ucs2(o) => (c < 0x1_0000).then(|| put(out, c, 2, o)),
            Codec::Utf16(fixed) => {
                let o = fixed.unwrap_or(Order::Le);
                match c {
                    0..=0xFFFF => Some(put(out, c, 2, o)),
                    0x1_0000..=0x10_FFFF => {
                        let v = c - 0x1_0000;
                        let (hi, lo) = (0xD800 + (v >> 10), 0xDC00 + (v & 0x3FF));
                        let mut tmp = [0u8; 8];
                        put(&mut tmp, lo, 2, o);
                        put(out, hi, 2, o);
                        out.get_mut(2..4)?.copy_from_slice(tmp.get(..2)?);
                        Some(4)
                    }
                    _ => None,
                }
            }
            Codec::Utf32(fixed) => (c <= 0x10_FFFF).then(|| put(out, c, 4, fixed.unwrap_or(Order::Le))),
            Codec::Ucs4(o) => (c <= 0x7FFF_FFFF).then(|| put(out, c, 4, o)),
        }
    }

    /// Whether a byte below 0x80 decodes to itself, one byte per character.
    fn ascii_source(self) -> bool {
        match self {
            Codec::Utf8 | Codec::Ascii | Codec::Latin1 => true,
            Codec::Charmap(c) => c.0.ascii(),
            _ => false,
        }
    }

    /// For ASCII code points: the unit this codec writes them as (its size and order).
    fn ascii_unit(self) -> Option<(usize, Order)> {
        match self {
            Codec::Utf8 | Codec::Ascii | Codec::Latin1 => Some((1, Order::Le)),
            Codec::Charmap(c) => c.0.ascii().then_some((1, Order::Le)),
            Codec::Utf16(o) => Some((2, o.unwrap_or(Order::Le))),
            Codec::Ucs2(o) => Some((2, o)),
            Codec::Utf32(o) => Some((4, o.unwrap_or(Order::Le))),
            Codec::Ucs4(o) => Some((4, o)),
        }
    }

    /// For a UTF-16 or UTF-32 source (also UCS-2, UCS-4): its unit size and byte order, once
    /// the order is known (`order`: what a BOM or the first unit selected).
    fn unit_form(self, order: Option<Order>) -> Option<(usize, Order)> {
        match self {
            Codec::Utf16(o) => Some((2, o.or(order)?)),
            Codec::Ucs2(o) => Some((2, o)),
            Codec::Utf32(o) => Some((4, o.or(order)?)),
            Codec::Ucs4(o) => Some((4, o)),
            _ => None,
        }
    }

    /// The BOM this codec writes first, if any.
    fn bom(self) -> Option<&'static [u8]> {
        match self {
            Codec::Utf16(None) => Some(&[0xFF, 0xFE]),
            Codec::Utf32(None) => Some(&[0xFF, 0xFE, 0, 0]),
            _ => None,
        }
    }
}

/// UTF-8 of a 31-bit value (the 5- and 6-byte forms above U+10FFFF, D-6.9).
fn encode_utf8_31(c: u32, out: &mut [u8; 8]) -> Option<usize> {
    // The forms of RFC 3629 directly; the 5- and 6-byte forms below.
    let byte = |v: u32| u8::try_from(v & 0xFF).unwrap_or(0);
    let tail = |shift: u32| byte(0x80 | ((c >> shift) & 0x3F));
    let direct: Option<(usize, [u8; 4])> = match c {
        0..=0x7F => Some((1, [byte(c), 0, 0, 0])),
        0x80..=0x7FF => Some((2, [byte(0xC0 | (c >> 6)), tail(0), 0, 0])),
        0x800..=0xFFFF => Some((3, [byte(0xE0 | (c >> 12)), tail(6), tail(0), 0])),
        0x1_0000..=0x1F_FFFF => Some((4, [byte(0xF0 | (c >> 18)), tail(12), tail(6), tail(0)])),
        _ => None,
    };
    if let Some((n, b)) = direct {
        out.get_mut(..4)?.copy_from_slice(&b);
        return Some(n);
    }
    let n = match c {
        0..=0x7F => 1,
        0x80..=0x7FF => 2,
        0x800..=0xFFFF => 3,
        0x1_0000..=0x1F_FFFF => 4,
        0x20_0000..=0x3FF_FFFF => 5,
        0x400_0000..=0x7FFF_FFFF => 6,
        _ => return None,
    };
    if n == 1 {
        *out.first_mut()? = u8::try_from(c).ok()?;
        return Some(1);
    }
    let lead: u32 = [0, 0, 0xC0, 0xE0, 0xF0, 0xF8, 0xFC].get(n).copied()?;
    *out.first_mut()? = u8::try_from(lead | (c >> (6 * (n - 1)))).ok()?;
    for i in 1..n {
        *out.get_mut(i)? = u8::try_from(0x80 | ((c >> (6 * (n - 1 - i))) & 0x3F)).ok()?;
    }
    Some(n)
}

/// ASCII equivalents of punctuation and symbols for the built-in locales' transliteration
/// (D-6.9), applied after compatibility decomposition.
const ASCII_EQUIVALENTS: [(u32, &str); 33] = [
    (0x00A9, "(C)"),
    (0x00AB, "<<"),
    (0x00AE, "(R)"),
    (0x00BB, ">>"),
    (0x00C6, "AE"),
    (0x00D7, "x"),
    (0x00DF, "ss"),
    (0x00E6, "ae"),
    (0x0152, "OE"),
    (0x0153, "oe"),
    (0x2010, "-"),
    (0x2011, "-"),
    (0x2012, "-"),
    (0x2013, "-"),
    (0x2014, "-"),
    (0x2015, "-"),
    (0x2018, "'"),
    (0x2019, "'"),
    (0x201A, "'"),
    (0x201B, "'"),
    (0x201C, "\""),
    (0x201D, "\""),
    (0x201E, "\""),
    (0x201F, "\""),
    (0x2022, "o"),
    (0x2039, "<"),
    (0x203A, ">"),
    (0x2044, "/"),
    (0x20AC, "EUR"),
    (0x2122, "TM"),
    (0x2212, "-"),
    (0x2215, "/"),
    (0x3000, " "),
];

/// The built-in locales' transliteration of `c` (D-6.9), with [`ASCII_EQUIVALENTS`] applied:
/// in `C.UTF-8` (`full`), its full decomposition without combining marks; in `C` and `POSIX`,
/// only a decomposition that involves a compatibility mapping, marks kept (a mark left over
/// does not convert, so the replacement is `?`, as the reference library's `C` locale gives).
/// A vulgar fraction (one with U+2044) is set off by spaces (` 1/2 `). `None` if the result
/// is `c` itself.
pub fn derived_translit(c: u32, full: bool, out: &mut [u32; 32]) -> Option<usize> {
    let mut h = [0u32; 3];
    let one = [c];
    let parts: &[u32] = match (crate::unicode::hangul(c, &mut h), crate::unicode::decomposition(c)) {
        (Some(n), _) if full => h.get(..n)?,
        (_, Some((compat, d))) if full || compat => d,
        _ => &one,
    };
    let fraction = parts.contains(&0x2044);
    let mut len = 0;
    let mut push = |v: u32, len: &mut usize| -> Option<()> {
        *out.get_mut(*len)? = v;
        *len += 1;
        Some(())
    };
    if fraction {
        push(u32::from(b' '), &mut len)?;
    }
    for &d in parts {
        if full && (crate::unicode::combining_class(d) != 0 || crate::unicode::is_combining(d)) {
            continue;
        }
        match ASCII_EQUIVALENTS.iter().find(|(k, _)| *k == d) {
            Some((_, s)) => s.bytes().try_for_each(|b| push(u32::from(b), &mut len))?,
            None => push(d, &mut len)?,
        }
    }
    if fraction {
        push(u32::from(b' '), &mut len)?;
    }
    (out.get(..len) != Some(&[c][..])).then_some(len)
}

/// The state a descriptor keeps between calls.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct State {
    /// The byte order a source BOM (or the first unit) selected.
    pub order: Option<Order>,
    /// Whether the target's BOM was written.
    pub bom_written: bool,
}

/// A conversion descriptor.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Conv {
    /// The source codec.
    pub from: Codec,
    /// The target codec.
    pub to: Codec,
    /// `//TRANSLIT` on the target.
    pub translit: bool,
    /// `//IGNORE` on the target.
    pub ignore: bool,
    /// The state between calls.
    pub state: State,
}

/// What a call converted, and how it ended.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Outcome {
    /// Input bytes consumed.
    pub read: usize,
    /// Output bytes written.
    pub written: usize,
    /// The irreversible conversions, or the error.
    pub result: Result<usize, Errno>,
}

impl Conv {
    /// `iconv(cd, NULL, NULL, …)`: back to the initial state (nothing to write: no codec has
    /// shift states).
    pub fn reset(&mut self) {
        self.state = State::default();
    }

    /// Convert `input` into `out` under `loc`'s `LC_CTYPE` (transliteration).
    pub fn convert(&mut self, input: &[u8], out: &mut [u8], loc: &Locale) -> Outcome {
        self.convert_with(input, out, loc, true)
    }

    /// [`Conv::convert`], the fused transcoders (`fast`) optional: the tests compare both.
    fn convert_with(&mut self, input: &[u8], out: &mut [u8], loc: &Locale, fused: bool) -> Outcome {
        let (mut read, mut written, mut irreversible, mut ignored) = (0usize, 0usize, 0usize, false);
        let end = |read, written, r: Result<usize, Errno>| Outcome { read, written, result: r };
        if let Some(bom) = self.to.bom().filter(|_| !self.state.bom_written && !input.is_empty()) {
            let Some(dst) = out.get_mut(..bom.len()) else { return end(0, 0, Err(Errno::E2BIG)) };
            dst.copy_from_slice(bom);
            written = bom.len();
            self.state.bom_written = true;
        }
        let fast = if self.from.ascii_source() { self.to.ascii_unit() } else { None };
        let unit_runs = self.from.unit_form(Some(Order::Le)).is_some() && matches!(self.to.ascii_unit(), Some((1, _)));
        // UTF-8 into a wide target: whole characters fused, the rest (errors, no room, UCS-2
        // astral) by the loop below, one character at a time (D-6.11).
        let wide = if fused && self.from == Codec::Utf8 { fast::wide_target(self.to) } else { None };
        // A UTF-16/32 source into UTF-8 likewise (ICONV-2, D-6.12), once the byte order is known.
        let narrow = fused && self.to == Codec::Utf8 && self.from.unit_form(Some(Order::Le)).is_some();
        // A single-byte charmap and UTF-8, either way, through the charmap's direct tables
        // (ICONV-3, D-6.13); what the tables do not own goes to the loop below.
        let single_in = if fused && self.to == Codec::Utf8 { fast::single_byte(self.from) } else { None };
        let single_out = if fused && self.from == Codec::Utf8 { fast::single_byte(self.to) } else { None };
        while read < input.len() {
            let rest = input.get(read..).unwrap_or(&[]);
            let fused_single = single_in
                .map(|sb| fast::single_to_utf8(rest, out.get_mut(written..).unwrap_or(&mut []), sb))
                .or_else(|| single_out.map(|sb| fast::utf8_to_single(rest, out.get_mut(written..).unwrap_or(&mut []), sb)));
            if let Some((r, wr)) = fused_single.filter(|&(r, _)| r > 0) {
                read += r;
                written += wr;
                continue;
            }
            if let Some(w) = fast::wide_source(self.from, self.state.order).filter(|_| narrow) {
                let (r, wr) = fast::wide_to_utf8(rest, out.get_mut(written..).unwrap_or(&mut []), w);
                if r > 0 {
                    read += r;
                    written += wr;
                    continue;
                }
            }
            if let Some(w) = wide {
                let (r, wr) = fast::utf8_to_wide(rest, out.get_mut(written..).unwrap_or(&mut []), w);
                if r > 0 {
                    read += r;
                    written += wr;
                    continue;
                }
            }
            // A run of ASCII units of a UTF-16 or UTF-32 source (byte order known) into a
            // target that writes ASCII as single bytes.
            if let Some((usize_src, order)) = self.from.unit_form(self.state.order).filter(|_| unit_runs) {
                let room = out.len().saturating_sub(written);
                let units = rest.chunks_exact(usize_src).take(room);
                let ascii = |u: &[u8]| match (usize_src, order) {
                    (2, Order::Le) => matches!(u, [b, 0] if *b < 0x80),
                    (2, Order::Be) => matches!(u, [0, b] if *b < 0x80),
                    (_, Order::Le) => matches!(u, [b, 0, 0, 0] if *b < 0x80),
                    (_, Order::Be) => matches!(u, [0, 0, 0, b] if *b < 0x80),
                };
                let low = |u: &[u8]| if order == Order::Le { u.first().copied() } else { u.last().copied() };
                let mut run = 0;
                for (u, slot) in units.take_while(|u| ascii(u)).zip(out.get_mut(written..).unwrap_or(&mut [])) {
                    *slot = low(u).unwrap_or(0);
                    run += 1;
                }
                if run > 0 {
                    read += run * usize_src;
                    written += run;
                    continue;
                }
            }
            // A run of ASCII converts unit by unit, without the general path.
            if let Some((size, order)) = fast {
                // `size` is 1, 2 or 4: a shift, not a division.
                let room = out.len().saturating_sub(written) >> size.trailing_zeros();
                let run = rest.iter().take(room).take_while(|b| b.is_ascii()).count();
                if run > 0 {
                    let dst = out.get_mut(written..written + run * size).unwrap_or(&mut []);
                    let src = rest.get(..run).unwrap_or(&[]);
                    let le = order == Order::Le;
                    match size {
                        1 => dst.copy_from_slice(src),
                        2 => {
                            for (unit, &b) in dst.chunks_exact_mut(2).zip(src) {
                                unit.copy_from_slice(&if le { [b, 0] } else { [0, b] });
                            }
                        }
                        _ => {
                            for (unit, &b) in dst.chunks_exact_mut(4).zip(src) {
                                unit.copy_from_slice(&if le { [b, 0, 0, 0] } else { [0, 0, 0, b] });
                            }
                        }
                    }
                    read += run;
                    written += run * size;
                    continue;
                }
            }
            let (c, used) = match self.from.decode(rest, self.state.order) {
                Dec::Char(c, n) => (c, n),
                Dec::Bom(n, o) => {
                    self.state.order = Some(o);
                    read += n;
                    continue;
                }
                Dec::Incomplete => {
                    let e = if ignored { Errno::EILSEQ } else { Errno::EINVAL };
                    return end(read, written, Err(e));
                }
                Dec::Invalid(n) => {
                    if self.ignore {
                        ignored = true;
                        read += n.clamp(1, rest.len());
                        continue;
                    }
                    return end(read, written, Err(Errno::EILSEQ));
                }
            };
            if self.state.order.is_none() && matches!(self.from, Codec::Utf16(None) | Codec::Utf32(None)) {
                self.state.order = Some(Order::Le);
            }
            // The character itself, else its transliteration (staged: all of it or nothing).
            let mut one = [0u8; 8];
            let n = match self.to.encode(c, &mut one) {
                Some(n) => {
                    let Some(dst) = out.get_mut(written..written + n) else { return end(read, written, Err(Errno::E2BIG)) };
                    // A few bytes: plain stores, not a call to the library's memcpy.
                    crate::mem::copy_inline(dst, one.get(..n).unwrap_or(&[]));
                    n
                }
                None => {
                    let mut buf = [0u8; 64];
                    match self.encode_char(c, &mut buf, loc) {
                        Some((n, lossy)) => {
                            let Some(dst) = out.get_mut(written..written + n) else { return end(read, written, Err(Errno::E2BIG)) };
                            dst.copy_from_slice(buf.get(..n).unwrap_or(&[]));
                            irreversible += usize::from(lossy);
                            n
                        }
                        None if self.ignore => {
                            ignored = true;
                            read += used;
                            continue;
                        }
                        None => return end(read, written, Err(Errno::EILSEQ)),
                    }
                }
            };
            written += n;
            read += used;
        }
        end(read, written, if ignored { Err(Errno::EILSEQ) } else { Ok(irreversible) })
    }

    /// `c` in the target, transliterated if need be: the bytes and whether it was lossy.
    fn encode_char(&self, c: u32, buf: &mut [u8; 64], loc: &Locale) -> Option<(usize, bool)> {
        let mut one = [0u8; 8];
        if let Some(n) = self.to.encode(c, &mut one) {
            buf.get_mut(..n)?.copy_from_slice(one.get(..n)?);
            return Some((n, false));
        }
        if !self.translit {
            return None;
        }
        let all = |chars: &mut dyn Iterator<Item = u32>, buf: &mut [u8; 64]| -> Option<usize> {
            let mut at = 0;
            for ch in chars {
                let mut one = [0u8; 8];
                let n = self.to.encode(ch, &mut one)?;
                buf.get_mut(at..at + n)?.copy_from_slice(one.get(..n)?);
                at += n;
            }
            Some(at)
        };
        match loc.ctype() {
            Ctype::File(f) => {
                if let Some(t) = f.translit(&[c]) {
                    for alt in t.alternatives() {
                        if let Some(n) = all(&mut alt.chars(), buf) {
                            return Some((n, true));
                        }
                    }
                }
                let missing = f.default_missing();
                if missing.is_empty() {
                    return None;
                }
                all(&mut missing.chars(), buf).map(|n| (n, true))
            }
            Ctype::C | Ctype::CUtf8 => {
                let mut d = [0u32; 32];
                if let Some(len) = derived_translit(c, matches!(loc.ctype(), Ctype::CUtf8), &mut d) {
                    if let Some(n) = all(&mut d.iter().take(len).copied(), buf) {
                        return Some((n, true));
                    }
                }
                all(&mut core::iter::once(u32::from(b'?')), buf).map(|n| (n, true))
            }
        }
    }

    /// The descriptor as words for the `iconv_t` heap block: the two codecs (tag, charmap
    /// address), the flags and the state.
    pub fn to_words(&self) -> [usize; 6] {
        let (ft, fc) = tag(self.from);
        let (tt, tc) = tag(self.to);
        let order = match self.state.order {
            None => 0,
            Some(Order::Le) => 1,
            Some(Order::Be) => 2,
        };
        [ft, fc, tt, tc, usize::from(self.translit) | usize::from(self.ignore) << 1, order | usize::from(self.state.bom_written) << 2]
    }

    /// The descriptor [`Conv::to_words`] stored; `charmap_at` turns a recorded address back
    /// into its charmap. `None` if the words are not a descriptor.
    pub fn from_words(w: [usize; 6], charmap_at: &dyn Fn(usize) -> &'static Charmap) -> Option<Conv> {
        let [ft, fc, tt, tc, flags, st] = w;
        let state = State {
            order: match st & 3 {
                0 => None,
                1 => Some(Order::Le),
                2 => Some(Order::Be),
                _ => return None,
            },
            bom_written: st & 4 != 0,
        };
        Some(Conv { from: untag(ft, fc, charmap_at)?, to: untag(tt, tc, charmap_at)?, translit: flags & 1 != 0, ignore: flags & 2 != 0, state })
    }
}

fn tag(c: Codec) -> (usize, usize) {
    let o = |o: Order| usize::from(o == Order::Be);
    match c {
        Codec::Utf8 => (1, 0),
        Codec::Utf16(None) => (2, 0),
        Codec::Utf16(Some(x)) => (3 + o(x), 0),
        Codec::Utf32(None) => (5, 0),
        Codec::Utf32(Some(x)) => (6 + o(x), 0),
        Codec::Ucs2(x) => (8 + o(x), 0),
        Codec::Ucs4(x) => (10 + o(x), 0),
        Codec::Ascii => (12, 0),
        Codec::Latin1 => (13, 0),
        Codec::Charmap(cm) => (14, core::ptr::from_ref(cm.0).expose_provenance()),
    }
}

fn untag(t: usize, addr: usize, charmap_at: &dyn Fn(usize) -> &'static Charmap) -> Option<Codec> {
    let o = |k: usize| if k == 0 { Order::Le } else { Order::Be };
    Some(match t {
        1 => Codec::Utf8,
        2 => Codec::Utf16(None),
        3 | 4 => Codec::Utf16(Some(o(t - 3))),
        5 => Codec::Utf32(None),
        6 | 7 => Codec::Utf32(Some(o(t - 6))),
        8 | 9 => Codec::Ucs2(o(t - 8)),
        10 | 11 => Codec::Ucs4(o(t - 10)),
        12 => Codec::Ascii,
        13 => Codec::Latin1,
        14 if addr != 0 => Codec::Charmap(CharmapRef(charmap_at(addr))),
        _ => return None,
    })
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::text::{Decoded, MbState};

    /// The UTF-8 decoder agrees with `text::decode` on every sequence of up to four bytes drawn
    /// from the bytes that matter at each position (leads, every continuation boundary, ASCII
    /// and the invalid bytes), whole and truncated.
    #[test]
    fn utf8_decoding_agrees_with_the_multibyte_decoder() {
        let mut bytes: [u8; 40] = [0; 40];
        let picks = [0x00u8, 0x41, 0x7F, 0x80, 0x81, 0x8F, 0x90, 0x9F, 0xA0, 0xBF, 0xC0, 0xC1, 0xC2, 0xDF, 0xE0, 0xE1, 0xEC, 0xED, 0xEE, 0xEF, 0xF0, 0xF1, 0xF3, 0xF4, 0xF5, 0xF8, 0xFC, 0xFE, 0xFF, 0x88, 0x98, 0xA8, 0xB8, 0x84, 0x94, 0xA4, 0xB4, 0x3F, 0x20, 0xC3];
        bytes.copy_from_slice(&picks);
        let reference = |s: &[u8]| match text::decode(&mut { MbState::INITIAL }, s, text::Encoding::Utf8) {
            Decoded::Char(c, n) => Dec::Char(c, n),
            Decoded::Null => Dec::Char(0, 1),
            Decoded::Incomplete => Dec::Incomplete,
            _ => Dec::Invalid(1),
        };
        let mut checked = 0usize;
        for &a in &bytes {
            if a < 0x80 {
                continue;
            }
            for &b in &bytes {
                for &c in &bytes {
                    for &d in &bytes {
                        let seq = [a, b, c, d];
                        for len in 1..=4 {
                            assert_eq!(decode_utf8(&seq[..len]), reference(&seq[..len]), "{:02x?}", &seq[..len]);
                            checked += 1;
                        }
                    }
                }
            }
        }
        // Every two-byte sequence with a non-ASCII lead.
        for a in 0x80..=0xFFu8 {
            for b in 0..=0xFFu8 {
                assert_eq!(decode_utf8(&[a, b]), reference(&[a, b]), "{a:02x} {b:02x}");
            }
        }
        assert!(checked > 1_000_000);
    }
}
