// §29.1 ONC RPC server, stream transports (LG-14, D-29.1); RFC 5531 wire format and record marking.
//! One #[test]: the service registry and `svc_run` are process-wide, so the cases run in order
//! (and in their own test binary, apart from the UDP cases). First `svc_getreqset` is driven by
//! hand, then a server thread runs `svc_run`; raw sockets check the framed replies byte for byte,
//! then a client compiled with `cc` against the HOST glibc (black box) calls the same server.
#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::arithmetic_side_effects, clippy::indexing_slicing, unreachable_pub)]

use core::ffi::{c_char, c_int, c_uint, c_void};
use core::mem::offset_of;
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::os::fd::IntoRawFd;
use std::os::unix::net::UnixStream;
use std::time::Duration;

use liella_boundary::posix::s04_strings::memcpy;
use liella_boundary::posix::s29_legacy::{Xdr, XdrProc, liella_compat_xdr_bytes, liella_compat_xdr_int, liella_compat_xdr_void};
use liella_boundary::posix::s29_rpc_svc as svc;
use svc::{SvcReq, SvcXprt, XpOps};

const PROG: u32 = 0x2000_4321;
const BLOB: usize = 30_000;

/// Reads `N` bytes at `p + off` through the memcpy export (this file contains no `unsafe`).
fn rd<const N: usize>(p: *const u8, off: usize) -> [u8; N] {
    let mut b = [0u8; N];
    memcpy(b.as_mut_ptr().cast(), p.wrapping_add(off).cast(), N);
    b
}

