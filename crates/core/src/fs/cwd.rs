//! The working directory (spec §13.1).
//!
//! The kernel's `getcwd` answers for paths up to a page; a longer working directory (the
//! kernel says `ENAMETOOLONG`) is named by walking up with `..` ([`walk_up`]): each parent is
//! opened relative to its child and searched for the entry with the child's device and inode,
//! so no path longer than one component is ever formed. A working directory the kernel reports
//! as `(unreachable)…` (outside the root after `chroot`, or on a detached mount) or as removed is
//! `ENOENT`.

use crate::error::{Errno, Result};
use crate::fs::dir::DirStream;
use crate::mem::TryVec;
use liella_kernel::fd::Fd;
use liella_kernel::path::{KStr, PATH_MAX};
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Stat, AT_SYMLINK_NOFOLLOW, O_CLOEXEC, O_DIRECTORY, O_PATH, O_RDONLY, SEEK_SET};

fn kstr(z: &[u8]) -> Result<KStr<'_>> {
    KStr::from_nul_terminated(z)
}

/// `getcwd(buf, size)` with a caller buffer: the kernel's answer written into `buf` (so an
/// unreachable directory leaves the kernel's `(unreachable)…` text there, as the record shows),
/// or the walk's when the path is longer than the kernel names. The length without the
/// terminator. `EINVAL` for an empty buffer, `ERANGE` when the path and its terminator do not
/// fit (the buffer untouched), `ENOENT` for an unreachable or removed directory.
pub fn into_buffer<S: Sys>(sys: &S, buf: &mut [u8]) -> Result<usize> {
    if buf.is_empty() {
        return Err(Errno::EINVAL);
    }
    match sys.getcwd(buf) {
        Ok(n) => reachable(buf.get(..n.saturating_sub(1)).unwrap_or(&[])),
        Err(Errno::ENAMETOOLONG) => {
            let p = walk_up(sys)?;
            let dst = buf.get_mut(..p.len().saturating_add(1)).ok_or(Errno::ERANGE)?;
            let (text, nul) = dst.split_at_mut(p.len());
            text.copy_from_slice(&p);
            nul.fill(0);
            Ok(p.len())
        }
        Err(e) => Err(e),
    }
}

/// The length of a kernel answer, or `ENOENT` if it is not an absolute path.
fn reachable(text: &[u8]) -> Result<usize> {
    if text.first() == Some(&b'/') { Ok(text.len()) } else { Err(Errno::ENOENT) }
}

/// The working directory's absolute path (without terminator), of any length.
pub fn current<S: Sys>(sys: &S) -> Result<TryVec<u8>> {
    let mut b = TryVec::with_capacity(PATH_MAX)?;
    b.resize(PATH_MAX, 0)?;
    match sys.getcwd(&mut b) {
        Ok(n) => {
            let len = reachable(b.get(..n.saturating_sub(1)).unwrap_or(&[]))?;
            b.truncate(len);
            Ok(b)
        }
        Err(Errno::ENAMETOOLONG | Errno::ERANGE) => walk_up(sys),
        Err(e) => Err(e),
    }
}

/// `get_current_dir_name()`: `pwd` (the `PWD` variable) when it is absolute, has no `.` or `..`
/// component, and names the working directory (the same device and inode), which keeps the
/// symbolic links the user went through; otherwise [`current`] (owner ruling 2026-10-03, D-13.14;
/// glibc also accepts relative and dotted forms). The caller passes no `pwd` in secure-execution
/// mode.
pub fn named<S: Sys>(sys: &S, pwd: Option<&[u8]>) -> Result<TryVec<u8>> {
    let plain = |p: &&[u8]| p.first() == Some(&b'/') && !p.contains(&0) && !p.split(|&c| c == b'/').any(|c| c == b"." || c == b"..");
    if let Some(p) = pwd.filter(plain) {
        let mut z = TryVec::with_capacity(p.len().saturating_add(1))?;
        z.extend_from_slice(p)?;
        z.push(0)?;
        let same = match (sys.fstatat(Fd::CWD, kstr(&z)?, 0), sys.fstatat(Fd::CWD, kstr(b".\0")?, 0)) {
            (Ok(a), Ok(b)) => a.dev == b.dev && a.ino == b.ino,
            _ => false,
        };
        if same {
            z.truncate(p.len());
            return Ok(z);
        }
    }
    current(sys)
}

