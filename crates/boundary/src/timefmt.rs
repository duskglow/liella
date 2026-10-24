//! Formatting and parsing time (spec §23.7; D-23.7): the glue between the C entry points and
//! the engines `liella::time::format` (`strftime`, `wcsftime`), `liella::time::parse`
//! (`strptime`) and `liella_legacy::getdate`.
//!
//! * The process's zone is reached as an implicit `tzset` would (§23.6 names `strftime` among
//!   the functions that call it), and only by the directives that need it: `%Z` with a NULL or
//!   empty `tm_zone`, `%s`, `strptime`'s `%s`, and `getdate`.
//! * `strftime`'s full-buffer result (`Written::Truncated`) is 0, as is an empty result (⚑ ARB,
//!   D-23.7); NULL arguments are 0 with `EFAULT` (S6).
//! * `strptime` sets only the fields it determined; a mismatch is NULL without `errno`, a NULL
//!   argument NULL with `EFAULT`.
//! * `getdate` stores its `struct tm` in the calling thread's result slots (`SLOT_GETDATE`, D-H3)
//!   and its error in `getdate_err`; `getdate_r` returns the error and writes only on success.

use liella::error::{Errno, KResult};
use liella::locale::view::LocaleView;
use liella::locale::Locale;
use liella::time::fast;
use liella::time::format::{self, Tm, Written};
use liella::time::parse::{self, Local, Options};
use liella::time::{Fields, Zone};
use liella_abi::layout::struct_tm;
use liella_abi::pack;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::CLOCK_REALTIME;
use liella_legacy::getdate;
use liella_rt::thread::SLOT_GETDATE;

use crate::sys::CSys;
use crate::{data, process, strz, thread, zone};

/// The process's zone, selected on first use.
struct Process {
    zone: core::cell::OnceCell<&'static Zone>,
}

impl Process {
    fn new() -> Process {
        Process { zone: core::cell::OnceCell::new() }
    }

    fn zone(&self) -> &'static Zone {
        self.zone.get_or_init(zone::implicit)
    }

    fn local_of(&self, z: &'static Zone, t: i64) -> KResult<Local<usize>> {
        let l = z.at(t)?;
        Ok(Local { civil: l.civil, isdst: l.isdst, offset: l.offset, zone: zone::addr(z.name_z(l.name)) })
    }
}

impl format::Env for Process {
    fn zone_name(&self, dst: bool) -> &[u8] {
        let z = self.zone();
        let g = z.globals();
        z.name(if dst { g.dst } else { g.std })
    }

    fn seconds(&self, tm: &Tm<'_>) -> i128 {
        let f = Fields {
            year: tm.full_year(),
            month: i64::from(tm.mon).saturating_add(1),
            day: i64::from(tm.mday),
            hour: i64::from(tm.hour),
            minute: i64::from(tm.min),
            second: i64::from(tm.sec),
        };
        let z = self.zone();
        match z.resolve(&f, tm.isdst) {
            Ok((t, _)) => i128::from(t),
            // Beyond time_t: the exact count, at the zone's standard offset.
            Err(_) => f.seconds().saturating_add(i128::from(z.globals().timezone)),
        }
    }
}

impl parse::Env for Process {
    type Zone = usize;

    fn local(&self, t: i64) -> KResult<Local<usize>> {
        self.local_of(self.zone(), t)
    }
}

impl getdate::Env for Process {
    fn now(&self) -> KResult<i64> {
        Ok(CSys.clock_gettime(CLOCK_REALTIME)?.sec)
    }

    fn mktime(&self, f: &Fields, isdst: i32) -> KResult<Local<usize>> {
        let z = self.zone();
        let (_, l) = z.resolve(f, isdst)?;
        Ok(Local { civil: l.civil, isdst: l.isdst, offset: l.offset, zone: zone::addr(z.name_z(l.name)) })
    }
}

fn tm_of(t: &struct_tm) -> Tm<'_> {
    Tm {
        sec: t.tm_sec,
        min: t.tm_min,
        hour: t.tm_hour,
        mday: t.tm_mday,
        mon: t.tm_mon,
        year: t.tm_year,
        wday: t.tm_wday,
        yday: t.tm_yday,
        isdst: t.tm_isdst,
        gmtoff: t.tm_gmtoff,
        zone: strz::tm_zone(t.tm_zone),
    }
}

fn length(w: KResult<Written>) -> KResult<usize> {
    match w? {
        Written::Fits(n) => Ok(n),
        Written::Truncated { .. } => Ok(0),
    }
}

/// `strftime(s, max, fmt, tm)` in `loc`: the length, or 0 (does not fit, or empty).
pub fn strftime(buf: KResult<&mut [u8]>, fmt: KResult<&[u8]>, tm: Option<&struct_tm>, view: &LocaleView) -> KResult<usize> {
    let (buf, fmt, tm) = (buf?, fmt?, tm.ok_or(Errno::EFAULT)?);
    // The fast lane takes the plain cases and leaves the buffer as the engine would, else None.
    if let Some(n) = fast::strftime(buf, fmt, &Raw(tm), view) {
        return Ok(n);
    }
    length(format::strftime(buf, fmt, &tm_of(tm), view.locale(), &Process::new()))
}

