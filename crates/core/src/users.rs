//! The user and group record formats (spec §17.3–17.6, phase 10): `passwd(5)`, `group(5)`,
//! `shadow(5)` and `gshadow(5)` lines, the `shells(5)` list, the terminal table, and the login
//! record of `utmp(5)`. Pure: no files and no C layouts (the databases are read by
//! `liella_legacy::nss` through the Name Service Switch, the records packed by `liella_abi::pack`).
//!
//! **Lines** (D-17.3, `spec/behaviour/users.md`). A line is one record: leading white space is
//! skipped, and an empty line or one whose first other character is `#` is no record. Fields
//! are separated by `:`; the last field of each format is the rest of the line, colons included
//! (`/bin/sh:extra` is a shell). Trailing fields may be omitted down to a minimum per format
//! (passwd: name, password, UID, GID; group: name, password, GID; shadow: name, password and
//! the six aging fields; gshadow: the name), as the reference reads them. A record needs a name.
//!
//! **Numbers** are decimal digits only (no sign, no white space, no `0x`) and must fit: UIDs and
//! GIDs in 32 bits, the shadow fields in `long`, its flag in `unsigned long`. An empty UID or
//! GID is valid only in a NIS compatibility entry (a name starting with `+` or `-`), where it
//! reads as 0; an empty shadow field is -1 (the flag all ones).
//!
//! **Lists** (group members, gshadow administrators and members) are comma-separated; white
//! space before an entry is dropped, white space after it kept, and empty entries are dropped.

use crate::error::{Errno, Result};
use crate::mem::TryVec;

/// ASCII white space, as the record formats skip it.
fn is_space(c: u8) -> bool {
    matches!(c, b' ' | b'\t' | b'\n' | b'\r' | 0x0b | 0x0c)
}

/// The text of the record on `line` (one line of a file, with or without its newline): `None`
/// for a blank line or a comment.
pub fn record(line: &[u8]) -> Option<&[u8]> {
    let line = line.strip_suffix(b"\n").unwrap_or(line);
    let start = line.iter().position(|&c| !is_space(c))?;
    let text = line.get(start..)?;
    if text.first() == Some(&b'#') { None } else { Some(text) }
}

/// The lines of a file's text (the last may lack its newline).
pub fn lines(text: &[u8]) -> impl Iterator<Item = &[u8]> {
    let end = text.strip_suffix(b"\n").unwrap_or(text);
    end.split(|&c| c == b'\n').filter(move |_| !text.is_empty())
}

/// The first `N` fields of `text`, the last taking the rest of the line; `None` for a field
/// the line does not reach.
fn fields<const N: usize>(text: &[u8]) -> [Option<&[u8]>; N] {
    let mut it = text.splitn(N, |&c| c == b':');
    core::array::from_fn(|_| it.next())
}

/// Whether `name` is a NIS compatibility entry (`+name`, `-name`, `+@netgroup`, `+`).
pub fn is_compat(name: &[u8]) -> bool {
    matches!(name.first(), Some(b'+' | b'-'))
}

/// A decimal number of digits only.
fn decimal(text: &[u8]) -> Option<u64> {
    if text.is_empty() {
        return None;
    }
    text.iter().try_fold(0u64, |v, &c| if c.is_ascii_digit() { v.checked_mul(10)?.checked_add(u64::from(c.wrapping_sub(b'0'))) } else { None })
}

/// A UID or GID field: 32 bits; empty only in a compatibility entry (then 0).
fn id(text: &[u8], compat: bool) -> Option<u32> {
    if text.is_empty() && compat {
        return Some(0);
    }
    u32::try_from(decimal(text)?).ok()
}

/// A shadow aging field: a non-negative `long`, or -1 when empty.
fn aging(text: &[u8]) -> Option<i64> {
    if text.is_empty() { Some(-1) } else { i64::try_from(decimal(text)?).ok() }
}

/// A comma-separated list of names.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct List<'a>(pub &'a [u8]);

