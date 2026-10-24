//! DNS messages (RFC 1035 §4; spec §16.4 `res_mkquery`, `ns_initparse`, `ns_parserr`,
//! `ns_skiprr`, `ns_msg_getflag`, `res_nameinquery`, `res_queriesmatch`; D-16.3).
//!
//! **Queries** ([`mkquery`], observed): the opcodes `QUERY` (0) and `NOTIFY` (4) only, any other
//! is an error; the flags are RD (when recursion is asked for) and AD; one question; `NOTIFY`
//! with data adds one additional record whose owner is the data read as a text name, of type
//! `NULL`, the question's class, TTL 0 and no data. The ID is the caller's. EDNS (RFC 6891) is
//! added by the resolver ([`add_opt`]), not by `res_mkquery`.
//!
//! **Parsing** ([`parse`]): a message must be at least a header and its sections must end
//! exactly at its end (`EMSGSIZE` otherwise). Records are read by section and index ([`rr`]);
//! an index or section out of range is `ENODEV`.

use crate::error::{Errno, Result};

use super::name::{self, Out, MAXCDNAME};

const E: Errno = Errno::EMSGSIZE;

/// The bytes of a header.
pub const HFIXEDSZ: usize = 12;
/// `QUERY`.
pub const OP_QUERY: u8 = 0;
/// `NOTIFY`.
pub const OP_NOTIFY: u8 = 4;
/// `UPDATE` (its sections have other names).
pub const OP_UPDATE: u8 = 5;

/// Flag bits of the header's second word.
pub mod flag {
    /// QR: a response.
    pub const QR: u16 = 0x8000;
    /// AA: authoritative.
    pub const AA: u16 = 0x0400;
    /// TC: truncated.
    pub const TC: u16 = 0x0200;
    /// RD: recursion desired.
    pub const RD: u16 = 0x0100;
    /// RA: recursion available.
    pub const RA: u16 = 0x0080;
    /// Z: reserved.
    pub const Z: u16 = 0x0040;
    /// AD: authentic data.
    pub const AD: u16 = 0x0020;
    /// CD: checking disabled.
    pub const CD: u16 = 0x0010;
}

/// A message's header.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Header {
    /// The ID.
    pub id: u16,
    /// The flags, opcode and response code.
    pub flags: u16,
    /// The record counts of the four sections.
    pub counts: [u16; 4],
}

impl Header {
    /// The header of `msg`.
    pub fn of(msg: &[u8]) -> Option<Header> {
        let h = msg.get(..HFIXEDSZ)?;
        let word = |i: usize| -> Option<u16> { Some(u16::from_be_bytes([*h.get(i)?, *h.get(i.checked_add(1)?)?])) };
        Some(Header { id: word(0)?, flags: word(2)?, counts: [word(4)?, word(6)?, word(8)?, word(10)?] })
    }

    /// The 12 bytes.
    pub fn bytes(&self) -> [u8; HFIXEDSZ] {
        let mut b = [0u8; HFIXEDSZ];
        let words = [self.id, self.flags, self.counts[0], self.counts[1], self.counts[2], self.counts[3]];
        for (chunk, w) in b.chunks_exact_mut(2).zip(words) {
            chunk.copy_from_slice(&w.to_be_bytes());
        }
        b
    }

    /// The response code.
    pub fn rcode(&self) -> u8 {
        u8::try_from(self.flags & 0xf).unwrap_or(0)
    }

    /// The opcode.
    pub fn opcode(&self) -> u8 {
        u8::try_from(self.flags >> 11 & 0xf).unwrap_or(0)
    }

    /// Whether `bit` is set.
    pub fn has(&self, bit: u16) -> bool {
        self.flags & bit != 0
    }
}

