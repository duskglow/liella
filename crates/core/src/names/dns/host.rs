//! Host answers for the `dns` source (spec §16.2, §16.3 through §16.4; D-16.3): the addresses
//! (`A`, `AAAA`) or names (`PTR`) a response gives for the name asked.
//!
//! **Chains** (RFC 1034 §3.6.2, observed): starting from the name asked, a `CNAME` record whose
//! owner is the current name makes its owner an alias and its target the current name; records
//! of the type asked count only when their owner is the current name (ASCII case ignored), so
//! records for other names in the answer are ignored. The official name is the last name of
//! the chain: the name as asked when there is no `CNAME` (its case kept, observed), else the
//! last target as the answer spells it.
//!
//! **Outcomes.** NXDOMAIN: not found. NOERROR without a record of the type: no data. Any other
//! response code is the caller's (the client turns them into temporary failures).

use crate::error::{Errno, Result};
use crate::mem::TryVec;
use crate::names::Ip;

use super::msg::{self, Parsed};
use super::name::{self, MAXCDNAME};
use super::{rcode, CLASS_IN, T_A, T_AAAA, T_CNAME, T_PTR};

/// A host found in an answer.
#[derive(Debug)]
pub struct Found {
    /// The official name (text).
    pub name: TryVec<u8>,
    /// The aliases (text), in chain order.
    pub aliases: TryVec<TryVec<u8>>,
    /// The addresses (`A`/`AAAA`), in answer order.
    pub addrs: TryVec<Ip>,
    /// The names of `PTR` records after the first.
    pub more_names: TryVec<TryVec<u8>>,
}

impl Default for Found {
    fn default() -> Found {
        Found { name: TryVec::new(), aliases: TryVec::new(), addrs: TryVec::new(), more_names: TryVec::new() }
    }
}

/// What an answer says about the name.
#[derive(Debug)]
pub enum Outcome {
    /// Records of the type.
    Found(Found),
    /// The name exists without records of the type.
    NoData,
    /// The name does not exist.
    NotFound,
}

fn text_of(wire: &[u8]) -> Result<TryVec<u8>> {
    let mut t = [0u8; name::MAXDNAME];
    let n = name::expand_text(wire, &mut t)?;
    TryVec::from_slice(t.get(..n.saturating_sub(1)).unwrap_or(&[]))
}

