//! Fused transcoders beside the pivot loop (ICONV-1, D-6.11): UTF-8 straight into UTF-16,
//! UTF-32, UCS-2 and UCS-4 without the per-character codec dispatch and staging of
//! `Conv::convert`.
//!
//! Contract: a fused routine converts only whole, valid characters the target stores natively,
//! advancing the input and the output together by whole characters, and stops, touching nothing
//! further, at the first thing it does not own: the end of the input, no room for the next
//! character, an incomplete or invalid UTF-8 sequence, or a code point above U+FFFF into UCS-2.
//! The general loop then handles that one character (so `E2BIG`, `EINVAL`, `EILSEQ`, `//IGNORE`
//! and `//TRANSLIT` are exactly the pivot's) and offers the rest to the fused routine again.

use super::{Codec, Order};
use crate::charmap::SingleByte;

/// A wide target of the fused UTF-8 transcoder: the unit size, the order and whether it is
/// UCS-2 (no surrogate pairs).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) struct Wide {
    size: usize,
    order: Order,
    ucs2: bool,
}

/// The fused target `to` denotes, if any. The BOM forms write little-endian units after their
/// BOM, which `convert` has written before the first fused call.
pub(super) fn wide_target(to: Codec) -> Option<Wide> {
    match to {
        Codec::Utf16(o) => Some(Wide { size: 2, order: o.unwrap_or(Order::Le), ucs2: false }),
        Codec::Utf32(o) => Some(Wide { size: 4, order: o.unwrap_or(Order::Le), ucs2: false }),
        Codec::Ucs2(o) => Some(Wide { size: 2, order: o, ucs2: true }),
        Codec::Ucs4(o) => Some(Wide { size: 4, order: o, ucs2: false }),
        _ => None,
    }
}

/// Convert the leading characters of the UTF-8 `input` into `out`: the bytes read and written.
pub(super) fn utf8_to_wide(input: &[u8], out: &mut [u8], w: Wide) -> (usize, usize) {
    let be = w.order == Order::Be;
    match (w.size, be, w.ucs2) {
        (2, false, false) => run::<2, false, false>(input, out),
        (2, true, false) => run::<2, true, false>(input, out),
        (2, false, true) => run::<2, false, true>(input, out),
        (2, true, true) => run::<2, true, true>(input, out),
        (_, false, _) => run::<4, false, false>(input, out),
        (_, true, _) => run::<4, true, false>(input, out),
    }
}

// Planted bugs of the tests (a thread-local switch compiled only into the unit tests): 1 = no
// check of the third byte, 2 = wrong byte order for UTF-32, 3 = no fallback for an astral
// character into UCS-2.
#[cfg(test)]
thread_local!(static PLANT: core::cell::Cell<u8> = const { core::cell::Cell::new(0) });

#[cfg(test)]
fn planted(n: u8) -> bool {
    PLANT.with(|p| p.get() == n)
}

#[cfg(not(test))]
#[inline(always)]
fn planted(_: u8) -> bool {
    false
}

/// Store one unit of `SZ` bytes (2 or 4) in the order `BE`.
#[inline(always)]
fn put<const SZ: usize, const BE: bool>(dst: &mut [u8; SZ], v: u32) {
    let be = if SZ == 4 && planted(2) { !BE } else { BE };
    // `v` fits `SZ` bytes: the low ones, most significant first for big-endian.
    let (b, src) = if be { (v.to_be_bytes(), 4 - SZ..4) } else { (v.to_le_bytes(), 0..SZ) };
    if let Some(src) = b.get(src) {
        dst.copy_from_slice(src);
    }
}

/// One non-ASCII UTF-8 character (RFC 3629, as `decode_utf8`): the code point and its length,
/// `None` for anything incomplete or invalid.
#[inline(always)]
fn decode_multi(rest: &[u8]) -> Option<(u32, usize)> {
    let cont = |b: u8| b & 0xC0 == 0x80;
    match *rest {
        [b0 @ 0xC2..=0xDF, b1, ..] if cont(b1) => Some(((u32::from(b0 & 0x1F) << 6) | u32::from(b1 & 0x3F), 2)),
        [b0 @ 0xE0..=0xEF, b1, b2, ..] if cont(b1) && (cont(b2) || planted(1)) => {
            let c = (u32::from(b0 & 0x0F) << 12) | (u32::from(b1 & 0x3F) << 6) | u32::from(b2 & 0x3F);
            // Not overlong, not a surrogate.
            (c >= 0x800 && !(0xD800..0xE000).contains(&c)).then_some((c, 3))
        }
        [b0 @ 0xF0..=0xF4, b1, b2, b3, ..] if cont(b1) && cont(b2) && cont(b3) => {
            let c = (u32::from(b0 & 0x07) << 18) | (u32::from(b1 & 0x3F) << 12) | (u32::from(b2 & 0x3F) << 6) | u32::from(b3 & 0x3F);
            (0x1_0000..=0x10_FFFF).contains(&c).then_some((c, 4))
        }
        _ => None,
    }
}

/// The high bit of each byte of a word.
const HIGH: u64 = 0x8080_8080_8080_8080;

