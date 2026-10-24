//! The C conventions of the DNS stub resolver (spec §16.4; D-16.3, D-16.6): the calling thread's
//! `_res`, the `res_*` and `__res_*` entries, the message, name and text functions (`dn_*`,
//! `ns_*`, `__p_*`, `__fp_*`, `__sym_*`, `__b64_*`, `__loc_*`, the old accessors), and the
//! resolver behind the `dns` source of host lookups. The logic is `liella::names::dns` and
//! `liella_legacy::resolv_compat`; the exports are `posix::s16_nameservices`.
//!
//! **`_res`** (D-16.3): per thread (`__res_state()`): the process's `_res` object for the
//! initial thread, a block allocated on first use for any other (freed when the thread exits).
//! It is read from `/etc/resolv.conf` and the environment on first use, and again when the file
//! changes (unless `RES_NORELOAD`); `res_init` reads it again at once. The `dns` source and
//! `getaddrinfo` use it, so changes a program makes to `_res` (servers, ports, options) apply
//! to them too (observed).
//!
//! **Failures** set `h_errno` and the state's `res_h_errno`, and `errno` when a system error is
//! the cause (`ETIMEDOUT` when no server answered). Successes leave both alone.
//!
//! **Static results** (`p_type` for an unknown type, `p_time`, `hostalias`, `loc_ntoa(NULL)`,
//! ...) are buffers per thread, one per function, overwritten by the calling thread's next call
//! (D-C0.2; NULL with `ENOMEM` when the thread cannot get its buffer).

use core::sync::atomic::Ordering::Relaxed;

use liella::error::Errno;
use liella::mem::TryVec;
use liella::names::dns::conf::{Config, RES_INIT, RES_NORELOAD};
use liella::names::dns::name::{self, Check, Compress, MAXCDNAME, MAXDNAME};
use liella::names::dns::{msg, text};
use liella_abi::layout::{ns_msg, ns_rr};
use liella_kernel::errno::KResult;
use liella_kernel::sys::Sys;
use liella_legacy::resolv_compat::{self as resolv, Env, Fail, Resolver, State, PATH_RESCONF, RESOLV_CONF, STATE_SIZE};

use crate::caller_alloc;
use crate::conv;
use crate::data;
use crate::errno;
use crate::netdb;
use crate::stdio::{self, Locking};
use crate::sys::CSys;
use crate::tb;

// ------------------------------------------------------------------------------ the state

/// The thread's state: its address (`__res_state()`).
pub fn res_state() -> usize {
    let b = tb::current();
    let a = b.res.load(Relaxed);
    if a != 0 {
        return a;
    }
    let a = if CSys.gettid() == CSys.getpid() {
        core::ptr::addr_of!(data::_res).expose_provenance()
    } else {
        // The state and, after it, the fingerprint the name-service cache compares (`pristine`).
        let p = caller_alloc::calloc(STATE_SIZE.saturating_add(8), 1);
        p.expose_provenance()
    };
    b.res.store(a, Relaxed);
    a
}

/// Free the calling thread's state (at its exit).
pub(crate) fn thread_ending() {
    let b = tb::current();
    let a = b.res.swap(0, Relaxed);
    if a != 0 && a != core::ptr::addr_of!(data::_res).expose_provenance() {
        if let Some(st) = own_state(a) {
            free_ext(&mut State(st));
        }
        caller_alloc::release(a);
    }
}

fn own_state(addr: usize) -> Option<&'static mut [u8; STATE_SIZE]> {
    conv::own_bytes(addr, STATE_SIZE).and_then(|b| <&mut [u8; STATE_SIZE]>::try_from(b).ok())
}

fn caller_state(st: KResult<&mut [u8]>) -> KResult<&mut [u8; STATE_SIZE]> {
    <&mut [u8; STATE_SIZE]>::try_from(st?).map_err(|_| Errno::EINVAL)
}

/// The environment the configuration reads: `LOCALDOMAIN`, `RES_OPTIONS` and `HOSTALIASES`, ignored in secure-execution mode (owner
/// ruling 2026-10-03; ld.so(8) lists them as removed there): a set-user-ID program must not
/// resolve names as its caller steers. The name-service cache keys hosts by the effective
/// configuration this produces (D-16.10), so an ignored value never reaches a key.
fn env_value(name: &[u8]) -> Option<&'static [u8]> {
    crate::process::secure_getenv(name).map(crate::strz::env_value)
}

fn env() -> Env<'static> {
    Env { localdomain: env_value(b"LOCALDOMAIN"), res_options: env_value(b"RES_OPTIONS") }
}

/// Free the IPv6 server addresses a state Liella initialized holds.
fn free_ext(st: &mut State<'_>) {
    if st.generation().is_none() {
        return;
    }
    for i in 0..3 {
        let a = st.ext_addr(i);
        if a != 0 {
            caller_alloc::release(a);
            st.set_ext_addr(i, 0);
        }
    }
}

/// Read the configuration into the state at `addr`.
fn init_state(bytes: &mut [u8; STATE_SIZE], addr: usize) -> KResult<()> {
    let mut st = State(bytes);
    free_ext(&mut st);
    let generation = RESOLV_CONF.generation(&CSys, PATH_RESCONF);
    let c = resolv::load(&CSys, PATH_RESCONF, env())?;
    let v6 = st.fill(&c, addr, generation, resolv::random_id(&CSys));
    for (i, s) in v6.iter().enumerate() {
        if let Some(s) = s {
            let a = caller_alloc::dup_bytes(&resolv::sockaddr_in6(s))?;
            st.set_ext_addr(i, a);
        }
    }
    record_pristine(bytes, addr);
    Ok(())
}

// ------------------------------------------------------------------------------ the cache's view

/// The configuration fingerprint of the initial thread's `_res` when Liella last read it.
static MAIN_PRINT: core::sync::atomic::AtomicU64 = core::sync::atomic::AtomicU64::new(0);

/// Where the fingerprint of the calling thread's state at `addr` is kept: `Ok` for `_res`
/// (`MAIN_PRINT`), `Err` with the 8 bytes after a state the library allocated, `None` for a
/// caller's state.
fn print_slot(addr: usize) -> Option<core::result::Result<(), &'static mut [u8]>> {
    if addr == core::ptr::addr_of!(data::_res).expose_provenance() {
        return Some(Ok(()));
    }
    if addr != 0 && tb::current().res.load(Relaxed) == addr {
        return conv::own_bytes(addr.saturating_add(STATE_SIZE), 8).map(Err);
    }
    None
}

