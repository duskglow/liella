//! Translation for spec §16: name services and databases (phase 10, part 3: the Name Service
//! Switch, `getaddrinfo`/`getnameinfo`, the legacy host and network lookups, services,
//! protocols, RPC programs, netgroups and mail aliases). The C conventions are in
//! `crate::netdb`. Part 4: the DNS resolver (§16.4, `crate::resolv`), `getaddrinfo_a` and its
//! companions (`crate::gai_async`). The compat-only names (`__nss_next`, `__nss_*_lookup`,
//! `__nss_database_lookup`, the `_gethtby*` family, `res_gethostby*`) wait for phase 15's
//! compat versions.

use liella::names::dns::name::Check;
use liella_abi::ctypes::{c_char, c_int, c_uint, c_ulong, c_void, size_t};
use liella_abi::layout::{ns_msg, ns_rr, struct___res_state, struct_addrinfo, struct_aliasent, struct_hostent, struct_netent, struct_protoent, struct_rpcent, struct_servent, struct_sockaddr_in};
use liella_kernel::uapi::{SigEvent, Timespec};

use crate::conv;
use crate::export;
use crate::gai_async;
use crate::netdb;
use crate::resolv;
use crate::ret;
use crate::strz;

const AF_INET: c_int = 2;

// ------------------------------------------------------------------------------ §16.1

export! { "GLIBC_2.2.5";
pub extern "C" fn __nss_configure_lookup(db: *const c_char, spec: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: db and spec are strings (nss.h).
    ret::zero(netdb::configure_lookup(unsafe { strz::strz(db) }, unsafe { strz::strz(spec) }))
}}

// The compat-only switch internals (`__nss_database_lookup`, `__nss_next`, `__nss_group_lookup`,
// `__nss_hosts_lookup`, `__nss_passwd_lookup`, all `@GLIBC_2.2.5`, no default version; phase 15
// LG-7). No header declares them; old NSS modules and programs may still bind them. Black-box
// (spec/behaviour/nss-resolv-compat.md) they answer nothing: every call returns -1. Exported only
// under the compat version by the stubs `cargo xtask compat-gen` writes (their `behaviour` rows in
// spec/abi/compat-plan.tsv), never into `libc.a` (R15-2); tests call these Rust functions.
crate::internal_entry! {
/// `__nss_database_lookup@GLIBC_2.2.5`: -1; the fourth argument (the result slot) is cleared when
/// not NULL (the reference dereferences it unconditionally; Liella does not fault on NULL).
pub extern "C" fn liella_compat___nss_database_lookup(_db: *const c_char, _alt: *const c_char, _defconfig: *const c_char, ni: Option<&mut usize>) -> c_int {
    if let Some(ni) = ni {
        *ni = 0;
    }
    -1
}}

crate::internal_entry! {
/// `__nss_next@GLIBC_2.2.5`: -1, writes nothing, `errno` unchanged.
pub extern "C" fn liella_compat___nss_next() -> c_int {
    -1
}}

crate::internal_entry! {
/// `__nss_group_lookup@GLIBC_2.2.5`: -1 with `errno` `ENOSYS`, writes nothing (one entry per plan behaviour
/// row, so compat-gen finds `liella_compat___nss_group_lookup`).
pub extern "C" fn liella_compat___nss_group_lookup() -> c_int {
    ret::zero(Err(liella_kernel::errno::Errno::ENOSYS))
}}
crate::internal_entry! {
/// `__nss_hosts_lookup@GLIBC_2.2.5`: -1 with `errno` `ENOSYS`, writes nothing (one entry per plan behaviour
/// row, so compat-gen finds `liella_compat___nss_hosts_lookup`).
pub extern "C" fn liella_compat___nss_hosts_lookup() -> c_int {
    ret::zero(Err(liella_kernel::errno::Errno::ENOSYS))
}}
crate::internal_entry! {
/// `__nss_passwd_lookup@GLIBC_2.2.5`: -1 with `errno` `ENOSYS`, writes nothing (one entry per plan behaviour
/// row, so compat-gen finds `liella_compat___nss_passwd_lookup`).
pub extern "C" fn liella_compat___nss_passwd_lookup() -> c_int {
    ret::zero(Err(liella_kernel::errno::Errno::ENOSYS))
}}

// The libresolv.so.2 compat-only hosts-file walkers (LG-7 part 2, spec/behaviour/nss-resolv-compat.md):
// compat-plan.tsv routes them to libresolv.so.2 (D-16.6); they are exported there when phase 14
// builds that object, until then tests call these Rust functions.
crate::internal_entry! {
/// `_gethtbyname@GLIBC_2.2.5`: `_gethtbyname2(name, AF_INET)`.
pub extern "C" fn liella_compat__gethtbyname(name: *const c_char) -> *mut struct_hostent {
    // UNSAFE(slice) SAFETY: name is a string (as gethostbyname, §16.3).
    ret::ptr(netdb::compat_gethtbyname2(unsafe { strz::strz(name) }, AF_INET))
}}

crate::internal_entry! {
/// `_gethtbyname2@GLIBC_2.2.5`: the hosts file's first entry of family `af` naming `name`.
pub extern "C" fn liella_compat__gethtbyname2(name: *const c_char, af: c_int) -> *mut struct_hostent {
    // UNSAFE(slice) SAFETY: name is a string (as gethostbyname2, §16.3).
    ret::ptr(netdb::compat_gethtbyname2(unsafe { strz::strz(name) }, af))
}}

crate::internal_entry! {
/// `_gethtbyaddr@GLIBC_2.2.5`: the hosts file's first entry of family `af` whose address begins with `addr`'s `len` bytes.
pub extern "C" fn liella_compat__gethtbyaddr(addr: *const c_void, len: c_int, af: c_int) -> *mut struct_hostent {
    let len = u32::try_from(len).unwrap_or(u32::MAX);
    // UNSAFE(slice) SAFETY: addr holds len bytes (as gethostbyaddr, §16.3); a length over 16 is never read.
    ret::ptr(netdb::compat_gethtbyaddr(if len > 16 { Err(liella_kernel::errno::Errno::EINVAL) } else { unsafe { conv::sockaddr(addr, len) } }, af))
}}

