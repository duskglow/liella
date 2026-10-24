// §29.1 ONC RPC client over TCP (LG-15, D-29.1); RFC 5531 record marking and wire format.
//! The Liella TCP client (`clnttcp_create`) against the Liella TCP server (`svc_run` in a
//! thread), against a server compiled with `cc` against the HOST glibc (black box), and against a
//! fake server that sends a reply one byte at a time in two fragments (a record mark split
//! across reads, short reads, the last-fragment bit), a reply for another xid first, a reply
//! whose last fragment ends before the results, no reply, and a close in mid-record. The results
//! match black-box probes of the host glibc client (handoff/rpc-client.md). The host-glibc
//! client against the Liella TCP server is s29_rpc_svc_tcp.rs.
#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::arithmetic_side_effects, clippy::indexing_slicing, unreachable_pub)]

use core::ffi::{c_char, c_int, c_void};
use std::io::{BufRead, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::fd::IntoRawFd;
use std::time::{Duration, Instant};

use liella_boundary::posix::s04_strings::memcpy;
use liella_boundary::posix::s29_legacy::{Xdr, XdrProc, liella_compat_xdr_int, liella_compat_xdr_void};
use liella_boundary::posix::s29_rpc_clnt as clnt;
use liella_boundary::posix::s29_rpc_svc as svc;
use clnt::Timeval;
use svc::{RpcErrC, SvcReq, SvcXprt};

const PROG: u32 = 0x2000_5679;

fn proc_of(addr: usize) -> XdrProc {
    let mut p: XdrProc = None;
    memcpy((&raw mut p).cast(), (&raw const addr).cast(), core::mem::size_of::<usize>());
    p
}

fn xdr_int() -> XdrProc {
    let f: extern "C" fn(*mut Xdr, *mut c_int) -> c_int = liella_compat_xdr_int;
    proc_of(f as usize)
}

fn xdr_void() -> XdrProc {
    let f: extern "C" fn() -> c_int = liella_compat_xdr_void;
    proc_of(f as usize)
}

fn rd_u64(p: *const u8, off: usize) -> u64 {
    let mut b = [0u8; 8];
    memcpy(b.as_mut_ptr().cast(), p.wrapping_add(off).cast(), 8);
    u64::from_ne_bytes(b)
}

extern "C" fn dispatch(req: *mut SvcReq, x: *mut SvcXprt) {
    match rd_u64(req.cast::<u8>().cast_const(), core::mem::offset_of!(SvcReq, rq_proc)) {
        1 => {
            let mut v: c_int = 41;
            svc::liella_compat_svc_sendreply(x, xdr_int(), (&raw mut v).cast::<c_char>());
        }
        99 => {
            svc::liella_compat_svc_sendreply(x, xdr_void(), core::ptr::null_mut());
            svc::liella_compat_svc_exit();
        }
        _ => svc::liella_compat_svcerr_noproc(x),
    }
}

fn client(port: u16) -> *mut clnt::Client {
    let mut a = [0u8; 16];
    a[..2].copy_from_slice(&2u16.to_ne_bytes());
    a[2..4].copy_from_slice(&port.to_be_bytes());
    a[4..8].copy_from_slice(&[127, 0, 0, 1]);
    let mut sock: c_int = -1;
    let c = clnt::liella_compat_clnttcp_create(a.as_mut_ptr().cast::<c_void>(), PROG.into(), 1, &mut sock, 0, 0);
    assert!(!c.is_null());
    assert!(sock > 2, "RPC_ANYSOCK: the socket is stored back");
    c
}

fn tv(ms: i64) -> Timeval {
    Timeval { tv_sec: ms / 1000, tv_usec: (ms % 1000) * 1000 }
}

fn int_call(c: *mut clnt::Client, proc_: u64, ms: i64) -> (c_int, c_int) {
    let (mut a, mut r) = (1, -1);
    (clnt::clnt_call(c, proc_, xdr_int(), (&raw mut a).cast(), xdr_int(), (&raw mut r).cast(), tv(ms)), r)
}

/// The same checks against either server; procedure 99 stops it.
fn check(port: u16, who: &str) {
    let c = client(port);
    assert_eq!(int_call(c, 1, 3000), (0, 41), "{who}: result");
    assert_eq!(int_call(c, 7, 3000).0, 10, "{who}: RPC_PROCUNAVAIL");
    assert_eq!(int_call(c, 1, 3000), (0, 41), "{who}: the stream is in step after an error reply");
    assert_eq!(clnt::clnt_call(c, 99, xdr_void(), core::ptr::null_mut(), xdr_void(), core::ptr::null_mut(), tv(3000)), 0, "{who}: stop");
    clnt::clnt_destroy(c);
}

const C_SERVER: &str = r#"
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
typedef int bool_t; typedef bool_t (*xdrproc_t)(void *, void *, ...);
struct svc_req { unsigned long rq_prog, rq_vers, rq_proc; };
typedef struct SVCXPRT { int xp_sock; unsigned short xp_port; } SVCXPRT;
__asm__(".symver svctcp_create,svctcp_create@GLIBC_2.2.5");
__asm__(".symver svc_register,svc_register@GLIBC_2.2.5");
__asm__(".symver svc_run,svc_run@GLIBC_2.2.5");
__asm__(".symver svc_sendreply,svc_sendreply@GLIBC_2.2.5");
__asm__(".symver svcerr_noproc,svcerr_noproc@GLIBC_2.2.5");
__asm__(".symver xdr_int,xdr_int@GLIBC_2.2.5");
__asm__(".symver xdr_void,xdr_void@GLIBC_2.2.5");
SVCXPRT *svctcp_create(int, unsigned, unsigned); bool_t svc_register(SVCXPRT *, unsigned long, unsigned long, void (*)(struct svc_req *, SVCXPRT *), unsigned long);
void svc_run(void); bool_t svc_sendreply(SVCXPRT *, xdrproc_t, char *); void svcerr_noproc(SVCXPRT *);
bool_t xdr_int(void *, int *); bool_t xdr_void(void);
static void disp(struct svc_req *r, SVCXPRT *x) {
  int v = 41;
  if (r->rq_proc == 1) svc_sendreply(x, (xdrproc_t)xdr_int, (char *)&v);
  else if (r->rq_proc == 99) { svc_sendreply(x, (xdrproc_t)xdr_void, 0); exit(0); }
  else svcerr_noproc(x); }
int main(void) {
  SVCXPRT *x = svctcp_create(-1, 0, 0); if (!x) return 1;
  if (!svc_register(x, PROG, 1, disp, 0)) return 2;
  printf("%u\n", x->xp_port); fflush(stdout); alarm(30); svc_run(); return 3; }
"#;

#[test]
fn tcp_client_against_liella_and_host_servers() {
    // Trace: no spec clause (LG-15 clnttcp_create interop; RFC 5531, black-box host-glibc server); D-29.1.
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let x = svc::liella_compat_svctcp_create(l.into_raw_fd(), 0, 0);
    assert!(!x.is_null());
    assert_eq!(svc::liella_compat_svc_register(x, PROG.into(), 1, Some(dispatch), 0), 1);
    let server = std::thread::spawn(|| svc::liella_compat_svc_run());
    check(port, "Liella server");
    server.join().unwrap();
    svc::liella_compat_svc_unregister(PROG.into(), 1);

    let dir = std::env::temp_dir().join(format!("liella-rpc-clnt-tcp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let src = dir.join("server.c");
    std::fs::write(&src, format!("#define PROG {PROG}u\n{C_SERVER}")).unwrap();
    let exe = dir.join("server");
    assert!(std::process::Command::new("cc").arg("-w").arg(&src).arg("-o").arg(&exe).status().unwrap().success(), "host server did not build");
    let mut child = std::process::Command::new(&exe).stdout(std::process::Stdio::piped()).spawn().unwrap();
    let mut line = String::new();
    std::io::BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
    check(line.trim().parse().unwrap(), "host-glibc server");
    assert!(child.wait().unwrap().success(), "the host server exits 0 after procedure 99");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Reads one call record (one fragment, as the client sends it); returns (xid bytes, proc).
fn call_of(s: &mut TcpStream) -> Option<([u8; 4], u32)> {
    let mut h = [0u8; 4];
    s.read_exact(&mut h).ok()?;
    let mut b = vec![0u8; (u32::from_be_bytes(h) & 0x7fff_ffff) as usize];
    s.read_exact(&mut b).ok()?;
    Some((b[..4].try_into().unwrap(), u32::from_be_bytes(b[20..24].try_into().unwrap())))
}

fn ok_reply(xid: [u8; 4], v: Option<u32>) -> Vec<u8> {
    let mut w = xid.to_vec();
    for x in [1u32, 0, 0, 0, 0] {
        w.extend_from_slice(&x.to_be_bytes());
    }
    if let Some(v) = v {
        w.extend_from_slice(&v.to_be_bytes());
    }
    w
}

fn mark(len: usize, last: bool) -> [u8; 4] {
    (len as u32 | if last { 0x8000_0000 } else { 0 }).to_be_bytes()
}

/// One connection of the fake server; the call's procedure picks the case.
fn fake(mut s: TcpStream) {
    let Some((xid, p)) = call_of(&mut s) else { return };
    match p {
        // A reply for another xid, then the right one sent byte by byte in two fragments:
        // the record marks are split across reads and every read is short.
        1 => {
            let mut other = xid;
            other[0] ^= 1;
            let r = ok_reply(other, Some(99));
            s.write_all(&mark(r.len(), true)).unwrap();
            s.write_all(&r).unwrap();
            let r = ok_reply(xid, Some(55));
            let mut o = mark(10, false).to_vec();
            o.extend_from_slice(&r[..10]);
            o.extend_from_slice(&mark(r.len() - 10, true));
            o.extend_from_slice(&r[10..]);
            for b in o {
                s.write_all(&[b]).unwrap();
                s.flush().unwrap();
                std::thread::sleep(Duration::from_millis(2));
            }
        }
        // The last fragment ends before the results.
        2 => {
            let r = ok_reply(xid, None);
            s.write_all(&mark(r.len(), true)).unwrap();
            s.write_all(&r).unwrap();
        }
        // A non-last fragment, then the connection closes.
        3 => {
            let r = ok_reply(xid, Some(5));
            s.write_all(&mark(r.len(), false)).unwrap();
            s.write_all(&r).unwrap();
            return;
        }
        // A mark promising more than is sent, then the connection closes.
        4 => {
            s.write_all(&mark(100, true)).unwrap();
            s.write_all(&xid).unwrap();
            return;
        }
        // No reply at all (6), or a call after a batched one (7).
        7 => {
            if let Some((xid2, _)) = call_of(&mut s) {
                let r = ok_reply(xid2, Some(70));
                s.write_all(&mark(r.len(), true)).unwrap();
                s.write_all(&r).unwrap();
            }
        }
        _ => {}
    }
    std::thread::sleep(Duration::from_secs(2));
}

#[test]
fn tcp_client_records_timeouts_and_truncation() {
    // Trace: no spec clause (LG-15 clnttcp_create: RFC 5531 record marking; results as probed on the host glibc client); D-29.1.
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for s in l.incoming() {
            let s = s.unwrap();
            std::thread::spawn(move || fake(s));
        }
    });
    let one = |p: u64, ms: i64| {
        let c = client(port);
        let t = Instant::now();
        let (st, r) = int_call(c, p, ms);
        let mut e = RpcErrC::default();
        clnt::clnt_geterr(c, &mut e);
        clnt::clnt_destroy(c);
        (st, r, e.a, t.elapsed())
    };
    let (st, r, _, _) = one(1, 3000);
    assert_eq!((st, r), (0, 55), "a reply for another xid is skipped; the split record joins");
    assert_eq!(one(2, 3000).0, 2, "a record ending before the results is RPC_CANTDECODERES");
    let (st, _, a, _) = one(3, 3000);
    assert_eq!((st, a), (4, 104), "end of input in mid-record: RPC_CANTRECV, ECONNRESET (host glibc probe)");
    let (st, _, a, _) = one(4, 3000);
    assert_eq!((st, a), (4, 104), "a short record then end of input: RPC_CANTRECV, ECONNRESET");
    let (st, _, _, el) = one(6, 300);
    assert_eq!(st, 5, "no reply: RPC_TIMEDOUT");
    assert!(el >= Duration::from_millis(290) && el < Duration::from_millis(1500), "timed out after the call's timeout: {el:?}");

    let c = client(port);
    let set = tv(200);
    assert_eq!(clnt::clnt_control(c, 1, (&raw const set).cast_mut().cast()), 1, "CLSET_TIMEOUT");
    let t = Instant::now();
    assert_eq!(int_call(c, 6, 5000).0, 5, "CLSET_TIMEOUT overrides the call's timeout");
    assert!(t.elapsed() < Duration::from_millis(1500));
    let mut g = tv(7);
    assert_eq!(clnt::clnt_control(c, 4, (&raw mut g).cast()), 0, "CLSET_RETRY_TIMEOUT is UDP's (probed: 0)");
    let mut fd: c_int = -1;
    assert_eq!(clnt::clnt_control(c, 6, (&raw mut fd).cast()), 1);
    assert_eq!(clnt::clnt_control(c, 9, core::ptr::null_mut()), 1, "CLSET_FD_NCLOSE");
    clnt::clnt_destroy(c);
    assert_eq!(liella_boundary::posix::s11_lowio::close(fd), 0, "CLSET_FD_NCLOSE keeps the socket open");

    let c = client(port);
    let mut g = tv(7);
    assert_eq!(clnt::clnt_control(c, 2, (&raw mut g).cast()), 1, "CLGET_TIMEOUT with none set");
    assert_eq!((g.tv_sec, g.tv_usec), (0, 7000), "and nothing written (host glibc probe)");
    let mut a = 1;
    assert_eq!(clnt::clnt_call(c, 7, xdr_int(), (&raw mut a).cast(), None, core::ptr::null_mut(), tv(0)), 0, "a batched call");
    assert_eq!(int_call(c, 70, 3000), (0, 70), "the next call flushes the batch and gets its reply");
    clnt::clnt_destroy(c);
}
