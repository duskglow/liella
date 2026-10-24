//! Text forms of DNS data (spec §16.4 `ns_sprintrr`, `ns_sprintrrf`, `ns_format_ttl`,
//! `ns_parse_ttl`, `ns_datetosecs`, the `__p_*`, `__fp_*`, `__sym_*`, `__b64_*` and `__loc_*`
//! entries; D-16.3). Every layout here is the one the reference prints (probes in
//! `tests/oracle/resolv_diff.c`); the functions build text into a [`TryVec`] and the C
//! conventions (buffers, static results, streams) are the boundary's.
//!
//! **Records** ([`sprintrr`]). `owner<pad 24>ttl class type<pad 40>data`, where a pad is tabs to
//! the column, or two spaces once a field reaches the column before it (then every later pad is
//! two spaces). The owner is absolute (a final dot) unless an origin is given and the name is in
//! it (`@` for the origin itself, else the labels above it), and empty when it equals the
//! previous owner the caller names. The TTL is [`format_ttl`]. Data: addresses, names (relative
//! to the origin like the owner), numbers and quoted strings for the types the reference prints
//! (A, NS, CNAME, SOA, MB, MG, MR, PTR, HINFO, MINFO, MX, TXT, RP, AFSDB, X25, ISDN, RT, NSAP,
//! AAAA, LOC, SRV, NAPTR, PX, WKS, A6, DNAME); every other type, and data that does not parse
//! for its type, as the RFC 3597 generic form `\# length ( hex ; text )`.
//!
//! **LOC** ([`loc_aton`], [`loc_ntoa`]): RFC 1876's text and binary forms. Sizes are kept in 64
//! bits (RFC 1876 allows up to 9e9 cm; the reference's text parser wraps larger values at 32
//! bits) and the altitude is its 32-bit value less 100,000 m (the reference prints values above
//! 21,474,836.47 m as negative garbage): deliberate differences.

#![allow(clippy::arithmetic_side_effects)] // dates, TTLs (u64 with fixed unit sizes), columns and bytes of short texts

use alloc::vec::Vec;

use crate::error::{Errno, Result};
use crate::mem::TryVec;
use crate::net::addr;

use super::msg::{self, Header, Parsed, OP_UPDATE};
use super::name::{self, MAXCDNAME};

// ------------------------------------------------------------------------------ TTLs and dates

/// Append the TTL text of `secs` (`ns_format_ttl`): weeks, days, hours, minutes and seconds,
/// each unit that is not zero; one unit alone in upper case, several in lower case; `0S`.
pub fn format_ttl(secs: u64, out: &mut TryVec<u8>) -> Result<()> {
    let units: [(u64, u8); 5] = [(604_800, b'w'), (86_400, b'd'), (3600, b'h'), (60, b'm'), (1, b's')];
    let mut rest = secs;
    let mut parts: [(u64, u8); 5] = [(0, 0); 5];
    let mut n = 0usize;
    for (size, unit) in units {
        let v = rest / size;
        rest %= size;
        if v > 0 {
            if let Some(p) = parts.get_mut(n) {
                *p = (v, unit);
            }
            n = n.saturating_add(1);
        }
    }
    if n == 0 {
        return out.extend_from_slice(b"0S");
    }
    for &(v, unit) in parts.get(..n).unwrap_or(&[]) {
        push_dec(out, v)?;
        out.push(if n == 1 { unit.to_ascii_uppercase() } else { unit })?;
    }
    Ok(())
}

/// Append the decimal text of `v`.
pub fn push_dec(out: &mut TryVec<u8>, v: u64) -> Result<()> {
    let mut t = crate::net::Text::<24>::new();
    t.push_dec(v);
    out.extend_from_slice(t.as_bytes())
}

fn push_signed(out: &mut TryVec<u8>, v: i64) -> Result<()> {
    if v < 0 {
        out.push(b'-')?;
    }
    push_dec(out, v.unsigned_abs())
}

/// `ns_parse_ttl(text)`: a decimal number alone, or numbers each followed by a unit (`w`, `d`,
/// `h`, `m`, `s`, any case; repeats add up). Nothing else is accepted (observed: no white space,
/// no number without a unit after a unit).
pub fn parse_ttl(text: &[u8]) -> Option<u64> {
    if text.is_empty() {
        return None;
    }
    if text.iter().all(u8::is_ascii_digit) {
        return text.iter().try_fold(0u64, |v, &c| v.checked_mul(10)?.checked_add(u64::from(c.wrapping_sub(b'0'))));
    }
    let mut total = 0u64;
    let mut v: Option<u64> = None;
    for &c in text {
        if c.is_ascii_digit() {
            v = Some(v.unwrap_or(0).checked_mul(10)?.checked_add(u64::from(c.wrapping_sub(b'0')))?);
            continue;
        }
        let mult = match c.to_ascii_lowercase() {
            b'w' => 604_800,
            b'd' => 86_400,
            b'h' => 3600,
            b'm' => 60,
            b's' => 1,
            _ => return None,
        };
        total = total.checked_add(v.take()?.checked_mul(mult)?)?;
    }
    if v.is_some() {
        return None;
    }
    Some(total)
}

/// Days from 1970-01-01 to the civil date (proleptic Gregorian).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The civil date of a day count from 1970-01-01.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `ns_datetosecs(text)`: `YYYYMMDDhhmmss` (exactly 14 digits; year after 1970, month 1–12, day
/// 1–31 without a check against the month, hour 0–23, minute and second 0–59; observed) as
/// seconds since the epoch, modulo 2^32.
#[allow(clippy::arithmetic_side_effects)] // small bounded integers
pub fn datetosecs(text: &[u8]) -> Option<u32> {
    if text.len() != 14 || !text.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let num = |a: usize, b: usize| -> i64 { text.get(a..b).unwrap_or(&[]).iter().fold(0i64, |v, &c| v * 10 + i64::from(c.wrapping_sub(b'0'))) };
    let (y, mo, d, h, mi, s) = (num(0, 4), num(4, 6), num(6, 8), num(8, 10), num(10, 12), num(12, 14));
    if y <= 1970 || !(1..=12).contains(&mo) || !(1..=31).contains(&d) || h > 23 || mi > 59 || s > 59 {
        return None;
    }
    let days = days_from_civil(y, mo, 1) + d - 1;
    let secs = days * 86_400 + h * 3600 + mi * 60 + s;
    Some(u32::try_from(secs.rem_euclid(1 << 32)).unwrap_or(0))
}

