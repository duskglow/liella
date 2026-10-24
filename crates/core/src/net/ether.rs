//! 48-bit Ethernet addresses (spec §15.7) and the lines of the `ethers` file (`ethers(5)`).
//!
//! Text form (`ether_aton`): six bytes of one or two hexadecimal digits separated by `:`. A byte
//! of one digit must be followed by its separator (`:`, or for the sixth byte the end of the
//! text or white space); after a two-digit sixth byte the rest of the text is not read
//! (netaddr.md). Bytes are delivered as their separators are seen, so a failed parse leaves the
//! bytes before the failure (the reference does the same; callers that pass a buffer see it).
//! Output (`ether_ntoa`): lowercase hexadecimal without leading zeros.

use super::{hex_value, is_space, Text};

/// The text of an Ethernet address (at most 17 bytes).
pub type EtherText = Text<18>;

/// The result of reading an address at the start of a text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Parsed {
    /// The bytes read, the first `stored` of them valid.
    pub bytes: [u8; 6],
    /// How many bytes were delivered (6 on success).
    pub stored: usize,
    /// Where the text after the address starts (on success).
    pub end: usize,
}

impl Parsed {
    /// The address, if all six bytes were read.
    pub fn address(&self) -> Option<[u8; 6]> {
        (self.stored == 6).then_some(self.bytes)
    }
}

/// Read an address at the start of `text`.
pub fn parse(text: &[u8]) -> Parsed {
    let mut p = Parsed { bytes: [0; 6], stored: 0, end: 0 };
    let mut i = 0usize;
    for k in 0..6usize {
        let Some(hi) = text.get(i).copied().and_then(hex_value) else { return p };
        i = i.saturating_add(1);
        let (v, two) = match text.get(i).copied().and_then(hex_value) {
            Some(lo) => {
                i = i.saturating_add(1);
                (hi.wrapping_shl(4) | lo, true)
            }
            None => (hi, false),
        };
        let next = text.get(i).copied();
        if k < 5 {
            if next != Some(b':') {
                return p;
            }
            i = i.saturating_add(1);
        } else if !two && next.is_some_and(|c| !is_space(c)) {
            return p;
        }
        if let Some(slot) = p.bytes.get_mut(k) {
            *slot = v;
        }
        p.stored = k.saturating_add(1);
    }
    p.end = i;
    p
}

/// The text of `a`: lowercase, no leading zeros.
pub fn format(a: [u8; 6]) -> EtherText {
    let mut t = EtherText::new();
    for (i, b) in a.iter().enumerate() {
        if i > 0 {
            t.push(b':');
        }
        t.push_hex(u32::from(*b), false);
    }
    t
}

/// One line of the `ethers` file (`ether_line`): the address at the start of the line, then
/// white space, then a host name, which ends at white space, `#` or the end of the line. A
/// line without a name is invalid. `parsed` carries the address bytes read, valid or not.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Line<'a> {
    /// The address as read.
    pub parsed: Parsed,
    /// The host name, if the line is valid.
    pub host: Option<&'a [u8]>,
}

/// Read one `ethers` line. Unlike the reference, which skips any one character after the
/// address (`00:11:22:33:44:55host` names `ost`), the address must be followed by white space
/// (`ethers(5)`: "an Ethernet address followed by whitespace and a host name").
pub fn line(text: &[u8]) -> Line<'_> {
    let parsed = parse(text);
    let host = (|| {
        parsed.address()?;
        let rest = text.get(parsed.end..)?;
        if !rest.first().copied().is_some_and(is_space) {
            return None;
        }
        let start = rest.iter().position(|&c| !is_space(c))?;
        let name = rest.get(start..)?;
        let len = name.iter().position(|&c| is_space(c) || c == b'#').unwrap_or(name.len());
        (len > 0).then(|| name.get(..len)).flatten()
    })();
    Line { parsed, host }
}

/// The entries of an `ethers` file's text: leading white space and `#` comments allowed,
/// invalid lines skipped (as the reference reads the file; netaddr.md, netdb.md).
pub fn entries(file: &[u8]) -> impl Iterator<Item = ([u8; 6], &[u8])> {
    file.split(|&c| c == b'\n').filter_map(|l| {
        let start = l.iter().position(|&c| !is_space(c))?;
        let l = l.get(start..)?;
        if l.first() == Some(&b'#') {
            return None;
        }
        let r = line(l);
        Some((r.parsed.address()?, r.host?))
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn text_forms() {
        assert_eq!(parse(b"00:11:22:33:44:55").address(), Some([0, 0x11, 0x22, 0x33, 0x44, 0x55]));
        assert_eq!(parse(b"0:1:2:3:4:5 x").address(), Some([0, 1, 2, 3, 4, 5]));
        assert_eq!(parse(b"00:11:22:33:44:55:66").address(), Some([0, 0x11, 0x22, 0x33, 0x44, 0x55]));
        let p = parse(b"00:11:22:33:44");
        assert_eq!((p.address(), p.stored), (None, 4));
        let p = parse(b"00:11:22:33:44:5g");
        assert_eq!((p.address(), p.stored), (None, 5));
        assert_eq!(parse(b"000:11:22:33:44:55").stored, 0);
        assert_eq!(format([0xab, 0, 0xc, 0xff, 0x10, 1]).as_bytes(), b"ab:0:c:ff:10:1");
    }

    #[test]
    fn lines() {
        assert_eq!(line(b"00:11:22:33:44:55\thost # c").host, Some(&b"host"[..]));
        assert_eq!(line(b"00:11:22:33:44:55 a b").host, Some(&b"a"[..]));
        assert_eq!(line(b"00:11:22:33:44:55 #x").host, None);
        assert_eq!(line(b"00:11:22:33:44:55host").host, None);
        assert_eq!(line(b"  00:11:22:33:44:55 x").host, None);
        let f = b"# c\n00:11:22:33:44:55 alpha\n  0:1:2:3:4:5\tbeta # t\nbad\n";
        let v: Vec<_> = entries(f).collect();
        assert_eq!(v.len(), 2);
        assert_eq!(v[1].1, b"beta");
    }
}
