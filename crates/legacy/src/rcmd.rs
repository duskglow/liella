//! BSD remote execution (spec §15.8; D-15.4): `rresvport`, `rcmd`, `rexec`, the trust checks
//! of `ruserok`/`iruserok`/`__ivaliduser`, and `ruserpass`'s `.netrc`. Implemented per contract
//! and documented as insecure: the trust model is "a privileged source port and an address in
//! a file" (⚑ HIST).
//!
//! Written under the clean-room rules of implementation-plan.md §1.4 from the spec, the Linux
//! man pages (`rcmd(3)`, `rexec(3)`, `hosts.equiv(5)`, `netrc(5)`) and black-box probes of the
//! reference (protocol seen by a test server; trust files in private mount namespaces; `.netrc`
//! under a private `HOME`). The wire protocol: the client sends the port of its error channel as
//! decimal text (an empty string for none) and a NUL, then, for `rcmd`, the local user, the
//! remote user and the command (for `rexec`: user, password, command), each NUL-terminated; the
//! server answers one byte, 0 for success or 1 followed by a message line, which the client
//! copies to standard error.
//!
//! Host names, users and netgroups are looked up through the Name Service Switch
//! (`crate::netdb_compat`, `crate::nss`). A netgroup host field (`+@group`, `-@group`) matches
//! when a name of the remote address (its host entry's official name or an alias) is a host of
//! the group; a netgroup user field when the remote user is a user of the group (`innetgr`,
//! `hosts.equiv(5)`). Without the netgroup database nobody is a member.
//!
//! Choices where the reference's behaviour is an artifact (probes, phase 10): a server that
//! closes without answering is reported as `<prog>: <host>: short read` with a newline (the
//! reference omits it for `rcmd` and prints a stale `errno` text for `rexec`); `rexec` waits for
//! the error channel's connection or the server's end, whichever comes first (the reference
//! blocks for ever if the server closes first); a host that is not found is reported as
//! `rcmd: getaddrinfo: Name or service not known`.

use liella::error::{Errno, Result};
use liella::mem::TryVec;
use liella::net::{is_space, Text};
use liella_kernel::fd::Fd;
use liella_kernel::path::KPathBuf;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{PollFd, SockAddrBuf, Stat, AF_INET, AF_INET6, AF_UNSPEC, AT_SYMLINK_NOFOLLOW, O_CLOEXEC, O_NOFOLLOW, O_RDONLY, POLLIN, SOCK_STREAM, S_IFMT, S_IFREG};

use liella::names::{Family, Ip};

use crate::netdb_compat::{self as netdb, Resolved};
use crate::nss::{self, Ctx};
use crate::nss_files::Key;

/// A local user's identity and home directory.
struct User {
    uid: u32,
    gid: u32,
    dir: TryVec<u8>,
}

/// A failure the caller reports as -1: with an `errno` to set, or without one (the reference
/// leaves `errno` alone after an unknown host or a refusal by the server).
pub type Failed = Option<Errno>;

fn family(af: i32) -> core::result::Result<Family, Failed> {
    match u32::try_from(af) {
        Ok(AF_INET) => Ok(Family::V4),
        Ok(AF_INET6) => Ok(Family::V6),
        Ok(AF_UNSPEC) => Ok(Family::Any),
        _ => Err(Some(Errno::EAFNOSUPPORT)),
    }
}

/// A socket address for `ip` and `port` (network order bytes in `port`'s storage).
fn sockaddr(ip: Ip, port: u16) -> ([u8; 28], usize) {
    let mut s = [0u8; 28];
    let (fam, len) = match ip {
        Ip::V4(a) => {
            if let Some(d) = s.get_mut(4..8) {
                d.copy_from_slice(&a);
            }
            (AF_INET, 16)
        }
        Ip::V6(a) => {
            if let Some(d) = s.get_mut(8..24) {
                d.copy_from_slice(&a);
            }
            (AF_INET6, 28)
        }
    };
    if let Some(d) = s.get_mut(..2) {
        d.copy_from_slice(&u16::try_from(fam).unwrap_or(0).to_ne_bytes());
    }
    if let Some(d) = s.get_mut(2..4) {
        d.copy_from_slice(&port.to_ne_bytes());
    }
    (s, len)
}

