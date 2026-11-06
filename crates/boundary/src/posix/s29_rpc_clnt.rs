//! §29.1 ONC RPC client side as failing stubs (D-29.5, owner rulings R13 and 2026-10-27).
//!
//! Liella does not implement the ONC RPC client or the port mapper calls. Every name stays
//! exported at its compat version so an old program still loads; a call fails at once, opens no
//! socket or file, leaves a caller's socket (`*sockp`) and buffers as they were, keeps no memory,
//! sets `errno` to `EOPNOTSUPP` and sets `rpc_createerr` to `RPC_SYSTEMERROR` with `re_errno`
//! `EOPNOTSUPP`:
//! * the constructors (`clnt_create`, `clntudp_create`, `clntudp_bufcreate`, `clnttcp_create`,
//!   `clntunix_create`, `clntraw_create`) and `authnone_create` return `NULL`;
//! * `callrpc`, `clnt_broadcast` and `pmap_rmtcall` return `RPC_SYSTEMERROR`;
//! * `pmap_getport` and `getrpcport` return `0`, `pmap_set` and `pmap_unset` `0` (`FALSE`),
//!   `pmap_getmaps` `NULL`, `rtime` `-1`; `get_myaddress` leaves its argument as it was.
//!
//! What stays: the `rpc_createerr` record (`__rpc_thread_createerr`, one per thread) and the
//! `clnt_*err` printers, so a program can still print why its call failed; `bindresvport`
//! (D-15.7: unchanged) and `_rpc_dtablesize`. Every entry is an `internal_entry!`:
//! `cargo xtask so` binds each compat version to the `liella_compat_<name>` symbol.

use core::ptr;
use core::sync::atomic::{AtomicU64, Ordering::Relaxed};

use liella_abi::ctypes::{c_char, c_int, c_uint, c_ulong, c_ushort, c_void};
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{GRND_NONBLOCK, Timespec};
use liella_legacy::sunrpc_clnt::{self as rules, Text};

use super::s11_lowio::write;
use super::s15_sockets::bind;
use super::s23_time::clock_gettime;
use super::s24_system::getdtablesize;
use super::s29_legacy::{Xdr, XdrProc};
use super::s29_rpc_auth::EOPNOTSUPP;
use super::s29_rpc_svc::{OpaqueAuth, RpcErrC};
use crate::sys::CSys;

const EADDRINUSE: i32 = 98;
const EAFNOSUPPORT: i32 = 97;
/// `RPC_SYSTEMERROR`: the `clnt_stat` of every client stub.
pub const RPC_SYSTEMERROR: c_int = 12;
const TEXT_SIZE: usize = 1024;

/// `struct timeval` (passed by value to `cl_call` and `pmap_rmtcall`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Timeval {
    /// `tv_sec`.
    pub tv_sec: i64,
    /// `tv_usec`.
    pub tv_usec: i64,
}

/// `CLIENT` (24 bytes).
#[repr(C)]
#[derive(Debug)]
pub struct Client {
    /// `cl_auth`.
    pub cl_auth: *mut AuthC,
    /// `cl_ops`.
    pub cl_ops: *const ClntOps,
    /// `cl_private`.
    pub cl_private: *mut c_char,
}

/// `struct clnt_ops`.
#[repr(C)]
#[derive(Debug)]
#[allow(missing_docs)]
pub struct ClntOps {
    pub cl_call: Option<extern "C" fn(*mut Client, c_ulong, XdrProc, *mut c_char, XdrProc, *mut c_char, Timeval) -> c_int>,
    pub cl_abort: Option<extern "C" fn(*mut Client)>,
    pub cl_geterr: Option<extern "C" fn(*mut Client, *mut RpcErrC)>,
    pub cl_freeres: Option<extern "C" fn(*mut Client, XdrProc, *mut c_char) -> c_int>,
    pub cl_destroy: Option<extern "C" fn(*mut Client)>,
    pub cl_control: Option<extern "C" fn(*mut Client, c_int, *mut c_char) -> c_int>,
}

/// `AUTH` (72 bytes).
#[repr(C)]
#[derive(Debug)]
pub struct AuthC {
    /// `ah_cred`.
    pub ah_cred: OpaqueAuth,
    /// `ah_verf`.
    pub ah_verf: OpaqueAuth,
    /// `ah_key` (`union des_block`).
    pub ah_key: [u8; 8],
    /// `ah_ops`.
    pub ah_ops: *const AuthOps,
    /// `ah_private`.
    pub ah_private: *mut c_char,
}

