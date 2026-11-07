//! §29.1 ONC RPC server side as failing stubs (D-29.5, owner rulings R13 and 2026-10-27).
//!
//! Liella does not implement the ONC RPC server. Every name stays exported at its compat
//! version so an old program still loads; a call fails at once with the failure value below,
//! sets `errno` to `EOPNOTSUPP`, and opens, reads, writes or closes no descriptor (a caller's
//! socket included), registers nothing with the port mapper and keeps no memory:
//! * the transport constructors (`svcudp_create`, `svcudp_bufcreate`, `svctcp_create`,
//!   `svcfd_create`, `svcraw_create`, `svcunix_create`, `svcunixfd_create`) return `NULL` and set
//!   `rpc_createerr` to `RPC_SYSTEMERROR` with `re_errno` `EOPNOTSUPP`, so
//!   `clnt_pcreateerror` can print why;
//! * `svc_register`, `svc_sendreply` and `svcudp_enablecache` return `0` (`FALSE`),
//!   `registerrpc` returns `-1`, `_authenticate` returns `AUTH_FAILED`;
//! * `xprt_register`, `xprt_unregister`, `svc_unregister`, the `svcerr_*` replies, the
//!   `svc_getreq*` readers, `svc_run` and `svc_exit` do nothing (`svc_run` returns at once);
//! * `_seterr_reply` stores `RPC_SYSTEMERROR` with `re_errno` `EOPNOTSUPP` in its `rpc_err`;
//! * `__rpc_thread_svc_fdset` still returns the process's `svc_fdset`, which no stub reads.
//!
//! Every entry is an `internal_entry!`: `cargo xtask so` binds each compat version to the
//! `liella_compat_<name>` symbol.

use core::sync::atomic::AtomicU64;

use liella_abi::ctypes::{c_char, c_int, c_uint, c_ulong, c_void};

use super::s29_legacy::XdrProc;
use super::s29_rpc_auth::EOPNOTSUPP;
use super::s29_rpc_clnt::{RPC_SYSTEMERROR, create_failed, unsupported};

/// `RPC_ANYSOCK`: "make the socket" (a constructor stub makes none).
pub const RPC_ANYSOCK: c_int = -1;
/// `AUTH_FAILED`: what `_authenticate` returns.
pub const AUTH_FAILED: c_int = 7;
/// `FD_SETSIZE`.
const FD_SETSIZE: usize = 1024;

/// `struct opaque_auth` (24 bytes).
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct OpaqueAuth {
    /// `oa_flavor`.
    pub oa_flavor: c_int,
    /// `oa_base`.
    pub oa_base: *mut c_char,
    /// `oa_length` (at most 400).
    pub oa_length: c_uint,
}

/// `struct rpc_err` (24 bytes).
#[repr(C)]
#[derive(Debug, Default)]
pub struct RpcErrC {
    /// `re_status`.
    pub re_status: c_int,
    /// The union's first word (`re_errno`/`re_why`, `low`, `s1`).
    pub a: u64,
    /// The union's second word (`high`, `s2`).
    pub b: u64,
}

/// `SVCXPRT` (opaque here: no stub reads one).
pub type SvcXprt = c_void;
/// The dispatch routine of `svc_register`.
pub type Dispatch = Option<extern "C" fn(*mut c_void, *mut SvcXprt)>;

/// `svc_fdset`: always empty (no transport is ever registered).
static SVC_FDSET: [AtomicU64; FD_SETSIZE / 64] = [const { AtomicU64::new(0) }; FD_SETSIZE / 64];

/// The constructor failure: `NULL`, `rpc_createerr` and `errno` set.
fn no_xprt() -> *mut SvcXprt {
    create_failed();
    core::ptr::null_mut()
}

crate::internal_entry! {
/// `svcudp_bufcreate`: `NULL` (stub).
pub extern "C" fn liella_compat_svcudp_bufcreate(_sock: c_int, _sendsz: c_uint, _recvsz: c_uint) -> *mut SvcXprt {
    no_xprt()
}}

crate::internal_entry! {
/// `svcudp_create`: `NULL` (stub).
pub extern "C" fn liella_compat_svcudp_create(_sock: c_int) -> *mut SvcXprt {
    no_xprt()
}}

crate::internal_entry! {
/// `svctcp_create`: `NULL` (stub).
pub extern "C" fn liella_compat_svctcp_create(_sock: c_int, _sendsize: c_uint, _recvsize: c_uint) -> *mut SvcXprt {
    no_xprt()
}}

crate::internal_entry! {
/// `svcfd_create`: `NULL` (stub); the descriptor is left open.
pub extern "C" fn liella_compat_svcfd_create(_fd: c_int, _sendsize: c_uint, _recvsize: c_uint) -> *mut SvcXprt {
    no_xprt()
}}

crate::internal_entry! {
/// `svcraw_create`: `NULL` (stub).
pub extern "C" fn liella_compat_svcraw_create() -> *mut SvcXprt {
    no_xprt()
}}

crate::internal_entry! {
/// `svcunix_create`: `NULL` (stub); no socket and no file at `path`.
pub extern "C" fn liella_compat_svcunix_create(_sock: c_int, _sendsize: c_uint, _recvsize: c_uint, _path: *mut c_char) -> *mut SvcXprt {
    no_xprt()
}}

crate::internal_entry! {
/// `svcunixfd_create`: `NULL` (stub); the descriptor is left open.
pub extern "C" fn liella_compat_svcunixfd_create(_fd: c_int, _sendsize: c_uint, _recvsize: c_uint) -> *mut SvcXprt {
    no_xprt()
}}

