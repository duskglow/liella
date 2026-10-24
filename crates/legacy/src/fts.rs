//! The `fts` family (spec §13.3; D-13.4, D-13.13): an iterator-style walk whose entries are
//! `FTSENT` records in memory the C caller reads (and partly writes: `fts_number`,
//! `fts_pointer`), reached through [`FtsMem`], which the boundary implements over the caller's
//! allocator.
//!
//! The walk is descriptor-relative ([`Levels`]: one descriptor per directory on the current
//! path), so it reads trees deeper than `PATH_MAX` in every mode. Without `FTS_NOCHDIR` (and
//! without `FTS_LOGICAL`, which implies it) the working directory follows the walk, as the
//! contract makes it observable (D-13.4): each directory is entered with `fchdir` before its
//! entries are returned and left for its parent before its post-order return, and `fts_close`
//! returns to the caller's working directory.
//!
//! Each `FTSENT` is one block: the 120-byte structure with `fts_name` at its end, then the
//! `struct stat` `fts_statp` points to. Paths live in one buffer per stream (FTS-SHARED-PATH,
//! ruling #615): every entry's `fts_path` points at its start and `fts_pathlen` gives the
//! entry's length; only the entry `fts_read` returned last is NUL-terminated there, and its
//! ancestors' paths are its prefixes (fts(3): directories stay valid while inside them;
//! entries from `fts_children`, kept past their validity, or seen by the comparison show
//! whatever the buffer holds now). `fts_accpath` equals `fts_path`, or points to the name in
//! chdir mode. When the buffer grows, every live entry is re-pointed. A directory's entries
//! are freed when its post-order entry is returned; everything is freed by `fts_close`.
//!
//! Decisions (each in the phase report):
//! * `fts_open` with no paths is `EINVAL` (D-13.13); a path that is empty is `ENOENT`
//!   (fts(3)); options outside `FTS_OPTIONMASK` are `EINVAL`; options with neither or both of
//!   `FTS_LOGICAL` and `FTS_PHYSICAL` are accepted (neither: physical; both: logical), as the
//!   reference does.
//! * Roots are compared under their whole argument (their `fts_name` until `fts_read` returns
//!   them; then the last component), as the record shows.
//! * `fts_set` with an instruction other than 0, `FTS_AGAIN`, `FTS_FOLLOW`, `FTS_NOINSTR`,
//!   `FTS_SKIP` returns -1 with `EINVAL` (fts(3); the reference returns 1).
//! * `fts_instr` is `FTS_NOINSTR` in every entry the library makes, and again once `fts_read`
//!   has acted on an instruction, as the reference (probe, FTS-FLAT-FIXES-2).
//! * The instruction lives only in the entry's `fts_instr`: one the caller writes there
//!   directly, without `fts_set`, is acted on as if `fts_set` had given it, as the reference
//!   (probe, FTS-INSTR-DIRECT; programs do this). A value that is no instruction is ignored
//!   and, like any other, reset to `FTS_NOINSTR` by the `fts_read` that sees it.
//! * `FTS_DC`'s `fts_cycle` is the ancestor the directory repeats (fts(3); the reference too:
//!   FTS-SHARED-PATH showed by name and level that the earlier record of the entry itself came
//!   from reading the ancestor's `fts_path`, which is the shared buffer there).
//! * `fts_children` after the walk has ended returns NULL, as at the end of a directory
//!   (D-13.13).
//! * A path longer than `fts_pathlen` (an `unsigned short`) can express, or a level deeper than
//!   `fts_level` (a `short`) can, is reported as `FTS_NS` with `ENAMETOOLONG` and not entered
//!   (D-13.13: only what cannot be represented fails).

use liella::error::{Errno, Result};
use liella::fs::dir::{DirStream, DT_DIR, DT_UNKNOWN};
use liella::fs::walk::Levels;
use liella::mem::TryVec;
use liella_abi::callback::SlotCompareFn;
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Stat, AT_SYMLINK_NOFOLLOW, O_CLOEXEC, O_DIRECTORY, O_PATH, SEEK_SET, S_IFDIR, S_IFLNK, S_IFMT, S_IFREG};

use crate::fs_tree::sort_slots;

/// `FTS_COMFOLLOW`.
pub const FTS_COMFOLLOW: i32 = 1;
/// `FTS_LOGICAL`.
pub const FTS_LOGICAL: i32 = 2;
/// `FTS_NOCHDIR`.
pub const FTS_NOCHDIR: i32 = 4;
/// `FTS_NOSTAT`.
pub const FTS_NOSTAT: i32 = 8;
/// `FTS_PHYSICAL`.
pub const FTS_PHYSICAL: i32 = 16;
/// `FTS_SEEDOT`.
pub const FTS_SEEDOT: i32 = 32;
/// `FTS_XDEV`.
pub const FTS_XDEV: i32 = 64;
/// `FTS_OPTIONMASK`.
pub const FTS_OPTIONMASK: i32 = 0xff;
/// `FTS_NAMEONLY` (`fts_children`).
pub const FTS_NAMEONLY: i32 = 0x100;

/// `fts_info` values.
pub mod info {
    /// A directory in pre-order.
    pub const D: u16 = 1;
    /// A directory that repeats an ancestor.
    pub const DC: u16 = 2;
    /// Anything else.
    pub const DEFAULT: u16 = 3;
    /// A directory that cannot be read.
    pub const DNR: u16 = 4;
    /// `.` or `..` (`FTS_SEEDOT`).
    pub const DOT: u16 = 5;
    /// A directory in post-order.
    pub const DP: u16 = 6;
    /// A regular file.
    pub const F: u16 = 8;
    /// No status: `stat` failed.
    pub const NS: u16 = 10;
    /// No status requested (`FTS_NOSTAT`, `FTS_NAMEONLY`).
    pub const NSOK: u16 = 11;
    /// A symbolic link.
    pub const SL: u16 = 12;
    /// A symbolic link naming nothing.
    pub const SLNONE: u16 = 13;
}