/// `struct auth_ops`.
#[repr(C)]
#[derive(Debug)]
#[allow(missing_docs)]
pub struct AuthOps {
    pub ah_nextverf: Option<extern "C" fn(*mut AuthC)>,
    pub ah_marshal: Option<extern "C" fn(*mut AuthC, *mut Xdr) -> c_int>,
    pub ah_validate: Option<extern "C" fn(*mut AuthC, *mut OpaqueAuth) -> c_int>,
    pub ah_refresh: Option<extern "C" fn(*mut AuthC) -> c_int>,
    pub ah_destroy: Option<extern "C" fn(*mut AuthC)>,
}

/// `struct rpc_createerr` (32 bytes).
#[repr(C)]
#[derive(Debug, Default)]
pub struct CreateErrC {
    /// `cf_stat`.
    pub cf_stat: c_int,
    /// `cf_error`.
    pub cf_error: RpcErrC,
}

/// `rpc_createerr` as four words: cf_stat, re_status, the union's two words (read by C callers
/// through `__rpc_thread_createerr` as a `struct rpc_createerr`). Probed: the host glibc keeps
/// one record per thread, and so does Liella (a block in the thread's result table, freed at
/// thread exit); this process-wide record is used only by a thread whose block cannot be had.
static CREATEERR: [AtomicU64; 4] = [const { AtomicU64::new(0) }; 4];

/// The calling thread's `rpc_createerr` block (32 bytes, zeroed when made, never moves).
fn createerr_block<'a>() -> Option<(usize, &'a mut [u8])> {
    crate::users::block_bytes(crate::users::RPC_CREATEERR, size_of::<CreateErrC>()).ok()
}

fn set_createerr(stat: c_int, err: RpcErrC) {
    let w = |v: c_int| u64::from(u32::from_ne_bytes(v.to_ne_bytes()));
    let words = [w(stat), w(err.re_status), err.a, err.b];
    if let Some((_, b)) = createerr_block() {
        for (c, v) in b.chunks_exact_mut(8).zip(words) {
            c.copy_from_slice(&v.to_ne_bytes());
        }
        return;
    }
    for (c, v) in CREATEERR.iter().zip(words) {
        c.store(v, Relaxed);
    }
}

fn createerr() -> (c_int, c_int, u64) {
    let mut words = [0u64; 4];
    if let Some((_, b)) = createerr_block() {
        for (o, c) in words.iter_mut().zip(b.chunks_exact(8)) {
            *o = u64::from_ne_bytes(c.try_into().unwrap_or([0; 8]));
        }
    } else {
        for (o, c) in words.iter_mut().zip(CREATEERR.iter()) {
            *o = c.load(Relaxed);
        }
    }
    let i = |v: u64| c_int::from_ne_bytes(u32::try_from(v & 0xffff_ffff).unwrap_or(0).to_ne_bytes());
    (i(words[0]), i(words[1]), words[2])
}

/// The calling thread's `rpc_createerr` as `(cf_stat, re_status, re_errno)` (read by tests).
#[must_use]
pub fn create_error() -> (c_int, c_int, u64) {
    createerr()
}

/// Clears the calling thread's `rpc_createerr` (`RPC_SUCCESS`, all zero; used by tests).
pub fn clear_create_error() {
    set_createerr(0, RpcErrC::default());
}

/// The text `clnt_spcreateerror(prefix)` returns, copied (at most 128 bytes) with its length.
#[must_use]
pub fn create_error_text(prefix: &core::ffi::CStr) -> ([u8; 128], usize) {
    let s = cstr(liella_compat_clnt_spcreateerror(prefix.as_ptr()));
    let mut out = [0u8; 128];
    let n = s.len().min(out.len());
    if let (Some(d), Some(src)) = (out.get_mut(..n), s.get(..n)) {
        d.copy_from_slice(src);
    }
    (out, n)
}

/// Sets `errno` to `EOPNOTSUPP` (every stub of the RPC client and server).
pub(crate) fn unsupported() {
    crate::errno::set_raw(EOPNOTSUPP);
}

/// The failure of a stub that would have made or used a connection: `rpc_createerr` becomes
/// `RPC_SYSTEMERROR` with `re_errno` `EOPNOTSUPP`, and `errno` `EOPNOTSUPP` (set last, so the
/// record's first use on this thread cannot change it).
pub(crate) fn create_failed() {
    set_createerr(RPC_SYSTEMERROR, RpcErrC { re_status: 0, a: u64::from(EOPNOTSUPP.unsigned_abs()), b: 0 });
    unsupported();
}

