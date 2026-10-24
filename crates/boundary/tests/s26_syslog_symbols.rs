// Specified from spec §26 (phase 13).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Translation-layer tests for the exported symbols of spec §26 (7 entries).
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! The process-wide logger sends to a receiving socket and a console file in a scratch
//! directory under `/tmp` (`selftest::syslog_paths`, accepted only in tests): no test writes
//! to the system log. The tests share the logger, so each takes [`LOCK`]. `syslog` and
//! `__syslog_chk` are variadic: their fixed-arity target `liella_vsyslog4` is tested here (with
//! formats that take no argument) and the C entry points through real varargs in
//! `crates/core/tests/syslog_products.rs` (a private `/dev`).
#![allow(non_snake_case)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, missing_docs)]

#[path = "../../core/tests/support/scratch.rs"]
mod scratch;

use core::ptr::{null, null_mut};
use std::ffi::CString;
use std::os::unix::net::UnixDatagram;
use std::sync::Mutex;
use std::time::Duration;

use liella_boundary::errno;
use liella_boundary::posix::s26_syslog as s;

const LOG_PID: i32 = 1;
const LOG_CONS: i32 = 2;
const LOG_NDELAY: i32 = 8;
const LOG_ERR: i32 = 3;
const LOG_INFO: i32 = 6;
const LOG_DEBUG: i32 = 7;
const LOG_USER: i32 = 8;
const LOG_DAEMON: i32 = 3 << 3;
const LOG_LOCAL2: i32 = 18 << 3;
const ENOENT: i32 = 2;

static LOCK: Mutex<()> = Mutex::new(());

/// A scratch directory with a receiver at `log` and an empty `console`, the process-wide logger
/// reset and sent there; everything removed when dropped.
struct Fixture {
    dir: scratch::Scratch,
    rx: UnixDatagram,
    _g: std::sync::MutexGuard<'static, ()>,
}

impl Fixture {
    fn new(tag: &str) -> Fixture {
        let g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // A short path under /tmp: the socket's path must fit `sun_path`, wherever the tree lives.
        let dir = scratch::Scratch::sock_dir(&format!("s26sym-{tag}"));
        std::fs::write(dir.join("console"), b"").unwrap();
        let rx = UnixDatagram::bind(dir.join("log")).unwrap();
        s::closelog();
        s::openlog(null(), 0, LOG_USER);
        s::setlogmask(0xff);
        let (l, c) = (dir.join("log"), dir.join("console"));
        liella_boundary::selftest::syslog_paths(Some((l.as_os_str().as_encoded_bytes(), c.as_os_str().as_encoded_bytes(), 50_000_000)));
        Fixture { dir, rx, _g: g }
    }

    /// The next record (`None` within 300 ms).
    fn next(&self) -> Option<String> {
        self.rx.set_read_timeout(Some(Duration::from_millis(300))).unwrap();
        let mut b = vec![0u8; 1 << 16];
        let n = self.rx.recv(&mut b).ok()?;
        Some(String::from_utf8_lossy(&b[..n]).into_owned())
    }

    /// The next record's priority and its text after the timestamp.
    fn record(&self) -> (i32, String) {
        let r = self.next().unwrap();
        let close = r.find('>').unwrap();
        (r[1..close].parse().unwrap(), r[close + 17..].to_string())
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        s::closelog();
        s::openlog(null(), 0, LOG_USER);
        s::setlogmask(0xff);
        liella_boundary::selftest::syslog_paths(None);
    }
}

fn log(pri: i32, fmt: &str) {
    let f = CString::new(fmt).unwrap();
    s::vsyslog(pri, f.as_ptr(), null_mut());
}

fn prog() -> String {
    std::env::current_exe().unwrap().file_name().unwrap().to_string_lossy().into_owned()
}

#[test]
fn sym_openlog() {
    let f = Fixture::new("openlog");
    s::openlog(c"id".as_ptr(), LOG_PID, LOG_LOCAL2);
    log(LOG_ERR, "x");
    assert_eq!(f.record(), (LOG_LOCAL2 | LOG_ERR, format!("id[{}]: x", std::process::id())), "identifier, options, facility");
    s::openlog(null(), 0, -1);
    log(LOG_ERR, "y");
    assert_eq!(f.record(), (LOG_LOCAL2 | LOG_ERR, format!("{}: y", prog())), "NULL is the program name (openlog(3)); -1 is not a facility");
    s::openlog(c"id".as_ptr(), 0, 0);
    log(LOG_ERR, "k");
    assert_eq!(f.record().0, LOG_ERR, "facility 0 (LOG_KERN) becomes the default");
    s::openlog(c"nd".as_ptr(), LOG_NDELAY, LOG_USER);
    assert!(f.next().is_none(), "connecting sends nothing");
}