/// After Liella filled the state at `addr` from the file and the environment: remember what it
/// showed, so that a later change by the program is seen (D-16.10: such a thread's host lookups
/// bypass the name-service cache).
fn record_pristine(bytes: &mut [u8; STATE_SIZE], addr: usize) {
    let Some(slot) = print_slot(addr) else { return };
    let print = config_of(bytes, addr).map_or(0, |c| liella_legacy::nscache::config_fingerprint(&c)) | 1;
    match slot {
        Ok(()) => MAIN_PRINT.store(print, Relaxed),
        Err(b) => b.copy_from_slice(&print.to_ne_bytes()),
    }
}

/// Whether the configuration `c` the thread's state at `addr` shows is the one Liella read (the
/// program did not change `_res` since).
fn pristine(c: &Config, addr: usize) -> bool {
    let recorded = match print_slot(addr) {
        Some(Ok(())) => MAIN_PRINT.load(Relaxed),
        Some(Err(b)) => <[u8; 8]>::try_from(&*b).map_or(0, u64::from_ne_bytes),
        None => 0,
    };
    recorded != 0 && recorded == liella_legacy::nscache::config_fingerprint(c) | 1
}

/// The configuration a state shows (see `resolv_compat`), extended from the file.
fn config_of(bytes: &mut [u8; STATE_SIZE], addr: usize) -> KResult<Config> {
    let st = State(bytes);
    let mut search: TryVec<&[u8]> = TryVec::new();
    let defd = addr.saturating_add(128)..addr.saturating_add(384);
    let mut inside: [Option<(usize, usize)>; 6] = [None; 6];
    let mut outside: [Option<&'static [u8]>; 6] = [None; 6];
    for (i, &a) in st.search_addrs().iter().enumerate() {
        if a == 0 {
            break;
        }
        if defd.contains(&a) {
            let at = a.saturating_sub(addr);
            let len = st.0.get(at..384).map_or(0, |s| s.iter().position(|&c| c == 0).unwrap_or(s.len()));
            if let Some(slot) = inside.get_mut(i) {
                *slot = Some((at, len));
            }
        } else if let Some(slot) = outside.get_mut(i) {
            *slot = conv::named_text(a);
        }
    }
    let mut v6 = [None; 3];
    for (i, slot) in v6.iter_mut().enumerate() {
        let fam_at = 20usize.saturating_add(i.saturating_mul(16));
        let fam = st.0.get(fam_at..fam_at.saturating_add(2)).and_then(|b| <[u8; 2]>::try_from(b).ok()).map_or(1, u16::from_ne_bytes);
        let a = st.ext_addr(i);
        if fam == 0 && a != 0 {
            *slot = conv::named_bytes(a, 28).and_then(|b| <[u8; 28]>::try_from(b).ok());
        }
    }
    for (inn, out) in inside.iter().zip(outside.iter()) {
        match (inn, out) {
            (Some((at, len)), _) => search.push(st.0.get(*at..at.saturating_add(*len)).unwrap_or(&[]))?,
            (None, Some(t)) => search.push(t)?,
            (None, None) => break,
        }
    }
    let servers = st.servers(&v6)?;
    let mut c = st.config(servers, &search)?;
    if c.servers.len() == 3 || c.search.len() == 6 {
        if let Ok(full) = resolv::load(&CSys, PATH_RESCONF, env()) {
            resolv::extend(&mut c, &full)?;
        }
    }
    Ok(c)
}

/// The calling thread's `_res`, read or read again as the module documentation says.
fn thread_state() -> KResult<(&'static mut [u8; STATE_SIZE], usize)> {
    let addr = res_state();
    let bytes = own_state(addr).ok_or(Errno::ENOMEM)?;
    let st = State(bytes);
    let generation = RESOLV_CONF.generation(&CSys, PATH_RESCONF);
    let stale = matches!(st.generation(), Some(g) if g != generation) && st.options() & RES_NORELOAD == 0;
    let bytes = st.0;
    if bytes.get(8).is_some_and(|&b| u64::from(b) & RES_INIT == 0) || stale {
        init_state(bytes, addr)?;
    }
    Ok((bytes, addr))
}

/// A caller's state, initialized if it is not.
fn ready(bytes: &mut [u8; STATE_SIZE], addr: usize) -> KResult<Config> {
    if State(bytes).options() & RES_INIT == 0 {
        init_state(bytes, addr)?;
    }
    config_of(bytes, addr)
}

/// The resolver of the `dns` source for the calling thread (`None`: it cannot be set up), and
/// whether the thread's `_res` is as Liella read it (the name-service cache serves only such a
/// thread, D-16.10).
pub(crate) fn host_resolver_pristine() -> Option<(Resolver, bool)> {
    let (bytes, addr) = thread_state().ok()?;
    let conf = config_of(bytes, addr).ok()?;
    let clean = pristine(&conf, addr);
    let aliases = env_value(b"HOSTALIASES").and_then(|p| {
        let mut z: TryVec<u8> = TryVec::from_slice(p).ok()?;
        z.push(0).ok()?;
        liella::system::read_file(&CSys, &z).ok()
    });
    Some((Resolver { conf, aliases }, clean))
}

/// `res_init()`: read the configuration into the thread's `_res` (0).
pub fn init() -> i32 {
    let addr = res_state();
    match own_state(addr).map(|b| init_state(b, addr)) {
        Some(Ok(())) => 0,
        Some(Err(e)) => {
            errno::set(e);
            -1
        }
        None => -1,
    }
}

/// `res_ninit(state)`.
pub fn ninit(st: KResult<&mut [u8]>) -> i32 {
    let r = caller_state(st).and_then(|b| {
        let addr = core::ptr::from_ref(&*b).expose_provenance();
        init_state(b, addr)
    });
    match r {
        Ok(()) => 0,
        Err(e) => {
            errno::set(e);
            -1
        }
    }
}

/// `res_nclose(state)`: the addresses the state holds are freed.
pub fn nclose(st: KResult<&mut [u8]>) {
    if let Ok(b) = caller_state(st) {
        free_ext(&mut State(b));
    }
}

/// `res_close()`.
pub fn close() {
    if let Some(b) = own_state(res_state()) {
        free_ext(&mut State(b));
    }
}

/// Report a failure: `h_errno`, the state's `res_h_errno`, `errno`.
fn fail(st: Option<&mut [u8; STATE_SIZE]>, f: Fail) -> i32 {
    netdb::set_h_errno(f.h);
    if let Some(b) = st {
        State(b).set_h_errno(f.h);
    }
    if let Some(e) = f.errno {
        errno::set(e);
    }
    -1
}

fn len_of(r: core::result::Result<usize, Fail>, st: Option<&mut [u8; STATE_SIZE]>) -> i32 {
    match r {
        Ok(n) => i32::try_from(n).unwrap_or(i32::MAX),
        Err(f) => fail(st, f),
    }
}

/// Which state a call uses: the thread's `_res`, or the caller's.
pub enum Which<'a> {
    /// `_res`.
    Thread,
    /// A `res_state` argument.
    Caller(KResult<&'a mut [u8]>),
}

fn with_state(w: Which<'_>, f: impl FnOnce(&Config) -> core::result::Result<usize, Fail>) -> i32 {
    let got = match w {
        Which::Thread => thread_state(),
        Which::Caller(st) => caller_state(st).map(|b| {
            let addr = core::ptr::from_ref(&*b).expose_provenance();
            (b, addr)
        }),
    };
    let (bytes, addr) = match got {
        Ok(x) => x,
        Err(e) => return fail(None, Fail { h: resolv_h::NETDB_INTERNAL, errno: Some(e) }),
    };
    let c = match ready(bytes, addr) {
        Ok(c) => c,
        Err(e) => return fail(Some(bytes), Fail { h: resolv_h::NETDB_INTERNAL, errno: Some(e) }),
    };
    len_of(f(&c), Some(bytes))
}

mod resolv_h {
    pub(super) use liella_legacy::netdb_compat::NETDB_INTERNAL;
}

fn bad_arg(e: Errno) -> Fail {
    Fail { h: resolv_h::NETDB_INTERNAL, errno: Some(e) }
}

/// `res_nquery`/`res_query`.
pub fn query(w: Which<'_>, name: KResult<&[u8]>, class: i32, rtype: i32, answer: KResult<&mut [u8]>) -> i32 {
    with_state(w, |c| resolv::nquery(&CSys, c, name.map_err(bad_arg)?, class, rtype, answer.map_err(bad_arg)?))
}

/// `res_nsearch`/`res_search`.
pub fn search(w: Which<'_>, name: KResult<&[u8]>, class: i32, rtype: i32, answer: KResult<&mut [u8]>) -> i32 {
    let aliases = env_value(b"HOSTALIASES").and_then(|p| {
        let mut z: TryVec<u8> = TryVec::from_slice(p).ok()?;
        z.push(0).ok()?;
        liella::system::read_file(&CSys, &z).ok()
    });
    with_state(w, |c| resolv::nsearch(&CSys, c, name.map_err(bad_arg)?, class, rtype, answer.map_err(bad_arg)?, aliases.as_deref()))
}

/// `res_nquerydomain`/`res_querydomain`.
pub fn querydomain(w: Which<'_>, name: KResult<&[u8]>, domain: KResult<Option<&[u8]>>, class: i32, rtype: i32, answer: KResult<&mut [u8]>) -> i32 {
    with_state(w, |c| resolv::nquerydomain(&CSys, c, name.map_err(bad_arg)?, domain.map_err(bad_arg)?, class, rtype, answer.map_err(bad_arg)?))
}

/// `res_nsend`/`res_send`.
pub fn send(w: Which<'_>, query: KResult<&[u8]>, answer: KResult<&mut [u8]>) -> i32 {
    with_state(w, |c| resolv::nsend(&CSys, c, query.map_err(bad_arg)?, answer.map_err(bad_arg)?))
}

/// `res_nmkquery`/`res_mkquery`: the query's length, or -1.
#[allow(clippy::too_many_arguments)] // the C call's arguments
pub fn mkquery(w: Which<'_>, op: i32, name: KResult<&[u8]>, class: i32, rtype: i32, data: KResult<Option<&[u8]>>, buf: KResult<&mut [u8]>) -> i32 {
    let options = match w {
        Which::Thread => thread_state().map(|(b, _)| State(b).options()),
        Which::Caller(st) => caller_state(st).and_then(|b| {
            let addr = core::ptr::from_ref(&*b).expose_provenance();
            if State(b).options() & RES_INIT == 0 {
                init_state(b, addr)?;
            }
            Ok(State(b).options())
        }),
    };
    let r = (|| -> KResult<usize> { resolv::mkquery(&CSys, options?, op, name?, class, rtype, data?, buf?) })();
    match r {
        Ok(n) => i32::try_from(n).unwrap_or(-1),
        Err(_) => -1,
    }
}

/// `res_randomid()`.
pub fn randomid() -> u32 {
    u32::from(resolv::random_id(&CSys))
}

/// `res_isourserver(sin)`: 1 when the IPv4 address and port are a server of `_res`.
pub fn is_our_server(sa: Option<&liella_abi::layout::struct_sockaddr_in>) -> i32 {
    let (Some(sa), Ok((bytes, addr))) = (sa, thread_state()) else { return 0 };
    let Ok(c) = config_of(bytes, addr) else { return 0 };
    let ip = liella::names::Ip::V4(sa.sin_addr.s_addr.to_ne_bytes());
    let port = u16::from_be(sa.sin_port);
    i32::from(c.servers.iter().any(|s| s.ip == ip && s.port == port))
}

// ------------------------------------------------------------------------------ names

/// `dn_expand(msg, eom, src, dst, dstsiz)`: the bytes the name takes at `src`, or -1.
pub fn dn_expand(msg: usize, eom: usize, src: usize, dst: KResult<&mut [u8]>) -> i32 {
    let r = (|| -> Option<usize> {
        let m = conv::named_bytes(msg, eom.checked_sub(msg)?)?;
        let at = src.checked_sub(msg)?;
        let mut w = [0u8; MAXCDNAME + 1];
        let (n, used) = name::unpack(m, at, &mut w).ok()?;
        name::expand_text(w.get(..n)?, dst.ok()?).ok()?;
        Some(used)
    })();
    r.and_then(|n| i32::try_from(n).ok()).unwrap_or(-1)
}

/// `ns_name_uncompress`: as `dn_expand`.
pub fn name_uncompress(msg: usize, eom: usize, src: usize, dst: KResult<&mut [u8]>) -> i32 {
    dn_expand(msg, eom, src, dst)
}

/// `ns_name_unpack(msg, eom, src, dst, dstsiz)`: the wire name into `dst`; the bytes taken at
/// `src`.
pub fn name_unpack(msg: usize, eom: usize, src: usize, dst: KResult<&mut [u8]>) -> i32 {
    let r = (|| -> Option<usize> {
        let m = conv::named_bytes(msg, eom.checked_sub(msg)?)?;
        let (_, used) = name::unpack(m, src.checked_sub(msg)?, dst.ok()?).ok()?;
        Some(used)
    })();
    r.and_then(|n| i32::try_from(n).ok()).unwrap_or(-1)
}

/// `dn_skipname(ptr, eom)`.
pub fn dn_skipname(ptr: usize, eom: usize) -> i32 {
    eom.checked_sub(ptr).and_then(|n| conv::named_bytes(ptr, n)).and_then(|m| name::skip(m).ok()).and_then(|n| i32::try_from(n).ok()).unwrap_or(-1)
}

/// `ns_name_skip(&ptr, eom)`: 0 and `*ptr` past the name, or -1.
pub fn name_skip(ptr: Option<&mut usize>, eom: usize) -> i32 {
    let Some(p) = ptr else { return -1 };
    match eom.checked_sub(*p).and_then(|n| conv::named_bytes(*p, n)).and_then(|m| name::skip(m).ok()) {
        Some(n) => {
            *p = p.saturating_add(n);
            0
        }
        None => {
            errno::set(Errno::EMSGSIZE);
            -1
        }
    }
}

/// The compression list of a `dnptrs` array of `slots` entries (`dnptrs[0]` the message start,
/// then names, NULL at the end): the message start and the number of names, whose offsets go
/// to the start of `out` (at least `slots - 1` entries).
fn ptr_list(dnptrs: usize, slots: usize, out: &mut [usize]) -> Option<(usize, usize)> {
    let raw = conv::named_bytes(dnptrs, slots.checked_mul(8)?)?;
    let word = |i: usize| raw.get(i.checked_mul(8)?..i.checked_mul(8)?.checked_add(8)?).and_then(|b| <[u8; 8]>::try_from(b).ok()).map(usize::from_ne_bytes);
    let base = word(0)?;
    let mut count = 0usize;
    for i in 1..slots {
        let p = word(i)?;
        if p == 0 {
            break;
        }
        *out.get_mut(count)? = p.checked_sub(base)?;
        count = count.checked_add(1)?;
    }
    Some((base, count))
}

/// The compression lists [`name_pack`] keeps on the stack (longer ones are on the heap).
const SMALL_LIST: usize = 32;

/// Write the compression list back after packing (a new entry, then NULL, when room).
fn ptr_list_store(dnptrs: usize, base: usize, names: &[usize], slots: usize) {
    let Some(raw) = conv::named_bytes_mut(dnptrs, slots.saturating_mul(8)) else { return };
    let mut put = |i: usize, v: usize| {
        if let Some(d) = raw.get_mut(i.saturating_mul(8)..i.saturating_mul(8).saturating_add(8)) {
            d.copy_from_slice(&v.to_ne_bytes());
        }
    };
    for (i, &n) in names.iter().enumerate() {
        put(i.saturating_add(1), base.saturating_add(n));
    }
    if names.len().saturating_add(1) < slots {
        put(names.len().saturating_add(1), 0);
    }
}

/// `ns_name_pack(src, dst, dstsiz, dnptrs, lastdnptr)` (`wire` the source name), and through it
/// `ns_name_compress` and `dn_comp`: the bytes written, or -1.
pub fn name_pack(wire: &[u8], dst: usize, dstsiz: usize, dnptrs: usize, last: usize) -> i32 {
    let r = (|| -> Option<usize> {
        if dnptrs == 0 {
            let out = conv::named_bytes_mut(dst, dstsiz)?;
            return name::pack(wire, out, 0, dstsiz, None).ok();
        }
        let slots = (if last == 0 { dnptrs.checked_add(16)? } else { last }).checked_sub(dnptrs)? / 8;
        let room = slots.saturating_sub(1);
        let mut small = [0usize; SMALL_LIST];
        let mut big: TryVec<usize> = TryVec::new();
        let store = if room <= SMALL_LIST {
            small.get_mut(..room)?
        } else {
            big.resize(room, 0).ok()?;
            big.as_mut_slice()
        };
        let (base, count) = ptr_list(dnptrs, slots, store)?;
        let at = dst.checked_sub(base)?;
        let region = conv::named_bytes_mut(base, at.checked_add(dstsiz)?)?;
        let list = store.get_mut(..slots.saturating_sub(2).max(count))?;
        let mut c = Compress { names: list, len: count, record: last != 0 };
        let n = name::pack(wire, region, at, dstsiz, Some(&mut c)).ok()?;
        let used = c.len;
        if last != 0 {
            ptr_list_store(dnptrs, base, store.get(..used).unwrap_or(&[]), slots);
        }
        Some(n)
    })();
    match r {
        Some(n) => i32::try_from(n).unwrap_or(-1),
        None => {
            errno::set(Errno::EMSGSIZE);
            -1
        }
    }
}

/// `dn_comp(exp_dn, comp_dn, length, dnptrs, lastdnptr)`.
pub fn dn_comp(text: KResult<&[u8]>, dst: usize, len: i32, dnptrs: usize, last: usize) -> i32 {
    let mut w = [0u8; MAXCDNAME + 1];
    match text.ok().and_then(|t| name::pton(t, &mut w).ok()) {
        Some((n, _)) => name_pack(w.get(..n).unwrap_or(&[]), dst, usize::try_from(len).unwrap_or(0), dnptrs, last),
        None => {
            errno::set(Errno::EMSGSIZE);
            -1
        }
    }
}

/// `ns_name_rollback(src, dnptrs, lastdnptr)`: the list ends before the first name at or after
/// `src`.
pub fn name_rollback(src: usize, dnptrs: usize, last: usize) {
    let Some(slots) = last.checked_sub(dnptrs).map(|n| n / 8) else { return };
    let Some(raw) = conv::named_bytes_mut(dnptrs, slots.saturating_mul(8)) else { return };
    for chunk in raw.chunks_exact_mut(8) {
        let p = usize::from_ne_bytes(<[u8; 8]>::try_from(&*chunk).unwrap_or([0; 8]));
        if p == 0 {
            return;
        }
        if p >= src {
            chunk.copy_from_slice(&0usize.to_ne_bytes());
            return;
        }
    }
}

/// `ns_name_ntop(src, dst, dstsiz)`: the bytes written with the terminator, or -1.
pub fn name_ntop(src: usize, dst: KResult<&mut [u8]>) -> i32 {
    let r = (|| -> Option<usize> { name::ntop(conv::named_wire(src)?, dst.ok()?).ok() })();
    r.and_then(|n| i32::try_from(n).ok()).unwrap_or_else(|| {
        errno::set(Errno::EMSGSIZE);
        -1
    })
}

/// `ns_name_ntol(src, dst, dstsiz)`.
pub fn name_ntol(src: usize, dst: KResult<&mut [u8]>) -> i32 {
    let r = (|| -> Option<usize> { name::ntol(conv::named_wire(src)?, dst.ok()?).ok() })();
    r.and_then(|n| i32::try_from(n).ok()).unwrap_or_else(|| {
        errno::set(Errno::EMSGSIZE);
        -1
    })
}

/// `ns_name_pton(src, dst, dstsiz)`: 1 for a fully qualified name, 0 for another, -1.
pub fn name_pton(src: KResult<&[u8]>, dst: KResult<&mut [u8]>) -> i32 {
    match (src, dst) {
        (Ok(s), Ok(d)) => match name::pton(s, d) {
            Ok((_, fq)) => i32::from(fq),
            Err(e) => {
                errno::set(e);
                -1
            }
        },
        _ => -1,
    }
}

/// `ns_name_compress(src, dst, dstsiz, dnptrs, lastdnptr)`.
pub fn name_compress(src: KResult<&[u8]>, dst: usize, dstsiz: usize, dnptrs: usize, last: usize) -> i32 {
    let mut w = [0u8; MAXCDNAME + 1];
    match src.ok().and_then(|t| name::pton(t, &mut w).ok()) {
        Some((n, _)) => name_pack(w.get(..n).unwrap_or(&[]), dst, dstsiz, dnptrs, last),
        None => {
            errno::set(Errno::EMSGSIZE);
            -1
        }
    }
}

/// `ns_name_pack` with the source name read at `src`.
pub fn name_pack_at(src: usize, dst: usize, dstsiz: usize, dnptrs: usize, last: usize) -> i32 {
    match conv::named_wire(src) {
        Some(w) => name_pack(w, dst, dstsiz, dnptrs, last),
        None => -1,
    }
}

/// The name checks.
pub fn check(name: KResult<&[u8]>, c: Check) -> i32 {
    i32::from(name.is_ok_and(|n| name::check(n, c)))
}

/// `ns_samename(a, b)`: 1, 0, or -1.
pub fn samename(a: KResult<&[u8]>, b: KResult<&[u8]>) -> i32 {
    match (a, b) {
        (Ok(a), Ok(b)) => name::samename(a, b).map_or(-1, i32::from),
        _ => -1,
    }
}

/// `ns_samedomain(a, b)`.
pub fn samedomain(a: KResult<&[u8]>, b: KResult<&[u8]>) -> i32 {
    i32::from(matches!((a, b), (Ok(a), Ok(b)) if name::samedomain(a, b)))
}

/// `ns_subdomain(a, b)`.
pub fn subdomain(a: KResult<&[u8]>, b: KResult<&[u8]>) -> i32 {
    i32::from(matches!((a, b), (Ok(a), Ok(b)) if name::subdomain(a, b)))
}

/// `ns_makecanon(src, dst, dstsize)`: 0 or -1 (`EMSGSIZE`).
pub fn makecanon(src: KResult<&[u8]>, dst: KResult<&mut [u8]>) -> i32 {
    match (src, dst) {
        (Ok(s), Ok(d)) => match name::makecanon(s, d) {
            Ok(_) => 0,
            Err(e) => {
                errno::set(e);
                -1
            }
        },
        _ => -1,
    }
}

/// `dn_count_labels(name)`.
pub fn count_labels(name: KResult<&[u8]>) -> i32 {
    name.map_or(0, name::count_labels)
}

// ------------------------------------------------------------------------------ messages

/// The bytes of the message an `ns_msg` describes.
fn handle_msg(h: &ns_msg) -> Option<&'static [u8]> {
    conv::named_bytes(h._msg, h._eom.checked_sub(h._msg)?)
}

