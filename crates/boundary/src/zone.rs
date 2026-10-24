//! Time-zone glue (spec §23.5–23.6; D-23.4 to D-23.6): the process's zone through
//! `liella_legacy::time_compat` with `TZ` and `TZDIR` from the environment, `struct tm`
//! results (packed by `liella_abi::pack`), and the library-owned results of `gmtime`,
//! `localtime`, `asctime` and `ctime` in the calling thread's result slots (D-H3, D-23.5).
//!
//! Which functions select the zone again (an implicit `tzset`): `localtime`, `mktime`
//! (`timelocal`) and `ctime`, as glibc (§23.6 names `localtime`, `mktime` and `strftime`). The
//! `_r` forms (`localtime_r`, `ctime_r`) use the zone last selected (as glibc), selecting one
//! only if none has been.
//!
//! Every `tzset`, implicit or explicit, reads `TZ` and `TZDIR` from the `environ` array (TZ-FRESH,
//! D-23.6); the selection compares the values' text with the current zone's and parses again only
//! on a change, so any change to the environment is seen, as in glibc.

use core::sync::atomic::{compiler_fence, Ordering::{Relaxed, Release}};

use liella::error::{Errno, KResult};
use liella::time::{self, Zone};
use liella_abi::layout::struct_tm;
use liella_abi::pack;
use liella_legacy::time_compat::{self as tc, AscFields, Env, Entry, ZoneObjects};
use liella_rt::thread::{SLOT_ASCTIME, SLOT_TM};
use liella_rt::tls::CACHE_TZ_PERIOD;

use crate::sys::CSys;
use crate::{data, process, strz, tb, thread};

/// `tm_zone` of UTC results (`gmtime`, `timegm`).
static GMT: &[u8] = b"GMT\0";

struct Glue;

impl ZoneObjects for Glue {
    fn entry_at(&self, addr: usize) -> &'static Entry {
        tb::zone_entry_at(addr)
    }
}

fn value(addr: usize) -> Option<&'static [u8]> {
    (addr != 0).then(|| strz::env_value(addr))
}

/// `TZ` from the environment now, and `TZDIR` (ignored in secure mode) on demand.
///
/// TZ-FRESH: every implicit `tzset` scans `environ` again for `TZ` (the shared fast scan,
/// ENV-SCAN) and the selection compares the value's bytes with the current zone's, so a write
/// into the `environ` array or an edit of the string in place is seen at once, as glibc sees it.
/// There is no cache keyed on a change counter or the array's address. `TZDIR` is scanned only
/// when `TZ` changed (glibc reads it only when it loads a zone file) and by the explicit `tzset`.
fn env() -> Env<'static> {
    let tz = process::getenv(b"TZ").unwrap_or(0);
    Env { tz: value(tz), tzdir: &tzdir, default: time::LOCALTIME }
}

fn tzdir() -> Option<&'static [u8]> {
    value(process::secure_getenv(b"TZDIR").unwrap_or(0))
}

/// `tzset`: select the zone `TZ` names and write the globals.
pub fn tzset() {
    tc::select(&CSys, &Glue, env(), &data::tz_globals(), true);
}

/// `liella_tz_invalidate` (D-X.13): every kept zone read from a file is checked against its file
/// before its next use (a changed one is read again); then the zone `TZ` names is selected, as
/// by `tzset`. It cannot fail.
pub fn invalidate() -> KResult<()> {
    tc::invalidate();
    tzset();
    Ok(())
}

/// An implicit `tzset`: the zone `TZ` names now.
pub(crate) fn implicit() -> &'static Zone {
    tc::select(&CSys, &Glue, env(), &data::tz_globals(), false)
}

/// The zone last selected.
fn current() -> &'static Zone {
    tc::selected(&Glue).unwrap_or_else(|| tc::current(&CSys, &Glue, env(), &data::tz_globals()))
}

pub(crate) fn addr(b: &[u8]) -> usize {
    b.as_ptr().expose_provenance()
}

/// `gmtime`'s conversion.
pub fn gmtime(t: i64) -> KResult<struct_tm> {
    let c = time::utc(t)?;
    Ok(pack::tm(&c, false, 0, addr(GMT)))
}

/// Run `f` with the calling thread's [`time::Period`] (its cache words, TB-SLOTS: three
/// `%fs`-relative loads, and stores only when `f` changed it).
fn with_period<R>(f: impl FnOnce(&mut time::Period) -> R) -> R {
    let w = |v: usize| u64::try_from(v).unwrap_or(0);
    let before = time::Period::from_words(&[w(tb::read::<tb::TzPeriod0>()), w(tb::read::<tb::TzPeriod1>()), w(tb::read::<tb::TzPeriod2>())]);
    let mut p = before;
    let r = f(&mut p);
    if p != before {
        let [a, lo, hi] = p.to_words().map(|x| usize::try_from(x).unwrap_or(0));
        if let Some([c0, c1, c2]) = tb::current().cache.get(CACHE_TZ_PERIOD..).and_then(|c| c.first_chunk::<{ time::Period::WORDS }>()) {
            // The zone number last: a `localtime` in a signal handler that interrupts the
            // stores finds number 0 (no period) or a whole period, never a mix of two.
            c0.store(0, Relaxed);
            compiler_fence(Release);
            c1.store(lo, Relaxed);
            c2.store(hi, Relaxed);
            compiler_fence(Release);
            c0.store(a, Relaxed);
        }
    }
    r
}

