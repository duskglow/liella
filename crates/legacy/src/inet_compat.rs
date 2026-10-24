//! The historical address texts of spec §15.3 and the RFC 2292 option helpers of §15.5: C
//! contracts the clean API does not offer (D-15.2). Written under the clean-room rules of
//! implementation-plan.md §1.4 from the spec, the Linux man pages (`inet(3)`,
//! `inet_net_pton(3)`), RFC 1706 (NSAP text), RFC 2292 §6.3 and `spec/behaviour/netaddr.md`
//! with probes of the reference.
//!
//! Values are host-order `u32`s; the boundary converts `struct in_addr` (network order).

use liella::net::{hex_digit, hex_value, is_space, Text};

/// A number of the numbers-and-dots notation (`inet(3)`): `0x`/`0X` and hexadecimal digits,
/// `0` and octal digits, or decimal digits. Its value (`None` beyond 32 bits) and length.
fn number(text: &[u8]) -> Option<(u64, usize)> {
    let (base, mut i) = match text {
        [b'0', b'x' | b'X', ..] => (16u64, 2usize),
        [b'0', ..] => (8, 1),
        [b'1'..=b'9', ..] => (10, 0),
        _ => return None,
    };
    let start = i;
    let mut v: u64 = 0;
    while let Some(&c) = text.get(i) {
        let d = match hex_value(c) {
            Some(d) if u64::from(d) < base => u64::from(d),
            _ => break,
        };
        v = v.checked_mul(base)?.checked_add(d)?;
        if v > u64::from(u32::MAX) {
            return None;
        }
        i = i.saturating_add(1);
    }
    // "0x" needs a digit; a lone "0" is octal zero.
    if base == 16 && i == start {
        return None;
    }
    Some((v, i))
}

/// The parts of a numbers-and-dots text (at most four) and where they end.
fn parts(text: &[u8]) -> Option<([u64; 4], usize, usize)> {
    let mut v = [0u64; 4];
    let mut n = 0usize;
    let mut i = 0usize;
    loop {
        let (x, len) = number(text.get(i..)?)?;
        *v.get_mut(n)? = x;
        n = n.saturating_add(1);
        i = i.saturating_add(len);
        if text.get(i) == Some(&b'.') {
            if n == 4 {
                return None;
            }
            i = i.saturating_add(1);
            continue;
        }
        return Some((v, n, i));
    }
}

/// `inet_aton` (§15.3): one to four parts in octal, decimal or hexadecimal (`a`: 32 bits;
/// `a.b`: 8 and 24; `a.b.c`: 8, 8 and 16; `a.b.c.d`: 8 each), ending at the end of the text or
/// at white space (the rest of the text is not read, as the reference: `"1.2.3.4 x"` is valid).
/// Kept by decision (D-15.2): the reference accepts the address field of a longer line, callers
/// may rely on it, and the contract's leniency is the function's purpose; the strict form is
/// `inet_pton`'s.
pub fn aton(text: &[u8]) -> Option<u32> {
    let (v, n, end) = parts(text)?;
    if text.get(end).is_some_and(|&c| !is_space(c)) {
        return None;
    }
    let [a, b, c, d] = v;
    let ok = |x: u64, bits: u32| x < 1u64.wrapping_shl(bits);
    let r = match n {
        1 => a,
        2 if ok(a, 8) && ok(b, 24) => a << 24 | b,
        3 if ok(a, 8) && ok(b, 8) && ok(c, 16) => a << 24 | b << 16 | c,
        4 if [a, b, c, d].iter().all(|&x| ok(x, 8)) => a << 24 | b << 16 | c << 8 | d,
        _ => return None,
    };
    u32::try_from(r).ok()
}

/// `AF_INET6`.
const AF_INET6: i32 = 10;

