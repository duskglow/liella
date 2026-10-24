//! System logging as an explicit handle (spec §26; D-26.1).
//!
//! * **Records** ([`Record`]): one message with its priority, local time ([`Stamp`]), identifier
//!   and optional process ID. The socket form is `<PRI>Mmm dd hh:mm:ss IDENT[PID]: TEXT`, the
//!   local form of RFC 3164 (no host name; the month in English, the day padded with a blank);
//!   over a stream socket the text ends at its first NUL and the record is followed by a NUL,
//!   so the framing stays unambiguous. The console form is `IDENT[PID]: TEXT\r\n` and the
//!   standard-error form `IDENT[PID]: TEXT` plus a newline unless the text ends with one; both
//!   end the text at its first NUL. `[PID]` appears only when the record carries one
//!   (`LOG_PID`, syslog(3)).
//! * **The connection** ([`Link`]): a Unix datagram socket connected to the logger's path, or a
//!   stream socket when the path is a stream listener (`EPROTOTYPE`). Its descriptor is kept
//!   above 2, so it never takes the place of a closed standard descriptor. A send that fails is
//!   retried once on a new connection (§26.1); if that fails too, the connection is closed and
//!   the next message connects again.
//! * **Never blocking forever.** A receiver that does not read, or a console nobody reads, is
//!   waited for at most [`WAIT_NS`] per message (`MSG_DONTWAIT`/`O_NONBLOCK`, then `ppoll`):
//!   past that the message counts as not delivered. The descriptors themselves stay blocking.
//! * **[`emit`]** delivers a record as the options ask (the socket; the console with
//!   `LOG_CONS` when that fails; standard error with `LOG_PERROR`), for the handle here and for
//!   the C interface's process-wide logger (`liella_legacy::log_compat`).
//! * **[`Logger`]**, the clean handle: an owned identifier, explicit options, facility and mask,
//!   its own connection and channels. Priorities with bits outside the priority and facility
//!   fields are `EINVAL`.

use liella_kernel::errno::Errno;
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{
    PollFd, Timespec, AF_UNIX, CLOCK_MONOTONIC, MSG_DONTWAIT, MSG_NOSIGNAL, O_CLOEXEC, O_NOCTTY, O_NONBLOCK, O_WRONLY, POLLOUT, SOCK_CLOEXEC,
    SOCK_DGRAM, SOCK_STREAM,
};

use crate::error::Result;
use crate::mem::TryVec;

/// `LOG_PID`: the process ID in every record.
pub const LOG_PID: u32 = 0x01;
/// `LOG_CONS`: a record that cannot be delivered goes to the console.
pub const LOG_CONS: u32 = 0x02;
/// `LOG_ODELAY`: connect with the first message (the default).
pub const LOG_ODELAY: u32 = 0x04;
/// `LOG_NDELAY`: connect when the logger is opened.
pub const LOG_NDELAY: u32 = 0x08;
/// `LOG_NOWAIT`: no visible effect.
pub const LOG_NOWAIT: u32 = 0x10;
/// `LOG_PERROR`: a copy of every record on standard error.
pub const LOG_PERROR: u32 = 0x20;
/// The priority field of a priority value.
pub const PRIMASK: u32 = 0x07;
/// The facility field of a priority value.
pub const FACMASK: u32 = 0x3f8;
/// `LOG_KERN`.
pub const LOG_KERN: u32 = 0;
/// `LOG_USER`, the default facility.
pub const LOG_USER: u32 = 8;
/// `LOG_AUTH`.
pub const LOG_AUTH: u32 = 32;
/// `LOG_ERR`.
pub const LOG_ERR: u32 = 3;
/// `_PATH_LOG`, the system logger's socket (Config).
pub const PATH_LOG: &[u8] = b"/dev/log\0";
/// The console `LOG_CONS` writes to.
pub const PATH_CONSOLE: &[u8] = b"/dev/console\0";
/// How long one message may wait for a receiver or the console: one second.
pub const WAIT_NS: i64 = 1_000_000_000;

/// Whether a record of priority `pri` passes `mask` (bit `pri & PRIMASK`, `LOG_MASK`).
pub fn passes(mask: u32, pri: u32) -> bool {
    mask.checked_shr(pri & PRIMASK).is_some_and(|m| m & 1 != 0)
}

// ------------------------------------------------------------------------------ records