/// `p_secstodate(secs)`: `YYYYMMDDhhmmss` in UTC, or `<overflow>` beyond 2^31 - 1 (observed).
#[allow(clippy::arithmetic_side_effects)] // bounded by the check
pub fn secstodate(secs: u64) -> [u8; 14] {
    if secs > 0x7fff_ffff {
        let mut o = [0u8; 14];
        o.get_mut(..10).unwrap_or(&mut []).copy_from_slice(b"<overflow>");
        return o;
    }
    let s = i64::try_from(secs).unwrap_or(0);
    let (y, m, d) = civil_from_days(s / 86_400);
    let r = s % 86_400;
    let mut o = [0u8; 14];
    let fields = [(y, 4usize), (m, 2), (d, 2), (r / 3600, 2), (r / 60 % 60, 2), (r % 60, 2)];
    let mut at = 0usize;
    for (v, w) in fields {
        let mut x = v;
        for k in (0..w).rev() {
            if let Some(c) = o.get_mut(at + k) {
                *c = b'0' + u8::try_from(x % 10).unwrap_or(0);
            }
            x /= 10;
        }
        at += w;
    }
    o
}

// ------------------------------------------------------------------------------ Base64

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// `b64_ntop(data)`: the Base64 text (RFC 4648, padded) into `out`, which must also hold a
/// terminator: the text's length.
pub fn b64_ntop(data: &[u8], out: &mut [u8]) -> Result<usize> {
    let need = data.len().div_ceil(3).checked_mul(4).ok_or(Errno::EMSGSIZE)?;
    if out.len() <= need {
        return Err(Errno::EMSGSIZE);
    }
    let mut o = name::Out::new(out);
    for chunk in data.chunks(3) {
        let b = [chunk.first().copied().unwrap_or(0), chunk.get(1).copied().unwrap_or(0), chunk.get(2).copied().unwrap_or(0)];
        let v = u32::from(b[0]) << 16 | u32::from(b[1]) << 8 | u32::from(b[2]);
        for k in 0..4usize {
            let c = if k > chunk.len() { b'=' } else { B64.get(usize::try_from(v >> (18 - 6 * u32::try_from(k).unwrap_or(0)) & 63).unwrap_or(0)).copied().unwrap_or(b'=') };
            o.push(c)?;
        }
    }
    o.push(0)?;
    Ok(need)
}

/// `b64_pton(text)`: the bytes the Base64 text stands for, into `out` (`None`: count only).
/// White space anywhere is skipped; the text must be whole quantums, padded at the end only.
pub fn b64_pton(text: &[u8], mut out: Option<&mut [u8]>) -> Result<usize> {
    let mut vals = [0u8; 4];
    let mut n = 0usize;
    let mut pad = 0usize;
    let mut len = 0usize;
    let mut done = false;
    for &c in text {
        if matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c) {
            continue;
        }
        if done {
            return Err(Errno::EINVAL);
        }
        let v = if c == b'=' {
            if n < 2 {
                return Err(Errno::EINVAL);
            }
            pad = pad.saturating_add(1);
            0
        } else {
            if pad > 0 {
                return Err(Errno::EINVAL);
            }
            u8::try_from(B64.iter().position(|&b| b == c).ok_or(Errno::EINVAL)?).unwrap_or(0)
        };
        *vals.get_mut(n).ok_or(Errno::EINVAL)? = v;
        n = n.saturating_add(1);
        if n == 4 {
            let bytes = [vals[0] << 2 | vals[1] >> 4, vals[1] << 4 | vals[2] >> 2, vals[2] << 6 | vals[3]];
            let take = 3usize.saturating_sub(pad);
            if let Some(o) = out.as_deref_mut() {
                let end = len.checked_add(take).ok_or(Errno::EMSGSIZE)?;
                o.get_mut(len..end).ok_or(Errno::EMSGSIZE)?.copy_from_slice(bytes.get(..take).unwrap_or(&[]));
            }
            len = len.saturating_add(take);
            n = 0;
            done = pad > 0;
        }
    }
    if n != 0 {
        return Err(Errno::EINVAL);
    }
    Ok(len)
}

// ------------------------------------------------------------------------------ symbol tables

/// One entry of a symbol table (`struct res_sym`).
#[derive(Clone, Copy, Debug)]
pub struct Sym {
    /// The number.
    pub number: i32,
    /// The symbolic name (NUL-terminated).
    pub name: &'static [u8],
    /// The human name (NUL-terminated), if any.
    pub human: Option<&'static [u8]>,
}

const fn sym(number: i32, name: &'static [u8], human: &'static [u8]) -> Sym {
    Sym { number, name, human: Some(human) }
}

/// `__p_type_syms` (the order and texts are the exported table's).
pub const TYPE_SYMS: [Sym; 45] = [
    sym(1, b"A\0", b"address\0"),
    sym(2, b"NS\0", b"name server\0"),
    sym(3, b"MD\0", b"mail destination (deprecated)\0"),
    sym(4, b"MF\0", b"mail forwarder (deprecated)\0"),
    sym(5, b"CNAME\0", b"canonical name\0"),
    sym(6, b"SOA\0", b"start of authority\0"),
    sym(7, b"MB\0", b"mailbox\0"),
    sym(8, b"MG\0", b"mail group member\0"),
    sym(9, b"MR\0", b"mail rename\0"),
    sym(10, b"NULL\0", b"null\0"),
    sym(11, b"WKS\0", b"well-known service (deprecated)\0"),
    sym(12, b"PTR\0", b"domain name pointer\0"),
    sym(13, b"HINFO\0", b"host information\0"),
    sym(14, b"MINFO\0", b"mailbox information\0"),
    sym(15, b"MX\0", b"mail exchanger\0"),
    sym(16, b"TXT\0", b"text\0"),
    sym(17, b"RP\0", b"responsible person\0"),
    sym(18, b"AFSDB\0", b"DCE or AFS server\0"),
    sym(19, b"X25\0", b"X25 address\0"),
    sym(20, b"ISDN\0", b"ISDN address\0"),
    sym(21, b"RT\0", b"router\0"),
    sym(22, b"NSAP\0", b"nsap address\0"),
    sym(23, b"NSAP_PTR\0", b"domain name pointer\0"),
    sym(24, b"SIG\0", b"signature\0"),
    sym(25, b"KEY\0", b"key\0"),
    sym(26, b"PX\0", b"mapping information\0"),
    sym(27, b"GPOS\0", b"geographical position (withdrawn)\0"),
    sym(28, b"AAAA\0", b"IPv6 address\0"),
    sym(29, b"LOC\0", b"location\0"),
    sym(30, b"NXT\0", b"next valid name (unimplemented)\0"),
    sym(31, b"EID\0", b"endpoint identifier (unimplemented)\0"),
    sym(32, b"NIMLOC\0", b"NIMROD locator (unimplemented)\0"),
    sym(33, b"SRV\0", b"server selection\0"),
    sym(34, b"ATMA\0", b"ATM address (unimplemented)\0"),
    sym(39, b"DNAME\0", b"Non-terminal DNAME (for IPv6)\0"),
    sym(250, b"TSIG\0", b"transaction signature\0"),
    sym(251, b"IXFR\0", b"incremental zone transfer\0"),
    sym(252, b"AXFR\0", b"zone transfer\0"),
    sym(253, b"MAILB\0", b"mailbox-related data (deprecated)\0"),
    sym(254, b"MAILA\0", b"mail agent (deprecated)\0"),
    sym(35, b"NAPTR\0", b"URN Naming Authority\0"),
    sym(36, b"KX\0", b"Key Exchange\0"),
    sym(37, b"CERT\0", b"Certificate\0"),
    sym(255, b"ANY\0", b"\"any\"\0"),
    Sym { number: 0, name: b"\0", human: None },
];

