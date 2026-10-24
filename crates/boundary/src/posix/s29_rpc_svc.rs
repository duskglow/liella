//! Translation for spec §29.1: the ONC RPC server transports (LG-14, D-29.1).
//!
//! Internal `liella_compat_*` entries like the XDR layer (R15-1, R15-2). Every rule applied to
//! the network's bytes (call header, credentials, routing, reply header, TCP record marking) is
//! `liella_legacy::sunrpc_svc`, bounds-checked with typed errors; this layer owns the sockets,
//! the `SVCXPRT` handles, the (program, version) registry and the read loop. Transports: UDP
//! (`svcudp_create`, `svcudp_bufcreate`), TCP (`svctcp_create`, and `svcfd_create` for a
//! connected stream). A stream's records are read through an `xdrrec` stream (owner ruling
//! 2026-10-23): the call header goes into a fixed 840-byte area, the arguments are decoded
//! straight off the stream, so no record is buffered whole and a connection's memory is bounded
//! by its buffer sizes whatever lengths a peer claims.

use core::ptr;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering::Relaxed};

use liella_rt::sync::RawMutex;

use liella_abi::ctypes::{c_char, c_int, c_uint, c_ulong, c_void};
use liella_legacy::sunrpc::{XDR_DECODE, XDR_ENCODE};
use liella_legacy::sunrpc_svc::{self as rules, Auth, Reply, RpcErr};

use super::s11_lowio::{close, poll, read, write};
use super::s15_sockets::{accept, bind, getsockname, listen, recvfrom, sendto, socket};
use super::s29_legacy::{Xdr, XdrProc, liella_compat_xdr_free, liella_compat_xdrmem_create};
use super::s29_xdr_streams::{REC_OPS, liella_compat_xdrrec_create, liella_compat_xdrrec_endofrecord, liella_compat_xdrrec_eof, liella_compat_xdrrec_skiprecord};
use crate::heap;

/// `RPC_ANYSOCK`: make the socket.
pub const RPC_ANYSOCK: c_int = -1;
/// `UDPMSGSIZE`: the UDP buffers when the caller gives 0.
const UDP_SIZE: c_uint = 8800;
/// The `xdrrec` buffer sizes of a stream transport when the caller gives 0.
const TCP_SIZE: c_uint = 4000;
/// A stream transport's reply-header area: the longest accepted or denied header (verifier of
/// `MAX_AUTH_BYTES`) fits.
const REPLY_HEADER_MAX: usize = 512;
/// `xprt_stat`: another call is already buffered.
pub const XPRT_MOREREQS: c_int = 1;
/// The registry's capacity: descriptors below `FD_SETSIZE`, (program, version) pairs.
const FD_SETSIZE: usize = 1024;
const MAX_CALLOUTS: usize = 128;
/// `xprt_stat`.
pub const XPRT_DIED: c_int = 0;
/// `xprt_stat`: nothing more to read.
pub const XPRT_IDLE: c_int = 2;
/// `AUTH_TOOWEAK` (`svcerr_weakauth`).
const AUTH_TOOWEAK: u32 = 5;
const POLLIN: i16 = 1;
const POLLNVAL: i16 = 0x20;
/// The seconds a connection may stall inside one record.
const STALL_MS: c_int = 35_000;

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

/// `SVCXPRT` (336 bytes on x86_64).
#[repr(C)]
#[derive(Debug)]
pub struct SvcXprt {
    /// `xp_sock`.
    pub xp_sock: c_int,
    /// `xp_port` (host order).
    pub xp_port: u16,
    /// `xp_ops`.
    pub xp_ops: *const XpOps,
    /// `xp_addrlen`: the caller's address length (UDP).
    pub xp_addrlen: c_int,
    /// `xp_raddr`: the caller's address.
    pub xp_raddr: [u8; 16],
    /// `xp_verf`: the verifier of the replies.
    pub xp_verf: OpaqueAuth,
    /// `xp_p1`: the transport's private block.
    pub xp_p1: *mut c_void,
    /// `xp_p2`.
    pub xp_p2: *mut c_void,
    /// `xp_pad`.
    pub xp_pad: [u8; 256],
}

/// `struct xp_ops`.
#[repr(C)]
#[derive(Debug)]
#[allow(missing_docs)]
pub struct XpOps {
    pub xp_recv: Option<extern "C" fn(*mut SvcXprt, *mut RpcMsg) -> c_int>,
    pub xp_stat: Option<extern "C" fn(*mut SvcXprt) -> c_int>,
    pub xp_getargs: Option<extern "C" fn(*mut SvcXprt, XdrProc, *mut c_void) -> c_int>,
    pub xp_reply: Option<extern "C" fn(*mut SvcXprt, *mut RpcMsg) -> c_int>,
    pub xp_freeargs: Option<extern "C" fn(*mut SvcXprt, XdrProc, *mut c_void) -> c_int>,
    pub xp_destroy: Option<extern "C" fn(*mut SvcXprt)>,
}

/// `struct rpc_msg` (96 bytes) as twelve 8-byte words. Call: xid 0, direction 1, rpcvers 2,
/// prog 3, vers 4, proc 5, cred 6..9, verf 9..12. Reply: stat 2; accepted: verf 3..6, ar_stat
/// 6, results `where` 7 and `proc` 8 (or low 7, high 8); denied: rj_stat 3, low or why 4, high 5.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
pub struct RpcMsg {
    /// The words.
    pub w: [u64; 12],
}

impl RpcMsg {
    fn at(&self, i: usize) -> u64 {
        self.w.get(i).copied().unwrap_or(0)
    }
    fn set(&mut self, i: usize, v: u64) {
        if let Some(s) = self.w.get_mut(i) {
            *s = v;
        }
    }
    /// The low 32 bits of word `i` (an `int`/`enum` member).
    fn int(&self, i: usize) -> u32 {
        u32::try_from(self.at(i) & 0xffff_ffff).unwrap_or(0)
    }
}

/// `struct svc_req` (64 bytes).
#[repr(C)]
#[derive(Debug)]
pub struct SvcReq {
    /// `rq_prog`.
    pub rq_prog: c_ulong,
    /// `rq_vers`.
    pub rq_vers: c_ulong,
    /// `rq_proc`.
    pub rq_proc: c_ulong,
    /// `rq_cred`.
    pub rq_cred: OpaqueAuth,
    /// `rq_clntcred`: an `authunix_parms` for `AUTH_UNIX`.
    pub rq_clntcred: *mut c_void,
    /// `rq_xprt`.
    pub rq_xprt: *mut SvcXprt,
}

