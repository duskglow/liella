//! `strptime` (spec §23.7; D-23.7; POSIX `strptime`): text to broken-down time, the fields it
//! parses and those that follow from them.
//!
//! **Directives** are those of `strftime` ([`super::format`]). Flags (`_ - 0 ^ # +`) are
//! accepted and ignored; a width is the most digits a number may take (POSIX). `E` and `O`
//! work where the locale has eras or alternative digits and otherwise mean the unmodified
//! conversion (POSIX: "as if the unmodified conversion specification were used").
//!
//! * White space in the format (and `%n`, `%t`) matches any amount of white space, none
//!   included; any other character must match itself.
//! * Numbers may follow white space and take at most their field's digits (`%Y` 4, `%j` 3, ...),
//!   stopping early once another digit could only exceed the field's maximum (`%M` on `60`
//!   reads `6`); then the value must be in range (`%d` 1–31, `%H` 0–23, `%S` 0–61, ...).
//!   `%C`, `%y` and `%Y` accept a leading `+` or `-` (POSIX), and so does `%s`.
//! * `%y` alone is 1969–2068; with `%C` it is the century's year, in either order; with `%Y` it
//!   replaces the year's last two digits.
//! * Names (`%a %A %b %B %h %p`) match the locale's full or abbreviated forms, the longest that
//!   matches, ignoring case (per character, `LC_CTYPE`'s maps). `%I`/`%l` combine with `%p`
//!   wherever it appears (12 without `%p` is midnight); `%H` does not.
//! * `%z` is `Z`, or a sign and `hh`, `hhmm` or `hh:mm` (minutes below 60); `%Z` consumes the
//!   following non-blank text and sets nothing; `%s` sets every field from the local time of
//!   that instant.
//! * Eras: `%EC` matches an era's name, `%Ey` reads an era year (of the era `%EC` named, else of
//!   the locale's first era), `%EY` tries each era's format in turn, then a plain year.
//!
//! **Computed fields.** Only fields the input determines are set. The weekday and the day of
//! the year are computed when the date is known: from the year, month and day; from the year
//! and day of the year (which also give the month and day); from the year, `%U` or `%W` week and
//! weekday; or from the ISO year, week and weekday. A week or day-of-year date that falls in a
//! neighbouring year keeps the given year, with the day of the year out of range relative to it
//! (365 or -1) and the month and day as glibc 2.39 sets them (after the year December 32 onward,
//! which `mktime` normalises to the right date; before it month -1, see `finish`). A field the
//! input gave is never replaced.
//! Fields the input did not determine (for example the weekday after `%b` alone) are left as
//! they were.
#![allow(clippy::arithmetic_side_effects)] // values are bounded by their digit counts (at most 19 digits, checked) before any arithmetic

use crate::ctype::{wtrans, Trans};
use crate::error::Result;
use crate::locale::Locale;
use crate::text::{self, Decoded, Encoding, MbState};

use super::civil::{self, CivilTime};
use super::lctime::{self, Era, Time};

/// The fields `strptime` determined (`year` is `tm_year`, `mon` 0-based); `None`: left alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Parsed<Z> {
    /// `tm_sec`.
    pub sec: Option<i32>,
    /// `tm_min`.
    pub min: Option<i32>,
    /// `tm_hour`.
    pub hour: Option<i32>,
    /// `tm_mday`.
    pub mday: Option<i32>,
    /// `tm_mon`.
    pub mon: Option<i32>,
    /// `tm_year`.
    pub year: Option<i32>,
    /// `tm_wday`.
    pub wday: Option<i32>,
    /// `tm_yday`.
    pub yday: Option<i32>,
    /// `tm_isdst`.
    pub isdst: Option<i32>,
    /// `tm_gmtoff`.
    pub gmtoff: Option<i64>,
    /// `tm_zone` (from `%s`).
    pub zone: Option<Z>,
}

impl<Z> Default for Parsed<Z> {
    fn default() -> Parsed<Z> {
        Parsed { sec: None, min: None, hour: None, mday: None, mon: None, year: None, wday: None, yday: None, isdst: None, gmtoff: None, zone: None }
    }
}

/// Local time of an instant (`%s`).
#[derive(Clone, Copy, Debug)]
pub struct Local<Z> {
    /// The civil time.
    pub civil: CivilTime,
    /// Daylight time.
    pub isdst: bool,
    /// Seconds east of UTC.
    pub offset: i64,
    /// The zone abbreviation, as the caller represents it.
    pub zone: Z,
}