/// `__p_class_syms`.
pub const CLASS_SYMS: [Sym; 7] = [
    Sym { number: 1, name: b"IN\0", human: None },
    Sym { number: 3, name: b"CHAOS\0", human: None },
    Sym { number: 4, name: b"HS\0", human: None },
    Sym { number: 4, name: b"HESIOD\0", human: None },
    Sym { number: 255, name: b"ANY\0", human: None },
    Sym { number: 254, name: b"NONE\0", human: None },
    Sym { number: 0, name: b"\0", human: None },
];

/// The response code names `p_rcode` gives (11 is the empty text, observed).
const RCODES: [(i32, &[u8]); 15] = [
    (0, b"NOERROR\0"),
    (1, b"FORMERR\0"),
    (2, b"SERVFAIL\0"),
    (3, b"NXDOMAIN\0"),
    (4, b"NOTIMP\0"),
    (5, b"REFUSED\0"),
    (6, b"YXDOMAIN\0"),
    (7, b"YXRRSET\0"),
    (8, b"NXRRSET\0"),
    (9, b"NOTAUTH\0"),
    (10, b"NOTZONE\0"),
    (11, b"\0"),
    (16, b"BADSIG\0"),
    (17, b"BADKEY\0"),
    (18, b"BADTIME\0"),
];

/// `_res_opcodes`: the opcode names.
pub const OPCODES: [&[u8]; 16] = [
    b"QUERY\0", b"IQUERY\0", b"CQUERYM\0", b"CQUERYU\0", b"NOTIFY\0", b"UPDATE\0", b"6\0", b"7\0", b"8\0", b"9\0", b"10\0", b"11\0", b"12\0", b"13\0",
    b"ZONEINIT\0", b"ZONEREF\0",
];

/// The names `p_option` gives the resolver options.
const OPTIONS: [(u64, &[u8]); 18] = [
    (0x1, b"init\0"),
    (0x2, b"debug\0"),
    (0x8, b"use-vc\0"),
    (0x20, b"igntc\0"),
    (0x40, b"recurs\0"),
    (0x80, b"defnam\0"),
    (0x100, b"styopn\0"),
    (0x200, b"dnsrch\0"),
    (0x1000, b"noaliases\0"),
    (0x4000, b"rotate\0"),
    (0x10_0000, b"edns0\0"),
    (0x20_0000, b"single-request\0"),
    (0x40_0000, b"single-request-reopen\0"),
    (0x80_0000, b"dnssec\0"),
    (0x100_0000, b"no-tld-query\0"),
    (0x200_0000, b"no-reload\0"),
    (0x400_0000, b"trust-ad\0"),
    (0x800_0000, b"no-aaaa\0"),
];

/// The entry of `syms` for `number` (the terminator excluded).
pub fn sym_of(syms: &'static [Sym], number: i32) -> Option<&'static Sym> {
    syms.iter().take_while(|s| s.name != b"\0").find(|s| s.number == number)
}

/// A static text, or a number to print.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Name {
    /// A name (NUL-terminated).
    Static(&'static [u8]),
    /// No name: the decimal number.
    Number(i64),
    /// No name: `?0x<hex>?` (`p_option`).
    Hex(u64),
}

impl Name {
    /// Append the text (without terminator).
    pub fn push_to(self, out: &mut TryVec<u8>) -> Result<()> {
        match self {
            Name::Static(s) => out.extend_from_slice(s.strip_suffix(b"\0").unwrap_or(s)),
            Name::Number(n) => push_signed(out, n),
            Name::Hex(v) => {
                out.extend_from_slice(b"?0x")?;
                let mut started = false;
                for shift in (0..16u32).rev() {
                    let d = u32::try_from(v >> (shift * 4) & 0xf).unwrap_or(0);
                    if d != 0 || started || shift == 0 {
                        started = true;
                        out.push(crate::net::hex_digit(d, false))?;
                    }
                }
                out.push(b'?')
            }
        }
    }
}

/// `p_type(type)`.
pub fn p_type(t: i32) -> Name {
    sym_of(&TYPE_SYMS, t).map_or(Name::Number(i64::from(t)), |s| Name::Static(s.name))
}

/// `p_class(class)`.
pub fn p_class(c: i32) -> Name {
    sym_of(&CLASS_SYMS, c).map_or(Name::Number(i64::from(c)), |s| Name::Static(s.name))
}

/// `p_rcode(rcode)`.
pub fn p_rcode(r: i32) -> Name {
    RCODES.iter().find(|(n, _)| *n == r).map_or(Name::Number(i64::from(r)), |(_, s)| Name::Static(s))
}

/// `p_option(option)`.
pub fn p_option(o: u64) -> Name {
    OPTIONS.iter().find(|(v, _)| *v == o).map_or(Name::Hex(o), |(_, s)| Name::Static(s))
}

fn type_name(t: u16, out: &mut TryVec<u8>) -> Result<()> {
    match sym_of(&TYPE_SYMS, i32::from(t)) {
        Some(s) => Name::Static(s.name).push_to(out),
        None if t == 38 => out.extend_from_slice(b"A6"),
        None => {
            out.extend_from_slice(b"TYPE")?;
            push_dec(out, u64::from(t))
        }
    }
}

fn class_name(c: u16, out: &mut TryVec<u8>) -> Result<()> {
    match sym_of(&CLASS_SYMS, i32::from(c)) {
        Some(s) => Name::Static(s.name).push_to(out),
        None => {
            out.extend_from_slice(b"CLASS")?;
            push_dec(out, u64::from(c))
        }
    }
}

// ------------------------------------------------------------------------------ records

/// One record to print.
#[derive(Clone, Copy, Debug)]
pub struct Record<'a> {
    /// The owner, text (the empty text is the root).
    pub owner: &'a [u8],
    /// The class.
    pub class: u16,
    /// The type.
    pub rtype: u16,
    /// The TTL.
    pub ttl: u64,
    /// The data.
    pub rdata: &'a [u8],
    /// The data's offset in the message, when it is inside it (names in the data can only be
    /// read then).
    pub rdata_at: Option<usize>,
}

