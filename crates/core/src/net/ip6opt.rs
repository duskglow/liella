//! IPv6 hop-by-hop and destination options, and type-0 routing headers, in caller buffers
//! (spec §15.5; RFC 3542 §10 and §7).
//!
//! An extension header is a next-header byte, a length byte (the header's size in 8-octet
//! units, not counting the first), then type-length-value options. Pad1 (type 0) is one zero
//! byte; PadN (type 1) is a type, a length and that many zero bytes. Builders given no buffer
//! (`None`) only compute the offsets. A routing header of type 0 holds a fixed 8-byte part
//! (next header, length = 2 × segments, type 0, segments left, 4 reserved bytes) and then the
//! segment addresses.
//!
//! Choices where RFC 3542 is silent, as the reference behaves (netaddr.md and probes): an
//! option's alignment must not exceed its length (so a length of 0 is refused);
//! `inet6_opt_next` and `inet6_opt_find` start after the 2-byte header when given offset 0, and
//! reject offset 1; `inet6_rth_add` beyond the declared segments fails; `inet6_rth_reverse`
//! and `inet6_rth_getaddr` go by the declared segments, however many were added. Liella also
//! refuses a "segments left" at or beyond the declared count, so the header is never written
//! past its length.

/// `IPV6_RTHDR_TYPE_0`.
pub const RTHDR_TYPE_0: i32 = 0;

/// Write the padding of `n` bytes at `at` (Pad1 or PadN).
fn pad(buf: &mut [u8], at: usize, n: usize) -> Option<()> {
    let area = buf.get_mut(at..at.checked_add(n)?)?;
    match n {
        0 => {}
        1 => area.fill(0),
        _ => {
            area.fill(0);
            *area.first_mut()? = 1;
            *area.get_mut(1)? = u8::try_from(n.checked_sub(2)?).ok()?;
        }
    }
    Some(())
}

/// `inet6_opt_init(extbuf, extlen)`: the header's size (2), after writing its length byte; a
/// buffer whose size is not a multiple of 8 between 8 and 2048 is an error.
pub fn opt_init(buf: Option<&mut [u8]>) -> Option<i32> {
    if let Some(b) = buf {
        let len = b.len();
        if len == 0 || len % 8 != 0 || len > 2048 {
            return None;
        }
        *b.get_mut(1)? = u8::try_from((len / 8).checked_sub(1)?).ok()?;
    }
    Some(2)
}

/// `inet6_opt_append(extbuf, extlen, offset, type, len, align, &databuf)`: the offset after the
/// option, and the offset of its data. Types 0 and 1 (the pads) are refused, the length is
/// 0–255, the alignment 1, 2, 4 or 8 and not beyond the length; the option's data starts at a
/// multiple of the alignment, after Pad1 or PadN.
pub fn opt_append(buf: Option<&mut [u8]>, offset: i32, ty: u8, len: i32, align: i32) -> Option<(i32, usize)> {
    let offset = usize::try_from(offset).ok().filter(|&o| o >= 2)?;
    let len = usize::try_from(len).ok().filter(|&l| l <= 255)?;
    let align = usize::try_from(align).ok().filter(|a| [1, 2, 4, 8].contains(a))?;
    if ty < 2 || align > len {
        return None;
    }
    let data = offset.checked_add(2)?.checked_next_multiple_of(align)?;
    let start = data.checked_sub(2)?;
    let end = data.checked_add(len)?;
    if let Some(b) = buf {
        if end > b.len() {
            return None;
        }
        pad(b, offset, start.checked_sub(offset)?)?;
        *b.get_mut(start)? = ty;
        *b.get_mut(start.checked_add(1)?)? = u8::try_from(len).ok()?;
    }
    Some((i32::try_from(end).ok()?, data))
}

/// `inet6_opt_finish(extbuf, extlen, offset)`: the header's final size, a multiple of 8, after
/// padding.
pub fn opt_finish(buf: Option<&mut [u8]>, offset: i32) -> Option<i32> {
    let offset = usize::try_from(offset).ok().filter(|&o| o >= 2)?;
    let end = offset.checked_next_multiple_of(8)?;
    if let Some(b) = buf {
        if end > b.len() {
            return None;
        }
        pad(b, offset, end.checked_sub(offset)?)?;
    }
    i32::try_from(end).ok()
}

/// One option of an extension header: its type, its data's offset and length, and the offset
/// after it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Opt {
    /// The option type.
    pub ty: u8,
    /// Offset of the option's data.
    pub data: usize,
    /// Length of the data.
    pub len: usize,
    /// Offset after the option.
    pub next: usize,
}

/// The option at `at`, if it lies inside `buf` (Pad1 has no length byte).
fn opt_at(buf: &[u8], at: usize) -> Option<Opt> {
    let ty = *buf.get(at)?;
    if ty == 0 {
        let next = at.checked_add(1)?;
        return Some(Opt { ty, data: next, len: 0, next });
    }
    let len = usize::from(*buf.get(at.checked_add(1)?)?);
    let data = at.checked_add(2)?;
    let next = data.checked_add(len)?;
    (next <= buf.len()).then_some(Opt { ty, data, len, next })
}

fn start(offset: i32) -> Option<usize> {
    match offset {
        0 => Some(2),
        o => usize::try_from(o).ok().filter(|&o| o >= 2),
    }
}

/// `inet6_opt_next(extbuf, extlen, offset, ...)`: the next option after `offset` that is not
/// padding.
pub fn opt_next(buf: &[u8], offset: i32) -> Option<Opt> {
    let mut at = start(offset)?;
    while at < buf.len() {
        let o = opt_at(buf, at)?;
        if o.ty > 1 {
            return Some(o);
        }
        at = o.next;
    }
    None
}