/// What the parser asks of the process's time zone.
pub trait Env {
    /// How the caller represents a zone abbreviation.
    type Zone: Copy;
    /// The local time of `t` (`%s`).
    fn local(&self, t: i64) -> Result<Local<Self::Zone>>;
}

/// Parsing options.
#[derive(Clone, Copy, Debug, Default)]
pub struct Options {
    /// Ordinary characters of the format match regardless of case (`getdate`'s templates).
    pub fold: bool,
}

/// The deepest nesting of locale formats (`%c` inside an era format inside `%EY` ...).
const DEPTH: u8 = 8;

/// Which day starts a `%U`/`%W` week.
#[derive(Clone, Copy)]
enum Week {
    /// `%U`: Sunday; week 1 begins on the year's first Sunday.
    Sunday,
    /// `%W`: Monday; week 1 begins on the year's first Monday.
    Monday,
}

/// What the conversions read so far: the result's fields plus the parts that only combine at
/// the end (year pieces, a 12-hour clock, week numbers).
#[derive(Clone, Copy)]
struct Acc<Z> {
    out: Parsed<Z>,
    /// `%Y` (the full year).
    year: Option<i64>,
    /// `%C`.
    century: Option<i64>,
    /// `%y`.
    yy: Option<i64>,
    /// The era `%EC` named (an index into the locale's eras).
    era: Option<usize>,
    /// `%Ey`.
    era_year: Option<i64>,
    /// `%I`/`%l`, 1–12.
    hour12: Option<i64>,
    /// `%p`: after noon.
    pm: Option<bool>,
    /// `%U` or `%W`.
    week: Option<(Week, i64)>,
    /// `%G` (`%g` widened).
    iso_year: Option<i64>,
    /// `%V`.
    iso_week: Option<i64>,
}

/// One parse: the locale, the zone and the input, shared by every level of nesting.
struct Scan<'a, E: Env> {
    loc: &'a Locale,
    time: Time<'a>,
    env: &'a E,
    input: &'a [u8],
    fold: bool,
    /// The input's (and the format's) encoding: `LC_CTYPE`'s.
    text: Encoding,
    /// The encoding of the `LC_TIME` strings.
    names: Encoding,
    eras: bool,
    alt: bool,
}

fn is_space(b: u8) -> bool {
    matches!(b, b' ' | b'\t' | b'\n' | b'\x0b' | b'\x0c' | b'\r')
}

fn skip_space(input: &[u8], pos: usize) -> usize {
    pos + input.get(pos..).map_or(0, |s| s.iter().take_while(|&&b| is_space(b)).count())
}

/// `%y`'s century rule: 69–99 are 1969–1999, 00–68 are 2000–2068.
fn pivot(yy: i64) -> i64 {
    if yy < 69 {
        2000 + yy
    } else {
        1900 + yy
    }
}

fn small(v: i64) -> Option<i32> {
    i32::try_from(v).ok()
}

/// One character of `s` in `enc`.
fn decode(s: &[u8], enc: Encoding) -> Option<(u32, usize)> {
    let mut st = MbState::INITIAL;
    match text::decode(&mut st, s, enc) {
        Decoded::Char(c, n) if n > 0 => Some((c, n)),
        _ => None,
    }
}

/// Parse `input` by `fmt` in `loc`: the bytes consumed and the fields determined, `None` when
/// the input does not match.
pub fn strptime<E: Env>(input: &[u8], fmt: &[u8], loc: &Locale, env: &E, opts: Options) -> Option<(usize, Parsed<E::Zone>)> {
    let time = Time::of(loc);
    let text = loc.encoding();
    let scan = Scan { loc, time, env, input, fold: opts.fold, text, names: time.encoding().unwrap_or(text), eras: time.has_eras(), alt: time.alt_digits().next().is_some() };
    let mut acc = Acc { out: Parsed::default(), year: None, century: None, yy: None, era: None, era_year: None, hour12: None, pm: None, week: None, iso_year: None, iso_week: None };
    let end = scan.walk(&mut acc, 0, fmt, DEPTH, None)?;
    Some((end, scan.finish(acc)?))
}