/// The ASCII bytes at the start of `input` (as many as fit in `out`) widened to units: how many.
#[inline(never)]
fn ascii_run<const SZ: usize, const BE: bool>(input: &[u8], out: &mut [u8]) -> usize {
    let room = out.len() / SZ;
    let mut n = 0;
    while let Some(eight) = input.get(n..).and_then(|s| s.first_chunk::<8>()) {
        if u64::from_ne_bytes(*eight) & HIGH != 0 || room - n < 8 {
            break;
        }
        for (unit, &b) in out.get_mut(n * SZ..).unwrap_or(&mut []).chunks_exact_mut(SZ).zip(eight) {
            if let Some(unit) = unit.first_chunk_mut::<SZ>() {
                put::<SZ, BE>(unit, u32::from(b));
            }
        }
        n += 8;
    }
    for (unit, &b) in out.get_mut(n * SZ..).unwrap_or(&mut []).chunks_exact_mut(SZ).zip(input.get(n..).unwrap_or(&[])) {
        if !b.is_ascii() {
            break;
        }
        if let Some(unit) = unit.first_chunk_mut::<SZ>() {
            put::<SZ, BE>(unit, u32::from(b));
        }
        n += 1;
    }
    n
}

fn run<const SZ: usize, const BE: bool, const UCS2: bool>(input: &[u8], out: &mut [u8]) -> (usize, usize) {
    let (mut i, mut o, mut run) = (0usize, 0usize, 0usize);
    while let Some(&b0) = input.get(i) {
        if b0 < 0x80 {
            // After eight in a row, the rest of the run by the out-of-line widening loop (its
            // setup would otherwise be paid by every short run).
            if run >= 8 {
                let n = ascii_run::<SZ, BE>(input.get(i..).unwrap_or(&[]), out.get_mut(o..).unwrap_or(&mut []));
                run = 0;
                if n > 0 {
                    i += n;
                    o += n * SZ;
                    continue;
                }
            }
            run += 1;
            let Some(dst) = out.get_mut(o..).and_then(|s| s.first_chunk_mut::<SZ>()) else { break };
            put::<SZ, BE>(dst, u32::from(b0));
            i += 1;
            o += SZ;
            continue;
        }
        run = 0;
        let Some((c, n)) = input.get(i..).and_then(decode_multi) else { break };
        if c <= 0xFFFF {
            let Some(dst) = out.get_mut(o..).and_then(|s| s.first_chunk_mut::<SZ>()) else { break };
            put::<SZ, BE>(dst, c);
            o += SZ;
        } else if SZ == 4 {
            let Some(dst) = out.get_mut(o..).and_then(|s| s.first_chunk_mut::<SZ>()) else { break };
            put::<SZ, BE>(dst, c);
            o += SZ;
        } else if !UCS2 {
            // UTF-16: a surrogate pair, both units or neither.
            let Some(dst) = out.get_mut(o..).and_then(|s| s.first_chunk_mut::<4>()) else { break };
            let v = c - 0x1_0000;
            let (hi, lo) = dst.split_at_mut(2);
            if let (Some(hi), Some(lo)) = (hi.first_chunk_mut::<2>(), lo.first_chunk_mut::<2>()) {
                put::<2, BE>(hi, 0xD800 + (v >> 10));
                put::<2, BE>(lo, 0xDC00 + (v & 0x3FF));
            }
            o += 4;
        } else if UCS2 && planted(3) {
            let Some(dst) = out.get_mut(o..).and_then(|s| s.first_chunk_mut::<SZ>()) else { break };
            put::<SZ, BE>(dst, c & 0xFFFF);
            o += SZ;
        } else {
            // UCS-2 cannot hold it: the general loop transliterates or rejects.
            break;
        }
        i += n;
    }
    (i, o)
}

/// The fused source `from` denotes once its byte order is known (`order`: what a BOM or the
/// first unit selected), if any: UTF-16, UTF-32, UCS-2 and UCS-4 units.
pub(super) fn wide_source(from: Codec, order: Option<Order>) -> Option<Wide> {
    match from {
        Codec::Utf16(o) => Some(Wide { size: 2, order: o.or(order)?, ucs2: false }),
        Codec::Utf32(o) => Some(Wide { size: 4, order: o.or(order)?, ucs2: false }),
        Codec::Ucs2(o) => Some(Wide { size: 2, order: o, ucs2: true }),
        Codec::Ucs4(o) => Some(Wide { size: 4, order: o, ucs2: false }),
        _ => None,
    }
}

/// Convert the leading characters of the wide `input` (byte order known) into UTF-8 `out`: the
/// bytes read and written.
pub(super) fn wide_to_utf8(input: &[u8], out: &mut [u8], w: Wide) -> (usize, usize) {
    let be = w.order == Order::Be;
    match (w.size, be, w.ucs2) {
        (2, false, false) => narrow::<2, false, false>(input, out),
        (2, true, false) => narrow::<2, true, false>(input, out),
        (2, false, true) => narrow::<2, false, true>(input, out),
        (2, true, true) => narrow::<2, true, true>(input, out),
        (_, false, _) => narrow::<4, false, false>(input, out),
        (_, true, _) => narrow::<4, true, false>(input, out),
    }
}

/// Load one unit of `SZ` bytes in the order `BE`.
#[inline(always)]
fn load<const SZ: usize, const BE: bool>(unit: &[u8; SZ]) -> u32 {
    let be = if SZ == 2 && planted(4) { !BE } else { BE };
    let mut v = 0u32;
    if be {
        for &b in unit {
            v = (v << 8) | u32::from(b);
        }
    } else {
        for &b in unit.iter().rev() {
            v = (v << 8) | u32::from(b);
        }
    }
    v
}

