//! `liella_secure_zero`: zeroing the compiler cannot remove (D-X.11). It is the one
//! implementation: `explicit_bzero` and `__explicit_bzero_chk` call it, and the heap wipes a
//! secure block (`liella_heap_alloc_secure`) with the same function (`conv::secure_zero`).
//!
//! **The guarantee.** When the call returns, each of the `len` bytes at `ptr` has been stored
//! as zero by it, whether the call is compiled separately, inlined or link-time optimised into
//! its caller, and even if the caller never reads the bytes again. The stores are made by
//! ordinary code and then the address is passed to an assembly function of the C layer
//! (`csys/src/machine/barrier.S`, OD-C1) that no optimiser can see into: the compiler must
//! assume it reads the object, so the stores cannot be removed, shortened or moved after it.
//! `cargo xtask native` checks this on an LTO build (`examples/secure_zero_codegen.rs`).
//! What it does not cover: copies of the data elsewhere (registers, spilled temporaries, other
//! buffers, swap), and ordering for other threads (it is not a synchronisation operation).

use core::ffi::c_void;

use super::abi;
use crate::conv;
use crate::export;

export! { "LIELLA_NATIVE_1";
/// Zero the `len` bytes at `ptr` so that the stores are never optimised away: 0, or `-EFAULT`
/// for NULL with a non-zero length or a length above `PTRDIFF_MAX` (nothing is written then).
/// NULL with length 0 does nothing.
pub extern "C" fn liella_secure_zero(ptr: *mut c_void, len: usize) -> i64 {
    // UNSAFE(slice) SAFETY: ptr is valid for writing len bytes (the native slice convention).
    abi::done(unsafe { conv::buf_mut(ptr, len) }.map(conv::secure_zero))
}}
