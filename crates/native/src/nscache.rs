//! The integrated name-service cache (`liella_nscache_*`, D-16.10): switch it for the process
//! or per category, invalidate it, read its state.
//!
//! The cache keeps the answers of the built-in sources (`files`, `dns`) for hosts, passwd,
//! group (group lists included), services, protocols and networks, in a table of the process or
//! in the shared region of the effective user. It never changes an answer: a change of a watched
//! file shows at most about a second late (at once in a process started after it, or after
//! [`invalidate`]), and DNS data never outlives its TTL.

#![forbid(unsafe_code)]

use core::mem::{align_of, offset_of, size_of};

use crate::buf;
use crate::ffi;
use crate::Errno;

/// A category of the cache (`LIELLA_NSCACHE_*`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Category {
    /// Hosts (`gethostbyname*`, `gethostbyaddr`, `getaddrinfo`, `getnameinfo`).
    Hosts = 0,
    /// The passwd database by name and UID.
    Passwd = 1,
    /// The group database by name and GID, and group lists.
    Group = 2,
    /// Services by name and port.
    Services = 3,
    /// Protocols by name and number.
    Protocols = 4,
    /// Networks by name and number.
    Networks = 5,
}

impl Category {
    /// Every category.
    pub const ALL: [Category; 6] = [Category::Hosts, Category::Passwd, Category::Group, Category::Services, Category::Protocols, Category::Networks];

    /// The category's number (`LIELLA_NSCACHE_*`).
    pub fn number(self) -> u32 {
        match self {
            Category::Hosts => 0,
            Category::Passwd => 1,
            Category::Group => 2,
            Category::Services => 3,
            Category::Protocols => 4,
            Category::Networks => 5,
        }
    }
}

/// What table the cache uses (`LIELLA_NSCACHE_MODE_*`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    /// Disabled.
    Off,
    /// Enabled; no table yet.
    Idle,
    /// A table of this process only.
    Private,
    /// The effective user's shared region.
    Shared,
    /// Enabled, but no table could be made.
    Unavailable,
    /// A value this crate does not know (a newer library).
    Other(u32),
}

/// `struct liella_nscache_info` (64 bytes, alignment 8), as the C library fills it.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Status {
    /// 1 enabled, 0 disabled.
    pub enabled: u32,
    /// The enabled categories (bit `1 << number`).
    pub categories: u32,
    /// `LIELLA_NSCACHE_MODE_*` ([`Status::mode`]).
    pub mode: u32,
    /// Why the table is not the shared region: `LIELLA_NSCACHE_REASON_*` (0: it is).
    pub reason: u32,
    /// The shared region's user ID (`u64::MAX`: none).
    pub owner: u64,
    /// The sum of the table's generations: it moves with every invalidation.
    pub generation: u64,
    /// Lookups answered from the table.
    pub hits: u64,
    /// Cached lookups the sources answered.
    pub misses: u64,
    /// `stat` calls made to validate the watched files.
    pub stats: u64,
    /// Answers stored.
    pub stores: u64,
}

// The layout of `struct liella_nscache_info` (include/liella/native.h asserts the same).
const _: () = assert!(size_of::<Status>() == 64 && align_of::<Status>() == 8);
const _: () = assert!(offset_of!(Status, owner) == 16 && offset_of!(Status, stores) == 56);

impl Status {
    /// The table in use.
    pub fn mode(&self) -> Mode {
        match self.mode {
            0 => Mode::Off,
            1 => Mode::Idle,
            2 => Mode::Private,
            3 => Mode::Shared,
            4 => Mode::Unavailable,
            m => Mode::Other(m),
        }
    }

    /// Whether `c` is enabled.
    pub fn has(&self, c: Category) -> bool {
        self.categories & 1u32.checked_shl(c.number()).unwrap_or(0) != 0
    }
}

/// Enable or disable the cache for the process: whether it was enabled. Disabled, nothing is
/// mapped, `stat`ed or kept.
pub fn enable(on: bool) -> bool {
    ffi::liella_nscache_enable(u32::from(on)) == 1
}

/// Enable or disable one category: whether it was enabled.
pub fn set_category(c: Category, on: bool) -> Result<bool, Errno> {
    buf::value(ffi::liella_nscache_category(c.number(), u32::from(on))).map(|v| v == 1)
}

/// Invalidate one category (`Some`) or all of them, here and in the user's shared region; the
/// next lookup validates the watched files at once.
pub fn invalidate(c: Option<Category>) -> Result<(), Errno> {
    buf::done(ffi::liella_nscache_invalidate(c.map_or(u32::MAX, Category::number)))
}

/// The cache's state.
pub fn status() -> Result<Status, Errno> {
    let mut s = Status::default();
    buf::done(ffi::liella_nscache_status(&mut s)).map(|()| s)
}