/// Units per ASCII block: 16 bytes.
const BLOCK: usize = 16;

/// The ASCII units at the start of `input` (as many as fit in `out`) narrowed to bytes: how many.
#[inline(never)]
fn narrow_ascii_run<const SZ: usize, const BE: bool>(input: &[u8], out: &mut [u8]) -> usize {
    // The bits that must be clear in each unit of a little-endian word of the memory bytes: the
    // high bits of the unit, and bit 7.
    let lane: u64 = match (SZ, BE) {
        (2, false) => 0xFF80,
        (2, true) => 0x80FF,
        (_, false) => 0xFFFF_FF80,
        (_, true) => 0x80FF_FFFF,
    };
    let mask = if SZ == 2 { lane * 0x0001_0001_0001_0001 } else { lane * 0x0000_0001_0000_0001 };
    let per = BLOCK / SZ;
    let low = if BE { SZ - 1 } else { 0 };
    let mut n = 0;
    while let (Some(blk), Some(dst)) = (input.get(n * SZ..).and_then(|s| s.first_chunk::<BLOCK>()), out.get_mut(n..).and_then(|s| s.get_mut(..per))) {
        let (a, b) = blk.split_at(8);
        let (Some(a), Some(b)) = (a.first_chunk::<8>(), b.first_chunk::<8>()) else { break };
        if (u64::from_le_bytes(*a) | u64::from_le_bytes(*b)) & mask != 0 {
            break;
        }
        for (d, unit) in dst.iter_mut().zip(blk.chunks_exact(SZ)) {
            *d = unit.get(low).copied().unwrap_or(0);
        }
        n += per;
    }
    for (slot, unit) in out.get_mut(n..).unwrap_or(&mut []).iter_mut().zip(input.get(n * SZ..).unwrap_or(&[]).chunks_exact(SZ)) {
        let Some(unit) = unit.first_chunk::<SZ>() else { break };
        let c = load::<SZ, BE>(unit);
        if c >= 0x80 {
            break;
        }
        *slot = u8::try_from(c).unwrap_or(0);
        n += 1;
    }
    n
}

/// Store the UTF-8 form of `c` (below U+0800 or a BMP non-surrogate, or astral) at `out[o..]`:
/// the bytes written, `None` for no room.
#[inline(always)]
fn put_utf8(out: &mut [u8], o: usize, c: u32) -> Option<usize> {
    let cont = |sh: u32| 0x80 | u8::try_from((c >> sh) & 0x3F).unwrap_or(0);
    let lead = |pre: u8, sh: u32| pre | u8::try_from(c >> sh).unwrap_or(0);
    let rest = out.get_mut(o..)?;
    if c < 0x800 {
        *rest.first_chunk_mut::<2>()? = [lead(0xC0, 6), cont(0)];
        Some(2)
    } else if c < 0x1_0000 {
        *rest.first_chunk_mut::<3>()? = [lead(0xE0, 12), cont(6), cont(0)];
        Some(3)
    } else {
        // The planted bug leaves the first continuation byte's top bits wrong.
        let second = if planted(6) { cont(12) & 0x7F } else { cont(12) };
        *rest.first_chunk_mut::<4>()? = [lead(0xF0, 18), second, cont(6), cont(0)];
        Some(4)
    }
}

fn narrow<const SZ: usize, const BE: bool, const UCS2: bool>(input: &[u8], out: &mut [u8]) -> (usize, usize) {
    let (mut i, mut o, mut run) = (0usize, 0usize, 0usize);
    while let Some(unit) = input.get(i..).and_then(|s| s.first_chunk::<SZ>()) {
        let c = load::<SZ, BE>(unit);
        if c < 0x80 {
            if run >= 8 {
                let n = narrow_ascii_run::<SZ, BE>(input.get(i..).unwrap_or(&[]), out.get_mut(o..).unwrap_or(&mut []));
                run = 0;
                if n > 0 {
                    i += n * SZ;
                    o += n;
                    continue;
                }
            }
            run += 1;
            let Some(slot) = out.get_mut(o) else { break };
            *slot = u8::try_from(c).unwrap_or(0);
            i += SZ;
            o += 1;
            continue;
        }
        run = 0;
        let (cp, used) = if SZ == 2 && (0xD800..0xE000).contains(&c) {
            // A surrogate pair, whole; anything else (UCS-2, lone, reversed, cut) is the general loop's.
            let lo = if UCS2 || c >= 0xDC00 {
                break;
            } else {
                match input.get(i + 2..).and_then(|s| s.first_chunk::<2>()) {
                    Some(u) => load::<2, BE>(u),
                    None => break,
                }
            };
            if !(0xDC00..0xE000).contains(&lo) && !planted(5) {
                break;
            }
            (0x1_0000 + ((c - 0xD800) << 10) + (lo & 0x3FF), 4)
        } else if SZ == 4 && (c > 0x10_FFFF || (0xD800..0xE000).contains(&c)) {
            break;
        } else {
            (c, SZ)
        };
        let Some(n) = put_utf8(out, o, cp) else { break };
        i += used;
        o += n;
    }
    (i, o)
}

