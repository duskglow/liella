// Specified from spec §14.1–§14.6 (phase 2) and §14.7–§14.10 (phase 12).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Translation-layer tests for the exported symbols of spec §14 (43 entries).
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(non_snake_case)]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::panic, missing_docs)]

use liella_abi::ctypes::c_char;

// ------------------------------------------------------------------------------ helpers (§14.1–14.6)

use core::ffi::c_void;
use core::ptr::{null, null_mut};
use std::sync::Mutex;

use liella_abi::ctypes::c_int;
use liella_abi::layout::{struct_hsearch_data, ENTRY};
use liella_boundary::errno;
use liella_boundary::posix::s04_strings as mem;
use liella_boundary::posix::s14_search as s;

const ESRCH: i32 = 3;

fn load<T: Copy + Default>(p: *const c_void) -> T {
    let mut v = T::default();
    mem::memcpy((&raw mut v).cast(), p, size_of::<T>());
    v
}

extern "C" fn cmp_i32(a: *const c_void, b: *const c_void) -> c_int {
    load::<i32>(a).cmp(&load::<i32>(b)) as c_int
}

/// Orders `(key, tag)` pairs by key only.
extern "C" fn cmp_first(a: *const c_void, b: *const c_void) -> c_int {
    load::<i32>(a).cmp(&load::<i32>(b)) as c_int
}

extern "C" fn cmp_ctx(a: *const c_void, b: *const c_void, ctx: *mut c_void) -> c_int {
    let o = load::<i32>(a).cmp(&load::<i32>(b)) as c_int;
    if ctx.is_null() { o } else { -o }
}

/// Keys are small integers carried in the pointer itself.
extern "C" fn cmp_addr(a: *const c_void, b: *const c_void) -> c_int {
    a.addr().cmp(&b.addr()) as c_int
}

fn key(n: usize) -> *const c_void {
    core::ptr::without_provenance(n)
}

/// The key stored in a tree node (its first word).
fn node_key(node: *const c_void) -> usize {
    load::<usize>(node)
}

static WALK: Mutex<Vec<(usize, c_int, c_int)>> = Mutex::new(Vec::new());
static WALK_R: Mutex<Vec<(usize, c_int, c_int)>> = Mutex::new(Vec::new());
static FREED: Mutex<Vec<usize>> = Mutex::new(Vec::new());

extern "C" fn record(node: *const c_void, visit: c_int, depth: c_int) {
    WALK.lock().unwrap().push((node_key(node), visit, depth));
}

extern "C" fn record_r(node: *const c_void, visit: c_int, ctx: *mut c_void) {
    WALK_R.lock().unwrap().push((node_key(node), visit, ctx.addr() as c_int));
}

extern "C" fn free_key(k: *mut c_void) {
    FREED.lock().unwrap().push(k.addr());
}

fn tree(keys: &[usize]) -> *mut c_void {
    let mut root: *mut c_void = null_mut();
    for &k in keys {
        assert!(!s::tsearch(key(k), Some(&mut root), Some(cmp_addr)).is_null());
    }
    root
}

fn ckey(t: &str) -> *mut c_char {
    Box::leak(t.bytes().map(|b| b as c_char).chain([0]).collect::<Vec<_>>().into_boxed_slice()).as_mut_ptr()
}

static HGLOBAL: Mutex<()> = Mutex::new(());

// ------------------------------------------------------------------------------ arrays

#[test]
fn sym_qsort() {
    let mut v = [5i32, -1, 3, 3, 0, 9, -7];
    s::qsort(v.as_mut_ptr().cast(), 7, 4, Some(cmp_i32));
    assert_eq!(v, [-7, -1, 0, 3, 3, 5, 9]);
    let mut pairs = [[2, 0], [1, 1], [2, 2], [1, 3], [2, 4]];
    s::qsort(pairs.as_mut_ptr().cast(), 5, 8, Some(cmp_first));
    assert_eq!(pairs, [[1, 1], [1, 3], [2, 0], [2, 2], [2, 4]], "stable (D-14.1)");
    s::qsort(null_mut(), 0, 4, Some(cmp_i32));
    // The merge path (more than 8 elements), stack and heap position tables, pairs and words.
    for n in [9usize, 60, 1000] {
        let mut x = 99u32;
        let mut big: Vec<[i32; 2]> = (0..n).map(|i| {
            x = x.wrapping_mul(1_103_515_245).wrapping_add(12345);
            [(x >> 25) as i32 - 64, i as i32]
        }).collect();
        let mut want = big.clone();
        want.sort_by_key(|p| p[0]);
        s::qsort(big.as_mut_ptr().cast(), n, 8, Some(cmp_first));
        assert_eq!(big, want, "n {n}: sorted and stable on the merge path");
        let mut w: Vec<i32> = (0..n as i32).rev().collect();
        s::qsort_r(w.as_mut_ptr().cast(), n, 4, Some(cmp_ctx), null_mut());
        assert_eq!(w, (0..n as i32).collect::<Vec<_>>(), "n {n}: qsort_r merge path");
        s::qsort_r(w.as_mut_ptr().cast(), n, 4, Some(cmp_ctx), core::ptr::without_provenance_mut(1));
        assert_eq!(w, (0..n as i32).rev().collect::<Vec<_>>(), "n {n}: qsort_r context on the merge path");
    }
    if std::env::var_os("LIELLA_QSORT_UNWIND").is_some() {
        let mut w = [2i32, 1];
        liella_boundary::selftest::expect_crash();
        s::qsort(w.as_mut_ptr().cast(), 2, 4, Some(boom));
        return;
    }
    use std::os::unix::process::ExitStatusExt;
    let st = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["sym_qsort", "--exact", "--test-threads=1"])
        .env("LIELLA_QSORT_UNWIND", "1")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .unwrap();
    assert_eq!(st.signal(), Some(6), "an exception through the comparator aborts (OD-6)");
}

extern "C" fn boom(_: *const c_void, _: *const c_void) -> c_int {
    panic!("thrown through qsort")
}

#[test]
fn sym_qsort_r() {
    let mut v = [1i32, 3, 2];
    s::qsort_r(v.as_mut_ptr().cast(), 3, 4, Some(cmp_ctx), null_mut());
    assert_eq!(v, [1, 2, 3]);
    s::qsort_r(v.as_mut_ptr().cast(), 3, 4, Some(cmp_ctx), core::ptr::without_provenance_mut(1));
    assert_eq!(v, [3, 2, 1], "the context reaches the comparator, last (GNU order)");
}

#[test]
fn sym_bsearch() {
    let v = [1i32, 3, 5, 7, 9];
    let k = 7i32;
    let r = s::bsearch((&raw const k).cast(), v.as_ptr().cast(), 5, 4, Some(cmp_i32));
    assert_eq!(r.cast_const(), v.as_ptr().wrapping_add(3).cast());
    let k = 4i32;
    assert!(s::bsearch((&raw const k).cast(), v.as_ptr().cast(), 5, 4, Some(cmp_i32)).is_null());
    assert!(s::bsearch((&raw const k).cast(), null(), 0, 4, Some(cmp_i32)).is_null());
}

