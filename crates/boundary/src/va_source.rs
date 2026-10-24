//! `VaSource` over the C layer's `va_arg` accessors (class *adapter*, IP-14). Safe code: the
//! accessors' obligation (read no more arguments, and of no other types, than the C caller
//! passed) is the format's, which the formatted-I/O engines read the arguments by.
//!
//! Every value crosses as its bits, never converted: an integer argument as the register's 64
//! bits (the engine narrows it by the conversion's length), a `double` as its IEEE bits, a
//! `long double` as the 16 bytes the x87 store wrote (80 significant bits, low word first).
//! (D-12.25, V1.)

use liella_abi::va::VaSource;

use crate::ffi;

/// A `long double` slot for the accessor (16 bytes, 16-aligned, as the x87 store needs).
#[repr(C, align(16))]
#[derive(Default)]
pub struct LongDouble(pub [u64; 2]);

/// The arguments of the `va_list` at `ap` (the address of the C caller's `va_list` object).
pub struct CVa(pub usize);

impl VaSource for CVa {
    fn int(&mut self) -> u64 {
        u64::from_ne_bytes(ffi::liella_va_long(self.0).to_ne_bytes())
    }

    fn double(&mut self) -> u64 {
        ffi::liella_va_double(self.0).to_bits()
    }

    fn long_double(&mut self) -> u128 {
        let mut slot = LongDouble::default();
        ffi::liella_va_ldouble(self.0, &mut slot);
        let [lo, hi] = slot.0;
        (u128::from(hi) << 64) | u128::from(lo)
    }
}