/// The local time of a record, as its header shows it.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Stamp {
    /// 0 (January) to 11.
    pub month: u8,
    /// 1 to 31.
    pub day: u8,
    /// 0 to 23.
    pub hour: u8,
    /// 0 to 59.
    pub min: u8,
    /// 0 to 60.
    pub sec: u8,
}

const MONTHS: &[u8; 36] = b"JanFebMarAprMayJunJulAugSepOctNovDec";

impl Stamp {
    /// `Mmm dd hh:mm:ss` (15 bytes; the day padded with a blank).
    pub fn text(&self) -> [u8; 15] {
        let m = usize::from(self.month % 12).saturating_mul(3);
        let name = MONTHS.get(m..m.saturating_add(3)).unwrap_or(b"Jan");
        let two = |v: u8, pad: u8| -> [u8; 2] {
            let v = v % 100;
            [if v < 10 { pad } else { b'0'.saturating_add(v / 10) }, b'0'.saturating_add(v % 10)]
        };
        let (d, h, mi, s) = (two(self.day, b' '), two(self.hour, b'0'), two(self.min, b'0'), two(self.sec, b'0'));
        let mut out = [b' '; 15];
        let parts: [&[u8]; 9] = [name, b" ", &d, b" ", &h, b":", &mi, b":", &s];
        for (o, b) in out.iter_mut().zip(parts.iter().flat_map(|p| p.iter())) {
            *o = *b;
        }
        out
    }
}

/// One message to the logger.
#[derive(Clone, Copy, Debug)]
pub struct Record<'a> {
    /// Facility and priority (the facility already chosen).
    pub pri: u32,
    /// The local time.
    pub stamp: Stamp,
    /// The identifier.
    pub ident: &'a [u8],
    /// The process ID, when the record shows it.
    pub pid: Option<i32>,
    /// The message.
    pub text: &'a [u8],
}

/// `v` in decimal, appended to `out`.
fn decimal(out: &mut TryVec<u8>, v: i64) -> Result<()> {
    let mut b = [0u8; 20];
    let mut n = v.unsigned_abs();
    let mut i = b.len();
    loop {
        i = i.saturating_sub(1);
        if let Some(d) = b.get_mut(i) {
            *d = b'0'.saturating_add(u8::try_from(n % 10).unwrap_or(0));
        }
        n /= 10;
        if n == 0 || i == 0 {
            break;
        }
    }
    if v < 0 {
        out.push(b'-')?;
    }
    out.extend_from_slice(b.get(i..).unwrap_or(&[]))
}

/// The text up to its first NUL.
fn to_nul(t: &[u8]) -> &[u8] {
    t.iter().position(|&b| b == 0).and_then(|n| t.get(..n)).unwrap_or(t)
}

impl Record<'_> {
    /// `IDENT[PID]: `.
    fn tag(&self, out: &mut TryVec<u8>) -> Result<()> {
        out.extend_from_slice(self.ident)?;
        if let Some(p) = self.pid {
            out.push(b'[')?;
            decimal(out, i64::from(p))?;
            out.push(b']')?;
        }
        out.extend_from_slice(b": ")
    }

    /// The socket form (see the module documentation).
    pub fn wire(&self, stream: bool) -> Result<TryVec<u8>> {
        let text = if stream { to_nul(self.text) } else { self.text };
        let mut out = TryVec::with_capacity(text.len().saturating_add(self.ident.len()).saturating_add(48))?;
        out.push(b'<')?;
        decimal(&mut out, i64::from(self.pri))?;
        out.push(b'>')?;
        out.extend_from_slice(&self.stamp.text())?;
        out.push(b' ')?;
        self.tag(&mut out)?;
        out.extend_from_slice(text)?;
        if stream {
            out.push(0)?;
        }
        Ok(out)
    }

    /// The console form: `IDENT[PID]: TEXT\r\n`, the text cut at its first NUL.
    pub fn console(&self) -> Result<TryVec<u8>> {
        let mut out = TryVec::new();
        self.tag(&mut out)?;
        out.extend_from_slice(to_nul(self.text))?;
        out.extend_from_slice(b"\r\n")?;
        Ok(out)
    }

    /// The standard-error form: `IDENT[PID]: TEXT`, the text cut at its first NUL, and a
    /// newline unless it ends with one.
    pub fn stderr(&self) -> Result<TryVec<u8>> {
        let mut out = TryVec::new();
        self.tag(&mut out)?;
        let text = to_nul(self.text);
        out.extend_from_slice(text)?;
        if text.last() != Some(&b'\n') {
            out.push(b'\n')?;
        }
        Ok(out)
    }
}