/// `ns_msg_getflag(msg, flag)` for the flag numbers of `ns_flag` (qr, opcode, aa, tc, rd, ra,
/// z, ad, cd, rcode); any other number is 0.
pub fn getflag(flags: u16, which: i32) -> i32 {
    let (mask, shift): (u16, u32) = match which {
        0 => (0x8000, 15),
        1 => (0x7800, 11),
        2 => (0x0400, 10),
        3 => (0x0200, 9),
        4 => (0x0100, 8),
        5 => (0x0080, 7),
        6 => (0x0040, 6),
        7 => (0x0020, 5),
        8 => (0x0010, 4),
        9 => (0x000f, 0),
        _ => return 0,
    };
    i32::from((flags & mask) >> shift)
}

/// Build a query into `buf` (see the module documentation): its length. `rd`: set RD.
#[allow(clippy::too_many_arguments)] // the C call's arguments
pub fn mkquery(op: u8, name: &[u8], class: u16, rtype: u16, data: Option<&[u8]>, buf: &mut [u8], id: u16, rd: bool) -> Result<usize> {
    if op != OP_QUERY && op != OP_NOTIFY {
        return Err(Errno::EINVAL);
    }
    let mut wire = [0u8; MAXCDNAME + 1];
    let (wl, _) = name::pton(name, &mut wire)?;
    let notify_rr = match (op, data) {
        (OP_NOTIFY, Some(d)) => {
            let text = d.get(..d.iter().position(|&c| c == 0).unwrap_or(d.len())).unwrap_or(&[]);
            let mut w2 = [0u8; MAXCDNAME + 1];
            let (n2, _) = name::pton(text, &mut w2)?;
            Some((w2, n2))
        }
        _ => None,
    };
    let mut flags = u16::from(op) << 11 | flag::AD;
    if rd {
        flags |= flag::RD;
    }
    let h = Header { id, flags, counts: [1, 0, 0, u16::from(notify_rr.is_some())] };
    if buf.len() < HFIXEDSZ {
        return Err(E);
    }
    let mut names = [0usize; 4];
    let limit = buf.len();
    let mut at = HFIXEDSZ;
    let mut comp = name::Compress { names: &mut names, len: 0, record: true };
    at = at.checked_add(name::pack(wire.get(..wl).unwrap_or(&[]), buf, at, limit.saturating_sub(at), Some(&mut comp))?).ok_or(E)?;
    let mut o = Out::new(buf.get_mut(at..).ok_or(E)?);
    o.push_all(&rtype.to_be_bytes())?;
    o.push_all(&class.to_be_bytes())?;
    at = at.checked_add(4).ok_or(E)?;
    if let Some((w2, n2)) = notify_rr {
        at = at.checked_add(name::pack(w2.get(..n2).unwrap_or(&[]), buf, at, limit.saturating_sub(at), Some(&mut comp))?).ok_or(E)?;
        let mut o = Out::new(buf.get_mut(at..).ok_or(E)?);
        o.push_all(&10u16.to_be_bytes())?;
        o.push_all(&class.to_be_bytes())?;
        o.push_all(&[0, 0, 0, 0, 0, 0])?;
        at = at.checked_add(10).ok_or(E)?;
    }
    buf.get_mut(..HFIXEDSZ).ok_or(E)?.copy_from_slice(&h.bytes());
    Ok(at)
}

/// Append an EDNS `OPT` record (RFC 6891) to the query `msg` of `len` bytes in `buf`: UDP
/// payload `size`, the DO bit when `dnssec`. The new length.
pub fn add_opt(buf: &mut [u8], len: usize, size: u16, dnssec: bool) -> Result<usize> {
    let mut h = Header::of(buf.get(..len).ok_or(E)?).ok_or(E)?;
    let end = len.checked_add(11).ok_or(E)?;
    let rr = buf.get_mut(len..end).ok_or(E)?;
    let s = size.to_be_bytes();
    let flags: u8 = if dnssec { 0x80 } else { 0 };
    rr.copy_from_slice(&[0, 0, 41, s[0], s[1], 0, 0, flags, 0, 0, 0]);
    h.counts[3] = h.counts[3].saturating_add(1);
    buf.get_mut(..HFIXEDSZ).ok_or(E)?.copy_from_slice(&h.bytes());
    Ok(end)
}

