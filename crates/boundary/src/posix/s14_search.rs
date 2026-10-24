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

/// `REG_NOTEOL`: `<regexp.h>` searches never let `$` match at the end of the subject (probe).
const REGEXP_H_EFLAGS: c_int = 2;

/// `<regexp.h>` `step`/`advance` (LG-10; compat only, probe-settled on glibc 2.39,
/// `spec/behaviour/regexp-h.md`): `expbuf` holds a `regex_t` at its first 8-aligned byte after
/// `expbuf[0]`, and the search runs with `REG_NOTEOL`. A match sets `*loc2` to its end and, for
/// `step`, `*loc1` to its start, and returns 1; no match returns 0 and leaves both. `advance`
/// (`anchored`) matches only at `string` itself. `locs` is never read or written. The generated
/// stubs pass the addresses of `loc1`/`loc2` through the GOT (`xtask/src/compat.rs` `LOADS`), so
/// an old non-PIE binary's copy is the one written.
fn regexp_h(string: *const c_char, expbuf: *const c_char, anchored: bool, loc1: Option<&mut *const c_char>, loc2: Option<&mut *const c_char>) -> c_int {
    if string.is_null() || expbuf.is_null() {
        return 0;
    }
    let Some(at) = conv::addr(expbuf).checked_add(1).and_then(|a| a.checked_next_multiple_of(8)) else { return 0 };
    let re = regex_t_at(at);
    let mut m = [regmatch_t { rm_so: -1, rm_eo: -1 }];
    let pmatch = core::ptr::from_mut(&mut m).expose_provenance();
    if regex::regexec(Some(re), conv::addr(string), 1, pmatch, REGEXP_H_EFLAGS) != 0 {
        return 0;
    }
    let (so, eo) = (m[0].rm_so, m[0].rm_eo);
    if anchored && so != 0 {
        return 0;
    }
    let at_off = |o: i32| string.wrapping_add(usize::try_from(o).unwrap_or(0));
    if let Some(l1) = loc1 {
        if !anchored && so >= 0 {
            *l1 = at_off(so);
        }
    }
    if let Some(l2) = loc2 {
        // REG_NOSUB reports no offsets: loc2 is the subject's start (glibc gives the same).
        *l2 = at_off(eo);
    }
    1
}

/// The `regex_t` that `<regexp.h>`'s `expbuf` holds, at address `at`.
// UNSAFE(slice): the compiled expression inside the caller's `expbuf`.
#[allow(unsafe_code)]
fn regex_t_at<'a>(at: usize) -> &'a regex_t {
    // SAFETY: <regexp.h>'s contract: `expbuf` holds a compiled expression, whose regex_t starts
    // at the first 8-aligned byte after `expbuf[0]` (probe-settled); `at` is that address.
    unsafe { &*core::ptr::with_exposed_provenance::<regex_t>(at) }
}

crate::internal_entry! {
/// `step@GLIBC_2.2.5` (compat only): the leftmost match of `expbuf` in `string` (see [`regexp_h`]).
pub extern "C" fn liella_compat_step(string: *const c_char, expbuf: *const c_char, loc1: Option<&mut *const c_char>, loc2: Option<&mut *const c_char>) -> c_int {
    regexp_h(string, expbuf, false, loc1, loc2)
}
}

crate::internal_entry! {
/// `advance@GLIBC_2.2.5` (compat only): a match of `expbuf` starting at `string` (see [`regexp_h`]).
pub extern "C" fn liella_compat_advance(string: *const c_char, expbuf: *const c_char, loc1: Option<&mut *const c_char>, loc2: Option<&mut *const c_char>) -> c_int {
    regexp_h(string, expbuf, true, loc1, loc2)
}
}

#[cfg(test)]
mod regexp_h_tests {
    use super::*;

