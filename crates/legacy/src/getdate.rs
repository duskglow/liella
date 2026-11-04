//! `getdate` and `getdate_r` (spec §23.7; D-23.7; POSIX `getdate`): a date from text, by the
//! first template of the file `DATEMSK` names that matches it. Legacy: configuration through an
//! environment-named file and (for `getdate`) a global error number (⚑ HIST).
//!
//! **Errors** (POSIX, `getdate_err`): 1 `DATEMSK` unset or empty; 2 the file cannot be opened;
//! 3 its status cannot be read; 4 it is not a regular file; 5 a read error; 6 no memory; 7 no
//! template matches; 8 the input is not a valid date (February 30) or not representable. A
//! missing file is 2 (POSIX: "cannot be opened for reading"); glibc reports 3.
//!
//! **Matching.** The input may have white space before and after it. Each line of the file is a
//! template (blank lines are skipped), parsed as `strptime` does (`liella::time::parse`) with
//! ordinary characters matched regardless of case; the first that consumes the whole input
//! decides, valid or not.
//!
//! **Completion** (POSIX). Missing parts come from the current local time:
//! * no time: the current hour, minute and second; a time given in part: the rest are 0;
//! * a month without a day: the first day of the month, and without a year the first such month
//!   from the current one on (next year if it has passed);
//! * a weekday without a day of the month: the first such day from the base day on (today, or
//!   the month's first day);
//! * a time without any date: tomorrow if its hour (0 when not given) is before the current hour,
//!   else today, also in the current hour with the minutes past (POSIX leaves the equal hour
//!   open; this is the reference's answer, D-23.7);
//! * a `%z` offset: the time is read in that offset; `%Z` is accepted and ignored (spec gap).
//!
//! The result is normalised as `mktime` does (`tm_isdst` from the zone unless `%s` gave it).
#![allow(clippy::arithmetic_side_effects)] // calendar fields of struct tm (i32) widened to i64

use liella::error::{Errno, KResult};
use liella::locale::Locale;
use liella::mem::TryVec;
use liella::time::civil::{self, Fields};
use liella::time::parse::{self, Local, Options, Parsed};
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{O_CLOEXEC, O_RDONLY, S_IFMT, S_IFREG};

/// `getdate_err` 1: `DATEMSK` is unset or empty.
pub const NO_DATEMSK: i32 = 1;
/// 2: the template file cannot be opened.
pub const CANNOT_OPEN: i32 = 2;
/// 3: its status cannot be read.
pub const NO_STATUS: i32 = 3;
/// 4: it is not a regular file.
pub const NOT_REGULAR: i32 = 4;
/// 5: reading it failed.
pub const READ_ERROR: i32 = 5;
/// 6: no memory.
pub const NO_MEMORY: i32 = 6;
/// 7: no template matches.
pub const NO_MATCH: i32 = 7;
/// 8: not a valid date.
pub const INVALID: i32 = 8;

/// What `getdate` asks of the process: the clock and the local zone.
pub trait Env: parse::Env {
    /// The current time.
    fn now(&self) -> KResult<i64>;
    /// `mktime` of `f` with `isdst` (negative: unknown): the local time it names.
    fn mktime(&self, f: &Fields, isdst: i32) -> KResult<Local<Self::Zone>>;
}

fn memory(e: Errno) -> i32 {
    if e == Errno::ENOMEM { NO_MEMORY } else { READ_ERROR }
}

/// The template file's bytes.
fn templates<S: Sys>(sys: &S, path: &[u8]) -> Result<TryVec<u8>, i32> {
    let mut z = TryVec::new();
    z.extend_from_slice(path).and_then(|()| z.push(0)).map_err(|_| NO_MEMORY)?;
    let p = KStr::from_nul_terminated(&z).map_err(|_| CANNOT_OPEN)?;
    let fd = sys.openat(Fd::CWD, p, O_RDONLY | O_CLOEXEC, 0).map_err(|_| CANNOT_OPEN)?;
    let r = (|| {
        let st = sys.fstat(fd).map_err(|_| NO_STATUS)?;
        if st.mode & S_IFMT != S_IFREG {
            return Err(NOT_REGULAR);
        }
        let mut data: TryVec<u8> = TryVec::with_capacity(usize::try_from(st.size).unwrap_or(0)).map_err(memory)?;
        let mut chunk = [0u8; 4096];
        loop {
            let n = match sys.read(fd, &mut chunk) {
                Ok(n) => n,
                Err(Errno::EINTR) => continue,
                Err(_) => return Err(READ_ERROR),
            };
            if n == 0 {
                return Ok(data);
            }
            data.extend_from_slice(chunk.get(..n).unwrap_or(&[])).map_err(memory)?;
        }
    })();
    let _ = sys.close(fd);
    r
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r')
}

