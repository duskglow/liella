//! The loader cache writer (UTIL-2, plan/utilities.md §4; format: spec Appendix D.3.1 and D.3.2,
//! order: D.3.3).
//!
//! The writer produces exactly the new format (`glibc-ld.so.cache1.1`) with the tag 0 (generator
//! text) and tag 1 (glibc-hwcaps names) extensions: what Liella's reader ([`super::Cache`]) reads
//! and what other direct readers of the file expect.
//!
//! * **Order.** Entries are sorted by [`order`]: descending by key under [`super::compare`] (the
//!   comparison the reader's binary search uses, so writer order and reader search agree by
//!   construction); ties by configuration order (`Entry::order`), then a hwcaps entry before a
//!   plain one, then path bytes; the remaining fields only separate entries no reader can tell
//!   apart. The order is total, so the output depends only on the set of entries given, never on
//!   the order they arrive in (no directory-order or file-system dependence).
//! * **Untrusted input.** Keys, paths, hwcaps names and the generator text come from file names
//!   and ELF headers: each is validated and bounded ([`NAME_MAX`], [`PATH_MAX`], [`HWCAPS_MAX`],
//!   [`GENERATOR_MAX`], [`ENTRIES_MAX`], and every offset must fit the format's 32 bits) before
//!   anything is written. A path is absolute, its components are non-empty and neither `.` nor
//!   `..`, and it ends in `/<key>` (in `/glibc-hwcaps/<name>/<key>` for a hwcaps entry): the
//!   value is the soname link in the scanned directory (Appendix D.3.1).
//! * **No allocation.** The caller's entry slice is sorted in place and the file is written into
//!   the caller's buffer ([`encoded_len`] gives its size), so the freestanding ldconfig (UTIL-3)
//!   and the loader crate need no allocator for it. Nothing is mapped or executed.

use core::cmp::Ordering;

use super::{compare, ENTRY, EXT_MAGIC, HEADER, HEADER_FLAGS, HWCAP_INDEX, MAGIC, TAG_GENERATOR, TAG_HWCAPS};

/// The longest key and hwcaps name, and the longest path component (bytes, without the NUL).
pub const NAME_MAX: usize = 255;
/// The longest path (bytes, without the NUL).
pub const PATH_MAX: usize = 4095;
/// The most distinct hwcaps subdirectory names in one cache.
pub const HWCAPS_MAX: usize = 64;
/// The longest generator text.
pub const GENERATOR_MAX: usize = 4096;
/// The most entries in one cache.
pub const ENTRIES_MAX: usize = 1 << 20;

/// The directory a hwcaps entry's subdirectory lives in.
const HWCAPS_DIR: &[u8] = b"/glibc-hwcaps/";

/// One library to record.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Entry<'a> {
    /// Library type and ABI (Appendix D.3.1; `Arch::CACHE_FLAGS` for this architecture).
    pub flags: u32,
    /// The soname, or the file name of a library without one.
    pub key: &'a [u8],
    /// The absolute path of the soname link: `<dir>/<key>`, or `<dir>/glibc-hwcaps/<name>/<key>`.
    pub path: &'a [u8],
    /// The glibc-hwcaps subdirectory name of a hwcaps entry.
    pub hwcaps: Option<&'a [u8]>,
    /// The position of the entry's directory in the configuration (smaller = earlier); a hwcaps
    /// entry takes the position of the directory it is under.
    pub order: u32,
}

/// Why a cache was not written. An index names the entry in the caller's order (before sorting).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteError {
    /// More than [`ENTRIES_MAX`] entries.
    TooManyEntries,
    /// The key is empty, longer than [`NAME_MAX`], `.` or `..`, or holds `/` or NUL.
    Key(usize),
    /// The path is not absolute and canonical, is longer than [`PATH_MAX`], or does not end in
    /// `/<key>` (`/glibc-hwcaps/<name>/<key>` for a hwcaps entry).
    Path(usize),
    /// The hwcaps name is not a valid subdirectory name (as for a key).
    Hwcaps(usize),
    /// More than [`HWCAPS_MAX`] distinct hwcaps names.
    TooManyHwcaps,
    /// The generator text is longer than [`GENERATOR_MAX`] or holds NUL.
    Generator,
    /// An offset or size would not fit the format's 32 bits.
    TooLarge,
    /// The output buffer is smaller than the file: the size it needs.
    Buffer(usize),
}

/// The writer's total order (module documentation): `Less` when `a` is written before `b`.
pub fn order(a: &Entry<'_>, b: &Entry<'_>) -> Ordering {
    compare(a.key, b.key)
        .reverse()
        .then(a.order.cmp(&b.order))
        .then(b.hwcaps.is_some().cmp(&a.hwcaps.is_some()))
        .then_with(|| a.path.cmp(b.path))
        .then_with(|| a.key.cmp(b.key))
        .then_with(|| a.hwcaps.cmp(&b.hwcaps))
        .then(a.flags.cmp(&b.flags))
}

