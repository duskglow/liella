//! Hidden iteration state of the `*ent` enumerations (H3; D-17.3, D-16.2): a process-wide,
//! locked position in one database file (P-scope: the contract describes one enumeration per
//! process, and threads may share it).
//!
//! A [`Cursor`] keeps the file open between steps and the offset of the next line; each step
//! reads lines from there until the caller's parser takes one ([`Step`]). A record that cannot
//! be returned now (`ERANGE`) leaves the cursor on it, so that a retry with a larger buffer
//! returns it.

use core::sync::atomic::{AtomicI32, AtomicU64, Ordering::Relaxed};

use liella::error::{Errno, Result};
use liella::mem::TryVec;
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{O_CLOEXEC, O_RDONLY};
use liella_rt::sync::RawMutex;

/// What an enumeration does with the line it reads.
#[derive(Debug)]
pub enum Step<T> {
    /// No record (a comment, a line that does not parse): go on to the next line.
    Skip,
    /// The record: the cursor moves past it.
    Take(T),
    /// The record cannot be returned now (`ERANGE`): the cursor stays on it.
    Stay(Errno),
}

/// A process-wide enumeration of one database file (`setpwent`/`getpwent`/`endpwent`, and the
/// other databases' equivalents): the file stays open between steps, and the position is the
/// offset of the next line.
#[derive(Debug)]
pub struct Cursor {
    lock: RawMutex,
    /// The open file, -1 when closed.
    fd: AtomicI32,
    /// The offset of the next line.
    offset: AtomicU64,
}

/// The line at `offset` of `fd`, without its newline, in `line`: the bytes it spans with the
/// newline, 0 at the end of the file.
pub fn line_at<S: Sys>(sys: &S, fd: Fd, offset: u64, line: &mut TryVec<u8>) -> Result<usize> {
    line.clear();
    let mut chunk = [0u8; 512];
    loop {
        let at = offset.checked_add(u64::try_from(line.len()).map_err(|_| Errno::EOVERFLOW)?).ok_or(Errno::EOVERFLOW)?;
        let n = sys.pread(fd, &mut chunk, i64::try_from(at).map_err(|_| Errno::EOVERFLOW)?)?;
        let got = chunk.get(..n).unwrap_or(&[]);
        if let Some(i) = got.iter().position(|&c| c == b'\n') {
            line.extend_from_slice(got.get(..i).unwrap_or(&[]))?;
            return Ok(line.len().saturating_add(1));
        }
        line.extend_from_slice(got)?;
        if n == 0 {
            return Ok(line.len());
        }
    }
}

impl Cursor {
    /// Closed, at the start.
    pub const fn new() -> Cursor {
        Cursor { lock: RawMutex::new(), fd: AtomicI32::new(-1), offset: AtomicU64::new(0) }
    }

    /// Back to the first line (`setpwent`).
    pub fn rewind<S: Sys>(&self, sys: &S) {
        let _g = self.lock.lock(sys);
        self.offset.store(0, Relaxed);
    }

    /// Open `file` now if it is not open, and go back to its first line (`setttyent`, which
    /// reports whether the file can be read): the open error, if any.
    pub fn restart<S: Sys>(&self, sys: &S, file: &[u8]) -> Result<()> {
        let _g = self.lock.lock(sys);
        self.offset.store(0, Relaxed);
        self.open(sys, file).map(|_| ())
    }

    /// Close the file (`endpwent`); the next step starts again at the first line.
    pub fn end<S: Sys>(&self, sys: &S) {
        let _g = self.lock.lock(sys);
        let fd = self.fd.swap(-1, Relaxed);
        if fd >= 0 {
            let _ = sys.close(Fd::new(fd));
        }
        self.offset.store(0, Relaxed);
    }

    /// The next record of `file` (opened on first use): `f` reads each line until it takes one.
    /// `Ok(None)` at the end of the file; the file's open error when it cannot be opened.
    pub fn next<S: Sys, T>(&self, sys: &S, file: &[u8], mut f: impl FnMut(&[u8]) -> Step<T>) -> Result<Option<T>> {
        let _g = self.lock.lock(sys);
        let fd = self.open(sys, file)?;
        let mut line = TryVec::new();
        loop {
            let at = self.offset.load(Relaxed);
            let n = line_at(sys, fd, at, &mut line)?;
            if n == 0 {
                return Ok(None);
            }
            let past = at.saturating_add(u64::try_from(n).unwrap_or(u64::MAX));
            match f(&line) {
                Step::Skip => self.offset.store(past, Relaxed),
                Step::Take(t) => {
                    self.offset.store(past, Relaxed);
                    return Ok(Some(t));
                }
                Step::Stay(e) => return Err(e),
            }
        }
    }

