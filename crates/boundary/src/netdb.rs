//! The C conventions of spec §16.1–16.3, §16.5 and §16.6 (phase 10, part 3; D-16.1, D-16.2):
//! `h_errno`, the `struct hostent`/`netent`/`servent`/`protoent`/`rpcent`/`aliasent` results,
//! the netgroup enumeration, `getaddrinfo`'s list and `getnameinfo`'s texts. The lookups are
//! `liella_legacy::{nss, netdb_compat, gai}` through the process's switch; the exports in
//! `posix::s16_nameservices` convert the C arguments.
//!
//! **Results.** As for the user databases (D-17.3): the non-`_r` forms return a record in a
//! block of the calling thread, one per record kind (`users::own`); the `_r` forms pack the
//! record's strings and arrays into the caller's buffer (`ERANGE` when it is too small; an
//! enumeration then stays on the record). Host and network lookups report failures in
//! `h_errno` (per thread, in the thread block; the `_r` forms through their `h_errnop`):
//! `HOST_NOT_FOUND`, `NO_DATA`, `TRY_AGAIN`, `NO_RECOVERY`, or `NETDB_INTERNAL` with `errno`
//! (`ERANGE` for a buffer too small, `EAFNOSUPPORT`/`EINVAL` for a family or length the call
//! cannot take). Their `_r` forms return 0 for an entry that is not there (`*result` NULL), the
//! error number otherwise (`EAGAIN` for `TRY_AGAIN`), and `ENOENT` at the end of an enumeration
//! (`HOST_NOT_FOUND`). The other databases return NULL (errno unchanged) for an entry that is
//! not there and at the end of an enumeration (`_r`: `ENOENT`, `errno` unchanged; #409).
//!
//! **`getaddrinfo`** returns one caller-owned block holding every entry, its socket address and
//! the canonical name (`freeaddrinfo` frees the block; NULL is accepted).

use core::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use liella::error::Errno;
use liella::names::db::{self, HostLine, Numbered, ServLine};
use liella::names::nss::{Answer, LookupError};
use liella::names::{msg, Family, Ip};
use liella_abi::ctypes::c_int;
use liella_abi::layout::{struct_addrinfo, struct_aliasent, struct_hostent, struct_netent, struct_protoent, struct_rpcent, struct_servent};
use liella_abi::pack::Packer;
use liella_kernel::errno::KResult;
use liella_legacy::cursors::{self, Step};
use liella_legacy::gai::{self, GaiError, Hints};
use liella_legacy::netdb_compat::{self as netdb, HOST_NOT_FOUND, NETDB_INTERNAL};
use liella_legacy::nss::{self, Ctx, NETGROUP_CURSOR};
use liella_legacy::nss_files::Host;
use liella_rt::sync::RawMutex;
use liella_rt::tls::H_ERRNO_OFFSET;

use crate::caller_alloc;
use crate::conv;
use crate::errno;
use crate::report;
use crate::sys::CSys;
use crate::tb;
use crate::users::{self, ALIAS, HOST, NET, PROTO, RPC, SERV};

const AF_INET: i32 = 2;
const AF_INET6: i32 = 10;

fn ctx() -> Ctx<'static, CSys> {
    crate::nscache::ctx()
}

