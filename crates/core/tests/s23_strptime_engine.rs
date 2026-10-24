//! The `strptime` engine (`liella::time::parse`) and the `LC_TIME` reader (`liella::time::lctime`)
//! on their own (spec §23.7; D-23.7): every C-locale case of `tests/oracle/timefmt_diff.c`, which
//! `timefmt_products.rs` checks against the reference, plus era entries and the C locale's items.
//! Written before COLD-REWRITE2-cold-01 replaced both modules, to show the rewrite is equivalent.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, missing_docs)]

use liella::error::{Errno, Result};
use liella::locale::Locale;
use liella::time::civil;
use liella::time::lctime::{self, parse_era, Bound, Time};
use liella::time::parse::{strptime, Env, Local, Options, Parsed};

struct Utc;

impl Env for Utc {
    type Zone = u8;

    fn local(&self, t: i64) -> Result<Local<u8>> {
        Ok(Local { civil: civil::utc(t)?, isdst: false, offset: 0, zone: 9 })
    }
}

struct NoZone;

impl Env for NoZone {
    type Zone = u8;

    fn local(&self, _: i64) -> Result<Local<u8>> {
        Err(Errno::EOVERFLOW)
    }
}

/// The oracle's notation: `NULL`, or `+consumed` and each field that was set.
fn show(r: Option<(usize, Parsed<u8>)>) -> String {
    let Some((n, p)) = r else { return "NULL".into() };
    let mut s = format!("+{n}");
    for (name, v) in [("sec", p.sec), ("min", p.min), ("hour", p.hour), ("mday", p.mday), ("mon", p.mon), ("year", p.year), ("wday", p.wday), ("yday", p.yday), ("isdst", p.isdst)] {
        if let Some(v) = v {
            s += &format!(" {name}={v}");
        }
    }
    if let Some(g) = p.gmtoff {
        s += &format!(" gmtoff={g}");
    }
    if let Some(z) = p.zone {
        s += &format!(" zone={z}");
    }
    s
}

fn run(cases: &[(&str, &str, &str)], opts: Options) {
    let mut bad = Vec::new();
    for &(fmt, input, want) in cases {
        let got = show(strptime(input.as_bytes(), fmt.as_bytes(), &Locale::C, &Utc, opts));
        if got != want {
            bad.push(format!("strptime({input:?}, {fmt:?}): want {want:?} got {got:?}"));
        }
    }
    assert!(bad.is_empty(), "{} of {} cases differ:\n{}", bad.len(), cases.len(), bad.join("\n"));
}