/// `struct authunix_parms` with the storage it points at (360 bytes, inside the 400-byte
/// `RQCRED_SIZE` area `_authenticate` is given).
#[repr(C)]
#[derive(Debug)]
pub struct CredArea {
    /// `aup_time`.
    pub aup_time: c_ulong,
    /// `aup_machname`.
    pub aup_machname: *mut c_char,
    /// `aup_uid`.
    pub aup_uid: u32,
    /// `aup_gid`.
    pub aup_gid: u32,
    /// `aup_len`.
    pub aup_len: c_uint,
    /// `aup_gids`.
    pub aup_gids: *mut u32,
    machname: [u8; 256],
    gids: [u32; rules::NGRPS],
}

/// A dispatch routine.
pub type Dispatch = Option<extern "C" fn(*mut SvcReq, *mut SvcXprt)>;

/// One (program, version) mapping; `dispatch` is the routine's address (0 = a free slot).
struct Callout {
    prog: AtomicU64,
    vers: AtomicU64,
    dispatch: AtomicUsize,
}

impl Callout {
    fn is(&self, prog: c_ulong, vers: c_ulong) -> bool {
        self.prog.load(Relaxed) == prog && self.vers.load(Relaxed) == vers
    }
    fn used(&self) -> bool {
        self.dispatch.load(Relaxed) != 0
    }
    fn set(&self, prog: c_ulong, vers: c_ulong, dispatch: usize) {
        self.prog.store(prog, Relaxed);
        self.vers.store(vers, Relaxed);
        self.dispatch.store(dispatch, Relaxed);
    }
    // UNSAFE(slice) SAFETY: a non-zero `dispatch` was stored by svc_register from a `Dispatch`
    // routine (dispatch_addr), so it is that function's address.
    #[allow(unsafe_code)]
    fn routine(&self) -> Dispatch {
        let a = self.dispatch.load(Relaxed);
        // SAFETY: see above; zero is excluded by the guard, and any other value is a Dispatch address.
        (a != 0).then(|| unsafe { core::mem::transmute::<usize, extern "C" fn(*mut SvcReq, *mut SvcXprt)>(a) })
    }
}

/// The registry; every field is changed only under `REG_LOCK` (atomics so it needs no `unsafe impl`).
struct Registry {
    callouts: [Callout; MAX_CALLOUTS],
    /// Registered transports by descriptor, as exposed addresses (0 = none).
    xports: [AtomicUsize; FD_SETSIZE],
    fdset: [AtomicU64; FD_SETSIZE / 64],
}

fn xp_at(r: &Registry, i: usize) -> *mut SvcXprt {
    r.xports.get(i).map_or(ptr::null_mut(), |a| ptr::with_exposed_provenance_mut(a.load(Relaxed)))
}

#[allow(clippy::as_conversions)] // a function pointer's address: the only spelling Rust has
fn dispatch_addr(d: Dispatch) -> usize {
    d.map_or(0, |f| f as usize)
}

#[allow(clippy::as_conversions)] // a function pointer's address: the only spelling Rust has
fn proc_addr(p: XdrProc) -> u64 {
    u64::try_from(p.map_or(0, |f| f as usize)).unwrap_or(0)
}

static REG: Registry = Registry {
    callouts: [const { Callout { prog: AtomicU64::new(0), vers: AtomicU64::new(0), dispatch: AtomicUsize::new(0) } }; MAX_CALLOUTS],
    xports: [const { AtomicUsize::new(0) }; FD_SETSIZE],
    fdset: [const { AtomicU64::new(0) }; FD_SETSIZE / 64],
};
static REG_LOCK: RawMutex = RawMutex::new();

/// The registry under its lock (never held across a dispatch or a socket call).
fn with<R>(f: impl FnOnce(&Registry) -> R) -> R {
    let _g = REG_LOCK.lock(&crate::sys::CSys);
    f(&REG)
}

fn slot(fd: c_int) -> Option<usize> {
    usize::try_from(fd).ok().filter(|i| *i < FD_SETSIZE)
}

fn bit(set: &[AtomicU64; FD_SETSIZE / 64], i: usize, on: bool) {
    if let Some(w) = set.get(i / 64) {
        let m = 1u64 << (i % 64);
        if on {
            w.fetch_or(m, Relaxed);
        } else {
            w.fetch_and(!m, Relaxed);
        }
    }
}

/// A heap buffer.
struct Buf {
    p: *mut u8,
    cap: usize,
}

impl Buf {
    const EMPTY: Self = Self { p: ptr::null_mut(), cap: 0 };

    fn new(cap: usize) -> Option<Self> {
        let p = heap::malloc_entry(cap).cast::<u8>();
        (!p.is_null()).then_some(Self { p, cap })
    }

    // UNSAFE(slice): the block and its capacity.
    #[allow(unsafe_code)]
    fn bytes(&mut self) -> &mut [u8] {
        if self.p.is_null() {
            return &mut [];
        }
        // UNSAFE(slice) SAFETY: p is a live block of cap bytes owned by this Buf.
        unsafe { core::slice::from_raw_parts_mut(self.p, self.cap) }
    }

