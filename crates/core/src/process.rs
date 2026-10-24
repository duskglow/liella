//! Program execution, spawning, child status and the shell interfaces (spec §18.3–§18.9;
//! D-18.2 to D-18.5).
//!
//! * **Exec search** (D-18.2): [`search`] runs one `exec` per candidate of `PATH` and decides,
//!   from each failure, whether the search goes on (`ENOENT`, `ENOTDIR`, and `EACCES`, which is
//!   remembered and reported last) or stops; `ENOEXEC` runs the file with the shell when the
//!   caller asks for that (the `p` exec forms, §18.3), and stops the search otherwise (the spawn
//!   forms). Candidates are built on the stack: the search allocates nothing, so a spawned child
//!   can run it.
//! * **Spawn** (D-18.3): the file actions are a typed enum ([`FileAction`]) kept in a compact
//!   byte encoding (the storage of the C object); [`run_child`] performs the attributes and the
//!   actions in a new child, in the order of posix_spawn(3), and then the exec. It uses only the
//!   kernel (`Sys`): no allocation and no lock, as a child of a multi-threaded caller requires.
//!   Every signal stays blocked until just before the exec, where the requested mask is set.
//! * **Child status** (D-18.4): [`ChildStatus`] and the wait-status word.
//! * **Shell interfaces** (D-18.5): `popen`'s mode ([`popen_mode`]).

use liella_kernel::errno::Errno;
use liella_kernel::fd::Fd;
use liella_kernel::ops::{FcntlInt, IoctlIntIn};
use liella_kernel::path::{KStr, PATH_MAX};
use liella_kernel::sys::{Disposition, Sys};
use liella_kernel::uapi::SigSet;

use crate::error::Result;
use crate::mem::TryVec;

// ------------------------------------------------------------------------------ exec search (§18.3)

/// The search list when `PATH` is not set (§18.3; the working directory is not searched).
pub const DEFAULT_PATH: &[u8] = b"/bin:/usr/bin";

/// Whether a failed candidate lets the search go on: the file is not there (`ENOENT`, also a
/// missing `#!` interpreter), a path element is not a directory (`ENOTDIR`), or it may not be
/// executed (`EACCES`, reported only if no later entry runs). Every other error is the
/// answer (exec(3): `ELOOP`, `ETXTBSY`, `ENAMETOOLONG`, `E2BIG`, ...).
pub fn search_continues(e: Errno) -> bool {
    e == Errno::ENOENT || e == Errno::ENOTDIR || e == Errno::EACCES
}