/// A file or directory name: non-empty, bounded, not `.` or `..`, no `/` or NUL.
fn name_ok(s: &[u8]) -> bool {
    !s.is_empty() && s.len() <= NAME_MAX && s != b"." && s != b".." && !s.iter().any(|&c| c == 0 || c == b'/')
}

/// An absolute path whose components are all names.
fn path_ok(p: &[u8]) -> bool {
    p.len() <= PATH_MAX && p.strip_prefix(b"/").is_some_and(|rest| rest.split(|&c| c == b'/').all(name_ok))
}

/// Validate entry `i`.
fn check(i: usize, e: &Entry<'_>) -> Result<(), WriteError> {
    if !name_ok(e.key) {
        return Err(WriteError::Key(i));
    }
    if let Some(h) = e.hwcaps {
        if !name_ok(h) {
            return Err(WriteError::Hwcaps(i));
        }
    }
    let dir = e.path.strip_suffix(e.key).and_then(|d| d.strip_suffix(b"/"));
    let dir = match (dir, e.hwcaps) {
        (Some(d), Some(h)) => d.strip_suffix(h).and_then(|d| d.strip_suffix(HWCAPS_DIR)),
        (d, None) => d,
        (None, Some(_)) => None,
    };
    if dir.is_none() || !path_ok(e.path) {
        return Err(WriteError::Path(i));
    }
    Ok(())
}

/// The distinct hwcaps names, sorted by bytes (deterministic indexes).
struct Names<'a> {
    names: [&'a [u8]; HWCAPS_MAX],
    len: usize,
}

impl<'a> Names<'a> {
    fn collect(entries: &[Entry<'a>]) -> Result<Self, WriteError> {
        let mut n = Names { names: [&[]; HWCAPS_MAX], len: 0 };
        for h in entries.iter().filter_map(|e| e.hwcaps) {
            let used = n.names.get(..n.len).unwrap_or(&[]);
            if let Err(at) = used.binary_search(&h) {
                if n.len >= HWCAPS_MAX {
                    return Err(WriteError::TooManyHwcaps);
                }
                n.names.get_mut(at..=n.len).ok_or(WriteError::TooManyHwcaps)?.rotate_right(1);
                *n.names.get_mut(at).ok_or(WriteError::TooManyHwcaps)? = h;
                n.len = n.len.saturating_add(1);
            }
        }
        Ok(n)
    }

    fn used(&self) -> &[&'a [u8]] {
        self.names.get(..self.len).unwrap_or(&[])
    }
}

/// Where each part of the file goes.
struct Layout {
    strings: usize,
    len_strings: usize,
    ext: usize,
    generator: usize,
    array: usize,
    total: usize,
}

fn add(a: usize, b: usize) -> Result<usize, WriteError> {
    a.checked_add(b).ok_or(WriteError::TooLarge)
}

fn mul(a: usize, b: usize) -> Result<usize, WriteError> {
    a.checked_mul(b).ok_or(WriteError::TooLarge)
}

fn up4(a: usize) -> Result<usize, WriteError> {
    a.checked_next_multiple_of(4).ok_or(WriteError::TooLarge)
}

fn layout(entries: &[Entry<'_>], names: &Names<'_>, generator: &[u8]) -> Result<Layout, WriteError> {
    let strings = add(HEADER, mul(ENTRY, entries.len())?)?;
    let mut len_strings = 0usize;
    for s in names.used().iter().copied().chain(entries.iter().map(|e| e.path)) {
        len_strings = add(len_strings, add(s.len(), 1)?)?;
    }
    let strings_end = add(strings, len_strings)?;
    let records = usize::from(!generator.is_empty()).saturating_add(usize::from(names.len != 0));
    let (ext, generator_at, array, total) = if records == 0 {
        (0, 0, 0, strings_end)
    } else {
        let ext = up4(strings_end)?;
        let generator_at = add(ext, add(8, mul(16, records)?)?)?;
        let generator_end = add(generator_at, generator.len())?;
        if names.len == 0 {
            (ext, generator_at, 0, generator_end)
        } else {
            let array = up4(generator_end)?;
            (ext, generator_at, array, add(array, mul(4, names.len)?)?)
        }
    };
    if u32::try_from(total).is_err() {
        return Err(WriteError::TooLarge);
    }
    Ok(Layout { strings, len_strings, ext, generator: generator_at, array, total })
}

/// Validate everything; the hwcaps names and the layout.
fn prepare<'a>(entries: &[Entry<'a>], generator: &[u8]) -> Result<(Names<'a>, Layout), WriteError> {
    if entries.len() > ENTRIES_MAX {
        return Err(WriteError::TooManyEntries);
    }
    if generator.len() > GENERATOR_MAX || generator.contains(&0) {
        return Err(WriteError::Generator);
    }
    for (i, e) in entries.iter().enumerate() {
        check(i, e)?;
    }
    let names = Names::collect(entries)?;
    let layout = layout(entries, &names, generator)?;
    Ok((names, layout))
}

