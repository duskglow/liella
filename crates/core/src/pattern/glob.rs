//! Path-name expansion (spec §14.8; XCU 2.13.3, glob(3), glob(7)): the paths matching a shell
//! pattern, with the standard flags and the GNU ones (`GLOB_PERIOD`, `GLOB_BRACE`,
//! `GLOB_NOMAGIC`, `GLOB_TILDE`, `GLOB_TILDE_CHECK`, `GLOB_ONLYDIR`, `GLOB_ALTDIRFUNC`).
//!
//! The file system is reached through an [`Access`]: the kernel ([`KernelAccess`]) or, for
//! `GLOB_ALTDIRFUNC`, the caller's directory functions (the boundary). The C structure
//! (`glob_t`, `GLOB_APPEND`, `GLOB_DOOFFS`, `gl_flags`) is the boundary's.
//!
//! # The walk
//!
//! * A pattern without metacharacters (an unescaped `*` or `?`, or an unescaped `[` that a `]`
//!   follows, [`is_magic`]) names one path: the path (escapes removed) is returned if it exists,
//!   checked with `lstat` (the current contract, `glob@@GLIBC_2.27`: a dangling symbolic link
//!   exists). A path that ends in `/` exists only as a directory (the kernel's answer).
//! * Otherwise the pattern is split at `/`. Components before the first one with metacharacters
//!   are a fixed prefix; each component with metacharacters is matched (with `fnmatch`, so
//!   brackets, classes and escapes are §14.7's) against the entries of every directory reached
//!   so far, in directory order; entries that more components follow must be directories
//!   (`d_type`, or `stat` when it is a link or unknown). Literal components after the last
//!   wildcard are appended and the whole path checked with `lstat`; a trailing `/` keeps only
//!   directories, and the slash.
//! * Leading periods: a wildcard matches a leading `.` only with `GLOB_PERIOD`, and only in the
//!   last component (directories reached through wildcards never start with `.`, so `.` and
//!   `..` are never walked through by a wildcard; observed).
//! * A directory that cannot be opened (any error but `ENOTDIR`) or read is reported to the
//!   error function; with `GLOB_ERR`, or when it returns nonzero, the call ends with
//!   `GLOB_ABORTED` and no results of its own.
//! * `GLOB_BRACE` expands `{a,b}` (nested, left to right, before anything else); each
//!   alternative is expanded and sorted on its own, and the groups keep the written order.
//!   `GLOB_TILDE` replaces a leading `~` or `~user` (up to the first `/`) with the home
//!   directory: `$HOME` when it is set and not empty, else the caller's passwd entry. An unknown
//!   user leaves the pattern as written (glob(3): "no substitution is performed"), or with
//!   `GLOB_TILDE_CHECK` makes the call `GLOB_NOMATCH`.
//! * Results are sorted in the locale's collation (`strcoll`), bytes breaking ties, unless
//!   `GLOB_NOSORT`; then they come in directory order (an order of the file system).
//! * Nothing found: `GLOB_NOCHECK` returns the pattern as given; `GLOB_NOMAGIC` does so only
//!   for a pattern without metacharacters; otherwise `GLOB_NOMATCH`.

#![allow(clippy::arithmetic_side_effects)] // offsets are bounded by the lengths of the pattern and the paths

use crate::error::{Errno, Result};
use crate::fs::dir::{DirStream, DT_DIR, DT_LNK, DT_UNKNOWN, STREAM_BUFFER};
use crate::locale::Locale;
use crate::mem::TryVec;
use liella_kernel::fd::Fd;
use liella_kernel::path::KPathBuf;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{AT_SYMLINK_NOFOLLOW, S_IFDIR, S_IFMT};

use super::fnmatch::{self, Pattern};

