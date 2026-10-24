//! Terminals, job control and pseudo-terminals (spec §19; D-19.1 to D-19.3).
//!
//! * **Attributes** (§19.1, D-19.1): [`Termios`] is the kernel's `struct termios2`, whose speeds
//!   are integers: [`Termios::set_output_speed`] takes any rate, and a rate without a speed code
//!   travels as `BOTHER` with the rate beside it. [`get`] and [`set`] use `TCGETS2`/`TCSETS2`.
//!   [`set`] follows the partial-application rule of the spec and POSIX: it succeeds when the
//!   terminal took at least one of the requested changes (or none was requested), and fails
//!   with `EINVAL` when it took none.
//! * **Line control, job control** (§19.1, §19.2): one `ioctl` each. An invalid queue or action
//!   is refused before the descriptor is looked at, as an invalid `tcsetattr` action is.
//! * **Pseudo-terminals** (§19.4, D-19.2): [`open_master`] checks its flags; the slave of a pair
//!   is opened from the master with `TIOCGPTPEER` ([`open_pair`]), never by name.
//! * **Terminal names** (§19.3, D-19.3): [`name`] reads the descriptor's `/proc/self/fd` link and
//!   accepts it only when that path is the same file (device, inode and device number) in this
//!   mount namespace; without `/proc`, or when the path is another file or none, it is `ENODEV`
//!   (OD-5).
//! * **`/etc/ttys`** (§19.5): [`TtyLine::parse`] reads one line of the BSD terminal table.

use liella_kernel::errno::Errno;
use liella_kernel::fd::Fd;
use liella_kernel::ops::{IoctlIntArg, IoctlIntIn, IoctlIntOut, TcSet2};
use liella_kernel::path::{KStr, PATH_MAX};
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{KTermios2, Winsize, O_NOCTTY, O_RDWR};

use crate::error::Result;

// ------------------------------------------------------------------------------ speeds (§19.1)

/// The number of control characters the kernel keeps (`NCCS` of the kernel structure).
pub const NCC: usize = 19;
/// The output speed code in `c_cflag` (⚑ KERN).
pub const CBAUD: u32 = 0o010_017;
/// The speed code meaning "the rate is in the speed field".
pub const BOTHER: u32 = 0o010_000;
/// How far the input speed code is shifted in `c_cflag`.
pub const IBSHIFT: u32 = 16;
/// The input speed code in `c_cflag` (0: the same as the output speed).
pub const CIBAUD: u32 = CBAUD << IBSHIFT;

/// The standard speed codes and their rates (⚑ KERN): `B0` to `B38400`, then the extended codes
/// `B57600` to `B4000000`.
const SPEEDS: [(u32, u32); 31] = [
    (0, 0),
    (1, 50),
    (2, 75),
    (3, 110),
    (4, 134),
    (5, 150),
    (6, 200),
    (7, 300),
    (8, 600),
    (9, 1200),
    (10, 1800),
    (11, 2400),
    (12, 4800),
    (13, 9600),
    (14, 19_200),
    (15, 38_400),
    (0o010_001, 57_600),
    (0o010_002, 115_200),
    (0o010_003, 230_400),
    (0o010_004, 460_800),
    (0o010_005, 500_000),
    (0o010_006, 576_000),
    (0o010_007, 921_600),
    (0o010_010, 1_000_000),
    (0o010_011, 1_152_000),
    (0o010_012, 1_500_000),
    (0o010_013, 2_000_000),
    (0o010_014, 2_500_000),
    (0o010_015, 3_000_000),
    (0o010_016, 3_500_000),
    (0o010_017, 4_000_000),
];

/// The rate of a standard speed code (`B9600` is 9600); `None` for `BOTHER` and non-codes.
pub fn code_rate(code: u32) -> Option<u32> {
    SPEEDS.iter().find(|&&(c, _)| c == code).map(|&(_, r)| r)
}

/// The standard speed code of a rate (9600 is `B9600`); `None` for any other rate.
pub fn rate_code(rate: u32) -> Option<u32> {
    SPEEDS.iter().find(|&&(_, r)| r == rate).map(|&(c, _)| c)
}

