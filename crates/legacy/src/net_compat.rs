//! The C shapes of spec §15.1–15.2 that add something to the kernel call: the in-out
//! `socklen_t` of the address and option results (truncate to the caller's capacity, report
//! the full length), `setsockopt`'s split between levels the typed call covers and those that
//! register caller memory with the kernel, `isfdtype`, and the `__cmsg_nxthdr` walk over a
//! control buffer. Spec §15.4 and §15.6 (phase 10): the `getifaddrs` and `if_nameindex`
//! blocks, and the source-filter structures.

use liella::error::{Errno, Result};
use liella::mem::TryVec;
use liella::net::iface::{Addr, Link, IF_NAMESIZE};
use liella_kernel::fd::Fd;
use liella_kernel::ops::SockLevel;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{SockAddrBuf, S_IFMT};

/// A caller's address result: its buffer (`*len` bytes on entry) and the length cell, which
/// receives the address's full length.
pub struct AddrOut<'a> {
    /// The caller's `struct sockaddr` storage.
    pub buf: &'a mut [u8],
    /// The caller's `socklen_t`.
    pub len: &'a mut u32,
}

/// Copies `src` into `out`, truncated to its capacity, and reports the full length (§15.1:
/// truncation is silent; the caller compares the lengths). The kernel's addresses are at most
/// `sizeof(struct sockaddr_storage)`, the size of `SockAddrBuf`, so this equals passing the
/// caller's buffer to the kernel.
pub fn store_addr(src: &SockAddrBuf, out: Option<AddrOut<'_>>) {
    if let Some(out) = out {
        for (d, s) in out.buf.iter_mut().zip(src.bytes()) {
            *d = *s;
        }
        *out.len = src.reported_len();
    }
}

/// `accept4(fd, addr, len, flags)` (§15.1).
pub fn accept<S: Sys>(sys: &S, fd: Fd, out: Option<AddrOut<'_>>, flags: u32) -> Result<Fd> {
    let mut peer = SockAddrBuf::new();
    let new = sys.accept4(fd, out.is_some().then_some(&mut peer), flags)?;
    store_addr(&peer, out);
    Ok(new)
}

/// `getsockname` (`peer` false) or `getpeername` (§15.1).
pub fn sockname<S: Sys>(sys: &S, fd: Fd, out: AddrOut<'_>, peer: bool) -> Result<()> {
    let mut a = SockAddrBuf::new();
    if peer { sys.getpeername(fd, &mut a)? } else { sys.getsockname(fd, &mut a)? }
    store_addr(&a, Some(out));
    Ok(())
}

/// `recvfrom(fd, buf, n, flags, addr, len)` (§15.2).
pub fn recvfrom<S: Sys>(sys: &S, fd: Fd, buf: &mut [u8], flags: u32, out: Option<AddrOut<'_>>) -> Result<usize> {
    let mut from = SockAddrBuf::new();
    let n = sys.recvfrom(fd, buf, flags, out.is_some().then_some(&mut from))?;
    store_addr(&from, out);
    Ok(n)
}

/// `getsockopt(fd, level, name, value, len)` (§15.1): the value's length is written back.
pub fn getsockopt<S: Sys>(sys: &S, fd: Fd, level: i32, name: i32, out: AddrOut<'_>) -> Result<()> {
    let level = u32::try_from(level).map_err(|_| Errno::EINVAL)?;
    let name = u32::try_from(name).map_err(|_| Errno::ENOPROTOOPT)?;
    let n = sys.getsockopt(fd, level, name, out.buf)?;
    *out.len = u32::try_from(n).map_err(|_| Errno::EINVAL)?;
    Ok(())
}