/// `GLOB_ERR`.
pub const ERR: i32 = 1;
/// `GLOB_MARK`.
pub const MARK: i32 = 1 << 1;
/// `GLOB_NOSORT`.
pub const NOSORT: i32 = 1 << 2;
/// `GLOB_DOOFFS`.
pub const DOOFFS: i32 = 1 << 3;
/// `GLOB_NOCHECK`.
pub const NOCHECK: i32 = 1 << 4;
/// `GLOB_APPEND`.
pub const APPEND: i32 = 1 << 5;
/// `GLOB_NOESCAPE`.
pub const NOESCAPE: i32 = 1 << 6;
/// `GLOB_PERIOD`.
pub const PERIOD: i32 = 1 << 7;
/// `GLOB_MAGCHAR` (set in `gl_flags`).
pub const MAGCHAR: i32 = 1 << 8;
/// `GLOB_ALTDIRFUNC`.
pub const ALTDIRFUNC: i32 = 1 << 9;
/// `GLOB_BRACE`.
pub const BRACE: i32 = 1 << 10;
/// `GLOB_NOMAGIC`.
pub const NOMAGIC: i32 = 1 << 11;
/// `GLOB_TILDE`.
pub const TILDE: i32 = 1 << 12;
/// `GLOB_ONLYDIR`.
pub const ONLYDIR: i32 = 1 << 13;
/// `GLOB_TILDE_CHECK`.
pub const TILDE_CHECK: i32 = 1 << 14;
/// Every flag bit `glob` knows; any other is `EINVAL` (decision: an error, never a guess at
/// what a future flag means).
pub const KNOWN_FLAGS: i32 = (1 << 15) - 1;

/// Why a call found nothing usable (the `GLOB_*` return values).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    /// `GLOB_NOSPACE` (1).
    NoSpace,
    /// `GLOB_ABORTED` (2).
    Aborted,
    /// `GLOB_NOMATCH` (3).
    NoMatch,
}

impl Failure {
    /// The return value.
    pub fn code(self) -> i32 {
        match self {
            Failure::NoSpace => 1,
            Failure::Aborted => 2,
            Failure::NoMatch => 3,
        }
    }
}

impl From<Errno> for Failure {
    fn from(_: Errno) -> Failure {
        Failure::NoSpace
    }
}

/// What a successful call found.
pub struct Found {
    /// The paths, in order.
    pub paths: TryVec<TryVec<u8>>,
    /// The pattern has metacharacters (`GLOB_MAGCHAR`).
    pub magic: bool,
}

/// The file system as `glob` sees it.
pub trait Access {
    /// An open directory.
    type Dir;
    /// Open the directory `path` (`.` for the working directory).
    fn open(&mut self, path: &[u8]) -> Result<Self::Dir>;
    /// The next entry of `dir`: its name in `name` and its `d_type`; `None` at the end.
    fn read(&mut self, dir: &mut Self::Dir, name: &mut TryVec<u8>) -> Result<Option<u8>>;
    /// Close `dir`.
    fn close(&mut self, dir: Self::Dir);
    /// The mode of `path`, not following a final symbolic link.
    fn lstat(&mut self, path: &[u8]) -> Result<u32>;
    /// The mode of `path`, following symbolic links.
    fn stat(&mut self, path: &[u8]) -> Result<u32>;
    /// Report an error with directory `path` (the error function); `true` to abort the walk.
    fn error(&mut self, path: &[u8], e: Errno) -> bool;
    /// The home directory of `user` (`None`: the caller's); `Ok(None)` when unknown.
    fn home(&mut self, user: Option<&[u8]>) -> Result<Option<TryVec<u8>>>;
    /// Whether `GLOB_NOCHECK` on a plain name (no metacharacter, backslash, brace, tilde,
    /// `GLOB_MARK` or `GLOB_ONLYDIR`) returns the name without an existence check
    /// (`glob@GLIBC_2.2.5`, LG-2: neither stat function is called). The result is the same
    /// either way; only the caller's callbacks can tell.
    fn nocheck_skips_existence(&self) -> bool {
        false
    }
}