/// A line being written: the column, and whether pads have become two spaces.
struct Line<'a> {
    out: &'a mut TryVec<u8>,
    col: usize,
    spaced: bool,
}

impl Line<'_> {
    fn put(&mut self, b: &[u8]) -> Result<()> {
        for &c in b {
            self.col = match c {
                b'\n' => 0,
                b'\t' => (self.col / 8).saturating_add(1).saturating_mul(8),
                _ => self.col.saturating_add(1),
            };
        }
        self.out.extend_from_slice(b)
    }

    fn put_vec(&mut self, f: impl FnOnce(&mut TryVec<u8>) -> Result<()>) -> Result<()> {
        let mut t = TryVec::new();
        f(&mut t)?;
        self.put(&t)
    }

    /// Pad to `target` (see the module documentation).
    fn pad(&mut self, target: usize) -> Result<()> {
        if self.spaced || self.col.saturating_add(1) >= target {
            self.spaced = true;
            return self.put(b"  ");
        }
        while self.col < target {
            self.put(b"\t")?;
        }
        Ok(())
    }

    /// Tabs to `target` when short of it.
    fn tabs_to(&mut self, target: usize) -> Result<()> {
        while self.col < target {
            self.put(b"\t")?;
        }
        Ok(())
    }
}

/// The labels of the text name `text` as a wire name.
fn wire(text: &[u8]) -> Option<([u8; MAXCDNAME + 1], usize)> {
    let mut w = [0u8; MAXCDNAME + 1];
    let (n, _) = name::pton(text, &mut w).ok()?;
    Some((w, n))
}

/// Append the text of the wire name `w` relative to `origin` (see the module documentation);
/// `absolute_text`: the text to use when the name is not in the origin (a final dot added).
fn name_rel(w: &[u8], origin: Option<&[u8]>, absolute_text: Option<&[u8]>, out: &mut TryVec<u8>) -> Result<()> {
    if let Some(o) = origin.and_then(wire) {
        let ow = o.0.get(..o.1).unwrap_or(&[]);
        let olabels = name::labels(ow).map_or(0, Iterator::count);
        let ls: Vec<&[u8]> = name::labels(w).map(Iterator::collect).unwrap_or_default();
        if ls.len() >= olabels {
            let keep = ls.len().saturating_sub(olabels);
            // The suffix of `w` after `keep` labels.
            let at = ls.iter().take(keep).fold(0usize, |a, l| a.saturating_add(l.len()).saturating_add(1));
            if name::wire_eq(w.get(at..).unwrap_or(&[]), ow) {
                if keep == 0 {
                    return out.push(b'@');
                }
                let mut tmp = [0u8; name::MAXDNAME];
                let mut sub = [0u8; MAXCDNAME + 1];
                let mut o2 = name::Out::new(&mut sub);
                for l in ls.iter().take(keep) {
                    o2.push(u8::try_from(l.len()).map_err(|_| Errno::EMSGSIZE)?)?;
                    o2.push_all(l)?;
                }
                o2.push(0)?;
                let n = name::ntop(o2.written(), &mut tmp)?;
                return out.extend_from_slice(tmp.get(..n.saturating_sub(1)).unwrap_or(&[]));
            }
        }
    }
    match absolute_text {
        Some(t) if !t.is_empty() && t != b"." => {
            out.extend_from_slice(t)?;
            let slashes = t.get(..t.len().saturating_sub(1)).unwrap_or(&[]).iter().rev().take_while(|&&c| c == b'\\').count();
            if t.last() != Some(&b'.') || slashes % 2 == 1 {
                out.push(b'.')?;
            }
            Ok(())
        }
        Some(_) => out.push(b'.'),
        None => {
            let mut tmp = [0u8; name::MAXDNAME];
            let n = name::ntop(w, &mut tmp)?;
            let t = tmp.get(..n.saturating_sub(1)).unwrap_or(&[]);
            out.extend_from_slice(t)?;
            if t != b"." {
                out.push(b'.')?;
            }
            Ok(())
        }
    }
}

/// A reader of a record's data.
struct Data<'a> {
    msg: &'a [u8],
    rd: &'a [u8],
    at: Option<usize>,
    pos: usize,
}

impl<'a> Data<'a> {
    fn left(&self) -> usize {
        self.rd.len().saturating_sub(self.pos)
    }

    fn bytes(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let b = self.rd.get(self.pos..end)?;
        self.pos = end;
        Some(b)
    }

    fn u16(&mut self) -> Option<u16> {
        let b = self.bytes(2)?;
        Some(u16::from_be_bytes([*b.first()?, *b.get(1)?]))
    }

    fn u32(&mut self) -> Option<u32> {
        let b = self.bytes(4)?;
        Some(u32::from_be_bytes(b.try_into().ok()?))
    }

    fn name(&mut self) -> Option<([u8; MAXCDNAME + 1], usize)> {
        let base = self.at?;
        let mut w = [0u8; MAXCDNAME + 1];
        let (n, used) = name::unpack(self.msg, base.checked_add(self.pos)?, &mut w).ok()?;
        if used > self.left() {
            return None;
        }
        self.pos = self.pos.checked_add(used)?;
        Some((w, n))
    }

    fn string(&mut self) -> Option<&'a [u8]> {
        let n = usize::from(*self.bytes(1)?.first()?);
        self.bytes(n)
    }
}

fn quoted(s: &[u8], out: &mut TryVec<u8>) -> Result<()> {
    out.push(b'"')?;
    for &c in s {
        if c == b'"' || c == b'\\' {
            out.push(b'\\')?;
        }
        out.push(c)?;
    }
    out.push(b'"')
}