/// `setsockopt(fd, level, name, value, len)` (§15.1). The typed call covers the levels whose
/// options never make the kernel keep a pointer into this process; the others (`SOL_XDP`,
/// `SOL_RDS`, ...) register the caller's memory for later writes and go to the kernel as the
/// caller's raw address (`untyped`).
pub fn setsockopt<S: Sys>(sys: &S, fd: Fd, level: i32, name: i32, value: &[u8], untyped: impl FnOnce() -> Result<()>) -> Result<()> {
    let name = u32::try_from(name).map_err(|_| Errno::ENOPROTOOPT)?;
    match u32::try_from(level).ok().and_then(SockLevel::from_code) {
        Some(l) => sys.setsockopt(fd, l, name, value),
        None => untyped(),
    }
}

/// `isfdtype(fd, type)` (§15.1): 1 if `fd`'s file type is `type` (`S_IFSOCK`, ...), else 0.
pub fn isfdtype<S: Sys>(sys: &S, fd: Fd, ty: i32) -> Result<i64> {
    let st = sys.fstat(fd)?;
    Ok(i64::from(u32::try_from(ty).is_ok_and(|t| st.mode & S_IFMT == t)))
}

/// `sizeof(struct cmsghdr)` on x86-64: `size_t cmsg_len`, `int cmsg_level`, `int cmsg_type`.
const CMSG_HDR: usize = 16;

/// `CMSG_ALIGN`: a multiple of `sizeof(size_t)`.
fn cmsg_align(n: usize) -> Option<usize> {
    n.checked_add(7).map(|v| v & !7)
}

/// The `cmsg_len` of the header at `off`, if the header is inside `area`.
fn cmsg_len(area: &[u8], off: usize) -> Option<usize> {
    let bytes: [u8; 8] = area.get(off..off.checked_add(8)?)?.try_into().ok()?;
    usize::try_from(u64::from_ne_bytes(bytes)).ok()
}

/// `__cmsg_nxthdr(mhdr, cmsg)` (§15.2) over `area`, the message's control buffer, with `cmsg`
/// at offset `off`: the address of the next header, or 0 (NULL) at the end. glibc's rule: a
/// header shorter than `struct cmsghdr` ends the walk, and the next header must fit in the
/// buffer both as a header and with its own (aligned) length.
pub fn cmsg_next(area: &[u8], off: usize) -> usize {
    (|| {
        let len = cmsg_len(area, off)?;
        if len < CMSG_HDR {
            return None;
        }
        let next = off.checked_add(cmsg_align(len)?)?;
        if next.checked_add(CMSG_HDR)? > area.len() {
            return None;
        }
        if next.checked_add(cmsg_align(cmsg_len(area, next)?)?)? > area.len() {
            return None;
        }
        area.as_ptr().addr().checked_add(next)
    })()
    .unwrap_or(0)
}

// ------------------------------------------------------------------ interfaces (§15.4)
//
// `getifaddrs` and `if_nameindex` hand out one caller-owned block each (`freeifaddrs` and
// `if_freenameindex` free it whole): the C structures first, then the texts and socket
// addresses they point to, every piece 8-aligned. The block's address is known before it is
// filled, so pointers are written as `base + offset`.

/// `sizeof(struct ifaddrs)`: `ifa_next`, `ifa_name`, `ifa_flags` (padded), `ifa_addr`,
/// `ifa_netmask`, `ifa_ifu`, `ifa_data`.
const IFADDRS: usize = 56;

/// A block being laid out: its size so far, and when filling, its bytes and address.
struct Block<'a> {
    at: usize,
    out: Option<(&'a mut [u8], usize)>,
}

impl Block<'_> {
    /// Reserve `n` bytes (8-aligned) and return their offset.
    fn take(&mut self, n: usize) -> Option<usize> {
        let off = self.at;
        self.at = off.checked_add(n.checked_next_multiple_of(8)?)?;
        Some(off)
    }

    /// Copy `bytes` to `off` (filling only).
    fn put(&mut self, off: usize, bytes: &[u8]) -> Option<()> {
        if let Some((out, _)) = &mut self.out {
            out.get_mut(off..off.checked_add(bytes.len())?)?.copy_from_slice(bytes);
        }
        Some(())
    }

    /// The address of `off` (0 when only measuring).
    fn addr(&self, off: usize) -> u64 {
        self.out.as_ref().map_or(0, |(_, base)| u64::try_from(base.saturating_add(off)).unwrap_or(0))
    }

    /// Store a pointer to `target` (`None`: NULL) at `off`.
    fn ptr(&mut self, off: usize, target: Option<usize>) -> Option<()> {
        let v = target.map_or(0, |t| self.addr(t));
        self.put(off, &v.to_ne_bytes())
    }

    /// Store `bytes` in a new piece and return its offset.
    fn piece(&mut self, bytes: &[u8]) -> Option<usize> {
        let off = self.take(bytes.len())?;
        self.put(off, bytes)?;
        Some(off)
    }
}

