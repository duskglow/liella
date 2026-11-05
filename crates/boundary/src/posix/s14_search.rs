//! Translation for spec §14: sorting, searching and pattern matching.
//!
//! Phase 2 covers §14.1–§14.6: arrays (`qsort`, `bsearch`, `lfind`, `lsearch`), trees, hash
//! tables and queues. Their nodes live in caller-visible memory reached through
//! `liella_legacy::search_c` (allocated with the caller's `malloc`). Phase 12 adds the regular
//! expressions (§14.9: the POSIX, GNU and BSD interfaces, through `crate::regex`) and `fnmatch`,
//! `glob` and `wordexp` (§14.7, §14.8, §14.10, through `crate::patterns`). `glob` and `glob64`
//! are the current contract, `glob@@GLIBC_2.27`; the compat `glob@GLIBC_2.2.5` comes with
//! phase 15 (its ledger).
//!
//! A C++ exception thrown by a comparator, tree action or `tdestroy`'s free routine reaches
//! the C caller, as in glibc (OD-6 revised, D-14.6). Only `qsort`/`qsort_r` have state to clean
//! up (`sort_guarded` frees their buffer); all of them compare before they change anything.

use liella_abi::callback::{ActionCtxFn, ActionFn, CompareCtxFn, CompareFn, FreeFn, GlobErrFn, GlobT};
use liella_abi::ctypes::{c_char, c_int, c_uint, c_ulong, c_void, size_t};
use liella_abi::layout::{regex_t, regmatch_t, struct_hsearch_data, struct_re_registers, wordexp_t, ENTRY};
use liella_legacy::search_c::{self, SortCompare, SortScratch, Table};

use crate::conv;
use crate::export;
use crate::patterns;
use crate::regex::{self, GnuArgs};
use crate::ret;
use crate::strz;

/// `<regexp.h>` `step`/`advance` (LG-10 deferred; compat only; owner ruling R6, 2026-11-04):
/// stubs. Neither inspects its arguments: both return 0 (no match) and leave `*loc1`, `*loc2`
/// and `locs` untouched. The generated stubs (`xtask/src/compat.rs` `LOADS`) and the exported
/// `loc1`/`loc2`/`locs` objects are unchanged.
const REGEXP_H_NO_MATCH: c_int = 0;

crate::internal_entry! {
/// `step@GLIBC_2.2.5` (compat only): stub, never matches (see [`REGEXP_H_NO_MATCH`]).
pub extern "C" fn liella_compat_step(_string: *const c_char, _expbuf: *const c_char, _loc1: Option<&mut *const c_char>, _loc2: Option<&mut *const c_char>) -> c_int {
    REGEXP_H_NO_MATCH
}
}

crate::internal_entry! {
/// `advance@GLIBC_2.2.5` (compat only): stub, never matches (see [`REGEXP_H_NO_MATCH`]).
pub extern "C" fn liella_compat_advance(_string: *const c_char, _expbuf: *const c_char, _loc1: Option<&mut *const c_char>, _loc2: Option<&mut *const c_char>) -> c_int {
    REGEXP_H_NO_MATCH
}
}

#[cfg(test)]
mod regexp_h_tests {
    use super::*;

    /// `step`/`advance` are stubs (owner ruling R6; no spec clause beyond the ruling,
    /// `spec/behaviour/regexp-h.md`): whatever the arguments, the result is 0 and neither
    /// `*loc1` nor `*loc2` is written. Table: (string, expbuf, with-outputs).
    #[test]
    fn step_and_advance_return_no_match_and_leave_the_outputs() {
        let subject = c"xxabcyy";
        let zeroed = [0u8; 64];
        let cases: [(*const c_char, *const c_char); 4] = [
            (subject.as_ptr(), zeroed.as_ptr().cast()),
            (subject.as_ptr(), core::ptr::null()),
            (core::ptr::null(), zeroed.as_ptr().cast()),
            (c"".as_ptr(), zeroed.as_ptr().cast()),
        ];
        for (i, &(string, expbuf)) in cases.iter().enumerate() {
            for advance in [false, true] {
                let sent = c"sentinel".as_ptr();
                let (mut l1, mut l2) = (sent, sent);
                let f = if advance { liella_compat_advance } else { liella_compat_step };
                assert_eq!(f(string, expbuf, Some(&mut l1), Some(&mut l2)), 0, "case {i} advance {advance}");
                assert!(l1 == sent && l2 == sent, "case {i} advance {advance}: outputs untouched");
                assert_eq!(f(string, expbuf, None, None), 0, "case {i} advance {advance}: no outputs");
            }
        }
    }
}