/// The data text of a record, or `None` when the data does not parse for its type (the
/// caller then prints the generic form). `Err` only for running out of memory.
fn rdata_text(r: &Record<'_>, msg: &[u8], origin: Option<&[u8]>, line: &mut Line<'_>) -> Result<Option<()>> {
    let mut d = Data { msg, rd: r.rdata, at: r.rdata_at, pos: 0 };
    let mut t = TryVec::new();
    let ok = (|| -> Result<Option<()>> {
        macro_rules! get {
            ($e:expr) => {
                match $e {
                    Some(v) => v,
                    None => return Ok(None),
                }
            };
        }
        let name = |d: &mut Data<'_>, t: &mut TryVec<u8>| -> Result<Option<()>> {
            let (w, n) = get!(d.name());
            name_rel(w.get(..n).unwrap_or(&[]), origin, None, t)?;
            Ok(Some(()))
        };
        match r.rtype {
            1 => {
                let a: [u8; 4] = get!(d.bytes(4).and_then(|b| b.try_into().ok()));
                t.extend_from_slice(addr::format_v4(a).as_bytes())?;
            }
            2 | 5 | 7 | 8 | 9 | 12 | 39 => get!(name(&mut d, &mut t)?),
            6 => {
                get!(name(&mut d, &mut t)?);
                t.push(b' ')?;
                get!(name(&mut d, &mut t)?);
                t.extend_from_slice(b" (\n")?;
                let labels: [&[u8]; 5] = [b"serial", b"refresh", b"retry", b"expiry", b"minimum"];
                let mut vals = [0u32; 5];
                for v in vals.iter_mut() {
                    *v = get!(d.u32());
                }
                line.put(&t)?;
                t.clear();
                for (i, (v, l)) in vals.iter().zip(labels).enumerate() {
                    line.put(b"\t\t\t\t\t")?;
                    if i == 0 {
                        line.put_vec(|o| push_dec(o, u64::from(*v)))?;
                    } else {
                        line.put_vec(|o| format_ttl(u64::from(*v), o))?;
                    }
                    if i == 4 {
                        line.put(b" )")?;
                    }
                    line.tabs_to(56)?;
                    line.put(b"; ")?;
                    line.put(l)?;
                    line.put(b"\n")?;
                }
            }
            13 | 20 => {
                quoted(get!(d.string()), &mut t)?;
                t.push(b' ')?;
                if d.left() > 0 {
                    quoted(get!(d.string()), &mut t)?;
                }
            }
            16 => {
                let mut first = true;
                while d.left() > 0 {
                    if !first {
                        t.push(b' ')?;
                    }
                    first = false;
                    match d.string() {
                        Some(s) => quoted(s, &mut t)?,
                        None => {
                            line.put(&t)?;
                            line.put(b"\"\"")?;
                            return Ok(None);
                        }
                    }
                }
            }
            19 => quoted(get!(d.string()), &mut t)?,
            14 | 17 => {
                get!(name(&mut d, &mut t)?);
                t.push(b' ')?;
                get!(name(&mut d, &mut t)?);
            }
            15 | 18 | 21 => {
                push_dec(&mut t, u64::from(get!(d.u16())))?;
                t.push(b' ')?;
                get!(name(&mut d, &mut t)?);
            }
            22 => {
                let b = get!(d.bytes(d.left()));
                for (i, &x) in b.iter().enumerate() {
                    if i % 2 == 1 {
                        t.push(b'.')?;
                    }
                    t.push(crate::net::hex_digit(u32::from(x >> 4), true))?;
                    t.push(crate::net::hex_digit(u32::from(x & 0xf), true))?;
                }
            }
            26 => {
                push_dec(&mut t, u64::from(get!(d.u16())))?;
                t.push(b' ')?;
                get!(name(&mut d, &mut t)?);
                t.push(b' ')?;
                get!(name(&mut d, &mut t)?);
            }
            28 => {
                let a: [u8; 16] = get!(d.bytes(16).and_then(|b| b.try_into().ok()));
                t.extend_from_slice(addr::format_v6(a).as_bytes())?;
            }
            29 => {
                let b: [u8; 16] = get!(d.bytes(16).and_then(|b| b.try_into().ok()));
                loc_ntoa(&b, &mut t)?;
            }
            33 => {
                for _ in 0..3 {
                    push_dec(&mut t, u64::from(get!(d.u16())))?;
                    t.push(b' ')?;
                }
                get!(name(&mut d, &mut t)?);
            }
            35 => {
                push_dec(&mut t, u64::from(get!(d.u16())))?;
                t.push(b' ')?;
                push_dec(&mut t, u64::from(get!(d.u16())))?;
                for _ in 0..3 {
                    t.push(b' ')?;
                    quoted(get!(d.string()), &mut t)?;
                }
                t.push(b' ')?;
                get!(name(&mut d, &mut t)?);
            }
            11 => {
                let a: [u8; 4] = get!(d.bytes(4).and_then(|b| b.try_into().ok()));
                t.extend_from_slice(addr::format_v4(a).as_bytes())?;
                t.push(b' ')?;
                push_dec(&mut t, u64::from(*get!(d.bytes(1)).first().unwrap_or(&0)))?;
                t.extend_from_slice(b" ( \n\t\t\t\t")?;
                let bits = get!(d.bytes(d.left()));
                let mut first = true;
                for (i, &byte) in bits.iter().enumerate() {
                    for bit in 0..8usize {
                        if byte & (0x80 >> bit) != 0 {
                            if !first {
                                t.push(b' ')?;
                            }
                            first = false;
                            push_dec(&mut t, u64::try_from(i.saturating_mul(8).saturating_add(bit)).unwrap_or(0))?;
                        }
                    }
                }
                t.extend_from_slice(b" )")?;
            }
            38 => {
                let plen = *get!(d.bytes(1)).first().unwrap_or(&0);
                if plen > 128 {
                    return Ok(None);
                }
                let skip = usize::from(plen / 8);
                let mut a = [0u8; 16];
                let tail = get!(d.bytes(16usize.saturating_sub(skip)));
                a.get_mut(skip..).unwrap_or(&mut []).copy_from_slice(tail);
                push_dec(&mut t, u64::from(plen))?;
                t.push(b' ')?;
                t.extend_from_slice(addr::format_v6(a).as_bytes())?;
                if plen > 0 {
                    t.push(b' ')?;
                    get!(name(&mut d, &mut t)?);
                }
            }
            _ => return Ok(None),
        }
        if d.left() != 0 {
            return Ok(None);
        }
        Ok(Some(()))
    })()?;
    if ok.is_some() {
        line.put(&t)?;
    }
    Ok(ok)
}