// ------------------------------------------------------------------------------ waiting

/// The monotonic time `ns` from now.
fn deadline<S: Sys>(sys: &S, ns: i64) -> Timespec {
    let now = sys.clock_gettime(CLOCK_MONOTONIC).unwrap_or_default();
    let total = now.nsec.saturating_add(ns % 1_000_000_000);
    let sec = now.sec.saturating_add(ns / 1_000_000_000).saturating_add(total / 1_000_000_000);
    Timespec { sec, nsec: total % 1_000_000_000 }
}

/// Wait until `fd` can take output, or `ETIMEDOUT` at `until` (monotonic).
fn wait_writable<S: Sys>(sys: &S, fd: Fd, until: Timespec) -> Result<()> {
    loop {
        let now = sys.clock_gettime(CLOCK_MONOTONIC)?;
        let mut left = Timespec { sec: until.sec.saturating_sub(now.sec), nsec: until.nsec.saturating_sub(now.nsec) };
        if left.nsec < 0 {
            left.nsec = left.nsec.saturating_add(1_000_000_000);
            left.sec = left.sec.saturating_sub(1);
        }
        if left.sec < 0 {
            return Err(Errno::ETIMEDOUT);
        }
        let mut p = [PollFd { fd: fd.raw(), events: POLLOUT, revents: 0 }];
        match sys.ppoll(&mut p, Some(&mut left), None) {
            Ok(0) => return Err(Errno::ETIMEDOUT),
            Ok(_) => return Ok(()),
            Err(Errno::EINTR) => {}
            Err(e) => return Err(e),
        }
    }
}

/// All of `bytes` to the socket `fd` without blocking, waiting for room until `until`.
fn send_all<S: Sys>(sys: &S, fd: Fd, bytes: &[u8], until: Timespec) -> Result<()> {
    let mut done = 0usize;
    loop {
        match sys.sendto(fd, bytes.get(done..).unwrap_or(&[]), MSG_NOSIGNAL | MSG_DONTWAIT, None) {
            Ok(n) => {
                done = done.saturating_add(n);
                if done >= bytes.len() {
                    return Ok(());
                }
            }
            Err(Errno::EAGAIN) => wait_writable(sys, fd, until)?,
            Err(Errno::EINTR) => {}
            Err(e) => return Err(e),
        }
    }
}