/// A host lookup's context: the process's switch, the calling thread's resolver for the `dns`
/// source when the `hosts` line names it (D-16.3), and the thread's locale for IDNA texts.
fn with_hosts<R>(f: impl FnOnce(&Ctx<'_, CSys>) -> R) -> R {
    hconf_env();
    let mut c = ctx().with_multi();
    nss::HOST_CONF.diagnose(&CSys, c.paths.host_conf, |n| crate::process::secure_getenv(n).map(crate::strz::env_value));
    let resolver = if c.spec(liella::names::nss::Database::Hosts).has(liella::names::nss::Service::Dns) { crate::resolv::host_resolver_pristine() } else { None };
    // A thread whose program changed `_res` bypasses the name-service cache (D-16.10).
    if resolver.as_ref().is_some_and(|(_, clean)| !clean) {
        c.cache = None;
    }
    c.dns = resolver.as_ref().map(|(r, _)| r);
    c.locale = Some(crate::locale::current());
    crate::data::set_hconf(c.multi, c.hconf.is_some_and(|s| s.reorder));
    f(&c)
}

/// The `RESOLV_*` variables `host.conf`'s settings take (D-16.13), read at the first host
/// lookup only, and like `HOSTALIASES` not in secure-execution mode; `RESOLV_HOST_CONF` names
/// the file read in place of `/etc/host.conf` (observed: a missing one gives the defaults).
fn hconf_env() {
    nss::HOST_CONF.env_once(&CSys, || {
        let get = |n: &[u8]| crate::process::secure_getenv(n).map(crate::strz::env_value);
        (liella::names::hostconf::Env::read(get), get(b"RESOLV_HOST_CONF"))
    });
}

/// The context of a `getaddrinfo` or `getnameinfo` call that consults no database (numeric
/// arguments): no configuration file is checked (the reference makes no system call there,
/// observed). `_res_hconf` is still filled at the first such call when no host lookup has.
fn without_switch<R>(f: impl FnOnce(&Ctx<'_, CSys>) -> R) -> R {
    if !crate::data::hconf_filled() {
        hconf_env();
        let c = ctx().with_multi();
        crate::data::set_hconf(c.multi, c.hconf.is_some_and(|s| s.reorder));
    }
    f(&Ctx::bare(&CSys, &nss::SYSTEM))
}

// ------------------------------------------------------------------------------ h_errno (§16.3)

/// `__h_errno_location()`: the address of this thread's `h_errno`.
pub fn h_errno_location() -> *mut c_int {
    core::ptr::with_exposed_provenance_mut(tb::tp().wrapping_add(H_ERRNO_OFFSET))
}

/// This thread's `h_errno`.
pub fn h_errno() -> i32 {
    tb::current().h_errno.load(Relaxed)
}

/// Set this thread's `h_errno`.
pub fn set_h_errno(v: i32) {
    tb::current().h_errno.store(v, Relaxed);
}

/// The `h_errno` of a failure (an argument error is `NETDB_INTERNAL`).
fn h_of(e: LookupError) -> (i32, Option<Errno>) {
    match e {
        LookupError::Os(x) if x == Errno::EAFNOSUPPORT || x == Errno::EINVAL => (NETDB_INTERNAL, Some(x)),
        e => netdb::h_errno_of(e),
    }
}

/// Report a failed host or network lookup: `h_errno`, and `errno` when one applies.
fn fail(e: LookupError) {
    let (h, en) = h_of(e);
    set_h_errno(h);
    if let Some(en) = en {
        errno::set(en);
    }
}

/// `hstrerror(code)`: the text's address.
pub fn hstrerror(code: i32) -> usize {
    msg::h_error(code).as_ptr().expose_provenance()
}

/// `herror(s)`: `s: text` (just the text when `s` is NULL or empty) and a newline on standard
/// error, for the current `h_errno` (observed).
pub fn herror(s: KResult<Option<&[u8]>>) {
    let saved = errno::get();
    let t = msg::h_error(h_errno());
    let text = t.strip_suffix(b"\0").unwrap_or(t);
    match s.ok().flatten().filter(|s| !s.is_empty()) {
        Some(s) => report::emit(&[s, b": ", text, b"\n"], false, saved),
        None => report::emit(&[text, b"\n"], false, saved),
    }
}

// ------------------------------------------------------------------------------ packing

fn af_len(ip: &Ip) -> (i32, i32) {
    match ip {
        Ip::V4(_) => (AF_INET, 4),
        Ip::V6(_) => (AF_INET6, 16),
    }
}

fn pack_host(h: &Host, k: &mut Packer<'_>) -> Option<struct_hostent> {
    let (h_addrtype, h_length) = af_len(h.addrs.first()?);
    let h_name = k.text(h.name())?;
    let h_aliases = k.texts(h.aliases())?;
    let h_addr_list = k.raws(h.addrs.iter().map(Ip::bytes))?;
    Some(struct_hostent { h_name, h_aliases, h_addrtype, h_length, h_addr_list })
}

fn pack_host_line(l: &HostLine<'_>, k: &mut Packer<'_>) -> Option<struct_hostent> {
    let (h_addrtype, h_length) = af_len(&l.addr);
    let h_name = k.text(l.name)?;
    let h_aliases = k.texts(l.aliases.clone())?;
    let h_addr_list = k.raws(core::iter::once(l.addr.bytes()))?;
    Some(struct_hostent { h_name, h_aliases, h_addrtype, h_length, h_addr_list })
}

fn pack_net(n: &Numbered<'_, u32>, k: &mut Packer<'_>) -> Option<struct_netent> {
    Some(struct_netent { n_name: k.text(n.name)?, n_aliases: k.texts(n.aliases.clone())?, n_addrtype: AF_INET, n_net: n.number })
}

fn pack_serv(s: &ServLine<'_>, k: &mut Packer<'_>) -> Option<struct_servent> {
    Some(struct_servent { s_name: k.text(s.name)?, s_aliases: k.texts(s.aliases.clone())?, s_port: i32::from(s.port.to_be()), s_proto: k.text(s.proto)? })
}

fn pack_proto(p: &Numbered<'_, i32>, k: &mut Packer<'_>) -> Option<struct_protoent> {
    Some(struct_protoent { p_name: k.text(p.name)?, p_aliases: k.texts(p.aliases.clone())?, p_proto: p.number })
}

fn pack_rpc(p: &Numbered<'_, i32>, k: &mut Packer<'_>) -> Option<struct_rpcent> {
    Some(struct_rpcent { r_name: k.text(p.name)?, r_aliases: k.texts(p.aliases.clone())?, r_number: p.number })
}

fn pack_alias(a: &db::Alias<'_>, k: &mut Packer<'_>) -> Option<struct_aliasent> {
    let n = a.members.iter().count();
    Some(struct_aliasent { alias_name: k.text(a.name)?, alias_members_len: u64::try_from(n).ok()?, alias_members: k.texts(a.members.iter())?, alias_local: 1 })
}

/// The `_r` protocol: `find` packs into the buffer (`Ok(None)`: no room). Returns the error
/// number; `herr` receives the `h_errno` value when the call has one. For an enumeration
/// (`ent`), `NotFound` is the end: `ENOENT`.
fn r_form<T: Copy>(out: Option<&mut T>, buf: KResult<&mut [u8]>, result: Option<&mut usize>, herr: Option<&mut i32>, ent: bool, find: impl FnOnce(&mut [u8], usize) -> Answer<Option<T>>) -> i32 {
    let mut h = 0;
    let code = (|| -> i32 {
        let Some(result) = result else {
            h = NETDB_INTERNAL;
            return Errno::EFAULT.get();
        };
        *result = 0;
        let (Some(out), Ok(buf)) = (out, buf) else {
            h = NETDB_INTERNAL;
            return Errno::EFAULT.get();
        };
        let base = buf.as_ptr().expose_provenance();
        match find(buf, base) {
            Ok(Some(v)) => {
                *out = v;
                *result = core::ptr::from_mut(out).expose_provenance();
                0
            }
            Ok(None) => {
                h = NETDB_INTERNAL;
                Errno::ERANGE.get()
            }
            Err(LookupError::NotFound) if ent => {
                h = HOST_NOT_FOUND;
                Errno::ENOENT.get()
            }
            Err(e) => {
                let (hh, en) = h_of(e);
                h = hh;
                match (e, en) {
                    (LookupError::NotFound | LookupError::NoData | LookupError::Unavailable, _) => 0,
                    (LookupError::TryAgain, _) => Errno::EAGAIN.get(),
                    (LookupError::Os(x), Some(_)) if h == NETDB_INTERNAL => {
                        errno::set(x);
                        x.get()
                    }
                    (LookupError::Os(x), _) => x.get(),
                }
            }
        }
    })();
    if let Some(herr) = herr {
        *herr = h;
    }
    code
}

/// An enumeration's `_r` step over the caller's buffer.
fn step_r<V: ?Sized, T>(v: &V, buf: &mut [u8], base: usize, pack: impl Fn(&V, &mut Packer<'_>) -> Option<T>) -> Step<T> {
    match pack(v, &mut Packer::new(buf, base)) {
        Some(t) => Step::Take(t),
        None => Step::Stay(Errno::ERANGE),
    }
}

/// An enumeration step's outcome for [`r_form`]: the end is `NotFound`, a refused record no
/// room.
fn ended<T>(r: KResult<Option<T>>) -> Answer<Option<T>> {
    match r {
        Ok(Some(t)) => Ok(Some(t)),
        Ok(None) => Err(LookupError::NotFound),
        Err(e) if e == Errno::ERANGE => Ok(None),
        Err(e) => Err(LookupError::Os(e)),
    }
}


/// A non-`_r` lookup of the databases without `h_errno`: NULL with `errno` unchanged when not
/// found.
fn own_found(r: Answer<KResult<usize>>) -> KResult<usize> {
    users::found(nss::found(r))
}

/// A non-`_r` enumeration step of the databases without `h_errno`: NULL with `errno` unchanged
/// at the end (#409).
fn own_next(r: KResult<Option<KResult<usize>>>) -> KResult<usize> {
    users::next_entry(r)
}

// ------------------------------------------------------------------------------ hosts (§16.3)

fn family(af: i32) -> Answer<Family> {
    match af {
        AF_INET => Ok(Family::V4),
        AF_INET6 => Ok(Family::V6),
        _ => Err(LookupError::Os(Errno::EAFNOSUPPORT)),
    }
}

/// The non-`_r` host result.
fn own_host(a: Answer<Host>) -> usize {
    match a.and_then(|h| users::own(HOST, |k| pack_host(&h, k)).map_err(LookupError::Os)) {
        Ok(p) => {
            set_h_errno(0);
            p
        }
        Err(e) => {
            fail(e);
            0
        }
    }
}

/// `gethostbyname2(name, af)` (`AF_INET` for `gethostbyname`).
pub fn gethostbyname2(name: KResult<&[u8]>, af: i32) -> usize {
    own_host(name.map_err(LookupError::Os).and_then(|n| with_hosts(|c| netdb::host_by_name(c, n, family(af)?))))
}

/// `gethostbyname2_r(name, af, ret, buf, buflen, result, h_errnop)`.
pub fn gethostbyname2_r(name: KResult<&[u8]>, af: i32, out: Option<&mut struct_hostent>, buf: KResult<&mut [u8]>, result: Option<&mut usize>, herr: Option<&mut i32>) -> i32 {
    r_form(out, buf, result, herr, false, |buf, base| {
        let h = with_hosts(|c| netdb::host_by_name(c, name?, family(af)?))?;
        Ok(pack_host(&h, &mut Packer::new(buf, base)))
    })
}

/// The address `gethostbyaddr` receives: `len` bytes of family `af`.
fn host_addr(addr: KResult<&[u8]>, af: i32) -> Answer<Ip> {
    let a = addr?;
    let bad = |_| LookupError::Os(Errno::EINVAL);
    match af {
        AF_INET => Ok(Ip::V4(a.try_into().map_err(bad)?)),
        AF_INET6 => Ok(Ip::V6(a.try_into().map_err(bad)?)),
        _ => Err(LookupError::Os(Errno::EAFNOSUPPORT)),
    }
}

/// `gethostbyaddr(addr, len, af)` (`addr` viewed as `len` bytes).
pub fn gethostbyaddr(addr: KResult<&[u8]>, af: i32) -> usize {
    own_host(host_addr(addr, af).and_then(|ip| with_hosts(|c| netdb::host_by_addr(c, ip))))
}

/// `gethostbyaddr_r(addr, len, af, ret, buf, buflen, result, h_errnop)`.
pub fn gethostbyaddr_r(addr: KResult<&[u8]>, af: i32, out: Option<&mut struct_hostent>, buf: KResult<&mut [u8]>, result: Option<&mut usize>, herr: Option<&mut i32>) -> i32 {
    r_form(out, buf, result, herr, false, |buf, base| {
        let h = with_hosts(|c| netdb::host_by_addr(c, host_addr(addr, af)?))?;
        Ok(pack_host(&h, &mut Packer::new(buf, base)))
    })
}

/// `sethostent(stayopen)` (the file stays open between steps whatever `stayopen` says).
pub fn sethostent() {
    cursors::HOST_CURSOR.rewind(&CSys);
}

/// `endhostent()`.
pub fn endhostent() {
    cursors::HOST_CURSOR.end(&CSys);
}

/// `gethostent()`: the `hosts` lines of the `files` source, each in its own family (D-16.2).
pub fn gethostent() -> usize {
    match netdb::next_host(&ctx(), |l| Step::Take(users::own(HOST, |k| pack_host_line(l, k)))) {
        Ok(Some(Ok(p))) => p,
        Ok(None) => {
            set_h_errno(HOST_NOT_FOUND);
            0
        }
        Ok(Some(Err(e))) | Err(e) => {
            fail(LookupError::Os(e));
            0
        }
    }
}

/// `gethostent_r(ret, buf, buflen, result, h_errnop)`.
pub fn gethostent_r(out: Option<&mut struct_hostent>, buf: KResult<&mut [u8]>, result: Option<&mut usize>, herr: Option<&mut i32>) -> i32 {
    r_form(out, buf, result, herr, true, |buf, base| ended(netdb::next_host(&ctx(), |l| step_r(l, buf, base, pack_host_line))))
}

// ------------------------------------------------------------------------------ libresolv compat (LG-7)

/// The `_gethtent` cursor: its own position in the hosts file, apart from `gethostent`'s.
static HT_CURSOR: cursors::Cursor = cursors::Cursor::new();

/// A `_ght*` result in its own block; a failure sets `h_errno` `HOST_NOT_FOUND` only (observed:
/// `errno` is never changed, an unknown family is `HOST_NOT_FOUND` too).
fn own_ht(found: Option<KResult<usize>>) -> usize {
    match found {
        Some(Ok(p)) => p,
        Some(Err(_)) | None => {
            set_h_errno(HOST_NOT_FOUND);
            0
        }
    }
}

/// `_gethtbyname2(name, af)` (`AF_INET` for `_gethtbyname`): the first line of the hosts file
/// only (no switch, no DNS, no numeric parsing) whose address is of `af` and that names `name`.
pub fn compat_gethtbyname2(name: KResult<&[u8]>, af: i32) -> usize {
    let found = (|| {
        let fam = family(af).ok()?;
        let h = liella_legacy::nss_files::host_by_name(&CSys, nss::SYSTEM.hosts, name.ok()?, fam, false).ok()?;
        Some(users::own(users::HT_HOST, |k| pack_host(&h, k)))
    })();
    own_ht(found)
}

/// `_gethtbyaddr(addr, len, af)`: the first hosts-file line of family `af` whose address begins
/// with the `len` bytes at `addr` (observed: a shorter `len` compares only that prefix). A `len`
/// longer than the family's address never matches.
pub fn compat_gethtbyaddr(addr: KResult<&[u8]>, af: i32) -> usize {
    let found = (|| {
        let fam = family(af).ok()?;
        let a = addr.ok()?;
        let text = liella_legacy::nss_files::read(&CSys, nss::SYSTEM.hosts).ok()?;
        let l = db::lines(&text).filter_map(db::host_line).find(|l| fam.admits(&l.addr) && l.addr.bytes().starts_with(a))?;
        Some(users::own(users::HT_HOST, |k| pack_host_line(&l, k)))
    })();
    own_ht(found)
}

/// `_sethtent(stayopen)`: rewinds the `_gethtent` cursor.
pub fn compat_sethtent() {
    HT_CURSOR.rewind(&CSys);
}

/// A `res_gethostby*` result in its own block ([`users::RES_HOST`]): `h_errno` 0 on success.
fn own_res(a: Answer<Host>) -> usize {
    match a.and_then(|h| users::own(users::RES_HOST, |k| pack_host(&h, k)).map_err(LookupError::Os)) {
        Ok(p) => {
            set_h_errno(0);
            p
        }
        Err(e) => {
            let (h, en) = res_codes(e);
            set_h_errno(h);
            if let Some(en) = en {
                errno::set(en);
            }
            0
        }
    }
}

/// `h_errno` and `errno` for a failed `res_gethostby*` call: as the other host lookups, except
/// that no reachable server (`Os(ECONNREFUSED)` from the `dns` source) is `HOST_NOT_FOUND` with
/// `errno` `ECONNREFUSED` (observed with a server refusing on 127.0.0.1).
pub fn res_codes(e: LookupError) -> (i32, Option<Errno>) {
    match e {
        LookupError::Os(x) if x == Errno::ECONNREFUSED => (HOST_NOT_FOUND, Some(x)),
        // A server that timed out (observed): TRY_AGAIN with ETIMEDOUT.
        LookupError::Os(x) if x == Errno::ETIMEDOUT => (netdb::h_errno_of(LookupError::TryAgain).0, Some(x)),
        e => h_of(e),
    }
}

/// A dotted quad of four decimal parts (each at most 255), the only numeric form
/// `res_gethostbyname` reads (observed: `1.2.3` is not found and asks nothing).
fn dotted_quad(t: &[u8]) -> Option<[u8; 4]> {
    let mut out = [0u8; 4];
    let mut parts = t.split(|&c| c == b'.');
    for o in out.iter_mut() {
        let p = parts.next()?;
        if p.is_empty() || p.len() > 3 || (p.len() > 1 && p.first() == Some(&b'0')) {
            return None;
        }
        // At most three digits, so this cannot overflow; checked anyway. A longer part (`0127`,
        // `1234`, `4294967423`) or one with a leading zero (`012`, `00`; `0` alone is read) is
        // not found (observed).
        let v = p.iter().try_fold(0u8, |v, &c| v.checked_mul(10)?.checked_add(c.checked_sub(b'0')?))?;
        *o = v;
    }
    parts.next().is_none().then_some(out)
}

/// The `dns` source alone for the `res_gethostby*` names (the hosts file and the switch are not
/// read, observed), with the calling thread's resolver.
fn dns_only(f: impl FnOnce(&liella_legacy::resolv_compat::Resolver) -> Answer<Host>) -> Answer<Host> {
    match crate::resolv::host_resolver_pristine() {
        Some((r, _)) => f(&r),
        None => Err(LookupError::Unavailable),
    }
}

/// What `res_gethostbyname*` does with `name` before any DNS query: `Ok(Some(quad))` for a name
/// read as a dotted quad, `Ok(None)` to ask DNS. A name of digits and dots not ending in a dot
/// is a dotted quad or not found, without asking; a name with a colon is not found; a name with
/// a label over 63 bytes or over 255 bytes in all is `NO_RECOVERY` with `EMSGSIZE`. An empty
/// name is `NO_RECOVERY`, with `EMSGSIZE` only when the configuration has a search list
/// (`searching`; without one `errno` is left alone, observed). Every other name is asked for,
/// host name or not (observed: `host_x.example.`, `bad_name!`, `1.2.3.4.` are sent).
pub fn res_name(n: &[u8], searching: bool) -> Answer<Option<[u8; 4]>> {
    if n.is_empty() && !searching {
        return Err(LookupError::Unavailable);
    }
    let mut wire = [0u8; liella::names::dns::name::MAXCDNAME];
    if n.is_empty() || liella::names::dns::name::pton(n, &mut wire).is_err() {
        return Err(LookupError::Os(Errno::EMSGSIZE));
    }
    if n.contains(&b':') {
        return Err(LookupError::NotFound);
    }
    if n.iter().all(|&c| c.is_ascii_digit() || c == b'.') && n.last() != Some(&b'.') {
        return dotted_quad(n).map(Some).ok_or(LookupError::NotFound);
    }
    Ok(None)
}

/// `res_gethostbyname2(name, af)` (and `res_gethostbyname`): `af` is ignored, the answer is
/// always `AF_INET` (observed: only `A` is asked, also for `AF_INET6` or an unknown family).
/// Names are classified by [`res_name`].
pub fn compat_res_gethostbyname(name: KResult<&[u8]>) -> usize {
    own_res(name.map_err(LookupError::Os).and_then(|n| {
        // Only the empty name depends on the search list; read the configuration for it alone.
        let searching = n.is_empty() && crate::resolv::host_resolver_pristine().is_some_and(|(r, _)| !r.conf.search.is_empty());
        match res_name(n, searching)? {
            Some(q) => {
                let mut h = Host::new(n)?;
                h.add_addr(Ip::V4(q))?;
                Ok(h)
            }
            None => dns_only(|r| liella_legacy::resolv_compat::host_by_name_any(&CSys, r, n, Family::V4, &mut liella_legacy::nscache::Life::Files, true)),
        }
    }))
}

/// `res_gethostbyaddr(addr, len, af)`: the `dns` source alone. An IPv6 address whose first 12
/// bytes are `::` or `::ffff:` is asked as its IPv4 address (`in-addr.arpa`) and answered as
/// `AF_INET` (observed for both forms; the `::` form's answer shape is assumed the same).
pub fn compat_res_gethostbyaddr(addr: KResult<&[u8]>, af: i32) -> usize {
    own_res(host_addr(addr, af).and_then(|ip| {
        let ip = match ip {
            Ip::V6([0, 0, 0, 0, 0, 0, 0, 0, 0, 0, x, y, a, b, c, d]) if x == y && (x == 0 || x == 0xff) => Ip::V4([a, b, c, d]),
            ip => ip,
        };
        dns_only(|r| liella_legacy::resolv_compat::host_by_addr(&CSys, r, ip))
    }))
}

/// `_gethtent()`: the next line of the hosts file, either family (no switch).
pub fn compat_gethtent() -> usize {
    let r = HT_CURSOR.next(&CSys, nss::SYSTEM.hosts, |line| match db::host_line(line) {
        Some(l) => Step::Take(users::own(users::HT_HOST, |k| pack_host_line(&l, k))),
        None => Step::Skip,
    });
    own_ht(r.ok().flatten())
}

// ------------------------------------------------------------------------------ networks (§16.3)

/// The non-`_r` network result (`h_errno` for failures).
fn own_net(a: Answer<KResult<usize>>) -> usize {
    match a.and_then(|r| r.map_err(LookupError::Os)) {
        Ok(p) => p,
        Err(e) => {
            fail(e);
            0
        }
    }
}

/// `getnetbyname(name)`.
pub fn getnetbyname(name: KResult<&[u8]>) -> usize {
    own_net(name.map_err(LookupError::Os).and_then(|n| netdb::net_by_name(&ctx(), n, |e| users::own(NET, |k| pack_net(e, k)))))
}

/// `getnetbyname_r(name, ret, buf, buflen, result, h_errnop)`.
pub fn getnetbyname_r(name: KResult<&[u8]>, out: Option<&mut struct_netent>, buf: KResult<&mut [u8]>, result: Option<&mut usize>, herr: Option<&mut i32>) -> i32 {
    r_form(out, buf, result, herr, false, |buf, base| netdb::net_by_name(&ctx(), name?, |e| pack_net(e, &mut Packer::new(buf, base))))
}

/// `getnetbyaddr(net, type)`.
pub fn getnetbyaddr(net: u32, kind: i32) -> usize {
    own_net(netdb::net_by_addr(&ctx(), net, kind, |e| users::own(NET, |k| pack_net(e, k))))
}

/// `getnetbyaddr_r(net, type, ret, buf, buflen, result, h_errnop)`.
pub fn getnetbyaddr_r(net: u32, kind: i32, out: Option<&mut struct_netent>, buf: KResult<&mut [u8]>, result: Option<&mut usize>, herr: Option<&mut i32>) -> i32 {
    r_form(out, buf, result, herr, false, |buf, base| netdb::net_by_addr(&ctx(), net, kind, |e| pack_net(e, &mut Packer::new(buf, base))))
}

/// `setnetent(stayopen)`.
pub fn setnetent() {
    cursors::NET_CURSOR.rewind(&CSys);
}

/// `endnetent()`.
pub fn endnetent() {
    cursors::NET_CURSOR.end(&CSys);
}

/// `getnetent()`.
pub fn getnetent() -> usize {
    match netdb::next_net(&ctx(), |n| Step::Take(users::own(NET, |k| pack_net(n, k)))) {
        Ok(Some(Ok(p))) => p,
        Ok(None) => {
            set_h_errno(HOST_NOT_FOUND);
            0
        }
        Ok(Some(Err(e))) | Err(e) => {
            fail(LookupError::Os(e));
            0
        }
    }
}

/// `getnetent_r(ret, buf, buflen, result, h_errnop)`.
pub fn getnetent_r(out: Option<&mut struct_netent>, buf: KResult<&mut [u8]>, result: Option<&mut usize>, herr: Option<&mut i32>) -> i32 {
    r_form(out, buf, result, herr, true, |buf, base| ended(netdb::next_net(&ctx(), |n| step_r(n, buf, base, pack_net))))
}

// ------------------------------------------------------------------------------ services (§16.5)

/// The port of `getservbyport`: an `int` holding a network-order 16-bit port.
pub fn port_of(port: i32) -> u16 {
    u16::from_be(u16::try_from(port & 0xffff).unwrap_or(0))
}

/// `getservbyname(name, proto)` (`proto` NULL: any protocol).
pub fn getservbyname(name: KResult<&[u8]>, proto: KResult<Option<&[u8]>>) -> KResult<usize> {
    own_found((|| netdb::serv_by_name(&ctx(), name?, proto?, |s| users::own(SERV, |k| pack_serv(s, k))))())
}

/// `getservbyname_r(name, proto, ret, buf, buflen, result)`.
pub fn getservbyname_r(name: KResult<&[u8]>, proto: KResult<Option<&[u8]>>, out: Option<&mut struct_servent>, buf: KResult<&mut [u8]>, result: Option<&mut usize>) -> i32 {
    r_form(out, buf, result, None, false, |buf, base| netdb::serv_by_name(&ctx(), name?, proto?, |s| pack_serv(s, &mut Packer::new(buf, base))))
}

/// `getservbyport(port, proto)`.
pub fn getservbyport(port: i32, proto: KResult<Option<&[u8]>>) -> KResult<usize> {
    own_found((|| netdb::serv_by_port(&ctx(), port_of(port), proto?, |s| users::own(SERV, |k| pack_serv(s, k))))())
}

/// `getservbyport_r(port, proto, ret, buf, buflen, result)`.
pub fn getservbyport_r(port: i32, proto: KResult<Option<&[u8]>>, out: Option<&mut struct_servent>, buf: KResult<&mut [u8]>, result: Option<&mut usize>) -> i32 {
    r_form(out, buf, result, None, false, |buf, base| netdb::serv_by_port(&ctx(), port_of(port), proto?, |s| pack_serv(s, &mut Packer::new(buf, base))))
}

/// `setservent(stayopen)`.
pub fn setservent() {
    cursors::SERV_CURSOR.rewind(&CSys);
}

/// `endservent()`.
pub fn endservent() {
    cursors::SERV_CURSOR.end(&CSys);
}

/// `getservent()`.
pub fn getservent() -> KResult<usize> {
    own_next(netdb::next_serv(&ctx(), |s| Step::Take(users::own(SERV, |k| pack_serv(s, k)))))
}

/// `getservent_r(ret, buf, buflen, result)`.
pub fn getservent_r(out: Option<&mut struct_servent>, buf: KResult<&mut [u8]>, result: Option<&mut usize>) -> i32 {
    r_form(out, buf, result, None, true, |buf, base| ended(netdb::next_serv(&ctx(), |s| step_r(s, buf, base, pack_serv))))
}

// ------------------------------------------------------------------------------ protocols, RPC (§16.5)

/// `getprotobyname(name)`.
pub fn getprotobyname(name: KResult<&[u8]>) -> KResult<usize> {
    own_found((|| netdb::proto_by_name(&ctx(), name?, |p| users::own(PROTO, |k| pack_proto(p, k))))())
}

/// `getprotobyname_r(name, ret, buf, buflen, result)`.
pub fn getprotobyname_r(name: KResult<&[u8]>, out: Option<&mut struct_protoent>, buf: KResult<&mut [u8]>, result: Option<&mut usize>) -> i32 {
    r_form(out, buf, result, None, false, |buf, base| netdb::proto_by_name(&ctx(), name?, |p| pack_proto(p, &mut Packer::new(buf, base))))
}

/// `getprotobynumber(number)`.
pub fn getprotobynumber(number: i32) -> KResult<usize> {
    own_found(netdb::proto_by_number(&ctx(), number, |p| users::own(PROTO, |k| pack_proto(p, k))))
}

/// `getprotobynumber_r(number, ret, buf, buflen, result)`.
pub fn getprotobynumber_r(number: i32, out: Option<&mut struct_protoent>, buf: KResult<&mut [u8]>, result: Option<&mut usize>) -> i32 {
    r_form(out, buf, result, None, false, |buf, base| netdb::proto_by_number(&ctx(), number, |p| pack_proto(p, &mut Packer::new(buf, base))))
}

/// `setprotoent(stayopen)`.
pub fn setprotoent() {
    cursors::PROTO_CURSOR.rewind(&CSys);
}

/// `endprotoent()`.
pub fn endprotoent() {
    cursors::PROTO_CURSOR.end(&CSys);
}

/// `getprotoent()`.
pub fn getprotoent() -> KResult<usize> {
    own_next(netdb::next_proto(&ctx(), |p| Step::Take(users::own(PROTO, |k| pack_proto(p, k)))))
}

/// `getprotoent_r(ret, buf, buflen, result)`.
pub fn getprotoent_r(out: Option<&mut struct_protoent>, buf: KResult<&mut [u8]>, result: Option<&mut usize>) -> i32 {
    r_form(out, buf, result, None, true, |buf, base| ended(netdb::next_proto(&ctx(), |p| step_r(p, buf, base, pack_proto))))
}

/// `getrpcbyname(name)`.
pub fn getrpcbyname(name: KResult<&[u8]>) -> KResult<usize> {
    own_found((|| netdb::rpc_by_name(&ctx(), name?, |p| users::own(RPC, |k| pack_rpc(p, k))))())
}

/// `getrpcbyname_r(name, ret, buf, buflen, result)`.
pub fn getrpcbyname_r(name: KResult<&[u8]>, out: Option<&mut struct_rpcent>, buf: KResult<&mut [u8]>, result: Option<&mut usize>) -> i32 {
    r_form(out, buf, result, None, false, |buf, base| netdb::rpc_by_name(&ctx(), name?, |p| pack_rpc(p, &mut Packer::new(buf, base))))
}

/// `getrpcbynumber(number)`.
pub fn getrpcbynumber(number: i32) -> KResult<usize> {
    own_found(netdb::rpc_by_number(&ctx(), number, |p| users::own(RPC, |k| pack_rpc(p, k))))
}

/// `getrpcbynumber_r(number, ret, buf, buflen, result)`.
pub fn getrpcbynumber_r(number: i32, out: Option<&mut struct_rpcent>, buf: KResult<&mut [u8]>, result: Option<&mut usize>) -> i32 {
    r_form(out, buf, result, None, false, |buf, base| netdb::rpc_by_number(&ctx(), number, |p| pack_rpc(p, &mut Packer::new(buf, base))))
}

/// `setrpcent(stayopen)`.
pub fn setrpcent() {
    cursors::RPC_CURSOR.rewind(&CSys);
}

/// `endrpcent()`.
pub fn endrpcent() {
    cursors::RPC_CURSOR.end(&CSys);
}

/// `getrpcent()`.
pub fn getrpcent() -> KResult<usize> {
    own_next(netdb::next_rpc(&ctx(), |p| Step::Take(users::own(RPC, |k| pack_rpc(p, k)))))
}

/// `getrpcent_r(ret, buf, buflen, result)`.
pub fn getrpcent_r(out: Option<&mut struct_rpcent>, buf: KResult<&mut [u8]>, result: Option<&mut usize>) -> i32 {
    r_form(out, buf, result, None, true, |buf, base| ended(netdb::next_rpc(&ctx(), |p| step_r(p, buf, base, pack_rpc))))
}

// ------------------------------------------------------------------------------ aliases (§16.6)

/// `getaliasbyname(name)`.
pub fn getaliasbyname(name: KResult<&[u8]>) -> KResult<usize> {
    own_found((|| nss::alias(&ctx(), name?, |a| users::own(ALIAS, |k| pack_alias(a, k))))())
}

/// `getaliasbyname_r(name, ret, buf, buflen, result)`.
pub fn getaliasbyname_r(name: KResult<&[u8]>, out: Option<&mut struct_aliasent>, buf: KResult<&mut [u8]>, result: Option<&mut usize>) -> i32 {
    r_form(out, buf, result, None, false, |buf, base| nss::alias(&ctx(), name?, |a| pack_alias(a, &mut Packer::new(buf, base))))
}

/// `setaliasent()`.
pub fn setaliasent() {
    cursors::ALIAS_CURSOR.rewind(&CSys);
}

/// `endaliasent()`.
pub fn endaliasent() {
    cursors::ALIAS_CURSOR.end(&CSys);
}

/// `getaliasent()`.
pub fn getaliasent() -> KResult<usize> {
    own_next(netdb::next_alias(&ctx(), |a| Step::Take(users::own(ALIAS, |k| pack_alias(a, k)))))
}

/// `getaliasent_r(ret, buf, buflen, result)`.
pub fn getaliasent_r(out: Option<&mut struct_aliasent>, buf: KResult<&mut [u8]>, result: Option<&mut usize>) -> i32 {
    r_form(out, buf, result, None, true, |buf, base| ended(netdb::next_alias(&ctx(), |a| step_r(a, buf, base, pack_alias))))
}

// ------------------------------------------------------------------------------ netgroups (§16.5)

/// `setnetgrent(group)`: 1 when the group is known, else 0.
pub fn setnetgrent(group: KResult<&[u8]>) -> i32 {
    i32::from(group.is_ok_and(|g| NETGROUP_CURSOR.set(&ctx(), g)))
}

/// `endnetgrent()`.
pub fn endnetgrent() {
    NETGROUP_CURSOR.end(&CSys);
}

/// The process-wide block `getnetgrent`'s strings live in, until the next call (P-scope: the
/// enumeration is process-wide).
static NETGR_ADDR: AtomicUsize = AtomicUsize::new(0);
static NETGR_SIZE: AtomicUsize = AtomicUsize::new(0);
static NETGR_LOCK: RawMutex = RawMutex::new();

/// Copy a triple's fields into `bytes` (at address `base`): each field's address (0 for an
/// empty field), or `None` when they do not fit.
fn put_triple(t: [Option<&[u8]>; 3], bytes: &mut [u8], base: usize) -> Option<[usize; 3]> {
    let mut out = [0usize; 3];
    let mut at = 0usize;
    for (slot, f) in out.iter_mut().zip(t) {
        if let Some(f) = f {
            let end = at.checked_add(f.len())?;
            bytes.get_mut(at..end)?.copy_from_slice(f);
            *bytes.get_mut(end)? = 0;
            *slot = base.checked_add(at)?;
            at = end.checked_add(1)?;
        }
    }
    Some(out)
}

fn triple_len(t: &[Option<&[u8]>; 3]) -> usize {
    t.iter().flatten().fold(0usize, |n, f| n.saturating_add(f.len()).saturating_add(1))
}

/// `getnetgrent(&host, &user, &domain)`: 1 and the next triple (NULL for an empty field), or 0
/// at the end.
pub fn getnetgrent(cells: [Option<&mut usize>; 3]) -> i32 {
    let _g = NETGR_LOCK.lock(&CSys);
    let r = NETGROUP_CURSOR.next(&ctx(), |t| {
        let need = triple_len(&t).max(1);
        let addr = users::grown(need, NETGR_ADDR.load(Relaxed), NETGR_SIZE.load(Relaxed), |a, n| {
            NETGR_ADDR.store(a, Relaxed);
            NETGR_SIZE.store(n, Relaxed);
        })
        .ok()?;
        put_triple(t, conv::own_bytes(addr, need)?, addr)
    });
    match r {
        Ok(Some(Some(addrs))) => {
            for (cell, a) in cells.into_iter().zip(addrs) {
                if let Some(c) = cell {
                    *c = a;
                }
            }
            1
        }
        Ok(Some(None)) => {
            errno::set(Errno::ENOMEM);
            0
        }
        Ok(None) => 0,
        Err(e) => {
            errno::set(e);
            0
        }
    }
}

/// `getnetgrent_r(&host, &user, &domain, buf, buflen)`: 1 and the next triple in `buf`; 0 at the
/// end, or 0 with `ERANGE` when it does not fit (the enumeration stays on it, observed).
pub fn getnetgrent_r(cells: [Option<&mut usize>; 3], buf: KResult<&mut [u8]>) -> i32 {
    let Ok(buf) = buf else {
        errno::set(Errno::EFAULT);
        return 0;
    };
    let base = buf.as_ptr().expose_provenance();
    match NETGROUP_CURSOR.next(&ctx(), |t| put_triple(t, buf, base)) {
        Ok(Some(Some(addrs))) => {
            for (cell, a) in cells.into_iter().zip(addrs) {
                if let Some(c) = cell {
                    *c = a;
                }
            }
            1
        }
        Ok(Some(None)) => {
            errno::set(Errno::ERANGE);
            0
        }
        Ok(None) => 0,
        Err(e) => {
            errno::set(e);
            0
        }
    }
}

/// `innetgr(group, host, user, domain)`: 1 when a triple of the group matches (NULL arguments
/// match any field), else 0.
pub fn innetgr(group: KResult<&[u8]>, host: KResult<Option<&[u8]>>, user: KResult<Option<&[u8]>>, domain: KResult<Option<&[u8]>>) -> i32 {
    match (group, host, user, domain) {
        (Ok(g), Ok(h), Ok(u), Ok(d)) => i32::from(nss::in_netgroup(&ctx(), g, h, u, d)),
        _ => 0,
    }
}

// ------------------------------------------------------------------------------ the switch (§16.1)

/// `__nss_configure_lookup(db, spec)`: 0, or -1 with `EINVAL` (D-16.1).
pub fn configure_lookup(db: KResult<&[u8]>, spec: KResult<&[u8]>) -> KResult<()> {
    nss::SWITCH.configure(&CSys, db.map_err(|_| Errno::EINVAL)?, spec.map_err(|_| Errno::EINVAL)?)
}

// ------------------------------------------------------------------------------ getaddrinfo (§16.2)

/// The bytes of one entry: the `struct addrinfo` and room for a `struct sockaddr_in6`.
const ENTRY: usize = size_of::<struct_addrinfo>() + 32;

/// The `EAI_*` code of a failure (`EAI_SYSTEM` sets `errno`).
fn eai(e: GaiError) -> i32 {
    match e {
        GaiError::Code(c) => c,
        GaiError::System(x) => {
            errno::set(x);
            gai::EAI_SYSTEM
        }
    }
}

/// Write the socket address of `e` into `out` (32 bytes): its length.
fn sockaddr(e: &gai::Entry, out: &mut [u8]) -> Option<u32> {
    let port = e.port.to_be_bytes();
    let (fam, len) = match e.ip {
        Ip::V4(_) => (2u16, 16u32),
        Ip::V6(_) => (10u16, 28u32),
    };
    out.get_mut(..2)?.copy_from_slice(&fam.to_ne_bytes());
    out.get_mut(2..4)?.copy_from_slice(&port);
    match e.ip {
        Ip::V4(a) => out.get_mut(4..8)?.copy_from_slice(&a),
        Ip::V6(a) => {
            out.get_mut(8..24)?.copy_from_slice(&a);
            out.get_mut(24..28)?.copy_from_slice(&e.scope.to_ne_bytes());
        }
    }
    Some(len)
}

/// `getaddrinfo(node, service, hints, &res)`: 0 and the list in `*res`, or an `EAI_*` code.
pub fn getaddrinfo(node: KResult<Option<&[u8]>>, service: KResult<Option<&[u8]>>, hints: Option<&struct_addrinfo>, res: Option<&mut usize>) -> i32 {
    let Some(res) = res else {
        errno::set(Errno::EFAULT);
        return gai::EAI_SYSTEM;
    };
    let (Ok(node), Ok(service)) = (node, service) else {
        errno::set(Errno::EFAULT);
        return gai::EAI_SYSTEM;
    };
    let hints = hints.map(|h| Hints { flags: h.ai_flags, family: h.ai_family, socktype: h.ai_socktype, protocol: h.ai_protocol });
    let lookup = |c: &Ctx<'_, CSys>| gai::getaddrinfo(c, node, service, hints);
    let found = match if gai::uses_switch(node, service, hints) { with_hosts(lookup) } else { without_switch(lookup) } {
        Ok(f) => f,
        Err(e) => return eai(e),
    };
    match build(&found) {
        Some(head) => {
            *res = head;
            0
        }
        None => gai::EAI_MEMORY,
    }
}

/// The caller-owned block for `found`: its address (that of the first entry).
fn build(found: &gai::Found) -> Option<usize> {
    let n = found.entries.len();
    let canon_len = found.canon.as_ref().map_or(0, |c| c.len().saturating_add(1));
    let total = n.checked_mul(ENTRY)?.checked_add(canon_len)?.max(1);
    let p = caller_alloc::calloc(total, 1);
    if p.is_null() {
        return None;
    }
    let base = p.expose_provenance();
    let canon_at = base.checked_add(n.checked_mul(ENTRY)?)?;
    let built = (|| {
        if let Some(c) = &found.canon {
            liella::net::copy_z(c, conv::own_bytes(canon_at, canon_len)?)?;
        }
        for (i, e) in found.entries.iter().enumerate() {
            let at = base.checked_add(i.checked_mul(ENTRY)?)?;
            let (rec, bytes, after) = conv::own_record::<struct_addrinfo>(at, ENTRY)?;
            let len = sockaddr(e, bytes)?;
            *rec = struct_addrinfo {
                ai_flags: found.flags,
                ai_family: match e.ip {
                    Ip::V4(_) => AF_INET,
                    Ip::V6(_) => AF_INET6,
                },
                ai_socktype: e.socktype,
                ai_protocol: e.protocol,
                ai_addrlen: len,
                ai_addr: after,
                ai_canonname: if i == 0 && found.canon.is_some() { canon_at } else { 0 },
                ai_next: if i.checked_add(1)? < n { at.checked_add(ENTRY)? } else { 0 },
            };
        }
        Some(())
    })();
    match built {
        Some(()) => Some(base),
        None => {
            caller_alloc::release(base);
            None
        }
    }
}

/// `freeaddrinfo(res)`: the block `getaddrinfo` returned (NULL: nothing).
pub fn freeaddrinfo(res: usize) {
    caller_alloc::release(res);
}

/// `gai_strerror(code)`: the text's address.
pub fn gai_strerror(code: i32) -> usize {
    msg::gai(code).as_ptr().expose_provenance()
}

/// `getnameinfo(sa, salen, host, hostlen, serv, servlen, flags)`: 0, or an `EAI_*` code
/// (`EAI_OVERFLOW` when a text and its terminator do not fit its buffer). A NULL buffer or a
/// zero length asks for nothing.
pub fn getnameinfo(sa: KResult<&[u8]>, host: Option<&mut [u8]>, serv: Option<&mut [u8]>, flags: i32) -> i32 {
    let Ok(sa) = sa else { return gai::EAI_FAMILY };
    let (host, serv) = (host.filter(|b| !b.is_empty()), serv.filter(|b| !b.is_empty()));
    let (want_host, want_serv) = (host.is_some(), serv.is_some());
    let lookup = |c: &Ctx<'_, CSys>| gai::getnameinfo(c, sa, flags, want_host, want_serv);
    let names = match if gai::ni_uses_switch(flags, want_host, want_serv) { with_hosts(lookup) } else { without_switch(lookup) } {
        Ok(n) => n,
        Err(e) => return eai(e),
    };
    for (text, buf) in [(names.host, host), (names.serv, serv)] {
        if let (Some(t), Some(b)) = (text, buf) {
            if liella::net::copy_z(&t, b).is_none() {
                return gai::EAI_OVERFLOW;
            }
        }
    }
    0
}
