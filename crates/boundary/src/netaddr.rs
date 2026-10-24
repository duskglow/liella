//! The C conventions of spec §15.3–15.8 (phase 10): result encodings, the library-owned
//! results (per-thread slots, D-H3; process-wide buffers where the contract names one), the
//! caller-owned blocks of `getifaddrs` and `if_nameindex`, and the process-wide trust state of
//! the `rcmd` family. The logic is in `liella::net` and `liella_legacy::{inet_compat, rcmd,
//! net_compat, nss}`; the exports in `posix::s15_sockets` convert the C arguments.
//!
//! Buffers whose size a C caller does not pass (a routing header, an ancillary-data object) are
//! viewed through a callback: the header is read first, through a short view that is dropped,
//! and then the whole object, so the two views never overlap in time.

use core::sync::atomic::Ordering::Relaxed;

use liella::error::Errno;
use liella::mem::TryVec;
use liella::net::{addr, copy_z, ether, iface, ip6opt};
use liella_kernel::errno::KResult;
use liella_kernel::fd::Fd;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{AF_INET, AF_INET6, Stat};
use liella_legacy::inet_compat as ic;
use liella::names::Ip;
use liella_legacy::nss::{self, Ctx};
use liella_legacy::result_slots::Buf;
use liella_legacy::{net_compat, rcmd};
use liella_rt::thread::{SLOT_ETHER_ATON, SLOT_ETHER_NTOA, SLOT_INET_NTOA};

use crate::caller_alloc;
use crate::conv;
use crate::stdio;
use crate::sys::CSys;
use crate::thread;

/// The calling thread's result slot `i`; `ENOMEM` when the thread has none and none can be made
/// (never a buffer shared with other threads, D-C0.2).
fn slot(i: usize) -> KResult<&'static Buf<32>> {
    thread::result_slot(i).ok_or(Errno::ENOMEM)
}

/// Store `text` and its terminator in a library-owned buffer: its address.
fn store_z<const N: usize>(buf: &Buf<N>, text: &[u8]) -> usize {
    let mut z = [0u8; 64];
    let n = text.len().min(63);
    if let (Some(d), Some(s)) = (z.get_mut(..n), text.get(..n)) {
        d.copy_from_slice(s);
    }
    buf.store(z.get(..=n).unwrap_or(&[0]));
    buf.addr()
}

// ------------------------------------------------------------------------------ §15.3

/// `inet_pton(af, src, dst)`: 1, 0 for invalid text (`dst` untouched), `EAFNOSUPPORT`.
pub fn pton<'a>(af: i32, text: KResult<&[u8]>, dst: impl FnOnce(usize) -> KResult<&'a mut [u8]>) -> KResult<i64> {
    let text = text?;
    let (bytes, n): ([u8; 16], usize) = match u32::try_from(af) {
        Ok(AF_INET) => match addr::parse_v4(text) {
            Some(a) => {
                let mut b = [0u8; 16];
                b.get_mut(..4).ok_or(Errno::EINVAL)?.copy_from_slice(&a);
                (b, 4)
            }
            None => return Ok(0),
        },
        Ok(AF_INET6) => match addr::parse_v6(text) {
            Some(a) => (a, 16),
            None => return Ok(0),
        },
        _ => return Err(Errno::EAFNOSUPPORT),
    };
    dst(n)?.copy_from_slice(bytes.get(..n).ok_or(Errno::EINVAL)?);
    Ok(1)
}

/// `inet_ntop(af, src, dst, size)`: the address of `dst`, `ENOSPC` when the text and its
/// terminator do not fit (`dst` untouched), `EAFNOSUPPORT`.
pub fn ntop<'a>(af: i32, src: impl FnOnce(usize) -> KResult<&'a [u8]>, dst: KResult<&mut [u8]>) -> KResult<usize> {
    let text = match u32::try_from(af) {
        Ok(AF_INET) => addr::format_v4(src(4)?.try_into().map_err(|_| Errno::EINVAL)?),
        Ok(AF_INET6) => addr::format_v6(src(16)?.try_into().map_err(|_| Errno::EINVAL)?),
        _ => return Err(Errno::EAFNOSUPPORT),
    };
    let dst = dst?;
    copy_z(text.as_bytes(), dst).ok_or(Errno::ENOSPC)?;
    Ok(dst.as_ptr().expose_provenance())
}

/// `inet_aton(cp, inp)`: 1 or 0; a NULL `inp` only validates.
pub fn aton(text: KResult<&[u8]>, out: Option<&mut u32>) -> KResult<i64> {
    Ok(match ic::aton(text?) {
        Some(v) => {
            if let Some(o) = out {
                *o = v.to_be();
            }
            1
        }
        None => 0,
    })
}

/// `inet_addr(cp)`: the address in network order, `INADDR_NONE` for invalid text.
pub fn inet_addr(text: KResult<&[u8]>) -> KResult<u32> {
    Ok(ic::aton(text?).map_or(u32::MAX, u32::to_be))
}

/// `inet_network(cp)`: the network number in host order, -1 for invalid text.
pub fn network(text: KResult<&[u8]>) -> KResult<u32> {
    Ok(ic::network(text?).unwrap_or(u32::MAX))
}

/// `inet_ntoa(in)`: the text in the calling thread's slot (`ENOMEM` without one).
pub fn ntoa(a: u32) -> KResult<usize> {
    Ok(store_z(slot(SLOT_INET_NTOA)?, addr::format_v4(a.to_ne_bytes()).as_bytes()))
}

/// `inet_makeaddr(net, host)`: network order.
pub fn makeaddr(net: u32, host: u32) -> u32 {
    ic::makeaddr(net, host).to_be()
}