fn af_of(ip: Ip) -> u32 {
    match ip {
        Ip::V4(_) => AF_INET,
        Ip::V6(_) => AF_INET6,
    }
}

/// The port of a socket address (host order).
fn port_of(a: &SockAddrBuf) -> Option<u16> {
    let b = a.bytes();
    Some(u16::from_be_bytes([*b.get(2)?, *b.get(3)?]))
}

/// Write a diagnostic to standard error (unbuffered, as `stderr` is).
fn diag<S: Sys>(sys: &S, parts: &[&[u8]]) {
    let mut line: Text<1200> = Text::new();
    for p in parts {
        line.push_all(p);
    }
    let _ = write_all(sys, Fd::STDERR, line.as_bytes());
}

fn describe(e: Errno) -> &'static [u8] {
    liella::diag::description(e.get()).map_or(b"Unknown error".as_slice(), str::as_bytes)
}

fn write_all<S: Sys>(sys: &S, fd: Fd, mut data: &[u8]) -> Result<()> {
    while !data.is_empty() {
        let n = sys.write(fd, data)?;
        if n == 0 {
            return Err(Errno::EIO);
        }
        data = data.get(n..).unwrap_or(&[]);
    }
    Ok(())
}

fn write_z<S: Sys>(sys: &S, fd: Fd, text: &[u8]) -> Result<()> {
    write_all(sys, fd, text)?;
    write_all(sys, fd, &[0])
}

// ------------------------------------------------------------------------------ rresvport

/// The privileged ports `rresvport` searches.
const LOW: i32 = 512;
const HIGH: i32 = 1023;

/// `rresvport_af(&port, af)` (§15.8): a stream socket bound to a privileged port, searched
/// downwards from `*port` (taken into 512–1023) and wrapping at 512; `EAGAIN` when every port
/// is in use. `*port` is the port bound, or the one that failed.
pub fn rresvport<S: Sys>(sys: &S, port: &mut i32, af: i32) -> Result<Fd> {
    let fam = match u32::try_from(af) {
        Ok(f @ (AF_INET | AF_INET6)) => f,
        _ => return Err(Errno::EAFNOSUPPORT),
    };
    let s = sys.socket(fam, SOCK_STREAM, 0)?;
    let mut p = (*port).clamp(LOW, HIGH);
    let start = p;
    loop {
        *port = p;
        let ip = if fam == AF_INET { Ip::V4([0; 4]) } else { Ip::V6([0; 16]) };
        let (a, len) = sockaddr(ip, u16::try_from(p).unwrap_or(0).to_be());
        match sys.bind(s, a.get(..len).unwrap_or(&[])) {
            Ok(()) => return Ok(s),
            Err(e) if e == Errno::EADDRINUSE => {
                p = if p == LOW { HIGH } else { p.saturating_sub(1) };
                if p == start {
                    let _ = sys.close(s);
                    return Err(Errno::EAGAIN);
                }
            }
            Err(e) => {
                let _ = sys.close(s);
                return Err(e);
            }
        }
    }
}

// ------------------------------------------------------------------------------ rcmd/rexec

/// A connection `rcmd` or `rexec` made.
#[derive(Debug)]
pub struct Session {
    /// The command's standard input and output.
    pub fd: Fd,
    /// The error channel, when one was asked for.
    pub fd2: Option<Fd>,
    /// The host's canonical name (what `*ahost` becomes).
    pub canonical: TryVec<u8>,
}

fn close_all<S: Sys>(sys: &S, fds: &[Option<Fd>]) {
    for f in fds.iter().flatten() {
        let _ = sys.close(*f);
    }
}

/// Read the server's answer: `Ok` for a 0 byte; otherwise its message line goes to standard
/// error. `who` names the caller in diagnostics (`rcmd`, `rexec`).
fn answer<S: Sys>(sys: &S, fd: Fd, who: &[u8], host: &[u8]) -> core::result::Result<(), Failed> {
    let mut b = [0u8; 1];
    match sys.read(fd, &mut b) {
        Ok(1) if b == [0] => return Ok(()),
        Ok(1) => {}
        Ok(_) => {
            diag(sys, &[who, b": ", host, b": short read\n"]);
            return Err(None);
        }
        Err(e) => {
            diag(sys, &[who, b": ", host, b": ", describe(e), b"\n"]);
            return Err(Some(e));
        }
    }
    let mut c = [0u8; 1];
    while let Ok(1) = sys.read(fd, &mut c) {
        let _ = write_all(sys, Fd::STDERR, &c);
        if c == *b"\n" {
            break;
        }
    }
    Err(None)
}

