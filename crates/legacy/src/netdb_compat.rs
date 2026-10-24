//! The netdb databases through the switch (spec §16.3, §16.5; D-16.2): hosts, networks,
//! services, protocols and RPC programs, their lookups and enumerations, and the `h_errno`
//! codes. The C structures are packed by the boundary (`liella_boundary::netdb`).
//!
//! **Hosts.** A numeric text is answered without a lookup: for IPv4, the forms `inet_aton`
//! reads (`1.2` is 1.0.0.2, `0x7f.1` is 127.0.0.1: POSIX gives `gethostbyname` and
//! `getaddrinfo` the `inet_addr` notation; the reference reads hexadecimal parts only in
//! `getaddrinfo`, D-16.2), for IPv6 RFC 4291 text; the answer's name is the text. An empty name
//! is no host (D-16.2: the reference matches a `hosts` line without names). Other names go to
//! the `hosts` sources ([`host_lookup`]: `files`; `dns` from phase 10, part 4). An IPv4-mapped
//! IPv6 address is looked up as the IPv4 address it stands for, and answered in IPv6 form
//! (observed).
//!
//! **Name matching** follows the reference per database (netdb.md): hosts, networks, protocols
//! and aliases ignore ASCII case; services and RPC programs do not, nor do the protocols of
//! services.

use liella::error::{Errno, Result};
use liella::mem::TryVec;
use liella::names::cache::Category;
use liella::names::db::{self, HostLine, Numbered, ServLine};
use liella::names::nss::{self, Answer, Database, LookupError, Service};
use liella::names::{Family, Ip};
use liella::net::{addr, is_space};
use liella_kernel::sys::Sys;

use crate::cursors::{self, Step};
use crate::nscache;
use crate::nss::Ctx;
use crate::resolv_compat as resolv;
use crate::nss_files::{self as files, Host};

/// `HOST_NOT_FOUND`.
pub const HOST_NOT_FOUND: i32 = 1;
/// `TRY_AGAIN`.
pub const TRY_AGAIN: i32 = 2;
/// `NO_RECOVERY`.
pub const NO_RECOVERY: i32 = 3;
/// `NO_DATA`.
pub const NO_DATA: i32 = 4;
/// `NETDB_INTERNAL`: see `errno`.
pub const NETDB_INTERNAL: i32 = -1;

/// The `h_errno` (and `errno`, when one applies) of a failed host or network lookup (D-16.2):
/// `HOST_NOT_FOUND`, `NO_DATA`, `TRY_AGAIN`; `NO_RECOVERY` when the sources are unavailable
/// (observed), with the error when one was reported (a file that cannot be read: the reference
/// sets `h_errno` 0 there, an artifact).
pub fn h_errno_of(e: LookupError) -> (i32, Option<Errno>) {
    match e {
        LookupError::NotFound => (HOST_NOT_FOUND, None),
        LookupError::NoData => (NO_DATA, None),
        LookupError::TryAgain => (TRY_AGAIN, None),
        LookupError::Unavailable => (NO_RECOVERY, None),
        LookupError::Os(e) if e == Errno::ENOMEM || e == Errno::ERANGE => (NETDB_INTERNAL, Some(e)),
        LookupError::Os(e) => (NO_RECOVERY, Some(e)),
    }
}

// ------------------------------------------------------------------------------ hosts

/// The address a numeric host text spells, of `family`.
pub fn numeric(text: &[u8], family: Family) -> Option<Ip> {
    if family != Family::V6 && !text.iter().any(|&c| is_space(c)) {
        if let Some(v) = crate::inet_compat::aton(text) {
            return Some(Ip::V4(v.to_be_bytes()));
        }
    }
    if family != Family::V4 {
        return addr::parse_v6(text).map(Ip::V6);
    }
    None
}