/// A parsed message: the header and where each section starts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Parsed {
    /// The header.
    pub header: Header,
    /// The offset of each section, `None` for an empty one.
    pub sections: [Option<usize>; 4],
}

/// The bytes the records (or, for section 0, questions) take from `at`: `count` of them.
pub fn skiprr(msg: &[u8], at: usize, section: usize, count: usize) -> Result<usize> {
    let mut p = at;
    for _ in 0..count {
        p = p.checked_add(name::skip(msg.get(p..).ok_or(E)?)?).ok_or(E)?;
        if section == 0 {
            p = p.checked_add(4).ok_or(E)?;
        } else {
            let fixed = msg.get(p..p.checked_add(10).ok_or(E)?).ok_or(E)?;
            let rdlen = usize::from(u16::from_be_bytes([*fixed.get(8).ok_or(E)?, *fixed.get(9).ok_or(E)?]));
            p = p.checked_add(10).ok_or(E)?.checked_add(rdlen).ok_or(E)?;
        }
        if p > msg.len() {
            return Err(E);
        }
    }
    p.checked_sub(at).ok_or(E)
}

/// `ns_initparse`: the sections of `msg`, which must end exactly at its end.
pub fn parse(msg: &[u8]) -> Result<Parsed> {
    let header = Header::of(msg).ok_or(E)?;
    let mut at = HFIXEDSZ;
    let mut sections = [None; 4];
    for (s, slot) in sections.iter_mut().enumerate() {
        let count = usize::from(header.counts.get(s).copied().unwrap_or(0));
        if count == 0 {
            continue;
        }
        *slot = Some(at);
        at = at.checked_add(skiprr(msg, at, s, count)?).ok_or(E)?;
    }
    if at != msg.len() {
        return Err(E);
    }
    Ok(Parsed { header, sections })
}

/// A record (or question) of a parsed message.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rr {
    /// The owner, wire form.
    pub name: [u8; MAXCDNAME + 1],
    /// Its length.
    pub name_len: usize,
    /// The type.
    pub rtype: u16,
    /// The class.
    pub class: u16,
    /// The TTL (0 for a question).
    pub ttl: u32,
    /// The data's offset in the message (`None` for a question).
    pub rdata: Option<usize>,
    /// The data's length.
    pub rdlen: u16,
    /// The offset after the record.
    pub next: usize,
}

impl Rr {
    /// The owner's wire form.
    pub fn owner(&self) -> &[u8] {
        self.name.get(..self.name_len).unwrap_or(&[])
    }

    /// The data, within `msg`.
    pub fn data<'a>(&self, msg: &'a [u8]) -> &'a [u8] {
        match self.rdata {
            Some(at) => msg.get(at..at.saturating_add(usize::from(self.rdlen))).unwrap_or(&[]),
            None => &[],
        }
    }
}

/// The record at `at` of section `section` (0: a question).
pub fn rr_at(msg: &[u8], at: usize, section: usize) -> Result<Rr> {
    let mut name = [0u8; MAXCDNAME + 1];
    let (name_len, used) = name::unpack(msg, at, &mut name)?;
    let p = at.checked_add(used).ok_or(E)?;
    let word = |i: usize| -> Result<u16> { Ok(u16::from_be_bytes([*msg.get(i).ok_or(E)?, *msg.get(i.checked_add(1).ok_or(E)?).ok_or(E)?])) };
    let rtype = word(p)?;
    let class = word(p.checked_add(2).ok_or(E)?)?;
    if section == 0 {
        return Ok(Rr { name, name_len, rtype, class, ttl: 0, rdata: None, rdlen: 0, next: p.checked_add(4).ok_or(E)? });
    }
    let ttl = u32::from(word(p.checked_add(4).ok_or(E)?)?) << 16 | u32::from(word(p.checked_add(6).ok_or(E)?)?);
    let rdlen = word(p.checked_add(8).ok_or(E)?)?;
    let rd = p.checked_add(10).ok_or(E)?;
    let next = rd.checked_add(usize::from(rdlen)).ok_or(E)?;
    if next > msg.len() {
        return Err(E);
    }
    Ok(Rr { name, name_len, rtype, class, ttl, rdata: Some(rd), rdlen, next })
}