/// `inet_lnaof(in)`.
pub fn lnaof(a: u32) -> u32 {
    ic::lnaof(u32::from_be(a))
}

/// `inet_netof(in)`.
pub fn netof(a: u32) -> u32 {
    ic::netof(u32::from_be(a))
}

/// `inet_net_pton(af, cp, buf, len)`: the prefix length; `ENOENT`, `EMSGSIZE`, `EAFNOSUPPORT`.
pub fn net_pton(af: i32, text: KResult<&[u8]>, out: KResult<&mut [u8]>) -> KResult<i64> {
    if u32::try_from(af) != Ok(AF_INET) {
        return Err(Errno::EAFNOSUPPORT);
    }
    match ic::net_pton(text?, out?) {
        Ok(bits) => Ok(i64::from(bits)),
        Err(ic::NetError::Syntax) => Err(Errno::ENOENT),
        Err(ic::NetError::Size) => Err(Errno::EMSGSIZE),
    }
}

/// `inet_net_ntop(af, cp, bits, buf, len)`: the address of `buf`; `EINVAL` for a prefix
/// outside 0–32, `EMSGSIZE`, `EAFNOSUPPORT`. `src` gives the bytes the prefix covers.
pub fn net_ntop<'a>(af: i32, src: impl FnOnce(usize) -> KResult<&'a [u8]>, bits: i32, out: KResult<&mut [u8]>) -> KResult<usize> {
    if u32::try_from(af) != Ok(AF_INET) {
        return Err(Errno::EAFNOSUPPORT);
    }
    let n = u32::try_from(bits).ok().filter(|&b| b <= 32).ok_or(Errno::EINVAL)?.div_ceil(8).max(1);
    let text = ic::net_ntop(src(usize::try_from(n).map_err(|_| Errno::EINVAL)?)?, bits).ok_or(Errno::EINVAL)?;
    let out = out?;
    copy_z(text.as_bytes(), out).ok_or(Errno::EMSGSIZE)?;
    Ok(out.as_ptr().expose_provenance())
}

/// `inet_neta(src, dst, size)`: the address of `dst`, `EMSGSIZE` when the text and its
/// terminator do not fit. (The reference also refuses buffers that would fit: it needs 12 bytes
/// for `10.1.2.3` and 4 for `10` (probes, phase 10); Liella needs the text's size.)
pub fn neta(v: u32, out: KResult<&mut [u8]>) -> KResult<usize> {
    let out = out?;
    copy_z(ic::neta(v).as_bytes(), out).ok_or(Errno::EMSGSIZE)?;
    Ok(out.as_ptr().expose_provenance())
}

/// `inet_nsap_addr(cp, buf, len)`: the bytes stored (0 for invalid text).
pub fn nsap_addr(text: KResult<&[u8]>, out: KResult<&mut [u8]>) -> u32 {
    match (text, out) {
        (Ok(t), Ok(o)) => u32::try_from(ic::nsap_addr(t, o)).unwrap_or(0),
        _ => 0,
    }
}

/// `inet_nsap_ntoa(len, cp, buf)`: the text in `buf`, or (NULL `buf`) in the calling thread's buffer;
/// at most [`ic::NSAP_MAX`] bytes are formatted. `out` gives the caller's buffer of a given size.
pub fn nsap_ntoa<'a>(len: i32, src: impl FnOnce(usize) -> KResult<&'a [u8]>, out: Option<impl FnOnce(usize) -> KResult<&'a mut [u8]>>) -> KResult<usize> {
    let n = usize::try_from(len).unwrap_or(0).min(ic::NSAP_MAX);
    match out {
        Some(view) => {
            let o = view(ic::nsap_text_len(n))?;
            ic::nsap_ntoa(src(n)?, o);
            Ok(o.as_ptr().expose_provenance())
        }
        None => {
            let mut text = [0u8; 640];
            let w = ic::nsap_ntoa(src(n)?, &mut text);
            let buf = crate::users::result_buf::<640>(crate::users::NSAP_NTOA)?;
            buf.store(text.get(..w).unwrap_or(&[0]));
            Ok(buf.addr())
        }
    }
}

// ------------------------------------------------------------------------------ §16.1

/// The caller's places of `__nss_hostname_digits_dots` (§16.1) besides the name and family.
pub struct DigitsDots<'a> {
    /// The entry filled.
    pub resbuf: Option<&'a mut liella_abi::layout::struct_hostent>,
    /// `*buffer`: the address of the strings' memory.
    pub buffer: Option<&'a mut usize>,
    /// `*buffer_size` (the allocating form), or NULL (the caller-buffer form).
    pub buffer_size: Option<&'a mut usize>,
    /// The caller buffer's size.
    pub buflen: usize,
    /// `*result` (written in the allocating form).
    pub result: Option<&'a mut usize>,
    /// `*status` (written in the caller-buffer form).
    pub status: Option<&'a mut i32>,
    /// `*h_errnop`.
    pub h_errnop: Option<&'a mut i32>,
}

/// `NSS_STATUS_TRYAGAIN`, `NSS_STATUS_NOTFOUND`, `NSS_STATUS_SUCCESS`; `NETDB_INTERNAL`,
/// `HOST_NOT_FOUND`.
const TRYAGAIN: i32 = -2;
const NOTFOUND: i32 = 0;
const SUCCESS: i32 = 1;
const NETDB_INTERNAL: i32 = -1;
const HOST_NOT_FOUND: i32 = 1;

/// The entry's bytes before the name: the address (16), the address list (2 words), the alias
/// list (1 word).
const DD_HEAD: usize = 40;