/// `__nss_hostname_digits_dots`'s reading of `name` (§16.1): `None` when it is no numeric name
/// (the function does nothing); `Some(None)` when it is one but gives no address of `af`'s
/// family (`AF_INET6`, any other value `AF_INET`); `Some(Some((family, bytes, length)))`.
/// Numeric: only digits and dots, not ending in a dot (read as [`aton`] reads it), or
/// hexadecimal digits, colons and dots with a colon (an IPv6 literal).
pub fn digits_dots(name: &[u8], af: i32) -> Option<Option<(i32, [u8; 16], usize)>> {
    let v4 = !name.is_empty() && name.iter().all(|&c| c.is_ascii_digit() || c == b'.') && name.last() != Some(&b'.');
    let v6 = name.contains(&b':') && name.iter().all(|&c| c.is_ascii_hexdigit() || c == b':' || c == b'.');
    if !v4 && !v6 {
        return None;
    }
    let want6 = af == AF_INET6;
    Some(match (v4, want6) {
        (true, false) => aton(name).map(|a| {
            let mut b = [0u8; 16];
            if let Some(d) = b.get_mut(..4) {
                d.copy_from_slice(&a.to_be_bytes());
            }
            (2, b, 4)
        }),
        (false, true) => liella::net::addr::parse_v6(name).map(|b| (AF_INET6, b, 16)),
        _ => None,
    })
}

/// `inet_network` (§15.3): a network number, one to four parts of one byte each (same number
/// syntax as [`aton`]), right-aligned (`"10.1"` is `0x0a01`), followed by nothing but white
/// space. A part beyond 255 is invalid (the reference wraps a part beyond 32 bits modulo 2³²
/// and so accepts `4294967296` as 0; Liella treats it as the out-of-range part it is).
pub fn network(text: &[u8]) -> Option<u32> {
    let (v, n, end) = parts(text)?;
    if !text.get(end..)?.iter().all(|&c| is_space(c)) {
        return None;
    }
    let mut r: u32 = 0;
    for &p in v.get(..n)? {
        r = r << 8 | u32::from(u8::try_from(p).ok()?);
    }
    Some(r)
}

/// The class of an address by its leading bits: A (`0`), B (`10`), else C (classes D and E
/// split as C, as the reference does). The network part's width in bits.
fn host_bits(a: u32) -> u32 {
    if a & 0x8000_0000 == 0 {
        24
    } else if a & 0xc000_0000 == 0x8000_0000 {
        16
    } else {
        8
    }
}

/// `inet_lnaof` (§15.3): the host part of `a` by its class.
pub fn lnaof(a: u32) -> u32 {
    a & 1u32.wrapping_shl(host_bits(a)).wrapping_sub(1)
}

/// `inet_netof` (§15.3): the network part of `a` by its class, right-aligned.
pub fn netof(a: u32) -> u32 {
    a.wrapping_shr(host_bits(a))
}

/// `inet_makeaddr(net, host)` (§15.3): class A for a network number below 128, B below 65536,
/// C below 2²⁴; a larger number is combined with the host part as is.
pub fn makeaddr(net: u32, host: u32) -> u32 {
    match net {
        0..=0x7f => net << 24 | (host & 0x00ff_ffff),
        0x80..=0xffff => net << 16 | (host & 0xffff),
        0x1_0000..=0xff_ffff => net << 8 | (host & 0xff),
        _ => net | host,
    }
}

/// Errors of the CIDR texts.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NetError {
    /// `ENOENT`: not a network text.
    Syntax,
    /// `EMSGSIZE`: the buffer is too small, or the prefix beyond 32.
    Size,
}