/// Copy the ASCII bytes at the start of `input` among its first eight to `out`, when both hold
/// eight bytes: how many (0 when either is shorter, or the first byte is not ASCII). Writes
/// exactly the bytes it reports.
#[inline(always)]
fn ascii_copy(input: &[u8], out: &mut [u8]) -> usize {
    let (Some(eight), Some(dst)) = (input.first_chunk::<8>(), out.first_chunk_mut::<8>()) else { return 0 };
    let high = u64::from_le_bytes(*eight) & HIGH;
    if high == 0 {
        *dst = *eight;
        return 8;
    }
    let k = usize::try_from(high.trailing_zeros() / 8).unwrap_or(0);
    crate::mem::copy_inline(dst.get_mut(..k).unwrap_or(&mut []), eight.get(..k).unwrap_or(&[]));
    k
}

/// The direct tables of a single-byte charmap on either side of a UTF-8 conversion (ICONV-3,
/// D-6.13): `c` is a `Codec::Charmap` with them; the caller checks the other side is UTF-8.
pub(super) fn single_byte(c: Codec) -> Option<&'static SingleByte> {
    match c {
        Codec::Charmap(cm) => cm.0.single_byte(),
        _ => None,
    }
}

/// Convert the leading bytes of the single-byte charmap text `input` into UTF-8: the bytes read
/// and written. Stops, touching nothing further, at a byte the table gives no UTF-8 for (no
/// character, a surrogate, above U+FFFF), or where the next character does not fit.
pub(super) fn single_to_utf8(input: &[u8], out: &mut [u8], sb: &SingleByte) -> (usize, usize) {
    let (mut i, mut o) = (0usize, 0usize);
    while let Some(&b) = input.get(i) {
        // The ASCII bytes among the next eight at once when ASCII is itself in the charset.
        if b < 0x80 && sb.ascii() {
            let k = ascii_copy(input.get(i..).unwrap_or(&[]), out.get_mut(o..).unwrap_or(&mut []));
            if k > 0 {
                i += k;
                o += k;
                continue;
            }
        }
        let (len, [b0, mut b1, b2]) = sb.utf8(b);
        if planted(7) {
            b1 ^= 1;
        }
        if len == 0 && planted(9) {
            // The planted bug stores a placeholder instead of leaving the byte to the general loop.
            let Some(slot) = out.get_mut(o) else { break };
            *slot = b'?';
            i += 1;
            o += 1;
            continue;
        }
        let Some(dst) = out.get_mut(o..) else { break };
        match len {
            1 => match dst.first_mut() {
                Some(d) => *d = b0,
                None => break,
            },
            2 => match dst.first_chunk_mut::<2>() {
                Some(d) => *d = [b0, b1],
                None => break,
            },
            3 => match dst.first_chunk_mut::<3>() {
                Some(d) => *d = [b0, b1, b2],
                None => break,
            },
            _ => break,
        }
        i += 1;
        o += usize::from(len);
    }
    (i, o)
}

/// Convert the leading characters of the UTF-8 `input` into the single-byte charmap in `out`:
/// the bytes read and written. Stops, touching nothing further, at the end of the input, an
/// incomplete or invalid sequence, a character the charset lacks (or above U+FFFF), or no room.
pub(super) fn utf8_to_single(input: &[u8], out: &mut [u8], sb: &SingleByte) -> (usize, usize) {
    let (mut i, mut o) = (0usize, 0usize);
    while let Some(&b) = input.get(i) {
        if b < 0x80 {
            if sb.ascii() {
                let k = ascii_copy(input.get(i..).unwrap_or(&[]), out.get_mut(o..).unwrap_or(&mut []));
                if k > 0 {
                    i += k;
                    o += k;
                    continue;
                }
            }
            match (sb.byte_of(u32::from(b)), out.get_mut(o)) {
                (Some(x), Some(d)) => *d = x,
                _ => break,
            }
            i += 1;
            o += 1;
            continue;
        }
        let Some((c, n)) = input.get(i..).and_then(decode_multi) else { break };
        let Some(mut x) = sb.byte_of(c) else {
            if planted(9) {
                // The planted bug stores a placeholder instead of leaving the character.
                let Some(d) = out.get_mut(o) else { break };
                *d = b'?';
                i += n;
                o += 1;
                continue;
            }
            break;
        };
        if planted(8) && c >= 0x100 {
            x = x.wrapping_add(1);
        }
        let Some(d) = out.get_mut(o) else { break };
        *d = x;
        i += n;
        o += 1;
    }
    (i, o)
}

#[cfg(test)]
#[allow(clippy::indexing_slicing, clippy::unwrap_used, clippy::arithmetic_side_effects)]
mod tests {
    use super::super::{Conv, Outcome, State};
    use super::*;
    use crate::locale::Locale;

    const CORPUS: &[&[u8]] = &[
        b"",
        b"plain ascii text",
        b"0123456789abcdefghijklmnopqrstuvwxyz\xC3\xA9012345678\xE2\x82\xAC01234567",
        b"01234567890123456\xFF0123456789abcdef\xF0\x9F\x98\x80",
        b"\xC3\xA9t\xC3\xA9",
        b"a\xE2\x82\xACb",
        b"\xF0\x9F\x98\x80x\xF0\x90\x80\x80",
        b"\xF4\x8F\xBF\xBF\xEF\xBF\xBD\xDF\xBF\xE0\xA0\x80",
        b"\xC2\x80\xC2\xBF\xC3\x80",
        b"ok\x80bad",
        b"ok\xBF",
        b"\xC0\x80",
        b"\xC1\xBF",
        b"\xE0\x80\x80",
        b"\xE0\x9F\xBF",
        b"\xED\xA0\x80",
        b"\xED\x9F\xBF\xEE\x80\x80",
        b"\xF0\x80\x80\x80",
        b"\xF0\x8F\xBF\xBF",
        b"\xF4\x90\x80\x80",
        b"\xF5\x80\x80\x80",
        b"\xF8\x88\x80\x80\x80",
        b"\xFF\xFEa",
        b"\xC3",
        b"a\xE2\x82",
        b"\xF0\x9F\x98",
        b"\xE2\x28\xA1",
        b"\xE2\x82\x28",
        b"\xF0\x9F\x28\x80",
        b"\xF0\x9F\x98\x28",
        b"x\xC3\xA9\xE2\x82\xAC\xF0\x9F\x98\x80\xC3\xA9y",
        b"\xEF\xBB\xBFbom",
        b"\xEF\xBF\xBE\xEF\xBF\xBF",
    ];

