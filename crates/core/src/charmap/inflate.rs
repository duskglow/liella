//! Gzip decompression (RFC 1952 around RFC 1951 deflate), for the installed charmaps
//! (D-6.8). Malformed input, a CRC or length mismatch, or output beyond the caller's limit is
//! an error; nothing here panics.

#![allow(clippy::arithmetic_side_effects)] // bit counts below 32, lengths below the output limit

use crate::error::{Errno, Result};
use crate::mem::TryVec;

/// Bits of the input, least significant first.
struct Bits<'a> {
    data: &'a [u8],
    pos: usize,
    bit: u32,
}

impl Bits<'_> {
    fn bit(&mut self) -> Result<u32> {
        let byte = *self.data.get(self.pos).ok_or(Errno::EINVAL)?;
        let v = u32::from(byte >> self.bit) & 1;
        self.bit += 1;
        if self.bit == 8 {
            self.bit = 0;
            self.pos += 1;
        }
        Ok(v)
    }

    fn bits(&mut self, n: u32) -> Result<u32> {
        let mut v = 0;
        for i in 0..n {
            v |= self.bit()? << i;
        }
        Ok(v)
    }

    fn align(&mut self) {
        if self.bit != 0 {
            self.bit = 0;
            self.pos += 1;
        }
    }
}

/// A canonical Huffman code: the number of codes of each length, and the symbols in code order.
struct Huffman {
    counts: [u16; 16],
    symbols: [u16; 320],
}

impl Huffman {
    fn new(lengths: &[u8]) -> Result<Huffman> {
        let mut h = Huffman { counts: [0; 16], symbols: [0; 320] };
        for &l in lengths {
            *h.counts.get_mut(usize::from(l)).ok_or(Errno::EINVAL)? += 1;
        }
        h.counts[0] = 0;
        // offs[k]: the first symbol slot of length k (the codes of shorter lengths before it).
        let mut offs = [0u16; 16];
        let mut acc = 0u16;
        for (o, c) in offs.iter_mut().skip(1).zip(h.counts.iter()) {
            acc += c;
            *o = acc;
        }
        for (sym, &l) in lengths.iter().enumerate() {
            if l != 0 {
                let o = offs.get_mut(usize::from(l)).ok_or(Errno::EINVAL)?;
                *h.symbols.get_mut(usize::from(*o)).ok_or(Errno::EINVAL)? = u16::try_from(sym).map_err(|_| Errno::EINVAL)?;
                *o += 1;
            }
        }
        Ok(h)
    }

    fn decode(&self, b: &mut Bits<'_>) -> Result<u16> {
        let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
        for len in 1..16 {
            code |= i32::try_from(b.bit()?).map_err(|_| Errno::EINVAL)?;
            let count = i32::from(self.counts.get(len).copied().unwrap_or(0));
            if code - count < first {
                let i = usize::try_from(index + (code - first)).map_err(|_| Errno::EINVAL)?;
                return self.symbols.get(i).copied().ok_or(Errno::EINVAL);
            }
            index += count;
            first += count;
            first <<= 1;
            code <<= 1;
        }
        Err(Errno::EINVAL)
    }
}

const LEN_BASE: [u16; 29] = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163, 195, 227, 258];
const LEN_EXTRA: [u8; 29] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0];
const DIST_BASE: [u16; 30] = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073, 4097, 6145, 8193, 12289, 16385, 24577];
const DIST_EXTRA: [u8; 30] = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13];

fn put(out: &mut TryVec<u8>, b: u8, limit: usize) -> Result<()> {
    if out.len() >= limit {
        return Err(Errno::EFBIG);
    }
    out.push(b)
}

