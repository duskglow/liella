//! Mount tables and `fstab` as text (spec §13.13; D-13.8, D-13.15).
//!
//! A table is lines of six fields separated by runs of spaces and tabs (fstab(5)): the source,
//! the mount point, the type, the options, the dump frequency and the pass number. Leading and
//! trailing blanks are ignored; a line whose first non-blank byte is `#` is a comment, and a
//! blank line is skipped. Missing string fields are empty and missing numbers 0; fields after
//! the sixth are ignored.
//!
//! **Escapes** (getmntent(3)): in the four string fields the sequences `\040`, `\011`, `\012`
//! and `\134` stand for a space, a tab, a newline and a backslash, and so does `\\` for a
//! backslash; every other backslash is an ordinary byte (`\101` stays as it is). The kernel
//! writes exactly the four octal forms (`/proc/self/mounts`), and so does [`put`].
//!
//! **Numbers.** `freq` and `passno` are each read from their own field as an optionally signed
//! decimal integer up to the first other byte (a field without digits is 0).
//!
//! **Lines that are not entries** (D-13.15; the reference library returns them altered): a
//! line holding a NUL byte (no C string can hold the field it cuts) and a line whose `freq` or
//! `passno` lies outside `int` are skipped, as a comment is. Lines have no length limit.
//!
//! **Options** ([`find_option`]): `opts` is a comma-separated list of `name` or `name=value`.

use crate::error::{Errno, Result};
use crate::mem::TryVec;

/// The four escapes: the byte and its text.
const ESCAPES: [(u8, &[u8; 4]); 4] = [(b' ', b"\\040"), (b'\t', b"\\011"), (b'\n', b"\\012"), (b'\\', b"\\134")];

/// One entry: the four strings decoded, and the two numbers.
#[derive(Debug)]
pub struct Entry {
    text: TryVec<u8>,
    ends: [usize; 4],
    /// The dump frequency (`mnt_freq`, `fs_freq`).
    pub freq: i32,
    /// The pass number (`mnt_passno`, `fs_passno`).
    pub passno: i32,
}

impl Entry {
    /// String field `i` (0: source, 1: mount point, 2: type, 3: options), decoded.
    pub fn field(&self, i: usize) -> &[u8] {
        let start = match i.checked_sub(1) {
            Some(p) => self.ends.get(p).copied().unwrap_or(0),
            None => 0,
        };
        let end = self.ends.get(i).copied().unwrap_or(start);
        self.text.get(start..end).unwrap_or(&[])
    }

    /// The bytes the four strings need with their terminators.
    pub fn text_len(&self) -> usize {
        self.text.len().saturating_add(4)
    }
}

fn blank(c: u8) -> bool {
    c == b' ' || c == b'\t'
}

/// The fields of `line`, split at runs of blanks.
fn fields(line: &[u8]) -> impl Iterator<Item = &[u8]> {
    line.split(|&c| blank(c)).filter(|f| !f.is_empty())
}

/// `field` decoded (the four escapes), appended to `out`.
fn decode(field: &[u8], out: &mut TryVec<u8>) -> Result<()> {
    let mut rest = field;
    while let Some((&c, tail)) = rest.split_first() {
        // A run without a backslash is copied whole.
        if c != b'\\' {
            let run = rest.iter().position(|&b| b == b'\\').unwrap_or(rest.len());
            out.extend_from_slice(rest.get(..run).unwrap_or(&[]))?;
            rest = rest.get(run..).unwrap_or(&[]);
            continue;
        }
        if c == b'\\' {
            if let Some(&(b, _)) = ESCAPES.iter().find(|(_, text)| rest.starts_with(&text[..])) {
                out.push(b)?;
                rest = rest.get(4..).unwrap_or(&[]);
                continue;
            }
            if let Some(after) = rest.strip_prefix(b"\\\\") {
                out.push(b'\\')?;
                rest = after;
                continue;
            }
        }
        out.push(c)?;
        rest = tail;
    }
    Ok(())
}

/// A number field: an optional sign and the decimal digits that follow; 0 without digits;
/// `None` outside `int`.
fn number(field: &[u8]) -> Option<i32> {
    let (neg, digits) = match field.split_first() {
        Some((b'-', d)) => (true, d),
        Some((b'+', d)) => (false, d),
        _ => (false, field),
    };
    let mut v: i64 = 0;
    for &c in digits.iter().take_while(|c| c.is_ascii_digit()) {
        v = v.checked_mul(10)?.checked_add(i64::from(c.wrapping_sub(b'0')))?;
        if v > 1 << 31 {
            return None;
        }
    }
    i32::try_from(if neg { v.checked_neg()? } else { v }).ok()
}

