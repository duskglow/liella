//! GNU message objects (`.mo`; spec §7.7, Appendix D.2.1, D-7.5; the GNU gettext manual, "The
//! Format of GNU MO Files").
//!
//! [`Mo::parse`] validates a whole image before any lookup: the header, both descriptor tables,
//! and every string's start, so that a lookup never reads outside the image. The image must end
//! with a NUL (the caller copies a file that does not, D.2.1 "files that are not catalogs"), so
//! every string is terminated within it. A file that fails the checks is not a catalog: it is
//! treated as absent (decision; the reference library reads outside its mapping or crashes on
//! some of them).
//!
//! **Lookup** (D-7.5, decision 2026-10-04): a binary search of the sorted original strings; the
//! hash table is neither used nor checked. Keys compare as NUL-terminated strings (the lengths in
//! the original table are not used, as observed); a plural entry's key is its singular part.
//!
//! **Index** (D-7.12, PJ-9): [`index`] records, once per load, each original's offset and length
//! (from its NUL, as above) and, when the originals are strictly ascending, an open-addressing
//! hash table of them; [`Mo::find_in`] then hashes the caller's key once and compares one
//! candidate. Originals out of order or repeated keep the binary search (over the stored
//! lengths), so every answer is the one [`Mo::find`] gives.
//!
//! **System-dependent strings** (revision minor 1, D.2.1): [`expand`] builds an ordinary image in
//! which each such string appears with its segments expanded for x86_64 (`<PRIu64>` is `lu`).

use core::cmp::Ordering;

use crate::error::Errno;
use crate::mem::TryVec;

/// The magic number, in the file's own byte order.
const MAGIC: u32 = 0x9504_12de;
/// The header of revision 0 (28 bytes) and of minor revision 1 (48 bytes).
const HEADER: usize = 28;
const HEADER_SYSDEP: usize = 48;

/// A validated `.mo` image.
#[derive(Clone, Copy, Debug)]
pub struct Mo<'a> {
    image: &'a [u8],
    big_endian: bool,
    n: usize,
    originals: usize,
    translations: usize,
    sysdep: bool,
}

fn u32_at(image: &[u8], at: usize, be: bool) -> Option<u32> {
    let b: [u8; 4] = image.get(at..at.checked_add(4)?)?.try_into().ok()?;
    Some(if be { u32::from_be_bytes(b) } else { u32::from_le_bytes(b) })
}

fn usize_at(image: &[u8], at: usize, be: bool) -> Option<usize> {
    usize::try_from(u32_at(image, at, be)?).ok()
}

/// The NUL-terminated string at `off` (without its NUL).
fn string_at(image: &[u8], off: usize) -> &[u8] {
    let s = image.get(off..).unwrap_or(&[]);
    s.get(..s.iter().position(|&b| b == 0).unwrap_or(s.len())).unwrap_or(&[])
}

/// Whether a table of `n` descriptors at `at` lies in the image, and every string it describes
/// starts inside the image.
fn table_ok(image: &[u8], at: usize, n: usize, be: bool) -> bool {
    let Some(end) = n.checked_mul(8).and_then(|b| b.checked_add(at)) else { return false };
    if end > image.len() {
        return false;
    }
    (0..n).all(|i| {
        i.checked_mul(8).and_then(|o| o.checked_add(at)).and_then(|d| d.checked_add(4)).and_then(|d| usize_at(image, d, be)).is_some_and(|off| off < image.len())
    })
}