#[test]
fn sym_lfind() {
    let v = [4i32, 8, 4];
    let n = 3usize;
    let k = 4i32;
    let r = s::lfind((&raw const k).cast(), v.as_ptr().cast(), Some(&n), 4, Some(cmp_i32));
    assert_eq!(r.cast_const(), v.as_ptr().cast(), "the first match");
    let k = 5i32;
    assert!(s::lfind((&raw const k).cast(), v.as_ptr().cast(), Some(&n), 4, Some(cmp_i32)).is_null());
    assert_eq!(n, 3);
}

#[test]
fn sym_lsearch() {
    let mut v = [1i32, 2, 0];
    let mut n = 2usize;
    let k = 2i32;
    let r = s::lsearch((&raw const k).cast(), v.as_mut_ptr().cast(), Some(&mut n), 4, Some(cmp_i32));
    assert_eq!((r.cast_const(), n), (v.as_ptr().wrapping_add(1).cast(), 2));
    let k = 9i32;
    let p = v.as_mut_ptr();
    let r = s::lsearch((&raw const k).cast(), p.cast(), Some(&mut n), 4, Some(cmp_i32));
    assert_eq!((r, n), (p.wrapping_add(2).cast(), 3), "appended and counted");
    assert_eq!(v, [1, 2, 9]);
}

// ------------------------------------------------------------------------------ trees

#[test]
fn sym_tsearch() {
    let mut root: *mut c_void = null_mut();
    let a = s::tsearch(key(50), Some(&mut root), Some(cmp_addr));
    assert_eq!(node_key(a), 50, "the node's first member is the key");
    assert_eq!(s::tsearch(key(50), Some(&mut root), Some(cmp_addr)), a, "found, not inserted twice");
    for k in 1..200 {
        s::tsearch(key(k * 7 % 199 + 1), Some(&mut root), Some(cmp_addr));
    }
    assert_eq!(s::tsearch(key(50), Some(&mut root), Some(cmp_addr)), a, "nodes stay put");
    let mut nothing: *mut c_void = null_mut();
    errno::set_raw(0);
    assert!(s::tsearch(key(1), None, Some(cmp_addr)).is_null());
    assert!(nothing.is_null());
    nothing = s::tsearch(key(3), Some(&mut nothing), Some(cmp_addr));
    assert!(!nothing.is_null());
}

#[test]
fn sym_tfind() {
    let root = tree(&[10, 20, 30]);
    assert_eq!(node_key(s::tfind(key(20), Some(&root), Some(cmp_addr))), 20);
    assert!(s::tfind(key(25), Some(&root), Some(cmp_addr)).is_null());
    let empty: *mut c_void = null_mut();
    assert!(s::tfind(key(1), Some(&empty), Some(cmp_addr)).is_null());
}

#[test]
fn sym_tdelete() {
    let mut root = tree(&[2, 1, 3]);
    assert!(!s::tdelete(key(1), Some(&mut root), Some(cmp_addr)).is_null(), "the parent");
    assert!(s::tfind(key(1), Some(&root), Some(cmp_addr)).is_null());
    assert!(s::tdelete(key(9), Some(&mut root), Some(cmp_addr)).is_null(), "absent");
    assert!(!s::tdelete(key(2), Some(&mut root), Some(cmp_addr)).is_null());
    assert!(!s::tdelete(key(3), Some(&mut root), Some(cmp_addr)).is_null(), "non-NULL even when the tree empties");
    assert!(root.is_null());
}

#[test]
fn sym_twalk() {
    let root = tree(&[2, 1, 3]);
    WALK.lock().unwrap().clear();
    s::twalk(root, Some(record));
    let w = WALK.lock().unwrap().clone();
    let inorder: Vec<usize> = w.iter().filter(|e| e.1 == 1 || e.1 == 3).map(|e| e.0).collect();
    assert_eq!(inorder, [1, 2, 3], "postorder (1) and leaf (3) visits give the in-order sequence");
    assert!(w.contains(&(2, 0, 0)) && w.contains(&(2, 2, 0)), "preorder and endorder of the root at depth 0");
    assert!(w.contains(&(1, 3, 1)), "a leaf at depth 1");
    s::twalk(root, None);
    s::twalk(null(), Some(record));
}

#[test]
fn sym_twalk_r() {
    let root = tree(&[5]);
    s::twalk_r(root, Some(record_r), core::ptr::without_provenance_mut(42));
    let w: Vec<_> = WALK_R.lock().unwrap().iter().filter(|e| e.0 == 5).copied().collect();
    assert_eq!(w, [(5, 3, 42)], "the closure argument reaches the action");
}

#[test]
fn sym_tdestroy() {
    let root = tree(&[1001, 1002, 1003]);
    s::tdestroy(root, Some(free_key));
    let mut freed: Vec<usize> = FREED.lock().unwrap().iter().copied().filter(|&k| k > 1000).collect();
    freed.sort_unstable();
    assert_eq!(freed, [1001, 1002, 1003], "every key, once");
    s::tdestroy(null_mut(), Some(free_key));
}

// ------------------------------------------------------------------------------ hash tables

#[test]
fn sym_hcreate() {
    let _g = HGLOBAL.lock().unwrap();
    assert_ne!(s::hcreate(10), 0);
    assert_ne!(s::hcreate(10), 0, "an existing table is kept");
    s::hdestroy();
}

#[test]
fn sym_hsearch() {
    let _g = HGLOBAL.lock().unwrap();
    s::hcreate(2);
    let k = ckey("alpha");
    let e = s::hsearch(ENTRY { key: k.addr(), data: 7 }, 1);
    assert!(!e.is_null());
    let found = s::hsearch(ENTRY { key: ckey("alpha").addr(), data: 0 }, 0);
    assert_eq!(found, e, "found by text, not by pointer");
    assert_eq!(load::<[usize; 2]>(found.cast())[1], 7, "the data");
    for i in 0..100 {
        assert!(!s::hsearch(ENTRY { key: ckey(&format!("k{i}")).addr(), data: i }, 1).is_null(), "the table grows (D-14.3)");
    }
    assert_eq!(s::hsearch(ENTRY { key: ckey("alpha").addr(), data: 0 }, 0), e, "entries do not move");
    errno::set_raw(0);
    assert!(s::hsearch(ENTRY { key: ckey("missing").addr(), data: 0 }, 0).is_null());
    assert_eq!(errno::get(), ESRCH);
    s::hdestroy();
}

#[test]
fn sym_hdestroy() {
    let _g = HGLOBAL.lock().unwrap();
    s::hcreate(4);
    s::hsearch(ENTRY { key: ckey("x").addr(), data: 1 }, 1);
    s::hdestroy();
    s::hcreate(4);
    assert!(s::hsearch(ENTRY { key: ckey("x").addr(), data: 0 }, 0).is_null(), "a fresh table");
    s::hdestroy();
}

fn htab() -> struct_hsearch_data {
    struct_hsearch_data { table: 0, size: 0, filled: 0 }
}

#[test]
fn sym_hcreate_r() {
    let mut t = htab();
    assert_ne!(s::hcreate_r(10, Some(&mut t)), 0);
    assert!(t.table != 0 && t.size >= 10, "a prime of at least nel entries");
    s::hdestroy_r(Some(&mut t));
}

