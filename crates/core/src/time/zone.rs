//! IP-23, the time-zone engine (spec §23.5–23.6; D-23.5, D-23.6): a [`Zone`] is the offset,
//! DST flag and abbreviation of every instant, built from a TZif file or a POSIX rule, and
//! answers both directions: instant to local time ([`Zone::at`], `localtime`) and local time to
//! instant ([`Zone::resolve`], `mktime`).
//!
//! **Lookup.** Before a file's first transition, its type 0 applies; between transitions, the
//! type the last one selected; after the last, the footer's rule when there is one, else the
//! last type (RFC 8536 §3.2, §3.3). A rule is evaluated from the changes of the year of the
//! instant and the years either side, so that change times beyond the year's ends (−167 to 167
//! hours, or dates near the turn of the year) fall where they belong.
//!
//! **Leap seconds.** In a file with leap-second records, instants count the inserted seconds:
//! the correction in force is subtracted before the calendar conversion, and the inserted
//! second itself reads as second 60. `resolve` treats the transitions of such a file as if they
//! were POSIX instants (an error of at most the correction, within that many seconds of a
//! transition).
//!
//! **Local to instant** (`mktime`; spec §23.5 asks that it be the inverse of `localtime` and
//! resolve `tm_isdst = -1`). The rules are glibc's as observed from outside, except where noted.
//! The fields other than the seconds give a wall time; the candidates are the instants whose
//! local time shows it. The seconds are then added as elapsed time (so 02:00:3600 in a skipped
//! hour is an hour after 02:00 read before the change, not 03:00).
//! * `tm_isdst < 0`: the earliest candidate (in a repeated hour, the first occurrence); in a
//!   skipped hour, the wall time read with the offset in force before the change (so the result
//!   is after it, as far past as the wall time was into the gap).
//! * `tm_isdst >= 0`: the earliest candidate with that flag; if none, the wall time read with
//!   the offset of the nearest period with that flag (so 02:30 with DST asked, in a skipped
//!   hour, reads as DST). DST asked of a zone that never has it reads one hour ahead of the time
//!   in force; standard time asked of a zone that never has it is ignored.
//!
//! Where Liella differs: glibc's choice of occurrence in a repeated hour with `tm_isdst = -1`
//! depends on the previous `mktime` call of the process (the first call of a process in a zone
//! east of UTC takes the later occurrence, a call after one that returned the earlier offset the
//! earlier one). Liella always takes the first occurrence.
#![allow(clippy::arithmetic_side_effects)] // i128 instants derived from i64 values and bounded offsets

use super::civil::{civil_from_days, civil_from_seconds, civil_from_seconds64, fits_tm, CivilTime, Fields, DAY};
use super::rule::{self, Date, Parsed, When};
use super::tzif;
use crate::error::{Errno, Result};
use crate::mem::{InlineVec, TryVec};
use core::sync::atomic::{AtomicI64, AtomicU32, Ordering::{Acquire, Relaxed, Release}};

/// An abbreviation: a static text, or one stored in its zone. Both are followed by a NUL in
/// memory ([`Zone::name_z`]).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Name {
    /// A static text, its NUL included.
    Static(&'static [u8]),
    /// Bytes `at..at + len` of the zone's names, followed by a NUL.
    Stored {
        /// Start.
        at: usize,
        /// Length (without the NUL).
        len: usize,
    },
}

/// The empty abbreviation.
pub const EMPTY: Name = Name::Static(b"\0");

/// Inline room for the transitions near one `mktime` instant (more spill to the heap).
const SPAN: usize = 8;

impl Default for Name {
    fn default() -> Name {
        EMPTY
    }
}

/// One kind of local time: offset east of UTC, DST flag, abbreviation.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Kind {
    /// Seconds east of UTC.
    pub offset: i64,
    /// Daylight-saving time.
    pub isdst: bool,
    /// The abbreviation.
    pub name: Name,
}

/// A POSIX rule with its abbreviations stored in the zone.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct Rule {
    std: Kind,
    dst: Option<(Kind, When, When)>,
}

/// What `tzset` publishes (spec §23.6): `tzname[0]`, `tzname[1]`, `timezone` (seconds west of
/// UTC) and `daylight`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Globals {
    /// The standard abbreviation.
    pub std: Name,
    /// The DST abbreviation.
    pub dst: Name,
    /// The standard offset, seconds *west* of UTC.
    pub timezone: i64,
    /// Whether the zone has (or had) daylight-saving time.
    pub daylight: bool,
}

/// A local time: the civil fields and the kind of time they are in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Local {
    /// The fields.
    pub civil: CivilTime,
    /// Daylight-saving time.
    pub isdst: bool,
    /// Seconds east of UTC.
    pub offset: i64,
    /// The abbreviation.
    pub name: Name,
}

/// A time zone (IP-23).
#[derive(Debug)]
pub struct Zone {
    names: TryVec<u8>,
    base: Kind,
    types: TryVec<Kind>,
    trans: TryVec<(i64, u8)>,
    /// Whether `trans` is in strictly increasing order (a well-formed file's), so that ranges
    /// of it can be found by binary search.
    sorted: bool,
    leaps: TryVec<(i64, i64)>,
    rule: Option<Rule>,
    /// The rule's two changes (DST starts, DST ends) of each year from [`RULE_FIRST`], each year
    /// computed once, when first needed: 16 bytes a year, [`RULE_YEARS`] years (4.8 KiB) for a
    /// rule with DST, nothing otherwise or without memory (the changes are then computed every
    /// time).
    rule_years: TryVec<YearSlot>,
    globals: Globals,
    /// Which zone this is, for [`Period`]: unique in the process and never reused (a zone read
    /// again is a new zone with a new number); 0 for a zone whose kind of time never changes
    /// (no transition and no rule with DST) or whose transitions are out of order, which has no
    /// period to remember.
    id: u32,
}

/// What a [`Period`] holds (`Period::kind`): the rule's standard time, its DST, the zone's base
/// kind, or (with the type's index in the low byte) a type of the file.
const P_RULE_STD: u16 = 0;
const P_RULE_DST: u16 = 1;
const P_BASE: u16 = 2;
const P_TYPE: u16 = 0x100;

/// The number of the next zone ([`Zone::id`]); a counter that wrapped reads as 0 (no caching).
static NEXT_ID: AtomicU32 = AtomicU32::new(1);

/// The period of constant local-time kind around the last instant a thread asked about (P7;
/// MKT-2: in a file's transitions too): the instants `lo..hi` (seconds since the epoch) in
/// which zone `id` has one kind of time, `kind` (`P_*`), and no change of it. A thread keeps one
/// (three words, [`Period::to_words`]); the zero one matches nothing. A rule's period lies inside
/// one year of the rule and past the file's last transition; a file's lies between two
/// transitions (or before the first, or after the last when there is no rule with DST); so
/// every instant in it gets the answer the full computation gives.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Period {
    id: u32,
    lo: i64,
    hi: i64,
    kind: u16,
}

impl Period {
    /// The number of words of [`Period::to_words`].
    pub const WORDS: usize = 3;

    /// The period as words (the thread's cache words, TB-SLOTS): the zone number with the kind
    /// in bits 32..48, then `lo` and `hi`.
    pub fn to_words(&self) -> [u64; Period::WORDS] {
        [u64::from(self.id) | (u64::from(self.kind) << 32), u64::from_ne_bytes(self.lo.to_ne_bytes()), u64::from_ne_bytes(self.hi.to_ne_bytes())]
    }

    /// The period of `w`, made by [`Period::to_words`] (any other words are some period, which
    /// matches a zone only if its number is that zone's; all zero is the zero period).
    pub fn from_words(w: &[u64; Period::WORDS]) -> Period {
        let [a, lo, hi] = *w;
        Period {
            id: u32::try_from(a & 0xffff_ffff).unwrap_or(0),
            lo: i64::from_ne_bytes(lo.to_ne_bytes()),
            hi: i64::from_ne_bytes(hi.to_ne_bytes()),
            kind: u16::try_from((a >> 32) & 0xffff).unwrap_or(0),
        }
    }

    /// Whether instant `t` is in the period.
    fn holds(&self, t: i128) -> bool {
        i128::from(self.lo) <= t && t < i128::from(self.hi)
    }

