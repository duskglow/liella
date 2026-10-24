//! The resolver configuration (spec §16.4 Initialization; `resolv.conf(5)`; D-16.3).
//!
//! **`resolv.conf`.** One keyword per line, at the start of the line; `#` or `;` in the first
//! column starts a comment. `nameserver ADDRESS` (IPv4, or IPv6 with an optional `%interface`
//! or `%index` zone; anything after the address is ignored, an address that does not parse is
//! skipped; port 53). `domain NAME` and `search NAME...` set the search list, the last such line
//! winning. `sortlist ADDRESS[/MASK]...` (the mask as `inet_aton` reads it, observed: `/8` is
//! the mask `0.0.0.8`; without a mask the address's class mask). `options OPTION...`, lines
//! adding up: `ndots:N` (at most 15), `timeout:N` (at most 30), `attempts:N` (at most 5),
//! `rotate`, `edns0`, `single-request`, `single-request-reopen`, `no-tld-query`, `use-vc`,
//! `no-reload`, `trust-ad`, `no-aaaa`; `inet6`, `debug` and unknown options have no effect
//! (observed: `inet6` is obsolete and `debug` needs a debugging build).
//!
//! **Defaults.** No server: 127.0.0.1. No search line: the domain of the host name (after its
//! first dot), if any. `ndots` 1, `timeout` 5, `attempts` 2, options `RES_INIT`, `RES_RECURSE`,
//! `RES_DEFNAMES`, `RES_DNSRCH`.
//!
//! **Environment.** `LOCALDOMAIN` replaces the search list (white-space separated; empty: no
//! list); `RES_OPTIONS` adds options after the file's. P-lift: the lists are unbounded here; the
//! exported state shows the first 3 servers, 6 search domains and 10 sort pairs (D-16.3).

use crate::error::Result;
use crate::mem::TryVec;
use crate::names::Ip;
use crate::net::addr;

/// `RES_INIT`.
pub const RES_INIT: u64 = 0x1;
/// `RES_DEBUG`.
pub const RES_DEBUG: u64 = 0x2;
/// `RES_USEVC`.
pub const RES_USEVC: u64 = 0x8;
/// `RES_IGNTC`.
pub const RES_IGNTC: u64 = 0x20;
/// `RES_RECURSE`.
pub const RES_RECURSE: u64 = 0x40;
/// `RES_DEFNAMES`.
pub const RES_DEFNAMES: u64 = 0x80;
/// `RES_STAYOPEN`.
pub const RES_STAYOPEN: u64 = 0x100;
/// `RES_DNSRCH`.
pub const RES_DNSRCH: u64 = 0x200;
/// `RES_NOALIASES`.
pub const RES_NOALIASES: u64 = 0x1000;
/// `RES_ROTATE`.
pub const RES_ROTATE: u64 = 0x4000;
/// `RES_USE_EDNS0`.
pub const RES_USE_EDNS0: u64 = 0x10_0000;
/// `RES_SNGLKUP`.
pub const RES_SNGLKUP: u64 = 0x20_0000;
/// `RES_SNGLKUPREOP`.
pub const RES_SNGLKUPREOP: u64 = 0x40_0000;
/// `RES_USE_DNSSEC`.
pub const RES_USE_DNSSEC: u64 = 0x80_0000;
/// `RES_NOTLDQUERY`.
pub const RES_NOTLDQUERY: u64 = 0x100_0000;
/// `RES_NORELOAD`.
pub const RES_NORELOAD: u64 = 0x200_0000;
/// `RES_TRUSTAD`.
pub const RES_TRUSTAD: u64 = 0x400_0000;
/// `RES_NOAAAA`.
pub const RES_NOAAAA: u64 = 0x800_0000;
/// `RES_DEFAULT`.
pub const RES_DEFAULT: u64 = RES_RECURSE | RES_DEFNAMES | RES_DNSRCH;

/// `RES_TIMEOUT`.
pub const DEFAULT_TIMEOUT: u32 = 5;
/// `RES_DFLRETRY`.
pub const DEFAULT_ATTEMPTS: u32 = 2;
/// `RES_MAXNDOTS`.
pub const MAX_NDOTS: u32 = 15;
/// `RES_MAXRETRANS`.
pub const MAX_TIMEOUT: u32 = 30;
/// `RES_MAXRETRY`.
pub const MAX_ATTEMPTS: u32 = 5;
/// The DNS port.
pub const PORT: u16 = 53;

/// A name server.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Server {
    /// Its address.
    pub ip: Ip,
    /// Its port.
    pub port: u16,
    /// The IPv6 scope (`sin6_scope_id`).
    pub scope: u32,
}