/// Whether `pattern` has a metacharacter: an unescaped `*` or `?`, or an unescaped `[` that a
/// `]` follows (`glob_pattern_p`; also `GLOB_MAGCHAR` and `GLOB_NOMAGIC`). With `escapes`, a
/// backslash quotes the next character.
pub fn is_magic(pattern: &[u8], escapes: bool) -> bool {
    let mut i = 0;
    let mut open = false;
    while let Some(&b) = pattern.get(i) {
        match b {
            b'\\' if escapes => i += 1,
            b'*' | b'?' => return true,
            b'[' => open = true,
            b']' if open => return true,
            _ => {}
        }
        i += 1;
    }
    false
}

/// `bytes` with backslash escapes removed; `None` when it ends with an unescaped backslash
/// (such a pattern matches nothing, as in `fnmatch`).
fn unescape(bytes: &[u8], escapes: bool) -> Result<Option<TryVec<u8>>> {
    let mut out = TryVec::with_capacity(bytes.len())?;
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'\\' && escapes {
            match bytes.get(i + 1) {
                Some(&n) => out.push(n)?,
                None => return Ok(None),
            }
            i += 2;
        } else {
            out.push(b)?;
            i += 1;
        }
    }
    Ok(Some(out))
}

/// The index of the brace that closes the `{` at `open`, nesting and escapes considered.
fn brace_close(p: &[u8], open: usize, escapes: bool) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = open;
    while let Some(&b) = p.get(i) {
        match b {
            b'\\' if escapes => i += 1,
            b'{' => depth += 1,
            b'}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// `GLOB_BRACE`: the alternatives of `p`, in order, appended to `out`; `false` for unbalanced
/// braces.
fn braces(p: &[u8], escapes: bool, out: &mut TryVec<TryVec<u8>>) -> Result<bool> {
    let mut i = 0;
    let open = loop {
        match p.get(i) {
            None => {
                out.push(TryVec::from_slice(p)?)?;
                return Ok(true);
            }
            Some(b'\\') if escapes => i += 2,
            Some(b'{') => break i,
            Some(_) => i += 1,
        }
    };
    let Some(close) = brace_close(p, open, escapes) else { return Ok(false) };
    let (head, inner, tail) = (p.get(..open).unwrap_or(&[]), p.get(open + 1..close).unwrap_or(&[]), p.get(close + 1..).unwrap_or(&[]));
    let mut start = 0;
    let mut depth = 0usize;
    let mut j = 0;
    let mut alts: TryVec<(usize, usize)> = TryVec::new();
    while let Some(&b) = inner.get(j) {
        match b {
            b'\\' if escapes => j += 1,
            b'{' => depth += 1,
            b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                alts.push((start, j))?;
                start = j + 1;
            }
            _ => {}
        }
        j += 1;
    }
    alts.push((start, inner.len()))?;
    for &(a, b) in alts.iter() {
        let mut one = TryVec::from_slice(head)?;
        one.extend_from_slice(inner.get(a..b).unwrap_or(&[]))?;
        one.extend_from_slice(tail)?;
        if !braces(&one, escapes, out)? {
            return Ok(false);
        }
    }
    Ok(true)
}

/// What tilde expansion did to one alternative.
enum Tilde {
    /// The pattern to expand.
    Pattern(TryVec<u8>),
    /// `GLOB_TILDE_CHECK` and an unknown user.
    Unknown,
}

fn tilde<A: Access>(p: &[u8], flags: i32, acc: &mut A) -> Result<Tilde> {
    let escapes = flags & NOESCAPE == 0;
    if flags & (TILDE | TILDE_CHECK) == 0 || p.first() != Some(&b'~') {
        return Ok(Tilde::Pattern(TryVec::from_slice(p)?));
    }
    let end = p.iter().position(|&b| b == b'/').unwrap_or(p.len());
    let prefix = p.get(1..end).unwrap_or(&[]);
    if prefix.iter().any(|&b| matches!(b, b'*' | b'?' | b'[')) {
        return Ok(Tilde::Pattern(TryVec::from_slice(p)?));
    }
    // The user name, escapes removed (a backslash before the `/` quotes nothing that remains).
    let mut user = TryVec::new();
    let mut i = 0;
    while let Some(&b) = prefix.get(i) {
        if b == b'\\' && escapes {
            if let Some(&n) = prefix.get(i + 1) {
                user.push(n)?;
            }
            i += 2;
        } else {
            user.push(b)?;
            i += 1;
        }
    }
    // A lookup that fails is a home that cannot be determined (glob(3)); only a lack of memory
    // is the call's error.
    let home = match acc.home(if user.is_empty() { None } else { Some(&user) }) {
        Ok(h) => h,
        Err(Errno::ENOMEM) => return Err(Errno::ENOMEM),
        Err(_) => None,
    };
    let Some(home) = home else {
        return Ok(if flags & TILDE_CHECK != 0 { Tilde::Unknown } else { Tilde::Pattern(TryVec::from_slice(p)?) });
    };
    let mut out = TryVec::new();
    for &b in home.iter() {
        if escapes && matches!(b, b'*' | b'?' | b'[' | b']' | b'\\') {
            out.push(b'\\')?;
        }
        out.push(b)?;
    }
    out.extend_from_slice(p.get(end..).unwrap_or(&[]))?;
    Ok(Tilde::Pattern(out))
}

/// A call's result, or why it failed.
type Outcome<T> = core::result::Result<T, Failure>;
/// Matching directory entries: name and `d_type`.
struct Entries {
    /// The names, back to back.
    names: TryVec<u8>,
    /// Each entry: where its name starts and ends in `names`, and its `d_type`.
    spans: TryVec<(usize, usize, u8)>,
}

impl Entries {
    fn new() -> Entries {
        Entries { names: TryVec::new(), spans: TryVec::new() }
    }

    fn add(&mut self, name: &[u8], kind: u8) -> Result<()> {
        let start = self.names.len();
        self.names.extend_from_slice(name)?;
        self.spans.push((start, self.names.len(), kind))
    }

    fn iter(&self) -> impl Iterator<Item = (&[u8], u8)> {
        self.spans.iter().map(|&(a, b, k)| (self.names.as_slice().get(a..b).unwrap_or(&[]), k))
    }
}
/// A home-directory lookup (`None`: the caller's).
pub type HomeFn<'a> = &'a mut dyn FnMut(Option<&[u8]>) -> Result<Option<TryVec<u8>>>;

fn is_dir(mode: u32) -> bool {
    mode & S_IFMT == S_IFDIR
}

struct Walk<'a, A: Access> {
    acc: &'a mut A,
    flags: i32,
    loc: &'a Locale,
}

