//! Domain names (RFC 1035 §3.1, §4.1.4; spec §16.4 `dn_comp`, `dn_expand`, `ns_name_*`,
//! `res_hnok` and the other checks; D-16.3).
//!
//! **Forms.** A *text* name is labels separated by dots, with `\X` for a literal byte `X` and
//! `\DDD` for the byte of decimal value `DDD` (three digits, at most 255); a final dot makes it
//! fully qualified; `.` and the empty text are the root. A *wire* name is the labels as length
//! bytes followed by their bytes, ending with a zero byte: at most 63 bytes a label and 255 bytes
//! in all (RFC 1035 §2.3.4). In a message a name may end with a *pointer* (two bytes, the top two
//! bits set) to an earlier or later name of the message (compression, RFC 1035 §4.1.4); label
//! types `01` and `10` (the obsolete bit and extended labels) are not names this library reads.
//! Errors are `EMSGSIZE`, as the reference reports them.
//!
//! **Text from wire** (observed): `.`, `\`, `"`, `;`, `(`, `)`, `$` and `@` are escaped with a
//! backslash; bytes outside `!`..`~` as `\DDD`; no final dot; the root is `.`.
//!
//! **Compression** ([`pack`]): the longest suffix of the name that a name already in the
//! message ends with (labels compared without regard to ASCII case) becomes a pointer; the
//! names a caller lists are searched label by label, so a pointer may point into the middle of
//! one. A name written with at least one label of its own is added to the list.
//!
//! **Checks** (`res_hnok`, `res_ownok`, `res_mailok`, `res_dnok`; observed): the text is read as
//! a name first (escapes resolved; a text that is not a name fails every check); then a host
//! name has letters, digits, `-` and `_` only, and does not begin with `-`; an owner name may
//! also begin with a `*` label; a mail name is a first label of any printable ASCII (the local
//! part) followed by at least one label of a host name; a domain name has printable ASCII only.
//! The root and the empty text pass every check.

use crate::error::{Errno, Result};

/// The most bytes of a wire name.
pub const MAXCDNAME: usize = 255;
/// The most bytes of a label.
pub const MAXLABEL: usize = 63;
/// The most bytes of a text name with its terminator (`NS_MAXDNAME`).
pub const MAXDNAME: usize = 1025;

const E: Errno = Errno::EMSGSIZE;

/// Bytes written into a caller's buffer; running out is `EMSGSIZE`.
pub struct Out<'a> {
    buf: &'a mut [u8],
    len: usize,
}