/// The working directory's path found by walking up through `..` (any length). `EACCES` when
/// an ancestor cannot be read, `ENOENT` when a directory is no longer in its parent.
pub fn walk_up<S: Sys>(sys: &S) -> Result<TryVec<u8>> {
    let mut cur = sys.openat(Fd::CWD, kstr(b".\0")?, O_PATH | O_DIRECTORY | O_CLOEXEC, 0)?;
    // The names from the working directory up, each followed by its length (8 bytes).
    let mut names: TryVec<u8> = TryVec::with_capacity(256)?;
    let r = (|| -> Result<()> {
        loop {
            let st = sys.fstat(cur)?;
            let parent = sys.openat(cur, kstr(b"..\0")?, O_RDONLY | O_DIRECTORY | O_CLOEXEC, 0)?;
            let step = (|| -> Result<bool> {
                let pst = sys.fstat(parent)?;
                if pst.dev == st.dev && pst.ino == st.ino {
                    return Ok(true);
                }
                let at = names.len();
                find_child(sys, parent, &st, pst.dev != st.dev, &mut names)?;
                let n = u64::try_from(names.len().saturating_sub(at)).unwrap_or(0);
                names.extend_from_slice(&n.to_ne_bytes())?;
                Ok(false)
            })();
            match step {
                Ok(false) => {
                    let _ = sys.close(cur);
                    cur = parent;
                }
                other => {
                    let _ = sys.close(parent);
                    return other.map(|_| ());
                }
            }
        }
    })();
    let _ = sys.close(cur);
    r?;
    let mut out = TryVec::with_capacity(names.len().saturating_add(1))?;
    let mut end = names.len();
    while end >= 8 {
        let lenb: [u8; 8] = names.get(end.saturating_sub(8)..end).and_then(|s| s.try_into().ok()).ok_or(Errno::EIO)?;
        let n = usize::try_from(u64::from_ne_bytes(lenb)).map_err(|_| Errno::EIO)?;
        let stop = end.saturating_sub(8);
        let start = stop.checked_sub(n).ok_or(Errno::EIO)?;
        out.push(b'/')?;
        out.extend_from_slice(names.get(start..stop).ok_or(Errno::EIO)?)?;
        end = start;
    }
    if out.is_empty() {
        out.push(b'/')?;
    }
    Ok(out)
}

/// Append to `out` the name under which `parent` holds the directory `child`: first among the
/// entries whose inode number matches, then (a mount point, or a file system whose entries do
/// not carry the inode) among all entries. `ENOENT` if there is none.
fn find_child<S: Sys>(sys: &S, parent: Fd, child: &Stat, all: bool, out: &mut TryVec<u8>) -> Result<()> {
    let mut d = DirStream::with_fd(parent, 8192, 0)?;
    let r = (|| -> Result<()> {
        for pass in 0..2 {
            if pass == 1 {
                d.seek(sys, 0)?;
            }
            while let Some(e) = d.next(sys)? {
                let name = d.name(&e);
                if name == b"." || name == b".." || (pass == 0 && !all && e.ino != child.ino) {
                    continue;
                }
                let Ok(st) = sys.fstatat(parent, kstr(d.name_z(&e))?, AT_SYMLINK_NOFOLLOW) else { continue };
                if st.dev == child.dev && st.ino == child.ino {
                    out.extend_from_slice(name)?;
                    return Ok(());
                }
            }
            if all {
                break;
            }
        }
        Err(Errno::ENOENT)
    })();
    // The descriptor is the caller's; only the position moved.
    let fd = d.into_fd();
    let _ = sys.lseek(fd, 0, SEEK_SET);
    r
}
