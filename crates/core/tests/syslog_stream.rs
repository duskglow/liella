//! §26 over a stream listener under stress (SYSLOG-STREAM-PARTIAL): the handle
//! `liella::log::Logger` on channels in a scratch directory (no test writes to the system log),
//! the log path a stream listener so the connection falls back to a stream socket
//! (`EPROTOTYPE`). Each record is big (300000 bytes of text), so it takes many partial sends:
//!
//! * a slow reader with a small `SO_RCVBUF`: the whole record and its NUL arrive;
//! * the reader closing mid-record: the caller is not killed by `SIGPIPE` (it runs with the
//!   default disposition, in a child process of its own), the record is sent whole again on a
//!   new connection and the next record follows it there;
//! * signals without `SA_RESTART` aimed at the sending thread throughout the send: the whole
//!   record and its NUL arrive.
//!
//! Probed against the reference C library 2.39 with a C program over a private listener bound
//! on `/dev/log` in an unprivileged mount namespace (record in the SYSLOG-STREAM-PARTIAL
//! report): the slow reader gets the whole record and its NUL; a reader closing mid-record does
//! not kill the caller and the next record arrives on a new connection.
#![allow(clippy::unwrap_used, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, missing_docs)]

#[path = "support/scratch.rs"]
mod scratch;

use core::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::io::Read;
use std::os::fd::AsRawFd;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use liella::log::{Channels, Delivery, Logger, Stamp};
use liella_boundary::posix::s20_signals as sg;
use liella_boundary::sys::CSys;
use liella_kernel::fd::Fd;
use liella_kernel::ops::SockLevel;
use liella_kernel::path::KStr;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::SigSet;
use liella_legacy::signal_compat::CSigaction;

const BIG: usize = 300_000;
const SIGUSR1: i32 = 10;
const SIGPIPE: i32 = 13;
const SO_RCVBUF: u32 = 8;
const STAMP: Stamp = Stamp { month: 9, day: 4, hour: 5, min: 6, sec: 7 };
/// Long enough for a slow reader on a loaded machine: the test is about framing, not the limit.
const WAIT_NS: i64 = 30_000_000_000;

/// `p` with a NUL.
fn z(p: &Path) -> Vec<u8> {
    let mut v = p.as_os_str().as_encoded_bytes().to_vec();
    v.push(0);
    v
}

/// The stream form of a record of the handle `pp` at `STAMP`, `LOG_USER|LOG_INFO`.
fn rec(text: &[u8]) -> Vec<u8> {
    let mut v = b"<14>Oct  4 05:06:07 pp: ".to_vec();
    v.extend_from_slice(text);
    v.push(0);
    v
}

/// A summary for a failed comparison (the records are too big to print).
fn show(b: &[u8]) -> String {
    let nuls: Vec<usize> = b.iter().enumerate().filter(|(_, c)| **c == 0).map(|(i, _)| i).collect();
    format!("{} bytes, NULs at {:?}", b.len(), &nuls[..nuls.len().min(8)])
}

/// A logger `pp` (`LOG_USER`) on a stream listener at `sock`.
fn logger(sock: &[u8], cons: &[u8]) -> Logger {
    let ch = Channels { socket: KStr::from_nul_terminated(sock).unwrap(), console: KStr::from_nul_terminated(cons).unwrap(), stderr: Fd::new(2), wait_ns: WAIT_NS };
    Logger::with_channels(&CSys, b"pp", 0, 8, &ch).unwrap()
}

/// Accept one connection: a small `SO_RCVBUF`, a read timeout against hangs.
fn accept(ls: &UnixListener, rcvbuf: bool) -> UnixStream {
    let (c, _) = ls.accept().unwrap();
    if rcvbuf {
        CSys.setsockopt(Fd::new(c.as_raw_fd()), SockLevel::Socket, SO_RCVBUF, &1024i32.to_ne_bytes()).unwrap();
    }
    c.set_read_timeout(Some(Duration::from_secs(20))).unwrap();
    c
}

/// Read `c` slowly, 512 bytes at a time with a pause, after a first pause that fills the
/// sender's buffer; to end of file, or `cut` bytes at most.
fn read_slow(c: &mut UnixStream, pause: Duration, cut: Option<usize>) -> Vec<u8> {
    std::thread::sleep(Duration::from_millis(50));
    let mut got = Vec::new();
    let mut b = [0u8; 512];
    loop {
        let want = cut.map_or(b.len(), |k| (k - got.len()).min(b.len()));
        if want == 0 {
            break;
        }
        match c.read(&mut b[..want]) {
            Ok(0) | Err(_) => break,
            Ok(n) => got.extend_from_slice(&b[..n]),
        }
        std::thread::sleep(pause);
    }
    got
}

