// §29.1 ONC RPC client, port mapper and server calls as failing stubs (D-29.5, owner rulings R13 and 2026-10-27).
//! Every stub in `s29_rpc_clnt` and `s29_rpc_svc` is called from one table: each must return its
//! failure value, set `errno` to `EOPNOTSUPP`, set `rpc_createerr` where D-29.5 says so, leave the
//! caller's buffers (socket numbers, results, addresses) as they were, open no file descriptor and
//! keep no memory; `clnt_spcreateerror` must then print the stub's error.
//! One test only: the heap check reads process-wide counters, which another test thread would move.
#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::indexing_slicing, unreachable_pub)]

use core::ffi::{c_char, c_int, c_ulong, c_void};
use core::ptr::{null, null_mut};
use std::collections::BTreeSet;
use std::ffi::OsString;

use liella_boundary::errno;
use liella_boundary::posix::s03_memory as m;
use liella_boundary::posix::s29_rpc_auth::EOPNOTSUPP;
use liella_boundary::posix::s29_rpc_clnt::{self as c, Timeval};
use liella_boundary::posix::s29_rpc_svc as s;

const CANARY: u8 = 0xA5;
const RPC_SYSTEMERROR: i64 = 12;
const AUTH_FAILED: i64 = 7;

type Stub = Box<dyn Fn(*mut c_char) -> i64>;

/// The open descriptor numbers, as a set: listing order is the file system's, so only membership is compared.
fn open_fds() -> BTreeSet<OsString> {
    std::fs::read_dir("/proc/self/fd").unwrap().map(|e| e.unwrap().file_name()).collect::<BTreeSet<_>>()
}

fn in_use() -> u64 {
    m::mallinfo2().uordblks
}

/// `(name, failure value, sets rpc_createerr, call)`: `b` is a 64-byte canary buffer the call
/// passes wherever the interface takes an output (a socket number, a result, an address).
fn table() -> Vec<(&'static str, i64, bool, Stub)> {
    let host = c"localhost".as_ptr();
    let udp = c"udp".as_ptr();
    let p = |q: *mut c_void| i64::from(!q.is_null());
    let t = Timeval { tv_sec: 1, tv_usec: 0 };
    let v = |b: *mut c_char| b.cast::<c_void>();
    let fd = |b: *mut c_char| b.wrapping_add(32).cast::<c_int>();
    vec![
        ("clnt_create", 0, true, Box::new(move |_| p(c::liella_compat_clnt_create(host, 1, 1, udp).cast()))),
        ("clntudp_create", 0, true, Box::new(move |b| p(c::liella_compat_clntudp_create(v(b), 1, 1, t, fd(b)).cast()))),
        ("clntudp_bufcreate", 0, true, Box::new(move |b| p(c::liella_compat_clntudp_bufcreate(v(b), 1, 1, t, fd(b), 0, 0).cast()))),
        ("clnttcp_create", 0, true, Box::new(move |b| p(c::liella_compat_clnttcp_create(v(b), 1, 1, fd(b), 0, 0).cast()))),
        ("clntunix_create", 0, true, Box::new(move |b| p(c::liella_compat_clntunix_create(v(b), 1, 1, fd(b), 0, 0).cast()))),
        ("clntraw_create", 0, true, Box::new(|_| p(c::liella_compat_clntraw_create(1, 1).cast()))),
        ("authnone_create", 0, true, Box::new(|_| p(c::liella_compat_authnone_create().cast()))),
        ("callrpc", RPC_SYSTEMERROR, true, Box::new(move |b| i64::from(c::liella_compat_callrpc(host, 1, 1, 1, None, null(), None, b)))),
        ("clnt_broadcast", RPC_SYSTEMERROR, true, Box::new(|b| i64::from(c::liella_compat_clnt_broadcast(1, 1, 1, None, b, None, b, None)))),
        ("pmap_rmtcall", RPC_SYSTEMERROR, true, Box::new(move |b| i64::from(c::liella_compat_pmap_rmtcall(v(b), 1, 1, 1, None, b, None, b, t, b.wrapping_add(48).cast::<c_ulong>())))),
        ("pmap_getport", 0, true, Box::new(|b| i64::from(c::liella_compat_pmap_getport(v(b), 1, 1, 17)))),
        ("getrpcport", 0, true, Box::new(move |_| i64::from(c::liella_compat_getrpcport(host, 1, 1, 17)))),
        ("pmap_set", 0, true, Box::new(|_| i64::from(c::liella_compat_pmap_set(1, 1, 17, 2049)))),
        ("pmap_unset", 0, true, Box::new(|_| i64::from(c::liella_compat_pmap_unset(1, 1)))),
        ("pmap_getmaps", 0, true, Box::new(|b| p(c::liella_compat_pmap_getmaps(v(b))))),
        ("get_myaddress", 0, true, Box::new(|b| { c::liella_compat_get_myaddress(v(b)); 0 })),
        ("rtime", -1, true, Box::new(|b| i64::from(c::liella_compat_rtime(v(b), b.wrapping_add(16).cast(), b.wrapping_add(32).cast())))),
        ("svcudp_create", 0, true, Box::new(|_| p(s::liella_compat_svcudp_create(s::RPC_ANYSOCK)))),
        ("svcudp_bufcreate", 0, true, Box::new(|_| p(s::liella_compat_svcudp_bufcreate(s::RPC_ANYSOCK, 0, 0)))),
        ("svctcp_create", 0, true, Box::new(|_| p(s::liella_compat_svctcp_create(s::RPC_ANYSOCK, 0, 0)))),
        ("svcfd_create", 0, true, Box::new(|_| p(s::liella_compat_svcfd_create(0, 0, 0)))),
        ("svcraw_create", 0, true, Box::new(|_| p(s::liella_compat_svcraw_create()))),
        ("svcunix_create", 0, true, Box::new(|b| p(s::liella_compat_svcunix_create(s::RPC_ANYSOCK, 0, 0, b)))),
        ("svcunixfd_create", 0, true, Box::new(|_| p(s::liella_compat_svcunixfd_create(0, 0, 0)))),
        ("svcudp_enablecache", 0, false, Box::new(|b| i64::from(s::liella_compat_svcudp_enablecache(v(b), 16)))),
        ("xprt_register", 0, false, Box::new(|b| { s::liella_compat_xprt_register(v(b)); 0 })),
        ("xprt_unregister", 0, false, Box::new(|b| { s::liella_compat_xprt_unregister(v(b)); 0 })),
        ("svc_register", 0, false, Box::new(|b| i64::from(s::liella_compat_svc_register(v(b), 1, 1, None, 17)))),
        ("svc_unregister", 0, false, Box::new(|_| { s::liella_compat_svc_unregister(1, 1); 0 })),
        ("registerrpc", -1, false, Box::new(|_| i64::from(s::liella_compat_registerrpc(1, 1, 1, None, None, None)))),
        ("_authenticate", AUTH_FAILED, false, Box::new(|b| i64::from(s::liella_compat__authenticate(v(b), v(b))))),
        ("svc_sendreply", 0, false, Box::new(|b| i64::from(s::liella_compat_svc_sendreply(v(b), None, b)))),
        ("svcerr_noproc", 0, false, Box::new(|b| { s::liella_compat_svcerr_noproc(v(b)); 0 })),
        ("svcerr_decode", 0, false, Box::new(|b| { s::liella_compat_svcerr_decode(v(b)); 0 })),
        ("svcerr_systemerr", 0, false, Box::new(|b| { s::liella_compat_svcerr_systemerr(v(b)); 0 })),
        ("svcerr_noprog", 0, false, Box::new(|b| { s::liella_compat_svcerr_noprog(v(b)); 0 })),
        ("svcerr_progvers", 0, false, Box::new(|b| { s::liella_compat_svcerr_progvers(v(b), 1, 2); 0 })),
        ("svcerr_auth", 0, false, Box::new(|b| { s::liella_compat_svcerr_auth(v(b), 1); 0 })),
        ("svcerr_weakauth", 0, false, Box::new(|b| { s::liella_compat_svcerr_weakauth(v(b)); 0 })),
        ("svc_getreq_common", 0, false, Box::new(|_| { s::liella_compat_svc_getreq_common(0); 0 })),
        ("svc_getreqset", 0, false, Box::new(|b| { s::liella_compat_svc_getreqset(b.cast::<u64>()); 0 })),
        ("svc_getreq", 0, false, Box::new(|_| { s::liella_compat_svc_getreq(1); 0 })),
        ("svc_getreq_poll", 0, false, Box::new(|b| { s::liella_compat_svc_getreq_poll(v(b), 1); 0 })),
        ("svc_run", 0, false, Box::new(|_| { s::liella_compat_svc_run(); 0 })),
        ("svc_exit", 0, false, Box::new(|_| { s::liella_compat_svc_exit(); 0 })),
    ]
}