/// A resolver configuration.
#[derive(Debug)]
pub struct Config {
    /// The servers, in order.
    pub servers: TryVec<Server>,
    /// The search list.
    pub search: TryVec<TryVec<u8>>,
    /// The sort list: (address, mask), network order.
    pub sort: TryVec<([u8; 4], [u8; 4])>,
    /// `ndots`.
    pub ndots: u32,
    /// `timeout`, seconds.
    pub timeout: u32,
    /// `attempts`.
    pub attempts: u32,
    /// The `RES_*` options.
    pub options: u64,
}

impl Default for Config {
    fn default() -> Config {
        Config::defaults()
    }
}

impl Config {
    /// The built-in defaults (no server, no search list).
    pub fn defaults() -> Config {
        Config { servers: TryVec::new(), search: TryVec::new(), sort: TryVec::new(), ndots: 1, timeout: DEFAULT_TIMEOUT, attempts: DEFAULT_ATTEMPTS, options: RES_INIT | RES_DEFAULT }
    }

    /// Apply the options of one `options` line or of `RES_OPTIONS`.
    pub fn apply_options(&mut self, text: &[u8]) {
        for opt in words(text) {
            let num = |prefix: &[u8], max: u32| -> Option<u32> {
                let v = opt.strip_prefix(prefix)?;
                if v.is_empty() || !v.iter().all(u8::is_ascii_digit) {
                    return None;
                }
                Some(v.iter().fold(0u32, |n, &c| n.saturating_mul(10).saturating_add(u32::from(c.wrapping_sub(b'0')))).min(max))
            };
            if let Some(n) = num(b"ndots:", MAX_NDOTS) {
                self.ndots = n;
            } else if let Some(n) = num(b"timeout:", MAX_TIMEOUT) {
                self.timeout = n;
            } else if let Some(n) = num(b"attempts:", MAX_ATTEMPTS) {
                self.attempts = n;
            } else {
                let bit = match opt {
                    b"rotate" => RES_ROTATE,
                    b"edns0" => RES_USE_EDNS0,
                    b"single-request" => RES_SNGLKUP,
                    b"single-request-reopen" => RES_SNGLKUPREOP,
                    b"no-tld-query" => RES_NOTLDQUERY,
                    b"use-vc" => RES_USEVC,
                    b"no-reload" => RES_NORELOAD,
                    b"trust-ad" => RES_TRUSTAD,
                    b"no-aaaa" => RES_NOAAAA,
                    _ => 0,
                };
                self.options |= bit;
            }
        }
    }

    /// Replace the search list with the words of `text`.
    pub fn set_search(&mut self, text: &[u8]) -> Result<()> {
        self.search.clear();
        for w in words(text) {
            self.search.push(TryVec::from_slice(w)?)?;
        }
        Ok(())
    }

    /// The configuration of a `resolv.conf` text (`None`: no file), the environment's
    /// `LOCALDOMAIN` and `RES_OPTIONS`, and the host name (for the default search list).
    /// `zone` gives the index of an interface named in an IPv6 zone.
    pub fn parse(text: Option<&[u8]>, localdomain: Option<&[u8]>, res_options: Option<&[u8]>, hostname: &[u8], zone: impl Fn(&[u8]) -> Option<u32>) -> Result<Config> {
        let mut c = Config::defaults();
        let mut searched = false;
        for line in text.unwrap_or(&[]).split(|&b| b == b'\n') {
            if matches!(line.first(), Some(b'#' | b';')) {
                continue;
            }
            let mut w = words(line);
            let Some(key) = w.next() else { continue };
            let rest = line.get(key.len()..).unwrap_or(&[]);
            match key {
                b"nameserver" => {
                    if let Some(s) = w.next().and_then(|a| server(a, &zone)) {
                        c.servers.push(s)?;
                    }
                }
                b"domain" => {
                    c.search.clear();
                    if let Some(d) = w.next() {
                        c.search.push(TryVec::from_slice(d)?)?;
                    }
                    searched = true;
                }
                b"search" => {
                    c.set_search(rest)?;
                    searched = true;
                }
                b"sortlist" => {
                    for item in w {
                        if let Some(p) = sort_pair(item) {
                            c.sort.push(p)?;
                        }
                    }
                }
                b"options" => c.apply_options(rest),
                _ => {}
            }
        }
        if !searched {
            if let Some(dot) = hostname.iter().position(|&b| b == b'.') {
                let d = hostname.get(dot.saturating_add(1)..).unwrap_or(&[]);
                if !d.is_empty() {
                    c.search.push(TryVec::from_slice(d)?)?;
                }
            }
        }
        if let Some(l) = localdomain {
            c.set_search(l)?;
        }
        if let Some(o) = res_options {
            c.apply_options(o);
        }
        if c.servers.is_empty() {
            c.servers.push(Server { ip: Ip::V4([127, 0, 0, 1]), port: PORT, scope: 0 })?;
        }
        Ok(c)
    }
}

/// The white-space separated words of `text`.
pub fn words(text: &[u8]) -> impl Iterator<Item = &[u8]> + Clone {
    text.split(|&b| matches!(b, b' ' | b'\t' | b'\r' | b'\n' | 0x0b | 0x0c)).filter(|w| !w.is_empty())
}

