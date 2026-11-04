//! The C view of the DNS stub resolver (spec §16.4; D-16.3): `struct __res_state` (`_res`, the
//! states of `res_ninit`), the `res_n*` calls over a configuration with their `h_errno` codes,
//! `HOSTALIASES`, the reloading of `/etc/resolv.conf`, and the `dns` source of the hosts
//! database (D-16.1).
//!
//! **The state** ([`State`]) is the 568 bytes of `struct __res_state` (Appendix C), read and
//! written at the member offsets: the configuration a caller sees and may change. Liella marks a
//! state it initialized (the first word of `_u._ext.__glibc_reserved`) and records there the
//! generation of `resolv.conf` it was read from. A state shows at most 3 servers (an IPv6
//! server's `nsaddr_list` entry has family 0 and its address is in `_u._ext.nsaddrs`, as the
//! reference shows it), 6 search domains (copied into `defdname`, `dnsrch` pointing there) and
//! 10 sort pairs; the configuration behind it is unbounded (P-lift): when the servers or search
//! domains a state shows are the first ones of the current file's, the file's further ones are
//! used too ([`extend`]).
//!
//! **Reloading** (spec Guarantees; observed). The file's identity (device, inode, size, times)
//! is checked whenever the calling thread's `_res` is used; when it changed since `_res` was
//! read, `_res` is read again (the caller's changes to it are then replaced, as in the
//! reference), unless `_res` has `RES_NORELOAD`. A missing file is the defaults. States of
//! `res_ninit` are the caller's and are never reloaded.
//!
//! **Results** of `res_nquery` and the others (observed): the response is copied into the
//! caller's buffer, cut to its size (the copied length is returned); the AD bit is kept only
//! with `RES_TRUSTAD`. Failures set `h_errno`: NXDOMAIN `HOST_NOT_FOUND`, a response without
//! answers `NO_DATA`, SERVFAIL, REFUSED and no response `TRY_AGAIN` (REFUSED is `TRY_AGAIN` over
//! TCP too: a deliberate difference, dns.md), FORMERR, NOTIMP and other codes `NO_RECOVERY`. A
//! success leaves `h_errno` alone (the reference's `res_search` sets `HOST_NOT_FOUND` on success
//! after trying a name that failed, and even when the first name answered: a deliberate
//! difference). With `RES_NOAAAA` an `AAAA` question is sent as `A` and answers `NO_DATA`.

use core::sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering::Relaxed};

use liella::error::{Errno, Result};
use liella::mem::TryVec;
use liella::names::dns::client::{self, Failure, Miss, Transport};
use liella::names::dns::conf::{self, Config, Server, RES_INIT, RES_NOAAAA, RES_NOALIASES, RES_RECURSE, RES_ROTATE, RES_TRUSTAD, RES_USE_DNSSEC, RES_USE_EDNS0};
use liella::names::dns::host::{self, Outcome};
use liella::names::dns::msg::{self, Header, HFIXEDSZ};
use liella::names::dns::name::{self, Check};
use liella::names::dns::{rcode, T_A, T_AAAA, T_PTR};
use liella::names::nss::{Answer, LookupError};
use liella::names::{Family, Ip};
use liella_kernel::sys::Sys;

use crate::netdb_compat::{HOST_NOT_FOUND, NO_DATA, NO_RECOVERY, TRY_AGAIN};
use crate::nscache::Life;
use crate::nss::Watched;
use crate::nss_files::Host;

/// The configuration file.
pub const PATH_RESCONF: &[u8] = b"/etc/resolv.conf\0";
/// The bytes of `struct __res_state`.
pub const STATE_SIZE: usize = 568;
/// The UDP payload size announced with EDNS (observed).
pub const EDNS_SIZE: u16 = 1200;

// ------------------------------------------------------------------------------ the state

/// Member offsets of `struct __res_state` (Appendix C).
mod off {
    pub(super) const RETRANS: usize = 0;
    pub(super) const RETRY: usize = 4;
    pub(super) const OPTIONS: usize = 8;
    pub(super) const NSCOUNT: usize = 16;
    pub(super) const NSADDR: usize = 20;
    pub(super) const ID: usize = 68;
    pub(super) const DNSRCH: usize = 72;
    pub(super) const DEFDNAME: usize = 128;
    pub(super) const PFCODE: usize = 384;
    pub(super) const BITS: usize = 392;
    pub(super) const SORT: usize = 396;
    pub(super) const H_ERRNO: usize = 496;
    pub(super) const VCSOCK: usize = 500;
    pub(super) const EXT_NSADDRS: usize = 536;
    pub(super) const EXT_MARK: usize = 560;
    pub(super) const EXT_GEN: usize = 564;
}

/// The value in `_u._ext.__glibc_reserved[0]` of a state Liella initialized.
const MARK: u32 = 0x4c69_4c61;

/// A `struct __res_state`, as its bytes.
pub struct State<'a>(pub &'a mut [u8; STATE_SIZE]);

