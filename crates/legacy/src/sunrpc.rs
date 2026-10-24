//! ONC RPC and XDR (spec §29.1): the rules of the XDR primitives (RFC 4506) and of the memory
//! stream, as plain arithmetic the C layer (`liella_boundary::posix::s29_legacy`) applies to the
//! caller's `XDR`. Every rule here was settled black-box against the reference library
//! (`spec/behaviour/xdr.md`, `xdr.glibc.txt`); none depends on pointers, so all of it is tested
//! in-process (R15-2).

/// `XDR_ENCODE`: the stream writes.
pub const XDR_ENCODE: i32 = 0;
/// `XDR_DECODE`: the stream reads.
pub const XDR_DECODE: i32 = 1;
/// `XDR_FREE`: the filter releases what a decode allocated.
pub const XDR_FREE: i32 = 2;
/// The size of one XDR unit (RFC 4506 §3): every item fills a multiple of four bytes.
pub const UNIT: u32 = 4;

/// The zero bytes that pad an opaque item to a whole unit (RFC 4506 §4.9) count `pad(len)`:
/// `(4 - len % 4) % 4`, which is the low two bits of `-len` (modular negation cannot overflow).
#[must_use]
pub const fn pad(len: u32) -> u32 {
    len.wrapping_neg() & 3
}

/// The memory stream's room check: `n` bytes fit when `n <= handy` (the bytes left), and the
/// answer is the room left after them. A miss changes nothing (the probe: a failed put or get
/// leaves the position and `x_handy` as they were).
#[must_use]
pub const fn take(handy: u32, n: u32) -> Option<u32> {
    handy.checked_sub(n)
}

/// `x_setpostn` on a memory stream: the stream ends at `pos + handy`, so a position up to that
/// end is valid (the end itself included) and the answer is the new room. A position past the
/// end fails and changes nothing.
#[must_use]
pub fn seek(pos: u32, handy: u32, to: u32) -> Option<u32> {
    // Two u32 values summed in u64 cannot overflow: the wrapping form never wraps.
    let end = u64::from(pos).wrapping_add(u64::from(handy));
    let room = end.checked_sub(u64::from(to))?;
    // A memory stream's size is an `unsigned int`, so a real room always fits; a sum built from
    // impossible values fails rather than truncating.
    u32::try_from(room).ok()
}

/// `xdr_long` encode: the wire holds 32 bits, so a `long` outside the `int32` range fails
/// before anything is written.
#[must_use]
pub fn long_to_wire(v: i64) -> Option<u32> {
    match i32::try_from(v) {
        Ok(w) => Some(u32::from_ne_bytes(w.to_ne_bytes())),
        Err(_) => None,
    }
}

/// `xdr_u_long` encode: an `unsigned long` above `UINT32_MAX` fails before anything is written.
#[must_use]
pub fn ulong_to_wire(v: u64) -> Option<u32> {
    u32::try_from(v).ok()
}

/// A word read as a signed `long` (`x_getlong`, `xdr_long`): sign-extended.
#[must_use]
pub fn long_from_wire(w: u32) -> i64 {
    i64::from(i32::from_ne_bytes(w.to_ne_bytes()))
}

/// A word read as an `unsigned long` (`xdr_u_long`): zero-extended.
#[must_use]
pub fn ulong_from_wire(w: u32) -> u64 {
    u64::from(w)
}

/// `xdr_bool` both ways: any non-zero value is `TRUE` (1), on the wire and in memory.
#[must_use]
pub fn bool_norm(v: u32) -> i32 {
    i32::from(v != 0)
}

/// A hyper (RFC 4506 §4.5) as its two words, the most significant first.
#[must_use]
pub const fn hyper_words(v: u64) -> [u32; 2] {
    let [a, b, c, d, e, f, g, h] = v.to_be_bytes();
    [u32::from_be_bytes([a, b, c, d]), u32::from_be_bytes([e, f, g, h])]
}

/// Two words back into a hyper.
#[must_use]
pub const fn hyper_join(hi: u32, lo: u32) -> u64 {
    let [a, b, c, d] = hi.to_be_bytes();
    let [e, f, g, h] = lo.to_be_bytes();
    u64::from_be_bytes([a, b, c, d, e, f, g, h])
}

/// The bytes a decoded string needs (its length plus the terminating NUL), or `None` when the
/// length exceeds `max` or the count does not fit an `unsigned int` (a length of `UINT32_MAX`
/// fails before any allocation).
#[must_use]
pub const fn string_room(len: u32, max: u32) -> Option<u32> {
    if len > max { None } else { len.checked_add(1) }
}

/// `xdr_bytes`/`xdr_string` length check on encode and decode: a length above `max` fails
/// (after the length word went through the stream, as in the reference library).
#[must_use]
pub const fn len_ok(len: u32, max: u32) -> bool {
    len <= max
}

/// A byte count as a `usize`. Exact on every 32- and 64-bit target; a target narrower than
/// 32 bits gets `usize::MAX`, which any allocation or room check then refuses.
#[must_use]
pub fn count(n: u32) -> usize {
    usize::try_from(n).unwrap_or(usize::MAX)
}

/// The low 32 bits of a `long` (`x_putlong` sends them unchecked; the filters range-check first).
#[must_use]
pub const fn low32(v: i64) -> u32 {
    let [a, b, c, d, ..] = v.to_le_bytes();
    u32::from_le_bytes([a, b, c, d])
}