fn block(b: &mut Bits<'_>, out: &mut TryVec<u8>, lit: &Huffman, dist: &Huffman, limit: usize) -> Result<()> {
    loop {
        let sym = lit.decode(b)?;
        match sym {
            0..=255 => put(out, u8::try_from(sym).map_err(|_| Errno::EINVAL)?, limit)?,
            256 => return Ok(()),
            _ => {
                let i = usize::from(sym - 257);
                let extra = u32::from(*LEN_EXTRA.get(i).ok_or(Errno::EINVAL)?);
                let len = usize::from(*LEN_BASE.get(i).ok_or(Errno::EINVAL)?) + usize::try_from(b.bits(extra)?).map_err(|_| Errno::EINVAL)?;
                let d = usize::from(dist.decode(b)?);
                let dextra = u32::from(*DIST_EXTRA.get(d).ok_or(Errno::EINVAL)?);
                let back = usize::from(*DIST_BASE.get(d).ok_or(Errno::EINVAL)?) + usize::try_from(b.bits(dextra)?).map_err(|_| Errno::EINVAL)?;
                let start = out.len().checked_sub(back).ok_or(Errno::EINVAL)?;
                for k in 0..len {
                    let v = *out.get(start + k).ok_or(Errno::EINVAL)?;
                    put(out, v, limit)?;
                }
            }
        }
    }
}

fn fixed() -> Result<(Huffman, Huffman)> {
    let mut l = [0u8; 288];
    for (i, v) in l.iter_mut().enumerate() {
        *v = match i {
            0..=143 => 8,
            144..=255 => 9,
            256..=279 => 7,
            _ => 8,
        };
    }
    Ok((Huffman::new(&l)?, Huffman::new(&[5u8; 30])?))
}

fn dynamic(b: &mut Bits<'_>) -> Result<(Huffman, Huffman)> {
    let hlit = usize::try_from(b.bits(5)?).map_err(|_| Errno::EINVAL)? + 257;
    let hdist = usize::try_from(b.bits(5)?).map_err(|_| Errno::EINVAL)? + 1;
    let hclen = usize::try_from(b.bits(4)?).map_err(|_| Errno::EINVAL)? + 4;
    const ORDER: [usize; 19] = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15];
    let mut cl = [0u8; 19];
    for &o in ORDER.iter().take(hclen) {
        *cl.get_mut(o).ok_or(Errno::EINVAL)? = u8::try_from(b.bits(3)?).map_err(|_| Errno::EINVAL)?;
    }
    let clh = Huffman::new(&cl)?;
    let mut lengths = [0u8; 320];
    let mut n = 0usize;
    while n < hlit + hdist {
        let sym = clh.decode(b)?;
        let (value, repeat) = match sym {
            0..=15 => (u8::try_from(sym).map_err(|_| Errno::EINVAL)?, 1),
            16 => (*lengths.get(n.checked_sub(1).ok_or(Errno::EINVAL)?).ok_or(Errno::EINVAL)?, 3 + b.bits(2)?),
            17 => (0, 3 + b.bits(3)?),
            _ => (0, 11 + b.bits(7)?),
        };
        for _ in 0..repeat {
            *lengths.get_mut(n).ok_or(Errno::EINVAL)? = value;
            n += 1;
        }
    }
    if n != hlit + hdist {
        return Err(Errno::EINVAL);
    }
    let lit = Huffman::new(lengths.get(..hlit).ok_or(Errno::EINVAL)?)?;
    let dist = Huffman::new(lengths.get(hlit..hlit + hdist).ok_or(Errno::EINVAL)?)?;
    Ok((lit, dist))
}

/// Inflate a raw deflate stream; returns the output and the input bytes consumed.
pub fn inflate(data: &[u8], limit: usize) -> Result<(TryVec<u8>, usize)> {
    let mut out = TryVec::new();
    let used = inflate_into(data, &mut out, limit)?;
    Ok((out, used))
}