/// `__nss_hostname_digits_dots(name, ...)` (§16.1, written 2026-10-05 by owner ruling from
/// `spec/behaviour/digits-dots.md`): 1 when `name` is numeric (handled, found or not), else 0.
/// `view(addr, n)` gives the `n` bytes at `addr` of the strings' memory.
pub fn digits_dots<'a>(name: KResult<&[u8]>, af: i32, mut p: DigitsDots<'_>, view: impl FnOnce(usize, usize) -> KResult<&'a mut [u8]>) -> i32 {
    let Ok(name) = name else { return 0 };
    if name.is_empty() {
        return 0;
    }
    // The allocating form sizes `*buffer` for any name but the empty one, numeric or not (as
    // observed), before the name is read.
    let found = ic::digits_dots(name, af);
    let set = |v: Option<&mut i32>, x: i32| {
        if let Some(v) = v {
            *v = x;
        }
    };
    let need = DD_HEAD.saturating_add(name.len()).saturating_add(1).saturating_add(7);
    let allocating = p.buffer_size.is_some();
    let (base, room) = match (p.buffer_size.as_deref_mut(), p.buffer.as_deref_mut()) {
        (Some(size), Some(buffer)) => {
            if *size < need {
                let q = caller_alloc::realloc(core::ptr::with_exposed_provenance_mut(*buffer), need);
                if q.is_null() {
                    if let Some(r) = p.result.as_deref_mut() {
                        *r = 0;
                    }
                    set(p.h_errnop, NETDB_INTERNAL);
                    crate::errno::set(Errno::ENOMEM);
                    return 1;
                }
                *buffer = q.expose_provenance();
                *size = need;
            }
            (*buffer, *size)
        }
        (None, Some(buffer)) => (*buffer, p.buflen),
        (_, None) => (0, 0),
    };
    let Some(found) = found else { return 0 };
    let Some((family, bytes, len)) = found else {
        match p.result.as_deref_mut() {
            Some(r) if allocating => *r = 0,
            _ => {}
        }
        if !allocating {
            set(p.status, NOTFOUND);
        }
        set(p.h_errnop, HOST_NOT_FOUND);
        return 1;
    };
    let aligned = base.checked_next_multiple_of(8).unwrap_or(0);
    let used = aligned.saturating_sub(base).saturating_add(DD_HEAD).saturating_add(name.len()).saturating_add(1);
    let fits = base != 0 && used <= room;
    let mem = if fits { view(aligned, used.saturating_sub(aligned.saturating_sub(base))).ok() } else { None };
    let (Some(mem), Some(entry)) = (mem, p.resbuf) else {
        set(p.status, TRYAGAIN);
        set(p.h_errnop, NETDB_INTERNAL);
        crate::errno::set(Errno::ERANGE);
        return 1;
    };
    // [address 16][address list: &address, NULL][aliases: NULL][name, NUL]
    let words = [aligned, 0, 0];
    if let Some(d) = mem.get_mut(..16) {
        d.copy_from_slice(&bytes);
    }
    for (d, w) in mem.get_mut(16..DD_HEAD).unwrap_or(&mut []).chunks_exact_mut(8).zip(words) {
        d.copy_from_slice(&w.to_ne_bytes());
    }
    let name_end = DD_HEAD.saturating_add(name.len());
    if let Some(d) = mem.get_mut(DD_HEAD..name_end) {
        d.copy_from_slice(name);
    }
    if let Some(z) = mem.get_mut(name_end) {
        *z = 0;
    }
    entry.h_name = aligned.saturating_add(DD_HEAD);
    entry.h_aliases = aligned.saturating_add(32);
    entry.h_addrtype = family;
    entry.h_length = i32::try_from(len).unwrap_or(4);
    entry.h_addr_list = aligned.saturating_add(16);
    if allocating {
        if let Some(r) = p.result.as_deref_mut() {
            *r = core::ptr::from_mut(entry).expose_provenance();
        }
    } else {
        set(p.status, SUCCESS);
    }
    set(p.h_errnop, 0);
    1
}

// ------------------------------------------------------------------------------ §15.4

/// `if_nametoindex(name)`: the index, or 0 with `errno`.
pub fn nametoindex(name: KResult<&[u8]>) -> (u32, Option<Errno>) {
    match name.and_then(|n| iface::index_of(&CSys, n)) {
        Ok(i) => (i, None),
        Err(e) => (0, Some(e)),
    }
}

/// `if_indextoname(index, buf)`: the address of `buf` (`IF_NAMESIZE` bytes), `ENXIO`.
pub fn indextoname(index: u32, buf: Option<&mut [u8; iface::IF_NAMESIZE]>) -> KResult<usize> {
    let name = iface::name_of(&CSys, index)?;
    let buf = buf.ok_or(Errno::EFAULT)?;
    copy_z(name.as_bytes(), buf).ok_or(Errno::ENAMETOOLONG)?;
    Ok(buf.as_ptr().expose_provenance())
}

/// A zeroed caller-owned block of `n` bytes and its address.
fn block(n: usize) -> KResult<(&'static mut [u8], usize)> {
    let p = caller_alloc::calloc(n.max(1), 1);
    let s = conv::fresh::<u8>(p, n.max(1)).ok_or(Errno::ENOMEM)?;
    Ok((s, p.expose_provenance()))
}

/// `if_nameindex()`: the caller-owned array, or NULL with `errno`.
pub fn nameindex() -> KResult<usize> {
    let links = iface::links(&CSys)?;
    let (out, base) = block(net_compat::nameindex_size(&links).ok_or(Errno::ENOMEM)?)?;
    if net_compat::nameindex_fill(&links, out, base).is_none() {
        caller_alloc::release(base);
        return Err(Errno::ENOMEM);
    }
    Ok(base)
}