crate::internal_entry! {
/// `_sethtent@GLIBC_2.2.5`: rewinds `_gethtent`'s own cursor.
pub extern "C" fn liella_compat__sethtent(_stayopen: c_int) {
    netdb::compat_sethtent()
}}

crate::internal_entry! {
/// `_gethtent@GLIBC_2.2.5`: the hosts file's next entry, either family; NULL at the end.
pub extern "C" fn liella_compat__gethtent() -> *mut struct_hostent {
    ret::ptr(netdb::compat_gethtent())
}}

crate::internal_entry! {
/// `res_gethostbyname@GLIBC_2.2.5`: the `dns` source alone, `AF_INET` (numeric dotted quads read).
pub extern "C" fn liella_compat_res_gethostbyname(name: *const c_char) -> *mut struct_hostent {
    // UNSAFE(slice) SAFETY: name is a string (as gethostbyname, §16.3).
    ret::ptr(netdb::compat_res_gethostbyname(unsafe { strz::strz(name) }))
}}

crate::internal_entry! {
/// `res_gethostbyname2@GLIBC_2.2.5`: `res_gethostbyname(name)`; `af` is ignored (observed).
pub extern "C" fn liella_compat_res_gethostbyname2(name: *const c_char, _af: c_int) -> *mut struct_hostent {
    // UNSAFE(slice) SAFETY: name is a string (as gethostbyname2, §16.3).
    ret::ptr(netdb::compat_res_gethostbyname(unsafe { strz::strz(name) }))
}}

crate::internal_entry! {
/// `res_gethostbyaddr@GLIBC_2.2.5`: the `dns` source alone for `addr`'s `len` bytes of family `af`.
pub extern "C" fn liella_compat_res_gethostbyaddr(addr: *const c_void, len: c_int, af: c_int) -> *mut struct_hostent {
    let len = u32::try_from(len).unwrap_or(u32::MAX);
    // UNSAFE(slice) SAFETY: addr holds len bytes (as gethostbyaddr, §16.3); a length over 16 is never read.
    ret::ptr(netdb::compat_res_gethostbyaddr(if len > 16 { Err(liella_kernel::errno::Errno::EINVAL) } else { unsafe { conv::sockaddr(addr, len) } }, af))
}}

// ------------------------------------------------------------------------------ §16.2

