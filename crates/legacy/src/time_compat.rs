//! The process-wide time zone of `tzset` and the `tzname`/`timezone`/`daylight` globals, and
//! `asctime`'s fixed text (spec §23.5–23.6; D-23.5, D-23.6). The zone engine itself is
//! `liella::time` (IP-23); this module keeps which zone is the process's and publishes it.
//!
//! **Zones are kept.** Every zone the process selects is kept, with the `TZ` and `TZDIR` values
//! it was selected under (and, for a file, the file's identity), for the rest of the process:
//! `tm_zone` and `tzname` point into it, so they stay valid however `TZ` changes later. A
//! selection under values seen before reuses that zone, so a process switching between zones
//! does not grow.
//!
//! **When the zone is read again** (§23.6). An implicit `tzset` (`localtime`, `mktime`,
//! `ctime`, ...) compares `TZ` and `TZDIR` with those of the current zone, and for a zone read
//! from a file (`/etc/localtime` for an unset `TZ`, or the file `TZ` names) the file's identity
//! (device, inode, size, modification and change times, by `stat`) at most once per second
//! (`crate::recheck`, owner ruling 2026-10-05); any difference selects again. A changed `TZ` or
//! `TZDIR` (compared, never stat'ed) and the explicit `tzset` check at once. glibc checks the file
//! only for `/etc/localtime`; Liella checks every zone file the same way. The common case, an
//! unchanged environment and no check due, takes no lock.
//! The `_r` forms use the current zone without checking (as glibc), selecting one only when
//! none has been.
//!
//! **Invalidation** (D-X.13). [`invalidate`] advances a generation; a kept zone read from a file
//! is used again only after its file's identity has been compared once in the new generation
//! (unchanged: reused; changed or gone: the zone is read again). Nothing is freed, so old
//! `tm_zone` and `tzname` pointers stay valid.
//!
//! **The globals** are written when the selected zone changes, and by every explicit `tzset`.
//! They describe the zone (`liella::time::Globals`), never the period of the last conversion:
//! glibc's `localtime` and `mktime` also rewrite them from the period they convert in.

use core::sync::atomic::{AtomicI32, AtomicI64, AtomicUsize, Ordering::Acquire, Ordering::Release};

use liella::error::{Errno, KResult};
use liella::mem::{TryBox, TryVec};
use liella::time::{self, FileId, Zone};
use liella_kernel::sys::Sys;
use liella_rt::sync::RawMutex;

use crate::recheck::{self, LastCheck};

/// A selected zone and what it was selected under.
#[derive(Debug)]
pub struct Entry {
    loaded: time::Loaded,
    tz: Option<TryVec<u8>>,
    dir: Option<TryVec<u8>>,
    next: AtomicUsize,
    /// The last check of the zone's file (`recheck`).
    checked: LastCheck,
    /// The generation ([`invalidate`]) in which the file's identity was last checked.
    generation: AtomicUsize,
}

impl Entry {
    /// The zone.
    pub fn zone(&self) -> &Zone {
        self.loaded.zone.as_ref().unwrap_or(self.loaded.fixed)
    }
}

/// Address-to-reference conversion for the entries this module leaked (the POSIX layer).
pub trait ZoneObjects {
    /// The entry at `addr`, an address this module recorded.
    fn entry_at(&self, addr: usize) -> &'static Entry;
}

/// The global objects `tzset` writes: `tzname` (two `char *`), `timezone`, `daylight`.
#[derive(Clone, Copy)]
pub struct Globals<'a> {
    /// `tzname`.
    pub tzname: &'a [AtomicUsize; 2],
    /// `timezone`.
    pub timezone: &'a AtomicI64,
    /// `daylight`.
    pub daylight: &'a AtomicI32,
}

