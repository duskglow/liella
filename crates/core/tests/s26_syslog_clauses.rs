// Specified from spec §26 (phase 13).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §26 (12 clauses): the clean handle `liella::log::Logger` on channels in
//! a scratch directory, and the C interface's process-wide logger through the exports, sent to
//! the same kind of channels (`selftest::syslog_paths`, accepted only in tests): no test writes
//! to the system log. The process-wide logger is shared by the tests of this file, which take
//! [`LOCK`] and reset it. The variadic entry points (`syslog`, `__syslog_chk`) are adapters
//! over `liella_vsyslog4`, exercised end to end by `syslog_products.rs`.
//!
//! The PID appears only with `LOG_PID` (syslog(3) and the reference; §26.1's "prefixed with ...
//! and PID" is reported as a spec error).
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, missing_docs)]

#[path = "support/scratch.rs"]
mod scratch;
#[path = "support/timeout.rs"]
mod timeout;

use core::ptr::null_mut;
use std::ffi::CString;
use std::io::Read;
use std::os::unix::net::{UnixDatagram, UnixListener};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use liella::error::Errno;
use liella::log::{Channels, Delivery, Logger, Stamp, LOG_CONS, LOG_NDELAY, LOG_NOWAIT, LOG_ODELAY, LOG_PERROR, LOG_PID};
use liella_boundary::errno;
use liella_boundary::posix::s26_syslog as x;
use liella_boundary::sys::CSys;
use liella_kernel::fd::Fd;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;

const LOG_ERR: i32 = 3;
const LOG_WARNING: i32 = 4;
const LOG_INFO: i32 = 6;
const LOG_USER: i32 = 8;
const LOG_LOCAL1: i32 = 17 << 3;
const EDOM: i32 = 33;
const EACCES: i32 = 13;

/// The process-wide logger and its test channels belong to one test at a time.
static LOCK: Mutex<()> = Mutex::new(());