/// `getifaddrs(&ifap)`: the caller-owned list in `*ifap`.
pub fn getifaddrs(ifap: Option<&mut usize>) -> KResult<()> {
    let ifap = ifap.ok_or(Errno::EFAULT)?;
    let links = iface::links(&CSys)?;
    let addrs = iface::addrs(&CSys)?;
    let size = net_compat::ifaddrs_size(&links, &addrs).ok_or(Errno::ENOMEM)?;
    if size == 0 {
        *ifap = 0;
        return Ok(());
    }
    let (out, base) = block(size)?;
    if net_compat::ifaddrs_fill(&links, &addrs, out, base).is_none() {
        caller_alloc::release(base);
        return Err(Errno::ENOMEM);
    }
    *ifap = base;
    Ok(())
}

/// `freeifaddrs` and `if_freenameindex`: release the block. Obligation (C contract): `addr` is
/// NULL or the result of `getifaddrs` or `if_nameindex`, not used afterwards.
pub fn release(addr: usize) {
    caller_alloc::release(addr);
}

// ------------------------------------------------------------------------------ §15.5

/// The `int` result of the option helpers: -1 for any failure (no `errno`, RFC 3542).
fn opt_int(r: Option<i32>) -> i32 {
    r.unwrap_or(-1)
}

/// `inet6_opt_init(extbuf, extlen)`.
pub fn opt_init(buf: KResult<Option<&mut [u8]>>) -> i32 {
    opt_int(buf.ok().and_then(ip6opt::opt_init))
}

/// `inet6_opt_append(extbuf, extlen, offset, type, len, align, &databuf)`.
pub fn opt_append(buf: KResult<Option<&mut [u8]>>, offset: i32, ty: u8, len: u32, align: u8, data: Option<&mut usize>) -> i32 {
    let Ok(buf) = buf else { return -1 };
    let base = buf.as_ref().map(|b| b.as_ptr().expose_provenance());
    let len = i32::try_from(len).unwrap_or(-1);
    match ip6opt::opt_append(buf, offset, ty, len, i32::from(align)) {
        Some((end, at)) => {
            if let (Some(base), Some(d)) = (base, data) {
                *d = base.saturating_add(at);
            }
            end
        }
        None => -1,
    }
}

/// `inet6_opt_finish(extbuf, extlen, offset)`.
pub fn opt_finish(buf: KResult<Option<&mut [u8]>>, offset: i32) -> i32 {
    opt_int(buf.ok().and_then(|b| ip6opt::opt_finish(b, offset)))
}

/// `inet6_opt_set_val`/`get_val(databuf, offset, val, vallen)`: copy `vallen` bytes; the offset
/// after them.
pub fn opt_val(offset: i32, vallen: u32, copy: impl FnOnce(usize, usize) -> KResult<()>) -> i32 {
    let (Ok(o), Ok(n)) = (usize::try_from(offset), usize::try_from(vallen)) else { return -1 };
    match copy(o, n) {
        Ok(()) => i32::try_from(o.saturating_add(n)).unwrap_or(-1),
        Err(_) => -1,
    }
}

/// `inet6_opt_next`/`find`: the offset after the option found, its type, length and data
/// stored where asked.
pub fn opt_walk(buf: KResult<&[u8]>, found: impl FnOnce(&[u8]) -> Option<ip6opt::Opt>, ty: Option<&mut u8>, len: Option<&mut u32>, data: Option<&mut usize>) -> i32 {
    let Ok(buf) = buf else { return -1 };
    let Some(o) = found(buf) else { return -1 };
    if let Some(t) = ty {
        *t = o.ty;
    }
    if let Some(l) = len {
        *l = u32::try_from(o.len).unwrap_or(0);
    }
    if let Some(d) = data {
        *d = buf.as_ptr().expose_provenance().saturating_add(o.data);
    }
    i32::try_from(o.next).unwrap_or(-1)
}

/// The length of the type-0 routing header that starts with `head` (8 bytes): its fixed part
/// and the addresses its length byte declares.
pub fn rth_len(head: KResult<[u8; 8]>) -> KResult<usize> {
    let h = head?;
    let n = ip6opt::rth_segments(&h).ok_or(Errno::EINVAL)?;
    Ok(ip6opt::rth_space(0, n).max(8))
}

/// `inet6_rth_add(bp, addr)`.
pub fn rth_add(bp: KResult<&mut [u8]>, a: Option<&[u8; 16]>) -> i32 {
    match (bp, a) {
        (Ok(b), Some(a)) => opt_int(ip6opt::rth_add(b, a).map(|()| 0)),
        _ => -1,
    }
}

/// `inet6_rth_reverse(in, out)`: `len` is the input header's size (from [`rth_len`]); `input`
/// views it and `out` the output, which may be the same header: the input is copied and its
/// view ended before the output is viewed, so the two never overlap in time.
pub fn rth_reverse<'a>(len: KResult<usize>, input: impl FnOnce(usize) -> KResult<&'a [u8]>, out: impl FnOnce(usize) -> KResult<&'a mut [u8]>) -> i32 {
    let mut copy = [0u8; 8 + 127 * 16];
    let Ok(n) = len else { return -1 };
    let n = n.min(copy.len());
    match input(n) {
        Ok(i) => match (copy.get_mut(..n), i.get(..n)) {
            (Some(d), Some(s)) => d.copy_from_slice(s),
            _ => return -1,
        },
        Err(_) => return -1,
    }
    let Ok(o) = out(n) else { return -1 };
    opt_int(ip6opt::rth_reverse(copy.get(..n).unwrap_or(&[]), o).map(|()| 0))
}