/// The host named `name` according to the `hosts` sources (no numeric reading): `files`, and
/// `dns` (the stub resolver, §16.4, when the context carries one; its answer for
/// [`Family::Any`] gives both families, `AAAA` first).
/// The answer goes through the name-service cache when the context has it (D-16.10).
pub fn host_lookup<S: Sys>(ctx: &Ctx<'_, S>, name: &[u8], family: Family) -> Answer<Host> {
    let fam = match family {
        Family::V4 => 4,
        Family::V6 => 6,
        Family::Any => 0,
    };
    nscache::host(ctx, Some(name), fam, None, |life| {
        nss::run(
            &ctx.spec(Database::Hosts),
            |svc| match (svc, ctx.dns) {
                (Service::Files, _) => files::host_by_name(ctx.sys, ctx.paths.hosts, name, family, ctx.multi),
                (Service::Dns, Some(r)) => resolv::host_by_name_life(ctx.sys, r, name, family, life),
                _ => Err(LookupError::Unavailable),
            },
            |_, _| Ok(()),
        )
    })
}

/// The host at `a` according to the `hosts` sources (through the cache, as [`host_lookup`]).
pub fn host_lookup_addr<S: Sys>(ctx: &Ctx<'_, S>, a: Ip) -> Answer<Host> {
    nscache::host(ctx, None, 0, Some(a), |life| {
        nss::run(
            &ctx.spec(Database::Hosts),
            |svc| match (svc, ctx.dns) {
                (Service::Files, _) => files::host_by_addr(ctx.sys, ctx.paths.hosts, a),
                (Service::Dns, Some(r)) => resolv::host_by_addr_life(ctx.sys, r, a, life),
                _ => Err(LookupError::Unavailable),
            },
            |_, _| Ok(()),
        )
    })
}

/// `gethostbyname2(name, family)` (`Family::V4` for `gethostbyname`): a numeric text, or the
/// host the sources know (see the module documentation).
pub fn host_by_name<S: Sys>(ctx: &Ctx<'_, S>, name: &[u8], family: Family) -> Answer<Host> {
    if name.is_empty() {
        return Err(LookupError::NotFound);
    }
    if let Some(ip) = numeric(name, family) {
        let mut h = Host::new(name)?;
        h.add_addr(ip)?;
        return Ok(h);
    }
    host_lookup(ctx, name, family)
}

/// `gethostbyaddr(a)`: an IPv4-mapped address is looked up as its IPv4 address and answered
/// in IPv6 form.
pub fn host_by_addr<S: Sys>(ctx: &Ctx<'_, S>, a: Ip) -> Answer<Host> {
    match a {
        Ip::V6(v6) => match Ip::unmapped(&v6) {
            Some(v4) => {
                let h = host_lookup_addr(ctx, Ip::V4(v4))?;
                let mut out = Host::new(h.name())?;
                for n in h.aliases() {
                    out.add_name(n)?;
                }
                for x in h.addrs.iter() {
                    out.add_addr(Ip::V6(x.to_v6()))?;
                }
                Ok(out)
            }
            None => host_lookup_addr(ctx, a),
        },
        Ip::V4(_) => host_lookup_addr(ctx, a),
    }
}

/// A host's official name and first address.
#[derive(Debug)]
pub struct Resolved {
    /// The official name (the text itself for a numeric address).
    pub canonical: TryVec<u8>,
    /// The first address.
    pub addr: Ip,
}

/// The host `name` through the process's switch (the `rcmd` family, `gethostid`): `Ok(None)`
/// when it is not found, an error only for a system failure.
pub fn resolve<S: Sys>(sys: &S, name: &[u8], family: Family) -> Result<Option<Resolved>> {
    let ctx = Ctx::system(sys).with_multi();
    match host_by_name(&ctx, name, family) {
        Ok(h) => match h.addrs.first() {
            Some(&addr) => Ok(Some(Resolved { canonical: TryVec::from_slice(h.name())?, addr })),
            None => Ok(None),
        },
        Err(LookupError::Os(e)) if e == Errno::ENOMEM => Err(e),
        Err(_) => Ok(None),
    }
}

/// The next `hosts` line (`gethostent`), given to `f`.
pub fn next_host<S: Sys, T>(ctx: &Ctx<'_, S>, mut f: impl FnMut(&HostLine<'_>) -> Step<T>) -> Result<Option<T>> {
    if !ctx.enumerates(Database::Hosts) {
        return Ok(None);
    }
    cursors::HOST_CURSOR.next(ctx.sys, ctx.paths.hosts, |l| db::host_line(l).map_or(Step::Skip, |h| f(&h)))
}