/// An exec of one file: the error it failed with (an exec that succeeds does not return).
pub type Exec<'f> = &'f mut dyn FnMut(KStr<'_>) -> Errno;

/// Execute `name` the way the `p` forms do (§18.3): a name with a `/` is executed as given; an
/// empty name is `ENOENT`; otherwise every element of `path` (`None`: [`DEFAULT_PATH`]) is
/// tried in order, an empty element meaning the working directory. A file the kernel refuses
/// with `ENOEXEC` is run by `shell` when given (the search ends with its result), and is the
/// error otherwise. The result is the error the search ends with.
///
/// `exec` is generic, not `dyn`: it runs once per `PATH` element, and a concrete closure lets
/// the caller's exec system call inline into the loop (SMALL-OPEN-ROWS; EXECVP-PATH-2 left
/// about 62 instructions per element above glibc, part of it this indirect call). `shell`
/// runs at most once per search and stays an [`Exec`].
pub fn search<E>(name: KStr<'_>, path: Option<&[u8]>, exec: &mut E, mut shell: Option<Exec<'_>>) -> Errno
where
    E: FnMut(KStr<'_>) -> Errno + ?Sized,
{
    let bytes = name.bytes();
    if bytes.is_empty() {
        return Errno::ENOENT;
    }
    let mut one = |candidate: KStr<'_>| match exec(candidate) {
        e if e == Errno::ENOEXEC => match shell.as_mut() {
            Some(sh) => (sh(candidate), true),
            None => (e, true),
        },
        e => (e, !search_continues(e)),
    };
    if bytes.contains(&b'/') {
        return one(name).0;
    }
    let mut eacces = false;
    // One buffer for the whole search, with `name` and its terminator placed once at its end:
    // each candidate is then its element and a `/` written just before the name, in place, and
    // costs only the element's bytes (a long `PATH` is common: EXECVP-PATH measured a 4 KiB
    // clear per element, EXECVP-PATH-2 the per-element split, copies and scans of the name).
    let tail = name.with_nul();
    let mut buf = [0u8; PATH_MAX];
    let Some(start) = PATH_MAX.checked_sub(tail.len()) else {
        // No candidate fits (every one ends with the whole name): the first one's answer.
        return Errno::ENAMETOOLONG;
    };
    if let Some(slot) = buf.get_mut(start..) {
        slot.copy_from_slice(tail);
    }
    let mut rest = path.unwrap_or(DEFAULT_PATH);
    let last = loop {
        let end = element_end(rest);
        let (element, after) = rest.split_at_checked(end).unwrap_or((rest, &[]));
        let (e, stop) = match after.first() {
            // An element with a NUL cannot be a path (the kernel would stop at it).
            Some(&0) => (Errno::EINVAL, true),
            _ => match candidate(&mut buf, start, element) {
                Ok(c) => one(c),
                Err(e) => (e, !search_continues(e)),
            },
        };
        if stop {
            return e;
        }
        if e == Errno::EACCES {
            eacces = true;
        }
        match after.split_first() {
            Some((_, more)) => rest = more,
            None => break e,
        }
    };
    if eacces { Errno::EACCES } else { last }
}

/// The index of the first `:` or NUL in `s` (`s.len()` when there is none), a word at a time:
/// a `PATH` element is usually tens of bytes, and this runs once per element.
fn element_end(s: &[u8]) -> usize {
    const LO: u64 = u64::from_ne_bytes([0x01; 8]);
    const HI: u64 = u64::from_ne_bytes([0x80; 8]);
    const COLON: u64 = u64::from_ne_bytes([b':'; 8]);
    let mut words = s.chunks_exact(8);
    let mut at = 0usize;
    for w in &mut words {
        // Always 8 bytes (an impossible short word reads as a NUL, ending the element).
        let w = u64::from_le_bytes(w.try_into().unwrap_or([0; 8]));
        let c = w ^ COLON;
        // A byte's high bit is set where it is zero in `w` or in `c`; a false mark can only sit
        // above a true one (the borrow runs upward), so the lowest mark is the first match.
        let marks = (w.wrapping_sub(LO) & !w | c.wrapping_sub(LO) & !c) & HI;
        if marks != 0 {
            let byte = usize::try_from(marks.trailing_zeros() / 8).unwrap_or(0);
            return at.wrapping_add(byte);
        }
        at = at.wrapping_add(8);
    }
    let tail = words.remainder();
    at.wrapping_add(tail.iter().position(|&b| b == b':' || b == 0).unwrap_or(tail.len()))
}

/// The candidate for `element` in `buf`, whose bytes from `start` hold the name and its
/// terminator: `element/name`, or the name alone for an empty element; `ENAMETOOLONG` when it
/// does not fit in `PATH_MAX` (the kernel's answer for such a path). `element` holds no NUL
/// and the name none but its terminator, so only the last byte is checked. Writes only the
/// element and the `/`.
fn candidate<'b>(buf: &'b mut [u8; PATH_MAX], start: usize, element: &[u8]) -> Result<KStr<'b>> {
    let first = if element.is_empty() {
        start
    } else {
        let slash = start.checked_sub(1).ok_or(Errno::ENAMETOOLONG)?;
        let first = slash.checked_sub(element.len()).ok_or(Errno::ENAMETOOLONG)?;
        buf.get_mut(first..slash).ok_or(Errno::ENAMETOOLONG)?.copy_from_slice(element);
        *buf.get_mut(slash).ok_or(Errno::ENAMETOOLONG)? = b'/';
        first
    };
    KStr::from_nul_free_terminated(buf.get(first..).ok_or(Errno::ENAMETOOLONG)?)
}