/// The size of the file [`write`] makes for `entries` and `generator` (validating them).
pub fn encoded_len(entries: &[Entry<'_>], generator: &[u8]) -> Result<usize, WriteError> {
    prepare(entries, generator).map(|(_, l)| l.total)
}

/// The output buffer, written at checked positions.
struct Out<'o> {
    buf: &'o mut [u8],
    need: usize,
}

impl Out<'_> {
    fn put(&mut self, at: usize, bytes: &[u8]) -> Result<(), WriteError> {
        let end = add(at, bytes.len())?;
        self.buf.get_mut(at..end).ok_or(WriteError::Buffer(self.need))?.copy_from_slice(bytes);
        Ok(())
    }

    fn u32(&mut self, at: usize, v: usize) -> Result<(), WriteError> {
        let v = u32::try_from(v).map_err(|_| WriteError::TooLarge)?;
        self.put(at, &v.to_le_bytes())
    }
}

/// Write the cache for `entries` (sorted in place by [`order`]) with the generator text
/// `generator` (no tag 0 record when it is empty) into `out`; the number of bytes written. Nothing
/// is written unless every input is valid and `out` is large enough.
pub fn write(entries: &mut [Entry<'_>], generator: &[u8], out: &mut [u8]) -> Result<usize, WriteError> {
    let (names, l) = prepare(entries, generator)?;
    if out.len() < l.total {
        return Err(WriteError::Buffer(l.total));
    }
    entries.sort_unstable_by(order);
    let mut o = Out { buf: out, need: l.total };
    o.buf.get_mut(..l.total).ok_or(WriteError::Buffer(l.total))?.fill(0);
    o.put(0, MAGIC)?;
    o.u32(20, entries.len())?;
    o.u32(24, l.len_strings)?;
    o.put(28, &[HEADER_FLAGS])?;
    o.u32(32, l.ext)?;
    // The string table: the hwcaps names, then each entry's path; a key is the tail of its path.
    let mut at = l.strings;
    let mut name_at = [0usize; HWCAPS_MAX];
    for (slot, n) in name_at.iter_mut().zip(names.used()) {
        *slot = at;
        o.put(at, n)?;
        at = add(at, add(n.len(), 1)?)?;
    }
    for (i, e) in entries.iter().enumerate() {
        let rec = add(HEADER, mul(ENTRY, i)?)?;
        let key_at = add(at, e.path.len().saturating_sub(e.key.len()))?;
        o.put(rec, &e.flags.to_le_bytes())?;
        o.u32(add(rec, 4)?, key_at)?;
        o.u32(add(rec, 8)?, at)?;
        let hwcap = match e.hwcaps {
            None => 0,
            Some(h) => {
                let index = names.used().binary_search(&h).map_err(|_| WriteError::Hwcaps(i))?;
                HWCAP_INDEX | u64::try_from(index).map_err(|_| WriteError::TooLarge)?
            }
        };
        o.put(add(rec, 16)?, &hwcap.to_le_bytes())?;
        o.put(at, e.path)?;
        at = add(at, add(e.path.len(), 1)?)?;
    }
    if l.ext != 0 {
        let mut records = 0usize;
        let mut record = |o: &mut Out<'_>, tag: u32, offset: usize, size: usize| -> Result<(), WriteError> {
            let rec = add(l.ext, add(8, mul(16, records)?)?)?;
            o.put(rec, &tag.to_le_bytes())?;
            o.u32(add(rec, 8)?, offset)?;
            o.u32(add(rec, 12)?, size)?;
            records = records.saturating_add(1);
            Ok(())
        };
        if !generator.is_empty() {
            record(&mut o, TAG_GENERATOR, l.generator, generator.len())?;
            o.put(l.generator, generator)?;
        }
        if names.len != 0 {
            record(&mut o, TAG_HWCAPS, l.array, mul(4, names.len)?)?;
            for (k, off) in name_at.iter().take(names.len).enumerate() {
                o.u32(add(l.array, mul(4, k)?)?, *off)?;
            }
        }
        o.put(l.ext, &EXT_MAGIC.to_le_bytes())?;
        o.u32(add(l.ext, 4)?, records)?;
    }
    Ok(l.total)
}