// ------------------------------------------------------------------------------ networks, services, protocols, RPC

fn parsed<'a, V>(line: &'a TryVec<u8>, parse: fn(&'a [u8]) -> Option<V>) -> Answer<V> {
    parse(line).ok_or(LookupError::NotFound)
}

/// `getnetbyname(name)`, given to `f`.
pub fn net_by_name<S: Sys, T>(ctx: &Ctx<'_, S>, name: &[u8], f: impl FnOnce(&Numbered<'_, u32>) -> T) -> Answer<T> {
    let line = nscache::line(ctx, Category::Networks, 1, &[name], || ctx.line(Database::Networks, ctx.paths.networks, |l| db::net_line(l).is_some_and(|n| n.names(name, db::same_nocase))))?;
    Ok(f(&parsed(&line, db::net_line)?))
}

/// `getnetbyaddr(net, type)`: `type` must be `AF_INET` (2); `net` is right-aligned (D-16.2).
pub fn net_by_addr<S: Sys, T>(ctx: &Ctx<'_, S>, net: u32, kind: i32, f: impl FnOnce(&Numbered<'_, u32>) -> T) -> Answer<T> {
    if kind != 2 {
        return Err(LookupError::NotFound);
    }
    let line = nscache::line(ctx, Category::Networks, 2, &[&net.to_le_bytes()], || ctx.line(Database::Networks, ctx.paths.networks, |l| db::net_line(l).is_some_and(|n| n.number == net)))?;
    Ok(f(&parsed(&line, db::net_line)?))
}

/// The next `networks` record (`getnetent`).
pub fn next_net<S: Sys, T>(ctx: &Ctx<'_, S>, mut f: impl FnMut(&Numbered<'_, u32>) -> Step<T>) -> Result<Option<T>> {
    if !ctx.enumerates(Database::Networks) {
        return Ok(None);
    }
    cursors::NET_CURSOR.next(ctx.sys, ctx.paths.networks, |l| db::net_line(l).map_or(Step::Skip, |n| f(&n)))
}

/// A services key's kind: `base` with any protocol, `base + 1` with one named.
fn proto_kind(base: u8, proto: Option<&[u8]>) -> u8 {
    if proto.is_some() { base.saturating_add(1) } else { base }
}

fn proto_ok(s: &ServLine<'_>, proto: Option<&[u8]>) -> bool {
    proto.is_none_or(|p| s.proto == p)
}

/// `getservbyname(name, proto)`: the first entry with the name and the protocol (any protocol
/// for `None`).
pub fn serv_by_name<S: Sys, T>(ctx: &Ctx<'_, S>, name: &[u8], proto: Option<&[u8]>, f: impl FnOnce(&ServLine<'_>) -> T) -> Answer<T> {
    let line = nscache::line(ctx, Category::Services, proto_kind(1, proto), &[name, proto.unwrap_or(&[])], || {
        ctx.line(Database::Services, ctx.paths.services, |l| db::serv_line(l).is_some_and(|s| s.names(name) && proto_ok(&s, proto)))
    })?;
    Ok(f(&parsed(&line, db::serv_line)?))
}

/// `getservbyport(port, proto)` (the port in host order here).
pub fn serv_by_port<S: Sys, T>(ctx: &Ctx<'_, S>, port: u16, proto: Option<&[u8]>, f: impl FnOnce(&ServLine<'_>) -> T) -> Answer<T> {
    let line = nscache::line(ctx, Category::Services, proto_kind(3, proto), &[&port.to_le_bytes(), proto.unwrap_or(&[])], || {
        ctx.line(Database::Services, ctx.paths.services, |l| db::serv_line(l).is_some_and(|s| s.port == port && proto_ok(&s, proto)))
    })?;
    Ok(f(&parsed(&line, db::serv_line)?))
}

