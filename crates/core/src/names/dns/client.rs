//! Sending queries (spec §16.4 Querying and Guarantees; `resolv.conf(5)`; D-16.3), and the
//! search rules of `res_nsearch`.
//!
//! **Servers** (observed): each attempt asks the servers in order, starting with the one the
//! caller names (rotation, `RES_ROTATE`, is the caller's counter); a server waits `timeout`
//! seconds (at least one) for its answer; one that cannot be reached (ICMP: connection refused)
//! is passed over at once; a response with SERVFAIL, REFUSED, NOTIMP or FORMERR passes to the
//! next server and is returned only when no server gives a better one. Another round follows
//! only when a server timed out, `attempts` rounds in all.
//!
//! **Transport.** UDP on a connected socket (so only the server's datagrams arrive; ICMP errors
//! are reported); a response whose ID or question does not match a pending query is ignored and
//! the wait goes on (spec: ID, source address and question must match). A truncated response
//! (TC) is asked again over TCP of the same server unless `RES_IGNTC`; `RES_USEVC` uses TCP
//! only. Over TCP a response that does not match is a failure of that server (a deliberate
//! difference: the reference accepts it, dns.md). Two queries (`A` and `AAAA`) are sent together
//! on one socket unless `RES_SNGLKUP` or `RES_SNGLKUPREOP` (observed); then one after the other,
//! each on its own socket. When one of a pair is answered and the other is not, the other is sent
//! once more, alone, before the next server (observed). An interrupted wait is resumed with the
//! time that remains.

use crate::error::{Errno, Result};
use crate::mem::TryVec;
use crate::names::Ip;
use liella_kernel::fd::Fd;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{PollFd, Timespec, AF_INET, AF_INET6, CLOCK_MONOTONIC, MSG_NOSIGNAL, POLLIN, POLLOUT, SOCK_CLOEXEC, SOCK_DGRAM, SOCK_NONBLOCK, SOCK_STREAM, SOL_SOCKET, SO_ERROR};

use super::conf::{Server, RES_IGNTC, RES_NOTLDQUERY, RES_SNGLKUP, RES_SNGLKUPREOP, RES_USEVC, RES_DEFNAMES, RES_DNSRCH};
use super::msg::{self, flag, Header, HFIXEDSZ};
use super::name;
use super::rcode;

/// The largest response read.
pub const MAX_RESPONSE: usize = 65_535;

/// How the queries are sent.
#[derive(Clone, Copy, Debug)]
pub struct Transport<'a> {
    /// The servers.
    pub servers: &'a [Server],
    /// Seconds to wait for each server.
    pub timeout: u32,
    /// Rounds over the servers.
    pub attempts: u32,
    /// The `RES_*` options.
    pub options: u64,
    /// The server asked first (taken modulo the count).
    pub start: usize,
}

/// Why no response came.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Failure {
    /// No server is configured.
    NoServer,
    /// Every server that was reached timed out.
    Timeout,
    /// No server could be reached.
    Unreachable(Errno),
    /// A system error.
    Os(Errno),
}

/// The outcome of one query at one server.
enum Res {
    Pending,
    Answer(TryVec<u8>),
    Timeout,
    Refused(Errno),
    Failed(Errno),
    NeedTcp,
}

fn now<S: Sys>(sys: &S) -> u64 {
    sys.clock_gettime(CLOCK_MONOTONIC).map_or(0, |t| u64::try_from(t.sec).unwrap_or(0).saturating_mul(1_000_000_000).saturating_add(u64::try_from(t.nsec).unwrap_or(0)))
}

/// Wait for `events` on `fd` until `deadline` (nanoseconds of the monotonic clock): whether
/// they came (`false`: timed out). Interruptions are resumed.
fn wait<S: Sys>(sys: &S, fd: Fd, events: i16, deadline: u64) -> Result<bool> {
    loop {
        let t = now(sys);
        if t >= deadline {
            return Ok(false);
        }
        let left = deadline.saturating_sub(t);
        let mut ts = Timespec { sec: i64::try_from(left / 1_000_000_000).unwrap_or(i64::MAX), nsec: i64::try_from(left % 1_000_000_000).unwrap_or(0) };
        let mut fds = [PollFd { fd: fd.raw(), events, revents: 0 }];
        match sys.ppoll(&mut fds, Some(&mut ts), None) {
            Ok(0) => {}
            Ok(_) => return Ok(true),
            Err(e) if e == Errno::EINTR => {}
            Err(e) => return Err(e),
        }
    }
}

