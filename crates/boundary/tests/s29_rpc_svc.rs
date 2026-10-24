// §29.1 ONC RPC server, UDP transport (LG-14, D-29.1); RFC 5531 wire format.
//! One #[test]: the service registry and `svc_run` are process-wide, so the cases run in order.
//! A server thread runs `svc_run`; a raw `UdpSocket` checks replies byte for byte, then a client
//! compiled with `cc` against the HOST glibc (black box) calls the same server.
#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::arithmetic_side_effects, clippy::indexing_slicing, unreachable_pub)]

use core::ffi::{c_char, c_int, c_void};
use core::mem::offset_of;
use std::net::UdpSocket;
use std::os::fd::IntoRawFd;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use liella_boundary::posix::s29_legacy::{Xdr, XdrProc, liella_compat_xdr_int, liella_compat_xdr_void};
use liella_boundary::posix::s29_rpc_svc as svc;
use liella_boundary::posix::s04_strings::memcpy;
use svc::{CredArea, SvcReq, SvcXprt, XpOps};

const PROG: u32 = 0x2000_1234;
static SEEN_UID: AtomicU32 = AtomicU32::new(u32::MAX);

/// Reads `N` bytes at `p + off` through the memcpy export (this file contains no `unsafe`).
fn rd<const N: usize>(p: *const u8, off: usize) -> [u8; N] {
    let mut b = [0u8; N];
    memcpy(b.as_mut_ptr().cast(), p.wrapping_add(off).cast(), N);
    b
}

fn rd_u64(p: *const u8, off: usize) -> u64 {
    u64::from_ne_bytes(rd(p, off))
}

fn rd_u32(p: *const u8, off: usize) -> u32 {
    u32::from_ne_bytes(rd(p, off))
}

/// An `XdrProc` made from a function's address: the pointer bytes are copied into the Option.
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

type GetArgs = extern "C" fn(*mut SvcXprt, XdrProc, *mut c_void) -> c_int;

/// The transport's `xp_getargs` entry, read out of its ops table.
fn getargs(x: *mut SvcXprt) -> GetArgs {
    let ops = rd_u64(x.cast::<u8>().cast_const(), offset_of!(SvcXprt, xp_ops)) as usize;
    let mut f: Option<GetArgs> = None;
    memcpy((&raw mut f).cast(), (ops + offset_of!(XpOps, xp_getargs)) as *const c_void, core::mem::size_of::<usize>());
    f.unwrap()
}

extern "C" fn dispatch(req: *mut SvcReq, x: *mut SvcXprt) {
    let rq = req.cast::<u8>().cast_const();
    match rd_u64(rq, offset_of!(SvcReq, rq_proc)) {
        0 => drop(svc::liella_compat_svc_sendreply(x, xdr_void(), core::ptr::null_mut())),
        1 => {
            let mut v: c_int = 0;
            if getargs(x)(x, xdr_int(), (&raw mut v).cast()) == 0 {
                svc::liella_compat_svcerr_decode(x);
                return;
            }
            v += 1;
            svc::liella_compat_svc_sendreply(x, xdr_int(), (&raw mut v).cast::<c_char>());
        }
        2 => svc::liella_compat_svcerr_systemerr(x),
        3 => {
            if rd_u32(rq, offset_of!(SvcReq, rq_cred.oa_flavor)) == 1 {
                // For AUTH_UNIX, rq_clntcred points at the decoded authunix_parms area.
                let cred = rd_u64(rq, offset_of!(SvcReq, rq_clntcred)) as usize as *const u8;
                SEEN_UID.store(rd_u32(cred, offset_of!(CredArea, aup_uid)), Ordering::SeqCst);
            }
            svc::liella_compat_svc_sendreply(x, xdr_void(), core::ptr::null_mut());
        }
        99 => {
            // Reply first: svc_exit destroys every transport.
            svc::liella_compat_svc_sendreply(x, xdr_void(), core::ptr::null_mut());
            svc::liella_compat_svc_exit();
        }
        _ => svc::liella_compat_svcerr_noproc(x),
    }
}