#[test]
fn s23_7_c_2_oracle_cases() {
    // Clauses: s23_7_c_2 (D-23.7). The reference's own answer differs only where KNOWN says so.
    run(
        &[
            ("%Y-%m-%d", "2024-02-29", "+10 mday=29 mon=1 year=124 wday=4 yday=59"),
            ("%Y-%m-%d", "2023-02-29", "+10 mday=29 mon=1 year=123 wday=3 yday=59"),
            ("%Y-%m-%d %H:%M:%S", "2024-07-04 13:05:09", "+19 sec=9 min=5 hour=13 mday=4 mon=6 year=124 wday=4 yday=185"),
            ("%Y-%m-%d", "2024-7-4", "+8 mday=4 mon=6 year=124 wday=4 yday=185"),
            ("%Y-%m-%d", " 2024 - 07 - 04", "NULL"),
            ("%Y %m", "2024    7", "+9 mon=6 year=124"),
            ("%d", "32", "NULL"),
            ("%d", "0", "NULL"),
            ("%m", "13", "NULL"),
            ("%H", "24", "NULL"),
            ("%M", "60", "+1 min=6"),
            ("%S", "60", "+2 sec=60"),
            ("%S", "61", "+2 sec=61"),
            ("%S", "62", "NULL"),
            ("%y", "69", "+2 year=69"),
            ("%y", "68", "+2 year=168"),
            ("%y", "00", "+2 year=100"),
            ("%C%y", "1969", "+4 year=69"),
            ("%C", "20", "+2 year=100"),
            ("%y %C", "05 19", "+5 year=5"),
            ("%Y", "-5", "+2 year=-1905"),
            ("%Y", "+2024", "+5 year=124"),
            ("%Y", "123456", "+4 year=-666"),
            ("%G-%V", "2020-53", "+7"),
            ("%j", "366", "+3 yday=365"),
            ("%j", "367", "NULL"),
            ("%Y %j", "2024 060", "+8 mday=29 mon=1 year=124 wday=4 yday=59"),
            ("%U %Y", "10 2024", "+7 year=124"),
            ("%W %w %Y", "10 3 2024", "+9 mday=6 mon=2 year=124 wday=3 yday=65"),
            ("%u", "7", "+1 wday=0"),
            ("%w", "7", "NULL"),
            ("%a", "monday", "+6 wday=1"),
            ("%a", "Mon", "+3 wday=1"),
            ("%a", "MONDAY rest", "+6 wday=1"),
            ("%A", "Tues", "+3 wday=2"),
            ("%b", "september", "+9 mon=8"),
            ("%b", "sept", "+3 mon=8"),
            ("%h", "Dec", "+3 mon=11"),
            ("%B %d %Y", "February 29 2024", "+16 mday=29 mon=1 year=124 wday=4 yday=59"),
            ("%I:%M %p", "12:30 am", "+8 min=30 hour=0"),
            ("%I:%M %p", "12:30 PM", "+8 min=30 hour=12"),
            ("%I %p", "13 PM", "NULL"),
            ("%p %I", "pm 3", "+4 hour=15"),
            ("%r", "01:02:03 PM", "+11 sec=3 min=2 hour=13"),
            ("%R", "23:59", "+5 min=59 hour=23"),
            ("%T", "23:59:59", "+8 sec=59 min=59 hour=23"),
            ("%D", "12/31/99", "+8 mday=31 mon=11 year=99 wday=5 yday=364"),
            ("%F", "2024-01-15", "+10 mday=15 mon=0 year=124 wday=1 yday=14"),
            ("%c", "Mon Jan  1 00:00:00 2024", "+24 sec=0 min=0 hour=0 mday=1 mon=0 year=124 wday=1 yday=0"),
            ("%x", "01/02/24", "+8 mday=2 mon=0 year=124 wday=2 yday=1"),
            ("%X", "10:11:12", "+8 sec=12 min=11 hour=10"),
            ("%e", " 5", "+2 mday=5"),
            ("%k", " 7", "+2 hour=7"),
            ("%l", " 7", "+2 hour=7"),
            ("%n%t", " \t\n  ", "+5"),
            ("%n", "", "+0"),
            ("%%", "%", "+1"),
            ("%%", "x", "NULL"),
            ("%z", "+0530", "+5 gmtoff=19800"),
            ("%z", "-03:30", "+6 gmtoff=-12600"),
            ("%z", "+05", "+3 gmtoff=18000"),
            ("%z", "Z", "+1 gmtoff=0"),
            ("%z", "+2400", "+5 gmtoff=86400"),
            ("%z", "+1260", "NULL"),
            ("%Z", "EST", "+3"),
            ("%Z %Y", "EST 2024", "+8 year=124"),
            ("%s", "86400", "+5 sec=0 min=0 hour=0 mday=2 mon=0 year=70 wday=5 yday=1 isdst=0 gmtoff=0 zone=9"),
            ("%s", "-1", "+2 sec=59 min=59 hour=23 mday=31 mon=11 year=69 wday=3 yday=364 isdst=0 gmtoff=0 zone=9"),
            ("%s", "99999999999999999999", "NULL"),
            ("%Q", "x", "NULL"),
            ("%Y", "", "NULL"),
            ("%Y", "abc", "NULL"),
            ("abc", "ABC", "NULL"),
            ("a b", "a     b", "+7"),
            ("ab", "a b", "NULL"),
            (" %Y", "2024", "+4 year=124"),
            ("%Y ", "2024", "+4 year=124"),
            ("%EY", "2024", "+4 year=124"),
            ("%Ey", "24", "+2 year=124"),
            ("%Oy", "24", "+2 year=124"),
            ("%Od", "05", "+2 mday=5"),
            ("%Ec", "Mon Jan  1 00:00:00 2024", "+24 sec=0 min=0 hour=0 mday=1 mon=0 year=124 wday=1 yday=0"),
            ("%Ex", "01/02/24", "+8 mday=2 mon=0 year=124 wday=2 yday=1"),
            ("%OH:%OM", "07:08", "+5 min=8 hour=7"),
            ("%Y%m%d", "20240229", "+8 mday=29 mon=1 year=124 wday=4 yday=59"),
            ("%H%M%S", "235960", "+6 sec=60 min=59 hour=23"),
            ("%Y-%m-%d", "2024-02-29trailing", "+10 mday=29 mon=1 year=124 wday=4 yday=59"),
        ],
        Options::default(),
    );
}