/// The caller's `struct tm`, read in place by the fast lane (`tm_zone` only for `%Z`).
struct Raw<'a>(&'a struct_tm);

impl fast::TmRead for Raw<'_> {
    #[inline]
    fn sec(&self) -> i32 {
        self.0.tm_sec
    }
    #[inline]
    fn min(&self) -> i32 {
        self.0.tm_min
    }
    #[inline]
    fn hour(&self) -> i32 {
        self.0.tm_hour
    }
    #[inline]
    fn mday(&self) -> i32 {
        self.0.tm_mday
    }
    #[inline]
    fn mon(&self) -> i32 {
        self.0.tm_mon
    }
    #[inline]
    fn year(&self) -> i32 {
        self.0.tm_year
    }
    #[inline]
    fn wday(&self) -> i32 {
        self.0.tm_wday
    }
    #[inline]
    fn yday(&self) -> i32 {
        self.0.tm_yday
    }
    #[inline]
    fn isdst(&self) -> i32 {
        self.0.tm_isdst
    }
    #[inline]
    fn gmtoff(&self) -> i64 {
        self.0.tm_gmtoff
    }
    fn zone(&self) -> Option<&[u8]> {
        strz::tm_zone(self.0.tm_zone)
    }
}

/// `wcsftime(s, max, fmt, tm)` in `loc`: the length in wide characters, or 0.
pub fn wcsftime(buf: KResult<&mut [i32]>, fmt: KResult<&[i32]>, tm: Option<&struct_tm>, loc: &Locale) -> KResult<usize> {
    let (buf, fmt, tm) = (buf?, fmt?, tm.ok_or(Errno::EFAULT)?);
    length(format::strftime(buf, fmt, &tm_of(tm), loc, &Process::new()))
}

/// `strptime(s, fmt, tm)` in `loc`, where `s` is at `base`: the address after the parsed text,
/// or 0 if the text does not match; the fields it determined are stored in `tm`.
pub fn strptime(base: usize, s: KResult<&[u8]>, fmt: KResult<&[u8]>, tm: Option<&mut struct_tm>, loc: &Locale) -> KResult<usize> {
    let (s, fmt, tm) = (s?, fmt?, tm.ok_or(Errno::EFAULT)?);
    let Some((n, p)) = parse::strptime(s, fmt, loc, &Process::new(), Options::default()) else { return Ok(0) };
    let set = |dst: &mut i32, v: Option<i32>| {
        if let Some(v) = v {
            *dst = v;
        }
    };
    set(&mut tm.tm_sec, p.sec);
    set(&mut tm.tm_min, p.min);
    set(&mut tm.tm_hour, p.hour);
    set(&mut tm.tm_mday, p.mday);
    set(&mut tm.tm_mon, p.mon);
    set(&mut tm.tm_year, p.year);
    set(&mut tm.tm_wday, p.wday);
    set(&mut tm.tm_yday, p.yday);
    set(&mut tm.tm_isdst, p.isdst);
    if let Some(off) = p.gmtoff {
        tm.tm_gmtoff = off;
    }
    if let Some(z) = p.zone {
        tm.tm_zone = z;
    }
    Ok(base.saturating_add(n))
}

/// The `struct tm` of `getdate(s)`, or the `getdate_err` number.
fn resolve(s: KResult<&[u8]>) -> Result<struct_tm, i32> {
    let s = s.map_err(|_| getdate::INVALID)?;
    let datemsk = process::getenv(b"DATEMSK").map(strz::env_value);
    let l = getdate::getdate(&CSys, datemsk, s, crate::locale::current(), &Process::new())?;
    Ok(pack::tm(&l.civil, l.isdst, l.offset, l.zone))
}

/// `getdate(s)`: the address of the calling thread's result, or 0 with `getdate_err` set.
pub fn getdate(s: KResult<&[u8]>) -> usize {
    match resolve(s) {
        Ok(tm) => match thread::result_area::<64>(SLOT_GETDATE) {
            Some(buf) => {
                buf.store(&pack::tm_bytes(&tm));
                buf.addr()
            }
            // No result slots for this thread (D-C0.2): "memory allocation failed".
            None => {
                data::getdate_err.store(getdate::NO_MEMORY, core::sync::atomic::Ordering::Relaxed);
                0
            }
        },
        Err(e) => {
            data::getdate_err.store(e, core::sync::atomic::Ordering::Relaxed);
            0
        }
    }
}

/// `getdate_r(s, res)`: 0 with `*res` filled, or the error number (`res` NULL: 8).
pub fn getdate_r(s: KResult<&[u8]>, res: Option<&mut struct_tm>) -> i32 {
    let Some(res) = res else { return getdate::INVALID };
    match resolve(s) {
        Ok(tm) => {
            *res = tm;
            0
        }
        Err(e) => e,
    }
}