    fn targets() -> Vec<Codec> {
        let mut t = vec![Codec::Utf16(None), Codec::Utf32(None)];
        for o in [Order::Le, Order::Be] {
            t.extend([Codec::Utf16(Some(o)), Codec::Utf32(Some(o)), Codec::Ucs2(o), Codec::Ucs4(o)]);
        }
        t
    }

    fn conv(to: Codec, translit: bool, ignore: bool) -> Conv {
        conv_from(Codec::Utf8, to, translit, ignore)
    }

    fn conv_from(from: Codec, to: Codec, translit: bool, ignore: bool) -> Conv {
        Conv { from, to, translit, ignore, state: State::default() }
    }

    type Call = (Outcome, Vec<u8>, State);

    fn call(c: &mut Conv, input: &[u8], cap: usize, loc: &Locale, fused: bool) -> Call {
        let mut out = vec![0xAAu8; cap];
        let o = c.convert_with(input, &mut out, loc, fused);
        out.truncate(o.written);
        (o, out, c.state)
    }

    /// Every target, flag set and locale: one call on every prefix of every corpus string
    /// (the input split at each byte) into every output size from 0 past the full output
    /// (an `E2BIG` at each shortfall) answers byte for byte as the pivot loop; and a second call
    /// resuming where the first stopped ends in the same bytes.
    #[test]
    fn fused_utf8_to_wide_equals_the_pivot_loop() {
        let calls = equals_the_pivot_loop();
        assert!(calls > 300_000, "{calls}");
    }

    /// Each planted bug (see `PLANT`) makes the comparison fail: the tests can fail.
    #[test]
    fn planted_bugs_are_caught() {
        for n in 1..=3 {
            PLANT.with(|p| p.set(n));
            let caught = std::panic::catch_unwind(equals_the_pivot_loop).is_err();
            PLANT.with(|p| p.set(0));
            assert!(caught, "planted bug {n} not caught");
        }
    }

    fn equals_the_pivot_loop() -> usize {
        let locales = [Locale::C, Locale::c_utf8(b"C.UTF-8")];
        let mut calls = 0usize;
        for to in targets() {
            for (translit, ignore) in [(false, false), (true, false), (false, true), (true, true)] {
                for loc in &locales {
                    for s in CORPUS {
                        for k in 0..=s.len() {
                            let part = &s[..k];
                            for cap in 0..(part.len() * 4 + 10) {
                                let a = call(&mut conv(to, translit, ignore), part, cap, loc, true);
                                let b = call(&mut conv(to, translit, ignore), part, cap, loc, false);
                                assert_eq!(a, b, "to {to:?} translit {translit} ignore {ignore} input {part:02x?} cap {cap}");
                                calls += 1;
                                // Resume: the rest of the whole string into a big buffer.
                                let rest = &s[a.0.read..];
                                let (mut fa, mut fb) = (conv(to, translit, ignore), conv(to, translit, ignore));
                                fa.state = a.2;
                                fb.state = b.2;
                                assert_eq!(call(&mut fa, rest, 200, loc, true), call(&mut fb, rest, 200, loc, false), "resume {part:02x?} cap {cap}");
                            }
                        }
                    }
                }
            }
        }
        calls
    }

    /// Exhaustive lead + two continuation bytes and every 4-byte window around the range
    /// edges, whole: the fused decoder accepts exactly what `decode_utf8` does.
    #[test]
    fn fused_decoder_agrees_with_decode_utf8() {
        use super::super::{decode_utf8, Dec};
        let picks = [0x00u8, 0x41, 0x7F, 0x80, 0x8F, 0x90, 0x9F, 0xA0, 0xBF, 0xC0, 0xC1, 0xC2, 0xDF, 0xE0, 0xE1, 0xEC, 0xED, 0xEE, 0xEF, 0xF0, 0xF1, 0xF3, 0xF4, 0xF5, 0xFF];
        for &a in picks.iter().filter(|&&a| a >= 0x80) {
            for &b in &picks {
                for &c in &picks {
                    for &d in &picks {
                        let seq = [a, b, c, d];
                        let want = match decode_utf8(&seq) {
                            Dec::Char(ch, n) => Some((ch, n)),
                            _ => None,
                        };
                        assert_eq!(decode_multi(&seq), want, "{seq:02x?}");
                    }
                }
            }
        }
    }