impl State<'_> {
    fn get<const N: usize>(&self, at: usize) -> [u8; N] {
        let mut b = [0u8; N];
        if let Some(s) = self.0.get(at..at.saturating_add(N)) {
            b.copy_from_slice(s);
        }
        b
    }

    fn put(&mut self, at: usize, bytes: &[u8]) {
        if let Some(d) = self.0.get_mut(at..at.saturating_add(bytes.len())) {
            d.copy_from_slice(bytes);
        }
    }

    fn i32_at(&self, at: usize) -> i32 {
        i32::from_ne_bytes(self.get(at))
    }

    fn u32_at(&self, at: usize) -> u32 {
        u32::from_ne_bytes(self.get(at))
    }

    fn usize_at(&self, at: usize) -> usize {
        usize::from_ne_bytes(self.get(at))
    }

    /// `options`.
    pub fn options(&self) -> u64 {
        u64::from_ne_bytes(self.get(off::OPTIONS))
    }

    /// Set `options`.
    pub fn set_options(&mut self, o: u64) {
        self.put(off::OPTIONS, &o.to_ne_bytes());
    }

    /// `pfcode`.
    pub fn pfcode(&self) -> u64 {
        u64::from_ne_bytes(self.get(off::PFCODE))
    }

    /// Set `res_h_errno`.
    pub fn set_h_errno(&mut self, h: i32) {
        self.put(off::H_ERRNO, &h.to_ne_bytes());
    }

    /// Whether Liella initialized the state, and from which generation of the file.
    pub fn generation(&self) -> Option<u32> {
        (self.u32_at(off::EXT_MARK) == MARK).then(|| self.u32_at(off::EXT_GEN))
    }

    /// The address `_u._ext.nsaddrs[i]` holds (an IPv6 server's `struct sockaddr_in6`).
    pub fn ext_addr(&self, i: usize) -> usize {
        self.usize_at(off::EXT_NSADDRS.saturating_add(i.saturating_mul(8)))
    }

    /// Set `_u._ext.nsaddrs[i]`.
    pub fn set_ext_addr(&mut self, i: usize, a: usize) {
        self.put(off::EXT_NSADDRS.saturating_add(i.saturating_mul(8)), &a.to_ne_bytes());
    }

    /// The addresses of the search domains (`dnsrch`, up to the first NULL; at most 6).
    pub fn search_addrs(&self) -> [usize; 6] {
        let mut out = [0usize; 6];
        for (i, o) in out.iter_mut().enumerate() {
            *o = self.usize_at(off::DNSRCH.saturating_add(i.saturating_mul(8)));
            if *o == 0 {
                break;
            }
        }
        out
    }

    /// The servers the state shows: IPv4 from `nsaddr_list`, IPv6 from `v6` (the
    /// `sockaddr_in6` bytes the caller read at [`State::ext_addr`]).
    pub fn servers(&self, v6: &[Option<[u8; 28]>; 3]) -> Result<TryVec<Server>> {
        let n = usize::try_from(self.i32_at(off::NSCOUNT)).unwrap_or(0).min(3);
        let mut out = TryVec::new();
        for (i, v6) in v6.iter().enumerate().take(n) {
            let at = off::NSADDR.saturating_add(i.saturating_mul(16));
            let sa: [u8; 16] = self.get(at);
            let fam = u16::from_ne_bytes([sa[0], sa[1]]);
            if u32::from(fam) == liella_kernel::uapi::AF_INET {
                out.push(Server { ip: Ip::V4([sa[4], sa[5], sa[6], sa[7]]), port: u16::from_be_bytes([sa[2], sa[3]]), scope: 0 })?;
            } else if let Some(b) = v6 {
                if u32::from(u16::from_ne_bytes([b[0], b[1]])) == liella_kernel::uapi::AF_INET6 {
                    let mut a = [0u8; 16];
                    a.copy_from_slice(b.get(8..24).unwrap_or(&[0; 16]));
                    out.push(Server { ip: Ip::V6(a), port: u16::from_be_bytes([b[2], b[3]]), scope: u32::from_ne_bytes([b[24], b[25], b[26], b[27]]) })?;
                }
            }
        }
        Ok(out)
    }

    /// The configuration the state shows: `servers` and `search` as read by the caller.
    pub fn config(&self, servers: TryVec<Server>, search: &[&[u8]]) -> Result<Config> {
        let bits: [u8; 4] = self.get(off::BITS);
        let nsort = usize::from(bits[0] >> 4).min(10);
        let mut sort = TryVec::new();
        for i in 0..nsort {
            let e: [u8; 8] = self.get(off::SORT.saturating_add(i.saturating_mul(8)));
            sort.push(([e[0], e[1], e[2], e[3]], [e[4], e[5], e[6], e[7]]))?;
        }
        let mut domains = TryVec::new();
        for d in search {
            domains.push(TryVec::from_slice(d)?)?;
        }
        Ok(Config {
            servers,
            search: domains,
            sort,
            ndots: u32::from(bits[0] & 0xf),
            timeout: u32::try_from(self.i32_at(off::RETRANS)).unwrap_or(0),
            attempts: u32::try_from(self.i32_at(off::RETRY)).unwrap_or(0),
            options: self.options(),
        })
    }

    /// Write `c` into the state (see the module documentation): `base` is the state's
    /// address, `id` the next query ID. The IPv6 servers among the first three, which the
    /// caller gives `_u._ext.nsaddrs` entries.
    pub fn fill(&mut self, c: &Config, base: usize, generation: u32, id: u16) -> [Option<Server>; 3] {
        self.0.fill(0);
        self.put(off::RETRANS, &i32::try_from(c.timeout).unwrap_or(i32::MAX).to_ne_bytes());
        self.put(off::RETRY, &i32::try_from(c.attempts).unwrap_or(i32::MAX).to_ne_bytes());
        self.set_options(c.options);
        let mut v6 = [None; 3];
        let shown = c.servers.len().min(3);
        self.put(off::NSCOUNT, &i32::try_from(shown).unwrap_or(3).to_ne_bytes());
        for (i, s) in c.servers.iter().take(3).enumerate() {
            let at = off::NSADDR.saturating_add(i.saturating_mul(16));
            match s.ip {
                Ip::V4(a) => {
                    self.put(at, &u16::try_from(liella_kernel::uapi::AF_INET).unwrap_or(2).to_ne_bytes());
                    self.put(at.saturating_add(2), &s.port.to_be_bytes());
                    self.put(at.saturating_add(4), &a);
                }
                Ip::V6(_) => {
                    if let Some(slot) = v6.get_mut(i) {
                        *slot = Some(*s);
                    }
                }
            }
        }
        self.put(off::ID, &id.to_ne_bytes());
        // The search list, NUL-separated in `defdname`, `dnsrch` pointing there.
        let mut at = 0usize;
        let mut n = 0usize;
        for d in c.search.iter().take(6) {
            let end = at.saturating_add(d.len());
            if end >= 256 {
                break;
            }
            self.put(off::DEFDNAME.saturating_add(at), d);
            self.put(off::DNSRCH.saturating_add(n.saturating_mul(8)), &base.saturating_add(off::DEFDNAME).saturating_add(at).to_ne_bytes());
            at = end.saturating_add(1);
            n = n.saturating_add(1);
        }
        let nsort = c.sort.len().min(10);
        let ndots = u8::try_from(c.ndots.min(15)).unwrap_or(15);
        self.put(off::BITS, &[ndots | u8::try_from(nsort).unwrap_or(10) << 4, 0, 0, 0]);
        for (i, (a, m)) in c.sort.iter().take(10).enumerate() {
            let at = off::SORT.saturating_add(i.saturating_mul(8));
            self.put(at, a);
            self.put(at.saturating_add(4), m);
        }
        self.put(off::VCSOCK, &(-1i32).to_ne_bytes());
        self.put(off::EXT_MARK, &MARK.to_ne_bytes());
        self.put(off::EXT_GEN, &generation.to_ne_bytes());
        v6
    }
}

