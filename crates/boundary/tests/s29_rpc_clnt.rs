// §29.1 ONC RPC client, UDP transport (LG-15, D-29.1); RFC 5531 wire format.
//! The Liella UDP client against a raw `UdpSocket` playing the server: a reply for another xid
//! is skipped and the retry's reply taken, a truncated reply after a full one is
//! `RPC_CANTDECODERES` (only the received bytes are read), a silent server times out at the
//! total timeout with retries, plus the port mapper's byte order and `bindresvport`.
#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::arithmetic_side_effects, clippy::indexing_slicing, unreachable_pub)]

use core::ffi::{c_char, c_int, c_void};
use std::net::UdpSocket;
use std::time::{Duration, Instant};

use liella_boundary::posix::s04_strings::memcpy;
use liella_boundary::posix::s29_legacy::{Xdr, XdrProc, liella_compat_xdr_int};
use liella_boundary::posix::s29_rpc_clnt as clnt;
use clnt::Timeval;

const PROG: u32 = 0x2000_4321;

fn xdr_int() -> XdrProc {
    let f: extern "C" fn(*mut Xdr, *mut c_int) -> c_int = liella_compat_xdr_int;
    let addr = f as usize;
    let mut p: XdrProc = None;
    memcpy((&raw mut p).cast(), (&raw const addr).cast(), core::mem::size_of::<usize>());
    p
}

fn words(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|v| v.to_be_bytes()).collect()
}

fn sin(port: u16) -> [u8; 16] {
    let mut a = [0u8; 16];
    a[..2].copy_from_slice(&2u16.to_ne_bytes());
    a[2..4].copy_from_slice(&port.to_be_bytes());
    a[4..8].copy_from_slice(&[127, 0, 0, 1]);
    a
}

/// A fake server and a client for it (retry every `wait_ms`).
fn pair(wait_ms: i64) -> (UdpSocket, *mut clnt::Client) {
    let s = UdpSocket::bind("127.0.0.1:0").unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let mut a = sin(s.local_addr().unwrap().port());
    let mut sock: c_int = -1;
    let w = Timeval { tv_sec: wait_ms / 1000, tv_usec: (wait_ms % 1000) * 1000 };
    let c = clnt::liella_compat_clntudp_create(a.as_mut_ptr().cast::<c_void>(), PROG.into(), 1, w, &mut sock);
    assert!(!c.is_null() && sock >= 0);
    (s, c)
}

/// One call of procedure 1 with argument 5; the status and the result.
fn call(c: *mut clnt::Client, secs: i64) -> (c_int, c_int) {
    let (mut a, mut r): (c_int, c_int) = (5, -1);
    let t = Timeval { tv_sec: secs, tv_usec: 0 };
    let st = clnt::clnt_call(c, 1, xdr_int(), (&raw mut a).cast::<c_char>(), xdr_int(), (&raw mut r).cast::<c_char>(), t);
    (st, r)
}

fn recv_xid(s: &UdpSocket) -> (u32, std::net::SocketAddr) {
    let mut b = [0u8; 2000];
    let (n, from) = s.recv_from(&mut b).unwrap();
    assert!(n >= 24 && b[4..8] == [0, 0, 0, 0], "a call");
    (u32::from_be_bytes(b[..4].try_into().unwrap()), from)
}

fn ok_reply(xid: u32, v: u32) -> Vec<u8> {
    words(&[xid, 1, 0, 0, 0, 0, v])
}

