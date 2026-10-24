//! Translation for spec §3: memory allocation.
//!
//! Every block comes from the process heap (`crate::heap`, IP-7) through the C family's way in
//! (`heap::c_*`: the C shapes of `liella_legacy::malloc_compat`, behind one test of the
//! debugging state); block-level misuse (an invalid or double `free`) is a diagnostic and abort
//! (S4). `brk`/`sbrk` move the program break, which the heap never uses (D-3.7). The hooks are
//! inert data (D-3.5); `M_PERTURB`, `mcheck` and `mtrace` have the semantics glibc provides with
//! `libc_malloc_debug.so`, here in the C library itself (D-3.5 as decided in phase 13).

use liella_abi::ctypes::{c_int, c_void, size_t};
use liella_abi::layout::{struct_mallinfo, struct_mallinfo2, struct_obstack, FILE};
use liella_legacy::malloc_compat;
use liella_legacy::obstack;

use crate::caller_alloc;
use crate::conv;
use crate::data;
use crate::export;
use crate::heap;
use crate::raw;
use crate::ret;

export! { "GLIBC_2.2.5";
pub extern "C" fn malloc(n: size_t) -> *mut c_void {
    heap::malloc_entry(n)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __libc_malloc(n: size_t) -> *mut c_void {
    heap::malloc_entry(n)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn free(p: *mut c_void) {
    heap::free_entry(conv::addr(p))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __libc_free(p: *mut c_void) {
    heap::free_entry(conv::addr(p))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn cfree(p: *mut c_void) {
    heap::free_entry(conv::addr(p))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn calloc(k: size_t, n: size_t) -> *mut c_void {
    ret::null(heap::c_calloc(k, n))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __libc_calloc(k: size_t, n: size_t) -> *mut c_void {
    ret::null(heap::c_calloc(k, n))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn realloc(p: *mut c_void, n: size_t) -> *mut c_void {
    ret::null(heap::checked("realloc", heap::c_realloc(conv::addr(p), n)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __libc_realloc(p: *mut c_void, n: size_t) -> *mut c_void {
    ret::null(heap::checked("realloc", heap::c_realloc(conv::addr(p), n)))
}}

export! { "GLIBC_2.26";
pub extern "C" fn reallocarray(p: *mut c_void, k: size_t, n: size_t) -> *mut c_void {
    ret::null(caller_alloc::reallocarray(conv::addr(p), k, n))
}}

export! { "GLIBC_2.16";
pub extern "C" fn aligned_alloc(align: size_t, n: size_t) -> *mut c_void {
    ret::null(heap::c_aligned_alloc(align, n))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn memalign(align: size_t, n: size_t) -> *mut c_void {
    ret::null(heap::c_memalign(align, n))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __libc_memalign(align: size_t, n: size_t) -> *mut c_void {
    ret::null(heap::c_memalign(align, n))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn posix_memalign(memptr: Option<&mut *mut c_void>, align: size_t, n: size_t) -> c_int {
    ret::errnum(conv::cell(memptr).and_then(|m| heap::c_posix_memalign(m, align, n)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn valloc(n: size_t) -> *mut c_void {
    ret::null(heap::c_valloc(n))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __libc_valloc(n: size_t) -> *mut c_void {
    ret::null(heap::c_valloc(n))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pvalloc(n: size_t) -> *mut c_void {
    ret::null(heap::c_pvalloc(n))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __libc_pvalloc(n: size_t) -> *mut c_void {
    ret::null(heap::c_pvalloc(n))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn malloc_usable_size(p: *mut c_void) -> size_t {
    heap::checked("malloc_usable_size", heap::c_usable_size(conv::addr(p)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn malloc_trim(pad: size_t) -> c_int {
    ret::truth(malloc_compat::trim(heap::drained(), pad))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mallopt(param: c_int, value: c_int) -> c_int {
    let ok = malloc_compat::mallopt(&heap::DEBUG, param, value);
    // Perturbation may be off now: the blocks it held go back to the heap.
    heap::drained();
    ret::truth(ok)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __libc_mallopt(param: c_int, value: c_int) -> c_int {
    let ok = malloc_compat::mallopt(&heap::DEBUG, param, value);
    // Perturbation may be off now: the blocks it held go back to the heap.
    heap::drained();
    ret::truth(ok)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mallinfo() -> struct_mallinfo {
    malloc_compat::mallinfo(heap::drained())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __libc_mallinfo() -> struct_mallinfo {
    malloc_compat::mallinfo(heap::drained())
}}

export! { "GLIBC_2.33";
pub extern "C" fn mallinfo2() -> struct_mallinfo2 {
    malloc_compat::mallinfo2(heap::drained())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn malloc_stats() {
    heap::malloc_stats()
}}

export! { "GLIBC_2.10";
pub extern "C" fn malloc_info(options: c_int, fp: *mut FILE) -> c_int {
    ret::zero(heap::malloc_info(options, conv::addr(fp)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __libc_freeres() {
    malloc_compat::inert()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn malloc_get_state() -> *mut c_void {
    ret::null(malloc_compat::state_unsupported())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn malloc_set_state(_state: *mut c_void) -> c_int {
    ret::int(malloc_compat::state_unsupported().map(|_| 0))
}}

// `__default_morecore` and `tr_break` are compat-only symbols (`@GLIBC_2.2.5`, no default
// version in 2.39; LG-11). As with `stime` (D-27.4), the translation carries an internal name and
// a `.symver` stub generated by `cargo xtask compat-gen` (`csys/src/machine/compat_aliases.S`)
// gives it the non-default version in `libc.so.6` alone: old binaries resolve it, a program
// linked anew (static or dynamic) does not find it (R15-2). Tests call the plain Rust function.
crate::internal_entry! {
/// `__default_morecore`'s translation, exported only as `__default_morecore@GLIBC_2.2.5`.
pub extern "C" fn liella_compat___default_morecore(_delta: isize) -> *mut c_void {
    ret::null(malloc_compat::state_unsupported())
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mcheck(abortfn: Option<extern "C" fn(c_int)>) -> c_int {
    heap::mcheck(abortfn, false)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mcheck_pedantic(abortfn: Option<extern "C" fn(c_int)>) -> c_int {
    heap::mcheck(abortfn, true)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mcheck_check_all() {
    heap::check_all()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mtrace() {
    heap::mtrace()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn muntrace() {
    heap::muntrace()
}}

crate::internal_entry! {
/// `tr_break`'s translation, exported only as `tr_break@GLIBC_2.2.5` (compat-only; see
/// `liella_compat___default_morecore`).
pub extern "C" fn liella_compat_tr_break() {
    malloc_compat::inert()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn mprobe(p: *mut c_void) -> c_int {
    heap::mprobe(conv::addr(p))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn brk(addr: *mut c_void) -> c_int {
    ret::zero(malloc_compat::brk(conv::addr(addr), raw::brk, &data::__curbrk))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn sbrk(delta: isize) -> *mut c_void {
    ret::addr_or_neg1(malloc_compat::sbrk(delta, raw::brk, &data::__curbrk))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __sbrk(delta: isize) -> *mut c_void {
    ret::addr_or_neg1(malloc_compat::sbrk(delta, raw::brk, &data::__curbrk))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _obstack_begin(h: Option<&mut struct_obstack>, size: c_int, align: c_int, chunkfun: usize, freefun: usize) -> c_int {
    // UNSAFE(slice) SAFETY: h is a struct obstack the caller set up; its chunks are memory chunkfun returned (§3.5).
    ret::must(conv::req(h).map(|o| obstack::obstack_begin(o, i64::from(size), i64::from(align), chunkfun, freefun, unsafe { conv::ctext() }, conv::CCalls, heap::obstack_handler())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _obstack_begin_1(h: Option<&mut struct_obstack>, size: c_int, align: c_int, chunkfun: usize, freefun: usize, arg: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: h is a struct obstack the caller set up; its chunks are memory chunkfun returned (§3.5).
    ret::must(conv::req(h).map(|o| obstack::obstack_begin_1(o, i64::from(size), i64::from(align), chunkfun, freefun, conv::addr(arg), unsafe { conv::ctext() }, conv::CCalls, heap::obstack_handler())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _obstack_newchunk(h: Option<&mut struct_obstack>, length: c_int) {
    // UNSAFE(slice) SAFETY: h is a struct obstack the caller set up; its chunks are memory chunkfun returned (§3.5).
    ret::unit(conv::req(h).map(|o| obstack::obstack_newchunk(o, conv::nonneg(length), unsafe { conv::ctext() }, conv::CCalls, |f, t, len| unsafe { conv::copy_at(f, t, len) }, heap::obstack_handler())))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _obstack_free(h: Option<&mut struct_obstack>, obj: *mut c_void) {
    // UNSAFE(slice) SAFETY: h is a struct obstack the caller set up; its chunks are memory chunkfun returned (§3.5).
    ret::unit(conv::req(h).map(|o| obstack::obstack_free(o, conv::addr(obj), unsafe { conv::ctext() }, conv::CCalls)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn obstack_free(h: Option<&mut struct_obstack>, obj: *mut c_void) {
    // UNSAFE(slice) SAFETY: h is a struct obstack the caller set up; its chunks are memory chunkfun returned (§3.5).
    ret::unit(conv::req(h).map(|o| obstack::obstack_free(o, conv::addr(obj), unsafe { conv::ctext() }, conv::CCalls)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _obstack_memory_used(h: Option<&mut struct_obstack>) -> c_int {
    // UNSAFE(slice) SAFETY: h is a struct obstack the caller set up; its chunks are memory chunkfun returned (§3.5).
    ret::must(conv::req(h).map(|o| obstack::obstack_memory_used(o, unsafe { conv::ctext() })))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _obstack_allocated_p(h: Option<&mut struct_obstack>, obj: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: h is a struct obstack the caller set up; its chunks are memory chunkfun returned (§3.5).
    ret::must(conv::req(h).map(|o| obstack::obstack_allocated_p(o, conv::addr(obj), unsafe { conv::ctext() })))
}}
