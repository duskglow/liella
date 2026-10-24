//! Translation for spec §29.1: the XDR record-marking and stdio streams (LG-13).
//!
//! Like the memory stream (`s29_legacy`), these are internal `liella_compat_*` entries (R15-1,
//! R15-2). The record stream's logic (fragments, buffer sizes, refills) is
//! `liella_legacy::sunrpc_rec::RecStream`; this layer keeps it with the caller's handle and
//! `readit`/`writeit` callbacks in a heap block that `x_private` points at. The stdio stream is
//! `fread`/`fwrite`/`ftell`/`fseek`/`fflush` over the caller's `FILE`. Behaviour record:
//! `spec/behaviour/xdr_streams.glibc.txt`, `spec/behaviour/xdr_streams_rec2.glibc.txt`.

use core::ptr;

use liella_abi::ctypes::{c_char, c_int, c_long, c_uint, c_void};
use liella_abi::layout::FILE;
use liella_legacy::sunrpc::{self as rules, XDR_DECODE, XDR_ENCODE};
use liella_legacy::sunrpc_rec::RecStream;

use super::s11_lowio::lseek;
use super::s12_stdio::{fflush, fread, fseek, ftell, fwrite};
use super::s29_legacy::{Xdr, XdrOps};
use crate::heap;

const SEEK_SET: c_int = 0;
const SEEK_CUR: c_int = 1;

/// `readit` / `writeit`: `(handle, buffer, length)`, returning the byte count.
pub type RecIo = Option<extern "C" fn(*mut c_char, *mut c_char, c_int) -> c_int>;

/// The record stream's private block (`x_private`).
#[derive(Debug)]
struct RecState {
    stream: RecStream,
    handle: *mut c_char,
    readit: RecIo,
    writeit: RecIo,
}

/// The record stream's operations: `x_ops` of every stream `liella_compat_xdrrec_create` sets up
/// (public so in-process tests can name a stream's table without reading through `x_ops`).
pub static REC_OPS: XdrOps = XdrOps {
    x_getlong: Some(rec_getlong),
    x_putlong: Some(rec_putlong),
    x_getbytes: Some(rec_getbytes),
    x_putbytes: Some(rec_putbytes),
    x_getpostn: Some(rec_getpostn),
    x_setpostn: Some(rec_setpostn),
    x_inline: Some(rec_inline),
    x_destroy: Some(rec_destroy),
    x_getint32: Some(rec_getint32),
    x_putint32: Some(rec_putint32),
};

/// The stdio stream's operations: `x_ops` of every stream `liella_compat_xdrstdio_create` sets up.
pub static STDIO_OPS: XdrOps = XdrOps {
    x_getlong: Some(stdio_getlong),
    x_putlong: Some(stdio_putlong),
    x_getbytes: Some(stdio_getbytes),
    x_putbytes: Some(stdio_putbytes),
    x_getpostn: Some(stdio_getpostn),
    x_setpostn: Some(stdio_setpostn),
    x_inline: Some(stdio_inline),
    x_destroy: Some(stdio_destroy),
    x_getint32: Some(stdio_getint32),
    x_putint32: Some(stdio_putint32),
};

/// A C length for a Rust slice: lengths past `c_int::MAX` are asked for as `c_int::MAX`.
fn c_len(n: usize) -> c_int {
    c_int::try_from(n).unwrap_or(c_int::MAX)
}

/// The stream's direction.
// UNSAFE(slice): the stream is a C pointer (§29.1).
#[allow(unsafe_code)]
fn op_of(x: *const Xdr) -> c_int {
    // UNSAFE(slice) SAFETY: x is the caller's XDR (every operation's contract).
    unsafe { (*x).x_op }
}

// --- the record stream -------------------------------------------------------------------

/// The record stream's state behind `x`.
// UNSAFE(slice): the stream and its private block are C pointers (§29.1).
#[allow(unsafe_code)]
fn rec_state<'a>(x: *const Xdr) -> &'a mut RecState {
    // UNSAFE(slice) SAFETY: x is a record stream (the caller's XDR); its x_private is the block
    // xdrrec_create wrote, live until x_destroy.
    let p = unsafe { (*x).x_private };
    // UNSAFE(slice) SAFETY: as above; the stream is used by one thread at a time (its contract).
    unsafe { &mut *p.cast::<RecState>() }
}

