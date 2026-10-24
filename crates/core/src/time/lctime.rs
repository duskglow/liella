//! The `LC_TIME` items `strftime` and `strptime` read (spec §23.7, §7.4; POSIX `LC_TIME`): day
//! and month names, the AM/PM strings, the date and time formats, the eras and the alternative
//! digits, through the locale's `nl_langinfo` items (D-7.10).
//!
//! **Eras** (POSIX `era`): each entry is `direction:offset:start_date:end_date:era_name:era_format`.
//! Dates are `yyyy/mm/dd`; years before AD 1 are negative (`-0001` is 1 BC), so a negative year
//! `y` is the proleptic Gregorian year `y + 1`. The end date may be `+*` (the end of time) or
//! `-*` (the beginning of time). An era's year at its start date's year is `offset`; with `+`
//! the numbers grow towards the end date ("years closer to the start_date have lower numbers"),
//! with `-` they shrink, whichever way in time the end date lies (an era ending at `-*` counts
//! back into the past). The compiled item holds the entries as consecutive
//! NUL-terminated strings, as many as `_NL_TIME_ERA_NUM_ENTRIES` says.
//!
//! **Alternative digits** (`alt_digits`): up to 100 NUL-terminated strings, the symbols of the
//! numbers 0 to 99; an empty string means the number has none.
#![allow(clippy::arithmetic_side_effects)] // era years: i64 offsets and years from short decimal fields, saturating where they meet

use crate::locale::file::Cat;
use crate::locale::{Item, Locale};
use crate::text::{self, Encoding};

/// `ABDAY_1`: the abbreviated day names (Sunday first), 7 items.
pub const ABDAY: usize = 0;
/// `DAY_1`: the full day names, 7 items.
pub const DAY: usize = 7;
/// `ABMON_1`: the abbreviated month names, 12 items.
pub const ABMON: usize = 14;
/// `MON_1`: the full month names, 12 items.
pub const MON: usize = 26;
/// `AM_STR`.
pub const AM: usize = 38;
/// `PM_STR`.
pub const PM: usize = 39;
/// `D_T_FMT`.
pub const D_T_FMT: usize = 40;
/// `D_FMT`.
pub const D_FMT: usize = 41;
/// `T_FMT`.
pub const T_FMT: usize = 42;
/// `T_FMT_AMPM`.
pub const T_FMT_AMPM: usize = 43;
/// `ERA`: the era entries.
pub const ERA: usize = 44;
/// `ERA_D_FMT`.
pub const ERA_D_FMT: usize = 46;
/// `ALT_DIGITS`.
pub const ALT_DIGITS: usize = 47;
/// `ERA_D_T_FMT`.
pub const ERA_D_T_FMT: usize = 48;
/// `ERA_T_FMT`.
pub const ERA_T_FMT: usize = 49;
/// `_NL_TIME_ERA_NUM_ENTRIES`: the number of era entries (a word).
pub const ERA_NUM: usize = 50;
/// `_NL_TIME_CODESET`: the codeset the category's strings are in.
pub const CODESET: usize = 110;
/// `ALTMON_1`: the month names in the nominative case, 12 items.
pub const ALTMON: usize = 111;
/// `_NL_ABALTMON_1`: the abbreviated nominative month names, 12 items.
pub const ABALTMON: usize = 135;

/// `T_FMT_AMPM` when the locale has none (POSIX: `%r` is `%I:%M:%S %p` in the POSIX locale).
pub const DEFAULT_AMPM: &[u8] = b"%I:%M:%S %p";

/// A calendar date `(year, month 1–12, day)`, the year proleptic Gregorian (0 is 1 BC).
pub type Date = (i64, i64, i64);

/// One end of an era.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bound {
    /// `-*`: the beginning of time.
    Past,
    /// `+*`: the end of time.
    Future,
    /// A date.
    At(Date),
}

/// One era entry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Era {
    /// `+`: era years grow from the start date towards the end date; `-`: they shrink.
    pub upward: bool,
    /// The era year of the start date's year.
    pub offset: i64,
    /// The start date.
    pub start: Date,
    /// The end date.
    pub end: Bound,
    /// The era's name (`%EC`).
    pub name: &'static [u8],
    /// The era's year format (`%EY`); may be empty.
    pub format: &'static [u8],
}

impl Era {
    /// Whether `d` lies in the era (both ends included).
    pub fn contains(&self, d: Date) -> bool {
        match self.end {
            Bound::Future => d >= self.start,
            Bound::Past => d <= self.start,
            Bound::At(e) if self.forward() => self.start <= d && d <= e,
            Bound::At(e) => e <= d && d <= self.start,
        }
    }

    /// The era year of `year` (`%Ey`).
    pub fn year_of(&self, year: i64) -> i64 {
        self.offset.saturating_add(self.step().saturating_mul(year.saturating_sub(self.start.0)))
    }