/// `struct sockaddr_ll` for a link and one of its hardware addresses (the address array grows
/// beyond its 8 declared bytes for longer addresses).
fn sockaddr_ll(l: &Link, hw: &[u8]) -> Option<([u8; 44], usize)> {
    let mut s = [0u8; 44];
    s.get_mut(0..2)?.copy_from_slice(&17u16.to_ne_bytes());
    s.get_mut(4..8)?.copy_from_slice(&l.index.to_ne_bytes());
    s.get_mut(8..10)?.copy_from_slice(&l.hatype.to_ne_bytes());
    *s.get_mut(11)? = u8::try_from(hw.len()).ok()?;
    s.get_mut(12..12usize.checked_add(hw.len())?)?.copy_from_slice(hw);
    Some((s, 20usize.max(12usize.checked_add(hw.len())?)))
}

/// `struct sockaddr_in` or `sockaddr_in6` for `a` (4 or 16 bytes), with a scope for IPv6.
fn sockaddr_ip(a: &[u8], scope: u32) -> Option<([u8; 28], usize)> {
    let mut s = [0u8; 28];
    if a.len() == 4 {
        s.get_mut(0..2)?.copy_from_slice(&2u16.to_ne_bytes());
        s.get_mut(4..8)?.copy_from_slice(a);
        Some((s, 16))
    } else {
        s.get_mut(0..2)?.copy_from_slice(&10u16.to_ne_bytes());
        s.get_mut(8..24)?.copy_from_slice(a.get(..16)?);
        s.get_mut(24..28)?.copy_from_slice(&scope.to_ne_bytes());
        Some((s, 28))
    }
}

/// The netmask of a prefix length, as its first `n` bytes.
fn mask(prefix: u8, n: usize) -> [u8; 16] {
    let mut m = [0u8; 16];
    let mut left = u32::from(prefix);
    for b in m.iter_mut().take(n) {
        let bits = left.min(8);
        *b = u8::try_from(0xff00u32.wrapping_shr(bits) & 0xff).unwrap_or(0);
        left = left.saturating_sub(bits);
    }
    m
}

/// Whether an IPv6 address needs its interface as scope: link-local unicast (`fe80::/10`) and
/// interface- or link-local multicast.
fn scoped(a: &[u8]) -> bool {
    match (a.first(), a.get(1)) {
        (Some(0xfe), Some(b)) => b & 0xc0 == 0x80,
        (Some(0xff), Some(b)) => matches!(b & 0x0f, 1 | 2),
        _ => false,
    }
}

/// One `struct ifaddrs` entry's contents.
struct Entry<'a> {
    name: &'a [u8],
    flags: u32,
    addr: Option<&'a [u8]>,
    netmask: Option<&'a [u8]>,
    ifu: Option<&'a [u8]>,
    data: Option<&'a [u8]>,
}

