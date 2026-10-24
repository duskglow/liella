//! DES (FIPS 46-3) for `ecb_crypt`/`cbc_crypt`/`des_setparity`/`passwd2des`/`xencrypt`/`xdecrypt`
//! (LG-16, D-29.1): the block cipher, ECB and CBC over 8-byte blocks, the status codes of
//! `<rpc/des_crypt.h>`, the reference's parity convention and its key and hex handling.
//! The tables are those of the standard; bit 1 is the most significant bit of a block.
//!
//! These are DES-based legacy functions, kept so that existing DES-encrypted secret keys still
//! move between systems. DES provides no meaningful security today; new software should use
//! modern authentication. Everything outside the cipher core follows the reference bit for bit
//! (owner ruling #1675, probe-settled on glibc 2.39, see handoff/rpc-xencrypt.md).

/// `DES_MAXDATA`: the most bytes one call takes.
pub const DES_MAXDATA: usize = 8192;
/// `DESERR_NONE`.
pub const DESERR_NONE: i32 = 0;
/// `DESERR_NOHWDEVICE`: done in software, though the hardware was asked for.
pub const DESERR_NOHWDEVICE: i32 = 1;
/// `DESERR_BADPARAM`.
pub const DESERR_BADPARAM: i32 = 3;
/// `DES_DECRYPT` mode bit.
pub const DES_DECRYPT: u32 = 1;
/// `DES_SW` mode bit (clear = `DES_HW`).
pub const DES_SW: u32 = 2;

const IP: [u8; 64] = [
    58, 50, 42, 34, 26, 18, 10, 2, 60, 52, 44, 36, 28, 20, 12, 4, 62, 54, 46, 38, 30, 22, 14, 6, 64, 56, 48, 40, 32, 24, 16, 8, 57, 49, 41, 33, 25, 17, 9, 1, 59, 51, 43,
    35, 27, 19, 11, 3, 61, 53, 45, 37, 29, 21, 13, 5, 63, 55, 47, 39, 31, 23, 15, 7,
];
const FP: [u8; 64] = [
    40, 8, 48, 16, 56, 24, 64, 32, 39, 7, 47, 15, 55, 23, 63, 31, 38, 6, 46, 14, 54, 22, 62, 30, 37, 5, 45, 13, 53, 21, 61, 29, 36, 4, 44, 12, 52, 20, 60, 28, 35, 3, 43,
    11, 51, 19, 59, 27, 34, 2, 42, 10, 50, 18, 58, 26, 33, 1, 41, 9, 49, 17, 57, 25,
];
const E: [u8; 48] = [
    32, 1, 2, 3, 4, 5, 4, 5, 6, 7, 8, 9, 8, 9, 10, 11, 12, 13, 12, 13, 14, 15, 16, 17, 16, 17, 18, 19, 20, 21, 20, 21, 22, 23, 24, 25, 24, 25, 26, 27, 28, 29, 28, 29,
    30, 31, 32, 1,
];
const P: [u8; 32] = [16, 7, 20, 21, 29, 12, 28, 17, 1, 15, 23, 26, 5, 18, 31, 10, 2, 8, 24, 14, 32, 27, 3, 9, 19, 13, 30, 6, 22, 11, 4, 25];
/// Permuted choice 1: never names bits 8, 16, ... 64 (the parity bits are ignored).
const PC1: [u8; 56] = [
    57, 49, 41, 33, 25, 17, 9, 1, 58, 50, 42, 34, 26, 18, 10, 2, 59, 51, 43, 35, 27, 19, 11, 3, 60, 52, 44, 36, 63, 55, 47, 39, 31, 23, 15, 7, 62, 54, 46, 38, 30, 22,
    14, 6, 61, 53, 45, 37, 29, 21, 13, 5, 28, 20, 12, 4,
];
const PC2: [u8; 48] = [
    14, 17, 11, 24, 1, 5, 3, 28, 15, 6, 21, 10, 23, 19, 12, 4, 26, 8, 16, 7, 27, 20, 13, 2, 41, 52, 31, 37, 47, 55, 30, 40, 51, 45, 33, 48, 44, 49, 39, 56, 34, 53,
    46, 42, 50, 36, 29, 32,
];
const SHIFTS: [u32; 16] = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1];
const S: [[u8; 64]; 8] = [
    [
        14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7, 0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12, 11, 9, 5, 3, 8, 4, 1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0,
        15, 12, 8, 2, 4, 9, 1, 7, 5, 11, 3, 14, 10, 0, 6, 13,
    ],
    [
        15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10, 3, 13, 4, 7, 15, 2, 8, 14, 12, 0, 1, 10, 6, 9, 11, 5, 0, 14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15,
        13, 8, 10, 1, 3, 15, 4, 2, 11, 6, 7, 12, 0, 5, 14, 9,
    ],
    [
        10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8, 13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5, 14, 12, 11, 15, 1, 13, 6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7,
        1, 10, 13, 0, 6, 9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12,
    ],
    [
        7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15, 13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2, 12, 1, 10, 14, 9, 10, 6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4,
        3, 15, 0, 6, 10, 1, 13, 8, 9, 4, 5, 11, 12, 7, 2, 14,
    ],
    [
        2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9, 14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15, 10, 3, 9, 8, 6, 4, 2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14,
        11, 8, 12, 7, 1, 14, 2, 13, 6, 15, 0, 9, 10, 4, 5, 3,
    ],
    [
        12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11, 10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13, 14, 0, 11, 3, 8, 9, 14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6,
        4, 3, 2, 12, 9, 5, 15, 10, 11, 14, 1, 7, 6, 0, 8, 13,
    ],
    [
        4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1, 13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5, 12, 2, 15, 8, 6, 1, 4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2,
        6, 11, 13, 8, 1, 4, 10, 7, 9, 5, 0, 15, 14, 2, 3, 12,
    ],
    [
        13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7, 1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6, 11, 0, 14, 9, 2, 7, 11, 4, 1, 9, 12, 14, 2, 0, 6, 10, 13, 15, 3, 5, 8,
        2, 1, 14, 7, 4, 10, 8, 13, 15, 12, 9, 0, 3, 5, 6, 11,
    ],
];

