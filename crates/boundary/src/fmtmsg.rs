//! `fmtmsg` and `addseverity` (spec §2.6; D-2.6), from the spec and `spec/behaviour/fmtmsg.md`.
//! Design: D-2.10 (F1-F7).
//!
//! A message is up to five fields, always in this order: label, severity, text, action (printed
//! as `TO FIX: action`) and tag. A field is present when its pointer is not NULL (an empty
//! string is present) and, for the severity, when it is not `MM_NOSEV`. A present field is
//! followed, when another present field comes after it, by its own separator: `": "` after the
//! label and the severity, a newline after the text, two spaces after the action. Every message
//! ends with a newline.
//!
//! `MSGVERB` (colon-separated keywords `label`, `severity`, `text`, `action`, `tag`, one
//! optional trailing colon; any other form selects every field) chooses the fields printed on
//! standard error; the console always gets every field. `SEV_LEVEL` (`keyword,level,string`
//! elements separated by colons; the keyword is ignored, the level is a whole C integer of any
//! base above 4 and within `int`, the string is everything after the second comma; malformed
//! elements are skipped) adds severities. Both are read once, at the first `fmtmsg`, into the
//! one table `addseverity` also writes, in call order.
//!
//! The message is put together once as a list of pieces and written to each destination in one
//! piece when memory allows (piece by piece otherwise). `fmtmsg` reports through its result
//! only: `errno` is as it was.

use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU32, AtomicUsize, Ordering::{Acquire, Relaxed, Release}};

use liella::error::Errno;
use liella::mem::TryVec;
use liella::num::parse::{parse_i64, Dialect, Status};
use liella::text::Encoding;
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{O_CLOEXEC, O_NOCTTY, O_WRONLY};
use liella_legacy::stdio_compat as sc;
use liella_rt::sync::RawMutex;

use crate::stdio::{self, Locking};
use crate::sys::CSys;
use crate::{conv, errno, locale, process, strz};

/// `MM_PRINT`: the message goes to standard error.
const MM_PRINT: i64 = 0x100;
/// `MM_CONSOLE`: the message goes to the console.
const MM_CONSOLE: i64 = 0x200;
/// `MM_OK`.
const MM_OK: i32 = 0;
/// `MM_NOTOK`: an invalid argument, or every requested output failed.
const MM_NOTOK: i32 = -1;
/// `MM_NOMSG`: writing to standard error failed.
const MM_NOMSG: i32 = 1;
/// `MM_NOCON`: writing to the console failed.
const MM_NOCON: i32 = 4;

/// The fields, one bit each, in message order.
const LABEL: u32 = 1;
const SEVERITY: u32 = 2;
const TEXT: u32 = 4;
const ACTION: u32 = 8;
const TAG: u32 = 16;
const ALL: u32 = LABEL | SEVERITY | TEXT | ACTION | TAG;

/// Each field in message order: its bit, what is printed before it, and its separator from a
/// following field.
const LAYOUT: [(u32, &[u8], &[u8]); 5] = [(LABEL, b"", b": "), (SEVERITY, b"", b": "), (TEXT, b"", b"\n"), (ACTION, b"TO FIX: ", b"  "), (TAG, b"", b"")];

/// The `MSGVERB` keywords.
const KEYWORDS: [(&[u8], u32); 5] = [(b"label", LABEL), (b"severity", SEVERITY), (b"text", TEXT), (b"action", ACTION), (b"tag", TAG)];

/// The standard severities by level (`MM_HALT` = 1 to `MM_INFO` = 4; 0 is `MM_NOSEV`).
const STANDARD: [&[u8]; 5] = [b"", b"HALT", b"ERROR", b"WARNING", b"INFO"];

/// The highest standard level: added severities are above it.
const LAST_STANDARD: i32 = 4;

/// How many added severities the table holds (D-2.6).
const CAPACITY: usize = 256;

/// One added severity: its level (0: a free entry) and its print string, a copy kept for the
/// rest of the process (a string read under [`LOCK`] stays valid after it).
struct Added {
    level: AtomicI32,
    addr: AtomicUsize,
    len: AtomicUsize,
}

/// The added severities (`addseverity` and `SEV_LEVEL`).
static ADDED: [Added; CAPACITY] = [const { Added { level: AtomicI32::new(0), addr: AtomicUsize::new(0), len: AtomicUsize::new(0) } }; CAPACITY];
/// Serialises [`ADDED`] and the reading of the environment.
static LOCK: RawMutex = RawMutex::new();
/// Whether the environment was read (at the first `fmtmsg`).
static ENV_READ: AtomicBool = AtomicBool::new(false);
/// The fields `MSGVERB` selects.
static SELECTED: AtomicU32 = AtomicU32::new(ALL);

/// Under [`LOCK`]: the entry holding `level` (0: a free entry).
fn entry(level: i32) -> Option<&'static Added> {
    ADDED.iter().find(|a| a.level.load(Relaxed) == level)
}

/// Under [`LOCK`]: the print string of an entry.
fn print_string(a: &Added) -> &'static [u8] {
    match a.len.load(Relaxed) {
        0 => b"",
        len => conv::mapped_bytes(a.addr.load(Relaxed), len),
    }
}