/// Runs `f` with the stream and closures over the caller's callbacks.
fn with_rec<T>(x: *const Xdr, f: impl FnOnce(&mut RecStream, &mut dyn FnMut(&mut [u8]) -> i64, &mut dyn FnMut(&[u8]) -> i64) -> T) -> T {
    let st = rec_state(x);
    let (handle, readit, writeit) = (st.handle, st.readit, st.writeit);
    // The caller's callbacks, called with its handle and a buffer of the stated length.
    let mut rd = |b: &mut [u8]| readit.map_or(-1, |g| i64::from(g(handle, b.as_mut_ptr().cast(), c_len(b.len()))));
    let mut wr = |b: &[u8]| writeit.map_or(-1, |g| i64::from(g(handle, b.as_ptr().cast_mut().cast(), c_len(b.len()))));
    f(&mut st.stream, &mut rd, &mut wr)
}

fn rec_get_word(x: *mut Xdr) -> Option<u32> {
    let mut b = [0; 4];
    with_rec(x, |s, r, _| s.get(&mut b, r)).then(|| u32::from_be_bytes(b))
}

fn rec_put_word(x: *mut Xdr, w: u32) -> c_int {
    c_int::from(with_rec(x, |s, _, wr| s.put(&w.to_be_bytes(), wr)))
}

// UNSAFE(slice): the stream and the objects are C pointers (§29.1).
#[allow(unsafe_code)]
extern "C" fn rec_getlong(x: *mut Xdr, lp: *mut c_long) -> c_int {
    rec_get_word(x).map_or(0, |w| {
        // UNSAFE(slice) SAFETY: lp is the caller's long (x_getlong's contract).
        unsafe { lp.write(rules::long_from_wire(w)) };
        1
    })
}

// UNSAFE(slice): the stream and the objects are C pointers (§29.1).
#[allow(unsafe_code)]
extern "C" fn rec_putlong(x: *mut Xdr, lp: *const c_long) -> c_int {
    // UNSAFE(slice) SAFETY: lp is the caller's long (x_putlong's contract).
    let l = unsafe { lp.read() };
    rec_put_word(x, rules::low32(l))
}

// UNSAFE(slice): the stream and the objects are C pointers (§29.1).
#[allow(unsafe_code)]
extern "C" fn rec_getint32(x: *mut Xdr, ip: *mut i32) -> c_int {
    rec_get_word(x).map_or(0, |w| {
        // UNSAFE(slice) SAFETY: ip is the caller's int32 (x_getint32's contract).
        unsafe { ip.write(i32::from_be_bytes(w.to_be_bytes())) };
        1
    })
}

// UNSAFE(slice): the stream and the objects are C pointers (§29.1).
#[allow(unsafe_code)]
extern "C" fn rec_putint32(x: *mut Xdr, ip: *const i32) -> c_int {
    // UNSAFE(slice) SAFETY: ip is the caller's int32 (x_putint32's contract).
    let i = unsafe { ip.read() };
    rec_put_word(x, u32::from_be_bytes(i.to_be_bytes()))
}

// UNSAFE(slice): the stream and the objects are C pointers (§29.1).
#[allow(unsafe_code)]
extern "C" fn rec_getbytes(x: *mut Xdr, addr: *mut c_char, len: c_uint) -> c_int {
    if len == 0 {
        return 1;
    }
    // UNSAFE(slice) SAFETY: addr has room for len bytes (x_getbytes's contract).
    let buf = unsafe { core::slice::from_raw_parts_mut(addr.cast::<u8>(), rules::count(len)) };
    c_int::from(with_rec(x, |s, r, _| s.get(buf, r)))
}

// UNSAFE(slice): the stream and the objects are C pointers (§29.1).
#[allow(unsafe_code)]
extern "C" fn rec_putbytes(x: *mut Xdr, addr: *const c_char, len: c_uint) -> c_int {
    if len == 0 {
        return 1;
    }
    // UNSAFE(slice) SAFETY: addr holds len bytes (x_putbytes's contract).
    let data = unsafe { core::slice::from_raw_parts(addr.cast::<u8>(), rules::count(len)) };
    c_int::from(with_rec(x, |s, _, w| s.put(data, w)))
}

