//! Interfaces and their addresses, from `NETLINK_ROUTE` (spec §15.4; D-15.3).
//!
//! * [`links`]: every interface (`RTM_GETLINK` dump): index, hardware type, flags (`IFF_*`, as
//!   the kernel reports them, `IFF_LOWER_UP` included), name, hardware and broadcast addresses,
//!   and the `IFLA_STATS` block (`struct rtnl_link_stats`, which `getifaddrs` hands out as
//!   `ifa_data`).
//! * [`addrs`]: every address (`RTM_GETADDR` dump, IPv4 then IPv6 as the kernel lists them).
//! * [`index_of`] and [`name_of`]: one link, asked for by name or by index.

use liella_kernel::sys::Sys;

use super::netlink::{self, NLM_F_DUMP, NLM_F_REQUEST, RTM_GETADDR, RTM_GETLINK, RTM_GETROUTE, RTM_NEWADDR, RTM_NEWLINK, RTM_NEWROUTE};
use super::Text;
use crate::error::{Errno, Result};
use crate::mem::TryVec;

/// `IF_NAMESIZE` (the kernel's `IFNAMSIZ`, ⚑ KERN).
pub const IF_NAMESIZE: usize = 16;

/// An interface name (at most 15 bytes).
pub type Name = Text<IF_NAMESIZE>;
/// A hardware address (`MAX_ADDR_LEN`, 32 bytes).
pub type HwAddr = Text<32>;
/// The statistics block (`struct rtnl_link_stats` is 96 bytes in Linux 6.x).
pub type Stats = Text<256>;

/// One interface.
#[derive(Clone, Debug, Default)]
pub struct Link {
    /// `ifi_index`.
    pub index: u32,
    /// `ifi_type` (`ARPHRD_*`).
    pub hatype: u16,
    /// `ifi_flags`.
    pub flags: u32,
    /// `IFLA_IFNAME`.
    pub name: Name,
    /// `IFLA_ADDRESS`, if any.
    pub address: Option<HwAddr>,
    /// `IFLA_BROADCAST`, if any.
    pub broadcast: Option<HwAddr>,
    /// `IFLA_STATS`, if any.
    pub stats: Option<Stats>,
}

/// One address of an interface.
#[derive(Clone, Copy, Debug, Default)]
pub struct Addr {
    /// `AF_INET` or `AF_INET6`.
    pub family: u8,
    /// The prefix length.
    pub prefix: u8,
    /// The interface index.
    pub index: u32,
    /// `IFA_LOCAL`: the local address of a point-to-point link (IPv4: of every link).
    pub local: Option<[u8; 16]>,
    /// `IFA_ADDRESS`: the address, or the peer's when `local` is given.
    pub address: Option<[u8; 16]>,
    /// `IFA_BROADCAST`.
    pub broadcast: Option<[u8; 16]>,
    /// `IFA_LABEL` (IPv4 aliases such as `eth0:1`).
    pub label: Option<Name>,
}

impl Addr {
    /// The interface's own address.
    pub fn own(&self) -> Option<[u8; 16]> {
        self.local.or(self.address)
    }

    /// The broadcast address, or the peer's address on a link that names one.
    pub fn other(&self) -> Option<[u8; 16]> {
        self.broadcast.or(if self.local.is_some() { self.address } else { None })
    }

    /// The length of the family's addresses (4 or 16).
    pub fn addr_len(&self) -> usize {
        if u32::from(self.family) == liella_kernel::uapi::AF_INET6 { 16 } else { 4 }
    }
}

const IFLA_ADDRESS: u16 = 1;
const IFLA_BROADCAST: u16 = 2;
const IFLA_IFNAME: u16 = 3;
const IFLA_STATS: u16 = 7;
const IFA_ADDRESS: u16 = 1;
const IFA_LOCAL: u16 = 2;
const IFA_LABEL: u16 = 3;
const IFA_BROADCAST: u16 = 4;

/// Request sequence numbers: any value serves (each request has its own socket).
const SEQ: u32 = 1;

fn text<const N: usize>(payload: &[u8]) -> Text<N> {
    let mut t = Text::new();
    let end = payload.iter().position(|&c| c == 0).unwrap_or(payload.len());
    t.push_all(payload.get(..end).unwrap_or(&[]));
    t
}

fn raw<const N: usize>(payload: &[u8]) -> Text<N> {
    let mut t = Text::new();
    t.push_all(payload);
    t
}

