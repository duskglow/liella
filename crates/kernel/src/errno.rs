//! Kernel error numbers (spec C0.1: the numbering is kernel ABI, ⚑ KERN).
//!
//! The constants in `errno_list.rs` are generated from the Linux UAPI headers
//! `asm-generic/errno-base.h` and `asm-generic/errno.h`, a normative reference (OD-2).
//! They are not typed by hand.

use core::num::NonZeroU16;

#[path = "errno_list.rs"]
mod list;
pub use list::MAX_ERRNO;

/// A kernel error number, 1..=[`MAX_ERRNO`].
///
/// The single error value of the whole library (rule X1). It is never 0.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Errno(NonZeroU16);

#[allow(clippy::as_conversions, clippy::arithmetic_side_effects)] // const fns: range-checked casts
impl Errno {
    /// Build a constant (only used with literal kernel numbers).
    const fn lit(n: u16) -> Errno {
        match NonZeroU16::new(n) {
            Some(v) => Errno(v),
            None => Errno::lit(22),
        }
    }

    /// The error number `n`, if it is a kernel error number.
    pub const fn new(n: i32) -> Option<Errno> {
        if n >= 1 && n <= MAX_ERRNO as i32 {
            match NonZeroU16::new(n as u16) {
                Some(v) => Some(Errno(v)),
                None => None,
            }
        } else {
            None
        }
    }

    /// The numeric value, as C `errno` holds it.
    pub const fn get(self) -> i32 {
        self.0.get() as i32
    }

    /// The symbolic name (`"ENOENT"`), or `None` for a number the kernel does not define.
    pub fn name(self) -> Option<&'static str> {
        list::NAMES.iter().find(|(v, _)| *v == self.0.get()).map(|(_, n)| *n)
    }

    /// Decode a raw kernel system-call result (rule X3). Values in [-4095, -1] are `-errno`
    /// (⚑ KERN); everything else is a successful result, returned as the unsigned register
    /// value. An error number above `MAX_ERRNO` (a future kernel's) still decodes as an error,
    /// reported as `EINVAL`, never as success.
    pub const fn decode(raw: i64) -> Result<u64, Errno> {
        if raw < 0 && raw >= -4095 {
            match Errno::new((-raw) as i32) {
                Some(e) => Err(e),
                None => Err(Errno::EINVAL),
            }
        } else {
            Ok(raw as u64)
        }
    }
}

impl core::fmt::Debug for Errno {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self.name() {
            Some(n) => write!(f, "{n}({})", self.get()),
            None => write!(f, "Errno({})", self.get()),
        }
    }
}

impl core::fmt::Display for Errno {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(self, f)
    }
}

/// Result of a kernel operation.
pub type KResult<T> = Result<T, Errno>;