/// The handle as a descriptor (its low 32 bits): the descriptor's offset plus the buffered
/// output (encode) or minus the unread input (decode); `u32::MAX` when the offset is unknown.
extern "C" fn rec_getpostn(x: *const Xdr) -> c_uint {
    let st = rec_state(x);
    let fd = c_int::from_ne_bytes(u32::try_from(st.handle.addr() & 0xffff_ffff).unwrap_or(0).to_ne_bytes());
    let Ok(at) = u64::try_from(lseek(fd, 0, SEEK_CUR)) else { return c_uint::MAX };
    let pos = match op_of(x) {
        XDR_ENCODE => at.wrapping_add(u64::try_from(st.stream.out_pending()).unwrap_or(0)),
        XDR_DECODE => at.wrapping_sub(u64::try_from(st.stream.in_pending()).unwrap_or(0)),
        _ => return c_uint::MAX,
    };
    u32::try_from(pos & 0xffff_ffff).unwrap_or(c_uint::MAX)
}

extern "C" fn rec_setpostn(_x: *mut Xdr, _pos: c_uint) -> c_int {
    0
}

extern "C" fn rec_inline(x: *mut Xdr, len: c_uint) -> *mut i32 {
    let st = rec_state(x);
    let n = rules::count(len);
    let (range, buf) = match op_of(x) {
        XDR_ENCODE => (st.stream.put_inline(n), st.stream.out_bytes()),
        XDR_DECODE => (st.stream.get_inline(n), st.stream.in_bytes()),
        _ => return ptr::null_mut(),
    };
    range.and_then(|r| buf.get_mut(r)).map_or(ptr::null_mut(), |s| s.as_mut_ptr().cast())
}

// UNSAFE(slice): the stream and its private block are C pointers (§29.1).
#[allow(unsafe_code)]
extern "C" fn rec_destroy(x: *mut Xdr) {
    // UNSAFE(slice) SAFETY: x is a record stream (the caller's XDR).
    let p = unsafe { (*x).x_private }.cast::<RecState>();
    // UNSAFE(slice) SAFETY: p is the block xdrrec_create wrote; it is dropped once, here.
    unsafe { ptr::drop_in_place(p) };
    heap::free_entry(p.addr());
}

crate::internal_entry! {
/// `xdrrec_create`: a record stream over the caller's callbacks; `x_op` is the caller's. When
/// the buffers cannot be had the stream is left as it was.
pub extern "C" fn liella_compat_xdrrec_create(x: *mut Xdr, sendsize: c_uint, recvsize: c_uint, handle: *mut c_char, readit: RecIo, writeit: RecIo) {
    let Some(stream) = RecStream::new(sendsize, recvsize) else { return };
    let p = heap::malloc_entry(core::mem::size_of::<RecState>()).cast::<RecState>();
    if p.is_null() {
        return;
    }
    // UNSAFE(slice) SAFETY: p is a fresh block of RecState's size, malloc-aligned (16).
    unsafe { p.write(RecState { stream, handle, readit, writeit }) };
    // UNSAFE(slice) SAFETY: x is the caller's XDR (xdrrec_create's contract).
    let xr = unsafe { &mut *x };
    xr.x_ops = &REC_OPS;
    xr.x_private = p.cast();
}}

crate::internal_entry! {
/// `xdrrec_endofrecord`: ends the record being encoded; sends it now when `sendnow` is set.
pub extern "C" fn liella_compat_xdrrec_endofrecord(x: *mut Xdr, sendnow: c_int) -> c_int {
    c_int::from(with_rec(x, |s, _, w| s.end_of_record(sendnow != 0, w)))
}}

crate::internal_entry! {
/// `xdrrec_skiprecord`: skips the rest of the record being decoded and starts the next.
pub extern "C" fn liella_compat_xdrrec_skiprecord(x: *mut Xdr) -> c_int {
    c_int::from(with_rec(x, |s, r, _| s.skip_record(r)))
}}

crate::internal_entry! {
/// `xdrrec_eof`: consumes the rest of the record; 1 when no more input is buffered.
pub extern "C" fn liella_compat_xdrrec_eof(x: *mut Xdr) -> c_int {
    c_int::from(with_rec(x, |s, r, _| s.eof(r)))
}}

// --- the stdio stream --------------------------------------------------------------------