    /// The open file (opened on first use; the lock is held).
    fn open<S: Sys>(&self, sys: &S, file: &[u8]) -> Result<Fd> {
        match self.fd.load(Relaxed) {
            n if n >= 0 => Ok(Fd::new(n)),
            _ => {
                let fd = sys.openat(Fd::CWD, KStr::from_nul_terminated(file)?, O_RDONLY | O_CLOEXEC, 0)?;
                // Kept across calls: never a standard descriptor (validation T12).
                let fd = liella::io::above_standard(sys, fd)?;
                self.fd.store(fd.raw(), Relaxed);
                Ok(fd)
            }
        }
    }

    /// For records that span lines (`aliases`): the rest of the file from the cursor's offset,
    /// given to `f`, which returns its step and the bytes the step consumed (the cursor moves
    /// past them for `Skip` and `Take`; `Skip` asks again). `Ok(None)` at the end.
    pub fn next_span<S: Sys, T>(&self, sys: &S, file: &[u8], mut f: impl FnMut(&[u8]) -> (Step<T>, usize)) -> Result<Option<T>> {
        let _g = self.lock.lock(sys);
        let fd = self.open(sys, file)?;
        let mut text = TryVec::new();
        let mut chunk = [0u8; 4096];
        let mut at = 0i64;
        loop {
            let n = sys.pread(fd, &mut chunk, at)?;
            if n == 0 {
                break;
            }
            text.extend_from_slice(chunk.get(..n).unwrap_or(&[]))?;
            at = at.saturating_add(i64::try_from(n).unwrap_or(i64::MAX));
        }
        loop {
            let off = usize::try_from(self.offset.load(Relaxed)).unwrap_or(usize::MAX);
            let rest = text.get(off..).unwrap_or(&[]);
            if rest.is_empty() {
                return Ok(None);
            }
            let (step, used) = f(rest);
            let past = u64::try_from(off.saturating_add(used.max(1))).unwrap_or(u64::MAX);
            match step {
                Step::Skip => self.offset.store(past, Relaxed),
                Step::Take(t) => {
                    self.offset.store(past, Relaxed);
                    return Ok(Some(t));
                }
                Step::Stay(e) => return Err(e),
            }
        }
    }
}

impl Default for Cursor {
    fn default() -> Cursor {
        Cursor::new()
    }
}

/// The `passwd` enumeration (`getpwent`).
pub static PW_CURSOR: Cursor = Cursor::new();
/// The `group` enumeration (`getgrent`).
pub static GR_CURSOR: Cursor = Cursor::new();
/// The `shadow` enumeration (`getspent`).
pub static SP_CURSOR: Cursor = Cursor::new();
/// The `gshadow` enumeration (`getsgent`).
pub static SG_CURSOR: Cursor = Cursor::new();
/// The `hosts` enumeration (`gethostent`).
pub static HOST_CURSOR: Cursor = Cursor::new();
/// The `networks` enumeration (`getnetent`).
pub static NET_CURSOR: Cursor = Cursor::new();
/// The `services` enumeration (`getservent`).
pub static SERV_CURSOR: Cursor = Cursor::new();
/// The `protocols` enumeration (`getprotoent`).
pub static PROTO_CURSOR: Cursor = Cursor::new();
/// The `rpc` enumeration (`getrpcent`).
pub static RPC_CURSOR: Cursor = Cursor::new();
/// The `aliases` enumeration (`getaliasent`).
pub static ALIAS_CURSOR: Cursor = Cursor::new();
/// The terminal table enumeration (`getttyent`, §19.5).
pub static TTY_CURSOR: Cursor = Cursor::new();
/// The `fstab` enumeration (`getfsent`, §13.13).
pub static FS_CURSOR: Cursor = Cursor::new();