    /// [`Period::holds`] for an `i64` instant.
    fn holds64(&self, t: i64) -> bool {
        self.lo <= t && t < self.hi
    }
}

/// One year's changes as instants, kept once computed (`start` is `i64::MIN` until then; `end`
/// is stored first, so a `start` read with acquire ordering finds its `end`). Computing a year
/// twice (two threads at once) stores the same values.
#[derive(Debug)]
struct YearSlot {
    start: AtomicI64,
    end: AtomicI64,
}

/// An `i128` instant as an `i64`, saturating.
fn sat(v: i128) -> i64 {
    i64::try_from(v).unwrap_or(if v < 0 { i64::MIN } else { i64::MAX })
}

/// The first year of [`Zone::rule_years`].
const RULE_FIRST: i64 = 1900;
/// The years [`Zone::rule_years`] covers.
const RULE_YEARS: i64 = 300;

/// The year containing local second `secs`.
fn year_of(secs: i128) -> i64 {
    if let Ok(s) = i64::try_from(secs) {
        return crate::time::civil::civil_from_days64(s.div_euclid(DAY)).0;
    }
    let (y, _, _) = civil_from_days(secs.div_euclid(i128::from(DAY)));
    i64::try_from(y).unwrap_or(if y < 0 { i64::MIN } else { i64::MAX })
}

impl Rule {
    /// The changes of `year`: (instant, DST from then on); from `years` (the zone's table from
    /// [`RULE_FIRST`]) when it covers the year.
    fn changes(&self, year: i64, years: &[YearSlot]) -> Option<[(i128, bool); 2]> {
        let (dst, start, end) = self.dst?;
        if let Some(slot) = year.checked_sub(RULE_FIRST).and_then(|k| usize::try_from(k).ok()).and_then(|k| years.get(k)) {
            let s = slot.start.load(Acquire);
            if s != i64::MIN {
                return Some([(i128::from(s), true), (i128::from(slot.end.load(Relaxed)), false)]);
            }
            let [(s, _), (e, _)] = self.changes(year, &[])?;
            if let (Ok(s), Ok(e)) = (i64::try_from(s), i64::try_from(e)) {
                if s != i64::MIN {
                    slot.end.store(e, Relaxed);
                    slot.start.store(s, Release);
                }
            }
            return Some([(s, true), (e, false)]);
        }
        let day = i128::from(DAY);
        let at = |w: When, off: i64| rule::day_of(w.date, year) * day + i128::from(w.time) - i128::from(off);
        Some([(at(start, self.std.offset), true), (at(end, dst.offset), false)])
    }

    /// The changes of the years around `t`, in time order (an end before a start at the same
    /// instant, so that year-round DST stays DST).
    fn around(&self, t: i128, years: &[YearSlot]) -> [(i128, bool); 6] {
        self.events(year_of(t + i128::from(self.std.offset)), years)
    }

    /// The changes of the years `y - 1` to `y + 1`, in time order.
    fn events(&self, y: i64, years: &[YearSlot]) -> [(i128, bool); 6] {
        let mut ev = [(0i128, false); 6];
        for (k, dy) in [-1i64, 0, 1].into_iter().enumerate() {
            if let Some([s, e]) = self.changes(y.saturating_add(dy), years) {
                if let Some(slot) = ev.get_mut(2 * k) {
                    *slot = s;
                }
                if let Some(slot) = ev.get_mut(2 * k + 1) {
                    *slot = e;
                }
            }
        }
        // The three years' changes come nearly always in order already (TZ-FRESH2): a sort is
        // run only when they are not (rules whose changes cross years).
        if !ev.is_sorted_by_key(|&(t, d)| (t, d)) {
            ev.sort_unstable_by_key(|&(t, d)| (t, d));
        }
        ev
    }

    fn at(&self, t: i128, years: &[YearSlot]) -> Kind {
        let Some((dst, _, _)) = self.dst else { return self.std };
        let ev = self.around(t, years);
        let flag = match ev.iter().rev().find(|e| e.0 <= t) {
            Some(e) => e.1,
            None => !ev.first().is_some_and(|e| e.1),
        };
        if flag { dst } else { self.std }
    }

    /// [`Rule::at`], with the period around `t` in which the answer is the same: between the
    /// neighbouring changes, cut to the year of `t` (the changes `at` looks at are those of the
    /// years around its year, so inside the year every instant gets the same ones). No period
    /// when `t` is past the last change looked at.
    fn at_period(&self, t: i128, years: &[YearSlot]) -> (Kind, Option<(i128, i128)>) {
        let Some((dst, _, _)) = self.dst else { return (self.std, None) };
        let off = i128::from(self.std.offset);
        let y = year_of(t + off);
        let ev = self.events(y, years);
        let (flag, lo, hi) = match ev.iter().rposition(|e| e.0 <= t) {
            Some(i) => (ev.get(i).is_some_and(|e| e.1), ev.get(i).map_or(t, |e| e.0), ev.get(i + 1).map(|e| e.0)),
            None => (!ev.first().is_some_and(|e| e.1), i128::MIN, ev.first().map(|e| e.0)),
        };
        let kind = if flag { dst } else { self.std };
        let Some(hi) = hi else { return (kind, None) };
        let day = i128::from(DAY);
        let jan1 = rule::day_of(Date::Day(0), y);
        let year_lo = jan1 * day - off;
        // The next January 1 is 365 or 366 days on: one conversion for both year ends.
        let year_hi = (jan1 + 365 + i128::from(super::civil::is_leap(y))) * day - off;
        (kind, Some((lo.max(year_lo), hi.min(year_hi))))
    }
}

/// Store `s` (deduplicated) in `names`, followed by a NUL.
fn intern(names: &mut TryVec<u8>, s: &[u8]) -> Result<Name> {
    let mut at = 0usize;
    for part in names.split(|&b| b == 0) {
        if part == s && at + part.len() < names.len() {
            return Ok(Name::Stored { at, len: s.len() });
        }
        at += part.len() + 1;
    }
    let at = names.len();
    names.extend_from_slice(s)?;
    names.push(0)?;
    Ok(Name::Stored { at, len: s.len() })
}

impl Zone {
    /// A zone of one kind of time, UTC, with static abbreviations (each with its NUL).
    pub const fn fixed(std: &'static [u8], dst: &'static [u8]) -> Zone {
        let base = Kind { offset: 0, isdst: false, name: Name::Static(std) };
        Zone {
            names: TryVec::new(),
            base,
            types: TryVec::new(),
            trans: TryVec::new(),
            sorted: true,
            leaps: TryVec::new(),
            rule: None,
            rule_years: TryVec::new(),
            globals: Globals { std: Name::Static(std), dst: Name::Static(dst), timezone: 0, daylight: false },
            id: 0,
        }
    }

    /// UTC named `name`, with no DST abbreviation: the zone of a `TZ` value that is neither a
    /// file nor a rule.
    fn unnamed_utc(name: &[u8]) -> Result<Zone> {
        let mut z = Zone::fixed(b"\0", b"\0");
        let n = intern(&mut z.names, name)?;
        z.base.name = n;
        z.globals.std = n;
        Ok(z)
    }

    /// The zone of a `TZ` value that is not a file: its rule, or UTC named by its standard
    /// name (empty if it has none).
    pub fn from_rule_text(text: &[u8]) -> Result<Zone> {
        match rule::parse(text) {
            Parsed::Invalid(name) => Zone::unnamed_utc(name.unwrap_or(&[])),
            Parsed::Rule(r) => {
                let mut z = Zone::fixed(b"\0", b"\0");
                let rule = z.store_rule(&r)?;
                z.base = rule.std;
                z.rule = Some(rule);
                z.number();
                z.fill_rule_years();
                let dst = rule.dst.map_or(rule.std, |d| d.0);
                z.globals = Globals { std: rule.std.name, dst: dst.name, timezone: -rule.std.offset, daylight: rule.dst.is_some() };
                Ok(z)
            }
        }
    }

    /// Give the zone its number ([`Zone::id`]) when it has periods to remember: its kind of time
    /// changes (a transition, a rule with DST) and its transitions are in order.
    fn number(&mut self) {
        let changes = !self.trans.is_empty() || self.rule.is_some_and(|r| r.dst.is_some());
        self.id = if changes && self.sorted { NEXT_ID.fetch_add(1, Relaxed) } else { 0 };
    }

