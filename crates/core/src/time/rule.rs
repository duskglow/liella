//! The POSIX `TZ` rule grammar (POSIX.1-2017 XBD §8.3), with the TZif version 3 extension of
//! RFC 8536 §3.3.1 (transition times from −167 to 167 hours), and its evaluation (spec §23.6;
//! D-23.6).
//!
//! `std offset [dst [offset] [,start[/time],end[/time]]]`. Names are three or more letters, or
//! three or more letters, digits, `+` and `-` between `<` and `>`. Offsets are `[+-]hh[:mm[:ss]]`
//! west of UTC with hh 0–24 and mm, ss 0–59; a missing DST offset is one hour east of the
//! standard one. Dates are `Jn` (1–365, February 29 never counted), `n` (0–365) or `Mm.w.d`
//! (month 1–12, week 1–5 where 5 is the last, weekday 0–6); times default to 02:00.
//!
//! Validity (Liella's choice where the grammar leaves the outcome open): the standard name and
//! offset are required, and a TZ value without them is not a rule ([`Parsed::Invalid`]). The
//! DST part is optional, and so is each piece of it: a DST name that is not a name makes the
//! zone standard time only; a DST offset out of range is the default one; a start or end date
//! (with its time) that is missing or out of range is the default date, that of
//! `M3.2.0,M11.1.0`. glibc instead clamps out-of-range offsets, gives odd transitions for
//! out-of-range dates, and keeps or drops DST inconsistently for a missing or malformed rule
//! (spec/behaviour/zones.md).
#![allow(clippy::arithmetic_side_effects)] // bounded parse values (hours <= 167, days <= 366) and i128 instants derived from i64

use super::civil::{self, days_from_civil, is_leap, month_days, DAY};

/// One hour.
const HOUR: i64 = 3600;

/// A transition date in the year.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Date {
    /// `Jn`: day 1–365, February 29 never counted.
    Julian(u16),
    /// `n`: day 0–365, February 29 counted.
    Day(u16),
    /// `Mm.w.d`: month 1–12, week 1–5 (5 = the last), weekday 0–6.
    Month(u8, u8, u8),
}

/// A date and the local time of day of the change (seconds, −167 to 167 hours).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct When {
    /// The date.
    pub date: Date,
    /// Seconds after the date's midnight (local time before the change).
    pub time: i64,
}

/// The default start date (`M3.2.0/2`).
pub const DEFAULT_START: When = When { date: Date::Month(3, 2, 0), time: 2 * HOUR };
/// The default end date (`M11.1.0/2`).
pub const DEFAULT_END: When = When { date: Date::Month(11, 1, 0), time: 2 * HOUR };

/// A parsed rule: names are byte ranges of the parsed text.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rule<'a> {
    /// The standard abbreviation.
    pub std: &'a [u8],
    /// The standard offset, seconds east of UTC.
    pub std_offset: i64,
    /// Daylight-saving time, if any.
    pub dst: Option<Dst<'a>>,
}

/// The DST part of a rule.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Dst<'a> {
    /// The DST abbreviation.
    pub name: &'a [u8],
    /// The DST offset, seconds east of UTC.
    pub offset: i64,
    /// When DST starts (local standard time).
    pub start: When,
    /// When DST ends (local DST time).
    pub end: When,
}

