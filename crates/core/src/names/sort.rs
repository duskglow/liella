//! RFC 6724 destination address selection (spec §16.2: `getaddrinfo` sorts its results), and
//! the policy tables of `gai.conf(5)`.
//!
//! Every address is handled in its IPv6 form (IPv4 as `::ffff:a.b.c.d`, RFC 6724 §2.1). The
//! caller gives each destination the source address the system would use for it (`None`: no
//! route, so the destination is unusable) and that source's prefix length. The rules applied
//! (§6), in order: 1 avoid unusable destinations; 2 prefer matching scope; 5 prefer matching
//! label; 6 prefer higher precedence; 8 prefer smaller scope; 9 longest matching prefix (IPv6
//! destinations only, the common prefix limited to the source's prefix length); 10 keep the
//! order. Rules 3 (deprecated sources), 4 (home addresses) and 7 (native transport) need
//! knowledge the interface list does not carry, and are not applied.
//!
//! **Tables.** The defaults are RFC 6724 §2.1's policy table and §3.2's IPv4 scopes. A
//! `gai.conf` with any `label` line replaces the whole label table, and likewise for
//! `precedence` and `scopev4` (`gai.conf(5)`); `reload yes` asks for the file to be re-read when
//! it changes (the default is to read it once).

use super::is_space;
use crate::net::addr;

/// One policy entry: a prefix and its value.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Entry {
    prefix: [u8; 16],
    len: u8,
    value: i32,
}

/// The most entries one table holds (more lines are ignored).
pub const MAX_ENTRIES: usize = 32;

/// A policy table (longest prefix wins).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Table {
    entries: [Entry; MAX_ENTRIES],
    n: usize,
}

const fn e(prefix: [u8; 16], len: u8, value: i32) -> Entry {
    Entry { prefix, len, value }
}

const Z: [u8; 16] = [0; 16];
const LOOPBACK: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1];
const MAPPED: [u8; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, 0, 0, 0, 0];

