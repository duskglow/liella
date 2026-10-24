//! Serialising results into caller memory already converted to `&mut [u8]` or `&mut T` by the
//! boundary, including the _r ERANGE protocol. Finding F13.
//!
//! Implementation phase: 8 (implementation-plan.md §6). Phase 2 adds the `div` result structs;
//! phase 10 the user, group and login records of §17 ([`Packer`]).

use crate::layout::{div_t, imaxdiv_t, ldiv_t, lldiv_t};

/// `div_t` from a quotient and remainder.
pub fn div(r: (i32, i32)) -> div_t {
    div_t { quot: r.0, rem: r.1 }
}

/// `ldiv_t`.
pub fn ldiv(r: (i64, i64)) -> ldiv_t {
    ldiv_t { quot: r.0, rem: r.1 }
}

/// `lldiv_t`.
pub fn lldiv(r: (i64, i64)) -> lldiv_t {
    lldiv_t { quot: r.0, rem: r.1 }
}

/// `imaxdiv_t`.
pub fn imaxdiv(r: (i64, i64)) -> imaxdiv_t {
    imaxdiv_t { quot: r.0, rem: r.1 }
}

// ------------------------------------------------------------------------------ struct tm (D-23.4)

use crate::layout::struct_tm;
use liella::time::{CivilTime, Fields};

/// `struct tm` from a civil time, its DST flag, offset (seconds east) and abbreviation address:
/// `tm_year` is the year − 1900 and `tm_mon` 0-based. The caller has checked that the year fits
/// (`liella::time::civil::fits_tm`); a year that does not is stored as `INT_MAX`.
pub fn tm(c: &CivilTime, isdst: bool, gmtoff: i64, zone: usize) -> struct_tm {
    struct_tm {
        tm_sec: i32::from(c.second),
        tm_min: i32::from(c.minute),
        tm_hour: i32::from(c.hour),
        tm_mday: i32::from(c.day),
        tm_mon: i32::from(c.month).saturating_sub(1),
        tm_year: c.year.checked_sub(1900).and_then(|y| i32::try_from(y).ok()).unwrap_or(i32::MAX),
        tm_wday: i32::from(c.weekday),
        tm_yday: i32::from(c.yearday),
        tm_isdst: i32::from(isdst),
        tm_gmtoff: gmtoff,
        tm_zone: zone,
    }
}

/// The fields of a caller's `struct tm` (full year, 1-based month), not normalised.
pub fn fields(t: &struct_tm) -> Fields {
    Fields {
        year: i64::from(t.tm_year).saturating_add(1900),
        month: i64::from(t.tm_mon).saturating_add(1),
        day: i64::from(t.tm_mday),
        hour: i64::from(t.tm_hour),
        minute: i64::from(t.tm_min),
        second: i64::from(t.tm_sec),
    }
}

/// The bytes of a `struct tm` (56, native order), for a library-owned result buffer.
pub fn tm_bytes(t: &struct_tm) -> [u8; 56] {
    let mut out = [0u8; 56];
    let words = [t.tm_sec, t.tm_min, t.tm_hour, t.tm_mday, t.tm_mon, t.tm_year, t.tm_wday, t.tm_yday, t.tm_isdst];
    let mut parts = out.chunks_exact_mut(4);
    for (w, chunk) in words.iter().zip(&mut parts) {
        chunk.copy_from_slice(&w.to_ne_bytes());
    }
    // Bytes 36..40 are padding; then tm_gmtoff and tm_zone.
    if let Some(s) = out.get_mut(40..48) {
        s.copy_from_slice(&t.tm_gmtoff.to_ne_bytes());
    }
    if let Some(s) = out.get_mut(48..56) {
        s.copy_from_slice(&u64::try_from(t.tm_zone).unwrap_or(0).to_ne_bytes());
    }
    out
}

// ------------------------------------------------------------------------------ records (§17, D-17.3)