crate::internal_entry! {
/// `svcudp_enablecache`: `0` (failure; stub).
pub extern "C" fn liella_compat_svcudp_enablecache(_x: *mut SvcXprt, _size: c_ulong) -> c_int {
    unsupported();
    0
}}

crate::internal_entry! {
/// `xprt_register`: no effect (stub).
pub extern "C" fn liella_compat_xprt_register(_x: *mut SvcXprt) {
    unsupported();
}}

crate::internal_entry! {
/// `xprt_unregister`: no effect (stub).
pub extern "C" fn liella_compat_xprt_unregister(_x: *mut SvcXprt) {
    unsupported();
}}

crate::internal_entry! {
/// `svc_register`: `0` (`FALSE`; stub); nothing registered, no port mapper contacted.
pub extern "C" fn liella_compat_svc_register(_x: *mut SvcXprt, _prog: c_ulong, _vers: c_ulong, _dispatch: Dispatch, _protocol: c_int) -> c_int {
    unsupported();
    0
}}

crate::internal_entry! {
/// `svc_unregister`: no effect (stub); no port mapper contacted.
pub extern "C" fn liella_compat_svc_unregister(_prog: c_ulong, _vers: c_ulong) {
    unsupported();
}}

crate::internal_entry! {
/// `registerrpc`: `-1` (stub); no transport made, nothing registered.
pub extern "C" fn liella_compat_registerrpc(_prog: c_ulong, _vers: c_ulong, _proc: c_ulong, _routine: Option<extern "C" fn(*mut c_char) -> *mut c_char>, _inproc: XdrProc, _outproc: XdrProc) -> c_int {
    unsupported();
    -1
}}

crate::internal_entry! {
/// `_authenticate`: `AUTH_FAILED` (stub); the request is not read.
pub extern "C" fn liella_compat__authenticate(_req: *mut c_void, _msg: *const c_void) -> c_int {
    unsupported();
    AUTH_FAILED
}}

crate::internal_entry! {
/// `svc_sendreply`: `0` (`FALSE`; stub); nothing sent.
pub extern "C" fn liella_compat_svc_sendreply(_x: *mut SvcXprt, _proc: XdrProc, _objp: *mut c_char) -> c_int {
    unsupported();
    0
}}

macro_rules! svcerr {
    ($($doc:literal $name:ident($($a:ident: $t:ty),*);)*) => {$(
        crate::internal_entry! {
        #[doc = $doc]
        pub extern "C" fn $name(_x: *mut SvcXprt $(, $a: $t)*) {
            unsupported();
        }}
    )*};
}

svcerr! {
    "`svcerr_noproc`: no effect (stub)." liella_compat_svcerr_noproc();
    "`svcerr_decode`: no effect (stub)." liella_compat_svcerr_decode();
    "`svcerr_systemerr`: no effect (stub)." liella_compat_svcerr_systemerr();
    "`svcerr_noprog`: no effect (stub)." liella_compat_svcerr_noprog();
    "`svcerr_progvers`: no effect (stub)." liella_compat_svcerr_progvers(_low: c_ulong, _high: c_ulong);
    "`svcerr_auth`: no effect (stub)." liella_compat_svcerr_auth(_why: c_int);
    "`svcerr_weakauth`: no effect (stub)." liella_compat_svcerr_weakauth();
}

crate::internal_entry! {
/// `svc_getreq_common`: no effect (stub); the descriptor is not read.
pub extern "C" fn liella_compat_svc_getreq_common(_fd: c_int) {
    unsupported();
}}

crate::internal_entry! {
/// `svc_getreqset`: no effect (stub); the set is not read.
pub extern "C" fn liella_compat_svc_getreqset(_readfds: *const u64) {
    unsupported();
}}

crate::internal_entry! {
/// `svc_getreq`: no effect (stub).
pub extern "C" fn liella_compat_svc_getreq(_rdfds: c_int) {
    unsupported();
}}

crate::internal_entry! {
/// `svc_getreq_poll`: no effect (stub); the array is not read.
pub extern "C" fn liella_compat_svc_getreq_poll(_pfd: *mut c_void, _nready: c_int) {
    unsupported();
}}

crate::internal_entry! {
/// `svc_run`: returns at once (stub); a program's "svc_run returned" path runs.
pub extern "C" fn liella_compat_svc_run() {
    unsupported();
}}

crate::internal_entry! {
/// `svc_exit`: no effect (stub).
pub extern "C" fn liella_compat_svc_exit() {
    unsupported();
}}

crate::internal_entry! {
/// `__rpc_thread_svc_fdset`: `svc_fdset`, an `fd_set` no stub reads.
pub extern "C" fn liella_compat___rpc_thread_svc_fdset() -> *mut u64 {
    // The words are atomics, so a caller's FD_SET through this pointer is a plain store.
    SVC_FDSET.as_ptr().cast::<u64>().cast_mut()
}}

crate::internal_entry! {
/// `_seterr_reply`: stores `RPC_SYSTEMERROR` / `EOPNOTSUPP` in `err` (stub); `msg` is not read.
pub extern "C" fn liella_compat__seterr_reply(_msg: *const c_void, err: *mut RpcErrC) {
    // UNSAFE(slice) SAFETY: err is NULL or the caller's struct rpc_err (the _seterr_reply contract).
    if let Some(e) = unsafe { err.as_mut() } {
        *e = RpcErrC { re_status: RPC_SYSTEMERROR, a: u64::from(EOPNOTSUPP.unsigned_abs()), b: 0 };
    }
    unsupported();
}}