/// `fts_set` instructions.
pub const FTS_AGAIN: u16 = 1;
/// See [`FTS_AGAIN`].
pub const FTS_FOLLOW: u16 = 2;
/// See [`FTS_AGAIN`].
pub const FTS_SKIP: u16 = 4;
/// No instruction: an entry's `fts_instr` until `fts_set` gives one, and after `fts_read` has
/// acted on it.
pub const FTS_NOINSTR: u16 = 3;

/// `FTSENT` field offsets (spec Appendix C).
mod off {
    pub(super) const CYCLE: usize = 0;
    pub(super) const PARENT: usize = 8;
    pub(super) const LINK: usize = 16;
    /// `fts_number`, then `fts_pointer`: the caller's, never written.
    pub(super) const NUMBER: usize = 24;
    pub(super) const ACCPATH: usize = 40;
    pub(super) const PATH: usize = 48;
    pub(super) const ERRNO: usize = 56;
    pub(super) const SYMFD: usize = 60;
    pub(super) const PATHLEN: usize = 64;
    pub(super) const NAMELEN: usize = 66;
    pub(super) const INO: usize = 72;
    pub(super) const DEV: usize = 80;
    pub(super) const NLINK: usize = 88;
    pub(super) const LEVEL: usize = 96;
    pub(super) const INFO: usize = 98;
    pub(super) const FLAGS: usize = 100;
    pub(super) const INSTR: usize = 102;
    pub(super) const STATP: usize = 104;
    pub(super) const NAME: usize = 112;
}
const STAT_SIZE: usize = 144;

/// Caller-visible memory for the `FTSENT` blocks.
pub trait FtsMem {
    /// An 8-aligned block from the caller's allocator, every byte written once: `head` zero
    /// bytes, then `tail`'s parts one after another, then a NUL (`head` plus the parts' total
    /// plus 1 bytes): its address. The tail is copied in, never zeroed first (FTS-PATH-ZERO).
    fn alloc(&mut self, head: usize, tail: &[&[u8]]) -> Option<usize>;
    /// Return a block [`alloc`](Self::alloc) gave.
    fn free(&mut self, addr: usize);
    /// The first `len` bytes of a live block [`alloc`](Self::alloc) gave.
    fn bytes(&mut self, addr: usize, len: usize) -> &mut [u8];
    /// Copy `len` bytes from `from` to `to`, each a (live block, offset into it) pair, the two
    /// blocks different ones.
    fn copy(&mut self, from: (usize, usize), to: (usize, usize), len: usize);
}

#[derive(Default)]
enum Children {
    #[default]
    NotBuilt,
    Built(TryVec<usize>),
    NameOnly(TryVec<usize>),
    /// Reading failed (`FTS_DNR`).
    Unreadable(Errno),
    /// Entering failed (chdir mode): no entries, the post-order entry carries the error.
    NoSearch(Errno),
}

struct Node {
    block: usize,
    size: usize,
    name_cap: usize,
    stat_off: usize,
    /// `fts_namelen`. The name itself is only in the block (`fts_name`): a copy kept here cost
    /// ~199 instructions per entry (FTS-FLAT-FIXES).
    name_len: usize,
    /// A root's whole argument (empty for other entries): its path.
    arg: TryVec<u8>,
    /// The length of `fts_path`; the bytes are in the stream's buffer only while the entry or
    /// one below it is the current one ([`Fts::place`]).
    path_len: usize,
    parent: usize,
    level: i32,
    info: u16,
    errno: i32,
    stat: Option<Stat>,
    cycle: usize,
    link: usize,
    children: Children,
    next_child: usize,
    follow: bool,
    root_dev: u64,
    root: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Start,
    Walking,
    Done,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Build {
    Read,
    Children,
    NameOnly,
}

/// An open `fts` stream.
pub struct Fts {
    options: i32,
    compar: Option<SlotCompareFn>,
    nodes: TryVec<Option<Node>>,
    free_slots: TryVec<usize>,
    roots: TryVec<usize>,
    cur: Option<usize>,
    state: State,
    /// `fts_children` has listed entries (which the caller may mark `FTS_SKIP` directly), or
    /// `fts_set` has given `FTS_SKIP` to an entry other than the current one: only then does
    /// the walk look for entries to pass over
    /// ([`Fts::passed_over`]; looking on every entry cost ~48 instructions each, FTS-FLAT-FIXES-2).
    skips_listed: bool,
    levels: Levels,
    orig: Option<Fd>,
    /// The shared path buffer (a block from [`FtsMem`]) and its size; 0 before `fts_open` made it.
    buf: usize,
    buf_cap: usize,
}

/// The path buffer's first size; it doubles as deeper paths need it.
const PATH_START: usize = 256;

const ROOT_PARENT: usize = 0;

/// Zero bytes: a name's terminator and the padding to its 8-aligned status.
const PAD: [u8; 8] = [0; 8];

/// The buffer a directory is read with.
const LIST_BUFFER: usize = 16 * 1024;

/// Entry `x`'s name, read from its block (`fts_name`).
fn name_of<'m, M: FtsMem>(mem: &'m mut M, x: &Node) -> &'m [u8] {
    let b: &'m [u8] = mem.bytes(x.block, off::NAME.saturating_add(x.name_len));
    b.get(off::NAME..).unwrap_or(&[])
}

fn put(b: &mut [u8], at: usize, v: &[u8]) {
    if let Some(d) = b.get_mut(at..at.saturating_add(v.len())) {
        d.copy_from_slice(v);
    }
}

fn stat_bytes(st: &Stat) -> [u8; STAT_SIZE] {
    let mut b = [0u8; STAT_SIZE];
    put(&mut b, 0, &st.dev.to_ne_bytes());
    put(&mut b, 8, &st.ino.to_ne_bytes());
    put(&mut b, 16, &st.nlink.to_ne_bytes());
    put(&mut b, 24, &st.mode.to_ne_bytes());
    put(&mut b, 28, &st.uid.to_ne_bytes());
    put(&mut b, 32, &st.gid.to_ne_bytes());
    put(&mut b, 40, &st.rdev.to_ne_bytes());
    put(&mut b, 48, &st.size.to_ne_bytes());
    put(&mut b, 56, &st.blksize.to_ne_bytes());
    put(&mut b, 64, &st.blocks.to_ne_bytes());
    for (i, t) in [st.atime, st.mtime, st.ctime].iter().enumerate() {
        let at = 72usize.saturating_add(i.saturating_mul(16));
        put(&mut b, at, &t.sec.to_ne_bytes());
        put(&mut b, at.saturating_add(8), &t.nsec.to_ne_bytes());
    }
    b
}