impl<E: Env> Scan<'_, E> {
    /// Characters equal, or equal after `LC_CTYPE`'s lower-case map.
    fn same(&self, a: u32, b: u32) -> bool {
        a == b || wtrans(a, Trans::Lower, self.loc) == wtrans(b, Trans::Lower, self.loc)
    }

    /// The input bytes at `pos` that spell `name` ignoring case, if they do.
    fn prefix(&self, pos: usize, name: &[u8]) -> Option<usize> {
        let (mut i, mut j) = (pos, 0);
        while j < name.len() {
            let (a, na) = decode(name.get(j..)?, self.names)?;
            let (b, nb) = decode(self.input.get(i..)?, self.text)?;
            if !self.same(a, b) {
                return None;
            }
            i += nb;
            j += na;
        }
        Some(i - pos)
    }

    /// The longest non-empty candidate the input starts with (the first of equal lengths):
    /// its value and the input after it.
    fn longest(&self, pos: usize, cands: impl IntoIterator<Item = (i64, &'static [u8])>) -> Option<(i64, usize)> {
        let mut best: Option<(i64, usize)> = None;
        for (v, name) in cands {
            if name.is_empty() {
                continue;
            }
            if let Some(n) = self.prefix(pos, name) {
                if best.is_none_or(|(_, m)| n > m) {
                    best = Some((v, n));
                }
            }
        }
        best.map(|(v, n)| (v, pos + n))
    }

    /// Walk `fmt` from input position `pos`; `era` is the era whose format is being matched.
    fn walk(&self, acc: &mut Acc<E::Zone>, mut pos: usize, fmt: &[u8], depth: u8, era: Option<usize>) -> Option<usize> {
        let mut i = 0;
        while let Some(&c) = fmt.get(i) {
            if is_space(c) {
                pos = skip_space(self.input, pos);
                i += 1;
            } else if c != b'%' {
                let (n, used) = if self.fold { self.literal_fold(pos, fmt.get(i..)?)? } else { (usize::from(self.input.get(pos) == Some(&c)), 1) };
                if n == 0 {
                    return None;
                }
                pos += n;
                i += used;
            } else {
                i += 1;
                while matches!(fmt.get(i), Some(b'_' | b'-' | b'0' | b'^' | b'#' | b'+')) {
                    i += 1;
                }
                let mut width: Option<usize> = None;
                while let Some(d) = fmt.get(i).filter(|d| d.is_ascii_digit()) {
                    width = Some(width.unwrap_or(0).saturating_mul(10).saturating_add(usize::from(d - b'0')));
                    i += 1;
                }
                let modifier = fmt.get(i).copied().filter(|m| matches!(m, b'E' | b'O'));
                i += usize::from(modifier.is_some());
                let &conv = fmt.get(i)?;
                i += 1;
                pos = self.conversion(acc, pos, conv, modifier, width.filter(|&w| w > 0), depth, era)?;
            }
        }
        Some(pos)
    }

    /// An ordinary format character against the input, ignoring case: (input bytes, format bytes).
    fn literal_fold(&self, pos: usize, fmt: &[u8]) -> Option<(usize, usize)> {
        let (a, na) = decode(fmt, self.text)?;
        let (b, nb) = decode(self.input.get(pos..)?, self.text)?;
        self.same(a, b).then_some((nb, na))
    }

    /// A decimal number after optional white space: at most `digits` digits, stopping early once
    /// another digit could only exceed `hi`, then within `lo..=hi` (`-hi..=hi` when `signed`).
    fn number(&self, pos: usize, digits: usize, lo: i64, hi: i64, signed: bool) -> Option<(i64, usize)> {
        let mut p = skip_space(self.input, pos);
        let neg = match self.input.get(p) {
            Some(&s @ (b'+' | b'-')) if signed => {
                p += 1;
                s == b'-'
            }
            _ => false,
        };
        let (start, mut v) = (p, 0i64);
        while let Some(d) = self.input.get(p).filter(|d| d.is_ascii_digit() && p - start < digits) {
            if p > start && v * 10 > hi {
                break;
            }
            v = v * 10 + i64::from(d - b'0');
            p += 1;
        }
        let v = if neg { -v } else { v };
        let range = if signed { -hi..=hi } else { lo..=hi };
        (p > start && range.contains(&v)).then_some((v, p))
    }

    /// `%O`: the longest of the locale's alternative symbols in `lo..=hi`, else a plain number.
    fn alt_number(&self, pos: usize, digits: usize, lo: i64, hi: i64) -> Option<(i64, usize)> {
        let p = skip_space(self.input, pos);
        self.longest(p, self.time.alt_digits().filter(|(n, _)| (lo..=hi).contains(n))).or_else(|| self.number(pos, digits, lo, hi, false))
    }

    /// The era with index `k`.
    fn era(&self, k: usize) -> Option<Era> {
        self.time.eras().nth(k)
    }

    #[allow(clippy::too_many_arguments)]
    fn conversion(&self, acc: &mut Acc<E::Zone>, pos: usize, conv: u8, modifier: Option<u8>, width: Option<usize>, depth: u8, era: Option<usize>) -> Option<usize> {
        let valid = match modifier {
            Some(b'E') => matches!(conv, b'c' | b'C' | b'x' | b'X' | b'y' | b'Y'),
            Some(_) => matches!(conv, b'd' | b'e' | b'H' | b'I' | b'k' | b'l' | b'm' | b'M' | b'S' | b'u' | b'U' | b'V' | b'w' | b'W' | b'y' | b'b' | b'B' | b'h'),
            None => true,
        };
        if !valid {
            return None;
        }
        let e = modifier == Some(b'E') && self.eras;
        let alt = modifier == Some(b'O') && self.alt;
        // A bounded field: the number, or the locale's symbol for it under %O.
        let field = |digits: usize, lo: i64, hi: i64| {
            let digits = width.unwrap_or(digits);
            if alt {
                self.alt_number(pos, digits, lo, hi)
            } else {
                self.number(pos, digits, lo, hi, false)
            }
        };
        // A year-like field: signed (POSIX), as wide as its digits allow.
        let year = |digits: usize| {
            let digits = width.unwrap_or(digits).min(9);
            self.number(pos, digits, 0, 10i64.pow(u32::try_from(digits).unwrap_or(9)) - 1, true)
        };
        let set = |slot: &mut Option<i32>, (v, p): (i64, usize), base: i64| -> Option<usize> {
            *slot = Some(small(v - base)?);
            Some(p)
        };
        let o = &mut acc.out;
        match conv {
            b'%' => (self.input.get(pos) == Some(&b'%')).then_some(pos + 1),
            b'n' | b't' => Some(skip_space(self.input, pos)),
            b'a' | b'A' => {
                let t = self.time;
                set(&mut o.wday, self.longest(pos, (0..7).flat_map(|w| [t.abday(w), t.day(w)].into_iter().flatten().map(move |n| (w, n))))?, 0)
            }
            b'b' | b'B' | b'h' => {
                let t = self.time;
                let bases = [lctime::ABMON, lctime::MON, lctime::ABALTMON, lctime::ALTMON];
                set(&mut o.mon, self.longest(pos, (0..12).flat_map(|m| bases.into_iter().filter_map(move |b| t.month(b, m)).map(move |n| (m, n))))?, 0)
            }
            b'p' => {
                let (am, pm) = (self.time.string(lctime::AM), self.time.string(lctime::PM));
                if let Some((v, p)) = self.longest(pos, [(0, am), (1, pm)]) {
                    acc.pm = Some(v == 1);
                    Some(p)
                } else if am.is_empty() || pm.is_empty() {
                    // A locale without one of the strings: it matches the empty input.
                    acc.pm = (am.is_empty() != pm.is_empty()).then_some(pm.is_empty());
                    Some(pos)
                } else {
                    None
                }
            }
            b'c' | b'x' | b'X' | b'r' | b'D' | b'F' | b'R' | b'T' => {
                let t = self.time;
                let sub: &[u8] = match conv {
                    b'c' => t.format(lctime::D_T_FMT, e.then_some(lctime::ERA_D_T_FMT)),
                    b'x' => t.format(lctime::D_FMT, e.then_some(lctime::ERA_D_FMT)),
                    b'X' => t.format(lctime::T_FMT, e.then_some(lctime::ERA_T_FMT)),
                    b'r' => t.format(lctime::T_FMT_AMPM, None),
                    b'D' => b"%m/%d/%y",
                    b'F' => b"%Y-%m-%d",
                    b'R' => b"%H:%M",
                    _ => b"%H:%M:%S",
                };
                self.walk(acc, pos, sub, depth.checked_sub(1)?, era)
            }
            b'C' if e => {
                let (k, p) = self.longest(pos, (0i64..).zip(self.time.eras()).filter(|(k, _)| era.is_none_or(|x| usize::try_from(*k).ok() == Some(x))).map(|(k, x)| (k, x.name)))?;
                acc.era = usize::try_from(k).ok();
                Some(p)
            }
            b'C' => {
                let (v, p) = self.number(pos, width.unwrap_or(2), 0, 99, true)?;
                acc.century = Some(v);
                Some(p)
            }
            b'y' if e => {
                let (v, p) = self.number(pos, width.unwrap_or(8), 0, 99_999_999, false)?;
                acc.era_year = Some(v);
                Some(p)
            }
            b'y' => {
                let (v, p) = if alt { field(2, 0, 99)? } else { self.number(pos, width.unwrap_or(2), 0, 99, true).filter(|(v, _)| *v >= 0)? };
                acc.yy = Some(v);
                Some(p)
            }
            b'Y' if e => {
                for (k, x) in self.time.eras().enumerate() {
                    if x.format.is_empty() {
                        continue;
                    }
                    let saved = *acc;
                    acc.era = Some(k);
                    if let Some(p) = self.walk(acc, pos, x.format, depth.checked_sub(1)?, Some(k)) {
                        acc.era_year = acc.era_year.or(Some(x.offset));
                        return Some(p);
                    }
                    *acc = saved;
                }
                let (v, p) = year(4)?;
                acc.year = Some(v);
                Some(p)
            }
            b'Y' => {
                let (v, p) = year(4)?;
                acc.year = Some(v);
                Some(p)
            }
            b'G' => {
                let (v, p) = year(4)?;
                acc.iso_year = Some(v);
                Some(p)
            }
            b'g' => {
                let (v, p) = self.number(pos, width.unwrap_or(2), 0, 99, false)?;
                acc.iso_year = Some(pivot(v));
                Some(p)
            }
            b'm' => set(&mut o.mon, field(2, 1, 12)?, 1),
            b'd' | b'e' => set(&mut o.mday, field(2, 1, 31)?, 0),
            b'H' | b'k' => {
                acc.hour12 = None;
                set(&mut o.hour, field(2, 0, 23)?, 0)
            }
            b'I' | b'l' => {
                let (v, p) = field(2, 1, 12)?;
                (o.hour, acc.hour12) = (None, Some(v));
                Some(p)
            }
            b'M' => set(&mut o.min, field(2, 0, 59)?, 0),
            b'S' => set(&mut o.sec, field(2, 0, 61)?, 0),
            b'j' => set(&mut o.yday, field(3, 1, 366)?, 1),
            b'u' => {
                let (v, p) = field(1, 1, 7)?;
                set(&mut o.wday, (v % 7, p), 0)
            }
            b'w' => set(&mut o.wday, field(1, 0, 6)?, 0),
            b'U' | b'W' => {
                let (v, p) = field(2, 0, 53)?;
                acc.week = Some((if conv == b'U' { Week::Sunday } else { Week::Monday }, v));
                Some(p)
            }
            b'V' => {
                let (v, p) = field(2, 1, 53)?;
                acc.iso_week = Some(v);
                Some(p)
            }
            b'z' => {
                let (v, p) = self.offset(pos)?;
                o.gmtoff = Some(v);
                Some(p)
            }
            b'Z' => Some(pos + self.input.get(pos..).map_or(0, |s| s.iter().take_while(|&&b| b != 0 && !is_space(b)).count())),
            b's' => self.seconds(acc, pos),
            _ => None,
        }
    }

    /// `%z`: `Z`, or a sign, two digits of hours and optionally two of minutes (below 60),
    /// with or without a colon between them.
    fn offset(&self, pos: usize) -> Option<(i64, usize)> {
        let two = |p: usize| -> Option<i64> {
            let s = self.input.get(p..p + 2)?;
            s.iter().all(u8::is_ascii_digit).then(|| s.iter().fold(0, |v, &d| v * 10 + i64::from(d - b'0')))
        };
        let sign = match self.input.get(pos)? {
            b'Z' => return Some((0, pos + 1)),
            b'+' => 1,
            b'-' => -1,
            _ => return None,
        };
        let hh = two(pos + 1)?;
        let colon = usize::from(self.input.get(pos + 3) == Some(&b':'));
        let digit_at = |p: usize| self.input.get(p).is_some_and(u8::is_ascii_digit);
        let (mm, end) = if colon == 1 || digit_at(pos + 3) { (two(pos + 3 + colon)?, pos + 5 + colon) } else { (0, pos + 3) };
        (mm < 60).then_some((sign * (hh * 3600 + mm * 60), end))
    }

    /// `%s`: a signed count of seconds; every field from its local time.
    fn seconds(&self, acc: &mut Acc<E::Zone>, pos: usize) -> Option<usize> {
        let mut p = skip_space(self.input, pos);
        let neg = matches!(self.input.get(p), Some(b'-'));
        p += usize::from(matches!(self.input.get(p), Some(b'-' | b'+')));
        let start = p;
        let mut t = 0i64;
        while let Some(d) = self.input.get(p).filter(|d| d.is_ascii_digit()) {
            let d = i64::from(d - b'0');
            t = t.checked_mul(10)?.checked_add(if neg { -d } else { d })?;
            p += 1;
        }
        if p == start {
            return None;
        }
        let l = self.env.local(t).ok()?;
        let c = l.civil;
        acc.out = Parsed {
            sec: Some(i32::from(c.second)),
            min: Some(i32::from(c.minute)),
            hour: Some(i32::from(c.hour)),
            mday: Some(i32::from(c.day)),
            mon: Some(i32::from(c.month) - 1),
            year: Some(small(c.year - 1900)?),
            wday: Some(i32::from(c.weekday)),
            yday: Some(i32::from(c.yearday)),
            isdst: Some(i32::from(l.isdst)),
            gmtoff: Some(l.offset),
            zone: Some(l.zone),
        };
        (acc.year, acc.century, acc.yy, acc.era_year, acc.hour12) = (Some(c.year), None, None, None, None);
        Some(p)
    }

    /// The full year the year fields give: an era year, `%Y` (its last two digits replaced by
    /// `%y`), `%C` with `%y`, or `%y` alone.
    fn full_year(&self, a: &Acc<E::Zone>) -> Option<i64> {
        if let Some(ey) = a.era_year {
            let x = a.era.and_then(|k| self.era(k)).or_else(|| self.time.eras().next())?;
            return Some(x.year_at(ey));
        }
        match (a.year, a.century, a.yy) {
            (Some(y), _, Some(yy)) => Some(y - y.rem_euclid(100) + yy),
            (Some(y), _, None) => Some(y),
            (None, Some(c), yy) if c < 0 => Some(c * 100 - yy.unwrap_or(0)),
            (None, Some(c), yy) => Some(c * 100 + yy.unwrap_or(0)),
            (None, None, yy) => yy.map(pivot),
        }
    }

    /// Combine the pieces, then compute the date fields the input determines but did not give.
    fn finish(&self, a: Acc<E::Zone>) -> Option<Parsed<E::Zone>> {
        let mut o = a.out;
        let full = self.full_year(&a);
        if let Some(y) = full {
            o.year = Some(small(y - 1900)?);
        }
        if let Some(h) = a.hour12 {
            o.hour = Some(small(h % 12 + if a.pm == Some(true) { 12 } else { 0 })?);
        }
        let year = full.or(o.year.map(|y| i64::from(y) + 1900));
        let jan1 = |y: i64| civil::days_from_civil64(y, 1, 1);
        let weekday = |days: i64| (days + 4).rem_euclid(7); // 1970-01-01 was a Thursday
        let iso = || {
            let (g, v, w) = (a.iso_year?, a.iso_week?, o.wday?);
            let jan4 = civil::days_from_civil64(g, 1, 4);
            Some(jan4 - (weekday(jan4) + 6) % 7 + (v - 1) * 7 + (i64::from(w) + 6) % 7)
        };
        // The day (days since 1970-01-01) the input names, and whether it must lie in `year`.
        let day = match year {
            Some(y) => match (o.mon, o.mday, o.yday, a.week, o.wday) {
                (Some(m), Some(d), ..) => Some(civil::days_from_civil64(y, i64::from(m) + 1, i64::from(d))),
                (_, _, Some(yd), ..) => Some(jan1(y) + i64::from(yd)),
                (_, _, _, Some((k, n)), Some(w)) => {
                    let (j1, w) = (weekday(jan1(y)), i64::from(w));
                    let (first, offset) = match k {
                        Week::Sunday => ((7 - j1) % 7, w),
                        Week::Monday => ((8 - j1) % 7, (w + 6) % 7),
                    };
                    Some(jan1(y) + first + (n - 1) * 7 + offset)
                }
                _ => iso(),
            },
            None => iso(),
        };
        if let Some(day) = day {
            let (cy, cm, cd) = civil::civil_from_days64(day);
            o.wday = o.wday.or(small(weekday(day)));
            match year {
                // A week or day-of-year date in a neighbouring year keeps the year the input gave
                // and the day of the year counts from its January 1 (365+ after, negative before).
                // The month and day match glibc 2.39's probed values (coordinator ruling
                // 2026-10-21): after the year, December of the given year with the day running on
                // (December 32 is January 1), which `timegm` normalises to the right date; before
                // it, month -1 with day `yday + 1`, less 365 when the given year is a leap year,
                // which does not name the right date after `timegm` (glibc's does not either).
                Some(y) if y != cy => {
                    let yday = day - jan1(y);
                    let (m, mday) = if cy > y {
                        (11, day - civil::days_from_civil64(y, 12, 1) + 1)
                    } else {
                        let leap = jan1(y + 1) - jan1(y) == 366;
                        (-1, yday + 1 - if leap { 365 } else { 0 })
                    };
                    o.mon = o.mon.or(Some(m));
                    o.mday = o.mday.or(small(mday));
                    o.yday = o.yday.or(small(yday));
                }
                _ => {
                    o.year = o.year.or(small(cy - 1900));
                    o.mon = o.mon.or(Some(i32::from(cm) - 1));
                    o.mday = o.mday.or(Some(i32::from(cd)));
                    o.yday = o.yday.or(small(day - jan1(cy)));
                }
            }
        }
        Some(o)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::as_conversions)]