use crate::layout::{struct_exit_status, struct_group, struct_passwd, struct_sgrp, struct_spwd, struct_utmp, struct_utmp__ut_tv};
use liella::users::{Group, Gshadow, List, Passwd, Shadow, Utmp};

/// Fills a caller's buffer with a record's strings and pointer arrays (the `_r` protocol): every
/// step returns `None` when the buffer is too small (`ERANGE`). Built without a buffer, it only
/// counts the bytes a record needs ([`Packer::measure`]), for library-owned results.
pub struct Packer<'a> {
    buf: Option<&'a mut [u8]>,
    base: usize,
    used: usize,
}

impl<'a> Packer<'a> {
    /// A packer over `buf`, whose first byte is at address `base`.
    pub fn new(buf: &'a mut [u8], base: usize) -> Packer<'a> {
        Packer { buf: Some(buf), base, used: 0 }
    }

    /// A packer that only measures (addresses are relative to 0).
    pub fn measure() -> Packer<'a> {
        Packer { buf: None, base: 0, used: 0 }
    }

    /// The bytes used so far, plus the alignment padding a real buffer may need (7).
    pub fn needed(&self) -> usize {
        self.used.saturating_add(7)
    }

    /// Reserve `n` bytes: their offset in the buffer.
    fn take(&mut self, n: usize) -> Option<usize> {
        let at = self.used;
        let end = at.checked_add(n)?;
        if let Some(b) = &self.buf {
            if end > b.len() {
                return None;
            }
        }
        self.used = end;
        Some(at)
    }

    /// Copy `text` and a terminating NUL: the copy's address.
    pub fn text(&mut self, text: &[u8]) -> Option<usize> {
        let at = self.take(text.len().checked_add(1)?)?;
        if let Some(b) = self.buf.as_deref_mut() {
            let d = b.get_mut(at..at.checked_add(text.len())?)?;
            d.copy_from_slice(text);
            *b.get_mut(at.checked_add(text.len())?)? = 0;
        }
        self.base.checked_add(at)
    }

    /// An array of `addrs` and a terminating NULL pointer, aligned for pointers: its address.
    pub fn ptrs(&mut self, addrs: &[usize]) -> Option<usize> {
        let pad = self.base.wrapping_add(self.used).wrapping_neg() & 7;
        self.take(pad)?;
        let n = addrs.len().checked_add(1)?;
        let at = self.take(n.checked_mul(8)?)?;
        if let Some(b) = self.buf.as_deref_mut() {
            let dst = b.get_mut(at..at.checked_add(n.checked_mul(8)?)?)?;
            for (chunk, a) in dst.chunks_exact_mut(8).zip(addrs.iter().copied().chain([0])) {
                chunk.copy_from_slice(&a.to_ne_bytes());
            }
        }
        self.base.checked_add(at)
    }

    /// The entries of `list` copied, then the pointer array naming them: its address.
    pub fn list(&mut self, list: List<'_>) -> Option<usize> {
        self.texts(list.iter())
    }

    /// `bytes` copied at an address aligned for any scalar (8): its address.
    pub fn raw(&mut self, bytes: &[u8]) -> Option<usize> {
        let pad = self.base.wrapping_add(self.used).wrapping_neg() & 7;
        self.take(pad)?;
        let at = self.take(bytes.len())?;
        if let Some(b) = self.buf.as_deref_mut() {
            b.get_mut(at..at.checked_add(bytes.len())?)?.copy_from_slice(bytes);
        }
        self.base.checked_add(at)
    }