fn z(text: &[u8]) -> Result<TryVec<u8>> {
    let mut v = TryVec::with_capacity(text.len().saturating_add(1))?;
    v.extend_from_slice(text)?;
    v.push(0)?;
    Ok(v)
}

fn align8(n: usize) -> usize {
    n.saturating_add(7) & !7
}

/// `fts_info` of a status.
fn classify(st: &Stat, dot: bool) -> u16 {
    if dot {
        return info::DOT;
    }
    match st.mode & S_IFMT {
        S_IFDIR => info::D,
        S_IFLNK => info::SL,
        S_IFREG => info::F,
        _ => info::DEFAULT,
    }
}

/// The status of `namez` (a name with its terminator) in `dir`: following links (then a link
/// naming nothing is `FTS_SLNONE`, whatever made `stat` fail) or not. The listing's names
/// already end in NUL, so a child's status needs no copy of its name (FTS-FLAT-PROFILE).
fn stat_entry<S: Sys>(sys: &S, dir: Fd, namez: &[u8], follow: bool, dot: bool) -> Result<(u16, Option<Stat>, i32)> {
    let k = KStr::from_nul_terminated(namez)?;
    if follow {
        match sys.fstatat(dir, k, 0) {
            Ok(st) => Ok((classify(&st, dot), Some(st), 0)),
            Err(e) => match sys.fstatat(dir, k, AT_SYMLINK_NOFOLLOW) {
                Ok(l) if l.mode & S_IFMT == S_IFLNK => Ok((info::SLNONE, Some(l), 0)),
                _ => Ok((info::NS, None, e.get())),
            },
        }
    } else {
        match sys.fstatat(dir, k, AT_SYMLINK_NOFOLLOW) {
            Ok(st) => Ok((classify(&st, dot), Some(st), 0)),
            Err(e) => Ok((info::NS, None, e.get())),
        }
    }
}

impl Fts {
    fn chdir_mode(&self) -> bool {
        self.options & (FTS_NOCHDIR | FTS_LOGICAL) == 0
    }

    fn logical(&self) -> bool {
        self.options & FTS_LOGICAL != 0
    }

    fn node(&self, n: usize) -> Result<&Node> {
        self.nodes.get(n).and_then(Option::as_ref).ok_or(Errno::EINVAL)
    }

    fn node_mut(&mut self, n: usize) -> Result<&mut Node> {
        self.nodes.get_mut(n).and_then(Option::as_mut).ok_or(Errno::EINVAL)
    }

    /// The block of entry `n` (0 if there is none).
    fn block(&self, n: usize) -> usize {
        self.node(n).map_or(0, |x| x.block)
    }

    /// The current entry's block (`fts_cur`).
    pub fn current(&self) -> usize {
        self.cur.map_or(0, |n| self.block(n))
    }

    /// `fts_options`.
    pub fn options(&self) -> i32 {
        self.options
    }

    /// The descriptor of the caller's working directory (chdir mode), for `fts_rfd`.
    pub fn saved_cwd(&self) -> Option<Fd> {
        self.orig
    }

    #[allow(clippy::too_many_arguments)]
    /// A new entry whose path is `path_len` bytes long (its bytes are put in the shared buffer
    /// only when the entry is returned: [`Fts::place`]). A root's path is its name.
    fn new_node<M: FtsMem>(&mut self, mem: &mut M, name: &[u8], path_len: usize, parent: usize, level: i32, st: (u16, Option<Stat>, i32), cycle: usize, follow: bool, root_dev: u64, root: bool) -> Result<usize> {
        let name_cap = name.len().saturating_add(1);
        let stat_off = align8(off::NAME.saturating_add(name_cap));
        // The stat ends past the 120-byte structure, so the block is never shorter than an
        // `FTSENT`; no path is kept in it.
        let size = stat_off.saturating_add(STAT_SIZE).saturating_add(1);
        // Each byte written once (FTS-FLAT-FIXES): the structure zeroed (filled in below), the
        // name, its terminator and padding, the status.
        let pad = PAD.get(..stat_off.saturating_sub(off::NAME).saturating_sub(name.len())).unwrap_or(&[]);
        let block = mem.alloc(off::NAME, &[name, pad, &stat_bytes(&st.1.unwrap_or_default())]).ok_or(Errno::ENOMEM)?;
        let node = Node {
            block,
            size,
            name_cap,
            stat_off,
            name_len: name.len(),
            arg: TryVec::from_slice(if root { name } else { &[] }).inspect_err(|_| mem.free(block))?,
            path_len,
            parent,
            level,
            info: st.0,
            errno: st.2,
            stat: st.1,
            cycle,
            link: 0,
            children: Children::NotBuilt,
            next_child: 0,
            follow,
            root_dev,
            root,
        };
        let idx = match self.free_slots.pop() {
            Some(i) => {
                if let Some(s) = self.nodes.get_mut(i) {
                    *s = Some(node);
                }
                i
            }
            None => {
                let i = self.nodes.len();
                if let Err(e) = self.nodes.push(Some(node)) {
                    mem.free(block);
                    return Err(e);
                }
                i
            }
        };
        self.write_header(mem, idx);
        put(mem.bytes(block, off::NAME), off::INSTR, &FTS_NOINSTR.to_ne_bytes());
        Ok(idx)
    }

    /// Entry `n`'s instruction: its `fts_instr`, as `fts_set` or the caller wrote it (the block
    /// is its only copy, FTS-INSTR-DIRECT).
    fn instr_of<M: FtsMem>(&self, mem: &mut M, n: usize) -> u16 {
        let Ok(x) = self.node(n) else { return FTS_NOINSTR };
        let b = mem.bytes(x.block, off::NAME);
        b.get(off::INSTR..off::STATP).map_or(FTS_NOINSTR, |v| u16::from_ne_bytes([v.first().copied().unwrap_or(0), v.get(1).copied().unwrap_or(0)]))
    }