    /// A long mixed run, whole, agrees with the pivot loop and with a hand-built UTF-16.
    #[test]
    fn long_mixed_text_matches_pivot_and_utf16() {
        let text = "ascii \u{e9}\u{20ac}\u{1F600} more ".repeat(300);
        let loc = Locale::C;
        let mut want = Vec::new();
        for u in text.encode_utf16() {
            want.extend_from_slice(&u.to_be_bytes());
        }
        let cap = text.len() * 4;
        let a = call(&mut conv(Codec::Utf16(Some(Order::Be)), false, false), text.as_bytes(), cap, &loc, true);
        assert_eq!(a.1, want);
        assert_eq!(a, call(&mut conv(Codec::Utf16(Some(Order::Be)), false, false), text.as_bytes(), cap, &loc, false));
    }

    // ---- ICONV-2: wide source into UTF-8 ----

    /// Unit sequences (code units, not characters) of the wide-source corpus.
    const UNITS: &[&[u32]] = &[
        &[],
        &[0x61, 0x62, 0x63],
        &[0xE9, 0x20AC, 0x7FF, 0x800, 0xFFFF, 0x7F, 0x80],
        &[0xD83D, 0xDE00],
        &[0xD83D, 0xDE00, 0x61, 0xD800, 0xDC00, 0xDBFF, 0xDFFF],
        &[0xD83D],
        &[0xDE00],
        &[0xD83D, 0x61],
        &[0xDE00, 0xD83D],
        &[0xD83D, 0xD83D, 0xDE00],
        &[0xD800, 0xE000],
        &[0xFEFF, 0x61],
        &[0xFFFE, 0x61],
        &[0x61, 0xFEFF, 0xFFFE, 0xFFFF],
        &[0x10FFFF, 0x110000],
        &[0x10000, 0x10FFFF, 0x1F600],
        &[0x7FFF_FFFF, 0x61],
        &[0x8000_0000],
        &[0xFFFE_0000, 0x61],
        &[0xD800],
        &[0xDFFF, 0x61],
        // ASCII runs long enough for the block loop, then something else.
        &[0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x6F, 0x70, 0x71, 0x72, 0x73, 0x74, 0xE9, 0x75],
        &[0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x6F, 0x70, 0x71, 0x72, 0x73, 0x74, 0x75, 0x76, 0x77, 0x78, 0xD83D, 0xDE00, 0x79],
        &[0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x6F, 0x70, 0x71, 0x80, 0x72],
        &[0x61, 0x62, 0x63, 0x64, 0x65, 0x66, 0x67, 0x68, 0x69, 0x6A, 0x6B, 0x6C, 0x6D, 0x6E, 0x6F, 0x70, 0x100],
    ];

    fn sources() -> Vec<Codec> {
        let mut t = vec![Codec::Utf16(None), Codec::Utf32(None)];
        for o in [Order::Le, Order::Be] {
            t.extend([Codec::Utf16(Some(o)), Codec::Utf32(Some(o)), Codec::Ucs2(o), Codec::Ucs4(o)]);
        }
        t
    }

    /// The unit size and the fixed order (`None` for the BOM forms) of a wide source.
    fn shape(from: Codec) -> (usize, Option<Order>) {
        match from {
            Codec::Utf16(o) => (2, o),
            Codec::Utf32(o) => (4, o),
            Codec::Ucs2(o) => (2, Some(o)),
            Codec::Ucs4(o) => (4, Some(o)),
            _ => unreachable!(),
        }
    }

    /// The bytes of `units` of `size` bytes in `order` (`None` if a unit does not fit).
    fn encode_units(units: &[u32], size: usize, order: Order) -> Option<Vec<u8>> {
        let mut v = Vec::new();
        for &u in units {
            if size == 2 && u > 0xFFFF {
                return None;
            }
            let b = u.to_be_bytes();
            let b = &b[4 - size..];
            if order == Order::Be {
                v.extend_from_slice(b);
            } else {
                v.extend(b.iter().rev());
            }
        }
        Some(v)
    }

    /// Every source, target UTF-8, flag set and locale: every prefix of every unit string in
    /// each byte order (the input split at each byte, odd cuts included) into every output size
    /// answers as the pivot loop, and so does the resumed second call.
    fn wide_equals_the_pivot_loop() -> usize {
        let locales = [Locale::C, Locale::c_utf8(b"C.UTF-8")];
        let mut calls = 0usize;
        for from in sources() {
            let (size, fixed) = shape(from);
            let orders: Vec<Order> = fixed.map_or(vec![Order::Le, Order::Be], |o| vec![o]);
            for (translit, ignore) in [(false, false), (true, false), (false, true), (true, true)] {
                for loc in &locales {
                    for units in UNITS {
                        for &order in &orders {
                            let Some(s) = encode_units(units, size, order) else { continue };
                            for k in 0..=s.len() {
                                let part = &s[..k];
                                for cap in 0..(part.len() * 2 + 10) {
                                    let a = call(&mut conv_from(from, Codec::Utf8, translit, ignore), part, cap, loc, true);
                                    let b = call(&mut conv_from(from, Codec::Utf8, translit, ignore), part, cap, loc, false);
                                    assert_eq!(a, b, "from {from:?} translit {translit} ignore {ignore} input {part:02x?} cap {cap}");
                                    calls += 1;
                                    let rest = &s[a.0.read..];
                                    let (mut fa, mut fb) = (conv_from(from, Codec::Utf8, translit, ignore), conv_from(from, Codec::Utf8, translit, ignore));
                                    fa.state = a.2;
                                    fb.state = b.2;
                                    assert_eq!(call(&mut fa, rest, 200, loc, true), call(&mut fb, rest, 200, loc, false), "resume {part:02x?} cap {cap}");
                                }
                            }
                        }
                    }
                }
            }
        }
        calls
    }