/// The records of type `want` for the text name `asked` in the response `msg` (see the module
/// documentation). `want` is `T_A`, `T_AAAA` or `T_PTR`.
pub fn read(msg: &[u8], asked: &[u8], want: u16) -> Result<Outcome> {
    let p: Parsed = msg::parse(msg)?;
    match p.header.rcode() {
        rcode::NXDOMAIN => return Ok(Outcome::NotFound),
        rcode::NOERROR => {}
        _ => return Err(Errno::EAGAIN),
    }
    let mut cur = [0u8; MAXCDNAME + 1];
    let (mut cur_len, _) = name::pton(asked, &mut cur)?;
    let mut out = Found::default();
    let mut chained = false;
    let count = usize::from(p.header.counts[1]);
    let mut rrs: TryVec<msg::Rr> = TryVec::new();
    for i in 0..count {
        rrs.push(msg::rr(msg, &p, 1, i)?)?;
    }
    // Follow the chain (the records may come in any order; each CNAME is used once).
    let mut used: TryVec<bool> = TryVec::new();
    used.resize(rrs.len(), false)?;
    loop {
        let curw = cur.get(..cur_len).unwrap_or(&[]);
        let next = rrs.iter().zip(used.iter()).position(|(r, &u)| !u && r.rtype == T_CNAME && r.class == CLASS_IN && name::wire_eq(r.owner(), curw));
        let Some(i) = next else { break };
        if let Some(u) = used.get_mut(i) {
            *u = true;
        }
        let r = rrs.get(i).ok_or(Errno::EINVAL)?;
        out.aliases.push(if chained { text_of(curw)? } else { TryVec::from_slice(strip_dot(asked))? })?;
        let mut t = [0u8; MAXCDNAME + 1];
        let rd = r.rdata.ok_or(Errno::EMSGSIZE)?;
        let (n, _) = name::unpack(msg, rd, &mut t)?;
        cur = t;
        cur_len = n;
        chained = true;
    }
    let curw = cur.get(..cur_len).unwrap_or(&[]);
    out.name = if chained { text_of(curw)? } else { TryVec::from_slice(strip_dot(asked))? };
    let mut first_ptr = true;
    for r in rrs.iter().filter(|r| r.rtype == want && r.class == CLASS_IN && name::wire_eq(r.owner(), curw)) {
        let d = r.data(msg);
        match want {
            T_A => {
                if let Ok(a) = <[u8; 4]>::try_from(d) {
                    out.addrs.push(Ip::V4(a))?;
                }
            }
            T_AAAA => {
                if let Ok(a) = <[u8; 16]>::try_from(d) {
                    out.addrs.push(Ip::V6(a))?;
                }
            }
            T_PTR => {
                let mut t = [0u8; MAXCDNAME + 1];
                let (n, _) = name::unpack(msg, r.rdata.ok_or(Errno::EMSGSIZE)?, &mut t)?;
                let text = text_of(t.get(..n).unwrap_or(&[]))?;
                if first_ptr {
                    out.name = text;
                    first_ptr = false;
                } else {
                    out.more_names.push(text)?;
                }
            }
            _ => {}
        }
    }
    if out.addrs.is_empty() && first_ptr {
        return Ok(Outcome::NoData);
    }
    Ok(Outcome::Found(out))
}

/// How long the answer `msg` may be kept (D-16.10), in seconds: with answer records, the
/// smallest TTL among them (RFC 2181 §5.2: a set's records share one; taking the smallest of
/// all, the chain's `CNAME`s included, never keeps any of them too long); without, the
/// negative-caching time of RFC 2308 §5: the smaller of the authority section's `SOA` record's
/// TTL and its `MINIMUM` field. `None`: nothing says (a negative answer without an `SOA` is not
/// cached, RFC 2308 §5), or the message does not parse.
pub fn ttl(msg: &[u8]) -> Option<u32> {
    let p = msg::parse(msg).ok()?;
    let answers = usize::from(p.header.counts[1]);
    if answers > 0 {
        let mut least: Option<u32> = None;
        for i in 0..answers {
            let r = msg::rr(msg, &p, 1, i).ok()?;
            least = Some(least.map_or(r.ttl, |l| l.min(r.ttl)));
        }
        return least;
    }
    for i in 0..usize::from(p.header.counts[2]) {
        let r = msg::rr(msg, &p, 2, i).ok()?;
        if r.rtype == super::T_SOA && r.class == CLASS_IN {
            let d = r.data(msg);
            let min = d.len().checked_sub(4).and_then(|at| d.get(at..)).and_then(|b| <[u8; 4]>::try_from(b).ok()).map(u32::from_be_bytes)?;
            return Some(r.ttl.min(min));
        }
    }
    None
}

fn strip_dot(t: &[u8]) -> &[u8] {
    match t.strip_suffix(b".") {
        Some(s) if !s.is_empty() && !s.ends_with(b"\\") => s,
        _ => t,
    }
}