/// Lay out (or, with `out`, fill) the `getifaddrs` list: an `AF_PACKET` entry per link, then
/// one entry per address, in the kernel's order. The size of the block.
fn ifaddrs_layout(links: &[Link], addrs: &[Addr], out: Option<(&mut [u8], usize)>) -> Option<usize> {
    let count = links.len().checked_add(addrs.len())?;
    let mut b = Block { at: 0, out };
    let heads = b.take(IFADDRS.checked_mul(count)?)?;
    let mut k = 0usize;
    let mut entry = |b: &mut Block<'_>, e: Entry<'_>| -> Option<()> {
        let head = heads.checked_add(IFADDRS.checked_mul(k)?)?;
        k = k.checked_add(1)?;
        let mut z = [0u8; IF_NAMESIZE];
        let n = e.name.len().min(IF_NAMESIZE.saturating_sub(1));
        z.get_mut(..n)?.copy_from_slice(e.name.get(..n)?);
        let name_off = b.piece(&z)?;
        let next = if k < count { Some(head.checked_add(IFADDRS)?) } else { None };
        b.ptr(head, next)?;
        b.ptr(head.checked_add(8)?, Some(name_off))?;
        b.put(head.checked_add(16)?, &e.flags.to_ne_bytes())?;
        for (slot, piece) in [(24usize, e.addr), (32, e.netmask), (40, e.ifu), (48, e.data)] {
            let off = match piece {
                Some(p) => Some(b.piece(p)?),
                None => None,
            };
            b.ptr(head.checked_add(slot)?, off)?;
        }
        Some(())
    };
    for l in links {
        let hw = l.address.as_ref().map_or(&[][..], |a| a.as_bytes());
        let (sa, n) = sockaddr_ll(l, hw)?;
        let bc = match &l.broadcast {
            Some(x) => Some(sockaddr_ll(l, x.as_bytes())?),
            None => None,
        };
        let e = Entry {
            name: l.name.as_bytes(),
            flags: l.flags,
            addr: Some(sa.get(..n)?),
            netmask: None,
            ifu: bc.as_ref().and_then(|(s, n)| s.get(..*n)),
            data: l.stats.as_ref().map(|s| s.as_bytes()),
        };
        entry(&mut b, e)?;
    }
    for a in addrs {
        let link = links.iter().find(|l| l.index == a.index);
        let name = match (&a.label, link) {
            (Some(lb), _) => lb.as_bytes(),
            (None, Some(l)) => l.name.as_bytes(),
            (None, None) => &[],
        };
        let n = a.addr_len();
        let ip = |x: Option<[u8; 16]>| -> Option<([u8; 28], usize)> {
            let x = x?;
            let v = x.get(..n)?;
            sockaddr_ip(v, if scoped(v) { a.index } else { 0 })
        };
        let own = ip(a.own());
        let other = ip(a.other());
        let m = mask(a.prefix, n);
        let ms = sockaddr_ip(m.get(..n)?, 0)?;
        let e = Entry {
            name,
            flags: link.map_or(0, |l| l.flags),
            addr: own.as_ref().and_then(|(s, l)| s.get(..*l)),
            netmask: ms.0.get(..ms.1),
            ifu: other.as_ref().and_then(|(s, l)| s.get(..*l)),
            data: None,
        };
        entry(&mut b, e)?;
    }
    Some(b.at)
}

/// The size of the `getifaddrs` block for these links and addresses.
pub fn ifaddrs_size(links: &[Link], addrs: &[Addr]) -> Option<usize> {
    ifaddrs_layout(links, addrs, None)
}

/// Fill the `getifaddrs` block `out` (from [`ifaddrs_size`]), which lives at address `base`.
pub fn ifaddrs_fill(links: &[Link], addrs: &[Addr], out: &mut [u8], base: usize) -> Option<()> {
    ifaddrs_layout(links, addrs, Some((out, base))).map(|_| ())
}

/// `sizeof(struct if_nameindex)`: `if_index` (padded), `if_name`.
const NAMEINDEX: usize = 16;

/// The size of the `if_nameindex` block: the entries, the zero entry, the names.
pub fn nameindex_size(links: &[Link]) -> Option<usize> {
    let n = links.len();
    n.checked_add(1)?.checked_mul(NAMEINDEX)?.checked_add(n.checked_mul(IF_NAMESIZE)?)
}