impl<'a> Mo<'a> {
    /// Validate `image` (which must end with a NUL; see the module documentation). `None` if it
    /// is not a usable catalog: shorter than its header, a wrong magic number, a revision other
    /// than major 0 or 1 with minor 0 or 1 (minor 1 with its fields before both tables), tables
    /// or string starts outside the image.
    pub fn parse(image: &'a [u8]) -> Option<Mo<'a>> {
        let m = Mo::open(image)?;
        (table_ok(image, m.originals, m.n, m.big_endian) && table_ok(image, m.translations, m.n, m.big_endian)).then_some(m)
    }

    /// An image [`Mo::parse`] accepted before: the header's checks only (every later access is
    /// bounds-checked anyway).
    pub fn open(image: &'a [u8]) -> Option<Mo<'a>> {
        if image.last() != Some(&0) || image.len() < HEADER {
            return None;
        }
        let big_endian = match u32_at(image, 0, false)? {
            MAGIC => false,
            m if m.swap_bytes() == MAGIC => true,
            _ => return None,
        };
        let revision = u32_at(image, 4, big_endian)?;
        let (major, minor) = (revision >> 16, revision & 0xffff);
        if major > 1 || minor > 1 {
            return None;
        }
        let n = usize_at(image, 8, big_endian)?;
        let originals = usize_at(image, 12, big_endian)?;
        let translations = usize_at(image, 16, big_endian)?;
        let sysdep = minor == 1;
        if sysdep && (originals < HEADER_SYSDEP || translations < HEADER_SYSDEP || image.len() < HEADER_SYSDEP) {
            return None;
        }
        let table_end = |at: usize| n.checked_mul(8).and_then(|b| b.checked_add(at)).is_some_and(|end| end <= image.len());
        if !table_end(originals) || !table_end(translations) {
            return None;
        }
        Some(Mo { image, big_endian, n, originals, translations, sysdep })
    }

    /// The fields [`Mo::open`] read from the header (byte order and revision flags, count, table
    /// offsets), for [`Mo::from_fields`].
    pub fn fields(&self) -> [usize; 4] {
        [usize::from(self.big_endian) | usize::from(self.sysdep) << 1, self.n, self.originals, self.translations]
    }

    /// The image `image` with the header fields `f` that [`Mo::fields`] gave for it, without
    /// reading its header again (a cached lookup plan, GETTEXT-4). Never unsafe: every access
    /// is bounds-checked, so fields that do not describe `image` give wrong answers, not faults.
    pub fn from_fields(image: &'a [u8], f: [usize; 4]) -> Mo<'a> {
        let [flags, n, originals, translations] = f;
        Mo { image, big_endian: flags & 1 != 0, n, originals, translations, sysdep: flags & 2 != 0 }
    }

    /// The number of strings (pairs).
    pub fn len(&self) -> usize {
        self.n
    }

    /// Whether the catalog has no strings.
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// Whether the file has system-dependent strings to expand ([`expand`]).
    pub fn has_sysdep(&self) -> bool {
        self.sysdep && usize_at(self.image, 36, self.big_endian).is_some_and(|m| m > 0)
    }

    fn descriptor(&self, table: usize, i: usize) -> Option<(usize, usize)> {
        let at = i.checked_mul(8)?.checked_add(table)?;
        Some((usize_at(self.image, at.checked_add(4)?, self.big_endian)?, usize_at(self.image, at, self.big_endian)?))
    }

    /// Original string `i` (its key: up to its first NUL).
    pub fn original(&self, i: usize) -> &'a [u8] {
        self.descriptor(self.originals, i).map_or(&[], |(off, _)| string_at(self.image, off))
    }

    /// Translation `i`: its offset in the image and its declared length (which covers every plural
    /// form, not counting the final NUL).
    pub fn translation(&self, i: usize) -> Option<(usize, usize)> {
        self.descriptor(self.translations, i)
    }

    /// The index of the entry whose key is `key`, by binary search (the first in table order among
    /// equal keys that the search meets).
    pub fn find(&self, key: &[u8]) -> Option<usize> {
        let (mut lo, mut hi) = (0usize, self.n);
        while lo < hi {
            let mid = lo.checked_add(hi.checked_sub(lo)? / 2)?;
            match key.cmp(self.original(mid)) {
                Ordering::Equal => return Some(mid),
                Ordering::Less => hi = mid,
                Ordering::Greater => lo = mid.checked_add(1)?,
            }
        }
        None
    }

    /// [`Mo::find`] through an [`index`] of this image: the same answer, with one hash of `key`
    /// and one comparison (or a binary search over the stored lengths). An index that does not
    /// describe this image (another count) is ignored.
    pub fn find_in(&self, index: &[u8], key: &[u8]) -> Option<usize> {
        self.find_in_hinted(index, key, 0)
    }

    /// [`Mo::find_in`] with `hint`: an entry number plus one (0: none), which an earlier answer of
    /// this image's index gave. The answer is a function of the key's bytes and the image alone,
    /// so when the hinted entry's original equals `key` it is the answer, found without the hash
    /// and the probe (GETTEXT-FOUND); a hint that does not match is ignored.
    pub fn find_in_hinted(&self, index: &[u8], key: &[u8], hint: usize) -> Option<usize> {
        let word = |j: usize| -> Option<usize> { usize::try_from(u32::from_ne_bytes(index.get(j.checked_mul(4)?..j.checked_mul(4)?.checked_add(4)?)?.try_into().ok()?)).ok() };
        // The three head words from one bounds check (GETTEXT-5).
        let Some(&[a0, a1, a2, a3, b0, b1, b2, b3, c0, c1, c2, c3]) = index.first_chunk::<12>() else { return self.find(key) };
        let rd = |b: [u8; 4]| usize::try_from(u32::from_ne_bytes(b)).ok();
        let (Some(mode), Some(n), Some(mask)) = (rd([a0, a1, a2, a3]), rd([b0, b1, b2, b3]), rd([c0, c1, c2, c3])) else { return self.find(key) };
        if n != self.n || mode > IX_HASH {
            return self.find(key);
        }
        // The original of entry `e`, from its stored offset and length.
        let original = |e: usize| -> Option<&'a [u8]> {
            let at = e.checked_mul(2)?.checked_add(IX_HEAD)?;
            let off = word(at)?;
            self.image.get(off..off.checked_add(word(at.checked_add(1)?)?)?)
        };
        if let Some(e) = hint.checked_sub(1) {
            if e < n && original(e).is_some_and(|o| same_bytes(o, key)) {
                return Some(e);
            }
        }
        if mode == IX_SEARCH {
            let (mut lo, mut hi) = (0usize, n);
            while lo < hi {
                let mid = lo.checked_add(hi.checked_sub(lo)? / 2)?;
                match key.cmp(original(mid)?) {
                    Ordering::Equal => return Some(mid),
                    Ordering::Less => hi = mid,
                    Ordering::Greater => lo = mid.checked_add(1)?,
                }
            }
            return None;
        }
        let table = n.checked_mul(2)?.checked_add(IX_HEAD)?;
        let mut slot = slot_of(key, mask);
        // The build placed every key within IX_PROBE_MAX slots of its home (else it kept the
        // search), so a key not found in that many slots is not in the catalog.
        for _ in 0..IX_PROBE_MAX.min(mask.saturating_add(1)) {
            #[cfg(test)]
            PROBES.with(|c| c.set(c.get().wrapping_add(1)));
            // An empty slot (0) ends the probe: the key is not in the catalog.
            let e = word(table.checked_add(slot)?)?.checked_sub(1)?;
            if same_bytes(original(e)?, key) {
                return Some(e);
            }
            slot = slot.wrapping_add(1) & mask;
        }
        None
    }

    /// The header entry (the translation of `""`), if the catalog has one.
    pub fn header(&self) -> Option<&'a [u8]> {
        let i = self.find(b"")?;
        let (off, _) = self.translation(i)?;
        Some(string_at(self.image, off))
    }

