//! The C conventions of spec §19 (phase 11): terminal attributes in the C `struct termios`
//! (D-19.1, converted by `liella_abi::pack`), the library-owned names of `ttyname` and
//! `ptsname` and their `_r` forms, the pty pair functions `openpty` and `forkpty`, and the
//! `/etc/ttys` enumeration with its static entry. The terminal operations are `liella::term`;
//! the `B*` speed codes `liella_legacy::term_compat`. The exports in `posix::s19_terminals`
//! convert the C arguments.
//!
//! **Names.** `ttyname` and `ptsname` return one process-wide buffer each, as their contracts
//! name a buffer of the calling thread's (D-C0.2). The `_r` forms return the error number
//! and also store it in `errno` (as the reference does); a NULL buffer is `EINVAL`
//! (ptsname(3)), a zero-sized one `ERANGE` before the descriptor is looked at, and one that
//! cannot hold the name and its terminator `ERANGE` with the buffer untouched.
//!
//! **`/etc/ttys`** (§19.5). One process-wide enumeration (`liella_legacy::cursors::TTY_CURSOR`)
//! and one static entry, whose strings are in a block the library keeps (grown as needed). Lines
//! of any length are read, and so is a last line without a newline. `getttynam` searches from
//! the first entry and closes the file after it, so the next `getttyent` starts again at the
//! first entry.

use liella::error::Errno;
use liella::term::{self, Field, TtyLine};
use liella_abi::layout::{struct_termios, struct_winsize};
use liella_abi::pack;
use liella_kernel::errno::KResult;
use liella_kernel::fd::Fd;
use liella_kernel::path::PATH_MAX;
use liella_kernel::sys::Sys;
use liella_legacy::accounting::TTYS;
use liella_legacy::cursors::{Step, TTY_CURSOR};
use liella_rt::sync::RawMutex;

use crate::conv;
use crate::fork;
use crate::sys::{CSys, CP};

// ------------------------------------------------------------------------------ attributes (§19.1)

/// `tcgetattr(fd, t)`: the settings in the C structure.
pub fn tcgetattr(fd: i32) -> KResult<struct_termios> {
    term::get(&CSys, conv::fd(fd)).map(|t| pack::termios(&t))
}

/// `tcsetattr(fd, action, t)`: an invalid action is `EINVAL` before anything else.
pub fn tcsetattr(fd: i32, action: i32, t: Option<&struct_termios>) -> KResult<()> {
    let when = term::When::from_c(action)?;
    term::set(&CSys, conv::fd(fd), when, &pack::termios_in(t.ok_or(Errno::EFAULT)?))
}

// ------------------------------------------------------------------------------ names (§19.3, §19.4)

/// The terminal name of `fd` and its terminator, given to `then`. Terminal names are short, so
/// a small buffer is tried first; a name that does not fit is read again into `PATH_MAX` bytes
/// (the same answer, without clearing 4 KiB per call).
fn with_tty_name<T>(fd: i32, then: impl FnOnce(&[u8]) -> KResult<T>) -> KResult<T> {
    let mut short = [0u8; 64];
    match term::name_in(&CSys, conv::fd(fd), &mut short) {
        Err(Errno::ENAMETOOLONG) => {}
        r => return then(short.get(..=r?).ok_or(Errno::ERANGE)?),
    }
    let mut long = [0u8; PATH_MAX];
    let len = term::name(&CSys, conv::fd(fd), &mut long)?;
    then(long.get(..=len).ok_or(Errno::ERANGE)?)
}

/// `ttyname(fd)`: the name in the calling thread's buffer (D-C0.2).
pub fn ttyname(fd: i32) -> KResult<usize> {
    // The name and its terminator.
    with_tty_name(fd, |name| {
        let buf = crate::users::result_buf::<PATH_MAX>(crate::users::TTYNAME)?;
        buf.store(name);
        Ok(buf.addr())
    })
}

/// `ttyname_r(fd, buf, size)`: a NULL buffer is `EINVAL`, an empty one `ERANGE` (before the
/// descriptor is looked at); the name and its terminator, or `ERANGE` with the buffer
/// untouched.
pub fn ttyname_r(fd: i32, buf: KResult<Option<&mut [u8]>>) -> KResult<()> {
    let buf = buf?.ok_or(Errno::EINVAL)?;
    if buf.is_empty() {
        return Err(Errno::ERANGE);
    }
    with_tty_name(fd, |name| {
        let text = name.split_last().map_or(name, |(_, t)| t);
        liella::net::copy_z(text, buf).map(|_| ()).ok_or(Errno::ERANGE)
    })
}