/// The `struct sockaddr_in6` bytes of an IPv6 server.
pub fn sockaddr_in6(s: &Server) -> [u8; 28] {
    let (b, _) = client::sockaddr(s);
    b
}

/// Use the file's servers and search domains beyond those a state shows when the state shows
/// the first ones of the file (see the module documentation).
pub fn extend(c: &mut Config, full: &Config) -> Result<()> {
    if c.servers.len() == 3 && full.servers.len() > 3 && full.servers.get(..3) == Some(c.servers.as_slice()) {
        c.servers.extend_from_slice(full.servers.get(3..).unwrap_or(&[]))?;
    }
    if c.search.len() == 6 && full.search.len() > 6 && c.search.iter().zip(full.search.iter()).all(|(a, b)| a.as_slice() == b.as_slice()) {
        for d in full.search.iter().skip(6) {
            c.search.push(TryVec::from_slice(d)?)?;
        }
    }
    Ok(())
}

/// What the configuration is read from besides the file: the environment's `LOCALDOMAIN` and
/// `RES_OPTIONS`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Env<'a> {
    /// `LOCALDOMAIN`.
    pub localdomain: Option<&'a [u8]>,
    /// `RES_OPTIONS`.
    pub res_options: Option<&'a [u8]>,
}

/// The configuration of the file `path` (NUL-terminated) and the environment.
pub fn load<S: Sys>(sys: &S, path: &[u8], env: Env<'_>) -> Result<Config> {
    let text = liella::system::read_file(sys, path).ok();
    let u = sys.uname().unwrap_or_default();
    let host = u.nodename.get(..u.nodename.iter().position(|&c| c == 0).unwrap_or(u.nodename.len())).unwrap_or(&[]);
    Config::parse(text.as_deref(), env.localdomain, env.res_options, host, |z| liella::net::iface::index_of(sys, z).ok())
}

/// The process's view of `resolv.conf`: a generation number that changes with the file.
pub struct ConfFile {
    file: Watched,
    generation: AtomicU32,
}

impl Default for ConfFile {
    fn default() -> ConfFile {
        ConfFile::new()
    }
}

impl ConfFile {
    /// Never checked.
    pub const fn new() -> ConfFile {
        ConfFile { file: Watched::new(), generation: AtomicU32::new(0) }
    }

    /// The generation of the file `path` now (checked against its identity).
    pub fn generation<S: Sys>(&self, sys: &S, path: &[u8]) -> u32 {
        self.file.with(
            sys,
            path,
            true,
            |_| {
                self.generation.fetch_add(1, Relaxed);
            },
            || self.generation.load(Relaxed),
        )
    }
}

/// The process's `resolv.conf`.
pub static RESOLV_CONF: ConfFile = ConfFile::new();

/// The rotation counter (`RES_ROTATE`): process-wide, one step per exchange (observed), from a
/// random first server, so that processes spread their first queries over the servers
/// (`resolv.conf(5)`: "rather than having all clients try the first listed server first";
/// the reference's first server varies from run to run too).
static ROTATION: AtomicUsize = AtomicUsize::new(0);
static ROTATION_SEEDED: AtomicBool = AtomicBool::new(false);

fn start<S: Sys>(sys: &S, c: &Config) -> usize {
    if c.options & RES_ROTATE == 0 {
        return 0;
    }
    if !ROTATION_SEEDED.swap(true, Relaxed) {
        ROTATION.fetch_add(usize::from(random_id(sys)), Relaxed);
    }
    ROTATION.fetch_add(1, Relaxed)
}

/// A random query ID (`res_randomid`).
pub fn random_id<S: Sys>(sys: &S) -> u16 {
    let mut b = [0u8; 2];
    if sys.getrandom(&mut b, liella_kernel::uapi::GRND_NONBLOCK).is_ok() {
        return u16::from_ne_bytes(b);
    }
    let t = sys.clock_gettime(liella_kernel::uapi::CLOCK_MONOTONIC).unwrap_or_default();
    u16::try_from(t.nsec & 0xffff).unwrap_or(0)
}

// ------------------------------------------------------------------------------ the res_n* calls

/// A failed `res_*` call: the `h_errno` value and the `errno` when one applies.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Fail {
    /// `h_errno`.
    pub h: i32,
    /// `errno`.
    pub errno: Option<Errno>,
}