    /// The strings of `items` copied (each terminated), then the pointer array naming them (NULL
    /// terminated): its address.
    pub fn texts<'b>(&mut self, items: impl Iterator<Item = &'b [u8]> + Clone) -> Option<usize> {
        self.items(items, |k, e| k.text(e))
    }

    /// The byte strings of `items` copied, each aligned for any scalar (addresses: `struct
    /// in_addr`, `struct in6_addr`), then the NULL-terminated pointer array: its address.
    pub fn raws<'b>(&mut self, items: impl Iterator<Item = &'b [u8]> + Clone) -> Option<usize> {
        self.items(items, |k, e| k.raw(e))
    }

    fn items<'b>(&mut self, items: impl Iterator<Item = &'b [u8]> + Clone, mut copy: impl FnMut(&mut Self, &[u8]) -> Option<usize>) -> Option<usize> {
        let mut addrs = [0usize; 64];
        let n = items.clone().count();
        if n <= addrs.len() {
            for (slot, e) in addrs.iter_mut().zip(items) {
                *slot = copy(self, e)?;
            }
            return self.ptrs(addrs.get(..n)?);
        }
        // A long list: the array first, filled in as the entries are copied.
        let pad = self.base.wrapping_add(self.used).wrapping_neg() & 7;
        self.take(pad)?;
        let table = self.take(n.checked_add(1)?.checked_mul(8)?)?;
        for (k, e) in items.enumerate() {
            let a = copy(self, e)?;
            if let Some(b) = self.buf.as_deref_mut() {
                let at = table.checked_add(k.checked_mul(8)?)?;
                b.get_mut(at..at.checked_add(8)?)?.copy_from_slice(&a.to_ne_bytes());
            }
        }
        if let Some(b) = self.buf.as_deref_mut() {
            let at = table.checked_add(n.checked_mul(8)?)?;
            b.get_mut(at..at.checked_add(8)?)?.copy_from_slice(&0usize.to_ne_bytes());
        }
        self.base.checked_add(table)
    }
}

/// `struct passwd` for `p`, its strings in `k`.
pub fn passwd(p: &Passwd<'_>, k: &mut Packer<'_>) -> Option<struct_passwd> {
    Some(struct_passwd { pw_name: k.text(p.name)?, pw_passwd: k.text(p.passwd)?, pw_uid: p.uid, pw_gid: p.gid, pw_gecos: k.text(p.gecos)?, pw_dir: k.text(p.dir)?, pw_shell: k.text(p.shell)? })
}

/// `struct group` for `g`, its strings and member array in `k`.
pub fn group(g: &Group<'_>, k: &mut Packer<'_>) -> Option<struct_group> {
    let gr_mem = k.list(g.members)?;
    Some(struct_group { gr_name: k.text(g.name)?, gr_passwd: k.text(g.passwd)?, gr_gid: g.gid, gr_mem })
}

/// `struct spwd` for `s`, its strings in `k`.
pub fn spwd(s: &Shadow<'_>, k: &mut Packer<'_>) -> Option<struct_spwd> {
    let [a0, a1, a2, a3, a4, a5] = s.aging;
    Some(struct_spwd { sp_namp: k.text(s.name)?, sp_pwdp: k.text(s.passwd)?, sp_lstchg: a0, sp_min: a1, sp_max: a2, sp_warn: a3, sp_inact: a4, sp_expire: a5, sp_flag: s.flag })
}

/// `struct sgrp` for `g`, its strings and arrays in `k`.
pub fn sgrp(g: &Gshadow<'_>, k: &mut Packer<'_>) -> Option<struct_sgrp> {
    let sg_adm = k.list(g.admins)?;
    let sg_mem = k.list(g.members)?;
    Some(struct_sgrp { sg_namp: k.text(g.name)?, sg_passwd: k.text(g.passwd)?, sg_adm, sg_mem })
}

fn chars<const N: usize>(b: &[u8; N]) -> [i8; N] {
    b.map(|c| i8::from_ne_bytes([c]))
}

fn bytes<const N: usize>(c: &[i8; N]) -> [u8; N] {
    c.map(|v| v.to_ne_bytes()[0])
}

