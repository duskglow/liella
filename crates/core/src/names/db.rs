//! The record formats of the `files` source (spec §16.3, §16.5, §16.6): one record per line,
//! read as borrowed views over the line.
//!
//! **Lines.** Fields are separated by white space; `#` starts a comment that runs to the end of
//! the line; a blank line, a comment and a line whose fields do not parse are no record (as the
//! reference skips them: netdb.md). The first name is the official one, the rest are aliases.
//!
//! * `hosts(5)`: an address (strict IPv4 or IPv6 text) and names. A line with an address and
//!   no name is a record with an empty name: enumerations return it, name lookups never match
//!   it (D-16.2).
//! * `networks(5)`: a name, a network number, aliases. The number is a dotted text of one to
//!   four parts of one byte each (decimal, `0x` hexadecimal or `0` octal), right-aligned, the
//!   value `inet_network` gives the same text: `127` is `0x7f`, `192.0.2` is `0xc00002`
//!   (D-16.2; the reference left-aligns, `0x7f000000`). A number that does not parse makes the
//!   line no record (the reference reads it as `0xffffffff`).
//! * `services(5)`: a name, `port/protocol`, aliases. The port is decimal, 0–65535: a larger
//!   one makes the line no record (the reference keeps its low 16 bits). A missing
//!   `/protocol` is an empty protocol (observed).
//! * `protocols(5)` and `rpc(5)`: a name, a decimal number, aliases.
//! * `aliases(5)`: `name: member, member, ...`; a line starting with white space continues the
//!   previous entry; `:include:` members are skipped, and an entry left without members is no
//!   record (observed).
//! * `netgroup(5)`: a name, then members: `(host,user,domain)` triples (fields trimmed; an empty
//!   field is a wildcard, `-` stands for no value) and names of other netgroups.

use super::{is_space, Ip};
use crate::net::addr;

/// The fields of a line before its comment, as an iterator that can be cloned (to measure, then
/// copy).
#[derive(Clone, Debug)]
pub struct Words<'a> {
    rest: &'a [u8],
}

impl<'a> Words<'a> {
    /// The words of `line` up to its comment.
    pub fn of(line: &'a [u8]) -> Words<'a> {
        let end = line.iter().position(|&c| c == b'#').unwrap_or(line.len());
        Words { rest: line.get(..end).unwrap_or(&[]) }
    }

    /// No words.
    pub const fn empty() -> Words<'static> {
        Words { rest: &[] }
    }
}

impl<'a> Iterator for Words<'a> {
    type Item = &'a [u8];

    fn next(&mut self) -> Option<&'a [u8]> {
        let start = self.rest.iter().position(|&c| !is_space(c))?;
        let rest = self.rest.get(start..)?;
        let len = rest.iter().position(|&c| is_space(c)).unwrap_or(rest.len());
        let (w, tail) = rest.split_at(len);
        self.rest = tail;
        Some(w)
    }
}

/// The lines of a file's text.
pub fn lines(text: &[u8]) -> impl Iterator<Item = &[u8]> {
    text.split(|&c| c == b'\n')
}

/// A `hosts` record.
#[derive(Clone, Debug)]
pub struct HostLine<'a> {
    /// The address.
    pub addr: Ip,
    /// The official name (empty for a line without names).
    pub name: &'a [u8],
    /// The aliases.
    pub aliases: Words<'a>,
}

impl HostLine<'_> {
    /// Whether `name` is the line's official name or an alias (ASCII case ignored).
    pub fn names(&self, name: &[u8]) -> bool {
        !name.is_empty() && (self.name.eq_ignore_ascii_case(name) || self.aliases.clone().any(|a| a.eq_ignore_ascii_case(name)))
    }
}

/// The `hosts` record on `line`.
pub fn host_line(line: &[u8]) -> Option<HostLine<'_>> {
    let mut w = Words::of(line);
    let a = w.next()?;
    let addr = match addr::parse_v4(a) {
        Some(v) => Ip::V4(v),
        None => Ip::V6(addr::parse_v6(a)?),
    };
    let name = w.next().unwrap_or(&[]);
    Some(HostLine { addr, name, aliases: w })
}

/// A record with a name, aliases and a number (`networks`, `protocols`, `rpc`).
#[derive(Clone, Debug)]
pub struct Numbered<'a, N> {
    /// The official name.
    pub name: &'a [u8],
    /// The number.
    pub number: N,
    /// The aliases.
    pub aliases: Words<'a>,
}

impl<N> Numbered<'_, N> {
    /// Whether `name` is the official name or an alias, by `eq`.
    pub fn names(&self, name: &[u8], eq: fn(&[u8], &[u8]) -> bool) -> bool {
        eq(self.name, name) || self.aliases.clone().any(|a| eq(a, name))
    }
}