export! { "GLIBC_2.2.5";
pub extern "C" fn qsort(base: *mut c_void, n: size_t, size: size_t, cmp: Option<CompareFn>) {
    // UNSAFE(slice) SAFETY: base holds n elements of size bytes (§14.1).
    ret::unit(conv::req(cmp).and_then(|f| unsafe { conv::elements_mut(base, n, size) }.map(|b| sort_guarded(|s| search_c::qsort(b, size, ByOffset(f), s)))))
}}

export! { "GLIBC_2.8";
pub extern "C" fn qsort_r(base: *mut c_void, n: size_t, size: size_t, cmp: Option<CompareCtxFn>, arg: *mut c_void) {
    // UNSAFE(slice) SAFETY: base holds n elements of size bytes (§14.1).
    ret::unit(conv::req(cmp).and_then(|f| unsafe { conv::elements_mut(base, n, size) }.map(|b| sort_guarded(|s| search_c::qsort(b, size, ByOffsetCtx(f, arg), s)))))
}}

/// `qsort`'s comparator on elements named by byte offset (QSORT-COMPARE-TRAIT): the pointers
/// are formed from the array's base without slicing, so a comparison costs no bounds checks.
struct ByOffset(CompareFn);

/// `qsort_r`'s comparator and its context (GNU order: the context last).
struct ByOffsetCtx(CompareCtxFn, *mut c_void);

/// The element at byte offset `o` of `base`.
// UNSAFE(slice): an element pointer formed without a bounds check.
#[allow(unsafe_code)]
#[inline(always)]
fn at(base: &[u8], o: usize) -> *const c_void {
    debug_assert!(o < base.len(), "SortCompare contract: offset {o} past {}", base.len());
    // UNSAFE(slice) SAFETY: the sort passes only offsets of whole elements of `base`, whatever
    // the comparator answers (the `SortCompare` contract in liella::search), so the pointer
    // stays inside `base` and keeps its provenance.
    unsafe { base.as_ptr().add(o) }.cast()
}

/// The element at address `a` of `base` (`SortCompare::gt_at`): the address carries the
/// provenance `base` exposed, and names a whole element of it (the same contract as `at`).
#[inline(always)]
fn at_addr(base: &[u8], a: usize) -> *const c_void {
    debug_assert!(a.wrapping_sub(base.as_ptr().addr()) < base.len(), "SortCompare contract: address {a:#x} outside the array");
    core::ptr::with_exposed_provenance(a)
}

impl SortCompare for ByOffset {
    #[inline(always)]
    fn gt(&mut self, base: &[u8], a: usize, b: usize) -> bool {
        (self.0)(at(base, a), at(base, b)) > 0
    }

    #[inline(always)]
    fn gt_at(&mut self, base: &[u8], a: usize, b: usize) -> bool {
        (self.0)(at_addr(base, a), at_addr(base, b)) > 0
    }
}

impl SortCompare for ByOffsetCtx {
    #[inline(always)]
    fn gt(&mut self, base: &[u8], a: usize, b: usize) -> bool {
        (self.0)(at(base, a), at(base, b), self.1) > 0
    }

    #[inline(always)]
    fn gt_at(&mut self, base: &[u8], a: usize, b: usize) -> bool {
        (self.0)(at_addr(base, a), at_addr(base, b), self.1) > 0
    }
}

/// Sort in a guard frame (OD-6 revised): if the comparator throws, the elements a merge holds
/// in its buffer go back into the array (which stays a permutation of its elements, as in
/// glibc) and the heap buffer is freed.
fn sort_guarded(sort: impl FnOnce(&mut SortScratch)) {
    let mut s = SortScratch::default();
    let _ = crate::posix::s22_threads::guarded(&mut s, sort, sort_unwound);
}

/// After a throw: free the heap buffer (the array is only written after the last comparison).
fn sort_unwound(s: &mut SortScratch) {
    s.heap = None;
}