    #[test]
    fn fused_wide_to_utf8_equals_the_pivot_loop() {
        let calls = wide_equals_the_pivot_loop();
        assert!(calls > 200_000, "{calls}");
    }

    /// Each planted bug of the wide-source direction makes the comparison fail.
    #[test]
    fn planted_bugs_of_the_wide_source_are_caught() {
        for n in 4..=6 {
            PLANT.with(|p| p.set(n));
            let caught = std::panic::catch_unwind(wide_equals_the_pivot_loop).is_err();
            PLANT.with(|p| p.set(0));
            assert!(caught, "planted bug {n} not caught");
        }
    }

    /// Every scalar value, from each wide source, whole: the bytes of `char::encode_utf8`.
    #[test]
    fn every_scalar_value_encodes_as_utf8() {
        let loc = Locale::C;
        for from in sources() {
            let (size, fixed) = shape(from);
            let order = fixed.unwrap_or(Order::Le);
            // A first ASCII unit settles the order of the BOM forms without being a BOM.
            let mut units: Vec<u32> = vec![0x41];
            let mut want = String::from("A");
            for c in (0..=0x10_FFFFu32).filter(|&c| char::from_u32(c).is_some()) {
                if size == 2 && c > 0xFFFF && matches!(from, Codec::Ucs2(_)) {
                    continue;
                }
                if size == 2 && c > 0xFFFF {
                    let v = c - 0x10000;
                    units.extend([0xD800 + (v >> 10), 0xDC00 + (v & 0x3FF)]);
                } else {
                    units.push(c);
                }
                want.push(char::from_u32(c).unwrap());
            }
            let input = encode_units(&units, size, order).unwrap();
            let mut c = conv_from(from, Codec::Utf8, false, false);
            let mut out = vec![0u8; want.len() + 8];
            let o = c.convert_with(&input, &mut out, &loc, true);
            assert_eq!(o.result, Ok(0), "{from:?}");
            assert_eq!(&out[..o.written], want.as_bytes(), "{from:?}");
            assert_eq!(o.read, input.len());
        }
    }

    // ---- ICONV-3: single-byte charmaps <-> UTF-8 (D-6.13) ----

    /// A charmap text from (code point, byte) pairs.
    fn charmap_text(name: &str, pairs: &[(u32, u8)]) -> Vec<u8> {
        let mut t = format!("<code_set_name> {name}\n<mb_cur_max> 1\nCHARMAP\n");
        for &(cp, b) in pairs {
            if cp < 0x1_0000 {
                t += &format!("<U{cp:04X}> /x{b:02x} X\n");
            } else {
                t += &format!("<U{cp:08X}> /x{b:02x} X\n");
            }
        }
        t += "END CHARMAP\n";
        t.into_bytes()
    }

    /// Three single-byte charsets: a Latin-9 like one (holes, a few BMP letters above 0x100), a
    /// KOI8 like one (Cyrillic, 3-byte box drawing, an astral and a surrogate entry, a hole),
    /// and one without ASCII that lists a code point twice.
    fn charmaps() -> Vec<Codec> {
        let ident = |r: core::ops::Range<u32>| r.map(|c| (c, u8::try_from(c).unwrap_or(0))).collect::<Vec<_>>();
        let mut latin = ident(0..0x80);
        latin.extend(ident(0xA0..0x100).into_iter().filter(|p| ![0xA4, 0xA6, 0xA8, 0xB4].contains(&p.0)));
        latin.extend([(0x20AC, 0xA4), (0x0160, 0xA6), (0x0161, 0xA8), (0x017D, 0xB4), (0x2026, 0x85), (0x2022, 0x95)]);
        let mut koi = ident(0..0x80);
        koi.extend((0..0x40u32).map(|k| (0x0410 + k, u8::try_from(0xC0 + k).unwrap_or(0))));
        koi.extend((0..0x10u32).map(|k| (0x2500 + k, u8::try_from(0xA0 + k).unwrap_or(0))));
        koi.extend([(0x1_F600, 0xB0), (0xD800, 0xB1), (0x0451, 0xB3)]);
        let mut odd = vec![(0x0391, 0x41), (0x0392, 0x42), (0x00E9, 0xE9), (0x00E9, 0xC9), (0x0100, 0x80), (0xFFFF, 0x81), (0x7FF, 0x82), (0x800, 0x83), (0x00, 0x00)];
        odd.extend(ident(0x61..0x7B));
        [("LATIN9", latin), ("KOI", koi), ("ODD", odd)]
            .into_iter()
            .map(|(n, p)| {
                let cm = crate::charmap::Charmap::parse(&charmap_text(n, &p)).unwrap();
                Codec::Charmap(crate::text::CharmapRef(Box::leak(Box::new(cm))))
            })
            .collect()
    }

    fn single_bytes_corpus() -> Vec<Vec<u8>> {
        let mut v: Vec<Vec<u8>> = (0..=255u8).map(|b| vec![b]).collect();
        v.extend((0..=255u8).map(|b| vec![b'a', b, b'z']));
        v.push(b"0123456789abcdefghijklmnopqrstuvwxyz".to_vec());
        v.push(b"01234567\xA401234567\xE9\xE9\xE9 01234567".to_vec());
        v.push((0..=255u8).collect());
        v.push((0..=255u8).rev().collect());
        v.push(b"abcdefgh\x85\x95\xA6\xA8\xB4\xFF\x81\xB0\xB1\xB3\xC0\xDF abcdefghij".to_vec());
        v
    }