// ------------------------------------------------------------------------------ spawn (§18.4)

/// `POSIX_SPAWN_RESETIDS`: the child takes the real user and group IDs as effective ones.
pub const SPAWN_RESETIDS: u32 = 0x01;
/// `POSIX_SPAWN_SETPGROUP`.
pub const SPAWN_SETPGROUP: u32 = 0x02;
/// `POSIX_SPAWN_SETSIGDEF`.
pub const SPAWN_SETSIGDEF: u32 = 0x04;
/// `POSIX_SPAWN_SETSIGMASK`.
pub const SPAWN_SETSIGMASK: u32 = 0x08;
/// `POSIX_SPAWN_SETSCHEDPARAM`.
pub const SPAWN_SETSCHEDPARAM: u32 = 0x10;
/// `POSIX_SPAWN_SETSCHEDULER`.
pub const SPAWN_SETSCHEDULER: u32 = 0x20;
/// `POSIX_SPAWN_USEVFORK` (no effect).
pub const SPAWN_USEVFORK: u32 = 0x40;
/// `POSIX_SPAWN_SETSID`.
pub const SPAWN_SETSID: u32 = 0x80;
/// `POSIX_SPAWN_SETCGROUP`.
pub const SPAWN_SETCGROUP: u32 = 0x100;
/// Every defined flag.
pub const SPAWN_FLAGS: u32 = 0x1ff;

/// The flags of `posix_spawnattr_setflags`: `EINVAL` for a bit that names no flag.
pub fn spawn_flags(flags: i32) -> Result<u32> {
    u32::try_from(flags).ok().filter(|f| f & !SPAWN_FLAGS == 0).ok_or(Errno::EINVAL)
}

/// The policies `posix_spawnattr_setschedpolicy` accepts: every policy `sched_setscheduler`
/// can set (`SCHED_OTHER`, `SCHED_FIFO`, `SCHED_RR`, `SCHED_BATCH`, `SCHED_IDLE`); `EINVAL`
/// otherwise (`SCHED_DEADLINE` needs `sched_setattr`).
pub fn spawn_policy(policy: i32) -> Result<u32> {
    match policy {
        0..=3 | 5 => u32::try_from(policy).map_err(|_| Errno::EINVAL),
        _ => Err(Errno::EINVAL),
    }
}

/// A spawn attributes object's values (the `posix_spawnattr_*` setters write them).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SpawnAttr {
    /// `POSIX_SPAWN_*` flags.
    pub flags: u32,
    /// The process group for `SETPGROUP` (0: the child's own).
    pub pgroup: i32,
    /// The signals `SETSIGDEF` resets to their default action.
    pub sigdefault: SigSet,
    /// The mask `SETSIGMASK` installs.
    pub sigmask: SigSet,
    /// The policy for `SETSCHEDULER`.
    pub policy: u32,
    /// The priority for `SETSCHEDULER` and `SETSCHEDPARAM`.
    pub priority: i32,
    /// The cgroup directory descriptor for `SETCGROUP`.
    pub cgroup: i32,
}

/// One file action (§18.4), applied in the child in the order they were added.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileAction<'a> {
    /// `addopen`: open `path` and make it descriptor `fd`.
    Open {
        /// The descriptor the file becomes.
        fd: i32,
        /// The file.
        path: KStr<'a>,
        /// `open` flags.
        flags: u32,
        /// `open` mode.
        mode: u32,
    },
    /// `addclose`: close a descriptor (one that is not open is no error).
    Close(i32),
    /// `adddup2(fd, newfd)`; with `fd == newfd` it clears `FD_CLOEXEC`.
    Dup2(i32, i32),
    /// `addchdir_np`.
    Chdir(KStr<'a>),
    /// `addfchdir_np`.
    Fchdir(i32),
    /// `addclosefrom_np`: close every descriptor from this one up.
    Closefrom(i32),
    /// `addtcsetpgrp_np`: make the child's process group the terminal's foreground group.
    Tcsetpgrp(i32),
}

