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
    /// The local time type records as stored (six bytes each, at least one), already checked:
    /// read them with [`Tzif::types`]. Kept in the file's bytes rather than copied into a table
    /// of their own, so that loading a zone makes one allocation fewer (PERF-ZONE-LOAD-FAULTS).
    pub type_records: &'a [u8],
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

    /// The local time types, in file order.
    pub fn types(&self) -> impl Iterator<Item = TypeRecord> + 'a {
        self.type_records.chunks_exact(6).map(type_record)
    }

    /// The number of local time types.
    pub fn type_count(&self) -> usize {
        self.type_records.chunks_exact(6).len()
    }
}

/// The record in `b` (six bytes: a big-endian `i32` offset, the DST flag, the designation
/// index); zero where `b` is short, which [`block`] never passes.
fn type_record(b: &[u8]) -> TypeRecord {
    let utoff = b.get(..4).and_then(|s| s.try_into().ok()).map_or(0, |w| i64::from(i32::from_be_bytes(w)));
    let isdst = b.get(4).is_some_and(|&v| v != 0);
    let idx = b.get(5).map_or(0, |&v| usize::from(v));
    TypeRecord { utoff, isdst, idx }
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
    // The times and their type indices in one pass over the two arrays, straight into the
    // result (the first use of a zone parses every transition of its file), checking each
    // index and that the times strictly increase.
    let width = if wide { 8 } else { 4 };
    let times = r.take(c.time.checked_mul(width).ok_or(Errno::EINVAL)?)?;
    let indices = r.take(c.time)?;
    out.transitions.resize(c.time, (0, 0))?;
    let mut prev = i64::MIN;
    for (n, ((slot, raw), &i)) in out.transitions.as_mut_slice().iter_mut().zip(times.chunks_exact(width)).zip(indices).enumerate() {
        let t = match (raw.try_into(), raw.try_into()) {
            (Ok(w), _) if wide => i64::from_be_bytes(w),
            (_, Ok(w)) if !wide => i64::from(i32::from_be_bytes(w)),
            _ => return Err(Errno::EINVAL),
        };
        if usize::from(i) >= c.types || (n > 0 && t <= prev) {
            return Err(Errno::EINVAL);
        }
        prev = t;
        *slot = (t, i);
    }
    out.type_records = r.take(c.types.checked_mul(6).ok_or(Errno::EINVAL)?)?;
    for t in out.types() {
        if t.utoff == i64::from(i32::MIN) || (t.idx >= c.chars && c.chars != 0) {
            return Err(Errno::EINVAL);
        }
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