/// Inflate into `out`, which keeps what was produced when an error stops the stream.
fn inflate_into(data: &[u8], out: &mut TryVec<u8>, limit: usize) -> Result<usize> {
    let mut b = Bits { data, pos: 0, bit: 0 };
    let out = &mut *out;
    loop {
        let last = b.bit()?;
        match b.bits(2)? {
            0 => {
                b.align();
                let p = b.pos;
                let len = u16::from_le_bytes([*data.get(p).ok_or(Errno::EINVAL)?, *data.get(p + 1).ok_or(Errno::EINVAL)?]);
                let nlen = u16::from_le_bytes([*data.get(p + 2).ok_or(Errno::EINVAL)?, *data.get(p + 3).ok_or(Errno::EINVAL)?]);
                if len != !nlen {
                    return Err(Errno::EINVAL);
                }
                let body = data.get(p + 4..p + 4 + usize::from(len)).ok_or(Errno::EINVAL)?;
                for &v in body {
                    put(out, v, limit)?;
                }
                b.pos = p + 4 + usize::from(len);
            }
            1 => {
                let (l, d) = fixed()?;
                block(&mut b, out, &l, &d, limit)?;
            }
            2 => {
                let (l, d) = dynamic(&mut b)?;
                block(&mut b, out, &l, &d, limit)?;
            }
            _ => return Err(Errno::EINVAL),
        }
        if last == 1 {
            b.align();
            return Ok(b.pos);
        }
    }
}

/// The CRC-32 (IEEE 802.3) of `data`.
pub fn crc32(data: &[u8]) -> u32 {
    let mut crc = !0u32;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
        }
    }
    !crc
}

/// The start of a gzip file's text: at most `limit` bytes, from the first bytes of the file
/// (no checks beyond the header: a charmap's header names, D-6.8).
pub fn gunzip_prefix(data: &[u8], limit: usize) -> Result<TryVec<u8>> {
    let p = member_start(data)?;
    let mut out = TryVec::new();
    // Running out of input or reaching the limit ends the prefix.
    let _ = inflate_into(data.get(p..).ok_or(Errno::EINVAL)?, &mut out, limit);
    Ok(out)
}

/// The offset of the deflate stream after a gzip header.
fn member_start(data: &[u8]) -> Result<usize> {
    if data.get(..3) != Some(&[0x1f, 0x8b, 8]) {
        return Err(Errno::EINVAL);
    }
    let flags = *data.get(3).ok_or(Errno::EINVAL)?;
    let mut p = 10usize;
    if flags & 4 != 0 {
        let xlen = usize::from(u16::from_le_bytes([*data.get(p).ok_or(Errno::EINVAL)?, *data.get(p + 1).ok_or(Errno::EINVAL)?]));
        p += 2 + xlen;
    }
    for flag in [8u8, 16] {
        if flags & flag != 0 {
            let rest = data.get(p..).ok_or(Errno::EINVAL)?;
            p += rest.iter().position(|&b| b == 0).ok_or(Errno::EINVAL)? + 1;
        }
    }
    if flags & 2 != 0 {
        p += 2;
    }
    Ok(p)
}

/// Decompress a gzip file (one member), checking its CRC-32 and length. At most `limit`
/// output bytes.
pub fn gunzip(data: &[u8], limit: usize) -> Result<TryVec<u8>> {
    let p = member_start(data)?;
    let (out, used) = inflate(data.get(p..).ok_or(Errno::EINVAL)?, limit)?;
    let t = p + used;
    let trailer = data.get(t..t + 8).ok_or(Errno::EINVAL)?;
    let word = |at: usize| -> Result<u32> { trailer.get(at..at + 4).and_then(|w| <[u8; 4]>::try_from(w).ok()).map(u32::from_le_bytes).ok_or(Errno::EINVAL) };
    let (crc, size) = (word(0)?, word(4)?);
    // ISIZE is the length modulo 2^32.
    let len32 = u32::try_from(out.len() % (1usize << 32)).map_err(|_| Errno::EINVAL)?;
    if crc != crc32(&out) || size != len32 {
        return Err(Errno::EINVAL);
    }
    Ok(out)
}