const fn p(a: u8, b: u8) -> [u8; 16] {
    [a, b, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
}

// A `const fn` loop: `i` stays below both lengths.
#[allow(clippy::indexing_slicing, clippy::arithmetic_side_effects)]
const fn table(list: &[Entry]) -> Table {
    let mut t = Table { entries: [e(Z, 0, 0); MAX_ENTRIES], n: 0 };
    let mut i = 0;
    while i < list.len() && i < MAX_ENTRIES {
        t.entries[i] = list[i];
        i += 1;
    }
    t.n = i;
    t
}

/// RFC 6724 §2.1: the default precedences.
pub const PRECEDENCE: Table = table(&[
    e(LOOPBACK, 128, 50),
    e(Z, 0, 40),
    e(MAPPED, 96, 35),
    e(p(0x20, 0x02), 16, 30),
    e(p(0x20, 0x01), 32, 5),
    e(p(0xfc, 0), 7, 3),
    e(Z, 96, 1),
    e(p(0xfe, 0xc0), 10, 1),
    e(p(0x3f, 0xfe), 16, 1),
]);

/// RFC 6724 §2.1: the default labels.
pub const LABEL: Table = table(&[
    e(LOOPBACK, 128, 0),
    e(Z, 0, 1),
    e(MAPPED, 96, 4),
    e(p(0x20, 0x02), 16, 2),
    e(p(0x20, 0x01), 32, 5),
    e(p(0xfc, 0), 7, 13),
    e(Z, 96, 3),
    e(p(0xfe, 0xc0), 10, 11),
    e(p(0x3f, 0xfe), 16, 12),
]);

const fn v4(a: u8, b: u8) -> [u8; 16] {
    [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, a, b, 0, 0]
}

/// RFC 6724 §3.2: IPv4 loopback and link-local addresses have link-local scope (2), the rest
/// global scope (14).
pub const SCOPEV4: Table = table(&[e(v4(127, 0), 104, 2), e(v4(169, 254), 112, 2), e(MAPPED, 96, 14)]);

impl Table {
    /// The value of the longest prefix covering `a`.
    pub fn lookup(&self, a: &[u8; 16]) -> Option<i32> {
        self.entries.get(..self.n)?.iter().filter(|x| common_prefix(&x.prefix, a) >= usize::from(x.len)).max_by_key(|x| x.len).map(|x| x.value)
    }

    fn push(&mut self, x: Entry) {
        if let Some(slot) = self.entries.get_mut(self.n) {
            *slot = x;
            self.n = self.n.saturating_add(1);
        }
    }

    fn encode(&self, out: &mut [u8]) {
        if let Some((n, rest)) = out.split_first_mut() {
            *n = u8::try_from(self.n).unwrap_or(0);
            for (chunk, x) in rest.chunks_exact_mut(ENTRY_BYTES).zip(self.entries.get(..self.n).unwrap_or(&[])) {
                let v = x.value.to_ne_bytes();
                for (d, s) in chunk.iter_mut().zip(x.prefix.iter().chain([&x.len]).chain(v.iter())) {
                    *d = *s;
                }
            }
        }
    }

    fn decode(bytes: &[u8]) -> Option<Table> {
        let (&n, rest) = bytes.split_first()?;
        let mut t = table(&[]);
        for chunk in rest.chunks_exact(ENTRY_BYTES).take(usize::from(n)) {
            t.push(e(chunk.get(..16)?.try_into().ok()?, *chunk.get(16)?, i32::from_ne_bytes(chunk.get(17..21)?.try_into().ok()?)));
        }
        (t.n == usize::from(n)).then_some(t)
    }
}

const ENTRY_BYTES: usize = 21;
const TABLE_BYTES: usize = 1 + MAX_ENTRIES * ENTRY_BYTES;
/// The bytes of an encoded [`Policy`].
pub const POLICY_BYTES: usize = 3 * TABLE_BYTES + 1;

/// The leading bits two addresses share.
pub fn common_prefix(a: &[u8; 16], b: &[u8; 16]) -> usize {
    let mut n = 0usize;
    for (x, y) in a.iter().zip(b) {
        let d = x ^ y;
        if d != 0 {
            return n.saturating_add(usize::try_from(d.leading_zeros()).unwrap_or(0));
        }
        n = n.saturating_add(8);
    }
    n
}

/// The policy `gai.conf` gives.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Policy {
    /// The precedence table.
    pub precedence: Table,
    /// The label table.
    pub label: Table,
    /// The IPv4 scope table.
    pub scopev4: Table,
    /// `reload yes`.
    pub reload: bool,
}

impl Default for Policy {
    fn default() -> Policy {
        Policy::DEFAULT
    }
}

/// An address and prefix length (`::/0`, `2002::/16`, `::ffff:169.254.0.0/112`).
fn prefix(text: &[u8]) -> Option<([u8; 16], u8)> {
    let slash = text.iter().position(|&c| c == b'/')?;
    let a = addr::parse_v6(text.get(..slash)?)?;
    let bits = text.get(slash.checked_add(1)?..)?;
    let mut len: u32 = 0;
    for &c in bits {
        len = len.checked_mul(10)?.checked_add(char::from(c).to_digit(10)?)?;
    }
    if bits.is_empty() || len > 128 {
        return None;
    }
    Some((a, u8::try_from(len).ok()?))
}

fn value(text: &[u8]) -> Option<i32> {
    let (neg, digits) = match text.strip_prefix(b"-") {
        Some(d) => (true, d),
        None => (false, text),
    };
    let mut v: i32 = 0;
    for &c in digits {
        v = v.checked_mul(10)?.checked_add(i32::try_from(char::from(c).to_digit(10)?).ok()?)?;
    }
    if digits.is_empty() {
        return None;
    }
    Some(if neg { v.checked_neg()? } else { v })
}

impl Policy {
    /// RFC 6724's tables, read once.
    pub const DEFAULT: Policy = Policy { precedence: PRECEDENCE, label: LABEL, scopev4: SCOPEV4, reload: false };