/// The next `services` record (`getservent`).
pub fn next_serv<S: Sys, T>(ctx: &Ctx<'_, S>, mut f: impl FnMut(&ServLine<'_>) -> Step<T>) -> Result<Option<T>> {
    if !ctx.enumerates(Database::Services) {
        return Ok(None);
    }
    cursors::SERV_CURSOR.next(ctx.sys, ctx.paths.services, |l| db::serv_line(l).map_or(Step::Skip, |s| f(&s)))
}

/// `getprotobyname(name)`.
pub fn proto_by_name<S: Sys, T>(ctx: &Ctx<'_, S>, name: &[u8], f: impl FnOnce(&Numbered<'_, i32>) -> T) -> Answer<T> {
    let line = nscache::line(ctx, Category::Protocols, 1, &[name], || ctx.line(Database::Protocols, ctx.paths.protocols, |l| db::numbered_line(l).is_some_and(|p| p.names(name, db::same_nocase))))?;
    Ok(f(&parsed(&line, db::numbered_line)?))
}

/// `getprotobynumber(number)`.
pub fn proto_by_number<S: Sys, T>(ctx: &Ctx<'_, S>, number: i32, f: impl FnOnce(&Numbered<'_, i32>) -> T) -> Answer<T> {
    let line = nscache::line(ctx, Category::Protocols, 2, &[&number.to_le_bytes()], || ctx.line(Database::Protocols, ctx.paths.protocols, |l| db::numbered_line(l).is_some_and(|p| p.number == number)))?;
    Ok(f(&parsed(&line, db::numbered_line)?))
}

/// The next `protocols` record (`getprotoent`).
pub fn next_proto<S: Sys, T>(ctx: &Ctx<'_, S>, mut f: impl FnMut(&Numbered<'_, i32>) -> Step<T>) -> Result<Option<T>> {
    if !ctx.enumerates(Database::Protocols) {
        return Ok(None);
    }
    cursors::PROTO_CURSOR.next(ctx.sys, ctx.paths.protocols, |l| db::numbered_line(l).map_or(Step::Skip, |p| f(&p)))
}

/// `getrpcbyname(name)`.
pub fn rpc_by_name<S: Sys, T>(ctx: &Ctx<'_, S>, name: &[u8], f: impl FnOnce(&Numbered<'_, i32>) -> T) -> Answer<T> {
    let line = ctx.line(Database::Rpc, ctx.paths.rpc, |l| db::rpc_line(l).is_some_and(|p| p.names(name, db::same)))?;
    Ok(f(&parsed(&line, db::rpc_line)?))
}

/// `getrpcbynumber(number)`.
pub fn rpc_by_number<S: Sys, T>(ctx: &Ctx<'_, S>, number: i32, f: impl FnOnce(&Numbered<'_, i32>) -> T) -> Answer<T> {
    let line = ctx.line(Database::Rpc, ctx.paths.rpc, |l| db::rpc_line(l).is_some_and(|p| p.number == number))?;
    Ok(f(&parsed(&line, db::rpc_line)?))
}

/// The next `rpc` record (`getrpcent`).
pub fn next_rpc<S: Sys, T>(ctx: &Ctx<'_, S>, mut f: impl FnMut(&Numbered<'_, i32>) -> Step<T>) -> Result<Option<T>> {
    if !ctx.enumerates(Database::Rpc) {
        return Ok(None);
    }
    cursors::RPC_CURSOR.next(ctx.sys, ctx.paths.rpc, |l| db::rpc_line(l).map_or(Step::Skip, |p| f(&p)))
}

/// The next `aliases` entry (`getaliasent`).
pub fn next_alias<S: Sys, T>(ctx: &Ctx<'_, S>, mut f: impl FnMut(&db::Alias<'_>) -> Step<T>) -> Result<Option<T>> {
    if !ctx.enumerates(Database::Aliases) {
        return Ok(None);
    }
    cursors::ALIAS_CURSOR.next_span(ctx.sys, ctx.paths.aliases, |rest| match db::alias_at(rest, 0) {
        None => (Step::Skip, rest.len()),
        Some((None, used)) => (Step::Skip, used),
        Some((Some(a), used)) => (f(&a), used),
    })
}
