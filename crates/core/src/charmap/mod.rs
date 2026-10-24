//! Charsets from the installed charmaps (D-6.8): the POSIX charmap format, read once and kept
//! for the process. A [`Charmap`] decodes byte sequences to code points and encodes back.
//!
//! **Format.** A header of `<code_set_name>`, `<mb_cur_max>`, `<mb_cur_min>`,
//! `<escape_char>` and `<comment_char>` lines, `% alias NAME` comments, then between
//! `CHARMAP` and `END CHARMAP` one mapping per line: `<Uxxxx> /x41 description`, or a range
//! `<Uxxxx>..<Uyyyy> /x81/x30/x81/x30` whose code points map to consecutive values of the
//! sequence's last byte. Byte values are written `/xHH`, `/dNNN` or `/oOOO` (with the escape
//! character). Lines naming more than one code point (rare combining sequences) and symbolic
//! names other than `<U…>` are skipped.

#![allow(clippy::arithmetic_side_effects)] // byte and count arithmetic bounded by the line's values

pub mod inflate;

use crate::error::{Errno, Result};
use crate::mem::TryVec;

/// The longest byte sequence of one character.
pub const MAX_BYTES: usize = 4;

/// One decoding step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// A character and the bytes it took.
    Char(u32, usize),
    /// The input is a proper prefix of a character: more is needed.
    Incomplete,
    /// The input begins no character.
    Invalid,
}

/// One mapping: `count` code points from `cp` to as many sequences, the last byte counting up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Range {
    cp: u32,
    count: u32,
    bytes: [u8; MAX_BYTES],
    len: u8,
}

impl Range {
    fn seq(&self) -> &[u8] {
        self.bytes.get(..usize::from(self.len)).unwrap_or(&[])
    }

    /// The sequence of `cp`, which the range covers. One addition to the little-endian word:
    /// the last byte never wraps (parsing checks it), and a byte-wise update would stall the
    /// store forwarding of the word read that follows.
    fn seq_of(&self, cp: u32) -> [u8; MAX_BYTES] {
        let shift = 8 * u32::from(self.len.saturating_sub(1));
        u32::from_le_bytes(self.bytes).wrapping_add(cp.wrapping_sub(self.cp) << shift).to_le_bytes()
    }

    /// Whether `next` continues this range: the next code points to the next sequences.
    fn continued_by(&self, next: &Range) -> bool {
        let n = usize::from(self.len);
        let last = |r: &Range| u32::from(r.bytes.get(n.saturating_sub(1)).copied().unwrap_or(0));
        self.len == next.len
            && self.cp.checked_add(self.count) == Some(next.cp)
            && self.bytes.get(..n.saturating_sub(1)) == next.bytes.get(..n.saturating_sub(1))
            && last(self) + self.count == last(next)
    }
}

/// `v` (sorted) with each run of ranges that continue one another merged into one: the
/// one-character lines of the 8-bit charmaps become a few ranges, so lookups search few.
fn merged(v: &TryVec<Range>) -> Result<TryVec<Range>> {
    let mut out: TryVec<Range> = TryVec::with_capacity(v.len())?;
    for r in v.iter() {
        match out.last_mut() {
            Some(prev) if prev.continued_by(r) => prev.count += r.count,
            _ => out.push(*r)?,
        }
    }
    Ok(out)
}

/// A parsed charmap.
#[derive(Debug)]
pub struct Charmap {
    name: TryVec<u8>,
    aliases: TryVec<TryVec<u8>>,
    mb_cur_max: usize,
    /// The longest sequence listed (may exceed `<mb_cur_max>` in composing charsets).
    max_len: usize,
    /// By byte sequence (length, then bytes).
    by_bytes: TryVec<Range>,
    /// By code point.
    by_cp: TryVec<Range>,
    /// `reach[i]`: the largest end (`cp + count`) of `by_cp[..=i]`, so that a search for the
    /// ranges covering a code point stops where no earlier range reaches it.
    reach: TryVec<u32>,
    /// Whether every code point below 0x80 is the one byte of the same value, both ways.
    ascii: bool,
    /// Every proper prefix of a multibyte sequence (length, bytes), sorted.
    prefixes: TryVec<([u8; MAX_BYTES], u8)>,
    /// Direct tables when every sequence is one byte (built at load, P8).
    single: Option<SingleByte>,
}