/// The socket address of a server.
pub fn sockaddr(s: &Server) -> ([u8; 28], usize) {
    let mut b = [0u8; 28];
    let port = s.port.to_be_bytes();
    let mut put = |at: usize, bytes: &[u8]| {
        if let Some(d) = b.get_mut(at..at.saturating_add(bytes.len())) {
            d.copy_from_slice(bytes);
        }
    };
    let len = match s.ip {
        Ip::V4(a) => {
            put(0, &u16::try_from(AF_INET).unwrap_or(2).to_ne_bytes());
            put(2, &port);
            put(4, &a);
            16
        }
        Ip::V6(a) => {
            put(0, &u16::try_from(AF_INET6).unwrap_or(10).to_ne_bytes());
            put(2, &port);
            put(8, &a);
            put(24, &s.scope.to_ne_bytes());
            28
        }
    };
    (b, len)
}

fn family(s: &Server) -> u32 {
    match s.ip {
        Ip::V4(_) => AF_INET,
        Ip::V6(_) => AF_INET6,
    }
}

/// Whether `resp` answers `query`: a response with the query's ID and the same questions.
pub fn matches(query: &[u8], resp: &[u8]) -> bool {
    let (Some(q), Some(r)) = (Header::of(query), Header::of(resp)) else { return false };
    if !r.has(flag::QR) || q.id != r.id || q.counts[0] != r.counts[0] {
        return false;
    }
    matches!(msg::queries_match(query, resp), Ok(true))
}

fn is_refusal(e: Errno) -> bool {
    e == Errno::ECONNREFUSED || e == Errno::ENETUNREACH || e == Errno::EHOSTUNREACH || e == Errno::EADDRNOTAVAIL || e == Errno::EAFNOSUPPORT
}

struct Sock<'a, S: Sys> {
    sys: &'a S,
    fd: Fd,
}

impl<S: Sys> Drop for Sock<'_, S> {
    fn drop(&mut self) {
        let _ = self.sys.close(self.fd);
    }
}

/// Ask `server` the `queries` not yet answered (`res` entries `Pending`) over UDP.
fn udp<S: Sys>(sys: &S, server: &Server, queries: &[&[u8]], res: &mut [Res], timeout: u32, igntc: bool) -> Result<()> {
    let fd = match sys.socket(family(server), SOCK_DGRAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0) {
        Ok(fd) => fd,
        Err(e) => {
            for r in res.iter_mut().filter(|r| matches!(r, Res::Pending)) {
                *r = if is_refusal(e) { Res::Refused(e) } else { Res::Failed(e) };
            }
            return Ok(());
        }
    };
    let s = Sock { sys, fd };
    let (sa, len) = sockaddr(server);
    let fail = |res: &mut [Res], e: Errno| {
        for r in res.iter_mut().filter(|r| matches!(r, Res::Pending)) {
            *r = if is_refusal(e) { Res::Refused(e) } else { Res::Failed(e) };
        }
    };
    if let Err(e) = sys.connect(s.fd, sa.get(..len).unwrap_or(&[])) {
        fail(res, e);
        return Ok(());
    }
    for (q, r) in queries.iter().zip(res.iter()) {
        if matches!(r, Res::Pending) {
            if let Err(e) = sys.sendto(s.fd, q, MSG_NOSIGNAL, None) {
                fail(res, e);
                return Ok(());
            }
        }
    }
    let deadline = now(sys).saturating_add(u64::from(timeout.max(1)).saturating_mul(1_000_000_000));
    let mut buf: TryVec<u8> = TryVec::new();
    buf.resize(MAX_RESPONSE, 0)?;
    while res.iter().any(|r| matches!(r, Res::Pending)) {
        match wait(sys, s.fd, POLLIN, deadline) {
            Ok(true) => {}
            Ok(false) => {
                for r in res.iter_mut().filter(|r| matches!(r, Res::Pending)) {
                    *r = Res::Timeout;
                }
                break;
            }
            Err(e) => {
                fail(res, e);
                break;
            }
        }
        let n = match sys.recvfrom(s.fd, &mut buf, 0, None) {
            Ok(n) => n,
            Err(e) if e == Errno::EAGAIN || e == Errno::EINTR => continue,
            Err(e) => {
                fail(res, e);
                break;
            }
        };
        let resp = buf.get(..n).unwrap_or(&[]);
        for (q, r) in queries.iter().zip(res.iter_mut()) {
            if matches!(r, Res::Pending) && matches(q, resp) {
                let tc = Header::of(resp).is_some_and(|h| h.has(flag::TC));
                *r = if tc && !igntc { Res::NeedTcp } else { Res::Answer(TryVec::from_slice(resp)?) };
                break;
            }
        }
    }
    Ok(())
}