/// (format, input, the oracle's answer, the date `timegm` makes of its year/mon/mday, and whether
/// that is the calendar date the input names).
type Case = (&'static str, &'static str, &'static str, (i64, u8, u8), bool);

#[test]
fn s23_7_c_2_neighbouring_year() {
    // Clauses: s23_7_c_2 (D-23.7, D-23.14 (4),
    // coordinator rulings 2026-10-21: match glibc's probed field values). A %U/%W week or %j day in
    // the next or previous year keeps the given year; yday counts from its January 1.
    // glibc 2.39, black-box probe on keke 2026-10-21 (strptime into a zeroed struct tm and into one
    // preset to mon=5 mday=15 yday=200 wday=3, same answers; then timegm of a copy):
    // After the year: "%Y %U %w" "2024 53 6" -> mday=42 mon=11 year=124 wday=6 yday=376, timegm
    // 2025-01-11; "%Y %W %w" "2024 53 3" -> 32/11/366, 2025-01-01; "%Y %j" "2023 366" -> 32/11
    // wday=1 yday=365, 2024-01-01; "%Y %U %w" "2020 53 6" -> 40/11/374, 2021-01-09.
    // Before the year: mon=-1, mday=yday+1 (less 365 in a leap year), so timegm lands a month or
    // more early: "%Y %W %w" "2024 00 0" -> mday=-365 mon=-1 wday=0 yday=-1, timegm 2022-11-30;
    // "%Y %U %w" "2023 00 0" -> -6/-1/-7, 2022-11-24; "%Y %U %w" "2016 00 0" -> -369/-1/-5,
    // 2014-11-26; "%Y %W %w" "2022 00 5" -> 0/-1/-1, 2021-11-30.
    // Liella also computes an ISO week date (glibc 2.39 sets nothing from %G/%V; differences row
    // "strptime %G/%V"); with %Y it follows the same rule: ISO week 1 of 2020 begins on 2019-12-30.
    let cases: [Case; 11] = [
        ("%Y %G %V %u", "2020 2020 1 1", "+13 mday=-366 mon=-1 year=120 wday=1 yday=-2", (2018, 11, 29), false),
        ("%Y %U %w", "2024 53 6", "+9 mday=42 mon=11 year=124 wday=6 yday=376", (2025, 1, 11), true),
        ("%Y %W %w", "2024 53 3", "+9 mday=32 mon=11 year=124 wday=3 yday=366", (2025, 1, 1), true),
        ("%Y %j", "2023 366", "+8 mday=32 mon=11 year=123 wday=1 yday=365", (2024, 1, 1), true),
        ("%Y %U %w", "2020 53 6", "+9 mday=40 mon=11 year=120 wday=6 yday=374", (2021, 1, 9), true),
        ("%Y %W %a", "2023 53 Sun", "+11 mday=38 mon=11 year=123 wday=0 yday=371", (2024, 1, 7), true),
        ("%Y %W %w", "2024 00 0", "+9 mday=-365 mon=-1 year=124 wday=0 yday=-1", (2022, 11, 30), false),
        ("%Y %U %a", "2023 0 Sun", "+10 mday=-6 mon=-1 year=123 wday=0 yday=-7", (2022, 11, 24), false),
        ("%Y %U %w", "2023 00 0", "+9 mday=-6 mon=-1 year=123 wday=0 yday=-7", (2022, 11, 24), false),
        ("%Y %U %w", "2016 00 0", "+9 mday=-369 mon=-1 year=116 wday=0 yday=-5", (2014, 11, 26), false),
        ("%Y %W %w", "2022 00 5", "+9 mday=0 mon=-1 year=122 wday=5 yday=-1", (2021, 11, 30), false),
    ];
    let mut bad = Vec::new();
    for (fmt, input, want, (y, m, d), right) in cases {
        let r = strptime(input.as_bytes(), fmt.as_bytes(), &Locale::C, &Utc, Options::default());
        let got = show(r);
        if got != want {
            bad.push(format!("strptime({input:?}, {fmt:?}): want {want:?} got {got:?}"));
            continue;
        }
        let p = r.unwrap().1;
        let f = civil::Fields {
            year: i64::from(p.year.unwrap()) + 1900,
            month: i64::from(p.mon.unwrap()) + 1,
            day: i64::from(p.mday.unwrap()),
            ..civil::Fields::default()
        };
        let (t, c) = civil::timegm(&f).unwrap();
        if (c.year, c.month, c.day) != (y, m, d) {
            bad.push(format!("{input:?} {fmt:?}: timegm gives {}-{}-{}, want {y}-{m}-{d}", c.year, c.month, c.day));
        }
        let weekday = (t.div_euclid(86_400) + 4).rem_euclid(7); // 1970-01-01 was a Thursday
        if right && i64::from(p.wday.unwrap()) != weekday {
            bad.push(format!("{input:?} {fmt:?}: wday {:?}, the date's is {weekday}", p.wday));
        }
    }
    assert!(bad.is_empty(), "{}", bad.join("\n"));
}

#[test]
fn s23_7_c_2_edges() {
    // Clauses: s23_7_c_2 (D-23.7): widths, flags, signs, ends of input, ISO weeks, folding. A week
    // or day-of-year date outside the year the input gave keeps that year, with fields out of range
    // relative to it (D-23.14 (4)).
    run(
        &[
            ("%3Y", "20245", "+3 year=-1698"),
            ("%_d", "07", "+2 mday=7"),
            ("%-m %^b", "3 MAR", "+5 mon=2"),
            ("%+Y", "2024", "+4 year=124"),
            ("%C", "+1", "+2 year=-1800"),
            ("%y", "+5", "+2 year=105"),
            ("%Y", "-", "NULL"),
            ("%Y", "+", "NULL"),
            ("%s", "+5", "+2 sec=5 min=0 hour=0 mday=1 mon=0 year=70 wday=4 yday=0 isdst=0 gmtoff=0 zone=9"),
            ("%m", "", "NULL"),
            ("%H:%M", "12:", "NULL"),
            ("%G %V %u", "2024 1 1", "+8 mday=1 mon=0 year=124 wday=1 yday=0"),
            ("%G %V %u", "2021 1 1", "+8 mday=4 mon=0 year=121 wday=1 yday=3"),
            ("%Y %W %a", "2024 0 Sun", "+10 mday=-365 mon=-1 year=124 wday=0 yday=-1"),
            ("%Y %U %a", "2023 1 Sun", "+10 mday=1 mon=0 year=123 wday=0 yday=0"),
            ("%Y %W %a", "2024 1 Mon", "+10 mday=1 mon=0 year=124 wday=1 yday=0"),
            ("%Y %j", "2024 366", "+8 mday=31 mon=11 year=124 wday=2 yday=365"),
            ("%Y %U %a", "2023 0 Sun", "+10 mday=-6 mon=-1 year=123 wday=0 yday=-7"),
            ("%Y %W %a", "2023 53 Sun", "+11 mday=38 mon=11 year=123 wday=0 yday=371"),
            ("%j %Y", "001 2023", "+8 mday=1 mon=0 year=123 wday=0 yday=0"),
            ("%Y %j", "2023 366", "+8 mday=32 mon=11 year=123 wday=1 yday=365"),
            ("%z", "-0000", "+5 gmtoff=0"),
            ("%z", "+05:3", "NULL"),
            ("%z", "+05x", "+3 gmtoff=18000"),
            ("%Z", "  ", "+0"),
            ("%Z%Y", "AB2024", "NULL"),
            ("%a%n", "Fri\n", "+4 wday=5"),
            ("%p", "x", "NULL"),
            ("%I", "0", "NULL"),
            ("%l %p", "12 pm", "+5 hour=12"),
            ("%y", "7", "+1 year=107"),
            ("x%", "x%", "NULL"),
            ("%E", "1", "NULL"),
            ("%O", "1", "NULL"),
            ("%EH", "1", "NULL"),
        ],
        Options::default(),
    );
    run(&[("Mon%n%dX", "mon 05x", "+7 mday=5"), ("ABC", "abc", "+3")], Options { fold: true });
    assert!(strptime(b"5", b"%s", &Locale::C, &NoZone, Options::default()).is_none(), "%s without a zone");
}

#[test]
fn s23_7_c_1_era_entries() {
    // Clauses: s23_7_c_1 (D-23.7): POSIX era entries, their dates and years.
    let e = parse_era(b"-:5:2000/06/15:1990/01/01:Back:%Ey").unwrap();
    assert_eq!((e.upward, e.offset, e.start, e.end), (false, 5, (2000, 6, 15), Bound::At((1990, 1, 1))));
    assert!(e.contains((1995, 1, 1)) && e.contains((1990, 1, 1)) && e.contains((2000, 6, 15)));
    assert!(!e.contains((2000, 6, 16)) && !e.contains((1989, 12, 31)));
    // `-` towards a past end: the numbers shrink as the end date is approached.
    assert_eq!((e.year_of(2000), e.year_of(1995), e.year_at(0)), (5, 0, 1995));
    let up_past = parse_era(b"+:1:2000/01/01:1990/01/01:P:").unwrap();
    assert_eq!((up_past.year_of(2000), up_past.year_of(1998), up_past.year_at(3), up_past.format), (1, 3, 1998, &b""[..]));
    let past = parse_era(b"+:1:1000/01/01:-*:Old:%Ey").unwrap();
    assert!(past.contains((-50_000, 1, 1)) && past.contains((1000, 1, 1)) && !past.contains((1000, 1, 2)));
    let fut = parse_era(b"+:0:1000/02/01:+*:New:").unwrap();
    assert!(fut.contains((9_999_999, 12, 31)) && !fut.contains((1000, 1, 31)));
    assert_eq!(fut.year_of(1000), 0);
    for bad in [&b""[..], b"+", b"+:1", b"+:1:2020/01/01", b"+:1:2020/01/01:+*", b"+:x:2020/01/01:+*:N:F", b"+:1:2020/1/x:+*:N:F", b"+:1:2020/01/01:*:N:F", b"+:1:2020/01/01/01:+*:N:F"] {
        assert!(parse_era(bad).is_none(), "{:?}", String::from_utf8_lossy(bad));
    }
    assert_eq!(parse_era(b"+:+3:2020/01/01:+*:N:F").map(|e| e.offset), Some(3), "a signed offset");
    assert_eq!(parse_era(b"+:1:+2020/01/01:+*:N:F").map(|e| e.start), Some((2020, 1, 1)), "a signed year");
    assert_eq!(parse_era(b"+:1:2020/01/01:+*::F").map(|e| e.name), Some(&b""[..]), "an empty name");
}

#[test]
fn s23_7_c_1_locale_items() {
    // Clauses: s23_7_c_1 (D-7.10, D-23.7): the C locale's LC_TIME items through the reader.
    let t = Time::of(&Locale::C);
    let days: Vec<_> = (0..7).map(|w| (t.abday(w).unwrap(), t.day(w).unwrap())).collect();
    assert_eq!(days[0], (&b"Sun"[..], &b"Sunday"[..]));
    assert_eq!(days[6], (&b"Sat"[..], &b"Saturday"[..]));
    assert_eq!((t.abday(-1), t.abday(7), t.day(-1)), (None, None, None));
    for m in 0..12 {
        assert_eq!(t.month(lctime::ALTMON, m), t.month(lctime::MON, m), "C: nominative = genitive");
        assert_eq!(t.month(lctime::ABALTMON, m), t.month(lctime::ABMON, m));
    }
    assert_eq!((t.month(lctime::ABMON, 0), t.month(lctime::MON, 12)), (Some(&b"Jan"[..]), None));
    assert_eq!((t.string(lctime::AM), t.string(lctime::PM)), (&b"AM"[..], &b"PM"[..]));
    assert_eq!(t.format(lctime::D_FMT, Some(lctime::ERA_D_FMT)), b"%m/%d/%y");
    assert_eq!(t.format(lctime::T_FMT, None), b"%H:%M:%S");
    assert_eq!(t.format(lctime::T_FMT_AMPM, Some(lctime::ERA_T_FMT)), b"%I:%M:%S %p");
    assert_eq!((t.eras().count(), t.era_of((2024, 1, 1)), t.alt_digit(5), t.alt_digit(100)), (0, None, None, None));
    let cu = Locale::c_utf8(b"C.UTF-8");
    let u = Time::of(&cu);
    assert_eq!((u.encoding(), u.day(3)), (Some(liella::text::Encoding::Utf8), Some(&b"Wednesday"[..])));
}
