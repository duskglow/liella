//! Translation for spec §29.1: the ONC RPC client over UDP and TCP (LG-15, D-29.1).
//!
//! Internal `liella_compat_*` entries like the server (R15-1, R15-2). Every rule applied to a
//! reply's bytes is `liella_legacy::sunrpc_clnt` (bounds-checked; a reply for another xid or too
//! short to carry one is skipped; anything malformed for this call is `RPC_CANTDECODERES`).
//! This layer owns the socket, the `CLIENT` handle, the retry/total timeouts, `AUTH_NONE`,
//! `rpc_createerr`, the `clnt_*` printers, `bindresvport`, `_rpc_dtablesize` and the port
//! mapper lookup `pmap_getport` (used by `clntudp_create` for a zero `sin_port`). Behaviour
//! settled by black-box probes of the host glibc (handoff/rpc-client.md).
//!
//! Deliberate difference (D-29.2, owner ruling 10-23): a UDP call takes replies only from the
//! address and port it was sent to; a datagram from anywhere else is dropped whatever its xid,
//! and when one shaped like an RPC reply arrived and the call times out, the call ends with
//! `RPC_CANTRECV` and `EREMCHG` instead of `RPC_TIMEDOUT`. The compat tunable
//! `liella.compat.rpc_reply_any_source=1` (COMPAT-12, ignored in a secure process) restores the
//! reference behaviour. Each call's xid is fresh random bits unless the caller set one with
//! `CLSET_XID`.

use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};

use liella_abi::ctypes::{c_char, c_int, c_uint, c_ulong, c_ushort, c_void};
use liella::config::Tunables;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{GRND_NONBLOCK, Timespec};
use liella_legacy::sunrpc::{XDR_DECODE, XDR_ENCODE};
use liella_legacy::sunrpc_clnt::{self as rules, Outcome, Text};
use liella_legacy::sunrpc_svc::{RpcErr, clnt_stat};

use super::s11_lowio::{close, poll, read, write};
use super::s15_sockets::{bind, connect, recvfrom, sendto, setsockopt, socket};
use super::s23_time::clock_gettime;
use super::s24_system::getdtablesize;
use super::s29_legacy::{Xdr, XdrProc, liella_compat_xdr_free, liella_compat_xdrmem_create};
use super::s29_xdr_streams::{liella_compat_xdrrec_create, liella_compat_xdrrec_endofrecord, liella_compat_xdrrec_eof, liella_compat_xdrrec_skiprecord};
use super::s29_rpc_svc::{OpaqueAuth, RPC_ANYSOCK, RpcErrC};
use crate::heap;
use crate::sys::CSys;

/// `UDPMSGSIZE`: the buffers when the caller gives 0.
const UDP_SIZE: c_uint = 8800;
const POLLIN: i16 = 1;
const EINTR: i32 = 4;
const EAGAIN: i32 = 11;
const EREMCHG: u64 = 78;
const EADDRINUSE: i32 = 98;
const EAFNOSUPPORT: i32 = 97;
const MSG_DONTWAIT: c_int = 0x40;
const MSG_ERRQUEUE: c_int = 0x2000;
/// `IPPROTO_IP` / `IP_RECVERR`: report ICMP errors (a closed port) to the receive call.
const IP_RECVERR: c_int = 11;
/// `IPPROTO_UDP` (the `pmap_getport` protocol of `clntudp_create`).
pub const IPPROTO_UDP: c_ulong = 17;
/// The port mapper: port 111, program 100000 version 2, `PMAPPROC_GETPORT` 3.
pub const PMAP_PORT: u16 = 111;
const PMAP_PROG: u32 = 100_000;
const PMAP_VERS: u32 = 2;
const PMAP_GETPORT: u32 = 3;
/// `RPC_PROGNOTREGISTERED`.
const PROGNOTREGISTERED: c_int = 15;
/// `AUTH_INVALIDRESP` (a verifier the AUTH rejects).
const AUTH_INVALIDRESP: u64 = 6;
/// `clnt_control` requests (rpc/clnt.h).
const CLSET_TIMEOUT: c_int = 1;
const CLGET_TIMEOUT: c_int = 2;
const CLGET_SERVER_ADDR: c_int = 3;
const CLSET_RETRY_TIMEOUT: c_int = 4;
const CLGET_RETRY_TIMEOUT: c_int = 5;
const CLGET_FD: c_int = 6;
const CLSET_FD_CLOSE: c_int = 8;
const CLSET_FD_NCLOSE: c_int = 9;
const CLGET_XID: c_int = 10;
const CLSET_XID: c_int = 11;
const CLGET_VERS: c_int = 12;
const CLGET_PROG: c_int = 14;
/// The `clnt_sperror`/`clnt_spcreateerror` buffer.
const TEXT_SIZE: usize = 1024;

/// `struct timeval` (passed by value to `cl_call`).
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Timeval {
    /// `tv_sec`.
    pub tv_sec: i64,
    /// `tv_usec`.
    pub tv_usec: i64,
}