    /// The policy a `gai.conf` text gives (lines that do not parse are ignored).
    pub fn parse(text: &[u8]) -> Policy {
        let empty = table(&[]);
        let (mut prec, mut label, mut scope) = (empty, empty, empty);
        let mut reload = false;
        for line in text.split(|&c| c == b'\n') {
            let line = line.get(..line.iter().position(|&c| c == b'#').unwrap_or(line.len())).unwrap_or(&[]);
            let mut w = line.split(|&c| is_space(c)).filter(|x| !x.is_empty());
            let (Some(key), arg) = (w.next(), w.next()) else { continue };
            let entry = |arg: Option<&[u8]>, v: Option<&[u8]>| -> Option<Entry> {
                let (p, len) = prefix(arg?)?;
                Some(e(p, len, value(v?)?))
            };
            match key {
                b"precedence" => {
                    if let Some(x) = entry(arg, w.next()) {
                        prec.push(x);
                    }
                }
                b"label" => {
                    if let Some(x) = entry(arg, w.next()) {
                        label.push(x);
                    }
                }
                b"scopev4" => {
                    if let Some(x) = entry(arg, w.next()) {
                        scope.push(x);
                    }
                }
                b"reload" => reload = arg == Some(b"yes"),
                _ => {}
            }
        }
        Policy {
            precedence: if prec.n > 0 { prec } else { PRECEDENCE },
            label: if label.n > 0 { label } else { LABEL },
            scopev4: if scope.n > 0 { scope } else { SCOPEV4 },
            reload,
        }
    }

    /// The scope of `a` (RFC 6724 §3.1, §3.2): multicast by its scope field; link-local
    /// unicast and the loopback address 2; site-local 5; IPv4 by the IPv4 table; else 14.
    pub fn scope(&self, a: &[u8; 16]) -> i32 {
        match *a {
            [0xff, s, ..] => i32::from(s & 0xf),
            [0xfe, b, ..] if b & 0xc0 == 0x80 => 2,
            [0xfe, b, ..] if b & 0xc0 == 0xc0 => 5,
            _ if *a == LOOPBACK => 2,
            [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, ..] => self.scopev4.lookup(a).unwrap_or(14),
            _ => 14,
        }
    }

    /// The bytes of the policy, for process-wide storage in atomics.
    pub fn encode(&self) -> [u8; POLICY_BYTES] {
        let mut out = [0u8; POLICY_BYTES];
        for (chunk, t) in out.chunks_exact_mut(TABLE_BYTES).zip([&self.precedence, &self.label, &self.scopev4]) {
            t.encode(chunk);
        }
        if let Some(r) = out.last_mut() {
            *r = u8::from(self.reload);
        }
        out
    }

    /// The policy [`Policy::encode`] gave `bytes`.
    pub fn decode(bytes: &[u8]) -> Option<Policy> {
        let mut t = bytes.chunks_exact(TABLE_BYTES);
        Some(Policy { precedence: Table::decode(t.next()?)?, label: Table::decode(t.next()?)?, scopev4: Table::decode(t.next()?)?, reload: bytes.get(POLICY_BYTES.checked_sub(1)?) == Some(&1) })
    }

    fn precedence_of(&self, a: &[u8; 16]) -> i32 {
        self.precedence.lookup(a).unwrap_or(0)
    }

    fn label_of(&self, a: &[u8; 16]) -> i32 {
        self.label.lookup(a).unwrap_or(1)
    }
}

/// A destination to sort.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Candidate {
    /// The destination, in IPv6 form.
    pub dest: [u8; 16],
    /// The source the system would use, in IPv6 form, and its prefix length; `None` when there
    /// is no route.
    pub source: Option<([u8; 16], u8)>,
}

fn is_mapped(a: &[u8; 16]) -> bool {
    a.get(..12) == MAPPED.get(..12)
}