#[test]
fn sym_closelog() {
    let f = Fixture::new("closelog");
    s::openlog(c"named".as_ptr(), LOG_PID, LOG_DAEMON);
    log(LOG_INFO, "a");
    assert!(f.record().1.starts_with("named["));
    s::closelog();
    log(LOG_INFO, "b");
    assert_eq!(f.record(), (LOG_DAEMON | LOG_INFO, format!("{}[{}]: b", prog(), std::process::id())), "the identifier is forgotten; options and facility stay");
    s::closelog();
    s::closelog();
}

#[test]
fn sym_setlogmask() {
    let f = Fixture::new("setlogmask");
    assert_eq!(s::setlogmask(0), 0xff);
    assert_eq!(s::setlogmask(1 << LOG_DEBUG), 0xff, "the previous mask");
    assert_eq!(s::setlogmask(0), 1 << LOG_DEBUG, "0 changes nothing");
    log(LOG_INFO, "masked");
    log(LOG_DEBUG, "passes");
    assert!(f.record().1.ends_with(": passes"));
    assert!(f.next().is_none());
    assert_eq!(s::setlogmask(-1), 1 << LOG_DEBUG);
    assert_eq!(s::setlogmask(0), -1, "stored whole");
}

#[test]
fn sym_syslog() {
    // The variadic entry point's target: `liella_vsyslog4(pri, 0, fmt, ap)`.
    let f = Fixture::new("syslog");
    errno::set_raw(ENOENT);
    s::liella_vsyslog4(LOG_INFO, 0, c"%m".as_ptr(), null_mut());
    assert_eq!(f.record(), (LOG_USER | LOG_INFO, format!("{}: No such file or directory", prog())));
    assert_eq!(errno::get(), ENOENT, "errno is left alone");
    s::liella_vsyslog4(LOG_INFO, 0, null(), null_mut());
    assert!(f.next().is_none(), "a NULL format logs nothing");
}

#[test]
fn sym___syslog_chk() {
    let f = Fixture::new("syslog_chk");
    s::liella_vsyslog4(LOG_ERR, 1, c"checked %%".as_ptr(), null_mut());
    assert_eq!(f.record(), (LOG_USER | LOG_ERR, format!("{}: checked %", prog())));
    s::setlogmask(1 << LOG_ERR);
    let writable = CString::new("%n").unwrap().into_bytes_with_nul();
    s::liella_vsyslog4(LOG_INFO, 1, writable.as_ptr().cast(), null_mut());
    assert!(f.next().is_none(), "masked: not formatted, so no %n check");
}

#[test]
fn sym_vsyslog() {
    let f = Fixture::new("vsyslog");
    s::openlog(c"v".as_ptr(), LOG_CONS, LOG_USER);
    s::vsyslog(LOG_INFO, c"plain".as_ptr(), null_mut());
    assert_eq!(f.record(), (LOG_USER | LOG_INFO, "v: plain".to_string()));
    // Undeliverable: the console.
    std::fs::remove_file(f.dir.join("log")).unwrap();
    s::closelog();
    s::openlog(c"v".as_ptr(), LOG_CONS, LOG_USER);
    s::vsyslog(LOG_INFO, c"to the console".as_ptr(), null_mut());
    assert_eq!(std::fs::read(f.dir.join("console")).unwrap(), b"v: to the console\r\n");
}

#[test]
fn sym___vsyslog_chk() {
    let f = Fixture::new("vsyslog_chk");
    s::__vsyslog_chk(LOG_INFO, 2, c"flag 2".as_ptr(), null_mut());
    assert!(f.record().1.ends_with(": flag 2"));
    s::__vsyslog_chk(0x400 | LOG_INFO, -1, c"bad bits".as_ptr(), null_mut());
    assert_eq!(f.record(), (35, format!("{}: syslog: unknown facility/priority: 406", prog())));
    assert_eq!(f.record(), (LOG_USER | LOG_INFO, format!("{}: bad bits", prog())));
}