#[test]
fn sym_hsearch_r() {
    let mut t = htab();
    s::hcreate_r(3, Some(&mut t));
    let mut out: *mut ENTRY = null_mut();
    assert_ne!(s::hsearch_r(ENTRY { key: ckey("one").addr(), data: 1 }, 1, Some(&mut out), Some(&mut t)), 0);
    assert!(!out.is_null());
    assert_eq!(t.filled, 1);
    let mut again: *mut ENTRY = null_mut();
    assert_ne!(s::hsearch_r(ENTRY { key: ckey("one").addr(), data: 0 }, 0, Some(&mut again), Some(&mut t)), 0);
    assert_eq!(again, out);
    errno::set_raw(0);
    assert_eq!(s::hsearch_r(ENTRY { key: ckey("two").addr(), data: 0 }, 0, Some(&mut again), Some(&mut t)), 0);
    assert_eq!((again, errno::get()), (null_mut(), ESRCH));
    s::hdestroy_r(Some(&mut t));
}

#[test]
fn sym_hdestroy_r() {
    let mut t = htab();
    s::hcreate_r(3, Some(&mut t));
    s::hdestroy_r(Some(&mut t));
    assert_eq!((t.table, t.size, t.filled), (0, 0, 0));
    errno::set_raw(0);
    s::hdestroy_r(None);
    assert_ne!(errno::get(), 0, "NULL: an error, not a crash");
}

// ------------------------------------------------------------------------------ queues

#[test]
fn sym_insque() {
    let mut q = [[0usize; 3]; 3];
    let (a, b, c) = (q[0].as_mut_ptr(), q[1].as_mut_ptr(), q[2].as_mut_ptr());
    s::insque(a.cast(), null_mut());
    s::insque(c.cast(), a.cast());
    s::insque(b.cast(), a.cast());
    let (a, b, c) = (a.addr(), b.addr(), c.addr());
    assert_eq!((q[0][0], q[0][1]), (b, 0), "a starts a linear list");
    assert_eq!((q[1][0], q[1][1]), (c, a));
    assert_eq!((q[2][0], q[2][1]), (0, b));
}

#[test]
fn sym_remque() {
    let mut q = [[0usize; 2]; 3];
    let (a, b, c) = (q[0].as_mut_ptr(), q[1].as_mut_ptr(), q[2].as_mut_ptr());
    s::insque(a.cast(), null_mut());
    s::insque(b.cast(), a.cast());
    s::insque(c.cast(), b.cast());
    s::remque(b.cast());
    let (a, c) = (a.addr(), c.addr());
    assert_eq!((q[0][0], q[2][1]), (c, a), "the neighbours are joined");
}

// ------------------------------------------------------------------------------ §14.9 regular expressions (phase 12)
//
// Each export's translation: the C arguments (NUL-terminated or length-delimited text, the
// `regex_t` fields, `pmatch`, `struct re_registers`) and the result convention. The engine's
// behaviour is covered by the §14.9 clause tests and the behaviour record
// (`regex_record.rs`, `patterns_products.rs`).

mod rx {
    use std::ffi::CString;
    use std::sync::Mutex;

    pub(crate) use liella_abi::layout::{regex_t, regmatch_t, struct_re_registers};
    pub(crate) use liella_boundary::posix::s14_search as s;

    /// `re_syntax_options`, `re_max_failures` and the `re_comp` pattern are process-wide.
    pub(crate) static GLOBAL: Mutex<()> = Mutex::new(());

    pub(crate) const POSIX_EXTENDED: u64 = 0x3b2fc;
    pub(crate) const POSIX_BASIC: u64 = 0x10102c6;

    pub(crate) fn z(s: &str) -> CString {
        CString::new(s).unwrap()
    }

    pub(crate) fn blank() -> regex_t {
        regex_t { buffer: 0, allocated: 0, used: 0, syntax: 0, fastmap: 0, translate: 0, re_nsub: 0, can_be_null: [0] }
    }

    /// The text at `p` (a NUL-terminated string the library returned).
    pub(crate) fn text(p: *const i8) -> String {
        let mut out = String::new();
        let mut k = 0usize;
        loop {
            let mut c = 0u8;
            liella_boundary::posix::s04_strings::memcpy((&raw mut c).cast(), p.wrapping_add(k).cast(), 1);
            if c == 0 {
                return out;
            }
            out.push(char::from(c));
            k += 1;
        }
    }

    /// Entry `i` of a register pair.
    pub(crate) fn reg(r: &struct_re_registers, i: usize) -> (i32, i32) {
        assert!(i < r.num_regs as usize);
        let read = |addr: usize| {
            let mut v = 0i32;
            liella_boundary::posix::s04_strings::memcpy((&raw mut v).cast(), core::ptr::with_exposed_provenance::<core::ffi::c_void>(addr + 4 * i), 4);
            v
        };
        (read(r.start), read(r.end))
    }

    /// Compile `pat` with the GNU interface under `syntax`.
    pub(crate) fn gnu(pat: &str, syntax: u64) -> regex_t {
        s::re_set_syntax(syntax);
        let mut b = blank();
        assert!(s::re_compile_pattern(pat.as_ptr().cast(), pat.len(), Some(&mut b)).is_null(), "{pat}");
        b
    }
}

#[test]
fn sym_regcomp() {
    use rx::*;
    let mut re = blank();
    assert_eq!(s::regcomp(Some(&mut re), z("(a)(b)").as_ptr(), 1), 0);
    assert_eq!((re.re_nsub, re.buffer != 0, re.used == re.allocated), (2, true, true));
    assert_eq!(re.syntax as u64, POSIX_EXTENDED, "the syntax regcomp compiled with");
    assert_eq!(re.can_be_null[0] & 0x91, 0, "can_be_null, no_sub and newline_anchor clear");
    s::regfree(Some(&mut re));
    assert_eq!(s::regcomp(Some(&mut re), z("a*").as_ptr(), 4 | 8 | 2), 0);
    assert_eq!(re.syntax as u64, (POSIX_BASIC & !0x40) | 0x100 | 0x40_0000, "REG_NEWLINE and REG_ICASE in the syntax");
    assert_eq!(re.can_be_null[0] & 0x91, 0x91, "can_be_null; REG_NOSUB is no_sub, REG_NEWLINE is newline_anchor");
    s::regfree(Some(&mut re));
    assert_eq!(s::regcomp(Some(&mut re), z("(").as_ptr(), 1), 8, "the error code");
    assert_eq!(s::regcomp(Some(&mut re), z("a\\)").as_ptr(), 0), 8, "REG_EPAREN (REG_ERPAREN is never returned)");
    assert_eq!(s::regcomp(None, z("a").as_ptr(), 0), 2, "a NULL regex_t is REG_BADPAT (no crash)");
    assert_eq!(s::regcomp(Some(&mut re), core::ptr::null(), 0), 2, "a NULL pattern is REG_BADPAT");
}