    /// Write `instr` into entry `n`'s `fts_instr`.
    fn set_instr<M: FtsMem>(&self, mem: &mut M, n: usize, instr: u16) {
        let Ok(x) = self.node(n) else { return };
        put(mem.bytes(x.block, off::NAME), off::INSTR, &instr.to_ne_bytes());
    }

    /// Whether `n` is a directory below a root that the caller marked `FTS_SKIP` after
    /// `fts_children` listed it (through `fts_set` or by writing `fts_instr`): the walk never
    /// returns it, as the reference (probe, FTS-FLAT-FIXES-2, FTS-INSTR-DIRECT; a file or root
    /// so marked is returned).
    fn passed_over<M: FtsMem>(&self, mem: &mut M, n: usize) -> bool {
        self.skips_listed && self.node(n).is_ok_and(|x| x.info == info::D && !x.root) && self.instr_of(mem, n) == FTS_SKIP
    }

    /// Whether entry `n`'s `fts_accpath` is its own name (chdir mode) rather than `fts_path`.
    fn acc_is_name(&self, n: usize, x: &Node) -> bool {
        self.chdir_mode() && !x.root && n != ROOT_PARENT
    }

    /// Entry `n`'s structure: every field before `fts_name`.
    fn header(&self, n: usize, x: &Node) -> [u8; off::NAME] {
        let parent = if n == ROOT_PARENT { 0 } else { self.block(x.parent) };
        let acc = if self.acc_is_name(n, x) { x.block.saturating_add(off::NAME) } else { self.buf };
        let st = x.stat.unwrap_or_default();
        let mut b = [0u8; off::NAME];
        put(&mut b, off::CYCLE, &x.cycle.to_ne_bytes());
        put(&mut b, off::PARENT, &parent.to_ne_bytes());
        put(&mut b, off::LINK, &x.link.to_ne_bytes());
        put(&mut b, off::ACCPATH, &acc.to_ne_bytes());
        put(&mut b, off::PATH, &self.buf.to_ne_bytes());
        put(&mut b, off::ERRNO, &x.errno.to_ne_bytes());
        put(&mut b, off::SYMFD, &0i32.to_ne_bytes());
        put(&mut b, off::PATHLEN, &u16::try_from(x.path_len).unwrap_or(u16::MAX).to_ne_bytes());
        put(&mut b, off::NAMELEN, &u16::try_from(x.name_len).unwrap_or(u16::MAX).to_ne_bytes());
        put(&mut b, off::INO, &st.ino.to_ne_bytes());
        put(&mut b, off::DEV, &st.dev.to_ne_bytes());
        put(&mut b, off::NLINK, &st.nlink.to_ne_bytes());
        put(&mut b, off::LEVEL, &i16::try_from(x.level).unwrap_or(i16::MAX).to_ne_bytes());
        put(&mut b, off::INFO, &x.info.to_ne_bytes());
        put(&mut b, off::FLAGS, &0u16.to_ne_bytes());
        // `fts_instr` is not part of it: the block holds the instruction ([`Fts::instr_of`]).
        put(&mut b, off::STATP, &x.block.saturating_add(x.stat_off).to_ne_bytes());
        b
    }

    /// Write entry `n`'s structure into its block (`fts_number`, `fts_pointer` and `fts_instr`
    /// stay as the caller or `fts_set` left them). The name is written when the block is made (a root's again in
    /// [`Fts::visit`]), the status then and by [`Fts::write`].
    fn write_header<M: FtsMem>(&self, mem: &mut M, n: usize) {
        let Ok(x) = self.node(n) else { return };
        let h = self.header(n, x);
        let b = mem.bytes(x.block, off::NAME);
        put(b, 0, h.get(..off::NUMBER).unwrap_or(&[]));
        put(b, off::ACCPATH, h.get(off::ACCPATH..off::INSTR).unwrap_or(&[]));
        put(b, off::STATP, h.get(off::STATP..).unwrap_or(&[]));
    }

    /// Write entry `n`'s structure and status into its block (its status changed).
    fn write<M: FtsMem>(&self, mem: &mut M, n: usize) {
        self.write_header(mem, n);
        let Ok(x) = self.node(n) else { return };
        put(mem.bytes(x.block, x.size), x.stat_off, &stat_bytes(&x.stat.unwrap_or_default()));
    }

    /// Make the path buffer at least `need` bytes long, keeping its first `keep` bytes. A new
    /// buffer re-points `fts_path` (and `fts_accpath` where it is `fts_path`) in every live
    /// entry: the current directory's list, the ancestors and their lists.
    fn reserve<M: FtsMem>(&mut self, mem: &mut M, keep: usize, need: usize) -> Result<()> {
        if need <= self.buf_cap {
            return Ok(());
        }
        let mut cap = self.buf_cap.max(PATH_START);
        while cap < need {
            cap = cap.checked_mul(2).ok_or(Errno::ENOMEM)?;
        }
        let kept = if self.buf == 0 { TryVec::with_capacity(0)? } else { TryVec::from_slice(mem.bytes(self.buf, keep.min(self.buf_cap)))? };
        let nb = mem.alloc(cap.saturating_sub(1), &[]).ok_or(Errno::ENOMEM)?;
        put(mem.bytes(nb, cap), 0, &kept);
        if self.buf != 0 {
            mem.free(self.buf);
        }
        self.buf = nb;
        self.buf_cap = cap;
        let to = nb.to_ne_bytes();
        for n in 0..self.nodes.len() {
            let Some(Some(x)) = self.nodes.get(n) else { continue };
            let b = mem.bytes(x.block, x.size);
            put(b, off::PATH, &to);
            if !self.acc_is_name(n, x) {
                put(b, off::ACCPATH, &to);
            }
        }
        Ok(())
    }