impl Fail {
    fn h(h: i32) -> Fail {
        Fail { h, errno: None }
    }
}

fn failure(f: Failure) -> Fail {
    match f {
        Failure::NoServer => Fail { h: TRY_AGAIN, errno: Some(Errno::ECONNREFUSED) },
        Failure::Timeout => Fail { h: TRY_AGAIN, errno: Some(Errno::ETIMEDOUT) },
        Failure::Unreachable(e) | Failure::Os(e) => Fail { h: TRY_AGAIN, errno: Some(e) },
    }
}

fn transport<'a, S: Sys>(sys: &S, c: &'a Config) -> Transport<'a> {
    Transport { servers: &c.servers, timeout: c.timeout, attempts: c.attempts, options: c.options, start: start(sys, c) }
}

/// Set or clear the AD bit of a message.
fn set_ad(m: &mut [u8], on: bool) {
    if let Some(b) = m.get_mut(3) {
        *b = if on { *b | 0x20 } else { *b & !0x20 };
    }
}

/// Copy a response to the caller's buffer, cut to its size: the bytes copied.
fn copy_out(resp: &[u8], answer: &mut [u8]) -> usize {
    let n = resp.len().min(answer.len());
    if let (Some(d), Some(s)) = (answer.get_mut(..n), resp.get(..n)) {
        d.copy_from_slice(s);
    }
    n
}

/// `res_nmkquery`: (op, name, class, type, data) into `buf`, RD as `options` says.
#[allow(clippy::too_many_arguments)] // the C call's arguments
pub fn mkquery<S: Sys>(sys: &S, options: u64, op: i32, name: &[u8], class: i32, rtype: i32, data: Option<&[u8]>, buf: &mut [u8]) -> Result<usize> {
    let op = u8::try_from(op).map_err(|_| Errno::EINVAL)?;
    let (class, rtype) = (u16::try_from(class).map_err(|_| Errno::EINVAL)?, u16::try_from(rtype).map_err(|_| Errno::EINVAL)?);
    msg::mkquery(op, name, class, rtype, data, buf, random_id(sys), options & RES_RECURSE != 0)
}

/// The query `res_nquery` sends for (name, class, type): its bytes.
fn build_query<S: Sys>(sys: &S, c: &Config, name: &[u8], class: u16, rtype: u16) -> Result<TryVec<u8>> {
    let mut q: TryVec<u8> = TryVec::new();
    q.resize(name::MAXCDNAME.saturating_add(HFIXEDSZ).saturating_add(32), 0)?;
    let mut n = msg::mkquery(msg::OP_QUERY, name, class, rtype, None, &mut q, random_id(sys), c.options & RES_RECURSE != 0)?;
    set_ad(&mut q, c.options & RES_TRUSTAD != 0);
    if c.options & (RES_USE_EDNS0 | RES_USE_DNSSEC) != 0 {
        n = msg::add_opt(&mut q, n, EDNS_SIZE, c.options & RES_USE_DNSSEC != 0)?;
    }
    q.truncate(n);
    Ok(q)
}

/// The `h_errno` of a response to `res_nquery` (see the module documentation), `None` for an
/// answer.
fn verdict(resp: &[u8], translated: bool) -> Option<i32> {
    let h = Header::of(resp)?;
    match h.rcode() {
        rcode::NOERROR if h.counts[1] == 0 || translated => Some(NO_DATA),
        rcode::NOERROR => None,
        rcode::NXDOMAIN => Some(HOST_NOT_FOUND),
        rcode::SERVFAIL | rcode::REFUSED => Some(TRY_AGAIN),
        _ => Some(NO_RECOVERY),
    }
}

/// `res_nquery(name, class, type)`: the response's length (cut to `answer`).
pub fn nquery<S: Sys>(sys: &S, c: &Config, name: &[u8], class: i32, rtype: i32, answer: &mut [u8]) -> core::result::Result<usize, Fail> {
    let (Ok(class), Ok(mut rtype)) = (u16::try_from(class), u16::try_from(rtype)) else { return Err(Fail { h: NO_RECOVERY, errno: Some(Errno::EINVAL) }) };
    let translated = rtype == T_AAAA && c.options & RES_NOAAAA != 0;
    if translated {
        rtype = T_A;
    }
    let q = build_query(sys, c, name, class, rtype).map_err(|e| Fail { h: NO_RECOVERY, errno: Some(e) })?;
    let [resp, _] = client::exchange(sys, &transport(sys, c), &[&q]).map_err(failure)?;
    let mut resp = resp.ok_or(Fail::h(TRY_AGAIN))?;
    if c.options & RES_TRUSTAD == 0 {
        set_ad(&mut resp, false);
    }
    let n = copy_out(&resp, answer);
    match verdict(&resp, translated) {
        None => Ok(n),
        Some(h) => Err(Fail::h(h)),
    }
}

/// The name `HOSTALIASES` gives `name` in the aliases text (`hostname(7)`: lines of an alias
/// and a name, the alias matched without regard to ASCII case).
pub fn hostalias<'a>(name: &[u8], aliases: &'a [u8]) -> Option<&'a [u8]> {
    aliases.split(|&c| c == b'\n').find_map(|line| {
        let mut w = conf::words(line);
        let (a, real) = (w.next()?, w.next()?);
        a.eq_ignore_ascii_case(name).then_some(real)
    })
}

fn miss(h: i32) -> Miss {
    match h {
        HOST_NOT_FOUND => Miss::NotFound,
        NO_DATA => Miss::NoData,
        NO_RECOVERY => Miss::NoRecovery,
        _ => Miss::TryAgain,
    }
}

fn h_of(m: Miss) -> i32 {
    match m {
        Miss::NotFound => HOST_NOT_FOUND,
        Miss::NoData => NO_DATA,
        Miss::NoRecovery => NO_RECOVERY,
        Miss::TryAgain => TRY_AGAIN,
    }
}