impl<A: Access> Walk<'_, A> {
    fn escapes(&self) -> bool {
        self.flags & NOESCAPE == 0
    }

    /// Whether the entry `path` (of type `kind`) is a directory.
    fn entry_is_dir(&mut self, path: &[u8], kind: u8) -> bool {
        match kind {
            DT_DIR => true,
            DT_LNK | DT_UNKNOWN => self.acc.stat(path).is_ok_and(is_dir),
            _ => false,
        }
    }

    /// The directory to open for a prefix (`tree/` -> `tree`, `/` -> `/`, empty -> `.`).
    fn dir_of(prefix: &[u8]) -> &[u8] {
        let mut end = prefix.len();
        while end > 0 && prefix.get(end - 1) == Some(&b'/') {
            end -= 1;
        }
        match (end, prefix.is_empty()) {
            (0, true) => b".",
            (0, false) => b"/",
            _ => prefix.get(..end).unwrap_or(b"."),
        }
    }

    /// The entries of `prefix`'s directory that `pat` matches, as `(name, d_type)`; `Ok(None)`
    /// when the walk must stop (`GLOB_ABORTED`).
    fn list(&mut self, prefix: &[u8], pat: &Pattern) -> Outcome<Option<Entries>> {
        let dir = Self::dir_of(prefix);
        let mut out = Entries::new();
        let mut d = match self.acc.open(dir) {
            Ok(d) => d,
            Err(Errno::ENOMEM) => return Err(Failure::NoSpace),
            Err(Errno::ENOTDIR) => return Ok(Some(out)),
            Err(e) => return self.report(dir, e).map(|go| go.then_some(out)),
        };
        let mut name = TryVec::new();
        let r = loop {
            match self.acc.read(&mut d, &mut name) {
                Ok(Some(kind)) => {
                    let hit = match pat.matches(&name, self.loc) {
                        Ok(h) => h,
                        Err(_) => break Err(Failure::NoSpace),
                    };
                    if hit && out.add(&name, kind).is_err() {
                        break Err(Failure::NoSpace);
                    }
                }
                Ok(None) => break Ok(true),
                Err(Errno::ENOMEM) => break Err(Failure::NoSpace),
                Err(e) => break self.report(dir, e),
            }
        };
        self.acc.close(d);
        Ok(r?.then_some(out))
    }

    /// Report a directory error: `Ok(true)` to go on.
    fn report(&mut self, dir: &[u8], e: Errno) -> Outcome<bool> {
        let abort = self.acc.error(dir, e);
        if abort || self.flags & ERR != 0 { Err(Failure::Aborted) } else { Ok(true) }
    }

    /// Mark (`GLOB_MARK`) and filter (`GLOB_ONLYDIR`) a found path.
    fn finish(&mut self, mut path: TryVec<u8>, kind: Option<u8>, out: &mut TryVec<TryVec<u8>>) -> Result<()> {
        if self.flags & (MARK | ONLYDIR) != 0 {
            let dir = match kind {
                Some(k) => self.entry_is_dir(&path, k),
                None => self.acc.stat(&path).is_ok_and(is_dir),
            };
            if self.flags & ONLYDIR != 0 && !dir {
                return Ok(());
            }
            if self.flags & MARK != 0 && dir && path.last() != Some(&b'/') {
                path.push(b'/')?;
            }
        }
        out.push(path)
    }

    /// Expand one pattern (no braces, tilde done): the paths, unsorted, appended to `out`.
    fn expand(&mut self, p: &[u8], out: &mut TryVec<TryVec<u8>>) -> Outcome<()> {
        let escapes = self.escapes();
        if p.is_empty() {
            return Ok(());
        }
        if !is_magic(p, escapes) {
            let Some(path) = unescape(p, escapes)? else { return Ok(()) };
            if self.acc.lstat(&path).is_ok() {
                let mut path = path;
                if self.flags & MARK != 0 && path.last() != Some(&b'/') && self.acc.stat(&path).is_ok_and(is_dir) {
                    path.push(b'/')?;
                }
                out.push(path)?;
            }
            return Ok(());
        }
        let lead = p.iter().take_while(|&&b| b == b'/').count();
        let rest = p.get(lead..).unwrap_or(&[]);
        let mut comps: TryVec<&[u8]> = TryVec::new();
        for c in rest.split(|&b| b == b'/') {
            comps.push(c)?;
        }
        let n = comps.len();
        let first = comps.iter().position(|c| is_magic(c, escapes)).unwrap_or(0);
        let last = comps.iter().rposition(|c| is_magic(c, escapes)).unwrap_or(0);
        // A literal component, escapes removed; a backslash just before a `/` quoted the
        // separator and is dropped, one ending the pattern makes it match nothing (`None`).
        let literal = |k: usize, c: &[u8]| -> Result<Option<TryVec<u8>>> {
            match unescape(c, escapes)? {
                Some(u) => Ok(Some(u)),
                None if k + 1 < n => unescape(c.get(..c.len().saturating_sub(1)).unwrap_or(&[]), escapes),
                None => Ok(None),
            }
        };
        // The fixed prefix.
        let mut prefix = TryVec::from_slice(p.get(..lead).unwrap_or(&[]))?;
        for (k, c) in comps.iter().enumerate().take(first) {
            prefix.extend_from_slice(&literal(k, c)?.unwrap_or_else(TryVec::new))?;
            prefix.push(b'/')?;
        }
        // The literal components after the last wildcard, each with the `/` before it.
        let mut tail: Option<TryVec<u8>> = None;
        if last + 1 < n {
            let mut t = TryVec::new();
            let mut ok = true;
            for (k, c) in comps.iter().enumerate().skip(last + 1) {
                t.push(b'/')?;
                match literal(k, c)? {
                    Some(u) => t.extend_from_slice(&u)?,
                    None => ok = false,
                }
            }
            if !ok {
                return Ok(());
            }
            tail = Some(t);
        }
        let trailing_slash = comps.last().is_some_and(|c| c.is_empty());
        let mut level: TryVec<TryVec<u8>> = TryVec::new();
        level.push(prefix)?;
        for k in first..=last {
            let comp = comps.get(k).copied().unwrap_or(&[]);
            let mut next = TryVec::new();
            if !is_magic(comp, escapes) {
                // A literal component between wildcards: kept where it names a directory.
                let Some(u) = literal(k, comp)? else { return Ok(()) };
                for pre in level.iter() {
                    let mut d = pre.try_clone()?;
                    d.extend_from_slice(&u)?;
                    if self.acc.stat(&d).is_ok_and(is_dir) {
                        d.push(b'/')?;
                        next.push(d)?;
                    }
                }
                level = next;
                continue;
            }
            // Leading periods: GLOB_PERIOD only in the last component.
            let mut fl = if escapes { 0 } else { fnmatch::NOESCAPE };
            if k + 1 < n || self.flags & PERIOD == 0 {
                fl |= fnmatch::PERIOD;
            }
            let pat = Pattern::new(comp, fl, self.loc)?;
            for pre in level.iter() {
                let Some(entries) = self.list(pre, &pat)? else { return Err(Failure::Aborted) };
                for (name, kind) in entries.iter() {
                    // One allocation: room for the name, a `/` and a literal tail.
                    let mut path = TryVec::with_capacity(pre.len() + name.len() + 1 + tail.as_ref().map_or(0, |t| t.len()))?;
                    path.extend_from_slice(pre)?;
                    path.extend_from_slice(name)?;
                    if k < last {
                        if self.entry_is_dir(&path, kind) {
                            path.push(b'/')?;
                            next.push(path)?;
                        }
                        continue;
                    }
                    match &tail {
                        None => self.finish(path, Some(kind), &mut next)?,
                        // Only a trailing `/`: the directories among the entries, with the slash.
                        Some(t) if t.as_slice() == b"/" => {
                            if self.entry_is_dir(&path, kind) {
                                path.push(b'/')?;
                                next.push(path)?;
                            }
                        }
                        Some(t) => {
                            path.extend_from_slice(t)?;
                            if self.acc.lstat(&path).is_ok() {
                                if trailing_slash {
                                    next.push(path)?;
                                } else {
                                    self.finish(path, None, &mut next)?;
                                }
                            }
                        }
                    }
                }
            }
            level = next;
        }
        // The paths move; nothing is copied.
        for path in level.into_inner() {
            out.push(path)?;
        }
        Ok(())
    }
}

