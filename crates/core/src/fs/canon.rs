//! Path canonicalization (spec §13.8; D-13.5).
//!
//! [`canonicalize`] resolves a path component by component. The common case looks each
//! component up by the name of the prefix resolved so far, one `readlinkat` per component and
//! no descriptor (see `by_name`). Once a name would reach `PATH_MAX` it starts again relative to
//! a descriptor for the directory resolved so far (an `O_PATH` descriptor, so only search
//! permission is needed, as for the kernel's own lookup). The result therefore has no length
//! limit (D-13.5); the C forms with a caller buffer apply `PATH_MAX` themselves.
//!
//! * `.` is dropped; `..` removes the last component of the result (the result is physical, so
//!   this is the real parent) without examining it, as the record shows for `rp/ns/..`.
//! * A symbolic link is replaced by its target (an absolute target restarts at `/`); at most
//!   [`MAX_LINKS`] links are followed in one call, the kernel's limit for one lookup.
//! * A component that is not a directory may only end the path: anything after it (another
//!   component, `.`, `..` or a slash) is `ENOTDIR`.
//! * A failure caused by a component that is missing (`ENOENT`) or cannot be examined
//!   (`EACCES`) carries the prefix resolved so far, including that component (spec §13.8
//!   "the buffer holds the prefix that was resolved"; when the working directory itself cannot
//!   be named the prefix is empty). Other failures (`ENOTDIR`, `ELOOP`, `ENAMETOOLONG`, ...)
//!   carry none.

use crate::error::{Errno, Result};
use crate::fs::cwd;
use crate::mem::TryVec;
use liella_kernel::fd::Fd;
use liella_kernel::path::{KStr, PATH_MAX};
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{AT_SYMLINK_NOFOLLOW, O_CLOEXEC, O_DIRECTORY, O_NOFOLLOW, O_PATH, S_IFDIR, S_IFLNK, S_IFMT};

/// The most symbolic links one resolution follows (the kernel's `MAXSYMLINKS`, ⚑ KERN).
pub const MAX_LINKS: u32 = 40;

/// A failed resolution: the error, and the prefix resolved (see the module documentation).
#[derive(Debug)]
pub struct CanonError {
    /// The error.
    pub errno: Errno,
    /// The resolved prefix, for `ENOENT` and `EACCES`.
    pub prefix: Option<TryVec<u8>>,
}

impl From<Errno> for CanonError {
    fn from(errno: Errno) -> CanonError {
        CanonError { errno, prefix: None }
    }
}

fn z(text: &[u8]) -> Result<TryVec<u8>> {
    let mut v = TryVec::with_capacity(text.len().saturating_add(1))?;
    v.extend_from_slice(text)?;
    v.push(0)?;
    Ok(v)
}

fn open_dir<S: Sys>(sys: &S, dir: Fd, name: &[u8]) -> Result<Fd> {
    let n = z(name)?;
    sys.openat(dir, KStr::from_nul_terminated(&n)?, O_PATH | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC, 0)
}

/// Open the directory `path` (absolute, physical, `""` for `/`) component by component.
fn reopen<S: Sys>(sys: &S, path: &[u8]) -> Result<Fd> {
    let mut fd = sys.openat(Fd::CWD, KStr::from_nul_terminated(b"/\0")?, O_PATH | O_DIRECTORY | O_CLOEXEC, 0)?;
    for c in path.split(|&b| b == b'/').filter(|c| !c.is_empty()) {
        let next = open_dir(sys, fd, c);
        let _ = sys.close(fd);
        fd = next?;
    }
    Ok(fd)
}

/// The error for a failure at the prefix `result`.
fn failed(errno: Errno, result: &[u8]) -> CanonError {
    let prefix = if errno == Errno::ENOENT || errno == Errno::EACCES { TryVec::from_slice(if result.is_empty() { b"/" } else { result }).ok() } else { None };
    CanonError { errno, prefix }
}