fn server(text: &[u8], zone: &impl Fn(&[u8]) -> Option<u32>) -> Option<Server> {
    if let Some(v4) = addr::parse_v4(text) {
        return Some(Server { ip: Ip::V4(v4), port: PORT, scope: 0 });
    }
    let (a, z) = match text.iter().position(|&b| b == b'%') {
        Some(p) => (text.get(..p)?, Some(text.get(p.checked_add(1)?..)?)),
        None => (text, None),
    };
    let v6 = addr::parse_v6(a)?;
    let scope = match z {
        None => 0,
        Some(z) if !z.is_empty() && z.iter().all(u8::is_ascii_digit) => z.iter().try_fold(0u32, |n, &c| n.checked_mul(10)?.checked_add(u32::from(c.wrapping_sub(b'0'))))?,
        Some(z) => zone(z)?,
    };
    Some(Server { ip: Ip::V6(v6), port: PORT, scope })
}

/// The address `inet_aton` reads from `text` (`a`, `a.b`, `a.b.c`, `a.b.c.d`; each part
/// decimal, octal with a leading 0, or hexadecimal with 0x; inet_aton(3)).
pub fn aton(text: &[u8]) -> Option<[u8; 4]> {
    let mut parts = [0u64; 4];
    let mut n = 0usize;
    for p in text.split(|&b| b == b'.') {
        let (digits, radix) = if let Some(h) = p.strip_prefix(b"0x").or_else(|| p.strip_prefix(b"0X")) {
            (h, 16u64)
        } else if p.len() > 1 && p.first() == Some(&b'0') {
            (p.get(1..)?, 8)
        } else {
            (p, 10)
        };
        if p.is_empty() {
            return None;
        }
        let mut v = 0u64;
        for &c in digits {
            let d = u64::from(crate::net::hex_value(c)?);
            if d >= radix {
                return None;
            }
            v = v.checked_mul(radix)?.checked_add(d)?;
            if v > 0xffff_ffff {
                return None;
            }
        }
        *parts.get_mut(n)? = v;
        n = n.checked_add(1)?;
    }
    let [a, b, c, d] = parts;
    let v: u64 = match n {
        1 => a,
        2 if a <= 0xff && b <= 0xff_ffff => a << 24 | b,
        3 if a <= 0xff && b <= 0xff && c <= 0xffff => a << 24 | b << 16 | c,
        4 if a <= 0xff && b <= 0xff && c <= 0xff && d <= 0xff => a << 24 | b << 16 | c << 8 | d,
        _ => return None,
    };
    Some(u32::try_from(v).ok()?.to_be_bytes())
}

fn sort_pair(item: &[u8]) -> Option<([u8; 4], [u8; 4])> {
    let (a, m) = match item.iter().position(|&b| b == b'/' || b == b'&') {
        Some(p) => (item.get(..p)?, Some(item.get(p.checked_add(1)?..)?)),
        None => (item, None),
    };
    let ip = aton(a)?;
    let mask = match m {
        Some(m) => aton(m)?,
        None => match ip.first().copied().unwrap_or(0) {
            0..=127 => [255, 0, 0, 0],
            128..=191 => [255, 255, 0, 0],
            _ => [255, 255, 255, 0],
        },
    };
    Some((ip, mask))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn file() {
        let t = b"# c\n; c\nnameserver 127.0.0.1\nnameserver ::1\nnameserver 999.1.1.1\nnameserver 127.0.0.3 x\ndomain d.test\nsearch a.test b.test\n\
                  sortlist 130.155.160.0/255.255.240.0 130.155.0.0 10.0.0.0/8\noptions ndots:20 timeout:99 attempts:9 rotate inet6 debug bogus\noptions edns0\n";
        let c = Config::parse(Some(t), None, None, b"keke", |_| None).unwrap();
        assert_eq!(c.servers.len(), 3);
        assert_eq!(c.search.len(), 2);
        assert_eq!((c.ndots, c.timeout, c.attempts), (15, 30, 5));
        assert_eq!(c.options, RES_INIT | RES_DEFAULT | RES_ROTATE | RES_USE_EDNS0);
        assert_eq!(c.sort[1], ([130, 155, 0, 0], [255, 255, 0, 0]));
        assert_eq!(c.sort[2].1, [0, 0, 0, 8]);
        let c = Config::parse(None, Some(b" l1 l2 "), Some(b"ndots:4 no-aaaa"), b"h.example.org", |_| None).unwrap();
        assert_eq!(c.search.len(), 2);
        assert_eq!(c.ndots, 4);
        assert_eq!(c.servers[0].ip, Ip::V4([127, 0, 0, 1]));
        let c = Config::parse(None, None, None, b"h.example.org", |_| None).unwrap();
        assert_eq!(c.search[0].as_slice(), b"example.org");
    }
}