/// A terminal's settings (`struct termios2`, ⚑ KERN): the four mode words (`cflag` with its
/// speed codes), the line discipline, the control characters and the input and output rates.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Termios {
    /// Input modes.
    pub iflag: u32,
    /// Output modes.
    pub oflag: u32,
    /// Control modes, with the speed codes ([`CBAUD`], [`CIBAUD`]).
    pub cflag: u32,
    /// Local modes.
    pub lflag: u32,
    /// The line discipline.
    pub line: u8,
    /// The control characters.
    pub cc: [u8; NCC],
    /// The input rate (used when the input code is `BOTHER`).
    pub ispeed: u32,
    /// The output rate (used when the output code is `BOTHER`).
    pub ospeed: u32,
}

impl Termios {
    /// The kernel's form.
    pub fn to_kernel(&self) -> KTermios2 {
        KTermios2 { iflag: self.iflag, oflag: self.oflag, cflag: self.cflag, lflag: self.lflag, line: self.line, cc: self.cc, ispeed: self.ispeed, ospeed: self.ospeed }
    }

    /// From the kernel's form.
    pub fn from_kernel(k: &KTermios2) -> Termios {
        Termios { iflag: k.iflag, oflag: k.oflag, cflag: k.cflag, lflag: k.lflag, line: k.line, cc: k.cc, ispeed: k.ispeed, ospeed: k.ospeed }
    }

    /// The output rate.
    pub fn output_speed(&self) -> u32 {
        code_rate(self.cflag & CBAUD).unwrap_or(self.ospeed)
    }

    /// The input rate (the output rate when no input speed is set).
    pub fn input_speed(&self) -> u32 {
        match (self.cflag & CIBAUD) >> IBSHIFT {
            0 => self.output_speed(),
            code => code_rate(code).unwrap_or(self.ispeed),
        }
    }

    /// Set the output rate: its speed code when it has one, else `BOTHER` and the rate.
    pub fn set_output_speed(&mut self, rate: u32) {
        self.cflag = (self.cflag & !CBAUD) | rate_code(rate).unwrap_or(BOTHER);
        self.ospeed = rate;
    }

    /// Set the input rate; 0 makes it follow the output rate.
    pub fn set_input_speed(&mut self, rate: u32) {
        let code = if rate == 0 { 0 } else { rate_code(rate).unwrap_or(BOTHER) };
        self.cflag = (self.cflag & !CIBAUD) | (code << IBSHIFT);
        self.ispeed = rate;
    }

    /// The settings as the kernel will hold them: each rate filled in from its speed code, the
    /// input rate from the output when it follows it, and a `BOTHER` code without a rate given
    /// the terminal's `current` rate (a structure filled in by hand, which the kernel would
    /// otherwise take as rate 0).
    fn complete(&self, current: &Termios) -> Termios {
        let mut t = *self;
        t.ospeed = match t.cflag & CBAUD {
            BOTHER if t.ospeed == 0 => current.ospeed,
            BOTHER => t.ospeed,
            code => code_rate(code).unwrap_or(t.ospeed),
        };
        t.ispeed = match (t.cflag & CIBAUD) >> IBSHIFT {
            0 => t.ospeed,
            BOTHER if t.ispeed == 0 => current.ispeed,
            BOTHER => t.ispeed,
            code => code_rate(code).unwrap_or(t.ispeed),
        };
        t
    }
}

/// `cfmakeraw`'s input modes to clear: `IGNBRK BRKINT PARMRK ISTRIP INLCR IGNCR ICRNL IXON`.
pub const RAW_IFLAG_OFF: u32 = 0o1 | 0o2 | 0o10 | 0o40 | 0o100 | 0o200 | 0o400 | 0o2000;
/// `cfmakeraw`'s output modes to clear: `OPOST`.
pub const RAW_OFLAG_OFF: u32 = 0o1;
/// `cfmakeraw`'s control modes to clear (`CSIZE PARENB`) before it sets `CS8`.
pub const RAW_CFLAG_OFF: u32 = 0o60 | 0o400;
/// `CS8`.
pub const CS8: u32 = 0o60;
/// `cfmakeraw`'s local modes to clear: `ECHO ECHONL ICANON ISIG IEXTEN`.
pub const RAW_LFLAG_OFF: u32 = 0o10 | 0o100 | 0o2 | 0o1 | 0o100_000;
/// The index of `VTIME` in the control characters.
pub const VTIME: usize = 5;
/// The index of `VMIN` in the control characters.
pub const VMIN: usize = 6;

