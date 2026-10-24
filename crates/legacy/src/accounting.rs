//! Login accounting and the files around it (spec §17.4 `lckpwdf`, §17.5, §17.5a, §17.6; D-17.4,
//! D-17.5): the `utmp` cursor and its process-wide file name, `wtmp` appends, the BSD `login`,
//! `logout` and `logwtmp` helpers, the password-file lock, the login-shell list, the terminal
//! table and the session's login name. Every path is a parameter (NUL-terminated; the constants
//! name the system files), so the tests use fixtures and never the system's records.
//!
//! **Locks** (D-17.4, D-17.5). File locks are open-file-description locks (`F_OFD_SETLK`) taken
//! in a timed retry loop: no `SIGALRM` handler is installed and no other signal is disturbed.
//! The `utmp` and `wtmp` locks wait at most [`UTMP_LOCK_TIMEOUT_MS`], the password-file lock
//! [`PWD_LOCK_TIMEOUT_MS`]; a lock still held then is `EAGAIN`. They conflict with the classic
//! process-associated locks other programs take on the same files.
//!
//! **The utmp cursor** ([`UtmpDb`], one per process, locked). Records are read and searched
//! forward from the current position; a search does not reconsider the record just read
//! (`getutid` twice finds the next match, as observed). `pututline` searches from the current
//! position for the record `getutid` would find and replaces it, or appends; a record that is
//! not a process or time record is always appended. The file is opened read-write, or read-only
//! when writing is refused, and then `pututline` fails with the refusal's error. A missing file
//! is not created.

use core::sync::atomic::{AtomicBool, AtomicI32, AtomicU64, AtomicUsize, Ordering::Relaxed};

use liella::error::{Errno, Result};
use liella::mem::TryVec;
use liella::users::{self, Utmp, UTMP_SIZE};
use liella_kernel::fd::Fd;
use liella_kernel::ops::FcntlLock;
use liella_kernel::path::{KPathBuf, KStr, PATH_MAX};
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Flock, Timespec, CLOCK_MONOTONIC, CLOCK_REALTIME, F_RDLCK, F_UNLCK, F_WRLCK, O_CLOEXEC, O_CREAT, O_RDONLY, O_RDWR, O_WRONLY, SEEK_SET};
use liella_rt::sync::RawMutex;

use crate::nss::{self, Ctx, Paths};
use crate::nss_files::{self, Key};
use crate::result_slots::Buf;

/// The `utmp` file (`_PATH_UTMP`).
pub const UTMP: &[u8] = b"/var/run/utmp\0";
/// The `wtmp` file (`_PATH_WTMP`).
pub const WTMP: &[u8] = b"/var/log/wtmp\0";
/// The password-file lock (`lckpwdf`).
pub const PWD_LOCK: &[u8] = b"/etc/.pwd.lock\0";
/// The login-shell list.
pub const SHELLS: &[u8] = b"/etc/shells\0";
/// The terminal table (`ttyslot`).
pub const TTYS: &[u8] = b"/etc/ttys\0";
/// The audit login UID of the process (`getlogin`).
pub const LOGINUID: &[u8] = b"/proc/self/loginuid\0";
/// How long a `utmp` or `wtmp` lock is waited for (§17.6: 10 s).
pub const UTMP_LOCK_TIMEOUT_MS: u64 = 10_000;
/// How long `lckpwdf` waits for the lock (§17.4: 15 s).
pub const PWD_LOCK_TIMEOUT_MS: u64 = 15_000;

/// The record size as a file offset step.
const STEP: u64 = 384;

// ------------------------------------------------------------------------------ file locks

fn nanos(t: Timespec) -> u64 {
    u64::try_from(t.sec).unwrap_or(0).saturating_mul(1_000_000_000).saturating_add(u64::try_from(t.nsec).unwrap_or(0))
}