/// The first 8 bytes of a header, copied (so the view that gave them can end).
pub fn head8(v: KResult<&[u8]>) -> KResult<[u8; 8]> {
    v?.get(..8).and_then(|s| s.try_into().ok()).ok_or(Errno::EINVAL)
}

// RFC 2292 (§15.5, deprecated).

/// `inet6_option_init(bp, &cmsgp, type)`.
pub fn option_init(head: KResult<&mut [u8]>, cmsgp: Option<&mut usize>, ty: i32) -> i32 {
    let (Ok(h), Some(c)) = (head, cmsgp) else { return -1 };
    let base = h.as_ptr().expose_provenance();
    match ic::option_init(h, ty) {
        Some(()) => {
            *c = base;
            0
        }
        None => -1,
    }
}

/// `inet6_option_append(cmsg, typep, multx, plusy)` (`opt` gives the option, its length from
/// its second byte) and `inet6_option_alloc(cmsg, datalen, multx, plusy)` (`opt` `None`): the
/// option's address, 0 on failure. RFC 2292 §6.3.3 reserves types 0 and 1 for the pads, so
/// `inet6_option_append` refuses them, and a negative `datalen` is refused (the reference
/// accepts both; probes, phase 10).
pub fn option_add<'a>(obj: impl Fn(usize) -> KResult<&'a mut [u8]>, opt: Option<KResult<&[u8]>>, datalen: i32, multx: i32, plusy: i32) -> usize {
    (|| -> Option<usize> {
        let old = ic::cmsg_len(obj(ic::CMSG_HDR).ok()?)?;
        let optlen = match &opt {
            Some(Ok(o)) => {
                let ty = *o.first()?;
                if ty < 2 {
                    return None;
                }
                usize::from(*o.get(1)?).checked_add(2)?
            }
            Some(Err(_)) => return None,
            None => usize::try_from(datalen).ok()?,
        };
        let (at, new) = ic::option_place(old, optlen, multx, plusy)?;
        let o = obj(new).ok()?;
        ic::option_layout(o, old, at, optlen, new)?;
        if let Some(Ok(src)) = opt {
            o.get_mut(at..at.checked_add(optlen)?)?.copy_from_slice(src.get(..optlen)?);
        }
        o.as_ptr().expose_provenance().checked_add(at)
    })()
    .unwrap_or(0)
}

/// `inet6_option_next(cmsg, &tptr)` and `inet6_option_find(cmsg, &tptr, type)`.
pub fn option_walk<'a>(obj: impl Fn(usize) -> KResult<&'a [u8]>, tptr: Option<&mut usize>, ty: Option<i32>) -> i32 {
    let Some(t) = tptr else { return -1 };
    let Some(o) = obj(ic::CMSG_HDR).ok().and_then(ic::cmsg_len).and_then(|n| obj(n).ok()) else { return -1 };
    let base = o.as_ptr().expose_provenance();
    let prev = match *t {
        0 => None,
        a => match a.checked_sub(base) {
            Some(off) if off < o.len() => Some(off),
            _ => return -1,
        },
    };
    let step = match ty {
        Some(ty) => match u8::try_from(ty) {
            Ok(ty) => ic::option_find(o, prev, ty),
            Err(_) => ic::Step::End,
        },
        None => ic::option_next(o, prev),
    };
    match step {
        ic::Step::At(off) => {
            *t = base.saturating_add(off);
            0
        }
        ic::Step::End => {
            *t = 0;
            -1
        }
        ic::Step::Error => -1,
    }
}

// ------------------------------------------------------------------------------ §15.6

/// `getipv4sourcefilter`: `slist` holds `*numsrc` addresses on entry.
pub fn get_ipv4_filter<'a>(fd: i32, iface: u32, group: u32, mode: Option<&mut u32>, numsrc: Option<&mut u32>, slist: impl FnOnce(usize) -> KResult<&'a mut [u8]>) -> KResult<()> {
    let (mode, numsrc) = (mode.ok_or(Errno::EFAULT)?, numsrc.ok_or(Errno::EFAULT)?);
    let cap = usize::try_from(*numsrc).map_err(|_| Errno::EINVAL)?;
    let list = slist(cap.checked_mul(4).ok_or(Errno::EINVAL)?)?;
    let f = net_compat::get_ipv4_filter(&CSys, conv::fd(fd), iface.to_ne_bytes(), group.to_ne_bytes(), list)?;
    *mode = f.mode;
    *numsrc = f.numsrc;
    Ok(())
}

/// `setipv4sourcefilter`.
pub fn set_ipv4_filter(fd: i32, iface: u32, group: u32, mode: u32, slist: KResult<&[u8]>) -> KResult<()> {
    net_compat::set_ipv4_filter(&CSys, conv::fd(fd), iface.to_ne_bytes(), group.to_ne_bytes(), mode, slist?)
}

/// `getsourcefilter`: `slist` holds `*numsrc` `struct sockaddr_storage` on entry.
pub fn get_filter<'a>(fd: i32, iface: u32, group: KResult<&[u8]>, mode: Option<&mut u32>, numsrc: Option<&mut u32>, slist: impl FnOnce(usize) -> KResult<&'a mut [u8]>) -> KResult<()> {
    let (mode, numsrc) = (mode.ok_or(Errno::EFAULT)?, numsrc.ok_or(Errno::EFAULT)?);
    let cap = usize::try_from(*numsrc).map_err(|_| Errno::EINVAL)?;
    let list = slist(cap.checked_mul(128).ok_or(Errno::EINVAL)?)?;
    let f = net_compat::get_filter(&CSys, conv::fd(fd), iface, group?, list)?;
    *mode = f.mode;
    *numsrc = f.numsrc;
    Ok(())
}

