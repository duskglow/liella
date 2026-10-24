// §29.1 ONC RPC client, UDP transport: where replies may come from (D-29.2, COMPAT-12).
//! A UDP call takes replies only from the address and port it was sent to (owner ruling 10-23):
//! replies from another address or another port are dropped and the real reply still
//! completes the call; a reply-shaped stray ends a timed-out call with `RPC_CANTRECV` and
//! `EREMCHG` whatever its xid (no oracle), junk leaves plain `RPC_TIMEDOUT`, no stray ends a call
//! early; `liella.compat.rpc_reply_any_source=1` restores the reference behaviour and is
//! ignored in a secure process; xids do not follow from one call to the next. The differential
//! case runs the host glibc client against the same stray reply and lists the difference.
#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::arithmetic_side_effects, clippy::indexing_slicing, unreachable_pub)]

use core::ffi::{c_char, c_int, c_void};
use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use liella::config::Tunables;
use liella_boundary::posix::s04_strings::memcpy;
use liella_boundary::posix::s29_legacy::{Xdr, XdrProc, liella_compat_xdr_int};
use liella_boundary::posix::s29_rpc_clnt as clnt;
use liella_boundary::posix::s29_rpc_svc::RpcErrC;
use clnt::Timeval;

const PROG: u32 = 0x2000_4322;
const CANTRECV: c_int = 4;
const TIMEDOUT: c_int = 5;
const EREMCHG: u64 = 78;

fn xdr_int() -> XdrProc {
    let f: extern "C" fn(*mut Xdr, *mut c_int) -> c_int = liella_compat_xdr_int;
    let addr = f as usize;
    let mut p: XdrProc = None;
    memcpy((&raw mut p).cast(), (&raw const addr).cast(), core::mem::size_of::<usize>());
    p
}

fn bytes(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|v| v.to_be_bytes()).collect()
}

/// An accepted reply for `xid` carrying the int `r`.
fn reply(xid: u32, r: u32) -> Vec<u8> {
    bytes(&[xid, 1, 0, 0, 0, 0, r])
}

