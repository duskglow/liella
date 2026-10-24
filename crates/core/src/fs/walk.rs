//! Tree walks (spec §13.3; D-13.4, D-13.13).
//!
//! The walks are `openat`-relative: a directory is opened relative to its parent's descriptor
//! and its entries are examined relative to its own, so no path is ever handed to the kernel
//! whole, the working directory is never changed, and there is no depth limit. The paths the
//! walk reports are built in memory and may be of any length.
//!
//! * [`Levels`] holds one descriptor per directory on the current path. When the process runs
//!   out of descriptors (`EMFILE`, `ENFILE`) the shallowest ones are closed, and reopened later
//!   from the nearest open ancestor, component by component, with their device and inode
//!   checked (a directory replaced meanwhile ends the walk with `ENOENT`). So the descriptor
//!   limit does not limit the depth either.
//! * [`list`] reads a directory whole, in the order the file system gives.
//! * [`walk`] is `nftw`'s walk: pre-order (or post-order) callbacks in directory order, each
//!   directory reported once (D-13.4; a directory reached a second time through a symbolic link
//!   is skipped silently, as the reference does), symbolic links followed unless physical.
//!   Its C conventions (`FTW_*` codes, `FTW_CHDIR`, `FTW_ACTIONRETVAL`) are in
//!   `liella_legacy::fs_tree`.

use crate::error::{Errno, Result};
use crate::fs::dir::DirStream;
use crate::mem::TryVec;
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Stat, AT_SYMLINK_NOFOLLOW, O_CLOEXEC, O_DIRECTORY, O_NOFOLLOW, O_RDONLY, S_IFDIR, S_IFLNK, S_IFMT};

/// The buffer a walk reads directories with.
const LIST_BUFFER: usize = 16 * 1024;

/// One entry of a [`Listing`].
#[derive(Clone, Copy, Debug)]
struct Item {
    start: usize,
    len: usize,
    kind: u8,
}

/// A directory's entries, in the order the file system produced them.
pub struct Listing {
    bytes: TryVec<u8>,
    items: TryVec<Item>,
}

impl Listing {
    /// An empty listing.
    pub fn empty() -> Listing {
        Listing { bytes: TryVec::from(alloc::vec::Vec::new()), items: TryVec::from(alloc::vec::Vec::new()) }
    }

    /// The number of entries.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// True without entries.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Entry `i`: its name with a terminator, and its `d_type`.
    pub fn get(&self, i: usize) -> Option<(&[u8], u8)> {
        let it = self.items.get(i)?;
        Some((self.bytes.get(it.start..it.start.checked_add(it.len)?.checked_add(1)?)?, it.kind))
    }
}

/// Read the directory open at `fd` from its current position to the end (`.` and `..` only if
/// `dots`). The descriptor stays open.
pub fn list<S: Sys>(sys: &S, fd: Fd, dots: bool) -> Result<Listing> {
    let mut d = DirStream::with_fd(fd, LIST_BUFFER, 0)?;
    let mut out = Listing { bytes: TryVec::with_capacity(256)?, items: TryVec::with_capacity(16)? };
    let r = (|| -> Result<()> {
        while let Some(e) = d.next(sys)? {
            let name = d.name(&e);
            if !dots && (name == b"." || name == b"..") {
                continue;
            }
            let start = out.bytes.len();
            out.bytes.extend_from_slice(d.name_z(&e))?;
            out.items.push(Item { start, len: name.len(), kind: e.kind })?;
        }
        Ok(())
    })();
    let _ = d.into_fd();
    r.map(|()| out)
}

fn same(a: &Stat, dev: u64, ino: u64) -> bool {
    a.dev == dev && a.ino == ino
}

/// One directory on the current path.
struct Level {
    fd: Option<Fd>,
    /// The component it was opened by (the whole root path for level 0), with a terminator.
    name: TryVec<u8>,
    flags: u32,
    dev: u64,
    ino: u64,
}

/// The descriptors of the directories on a walk's current path (see the module
/// documentation). Level 0 is opened relative to `base`; it is never closed early.
pub struct Levels {
    base: Fd,
    stack: TryVec<Level>,
}

fn out_of_descriptors(e: Errno) -> bool {
    e == Errno::EMFILE || e == Errno::ENFILE
}

impl Levels {
    /// No levels yet; the root is opened relative to `base`.
    pub fn new(base: Fd) -> Levels {
        Levels { base, stack: TryVec::from(alloc::vec::Vec::new()) }
    }

    /// The number of levels.
    pub fn depth(&self) -> usize {
        self.stack.len()
    }