/// The outcome of parsing a `TZ` value as a rule.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Parsed<'a> {
    /// A rule.
    Rule(Rule<'a>),
    /// Not a rule: the standard name, if the value begins with a valid one.
    Invalid(Option<&'a [u8]>),
}

struct Cursor<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Cursor<'a> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn bump(&mut self) {
        self.i += 1;
    }

    fn at_end(&self) -> bool {
        self.i >= self.s.len()
    }

    /// A name: `<...>` or a run of letters, at least three characters.
    fn name(&mut self) -> Option<&'a [u8]> {
        if self.peek() == Some(b'<') {
            self.bump();
            let start = self.i;
            while let Some(c) = self.peek() {
                if c == b'>' {
                    let n = self.s.get(start..self.i)?;
                    self.bump();
                    return (n.len() >= 3).then_some(n);
                }
                if !(c.is_ascii_alphanumeric() || c == b'+' || c == b'-') {
                    return None;
                }
                self.bump();
            }
            return None;
        }
        let start = self.i;
        while self.peek().is_some_and(|c| c.is_ascii_alphabetic()) {
            self.bump();
        }
        let n = self.s.get(start..self.i)?;
        (n.len() >= 3).then_some(n)
    }

    /// Decimal digits (at least one, at most `max` of them): the value.
    fn number(&mut self, max: usize) -> Option<i64> {
        let start = self.i;
        let mut v: i64 = 0;
        while let Some(c) = self.peek().filter(u8::is_ascii_digit) {
            if self.i - start >= max {
                return None;
            }
            v = v * 10 + i64::from(c - b'0');
            self.bump();
        }
        (self.i > start).then_some(v)
    }

    /// `[+-]hh[:mm[:ss]]` with hh at most `max_hours`: seconds (sign applied).
    fn hms(&mut self, max_hours: i64) -> Option<i64> {
        let sign = match self.peek() {
            Some(b'+') => {
                self.bump();
                1
            }
            Some(b'-') => {
                self.bump();
                -1
            }
            _ => 1,
        };
        let h = self.number(3)?;
        if h > max_hours {
            return None;
        }
        let mut secs = h * HOUR;
        for unit in [60, 1] {
            if self.peek() != Some(b':') {
                break;
            }
            self.bump();
            let v = self.number(2)?;
            if v > 59 {
                return None;
            }
            secs += v * unit;
        }
        Some(sign * secs)
    }

    /// A date with its optional time, filling the whole of the text.
    fn when(&mut self) -> Option<When> {
        let date = match self.peek()? {
            b'J' => {
                self.bump();
                let n = self.number(3)?;
                (1..=365).contains(&n).then(|| Date::Julian(u16::try_from(n).unwrap_or(1)))?
            }
            b'M' => {
                self.bump();
                let m = self.number(2)?;
                self.expect(b'.')?;
                let w = self.number(1)?;
                self.expect(b'.')?;
                let d = self.number(1)?;
                let ok = (1..=12).contains(&m) && (1..=5).contains(&w) && (0..=6).contains(&d);
                ok.then(|| Date::Month(u8::try_from(m).unwrap_or(1), u8::try_from(w).unwrap_or(1), u8::try_from(d).unwrap_or(0)))?
            }
            _ => {
                let n = self.number(3)?;
                (0..=365).contains(&n).then(|| Date::Day(u16::try_from(n).unwrap_or(0)))?
            }
        };
        let time = if self.peek() == Some(b'/') {
            self.bump();
            self.hms(167)?
        } else {
            2 * HOUR
        };
        self.at_end().then_some(When { date, time })
    }

    fn expect(&mut self, c: u8) -> Option<()> {
        (self.peek() == Some(c)).then(|| self.bump())
    }
}

/// A date and time from its whole text, `None` if it is not one.
fn when_of(text: Option<&[u8]>) -> Option<When> {
    Cursor { s: text?, i: 0 }.when()
}

/// Parse `text` (without a leading `:`).
pub fn parse(text: &[u8]) -> Parsed<'_> {
    let mut c = Cursor { s: text, i: 0 };
    let Some(std) = c.name() else { return Parsed::Invalid(None) };
    if !c.peek().is_some_and(|b| b.is_ascii_digit() || b == b'+' || b == b'-') {
        return Parsed::Invalid(Some(std));
    }
    let Some(west) = c.hms(24) else { return Parsed::Invalid(Some(std)) };
    let std_offset = -west;
    let mut rule = Rule { std, std_offset, dst: None };
    if c.at_end() {
        return Parsed::Rule(rule);
    }
    let Some(name) = c.name() else { return Parsed::Rule(rule) };
    let mut offset = std_offset + HOUR;
    if c.peek().is_some_and(|b| b.is_ascii_digit() || b == b'+' || b == b'-') {
        if let Some(w) = c.hms(24) {
            offset = -w;
        }
        // An offset out of range is the default one; skip what remains of it.
        while c.peek().is_some_and(|b| b.is_ascii_digit() || b == b':' || b == b'+' || b == b'-') {
            c.bump();
        }
    }
    let (mut start, mut end) = (DEFAULT_START, DEFAULT_END);
    if c.peek() == Some(b',') {
        let rest = text.get(c.i + 1..).unwrap_or(&[]);
        let mut parts = rest.splitn(2, |&b| b == b',');
        start = when_of(parts.next()).unwrap_or(DEFAULT_START);
        end = when_of(parts.next()).unwrap_or(DEFAULT_END);
    }
    rule.dst = Some(Dst { name, offset, start, end });
    Parsed::Rule(rule)
}

/// The day (since 1970-01-01) of `date` in `year`.
pub fn day_of(date: Date, year: i64) -> i128 {
    if year.unsigned_abs() < civil::FAST {
        return i128::from(day_of64(date, year));
    }
    let jan1 = days_from_civil(i128::from(year), 1, 1);
    match date {
        Date::Julian(n) => {
            let n = i128::from(n);
            jan1 + if is_leap(year) && n >= 60 { n } else { n - 1 }
        }
        Date::Day(n) => jan1 + i128::from(n),
        Date::Month(m, w, d) => {
            let first = days_from_civil(i128::from(year), i128::from(m), 1);
            let wd1 = (first + 4).rem_euclid(7);
            let mut day = 1 + (i128::from(d) - wd1).rem_euclid(7) + 7 * (i128::from(w) - 1);
            let len = i128::from(month_days(year, m));
            while day > len {
                day -= 7;
            }
            first + day - 1
        }
    }
}