/// `canonicalize_file_name(path)`: the absolute path without `.`, `..` or symbolic-link
/// components that names the same file (it must exist). See the module documentation.
pub fn canonicalize<S: Sys>(sys: &S, path: &[u8]) -> core::result::Result<TryVec<u8>, CanonError> {
    if path.is_empty() {
        return Err(Errno::ENOENT.into());
    }
    // The one scan for a NUL: every name `by_name` builds is NUL-free from here on (see `lookup`).
    if path.contains(&0) {
        return Err(Errno::EINVAL.into());
    }
    match by_name(sys, path) {
        Ok(r) => Ok(r),
        Err(Named::Failed(e)) => Err(e),
        Err(Named::TooLong) => by_descriptor(sys, path),
    }
}

/// Why the name-based resolution stopped.
enum Named {
    /// A final answer.
    Failed(CanonError),
    /// A name to look up reached `PATH_MAX`: start again with the descriptor walk.
    TooLong,
}

impl From<CanonError> for Named {
    fn from(e: CanonError) -> Named {
        Named::Failed(e)
    }
}

impl From<Errno> for Named {
    fn from(e: Errno) -> Named {
        Named::Failed(e.into())
    }
}

/// The common case: each component is examined with ONE `readlinkat` on the name of the prefix
/// resolved so far (absolute, or relative to the working directory while the result is still
/// below it), which also follows a symbolic link (`EINVAL` = it exists and is not a link). No
/// descriptor is opened. The prefix holds no `.`, `..` or links, so the kernel's lookup of it
/// names the same directories the descriptor walk would open, with the same permission checks.
/// A component that is not a directory is caught by the next lookup (`ENOTDIR`), or, when only
/// `.`, `..` or slashes follow it, by one `lstat` of it (no search permission needed). Names reaching
/// `PATH_MAX` go to [`by_descriptor`] (no length limit, D-13.5).
fn by_name<S: Sys>(sys: &S, path: &[u8]) -> core::result::Result<TryVec<u8>, Named> {
    // `result` is the resolved prefix ("" stands for "/"); with `rel = Some(k)` the lookup name
    // is `result[k + 1..]` relative to the working directory (`result[..k]` is its name).
    let (mut result, mut rel) = if path.first() == Some(&b'/') {
        (TryVec::with_capacity(path.len().saturating_add(1))?, None)
    } else {
        let mut c = match cwd::current(sys) {
            Ok(c) => c,
            Err(e) if e == Errno::ENOENT || e == Errno::EACCES => return Err(CanonError { errno: e, prefix: Some(TryVec::with_capacity(0)?) }.into()),
            Err(e) => return Err(e.into()),
        };
        if c.as_slice() == b"/" {
            c.clear();
            (c, None)
        } else {
            let k = c.len();
            (c, Some(k))
        }
    };
    let mut rest = TryVec::from_slice(path)?;
    let mut pos = 0usize;
    let mut links = 0u32;
    // Link targets: most fit the small buffer; a longer one is read again into `target`.
    let mut small = [0u8; 256];
    let mut target: Option<TryVec<u8>> = None;
    // A component that exists, is not a link and was followed by more of the path: it must be
    // a directory, which the next lookup in it proves.
    let mut unproven = false;
    loop {
        let tail = rest.get(pos..).unwrap_or(&[]);
        let skip = tail.iter().position(|&b| b != b'/').unwrap_or(tail.len());
        let tail = tail.get(skip..).unwrap_or(&[]);
        let clen = tail.iter().position(|&b| b == b'/').unwrap_or(tail.len());
        let comp_at = pos.saturating_add(skip);
        let comp_end = comp_at.saturating_add(clen);
        let comp = rest.get(comp_at..comp_end).unwrap_or(&[]);
        let named = !comp.is_empty() && comp != b"." && comp != b"..";
        if unproven && !named {
            // Examined without searching it (`..` after an unsearchable directory is fine).
            unproven = false;
            match lookup(sys, &mut result, rel, None) {
                Ok(1) => {}
                Ok(_) => return Err(Errno::ENOTDIR.into()),
                Err(Errno::ENAMETOOLONG) => return Err(Named::TooLong),
                Err(e) => return Err(failed(e, &result).into()),
            }
        }
        if comp.is_empty() {
            if result.is_empty() {
                result.push(b'/')?;
            }
            return Ok(result);
        }
        pos = comp_end;
        if comp == b"." {
            continue;
        }
        if comp == b".." {
            let cut = result.iter().rposition(|&b| b == b'/').unwrap_or(0);
            result.truncate(cut);
            if rel.is_some_and(|k| cut < k) {
                rel = None;
            }
            continue;
        }
        unproven = false;
        let mark = result.len();
        result.push(b'/')?;
        result.extend_from_slice(comp)?;
        let more = pos < rest.len();
        let n = match lookup(sys, &mut result, rel, Some(&mut small)) {
            Err(Errno::EINVAL) => {
                unproven = more;
                continue;
            }
            Err(Errno::ENAMETOOLONG) => return Err(Named::TooLong),
            Err(e) => return Err(failed(e, &result).into()),
            Ok(n) => n,
        };
        links = links.saturating_add(1);
        if links > MAX_LINKS {
            return Err(Errno::ELOOP.into());
        }
        let t = if n < small.len() {
            small.get(..n).unwrap_or(&[])
        } else {
            // The target may have been cut: read it again whole (a link is at most PATH_MAX).
            let buf = match target.as_mut() {
                Some(t) => t,
                None => {
                    let mut t = TryVec::with_capacity(PATH_MAX)?;
                    t.resize(PATH_MAX, 0)?;
                    target.insert(t)
                }
            };
            let n = lookup(sys, &mut result, rel, Some(buf.as_mut_slice())).map_err(|e| failed(e, &result))?;
            buf.get(..n).unwrap_or(&[])
        };
        if t.is_empty() {
            return Err(failed(Errno::ENOENT, &result).into());
        }
        result.truncate(mark);
        if t.first() == Some(&b'/') {
            result.clear();
            rel = None;
        }
        let left = rest.get(pos..).unwrap_or(&[]);
        let mut joined = TryVec::with_capacity(t.len().saturating_add(left.len()))?;
        joined.extend_from_slice(t)?;
        joined.extend_from_slice(left)?;
        rest = joined;
        pos = 0;
    }
}