impl Termios {
    /// Raw mode (`cfmakeraw`): no input or output processing, 8-bit characters without parity,
    /// no echo, signals or line editing, and reads that return at each byte (`VMIN` 1, `VTIME`
    /// 0). Everything else is kept.
    pub fn make_raw(&mut self) {
        self.iflag &= !RAW_IFLAG_OFF;
        self.oflag &= !RAW_OFLAG_OFF;
        self.cflag = (self.cflag & !RAW_CFLAG_OFF) | CS8;
        self.lflag &= !RAW_LFLAG_OFF;
        if let Some(v) = self.cc.get_mut(VMIN) {
            *v = 1;
        }
        if let Some(v) = self.cc.get_mut(VTIME) {
            *v = 0;
        }
    }
}

// ------------------------------------------------------------------------------ attributes (§19.1)

/// When [`set`] applies the settings.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum When {
    /// At once (`TCSANOW`).
    Now,
    /// Once the output written so far has been transmitted (`TCSADRAIN`).
    Drain,
    /// As `Drain`, discarding input not yet read (`TCSAFLUSH`).
    Flush,
}

impl When {
    /// From the C action (`TCSANOW` 0, `TCSADRAIN` 1, `TCSAFLUSH` 2); any other is `EINVAL`.
    pub fn from_c(action: i32) -> Result<When> {
        match action {
            0 => Ok(When::Now),
            1 => Ok(When::Drain),
            2 => Ok(When::Flush),
            _ => Err(Errno::EINVAL),
        }
    }

    fn request(self) -> TcSet2 {
        match self {
            When::Now => TcSet2::Now,
            When::Drain => TcSet2::Drain,
            When::Flush => TcSet2::Flush,
        }
    }
}

/// The settings of the terminal open on `fd` (`ENOTTY` when it is not one).
pub fn get<S: Sys>(sys: &S, fd: Fd) -> Result<Termios> {
    sys.tcgets2(fd).map(|k| Termios::from_kernel(&k))
}

/// Whether `fd` is a terminal: `Ok` when it is, else `ENOTTY` (or `EBADF`).
pub fn is_terminal<S: Sys>(sys: &S, fd: Fd) -> Result<()> {
    sys.tcgets2(fd).map(|_| ())
}

/// Apply `t` to the terminal open on `fd` `when` asked. Succeeds when the terminal took at
/// least one requested change, or when nothing was to change; `EINVAL` when it took none of
/// them (§19.1 ⚑ HIST: callers re-read to see which were taken).
pub fn set<S: Sys>(sys: &S, fd: Fd, when: When, t: &Termios) -> Result<()> {
    let before = get(sys, fd)?;
    let want = t.complete(&before);
    sys.tcsets2(fd, when.request(), &want.to_kernel())?;
    if want == before {
        return Ok(());
    }
    match get(sys, fd) {
        Ok(after) if after == before => Err(Errno::EINVAL),
        _ => Ok(()),
    }
}

// ------------------------------------------------------------------------------ line control (§19.1)

/// Send a break: about a quarter of a second for `duration` 0 or less, else `duration / 100`
/// tenths of a second, rounded up.
pub fn send_break<S: Sys>(sys: &S, fd: Fd, duration: i32) -> Result<()> {
    if duration <= 0 {
        return sys.ioctl_int(fd, IoctlIntArg::Tcsbrk, 0).map(|_| ());
    }
    let tenths = i64::from(duration).saturating_add(99).checked_div(100).unwrap_or(1);
    sys.ioctl_int(fd, IoctlIntArg::Tcsbrkp, tenths).map(|_| ())
}

