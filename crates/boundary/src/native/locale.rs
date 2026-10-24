//! `liella_locale_*`: the calling thread's current locale (its `uselocale` choice, else the
//! global locale `setlocale` sets).

use super::abi;
use crate::conv;
use crate::export;
use crate::locale;

export! { "LIELLA_NATIVE_1";
/// The name of `category` (`LIELLA_LC_*`) into `buf` (copied only if it fits whole): its
/// length. `LIELLA_LC_ALL` gives the composite name (one name when all categories agree);
/// an unknown category is `-EINVAL`.
pub extern "C" fn liella_locale_name(category: i32, buf: *mut u8, buf_len: usize) -> i64 {
    // UNSAFE(slice) SAFETY: buf holds buf_len writable bytes (D-X.10).
    abi::value(unsafe { conv::buf_mut(buf.cast(), buf_len) }.and_then(|b| locale::current_name(category, b)))
}}

export! { "LIELLA_NATIVE_1";
/// The decimal point of `LC_NUMERIC` (its bytes, possibly several) into `buf` (copied only if
/// it fits whole): its length.
pub extern "C" fn liella_locale_decimal_point(buf: *mut u8, buf_len: usize) -> i64 {
    // UNSAFE(slice) SAFETY: buf holds buf_len writable bytes (D-X.10).
    abi::value(unsafe { conv::buf_mut(buf.cast(), buf_len) }.map(|b| conv::copy_out(locale::radix(locale::current()), b)))
}}

export! { "LIELLA_NATIVE_1";
/// Reload the locale data (D-X.13): every mapped locale file and parsed charmap is compared
/// with its file before its next use and read again if the file changed; then the global locale
/// is loaded again by its names. Locale objects (`newlocale`, `duplocale`) keep their data.
/// 0, or `-ENOENT` when a name of the global locale no longer resolves (it is then unchanged).
pub extern "C" fn liella_locale_invalidate() -> i64 {
    abi::done(locale::invalidate())
}}