/// `create_failed`, then `NULL`.
fn no_client<T>() -> *mut T {
    create_failed();
    ptr::null_mut()
}

/// `create_failed`, then `RPC_SYSTEMERROR`.
fn call_failed() -> c_int {
    create_failed();
    RPC_SYSTEMERROR
}

crate::internal_entry! {
/// `__rpc_thread_createerr`: the calling thread's `rpc_createerr` record.
pub extern "C" fn liella_compat___rpc_thread_createerr() -> *mut CreateErrC {
    match createerr_block() {
        Some((addr, _)) => ptr::with_exposed_provenance_mut(addr),
        None => CREATEERR.as_ptr().cast::<CreateErrC>().cast_mut(),
    }
}}

crate::internal_entry! {
/// `authnone_create`: `NULL` (stub).
pub extern "C" fn liella_compat_authnone_create() -> *mut AuthC {
    no_client()
}}

crate::internal_entry! {
/// `clnt_create`: `NULL` (stub); no host lookup.
pub extern "C" fn liella_compat_clnt_create(_host: *const c_char, _prog: c_ulong, _vers: c_ulong, _proto: *const c_char) -> *mut Client {
    no_client()
}}

crate::internal_entry! {
/// `clntudp_bufcreate`: `NULL` (stub); `*sockp` untouched.
pub extern "C" fn liella_compat_clntudp_bufcreate(_raddr: *mut c_void, _prog: c_ulong, _vers: c_ulong, _wait: Timeval, _sockp: *mut c_int, _sendsz: c_uint, _recvsz: c_uint) -> *mut Client {
    no_client()
}}

crate::internal_entry! {
/// `clntudp_create`: `NULL` (stub); `*sockp` untouched.
pub extern "C" fn liella_compat_clntudp_create(_raddr: *mut c_void, _prog: c_ulong, _vers: c_ulong, _wait: Timeval, _sockp: *mut c_int) -> *mut Client {
    no_client()
}}

crate::internal_entry! {
/// `clnttcp_create`: `NULL` (stub); `*sockp` untouched.
pub extern "C" fn liella_compat_clnttcp_create(_raddr: *mut c_void, _prog: c_ulong, _vers: c_ulong, _sockp: *mut c_int, _sendsz: c_uint, _recvsz: c_uint) -> *mut Client {
    no_client()
}}

crate::internal_entry! {
/// `clntunix_create`: `NULL` (stub); `*sockp` untouched.
pub extern "C" fn liella_compat_clntunix_create(_raddr: *mut c_void, _prog: c_ulong, _vers: c_ulong, _sockp: *mut c_int, _sendsz: c_uint, _recvsz: c_uint) -> *mut Client {
    no_client()
}}

crate::internal_entry! {
/// `clntraw_create`: `NULL` (stub).
pub extern "C" fn liella_compat_clntraw_create(_prog: c_ulong, _vers: c_ulong) -> *mut Client {
    no_client()
}}

crate::internal_entry! {
/// `callrpc`: `RPC_SYSTEMERROR` (stub); `out` untouched.
pub extern "C" fn liella_compat_callrpc(_host: *const c_char, _prog: c_ulong, _vers: c_ulong, _proc: c_ulong, _inproc: XdrProc, _in: *const c_char, _outproc: XdrProc, _out: *mut c_char) -> c_int {
    call_failed()
}}

crate::internal_entry! {
/// `clnt_broadcast`: `RPC_SYSTEMERROR` (stub); nothing sent, `eachresult` never called.
pub extern "C" fn liella_compat_clnt_broadcast(_prog: c_ulong, _vers: c_ulong, _proc: c_ulong, _xargs: XdrProc, _argsp: *mut c_char, _xresults: XdrProc, _resultsp: *mut c_char, _eachresult: Option<extern "C" fn(*mut c_char, *mut c_void) -> c_int>) -> c_int {
    call_failed()
}}

crate::internal_entry! {
/// `pmap_rmtcall`: `RPC_SYSTEMERROR` (stub); `out` and `*portp` untouched.
pub extern "C" fn liella_compat_pmap_rmtcall(_addr: *mut c_void, _prog: c_ulong, _vers: c_ulong, _proc: c_ulong, _xargs: XdrProc, _argsp: *mut c_char, _xres: XdrProc, _resp: *mut c_char, _tout: Timeval, _portp: *mut c_ulong) -> c_int {
    call_failed()
}}