/// A link from a `RTM_NEWLINK` body (`ifinfomsg` then attributes).
pub fn parse_link(body: &[u8]) -> Option<Link> {
    let head = body.get(..16)?;
    let mut l = Link {
        hatype: u16::from_ne_bytes([*head.get(2)?, *head.get(3)?]),
        index: u32::from_ne_bytes([*head.get(4)?, *head.get(5)?, *head.get(6)?, *head.get(7)?]),
        flags: u32::from_ne_bytes([*head.get(8)?, *head.get(9)?, *head.get(10)?, *head.get(11)?]),
        ..Link::default()
    };
    for (ty, p) in netlink::attrs(body.get(16..)?) {
        match ty {
            IFLA_IFNAME => l.name = text(p),
            IFLA_ADDRESS => l.address = Some(raw(p)),
            IFLA_BROADCAST => l.broadcast = Some(raw(p)),
            IFLA_STATS => l.stats = Some(raw(p)),
            _ => {}
        }
    }
    Some(l)
}

/// An address from a `RTM_NEWADDR` body (`ifaddrmsg` then attributes).
pub fn parse_addr(body: &[u8]) -> Option<Addr> {
    let head = body.get(..8)?;
    let mut a = Addr { family: *head.first()?, prefix: *head.get(1)?, index: u32::from_ne_bytes([*head.get(4)?, *head.get(5)?, *head.get(6)?, *head.get(7)?]), ..Addr::default() };
    let n = a.addr_len();
    let ip = |p: &[u8]| -> Option<[u8; 16]> {
        let mut v = [0u8; 16];
        v.get_mut(..n)?.copy_from_slice(p.get(..n)?);
        Some(v)
    };
    for (ty, p) in netlink::attrs(body.get(8..)?) {
        match ty {
            IFA_ADDRESS => a.address = ip(p),
            IFA_LOCAL => a.local = ip(p),
            IFA_BROADCAST => a.broadcast = ip(p),
            IFA_LABEL => a.label = Some(text(p)),
            _ => {}
        }
    }
    Some(a)
}

/// `struct ifinfomsg` asking for `index` (0: any).
fn ifinfomsg(index: u32) -> [u8; 16] {
    let mut b = [0u8; 16];
    if let Some(d) = b.get_mut(4..8) {
        d.copy_from_slice(&index.to_ne_bytes());
    }
    b
}

/// Every interface.
pub fn links<S: Sys>(sys: &S) -> Result<TryVec<Link>> {
    let req = netlink::request(RTM_GETLINK, NLM_F_REQUEST | NLM_F_DUMP, SEQ, &ifinfomsg(0), None)?;
    let mut out = TryVec::new();
    netlink::transact(sys, &req, SEQ, true, |m| {
        if m.ty == RTM_NEWLINK {
            if let Some(l) = parse_link(m.body) {
                out.push(l)?;
            }
        }
        Ok(())
    })?;
    Ok(out)
}

/// Every IPv4 and IPv6 address.
pub fn addrs<S: Sys>(sys: &S) -> Result<TryVec<Addr>> {
    let req = netlink::request(RTM_GETADDR, NLM_F_REQUEST | NLM_F_DUMP, SEQ, &[0u8; 8], None)?;
    let mut out = TryVec::new();
    netlink::transact(sys, &req, SEQ, true, |m| {
        if m.ty == RTM_NEWADDR {
            if let Some(a) = parse_addr(m.body) {
                let fam = u32::from(a.family);
                if fam == liella_kernel::uapi::AF_INET || fam == liella_kernel::uapi::AF_INET6 {
                    out.push(a)?;
                }
            }
        }
        Ok(())
    })?;
    Ok(out)
}

/// One link, asked for by index or (`index` 0) by name.
fn one<S: Sys>(sys: &S, index: u32, name: Option<&[u8]>) -> Result<Link> {
    let mut z = [0u8; IF_NAMESIZE];
    let attr = match name {
        Some(n) => {
            z.get_mut(..n.len()).ok_or(Errno::ENODEV)?.copy_from_slice(n);
            Some((IFLA_IFNAME, z.get(..=n.len()).ok_or(Errno::ENODEV)?))
        }
        None => None,
    };
    let req = netlink::request(RTM_GETLINK, NLM_F_REQUEST, SEQ, &ifinfomsg(index), attr)?;
    let mut found = None;
    netlink::transact(sys, &req, SEQ, false, |m| {
        if m.ty == RTM_NEWLINK {
            found = parse_link(m.body);
        }
        Ok(())
    })?;
    found.ok_or(Errno::ENODEV)
}

/// The route the kernel would use to reach a destination.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Route {
    /// `RTA_OIF`: the outgoing interface (0 when not given).
    pub oif: u32,
    /// `RTA_PREFSRC`: the source address the kernel prefers, if it names one.
    pub prefsrc: Option<[u8; 16]>,
}

const RTA_DST: u16 = 1;
const RTA_OIF: u16 = 4;
const RTA_PREFSRC: u16 = 7;