fn udp(ip: &str) -> UdpSocket {
    let s = UdpSocket::bind((ip, 0)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s
}

/// The xid of the next call datagram on `s`, and where it came from.
fn recv_call(s: &UdpSocket) -> (u32, SocketAddr) {
    let mut b = [0u8; 2000];
    let (_, from) = s.recv_from(&mut b).unwrap();
    (u32::from_be_bytes(b[..4].try_into().unwrap()), from)
}

/// A client for the fake server `s` (retry every 100 ms).
fn client(s: &UdpSocket) -> *mut clnt::Client {
    let mut a = [0u8; 16];
    a[..2].copy_from_slice(&2u16.to_ne_bytes());
    a[2..4].copy_from_slice(&s.local_addr().unwrap().port().to_be_bytes());
    a[4..8].copy_from_slice(&[127, 0, 0, 1]);
    let mut sock: c_int = -1;
    let c = clnt::liella_compat_clntudp_create(a.as_mut_ptr().cast::<c_void>(), PROG.into(), 1, Timeval { tv_sec: 0, tv_usec: 100_000 }, &mut sock);
    assert!(!c.is_null());
    c
}

/// One call with a total timeout of `ms`: status, result, `re_errno` slot, time taken.
fn call(c: *mut clnt::Client, ms: i64) -> (c_int, c_int, u64, Duration) {
    let (mut a, mut r): (c_int, c_int) = (5, -1);
    let t0 = Instant::now();
    let st = clnt::clnt_call(c, 1, xdr_int(), (&raw mut a).cast::<c_char>(), xdr_int(), (&raw mut r).cast::<c_char>(), Timeval { tv_sec: ms / 1000, tv_usec: (ms % 1000) * 1000 });
    let mut e = RpcErrC::default();
    clnt::clnt_geterr(c, &mut e);
    (st, r, e.a, t0.elapsed())
}

/// A fake server that answers the first call datagram with `act` (given the xid and the
/// client's address), on its own thread while the call runs here.
fn run(server: UdpSocket, c: *mut clnt::Client, ms: i64, act: impl FnOnce(&UdpSocket, u32, SocketAddr) + Send + 'static) -> ((c_int, c_int, u64, Duration), u32) {
    // The earlier calls' retries are still queued: drop them first.
    server.set_nonblocking(true).unwrap();
    while server.recv_from(&mut [0u8; 2000]).is_ok() {}
    server.set_nonblocking(false).unwrap();
    let h = std::thread::spawn(move || {
        let (xid, from) = recv_call(&server);
        act(&server, xid, from);
        xid
    });
    let r = call(c, ms);
    (r, h.join().unwrap())
}

fn set_flag(text: &[u8], secure: bool) {
    clnt::compat_from_start(&Tunables::parse(text, secure));
}

#[test]
fn udp_reply_source_rule_diagnosis_flag_and_xids() {
    // Trace: no spec clause (LG-15 ONC RPC client over UDP; RFC 5531 matches replies by xid only); D-29.2, COMPAT-12.
    set_flag(b"", false);
    // 1. A reply from another address and one from the right address but another port are
    //    dropped; the server's own reply then completes the call.
    let s = udp("127.0.0.1");
    let c = client(&s);
    let ((st, r, _, _), _) = run(s.try_clone().unwrap(), c, 3000, |s, xid, to| {
        udp("127.0.0.2").send_to(&reply(xid, 66), to).unwrap();
        udp("127.0.0.1").send_to(&reply(xid, 77), to).unwrap();
        std::thread::sleep(Duration::from_millis(50));
        s.send_to(&reply(xid, 7), to).unwrap();
    });
    assert_eq!((st, r), (0, 7), "wrong-address and wrong-port replies must be dropped, the server's reply taken");

    // 2. A stray with the call's xid: the call still waits the whole timeout, then CANTRECV
    //    with EREMCHG. 3. The same with another xid: the same result (no oracle).
    for (what, delta) in [("matching xid", 0u32), ("other xid", 0x5a5a)] {
        let ((st, _, errno, took), _) = run(s.try_clone().unwrap(), c, 600, move |_, xid, to| {
            udp("127.0.0.2").send_to(&reply(xid.wrapping_add(delta), 66), to).unwrap();
        });
        assert_eq!((st, errno), (CANTRECV, EREMCHG), "{what} from another address: RPC_CANTRECV with EREMCHG at the timeout");
        assert!(took >= Duration::from_millis(550), "{what}: a stray must not end the call early ({took:?})");
    }
    // 4. Junk that is not an RPC reply: dropped, plain RPC_TIMEDOUT.
    let ((st, _, _, took), _) = run(s.try_clone().unwrap(), c, 600, |_, xid, to| {
        let w = udp("127.0.0.2");
        w.send_to(&bytes(&[xid, 0, 2, PROG]), to).unwrap();
        w.send_to(&[1, 2, 3], to).unwrap();
    });
    assert_eq!(st, TIMEDOUT, "junk from another address leaves plain RPC_TIMEDOUT");
    assert!(took >= Duration::from_millis(550));

    // 5. The xids of successive calls do not follow from each other (D-29.2).
    let xids: Vec<u32> = (0..3)
        .map(|_| {
            run(s.try_clone().unwrap(), c, 3000, |s, xid, to| {
                s.send_to(&reply(xid, 1), to).unwrap();
            })
            .1
        })
        .collect();
    assert!(xids.windows(2).any(|w| clnt::next_xid(w[0]) != w[1] && w[0].wrapping_add(1) != w[1]), "xids must not step: {xids:x?}");

    // 6. COMPAT-12 set: the reference behaviour, a matching reply from anywhere is taken and a
    //    stray of another xid ends in plain RPC_TIMEDOUT.
    set_flag(b"liella.compat.rpc_reply_any_source=1", false);
    let ((st, r, _, _), _) = run(s.try_clone().unwrap(), c, 3000, |_, xid, to| {
        udp("127.0.0.2").send_to(&reply(xid, 66), to).unwrap();
    });
    assert_eq!((st, r), (0, 66), "with the flag a reply from another address is taken");
    let ((st, _, _, _), _) = run(s.try_clone().unwrap(), c, 600, |_, xid, to| {
        udp("127.0.0.2").send_to(&reply(xid ^ 1, 66), to).unwrap();
    });
    assert_eq!(st, TIMEDOUT, "with the flag there is no EREMCHG diagnosis");

    // 7. The flag in a secure (AT_SECURE) process is ignored: the default rule holds.
    set_flag(b"liella.compat.rpc_reply_any_source=1", true);
    let ((st, _, errno, _), _) = run(s.try_clone().unwrap(), c, 600, |_, xid, to| {
        udp("127.0.0.2").send_to(&reply(xid, 66), to).unwrap();
    });
    assert_eq!((st, errno), (CANTRECV, EREMCHG), "the flag must be ignored in a secure process");
    set_flag(b"", false);
    clnt::clnt_destroy(c);
}

/// The host glibc client against the same stray (black box), `PROG` and the port from argv.
const C_PROG: &str = r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <sys/time.h>
typedef int bool_t; typedef bool_t (*xdrproc_t)(void *, void *, ...);
struct CLIENT; typedef struct CLIENT CLIENT;
struct clnt_ops { int (*cl_call)(CLIENT *, unsigned long, xdrproc_t, char *, xdrproc_t, char *, struct timeval); };
struct CLIENT { void *cl_auth; struct clnt_ops *cl_ops; void *cl_private; };
__asm__(".symver clntudp_create,clntudp_create@GLIBC_2.2.5");
__asm__(".symver xdr_int,xdr_int@GLIBC_2.2.5");
CLIENT *clntudp_create(struct sockaddr_in *, unsigned long, unsigned long, struct timeval, int *);
bool_t xdr_int(void *, int *);
int main(int argc, char **argv) {
  struct sockaddr_in a; memset(&a, 0, sizeof a); a.sin_family = AF_INET;
  a.sin_port = htons(atoi(argv[1])); a.sin_addr.s_addr = htonl(0x7f000001);
  struct timeval w = {0, 100000}, t = {0, 600000}; int s = -1, x = 5, r = -1;
  CLIENT *c = clntudp_create(&a, PROG, 1, w, &s); if (!c) { puts("create failed"); return 1; }
  int st = c->cl_ops->cl_call(c, 1, (xdrproc_t)xdr_int, (char *)&x, (xdrproc_t)xdr_int, (char *)&r, t);
  printf("%d %d\n", st, r); return 0; }
"#;

#[test]
fn udp_reply_source_differs_from_host_glibc() {
    // Trace: no spec clause (LG-15; black-box host-glibc client); D-29.2 lists this difference.
    let dir = std::env::temp_dir().join(format!("liella-rpc-src-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("client.c");
    std::fs::write(&src, format!("#define PROG {PROG}u\n{C_PROG}")).unwrap();
    let exe = dir.join("client");
    assert!(std::process::Command::new("cc").arg("-w").arg(&src).arg("-o").arg(&exe).status().unwrap().success(), "host client did not build");
    let s = udp("127.0.0.1");
    let port = s.local_addr().unwrap().port();
    let h = std::thread::spawn(move || {
        let (xid, to) = recv_call(&s);
        udp("127.0.0.2").send_to(&reply(xid, 66), to).unwrap();
    });
    let out = std::process::Command::new(&exe).arg(port.to_string()).output().unwrap();
    h.join().unwrap();
    let host = String::from_utf8(out.stdout).unwrap();
    // Listed difference D-29.2: the reference takes the reply with the call's xid from another
    // address; Liella drops it (udp_reply_source_rule_diagnosis_flag_and_xids, case 2).
    assert_eq!(host.trim(), "0 66", "host glibc: a reply with the matching xid is accepted from any source address");
}