impl Timeval {
    /// The milliseconds of a non-negative value (a negative part counts as 0).
    #[must_use]
    pub fn millis(self) -> i64 {
        self.tv_sec.max(0).saturating_mul(1000).saturating_add(self.tv_usec.max(0) / 1000)
    }
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

fn errc(status: c_int, a: u64) -> RpcErrC {
    RpcErrC { re_status: status, a, b: 0 }
}

fn errno_err(status: c_int) -> RpcErrC {
    errc(status, u64::from(crate::errno::get().unsigned_abs()))
}

fn now_ms() -> i64 {
    let mut ts = Timespec { sec: 0, nsec: 0 };
    clock_gettime(1, Some(&mut ts));
    ts.sec.saturating_mul(1000).saturating_add(ts.nsec / 1_000_000)
}

#[repr(C)]
struct PollFd {
    fd: c_int,
    events: i16,
    revents: i16,
}

/// A UDP client's private block.
struct Cu {
    sock: c_int,
    closeit: bool,
    addr: [u8; 16],
    prog: u32,
    vers: u32,
    xid: u32,
    /// `CLSET_XID` chose the next call's xid.
    xid_set: bool,
    wait: Timeval,
    total: Option<Timeval>,
    err: RpcErrC,
    out: *mut u8,
    sendsz: usize,
    inb: *mut u8,
    recvsz: usize,
    /// The bytes of the last reply (the results' decode reads `inb[..len]`).
    len: usize,
}

static AUTHNONE_OPS: AuthOps = AuthOps { ah_nextverf: Some(an_nextverf), ah_marshal: Some(an_marshal), ah_validate: Some(an_validate), ah_refresh: Some(an_refresh), ah_destroy: Some(an_destroy) };

extern "C" fn an_nextverf(_a: *mut AuthC) {}

/// Writes the empty credential and verifier (flavor 0, length 0, twice).
extern "C" fn an_marshal(_a: *mut AuthC, x: *mut Xdr) -> c_int {
    c_int::from(put_bytes(x, &[0u8; 16]))
}

extern "C" fn an_validate(_a: *mut AuthC, _v: *mut OpaqueAuth) -> c_int {
    1
}

extern "C" fn an_refresh(_a: *mut AuthC) -> c_int {
    0
}

extern "C" fn an_destroy(_a: *mut AuthC) {}

// UNSAFE(slice): the caller's XDR stream and its ops table.
#[allow(unsafe_code)]
pub(crate) fn put_bytes(x: *mut Xdr, b: &[u8]) -> bool {
    // UNSAFE(slice) SAFETY: x is NULL or a live XDR stream whose x_ops is a valid table.
    let Some(put) = (unsafe { x.as_ref() }).and_then(|xr| unsafe { xr.x_ops.as_ref() }).and_then(|o| o.x_putbytes) else { return false };
    put(x, b.as_ptr().cast(), c_uint::try_from(b.len()).unwrap_or(c_uint::MAX)) != 0
}

// UNSAFE(slice): a stream's position through its ops table.
#[allow(unsafe_code)]
fn getpos(x: &Xdr) -> usize {
    // UNSAFE(slice) SAFETY: x_ops of a stream made by xdrmem_create is its static table.
    let f = unsafe { x.x_ops.as_ref() }.and_then(|o| o.x_getpostn);
    f.map_or(0, |g| usize::try_from(g(x)).unwrap_or(0))
}

static AUTHNONE: AtomicUsize = AtomicUsize::new(0);

// UNSAFE(slice): a fresh block of AUTH's size.
#[allow(unsafe_code)]
fn make_authnone() -> usize {
    let a = heap::malloc_entry(core::mem::size_of::<AuthC>()).cast::<AuthC>();
    if a.is_null() {
        return 0;
    }
    let none = OpaqueAuth { oa_flavor: 0, oa_base: ptr::null_mut(), oa_length: 0 };
    // UNSAFE(slice) SAFETY: a is a fresh block of AuthC's size, malloc-aligned (16).
    unsafe { a.write(AuthC { ah_cred: none, ah_verf: none, ah_key: [0; 8], ah_ops: &AUTHNONE_OPS, ah_private: ptr::null_mut() }) };
    a.expose_provenance()
}

crate::internal_entry! {
/// `authnone_create`: the shared `AUTH_NONE` handle (made once; its destroy does nothing).
pub extern "C" fn liella_compat_authnone_create() -> *mut AuthC {
    let mut p = AUTHNONE.load(Relaxed);
    if p == 0 {
        let n = make_authnone();
        p = match AUTHNONE.compare_exchange(0, n, Relaxed, Relaxed) {
            Ok(_) => n,
            Err(old) => {
                if n != 0 {
                    heap::free_entry(n);
                }
                old
            }
        };
    }
    ptr::with_exposed_provenance_mut(p)
}}

crate::internal_entry! {
/// `__rpc_thread_createerr`: the `rpc_createerr` record.
pub extern "C" fn liella_compat___rpc_thread_createerr() -> *mut CreateErrC {
    match createerr_block() {
        Some((addr, _)) => ptr::with_exposed_provenance_mut(addr),
        None => CREATEERR.as_ptr().cast::<CreateErrC>().cast_mut(),
    }
}}

static CLNTUDP_OPS: ClntOps = ClntOps { cl_call: Some(u_call), cl_abort: Some(u_abort), cl_geterr: Some(u_geterr), cl_freeres: Some(u_freeres), cl_destroy: Some(u_destroy), cl_control: Some(u_control) };

// UNSAFE(slice): the private block of a client made here (cl_ops is CLNTUDP_OPS).
#[allow(unsafe_code)]
fn cu_of<'a>(c: *mut Client) -> Option<&'a mut Cu> {
    // UNSAFE(slice) SAFETY: c is NULL or a live CLIENT (the cl_ops contract).
    let cl = unsafe { c.as_ref() }?;
    if !ptr::eq(cl.cl_ops, &CLNTUDP_OPS) {
        return None;
    }
    // UNSAFE(slice) SAFETY: cl_private of a CLNTUDP_OPS client is its Cu (create wrote it).
    unsafe { cl.cl_private.cast::<Cu>().as_mut() }
}

// UNSAFE(slice): a buffer of the private block (len bytes at p, owned by the Cu).
#[allow(unsafe_code)]
fn buf<'a>(p: *mut u8, len: usize) -> &'a mut [u8] {
    if p.is_null() {
        return &mut [];
    }
    // UNSAFE(slice) SAFETY: p is a live block of len bytes owned by the client.
    unsafe { core::slice::from_raw_parts_mut(p, len) }
}

// UNSAFE(slice): the client's AUTH and its ops table.
#[allow(unsafe_code)]
fn auth_ops<'a>(c: *mut Client) -> (*mut AuthC, Option<&'a AuthOps>) {
    // UNSAFE(slice) SAFETY: c is a live CLIENT; cl_auth is NULL or a live AUTH.
    let a = unsafe { c.as_ref() }.map_or(ptr::null_mut(), |cl| cl.cl_auth);
    // UNSAFE(slice) SAFETY: a is NULL or a live AUTH whose ah_ops is NULL or its table.
    (a, unsafe { a.as_ref() }.and_then(|ar| unsafe { ar.ah_ops.as_ref() }))
}

/// One call: `enc` writes the credential, verifier and arguments after the header, `dec`
/// reads the results (at the offset given) out of the reply. Retries every `cu.wait` until
/// `total_ms` has passed since the first send.
type Enc<'e> = &'e mut dyn FnMut(&mut [u8]) -> Option<usize>;
type Dec<'d> = &'d mut dyn FnMut(usize, &mut [u8]) -> Result<(), RpcErrC>;