impl<'a> List<'a> {
    /// The entries: white space before each dropped, empty ones skipped.
    pub fn iter(&self) -> impl Iterator<Item = &'a [u8]> + Clone + use<'a> {
        let text = self.0;
        text.split(|&c| c == b',').map(|e| e.get(e.iter().position(|&c| !is_space(c)).unwrap_or(e.len())..).unwrap_or(&[])).filter(|e| !e.is_empty())
    }

    /// Whether `name` is an entry.
    pub fn contains(&self, name: &[u8]) -> bool {
        self.iter().any(|e| e == name)
    }
}

/// A user record (`struct passwd`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Passwd<'a> {
    /// `pw_name`.
    pub name: &'a [u8],
    /// `pw_passwd`.
    pub passwd: &'a [u8],
    /// `pw_uid`.
    pub uid: u32,
    /// `pw_gid`.
    pub gid: u32,
    /// `pw_gecos`.
    pub gecos: &'a [u8],
    /// `pw_dir`.
    pub dir: &'a [u8],
    /// `pw_shell`.
    pub shell: &'a [u8],
}

/// The user record on `line` (`passwd(5)`), `None` for a line that is none or does not parse.
pub fn passwd(line: &[u8]) -> Option<Passwd<'_>> {
    let [name, pw, uid, gid, gecos, dir, shell] = fields::<7>(record(line)?);
    let name = name.filter(|n| !n.is_empty())?;
    let compat = is_compat(name);
    Some(Passwd { name, passwd: pw?, uid: id(uid?, compat)?, gid: id(gid?, compat)?, gecos: gecos.unwrap_or(&[]), dir: dir.unwrap_or(&[]), shell: shell.unwrap_or(&[]) })
}

/// A group record (`struct group`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Group<'a> {
    /// `gr_name`.
    pub name: &'a [u8],
    /// `gr_passwd`.
    pub passwd: &'a [u8],
    /// `gr_gid`.
    pub gid: u32,
    /// `gr_mem`.
    pub members: List<'a>,
}

/// The group record on `line` (`group(5)`).
pub fn group(line: &[u8]) -> Option<Group<'_>> {
    let [name, pw, gid, members] = fields::<4>(record(line)?);
    let name = name.filter(|n| !n.is_empty())?;
    Some(Group { name, passwd: pw?, gid: id(gid?, is_compat(name))?, members: List(members.unwrap_or(&[])) })
}

/// A shadow password record (`struct spwd`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Shadow<'a> {
    /// `sp_namp`.
    pub name: &'a [u8],
    /// `sp_pwdp`.
    pub passwd: &'a [u8],
    /// `sp_lstchg`, `sp_min`, `sp_max`, `sp_warn`, `sp_inact`, `sp_expire` (-1: empty).
    pub aging: [i64; 6],
    /// `sp_flag` (all ones: empty).
    pub flag: u64,
}

/// The shadow record on `line` (`shadow(5)`): nine fields, the flag may be omitted.
pub fn shadow(line: &[u8]) -> Option<Shadow<'_>> {
    let [name, pw, a0, a1, a2, a3, a4, a5, flag] = fields::<9>(record(line)?);
    let name = name.filter(|n| !n.is_empty())?;
    let aging = [aging(a0?)?, aging(a1?)?, aging(a2?)?, aging(a3?)?, aging(a4?)?, aging(a5?)?];
    let flag = match flag {
        None | Some(b"") => u64::MAX,
        Some(t) => decimal(t)?,
    };
    Some(Shadow { name, passwd: pw?, aging, flag })
}

/// A shadow group record (`struct sgrp`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Gshadow<'a> {
    /// `sg_namp`.
    pub name: &'a [u8],
    /// `sg_passwd`.
    pub passwd: &'a [u8],
    /// `sg_adm`.
    pub admins: List<'a>,
    /// `sg_mem`.
    pub members: List<'a>,
}

