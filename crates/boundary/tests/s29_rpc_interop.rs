// §29.1 ONC RPC client interop (LG-15, D-29.1); RFC 5531 wire format.
//! The Liella UDP client against the Liella server (`svc_run` in a thread) and against a server
//! compiled with `cc` against the HOST glibc (black box): results, PROCUNAVAIL, PROGVERSMISMATCH
//! with the version range, and CLSET_XID/CLGET_XID as probed on the host glibc. Plus the
//! per-thread `rpc_createerr` record and printer text (probed: one per thread).
#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::arithmetic_side_effects, clippy::indexing_slicing, unreachable_pub)]

use core::ffi::{c_char, c_int, c_void};
use core::mem::offset_of;
use std::io::BufRead;
use std::net::UdpSocket;
use std::os::fd::IntoRawFd;

use liella_boundary::posix::s04_strings::memcpy;
use liella_boundary::posix::s29_legacy::{Xdr, XdrProc, liella_compat_xdr_int, liella_compat_xdr_void};
use liella_boundary::posix::s29_rpc_clnt as clnt;
use liella_boundary::posix::s29_rpc_svc as svc;
use clnt::Timeval;
use svc::{RpcErrC, SvcReq, SvcXprt, XpOps};

const PROG: u32 = 0x2000_5678;

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

type GetArgs = extern "C" fn(*mut SvcXprt, XdrProc, *mut c_void) -> c_int;

extern "C" fn dispatch(req: *mut SvcReq, x: *mut SvcXprt) {
    match rd_u64(req.cast::<u8>().cast_const(), offset_of!(SvcReq, rq_proc)) {
        1 => {
            let ops = rd_u64(x.cast::<u8>().cast_const(), offset_of!(SvcXprt, xp_ops)) as usize;
            let mut f: Option<GetArgs> = None;
            memcpy((&raw mut f).cast(), (ops + offset_of!(XpOps, xp_getargs)) as *const c_void, core::mem::size_of::<usize>());
            let mut v: c_int = 0;
            if f.unwrap()(x, xdr_int(), (&raw mut v).cast()) == 0 {
                svc::liella_compat_svcerr_decode(x);
                return;
            }
            v += 1;
            svc::liella_compat_svc_sendreply(x, xdr_int(), (&raw mut v).cast::<c_char>());
        }
        99 => {
            svc::liella_compat_svc_sendreply(x, xdr_void(), core::ptr::null_mut());
            svc::liella_compat_svc_exit();
        }
        _ => svc::liella_compat_svcerr_noproc(x),
    }
}

fn client(port: u16, vers: u64) -> *mut clnt::Client {
    let mut a = [0u8; 16];
    a[..2].copy_from_slice(&2u16.to_ne_bytes());
    a[2..4].copy_from_slice(&port.to_be_bytes());
    a[4..8].copy_from_slice(&[127, 0, 0, 1]);
    let mut sock: c_int = -1;
    let c = clnt::liella_compat_clntudp_create(a.as_mut_ptr().cast::<c_void>(), PROG.into(), vers, Timeval { tv_sec: 0, tv_usec: 300_000 }, &mut sock);
    assert!(!c.is_null());
    c
}

const T: Timeval = Timeval { tv_sec: 3, tv_usec: 0 };

fn int_call(c: *mut clnt::Client, proc_: u64, v: c_int) -> (c_int, c_int) {
    let (mut a, mut r) = (v, -1);
    (clnt::clnt_call(c, proc_, xdr_int(), (&raw mut a).cast(), xdr_int(), (&raw mut r).cast(), T), r)
}

/// The same checks against either server; the last call (procedure 99) stops it.
fn check(port: u16, who: &str) {
    let c = client(port, 1);
    assert_eq!(int_call(c, 1, 41), (0, 42), "{who}: result");
    assert_eq!(int_call(c, 7, 0).0, 10, "{who}: RPC_PROCUNAVAIL");
    let mut x: u32 = 0x12345;
    assert_eq!(clnt::clnt_control(c, 11, (&raw mut x).cast()), 1, "{who}: CLSET_XID");
    assert_eq!(int_call(c, 1, 1), (0, 2), "{who}: call after CLSET_XID");
    let mut got: u32 = 0;
    clnt::clnt_control(c, 10, (&raw mut got).cast());
    assert_eq!(got, 0x0101_2344, "{who}: CLGET_XID after CLSET_XID 0x12345 and one call (host glibc probe)");
    let v = client(port, 2);
    assert_eq!(int_call(v, 1, 0).0, 9, "{who}: RPC_PROGVERSMISMATCH");
    let mut e = RpcErrC::default();
    clnt::clnt_geterr(v, &mut e);
    assert_eq!((e.a, e.b), (1, 1), "{who}: version range");
    clnt::clnt_destroy(v);
    assert_eq!(clnt::clnt_call(c, 99, xdr_void(), core::ptr::null_mut(), xdr_void(), core::ptr::null_mut(), T), 0, "{who}: stop");
    clnt::clnt_destroy(c);
}