/// The decimal text of a port.
fn port_text(p: u16) -> Text<8> {
    let mut t = Text::new();
    t.push_dec(u64::from(p));
    t
}

/// What `rcmd` and `rexec` are asked to do.
#[derive(Clone, Copy, Debug)]
pub struct Request<'a> {
    /// The host (`*ahost`).
    pub host: &'a [u8],
    /// The server's port, in network order.
    pub port: u16,
    /// The first and second names sent: for `rcmd` the local and remote users, for `rexec`
    /// the user and the password.
    pub names: [&'a [u8]; 2],
    /// The command.
    pub cmd: &'a [u8],
    /// Whether an error channel is wanted (`fd2p` not NULL).
    pub want_fd2: bool,
    /// The address family (`AF_INET`, `AF_INET6`, `AF_UNSPEC`).
    pub af: i32,
}

/// Wait until the error channel's listener `l` has a connection: `Ok`, or (the server answered
/// or closed first) a protocol failure.
fn await_channel<S: Sys>(sys: &S, s: Fd, l: Fd) -> core::result::Result<(), Failed> {
    let mut fds = [PollFd { fd: s.raw(), events: POLLIN, revents: 0 }, PollFd { fd: l.raw(), events: POLLIN, revents: 0 }];
    sys.ppoll(&mut fds, None, None).map_err(Some)?;
    if fds.get(1).is_none_or(|f| f.revents & POLLIN == 0) {
        diag(sys, &[b"poll: protocol failure in circuit setup\n"]);
        return Err(None);
    }
    Ok(())
}

/// `rcmd_af(&ahost, rport, locuser, remuser, cmd, fd2p, af)` (§15.8). Diagnostics go to
/// standard error, as the man page says. `found` receives the host's canonical name as soon as
/// it is known (the reference updates `*ahost` even when the server then refuses).
pub fn rcmd<S: Sys>(sys: &S, req: Request<'_>, found: impl FnOnce(&[u8])) -> core::result::Result<Session, Failed> {
    let Request { host, port: rport, names: [locuser, remuser], cmd, want_fd2, af } = req;
    let fam = family(af)?;
    let Some(h) = netdb::resolve(sys, host, fam).map_err(Some)? else {
        diag(sys, &[b"rcmd: getaddrinfo: Name or service not known\n"]);
        return Err(None);
    };
    found(&h.canonical);
    let (to, to_len) = sockaddr(h.addr, rport);
    let sock_af = i32::try_from(af_of(h.addr)).unwrap_or(0);
    let mut lport = HIGH;
    let s = loop {
        let s = match rresvport(sys, &mut lport, sock_af) {
            Ok(s) => s,
            Err(e) => {
                diag(sys, &[b"rcmd: socket: ", describe(e), b"\n"]);
                return Err(Some(e));
            }
        };
        match sys.connect(s, to.get(..to_len).unwrap_or(&[])) {
            Ok(()) => break s,
            Err(e) if e == Errno::EADDRINUSE && lport > LOW => {
                let _ = sys.close(s);
                lport = lport.saturating_sub(1);
            }
            Err(e) => {
                let _ = sys.close(s);
                diag(sys, &[h.canonical.as_slice(), b": ", describe(e), b"\n"]);
                return Err(Some(e));
            }
        }
    };
    let mut fd2 = None;
    let setup = (|| -> core::result::Result<(), Failed> {
        if !want_fd2 {
            return write_z(sys, s, b"").map_err(Some);
        }
        let mut p2 = lport.saturating_sub(1);
        let l = rresvport(sys, &mut p2, sock_af).map_err(|e| {
            diag(sys, &[b"rcmd: socket: ", describe(e), b"\n"]);
            Some(e)
        })?;
        let r = (|| -> core::result::Result<Fd, Failed> {
            sys.listen(l, 1).map_err(Some)?;
            write_z(sys, s, port_text(u16::try_from(p2).unwrap_or(0)).as_bytes()).map_err(Some)?;
            await_channel(sys, s, l)?;
            let mut peer = SockAddrBuf::new();
            let c = sys.accept4(l, Some(&mut peer), 0).map_err(Some)?;
            if !port_of(&peer).is_some_and(|p| (512..=1023).contains(&p)) {
                let _ = sys.close(c);
                diag(sys, &[b"socket: protocol failure in circuit setup\n"]);
                return Err(None);
            }
            Ok(c)
        })();
        let _ = sys.close(l);
        fd2 = Some(r?);
        Ok(())
    })();
    let done = setup.and_then(|()| {
        for part in [locuser, remuser, cmd] {
            write_z(sys, s, part).map_err(Some)?;
        }
        answer(sys, s, b"rcmd", &h.canonical)
    });
    match done {
        Ok(()) => Ok(Session { fd: s, fd2, canonical: h.canonical }),
        Err(e) => {
            close_all(sys, &[Some(s), fd2]);
            Err(e)
        }
    }
}