const TAG_OPEN: u8 = 1;
const TAG_CLOSE: u8 = 2;
const TAG_DUP2: u8 = 3;
const TAG_CHDIR: u8 = 4;
const TAG_FCHDIR: u8 = 5;
const TAG_CLOSEFROM: u8 = 6;
const TAG_TCSETPGRP: u8 = 7;
/// Bytes of a record before its path: the tag, three 32-bit values, the path's length.
const HEAD: usize = 17;

/// Append `action` to an encoded action list: a tag byte, three native-endian 32-bit values
/// and the path (with its terminator, after its length) for the actions that name one.
pub fn encode(action: &FileAction<'_>, out: &mut TryVec<u8>) -> Result<()> {
    let (tag, a, b, c, path): (u8, i32, u32, u32, &[u8]) = match *action {
        FileAction::Open { fd, path, flags, mode } => (TAG_OPEN, fd, flags, mode, path.with_nul()),
        FileAction::Close(fd) => (TAG_CLOSE, fd, 0, 0, b""),
        FileAction::Dup2(fd, newfd) => (TAG_DUP2, fd, u32::from_ne_bytes(newfd.to_ne_bytes()), 0, b""),
        FileAction::Chdir(path) => (TAG_CHDIR, 0, 0, 0, path.with_nul()),
        FileAction::Fchdir(fd) => (TAG_FCHDIR, fd, 0, 0, b""),
        FileAction::Closefrom(fd) => (TAG_CLOSEFROM, fd, 0, 0, b""),
        FileAction::Tcsetpgrp(fd) => (TAG_TCSETPGRP, fd, 0, 0, b""),
    };
    let len = u32::try_from(path.len()).map_err(|_| Errno::ENOMEM)?;
    out.reserve(HEAD.saturating_add(path.len()))?;
    out.push(tag)?;
    for v in [u32::from_ne_bytes(a.to_ne_bytes()), b, c, len] {
        out.extend_from_slice(&v.to_ne_bytes())?;
    }
    out.extend_from_slice(path)
}

/// The actions of an encoded list, in order (decoding stops at a malformed record, which only
/// a caller that wrote into the C object itself can produce).
pub fn actions(list: &[u8]) -> Actions<'_> {
    Actions(list)
}

/// See [`actions`].
#[derive(Clone, Debug)]
pub struct Actions<'a>(&'a [u8]);

fn word(b: &[u8], at: usize) -> Option<u32> {
    let end = at.checked_add(4)?;
    Some(u32::from_ne_bytes(b.get(at..end)?.try_into().ok()?))
}

impl<'a> Iterator for Actions<'a> {
    type Item = FileAction<'a>;

    fn next(&mut self) -> Option<FileAction<'a>> {
        let b = self.0;
        let (&tag, _) = b.split_first()?;
        self.0 = &[];
        let int = |at: usize| word(b, at).map(|v| i32::from_ne_bytes(v.to_ne_bytes()));
        let (a, x, y) = (int(1)?, word(b, 5)?, word(b, 9)?);
        let len = usize::try_from(word(b, 13)?).ok()?;
        let end = HEAD.checked_add(len)?;
        let path = b.get(HEAD..end)?;
        let kpath = || KStr::from_nul_terminated(path).ok();
        let action = match tag {
            TAG_OPEN => FileAction::Open { fd: a, path: kpath()?, flags: x, mode: y },
            TAG_CLOSE => FileAction::Close(a),
            TAG_DUP2 => FileAction::Dup2(a, i32::from_ne_bytes(x.to_ne_bytes())),
            TAG_CHDIR => FileAction::Chdir(kpath()?),
            TAG_FCHDIR => FileAction::Fchdir(a),
            TAG_CLOSEFROM => FileAction::Closefrom(a),
            TAG_TCSETPGRP => FileAction::Tcsetpgrp(a),
            _ => return None,
        };
        self.0 = b.get(end..).unwrap_or(&[]);
        Some(action)
    }
}

