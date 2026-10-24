//! Category *extern*: the interposable C services the translation layer calls by symbol
//! (finding F23, decision D-2.1): caller-owned memory through `malloc`/`calloc`/`realloc`/`free`,
//! and the calling thread's `errno` cell through `__errno_location`.
//!
//! In products these symbols resolve to Liella's own exports (phases 3–4), or to a program's own
//! malloc family when it replaces it (D-X.14, "Replacing malloc"); in hosted test builds they
//! resolve to the running C library. Either way C callers and the library agree on one allocator
//! and one `errno`. The allocator is reached through the C layer's forwarders
//! (`liella_caller_malloc`, ...: one call each to the symbol), never directly: within the
//! library's own optimised code a call to `malloc` could be bound to, inlined from or merged with
//! the library's definition, and a program's replacement would then be bypassed.

use core::ffi::c_void;

use liella_kernel::errno::{Errno, KResult};

use crate::conv;

// Soundness: `malloc`, `calloc` and `__errno_location` are sound for every argument (they
// return fresh memory, NULL, or the thread's errno cell). `realloc` and `free` take a pointer
// the caller vouches for: they are crate-private and called only with pointers that a C
// contract says came from this allocator (documented at each call).
//
// UNSAFE(extern): Rust cannot call C library functions without declaring them.
#[allow(unsafe_code)]
unsafe extern "C" {
    #[link_name = "liella_caller_malloc"]
    safe fn c_malloc(n: usize) -> *mut c_void;
    #[link_name = "liella_caller_calloc"]
    safe fn c_calloc(n: usize, size: usize) -> *mut c_void;
    /// Obligation: `p` is NULL or came from this allocator and is not used afterwards.
    #[link_name = "liella_caller_realloc"]
    safe fn c_realloc(p: *mut c_void, n: usize) -> *mut c_void;
    /// Obligation: `p` is NULL or came from this allocator and is not used afterwards.
    #[link_name = "liella_caller_free"]
    safe fn c_free(p: *mut c_void);
    pub(crate) safe fn __errno_location() -> *mut i32;
}

/// Count `d` caller-owned blocks more (or fewer) live on the calling thread when `yes`: the
/// test builds' leak measure (`selftest::caller_blocks`, FREE-POISON-TESTS-6 ruling #296), valid
/// whichever allocator serves the symbols. Products compile it away.
#[inline(always)]
fn counted(yes: bool, d: i64) {
    #[cfg(feature = "selftest")]
    if yes {
        crate::selftest::count_caller(d);
    }
    #[cfg(not(feature = "selftest"))]
    let _ = (yes, d);
}

/// `malloc(n)` through the symbol.
#[allow(dead_code)] // the §3 exports (phase 3)
#[inline(always)]
pub(crate) fn malloc(n: usize) -> *mut c_void {
    let p = c_malloc(n);
    counted(!p.is_null(), 1);
    p
}

/// `calloc(n, size)` through the symbol.
#[inline(always)]
pub(crate) fn calloc(n: usize, size: usize) -> *mut c_void {
    let p = c_calloc(n, size);
    counted(!p.is_null(), 1);
    p
}

/// `realloc(p, n)` through the symbol. Obligation: `p` is NULL or came from this allocator and
/// is not used afterwards.
#[allow(dead_code)] // the §3 exports (phase 3)
#[inline(always)]
pub(crate) fn realloc(p: *mut c_void, n: usize) -> *mut c_void {
    let q = c_realloc(p, n);
    // A new block from NULL; `realloc(p, 0)` returning NULL released `p`.
    counted(p.is_null() && !q.is_null(), 1);
    counted(!p.is_null() && n == 0 && q.is_null(), -1);
    q
}

/// `free(p)` through the symbol. Obligation: `p` is NULL or came from this allocator and is not
/// used afterwards.
#[inline(always)]
pub(crate) fn free(p: *mut c_void) {
    counted(!p.is_null(), -1);
    c_free(p);
}

/// A new caller-owned copy of `text` plus a terminating zero unit (`strdup`, `wcsdup`): its
/// address, or `ENOMEM`.
///
/// The copy is `malloc`ed (§4.10: "a new `malloc`ed copy"), not `calloc`ed: every unit is written
/// here, so zeroing it first cost ~42 instructions per `strdup` (STRDUP-FAST).
pub(crate) fn dup_z<T: Copy + Default>(text: &[T]) -> KResult<usize> {
    let k = text.len();
    // `text` is a slice, so `k * size_of::<T>() <= isize::MAX`: `(k + 1) * size_of::<T>()` cannot wrap
    // (it is at most `isize::MAX + size_of::<T>()`). A size above `isize::MAX` is refused by `malloc`
    // itself (NULL, `ENOMEM`), so no separate check is made here (STRDUP-FAST-2: ~2 instructions).
    let bytes = k.wrapping_add(1).wrapping_mul(size_of::<T>());
    let p = malloc(bytes).cast::<T>();
    if p.is_null() {
        return Err(Errno::ENOMEM);
    }
    conv::fresh_copy_z(p, text);
    Ok(p.expose_provenance())
}

/// A new caller-owned copy of `bytes` (no terminator added); its address, or `ENOMEM`.
pub(crate) fn dup_bytes(bytes: &[u8]) -> KResult<usize> {
    let p = calloc(bytes.len().max(1), 1);
    let dst = conv::fresh::<u8>(p, bytes.len().max(1)).ok_or(Errno::ENOMEM)?;
    for (d, s) in dst.iter_mut().zip(bytes) {
        *d = *s;
    }
    Ok(p.expose_provenance())
}

/// `reallocarray(p, k, n)`: `realloc(p, k * n)` through the (replaceable) `realloc` symbol, as
/// glibc's manual describes it ("Replacing malloc", D-X.14); `ENOMEM` when the product
/// overflows. A size of 0 is `realloc`'s (the block freed, NULL without an error). Obligation (on
/// the C caller): `addr` is 0 or came from this allocator.
/// Without the exported symbols (hosted test builds, where `realloc` is the running C library's
/// while `free` and `malloc_usable_size` under test are Liella's), the heap's own path.
pub(crate) fn reallocarray(addr: usize, k: usize, n: usize) -> KResult<usize> {
    #[cfg(not(feature = "export"))]
    {
        let r = crate::heap::checked("realloc", crate::heap::c_reallocarray(addr, k, n));
        if let Ok(q) = r {
            counted(addr == 0 && q != 0, 1);
            counted(addr != 0 && q == 0, -1);
        }
        r
    }
    #[cfg(feature = "export")]
    {
        reallocarray_by_symbol(addr, k, n)
    }
}

#[cfg(feature = "export")]
fn reallocarray_by_symbol(addr: usize, k: usize, n: usize) -> KResult<usize> {
    let total = k.checked_mul(n).ok_or(Errno::ENOMEM)?;
    let q = c_realloc(core::ptr::with_exposed_provenance_mut(addr), total);
    counted(addr == 0 && !q.is_null(), 1);
    counted(addr != 0 && total == 0 && q.is_null(), -1);
    if q.is_null() && total != 0 {
        return Err(Errno::ENOMEM);
    }
    Ok(q.expose_provenance())
}

/// Release a caller-owned block. Obligation (on the caller of this function): `addr` is 0 or
/// came from this allocator and is not used afterwards.
pub(crate) fn release(addr: usize) {
    free(core::ptr::with_exposed_provenance_mut(addr));
}