/// A fresh scratch directory with a short path (the sockets in it must fit `sun_path`, wherever
/// the tree lives), removed when dropped.
struct Scratch(scratch::Scratch);

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let s = Scratch(scratch::Scratch::sock_dir(&format!("s26-{tag}")));
        std::fs::write(s.path("console"), b"").unwrap();
        s
    }
    fn path(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

/// `p` with a NUL.
fn z(p: &Path) -> Vec<u8> {
    let mut v = p.as_os_str().as_encoded_bytes().to_vec();
    v.push(0);
    v
}

/// A datagram receiver at `p`.
fn receiver(p: &Path) -> UnixDatagram {
    let _ = std::fs::remove_file(p);
    UnixDatagram::bind(p).unwrap()
}

/// The next datagram (`None` when nothing comes: within 3 s with `wait`, else 300 ms).
fn next(r: &UnixDatagram, wait: bool) -> Option<Vec<u8>> {
    r.set_read_timeout(Some(if wait { Duration::from_secs(3) } else { Duration::from_millis(300) })).unwrap();
    let mut b = vec![0u8; 1 << 20];
    r.recv(&mut b).ok().map(|n| b[..n].to_vec())
}

/// A record split into its priority, its timestamp text and the rest.
fn split(rec: &[u8]) -> (u32, String, String) {
    let s = String::from_utf8_lossy(rec).into_owned();
    assert!(s.starts_with('<'), "{s:?}");
    let close = s.find('>').unwrap();
    let pri = s[1..close].parse().unwrap();
    let stamp = s[close + 1..close + 16].to_string();
    assert_eq!(&s[close + 16..close + 17], " ", "{s:?}");
    (pri, stamp, s[close + 17..].to_string())
}

/// The local time of instant `t` as a header shows it.
fn stamp_at(t: i64) -> String {
    let tm = liella_boundary::zone::localtime_r(t).unwrap();
    let s = Stamp { month: tm.tm_mon as u8, day: tm.tm_mday as u8, hour: tm.tm_hour as u8, min: tm.tm_min as u8, sec: tm.tm_sec as u8 };
    String::from_utf8(s.text().to_vec()).unwrap()
}

fn now() -> i64 {
    CSys.clock_gettime(0).unwrap().sec
}

/// Channels for the handle.
fn channels<'a>(sock: &'a [u8], console: &'a [u8], stderr: Fd, wait_ns: i64) -> Channels<'a> {
    Channels { socket: KStr::from_nul_terminated(sock).unwrap(), console: KStr::from_nul_terminated(console).unwrap(), stderr, wait_ns }
}

const STAMP: Stamp = Stamp { month: 9, day: 4, hour: 5, min: 6, sec: 7 };

/// The process-wide logger as a test starts: closed, no options, `LOG_USER`, every priority,
/// its channels in `s` (socket `log`, console `console`), short waits.
fn global(s: &Scratch) {
    x::closelog();
    x::openlog(core::ptr::null(), 0, LOG_USER);
    x::setlogmask(0xff);
    let (sock, cons) = (s.path("log"), s.path("console"));
    liella_boundary::selftest::syslog_paths(Some((sock.as_os_str().as_encoded_bytes(), cons.as_os_str().as_encoded_bytes(), 50_000_000)));
}

fn done() {
    x::closelog();
    x::openlog(core::ptr::null(), 0, LOG_USER);
    x::setlogmask(0xff);
    liella_boundary::selftest::syslog_paths(None);
}

/// `vsyslog(pri, fmt)` for a format that takes no argument.
fn log(pri: i32, fmt: &str) {
    let f = CString::new(fmt).unwrap();
    x::vsyslog(pri, f.as_ptr(), null_mut());
}

fn pipe() -> (Fd, Fd) {
    let [r, w] = CSys.pipe2(0o2000000 | 0o4000).unwrap();
    (r, w)
}

fn drain(fd: Fd) -> Vec<u8> {
    let mut b = vec![0u8; 1 << 16];
    let n = CSys.read(fd, &mut b).unwrap_or(0);
    b[..n].to_vec()
}

/// How many sockets the process has open.
fn sockets() -> usize {
    std::fs::read_dir("/proc/self/fd").unwrap().filter_map(|e| std::fs::read_link(e.ok()?.path()).ok()).filter(|l| l.to_string_lossy().starts_with("socket:")).count()
}

fn prog() -> String {
    std::env::current_exe().unwrap().file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn s26_1_c_1() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let s = Scratch::new("c1");
    let r = receiver(&s.path("log"));
    global(&s);
    // The identifier, the options and the default facility, for the whole process.
    let mut ident = b"clause-c1\0".to_vec();
    x::openlog(ident.as_ptr().cast(), LOG_PID as i32, LOG_LOCAL1);
    log(LOG_ERR, "x");
    let (pri, _, rest) = split(&next(&r, true).unwrap());
    assert_eq!((pri, rest), (139, format!("clause-c1[{}]: x", std::process::id())));
    // ⚑ HIST: the identifier is kept by pointer.
    ident[..9].copy_from_slice(b"changed-1");
    log(LOG_ERR, "y");
    assert!(split(&next(&r, true).unwrap()).2.starts_with("changed-1["), "read again for each message");
    // An explicit facility wins; an invalid default facility is ignored.
    log(3 << 3 | LOG_ERR, "z");
    assert_eq!(split(&next(&r, true).unwrap()).0, 27);
    x::openlog(core::ptr::null(), 0, 0x400);
    log(LOG_ERR, "w");
    let (pri, _, rest) = split(&next(&r, true).unwrap());
    assert_eq!((pri, rest), (139, format!("{}: w", prog())), "options replaced; NULL is the program name (openlog(3)); facility 0x400 ignored");
    done();
    drop(ident);
    // The clean handle: owned identifier, explicit facility.
    let (sock, cons) = (z(&s.path("log")), z(&s.path("console")));
    let mut l = Logger::with_channels(&CSys, b"core-c1", LOG_PID, LOG_LOCAL1 as u32, &channels(&sock, &cons, Fd::new(2), 50_000_000)).unwrap();
    assert_eq!(l.log(&CSys, LOG_ERR as u32, STAMP, b"x"), Ok(Delivery::Sent));
    assert_eq!(next(&r, true).unwrap(), format!("<139>Oct  4 05:06:07 core-c1[{}]: x", std::process::id()).into_bytes());
    assert_eq!(Logger::with_channels(&CSys, b"bad", 0, 0x400, &channels(&sock, &cons, Fd::new(2), 0)).err(), Some(Errno::EINVAL), "a facility outside the field");
    assert_eq!(l.log(&CSys, 0x400 | 3, STAMP, b"x"), Err(Errno::EINVAL), "bits outside priority and facility");
    l.close(&CSys);
}

#[test]
fn s26_1_c_1_1() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let s = Scratch::new("c11");
    let r = receiver(&s.path("log"));
    let (sock, cons) = (z(&s.path("log")), z(&s.path("console")));
    let ch = channels(&sock, &cons, Fd::new(2), 50_000_000);
    let pid = std::process::id();
    let mut with = Logger::with_channels(&CSys, b"p", LOG_PID, 8, &ch).unwrap();
    let mut without = Logger::with_channels(&CSys, b"p", 0, 8, &ch).unwrap();
    with.log(&CSys, 6, STAMP, b"m").unwrap();
    assert_eq!(split(&next(&r, true).unwrap()).2, format!("p[{pid}]: m"), "LOG_PID");
    without.log(&CSys, 6, STAMP, b"m").unwrap();
    assert_eq!(split(&next(&r, true).unwrap()).2, "p: m", "no PID without LOG_PID");
    // LOG_CONS: undeliverable records go to the console (no priority, no timestamp, CR LF).
    drop(r);
    std::fs::remove_file(s.path("log")).unwrap();
    let mut cons_l = Logger::with_channels(&CSys, b"c", LOG_CONS | LOG_PID, 8, &ch).unwrap();
    assert_eq!(cons_l.log(&CSys, 6, STAMP, b"to the console"), Ok(Delivery::Console));
    assert_eq!(std::fs::read(s.path("console")).unwrap(), format!("c[{pid}]: to the console\r\n").into_bytes());
    for l in [&mut with, &mut without, &mut cons_l] {
        l.close(&CSys);
    }
    // Through the process-wide logger.
    std::fs::write(s.path("console"), b"").unwrap();
    global(&s);
    x::openlog(c"g".as_ptr(), (LOG_CONS | LOG_PID) as i32, LOG_USER);
    log(LOG_INFO, "via %% sign");
    assert_eq!(std::fs::read(s.path("console")).unwrap(), format!("g[{pid}]: via % sign\r\n").into_bytes());
    done();
}