/// `regexec@@GLIBC_2.3.4` (the default version). `regexec@GLIBC_2.2.5` is a compat-only version
/// and comes with phase 15.
#[test]
fn sym_regexec() {
    use rx::*;
    let mut re = blank();
    assert_eq!(s::regcomp(Some(&mut re), z("b(c)?").as_ptr(), 1), 0);
    let mut m = [regmatch_t { rm_so: 77, rm_eo: 77 }; 4];
    assert_eq!(s::regexec(Some(&re), z("abd").as_ptr(), 4, m.as_mut_ptr(), 0), 0);
    let got: Vec<(i32, i32)> = m.iter().map(|x| (x.rm_so, x.rm_eo)).collect();
    assert_eq!(got, [(1, 2), (-1, -1), (-1, -1), (-1, -1)], "exactly nmatch entries, -1 beyond re_nsub");
    let mut m = [regmatch_t { rm_so: 77, rm_eo: 77 }; 2];
    assert_eq!(s::regexec(Some(&re), z("abc").as_ptr(), 1, m.as_mut_ptr(), 0), 0);
    assert_eq!((m[0].rm_so, m[0].rm_eo, m[1].rm_so), (1, 3, 77), "nmatch=1 leaves the rest");
    assert_eq!(s::regexec(Some(&re), z("xyz").as_ptr(), 2, m.as_mut_ptr(), 0), 1, "REG_NOMATCH");
    assert_eq!(s::regexec(Some(&re), z("abc").as_ptr(), 0, core::ptr::null_mut(), 0), 0, "nmatch 0 with no array");
    // REG_STARTEND: the range of pmatch[0], NUL included; offsets from the buffer start.
    let buf = b"b\0bc\0";
    let mut m = [regmatch_t { rm_so: 1, rm_eo: 4 }, regmatch_t { rm_so: 0, rm_eo: 0 }];
    assert_eq!(s::regexec(Some(&re), buf.as_ptr().cast(), 2, m.as_mut_ptr(), 4), 0);
    assert_eq!((m[0].rm_so, m[0].rm_eo, m[1].rm_so, m[1].rm_eo), (2, 4, 3, 4));
    s::regfree(Some(&mut re));
    let mut re = blank();
    assert_eq!(s::regcomp(Some(&mut re), z("^a").as_ptr(), 0), 0);
    assert_eq!(s::regexec(Some(&re), z("a").as_ptr(), 0, core::ptr::null_mut(), 1), 1, "REG_NOTBOL");
    s::regfree(Some(&mut re));
    assert_eq!(s::regexec(None, z("a").as_ptr(), 0, core::ptr::null_mut(), 0), 2, "a NULL regex_t: REG_BADPAT");
    let freed = blank();
    assert_eq!(s::regexec(Some(&freed), z("a").as_ptr(), 0, core::ptr::null_mut(), 0), 2, "an uncompiled regex_t: REG_BADPAT");
}

#[test]
fn sym_regerror() {
    use rx::*;
    let mut b = [b'Z' as i8; 8];
    assert_eq!(s::regerror(8, None, b.as_mut_ptr(), 8), 18, "the length of the message plus one");
    assert_eq!(text(b.as_ptr()), "Unmatch", "truncated and terminated");
    assert_eq!(s::regerror(8, None, core::ptr::null_mut(), 0), 18);
    let mut big = [0i8; 40];
    assert_eq!(s::regerror(-1, None, big.as_mut_ptr(), 40), 14);
    assert_eq!(text(big.as_ptr()), "Unknown error");
}

#[test]
fn sym_regfree() {
    use rx::*;
    let mut re = blank();
    assert_eq!(s::regcomp(Some(&mut re), z("a").as_ptr(), 0), 0);
    s::regfree(Some(&mut re));
    assert_eq!((re.buffer, re.allocated, re.used, re.fastmap, re.translate), (0, 0, 0, 0, 0));
    s::regfree(Some(&mut re));
    s::regfree(None);
}

#[test]
fn sym_re_set_syntax() {
    use rx::*;
    let _g = GLOBAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let old = s::re_set_syntax(POSIX_EXTENDED);
    assert_eq!(s::re_set_syntax(POSIX_BASIC), POSIX_EXTENDED, "the previous syntax");
    assert_eq!(liella_boundary::data::re_syntax_options.load(std::sync::atomic::Ordering::Relaxed), POSIX_BASIC);
    s::re_set_syntax(old);
}

#[test]
fn sym_re_syntax_options() {
    use rx::*;
    use std::sync::atomic::Ordering::Relaxed;
    let _g = GLOBAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let o = &liella_boundary::data::re_syntax_options;
    let old = o.load(Relaxed);
    // Emacs syntax (0): `|` is literal and `\|` alternation.
    o.store(0, Relaxed);
    let mut b = blank();
    assert!(s::re_compile_pattern(b"a|b".as_ptr().cast(), 3, Some(&mut b)).is_null());
    assert_eq!(s::re_search(Some(&mut b), b"a|b".as_ptr().cast(), 3, 0, 3, None), 0);
    assert_eq!(s::re_search(Some(&mut b), b"b".as_ptr().cast(), 1, 0, 1, None), -1);
    s::regfree(Some(&mut b));
    o.store(POSIX_EXTENDED, Relaxed);
    let mut b = blank();
    assert!(s::re_compile_pattern(b"a|b".as_ptr().cast(), 3, Some(&mut b)).is_null());
    assert_eq!(s::re_search(Some(&mut b), b"b".as_ptr().cast(), 1, 0, 1, None), 0, "the value in force when compiling");
    s::regfree(Some(&mut b));
    o.store(old, Relaxed);
}

#[test]
fn sym_re_max_failures() {
    use rx::*;
    use std::sync::atomic::Ordering::Relaxed;
    let _g = GLOBAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let f = &liella_boundary::data::re_max_failures;
    assert_eq!(f.load(Relaxed), 2000, "the initial value");
    let mut re = blank();
    assert_eq!(s::regcomp(Some(&mut re), z("(a*)*(b*)*\\1\\2c").as_ptr(), 1), 0);
    let subj = z(&"ab".repeat(3000));
    f.store(1, Relaxed);
    assert_eq!(s::regexec(Some(&re), subj.as_ptr(), 0, core::ptr::null_mut(), 0), 12, "past the budget: REG_ESPACE");
    f.store(2000, Relaxed);
    let small = z("aabc");
    assert_eq!(s::regexec(Some(&re), small.as_ptr(), 0, core::ptr::null_mut(), 0), 0);
    s::regfree(Some(&mut re));
}

#[test]
fn sym_re_compile_pattern() {
    use rx::*;
    let _g = GLOBAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let old = s::re_set_syntax(POSIX_EXTENDED);
    let mut b = blank();
    // Length-delimited: the bytes after `length` are not part of the pattern.
    assert!(s::re_compile_pattern(b"(ab)XX".as_ptr().cast(), 4, Some(&mut b)).is_null());
    assert_eq!((b.re_nsub, b.syntax as u64), (1, POSIX_EXTENDED));
    assert_eq!(b.can_be_null[0] & 0x80, 0x80, "newline_anchor is set");
    assert_eq!(s::re_search(Some(&mut b), b"xabXX".as_ptr().cast(), 5, 0, 5, None), 1);
    assert_eq!(s::re_search(Some(&mut b), b"XX".as_ptr().cast(), 2, 0, 2, None), -1);
    s::regfree(Some(&mut b));
    let e = s::re_compile_pattern(b"a{2,1}".as_ptr().cast(), 6, Some(&mut b));
    assert_eq!(text(e), "Invalid content of \\{\\}");
    let e = s::re_compile_pattern(b"a)".as_ptr().cast(), 2, Some(&mut b));
    assert!(e.is_null(), "an unmatched ) is ordinary in POSIX_EXTENDED");
    s::regfree(Some(&mut b));
    s::re_set_syntax(POSIX_EXTENDED & !(1 << 17));
    let e = s::re_compile_pattern(b"a)".as_ptr().cast(), 2, Some(&mut b));
    assert_eq!(text(e), "Unmatched ) or \\)", "the GNU interface reports REG_ERPAREN");
    s::re_set_syntax(old);
}