const C_SERVER: &str = r#"
#include <stdio.h>
#include <stdlib.h>
#include <unistd.h>
typedef int bool_t; typedef bool_t (*xdrproc_t)(void *, void *, ...);
struct svc_req { unsigned long rq_prog, rq_vers, rq_proc; struct { int f; char *b; unsigned l; } rq_cred; void *rq_clntcred; void *rq_xprt; };
typedef struct SVCXPRT SVCXPRT;
struct xp_ops { bool_t (*xp_recv)(void); int (*xp_stat)(void); bool_t (*xp_getargs)(SVCXPRT *, xdrproc_t, char *); };
struct SVCXPRT { int xp_sock; unsigned short xp_port; struct xp_ops *xp_ops; };
__asm__(".symver svcudp_create,svcudp_create@GLIBC_2.2.5");
__asm__(".symver svc_register,svc_register@GLIBC_2.2.5");
__asm__(".symver svc_run,svc_run@GLIBC_2.2.5");
__asm__(".symver svc_sendreply,svc_sendreply@GLIBC_2.2.5");
__asm__(".symver svcerr_noproc,svcerr_noproc@GLIBC_2.2.5");
__asm__(".symver svcerr_decode,svcerr_decode@GLIBC_2.2.5");
__asm__(".symver xdr_int,xdr_int@GLIBC_2.2.5");
__asm__(".symver xdr_void,xdr_void@GLIBC_2.2.5");
SVCXPRT *svcudp_create(int); bool_t svc_register(SVCXPRT *, unsigned long, unsigned long, void (*)(struct svc_req *, SVCXPRT *), unsigned long);
void svc_run(void); bool_t svc_sendreply(SVCXPRT *, xdrproc_t, char *); void svcerr_noproc(SVCXPRT *); void svcerr_decode(SVCXPRT *);
bool_t xdr_int(void *, int *); bool_t xdr_void(void);
static void disp(struct svc_req *r, SVCXPRT *x) {
  int v = 0;
  if (r->rq_proc == 1) { if (!x->xp_ops->xp_getargs(x, (xdrproc_t)xdr_int, (char *)&v)) { svcerr_decode(x); return; }
    v++; svc_sendreply(x, (xdrproc_t)xdr_int, (char *)&v); }
  else if (r->rq_proc == 99) { svc_sendreply(x, (xdrproc_t)xdr_void, 0); exit(0); }
  else svcerr_noproc(x); }
int main(void) {
  SVCXPRT *x = svcudp_create(-1); if (!x) return 1;
  if (!svc_register(x, PROG, 1, disp, 0)) return 2;
  printf("%u\n", x->xp_port); fflush(stdout); alarm(30); svc_run(); return 3; }
"#;

#[test]
fn liella_client_against_liella_and_host_servers() {
    // Trace: no spec clause (LG-15 client interop; RFC 5531 wire format, black-box host-glibc server); D-29.1.
    let s = UdpSocket::bind("127.0.0.1:0").unwrap();
    let port = s.local_addr().unwrap().port();
    let x = svc::liella_compat_svcudp_create(s.into_raw_fd());
    assert!(!x.is_null());
    assert_eq!(svc::liella_compat_svc_register(x, PROG.into(), 1, Some(dispatch), 0), 1);
    let server = std::thread::spawn(|| svc::liella_compat_svc_run());
    check(port, "Liella server");
    server.join().unwrap();
    svc::liella_compat_svc_unregister(PROG.into(), 1);

    let dir = std::env::temp_dir().join(format!("liella-rpc-interop-{}", std::process::id()));
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

#[test]
fn createerr_and_printer_text_are_per_thread() {
    // Trace: no spec clause (LG-15 `__rpc_thread_createerr`, `clnt_spcreateerror`; per thread as probed on the host glibc); D-29.1.
    let mine = clnt::liella_compat___rpc_thread_createerr();
    let stat: c_int = 13;
    memcpy(mine.cast(), (&raw const stat).cast(), 4);
    let text = clnt::liella_compat_clnt_spcreateerror(c"m".as_ptr()) as usize;
    assert_eq!(clnt::liella_compat_clnt_spcreateerror(c"n".as_ptr()) as usize, text, "one buffer per thread");
    let (other, other_stat, other_text) = std::thread::spawn(|| {
        let p = clnt::liella_compat___rpc_thread_createerr();
        let mut s: c_int = -1;
        memcpy((&raw mut s).cast(), p.cast_const().cast(), 4);
        (p as usize, s, clnt::liella_compat_clnt_spcreateerror(c"t".as_ptr()) as usize)
    })
    .join()
    .unwrap();
    assert_ne!(other, mine as usize, "rpc_createerr is per thread");
    assert_eq!(other_stat, 0, "a new thread's record starts clear");
    assert_ne!(other_text, text, "the printer text is per thread");
    assert_eq!(clnt::next_xid(0x0001_2344), 0x0101_2344);
    assert_eq!(clnt::next_xid(0xff00_0000), 0x0001_0000, "the wire's first byte carries into the next");
}