/// Exact comparison.
pub fn same(a: &[u8], b: &[u8]) -> bool {
    a == b
}

/// Comparison ignoring ASCII case.
pub fn same_nocase(a: &[u8], b: &[u8]) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// One part of a network number: decimal, `0x` hexadecimal or `0` octal, at most 255.
fn net_part(p: &[u8]) -> Option<u32> {
    let (digits, radix) = match p {
        [b'0', b'x' | b'X', rest @ ..] => (rest, 16),
        [b'0', rest @ ..] if !rest.is_empty() => (rest, 8),
        _ => (p, 10),
    };
    if digits.is_empty() || digits.len() > 8 {
        return None;
    }
    let mut v: u32 = 0;
    for &c in digits {
        let d = char::from(c).to_digit(radix)?;
        v = v.checked_mul(radix)?.checked_add(d)?;
    }
    (v <= 255).then_some(v)
}

/// A network number (see the module documentation): right-aligned.
pub fn network_number(text: &[u8]) -> Option<u32> {
    let mut v: u32 = 0;
    let mut n = 0usize;
    for p in text.split(|&c| c == b'.') {
        n = n.checked_add(1)?;
        if n > 4 {
            return None;
        }
        v = v.checked_shl(8)?.checked_add(net_part(p)?)?;
    }
    Some(v)
}

/// The `networks` record on `line`.
pub fn net_line(line: &[u8]) -> Option<Numbered<'_, u32>> {
    let mut w = Words::of(line);
    let name = w.next()?;
    let number = network_number(w.next()?)?;
    Some(Numbered { name, number, aliases: w })
}

/// A decimal number of digits only, within `max`.
fn decimal(t: &[u8], max: u32) -> Option<u32> {
    if t.is_empty() || t.len() > 10 {
        return None;
    }
    let mut v: u64 = 0;
    for &c in t {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add(u64::from(c.wrapping_sub(b'0')))?;
    }
    u32::try_from(v).ok().filter(|&v| v <= max)
}

/// A port number text (decimal digits only, 0–65535): the numeric service of `getaddrinfo` and
/// the ports of `services` lines.
pub fn port(text: &[u8]) -> Option<u16> {
    decimal(text, 65535).and_then(|v| u16::try_from(v).ok())
}

/// A `services` record.
#[derive(Clone, Debug)]
pub struct ServLine<'a> {
    /// The official name.
    pub name: &'a [u8],
    /// The port.
    pub port: u16,
    /// The protocol (empty when the line names none).
    pub proto: &'a [u8],
    /// The aliases.
    pub aliases: Words<'a>,
}

impl ServLine<'_> {
    /// Whether `name` is the official name or an alias (case-sensitive, as the reference).
    pub fn names(&self, name: &[u8]) -> bool {
        self.name == name || self.aliases.clone().any(|a| a == name)
    }
}

/// The `services` record on `line`.
pub fn serv_line(line: &[u8]) -> Option<ServLine<'_>> {
    let mut w = Words::of(line);
    let name = w.next()?;
    let pp = w.next()?;
    let (p, proto) = match pp.iter().position(|&c| c == b'/') {
        Some(i) => (pp.get(..i)?, pp.get(i.checked_add(1)?..)?),
        None => (pp, &[][..]),
    };
    Some(ServLine { name, port: port(p)?, proto, aliases: w })
}

/// The `protocols` record on `line`.
pub fn numbered_line(line: &[u8]) -> Option<Numbered<'_, i32>> {
    let mut w = Words::of(line);
    let name = w.next()?;
    let number = i32::try_from(decimal(w.next()?, u32::try_from(i32::MAX).unwrap_or(0))?).ok()?;
    Some(Numbered { name, number, aliases: w })
}

/// The number of an `rpc` line, as the reference reads it (probed): decimal digits, any number of
/// leading zeros, after an optional `+` (or a `-` before a zero value), up to 4294967295; the value
/// is kept as the `int` it converts to, so 2147483648 and above wrap to negative numbers.
fn rpc_number(t: &[u8]) -> Option<i32> {
    let (minus, digits) = match t.split_first() {
        Some((b'+', d)) => (false, d),
        Some((b'-', d)) => (true, d),
        _ => (false, t),
    };
    if digits.is_empty() {
        return None;
    }
    let mut v: u32 = 0;
    for &c in digits {
        if !c.is_ascii_digit() {
            return None;
        }
        v = v.checked_mul(10)?.checked_add(u32::from(c.wrapping_sub(b'0')))?;
    }
    if minus && v != 0 {
        return None;
    }
    Some(i32::from_ne_bytes(v.to_ne_bytes()))
}

/// The `rpc` record on `line`.
pub fn rpc_line(line: &[u8]) -> Option<Numbered<'_, i32>> {
    let mut w = Words::of(line);
    let name = w.next()?;
    let number = rpc_number(w.next()?)?;
    Some(Numbered { name, number, aliases: w })
}

