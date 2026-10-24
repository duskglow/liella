//! The current locale (`liella_locale_*`): the calling thread's (its `uselocale` choice, else
//! the global locale `setlocale` sets), shared with the C code.

#![forbid(unsafe_code)]

use alloc::vec::Vec;

use crate::buf::{self, Fit};
use crate::ffi;
use crate::Errno;

/// A locale category (`LIELLA_LC_*`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    /// `LC_CTYPE`.
    Ctype,
    /// `LC_NUMERIC`.
    Numeric,
    /// `LC_TIME`.
    Time,
    /// `LC_COLLATE`.
    Collate,
    /// `LC_MONETARY`.
    Monetary,
    /// `LC_MESSAGES`.
    Messages,
    /// `LC_ALL`: for [`name`], the composite name of all categories.
    All,
    /// `LC_PAPER`.
    Paper,
    /// `LC_NAME`.
    Name,
    /// `LC_ADDRESS`.
    Address,
    /// `LC_TELEPHONE`.
    Telephone,
    /// `LC_MEASUREMENT`.
    Measurement,
    /// `LC_IDENTIFICATION`.
    Identification,
}

impl Category {
    /// The C ABI's value (`LIELLA_LC_*`).
    pub const fn raw(self) -> i32 {
        match self {
            Category::Ctype => 0,
            Category::Numeric => 1,
            Category::Time => 2,
            Category::Collate => 3,
            Category::Monetary => 4,
            Category::Messages => 5,
            Category::All => 6,
            Category::Paper => 7,
            Category::Name => 8,
            Category::Address => 9,
            Category::Telephone => 10,
            Category::Measurement => 11,
            Category::Identification => 12,
        }
    }
}

/// The current locale's name for `category` (`"C"`, `"de_DE.UTF-8"`; for [`Category::All`] the
/// composite name when the categories differ).
pub fn name(category: Category) -> Result<Vec<u8>, Errno> {
    buf::fetch(|b| ffi::locale_name(category.raw(), b))
}

/// The current locale's name for `category` into `buf` (whole or not at all).
pub fn name_into(category: Category, buf: &mut [u8]) -> Result<Fit, Errno> {
    let len = buf.len();
    buf::fit(ffi::locale_name(category.raw(), buf), len)
}

/// The current locale's decimal point (`"."`, `","`; a multibyte character in some locales).
pub fn decimal_point() -> Result<Vec<u8>, Errno> {
    buf::fetch(ffi::locale_decimal_point)
}

/// The current locale's decimal point into `buf` (whole or not at all).
pub fn decimal_point_into(buf: &mut [u8]) -> Result<Fit, Errno> {
    let len = buf.len();
    buf::fit(ffi::locale_decimal_point(buf), len)
}

/// Reload the locale data (D-X.13): every mapped locale file and charmap is compared with its
/// file before its next use and read again if the file changed; then the global locale is
/// loaded again by its names. `ENOENT` when one of them no longer resolves (the global locale is
/// then unchanged).
pub fn invalidate() -> Result<(), Errno> {
    buf::done(ffi::liella_locale_invalidate())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects)]
mod tests {
    use super::*;

    #[test]
    fn categories_have_the_c_values() {
        use Category::*;
        let all = [Ctype, Numeric, Time, Collate, Monetary, Messages, All, Paper, Name, Address, Telephone, Measurement, Identification];
        for (i, c) in all.iter().enumerate() {
            assert_eq!(usize::try_from(c.raw()).unwrap(), i, "{c:?}");
        }
    }
}