    /// Whether a level is the directory `(dev, ino)`.
    pub fn contains(&self, dev: u64, ino: u64) -> bool {
        self.stack.iter().any(|l| l.dev == dev && l.ino == ino)
    }

    /// Close one descriptor among levels 1.. other than `keep` (the shallowest), to make room.
    fn evict<S: Sys>(&mut self, sys: &S, keep: usize) -> bool {
        for (i, l) in self.stack.iter_mut().enumerate().skip(1) {
            if i == keep {
                continue;
            }
            if let Some(fd) = l.fd.take() {
                let _ = sys.close(fd);
                return true;
            }
        }
        false
    }

    fn open_evicting<S: Sys>(&mut self, sys: &S, dir: Fd, name: &[u8], flags: u32, keep: usize) -> Result<Fd> {
        loop {
            match sys.openat(dir, KStr::from_nul_terminated(name)?, flags, 0) {
                Err(e) if out_of_descriptors(e) && self.evict(sys, keep) => continue,
                r => return r,
            }
        }
    }

    /// The descriptor of level `i`, reopened if it was closed to make room.
    pub fn fd<S: Sys>(&mut self, sys: &S, i: usize) -> Result<Fd> {
        if let Some(fd) = self.stack.get(i).and_then(|l| l.fd) {
            return Ok(fd);
        }
        let j = (0..i).rev().find(|&j| self.stack.get(j).is_some_and(|l| l.fd.is_some())).ok_or(Errno::EBADF)?;
        let mut cur = self.stack.get(j).and_then(|l| l.fd).ok_or(Errno::EBADF)?;
        let mut owned = false;
        for m in j.saturating_add(1)..=i {
            let (name, flags, dev, ino) = {
                let l = self.stack.get(m).ok_or(Errno::EBADF)?;
                (TryVec::from_slice(&l.name)?, l.flags, l.dev, l.ino)
            };
            let next = self.open_evicting(sys, cur, &name, flags, j);
            if owned {
                let _ = sys.close(cur);
            }
            cur = next?;
            owned = true;
            let st = sys.fstat(cur);
            if !st.is_ok_and(|s| same(&s, dev, ino)) {
                let _ = sys.close(cur);
                return Err(Errno::ENOENT);
            }
        }
        if let Some(l) = self.stack.get_mut(i) {
            l.fd = Some(cur);
        }
        Ok(cur)
    }

    /// Open the directory `name` (a terminated component, or the root path for the first
    /// level) below the deepest level and make it the new deepest level. `follow`: a symbolic
    /// link may be followed (else `O_NOFOLLOW`).
    pub fn push<S: Sys>(&mut self, sys: &S, name: &[u8], follow: bool) -> Result<(Fd, Stat)> {
        let flags = O_RDONLY | O_DIRECTORY | O_CLOEXEC | if follow { 0 } else { O_NOFOLLOW };
        let depth = self.stack.len();
        let parent = match depth.checked_sub(1) {
            Some(top) => self.fd(sys, top)?,
            None => self.base,
        };
        self.stack.reserve(1)?;
        let mut owned = TryVec::with_capacity(name.len())?;
        owned.extend_from_slice(name)?;
        let fd = self.open_evicting(sys, parent, name, flags, depth.saturating_sub(1))?;
        let st = match sys.fstat(fd) {
            Ok(s) => s,
            Err(e) => {
                let _ = sys.close(fd);
                return Err(e);
            }
        };
        self.stack.push(Level { fd: Some(fd), name: owned, flags, dev: st.dev, ino: st.ino })?;
        Ok((fd, st))
    }

    /// Close the deepest level.
    pub fn pop<S: Sys>(&mut self, sys: &S) {
        if let Some(l) = self.stack.pop() {
            if let Some(fd) = l.fd {
                let _ = sys.close(fd);
            }
        }
    }

    /// Close every level.
    pub fn close_all<S: Sys>(&mut self, sys: &S) {
        while !self.stack.is_empty() {
            self.pop(sys);
        }
    }
}

/// A set of directories (device, inode), for "each directory once".
struct Seen {
    slots: TryVec<Option<(u64, u64)>>,
    used: usize,
}

impl Seen {
    fn new() -> Seen {
        Seen { slots: TryVec::from(alloc::vec::Vec::new()), used: 0 }
    }

    fn slot(cap: usize, key: (u64, u64)) -> usize {
        let h = (key.1 ^ key.0.rotate_left(32)).wrapping_mul(0x9e37_79b9_7f4a_7c15);
        usize::try_from(h >> 32).unwrap_or(0).checked_rem(cap).unwrap_or(0)
    }