/// The tables of a single-byte charset, once per loaded charmap (about 3.3 KiB, immutable;
/// D-6.13). `decode` and `encode` answer the general lookups; the rest serves the fused
/// charmap <-> UTF-8 transcoder (`iconv::fast`), which hands every character it does not find
/// here back to the general loop, so a missing entry is never an error, only a slower path.
#[derive(Debug)]
pub struct SingleByte {
    /// The code point of each byte (`u32::MAX`: none).
    decode: [u32; 256],
    /// The byte of each code point below 0x100 (`u16::MAX`: none; the lowest byte when a code
    /// point is listed twice).
    encode: [u16; 256],
    /// The UTF-8 form of each byte: length and bytes; length 0 for a byte with no character, a
    /// surrogate or a code point above U+FFFF (left to the general loop).
    utf8: [(u8, [u8; 3]); 256],
    /// The BMP code points from 0x100 the charset has, ascending, with their bytes
    /// (`high_n` entries; as `encode` answers them).
    high_cp: [u16; 256],
    high_byte: [u8; 256],
    high_n: usize,
    /// Whether every byte below 0x80 is that code point (the charmap's `ascii`).
    ascii: bool,
}

impl SingleByte {
    /// The UTF-8 form of byte `b`: its length (0: not for the fused path) and bytes.
    #[inline(always)]
    pub fn utf8(&self, b: u8) -> (u8, [u8; 3]) {
        self.utf8.get(usize::from(b)).copied().unwrap_or((0, [0; 3]))
    }

    /// The byte of `cp`, as `Charmap::encode` answers; `None` also for what the fused path
    /// leaves to the general loop (above U+FFFF).
    #[inline(always)]
    pub fn byte_of(&self, cp: u32) -> Option<u8> {
        if cp < 0x100 {
            return u8::try_from(*self.encode.get(usize::try_from(cp).ok()?)?).ok();
        }
        let n = self.high_n;
        let cps = self.high_cp.get(..n)?;
        let key = u16::try_from(cp).ok()?;
        let i = cps.binary_search(&key).ok()?;
        self.high_byte.get(i).copied()
    }

    /// Whether the bytes below 0x80 are ASCII.
    #[inline(always)]
    pub fn ascii(&self) -> bool {
        self.ascii
    }
}

/// Parse one byte value (`/xHH`, `/dNNN`, `/oOOO`) at the start of `s`; the value and the bytes
/// it took.
fn byte_value(s: &[u8], esc: u8) -> Option<(u8, usize)> {
    if s.first() != Some(&esc) {
        return None;
    }
    let (radix, max) = match s.get(1)? {
        b'x' => (16, 2),
        b'd' => (10, 3),
        b'o' => (8, 3),
        _ => return None,
    };
    let digits = s.get(2..)?.iter().take(max).take_while(|c| char::from(**c).is_digit(radix)).count();
    if digits == 0 {
        return None;
    }
    let text = core::str::from_utf8(s.get(2..2 + digits)?).ok()?;
    Some((u8::try_from(u32::from_str_radix(text, radix).ok()?).ok()?, 2 + digits))
}

/// `<Uxxxx>` at the start of `s`: the code point and the bytes it took.
fn symbol(s: &[u8]) -> Option<(u32, usize)> {
    if s.get(..2)? != b"<U" {
        return None;
    }
    let end = s.iter().position(|&c| c == b'>')?;
    let hex = core::str::from_utf8(s.get(2..end)?).ok()?;
    Some((u32::from_str_radix(hex, 16).ok()?, end + 1))
}