/// Wait until the output written so far has been transmitted.
pub fn drain<S: Sys>(sys: &S, fd: Fd) -> Result<()> {
    sys.ioctl_int(fd, IoctlIntArg::Tcsbrk, 1).map(|_| ())
}

/// Discard data not yet read (`queue` 0, `TCIFLUSH`), not yet transmitted (1, `TCOFLUSH`) or
/// both (2, `TCIOFLUSH`). Another queue is `EINVAL`, before the descriptor is looked at.
pub fn flush<S: Sys>(sys: &S, fd: Fd, queue: i32) -> Result<()> {
    if !(0..=2).contains(&queue) {
        return Err(Errno::EINVAL);
    }
    sys.ioctl_int(fd, IoctlIntArg::Tcflsh, i64::from(queue)).map(|_| ())
}

/// Suspend output (`action` 0, `TCOOFF`), resume it (1, `TCOON`), or transmit the STOP (2,
/// `TCIOFF`) or START character (3, `TCION`). Another action is `EINVAL`, before the
/// descriptor is looked at.
pub fn flow<S: Sys>(sys: &S, fd: Fd, action: i32) -> Result<()> {
    if !(0..=3).contains(&action) {
        return Err(Errno::EINVAL);
    }
    sys.ioctl_int(fd, IoctlIntArg::Tcxonc, i64::from(action)).map(|_| ())
}

// ------------------------------------------------------------------------------ job control (§19.2)

/// The session whose controlling terminal `fd` is.
pub fn session<S: Sys>(sys: &S, fd: Fd) -> Result<i32> {
    sys.ioctl_get_int(fd, IoctlIntOut::Tiocgsid)
}

/// The foreground process group of the terminal (the kernel's answer: 0 for a pty master
/// whose slave is no session's terminal).
pub fn foreground<S: Sys>(sys: &S, fd: Fd) -> Result<i32> {
    sys.ioctl_get_int(fd, IoctlIntOut::Tiocgpgrp)
}

/// Make `pgrp` the terminal's foreground group (a background caller gets `SIGTTOU` unless it
/// blocks or ignores it, ⚑ KERN).
pub fn set_foreground<S: Sys>(sys: &S, fd: Fd, pgrp: i32) -> Result<()> {
    sys.ioctl_set_int(fd, IoctlIntIn::Tiocspgrp, pgrp).map(|_| ())
}

/// Make `fd` (a terminal) the controlling terminal of a new session led by the caller, and
/// standard input, output and error; `fd` is closed unless it is one of those (§19.4
/// `login_tty`). A caller that already leads a session keeps it and takes the terminal for it.
/// When `fd` is not a terminal nothing changes.
pub fn login<S: Sys>(sys: &S, fd: Fd) -> Result<()> {
    is_terminal(sys, fd)?;
    // A session leader cannot start another session (EPERM); it takes the terminal below.
    let _ = sys.setsid();
    sys.ioctl_int(fd, IoctlIntArg::Tiocsctty, 0)?;
    for std in [Fd::STDIN, Fd::STDOUT, Fd::STDERR] {
        sys.dup2(fd, std)?;
    }
    if fd.raw() > 2 {
        let _ = sys.close(fd);
    }
    Ok(())
}

// ------------------------------------------------------------------------------ pseudo-terminals (§19.4)

/// The multiplexer every new pty master is opened from.
pub const PTMX: &[u8] = b"/dev/ptmx\0";

/// The `open` flags a pty master may be opened with: an access mode and the flags that shape
/// an open file description. `O_DIRECTORY`, `O_PATH` and `O_TMPFILE` (which would not open the
/// multiplexer as a terminal) and bits `open` does not define are `EINVAL` (POSIX
/// `posix_openpt`: "the value of oflag is not valid").
pub const MASTER_FLAGS: u32 = 0o3 // O_ACCMODE
    | 0o100 // O_CREAT
    | 0o200 // O_EXCL
    | 0o400 // O_NOCTTY
    | 0o1000 // O_TRUNC
    | 0o2000 // O_APPEND
    | 0o4000 // O_NONBLOCK
    | 0o10_000 // O_DSYNC
    | 0o20_000 // O_ASYNC
    | 0o40_000 // O_DIRECT
    | 0o100_000 // O_LARGEFILE
    | 0o400_000 // O_NOFOLLOW
    | 0o1_000_000 // O_NOATIME
    | 0o2_000_000 // O_CLOEXEC
    | 0o4_000_000; // __O_SYNC (O_SYNC with O_DSYNC)