/// The slave name of the master `fd`, in `out`: its length.
fn slave_name(fd: i32, out: &mut [u8]) -> KResult<usize> {
    let n = term::pty_number(&CSys, conv::fd(fd))?;
    let mut name = [0u8; term::SLAVE_NAME_MAX];
    let len = term::slave_name(n, &mut name);
    out.get_mut(..len).ok_or(Errno::ERANGE)?.copy_from_slice(name.get(..len).ok_or(Errno::ERANGE)?);
    Ok(len)
}

/// `ptsname(fd)`: the slave's name in the calling thread's buffer (D-C0.2).
pub fn ptsname(fd: i32) -> KResult<usize> {
    let mut name = [0u8; 20];
    let len = slave_name(fd, &mut name)?;
    let buf = crate::users::result_buf::<20>(crate::users::PTSNAME)?;
    buf.store(name.get(..=len).ok_or(Errno::ERANGE)?);
    Ok(buf.addr())
}

/// `ptsname_r(fd, buf, size)`: a NULL buffer is `EINVAL` (ptsname(3)), then the master's
/// errors, then `ERANGE`.
pub fn ptsname_r(fd: i32, buf: KResult<Option<&mut [u8]>>) -> KResult<()> {
    let buf = buf?.ok_or(Errno::EINVAL)?;
    let mut name = [0u8; 20];
    let len = slave_name(fd, &mut name)?;
    liella::net::copy_z(name.get(..len).ok_or(Errno::ERANGE)?, buf).map(|_| ()).ok_or(Errno::ERANGE)
}

// ------------------------------------------------------------------------------ pty pairs (§19.4)

/// A new pair for `openpty`/`forkpty`, the slave's name written at `name` when it is not 0
/// (the C contract gives no size: the name and its terminator, at most 20 bytes).
fn pair(name: usize, termp: Option<&struct_termios>, winp: Option<&struct_winsize>) -> KResult<(Fd, Fd)> {
    let settings = termp.map(pack::termios_in);
    let window = winp.map(pack::winsize_in);
    let (m, s) = term::open_pair(&CSys, &CP, settings.as_ref(), window.as_ref())?;
    if name != 0 {
        let mut text = [0u8; 20];
        match slave_name(m.raw(), &mut text) {
            Ok(len) => conv::out_bytes(name, len.saturating_add(1)).copy_from_slice(text.get(..=len).unwrap_or(&[0])),
            Err(e) => {
                let _ = CSys.close(m);
                let _ = CSys.close(s);
                return Err(e);
            }
        }
    }
    Ok((m, s))
}

/// `openpty(&master, &slave, name, termp, winp)`: NULL `master` or `slave` is `EFAULT`, and
/// nothing is opened.
pub fn openpty(amaster: Option<&mut i32>, aslave: Option<&mut i32>, name: usize, termp: Option<&struct_termios>, winp: Option<&struct_winsize>) -> KResult<()> {
    let (Some(am), Some(asl)) = (amaster, aslave) else { return Err(Errno::EFAULT) };
    let (m, s) = pair(name, termp, winp)?;
    *am = m.raw();
    *asl = s.raw();
    Ok(())
}

/// `forkpty(&master, name, termp, winp)`: `openpty`, then `fork`; the child closes the master
/// and makes the slave its terminal (`login_tty`; on failure it exits with status 1), the
/// parent closes the slave and receives the master. NULL `master` is `EFAULT`.
pub fn forkpty(amaster: Option<&mut i32>, name: usize, termp: Option<&struct_termios>, winp: Option<&struct_winsize>) -> KResult<i32> {
    let am = amaster.ok_or(Errno::EFAULT)?;
    let (m, s) = pair(name, termp, winp)?;
    match fork::fork() {
        Ok(0) => {
            let _ = CSys.close(m);
            if term::login(&CSys, s).is_err() {
                CSys.exit_group(1);
            }
            Ok(0)
        }
        Ok(pid) => {
            let _ = CSys.close(s);
            *am = m.raw();
            Ok(pid)
        }
        Err(e) => {
            let _ = CSys.close(m);
            let _ = CSys.close(s);
            Err(e)
        }
    }
}