/// `rexec_af(&ahost, rport, user, passwd, cmd, fd2p, af)` (§15.8), with the user and password
/// already known. An unknown host fails without a diagnostic (as the reference). `found` as
/// for [`rcmd`].
pub fn rexec<S: Sys>(sys: &S, req: Request<'_>, found: impl FnOnce(&[u8])) -> core::result::Result<Session, Failed> {
    let Request { host, port: rport, names: [user, pass], cmd, want_fd2, af } = req;
    let fam = family(af).map_err(|_| None)?;
    let Some(h) = netdb::resolve(sys, host, fam).map_err(Some)? else { return Err(None) };
    found(&h.canonical);
    let (to, to_len) = sockaddr(h.addr, rport);
    let s = sys.socket(af_of(h.addr), SOCK_STREAM, 0).map_err(Some)?;
    if let Err(e) = sys.connect(s, to.get(..to_len).unwrap_or(&[])) {
        let _ = sys.close(s);
        diag(sys, &[h.canonical.as_slice(), b": ", describe(e), b"\n"]);
        return Err(Some(e));
    }
    let mut fd2 = None;
    let done = (|| -> core::result::Result<(), Failed> {
        if want_fd2 {
            let l = sys.socket(af_of(h.addr), SOCK_STREAM, 0).map_err(Some)?;
            let r = (|| -> core::result::Result<Fd, Failed> {
                sys.listen(l, 1).map_err(Some)?;
                let mut me = SockAddrBuf::new();
                sys.getsockname(l, &mut me).map_err(Some)?;
                write_z(sys, s, port_text(port_of(&me).unwrap_or(0)).as_bytes()).map_err(Some)?;
                await_channel(sys, s, l)?;
                sys.accept4(l, None, 0).map_err(Some)
            })();
            let _ = sys.close(l);
            fd2 = Some(r?);
        } else {
            write_z(sys, s, b"").map_err(Some)?;
        }
        for part in [user, pass, cmd] {
            write_z(sys, s, part).map_err(Some)?;
        }
        answer(sys, s, b"rexec", h.canonical.as_slice())
    })();
    match done {
        Ok(()) => Ok(Session { fd: s, fd2, canonical: h.canonical }),
        Err(e) => {
            close_all(sys, &[Some(s), fd2]);
            Err(e)
        }
    }
}

// ------------------------------------------------------------------------------ trust files

/// The messages `__rcmd_errstr` points to, with their terminators (each observed from the
/// reference in a private mount namespace, phase 10).
pub mod errstr {
    /// The file's status could not be read (it does not exist).
    pub const LSTAT: &[u8] = b"lstat failed\0";
    /// A symbolic link, directory or other non-regular file.
    pub const NOT_REGULAR: &[u8] = b"not regular file\0";
    /// The file could not be opened, or (never seen in practice) its open descriptor's status
    /// could not be read.
    pub const OPEN: &[u8] = b"cannot open\0";
    /// Owned by someone the file may not belong to (`hosts.equiv`: anyone but the superuser;
    /// `.rhosts`: anyone but the superuser and the user).
    pub const OWNER: &[u8] = b"bad owner\0";
    /// Writable by the group or others.
    pub const WRITABLE: &[u8] = b"writeable by other than owner\0";
    /// More than one link.
    pub const LINKED: &[u8] = b"hard linked somewhere\0";
}