/// Take a whole-file lock of `kind` (`F_RDLCK`, `F_WRLCK`) on `fd`, retrying for up to
/// `timeout_ms` while another holder has a conflicting one: `EAGAIN` when it is still held.
pub fn lock_file<S: Sys>(sys: &S, fd: Fd, kind: i16, timeout_ms: u64) -> Result<()> {
    let start = nanos(sys.clock_gettime(CLOCK_MONOTONIC)?);
    let timeout = timeout_ms.saturating_mul(1_000_000);
    // The pause between attempts: 1 ms, doubling up to 100 ms.
    let mut pause: u64 = 1_000_000;
    loop {
        let mut fl = Flock::new(kind, i16::try_from(SEEK_SET).unwrap_or(0), 0, 0);
        match sys.fcntl_lock(fd, FcntlLock::OfdSetLk, &mut fl) {
            Ok(()) => return Ok(()),
            Err(e) if e == Errno::EAGAIN || e == Errno::EACCES => {}
            Err(e) => return Err(e),
        }
        let waited = nanos(sys.clock_gettime(CLOCK_MONOTONIC)?).saturating_sub(start);
        let left = timeout.saturating_sub(waited);
        if left == 0 {
            return Err(Errno::EAGAIN);
        }
        let ns = i64::try_from(pause.min(left)).unwrap_or(i64::MAX);
        // An interrupted pause only shortens the wait; the deadline decides.
        let _ = sys.clock_nanosleep(CLOCK_MONOTONIC, 0, &Timespec { sec: ns / 1_000_000_000, nsec: ns % 1_000_000_000 }, None);
        pause = pause.saturating_mul(2).min(100_000_000);
    }
}

/// Release the lock on `fd`.
pub fn unlock_file<S: Sys>(sys: &S, fd: Fd) {
    let mut fl = Flock::new(F_UNLCK, i16::try_from(SEEK_SET).unwrap_or(0), 0, 0);
    let _ = sys.fcntl_lock(fd, FcntlLock::OfdSetLk, &mut fl);
}

/// `f` with `fd` locked as `kind` for at most [`UTMP_LOCK_TIMEOUT_MS`].
fn locked<S: Sys, T>(sys: &S, fd: Fd, kind: i16, f: impl FnOnce() -> Result<T>) -> Result<T> {
    lock_file(sys, fd, kind, UTMP_LOCK_TIMEOUT_MS)?;
    let r = f();
    unlock_file(sys, fd);
    r
}

// ------------------------------------------------------------------------------ records

/// The record at `offset`, `None` past the last whole record.
fn record_at<S: Sys>(sys: &S, fd: Fd, offset: u64) -> Result<Option<Utmp>> {
    let mut b = [0u8; UTMP_SIZE];
    let mut got = 0usize;
    while got < UTMP_SIZE {
        let at = offset.saturating_add(u64::try_from(got).unwrap_or(0));
        let n = sys.pread(fd, b.get_mut(got..).unwrap_or(&mut []), i64::try_from(at).map_err(|_| Errno::EOVERFLOW)?)?;
        if n == 0 {
            return Ok(None);
        }
        got = got.saturating_add(n);
    }
    Ok(Some(Utmp::from_bytes(&b)))
}

/// Write `rec` at `offset` whole.
fn write_at<S: Sys>(sys: &S, fd: Fd, offset: u64, rec: &Utmp) -> Result<()> {
    let b = rec.to_bytes();
    let mut done = 0usize;
    while done < UTMP_SIZE {
        let at = offset.saturating_add(u64::try_from(done).unwrap_or(0));
        let n = sys.pwrite(fd, b.get(done..).unwrap_or(&[]), i64::try_from(at).map_err(|_| Errno::EOVERFLOW)?)?;
        if n == 0 {
            return Err(Errno::ENOSPC);
        }
        done = done.saturating_add(n);
    }
    Ok(())
}

/// The offset a new record goes to: the end of the last whole record.
fn end_of_records<S: Sys>(sys: &S, fd: Fd) -> Result<u64> {
    let size = u64::try_from(sys.fstat(fd)?.size).unwrap_or(0);
    Ok(size.saturating_sub(size % STEP))
}

/// Search forward from `offset` for a record `pred` accepts: its offset, or the end of the
/// records.
fn search<S: Sys>(sys: &S, fd: Fd, mut offset: u64, pred: impl Fn(&Utmp) -> bool) -> Result<(u64, Option<Utmp>)> {
    loop {
        match record_at(sys, fd, offset)? {
            None => return Ok((offset, None)),
            Some(u) if pred(&u) => return Ok((offset, Some(u))),
            Some(_) => offset = offset.saturating_add(STEP),
        }
    }
}

fn open_path<S: Sys>(sys: &S, path: &[u8], flags: u32) -> Result<Fd> {
    sys.openat(Fd::CWD, KStr::from_nul_terminated(path)?, flags | O_CLOEXEC, 0)
}