/// Ask `server` one query over TCP.
fn tcp<S: Sys>(sys: &S, server: &Server, query: &[u8], timeout: u32) -> Result<Res> {
    let fd = match sys.socket(family(server), SOCK_STREAM | SOCK_CLOEXEC | SOCK_NONBLOCK, 0) {
        Ok(fd) => fd,
        Err(e) => return Ok(if is_refusal(e) { Res::Refused(e) } else { Res::Failed(e) }),
    };
    let s = Sock { sys, fd };
    let deadline = now(sys).saturating_add(u64::from(timeout.max(1)).saturating_mul(1_000_000_000));
    let (sa, len) = sockaddr(server);
    match sys.connect(s.fd, sa.get(..len).unwrap_or(&[])) {
        Ok(()) => {}
        Err(e) if e == Errno::EINPROGRESS => {
            if !wait(sys, s.fd, POLLOUT, deadline)? {
                return Ok(Res::Timeout);
            }
            let mut v = [0u8; 4];
            sys.getsockopt(s.fd, SOL_SOCKET, SO_ERROR, &mut v)?;
            let err = i32::from_ne_bytes(v);
            if err != 0 {
                let e = Errno::new(err).unwrap_or(Errno::EIO);
                return Ok(if is_refusal(e) { Res::Refused(e) } else { Res::Failed(e) });
            }
        }
        Err(e) => return Ok(if is_refusal(e) { Res::Refused(e) } else { Res::Failed(e) }),
    }
    let mut out: TryVec<u8> = TryVec::new();
    out.extend_from_slice(&u16::try_from(query.len()).map_err(|_| Errno::EMSGSIZE)?.to_be_bytes())?;
    out.extend_from_slice(query)?;
    let mut sent = 0usize;
    while sent < out.len() {
        match sys.sendto(s.fd, out.get(sent..).unwrap_or(&[]), MSG_NOSIGNAL, None) {
            Ok(n) => sent = sent.saturating_add(n),
            Err(e) if e == Errno::EAGAIN || e == Errno::EINTR => {
                if !wait(sys, s.fd, POLLOUT, deadline)? {
                    return Ok(Res::Timeout);
                }
            }
            Err(e) => return Ok(if is_refusal(e) { Res::Refused(e) } else { Res::Failed(e) }),
        }
    }
    let read = |buf: &mut [u8]| -> Result<Option<()>> {
        let mut got = 0usize;
        while got < buf.len() {
            match sys.recvfrom(s.fd, buf.get_mut(got..).unwrap_or(&mut []), 0, None) {
                Ok(0) => return Err(Errno::ECONNRESET),
                Ok(n) => got = got.saturating_add(n),
                Err(e) if e == Errno::EAGAIN || e == Errno::EINTR => {
                    if !wait(sys, s.fd, POLLIN, deadline)? {
                        return Ok(None);
                    }
                }
                Err(e) => return Err(e),
            }
        }
        Ok(Some(()))
    };
    let mut lb = [0u8; 2];
    match read(&mut lb) {
        Ok(Some(())) => {}
        Ok(None) => return Ok(Res::Timeout),
        Err(e) => return Ok(Res::Failed(e)),
    }
    let mut resp: TryVec<u8> = TryVec::new();
    resp.resize(usize::from(u16::from_be_bytes(lb)), 0)?;
    match read(&mut resp) {
        Ok(Some(())) => {}
        Ok(None) => return Ok(Res::Timeout),
        Err(e) => return Ok(Res::Failed(e)),
    }
    if !matches(query, &resp) {
        return Ok(Res::Failed(Errno::EPROTO));
    }
    Ok(Res::Answer(resp))
}