fn exchange(cu: &mut Cu, proc_: u32, total_ms: i64, enc: Enc<'_>, dec: Dec<'_>) -> RpcErrC {
    cu.xid = call_xid(cu.xid, &mut cu.xid_set);
    let xid = cu.xid;
    let any_source = ANY_SOURCE.load(Relaxed);
    // A datagram shaped like a reply came from another address during this call (D-29.2).
    let mut stray = false;
    let out = buf(cu.out, cu.sendsz);
    let Some(h) = rules::call_header(xid, cu.prog, cu.vers, proc_, out) else { return errc(rules::CANTENCODEARGS, 0) };
    let Some(rest) = out.get_mut(h..) else { return errc(rules::CANTENCODEARGS, 0) };
    let Some(n) = enc(rest) else { return errc(rules::CANTENCODEARGS, 0) };
    let len = h.saturating_add(n);
    let start = now_ms();
    let wait = cu.wait.millis().max(1);
    loop {
        let sent = sendto(cu.sock, cu.out.cast_const().cast(), len, 0, cu.addr.as_ptr().cast(), 16);
        if usize::try_from(sent).ok() != Some(len) {
            return errno_err(rules::CANTSEND);
        }
        if total_ms <= 0 {
            return errc(rules::TIMEDOUT, 0);
        }
        let resend = now_ms().saturating_add(wait);
        loop {
            let now = now_ms();
            let end = start.saturating_add(total_ms);
            if now >= end {
                return timed_out(stray);
            }
            let left = resend.min(end).saturating_sub(now);
            if left <= 0 {
                break;
            }
            let mut pfd = PollFd { fd: cu.sock, events: POLLIN, revents: 0 };
            let r = poll((&raw mut pfd).cast(), 1, c_int::try_from(left).unwrap_or(c_int::MAX));
            if r < 0 {
                if crate::errno::get() == EINTR {
                    continue;
                }
                return errno_err(rules::CANTRECV);
            }
            if r == 0 {
                continue;
            }
            // A reply longer than the buffer is cut to it (as probed: a 9000-byte reply into the
            // 8800-byte buffer decodes); only the bytes received are ever read.
            let mut from = [0u8; 16];
            let mut flen: c_uint = 16;
            let got = recvfrom(cu.sock, cu.inb.cast(), cu.recvsz, MSG_DONTWAIT, from.as_mut_ptr().cast(), Some(&mut flen));
            let Ok(m) = usize::try_from(got) else {
                let e = crate::errno::get();
                if e == EINTR || e == EAGAIN {
                    continue;
                }
                // Drop the queued ICMP report so the next call does not see it again.
                let mut junk = [0u8; 64];
                recvfrom(cu.sock, junk.as_mut_ptr().cast(), junk.len(), MSG_ERRQUEUE | MSG_DONTWAIT, ptr::null_mut(), None);
                return errc(rules::CANTRECV, u64::from(e.unsigned_abs()));
            };
            cu.len = m.min(cu.recvsz);
            let reply = buf(cu.inb, cu.len);
            // D-29.2: only the called address answers; anything else is dropped before its xid
            // is read, and only its shape is remembered for the diagnosis at the timeout.
            if !any_source && !rules::same_source(&cu.addr, &from) {
                stray |= rules::looks_like_reply(reply);
                continue;
            }
            match rules::decode_reply(reply, xid) {
                Outcome::Ignore => {}
                Outcome::CantDecode => return errc(rules::CANTDECODERES, 0),
                Outcome::Error(e) => return rpc_err(e),
                Outcome::Success { results, .. } => {
                    let r = buf(cu.inb, cu.len);
                    return match dec(results, r) {
                        Ok(()) => errc(clnt_stat::SUCCESS, 0),
                        Err(e) => e,
                    };
                }
            }
        }
    }
}

/// The end of a call that got no reply in time: `RPC_TIMEDOUT`, or `RPC_CANTRECV` with
/// `EREMCHG` when a reply-shaped datagram came from another address meanwhile (D-29.2).
fn timed_out(stray: bool) -> RpcErrC {
    if stray { errc(rules::CANTRECV, EREMCHG) } else { errc(rules::TIMEDOUT, 0) }
}

/// COMPAT-12: replies are taken from any source (the reference behaviour). Set once at
/// start-up from `liella.compat.rpc_reply_any_source`; never in a secure process, where
/// `Tunables::parse` honours nothing.
static ANY_SOURCE: AtomicBool = AtomicBool::new(false);

/// Start-up (and tests): take the compat flags this module reads from the parsed tunables.
pub fn compat_from_start(t: &Tunables) {
    ANY_SOURCE.store(t.rpc_reply_any_source == Some(true), Relaxed);
}

/// The xid of the next call: the one `CLSET_XID` chose (stepped as probed), otherwise fresh
/// random bits, so one call's xid tells nothing about the next (D-29.2).
fn call_xid(xid: u32, set: &mut bool) -> u32 {
    if core::mem::take(set) { next_xid(xid) } else { fresh_xid(xid) }
}

/// Random bits from the kernel; if it has none yet, the clock mixed with `prev`.
fn fresh_xid(prev: u32) -> u32 {
    let mut b = [0u8; 4];
    if CSys.getrandom(&mut b, GRND_NONBLOCK) == Ok(4) {
        return u32::from_ne_bytes(b);
    }
    let mut ts = Timespec { sec: 0, nsec: 0 };
    clock_gettime(1, Some(&mut ts));
    let z = u64::from_ne_bytes(ts.nsec.to_ne_bytes()) ^ u64::from_ne_bytes(ts.sec.to_ne_bytes()).rotate_left(32) ^ u64::from(prev);
    let z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    let z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    u32::try_from((z ^ (z >> 31)) >> 32).unwrap_or(0)
}

fn rpc_err(e: RpcErr) -> RpcErrC {
    match e {
        RpcErr::Status(s) => errc(s, 0),
        RpcErr::Versions(s, l, h) => RpcErrC { re_status: s, a: l, b: h },
        RpcErr::Why(w) => errc(clnt_stat::AUTHERROR, u64::from(w)),
        RpcErr::Failed(s1, s2) => RpcErrC { re_status: clnt_stat::FAILED, a: u64::from_ne_bytes(s1.to_ne_bytes()), b: u64::from_ne_bytes(s2.to_ne_bytes()) },
    }
}

fn mem(b: &mut [u8], op: c_int) -> Xdr {
    let mut x = Xdr { x_op: op, x_ops: ptr::null(), x_public: ptr::null_mut(), x_private: ptr::null_mut(), x_base: ptr::null_mut(), x_handy: 0 };
    liella_compat_xdrmem_create(&mut x, b.as_mut_ptr().cast(), c_uint::try_from(b.len()).unwrap_or(c_uint::MAX), op);
    x
}