fn rd_u64(p: *const u8, off: usize) -> u64 {
    u64::from_ne_bytes(rd(p, off))
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

/// `struct { char *p; u_int n; }` through `xdr_bytes` (at most 65536 bytes).
extern "C" fn xdr_blob(x: *mut Xdr, o: *mut u64) -> c_int {
    liella_compat_xdr_bytes(x, o.cast::<*mut c_char>(), o.wrapping_add(1).cast::<c_uint>(), 65_536)
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
        4 => {
            // Replies with the blob's length when every byte i is i % 251, else -1.
            let mut blob = [0u64; 2];
            let f: extern "C" fn(*mut Xdr, *mut u64) -> c_int = xdr_blob;
            if getargs(x)(x, proc_of(f as usize), blob.as_mut_ptr().cast()) == 0 {
                svc::liella_compat_svcerr_decode(x);
                return;
            }
            let n = (blob[1] & 0xFFFF_FFFF) as usize;
            let mut v = vec![0u8; n];
            memcpy(v.as_mut_ptr().cast(), blob[0] as usize as *const c_void, n);
            let mut r: c_int = if v.iter().enumerate().all(|(i, b)| usize::from(*b) == i % 251) { n as c_int } else { -1 };
            svc::liella_compat_svc_sendreply(x, xdr_int(), (&raw mut r).cast::<c_char>());
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

fn call(xid: u32, proc_: u32, cred: &[u32], args: &[u32]) -> Vec<u8> {
    let mut w = vec![xid, 0, 2, PROG, 1, proc_];
    w.extend_from_slice(cred);
    w.extend_from_slice(&[0, 0]);
    w.extend_from_slice(args);
    words(&w)
}

/// One record: each piece a fragment, the last one flagged.
fn rec(frags: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, f) in frags.iter().enumerate() {
        let last = if i + 1 == frags.len() { 0x8000_0000 } else { 0 };
        out.extend_from_slice(&(last | f.len() as u32).to_be_bytes());
        out.extend_from_slice(f);
    }
    out
}

/// Reads one reply record (any number of fragments) as XDR words.
fn reply(s: &mut impl Read) -> Option<Vec<u32>> {
    let mut out = Vec::new();
    loop {
        let mut h = [0u8; 4];
        s.read_exact(&mut h).ok()?;
        let h = u32::from_be_bytes(h);
        let mut b = vec![0u8; (h & 0x7FFF_FFFF) as usize];
        s.read_exact(&mut b).ok()?;
        out.extend(b);
        if h & 0x8000_0000 != 0 {
            return Some(out.chunks(4).map(|q| u32::from_be_bytes(q.try_into().unwrap())).collect());
        }
    }
}

/// The server closed the connection (end of input, no reply bytes first).
fn closed(s: &mut impl Read) -> bool {
    let mut b = [0u8; 1];
    matches!(s.read(&mut b), Ok(0))
}

fn ok(xid: u32, rest: &[u32]) -> Vec<u32> {
    [&[xid, 1, 0, 0, 0][..], rest].concat()
}

fn connect(port: u16) -> TcpStream {
    let s = TcpStream::connect(("127.0.0.1", port)).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.set_nodelay(true).unwrap();
    s
}

/// The registered-descriptor set (`svc_fdset`).
fn fdset() -> [u64; 16] {
    let b: [u8; 128] = rd(svc::liella_compat___rpc_thread_svc_fdset().cast::<u8>().cast_const(), 0);
    core::array::from_fn(|i| u64::from_ne_bytes(b[i * 8..i * 8 + 8].try_into().unwrap()))
}

fn has(set: &[u64; 16], fd: i32) -> bool {
    set[fd as usize / 64] & (1 << (fd % 64)) != 0
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
__asm__(".symver clnttcp_create,clnttcp_create@GLIBC_2.2.5");
__asm__(".symver xdr_int,xdr_int@GLIBC_2.2.5");
__asm__(".symver xdr_void,xdr_void@GLIBC_2.2.5");
__asm__(".symver xdr_bytes,xdr_bytes@GLIBC_2.2.5");
__asm__(".symver authunix_create_default,authunix_create_default@GLIBC_2.2.5");
CLIENT *clnttcp_create(struct sockaddr_in *, unsigned long, unsigned long, int *, unsigned, unsigned);
bool_t xdr_int(void *, int *); bool_t xdr_void(void); void *authunix_create_default(void);
bool_t xdr_bytes(void *, char **, unsigned *, unsigned);
struct blob { char *p; unsigned n; };
static bool_t xdr_blob(void *x, struct blob *b) { return xdr_bytes(x, &b->p, &b->n, 65536); }
static CLIENT *mk(int port, unsigned long vers, unsigned sendsz) {
  struct sockaddr_in a; memset(&a, 0, sizeof a); a.sin_family = AF_INET; a.sin_port = htons(port);
  a.sin_addr.s_addr = htonl(INADDR_LOOPBACK); int s = -1;
  CLIENT *c = clnttcp_create(&a, PROG, vers, &s, sendsz, 0); if (!c) { puts("create failed"); exit(1); } return c; }
int main(int argc, char **argv) {
  int port = atoi(argv[1]); struct timeval t = {5, 0}; int in = 41, out = 0; struct rpc_err e;
  CLIENT *c = mk(port, 1, 0);
  int st = c->cl_ops->cl_call(c, 1, (xdrproc_t)xdr_int, (char *)&in, (xdrproc_t)xdr_int, (char *)&out, t);
  printf("int %d %d\n", st, out);
  printf("noproc %d\n", c->cl_ops->cl_call(c, 7, (xdrproc_t)xdr_void, 0, (xdrproc_t)xdr_void, 0, t));
  printf("garbage %d\n", c->cl_ops->cl_call(c, 1, (xdrproc_t)xdr_void, 0, (xdrproc_t)xdr_void, 0, t));
  printf("syserr %d\n", c->cl_ops->cl_call(c, 2, (xdrproc_t)xdr_void, 0, (xdrproc_t)xdr_void, 0, t));
  c->cl_auth = authunix_create_default();
  printf("unix %d\n", c->cl_ops->cl_call(c, 0, (xdrproc_t)xdr_void, 0, (xdrproc_t)xdr_void, 0, t));
  CLIENT *b = mk(port, 1, 1000);
  struct blob bl; bl.n = BLOB; bl.p = malloc(BLOB); for (unsigned i = 0; i < BLOB; i++) bl.p[i] = (char)(i % 251);
  out = 0; st = b->cl_ops->cl_call(b, 4, (xdrproc_t)xdr_blob, (char *)&bl, (xdrproc_t)xdr_int, (char *)&out, t);
  printf("blob %d %d\n", st, out);
  CLIENT *v = mk(port, 2, 0);
  st = v->cl_ops->cl_call(v, 0, (xdrproc_t)xdr_void, 0, (xdrproc_t)xdr_void, 0, t); v->cl_ops->cl_geterr(v, &e);
  printf("vers %d %lu %lu\n", st, e.ru.RE_vers.low, e.ru.RE_vers.high);
  return 0; }
"#;

fn host_client(port: u16) -> Vec<String> {
    let dir = std::env::temp_dir().join(format!("liella-rpc-svc-tcp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let c = dir.join("client.c");
    std::fs::write(&c, format!("#define PROG {PROG}u\n#define BLOB {BLOB}u\n{C_PROG}")).unwrap();
    let exe = dir.join("client");
    let st = std::process::Command::new("cc").arg("-w").arg(&c).arg("-o").arg(&exe).status().unwrap();
    assert!(st.success(), "host client did not build");
    let out = std::process::Command::new(&exe).arg(port.to_string()).output().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    String::from_utf8(out.stdout).unwrap().lines().map(String::from).collect()
}

#[test]
fn svc_tcp_streamed_records_and_host_client() {
    // Trace: no spec clause (LG-14 ONC RPC server over TCP, records streamed through xdrrec; RFC 5531 record marking, black-box host-glibc client); D-29.1.
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = l.local_addr().unwrap().port();
    let lfd = l.into_raw_fd();
    let x = svc::liella_compat_svctcp_create(lfd, 0, 0);
    assert!(!x.is_null());
    assert_eq!(u16::from_ne_bytes(rd(x.cast::<u8>().cast_const(), offset_of!(SvcXprt, xp_port))), port);
    // Protocol 0: no portmapper registration (none runs here).
    assert_eq!(svc::liella_compat_svc_register(x, PROG.into(), 1, Some(dispatch), 0), 1);
    assert_eq!(svc::liella_compat_svc_register(x, PROG.into(), 3, Some(dispatch), 0), 1);
    assert!(has(&fdset(), lfd), "svctcp_create registers its socket");

    // svc_getreqset by hand: the listener's bit accepts, the new connection's bit serves the call.
    let mut c = connect(port);
    c.write_all(&rec(&[&call(1, 1, &NONE, &[41])])).unwrap();
    let mut set = [0u64; 16];
    set[lfd as usize / 64] |= 1 << (lfd % 64);
    svc::liella_compat_svc_getreqset(set.as_ptr());
    let mut conn = fdset();
    conn[lfd as usize / 64] &= !(1 << (lfd % 64));
    assert_eq!(conn.iter().map(|w| w.count_ones()).sum::<u32>(), 1, "accept registers one connection");
    svc::liella_compat_svc_getreqset(conn.as_ptr());
    let mut raw = [0u8; 32];
    c.read_exact(&mut raw).unwrap();
    assert_eq!(raw.to_vec(), [&0x8000_001C_u32.to_be_bytes()[..], &words(&ok(1, &[0, 42]))].concat(), "one framed reply, byte for byte");

    // svcfd_create over a stream pair; xprt_unregister / xprt_register move its descriptor.
    let (a, mut u) = UnixStream::pair().unwrap();
    u.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    let ufd = a.into_raw_fd();
    let y = svc::liella_compat_svcfd_create(ufd, 0, 0);
    assert!(!y.is_null());
    assert!(has(&fdset(), ufd));
    svc::liella_compat_xprt_unregister(y);
    assert!(!has(&fdset(), ufd), "xprt_unregister clears the descriptor");
    svc::liella_compat_xprt_register(y);
    assert!(has(&fdset(), ufd), "xprt_register sets it again");

    let server = std::thread::spawn(|| svc::liella_compat_svc_run());

    // The same connection again, now through svc_run; then the stream-pair transport.
    c.write_all(&rec(&[&call(2, 1, &NONE, &[1])])).unwrap();
    assert_eq!(reply(&mut c), Some(ok(2, &[0, 2])));
    u.write_all(&rec(&[&call(3, 1, &NONE, &[9])])).unwrap();
    assert_eq!(reply(&mut u), Some(ok(3, &[0, 10])));

    // A call split over three fragments (cut off the word boundaries), written a byte at a time.
    let m = call(4, 1, &NONE, &[99]);
    for b in rec(&[&m[..10], &m[10..30], &m[30..]]) {
        c.write_all(&[b]).unwrap();
    }
    assert_eq!(reply(&mut c), Some(ok(4, &[0, 100])));

    // Two calls in one write: both are served, in order.
    c.write_all(&[rec(&[&call(5, 1, &NONE, &[5])]), rec(&[&call(6, 0, &NONE, &[])])].concat()).unwrap();
    assert_eq!(reply(&mut c), Some(ok(5, &[0, 6])));
    assert_eq!(reply(&mut c), Some(ok(6, &[0])));

    // Errors on a stream: noproc, and argument decoding past the record's end (garbage args).
    c.write_all(&rec(&[&call(7, 7, &NONE, &[])])).unwrap();
    assert_eq!(reply(&mut c), Some(ok(7, &[3])));
    c.write_all(&rec(&[&call(8, 1, &NONE, &[])])).unwrap();
    assert_eq!(reply(&mut c), Some(ok(8, &[4])));

    // A record whose credential is over 400 bytes is dropped (no reply); the next one is served.
    let bad = call(9, 1, &[1, 404], &[0; 102]);
    c.write_all(&[rec(&[&bad]), rec(&[&call(10, 1, &NONE, &[10])])].concat()).unwrap();
    assert_eq!(reply(&mut c), Some(ok(10, &[0, 11])), "only the good call is answered");

    // A peer closing mid-record: the record fails and the server closes that connection.
    let mut d = connect(port);
    d.write_all(&(0x8000_0000_u32 | 100).to_be_bytes()).unwrap();
    d.write_all(&call(11, 1, &NONE, &[1])[..20]).unwrap();
    d.shutdown(Shutdown::Write).unwrap();
    assert!(closed(&mut d), "a record cut short ends its connection");
    // A fragment header claiming 2 GiB, a few bytes, then the end of input.
    let mut e = connect(port);
    e.write_all(&[&0xFFFF_FFFF_u32.to_be_bytes()[..], &[0; 12]].concat()).unwrap();
    e.shutdown(Shutdown::Write).unwrap();
    assert!(closed(&mut e), "a claimed length that never arrives ends its connection");
    // The server lives on: the old connection and a new one are both served.
    c.write_all(&rec(&[&call(12, 1, &NONE, &[12])])).unwrap();
    assert_eq!(reply(&mut c), Some(ok(12, &[0, 13])));
    let mut f = connect(port);
    f.write_all(&rec(&[&call(13, 1, &NONE, &[13])])).unwrap();
    assert_eq!(reply(&mut f), Some(ok(13, &[0, 14])));
    drop(f);

    // Malformed records (the stream corpus), each followed by a good call in the same write:
    // the bad record is dropped without a reply, the connection lives on and the good call is
    // answered (from the bytes already buffered, or on the next poll).
    let frag = |b: &[u8], last: bool| [&(b.len() as u32 | if last { 0x8000_0000 } else { 0 }).to_be_bytes()[..], b].concat();
    let m = call(15, 1, &NONE, &[15]);
    let corpus: [(&str, Vec<u8>); 4] = [
        ("zero-length fragment inside a record", [frag(&m[..12], false), frag(&[], false), frag(&m[12..], true)].concat()),
        ("zero-length record", frag(&[], true)),
        ("last-fragment flag cutting a call short", [frag(&m[..12], true), frag(&m[12..], true)].concat()),
        ("credential length near 4 GiB", rec(&[&call(15, 1, &[1, 0xffff_fff0], &[])])),
    ];
    for (i, (what, bad)) in corpus.iter().enumerate() {
        let xid = 20 + i as u32;
        c.write_all(&[&bad[..], &rec(&[&call(xid, 1, &NONE, &[xid])])].concat()).unwrap();
        assert_eq!(reply(&mut c), Some(ok(xid, &[0, xid + 1])), "{what}: dropped, the next call answered");
    }
    // One-byte fragments in two writes (reads interleave with fragment and word edges).
    let m = call(30, 1, &NONE, &[30]);
    let tiny: Vec<u8> = m.iter().enumerate().flat_map(|(i, b)| frag(&[*b], i + 1 == m.len())).collect();
    let (a, b) = tiny.split_at(tiny.len() / 2 + 1);
    c.write_all(a).unwrap();
    std::thread::sleep(Duration::from_millis(20));
    c.write_all(b).unwrap();
    assert_eq!(reply(&mut c), Some(ok(30, &[0, 31])), "one-byte fragments");

    // Interop: a client built against the host glibc (RPC_CANTDECODEARGS 11, RPC_PROCUNAVAIL 10,
    // RPC_SYSTEMERROR 12, RPC_PROGVERSMISMATCH 9); its 30000-byte argument comes in ~1000-byte fragments.
    let lines = host_client(port);
    assert_eq!(lines, ["int 0 42", "noproc 10", "garbage 11", "syserr 12", "unix 0", &format!("blob 0 {BLOB}"), "vers 9 1 3"]);

    c.write_all(&rec(&[&call(14, 99, &NONE, &[])])).unwrap();
    assert_eq!(reply(&mut c), Some(ok(14, &[0])));
    server.join().unwrap();
    assert!(closed(&mut u), "svc_exit destroys the stream-pair transport too");
    svc::liella_compat_svc_unregister(PROG.into(), 1);
    svc::liella_compat_svc_unregister(PROG.into(), 3);
}