/// Parse one line (without its newline): the entry, or `None` for a line that is not one (a
/// comment, a blank line, a line with a NUL byte or a number outside `int`). Fails only for
/// memory.
pub fn parse(line: &[u8]) -> Result<Option<Entry>> {
    if line.contains(&0) {
        return Ok(None);
    }
    let mut it = fields(line);
    // The decoded strings are never longer than the line: one allocation.
    let mut text = TryVec::with_capacity(line.len())?;
    let mut ends = [0usize; 4];
    let mut any = false;
    for (k, end) in ends.iter_mut().enumerate() {
        if let Some(f) = it.next() {
            if k == 0 && f.first() == Some(&b'#') {
                return Ok(None);
            }
            any = true;
            decode(f, &mut text)?;
        }
        *end = text.len();
    }
    if !any {
        return Ok(None);
    }
    let (Some(freq), Some(passno)) = (it.next().map_or(Some(0), number), it.next().map_or(Some(0), number)) else { return Ok(None) };
    Ok(Some(Entry { text, ends, freq, passno }))
}

/// Append `n` in decimal.
fn put_int(n: i32, out: &mut TryVec<u8>) -> Result<()> {
    if n < 0 {
        out.push(b'-')?;
    }
    let mut v = n.unsigned_abs();
    let mut digits = [0u8; 10];
    let mut k = digits.len();
    loop {
        k = k.saturating_sub(1);
        if let Some(d) = digits.get_mut(k) {
            *d = b'0'.saturating_add(u8::try_from(v % 10).unwrap_or(0));
        }
        v /= 10;
        if v == 0 || k == 0 {
            break;
        }
    }
    out.extend_from_slice(digits.get(k..).unwrap_or(&[]))
}

/// The line of an entry (`addmntent`), with its newline: the four strings escaped, single
/// spaces, the numbers in decimal. `EINVAL` for an entry that would not read back (D-13.15): an
/// empty string (the fields would shift) or a source starting with `#` (a comment).
pub fn put(strings: [&[u8]; 4], freq: i32, passno: i32, out: &mut TryVec<u8>) -> Result<()> {
    if strings.iter().any(|s| s.is_empty()) || strings[0].first() == Some(&b'#') {
        return Err(Errno::EINVAL);
    }
    for s in strings {
        for &c in s {
            match ESCAPES.iter().find(|(b, _)| *b == c) {
                Some((_, text)) => out.extend_from_slice(&text[..])?,
                None => out.push(c)?,
            }
        }
        out.push(b' ')?;
    }
    put_int(freq, out)?;
    out.push(b' ')?;
    put_int(passno, out)?;
    out.push(b'\n')
}

/// The options of `opts` with their offsets.
fn options(opts: &[u8]) -> impl Iterator<Item = (usize, &[u8])> {
    let mut at = 0usize;
    opts.split(|&c| c == b',').map(move |o| {
        let start = at;
        at = at.saturating_add(o.len()).saturating_add(1);
        (start, o)
    })
}

/// `hasmntopt` (D-13.15): the offset in `opts` of the first option `query` names. A query
/// without `=` names an option by its name (`ro` finds `ro` and `ro=1`); a query with `=` names
/// an option exactly (`size=10` finds `size=10` only). A query with a comma names no option.
pub fn find_option(opts: &[u8], query: &[u8]) -> Option<usize> {
    if query.contains(&b',') {
        return None;
    }
    let exact = query.contains(&b'=');
    options(opts)
        .find(|(_, o)| {
            if exact {
                *o == query
            } else {
                o.strip_prefix(query).is_some_and(|rest| rest.is_empty() || rest.first() == Some(&b'='))
            }
        })
        .map(|(at, _)| at)
}

/// The BSD access type of an `fstab` entry (`fs_type`, getfsent(3)): the last of the options
/// `rw`, `rq`, `ro`, `sw`, `xx` in `opts` (by name, so `ro=1` counts as `ro`), since a later
/// option overrides an earlier one (mount(8)); `??` when there is none.
pub fn fs_type(opts: &[u8]) -> &'static [u8] {
    const TYPES: [&[u8]; 5] = [b"rw", b"rq", b"ro", b"sw", b"xx"];
    let mut found: &'static [u8] = b"??";
    for (_, o) in options(opts) {
        let name = o.split(|&c| c == b'=').next().unwrap_or(o);
        if let Some(t) = TYPES.iter().find(|t| **t == name) {
            found = t;
        }
    }
    found
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects)]
mod tests {
    use super::*;

    fn ent(line: &[u8]) -> Option<(Vec<Vec<u8>>, i32, i32)> {
        parse(line).unwrap().map(|e| ((0..4).map(|i| e.field(i).to_vec()).collect(), e.freq, e.passno))
    }