#[test]
fn s26_1_c_1_2() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let s = Scratch::new("c12");
    let r = receiver(&s.path("log"));
    let (sock, cons) = (z(&s.path("log")), z(&s.path("console")));
    let ch = channels(&sock, &cons, Fd::new(2), 50_000_000);
    let mut now_l = Logger::with_channels(&CSys, b"n", LOG_NDELAY, 8, &ch).unwrap();
    assert!(now_l.is_connected(), "LOG_NDELAY connects at once");
    let mut lazy = Logger::with_channels(&CSys, b"o", LOG_ODELAY, 8, &ch).unwrap();
    assert!(!lazy.is_connected(), "LOG_ODELAY connects with the first message");
    lazy.log(&CSys, 6, STAMP, b"m").unwrap();
    assert!(lazy.is_connected());
    assert!(next(&r, true).is_some());
    now_l.close(&CSys);
    lazy.close(&CSys);
    // The process-wide logger: openlog with LOG_NDELAY opens the socket.
    global(&s);
    let before = sockets();
    x::openlog(c"nd".as_ptr(), LOG_NDELAY as i32, LOG_USER);
    assert_eq!(sockets(), before + 1, "connected by openlog");
    x::openlog(c"nd".as_ptr(), LOG_NDELAY as i32, LOG_USER);
    assert_eq!(sockets(), before + 1, "a second openlog keeps the connection");
    log(LOG_INFO, "m");
    assert_eq!(split(&next(&r, true).unwrap()).2, "nd: m");
    done();
}

#[test]
fn s26_1_c_1_3() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let s = Scratch::new("c13");
    let r = receiver(&s.path("log"));
    let (sock, cons) = (z(&s.path("log")), z(&s.path("console")));
    let ch = channels(&sock, &cons, Fd::new(2), 50_000_000);
    let mut a = Logger::with_channels(&CSys, b"w", LOG_NOWAIT, 8, &ch).unwrap();
    let mut b = Logger::with_channels(&CSys, b"w", 0, 8, &ch).unwrap();
    a.log(&CSys, 6, STAMP, b"m").unwrap();
    b.log(&CSys, 6, STAMP, b"m").unwrap();
    let (one, two) = (next(&r, true), next(&r, true));
    assert!(one.is_some());
    assert_eq!(one, two, "LOG_NOWAIT changes nothing visible");
    a.close(&CSys);
    b.close(&CSys);
}