/// `getdate(input)` with `DATEMSK`'s value `datemsk`: the local time the input names, or the
/// `getdate_err` number.
pub fn getdate<S: Sys, E: Env>(sys: &S, datemsk: Option<&[u8]>, input: &[u8], loc: &Locale, env: &E) -> Result<Local<E::Zone>, i32> {
    let path = datemsk.filter(|p| !p.is_empty()).ok_or(NO_DATEMSK)?;
    let file = templates(sys, path)?;
    let start = input.iter().position(|&b| !is_space(b)).unwrap_or(input.len());
    let text = input.get(start..).unwrap_or(&[]);
    for line in file.split(|&b| b == b'\n') {
        if line.iter().all(|&b| is_space(b)) {
            continue;
        }
        let Some((n, p)) = parse::strptime(text, line, loc, env, Options { fold: true }) else { continue };
        if !text.get(n..).unwrap_or(&[]).iter().all(|&b| is_space(b)) {
            continue;
        }
        return complete(&p, env);
    }
    Err(NO_MATCH)
}

/// Fill what the template left out (module documentation) and normalise.
fn complete<E: Env>(p: &Parsed<E::Zone>, env: &E) -> Result<Local<E::Zone>, i32> {
    let now = env.now().map_err(|_| INVALID)?;
    let cur = env.local(now).map_err(|_| INVALID)?.civil;
    let field = |v: Option<i32>| v.map(i64::from);
    let (pyear, pmon, pmday, pwday) = (field(p.year).map(|y| y + 1900), field(p.mon), field(p.mday), field(p.wday));
    // A year alone is not a date here: an earlier hour still moves to the next day (the reference's answer).
    let date_given = pmon.is_some() || pmday.is_some() || p.yday.is_some() || pwday.is_some();
    let time_given = p.hour.is_some() || p.min.is_some() || p.sec.is_some();
    let cur_mon = i64::from(cur.month) - 1;
    let mut year = pyear.unwrap_or(cur.year);
    let (mon, mut mday) = match (pmon, pmday, field(p.yday)) {
        (Some(m), Some(d), _) => (m, d),
        (Some(m), None, _) => {
            if pyear.is_none() && m < cur_mon {
                year += 1;
            }
            (m, 1)
        }
        (None, Some(d), _) => (cur_mon, d),
        (None, None, Some(yd)) => {
            let (_, m, d) = civil::civil_from_days(civil::days_from_civil(i128::from(year), 1, 1) + i128::from(yd));
            (i64::from(m) - 1, i64::from(d))
        }
        (None, None, None) => (cur_mon, i64::from(cur.day)),
    };
    if !(0..12).contains(&mon) || mday < 1 || mday > civil::month_days(year, u8::try_from(mon + 1).unwrap_or(1)) {
        return Err(INVALID);
    }
    if let (Some(w), None) = (pwday, pmday) {
        // The first day from the base day on with that weekday.
        let wd = (civil::days_from_civil(i128::from(year), i128::from(mon) + 1, i128::from(mday)) + 4).rem_euclid(7);
        mday += (w - i64::try_from(wd).unwrap_or(0)).rem_euclid(7);
    }
    let (mut hour, mut min, mut sec) = (i64::from(cur.hour), i64::from(cur.minute), i64::from(cur.second));
    if time_given {
        (hour, min, sec) = (field(p.hour).unwrap_or(0), field(p.min).unwrap_or(0), field(p.sec).unwrap_or(0));
        // Only the hour decides: the current hour is today, minutes past or not (the reference's answer).
        if !date_given && hour < i64::from(cur.hour) {
            mday += 1;
        }
    }
    let f = Fields { year, month: mon + 1, day: mday, hour, minute: min, second: sec };
    if let Some(off) = p.gmtoff {
        let t = i64::try_from(f.seconds() - i128::from(off)).map_err(|_| INVALID)?;
        return env.local(t).map_err(|_| INVALID);
    }
    env.mktime(&f, p.isdst.unwrap_or(-1)).map_err(|_| INVALID)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;
    use liella::time::utc;

    /// UTC, at 2026-10-02 19:46:39 (a Friday).
    struct At;

    const NOW: i64 = 1_790_970_399;

    impl parse::Env for At {
        type Zone = ();

        fn local(&self, t: i64) -> KResult<Local<()>> {
            Ok(Local { civil: utc(t)?, isdst: false, offset: 0, zone: () })
        }
    }

    impl Env for At {
        fn now(&self) -> KResult<i64> {
            Ok(NOW)
        }

        fn mktime(&self, f: &Fields, _: i32) -> KResult<Local<()>> {
            let (_, c) = liella::time::timegm(f)?;
            Ok(Local { civil: c, isdst: false, offset: 0, zone: () })
        }
    }

    fn done(fmt: &str, input: &str) -> Result<(i64, u8, u8, u8, u8, u8), i32> {
        let (_, p) = parse::strptime(input.as_bytes(), fmt.as_bytes(), &Locale::C, &At, Options { fold: true }).ok_or(NO_MATCH)?;
        let l = complete(&p, &At)?;
        let c = l.civil;
        Ok((c.year, c.month, c.day, c.hour, c.minute, c.second))
    }

    #[test]
    fn posix_rules() {
        assert_eq!(utc(NOW).unwrap().weekday, 5);
        assert_eq!(done("%Y-%m-%d %H:%M:%S", "2024-02-29 13:05:09"), Ok((2024, 2, 29, 13, 5, 9)));
        assert_eq!(done("%Y-%m-%d", "2023-02-29"), Err(INVALID));
        assert_eq!(done("%a", "Fri"), Ok((2026, 10, 2, 19, 46, 39)), "today matches");
        assert_eq!(done("%a", "Mon"), Ok((2026, 10, 5, 19, 46, 39)));
        assert_eq!(done("%B", "October"), Ok((2026, 10, 1, 19, 46, 39)), "the current month, first day");
        assert_eq!(done("%B", "January"), Ok((2027, 1, 1, 19, 46, 39)), "a month past: next year");
        assert_eq!(done("%b %a", "Jan Fri"), Ok((2027, 1, 1, 19, 46, 39)));
        assert_eq!(done("%b %a %Y", "Jan Wed 1989"), Ok((1989, 1, 4, 19, 46, 39)));
        assert_eq!(done("%H:%M", "23:59"), Ok((2026, 10, 2, 23, 59, 0)), "still to come today");
        assert_eq!(done("%H:%M", "10:30"), Ok((2026, 10, 3, 10, 30, 0)), "passed: tomorrow");
        assert_eq!(done("%a %H", "Fri 9"), Ok((2026, 10, 2, 9, 0, 0)), "a date given: no moving");
        // A time alone against the current hour (19:46:39), s23_7_c_3: only the hour decides.
        for (input, day, why) in [
            ("20:00", 2, "a later hour: today"),
            ("19:50", 2, "the current hour, minutes ahead: today"),
            ("19:10", 2, "the current hour, minutes past: today"),
            ("19:46:00", 2, "the current minute, seconds past: today"),
            ("18:59", 3, "an earlier hour: tomorrow"),
        ] {
            let fmt = if input.len() > 5 { "%H:%M:%S" } else { "%H:%M" };
            assert_eq!(done(fmt, input).map(|c| c.2), Ok(day), "{input}: {why}");
        }
        assert_eq!(done("%Y %H:%M", "2030 10:00"), Ok((2030, 10, 3, 10, 0, 0)), "a year alone: an earlier hour is the next day");
        assert_eq!(done("%M", "50"), Ok((2026, 10, 3, 0, 50, 0)), "minutes alone: hour 0, before the current hour");
        assert_eq!(done("%b %H:%S", "Feb 10:30"), Ok((2027, 2, 1, 10, 0, 30)));
        assert_eq!(done("%m/%d", "1/1"), Ok((2026, 1, 1, 19, 46, 39)));
        assert_eq!(done("%b %d", "Feb 30"), Err(INVALID));
        assert_eq!(done("%Y %j", "2024 60"), Ok((2024, 2, 29, 19, 46, 39)));
        assert_eq!(done("%Y-%m-%d %H:%M %z", "2024-01-01 12:00 +0100"), Ok((2024, 1, 1, 11, 0, 0)));
    }
}