/// `struct utmp` (and `struct utmpx`, the same layout) for a record: the seconds are stored as
/// unsigned 32-bit in the signed C member, so a C caller reads them as `uint32_t` after 2038
/// (D-17.5).
pub fn utmp(u: &Utmp) -> struct_utmp {
    struct_utmp {
        ut_type: u.kind,
        ut_pid: u.pid,
        ut_line: chars(&u.line),
        ut_id: chars(&u.id),
        ut_user: chars(&u.user),
        ut_host: chars(&u.host),
        ut_exit: struct_exit_status { e_termination: u.exit[0], e_exit: u.exit[1] },
        ut_session: u.session,
        ut_tv: {
            let (sec, usec) = liella::users::ut_tv(u.time);
            struct_utmp__ut_tv { tv_sec: i32::from_ne_bytes(sec.to_ne_bytes()), tv_usec: usec }
        },
        ut_addr_v6: u.addr,
        __glibc_reserved: [0; 20],
    }
}

/// The record a caller's `struct utmp` holds; its 32-bit seconds read as unsigned (D-17.5), so the
/// caller's bits are written back unchanged.
pub fn utmp_in(c: &struct_utmp) -> Utmp {
    Utmp {
        kind: c.ut_type,
        pid: c.ut_pid,
        line: bytes(&c.ut_line),
        id: bytes(&c.ut_id),
        user: bytes(&c.ut_user),
        host: bytes(&c.ut_host),
        exit: [c.ut_exit.e_termination, c.ut_exit.e_exit],
        session: c.ut_session,
        time: (liella::users::ut_sec(c.ut_tv.tv_sec), c.ut_tv.tv_usec),
        addr: c.ut_addr_v6,
    }
}

// ------------------------------------------------------------------------------ terminal settings (§19.1, D-19.1)

use crate::layout::{struct_termios, struct_winsize};
use liella::term::{Termios, CIBAUD, NCC};
use liella_kernel::uapi::Winsize;

/// `c_ispeed` of a structure whose input speed was set to 0 (`cfsetispeed(t, 0)`): the input
/// speed then follows the output speed when applied (POSIX), while `cfgetispeed` reports `B0`.
/// No rate is this value; every other `c_ispeed` is the input rate (`c_ospeed` the output rate)
/// that a `BOTHER` speed code in `c_cflag` refers to, as in the kernel's `struct termios2`.
pub const ISPEED_ZERO: u32 = u32::MAX;

/// The C `struct termios` for `t`: the mode words as the kernel has them, its 19 control
/// characters followed by zeros (`NCCS` is 32 in C), and the two rates.
pub fn termios(t: &Termios) -> struct_termios {
    let mut cc = [0u8; 32];
    if let Some(head) = cc.get_mut(..NCC) {
        head.copy_from_slice(&t.cc);
    }
    struct_termios { c_iflag: t.iflag, c_oflag: t.oflag, c_cflag: t.cflag, c_lflag: t.lflag, c_line: t.line, c_cc: cc, c_ispeed: t.ispeed, c_ospeed: t.ospeed }
}

/// The settings a C `struct termios` asks for: the mode words as given (control characters
/// past the kernel's 19 are not kept), the rates as given, and an input speed of 0
/// ([`ISPEED_ZERO`]) as an input speed that follows the output speed.
pub fn termios_in(c: &struct_termios) -> Termios {
    let mut cc = [0u8; NCC];
    if let Some(head) = c.c_cc.get(..NCC) {
        cc.copy_from_slice(head);
    }
    let (cflag, ispeed) = if c.c_ispeed == ISPEED_ZERO { (c.c_cflag & !CIBAUD, 0) } else { (c.c_cflag, c.c_ispeed) };
    Termios { iflag: c.c_iflag, oflag: c.c_oflag, cflag, lflag: c.c_lflag, line: c.c_line, cc, ispeed, ospeed: c.c_ospeed }
}

/// The kernel window size for a C `struct winsize`.
pub fn winsize_in(w: &struct_winsize) -> Winsize {
    Winsize { row: w.ws_row, col: w.ws_col, xpixel: w.ws_xpixel, ypixel: w.ws_ypixel }
}