#[test]
fn clnt_udp_retry_truncation_timeout() {
    // Trace: no spec clause (LG-15 ONC RPC client over UDP; RFC 5531 wire format, probe-settled host-glibc behaviour); D-29.1.
    assert_eq!(core::mem::size_of::<clnt::Client>(), 24);
    assert_eq!(core::mem::size_of::<clnt::AuthC>(), 72);
    assert_eq!(core::mem::size_of::<clnt::CreateErrC>(), 32);

    // A reply for another xid is skipped; the retransmission (same xid) gets the real reply.
    let (s, c) = pair(200);
    let mut tv = Timeval::default();
    assert_eq!(clnt::clnt_control(c, 2, (&raw mut tv).cast()), 1);
    assert_eq!(tv, Timeval { tv_sec: -1, tv_usec: -1 }, "CLGET_TIMEOUT before CLSET_TIMEOUT");
    let t = std::thread::spawn(move || {
        let (x, from) = recv_xid(&s);
        s.send_to(&ok_reply(x ^ 1, 99), from).unwrap();
        let (x2, from) = recv_xid(&s);
        assert_eq!(x2, x, "a retry keeps the xid");
        s.send_to(&ok_reply(x, 42), from).unwrap();
        s
    });
    assert_eq!(call(c, 3), (0, 42), "wrong xid skipped, retry answered");
    let s = t.join().unwrap();

    // A full reply, then a truncated one in the same buffer: only the received bytes count.
    let t = std::thread::spawn(move || {
        let (x, from) = recv_xid(&s);
        s.send_to(&ok_reply(x, 7), from).unwrap();
        let (x, from) = recv_xid(&s);
        s.send_to(&words(&[x, 1, 0, 0, 0, 0]), from).unwrap();
        let (x, from) = recv_xid(&s);
        s.send_to(&words(&[x, 1, 0, 0]), from).unwrap();
        s
    });
    assert_eq!(call(c, 3), (0, 7));
    assert_eq!(call(c, 3).0, 2, "success without results is CANTDECODERES");
    assert_eq!(call(c, 3).0, 2, "truncated verifier is CANTDECODERES");
    let s = t.join().unwrap();

    // A silent server: TIMEDOUT at the total timeout (1 s), retrying every 200 ms.
    let t0 = Instant::now();
    assert_eq!(call(c, 1).0, 5, "silent server times out");
    let took = t0.elapsed();
    assert!(took >= Duration::from_millis(950) && took < Duration::from_millis(1600), "total timeout honoured: {took:?}");
    s.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
    let mut b = [0u8; 2000];
    let mut sends = 0;
    while s.recv_from(&mut b).is_ok() {
        sends += 1;
    }
    assert!((3..=7).contains(&sends), "retries every wait: {sends} sends");
    clnt::clnt_destroy(c);

    // The port mapper's call arguments and the stored port are network order.
    assert_eq!(clnt::pmap_args(PROG, 1, 17), words(&[PROG, 1, 17, 0])[..]);
    assert_eq!(clnt::pmap_result(&words(&[2049])), Some(2049));
    assert_eq!(clnt::pmap_result(&[0, 0, 8]), None, "cut short");
    // As probed on the host glibc: the low 16 bits are the port (65536 gives 0, not registered).
    assert_eq!(clnt::pmap_result(&words(&[70_000])), Some(4464), "low 16 bits");
    assert_eq!(clnt::pmap_result(&words(&[65_536])), Some(0), "low 16 bits");
    let mut a = sin(0);
    clnt::set_port(&mut a, 0x1234);
    assert_eq!(a[2..4], [0x12, 0x34]);

    // bindresvport: a non-AF_INET address is EAFNOSUPPORT; unprivileged, -1 (EACCES, as probed);
    // privileged (a root test harness), a port in 512..=1023.
    let fd = liella_boundary::posix::s15_sockets::socket(2, 2, 0);
    let mut bad = sin(0);
    bad[..2].copy_from_slice(&10u16.to_ne_bytes());
    assert_eq!(clnt::liella_compat_bindresvport(fd, bad.as_mut_ptr().cast()), -1);
    let mut any = sin(0);
    any[4..8].copy_from_slice(&[0, 0, 0, 0]);
    let r = clnt::liella_compat_bindresvport(fd, any.as_mut_ptr().cast());
    if r == 0 {
        let p = u16::from_be_bytes([any[2], any[3]]);
        assert!((512..=1023).contains(&p), "reserved port {p}");
    } else {
        // Unprivileged: the first bind's EACCES ends the search (EACCES itself is probe-settled;
        // errno is not read here: the test binary's errno is the host's).
        assert_eq!(r, -1);
    }
    liella_boundary::posix::s11_lowio::close(fd);
}