/// `readlinkat` of the lookup name of `result` (see [`by_name`]) into `buf`, or with `None`
/// 1 if `lstat` finds a directory, else 0. `ENAMETOOLONG` when the name reaches `PATH_MAX`.
fn lookup<S: Sys>(sys: &S, result: &mut TryVec<u8>, rel: Option<usize>, buf: Option<&mut [u8]>) -> Result<usize> {
    let from = rel.map_or(0, |k| k.saturating_add(1));
    if result.len().saturating_sub(from) >= PATH_MAX {
        return Err(Errno::ENAMETOOLONG);
    }
    result.push(0)?;
    // `result` is NUL-free (the path was checked once in `canonicalize`; the working directory and
    // link targets come from the kernel as C strings, ⚑ KERN), so only the terminator is checked:
    // a scan per lookup would cost O(depth) per component, O(depth^2) per call (REALPATH-DEPTH).
    let r = match KStr::from_nul_free_terminated(result.get(from..).unwrap_or(&[])) {
        Ok(name) => match buf {
            Some(b) => sys.readlinkat(Fd::CWD, name, b),
            None => sys.fstatat(Fd::CWD, name, AT_SYMLINK_NOFOLLOW).map(|st| usize::from(st.mode & S_IFMT == S_IFDIR)),
        },
        Err(e) => Err(e),
    };
    result.pop();
    r
}