mod tests {
    use super::*;
    use crate::error::Errno;

    struct Utc;

    impl Env for Utc {
        type Zone = u8;

        fn local(&self, t: i64) -> Result<Local<u8>> {
            Ok(Local { civil: civil::utc(t)?, isdst: false, offset: 0, zone: 7 })
        }
    }

    struct NoZone;

    impl Env for NoZone {
        type Zone = u8;

        fn local(&self, _: i64) -> Result<Local<u8>> {
            Err(Errno::EOVERFLOW)
        }
    }

    fn p(fmt: &str, input: &str) -> Option<(usize, Parsed<u8>)> {
        strptime(input.as_bytes(), fmt.as_bytes(), &Locale::C, &Utc, Options::default())
    }

    /// (consumed, [sec, min, hour, mday, mon, year, wday, yday]) with -1 for unset.
    fn f(fmt: &str, input: &str) -> Option<(usize, [i32; 8])> {
        let (n, x) = p(fmt, input)?;
        let g = |v: Option<i32>| v.unwrap_or(-1);
        Some((n, [g(x.sec), g(x.min), g(x.hour), g(x.mday), g(x.mon), g(x.year), g(x.wday), g(x.yday)]))
    }

    #[test]
    fn dates_and_computed_fields() {
        assert_eq!(f("%Y-%m-%d", "2024-02-29"), Some((10, [-1, -1, -1, 29, 1, 124, 4, 59])));
        assert_eq!(f("%Y-%m-%d", "2023-02-29"), Some((10, [-1, -1, -1, 29, 1, 123, 3, 59])), "normalised for the computation only");
        assert_eq!(f("%Y %j", "2024 060"), Some((8, [-1, -1, -1, 29, 1, 124, 4, 59])));
        assert_eq!(f("%W %w %Y", "10 3 2024"), Some((9, [-1, -1, -1, 6, 2, 124, 3, 65])));
        assert_eq!(f("%Y %U %w", "2024 1 0"), Some((8, [-1, -1, -1, 7, 0, 124, 0, 6])));
        assert_eq!(f("%G %V %u", "2020 53 7"), Some((9, [-1, -1, -1, 3, 0, 121, 0, 2])), "ISO week date");
        // Nothing computed from fields the input did not give.
        assert_eq!(f("%b", "september"), Some((9, [-1, -1, -1, -1, 8, -1, -1, -1])));
        assert_eq!(f("%Y %m", "2024    7"), Some((9, [-1, -1, -1, -1, 6, 124, -1, -1])));
        assert_eq!(f("%m %d", "2 29"), Some((4, [-1, -1, -1, 29, 1, -1, -1, -1])));
        // A given weekday is kept.
        assert_eq!(f("%a %Y %m %d", "Mon 2024 2 29"), Some((13, [-1, -1, -1, 29, 1, 124, 1, 59])));
    }