    /// Make the (empty) [`Zone::rule_years`] table of a rule with DST; left without one when
    /// memory runs out (the changes are then computed every time, with the same results).
    fn fill_rule_years(&mut self) {
        if self.rule.is_none_or(|r| r.dst.is_none()) {
            return;
        }
        let mut v = TryVec::new();
        if v.reserve_exact(usize::try_from(RULE_YEARS).unwrap_or(0)).is_err() {
            return;
        }
        for _ in 0..RULE_YEARS {
            if v.push(YearSlot { start: AtomicI64::new(i64::MIN), end: AtomicI64::new(0) }).is_err() {
                return;
            }
        }
        self.rule_years = v;
    }

    fn store_rule(&mut self, r: &rule::Rule<'_>) -> Result<Rule> {
        let std = Kind { offset: r.std_offset, isdst: false, name: intern(&mut self.names, r.std)? };
        let dst = match r.dst {
            Some(d) => Some((Kind { offset: d.offset, isdst: true, name: intern(&mut self.names, d.name)? }, d.start, d.end)),
            None => None,
        };
        Ok(Rule { std, dst })
    }

    /// The zone of a TZif file. `EINVAL` if it is not one.
    pub fn from_tzif(bytes: &[u8]) -> Result<Zone> {
        let f = tzif::parse(bytes)?;
        let mut z = Zone::fixed(b"\0", b"\0");
        z.types.reserve(f.types.len())?;
        for t in f.types.iter() {
            let name = intern(&mut z.names, f.designation(t))?;
            z.types.push(Kind { offset: t.utoff, isdst: t.isdst, name })?;
        }
        z.base = *z.types.first().ok_or(Errno::EINVAL)?;
        z.trans = f.transitions;
        z.sorted = z.trans.windows(2).all(|w| matches!(w, [a, b] if a.0 < b.0));
        z.leaps = f.leaps;
        if let Parsed::Rule(r) = rule::parse(f.footer) {
            z.rule = Some(z.store_rule(&r)?);
            z.fill_rule_years();
        }
        z.number();
        z.globals = z.file_globals();
        Ok(z)
    }

    /// The globals of a file: each flag's abbreviation is the one of the last transition to a
    /// type with that flag (type 0 when there are none), the footer's names replacing them;
    /// `timezone` is the footer's standard offset, else that of the last standard type;
    /// `daylight` says whether any type or the footer has DST. A zone without DST has its
    /// standard abbreviation in both places.
    fn file_globals(&self) -> Globals {
        let (mut std, mut dst): (Option<Kind>, Option<Kind>) = (None, None);
        let mut note = |k: Kind| if k.isdst { dst = Some(k) } else { std = Some(k) };
        note(self.base);
        for &(_, i) in self.trans.iter() {
            if let Some(&k) = self.types.get(usize::from(i)) {
                note(k);
            }
        }
        if let Some(r) = self.rule {
            std = Some(r.std);
            if let Some((d, _, _)) = r.dst {
                dst = Some(d);
            }
        }
        let daylight = self.types.iter().any(|k| k.isdst) || self.rule.is_some_and(|r| r.dst.is_some());
        let std = std.or(dst).unwrap_or(self.base);
        let dst = dst.unwrap_or(std);
        Globals { std: std.name, dst: dst.name, timezone: -std.offset, daylight }
    }

    /// What `tzset` publishes for this zone.
    pub fn globals(&self) -> Globals {
        self.globals
    }

    /// The abbreviation's bytes followed by its NUL.
    pub fn name_z(&self, n: Name) -> &[u8] {
        match n {
            Name::Static(s) => s,
            Name::Stored { at, len } => self.names.get(at..at + len + 1).unwrap_or(b"\0"),
        }
    }

    /// The abbreviation's bytes.
    pub fn name(&self, n: Name) -> &[u8] {
        let z = self.name_z(n);
        z.get(..z.len().saturating_sub(1)).unwrap_or(&[])
    }

    /// The kind of time at instant `t`.
    fn kind_at(&self, t: i128) -> Kind {
        let n = self.trans.partition_point(|&(tt, _)| i128::from(tt) <= t);
        if n == 0 {
            return match (self.trans.is_empty(), self.rule) {
                (true, Some(r)) => r.at(t, &self.rule_years),
                _ => self.base,
            };
        }
        if n == self.trans.len() {
            if let Some(r) = self.rule {
                return r.at(t, &self.rule_years);
            }
        }
        self.trans.get(n - 1).and_then(|&(_, i)| self.types.get(usize::from(i))).copied().unwrap_or(self.base)
    }

    /// The leap-second correction in force at `t`, and whether `t` is an inserted second.
    fn leap(&self, t: i128) -> (i128, bool) {
        let n = self.leaps.partition_point(|&(occ, _)| i128::from(occ) <= t);
        let Some(&(occ, corr)) = n.checked_sub(1).and_then(|i| self.leaps.get(i)) else { return (0, false) };
        let prev = n.checked_sub(2).and_then(|i| self.leaps.get(i)).map_or(0, |l| l.1);
        (i128::from(corr), i128::from(occ) == t && corr > prev)
    }

    /// The kind a period of this zone holds (`Period::kind`; the base kind for one that names
    /// nothing of the zone, which a period of the zone never does).
    fn kind_of(&self, p: &Period) -> Kind {
        match (p.kind, self.rule) {
            (P_RULE_STD, Some(r)) => r.std,
            (P_RULE_DST, Some(r)) => r.dst.map_or(r.std, |d| d.0),
            (k, _) if k & P_TYPE != 0 => self.types.get(usize::from(k & 0xff)).copied().unwrap_or(self.base),
            _ => self.base,
        }
    }

    /// [`Zone::kind_at`] `t`, with the period around `t` in which every instant has that kind
    /// and no change of it (a transition, a rule's change) falls (MKT-2): between the
    /// neighbouring transitions of a file (found by binary search, which needs them in order:
    /// no period otherwise), else the rule's period ([`Rule::at_period`]) cut to start at the
    /// last transition, else from the last transition on.
    fn kind_period(&self, t: i128) -> (Kind, Option<Period>) {
        if self.id == 0 {
            return (self.kind_at(t), None);
        }
        let per = |lo: i64, hi: i64, kind: u16| Some(Period { id: self.id, lo, hi, kind });
        let n = self.trans.partition_point(|&(tt, _)| i128::from(tt) <= t);
        let prev = n.checked_sub(1).and_then(|i| self.trans.get(i)).copied();
        if let Some(&(next, _)) = self.trans.get(n) {
            return match prev.and_then(|(lo, i)| Some((lo, i, *self.types.get(usize::from(i))?))) {
                Some((lo, i, k)) => (k, per(lo, next, P_TYPE | u16::from(i))),
                None => (self.base, per(prev.map_or(i64::MIN, |p| p.0), next, P_BASE)),
            };
        }
        // Past the last transition (or none).
        let last = prev.map_or(i64::MIN, |p| p.0);
        match self.rule {
            Some(r) if r.dst.is_some() => {
                let (k, p) = r.at_period(t, &self.rule_years);
                let p = p.and_then(|(lo, hi)| per(sat(lo).max(last), sat(hi), if k.isdst { P_RULE_DST } else { P_RULE_STD }));
                (k, p)
            }
            Some(r) => (r.std, per(last, i64::MAX, P_RULE_STD)),
            None => match prev.and_then(|(_, i)| Some((i, *self.types.get(usize::from(i))?))) {
                Some((i, k)) => (k, per(last, i64::MAX, P_TYPE | u16::from(i))),
                None => (self.base, per(last, i64::MAX, P_BASE)),
            },
        }
    }

    /// [`Zone::kind_at`] through the period `c` (P7, MKT-2): the answer from `c` when `t` is in
    /// it, else computed and kept in `c` as the new period ([`Zone::kind_period`]).
    fn kind_via(&self, t: i128, c: &mut Period) -> Kind {
        if self.id != 0 && c.id == self.id && c.holds(t) {
            return self.kind_of(c);
        }
        let (k, p) = self.kind_period(t);
        *c = p.unwrap_or_default();
        k
    }

