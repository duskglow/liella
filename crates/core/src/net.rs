//! Addresses, interfaces and their text forms (spec §15.3–15.7).
//!
//! Written under the clean-room rules of implementation-plan.md §1.4 from the spec, the RFCs
//! (4291, 5952, 3542, 3549), the Linux man pages and `spec/behaviour/netaddr.md`.
//!
//! * [`addr`]: the strict text forms of IPv4 and IPv6 addresses (`inet_pton`, `inet_ntop`).
//!   The historical lenient forms (`inet_aton`, the classful helpers, CIDR and NSAP texts) are
//!   C contracts and live in `liella_legacy::inet_compat` (D-15.2).
//! * [`ether`]: 48-bit Ethernet addresses and the lines of the `ethers` file.
//! * [`ip6opt`]: IPv6 hop-by-hop and destination options and type-0 routing headers in caller
//!   buffers (RFC 3542).
//! * [`netlink`] and [`iface`]: interface enumeration over `NETLINK_ROUTE` only (D-15.3).

pub mod addr;
pub mod ether;
pub mod iface;
pub mod ip6opt;
pub mod netlink;

/// A short text built in place (addresses, interface names): at most `N` bytes, no
/// terminator. Pushing beyond `N` bytes is ignored and remembered ([`Text::overflowed`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Text<const N: usize> {
    buf: [u8; N],
    len: usize,
    over: bool,
}

impl<const N: usize> Default for Text<N> {
    fn default() -> Text<N> {
        Text::new()
    }
}

impl<const N: usize> Text<N> {
    /// An empty text.
    pub const fn new() -> Text<N> {
        Text { buf: [0; N], len: 0, over: false }
    }

    /// The bytes so far.
    pub fn as_bytes(&self) -> &[u8] {
        self.buf.get(..self.len).unwrap_or(&[])
    }

    /// Whether a push did not fit.
    pub fn overflowed(&self) -> bool {
        self.over
    }

    /// Append one byte.
    pub fn push(&mut self, b: u8) {
        match self.buf.get_mut(self.len) {
            Some(slot) => {
                *slot = b;
                self.len = self.len.saturating_add(1);
            }
            None => self.over = true,
        }
    }

    /// Append bytes.
    pub fn push_all(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.push(b);
        }
    }

    /// Append `v` in decimal.
    pub fn push_dec(&mut self, v: u64) {
        let mut digits = [0u8; 20];
        let mut i = digits.len();
        let mut v = v;
        loop {
            i = i.saturating_sub(1);
            if let Some(d) = digits.get_mut(i) {
                *d = b'0'.wrapping_add(u8::try_from(v % 10).unwrap_or(0));
            }
            v /= 10;
            if v == 0 || i == 0 {
                break;
            }
        }
        self.push_all(digits.get(i..).unwrap_or(&[]));
    }

    /// Append `v` in hexadecimal without leading zeros (`upper`: `A`–`F`).
    pub fn push_hex(&mut self, v: u32, upper: bool) {
        let mut started = false;
        for shift in [28u32, 24, 20, 16, 12, 8, 4, 0] {
            let nibble = v.wrapping_shr(shift) & 0xf;
            if nibble != 0 || started || shift == 0 {
                started = true;
                self.push(hex_digit(nibble, upper));
            }
        }
    }

}

/// The digit of a nibble.
pub fn hex_digit(n: u32, upper: bool) -> u8 {
    let n = u8::try_from(n & 0xf).unwrap_or(0);
    match (n, upper) {
        (0..=9, _) => b'0'.wrapping_add(n),
        (_, true) => b'A'.wrapping_add(n.wrapping_sub(10)),
        (_, false) => b'a'.wrapping_add(n.wrapping_sub(10)),
    }
}

/// The value of a hexadecimal digit.
pub fn hex_value(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c.wrapping_sub(b'0')),
        b'a'..=b'f' => Some(c.wrapping_sub(b'a').wrapping_add(10)),
        b'A'..=b'F' => Some(c.wrapping_sub(b'A').wrapping_add(10)),
        _ => None,
    }
}

/// C `isspace` in the "C" locale: space, `\t`, `\n`, `\v`, `\f`, `\r`.
pub fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// Copy `text` and a terminating NUL into `out` if both fit: the length copied (without the
/// NUL), or `None` (nothing written).
pub fn copy_z(text: &[u8], out: &mut [u8]) -> Option<usize> {
    let dst = out.get_mut(..=text.len())?;
    let (body, nul) = dst.split_at_mut(text.len());
    body.copy_from_slice(text);
    nul.fill(0);
    Some(text.len())
}