export! { "GLIBC_2.2.5";
pub extern "C" fn bsearch(key: *const c_void, base: *const c_void, n: size_t, size: size_t, cmp: Option<CompareFn>) -> *mut c_void {
    // UNSAFE(slice) SAFETY: base holds n sorted elements of size bytes (§14.2).
    ret::found(base.cast::<u8>(), conv::req(cmp).and_then(|f| unsafe { conv::elements(base, n, size) }.map(|b| search_c::bsearch(key, b, size, f)))).cast()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn lfind(key: *const c_void, base: *const c_void, nmemb: Option<&size_t>, size: size_t, cmp: Option<CompareFn>) -> *mut c_void {
    // UNSAFE(slice) SAFETY: nmemb is a valid size_t; base holds *nmemb elements of size bytes (§14.3).
    ret::found(base.cast::<u8>(), conv::req(cmp).and_then(|f| conv::req(nmemb).and_then(|n| unsafe { conv::elements(base, conv::copy(n), size) }.map(|b| search_c::lfind(key, b, size, f))))).cast()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn lsearch(key: *const c_void, base: *mut c_void, nmemb: Option<&mut size_t>, size: size_t, cmp: Option<CompareFn>) -> *mut c_void {
    // UNSAFE(slice) SAFETY: key is size bytes; nmemb is a valid size_t; base has room for *nmemb + 1 elements (§14.3).
    ret::at(base.cast::<u8>(), conv::req(cmp).and_then(|f| unsafe { conv::buf(key, size) }.and_then(|k| conv::req(nmemb).and_then(|n| unsafe { conv::elements_room(base, conv::copy(n), size) }.map(|b| search_c::lsearch(k, b, n, size, f)))))).cast()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn tsearch(key: *const c_void, rootp: Option<&mut *mut c_void>, cmp: Option<CompareFn>) -> *mut c_void {
    // UNSAFE(slice) SAFETY: rootp is a valid root variable of a tree built by these functions (§14.4).
    ret::null(conv::cell(rootp).and_then(|r| conv::req(cmp).and_then(|f| search_c::tsearch(conv::addr(key), r, unsafe { conv::ctext() }, f))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn tfind(key: *const c_void, rootp: Option<&*mut c_void>, cmp: Option<CompareFn>) -> *mut c_void {
    // UNSAFE(slice) SAFETY: rootp is a valid root variable of a tree built by these functions (§14.4).
    ret::null(conv::req(rootp).and_then(|r| conv::req(cmp).map(|f| search_c::tfind(conv::addr(key), conv::addr(conv::copy(r).cast_const()), unsafe { conv::ctext() }, f))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn tdelete(key: *const c_void, rootp: Option<&mut *mut c_void>, cmp: Option<CompareFn>) -> *mut c_void {
    // UNSAFE(slice) SAFETY: rootp is a valid root variable of a tree built by these functions (§14.4).
    ret::null(conv::cell(rootp).and_then(|r| conv::req(cmp).map(|f| search_c::tdelete(conv::addr(key), r.var_addr(), r, unsafe { conv::ctext() }, f))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn twalk(root: *const c_void, action: Option<ActionFn>) {
    // UNSAFE(slice) SAFETY: root is NULL or a tree built by these functions (§14.4).
    search_c::twalk(conv::addr(root), unsafe { conv::ctext() }, action)
}}

export! { "GLIBC_2.30";
pub extern "C" fn twalk_r(root: *const c_void, action: Option<ActionCtxFn>, closure: *mut c_void) {
    // UNSAFE(slice) SAFETY: root is NULL or a tree built by these functions (§14.4).
    search_c::twalk_r(conv::addr(root), unsafe { conv::ctext() }, action, closure)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn tdestroy(root: *mut c_void, free_key: Option<FreeFn>) {
    // UNSAFE(slice) SAFETY: root is NULL or a tree built by these functions (§14.4).
    search_c::tdestroy(conv::addr(root), unsafe { conv::ctext() }, free_key)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn hcreate(nel: size_t) -> c_int {
    // UNSAFE(slice) SAFETY: the table's keys are strings that outlive it (§14.5).
    ret::nonzero(search_c::hcreate(Table::Global, nel, unsafe { conv::ctext() }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn hsearch(item: ENTRY, action: c_int) -> *mut ENTRY {
    // UNSAFE(slice) SAFETY: the table's keys are strings that outlive it (§14.5).
    ret::null(search_c::hsearch(Table::Global, item, conv::flag(action), unsafe { conv::ctext() }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn hdestroy() {
    // UNSAFE(slice) SAFETY: the table's keys are strings that outlive it (§14.5).
    search_c::hdestroy(Table::Global, unsafe { conv::ctext() })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn hcreate_r(nel: size_t, htab: Option<&mut struct_hsearch_data>) -> c_int {
    // UNSAFE(slice) SAFETY: htab is a valid hsearch_data; its keys are strings that outlive it (§14.5).
    ret::nonzero(conv::req(htab).and_then(|d| search_c::hcreate(Table::Caller(d), nel, unsafe { conv::ctext() })))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn hsearch_r(item: ENTRY, action: c_int, retval: Option<&mut *mut ENTRY>, htab: Option<&mut struct_hsearch_data>) -> c_int {
    // UNSAFE(slice) SAFETY: retval is a valid ENTRY * variable; htab is a valid hsearch_data; its keys are strings that outlive it (§14.5).
    ret::nonzero(conv::cell(retval).and_then(|r| conv::req(htab).and_then(|d| search_c::hsearch_r(Table::Caller(d), item, conv::flag(action), r, unsafe { conv::ctext() }))))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn hdestroy_r(htab: Option<&mut struct_hsearch_data>) {
    // UNSAFE(slice) SAFETY: htab is a valid hsearch_data; its keys are strings that outlive it (§14.5).
    ret::errno_only(conv::req(htab).map(|d| search_c::hdestroy(Table::Caller(d), unsafe { conv::ctext() })))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn insque(elem: *mut c_void, prev: *mut c_void) {
    // UNSAFE(slice) SAFETY: the elements start with forward and backward pointers (§14.6).
    search_c::insque(conv::addr(elem), conv::addr(prev), unsafe { conv::ctext() })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn remque(elem: *mut c_void) {
    // UNSAFE(slice) SAFETY: the elements start with forward and backward pointers (§14.6).
    search_c::remque(conv::addr(elem), unsafe { conv::ctext() })
}}

// ------------------------------------------------------------------------------ §14.7 fnmatch

export! { "GLIBC_2.2.5";
pub extern "C" fn fnmatch(pattern: *const c_char, string: *const c_char, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: pattern and string are NUL-terminated strings (§14.7).
    patterns::fnmatch(unsafe { strz::strz(pattern) }, unsafe { strz::strz(string) }, flags)
}}

// ------------------------------------------------------------------------------ §14.8 glob

export! { "GLIBC_2.27";
pub extern "C" fn glob(pattern: *const c_char, flags: c_int, errfunc: Option<GlobErrFn>, pglob: Option<&mut GlobT>) -> c_int {
    // UNSAFE(slice) SAFETY: pattern is a NUL-terminated string (§14.8).
    patterns::glob(unsafe { strz::strz(pattern) }, flags, errfunc, pglob)
}}

export! { "GLIBC_2.27";
pub extern "C" fn glob64(pattern: *const c_char, flags: c_int, errfunc: Option<GlobErrFn>, pglob: Option<&mut GlobT>) -> c_int {
    // UNSAFE(slice) SAFETY: pattern is a NUL-terminated string (§14.8); glob64_t is glob_t's layout.
    patterns::glob(unsafe { strz::strz(pattern) }, flags, errfunc, pglob)
}}

crate::internal_entry! {
/// `glob@GLIBC_2.2.5` and `glob64@GLIBC_2.2.5`: with `GLOB_ALTDIRFUNC`, existence is checked
/// with `gl_stat` (LG-2, the plan's ledger). Exported only under those compat versions.
pub extern "C" fn liella_compat_glob(pattern: *const c_char, flags: c_int, errfunc: Option<GlobErrFn>, pglob: Option<&mut GlobT>) -> c_int {
    // UNSAFE(slice) SAFETY: pattern is a NUL-terminated string (§14.8).
    patterns::glob_2_2_5(unsafe { strz::strz(pattern) }, flags, errfunc, pglob)
}}

crate::internal_entry! {
/// `glob64@GLIBC_2.2.5`: the same contract as [`liella_compat_glob`] (glob64_t is glob_t's layout);
/// its own entry so the plan's behaviour row finds `liella_compat_glob64` (compat-gen).
pub extern "C" fn liella_compat_glob64(pattern: *const c_char, flags: c_int, errfunc: Option<GlobErrFn>, pglob: Option<&mut GlobT>) -> c_int {
    // UNSAFE(slice) SAFETY: pattern is a NUL-terminated string (§14.8); glob64_t is glob_t's layout.
    patterns::glob_2_2_5(unsafe { strz::strz(pattern) }, flags, errfunc, pglob)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn globfree(pglob: Option<&mut GlobT>) {
    patterns::globfree(pglob)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn globfree64(pglob: Option<&mut GlobT>) {
    patterns::globfree(pglob)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn glob_pattern_p(pattern: *const c_char, quote: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: pattern is a NUL-terminated string (§14.8).
    patterns::glob_pattern_p(unsafe { strz::strz(pattern) }, quote)
}}

// ------------------------------------------------------------------------------ §14.10 wordexp

export! { "GLIBC_2.2.5";
pub extern "C" fn wordexp(words: *const c_char, we: Option<&mut wordexp_t>, flags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: words is a NUL-terminated string (§14.10).
    patterns::wordexp(unsafe { strz::strz(words) }, we, flags)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn wordfree(we: Option<&mut wordexp_t>) {
    patterns::wordfree(we)
}}

// ------------------------------------------------------------------------------ §14.9 regular expressions

export! { "GLIBC_2.2.5";
pub extern "C" fn regcomp(preg: Option<&mut regex_t>, pattern: *const c_char, cflags: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: pattern is a NUL-terminated string (§14.9).
    regex::regcomp(preg, unsafe { strz::strz(pattern) }, cflags)
}}

export! { "GLIBC_2.3.4";
pub extern "C" fn regexec(preg: Option<&regex_t>, string: *const c_char, nmatch: size_t, pmatch: *mut regmatch_t, eflags: c_int) -> c_int {
    regex::regexec(preg, conv::addr(string), nmatch, conv::addr(pmatch), eflags)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn regerror(errcode: c_int, _preg: Option<&regex_t>, errbuf: *mut c_char, errbuf_size: size_t) -> size_t {
    // UNSAFE(slice) SAFETY: errbuf is NULL or holds errbuf_size writable bytes (§14.9); preg does not change the text.
    regex::regerror(errcode, unsafe { conv::opt_buf_mut(errbuf, errbuf_size) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn regfree(preg: Option<&mut regex_t>) {
    regex::regfree(preg)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn re_set_syntax(syntax: c_ulong) -> c_ulong {
    regex::re_set_syntax(syntax)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn re_compile_pattern(pattern: *const c_char, length: size_t, buffer: Option<&mut regex_t>) -> *const c_char {
    // UNSAFE(slice) SAFETY: pattern holds length bytes (§14.9: length-delimited).
    regex::re_compile_pattern(unsafe { conv::buf(pattern.cast(), length) }, buffer)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn re_compile_fastmap(buffer: Option<&mut regex_t>) -> c_int {
    regex::re_compile_fastmap(buffer)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn re_search(buffer: Option<&mut regex_t>, string: *const c_char, length: c_int, start: c_int, range: c_int, regs: Option<&mut struct_re_registers>) -> c_int {
    regex::gnu_search(buffer, GnuArgs { s1: (conv::addr(string), length), s2: (0, 0), start, range, stop: length, anchored: false }, regs)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn re_search_2(buffer: Option<&mut regex_t>, string1: *const c_char, length1: c_int, string2: *const c_char, length2: c_int, start: c_int, range: c_int, regs: Option<&mut struct_re_registers>, stop: c_int) -> c_int {
    regex::gnu_search(buffer, GnuArgs { s1: (conv::addr(string1), length1), s2: (conv::addr(string2), length2), start, range, stop, anchored: false }, regs)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn re_match(buffer: Option<&mut regex_t>, string: *const c_char, length: c_int, start: c_int, regs: Option<&mut struct_re_registers>) -> c_int {
    regex::gnu_search(buffer, GnuArgs { s1: (conv::addr(string), length), s2: (0, 0), start, range: 0, stop: length, anchored: true }, regs)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn re_match_2(buffer: Option<&mut regex_t>, string1: *const c_char, length1: c_int, string2: *const c_char, length2: c_int, start: c_int, regs: Option<&mut struct_re_registers>, stop: c_int) -> c_int {
    regex::gnu_search(buffer, GnuArgs { s1: (conv::addr(string1), length1), s2: (conv::addr(string2), length2), start, range: 0, stop, anchored: true }, regs)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn re_set_registers(buffer: Option<&mut regex_t>, regs: Option<&mut struct_re_registers>, num_regs: c_uint, starts: *mut c_int, ends: *mut c_int) {
    regex::re_set_registers(buffer, regs, num_regs, conv::addr(starts), conv::addr(ends))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn re_comp(s: *const c_char) -> *mut c_char {
    // UNSAFE(slice) SAFETY: s is NULL or a NUL-terminated string (§14.9).
    regex::re_comp(unsafe { strz::strz(s) })
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn re_exec(s: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: s is a NUL-terminated string (§14.9).
    regex::re_exec(unsafe { strz::strz(s) })
}}