    /// `localtime`: the local time of instant `t`. `EOVERFLOW` when the year does not fit a
    /// `struct tm`.
    pub fn at(&self, t: i64) -> Result<Local> {
        self.local(t, self.kind_at(i128::from(t)))
    }

    /// [`Zone::at`] with the thread's [`Period`] `c`, which it reads and updates: the same
    /// answers.
    pub fn at_cached(&self, t: i64, c: &mut Period) -> Result<Local> {
        self.local(t, self.kind_via(i128::from(t), c))
    }

    /// Whether [`Zone::at_cached`] can use a [`Period`] at `t` (a zone whose kind of time changes,
    /// [`Zone::id`]): callers skip the thread's period otherwise.
    pub fn uses_period(&self, _t: i64) -> bool {
        self.id != 0
    }

    /// The local time of instant `t` of kind `k`.
    fn local(&self, t: i64, k: Kind) -> Result<Local> {
        let t = i128::from(t);
        let (corr, inserted) = self.leap(t);
        let mut civil = civil_from_seconds(t - corr + i128::from(k.offset))?;
        if !fits_tm(civil.year) {
            return Err(Errno::EOVERFLOW);
        }
        if inserted {
            civil.second = 60;
        }
        Ok(Local { civil, isdst: k.isdst, offset: k.offset, name: k.name })
    }

    /// The instants in `(a, b]` at which the kind of time may change, in order and without
    /// repeats. Inline storage: `mktime`'s six-day window holds a few (AUD-M6: no allocation
    /// per call); a file with more transitions there spills to the heap.
    fn boundaries(&self, a: i128, b: i128) -> Result<InlineVec<i128, SPAN>> {
        let mut out = InlineVec::<i128, SPAN>::new();
        // The transitions in (a, b]: found by binary search when they are sorted.
        let (lo, hi) = if self.sorted {
            let lo = self.trans.partition_point(|&(tt, _)| i128::from(tt) <= a);
            (lo, self.trans.partition_point(|&(tt, _)| i128::from(tt) <= b).max(lo))
        } else {
            (0, self.trans.len())
        };
        for &(tt, _) in self.trans.get(lo..hi).unwrap_or(&[]) {
            let tt = i128::from(tt);
            if tt > a && tt <= b {
                out.push(tt)?;
            }
        }
        let last = self.trans.last().map(|&(tt, _)| i128::from(tt));
        if let Some(r) = self.rule.filter(|r| r.dst.is_some()) {
            if last.is_none_or(|l| b >= l) {
                let lo = last.map_or(a, |l| a.max(l));
                let (y0, y1) = (year_of(lo).saturating_sub(1), year_of(b).saturating_add(1));
                let mut y = y0;
                while y <= y1 && y.saturating_sub(y0) < 16 {
                    for (tt, _) in r.changes(y, &self.rule_years).unwrap_or_default() {
                        if tt > lo && tt <= b {
                            out.push(tt)?;
                        }
                    }
                    y = y.saturating_add(1);
                }
            }
        }
        let all = out.as_mut_slice();
        all.sort_unstable();
        let mut dedup = InlineVec::<i128, SPAN>::new();
        let mut prev = None;
        for &t in all.iter() {
            if prev != Some(t) {
                dedup.push(t)?;
                prev = Some(t);
            }
        }
        Ok(dedup)
    }

    /// Whether some period of the zone has DST flag `f`.
    fn has_flag(&self, f: bool) -> bool {
        self.base.isdst == f
            || self.types.iter().any(|k| k.isdst == f)
            || self.rule.is_some_and(|r| !f || r.dst.is_some())
    }

    /// The offset of the period with flag `f` nearest to instant `r`.
    fn nearest_offset(&self, f: bool, r: i128, segs: &[(i128, Option<i128>, Kind)]) -> Option<i64> {
        let dist = |s: i128, e: Option<i128>| {
            if r < s {
                s - r
            } else if e.is_some_and(|e| r >= e) {
                r - e.unwrap_or(r) + 1
            } else {
                0
            }
        };
        let near = segs.iter().filter(|s| s.2.isdst == f).min_by_key(|s| dist(s.0, s.1)).map(|s| s.2.offset);
        if near.is_some() {
            return near;
        }
        let mut best: Option<(i128, i64)> = None;
        let mut consider = |d: i128, k: Kind| {
            if k.isdst == f && best.is_none_or(|b| d < b.0) {
                best = Some((d, k.offset));
            }
        };
        let first = self.trans.first().map(|&(tt, _)| i128::from(tt));
        consider(first.map_or(0, |ft| (r - ft).max(0)), self.base);
        for &(tt, i) in self.trans.iter() {
            if let Some(&k) = self.types.get(usize::from(i)) {
                consider((i128::from(tt) - r).abs(), k);
            }
        }
        if let Some(rule) = self.rule {
            let last = self.trans.last().map_or(r, |&(tt, _)| r.max(i128::from(tt)));
            let extra = last - r;
            consider(extra, rule.std);
            if let Some((d, _, _)) = rule.dst {
                let closest = rule.around(last, &self.rule_years).iter().filter(|e| e.1).map(|e| (e.0 - last).abs()).min().unwrap_or(0);
                consider(extra + closest, d);
            }
        }
        best.map(|b| b.1)
    }

    /// `mktime`: the instant whose local time shows `f`, by the rules of the module
    /// documentation, with its local time. `isdst`: `tm_isdst` (negative: unknown).
    /// `EOVERFLOW` when the instant does not fit a `time_t` or its year a `struct tm`.
    pub fn resolve(&self, f: &Fields, isdst: i32) -> Result<(i64, Local)> {
        self.resolve_in(f, isdst, None)
    }

    /// [`Zone::resolve`] with the thread's [`Period`] `c` (TB-SLOTS), which it reads and
    /// updates: the same answers. When the period holds the whole window `resolve` searches
    /// (so the window has no change and one kind), the answer comes from that kind without the
    /// search.
    pub fn resolve_cached(&self, f: &Fields, isdst: i32, c: &mut Period) -> Result<(i64, Local)> {
        self.resolve_in(f, isdst, Some(c))
    }

    /// Whether [`Zone::resolve_cached`] can use a [`Period`] for `f` (a rule zone with DST, and
    /// the window `resolve` searches past the last transition of a file): callers skip the
    /// thread's period otherwise, which costs time and cannot hit.
    pub fn resolve_uses_period(&self, _f: &Fields) -> bool {
        self.id != 0
    }

    fn resolve_in(&self, f: &Fields, isdst: i32, c: Option<&mut Period>) -> Result<(i64, Local)> {
        self.resolve_by(f, isdst, c, true)
    }