#[test]
fn s26_1_c_1_4() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let s = Scratch::new("c14");
    let r = receiver(&s.path("log"));
    let (sock, cons) = (z(&s.path("log")), z(&s.path("console")));
    let (pr, pw) = pipe();
    let ch = channels(&sock, &cons, pw, 50_000_000);
    let mut l = Logger::with_channels(&CSys, b"perr", LOG_PERROR, 8, &ch).unwrap();
    l.log(&CSys, 3, STAMP, b"to both").unwrap();
    assert_eq!(drain(pr), b"perr: to both\n", "standard error: no priority, no timestamp, a newline");
    assert_eq!(split(&next(&r, true).unwrap()).2, "perr: to both");
    l.log(&CSys, 3, STAMP, b"line\n").unwrap();
    assert_eq!(drain(pr), b"perr: line\n", "no second newline");
    assert!(next(&r, true).is_some());
    l.log(&CSys, 3, STAMP, b"a\0b").unwrap();
    assert_eq!(drain(pr), b"perr: a\n", "cut at a NUL");
    assert!(next(&r, true).unwrap().ends_with(b"a\0b"), "the datagram keeps it");
    l.set_mask(1 << 3);
    l.log(&CSys, 6, STAMP, b"masked").unwrap();
    // Written whether or not the record is delivered.
    drop(r);
    std::fs::remove_file(s.path("log")).unwrap();
    assert_eq!(l.log(&CSys, 3, STAMP, b"lost"), Ok(Delivery::Lost));
    assert_eq!(drain(pr), b"perr: lost\n", "masked records are not copied; undelivered ones are");
    l.close(&CSys);
    let _ = CSys.close(pr);
    let _ = CSys.close(pw);
}

#[test]
fn s26_1_c_2() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let s = Scratch::new("c2");
    let r = receiver(&s.path("log"));
    global(&s);
    x::openlog(c"fmt".as_ptr(), 0, LOG_USER);
    errno::set_raw(EACCES);
    let t0 = now();
    log(LOG_INFO, "m=%m %%m [%-20m]");
    let (pri, stamp, rest) = split(&next(&r, true).unwrap());
    assert_eq!(pri, 14, "LOG_USER|LOG_INFO");
    assert!((t0 - 1..=now() + 1).any(|t| stamp_at(t) == stamp), "local time, Mmm dd hh:mm:ss: {stamp:?}");
    assert_eq!(rest, "fmt: m=Permission denied %m [Permission denied   ]", "printf plus %m (errno at entry)");
    assert_eq!(errno::get(), EACCES);
    log(LOG_INFO, "trailing\n");
    assert_eq!(split(&next(&r, true).unwrap()).2, "fmt: trailing\n", "nothing stripped or added");
    // An empty identifier keeps its blank; no identifier is the program's short name.
    x::openlog(c"".as_ptr(), 0, LOG_USER);
    log(LOG_INFO, "e");
    assert_eq!(split(&next(&r, true).unwrap()).2, ": e");
    x::closelog();
    log(LOG_INFO, "p");
    assert_eq!(split(&next(&r, true).unwrap()).2, format!("{}: p", prog()));
    // Bits outside the fields: a report at LOG_AUTH|LOG_ERR, then the record with them cleared.
    log(0x400 | LOG_ERR, "bad");
    let (p1, _, t1) = split(&next(&r, true).unwrap());
    assert_eq!((p1, t1), (35, format!("{}: syslog: unknown facility/priority: 403", prog())));
    let (p2, _, t2) = split(&next(&r, true).unwrap());
    assert_eq!((p2, t2), (11, format!("{}: bad", prog())));
    done();
    // The handle over a stream listener: each record ends with a NUL, its text cut at a NUL.
    drop(r);
    std::fs::remove_file(s.path("log")).unwrap();
    let ls = UnixListener::bind(s.path("log")).unwrap();
    let (sock, cons) = (z(&s.path("log")), z(&s.path("console")));
    let mut l = Logger::with_channels(&CSys, b"st", 0, 8, &channels(&sock, &cons, Fd::new(2), 50_000_000)).unwrap();
    l.log(&CSys, 6, STAMP, b"one").unwrap();
    l.log(&CSys, 6, STAMP, b"a\0b").unwrap();
    l.close(&CSys);
    let (mut c, _) = ls.accept().unwrap();
    c.set_read_timeout(Some(Duration::from_secs(3))).unwrap();
    let mut got = Vec::new();
    let _ = c.read_to_end(&mut got);
    assert_eq!(got, b"<14>Oct  4 05:06:07 st: one\0<14>Oct  4 05:06:07 st: a\0");
}

