//! Proleptic Gregorian calendar arithmetic (spec §23.5; D-23.4): days and seconds since
//! 1970-01-01 00:00:00 and the civil fields, for any year an `i64` count of seconds can reach.
//!
//! The day count is computed in 400-year eras (146,097 days each, the Gregorian cycle), with
//! the year starting in March so that February's variable length comes last.
#![allow(clippy::arithmetic_side_effects)] // i128 arithmetic on values derived from i64 inputs: every intermediate stays below 2^80; the i64 paths take day counts below 2^48 and years below 2^40, whose intermediates stay below 2^52

use crate::error::{Errno, Result};

/// Seconds in a day.
pub const DAY: i64 = 86_400;

/// Days in a 400-year Gregorian cycle.
const ERA_DAYS: i128 = 146_097;
/// Days from 0000-03-01 to 1970-01-01.
const EPOCH_SHIFT: i128 = 719_468;
/// Years and day counts below this take the 64-bit paths (native division; the 128-bit
/// arithmetic is for normalising arbitrary fields).
pub(crate) const FAST: u64 = 1 << 40;

/// A calendar instant (D-23.4): the full year, month 1–12, day 1–31, hour, minute, second
/// (0–60: 60 only for an inserted leap second), weekday (0 = Sunday) and day of the year
/// (0-based).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct CivilTime {
    /// The year (not offset by 1900).
    pub year: i64,
    /// 1–12.
    pub month: u8,
    /// 1–31.
    pub day: u8,
    /// 0–23.
    pub hour: u8,
    /// 0–59.
    pub minute: u8,
    /// 0–60.
    pub second: u8,
    /// 0–6, Sunday first.
    pub weekday: u8,
    /// 0–365.
    pub yearday: u16,
}

/// Broken-down fields as a caller gives them (`struct tm`'s, widened): any values, normalised
/// by [`Fields::seconds`]. `month` is 1-based (January = 1), `year` the full year.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Fields {
    /// The full year.
    pub year: i64,
    /// The month, 1-based; values outside 1–12 carry into the year.
    pub month: i64,
    /// The day of the month; values outside the month carry.
    pub day: i64,
    /// Hours.
    pub hour: i64,
    /// Minutes.
    pub minute: i64,
    /// Seconds.
    pub second: i64,
}

impl Fields {
    /// The fields as seconds since 1970-01-01 00:00:00 of the same clock, every field
    /// normalised (day 32 is the next month's first, minute -1 the previous hour's last, ...).
    pub fn seconds(&self) -> i128 {
        // 64-bit division where the month fits (every month but `i64::MIN`; MKT-1: the 128-bit
        // one was a library call on every `mktime`).
        let (carry, m0) = match self.month.checked_sub(1) {
            Some(m0) => (i128::from(m0.div_euclid(12)), i128::from(m0.rem_euclid(12))),
            None => {
                let m0 = i128::from(self.month) - 1;
                (m0.div_euclid(12), m0.rem_euclid(12))
            }
        };
        let year = i128::from(self.year) + carry;
        let month = m0 + 1;
        let days = days_from_civil(year, month, 1) + i128::from(self.day) - 1;
        days * i128::from(DAY) + i128::from(self.hour) * 3600 + i128::from(self.minute) * 60 + i128::from(self.second)
    }

    /// [`Fields::seconds`] without the seconds field, in 64-bit arithmetic; `None` unless every
    /// field is below 2^31 in magnitude (then no intermediate comes near 2^57; TZ-FRESH3: the
    /// 128-bit conversion was a third of `mktime`).
    pub(crate) fn seconds64(&self) -> Option<i64> {
        const B: i64 = 1 << 31;
        let small = |v: i64| v > -B && v < B;
        if !(small(self.year) && small(self.month) && small(self.day) && small(self.hour) && small(self.minute) && small(self.second)) {
            return None;
        }
        let m0 = self.month - 1;
        let days = days_from_civil64(self.year + m0.div_euclid(12), m0.rem_euclid(12) + 1, 1) + self.day - 1;
        Some(days * DAY + self.hour * 3600 + self.minute * 60)
    }
}

/// Whether `year` has a February 29.
pub fn is_leap(year: i64) -> bool {
    year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
}