/// `glob(pattern, flags)` over `acc` in `loc` (flags already validated against
/// [`KNOWN_FLAGS`]; `GLOB_APPEND`, `GLOB_DOOFFS` and `GLOB_ALTDIRFUNC` are the caller's).
pub fn glob<A: Access>(pattern: &[u8], flags: i32, loc: &Locale, acc: &mut A) -> Outcome<Found> {
    let escapes = flags & NOESCAPE == 0;
    let magic = is_magic(pattern, escapes);
    if flags & NOCHECK != 0
        && flags & (MARK | ONLYDIR | BRACE | TILDE | TILDE_CHECK) == 0
        && !magic
        && !pattern.contains(&b'\\')
        && acc.nocheck_skips_existence()
    {
        let mut paths = TryVec::new();
        paths.push(TryVec::from_slice(pattern)?)?;
        return Ok(Found { paths, magic });
    }
    let mut alternatives = TryVec::new();
    if flags & BRACE != 0 {
        if !braces(pattern, escapes, &mut alternatives)? {
            alternatives.clear();
        }
    } else {
        alternatives.push(TryVec::from_slice(pattern)?)?;
    }
    let mut paths: TryVec<TryVec<u8>> = TryVec::new();
    for alt in alternatives.iter() {
        let p = match tilde(alt, flags, acc)? {
            Tilde::Pattern(p) => p,
            Tilde::Unknown => return Err(Failure::NoMatch),
        };
        let mut found = TryVec::new();
        Walk { acc, flags, loc }.expand(&p, &mut found)?;
        if flags & NOSORT == 0 {
            found.sort_unstable_by(|a, b| loc.compare_bytes(a, b).then_with(|| a.as_slice().cmp(b.as_slice())));
        }
        for f in found.into_inner() {
            paths.push(f)?;
        }
    }
    if paths.is_empty() {
        if flags & NOCHECK != 0 || (flags & NOMAGIC != 0 && !magic) {
            paths.push(TryVec::from_slice(pattern)?)?;
        } else {
            return Err(Failure::NoMatch);
        }
    }
    Ok(Found { paths, magic })
}