extern "C" fn u_call(c: *mut Client, proc_: c_ulong, xargs: XdrProc, argsp: *mut c_char, xres: XdrProc, resp: *mut c_char, timeout: Timeval) -> c_int {
    let (auth, ops) = auth_ops(c);
    let Some(cu) = cu_of(c) else { return rules::CANTSEND };
    let total = cu.total.unwrap_or(timeout).millis();
    let marshal = ops.and_then(|o| o.ah_marshal);
    let validate = ops.and_then(|o| o.ah_validate);
    let mut enc = |rest: &mut [u8]| -> Option<usize> {
        let mut x = mem(rest, XDR_ENCODE);
        if marshal?(auth, &mut x) == 0 || xargs?(&mut x, argsp.cast()) == 0 {
            return None;
        }
        Some(getpos(&x))
    };
    let mut dec = |at: usize, reply: &mut [u8]| -> Result<(), RpcErrC> {
        if let Some(v) = validate {
            let verf = verifier(reply, at);
            let mut oa = OpaqueAuth { oa_flavor: c_int::from_ne_bytes(verf.0.to_ne_bytes()), oa_base: verf.1.as_ptr().cast_mut().cast(), oa_length: c_uint::try_from(verf.1.len()).unwrap_or(0) };
            if v(auth, &mut oa) == 0 {
                return Err(errc(clnt_stat::AUTHERROR, AUTH_INVALIDRESP));
            }
        }
        let mut x = mem(reply.get_mut(at..).unwrap_or_default(), XDR_DECODE);
        match xres {
            Some(f) if f(&mut x, resp.cast()) != 0 => Ok(()),
            _ => Err(errc(rules::CANTDECODERES, 0)),
        }
    };
    let pnum = u32::try_from(proc_).unwrap_or(u32::MAX);
    let e = exchange(cu, pnum, total, &mut enc, &mut dec);
    let st = e.re_status;
    cu.err = e;
    st
}

/// The verifier of a successful reply whose results start at `at`: the flavor and body that
/// sit before the accept status (already bounds-checked by the decode).
fn verifier(reply: &[u8], at: usize) -> (u32, &[u8]) {
    let mut r = liella_legacy::sunrpc_svc::Reader::new(reply);
    let _ = r.take(12);
    match liella_legacy::sunrpc_svc::auth(&mut r) {
        Ok(a) if r.pos().saturating_add(4) == at => (a.flavor, a.body),
        _ => (0, &[]),
    }
}

extern "C" fn u_abort(_c: *mut Client) {}

// UNSAFE(slice): the caller's rpc_err.
#[allow(unsafe_code)]
extern "C" fn u_geterr(c: *mut Client, e: *mut RpcErrC) {
    let Some(cu) = cu_of(c) else { return };
    // UNSAFE(slice) SAFETY: e is NULL or the caller's rpc_err.
    if let Some(dst) = unsafe { e.as_mut() } {
        *dst = RpcErrC { re_status: cu.err.re_status, a: cu.err.a, b: cu.err.b };
    }
}

extern "C" fn u_freeres(_c: *mut Client, xres: XdrProc, resp: *mut c_char) -> c_int {
    liella_compat_xdr_free(xres, resp);
    1
}

extern "C" fn u_destroy(c: *mut Client) {
    let Some(cu) = cu_of(c) else { return };
    if cu.closeit {
        close(cu.sock);
    }
    for p in [cu.out.addr(), cu.inb.addr(), c.addr()] {
        if p != 0 {
            heap::free_entry(p);
        }
    }
    heap::free_entry(ptr::from_mut(cu).addr());
}

// UNSAFE(slice): the caller's control object (its type is the request's, rpc/clnt.h).
#[allow(unsafe_code)]
fn ctl_get<T>(info: *mut c_char) -> T {
    // UNSAFE(slice) SAFETY: info points at the request's object; unaligned access is used.
    unsafe { info.cast::<T>().read_unaligned() }
}

// UNSAFE(slice): the caller's control object (its type is the request's, rpc/clnt.h).
#[allow(unsafe_code)]
fn ctl_put<T>(info: *mut c_char, v: T) {
    // UNSAFE(slice) SAFETY: info points at the request's object; unaligned access is used.
    unsafe { info.cast::<T>().write_unaligned(v) };
}

extern "C" fn u_control(c: *mut Client, req: c_int, info: *mut c_char) -> c_int {
    let Some(cu) = cu_of(c) else { return 0 };
    if info.is_null() && !matches!(req, CLSET_FD_CLOSE | CLSET_FD_NCLOSE) {
        return 0;
    }
    match req {
        CLSET_TIMEOUT => cu.total = Some(ctl_get(info)),
        CLGET_TIMEOUT => ctl_put(info, cu.total.unwrap_or(Timeval { tv_sec: -1, tv_usec: -1 })),
        CLSET_RETRY_TIMEOUT => cu.wait = ctl_get(info),
        CLGET_RETRY_TIMEOUT => ctl_put(info, cu.wait),
        CLGET_SERVER_ADDR => ctl_put(info, cu.addr),
        CLGET_FD => ctl_put(info, cu.sock),
        CLSET_FD_CLOSE => cu.closeit = true,
        CLSET_FD_NCLOSE => cu.closeit = false,
        CLGET_XID => ctl_put(info, cu.xid),
        // Probed: the next call carries `next_xid(x - 1)` (0x12345 gives 0x01012344).
        CLSET_XID => (cu.xid, cu.xid_set) = (ctl_get::<u32>(info).wrapping_sub(1), true),
        CLGET_VERS => ctl_put(info, c_ulong::from(cu.vers)),
        CLGET_PROG => ctl_put(info, c_ulong::from(cu.prog)),
        _ => return 0,
    }
    1
}

/// The port mapper call's arguments: `struct pmap` {prog, vers, prot, port 0}, network order.
#[must_use]
pub fn pmap_args(prog: u32, vers: u32, prot: u32) -> [u8; 16] {
    let mut b = [0u8; 16];
    for (d, w) in b.chunks_exact_mut(4).zip([prog, vers, prot, 0]) {
        d.copy_from_slice(&w.to_be_bytes());
    }
    b
}

/// The port of a `PMAPPROC_GETPORT` reply's results (one unsigned word, network order); `None`
/// when the results are cut short. As probed on the host glibc, the word's low 16 bits are the
/// port (70000 gives 4464, 65536 gives 0, which `pmap_getport` reports as not registered).
#[must_use]
pub fn pmap_result(res: &[u8]) -> Option<u16> {
    let w = u32::from_be_bytes(res.get(..4)?.try_into().ok()?);
    u16::try_from(w & 0xffff).ok()
}