    /// Put entry `n`'s path in the buffer, NUL-terminated: a root's whole argument; any other
    /// entry's separator and name after its parent's path, which the buffer already holds (the
    /// walk only returns a child of the entry it returned last or of one of that entry's
    /// ancestors, and everything written since then went past the parent's path).
    fn place<M: FtsMem>(&mut self, mem: &mut M, n: usize) -> Result<()> {
        let (start, len) = {
            let x = self.node(n)?;
            (if x.root { 0 } else { self.node(x.parent)?.path_len }, x.path_len)
        };
        let end = len.checked_add(1).ok_or(Errno::ENOMEM)?;
        self.reserve(mem, start, end)?;
        let x = self.node(n)?;
        if x.root {
            put(mem.bytes(self.buf, end), 0, &x.arg);
        } else if x.name_len <= len {
            let name_at = len.saturating_sub(x.name_len);
            if name_at > start {
                put(mem.bytes(self.buf, end), start, b"/");
            }
            mem.copy((x.block, off::NAME), (self.buf, name_at), x.name_len);
        }
        put(mem.bytes(self.buf, end), len, &[0]);
        Ok(())
    }

    /// End the buffer's path after entry `n`'s (a directory returned again after entries below
    /// it: its path is the buffer's prefix).
    fn terminate<M: FtsMem>(&self, mem: &mut M, n: usize) {
        let Ok(x) = self.node(n) else { return };
        if x.path_len < self.buf_cap {
            put(mem.bytes(self.buf, x.path_len.saturating_add(1)), x.path_len, &[0]);
        }
    }

    /// `fts_open(argv, options, compar)`.
    pub fn open<S: Sys, M: FtsMem>(sys: &S, mem: &mut M, argv: &[&[u8]], options: i32, compar: Option<SlotCompareFn>) -> Result<Fts> {
        if options & !FTS_OPTIONMASK != 0 || argv.is_empty() {
            return Err(Errno::EINVAL);
        }
        if argv.iter().any(|a| a.is_empty()) {
            return Err(Errno::ENOENT);
        }
        let chdir = options & (FTS_NOCHDIR | FTS_LOGICAL) == 0;
        let orig = if chdir { Some(sys.openat(Fd::CWD, KStr::from_nul_terminated(b".\0")?, O_PATH | O_DIRECTORY | O_CLOEXEC, 0)?) } else { None };
        let mut f = Fts {
            options,
            compar,
            nodes: TryVec::with_capacity(argv.len().saturating_add(16))?,
            free_slots: TryVec::with_capacity(0)?,
            roots: TryVec::with_capacity(argv.len())?,
            cur: None,
            state: State::Start,
            skips_listed: false,
            levels: Levels::new(orig.unwrap_or(Fd::CWD)),
            orig,
            buf: 0,
            buf_cap: 0,
        };
        let r = f.open_roots(sys, mem, argv);
        match r {
            Ok(()) => Ok(f),
            Err(e) => {
                let _ = f.close(sys, mem);
                Err(e)
            }
        }
    }

    fn open_roots<S: Sys, M: FtsMem>(&mut self, sys: &S, mem: &mut M, argv: &[&[u8]]) -> Result<()> {
        self.reserve(mem, 0, PATH_START)?;
        let rp = self.new_node(mem, b"", 0, ROOT_PARENT, -1, (0, None, 0), 0, false, 0, false)?;
        debug_assert_eq!(rp, ROOT_PARENT);
        let follow = self.options & (FTS_LOGICAL | FTS_COMFOLLOW) != 0;
        let base = self.orig.unwrap_or(Fd::CWD);
        let mut roots = TryVec::with_capacity(argv.len())?;
        for a in argv {
            let st = if u16::try_from(a.len()).is_err() { (info::NS, None, Errno::ENAMETOOLONG.get()) } else { stat_entry(sys, base, &z(a)?, follow, false)? };
            let dev = st.1.map_or(0, |s| s.dev);
            let n = self.new_node(mem, a, a.len(), ROOT_PARENT, 0, st, 0, follow, dev, true)?;
            roots.push(n)?;
        }
        if let Some(c) = self.compar {
            let blocks: TryVec<(usize, usize)> = {
                let mut v = TryVec::with_capacity(roots.len())?;
                for &n in roots.iter() {
                    v.push((self.block(n), n))?;
                }
                v
            };
            let mut v = blocks;
            sort_slots(&mut v, |p| p.0, c)?;
            roots.clear();
            for (_, n) in v.iter() {
                roots.push(*n)?;
            }
        }
        self.link(mem, &roots);
        self.roots = roots;
        Ok(())
    }

    /// Chain `list` through `fts_link`.
    fn link<M: FtsMem>(&mut self, mem: &mut M, list: &[usize]) {
        for (i, &n) in list.iter().enumerate() {
            let next = list.get(i.saturating_add(1)).map_or(0, |&m| self.block(m));
            if let Ok(x) = self.node_mut(n) {
                x.link = next;
                // Only `fts_link` changed: the rest of the block is current.
                put(mem.bytes(x.block, x.size), off::LINK, &next.to_ne_bytes());
            }
        }
    }

    /// The directory a node's name is relative to: its parent's level, or the base for roots.
    fn dir_of<S: Sys>(&mut self, sys: &S, n: usize) -> Result<Fd> {
        let x = self.node(n)?;
        if x.root {
            return Ok(self.orig.unwrap_or(Fd::CWD));
        }
        let lvl = usize::try_from(x.level.saturating_sub(1)).map_err(|_| Errno::EINVAL)?;
        self.levels.fd(sys, lvl)
    }

    /// Restat entry `n` (`FTS_AGAIN`, `FTS_FOLLOW`); its entries are dropped.
    fn restat<S: Sys, M: FtsMem>(&mut self, sys: &S, mem: &mut M, n: usize, follow: bool) -> Result<()> {
        self.unpush(sys, n);
        self.drop_children(mem, n);
        let dir = self.dir_of(sys, n)?;
        let (namez, dot) = {
            let x = self.node(n)?;
            if x.root {
                (z(&x.arg)?, false)
            } else {
                let name = name_of(mem, x);
                (z(name)?, name == b"." || name == b"..")
            }
        };
        let (inf, st, e) = stat_entry(sys, dir, &namez, follow, dot)?;
        let x = self.node_mut(n)?;
        x.info = inf;
        x.stat = st;
        x.errno = e;
        x.follow = follow || x.follow;
        x.next_child = 0;
        if x.info == info::D {
            self.check_cycle(n)?;
        }
        self.write(mem, n);
        Ok(())
    }