    fn utf8_text_corpus() -> Vec<Vec<u8>> {
        let chars = ["a", "~", "\u{0}", "\u{e9}", "\u{c9}", "\u{20ac}", "\u{160}", "\u{161}", "\u{17d}", "\u{2026}", "\u{2022}", "\u{410}", "\u{44f}", "\u{451}", "\u{2500}", "\u{250f}", "\u{1f600}", "\u{ffff}", "\u{7ff}", "\u{800}", "\u{100}", "\u{391}", "\u{ff}", "\u{a0}", "\u{80}", "\u{9f}", "\u{4e2d}"];
        let mut v: Vec<Vec<u8>> = chars.iter().map(|c| c.as_bytes().to_vec()).collect();
        v.extend(chars.iter().map(|c| format!("abcdefgh{c}ijklmnopq{c}{c}").into_bytes()));
        v.push(b"0123456789abcdefghijklmnopqrstuvwxyz".to_vec());
        v.push(chars.concat().into_bytes());
        v.extend(CORPUS.iter().map(|c| c.to_vec()));
        v
    }

    /// Both directions of every single-byte charset against UTF-8: every prefix of every corpus
    /// string into every output size, for each flag set and locale, equals the pivot loop; and
    /// resuming where the first call stopped ends in the same bytes.
    fn single_equals_the_pivot_loop() -> usize {
        let locales = [Locale::C, Locale::c_utf8(b"C.UTF-8")];
        let mut calls = 0usize;
        for cm in charmaps() {
            for (from, to, corpus) in [(cm, Codec::Utf8, single_bytes_corpus()), (Codec::Utf8, cm, utf8_text_corpus())] {
                for (translit, ignore) in [(false, false), (true, false), (false, true), (true, true)] {
                    for loc in &locales {
                        for s in &corpus {
                            let long = s.len() > 40;
                            for k in (0..=s.len()).step_by(if long { 3 } else { 1 }).chain([s.len()]) {
                                let part = &s[..k];
                                for cap in (0..(part.len() * 4 + 10)).step_by(if long { 5 } else { 1 }) {
                                    let a = call(&mut conv_from(from, to, translit, ignore), part, cap, loc, true);
                                    let b = call(&mut conv_from(from, to, translit, ignore), part, cap, loc, false);
                                    assert_eq!(a, b, "{from:?} -> {to:?} translit {translit} ignore {ignore} input {part:02x?} cap {cap}");
                                    calls += 1;
                                    let rest = &s[a.0.read..];
                                    let (mut fa, mut fb) = (conv_from(from, to, translit, ignore), conv_from(from, to, translit, ignore));
                                    fa.state = a.2;
                                    fb.state = b.2;
                                    assert_eq!(call(&mut fa, rest, 400, loc, true), call(&mut fb, rest, 400, loc, false), "resume {part:02x?} cap {cap}");
                                }
                            }
                        }
                    }
                }
            }
        }
        calls
    }

    #[test]
    fn fused_single_byte_equals_the_pivot_loop() {
        let calls = single_equals_the_pivot_loop();
        assert!(calls > 100_000, "{calls}");
    }

    /// The tables answer exactly what the general lookups answer (every byte, every code point
    /// below U+10000), and the fused routines convert something (the tests are not vacuous).
    #[test]
    fn single_byte_tables_agree_with_the_general_lookups() {
        use crate::charmap::Step;
        for cm in charmaps() {
            let Codec::Charmap(c) = cm else { continue };
            let sb = c.0.single_byte().unwrap();
            for b in 0..=255u8 {
                let want = match c.0.decode(&[b]) {
                    Step::Char(cp, 1) => char::from_u32(cp).filter(|_| cp < 0x1_0000).map(|ch| ch.to_string().into_bytes()),
                    _ => None,
                };
                let (len, bytes) = sb.utf8(b);
                let got = (len > 0).then(|| bytes[..usize::from(len)].to_vec());
                assert_eq!(got, want, "byte {b:02x} of {:?}", c.0.name());
            }
            let mut one = [0u8; 8];
            for cp in 0..0x1_0000u32 {
                // Surrogates are never encoded (`Codec::encode`): the table leaves them out.
                let want = c.0.encode(cp, &mut one).map(|_| one[0]).filter(|_| !(0xD800..0xE000).contains(&cp));
                assert_eq!(sb.byte_of(cp), want, "cp {cp:04x} of {:?}", c.0.name());
            }
            let (r, w) = single_to_utf8(b"abcdefgh\xE9", &mut [0u8; 32], sb);
            assert!(r >= 8 && w >= 8, "{r} {w}");
        }
    }

    /// Each planted bug (7 = a wrong table entry, 8 = the reverse map off by one, 9 = no hand-back
    /// of an unmappable character) makes the comparison fail.
    #[test]
    fn planted_bugs_of_the_single_byte_paths_are_caught() {
        for n in 7..=9 {
            PLANT.with(|p| p.set(n));
            let caught = std::panic::catch_unwind(single_equals_the_pivot_loop).is_err();
            PLANT.with(|p| p.set(0));
            assert!(caught, "planted bug {n} not caught");
        }
    }
}