/// Fill the `if_nameindex` block `out` (zeroed, from [`nameindex_size`]) at address `base`.
pub fn nameindex_fill(links: &[Link], out: &mut [u8], base: usize) -> Option<()> {
    let names = links.len().checked_add(1)?.checked_mul(NAMEINDEX)?;
    for (i, l) in links.iter().enumerate() {
        let e = i.checked_mul(NAMEINDEX)?;
        let name = names.checked_add(i.checked_mul(IF_NAMESIZE)?)?;
        out.get_mut(e..e.checked_add(4)?)?.copy_from_slice(&l.index.to_ne_bytes());
        let p = u64::try_from(base.checked_add(name)?).ok()?;
        out.get_mut(e.checked_add(8)?..e.checked_add(16)?)?.copy_from_slice(&p.to_ne_bytes());
        let t = l.name.as_bytes();
        out.get_mut(name..name.checked_add(t.len())?)?.copy_from_slice(t);
    }
    Some(())
}

// ------------------------------------------------------------------ source filters (§15.6)

/// `IP_MSFILTER` (level `IPPROTO_IP`).
const IP_MSFILTER: u32 = 41;
/// `MCAST_MSFILTER` (levels `IPPROTO_IP` and `IPPROTO_IPV6`).
const MCAST_MSFILTER: u32 = 48;
/// `sizeof(struct sockaddr_storage)`.
const STORAGE: usize = 128;
/// The offset of `gf_slist` in `struct group_filter` (`gf_interface`, padding, `gf_group`,
/// `gf_fmode`, `gf_numsrc`).
const GF_SLIST: usize = 144;

/// A source filter: its mode (`MCAST_INCLUDE` 1, `MCAST_EXCLUDE` 0) and the number of sources
/// the kernel holds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Filter {
    /// `fmode`.
    pub mode: u32,
    /// The kernel's number of sources (the sources copied are at most the caller's capacity).
    pub numsrc: u32,
}

fn word(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_ne_bytes(b.get(at..at.checked_add(4)?)?.try_into().ok()?))
}

/// `struct ip_msfilter` for `group` on `iface` with room for `n` sources (at least one).
fn ip_msfilter(group: [u8; 4], iface: [u8; 4], mode: u32, n: usize) -> Result<TryVec<u8>> {
    let mut v = TryVec::new();
    v.resize(n.max(1).checked_mul(4).and_then(|x| x.checked_add(16)).ok_or(Errno::ENOMEM)?, 0)?;
    let count = u32::try_from(n).map_err(|_| Errno::EINVAL)?;
    for (at, bytes) in [(0usize, group), (4, iface), (8, mode.to_ne_bytes()), (12, count.to_ne_bytes())] {
        v.get_mut(at..at.saturating_add(4)).ok_or(Errno::EINVAL)?.copy_from_slice(&bytes);
    }
    Ok(v)
}

/// `getipv4sourcefilter(s, interface, group, &fmode, &numsrc, slist)` (§15.6; RFC 3678 §5.2.1):
/// the filter, with up to `slist.len() / 4` sources copied.
pub fn get_ipv4_filter<S: Sys>(sys: &S, fd: Fd, iface: [u8; 4], group: [u8; 4], slist: &mut [u8]) -> Result<Filter> {
    let cap = slist.len() / 4;
    let mut buf = ip_msfilter(group, iface, 0, cap)?;
    sys.getsockopt(fd, 0, IP_MSFILTER, &mut buf)?;
    let f = Filter { mode: word(&buf, 8).ok_or(Errno::EINVAL)?, numsrc: word(&buf, 12).ok_or(Errno::EINVAL)? };
    let n = usize::try_from(f.numsrc).unwrap_or(usize::MAX).min(cap).checked_mul(4).ok_or(Errno::EINVAL)?;
    slist.get_mut(..n).ok_or(Errno::EINVAL)?.copy_from_slice(buf.get(16..16usize.saturating_add(n)).ok_or(Errno::EINVAL)?);
    Ok(f)
}