    /// The test buffer's `regex_t` at `at` (8-aligned, inside a buffer larger than a `regex_t`).
    // UNSAFE(slice): test storage.
    #[allow(unsafe_code)]
    fn regex_t_mut<'a>(at: usize) -> &'a mut regex_t {
        // SAFETY: the callers pass an 8-aligned address inside a live zeroed buffer big enough for a regex_t.
        unsafe { &mut *core::ptr::with_exposed_provenance_mut::<regex_t>(at) }
    }

    /// Runs `f` on `s` with `pat` compiled at offset 8 of an 8-aligned buffer and `expbuf` at
    /// `buf + 8 - k` (glibc reads the regex_t at the next 8-aligned byte after expbuf[0]).
    fn run(pat: &core::ffi::CStr, cflags: c_int, s: &core::ffi::CStr, k: usize, step: bool) -> (c_int, Option<usize>, Option<usize>) {
        let mut buf = [0u64; 32];
        let base = buf.as_mut_ptr().expose_provenance();
        let re = regex_t_mut(base.saturating_add(8));
        assert_eq!(regcomp(Some(&mut *re), pat.as_ptr(), cflags), 0);
        let sent = c"sentinel".as_ptr();
        let (mut l1, mut l2) = (sent, sent);
        let e = core::ptr::with_exposed_provenance::<c_char>(base.saturating_add(8).saturating_sub(k));
        let r = if step { liella_compat_step(s.as_ptr(), e, Some(&mut l1), Some(&mut l2)) } else { liella_compat_advance(s.as_ptr(), e, Some(&mut l1), Some(&mut l2)) };
        regfree(Some(re));
        let off = |p: *const c_char| (p != sent).then(|| p.addr().saturating_sub(s.as_ptr().addr()));
        (r, off(l1), off(l2))
    }

    #[test]
    fn step_finds_the_leftmost_match_as_glibc_does() {
        for k in 1..=8 {
            assert_eq!(run(c"abc", 0, c"xxabcyy", k, true), (1, Some(2), Some(5)), "k {k}");
        }
        assert_eq!(run(c"abc", 0, c"xxab", 8, true), (0, None, None));
        assert_eq!(run(c"a*", 0, c"bbb", 8, true), (1, Some(0), Some(0)));
        assert_eq!(run(c"^abc", 0, c"xabc", 8, true), (0, None, None));
        assert_eq!(run(c"bc", 0, c"abc", 8, true), (1, Some(1), Some(3)));
        // `$` never matches at the end of the subject (REG_NOTEOL), but does before a newline.
        assert_eq!(run(c"c$", 0, c"abc", 8, true), (0, None, None));
        assert_eq!(run(c"b$", 4, c"ab\nc", 8, true), (1, Some(1), Some(2)));
        assert_eq!(run(c"ABC", 2, c"xabcx", 8, true), (1, Some(1), Some(4)));
        assert_eq!(run(c"x*", 0, c"", 8, true), (1, Some(0), Some(0)));
    }

    #[test]
    fn advance_matches_only_at_the_start_and_sets_only_loc2() {
        assert_eq!(run(c"abc", 0, c"abcyy", 8, false), (1, None, Some(3)));
        assert_eq!(run(c"abc", 0, c"xabc", 8, false), (0, None, None));
        assert_eq!(run(c"x*abc", 0, c"xxabcyy", 8, false), (1, None, Some(5)));
        assert_eq!(run(c"a*", 0, c"bbb", 8, false), (1, None, Some(0)));
        assert_eq!(run(c"abc$", 0, c"abc", 8, false), (0, None, None));
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

impl SortCompare for ByOffset {
    #[inline(always)]
    fn gt(&mut self, base: &[u8], a: usize, b: usize) -> bool {
        (self.0)(at(base, a), at(base, b)) > 0
    }
}

impl SortCompare for ByOffsetCtx {
    #[inline(always)]
    fn gt(&mut self, base: &[u8], a: usize, b: usize) -> bool {
        (self.0)(at(base, a), at(base, b), self.1) > 0
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