/// The environment a selection reads: `TZ` (`None`: unset) and, only when the selection has to
/// look further than "`TZ` is unchanged", `TZDIR`.
#[derive(Clone, Copy)]
pub struct Env<'a> {
    /// `TZ`.
    pub tz: Option<&'a [u8]>,
    /// `TZDIR` (`None`: unset or ignored), read only on a change of `TZ` and by the explicit
    /// `tzset` (TZ-FRESH: as glibc, a change of `TZDIR` alone is not seen by an implicit one).
    pub tzdir: &'a dyn Fn() -> Option<&'a [u8]>,
    /// The file for an unset `TZ` (NUL-terminated): `liella::time::LOCALTIME`.
    pub default: &'a [u8],
}

static LOCK: RawMutex = RawMutex::new();
static HEAD: AtomicUsize = AtomicUsize::new(0);
static CURRENT: AtomicUsize = AtomicUsize::new(0);
/// The invalidation generation (D-X.13).
static GENERATION: AtomicUsize = AtomicUsize::new(0);

/// `liella_tz_invalidate` (D-X.13): every kept zone read from a file is checked against its file
/// again before its next use, and read again if the file changed. The new generation.
pub fn invalidate() -> usize {
    GENERATION.fetch_add(1, Release).wrapping_add(1)
}

/// The lock serialising selections (held across `fork`).
pub fn lock() -> &'static RawMutex {
    &LOCK
}

fn addr_of<T: ?Sized>(r: &'static T) -> usize {
    core::ptr::from_ref(r).cast::<u8>().expose_provenance()
}

fn copy(v: Option<&[u8]>) -> KResult<Option<TryVec<u8>>> {
    v.map(TryVec::from_slice).transpose()
}

/// Write `z`'s globals.
pub fn publish(z: &'static Zone, g: &Globals<'_>) {
    let v = z.globals();
    for (slot, name) in g.tzname.iter().zip([v.std, v.dst]) {
        slot.store(addr_of(z.name_z(name)), Release);
    }
    g.timezone.store(v.timezone, Release);
    g.daylight.store(i32::from(v.daylight), Release);
}

/// Select the zone `env` names (an implicit `tzset`, or the explicit one with `always`): the
/// current one if nothing changed, else a kept one selected under the same values, else a new
/// one. The globals are written when the zone changes, and always with `always`. Without
/// memory for a new zone, the current one stays (UTC if there is none).
///
/// A zone file's identity (`/etc/localtime` for an unset `TZ`, or the file `TZ` names) is
/// checked (`stat`) by the explicit `tzset`, when the selection moves to another kept zone, and
/// for the current zone at most once per second (D-23.6; owner ruling 2026-10-05).
pub fn select<S: Sys>(sys: &S, objs: &impl ZoneObjects, env: Env<'_>, g: &Globals<'_>, always: bool) -> &'static Zone {
    let cur = CURRENT.load(Acquire);
    // The hot path, without the lock: the current zone, the same `TZ`, no check due.
    if !always && cur != 0 {
        let e = objs.entry_at(cur);
        if e.tz.as_deref() == env.tz && (e.loaded.file.is_none() || (e.generation.load(Acquire) == GENERATION.load(Acquire) && !e.checked.due(recheck::now(sys)))) {
            return e.zone();
        }
    }
    let now = recheck::now(sys);
    let _l = LOCK.lock(sys);
    let generation = GENERATION.load(Acquire);
    let cur = CURRENT.load(Acquire);
    let tzdir = (env.tzdir)();
    let mut stat: Option<Option<FileId>> = None;
    let mut fits = |e: &Entry| -> bool {
        let current = cur != 0 && core::ptr::eq(e, objs.entry_at(cur));
        // The current zone with an unchanged `TZ` stays whatever `TZDIR` is (glibc), except at
        // an explicit `tzset`; any other kept zone must match `TZDIR` as well.
        if e.tz.as_deref() != env.tz || (e.dir.as_deref() != tzdir && !(current && !always)) {
            return false;
        }
        match &e.loaded.file {
            Some((path, id)) => {
                let stale = e.generation.load(Acquire) != generation;
                if !always && !stale && current && !e.checked.due(now) {
                    return true;
                }
                let same = *stat.get_or_insert_with(|| time::file_id(sys, path)) == *id;
                if same {
                    e.checked.checked(now);
                    e.generation.store(generation, Release);
                }
                same
            }
            None => true,
        }
    };
    if cur != 0 && fits(objs.entry_at(cur)) {
        let z = objs.entry_at(cur).zone();
        if always {
            publish(z, g);
        }
        return z;
    }
    let mut at = HEAD.load(Acquire);
    while at != 0 {
        let e = objs.entry_at(at);
        if fits(e) {
            CURRENT.store(at, Release);
            publish(e.zone(), g);
            return e.zone();
        }
        at = e.next.load(Acquire);
    }
    match add(sys, env.tz, tzdir, env.default) {
        Ok(e) => {
            CURRENT.store(addr_of(e), Release);
            publish(e.zone(), g);
            e.zone()
        }
        Err(_) if cur != 0 => objs.entry_at(cur).zone(),
        Err(_) => &time::UTC,
    }
}

