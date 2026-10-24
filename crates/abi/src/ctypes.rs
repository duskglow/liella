//! C scalar types for x86_64 LP64 (System V ABI; spec Appendix C.1).

#![allow(non_camel_case_types)]

/// `char` (signed on x86_64).
pub type c_char = i8;
/// `signed char`.
pub type c_schar = i8;
/// `unsigned char`.
pub type c_uchar = u8;
/// `short`.
pub type c_short = i16;
/// `unsigned short`.
pub type c_ushort = u16;
/// `int`.
pub type c_int = i32;
/// `unsigned int`.
pub type c_uint = u32;
/// `long`.
pub type c_long = i64;
/// `unsigned long`.
pub type c_ulong = u64;
/// `long long`.
pub type c_longlong = i64;
/// `unsigned long long`.
pub type c_ulonglong = u64;
/// `size_t`.
pub type size_t = usize;
/// `ssize_t`.
pub type ssize_t = isize;
/// `ptrdiff_t`.
pub type ptrdiff_t = isize;
/// `off_t` (64-bit on x86_64, C0.4).
pub type off_t = i64;
/// `wchar_t` (UTF-32 code point, signed 32-bit).
pub type wchar_t = i32;
/// `wint_t`.
pub type wint_t = u32;
/// `pid_t`.
pub type pid_t = i32;
/// `uid_t`.
pub type uid_t = u32;
/// `gid_t`.
pub type gid_t = u32;
/// `mode_t`.
pub type mode_t = u32;
/// `time_t` (64-bit, C0.4).
pub type time_t = i64;
/// `socklen_t`.
pub type socklen_t = u32;
/// `void` as a pointee.
pub type c_void = core::ffi::c_void;
/// `char8_t` (C23).
pub type char8_t = u8;
/// `char16_t`.
pub type char16_t = u16;
/// `char32_t`.
pub type char32_t = u32;
/// `mbstate_t`: 8 opaque bytes, 4-aligned (spec §6.1, D-6.1).
#[repr(C, align(4))]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct mbstate_t {
    /// The state's ABI bytes.
    pub bytes: [u8; 8],
}
/// `wctype_t`: an opaque class descriptor.
pub type wctype_t = c_ulong;
/// `wctrans_t`: an opaque mapping descriptor (a pointer to a table).
pub type wctrans_t = *const i32;
/// `locale_t`: an opaque pointer (spec §7).
pub type locale_t = *mut c_void;

/// The bits of an x87 `long double` (`_Float64x`) as the Rust side sees them (OD-C4): the
/// 64-bit significand with its explicit leading bit, then sign and exponent. The C adapter
/// converts between this and `long double` in one statement.
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct LdBits {
    /// Significand.
    pub mant: u64,
    /// Sign (bit 15) and biased exponent.
    pub se: u16,
    /// Padding to the 16 bytes of `long double`.
    pub pad: [u16; 3],
}

impl LdBits {
    /// From the low 80 bits of `b` (the `Format::F80` packing).
    pub const fn from_bits(b: u128) -> LdBits {
        let le = b.to_le_bytes();
        LdBits { mant: u64::from_le_bytes([le[0], le[1], le[2], le[3], le[4], le[5], le[6], le[7]]), se: u16::from_le_bytes([le[8], le[9]]), pad: [0; 3] }
    }

    /// The 80 bits as an integer.
    pub const fn to_bits(self) -> u128 {
        let m = self.mant.to_le_bytes();
        let e = self.se.to_le_bytes();
        u128::from_le_bytes([m[0], m[1], m[2], m[3], m[4], m[5], m[6], m[7], e[0], e[1], 0, 0, 0, 0, 0, 0])
    }
}

/// The bits of a `_Float128`, low half first (OD-C4).
#[repr(C)]
#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct F128Bits {
    /// Low 64 bits.
    pub lo: u64,
    /// High 64 bits (sign, exponent, top of the fraction).
    pub hi: u64,
}

impl F128Bits {
    /// From the binary128 packing.
    pub const fn from_bits(b: u128) -> F128Bits {
        let le = b.to_le_bytes();
        F128Bits {
            lo: u64::from_le_bytes([le[0], le[1], le[2], le[3], le[4], le[5], le[6], le[7]]),
            hi: u64::from_le_bytes([le[8], le[9], le[10], le[11], le[12], le[13], le[14], le[15]]),
        }
    }

    /// The 128 bits as an integer.
    pub const fn to_bits(self) -> u128 {
        let l = self.lo.to_le_bytes();
        let h = self.hi.to_le_bytes();
        u128::from_le_bytes([l[0], l[1], l[2], l[3], l[4], l[5], l[6], l[7], h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]])
    }
}