    /// Mark a directory that repeats an ancestor as `FTS_DC`.
    fn check_cycle(&mut self, n: usize) -> Result<()> {
        let (st, parent) = {
            let x = self.node(n)?;
            let Some(st) = x.stat else { return Ok(()) };
            (st, x.parent)
        };
        let blk = self.cycle_of(parent, &st)?;
        if blk != 0 {
            let x = self.node_mut(n)?;
            x.info = info::DC;
            x.cycle = blk;
        }
        Ok(())
    }

    /// The block of the directory among `p` and its ancestors that `st` repeats, 0 if none.
    fn cycle_of(&self, mut p: usize, st: &Stat) -> Result<usize> {
        while p != ROOT_PARENT {
            let a = self.node(p)?;
            if a.stat.is_some_and(|s| s.dev == st.dev && s.ino == st.ino) {
                return Ok(a.block);
            }
            p = a.parent;
        }
        Ok(0)
    }

    /// Close entry `n`'s directory if it is the deepest level.
    fn unpush<S: Sys>(&mut self, sys: &S, n: usize) {
        let Ok(x) = self.node(n) else { return };
        if usize::try_from(x.level).is_ok_and(|l| self.levels.depth() == l.saturating_add(1)) {
            self.levels.pop(sys);
        }
    }

    fn free_node<M: FtsMem>(&mut self, mem: &mut M, n: usize) {
        self.drop_children(mem, n);
        if let Some(slot) = self.nodes.get_mut(n) {
            if let Some(x) = slot.take() {
                mem.free(x.block);
                let _ = self.free_slots.push(n);
            }
        }
    }

    fn drop_children<M: FtsMem>(&mut self, mem: &mut M, n: usize) {
        let list = match self.node_mut(n) {
            Ok(x) => match core::mem::take(&mut x.children) {
                Children::Built(l) | Children::NameOnly(l) => l,
                _ => return,
            },
            Err(_) => return,
        };
        for &c in list.iter() {
            self.free_node(mem, c);
        }
    }

    /// Read directory `n`'s entries (see [`Children`]).
    fn build<S: Sys, M: FtsMem>(&mut self, sys: &S, mem: &mut M, n: usize, how: Build) -> Result<()> {
        let (level, follow, path_len, root_dev, slash) = {
            let x = self.node(n)?;
            if how == Build::Read && matches!(x.children, Children::Built(_) | Children::Unreadable(_) | Children::NoSearch(_)) {
                return Ok(());
            }
            (x.level, x.follow, x.path_len, x.root_dev, x.root && x.arg.last() == Some(&b'/'))
        };
        self.drop_children(mem, n);
        let ulevel = usize::try_from(level).map_err(|_| Errno::EINVAL)?;
        if self.levels.depth() != ulevel.saturating_add(1) {
            // A root is opened by its whole argument, anything else by its name.
            let opened = {
                let x = self.node(n)?;
                if x.root {
                    z(&x.arg)?
                } else {
                    z(name_of(mem, x))?
                }
            };
            if let Err(e) = self.levels.push(sys, &opened, follow) {
                self.node_mut(n)?.children = Children::Unreadable(e);
                return Ok(());
            }
        }
        let fd = self.levels.fd(sys, ulevel)?;
        if self.chdir_mode() && how != Build::NameOnly {
            if let Err(e) = sys.fchdir(fd) {
                self.levels.pop(sys);
                self.node_mut(n)?.children = Children::NoSearch(e);
                return Ok(());
            }
        }
        let mut kids = TryVec::with_capacity(16)?;
        // A directory read before (`fts_children` called again) is read from its start. Each
        // entry is made from the kernel's record as it is read, with no listing of the names
        // copied first (it cost ~196 instructions per entry, FTS-FLAT-FIXES).
        let mut d = match sys.lseek(fd, 0, SEEK_SET).and_then(|_| DirStream::with_fd(fd, LIST_BUFFER, 0)) {
            Ok(d) => d,
            Err(e) => {
                self.leave_level(sys, n)?;
                self.node_mut(n)?.children = Children::Unreadable(e);
                return Ok(());
            }
        };
        let dots = self.options & FTS_SEEDOT != 0;
        let logical = self.logical();
        let nostat = self.options & FTS_NOSTAT != 0 && !logical;
        let child_level = level.saturating_add(1);
        // Only a root's path can end in a slash (names have none).
        let sep_len = usize::from(!slash);
        // Ok(Err(e)): reading the directory failed; Err(e): making an entry failed.
        let r = (|| -> Result<core::result::Result<(), Errno>> {
            loop {
                let e = match d.next(sys) {
                    Ok(Some(e)) => e,
                    Ok(None) => return Ok(Ok(())),
                    Err(e) => return Ok(Err(e)),
                };
                let (namez, cname, dtype) = (d.name_z(&e), d.name(&e), e.kind);
                let dot = cname == b"." || cname == b"..";
                if dot && !dots {
                    continue;
                }
                let cpath_len = path_len.saturating_add(sep_len).saturating_add(cname.len());
                let st = if u16::try_from(cpath_len).is_err() || i16::try_from(child_level).is_err() {
                    (info::NS, None, Errno::ENAMETOOLONG.get())
                } else if how == Build::NameOnly || (nostat && !dot && dtype != DT_DIR && dtype != DT_UNKNOWN) {
                    (info::NSOK, None, 0)
                } else {
                    stat_entry(sys, fd, namez, logical, dot)?
                };
                // A directory repeating an ancestor is `FTS_DC`, known before its block is made.
                let cycle = match st.1 {
                    Some(s) if st.0 == info::D => self.cycle_of(n, &s)?,
                    _ => 0,
                };
                let st = if cycle != 0 { (info::DC, st.1, st.2) } else { st };
                let c = self.new_node(mem, cname, cpath_len, n, child_level, st, cycle, logical, root_dev, false)?;
                if let Err(e) = kids.push(c) {
                    self.free_node(mem, c);
                    return Err(e);
                }
            }
        })();
        let _ = d.into_fd();
        let failed = match r {
            Ok(Ok(())) => None,
            Ok(Err(e)) => Some((e, true)),
            Err(e) => Some((e, false)),
        };
        if let Some((e, reading)) = failed {
            for &k in kids.iter() {
                self.free_node(mem, k);
            }
            if !reading {
                return Err(e);
            }
            self.leave_level(sys, n)?;
            self.node_mut(n)?.children = Children::Unreadable(e);
            return Ok(());
        }
        if let Some(c) = self.compar {
            let mut v = TryVec::with_capacity(kids.len())?;
            for &k in kids.iter() {
                v.push((self.block(k), k))?;
            }
            sort_slots(&mut v, |p| p.0, c)?;
            kids.clear();
            for (_, k) in v.iter() {
                kids.push(*k)?;
            }
        }
        self.link(mem, &kids);
        let x = self.node_mut(n)?;
        x.next_child = 0;
        x.children = if how == Build::NameOnly { Children::NameOnly(kids) } else { Children::Built(kids) };
        Ok(())
    }