/// Under [`LOCK`]: make `level` (above 4) print a copy of `text`, or remove it for `None`.
/// False when there is nothing to remove, no free entry, or no memory for the copy.
fn define(level: i32, text: Option<&[u8]>) -> bool {
    let Some(text) = text else {
        return entry(level).map(|a| a.level.store(0, Relaxed)).is_some();
    };
    let Some(slot) = entry(level).or_else(|| entry(0)) else {
        return false;
    };
    let mut copy = TryVec::new();
    if copy.extend_from_slice(text).is_err() {
        return false;
    }
    let kept: &'static [u8] = copy.leak();
    slot.addr.store(kept.as_ptr().expose_provenance(), Relaxed);
    slot.len.store(kept.len(), Relaxed);
    slot.level.store(level, Relaxed);
    true
}

/// `s` split at the first `sep`.
fn split_once(s: &[u8], sep: u8) -> Option<(&[u8], &[u8])> {
    let (head, tail) = s.split_at(s.iter().position(|&b| b == sep)?);
    Some((head, tail.get(1..)?))
}

/// The fields a `MSGVERB` value selects.
fn msgverb(value: &[u8]) -> u32 {
    let body = value.strip_suffix(b":").unwrap_or(value);
    if body.is_empty() {
        return ALL;
    }
    let mut selected = 0;
    for word in body.split(|&b| b == b':') {
        match KEYWORDS.iter().find(|(k, _)| *k == word) {
            Some((_, bit)) => selected |= bit,
            None => return ALL,
        }
    }
    selected
}

/// A `SEV_LEVEL` element's level and print string; `None` when malformed or not above 4.
fn sev_level_element(element: &[u8]) -> Option<(i32, &[u8])> {
    let (_keyword, rest) = split_once(element, b',')?;
    let (level, string) = split_once(rest, b',')?;
    let parsed = parse_i64(level, 0, Dialect::C99);
    if !matches!(parsed.status, Status::Ok) || parsed.used != level.len() {
        return None;
    }
    let level = i32::try_from(parsed.value).ok()?;
    (level > LAST_STANDARD).then_some((level, string))
}

/// Read `MSGVERB` and `SEV_LEVEL`, the first time only.
fn read_environment() {
    if ENV_READ.load(Acquire) {
        return;
    }
    let _g = LOCK.lock(&CSys);
    if ENV_READ.load(Relaxed) {
        return;
    }
    let var = |name: &[u8]| process::getenv(name).map(strz::env_value);
    SELECTED.store(var(b"MSGVERB").map_or(ALL, msgverb), Relaxed);
    for element in var(b"SEV_LEVEL").unwrap_or(b"").split(|&b| b == b':') {
        if let Some((level, string)) = sev_level_element(element) {
            define(level, Some(string));
        }
    }
    ENV_READ.store(true, Release);
}

/// The print string of `severity`: `Some(None)` for `MM_NOSEV`, `None` for an unknown level.
fn severity_text(severity: i32) -> Option<Option<&'static [u8]>> {
    match severity {
        0 => Some(None),
        1..=LAST_STANDARD => STANDARD.get(usize::try_from(severity).ok()?).map(|s| Some(*s)),
        _ if severity > LAST_STANDARD => {
            let _g = LOCK.lock(&CSys);
            entry(severity).map(|a| Some(print_string(a)))
        }
        _ => None,
    }
}

/// Whether `label` is `component:subcomponent` of at most 10 and 14 bytes (split at the first
/// colon).
fn label_ok(label: &[u8]) -> bool {
    split_once(label, b':').is_some_and(|(component, sub)| component.len() <= 10 && sub.len() <= 14)
}

/// A message as its pieces, in order.
struct Message<'a> {
    parts: [&'a [u8]; 16],
    n: usize,
}

