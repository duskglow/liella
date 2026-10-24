//! Calendar conversion and time zones (spec §23.5–23.6; D-23.4 to D-23.6). Clocks, sleeping and
//! timers (§23.1–23.4) are translations in the POSIX layer and `liella_legacy::clock_compat`.
//!
//! * [`civil`]: the proleptic Gregorian calendar, `struct tm`'s fields as [`CivilTime`] (full
//!   year, month 1–12), normalisation of out-of-range [`Fields`], UTC (`gmtime`, `timegm`).
//! * [`zone`]: IP-23, the zone engine: a [`Zone`] from a TZif file ([`tzif`]) or a POSIX rule
//!   ([`rule`]), instant to local time and back.
//! * [`load`]: which zone a `TZ` value names (§23.6), read through the kernel value `S`.
//! * [`format`] and [`parse`]: `strftime`/`wcsftime` and `strptime` (§23.7; D-23.7), with the
//!   `LC_TIME` items of [`lctime`]. `getdate` (its template file and completion rules) is
//!   `liella_legacy::getdate`.
//!
//! **Selecting the zone** (§23.6). `TZ` unset: the file `/etc/localtime` (UTC if it cannot be
//! read). `TZ` empty, or only `:`: UTC, abbreviated `Universal` (as glibc). Otherwise, after one
//! leading `:` is dropped: the file it names (an absolute path, or a path under `TZDIR`, by
//! default `/usr/share/zoneinfo`), if that is a regular TZif file; else the value as a POSIX
//! rule; else UTC, abbreviated by the value's leading valid name (empty if none, as glibc).

pub mod civil;
pub mod fast;
pub mod format;
pub mod lctime;
pub mod parse;
pub mod rule;
pub mod tzif;
pub mod zone;

pub use civil::{difftime, dysize, timegm, utc, CivilTime, Fields};
pub use zone::{Globals, Local, Name, Period, Zone};

use crate::error::{Errno, Result};
use crate::mem::TryVec;
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Stat, O_CLOEXEC, O_RDONLY, S_IFMT, S_IFREG};

/// The system's default zone, used when `TZ` is unset.
pub const LOCALTIME: &[u8] = b"/etc/localtime\0";
/// The zone directory when `TZDIR` is unset.
pub const ZONEINFO: &[u8] = b"/usr/share/zoneinfo";
/// The largest zone file read (installed files are under 100 KiB).
const FILE_LIMIT: usize = 4 << 20;

/// The zone used when `TZ` is unset and the default file cannot be read.
pub static UTC: Zone = Zone::fixed(b"UTC\0", b"UTC\0");
/// The zone of an empty `TZ`.
pub static UNIVERSAL: Zone = Zone::fixed(b"Universal\0", b"\0");

/// A file's identity: what changes when it is replaced or rewritten (§23.6: the file is read
/// again when it changes).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct FileId {
    dev: u64,
    ino: u64,
    size: i64,
    mtime: (i64, i64),
    ctime: (i64, i64),
}

impl FileId {
    /// The identity `st` records.
    fn of(st: &Stat) -> FileId {
        FileId { dev: st.dev, ino: st.ino, size: st.size, mtime: (st.mtime.sec, st.mtime.nsec), ctime: (st.ctime.sec, st.ctime.nsec) }
    }
}

/// A selected zone: the zone read or parsed (`None`: the static one in `fixed`) and, for a zone
/// named as a file, the path (NUL-terminated) and the identity it had (`None` if it could not
/// be examined).
#[derive(Debug)]
pub struct Loaded {
    /// The zone, unless static.
    pub zone: Option<Zone>,
    /// The static zone when `zone` is `None`.
    pub fixed: &'static Zone,
    /// The file and its identity.
    pub file: Option<(TryVec<u8>, Option<FileId>)>,
}

/// The identity of the file at `path` (NUL-terminated, symbolic links followed), `None` if it
/// cannot be examined.
pub fn file_id<S: Sys>(sys: &S, path: &[u8]) -> Option<FileId> {
    let path = KStr::from_nul_terminated(path).ok()?;
    sys.fstatat(Fd::CWD, path, 0).ok().map(|st| FileId::of(&st))
}