/// `res_nsearch(name, class, type)` (the search rules of `client::search`; `aliases`: the
/// `HOSTALIASES` file's text).
#[allow(clippy::too_many_arguments)] // the C call's arguments
pub fn nsearch<S: Sys>(sys: &S, c: &Config, name: &[u8], class: i32, rtype: i32, answer: &mut [u8], aliases: Option<&[u8]>) -> core::result::Result<usize, Fail> {
    if !name.contains(&b'.') && c.options & RES_NOALIASES == 0 {
        if let Some(real) = aliases.and_then(|t| hostalias(name, t)) {
            return nquery(sys, c, real, class, rtype, answer);
        }
    }
    let domains: TryVec<&[u8]> = {
        let mut d = TryVec::new();
        for s in c.search.iter() {
            d.push(s.as_slice()).map_err(|e| Fail { h: NO_RECOVERY, errno: Some(e) })?;
        }
        d
    };
    let mut last_errno = None;
    let r = client::search(name, c.ndots, &domains, c.options, |n| {
        nquery(sys, c, n, class, rtype, answer).map_err(|f| {
            last_errno = f.errno;
            miss(f.h)
        })
    });
    match r {
        Ok((n, _)) => Ok(n),
        Err(m) => Err(Fail { h: h_of(m), errno: last_errno }),
    }
}

/// `res_nquerydomain(name, domain, class, type)`.
#[allow(clippy::too_many_arguments)] // the C call's arguments
pub fn nquerydomain<S: Sys>(sys: &S, c: &Config, name: &[u8], domain: Option<&[u8]>, class: i32, rtype: i32, answer: &mut [u8]) -> core::result::Result<usize, Fail> {
    let full = client::join(name, domain).map_err(|e| Fail { h: NO_RECOVERY, errno: Some(e) })?;
    nquery(sys, c, &full, class, rtype, answer)
}

/// `res_nsend(msg)`: the response (cut to `answer`) to a prebuilt message.
pub fn nsend<S: Sys>(sys: &S, c: &Config, query: &[u8], answer: &mut [u8]) -> core::result::Result<usize, Fail> {
    let mut q = TryVec::from_slice(query).map_err(|e| Fail { h: NO_RECOVERY, errno: Some(e) })?;
    if q.len() >= HFIXEDSZ {
        set_ad(&mut q, c.options & RES_TRUSTAD != 0);
    }
    let [resp, _] = client::exchange(sys, &transport(sys, c), &[&q]).map_err(failure)?;
    let mut resp = resp.ok_or(Fail::h(TRY_AGAIN))?;
    if c.options & RES_TRUSTAD == 0 {
        set_ad(&mut resp, false);
    }
    Ok(copy_out(&resp, answer))
}

// ------------------------------------------------------------------------------ the `dns` source

/// What the `dns` source of a lookup uses: the configuration (the calling thread's `_res`, D-16.3)
/// and the `HOSTALIASES` text.
#[derive(Debug)]
pub struct Resolver {
    /// The configuration.
    pub conf: Config,
    /// The `HOSTALIASES` file's text, if the variable names one.
    pub aliases: Option<TryVec<u8>>,
}

/// One name's answer for the families asked.
/// What the servers did on the queries that got no answer.
#[derive(Clone, Copy, Default)]
struct Net {
    /// No server could be reached.
    unreachable: bool,
    /// A server was reached and timed out.
    timed_out: bool,
}

/// A failed `dns` lookup's error. `res` (the `res_gethostbyname*` report, observed with one
/// server, `attempts:1 timeout:1`): no server reachable (refused, or no route) is
/// `Os(ECONNREFUSED)` (`HOST_NOT_FOUND`), a timeout `Os(ETIMEDOUT)` (`TRY_AGAIN`).
fn host_miss(m: Miss, net: Net, res: bool) -> LookupError {
    match m {
        Miss::TryAgain if res && net.unreachable => LookupError::Os(Errno::ECONNREFUSED),
        Miss::TryAgain if res && net.timed_out => LookupError::Os(Errno::ETIMEDOUT),
        m => lookup_error(m, net.unreachable),
    }
}

