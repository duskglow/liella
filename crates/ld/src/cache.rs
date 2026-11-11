//! The loader cache reader (plan/ph14/design.md §4, phase 14 DL-8, D-27.16; format: spec Appendix
//! D.3, DL-0c).
//!
//! Only the new format (`glibc-ld.so.cache1.1`, D.3.1) is read (owner ruling #941): an old or
//! compat file, like any file this reader cannot make sense of, is "no cache". The file is
//! root-owned configuration but untrusted input (threat model rule 5): every offset, count and
//! string end is checked before use, and any inconsistency means "no cache" (the caller falls back
//! to the default directories), never an out-of-bounds read or a panic. Work is bounded: a lookup
//! is a binary search plus one run of equal keys.

use core::cmp::Ordering;

/// The magic and version at offset 0 (no NUL).
pub const MAGIC: &[u8; 20] = b"glibc-ld.so.cache1.1";
/// The header size; the entries follow it.
pub const HEADER: usize = 48;
/// The size of one entry.
pub const ENTRY: usize = 24;
/// The extension directory's magic.
pub const EXT_MAGIC: u32 = 0xeaa4_2174;
/// The extension tag of the hwcaps-name array.
pub const TAG_HWCAPS: u32 = 1;
/// Bit 62 of an entry's `hwcap`: its low 32 bits index the hwcaps-name array.
pub const HWCAP_INDEX: u64 = 1 << 62;
/// Extension records looked at (bounded work on a hostile file).
pub const EXT_MAX: usize = 64;

/// A parsed cache: the file and the checked positions of its parts.
#[derive(Clone, Copy, Debug)]
pub struct Cache<'a> {
    file: &'a [u8],
    nlibs: usize,
    /// The string table, `[start, end)` in the file.
    strings: (usize, usize),
    /// The hwcaps-name array: file offset of its first u32, and the number of names.
    hwcaps: (usize, usize),
}

fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    let w = b.get(at..at.checked_add(4)?)?;
    Some(u32::from_le_bytes(w.try_into().ok()?))
}

fn u64_at(b: &[u8], at: usize) -> Option<u64> {
    let w = b.get(at..at.checked_add(8)?)?;
    Some(u64::from_le_bytes(w.try_into().ok()?))
}

fn usize_at(b: &[u8], at: usize) -> Option<usize> {
    usize::try_from(u32_at(b, at)?).ok()
}

/// The digit run of `s` at `k` without its leading zeros, and the index after the run.
fn digits(s: &[u8], mut k: usize) -> (&[u8], usize) {
    while s.get(k) == Some(&b'0') {
        k = k.saturating_add(1);
    }
    let start = k;
    while s.get(k).is_some_and(u8::is_ascii_digit) {
        k = k.saturating_add(1);
    }
    (s.get(start..k).unwrap_or(&[]), k)
}

/// The comparison the cache is sorted by and the loader matches keys with (Appendix D.3.3): a run
/// of digits compares by numeric value (leading zeros ignored) and ranks above any other byte, other
/// bytes compare by value, and a key that is a prefix of another is smaller.
pub fn compare(a: &[u8], b: &[u8]) -> Ordering {
    let (mut i, mut j) = (0usize, 0usize);
    loop {
        let (x, y) = match (a.get(i), b.get(j)) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(&x), Some(&y)) => (x, y),
        };
        match (x.is_ascii_digit(), y.is_ascii_digit()) {
            (true, true) => {
                let ((da, ni), (db, nj)) = (digits(a, i), digits(b, j));
                let by_value = da.len().cmp(&db.len()).then_with(|| da.cmp(db));
                if by_value != Ordering::Equal {
                    return by_value;
                }
                (i, j) = (ni, nj);
            }
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            (false, false) if x != y => return x.cmp(&y),
            (false, false) => (i, j) = (i.saturating_add(1), j.saturating_add(1)),
        }
    }
}

impl<'a> Cache<'a> {
    /// Parse `file`; `None` when it is not a well-formed new-format cache (the caller then searches
    /// as if there were no cache).
    pub fn parse(file: &'a [u8]) -> Option<Self> {
        if file.get(..MAGIC.len())? != MAGIC {
            return None;
        }
        let nlibs = usize_at(file, 20)?;
        let len_strings = usize_at(file, 24)?;
        let entries_end = HEADER.checked_add(nlibs.checked_mul(ENTRY)?)?;
        let strings_end = entries_end.checked_add(len_strings)?;
        if strings_end > file.len() {
            return None;
        }
        let mut cache = Cache { file, nlibs, strings: (entries_end, strings_end), hwcaps: (0, 0) };
        let ext = usize_at(file, 32)?;
        if ext != 0 {
            cache.hwcaps = cache.extensions(ext)?;
        }
        Some(cache)
    }

