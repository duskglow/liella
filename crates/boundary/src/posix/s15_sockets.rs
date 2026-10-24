//! Translation for spec §15: sockets (phase 5: §15.1 lifecycle and §15.2 data transfer), and
//! (phase 10) address conversion, interfaces, IPv6 helpers, source filters, Ethernet names and
//! the `rcmd` family (§15.3–15.8), whose C conventions are in `crate::netaddr`.
//!
//! Addresses in are byte slices of the caller's `socklen_t`. Address and option results go
//! through `liella_legacy::net_compat`, which copies them back truncated and reports the full
//! length. `struct msghdr` and `struct mmsghdr` have the kernel's layout and are viewed in
//! place (`conv::msghdr`), so the kernel reads and writes the buffers they name, as for a C
//! caller. `setsockopt` at a level that registers caller memory, and `SIOCATMARK`, go through
//! `crate::raw`.

use liella_abi::ctypes::{c_char, c_int, c_uint, c_ushort, c_void, size_t, ssize_t};
use liella_kernel::mem::{MMsgHdr, MMsgHdrMut};
use liella_kernel::sys::Sys;
use liella_kernel::uapi::Timespec;
use liella_legacy::net_compat;

use crate::cancel;
use crate::conv;
use crate::export;
use crate::netaddr;
use crate::raw;
use crate::ret;
use crate::strz;
use crate::sys::{CSys, CP};

/// A `socklen_t` or count as a length (beyond the address space: an impossible length, which
/// the conversions refuse).
fn n32(v: u32) -> usize {
    usize::try_from(v).unwrap_or(usize::MAX)
}