/// Read the zone file at `path` (NUL-terminated): the zone if it is a regular TZif file, and
/// the identity of what was opened. A file that cannot be opened, examined or read is no zone;
/// only memory exhaustion fails.
fn read_zone<S: Sys>(sys: &S, path: &[u8]) -> Result<(Option<Zone>, Option<FileId>)> {
    let Ok(path) = KStr::from_nul_terminated(path) else { return Ok((None, None)) };
    let Ok(fd) = sys.openat(Fd::CWD, path, O_RDONLY | O_CLOEXEC, 0) else { return Ok((None, None)) };
    let read = read_open(sys, fd);
    // Read-only: nothing is lost if closing fails.
    let _ = sys.close(fd);
    read
}

/// [`read_zone`] on the open `fd`. The identity is the one of the file opened, so a file
/// replaced while it is read is noticed at the next check.
fn read_open<S: Sys>(sys: &S, fd: Fd) -> Result<(Option<Zone>, Option<FileId>)> {
    let Ok(st) = sys.fstat(fd) else { return Ok((None, None)) };
    let id = Some(FileId::of(&st));
    let size = usize::try_from(st.size).unwrap_or(usize::MAX);
    if st.mode & S_IFMT != S_IFREG || size > FILE_LIMIT {
        return Ok((None, id));
    }
    let Some(bytes) = read_all(sys, fd, size)? else { return Ok((None, id)) };
    match Zone::from_tzif(&bytes) {
        Ok(zone) => Ok((Some(zone), id)),
        Err(e) if e == Errno::ENOMEM => Err(e),
        Err(_) => Ok((None, id)),
    }
}

/// The contents of `fd` to its end, `hint` bytes expected; `None` if a read fails or the file
/// is (or has grown) larger than [`FILE_LIMIT`].
fn read_all<S: Sys>(sys: &S, fd: Fd, hint: usize) -> Result<Option<TryVec<u8>>> {
    let mut bytes = TryVec::new();
    // One byte beyond the expected size, so that the end of a file of the size fstat gave is
    // seen by the first read past it without a resize.
    bytes.resize(hint.saturating_add(1), 0)?;
    let mut len = 0usize;
    loop {
        if len == bytes.len() {
            if len > FILE_LIMIT {
                return Ok(None);
            }
            bytes.resize(len.saturating_mul(2).min(FILE_LIMIT.saturating_add(1)), 0)?;
        }
        let Some(spare) = bytes.get_mut(len..) else { return Ok(None) };
        match sys.read(fd, spare) {
            Ok(0) => break,
            Ok(n) => len = len.saturating_add(n),
            Err(e) if e == Errno::EINTR => {}
            Err(_) => return Ok(None),
        }
    }
    bytes.truncate(len);
    Ok(Some(bytes))
}

/// `value` as a NUL-terminated path: as given if absolute, else under `tzdir` (unset:
/// [`ZONEINFO`]).
fn zone_path(value: &[u8], tzdir: Option<&[u8]>) -> Result<TryVec<u8>> {
    let mut path = TryVec::new();
    if !value.starts_with(b"/") {
        path.extend_from_slice(tzdir.unwrap_or(ZONEINFO))?;
        path.push(b'/')?;
    }
    path.extend_from_slice(value)?;
    path.push(0)?;
    Ok(path)
}

/// The zone `TZ` names (§23.6; the module documentation): `tz` is `TZ`'s value (`None`:
/// unset), `tzdir` `TZDIR`'s, `default` the file for an unset `TZ` (NUL-terminated;
/// [`LOCALTIME`]). Only memory exhaustion fails.
pub fn load<S: Sys>(sys: &S, tz: Option<&[u8]>, tzdir: Option<&[u8]>, default: &[u8]) -> Result<Loaded> {
    let Some(tz) = tz else {
        // Unset: the default file, watched even when it cannot be read (it may appear later).
        let (zone, id) = read_zone(sys, default)?;
        let mut path = TryVec::new();
        path.extend_from_slice(default)?;
        return Ok(Loaded { zone, fixed: &UTC, file: Some((path, id)) });
    };
    let value = tz.strip_prefix(b":").unwrap_or(tz);
    if value.is_empty() {
        return Ok(Loaded { zone: None, fixed: &UNIVERSAL, file: None });
    }
    let path = zone_path(value, tzdir)?;
    if let (Some(zone), id) = read_zone(sys, &path)? {
        return Ok(Loaded { zone: Some(zone), fixed: &UTC, file: Some((path, id)) });
    }
    // Not a zone file: a rule (or UTC named by the value's leading name); not watched.
    Ok(Loaded { zone: Some(Zone::from_rule_text(value)?), fixed: &UTC, file: None })
}