/// The program a spawned child runs.
#[derive(Clone, Copy, Debug)]
pub enum Program<'a> {
    /// `posix_spawn`: this path (relative paths resolve after the actions ran).
    Path(KStr<'a>),
    /// `posix_spawnp`: [`search`] for `name` in `path` (the caller's `PATH`), without the
    /// shell for `ENOEXEC`.
    Search {
        /// The name to look for.
        name: KStr<'a>,
        /// The caller's `PATH` (`None`: unset).
        path: Option<&'a [u8]>,
    },
}

/// What a spawned child does before its program runs.
#[derive(Clone, Copy, Debug)]
pub struct Child<'a> {
    /// The attributes (all zero without an attributes object).
    pub attr: SpawnAttr,
    /// The encoded file actions.
    pub actions: &'a [u8],
    /// The mask the program starts with: the attribute's with `SETSIGMASK`, else the caller's.
    pub mask: SigSet,
    /// Reset every caught signal to its default action first (when the kernel could not do it
    /// at creation, `CLONE_CLEAR_SIGHAND`).
    pub reset_handlers: bool,
    /// The program.
    pub program: Program<'a>,
}

const SIGKILL: i32 = 9;
const SIGSTOP: i32 = 19;
const SIG_SETMASK: u32 = 2;
const FD_CLOEXEC: i64 = 1;