/// `inet6_opt_find(extbuf, extlen, offset, type, ...)`: the next option of type `ty` after
/// `offset` (padding included: the pads have types too).
pub fn opt_find(buf: &[u8], offset: i32, ty: u8) -> Option<Opt> {
    let mut at = start(offset)?;
    while at < buf.len() {
        let o = opt_at(buf, at)?;
        if o.ty == ty {
            return Some(o);
        }
        at = o.next;
    }
    None
}

/// `inet6_rth_space(type, segments)`: the size of a routing header, 0 for an unsupported type
/// or a segment count outside 0–127.
pub fn rth_space(ty: i32, segments: i32) -> usize {
    match (ty, usize::try_from(segments)) {
        (RTHDR_TYPE_0, Ok(n)) if n <= 127 => n.saturating_mul(16).saturating_add(8),
        _ => 0,
    }
}

/// `inet6_rth_init(bp, bp_len, type, segments)`: a zeroed type-0 header of `segments`
/// addresses at the start of `buf`.
pub fn rth_init(buf: &mut [u8], ty: i32, segments: i32) -> Option<()> {
    let space = rth_space(ty, segments);
    if space == 0 || space > buf.len() {
        return None;
    }
    let h = buf.get_mut(..space)?;
    h.fill(0);
    *h.get_mut(1)? = u8::try_from(segments).ok()?.wrapping_mul(2);
    Some(())
}

/// The segment count of a type-0 header (its length byte / 2).
fn segments(buf: &[u8]) -> Option<usize> {
    let len = usize::from(*buf.get(1)?);
    (*buf.get(2)? == 0).then_some(len / 2)
}

/// `inet6_rth_segments(bp)`.
pub fn rth_segments(buf: &[u8]) -> Option<i32> {
    i32::try_from(segments(buf)?).ok()
}

fn addr_range(i: usize) -> Option<core::ops::Range<usize>> {
    let at = i.checked_mul(16)?.checked_add(8)?;
    Some(at..at.checked_add(16)?)
}

/// `inet6_rth_add(bp, addr)`: store the next address and count it in "segments left".
pub fn rth_add(buf: &mut [u8], addr: &[u8; 16]) -> Option<()> {
    let n = segments(buf)?;
    let left = usize::from(*buf.get(3)?);
    if left >= n {
        return None;
    }
    buf.get_mut(addr_range(left)?)?.copy_from_slice(addr);
    *buf.get_mut(3)? = u8::try_from(left.checked_add(1)?).ok()?;
    Some(())
}

/// `inet6_rth_reverse(in, out)` over one header (`buf` holds `in` and, at `out`, the output):
/// the addresses in reverse order, "segments left" set to their count. `in` and `out` may be
/// the same header.
pub fn rth_reverse(input: &[u8], out: &mut [u8]) -> Option<()> {
    let n = segments(input)?;
    let len = addr_range(n)?.start;
    let head = input.get(..8)?;
    let mut addrs = [[0u8; 16]; 127];
    for (i, slot) in addrs.iter_mut().take(n).enumerate() {
        slot.copy_from_slice(input.get(addr_range(i)?)?);
    }
    let o = out.get_mut(..len)?;
    o.get_mut(..8)?.copy_from_slice(head);
    *o.get_mut(3)? = u8::try_from(n).ok()?;
    for (i, a) in addrs.iter().take(n).rev().enumerate() {
        o.get_mut(addr_range(i)?)?.copy_from_slice(a);
    }
    Some(())
}

/// `inet6_rth_getaddr(bp, index)`: the offset of address `index`.
pub fn rth_getaddr(buf: &[u8], index: i32) -> Option<usize> {
    let i = usize::try_from(index).ok()?;
    (i < segments(buf)?).then(|| addr_range(i).map(|r| r.start)).flatten()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn options_round_trip() {
        let mut b = [0xeeu8; 64];
        assert_eq!(opt_init(Some(&mut b)), Some(2));
        assert_eq!(b[1], 7);
        let (o, d) = opt_append(Some(&mut b), 2, 0xc2, 4, 4).unwrap();
        assert_eq!((o, d), (8, 4));
        b[4..8].copy_from_slice(&[0x11, 0x22, 0x33, 0x44]);
        let (o, _) = opt_append(Some(&mut b), o, 5, 2, 2).unwrap();
        assert_eq!(o, 12);
        assert_eq!(opt_append(Some(&mut b), o, 0x10, 3, 8), None);
        assert_eq!(opt_append(Some(&mut b), o, 0, 4, 4), None);
        assert_eq!(opt_finish(Some(&mut b), o), Some(16));
        assert_eq!(b[12..16], [1, 2, 0, 0]);
        let first = opt_next(&b[..16], 0).unwrap();
        assert_eq!((first.ty, first.len, first.next), (0xc2, 4, 8));
        assert_eq!(opt_find(&b[..16], 0, 5).unwrap().next, 12);
        assert_eq!(opt_append(None, 5, 0xc2, 4, 4), Some((12, 8)));
        assert_eq!(opt_finish(None, 17), Some(24));
    }

    #[test]
    fn routing_header() {
        assert_eq!(rth_space(0, 3), 56);
        assert_eq!(rth_space(0, 128), 0);
        let mut b = [0xeeu8; 128];
        rth_init(&mut b, 0, 3).unwrap();
        for i in 1..=3u8 {
            rth_add(&mut b, &[i; 16]).unwrap();
        }
        assert_eq!(rth_add(&mut b, &[9; 16]), None);
        assert_eq!(&b[..8], &[0, 6, 0, 3, 0, 0, 0, 0]);
        let copy = b;
        rth_reverse(&copy, &mut b).unwrap();
        assert_eq!(b[8], 3);
        assert_eq!(rth_getaddr(&b, 3), None);
    }
}