/// `setsourcefilter`.
pub fn set_filter(fd: i32, iface: u32, group: KResult<&[u8]>, mode: u32, slist: KResult<&[u8]>) -> KResult<()> {
    net_compat::set_filter(&CSys, conv::fd(fd), iface, group?, mode, slist?)
}

// ------------------------------------------------------------------------------ §15.7

/// `ether_aton_r(asc, addr)`: the address of `addr`, or 0 (NULL) for invalid text; the bytes
/// read before the failure are stored (as the reference).
pub fn ether_aton_r(text: KResult<&[u8]>, out: Option<&mut [u8; 6]>) -> KResult<usize> {
    let (text, out) = (text?, out.ok_or(Errno::EFAULT)?);
    let p = ether::parse(text);
    let n = p.stored.min(6);
    out.get_mut(..n).ok_or(Errno::EINVAL)?.copy_from_slice(p.bytes.get(..n).ok_or(Errno::EINVAL)?);
    Ok(if p.address().is_some() { out.as_ptr().expose_provenance() } else { 0 })
}

/// `ether_aton(asc)`: the address in the calling thread's slot, or NULL.
pub fn ether_aton(text: KResult<&[u8]>) -> KResult<usize> {
    let p = ether::parse(text?);
    let Some(a) = p.address() else { return Ok(0) };
    let buf = slot(SLOT_ETHER_ATON)?;
    buf.store(&a);
    Ok(buf.addr())
}

/// `ether_ntoa_r(addr, buf)`: the text in `buf` (at least 18 bytes, the caller's contract).
pub fn ether_ntoa_r(a: Option<&[u8; 6]>, out: KResult<&mut [u8]>) -> KResult<usize> {
    let t = ether::format(*a.ok_or(Errno::EFAULT)?);
    let out = out?;
    copy_z(t.as_bytes(), out).ok_or(Errno::EFAULT)?;
    Ok(out.as_ptr().expose_provenance())
}

/// `ether_ntoa(addr)`: the text in the calling thread's slot.
pub fn ether_ntoa(a: Option<&[u8; 6]>) -> KResult<usize> {
    let t = ether::format(*a.ok_or(Errno::EFAULT)?);
    Ok(store_z(slot(SLOT_ETHER_NTOA)?, t.as_bytes()))
}

/// `ether_line(line, addr, hostname)`: 0, or -1 for a line without an address and a name;
/// the address bytes read are stored either way. `host` gives the caller's buffer for a name
/// of a given length (the contract leaves its size to the caller).
pub fn ether_line<'a>(line: KResult<&[u8]>, out: Option<&mut [u8; 6]>, host: impl FnOnce(usize) -> KResult<&'a mut [u8]>) -> KResult<i64> {
    let (line, out) = (line?, out.ok_or(Errno::EFAULT)?);
    let r = ether::line(line);
    let n = r.parsed.stored.min(6);
    out.get_mut(..n).ok_or(Errno::EINVAL)?.copy_from_slice(r.parsed.bytes.get(..n).ok_or(Errno::EINVAL)?);
    let Some(name) = r.host else { return Ok(-1) };
    copy_z(name, host(name.len().saturating_add(1))?).ok_or(Errno::EFAULT)?;
    Ok(0)
}

/// `ether_hostton(hostname, addr)`: 0 or -1 (the `ethers` database through the switch,
/// `liella_legacy::nss`).
pub fn ether_hostton(name: KResult<&[u8]>, out: Option<&mut [u8; 6]>) -> KResult<i64> {
    let (name, out) = (name?, out.ok_or(Errno::EFAULT)?);
    Ok(match nss::ether_by_name(&Ctx::system(&CSys), name) {
        Ok(a) => {
            *out = a;
            0
        }
        Err(_) => -1,
    })
}

/// `ether_ntohost(hostname, addr)`: 0 or -1. `host` gives the caller's buffer for a name of a
/// given length.
pub fn ether_ntohost<'a>(host: impl FnOnce(usize) -> KResult<&'a mut [u8]>, a: Option<&[u8; 6]>) -> KResult<i64> {
    // Any failure of the lookup (no entry, no file, no source) is -1, as for `ether_hostton`.
    let Ok(name) = nss::ether_by_addr(&Ctx::system(&CSys), *a.ok_or(Errno::EFAULT)?) else { return Ok(-1) };
    copy_z(&name, host(name.len().saturating_add(1))?).ok_or(Errno::EFAULT)?;
    Ok(0)
}

// ------------------------------------------------------------------------------ §15.8

/// `rresvport_af(&port, af)`.
pub fn rresvport(port: Option<&mut i32>, af: i32) -> KResult<Fd> {
    rcmd::rresvport(&CSys, port.ok_or(Errno::EFAULT)?, af)
}

/// Store the canonical host name in the calling thread's buffer and `*ahost`; without the
/// buffer `*ahost` keeps the caller's name (D-C0.2: never a buffer shared with other threads).
fn set_host(ahost: &mut usize, name: &[u8]) {
    let n = name.len().min(1024);
    let mut z = [0u8; 1026];
    if let (Some(d), Some(s)) = (z.get_mut(..n), name.get(..n)) {
        d.copy_from_slice(s);
    }
    if let Ok(buf) = crate::users::result_buf::<1026>(crate::users::RCMD_HOST) {
        buf.store(z.get(..=n).unwrap_or(&[0]));
        *ahost = buf.addr();
    }
}

/// The result of `rcmd`/`rexec`: the descriptor, or -1 with `errno` when the failure has one.
pub fn session(r: Result<rcmd::Session, rcmd::Failed>, fd2p: Option<&mut i32>) -> i32 {
    match r {
        Ok(s) => {
            if let (Some(p), Some(f)) = (fd2p, s.fd2) {
                *p = f.raw();
            }
            s.fd.raw()
        }
        Err(e) => {
            if let Some(e) = e {
                crate::errno::set_raw(e.get());
            }
            -1
        }
    }
}