/// The generic form of data (`\# length ( hex ; text )`), with the format-error note.
fn generic(rd: &[u8], error: bool, line: &mut Line<'_>) -> Result<()> {
    line.put(b"\\# ")?;
    line.put_vec(|o| push_dec(o, u64::try_from(rd.len()).unwrap_or(0)))?;
    if rd.is_empty() {
        return Ok(());
    }
    line.put(b" (")?;
    if error {
        line.put(b" ; RR format error")?;
    }
    line.put(b"\n")?;
    let chunks = rd.len().div_ceil(16);
    for (i, chunk) in rd.chunks(16).enumerate() {
        line.put(b"\t")?;
        let mut hex = TryVec::new();
        for &b in chunk {
            hex.push(crate::net::hex_digit(u32::from(b >> 4), false))?;
            hex.push(crate::net::hex_digit(u32::from(b & 0xf), false))?;
            hex.push(b' ')?;
        }
        let last = i.saturating_add(1) == chunks;
        if last && chunk.len() < 16 {
            hex.push(b')')?;
        }
        line.put(&hex)?;
        // The reference's pad (observed): tabs for the part's width to column 46.
        let tabs = 46usize.saturating_sub(hex.len()).div_ceil(8);
        for _ in 0..tabs {
            line.put(b"\t")?;
        }
        line.put(b"; ")?;
        let text: TryVec<u8> = {
            let mut t = TryVec::new();
            for &b in chunk {
                t.push(if (0x20..=0x7e).contains(&b) { b } else { b'.' })?;
            }
            t
        };
        line.put(&text)?;
        if !last {
            line.put(b"\n")?;
        }
    }
    Ok(())
}

/// `ns_sprintrrf`: the text of one record (see the module documentation) into `out`.
/// `name_ctx`: the previous owner (an equal owner is printed empty); `origin`: names in it are
/// printed relative to it. `Err(EMSGSIZE)` when a name of the data cannot be read.
pub fn sprintrr(msg: &[u8], r: &Record<'_>, name_ctx: Option<&[u8]>, origin: Option<&[u8]>, out: &mut TryVec<u8>) -> Result<()> {
    let mut line = Line { out, col: 0, spaced: false };
    let owner_wire = wire(if r.owner.is_empty() { b"." } else { r.owner });
    let same_ctx = name_ctx.is_some_and(|c| matches!(name::samename(r.owner, c), Ok(true)));
    if !same_ctx {
        let mut t = TryVec::new();
        match owner_wire {
            Some((w, n)) => name_rel(w.get(..n).unwrap_or(&[]), origin, Some(r.owner), &mut t)?,
            None => return Err(Errno::EMSGSIZE),
        }
        line.put(&t)?;
    }
    line.pad(24)?;
    line.put_vec(|o| format_ttl(r.ttl, o))?;
    line.put(b" ")?;
    line.put_vec(|o| class_name(r.class, o))?;
    line.put(b" ")?;
    line.put_vec(|o| type_name(r.rtype, o))?;
    line.pad(40)?;
    let names = matches!(r.rtype, 2 | 5 | 6 | 7 | 8 | 9 | 12 | 14 | 15 | 17 | 18 | 21 | 26 | 33 | 35 | 39) || (r.rtype == 38 && r.rdata.first().is_some_and(|&p| p > 0));
    if names && r.rdata_at.is_none() {
        return Err(Errno::EMSGSIZE);
    }
    let known = matches!(r.rtype, 1 | 2 | 5 | 6 | 7 | 8 | 9 | 11 | 12 | 13 | 14 | 15 | 16 | 17 | 18 | 19 | 20 | 21 | 22 | 26 | 28 | 29 | 33 | 35 | 38 | 39);
    if !known {
        return generic(r.rdata, false, &mut line);
    }
    let mut probe = TryVec::new();
    let mut scratch = Line { out: &mut probe, col: line.col, spaced: line.spaced };
    match rdata_text(r, msg, origin, &mut scratch)? {
        Some(()) => {
            line.out.extend_from_slice(&probe)?;
            Ok(())
        }
        None => {
            // TXT prints what it read before the error (observed `""`).
            if r.rtype == 16 && !probe.is_empty() {
                line.put(&probe)?;
            }
            generic(r.rdata, true, &mut line)
        }
    }
}

// ------------------------------------------------------------------------------ messages

/// `RES_PRF_*` bits of `pfcode` that the message printer reads.
pub mod prf {
    /// The question section.
    pub const QUES: u64 = 0x10;
    /// The answer section.
    pub const ANS: u64 = 0x20;
    /// The authority section.
    pub const AUTH: u64 = 0x40;
    /// The additional section.
    pub const ADD: u64 = 0x80;
    /// The counts (and the section headings).
    pub const HEAD1: u64 = 0x100;
    /// The flags.
    pub const HEAD2: u64 = 0x200;
    /// The opcode, status and ID line.
    pub const HEADX: u64 = 0x800;
}