/// Open a new pty master with `flags` (checked against [`MASTER_FLAGS`]).
pub fn open_master<S: Sys>(sys: &S, flags: u32) -> Result<Fd> {
    if flags & !MASTER_FLAGS != 0 {
        return Err(Errno::EINVAL);
    }
    sys.openat(Fd::CWD, KStr::from_nul_terminated(PTMX)?, flags, 0)
}

/// The number of the pty whose master is `fd` (`ENOTTY` for anything else).
pub fn pty_number<S: Sys>(sys: &S, fd: Fd) -> Result<u32> {
    sys.ioctl_get_int(fd, IoctlIntOut::Tiocgptn).and_then(|n| u32::try_from(n).map_err(|_| Errno::ENOTTY))
}

/// A master's errors as `grantpt` and `unlockpt` report them: not a master is `EINVAL`.
fn as_master(e: Errno) -> Errno {
    if e == Errno::ENOTTY { Errno::EINVAL } else { e }
}

/// `grantpt`'s check: `fd` is a pty master. The kernel's devpts already gave the slave its
/// owner and mode (§19.4).
pub fn check_master<S: Sys>(sys: &S, fd: Fd) -> Result<()> {
    pty_number(sys, fd).map(|_| ()).map_err(as_master)
}

/// Unlock the slave of the master `fd`, so that it can be opened.
pub fn unlock<S: Sys>(sys: &S, fd: Fd) -> Result<()> {
    sys.ioctl_set_int(fd, IoctlIntIn::Tiocsptlck, 0).map(|_| ()).map_err(as_master)
}

/// The longest slave path: `/dev/pts/` and a `u32`.
pub const SLAVE_NAME_MAX: usize = 9 + 10;

/// The path of slave `n` in `out`: its length.
pub fn slave_name(n: u32, out: &mut [u8; SLAVE_NAME_MAX]) -> usize {
    let prefix = b"/dev/pts/";
    let (head, tail) = out.split_at_mut(prefix.len());
    head.copy_from_slice(prefix);
    let mut digits = [0u8; 10];
    let d = decimal(n, &mut digits);
    if let Some(dst) = tail.get_mut(..d.len()) {
        dst.copy_from_slice(d);
    }
    prefix.len().saturating_add(d.len())
}

/// `n` in decimal, at the end of `out`.
fn decimal(mut n: u32, out: &mut [u8; 10]) -> &[u8] {
    let mut k = out.len();
    loop {
        k = k.saturating_sub(1);
        if let Some(c) = out.get_mut(k) {
            *c = b'0'.saturating_add(u8::try_from(n % 10).unwrap_or(0));
        }
        n /= 10;
        if n == 0 || k == 0 {
            break;
        }
    }
    out.get(k..).unwrap_or(&[])
}

/// A new pty pair: the master (`O_RDWR`) and its slave, opened from the master with
/// `TIOCGPTPEER` (D-19.2; never by name, so it works without `/proc` or a matching `/dev/pts`,
/// and the slave never becomes a controlling terminal). Neither is close-on-exec. `settings`
/// is applied to the slave as [`set`] would (`TCSAFLUSH`), as far as the slave takes it, and
/// `window` becomes its window size. `open` is the kernel the master is opened with (the
/// cancellation point form, for `openpty`).
pub fn open_pair<S: Sys, O: Sys>(sys: &S, open: &O, settings: Option<&Termios>, window: Option<&Winsize>) -> Result<(Fd, Fd)> {
    let master = open.openat(Fd::CWD, KStr::from_nul_terminated(PTMX)?, O_RDWR | O_NOCTTY, 0)?;
    let slave = unlock(sys, master)
        .and_then(|()| sys.ioctl_int(master, IoctlIntArg::Tiocgptpeer, i64::from(O_RDWR | O_NOCTTY)))
        .and_then(|s| i32::try_from(s).map_err(|_| Errno::EBADF));
    let slave = match slave {
        Ok(s) => Fd::new(s),
        Err(e) => {
            let _ = sys.close(master);
            return Err(e);
        }
    };
    if let Some(t) = settings {
        // The pair exists either way; the caller re-reads to see what the slave took.
        let _ = set(sys, slave, When::Flush, t);
    }
    if let Some(w) = window {
        let _ = sys.tiocswinsz(slave, w);
    }
    Ok((master, slave))
}

