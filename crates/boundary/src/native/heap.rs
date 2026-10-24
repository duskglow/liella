//! `liella_heap_*`: the process heap (the one behind `malloc` and, in the products, Rust's
//! global allocator), in the shape of Rust's `GlobalAlloc`. A block from any of these may be
//! released with `free` and vice versa: they take the C family's way into the heap
//! (`heap::c_*`, so `mcheck` and `mtrace` see them too). Addresses, or NULL when no block can be
//! had; misuse of a block is a diagnostic and abort, as for `free`. Secure blocks
//! (`liella_heap_alloc_secure`) are zeroed by the heap whenever they are released
//! (`liella_rt::heap`, D-X.11).

use core::ffi::c_void;

use liella_legacy::malloc_compat;

use super::abi;
use crate::conv;
use crate::export;
use crate::heap;

export! { "LIELLA_NATIVE_1";
/// `size` bytes aligned to `align` (a power of two; every block is aligned to at least
/// `LIELLA_HEAP_MIN_ALIGN`), or NULL. Size 0 gives a unique block.
pub extern "C" fn liella_heap_alloc(size: usize, align: usize) -> *mut c_void {
    abi::addr(heap::c_aligned_alloc(align, size))
}}

export! { "LIELLA_NATIVE_1";
/// As `liella_heap_alloc`, with the `size` bytes zeroed.
pub extern "C" fn liella_heap_alloc_zeroed(size: usize, align: usize) -> *mut c_void {
    abi::addr(heap::c_aligned_calloc(align, size))
}}

export! { "LIELLA_NATIVE_1";
/// A *secure* block (D-X.11): as `liella_heap_alloc` (alignment and size 0 alike), in a
/// mapping of its own that reads as zero and is kept out of core dumps. Whatever releases it
/// (`liella_heap_free`, `free`, `liella_heap_realloc` or `realloc` moving it) zeroes it before
/// its memory can be reused; a reallocation stays secure.
pub extern "C" fn liella_heap_alloc_secure(size: usize, align: usize) -> *mut c_void {
    abi::addr(malloc_compat::aligned_alloc_secure(&heap::CTX, align, size))
}}

export! { "LIELLA_NATIVE_1";
/// The block at `ptr` resized to `size` bytes, its contents kept up to the smaller size; NULL
/// on failure, the block then unchanged. As `realloc`: NULL `ptr` allocates, size 0 frees and
/// gives NULL. The result is aligned to `LIELLA_HEAP_MIN_ALIGN` only; a block that needs more
/// is moved by the caller (allocate, copy, free). A secure block stays secure, and the bytes
/// past its old usable size read as zero; moved, the old block is zeroed.
pub extern "C" fn liella_heap_realloc(ptr: *mut c_void, size: usize) -> *mut c_void {
    abi::addr(heap::checked("realloc", heap::c_realloc(conv::addr(ptr), size)))
}}

export! { "LIELLA_NATIVE_1";
/// Release the block at `ptr` (NULL: nothing).
pub extern "C" fn liella_heap_free(ptr: *mut c_void) {
    heap::checked("free", heap::c_free(conv::addr(ptr)))
}}

export! { "LIELLA_NATIVE_1";
/// The bytes usable in the block at `ptr` (at least its size; 0 for NULL).
pub extern "C" fn liella_heap_usable_size(ptr: *const c_void) -> usize {
    heap::checked("malloc_usable_size", heap::c_usable_size(conv::addr(ptr)))
}}