    /// The year whose era year is `ey` (the inverse of [`Era::year_of`]).
    pub fn year_at(&self, ey: i64) -> i64 {
        self.start.0.saturating_add(self.step().saturating_mul(ey.saturating_sub(self.offset)))
    }

    /// Whether the end date lies after the start date.
    fn forward(&self) -> bool {
        match self.end {
            Bound::Future => true,
            Bound::Past => false,
            Bound::At(e) => e >= self.start,
        }
    }

    /// How an era year moves per calendar year: +1 when the numbers grow the way time runs
    /// from the start date, -1 when they run against it (direction times the end's side).
    fn step(&self) -> i64 {
        if self.upward == self.forward() {
            1
        } else {
            -1
        }
    }
}

/// A decimal field of an era entry, with an optional sign: 1 to 18 digits and nothing else.
fn signed_field(s: &[u8]) -> Option<i64> {
    let (neg, digits) = match s.split_first() {
        Some((b'-', rest)) => (true, rest),
        Some((b'+', rest)) => (false, rest),
        _ => (false, s),
    };
    if digits.is_empty() || digits.len() > 18 || !digits.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let v = digits.iter().fold(0i64, |v, &d| v * 10 + i64::from(d - b'0'));
    Some(if neg { -v } else { v })
}

/// `yyyy/mm/dd`, with a negative year counting from 1 BC; the month 1–12, the day 1–31.
fn date(s: &[u8]) -> Option<Date> {
    let mut parts = s.split(|&b| b == b'/');
    let (y, m, d) = (signed_field(parts.next()?)?, signed_field(parts.next()?)?, signed_field(parts.next()?)?);
    if parts.next().is_some() || !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    Some((if y < 0 { y + 1 } else { y }, m, d))
}

/// Parse one era entry; `None` if it is malformed.
pub fn parse_era(s: &'static [u8]) -> Option<Era> {
    // Six fields; the format, last, keeps any further colons.
    let mut f = s.splitn(6, |&b| b == b':');
    let upward = match f.next()? {
        b"+" => true,
        b"-" => false,
        _ => return None,
    };
    let offset = signed_field(f.next()?)?;
    let start = date(f.next()?)?;
    let end = match f.next()? {
        b"+*" => Bound::Future,
        b"-*" => Bound::Past,
        d => Bound::At(date(d)?),
    };
    let name = f.next()?;
    let format = f.next()?;
    Some(Era { upward, offset, start, end, name, format })
}

/// A locale's `LC_TIME`.
#[derive(Clone, Copy)]
pub struct Time<'l> {
    loc: &'l Locale,
}

impl<'l> Time<'l> {
    /// The `LC_TIME` of `loc`.
    pub fn of(loc: &'l Locale) -> Time<'l> {
        Time { loc }
    }

    /// Item `i` as a string, without its terminator.
    pub fn string(&self, i: usize) -> &'static [u8] {
        self.loc.string(Cat::Time, i)
    }