/// The current time as `ut_tv` holds it, for the records the library stamps itself (`logwtmp`,
/// `logout`): the 32-bit field holds unsigned seconds to 2106-02-07 06:28:15 UTC (D-17.5); after
/// that, or before 1970, the record carries no time, `(0, 0)`, never a wrapped one (F38).
pub fn now<S: Sys>(sys: &S) -> Result<(i64, i32)> {
    let t = sys.clock_gettime(CLOCK_REALTIME)?;
    Ok(stamp(t.sec, t.nsec))
}

/// The `ut_tv` of a reading of the realtime clock (`sec`, `nsec`): see [`now`].
pub fn stamp(sec: i64, nsec: i64) -> (i64, i32) {
    if (0..=users::UT_SEC_MAX).contains(&sec) { (sec, i32::try_from(nsec / 1000).unwrap_or(0)) } else { (0, 0) }
}

/// The process-wide `utmp` cursor and file name (`setutent`, `getutent`, `getutid`,
/// `getutline`, `pututline`, `endutent`, `utmpname`, and the `utmpx` names).
pub struct UtmpDb {
    lock: RawMutex,
    /// The open file, -1 when closed.
    fd: AtomicI32,
    /// The offset of the next record.
    offset: AtomicU64,
    /// Why the file is open read-only (0: it is writable).
    write_error: AtomicI32,
    /// Whether `utmpname` chose the name (else [`UTMP`]).
    named: AtomicBool,
    name_len: AtomicUsize,
    name: Buf<PATH_MAX>,
}

impl Default for UtmpDb {
    fn default() -> UtmpDb {
        UtmpDb::new()
    }
}

impl UtmpDb {
    /// Closed, on the system file.
    pub const fn new() -> UtmpDb {
        UtmpDb { lock: RawMutex::new(), fd: AtomicI32::new(-1), offset: AtomicU64::new(0), write_error: AtomicI32::new(0), named: AtomicBool::new(false), name_len: AtomicUsize::new(0), name: Buf::new() }
    }

    /// `utmpname(name)`: close the file and use `name` from now on (any text, the empty one
    /// included: opening it then fails). `ENAMETOOLONG` for a name no path can have.
    pub fn set_name<S: Sys>(&self, sys: &S, name: &[u8]) -> Result<()> {
        if name.len() >= PATH_MAX {
            return Err(Errno::ENAMETOOLONG);
        }
        let _g = self.lock.lock(sys);
        self.close_locked(sys);
        self.name.store(name);
        self.name_len.store(name.len(), Relaxed);
        self.named.store(true, Relaxed);
        Ok(())
    }

    fn path(&self) -> Result<KPathBuf> {
        if !self.named.load(Relaxed) {
            return KPathBuf::new(UTMP.strip_suffix(b"\0").unwrap_or(UTMP));
        }
        let mut b = [0u8; PATH_MAX];
        let n = self.name_len.load(Relaxed).min(PATH_MAX);
        let name = b.get_mut(..n).ok_or(Errno::EINVAL)?;
        self.name.load(name);
        KPathBuf::new(name)
    }

    fn close_locked<S: Sys>(&self, sys: &S) {
        let fd = self.fd.swap(-1, Relaxed);
        if fd >= 0 {
            let _ = sys.close(Fd::new(fd));
        }
        self.offset.store(0, Relaxed);
        self.write_error.store(0, Relaxed);
    }

    fn open_locked<S: Sys>(&self, sys: &S) -> Result<Fd> {
        let fd = self.fd.load(Relaxed);
        if fd >= 0 {
            return Ok(Fd::new(fd));
        }
        let path = self.path()?;
        let fd = match sys.openat(Fd::CWD, path.as_kstr(), O_RDWR | O_CLOEXEC, 0) {
            Ok(fd) => fd,
            Err(e) if e == Errno::EACCES || e == Errno::EROFS || e == Errno::EPERM => {
                let fd = sys.openat(Fd::CWD, path.as_kstr(), O_RDONLY | O_CLOEXEC, 0)?;
                self.write_error.store(e.get(), Relaxed);
                fd
            }
            Err(e) => return Err(e),
        };
        // Kept across calls: never a standard descriptor (validation T12).
        let fd = liella::io::above_standard(sys, fd)?;
        self.fd.store(fd.raw(), Relaxed);
        self.offset.store(0, Relaxed);
        Ok(fd)
    }

    /// `setutent`: open the file if needed and go back to its first record. An open failure
    /// is reported here, and again by the next operation.
    pub fn rewind<S: Sys>(&self, sys: &S) -> Result<()> {
        let _g = self.lock.lock(sys);
        self.open_locked(sys)?;
        self.offset.store(0, Relaxed);
        Ok(())
    }