#[test]
fn sym_re_compile_fastmap() {
    use rx::*;
    let _g = GLOBAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let old = s::re_set_syntax(POSIX_EXTENDED);
    let mut map = vec![0u8; 256];
    let mut b = blank();
    b.fastmap = map.as_mut_ptr().expose_provenance();
    assert!(s::re_compile_pattern(b"x[yz]".as_ptr().cast(), 5, Some(&mut b)).is_null());
    assert_eq!(b.fastmap, map.as_ptr().expose_provenance(), "the caller's fastmap is kept");
    assert_eq!(s::re_compile_fastmap(Some(&mut b)), 0);
    assert!(map[usize::from(b'x')] != 0, "a byte a match can start with is marked");
    assert_eq!(b.can_be_null[0] & 0x08, 0x08, "fastmap_accurate");
    assert_eq!(s::re_search(Some(&mut b), b"aaxz".as_ptr().cast(), 4, 0, 4, None), 2);
    b.fastmap = 0; // the vector is the test's; regfree would free a malloc'ed one
    s::regfree(Some(&mut b));
    s::re_set_syntax(old);
}

#[test]
fn sym_re_search() {
    use rx::*;
    let _g = GLOBAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut b = gnu("(a)(b)?", POSIX_EXTENDED);
    let mut regs = struct_re_registers { num_regs: 0, start: 0, end: 0 };
    assert_eq!(s::re_search(Some(&mut b), b"xxa".as_ptr().cast(), 3, 0, 3, Some(&mut regs)), 2);
    assert_eq!((regs.num_regs, reg(&regs, 0), reg(&regs, 1), reg(&regs, 2), reg(&regs, 3)), (4, (2, 3), (2, 3), (-1, -1), (-1, -1)), "allocated: re_nsub + 2 registers");
    assert_eq!(b.can_be_null[0] & 0x06, 0x02, "regs_allocated is REGS_REALLOCATE");
    assert_eq!(s::re_search(Some(&mut b), b"abcab".as_ptr().cast(), 5, 4, -4, None), 3, "a negative range searches backwards");
    assert_eq!(s::re_search(Some(&mut b), b"abcab".as_ptr().cast(), 5, 1, 100, None), 3, "a range past the end is clipped");
    assert_eq!(s::re_search(Some(&mut b), b"abcab".as_ptr().cast(), 5, 6, 1, None), -1, "a start outside the buffer");
    assert_eq!(s::re_search(Some(&mut b), b"abcab".as_ptr().cast(), 5, -1, 1, None), -1);
    assert_eq!(s::re_search(None, b"a".as_ptr().cast(), 1, 0, 1, None), -2, "a NULL buffer: -2");
    // A negative size (ptx passes `limit - cursor`): no match, -1, as glibc (T26), not -2.
    assert_eq!(s::re_search(Some(&mut b), b"abcab".as_ptr().cast(), -3, 0, 5, None), -1, "a negative size");
    assert_eq!(s::re_search(Some(&mut b), b"abcab".as_ptr().cast(), -1, 0, 0, None), -1, "a negative size, empty range");
    assert_eq!(s::re_match(Some(&mut b), b"abcab".as_ptr().cast(), -2, 0, None), -1, "re_match with a negative size");
    // `^` does not match at a start inside the buffer; not_bol is honoured.
    let mut c = gnu("^b", POSIX_EXTENDED);
    assert_eq!(s::re_search(Some(&mut c), b"ab".as_ptr().cast(), 2, 1, 1, None), -1);
    assert_eq!(s::re_search(Some(&mut c), b"a\nb".as_ptr().cast(), 3, 0, 3, None), 2, "newline_anchor");
    c.can_be_null[0] |= 0x20;
    assert_eq!(s::re_search(Some(&mut c), b"b".as_ptr().cast(), 1, 0, 1, None), -1, "not_bol");
    s::regfree(Some(&mut c));
    // no_sub: no registers.
    b.can_be_null[0] |= 0x10;
    let mut none = struct_re_registers { num_regs: 0, start: 0, end: 0 };
    assert_eq!(s::re_search(Some(&mut b), b"a".as_ptr().cast(), 1, 0, 1, Some(&mut none)), 0);
    assert_eq!(none.num_regs, 0);
    s::regfree(Some(&mut b));
}

#[test]
fn sym_re_search_2() {
    use rx::*;
    let _g = GLOBAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut b = gnu("a(b+)c", POSIX_EXTENDED);
    let mut regs = struct_re_registers { num_regs: 0, start: 0, end: 0 };
    let (s1, s2) = (b"xab", b"bcy");
    assert_eq!(s::re_search_2(Some(&mut b), s1.as_ptr().cast(), 3, s2.as_ptr().cast(), 3, 0, 6, Some(&mut regs), 6), 1);
    assert_eq!((reg(&regs, 0), reg(&regs, 1)), ((1, 5), (2, 4)), "offsets in the joined string");
    assert_eq!(s::re_search_2(Some(&mut b), s1.as_ptr().cast(), 3, s2.as_ptr().cast(), 3, 0, 6, None, 4), -1, "stop ends the text");
    assert_eq!(s::re_search_2(Some(&mut b), core::ptr::null(), 0, b"abbc".as_ptr().cast(), 4, 0, 4, None, 4), 0, "an empty first string");
    s::regfree(Some(&mut b));
}

#[test]
fn sym_re_match() {
    use rx::*;
    let _g = GLOBAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut b = gnu("a|ab", POSIX_EXTENDED);
    assert_eq!(s::re_match(Some(&mut b), b"xab".as_ptr().cast(), 3, 1, None), 2, "the longest match at start, as a length");
    assert_eq!(s::re_match(Some(&mut b), b"xab".as_ptr().cast(), 3, 0, None), -1);
    s::regfree(Some(&mut b));
}

#[test]
fn sym_re_match_2() {
    use rx::*;
    let _g = GLOBAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut b = gnu("a(b+)c", POSIX_EXTENDED);
    assert_eq!(s::re_match_2(Some(&mut b), b"xab".as_ptr().cast(), 3, b"bcy".as_ptr().cast(), 3, 1, None, 6), 4, "across the boundary");
    assert_eq!(s::re_match_2(Some(&mut b), b"xab".as_ptr().cast(), 3, b"bcy".as_ptr().cast(), 3, 1, None, 3), -1);
    s::regfree(Some(&mut b));
}