impl<'a> Out<'a> {
    /// Writing to `buf` from its start.
    pub fn new(buf: &'a mut [u8]) -> Out<'a> {
        Out { buf, len: 0 }
    }

    /// One byte.
    pub fn push(&mut self, b: u8) -> Result<()> {
        *self.buf.get_mut(self.len).ok_or(E)? = b;
        self.len = self.len.checked_add(1).ok_or(E)?;
        Ok(())
    }

    /// Bytes.
    pub fn push_all(&mut self, bytes: &[u8]) -> Result<()> {
        let end = self.len.checked_add(bytes.len()).ok_or(E)?;
        self.buf.get_mut(self.len..end).ok_or(E)?.copy_from_slice(bytes);
        self.len = end;
        Ok(())
    }

    /// The bytes written so far.
    pub fn len(&self) -> usize {
        self.len
    }

    /// Whether nothing was written.
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The bytes written.
    pub fn written(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or(&[])
    }
}

// ------------------------------------------------------------------------------ text <-> wire

/// The wire form of the text name `text` into `out`: its length and whether the text was fully
/// qualified (ends with an unescaped dot, or is `.`).
pub fn pton(text: &[u8], out: &mut [u8]) -> Result<(usize, bool)> {
    let mut o = Out::new(out);
    if text == b"." {
        o.push(0)?;
        return Ok((1, true));
    }
    let mut label = [0u8; MAXLABEL];
    let mut n = 0usize;
    let mut i = 0usize;
    let mut fq = false;
    while let Some(&c) = text.get(i) {
        let b = match c {
            b'\\' => {
                let next = *text.get(i.saturating_add(1)).ok_or(E)?;
                if next.is_ascii_digit() {
                    let d = text.get(i.saturating_add(1)..i.saturating_add(4)).ok_or(E)?;
                    if !d.iter().all(u8::is_ascii_digit) {
                        return Err(E);
                    }
                    let v = d.iter().fold(0u32, |v, &x| v.saturating_mul(10).saturating_add(u32::from(x.wrapping_sub(b'0'))));
                    i = i.saturating_add(4);
                    u8::try_from(v).map_err(|_| E)?
                } else {
                    i = i.saturating_add(2);
                    next
                }
            }
            b'.' => {
                if n == 0 {
                    return Err(E);
                }
                put_label(&mut o, label.get(..n).unwrap_or(&[]))?;
                n = 0;
                i = i.saturating_add(1);
                if i == text.len() {
                    fq = true;
                }
                continue;
            }
            _ => {
                i = i.saturating_add(1);
                c
            }
        };
        *label.get_mut(n).ok_or(E)? = b;
        n = n.saturating_add(1);
    }
    if n > 0 {
        put_label(&mut o, label.get(..n).unwrap_or(&[]))?;
    }
    o.push(0)?;
    if o.len() > MAXCDNAME {
        return Err(E);
    }
    Ok((o.len(), fq))
}

fn put_label(o: &mut Out<'_>, label: &[u8]) -> Result<()> {
    o.push(u8::try_from(label.len()).map_err(|_| E)?)?;
    o.push_all(label)?;
    if o.len() >= MAXCDNAME {
        return Err(E);
    }
    Ok(())
}

/// The labels of a wire name (no pointers), or `None` when it is not one.
pub fn labels(wire: &[u8]) -> Option<Labels<'_>> {
    let mut at = 0usize;
    loop {
        let n = usize::from(*wire.get(at)?);
        if n == 0 {
            return Some(Labels { wire: wire.get(..=at)?, at: 0 });
        }
        if n > MAXLABEL {
            return None;
        }
        at = at.checked_add(n)?.checked_add(1)?;
    }
}

/// An iterator over the labels of a wire name.
#[derive(Clone)]
pub struct Labels<'a> {
    wire: &'a [u8],
    at: usize,
}

impl<'a> Labels<'a> {
    /// The wire name (with its final zero byte).
    pub fn wire(&self) -> &'a [u8] {
        self.wire
    }
}

impl<'a> Iterator for Labels<'a> {
    type Item = &'a [u8];
    fn next(&mut self) -> Option<&'a [u8]> {
        let n = usize::from(*self.wire.get(self.at)?);
        if n == 0 {
            return None;
        }
        let start = self.at.checked_add(1)?;
        let end = start.checked_add(n)?;
        self.at = end;
        self.wire.get(start..end)
    }
}

/// Append the text of one label (escaped) to `o`.
fn label_text(o: &mut Out<'_>, label: &[u8]) -> Result<()> {
    for &b in label {
        match b {
            b'.' | b'\\' | b'"' | b';' | b'(' | b')' | b'$' | b'@' => {
                o.push(b'\\')?;
                o.push(b)?;
            }
            0x21..=0x7e => o.push(b)?,
            _ => {
                o.push(b'\\')?;
                o.push(b'0'.saturating_add(b / 100))?;
                o.push(b'0'.saturating_add(b / 10 % 10))?;
                o.push(b'0'.saturating_add(b % 10))?;
            }
        }
    }
    Ok(())
}

/// The text of the wire name `wire` into `out`, NUL-terminated: the bytes written with the
/// terminator (`ns_name_ntop`).
pub fn ntop(wire: &[u8], out: &mut [u8]) -> Result<usize> {
    let mut o = Out::new(out);
    let first = *wire.first().ok_or(E)?;
    if first == 0 {
        o.push(b'.')?;
    } else {
        let mut at = 0usize;
        loop {
            let n = *wire.get(at).ok_or(E)?;
            if n == 0 {
                break;
            }
            if n & 0xc0 != 0 {
                return Err(E);
            }
            if at > 0 {
                o.push(b'.')?;
            }
            let start = at.checked_add(1).ok_or(E)?;
            let end = start.checked_add(usize::from(n)).ok_or(E)?;
            label_text(&mut o, wire.get(start..end).ok_or(E)?)?;
            at = end;
        }
    }
    o.push(0)?;
    Ok(o.len())
}

