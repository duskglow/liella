//! The TZif format, versions 1 to 4 (RFC 8536 and its version 4 update): header, transition
//! times and types, local time type records, designations, leap-second records, and the
//! version 2+ footer (spec §23.6, D-23.6). Every read is bounds-checked; a file whose counts do
//! not fit its length, whose indices are out of range, or that has no local time type is
//! rejected (`EINVAL`). The standard/wall and UT/local indicators only matter to a POSIX-rule
//! fallback reader, which this one is not, and are skipped.

use crate::error::{Errno, Result};
use crate::mem::TryVec;

/// A local time type record.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct TypeRecord {
    /// Seconds east of UTC.
    pub utoff: i64,
    /// Daylight-saving time.
    pub isdst: bool,
    /// The designation, an index into [`Tzif::designations`].
    pub idx: usize,
}

/// A parsed TZif file (the version 2+ data when present).
#[derive(Debug, Default)]
pub struct Tzif<'a> {
    /// The version: 1, 2, 3 or 4.
    pub version: u8,
    /// Transition times and the index of the type each selects.
    pub transitions: TryVec<(i64, u8)>,
    /// The local time types (at least one).
    pub types: TryVec<TypeRecord>,
    /// The designation bytes (NUL-separated).
    pub designations: &'a [u8],
    /// Leap-second records: occurrence and the correction from then on.
    pub leaps: TryVec<(i64, i64)>,
    /// The footer's TZ string (empty when absent).
    pub footer: &'a [u8],
}

impl<'a> Tzif<'a> {
    /// The designation of `t` (up to its NUL).
    pub fn designation(&self, t: &TypeRecord) -> &'a [u8] {
        let rest = self.designations.get(t.idx..).unwrap_or(&[]);
        rest.split(|&b| b == 0).next().unwrap_or(&[])
    }
}

struct Reader<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.at.checked_add(n).ok_or(Errno::EINVAL)?;
        let s = self.b.get(self.at..end).ok_or(Errno::EINVAL)?;
        self.at = end;
        Ok(s)
    }

    fn u32(&mut self) -> Result<u32> {
        let s = self.take(4)?;
        Ok(u32::from_be_bytes(s.try_into().map_err(|_| Errno::EINVAL)?))
    }

    fn i32(&mut self) -> Result<i64> {
        let s = self.take(4)?;
        Ok(i64::from(i32::from_be_bytes(s.try_into().map_err(|_| Errno::EINVAL)?)))
    }

    fn i64(&mut self) -> Result<i64> {
        let s = self.take(8)?;
        Ok(i64::from_be_bytes(s.try_into().map_err(|_| Errno::EINVAL)?))
    }

    fn time(&mut self, wide: bool) -> Result<i64> {
        if wide { self.i64() } else { self.i32() }
    }

    fn byte(&mut self) -> Result<u8> {
        Ok(*self.take(1)?.first().ok_or(Errno::EINVAL)?)
    }
}

/// The counts of a header: isutcnt, isstdcnt, leapcnt, timecnt, typecnt, charcnt.
struct Counts {
    isut: usize,
    isstd: usize,
    leap: usize,
    time: usize,
    types: usize,
    chars: usize,
}

fn header(r: &mut Reader<'_>) -> Result<(u8, Counts)> {
    if r.take(4)? != b"TZif" {
        return Err(Errno::EINVAL);
    }
    let version = match r.byte()? {
        0 => 1,
        v @ b'2'..=b'9' => v.saturating_sub(b'0'),
        _ => return Err(Errno::EINVAL),
    };
    r.take(15)?;
    let mut n = [0usize; 6];
    for slot in &mut n {
        *slot = usize::try_from(r.u32()?).map_err(|_| Errno::EINVAL)?;
    }
    let [isut, isstd, leap, time, types, chars] = n;
    if types == 0 || (isut != 0 && isut != types) || (isstd != 0 && isstd != types) {
        return Err(Errno::EINVAL);
    }
    Ok((version, Counts { isut, isstd, leap, time, types, chars }))
}

/// Bytes of a data block with these counts (`wide`: 64-bit times).
fn block_len(c: &Counts, wide: bool) -> Option<usize> {
    let t = if wide { 8 } else { 4 };
    let parts = [
        c.time.checked_mul(t)?,
        c.time,
        c.types.checked_mul(6)?,
        c.chars,
        c.leap.checked_mul(t.checked_add(4)?)?,
        c.isstd,
        c.isut,
    ];
    parts.iter().try_fold(0usize, |a, &p| a.checked_add(p))
}

fn block<'a>(r: &mut Reader<'a>, c: &Counts, wide: bool, version: u8) -> Result<Tzif<'a>> {
    if block_len(c, wide).is_none_or(|n| r.at.checked_add(n).is_none_or(|e| e > r.b.len())) {
        return Err(Errno::EINVAL);
    }
    let mut out = Tzif { version, ..Tzif::default() };
    out.transitions.reserve(c.time)?;
    let mut times = TryVec::with_capacity(c.time)?;
    for _ in 0..c.time {
        times.push(r.time(wide)?)?;
    }
    for &t in times.iter() {
        let i = r.byte()?;
        if usize::from(i) >= c.types {
            return Err(Errno::EINVAL);
        }
        out.transitions.push((t, i))?;
    }
    if out.transitions.windows(2).any(|w| w.first().map(|x| x.0) >= w.get(1).map(|x| x.0)) {
        return Err(Errno::EINVAL);
    }
    out.types.reserve(c.types)?;
    for _ in 0..c.types {
        let utoff = r.i32()?;
        let isdst = r.byte()? != 0;
        let idx = usize::from(r.byte()?);
        if utoff == i64::from(i32::MIN) || (idx >= c.chars && c.chars != 0) {
            return Err(Errno::EINVAL);
        }
        out.types.push(TypeRecord { utoff, isdst, idx })?;
    }
    out.designations = r.take(c.chars)?;
    out.leaps.reserve(c.leap)?;
    for _ in 0..c.leap {
        let occ = r.time(wide)?;
        let corr = r.i32()?;
        out.leaps.push((occ, corr))?;
    }
    r.take(c.isstd)?;
    r.take(c.isut)?;
    Ok(out)
}

/// Parse a TZif file.
pub fn parse(bytes: &[u8]) -> Result<Tzif<'_>> {
    let mut r = Reader { b: bytes, at: 0 };
    let (version, c1) = header(&mut r)?;
    if version == 1 {
        return block(&mut r, &c1, false, 1);
    }
    let skip = block_len(&c1, false).ok_or(Errno::EINVAL)?;
    r.take(skip)?;
    let (_, c2) = header(&mut r)?;
    let mut out = block(&mut r, &c2, true, version)?;
    // The footer: a newline, the TZ string, a newline (absent in a truncated file).
    if r.take(1).ok() == Some(b"\n") {
        let rest = r.b.get(r.at..).unwrap_or(&[]);
        if let Some(n) = rest.iter().position(|&b| b == b'\n') {
            out.footer = rest.get(..n).unwrap_or(&[]);
        }
    }
    Ok(out)
}