#[test]
fn sym_re_set_registers() {
    use rx::*;
    let _g = GLOBAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut b = gnu("a(b+)c", POSIX_EXTENDED);
    let starts: &'static mut [i32] = Vec::leak(vec![0i32; 3]);
    let ends: &'static mut [i32] = Vec::leak(vec![0i32; 3]);
    let mut regs = struct_re_registers { num_regs: 0, start: 0, end: 0 };
    s::re_set_registers(Some(&mut b), Some(&mut regs), 3, starts.as_mut_ptr(), ends.as_mut_ptr());
    assert_eq!((regs.num_regs, b.can_be_null[0] & 0x06), (3, 0x02), "REGS_REALLOCATE with the caller's arrays");
    assert_eq!(s::re_search(Some(&mut b), b"zabbbc".as_ptr().cast(), 6, 0, 6, Some(&mut regs)), 1);
    assert_eq!((regs.num_regs, regs.start, reg(&regs, 0), reg(&regs, 1), reg(&regs, 2)), (3, starts.as_ptr().expose_provenance(), (1, 6), (2, 5), (-1, -1)), "large enough: used in place");
    s::re_set_registers(Some(&mut b), Some(&mut regs), 0, core::ptr::null_mut(), core::ptr::null_mut());
    assert_eq!((regs.num_regs, regs.start, regs.end, b.can_be_null[0] & 0x06), (0, 0, 0, 0), "0: back to REGS_UNALLOCATED");
    s::regfree(Some(&mut b));
}

#[test]
fn sym_re_comp() {
    use rx::*;
    let _g = GLOBAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let old = s::re_set_syntax(0);
    assert!(s::re_comp(z("x\\(y\\)").as_ptr()).is_null());
    assert!(s::re_comp(core::ptr::null()).is_null(), "NULL with a pattern compiled");
    assert_eq!(text(s::re_comp(z("\\(").as_ptr())), "Unmatched ( or \\(");
    assert_eq!(text(s::re_comp(core::ptr::null())), "No previous regular expression");
    s::re_set_syntax(old);
}

#[test]
fn sym_re_exec() {
    use rx::*;
    let _g = GLOBAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let old = s::re_set_syntax(0);
    assert!(s::re_comp(z("a\\|b").as_ptr()).is_null());
    assert_eq!((s::re_exec(z("xb").as_ptr()), s::re_exec(z("xc").as_ptr())), (1, 0));
    assert!(!s::re_comp(z("\\(").as_ptr()).is_null());
    assert_eq!(s::re_exec(z("abc").as_ptr()), 0, "no pattern: 0, never a crash");
    s::re_set_syntax(old);
}

// ------------------------------------------------------------------------------ §14.7, §14.8, §14.10 (phase 12)

mod pt {
    use core::ffi::c_void;
    use std::cell::RefCell;
    use std::path::PathBuf;

    pub(crate) use liella_abi::callback::GlobT;
    pub(crate) use liella_abi::layout::wordexp_t;
    pub(crate) use liella_boundary::posix::s14_search as s;

    pub(crate) const ERR: i32 = 1;
    pub(crate) const MARK: i32 = 2;
    pub(crate) const DOOFFS: i32 = 8;
    pub(crate) const NOCHECK: i32 = 16;
    pub(crate) const APPEND: i32 = 32;
    pub(crate) const MAGCHAR: i32 = 256;

    pub(crate) fn z(s: &str) -> std::ffi::CString {
        std::ffi::CString::new(s).unwrap()
    }

    pub(crate) fn blank() -> GlobT {
        GlobT { gl_pathc: 0, gl_pathv: 0, gl_offs: 0, gl_flags: 0, gl_closedir: None, gl_readdir: None, gl_opendir: None, gl_lstat: None, gl_stat: None }
    }

    fn word(addr: usize, i: usize) -> usize {
        let mut v = 0usize;
        liella_boundary::posix::s04_strings::memcpy((&raw mut v).cast(), core::ptr::with_exposed_provenance::<c_void>(addr + 8 * i), 8);
        v
    }

    pub(crate) fn text(p: usize) -> String {
        let mut out = Vec::new();
        loop {
            let mut ch = 0u8;
            liella_boundary::posix::s04_strings::memcpy((&raw mut ch).cast(), core::ptr::with_exposed_provenance::<c_void>(p + out.len()), 1);
            if ch == 0 {
                return String::from_utf8_lossy(&out).into_owned();
            }
            out.push(ch);
        }
    }

    /// The `n` entries of a result vector, the last name of each path (`None`: a null slot).
    pub(crate) fn names(v: usize, n: usize) -> Vec<Option<String>> {
        (0..n).map(|i| match word(v, i) {
            0 => None,
            p => Some(text(p).rsplit('/').next().unwrap().to_string()),
        }).collect()
    }

    /// The text of a vector's first entry.
    pub(crate) fn first(v: usize) -> String {
        text(word(v, 0))
    }

    pub(crate) fn some(items: &[&str]) -> Vec<Option<String>> {
        items.iter().map(|s| if s.is_empty() { None } else { Some(s.to_string()) }).collect()
    }

    /// A scratch tree under `target/` for this test: `f1`, `f2`, a directory `d` holding `x`,
    /// an unreadable directory `noread`, a dangling link `dang`.
    pub(crate) fn tree(name: &str) -> PathBuf {
        let d = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-scratch").join(format!("s14sym-{name}-{}", std::process::id()));
        if d.exists() {
            let _ = std::fs::set_permissions(d.join("noread"), std::os::unix::fs::PermissionsExt::from_mode(0o755));
            std::fs::remove_dir_all(&d).unwrap();
        }
        std::fs::create_dir_all(d.join("d")).unwrap();
        std::fs::create_dir_all(d.join("noread")).unwrap();
        for f in ["f1", "f2", "d/x", "noread/y"] {
            std::fs::write(d.join(f), b"").unwrap();
        }
        std::os::unix::fs::symlink("nowhere", d.join("dang")).unwrap();
        std::fs::set_permissions(d.join("noread"), std::os::unix::fs::PermissionsExt::from_mode(0o300)).unwrap();
        std::fs::canonicalize(&d).unwrap()
    }

    pub(crate) fn remove(d: &std::path::Path) {
        let _ = std::fs::set_permissions(d.join("noread"), std::os::unix::fs::PermissionsExt::from_mode(0o755));
        std::fs::remove_dir_all(d).unwrap();
    }

    thread_local! {
        pub(crate) static ERRS: RefCell<Vec<(String, i32)>> = const { RefCell::new(Vec::new()) };
        pub(crate) static ERR_RET: RefCell<i32> = const { RefCell::new(0) };
    }

    pub(crate) extern "C-unwind" fn errfunc(p: *const core::ffi::c_char, e: core::ffi::c_int) -> core::ffi::c_int {
        ERRS.with(|v| v.borrow_mut().push((text(p.expose_provenance()).rsplit('/').next().unwrap().to_string(), e)));
        ERR_RET.with(|r| *r.borrow())
    }

    pub(crate) fn root_can_read_anything() -> bool {
        liella_boundary::posix::s17_users::geteuid() == 0
    }
}

