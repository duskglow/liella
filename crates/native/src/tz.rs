//! The current time zone (`liella_tz_*`): the one `TZ` names, shared with the C code's
//! `localtime` and `mktime`. Conversions between instants (seconds since the epoch) and local
//! times as [`Tm`].

#![forbid(unsafe_code)]

use core::mem::{align_of, offset_of, size_of};

use crate::buf;
use crate::ffi;
use crate::Errno;

/// The capacity of [`Tm::abbr`] (`LIELLA_TZ_ABBR_CAP`).
pub const ABBR_CAP: usize = 16;

/// A local time in the current zone: `struct liella_tm` (64 bytes, alignment 8), field for
/// field.
///
/// As a result every field is set. As the input of [`from_local`], `year` to `second` and
/// `isdst` are read (values outside their ranges carry, as `mktime`'s do) and the rest is
/// ignored.
#[repr(C)]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Tm {
    /// The full year (2026 is 2026; may be zero or negative).
    pub year: i64,
    /// 1 to 12.
    pub month: i32,
    /// 1 to 31.
    pub day: i32,
    /// 0 to 23.
    pub hour: i32,
    /// 0 to 59.
    pub minute: i32,
    /// 0 to 60 (60 only for an inserted leap second).
    pub second: i32,
    /// 0 to 6, Sunday = 0.
    pub weekday: i32,
    /// 0 to 365: days since 1 January.
    pub yearday: i32,
    /// Daylight time: 1 in effect, 0 not; as input, negative lets the zone decide.
    pub isdst: i32,
    /// Seconds east of UTC.
    pub utc_offset: i32,
    /// The abbreviation's full length; [`abbr`](Tm::abbr) holds it only if it is at most
    /// [`ABBR_CAP`].
    pub abbr_len: u32,
    /// The zone abbreviation (`"CEST"`), not NUL-terminated; see [`Tm::abbr`].
    pub abbr: [u8; ABBR_CAP],
}

// The layout of `struct liella_tm` (include/liella/native.h asserts the same).
const _: () = assert!(size_of::<Tm>() == 64 && align_of::<Tm>() == 8);
const _: () = assert!(offset_of!(Tm, year) == 0 && offset_of!(Tm, month) == 8 && offset_of!(Tm, day) == 12);
const _: () = assert!(offset_of!(Tm, hour) == 16 && offset_of!(Tm, minute) == 20 && offset_of!(Tm, second) == 24);
const _: () = assert!(offset_of!(Tm, weekday) == 28 && offset_of!(Tm, yearday) == 32 && offset_of!(Tm, isdst) == 36);
const _: () = assert!(offset_of!(Tm, utc_offset) == 40 && offset_of!(Tm, abbr_len) == 44 && offset_of!(Tm, abbr) == 48);

impl Default for Tm {
    /// 1970-01-01 00:00:00, with `isdst` negative (the zone decides): an input for
    /// [`from_local`] to fill in.
    fn default() -> Tm {
        Tm { year: 1970, month: 1, day: 1, hour: 0, minute: 0, second: 0, weekday: 0, yearday: 0, isdst: -1, utc_offset: 0, abbr_len: 0, abbr: [0; ABBR_CAP] }
    }
}

impl Tm {
    /// An input for [`from_local`]: the date and time given, the zone deciding daylight time.
    pub fn civil(year: i64, month: i32, day: i32, hour: i32, minute: i32, second: i32) -> Tm {
        Tm { year, month, day, hour, minute, second, ..Tm::default() }
    }

    /// The zone abbreviation, or `None` if it is longer than [`ABBR_CAP`] (only its length,
    /// [`abbr_len`](Tm::abbr_len), is known then).
    pub fn abbr(&self) -> Option<&[u8]> {
        usize::try_from(self.abbr_len).ok().and_then(|n| self.abbr.get(..n))
    }
}

/// The local time of the instant `time` (seconds since the epoch) in the current zone, with
/// its offset and abbreviation.
pub fn to_local(time: i64) -> Result<Tm, Errno> {
    let mut tm = Tm::default();
    buf::done(ffi::liella_tz_to_local(time, &mut tm)).map(|()| tm)
}

/// The instant of the local time `tm` in the current zone, and `tm` normalised (every field
/// set, out-of-range values carried, as by [`to_local`]). `isdst` is read as `mktime` reads
/// `tm_isdst`.
pub fn from_local(tm: &Tm) -> Result<(i64, Tm), Errno> {
    let mut out = *tm;
    let mut time = 0;
    buf::done(ffi::liella_tz_from_local(&mut out, &mut time)).map(|()| (time, out))
}

/// Reload the time-zone data (D-X.13): every zone the process keeps from a file is compared with
/// its file before its next use and read again if the file changed (a tzdata update); then the
/// zone `TZ` names is selected, as C's `tzset` does.
pub fn invalidate() {
    let _ = ffi::liella_tz_invalidate();
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects)]
mod tests {
    use super::*;

    #[test]
    fn abbr_is_known_only_when_it_fits() {
        let mut tm = Tm::default();
        tm.abbr[..4].copy_from_slice(b"CEST");
        tm.abbr_len = 4;
        assert_eq!(tm.abbr(), Some(&b"CEST"[..]));
        tm.abbr_len = 16;
        assert_eq!(tm.abbr().map(<[u8]>::len), Some(16));
        tm.abbr_len = 17;
        assert_eq!(tm.abbr(), None);
        tm.abbr_len = 0;
        assert_eq!(tm.abbr(), Some(&b""[..]));
    }

    #[test]
    fn civil_lets_the_zone_decide() {
        let tm = Tm::civil(2026, 7, 1, 14, 0, 0);
        assert_eq!((tm.year, tm.month, tm.day, tm.hour, tm.isdst), (2026, 7, 1, 14, -1));
    }
}