fn words(w: &[u32]) -> Vec<u8> {
    w.iter().flat_map(|v| v.to_be_bytes()).collect()
}

fn call(xid: u32, rpcvers: u32, prog: u32, vers: u32, proc_: u32, cred: &[u32], args: &[u32]) -> Vec<u8> {
    let mut w = vec![xid, 0, rpcvers, prog, vers, proc_];
    w.extend_from_slice(cred);
    w.extend_from_slice(&[0, 0]);
    w.extend_from_slice(args);
    words(&w)
}

fn ask(c: &UdpSocket, msg: &[u8]) -> Option<Vec<u32>> {
    c.send(msg).unwrap();
    let mut b = [0u8; 9000];
    let n = c.recv(&mut b).ok()?;
    Some(b[..n].chunks(4).map(|q| u32::from_be_bytes(q.try_into().unwrap())).collect())
}

const NONE: [u32; 2] = [0, 0];
const C_PROG: &str = r#"
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <netinet/in.h>
#include <arpa/inet.h>
#include <sys/time.h>
typedef int bool_t; typedef bool_t (*xdrproc_t)(void *, void *, ...);
struct rpc_err { int re_status; union { int RE_errno; int RE_why; struct { unsigned long low, high; } RE_vers; struct { long s1, s2; } RE_lb; } ru; };
typedef struct CLIENT CLIENT;
struct clnt_ops { int (*cl_call)(CLIENT *, unsigned long, xdrproc_t, char *, xdrproc_t, char *, struct timeval);
  void (*cl_abort)(CLIENT *); void (*cl_geterr)(CLIENT *, struct rpc_err *); bool_t (*cl_freeres)(CLIENT *, xdrproc_t, char *);
  void (*cl_destroy)(CLIENT *); bool_t (*cl_control)(CLIENT *, int, char *); };
struct CLIENT { void *cl_auth; struct clnt_ops *cl_ops; void *cl_private; };
__asm__(".symver clntudp_create,clntudp_create@GLIBC_2.2.5");
__asm__(".symver xdr_int,xdr_int@GLIBC_2.2.5");
__asm__(".symver xdr_void,xdr_void@GLIBC_2.2.5");
__asm__(".symver authunix_create_default,authunix_create_default@GLIBC_2.2.5");
CLIENT *clntudp_create(struct sockaddr_in *, unsigned long, unsigned long, struct timeval, int *);
bool_t xdr_int(void *, int *); bool_t xdr_void(void); void *authunix_create_default(void);
static CLIENT *mk(int port, unsigned long vers) {
  struct sockaddr_in a; memset(&a, 0, sizeof a); a.sin_family = AF_INET; a.sin_port = htons(port);
  a.sin_addr.s_addr = htonl(INADDR_LOOPBACK); int s = -1; struct timeval w = {1, 0};
  CLIENT *c = clntudp_create(&a, PROG, vers, w, &s); if (!c) { puts("create failed"); exit(1); } return c; }
