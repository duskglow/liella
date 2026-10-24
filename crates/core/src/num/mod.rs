//! Text/number conversion, exact float formatting and parsing, integer and bit utilities
//! (spec §8; decisions D-8.1–D-8.9).

pub mod bigint;
pub mod float;
pub mod format;
pub mod parse;

#[cfg(test)]
mod glibc_diff;

pub use float::{Float, Format, Rounding, Value};

/// C23 `<stdbit.h>` over one unsigned width (D-8.7: one generic implementation, five widths).
pub trait StdBit: Copy + Eq {
    /// Width in bits.
    const WIDTH: u32;
    /// `stdc_leading_zeros`.
    fn leading_zeros_(self) -> u32;
    /// `stdc_leading_ones`.
    fn leading_ones_(self) -> u32;
    /// `stdc_trailing_zeros`.
    fn trailing_zeros_(self) -> u32;
    /// `stdc_trailing_ones`.
    fn trailing_ones_(self) -> u32;
    /// `stdc_count_ones`.
    fn count_ones_(self) -> u32;
    /// True if zero.
    fn is_zero_(self) -> bool;
    /// True if all ones.
    fn is_max_(self) -> bool;
    /// `stdc_bit_floor`.
    fn bit_floor_(self) -> Self;
    /// `stdc_bit_ceil` (0 if not representable, ⚑ ARB per C23).
    fn bit_ceil_(self) -> Self;
}

macro_rules! stdbit {
    ($($t:ty),*) => {$(
        impl StdBit for $t {
            const WIDTH: u32 = <$t>::BITS;
            fn leading_zeros_(self) -> u32 { self.leading_zeros() }
            fn leading_ones_(self) -> u32 { self.leading_ones() }
            fn trailing_zeros_(self) -> u32 { self.trailing_zeros() }
            fn trailing_ones_(self) -> u32 { self.trailing_ones() }
            fn count_ones_(self) -> u32 { self.count_ones() }
            fn is_zero_(self) -> bool { self == 0 }
            fn is_max_(self) -> bool { self == <$t>::MAX }
            fn bit_floor_(self) -> $t { if self == 0 { 0 } else { <$t>::MAX.checked_shr(self.leading_zeros()).unwrap_or(0) ^ <$t>::MAX.checked_shr(self.leading_zeros().saturating_add(1)).unwrap_or(0) } }
            fn bit_ceil_(self) -> $t { if self <= 1 { 1 } else { self.checked_next_power_of_two().unwrap_or(0) } }
        }
    )*};
}
stdbit!(u8, u16, u32, u64);

/// `stdc_first_leading_one`: 1-based index from the most significant end, 0 if none.
pub fn first_leading_one<T: StdBit>(x: T) -> u32 {
    if x.is_zero_() { 0 } else { x.leading_zeros_().saturating_add(1) }
}

/// `stdc_first_leading_zero`.
pub fn first_leading_zero<T: StdBit>(x: T) -> u32 {
    if x.is_max_() { 0 } else { x.leading_ones_().saturating_add(1) }
}

/// `stdc_first_trailing_one` (also `ffs` on the unsigned reinterpretation, D-8.7).
pub fn first_trailing_one<T: StdBit>(x: T) -> u32 {
    if x.is_zero_() { 0 } else { x.trailing_zeros_().saturating_add(1) }
}

/// `stdc_first_trailing_zero`.
pub fn first_trailing_zero<T: StdBit>(x: T) -> u32 {
    if x.is_max_() { 0 } else { x.trailing_ones_().saturating_add(1) }
}

/// `stdc_count_zeros`.
pub fn count_zeros<T: StdBit>(x: T) -> u32 {
    T::WIDTH.saturating_sub(x.count_ones_())
}

/// `stdc_has_single_bit`.
pub fn has_single_bit<T: StdBit>(x: T) -> bool {
    x.count_ones_() == 1
}

/// `stdc_bit_width`: 1 + floor(log2 x), 0 for 0.
pub fn bit_width<T: StdBit>(x: T) -> u32 {
    T::WIDTH.saturating_sub(x.leading_zeros_())
}

/// Quotient and remainder, truncated toward zero; `None` for division by zero or the one
/// overflowing case (`MIN / -1`), which the boundary turns into a diagnostic (D-8.6).
pub fn div_rem(n: i64, d: i64) -> Option<(i64, i64)> {
    Some((n.checked_div(d)?, n.checked_rem(d)?))
}

/// The radix-64 alphabet of `l64a`/`a64l` (spec §8.10).
const RADIX64: &[u8; 64] = b"./0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// `l64a`: up to six radix-64 digits of the low 32 bits of `n`, least significant first; the
/// empty text for 0. Returns the digit count written into `out`.
pub fn l64a(n: i64, out: &mut [u8; 6]) -> usize {
    let mut v = u64::from_le_bytes(n.to_le_bytes()) & 0xffff_ffff;
    let mut len = 0usize;
    while v != 0 {
        if let (Some(slot), Some(&c)) = (out.get_mut(len), usize::try_from(v & 63).ok().and_then(|d| RADIX64.get(d))) {
            *slot = c;
        }
        v >>= 6;
        len = len.saturating_add(1);
    }
    len
}

/// `a64l`: the value of up to six radix-64 digits (least significant first), stopping at the
/// first character outside the alphabet; the 32-bit result is sign-extended.
pub fn a64l(s: &[u8]) -> i64 {
    let mut v: u32 = 0;
    for (i, &c) in s.iter().take(6).enumerate() {
        let Some(d) = RADIX64.iter().position(|&x| x == c) else { break };
        let shift = u32::try_from(i.saturating_mul(6)).unwrap_or(0);
        v |= u32::try_from(d).unwrap_or(0).wrapping_shl(shift);
    }
    i64::from(i32::from_le_bytes(v.to_le_bytes()))
}
