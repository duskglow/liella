//! `liella_tz_*`: the current time zone, the one `TZ` names (read again whenever the
//! environment changed, as `localtime` and `mktime` do), shared with the C functions.

use super::abi;
use super::types::LiellaTm;
use crate::export;

export! { "LIELLA_NATIVE_1";
/// The local time of the instant `time` (seconds since the epoch) into `*out`, with the
/// zone's offset and abbreviation. A year outside the zone conversion's range (years that fit
/// a C `int`, as for `localtime`) is `-EOVERFLOW`.
pub extern "C" fn liella_tz_to_local(time: i64, out: Option<&mut LiellaTm>) -> i64 {
    abi::to_local(time, out)
}}

export! { "LIELLA_NATIVE_1";
/// The instant of the local time in `*tm` (`year` to `second`, out-of-range values carrying,
/// and `isdst`: negative lets the zone decide) into `*time`; `*tm` is then normalised, every
/// field set. On failure both are left alone.
pub extern "C" fn liella_tz_from_local(tm: Option<&mut LiellaTm>, time: Option<&mut i64>) -> i64 {
    abi::from_local(tm, time)
}}

export! { "LIELLA_NATIVE_1";
/// Reload the time-zone data (D-X.13): every zone kept from a file is compared with its file
/// before its next use and read again if the file changed; then the zone `TZ` names is selected,
/// as `tzset` does. Zones are otherwise kept for the life of the process. 0.
pub extern "C" fn liella_tz_invalidate() -> i64 {
    abi::done(crate::zone::invalidate())
}}