    #[test]
    fn numbers() {
        assert_eq!(f("%Y", "123456").unwrap().0, 4);
        assert_eq!(f("%M", "60"), Some((1, [-1, 6, -1, -1, -1, -1, -1, -1])));
        assert_eq!(f("%d", "32"), None);
        assert_eq!(f("%d", "40").unwrap().1[3], 4);
        assert_eq!(f("%S", "61").unwrap().1[0], 61);
        assert_eq!(f("%S", "62"), None);
        assert_eq!(f("%j", "367"), None);
        assert_eq!(f("%u", "7").unwrap().1[6], 0);
        assert_eq!(f("%w", "7"), None);
        assert_eq!(f("%Y", " 2024").unwrap().1[5], 124, "after white space");
        assert_eq!(f("%4d", "0005").unwrap().1[3], 5, "a width is the most digits");
        assert_eq!(f("%2Y", "2024").unwrap(), (2, [-1, -1, -1, -1, -1, -1880, -1, -1]));
        // Years: signs (POSIX), %C and %y in either order, %y alone 1969-2068.
        assert_eq!(f("%Y", "-5").unwrap().1[5], -1905);
        assert_eq!(f("%Y", "+2024").unwrap().1[5], 124);
        assert_eq!((f("%y", "69").unwrap().1[5], f("%y", "68").unwrap().1[5]), (69, 168));
        assert_eq!(f("%C%y", "1969").unwrap().1[5], 69);
        assert_eq!(f("%y %C", "05 19").unwrap().1[5], 5);
        assert_eq!(f("%C", "20").unwrap().1[5], 100);
        assert_eq!(f("%C %y", "-1 05").unwrap().1[5], -2005);
        assert_eq!(f("%Y %y", "1950 07").unwrap().1[5], 7);
    }