int main(int argc, char **argv) {
  int port = atoi(argv[1]); struct timeval t = {3, 0}; int in = 41, out = 0; struct rpc_err e;
  CLIENT *c = mk(port, 1);
  int st = c->cl_ops->cl_call(c, 1, (xdrproc_t)xdr_int, (char *)&in, (xdrproc_t)xdr_int, (char *)&out, t);
  printf("int %d %d\n", st, out);
  printf("noproc %d\n", c->cl_ops->cl_call(c, 7, (xdrproc_t)xdr_void, 0, (xdrproc_t)xdr_void, 0, t));
  printf("garbage %d\n", c->cl_ops->cl_call(c, 1, (xdrproc_t)xdr_void, 0, (xdrproc_t)xdr_void, 0, t));
  printf("syserr %d\n", c->cl_ops->cl_call(c, 2, (xdrproc_t)xdr_void, 0, (xdrproc_t)xdr_void, 0, t));
  c->cl_auth = authunix_create_default();
  printf("unix %d %u\n", c->cl_ops->cl_call(c, 3, (xdrproc_t)xdr_void, 0, (xdrproc_t)xdr_void, 0, t), (unsigned)getuid());
  CLIENT *v = mk(port, 2);
  st = v->cl_ops->cl_call(v, 0, (xdrproc_t)xdr_void, 0, (xdrproc_t)xdr_void, 0, t); v->cl_ops->cl_geterr(v, &e);
  printf("vers %d %lu %lu\n", st, e.ru.RE_vers.low, e.ru.RE_vers.high);
  return 0; }
"#;