/// A private copy of the host text `*ahost` names: `*ahost` may point at the buffer
/// [`set_host`] rewrites.
fn host_copy(text: KResult<&[u8]>) -> Result<TryVec<u8>, rcmd::Failed> {
    TryVec::from_slice(text.map_err(Some)?).map_err(Some)
}

/// `rcmd_af(&ahost, rport, locuser, remuser, cmd, fd2p, af)`.
#[allow(clippy::too_many_arguments)] // the C call's seven and the reader of `*ahost`
pub fn rcmd_af(ahost: Option<&mut usize>, host: impl FnOnce(usize) -> KResult<&'static [u8]>, rport: u16, locuser: KResult<&[u8]>, remuser: KResult<&[u8]>, cmd: KResult<&[u8]>, fd2p: Option<&mut i32>, af: u16) -> i32 {
    let Some(ahost) = ahost else { return session(Err(Some(Errno::EFAULT)), None) };
    let r = (|| {
        let h = host_copy(host(*ahost))?;
        let names = [locuser.map_err(Some)?, remuser.map_err(Some)?];
        let req = rcmd::Request { host: &h, port: rport, names, cmd: cmd.map_err(Some)?, want_fd2: fd2p.is_some(), af: i32::from(af) };
        rcmd::rcmd(&CSys, req, |canonical| set_host(ahost, canonical))
    })();
    session(r, fd2p)
}

/// The program's short name, for `ruserpass`'s messages.
fn progname() -> &'static [u8] {
    conv::process_text(crate::ffi::program_invocation_short_name.load(Relaxed))
}

/// The `.netrc` lookup of `ruserpass` and `rexec`: `$HOME/.netrc` for `host`, with the name and
/// password already known. No `HOME` or no file: nothing is learned.
fn netrc(host: &[u8], name: Option<&[u8]>, pass: Option<&[u8]>) -> Result<rcmd::Netrc, rcmd::Refused> {
    let given = || -> Result<rcmd::Netrc, rcmd::Refused> {
        let copy = |v: Option<&[u8]>| v.map(TryVec::from_slice).transpose().map_err(|_| rcmd::Refused::Memory);
        Ok(rcmd::Netrc { name: copy(name)?, pass: copy(pass)? })
    };
    let Some(home) = crate::process::getenv(b"HOME").map(crate::strz::env_value) else { return given() };
    let path = (|| -> KResult<liella_kernel::path::KPathBuf> {
        let mut v = TryVec::new();
        v.extend_from_slice(home)?;
        v.extend_from_slice(b"/.netrc")?;
        liella_kernel::path::KPathBuf::new(&v)
    })();
    let Ok(path) = path else { return given() };
    let Ok(fd) = CSys.openat(Fd::CWD, path.as_kstr(), liella_kernel::uapi::O_RDONLY | liella_kernel::uapi::O_CLOEXEC, 0) else { return given() };
    let read = (|| -> KResult<(TryVec<u8>, Stat)> {
        let st = CSys.fstat(fd)?;
        let mut text = TryVec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let k = CSys.read(fd, &mut chunk)?;
            if k == 0 {
                return Ok((text, st));
            }
            text.extend_from_slice(chunk.get(..k).unwrap_or(&[]))?;
        }
    })();
    let _ = CSys.close(fd);
    let Ok((text, st)) = read else { return given() };
    rcmd::ruserpass(&text, st.mode, host, name, pass, |parts| {
        let mut line: liella::net::Text<512> = liella::net::Text::new();
        line.push_all(progname());
        line.push_all(b": ");
        for p in parts {
            line.push_all(p);
        }
        let _ = CSys.write(Fd::STDERR, line.as_bytes());
    })
}

/// `ruserpass(host, &aname, &apass)` over `$HOME/.netrc`: 0, or -1 when a password sits in a
/// file others may read. What was learned is stored as caller-owned copies (kept for the
/// process, as the reference's static results), the login name also on that refusal.
pub fn ruserpass<'a>(host: KResult<&[u8]>, aname: Option<&mut usize>, apass: Option<&mut usize>, text_at: impl Fn(usize) -> KResult<&'a [u8]>) -> i32 {
    let (Ok(host), Some(aname), Some(apass)) = (host, aname, apass) else { return -1 };
    let given = |a: usize| if a == 0 { Ok(None) } else { text_at(a).map(Some) };
    let (Ok(name), Ok(pass)) = (given(*aname), given(*apass)) else { return -1 };
    let (found, status) = match netrc(host, name, pass) {
        Ok(found) => (found, 0),
        Err(rcmd::Refused::Exposed(found)) => (found, -1),
        Err(rcmd::Refused::Memory) => return -1,
    };
    // Only what the file supplied is replaced; a caller's own text stays where it is.
    for (cell, v, had) in [(aname, found.name, name.is_some()), (apass, found.pass, pass.is_some())] {
        if let (Some(v), false) = (v, had) {
            if let Ok(a) = caller_alloc::dup_z(&v) {
                *cell = a;
            }
        }
    }
    status
}

