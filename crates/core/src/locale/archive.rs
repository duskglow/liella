//! The locale archive (spec Appendix D.1.6): a name table and, per locale, the file image of
//! each category.

use super::file::{u32_at, Cat};

const MAGIC: u32 = 0xde02_0109;

/// A locale archive.
#[derive(Clone, Copy, Debug)]
pub struct Archive {
    data: &'static [u8],
    namehash: usize,
    namehash_size: usize,
}

impl Archive {
    /// Check the header.
    pub fn parse(data: &'static [u8]) -> Option<Archive> {
        if u32_at(data, 0)? != MAGIC {
            return None;
        }
        let namehash = usize::try_from(u32_at(data, 8)?).ok()?;
        let namehash_size = usize::try_from(u32_at(data, 16)?).ok()?;
        let end = namehash.checked_add(namehash_size.checked_mul(12)?)?;
        (end <= data.len()).then_some(Archive { data, namehash, namehash_size })
    }

    /// The category file images of the locale named exactly `name` (the archive stores names
    /// with normalized codesets, [`super::name::archive_form`]).
    ///
    /// The name table is scanned (D.1.6), four entries at a time: a group whose four
    /// `name_offset`s are all 0 (unused) is skipped with one test, and a used entry is rejected
    /// at its first differing byte without measuring the stored name (SETLOCALE-5: the table of
    /// a typical archive has hundreds of slots and a handful of names; the scan cost ~21
    /// instructions per slot, 11.5k for `en_US.UTF-8`). An entry whose name lies outside the
    /// file is skipped.
    pub fn lookup(&self, name: &[u8]) -> Option<Record> {
        if name.contains(&0) {
            return None;
        }
        let table = self.data.get(self.namehash..self.namehash.checked_add(self.namehash_size.checked_mul(12)?)?)?;
        let mut groups = table.chunks_exact(48);
        for g in groups.by_ref() {
            let used = name_offset(g, 0) | name_offset(g, 1) | name_offset(g, 2) | name_offset(g, 3);
            if used == 0 {
                continue;
            }
            if let Some(r) = g.chunks_exact(12).find_map(|e| self.entry(e, name)) {
                return Some(r);
            }
        }
        groups.remainder().chunks_exact(12).find_map(|e| self.entry(e, name))
    }

    /// The record of the name table entry `e` when it is used and names exactly `name` (which
    /// has no NUL).
    fn entry(&self, e: &[u8], name: &[u8]) -> Option<Record> {
        let off = usize::try_from(name_offset(e, 0)).ok()?;
        if off == 0 {
            return None;
        }
        let end = off.checked_add(name.len())?;
        if self.data.get(off..end)? != name || self.data.get(end) != Some(&0) {
            return None;
        }
        let rec = usize::try_from(u32_at(e, 8)?).ok()?;
        Some(Record { data: self.data, at: rec })
    }
}

/// The `name_offset` of entry `k` of the name table entries `g` (0 when out of range).
fn name_offset(g: &[u8], k: usize) -> u32 {
    k.checked_mul(12).and_then(|at| u32_at(g, at.checked_add(4)?)).unwrap_or(0)
}

/// One archived locale.
#[derive(Clone, Copy, Debug)]
pub struct Record {
    data: &'static [u8],
    at: usize,
}

impl Record {
    /// The file image of category `cat`.
    pub fn category(&self, cat: Cat) -> Option<&'static [u8]> {
        let slot = usize::try_from(cat.number()).ok()?;
        let at = self.at.checked_add(4)?.checked_add(slot.checked_mul(8)?)?;
        let off = usize::try_from(u32_at(self.data, at)?).ok()?;
        let len = usize::try_from(u32_at(self.data, at.checked_add(4)?)?).ok()?;
        self.data.get(off..off.checked_add(len)?)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::as_conversions, clippy::arithmetic_side_effects)]
mod tests {
    use super::*;

    /// An archive image with a name table of `size` slots; `names` puts a name in a slot, with a
    /// record whose `LC_CTYPE` slot (offset, len) is (slot, 1) so a hit tells which entry
    /// answered. A name of `None` puts a name offset past the end of the file in the slot.
    fn image(size: usize, names: &[(usize, Option<&[u8]>)]) -> &'static [u8] {
        let namehash = 64;
        let mut d = vec![0u8; namehash + size * 12];
        d[0..4].copy_from_slice(&MAGIC.to_le_bytes());
        d[8..12].copy_from_slice(&(namehash as u32).to_le_bytes());
        d[16..20].copy_from_slice(&(size as u32).to_le_bytes());
        for &(slot, name) in names {
            let at = namehash + slot * 12;
            let off = match name {
                Some(n) => {
                    let off = d.len();
                    d.extend_from_slice(n);
                    d.push(0);
                    off as u32
                }
                None => u32::MAX,
            };
            let rec = d.len();
            d.extend_from_slice(&[0; 4 + 13 * 8]);
            let ctype = rec + 4 + 8 * Cat::Ctype.number() as usize;
            d[ctype..ctype + 4].copy_from_slice(&(slot as u32).to_le_bytes());
            d[ctype + 4..ctype + 8].copy_from_slice(&1u32.to_le_bytes());
            d[at + 4..at + 8].copy_from_slice(&off.to_le_bytes());
            d[at + 8..at + 12].copy_from_slice(&(rec as u32).to_le_bytes());
        }
        Box::leak(d.into_boxed_slice())
    }

    /// The slot that answered `name`, through the record's `LC_CTYPE` offset.
    fn found(a: &Archive, name: &[u8]) -> Option<usize> {
        a.lookup(name).map(|r| {
            let at = r.at + 4 + 8 * Cat::Ctype.number() as usize;
            u32_at(r.data, at).unwrap() as usize
        })
    }

    #[test]
    fn lookup_finds_names_in_every_group_position_and_the_remainder() {
        // 19 slots: four groups of four, each with one name at a different position, then a
        // remainder of three.
        let names: [(usize, Option<&[u8]>); 6] = [
            (0, Some(b"aa_AA.utf8")),
            (5, Some(b"en")),
            (10, Some(b"en_US.utf8")),
            (15, Some(b"de_DE.utf8@euro")),
            (16, Some(b"fr_FR")),
            (18, Some(b"zz_ZZ")),
        ];
        let a = Archive::parse(image(19, &names)).unwrap();
        for (slot, n) in names {
            assert_eq!(found(&a, n.unwrap()), Some(slot), "{}", String::from_utf8_lossy(n.unwrap()));
        }
        for miss in [&b"e"[..], b"en_", b"en_US", b"en_US.utf8x", b"zz_ZZ.", b"", b"fr_FR\0", b"en\0"] {
            assert_eq!(found(&a, miss), None, "{}", String::from_utf8_lossy(miss));
        }
    }

    #[test]
    fn lookup_skips_a_name_outside_the_file_and_finds_a_later_one() {
        for size in [1usize, 2, 4, 5, 9] {
            let mut names = vec![(0, None)];
            if size > 1 {
                names.push((size - 1, Some(&b"en_US.utf8"[..])));
            }
            let a = Archive::parse(image(size, &names)).unwrap();
            assert_eq!(found(&a, b"en_US.utf8"), (size > 1).then_some(size - 1), "size {size}");
        }
    }

    #[test]
    fn lookup_in_an_empty_table_finds_nothing() {
        let a = Archive::parse(image(0, &[])).unwrap();
        assert_eq!(found(&a, b"C"), None);
        let a = Archive::parse(image(4, &[])).unwrap();
        assert_eq!(found(&a, b"C"), None);
    }
}