    /// `endutent`: close the file.
    pub fn close<S: Sys>(&self, sys: &S) {
        let _g = self.lock.lock(sys);
        self.close_locked(sys);
    }

    /// `getutent`: the next record, `None` at the end.
    pub fn next<S: Sys>(&self, sys: &S) -> Result<Option<Utmp>> {
        self.find(sys, |_| true)
    }

    /// The next record `pred` accepts, searching forward; `None` at the end.
    pub fn find<S: Sys>(&self, sys: &S, pred: impl Fn(&Utmp) -> bool) -> Result<Option<Utmp>> {
        let _g = self.lock.lock(sys);
        let fd = self.open_locked(sys)?;
        let (at, u) = locked(sys, fd, F_RDLCK, || search(sys, fd, self.offset.load(Relaxed), pred))?;
        // Past the record found, or at the end of the records.
        self.offset.store(if u.is_some() { at.saturating_add(STEP) } else { at }, Relaxed);
        Ok(u)
    }

    /// `getutid(key)`: `EINVAL` for a key that is neither a time nor a process record,
    /// `ESRCH` when no record matches.
    pub fn find_id<S: Sys>(&self, sys: &S, key: &Utmp) -> Result<Utmp> {
        users::id_class(key.kind).ok_or(Errno::EINVAL)?;
        self.find(sys, |u| u.matches_id(key))?.ok_or(Errno::ESRCH)
    }

    /// `getutline(key)`: `ESRCH` when no login or user process is on `key`'s line.
    pub fn find_line<S: Sys>(&self, sys: &S, key: &Utmp) -> Result<Utmp> {
        self.find(sys, |u| u.matches_line(key))?.ok_or(Errno::ESRCH)
    }

    /// `pututline(rec)`: replace the record `getutid(rec)` finds from the current position, or
    /// append; the position is then after it.
    pub fn put<S: Sys>(&self, sys: &S, rec: &Utmp) -> Result<()> {
        let _g = self.lock.lock(sys);
        let fd = self.open_locked(sys)?;
        if let Some(e) = Errno::new(self.write_error.load(Relaxed)) {
            return Err(e);
        }
        let at = locked(sys, fd, F_WRLCK, || {
            let (at, found) = search(sys, fd, self.offset.load(Relaxed), |u| u.matches_id(rec))?;
            let at = if found.is_some() { at } else { end_of_records(sys, fd)? };
            write_at(sys, fd, at, rec)?;
            Ok(at)
        })?;
        self.offset.store(at.saturating_add(STEP), Relaxed);
        Ok(())
    }
}

/// The process's `utmp` cursor.
pub static UTMP_DB: UtmpDb = UtmpDb::new();

/// `updwtmp(file, rec)`: append `rec` to the existing file `file` (it is not created); a
/// partial write is undone.
pub fn append<S: Sys>(sys: &S, file: &[u8], rec: &Utmp) -> Result<()> {
    let fd = open_path(sys, file, O_WRONLY)?;
    let r = locked(sys, fd, F_WRLCK, || {
        let size = sys.fstat(fd)?.size;
        let r = write_at(sys, fd, u64::try_from(size).unwrap_or(0), rec);
        if r.is_err() {
            let _ = sys.ftruncate(fd, size);
        }
        r
    });
    let _ = sys.close(fd);
    r
}

/// Write `rec` to the `utmp` file `file` in place of the record with its `ut_id` (or append),
/// without disturbing the process's cursor (`login`).
pub fn put_in<S: Sys>(sys: &S, file: &[u8], rec: &Utmp) -> Result<()> {
    let fd = open_path(sys, file, O_RDWR)?;
    let r = locked(sys, fd, F_WRLCK, || {
        let (at, found) = search(sys, fd, 0, |u| u.matches_id(rec))?;
        write_at(sys, fd, if found.is_some() { at } else { end_of_records(sys, fd)? }, rec)
    });
    let _ = sys.close(fd);
    r
}

/// `logout(line)` on the `utmp` file `file`: the login or user process on `line` becomes a
/// dead process with no user or host, stamped `time`. Whether one was found and written.
pub fn logout_in<S: Sys>(sys: &S, file: &[u8], line: &[u8], time: (i64, i32)) -> Result<bool> {
    let fd = open_path(sys, file, O_RDWR)?;
    let mut key = Utmp::default();
    users::set_field(&mut key.line, line);
    let r = locked(sys, fd, F_WRLCK, || {
        let (at, found) = search(sys, fd, 0, |u| u.matches_line(&key))?;
        let Some(mut u) = found else { return Ok(false) };
        u.kind = users::DEAD_PROCESS;
        u.user = [0; 32];
        u.host = [0; 256];
        u.time = time;
        write_at(sys, fd, at, &u)?;
        Ok(true)
    });
    let _ = sys.close(fd);
    r
}