// ------------------------------------------------------------------------------ aliases

/// The members of an `aliases` entry: comma-separated, trimmed, `:include:` and empty members
/// skipped. May span lines.
#[derive(Clone, Debug)]
pub struct Members<'a> {
    text: &'a [u8],
}

impl<'a> Members<'a> {
    /// The members.
    pub fn iter(&self) -> impl Iterator<Item = &'a [u8]> + Clone + use<'a> {
        self.text.split(|&c| c == b',').map(trim).filter(|m| !m.is_empty() && !m.starts_with(b":include:"))
    }
}

/// `t` without white space around it.
pub fn trim(t: &[u8]) -> &[u8] {
    let start = t.iter().position(|&c| !is_space(c)).unwrap_or(t.len());
    let end = t.iter().rposition(|&c| !is_space(c)).map_or(start, |e| e.saturating_add(1));
    t.get(start..end).unwrap_or(&[])
}

/// An `aliases` entry.
#[derive(Clone, Debug)]
pub struct Alias<'a> {
    /// The alias.
    pub name: &'a [u8],
    /// Its members.
    pub members: Members<'a>,
}

/// The `aliases` entry starting at `at` in `text` (the offset of a line), and the offset after
/// it: `Ok(None)` past the end, `Err(next)` for lines that are no entry (skipped up to `next`).
pub fn alias_at(text: &[u8], at: usize) -> Option<(Option<Alias<'_>>, usize)> {
    let rest = text.get(at..)?;
    if rest.is_empty() {
        return None;
    }
    // The entry's extent: its line and the continuation lines (starting with white space).
    let mut end = rest.iter().position(|&c| c == b'\n').map_or(rest.len(), |i| i.saturating_add(1));
    while let Some(&c) = rest.get(end) {
        if c == b'\n' || !is_space(c) {
            break;
        }
        end = rest.get(end..)?.iter().position(|&c| c == b'\n').map_or(rest.len(), |i| end.saturating_add(i).saturating_add(1));
    }
    let next = at.checked_add(end)?;
    let body = rest.get(..end)?;
    let first = body.iter().position(|&c| !is_space(c));
    let entry = (|| {
        let first = first?;
        if body.get(first) == Some(&b'#') {
            return None;
        }
        // Comment lines inside the entry are not members.
        let colon = body.iter().position(|&c| c == b':')?;
        let name = trim(body.get(..colon)?);
        let members = Members { text: body.get(colon.checked_add(1)?..)? };
        (!name.is_empty() && members.iter().next().is_some()).then_some(Alias { name, members })
    })();
    Some((entry, next))
}

/// Every `aliases` entry of `text`, in order.
pub fn aliases(text: &[u8]) -> impl Iterator<Item = Alias<'_>> {
    let mut at = 0usize;
    core::iter::from_fn(move || {
        loop {
            let (e, next) = alias_at(text, at)?;
            at = next;
            if e.is_some() {
                return e;
            }
        }
    })
}

// ------------------------------------------------------------------------------ netgroups