/// 365 or 366.
pub fn year_days(year: i64) -> i64 {
    if is_leap(year) { 366 } else { 365 }
}

/// Days in `month` (1–12) of `year`.
pub fn month_days(year: i64, month: u8) -> i64 {
    match month {
        2 if is_leap(year) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// Days from 1970-01-01 to the first day of `month` (1–12) of `year`, plus `day - 1`.
pub fn days_from_civil(year: i128, month: i128, day: i128) -> i128 {
    if let (Ok(y), Ok(m), Ok(d)) = (i64::try_from(year), i64::try_from(month), i64::try_from(day)) {
        if y.unsigned_abs() < FAST && (1..=12).contains(&m) && d.unsigned_abs() < FAST {
            return i128::from(days_from_civil64(y, m, d));
        }
    }
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * ERA_DAYS + doe - EPOCH_SHIFT
}

/// [`days_from_civil`] in 64-bit arithmetic: `year` and `day` below 2^40, `month` 1–12.
pub(crate) fn days_from_civil64(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The era shift of [`civil_days64`]: 400-year eras added so that every day count above -2^48
/// counts from a non-negative origin.
const NS_ERAS: i64 = 2_000_000_000;
/// Days from that origin (0000-03-01, [`NS_ERAS`] eras earlier) to 1970-01-01.
const NS_SHIFT: i64 = 719_468 + 146_097 * NS_ERAS;

/// The year, month (1–12), day (1–31) and day of the year (0–365) of day `days` since
/// 1970-01-01, for any `days` below 2^48 in magnitude (every intermediate stays below 2^52).
/// Neri and Schneider's computation (Euclidean affine functions and their application to
/// calendar algorithms, Software: Practice and Experience, 2023), unsigned, with the year
/// starting in March: the centuries by exact division, the year of the century and the day of
/// the year by one multiply-shift (exact for the day of a century, 0–36,524), the month and day
/// by another (exact for the day of a year, 0–365). MKT-2: a shorter chain of dependent
/// multiplications than the era / year-of-era divisions it replaces (gmtime's latency).
pub(crate) fn civil_days64(days: i64) -> (i64, u8, u8, u16) {
    let n = u64::from_ne_bytes((days + NS_SHIFT).to_ne_bytes());
    let n1 = 4 * n + 3;
    let c = n1 / 146_097;
    let nc = n1 % 146_097 / 4;
    let p2 = 2_939_745 * (4 * nc + 3);
    let z = p2 >> 32;
    let ny = (p2 & 0xffff_ffff) / 2_939_745 / 4;
    let n3 = 2_141 * ny + 197_913;
    let (m, d) = (n3 >> 16, (n3 & 0xffff) / 2_141);
    let jan_feb = ny >= 306;
    // The March-based year is 100 c + z: z is its last two digits, so it is a leap year (its
    // February 29 before March) when z is a multiple of 4 and not 0, or 0 with c a multiple of 4.
    let leap = z % 4 == 0 && (z != 0 || c % 4 == 0);
    let yday = if jan_feb { ny - 306 } else { ny + 59 + u64::from(leap) };
    let year = i64::try_from(100 * c + z).unwrap_or(0) - 400 * NS_ERAS + i64::from(jan_feb);
    let month = if jan_feb { m - 12 } else { m };
    (year, u8::try_from(month).unwrap_or(1), u8::try_from(d + 1).unwrap_or(1), u16::try_from(yday).unwrap_or(0))
}

/// [`civil_from_days`] in 64-bit arithmetic: any `days` below 2^48 ([`civil_days64`]);
/// [`civil_from_days`] uses it below 2^40, [`civil_from_seconds64`] for any `i64` count.
pub(crate) fn civil_from_days64(days: i64) -> (i64, u8, u8) {
    let (y, m, d, _) = civil_days64(days);
    (y, m, d)
}

/// The year, month (1–12) and day (1–31) of day `days` since 1970-01-01.
pub fn civil_from_days(days: i128) -> (i128, u8, u8) {
    if let Ok(d) = i64::try_from(days) {
        if d.unsigned_abs() < FAST {
            let (y, m, dd) = civil_from_days64(d);
            return (i128::from(y), m, dd);
        }
    }
    let z = days + EPOCH_SHIFT;
    let era = z.div_euclid(ERA_DAYS);
    let doe = z - era * ERA_DAYS;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i128::from(m <= 2);
    (y, u8::try_from(m).unwrap_or(1), u8::try_from(d).unwrap_or(1))
}

/// The civil time `secs` seconds after 1970-01-01 00:00:00 (any offset already applied).
/// `EOVERFLOW` if the year does not fit an `i64`.
pub fn civil_from_seconds(secs: i128) -> Result<CivilTime> {
    if let Ok(s) = i64::try_from(secs) {
        return Ok(civil_from_seconds64(s));
    }
    let day = i128::from(DAY);
    let days = secs.div_euclid(day);
    let rem = secs.rem_euclid(day);
    let (y, month, d) = civil_from_days(days);
    let year = i64::try_from(y).map_err(|_| Errno::EOVERFLOW)?;
    let yearday = days - days_from_civil(y, 1, 1);
    Ok(CivilTime {
        year,
        month,
        day: d,
        hour: u8::try_from(rem / 3600).unwrap_or(0),
        minute: u8::try_from(rem % 3600 / 60).unwrap_or(0),
        second: u8::try_from(rem % 60).unwrap_or(0),
        weekday: u8::try_from((days + 4).rem_euclid(7)).unwrap_or(0),
        yearday: u16::try_from(yearday).unwrap_or(0),
    })
}

/// [`civil_from_seconds`] for any `i64` count: the day count stays below 2^47, the year
/// below 2^39, so the 64-bit paths apply.
pub(crate) fn civil_from_seconds64(secs: i64) -> CivilTime {
    let days = secs.div_euclid(DAY);
    let rem = secs.rem_euclid(DAY);
    let (year, month, d, yearday) = civil_days64(days);
    CivilTime {
        year,
        month,
        day: d,
        hour: u8::try_from(rem / 3600).unwrap_or(0),
        minute: u8::try_from(rem % 3600 / 60).unwrap_or(0),
        second: u8::try_from(rem % 60).unwrap_or(0),
        weekday: u8::try_from((days + 4).rem_euclid(7)).unwrap_or(0),
        yearday,
    }
}

/// Whether `year` can be a `struct tm`'s (`tm_year`, the year − 1900, is an `int`).
pub fn fits_tm(year: i64) -> bool {
    year.checked_sub(1900).is_some_and(|y| i32::try_from(y).is_ok())
}

/// UTC (`gmtime`): `EOVERFLOW` when the year does not fit a `struct tm`.
pub fn utc(t: i64) -> Result<CivilTime> {
    let c = civil_from_seconds(i128::from(t))?;
    if fits_tm(c.year) { Ok(c) } else { Err(Errno::EOVERFLOW) }
}

/// `timegm`: the instant of `f` read as UTC, with its normalised fields. `EOVERFLOW` when the
/// instant does not fit a `time_t` or its year a `struct tm`.
pub fn timegm(f: &Fields) -> Result<(i64, CivilTime)> {
    let t = i64::try_from(f.seconds()).map_err(|_| Errno::EOVERFLOW)?;
    Ok((t, utc(t)?))
}

/// `difftime(t1, t0)`: the difference as a `double`, exact up to the double's precision (the
/// subtraction is done in 128 bits, so it never overflows).
#[allow(clippy::as_conversions)] // i128 to f64 is the rounding conversion the contract asks for
pub fn difftime(t1: i64, t0: i64) -> f64 {
    (i128::from(t1) - i128::from(t0)) as f64
}

/// `dysize(y)`: the days in year `y` (365 or 366).
pub fn dysize(year: i32) -> i32 {
    if is_leap(i64::from(year)) { 366 } else { 365 }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions)]
mod tests {
    use super::*;

    #[test]
    fn days_round_trip() {
        for days in (-1_000_000i128..1_000_000).step_by(997) {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, i128::from(m), i128::from(d)), days);
        }
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
    }

    /// The 64-bit paths agree with the 128-bit arithmetic wherever they apply.
    #[test]
    fn fast_paths_agree() {
        let slow_days = |days: i128| {
            let z = days + EPOCH_SHIFT;
            let era = z.div_euclid(ERA_DAYS);
            let doe = z - era * ERA_DAYS;
            let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let d = doy - (153 * mp + 2) / 5 + 1;
            let m = if mp < 10 { mp + 3 } else { mp - 9 };
            (yoe + era * 400 + i128::from(m <= 2), m, d)
        };
        let mut days: i64 = -(1 << 39);
        while days < 1 << 39 {
            let (y, m, d) = civil_from_days64(days);
            let (sy, sm, sd) = slow_days(i128::from(days));
            assert_eq!((i128::from(y), i128::from(m), i128::from(d)), (sy, sm, sd), "day {days}");
            assert_eq!(days_from_civil64(y, i64::from(m), i64::from(d)), days);
            days += 1_234_567_891;
        }
        for days in -800_000i64..800_000 {
            let (y, m, d) = civil_from_days64(days);
            assert_eq!((i128::from(y), i128::from(m), i128::from(d)), slow_days(i128::from(days)));
        }
        for secs in [i64::MIN, i64::MIN + 1, -1, 0, 1, i64::MAX - 1, i64::MAX] {
            let fast = civil_from_seconds64(secs);
            let (y, m, d) = slow_days(i128::from(secs).div_euclid(86_400));
            assert_eq!((i128::from(fast.year), i128::from(fast.month), i128::from(fast.day)), (y, m, d), "second {secs}");
        }
        // MKT-2: the Neri-Schneider path against the era arithmetic it replaced (`slow_days`),
        // the day of the year against the day count of January 1 and the weekday: every day of
        // +/-6,000 years (every leap rule, century and era edge), a stride over the whole
        // domain (+/-2^48), and its ends.
        let check = |days: i64| {
            let (y, m, d, yd) = civil_days64(days);
            assert_eq!((i128::from(y), i128::from(m), i128::from(d)), slow_days(i128::from(days)), "day {days}");
            assert_eq!(i64::from(yd), days - days_from_civil64(y, 1, 1), "yearday of day {days}");
        };
        for days in -2_200_000i64..2_200_000 {
            check(days);
        }
        let mut days: i64 = -(1 << 48) + 1;
        while days < 1 << 48 {
            check(days);
            days += 281_474_977;
        }
        for days in [-(1i64 << 48) + 1, -(1 << 47), (1 << 47) - 1, (1 << 48) - 1, i64::MIN / 86_400 - 1, i64::MAX / 86_400] {
            check(days);
        }
        for secs in [i64::MIN, i64::MIN + 1, -86_401, -86_400, -1, 0, 951_782_400, 4_107_542_400, i64::MAX] {
            let c = civil_from_seconds64(secs);
            let days = secs.div_euclid(86_400);
            assert_eq!(i64::from(c.yearday), days - days_from_civil64(c.year, 1, 1), "second {secs}");
            assert_eq!(i64::from(c.weekday), (days + 4).rem_euclid(7), "second {secs}");
        }
    }

    #[test]
    fn seconds_to_civil() {
        let c = civil_from_seconds(951_782_400).unwrap();
        assert_eq!((c.year, c.month, c.day, c.weekday, c.yearday), (2000, 2, 29, 2, 59));
        let c = civil_from_seconds(-62_135_596_801).unwrap();
        assert_eq!((c.year, c.month, c.day, c.hour, c.weekday, c.yearday), (0, 12, 31, 23, 0, 365));
        assert!(utc(67_767_976_233_532_800).is_ok());
        assert_eq!(utc(-67_768_040_609_740_801), Err(Errno::EOVERFLOW));
        assert_eq!(utc(i64::MAX), Err(Errno::EOVERFLOW));
    }

    #[test]
    fn normalisation() {
        let f = Fields { year: 2024, month: 1, day: 32, hour: 25, minute: 61, second: 61 };
        let (t, c) = timegm(&f).unwrap();
        assert_eq!(t, 1_706_839_321);
        assert_eq!((c.month, c.day, c.hour, c.minute, c.second), (2, 2, 2, 2, 1));
        let f = Fields { year: 2023, month: 0, day: 1, ..Fields::default() };
        assert_eq!(timegm(&f).unwrap().1.month, 12);
        assert_eq!(dysize(2100), 365);
        assert_eq!(dysize(2000), 366);
        assert_eq!(difftime(0, i64::MAX), -9.223_372_036_854_776e18);
    }
}