/// `fp_nquery(msg)`: the reference's debugging dump of a message, `pfcode` choosing the parts
/// (0: the header, flags and counts and every record without headings; observed).
pub fn nquery(msg: &[u8], pfcode: u64, out: &mut TryVec<u8>) -> Result<()> {
    let p: Parsed = match msg::parse(msg) {
        Ok(p) => p,
        Err(_) => return out.extend_from_slice(b";; ns_initparse: Message too long\n"),
    };
    let h: Header = p.header;
    let all = pfcode == 0;
    let on = |bit: u64| all || pfcode & bit != 0;
    if on(prf::HEADX) {
        out.extend_from_slice(b";; ->>HEADER<<- opcode: ")?;
        Name::Static(OPCODES.get(usize::from(h.opcode())).copied().unwrap_or(b"?\0")).push_to(out)?;
        out.extend_from_slice(b", status: ")?;
        p_rcode(i32::from(h.rcode())).push_to(out)?;
        out.extend_from_slice(b", id: ")?;
        push_dec(out, u64::from(h.id))?;
        out.push(b'\n')?;
    }
    let update = h.opcode() == OP_UPDATE;
    let names: [&[u8]; 4] = if update { [b"ZONE", b"PREREQUISITE", b"UPDATE", b"ADDITIONAL"] } else { [b"QUERY", b"ANSWER", b"AUTHORITY", b"ADDITIONAL"] };
    if on(prf::HEADX) || on(prf::HEAD2) || on(prf::HEAD1) {
        if on(prf::HEADX) {
            out.push(b';')?;
        }
        if on(prf::HEAD2) {
            out.extend_from_slice(b"; flags:")?;
            let bits: [(u16, &[u8]); 8] = [(0x8000, b" qr"), (0x0400, b" aa"), (0x0200, b" tc"), (0x0100, b" rd"), (0x0080, b" ra"), (0x0040, b" ??"), (0x0020, b" ad"), (0x0010, b" cd")];
            for (b, n) in bits {
                if h.flags & b != 0 {
                    out.extend_from_slice(n)?;
                }
            }
        }
        if on(prf::HEAD1) {
            for (i, (n, c)) in names.iter().zip(h.counts).enumerate() {
                out.extend_from_slice(if i == 0 { b"; " } else { b", " })?;
                out.extend_from_slice(n)?;
                out.extend_from_slice(b": ")?;
                push_dec(out, u64::from(c))?;
            }
        }
        out.push(b'\n')?;
    }
    let headings = !all && pfcode & prf::HEAD1 != 0;
    let bits = [prf::QUES, prf::ANS, prf::AUTH, prf::ADD];
    for (s, (bit, title)) in bits.iter().zip(names).enumerate() {
        if !on(*bit) {
            continue;
        }
        let count = usize::from(h.counts.get(s).copied().unwrap_or(0));
        // An empty section has no heading (and no blank line after it), as the reference prints.
        let headings = headings && count > 0;
        if headings {
            out.extend_from_slice(b";; ")?;
            out.extend_from_slice(title)?;
            out.extend_from_slice(b" SECTION:\n")?;
        }
        for i in 0..count {
            let Ok(rr) = msg::rr(msg, &p, s, i) else { break };
            let mut text = [0u8; name::MAXDNAME];
            let n = name::expand_text(rr.owner(), &mut text)?;
            let owner = text.get(..n.saturating_sub(1)).unwrap_or(&[]);
            if s == 0 {
                out.extend_from_slice(b";;\t")?;
                out.extend_from_slice(owner)?;
                out.extend_from_slice(b", type = ")?;
                p_type(i32::from(rr.rtype)).push_to(out)?;
                out.extend_from_slice(b", class = ")?;
                p_class(i32::from(rr.class)).push_to(out)?;
                out.push(b'\n')?;
            } else {
                let rec = Record { owner, class: rr.class, rtype: rr.rtype, ttl: u64::from(rr.ttl), rdata: rr.data(msg), rdata_at: rr.rdata };
                if sprintrr(msg, &rec, None, None, out).is_ok() {
                    out.push(b'\n')?;
                }
            }
        }
        if headings {
            out.push(b'\n')?;
        }
    }
    Ok(())
}

/// `fp_resstat(state)`: the options line.
pub fn resstat(options: u64, out: &mut TryVec<u8>) -> Result<()> {
    out.extend_from_slice(b";; res options:")?;
    for bit in 0..32u32 {
        let o = 1u64 << bit;
        if options & o != 0 {
            out.push(b' ')?;
            p_option(o).push_to(out)?;
        }
    }
    out.push(b'\n')
}

// ------------------------------------------------------------------------------ LOC (RFC 1876)

/// The text of a size or precision byte (mantissa and power of ten, centimetres) in metres.
fn loc_size(b: u8, out: &mut TryVec<u8>) -> Result<()> {
    let cm = u64::from(b >> 4).saturating_mul(10u64.saturating_pow(u32::from(b & 0xf)));
    push_dec(out, cm / 100)?;
    out.push(b'.')?;
    push2(out, cm % 100)?;
    out.push(b'm')
}

fn push2(out: &mut TryVec<u8>, v: u64) -> Result<()> {
    out.push(b'0'.saturating_add(u8::try_from(v / 10 % 10).unwrap_or(0)))?;
    out.push(b'0'.saturating_add(u8::try_from(v % 10).unwrap_or(0)))
}

fn loc_angle(v: u32, pos: u8, neg: u8, out: &mut TryVec<u8>) -> Result<()> {
    let x = i64::from(v) - (1i64 << 31);
    let (ms, h) = if x < 0 { (x.unsigned_abs(), neg) } else { (x.unsigned_abs(), pos) };
    push_dec(out, ms / 3_600_000)?;
    out.push(b' ')?;
    push2(out, ms / 60_000 % 60)?;
    out.push(b' ')?;
    push2(out, ms / 1000 % 60)?;
    out.push(b'.')?;
    let f = ms % 1000;
    out.push(b'0'.saturating_add(u8::try_from(f / 100).unwrap_or(0)))?;
    push2(out, f % 100)?;
    out.push(b' ')?;
    out.push(h)
}

/// `loc_ntoa(binary)`: the RFC 1876 text of a `LOC` record's 16 bytes.
pub fn loc_ntoa(b: &[u8; 16], out: &mut TryVec<u8>) -> Result<()> {
    if b[0] != 0 {
        return out.extend_from_slice(b"; error: unknown LOC RR version");
    }
    let word = |i: usize| b.get(i..i + 4).and_then(|w| <[u8; 4]>::try_from(w).ok()).map_or(0, u32::from_be_bytes);
    loc_angle(word(4), b'N', b'S', out)?;
    out.push(b' ')?;
    loc_angle(word(8), b'E', b'W', out)?;
    out.push(b' ')?;
    let alt = i64::from(word(12)) - 10_000_000;
    if alt < 0 {
        out.push(b'-')?;
    }
    let a = alt.unsigned_abs();
    push_dec(out, a / 100)?;
    out.push(b'.')?;
    push2(out, a % 100)?;
    out.push(b'm')?;
    for &x in b.get(1..4).unwrap_or(&[]) {
        out.push(b' ')?;
        loc_size(x, out)?;
    }
    Ok(())
}

/// A decimal number with an optional fraction (`frac` digits kept, scaled) and sign.
fn loc_number(tok: &[u8], frac: u32) -> Option<i64> {
    let (neg, t) = match tok.strip_prefix(b"-") {
        Some(r) => (true, r),
        None => (false, tok),
    };
    let t = t.strip_suffix(b"m").unwrap_or(t);
    let (int, f) = match t.iter().position(|&c| c == b'.') {
        Some(p) => (t.get(..p)?, t.get(p.checked_add(1)?..)?),
        None => (t, &[][..]),
    };
    if int.is_empty() && f.is_empty() || !int.iter().chain(f).all(u8::is_ascii_digit) {
        return None;
    }
    let mut v = int.iter().try_fold(0i64, |v, &c| v.checked_mul(10)?.checked_add(i64::from(c.wrapping_sub(b'0'))))?;
    for k in 0..frac {
        let d = f.get(usize::try_from(k).ok()?).map_or(0, |&c| i64::from(c.wrapping_sub(b'0')));
        v = v.checked_mul(10)?.checked_add(d)?;
    }
    Some(if neg { -v } else { v })
}