/// The file system through the kernel, relative to the working directory, with the caller's
/// error function and home-directory lookup.
pub struct KernelAccess<'a, S: Sys> {
    /// The kernel.
    pub sys: &'a S,
    /// The error function: `true` to abort.
    pub on_error: &'a mut dyn FnMut(&[u8], Errno) -> bool,
    /// The home-directory lookup.
    pub on_home: HomeFn<'a>,
}

impl<S: Sys> Access for KernelAccess<'_, S> {
    type Dir = DirStream;

    fn open(&mut self, path: &[u8]) -> Result<DirStream> {
        let k = KPathBuf::new(path)?;
        DirStream::open_growing(self.sys, Fd::CWD, k.as_kstr(), 4096, STREAM_BUFFER)
    }

    fn read(&mut self, dir: &mut DirStream, name: &mut TryVec<u8>) -> Result<Option<u8>> {
        let Some(e) = dir.next(self.sys)? else { return Ok(None) };
        name.clear();
        name.extend_from_slice(dir.name(&e))?;
        Ok(Some(e.kind))
    }

    fn close(&mut self, dir: DirStream) {
        let _ = dir.close(self.sys);
    }

    fn lstat(&mut self, path: &[u8]) -> Result<u32> {
        let k = KPathBuf::new(path)?;
        Ok(self.sys.fstatat(Fd::CWD, k.as_kstr(), AT_SYMLINK_NOFOLLOW)?.mode)
    }

    fn stat(&mut self, path: &[u8]) -> Result<u32> {
        let k = KPathBuf::new(path)?;
        Ok(self.sys.fstatat(Fd::CWD, k.as_kstr(), 0)?.mode)
    }

    fn error(&mut self, path: &[u8], e: Errno) -> bool {
        (self.on_error)(path, e)
    }

    fn home(&mut self, user: Option<&[u8]>) -> Result<Option<TryVec<u8>>> {
        (self.on_home)(user)
    }
}