/// All of `bytes` to the descriptor `fd`; with `until`, a descriptor that would block is waited
/// for until then at most.
fn write_all<S: Sys>(sys: &S, fd: Fd, bytes: &[u8], until: Option<Timespec>) -> Result<()> {
    let mut done = 0usize;
    while done < bytes.len() {
        match sys.write(fd, bytes.get(done..).unwrap_or(&[])) {
            Ok(0) => return Err(Errno::EIO),
            Ok(n) => done = done.saturating_add(n),
            Err(Errno::EINTR) => {}
            Err(Errno::EAGAIN) => match until {
                Some(t) => wait_writable(sys, fd, t)?,
                None => return Err(Errno::EAGAIN),
            },
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

// ------------------------------------------------------------------------------ the connection

/// The connection to the logger: a descriptor (or none) and whether it is a stream.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Link {
    fd: i32,
    stream: bool,
}

impl Link {
    /// No connection.
    pub const CLOSED: Link = Link { fd: -1, stream: false };

    /// A connection kept as its parts (`fd` -1: none).
    pub const fn from_raw(fd: i32, stream: bool) -> Link {
        Link { fd, stream }
    }

    /// The parts: the descriptor (-1: none) and whether it is a stream.
    pub const fn raw(self) -> (i32, bool) {
        (self.fd, self.stream)
    }

    /// Whether there is a connection.
    pub const fn is_open(self) -> bool {
        self.fd >= 0
    }

    /// Connect to the socket at `path`: a datagram socket, or a stream socket when the path is a
    /// stream listener. On failure there is no connection.
    pub fn connect<S: Sys>(&mut self, sys: &S, path: KStr<'_>) -> Result<()> {
        self.close(sys);
        let addr = sockaddr(path)?;
        let mut last = Errno::ENOENT;
        for (kind, stream) in [(SOCK_DGRAM, false), (SOCK_STREAM, true)] {
            let fd = crate::io::above_standard(sys, sys.socket(AF_UNIX, kind | SOCK_CLOEXEC, 0)?)?;
            match sys.connect(fd, addr.as_slice()) {
                Ok(()) => {
                    *self = Link { fd: fd.raw(), stream };
                    return Ok(());
                }
                Err(e) => {
                    let _ = sys.close(fd);
                    last = e;
                    if e != Errno::EPROTOTYPE {
                        break;
                    }
                }
            }
        }
        Err(last)
    }

    /// Close the connection (nothing if there is none).
    pub fn close<S: Sys>(&mut self, sys: &S) {
        if self.is_open() {
            let _ = sys.close(Fd::new(self.fd));
        }
        *self = Link::CLOSED;
    }

    /// One attempt on the current connection.
    fn attempt<S: Sys>(&self, sys: &S, rec: &Record<'_>, until: Timespec) -> Result<()> {
        let wire = rec.wire(self.stream)?;
        send_all(sys, Fd::new(self.fd), wire.as_slice(), until)
    }

    /// Deliver `rec` to the socket at `path`, connecting first when there is no connection,
    /// waiting for a busy receiver at most `wait_ns`. A failed send is retried once on a new
    /// connection; after a second failure the connection is closed. A receiver that stays busy
    /// is `ETIMEDOUT` (a stream connection is then closed, its framing being unknown).
    pub fn deliver<S: Sys>(&mut self, sys: &S, path: KStr<'_>, rec: &Record<'_>, wait_ns: i64) -> Result<()> {
        let until = deadline(sys, wait_ns);
        if !self.is_open() {
            self.connect(sys, path)?;
        }
        match self.attempt(sys, rec, until) {
            Ok(()) => return Ok(()),
            Err(Errno::ETIMEDOUT) => {
                if self.stream {
                    self.close(sys);
                }
                return Err(Errno::ETIMEDOUT);
            }
            Err(Errno::ENOMEM) => return Err(Errno::ENOMEM),
            Err(_) => {}
        }
        self.connect(sys, path)?;
        let r = self.attempt(sys, rec, until);
        if r.is_err() {
            self.close(sys);
        }
        r
    }
}

/// `struct sockaddr_un` for `path`: the family, then the path with its NUL.
fn sockaddr(path: KStr<'_>) -> Result<TryVec<u8>> {
    let p = path.with_nul();
    if p.len() > 108 {
        return Err(Errno::ENAMETOOLONG);
    }
    let mut a = TryVec::with_capacity(p.len().saturating_add(2))?;
    a.extend_from_slice(&u16::try_from(AF_UNIX).unwrap_or(1).to_ne_bytes())?;
    a.extend_from_slice(p)?;
    Ok(a)
}

/// Write `bytes` to the console at `path`: opened for this write only, without blocking past
/// `wait_ns`.
pub fn to_console<S: Sys>(sys: &S, path: KStr<'_>, bytes: &[u8], wait_ns: i64) -> Result<()> {
    let until = deadline(sys, wait_ns);
    let fd = sys.openat(Fd::CWD, path, O_WRONLY | O_NOCTTY | O_CLOEXEC | O_NONBLOCK, 0)?;
    let r = write_all(sys, fd, bytes, Some(until));
    let _ = sys.close(fd);
    r
}

// ------------------------------------------------------------------------------ delivery

/// Where records go besides the connection.
#[derive(Clone, Copy, Debug)]
pub struct Channels<'a> {
    /// The logger's socket.
    pub socket: KStr<'a>,
    /// The console (`LOG_CONS`).
    pub console: KStr<'a>,
    /// Standard error (`LOG_PERROR`).
    pub stderr: Fd,
    /// How long one record may wait for a busy receiver or console, in nanoseconds.
    pub wait_ns: i64,
}

impl Channels<'static> {
    /// The system's channels: `/dev/log`, `/dev/console`, descriptor 2, [`WAIT_NS`].
    pub fn system() -> Result<Channels<'static>> {
        Ok(Channels { socket: KStr::from_nul_terminated(PATH_LOG)?, console: KStr::from_nul_terminated(PATH_CONSOLE)?, stderr: Fd::new(2), wait_ns: WAIT_NS })
    }
}

/// What became of a record.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Delivery {
    /// The logger received it.
    Sent,
    /// It went to the console instead.
    Console,
    /// It reached neither (or was masked).
    Lost,
}