/// The record `logwtmp(line, name, host)` appends: a user process, or a dead one when `name`
/// is empty (a logout), of process `pid` at `time`.
pub fn wtmp_record(line: &[u8], name: &[u8], host: &[u8], pid: i32, time: (i64, i32)) -> Utmp {
    let mut u = Utmp { kind: if name.is_empty() { users::DEAD_PROCESS } else { users::USER_PROCESS }, pid, time, ..Utmp::default() };
    users::set_field(&mut u.line, line);
    users::set_field(&mut u.user, name);
    users::set_field(&mut u.host, host);
    u
}

// ------------------------------------------------------------------------------ lckpwdf

/// The descriptor holding the password-file lock, -1 when not held.
static PWD_LOCK_FD: AtomicI32 = AtomicI32::new(-1);
/// Serializes `lckpwdf` and `ulckpwdf`.
static PWD_LOCK_GUARD: RawMutex = RawMutex::new();

/// `lckpwdf()` on the lock file `file`: `Ok(true)` once locked, `Ok(false)` when this process
/// holds it already; the open error (privilege), or `EAGAIN` after `timeout_ms`.
pub fn lock_passwd<S: Sys>(sys: &S, file: &[u8], timeout_ms: u64) -> Result<bool> {
    let _g = PWD_LOCK_GUARD.lock(sys);
    if PWD_LOCK_FD.load(Relaxed) >= 0 {
        return Ok(false);
    }
    let fd = sys.openat(Fd::CWD, KStr::from_nul_terminated(file)?, O_WRONLY | O_CREAT | O_CLOEXEC, 0o600)?;
    // Kept until `ulckpwdf`: never a standard descriptor (validation T12).
    let fd = liella::io::above_standard(sys, fd)?;
    if let Err(e) = lock_file(sys, fd, F_WRLCK, timeout_ms) {
        let _ = sys.close(fd);
        return Err(e);
    }
    PWD_LOCK_FD.store(fd.raw(), Relaxed);
    Ok(true)
}

/// `ulckpwdf()`: whether a lock was held (and is now released).
pub fn unlock_passwd<S: Sys>(sys: &S) -> bool {
    let _g = PWD_LOCK_GUARD.lock(sys);
    let fd = PWD_LOCK_FD.swap(-1, Relaxed);
    if fd < 0 {
        return false;
    }
    unlock_file(sys, Fd::new(fd));
    let _ = sys.close(Fd::new(fd));
    true
}

// ------------------------------------------------------------------------------ shells (§17.5a)

/// The index of the next shell `getusershell` returns (process-wide).
static SHELL_NEXT: AtomicUsize = AtomicUsize::new(0);
/// Serializes the shell cursor.
static SHELL_LOCK: RawMutex = RawMutex::new();

/// `getusershell()` over `file`: the next shell given to `f` (and the cursor moved past it),
/// `None` at the end. A file that cannot be read lists `/bin/sh` and `/bin/csh`.
pub fn next_shell<S: Sys, T>(sys: &S, file: &[u8], f: impl FnOnce(&[u8]) -> T) -> Option<T> {
    let _g = SHELL_LOCK.lock(sys);
    let i = SHELL_NEXT.load(Relaxed);
    let text = liella::system::read_file(sys, file);
    let shell = match &text {
        Ok(t) => users::shells(t).nth(i),
        Err(_) => users::DEFAULT_SHELLS.get(i).copied(),
    }?;
    SHELL_NEXT.store(i.saturating_add(1), Relaxed);
    Some(f(shell))
}

/// `setusershell()` and `endusershell()`: back to the first shell.
pub fn rewind_shells<S: Sys>(sys: &S) {
    let _g = SHELL_LOCK.lock(sys);
    SHELL_NEXT.store(0, Relaxed);
}

// ------------------------------------------------------------------------------ terminals, login name (§17.5)