fn local_in(z: &'static Zone, t: i64) -> KResult<struct_tm> {
    let l = if z.uses_period(t) { with_period(|p| z.at_cached(t, p)) } else { z.at(t) }?;
    Ok(pack::tm(&l.civil, l.isdst, l.offset, addr(z.name_z(l.name))))
}

/// `localtime`'s conversion (an implicit `tzset`).
pub fn localtime(t: i64) -> KResult<struct_tm> {
    local_in(implicit(), t)
}

/// `localtime_r`'s conversion (the zone last selected).
pub fn localtime_r(t: i64) -> KResult<struct_tm> {
    local_in(current(), t)
}

/// `mktime`: the instant of `tm` in the local zone (an implicit `tzset`); `tm` is normalised
/// on success and left alone on failure.
pub fn mktime(tm: &mut struct_tm) -> KResult<i64> {
    let z = implicit();
    let f = pack::fields(tm);
    let (t, l) = if z.resolve_uses_period(&f) { with_period(|p| z.resolve_cached(&f, tm.tm_isdst, p)) } else { z.resolve(&f, tm.tm_isdst) }?;
    *tm = pack::tm(&l.civil, l.isdst, l.offset, addr(z.name_z(l.name)));
    Ok(t)
}

/// `timegm`: the instant of `tm` read as UTC; `tm` is normalised on success.
pub fn timegm(tm: &mut struct_tm) -> KResult<i64> {
    let (t, c) = time::timegm(&pack::fields(tm))?;
    *tm = pack::tm(&c, false, 0, addr(GMT));
    Ok(t)
}

/// The local time of `t` in the zone `TZ` names (an implicit `tzset`, as `localtime`) and its
/// abbreviation: the native interface's conversion (D-X.10), with the full-width year of the
/// clean API rather than `struct tm`'s.
pub fn local_at(t: i64) -> KResult<(time::Local, &'static [u8])> {
    let z = implicit();
    let l = z.at(t)?;
    Ok((l, z.name(l.name)))
}

/// The instant of the local time `f` in that zone (`isdst` as `mktime`'s `tm_isdst`), with
/// its normalised local time and abbreviation: the native interface's reverse conversion.
pub fn resolve(f: &time::Fields, isdst: i32) -> KResult<(i64, time::Local, &'static [u8])> {
    let z = implicit();
    let (t, l) = z.resolve(f, isdst)?;
    Ok((t, l, z.name(l.name)))
}

/// Store a result in the caller's `struct tm`: its address.
pub fn store_tm(out: Option<&mut struct_tm>, r: KResult<struct_tm>) -> KResult<usize> {
    let out = out.ok_or(Errno::EFAULT)?;
    *out = r?;
    Ok(core::ptr::from_mut(out).expose_provenance())
}

/// Store a result in the calling thread's `struct tm` that `gmtime` and `localtime` share: its
/// address; `ENOMEM` for a thread without result slots (D-C0.2).
pub fn static_tm(tm: struct_tm) -> KResult<usize> {
    let buf = thread::result_area::<64>(SLOT_TM).ok_or(Errno::ENOMEM)?;
    buf.store(&pack::tm_bytes(&tm));
    Ok(buf.addr())
}

fn asc_fields(tm: &struct_tm) -> AscFields {
    AscFields { wday: tm.tm_wday, mon: tm.tm_mon, mday: tm.tm_mday, hour: tm.tm_hour, min: tm.tm_min, sec: tm.tm_sec, year: tm.tm_year }
}

/// `asctime_r`: the text and its NUL in `buf` (the caller's 26 bytes).
pub fn asctime_into(tm: &struct_tm, buf: &mut [u8]) -> KResult<()> {
    let (text, n) = tc::asctime(&asc_fields(tm))?;
    // The text array holds zeros after the text, so `..=n` ends with the NUL.
    let src = text.get(..=n).ok_or(Errno::EOVERFLOW)?;
    buf.get_mut(..=n).ok_or(Errno::EFAULT)?.copy_from_slice(src);
    Ok(())
}

/// `asctime`: the text in the buffer `asctime` and `ctime` share: its address.
pub fn asctime_static(tm: &struct_tm) -> KResult<usize> {
    let (text, n) = tc::asctime(&asc_fields(tm))?;
    let buf = thread::result_slot(SLOT_ASCTIME).ok_or(Errno::ENOMEM)?;
    buf.store(text.get(..=n).unwrap_or(&[0]));
    Ok(buf.addr())
}

/// `ctime(t)`: `asctime(localtime(t))`, both static results written.
pub fn ctime(t: i64) -> KResult<usize> {
    let tm = localtime(t)?;
    static_tm(tm)?;
    asctime_static(&tm)
}

/// `ctime_r(t, buf)`: `asctime_r(localtime_r(t), buf)`.
pub fn ctime_r(t: i64, buf: &mut [u8]) -> KResult<()> {
    asctime_into(&localtime_r(t)?, buf)
}