impl<'a> Message<'a> {
    /// The message of the present `fields` (in [`LAYOUT`] order) that `select` names.
    fn of(fields: &[Option<&'a [u8]>; 5], select: u32) -> Message<'a> {
        let mut m = Message { parts: [b""; 16], n: 0 };
        let mut separator: &[u8] = b"";
        for (&(bit, before, after), value) in LAYOUT.iter().zip(fields) {
            if let (true, Some(v)) = (select & bit != 0, value) {
                m.push(separator);
                m.push(before);
                m.push(v);
                separator = after;
            }
        }
        m.push(b"\n");
        m
    }

    fn push(&mut self, piece: &'a [u8]) {
        if let Some(slot) = self.parts.get_mut(self.n) {
            *slot = piece;
            self.n = self.n.saturating_add(1);
        }
    }

    fn pieces(&self) -> &[&'a [u8]] {
        self.parts.get(..self.n).unwrap_or(&[])
    }

    /// The message in one buffer, `None` without memory for it.
    fn joined(&self) -> Option<TryVec<u8>> {
        let mut whole = TryVec::new();
        self.pieces().iter().try_for_each(|p| whole.extend_from_slice(p)).ok()?;
        Some(whole)
    }
}

/// Write `m` to the current `stderr` stream in its orientation (an undecided one becomes
/// byte-oriented): whether it all went.
fn to_stderr(m: &Message<'_>) -> bool {
    let enc = Encoding::of(locale::current());
    let whole = m.joined();
    let r = stdio::with(stdio::stderr(), Locking::Locked, |g, file, ext| match &whole {
        Some(w) => sc::write_text(g, file, ext, w.as_slice(), true, enc),
        None => m.pieces().iter().try_for_each(|p| sc::write_text(g, file, ext, p, true, enc)),
    });
    matches!(r, Ok(Ok(())))
}

/// Write all of `data` to `fd`.
fn write_all(fd: Fd, mut data: &[u8]) -> bool {
    while !data.is_empty() {
        match CSys.write(fd, data) {
            Ok(0) => return false,
            Ok(n) => data = data.get(n..).unwrap_or(b""),
            Err(Errno::EINTR) => {}
            Err(_) => return false,
        }
    }
    true
}

/// Write `m` to `/dev/console`: whether it all went.
fn to_console(m: &Message<'_>) -> bool {
    let Ok(path) = KStr::from_nul_terminated(b"/dev/console\0") else {
        return false;
    };
    let Ok(fd) = CSys.openat(Fd::CWD, path, O_WRONLY | O_NOCTTY | O_CLOEXEC, 0) else {
        return false;
    };
    let ok = match m.joined() {
        Some(w) => write_all(fd, w.as_slice()),
        None => m.pieces().iter().all(|p| write_all(fd, p)),
    };
    let _ = CSys.close(fd);
    ok
}

/// `fmtmsg(classification, label, severity, text, action, tag)`.
pub fn fmtmsg(classification: i64, label: Option<&[u8]>, severity: i32, text: Option<&[u8]>, action: Option<&[u8]>, tag: Option<&[u8]>) -> i32 {
    if label.is_some_and(|l| !label_ok(l)) {
        return MM_NOTOK;
    }
    read_environment();
    let Some(severity) = severity_text(severity) else {
        return MM_NOTOK;
    };
    let fields = [label, severity, text, action, tag];
    let saved = errno::get();
    let printed = (classification & MM_PRINT != 0).then(|| to_stderr(&Message::of(&fields, SELECTED.load(Relaxed))));
    let consoled = (classification & MM_CONSOLE != 0).then(|| to_console(&Message::of(&fields, ALL)));
    errno::set_raw(saved);
    match (printed, consoled) {
        (Some(false), Some(false)) => MM_NOTOK,
        (Some(false), _) => MM_NOMSG,
        (_, Some(false)) => MM_NOCON,
        _ => MM_OK,
    }
}

/// `addseverity(severity, text)`: give `severity` (above 4) the print string `text` (a copy),
/// or remove it for NULL.
pub fn addseverity(severity: i32, text: Option<&[u8]>) -> i32 {
    if severity <= LAST_STANDARD {
        return MM_NOTOK;
    }
    let _g = LOCK.lock(&CSys);
    if define(severity, text) { MM_OK } else { MM_NOTOK }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msgverb_forms() {
        assert_eq!(msgverb(b"text:action"), TEXT | ACTION);
        assert_eq!(msgverb(b"tag:label:"), TAG | LABEL);
        for all in [&b""[..], b":", b"text::tag", b"text:bogus", b"TEXT", b"::"] {
            assert_eq!(msgverb(all), ALL, "{all:?}");
        }
    }

    #[test]
    fn sev_level_elements() {
        assert_eq!(sev_level_element(b"x,5,FIVE"), Some((5, &b"FIVE"[..])));
        assert_eq!(sev_level_element(b",0x10,A,B"), Some((16, &b"A,B"[..])));
        assert_eq!(sev_level_element(b"x,010,"), Some((8, &b""[..])));
        for bad in [&b"x,4,FOUR"[..], b"x,-7,N", b"x,9", b"x", b"x,,E", b"x,5z,E", b"x,2147483648,B"] {
            assert_eq!(sev_level_element(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn labels() {
        for good in [&b"ABCDEFGHIJ:abcdefghijklmn"[..], b":", b"a:b:c"] {
            assert!(label_ok(good));
        }
        for bad in [&b"ABCDEFGHIJK:x"[..], b"a:abcdefghijklmno", b"nocolon", b""] {
            assert!(!label_ok(bad));
        }
    }

    #[test]
    fn layout() {
        let all = [Some(&b"UX:cat"[..]), Some(&b"ERROR"[..]), Some(&b"text"[..]), Some(&b"act"[..]), Some(&b"tag"[..])];
        let text = |m: Message<'_>| m.joined().map(|w| w.as_slice().to_vec());
        assert_eq!(text(Message::of(&all, ALL)).as_deref(), Some(&b"UX:cat: ERROR: text\nTO FIX: act  tag\n"[..]));
        assert_eq!(text(Message::of(&all, TAG | LABEL)).as_deref(), Some(&b"UX:cat: tag\n"[..]));
        assert_eq!(text(Message::of(&[None; 5], ALL)).as_deref(), Some(&b"\n"[..]));
    }
}