/// The xid of the next call after `xid`. As probed on the host glibc (CLGET_XID after calls),
/// the increment is applied to the xid's wire bytes read as a native word, so on x86_64 each
/// call adds 0x0100_0000 to the value CLGET_XID reports.
#[must_use]
pub fn next_xid(xid: u32) -> u32 {
    u32::from_be_bytes(u32::from_ne_bytes(xid.to_be_bytes()).wrapping_add(1).to_ne_bytes())
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
/// `pmap_getport`: asks the port mapper at `addr` (port 111) for (prog, vers, prot). 0 with
/// `rpc_createerr` set on failure: `RPC_PMAPFAILURE` with the call's error, or
/// `RPC_PROGNOTREGISTERED` when the mapper answers 0.
pub extern "C" fn liella_compat_pmap_getport(addr: *mut c_void, prog: c_ulong, vers: c_ulong, prot: c_uint) -> c_ushort {
    let Some(mut sin) = sin_of(addr) else { return 0 };
    set_port(&mut sin, PMAP_PORT);
    let args = pmap_args(u32::try_from(prog).unwrap_or(u32::MAX), u32::try_from(vers).unwrap_or(u32::MAX), prot);
    let Some(cu) = new_cu(RPC_ANYSOCK, sin, PMAP_PROG, PMAP_VERS, Timeval { tv_sec: 5, tv_usec: 0 }, 400, 400) else { return 0 };
    let mut port = 0u16;
    let mut enc = |rest: &mut [u8]| -> Option<usize> {
        let dst = rest.get_mut(..32)?;
        dst.get_mut(16..)?.copy_from_slice(&args);
        dst.get_mut(..16)?.fill(0);
        Some(32)
    };
    let mut dec = |at: usize, reply: &mut [u8]| -> Result<(), RpcErrC> {
        port = reply.get(at..).and_then(pmap_result).ok_or_else(|| errc(rules::CANTDECODERES, 0))?;
        Ok(())
    };
    let e = exchange(cu, PMAP_GETPORT, 60_000, &mut enc, &mut dec);
    free_cu(cu);
    if e.re_status != clnt_stat::SUCCESS {
        set_createerr(rules::PMAPFAILURE, e);
        return 0;
    }
    if port == 0 {
        set_createerr(PROGNOTREGISTERED, RpcErrC::default());
    }
    port
}}

fn free_cu(cu: &mut Cu) {
    if cu.closeit {
        close(cu.sock);
    }
    for p in [cu.out.addr(), cu.inb.addr()] {
        if p != 0 {
            heap::free_entry(p);
        }
    }
    heap::free_entry(ptr::from_mut(cu).addr());
}

fn round4(n: c_uint) -> usize {
    let n = if n == 0 { UDP_SIZE } else { n };
    usize::try_from(n.saturating_add(3) & !3).unwrap_or(0)
}

// UNSAFE(slice): a fresh heap block of Cu's size and the two buffers.
#[allow(unsafe_code)]
fn new_cu<'a>(sock: c_int, addr: [u8; 16], prog: u32, vers: u32, wait: Timeval, sendsz: c_uint, recvsz: c_uint) -> Option<&'a mut Cu> {
    let (sendsz, recvsz) = (round4(sendsz), round4(recvsz));
    let (s, closeit) = if sock == RPC_ANYSOCK {
        let s = socket(2, 2, 0);
        if s < 0 {
            set_createerr(clnt_stat::SYSTEMERROR, errno_err(0));
            return None;
        }
        // Not privileged here: a reserved port is tried and its failure ignored, as probed.
        let _ = liella_compat_bindresvport(s, ptr::null_mut());
        (s, true)
    } else {
        (sock, false)
    };
    let one: c_int = 1;
    setsockopt(s, 0, IP_RECVERR, (&raw const one).cast(), 4);
    let out = heap::malloc_entry(sendsz).cast::<u8>();
    let inb = heap::malloc_entry(recvsz).cast::<u8>();
    let cu = heap::malloc_entry(core::mem::size_of::<Cu>()).cast::<Cu>();
    if out.is_null() || inb.is_null() || cu.is_null() {
        for p in [out.addr(), inb.addr(), cu.addr()] {
            if p != 0 {
                heap::free_entry(p);
            }
        }
        if closeit {
            close(s);
        }
        set_createerr(clnt_stat::SYSTEMERROR, errc(0, 12));
        return None;
    }
    let seed = fresh_xid(0);
    // UNSAFE(slice) SAFETY: cu is a fresh block of Cu's size, malloc-aligned (16).
    unsafe { cu.write(Cu { sock: s, closeit, addr, prog, vers, xid: seed, xid_set: false, wait, total: None, err: RpcErrC::default(), out, sendsz, inb, recvsz, len: 0 }) };
    // UNSAFE(slice) SAFETY: cu was written just above.
    unsafe { cu.as_mut() }
}

crate::internal_entry! {
/// `clntudp_bufcreate`: a UDP client for (prog, vers) at `raddr` (looked up through the port
/// mapper when its port is 0, and stored back), retrying every `wait`. `*sockp` is the socket
/// (`RPC_ANYSOCK`: made here, and closed by destroy).
pub extern "C" fn liella_compat_clntudp_bufcreate(raddr: *mut c_void, prog: c_ulong, vers: c_ulong, wait: Timeval, sockp: *mut c_int, sendsz: c_uint, recvsz: c_uint) -> *mut Client {
    let (Some(mut sin), Some(sock)) = (sin_of(raddr), sock_in(sockp)) else {
        set_createerr(clnt_stat::SYSTEMERROR, errc(0, 22));
        return ptr::null_mut();
    };
    let (p, v) = (u32::try_from(prog).unwrap_or(u32::MAX), u32::try_from(vers).unwrap_or(u32::MAX));
    if sin.get(2..4) == Some(&[0, 0]) {
        let port = liella_compat_pmap_getport(raddr, prog, vers, c_uint::try_from(IPPROTO_UDP).unwrap_or(17));
        if port == 0 {
            return ptr::null_mut();
        }
        set_port(&mut sin, port);
        sin_store(raddr, sin);
    }
    let Some(cu) = new_cu(sock, sin, p, v, wait, sendsz, recvsz) else { return ptr::null_mut() };
    let c = heap::malloc_entry(core::mem::size_of::<Client>()).cast::<Client>();
    if c.is_null() {
        free_cu(cu);
        set_createerr(clnt_stat::SYSTEMERROR, errc(0, 12));
        return ptr::null_mut();
    }
    sock_out(sockp, cu.sock);
    write_client(c, ptr::from_mut(cu).cast(), &CLNTUDP_OPS);
    c
}}

// UNSAFE(slice): a fresh block of Client's size.
#[allow(unsafe_code)]
fn write_client(c: *mut Client, private: *mut c_char, ops: &'static ClntOps) {
    // UNSAFE(slice) SAFETY: c is a fresh block of Client's size, malloc-aligned (16).
    unsafe { c.write(Client { cl_auth: liella_compat_authnone_create(), cl_ops: ops, cl_private: private }) };
}

// UNSAFE(slice): the caller's socket word.
#[allow(unsafe_code)]
fn sock_in(p: *mut c_int) -> Option<c_int> {
    // UNSAFE(slice) SAFETY: p is NULL or the caller's int.
    unsafe { p.as_ref() }.copied()
}

// UNSAFE(slice): the caller's socket word.
#[allow(unsafe_code)]
fn sock_out(p: *mut c_int, v: c_int) {
    // UNSAFE(slice) SAFETY: p is the caller's int (sock_in read it).
    if let Some(d) = unsafe { p.as_mut() } {
        *d = v;
    }
}