/// Whether a response's code sends the query on to the next server.
fn passes_on(resp: &[u8]) -> bool {
    Header::of(resp).is_some_and(|h| matches!(h.rcode(), rcode::SERVFAIL | rcode::REFUSED | rcode::NOTIMP | rcode::FORMERR))
}

/// Send the queries (one or two) as `t` says (see the module documentation): each one's
/// answer, `None` for one that got none while the other did.
pub fn exchange<S: Sys>(sys: &S, t: &Transport<'_>, queries: &[&[u8]]) -> core::result::Result<[Option<TryVec<u8>>; 2], Failure> {
    let n = t.servers.len();
    if n == 0 || queries.is_empty() || queries.len() > 2 {
        return Err(Failure::NoServer);
    }
    let mut finals: [Option<TryVec<u8>>; 2] = [None, None];
    let mut fallback: [Option<TryVec<u8>>; 2] = [None, None];
    let mut saw_timeout = false;
    let mut unreachable = None;
    let mut other = None;
    let sequential = t.options & (RES_SNGLKUP | RES_SNGLKUPREOP) != 0;
    for _ in 0..t.attempts.max(1) {
        let mut timed_out = false;
        for k in 0..n {
            let Some(server) = t.servers.get(t.start.wrapping_add(k).checked_rem(n).unwrap_or(0)) else { continue };
            let mut res = [Res::Pending, Res::Pending];
            for (i, r) in res.iter_mut().enumerate() {
                if i >= queries.len() || finals.get(i).is_some_and(Option::is_some) {
                    *r = Res::Failed(Errno::EALREADY);
                }
            }
            if !res.iter().any(|r| matches!(r, Res::Pending)) {
                break;
            }
            if t.options & RES_USEVC != 0 {
                for (q, r) in queries.iter().zip(res.iter_mut()) {
                    if matches!(r, Res::Pending) {
                        *r = tcp(sys, server, q, t.timeout).map_err(Failure::Os)?;
                    }
                }
            } else if sequential && queries.len() > 1 {
                for i in 0..queries.len() {
                    let (Some(r), Some(q)) = (res.get_mut(i), queries.get(i)) else { continue };
                    if matches!(r, Res::Pending) {
                        udp(sys, server, core::slice::from_ref(q), core::slice::from_mut(r), t.timeout, t.options & RES_IGNTC != 0).map_err(Failure::Os)?;
                    }
                }
            } else {
                let m = queries.len();
                udp(sys, server, queries, res.get_mut(..m).unwrap_or(&mut []), t.timeout, t.options & RES_IGNTC != 0).map_err(Failure::Os)?;
                // One of a pair answered and the other lost: the lost one is sent once more, alone
                // (observed; the server may handle only one query at a time, resolv.conf(5)).
                if m == 2 && res.iter().any(|r| matches!(r, Res::Answer(_) | Res::NeedTcp)) {
                    for (i, r) in res.iter_mut().enumerate() {
                        if let (Res::Timeout, Some(q)) = (&*r, queries.get(i)) {
                            *r = Res::Pending;
                            udp(sys, server, core::slice::from_ref(q), core::slice::from_mut(r), t.timeout, t.options & RES_IGNTC != 0).map_err(Failure::Os)?;
                        }
                    }
                }
            }
            for (i, r) in res.iter_mut().enumerate() {
                if matches!(r, Res::NeedTcp) {
                    if let Some(q) = queries.get(i) {
                        *r = tcp(sys, server, q, t.timeout).map_err(Failure::Os)?;
                    }
                }
            }
            for (i, r) in res.into_iter().enumerate() {
                match r {
                    Res::Answer(a) => {
                        let slot = if passes_on(&a) { fallback.get_mut(i) } else { finals.get_mut(i) };
                        if let Some(s) = slot {
                            *s = Some(a);
                        }
                    }
                    Res::Timeout => {
                        timed_out = true;
                        saw_timeout = true;
                    }
                    Res::Refused(e) => unreachable = Some(e),
                    Res::Failed(e) if e != Errno::EALREADY => other = Some(e),
                    _ => {}
                }
            }
            if finals.iter().take(queries.len()).all(Option::is_some) {
                break;
            }
        }
        if !timed_out || finals.iter().take(queries.len()).all(Option::is_some) {
            break;
        }
    }
    let mut out: [Option<TryVec<u8>>; 2] = [None, None];
    for (i, o) in out.iter_mut().enumerate() {
        *o = finals.get_mut(i).and_then(Option::take).or_else(|| fallback.get_mut(i).and_then(Option::take));
    }
    if out.iter().any(Option::is_some) {
        return Ok(out);
    }
    Err(if saw_timeout {
        Failure::Timeout
    } else if let Some(e) = other {
        Failure::Os(e)
    } else {
        Failure::Unreachable(unreachable.unwrap_or(Errno::ECONNREFUSED))
    })
}