/// The text of a trust file that passes the checks of `rcmd(3)`: a regular file (not a
/// symbolic link) owned by the superuser or (`.rhosts`) `uid`, writable by its owner only, with
/// one link. The checks run in that order (as observed); a failed one records its message in
/// `why`.
fn trusted_file<S: Sys>(sys: &S, path: &[u8], uid: Option<u32>, why: &mut Option<&'static [u8]>) -> Option<TryVec<u8>> {
    let p = KPathBuf::new(path).ok()?;
    let fail = |why: &mut Option<&'static [u8]>, m: &'static [u8]| {
        *why = Some(m);
        None
    };
    let Ok(st) = sys.fstatat(Fd::CWD, p.as_kstr(), AT_SYMLINK_NOFOLLOW) else { return fail(why, errstr::LSTAT) };
    if st.mode & S_IFMT != S_IFREG {
        return fail(why, errstr::NOT_REGULAR);
    }
    let Ok(fd) = sys.openat(Fd::CWD, p.as_kstr(), O_RDONLY | O_CLOEXEC | O_NOFOLLOW, 0) else { return fail(why, errstr::OPEN) };
    let r = (|| {
        let st: Stat = match sys.fstat(fd) {
            Ok(s) => s,
            Err(_) => return fail(why, errstr::OPEN),
        };
        if st.uid != 0 && Some(st.uid) != uid {
            return fail(why, errstr::OWNER);
        }
        if st.mode & 0o022 != 0 {
            return fail(why, errstr::WRITABLE);
        }
        if st.nlink > 1 {
            return fail(why, errstr::LINKED);
        }
        let mut out = TryVec::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = sys.read(fd, &mut chunk).ok()?;
            if n == 0 {
                return Some(out);
            }
            out.extend_from_slice(chunk.get(..n)?).ok()?;
        }
    })();
    let _ = sys.close(fd);
    r
}

/// How a host field of a trust line applies to the remote host.
enum HostMatch {
    Yes,
    No,
    Deny,
}

/// Whether the host entry `name` (a name or a numeric address) is `raddr`.
fn host_is<S: Sys>(sys: &S, name: &[u8], raddr: Ip) -> bool {
    let fam = match raddr {
        Ip::V4(_) => Family::V4,
        Ip::V6(_) => Family::V6,
    };
    matches!(netdb::resolve(sys, name, fam), Ok(Some(Resolved { addr, .. })) if addr == raddr)
}

/// Whether a name of `raddr` (its host entry's official name or an alias) is a host of the
/// netgroup `group`.
fn host_in_group<S: Sys>(sys: &S, group: &[u8], raddr: Ip) -> bool {
    let ctx = Ctx::system(sys);
    let Ok(h) = netdb::host_by_addr(&ctx, raddr) else { return false };
    core::iter::once(h.name()).chain(h.aliases()).any(|n| nss::in_netgroup(&ctx, group, Some(n), None, None))
}

/// Whether `user` is a user of the netgroup `group`.
fn user_in_group<S: Sys>(sys: &S, group: &[u8], user: &[u8]) -> bool {
    nss::in_netgroup(&Ctx::system(sys), group, None, Some(user), None)
}

fn host_field<S: Sys>(sys: &S, f: &[u8], raddr: Ip) -> HostMatch {
    match f {
        b"+" => HostMatch::Yes,
        [b'+', b'@', g @ ..] => {
            if host_in_group(sys, g, raddr) {
                HostMatch::Yes
            } else {
                HostMatch::No
            }
        }
        [b'-', b'@', g @ ..] => {
            if host_in_group(sys, g, raddr) {
                HostMatch::Deny
            } else {
                HostMatch::No
            }
        }
        [b'-', rest @ ..] => {
            if rest == b"+" || host_is(sys, rest, raddr) {
                HostMatch::Deny
            } else {
                HostMatch::No
            }
        }
        _ if host_is(sys, f, raddr) => HostMatch::Yes,
        _ => HostMatch::No,
    }
}

