//! The `strftime` fast lane (STRFTIME-FAST; D-23.7): byte output of literals and the plain
//! conversions, straight from the caller's `struct tm`, without the general engine's copy of the
//! time, its `Ctx`/`Sink`/`Process` set-up and its per-directive `Result` plumbing.
//!
//! **Contract.** [`strftime`] returns `Some(n)` only when the general engine would return
//! `Written::Fits(n)` with the same bytes in `out[..=n]`, and then it has written nothing else.
//! It returns `None` for everything it does not handle: a flag, width or modifier, a conversion
//! outside [`directive`]'s list, a trailing `%`, a number outside `0..=9999`, a zone name that
//! must come from the process zone, an `LC_TIME` composite that is too long for its scratch space,
//! the nesting limit, and any text that does not fit. The caller then runs the general engine
//! from the start: it rewrites the bytes the lane stored, in the same places and with the same
//! values (the lane only ever stores a whole field, in order, where the engine stores it), so
//! the buffer ends as if the lane had not run. Composites (`%c`, `%F`, ...) are built apart and
//! stored as one field, as the engine's all-or-nothing composites are.
//!
//! The lane shares the engine's number and field code ([`Small`]); the tests compare the two
//! over every directive, flag, width, locale, odd time and buffer length.

#![allow(clippy::arithmetic_side_effects)] // struct tm fields are i32 widened to i64 (no overflow); lengths are checked

use super::format::{Small, Tm, Unit, DEPTH, PAIRS};
use super::lctime::{self, Time};
use crate::locale::view::{LocaleView, TimeNames};

/// The fields of a `struct tm` the lane reads (the boundary implements it on the C structure,
/// so that nothing is copied).
pub trait TmRead {
    /// `tm_sec`.
    fn sec(&self) -> i32;
    /// `tm_min`.
    fn min(&self) -> i32;
    /// `tm_hour`.
    fn hour(&self) -> i32;
    /// `tm_mday`.
    fn mday(&self) -> i32;
    /// `tm_mon`.
    fn mon(&self) -> i32;
    /// `tm_year`.
    fn year(&self) -> i32;
    /// `tm_wday`.
    fn wday(&self) -> i32;
    /// `tm_yday`.
    fn yday(&self) -> i32;
    /// `tm_isdst`.
    fn isdst(&self) -> i32;
    /// `tm_gmtoff`.
    fn gmtoff(&self) -> i64;
    /// `tm_zone`'s text (`None`: NULL); read only for `%Z`.
    fn zone(&self) -> Option<&[u8]>;
}

impl TmRead for Tm<'_> {
    fn sec(&self) -> i32 {
        self.sec
    }
    fn min(&self) -> i32 {
        self.min
    }
    fn hour(&self) -> i32 {
        self.hour
    }
    fn mday(&self) -> i32 {
        self.mday
    }
    fn mon(&self) -> i32 {
        self.mon
    }
    fn year(&self) -> i32 {
        self.year
    }
    fn wday(&self) -> i32 {
        self.wday
    }
    fn yday(&self) -> i32 {
        self.yday
    }
    fn isdst(&self) -> i32 {
        self.isdst
    }
    fn gmtoff(&self) -> i64 {
        self.gmtoff
    }
    fn zone(&self) -> Option<&[u8]> {
        self.zone
    }
}

/// The room for an `LC_TIME` composite (`%c`, `%x`, `%X`, `%r`) before it is stored as a field;
/// longer ones take the general engine.
const SCRATCH: usize = 96;

/// Where the text goes: `buf` from `pos`, keeping `reserve` units free after every field (1 for
/// the caller's buffer: the terminator; 0 for a composite's scratch space).
struct Out<'b> {
    buf: &'b mut [u8],
    pos: usize,
    reserve: usize,
}

impl Out<'_> {
    /// Store `s` as one field; `None` (nothing stored) when it does not fit.
    #[inline]
    fn put(&mut self, s: &[u8]) -> Option<()> {
        let end = self.pos.checked_add(s.len())?;
        if end.checked_add(self.reserve)? > self.buf.len() {
            return None;
        }
        u8::copy_into(self.buf.get_mut(self.pos..end)?, s);
        self.pos = end;
        Some(())
    }

    /// [`Out::put`] of a field whose length is a constant: `N` bytes stored in place (no length
    /// dispatch), with the same fit check.
    #[inline(always)]
    fn put_n<const N: usize>(&mut self, s: [u8; N]) -> Option<()> {
        let end = self.pos.checked_add(N)?;
        if end.checked_add(self.reserve)? > self.buf.len() {
            return None;
        }
        self.buf.get_mut(self.pos..end)?.copy_from_slice(&s);
        self.pos = end;
        Some(())
    }
}