/// Deliver `rec` as `options` ask: to the logger through `link`; when that fails and
/// `LOG_CONS` is set, to the console; with `LOG_PERROR`, also to standard error (whatever
/// became of it).
pub fn emit<S: Sys>(sys: &S, link: &mut Link, ch: &Channels<'_>, options: u32, rec: &Record<'_>) -> Delivery {
    let mut out = match link.deliver(sys, ch.socket, rec, ch.wait_ns) {
        Ok(()) => Delivery::Sent,
        Err(_) => Delivery::Lost,
    };
    if out == Delivery::Lost && options & LOG_CONS != 0 && rec.console().and_then(|t| to_console(sys, ch.console, t.as_slice(), ch.wait_ns)).is_ok() {
        out = Delivery::Console;
    }
    if options & LOG_PERROR != 0 {
        if let Ok(t) = rec.stderr() {
            let _ = write_all(sys, ch.stderr, t.as_slice(), None);
        }
    }
    out
}

// ------------------------------------------------------------------------------ the handle

/// A logger: an owned identifier, options, a default facility, a mask, its connection and its
/// channels (by default the system's). The kernel is a value the handle does not keep (X5), so
/// dropping it does not close the connection: [`Logger::close`] does.
#[derive(Debug)]
pub struct Logger {
    ident: TryVec<u8>,
    options: u32,
    facility: u32,
    mask: u32,
    link: Link,
    socket: TryVec<u8>,
    console: TryVec<u8>,
    stderr: Fd,
    wait_ns: i64,
}

impl Logger {
    /// A logger named `ident` with `options` (`LOG_*`) and the default facility `facility`
    /// (bits outside the facility field are `EINVAL`), passing every priority, on the
    /// system's channels. With `LOG_NDELAY` it connects at once (a failure is not an error:
    /// the first record tries again).
    pub fn new<S: Sys>(sys: &S, ident: &[u8], options: u32, facility: u32) -> Result<Logger> {
        Self::with_channels(sys, ident, options, facility, &Channels::system()?)
    }

    /// [`Logger::new`] on other channels.
    pub fn with_channels<S: Sys>(sys: &S, ident: &[u8], options: u32, facility: u32, ch: &Channels<'_>) -> Result<Logger> {
        if facility & !FACMASK != 0 {
            return Err(Errno::EINVAL);
        }
        let mut l = Logger {
            ident: TryVec::from_slice(ident)?,
            options,
            facility,
            mask: 0xff,
            link: Link::CLOSED,
            socket: TryVec::from_slice(ch.socket.with_nul())?,
            console: TryVec::from_slice(ch.console.with_nul())?,
            stderr: ch.stderr,
            wait_ns: ch.wait_ns,
        };
        if options & LOG_NDELAY != 0 {
            let _ = l.link.connect(sys, ch.socket);
        }
        Ok(l)
    }

    /// Install `mask` (`LOG_MASK`/`LOG_UPTO` bits); the previous one.
    pub fn set_mask(&mut self, mask: u32) -> u32 {
        core::mem::replace(&mut self.mask, mask)
    }

    /// Whether a record of priority `pri` would be logged (a caller formats only records that
    /// pass).
    pub fn passes(&self, pri: u32) -> bool {
        passes(self.mask, pri)
    }

    /// Whether the logger is connected.
    pub fn is_connected(&self) -> bool {
        self.link.is_open()
    }

    /// Log `text` at `pri` (the default facility when its facility field is 0) with local time
    /// `stamp`. `EINVAL` for bits outside the priority and facility fields; otherwise what
    /// became of the record ([`Delivery`]; a masked record is `Lost`).
    pub fn log<S: Sys>(&mut self, sys: &S, pri: u32, stamp: Stamp, text: &[u8]) -> Result<Delivery> {
        if pri & !(PRIMASK | FACMASK) != 0 {
            return Err(Errno::EINVAL);
        }
        if !self.passes(pri) {
            return Ok(Delivery::Lost);
        }
        let pri = if pri & FACMASK == 0 { pri | self.facility } else { pri };
        let pid = (self.options & LOG_PID != 0).then(|| sys.getpid());
        let rec = Record { pri, stamp, ident: self.ident.as_slice(), pid, text };
        let ch = Channels {
            socket: KStr::from_nul_terminated(self.socket.as_slice())?,
            console: KStr::from_nul_terminated(self.console.as_slice())?,
            stderr: self.stderr,
            wait_ns: self.wait_ns,
        };
        Ok(emit(sys, &mut self.link, &ch, self.options, &rec))
    }

    /// Close the connection (the next record connects again).
    pub fn close<S: Sys>(&mut self, sys: &S) {
        self.link.close(sys);
    }
}
