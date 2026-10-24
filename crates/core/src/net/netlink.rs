//! `NETLINK_ROUTE` messages (RFC 3549; the kernel's `rtnetlink` UAPI, ⚑ KERN): building the
//! link and address requests, walking the replies, and one request-reply transaction over a
//! private socket (D-15.3: interface enumeration uses netlink only).
//!
//! A message is a 16-byte header (`nlmsg_len`, `nlmsg_type`, `nlmsg_flags`, `nlmsg_seq`,
//! `nlmsg_pid`) and a body, padded to 4 bytes. Route bodies are a fixed structure
//! (`ifinfomsg`, 16 bytes; `ifaddrmsg`, 8 bytes) followed by attributes (`rtattr`: a 2-byte
//! length including its 4-byte header, a 2-byte type, the payload, padded to 4).

use liella_kernel::sys::Sys;
use liella_kernel::uapi::{AF_NETLINK, MSG_PEEK, MSG_TRUNC, NETLINK_ROUTE, NLMSG_DONE, NLMSG_ERROR, SOCK_CLOEXEC, SOCK_RAW};

use crate::error::{Errno, Result};
use crate::mem::TryVec;

/// `RTM_NEWLINK`.
pub const RTM_NEWLINK: u16 = 16;
/// `RTM_GETLINK`.
pub const RTM_GETLINK: u16 = 18;
/// `RTM_NEWADDR`.
pub const RTM_NEWADDR: u16 = 20;
/// `RTM_GETADDR`.
pub const RTM_GETADDR: u16 = 22;
/// `RTM_NEWROUTE`.
pub const RTM_NEWROUTE: u16 = 24;
/// `RTM_GETROUTE`.
pub const RTM_GETROUTE: u16 = 26;
/// `NLM_F_REQUEST`.
pub const NLM_F_REQUEST: u16 = 1;
/// `NLM_F_DUMP` (`NLM_F_ROOT | NLM_F_MATCH`).
pub const NLM_F_DUMP: u16 = 0x300;
/// The header size.
pub const HDR: usize = 16;

/// `NLMSG_ALIGN`.
pub fn align4(n: usize) -> Option<usize> {
    n.checked_next_multiple_of(4)
}

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_ne_bytes([*b.get(at)?, *b.get(at.checked_add(1)?)?]))
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let s = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_ne_bytes([*s.first()?, *s.get(1)?, *s.get(2)?, *s.get(3)?]))
}

/// A request: header, `body`, then one optional attribute. Its bytes, in `out`.
pub fn request(ty: u16, flags: u16, seq: u32, body: &[u8], attr: Option<(u16, &[u8])>) -> Result<TryVec<u8>> {
    let body_len = align4(body.len()).ok_or(Errno::EINVAL)?;
    let attr_len = match attr {
        Some((_, p)) => align4(p.len().checked_add(4).ok_or(Errno::EINVAL)?).ok_or(Errno::EINVAL)?,
        None => 0,
    };
    let total = HDR.checked_add(body_len).and_then(|n| n.checked_add(attr_len)).ok_or(Errno::EINVAL)?;
    let mut m = TryVec::new();
    m.resize(total, 0)?;
    let t = u32::try_from(total).map_err(|_| Errno::EINVAL)?;
    let (tb, yb, fb, sb, pb) = (t.to_ne_bytes(), ty.to_ne_bytes(), flags.to_ne_bytes(), seq.to_ne_bytes(), 0u32.to_ne_bytes());
    let hdr: [&[u8]; 5] = [&tb, &yb, &fb, &sb, &pb];
    let mut at = 0usize;
    for part in hdr.iter().copied().chain([body]) {
        let end = at.checked_add(part.len()).ok_or(Errno::EINVAL)?;
        m.get_mut(at..end).ok_or(Errno::EINVAL)?.copy_from_slice(part);
        at = end;
    }
    if let Some((aty, payload)) = attr {
        let at = HDR.checked_add(body_len).ok_or(Errno::EINVAL)?;
        let alen = u16::try_from(payload.len().checked_add(4).ok_or(Errno::EINVAL)?).map_err(|_| Errno::EINVAL)?;
        let head = [alen.to_ne_bytes(), aty.to_ne_bytes()];
        for (i, part) in head.iter().enumerate() {
            let p = at.checked_add(i.checked_mul(2).ok_or(Errno::EINVAL)?).ok_or(Errno::EINVAL)?;
            m.get_mut(p..p.checked_add(2).ok_or(Errno::EINVAL)?).ok_or(Errno::EINVAL)?.copy_from_slice(part);
        }
        let p = at.checked_add(4).ok_or(Errno::EINVAL)?;
        m.get_mut(p..p.checked_add(payload.len()).ok_or(Errno::EINVAL)?).ok_or(Errno::EINVAL)?.copy_from_slice(payload);
    }
    Ok(m)
}