/// A numeric field of a fixed length, as [`Lane::digits`] finds it.
enum Field {
    Two([u8; 2]),
    Four([u8; 4]),
}

/// The two units `Small::num(v, 2, fill)` appends for `v` in `0..=99`, else `None`.
#[inline(always)]
fn two(v: i32, fill: u8) -> Option<[u8; 2]> {
    let [a, b] = *PAIRS.get(usize::try_from(v).ok()?)?;
    Some([if v < 10 { fill } else { a }, b])
}

struct Lane<'a, T> {
    tm: &'a T,
    time: Time<'a>,
    names: &'a TimeNames,
}

/// Format `fmt` by `tm` into `out` as the general engine does, or `None` (see the module
/// documentation). `Some(n)`: `n` bytes and a terminator are stored.
#[inline]
pub fn strftime<T: TmRead>(out: &mut [u8], fmt: &[u8], tm: &T, view: &LocaleView) -> Option<usize> {
    let lane = Lane { tm, time: Time::of(view.locale()), names: view.names() };
    let mut w = Out { buf: out, pos: 0, reserve: 1 };
    lane.run(&mut w, fmt, 0)?;
    let n = w.pos;
    *w.buf.get_mut(n)? = 0;
    Some(n)
}

impl<T: TmRead> Lane<'_, T> {
    fn run(&self, w: &mut Out<'_>, fmt: &[u8], depth: u8) -> Option<()> {
        let mut rest = fmt;
        while !rest.is_empty() {
            let n = rest.iter().position(|&b| b == b'%').unwrap_or(rest.len());
            if n == 1 {
                // A one-byte literal (a date or time separator) as a one-byte field.
                w.put_n([*rest.first()?])?;
                rest = rest.get(1..)?;
                continue;
            }
            if n > 0 {
                w.put(rest.get(..n)?)?;
                rest = rest.get(n..)?;
                continue;
            }
            // Only a conversion right after the `%`: no flag, width or modifier.
            let c = *rest.get(1)?;
            match self.digits(c) {
                Some(Field::Two(f)) => w.put_n(f)?,
                Some(Field::Four(f)) => w.put_n(f)?,
                None => self.directive(w, c, depth)?,
            }
            rest = rest.get(2..)?;
        }
        Some(())
    }

    /// The common numeric conversions in their usual range as a fixed-length field, written as
    /// [`Lane::directive`] writes them (`Small::num` with the same width and fill); `None` for
    /// every other conversion or value (then `directive` decides).
    #[inline(always)]
    fn digits(&self, c: u8) -> Option<Field> {
        let tm = self.tm;
        let f = match c {
            b'Y' => {
                let y = tm.year().checked_add(1900)?;
                if !(1000..=9999).contains(&y) {
                    return None;
                }
                let ([a, b], [c, d]) = (*PAIRS.get(usize::try_from(y / 100).ok()?)?, *PAIRS.get(usize::try_from(y % 100).ok()?)?);
                return Some(Field::Four([a, b, c, d]));
            }
            b'm' => two(tm.mon().checked_add(1)?, b'0'),
            b'd' => two(tm.mday(), b'0'),
            b'e' => two(tm.mday(), b' '),
            b'H' => two(tm.hour(), b'0'),
            b'k' => two(tm.hour(), b' '),
            b'M' => two(tm.min(), b'0'),
            b'S' => two(tm.sec(), b'0'),
            _ => None,
        };
        f.map(Field::Two)
    }

    /// An `LC_TIME` composite as one field (the general engine builds it apart as well).
    fn composite(&self, w: &mut Out<'_>, fmt: &[u8], depth: u8) -> Option<()> {
        // Past the nesting limit the engine's composite is empty: its path.
        if depth >= DEPTH {
            return None;
        }
        if w.reserve == 0 {
            // Already apart: the outermost composite is the field.
            return self.run(w, fmt, depth + 1);
        }
        let mut scratch = [0u8; SCRATCH];
        let mut apart = Out { buf: &mut scratch, pos: 0, reserve: 0 };
        self.run(&mut apart, fmt, depth + 1)?;
        let n = apart.pos;
        w.put(scratch.get(..n)?)
    }

    /// Conversion `c` as `format::direct` writes it (the same numbers and names). (Tried: the
    /// names matched before the numbers, or the numbers in a function of their own: the names
    /// gained 10 instructions and `%Y-%m-%dT%H:%M:%S%z` lost 70 to 350.)
    #[inline]
    fn directive(&self, w: &mut Out<'_>, c: u8, depth: u8) -> Option<()> {
        let tm = self.tm;
        let year = i64::from(tm.year()) + 1900;
        let mut f = Small::<u8>::new();
        let (wday, yday, hour) = (i64::from(tm.wday()), i64::from(tm.yday()), i64::from(tm.hour()));
        let hour12 = || match hour.rem_euclid(12) {
            0 => 12,
            h => h,
        };
        let (mday, mon, min, sec) = (i64::from(tm.mday()), i64::from(tm.mon()) + 1, i64::from(tm.min()), i64::from(tm.sec()));
        // The composites built from numbers (`%T` ...) are not available past the nesting limit.
        let nested = depth < DEPTH;
        let done = match c {
            b'Y' => f.num(year, 1, b'0'),
            b'C' => f.num(year.div_euclid(100), 2, b'0'),
            b'y' => f.num(year.rem_euclid(100), 2, b'0'),
            b'm' => f.num(mon, 2, b'0'),
            b'd' => f.num(mday, 2, b'0'),
            b'e' => f.num(mday, 2, b' '),
            b'H' => f.num(hour, 2, b'0'),
            b'k' => f.num(hour, 2, b' '),
            b'I' => f.num(hour12(), 2, b'0'),
            b'l' => f.num(hour12(), 2, b' '),
            b'M' => f.num(min, 2, b'0'),
            b'S' => f.num(sec, 2, b'0'),
            b'j' => f.num(yday + 1, 3, b'0'),
            b'u' => f.num((wday - 1).rem_euclid(7) + 1, 1, b'0'),
            b'w' => f.num(wday, 1, b'0'),
            b'U' => f.num((yday + 7 - wday.rem_euclid(7)).div_euclid(7), 2, b'0'),
            b'W' => f.num((yday + 7 - (wday + 6).rem_euclid(7)).div_euclid(7), 2, b'0'),
            b'T' => nested && f.num(hour, 2, b'0') && f.ch(b':') && f.num(min, 2, b'0') && f.ch(b':') && f.num(sec, 2, b'0'),
            b'R' => nested && f.num(hour, 2, b'0') && f.ch(b':') && f.num(min, 2, b'0'),
            b'D' => nested && f.num(mon, 2, b'0') && f.ch(b'/') && f.num(mday, 2, b'0') && f.ch(b'/') && f.num(year.rem_euclid(100), 2, b'0'),
            b'F' => nested && f.num(year, 1, b'0') && f.ch(b'-') && f.num(mon, 2, b'0') && f.ch(b'-') && f.num(mday, 2, b'0'),
            b'z' if tm.isdst() < 0 => true,
            b'z' => {
                let gmtoff = tm.gmtoff();
                let a = gmtoff.unsigned_abs();
                let (hh, mm) = (a / 3600, a / 60 % 60);
                hh < 100
                    && f.ch(if gmtoff < 0 { b'-' } else { b'+' })
                    && f.num(i64::try_from(hh).unwrap_or(-1), 2, b'0')
                    && f.num(i64::try_from(mm).unwrap_or(-1), 2, b'0')
            }
            b'n' => f.ch(b'\n'),
            b't' => f.ch(b'\t'),
            b'%' => f.ch(b'%'),
            b'p' => return w.put(self.names.ampm(hour >= 12)),
            b'a' => return w.put(self.names.abday(wday).unwrap_or(b"?")),
            b'A' => return w.put(self.names.day(wday).unwrap_or(b"?")),
            b'b' | b'h' => return w.put(self.names.abmon(i64::from(tm.mon())).unwrap_or(b"?")),
            b'B' => return w.put(self.names.mon(i64::from(tm.mon())).unwrap_or(b"?")),
            // A zone name of the caller's `struct tm`; the process zone's is the engine's.
            b'Z' => return w.put(tm.zone().filter(|z| !z.is_empty())?),
            b'c' => return self.composite(w, self.time.format(lctime::D_T_FMT, None), depth),
            b'x' => return self.composite(w, self.time.format(lctime::D_FMT, None), depth),
            b'X' => return self.composite(w, self.time.format(lctime::T_FMT, None), depth),
            b'r' => return self.composite(w, self.time.format(lctime::T_FMT_AMPM, None), depth),
            _ => false,
        };
        if done {
            w.put(f.units())
        } else {
            None
        }
    }
}