/// `__ivaliduser` over a trust file's text (§15.8; `hosts.equiv(5)` lines): whether `ruser` on
/// `raddr` may act as `luser`. Each line is a host field at the start of the line and an
/// optional user field: `+` (any host or user), `-name` (deny), `+@group`/`-@group`
/// (netgroups; see the module documentation), or a name or address. A host field alone admits
/// a remote user of the local user's name. The first line that decides wins.
pub fn validate<S: Sys>(sys: &S, text: &[u8], raddr: Ip, luser: &[u8], ruser: &[u8]) -> bool {
    for line in text.split(|&c| c == b'\n') {
        let mut fields = line.split(|&c| is_space(c));
        let Some(host) = fields.next().filter(|h| !h.is_empty()) else { continue };
        let user = fields.find(|f| !f.is_empty());
        match host_field(sys, host, raddr) {
            HostMatch::No => continue,
            HostMatch::Deny => return false,
            HostMatch::Yes => {}
        }
        match user {
            None if ruser == luser => return true,
            None => {}
            Some(b"+") => return true,
            Some([b'+', b'@', g @ ..]) if user_in_group(sys, g, ruser) => return true,
            Some([b'+', b'@', ..]) => {}
            Some([b'-', b'@', g @ ..]) if user_in_group(sys, g, ruser) => return false,
            Some([b'-', b'@', ..]) => {}
            Some([b'-', rest @ ..]) if *rest == *ruser => return false,
            Some([b'-', ..]) => {}
            Some(u) if *u == *ruser => return true,
            Some(_) => {}
        }
    }
    false
}

/// Where the trust files are, and whether `.rhosts` is read.
#[derive(Clone, Copy, Debug)]
pub struct TrustFiles<'a> {
    /// The system-wide file: `/etc/hosts.equiv`.
    pub equiv: &'a [u8],
    /// The directory of the user's `.rhosts`; `None`: the user's home directory.
    pub home: Option<&'a [u8]>,
    /// `__check_rhosts_file`: whether `.rhosts` is read.
    pub check_rhosts: bool,
}

impl TrustFiles<'static> {
    /// The system's files: `/etc/hosts.equiv` and `~/.rhosts`.
    pub const fn system(check_rhosts: bool) -> TrustFiles<'static> {
        TrustFiles { equiv: b"/etc/hosts.equiv", home: None, check_rhosts }
    }
}

/// `iruserok_af(raddr, superuser, ruser, luser, af)` (§15.8): the system-wide file (owned by
/// the superuser) unless `superuser`, even for a local user the user database does not know,
/// then (when `check_rhosts`) the local user's `.rhosts` (owned by the superuser or the user),
/// read with the user's file-system identity (`setfsuid`/`setfsgid`, restored after) so that a
/// superuser server reads only what the user could. `why` receives the message of the last
/// failed file check (`__rcmd_errstr`; an earlier message stays when a later file admits).
pub fn iruserok<S: Sys>(sys: &S, files: TrustFiles<'_>, raddr: Ip, superuser: bool, ruser: &[u8], luser: &[u8], why: &mut Option<&'static [u8]>) -> bool {
    let user = nss::passwd(&Ctx::system(sys), Key::Name(luser), |p| -> Result<User> { Ok(User { uid: p.uid, gid: p.gid, dir: TryVec::from_slice(p.dir)? }) }).ok().and_then(|u| u.ok());
    if !superuser {
        if let Some(text) = trusted_file(sys, files.equiv, None, why) {
            if validate(sys, &text, raddr, luser, ruser) {
                return true;
            }
        }
    }
    if !files.check_rhosts {
        return false;
    }
    let Some(u) = user else { return false };
    let mut path: TryVec<u8> = TryVec::new();
    if path.extend_from_slice(files.home.unwrap_or(&u.dir)).and_then(|()| path.extend_from_slice(b"/.rhosts")).is_err() {
        return false;
    }
    let old_uid = sys.setfsuid(u.uid);
    let old_gid = sys.setfsgid(u.gid);
    let text = trusted_file(sys, &path, Some(u.uid), why);
    sys.setfsgid(old_gid);
    sys.setfsuid(old_uid);
    text.is_some_and(|t| validate(sys, &t, raddr, luser, ruser))
}