    /// [`Zone::resolve_in`]; `fast`: whether the shortcuts may answer (false: always the full
    /// search, the reference the tests compare the shortcuts with).
    fn resolve_by(&self, f: &Fields, isdst: i32, mut c: Option<&mut Period>, fast: bool) -> Result<(i64, Local)> {
        if fast && self.leaps.is_empty() {
            if let Some(r) = self.resolve_small(f, isdst, c.as_deref_mut()) {
                return r;
            }
        }
        // The seconds are elapsed time after the other fields' instant (as glibc).
        let l = Fields { second: 0, ..*f }.seconds();
        if i64::try_from(l + i128::from(f.second)).is_err() {
            return Err(Errno::EOVERFLOW);
        }
        let w = 3 * i128::from(DAY);
        let (lo, hi) = (l - w, l + w);
        // Standard time asked of a zone that never has it is ignored; DST asked of a zone that
        // never has it is one hour ahead of the time in force (as glibc, and as the TZ grammar's
        // default DST offset).
        let want = (isdst >= 0).then_some(isdst > 0).filter(|&d| d || self.has_flag(d));
        // A zone whose kind never changes (no transition, no DST rule: UTC, a fixed offset;
        // MKT-1): the search below would find no change, the one candidate `l - k.offset` and no
        // gap, and take it unless `want` is the other kind (then it searches as before).
        if fast && self.trans.is_empty() && self.rule.is_none_or(|r| r.dst.is_none()) {
            // [`Zone::kind_at`] of such a zone, at every instant.
            let k = self.rule.map_or(self.base, |r| r.std);
            if want.is_none_or(|d| d == k.isdst) {
                return self.resolved(l - i128::from(k.offset), f, c, Some(k), None);
            }
        }
        if fast && self.id != 0 {
            // A period holding the whole window (the thread's, else the one around `lo`, found
            // by binary search in a file's transitions; MKT-2): no change in (lo, hi] and its
            // kind `k` at `lo`. The search below would find the one candidate `l - k.offset` and
            // no gap, and take it unless `want` is the other kind.
            let p = match c.as_deref().filter(|p| p.id == self.id && p.holds(lo) && p.holds(hi)) {
                Some(&p) => Some(p),
                None => self.kind_period(lo).1.filter(|p| p.holds(hi)),
            };
            if let Some(p) = p {
                let k = self.kind_of(&p);
                if want.is_none_or(|d| d == k.isdst) {
                    return self.resolved(l - i128::from(k.offset), f, c, None, Some(p));
                }
            }
        }
        let mut segs = InlineVec::<(i128, Option<i128>, Kind), SPAN>::new();
        let bounds = self.boundaries(lo, hi)?;
        let mut start = lo;
        for &b in bounds.as_slice() {
            segs.push((start, Some(b), self.kind_at(start)))?;
            start = b;
        }
        segs.push((start, None, self.kind_at(start)))?;
        // Candidates (in time order) and the gap reading.
        let segs = segs.as_slice();
        let mut cands = InlineVec::<(i128, Kind), SPAN>::new();
        let mut gap: Option<i128> = None;
        for (i, &(s, e, k)) in segs.iter().enumerate() {
            let t = l - i128::from(k.offset);
            if (i == 0 || t >= s) && e.is_none_or(|e| t < e) {
                cands.push((t, k))?;
            }
            if let (Some(e), Some(next)) = (e, segs.get(i + 1)) {
                if gap.is_none() && t >= e && l - i128::from(next.2.offset) < e {
                    gap = Some(t);
                }
            }
        }
        let cands = cands.as_mut_slice();
        cands.sort_unstable_by_key(|c| c.0);
        let fallback = || l - i128::from(self.kind_at(l).offset);
        let t = match want {
            None => cands.first().map(|c| c.0).or(gap).unwrap_or_else(fallback),
            Some(d) => match cands.iter().find(|c| c.1.isdst == d) {
                Some(c) => c.0,
                None => {
                    let r = cands.first().map(|c| c.0).or(gap).unwrap_or_else(fallback);
                    match self.nearest_offset(d, r, segs) {
                        Some(off) => l - i128::from(off),
                        None => l - i128::from(self.kind_at(r).offset) - 3600,
                    }
                }
            },
        };
        self.resolved(t, f, c, None, None)
    }

    /// The common case of [`Zone::resolve_by`] in 64-bit arithmetic (TZ-FRESH3), for a zone
    /// without leap seconds and fields below 2^31: the answer when the window has one kind (a
    /// zone that never changes, or a period holding the whole window) and `isdst` does not ask
    /// for the other kind, else `None` (the full search answers). The same answers: the
    /// shortcuts of `resolve_by` with the instant `l - offset + second` and its local time
    /// `l + second` (no leap correction), checked by `resolve_shortcuts_change_nothing`.
    #[inline(always)]
    fn resolve_small(&self, f: &Fields, isdst: i32, c: Option<&mut Period>) -> Option<Result<(i64, Local)>> {
        let l = f.seconds64()?;
        let want = (isdst >= 0).then_some(isdst > 0).filter(|&d| d || self.has_flag(d));
        let ok = |k: Kind| want.is_none_or(|d| d == k.isdst);
        let (k, p) = if self.trans.is_empty() && self.rule.is_none_or(|r| r.dst.is_none()) {
            (self.rule.map_or(self.base, |r| r.std), None)
        } else if self.id != 0 {
            let w = 3 * DAY;
            let (lo, hi) = (l - w, l + w);
            let p = match c.as_deref().filter(|p| p.id == self.id && p.holds64(lo) && p.holds64(hi)) {
                Some(&p) => p,
                None => self.kind_period(i128::from(lo)).1.filter(|p| p.holds64(hi))?,
            };
            (self.kind_of(&p), Some(p))
        } else {
            return None;
        };
        if !ok(k) {
            return None;
        }
        let t = l - k.offset + f.second;
        if let Some(p) = p {
            if !p.holds64(t) {
                return None;
            }
            if let Some(c) = c {
                *c = p;
            }
        }
        let civil = civil_from_seconds64(l + f.second);
        if !fits_tm(civil.year) {
            return Some(Err(Errno::EOVERFLOW));
        }
        Some(Ok((t, Local { civil, isdst: k.isdst, offset: k.offset, name: k.name })))
    }

    /// The end of [`Zone::resolve_in`]: the instant `t` (before `f`'s seconds and leap seconds)
    /// and its local time (of kind `k` when the caller knows it holds at every instant; else of
    /// the period `p` of this zone when it holds the instant, which becomes the thread's period
    /// `c`; else through `c` when there is one).
    fn resolved(&self, t: i128, f: &Fields, c: Option<&mut Period>, k: Option<Kind>, p: Option<Period>) -> Result<(i64, Local)> {
        let t = t + i128::from(f.second);
        // Into the file's time scale when it counts leap seconds.
        let mut corr = 0;
        for _ in 0..2 {
            corr = self.leap(t + corr).0;
        }
        let t = i64::try_from(t + corr).map_err(|_| Errno::EOVERFLOW)?;
        let l = match (k, p.filter(|p| p.holds(i128::from(t))), c) {
            (Some(k), _, _) => self.local(t, k)?,
            (None, Some(p), c) => {
                if let Some(c) = c {
                    *c = p;
                }
                self.local(t, self.kind_of(&p))?
            }
            (None, None, Some(c)) => self.at_cached(t, c)?,
            (None, None, None) => self.at(t)?,
        };
        Ok((t, l))
    }

}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions)]
mod tests {
    use super::*;

    fn fields(y: i64, mo: i64, d: i64, h: i64, mi: i64) -> Fields {
        Fields { year: y, month: mo, day: d, hour: h, minute: mi, second: 0 }
    }