fn lookup_one<S: Sys>(sys: &S, c: &Config, n: &[u8], family: Family, net: &mut Net, life: &mut Life) -> core::result::Result<host::Found, Miss> {
    let noaaaa = c.options & RES_NOAAAA != 0;
    let types: &[u16] = match (family, noaaaa) {
        (Family::V4, _) | (_, true) => &[T_A],
        (Family::V6, false) => &[T_AAAA],
        // Sent A first (observed); the IPv6 addresses come first in the answer.
        (Family::Any, false) => &[T_A, T_AAAA],
    };
    let mut qs: TryVec<TryVec<u8>> = TryVec::new();
    for &t in types {
        let mut q = build_query(sys, c, n, liella::names::dns::CLASS_IN, t).map_err(|_| Miss::NoRecovery)?;
        // Distinct IDs for the queries sent together.
        if let (Some(prev), Some(id)) = (qs.first(), q.get_mut(..2)) {
            if prev.get(..2) == Some(&*id) {
                if let Some(b) = id.get_mut(1) {
                    *b ^= 1;
                }
            }
        }
        qs.push(q).map_err(|_| Miss::NoRecovery)?;
    }
    let refs: TryVec<&[u8]> = {
        let mut r = TryVec::new();
        for q in qs.iter() {
            r.push(q.as_slice()).map_err(|_| Miss::NoRecovery)?;
        }
        r
    };
    let answers = match client::exchange(sys, &transport(sys, c), &refs) {
        Ok(a) => a,
        Err(Failure::Unreachable(_)) => {
            net.unreachable = true;
            return Err(Miss::TryAgain);
        }
        Err(Failure::Timeout) => {
            net.timed_out = true;
            return Err(Miss::TryAgain);
        }
        Err(_) => return Err(Miss::TryAgain),
    };
    let mut found: Option<host::Found> = None;
    let (mut nodata, mut again, mut norec) = (false, false, false);
    for (resp, &t) in answers.iter().zip(types) {
        let Some(resp) = resp else {
            life.dns(None);
            again = true;
            continue;
        };
        life.dns(host::ttl(resp));
        match host::read(resp, n, t) {
            Ok(Outcome::Found(f)) => match &mut found {
                None => found = Some(f),
                Some(acc) => {
                    for a in f.addrs.iter() {
                        acc.addrs.push(*a).map_err(|_| Miss::NoRecovery)?;
                    }
                }
            },
            Ok(Outcome::NoData) => nodata = true,
            Ok(Outcome::NotFound) => {}
            Err(e) if e == Errno::EAGAIN => match Header::of(resp).map(|h| h.rcode()) {
                Some(rcode::SERVFAIL | rcode::REFUSED) => again = true,
                _ => norec = true,
            },
            Err(_) => norec = true,
        }
    }
    if let Some(f) = &mut found {
        f.addrs.as_mut_slice().sort_by_key(|a| matches!(a, Ip::V4(_)));
    }
    let wanted_v6 = family == Family::V6 && noaaaa;
    match found {
        Some(_) if wanted_v6 => Err(Miss::NoData),
        Some(f) => Ok(f),
        None if again => Err(Miss::TryAgain),
        None if norec => Err(Miss::NoRecovery),
        None if nodata => Err(Miss::NoData),
        None => Err(Miss::NotFound),
    }
}

/// Order IPv4 addresses by the sort list (the first matching pair; unmatched ones last, in
/// their order).
fn sort_addrs(addrs: &mut TryVec<Ip>, sort: &[([u8; 4], [u8; 4])]) {
    if sort.is_empty() {
        return;
    }
    let rank = |ip: &Ip| -> usize {
        match ip {
            Ip::V4(a) => sort.iter().position(|(net, mask)| a.iter().zip(mask).map(|(x, m)| x & m).eq(net.iter().zip(mask).map(|(x, m)| x & m))).unwrap_or(sort.len()),
            Ip::V6(_) => sort.len(),
        }
    };
    addrs.as_mut_slice().sort_by_key(rank);
}

fn lookup_error(m: Miss, unreachable: bool) -> LookupError {
    match m {
        Miss::NotFound => LookupError::NotFound,
        Miss::NoData => LookupError::NoData,
        Miss::TryAgain if unreachable => LookupError::Unavailable,
        Miss::TryAgain => LookupError::TryAgain,
        Miss::NoRecovery => LookupError::Unavailable,
    }
}

/// The `dns` source: the host `name` (`Family::Any`: `AAAA` and `A`). A name that is not a host
/// name is not asked for (observed). A name without a dot that `HOSTALIASES` lists is replaced by
/// its alias, without the search list; otherwise the search rules apply.
pub fn host_by_name<S: Sys>(sys: &S, r: &Resolver, name: &[u8], family: Family) -> Answer<Host> {
    host_by_name_life(sys, r, name, family, &mut Life::Files)
}

/// [`host_by_name`], reporting how long the answer may be kept (the TTLs of every response,
/// D-16.10).
pub fn host_by_name_life<S: Sys>(sys: &S, r: &Resolver, name: &[u8], family: Family, life: &mut Life) -> Answer<Host> {
    if !name::check(name, Check::Host) {
        return Err(LookupError::NotFound);
    }
    host_by_name_any(sys, r, name, family, life, false)
}

/// [`host_by_name_life`] without the host-name check: every name is asked for, as the
/// `res_gethostbyname*` compat names do (observed: `host_x.example.` and `bad_name!` are sent).
/// `res`: no server reachable is `Os(ECONNREFUSED)` (the `res_gethostbyname*` report:
/// `HOST_NOT_FOUND` with `errno` `ECONNREFUSED`, observed with a refusing server).
pub fn host_by_name_any<S: Sys>(sys: &S, r: &Resolver, name: &[u8], family: Family, life: &mut Life, res: bool) -> Answer<Host> {
    let c = &r.conf;
    let mut net = Net::default();
    let alias = if !name.contains(&b'.') && c.options & RES_NOALIASES == 0 { r.aliases.as_deref().and_then(|t| hostalias(name, t)) } else { None };
    let found = match alias {
        Some(real) => lookup_one(sys, c, real, family, &mut net, life),
        None => {
            let mut domains: TryVec<&[u8]> = TryVec::new();
            for s in c.search.iter() {
                domains.push(s.as_slice())?;
            }
            client::search(name, c.ndots, &domains, c.options, |n| lookup_one(sys, c, n, family, &mut net, life)).map(|(f, _)| f)
        }
    };
    let mut f = found.map_err(|m| host_miss(m, net, res))?;
    sort_addrs(&mut f.addrs, &c.sort);
    let mut h = Host::new(&f.name)?;
    for a in f.aliases.iter() {
        h.add_name(a)?;
    }
    for a in f.addrs.iter() {
        h.add_addr(*a)?;
    }
    Ok(h)
}

/// The `dns` source: the host at `ip` (its `PTR` records; no search list).
pub fn host_by_addr<S: Sys>(sys: &S, r: &Resolver, ip: Ip) -> Answer<Host> {
    host_by_addr_life(sys, r, ip, &mut Life::Files)
}