// ------------------------------------------------------------------------------ terminal names (§19.3)

/// The path of the terminal open on `fd`, in `out`: its length. `ENOTTY` (or `EBADF`) when
/// `fd` is not a terminal; `ENODEV` when the kernel's name for it (the `/proc/self/fd` link)
/// is not that same terminal in this mount namespace, or, without `/proc`, when no name the
/// device number suggests is that terminal ([`name_by_device`], D-19.3, OD-5).
pub fn name<S: Sys>(sys: &S, fd: Fd, out: &mut [u8; PATH_MAX]) -> Result<usize> {
    name_in(sys, fd, out).map_err(|e| if e == Errno::ENAMETOOLONG { Errno::ENODEV } else { e })
}

/// As [`name`], in a buffer of any size, so that a caller can try a small one first:
/// `ENAMETOOLONG` when the name and its terminator do not fit in `out` (which [`name`], with
/// `PATH_MAX` bytes, reports as `ENODEV`). The terminator is written after the name.
pub fn name_in<S: Sys>(sys: &S, fd: Fd, out: &mut [u8]) -> Result<usize> {
    is_terminal(sys, fd)?;
    let st = sys.fstat(fd)?;
    let mut link = *b"/proc/self/fd/\0\0\0\0\0\0\0\0\0\0\0";
    let mut digits = [0u8; 10];
    let d = decimal(u32::try_from(fd.raw()).map_err(|_| Errno::EBADF)?, &mut digits);
    let end = 14usize.saturating_add(d.len());
    if let Some(dst) = link.get_mut(14..end) {
        dst.copy_from_slice(d);
    }
    let link = KStr::from_nul_terminated(link.get(..=end).ok_or(Errno::ENODEV)?)?;
    let len = match sys.readlinkat(Fd::CWD, link, out) {
        Ok(len) => len,
        // No `/proc` (OD-5): the name the device number suggests, if it is this terminal.
        Err(Errno::ENOENT) => return name_by_device(sys, &st, out),
        Err(_) => return Err(Errno::ENODEV),
    };
    // The link is read into `out` and terminated there, so it is checked without a copy; one
    // that fills `out` may have been cut short.
    let nul = out.get_mut(len).ok_or(Errno::ENAMETOOLONG)?;
    *nul = 0;
    let target = out.get(..=len).ok_or(Errno::ENODEV)?;
    if target.first() != Some(&b'/') {
        return Err(Errno::ENODEV);
    }
    let path = KStr::from_nul_terminated(target).map_err(|_| Errno::ENODEV)?;
    match sys.fstatat(Fd::CWD, path, 0) {
        Ok(there) if there.dev == st.dev && there.ino == st.ino && there.rdev == st.rdev => Ok(len),
        _ => Err(Errno::ENODEV),
    }
}