/// `fnmatch@@GLIBC_2.2.5`.
#[test]
fn sym_fnmatch() {
    use pt::*;
    let m = |p: &str, t: &str, f: i32| s::fnmatch(z(p).as_ptr(), z(t).as_ptr(), f);
    assert_eq!((m("a*", "abc", 0), m("a?", "abc", 0), m("[!a]*", "abc", 0)), (0, 1, 1), "0 or FNM_NOMATCH (1)");
    assert_eq!((m("*", "a/b", 1), m("*", ".a", 4), m("A", "a", 16), m("a", "a/b", 8), m("@(a|b)", "b", 32), m("\\*", "\\x", 2)), (1, 1, 0, 0, 0, 0), "the flag bits of <fnmatch.h>");
    assert_eq!(m("a", "a", 1 << 20), 0, "unknown bits are ignored");
    assert_eq!(s::fnmatch(core::ptr::null(), z("a").as_ptr(), 0), -1, "NULL: an error value, never a crash");
    assert_eq!(s::fnmatch(z("a").as_ptr(), core::ptr::null(), 0), -1);
}

// no spec clause: compat-plan.tsv behaviour rows glob/glob64@GLIBC_2.2.5 (LG-2; the plan's
// ledger and spec/behaviour/patterns.glibc.txt.gz "glob compat", v/dang lines).
#[test]
fn compat_glob_2_2_5_altdirfunc_checks_with_gl_stat() {
    // Trace: no spec clause (compat-plan.tsv behaviour rows glob/glob64@GLIBC_2.2.5; LG-2).
    use pt::*;
    use std::sync::atomic::{AtomicU32, Ordering::SeqCst};
    use liella_abi::callback::{GlobClosedirFn, GlobOpendirFn, GlobReaddirFn, GlobStatFn};
    static LSTAT: AtomicU32 = AtomicU32::new(0);
    static STAT: AtomicU32 = AtomicU32::new(0);
    extern "C-unwind" fn lst(_: *const c_char, _: *mut c_void) -> c_int {
        LSTAT.fetch_add(1, SeqCst);
        0
    }
    extern "C-unwind" fn st(_: *const c_char, _: *mut c_void) -> c_int {
        STAT.fetch_add(1, SeqCst);
        errno::set_raw(2);
        -1
    }
    extern "C-unwind" fn od(_: *const c_char) -> *mut c_void {
        null_mut()
    }
    extern "C-unwind" fn rd(_: *mut c_void) -> *mut c_void {
        null_mut()
    }
    extern "C-unwind" fn cd(_: *mut c_void) {}
    const ALTDIRFUNC: i32 = 1 << 9;
    let run = |old: bool, flags: i32| {
        let mut g = blank();
        g.gl_lstat = Some(lst as GlobStatFn);
        g.gl_stat = Some(st as GlobStatFn);
        g.gl_opendir = Some(od as GlobOpendirFn);
        g.gl_readdir = Some(rd as GlobReaddirFn);
        g.gl_closedir = Some(cd as GlobClosedirFn);
        LSTAT.store(0, SeqCst);
        STAT.store(0, SeqCst);
        let p = z("v/dang");
        let r = if old { s::liella_compat_glob(p.as_ptr(), flags, None, Some(&mut g)) } else { s::glob(p.as_ptr(), flags, None, Some(&mut g)) };
        let out = (r, g.gl_pathc, STAT.load(SeqCst) > 0, LSTAT.load(SeqCst) > 0);
        s::globfree(Some(&mut g));
        out
    };
    assert_eq!(run(true, ALTDIRFUNC), (3, 0, true, false), "glob@2.2.5: gl_stat fails, GLOB_NOMATCH, gl_lstat not called");
    assert_eq!(run(false, ALTDIRFUNC), (0, 1, false, true), "glob@@2.27: gl_lstat, found");
    assert_eq!(run(true, ALTDIRFUNC | NOCHECK), (0, 1, false, false), "NOCHECK: neither called");
    errno::set_raw(0);
}

/// `glob@@GLIBC_2.27`, the current contract (the default symbol). The compat version
/// `glob@GLIBC_2.2.5` (`gl_stat` instead of `gl_lstat` for names without wildcards) comes with
/// phase 15, as the plan's ledger records.
#[test]
fn sym_glob() {
    use pt::*;
    let d = tree("glob");
    let p = |rest: &str| z(&format!("{}/{rest}", d.display()));
    let mut g = blank();
    g.gl_offs = 3;
    assert_eq!(s::glob(p("f*").as_ptr(), 0, None, Some(&mut g)), 0);
    assert_eq!((g.gl_pathc, g.gl_offs, g.gl_flags, names(g.gl_pathv, 3)), (2, 0, MAGCHAR, some(&["f1", "f2", ""])), "gl_offs ignored and reset; gl_flags with GLOB_MAGCHAR");
    s::globfree(Some(&mut g));
    // A name without wildcards: gl_lstat semantics (a dangling link exists); GLOB_MARK.
    let mut g = blank();
    assert_eq!(s::glob(p("dang").as_ptr(), MARK, None, Some(&mut g)), 0);
    assert_eq!((g.gl_flags, names(g.gl_pathv, 2)), (MARK, some(&["dang", ""])));
    s::globfree(Some(&mut g));
    let mut g = blank();
    assert_eq!(s::glob(p("d").as_ptr(), MARK, None, Some(&mut g)), 0);
    assert!(pt::first(g.gl_pathv).ends_with("/d/"), "a directory gets a slash");
    s::globfree(Some(&mut g));
    // GLOB_NOMATCH: nothing written to gl_flags; with GLOB_DOOFFS the null slots.
    let mut g = blank();
    g.gl_flags = 77;
    g.gl_offs = 2;
    assert_eq!(s::glob(p("none*").as_ptr(), DOOFFS, None, Some(&mut g)), 3);
    assert_eq!((g.gl_pathc, g.gl_flags, names(g.gl_pathv, 3)), (0, 77, some(&["", "", ""])));
    s::globfree(Some(&mut g));
    // The error function receives the directory and errno; nonzero aborts, as GLOB_ERR does.
    if !root_can_read_anything() {
        ERR_RET.with(|r| *r.borrow_mut() = 0);
        ERRS.with(|v| v.borrow_mut().clear());
        let mut g = blank();
        assert_eq!(s::glob(p("*/*").as_ptr(), 0, Some(errfunc), Some(&mut g)), 0);
        assert_eq!(names(g.gl_pathv, 2), some(&["x", ""]));
        assert_eq!(ERRS.with(|v| v.borrow().clone()), vec![("noread".to_string(), 13)]);
        s::globfree(Some(&mut g));
        let mut g = blank();
        assert_eq!(s::glob(p("*/*").as_ptr(), ERR, None, Some(&mut g)), 2, "GLOB_ABORTED");
        assert_eq!((g.gl_pathc, g.gl_pathv), (0, 0), "no partial results");
        // An aborted GLOB_APPEND keeps the earlier results.
        assert_eq!(s::glob(p("f1").as_ptr(), 0, None, Some(&mut g)), 0);
        ERR_RET.with(|r| *r.borrow_mut() = 1);
        assert_eq!(s::glob(p("*/*").as_ptr(), APPEND, Some(errfunc), Some(&mut g)), 2);
        assert_eq!((g.gl_pathc, names(g.gl_pathv, 2)), (1, some(&["f1", ""])));
        s::globfree(Some(&mut g));
    }
    // GLOB_NOCHECK; errors of the call itself.
    let mut g = blank();
    assert_eq!(s::glob(z("no/such/*").as_ptr(), NOCHECK, None, Some(&mut g)), 0);
    assert_eq!((g.gl_pathc, pt::first(g.gl_pathv)), (1, "no/such/*".to_string()));
    s::globfree(Some(&mut g));
    assert_eq!(s::glob(z("*").as_ptr(), 1 << 15, None, Some(&mut g)), -1);
    assert_eq!(liella_boundary::errno::get(), 22, "unknown flag bits: EINVAL");
    assert_eq!(s::glob(core::ptr::null(), 0, None, Some(&mut g)), -1);
    assert_eq!(liella_boundary::errno::get(), 14, "a NULL pattern: EFAULT");
    assert_eq!(s::glob(z("*").as_ptr(), 0, None, None), -1, "a NULL glob_t: EFAULT, never a crash");
    remove(&d);
}