/// `inet_net_pton(AF_INET, text, out)` (§15.3, `inet_net_pton(3)`): the prefix length. The
/// bytes are written as they are read, so a failed call can leave some. Forms: `0x` and
/// hexadecimal digits filling the bytes from the left, or one to four decimal parts of 0–255;
/// then optionally `/` and a prefix length of 0–32. Without one, the length comes from the
/// first byte (240+: 32, 224+: 4, 192+: 24, 128+: 16, else 8: the class, `inet_net_pton(3)`)
/// and grows to cover every byte given, except class D's 4 (observed: `224.1.2.3` is 4,
/// `128.1.2.3` is 32); bytes between those given and the prefix length are written as zero.
pub fn net_pton(text: &[u8], out: &mut [u8]) -> Result<i32, NetError> {
    let mut put = |k: usize, v: u8| -> Result<(), NetError> {
        *out.get_mut(k).ok_or(NetError::Size)? = v;
        Ok(())
    };
    let mut i = 0usize;
    let mut bytes = 0usize;
    let mut first = 0u8;
    if matches!(text, [b'0', b'x' | b'X', c, ..] if c.is_ascii_hexdigit()) {
        i = 2;
        let mut pending: Option<u8> = None;
        while let Some(d) = text.get(i).copied().and_then(hex_value) {
            i = i.saturating_add(1);
            match pending.take() {
                None => pending = Some(d),
                Some(hi) => {
                    let v = hi << 4 | d;
                    if bytes == 0 {
                        first = v;
                    }
                    put(bytes, v)?;
                    bytes = bytes.saturating_add(1);
                }
            }
        }
        if let Some(hi) = pending {
            if bytes == 0 {
                first = hi << 4;
            }
            put(bytes, hi << 4)?;
            bytes = bytes.saturating_add(1);
        }
    } else if text.first().is_some_and(u8::is_ascii_digit) {
        loop {
            let digits = text.get(i..).unwrap_or(&[]).iter().take_while(|c| c.is_ascii_digit()).count();
            if digits == 0 {
                return Err(NetError::Syntax);
            }
            let mut v: u32 = 0;
            for &c in text.get(i..i.saturating_add(digits)).unwrap_or(&[]) {
                v = v.saturating_mul(10).saturating_add(u32::from(c.wrapping_sub(b'0')));
            }
            let v = u8::try_from(v).map_err(|_| NetError::Syntax)?;
            i = i.saturating_add(digits);
            if bytes == 0 {
                first = v;
            }
            put(bytes, v)?;
            bytes = bytes.saturating_add(1);
            if text.get(i) != Some(&b'.') {
                break;
            }
            i = i.saturating_add(1);
        }
    } else {
        return Err(NetError::Syntax);
    }
    let given = bytes.saturating_mul(8);
    let bits = match text.get(i) {
        None => {
            let class = match first {
                240..=255 => 32,
                224..=239 => 4,
                192..=223 => 24,
                128..=191 => 16,
                _ => 8,
            };
            if class >= 8 && given > class { given } else { class }
        }
        Some(b'/') => {
            let tail = text.get(i.saturating_add(1)..).unwrap_or(&[]);
            if tail.is_empty() || !tail.iter().all(u8::is_ascii_digit) {
                return Err(NetError::Syntax);
            }
            let mut v: usize = 0;
            for &c in tail {
                v = v.saturating_mul(10).saturating_add(usize::from(c.wrapping_sub(b'0')));
            }
            if v > 32 {
                return Err(NetError::Size);
            }
            v
        }
        Some(_) => return Err(NetError::Syntax),
    };
    let need = bits.div_ceil(8);
    while bytes < need {
        put(bytes, 0)?;
        bytes = bytes.saturating_add(1);
    }
    i32::try_from(bits).map_err(|_| NetError::Size)
}

/// `inet_net_ntop(AF_INET, src, bits)` (§15.3): the bytes the prefix covers (at least one), the
/// last masked to the prefix, then `/bits`. `None` for a prefix outside 0–32 (`EINVAL`).
pub fn net_ntop(src: &[u8], bits: i32) -> Option<Text<20>> {
    let bits = u32::try_from(bits).ok().filter(|&b| b <= 32)?;
    let n = usize::try_from(bits.div_ceil(8).max(1)).ok()?;
    let mut t = Text::new();
    for k in 0..n {
        let mut b = *src.get(k)?;
        let covered = bits.saturating_sub(u32::try_from(k).ok()?.saturating_mul(8));
        if covered < 8 {
            b &= u8::try_from(0xff00u32.wrapping_shr(covered) & 0xff).unwrap_or(0);
        }
        if k > 0 {
            t.push(b'.');
        }
        t.push_dec(u64::from(b));
    }
    t.push(b'/');
    t.push_dec(u64::from(bits));
    Some(t)
}