/// The `width`-bit value whose bit `i` (1 = most significant) is bit `table[i]` of the
/// `from`-bit `input`.
fn permute(input: u64, from: u32, table: &[u8]) -> u64 {
    table.iter().fold(0, |out, &t| out.wrapping_shl(1) | (input.wrapping_shr(from.wrapping_sub(u32::from(t))) & 1))
}

/// The 16 round keys (48 bits each) of `key`.
#[derive(Clone, Debug)]
pub struct Schedule([u64; 16]);

/// The key schedule of an 8-byte key (the low bit of each byte, the parity bit, is ignored).
pub fn schedule(key: &[u8; 8]) -> Schedule {
    let cd = permute(u64::from_be_bytes(*key), 64, &PC1);
    let (mut c, mut d) = (cd.wrapping_shr(28) & 0xFFF_FFFF, cd & 0xFFF_FFFF);
    let rot = |v: u64, n: u32| (v.wrapping_shl(n) | v.wrapping_shr(28u32.wrapping_sub(n))) & 0xFFF_FFFF;
    let mut ks = [0u64; 16];
    for (k, &n) in ks.iter_mut().zip(SHIFTS.iter()) {
        c = rot(c, n);
        d = rot(d, n);
        *k = permute(c.wrapping_shl(28) | d, 56, &PC2);
    }
    Schedule(ks)
}

/// The cipher function f(R, K).
fn f(r: u64, k: u64) -> u64 {
    let x = permute(r, 32, &E) ^ k;
    let mut out = 0u64;
    for (i, sbox) in S.iter().enumerate() {
        let shift = 42u32.wrapping_sub(u32::try_from(i).unwrap_or(0).wrapping_mul(6));
        let six = x.wrapping_shr(shift) & 0x3F;
        let row = (six.wrapping_shr(4) & 2) | (six & 1);
        let col = six.wrapping_shr(1) & 0xF;
        let idx = usize::try_from(row.wrapping_mul(16) | col).unwrap_or(0);
        out = out.wrapping_shl(4) | u64::from(sbox.get(idx).copied().unwrap_or(0));
    }
    permute(out, 32, &P)
}

