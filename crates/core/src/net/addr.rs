//! The strict text forms of Internet addresses (spec §15.3: `inet_pton`, `inet_ntop`).
//!
//! * IPv4: exactly four decimal parts of 0–255, without leading zeros (spec §15.3; RFC 6943
//!   §3.1.1's "strict form").
//! * IPv6: the RFC 4291 §2.2 forms: eight groups of one to four hexadecimal digits, one `::`
//!   standing for one or more zero groups, and an IPv4 dotted quad (strict) as the last 32
//!   bits. No zone suffix, no surrounding white space.
//! * Formatting follows RFC 5952 §4: lowercase, no leading zeros, the longest run of two or more
//!   zero groups compressed (the first of equal runs), and the IPv4 dotted quad for the
//!   IPv4-mapped prefix `::ffff:0:0/96` and for the IPv4-compatible `::/96` addresses whose
//!   last 32 bits do not start with a zero group (`::1` stays `::1`, `::0.1.0.2` is mixed),
//!   as the reference formats them (netaddr.md).

use super::{hex_value, Text};

/// The longest IPv6 text plus its terminator (`INET6_ADDRSTRLEN`).
pub const INET6_ADDRSTRLEN: usize = 46;
/// The longest IPv4 text plus its terminator (`INET_ADDRSTRLEN`).
pub const INET_ADDRSTRLEN: usize = 16;

/// An address text (at most 45 bytes).
pub type AddrText = Text<INET6_ADDRSTRLEN>;

/// A dotted quad of exactly four decimal parts 0–255 without leading zeros.
pub fn parse_v4(text: &[u8]) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut parts = text.split(|&c| c == b'.');
    for slot in &mut out {
        let p = parts.next()?;
        if p.is_empty() || p.len() > 3 || (p.len() > 1 && p.first() == Some(&b'0')) {
            return None;
        }
        let mut v: u32 = 0;
        for &c in p {
            if !c.is_ascii_digit() {
                return None;
            }
            v = v.wrapping_mul(10).wrapping_add(u32::from(c.wrapping_sub(b'0')));
        }
        *slot = u8::try_from(v).ok()?;
    }
    if parts.next().is_some() {
        return None;
    }
    Some(out)
}

/// An IPv6 text (RFC 4291 §2.2).
pub fn parse_v6(text: &[u8]) -> Option<[u8; 16]> {
    let mut groups = [0u16; 8];
    let mut n = 0usize;
    let mut gap: Option<usize> = None;
    let mut i = 0usize;
    if text.starts_with(b"::") {
        gap = Some(0);
        i = 2;
    } else if text.first() == Some(&b':') {
        return None;
    }
    while i < text.len() {
        let rest = text.get(i..)?;
        let len = rest.iter().position(|&c| c == b':').unwrap_or(rest.len());
        let token = rest.get(..len)?;
        let end = i.checked_add(len)?;
        if token.contains(&b'.') {
            // The IPv4 tail: the last token, in place of two groups.
            if end != text.len() || n > 6 {
                return None;
            }
            let v4 = parse_v4(token)?;
            for pair in v4.chunks_exact(2) {
                let g = groups.get_mut(n)?;
                *g = u16::from_be_bytes([*pair.first()?, *pair.get(1)?]);
                n = n.checked_add(1)?;
            }
            break;
        }
        if token.is_empty() || token.len() > 4 {
            return None;
        }
        let mut v: u16 = 0;
        for &c in token {
            v = v.wrapping_shl(4) | u16::from(hex_value(c)?);
        }
        *groups.get_mut(n)? = v;
        n = n.checked_add(1)?;
        if end == text.len() {
            break;
        }
        // At a colon.
        if text.get(end.checked_add(1)?) == Some(&b':') {
            if gap.is_some() {
                return None;
            }
            gap = Some(n);
            i = end.checked_add(2)?;
        } else {
            i = end.checked_add(1)?;
            if i == text.len() {
                return None;
            }
        }
    }
    let mut full = [0u16; 8];
    match gap {
        Some(at) => {
            if n >= 8 {
                return None;
            }
            let head = groups.get(..at)?;
            let tail = groups.get(at..n)?;
            full.get_mut(..at)?.copy_from_slice(head);
            full.get_mut(8usize.checked_sub(tail.len())?..)?.copy_from_slice(tail);
        }
        None => {
            if n != 8 {
                return None;
            }
            full = groups;
        }
    }
    let mut out = [0u8; 16];
    for (dst, g) in out.chunks_exact_mut(2).zip(full) {
        dst.copy_from_slice(&g.to_be_bytes());
    }
    Some(out)
}

/// The dotted quad of `a`.
pub fn format_v4(a: [u8; 4]) -> AddrText {
    let mut t = AddrText::new();
    push_v4(&mut t, a);
    t
}

fn push_v4(t: &mut AddrText, a: [u8; 4]) {
    for (i, b) in a.iter().enumerate() {
        if i > 0 {
            t.push(b'.');
        }
        t.push_dec(u64::from(*b));
    }
}