/// In a new child whose signals are all blocked: apply the attributes (dispositions, session,
/// process group, scheduling, IDs, in the order of posix_spawn(3)), then the file actions in
/// order, then the mask, then run the program through `exec`. Returns the error that stopped it
/// (the child then reports it and exits); it does not return when the program runs. `exec` is
/// generic for the same reason as in [`search`].
pub fn run_child<S: Sys, E>(sys: &S, child: &Child<'_>, exec: &mut E) -> Errno
where
    E: FnMut(KStr<'_>) -> Errno + ?Sized,
{
    match prepare(sys, child) {
        Err(e) => e,
        Ok(()) => match child.program {
            Program::Path(p) => exec(p),
            Program::Search { name, path } => search(name, path, exec, None),
        },
    }
}

fn prepare<S: Sys>(sys: &S, child: &Child<'_>) -> Result<()> {
    let a = &child.attr;
    if child.reset_handlers {
        for sig in (1..=64).filter(|&s| s != SIGKILL && s != SIGSTOP) {
            if sys.sigaction_get(sig).is_ok_and(|k| k.handler > 1) {
                let _ = sys.sigaction_set(sig, Disposition::Default, 0, SigSet::empty());
            }
        }
    }
    if a.flags & SPAWN_SETSIGDEF != 0 {
        // `SIGKILL` and `SIGSTOP` are always at their default.
        for sig in (1..=64).filter(|&s| s != SIGKILL && s != SIGSTOP && a.sigdefault.contains(s)) {
            let _ = sys.sigaction_set(sig, Disposition::Default, 0, SigSet::empty());
        }
    }
    if a.flags & SPAWN_SETSID != 0 {
        sys.setsid()?;
    }
    if a.flags & SPAWN_SETPGROUP != 0 {
        sys.setpgid(0, a.pgroup)?;
    }
    let param = liella_kernel::uapi::SchedParam { priority: a.priority };
    if a.flags & SPAWN_SETSCHEDULER != 0 {
        sys.sched_setscheduler(0, a.policy, &param)?;
    } else if a.flags & SPAWN_SETSCHEDPARAM != 0 {
        sys.sched_setparam(0, &param)?;
    }
    if a.flags & SPAWN_RESETIDS != 0 {
        sys.setresgid(u32::MAX, sys.getgid(), u32::MAX)?;
        sys.setresuid(u32::MAX, sys.getuid(), u32::MAX)?;
    }
    for action in actions(child.actions) {
        apply(sys, &action)?;
    }
    sys.sigprocmask(SIG_SETMASK, Some(&child.mask))?;
    Ok(())
}

fn apply<S: Sys>(sys: &S, action: &FileAction<'_>) -> Result<()> {
    match *action {
        FileAction::Open { fd, path, flags, mode } => {
            let got = sys.openat(Fd::CWD, path, flags, mode)?;
            if got.raw() != fd {
                let r = sys.dup2(got, Fd::new(fd));
                let _ = sys.close(got);
                r?;
            }
        }
        // Closing a descriptor that is not open is no error; Linux closes it whatever else
        // `close` reports.
        FileAction::Close(fd) => {
            let _ = sys.close(Fd::new(fd));
        }
        FileAction::Dup2(fd, newfd) if fd == newfd => {
            let flags = sys.fcntl_int(Fd::new(fd), FcntlInt::GetFd, 0)?;
            sys.fcntl_int(Fd::new(fd), FcntlInt::SetFd, flags & !FD_CLOEXEC)?;
        }
        FileAction::Dup2(fd, newfd) => {
            sys.dup2(Fd::new(fd), Fd::new(newfd))?;
        }
        FileAction::Chdir(path) => sys.chdir(path)?,
        FileAction::Fchdir(fd) => sys.fchdir(Fd::new(fd))?,
        FileAction::Closefrom(fd) => {
            closefrom_child(sys, u32::try_from(fd).map_err(|_| Errno::EBADF)?)?;
        }
        FileAction::Tcsetpgrp(fd) => {
            sys.ioctl_set_int(Fd::new(fd), IoctlIntIn::Tiocspgrp, sys.getpgrp())?;
        }
    }
    Ok(())
}

/// The `Closefrom` action: `close_range` first; where the kernel lacks it (`ENOSYS`, or `EINVAL`
/// from a filter), close what `/proc/self/fd` lists from `lo` up, in passes (closing while
/// reading a directory can skip entries) until a pass closes nothing; without `/proc`, every
/// descriptor from `lo` to the soft `RLIMIT_NOFILE` limit. No allocation: the child may share
/// its parent's memory. The walk costs one pass per batch of open descriptors, not per possible
/// descriptor, so a huge limit costs nothing there.
fn closefrom_child<S: Sys>(sys: &S, lo: u32) -> Result<()> {
    match sys.close_range(lo, u32::MAX, 0) {
        Err(Errno::ENOSYS | Errno::EINVAL) => {}
        r => return r,
    }
    let walked = (|| -> Result<()> {
        loop {
            let dir = sys.openat(Fd::CWD, KStr::from_nul_terminated(b"/proc/self/fd\0")?, 0o200000 | 0o2000000, 0)?;
            let mut buf = [0u8; 1024];
            let mut closed = false;
            let mut res = Ok(());
            'read: loop {
                let n = match sys.getdents64(dir, &mut buf) {
                    Ok(0) => break,
                    Ok(n) => n,
                    Err(e) => {
                        res = Err(e);
                        break;
                    }
                };
                let mut off = 0usize;
                while let Some(rec) = buf.get(off..n) {
                    let Some(reclen) = rec.get(16..18).and_then(|b| <[u8; 2]>::try_from(b).ok()).map(|b| usize::from(u16::from_ne_bytes(b))) else { break };
                    if reclen == 0 {
                        break 'read;
                    }
                    let name = rec.get(19..reclen).unwrap_or(&[]);
                    let name = name.split(|&b| b == 0).next().unwrap_or(&[]);
                    let fd = (!name.is_empty()).then(|| name.iter().try_fold(0u32, |a, &b| a.checked_mul(10)?.checked_add(char::from(b).to_digit(10)?))).flatten();
                    if let Some(fd) = fd.filter(|&fd| fd >= lo && i64::from(fd) != i64::from(dir.raw())) {
                        let _ = sys.close(Fd::new(i32::try_from(fd).unwrap_or(-1)));
                        closed = true;
                    }
                    off = off.saturating_add(reclen);
                }
            }
            let _ = sys.close(dir);
            res?;
            if !closed {
                return Ok(());
            }
        }
    })();
    if walked.is_ok() {
        return Ok(());
    }
    let limit = sys.prlimit(0, liella_kernel::uapi::RLIMIT_NOFILE, None)?.cur;
    let top = u32::try_from(limit).unwrap_or(u32::MAX);
    for fd in lo..top {
        let Ok(raw) = i32::try_from(fd) else { break };
        let _ = sys.close(Fd::new(raw));
    }
    Ok(())
}

// ------------------------------------------------------------------------------ child status (§18.6)

/// How a child changed state (D-18.4): the meaning of a wait-status word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChildStatus {
    /// It exited with this code (the low 8 bits of the value it passed, ⚑ KERN).
    Exited(u8),
    /// A signal ended it.
    Killed {
        /// The signal.
        signal: u8,
        /// Whether a core file was written.
        core: bool,
    },
    /// A signal stopped it.
    Stopped(u8),
    /// `SIGCONT` resumed it.
    Continued,
}