    /// Insert `key`: `false` if it was already present.
    fn insert(&mut self, key: (u64, u64)) -> Result<bool> {
        if self.used.saturating_add(1).saturating_mul(2) > self.slots.len() {
            let cap = self.slots.len().saturating_mul(2).max(64);
            let mut slots = TryVec::with_capacity(cap)?;
            slots.resize(cap, None)?;
            for k in self.slots.iter().flatten() {
                Self::place(&mut slots, *k);
            }
            self.slots = slots;
        }
        if !Self::place(&mut self.slots, key) {
            return Ok(false);
        }
        self.used = self.used.saturating_add(1);
        Ok(true)
    }

    fn place(slots: &mut [Option<(u64, u64)>], key: (u64, u64)) -> bool {
        let cap = slots.len();
        let mut i = Self::slot(cap, key);
        for _ in 0..cap {
            match slots.get_mut(i) {
                Some(s @ None) => {
                    *s = Some(key);
                    return true;
                }
                Some(Some(k)) if *k == key => return false,
                _ => i = i.saturating_add(1).checked_rem(cap).unwrap_or(0),
            }
        }
        false
    }
}

/// What a walk reports an entry as (the `FTW_*` type codes).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// `FTW_F`: not a directory or (physical walks) a symbolic link.
    File,
    /// `FTW_D`: a directory, before its contents.
    Dir,
    /// `FTW_DNR`: a directory that cannot be read.
    DirUnreadable,
    /// `FTW_NS`: `stat` failed (the entry cannot be examined).
    NoStat,
    /// `FTW_SL`: a symbolic link (physical walks).
    Symlink,
    /// `FTW_DP`: a directory, after its contents (post-order walks).
    DirPost,
    /// `FTW_SLN`: a symbolic link naming nothing (walks that follow links).
    Dangling,
}

/// One entry reported.
pub struct Visit<'a> {
    /// The path: the root as given (trailing slashes removed), then the names below it,
    /// with a terminator.
    pub path: &'a [u8],
    /// The offset of the last component in `path`.
    pub base: usize,
    /// The depth below the root (the root is 0).
    pub level: usize,
    /// What the entry is.
    pub kind: Kind,
    /// Its status (`None` for [`Kind::NoStat`]).
    pub stat: Option<&'a Stat>,
}

/// What a walk tells its caller.
pub enum Event<'a> {
    /// An entry.
    Visit(Visit<'a>),
    /// The walk enters a directory (after its pre-order visit): its descriptor.
    Enter(Fd),
    /// The walk leaves a directory (after its post-order visit) for its parent: the parent's
    /// descriptor, `None` when leaving the root.
    Leave(Option<Fd>),
}

/// What the caller wants after a visit.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Go on.
    Continue,
    /// Do not enter this directory (no effect on anything else).
    SkipSubtree,
    /// Skip the rest of the entry's directory (its post-order visit still happens).
    SkipSiblings,
    /// End the walk; it returns this value.
    Stop(i32),
}

/// How to walk.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Options {
    /// Do not follow symbolic links (`FTW_PHYS`).
    pub physical: bool,
    /// Stay on the root's file system (`FTW_MOUNT`): entries on another are not reported.
    pub mount: bool,
    /// Report directories after their contents (`FTW_DEPTH`).
    pub depth: bool,
}

struct Frame {
    listing: Listing,
    next: usize,
    /// The directory's path length (without terminator).
    path_len: usize,
    base: usize,
    level: usize,
    stat: Stat,
    skip: bool,
}

/// Status of `name` in `dir`: the entry's kind and status as the walk reports it, or `None`
/// (skip silently) or an error that ends the walk.
fn examine<S: Sys>(sys: &S, dir: Fd, name: KStr<'_>, physical: bool) -> Result<(Kind, Option<Stat>)> {
    let flags = if physical { AT_SYMLINK_NOFOLLOW } else { 0 };
    match sys.fstatat(dir, name, flags) {
        Ok(st) => {
            let kind = match st.mode & S_IFMT {
                S_IFDIR => Kind::Dir,
                S_IFLNK => Kind::Symlink,
                _ => Kind::File,
            };
            Ok((kind, Some(st)))
        }
        Err(Errno::EACCES) => Ok((Kind::NoStat, None)),
        Err(Errno::ENOENT) => match sys.fstatat(dir, name, AT_SYMLINK_NOFOLLOW) {
            Ok(l) if !physical && l.mode & S_IFMT == S_IFLNK => Ok((Kind::Dangling, Some(l))),
            _ => Ok((Kind::NoStat, None)),
        },
        Err(e) => Err(e),
    }
}