/// [`host_by_addr`], reporting how long the answer may be kept (D-16.10).
pub fn host_by_addr_life<S: Sys>(sys: &S, r: &Resolver, ip: Ip, life: &mut Life) -> Answer<Host> {
    let c = &r.conf;
    let rev = host::reverse_name(&ip)?;
    let q = build_query(sys, c, &rev, 1, T_PTR)?;
    let resp = match client::exchange(sys, &transport(sys, c), &[&q]) {
        Ok([Some(a), _]) => a,
        Ok(_) | Err(Failure::Timeout | Failure::Os(_) | Failure::NoServer) => return Err(LookupError::TryAgain),
        Err(Failure::Unreachable(_)) => return Err(LookupError::Unavailable),
    };
    life.dns(host::ttl(&resp));
    match host::read(&resp, &rev, T_PTR) {
        Ok(Outcome::Found(f)) => {
            let mut h = Host::new(&f.name)?;
            for n in f.more_names.iter() {
                h.add_name(n)?;
            }
            h.add_addr(ip)?;
            Ok(h)
        }
        Ok(Outcome::NoData) => Err(LookupError::NoData),
        Ok(Outcome::NotFound) => Err(LookupError::NotFound),
        Err(_) => Err(match Header::of(&resp).map(|h| h.rcode()) {
            Some(rcode::SERVFAIL | rcode::REFUSED) => LookupError::TryAgain,
            _ => LookupError::Unavailable,
        }),
    }
}

/// A `networks` record from the `dns` source (§16.3): the official name, the right-aligned
/// number and the aliases (text, separated by spaces).
#[derive(Debug)]
pub struct DnsNet {
    /// The official name.
    pub name: TryVec<u8>,
    /// The number, right-aligned (D-16.2).
    pub number: u32,
    /// The aliases, separated by spaces.
    pub aliases: TryVec<u8>,
}

/// The `PTR` question for the network `net` (observed): its trailing zero octets dropped, the
/// rest padded with zero octets to four and written in reverse under `in-addr.arpa`
/// (`192.0.2` and `192.0.2.0` both ask `0.2.0.192.in-addr.arpa`). Returns the name and the
/// number without its trailing zero octets.
pub fn net_ptr_name(net: u32) -> (TryVec<u8>, u32) {
    let mut n = net;
    while n != 0 && n & 0xff == 0 {
        n >>= 8;
    }
    let be = n.to_be_bytes();
    let sig = be.get(be.iter().position(|&b| b != 0).unwrap_or(3)..).unwrap_or(&[]);
    let mut octets = [0u8; 4];
    for (o, b) in octets.iter_mut().zip(sig) {
        *o = *b;
    }
    let mut q = TryVec::new();
    for o in octets.iter().rev() {
        let d = [b'0'.wrapping_add(o.wrapping_div(100)), b'0'.wrapping_add(o.wrapping_div(10).wrapping_rem(10)), b'0'.wrapping_add(o.wrapping_rem(10))];
        let skip = if *o >= 100 { 0 } else if *o >= 10 { 1 } else { 2 };
        let _ = q.extend_from_slice(d.get(skip..).unwrap_or(&[])).and_then(|()| q.push(b'.'));
    }
    let _ = q.extend_from_slice(b"in-addr.arpa");
    (q, n)
}

/// The number an `in-addr.arpa` name spells (observed): its labels read in reverse, each the
/// value of its leading digits (0 for none), shifted in by octets.
pub fn net_of_reverse(text: &[u8]) -> u32 {
    let labels = text.get(..text.len().saturating_sub(13)).unwrap_or(&[]);
    labels.rsplit(|&c| c == b'.').filter(|l| !l.is_empty()).fold(0u32, |acc, l| {
        let v = l.iter().take_while(|c| c.is_ascii_digit()).fold(0u32, |v, &c| v.wrapping_mul(10).wrapping_add(u32::from(c.wrapping_sub(b'0'))));
        acc.wrapping_shl(8).wrapping_add(v)
    })
}

/// Whether `text` is a name under `in-addr.arpa` (ASCII case ignored).
fn under_in_addr(text: &[u8]) -> bool {
    text.len() > 13 && text.get(text.len().saturating_sub(13)..).is_some_and(|t| t.eq_ignore_ascii_case(b".in-addr.arpa"))
}

/// One `PTR` question for the `networks` source; `seen` gets each answer `PTR` record's target
/// (text) until it returns `false`. The answer count is never trusted beyond the packet: a
/// record that does not fit ends the walk. A target that cannot be read is a miss (NOT_FOUND);
/// an error code maps as for hosts (SERVFAIL, REFUSED and no answer: TRY_AGAIN; NXDOMAIN:
/// NOT_FOUND; NOERROR without records: NO_DATA).
fn net_query<S: Sys>(sys: &S, c: &Config, qname: &[u8], unreachable: &mut bool, mut seen: impl FnMut(&[u8]) -> core::result::Result<bool, Miss>) -> core::result::Result<(), Miss> {
    let q = build_query(sys, c, qname, 1, T_PTR).map_err(|_| Miss::NoRecovery)?;
    let resp = match client::exchange(sys, &transport(sys, c), &[&q]) {
        Ok([Some(a), _]) => a,
        Err(Failure::Unreachable(_)) => {
            *unreachable = true;
            return Err(Miss::TryAgain);
        }
        Ok(_) | Err(_) => return Err(Miss::TryAgain),
    };
    let h = Header::of(&resp).ok_or(Miss::NoRecovery)?;
    match h.rcode() {
        rcode::NOERROR => {}
        rcode::NXDOMAIN => return Err(Miss::NotFound),
        rcode::SERVFAIL | rcode::REFUSED => return Err(Miss::TryAgain),
        _ => return Err(Miss::NoRecovery),
    }
    let [qd, an, ..] = h.counts;
    if an == 0 {
        return Err(Miss::NoData);
    }
    let mut at = HFIXEDSZ;
    for _ in 0..qd {
        at = msg::rr_at(&resp, at, 0).map_err(|_| Miss::NotFound)?.next;
    }
    let mut wire = [0u8; name::MAXDNAME];
    let mut text = [0u8; name::MAXDNAME];
    for _ in 0..an {
        let Ok(rr) = msg::rr_at(&resp, at, 1) else { break };
        at = rr.next;
        if rr.rtype != T_PTR {
            continue;
        }
        let target = rr.rdata.ok_or(Miss::NotFound)?;
        let (wl, _) = name::unpack(&resp, target, &mut wire).map_err(|_| Miss::NotFound)?;
        let tl = name::expand_text(wire.get(..wl).unwrap_or(&[]), &mut text).map_err(|_| Miss::NotFound)?;
        if !seen(text.get(..tl.saturating_sub(1)).unwrap_or(&[]))? {
            break;
        }
    }
    Ok(())
}