/// `rexec_af(&ahost, rport, name, pass, cmd, fd2p, af)`: a missing name or password comes from
/// `$HOME/.netrc` (as `ruserpass`); still missing, the call fails (the reference would prompt
/// on the terminal, which needs `getpass`, phase 11).
#[allow(clippy::too_many_arguments)] // the C call's seven and the reader of `*ahost`
pub fn rexec_af(ahost: Option<&mut usize>, host: impl FnOnce(usize) -> KResult<&'static [u8]>, rport: i32, name: KResult<Option<&[u8]>>, pass: KResult<Option<&[u8]>>, cmd: KResult<&[u8]>, fd2p: Option<&mut i32>, af: u16) -> i32 {
    let Some(ahost) = ahost else { return session(Err(Some(Errno::EFAULT)), None) };
    let r = (|| {
        let h = host_copy(host(*ahost))?;
        let (n, p) = (name.map_err(Some)?, pass.map_err(Some)?);
        let found = match (n, p) {
            (Some(_), Some(_)) => rcmd::Netrc::default(),
            _ => netrc(&h, n, p).map_err(|_| None)?,
        };
        let (Some(n), Some(p)) = (n.or(found.name.as_deref()), p.or(found.pass.as_deref())) else { return Err(None) };
        let port = u16::try_from(rport & 0xffff).unwrap_or(0);
        let req = rcmd::Request { host: &h, port, names: [n, p], cmd: cmd.map_err(Some)?, want_fd2: fd2p.is_some(), af: i32::from(af) };
        rcmd::rexec(&CSys, req, |canonical| set_host(ahost, canonical))
    })();
    session(r, fd2p)
}

/// Record `why` in `__rcmd_errstr`.
fn note(why: Option<&'static [u8]>) {
    if let Some(m) = why {
        crate::data::set_rcmd_errstr(m);
    }
}

/// `ruserok_af(rhost, superuser, ruser, luser, af)`: 0 or -1.
pub fn ruserok(rhost: KResult<&[u8]>, superuser: i32, ruser: KResult<&[u8]>, luser: KResult<&[u8]>, af: u16) -> i32 {
    let (Ok(h), Ok(r), Ok(l)) = (rhost, ruser, luser) else { return -1 };
    let mut why = None;
    let ok = rcmd::ruserok(&CSys, rcmd::TrustFiles::system(crate::data::check_rhosts()), h, superuser != 0, r, l, i32::from(af), &mut why);
    note(why);
    if ok { 0 } else { -1 }
}

/// `iruserok_af(raddr, superuser, ruser, luser, af)`: 0 or -1. `raddr` gives the address bytes
/// (4 or 16, by family).
pub fn iruserok<'a>(raddr: impl FnOnce(usize) -> KResult<&'a [u8]>, superuser: i32, ruser: KResult<&[u8]>, luser: KResult<&[u8]>, af: u16) -> i32 {
    let n = match u32::from(af) {
        AF_INET => 4,
        AF_INET6 => 16,
        _ => return -1,
    };
    let (Ok(a), Ok(r), Ok(l)) = (raddr(n), ruser, luser) else { return -1 };
    let Some(ip) = rcmd::ip_of(i32::from(af), a) else { return -1 };
    let mut why = None;
    let ok = rcmd::iruserok(&CSys, rcmd::TrustFiles::system(crate::data::check_rhosts()), ip, superuser != 0, r, l, &mut why);
    note(why);
    if ok { 0 } else { -1 }
}

/// `iruserok(raddr, superuser, ruser, luser)`: [`iruserok`] for an IPv4 address (network
/// order).
pub fn iruserok_v4(raddr: u32, superuser: i32, ruser: KResult<&[u8]>, luser: KResult<&[u8]>) -> i32 {
    let b = raddr.to_ne_bytes();
    iruserok(|_| Ok(&b[..]), superuser, ruser, luser, 2)
}

/// `inet6_rth_segments(bp)`.
pub fn rth_segments(head: KResult<[u8; 8]>) -> i32 {
    head.ok().and_then(|h| ip6opt::rth_segments(&h)).unwrap_or(-1)
}

/// `inet6_rth_getaddr(bp, index)`: the offset of the address, from the header.
pub fn rth_getaddr(head: KResult<[u8; 8]>, index: i32) -> Option<usize> {
    ip6opt::rth_getaddr(&head.ok()?, index)
}

/// `inet6_rth_init(bp, bp_len, type, segments)`: the address of `bp`, or 0.
pub fn rth_init(bp: KResult<&mut [u8]>, ty: i32, segments: i32) -> usize {
    match bp {
        Ok(b) => {
            let a = b.as_ptr().expose_provenance();
            ip6opt::rth_init(b, ty, segments).map_or(0, |()| a)
        }
        Err(_) => 0,
    }
}

/// `__ivaliduser(hostf, raddr, luser, ruser)`: 0 if the stream's lines admit `ruser`, else -1.
pub fn ivaliduser(fp: usize, raddr: u32, luser: KResult<&[u8]>, ruser: KResult<&[u8]>) -> i32 {
    let (Ok(l), Ok(r)) = (luser, ruser) else { return -1 };
    let mut text = liella::mem::TryVec::new();
    while let Ok(Some(c)) = stdio::getc(fp, stdio::Locking::Locked) {
        if text.push(c).is_err() {
            return -1;
        }
    }
    if rcmd::validate(&CSys, &text, Ip::V4(raddr.to_ne_bytes()), l, r) { 0 } else { -1 }
}

// ------------------------------------------------------------------------------ §24.5

/// `gethostid()` (`liella_legacy::sys_compat::gethostid`): sign-extended to `long`.
pub fn gethostid() -> i64 {
    i64::from(liella_legacy::sys_compat::gethostid(&CSys, liella_legacy::sys_compat::HOSTID_FILE))
}

/// `sethostid(id)` (`liella_legacy::sys_compat::sethostid`).
pub fn sethostid(id: i64) -> KResult<()> {
    liella_legacy::sys_compat::sethostid(&CSys, liella_legacy::sys_compat::HOSTID_FILE, id)
}