/// The end of a symbolic name `<…>` at the start of `s` (the escape character quotes the
/// next byte, so `<D//>` and `</>>` are names).
fn name_end(s: &[u8], esc: u8) -> Option<usize> {
    if s.first() != Some(&b'<') {
        return None;
    }
    let mut i = 1;
    while let Some(&c) = s.get(i) {
        if c == esc {
            i += 2;
            continue;
        }
        if c == b'>' {
            return Some(i + 1);
        }
        i += 1;
    }
    None
}

fn trim(s: &[u8]) -> &[u8] {
    s.trim_ascii()
}

/// The names a charmap's header gives (`<code_set_name>` and the `alias` comment lines),
/// read from the text before its `CHARMAP` line; `f` sees each. For finding a charmap by an
/// alias without parsing every file.
pub fn header_names(text: &[u8], f: &mut dyn FnMut(&[u8])) {
    let mut comment = b'#';
    for raw in text.split(|&c| c == b'\n') {
        let line = trim(raw);
        if line == b"CHARMAP" {
            return;
        }
        if let Some(rest) = line.strip_prefix(b"<code_set_name>") {
            f(trim(rest));
        } else if let Some(rest) = line.strip_prefix(b"<comment_char>") {
            comment = trim(rest).first().copied().unwrap_or(comment);
        } else if line.first() == Some(&comment) {
            if let Some(a) = trim(line.get(1..).unwrap_or(&[])).strip_prefix(b"alias") {
                f(trim(a));
            }
        }
    }
}