// ------------------------------------------------------------------------------ /etc/ttys (§19.5)

/// Serializes the enumeration (its cursor is process-wide).
static TTY_LOCK: RawMutex = RawMutex::new();

/// The bytes of `struct ttyent` (six words).
const TTYENT_SIZE: usize = 48;

/// Fill the calling thread's entry with `e` (D-C0.2: the `struct ttyent` and its strings in the
/// thread's block, which the next call overwrites): its address.
fn store_entry(e: &TtyLine<'_>) -> KResult<usize> {
    let comment = e.comment.map(|c| c.iter().copied());
    let need = [Some(e.name), e.getty, e.kind, e.window].iter().flatten().map(|f| f.len().saturating_add(1)).sum::<usize>().saturating_add(comment.clone().map_or(0, |c| c.count().saturating_add(1)));
    let (entry, all) = crate::users::block_bytes(crate::users::TTYENT, need.saturating_add(TTYENT_SIZE))?;
    let (head, block) = all.split_at_mut(TTYENT_SIZE);
    let addr = entry.saturating_add(TTYENT_SIZE);
    let mut used = 0usize;
    let mut put = |bytes: &mut dyn Iterator<Item = u8>| -> usize {
        let at = used;
        for b in bytes.chain(core::iter::once(0)) {
            if let Some(slot) = block.get_mut(used) {
                *slot = b;
            }
            used = used.saturating_add(1);
        }
        addr.saturating_add(at)
    };
    let mut field = |f: Option<Field<'_>>| f.map_or(0, |f| put(&mut f.bytes()));
    let words = [field(Some(e.name)), field(e.getty), field(e.kind), 0, field(e.window)];
    let comment = e.comment.map_or(0, |c| put(&mut c.iter().copied()));
    let status = usize::try_from(u32::from_ne_bytes(e.status.to_ne_bytes())).unwrap_or(0);
    let words = [words[0], words[1], words[2], status, words[4], comment];
    for (d, w) in head.chunks_exact_mut(8).zip(words) {
        d.copy_from_slice(&w.to_ne_bytes());
    }
    Ok(entry)
}

/// `setttyent()`: open the table (if it is not open) at its first entry.
pub fn setttyent() -> KResult<()> {
    let _g = TTY_LOCK.lock(&CSys);
    TTY_CURSOR.restart(&CSys, TTYS)
}

/// `endttyent()`: close the table; always 1.
pub fn endttyent() -> i32 {
    let _g = TTY_LOCK.lock(&CSys);
    TTY_CURSOR.end(&CSys);
    1
}

/// The next entry in the static entry: its address, 0 at the end.
fn next_entry() -> KResult<usize> {
    let r = TTY_CURSOR.next(&CSys, TTYS, |line| match TtyLine::parse(line) {
        None => Step::Skip,
        Some(e) => match store_entry(&e) {
            Ok(a) => Step::Take(a),
            Err(err) => Step::Stay(err),
        },
    });
    r.map(|a| a.unwrap_or(0))
}

/// `getttyent()`: the next entry, 0 (with `errno` unchanged) at the end.
pub fn getttyent() -> KResult<usize> {
    let _g = TTY_LOCK.lock(&CSys);
    next_entry()
}

/// `getttynam(name)`: the first entry named `name`, 0 when there is none; the table is closed
/// afterwards.
pub fn getttynam(name: KResult<&[u8]>) -> KResult<usize> {
    let name = name?;
    let _g = TTY_LOCK.lock(&CSys);
    let found = TTY_CURSOR.restart(&CSys, TTYS).and_then(|()| {
        let r = TTY_CURSOR.next(&CSys, TTYS, |line| match TtyLine::parse(line) {
            Some(e) if e.name.bytes().eq(name.iter().copied()) => match store_entry(&e) {
                Ok(a) => Step::Take(a),
                Err(err) => Step::Stay(err),
            },
            _ => Step::Skip,
        });
        r.map(|a| a.unwrap_or(0))
    });
    TTY_CURSOR.end(&CSys);
    found
}