export! { "GLIBC_2.2.5";
pub extern "C" fn getaddrinfo(node: *const c_char, service: *const c_char, hints: Option<&struct_addrinfo>, res: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: node and service are NULL or strings (§16.2).
    netdb::getaddrinfo(unsafe { strz::strz_opt(node) }, unsafe { strz::strz_opt(service) }, hints, res)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn freeaddrinfo(res: *mut struct_addrinfo) {
    netdb::freeaddrinfo(conv::addr(res))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn gai_strerror(code: c_int) -> *const c_char {
    ret::ptr(netdb::gai_strerror(code))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getnameinfo(sa: *const c_void, salen: u32, host: *mut c_char, hostlen: u32, serv: *mut c_char, servlen: u32, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: sa holds salen bytes; host and serv are NULL or hold hostlen and servlen bytes (§16.2).
    netdb::getnameinfo(unsafe { conv::sockaddr(sa, salen) }, unsafe { conv::opt_buf_mut(host, conv::len32(hostlen)) }.ok().flatten(), unsafe { conv::opt_buf_mut(serv, conv::len32(servlen)) }.ok().flatten(), flags)
}}

// ------------------------------------------------------------------------------ §16.3: hosts

export! { "GLIBC_2.2.5";
pub extern "C" fn gethostbyname(name: *const c_char) -> *mut struct_hostent {
    // UNSAFE(slice) SAFETY: name is a string (§16.3).
    ret::ptr(netdb::gethostbyname2(unsafe { strz::strz(name) }, AF_INET))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn gethostbyname2(name: *const c_char, af: c_int) -> *mut struct_hostent {
    // UNSAFE(slice) SAFETY: name is a string (§16.3).
    ret::ptr(netdb::gethostbyname2(unsafe { strz::strz(name) }, af))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn gethostbyaddr(addr: *const c_void, len: u32, af: c_int) -> *mut struct_hostent {
    // UNSAFE(slice) SAFETY: addr holds len bytes (§16.3).
    ret::ptr(netdb::gethostbyaddr(unsafe { conv::sockaddr(addr, len) }, af))
}}

export! { "GLIBC_2.2.5";
#[allow(clippy::too_many_arguments)]
pub extern "C" fn __nss_hostname_digits_dots(name: *const c_char, resbuf: Option<&mut struct_hostent>, buffer: Option<&mut usize>, buffer_size: Option<&mut size_t>, buflen: size_t, result: Option<&mut usize>, status: Option<&mut c_int>, af: c_int, h_errnop: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: name is a string; *buffer holds buflen bytes, or is the block of *buffer_size bytes this call sizes with realloc (§16.1).
    crate::netaddr::digits_dots(unsafe { strz::strz(name) }, af, crate::netaddr::DigitsDots { resbuf, buffer, buffer_size, buflen, result, status, h_errnop }, |a, n| unsafe { conv::buf_mut(core::ptr::with_exposed_provenance_mut::<c_void>(a), n) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn gethostbyname_r(name: *const c_char, out: Option<&mut struct_hostent>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>, h_errnop: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: name is a string; buf holds buflen bytes (§16.3).
    netdb::gethostbyname2_r(unsafe { strz::strz(name) }, AF_INET, out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result, h_errnop)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn gethostbyname2_r(name: *const c_char, af: c_int, out: Option<&mut struct_hostent>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>, h_errnop: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: name is a string; buf holds buflen bytes (§16.3).
    netdb::gethostbyname2_r(unsafe { strz::strz(name) }, af, out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result, h_errnop)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn gethostbyaddr_r(addr: *const c_void, len: u32, af: c_int, out: Option<&mut struct_hostent>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>, h_errnop: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: addr holds len bytes; buf holds buflen bytes (§16.3).
    netdb::gethostbyaddr_r(unsafe { conv::sockaddr(addr, len) }, af, out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result, h_errnop)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sethostent(_stayopen: c_int) {
    netdb::sethostent()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn endhostent() {
    netdb::endhostent()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn gethostent() -> *mut struct_hostent {
    ret::ptr(netdb::gethostent())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn gethostent_r(out: Option<&mut struct_hostent>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>, h_errnop: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds buflen bytes (§16.3).
    netdb::gethostent_r(out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result, h_errnop)
}}

// ------------------------------------------------------------------------------ §16.3: networks

export! { "GLIBC_2.2.5";
pub extern "C" fn getnetbyname(name: *const c_char) -> *mut struct_netent {
    // UNSAFE(slice) SAFETY: name is a string (§16.3).
    ret::ptr(netdb::getnetbyname(unsafe { strz::strz(name) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getnetbyname_r(name: *const c_char, out: Option<&mut struct_netent>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>, h_errnop: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: name is a string; buf holds buflen bytes (§16.3).
    netdb::getnetbyname_r(unsafe { strz::strz(name) }, out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result, h_errnop)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getnetbyaddr(net: u32, kind: c_int) -> *mut struct_netent {
    ret::ptr(netdb::getnetbyaddr(net, kind))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getnetbyaddr_r(net: u32, kind: c_int, out: Option<&mut struct_netent>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>, h_errnop: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds buflen bytes (§16.3).
    netdb::getnetbyaddr_r(net, kind, out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result, h_errnop)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setnetent(_stayopen: c_int) {
    netdb::setnetent()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn endnetent() {
    netdb::endnetent()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getnetent() -> *mut struct_netent {
    ret::ptr(netdb::getnetent())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getnetent_r(out: Option<&mut struct_netent>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>, h_errnop: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds buflen bytes (§16.3).
    netdb::getnetent_r(out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result, h_errnop)
}}

// ------------------------------------------------------------------------------ §16.3: h_errno

export! { "GLIBC_2.2.5";
pub extern "C" fn __h_errno_location() -> *mut c_int {
    netdb::h_errno_location()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn herror(s: *const c_char) {
    // UNSAFE(slice) SAFETY: s is NULL or a string (§16.3).
    netdb::herror(unsafe { strz::strz_opt(s) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn hstrerror(code: c_int) -> *const c_char {
    ret::ptr(netdb::hstrerror(code))
}}

// ------------------------------------------------------------------------------ §16.5: services

export! { "GLIBC_2.2.5";
pub extern "C" fn getservbyname(name: *const c_char, proto: *const c_char) -> *mut struct_servent {
    // UNSAFE(slice) SAFETY: name is a string, proto NULL or a string (§16.5).
    ret::null(netdb::getservbyname(unsafe { strz::strz(name) }, unsafe { strz::strz_opt(proto) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getservbyname_r(name: *const c_char, proto: *const c_char, out: Option<&mut struct_servent>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: name is a string, proto NULL or a string; buf holds buflen bytes (§16.5).
    netdb::getservbyname_r(unsafe { strz::strz(name) }, unsafe { strz::strz_opt(proto) }, out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getservbyport(port: c_int, proto: *const c_char) -> *mut struct_servent {
    // UNSAFE(slice) SAFETY: proto is NULL or a string (§16.5).
    ret::null(netdb::getservbyport(port, unsafe { strz::strz_opt(proto) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getservbyport_r(port: c_int, proto: *const c_char, out: Option<&mut struct_servent>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: proto is NULL or a string; buf holds buflen bytes (§16.5).
    netdb::getservbyport_r(port, unsafe { strz::strz_opt(proto) }, out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setservent(_stayopen: c_int) {
    netdb::setservent()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn endservent() {
    netdb::endservent()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getservent() -> *mut struct_servent {
    ret::null(netdb::getservent())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getservent_r(out: Option<&mut struct_servent>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds buflen bytes (§16.5).
    netdb::getservent_r(out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

// ------------------------------------------------------------------------------ §16.5: protocols

export! { "GLIBC_2.2.5";
pub extern "C" fn getprotobyname(name: *const c_char) -> *mut struct_protoent {
    // UNSAFE(slice) SAFETY: name is a string (§16.5).
    ret::null(netdb::getprotobyname(unsafe { strz::strz(name) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getprotobyname_r(name: *const c_char, out: Option<&mut struct_protoent>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: name is a string; buf holds buflen bytes (§16.5).
    netdb::getprotobyname_r(unsafe { strz::strz(name) }, out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getprotobynumber(number: c_int) -> *mut struct_protoent {
    ret::null(netdb::getprotobynumber(number))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getprotobynumber_r(number: c_int, out: Option<&mut struct_protoent>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds buflen bytes (§16.5).
    netdb::getprotobynumber_r(number, out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setprotoent(_stayopen: c_int) {
    netdb::setprotoent()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn endprotoent() {
    netdb::endprotoent()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getprotoent() -> *mut struct_protoent {
    ret::null(netdb::getprotoent())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getprotoent_r(out: Option<&mut struct_protoent>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds buflen bytes (§16.5).
    netdb::getprotoent_r(out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

// ------------------------------------------------------------------------------ §16.5: RPC programs

export! { "GLIBC_2.2.5";
pub extern "C" fn getrpcbyname(name: *const c_char) -> *mut struct_rpcent {
    // UNSAFE(slice) SAFETY: name is a string (§16.5).
    ret::null(netdb::getrpcbyname(unsafe { strz::strz(name) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getrpcbyname_r(name: *const c_char, out: Option<&mut struct_rpcent>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: name is a string; buf holds buflen bytes (§16.5).
    netdb::getrpcbyname_r(unsafe { strz::strz(name) }, out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getrpcbynumber(number: c_int) -> *mut struct_rpcent {
    ret::null(netdb::getrpcbynumber(number))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getrpcbynumber_r(number: c_int, out: Option<&mut struct_rpcent>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds buflen bytes (§16.5).
    netdb::getrpcbynumber_r(number, out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setrpcent(_stayopen: c_int) {
    netdb::setrpcent()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn endrpcent() {
    netdb::endrpcent()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getrpcent() -> *mut struct_rpcent {
    ret::null(netdb::getrpcent())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getrpcent_r(out: Option<&mut struct_rpcent>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds buflen bytes (§16.5).
    netdb::getrpcent_r(out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

// ------------------------------------------------------------------------------ §16.5: netgroups

export! { "GLIBC_2.2.5";
pub extern "C" fn setnetgrent(group: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: group is a string (§16.5).
    netdb::setnetgrent(unsafe { strz::strz(group) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getnetgrent(host: Option<&mut usize>, user: Option<&mut usize>, domain: Option<&mut usize>) -> c_int {
    netdb::getnetgrent([host, user, domain])
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getnetgrent_r(host: Option<&mut usize>, user: Option<&mut usize>, domain: Option<&mut usize>, buf: *mut c_char, buflen: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds buflen bytes (§16.5).
    netdb::getnetgrent_r([host, user, domain], unsafe { conv::buf_mut(buf.cast(), buflen) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn endnetgrent() {
    netdb::endnetgrent()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn innetgr(group: *const c_char, host: *const c_char, user: *const c_char, domain: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: group is a string; host, user and domain are NULL or strings (§16.5).
    netdb::innetgr(unsafe { strz::strz(group) }, unsafe { strz::strz_opt(host) }, unsafe { strz::strz_opt(user) }, unsafe { strz::strz_opt(domain) })
}}

// ------------------------------------------------------------------------------ §16.6

export! { "GLIBC_2.2.5";
pub extern "C" fn setaliasent() {
    netdb::setaliasent()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn endaliasent() {
    netdb::endaliasent()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getaliasent() -> *mut struct_aliasent {
    ret::null(netdb::getaliasent())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getaliasent_r(out: Option<&mut struct_aliasent>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: buf holds buflen bytes (§16.6).
    netdb::getaliasent_r(out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getaliasbyname(name: *const c_char) -> *mut struct_aliasent {
    // UNSAFE(slice) SAFETY: name is a string (§16.6).
    ret::null(netdb::getaliasbyname(unsafe { strz::strz(name) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getaliasbyname_r(name: *const c_char, out: Option<&mut struct_aliasent>, buf: *mut c_char, buflen: size_t, result: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: name is a string; buf holds buflen bytes (§16.6).
    netdb::getaliasbyname_r(unsafe { strz::strz(name) }, out, unsafe { conv::buf_mut(buf.cast(), buflen) }, result)
}}

// ------------------------------------------------------------------------------ §16.2: getaddrinfo_a
//
// The functions of libanl, part of libc since 2.34 (`GLIBC_2.34`; the `GLIBC_2.2.5` versions are
// compat-only, phase 15). The logic is `crate::gai_async`.

export! { "GLIBC_2.34";
pub extern "C" fn getaddrinfo_a(mode: c_int, list: *const *mut c_void, nitems: c_int, sevp: Option<&SigEvent>) -> c_int {
    // UNSAFE(slice) SAFETY: list holds nitems entries, each NULL or a struct gaicb (netdb.h).
    gai_async::getaddrinfo_a(mode, unsafe { conv::elements(list.cast(), conv::nonneg(nitems), 8) }, sevp)
}}

export! { "GLIBC_2.34";
pub extern "C" fn gai_suspend(list: *const *const c_void, nitems: c_int, timeout: Option<&Timespec>) -> c_int {
    // UNSAFE(slice) SAFETY: list holds nitems entries, each NULL or a struct gaicb (netdb.h).
    gai_async::gai_suspend(unsafe { conv::elements(list.cast(), conv::nonneg(nitems), 8) }, timeout)
}}

export! { "GLIBC_2.34";
pub extern "C" fn gai_error(req: *mut c_void) -> c_int {
    gai_async::gai_error(conv::addr(req))
}}

export! { "GLIBC_2.34";
pub extern "C" fn gai_cancel(req: *mut c_void) -> c_int {
    gai_async::gai_cancel(conv::addr(req))
}}

// ------------------------------------------------------------------------------ §16.4: the state
//
// Library placement (D-16.6): the names Appendix C gives `libresolv.so.2` are in `libc.a` and
// not in the version script of `libc.so.6`; the `__res_*` forms of `GLIBC_2.2.5` that 2.34
// replaced with plain names are compat versions of the same functions (their own exports here).

const STATE: usize = size_of::<struct___res_state>();

export! { "GLIBC_2.2.5";
pub extern "C" fn __res_state() -> *mut struct___res_state {
    core::ptr::with_exposed_provenance_mut(resolv::res_state())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __res_init() -> c_int {
    resolv::init()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __res_ninit(statep: *mut struct___res_state) -> c_int {
    // UNSAFE(slice) SAFETY: statep is a struct __res_state (resolv.h).
    resolv::ninit(unsafe { conv::buf_mut(statep.cast(), STATE) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __res_nclose(statep: *mut struct___res_state) {
    // UNSAFE(slice) SAFETY: statep is a struct __res_state (resolv.h).
    resolv::nclose(unsafe { conv::buf_mut(statep.cast(), STATE) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __res_close() {
    resolv::close()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __res_randomid() -> c_uint {
    resolv::randomid()
}}

// ------------------------------------------------------------------------------ §16.4: queries

macro_rules! res_query_fns {
    ($v:literal, $q:ident, $s:ident, $d:ident, $send:ident, $mk:ident) => {
        export! { $v;
        pub extern "C" fn $q(dname: *const c_char, class: c_int, rtype: c_int, answer: *mut u8, anslen: c_int) -> c_int {
            // UNSAFE(slice) SAFETY: dname is a string; answer holds anslen bytes (resolv.h).
            resolv::query(resolv::Which::Thread, unsafe { strz::strz(dname) }, class, rtype, unsafe { conv::buf_mut(answer.cast(), conv::nonneg(anslen)) })
        }}

        export! { $v;
        pub extern "C" fn $s(dname: *const c_char, class: c_int, rtype: c_int, answer: *mut u8, anslen: c_int) -> c_int {
            // UNSAFE(slice) SAFETY: dname is a string; answer holds anslen bytes (resolv.h).
            resolv::search(resolv::Which::Thread, unsafe { strz::strz(dname) }, class, rtype, unsafe { conv::buf_mut(answer.cast(), conv::nonneg(anslen)) })
        }}

        export! { $v;
        pub extern "C" fn $d(name: *const c_char, domain: *const c_char, class: c_int, rtype: c_int, answer: *mut u8, anslen: c_int) -> c_int {
            // UNSAFE(slice) SAFETY: name is a string, domain NULL or a string; answer holds anslen bytes (resolv.h).
            resolv::querydomain(resolv::Which::Thread, unsafe { strz::strz(name) }, unsafe { strz::strz_opt(domain) }, class, rtype, unsafe { conv::buf_mut(answer.cast(), conv::nonneg(anslen)) })
        }}

        export! { $v;
        pub extern "C" fn $send(msg: *const u8, msglen: c_int, answer: *mut u8, anslen: c_int) -> c_int {
            // UNSAFE(slice) SAFETY: msg holds msglen bytes, answer anslen bytes (resolv.h).
            resolv::send(resolv::Which::Thread, unsafe { conv::buf(msg.cast(), conv::nonneg(msglen)) }, unsafe { conv::buf_mut(answer.cast(), conv::nonneg(anslen)) })
        }}

        export! { $v;
        pub extern "C" fn $mk(op: c_int, dname: *const c_char, class: c_int, rtype: c_int, data: *const u8, datalen: c_int, _newrr: *const u8, buf: *mut u8, buflen: c_int) -> c_int {
            // UNSAFE(slice) SAFETY: dname is a string; data holds datalen bytes, buf buflen bytes (resolv.h).
            resolv::mkquery(resolv::Which::Thread, op, unsafe { strz::strz(dname) }, class, rtype, unsafe { conv::buf(data.cast(), conv::nonneg(datalen)) }.map(|d| (!d.is_empty()).then_some(d)), unsafe { conv::buf_mut(buf.cast(), conv::nonneg(buflen)) })
        }}
    };
}

res_query_fns!("GLIBC_2.34", res_query, res_search, res_querydomain, res_send, res_mkquery);
res_query_fns!("GLIBC_2.2.5", __res_query, __res_search, __res_querydomain, __res_send, __res_mkquery);

macro_rules! res_nquery_fns {
    ($v:literal, $q:ident, $s:ident, $d:ident, $send:ident, $mk:ident) => {
        export! { $v;
        pub extern "C" fn $q(statep: *mut struct___res_state, dname: *const c_char, class: c_int, rtype: c_int, answer: *mut u8, anslen: c_int) -> c_int {
            // UNSAFE(slice) SAFETY: statep is a struct __res_state, dname a string; answer holds anslen bytes (resolv.h).
            resolv::query(resolv::Which::Caller(unsafe { conv::buf_mut(statep.cast(), STATE) }), unsafe { strz::strz(dname) }, class, rtype, unsafe { conv::buf_mut(answer.cast(), conv::nonneg(anslen)) })
        }}

        export! { $v;
        pub extern "C" fn $s(statep: *mut struct___res_state, dname: *const c_char, class: c_int, rtype: c_int, answer: *mut u8, anslen: c_int) -> c_int {
            // UNSAFE(slice) SAFETY: statep is a struct __res_state, dname a string; answer holds anslen bytes (resolv.h).
            resolv::search(resolv::Which::Caller(unsafe { conv::buf_mut(statep.cast(), STATE) }), unsafe { strz::strz(dname) }, class, rtype, unsafe { conv::buf_mut(answer.cast(), conv::nonneg(anslen)) })
        }}

        export! { $v;
        pub extern "C" fn $d(statep: *mut struct___res_state, name: *const c_char, domain: *const c_char, class: c_int, rtype: c_int, answer: *mut u8, anslen: c_int) -> c_int {
            // UNSAFE(slice) SAFETY: statep is a struct __res_state, name a string, domain NULL or a string; answer holds anslen bytes (resolv.h).
            resolv::querydomain(resolv::Which::Caller(unsafe { conv::buf_mut(statep.cast(), STATE) }), unsafe { strz::strz(name) }, unsafe { strz::strz_opt(domain) }, class, rtype, unsafe { conv::buf_mut(answer.cast(), conv::nonneg(anslen)) })
        }}

        export! { $v;
        pub extern "C" fn $send(statep: *mut struct___res_state, msg: *const u8, msglen: c_int, answer: *mut u8, anslen: c_int) -> c_int {
            // UNSAFE(slice) SAFETY: statep is a struct __res_state; msg holds msglen bytes, answer anslen bytes (resolv.h).
            resolv::send(resolv::Which::Caller(unsafe { conv::buf_mut(statep.cast(), STATE) }), unsafe { conv::buf(msg.cast(), conv::nonneg(msglen)) }, unsafe { conv::buf_mut(answer.cast(), conv::nonneg(anslen)) })
        }}

        export! { $v;
        pub extern "C" fn $mk(statep: *mut struct___res_state, op: c_int, dname: *const c_char, class: c_int, rtype: c_int, data: *const u8, datalen: c_int, _newrr: *const u8, buf: *mut u8, buflen: c_int) -> c_int {
            // UNSAFE(slice) SAFETY: statep is a struct __res_state, dname a string; data holds datalen bytes, buf buflen bytes (resolv.h).
            resolv::mkquery(resolv::Which::Caller(unsafe { conv::buf_mut(statep.cast(), STATE) }), op, unsafe { strz::strz(dname) }, class, rtype, unsafe { conv::buf(data.cast(), conv::nonneg(datalen)) }.map(|d| (!d.is_empty()).then_some(d)), unsafe { conv::buf_mut(buf.cast(), conv::nonneg(buflen)) })
        }}
    };
}

res_nquery_fns!("GLIBC_2.34", res_nquery, res_nsearch, res_nquerydomain, res_nsend, res_nmkquery);
res_nquery_fns!("GLIBC_2.2.5", __res_nquery, __res_nsearch, __res_nquerydomain, __res_nsend, __res_nmkquery);

export! { "GLIBC_2.2.5";
pub extern "C" fn __res_isourserver(sin: Option<&struct_sockaddr_in>) -> c_int {
    resolv::is_our_server(sin)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __res_nameinquery(name: *const c_char, rtype: c_int, class: c_int, buf: *const u8, eom: *const u8) -> c_int {
    // UNSAFE(slice) SAFETY: name is a string (resolv.h).
    resolv::nameinquery(unsafe { strz::strz(name) }, rtype, class, conv::addr(buf), conv::addr(eom))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __res_queriesmatch(buf1: *const u8, eom1: *const u8, buf2: *const u8, eom2: *const u8) -> c_int {
    resolv::queriesmatch(conv::addr(buf1), conv::addr(eom1), conv::addr(buf2), conv::addr(eom2))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __hostalias(name: *const c_char) -> *const c_char {
    // UNSAFE(slice) SAFETY: name is a string (resolv.h).
    core::ptr::with_exposed_provenance(resolv::hostalias(None, unsafe { strz::strz(name) }, None))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __res_hostalias(statep: *mut struct___res_state, name: *const c_char, buf: *mut c_char, buflen: size_t) -> *const c_char {
    // UNSAFE(slice) SAFETY: statep is a struct __res_state, name a string; buf holds buflen bytes (resolv.h).
    core::ptr::with_exposed_provenance(resolv::hostalias(resolv::state_options(unsafe { conv::buf_mut(statep.cast(), STATE) }), unsafe { strz::strz(name) }, unsafe { conv::buf_mut(buf.cast(), buflen) }.ok()))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn res_send_setqhook(_hook: *mut c_void) {
    // Obsolete (compat): the hook is never called.
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn res_send_setrhook(_hook: *mut c_void) {
    // Obsolete (compat): the hook is never called.
}}

// ------------------------------------------------------------------------------ §16.4: names

macro_rules! name_checks {
    ($v:literal, $h:ident, $o:ident, $m:ident, $d:ident) => {
        export! { $v;
        pub extern "C" fn $h(dn: *const c_char) -> c_int {
            // UNSAFE(slice) SAFETY: dn is a string (resolv.h).
            resolv::check(unsafe { strz::strz(dn) }, Check::Host)
        }}

        export! { $v;
        pub extern "C" fn $o(dn: *const c_char) -> c_int {
            // UNSAFE(slice) SAFETY: dn is a string (resolv.h).
            resolv::check(unsafe { strz::strz(dn) }, Check::Owner)
        }}

        export! { $v;
        pub extern "C" fn $m(dn: *const c_char) -> c_int {
            // UNSAFE(slice) SAFETY: dn is a string (resolv.h).
            resolv::check(unsafe { strz::strz(dn) }, Check::Mail)
        }}

        export! { $v;
        pub extern "C" fn $d(dn: *const c_char) -> c_int {
            // UNSAFE(slice) SAFETY: dn is a string (resolv.h).
            resolv::check(unsafe { strz::strz(dn) }, Check::Domain)
        }}
    };
}

name_checks!("GLIBC_2.34", res_hnok, res_ownok, res_mailok, res_dnok);
name_checks!("GLIBC_2.2.5", __res_hnok, __res_ownok, __res_mailok, __res_dnok);

macro_rules! dn_fns {
    ($v:literal, $comp:ident, $expand:ident, $skip:ident) => {
        export! { $v;
        pub extern "C" fn $comp(exp_dn: *const c_char, comp_dn: *mut u8, length: c_int, dnptrs: *mut *mut u8, lastdnptr: *mut *mut u8) -> c_int {
            // UNSAFE(slice) SAFETY: exp_dn is a string (resolv.h).
            resolv::dn_comp(unsafe { strz::strz(exp_dn) }, conv::addr(comp_dn), length, conv::addr(dnptrs), conv::addr(lastdnptr))
        }}

        export! { $v;
        pub extern "C" fn $expand(msg: *const u8, eom: *const u8, src: *const u8, dst: *mut c_char, length: c_int) -> c_int {
            // UNSAFE(slice) SAFETY: dst holds length bytes (resolv.h).
            resolv::dn_expand(conv::addr(msg), conv::addr(eom), conv::addr(src), unsafe { conv::buf_mut(dst.cast(), conv::nonneg(length)) })
        }}

        export! { $v;
        pub extern "C" fn $skip(ptr: *const u8, eom: *const u8) -> c_int {
            resolv::dn_skipname(conv::addr(ptr), conv::addr(eom))
        }}
    };
}

dn_fns!("GLIBC_2.34", dn_comp, dn_expand, dn_skipname);
dn_fns!("GLIBC_2.2.5", __dn_comp, __dn_expand, __dn_skipname);

export! { "GLIBC_2.2.5";
pub extern "C" fn __dn_count_labels(name: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: name is a string (resolv.h).
    resolv::count_labels(unsafe { strz::strz(name) })
}}

export! { "GLIBC_2.34";
pub extern "C" fn ns_name_ntop(src: *const u8, dst: *mut c_char, dstsiz: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: dst holds dstsiz bytes (arpa/nameser.h).
    resolv::name_ntop(conv::addr(src), unsafe { conv::buf_mut(dst.cast(), dstsiz) })
}}

export! { "GLIBC_2.34";
pub extern "C" fn ns_name_pton(src: *const c_char, dst: *mut u8, dstsiz: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: src is a string; dst holds dstsiz bytes (arpa/nameser.h).
    resolv::name_pton(unsafe { strz::strz(src) }, unsafe { conv::buf_mut(dst.cast(), dstsiz) })
}}

export! { "GLIBC_2.34";
pub extern "C" fn ns_name_unpack(msg: *const u8, eom: *const u8, src: *const u8, dst: *mut u8, dstsiz: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: dst holds dstsiz bytes (arpa/nameser.h).
    resolv::name_unpack(conv::addr(msg), conv::addr(eom), conv::addr(src), unsafe { conv::buf_mut(dst.cast(), dstsiz) })
}}

export! { "GLIBC_2.34";
pub extern "C" fn ns_name_pack(src: *const u8, dst: *mut u8, dstsiz: c_int, dnptrs: *mut *const u8, lastdnptr: *mut *const u8) -> c_int {
    resolv::name_pack_at(conv::addr(src), conv::addr(dst), conv::nonneg(dstsiz), conv::addr(dnptrs), conv::addr(lastdnptr))
}}

export! { "GLIBC_2.34";
pub extern "C" fn ns_name_uncompress(msg: *const u8, eom: *const u8, src: *const u8, dst: *mut c_char, dstsiz: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: dst holds dstsiz bytes (arpa/nameser.h).
    resolv::name_uncompress(conv::addr(msg), conv::addr(eom), conv::addr(src), unsafe { conv::buf_mut(dst.cast(), dstsiz) })
}}

export! { "GLIBC_2.34";
pub extern "C" fn ns_name_compress(src: *const c_char, dst: *mut u8, dstsiz: size_t, dnptrs: *mut *const u8, lastdnptr: *mut *const u8) -> c_int {
    // UNSAFE(slice) SAFETY: src is a string (arpa/nameser.h).
    resolv::name_compress(unsafe { strz::strz(src) }, conv::addr(dst), dstsiz, conv::addr(dnptrs), conv::addr(lastdnptr))
}}

export! { "GLIBC_2.34";
pub extern "C" fn ns_name_skip(ptrptr: Option<&mut usize>, eom: *const u8) -> c_int {
    resolv::name_skip(ptrptr, conv::addr(eom))
}}

export! { "GLIBC_2.9";
pub extern "C" fn ns_name_ntol(src: *const u8, dst: *mut u8, dstsiz: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: dst holds dstsiz bytes (arpa/nameser.h).
    resolv::name_ntol(conv::addr(src), unsafe { conv::buf_mut(dst.cast(), dstsiz) })
}}

export! { "GLIBC_2.9";
pub extern "C" fn ns_name_rollback(src: *const u8, dnptrs: *mut *const u8, lastdnptr: *mut *const u8) {
    resolv::name_rollback(conv::addr(src), conv::addr(dnptrs), conv::addr(lastdnptr))
}}

export! { "GLIBC_2.9";
pub extern "C" fn ns_samename(a: *const c_char, b: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are strings (arpa/nameser.h).
    resolv::samename(unsafe { strz::strz(a) }, unsafe { strz::strz(b) })
}}

export! { "GLIBC_2.9";
pub extern "C" fn ns_samedomain(a: *const c_char, b: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are strings (arpa/nameser.h).
    resolv::samedomain(unsafe { strz::strz(a) }, unsafe { strz::strz(b) })
}}

export! { "GLIBC_2.9";
pub extern "C" fn ns_subdomain(a: *const c_char, b: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: a and b are strings (arpa/nameser.h).
    resolv::subdomain(unsafe { strz::strz(a) }, unsafe { strz::strz(b) })
}}

export! { "GLIBC_2.9";
pub extern "C" fn ns_makecanon(src: *const c_char, dst: *mut c_char, dstsize: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: src is a string; dst holds dstsize bytes (arpa/nameser.h).
    resolv::makecanon(unsafe { strz::strz(src) }, unsafe { conv::buf_mut(dst.cast(), dstsize) })
}}

// ------------------------------------------------------------------------------ §16.4: messages

export! { "GLIBC_2.9";
pub extern "C" fn ns_initparse(msg: *const u8, msglen: c_int, handle: Option<&mut ns_msg>) -> c_int {
    // UNSAFE(slice) SAFETY: msg holds msglen bytes (arpa/nameser.h).
    resolv::initparse(unsafe { conv::buf(msg.cast(), conv::nonneg(msglen)) }, handle)
}}

export! { "GLIBC_2.9";
pub extern "C" fn ns_skiprr(ptr: *const u8, eom: *const u8, section: c_int, count: c_int) -> c_int {
    resolv::skiprr(conv::addr(ptr), conv::addr(eom), section, count)
}}

export! { "GLIBC_2.9";
pub extern "C" fn ns_parserr(handle: Option<&mut ns_msg>, section: c_int, rrnum: c_int, rr: Option<&mut ns_rr>) -> c_int {
    resolv::parserr(handle, section, rrnum, rr)
}}

export! { "GLIBC_2.9";
pub extern "C" fn ns_msg_getflag(handle: ns_msg, flag: c_int) -> c_int {
    resolv::msg_getflag(handle, flag)
}}

export! { "GLIBC_2.9";
pub extern "C" fn ns_sprintrr(handle: Option<&ns_msg>, rr: Option<&ns_rr>, name_ctx: *const c_char, origin: *const c_char, buf: *mut c_char, buflen: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: name_ctx and origin are NULL or strings; buf holds buflen bytes (arpa/nameser.h).
    resolv::sprintrr(handle, rr, unsafe { strz::strz_opt(name_ctx) }, unsafe { strz::strz_opt(origin) }, unsafe { conv::buf_mut(buf.cast(), buflen) })
}}

export! { "GLIBC_2.9";
pub extern "C" fn ns_sprintrrf(msg: *const u8, msglen: size_t, name: *const c_char, class: c_int, rtype: c_int, ttl: c_ulong, rdata: *const u8, rdlen: size_t, name_ctx: *const c_char, origin: *const c_char, buf: *mut c_char, buflen: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: msg holds msglen bytes; name is a string, name_ctx and origin NULL or strings; buf holds buflen bytes (arpa/nameser.h).
    resolv::sprintrrf(unsafe { conv::buf(msg.cast(), msglen) }, unsafe { strz::strz(name) }, class, rtype, ttl, conv::addr(rdata), rdlen, unsafe { strz::strz_opt(name_ctx) }, unsafe { strz::strz_opt(origin) }, unsafe { conv::buf_mut(buf.cast(), buflen) })
}}

export! { "GLIBC_2.9";
pub extern "C" fn ns_get16(src: *const u8) -> c_uint {
    u32::try_from(resolv::get_be(conv::addr(src), 2)).unwrap_or(0)
}}

export! { "GLIBC_2.9";
pub extern "C" fn ns_get32(src: *const u8) -> c_ulong {
    resolv::get_be(conv::addr(src), 4)
}}

export! { "GLIBC_2.9";
pub extern "C" fn ns_put16(v: c_uint, dst: *mut u8) {
    resolv::put_be(u64::from(v), conv::addr(dst), 2)
}}

export! { "GLIBC_2.9";
pub extern "C" fn ns_put32(v: c_ulong, dst: *mut u8) {
    resolv::put_be(v, conv::addr(dst), 4)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _getlong(src: *const u8) -> c_ulong {
    resolv::get_be(conv::addr(src), 4)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _getshort(src: *const u8) -> c_ulong {
    resolv::get_be(conv::addr(src), 2)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __putlong(v: u32, dst: *mut u8) {
    resolv::put_be(u64::from(v), conv::addr(dst), 4)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __putshort(v: u16, dst: *mut u8) {
    resolv::put_be(u64::from(v), conv::addr(dst), 2)
}}

// ------------------------------------------------------------------------------ §16.4: texts

export! { "GLIBC_2.9";
pub extern "C" fn ns_format_ttl(src: c_ulong, dst: *mut c_char, dstlen: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: dst holds dstlen bytes (arpa/nameser.h).
    resolv::format_ttl(src, unsafe { conv::buf_mut(dst.cast(), dstlen) })
}}

export! { "GLIBC_2.9";
pub extern "C" fn ns_parse_ttl(src: *const c_char, dst: Option<&mut c_ulong>) -> c_int {
    // UNSAFE(slice) SAFETY: src is a string (arpa/nameser.h).
    resolv::parse_ttl(unsafe { strz::strz(src) }, dst)
}}

export! { "GLIBC_2.9";
pub extern "C" fn ns_datetosecs(cp: *const c_char, errp: Option<&mut c_int>) -> u32 {
    // UNSAFE(slice) SAFETY: cp is a string (arpa/nameser.h).
    resolv::datetosecs(unsafe { strz::strz(cp) }, errp)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __b64_ntop(src: *const u8, srclength: size_t, target: *mut c_char, targsize: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: src holds srclength bytes, target targsize bytes (resolv.h).
    resolv::b64_ntop(unsafe { conv::buf(src.cast(), srclength) }, unsafe { conv::buf_mut(target.cast(), targsize) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __b64_pton(src: *const c_char, target: *mut u8, targsize: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: src is a string; target is NULL or holds targsize bytes (resolv.h).
    resolv::b64_pton(unsafe { strz::strz(src) }, unsafe { conv::opt_buf_mut(target.cast(), targsize) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __loc_aton(ascii: *const c_char, binary: *mut u8) -> c_int {
    // UNSAFE(slice) SAFETY: ascii is a string; binary holds the 16 bytes of a LOC record (resolv.h).
    resolv::loc_aton(unsafe { strz::strz(ascii) }, unsafe { conv::buf_mut(binary.cast(), 16) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __loc_ntoa(binary: *const u8, ascii: *mut c_char) -> *const c_char {
    // UNSAFE(slice) SAFETY: binary holds the 16 bytes of a LOC record (resolv.h).
    core::ptr::with_exposed_provenance(resolv::loc_ntoa(unsafe { conv::buf(binary.cast(), 16) }, conv::addr(ascii)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __p_type(t: c_int) -> *const c_char {
    core::ptr::with_exposed_provenance(resolv::p_type(t))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __p_class(c: c_int) -> *const c_char {
    core::ptr::with_exposed_provenance(resolv::p_class(c))
}}

export! { "GLIBC_2.3.2";
pub extern "C" fn __p_rcode(r: c_int) -> *const c_char {
    core::ptr::with_exposed_provenance(resolv::p_rcode(r))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __p_option(option: c_ulong) -> *const c_char {
    core::ptr::with_exposed_provenance(resolv::p_option(option))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __p_time(value: u32) -> *const c_char {
    core::ptr::with_exposed_provenance(resolv::p_time(value))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __p_secstodate(secs: c_ulong) -> *const c_char {
    core::ptr::with_exposed_provenance(resolv::p_secstodate(secs))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __sym_ston(syms: *const c_void, name: *const c_char, success: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: name is a string (resolv.h).
    resolv::sym_ston(conv::addr(syms), unsafe { strz::strz(name) }, success)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __sym_ntos(syms: *const c_void, number: c_int, success: Option<&mut c_int>) -> *const c_char {
    core::ptr::with_exposed_provenance(resolv::sym_ntos(conv::addr(syms), number, success, false))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __sym_ntop(syms: *const c_void, number: c_int, success: Option<&mut c_int>) -> *const c_char {
    core::ptr::with_exposed_provenance(resolv::sym_ntos(conv::addr(syms), number, success, true))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __fp_nquery(msg: *const u8, len: c_int, file: *mut c_void) {
    resolv::fp_nquery(conv::addr(msg), len, conv::addr(file))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __fp_query(msg: *const u8, file: *mut c_void) {
    resolv::fp_query(conv::addr(msg), conv::addr(file))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __p_query(msg: *const u8) {
    resolv::p_query(conv::addr(msg))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __fp_resstat(statep: *const struct___res_state, file: *mut c_void) {
    // UNSAFE(slice) SAFETY: statep is a struct __res_state (resolv.h).
    resolv::fp_resstat(unsafe { conv::buf_mut(statep.cast_mut().cast(), STATE) }, conv::addr(file))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __p_cdnname(cp: *const u8, msg: *const u8, len: c_int, file: *mut c_void) -> *const u8 {
    core::ptr::with_exposed_provenance(resolv::p_cdnname(conv::addr(cp), conv::addr(msg), len, conv::addr(file), false))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __p_cdname(cp: *const u8, msg: *const u8, file: *mut c_void) -> *const u8 {
    core::ptr::with_exposed_provenance(resolv::p_cdnname(conv::addr(cp), conv::addr(msg), 512, conv::addr(file), false))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __p_fqname(cp: *const u8, msg: *const u8, file: *mut c_void) -> *const u8 {
    core::ptr::with_exposed_provenance(resolv::p_cdnname(conv::addr(cp), conv::addr(msg), 512, conv::addr(file), true))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __p_fqnname(cp: *const u8, msg: *const u8, msglen: c_int, name: *mut c_char, namelen: c_int) -> *const u8 {
    // UNSAFE(slice) SAFETY: name holds namelen bytes (resolv.h).
    core::ptr::with_exposed_provenance(resolv::p_fqnname(conv::addr(cp), conv::addr(msg), msglen, unsafe { conv::buf_mut(name.cast(), conv::nonneg(namelen)) }))
}}