/// The size byte for `cm` centimetres (the largest power of ten not above it).
fn loc_prec(cm: u64) -> u8 {
    let mut e = 0u8;
    let mut m = cm;
    while m >= 10 && e < 9 {
        m /= 10;
        e = e.saturating_add(1);
    }
    u8::try_from(m.min(9)).unwrap_or(9) << 4 | e
}

/// `loc_aton(text)`: the 16 bytes of the RFC 1876 text form, or `None`.
pub fn loc_aton(text: &[u8]) -> Option<[u8; 16]> {
    let toks: Vec<&[u8]> = text.split(|&c| c == b' ' || c == b'\t' || c == b'\n').filter(|t| !t.is_empty()).collect();
    let mut i = 0usize;
    let mut angle = |pos: u8, neg: u8| -> Option<u32> {
        let mut parts = [0i64; 3];
        let mut n = 0usize;
        loop {
            let t = *toks.get(i)?;
            i = i.checked_add(1)?;
            if t == [pos] || t == [neg] {
                if n == 0 {
                    return None;
                }
                let ms = parts[0].checked_mul(3_600_000)?.checked_add(parts[1].checked_mul(60_000)?)?.checked_add(parts[2])?;
                let v = if t == [pos] { (1i64 << 31).checked_add(ms)? } else { (1i64 << 31).checked_sub(ms)? };
                return u32::try_from(v.rem_euclid(1 << 32)).ok();
            }
            *parts.get_mut(n)? = if n == 2 { loc_number(t, 3)? } else { loc_number(t, 0)? };
            n = n.checked_add(1)?;
        }
    };
    let lat = angle(b'N', b'S')?;
    let lon = angle(b'E', b'W')?;
    let alt_cm = loc_number(toks.get(i)?, 2)?;
    i = i.checked_add(1)?;
    let mut sizes = [100u64, 1_000_000, 1000];
    for s in sizes.iter_mut() {
        let Some(t) = toks.get(i) else { break };
        *s = u64::try_from(loc_number(t, 2)?).ok()?;
        i = i.checked_add(1)?;
    }
    let alt = u32::try_from(alt_cm.checked_add(10_000_000)?.rem_euclid(1 << 32)).ok()?;
    let mut b = [0u8; 16];
    b[1] = loc_prec(sizes[0]);
    b[2] = loc_prec(sizes[1]);
    b[3] = loc_prec(sizes[2]);
    b.get_mut(4..8)?.copy_from_slice(&lat.to_be_bytes());
    b.get_mut(8..12)?.copy_from_slice(&lon.to_be_bytes());
    b.get_mut(12..16)?.copy_from_slice(&alt.to_be_bytes());
    Some(b)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn s(f: impl FnOnce(&mut TryVec<u8>) -> Result<()>) -> alloc::string::String {
        let mut t = TryVec::new();
        f(&mut t).unwrap();
        alloc::string::String::from_utf8(t.into_inner()).unwrap()
    }

    #[test]
    fn ttl() {
        assert_eq!(s(|o| format_ttl(0, o)), "0S");
        assert_eq!(s(|o| format_ttl(90061, o)), "1d1h1m1s");
        assert_eq!(s(|o| format_ttl(3600, o)), "1H");
        assert_eq!(parse_ttl(b"1W2D3H4M5S"), Some(788_645));
        assert_eq!(parse_ttl(b"1H30"), None);
        assert_eq!(parse_ttl(b"3600"), Some(3600));
        assert_eq!(datetosecs(b"20240101000000"), Some(1_704_067_200));
        assert_eq!(datetosecs(b"20230229000000"), Some(1_677_628_800));
        assert_eq!(datetosecs(b"19700101000000"), None);
        assert_eq!(&secstodate(1_704_067_200), b"20240101000000");
    }

    #[test]
    fn base64() {
        let mut o = [0u8; 32];
        assert_eq!(b64_ntop(b"Hello, world!", &mut o).unwrap(), 20);
        assert_eq!(&o[..20], b"SGVsbG8sIHdvcmxkIQ==");
        assert!(b64_ntop(b"A", &mut [0u8; 4]).is_err());
        assert_eq!(b64_pton(b" QU JD\n", Some(&mut o)).unwrap(), 3);
        assert!(b64_pton(b"QQ", None).is_err());
        assert!(b64_pton(b"QQ==QQ==", None).is_err());
        assert!(b64_pton(b"QUJD=", None).is_err());
    }

    #[test]
    fn loc() {
        let b = loc_aton(b"52 22 23.000 N 4 53 32.000 E -2.00m 0.00m 10000m 10m").unwrap();
        assert_eq!(b, [0, 0, 0x16, 0x13, 0x8b, 0x3c, 0xf0, 0x18, 0x81, 0x0c, 0xbc, 0xe0, 0, 0x98, 0x95, 0xb8]);
        assert_eq!(s(|o| loc_ntoa(&b, o)), "52 22 23.000 N 4 53 32.000 E -2.00m 0.00m 10000.00m 10.00m");
        assert!(loc_aton(b"bogus").is_none());
    }

    #[test]
    fn records() {
        let msg = b"\x01a\x07example\x00";
        let r = Record { owner: b"c.example", class: 1, rtype: 5, ttl: 3600, rdata: msg, rdata_at: Some(0) };
        assert_eq!(s(|o| sprintrr(msg, &r, None, None, o)), "c.example.\t\t1H IN CNAME\ta.example.");
        assert_eq!(s(|o| sprintrr(msg, &r, None, Some(b"example"), o)), "c\t\t\t1H IN CNAME\ta");
        let r = Record { owner: b"x.y", class: 1, rtype: 10, ttl: 60, rdata: b"\x01\x02\x03", rdata_at: None };
        assert_eq!(s(|o| sprintrr(msg, &r, None, None, o)), "x.y.\t\t\t1M IN NULL\t\\# 3 (\n\t01 02 03 )\t\t\t\t\t; ...");
    }
}