    #[test]
    fn names_times_zones() {
        assert_eq!(f("%a", "MONDAY rest").unwrap().0, 6);
        assert_eq!(f("%a", "Monda").unwrap().0, 3);
        assert_eq!(f("%b", "sept").unwrap().0, 3);
        assert_eq!(f("%a", " Mon"), None, "names do not skip white space");
        assert_eq!(f("%I:%M %p", "12:30 am").unwrap().1[2], 0);
        assert_eq!(f("%p %I", "pm 3").unwrap().1[2], 15);
        assert_eq!(f("%H %p", "13 PM").unwrap().1[2], 13);
        assert_eq!(f("%I", "12").unwrap().1[2], 0);
        assert_eq!(f("%r", "01:02:03 PM").unwrap().1[..3], [3, 2, 13]);
        let z = |s: &str| p("%z", s).map(|(n, x)| (n, x.gmtoff.unwrap()));
        assert_eq!((z("+0530"), z("-03:30"), z("+05"), z("Z"), z("+2400")), (Some((5, 19_800)), Some((6, -12_600)), Some((3, 18_000)), Some((1, 0)), Some((5, 86_400))));
        assert_eq!((z("+1260"), z("+053"), z("+5"), z("z")), (None, None, None, None));
        assert_eq!(f("%Z %Y", "EST 2024").unwrap(), (8, [-1, -1, -1, -1, -1, 124, -1, -1]));
        assert_eq!(f("%Z", "").unwrap().0, 0);
        let (n, s) = p("%s", "86400").unwrap();
        assert_eq!((n, s.year, s.mday, s.wday, s.isdst, s.gmtoff, s.zone), (5, Some(70), Some(2), Some(5), Some(0), Some(0), Some(7)));
        assert_eq!(p("%s", "-1").unwrap().1.year, Some(69));
        assert!(p("%s", "99999999999999999999").is_none());
        assert!(strptime(b"5", b"%s", &Locale::C, &NoZone, Options::default()).is_none());
    }

    #[test]
    fn literals_and_modifiers() {
        assert_eq!(f("a b", "a     b").unwrap().0, 7);
        assert_eq!(f("ab", "a b"), None);
        assert_eq!(f("abc", "ABC"), None);
        assert!(strptime(b"ABC", b"abc", &Locale::C, &Utc, Options { fold: true }).is_some());
        assert_eq!(f("%n%t", " \t\n  ").unwrap().0, 5);
        assert_eq!(f("%%", "%").unwrap().0, 1);
        assert_eq!((f("%Q", "x"), f("%", "%"), f("%Ea", "Mon")), (None, None, None));
        // E and O without eras or alternative digits: the unmodified conversion.
        assert_eq!(f("%Ey", "24").unwrap().1[5], 124);
        assert_eq!(f("%OH:%OM", "07:08").unwrap().1[1..3], [8, 7]);
        assert_eq!(f("%EY", "2024").unwrap().1[5], 124);
        assert_eq!(f("%Ec", "Mon Jan  1 00:00:00 2024").unwrap().1, [0, 0, 0, 1, 0, 124, 1, 0]);
    }
}