/// `ruserok_af(rhost, superuser, ruser, luser, af)` (§15.8): [`iruserok`] for the address of
/// `rhost` (the `hosts` database through the switch; an unknown host is refused without a file check).
#[allow(clippy::too_many_arguments)] // the C call's five, the files and the message cell
pub fn ruserok<S: Sys>(sys: &S, files: TrustFiles<'_>, rhost: &[u8], superuser: bool, ruser: &[u8], luser: &[u8], af: i32, why: &mut Option<&'static [u8]>) -> bool {
    let Ok(fam) = family(af) else { return false };
    match netdb::resolve(sys, rhost, fam) {
        Ok(Some(h)) => iruserok(sys, files, h.addr, superuser, ruser, luser, why),
        _ => false,
    }
}

/// An address handed to `iruserok_af` as bytes of family `af`.
pub fn ip_of(af: i32, bytes: &[u8]) -> Option<Ip> {
    match u32::try_from(af).ok()? {
        AF_INET => Some(Ip::V4(bytes.get(..4)?.try_into().ok()?)),
        AF_INET6 => Some(Ip::V6(bytes.get(..16)?.try_into().ok()?)),
        _ => None,
    }
}

// ------------------------------------------------------------------------------ .netrc

/// Why `ruserpass` failed.
#[derive(Debug)]
pub enum Refused {
    /// A password in a file others may read; what was learned before the refusal (the login
    /// name, as the reference also stores it).
    Exposed(Netrc),
    /// No memory for the results.
    Memory,
}

/// What `ruserpass` found.
#[derive(Debug, Default)]
pub struct Netrc {
    /// The login name to use (`None`: still unknown).
    pub name: Option<TryVec<u8>>,
    /// The password (`None`: still unknown).
    pub pass: Option<TryVec<u8>>,
}

/// The tokens of a `.netrc` text from offset `from`: words separated by white space (or
/// commas), `"quoted"` words with `\` escapes; each with the offset where it ends.
fn tokens(text: &[u8], from: usize) -> impl Iterator<Item = (TryVec<u8>, usize)> + '_ {
    let mut i = from;
    core::iter::from_fn(move || {
        while text.get(i).is_some_and(|&c| is_space(c) || c == b',') {
            i = i.saturating_add(1);
        }
        let mut w = TryVec::new();
        if text.get(i) == Some(&b'"') {
            i = i.saturating_add(1);
            while let Some(&c) = text.get(i) {
                i = i.saturating_add(1);
                match c {
                    b'"' => break,
                    b'\\' => {
                        let n = *text.get(i)?;
                        i = i.saturating_add(1);
                        w.push(n).ok()?;
                    }
                    _ => w.push(c).ok()?,
                }
            }
        } else {
            while let Some(&c) = text.get(i).filter(|&&c| !is_space(c) && c != b',') {
                i = i.saturating_add(1);
                w.push(c).ok()?;
            }
            if w.is_empty() {
                return None;
            }
        }
        Some((w, i))
    })
}

/// One entry of a `.netrc` file.
#[derive(Default)]
struct Entry {
    machine: Option<TryVec<u8>>,
    default: bool,
    login: Option<TryVec<u8>>,
    password: Option<TryVec<u8>>,
}