    fn free(&mut self) {
        if !self.p.is_null() {
            heap::free_entry(self.p.addr());
        }
        *self = Self::EMPTY;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Udp,
    Listen,
    Conn,
}

/// A transport's private block (`xp_p1`).
struct Priv {
    kind: Kind,
    sendsz: c_uint,
    recvsz: c_uint,
    inbuf: Buf,
    outbuf: Buf,
    /// The bytes of the message in `inbuf`, and where its arguments start.
    len: usize,
    args: usize,
    xid: u32,
    /// Set by the stream callbacks when the peer closed, stalled or failed (read by `x_stat`).
    died: AtomicBool,
    /// A stream connection's `xdrrec` stream (handle: the transport); unused otherwise.
    rec: Xdr,
}

/// The one operations table; each entry branches on the transport's kind.
pub static SVC_OPS: XpOps = XpOps {
    xp_recv: Some(x_recv),
    xp_stat: Some(x_stat),
    xp_getargs: Some(x_getargs),
    xp_reply: Some(x_reply),
    xp_freeargs: Some(x_freeargs),
    xp_destroy: Some(x_destroy),
};

// UNSAFE(slice): a transport made here (`xp_ops` is `SVC_OPS`) and its private block.
#[allow(unsafe_code)]
fn private<'a>(x: *mut SvcXprt) -> Option<&'a mut Priv> {
    // UNSAFE(slice) SAFETY: x is a live SVCXPRT (the xp_ops contract).
    let xr = unsafe { x.as_ref() }?;
    if !ptr::eq(xr.xp_ops, &SVC_OPS) {
        return None;
    }
    // UNSAFE(slice) SAFETY: xp_p1 of an SVC_OPS transport is its Priv (make wrote it).
    unsafe { xr.xp_p1.cast::<Priv>().as_mut() }
}

// UNSAFE(slice): the caller's SVCXPRT.
#[allow(unsafe_code)]
fn xprt<'a>(x: *mut SvcXprt) -> Option<&'a mut SvcXprt> {
    // UNSAFE(slice) SAFETY: x is NULL or a live SVCXPRT (the C contract).
    unsafe { x.as_mut() }
}

// UNSAFE(slice): bytes the caller's message points at.
#[allow(unsafe_code)]
fn bytes_at<'a>(p: u64, len: u32) -> Option<&'a [u8]> {
    if len > rules::MAX_AUTH_BYTES {
        return None;
    }
    let n = usize::try_from(len).ok()?;
    if n == 0 {
        return Some(&[]);
    }
    let p = ptr::with_exposed_provenance::<u8>(usize::try_from(p).ok().filter(|a| *a != 0)?);
    // UNSAFE(slice) SAFETY: an opaque_auth's base holds its length (at most 400) bytes.
    Some(unsafe { core::slice::from_raw_parts(p, n) })
}

fn word_of(p: *const u8) -> u64 {
    u64::try_from(p.addr()).unwrap_or(0)
}

/// Waits for `fd` to be readable (`STALL_MS` at most).
fn ready(fd: c_int) -> bool {
    let mut pfd = [PollFd { fd, events: POLLIN, revents: 0 }];
    poll(pfd.as_mut_ptr().cast(), 1, STALL_MS) > 0
}

// UNSAFE(slice): a transport's socket and private block as raw pointers (the xdrrec callbacks run
// while the stream inside the block is in use, so they never take a reference to the whole block).
#[allow(unsafe_code)]
fn conn_raw(x: *const SvcXprt) -> Option<(c_int, *mut Priv)> {
    // UNSAFE(slice) SAFETY: x is NULL or a live SVCXPRT; three fields are copied out of it.
    let x = unsafe { x.as_ref() }?;
    ptr::eq(x.xp_ops, &SVC_OPS).then_some((x.xp_sock, x.xp_p1.cast::<Priv>()))
}

// UNSAFE(slice): one field of a live private block.
#[allow(unsafe_code)]
fn died_of<'a>(p: *mut Priv) -> &'a AtomicBool {
    // UNSAFE(slice) SAFETY: p is a live Priv (conn_raw); only this field is referenced.
    unsafe { &(*p).died }
}

// UNSAFE(slice): the address of a stream transport's xdrrec stream.
#[allow(unsafe_code)]
fn rec_of(x: *const SvcXprt) -> Option<*mut Xdr> {
    let (_, p) = conn_raw(x)?;
    // UNSAFE(slice) SAFETY: p is a live Priv; its kind is read and rec's address taken, no reference.
    (unsafe { (*p).kind } == Kind::Conn && unsafe { !(*p).rec.x_ops.is_null() }).then(|| unsafe { &raw mut (*p).rec })
}

// UNSAFE(slice): the direction of a stream transport's own xdrrec stream.
#[allow(unsafe_code)]
fn set_op(xs: *mut Xdr, op: c_int) {
    // UNSAFE(slice) SAFETY: xs is rec_of's live stream.
    unsafe { (*xs).x_op = op };
}

/// `readit` of a stream transport (handle: the transport): one read once the socket is readable
/// (`STALL_MS` at most); end of input, a stall or an error marks the transport dead.
extern "C" fn conn_read(h: *mut c_char, buf: *mut c_char, len: c_int) -> c_int {
    let Some((fd, p)) = conn_raw(h.cast()) else { return -1 };
    let n = if ready(fd) { read(fd, buf.cast(), usize::try_from(len).unwrap_or(0)) } else { -1 };
    match c_int::try_from(n) {
        Ok(n) if n > 0 => n,
        _ => {
            died_of(p).store(true, Relaxed);
            -1
        }
    }
}

// UNSAFE(slice): the bytes the record stream hands its writer.
#[allow(unsafe_code)]
extern "C" fn conn_write(h: *mut c_char, buf: *mut c_char, len: c_int) -> c_int {
    let Some((fd, p)) = conn_raw(h.cast()) else { return -1 };
    // UNSAFE(slice) SAFETY: buf holds len bytes (the writeit contract).
    let b = unsafe { core::slice::from_raw_parts(buf.cast::<u8>().cast_const(), usize::try_from(len).unwrap_or(0)) };
    if write_all(fd, b) {
        return len;
    }
    died_of(p).store(true, Relaxed);
    -1
}

fn write_all(fd: c_int, buf: &[u8]) -> bool {
    let mut at = 0usize;
    while let Some(rest) = buf.get(at..).filter(|r| !r.is_empty()) {
        let Ok(n) = usize::try_from(write(fd, rest.as_ptr().cast(), rest.len())) else { return false };
        if n == 0 {
            return false;
        }
        at = at.saturating_add(n);
    }
    true
}

/// Starts the next record of a stream transport and reads its call header into `inbuf` (at most
/// `CALL_HEADER_MAX` bytes, each auth length checked before its body is read); the arguments
/// stay in the stream for `x_getargs`.
fn recv_stream(x: *mut SvcXprt, xs: *mut Xdr) -> bool {
    if liella_compat_xdrrec_skiprecord(xs) == 0 {
        return false;
    }
    let mut hdr = [0u8; rules::CALL_HEADER_MAX];
    let get = REC_OPS.x_getbytes;
    let got = |b: &mut [u8]| get.is_some_and(|g| g(xs, b.as_mut_ptr().cast(), c_uint::try_from(b.len()).unwrap_or(0)) != 0);
    let Some(n) = rules::read_call_header(got, &mut hdr) else { return false };
    let Some(p) = private(x) else { return false };
    let (Some(d), Some(s)) = (p.inbuf.bytes().get_mut(..n), hdr.get(..n)) else { return false };
    d.copy_from_slice(s);
    p.len = n;
    true
}

/// Encodes one reply onto a stream transport's record stream and sends the record; a failure
/// leaves a part-written record, so the connection is marked dead.
fn send_stream(x: *mut SvcXprt, xs: *mut Xdr, verf: &Auth<'_>, reply: Reply, results: Option<(XdrProc, *mut c_void)>) -> bool {
    let Some(xid) = private(x).map(|p| p.xid) else { return false };
    let mut hdr = [0u8; REPLY_HEADER_MAX];
    let Some(n) = rules::reply_header(xid, verf, reply, &mut hdr) else { return false };
    let put = REC_OPS.x_putbytes;
    set_op(xs, XDR_ENCODE);
    let ok = put.is_some_and(|f| f(xs, hdr.as_ptr().cast(), c_uint::try_from(n).unwrap_or(0)) != 0)
        && results.is_none_or(|(proc_, objp)| proc_.is_none_or(|f| f(xs, objp) != 0))
        && liella_compat_xdrrec_endofrecord(xs, 1) != 0;
    set_op(xs, XDR_DECODE);
    if !ok && let Some((_, p)) = conn_raw(x) {
        died_of(p).store(true, Relaxed);
    }
    ok
}

/// Decodes the message in `inbuf` into `msg` (credential and verifier point into `inbuf`).
fn fill(p: &mut Priv, msg: &mut RpcMsg) -> bool {
    let len = p.len;
    let Some(m) = p.inbuf.bytes().get(..len) else { return false };
    let Ok(c) = rules::decode_call(m) else { return false };
    let (xid, args) = (c.xid, c.args);
    for (i, v) in [c.xid, rules::MSG_CALL, c.rpcvers, c.prog, c.vers, c.proc_].into_iter().enumerate() {
        msg.set(i, u64::from(v));
    }
    for (i, a) in [(6usize, &c.cred), (9, &c.verf)] {
        msg.set(i, u64::from(a.flavor));
        msg.set(i.saturating_add(1), word_of(a.body.as_ptr()));
        msg.set(i.saturating_add(2), u64::try_from(a.body.len()).unwrap_or(0));
    }
    p.xid = xid;
    p.args = args;
    true
}

extern "C" fn x_recv(x: *mut SvcXprt, msg: *mut RpcMsg) -> c_int {
    let streamed = rec_of(x).map(|xs| recv_stream(x, xs));
    let (Some(xr), Some(p)) = (xprt(x), private(x)) else { return 0 };
    let ok = match p.kind {
        Kind::Udp => {
            let mut alen: c_uint = 16;
            let b = p.inbuf.bytes();
            let n = recvfrom(xr.xp_sock, b.as_mut_ptr().cast(), b.len(), 0, xr.xp_raddr.as_mut_ptr().cast(), Some(&mut alen));
            let Ok(n) = usize::try_from(n) else { return 0 };
            xr.xp_addrlen = c_int::try_from(alen).unwrap_or(0);
            p.len = n;
            true
        }
        Kind::Listen => {
            let mut raddr = [0u8; 16];
            let mut alen: c_uint = 16;
            let fd = accept(xr.xp_sock, raddr.as_mut_ptr().cast(), Some(&mut alen));
            if fd >= 0 {
                let c = liella_compat_svcfd_create(fd, p.sendsz, p.recvsz);
                if let Some(cr) = xprt(c) {
                    cr.xp_raddr = raddr;
                    cr.xp_addrlen = c_int::try_from(alen).unwrap_or(0);
                } else {
                    close(fd);
                }
            }
            return 0;
        }
        Kind::Conn => streamed == Some(true),
    };
    // UNSAFE(slice) SAFETY: msg is the caller's rpc_msg (the xp_recv contract).
    #[allow(unsafe_code)]
    let Some(m) = (unsafe { msg.as_mut() }) else { return 0 };
    c_int::from(ok && fill(p, m))
}

extern "C" fn x_stat(x: *mut SvcXprt) -> c_int {
    let Some((_, p)) = conn_raw(x) else { return XPRT_DIED };
    // A stream finishes the record just served; whatever is still buffered is the next call.
    let more = rec_of(x).is_some_and(|xs| liella_compat_xdrrec_eof(xs) == 0);
    if died_of(p).load(Relaxed) {
        XPRT_DIED
    } else if more {
        XPRT_MOREREQS
    } else {
        XPRT_IDLE
    }
}

fn mem_stream(b: &mut [u8], op: c_int) -> Xdr {
    let mut x = Xdr { x_op: op, x_ops: ptr::null(), x_public: ptr::null_mut(), x_private: ptr::null_mut(), x_base: ptr::null_mut(), x_handy: 0 };
    liella_compat_xdrmem_create(&mut x, b.as_mut_ptr().cast(), c_uint::try_from(b.len()).unwrap_or(c_uint::MAX), op);
    x
}

extern "C" fn x_getargs(x: *mut SvcXprt, proc_: XdrProc, objp: *mut c_void) -> c_int {
    if let Some(xs) = rec_of(x) {
        return proc_.map_or(0, |f| f(xs, objp));
    }
    let Some(p) = private(x) else { return 0 };
    let (args, len) = (p.args, p.len);
    let Some(b) = p.inbuf.bytes().get_mut(args..len) else { return 0 };
    let mut s = mem_stream(b, XDR_DECODE);
    proc_.map_or(0, |f| f(&mut s, objp))
}

extern "C" fn x_freeargs(_x: *mut SvcXprt, proc_: XdrProc, objp: *mut c_void) -> c_int {
    liella_compat_xdr_free(proc_, objp.cast());
    1
}

// UNSAFE(slice): the caller's message and the result filter stored in it.
#[allow(unsafe_code)]
extern "C" fn x_reply(x: *mut SvcXprt, msg: *mut RpcMsg) -> c_int {
    // UNSAFE(slice) SAFETY: msg is the caller's rpc_msg (the xp_reply contract).
    let Some(m) = (unsafe { msg.as_ref() }) else { return 0 };
    let (reply, results) = match m.int(2) {
        rules::MSG_ACCEPTED => {
            let (lo, hi) = (u32::try_from(m.at(7)).unwrap_or(u32::MAX), u32::try_from(m.at(8)).unwrap_or(u32::MAX));
            // UNSAFE(slice) SAFETY: word 8 of a SUCCESS reply is the caller's xdrproc_t (or NULL).
            let proc_ = unsafe { core::mem::transmute::<u64, XdrProc>(m.at(8)) };
            match m.int(6) {
                0 => (Reply::Success, Some((proc_, ptr::with_exposed_provenance_mut::<c_void>(usize::try_from(m.at(7)).unwrap_or(0))))),
                1 => (Reply::ProgUnavail, None),
                2 => (Reply::ProgMismatch(lo, hi), None),
                3 => (Reply::ProcUnavail, None),
                4 => (Reply::GarbageArgs, None),
                5 => (Reply::SystemErr, None),
                _ => return 0,
            }
        }
        rules::MSG_DENIED => match m.int(3) {
            0 => (Reply::RpcMismatch(u32::try_from(m.at(4)).unwrap_or(u32::MAX), u32::try_from(m.at(5)).unwrap_or(u32::MAX)), None),
            1 => (Reply::AuthError(m.int(4)), None),
            _ => return 0,
        },
        _ => return 0,
    };
    let Some(body) = bytes_at(m.at(4), m.int(5)) else { return 0 };
    let verf = Auth { flavor: m.int(3), body };
    c_int::from(send(x, &verf, reply, results))
}

/// Encodes and sends one reply on one of this layer's transports.
fn send(x: *mut SvcXprt, verf: &Auth<'_>, reply: Reply, results: Option<(XdrProc, *mut c_void)>) -> bool {
    if let Some(xs) = rec_of(x) {
        return send_stream(x, xs, verf, reply, results);
    }
    let (Some(xr), Some(p)) = (xprt(x), private(x)) else { return false };
    if p.kind != Kind::Udp {
        return false;
    }
    let xid = p.xid;
    let out = p.outbuf.bytes();
    let Some(n) = rules::reply_header(xid, verf, reply, out) else { return false };
    let mut total = n;
    if let Some((proc_, objp)) = results {
        let Some(rest) = out.get_mut(total..) else { return false };
        let mut s = mem_stream(rest, XDR_ENCODE);
        // A datagram has its size: results that do not fit are not sent.
        if !proc_.is_none_or(|f| f(&mut s, objp) != 0) {
            return false;
        }
        total = total.saturating_add(s.x_private.addr().saturating_sub(s.x_base.addr()));
    }
    let Some(msg) = p.outbuf.bytes().get(..total) else { return false };
    let alen = c_uint::try_from(xr.xp_addrlen).unwrap_or(0);
    usize::try_from(sendto(xr.xp_sock, msg.as_ptr().cast(), total, 0, xr.xp_raddr.as_ptr().cast(), alen)).is_ok_and(|s| s == total)
}

extern "C" fn x_destroy(x: *mut SvcXprt) {
    liella_compat_xprt_unregister(x);
    if let (Some(xs), Some(d)) = (rec_of(x), REC_OPS.x_destroy) {
        d(xs);
    }
    let Some(xr) = xprt(x) else { return };
    close(xr.xp_sock);
    if let Some(p) = private(x) {
        p.inbuf.free();
        p.outbuf.free();
        heap::free_entry(xr.xp_p1.addr());
    }
    heap::free_entry(x.addr());
}

#[repr(C)]
#[derive(Clone, Copy)]
struct PollFd {
    fd: c_int,
    events: i16,
    revents: i16,
}

/// The local port of `sock`, binding it to an ephemeral port first when it has none.
fn port(sock: c_int) -> Option<u16> {
    let mut sa = [0u8; 16];
    let mut len: c_uint = 16;
    if getsockname(sock, sa.as_mut_ptr().cast(), Some(&mut len)) != 0 {
        return None;
    }
    if sa.get(2..4)? == [0, 0] {
        let any: [u8; 16] = [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        if bind(sock, any.as_ptr().cast(), 16) != 0 || getsockname(sock, sa.as_mut_ptr().cast(), Some(&mut len)) != 0 {
            return None;
        }
    }
    Some(u16::from_be_bytes([*sa.get(2)?, *sa.get(3)?]))
}

fn round4(n: c_uint, dflt: c_uint) -> c_uint {
    let n = if n == 0 { dflt } else { n };
    n.saturating_add(3) & !3
}

// UNSAFE(slice): fresh heap blocks of the handle's and the private block's sizes.
#[allow(unsafe_code)]
fn make(sock: c_int, kind: Kind, sendsz: c_uint, recvsz: c_uint, xp_port: u16) -> *mut SvcXprt {
    // A stream keeps only its call header here (the xdrrec stream has the record buffers).
    let (isz, osz) = if kind == Kind::Conn { (rules::CALL_HEADER_MAX, 4) } else { (usize::try_from(recvsz).unwrap_or(0), usize::try_from(sendsz).unwrap_or(0)) };
    let bufs = if kind == Kind::Listen {
        Some((Buf::EMPTY, Buf::EMPTY))
    } else {
        let i = Buf::new(isz);
        let o = Buf::new(osz);
        match (i, o) {
            (Some(i), Some(o)) => Some((i, o)),
            (Some(mut i), None) | (None, Some(mut i)) => {
                i.free();
                None
            }
            (None, None) => None,
        }
    };
    let Some((inbuf, outbuf)) = bufs else { return ptr::null_mut() };
    let x = heap::malloc_entry(core::mem::size_of::<SvcXprt>()).cast::<SvcXprt>();
    let pv = heap::malloc_entry(core::mem::size_of::<Priv>()).cast::<Priv>();
    if x.is_null() || pv.is_null() {
        let (mut i, mut o) = (inbuf, outbuf);
        i.free();
        o.free();
        for b in [x.addr(), pv.addr()] {
            if b != 0 {
                heap::free_entry(b);
            }
        }
        return ptr::null_mut();
    }
    let rec = Xdr { x_op: XDR_DECODE, x_ops: ptr::null(), x_public: ptr::null_mut(), x_private: ptr::null_mut(), x_base: ptr::null_mut(), x_handy: 0 };
    // UNSAFE(slice) SAFETY: pv is a fresh block of Priv's size, malloc-aligned (16).
    unsafe { pv.write(Priv { kind, sendsz, recvsz, inbuf, outbuf, len: 0, args: 0, xid: 0, died: AtomicBool::new(false), rec }) };
    let verf = OpaqueAuth { oa_flavor: 0, oa_base: ptr::null_mut(), oa_length: 0 };
    // UNSAFE(slice) SAFETY: x is a fresh block of SvcXprt's size, malloc-aligned (16).
    unsafe {
        x.write(SvcXprt { xp_sock: sock, xp_port, xp_ops: &SVC_OPS, xp_addrlen: 0, xp_raddr: [0; 16], xp_verf: verf, xp_p1: pv.cast(), xp_p2: ptr::null_mut(), xp_pad: [0; 256] });
    };
    if kind == Kind::Conn {
        // UNSAFE(slice) SAFETY: pv is the Priv written above; rec's address is taken, no reference.
        let xs = unsafe { &raw mut (*pv).rec };
        liella_compat_xdrrec_create(xs, sendsz, recvsz, x.cast(), Some(conn_read), Some(conn_write));
        if rec_of(x).is_none() {
            if let Some(p) = private(x) {
                p.inbuf.free();
                p.outbuf.free();
            }
            heap::free_entry(pv.addr());
            heap::free_entry(x.addr());
            return ptr::null_mut();
        }
    }
    liella_compat_xprt_register(x);
    x
}

fn open(sock: c_int, kind: c_int) -> Option<(c_int, bool)> {
    if sock != RPC_ANYSOCK {
        return Some((sock, false));
    }
    let s = socket(2, kind, 0);
    (s >= 0).then_some((s, true))
}

crate::internal_entry! {
/// `svcudp_bufcreate`: a UDP transport on `sock` (made and bound when `RPC_ANYSOCK`), with
/// datagram buffers of the given sizes (8800 when 0); registered for reading.
pub extern "C" fn liella_compat_svcudp_bufcreate(sock: c_int, sendsz: c_uint, recvsz: c_uint) -> *mut SvcXprt {
    let Some((s, made)) = open(sock, 2) else { return ptr::null_mut() };
    let x = port(s).map_or(ptr::null_mut(), |pt| make(s, Kind::Udp, round4(sendsz, UDP_SIZE), round4(recvsz, UDP_SIZE), pt));
    if x.is_null() && made {
        close(s);
    }
    x
}}

crate::internal_entry! {
/// `svcudp_create`: `svcudp_bufcreate` with the default sizes.
pub extern "C" fn liella_compat_svcudp_create(sock: c_int) -> *mut SvcXprt {
    liella_compat_svcudp_bufcreate(sock, UDP_SIZE, UDP_SIZE)
}}

crate::internal_entry! {
/// `svctcp_create`: a listening TCP transport; each accepted connection becomes a
/// `svcfd_create` transport with these buffer sizes.
pub extern "C" fn liella_compat_svctcp_create(sock: c_int, sendsize: c_uint, recvsize: c_uint) -> *mut SvcXprt {
    let Some((s, made)) = open(sock, 1) else { return ptr::null_mut() };
    let x = match port(s) {
        Some(pt) if listen(s, 4096) == 0 => make(s, Kind::Listen, round4(sendsize, TCP_SIZE), round4(recvsize, TCP_SIZE), pt),
        _ => ptr::null_mut(),
    };
    if x.is_null() && made {
        close(s);
    }
    x
}}

crate::internal_entry! {
/// `svcfd_create`: a transport over the connected stream `fd` (TCP record marking).
pub extern "C" fn liella_compat_svcfd_create(fd: c_int, sendsize: c_uint, recvsize: c_uint) -> *mut SvcXprt {
    make(fd, Kind::Conn, round4(sendsize, TCP_SIZE), round4(recvsize, TCP_SIZE), 0)
}}

crate::internal_entry! {
/// `xprt_register`: reads from the transport's socket from now on (descriptors below 1024).
pub extern "C" fn liella_compat_xprt_register(x: *mut SvcXprt) {
    let Some(i) = xprt(x).and_then(|xr| slot(xr.xp_sock)) else { return };
    with(|r| {
        if let Some(s) = r.xports.get(i) {
            s.store(x.expose_provenance(), Relaxed);
            bit(&r.fdset, i, true);
        }
    });
}}

crate::internal_entry! {
/// `xprt_unregister`: stops reading from the transport's socket (if it is the one registered).
pub extern "C" fn liella_compat_xprt_unregister(x: *mut SvcXprt) {
    let Some(i) = xprt(x).and_then(|xr| slot(xr.xp_sock)) else { return };
    with(|r| {
        if let Some(s) = r.xports.get(i).filter(|s| s.load(Relaxed) == x.addr()) {
            s.store(0, Relaxed);
            bit(&r.fdset, i, false);
        }
    });
}}

crate::internal_entry! {
/// `svc_register`: maps (`prog`, `vers`) to `dispatch`. False when the pair is mapped to
/// another routine or the table is full. A non-zero `protocol` also asks the portmapper, which
/// is LG-15 (`pmap_set`): until then such a call keeps the mapping and answers false.
pub extern "C" fn liella_compat_svc_register(_x: *mut SvcXprt, prog: c_ulong, vers: c_ulong, dispatch: Dispatch, protocol: c_int) -> c_int {
    let ok = with(|r| {
        if let Some(c) = r.callouts.iter().find(|c| c.used() && c.is(prog, vers)) {
            return c.dispatch.load(Relaxed) == dispatch_addr(dispatch);
        }
        dispatch.is_some() && r.callouts.iter().find(|c| !c.used()).map(|c| c.set(prog, vers, dispatch_addr(dispatch))).is_some()
    });
    c_int::from(ok && protocol == 0)
}}

crate::internal_entry! {
/// `svc_unregister`: removes the mapping of (`prog`, `vers`).
pub extern "C" fn liella_compat_svc_unregister(prog: c_ulong, vers: c_ulong) {
    with(|r| r.callouts.iter().filter(|c| c.is(prog, vers)).for_each(|c| c.set(0, 0, 0)));
}}

/// Fills `area` from an `AUTH_UNIX` credential.
fn unix_into(u: &rules::UnixCred<'_>, area: &mut CredArea) {
    area.machname.fill(0);
    if let Some(d) = area.machname.get_mut(..u.machine.len()) {
        d.copy_from_slice(u.machine);
    }
    area.gids = u.gids;
    area.aup_time = c_ulong::from(u.stamp);
    area.aup_uid = u.uid;
    area.aup_gid = u.gid;
    area.aup_len = c_uint::try_from(u.ngids).unwrap_or(0);
    area.aup_machname = area.machname.as_mut_ptr().cast();
    area.aup_gids = area.gids.as_mut_ptr();
}

/// `_authenticate`'s rule: `AUTH_OK` (0) or the `auth_stat` to refuse with.
fn authenticate(req: &mut SvcReq, area: Option<&mut CredArea>) -> u32 {
    let Some(body) = bytes_at(word_of(req.rq_cred.oa_base.cast()), req.rq_cred.oa_length) else { return rules::AUTH_BADCRED };
    let cred = Auth { flavor: u32::from_ne_bytes(req.rq_cred.oa_flavor.to_ne_bytes()), body };
    match rules::authenticate(&cred) {
        Ok(Some(u)) => {
            if let Some(a) = area {
                unix_into(&u, a);
            }
            0
        }
        Ok(None) => 0,
        Err(why) => why,
    }
}

fn auth_of(m: &RpcMsg, i: usize) -> OpaqueAuth {
    OpaqueAuth { oa_flavor: c_int::from_ne_bytes(m.int(i).to_ne_bytes()), oa_base: ptr::with_exposed_provenance_mut::<c_char>(usize::try_from(m.at(i.saturating_add(1))).unwrap_or(0)), oa_length: m.int(i.saturating_add(2)) }
}

crate::internal_entry! {
/// `_authenticate`: checks the call's credential; for `AUTH_UNIX` writes the parsed
/// `authunix_parms` (and its strings) into `rq_clntcred` (a 400-byte area) when it is not NULL.
pub extern "C" fn liella_compat__authenticate(req: *mut SvcReq, msg: *const RpcMsg) -> c_int {
    // UNSAFE(slice) SAFETY: req and msg are the caller's (the _authenticate contract).
    let (Some(rq), Some(m)) = (unsafe { req.as_mut() }, unsafe { msg.as_ref() }) else { return c_int::from(rules::AUTH_BADCRED != 0) };
    rq.rq_cred = auth_of(m, 6);
    // UNSAFE(slice) SAFETY: rq_clntcred is NULL or the caller's 400-byte RQCRED area.
    let area = unsafe { rq.rq_clntcred.cast::<CredArea>().as_mut() };
    if let Some(x) = xprt(rq.rq_xprt) {
        x.xp_verf = OpaqueAuth { oa_flavor: 0, oa_base: ptr::null_mut(), oa_length: 0 };
    }
    c_int::try_from(authenticate(rq, area)).unwrap_or(c_int::MAX)
}}

/// Answers `reply` on `x` with no results (any transport: through its `xp_reply`).
// UNSAFE(slice): the transport's operations table.
#[allow(unsafe_code)]
fn answer(x: *mut SvcXprt, reply: Reply, results: Option<(XdrProc, *mut c_void)>) -> c_int {
    let Some(xr) = xprt(x) else { return 0 };
    let mut m = RpcMsg::default();
    m.set(1, u64::from(rules::MSG_REPLY));
    let v = xr.xp_verf;
    let mut acc = |stat: u64, a: u64, b: u64| {
        m.set(2, u64::from(rules::MSG_ACCEPTED));
        m.set(3, u64::from(u32::from_ne_bytes(v.oa_flavor.to_ne_bytes())));
        m.set(4, word_of(v.oa_base.cast()));
        m.set(5, u64::from(v.oa_length));
        m.set(6, stat);
        m.set(7, a);
        m.set(8, b);
    };
    match reply {
        Reply::Success => {
            let (p, o) = results.unwrap_or((None, ptr::null_mut()));
            acc(0, u64::try_from(o.expose_provenance()).unwrap_or(0), proc_addr(p));
        }
        Reply::ProgUnavail => acc(1, 0, 0),
        Reply::ProgMismatch(lo, hi) => acc(2, u64::from(lo), u64::from(hi)),
        Reply::ProcUnavail => acc(3, 0, 0),
        Reply::GarbageArgs => acc(4, 0, 0),
        Reply::SystemErr => acc(5, 0, 0),
        Reply::RpcMismatch(lo, hi) => {
            m.set(2, u64::from(rules::MSG_DENIED));
            m.set(4, u64::from(lo));
            m.set(5, u64::from(hi));
        }
        Reply::AuthError(why) => {
            m.set(2, u64::from(rules::MSG_DENIED));
            m.set(3, 1);
            m.set(4, u64::from(why));
        }
    }
    // UNSAFE(slice) SAFETY: xp_ops of a live transport is its operations table.
    let Some(f) = (unsafe { xr.xp_ops.as_ref() }).and_then(|o| o.xp_reply) else { return 0 };
    f(x, &mut m)
}

crate::internal_entry! {
/// `svc_sendreply`: a SUCCESS reply with `proc_` encoding the results at `objp`.
pub extern "C" fn liella_compat_svc_sendreply(x: *mut SvcXprt, proc_: XdrProc, objp: *mut c_char) -> c_int {
    answer(x, Reply::Success, Some((proc_, objp.cast())))
}}

macro_rules! svcerr {
    ($($doc:literal $name:ident($($a:ident: $t:ty),*) => $r:expr;)*) => {$(
        crate::internal_entry! {
        #[doc = $doc]
        pub extern "C" fn $name(x: *mut SvcXprt $(, $a: $t)*) {
            answer(x, $r, None);
        }}
    )*};
}

svcerr! {
    "`svcerr_noproc`: PROC_UNAVAIL." liella_compat_svcerr_noproc() => Reply::ProcUnavail;
    "`svcerr_decode`: GARBAGE_ARGS." liella_compat_svcerr_decode() => Reply::GarbageArgs;
    "`svcerr_systemerr`: SYSTEM_ERR." liella_compat_svcerr_systemerr() => Reply::SystemErr;
    "`svcerr_noprog`: PROG_UNAVAIL." liella_compat_svcerr_noprog() => Reply::ProgUnavail;
    "`svcerr_progvers`: PROG_MISMATCH with the versions served." liella_compat_svcerr_progvers(low: c_ulong, high: c_ulong) =>
        Reply::ProgMismatch(u32::try_from(low).unwrap_or(u32::MAX), u32::try_from(high).unwrap_or(u32::MAX));
    "`svcerr_auth`: AUTH_ERROR with `why`." liella_compat_svcerr_auth(why: c_int) => Reply::AuthError(u32::from_ne_bytes(why.to_ne_bytes()));
    "`svcerr_weakauth`: AUTH_ERROR AUTH_TOOWEAK." liella_compat_svcerr_weakauth() => Reply::AuthError(AUTH_TOOWEAK);
}

/// The registered routine for (`prog`, `vers`), or the reply when there is none.
fn lookup(prog: u64, vers: u64) -> Result<Dispatch, Reply> {
    with(|r| {
        if let Some(c) = r.callouts.iter().find(|c| c.used() && c.is(prog, vers)) {
            return Ok(c.routine());
        }
        let mut pairs = [(0u32, 0u32); MAX_CALLOUTS];
        let mut n = 0usize;
        for c in r.callouts.iter().filter(|c| c.used()) {
            if let (Ok(p), Ok(v), Some(s)) = (u32::try_from(c.prog.load(Relaxed)), u32::try_from(c.vers.load(Relaxed)), pairs.get_mut(n)) {
                *s = (p, v);
                n = n.saturating_add(1);
            }
        }
        let (p, v) = (u32::try_from(prog).unwrap_or(u32::MAX), u32::try_from(vers).unwrap_or(u32::MAX));
        Err(rules::route(pairs.get(..n).unwrap_or(&[]), p, v).unwrap_or(Reply::ProgUnavail))
    })
}

/// Answers one decoded call on `x`.
fn handle(x: *mut SvcXprt, m: &RpcMsg) {
    if m.int(2) != rules::RPC_VERS {
        answer(x, Reply::RpcMismatch(rules::RPC_VERS, rules::RPC_VERS), None);
        return;
    }
    let mut area = CredArea { aup_time: 0, aup_machname: ptr::null_mut(), aup_uid: 0, aup_gid: 0, aup_len: 0, aup_gids: ptr::null_mut(), machname: [0; 256], gids: [0; rules::NGRPS] };
    let mut req = SvcReq { rq_prog: m.at(3), rq_vers: m.at(4), rq_proc: m.at(5), rq_cred: auth_of(m, 6), rq_clntcred: ptr::null_mut(), rq_xprt: x };
    let why = authenticate(&mut req, Some(&mut area));
    if why != 0 {
        answer(x, Reply::AuthError(why), None);
        return;
    }
    if let Some(xr) = xprt(x) {
        xr.xp_verf = OpaqueAuth { oa_flavor: 0, oa_base: ptr::null_mut(), oa_length: 0 };
    }
    req.rq_clntcred = (&raw mut area).cast();
    match lookup(m.at(3), m.at(4)) {
        Ok(Some(f)) => f(&mut req, x),
        Ok(None) => {}
        Err(r) => {
            answer(x, r, None);
        }
    }
}

crate::internal_entry! {
/// `svc_getreq_common`: reads and answers what is waiting on `fd`; a stream transport whose peer
/// is gone (or broke the record rules) is destroyed.
pub extern "C" fn liella_compat_svc_getreq_common(fd: c_int) {
    let Some(i) = slot(fd) else { return };
    let x = with(|r| xp_at(r, i));
    let Some(xr) = xprt(x) else { return };
    // UNSAFE(slice) SAFETY: xp_ops of a registered transport is its operations table.
    let Some(ops) = (unsafe { xr.xp_ops.as_ref() }) else { return };
    loop {
        let mut m = RpcMsg::default();
        if ops.xp_recv.is_some_and(|f| f(x, &mut m) != 0) {
            handle(x, &m);
        }
        // The dispatch routine may have destroyed the transport (svc_exit, svc_destroy).
        if !with(|r| r.xports.get(i).is_some_and(|s| s.load(Relaxed) == x.addr())) {
            return;
        }
        // A stream may hold further calls already read off the socket: serve them now (poll
        // would not report them).
        match ops.xp_stat.map_or(XPRT_IDLE, |f| f(x)) {
            XPRT_DIED => {
                if let Some(d) = ops.xp_destroy {
                    d(x);
                }
                return;
            }
            XPRT_MOREREQS => {}
            _ => return,
        }
    }
}}

crate::internal_entry! {
/// `svc_getreqset`: `svc_getreq_common` for each descriptor in `readfds` (an `fd_set`).
pub extern "C" fn liella_compat_svc_getreqset(readfds: *const u64) {
    // UNSAFE(slice) SAFETY: readfds is the caller's fd_set (1024 bits).
    let Some(set) = (unsafe { readfds.cast::<[u64; FD_SETSIZE / 64]>().as_ref() }) else { return };
    let set = *set;
    for (wi, w) in set.iter().enumerate() {
        for b in 0..64usize {
            if w & (1u64 << b) != 0 {
                liella_compat_svc_getreq_common(c_int::try_from(wi.saturating_mul(64).saturating_add(b)).unwrap_or(-1));
            }
        }
    }
}}

crate::internal_entry! {
/// `svc_getreq`: `svc_getreqset` for the descriptors 0..32 in `rdfds`.
pub extern "C" fn liella_compat_svc_getreq(rdfds: c_int) {
    let mut set = [0u64; FD_SETSIZE / 64];
    set[0] = u64::from(u32::from_ne_bytes(rdfds.to_ne_bytes()));
    liella_compat_svc_getreqset(set.as_ptr());
}}

crate::internal_entry! {
/// `svc_getreq_poll`: `svc_getreq_common` for each of the first `nready` ready entries of
/// `pfd`; an entry with `POLLNVAL` unregisters its descriptor.
pub extern "C" fn liella_compat_svc_getreq_poll(pfd: *mut c_void, nready: c_int) {
    let n = usize::try_from(nready).unwrap_or(0);
    let mut seen = 0usize;
    let mut i = 0usize;
    while seen < n {
        // UNSAFE(slice) SAFETY: pfd holds at least nready ready entries (the caller's poll result).
        let at = pfd.cast::<PollFd>().wrapping_add(i);
        // UNSAFE(slice) SAFETY: `at` is inside the caller's array (see above).
        let e = unsafe { at.read() };
        i = i.saturating_add(1);
        if e.revents == 0 {
            continue;
        }
        seen = seen.saturating_add(1);
        if e.revents & POLLNVAL != 0 {
            let x = slot(e.fd).map_or(ptr::null_mut(), |s| with(|r| xp_at(r, s)));
            liella_compat_xprt_unregister(x);
        } else {
            liella_compat_svc_getreq_common(e.fd);
        }
    }
}}

crate::internal_entry! {
/// `svc_run`: waits for and answers calls until no transport is registered (`svc_exit`) or the
/// wait fails other than by a signal.
pub extern "C" fn liella_compat_svc_run() {
    loop {
        let mut pfds = [PollFd { fd: -1, events: 0, revents: 0 }; FD_SETSIZE];
        let n = with(|r| {
            let mut n = 0usize;
            for i in 0..FD_SETSIZE {
                if r.xports.get(i).is_some_and(|x| x.load(Relaxed) != 0) {
                    if let Some(p) = pfds.get_mut(n) {
                        *p = PollFd { fd: c_int::try_from(i).unwrap_or(-1), events: POLLIN, revents: 0 };
                    }
                    n = n.saturating_add(1);
                }
            }
            n
        });
        if n == 0 {
            return;
        }
        let rc = poll(pfds.as_mut_ptr().cast(), c_ulong::try_from(n).unwrap_or(0), -1);
        if rc < 0 {
            if crate::errno::get() == 4 {
                continue;
            }
            return;
        }
        liella_compat_svc_getreq_poll(pfds.as_mut_ptr().cast(), rc);
    }
}}

crate::internal_entry! {
/// `svc_exit`: destroys every registered transport and mapping, so `svc_run` returns.
pub extern "C" fn liella_compat_svc_exit() {
    for i in 0..FD_SETSIZE {
        let x = with(|r| xp_at(r, i));
        // UNSAFE(slice) SAFETY: xp_ops of a registered transport is its operations table.
        if let Some(d) = xprt(x).and_then(|xr| unsafe { xr.xp_ops.as_ref() }).and_then(|o| o.xp_destroy) {
            d(x);
        }
        liella_compat_xprt_unregister(x);
    }
    with(|r| r.callouts.iter().for_each(|c| c.set(0, 0, 0)));
}}

crate::internal_entry! {
/// `__rpc_thread_svc_fdset`: the set of registered descriptors (`svc_fdset`).
pub extern "C" fn liella_compat___rpc_thread_svc_fdset() -> *mut u64 {
    // A pointer into the static registry (read by the caller as an fd_set; the words are atomics).
    REG.fdset.as_ptr().cast::<u64>().cast_mut()
}}

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

crate::internal_entry! {
/// `_seterr_reply`: the client's view (`rpc_err`) of the reply in `msg`.
pub extern "C" fn liella_compat__seterr_reply(msg: *const RpcMsg, err: *mut RpcErrC) {
    // UNSAFE(slice) SAFETY: msg and err are the caller's (the _seterr_reply contract).
    let (Some(m), Some(e)) = (unsafe { msg.as_ref() }, unsafe { err.as_mut() }) else { return };
    let rp = m.int(2);
    let (stat, lo, hi) = if rp == rules::MSG_ACCEPTED { (m.int(6), m.at(7), m.at(8)) } else { (m.int(3), m.at(4), m.at(5)) };
    *e = match rules::seterr(rp, stat, lo, hi, m.int(4)) {
        RpcErr::Status(s) => RpcErrC { re_status: s, a: e.a, b: e.b },
        RpcErr::Versions(s, l, h) => RpcErrC { re_status: s, a: l, b: h },
        RpcErr::Why(w) => RpcErrC { re_status: rules::clnt_stat::AUTHERROR, a: u64::from(w), b: e.b },
        RpcErr::Failed(s1, s2) => RpcErrC { re_status: rules::clnt_stat::FAILED, a: u64::from_ne_bytes(s1.to_ne_bytes()), b: u64::from_ne_bytes(s2.to_ne_bytes()) },
    };
}}

/// The read side of a raw pointer word (for tests that build an `rpc_msg`).
#[must_use]
pub fn ptr_word(p: *const c_char) -> u64 {
    word_of(p.cast())
}