/// One block through the 16 rounds (round keys in reverse order to decrypt).
pub fn block(s: &Schedule, b: u64, decrypt: bool) -> u64 {
    let ip = permute(b, 64, &IP);
    let (mut l, mut r) = (ip.wrapping_shr(32), ip & 0xFFFF_FFFF);
    for i in 0..16usize {
        let k = if decrypt { s.0.get(15usize.wrapping_sub(i)) } else { s.0.get(i) };
        let next = l ^ f(r, k.copied().unwrap_or(0));
        l = r;
        r = next;
    }
    permute(r.wrapping_shl(32) | l, 64, &FP)
}

/// The status of a call with `len` bytes and `mode`: `Err(DESERR_BADPARAM)` when `len` is not a
/// multiple of 8 or above `DES_MAXDATA` (the data is then untouched), else the code to return.
pub fn status(len: usize, mode: u32) -> Result<i32, i32> {
    if len % 8 != 0 || len > DES_MAXDATA {
        return Err(DESERR_BADPARAM);
    }
    Ok(if mode & DES_SW != 0 { DESERR_NONE } else { DESERR_NOHWDEVICE })
}

fn word(c: &[u8]) -> u64 {
    let mut b = [0u8; 8];
    b.iter_mut().zip(c).for_each(|(d, s)| *d = *s);
    u64::from_be_bytes(b)
}

/// ECB over `data` (whole blocks; a trailing partial block is left alone).
pub fn ecb(key: &[u8; 8], data: &mut [u8], decrypt: bool) {
    let s = schedule(key);
    for c in data.chunks_exact_mut(8) {
        c.copy_from_slice(&block(&s, word(c), decrypt).to_be_bytes());
    }
}

/// CBC over `data`; `iv` ends as the last ciphertext block (both directions).
pub fn cbc(key: &[u8; 8], data: &mut [u8], iv: &mut [u8; 8], decrypt: bool) {
    let s = schedule(key);
    let mut chain = u64::from_be_bytes(*iv);
    for c in data.chunks_exact_mut(8) {
        let w = word(c);
        let out = if decrypt { block(&s, w, true) ^ chain } else { block(&s, w ^ chain, false) };
        chain = if decrypt { w } else { out };
        c.copy_from_slice(&out.to_be_bytes());
    }
    *iv = chain.to_be_bytes();
}

/// The reference's `des_setparity`: bit 0x80 of each byte is cleared and the low bit set so
/// that the byte has odd parity; bits 0x02..=0x40 are kept (probe-settled, all 256 values).
pub fn setparity(key: &mut [u8; 8]) {
    for b in key.iter_mut() {
        let kept = *b & 0x7E;
        *b = if kept.count_ones() % 2 == 0 { kept | 1 } else { kept };
    }
}

/// `passwd2des`: byte `i` of the key is password byte `i` shifted left by one, for the first
/// eight bytes up to the NUL (the rest is ignored, the key is zero beyond a shorter password),
/// then [`setparity`].
pub fn passwd2des(pw: &[u8]) -> [u8; 8] {
    let mut k = [0u8; 8];
    k.iter_mut().zip(pw.iter().take_while(|&&c| c != 0)).for_each(|(d, &c)| *d = c.wrapping_shl(1));
    setparity(&mut k);
    k
}

/// The value `xencrypt`/`xdecrypt` read for a hex digit: `0`-`9`, then `a`-`z` and `A`-`Z` as
/// 10..=35, anything else -1 (probe-settled for every byte).
fn digit(c: u8) -> i32 {
    match c {
        b'0'..=b'9' => i32::from(c.wrapping_sub(b'0')),
        b'a'..=b'z' => i32::from(c.wrapping_sub(b'a')).wrapping_add(10),
        b'A'..=b'Z' => i32::from(c.wrapping_sub(b'A')).wrapping_add(10),
        _ => -1,
    }
}

/// The byte a digit pair decodes to: 16 times the first value plus the second, kept to 8 bits.
fn pair(hi: u8, lo: u8) -> u8 {
    u8::try_from(digit(hi).wrapping_shl(4).wrapping_add(digit(lo)) & 0xFF).unwrap_or(0)
}

const HEX: &[u8; 16] = b"0123456789abcdef";