#[test]
fn s26_1_c_3() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let s = Scratch::new("c3");
    let r = receiver(&s.path("log"));
    global(&s);
    assert_eq!(x::setlogmask(0), 0xff, "0 asks; every priority passes at first");
    assert_eq!(x::setlogmask(1 << LOG_ERR), 0xff, "LOG_MASK(LOG_ERR); the previous mask");
    log(LOG_INFO, "no");
    log(LOG_ERR, "yes");
    assert!(split(&next(&r, true).unwrap()).2.ends_with(": yes"));
    assert!(next(&r, false).is_none());
    assert_eq!(x::setlogmask((1 << (LOG_WARNING + 1)) - 1), 1 << LOG_ERR, "LOG_UPTO(LOG_WARNING)");
    log(LOG_INFO, "no");
    log(LOG_WARNING, "yes");
    log(0x80 | LOG_INFO, "no");
    assert_eq!(split(&next(&r, true).unwrap()).0, 12);
    assert!(next(&r, false).is_none(), "the facility plays no part");
    let (sock, cons) = (z(&s.path("log")), z(&s.path("console")));
    let mut l = Logger::with_channels(&CSys, b"m", 0, 8, &channels(&sock, &cons, Fd::new(2), 0)).unwrap();
    assert_eq!(l.set_mask(1 << 6), 0xff);
    assert!(l.passes(6) && !l.passes(3));
    done();
}

#[test]
fn s26_1_c_4() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let s = Scratch::new("c4");
    let r = receiver(&s.path("log"));
    global(&s);
    let before = sockets();
    log(LOG_INFO, "open");
    assert!(next(&r, true).is_some());
    assert_eq!(sockets(), before + 1, "the first message connects");
    x::closelog();
    assert_eq!(sockets(), before, "closelog closes the connection");
    x::closelog();
    assert_eq!(sockets(), before, "twice is harmless");
    log(LOG_INFO, "again");
    assert!(next(&r, true).is_some(), "the next message connects again");
    let (sock, cons) = (z(&s.path("log")), z(&s.path("console")));
    let mut l = Logger::with_channels(&CSys, b"c", LOG_NDELAY, 8, &channels(&sock, &cons, Fd::new(2), 0)).unwrap();
    l.close(&CSys);
    assert!(!l.is_connected());
    done();
}

#[test]
fn s26_1_g_1() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let s = Scratch::new("g1");
    let r = receiver(&s.path("log"));
    global(&s);
    x::setlogmask(1 << LOG_ERR);
    let before = sockets();
    // Formatting this with the checks on would abort the process (a %n in writable memory).
    let writable = CString::new("%n").unwrap().into_bytes_with_nul();
    x::__vsyslog_chk(LOG_INFO, 1, writable.as_ptr().cast(), null_mut());
    assert!(next(&r, false).is_none(), "discarded");
    assert_eq!(sockets(), before, "without connecting");
    done();
}

#[test]
fn s26_1_g_2() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let s = Scratch::new("g2");
    let r1 = receiver(&s.path("log"));
    let (sock, cons) = (z(&s.path("log")), z(&s.path("console")));
    let mut l = Logger::with_channels(&CSys, b"re", 0, 8, &channels(&sock, &cons, Fd::new(2), 50_000_000)).unwrap();
    assert_eq!(l.log(&CSys, 6, STAMP, b"one"), Ok(Delivery::Sent));
    assert!(next(&r1, true).is_some());
    // The receiver is replaced: the send on the old connection fails, the retry reaches the new one.
    drop(r1);
    let r2 = receiver(&s.path("log"));
    assert_eq!(l.log(&CSys, 6, STAMP, b"two"), Ok(Delivery::Sent));
    assert_eq!(split(&next(&r2, true).unwrap()).2, "re: two");
    l.close(&CSys);
    // The same through the process-wide logger.
    global(&s);
    log(LOG_INFO, "a");
    assert!(next(&r2, true).is_some());
    drop(r2);
    let r3 = receiver(&s.path("log"));
    log(LOG_INFO, "b");
    assert!(split(&next(&r3, true).unwrap()).2.ends_with(": b"));
    done();
}