/// The shadow group record on `line` (`gshadow(5)`).
pub fn gshadow(line: &[u8]) -> Option<Gshadow<'_>> {
    let [name, pw, adm, mem] = fields::<4>(record(line)?);
    let name = name.filter(|n| !n.is_empty())?;
    Some(Gshadow { name, passwd: pw.unwrap_or(&[]), admins: List(adm.unwrap_or(&[])), members: List(mem.unwrap_or(&[])) })
}

// ------------------------------------------------------------------------------ writing

/// Append a field's text: `EINVAL` when it would split the line (a `:` or a newline).
fn put_text(out: &mut TryVec<u8>, text: &[u8]) -> Result<()> {
    if text.iter().any(|&c| c == b':' || c == b'\n') {
        return Err(Errno::EINVAL);
    }
    out.extend_from_slice(text)
}

/// Append a decimal number.
fn put_number(out: &mut TryVec<u8>, v: i128) -> Result<()> {
    let mut digits = [0u8; 40];
    let mut n = v.unsigned_abs();
    let mut i = digits.len();
    loop {
        i = i.saturating_sub(1);
        if let Some(d) = digits.get_mut(i) {
            *d = b'0'.saturating_add(u8::try_from(n % 10).unwrap_or(0));
        }
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if v < 0 {
        out.push(b'-')?;
    }
    out.extend_from_slice(digits.get(i..).unwrap_or(&[]))
}

/// Append a list: entries separated by commas, each free of `,`, `:` and newlines.
fn put_list<'a>(out: &mut TryVec<u8>, entries: impl IntoIterator<Item = &'a [u8]>) -> Result<()> {
    for (k, e) in entries.into_iter().enumerate() {
        if e.contains(&b',') {
            return Err(Errno::EINVAL);
        }
        if k > 0 {
            out.push(b',')?;
        }
        put_text(out, e)?;
    }
    Ok(())
}

/// A record's name: present, and not empty (a line without one does not read back).
fn put_name(out: &mut TryVec<u8>, name: Option<&[u8]>) -> Result<()> {
    match name {
        Some(n) if !n.is_empty() => put_text(out, n),
        _ => Err(Errno::EINVAL),
    }
}

/// An ID: in a compatibility entry, 0 is written empty (it reads back as 0).
fn put_id(out: &mut TryVec<u8>, v: u32, compat: bool) -> Result<()> {
    if compat && v == 0 { Ok(()) } else { put_number(out, i128::from(v)) }
}

/// A user record's fields for writing; `None` strings are written empty, a missing name is
/// `EINVAL`.
#[derive(Clone, Copy, Debug, Default)]
pub struct PasswdOut<'a> {
    /// `pw_name`.
    pub name: Option<&'a [u8]>,
    /// `pw_passwd`.
    pub passwd: Option<&'a [u8]>,
    /// `pw_uid`.
    pub uid: u32,
    /// `pw_gid`.
    pub gid: u32,
    /// `pw_gecos`.
    pub gecos: Option<&'a [u8]>,
    /// `pw_dir`.
    pub dir: Option<&'a [u8]>,
    /// `pw_shell`.
    pub shell: Option<&'a [u8]>,
}

/// The `passwd(5)` line of `p`, with its newline (`putpwent`). `EINVAL` for a field that would
/// not read back (a colon or newline in it, no name).
pub fn put_passwd(p: &PasswdOut<'_>, out: &mut TryVec<u8>) -> Result<()> {
    put_name(out, p.name)?;
    let compat = p.name.is_some_and(is_compat);
    out.push(b':')?;
    put_text(out, p.passwd.unwrap_or(&[]))?;
    out.push(b':')?;
    put_id(out, p.uid, compat)?;
    out.push(b':')?;
    put_id(out, p.gid, compat)?;
    for f in [p.gecos, p.dir, p.shell] {
        out.push(b':')?;
        put_text(out, f.unwrap_or(&[]))?;
    }
    out.push(b'\n')
}