crate::internal_entry! {
/// `pmap_getport`: `0` (stub); the address is not read.
pub extern "C" fn liella_compat_pmap_getport(_addr: *mut c_void, _prog: c_ulong, _vers: c_ulong, _prot: c_uint) -> c_ushort {
    create_failed();
    0
}}

crate::internal_entry! {
/// `getrpcport`: `0` (stub); no host lookup.
pub extern "C" fn liella_compat_getrpcport(_host: *const c_char, _prog: c_ulong, _vers: c_ulong, _proto: c_uint) -> c_int {
    create_failed();
    0
}}

crate::internal_entry! {
/// `pmap_set`: `0` (`FALSE`; stub).
pub extern "C" fn liella_compat_pmap_set(_prog: c_ulong, _vers: c_ulong, _protocol: c_int, _port: c_ushort) -> c_int {
    create_failed();
    0
}}

crate::internal_entry! {
/// `pmap_unset`: `0` (`FALSE`; stub).
pub extern "C" fn liella_compat_pmap_unset(_prog: c_ulong, _vers: c_ulong) -> c_int {
    create_failed();
    0
}}

crate::internal_entry! {
/// `pmap_getmaps`: `NULL` (stub).
pub extern "C" fn liella_compat_pmap_getmaps(_addr: *mut c_void) -> *mut c_void {
    no_client()
}}

crate::internal_entry! {
/// `get_myaddress`: no effect (stub); `addr` untouched.
pub extern "C" fn liella_compat_get_myaddress(_addr: *mut c_void) {
    create_failed();
}}

crate::internal_entry! {
/// `rtime`: `-1` (stub); `timep` untouched.
pub extern "C" fn liella_compat_rtime(_addrp: *mut c_void, _timep: *mut c_void, _timeout: *mut c_void) -> c_int {
    create_failed();
    -1
}}

/// Random bits from the kernel; if it has none yet, the clock (the start of `bindresvport`'s
/// walk over the reserved ports).
fn seed() -> u32 {
    let mut b = [0u8; 4];
    if CSys.getrandom(&mut b, GRND_NONBLOCK) == Ok(4) {
        return u32::from_ne_bytes(b);
    }
    let mut ts = Timespec { sec: 0, nsec: 0 };
    clock_gettime(1, Some(&mut ts));
    let z = u64::from_ne_bytes(ts.nsec.to_ne_bytes()) ^ u64::from_ne_bytes(ts.sec.to_ne_bytes()).rotate_left(32);
    let z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    let z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    u32::try_from((z ^ (z >> 31)) >> 32).unwrap_or(0)
}

/// Stores `port` (host order) as a `sockaddr_in`'s `sin_port` (network order).
pub fn set_port(sin: &mut [u8; 16], port: u16) {
    if let Some(d) = sin.get_mut(2..4) {
        d.copy_from_slice(&port.to_be_bytes());
    }
}

// UNSAFE(slice): the caller's sockaddr_in (16 bytes).
#[allow(unsafe_code)]
fn sin_of(p: *const c_void) -> Option<[u8; 16]> {
    // UNSAFE(slice) SAFETY: p is NULL or a struct sockaddr_in of the caller.
    (!p.is_null()).then(|| unsafe { p.cast::<[u8; 16]>().read_unaligned() })
}

// UNSAFE(slice): the caller's sockaddr_in (16 bytes).
#[allow(unsafe_code)]
fn sin_store(p: *mut c_void, v: [u8; 16]) {
    if !p.is_null() {
        // UNSAFE(slice) SAFETY: p is a struct sockaddr_in of the caller.
        unsafe { p.cast::<[u8; 16]>().write_unaligned(v) };
    }
}