/// The text of a wire name as `dn_expand` gives it: the root is the empty text. The bytes
/// written with the terminator.
pub fn expand_text(wire: &[u8], out: &mut [u8]) -> Result<usize> {
    if wire.first() == Some(&0) {
        *out.first_mut().ok_or(E)? = 0;
        return Ok(1);
    }
    ntop(wire, out)
}

/// Lower-case (ASCII) copy of the wire name `wire` into `out`: its length (`ns_name_ntol`).
pub fn ntol(wire: &[u8], out: &mut [u8]) -> Result<usize> {
    let l = labels(wire).ok_or(E)?;
    let w = l.wire();
    let dst = out.get_mut(..w.len()).ok_or(E)?;
    for (d, s) in dst.iter_mut().zip(w) {
        *d = s.to_ascii_lowercase();
    }
    Ok(w.len())
}

// ------------------------------------------------------------------------------ messages

/// Read the possibly compressed name at `start` of the message `msg` into `out` as a wire
/// name: (its length, the bytes the name takes at `start`). Pointers may point anywhere in the
/// message; a loop or a name beyond the message or beyond 255 bytes is `EMSGSIZE`.
pub fn unpack(msg: &[u8], start: usize, out: &mut [u8]) -> Result<(usize, usize)> {
    let mut o = Out::new(out);
    let mut at = start;
    let mut used: Option<usize> = None;
    let mut seen = 0usize;
    loop {
        let n = *msg.get(at).ok_or(E)?;
        match n & 0xc0 {
            0 => {
                let n = usize::from(n);
                if n == 0 {
                    o.push(0)?;
                    let used = match used {
                        Some(u) => u,
                        None => at.checked_add(1).ok_or(E)?.checked_sub(start).ok_or(E)?,
                    };
                    if o.len() > MAXCDNAME {
                        return Err(E);
                    }
                    return Ok((o.len(), used));
                }
                let end = at.checked_add(n).ok_or(E)?.checked_add(1).ok_or(E)?;
                o.push_all(msg.get(at..end).ok_or(E)?)?;
                if o.len() >= MAXCDNAME {
                    return Err(E);
                }
                at = end;
            }
            0xc0 => {
                let lo = *msg.get(at.checked_add(1).ok_or(E)?).ok_or(E)?;
                if used.is_none() {
                    used = Some(at.checked_add(2).ok_or(E)?.checked_sub(start).ok_or(E)?);
                }
                let target = usize::from(n & 0x3f) << 8 | usize::from(lo);
                seen = seen.saturating_add(2);
                if target >= msg.len() || seen >= msg.len() {
                    return Err(E);
                }
                at = target;
            }
            _ => return Err(E),
        }
    }
}

/// The bytes the possibly compressed name at the start of `msg` takes (`dn_skipname`).
pub fn skip(msg: &[u8]) -> Result<usize> {
    let mut at = 0usize;
    loop {
        let n = *msg.get(at).ok_or(E)?;
        match n & 0xc0 {
            0 if n == 0 => return at.checked_add(1).ok_or(E),
            0 => {
                at = at.checked_add(usize::from(n)).ok_or(E)?.checked_add(1).ok_or(E)?;
                if at >= msg.len() {
                    return Err(E);
                }
            }
            0xc0 => {
                if at.checked_add(1).is_none_or(|x| x >= msg.len()) {
                    return Err(E);
                }
                return at.checked_add(2).ok_or(E);
            }
            _ => return Err(E),
        }
    }
}

/// Whether the name in `msg` at `at` (following pointers) equals the wire name `suffix`
/// (labels compared without regard to ASCII case).
fn equals_at(msg: &[u8], mut at: usize, suffix: &[u8]) -> bool {
    let mut s = 0usize;
    let mut hops = 0usize;
    loop {
        let Some(&n) = msg.get(at) else { return false };
        if n & 0xc0 == 0xc0 {
            let Some(&lo) = msg.get(at.saturating_add(1)) else { return false };
            hops = hops.saturating_add(1);
            if hops > 128 {
                return false;
            }
            at = usize::from(n & 0x3f) << 8 | usize::from(lo);
            continue;
        }
        if n & 0xc0 != 0 {
            return false;
        }
        let Some(&m) = suffix.get(s) else { return false };
        if n != m {
            return false;
        }
        if n == 0 {
            return true;
        }
        let len = usize::from(n);
        let (a0, s0) = (at.saturating_add(1), s.saturating_add(1));
        let (Some(a), Some(b)) = (msg.get(a0..a0.saturating_add(len)), suffix.get(s0..s0.saturating_add(len))) else { return false };
        if !a.eq_ignore_ascii_case(b) {
            return false;
        }
        at = a0.saturating_add(len);
        s = s0.saturating_add(len);
    }
}