/// The reverse-lookup name of an address (RFC 1035 §3.5 `in-addr.arpa`, RFC 3596 §2.5
/// `ip6.arpa` nibbles).
pub fn reverse_name(ip: &Ip) -> Result<TryVec<u8>> {
    let mut t = TryVec::new();
    match ip {
        Ip::V4(a) => {
            for &b in a.iter().rev() {
                super::text::push_dec(&mut t, u64::from(b))?;
                t.push(b'.')?;
            }
            t.extend_from_slice(b"in-addr.arpa")?;
        }
        Ip::V6(a) => {
            for &b in a.iter().rev() {
                t.push(crate::net::hex_digit(u32::from(b & 0xf), false))?;
                t.push(b'.')?;
                t.push(crate::net::hex_digit(u32::from(b >> 4), false))?;
                t.push(b'.')?;
            }
            t.extend_from_slice(b"ip6.arpa")?;
        }
    }
    Ok(t)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn chain() {
        // c1 -> c2 -> a, an unrelated A, then a's A.
        let mut m = alloc::vec::Vec::new();
        m.extend_from_slice(&[0, 1, 0x81, 0x80, 0, 1, 0, 4, 0, 0, 0, 0]);
        m.extend_from_slice(b"\x02c1\x04test\x00\x00\x01\x00\x01");
        let rr = |m: &mut alloc::vec::Vec<u8>, owner: &[u8], t: u16, data: &[u8]| {
            m.extend_from_slice(owner);
            m.extend_from_slice(&t.to_be_bytes());
            m.extend_from_slice(&[0, 1, 0, 0, 0, 60]);
            m.extend_from_slice(&u16::try_from(data.len()).unwrap().to_be_bytes());
            m.extend_from_slice(data);
        };
        rr(&mut m, b"\x02c1\x04test\x00", 5, b"\x02c2\x04test\x00");
        rr(&mut m, b"\x04evil\x04test\x00", 1, &[6, 6, 6, 6]);
        rr(&mut m, b"\x01a\x04test\x00", 1, &[192, 0, 2, 10]);
        rr(&mut m, b"\x02c2\x04test\x00", 5, b"\x01a\x04test\x00");
        let f = match read(&m, b"C1.test", T_A).unwrap() {
            Outcome::Found(f) => f,
            o => unreachable!("{o:?}"),
        };
        assert_eq!(f.name.as_slice(), b"a.test");
        assert_eq!(f.aliases.len(), 2);
        assert_eq!(f.aliases[0].as_slice(), b"C1.test");
        assert_eq!(f.addrs.as_slice(), &[Ip::V4([192, 0, 2, 10])]);
        assert!(matches!(read(&m, b"c1.test", T_AAAA).unwrap(), Outcome::NoData));
        assert_eq!(reverse_name(&Ip::V4([192, 0, 2, 10])).unwrap().as_slice(), b"10.2.0.192.in-addr.arpa");
        assert_eq!(ttl(&m), Some(60));
        // The smallest TTL of the answer section.
        // (header 12, question 13, first record's owner 9, type and class 4, TTL's low byte)
        m[12 + 13 + 9 + 4 + 3] = 7;
        assert_eq!(ttl(&m), Some(7));
    }

    #[test]
    fn negative_ttl() {
        // NXDOMAIN with an SOA in the authority section: min(TTL 600, MINIMUM 120).
        let mut m = alloc::vec::Vec::new();
        m.extend_from_slice(&[0, 1, 0x81, 0x83, 0, 1, 0, 0, 0, 1, 0, 0]);
        m.extend_from_slice(b"\x01x\x04test\x00\x00\x01\x00\x01");
        m.extend_from_slice(b"\x04test\x00\x00\x06\x00\x01\x00\x00\x02\x58");
        let mut rd = alloc::vec::Vec::new();
        rd.extend_from_slice(b"\x02ns\x04test\x00\x05admin\x04test\x00");
        for v in [1u32, 7200, 3600, 1_209_600, 120] {
            rd.extend_from_slice(&v.to_be_bytes());
        }
        m.extend_from_slice(&u16::try_from(rd.len()).unwrap().to_be_bytes());
        m.extend_from_slice(&rd);
        assert_eq!(ttl(&m), Some(120));
        // Without the SOA: nothing says.
        let mut bare = m[..12 + 11].to_vec();
        bare[9] = 0;
        assert_eq!(ttl(&bare), None);
    }
}