    /// The hwcaps-name array from the extension directory at `ext` (`(0, 0)` when it has none);
    /// `None` when the directory is inconsistent.
    fn extensions(&self, ext: usize) -> Option<(usize, usize)> {
        if u32_at(self.file, ext)? != EXT_MAGIC {
            return None;
        }
        let count = usize_at(self.file, ext.checked_add(4)?)?;
        let records = ext.checked_add(8)?;
        if records.checked_add(count.checked_mul(16)?)? > self.file.len() {
            return None;
        }
        for i in 0..count.min(EXT_MAX) {
            let rec = records.checked_add(i.checked_mul(16)?)?;
            if u32_at(self.file, rec)? != TAG_HWCAPS {
                continue;
            }
            let offset = usize_at(self.file, rec.checked_add(8)?)?;
            let size = usize_at(self.file, rec.checked_add(12)?)?;
            if size % 4 != 0 || offset.checked_add(size)? > self.file.len() {
                return None;
            }
            return Some((offset, size / 4));
        }
        Some((0, 0))
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.nlibs
    }

    /// No entries.
    pub fn is_empty(&self) -> bool {
        self.nlibs == 0
    }

    /// The NUL-terminated string at offset `off`, which must lie in the string table and end there.
    fn string(&self, off: usize) -> Option<&'a [u8]> {
        if off < self.strings.0 {
            return None;
        }
        let rest = self.file.get(off..self.strings.1)?;
        let end = rest.iter().position(|&b| b == 0)?;
        rest.get(..end)
    }

    /// Entry `i`: flags, key, value and hwcap word.
    fn entry(&self, i: usize) -> Option<(u32, &'a [u8], &'a [u8], u64)> {
        let at = HEADER.checked_add(i.checked_mul(ENTRY)?)?;
        let key = self.string(usize_at(self.file, at.checked_add(4)?)?)?;
        let value = self.string(usize_at(self.file, at.checked_add(8)?)?)?;
        if value.first() != Some(&b'/') {
            return None; // a cache path is absolute: anything else is an inconsistent entry
        }
        Some((u32_at(self.file, at)?, key, value, u64_at(self.file, at.checked_add(16)?)?))
    }

    /// The hwcaps name with index `i`.
    fn hwcaps_name(&self, i: usize) -> Option<&'a [u8]> {
        if i >= self.hwcaps.1 {
            return None;
        }
        self.string(usize_at(self.file, self.hwcaps.0.checked_add(i.checked_mul(4)?)?)?)
    }

    /// The path the cache gives for `name`: among the entries whose key equals `name` under
    /// [`compare`] (file order) and whose flags are `flags`, the hwcaps entry whose subdirectory is
    /// earliest in `hwcaps` (the supported list, highest priority first) among those before the
    /// first plain entry, else that plain entry (Appendix D.3.3; owner #940). `None` when there is
    /// no such entry or an entry on the way is inconsistent.
    pub fn lookup(&self, name: &[u8], flags: u32, hwcaps: &[&str]) -> Option<&'a [u8]> {
        // Sorted descending: the first entry whose key is not greater than `name`.
        let (mut lo, mut hi) = (0usize, self.nlibs);
        while lo < hi {
            let mid = lo.midpoint(hi);
            if compare(self.entry(mid)?.1, name) == Ordering::Greater {
                lo = mid.saturating_add(1);
            } else {
                hi = mid;
            }
        }
        let mut best: Option<(usize, &'a [u8])> = None;
        for i in lo..self.nlibs {
            let (f, key, value, hwcap) = self.entry(i)?;
            if compare(key, name) != Ordering::Equal {
                break;
            }
            if f != flags {
                continue;
            }
            if hwcap == 0 {
                return Some(best.map_or(value, |b| b.1));
            }
            if hwcap & HWCAP_INDEX == 0 {
                continue; // a legacy hardware-capability entry: not one this loader selects
            }
            let index = usize::try_from(hwcap & 0xffff_ffff).ok()?;
            let Some(sub) = self.hwcaps_name(index) else { continue };
            if let Some(rank) = hwcaps.iter().position(|h| h.as_bytes() == sub) {
                if best.is_none_or(|b| rank < b.0) {
                    best = Some((rank, value));
                }
            }
        }
        best.map(|b| b.1)
    }
}