fn set_path(path: &mut TryVec<u8>, len: usize, name: &[u8]) -> Result<usize> {
    path.truncate(len);
    if path.last() != Some(&b'/') {
        path.push(b'/')?;
    }
    let base = path.len();
    path.extend_from_slice(name)?;
    path.push(0)?;
    // D-13.13: every path must be representable in the C callback's `int` offsets.
    if i32::try_from(path.len()).is_err() {
        return Err(Errno::ENAMETOOLONG);
    }
    Ok(base)
}

/// `nftw`'s walk of `root` (see the module documentation): `f` sees every entry and the
/// directory changes; the result is 0 at the end, the value of [`Action::Stop`], or the error
/// that ended the walk. Paths are relative to the working directory at the call.
pub fn walk<S: Sys>(sys: &S, root: &[u8], opt: Options, f: &mut dyn FnMut(Event<'_>) -> Result<Action>) -> Result<i32> {
    walk_levels(sys, &mut Levels::new(Fd::CWD), root, opt, f)
}

/// [`walk`] with its open directories in `levels` (closed at the end), for a caller that must
/// close them itself if `f` never returns (a C++ exception, OD-6 revised).
pub fn walk_levels<S: Sys>(sys: &S, levels: &mut Levels, root: &[u8], opt: Options, f: &mut dyn FnMut(Event<'_>) -> Result<Action>) -> Result<i32> {
    let r = walk_in(sys, levels, root, opt, f);
    levels.close_all(sys);
    r
}

fn walk_in<S: Sys>(sys: &S, levels: &mut Levels, root: &[u8], opt: Options, f: &mut dyn FnMut(Event<'_>) -> Result<Action>) -> Result<i32> {
    // The root as given, without trailing slashes (one is kept for "/").
    let keep = root.iter().rposition(|&b| b != b'/').map_or(root.len().min(1), |i| i.saturating_add(1));
    let root = root.get(..keep).unwrap_or(&[]);
    let mut path = TryVec::with_capacity(root.len().saturating_add(256))?;
    path.extend_from_slice(root)?;
    path.push(0)?;
    let root_base = match root.iter().rposition(|&b| b == b'/') {
        Some(i) if i.saturating_add(1) < root.len() => i.saturating_add(1),
        _ => 0,
    };
    let rootz = TryVec::from_slice(&path)?;
    let (kind, st) = examine_root(sys, KStr::from_nul_terminated(&rootz)?, opt.physical)?;
    if kind != Kind::Dir {
        return match visit(&path, root_base, 0, kind, st.as_ref(), f)? {
            Action::Stop(v) => Ok(v),
            _ => Ok(0),
        };
    }
    let st = st.ok_or(Errno::EIO)?;
    let root_dev = st.dev;
    let mut seen = Seen::new();
    seen.insert((st.dev, st.ino))?;
    let fd = match levels.push(sys, &rootz, !opt.physical) {
        Ok((fd, _)) => fd,
        Err(Errno::EACCES) => {
            return match visit(&path, root_base, 0, Kind::DirUnreadable, Some(&st), f)? {
                Action::Stop(v) => Ok(v),
                _ => Ok(0),
            };
        }
        Err(e) => return Err(e),
    };
    if !opt.depth {
        match visit(&path, root_base, 0, Kind::Dir, Some(&st), f)? {
            Action::Stop(v) => return Ok(v),
            Action::SkipSubtree | Action::SkipSiblings => return Ok(0),
            Action::Continue => {}
        }
    }
    if let Action::Stop(v) = f(Event::Enter(fd))? {
        return Ok(v);
    }
    let listing = list(sys, fd, false)?;
    let mut frames: TryVec<Frame> = TryVec::with_capacity(16)?;
    frames.push(Frame { listing, next: 0, path_len: root.len(), base: root_base, level: 0, stat: st, skip: false })?;
    while let Some(top) = frames.len().checked_sub(1) {
        let Some(frame) = frames.get_mut(top) else { break };
        if !frame.skip && frame.next < frame.listing.len() {
            let Some((namez, _)) = frame.listing.get(frame.next) else { break };
            frame.next = frame.next.saturating_add(1);
            let namez = TryVec::from_slice(namez)?;
            let name = namez.get(..namez.len().saturating_sub(1)).unwrap_or(&[]);
            let level = frame.level.saturating_add(1);
            let (dir_level, path_len) = (frame.level, frame.path_len);
            let base = set_path(&mut path, path_len, name)?;
            let dir = levels.fd(sys, dir_level)?;
            let (kind, cst) = examine(sys, dir, KStr::from_nul_terminated(&namez)?, opt.physical)?;
            if opt.mount && cst.as_ref().is_some_and(|s| s.dev != root_dev) {
                continue;
            }
            let action = if kind == Kind::Dir {
                let cst = cst.ok_or(Errno::EIO)?;
                let fresh = if opt.physical { !levels.contains(cst.dev, cst.ino) } else { seen.insert((cst.dev, cst.ino))? };
                if !fresh {
                    continue;
                }
                let cfd = match levels.push(sys, &namez, !opt.physical) {
                    Ok((fd, _)) => fd,
                    Err(Errno::EACCES) => {
                        let a = visit(&path, base, level, Kind::DirUnreadable, Some(&cst), f)?;
                        apply(&mut frames, a)?;
                        if let Action::Stop(v) = a {
                            return Ok(v);
                        }
                        continue;
                    }
                    Err(e) => return Err(e),
                };
                let mut entered = Action::Continue;
                if !opt.depth {
                    entered = visit(&path, base, level, Kind::Dir, Some(&cst), f)?;
                }
                match entered {
                    Action::Stop(v) => return Ok(v),
                    Action::SkipSubtree | Action::SkipSiblings => {
                        levels.pop(sys);
                        entered
                    }
                    Action::Continue => {
                        if let Action::Stop(v) = f(Event::Enter(cfd))? {
                            return Ok(v);
                        }
                        let listing = list(sys, cfd, false)?;
                        let path_len = path.len().saturating_sub(1);
                        frames.push(Frame { listing, next: 0, path_len, base, level, stat: cst, skip: false })?;
                        Action::Continue
                    }
                }
            } else {
                visit(&path, base, level, kind, cst.as_ref(), f)?
            };
            if let Action::Stop(v) = action {
                return Ok(v);
            }
            if action == Action::SkipSiblings {
                apply(&mut frames, action)?;
            }
        } else {
            let Some(done) = frames.pop() else { break };
            path.truncate(done.path_len);
            path.push(0)?;
            let mut action = Action::Continue;
            if opt.depth {
                action = visit(&path, done.base, done.level, Kind::DirPost, Some(&done.stat), f)?;
            }
            if let Action::Stop(v) = action {
                return Ok(v);
            }
            let parent = match done.level.checked_sub(1) {
                Some(p) => Some(levels.fd(sys, p)?),
                None => None,
            };
            if let Action::Stop(v) = f(Event::Leave(parent))? {
                return Ok(v);
            }
            levels.pop(sys);
            if action == Action::SkipSiblings {
                apply(&mut frames, action)?;
            }
        }
    }
    Ok(0)
}

fn visit(path: &[u8], base: usize, level: usize, kind: Kind, stat: Option<&Stat>, f: &mut dyn FnMut(Event<'_>) -> Result<Action>) -> Result<Action> {
    f(Event::Visit(Visit { path, base, level, kind, stat }))
}

/// `SkipSiblings` ends the listing of the innermost directory being read.
fn apply(frames: &mut TryVec<Frame>, a: Action) -> Result<()> {
    if a == Action::SkipSiblings {
        if let Some(top) = frames.len().checked_sub(1).and_then(|t| frames.get_mut(t)) {
            top.skip = true;
        }
    }
    Ok(())
}

/// The root's kind and status: `Kind::Dangling` for a symbolic link naming nothing when links
/// are followed; an error (no visit) for a root that cannot be examined.
fn examine_root<S: Sys>(sys: &S, root: KStr<'_>, physical: bool) -> Result<(Kind, Option<Stat>)> {
    let flags = if physical { AT_SYMLINK_NOFOLLOW } else { 0 };
    match sys.fstatat(Fd::CWD, root, flags) {
        Ok(st) => {
            let kind = match st.mode & S_IFMT {
                S_IFDIR => Kind::Dir,
                S_IFLNK => Kind::Symlink,
                _ => Kind::File,
            };
            Ok((kind, Some(st)))
        }
        Err(Errno::ENOENT) if !physical => match sys.fstatat(Fd::CWD, root, AT_SYMLINK_NOFOLLOW) {
            Ok(l) if l.mode & S_IFMT == S_IFLNK => Ok((Kind::Dangling, Some(l))),
            _ => Err(Errno::ENOENT),
        },
        Err(e) => Err(e),
    }
}