// ------------------------------------------------------------------------------ search

/// Why a name was not answered, as `h_errno` reports it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Miss {
    /// `HOST_NOT_FOUND`.
    NotFound,
    /// `TRY_AGAIN`.
    TryAgain,
    /// `NO_RECOVERY`.
    NoRecovery,
    /// `NO_DATA`.
    NoData,
}

/// The names `res_nsearch` tries for `name` (observed; `resolver(3)`): a name with a final dot
/// as it is; otherwise, with at least `ndots` dots, the name first; then, when `RES_DNSRCH`
/// is set (for a name without dots, `RES_DEFNAMES` with the first domain only), the name in
/// each search domain; then the name itself if not yet tried, unless it has no dot and
/// `RES_NOTLDQUERY` is set. The search order and the query are the caller's: `ask` returns the
/// answer or the miss, and the first answer ends the search. The failure is `NoData` when some
/// name exists without the data, `TryAgain` when a server failed, else the last miss.
pub fn search<T>(name: &[u8], ndots: u32, domains: &[&[u8]], options: u64, mut ask: impl FnMut(&[u8]) -> core::result::Result<T, Miss>) -> core::result::Result<(T, TryVec<u8>), Miss> {
    let mut tmp: TryVec<u8> = TryVec::new();
    if name.last() == Some(&b'.') && name.len() > 1 {
        let n = name.get(..name.len().saturating_sub(1)).unwrap_or(&[]);
        return ask(n).and_then(|v| Ok((v, TryVec::from_slice(n).map_err(|_| Miss::NoRecovery)?)));
    }
    let dots = u32::try_from(name.iter().filter(|&&c| c == b'.').count()).unwrap_or(u32::MAX);
    let mut tried = false;
    let mut nodata = false;
    let mut again = false;
    let mut last = Miss::NotFound;
    let mut note = |m: Miss, last: &mut Miss| {
        match m {
            Miss::NoData => nodata = true,
            Miss::TryAgain => again = true,
            _ => {}
        }
        *last = m;
    };
    if dots >= ndots {
        tried = true;
        match ask(name) {
            Ok(v) => return Ok((v, TryVec::from_slice(name).map_err(|_| Miss::NoRecovery)?)),
            Err(m) => note(m, &mut last),
        }
    }
    let use_list = (dots == 0 && options & (RES_DEFNAMES | RES_DNSRCH) != 0) || (dots > 0 && options & RES_DNSRCH != 0);
    if use_list {
        let n = if options & RES_DNSRCH != 0 { domains.len() } else { domains.len().min(1) };
        for d in domains.iter().take(n) {
            tmp.clear();
            tmp.extend_from_slice(name).map_err(|_| Miss::NoRecovery)?;
            if d.is_empty() || *d == b"." {
                if tried {
                    continue;
                }
                tried = true;
            } else {
                tmp.push(b'.').map_err(|_| Miss::NoRecovery)?;
                tmp.extend_from_slice(d.strip_suffix(b".").unwrap_or(d)).map_err(|_| Miss::NoRecovery)?;
            }
            match ask(&tmp) {
                Ok(v) => return Ok((v, tmp)),
                Err(m) => note(m, &mut last),
            }
        }
    }
    // RES_NOTLDQUERY has no effect without RES_DEFNAMES or RES_DNSRCH (resolv.conf(5)).
    let no_tld = dots == 0 && options & RES_NOTLDQUERY != 0 && options & (RES_DEFNAMES | RES_DNSRCH) != 0;
    if !tried && !no_tld {
        match ask(name) {
            Ok(v) => return Ok((v, TryVec::from_slice(name).map_err(|_| Miss::NoRecovery)?)),
            Err(m) => note(m, &mut last),
        }
    }
    Err(if nodata {
        Miss::NoData
    } else if again {
        Miss::TryAgain
    } else {
        last
    })
}

