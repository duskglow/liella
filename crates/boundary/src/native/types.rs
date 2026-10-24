//! The structures of the native C ABI (D-X.10), each `#[repr(C)]` with its fields in an order
//! that needs no padding, so that the C declaration in `include/liella/native.h` (generated
//! from `spec/abi/native.tsv`) is the same object. Sizes, alignments and offsets are asserted
//! in the header and in `crates/boundary/tests/native_v1.rs`. Fields are never reordered,
//! retyped or removed: a later node that needs more defines a new structure.

use liella::error::{Errno, KResult};
use liella::time::{CivilTime, Fields, Local};
use liella_kernel::uapi;

/// `struct liella_timespec` (16 bytes, alignment 8): an instant as seconds since the epoch
/// and nanoseconds, or a reading of a clock.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct LiellaTimespec {
    /// Seconds.
    pub sec: i64,
    /// Nanoseconds, 0 to 999,999,999.
    pub nsec: i64,
}

impl From<uapi::Timespec> for LiellaTimespec {
    fn from(t: uapi::Timespec) -> Self {
        LiellaTimespec { sec: t.sec, nsec: t.nsec }
    }
}

/// `struct liella_stat` (120 bytes, alignment 8): a file's status.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct LiellaStat {
    /// The device holding the file.
    pub dev: u64,
    /// The inode number.
    pub ino: u64,
    /// The number of hard links.
    pub nlink: u64,
    /// The device a special file stands for.
    pub rdev: u64,
    /// The size in bytes.
    pub size: i64,
    /// The preferred I/O block size.
    pub blksize: i64,
    /// The 512-byte blocks allocated.
    pub blocks: i64,
    /// The last access.
    pub atime: LiellaTimespec,
    /// The last modification of the contents.
    pub mtime: LiellaTimespec,
    /// The last change of the status.
    pub ctime: LiellaTimespec,
    /// The type and permission bits (`LIELLA_S_IF*`, the kernel's values).
    pub mode: u32,
    /// The owner.
    pub uid: u32,
    /// The group.
    pub gid: u32,
    /// Always 0 (keeps the size a multiple of the alignment without implicit padding).
    pub reserved: u32,
}

impl From<uapi::Stat> for LiellaStat {
    fn from(s: uapi::Stat) -> Self {
        LiellaStat {
            dev: s.dev,
            ino: s.ino,
            nlink: s.nlink,
            rdev: s.rdev,
            size: s.size,
            blksize: s.blksize,
            blocks: s.blocks,
            atime: s.atime.into(),
            mtime: s.mtime.into(),
            ctime: s.ctime.into(),
            mode: s.mode,
            uid: s.uid,
            gid: s.gid,
            reserved: 0,
        }
    }
}

/// The capacity of [`LiellaTm::abbr`].
pub const ABBR_CAP: usize = 16;

/// `struct liella_tm` (64 bytes, alignment 8): a local time in the current zone.
///
/// As a result every field is set. As the input of `liella_tz_from_local`, `year` to
/// `second` and `isdst` are read (values outside their ranges carry, as `mktime`'s do) and the
/// rest is ignored.
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct LiellaTm {
    /// The full year (2026 is 2026; may be zero or negative).
    pub year: i64,
    /// 1 to 12.
    pub month: i32,
    /// 1 to 31.
    pub day: i32,
    /// 0 to 23.
    pub hour: i32,
    /// 0 to 59.
    pub minute: i32,
    /// 0 to 60 (60 only for an inserted leap second).
    pub second: i32,
    /// 0 to 6, Sunday = 0.
    pub weekday: i32,
    /// 0 to 365: days since 1 January.
    pub yearday: i32,
    /// Daylight time: 1 in effect, 0 not; as input, a negative value lets the zone decide.
    pub isdst: i32,
    /// Seconds east of UTC.
    pub utc_offset: i32,
    /// The abbreviation's full length; `abbr` holds it only if it is at most [`ABBR_CAP`].
    pub abbr_len: u32,
    /// The zone abbreviation (`"CEST"`), not NUL-terminated.
    pub abbr: [u8; ABBR_CAP],
}

impl LiellaTm {
    /// A local time and its abbreviation as the structure. An offset beyond `i32` (no zone has
    /// one: offsets are below a day) is `EOVERFLOW`.
    pub fn from_local(l: &Local, abbr: &[u8]) -> KResult<LiellaTm> {
        let c: &CivilTime = &l.civil;
        let mut tm = LiellaTm {
            year: c.year,
            month: i32::from(c.month),
            day: i32::from(c.day),
            hour: i32::from(c.hour),
            minute: i32::from(c.minute),
            second: i32::from(c.second),
            weekday: i32::from(c.weekday),
            yearday: i32::from(c.yearday),
            isdst: i32::from(l.isdst),
            utc_offset: i32::try_from(l.offset).map_err(|_| Errno::EOVERFLOW)?,
            abbr_len: u32::try_from(abbr.len()).map_err(|_| Errno::EOVERFLOW)?,
            abbr: [0; ABBR_CAP],
        };
        crate::conv::copy_out(abbr, &mut tm.abbr);
        Ok(tm)
    }

    /// The fields `liella_tz_from_local` reads.
    pub fn fields(&self) -> Fields {
        Fields {
            year: self.year,
            month: i64::from(self.month),
            day: i64::from(self.day),
            hour: i64::from(self.hour),
            minute: i64::from(self.minute),
            second: i64::from(self.second),
        }
    }
}

/// `struct liella_nscache_info` (64 bytes, alignment 8): the state of the process's
/// name-service cache (D-16.10).
#[repr(C)]
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct LiellaNscacheInfo {
    /// 1 enabled, 0 disabled.
    pub enabled: u32,
    /// The enabled categories (bit `1 << LIELLA_NSCACHE_<category>`).
    pub categories: u32,
    /// `LIELLA_NSCACHE_MODE_*`.
    pub mode: u32,
    /// `LIELLA_NSCACHE_REASON_*`.
    pub reason: u32,
    /// The shared region's user ID (`u64::MAX`: none).
    pub owner: u64,
    /// The sum of the table's generations (0: no table): it moves with every invalidation.
    pub generation: u64,
    /// Lookups answered from the table.
    pub hits: u64,
    /// Cached lookups the sources answered.
    pub misses: u64,
    /// `stat` calls made to validate.
    pub stats: u64,
    /// Answers stored.
    pub stores: u64,
}

impl From<liella_legacy::nscache::Status> for LiellaNscacheInfo {
    fn from(s: liella_legacy::nscache::Status) -> Self {
        LiellaNscacheInfo {
            enabled: u32::from(s.enabled),
            categories: s.categories,
            mode: s.mode,
            reason: s.reason,
            owner: s.owner,
            generation: s.generation,
            hits: s.hits,
            misses: s.misses,
            stats: s.stats,
            stores: s.stores,
        }
    }
}
