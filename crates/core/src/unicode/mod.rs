//! Unicode character properties (IP-19, D-5.4): the built-in `C.UTF-8` locale's classes, simple
//! case maps and display widths, and the internal Unicode API (full case mapping, D-5.2; case
//! folding; decomposition). The tables are generated from the Unicode 15.1.0 Character
//! Database by `cargo xtask unicode-gen` (`tables.rs`).

mod tables;

use crate::ctype::Class;

/// The properties of one code point.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Props {
    /// The `<ctype.h>` class mask (`_IS*` bits).
    pub(crate) class: u16,
    /// Bit 0: in class `combining`; bit 1: in class `combining_level3`.
    pub(crate) extra: u8,
    /// `wcwidth`.
    pub(crate) width: i8,
    /// Simple case mappings, as differences from the code point.
    pub(crate) upper: i32,
    pub(crate) lower: i32,
    pub(crate) title: i32,
}

/// No properties: an unassigned code point, or a value above U+10FFFF.
const NONE: Props = Props { class: 0, extra: 0, width: -1, upper: 0, lower: 0, title: 0 };

fn props(c: u32) -> Props {
    let block = usize::try_from(c >> tables::BLOCK_SHIFT).unwrap_or(usize::MAX);
    let Some(&b) = tables::STAGE1.get(block) else { return NONE };
    let low = usize::try_from(c & 0xff).unwrap_or(0);
    let id = tables::STAGE2.get(usize::from(b)).and_then(|blk| blk.get(low)).copied();
    id.and_then(|i| tables::RECORDS.get(usize::from(i))).copied().unwrap_or(NONE)
}

/// The `<ctype.h>` classes of code point `c` (the `C.UTF-8` definition).
pub fn classes(c: u32) -> Class {
    Class(props(c).class)
}

/// Whether `c` is in the `C.UTF-8` class `combining`.
pub fn is_combining(c: u32) -> bool {
    props(c).extra & 1 != 0
}

/// Whether `c` is in the `C.UTF-8` class `combining_level3`.
pub fn is_combining_level3(c: u32) -> bool {
    props(c).extra & 2 != 0
}

/// `wcwidth` in `C.UTF-8`: 0, 1, 2, or −1 for a character that is not printable.
pub fn width(c: u32) -> i32 {
    i32::from(props(c).width)
}

fn apply(c: u32, delta: i32) -> u32 {
    c.checked_add_signed(delta).unwrap_or(c)
}

/// The simple (one-to-one) uppercase mapping.
pub fn to_upper(c: u32) -> u32 {
    apply(c, props(c).upper)
}

/// The simple lowercase mapping.
pub fn to_lower(c: u32) -> u32 {
    apply(c, props(c).lower)
}

/// The simple titlecase mapping.
pub fn to_title(c: u32) -> u32 {
    apply(c, props(c).title)
}

fn pool(span: (u32, u8)) -> &'static [u32] {
    let start = usize::try_from(span.0).unwrap_or(usize::MAX);
    tables::POOL.get(start..start.saturating_add(usize::from(span.1))).unwrap_or(&[])
}

/// Which full case mapping.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Case {
    /// Lowercase.
    Lower,
    /// Titlecase.
    Title,
    /// Uppercase.
    Upper,
}

/// The unconditional full case mapping of `c` (D-5.2), written to `out`; returns the number of
/// code points (at most 3). Context-dependent mappings (final sigma, language rules) are the
/// caller's.
pub fn full_case(c: u32, case: Case, out: &mut [u32; 3]) -> usize {
    if let Ok(i) = tables::SPECIAL.binary_search_by_key(&c, |e| e.0) {
        let k = match case {
            Case::Lower => 0,
            Case::Title => 1,
            Case::Upper => 2,
        };
        let span = tables::SPECIAL.get(i).and_then(|e| e.1.get(k)).copied().unwrap_or((0, 0));
        let seq = pool(span);
        for (o, &v) in out.iter_mut().zip(seq) {
            *o = v;
        }
        return seq.len().min(3);
    }
    out[0] = match case {
        Case::Lower => to_lower(c),
        Case::Title => to_title(c),
        Case::Upper => to_upper(c),
    };
    1
}

/// The full case folding of `c` (for case-insensitive comparison, D-4.4), written to `out`;
/// returns the number of code points (at most 3).
pub fn fold(c: u32, out: &mut [u32; 3]) -> usize {
    match tables::FOLD.binary_search_by_key(&c, |e| e.0) {
        Ok(i) => {
            let seq = tables::FOLD.get(i).map_or(&[][..], |e| pool(e.1));
            for (o, &v) in out.iter_mut().zip(seq) {
                *o = v;
            }
            seq.len().min(3)
        }
        Err(_) => {
            out[0] = c;
            1
        }
    }
}

/// The full decomposition of `c` and whether it involves a compatibility mapping; `None` if `c`
/// does not decompose. Hangul syllables decompose algorithmically ([`hangul`]).
pub fn decomposition(c: u32) -> Option<(bool, &'static [u32])> {
    let i = tables::DECOMP.binary_search_by_key(&c, |e| e.0).ok()?;
    tables::DECOMP.get(i).map(|e| (e.1, pool(e.2)))
}

/// The canonical decomposition of a precomposed Hangul syllable into two or three jamo.
pub fn hangul(c: u32, out: &mut [u32; 3]) -> Option<usize> {
    const S_BASE: u32 = 0xAC00;
    const COUNT: u32 = 11172;
    let s = c.checked_sub(S_BASE).filter(|s| *s < COUNT)?;
    // s < 11172, so every result below is a small constant plus a bounded quotient.
    out[0] = 0x1100u32.checked_add(s.checked_div(588)?)?;
    out[1] = 0x1161u32.checked_add(s.checked_rem(588)?.checked_div(28)?)?;
    let t = s.checked_rem(28)?;
    if t == 0 {
        Some(2)
    } else {
        out[2] = 0x11A7u32.checked_add(t)?;
        Some(3)
    }
}

/// The canonical combining class of `c`.
pub fn combining_class(c: u32) -> u8 {
    let i = tables::CCC.partition_point(|e| e.1 < c);
    match tables::CCC.get(i) {
        Some(&(a, _, k)) if a <= c => k,
        _ => 0,
    }
}