/// `ruserpass(host, &name, &pass)` over a `.netrc` text (`netrc(5)`): the first entry for
/// `host` (case-insensitive; `default` matches any host) whose login agrees with `name` gives
/// the login when `name` is unknown and the password when `pass` is. A password in the chosen
/// entry of a file the group or others may read is refused (-1), even when `pass` is given;
/// `diag` receives the messages (the caller prefixes the program name). Where the reference's
/// parser is an artifact (probes, phase 10), Liella follows `netrc(5)`: the value of `account`
/// is skipped (the reference reports it as an unknown keyword), a `macdef` body runs to the next
/// blank line (the reference reads the body's lines as entries), `password` may precede `login`
/// (the reference crashes), and a `default` entry before others does not stop the search for a
/// later `machine` entry of the requested login.
pub fn ruserpass(text: &[u8], mode: u32, host: &[u8], name: Option<&[u8]>, pass: Option<&[u8]>, mut diag: impl FnMut(&[&[u8]])) -> core::result::Result<Netrc, Refused> {
    let mut entries: TryVec<Entry> = TryVec::new();
    let mut toks = tokens(text, 0);
    while let Some((t, at)) = toks.next() {
        let value = |toks: &mut dyn Iterator<Item = (TryVec<u8>, usize)>| toks.next().map(|(v, _)| v);
        match t.as_slice() {
            b"machine" => {
                let e = Entry { machine: value(&mut toks), ..Entry::default() };
                entries.push(e).map_err(|_| Refused::Memory)?;
            }
            b"default" => entries.push(Entry { default: true, ..Entry::default() }).map_err(|_| Refused::Memory)?,
            b"login" | b"password" | b"account" => {
                let v = value(&mut toks);
                if let Some(e) = entries.as_mut_slice().last_mut() {
                    match t.as_slice() {
                        b"login" => e.login = v,
                        b"password" => e.password = v,
                        _ => {}
                    }
                }
            }
            b"macdef" => {
                // The macro runs to the next blank line.
                let rest = text.get(at..).unwrap_or(&[]);
                let skip = rest.windows(2).position(|w| w == b"\n\n").map_or(rest.len(), |p| p.saturating_add(2));
                toks = tokens(text, at.saturating_add(skip));
            }
            other => diag(&[b"Unknown .netrc keyword ", other, b"\n"]),
        }
    }
    let mut out = Netrc { name: name.map(TryVec::from_slice).transpose().map_err(|_| Refused::Memory)?, pass: pass.map(TryVec::from_slice).transpose().map_err(|_| Refused::Memory)? };
    let chosen = entries.iter().find(|e| {
        let host_ok = e.default || e.machine.as_ref().is_some_and(|m| m.eq_ignore_ascii_case(host));
        let login_ok = match (&out.name, &e.login) {
            (Some(n), Some(l)) => n.as_slice() == l.as_slice(),
            _ => true,
        };
        host_ok && login_ok
    });
    if let Some(e) = chosen {
        if out.name.is_none() {
            if let Some(l) = &e.login {
                out.name = Some(l.try_clone().map_err(|_| Refused::Memory)?);
            }
        }
        if let Some(p) = &e.password {
            if mode & 0o077 != 0 {
                diag(&[b"Error: .netrc file is readable by others.\n"]);
                diag(&[b"Remove 'password' line or make file unreadable by others.\n"]);
                return Err(Refused::Exposed(out));
            }
            if out.pass.is_none() {
                out.pass = Some(p.try_clone().map_err(|_| Refused::Memory)?);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn netrc_entries() {
        let mut said = 0;
        let r = ruserpass(b"machine h1 login u1 password p1", 0o600, b"H1", None, None, |_| said += 1).unwrap();
        assert_eq!((r.name.unwrap().as_slice(), r.pass.unwrap().as_slice()), (&b"u1"[..], &b"p1"[..]));
        let r = ruserpass(b"machine h1 login u1 password p1", 0o600, b"h1", Some(b"other"), None, |_| ()).unwrap();
        assert!(r.pass.is_none());
        let r = ruserpass(b"machine h1 login u1 password p1 machine h1 login u2 password p2", 0o600, b"h1", Some(b"u2"), None, |_| ()).unwrap();
        assert_eq!(r.pass.unwrap().as_slice(), b"p2");
        let r = ruserpass(b"machine h2 login u1 password p1\ndefault login du password dp", 0o600, b"h1", None, None, |_| ()).unwrap();
        assert_eq!(r.name.unwrap().as_slice(), b"du");
        assert!(ruserpass(b"machine h1 login u1 password p1", 0o644, b"h1", None, None, |_| ()).is_err());
        let r = ruserpass(b"machine h1 login \"u 1\" password p1 bogus", 0o600, b"h1", None, None, |_| said += 1).unwrap();
        assert_eq!(r.name.unwrap().as_slice(), b"u 1");
        assert_eq!(said, 1);
    }
}