/// `inet_neta(src)` (§15.3): a network number's text, its leading zero bytes omitted (`0x0a14`
/// is `10.20`), `0.0.0.0` for zero. Every remaining byte is printed: the reference also drops
/// zero bytes after the first (`0x0a000100` prints as `10.1`), which makes distinct numbers
/// print alike; Liella prints `10.0.1.0`.
pub fn neta(v: u32) -> Text<16> {
    let mut t = Text::new();
    if v == 0 {
        t.push_all(b"0.0.0.0");
        return t;
    }
    let b = v.to_be_bytes();
    let start = b.iter().position(|&x| x != 0).unwrap_or(0);
    for (k, x) in b.iter().enumerate().skip(start) {
        if k > start {
            t.push(b'.');
        }
        t.push_dec(u64::from(*x));
    }
    t
}

/// `inet_nsap_addr(text, buf)` (§15.3): the bytes of an NSAP address's hexadecimal digits,
/// written as they are read; reading stops when `out` is full. Digit pairs, with `.`, `+` and
/// `/` allowed between pairs (as the reference). Any other character (white space, a `0x`
/// prefix: the presentation form of RFC 1706 is the caller's to strip, as the reference
/// expects), or an odd number of digits, makes the result 0.
pub fn nsap_addr(text: &[u8], out: &mut [u8]) -> usize {
    let mut n = 0usize;
    let mut it = text.iter().copied();
    while n < out.len() {
        let Some(c) = it.next() else { break };
        if matches!(c, b'.' | b'+' | b'/') {
            continue;
        }
        let (Some(hi), Some(lo)) = (hex_value(c), it.next().and_then(hex_value)) else { return 0 };
        if let Some(slot) = out.get_mut(n) {
            *slot = hi << 4 | lo;
        }
        n = n.saturating_add(1);
    }
    n
}

/// The most bytes `nsap_ntoa` formats (more are ignored, as the reference does with either
/// buffer).
pub const NSAP_MAX: usize = 255;

/// `inet_nsap_ntoa(len, bytes, buf)` (§15.3): uppercase digit pairs, the first byte alone and
/// then a `.` before every second byte (`47.0005.80FF`), into `out` (which the caller sizes:
/// two digits per byte, a dot per two bytes, the terminator). The bytes written, with the
/// terminator.
pub fn nsap_ntoa(bytes: &[u8], out: &mut [u8]) -> usize {
    let mut at = 0usize;
    let mut put = |c: u8| {
        if let Some(slot) = out.get_mut(at) {
            *slot = c;
            at = at.saturating_add(1);
        }
    };
    for (k, &b) in bytes.iter().enumerate() {
        if k % 2 == 1 {
            put(b'.');
        }
        put(hex_digit(u32::from(b >> 4), true));
        put(hex_digit(u32::from(b & 0xf), true));
    }
    put(0);
    at
}

/// The size of `nsap_ntoa`'s text for `n` bytes, with the terminator.
pub fn nsap_text_len(n: usize) -> usize {
    n.saturating_mul(2).saturating_add(n / 2).saturating_add(1)
}

// ------------------------------------------------------------------ RFC 2292 options (§15.5)
//
// An ancillary-data object: `struct cmsghdr` (`cmsg_len` as `size_t`, `cmsg_level`,
// `cmsg_type`; 16 bytes) followed by an IPv6 extension header that grows as options are
// added: each option is placed at an offset `multx * k + plusy` past the padding, and the
// header is padded to a multiple of 8 after each addition.

/// `sizeof(struct cmsghdr)`.
pub const CMSG_HDR: usize = 16;
/// `IPPROTO_IPV6`.
const IPPROTO_IPV6: i32 = 41;
/// `IPV6_HOPOPTS`.
pub const IPV6_HOPOPTS: i32 = 54;
/// `IPV6_DSTOPTS`.
pub const IPV6_DSTOPTS: i32 = 59;