    fn strs(e: &[&str]) -> Vec<Vec<u8>> {
        e.iter().map(|s| s.as_bytes().to_vec()).collect()
    }

    #[test]
    fn fields_and_escapes() {
        assert_eq!(ent(b"  /dev/a\\040b \t /m\\011x\\012y\\134z ext4 rw 1 2  "), Some((strs(&["/dev/a b", "/m\tx\ny\\z", "ext4", "rw"]), 1, 2)));
        assert_eq!(ent(b"a\\101 b\\\\ c\\ d\\0 0 0"), Some((strs(&["a\\101", "b\\", "c\\", "d\\0"]), 0, 0)));
        assert_eq!(ent(b"a\\\\134 b\\\\\\040 c d"), Some((strs(&["a\\134", "b\\ ", "c", "d"]), 0, 0)), "left to right");
        assert_eq!(ent(b"only"), Some((strs(&["only", "", "", ""]), 0, 0)));
        assert_eq!(ent(b"a b c d 5"), Some((strs(&["a", "b", "c", "d"]), 5, 0)));
        assert_eq!(ent(b"a b c d 1 2 3 4"), Some((strs(&["a", "b", "c", "d"]), 1, 2)));
        assert_eq!(ent(b"a #b c d"), Some((strs(&["a", "#b", "c", "d"]), 0, 0)));
    }

    #[test]
    fn not_entries() {
        for l in [&b""[..], b"   \t", b"# c", b"  #x y", b"a\0b c d e 0 0", b"\0", b"a b c d 2147483648 0", b"a b c d 0 -2147483649"] {
            assert!(ent(l).is_none(), "{l:?}");
        }
    }

    #[test]
    fn numbers() {
        assert_eq!(number(b"-1"), Some(-1));
        assert_eq!(number(b"+2"), Some(2));
        assert_eq!(number(b"1abc"), Some(1));
        assert_eq!(number(b"x"), Some(0));
        assert_eq!(number(b"2147483647"), Some(i32::MAX));
        assert_eq!(number(b"-2147483648"), Some(i32::MIN));
        assert_eq!(number(b"2147483648"), None);
        assert_eq!(number(&[b'7'; 5000]), None);
        assert_eq!(ent(b"a b c d 0x10 010").map(|e| (e.1, e.2)), Some((0, 10)), "each number from its own field");
    }

    #[test]
    fn put_round_trips() {
        let mut out = TryVec::new();
        put([b"a b", b"/m\tp", b"t\ny", b"o\\p"], -1, i32::MIN, &mut out).unwrap();
        assert_eq!(&out[..], b"a\\040b /m\\011p t\\012y o\\134p -1 -2147483648\n");
        let line = out.get(..out.len() - 1).unwrap();
        assert_eq!(ent(line), Some((strs(&["a b", "/m\tp", "t\ny", "o\\p"]), -1, i32::MIN)));
        let mut out = TryVec::new();
        assert_eq!(put([b"", b"/d", b"t", b"o"], 0, 0, &mut out), Err(Errno::EINVAL));
        assert_eq!(put([b"s", b"/d", b"t", b""], 0, 0, &mut out), Err(Errno::EINVAL));
        assert_eq!(put([b"#s", b"/d", b"t", b"o"], 0, 0, &mut out), Err(Errno::EINVAL));
        put([b"s#", b"#d", b"t", b"o"], 0, 0, &mut out).unwrap();
        assert_eq!(&out[..], b"s# #d t o 0 0\n");
    }

    #[test]
    fn options_match() {
        let f = |o: &str, q: &str| find_option(o.as_bytes(), q.as_bytes());
        assert_eq!(f("rw,noro,ro", "ro"), Some(8));
        assert_eq!(f("rw=1", "rw"), Some(0));
        assert_eq!(f("rw=1", "rw="), None);
        assert_eq!(f("size==1", "size="), None);
        assert_eq!(f("size=10", "size=10"), Some(0));
        assert_eq!(f("size=10", "size=1"), None);
        assert_eq!(f("rw,ro", "rw,ro"), None);
        assert_eq!(f("a,,b", ""), Some(2));
        assert_eq!(f("relatime", "atime"), None);
        assert_eq!(f("x=a,b", "b"), Some(4));
    }

    #[test]
    fn access_type() {
        assert_eq!(fs_type(b"rw,ro"), b"ro");
        assert_eq!(fs_type(b"ro,rw"), b"rw");
        assert_eq!(fs_type(b"noauto,ro=1"), b"ro");
        assert_eq!(fs_type(b"defaults"), b"??");
        assert_eq!(fs_type(b"noro,rwx"), b"??");
        assert_eq!(fs_type(b"sw,xx"), b"xx");
    }
}