crate::internal_entry! {
/// `bindresvport`: binds `sd` to a free reserved port (512..1023, each once). `sin` NULL means
/// INADDR_ANY; a family other than `AF_INET` is `EAFNOSUPPORT`. Stops at the first error other
/// than `EADDRINUSE` (unprivileged: `EACCES`, as probed).
pub extern "C" fn liella_compat_bindresvport(sd: c_int, sin: *mut c_void) -> c_int {
    let mut a = sin_of(sin).unwrap_or([2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    if a.get(..2) != Some(&2u16.to_ne_bytes()) {
        crate::errno::set_raw(EAFNOSUPPORT);
        return -1;
    }
    for port in rules::resv_ports(seed()) {
        set_port(&mut a, port);
        if bind(sd, a.as_ptr().cast(), 16) == 0 {
            sin_store(sin, a);
            return 0;
        }
        if crate::errno::get() != EADDRINUSE {
            return -1;
        }
    }
    -1
}}

crate::internal_entry! {
/// `_rpc_dtablesize`: the soft descriptor limit (`getdtablesize`; as probed, no 1024 cap).
pub extern "C" fn liella_compat__rpc_dtablesize() -> c_int {
    getdtablesize()
}}

/// The printers' buffer: the calling thread's block (probed: the host glibc's texts are per
/// thread; D-C0.2), made on first use and freed at thread exit; `None` without memory.
fn text_buf<'a>() -> Option<&'a mut [u8]> {
    crate::users::block_bytes(crate::users::RPC_TEXT, TEXT_SIZE).ok().map(|(_, b)| b)
}

// UNSAFE(slice): a NUL-terminated C string of the caller (or of strerror).
#[allow(unsafe_code)]
fn cstr<'a>(p: *const c_char) -> &'a [u8] {
    if p.is_null() {
        return &[];
    }
    // UNSAFE(slice) SAFETY: p is a NUL-terminated string.
    unsafe { core::ffi::CStr::from_ptr(p) }.to_bytes()
}

fn errtext<'a>(e: u64) -> &'a [u8] {
    cstr(ptr::with_exposed_provenance(crate::report::strerror(i32::try_from(e).unwrap_or(0))))
}

fn render(f: impl FnOnce(&mut Text<'_>)) -> *mut c_char {
    let Some(b) = text_buf() else { return ptr::null_mut() };
    let p = b.as_mut_ptr();
    let mut t = Text::new(b);
    f(&mut t);
    t.finish();
    p.cast()
}

fn emit(p: *const c_char) {
    let s = cstr(p);
    write(2, s.as_ptr().cast(), s.len());
}

crate::internal_entry! {
/// `clnt_sperrno`: the static text of a `clnt_stat`.
pub extern "C" fn liella_compat_clnt_sperrno(stat: c_int) -> *const c_char {
    render(|t| t.put(rules::sperrno(stat).as_bytes())).cast_const()
}}

crate::internal_entry! {
/// `clnt_perrno`: the text to standard error, without a newline (as probed).
pub extern "C" fn liella_compat_clnt_perrno(stat: c_int) {
    emit(liella_compat_clnt_sperrno(stat));
}}

crate::internal_entry! {
/// `clnt_sperror`: the client's last error after `s` (a caller's own `CLIENT`: Liella makes none).
pub extern "C" fn liella_compat_clnt_sperror(c: *mut Client, s: *const c_char) -> *mut c_char {
    let mut e = RpcErrC::default();
    clnt_geterr(c, &mut e);
    let pre = cstr(s);
    render(|t| rules::sperror(t, pre, e.re_status, e.a, e.b, errtext(e.a)))
}}

crate::internal_entry! {
/// `clnt_perror`: `clnt_sperror` to standard error.
pub extern "C" fn liella_compat_clnt_perror(c: *mut Client, s: *const c_char) {
    emit(liella_compat_clnt_sperror(c, s));
}}

crate::internal_entry! {
/// `clnt_spcreateerror`: the last create error (`rpc_createerr`) after `s`.
pub extern "C" fn liella_compat_clnt_spcreateerror(s: *const c_char) -> *mut c_char {
    let (cf, re, a) = createerr();
    let pre = cstr(s);
    render(|t| rules::spcreateerror(t, pre, cf, re, errtext(a)))
}}

crate::internal_entry! {
/// `clnt_pcreateerror`: `clnt_spcreateerror` to standard error.
pub extern "C" fn liella_compat_clnt_pcreateerror(s: *const c_char) {
    emit(liella_compat_clnt_spcreateerror(s));
}}

// UNSAFE(slice): the client's ops table (a CLIENT the caller made).
#[allow(unsafe_code)]
fn ops_of<'a>(c: *mut Client) -> Option<&'a ClntOps> {
    // UNSAFE(slice) SAFETY: c is NULL or a live CLIENT whose cl_ops is its table.
    unsafe { c.as_ref() }.and_then(|cl| unsafe { cl.cl_ops.as_ref() })
}

/// `CLNT_GETERR`.
fn clnt_geterr(c: *mut Client, e: &mut RpcErrC) {
    if let Some(g) = ops_of(c).and_then(|o| o.cl_geterr) {
        g(c, e);
    }
}