/// Runs `f` with a canary buffer; returns (result, errno, buffer after).
fn run(f: &dyn Fn(*mut c_char) -> i64) -> (i64, i32, [u8; 64]) {
    let mut buf = [CANARY; 64];
    errno::set_raw(0);
    let r = f(buf.as_mut_ptr().cast());
    (r, errno::get(), buf)
}

#[test]
fn every_rpc_client_and_server_stub_fails_cleanly() {
    // Trace: no spec clause (§29.1 RPC client/server stubs; D-29.5 failure table, owner rulings R13 and 2026-10-27).
    let table = table();
    let want_text: &[u8] = b"stub: RPC: Remote system error - Operation not supported\n";
    // First round: the per-thread rpc_createerr and printer blocks are made here (freed at thread exit).
    for (_, _, _, f) in &table {
        run(f.as_ref());
    }
    let _ = c::create_error_text(c"stub");
    let (fds, heap) = (open_fds(), in_use());
    for (name, want, sets, f) in &table {
        c::clear_create_error();
        let (got, e, buf) = run(f.as_ref());
        assert_eq!(got, *want, "{name}: failure value");
        assert_eq!(e, EOPNOTSUPP, "{name}: errno");
        assert!(buf.iter().all(|&b| b == CANARY), "{name}: wrote into the caller's buffer");
        assert_eq!(open_fds(), fds, "{name}: opened a descriptor");
        assert_eq!(in_use(), heap, "{name}: kept memory");
        if *sets {
            assert_eq!(c::create_error(), (12, 0, 95), "{name}: rpc_createerr");
            let (text, n) = c::create_error_text(c"stub");
            assert_eq!(&text[..n], want_text, "{name}: clnt_spcreateerror");
        } else {
            assert_eq!(c::create_error(), (0, 0, 0), "{name}: touched rpc_createerr");
        }
    }
    // `_seterr_reply` reports the stub's error in its rpc_err.
    let mut e = s::RpcErrC { re_status: 0, a: 7, b: 9 };
    errno::set_raw(0);
    s::liella_compat__seterr_reply(null_mut(), &raw mut e);
    assert_eq!((e.re_status, e.a, e.b), (12, 95, 0));
    assert_eq!(errno::get(), EOPNOTSUPP);
}