/// Without `/proc` (OD-5): the name below `/dev` that the terminal's device number gives under
/// the kernel's device list (Documentation/admin-guide/devices.txt): majors 136-143 the Unix98
/// pseudo-terminal slaves `pts/N`, major 4 the virtual consoles `ttyN` (minors 0-63) and the
/// serial ports `ttySN` (64-255), major 5 `tty`, `console` and `ptmx`. The name is accepted only
/// when it is this very terminal (the same device, inode and device number as `st`, the
/// terminal's status), so a mount namespace with another `/dev` gives `ENODEV`, never a wrong
/// name. Written to `out` with its terminator: its length.
pub fn name_by_device<S: Sys>(sys: &S, st: &liella_kernel::uapi::Stat, out: &mut [u8]) -> Result<usize> {
    let (major, minor) = (crate::fs::major(st.rdev), crate::fs::minor(st.rdev));
    let mut digits = [0u8; 10];
    let (stem, number): (&[u8], Option<u32>) = match (major, minor) {
        (136..=143, m) => (b"/dev/pts/", major.checked_sub(136).and_then(|k| k.checked_mul(256)).and_then(|k| k.checked_add(m))),
        (4, 0..=63) => (b"/dev/tty", Some(minor)),
        (4, m) => (b"/dev/ttyS", m.checked_sub(64)),
        (5, 0) => (b"/dev/tty", None),
        (5, 1) => (b"/dev/console", None),
        (5, 2) => (b"/dev/ptmx", None),
        _ => return Err(Errno::ENODEV),
    };
    let tail: &[u8] = match number {
        Some(n) => decimal(n, &mut digits),
        None => &[],
    };
    let len = stem.len().saturating_add(tail.len());
    let mut path = [0u8; 32];
    path.get_mut(..stem.len()).ok_or(Errno::ENODEV)?.copy_from_slice(stem);
    path.get_mut(stem.len()..len).ok_or(Errno::ENODEV)?.copy_from_slice(tail);
    let there = sys.fstatat(Fd::CWD, KStr::from_nul_terminated(path.get(..=len).ok_or(Errno::ENODEV)?)?, 0).map_err(|_| Errno::ENODEV)?;
    if (there.dev, there.ino, there.rdev) != (st.dev, st.ino, st.rdev) {
        return Err(Errno::ENODEV);
    }
    out.get_mut(..len).ok_or(Errno::ENAMETOOLONG)?.copy_from_slice(path.get(..len).unwrap_or(&[]));
    *out.get_mut(len).ok_or(Errno::ENAMETOOLONG)? = 0;
    Ok(len)
}

// ------------------------------------------------------------------------------ /etc/ttys (§19.5)

/// `ty_status`: the terminal runs a getty (`on`).
pub const TTY_ON: i32 = 1;
/// `ty_status`: root may log in on it (`secure`).
pub const TTY_SECURE: i32 = 2;

/// A field of a `/etc/ttys` line, as written: in a quoted field, `\"` stands for `"`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Field<'a> {
    raw: &'a [u8],
    quoted: bool,
}

impl<'a> Field<'a> {
    /// The field's bytes.
    pub fn bytes(self) -> impl Iterator<Item = u8> + 'a {
        let quoted = self.quoted;
        let mut it = self.raw.iter().copied().peekable();
        core::iter::from_fn(move || {
            let c = it.next()?;
            if quoted && c == b'\\' && it.peek() == Some(&b'"') {
                return it.next();
            }
            Some(c)
        })
    }

    /// The number of bytes [`Field::bytes`] gives.
    pub fn len(self) -> usize {
        self.bytes().count()
    }

    /// Whether the field is empty.
    pub fn is_empty(self) -> bool {
        self.raw.is_empty()
    }

    /// Whether the field's bytes are `text`.
    fn is(self, text: &[u8]) -> bool {
        self.bytes().eq(text.iter().copied())
    }
}

/// One entry of `/etc/ttys`: the name, then the getty command and the terminal type, then
/// flags (`on`, `off`, `secure`, `window=VALUE`) until a comment. Fields are separated by
/// spaces and tabs; a field may be quoted (`"..."`, with `\"` for a quote; a quote left open
/// runs to the end of the line).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TtyLine<'a> {
    /// The terminal's name below `/dev`.
    pub name: Field<'a>,
    /// The command that serves it; `None` when the line ends after the name.
    pub getty: Option<Field<'a>>,
    /// The terminal type; `None` when the line, or a comment, ends before it.
    pub kind: Option<Field<'a>>,
    /// [`TTY_ON`] and [`TTY_SECURE`].
    pub status: i32,
    /// The `window=` value.
    pub window: Option<Field<'a>>,
    /// The comment: after a `#` (and the white space after it), or the rest of the line from
    /// the first flag word that is none of the flags.
    pub comment: Option<&'a [u8]>,
}