/// A signed 32-bit value as its wire word (two's complement bits, RFC 4506 §4.1).
#[must_use]
pub const fn word_of_i32(v: i32) -> u32 {
    u32::from_ne_bytes(v.to_ne_bytes())
}

/// A wire word as a signed 32-bit value.
#[must_use]
pub const fn i32_of_word(w: u32) -> i32 {
    i32::from_ne_bytes(w.to_ne_bytes())
}

/// The low 16 bits of a word as a signed `short` (the probe: decode truncates).
#[must_use]
pub const fn i16_of_word(w: u32) -> i16 {
    let [a, b, ..] = w.to_le_bytes();
    i16::from_le_bytes([a, b])
}

/// The low 16 bits of a word as an `unsigned short`.
#[must_use]
pub const fn u16_of_word(w: u32) -> u16 {
    let [a, b, ..] = w.to_le_bytes();
    u16::from_le_bytes([a, b])
}

/// The low 8 bits of a word as a signed `char`.
#[must_use]
pub const fn i8_of_word(w: u32) -> i8 {
    let [a, ..] = w.to_le_bytes();
    i8::from_le_bytes([a])
}

/// The low 8 bits of a word as an `unsigned char`.
#[must_use]
pub const fn u8_of_word(w: u32) -> u8 {
    let [a, ..] = w.to_le_bytes();
    a
}

/// A signed hyper as its 64 bits.
#[must_use]
pub const fn i64_bits(v: i64) -> u64 {
    u64::from_ne_bytes(v.to_ne_bytes())
}

/// 64 bits as a signed hyper.
#[must_use]
pub const fn i64_of_bits(v: u64) -> i64 {
    i64::from_ne_bytes(v.to_ne_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc4506_units_and_padding() {
        // RFC 4506 §4.9-4.11: opaque data and strings are padded to a multiple of four bytes.
        assert_eq!([pad(0), pad(1), pad(2), pad(3), pad(4), pad(5), pad(u32::MAX)], [0, 3, 2, 1, 0, 3, 1]);
    }

    #[test]
    fn room_and_seek() {
        assert_eq!((take(4, 4), take(3, 4), take(0, 0), take(u32::MAX, u32::MAX)), (Some(0), None, Some(0), Some(0)));
        // xdr.glibc.txt "setpostn": a 16-byte stream accepts 8 and 16, refuses 17.
        assert_eq!((seek(0, 16, 8), seek(8, 8, 16), seek(16, 0, 17), seek(16, 0, 0)), (Some(8), Some(0), None, Some(16)));
        assert_eq!(seek(u32::MAX, u32::MAX, u32::MAX), Some(u32::MAX));
    }

    #[test]
    fn integer_rules() {
        assert_eq!((long_to_wire(-1), long_to_wire(0x7fff_ffff), long_to_wire(0x8000_0000)), (Some(u32::MAX), Some(0x7fff_ffff), None));
        assert_eq!((long_to_wire(i64::from(i32::MIN)), long_to_wire(-0x8000_0001), long_to_wire(0x1_0000_0001)), (Some(0x8000_0000), None, None));
        assert_eq!((ulong_to_wire(0xffff_ffff), ulong_to_wire(0x1_0000_0000), ulong_to_wire(u64::MAX)), (Some(u32::MAX), None, None));
        assert_eq!((long_from_wire(0xffff_ffff), long_from_wire(0x8000_0000), ulong_from_wire(0xffff_ffff)), (-1, -0x8000_0000, 0xffff_ffff));
        assert_eq!([bool_norm(0), bool_norm(1), bool_norm(2), bool_norm(u32::MAX)], [0, 1, 1, 1]);
        assert_eq!(hyper_words(0x0102_0304_0506_0708), [0x0102_0304, 0x0506_0708]);
        assert_eq!(hyper_join(0xfedc_ba98, 0x7654_3210), 0xfedc_ba98_7654_3210);
    }

    #[test]
    fn length_rules() {
        assert_eq!((string_room(5, 10), string_room(5, 5), string_room(5, 4), string_room(0, 0)), (Some(6), Some(6), None, Some(1)));
        assert_eq!(string_room(u32::MAX, u32::MAX), None, "the count of a maximal length does not fit");
        assert!(len_ok(3, 10) && len_ok(10, 10) && !len_ok(11, 10) && !len_ok(3, 2));
    }

    #[test]
    fn conversions() {
        assert_eq!((count(0), count(u32::MAX)), (0, 0xffff_ffff));
        assert_eq!((low32(0x1_ffff_ffff), low32(-1), low32(i64::MIN)), (u32::MAX, u32::MAX, 0));
        assert_eq!((word_of_i32(-1), word_of_i32(i32::MIN), i32_of_word(0x8000_0000), i32_of_word(7)), (u32::MAX, 0x8000_0000, i32::MIN, 7));
        assert_eq!((i16_of_word(0x1_8000), u16_of_word(0x1_ffff), i8_of_word(0x1ff), u8_of_word(0x1ff)), (i16::MIN, u16::MAX, -1, 0xff));
        // MUTATION-CAMPAIGN-1: a positive, a zero and the most negative low byte (a constant -1 survived 0x1ff alone).
        assert_eq!((i8_of_word(0x17f), i8_of_word(0x100), i8_of_word(0xffff_ff80)), (127, 0, i8::MIN));
        assert_eq!((i64_bits(-1), i64_of_bits(0x8000_0000_0000_0000), i64_of_bits(i64_bits(-5))), (u64::MAX, i64::MIN, -5));
    }
}