    /// Item `i`'s whole data (string arrays).
    fn data(&self, i: usize) -> &'static [u8] {
        match self.loc.item(Cat::Time, i) {
            Item::Bytes(b) => b,
            Item::Word(_) => b"",
        }
    }

    /// Item `base + k` for `k` in `0..n`, `None` outside.
    fn nth(&self, base: usize, n: i64, k: i64) -> Option<&'static [u8]> {
        let k = usize::try_from(k).ok().filter(|_| (0..n).contains(&k))?;
        Some(self.string(base + k))
    }

    /// The abbreviated name of weekday `w` (0 = Sunday), `None` out of range.
    pub fn abday(&self, w: i64) -> Option<&'static [u8]> {
        self.nth(ABDAY, 7, w)
    }

    /// The full name of weekday `w`.
    pub fn day(&self, w: i64) -> Option<&'static [u8]> {
        self.nth(DAY, 7, w)
    }

    /// Month `m`'s (0 = January) name from the 12 items at `base`.
    pub fn month(&self, base: usize, m: i64) -> Option<&'static [u8]> {
        self.nth(base, 12, m)
    }

    /// The format of a composite conversion: the era form when `era` and the locale has one,
    /// else the plain one (`T_FMT_AMPM` falls back to [`DEFAULT_AMPM`]).
    pub fn format(&self, plain: usize, era: Option<usize>) -> &'static [u8] {
        if let Some(f) = era.map(|e| self.string(e)).filter(|f| !f.is_empty()) {
            return f;
        }
        match self.string(plain) {
            b"" if plain == T_FMT_AMPM => DEFAULT_AMPM,
            f => f,
        }
    }

    /// The NUL-terminated strings of array item `i`, at most `n` of them.
    fn strings(&self, i: usize, n: usize) -> impl Iterator<Item = &'static [u8]> + 'static {
        let d = self.data(i);
        // A trailing terminator ends the last string; it does not start another.
        let d = d.strip_suffix(b"\0").unwrap_or(d);
        d.split(|&b| b == 0).filter(move |_| !d.is_empty()).take(n)
    }

    /// The era entries, in the locale's order (malformed entries skipped).
    pub fn eras(&self) -> impl Iterator<Item = Era> + 'static {
        let n = match self.loc.item(Cat::Time, ERA_NUM) {
            Item::Word(n) => usize::try_from(n).unwrap_or(0),
            Item::Bytes(_) => 0,
        };
        self.strings(ERA, n).filter_map(parse_era)
    }

    /// Whether the locale has eras.
    pub fn has_eras(&self) -> bool {
        self.eras().next().is_some()
    }

    /// The era of `d`: the first entry that contains it.
    pub fn era_of(&self, d: Date) -> Option<Era> {
        self.eras().find(|e| e.contains(d))
    }

    /// The alternative symbol of `n` (0–99), if the locale has one.
    pub fn alt_digit(&self, n: i64) -> Option<&'static [u8]> {
        let n = usize::try_from(n).ok().filter(|&n| n < 100)?;
        self.strings(ALT_DIGITS, 100).nth(n).filter(|s| !s.is_empty())
    }

    /// The alternative symbols: `(number, symbol)` for each number that has one.
    pub fn alt_digits(&self) -> impl Iterator<Item = (i64, &'static [u8])> + 'static {
        (0i64..).zip(self.strings(ALT_DIGITS, 100)).filter(|(_, s)| !s.is_empty())
    }

    /// The encoding of the category's strings (`_NL_TIME_CODESET`): `None` when it is not a
    /// built-in one, and the caller uses `LC_CTYPE`'s.
    pub fn encoding(&self) -> Option<Encoding> {
        text::builtin_charset(self.string(CODESET))
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn era_entries() {
        let e = parse_era(b"+:2:2020/01/01:+*:Reiwa:%EC%Ey").unwrap();
        assert_eq!((e.upward, e.offset, e.start, e.end, e.name, e.format), (true, 2, (2020, 1, 1), Bound::Future, &b"Reiwa"[..], &b"%EC%Ey"[..]));
        assert!(e.contains((2024, 1, 1)) && !e.contains((2019, 12, 31)));
        assert_eq!((e.year_of(2024), e.year_at(6)), (6, 2024));
        // 1 BC is year 0; an era ending at the beginning of time counts back from its start.
        let bc = parse_era(b"+:1:-0001/12/31:-*:BC:%EC%Ey").unwrap();
        assert_eq!(bc.start, (0, 12, 31));
        assert!(bc.contains((0, 1, 1)) && bc.contains((-1, 6, 1)) && !bc.contains((1, 1, 1)));
        assert_eq!((bc.year_of(0), bc.year_of(-1), bc.year_at(2)), (1, 2, -1));
        // `-`: the numbers shrink towards the end date.
        let down = parse_era(b"-:100:2000/01/01:+*:D:%Ey").unwrap();
        assert_eq!((down.year_of(2000), down.year_of(2010), down.year_at(90)), (100, 90, 2010));
        let be = parse_era(b"+:1:-543/01/01:+*:BE:%EC %Ey").unwrap();
        assert_eq!((be.start.0, be.year_of(2024)), (-542, 2567));
        // A bounded era, the end before the start or after it.
        let h = parse_era(b"+:1:1989/01/08:1989/12/31:H:%EC1").unwrap();
        assert!(h.contains((1989, 1, 8)) && h.contains((1989, 12, 31)) && !h.contains((1989, 1, 7)));
        assert!(parse_era(b"x:1:2020/01/01:+*:N:F").is_none());
        assert!(parse_era(b"+:1:2020/01:+*:N:F").is_none());
        assert_eq!(parse_era(b"+:1:2020/01/01:+*:N:a:b").unwrap().format, b"a:b");
    }

    #[test]
    fn c_locale() {
        let t = Time::of(&Locale::C);
        assert_eq!(t.abday(1), Some(&b"Mon"[..]));
        assert_eq!(t.day(7), None);
        assert_eq!(t.month(MON, 11), Some(&b"December"[..]));
        assert_eq!(t.month(ABMON, -1), None);
        assert_eq!(t.format(T_FMT_AMPM, None), b"%I:%M:%S %p");
        assert_eq!(t.format(D_T_FMT, Some(ERA_D_T_FMT)), b"%a %b %e %H:%M:%S %Y");
        assert!(!t.has_eras());
        assert_eq!(t.alt_digit(0), None);
        assert_eq!(t.alt_digits().count(), 0);
        assert_eq!(t.encoding(), Some(Encoding::Ascii));
    }
}