/// [`day_of`] in 64-bit arithmetic, for years below `civil::FAST`.
fn day_of64(date: Date, year: i64) -> i64 {
    let jan1 = civil::days_from_civil64(year, 1, 1);
    match date {
        Date::Julian(n) => {
            let n = i64::from(n);
            jan1 + if is_leap(year) && n >= 60 { n } else { n - 1 }
        }
        Date::Day(n) => jan1 + i64::from(n),
        Date::Month(m, w, d) => {
            let first = civil::days_from_civil64(year, i64::from(m), 1);
            let wd1 = (first + 4).rem_euclid(7);
            let mut day = 1 + (i64::from(d) - wd1).rem_euclid(7) + 7 * (i64::from(w) - 1);
            let len = month_days(year, m);
            while day > len {
                day -= 7;
            }
            first + day - 1
        }
    }
}

impl Dst<'_> {
    /// The two changes of `year` as UTC instants: (DST starts, DST ends).
    pub fn changes(&self, std_offset: i64, year: i64) -> (i128, i128) {
        let day = i128::from(DAY);
        let start = day_of(self.start.date, year) * day + i128::from(self.start.time) - i128::from(std_offset);
        let end = day_of(self.end.date, year) * day + i128::from(self.end.time) - i128::from(self.offset);
        (start, end)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions)]
mod tests {
    use super::*;

    fn rule(s: &str) -> Rule<'_> {
        match parse(s.as_bytes()) {
            Parsed::Rule(r) => r,
            Parsed::Invalid(_) => panic!("{s} is not a rule"),
        }
    }

    #[test]
    fn grammar() {
        let r = rule("EST5EDT,M3.2.0,M11.1.0");
        assert_eq!((r.std, r.std_offset), (&b"EST"[..], -5 * 3600));
        let d = r.dst.unwrap();
        assert_eq!((d.name, d.offset, d.start, d.end), (&b"EDT"[..], -4 * 3600, DEFAULT_START, DEFAULT_END));
        let r = rule("<+0330>-3:30");
        assert_eq!((r.std, r.std_offset, r.dst), (&b"+0330"[..], 12_600, None));
        let r = rule("<-03>3<-02>,M3.5.0/-2,M10.5.0/-1");
        assert_eq!(r.dst.unwrap().start.time, -7200);
        assert_eq!(rule("AAA24:59:59").std_offset, -89_999);
        assert_eq!(rule("AAA3BBB,M3.2.0/167,M11.1.0/-167").dst.unwrap().end.time, -167 * 3600);
        assert_eq!(rule("AAA3BBB-1:30,M3.2.0,M11.1.0").dst.unwrap().offset, 5400);
        assert_eq!(parse(b"AAA"), Parsed::Invalid(Some(b"AAA")));
        assert_eq!(parse(b"AB1"), Parsed::Invalid(None));
        assert_eq!(parse(b"<AB>1"), Parsed::Invalid(None));
        assert_eq!(parse(b"AAA25"), Parsed::Invalid(Some(b"AAA")));
        assert_eq!(parse(b"Invalid//Zone"), Parsed::Invalid(Some(b"Invalid")));
        assert_eq!(parse(b""), Parsed::Invalid(None));
        // Optional pieces degrade to their defaults.
        assert_eq!(rule("AAA3BBB,M13.1.0,M11.1.0").dst.unwrap().start, DEFAULT_START);
        assert_eq!(rule("AAA3BBB,J0/2,J300/2").dst.unwrap().start, DEFAULT_START);
        assert_eq!(rule("AAA3BBB,M3.2.0").dst.unwrap().end, DEFAULT_END);
        assert_eq!(rule("AAA3BBB+25").dst.unwrap().offset, -7200);
        assert_eq!(rule("AAA3<BB>").dst, None);
    }

    #[test]
    fn dates() {
        // 2024: J60 is March 1, 59 is February 29, M3.2.0 is March 10, M10.5.0 October 27.
        let day = |d, y| super::super::civil::civil_from_days(day_of(d, y));
        assert_eq!(day(Date::Julian(60), 2024), (2024, 3, 1));
        assert_eq!(day(Date::Day(59), 2024), (2024, 2, 29));
        assert_eq!(day(Date::Month(3, 2, 0), 2024), (2024, 3, 10));
        assert_eq!(day(Date::Month(10, 5, 0), 2024), (2024, 10, 27));
        assert_eq!(day(Date::Month(11, 1, 0), 2024), (2024, 11, 3));
    }
}