/// `inet6_option_space(nbytes)`: an object for one option of `nbytes` (alignment padding, type
/// and length included): the header, the extension header's 2 bytes and the option, rounded to
/// 8.
pub fn option_space(nbytes: i32) -> i32 {
    let n = usize::try_from(nbytes).unwrap_or(0);
    let r = n.saturating_add(2).checked_next_multiple_of(8).unwrap_or(usize::MAX).saturating_add(CMSG_HDR);
    i32::try_from(r).unwrap_or(i32::MAX)
}

/// `inet6_option_init(bp, &cmsgp, type)`: the header of an empty object in `head` (16 bytes).
pub fn option_init(head: &mut [u8], ty: i32) -> Option<()> {
    if ty != IPV6_HOPOPTS && ty != IPV6_DSTOPTS {
        return None;
    }
    head.get_mut(0..8)?.copy_from_slice(&u64::try_from(CMSG_HDR).ok()?.to_ne_bytes());
    head.get_mut(8..12)?.copy_from_slice(&IPPROTO_IPV6.to_ne_bytes());
    head.get_mut(12..16)?.copy_from_slice(&ty.to_ne_bytes());
    Some(())
}

/// The object's current length (`cmsg_len`), read from its first 8 bytes.
pub fn cmsg_len(head: &[u8]) -> Option<usize> {
    let b: [u8; 8] = head.get(..8)?.try_into().ok()?;
    usize::try_from(u64::from_ne_bytes(b)).ok()
}

/// Where an option of `optlen` bytes aligned `multx * k + plusy` goes in an object of
/// `cmsg_len` bytes: (the option's offset, the object's new length). `None` for an alignment
/// RFC 2292 does not allow (`multx` 1, 2, 4 or 8; `plusy` 0–7) or a header beyond 2048 bytes.
pub fn option_place(cmsg_len: usize, optlen: usize, multx: i32, plusy: i32) -> Option<(usize, usize)> {
    let multx = usize::try_from(multx).ok().filter(|m| [1, 2, 4, 8].contains(m))?;
    let plusy = usize::try_from(plusy).ok().filter(|&p| p <= 7)?;
    let ext = cmsg_len.checked_sub(CMSG_HDR)?;
    let off = if ext == 0 { 2 } else { ext };
    let at = off.checked_next_multiple_of(multx)?.checked_add(plusy)?;
    let end = at.checked_add(optlen)?;
    let total = end.checked_next_multiple_of(8)?;
    if total > 2048 {
        return None;
    }
    Some((CMSG_HDR.checked_add(at)?, CMSG_HDR.checked_add(total)?))
}

/// Pad1 or PadN over `area`.
fn pad(area: &mut [u8]) {
    area.fill(0);
    let n = area.len();
    if n >= 2 {
        if let Some(t) = area.first_mut() {
            *t = 1;
        }
        if let Some(l) = area.get_mut(1) {
            *l = u8::try_from(n.saturating_sub(2)).unwrap_or(0);
        }
    }
}

/// Lay out an option of `optlen` bytes at `at` in `obj` (the whole object, `new_len` bytes,
/// as [`option_place`] gave): the padding before and after it, the extension header's length
/// byte and `cmsg_len`. The option's own bytes are the caller's (`option_append` copies them,
/// `option_alloc` leaves them).
pub fn option_layout(obj: &mut [u8], old_len: usize, at: usize, optlen: usize, new_len: usize) -> Option<()> {
    let ext_old = old_len.checked_sub(CMSG_HDR)?;
    let pad_from = CMSG_HDR.checked_add(if ext_old == 0 { 2 } else { ext_old })?;
    pad(obj.get_mut(pad_from..at)?);
    let end = at.checked_add(optlen)?;
    pad(obj.get_mut(end..new_len)?);
    let units = new_len.checked_sub(CMSG_HDR)?.checked_div(8)?.checked_sub(1)?;
    *obj.get_mut(CMSG_HDR.checked_add(1)?)? = u8::try_from(units).ok()?;
    obj.get_mut(0..8)?.copy_from_slice(&u64::try_from(new_len).ok()?.to_ne_bytes());
    Some(())
}