#[test]
fn s26_1_g_3() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let s = Scratch::new("g3");
    let (sock, cons) = (z(&s.path("log")), z(&s.path("console")));
    let ch = channels(&sock, &cons, Fd::new(2), 50_000_000);
    let mut plain = Logger::with_channels(&CSys, b"lost", 0, 8, &ch).unwrap();
    assert_eq!(plain.log(&CSys, 6, STAMP, b"m"), Ok(Delivery::Lost), "no receiver, no LOG_CONS");
    assert!(std::fs::read(s.path("console")).unwrap().is_empty());
    // A receiver that does not read: the wait is bounded, then the console takes the record.
    let r = receiver(&s.path("log"));
    let mut l = Logger::with_channels(&CSys, b"busy", LOG_CONS, 8, &ch).unwrap();
    let mut outcome = Delivery::Sent;
    let mut slowest = Duration::ZERO;
    for _ in 0..5000 {
        let t = Instant::now();
        outcome = l.log(&CSys, 6, STAMP, &[b'x'; 200]).unwrap();
        slowest = slowest.max(t.elapsed());
        if outcome != Delivery::Sent {
            break;
        }
    }
    assert_eq!(outcome, Delivery::Console, "a full receiver: the record goes to the console");
    // HANG GUARD, not a speed measurement (WALLCLOCK-ASSERT-SWEEP): a blocked send waits for ever (or
    // for the long wait the logger would have), so a minute cannot be tripped by load.
    assert!(slowest < Duration::from_secs(60), "never blocked: {slowest:?}");
    assert!(std::fs::read(s.path("console")).unwrap().starts_with(b"busy: xxx"));
    drop(r);
    // A FIFO console nobody reads: not waited for.
    std::fs::remove_file(s.path("console")).unwrap();
    let f = CString::new(s.path("console").as_os_str().as_encoded_bytes()).unwrap();
    assert_eq!(liella_boundary::posix::s13_filesystem::mkfifo(f.as_ptr(), 0o600), 0);
    std::fs::remove_file(s.path("log")).unwrap();
    let t = Instant::now();
    assert_eq!(l.log(&CSys, 6, STAMP, b"m"), Ok(Delivery::Lost));
    // HANG GUARD (WALLCLOCK-ASSERT-SWEEP): a blocking open of the FIFO never returns; 60 s idle-machine
    // base (scaled further by load) cannot be reached by a slow host.
    timeout::assert_within("a lost log message must not block", Duration::from_secs(60), t.elapsed());
    l.close(&CSys);
    plain.close(&CSys);
}

#[test]
fn s26_1_g_4() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let s = Scratch::new("g4");
    let r = receiver(&s.path("log"));
    global(&s);
    log(LOG_INFO, "connect");
    assert!(next(&r, true).is_some());
    // The receiver goes: the sends fail, nothing is reported and errno is left alone.
    drop(r);
    errno::set_raw(EDOM);
    log(LOG_INFO, "to a closed receiver");
    assert_eq!(errno::get(), EDOM, "a failed send");
    std::fs::remove_file(s.path("log")).unwrap();
    log(LOG_INFO, "to no receiver");
    assert_eq!(errno::get(), EDOM, "no /dev/log");
    let r = receiver(&s.path("log"));
    let big = CString::new(vec![b'b'; 1 << 20]).unwrap();
    errno::set_raw(EDOM);
    x::vsyslog(LOG_INFO, big.as_ptr(), null_mut());
    assert_eq!(errno::get(), EDOM, "a message too long for a datagram");
    x::vsyslog(LOG_INFO, core::ptr::null(), null_mut());
    assert_eq!(errno::get(), EDOM, "a NULL format logs nothing");
    assert!(next(&r, false).is_none());
    done();
}