impl Charmap {
    /// Parse a charmap's text. Lines that do not parse are skipped; a charmap without any
    /// mapping is `EINVAL`.
    pub fn parse(text: &[u8]) -> Result<Charmap> {
        let mut cm = Charmap {
            name: TryVec::new(),
            aliases: TryVec::new(),
            mb_cur_max: 1,
            max_len: 1,
            by_bytes: TryVec::new(),
            by_cp: TryVec::new(),
            reach: TryVec::new(),
            ascii: false,
            prefixes: TryVec::new(),
            single: None,
        };
        // The escape character defaults to '/', the glibc charmaps' convention (POSIX says '\\';
        // the charmaps that rely on a default write '/').
        let (mut esc, mut comment) = (b'/', b'#');
        let mut in_map = false;
        for raw in text.split(|&c| c == b'\n') {
            let line = trim(raw);
            if !in_map {
                if let Some(rest) = line.strip_prefix(b"<code_set_name>") {
                    cm.name.extend_from_slice(trim(rest))?;
                } else if let Some(rest) = line.strip_prefix(b"<mb_cur_max>") {
                    cm.mb_cur_max = core::str::from_utf8(trim(rest)).ok().and_then(|t| t.parse().ok()).unwrap_or(1);
                } else if let Some(rest) = line.strip_prefix(b"<escape_char>") {
                    esc = trim(rest).first().copied().unwrap_or(esc);
                } else if let Some(rest) = line.strip_prefix(b"<comment_char>") {
                    comment = trim(rest).first().copied().unwrap_or(comment);
                } else if line.first() == Some(&comment) {
                    let body = trim(line.get(1..).unwrap_or(&[]));
                    if let Some(a) = body.strip_prefix(b"alias") {
                        let mut v = TryVec::new();
                        v.extend_from_slice(trim(a))?;
                        if !v.is_empty() {
                            cm.aliases.push(v)?;
                        }
                    }
                } else if line == b"CHARMAP" {
                    in_map = true;
                } else if line.starts_with(b"<U") && line.contains(&esc) {
                    // A mapping before any CHARMAP line: some files have only END CHARMAP.
                    in_map = true;
                }
                if !in_map {
                    continue;
                }
            }
            if line.starts_with(b"END CHARMAP") {
                break;
            }
            if line.first() == Some(&comment) || line.is_empty() {
                continue;
            }
            // <Ua>[..<Ub>] bytes, or <name> bytes <Ua> (a symbolic name, the code point after).
            let (first, n, named) = match symbol(line) {
                Some((cp, n)) => (cp, n, false),
                None => {
                    let Some(close) = name_end(line, esc) else { continue };
                    let after = trim(line.get(close..).unwrap_or(&[]));
                    let mut p = after;
                    while let Some((_, used)) = byte_value(p, esc) {
                        p = p.get(used..).unwrap_or(&[]);
                    }
                    let Some((cp, _)) = symbol(trim(p)) else { continue };
                    (cp, close, true)
                }
            };
            let mut rest = line.get(n..).unwrap_or(&[]);
            let mut last = first;
            if named {
                // The bytes follow the name; the code point after them was read above.
            } else if let Some(r) = rest.strip_prefix(b"..") {
                let Some((l, m)) = symbol(r) else { continue };
                last = l;
                rest = r.get(m..).unwrap_or(&[]);
            } else if rest.starts_with(b"<") {
                continue; // several code points for one sequence
            }
            let mut rest = trim(rest);
            let mut bytes = [0u8; MAX_BYTES];
            let mut len = 0usize;
            while let Some((v, used)) = byte_value(rest, esc) {
                if len == MAX_BYTES {
                    len = MAX_BYTES + 1;
                    break;
                }
                *bytes.get_mut(len).ok_or(Errno::EINVAL)? = v;
                len += 1;
                rest = rest.get(used..).unwrap_or(&[]);
            }
            if len == 0 || len > MAX_BYTES || last < first {
                continue;
            }
            let count = last - first + 1;
            // The last byte counts up; it must not wrap.
            if u32::from(*bytes.get(len - 1).ok_or(Errno::EINVAL)?) + count - 1 > 0xff {
                continue;
            }
            let r = Range { cp: first, count, bytes, len: u8::try_from(len).map_err(|_| Errno::EINVAL)? };
            cm.by_bytes.push(r)?;
            cm.by_cp.push(r)?;
            cm.max_len = cm.max_len.max(len);
            for k in 1..len {
                let mut p = [0u8; MAX_BYTES];
                if let (Some(dst), Some(src)) = (p.get_mut(..k), bytes.get(..k)) {
                    dst.copy_from_slice(src);
                }
                cm.prefixes.push((p, u8::try_from(k).map_err(|_| Errno::EINVAL)?))?;
            }
        }
        if cm.by_bytes.is_empty() {
            return Err(Errno::EINVAL);
        }
        cm.by_bytes.sort_by_key(|r| (r.len, r.bytes));
        cm.by_bytes = merged(&cm.by_bytes)?;
        // A code point listed under several sequences encodes as the lowest of them (as the
        // reference iconv does: ARMSCII-8's comma is 0x2c, not 0xab).
        cm.by_cp.sort_by_key(|r| (r.cp, r.len, r.bytes));
        cm.by_cp = merged(&cm.by_cp)?;
        let mut far = 0u32;
        for r in cm.by_cp.iter() {
            far = far.max(r.cp.saturating_add(r.count));
            cm.reach.push(far)?;
        }
        cm.prefixes.sort_unstable();
        let mut unique = TryVec::with_capacity(cm.prefixes.len())?;
        for p in cm.prefixes.iter() {
            if unique.last() != Some(p) {
                unique.push(*p)?;
            }
        }
        cm.prefixes = unique;
        cm.mb_cur_max = cm.mb_cur_max.clamp(1, MAX_BYTES);
        let mut one = [0u8; MAX_BYTES];
        cm.ascii = (0u8..0x80).all(|b| cm.decode(&[b]) == Step::Char(u32::from(b), 1) && cm.encode(u32::from(b), &mut one) == Some(1) && one.first() == Some(&b));
        if cm.max_len == 1 {
            // The tables answer exactly what the general lookups answer (built from them).
            let mut t = SingleByte { decode: [u32::MAX; 256], encode: [u16::MAX; 256], utf8: [(0, [0; 3]); 256], high_cp: [0; 256], high_byte: [0; 256], high_n: 0, ascii: false };
            for b in 0..=255u8 {
                if let (Step::Char(cp, 1), Some(slot)) = (cm.decode(&[b]), t.decode.get_mut(usize::from(b))) {
                    *slot = cp;
                }
            }
            for cp in 0..256u32 {
                if let (Some(1), Some(slot)) = (cm.encode(cp, &mut one), t.encode.get_mut(usize::try_from(cp).unwrap_or(usize::MAX))) {
                    *slot = one.first().copied().map_or(u16::MAX, u16::from);
                }
            }
            // The fused tables, from the general lookups so that they agree with them.
            let mut found = [(0u16, 0u8); 256];
            let mut hi = 0usize;
            for (cp, _, _) in cm.mappings() {
                let Ok(key) = u16::try_from(cp) else { continue };
                if key < 0x100 || (0xD800..0xE000).contains(&key) || cm.encode(cp, &mut one) != Some(1) {
                    continue;
                }
                let (Some(&byte), Some(slot)) = (one.first(), found.get_mut(hi)) else { continue };
                *slot = (key, byte);
                hi += 1;
            }
            let found = found.get_mut(..hi).unwrap_or(&mut []);
            found.sort_unstable();
            let mut last = None;
            hi = 0;
            for &(key, byte) in found.iter() {
                if last == Some(key) {
                    continue;
                }
                last = Some(key);
                if let (Some(c), Some(b)) = (t.high_cp.get_mut(hi), t.high_byte.get_mut(hi)) {
                    (*c, *b) = (key, byte);
                    hi += 1;
                }
            }
            t.high_n = hi;
            t.ascii = cm.ascii;
            for (slot, &cp) in t.utf8.iter_mut().zip(t.decode.iter()) {
                *slot = match char::from_u32(cp) {
                    Some(ch) if cp < 0x1_0000 => {
                        let mut buf = [0u8; 4];
                        let enc = ch.encode_utf8(&mut buf).as_bytes();
                        let mut bytes = [0u8; 3];
                        if let Some(d) = bytes.get_mut(..enc.len()) {
                            d.copy_from_slice(enc);
                        }
                        (u8::try_from(enc.len()).unwrap_or(0), bytes)
                    }
                    _ => (0, [0; 3]),
                };
            }
            cm.single = Some(t);
        }
        Ok(cm)
    }