crate::internal_entry! {
/// `clntudp_create`: `clntudp_bufcreate` with 8800-byte buffers.
pub extern "C" fn liella_compat_clntudp_create(raddr: *mut c_void, prog: c_ulong, vers: c_ulong, wait: Timeval, sockp: *mut c_int) -> *mut Client {
    liella_compat_clntudp_bufcreate(raddr, prog, vers, wait, sockp, UDP_SIZE, UDP_SIZE)
}}

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
    let seed = fresh_xid(0);
    for port in rules::resv_ports(seed) {
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
/// `clnt_sperror`: the client's last error after `s`.
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

// UNSAFE(slice): the client's ops table (any CLIENT, made here or by the caller).
#[allow(unsafe_code)]
fn ops_of<'a>(c: *mut Client) -> Option<&'a ClntOps> {
    // UNSAFE(slice) SAFETY: c is NULL or a live CLIENT whose cl_ops is its table.
    unsafe { c.as_ref() }.and_then(|cl| unsafe { cl.cl_ops.as_ref() })
}

/// `CLNT_GETERR`.
pub fn clnt_geterr(c: *mut Client, e: &mut RpcErrC) {
    if let Some(g) = ops_of(c).and_then(|o| o.cl_geterr) {
        g(c, e);
    }
}

/// `CLNT_CALL`.
pub fn clnt_call(c: *mut Client, proc_: c_ulong, xargs: XdrProc, argsp: *mut c_char, xres: XdrProc, resp: *mut c_char, t: Timeval) -> c_int {
    ops_of(c).and_then(|o| o.cl_call).map_or(rules::CANTSEND, |f| f(c, proc_, xargs, argsp, xres, resp, t))
}

/// `CLNT_CONTROL`.
pub fn clnt_control(c: *mut Client, req: c_int, info: *mut c_char) -> c_int {
    ops_of(c).and_then(|o| o.cl_control).map_or(0, |f| f(c, req, info))
}

/// `CLNT_DESTROY`.
pub fn clnt_destroy(c: *mut Client) {
    if let Some(f) = ops_of(c).and_then(|o| o.cl_destroy) {
        f(c);
    }
}

// --- the TCP client (clnttcp_create) -----------------------------------------------------

/// `ECONNRESET`: the error a closed connection reports (as probed on the host glibc).
const ECONNRESET: u64 = 104;
/// The longest reply head read before the results: xid, type, stat; verifier (400 bytes at
/// most); accept stat and a version range.
const HEAD_MAX: usize = 432;

/// A TCP client's private block. The record stream's callbacks reach it through a raw pointer
/// while `xs` is in use, so no reference to the whole block lives across an XDR call.
struct Ct {
    sock: c_int,
    closeit: bool,
    addr: [u8; 16],
    prog: u32,
    vers: u32,
    xid: u32,
    /// `CLSET_XID` chose the next call's xid.
    xid_set: bool,
    total: Option<Timeval>,
    err: RpcErrC,
    /// When the call in progress times out (`now_ms` clock).
    deadline: i64,
    /// The transport error of the call in progress (status 0: none).
    io: RpcErrC,
    xs: Xdr,
}

static CLNTTCP_OPS: ClntOps = ClntOps { cl_call: Some(t_call), cl_abort: Some(u_abort), cl_geterr: Some(t_geterr), cl_freeres: Some(u_freeres), cl_destroy: Some(t_destroy), cl_control: Some(t_control) };

// UNSAFE(slice): the private block of a client made by clnttcp_create (cl_ops is CLNTTCP_OPS).
#[allow(unsafe_code)]
fn ct_raw(c: *mut Client) -> Option<*mut Ct> {
    // UNSAFE(slice) SAFETY: c is NULL or a live CLIENT (the cl_ops contract).
    let cl = unsafe { c.as_ref() }?;
    ptr::eq(cl.cl_ops, &CLNTTCP_OPS).then(|| cl.cl_private.cast::<Ct>())
}

// UNSAFE(slice): a short-lived access to a live Ct (never held across an XDR call).
#[allow(unsafe_code)]
fn with_ct<T>(p: *mut Ct, f: impl FnOnce(&mut Ct) -> T) -> T {
    // UNSAFE(slice) SAFETY: p is a live Ct; no other reference to it exists during f.
    f(unsafe { &mut *p })
}

// UNSAFE(slice): the address of a Ct's record stream (no reference taken).
#[allow(unsafe_code)]
fn xs_of(p: *mut Ct) -> *mut Xdr {
    // UNSAFE(slice) SAFETY: p is a live Ct; only the field's address is taken.
    unsafe { &raw mut (*p).xs }
}

// UNSAFE(slice): the record stream's direction and ops table.
#[allow(unsafe_code)]
fn xs_set_op(x: *mut Xdr, op: c_int) {
    // UNSAFE(slice) SAFETY: x is a live Ct's record stream.
    unsafe { (*x).x_op = op };
}

// UNSAFE(slice): a stream's getbytes through its ops table.
#[allow(unsafe_code)]
fn get_bytes(x: *mut Xdr, b: &mut [u8]) -> bool {
    // UNSAFE(slice) SAFETY: x is a live record stream whose x_ops is its static table.
    let Some(get) = (unsafe { x.as_ref() }).and_then(|xr| unsafe { xr.x_ops.as_ref() }).and_then(|o| o.x_getbytes) else { return false };
    get(x, b.as_mut_ptr().cast(), c_uint::try_from(b.len()).unwrap_or(c_uint::MAX)) != 0
}

// UNSAFE(slice): a stream's destroy through its ops table.
#[allow(unsafe_code)]
fn xs_destroy(x: *mut Xdr) {
    // UNSAFE(slice) SAFETY: x is a live record stream whose x_ops is NULL or its table.
    if let Some(d) = (unsafe { x.as_ref() }).and_then(|xr| unsafe { xr.x_ops.as_ref() }).and_then(|o| o.x_destroy) {
        d(x);
    }
}

fn set_io(p: *mut Ct, e: RpcErrC) {
    with_ct(p, |ct| ct.io = e);
}

/// The call's transport error if one happened, else `stat`.
fn io_or(p: *mut Ct, stat: c_int) -> RpcErrC {
    let io = with_ct(p, |ct| core::mem::take(&mut ct.io));
    if io.re_status != clnt_stat::SUCCESS { io } else { errc(stat, 0) }
}

/// `readit` (handle: the Ct): one read once the socket is readable before the deadline; short
/// reads are the record layer's to join. End of input is `RPC_CANTRECV` with `ECONNRESET`.
extern "C" fn ct_read(h: *mut c_char, b: *mut c_char, len: c_int) -> c_int {
    let p = h.cast::<Ct>();
    let (fd, deadline) = with_ct(p, |ct| (ct.sock, ct.deadline));
    loop {
        let left = deadline.saturating_sub(now_ms());
        if left <= 0 {
            set_io(p, errc(rules::TIMEDOUT, 0));
            return -1;
        }
        let mut pfd = PollFd { fd, events: POLLIN, revents: 0 };
        let r = poll((&raw mut pfd).cast(), 1, c_int::try_from(left).unwrap_or(c_int::MAX));
        if r == 0 || (r < 0 && crate::errno::get() == EINTR) {
            continue;
        }
        if r < 0 {
            set_io(p, errno_err(rules::CANTRECV));
            return -1;
        }
        let n = read(fd, b.cast(), usize::try_from(len).unwrap_or(0));
        match c_int::try_from(n) {
            Ok(n) if n > 0 => return n,
            Ok(0) => set_io(p, errc(rules::CANTRECV, ECONNRESET)),
            _ if matches!(crate::errno::get(), EINTR | EAGAIN) => continue,
            _ => set_io(p, errno_err(rules::CANTRECV)),
        }
        return -1;
    }
}

/// `writeit` (handle: the Ct): the whole buffer, or `RPC_CANTSEND` with the error.
extern "C" fn ct_write(h: *mut c_char, b: *mut c_char, len: c_int) -> c_int {
    let fd = with_ct(h.cast(), |ct: &mut Ct| ct.sock);
    let total = usize::try_from(len).unwrap_or(0);
    let mut at = 0usize;
    while at < total {
        let n = write(fd, b.cast_const().wrapping_add(at).cast(), total.saturating_sub(at));
        match usize::try_from(n) {
            Ok(n) if n > 0 => at = at.saturating_add(n),
            _ if n < 0 && crate::errno::get() == EINTR => {}
            _ => {
                set_io(h.cast(), errno_err(rules::CANTSEND));
                return -1;
            }
        }
    }
    len
}

/// Reads the reply head word by word into `head` (only as far as its own counts say, so a
/// hostile verifier length cannot make it read more); returns the bytes read.
fn read_head(xs: *mut Xdr, head: &mut [u8; HEAD_MAX]) -> usize {
    let mut at = 0usize;
    let mut word = |head: &mut [u8; HEAD_MAX]| -> Option<u32> {
        let end = at.checked_add(4)?;
        let dst = head.get_mut(at..end)?;
        if !get_bytes(xs, dst) {
            return None;
        }
        at = end;
        Some(u32::from_be_bytes(<[u8; 4]>::try_from(&*dst).ok()?))
    };
    let _ = (|| -> Option<()> {
        let (_, _, stat) = (word(head)?, word(head)?, word(head)?);
        if stat == 0 {
            let (_, len) = (word(head)?, word(head)?);
            if len > 400 {
                return None;
            }
            for _ in 0..len.div_ceil(4) {
                word(head)?;
            }
            if word(head)? == 2 {
                word(head)?;
                word(head)?;
            }
        } else if stat == 1 {
            match word(head)? {
                0 => {
                    word(head)?;
                    word(head)?;
                }
                1 => {
                    word(head)?;
                }
                _ => {}
            }
        }
        Some(())
    })();
    at
}

/// One call over the stream; replies for other xids are skipped record by record.
#[allow(clippy::too_many_arguments)]
fn t_exchange(p: *mut Ct, c: *mut Client, proc_: u32, xargs: XdrProc, argsp: *mut c_char, xres: XdrProc, resp: *mut c_char, timeout: Timeval) -> RpcErrC {
    let (auth, ops) = auth_ops(c);
    let mut hdr = [0u8; 64];
    let (xid, total, n) = with_ct(p, |ct| {
        ct.xid = call_xid(ct.xid, &mut ct.xid_set);
        (ct.xid, ct.total.unwrap_or(timeout).millis(), rules::call_header(ct.xid, ct.prog, ct.vers, proc_, &mut hdr))
    });
    let Some(n) = n else { return errc(rules::CANTENCODEARGS, 0) };
    with_ct(p, |ct| {
        ct.io = RpcErrC::default();
        ct.deadline = now_ms().saturating_add(total.max(0));
    });
    let xs = xs_of(p);
    xs_set_op(xs, XDR_ENCODE);
    let marshal = ops.and_then(|o| o.ah_marshal);
    let encoded = put_bytes(xs, hdr.get(..n).unwrap_or_default()) && marshal.is_some_and(|m| m(auth, xs) != 0) && xargs.is_some_and(|f| f(xs, argsp.cast()) != 0);
    if !encoded {
        let e = io_or(p, rules::CANTENCODEARGS);
        // The partial record must not prefix the next call: start a fresh stream.
        renew_stream(p);
        return e;
    }
    // Batched (documented RPC batching; probed): no results routine and a zero timeout.
    let batch = xres.is_none() && total == 0;
    if liella_compat_xdrrec_endofrecord(xs, c_int::from(!batch)) == 0 {
        return io_or(p, rules::CANTSEND);
    }
    if batch {
        return errc(clnt_stat::SUCCESS, 0);
    }
    if total <= 0 {
        return errc(rules::TIMEDOUT, 0);
    }
    xs_set_op(xs, XDR_DECODE);
    loop {
        if liella_compat_xdrrec_skiprecord(xs) == 0 {
            return io_or(p, rules::CANTRECV);
        }
        let mut head = [0u8; HEAD_MAX];
        let got = read_head(xs, &mut head);
        let h = head.get(..got).unwrap_or_default();
        match rules::decode_reply(h, xid) {
            Outcome::Ignore => {}
            Outcome::CantDecode => return io_or(p, rules::CANTDECODERES),
            Outcome::Error(e) => return rpc_err(e),
            Outcome::Success { results, .. } => {
                if let Some(v) = ops.and_then(|o| o.ah_validate) {
                    let verf = verifier(h, results);
                    let mut oa = OpaqueAuth { oa_flavor: c_int::from_ne_bytes(verf.0.to_ne_bytes()), oa_base: verf.1.as_ptr().cast_mut().cast(), oa_length: c_uint::try_from(verf.1.len()).unwrap_or(0) };
                    if v(auth, &mut oa) == 0 {
                        return errc(clnt_stat::AUTHERROR, AUTH_INVALIDRESP);
                    }
                }
                // The results are decoded from the stream itself (no buffering cap). A reply
                // counts once its record ends: the rest is consumed, and end of input before the
                // last fragment is RPC_CANTRECV (as probed on the host glibc).
                return match xres {
                    Some(f) if f(xs, resp.cast()) != 0 => {
                        liella_compat_xdrrec_eof(xs);
                        io_or(p, clnt_stat::SUCCESS)
                    }
                    _ => io_or(p, rules::CANTDECODERES),
                };
            }
        }
    }
}

extern "C" fn t_call(c: *mut Client, proc_: c_ulong, xargs: XdrProc, argsp: *mut c_char, xres: XdrProc, resp: *mut c_char, timeout: Timeval) -> c_int {
    let Some(p) = ct_raw(c) else { return rules::CANTSEND };
    let e = t_exchange(p, c, u32::try_from(proc_).unwrap_or(u32::MAX), xargs, argsp, xres, resp, timeout);
    let st = e.re_status;
    with_ct(p, |ct| ct.err = e);
    st
}

// UNSAFE(slice): the caller's rpc_err.
#[allow(unsafe_code)]
extern "C" fn t_geterr(c: *mut Client, e: *mut RpcErrC) {
    let Some(p) = ct_raw(c) else { return };
    // UNSAFE(slice) SAFETY: e is NULL or the caller's rpc_err.
    if let Some(dst) = unsafe { e.as_mut() } {
        with_ct(p, |ct| *dst = RpcErrC { re_status: ct.err.re_status, a: ct.err.a, b: ct.err.b });
    }
}

/// Replaces the record stream with a fresh one (same sizes as at create: the defaults).
fn renew_stream(p: *mut Ct) {
    let xs = xs_of(p);
    xs_destroy(xs);
    with_ct(p, |ct| ct.xs = mem(&mut [], XDR_ENCODE));
    liella_compat_xdrrec_create(xs, 0, 0, p.cast(), Some(ct_read), Some(ct_write));
}

extern "C" fn t_destroy(c: *mut Client) {
    let Some(p) = ct_raw(c) else { return };
    if with_ct(p, |ct| ct.closeit) {
        close(with_ct(p, |ct| ct.sock));
    }
    xs_destroy(xs_of(p));
    heap::free_entry(p.addr());
    heap::free_entry(c.addr());
}

extern "C" fn t_control(c: *mut Client, req: c_int, info: *mut c_char) -> c_int {
    let Some(p) = ct_raw(c) else { return 0 };
    if info.is_null() && !matches!(req, CLSET_FD_CLOSE | CLSET_FD_NCLOSE) {
        return 0;
    }
    with_ct(p, |ct| {
        match req {
            CLSET_TIMEOUT => ct.total = Some(ctl_get(info)),
            // Probed: with no timeout set the request succeeds and writes nothing.
            CLGET_TIMEOUT => {
                if let Some(t) = ct.total {
                    ctl_put(info, t);
                }
            }
            CLGET_SERVER_ADDR => ctl_put(info, ct.addr),
            CLGET_FD => ctl_put(info, ct.sock),
            CLSET_FD_CLOSE => ct.closeit = true,
            CLSET_FD_NCLOSE => ct.closeit = false,
            CLGET_XID => ctl_put(info, ct.xid),
            CLSET_XID => (ct.xid, ct.xid_set) = (ctl_get::<u32>(info).wrapping_sub(1), true),
            CLGET_VERS => ctl_put(info, c_ulong::from(ct.vers)),
            CLGET_PROG => ctl_put(info, c_ulong::from(ct.prog)),
            // The retry timeouts are UDP's (probed: 0 here).
            _ => return 0,
        }
        1
    })
}

/// `IPPROTO_TCP`.
pub const IPPROTO_TCP: c_ulong = 6;

// UNSAFE(slice): fresh blocks of Ct's and Client's size.
#[allow(unsafe_code)]
fn new_ct(sock: c_int, closeit: bool, addr: [u8; 16], prog: u32, vers: u32, sendsz: c_uint, recvsz: c_uint) -> *mut Client {
    let ct = heap::malloc_entry(core::mem::size_of::<Ct>()).cast::<Ct>();
    let c = heap::malloc_entry(core::mem::size_of::<Client>()).cast::<Client>();
    if !ct.is_null() && !c.is_null() {
        let seed = fresh_xid(0);
        let xs = mem(&mut [], XDR_ENCODE);
        // UNSAFE(slice) SAFETY: ct is a fresh block of Ct's size, malloc-aligned (16).
        unsafe { ct.write(Ct { sock, closeit, addr, prog, vers, xid: seed, xid_set: false, total: None, err: RpcErrC::default(), deadline: 0, io: RpcErrC::default(), xs }) };
        let xs = xs_of(ct);
        xs_set_op(xs, XDR_ENCODE);
        liella_compat_xdrrec_create(xs, sendsz, recvsz, ct.cast(), Some(ct_read), Some(ct_write));
        if with_ct(ct, |t| !ptr::eq(t.xs.x_ops, &super::s29_xdr_streams::REC_OPS)) {
            heap::free_entry(ct.addr());
            heap::free_entry(c.addr());
            return ptr::null_mut();
        }
        write_client(c, ct.cast(), &CLNTTCP_OPS);
        return c;
    }
    for q in [ct.addr(), c.addr()] {
        if q != 0 {
            heap::free_entry(q);
        }
    }
    ptr::null_mut()
}

crate::internal_entry! {
/// `clnttcp_create`: a TCP client for (prog, vers) at `raddr` (port mapper lookup when its port
/// is 0, stored back). `RPC_ANYSOCK`: a socket is made (reserved port tried), stored in `*sockp`
/// and connected (probed: stored even when the connect fails), and destroy closes it; a caller's
/// socket must be connected already. Records go through `xdrrec` (owner ruling 10-23: no cap).
pub extern "C" fn liella_compat_clnttcp_create(raddr: *mut c_void, prog: c_ulong, vers: c_ulong, sockp: *mut c_int, sendsz: c_uint, recvsz: c_uint) -> *mut Client {
    let (Some(mut sin), Some(sock)) = (sin_of(raddr), sock_in(sockp)) else {
        set_createerr(clnt_stat::SYSTEMERROR, errc(0, 22));
        return ptr::null_mut();
    };
    let (p, v) = (u32::try_from(prog).unwrap_or(u32::MAX), u32::try_from(vers).unwrap_or(u32::MAX));
    if sin.get(2..4) == Some(&[0, 0]) {
        let port = liella_compat_pmap_getport(raddr, prog, vers, c_uint::try_from(IPPROTO_TCP).unwrap_or(6));
        if port == 0 {
            return ptr::null_mut();
        }
        set_port(&mut sin, port);
        sin_store(raddr, sin);
    }
    let (s, closeit) = if sock == RPC_ANYSOCK {
        let s = socket(2, 1, 0);
        if s < 0 {
            set_createerr(clnt_stat::SYSTEMERROR, errno_err(0));
            return ptr::null_mut();
        }
        let _ = liella_compat_bindresvport(s, ptr::null_mut());
        sock_out(sockp, s);
        if connect(s, sin.as_ptr().cast(), 16) < 0 {
            set_createerr(clnt_stat::SYSTEMERROR, errno_err(0));
            close(s);
            return ptr::null_mut();
        }
        (s, true)
    } else {
        (sock, false)
    };
    let c = new_ct(s, closeit, sin, p, v, sendsz, recvsz);
    if c.is_null() {
        if closeit {
            close(s);
        }
        set_createerr(clnt_stat::SYSTEMERROR, errc(0, 12));
    }
    c
}}