/// The `group(5)` line of a group (`putgrent`); `members` are the entries of `gr_mem`.
pub fn put_group<'a>(name: Option<&[u8]>, passwd: Option<&[u8]>, gid: u32, members: impl IntoIterator<Item = &'a [u8]>, out: &mut TryVec<u8>) -> Result<()> {
    put_name(out, name)?;
    out.push(b':')?;
    put_text(out, passwd.unwrap_or(&[]))?;
    out.push(b':')?;
    put_id(out, gid, name.is_some_and(is_compat))?;
    out.push(b':')?;
    put_list(out, members)?;
    out.push(b'\n')
}

/// The `shadow(5)` line of a record (`putspent`): aging fields of -1 and a flag of all ones are
/// written empty.
pub fn put_shadow(name: Option<&[u8]>, passwd: Option<&[u8]>, aging: [i64; 6], flag: u64, out: &mut TryVec<u8>) -> Result<()> {
    put_name(out, name)?;
    out.push(b':')?;
    put_text(out, passwd.unwrap_or(&[]))?;
    for a in aging {
        out.push(b':')?;
        if a != -1 {
            put_number(out, i128::from(a))?;
        }
    }
    out.push(b':')?;
    if flag != u64::MAX {
        put_number(out, i128::from(flag))?;
    }
    out.push(b'\n')
}

/// The `gshadow(5)` line of a record (`putsgent`).
pub fn put_gshadow<'a, 'b>(name: Option<&[u8]>, passwd: Option<&[u8]>, admins: impl IntoIterator<Item = &'a [u8]>, members: impl IntoIterator<Item = &'b [u8]>, out: &mut TryVec<u8>) -> Result<()> {
    put_name(out, name)?;
    out.push(b':')?;
    put_text(out, passwd.unwrap_or(&[]))?;
    out.push(b':')?;
    put_list(out, admins)?;
    out.push(b':')?;
    put_list(out, members)?;
    out.push(b'\n')
}

/// `getpw`'s line: the record as `putpwent` writes it, without the newline.
pub fn passwd_line(p: &Passwd<'_>, out: &mut TryVec<u8>) -> Result<()> {
    let o = PasswdOut { name: Some(p.name), passwd: Some(p.passwd), uid: p.uid, gid: p.gid, gecos: Some(p.gecos), dir: Some(p.dir), shell: Some(p.shell) };
    put_passwd(&o, out)?;
    out.pop();
    Ok(())
}

// ------------------------------------------------------------------------------ group lists

/// The groups of `user` in a group file's text, `base` first (`getgrouplist`, `initgroups`):
/// the base GID never repeated, every other group's GID in file order (a GID two groups share is
/// listed once per group, as the reference does). Compatibility entries are not groups (they are not found by
/// name or GID either).
pub fn group_list(text: &[u8], user: &[u8], base: u32, out: &mut TryVec<u32>) -> Result<()> {
    out.push(base)?;
    for g in lines(text).filter_map(group) {
        if !is_compat(g.name) && g.members.contains(user) && g.gid != base {
            out.push(g.gid)?;
        }
    }
    Ok(())
}

// ------------------------------------------------------------------------------ shells, ttys

/// The shells `/etc/shells` lists when it is missing (§17.5a).
pub const DEFAULT_SHELLS: [&[u8]; 2] = [b"/bin/sh", b"/bin/csh"];

/// The shells of a `shells(5)` text, in order: on each line, after white space, an absolute path
/// up to the first white space or `#`; other lines (comments, blank, relative names) are none.
pub fn shells(text: &[u8]) -> impl Iterator<Item = &[u8]> {
    lines(text).filter_map(|l| {
        let start = l.iter().position(|&c| !is_space(c))?;
        let rest = l.get(start..)?;
        let end = rest.iter().position(|&c| is_space(c) || c == b'#').unwrap_or(rest.len());
        rest.get(..end).filter(|s| s.first() == Some(&b'/'))
    })
}