    /// The offset of plural form `k` of translation `i`, or of form 0 when the translation has no
    /// form `k` (as observed: a form exists when the declared length, bounded by the image, covers
    /// the NULs before it). Form 0 ignores the length.
    pub fn form(&self, i: usize, k: u64) -> Option<usize> {
        let (off, len) = self.translation(i)?;
        if k == 0 {
            return Some(off);
        }
        let end = off.saturating_add(len).min(self.image.len());
        let body = self.image.get(off..end).unwrap_or(&[]);
        let mut left = k;
        for (pos, &b) in body.iter().enumerate() {
            if b == 0 {
                left = left.saturating_sub(1);
                if left == 0 {
                    return off.checked_add(pos)?.checked_add(1);
                }
            }
        }
        Some(off)
    }

    /// The NUL-terminated text at `off` (without the NUL).
    pub fn text(&self, off: usize) -> &'a [u8] {
        string_at(self.image, off)
    }

    /// The bytes of translation `i` that hold its forms (the declared length, bounded by the
    /// image, and at least form 0).
    fn translation_bytes(&self, i: usize) -> &'a [u8] {
        let Some((off, len)) = self.translation(i) else { return &[] };
        let first = string_at(self.image, off).len();
        let end = off.saturating_add(len.max(first)).min(self.image.len());
        self.image.get(off..end).unwrap_or(&[])
    }
}

/// The charset the header entry names (`charset=` anywhere in it, up to a blank, `;` or the end
/// of the line), if any.
pub fn charset(header: &[u8]) -> Option<&[u8]> {
    const KEY: &[u8] = b"charset=";
    let at = header.windows(KEY.len()).position(|w| w == KEY)?;
    let v = header.get(at.checked_add(KEY.len())?..)?;
    v.get(..v.iter().position(|&b| matches!(b, b' ' | b'\t' | b'\n' | b';')).unwrap_or(v.len()))
}

/// Index layout ([`index`]): `u32` words in native order: the mode, the entry count, the hash
/// table's mask, then each original's offset and length, then (hash mode) the table, whose slots
/// hold an entry number plus one (0: empty).
const IX_HEAD: usize = 3;
const IX_SEARCH: usize = 0;
const IX_HASH: usize = 1;
/// The longest probe a table built here may need; a catalog whose keys collide more (only a
/// file made to) keeps the binary search. Building places a key only within this many slots of
/// its home and a lookup stops after as many, so neither can degrade.
const IX_PROBE_MAX: usize = 64;