/// The outcome of a walk step of `inet6_option_next`/`find`. RFC 2292 §6.3.5–6.3.6: at the end
/// of the options (or when the type is not found) `*tptrp` becomes NULL, after an error it is
/// left as it was. The reference leaves `*tptrp` unchanged in both cases (probes, phase 10);
/// Liella follows the RFC the spec names.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Step {
    /// An option at this offset in the object.
    At(usize),
    /// No more options (`*tptrp` becomes NULL).
    End,
    /// A malformed object, or a `*tptrp` that is not an option of it (`*tptrp` is left as it
    /// was).
    Error,
}

/// The option after the one at `prev` (`None`: the first) in `obj` (the whole object). Pad
/// options are returned too, as the reference returns them.
pub fn option_next(obj: &[u8], prev: Option<usize>) -> Step {
    let ok_type = |o: &[u8]| -> Option<bool> {
        let level = i32::from_ne_bytes(o.get(8..12)?.try_into().ok()?);
        let ty = i32::from_ne_bytes(o.get(12..16)?.try_into().ok()?);
        Some(level == IPPROTO_IPV6 && (ty == IPV6_HOPOPTS || ty == IPV6_DSTOPTS))
    };
    let Some(true) = ok_type(obj) else { return Step::Error };
    let Some(len) = cmsg_len(obj).filter(|&l| l <= obj.len() && l >= CMSG_HDR) else { return Step::Error };
    let first = CMSG_HDR.saturating_add(2);
    let at = match prev {
        None => first,
        // Before the first option: not an option of this object.
        Some(p) if p < first => return Step::Error,
        Some(p) => match (obj.get(p), obj.get(p.saturating_add(1))) {
            (Some(0), _) => p.saturating_add(1),
            (Some(_), Some(&l)) => p.saturating_add(2).saturating_add(usize::from(l)),
            _ => return Step::Error,
        },
    };
    if at >= len {
        return Step::End;
    }
    let size = match (obj.get(at), obj.get(at.saturating_add(1))) {
        (Some(0), _) => 1,
        (Some(_), Some(&l)) => usize::from(l).saturating_add(2),
        _ => return Step::Error,
    };
    if at.saturating_add(size) > len { Step::Error } else { Step::At(at) }
}