/// Whether `da` comes before `db` (RFC 6724 §6), `None` when the rules applied do not decide.
fn prefer(policy: &Policy, da: &Candidate, db: &Candidate) -> Option<bool> {
    // Rule 1: avoid unusable destinations.
    let (sa, sb) = match (da.source, db.source) {
        (Some(_), None) => return Some(true),
        (None, Some(_)) => return Some(false),
        (a, b) => (a, b),
    };
    let decide = |a: bool, b: bool| -> Option<bool> { (a != b).then_some(a) };
    if let (Some((sa, _)), Some((sb, _))) = (sa, sb) {
        // Rule 2: prefer matching scope.
        if let Some(r) = decide(policy.scope(&da.dest) == policy.scope(&sa), policy.scope(&db.dest) == policy.scope(&sb)) {
            return Some(r);
        }
        // Rule 5: prefer matching label.
        if let Some(r) = decide(policy.label_of(&da.dest) == policy.label_of(&sa), policy.label_of(&db.dest) == policy.label_of(&sb)) {
            return Some(r);
        }
    }
    // Rule 6: prefer higher precedence.
    let (pa, pb) = (policy.precedence_of(&da.dest), policy.precedence_of(&db.dest));
    if pa != pb {
        return Some(pa > pb);
    }
    // Rule 8: prefer smaller scope.
    let (ca, cb) = (policy.scope(&da.dest), policy.scope(&db.dest));
    if ca != cb {
        return Some(ca < cb);
    }
    // Rule 9: longest matching prefix, IPv6 only.
    if let (Some((sa, la)), Some((sb, lb))) = (sa, sb) {
        if !is_mapped(&da.dest) && !is_mapped(&db.dest) {
            let a = common_prefix(&sa, &da.dest).min(usize::from(la));
            let b = common_prefix(&sb, &db.dest).min(usize::from(lb));
            if a != b {
                return Some(a > b);
            }
        }
    }
    None
}

/// Sort `items` by their candidates (`key`), stably (rule 10).
pub fn sort<T>(policy: &Policy, items: &mut [T], key: impl Fn(&T) -> Candidate) {
    // An insertion sort: the lists are short, and it is stable.
    let n = items.len();
    let mut i = 1usize;
    while i < n {
        let mut j = i;
        while j > 0 {
            let (Some(a), Some(b)) = (items.get(j.wrapping_sub(1)), items.get(j)) else { break };
            if prefer(policy, &key(b), &key(a)) == Some(true) {
                items.swap(j.wrapping_sub(1), j);
                j = j.wrapping_sub(1);
            } else {
                break;
            }
        }
        i = i.saturating_add(1);
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn a(t: &str) -> [u8; 16] {
        addr::parse_v6(t.as_bytes()).unwrap()
    }

    #[test]
    fn rules() {
        let pol = Policy::DEFAULT;
        // Loopbacks with sources: ::1 (precedence 50) before 127.0.0.1 (35).
        let mut v = [Candidate { dest: a("::ffff:127.0.0.1"), source: Some((a("::ffff:127.0.0.1"), 104)) }, Candidate { dest: a("::1"), source: Some((a("::1"), 128)) }];
        sort(&pol, &mut v, |c| *c);
        assert_eq!(v[0].dest, a("::1"));
        // Unusable both: precedence decides.
        let mut v = [Candidate { dest: a("::ffff:192.0.2.4"), source: None }, Candidate { dest: a("2001:db8::10"), source: None }];
        sort(&pol, &mut v, |c| *c);
        assert_eq!(v[0].dest, a("2001:db8::10"));
        // A usable one first.
        let mut v = [Candidate { dest: a("2001:db8::10"), source: None }, Candidate { dest: a("::ffff:192.0.2.4"), source: Some((a("::ffff:192.0.2.9"), 120)) }];
        sort(&pol, &mut v, |c| *c);
        assert_eq!(v[0].dest, a("::ffff:192.0.2.4"));
        assert_eq!(pol.scope(&a("fe80::1")), 2);
        assert_eq!(pol.scope(&a("ff05::1")), 5);
        assert_eq!(pol.scope(&a("::ffff:169.254.1.1")), 2);
        assert_eq!(pol.scope(&a("::ffff:192.0.2.1")), 14);
    }

    #[test]
    fn gai_conf() {
        let p = Policy::parse(b"# x\nprecedence ::ffff:0:0/96 100\nreload yes\nlabel bad\n");
        assert_eq!(p.precedence.lookup(&a("::ffff:1.2.3.4")), Some(100));
        assert_eq!(p.precedence.lookup(&a("::1")), None);
        assert_eq!(p.label, LABEL);
        assert!(p.reload);
    }
}