/// Load the zone `env` names and keep it. The caller holds [`LOCK`].
fn add<S: Sys>(sys: &S, tz: Option<&[u8]>, tzdir: Option<&[u8]>, default: &[u8]) -> KResult<&'static Entry> {
    let generation = AtomicUsize::new(GENERATION.load(Acquire));
    let loaded = time::load(sys, tz, tzdir, default)?;
    let checked = LastCheck::new();
    checked.checked(recheck::now(sys));
    let e = Entry { loaded, tz: copy(tz)?, dir: copy(tzdir)?, next: AtomicUsize::new(HEAD.load(Acquire)), checked, generation };
    let e: &'static Entry = TryBox::new(e)?.leak();
    HEAD.store(addr_of(e), Release);
    Ok(e)
}

/// The zone last selected, if any, without reading the environment (the `_r` forms' hot path:
/// AUD-M6, so their caller need not measure `TZ` and `TZDIR` only to discard them).
pub fn selected(objs: &impl ZoneObjects) -> Option<&'static Zone> {
    match CURRENT.load(Acquire) {
        0 => None,
        a => Some(objs.entry_at(a).zone()),
    }
}

/// The current zone without checking `TZ` (the `_r` forms), selecting one if none has been.
pub fn current<S: Sys>(sys: &S, objs: &impl ZoneObjects, env: Env<'_>, g: &Globals<'_>) -> &'static Zone {
    match CURRENT.load(Acquire) {
        0 => select(sys, objs, env, g, false),
        a => objs.entry_at(a).zone(),
    }
}

// ------------------------------------------------------------------------------ asctime

/// The length of `asctime`'s text with its NUL: the fixed 26 bytes of the C standard's
/// `"%.3s %.3s%3d %.2d:%.2d:%.2d %d\n"`.
pub const ASCTIME_LEN: usize = 26;

const DAYS: [&[u8; 3]; 7] = [b"Sun", b"Mon", b"Tue", b"Wed", b"Thu", b"Fri", b"Sat"];
const MONTHS: [&[u8; 3]; 12] = [b"Jan", b"Feb", b"Mar", b"Apr", b"May", b"Jun", b"Jul", b"Aug", b"Sep", b"Oct", b"Nov", b"Dec"];

/// The `struct tm` fields `asctime` prints (as given, not normalised).
#[derive(Clone, Copy, Debug, Default)]
pub struct AscFields {
    /// `tm_wday`.
    pub wday: i32,
    /// `tm_mon`.
    pub mon: i32,
    /// `tm_mday`.
    pub mday: i32,
    /// `tm_hour`.
    pub hour: i32,
    /// `tm_min`.
    pub min: i32,
    /// `tm_sec`.
    pub sec: i32,
    /// `tm_year`.
    pub year: i32,
}

/// A small output buffer for the text.
struct Out {
    b: [u8; 64],
    n: usize,
}

impl Out {
    fn put(&mut self, s: &[u8]) {
        for &c in s {
            if let Some(slot) = self.b.get_mut(self.n) {
                *slot = c;
            }
            self.n = self.n.saturating_add(1);
        }
    }