/// Run test `name` alone in a child process (its own signal dispositions); `body` is its
/// verdict there.
fn alone(name: &str, body: impl FnOnce() -> bool) {
    if std::env::var("LIELLA_SYSLOG_STREAM_ALONE").is_ok_and(|v| v == name) {
        CSys.exit_group(if body() { 0 } else { 1 });
    }
    let o = std::process::Command::new(std::env::current_exe().unwrap())
        .args([name, "--exact", "--test-threads=1", "--nocapture"])
        .env("LIELLA_SYSLOG_STREAM_ALONE", name)
        .output()
        .unwrap();
    let err = String::from_utf8_lossy(&o.stderr);
    let tail: String = err.lines().rev().take(12).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
    use std::os::unix::process::ExitStatusExt;
    assert!(o.status.code() == Some(0), "{name} failed in its child (exit {:?}, signal {:?}; 13 = SIGPIPE): {tail}", o.status.code(), o.status.signal());
}

#[test]
fn syslog_stream_slow_reader_gets_whole_record() {
    // Trace: s26_1_c_2 (a record over a stream listener ends with its NUL), partial sends.
    let s = scratch::Scratch::sock_dir("s26-stream-slow");
    let (sock, cons) = (z(&s.join("log")), z(&s.join("console")));
    let ls = UnixListener::bind(s.join("log")).unwrap();
    let reader = std::thread::spawn(move || read_slow(&mut accept(&ls, true), Duration::from_micros(100), None));
    let text = vec![b'x'; BIG];
    let mut l = logger(&sock, &cons);
    assert_eq!(l.log(&CSys, 6, STAMP, &text), Ok(Delivery::Sent));
    l.close(&CSys);
    let got = reader.join().unwrap();
    assert!(got == rec(&text), "the whole record and its NUL: got {}, want {}", show(&got), show(&rec(&text)));
}

#[test]
fn syslog_stream_reader_closing_mid_record() {
    // Trace: s26_1_c_2 (a failed send is retried once on a new connection), SIGPIPE.
    alone("syslog_stream_reader_closing_mid_record", || {
        sg::signal(SIGPIPE, 0);
        let s = scratch::Scratch::sock_dir("s26-stream-close");
        let (sock, cons) = (z(&s.join("log")), z(&s.join("console")));
        let ls = UnixListener::bind(s.join("log")).unwrap();
        let reader = std::thread::spawn(move || {
            let first = read_slow(&mut accept(&ls, true), Duration::ZERO, Some(1000));
            let mut c2 = accept(&ls, false);
            let mut rest = Vec::new();
            let _ = c2.read_to_end(&mut rest);
            (first.len(), rest)
        });
        let text = vec![b'a'; BIG];
        let mut l = logger(&sock, &cons);
        let r1 = l.log(&CSys, 6, STAMP, &text);
        let r2 = l.log(&CSys, 6, STAMP, b"second");
        l.close(&CSys);
        let (cut, rest) = reader.join().unwrap();
        let mut want = rec(&text);
        want.extend_from_slice(&rec(b"second"));
        let ok = r1 == Ok(Delivery::Sent) && r2 == Ok(Delivery::Sent) && cut == 1000 && rest == want;
        if !ok {
            eprintln!("r1 {r1:?} r2 {r2:?} first connection read {cut}; second connection: got {}, want {}", show(&rest), show(&want));
        }
        ok
    });
}

/// A handler as its address.
fn handler(f: extern "C" fn(i32)) -> usize {
    f as usize
}

static HITS: AtomicUsize = AtomicUsize::new(0);

extern "C" fn on_usr1(_: i32) {
    HITS.fetch_add(1, Ordering::SeqCst);
}

#[test]
fn syslog_stream_send_interrupted_by_signals() {
    // Trace: s26_1_c_2 (the whole record and its NUL arrive), EINTR during the send.
    alone("syslog_stream_send_interrupted_by_signals", || {
        let a = CSigaction::new(handler(on_usr1), SigSet::empty(), 0); // no SA_RESTART
        assert_eq!(sg::sigaction(SIGUSR1, Some(&a), None), 0);
        let s = scratch::Scratch::sock_dir("s26-stream-eintr");
        let (sock, cons) = (z(&s.join("log")), z(&s.join("console")));
        let ls = UnixListener::bind(s.join("log")).unwrap();
        let reader = std::thread::spawn(move || read_slow(&mut accept(&ls, true), Duration::from_micros(200), None));
        let (pid, tid) = (CSys.getpid(), CSys.gettid());
        let stop = Arc::new(AtomicBool::new(false));
        let kick = {
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                while !stop.load(Ordering::SeqCst) {
                    let _ = CSys.tgkill(pid, tid, SIGUSR1);
                    std::thread::sleep(Duration::from_millis(1));
                }
            })
        };
        let text = vec![b'x'; BIG];
        let mut l = logger(&sock, &cons);
        let r = l.log(&CSys, 6, STAMP, &text);
        stop.store(true, Ordering::SeqCst);
        kick.join().unwrap();
        l.close(&CSys);
        let got = reader.join().unwrap();
        let hits = HITS.load(Ordering::SeqCst);
        let ok = r == Ok(Delivery::Sent) && hits > 0 && got == rec(&text);
        if !ok {
            eprintln!("r {r:?} signals {hits}: got {}, want {}", show(&got), show(&rec(&text)));
        }
        ok
    });
}