    /// Close entry `n`'s directory and, in chdir mode, go back to its parent.
    fn leave_level<S: Sys>(&mut self, sys: &S, n: usize) -> Result<()> {
        self.unpush(sys, n);
        if self.chdir_mode() {
            let x = self.node(n)?;
            let to = if x.root {
                self.orig.unwrap_or(Fd::CWD)
            } else {
                let p = usize::try_from(x.level.saturating_sub(1)).map_err(|_| Errno::EINVAL)?;
                self.levels.fd(sys, p)?
            };
            sys.fchdir(to)?;
        }
        Ok(())
    }

    /// Make `n` the current entry and return its block: a root gets its last component as its
    /// name, and a pending `FTS_FOLLOW` (from `fts_children`) is applied.
    fn visit<S: Sys, M: FtsMem>(&mut self, sys: &S, mem: &mut M, n: usize) -> Result<usize> {
        let (root, inf) = {
            let x = self.node(n)?;
            (x.root, x.info)
        };
        let instr = self.instr_of(mem, n);
        if root {
            let x = self.node(n)?;
            let cut = x.arg.iter().rposition(|&b| b == b'/').map_or(0, |i| i.saturating_add(1));
            let last = x.arg.get(cut..).unwrap_or(&[]);
            if let Some(d) = mem.bytes(x.block, off::NAME.saturating_add(x.name_cap)).get_mut(off::NAME..) {
                d.fill(0);
                put(d, 0, last);
            }
            let len = last.len();
            self.node_mut(n)?.name_len = len;
        }
        self.place(mem, n)?;
        if instr == FTS_FOLLOW && (inf == info::SL || inf == info::SLNONE) {
            self.set_instr(mem, n, FTS_NOINSTR);
            self.restat(sys, mem, n, true)?;
        }
        // Every other entry's block is current: written when it was made, and by every change
        // since (cycle check, `fts_link`, `fts_set`, a restat, a buffer move). A root's name
        // changed above. Writing every entry again cost ~316 instructions each (FTS-FLAT-PROFILE).
        if root {
            self.write_header(mem, n);
        }
        self.cur = Some(n);
        Ok(self.block(n))
    }