/// The terminal open on `fd`, as its path below `/dev` (`pts/0`), in `out`: its length; `None`
/// when `fd` is not a terminal, or when its name cannot be found (without `/proc`, a terminal
/// whose device number names no node in `/dev` that is it, OD-5).
pub fn tty_line<S: Sys>(sys: &S, fd: Fd, out: &mut [u8; 256]) -> Option<usize> {
    sys.tcgets(fd).ok()?;
    let mut path = *b"/proc/self/fd/0123456789";
    let mut digits = [0u8; 10];
    let mut n = u32::try_from(fd.raw()).ok()?;
    let mut k = digits.len();
    loop {
        k = k.checked_sub(1)?;
        *digits.get_mut(k)? = b'0'.saturating_add(u8::try_from(n % 10).ok()?);
        n /= 10;
        if n == 0 {
            break;
        }
    }
    let d = digits.get(k..)?;
    let end = 14usize.checked_add(d.len())?;
    path.get_mut(14..end)?.copy_from_slice(d);
    let path = KPathBuf::new(path.get(..end)?).ok()?;
    let mut link = [0u8; 300];
    let len = match sys.readlinkat(Fd::CWD, path.as_kstr(), &mut link) {
        Ok(len) => len,
        // No `/proc` (OD-5): the name the device number gives, if it is this terminal.
        Err(_) => liella::term::name_by_device(sys, &sys.fstat(fd).ok()?, &mut link).ok()?,
    };
    let target = link.get(..len)?;
    let name = target.strip_prefix(b"/dev/").unwrap_or(target);
    out.get_mut(..name.len())?.copy_from_slice(name);
    Some(name.len())
}

/// The session's terminal: the first of standard input, output and error that is one (as
/// `login` chooses it), below `/dev`.
pub fn session_tty<S: Sys>(sys: &S, out: &mut [u8; 256]) -> Option<usize> {
    [Fd::STDIN, Fd::STDOUT, Fd::STDERR].into_iter().find_map(|fd| tty_line(sys, fd, out))
}

/// `ttyslot()` over the terminal table `file`: the 1-based index of the entry for `line`, 0 when
/// there is none (or no table).
pub fn tty_slot<S: Sys>(sys: &S, file: &[u8], line: &[u8]) -> usize {
    liella::system::read_file(sys, file).ok().and_then(|t| users::tty_slot(&t, line)).unwrap_or(0)
}

/// The files `getlogin` reads.
#[derive(Clone, Copy, Debug)]
pub struct LoginFiles<'a> {
    /// The audit login UID ([`LOGINUID`]).
    pub loginuid: &'a [u8],
    /// The user database ([`nss_files::PASSWD`]), read through the switch (`passwd`).
    pub passwd: &'a [u8],
    /// The login records ([`UTMP`]).
    pub utmp: &'a [u8],
}

/// The system's files.
pub const SYSTEM_LOGIN: LoginFiles<'static> = LoginFiles { loginuid: LOGINUID, passwd: nss_files::PASSWD, utmp: UTMP };

/// The login name of the session (`getlogin`), given to `f`: the user of the audit login UID
/// when it is set and the user database knows it; otherwise the user of the login record of
/// the session's terminal `tty` (below `/dev`). `ENXIO` without a terminal, `ENOENT` when no
/// record names a user for it.
pub fn login_name<S: Sys, T>(sys: &S, files: LoginFiles<'_>, tty: Option<&[u8]>, f: impl FnOnce(&[u8]) -> T) -> Result<T> {
    let uid = liella::system::read_file(sys, files.loginuid).ok().and_then(|t| core::str::from_utf8(t.trim_ascii()).ok()?.parse::<u32>().ok());
    let mut name: TryVec<u8> = TryVec::new();
    if let Some(uid) = uid.filter(|&u| u != u32::MAX) {
        let paths = Paths { passwd: files.passwd, ..nss::SYSTEM };
        if let Ok(Some(n)) = nss::found(nss::passwd(&Ctx::with_paths(sys, &paths), Key::Id(uid), |p| TryVec::from_slice(p.name))) {
            name = n?;
        }
    }
    if name.is_empty() {
        let tty = tty.ok_or(Errno::ENXIO)?;
        let text = liella::system::read_file(sys, files.utmp).map_err(|_| Errno::ENOENT)?;
        let mut key = Utmp::default();
        users::set_field(&mut key.line, tty);
        let found = text.chunks_exact(UTMP_SIZE).filter_map(|c| c.try_into().ok()).map(Utmp::from_bytes).find(|u| u.matches_line(&key)).ok_or(Errno::ENOENT)?;
        name = TryVec::from_slice(users::field_text(&found.user))?;
        if name.is_empty() {
            return Err(Errno::ENOENT);
        }
    }
    Ok(f(&name))
}