fn is_blank(c: u8) -> bool {
    c == b' ' || c == b'\t'
}

/// A cursor over one line.
struct Scan<'a> {
    rest: &'a [u8],
}

impl<'a> Scan<'a> {
    fn skip_blanks(&mut self) {
        let n = self.rest.iter().take_while(|&&c| is_blank(c)).count();
        self.rest = self.rest.get(n..).unwrap_or(&[]);
    }

    /// The comment after the `#` at the cursor: the rest of the line without the `#` and the
    /// white space after it.
    fn comment(&mut self) -> &'a [u8] {
        let after = self.rest.get(1..).unwrap_or(&[]);
        let n = after.iter().take_while(|&&c| is_blank(c)).count();
        self.rest = &[];
        after.get(n..).unwrap_or(&[])
    }

    /// The field at the cursor: a quoted string (to its closing quote, or the end of the line),
    /// or a word that ends at white space or, when `hash_ends`, at a `#` (left at the cursor).
    fn field(&mut self, hash_ends: bool) -> Field<'a> {
        if let Some(body) = self.rest.strip_prefix(b"\"") {
            let mut k = 0usize;
            let mut close = None;
            while let Some(&c) = body.get(k) {
                if c == b'\\' && body.get(k.saturating_add(1)) == Some(&b'"') {
                    k = k.saturating_add(2);
                } else if c == b'"' {
                    close = Some(k);
                    break;
                } else {
                    k = k.saturating_add(1);
                }
            }
            let (raw, next) = match close {
                Some(k) => (body.get(..k).unwrap_or(&[]), body.get(k.saturating_add(1)..).unwrap_or(&[])),
                None => (body, &[][..]),
            };
            self.rest = next;
            return Field { raw, quoted: true };
        }
        let n = self.rest.iter().take_while(|&&c| !is_blank(c) && !(hash_ends && c == b'#')).count();
        let raw = self.rest.get(..n).unwrap_or(&[]);
        self.rest = self.rest.get(n..).unwrap_or(&[]);
        Field { raw, quoted: false }
    }
}

impl<'a> TtyLine<'a> {
    /// The entry on `line` (without its newline); `None` for a blank line or a comment line.
    pub fn parse(line: &'a [u8]) -> Option<TtyLine<'a>> {
        let mut s = Scan { rest: line };
        s.skip_blanks();
        if matches!(s.rest.first(), None | Some(b'#')) {
            return None;
        }
        let mut e = TtyLine { name: s.field(true), getty: None, kind: None, status: 0, window: None, comment: None };
        // The getty command and the type: a `#` ends the field it is in (an empty one when it
        // starts it) and starts the comment.
        for second in [false, true] {
            s.skip_blanks();
            let field = match s.rest.first() {
                None => return Some(e),
                Some(b'#') => Field { raw: &[], quoted: false },
                Some(_) => s.field(true),
            };
            if second {
                e.kind = Some(field);
            } else {
                e.getty = Some(field);
            }
            if s.rest.first() == Some(&b'#') {
                e.comment = Some(s.comment());
                return Some(e);
            }
        }
        // The flags.
        loop {
            s.skip_blanks();
            let word = s.rest;
            match s.rest.first() {
                None => return Some(e),
                Some(b'#') => {
                    e.comment = Some(s.comment());
                    return Some(e);
                }
                Some(_) => {}
            }
            if let Some(v) = s.rest.strip_prefix(b"window=") {
                s.rest = v;
                e.window = Some(s.field(false));
                continue;
            }
            let w = s.field(false);
            if w.is(b"on") {
                e.status |= TTY_ON;
            } else if w.is(b"off") {
                e.status &= !TTY_ON;
            } else if w.is(b"secure") {
                e.status |= TTY_SECURE;
            } else {
                e.comment = Some(word);
                return Some(e);
            }
        }
    }
}