    /// The direct tables when every sequence is one byte (D-6.13).
    pub fn single_byte(&self) -> Option<&SingleByte> {
        self.single.as_ref()
    }

    /// The charset's name (`<code_set_name>`).
    pub fn name(&self) -> &[u8] {
        &self.name
    }

    /// The charset's aliases (the `alias` comment lines).
    pub fn aliases(&self) -> impl Iterator<Item = &[u8]> + '_ {
        self.aliases.iter().map(|a| &a[..])
    }

    /// Whether ASCII is itself in this charset (each byte below 0x80 is that code point).
    pub fn ascii(&self) -> bool {
        self.ascii
    }

    /// The longest sequence (`MB_CUR_MAX`).
    pub fn mb_cur_max(&self) -> usize {
        self.mb_cur_max
    }

    /// The mapping whose sequences begin with the `len` bytes of `seq`, if any.
    fn find_bytes(&self, seq: &[u8]) -> Option<u32> {
        let len = u8::try_from(seq.len()).ok()?;
        let mut key = [0u8; MAX_BYTES];
        key.get_mut(..seq.len())?.copy_from_slice(seq);
        // The last range starting at or before `seq` with this length.
        let i = self.by_bytes.partition_point(|r| (r.len, r.bytes) <= (len, key));
        let r = self.by_bytes.get(i.checked_sub(1)?)?;
        if r.len != len || r.seq().get(..seq.len() - 1) != seq.get(..seq.len() - 1) {
            return None;
        }
        let off = u32::from(*seq.last()?).checked_sub(u32::from(*r.seq().last()?))?;
        (off < r.count).then(|| r.cp + off)
    }

    fn is_prefix(&self, seq: &[u8]) -> bool {
        let Ok(len) = u8::try_from(seq.len()) else { return false };
        let mut key = [0u8; MAX_BYTES];
        if let Some(k) = key.get_mut(..seq.len()) {
            k.copy_from_slice(seq);
        }
        self.prefixes.binary_search(&(key, len)).is_ok()
    }

    /// The longest byte sequence the charmap lists (1 for a single-byte charset). A decoder
    /// fed a growing window needs another byte only while a character may still be longer.
    pub fn max_len(&self) -> usize {
        self.max_len
    }

    /// Decode one character from `input`.
    /// The longest listed sequence wins (TCVN5712-1 and TSCII list composed pairs); when the
    /// input ends inside a longer candidate, a shorter complete character is taken.
    pub fn decode(&self, input: &[u8]) -> Step {
        if let Some(t) = &self.single {
            return match input.first() {
                None => Step::Incomplete,
                Some(&b) => match t.decode.get(usize::from(b)).copied() {
                    Some(cp) if cp != u32::MAX => Step::Char(cp, 1),
                    _ => Step::Invalid,
                },
            };
        }
        let mut best: Option<(u32, usize)> = None;
        for len in 1..=self.max_len.min(input.len()) {
            let Some(seq) = input.get(..len) else { break };
            if let Some(cp) = self.find_bytes(seq) {
                best = Some((cp, len));
            }
            if !self.is_prefix(seq) {
                break;
            }
        }
        match best {
            Some((cp, n)) => Step::Char(cp, n),
            None if input.is_empty() || (input.len() < self.max_len && self.is_prefix(input)) => Step::Incomplete,
            None => Step::Invalid,
        }
    }

    /// Every mapping as (code point, bytes), in byte order (for tests and transliteration
    /// tables).
    pub fn mappings(&self) -> impl Iterator<Item = (u32, [u8; MAX_BYTES], usize)> + '_ {
        self.by_bytes.iter().flat_map(|r| {
            (0..r.count).map(move |k| {
                let mut b = r.bytes;
                if let Some(x) = b.get_mut(usize::from(r.len).saturating_sub(1)) {
                    *x = u8::try_from(u32::from(*x) + k).unwrap_or(0xff);
                }
                (r.cp + k, b, usize::from(r.len))
            })
        })
    }

    /// Encode `cp` into `out`; the byte count, or `None` if the charset cannot represent it. A
    /// code point listed under several sequences takes the lowest.
    pub fn encode(&self, cp: u32, out: &mut [u8]) -> Option<usize> {
        if let Some(t) = &self.single {
            if let Some(&b) = usize::try_from(cp).ok().and_then(|i| t.encode.get(i)) {
                *out.first_mut()? = u8::try_from(b).ok()?;
                return Some(1);
            }
        }
        let i = self.by_cp.partition_point(|r| r.cp <= cp);
        // Ranges covering `cp` lie before `i` and after the last place where nothing reaches
        // past `cp` (`reach` grows monotonically; usually one range, a few when a code point is
        // listed twice).
        let start = self.reach.get(..i)?.partition_point(|&end| end <= cp);
        let mut best: Option<(u8, [u8; MAX_BYTES])> = None;
        for r in self.by_cp.get(start..i)? {
            if cp - r.cp < r.count {
                let seq = (r.len, r.seq_of(cp));
                if best.is_none_or(|b| seq < b) {
                    best = Some(seq);
                }
            }
        }
        let (len, bytes) = best?;
        let len = usize::from(len);
        out.get_mut(..len)?.copy_from_slice(bytes.get(..len)?);
        Some(len)
    }
}