/// The 1-based index of the entry for the terminal `name` (its path below `/dev`, `pts/0`) in a
/// terminal table (`/etc/ttys`: one entry per line, its first word the terminal; comments and
/// blank lines are no entry), `None` if it has none (`ttyslot`).
pub fn tty_slot(text: &[u8], name: &[u8]) -> Option<usize> {
    let mut entries = lines(text).filter_map(record).map(|r| r.get(..r.iter().position(|&c| is_space(c)).unwrap_or(r.len())).unwrap_or(&[]));
    entries.position(|e| e == name).map(|i| i.saturating_add(1))
}

// ------------------------------------------------------------------------------ login records

/// The size of a login record in the files (`struct utmp`, 384 bytes).
pub const UTMP_SIZE: usize = 384;
/// `ut_type`: empty.
pub const EMPTY: i16 = 0;
/// `ut_type`: change of run level.
pub const RUN_LVL: i16 = 1;
/// `ut_type`: boot time.
pub const BOOT_TIME: i16 = 2;
/// `ut_type`: time after a clock change.
pub const NEW_TIME: i16 = 3;
/// `ut_type`: time before a clock change.
pub const OLD_TIME: i16 = 4;
/// `ut_type`: a process `init` started.
pub const INIT_PROCESS: i16 = 5;
/// `ut_type`: a `getty` waiting for a login.
pub const LOGIN_PROCESS: i16 = 6;
/// `ut_type`: a user's session.
pub const USER_PROCESS: i16 = 7;
/// `ut_type`: a session that ended.
pub const DEAD_PROCESS: i16 = 8;

/// The last second the 32-bit `ut_tv.tv_sec` holds, read as unsigned (D-17.5, F38):
/// 2106-02-07 06:28:15 UTC.
pub const UT_SEC_MAX: i64 = 0xFFFF_FFFF;

/// `ut_tv` as the shared 32-bit format holds it (D-17.5, F38): seconds in `[0, 2^32)` as their
/// unsigned 32-bit value, so a time to 2106-02-07 06:28:15 UTC is kept and every value before
/// 2038-01-19 03:14:07 UTC has the same bytes as the signed field glibc writes. A time outside
/// that range is no time at all, `(0, 0)`: never wrapped.
pub fn ut_tv(time: (i64, i32)) -> (u32, i32) {
    match u32::try_from(time.0) {
        Ok(sec) => (sec, time.1),
        Err(_) => (0, 0),
    }
}

/// The seconds of a 32-bit `ut_tv.tv_sec` as stored (the C member is a signed `int32_t`):
/// unsigned, `[0, 2^32)` (D-17.5).
pub fn ut_sec(raw: i32) -> i64 {
    i64::from(u32::from_ne_bytes(raw.to_ne_bytes()))
}

/// A login record (`utmp(5)`). The time is 64-bit here; the file and the C structure keep the
/// 32-bit `ut_tv` of the shared format, read and written as unsigned seconds (D-17.5, F38).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Utmp {
    /// `ut_type`.
    pub kind: i16,
    /// `ut_pid`.
    pub pid: i32,
    /// `ut_line`: the terminal below `/dev`.
    pub line: [u8; 32],
    /// `ut_id`: the `inittab` identifier.
    pub id: [u8; 4],
    /// `ut_user`.
    pub user: [u8; 32],
    /// `ut_host`.
    pub host: [u8; 256],
    /// `ut_exit`: termination and exit status.
    pub exit: [i16; 2],
    /// `ut_session`.
    pub session: i32,
    /// `ut_tv`: seconds and microseconds.
    pub time: (i64, i32),
    /// `ut_addr_v6`.
    pub addr: [i32; 4],
}

impl Default for Utmp {
    fn default() -> Utmp {
        Utmp { kind: EMPTY, pid: 0, line: [0; 32], id: [0; 4], user: [0; 32], host: [0; 256], exit: [0; 2], session: 0, time: (0, 0), addr: [0; 4] }
    }
}