/// `glob64@@GLIBC_2.27`: `glob` on `glob64_t`, the same layout on x86-64.
#[test]
fn sym_glob64() {
    use pt::*;
    let d = tree("glob64");
    let mut g = blank();
    assert_eq!(s::glob64(z(&format!("{}/f?", d.display())).as_ptr(), 0, None, Some(&mut g)), 0);
    assert_eq!((g.gl_pathc, names(g.gl_pathv, 3)), (2, some(&["f1", "f2", ""])));
    s::globfree64(Some(&mut g));
    remove(&d);
}

/// `glob_pattern_p@@GLIBC_2.2.5`.
#[test]
fn sym_glob_pattern_p() {
    use pt::*;
    let p = |pat: &str, q: i32| s::glob_pattern_p(z(pat).as_ptr(), q);
    assert_eq!((p("a*", 1), p("a?", 0), p("a[b]", 1), p("a[", 1), p("a", 1), p("a\\*", 1), p("a\\*", 0)), (1, 1, 1, 0, 0, 0, 1));
    assert_eq!(s::glob_pattern_p(core::ptr::null(), 0), 0, "NULL: no wildcards");
}

/// `globfree@@GLIBC_2.2.5`.
#[test]
fn sym_globfree() {
    use pt::*;
    let mut g = blank();
    s::globfree(Some(&mut g));
    s::globfree(None);
    assert_eq!(s::glob(z("/").as_ptr(), DOOFFS, None, Some(&mut g)), 0);
    assert_ne!(g.gl_pathv, 0);
    s::globfree(Some(&mut g));
    assert_eq!(g.gl_pathv, 0, "the vector is gone");
    s::globfree(Some(&mut g));
}

/// `globfree64@@GLIBC_2.2.5`.
#[test]
fn sym_globfree64() {
    use pt::*;
    let mut g = blank();
    assert_eq!(s::glob64(z("/").as_ptr(), 0, None, Some(&mut g)), 0);
    s::globfree64(Some(&mut g));
    assert_eq!(g.gl_pathv, 0);
    s::globfree64(None);
}

/// `wordexp@@GLIBC_2.2.5`.
#[test]
fn sym_wordexp() {
    use pt::*;
    let mut w = wordexp_t { we_wordc: 0, we_wordv: 0, we_offs: 1 };
    assert_eq!(s::wordexp(z("a 'b c' $((6*7)) $(echo hi)").as_ptr(), Some(&mut w), 1), 0);
    assert_eq!((w.we_wordc, w.we_offs, names(w.we_wordv, 6)), (4, 1, some(&["", "a", "b c", "42", "hi", ""])), "WRDE_DOOFFS slots, then the words, then NULL");
    assert_eq!(s::wordexp(z("more").as_ptr(), Some(&mut w), 1 | 2), 0);
    assert_eq!((w.we_wordc, names(w.we_wordv, 7)), (5, some(&["", "a", "b c", "42", "hi", "more", ""])), "WRDE_APPEND");
    s::wordfree(Some(&mut w));
    // Each failure: the code, and the structure untouched.
    for (words, flags, code) in [("a|b", 0, 2), ("$NO_SUCH_VARIABLE_ZQ", 32, 3), ("$(echo x)", 4, 4), ("'a", 0, 5), ("$((1/0))", 0, 5), ("$((1<<64))", 0, 5)] {
        let mut w = wordexp_t { we_wordc: 777, we_wordv: 0, we_offs: 0 };
        assert_eq!(s::wordexp(z(words).as_ptr(), Some(&mut w), flags), code, "{words}");
        assert_eq!((w.we_wordc, w.we_wordv), (777, 0), "{words}");
    }
    assert_eq!(s::wordexp(z("a").as_ptr(), None, 0), 5, "a NULL wordexp_t: an error, never a crash");
    assert_eq!(liella_boundary::errno::get(), 14);
    // ${X:=w} assigns for the rest of the call only; the environment is never written (owner
    // ruling 2026-10-03, D-14.7).
    let mut w = wordexp_t { we_wordc: 0, we_wordv: 0, we_offs: 0 };
    assert_eq!(s::wordexp(z("${LIELLA_WX_ASSIGN_Q:=v} $LIELLA_WX_ASSIGN_Q").as_ptr(), Some(&mut w), 0), 0);
    assert_eq!((w.we_wordc, names(w.we_wordv, 3)), (2, some(&["v", "v", ""])), "a later reference in the same call sees the value");
    s::wordfree(Some(&mut w));
    assert!(liella_boundary::posix::s01_startup::getenv(z("LIELLA_WX_ASSIGN_Q").as_ptr()).is_null(), "the environment is untouched");
    assert_eq!(s::wordexp(z("${LIELLA_WX_ASSIGN_Q-unset}").as_ptr(), Some(&mut w), 0), 0);
    assert_eq!(names(w.we_wordv, 2), some(&["unset", ""]), "the next call does not see it");
    s::wordfree(Some(&mut w));
}

/// `wordfree@@GLIBC_2.2.5`.
#[test]
fn sym_wordfree() {
    use pt::*;
    let mut w = wordexp_t { we_wordc: 0, we_wordv: 0, we_offs: 0 };
    s::wordfree(Some(&mut w));
    s::wordfree(None);
    assert_eq!(s::wordexp(z("x y").as_ptr(), Some(&mut w), 0), 0);
    s::wordfree(Some(&mut w));
    assert_eq!((w.we_wordv, w.we_wordc), (0, 2), "the vector is gone; the count stays");
    s::wordfree(Some(&mut w));
    // An uninitialised `we_offs` without WRDE_DOOFFS: the call sets it to 0, so wordfree frees
    // the vector it built.
    let mut w = wordexp_t { we_wordc: 0, we_wordv: 0, we_offs: 1000 };
    assert_eq!(s::wordexp(z("x y").as_ptr(), Some(&mut w), 0), 0);
    assert_eq!((w.we_wordc, w.we_offs), (2, 0));
    s::wordfree(Some(&mut w));
    assert_eq!(w.we_wordv, 0);
}