/// `ns_initparse(msg, msglen, handle)`: 0, or -1 with `EMSGSIZE`.
pub fn initparse(m: KResult<&[u8]>, h: Option<&mut ns_msg>) -> i32 {
    let (Ok(m), Some(h)) = (m, h) else {
        errno::set(Errno::EMSGSIZE);
        return -1;
    };
    match msg::parse(m) {
        Ok(p) => {
            let base = m.as_ptr().expose_provenance();
            *h = ns_msg {
                _msg: base,
                _eom: base.saturating_add(m.len()),
                _id: p.header.id,
                _flags: p.header.flags,
                _counts: p.header.counts,
                _sections: p.sections.map(|s| s.map_or(0, |o| base.saturating_add(o))),
                _sect: 4,
                _rrnum: -1,
                _msg_ptr: 0,
            };
            0
        }
        Err(e) => {
            errno::set(e);
            -1
        }
    }
}

/// `ns_skiprr(ptr, eom, section, count)`: the bytes, or -1.
pub fn skiprr(ptr: usize, eom: usize, section: i32, count: i32) -> i32 {
    let r = (|| -> Option<usize> {
        let m = conv::named_bytes(ptr, eom.checked_sub(ptr)?)?;
        msg::skiprr(m, 0, usize::try_from(section).ok()?, usize::try_from(count).ok()?).ok()
    })();
    r.and_then(|n| i32::try_from(n).ok()).unwrap_or_else(|| {
        errno::set(Errno::EMSGSIZE);
        -1
    })
}