/// `res_nquerydomain(name, domain)`: the name to ask (`domain` `None`: `name` without one final
/// dot). `Err` when the result is not a name.
pub fn join(name: &[u8], domain: Option<&[u8]>) -> Result<TryVec<u8>> {
    let mut t = TryVec::new();
    match domain {
        None => {
            let n = match name.strip_suffix(b".") {
                Some(s) if !s.is_empty() => s,
                _ => name,
            };
            t.extend_from_slice(n)?;
        }
        Some(d) => {
            t.extend_from_slice(name)?;
            t.push(b'.')?;
            t.extend_from_slice(d)?;
        }
    }
    if t.len().saturating_add(1) > name::MAXDNAME {
        return Err(Errno::EMSGSIZE);
    }
    Ok(t)
}

/// Whether a response is usable as an answer for the caller of `res_nquery`: (rcode, answer
/// count).
pub fn summary(resp: &[u8]) -> Option<(u8, u16)> {
    let h = Header::of(resp)?;
    (resp.len() >= HFIXEDSZ).then_some((h.rcode(), h.counts[1]))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    fn order(name: &[u8], ndots: u32, domains: &[&[u8]], options: u64) -> alloc::vec::Vec<alloc::string::String> {
        let mut seen = alloc::vec::Vec::new();
        let _ = search::<()>(name, ndots, domains, options, |n| {
            seen.push(alloc::string::String::from_utf8(n.to_vec()).unwrap());
            Err(Miss::NotFound)
        });
        seen
    }

    #[test]
    fn search_order() {
        let d: [&[u8]; 2] = [b"s1.test", b"s2.test"];
        let all = super::super::conf::RES_DEFAULT;
        assert_eq!(order(b"x", 1, &d, all), ["x.s1.test", "x.s2.test", "x"]);
        assert_eq!(order(b"a.b", 1, &d, all), ["a.b", "a.b.s1.test", "a.b.s2.test"]);
        assert_eq!(order(b"a.b.", 1, &d, all), ["a.b"]);
        assert_eq!(order(b"x", 1, &d, all | RES_NOTLDQUERY), ["x.s1.test", "x.s2.test"]);
        assert_eq!(order(b"x", 1, &d, RES_DEFNAMES), ["x.s1.test", "x"]);
        assert_eq!(order(b"x", 1, &d, 0), ["x"]);
        assert_eq!(order(b"x", 1, &d, RES_NOTLDQUERY), ["x"]);
        let e: [&[u8]; 3] = [b"", b"l1", b"l2"];
        assert_eq!(order(b"a.b.c", 4, &e, all), ["a.b.c", "a.b.c.l1", "a.b.c.l2"]);
    }
}