/// `inet6_option_find`: the next option of type `ty` after `prev`.
pub fn option_find(obj: &[u8], prev: Option<usize>, ty: u8) -> Step {
    let mut prev = prev;
    loop {
        match option_next(obj, prev) {
            Step::At(at) if obj.get(at) == Some(&ty) => return Step::At(at),
            Step::At(at) => prev = Some(at),
            other => return other,
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn lenient_ipv4() {
        for (t, v) in [("1.2.3.4", 0x01020304), ("10.1", 0x0a000001), ("10.1.2", 0x0a010002), ("0x7f.1", 0x7f000001), ("0177.0.0.1", 0x7f000001), ("4294967295", 0xffffffff), ("1.2.3.4 x", 0x01020304), ("1.2 3", 0x01000002), ("0", 0)] {
            assert_eq!(aton(t.as_bytes()), Some(v), "{t}");
        }
        for t in ["4294967296", "1.2.3.4.5", "1.2.3.256", "1.16777216", "08.1.1.1", "0x.1.1.1", " 1.2.3.4", "", "1.2.3.", ".1.2.3", "1.2.3.4x", "0x", "-1"] {
            assert_eq!(aton(t.as_bytes()), None, "{t}");
        }
    }

    #[test]
    fn network_numbers() {
        for (t, v) in [("1.2.3.4", Some(0x01020304)), ("10.1", Some(0x0a01)), ("0x7f.1", Some(0x7f01)), ("0x7f000001", None), ("1.2.3.4 ", Some(0x01020304)), ("1.2.3.4 x", None), ("4294967296", None), ("0377", Some(0xff)), ("0400", None)] {
            assert_eq!(network(t.as_bytes()), v, "{t}");
        }
    }

    #[test]
    fn classful() {
        assert_eq!((lnaof(0x0a010203), netof(0x0a010203)), (0x010203, 0x0a));
        assert_eq!((lnaof(0x80010203), netof(0x80010203)), (0x0203, 0x8001));
        assert_eq!((lnaof(0xe0000001), netof(0xe0000001)), (1, 0xe00000));
        assert_eq!(makeaddr(0x0a, 0xffffffff), 0x0affffff);
        assert_eq!(makeaddr(0x80, 0x0102), 0x00800102);
        assert_eq!(makeaddr(0xc0a800, 1), 0xc0a80001);
        assert_eq!(makeaddr(0xffffff01, 2), 0xffffff03);
    }

    #[test]
    fn cidr() {
        let mut b = [0xee; 4];
        assert_eq!(net_pton(b"10/8", &mut b), Ok(8));
        assert_eq!(b, [10, 0xee, 0xee, 0xee]);
        let mut b = [0xee; 4];
        assert_eq!(net_pton(b"10.1.2.3", &mut b), Ok(32));
        let mut b = [0xee; 4];
        assert_eq!(net_pton(b"255", &mut b), Ok(32));
        assert_eq!(b, [255, 0, 0, 0]);
        let mut b = [0xee; 4];
        assert_eq!(net_pton(b"0x123/12", &mut b), Ok(12));
        assert_eq!(b[..2], [0x12, 0x30]);
        assert_eq!(net_pton(b"10/33", &mut [0; 4]), Err(NetError::Size));
        assert_eq!(net_pton(b"default", &mut [0; 4]), Err(NetError::Syntax));
        assert_eq!(net_pton(b"1.2.3.4.5/32", &mut [0; 4]), Err(NetError::Size));
        assert_eq!(net_ntop(&[10, 1, 0, 0], 6).unwrap().as_bytes(), b"8/6");
        assert_eq!(net_ntop(&[10, 1, 0, 0], 9).unwrap().as_bytes(), b"10.0/9");
        assert_eq!(net_ntop(&[10, 1, 0, 0], 0).unwrap().as_bytes(), b"0/0");
        assert_eq!(net_ntop(&[0; 4], 33), None);
        assert_eq!(neta(0x0a1e1e).as_bytes(), b"10.30.30");
        assert_eq!(neta(0).as_bytes(), b"0.0.0.0");
        assert_eq!(neta(0x0a000100).as_bytes(), b"10.0.1.0");
    }

    #[test]
    fn nsap() {
        let mut b = [0u8; 32];
        assert_eq!(nsap_addr(b"47000580ffef00", &mut b), 7);
        assert_eq!(nsap_addr(b"47+00/05", &mut b), 3);
        assert_eq!(nsap_addr(b"0x47.0005.80ffef00", &mut b), 0);
        assert_eq!(nsap_addr(b"470", &mut b), 0);
        assert_eq!(nsap_addr(b"4.7", &mut b), 0);
        assert_eq!(nsap_addr(b"47000580", &mut b[..2]), 2);
        let mut t = [0u8; 32];
        let n = nsap_ntoa(&[0x47, 0, 5, 0x80, 0xff, 0xef, 0], &mut t);
        assert_eq!(&t[..n], b"47.0005.80FF.EF00\0");
        assert_eq!(nsap_text_len(7), 18);
    }

    #[test]
    fn rfc2292() {
        assert_eq!((option_space(6), option_space(7)), (24, 32));
        let mut obj = [0xeeu8; 80];
        option_init(&mut obj, IPV6_HOPOPTS).unwrap();
        let (at, len) = option_place(16, 6, 4, 2).unwrap();
        assert_eq!((at, len), (22, 32));
        option_layout(&mut obj, 16, at, 6, len).unwrap();
        obj[at..at + 6].copy_from_slice(&[0xc2, 4, 1, 2, 3, 4]);
        assert_eq!(obj[17], 1);
        assert_eq!(option_next(&obj, None), Step::At(18));
        assert_eq!(option_find(&obj, None, 0xc2), Step::At(22));
        assert_eq!(option_find(&obj, Some(22), 0xc2), Step::End);
        assert_eq!(option_place(32, 3, 3, 0), None);
    }
}