    /// `fts_read`: the next entry, or `None` at the end.
    pub fn read<S: Sys, M: FtsMem>(&mut self, sys: &S, mem: &mut M) -> Result<Option<usize>> {
        match self.state {
            State::Done => return Ok(None),
            State::Start => {
                self.state = State::Walking;
                let Some(&first) = self.roots.first() else {
                    self.state = State::Done;
                    return Ok(None);
                };
                return self.visit(sys, mem, first).map(Some);
            }
            State::Walking => {}
        }
        let Some(cur) = self.cur else { return Ok(None) };
        let inf = self.node(cur)?.info;
        let instr = self.instr_of(mem, cur);
        // The instruction is used up: the entry reads `FTS_NOINSTR` again. Only when it holds
        // another value, so the common walk writes nothing here.
        if instr != FTS_NOINSTR {
            self.set_instr(mem, cur, FTS_NOINSTR);
        }
        if instr == FTS_AGAIN {
            let follow = self.node(cur)?.follow;
            self.restat(sys, mem, cur, follow)?;
            return self.visit(sys, mem, cur).map(Some);
        }
        if instr == FTS_FOLLOW && (inf == info::SL || inf == info::SLNONE) {
            self.restat(sys, mem, cur, true)?;
            return self.visit(sys, mem, cur).map(Some);
        }
        if inf == info::D {
            let (dev, root_dev) = {
                let x = self.node(cur)?;
                (x.stat.map_or(0, |s| s.dev), x.root_dev)
            };
            if instr == FTS_SKIP || (self.options & FTS_XDEV != 0 && dev != root_dev) {
                self.unpush(sys, cur);
                self.drop_children(mem, cur);
                self.node_mut(cur)?.info = info::DP;
                self.write_header(mem, cur);
                self.terminate(mem, cur);
                return Ok(Some(self.block(cur)));
            }
            if matches!(self.node(cur)?.children, Children::NameOnly(_)) {
                self.drop_children(mem, cur);
            }
            self.build(sys, mem, cur, Build::Read)?;
            let first = match &self.node(cur)?.children {
                Children::Unreadable(e) => {
                    let e = e.get();
                    let x = self.node_mut(cur)?;
                    x.info = info::DNR;
                    x.errno = e;
                    self.write_header(mem, cur);
                    self.terminate(mem, cur);
                    return Ok(Some(self.block(cur)));
                }
                Children::NoSearch(e) => {
                    let e = e.get();
                    let x = self.node_mut(cur)?;
                    x.info = info::DP;
                    x.errno = e;
                    self.write_header(mem, cur);
                    self.terminate(mem, cur);
                    return Ok(Some(self.block(cur)));
                }
                Children::Built(l) => l.iter().position(|&c| !self.passed_over(mem, c)).and_then(|i| Some((i, *l.get(i)?))),
                _ => None,
            };
            // Entries built by `fts_children` were read without staying in the directory.
            if self.chdir_mode() {
                let lvl = usize::try_from(self.node(cur)?.level).map_err(|_| Errno::EINVAL)?;
                let fd = self.levels.fd(sys, lvl)?;
                sys.fchdir(fd)?;
            }
            match first {
                Some((i, c)) => {
                    self.node_mut(cur)?.next_child = i.saturating_add(1);
                    return self.visit(sys, mem, c).map(Some);
                }
                None => {
                    self.leave_level(sys, cur)?;
                    self.drop_children(mem, cur);
                    self.node_mut(cur)?.info = info::DP;
                    self.write_header(mem, cur);
                    self.terminate(mem, cur);
                    return Ok(Some(self.block(cur)));
                }
            }
        }
        // The next sibling, or the parent in post-order.
        let parent = self.node(cur)?.parent;
        if parent == ROOT_PARENT {
            let pos = self.roots.iter().position(|&r| r == cur).ok_or(Errno::EINVAL)?;
            return match self.roots.get(pos.saturating_add(1)).copied() {
                Some(next) => self.visit(sys, mem, next).map(Some),
                None => {
                    self.state = State::Done;
                    self.cur = None;
                    Ok(None)
                }
            };
        }
        // One lookup of the parent unless an entry may be passed over (FTS-FLAT-FIXES-2: the
        // second lookup and the search cost ~25 instructions per entry).
        let next = if self.skips_listed {
            let p = self.node(parent)?;
            let found = match &p.children {
                Children::Built(l) => l.iter().enumerate().skip(p.next_child).find(|&(_, &c)| !self.passed_over(mem, c)).map(|(i, &c)| (i, c)),
                _ => None,
            };
            if let Some((i, _)) = found {
                self.node_mut(parent)?.next_child = i.saturating_add(1);
            }
            found.map(|(_, c)| c)
        } else {
            let p = self.node_mut(parent)?;
            let next = match &p.children {
                Children::Built(l) => l.get(p.next_child).copied(),
                _ => None,
            };
            if next.is_some() {
                p.next_child = p.next_child.saturating_add(1);
            }
            next
        };
        if let Some(s) = next {
            return self.visit(sys, mem, s).map(Some);
        }
        self.leave_level(sys, parent)?;
        self.drop_children(mem, parent);
        self.node_mut(parent)?.info = info::DP;
        self.write_header(mem, parent);
        self.terminate(mem, parent);
        self.cur = Some(parent);
        Ok(Some(self.block(parent)))
    }

    /// `fts_children(ftsp, options)`: the first entry of the current directory's list (the roots
    /// before the first `fts_read`), `None` (no error) when there is none.
    pub fn children<S: Sys, M: FtsMem>(&mut self, sys: &S, mem: &mut M, options: i32) -> Result<Option<usize>> {
        if options != 0 && options != FTS_NAMEONLY {
            return Err(Errno::EINVAL);
        }
        match self.state {
            State::Done => return Ok(None),
            State::Start => return Ok(self.roots.first().map(|&r| self.block(r))),
            State::Walking => {}
        }
        let Some(cur) = self.cur else { return Ok(None) };
        if self.node(cur)?.info != info::D {
            return Ok(None);
        }
        let how = if options == FTS_NAMEONLY { Build::NameOnly } else { Build::Children };
        self.build(sys, mem, cur, how)?;
        // The caller may write `FTS_SKIP` into any listed entry without `fts_set`
        // (FTS-INSTR-DIRECT): from now on the walk looks for entries to pass over. A
        // `FTS_NAMEONLY` list is dropped by the next `fts_read`, so its marks never count.
        if how == Build::Children {
            self.skips_listed = true;
        }
        let first = match &self.node(cur)?.children {
            Children::Unreadable(e) | Children::NoSearch(e) => return Err(*e),
            Children::Built(l) | Children::NameOnly(l) => l.first().copied(),
            Children::NotBuilt => None,
        };
        // Building may have entered the directory; the caller's view does not move.
        if self.chdir_mode() && how != Build::NameOnly {
            let x = self.node(cur)?;
            let to = if x.root {
                self.orig.unwrap_or(Fd::CWD)
            } else {
                let p = usize::try_from(x.level.saturating_sub(1)).map_err(|_| Errno::EINVAL)?;
                self.levels.fd(sys, p)?
            };
            sys.fchdir(to)?;
        }
        Ok(first.map(|c| self.block(c)))
    }

    /// `fts_set(ftsp, f, instr)`.
    pub fn set<M: FtsMem>(&mut self, mem: &mut M, entry: usize, instr: i32) -> Result<()> {
        let instr = match u16::try_from(instr) {
            Ok(i @ (0 | FTS_AGAIN | FTS_FOLLOW | FTS_NOINSTR | FTS_SKIP)) => i,
            _ => return Err(Errno::EINVAL),
        };
        let n = self.nodes.iter().position(|s| s.as_ref().is_some_and(|x| x.block == entry && entry != 0)).ok_or(Errno::EINVAL)?;
        if instr == FTS_SKIP && self.cur != Some(n) {
            self.skips_listed = true;
        }
        self.set_instr(mem, n, instr);
        Ok(())
    }

    /// `fts_close`: every entry freed, every descriptor closed, and in chdir mode the caller's
    /// working directory restored.
    pub fn close<S: Sys, M: FtsMem>(mut self, sys: &S, mem: &mut M) -> Result<()> {
        self.levels.close_all(sys);
        let mut r = Ok(());
        if let Some(o) = self.orig.take() {
            r = sys.fchdir(o);
            let _ = sys.close(o);
        }
        for i in 0..self.nodes.len() {
            if let Some(Some(x)) = self.nodes.get_mut(i).map(Option::take) {
                mem.free(x.block);
            }
        }
        if self.buf != 0 {
            mem.free(self.buf);
        }
        r
    }
}