/// `xencrypt`/`xdecrypt` over the hex text `secret` (its bytes before the NUL): the first
/// `len / 2` digit pairs are decoded, run through CBC with a zero IV under
/// [`passwd2des`]`(pw)`, and written back as lower-case hex. Returns the number of hex digits
/// written (the caller ends the text there), or `None`, with `secret` untouched, when `len / 2`
/// is not a multiple of 8 or exceeds [`DES_MAXDATA`].
pub fn xcrypt(secret: &mut [u8], pw: &[u8], decrypt: bool) -> Option<usize> {
    let n = secret.len().wrapping_shr(1);
    status(n, DES_SW).ok()?;
    let mut buf = [0u8; DES_MAXDATA];
    let data = buf.get_mut(..n)?;
    for (d, p) in data.iter_mut().zip(secret.chunks_exact(2)) {
        *d = pair(p.first().copied().unwrap_or(0), p.get(1).copied().unwrap_or(0));
    }
    cbc(&passwd2des(pw), data, &mut [0u8; 8], decrypt);
    for (p, &b) in secret.chunks_exact_mut(2).zip(data.iter()) {
        let hi = HEX.get(usize::from(b.wrapping_shr(4))).copied().unwrap_or(b'0');
        let lo = HEX.get(usize::from(b & 0xF)).copied().unwrap_or(b'0');
        p.copy_from_slice(&[hi, lo]);
    }
    Some(n.wrapping_mul(2))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(s: &str) -> [u8; 8] {
        u64::from_str_radix(s, 16).unwrap_or(0).to_be_bytes()
    }

    /// FIPS 46 / SP 800-17 published vectors: (key, plaintext, ciphertext).
    const VECTORS: [(&str, &str, &str); 5] = [
        ("133457799BBCDFF1", "0123456789ABCDEF", "85E813540F0AB405"),
        ("0123456789ABCDEF", "4E6F772069732074", "3FA40E8A984D4815"),
        ("0101010101010101", "95F8A5E5DD31D900", "8000000000000000"),
        ("0101010101010101", "8000000000000000", "95F8A5E5DD31D900"),
        ("8001010101010101", "0000000000000000", "95A8D72813DAA94D"),
    ];

    #[test]
    fn fips_vectors() {
        for (k, p, c) in VECTORS {
            let mut b = h(p);
            ecb(&h(k), &mut b, false);
            assert_eq!(b, h(c), "encrypt {k}");
            ecb(&h(k), &mut b, true);
            assert_eq!(b, h(p), "decrypt {k}");
        }
    }

    #[test]
    fn parity_bits_ignored() {
        let mut k = h("133457799BBCDFF1");
        k.iter_mut().for_each(|b| *b ^= 1);
        let mut b = h("0123456789ABCDEF");
        ecb(&k, &mut b, false);
        assert_eq!(b, h("85E813540F0AB405"));
    }

    #[test]
    fn setparity_reference() {
        // Probe-settled: bit 0x80 cleared, odd parity in the low bit.
        let mut k = h("133457799BBCDFF0");
        setparity(&mut k);
        assert_eq!(k, h("133457791A3D5E70"));
        let mut z = [0u8; 8];
        setparity(&mut z);
        assert_eq!(z, [1u8; 8]);
        let mut f = [0xFFu8; 8];
        setparity(&mut f);
        assert_eq!(f, [0x7Fu8; 8]);
    }

    #[test]
    fn passwd2des_reference() {
        // Probe-settled keys.
        assert_eq!(passwd2des(b""), [1u8; 8]);
        assert_eq!(passwd2des(b"abcdefghZZZZ"), h("434546494A4C4F51"));
        assert_eq!(passwd2des(b"hello world"), h("514A58585E406E5E"));
        assert_eq!(passwd2des(b"\xff"), h("7F01010101010101"));
    }

    #[test]
    fn xcrypt_reference() {
        // Probe-settled texts (password "pw").
        let mut s = *b"0123456789ABCDEF0";
        assert_eq!(xcrypt(&mut s, b"pw", false), Some(16));
        assert_eq!(&s[..16], b"ea4d2a2f7ffbd41a");
        let mut s = *b"0123456789abcdez";
        assert_eq!(xcrypt(&mut s, b"pw", false), Some(16));
        assert_eq!(&s, b"b772f08839c4cb19");
        let mut s = *b"0123456789abcde";
        assert_eq!(xcrypt(&mut s, b"pw", true), None);
        assert_eq!(&s, b"0123456789abcde");
    }
}