/// One message of a reply.
#[derive(Clone, Copy, Debug)]
pub struct Msg<'a> {
    /// `nlmsg_type`.
    pub ty: u16,
    /// `nlmsg_seq`.
    pub seq: u32,
    /// The body (without padding beyond `nlmsg_len`).
    pub body: &'a [u8],
}

/// The messages in a datagram; a malformed header ends the walk.
pub fn messages(buf: &[u8]) -> impl Iterator<Item = Msg<'_>> {
    let mut at = 0usize;
    core::iter::from_fn(move || {
        let len = usize::try_from(u32_at(buf, at)?).ok()?;
        if len < HDR {
            return None;
        }
        let msg = Msg { ty: u16_at(buf, at.checked_add(4)?)?, seq: u32_at(buf, at.checked_add(8)?)?, body: buf.get(at.checked_add(HDR)?..at.checked_add(len)?)? };
        at = at.checked_add(align4(len)?)?;
        Some(msg)
    })
}

/// The attributes in `buf`; a malformed one ends the walk.
pub fn attrs(buf: &[u8]) -> impl Iterator<Item = (u16, &[u8])> {
    let mut at = 0usize;
    core::iter::from_fn(move || {
        let len = usize::from(u16_at(buf, at)?);
        if len < 4 {
            return None;
        }
        let ty = u16_at(buf, at.checked_add(2)?)?;
        let payload = buf.get(at.checked_add(4)?..at.checked_add(len)?)?;
        at = at.checked_add(align4(len)?)?;
        Some((ty, payload))
    })
}

/// The error a `NLMSG_ERROR` body carries (0: an acknowledgement).
fn error_of(body: &[u8]) -> Option<i32> {
    u32_at(body, 0).map(|v| i32::from_ne_bytes(v.to_ne_bytes()))
}

/// Send `req` on a new `NETLINK_ROUTE` socket and hand each reply message to `each` until the
/// end of a dump (`dump`), or until the first message otherwise. A `NLMSG_ERROR` with a
/// non-zero code is that error. Replies not from the kernel (port 0) or with another sequence
/// number are ignored.
pub fn transact<S: Sys>(sys: &S, req: &[u8], seq: u32, dump: bool, mut each: impl FnMut(Msg<'_>) -> Result<()>) -> Result<()> {
    let fd = sys.socket(AF_NETLINK, SOCK_RAW | SOCK_CLOEXEC, NETLINK_ROUTE)?;
    let r = (|| {
        // struct sockaddr_nl: family, pad, pid 0 (the kernel), groups 0.
        let mut kernel = [0u8; 12];
        kernel.get_mut(..2).ok_or(Errno::EINVAL)?.copy_from_slice(&u16::try_from(AF_NETLINK).map_err(|_| Errno::EINVAL)?.to_ne_bytes());
        sys.sendto(fd, req, 0, Some(&kernel))?;
        let mut buf: TryVec<u8> = TryVec::new();
        buf.resize(32 * 1024, 0)?;
        loop {
            let want = sys.recvfrom(fd, &mut buf, MSG_PEEK | MSG_TRUNC, None)?;
            if want > buf.len() {
                buf.resize(want, 0)?;
            }
            let mut from = liella_kernel::uapi::SockAddrBuf::new();
            let n = sys.recvfrom(fd, &mut buf, 0, Some(&mut from))?;
            if u32_at(from.bytes(), 4).unwrap_or(0) != 0 {
                continue;
            }
            for m in messages(buf.get(..n).unwrap_or(&[])) {
                if m.seq != seq {
                    continue;
                }
                match m.ty {
                    NLMSG_DONE => return Ok(()),
                    NLMSG_ERROR => {
                        return match error_of(m.body) {
                            Some(0) => Ok(()),
                            Some(e) => Err(Errno::new(e.checked_neg().unwrap_or(0)).unwrap_or(Errno::EIO)),
                            None => Err(Errno::EIO),
                        };
                    }
                    _ => {
                        each(m)?;
                        if !dump {
                            return Ok(());
                        }
                    }
                }
            }
        }
    })();
    let _ = sys.close(fd);
    r
}