/// A netgroup member.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NetMember<'a> {
    /// `(host,user,domain)`: `None` for an empty field (any value).
    Triple([Option<&'a [u8]>; 3]),
    /// Another netgroup.
    Group(&'a [u8]),
}

/// The members of the netgroup line `line` if it defines `group` (case-sensitive), as the line
/// lists them; an invalid triple (not three fields, unclosed) is skipped.
pub fn netgroup<'a>(line: &'a [u8], group: &[u8]) -> Option<impl Iterator<Item = NetMember<'a>> + use<'a>> {
    let line = line.get(..line.iter().position(|&c| c == b'#').unwrap_or(line.len()))?;
    let start = line.iter().position(|&c| !is_space(c))?;
    let rest = line.get(start..)?;
    let len = rest.iter().position(|&c| is_space(c)).unwrap_or(rest.len());
    if rest.get(..len)? != group {
        return None;
    }
    let mut body = rest.get(len..)?;
    Some(core::iter::from_fn(move || {
        loop {
            let s = body.iter().position(|&c| !is_space(c))?;
            body = body.get(s..)?;
            if body.first() == Some(&b'(') {
                let close = body.iter().position(|&c| c == b')');
                let inner = body.get(1..close.unwrap_or(body.len()))?;
                body = body.get(close.map_or(body.len(), |c| c.saturating_add(1))..)?;
                if close.is_none() {
                    continue;
                }
                let mut f = inner.split(|&c| c == b',').map(trim);
                let (h, u, d) = (f.next()?, f.next(), f.next());
                let (Some(u), Some(d), None) = (u, d, f.next()) else { continue };
                let opt = |x: &'a [u8]| (!x.is_empty()).then_some(x);
                return Some(NetMember::Triple([opt(h), opt(u), opt(d)]));
            }
            let n = body.iter().position(|&c| is_space(c) || c == b'(').unwrap_or(body.len());
            let (name, tail) = body.split_at(n);
            body = tail;
            return Some(NetMember::Group(name));
        }
    }))
}

/// Whether a triple's field `have` matches the value asked for (`None`: anything). An empty
/// field matches every value; `-` matches none; hosts and domains match without regard to
/// ASCII case, users exactly (observed).
pub fn field_matches(have: Option<&[u8]>, want: Option<&[u8]>, nocase: bool) -> bool {
    match (have, want) {
        (_, None) | (None, _) => true,
        (Some(b"-"), Some(_)) => false,
        (Some(h), Some(w)) => {
            if nocase {
                h.eq_ignore_ascii_case(w)
            } else {
                h == w
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn formats() {
        let h = host_line(b"  192.0.2.1   alpha alpha.example.org   a1  # c");
        assert!(h.is_some(), "hosts line: blanks split the address from the names and a trailing comment is stripped");
        let h = h.unwrap();
        assert_eq!((h.addr, h.name), (Ip::V4([192, 0, 2, 1]), &b"alpha"[..]), "hosts line: aliases are split from the name");
        assert!(h.names(b"A1") && !h.names(b"c") && !h.names(b""), "hosts line: an alias matches, a comment is stripped");
        assert!(host_line(b"300.0.0.1 bad").is_none());
        assert_eq!(host_line(b"192.0.2.6").unwrap().name, b"");
        assert_eq!(network_number(b"127"), Some(0x7f));
        assert_eq!(network_number(b"192.0.2"), Some(0xc00002));
        assert_eq!(network_number(b"0x7f.1"), Some(0x7f01));
        assert_eq!(network_number(b"notanumber"), None);
        assert_eq!(network_number(b"1.2.3.4.5"), None);
        let s = serv_line(b"http 80/tcp www www-http").unwrap();
        assert_eq!((s.name, s.port, s.proto, s.aliases.collect::<Vec<_>>()), (&b"http"[..], 80, &b"tcp"[..], vec![&b"www"[..], b"www-http"]));
        assert!(serv_line(b"badport 70000/tcp").is_none());
        assert_eq!(serv_line(b"noproto 9999").unwrap().proto, b"");
        assert_eq!(numbered_line(b"tcp 6 TCP").unwrap().number, 6);
        assert!(numbered_line(b"bad x").is_none());
        // rpc numbers (probed): `+`, `-0`, leading zeros, u32 values wrapping to int.
        let n = |l: &[u8]| rpc_line(l).map(|p| p.number);
        assert_eq!((n(b"a +5"), n(b"a -0"), n(b"a 0000000000000100021")), (Some(5), Some(0), Some(100_021)));
        assert_eq!((n(b"a 2147483648"), n(b"a 4294967295")), (Some(i32::MIN), Some(-1)));
        for bad in [&b"a -1"[..], b"a 4294967296", b"a ++5", b"a +", b"a -", b"a 0x10", b"a 1e3", b"a 5x", b"a"] {
            assert_eq!(n(bad), None, "{}", String::from_utf8_lossy(bad));
        }
    }

    #[test]
    fn alias_entries() {
        let f = b"postmaster: root\nroot: alice, bob\n# c\nlist: :include:/etc/listfile\nmulti: a,\n  b,\n  c\n";
        let v: Vec<_> = aliases(f).map(|a| (a.name, a.members.iter().collect::<Vec<_>>())).collect();
        assert_eq!(v, vec![(&b"postmaster"[..], vec![&b"root"[..]]), (b"root", vec![b"alice", b"bob"]), (b"multi", vec![b"a", b"b", b"c"])]);
    }

    #[test]
    fn netgroups() {
        let m: Vec<_> = netgroup(b"trusted (alpha,alice,example.org) (beta,-,) nested (x,y)", b"trusted").unwrap().collect();
        assert_eq!(m.len(), 3);
        assert_eq!(m[1], NetMember::Triple([Some(b"beta"), Some(b"-"), None]));
        assert_eq!(m[2], NetMember::Group(b"nested"));
        let m: Vec<_> = netgroup(b"ng2 (h2 , u2 , d2 )", b"ng2").unwrap().collect();
        assert_eq!(m, vec![NetMember::Triple([Some(b"h2"), Some(b"u2"), Some(b"d2")])]);
        assert!(netgroup(b"ng x", b"NG").is_none());
        assert!(field_matches(Some(b"H1"), Some(b"h1"), true) && !field_matches(Some(b"-"), Some(b"x"), false) && field_matches(Some(b"-"), None, false));
    }
}