/// The offset in `msg` of a label sequence equal to `suffix`, searched in the names that
/// start at `names` (and the names they point into).
fn find_suffix(msg: &[u8], names: &[usize], suffix: &[u8]) -> Option<usize> {
    for &start in names {
        let mut at = start;
        let mut hops = 0usize;
        loop {
            let n = *msg.get(at)?;
            if n & 0xc0 == 0xc0 {
                let lo = *msg.get(at.saturating_add(1))?;
                hops = hops.saturating_add(1);
                if hops > 128 {
                    break;
                }
                at = usize::from(n & 0x3f) << 8 | usize::from(lo);
                continue;
            }
            if n == 0 || n & 0xc0 != 0 {
                break;
            }
            if at < 0x4000 && equals_at(msg, at, suffix) {
                return Some(at);
            }
            at = at.saturating_add(usize::from(n)).saturating_add(1);
        }
    }
    None
}

/// Compression state for [`pack`]: the offsets (in the message) of the names written so far
/// (not the message's start, which the C list holds first), and how many may be listed.
pub struct Compress<'a> {
    /// The offsets of earlier names.
    pub names: &'a mut [usize],
    /// How many entries of `names` are in use.
    pub len: usize,
    /// Whether the new name may be added (the caller gave room for the list to grow).
    pub record: bool,
}

/// Write the wire name `wire` into `msg` at `at` (at most `limit` bytes), compressed against
/// the names of `comp` when given: the bytes written (`ns_name_pack`).
pub fn pack(wire: &[u8], msg: &mut [u8], at: usize, limit: usize, comp: Option<&mut Compress<'_>>) -> Result<usize> {
    let l = labels(wire).ok_or(E)?;
    let wire = l.wire();
    // The start of each label in `wire` (128 labels of at most 64 bytes: below 8192, so 16 bits
    // hold it and the table stays small to clear).
    let mut starts = [0u16; 128];
    let mut count = 0usize;
    let mut pos = 0usize;
    while let Some(&n) = wire.get(pos) {
        if n == 0 {
            break;
        }
        *starts.get_mut(count).ok_or(E)? = u16::try_from(pos).map_err(|_| E)?;
        count = count.saturating_add(1);
        pos = pos.saturating_add(usize::from(n)).saturating_add(1);
    }
    let mut pointer: Option<(usize, usize)> = None;
    if let Some(c) = &comp {
        let names = c.names.get(..c.len).unwrap_or(&[]);
        for (i, &s) in starts.get(..count).unwrap_or(&[]).iter().enumerate() {
            if let Some(p) = find_suffix(msg, names, wire.get(usize::from(s)..).unwrap_or(&[])) {
                pointer = Some((i, p));
                break;
            }
        }
    }
    let own = match pointer {
        Some((i, _)) => starts.get(i).copied().map_or(0, usize::from),
        None => wire.len(),
    };
    let total = own.checked_add(if pointer.is_some() { 2 } else { 0 }).ok_or(E)?;
    if total > limit {
        return Err(E);
    }
    let end = at.checked_add(total).ok_or(E)?;
    let dst = msg.get_mut(at..end).ok_or(E)?;
    let (head, tail) = dst.split_at_mut(own);
    head.copy_from_slice(wire.get(..own).ok_or(E)?);
    if let Some((_, p)) = pointer {
        let p = u16::try_from(p).map_err(|_| E)? | 0xc000;
        tail.copy_from_slice(&p.to_be_bytes());
    }
    if let Some(c) = comp {
        if own > 0 && c.record && at < 0x4000 && c.len < c.names.len() {
            if let Some(slot) = c.names.get_mut(c.len) {
                *slot = at;
                c.len = c.len.saturating_add(1);
            }
        }
    }
    Ok(total)
}