    /// A rule zone answers the same with its table of yearly changes as without it, around
    /// every change of every year the table covers and beyond its ends, both directions.
    #[test]
    fn rule_year_table_changes_nothing() {
        for text in [&b"EST5EDT,M3.2.0,M11.1.0"[..], b"<-03>3<-02>,M3.5.0/-2,M10.5.0/-1", b"AAA-10BBB,M10.1.0,M4.1.0/3", b"XXX3YYY,J60/25,300/-167", b"EST5EDT,0/0,J365/25"] {
            let with = Zone::from_rule_text(text).unwrap();
            let mut without = Zone::from_rule_text(text).unwrap();
            assert_eq!(with.rule_years.len(), usize::try_from(RULE_YEARS).unwrap());
            without.rule_years = TryVec::new();
            let r = with.rule.unwrap();
            for year in RULE_FIRST - 3..RULE_FIRST + RULE_YEARS + 3 {
                for (c, _) in r.changes(year, &[]).unwrap() {
                    let c = i64::try_from(c).unwrap();
                    for t in [c - 3601, c - 1, c, c + 1, c + 3600, c + 86_400 * 40] {
                        assert_eq!(with.at(t).unwrap(), without.at(t).unwrap(), "{} at {t}", String::from_utf8_lossy(text));
                    }
                    let l = with.at(c).unwrap().civil;
                    for isdst in [-1, 0, 1] {
                        for dh in [-1i64, 0, 1] {
                            let f = Fields { year: l.year, month: l.month.into(), day: l.day.into(), hour: i64::from(l.hour) + dh, minute: 30, second: 0 };
                            assert_eq!(with.resolve(&f, isdst).ok(), without.resolve(&f, isdst).ok(), "{} resolve {f:?} {isdst}", String::from_utf8_lossy(text));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn rule_zone() {
        let z = Zone::from_rule_text(b"EST5EDT,M3.2.0,M11.1.0").unwrap();
        let g = z.globals();
        assert_eq!((z.name(g.std), z.name(g.dst), g.timezone, g.daylight), (&b"EST"[..], &b"EDT"[..], 18_000, true));
        // 2024: DST from 1710054000 to 1730613600.
        assert!(!z.at(1_710_053_999).unwrap().isdst);
        assert!(z.at(1_710_054_000).unwrap().isdst);
        assert!(z.at(1_730_613_599).unwrap().isdst);
        assert!(!z.at(1_730_613_600).unwrap().isdst);
        let t = 1_710_054_000i64;
        // Gap: 02:30 reads with EST (after the change); with DST asked, with EDT.
        assert_eq!(z.resolve(&fields(2024, 3, 10, 2, 30), -1).unwrap().0, t + 1800);
        assert_eq!(z.resolve(&fields(2024, 3, 10, 2, 30), 1).unwrap().0, t - 1800);
        assert_eq!(z.resolve(&fields(2024, 3, 10, 3, 0), -1).unwrap().0, t);
        // Overlap: the first occurrence unless standard time is asked for.
        let e = 1_730_613_600i64;
        assert_eq!(z.resolve(&fields(2024, 11, 3, 1, 30), -1).unwrap().0, e - 1800);
        assert_eq!(z.resolve(&fields(2024, 11, 3, 1, 30), 0).unwrap().0, e + 1800);
        assert_eq!(z.resolve(&fields(2024, 11, 3, 2, 30), -1).unwrap().0, e + 5400);
        // DST asked where standard time is in force: read with the DST offset.
        assert_eq!(z.resolve(&fields(2024, 1, 1, 12, 0), 1).unwrap().1.civil.hour, 11);
    }

    #[test]
    fn invalid_values() {
        let z = Zone::from_rule_text(b"Invalid//Zone").unwrap();
        let g = z.globals();
        assert_eq!((z.name(g.std), z.name(g.dst), g.timezone), (&b"Invalid"[..], &b""[..], 0));
        assert_eq!(z.name_z(z.at(0).unwrap().name), b"Invalid\0");
        // No DST in the zone: DST asked reads one hour ahead; standard time asked is ordinary.
        let z = Zone::from_rule_text(b"JST-9").unwrap();
        assert_eq!(z.resolve(&fields(2024, 7, 1, 12, 0), 1).unwrap().1.civil.hour, 11);
        assert_eq!(z.resolve(&fields(2024, 7, 1, 12, 0), 0).unwrap().1.civil.hour, 12);
        // The seconds are elapsed time after the rest of the fields.
        let z = Zone::from_rule_text(b"EST5EDT,M3.2.0,M11.1.0").unwrap();
        let f = Fields { second: 3600, ..fields(2024, 3, 10, 2, 0) };
        assert_eq!(z.resolve(&f, -1).unwrap().0, 1_710_057_600);
        assert_eq!(z.resolve(&fields(2024, 3, 10, 3, 0), -1).unwrap().0, 1_710_054_000);
    }

    #[test]
    fn southern_and_year_round() {
        let z = Zone::from_rule_text(b"NZST-12NZDT,M9.5.0,M4.1.0/3").unwrap();
        assert!(z.at(1_704_067_200).unwrap().isdst, "January is summer");
        assert!(!z.at(1_719_792_000).unwrap().isdst, "July is winter");
        let z = Zone::from_rule_text(b"AAA3BBB,J1/0,J365/25").unwrap();
        for t in [1_704_067_200i64, 1_719_792_000, 1_735_689_000] {
            assert!(z.at(t).unwrap().isdst, "year-round DST at {t}");
        }
    }

    fn tzif(types: &[(i32, u8, u8)], trans: &[(i64, u8)], names: &[u8], leaps: &[(i64, i32)], footer: &[u8]) -> TryVec<u8> {
        let mut v = TryVec::new();
        let header = |v: &mut TryVec<u8>, ver: u8, nt: usize| {
            v.extend_from_slice(b"TZif").unwrap();
            v.push(ver).unwrap();
            v.extend_from_slice(&[0; 15]).unwrap();
            for n in [0, 0, leaps.len(), nt, types.len(), names.len()] {
                v.extend_from_slice(&u32::try_from(n).unwrap().to_be_bytes()).unwrap();
            }
        };
        // A minimal version 1 block (one type, one designation byte), then the version 2 data.
        v.extend_from_slice(b"TZif2").unwrap();
        v.extend_from_slice(&[0; 15]).unwrap();
        v.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 1]).unwrap();
        v.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0]).unwrap();
        header(&mut v, b'2', trans.len());
        for (t, _) in trans {
            v.extend_from_slice(&t.to_be_bytes()).unwrap();
        }
        for (_, i) in trans {
            v.push(*i).unwrap();
        }
        for (off, dst, idx) in types {
            v.extend_from_slice(&off.to_be_bytes()).unwrap();
            v.push(*dst).unwrap();
            v.push(*idx).unwrap();
        }
        v.extend_from_slice(names).unwrap();
        for (occ, corr) in leaps {
            v.extend_from_slice(&occ.to_be_bytes()).unwrap();
            v.extend_from_slice(&corr.to_be_bytes()).unwrap();
        }
        v.push(b'\n').unwrap();
        v.extend_from_slice(footer).unwrap();
        v.push(b'\n').unwrap();
        v
    }

    #[test]
    fn files() {
        // LMT until 1000000, then XST (+1) and XDT (+2) alternating, footer after.
        let f = tzif(&[(600, 0, 0), (3600, 0, 4), (7200, 1, 8)], &[(1_000_000, 1), (2_000_000, 2), (3_000_000, 1)], b"LMT\0XST\0XDT\0", &[], b"XST-1");
        let z = Zone::from_tzif(&f).unwrap();
        assert_eq!(z.at(0).unwrap().offset, 600);
        assert_eq!(z.name(z.at(2_500_000).unwrap().name), b"XDT");
        assert_eq!(z.name(z.at(5_000_000).unwrap().name), b"XST");
        let g = z.globals();
        assert_eq!((z.name(g.std), z.name(g.dst), g.timezone, g.daylight), (&b"XST"[..], &b"XDT"[..], -3600, true));
        assert_eq!(Zone::from_tzif(b"TZif2").unwrap_err(), Errno::EINVAL);
        // Leap seconds: the inserted second reads as :60.
        let f = tzif(&[(0, 0, 0)], &[], b"UTC\0", &[(78_796_800, 1), (94_694_401, 2)], b"");
        let z = Zone::from_tzif(&f).unwrap();
        let c = z.at(78_796_800).unwrap().civil;
        assert_eq!((c.month, c.day, c.hour, c.minute, c.second), (6, 30, 23, 59, 60));
        let c = z.at(78_796_801).unwrap().civil;
        assert_eq!((c.month, c.day, c.hour, c.second), (7, 1, 0, 0));
        assert_eq!(z.resolve(&fields(1972, 7, 1, 0, 0), -1).unwrap().0, 78_796_801);
    }

    /// The instants around every change of the years in `years`, of `z`'s rule.
    fn edges(z: &Zone, years: core::ops::Range<i64>) -> Vec<i64> {
        let r = z.rule.unwrap();
        let mut v = Vec::new();
        for year in years {
            for (c, _) in r.changes(year, &[]).unwrap() {
                let c = i64::try_from(c).unwrap();
                for d in [-86_400 * 400, -3601, -1, 0, 1, 3599, 3600, 86_400 * 40] {
                    v.push(c + d);
                }
            }
        }
        v
    }

    /// P7: a thread's [`Period`] changes no answer: kept across calls in any order (the
    /// instants shuffled, years apart, around every change, past the table's ends),
    /// `at_cached` gives what `at` gives.
    #[test]
    fn period_changes_nothing() {
        let file = tzif(&[(600, 0, 0), (3600, 0, 4), (7200, 1, 8)], &[(1_000_000, 1), (2_000_000, 2), (3_000_000, 1)], b"LMT\0XST\0XDT\0", &[], b"XST-1XDT,M3.2.0,M11.1.0");
        let mut zones = vec![Zone::from_tzif(&file).unwrap()];
        for text in [&b"EST5EDT,M3.2.0,M11.1.0"[..], b"<-03>3<-02>,M3.5.0/-2,M10.5.0/-1", b"AAA-10BBB,M10.1.0,M4.1.0/3", b"XXX3YYY,J60/25,300/-167", b"EST5EDT,0/0,J365/25", b"EST5EDT"] {
            zones.push(Zone::from_rule_text(text).unwrap());
        }
        for z in &zones {
            let mut ts = vec![i64::MIN, i64::MIN + 1, -1, 0, 1, 2_999_999, 3_000_000, 3_000_001, 4_000_000, i64::MAX - 1, i64::MAX];
            if z.rule.is_some_and(|r| r.dst.is_some()) {
                ts.extend(edges(z, RULE_FIRST - 3..RULE_FIRST + RULE_YEARS + 3));
            }
            let mut p = Period::default();
            // In order, then in a scrambled order (a fixed stride).
            let n = ts.len();
            let order: Vec<usize> = (0..n).chain((0..n).map(|i| i * 7919 % n)).collect();
            for i in order {
                let t = ts[i];
                for _ in 0..2 {
                    assert_eq!(z.at_cached(t, &mut p).ok(), z.at(t).ok(), "at {t} with {p:?}");
                }
            }
        }
    }

    /// TB-SLOTS: `resolve_cached` (mktime through the thread's period) gives what `resolve`
    /// gives, for local times around every change, every `isdst`, in order and scrambled.
    #[test]
    fn resolve_period_changes_nothing() {
        let file = tzif(&[(600, 0, 0), (3600, 0, 4), (7200, 1, 8)], &[(1_000_000, 1), (2_000_000, 2), (3_000_000, 1)], b"LMT XST XDT ", &[], b"XST-1XDT,M3.2.0,M11.1.0");
        let mut zones = vec![Zone::from_tzif(&file).unwrap()];
        for text in [&b"EST5EDT,M3.2.0,M11.1.0"[..], b"<-03>3<-02>,M3.5.0/-2,M10.5.0/-1", b"AAA-10BBB,M10.1.0,M4.1.0/3", b"XXX3YYY,J60/25,300/-167", b"EST5EDT,0/0,J365/25", b"EST5EDT"] {
            zones.push(Zone::from_rule_text(text).unwrap());
        }
        let mut hits = 0usize;
        for z in &zones {
            let mut ts = vec![-1, 0, 1, 2_999_999, 3_000_000, 3_000_001, 4_000_000, 1_720_000_000];
            if z.rule.is_some_and(|r| r.dst.is_some()) {
                ts.extend(edges(z, 1990..2040));
            }
            let mut fs = Vec::new();
            for &t in &ts {
                let Ok(l) = z.at(t) else { continue };
                for dh in [-1, 0, 1] {
                    for (mi, sec) in [(l.civil.minute, 0), (30, 3600)] {
                        fs.push(Fields { year: l.civil.year, month: l.civil.month.into(), day: l.civil.day.into(), hour: i64::from(l.civil.hour) + dh, minute: mi.into(), second: sec });
                    }
                }
            }
            let mut p = Period::default();
            let n = fs.len();
            let order: Vec<usize> = (0..n).chain((0..n).map(|i| i * 7919 % n)).collect();
            for i in order {
                for isdst in [-1, 0, 1] {
                    let want = z.resolve(&fs[i], isdst).ok();
                    for _ in 0..2 {
                        let before = p;
                        assert_eq!(z.resolve_cached(&fs[i], isdst, &mut p).ok(), want, "{:?} isdst {isdst} with {before:?}", fs[i]);
                        hits += usize::from(before == p && before.id != 0);
                    }
                }
            }
        }
        assert!(hits > 1000, "the period is used: {hits}");
    }

    /// MKT-1: a zone whose kind never changes (UTC, a fixed offset, a file without transitions)
    /// resolves every local time to `fields - offset` (an hour earlier for `isdst > 0`, which
    /// the search answers), with that kind; leap seconds round-trip. A file without transitions
    /// but with a DST type still searches for `isdst > 0`.
    #[test]
    fn resolve_fixed_zone() {
        let plain = [(Zone::from_rule_text(b"UTC0").unwrap(), 0), (Zone::from_rule_text(b"<+0530>-5:30").unwrap(), 19_800), (Zone::from_rule_text(b"EST5").unwrap(), -18_000), (Zone::from_tzif(&tzif(&[(3600, 0, 0)], &[], b"XST ", &[], b"XST-1")).unwrap(), 3600)];
        let leap = Zone::from_tzif(&tzif(&[(0, 0, 0)], &[], b"UTC ", &[(78_796_800, 1), (94_694_401, 2)], b"UTC0")).unwrap();
        let dst_type = Zone::from_tzif(&tzif(&[(0, 0, 0), (3600, 1, 4)], &[], b"XST XDT ", &[], b"")).unwrap();
        let mut fs = Vec::new();
        for year in [-1000, 1900, 1970, 1972, 2024, 2100, 9999] {
            for (month, day, hour, second) in [(0, 1, 0, 0), (2, 29, 25, 3601), (13, 0, -1, -61), (6, 15, 12, 59), (-5, 40, 2, 30)] {
                fs.push(Fields { year, month, day, hour, minute: 7, second });
            }
        }
        for f in &fs {
            for (z, off) in &plain {
                for isdst in [-1, 0, 1] {
                    // DST asked of a zone without it: one hour ahead (the search's answer).
                    let want = Fields { second: 0, ..*f }.seconds() + i128::from(f.second) - i128::from(*off) - if isdst > 0 { 3600 } else { 0 };
                    let (t, l) = z.resolve(f, isdst).unwrap();
                    assert_eq!((i128::from(t), l.offset, l.isdst), (want, *off, false), "{f:?} isdst {isdst} offset {off}");
                    assert_eq!(z.at(t).unwrap(), l);
                }
            }
            for isdst in [-1, 0, 1] {
                let (t, l) = leap.resolve(f, isdst).unwrap();
                assert_eq!(leap.at(t).unwrap(), l);
                if l.civil.second != 60 {
                    assert_eq!(l.civil, civil_from_seconds(f.seconds() - if isdst > 0 { 3600 } else { 0 }).unwrap(), "{f:?} isdst {isdst}");
                }
            }
            let base = dst_type.resolve(f, -1).unwrap();
            assert_eq!(i128::from(base.0), f.seconds(), "{f:?}");
            assert_eq!(dst_type.resolve(f, 1).unwrap().0, base.0 - 3600, "{f:?}");
        }
    }

    /// P7: one [`Period`] used with several zones (a `TZ` change between two calls of a thread,
    /// a zone read again) never carries one zone's answer to another. Red without the zone's
    /// number in the period.
    #[test]
    fn period_is_of_its_zone() {
        let a = Zone::from_rule_text(b"EST5EDT,M3.2.0,M11.1.0").unwrap();
        let b = Zone::from_rule_text(b"EST5EDT,M3.2.0,M11.1.0").unwrap();
        let c = Zone::from_rule_text(b"AAA-10BBB,M10.1.0,M4.1.0/3").unwrap();
        let (summer, winter) = (1_720_000_000, 1_700_000_000);
        let mut p = Period::default();
        for t in [summer, summer + 3600, winter, summer, winter + 1, summer] {
            for z in [&a, &c, &b, &c, &a] {
                assert_eq!(z.at_cached(t, &mut p).unwrap(), z.at(t).unwrap(), "{t}");
            }
        }
        assert_ne!(a.id, b.id);
        assert_ne!(a.id, 0);
        assert_eq!(Zone::fixed(b"U\0", b"U\0").id, 0);
        assert!(a.uses_period(summer) && !Zone::fixed(b"U\0", b"U\0").uses_period(summer));
        assert!(!Zone::from_rule_text(b"EST5").unwrap().uses_period(summer));
    }

    #[test]
    fn period_words() {
        let p = Period { id: 7, lo: -5, hi: i64::MAX, kind: P_RULE_DST };
        assert_eq!(Period::from_words(&p.to_words()), p);
        let q = Period { id: u32::MAX, lo: i64::MIN, hi: -1, kind: P_TYPE | 0xff };
        assert_eq!(Period::from_words(&q.to_words()), q);
        assert_eq!(Period::from_words(&[0; Period::WORDS]), Period::default());
    }

    /// The zones of the MKT-2 sweep: system zoneinfo files (every gate machine has them; a
    /// missing one fails the test), a synthetic file with yearly transitions, one with leap seconds, a file whose
    /// transitions are out of order, POSIX rules.
    fn sweep_zones() -> Vec<(String, Zone)> {
        let mut v = Vec::new();
        for name in ["America/New_York", "Europe/London", "Australia/Lord_Howe", "Asia/Kolkata", "America/Sao_Paulo", "Pacific/Apia", "Europe/Dublin"] {
            let bytes = std::fs::read(format!("/usr/share/zoneinfo/{name}")).unwrap_or_else(|e| panic!("{name}: {e}"));
            v.push((name.to_string(), Zone::from_tzif(&bytes).unwrap()));
        }
        // Yearly DST from 1970 to 2030 (XST +1, XDT +2), then a footer rule.
        let mut trans = Vec::new();
        for y in 0..60i64 {
            trans.push((y * 31_556_952 + 7_000_000, 2u8));
            trans.push((y * 31_556_952 + 25_000_000, 1u8));
        }
        let f = tzif(&[(600, 0, 0), (3600, 0, 4), (7200, 1, 8)], &trans, b"LMT XST XDT ", &[], b"XST-1XDT,M3.5.0,M10.5.0/3");
        v.push(("yearly".into(), Zone::from_tzif(&f).unwrap()));
        // Transitions out of order (the parser refuses such a file; built by hand).
        let f = tzif(&[(600, 0, 0), (3600, 0, 4), (7200, 1, 8)], &[(1_000_000, 2), (2_000_000, 1), (3_000_000, 1)], b"LMT XST XDT ", &[], b"");
        let mut z = Zone::from_tzif(&f).unwrap();
        z.trans = TryVec::new();
        for t in [(3_000_000, 1), (1_000_000, 2), (2_000_000, 1)] {
            z.trans.push(t).unwrap();
        }
        z.sorted = false;
        z.number();
        v.push(("unsorted".into(), z));
        let f = tzif(&[(3600, 0, 0), (7200, 1, 4)], &[(1_000_000, 1), (2_000_000, 0)], b"XST XDT ", &[], b"");
        v.push(("no footer".into(), Zone::from_tzif(&f).unwrap()));
        // Leap seconds with transitions (Ubuntu 24.04 ships no right/ zones).
        let f = tzif(&[(3600, 0, 0), (7200, 1, 4)], &[(70_000_000, 1), (80_000_000, 0), (90_000_000, 1), (100_000_000, 0)], b"XST XDT ", &[(78_796_800, 1), (94_694_401, 2)], b"XST-1XDT,M3.5.0,M10.5.0/3");
        v.push(("leaps".into(), Zone::from_tzif(&f).unwrap()));
        for text in [&b"EST5EDT,M3.2.0,M11.1.0"[..], b"<-03>3<-02>,M3.5.0/-2,M10.5.0/-1", b"AAA-10BBB,M10.1.0,M4.1.0/3", b"XXX3YYY,J60/25,300/-167", b"EST5EDT,0/0,J365/25"] {
            v.push((String::from_utf8_lossy(text).into_owned(), Zone::from_rule_text(text).unwrap()));
        }
        v
    }

    /// The instants of the MKT-2 sweep of `z`: every transition and every rule change from 1900
    /// to 2100, each -1 s, exact and +1 s (and an hour either side), and far past and future.
    fn sweep_instants(z: &Zone, years: core::ops::Range<i64>, offsets: &[i64]) -> Vec<i64> {
        let mut ts = vec![i64::MIN, i64::MIN + 1, -67_768_040_609_740_801, -1_000_000_000_000, -2_208_988_800, -1, 0, 1, 1_720_000_000, 4_102_444_800, 32_503_680_000, 67_767_976_233_316_800, i64::MAX - 1, i64::MAX];
        let mut edges: Vec<i64> = z.trans.iter().map(|t| t.0).collect();
        if let Some(r) = z.rule.filter(|r| r.dst.is_some()) {
            for year in years {
                for (c, _) in r.changes(year, &[]).unwrap() {
                    edges.push(i64::try_from(c).unwrap());
                }
            }
        }
        for c in edges {
            for &d in offsets {
                ts.push(c.saturating_add(d));
            }
        }
        ts
    }

    /// MKT-2: `at_cached` with a period carried from call to call (in order, then scrambled)
    /// answers what `at` answers, around every transition and rule change of the sweep zones.
    #[test]
    fn transition_period_changes_nothing() {
        let mut hits = 0usize;
        for (name, z) in sweep_zones() {
            let ts = sweep_instants(&z, 1900..2100, &[-3601, -1, 0, 1, 3600]);
            let n = ts.len();
            let order: Vec<usize> = (0..n).chain((0..n).map(|i| i * 7919 % n)).collect();
            let mut p = Period::default();
            for i in order {
                let t = ts[i];
                for _ in 0..2 {
                    let before = p;
                    assert_eq!(z.at_cached(t, &mut p).ok(), z.at(t).ok(), "{name} at {t} with {before:?}");
                    hits += usize::from(before == p && p.id != 0);
                }
            }
            assert_eq!(z.uses_period(0), z.id != 0, "{name}");
            assert_eq!(z.sorted, name != "unsorted", "{name}");
            assert_eq!(z.id == 0, !z.sorted || (z.trans.is_empty() && z.rule.is_none_or(|r| r.dst.is_none())), "{name}");
        }
        assert!(hits > 10_000, "the period is used: {hits}");
    }

    /// MKT-2: `resolve` and `resolve_cached` (with the window shortcuts) answer what the full
    /// search answers, for local times around every transition and rule change of the sweep
    /// zones (gaps and overlaps), every `tm_isdst`, fields out of range, in order and scrambled.
    #[test]
    fn resolve_shortcuts_change_nothing() {
        let mut kept = 0usize;
        for (name, z) in sweep_zones() {
            let mut fs = Vec::new();
            for t in sweep_instants(&z, 1960..2045, &[-1, 0, 1]) {
                let Ok(l) = z.at(t) else { continue };
                let c = l.civil;
                let (y, mo, d, h, mi) = (c.year, i64::from(c.month), i64::from(c.day), i64::from(c.hour), i64::from(c.minute));
                for dh in [-1, 0, 1] {
                    for (mi, sec) in [(mi, 0), (30, 0), (0, 3600)] {
                        fs.push(Fields { year: y, month: mo, day: d, hour: h + dh, minute: mi, second: sec });
                    }
                }
                // Out-of-range fields that normalise to the same neighbourhood.
                fs.push(Fields { year: y - 1, month: mo + 12, day: d, hour: h, minute: mi + 60, second: -61 });
                fs.push(Fields { year: y, month: mo, day: d + 1, hour: h - 24, minute: mi, second: 0 });
            }
            // The benchmark's shape: the middle of every month at noon, 1900 to 2100.
            for y in 1900..2100 {
                for m in 1..=12 {
                    fs.push(Fields { year: y, month: m, day: 15, hour: 12, minute: 0, second: 0 });
                }
            }
            // The edges of the 64-bit shortcut (TZ-FRESH3): fields just inside and outside 2^31.
            for (y, m, s) in [(2_147_483_000, 1, 0), (2_147_483_647, 12, 0), (2_147_483_648, 1, 0), (-2_147_483_647, 1, 0), (2024, 1, 2_147_483_647), (2024, 1, -2_147_483_647), (2024, 2_147_483_647, 0), (2024, 1, 2_147_483_648)] {
                fs.push(Fields { year: y, month: m, day: 1, hour: 0, minute: 0, second: s });
            }
            for (y, m) in [(i64::MIN / 2, 1), (-1000, 13), (9999, 0), (292_277_026_596, 12), (i64::MAX / 2, 1)] {
                fs.push(Fields { year: y, month: m, day: 1, hour: 0, minute: 0, second: 0 });
            }
            let n = fs.len();
            let order: Vec<usize> = (0..n).chain((0..n).map(|i| i * 7919 % n)).collect();
            let mut p = Period::default();
            for i in order {
                let f = &fs[i];
                for isdst in [-1, 0, 1] {
                    let want = z.resolve_by(f, isdst, None, false);
                    assert_eq!(z.resolve(f, isdst), want, "{name} resolve {f:?} isdst {isdst}");
                    let before = p;
                    assert_eq!(z.resolve_cached(f, isdst, &mut p), want, "{name} resolve_cached {f:?} isdst {isdst} with {before:?}");
                    kept += usize::from(before == p && p.id != 0);
                }
            }
        }
        assert!(kept > 10_000, "the period is used: {kept}");
    }
}