/// `ns_parserr`: record `index` of section `section`.
pub fn rr(msg: &[u8], p: &Parsed, section: usize, index: usize) -> Result<Rr> {
    let count = usize::from(*p.header.counts.get(section).ok_or(Errno::ENODEV)?);
    if index >= count {
        return Err(Errno::ENODEV);
    }
    let start = p.sections.get(section).copied().flatten().ok_or(Errno::ENODEV)?;
    let at = start.checked_add(skiprr(msg, start, section, index)?).ok_or(E)?;
    rr_at(msg, at, section)
}

/// `res_nameinquery(name, type, class, msg)`: whether the questions of `msg` include this one
/// (names compared as names); `Err` for a message that does not parse.
pub fn name_in_query(name: &[u8], rtype: u16, class: u16, msg: &[u8]) -> Result<bool> {
    let mut want = [0u8; MAXCDNAME + 1];
    let (wl, _) = name::pton(name, &mut want)?;
    let want = want.get(..wl).unwrap_or(&[]);
    let h = Header::of(msg).ok_or(E)?;
    let mut at = HFIXEDSZ;
    for _ in 0..h.counts[0] {
        let q = rr_at(msg, at, 0)?;
        if q.rtype == rtype && q.class == class && name::wire_eq(q.owner(), want) {
            return Ok(true);
        }
        at = q.next;
    }
    Ok(false)
}

/// `res_queriesmatch(q1, q2)`: whether the two messages ask the same questions (in any order;
/// IDs and flags are not compared); `Err` when either does not parse.
pub fn queries_match(a: &[u8], b: &[u8]) -> Result<bool> {
    let (ha, hb) = (Header::of(a).ok_or(E)?, Header::of(b).ok_or(E)?);
    if ha.counts[0] != hb.counts[0] {
        return Ok(false);
    }
    let mut at = HFIXEDSZ;
    for _ in 0..ha.counts[0] {
        let q = rr_at(a, at, 0)?;
        let mut found = false;
        let mut bt = HFIXEDSZ;
        for _ in 0..hb.counts[0] {
            let r = rr_at(b, bt, 0)?;
            if r.rtype == q.rtype && r.class == q.class && name::wire_eq(r.owner(), q.owner()) {
                found = true;
            }
            bt = r.next;
        }
        if !found {
            return Ok(false);
        }
        at = q.next;
    }
    Ok(true)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn query() {
        let mut b = [0u8; 64];
        let n = mkquery(0, b"www.example.com", 1, 1, None, &mut b, 0x1234, true).unwrap();
        assert_eq!(n, 33);
        assert_eq!(&b[2..4], &[0x01, 0x20]);
        assert!(mkquery(1, b"a", 1, 1, None, &mut b, 0, true).is_err());
        assert!(mkquery(0, b"a.example", 1, 1, None, &mut b[..26], 0, true).is_err());
        assert_eq!(mkquery(0, b"a.example", 1, 1, None, &mut b[..27], 0, true).unwrap(), 27);
        let p = parse(&b[..27]).unwrap();
        assert_eq!(p.header.counts, [1, 0, 0, 0]);
        assert!(parse(&b[..28]).is_err());
        assert!(name_in_query(b"A.example.", 1, 1, &b[..27]).unwrap());
        let n = mkquery(4, b"a.example", 1, 6, Some(b"\x03abc\x00"), &mut b, 0, true).unwrap();
        assert_eq!(n, 43);
    }
}