// ------------------------------------------------------------------------------ comparison

/// The wire form of a text name, or `None` when it is not one.
fn wire_of(text: &[u8]) -> Option<([u8; MAXCDNAME + 1], usize)> {
    let mut w = [0u8; MAXCDNAME + 1];
    let (n, _) = pton(text, &mut w).ok()?;
    Some((w, n))
}

/// Whether two wire names are equal without regard to ASCII case.
pub fn wire_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.eq_ignore_ascii_case(b)
}

/// `ns_samename(a, b)`: whether the text names are the same name (ASCII case and a final dot
/// ignored); an error when either is not a name.
pub fn samename(a: &[u8], b: &[u8]) -> Result<bool> {
    let ((wa, na), (wb, nb)) = (wire_of(a).ok_or(E)?, wire_of(b).ok_or(E)?);
    Ok(wire_eq(wa.get(..na).unwrap_or(&[]), wb.get(..nb).unwrap_or(&[])))
}

/// `ns_samedomain(a, b)`: whether `a` is `b` or a name below it (case ignored; the root
/// contains every name). False when either is not a name.
pub fn samedomain(a: &[u8], b: &[u8]) -> bool {
    let (Some((wa, na)), Some((wb, nb))) = (wire_of(a), wire_of(b)) else { return false };
    let (wa, wb) = (wa.get(..na).unwrap_or(&[]), wb.get(..nb).unwrap_or(&[]));
    // `b` must be a suffix of `a` that starts at a label boundary of `a`.
    let Some(ls) = labels(wa) else { return false };
    let mut at = 0usize;
    for l in ls.clone() {
        if wire_eq(wa.get(at..).unwrap_or(&[]), wb) {
            return true;
        }
        at = at.saturating_add(l.len()).saturating_add(1);
    }
    wire_eq(wa.get(at..).unwrap_or(&[]), wb)
}

/// `ns_subdomain(a, b)`: `a` is a name strictly below `b`.
pub fn subdomain(a: &[u8], b: &[u8]) -> bool {
    !matches!(samename(a, b), Ok(true)) && samedomain(a, b)
}

/// `ns_makecanon(src)`: the text with its final unescaped dots removed and one dot added, into
/// `out` (NUL-terminated). The bytes written with the terminator.
pub fn makecanon(src: &[u8], out: &mut [u8]) -> Result<usize> {
    let mut end = src.len();
    while end > 0 && src.get(end.saturating_sub(1)) == Some(&b'.') {
        // A dot after an odd number of backslashes is escaped.
        let slashes = src.get(..end.saturating_sub(1)).unwrap_or(&[]).iter().rev().take_while(|&&c| c == b'\\').count();
        if slashes % 2 == 1 {
            break;
        }
        end = end.saturating_sub(1);
    }
    let mut o = Out::new(out);
    o.push_all(src.get(..end).unwrap_or(&[]))?;
    o.push(b'.')?;
    o.push(0)?;
    Ok(o.len())
}

/// `dn_count_labels(name)`: the labels of a text name, a leading `*` label not counted, a
/// final dot ignored (observed: the root `.` counts one).
pub fn count_labels(name: &[u8]) -> i32 {
    if name.is_empty() {
        return 0;
    }
    let dots = name.iter().filter(|&&c| c == b'.').count();
    let mut n = dots.saturating_add(1);
    if name.last() == Some(&b'.') {
        n = n.saturating_sub(1);
    }
    if name.first() == Some(&b'*') {
        n = n.saturating_sub(1);
    }
    i32::try_from(n).unwrap_or(i32::MAX)
}

// ------------------------------------------------------------------------------ checks

/// The four name checks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Check {
    /// `res_hnok`: a host name.
    Host,
    /// `res_ownok`: an owner name (a host name, possibly below a `*` label).
    Owner,
    /// `res_mailok`: a mail name (`local.host.name`).
    Mail,
    /// `res_dnok`: a domain name of printable ASCII.
    Domain,
}

fn host_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-' || b == b'_'
}