impl ChildStatus {
    /// The wait-status word (`waitpid`'s `*status`): exit code `c` is `c << 8`, a signal `s` is
    /// `s` (with `0x80` for a core dump), a stop by `s` is `(s << 8) | 0x7f`, a continue
    /// `0xffff`.
    pub fn word(self) -> i32 {
        match self {
            ChildStatus::Exited(c) => i32::from(c) << 8,
            ChildStatus::Killed { signal, core } => i32::from(signal & 0x7f) | if core { 0x80 } else { 0 },
            ChildStatus::Stopped(s) => (i32::from(s) << 8) | 0x7f,
            ChildStatus::Continued => 0xffff,
        }
    }

    /// The meaning of a wait-status word (`None` for a word no child state produces).
    pub fn from_word(w: i32) -> Option<ChildStatus> {
        let high = u8::try_from((w >> 8) & 0xff).ok()?;
        let low = w & 0x7f;
        if w == 0xffff {
            Some(ChildStatus::Continued)
        } else if low == 0 && w & !0xff00 == 0 {
            Some(ChildStatus::Exited(high))
        } else if low == 0x7f && w & !0xff7f == 0 {
            Some(ChildStatus::Stopped(high))
        } else if w & !0xff == 0 {
            Some(ChildStatus::Killed { signal: u8::try_from(low).ok()?, core: w & 0x80 != 0 })
        } else {
            None
        }
    }
}

// ------------------------------------------------------------------------------ process descriptors (§18.5)

/// The `Pid:` value of a `/proc/self/fdinfo/N` text: `None` when the descriptor is not a
/// pidfd (no such line), otherwise the value (`-1` once the process is reaped, `0` when it is
/// outside the caller's PID namespace).
pub fn fdinfo_pid(text: &[u8]) -> Option<i64> {
    text.split(|&b| b == b'\n').find_map(|line| {
        let v = line.strip_prefix(b"Pid:")?;
        core::str::from_utf8(v).ok()?.trim().parse().ok()
    })
}

// ------------------------------------------------------------------------------ shell interfaces (§18.7)

/// A `popen` mode: the direction, and close-on-exec for the caller's end (`e`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PopenMode {
    /// `r`: the caller reads the command's standard output (else `w`: writes its input).
    pub read: bool,
    /// `e`: the stream's descriptor is close-on-exec.
    pub cloexec: bool,
}

/// `popen`'s mode string (§18.7, popen(3)): exactly one of `r` and `w`, and at most one `e`,
/// in any order; anything else is `EINVAL`.
pub fn popen_mode(mode: &[u8]) -> Result<PopenMode> {
    let count = |c: u8| mode.iter().filter(|&&b| b == c).count();
    let (r, w, e) = (count(b'r'), count(b'w'), count(b'e'));
    if r.saturating_add(w) != 1 || e > 1 || r.saturating_add(w).saturating_add(e) != mode.len() {
        return Err(Errno::EINVAL);
    }
    Ok(PopenMode { read: r == 1, cloexec: e == 1 })
}