/// The `dns` source of `getnetbyname` (observed): a `PTR` question for `name` through the search
/// rules; the first answer target under `in-addr.arpa` gives the number and is the alias, the
/// official name is `name`. Answers without such a target: TRY_AGAIN. Nothing is kept per
/// record beyond the one returned.
pub fn net_by_name<S: Sys>(sys: &S, r: &Resolver, name: &[u8]) -> Answer<DnsNet> {
    if name.is_empty() {
        return Err(LookupError::Unavailable);
    }
    let c = &r.conf;
    let mut unreachable = false;
    let mut domains: TryVec<&[u8]> = TryVec::new();
    for s in c.search.iter() {
        domains.push(s.as_slice())?;
    }
    let found = client::search(name, c.ndots, &domains, c.options, |n| {
        let mut hit: Option<TryVec<u8>> = None;
        net_query(sys, c, n, &mut unreachable, |t| {
            if under_in_addr(t) {
                hit = Some(TryVec::from_slice(t).map_err(|_| Miss::NoRecovery)?);
                return Ok(false);
            }
            Ok(true)
        })?;
        hit.ok_or(Miss::TryAgain)
    });
    let target = found.map_err(|m| lookup_error(m, unreachable))?.0;
    Ok(DnsNet { name: TryVec::from_slice(name)?, number: net_of_reverse(&target), aliases: target })
}

/// The `dns` source of `getnetbyaddr` (observed): one `PTR` question for [`net_ptr_name`]; the
/// first target is the official name, the others the aliases; the number is `net` without its
/// trailing zero octets. Answers without a `PTR` record: TRY_AGAIN.
pub fn net_by_addr<S: Sys>(sys: &S, r: &Resolver, net: u32) -> Answer<DnsNet> {
    let (qname, number) = net_ptr_name(net);
    let mut unreachable = false;
    let mut out = DnsNet { name: TryVec::new(), number, aliases: TryVec::new() };
    let mut first = true;
    net_query(sys, &r.conf, &qname, &mut unreachable, |t| {
        let to = if first { &mut out.name } else { &mut out.aliases };
        if !first && !to.is_empty() {
            to.push(b' ').map_err(|_| Miss::NoRecovery)?;
        }
        to.extend_from_slice(t).map_err(|_| Miss::NoRecovery)?;
        first = false;
        Ok(true)
    })
    .map_err(|m| lookup_error(m, unreachable))?;
    if first {
        return Err(LookupError::TryAgain);
    }
    Ok(out)
}

/// `RES_INIT` and the default options of a state no one initialized.
pub fn initialized(options: u64) -> bool {
    options & RES_INIT != 0
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    // shiki 2026-10-23 (COMPAT-NSS-RESOLV-PT2-5 probe; no spec clause: LG-7 compat): one server,
    // attempts:1 timeout:1. res_: refused or no route = ECONNREFUSED, timeout = ETIMEDOUT; the
    // other host lookups keep the plain mapping.
    #[test]
    fn host_miss_table() {
        let (un, to) = (Net { unreachable: true, timed_out: false }, Net { unreachable: false, timed_out: true });
        let none = Net::default();
        let table = [
            (Miss::TryAgain, un, true, LookupError::Os(Errno::ECONNREFUSED)),
            (Miss::TryAgain, to, true, LookupError::Os(Errno::ETIMEDOUT)),
            (Miss::TryAgain, none, true, LookupError::TryAgain),
            (Miss::NotFound, un, true, LookupError::NotFound),
            (Miss::TryAgain, un, false, LookupError::Unavailable),
            (Miss::TryAgain, to, false, LookupError::TryAgain),
        ];
        for (i, (m, net, res, want)) in table.into_iter().enumerate() {
            assert_eq!(host_miss(m, net, res), want, "row {i}");
        }
    }

    #[test]
    fn state_round_trip() {
        let t = b"nameserver 127.0.0.1\nnameserver ::1\nsearch a.test b.test\nsortlist 10.0.0.0\noptions ndots:3 rotate\n";
        let c = Config::parse(Some(t), None, None, b"h", |_| None).unwrap();
        let mut b = [0u8; STATE_SIZE];
        let mut st = State(&mut b);
        let v6 = st.fill(&c, 0x1000, 7, 0x1234);
        assert!(v6[1].is_some());
        assert_eq!(st.generation(), Some(7));
        let ext = [None, Some(sockaddr_in6(&v6[1].unwrap())), None];
        let servers = st.servers(&ext).unwrap();
        assert_eq!(servers.len(), 2);
        let back = st.config(servers, &[b"a.test", b"b.test"]).unwrap();
        assert_eq!((back.ndots, back.timeout, back.attempts, back.options), (3, 5, 2, c.options));
        assert_eq!(back.sort.len(), 1);
        assert_eq!(st.search_addrs()[1], 0x1000 + 128 + 7);
        assert_eq!(hostalias(b"MYALIAS", b"x\nmyalias who.test\n"), Some(&b"who.test"[..]));
    }
}