#[cfg(test)]
thread_local! {
    /// Slots examined by hash-mode lookups on this thread (tests bound the probe length).
    static PROBES: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A 64-bit hash of `key`, eight bytes per step (multiply-rotate; the catalog's own hash table
/// is not used, D-7.5).
fn hash(key: &[u8]) -> u64 {
    const K: u64 = 0x9e37_79b9_7f4a_7c15;
    let step = |h: u64, w: u64| (h.rotate_left(5) ^ w).wrapping_mul(K);
    let mut h = u64::try_from(key.len()).unwrap_or(u64::MAX).wrapping_mul(K);
    let mut chunks = key.chunks_exact(8);
    for c in &mut chunks {
        h = step(h, c.try_into().map_or(0, u64::from_le_bytes));
    }
    let rest = chunks.remainder();
    if !rest.is_empty() {
        h = step(h, short_word(rest));
    }
    h ^ (h >> 32)
}

/// The zero-padded little-endian word of `b`, at most eight bytes (GETTEXT-5): the same value as
/// folding the bytes from the last, from two overlapping reads (four bytes each) or three byte
/// reads, with no loop and no copy call. Bytes beyond the eighth are ignored.
pub fn short_word(b: &[u8]) -> u64 {
    let n = b.len().min(8);
    let word4 = |at: usize| -> u64 { b.get(at..at.wrapping_add(4)).and_then(|s| <[u8; 4]>::try_from(s).ok()).map_or(0, |a| u64::from(u32::from_le_bytes(a))) };
    if n >= 4 {
        // Bytes 0..4 and n-4..n: where they overlap they are the same bytes.
        return word4(0) | (word4(n.wrapping_sub(4)) << (n.wrapping_sub(4) << 3));
    }
    if n == 0 {
        return 0;
    }
    let at = |i: usize| -> u64 { b.get(i).copied().map_or(0, u64::from) };
    // Bytes 0, n/2 and n-1 cover 1 to 3 bytes (equal positions give equal bytes).
    at(0) | (at(n >> 1) << ((n >> 1) << 3)) | (at(n.wrapping_sub(1)) << (n.wrapping_sub(1) << 3))
}

/// `a == b` for two slices, without a call for lengths up to 16: whole words from both ends.
fn same_bytes(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let n = a.len();
    if n > 16 {
        return a == b;
    }
    if n >= 8 {
        let w = |s: &[u8], at: usize| s.get(at..at.wrapping_add(8)).and_then(|x| <[u8; 8]>::try_from(x).ok()).map(u64::from_le_bytes);
        let end = n.wrapping_sub(8);
        return w(a, 0) == w(b, 0) && w(a, end) == w(b, end);
    }
    short_word(a) == short_word(b)
}

fn slot_of(key: &[u8], mask: usize) -> usize {
    usize::try_from(hash(key) & u64::from(u32::MAX)).unwrap_or(0) & mask
}

/// The lookup index of `m` (see the module documentation and [`Mo::find_in`]): `8n` bytes of
/// offsets and lengths plus, for strictly ascending originals, a table of 2 to 4 slots of 4
/// bytes per entry (at most `24n + 12` bytes in all). Built once per load; `None` (search
/// without it) when a length does not fit 32 bits.
pub fn index(m: &Mo<'_>) -> Result<Option<TryVec<u8>>, Errno> {
    let n = m.n;
    // Every word of the index (up to four slots per entry) fits 32 bits.
    if u32::try_from(n.saturating_mul(4)).is_err() {
        return Ok(None);
    }
    let mut spans: TryVec<(u32, u32)> = TryVec::with_capacity(n)?;
    for i in 0..n {
        let Some((off, _)) = m.descriptor(m.originals, i) else { return Ok(None) };
        let len = string_at(m.image, off).len();
        let (Ok(o), Ok(l)) = (u32::try_from(off), u32::try_from(len)) else { return Ok(None) };
        spans.push((o, l))?;
    }
    let key = |&(o, l): &(u32, u32)| -> &[u8] {
        let (o, l) = (usize::try_from(o).unwrap_or(usize::MAX), usize::try_from(l).unwrap_or(0));
        m.image.get(o..o.saturating_add(l)).unwrap_or(&[])
    };
    let ascending = spans.as_slice().windows(2).all(|w| matches!(w, [a, b] if key(a) < key(b)));
    let cap = if ascending { n.checked_mul(2).ok_or(Errno::ENOMEM)?.max(2).checked_next_power_of_two().ok_or(Errno::ENOMEM)? } else { 0 };
    let mask = cap.saturating_sub(1);
    let mut table: TryVec<u32> = TryVec::new();
    table.resize(cap, 0)?;
    let mut hashed = ascending;
    for (i, s) in spans.as_slice().iter().enumerate() {
        if !hashed {
            break;
        }
        let mut slot = slot_of(key(s), mask);
        let mut placed = false;
        for _ in 0..IX_PROBE_MAX.min(cap) {
            if let Some(t) = table.as_mut_slice().get_mut(slot) {
                if *t == 0 {
                    *t = u32::try_from(i.saturating_add(1)).map_err(|_| Errno::ENOMEM)?;
                    placed = true;
                    break;
                }
            }
            slot = slot.wrapping_add(1) & mask;
        }
        hashed = placed;
    }
    let words = |out: &mut TryVec<u8>, v: usize| -> Result<(), Errno> { out.extend_from_slice(&u32::try_from(v).map_err(|_| Errno::ENOMEM)?.to_ne_bytes()) };
    let size = n.checked_mul(2).and_then(|w| w.checked_add(IX_HEAD)).and_then(|w| w.checked_add(if hashed { cap } else { 0 })).and_then(|w| w.checked_mul(4)).ok_or(Errno::ENOMEM)?;
    let mut out = TryVec::with_capacity(size)?;
    words(&mut out, if hashed { IX_HASH } else { IX_SEARCH })?;
    words(&mut out, n)?;
    words(&mut out, if hashed { mask } else { 0 })?;
    for &(o, l) in spans.as_slice() {
        out.extend_from_slice(&o.to_ne_bytes())?;
        out.extend_from_slice(&l.to_ne_bytes())?;
    }
    if hashed {
        for &t in table.as_slice() {
            out.extend_from_slice(&t.to_ne_bytes())?;
        }
    }
    Ok(Some(out))
}

/// The expansion of a system-dependent segment on x86_64: the `<inttypes.h>` format macros
/// (`PRI` and `SCN` with `d`, `i`, `o`, `u`, `x`, `X`; widths 8, 16, 32, 64, `LEAST*`, `FAST*`,
/// `MAX`, `PTR`; established from the installed header's values) and the `I` flag.
fn segment(name: &[u8]) -> Option<TryVec<u8>> {
    let mut out = TryVec::new();
    if name == b"I" {
        out.push(b'I').ok()?;
        return Some(out);
    }
    let (scan, rest) = match (name.strip_prefix(b"PRI"), name.strip_prefix(b"SCN")) {
        (Some(r), _) => (false, r),
        (_, Some(r)) => (true, r),
        _ => return None,
    };
    let (&conv, width) = rest.split_first()?;
    if !matches!(conv, b'd' | b'i' | b'o' | b'u' | b'x') && !(conv == b'X' && !scan) {
        return None;
    }
    let prefix: &[u8] = match width {
        b"8" | b"LEAST8" | b"FAST8" => {
            if scan {
                b"hh"
            } else {
                b""
            }
        }
        b"16" | b"LEAST16" => {
            if scan {
                b"h"
            } else {
                b""
            }
        }
        b"32" | b"LEAST32" => b"",
        b"64" | b"LEAST64" | b"FAST16" | b"FAST32" | b"FAST64" | b"MAX" | b"PTR" => b"l",
        _ => return None,
    };
    out.extend_from_slice(prefix).ok()?;
    out.push(conv).ok()?;
    Some(out)
}

/// Expand system-dependent string `j` (from the table at `table`) into `out`, with its final
/// NUL: `Ok(false)` if a segment is unknown or the descriptor is malformed.
fn expand_one(mo: &Mo<'_>, table: usize, j: usize, out: &mut TryVec<u8>) -> Result<bool, Errno> {
    let (image, be) = (mo.image, mo.big_endian);
    let Some(segments) = usize_at(image, 32, be) else { return Ok(false) };
    let Some(nseg) = usize_at(image, 28, be) else { return Ok(false) };
    let Some(desc) = j.checked_mul(4).and_then(|o| o.checked_add(table)).and_then(|a| usize_at(image, a, be)) else { return Ok(false) };
    let Some(mut text) = usize_at(image, desc, be) else { return Ok(false) };
    let mut at = desc.saturating_add(4);
    // At most one pair per byte of the image: a malformed table cannot loop.
    for _ in 0..image.len() {
        let (Some(size), Some(reference)) = (usize_at(image, at, be), u32_at(image, at.saturating_add(4), be)) else { return Ok(false) };
        at = at.saturating_add(8);
        let Some(part) = text.checked_add(size).and_then(|end| image.get(text..end)) else { return Ok(false) };
        out.extend_from_slice(part)?;
        text = text.saturating_add(size);
        if reference == u32::MAX {
            return Ok(out.as_slice().last() == Some(&0));
        }
        let Ok(r) = usize::try_from(reference) else { return Ok(false) };
        if r >= nseg {
            return Ok(false);
        }
        let Some(name_off) = r.checked_mul(8).and_then(|o| o.checked_add(segments)).and_then(|a| usize_at(image, a.saturating_add(4), be)) else { return Ok(false) };
        let Some(exp) = segment(string_at(image, name_off)) else { return Ok(false) };
        out.extend_from_slice(exp.as_slice())?;
    }
    Ok(false)
}

/// One entry of an expanded image: key, translation bytes (forms and their final NUL), position.
struct Entry {
    key: (usize, usize),
    value: (usize, usize),
    order: usize,
}

/// An ordinary (revision 0, little-endian) image holding `mo`'s strings and its system-dependent
/// strings expanded for x86_64 (see the module documentation), sorted for [`Mo::find`]. Entries
/// whose segments are unknown are left out.
pub fn expand(mo: &Mo<'_>) -> Result<TryVec<u8>, Errno> {
    let (image, be) = (mo.image, mo.big_endian);
    let mut pool: TryVec<u8> = TryVec::new();
    let mut entries: TryVec<Entry> = TryVec::new();
    let mut add = |pool: &mut TryVec<u8>, key: &[u8], value: &[u8], order: usize| -> Result<(), Errno> {
        let k = (pool.len(), key.len());
        pool.extend_from_slice(key)?;
        let v = (pool.len(), value.len());
        pool.extend_from_slice(value)?;
        entries.push(Entry { key: k, value: v, order })
    };
    let mut tmp = TryVec::new();
    for i in 0..mo.n {
        let key = mo.original(i);
        tmp.clear();
        tmp.extend_from_slice(key)?;
        tmp.push(0)?;
        let mut val = TryVec::from_slice(mo.translation_bytes(i))?;
        val.push(0)?;
        add(&mut pool, tmp.as_slice(), val.as_slice(), i)?;
    }
    let m = usize_at(image, 36, be).unwrap_or(0);
    let (orig_tab, trans_tab) = (usize_at(image, 40, be).unwrap_or(0), usize_at(image, 44, be).unwrap_or(0));
    for j in 0..m.min(image.len()) {
        let (mut k, mut v) = (TryVec::new(), TryVec::new());
        if expand_one(mo, orig_tab, j, &mut k)? && expand_one(mo, trans_tab, j, &mut v)? {
            add(&mut pool, k.as_slice(), v.as_slice(), mo.n.saturating_add(j))?;
        }
    }
    let p = pool.as_slice();
    let key_of = |e: &Entry| -> &[u8] { string_at(p.get(e.key.0..e.key.0.saturating_add(e.key.1)).unwrap_or(&[]), 0) };
    entries.as_mut_slice().sort_unstable_by(|a, b| key_of(a).cmp(key_of(b)).then(a.order.cmp(&b.order)));
    // Header (28 bytes), the two tables, then the strings.
    let n = entries.len();
    let tables = n.checked_mul(16).and_then(|t| t.checked_add(HEADER)).ok_or(Errno::ENOMEM)?;
    let total = tables.checked_add(p.len()).ok_or(Errno::ENOMEM)?;
    let mut out = TryVec::with_capacity(total)?;
    let w32 = |out: &mut TryVec<u8>, v: usize| -> Result<(), Errno> { out.extend_from_slice(&u32::try_from(v).map_err(|_| Errno::ENOMEM)?.to_le_bytes()) };
    for v in [usize::try_from(MAGIC).unwrap_or(0), 0, n, HEADER, HEADER.saturating_add(n.saturating_mul(8)), 0, tables] {
        w32(&mut out, v)?;
    }
    for e in entries.as_slice() {
        // The declared length excludes the final NUL.
        w32(&mut out, e.key.1.saturating_sub(1))?;
        w32(&mut out, tables.saturating_add(e.key.0))?;
    }
    for e in entries.as_slice() {
        w32(&mut out, e.value.1.saturating_sub(1))?;
        w32(&mut out, tables.saturating_add(e.value.0))?;
    }
    out.extend_from_slice(p)?;
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions)]
mod tests {
    use super::*;
    /// `short_word` is the zero-padded little-endian fold of up to eight bytes, and `same_bytes`
    /// agrees with `==` for every length up to 20 and a change of any single byte (GETTEXT-5).
    #[test]
    fn short_word_and_same_bytes() {
        let text: Vec<u8> = (1..=40u8).map(|b| b.wrapping_mul(37) | 1).collect();
        for len in 0..=8usize {
            let want = text[..len].iter().rev().fold(0u64, |w, &b| (w << 8) | u64::from(b));
            assert_eq!(short_word(&text[..len]), want, "len {len}");
        }
        for len in 0..=20usize {
            let a = &text[..len];
            assert!(same_bytes(a, a), "len {len}");
            assert!(!same_bytes(a, &text[..len + 1]), "len {len}: longer");
            for i in 0..len {
                let mut b = a.to_vec();
                b[i] ^= 0x01;
                assert!(!same_bytes(a, &b), "len {len}, byte {i}");
            }
        }
    }


    /// A table-less catalog of `pairs` (in the given order), as the GNU manual lays it out.
    pub(crate) fn build(pairs: &[(&[u8], &[u8])], be: bool) -> Vec<u8> {
        let n = pairs.len();
        let (o, t) = (28usize, 28 + 8 * n);
        let mut strings = Vec::new();
        let mut descs = Vec::new();
        let base = 28 + 16 * n;
        for (k, _) in pairs {
            descs.push((k.len(), base + strings.len()));
            strings.extend_from_slice(k);
            strings.push(0);
        }
        for (_, v) in pairs {
            descs.push((v.len(), base + strings.len()));
            strings.extend_from_slice(v);
            strings.push(0);
        }
        let w = |v: usize| if be { (v as u32).to_be_bytes() } else { (v as u32).to_le_bytes() };
        let mut out = Vec::new();
        for v in [MAGIC as usize, 0, n, o, t, 0, base] {
            out.extend_from_slice(&w(v));
        }
        for (l, off) in descs {
            out.extend_from_slice(&w(l));
            out.extend_from_slice(&w(off));
        }
        out.extend_from_slice(&strings);
        out
    }

    #[test]
    fn lookup_both_orders() {
        let pairs: [(&[u8], &[u8]); 4] = [(b"", b"Content-Type: text/plain; charset=UTF-8\n"), (b"file\0files", b"Datei\0Dateien"), (b"hello", b"hallo"), (b"menu\x04open", b"oeffnen")];
        for be in [false, true] {
            let img = build(&pairs, be);
            let mo = Mo::parse(&img).unwrap();
            assert_eq!(mo.find(b"hello"), Some(2));
            assert_eq!(mo.find(b"file"), Some(1));
            assert_eq!(mo.find(b"hell"), None);
            assert_eq!(mo.text(mo.form(1, 1).unwrap()), b"Dateien");
            assert_eq!(mo.text(mo.form(1, 2).unwrap()), b"Datei", "beyond the stored forms: form 0");
            assert_eq!(charset(mo.header().unwrap()), Some(&b"UTF-8"[..]));
        }
    }

    /// `find_in` through the index answers as `find` for every key, its prefixes and extensions,
    /// and misses.
    fn same_answers(img: &[u8], probes: &[Vec<u8>], hashed: bool) {
        let mo = Mo::parse(img).unwrap();
        let ix = index(&mo).unwrap().unwrap();
        assert_eq!(u32::from_ne_bytes(ix[..4].try_into().unwrap()) == 1, hashed, "index mode");
        // Rebuilt from its fields (a lookup plan, GETTEXT-4): the same answers and forms.
        let again = Mo::from_fields(img, mo.fields());
        for p in probes {
            assert_eq!(mo.find_in(&ix, p), mo.find(p), "key {:?}", String::from_utf8_lossy(p));
            let i = again.find_in(&ix, p);
            assert_eq!(i, mo.find(p), "rebuilt, key {:?}", String::from_utf8_lossy(p));
            if let Some(i) = i {
                assert_eq!((again.form(i, 0), again.form(i, 1)), (mo.form(i, 0), mo.form(i, 1)));
            }
        }
        assert_eq!(mo.find_in(&ix[..8], b"hello"), mo.find(b"hello"), "a damaged index is ignored");
    }

    fn probes_of(keys: &[&[u8]]) -> Vec<Vec<u8>> {
        let mut out = vec![b"".to_vec(), b"zz".to_vec(), b"\xff".to_vec()];
        for k in keys {
            let k = string_at(k, 0).to_vec();
            for cut in 0..=k.len() {
                out.push(k[..cut].to_vec());
            }
            out.push([k.as_slice(), b"x"].concat());
            out.push([k.as_slice(), b"\0"].concat());
        }
        out
    }

    #[test]
    fn index_matches_search() {
        let pairs: [(&[u8], &[u8]); 7] = [
            (b"", b"Content-Type: text/plain; charset=UTF-8\n"),
            (b"fil", b"F"),
            (b"file\0files", b"Datei\0Dateien"),
            (b"hello", b"hallo"),
            (b"hello world, a key longer than eight bytes", b"hallo welt"),
            (b"menu\x04open", b"oeffnen"),
            (b"\xc3\xa9t\xc3\xa9", b"summer"),
        ];
        let keys: Vec<&[u8]> = pairs.iter().map(|p| p.0).collect();
        for be in [false, true] {
            same_answers(&build(&pairs, be), &probes_of(&keys), true);
        }
        // Declared key lengths that disagree with the NULs are not used (as observed).
        let mut img = build(&pairs, false);
        for i in 0..pairs.len() {
            img[28 + 8 * i..32 + 8 * i].copy_from_slice(&(1u32 + i as u32 * 7).to_le_bytes());
        }
        same_answers(&img, &probes_of(&keys), true);
        assert_eq!(Mo::parse(&img).unwrap().find(b"hello"), Some(3));
        // Out of order and repeated keys keep the binary search (and its answers).
        let unsorted: [(&[u8], &[u8]); 4] = [(b"pear", b"1"), (b"apple", b"2"), (b"zebra", b"3"), (b"apple", b"4")];
        let keys: Vec<&[u8]> = unsorted.iter().map(|p| p.0).collect();
        same_answers(&build(&unsorted, false), &probes_of(&keys), false);
        let dup: [(&[u8], &[u8]); 3] = [(b"a", b"1"), (b"b", b"2"), (b"b", b"3")];
        same_answers(&build(&dup, false), &probes_of(&[b"a", b"b"]), false);
        // Many keys: probing past collisions.
        let mut many: Vec<Vec<u8>> = (0..2000).map(|i| format!("message number {i}").into_bytes()).collect();
        many.sort();
        let pairs: Vec<(&[u8], &[u8])> = many.iter().map(|k| (k.as_slice(), b"t".as_slice())).collect();
        let img = build(&pairs, false);
        let mo = Mo::parse(&img).unwrap();
        let ix = index(&mo).unwrap().unwrap();
        for (i, k) in many.iter().enumerate() {
            assert_eq!(mo.find_in(&ix, k), Some(i));
            assert_eq!(mo.find_in(&ix, &[k.as_slice(), b"0"].concat()), mo.find(&[k.as_slice(), b"0"].concat()));
        }
        assert_eq!(mo.find_in(&ix, b"message number"), None);
        // GETTEXT-FOUND: a hint (right, wrong, out of range, none) never changes the answer, also with repeated keys.
        for (i, k) in many.iter().enumerate().step_by(37) {
            for hint in [0, 1, i, i + 1, i + 2, many.len(), many.len() + 1, usize::MAX] {
                assert_eq!(mo.find_in_hinted(&ix, k, hint), Some(i), "hint {hint}");
                assert_eq!(mo.find_in_hinted(&ix, b"message number 5x", hint), None, "absent, hint {hint}");
            }
        }
        let dup: [(&[u8], &[u8]); 4] = [(b"a", b"1"), (b"b", b"2"), (b"b", b"3"), (b"c", b"4")];
        let img = build(&dup, false);
        let mo = Mo::parse(&img).unwrap();
        let ix = index(&mo).unwrap().unwrap();
        for k in [b"a".as_slice(), b"b", b"c", b"d"] {
            // Hints are only ever earlier answers for the key's bytes: those leave the answer unchanged.
            let own = mo.find_in(&ix, k).map_or(0, |e| e + 1);
            for hint in [0, own] {
                assert_eq!(mo.find_in_hinted(&ix, k, hint), mo.find_in(&ix, k), "dup key {k:?} hint {hint}");
            }
        }
        // An empty catalog.
        let img = build(&[], false);
        same_answers(&img, &probes_of(&[]), true);
    }

    #[test]
    fn damaged_images_are_rejected() {
        let img = build(&[(b"hello", b"hallo")], false);
        assert!(Mo::parse(&img[..27]).is_none());
        let mut bad = img.clone();
        bad[0] = 0;
        assert!(Mo::parse(&bad).is_none());
        let mut huge = img.clone();
        huge[8..12].copy_from_slice(&0x7fff_ffffu32.to_le_bytes());
        assert!(Mo::parse(&huge).is_none());
        let mut off = img.clone();
        off[32..36].copy_from_slice(&0x100000u32.to_le_bytes());
        assert!(Mo::parse(&off).is_none());
        let mut rev = img.clone();
        rev[4..8].copy_from_slice(&0x20000u32.to_le_bytes());
        assert!(Mo::parse(&rev).is_none());
    }

    #[test]
    fn sysdep_segments() {
        assert_eq!(segment(b"PRIu64").unwrap().as_slice(), b"lu");
        assert_eq!(segment(b"PRId32").unwrap().as_slice(), b"d");
        assert_eq!(segment(b"SCNd8").unwrap().as_slice(), b"hhd");
        assert_eq!(segment(b"PRIxMAX").unwrap().as_slice(), b"lx");
        assert!(segment(b"SCNX32").is_none());
        assert!(segment(b"PRIq64").is_none());
    }

    /// A crafted catalog whose keys fill 1000 consecutive slots (each one placed at its home, so
    /// the build keeps the hash table): a miss whose home is the first slot must stop after
    /// IX_PROBE_MAX compares, not walk the run, and every answer stays that of `find`.
    #[test]
    fn miss_probe_is_capped() {
        let cap = 2048usize;
        // Keys homing at slots 0..1000, one each (the hash is unkeyed, so a file can do this).
        // Every candidate fills whichever of those slots it homes at (about 13 000 candidates;
        // searching the slots one after the other took about 2 million, 4 minutes unoptimised).
        let mut by_slot: Vec<Option<Vec<u8>>> = vec![None; 1000];
        let mut left = by_slot.len();
        let mut i = 0u64;
        while left > 0 {
            let k = format!("k{i}").into_bytes();
            i += 1;
            if let Some(e @ None) = by_slot.get_mut(slot_of(&k, cap - 1)) {
                *e = Some(k);
                left -= 1;
            }
        }
        let mut keys: Vec<Vec<u8>> = by_slot.into_iter().flatten().collect();
        assert_eq!(keys.len(), 1000);        let miss = loop {
            let k = format!("miss{i}").into_bytes();
            i += 1;
            if slot_of(&k, cap - 1) == 0 {
                break k;
            }
        };
        keys.sort();
        let pairs: Vec<(&[u8], &[u8])> = keys.iter().map(|k| (k.as_slice(), b"t".as_slice())).collect();
        let img = build(&pairs, false);
        let mo = Mo::parse(&img).unwrap();
        let ix = index(&mo).unwrap().unwrap();
        assert_eq!(u32::from_ne_bytes(ix[..4].try_into().unwrap()) as usize, IX_HASH, "the crafted table is kept");
        PROBES.with(|c| c.set(0));
        assert_eq!(mo.find_in(&ix, &miss), None);
        assert_eq!(mo.find(&miss), None);
        let walked = PROBES.with(std::cell::Cell::get);
        assert!(walked <= IX_PROBE_MAX, "a miss walked {walked} slots");
        for (j, k) in keys.iter().enumerate() {
            assert_eq!(mo.find_in(&ix, k), Some(j));
        }
    }
}