/// `setipv4sourcefilter(s, interface, group, fmode, numsrc, slist)` (§15.6).
pub fn set_ipv4_filter<S: Sys>(sys: &S, fd: Fd, iface: [u8; 4], group: [u8; 4], mode: u32, slist: &[u8]) -> Result<()> {
    let n = slist.len() / 4;
    let mut buf = ip_msfilter(group, iface, mode, n)?;
    buf.get_mut(16..16usize.saturating_add(slist.len())).ok_or(Errno::EINVAL)?.copy_from_slice(slist);
    if n == 0 {
        buf.truncate(16);
    }
    sys.setsockopt(fd, SockLevel::Ip, IP_MSFILTER, &buf)
}

/// The level of a group's family (`EINVAL` for others, as the reference).
fn group_level(group: &[u8]) -> Result<SockLevel> {
    match group.get(..2).map(|f| u16::from_ne_bytes([f.first().copied().unwrap_or(0), f.get(1).copied().unwrap_or(0)])) {
        Some(2) => Ok(SockLevel::Ip),
        Some(10) => Ok(SockLevel::Ipv6),
        _ => Err(Errno::EINVAL),
    }
}

/// `struct group_filter` with room for `n` sources (at least one).
fn group_filter(iface: u32, group: &[u8], mode: u32, n: usize) -> Result<TryVec<u8>> {
    if group.len() > STORAGE {
        return Err(Errno::EINVAL);
    }
    let mut v = TryVec::new();
    v.resize(n.max(1).checked_mul(STORAGE).and_then(|x| x.checked_add(GF_SLIST)).ok_or(Errno::ENOMEM)?, 0)?;
    v.get_mut(0..4).ok_or(Errno::EINVAL)?.copy_from_slice(&iface.to_ne_bytes());
    v.get_mut(8..8usize.saturating_add(group.len())).ok_or(Errno::EINVAL)?.copy_from_slice(group);
    v.get_mut(136..140).ok_or(Errno::EINVAL)?.copy_from_slice(&mode.to_ne_bytes());
    v.get_mut(140..144).ok_or(Errno::EINVAL)?.copy_from_slice(&u32::try_from(n).map_err(|_| Errno::EINVAL)?.to_ne_bytes());
    Ok(v)
}

/// `getsourcefilter(s, interface, group, grouplen, &fmode, &numsrc, slist)` (§15.6; RFC 3678
/// §5.2.2): sources are `struct sockaddr_storage`; up to `slist.len() / 128` are copied.
pub fn get_filter<S: Sys>(sys: &S, fd: Fd, iface: u32, group: &[u8], slist: &mut [u8]) -> Result<Filter> {
    let level = group_level(group)?.code();
    let cap = slist.len() / STORAGE;
    let mut buf = group_filter(iface, group, 0, cap)?;
    sys.getsockopt(fd, level, MCAST_MSFILTER, &mut buf)?;
    let f = Filter { mode: word(&buf, 136).ok_or(Errno::EINVAL)?, numsrc: word(&buf, 140).ok_or(Errno::EINVAL)? };
    let n = usize::try_from(f.numsrc).unwrap_or(usize::MAX).min(cap).checked_mul(STORAGE).ok_or(Errno::EINVAL)?;
    slist.get_mut(..n).ok_or(Errno::EINVAL)?.copy_from_slice(buf.get(GF_SLIST..GF_SLIST.saturating_add(n)).ok_or(Errno::EINVAL)?);
    Ok(f)
}

/// `setsourcefilter(s, interface, group, grouplen, fmode, numsrc, slist)` (§15.6).
pub fn set_filter<S: Sys>(sys: &S, fd: Fd, iface: u32, group: &[u8], mode: u32, slist: &[u8]) -> Result<()> {
    let level = group_level(group)?;
    let n = slist.len() / STORAGE;
    let mut buf = group_filter(iface, group, mode, n)?;
    buf.get_mut(GF_SLIST..GF_SLIST.saturating_add(slist.len())).ok_or(Errno::EINVAL)?.copy_from_slice(slist);
    if n == 0 {
        buf.truncate(GF_SLIST);
    }
    sys.setsockopt(fd, level, MCAST_MSFILTER, &buf)
}