/// The RFC 5952 text of `a` (see the module documentation for the mixed forms).
pub fn format_v6(a: [u8; 16]) -> AddrText {
    let mut g = [0u16; 8];
    for (dst, pair) in g.iter_mut().zip(a.chunks_exact(2)) {
        *dst = u16::from_be_bytes([pair.first().copied().unwrap_or(0), pair.get(1).copied().unwrap_or(0)]);
    }
    let mut t = AddrText::new();
    let v4 = [a.get(12).copied().unwrap_or(0), a.get(13).copied().unwrap_or(0), a.get(14).copied().unwrap_or(0), a.get(15).copied().unwrap_or(0)];
    let zero5 = g.iter().take(5).all(|&x| x == 0);
    if zero5 && g.get(5) == Some(&0xffff) {
        t.push_all(b"::ffff:");
        push_v4(&mut t, v4);
        return t;
    }
    if zero5 && g.get(5) == Some(&0) && g.get(6).is_some_and(|&x| x != 0) {
        t.push_all(b"::");
        push_v4(&mut t, v4);
        return t;
    }
    // The longest run of two or more zero groups; the first of equal runs.
    let (mut best_at, mut best_len) = (0usize, 0usize);
    let mut i = 0usize;
    while i < 8 {
        if g.get(i) == Some(&0) {
            let start = i;
            while g.get(i) == Some(&0) {
                i = i.saturating_add(1);
            }
            let len = i.saturating_sub(start);
            if len > best_len {
                (best_at, best_len) = (start, len);
            }
        } else {
            i = i.saturating_add(1);
        }
    }
    let run = if best_len >= 2 { Some((best_at, best_at.saturating_add(best_len))) } else { None };
    let mut i = 0usize;
    while i < 8 {
        if let Some((s, e)) = run {
            if i == s {
                t.push_all(b"::");
                i = e;
                continue;
            }
        }
        if i > 0 && run.is_none_or(|(_, e)| e != i) {
            t.push(b':');
        }
        t.push_hex(u32::from(g.get(i).copied().unwrap_or(0)), false);
        i = i.saturating_add(1);
    }
    t
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn v6(s: &str) -> String {
        String::from_utf8(format_v6(parse_v6(s.as_bytes()).unwrap()).as_bytes().to_vec()).unwrap()
    }

    #[test]
    fn strict_ipv4() {
        assert_eq!(parse_v4(b"1.2.3.4"), Some([1, 2, 3, 4]));
        assert_eq!(parse_v4(b"0.0.0.0"), Some([0; 4]));
        for bad in ["01.2.3.4", "1.2.3", "1.2.3.4.5", "256.1.1.1", "1..3.4", " 1.2.3.4", "1.2.3.4 ", "", "0x1.2.3.4", "1.2.3.-4", "1234.1.1.1"] {
            assert_eq!(parse_v4(bad.as_bytes()), None, "{bad}");
        }
    }

    #[test]
    fn ipv6_forms() {
        assert_eq!(parse_v6(b"::"), Some([0; 16]));
        assert_eq!(parse_v6(b"1:2:3:4:5:6:7::").unwrap()[14..], [0, 0]);
        assert!(parse_v6(b"::0:0:0:0:0:0:0").is_some());
        for bad in [":::", ":1::", "1:::2", "1::2::3", "1:2:3:4:5:6:7:8:9", "1:2:3:4:5:6:7", "1:2:3:4:5:6:7::8", "12345::", "fe80::1%eth0", "1::2:", "::1.2.3", "1:2:3:4:5:6:7:1.2.3.4", "::1.2.3.04", "g::", ""] {
            assert_eq!(parse_v6(bad.as_bytes()), None, "{bad}");
        }
        assert_eq!(parse_v6(b"1:2:3:4:5::1.2.3.4").unwrap()[10..], [0, 0, 1, 2, 3, 4]);
    }

    #[test]
    fn rfc5952_text() {
        for (i, o) in [
            ("::", "::"),
            ("::1", "::1"),
            ("1:0:0:2:0:0:0:3", "1:0:0:2::3"),
            ("1:0:0:2:0:0:3:4", "1::2:0:0:3:4"),
            ("1:0:2:0:3:0:4:0", "1:0:2:0:3:0:4:0"),
            ("::ffff:1.2.3.4", "::ffff:1.2.3.4"),
            ("::ffff:0:0", "::ffff:0.0.0.0"),
            ("::0.0.0.1", "::1"),
            ("::0.0.255.255", "::ffff"),
            ("::1:2", "::0.1.0.2"),
            ("::ffff:0:1.2.3.4", "::ffff:0:102:304"),
            ("1:0:0:1:0:0:0:0", "1:0:0:1::"),
            ("ABCD::EF", "abcd::ef"),
        ] {
            assert_eq!(v6(i), o, "{i}");
        }
    }
}