fn host_labels(mut ls: Labels<'_>, first_rule: bool) -> bool {
    let mut first = first_rule;
    ls.all(|l| {
        let ok = l.iter().all(|&b| host_byte(b)) && !(first && l.first() == Some(&b'-'));
        first = false;
        ok
    })
}

/// Whether the text `name` passes `check` (see the module documentation).
pub fn check(name: &[u8], check: Check) -> bool {
    let Some((w, n)) = wire_of(name) else { return false };
    let Some(ls) = labels(w.get(..n).unwrap_or(&[])) else { return false };
    match check {
        Check::Host => host_labels(ls, true),
        Check::Owner => {
            let mut rest = ls.clone();
            if rest.next() == Some(b"*".as_slice()) { host_labels(rest, true) } else { host_labels(ls, true) }
        }
        Check::Mail => {
            let mut rest = ls.clone();
            match rest.next() {
                None => true,
                Some(local) => local.iter().all(|&b| (0x21..=0x7e).contains(&b)) && rest.clone().next().is_some() && host_labels(rest, false),
            }
        }
        Check::Domain => ls.clone().all(|l| l.iter().all(|&b| (0x21..=0x7e).contains(&b))),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn w(text: &[u8]) -> alloc::vec::Vec<u8> {
        let mut b = [0u8; 256];
        let (n, _) = pton(text, &mut b).unwrap();
        b[..n].to_vec()
    }

    #[test]
    fn text_forms() {
        assert_eq!(w(b"a.b"), b"\x01a\x01b\x00");
        assert_eq!(pton(b"a.b.", &mut [0; 16]).unwrap(), (5, true));
        assert_eq!(pton(b"", &mut [0; 16]).unwrap(), (1, false));
        assert!(pton(b"a..b", &mut [0; 16]).is_err());
        assert!(pton(b"a\\256", &mut [0; 16]).is_err());
        assert!(pton(b"x\\0", &mut [0; 16]).is_err());
        assert_eq!(w(b"\\065bc"), b"\x03Abc\x00");
        let mut t = [0u8; 64];
        let n = ntop(b"\x05a \";\x7f\x03()$\x02@.\x00", &mut t).unwrap();
        assert_eq!(&t[..n], b"a\\032\\\"\\;\\127.\\(\\)\\$.\\@\\.\0");
        assert!(ntop(b"\x41\x08\xff\x00", &mut t).is_err());
    }

    #[test]
    fn compression() {
        let mut msg = [0u8; 128];
        let mut names = [0usize; 8];
        let mut c = Compress { names: &mut names, len: 0, record: true };
        let a = pack(&w(b"www.example.com"), &mut msg, 12, 100, Some(&mut c)).unwrap();
        let b = pack(&w(b"mail.example.com"), &mut msg, 12 + a, 100, Some(&mut c)).unwrap();
        let d = pack(&w(b"example.com"), &mut msg, 12 + a + b, 100, Some(&mut c)).unwrap();
        assert_eq!((a, b, d), (17, 7, 2));
        assert_eq!(&msg[12 + a..12 + a + b + d], b"\x04mail\xc0\x10\xc0\x10");
        let used = c.len;
        assert_eq!(&names[..used], &[12, 29]);
        let mut out = [0u8; 256];
        assert_eq!(unpack(&msg, 12 + a, &mut out).unwrap(), (18, 7));
        assert!(unpack(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xc0, 12], 12, &mut out).is_err());
    }

    #[test]
    fn checks() {
        assert!(check(b"ex-ample.com", Check::Host));
        assert!(!check(b"-example.com", Check::Host));
        assert!(check(b"a.-b", Check::Host));
        assert!(check(b"*.example.com", Check::Owner));
        assert!(!check(b"a.*", Check::Owner));
        assert!(check(b"foo@bar.example", Check::Mail));
        assert!(!check(b"a", Check::Mail));
        assert!(check(b"", Check::Mail));
        assert!(check(b"a/b", Check::Domain));
        assert!(!check(b"a\\ b", Check::Domain));
        assert!(samedomain(b"A.EXAMPLE.com", b"example.COM"));
        assert!(!samedomain(b"aexample.com", b"example.com"));
        assert_eq!(count_labels(b"*.b.c"), 2);
    }
}