/// The descriptor walk (no length limit): each directory is opened (`O_PATH`) relative to the
/// last one.
fn by_descriptor<S: Sys>(sys: &S, path: &[u8]) -> core::result::Result<TryVec<u8>, CanonError> {
    // The resolved prefix ("" stands for "/") and a descriptor for it.
    let (mut result, start) = if path.first() == Some(&b'/') {
        (TryVec::with_capacity(path.len())?, reopen(sys, b"")?)
    } else {
        let c = match cwd::current(sys) {
            Ok(c) => c,
            Err(e) if e == Errno::ENOENT || e == Errno::EACCES => return Err(CanonError { errno: e, prefix: Some(TryVec::with_capacity(0)?) }),
            Err(e) => return Err(e.into()),
        };
        let mut r = c;
        if r.as_slice() == b"/" {
            r.clear();
        }
        let fd = sys.openat(Fd::CWD, KStr::from_nul_terminated(b".\0")?, O_PATH | O_DIRECTORY | O_CLOEXEC, 0)?;
        (r, fd)
    };
    let mut fd = start;
    let r = resolve(sys, &mut fd, &mut result, path);
    let _ = sys.close(fd);
    r?;
    if result.is_empty() {
        result.push(b'/')?;
    }
    Ok(result)
}

fn resolve<S: Sys>(sys: &S, fd: &mut Fd, result: &mut TryVec<u8>, path: &[u8]) -> core::result::Result<(), CanonError> {
    let mut rest = TryVec::from_slice(path)?;
    let mut links = 0u32;
    let mut target = TryVec::with_capacity(PATH_MAX)?;
    target.resize(PATH_MAX, 0)?;
    loop {
        let skip = rest.iter().position(|&b| b != b'/').unwrap_or(rest.len());
        let tail = rest.get(skip..).unwrap_or(&[]);
        if tail.is_empty() {
            return Ok(());
        }
        let clen = tail.iter().position(|&b| b == b'/').unwrap_or(tail.len());
        let comp = TryVec::from_slice(tail.get(..clen).unwrap_or(&[]))?;
        let after = TryVec::from_slice(tail.get(clen..).unwrap_or(&[]))?;
        let more = !after.is_empty();
        rest = after;
        match comp.as_slice() {
            b"." => continue,
            b".." => {
                let cut = result.iter().rposition(|&b| b == b'/').unwrap_or(0);
                result.truncate(cut);
                let next = reopen(sys, result).map_err(|e| failed(e, result))?;
                let _ = sys.close(*fd);
                *fd = next;
                continue;
            }
            _ => {}
        }
        let zc = z(&comp)?;
        let name = KStr::from_nul_terminated(&zc)?;
        let mark = result.len();
        result.push(b'/')?;
        result.extend_from_slice(&comp)?;
        // With more to resolve, a directory is opened at once (one call instead of a stat and
        // an open); the kernel refuses anything else with ENOTDIR (with O_NOFOLLOW a symbolic
        // link too), which is then examined below. Other errors are the ones a stat gives.
        if more {
            match sys.openat(*fd, name, O_PATH | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC, 0) {
                Ok(next) => {
                    let _ = sys.close(*fd);
                    *fd = next;
                    continue;
                }
                Err(Errno::ENOTDIR) => {}
                Err(e) => return Err(failed(e, result)),
            }
        }
        let st = sys.fstatat(*fd, name, AT_SYMLINK_NOFOLLOW).map_err(|e| failed(e, result))?;
        match st.mode & S_IFMT {
            S_IFLNK => {
                links = links.saturating_add(1);
                if links > MAX_LINKS {
                    return Err(Errno::ELOOP.into());
                }
                let n = sys.readlinkat(*fd, name, &mut target).map_err(|e| failed(e, result))?;
                let t = target.get(..n).unwrap_or(&[]);
                if t.is_empty() {
                    return Err(failed(Errno::ENOENT, result));
                }
                result.truncate(mark);
                if t.first() == Some(&b'/') {
                    result.clear();
                    let root = reopen(sys, b"")?;
                    let _ = sys.close(*fd);
                    *fd = root;
                }
                let mut joined = TryVec::with_capacity(t.len().saturating_add(rest.len()).saturating_add(1))?;
                joined.extend_from_slice(t)?;
                joined.extend_from_slice(&rest)?;
                rest = joined;
            }
            // The last component: nothing is looked up in it.
            S_IFDIR if !more => {}
            S_IFDIR => {
                let next = open_dir(sys, *fd, &comp).map_err(|e| failed(e, result))?;
                let _ = sys.close(*fd);
                *fd = next;
            }
            _ => {
                if more {
                    return Err(Errno::ENOTDIR.into());
                }
            }
        }
    }
}