export! { "GLIBC_2.2.5";
pub extern "C" fn socket(domain: c_int, kind: c_int, protocol: c_int) -> c_int {
    ret::fd(CSys.socket(conv::bits(domain), conv::bits(kind), conv::bits(protocol)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn socketpair(domain: c_int, kind: c_int, protocol: c_int, sv: Option<&mut [c_int; 2]>) -> c_int {
    ret::pair(sv, CSys.socketpair(conv::bits(domain), conv::bits(kind), conv::bits(protocol)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn bind(fd: c_int, addr: *const c_void, len: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: addr holds len readable bytes (§15.1).
    ret::zero(unsafe { conv::sockaddr(addr, len) }.and_then(|a| CSys.bind(conv::fd(fd), a)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn listen(fd: c_int, backlog: c_int) -> c_int {
    ret::zero(CSys.listen(conv::fd(fd), backlog))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn accept(fd: c_int, addr: *mut c_void, len: Option<&mut c_uint>) -> c_int {
    // UNSAFE(slice) SAFETY: addr is NULL or holds *len writable bytes (§15.1).
    cancel::point(ret::fd(unsafe { conv::addr_out(addr, len) }.and_then(|o| net_compat::accept(&CP, conv::fd(fd), o, 0))))
}}

export! { "GLIBC_2.10";
pub extern "C" fn accept4(fd: c_int, addr: *mut c_void, len: Option<&mut c_uint>, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: addr is NULL or holds *len writable bytes (§15.1).
    cancel::point(ret::fd(unsafe { conv::addr_out(addr, len) }.and_then(|o| net_compat::accept(&CP, conv::fd(fd), o, conv::bits(flags)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn connect(fd: c_int, addr: *const c_void, len: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: addr holds len readable bytes (§15.1).
    cancel::point(ret::zero(unsafe { conv::sockaddr(addr, len) }.and_then(|a| CP.connect(conv::fd(fd), a))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __connect(fd: c_int, addr: *const c_void, len: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: addr holds len readable bytes (§15.1).
    ret::zero(unsafe { conv::sockaddr(addr, len) }.and_then(|a| CSys.connect(conv::fd(fd), a)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn shutdown(fd: c_int, how: c_int) -> c_int {
    ret::zero(CSys.shutdown(conv::fd(fd), conv::bits(how)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getsockname(fd: c_int, addr: *mut c_void, len: Option<&mut c_uint>) -> c_int {
    // UNSAFE(slice) SAFETY: addr is NULL or holds *len writable bytes (§15.1).
    ret::zero(unsafe { conv::addr_out(addr, len) }.and_then(conv::req).and_then(|o| net_compat::sockname(&CSys, conv::fd(fd), o, false)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getpeername(fd: c_int, addr: *mut c_void, len: Option<&mut c_uint>) -> c_int {
    // UNSAFE(slice) SAFETY: addr is NULL or holds *len writable bytes (§15.1).
    ret::zero(unsafe { conv::addr_out(addr, len) }.and_then(conv::req).and_then(|o| net_compat::sockname(&CSys, conv::fd(fd), o, true)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn getsockopt(fd: c_int, level: c_int, name: c_int, value: *mut c_void, len: Option<&mut c_uint>) -> c_int {
    // UNSAFE(slice) SAFETY: value holds *len writable bytes (§15.1).
    ret::zero(unsafe { conv::addr_out(value, len) }.and_then(conv::req).and_then(|o| net_compat::getsockopt(&CSys, conv::fd(fd), level, name, o)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn setsockopt(fd: c_int, level: c_int, name: c_int, value: *const c_void, len: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: value holds len readable bytes (§15.1).
    ret::zero(unsafe { conv::sockaddr(value, len) }.and_then(|v| net_compat::setsockopt(&CSys, conv::fd(fd), level, name, v, || raw::setsockopt(fd, level, name, conv::addr(value), len))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sockatmark(fd: c_int) -> c_int {
    ret::int(raw::sockatmark(fd))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn isfdtype(fd: c_int, kind: c_int) -> c_int {
    ret::int(net_compat::isfdtype(&CSys, conv::fd(fd), kind))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn send(fd: c_int, buf: *const c_void, n: size_t, flags: c_int) -> ssize_t {
    // UNSAFE(slice) SAFETY: buf holds n bytes (§15.2).
    cancel::point(ret::ssize(unsafe { conv::buf(buf, n) }.and_then(|b| CP.sendto(conv::fd(fd), b, conv::bits(flags), None))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __send(fd: c_int, buf: *const c_void, n: size_t, flags: c_int) -> ssize_t {
    // UNSAFE(slice) SAFETY: buf holds n bytes (§15.2).
    ret::ssize(unsafe { conv::buf(buf, n) }.and_then(|b| CSys.sendto(conv::fd(fd), b, conv::bits(flags), None)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn recv(fd: c_int, buf: *mut c_void, n: size_t, flags: c_int) -> ssize_t {
    // UNSAFE(slice) SAFETY: buf holds n bytes (§15.2).
    cancel::point(ret::ssize(unsafe { conv::buf_mut(buf, n) }.and_then(|b| CP.recvfrom(conv::fd(fd), b, conv::bits(flags), None))))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __recv_chk(fd: c_int, buf: *mut c_void, n: size_t, buflen: size_t, flags: c_int) -> ssize_t {
    // UNSAFE(slice) SAFETY: as recv, after the fortify check.
    cancel::point(ret::ssize(unsafe { conv::buf_mut(buf, conv::chk(n, buflen)) }.and_then(|b| CP.recvfrom(conv::fd(fd), b, conv::bits(flags), None))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sendto(fd: c_int, buf: *const c_void, n: size_t, flags: c_int, addr: *const c_void, len: c_uint) -> ssize_t {
    // UNSAFE(slice) SAFETY: buf holds n bytes; addr is NULL or holds len bytes (§15.2).
    cancel::point(ret::ssize(unsafe { conv::buf(buf, n) }.and_then(|b| unsafe { conv::opt_sockaddr(addr, len) }.and_then(|a| CP.sendto(conv::fd(fd), b, conv::bits(flags), a)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn recvfrom(fd: c_int, buf: *mut c_void, n: size_t, flags: c_int, addr: *mut c_void, len: Option<&mut c_uint>) -> ssize_t {
    // UNSAFE(slice) SAFETY: buf holds n bytes; addr is NULL or holds *len writable bytes (§15.2).
    cancel::point(ret::ssize(unsafe { conv::buf_mut(buf, n) }.and_then(|b| unsafe { conv::addr_out(addr, len) }.and_then(|o| net_compat::recvfrom(&CP, conv::fd(fd), b, conv::bits(flags), o)))))
}}

export! { "GLIBC_2.4";
pub extern "C" fn __recvfrom_chk(fd: c_int, buf: *mut c_void, n: size_t, buflen: size_t, flags: c_int, addr: *mut c_void, len: Option<&mut c_uint>) -> ssize_t {
    // UNSAFE(slice) SAFETY: as recvfrom, after the fortify check.
    cancel::point(ret::ssize(unsafe { conv::buf_mut(buf, conv::chk(n, buflen)) }.and_then(|b| unsafe { conv::addr_out(addr, len) }.and_then(|o| net_compat::recvfrom(&CP, conv::fd(fd), b, conv::bits(flags), o)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sendmsg(fd: c_int, msg: *const c_void, flags: c_int) -> ssize_t {
    // UNSAFE(slice) SAFETY: msg is a struct msghdr naming readable buffers (§15.2).
    cancel::point(ret::ssize(unsafe { conv::msghdr(msg) }.and_then(|h| CP.sendmsg(conv::fd(fd), h, conv::bits(flags)))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn recvmsg(fd: c_int, msg: *mut c_void, flags: c_int) -> ssize_t {
    // UNSAFE(slice) SAFETY: msg is a writable struct msghdr naming writable buffers (§15.2).
    cancel::point(ret::ssize(unsafe { conv::msghdr_mut(msg) }.and_then(|h| CP.recvmsg(conv::fd(fd), h, conv::bits(flags)))))
}}

export! { "GLIBC_2.14";
pub extern "C" fn sendmmsg(fd: c_int, vec: *mut c_void, n: c_uint, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: vec holds n struct mmsghdr, each naming readable buffers (§15.2).
    cancel::point(ret::count(unsafe { conv::array_mut::<MMsgHdr<'_>>(vec, i64::from(n)) }.and_then(|v| CP.sendmmsg(conv::fd(fd), v, conv::bits(flags)))))
}}

export! { "GLIBC_2.12";
pub extern "C" fn recvmmsg(fd: c_int, vec: *mut c_void, n: c_uint, flags: c_int, timeout: Option<&mut Timespec>) -> c_int {
    // UNSAFE(slice) SAFETY: vec holds n struct mmsghdr, each naming writable buffers (§15.2).
    cancel::point(ret::count(unsafe { conv::array_mut::<MMsgHdrMut<'_>>(vec, i64::from(n)) }.and_then(|v| CP.recvmmsg(conv::fd(fd), v, conv::bits(flags), timeout))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __cmsg_nxthdr(mhdr: *mut c_void, cmsg: *mut c_void) -> *mut c_void {
    // UNSAFE(slice) SAFETY: mhdr is a struct msghdr with a readable control buffer, and cmsg a header in it (§15.2).
    ret::ptr(unsafe { conv::cmsg_area(mhdr, cmsg) }.map_or(0, |(area, off)| net_compat::cmsg_next(area, off)))
}}

// ------------------------------------------------------------------------------ §15.3

export! { "GLIBC_2.2.5";
pub extern "C" fn inet_pton(af: c_int, src: *const c_char, dst: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: src is a string; dst holds the family's address size (§15.3).
    ret::int(netaddr::pton(af, unsafe { strz::strz(src) }, |n| unsafe { conv::buf_mut(dst, n) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn inet_ntop(af: c_int, src: *const c_void, dst: *mut c_char, size: c_uint) -> *const c_char {
    // UNSAFE(slice) SAFETY: src holds the family's address; dst holds size bytes (§15.3).
    ret::null(netaddr::ntop(af, |n| unsafe { conv::buf(src, n) }, unsafe { conv::buf_mut(dst.cast(), n32(size)) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn inet_aton(cp: *const c_char, inp: Option<&mut u32>) -> c_int {
    // UNSAFE(slice) SAFETY: cp is a string (§15.3).
    ret::int_must(netaddr::aton(unsafe { strz::strz(cp) }, inp))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn inet_addr(cp: *const c_char) -> u32 {
    // UNSAFE(slice) SAFETY: cp is a string (§15.3).
    ret::must(netaddr::inet_addr(unsafe { strz::strz(cp) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn inet_network(cp: *const c_char) -> u32 {
    // UNSAFE(slice) SAFETY: cp is a string (§15.3).
    ret::must(netaddr::network(unsafe { strz::strz(cp) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn inet_ntoa(a: u32) -> *mut c_char {
    ret::null(netaddr::ntoa(a))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn inet_makeaddr(net: u32, host: u32) -> u32 {
    netaddr::makeaddr(net, host)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn inet_lnaof(a: u32) -> u32 {
    netaddr::lnaof(a)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn inet_netof(a: u32) -> u32 {
    netaddr::netof(a)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn inet_net_pton(af: c_int, cp: *const c_char, buf: *mut c_void, len: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: cp is a string; buf holds len bytes (§15.3).
    ret::int(netaddr::net_pton(af, unsafe { strz::strz(cp) }, unsafe { conv::buf_mut(buf, len) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn inet_net_ntop(af: c_int, cp: *const c_void, bits: c_int, buf: *mut c_char, len: size_t) -> *mut c_char {
    // UNSAFE(slice) SAFETY: cp holds the bytes the prefix covers; buf holds len bytes (§15.3).
    ret::null(netaddr::net_ntop(af, |n| unsafe { conv::buf(cp, n) }, bits, unsafe { conv::buf_mut(buf.cast(), len) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn inet_neta(net: u32, buf: *mut c_char, len: size_t) -> *mut c_char {
    // UNSAFE(slice) SAFETY: buf holds len bytes (§15.3).
    ret::null(netaddr::neta(net, unsafe { conv::buf_mut(buf.cast(), len) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn inet_nsap_addr(cp: *const c_char, buf: *mut u8, len: c_int) -> c_uint {
    // UNSAFE(slice) SAFETY: cp is a string; buf holds len bytes (§15.3).
    netaddr::nsap_addr(unsafe { strz::strz(cp) }, unsafe { conv::buf_mut(buf.cast(), conv::nonneg(len)) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn inet_nsap_ntoa(len: c_int, cp: *const u8, buf: *mut c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: cp holds len bytes; buf is NULL or holds the text's size (§15.3).
    ret::null(netaddr::nsap_ntoa(len, |n| unsafe { conv::buf(cp.cast(), n) }, (!buf.is_null()).then_some(|n| unsafe { conv::buf_mut(buf.cast(), n) })))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn htonl(v: u32) -> u32 {
    v.to_be()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ntohl(v: u32) -> u32 {
    u32::from_be(v)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn htons(v: u16) -> u16 {
    v.to_be()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ntohs(v: u16) -> u16 {
    u16::from_be(v)
}}

// ------------------------------------------------------------------------------ §15.4

export! { "GLIBC_2.2.5";
pub extern "C" fn if_nametoindex(name: *const c_char) -> c_uint {
    // UNSAFE(slice) SAFETY: name is a string (§15.4).
    ret::with_errno(netaddr::nametoindex(unsafe { strz::strz(name) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn if_indextoname(index: c_uint, buf: Option<&mut [u8; 16]>) -> *mut c_char {
    ret::null(netaddr::indextoname(index, buf))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn if_nameindex() -> *mut c_void {
    ret::null(netaddr::nameindex())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn if_freenameindex(p: *mut c_void) {
    netaddr::release(conv::addr(p))
}}

export! { "GLIBC_2.3";
pub extern "C" fn getifaddrs(ifap: Option<&mut usize>) -> c_int {
    ret::zero(netaddr::getifaddrs(ifap))
}}

export! { "GLIBC_2.3";
pub extern "C" fn freeifaddrs(p: *mut c_void) {
    netaddr::release(conv::addr(p))
}}

// ------------------------------------------------------------------------------ §15.5

export! { "GLIBC_2.5";
pub extern "C" fn inet6_opt_init(extbuf: *mut c_void, extlen: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: extbuf is NULL or holds extlen bytes (RFC 3542).
    netaddr::opt_init(unsafe { conv::opt_buf_mut(extbuf.cast(), n32(extlen)) })
}}

export! { "GLIBC_2.5";
pub extern "C" fn inet6_opt_append(extbuf: *mut c_void, extlen: c_uint, offset: c_int, ty: u8, len: c_uint, align: u8, databufp: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: extbuf is NULL or holds extlen bytes (RFC 3542).
    netaddr::opt_append(unsafe { conv::opt_buf_mut(extbuf.cast(), n32(extlen)) }, offset, ty, len, align, databufp)
}}

export! { "GLIBC_2.5";
pub extern "C" fn inet6_opt_finish(extbuf: *mut c_void, extlen: c_uint, offset: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: extbuf is NULL or holds extlen bytes (RFC 3542).
    netaddr::opt_finish(unsafe { conv::opt_buf_mut(extbuf.cast(), n32(extlen)) }, offset)
}}

export! { "GLIBC_2.5";
pub extern "C" fn inet6_opt_set_val(databuf: *mut c_void, offset: c_int, val: *const c_void, vallen: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: databuf holds offset + vallen bytes, val vallen bytes, distinct (RFC 3542).
    netaddr::opt_val(offset, vallen, |o, n| unsafe { conv::buf_mut(databuf, o.saturating_add(n)) }.and_then(|d| unsafe { conv::buf(val, n) }.map(|v| conv::copy_units(d.get_mut(o..).unwrap_or(&mut []), v)).map(drop)))
}}

export! { "GLIBC_2.5";
pub extern "C" fn inet6_opt_get_val(databuf: *mut c_void, offset: c_int, val: *mut c_void, vallen: c_uint) -> c_int {
    // UNSAFE(slice) SAFETY: databuf holds offset + vallen bytes, val vallen writable bytes, distinct (RFC 3542).
    netaddr::opt_val(offset, vallen, |o, n| unsafe { conv::buf(databuf.cast_const(), o.saturating_add(n)) }.and_then(|d| unsafe { conv::buf_mut(val, n) }.map(|v| conv::copy_units(v, d.get(o..).unwrap_or(&[]))).map(drop)))
}}

export! { "GLIBC_2.5";
pub extern "C" fn inet6_opt_next(extbuf: *mut c_void, extlen: c_uint, offset: c_int, typep: Option<&mut u8>, lenp: Option<&mut c_uint>, databufp: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: extbuf holds extlen bytes (RFC 3542).
    netaddr::opt_walk(unsafe { conv::buf(extbuf.cast_const(), n32(extlen)) }, |b| liella::net::ip6opt::opt_next(b, offset), typep, lenp, databufp)
}}

export! { "GLIBC_2.5";
pub extern "C" fn inet6_opt_find(extbuf: *mut c_void, extlen: c_uint, offset: c_int, ty: u8, lenp: Option<&mut c_uint>, databufp: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: extbuf holds extlen bytes (RFC 3542).
    netaddr::opt_walk(unsafe { conv::buf(extbuf.cast_const(), n32(extlen)) }, |b| liella::net::ip6opt::opt_find(b, offset, ty), None, lenp, databufp)
}}

export! { "GLIBC_2.5";
pub extern "C" fn inet6_rth_space(ty: c_int, segments: c_int) -> c_uint {
    u32::try_from(liella::net::ip6opt::rth_space(ty, segments)).unwrap_or(0)
}}

export! { "GLIBC_2.5";
pub extern "C" fn inet6_rth_init(bp: *mut c_void, bp_len: c_uint, ty: c_int, segments: c_int) -> *mut c_void {
    // UNSAFE(slice) SAFETY: bp holds bp_len bytes (RFC 3542).
    ret::addr(netaddr::rth_init(unsafe { conv::buf_mut(bp, n32(bp_len)) }, ty, segments))
}}

export! { "GLIBC_2.5";
pub extern "C" fn inet6_rth_add(bp: *mut c_void, addr: Option<&[u8; 16]>) -> c_int {
    // UNSAFE(slice) SAFETY: bp is a routing header inet6_rth_init made: its fixed part, then the space its length declares (RFC 3542).
    netaddr::rth_add(netaddr::rth_len(netaddr::head8(unsafe { conv::buf(bp.cast_const(), 8) })).and_then(|n| unsafe { conv::buf_mut(bp, n) }), addr)
}}

export! { "GLIBC_2.5";
pub extern "C" fn inet6_rth_reverse(input: *const c_void, out: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: input is a routing header; out has room for one of its size and may be the same header (RFC 3542).
    netaddr::rth_reverse(netaddr::rth_len(netaddr::head8(unsafe { conv::buf(input, 8) })), |n| unsafe { conv::buf(input, n) }, |n| unsafe { conv::buf_mut(out, n) })
}}

export! { "GLIBC_2.5";
pub extern "C" fn inet6_rth_segments(bp: *const c_void) -> c_int {
    // UNSAFE(slice) SAFETY: bp is a routing header (its fixed 8 bytes are read; RFC 3542).
    netaddr::rth_segments(netaddr::head8(unsafe { conv::buf(bp, 8) }))
}}

export! { "GLIBC_2.5";
pub extern "C" fn inet6_rth_getaddr(bp: *const c_void, index: c_int) -> *mut c_void {
    // UNSAFE(slice) SAFETY: bp is a routing header (its fixed 8 bytes are read; RFC 3542).
    ret::found(bp.cast::<u8>(), Ok(netaddr::rth_getaddr(netaddr::head8(unsafe { conv::buf(bp, 8) }), index))).cast()
}}

export! { "GLIBC_2.3.3";
pub extern "C" fn inet6_option_space(nbytes: c_int) -> c_int {
    liella_legacy::inet_compat::option_space(nbytes)
}}

export! { "GLIBC_2.3.3";
pub extern "C" fn inet6_option_init(bp: *mut c_void, cmsgp: Option<&mut usize>, ty: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: bp has room for an ancillary-data object (inet6_option_space; RFC 2292).
    netaddr::option_init(unsafe { conv::buf_mut(bp, 16) }, cmsgp, ty)
}}

export! { "GLIBC_2.3.3";
pub extern "C" fn inet6_option_append(cmsg: *mut c_void, typep: *const u8, multx: c_int, plusy: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: cmsg is an object inet6_option_init made, with room for the option; typep is an option: type, length, data (RFC 2292).
    if netaddr::option_add(|n| unsafe { conv::buf_mut(cmsg, n) }, Some(unsafe { conv::buf(typep.cast(), 2) }.and_then(|h| unsafe { conv::buf(typep.cast(), usize::from(h.get(1).copied().unwrap_or(0)).saturating_add(2)) })), 0, multx, plusy) == 0 { -1 } else { 0 }
}}

export! { "GLIBC_2.3.3";
pub extern "C" fn inet6_option_alloc(cmsg: *mut c_void, datalen: c_int, multx: c_int, plusy: c_int) -> *mut u8 {
    // UNSAFE(slice) SAFETY: cmsg is an object inet6_option_init made, with room for the option (RFC 2292).
    ret::ptr(netaddr::option_add(|n| unsafe { conv::buf_mut(cmsg, n) }, None, datalen, multx, plusy))
}}

export! { "GLIBC_2.3.3";
pub extern "C" fn inet6_option_next(cmsg: *const c_void, tptrp: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: cmsg is an ancillary-data object of cmsg_len bytes (RFC 2292).
    netaddr::option_walk(|n| unsafe { conv::buf(cmsg, n) }, tptrp, None)
}}

export! { "GLIBC_2.3.3";
pub extern "C" fn inet6_option_find(cmsg: *const c_void, tptrp: Option<&mut usize>, ty: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: cmsg is an ancillary-data object of cmsg_len bytes (RFC 2292).
    netaddr::option_walk(|n| unsafe { conv::buf(cmsg, n) }, tptrp, Some(ty))
}}

// ------------------------------------------------------------------------------ §15.6

export! { "GLIBC_2.3.4";
pub extern "C" fn getipv4sourcefilter(s: c_int, interface: u32, group: u32, fmode: Option<&mut u32>, numsrc: Option<&mut u32>, slist: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: slist holds *numsrc struct in_addr (RFC 3678).
    ret::zero(netaddr::get_ipv4_filter(s, interface, group, fmode, numsrc, |n| unsafe { conv::buf_mut(slist, n) }))
}}

export! { "GLIBC_2.3.4";
pub extern "C" fn setipv4sourcefilter(s: c_int, interface: u32, group: u32, fmode: u32, numsrc: u32, slist: *const c_void) -> c_int {
    // UNSAFE(slice) SAFETY: slist holds numsrc struct in_addr (RFC 3678).
    ret::zero(netaddr::set_ipv4_filter(s, interface, group, fmode, unsafe { conv::buf(slist, n32(numsrc).saturating_mul(4)) }))
}}

export! { "GLIBC_2.3.4";
pub extern "C" fn getsourcefilter(s: c_int, interface: u32, group: *const c_void, grouplen: c_uint, fmode: Option<&mut u32>, numsrc: Option<&mut u32>, slist: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: group holds grouplen bytes; slist holds *numsrc struct sockaddr_storage (RFC 3678).
    ret::zero(netaddr::get_filter(s, interface, unsafe { conv::sockaddr(group, grouplen) }, fmode, numsrc, |n| unsafe { conv::buf_mut(slist, n) }))
}}

export! { "GLIBC_2.3.4";
pub extern "C" fn setsourcefilter(s: c_int, interface: u32, group: *const c_void, grouplen: c_uint, fmode: u32, numsrc: u32, slist: *const c_void) -> c_int {
    // UNSAFE(slice) SAFETY: group holds grouplen bytes; slist holds numsrc struct sockaddr_storage (RFC 3678).
    ret::zero(netaddr::set_filter(s, interface, unsafe { conv::sockaddr(group, grouplen) }, fmode, unsafe { conv::buf(slist, n32(numsrc).saturating_mul(128)) }))
}}

// ------------------------------------------------------------------------------ §15.7

export! { "GLIBC_2.2.5";
pub extern "C" fn ether_aton_r(asc: *const c_char, addr: Option<&mut [u8; 6]>) -> *mut c_void {
    // UNSAFE(slice) SAFETY: asc is a string (§15.7).
    ret::null(netaddr::ether_aton_r(unsafe { strz::strz(asc) }, addr))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ether_aton(asc: *const c_char) -> *mut c_void {
    // UNSAFE(slice) SAFETY: asc is a string (§15.7).
    ret::null(netaddr::ether_aton(unsafe { strz::strz(asc) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ether_ntoa_r(addr: Option<&[u8; 6]>, buf: *mut c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: buf holds the text (18 bytes; §15.7).
    ret::null(netaddr::ether_ntoa_r(addr, unsafe { conv::buf_mut(buf.cast(), 18) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ether_ntoa(addr: Option<&[u8; 6]>) -> *mut c_char {
    ret::null(netaddr::ether_ntoa(addr))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ether_line(line: *const c_char, addr: Option<&mut [u8; 6]>, hostname: *mut c_char) -> c_int {
    // UNSAFE(slice) SAFETY: line is a string; hostname has room for the name (§15.7).
    ret::int_must(netaddr::ether_line(unsafe { strz::strz(line) }, addr, |n| unsafe { conv::buf_mut(hostname.cast(), n) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ether_hostton(hostname: *const c_char, addr: Option<&mut [u8; 6]>) -> c_int {
    // UNSAFE(slice) SAFETY: hostname is a string (§15.7).
    ret::int_must(netaddr::ether_hostton(unsafe { strz::strz(hostname) }, addr))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ether_ntohost(hostname: *mut c_char, addr: Option<&[u8; 6]>) -> c_int {
    // UNSAFE(slice) SAFETY: hostname has room for the name (§15.7).
    ret::int_must(netaddr::ether_ntohost(|n| unsafe { conv::buf_mut(hostname.cast(), n) }, addr))
}}

// ------------------------------------------------------------------------------ §15.8

export! { "GLIBC_2.2.5";
pub extern "C" fn rresvport(port: Option<&mut c_int>) -> c_int {
    ret::fd(netaddr::rresvport(port, 2))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn rresvport_af(port: Option<&mut c_int>, af: c_ushort) -> c_int {
    ret::fd(netaddr::rresvport(port, i32::from(af)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn rcmd(ahost: Option<&mut usize>, rport: c_ushort, locuser: *const c_char, remuser: *const c_char, cmd: *const c_char, fd2p: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: *ahost and the user and command arguments are strings (§15.8).
    netaddr::rcmd_af(ahost, |a| unsafe { strz::strz(core::ptr::with_exposed_provenance(a)) }, rport, unsafe { strz::strz(locuser) }, unsafe { strz::strz(remuser) }, unsafe { strz::strz(cmd) }, fd2p, 2)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn rcmd_af(ahost: Option<&mut usize>, rport: c_ushort, locuser: *const c_char, remuser: *const c_char, cmd: *const c_char, fd2p: Option<&mut c_int>, af: c_ushort) -> c_int {
    // UNSAFE(slice) SAFETY: *ahost and the user and command arguments are strings (§15.8).
    netaddr::rcmd_af(ahost, |a| unsafe { strz::strz(core::ptr::with_exposed_provenance(a)) }, rport, unsafe { strz::strz(locuser) }, unsafe { strz::strz(remuser) }, unsafe { strz::strz(cmd) }, fd2p, af)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn rexec(ahost: Option<&mut usize>, rport: c_int, name: *const c_char, pass: *const c_char, cmd: *const c_char, fd2p: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: *ahost and cmd are strings; name and pass are NULL or strings (§15.8).
    netaddr::rexec_af(ahost, |a| unsafe { strz::strz(core::ptr::with_exposed_provenance(a)) }, rport, unsafe { strz::strz_opt(name) }, unsafe { strz::strz_opt(pass) }, unsafe { strz::strz(cmd) }, fd2p, 2)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn rexec_af(ahost: Option<&mut usize>, rport: c_int, name: *const c_char, pass: *const c_char, cmd: *const c_char, fd2p: Option<&mut c_int>, af: c_ushort) -> c_int {
    // UNSAFE(slice) SAFETY: *ahost and cmd are strings; name and pass are NULL or strings (§15.8).
    netaddr::rexec_af(ahost, |a| unsafe { strz::strz(core::ptr::with_exposed_provenance(a)) }, rport, unsafe { strz::strz_opt(name) }, unsafe { strz::strz_opt(pass) }, unsafe { strz::strz(cmd) }, fd2p, af)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ruserok(rhost: *const c_char, superuser: c_int, ruser: *const c_char, luser: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: the arguments are strings (§15.8).
    netaddr::ruserok(unsafe { strz::strz(rhost) }, superuser, unsafe { strz::strz(ruser) }, unsafe { strz::strz(luser) }, 2)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ruserok_af(rhost: *const c_char, superuser: c_int, ruser: *const c_char, luser: *const c_char, af: c_ushort) -> c_int {
    // UNSAFE(slice) SAFETY: the arguments are strings (§15.8).
    netaddr::ruserok(unsafe { strz::strz(rhost) }, superuser, unsafe { strz::strz(ruser) }, unsafe { strz::strz(luser) }, af)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn iruserok(raddr: u32, superuser: c_int, ruser: *const c_char, luser: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: the user names are strings (§15.8).
    netaddr::iruserok_v4(raddr, superuser, unsafe { strz::strz(ruser) }, unsafe { strz::strz(luser) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn iruserok_af(raddr: *const c_void, superuser: c_int, ruser: *const c_char, luser: *const c_char, af: c_ushort) -> c_int {
    // UNSAFE(slice) SAFETY: raddr holds an address of family af; the user names are strings (§15.8).
    netaddr::iruserok(|n| unsafe { conv::buf(raddr, n) }, superuser, unsafe { strz::strz(ruser) }, unsafe { strz::strz(luser) }, af)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn ruserpass(host: *const c_char, aname: Option<&mut usize>, apass: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: host is a string; *aname and *apass are NULL or strings (§15.8).
    netaddr::ruserpass(unsafe { strz::strz(host) }, aname, apass, |a| unsafe { strz::strz(core::ptr::with_exposed_provenance(a)) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __ivaliduser(hostf: *mut c_void, raddr: u32, luser: *const c_char, ruser: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: the user names are strings; hostf is a stream (§15.8).
    netaddr::ivaliduser(conv::addr(hostf), raddr, unsafe { strz::strz(luser) }, unsafe { strz::strz(ruser) })
}}