fn host_client(port: u16) -> Vec<String> {
    let dir = std::env::temp_dir().join(format!("liella-rpc-svc-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let c = dir.join("client.c");
    std::fs::write(&c, format!("#define PROG {PROG}u\n{C_PROG}")).unwrap();
    let exe = dir.join("client");
    let st = std::process::Command::new("cc").arg("-w").arg(&c).arg("-o").arg(&exe).status().unwrap();
    assert!(st.success(), "host client did not build");
    let out = std::process::Command::new(&exe).arg(port.to_string()).output().unwrap();
    String::from_utf8(out.stdout).unwrap().lines().map(String::from).collect()
}

#[test]
fn svc_udp_round_trip_errors_and_host_client() {
    // Trace: no spec clause (LG-14 ONC RPC server over UDP; RFC 5531 wire format, black-box host-glibc client); D-29.1.
    assert_eq!(core::mem::size_of::<SvcXprt>(), 336);
    assert_eq!(core::mem::size_of::<SvcReq>(), 64);
    let s = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = s.local_addr().unwrap().port();
    let x = svc::liella_compat_svcudp_create(s.into_raw_fd());
    assert!(!x.is_null());
    assert_eq!(u16::from_ne_bytes(rd(x.cast::<u8>().cast_const(), offset_of!(SvcXprt, xp_port))), port);
    assert_eq!(svc::liella_compat_svc_register(x, PROG.into(), 1, Some(dispatch), 0), 1);
    assert_eq!(svc::liella_compat_svc_register(x, PROG.into(), 3, Some(dispatch), 0), 1);
    let server = std::thread::spawn(|| svc::liella_compat_svc_run());

    let c = UdpSocket::bind("127.0.0.1:0").unwrap();
    c.connect(("127.0.0.1", port)).unwrap();
    c.set_read_timeout(Some(Duration::from_millis(400))).unwrap();
    let ok = |xid: u32, rest: &[u32]| [&[xid, 1, 0, 0, 0][..], rest].concat();
    // Accepted replies, byte for byte.
    assert_eq!(ask(&c, &call(7, 2, PROG, 1, 1, &NONE, &[41])), Some(ok(7, &[0, 42])));
    assert_eq!(ask(&c, &call(8, 2, PROG + 1, 1, 1, &NONE, &[41])), Some(ok(8, &[1])));
    assert_eq!(ask(&c, &call(9, 2, PROG, 2, 1, &NONE, &[41])), Some(ok(9, &[2, 1, 3])));
    assert_eq!(ask(&c, &call(10, 2, PROG, 1, 7, &NONE, &[])), Some(ok(10, &[3])));
    assert_eq!(ask(&c, &call(11, 2, PROG, 1, 1, &NONE, &[])), Some(ok(11, &[4])));
    assert_eq!(ask(&c, &call(12, 2, PROG, 1, 2, &NONE, &[])), Some(ok(12, &[5])));
    // Denied: rpcvers 3 -> RPC_MISMATCH 2..2; unknown credential flavour -> AUTH_ERROR.
    assert_eq!(ask(&c, &call(13, 3, PROG, 1, 0, &NONE, &[])), Some(vec![13, 1, 1, 0, 2, 2]));
    let r = ask(&c, &call(14, 2, PROG, 1, 0, &[77, 0], &[])).unwrap();
    assert_eq!((r[..4].to_vec(), r.len()), (vec![14, 1, 1, 1], 5), "unknown credential flavour must be refused with AUTH_ERROR");
    // AUTH_UNIX: stamp, machine "h", uid 4242, gid 7, no groups.
    let unix = [1, 24, 5, 1, u32::from_be_bytes(*b"h\0\0\0"), 4242, 7, 0];
    assert_eq!(ask(&c, &call(15, 2, PROG, 1, 3, &unix, &[])), Some(ok(15, &[0])));
    assert_eq!(SEEN_UID.swap(u32::MAX, Ordering::SeqCst), 4242);
    // Malformed calls get no reply and do not stop the server.
    let full = call(16, 2, PROG, 1, 1, &NONE, &[41]);
    for n in [0, 3, 4, 8, 20, 36] {
        assert_eq!(ask(&c, &full[..n]), None, "truncated to {n}");
    }
    assert_eq!(ask(&c, &words(&[17, 1, 0, 0, 0, 0])), None, "a REPLY is not a call");
    assert_eq!(ask(&c, &call(18, 2, PROG, 1, 1, &[1, 0xFFFF_FFF0], &[])), None, "absurd credential length");
    assert_eq!(ask(&c, &call(19, 2, PROG, 1, 1, &[1, 404], &[0; 101])), None, "credential over 400 bytes");
    assert_eq!(ask(&c, &call(20, 2, PROG, 1, 1, &NONE, &[41])), Some(ok(20, &[0, 42])));

    // Interop: a client built against the host glibc (RPC_PROGUNAVAIL 8 .. RPC_SYSTEMERROR 12).
    let lines = host_client(port);
    assert_eq!(lines, ["int 0 42", "noproc 10", "garbage 11", "syserr 12", &format!("unix 0 {}", nix_uid()), "vers 9 1 3"]);
    assert_eq!(SEEN_UID.load(Ordering::SeqCst), nix_uid(), "the host client's AUTH_UNIX uid");

    assert_eq!(ask(&c, &call(21, 2, PROG, 1, 99, &NONE, &[])), Some(ok(21, &[0])));
    server.join().unwrap();
    svc::liella_compat_svc_unregister(PROG.into(), 1);
    svc::liella_compat_svc_unregister(PROG.into(), 3);
}

fn nix_uid() -> u32 {
    let o = std::process::Command::new("id").arg("-u").output().unwrap();
    String::from_utf8(o.stdout).unwrap().trim().parse().unwrap()
}

#[test]
fn authenticate_entry_bounds_the_credential_length() {
    // Trace: no spec clause (LG-14 `_authenticate` called directly by C code; RFC 5531 caps an opaque_auth body at 400 bytes); D-29.1.
    let mut body = [0u8; 404];
    let mut msg = svc::RpcMsg::default();
    msg.w[6] = 0; // AUTH_NONE
    msg.w[7] = body.as_mut_ptr() as usize as u64;
    let mut req = SvcReq { rq_prog: 0, rq_vers: 0, rq_proc: 0, rq_cred: svc::OpaqueAuth { oa_flavor: 0, oa_base: core::ptr::null_mut(), oa_length: 0 }, rq_clntcred: core::ptr::null_mut(), rq_xprt: core::ptr::null_mut() };
    msg.w[8] = 400;
    assert_eq!(svc::liella_compat__authenticate(&mut req, &msg), 0, "a 400-byte AUTH_NONE body is within the bound");
    msg.w[8] = 404;
    assert_eq!(svc::liella_compat__authenticate(&mut req, &msg), 1, "credential over 400 bytes must be AUTH_BADCRED before the body is read");
}