// UNSAFE(slice): the stream is a C pointer (§29.1).
#[allow(unsafe_code)]
fn file(x: *const Xdr) -> *mut FILE {
    // UNSAFE(slice) SAFETY: x is a stdio stream (the caller's XDR); x_private is its FILE.
    unsafe { (*x).x_private }.cast()
}

fn stdio_get_word(x: *mut Xdr) -> Option<u32> {
    let mut b = [0u8; 4];
    (fread(b.as_mut_ptr().cast(), 4, 1, file(x)) == 1).then(|| u32::from_be_bytes(b))
}

fn stdio_put_word(x: *mut Xdr, w: u32) -> c_int {
    let b = w.to_be_bytes();
    c_int::from(fwrite(b.as_ptr().cast(), 4, 1, file(x)) == 1)
}

// UNSAFE(slice): the stream and the objects are C pointers (§29.1).
#[allow(unsafe_code)]
extern "C" fn stdio_getlong(x: *mut Xdr, lp: *mut c_long) -> c_int {
    stdio_get_word(x).map_or(0, |w| {
        // UNSAFE(slice) SAFETY: lp is the caller's long (x_getlong's contract).
        unsafe { lp.write(rules::long_from_wire(w)) };
        1
    })
}

// UNSAFE(slice): the stream and the objects are C pointers (§29.1).
#[allow(unsafe_code)]
extern "C" fn stdio_putlong(x: *mut Xdr, lp: *const c_long) -> c_int {
    // UNSAFE(slice) SAFETY: lp is the caller's long (x_putlong's contract).
    let l = unsafe { lp.read() };
    stdio_put_word(x, rules::low32(l))
}

// UNSAFE(slice): the stream and the objects are C pointers (§29.1).
#[allow(unsafe_code)]
extern "C" fn stdio_getint32(x: *mut Xdr, ip: *mut i32) -> c_int {
    stdio_get_word(x).map_or(0, |w| {
        // UNSAFE(slice) SAFETY: ip is the caller's int32 (x_getint32's contract).
        unsafe { ip.write(i32::from_be_bytes(w.to_be_bytes())) };
        1
    })
}

// UNSAFE(slice): the stream and the objects are C pointers (§29.1).
#[allow(unsafe_code)]
extern "C" fn stdio_putint32(x: *mut Xdr, ip: *const i32) -> c_int {
    // UNSAFE(slice) SAFETY: ip is the caller's int32 (x_putint32's contract).
    let i = unsafe { ip.read() };
    stdio_put_word(x, u32::from_be_bytes(i.to_be_bytes()))
}

extern "C" fn stdio_getbytes(x: *mut Xdr, addr: *mut c_char, len: c_uint) -> c_int {
    c_int::from(len == 0 || fread(addr.cast::<c_void>(), rules::count(len), 1, file(x)) == 1)
}

extern "C" fn stdio_putbytes(x: *mut Xdr, addr: *const c_char, len: c_uint) -> c_int {
    c_int::from(len == 0 || fwrite(addr.cast::<c_void>(), rules::count(len), 1, file(x)) == 1)
}

/// `ftell`, truncated to 32 bits (`-1` reads as `u32::MAX`).
extern "C" fn stdio_getpostn(x: *const Xdr) -> c_uint {
    let at = ftell(file(x));
    u32::try_from(at & 0xffff_ffff).unwrap_or(c_uint::MAX)
}

extern "C" fn stdio_setpostn(x: *mut Xdr, pos: c_uint) -> c_int {
    c_int::from(fseek(file(x), i64::from(pos), SEEK_SET) == 0)
}

extern "C" fn stdio_inline(_x: *mut Xdr, _len: c_uint) -> *mut i32 {
    ptr::null_mut()
}

extern "C" fn stdio_destroy(x: *mut Xdr) {
    fflush(file(x));
}

crate::internal_entry! {
/// `xdrstdio_create`: a stream over the caller's `FILE` in direction `op`.
pub extern "C" fn liella_compat_xdrstdio_create(x: *mut Xdr, fp: *mut FILE, op: c_int) {
    // UNSAFE(slice) SAFETY: x is the caller's XDR (xdrstdio_create's contract).
    let xr = unsafe { &mut *x };
    xr.x_op = op;
    xr.x_ops = &STDIO_OPS;
    xr.x_private = fp.cast();
}}
