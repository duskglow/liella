//! A compiled locale category (spec Appendix D.1.1): a magic word, an item count, item offsets
//! and the item data. Every read is bounds-checked; [`Category::parse`] rejects a file whose
//! header or offsets do not fit (D-7.7).
#![allow(clippy::arithmetic_side_effects)] // offsets are u32 file values or slice lengths: sums of a few stay far below usize::MAX

/// The categories, by their file and `nl_item` numbers (Appendix D.1.1).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
pub enum Cat {
    /// `LC_CTYPE`.
    Ctype = 0,
    /// `LC_NUMERIC`.
    Numeric = 1,
    /// `LC_TIME`.
    Time = 2,
    /// `LC_COLLATE`.
    Collate = 3,
    /// `LC_MONETARY`.
    Monetary = 4,
    /// `LC_MESSAGES`.
    Messages = 5,
    /// `LC_PAPER`.
    Paper = 7,
    /// `LC_NAME`.
    Name = 8,
    /// `LC_ADDRESS`.
    Address = 9,
    /// `LC_TELEPHONE`.
    Telephone = 10,
    /// `LC_MEASUREMENT`.
    Measurement = 11,
    /// `LC_IDENTIFICATION`.
    Identification = 12,
}

impl Cat {
    /// The twelve categories in the order of composite names (Appendix D.1).
    pub const ALL: [Cat; 12] = [
        Cat::Ctype, Cat::Numeric, Cat::Time, Cat::Collate, Cat::Monetary, Cat::Messages,
        Cat::Paper, Cat::Name, Cat::Address, Cat::Telephone, Cat::Measurement, Cat::Identification,
    ];

    /// The C category number (`LC_CTYPE` = 0, ...).
    pub fn number(self) -> i32 {
        match self {
            Cat::Ctype => 0,
            Cat::Numeric => 1,
            Cat::Time => 2,
            Cat::Collate => 3,
            Cat::Monetary => 4,
            Cat::Messages => 5,
            Cat::Paper => 7,
            Cat::Name => 8,
            Cat::Address => 9,
            Cat::Telephone => 10,
            Cat::Measurement => 11,
            Cat::Identification => 12,
        }
    }

    /// The category with C number `n` (`LC_ALL` = 6 is not a category).
    #[inline]
    pub fn from_number(n: i32) -> Option<Cat> {
        match n {
            0..=5 => Cat::ALL.get(usize::try_from(n).ok()?).copied(),
            7..=12 => Cat::ALL.get(usize::try_from(n - 1).ok()?).copied(),
            _ => None,
        }
    }

    /// The position of this category in [`Cat::ALL`].
    #[inline]
    pub const fn index(self) -> usize {
        // The C numbers with `LC_ALL` (6) taken out.
        match self {
            Cat::Ctype => 0,
            Cat::Numeric => 1,
            Cat::Time => 2,
            Cat::Collate => 3,
            Cat::Monetary => 4,
            Cat::Messages => 5,
            Cat::Paper => 6,
            Cat::Name => 7,
            Cat::Address => 8,
            Cat::Telephone => 9,
            Cat::Measurement => 10,
            Cat::Identification => 11,
        }
    }

    /// `LC_CTYPE`, ..., as in composite names.
    pub fn name(self) -> &'static [u8] {
        match self {
            Cat::Ctype => b"LC_CTYPE",
            Cat::Numeric => b"LC_NUMERIC",
            Cat::Time => b"LC_TIME",
            Cat::Collate => b"LC_COLLATE",
            Cat::Monetary => b"LC_MONETARY",
            Cat::Messages => b"LC_MESSAGES",
            Cat::Paper => b"LC_PAPER",
            Cat::Name => b"LC_NAME",
            Cat::Address => b"LC_ADDRESS",
            Cat::Telephone => b"LC_TELEPHONE",
            Cat::Measurement => b"LC_MEASUREMENT",
            Cat::Identification => b"LC_IDENTIFICATION",
        }
    }

    /// The file holding this category in a locale directory.
    pub fn file_name(self) -> &'static [u8] {
        match self {
            Cat::Messages => b"LC_MESSAGES/SYS_LC_MESSAGES",
            other => other.name(),
        }
    }
}

/// A validated category file.
#[derive(Clone, Copy, Debug)]
pub struct Category {
    data: &'static [u8],
    n: usize,
}

pub(crate) fn u32_at(b: &[u8], off: usize) -> Option<u32> {
    let w: [u8; 4] = b.get(off..off.checked_add(4)?)?.try_into().ok()?;
    Some(u32::from_le_bytes(w))
}

pub(crate) fn i32_at(b: &[u8], off: usize) -> Option<i32> {
    u32_at(b, off).map(|v| i32::from_le_bytes(v.to_le_bytes()))
}

impl Category {
    /// Check a category file's header: the item count, and every item offset within the file
    /// and in non-decreasing order.
    pub fn parse(data: &'static [u8]) -> Option<Category> {
        let n = usize::try_from(u32_at(data, 4)?).ok()?;
        let table_end = n.checked_mul(4)?.checked_add(8)?;
        if table_end > data.len() {
            return None;
        }
        let mut prev = table_end;
        for i in 0..n {
            let off = usize::try_from(u32_at(data, 8 + 4 * i)?).ok()?;
            if off < prev || off > data.len() {
                return None;
            }
            prev = off;
        }
        Some(Category { data, n })
    }

    /// The whole file.
    pub fn bytes(&self) -> &'static [u8] {
        self.data
    }

    /// The magic word.
    pub fn magic(&self) -> u32 {
        u32_at(self.data, 0).unwrap_or(0)
    }

    /// The number of items.
    pub fn len(&self) -> usize {
        self.n
    }

    /// Whether the file has no items.
    pub fn is_empty(&self) -> bool {
        self.n == 0
    }

    /// The data of item `i` (empty if there is no such item).
    pub fn item(&self, i: usize) -> &'static [u8] {
        let data = self.data;
        if i >= self.n {
            return &[];
        }
        let start = u32_at(data, 8 + 4 * i).and_then(|v| usize::try_from(v).ok()).unwrap_or(0);
        let end = if i + 1 < self.n {
            u32_at(data, 12 + 4 * i).and_then(|v| usize::try_from(v).ok()).unwrap_or(start)
        } else {
            data.len()
        };
        data.get(start..end).unwrap_or(&[])
    }

    /// Item `i` as a `u32` word.
    pub fn word(&self, i: usize) -> u32 {
        u32_at(self.item(i), 0).unwrap_or(0)
    }

    /// Item `i` as a string, without its terminator.
    pub fn string(&self, i: usize) -> &'static [u8] {
        let s = self.item(i);
        let end = s.iter().position(|&b| b == 0).unwrap_or(s.len());
        s.get(..end).unwrap_or(&[])
    }
}