/// The class of a search key's type (`getutid`): `Some(false)` for the types matched by type
/// alone, `Some(true)` for those matched by `ut_id`, `None` for the rest (no search: `EINVAL`).
pub fn id_class(kind: i16) -> Option<bool> {
    match kind {
        RUN_LVL | BOOT_TIME | NEW_TIME | OLD_TIME => Some(false),
        INIT_PROCESS | LOGIN_PROCESS | USER_PROCESS | DEAD_PROCESS => Some(true),
        _ => None,
    }
}

/// The bytes of a fixed text field up to its first NUL.
pub fn field_text(f: &[u8]) -> &[u8] {
    f.get(..f.iter().position(|&c| c == 0).unwrap_or(f.len())).unwrap_or(&[])
}

/// Store `text` in a fixed field: truncated to it, the rest zero (as `strncpy`).
pub fn set_field(f: &mut [u8], text: &[u8]) {
    for (k, d) in f.iter_mut().enumerate() {
        *d = text.get(k).copied().unwrap_or(0);
    }
}

/// `N` bytes of `b` at `at` (zeros past its end).
fn bytes_of<const N: usize>(b: &[u8], at: usize) -> [u8; N] {
    b.get(at..at.saturating_add(N)).and_then(|s| s.try_into().ok()).unwrap_or([0; N])
}

impl Utmp {
    /// Whether this record answers `getutid(key)`: the same type for the time records, the same
    /// `ut_id` among the process records.
    pub fn matches_id(&self, key: &Utmp) -> bool {
        match (id_class(key.kind), id_class(self.kind)) {
            (Some(false), _) => self.kind == key.kind,
            (Some(true), Some(true)) => self.id == key.id,
            _ => false,
        }
    }

    /// Whether this record answers `getutline(key)`: a login or user process on the same line.
    pub fn matches_line(&self, key: &Utmp) -> bool {
        matches!(self.kind, LOGIN_PROCESS | USER_PROCESS) && field_text(&self.line) == field_text(&key.line)
    }

    /// The record in the file format (native byte order, the 32-bit time).
    pub fn to_bytes(&self) -> [u8; UTMP_SIZE] {
        let mut b = [0u8; UTMP_SIZE];
        let mut put = |at: usize, v: &[u8]| {
            if let Some(d) = b.get_mut(at..at.saturating_add(v.len())) {
                d.copy_from_slice(v);
            }
        };
        put(0, &self.kind.to_ne_bytes());
        put(4, &self.pid.to_ne_bytes());
        put(8, &self.line);
        put(40, &self.id);
        put(44, &self.user);
        put(76, &self.host);
        put(332, &self.exit[0].to_ne_bytes());
        put(334, &self.exit[1].to_ne_bytes());
        put(336, &self.session.to_ne_bytes());
        let (sec, usec) = ut_tv(self.time);
        put(340, &sec.to_ne_bytes());
        put(344, &usec.to_ne_bytes());
        for (k, a) in self.addr.iter().enumerate() {
            put(348usize.saturating_add(k.saturating_mul(4)), &a.to_ne_bytes());
        }
        b
    }

    /// The record in a file's bytes.
    pub fn from_bytes(b: &[u8; UTMP_SIZE]) -> Utmp {
        let mut u = Utmp { kind: i16::from_ne_bytes(bytes_of(b, 0)), pid: i32::from_ne_bytes(bytes_of(b, 4)), ..Utmp::default() };
        u.line = bytes_of(b, 8);
        u.id = bytes_of(b, 40);
        u.user = bytes_of(b, 44);
        u.host = bytes_of(b, 76);
        u.exit = [i16::from_ne_bytes(bytes_of(b, 332)), i16::from_ne_bytes(bytes_of(b, 334))];
        u.session = i32::from_ne_bytes(bytes_of(b, 336));
        u.time = (i64::from(u32::from_ne_bytes(bytes_of(b, 340))), i32::from_ne_bytes(bytes_of(b, 344)));
        for (k, a) in u.addr.iter_mut().enumerate() {
            *a = i32::from_ne_bytes(bytes_of(b, 348usize.saturating_add(k.saturating_mul(4))));
        }
        u
    }
}