/// `ns_parserr(handle, section, rrnum, rr)`: 0, or -1 (`ENODEV` out of range, `EMSGSIZE`).
pub fn parserr(h: Option<&mut ns_msg>, section: i32, rrnum: i32, rr: Option<&mut ns_rr>) -> i32 {
    let (Some(h), Some(rr)) = (h, rr) else { return -1 };
    let r = (|| -> core::result::Result<(), Errno> {
        let m = handle_msg(h).ok_or(Errno::EMSGSIZE)?;
        let p = msg::Parsed { header: msg::Header { id: h._id, flags: h._flags, counts: h._counts }, sections: h._sections.map(|s| s.checked_sub(h._msg)) };
        let (s, i) = (usize::try_from(section).map_err(|_| Errno::ENODEV)?, usize::try_from(rrnum).map_err(|_| Errno::ENODEV)?);
        if s >= 4 {
            return Err(Errno::ENODEV);
        }
        let r = msg::rr(m, &p, s, i)?;
        let mut text = [0u8; MAXDNAME];
        let n = name::expand_text(r.owner(), &mut text)?;
        let mut namebuf = [0i8; 1025];
        for (d, &c) in namebuf.iter_mut().zip(text.get(..n).unwrap_or(&[])) {
            *d = i8::from_ne_bytes([c]);
        }
        *rr = ns_rr { name: namebuf, r#type: r.rtype, rr_class: r.class, ttl: r.ttl, rdlength: r.rdlen, rdata: r.rdata.map_or(0, |o| h._msg.saturating_add(o)) };
        h._sect = u32::try_from(s).unwrap_or(4);
        h._rrnum = rrnum.saturating_add(1);
        h._msg_ptr = h._msg.saturating_add(r.next);
        Ok(())
    })();
    match r {
        Ok(()) => 0,
        Err(e) => {
            errno::set(e);
            -1
        }
    }
}

/// `ns_msg_getflag(handle, flag)`.
pub fn msg_getflag(h: ns_msg, which: i32) -> i32 {
    msg::getflag(h._flags, which)
}

fn finish_text(t: &[u8], out: KResult<&mut [u8]>) -> i32 {
    match out.ok().and_then(|o| liella::net::copy_z(t, o)) {
        Some(_) => i32::try_from(t.len()).unwrap_or(-1),
        None => {
            errno::set(Errno::ENOSPC);
            -1
        }
    }
}

/// `ns_sprintrr(handle, rr, name_ctx, origin, buf, buflen)`: the text's length, or -1.
pub fn sprintrr(h: Option<&ns_msg>, rr: Option<&ns_rr>, ctx: KResult<Option<&[u8]>>, origin: KResult<Option<&[u8]>>, out: KResult<&mut [u8]>) -> i32 {
    let (Some(h), Some(rr)) = (h, rr) else { return -1 };
    let Some(m) = handle_msg(h) else { return -1 };
    let owner_len = rr.name.iter().position(|&c| c == 0).unwrap_or(rr.name.len());
    let owner: TryVec<u8> = match rr.name.get(..owner_len).map(|n| n.iter().map(|&c| c.to_ne_bytes()[0])) {
        Some(it) => {
            let mut v = TryVec::new();
            for c in it {
                if v.push(c).is_err() {
                    return -1;
                }
            }
            v
        }
        None => TryVec::new(),
    };
    let at = rr.rdata.checked_sub(h._msg).filter(|&o| o.saturating_add(usize::from(rr.rdlength)) <= m.len());
    let rdata = at.and_then(|o| m.get(o..o.saturating_add(usize::from(rr.rdlength)))).unwrap_or(&[]);
    let rec = text::Record { owner: &owner, class: rr.rr_class, rtype: rr.r#type, ttl: u64::from(rr.ttl), rdata, rdata_at: at };
    let mut t = TryVec::new();
    match text::sprintrr(m, &rec, ctx.ok().flatten(), origin.ok().flatten(), &mut t) {
        Ok(()) => finish_text(&t, out),
        Err(e) => {
            errno::set(e);
            -1
        }
    }
}

/// `ns_sprintrrf(msg, msglen, name, class, type, ttl, rdata, rdlen, name_ctx, origin, buf,
/// buflen)`.
#[allow(clippy::too_many_arguments)] // the C call's arguments
pub fn sprintrrf(m: KResult<&[u8]>, owner: KResult<&[u8]>, class: i32, rtype: i32, ttl: u64, rdata: usize, rdlen: usize, ctx: KResult<Option<&[u8]>>, origin: KResult<Option<&[u8]>>, out: KResult<&mut [u8]>) -> i32 {
    let (Ok(m), Ok(owner)) = (m, owner) else { return -1 };
    let base = m.as_ptr().expose_provenance();
    let at = rdata.checked_sub(base).filter(|&o| o.saturating_add(rdlen) <= m.len());
    let data = match at {
        Some(o) => m.get(o..o.saturating_add(rdlen)).unwrap_or(&[]),
        None => conv::named_bytes(rdata, rdlen).unwrap_or(&[]),
    };
    let rec = text::Record { owner, class: u16::try_from(class).unwrap_or(0), rtype: u16::try_from(rtype).unwrap_or(0), ttl, rdata: data, rdata_at: at };
    let mut t = TryVec::new();
    match text::sprintrr(m, &rec, ctx.ok().flatten(), origin.ok().flatten(), &mut t) {
        Ok(()) => finish_text(&t, out),
        Err(e) => {
            errno::set(e);
            -1
        }
    }
}

/// `res_nameinquery(name, type, class, buf, eom)`: 1, 0, or -1.
pub fn nameinquery(name: KResult<&[u8]>, rtype: i32, class: i32, buf: usize, eom: usize) -> i32 {
    let r = (|| -> Option<bool> {
        let m = conv::named_bytes(buf, eom.checked_sub(buf)?)?;
        msg::name_in_query(name.ok()?, u16::try_from(rtype).ok()?, u16::try_from(class).ok()?, m).ok()
    })();
    r.map_or(-1, i32::from)
}

/// `res_queriesmatch(buf1, eom1, buf2, eom2)`: 1, 0, or -1.
pub fn queriesmatch(b1: usize, e1: usize, b2: usize, e2: usize) -> i32 {
    let r = (|| -> Option<bool> {
        let a = conv::named_bytes(b1, e1.checked_sub(b1)?)?;
        let b = conv::named_bytes(b2, e2.checked_sub(b2)?)?;
        msg::queries_match(a, b).ok()
    })();
    r.map_or(-1, i32::from)
}

/// The 16- or 32-bit big-endian value at `p` (`ns_get16`, `ns_get32`, `_getshort`, `_getlong`).
pub fn get_be(p: usize, n: usize) -> u64 {
    conv::named_bytes(p, n).map_or(0, |b| b.iter().fold(0u64, |v, &x| v << 8 | u64::from(x)))
}

/// Store the low `n` bytes of `v` big-endian at `p` (`ns_put16`, `ns_put32`, `putshort`,
/// `putlong`).
pub fn put_be(v: u64, p: usize, n: usize) {
    if let Some(d) = conv::named_bytes_mut(p, n) {
        let b = v.to_be_bytes();
        d.copy_from_slice(b.get(8usize.saturating_sub(n)..).unwrap_or(&[]));
    }
}

// ------------------------------------------------------------------------------ texts

/// A static result: `text` copied (cut to fit) with a terminator into the calling thread's
/// buffer of `kind`; its address.
fn static_text<const N: usize>(kind: usize, text: &[u8]) -> usize {
    let mut b = [0u8; N];
    let n = text.len().min(N.saturating_sub(1));
    b.get_mut(..n).unwrap_or(&mut []).copy_from_slice(text.get(..n).unwrap_or(&[]));
    // The calling thread's buffer of `kind` (D-C0.2); without one, NULL with `ENOMEM`.
    match crate::users::result_buf::<N>(kind) {
        Ok(buf) => {
            buf.store(&b);
            buf.addr()
        }
        Err(e) => {
            crate::errno::set(e);
            0
        }
    }
}

fn name_addr<const N: usize>(n: text::Name, kind: usize) -> usize {
    match n {
        text::Name::Static(s) => s.as_ptr().expose_provenance(),
        other => {
            let mut t = TryVec::new();
            if other.push_to(&mut t).is_err() {
                return static_text::<N>(kind, b"");
            }
            static_text::<N>(kind, &t)
        }
    }
}

/// `p_type(type)`.
pub fn p_type(t: i32) -> usize {
    name_addr::<32>(text::p_type(t), crate::users::P_TYPE)
}

/// `p_class(class)`.
pub fn p_class(c: i32) -> usize {
    name_addr::<32>(text::p_class(c), crate::users::P_CLASS)
}

/// `p_rcode(rcode)`.
pub fn p_rcode(r: i32) -> usize {
    name_addr::<32>(text::p_rcode(r), crate::users::P_RCODE)
}

/// `p_option(option)`.
pub fn p_option(o: u64) -> usize {
    name_addr::<32>(text::p_option(o), crate::users::P_OPTION)
}

/// `p_time(value)`.
pub fn p_time(v: u32) -> usize {
    let mut t = TryVec::new();
    let _ = text::format_ttl(u64::from(v), &mut t);
    static_text::<64>(crate::users::P_TIME, &t)
}

/// `p_secstodate(secs)`.
pub fn p_secstodate(s: u64) -> usize {
    let d = text::secstodate(s);
    let n = d.iter().position(|&c| c == 0).unwrap_or(d.len());
    static_text::<16>(crate::users::P_SECSTODATE, d.get(..n).unwrap_or(&[]))
}

/// `ns_format_ttl(src, dst, dstlen)`: the text's length, or -1.
pub fn format_ttl(v: u64, out: KResult<&mut [u8]>) -> i32 {
    let mut t = TryVec::new();
    if text::format_ttl(v, &mut t).is_err() {
        return -1;
    }
    match out.ok().and_then(|o| liella::net::copy_z(&t, o)) {
        Some(_) => i32::try_from(t.len()).unwrap_or(-1),
        None => {
            errno::set(Errno::EMSGSIZE);
            -1
        }
    }
}

/// `ns_parse_ttl(src, dst)`: 0 and `*dst`, or -1 (`*dst` unchanged).
pub fn parse_ttl(src: KResult<&[u8]>, dst: Option<&mut u64>) -> i32 {
    match (src.ok().and_then(text::parse_ttl), dst) {
        (Some(v), Some(d)) => {
            *d = v;
            0
        }
        (Some(_), None) => 0,
        _ => {
            errno::set(Errno::EINVAL);
            -1
        }
    }
}

/// `ns_datetosecs(cp, errp)`: the seconds, 0 with `*errp` 1 for a bad text.
pub fn datetosecs(cp: KResult<&[u8]>, errp: Option<&mut i32>) -> u32 {
    let v = cp.ok().and_then(text::datetosecs);
    if let Some(e) = errp {
        *e = i32::from(v.is_none());
    }
    v.unwrap_or(0)
}

/// `b64_ntop(src, srclength, target, targsize)`.
pub fn b64_ntop(src: KResult<&[u8]>, out: KResult<&mut [u8]>) -> i32 {
    match (src, out) {
        (Ok(s), Ok(o)) => text::b64_ntop(s, o).ok().and_then(|n| i32::try_from(n).ok()).unwrap_or(-1),
        _ => -1,
    }
}

/// `b64_pton(src, target, targsize)` (`target` NULL: the length only).
pub fn b64_pton(src: KResult<&[u8]>, out: KResult<Option<&mut [u8]>>) -> i32 {
    match (src, out) {
        (Ok(s), Ok(o)) => text::b64_pton(s, o).ok().and_then(|n| i32::try_from(n).ok()).unwrap_or(-1),
        _ => -1,
    }
}

/// `loc_aton(ascii, binary)`: 16, or 0.
pub fn loc_aton(ascii: KResult<&[u8]>, binary: KResult<&mut [u8]>) -> i32 {
    match (ascii.ok().and_then(text::loc_aton), binary) {
        (Some(b), Ok(out)) => {
            out.copy_from_slice(b.get(..out.len()).unwrap_or(&b));
            16
        }
        _ => 0,
    }
}

/// `loc_ntoa(binary, ascii)`: the text in `ascii` (NULL: a static buffer); its address.
pub fn loc_ntoa(binary: KResult<&[u8]>, ascii: usize) -> usize {
    let Some(b) = binary.ok().and_then(|b| <[u8; 16]>::try_from(b).ok()) else { return 0 };
    let mut t = TryVec::new();
    if text::loc_ntoa(&b, &mut t).is_err() {
        return 0;
    }
    if ascii == 0 {
        return static_text::<128>(crate::users::LOC_NTOA, &t);
    }
    match conv::named_bytes_mut(ascii, t.len().saturating_add(1)).and_then(|o| liella::net::copy_z(&t, o)) {
        Some(_) => ascii,
        None => 0,
    }
}

/// The entries of a `struct res_sym` array (until the entry whose name is NULL).
fn syms(p: usize) -> TryVec<(i32, usize, usize)> {
    let mut out = TryVec::new();
    let mut at = p;
    for _ in 0..4096 {
        let Some(b) = conv::named_bytes(at, 24) else { break };
        let num = i32::from_ne_bytes(<[u8; 4]>::try_from(b.get(..4).unwrap_or(&[0; 4])).unwrap_or([0; 4]));
        let name = usize::from_ne_bytes(<[u8; 8]>::try_from(b.get(8..16).unwrap_or(&[0; 8])).unwrap_or([0; 8]));
        let human = usize::from_ne_bytes(<[u8; 8]>::try_from(b.get(16..24).unwrap_or(&[0; 8])).unwrap_or([0; 8]));
        if name == 0 || out.push((num, name, human)).is_err() {
            break;
        }
        at = at.saturating_add(24);
    }
    out
}

/// `sym_ston(syms, name, success)`.
pub fn sym_ston(table: usize, name: KResult<&[u8]>, ok: Option<&mut i32>) -> i32 {
    let found = name.ok().and_then(|n| syms(table).iter().find(|e| conv::named_text(e.1).is_some_and(|t| t.eq_ignore_ascii_case(n))).map(|e| e.0));
    if let Some(o) = ok {
        *o = i32::from(found.is_some());
    }
    found.unwrap_or(0)
}

/// `sym_ntos(syms, number, success)` (`human`: `sym_ntop`).
pub fn sym_ntos(table: usize, number: i32, ok: Option<&mut i32>, human: bool) -> usize {
    let e = syms(table).iter().find(|e| e.0 == number).copied();
    if let Some(o) = ok {
        *o = i32::from(e.is_some());
    }
    match e {
        Some((_, n, h)) => {
            if human {
                h
            } else {
                n
            }
        }
        None => name_addr::<32>(text::Name::Number(i64::from(number)), if human { crate::users::SYM_NTOP } else { crate::users::SYM_NTOS }),
    }
}

/// Write `t` to the stream `fp`.
fn put(fp: usize, t: &[u8]) {
    let _ = stdio::fputs(Ok(t), fp, Locking::Locked);
}

/// `fp_nquery(msg, len, file)`.
pub fn fp_nquery(m: usize, len: i32, fp: usize) {
    let pfcode = thread_state().map_or(0, |(b, _)| State(b).pfcode());
    let mut t = TryVec::new();
    let bytes = usize::try_from(len).ok().and_then(|n| conv::named_bytes(m, n)).unwrap_or(&[]);
    if text::nquery(bytes, pfcode, &mut t).is_ok() {
        put(fp, &t);
    }
}

/// `fp_query(msg, file)`: `fp_nquery` of 512 bytes (`PACKETSZ`; observed).
pub fn fp_query(m: usize, fp: usize) {
    fp_nquery(m, 512, fp);
}

/// `p_query(msg)`: to standard output.
pub fn p_query(m: usize) {
    fp_query(m, stdio::stdout());
}

/// `fp_resstat(state, file)`.
pub fn fp_resstat(st: KResult<&mut [u8]>, fp: usize) {
    let Ok(b) = caller_state(st) else { return };
    let mut t = TryVec::new();
    if text::resstat(State(b).options(), &mut t).is_ok() {
        put(fp, &t);
    }
}

/// The name at `cp` of the message at `msg` (`len` bytes): its text and the bytes it takes.
fn name_in(cp: usize, m: usize, len: usize) -> Option<(TryVec<u8>, usize)> {
    let bytes = conv::named_bytes(m, len)?;
    let mut w = [0u8; MAXCDNAME + 1];
    let (n, used) = name::unpack(bytes, cp.checked_sub(m)?, &mut w).ok()?;
    let mut t = [0u8; MAXDNAME];
    let k = name::expand_text(w.get(..n)?, &mut t).ok()?;
    Some((TryVec::from_slice(t.get(..k.saturating_sub(1))?).ok()?, used))
}

/// `p_cdnname(cp, msg, len, file)` (`fq`: `p_fqname`, with a final dot): the address after
/// the name, or NULL.
pub fn p_cdnname(cp: usize, m: usize, len: i32, fp: usize, fq: bool) -> usize {
    let Some((mut t, used)) = usize::try_from(len).ok().and_then(|l| name_in(cp, m, l)) else { return 0 };
    if fq && t.last() != Some(&b'.') && t.push(b'.').is_err() {
        return 0;
    }
    put(fp, &t);
    cp.saturating_add(used)
}

/// `p_fqnname(cp, msg, msglen, name, namelen)`: the absolute name into `name`; the address
/// after the name, or NULL.
pub fn p_fqnname(cp: usize, m: usize, len: i32, out: KResult<&mut [u8]>) -> usize {
    let Some((mut t, used)) = usize::try_from(len).ok().and_then(|l| name_in(cp, m, l)) else { return 0 };
    if t.last() != Some(&b'.') && t.push(b'.').is_err() {
        return 0;
    }
    match out.ok().and_then(|o| liella::net::copy_z(&t, o)) {
        Some(_) => cp.saturating_add(used),
        None => 0,
    }
}

/// `hostalias(name)` (`res_hostalias` with `buf`): the name `HOSTALIASES` gives, or NULL.
pub fn hostalias(st_options: Option<u64>, name: KResult<&[u8]>, buf: Option<&mut [u8]>) -> usize {
    let options = st_options.unwrap_or_else(|| thread_state().map_or(0, |(b, _)| State(b).options()));
    if options & liella::names::dns::conf::RES_NOALIASES != 0 {
        return 0;
    }
    let Ok(name) = name else { return 0 };
    let Some(p) = env_value(b"HOSTALIASES") else { return 0 };
    let Ok(mut z) = TryVec::from_slice(p) else { return 0 };
    if z.push(0).is_err() {
        return 0;
    }
    let Ok(text) = liella::system::read_file(&CSys, &z) else { return 0 };
    let Some(real) = resolv::hostalias(name, &text) else { return 0 };
    match buf {
        Some(b) => match liella::net::copy_z(real, b) {
            Some(_) => b.as_ptr().expose_provenance(),
            None => 0,
        },
        None => static_text::<1025>(crate::users::HOSTALIAS, real),
    }
}

/// The options of a caller's state (for `res_hostalias`).
pub fn state_options(st: KResult<&mut [u8]>) -> Option<u64> {
    caller_state(st).ok().map(|b| State(b).options())
}