    /// `v` in decimal, at least `digits` digits (zero-padded after any sign), right-aligned in
    /// `width` columns.
    fn int(&mut self, v: i64, digits: usize, width: usize) {
        let mut d = [0u8; 20];
        let mut k = d.len();
        let mut m = v.unsigned_abs();
        loop {
            k = k.saturating_sub(1);
            if let Some(slot) = d.get_mut(k) {
                *slot = b'0'.saturating_add(u8::try_from(m % 10).unwrap_or(0));
            }
            m /= 10;
            if m == 0 {
                break;
            }
        }
        let digs = d.get(k..).unwrap_or(&[]);
        let zeros = digits.saturating_sub(digs.len());
        let len = digs.len().saturating_add(zeros).saturating_add(usize::from(v < 0));
        for _ in len..width {
            self.put(b" ");
        }
        if v < 0 {
            self.put(b"-");
        }
        for _ in 0..zeros {
            self.put(b"0");
        }
        self.put(digs);
    }
}

/// `asctime`'s text (`"Sun Sep 16 01:03:52 1973\n"`, without the NUL): names outside their
/// range print as `???`. `EOVERFLOW` when the text and its NUL do not fit the fixed 26 bytes
/// (spec §23.5's ARB: years 0 to 9999 with in-range fields fit, and so do years down to -999,
/// as in glibc's `asctime_r`; out-of-range fields that widen the text fail for `asctime` too,
/// where glibc prints them into a larger static buffer).
pub fn asctime(f: &AscFields) -> KResult<([u8; ASCTIME_LEN], usize)> {
    let name = |table: &[&'static [u8; 3]], i: i32| -> &'static [u8] {
        usize::try_from(i).ok().and_then(|i| table.get(i)).map_or(b"???", |s| &s[..])
    };
    let mut o = Out { b: [0; 64], n: 0 };
    o.put(name(&DAYS, f.wday));
    o.put(b" ");
    o.put(name(&MONTHS, f.mon));
    o.int(i64::from(f.mday), 1, 3);
    o.put(b" ");
    o.int(i64::from(f.hour), 2, 0);
    o.put(b":");
    o.int(i64::from(f.min), 2, 0);
    o.put(b":");
    o.int(i64::from(f.sec), 2, 0);
    o.put(b" ");
    o.int(i64::from(f.year).saturating_add(1900), 1, 0);
    o.put(b"\n");
    if o.n >= ASCTIME_LEN {
        return Err(Errno::EOVERFLOW);
    }
    let mut out = [0u8; ASCTIME_LEN];
    for (d, s) in out.iter_mut().zip(o.b.iter().take(o.n)) {
        *d = *s;
    }
    Ok((out, o.n))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions)]
mod tests {
    use super::*;

    fn text(f: AscFields) -> Option<std::string::String> {
        asctime(&f).ok().map(|(b, n)| std::string::String::from_utf8_lossy(&b[..n]).into_owned())
    }

    #[test]
    fn layout() {
        let f = AscFields { wday: 0, mon: 8, mday: 16, hour: 1, min: 3, sec: 52, year: 73 };
        assert_eq!(text(f).as_deref(), Some("Sun Sep 16 01:03:52 1973\n"));
        assert_eq!(text(AscFields { year: -1900, ..f }).as_deref(), Some("Sun Sep 16 01:03:52 0\n"));
        assert_eq!(text(AscFields { year: -2899, ..f }).as_deref(), Some("Sun Sep 16 01:03:52 -999\n"));
        assert_eq!(text(AscFields { year: -2900, ..f }), None);
        assert_eq!(text(AscFields { year: 8100, ..f }), None);
        assert_eq!(text(AscFields { wday: 7, mon: 12, ..f }).as_deref(), Some("??? ??? 16 01:03:52 1973\n"));
        assert_eq!(text(AscFields { mday: 123, hour: 10, ..f }).as_deref(), Some("Sun Sep123 10:03:52 1973\n"));
        assert_eq!(text(AscFields { mday: 123, hour: 100, ..f }), None);
        assert_eq!(text(AscFields { mday: 1, ..f }).as_deref(), Some("Sun Sep  1 01:03:52 1973\n"));
    }
}