/// The kernel's route to `dest` (4 or 16 bytes): `RTM_GETROUTE` for one address, the question
/// `connect` on a datagram socket answers (spec §16.2), asked without a socket of that family.
/// `Ok(None)` when there is no route (`ENETUNREACH`, `EHOSTUNREACH`, `ENETDOWN`, `EINVAL`).
pub fn route<S: Sys>(sys: &S, dest: &[u8]) -> Result<Option<Route>> {
    let (family, bits) = match dest.len() {
        4 => (liella_kernel::uapi::AF_INET, 32u8),
        16 => (liella_kernel::uapi::AF_INET6, 128u8),
        _ => return Err(Errno::EINVAL),
    };
    // struct rtmsg: family, dst_len, src_len, tos, table, protocol, scope, type, flags.
    let mut rtm = [0u8; 12];
    if let Some(f) = rtm.first_mut() {
        *f = u8::try_from(family).map_err(|_| Errno::EINVAL)?;
    }
    if let Some(l) = rtm.get_mut(1) {
        *l = bits;
    }
    let req = netlink::request(RTM_GETROUTE, NLM_F_REQUEST, SEQ, &rtm, Some((RTA_DST, dest)))?;
    let mut found = None;
    let n = dest.len();
    let r = netlink::transact(sys, &req, SEQ, false, |m| {
        if m.ty == RTM_NEWROUTE {
            let mut route = Route::default();
            for (ty, p) in netlink::attrs(m.body.get(12..).unwrap_or(&[])) {
                match ty {
                    RTA_OIF => route.oif = p.get(..4).and_then(|b| <[u8; 4]>::try_from(b).ok()).map_or(0, u32::from_ne_bytes),
                    RTA_PREFSRC => {
                        let mut v = [0u8; 16];
                        if let (Some(d), Some(s)) = (v.get_mut(..n), p.get(..n)) {
                            d.copy_from_slice(s);
                            route.prefsrc = Some(v);
                        }
                    }
                    _ => {}
                }
            }
            found = Some(route);
        }
        Ok(())
    });
    match r {
        Ok(()) => Ok(found),
        Err(e) if e == Errno::ENETUNREACH || e == Errno::EHOSTUNREACH || e == Errno::ENETDOWN || e == Errno::EINVAL => Ok(None),
        Err(e) => Err(e),
    }
}

/// `if_nametoindex(name)`: the index, `ENODEV` for no such interface (a name that cannot be an
/// interface's, empty or of `IF_NAMESIZE` bytes or more, included).
pub fn index_of<S: Sys>(sys: &S, name: &[u8]) -> Result<u32> {
    if name.is_empty() || name.len() >= IF_NAMESIZE {
        return Err(Errno::ENODEV);
    }
    match one(sys, 0, Some(name)) {
        Ok(l) if l.name.as_bytes() == name => Ok(l.index),
        Ok(_) => Err(Errno::ENODEV),
        Err(e) if e == Errno::EINVAL || e == Errno::ENODEV => Err(Errno::ENODEV),
        Err(e) => Err(e),
    }
}

/// `if_indextoname(index)`: the name, `ENXIO` for no such interface (the man page's error).
pub fn name_of<S: Sys>(sys: &S, index: u32) -> Result<Name> {
    let i = i32::try_from(index).map_err(|_| Errno::ENXIO)?;
    if i <= 0 {
        return Err(Errno::ENXIO);
    }
    match one(sys, index, None) {
        Ok(l) => Ok(l.name),
        Err(e) if e == Errno::ENODEV || e == Errno::EINVAL => Err(Errno::ENXIO),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn parse_bodies() {
        let mut body = ifinfomsg(7).to_vec();
        body[2..4].copy_from_slice(&772u16.to_ne_bytes());
        body[8..12].copy_from_slice(&0x10049u32.to_ne_bytes());
        body.extend_from_slice(&7u16.to_ne_bytes());
        body.extend_from_slice(&IFLA_IFNAME.to_ne_bytes());
        body.extend_from_slice(b"lo\0\0");
        let l = parse_link(&body).unwrap();
        assert_eq!((l.index, l.hatype, l.flags, l.name.as_bytes()), (7, 772, 0x10049, &b"lo"[..]));
        let mut a = vec![2u8, 8, 0, 0];
        a.extend_from_slice(&3u32.to_ne_bytes());
        for ty in [IFA_ADDRESS, IFA_LOCAL] {
            a.extend_from_slice(&8u16.to_ne_bytes());
            a.extend_from_slice(&ty.to_ne_bytes());
            a.extend_from_slice(&[127, 0, 0, 1]);
        }
        let x = parse_addr(&a).unwrap();
        assert_eq!((x.family, x.prefix, x.index), (2, 8, 3));
        assert_eq!(x.own().unwrap()[..4], [127, 0, 0, 1]);
        assert_eq!(x.other().unwrap()[..4], [127, 0, 0, 1], "the peer slot when IFA_LOCAL is given");
    }
}
