// Specified from spec §4 and its Appendix B list. Generated stubs were replaced by real tests of
// the SAME NAME (see crates/spectest); `cargo xtask stubs` now only checks coverage.

//! Translation-layer tests for the exported symbols of spec §4 (174 entries).
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! Each test calls the export as a Rust function (D-X.5) with buffers owned by the test, and
//! reads results back through other exports (`strcpy`, `wcscpy`) where the result lives in
//! memory the export allocated. Fortify failures run in a forked child, which must end with
//! `SIGABRT` (spec §2.5 `__chk_fail`).
//!
//! The three `string2.h` copy helpers stay pending: their parameter lists are not recorded
//! (ERRATA E6).
#![allow(non_snake_case)]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use core::ptr::{null, null_mut};

use liella_abi::ctypes::{c_char, c_int, c_uint, c_void, size_t, wchar_t};
use liella_boundary::posix::s04_strings as s;
use liella_boundary::selftest::{self, opaque, ChildEnd};
use liella_boundary::sys::CSys;
use spectest::symbol;

const SIGABRT: i32 = 6;

fn cb(b: &[u8]) -> Vec<c_char> {
    b.iter().map(|&x| i8::from_le_bytes([x])).chain([0]).collect()
}

fn c(t: &str) -> Vec<c_char> {
    cb(t.as_bytes())
}

fn w(t: &str) -> Vec<wchar_t> {
    t.chars().map(|ch| ch as wchar_t).chain([0]).collect()
}

fn txt(v: &[c_char]) -> String {
    v.iter().take_while(|&&b| b != 0).map(|&b| char::from(b.to_le_bytes()[0])).collect()
}

fn wtxt(v: &[wchar_t]) -> String {
    v.iter().take_while(|&&u| u != 0).map(|&u| char::from_u32(u as u32).unwrap()).collect()
}

/// The text at `p` (memory the export allocated), copied out through `strcpy`.
fn read(p: *const c_char) -> String {
    let mut b = vec![0 as c_char; s::strlen(p) + 1];
    s::strcpy(b.as_mut_ptr(), p);
    txt(&b)
}

fn wread(p: *const wchar_t) -> String {
    let mut b = vec![0 as wchar_t; s::wcslen(p) + 1];
    s::wcscpy(b.as_mut_ptr(), p);
    wtxt(&b)
}

fn off<T>(r: *const T, base: *const T) -> usize {
    (r.addr() - base.addr()) / size_of::<T>()
}

fn aborts(body: fn(&CSys) -> i32) {
    assert_eq!(selftest::in_child_quiet(body).unwrap(), ChildEnd::Signaled(SIGABRT));
}

/// The entries of an argz vector, read through `argz_next`.
fn entries(z: *const c_char, n: size_t) -> Vec<String> {
    let mut v = Vec::new();
    let mut e = s::argz_next(z, n, null());
    while !e.is_null() {
        v.push(read(e));
        e = s::argz_next(z, n, e);
    }
    v
}

/// An argz vector built by `argz_create_sep`.
fn argz(t: &str, sep: u8) -> (*mut c_char, size_t) {
    let mut z = null_mut();
    let mut n = 0;
    assert_eq!(s::argz_create_sep(c(t).as_ptr(), c_int::from(sep), Some(&mut z), Some(&mut n)), 0);
    (z, n)
}

// ------------------------------------------------------------------------------ argz / envz

#[test]
fn sym_argz_add() {
    let (mut z, mut n) = argz("a", b':');
    assert_eq!(s::argz_add(Some(&mut z), Some(&mut n), c("bc").as_ptr()), 0);
    assert_eq!((entries(z, n), n), (vec!["a".into(), "bc".into()], 5));
}

#[test]
fn sym_argz_add_sep() {
    let (mut z, mut n) = argz("a", b':');
    assert_eq!(s::argz_add_sep(Some(&mut z), Some(&mut n), c("b::c").as_ptr(), c_int::from(b':')), 0);
    assert_eq!(entries(z, n), ["a", "b", "c"]);
}

#[test]
fn sym_argz_append() {
    let (mut z, mut n) = argz("a", b':');
    let more = cb(b"x\0y");
    assert_eq!(s::argz_append(Some(&mut z), Some(&mut n), more.as_ptr(), more.len()), 0);
    assert_eq!(entries(z, n), ["a", "x", "y"]);
}

#[test]
fn sym___argz_count() {
    let v = cb(b"a\0bc\0");
    assert_eq!(s::__argz_count(v.as_ptr(), v.len() - 1), 2);
    assert_eq!(s::__argz_count(null(), 0), 0);
}

#[test]
fn sym_argz_count() {
    let v = cb(b"a\0\0bc\0");
    assert_eq!(s::argz_count(v.as_ptr(), v.len() - 1), 3, "an empty entry counts");
}

#[test]
fn sym_argz_create() {
    let (a, b) = (c("a"), c("bc"));
    let argv = [a.as_ptr().cast_mut(), b.as_ptr().cast_mut(), null_mut()];
    let mut z = null_mut();
    let mut n = 99;
    assert_eq!(s::argz_create(argv.as_ptr(), Some(&mut z), Some(&mut n)), 0);
    assert_eq!((entries(z, n), n), (vec!["a".into(), "bc".into()], 5));
    let empty = [null_mut()];
    assert_eq!(s::argz_create(empty.as_ptr(), Some(&mut z), Some(&mut n)), 0);
    assert_eq!((z, n), (null_mut(), 0), "an empty vector is NULL, 0");
}

#[test]
fn sym_argz_create_sep() {
    let (z, n) = argz(":a::b:", b':');
    assert_eq!((entries(z, n), n), (vec!["a".into(), "b".into()], 4), "empty fields dropped");
}

#[test]
fn sym_argz_delete() {
    let (mut z, mut n) = argz("a:b:c", b':');
    let second = s::argz_next(z, n, z);
    s::argz_delete(Some(&mut z), Some(&mut n), second);
    assert_eq!(entries(z, n), ["a", "c"]);
    s::argz_delete(Some(&mut z), Some(&mut n), null_mut());
    assert_eq!(n, 4, "NULL entry: unchanged");
    let first = z;
    s::argz_delete(Some(&mut z), Some(&mut n), first);
    let first = z;
    s::argz_delete(Some(&mut z), Some(&mut n), first);
    assert_eq!((z, n), (null_mut(), 0), "deleting the last entry leaves NULL, 0");
}

#[test]
fn sym_argz_extract() {
    let (z, n) = argz("a:bc", b':');
    let mut argv = [null_mut::<c_char>(); 3];
    argv[2] = z;
    s::argz_extract(z, n, argv.as_mut_ptr());
    assert_eq!((read(argv[0]), read(argv[1]), argv[2]), ("a".into(), "bc".into(), null_mut()));
    assert_eq!(off(argv[1], z), 2, "pointers into the vector itself");
}

#[test]
fn sym_argz_insert() {
    let (mut z, mut n) = argz("a:c", b':');
    let c_entry = s::argz_next(z, n, z);
    assert_eq!(s::argz_insert(Some(&mut z), Some(&mut n), c_entry, c("b").as_ptr()), 0);
    assert_eq!(entries(z, n), ["a", "b", "c"]);
    assert_eq!(s::argz_insert(Some(&mut z), Some(&mut n), null_mut(), c("d").as_ptr()), 0, "NULL appends");
    assert_eq!(entries(z, n), ["a", "b", "c", "d"]);
    let (mut y, mut m) = argz("ab:c", b':');
    let mid = y.wrapping_add(1);
    assert_eq!(s::argz_insert(Some(&mut y), Some(&mut m), mid, c("x").as_ptr()), 0);
    assert_eq!(entries(y, m), ["x", "ab", "c"], "a position inside an entry moves to its start");
    let outside = c("zz");
    assert_eq!(s::argz_insert(Some(&mut y), Some(&mut m), outside.as_ptr().cast_mut(), c("q").as_ptr()), 22, "EINVAL");
}

#[test]
fn sym___argz_next() {
    let v = cb(b"a\0bc\0");
    let p = v.as_ptr();
    let first = s::__argz_next(p, 5, null());
    assert_eq!(off(first, p), 0);
    assert_eq!(off(s::__argz_next(p, 5, first), p), 2);
    assert!(s::__argz_next(p, 5, p.wrapping_add(2)).is_null());
    assert!(s::__argz_next(null(), 0, null()).is_null());
}

#[test]
fn sym_argz_next() {
    let (z, n) = argz("x:y", b':');
    assert_eq!(entries(z, n), ["x", "y"]);
}

#[test]
fn sym_argz_replace() {
    let (mut z, mut n) = argz("aXa:bb:X", b':');
    let mut count: c_uint = 1;
    assert_eq!(s::argz_replace(Some(&mut z), Some(&mut n), c("X").as_ptr(), c("yz").as_ptr(), Some(&mut count)), 0);
    assert_eq!((entries(z, n), count), (vec!["ayza".into(), "bb".into(), "yz".into()], 3), "count is added to");
    assert_eq!(s::argz_replace(Some(&mut z), Some(&mut n), c("b").as_ptr(), c("").as_ptr(), None), 0);
    assert_eq!(entries(z, n), ["ayza", "", "yz"]);
}

#[test]
fn sym___argz_stringify() {
    let mut v = cb(b"a\0b\0c\0");
    s::__argz_stringify(v.as_mut_ptr(), 6, c_int::from(b','));
    assert_eq!(txt(&v), "a,b,c");
}

#[test]
fn sym_argz_stringify() {
    let mut v = cb(b"ab\0c\0");
    s::argz_stringify(v.as_mut_ptr(), 5, c_int::from(b' '));
    assert_eq!(txt(&v), "ab c");
    s::argz_stringify(null_mut(), 0, 0);
}

fn envz(t: &[u8]) -> (*mut c_char, size_t) {
    let mut z = null_mut();
    let mut n = 0;
    assert_eq!(s::argz_append(Some(&mut z), Some(&mut n), cb(t).as_ptr(), t.len()), 0);
    (z, n)
}

#[test]
fn sym_envz_add() {
    let (mut z, mut n) = envz(b"A=1\0B=2\0");
    assert_eq!(s::envz_add(Some(&mut z), Some(&mut n), c("A").as_ptr(), c("9").as_ptr()), 0);
    assert_eq!(s::envz_add(Some(&mut z), Some(&mut n), c("N").as_ptr(), null()), 0);
    assert_eq!(entries(z, n), ["B=2", "A=9", "N"], "replaced entries move to the end; NULL value: no '='");
}

#[test]
fn sym_envz_entry() {
    let (z, n) = envz(b"A=1\0BB\0C=\0");
    assert_eq!(off(s::envz_entry(z, n, c("BB").as_ptr()), z), 4);
    assert_eq!(off(s::envz_entry(z, n, c("C").as_ptr()), z), 7);
    assert!(s::envz_entry(z, n, c("B").as_ptr()).is_null());
}

#[test]
fn sym_envz_get() {
    let (z, n) = envz(b"A=1\0BB\0C=\0");
    assert_eq!(read(s::envz_get(z, n, c("A").as_ptr())), "1");
    assert!(s::envz_get(z, n, c("BB").as_ptr()).is_null(), "a null value");
    assert_eq!(read(s::envz_get(z, n, c("C").as_ptr())), "", "an empty value is distinct from null");
    assert!(s::envz_get(z, n, c("D").as_ptr()).is_null());
}

#[test]
fn sym_envz_merge() {
    let (mut z, mut n) = envz(b"A=1\0B=2\0");
    let other = cb(b"A=7\0C=3\0");
    assert_eq!(s::envz_merge(Some(&mut z), Some(&mut n), other.as_ptr(), 8, 0), 0);
    assert_eq!(entries(z, n), ["A=1", "B=2", "C=3"]);
    assert_eq!(s::envz_merge(Some(&mut z), Some(&mut n), other.as_ptr(), 8, 1), 0);
    assert_eq!(read(s::envz_get(z, n, c("A").as_ptr())), "7", "override");
}

#[test]
fn sym_envz_remove() {
    let (mut z, mut n) = envz(b"A=1\0B=2\0");
    s::envz_remove(Some(&mut z), Some(&mut n), c("A").as_ptr());
    assert_eq!(entries(z, n), ["B=2"]);
    s::envz_remove(Some(&mut z), Some(&mut n), c("Q").as_ptr());
    assert_eq!(n, 4);
}

#[test]
fn sym_envz_strip() {
    let (mut z, mut n) = envz(b"A=1\0B\0C=\0");
    s::envz_strip(Some(&mut z), Some(&mut n));
    assert_eq!(entries(z, n), ["A=1", "C="]);
}

// ------------------------------------------------------------------------------ paths

#[test]
fn sym_basename() {
    for (p, o) in [("a/b", 2), ("a/", 2), ("abc", 0), ("/", 1), ("", 0)] {
        let v = c(p);
        assert_eq!(off(s::basename(v.as_ptr()), v.as_ptr()), o, "{p}");
        assert_eq!(txt(&v), p, "the GNU form never modifies");
    }
}

#[test]
fn sym___xpg_basename() {
    let mut v = c("a/b//");
    let r = s::__xpg_basename(v.as_mut_ptr());
    assert_eq!((off(r, v.as_ptr()), txt(&v)), (2, "a/b".into()), "trailing slashes stripped in place");
    let mut root = c("///");
    assert_eq!(read(s::__xpg_basename(root.as_mut_ptr())), "/");
    assert_eq!(read(s::__xpg_basename(null_mut())), ".");
    assert_eq!(read(s::__xpg_basename(c("").as_mut_ptr())), ".");
}

#[test]
fn sym_dirname() {
    for (p, d) in [("/usr/lib", "/usr"), ("/usr/", "/"), ("a", "."), ("", "."), ("/", "/"), ("a//b", "a"), ("a/..", "a")] {
        let mut v = c(p);
        assert_eq!(read(s::dirname(v.as_mut_ptr())), d, "{p}");
    }
    assert_eq!(read(s::dirname(null_mut())), ".");
}

// ------------------------------------------------------------------------------ memory blocks

#[test]
fn sym_bcmp() {
    let (a, b) = (c("abc"), c("abd"));
    assert_eq!(s::bcmp(a.as_ptr().cast(), b.as_ptr().cast(), 3), 1); // equality only: 0 or 1 (D-4.43)
    assert_eq!(s::bcmp(a.as_ptr().cast(), b.as_ptr().cast(), 2), 0);
}

#[test]
fn sym___memcmpeq() {
    let (a, b) = (c("abc"), c("abd"));
    assert_ne!(s::__memcmpeq(a.as_ptr().cast(), b.as_ptr().cast(), 3), 0);
    assert_eq!(s::__memcmpeq(a.as_ptr().cast(), b.as_ptr().cast(), 0), 0);
}

#[test]
fn sym_memcmp() {
    let a = [0x01u8, 0x80];
    let b = [0x01u8, 0x7f];
    assert!(s::memcmp(a.as_ptr().cast(), b.as_ptr().cast(), 2) > 0, "bytes compare as unsigned char");
    assert!(s::memcmp(b.as_ptr().cast(), a.as_ptr().cast(), 2) < 0);
    assert_eq!(s::memcmp(null(), null(), 0), 0);
}

#[test]
fn sym_bcopy() {
    let mut v = c("abcdef");
    let p = v.as_mut_ptr();
    s::bcopy(p.cast_const().cast(), p.wrapping_add(2).cast(), 4);
    assert_eq!(txt(&v), "ababcd", "overlap moves correctly");
}

#[test]
fn sym___bzero() {
    let mut v = c("abc");
    s::__bzero(v.as_mut_ptr().cast(), 2);
    assert_eq!(v, [0, 0, 99, 0]);
}

#[test]
fn sym_bzero() {
    let mut v = c("abc");
    s::bzero(v.as_mut_ptr().cast(), 3);
    assert_eq!(v, [0; 4]);
    s::bzero(null_mut(), 0);
}

#[test]
fn sym_explicit_bzero() {
    let mut v = c("secret");
    s::explicit_bzero(v.as_mut_ptr().cast(), 6);
    assert_eq!(v, [0; 7]);
}

#[test]
fn sym___explicit_bzero_chk() {
    let mut v = c("ab");
    s::__explicit_bzero_chk(v.as_mut_ptr().cast(), 2, 3);
    assert_eq!(v, [0; 3]);
    aborts(|_| {
        let mut b = [1u8; 4];
        s::__explicit_bzero_chk(b.as_mut_ptr().cast(), 5, 4);
        0
    });
}

#[test]
fn sym_memccpy() {
    let src = c("abc:def");
    let mut d = [0 as c_char; 8];
    let r = s::memccpy(d.as_mut_ptr().cast(), src.as_ptr().cast(), c_int::from(b':'), 7);
    assert_eq!((off(r.cast::<c_char>(), d.as_ptr()), txt(&d)), (4, "abc:".into()));
    let mut e = [0 as c_char; 8];
    assert!(s::memccpy(e.as_mut_ptr().cast(), src.as_ptr().cast(), c_int::from(b'!'), 3).is_null());
    assert_eq!(txt(&e), "abc", "not found: n bytes copied");
}

#[test]
fn sym_memchr() {
    let v = [1u8, 0xff, 3, 0xff];
    let p: *const c_void = v.as_ptr().cast();
    assert_eq!(off(s::memchr(p, -1, 4).cast::<u8>(), v.as_ptr()), 1, "(unsigned char)c");
    assert!(s::memchr(p, 3, 2).is_null(), "bounded by n");
}

#[test]
fn sym_memrchr() {
    let v = [1u8, 0xff, 3, 0xff];
    assert_eq!(off(s::memrchr(v.as_ptr().cast(), 0xff, 4).cast::<u8>(), v.as_ptr()), 3);
    assert!(s::memrchr(v.as_ptr().cast(), 9, 4).is_null());
}

#[test]
fn sym___rawmemchr() {
    let v = c("abc");
    assert_eq!(off(s::__rawmemchr(v.as_ptr().cast(), 0).cast::<c_char>(), v.as_ptr()), 3);
}

#[test]
fn sym_rawmemchr() {
    let v = c("abcb");
    assert_eq!(off(s::rawmemchr(v.as_ptr().cast(), c_int::from(b'b')).cast::<c_char>(), v.as_ptr()), 1);
}

// no spec clause: compat-plan.tsv behaviour row memcpy@GLIBC_2.2.5 (LG-2; overlap-safe, probe-settled).
#[test]
fn compat_memcpy_2_2_5_is_overlap_safe() {
    // Trace: no spec clause (compat-plan.tsv behaviour row memcpy@GLIBC_2.2.5; LG-2).
    for n in [1usize, 7, 16, 33, 100, 255, 1000] {
        for shift in [-17isize, -1, 1, 8, 64] {
            let mut a: Vec<u8> = (0..4096u32).map(|k| (k * 7 + 1) as u8).collect();
            let mut b = a.clone();
            let base = a.as_mut_ptr().wrapping_add(1024);
            let r = s::liella_compat_memcpy(base.wrapping_offset(shift).cast(), base.cast(), n);
            assert_eq!(r, base.wrapping_offset(shift).cast(), "returns the destination");
            b.copy_within(1024..1024 + n, (1024 + shift) as usize);
            assert!(a == b, "n={n} shift={shift}: memmove's result");
        }
    }
}

#[test]
fn sym_memcpy() {
    let src = c("hello");
    let mut d = [0 as c_char; 6];
    let r = s::memcpy(d.as_mut_ptr().cast(), src.as_ptr().cast(), 6);
    assert_eq!((r, txt(&d)), (d.as_mut_ptr().cast(), "hello".into()));
    let mut v = c("abcdef");
    let p = v.as_mut_ptr();
    opaque::memcpy(p.cast(), p.wrapping_add(1).cast(), 5);
    assert_eq!(txt(&v), "bcdeff", "overlap is moved correctly (D-4.2)");
}

#[test]
fn sym___memcpy_chk() {
    let src = c("abc");
    let mut d = [0 as c_char; 4];
    s::__memcpy_chk(d.as_mut_ptr().cast(), src.as_ptr().cast(), 4, 4);
    assert_eq!(txt(&d), "abc");
    s::__memcpy_chk(d.as_mut_ptr().cast(), src.as_ptr().cast(), 4, usize::MAX);
    aborts(|_| {
        let mut b = [0u8; 2];
        s::__memcpy_chk(b.as_mut_ptr().cast(), b"abc".as_ptr().cast(), 3, 2);
        0
    });
}

#[test]
fn sym_memfrob() {
    let mut v = c("ab");
    let p = v.as_mut_ptr().cast();
    assert_eq!(s::memfrob(p, 2), p);
    assert_eq!(v[..2], [b'a' as c_char ^ 42, b'b' as c_char ^ 42]);
    s::memfrob(p, 2);
    assert_eq!(txt(&v), "ab", "its own inverse");
}

#[test]
fn sym_memmem() {
    let h = cb(b"aab\0ab");
    let p = h.as_ptr().cast();
    assert_eq!(off(s::memmem(p, 6, b"ab".as_ptr().cast(), 2).cast::<c_char>(), h.as_ptr()), 1);
    assert_eq!(off(s::memmem(p, 6, b"\0a".as_ptr().cast(), 2).cast::<c_char>(), h.as_ptr()), 3, "binary data");
    assert_eq!(s::memmem(p, 6, null(), 0), p.cast_mut(), "empty needle matches at the start");
    assert!(s::memmem(p, 2, b"ab".as_ptr().cast(), 2).is_null());
}

#[test]
fn sym_memmove() {
    let mut v = c("abcdef");
    let p = v.as_mut_ptr();
    assert_eq!(s::memmove(p.wrapping_add(1).cast(), p.cast(), 4), p.wrapping_add(1).cast());
    assert_eq!(txt(&v), "aabcdf");
}

#[test]
fn sym___memmove_chk() {
    let mut v = c("abcd");
    let p = v.as_mut_ptr();
    s::__memmove_chk(p.cast(), p.wrapping_add(1).cast(), 3, 5);
    assert_eq!(txt(&v), "bcdd");
    aborts(|_| {
        let mut b = [0u8; 4];
        let p = b.as_mut_ptr();
        s::__memmove_chk(p.cast(), p.wrapping_add(1).cast(), 3, 2);
        0
    });
}

#[test]
fn sym___mempcpy() {
    let mut d = [0 as c_char; 4];
    let r = s::__mempcpy(d.as_mut_ptr().cast(), c("xyz").as_ptr().cast(), 3);
    assert_eq!((off(r.cast::<c_char>(), d.as_ptr()), txt(&d)), (3, "xyz".into()));
}

#[test]
fn sym_mempcpy() {
    let mut d = [0 as c_char; 4];
    let r = s::mempcpy(d.as_mut_ptr().cast(), c("xy").as_ptr().cast(), 2);
    assert_eq!(off(r.cast::<c_char>(), d.as_ptr()), 2, "returns d + n");
    assert_eq!(s::mempcpy(d.as_mut_ptr().cast(), null(), 0), d.as_mut_ptr().cast());
}

#[test]
fn sym___mempcpy_chk() {
    let mut d = [0 as c_char; 4];
    let r = s::__mempcpy_chk(d.as_mut_ptr().cast(), c("xy").as_ptr().cast(), 2, 4);
    assert_eq!(off(r.cast::<c_char>(), d.as_ptr()), 2);
    aborts(|_| {
        let mut b = [0u8; 1];
        s::__mempcpy_chk(b.as_mut_ptr().cast(), b"ab".as_ptr().cast(), 2, 1);
        0
    });
}

#[test]
fn fam_mempcpy_every_size_class() {
    // WL-HOT-SEQ: the mempcpy entries share memcpy's size-class kernels; each class boundary,
    // both alignments and all three entries return d + n and write exactly [d, d + n).
    type Copy = fn(*mut c_void, *const c_void, usize) -> *mut c_void;
    let entries: [(&str, Copy); 3] = [
        ("mempcpy", |d, s_, n| s::mempcpy(d, s_, n)),
        ("__mempcpy", |d, s_, n| s::__mempcpy(d, s_, n)),
        ("__mempcpy_chk", |d, s_, n| s::__mempcpy_chk(d, s_, n, n + 3)),
    ];
    let src: Vec<u8> = (0..1100u32).map(|i| (i * 7 + 1) as u8).collect();
    for (name, f) in entries {
        for n in (0..=130).chain([255, 256, 257, 1024]) {
            for (sa, da) in [(0, 0), (1, 3), (5, 0)] {
                let mut d = vec![0xEEu8; 1100 + 16];
                let r = f(d[da..].as_mut_ptr().cast(), src[sa..].as_ptr().cast(), n);
                assert_eq!(r as usize, d[da..].as_ptr() as usize + n, "{name} {n}: returns d + n");
                assert_eq!(&d[da..da + n], &src[sa..sa + n], "{name} {n} +{sa}/+{da}: bytes");
                assert!(d[..da].iter().chain(&d[da + n..]).all(|&b| b == 0xEE), "{name} {n}: outside written");
            }
        }
    }
}

symbol!(sym___mempcpy_small,"__mempcpy_small", "4", "2.2.5", r##"pending: parameter list unknown (ERRATA E6)"##);

#[test]
fn sym_memset() {
    let mut v = [0u8; 4];
    let p = v.as_mut_ptr().cast();
    assert_eq!(s::memset(p, 0x1ff, 3), p);
    assert_eq!(v, [0xff, 0xff, 0xff, 0], "(unsigned char)c, exactly n bytes");
}

#[test]
fn sym___memset_chk() {
    let mut v = [0u8; 4];
    s::__memset_chk(v.as_mut_ptr().cast(), 7, 4, 4);
    assert_eq!(v, [7; 4]);
    aborts(|_| {
        let mut b = [0u8; 4];
        s::__memset_chk(b.as_mut_ptr().cast(), 7, 5, 4);
        0
    });
}

#[test]
fn sym_swab() {
    let src = [1u8, 2, 3, 4, 5];
    let mut d = [0u8; 5];
    s::swab(src.as_ptr().cast(), d.as_mut_ptr().cast(), 5);
    assert_eq!(d, [2, 1, 4, 3, 0], "an odd final byte is dropped");
    s::swab(src.as_ptr().cast(), d.as_mut_ptr().cast(), -1);
    assert_eq!(d, [2, 1, 4, 3, 0], "a negative count does nothing");
}

// ------------------------------------------------------------------------------ search

#[test]
fn sym_index() {
    let v = c("abca");
    assert_eq!(off(s::index(v.as_ptr(), c_int::from(b'a')), v.as_ptr()), 0);
}

#[test]
fn sym_rindex() {
    let v = c("abca");
    assert_eq!(off(s::rindex(v.as_ptr(), c_int::from(b'a')), v.as_ptr()), 3);
}

#[test]
fn sym_strchr() {
    let v = c("ab\u{7f}");
    assert_eq!(off(s::strchr(v.as_ptr(), c_int::from(b'b')), v.as_ptr()), 1);
    assert_eq!(off(s::strchr(v.as_ptr(), 0), v.as_ptr()), 3, "the terminator is searchable");
    assert!(s::strchr(v.as_ptr(), c_int::from(b'z')).is_null());
    let hi = cb(&[b'a', 0xe9]);
    assert_eq!(off(s::strchr(hi.as_ptr(), 0xe9), hi.as_ptr()), 1, "c converted to char");
}

#[test]
fn sym_strchrnul() {
    let v = c("abc");
    assert_eq!(off(s::strchrnul(v.as_ptr(), c_int::from(b'z')), v.as_ptr()), 3);
    assert_eq!(off(s::strchrnul(v.as_ptr(), c_int::from(b'b')), v.as_ptr()), 1);
}

#[test]
fn sym_strrchr() {
    let v = c("a/b/c");
    assert_eq!(off(s::strrchr(v.as_ptr(), c_int::from(b'/')), v.as_ptr()), 3);
    assert_eq!(off(s::strrchr(v.as_ptr(), 0), v.as_ptr()), 5);
    assert!(s::strrchr(v.as_ptr(), c_int::from(b'x')).is_null());
}

#[test]
fn sym_strstr() {
    let h = c("abcabd");
    assert_eq!(off(s::strstr(h.as_ptr(), c("abd").as_ptr()), h.as_ptr()), 3);
    assert_eq!(s::strstr(h.as_ptr(), c("").as_ptr()), h.as_ptr().cast_mut());
    assert!(s::strstr(h.as_ptr(), c("abe").as_ptr()).is_null());
}

#[test]
fn sym___strcasestr() {
    let h = c("xHeLLo");
    assert_eq!(off(s::__strcasestr(h.as_ptr(), c("hello").as_ptr()), h.as_ptr()), 1);
}

#[test]
fn sym_strcasestr() {
    let h = c("ABC");
    assert_eq!(off(s::strcasestr(h.as_ptr(), c("bc").as_ptr()), h.as_ptr()), 1);
    assert!(s::strcasestr(h.as_ptr(), c("cd").as_ptr()).is_null());
}

#[test]
fn sym_strspn() {
    assert_eq!(s::strspn(c("aabxa").as_ptr(), c("ba").as_ptr()), 3);
    assert_eq!(s::strspn(c("abc").as_ptr(), c("").as_ptr()), 0);
}

#[test]
fn sym_strcspn() {
    assert_eq!(s::strcspn(c("abc:d").as_ptr(), c(":;").as_ptr()), 3);
    assert_eq!(s::strcspn(c("abc").as_ptr(), c("").as_ptr()), 3, "the terminator is never a member");
}

#[test]
fn sym_strpbrk() {
    let v = c("ab;c:");
    assert_eq!(off(s::strpbrk(v.as_ptr(), c(":;").as_ptr()), v.as_ptr()), 2);
    assert!(s::strpbrk(v.as_ptr(), c("x").as_ptr()).is_null());
}

#[test]
fn sym___strspn_c1() {
    assert_eq!(s::__strspn_c1(c("aab").as_ptr(), c_int::from(b'a')), 2);
}

#[test]
fn sym___strspn_c2() {
    assert_eq!(s::__strspn_c2(c("abac").as_ptr(), c_int::from(b'a'), c_int::from(b'b')), 3);
}

#[test]
fn sym___strspn_c3() {
    assert_eq!(s::__strspn_c3(c("abcd").as_ptr(), c_int::from(b'a'), c_int::from(b'b'), c_int::from(b'c')), 3);
}

#[test]
fn sym___strcspn_c1() {
    assert_eq!(s::__strcspn_c1(c("ab:c").as_ptr(), c_int::from(b':')), 2);
    assert_eq!(s::__strcspn_c1(c("ab").as_ptr(), 0), 2);
}

#[test]
fn sym___strcspn_c2() {
    assert_eq!(s::__strcspn_c2(c("ab;c").as_ptr(), c_int::from(b':'), c_int::from(b';')), 2);
}

#[test]
fn sym___strcspn_c3() {
    assert_eq!(s::__strcspn_c3(c("ab,c").as_ptr(), c_int::from(b':'), c_int::from(b';'), c_int::from(b',')), 2);
}

#[test]
fn sym___strpbrk_c2() {
    let v = c("ab;c");
    assert_eq!(off(s::__strpbrk_c2(v.as_ptr(), c_int::from(b':'), c_int::from(b';')), v.as_ptr()), 2);
    assert!(s::__strpbrk_c2(v.as_ptr(), c_int::from(b'x'), c_int::from(b'y')).is_null());
}

#[test]
fn sym___strpbrk_c3() {
    let v = c("ab,c");
    assert_eq!(off(s::__strpbrk_c3(v.as_ptr(), c_int::from(b':'), c_int::from(b';'), c_int::from(b',')), v.as_ptr()), 2);
}

// ------------------------------------------------------------------------------ compare

#[test]
fn sym_strcmp() {
    assert_eq!(s::strcmp(c("abc").as_ptr(), c("abc").as_ptr()), 0);
    assert!(s::strcmp(c("ab").as_ptr(), c("abc").as_ptr()) < 0);
    assert!(s::strcmp(cb(&[0x80]).as_ptr(), c("a").as_ptr()) > 0, "unsigned char order");
}

/// `strcmp` and `strncmp` agree with byte order (unsigned) for every length around the C layer's
/// vector widths, a difference at every position, and every bound.
#[test]
fn strcmp_strncmp_every_length_and_position() {
    // Clauses: s4_3_c_1 (D-4.14, P7).
    for la in 0..40usize {
        for lb in [0usize, 1, 7, 8, 9, 15, 16, 17, 31, 32, 33, 39] {
            let base: Vec<u8> = (0..40u8).map(|i| b'a' + i % 26).collect();
            let a = &base[..la];
            for pos in 0..=lb {
                let mut b = base[..lb].to_vec();
                if pos < lb {
                    b[pos] = if pos % 2 == 0 { 0xE9 } else { b'A' };
                }
                let want = a.cmp(&b[..]);
                let (pa, pb) = (cb(a), cb(&b));
                assert_eq!(s::strcmp(pa.as_ptr(), pb.as_ptr()).cmp(&0), want, "strcmp {la} {lb} {pos}");
                for n in [0usize, 1, 7, 8, 9, 16, 17, 40] {
                    let want_n = a[..la.min(n)].cmp(&b[..lb.min(n)]);
                    assert_eq!(s::strncmp(pa.as_ptr(), pb.as_ptr(), n).cmp(&0), want_n, "strncmp {la} {lb} {pos} {n}");
                }
            }
        }
    }
}

#[test]
fn sym_strncmp() {
    assert_eq!(s::strncmp(c("abcX").as_ptr(), c("abcY").as_ptr(), 3), 0);
    assert!(s::strncmp(c("abcX").as_ptr(), c("abcY").as_ptr(), 4) < 0);
    assert_eq!(s::strncmp(c("a").as_ptr(), c("b").as_ptr(), 0), 0);
    // A length of 0 reads nothing: NULL is accepted (the reference; validation T18).
    assert_eq!(s::strncmp(null(), null(), 0), 0);
}

#[test]
fn sym___strcasecmp() {
    assert_eq!(s::__strcasecmp(c("HeLLo").as_ptr(), c("hello").as_ptr()), 0);
}

#[test]
fn sym_strcasecmp() {
    assert_eq!(s::strcasecmp(c("ABC").as_ptr(), c("abc").as_ptr()), 0);
    assert!(s::strcasecmp(c("abc").as_ptr(), c("ABD").as_ptr()) < 0);
    assert!(s::strcasecmp(c("[").as_ptr(), c("a").as_ptr()) < 0, "folds to lower case before comparing");
}

#[test]
fn sym___strcasecmp_l() {
    assert_eq!(s::__strcasecmp_l(c("X").as_ptr(), c("x").as_ptr(), null_mut()), 0);
}

#[test]
fn sym_strcasecmp_l() {
    assert!(s::strcasecmp_l(c("a").as_ptr(), c("B").as_ptr(), null_mut()) < 0);
}

#[test]
fn sym_strncasecmp() {
    assert_eq!(s::strncasecmp(c("ABx").as_ptr(), c("aby").as_ptr(), 2), 0);
    assert!(s::strncasecmp(c("ABx").as_ptr(), c("aby").as_ptr(), 3) < 0);
    assert_eq!(s::strncasecmp(null(), null(), 0), 0, "a length of 0 reads nothing (T18)");
}

#[test]
fn sym___strncasecmp_l() {
    assert_eq!(s::__strncasecmp_l(c("ABx").as_ptr(), c("aby").as_ptr(), 2, null_mut()), 0);
}

#[test]
fn sym_strncasecmp_l() {
    assert!(s::strncasecmp_l(c("b").as_ptr(), c("A").as_ptr(), 1, null_mut()) > 0);
}

#[test]
fn sym_strcoll() {
    assert!(s::strcoll(c("a").as_ptr(), c("b").as_ptr()) < 0, "the C locale collates as strcmp");
    assert!(s::strcoll(c("B").as_ptr(), c("a").as_ptr()) < 0);
}

#[test]
fn sym___strcoll_l() {
    assert_eq!(s::__strcoll_l(c("x").as_ptr(), c("x").as_ptr(), null_mut()), 0);
}

#[test]
fn sym_strcoll_l() {
    assert!(s::strcoll_l(c("b").as_ptr(), c("a").as_ptr(), null_mut()) > 0);
}

#[test]
fn sym___strverscmp() {
    assert!(s::__strverscmp(c("a2").as_ptr(), c("a10").as_ptr()) < 0);
}

#[test]
fn sym_strverscmp() {
    assert!(s::strverscmp(c("file9").as_ptr(), c("file10").as_ptr()) < 0);
    assert!(s::strverscmp(c("1.01").as_ptr(), c("1.1").as_ptr()) < 0, "leading zeros are fractional");
    assert_eq!(s::strverscmp(c("x").as_ptr(), c("x").as_ptr()), 0);
    // The GNU C Library manual's and strverscmp(3)'s order, and the text after a fractional run
    // that is a prefix of the other (glibc 2.39 probe, validation T18).
    let chain = ["000", "00", "01", "010", "01a", "09", "0", "1", "9", "10"];
    for (i, x) in chain.iter().enumerate() {
        for (j, y) in chain.iter().enumerate() {
            let o = s::strverscmp(c(x).as_ptr(), c(y).as_ptr());
            assert_eq!(o.cmp(&0), i.cmp(&j), "strverscmp({x}, {y})");
        }
    }
    assert!(s::strverscmp(c("1.01").as_ptr(), c("1.010").as_ptr()) < 0);
}

#[test]
fn sym_strxfrm() {
    let mut d = [9 as c_char; 4];
    assert_eq!(s::strxfrm(d.as_mut_ptr(), c("abc").as_ptr(), 4), 3);
    assert_eq!(txt(&d), "abc");
    let mut e = [9 as c_char; 2];
    assert_eq!(s::strxfrm(e.as_mut_ptr(), c("abc").as_ptr(), 2), 3, "result >= n: incomplete");
    assert_eq!(s::strxfrm(null_mut(), c("abcd").as_ptr(), 0), 4, "measuring call");
}

#[test]
fn sym___strxfrm_l() {
    assert_eq!(s::__strxfrm_l(null_mut(), c("ab").as_ptr(), 0, null_mut()), 2);
}

#[test]
fn sym_strxfrm_l() {
    let mut d = [0 as c_char; 3];
    assert_eq!(s::strxfrm_l(d.as_mut_ptr(), c("ab").as_ptr(), 3, null_mut()), 2);
    assert_eq!(txt(&d), "ab");
}

// ------------------------------------------------------------------------------ length

#[test]
fn sym_strlen() {
    assert_eq!(s::strlen(c("").as_ptr()), 0);
    assert_eq!(s::strlen(c("hello").as_ptr()), 5);
}

#[test]
fn sym_strnlen() {
    assert_eq!(s::strnlen(c("hello").as_ptr(), 3), 3);
    assert_eq!(s::strnlen(c("hi").as_ptr(), 10), 2);
    let unterminated = [b'a' as c_char; 4];
    assert_eq!(s::strnlen(unterminated.as_ptr(), 4), 4, "reads no further than n bytes");
    assert_eq!(s::strnlen(null(), 0), 0, "a length of 0 reads nothing (validation T18)");
}

// ------------------------------------------------------------------------------ copy

#[test]
fn sym_strcpy() {
    let mut d = [9 as c_char; 6];
    assert_eq!(s::strcpy(d.as_mut_ptr(), c("abc").as_ptr()), d.as_mut_ptr());
    assert_eq!(d, [97, 98, 99, 0, 9, 9], "the terminator is copied, nothing beyond");
}

#[test]
fn sym___strcpy_chk() {
    let mut d = [0 as c_char; 4];
    s::__strcpy_chk(d.as_mut_ptr(), c("abc").as_ptr(), 4);
    assert_eq!(txt(&d), "abc");
    aborts(|_| {
        let mut b = [0 as c_char; 4];
        s::__strcpy_chk(b.as_mut_ptr(), c"abcd".as_ptr(), 4);
        0
    });
}

#[test]
fn fam_strncpy_short_kernel_sweep() {
    // STRNCPY-SHORT: n 0..=72 (every store class of the one-pass entry, its 64-byte limit and the
    // general path) against text lengths 0..=70 (n < len, n == len, n > len with padding) at
    // source offsets around a page end and destination offsets 0..=3: exactly n bytes are
    // written (the text up to n, then NULs), nothing outside, and d is returned.
    let mut src = vec![0u8; 3 * 4096];
    let base = (4096 - (src.as_ptr() as usize) % 4096) % 4096 + 4096;
    for len in (0..=70usize).step_by(3).chain([1, 7, 16, 25, 32, 63, 64]) {
        for off in (4096 - 80..=4096 - 1).step_by(9).chain([0, 4032, 4033, 4096 - 64 - 1]) {
            let at = base - 4096 + off;
            src[at..at + len].iter_mut().enumerate().for_each(|(i, b)| *b = b'a' + (i % 26) as u8);
            src[at + len] = 0;
            let sp = src[at..].as_ptr().cast::<c_char>();
            for n in 0..=72usize {
                for (doff, f) in (0..4usize).flat_map(|o| (0..5).map(move |f| (o, f))) {
                    // STRNCPY-FAMILY: f 0 strncpy, 1 stpncpy (returns the end), 2 __strncpy_chk
                    // with destlen == n (the largest size the check accepts); STPNCPY-ALIAS:
                    // 3 __stpncpy, 4 __stpncpy_chk with destlen == n (both return the end).
                    let mut d = vec![0x5au8; n + 8];
                    let dp = d[doff..].as_mut_ptr().cast::<c_char>();
                    let k = len.min(n);
                    let (r, want) = match f {
                        0 => (s::strncpy(dp, sp, n), dp),
                        1 => (s::stpncpy(dp, sp, n), dp.wrapping_add(k)),
                        2 => (s::__strncpy_chk(dp, sp, n, n), dp),
                        3 => (s::__stpncpy(dp, sp, n), dp.wrapping_add(k)),
                        _ => (s::__stpncpy_chk(dp, sp, n, n), dp.wrapping_add(k)),
                    };
                    assert_eq!(r, want, "f {f} len {len} off {off} n {n} doff {doff}: return value");
                    assert_eq!(&d[doff..doff + k], &src[at..at + k], "f {f} len {len} off {off} n {n}: text");
                    assert!(d[doff + k..doff + n].iter().all(|&b| b == 0), "f {f} len {len} off {off} n {n} doff {doff}: padding");
                    assert!(d[..doff].iter().chain(&d[doff + n..]).all(|&b| b == 0x5a), "f {f} len {len} off {off} n {n} doff {doff}: wrote outside");
                }
            }
        }
    }
}

#[test]
fn fam_strncpy_chk_rejects_n_over_destlen() {
    // STRNCPY-FAMILY: the fortify check comes first, for every n of the one-pass range too.
    fn over(n: usize) -> i32 {
        let mut b = [0 as c_char; 80];
        s::__strncpy_chk(b.as_mut_ptr(), c"ab".as_ptr(), n, n - 1);
        0
    }
    aborts(|_| over(1));
    aborts(|_| over(5));
    aborts(|_| over(25));
    aborts(|_| over(64));
    aborts(|_| over(65));
}

#[test]
fn fam_stpncpy_chk_rejects_n_over_destlen() {
    // STPNCPY-ALIAS: __stpncpy_chk's fortify check comes first, for every n of the one-pass range too.
    fn over(n: usize) -> i32 {
        let mut b = [0 as c_char; 80];
        s::__stpncpy_chk(b.as_mut_ptr(), c"ab".as_ptr(), n, n - 1);
        0
    }
    aborts(|_| over(1));
    aborts(|_| over(5));
    aborts(|_| over(25));
    aborts(|_| over(64));
    aborts(|_| over(65));
}

#[test]
fn fam_strcpy_short_kernel_sweep() {
    // STRCPY-SHORT: every length 0..=80 (each copy class of the one-pass kernel, its 64-byte limit
    // and the general path) at source offsets around a page end (the kernel's page rule) and
    // destination offsets 0..=3: strcpy/stpcpy/__strcpy_chk/__stpcpy_chk (exact room) copy the
    // text and its terminator, return the right pointer and write nothing past the terminator.
    let mut src = vec![0u8; 3 * 4096];
    let base = (4096 - (src.as_ptr() as usize) % 4096) % 4096 + 4096;
    for len in 0..=80usize {
        for off in (4096 - 96..=4096 - 1).step_by(5).chain([4032, 4033, 4031, 4096 - 64 - 1]) {
            let at = base - 4096 + off;
            src[at..at + len].iter_mut().enumerate().for_each(|(i, b)| *b = b'a' + (i % 26) as u8);
            src[at + len] = 0;
            let sp = src[at..].as_ptr().cast::<c_char>();
            for doff in 0..4usize {
                for f in 0..4 {
                    let mut d = vec![0x5au8; len + 8];
                    let dp = d[doff..].as_mut_ptr().cast::<c_char>();
                    let r = match f {
                        0 => s::strcpy(dp, sp),
                        1 => s::stpcpy(dp, sp),
                        2 => s::__strcpy_chk(dp, sp, len + 1),
                        _ => s::__stpcpy_chk(dp, sp, len + 1),
                    };
                    let want = if f % 2 == 0 { dp } else { dp.wrapping_add(len) };
                    assert_eq!(r, want, "f {f} len {len} off {off} doff {doff}: return value");
                    assert_eq!(&d[doff..doff + len], &src[at..at + len], "f {f} len {len} off {off}: text");
                    assert_eq!(d[doff + len], 0, "f {f} len {len} off {off}: terminator");
                    assert!(d[..doff].iter().chain(&d[doff + len + 1..]).all(|&b| b == 0x5a), "f {f} len {len} off {off} doff {doff}: wrote outside");
                }
            }
        }
    }
}

#[test]
fn fam_chk_copies_sweep_class_boundaries() {
    // MEMPCPY plants: every length around the copy classes, exact fit passes, one byte short aborts,
    // nothing is written past the text and its terminator.
    // The aborting calls run in a forked child through a plain `fn`, so the length travels in a
    // static the child inherits and the child rebuilds the source itself.
    // The children never touch the heap (selftest::in_child): another test thread may hold the
    // allocator's lock at the fork, and a child that then allocates blocks forever while the parent
    // waits for it (MEMCMP-EQ-24-32-2: this test hung a gate for 45 minutes). `source` is therefore
    // a stack array, 300 non-zero bytes and a terminator; a child cuts the text with `text[len] = 0`.
    static LEN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    fn source() -> [u8; 301] {
        let mut s = [0u8; 301];
        for (i, b) in s[..300].iter_mut().enumerate() {
            *b = u8::try_from(i % 251).unwrap() + 1;
        }
        s
    }
    let src = source();
    for len in (0..=140usize).chain([255, 256, 257, 299]) {
        LEN.store(len, std::sync::atomic::Ordering::Relaxed);
        let mut text = src[..len].to_vec();
        text.push(0);
        let mut d = vec![0xEEu8; 320];
        let r = s::__strcpy_chk(d.as_mut_ptr().cast(), text.as_ptr().cast(), len + 1);
        assert_eq!(r.cast::<u8>(), d.as_mut_ptr(), "strcpy_chk len={len}");
        assert_eq!(&d[..=len], &text[..], "strcpy_chk len={len}");
        assert!(d[len + 1..].iter().all(|&b| b == 0xEE), "strcpy_chk len={len} overrun");
        let mut e = vec![0xEEu8; 320];
        let r = s::__memcpy_chk(e.as_mut_ptr().cast(), src.as_ptr().cast(), len, len);
        assert_eq!(r.cast::<u8>(), e.as_mut_ptr(), "memcpy_chk len={len}");
        assert_eq!(&e[..len], &src[..len]);
        assert!(e[len..].iter().all(|&b| b == 0xEE), "memcpy_chk len={len} overrun");
        if len > 0 {
            aborts(|_| {
                let len = LEN.load(std::sync::atomic::Ordering::Relaxed);
                let src = source();
                let mut b = [0u8; 1];
                s::__memcpy_chk(b.as_mut_ptr().cast(), src.as_ptr().cast(), len, len - 1);
                0
            });
        }
        aborts(|_| {
            let len = LEN.load(std::sync::atomic::Ordering::Relaxed);
            let mut text = source();
            text[len] = 0;
            let mut b = [0u8; 1];
            s::__strcpy_chk(b.as_mut_ptr().cast(), text.as_ptr().cast(), len);
            0
        });
        // STRCPY-FAMILY: the direct __stpcpy_chk entry checks the room before any store too.
        aborts(|_| {
            let len = LEN.load(std::sync::atomic::Ordering::Relaxed);
            let mut text = source();
            text[len] = 0;
            let mut b = [0u8; 1];
            s::__stpcpy_chk(b.as_mut_ptr().cast(), text.as_ptr().cast(), len);
            0
        });
    }
}

symbol!(sym___strcpy_small,"__strcpy_small", "4", "2.2.5", r##"pending: parameter list unknown (ERRATA E6)"##);

#[test]
fn sym___stpcpy() {
    let mut d = [0 as c_char; 4];
    assert_eq!(off(s::__stpcpy(d.as_mut_ptr(), c("ab").as_ptr()), d.as_ptr()), 2);
}

#[test]
fn sym_stpcpy() {
    let mut d = [0 as c_char; 8];
    let e = s::stpcpy(d.as_mut_ptr(), c("ab").as_ptr());
    s::stpcpy(e, c("cd").as_ptr());
    assert_eq!(txt(&d), "abcd", "returns the copied terminator, for appending");
}

#[test]
fn sym___stpcpy_chk() {
    let mut d = [0 as c_char; 3];
    assert_eq!(off(s::__stpcpy_chk(d.as_mut_ptr(), c("ab").as_ptr(), 3), d.as_ptr()), 2);
    aborts(|_| {
        let mut b = [0 as c_char; 2];
        s::__stpcpy_chk(b.as_mut_ptr(), c"ab".as_ptr(), 2);
        0
    });
}

symbol!(sym___stpcpy_small, "__stpcpy_small", "4", "2.2.5", r##"pending: parameter list unknown (ERRATA E6)"##);

#[test]
fn sym_strncpy() {
    let mut d = [9 as c_char; 5];
    s::strncpy(d.as_mut_ptr(), c("ab").as_ptr(), 4);
    assert_eq!(d, [97, 98, 0, 0, 9], "zero-padded to exactly n");
    let mut e = [9 as c_char; 3];
    s::strncpy(e.as_mut_ptr(), c("abcdef").as_ptr(), 2);
    assert_eq!(e, [97, 98, 9], "not terminated when s is n bytes or longer");
}

#[test]
fn sym___strncpy_chk() {
    let mut d = [9 as c_char; 4];
    s::__strncpy_chk(d.as_mut_ptr(), c("a").as_ptr(), 4, 4);
    assert_eq!(d, [97, 0, 0, 0]);
    aborts(|_| {
        let mut b = [0 as c_char; 4];
        s::__strncpy_chk(b.as_mut_ptr(), c"a".as_ptr(), 5, 4);
        0
    });
}

#[test]
fn sym___stpncpy() {
    let mut d = [0 as c_char; 4];
    assert_eq!(off(s::__stpncpy(d.as_mut_ptr(), c("ab").as_ptr(), 4), d.as_ptr()), 2);
}

#[test]
fn sym_stpncpy() {
    let mut d = [0 as c_char; 4];
    assert_eq!(off(s::stpncpy(d.as_mut_ptr(), c("abcdef").as_ptr(), 3), d.as_ptr()), 3, "min(len, n)");
    assert_eq!(off(s::stpncpy(d.as_mut_ptr(), null(), 0), d.as_ptr()), 0, "a length of 0 reads nothing (T18)");
}

#[test]
fn sym___stpncpy_chk() {
    let mut d = [0 as c_char; 4];
    assert_eq!(off(s::__stpncpy_chk(d.as_mut_ptr(), c("a").as_ptr(), 4, 4), d.as_ptr()), 1);
    aborts(|_| {
        let mut b = [0 as c_char; 2];
        s::__stpncpy_chk(b.as_mut_ptr(), c"a".as_ptr(), 3, 2);
        0
    });
}

#[test]
fn sym_strlcpy() {
    let mut d = [9 as c_char; 4];
    assert_eq!(s::strlcpy(d.as_mut_ptr(), c("abcdef").as_ptr(), 4), 6, "returns strlen(s)");
    assert_eq!(txt(&d), "abc", "truncated and terminated");
    assert_eq!(s::strlcpy(null_mut(), c("xy").as_ptr(), 0), 2, "size 0 writes nothing");
}

#[test]
fn sym___strlcpy_chk() {
    let mut d = [0 as c_char; 4];
    assert_eq!(s::__strlcpy_chk(d.as_mut_ptr(), c("ab").as_ptr(), 4, 4), 2);
    aborts(|_| {
        let mut b = [0 as c_char; 4];
        s::__strlcpy_chk(b.as_mut_ptr(), c"a".as_ptr(), 8, 4);
        0
    });
}

#[test]
fn sym_strdup() {
    let p = s::strdup(c("dup me").as_ptr());
    assert_eq!(read(p), "dup me");
}

#[test]
fn sym_strdup_every_short_length() {
    // STRDUP-FAST: copies of 0 to 40 units take the inline short path (up to 16 bytes) or the
    // kernel; each must hold every unit and the terminator, however the string ends.
    for len in 0..=40usize {
        let text: String = (0..len).map(|i| char::from(b'a' + (i % 26) as u8)).collect();
        let src = c(&text);
        assert_eq!(read(s::strdup(src.as_ptr())), text, "strdup {len}");
        assert_eq!(read(s::strndup(src.as_ptr(), len + 5)), text, "strndup room {len}");
        let cut: String = text.chars().take(len / 2).collect();
        assert_eq!(read(s::strndup(src.as_ptr(), len / 2)), cut, "strndup cut {len}");
    }
    for len in 0..=8usize {
        let text: String = (0..len).map(|i| char::from(b'a' + i as u8)).collect();
        assert_eq!(wread(s::wcsdup(w(&text).as_ptr())), text, "wcsdup {len}");
    }
}

#[test]
fn sym___strdup() {
    assert_eq!(read(s::__strdup(c("").as_ptr())), "");
}

#[test]
fn sym_strndup() {
    assert_eq!(read(s::strndup(c("abcdef").as_ptr(), 3)), "abc", "always terminated");
    assert_eq!(read(s::strndup(c("ab").as_ptr(), 9)), "ab");
    // No byte is read for a length of 0, so any pointer gives an empty copy (the reference's
    // answer, which gnulib's configure test requires; validation T11).
    assert_eq!(read(s::strndup(core::ptr::null(), 0)), "");
}

#[test]
fn sym___strndup() {
    assert_eq!(read(s::__strndup(c("xyz").as_ptr(), 1)), "x");
    assert_eq!(read(s::__strndup(core::ptr::null(), 0)), "");
}

#[test]
fn sym_strfry() {
    let mut v = c("abcdefghij");
    assert_eq!(s::strfry(v.as_mut_ptr()), v.as_mut_ptr());
    let mut got: Vec<u8> = txt(&v).into_bytes();
    got.sort_unstable();
    assert_eq!(got, b"abcdefghij", "a permutation of the bytes");
}

// ------------------------------------------------------------------------------ concatenate

#[test]
fn sym_strcat() {
    let mut d = [0 as c_char; 8];
    s::strcpy(d.as_mut_ptr(), c("ab").as_ptr());
    assert_eq!(s::strcat(d.as_mut_ptr(), c("cd").as_ptr()), d.as_mut_ptr());
    assert_eq!(txt(&d), "abcd");
}

#[test]
fn sym___strcat_chk() {
    let mut d = [0 as c_char; 5];
    s::strcpy(d.as_mut_ptr(), c("ab").as_ptr());
    s::__strcat_chk(d.as_mut_ptr(), c("cd").as_ptr(), 5);
    assert_eq!(txt(&d), "abcd");
    aborts(|_| {
        let mut b = [0 as c_char; 4];
        s::strcpy(b.as_mut_ptr(), c"ab".as_ptr());
        s::__strcat_chk(b.as_mut_ptr(), c"cd".as_ptr(), 4);
        0
    });
}

#[test]
fn sym_strncat() {
    let mut d = [9 as c_char; 8];
    s::strcpy(d.as_mut_ptr(), c("ab").as_ptr());
    s::strncat(d.as_mut_ptr(), c("cdef").as_ptr(), 2);
    assert_eq!(d[..5], [97, 98, 99, 100, 0], "n bounds the source; a terminator follows");
}

#[test]
fn sym___strncat_chk() {
    let mut d = [0 as c_char; 4];
    s::__strncat_chk(d.as_mut_ptr(), c("xyz").as_ptr(), 3, 4);
    assert_eq!(txt(&d), "xyz");
    aborts(|_| {
        let mut b = [0 as c_char; 4];
        s::__strncat_chk(b.as_mut_ptr(), c"wxyz".as_ptr(), 4, 4);
        0
    });
}

#[test]
fn sym_strlcat() {
    let mut d = [0 as c_char; 6];
    s::strcpy(d.as_mut_ptr(), c("ab").as_ptr());
    assert_eq!(s::strlcat(d.as_mut_ptr(), c("cdefg").as_ptr(), 6), 7);
    assert_eq!(txt(&d), "abcde", "total length kept at size - 1");
    let mut u = [b'x' as c_char; 3];
    assert_eq!(s::strlcat(u.as_mut_ptr(), c("ab").as_ptr(), 3), 5, "unterminated within size: size + strlen(s)");
    assert_eq!(u, [b'x' as c_char; 3]);
}

#[test]
fn sym___strlcat_chk() {
    let mut d = [0 as c_char; 4];
    assert_eq!(s::__strlcat_chk(d.as_mut_ptr(), c("ab").as_ptr(), 4, 4), 2);
    aborts(|_| {
        let mut b = [0 as c_char; 4];
        s::__strlcat_chk(b.as_mut_ptr(), c"a".as_ptr(), 5, 4);
        0
    });
}

// ------------------------------------------------------------------------------ tokenize

#[test]
fn sym_strtok() {
    let mut v = c("::a:bc::d:");
    let p = v.as_mut_ptr();
    let delim = c(":");
    let mut got = Vec::new();
    let mut t = s::strtok(p, delim.as_ptr());
    while !t.is_null() {
        got.push(read(t));
        t = s::strtok(null_mut(), delim.as_ptr());
    }
    assert_eq!(got, ["a", "bc", "d"], "empty tokens are never returned");
    assert!(s::strtok(null_mut(), delim.as_ptr()).is_null(), "stays exhausted");
}

fn tokens_r(f: extern "C" fn(*mut c_char, *const c_char, Option<&mut *mut c_char>) -> *mut c_char, input: &str, delim: &str) -> Vec<String> {
    let mut v = c(input);
    let d = c(delim);
    let mut save = null_mut();
    let mut got = Vec::new();
    let mut t = f(v.as_mut_ptr(), d.as_ptr(), Some(&mut save));
    while !t.is_null() {
        got.push(read(t));
        t = f(null_mut(), d.as_ptr(), Some(&mut save));
    }
    got
}

#[test]
fn sym_strtok_r() {
    assert_eq!(tokens_r(s::strtok_r, " a  b ", " "), ["a", "b"]);
    assert!(tokens_r(s::strtok_r, ";;", ";").is_empty());
}

#[test]
fn sym___strtok_r() {
    assert_eq!(tokens_r(s::__strtok_r, "a,b;c", ",;"), ["a", "b", "c"]);
}

#[test]
fn sym___strtok_r_1c() {
    let mut v = c("--x-y");
    let mut save = null_mut();
    let sep = b'-' as c_char;
    assert_eq!(read(s::__strtok_r_1c(v.as_mut_ptr(), sep, Some(&mut save))), "x");
    assert_eq!(read(s::__strtok_r_1c(null_mut(), sep, Some(&mut save))), "y");
    assert!(s::__strtok_r_1c(null_mut(), sep, Some(&mut save)).is_null());
}

fn sep_all(f: impl Fn(Option<&mut *mut c_char>) -> *mut c_char, input: &str) -> Vec<String> {
    let mut v = c(input);
    let mut cur = v.as_mut_ptr();
    let mut got = Vec::new();
    loop {
        let t = f(Some(&mut cur));
        if t.is_null() {
            return got;
        }
        got.push(read(t));
    }
}

#[test]
fn sym_strsep() {
    let d = c(":;");
    assert_eq!(sep_all(|p| s::strsep(p, d.as_ptr()), "a::b;"), ["a", "", "b", ""], "empty tokens are returned");
    let mut nothing: *mut c_char = null_mut();
    assert!(s::strsep(Some(&mut nothing), d.as_ptr()).is_null());
}

#[test]
fn sym___strsep_g() {
    let d = c(",");
    assert_eq!(sep_all(|p| s::__strsep_g(p, d.as_ptr()), "x,y"), ["x", "y"]);
}

#[test]
fn sym___strsep_1c() {
    assert_eq!(sep_all(|p| s::__strsep_1c(p, b',' as c_char), "a,,b"), ["a", "", "b"]);
}

#[test]
fn sym___strsep_2c() {
    assert_eq!(sep_all(|p| s::__strsep_2c(p, b',' as c_char, b';' as c_char), "a;b,c"), ["a", "b", "c"]);
}

#[test]
fn sym___strsep_3c() {
    assert_eq!(sep_all(|p| s::__strsep_3c(p, b',' as c_char, b';' as c_char, b' ' as c_char), "a b;c,d"), ["a", "b", "c", "d"]);
}

#[test]
fn sym_wcstok() {
    let mut v = w("..a.bc.");
    let d = w(".");
    let mut save = null_mut();
    let mut got = Vec::new();
    let mut t = s::wcstok(v.as_mut_ptr(), d.as_ptr(), Some(&mut save));
    while !t.is_null() {
        got.push(wread(t));
        t = s::wcstok(null_mut(), d.as_ptr(), Some(&mut save));
    }
    assert_eq!(got, ["a", "bc"]);
}

// ------------------------------------------------------------------------------ wide

#[test]
fn sym_wcslen() {
    assert_eq!(s::wcslen(w("héllo").as_ptr()), 5);
}

#[test]
fn sym_wcsnlen() {
    assert_eq!(s::wcsnlen(w("abc").as_ptr(), 2), 2);
    assert_eq!(s::wcsnlen(w("abc").as_ptr(), 9), 3);
    assert_eq!(s::wcsnlen(null(), 0), 0, "a length of 0 reads nothing (validation T18)");
}

#[test]
fn sym_wcscmp() {
    assert!(s::wcscmp(w("a").as_ptr(), w("b").as_ptr()) < 0);
    assert_eq!(s::wcscmp(w("é").as_ptr(), w("é").as_ptr()), 0);
    assert!(s::wcscmp(w("ab").as_ptr(), w("a").as_ptr()) > 0);
}

#[test]
fn sym_wcsncmp() {
    assert_eq!(s::wcsncmp(w("abX").as_ptr(), w("abY").as_ptr(), 2), 0);
    assert!(s::wcsncmp(w("abX").as_ptr(), w("abY").as_ptr(), 3) < 0);
}

#[test]
fn sym_wcscasecmp() {
    assert_eq!(s::wcscasecmp(w("AbC").as_ptr(), w("aBc").as_ptr()), 0);
    assert!(s::wcscasecmp(w("a").as_ptr(), w("B").as_ptr()) < 0);
}

#[test]
fn sym___wcscasecmp_l() {
    assert_eq!(s::__wcscasecmp_l(w("Q").as_ptr(), w("q").as_ptr(), null_mut()), 0);
}

#[test]
fn sym_wcscasecmp_l() {
    assert!(s::wcscasecmp_l(w("z").as_ptr(), w("A").as_ptr(), null_mut()) > 0);
}

#[test]
fn sym_wcsncasecmp() {
    assert_eq!(s::wcsncasecmp(w("ABx").as_ptr(), w("aby").as_ptr(), 2), 0);
}

#[test]
fn sym___wcsncasecmp_l() {
    assert!(s::__wcsncasecmp_l(w("ABx").as_ptr(), w("aby").as_ptr(), 3, null_mut()) < 0);
}

#[test]
fn sym_wcsncasecmp_l() {
    assert_eq!(s::wcsncasecmp_l(w("x").as_ptr(), w("X").as_ptr(), 1, null_mut()), 0);
}

#[test]
fn sym_wcscoll() {
    assert!(s::wcscoll(w("a").as_ptr(), w("b").as_ptr()) < 0);
}

#[test]
fn sym___wcscoll_l() {
    assert_eq!(s::__wcscoll_l(w("x").as_ptr(), w("x").as_ptr(), null_mut()), 0);
}

#[test]
fn sym_wcscoll_l() {
    assert!(s::wcscoll_l(w("b").as_ptr(), w("a").as_ptr(), null_mut()) > 0);
}

#[test]
fn sym_wcsxfrm() {
    let mut d = [9 as wchar_t; 3];
    assert_eq!(s::wcsxfrm(d.as_mut_ptr(), w("ab").as_ptr(), 3), 2);
    assert_eq!(wtxt(&d), "ab");
    assert_eq!(s::wcsxfrm(null_mut(), w("abc").as_ptr(), 0), 3);
}

#[test]
fn sym___wcsxfrm_l() {
    assert_eq!(s::__wcsxfrm_l(null_mut(), w("a").as_ptr(), 0, null_mut()), 1);
}

#[test]
fn sym_wcsxfrm_l() {
    let mut d = [0 as wchar_t; 2];
    assert_eq!(s::wcsxfrm_l(d.as_mut_ptr(), w("a").as_ptr(), 2, null_mut()), 1);
}

#[test]
fn sym_wcschr() {
    let v = w("aéb");
    assert_eq!(off(s::wcschr(v.as_ptr(), 'é' as wchar_t), v.as_ptr()), 1);
    assert_eq!(off(s::wcschr(v.as_ptr(), 0), v.as_ptr()), 3);
    assert!(s::wcschr(v.as_ptr(), 'z' as wchar_t).is_null());
}

#[test]
fn sym_wcschrnul() {
    let v = w("ab");
    assert_eq!(off(s::wcschrnul(v.as_ptr(), 'z' as wchar_t), v.as_ptr()), 2);
}

#[test]
fn sym_wcsrchr() {
    let v = w("abab");
    assert_eq!(off(s::wcsrchr(v.as_ptr(), 'a' as wchar_t), v.as_ptr()), 2);
}

#[test]
fn sym_wcsstr() {
    let h = w("xxyxyz");
    assert_eq!(off(s::wcsstr(h.as_ptr(), w("xyz").as_ptr()), h.as_ptr()), 3);
    assert_eq!(s::wcsstr(h.as_ptr(), w("").as_ptr()), h.as_ptr().cast_mut());
}

#[test]
fn sym_wcswcs() {
    let h = w("abc");
    assert_eq!(off(s::wcswcs(h.as_ptr(), w("bc").as_ptr()), h.as_ptr()), 1);
}

#[test]
fn sym_wcsspn() {
    assert_eq!(s::wcsspn(w("aabx").as_ptr(), w("ab").as_ptr()), 3);
}

#[test]
fn sym_wcscspn() {
    assert_eq!(s::wcscspn(w("abc:").as_ptr(), w(":").as_ptr()), 3);
    assert_eq!(s::wcscspn(w("abc").as_ptr(), w("").as_ptr()), 3);
}

#[test]
fn sym_wcspbrk() {
    let v = w("ab:c");
    assert_eq!(off(s::wcspbrk(v.as_ptr(), w(":;").as_ptr()), v.as_ptr()), 2);
    assert!(s::wcspbrk(v.as_ptr(), w("x").as_ptr()).is_null());
}

#[test]
fn sym_wcscpy() {
    let mut d = [9 as wchar_t; 4];
    assert_eq!(s::wcscpy(d.as_mut_ptr(), w("ab").as_ptr()), d.as_mut_ptr());
    assert_eq!(d, [97, 98, 0, 9]);
}

#[test]
fn sym___wcscpy_chk() {
    let mut d = [0 as wchar_t; 3];
    s::__wcscpy_chk(d.as_mut_ptr(), w("ab").as_ptr(), 3);
    assert_eq!(wtxt(&d), "ab");
    aborts(|_| {
        let mut b = [0 as wchar_t; 3];
        let src = [97, 98, 99, 0];
        s::__wcscpy_chk(b.as_mut_ptr(), src.as_ptr(), 3);
        0
    });
}

#[test]
fn sym_wcpcpy() {
    let mut d = [0 as wchar_t; 4];
    assert_eq!(off(s::wcpcpy(d.as_mut_ptr(), w("ab").as_ptr()), d.as_ptr()), 2);
}

#[test]
fn sym___wcpcpy_chk() {
    let mut d = [0 as wchar_t; 3];
    assert_eq!(off(s::__wcpcpy_chk(d.as_mut_ptr(), w("ab").as_ptr(), 3), d.as_ptr()), 2);
    aborts(|_| {
        let mut b = [0 as wchar_t; 2];
        let src = [97, 98, 0];
        s::__wcpcpy_chk(b.as_mut_ptr(), src.as_ptr(), 2);
        0
    });
}

#[test]
fn sym_wcsncpy() {
    let mut d = [9 as wchar_t; 4];
    s::wcsncpy(d.as_mut_ptr(), w("a").as_ptr(), 3);
    assert_eq!(d, [97, 0, 0, 9]);
    assert_eq!(s::wcsncpy(d.as_mut_ptr(), null(), 0), d.as_mut_ptr(), "a length of 0 reads nothing (T18)");
}

#[test]
fn sym___wcsncpy_chk() {
    let mut d = [9 as wchar_t; 3];
    s::__wcsncpy_chk(d.as_mut_ptr(), w("a").as_ptr(), 3, 3);
    assert_eq!(d, [97, 0, 0]);
    aborts(|_| {
        let mut b = [0 as wchar_t; 3];
        let src = [97, 0];
        s::__wcsncpy_chk(b.as_mut_ptr(), src.as_ptr(), 4, 3);
        0
    });
}

#[test]
fn sym_wcpncpy() {
    let mut d = [0 as wchar_t; 4];
    assert_eq!(off(s::wcpncpy(d.as_mut_ptr(), w("abc").as_ptr(), 2), d.as_ptr()), 2);
}

#[test]
fn sym___wcpncpy_chk() {
    let mut d = [0 as wchar_t; 4];
    assert_eq!(off(s::__wcpncpy_chk(d.as_mut_ptr(), w("a").as_ptr(), 4, 4), d.as_ptr()), 1);
    aborts(|_| {
        let mut b = [0 as wchar_t; 2];
        let src = [97, 0];
        s::__wcpncpy_chk(b.as_mut_ptr(), src.as_ptr(), 3, 2);
        0
    });
}

#[test]
fn sym_wcscat() {
    let mut d = [0 as wchar_t; 5];
    s::wcscpy(d.as_mut_ptr(), w("ab").as_ptr());
    s::wcscat(d.as_mut_ptr(), w("cd").as_ptr());
    assert_eq!(wtxt(&d), "abcd");
}

#[test]
fn sym___wcscat_chk() {
    let mut d = [0 as wchar_t; 5];
    s::wcscpy(d.as_mut_ptr(), w("ab").as_ptr());
    s::__wcscat_chk(d.as_mut_ptr(), w("cd").as_ptr(), 5);
    assert_eq!(wtxt(&d), "abcd");
    aborts(|_| {
        let mut b = [97 as wchar_t, 0, 0];
        let src = [98, 99, 0];
        s::__wcscat_chk(b.as_mut_ptr(), src.as_ptr(), 3);
        0
    });
}

#[test]
fn sym_wcsncat() {
    let mut d = [0 as wchar_t; 5];
    s::wcscpy(d.as_mut_ptr(), w("a").as_ptr());
    s::wcsncat(d.as_mut_ptr(), w("bcd").as_ptr(), 2);
    assert_eq!(wtxt(&d), "abc");
}

#[test]
fn sym___wcsncat_chk() {
    let mut d = [0 as wchar_t; 3];
    s::__wcsncat_chk(d.as_mut_ptr(), w("xyz").as_ptr(), 2, 3);
    assert_eq!(wtxt(&d), "xy");
    aborts(|_| {
        let mut b = [0 as wchar_t; 2];
        let src = [97, 98, 0];
        s::__wcsncat_chk(b.as_mut_ptr(), src.as_ptr(), 2, 2);
        0
    });
}

#[test]
fn sym_wcslcpy() {
    let mut d = [9 as wchar_t; 3];
    assert_eq!(s::wcslcpy(d.as_mut_ptr(), w("abcd").as_ptr(), 3), 4);
    assert_eq!(wtxt(&d), "ab");
}

#[test]
fn sym___wcslcpy_chk() {
    let mut d = [0 as wchar_t; 3];
    assert_eq!(s::__wcslcpy_chk(d.as_mut_ptr(), w("ab").as_ptr(), 3, 3), 2);
    aborts(|_| {
        let mut b = [0 as wchar_t; 2];
        let src = [97, 0];
        s::__wcslcpy_chk(b.as_mut_ptr(), src.as_ptr(), 3, 2);
        0
    });
}

#[test]
fn sym_wcslcat() {
    let mut d = [0 as wchar_t; 4];
    s::wcscpy(d.as_mut_ptr(), w("a").as_ptr());
    assert_eq!(s::wcslcat(d.as_mut_ptr(), w("bcd").as_ptr(), 4), 4);
    assert_eq!(wtxt(&d), "abc");
}

#[test]
fn sym___wcslcat_chk() {
    let mut d = [0 as wchar_t; 4];
    assert_eq!(s::__wcslcat_chk(d.as_mut_ptr(), w("ab").as_ptr(), 4, 4), 2);
    aborts(|_| {
        let mut b = [0 as wchar_t; 2];
        let src = [97, 0];
        s::__wcslcat_chk(b.as_mut_ptr(), src.as_ptr(), 3, 2);
        0
    });
}

#[test]
fn sym_wcsdup() {
    assert_eq!(wread(s::wcsdup(w("wïde").as_ptr())), "wïde");
}

#[test]
fn sym_wmemchr() {
    let v = [1, -5, 3];
    assert_eq!(off(s::wmemchr(v.as_ptr(), -5, 3), v.as_ptr()), 1);
    assert!(s::wmemchr(v.as_ptr(), 3, 2).is_null());
}

#[test]
fn sym_wmemcmp() {
    let (a, b) = ([1, 2], [1, 3]);
    assert!(s::wmemcmp(a.as_ptr(), b.as_ptr(), 2) < 0);
    assert_eq!(s::wmemcmp(a.as_ptr(), b.as_ptr(), 1), 0);
}

#[test]
fn sym_wmemcpy() {
    let mut d = [0 as wchar_t; 3];
    assert_eq!(s::wmemcpy(d.as_mut_ptr(), [7, 8, 9].as_ptr(), 3), d.as_mut_ptr());
    assert_eq!(d, [7, 8, 9]);
}

#[test]
fn sym___wmemcpy_chk() {
    let mut d = [0 as wchar_t; 2];
    s::__wmemcpy_chk(d.as_mut_ptr(), [1, 2].as_ptr(), 2, 2);
    assert_eq!(d, [1, 2]);
    aborts(|_| {
        let mut b = [0 as wchar_t; 2];
        s::__wmemcpy_chk(b.as_mut_ptr(), [1, 2, 3].as_ptr(), 3, 2);
        0
    });
}

#[test]
fn sym_wmemmove() {
    let mut v = [1, 2, 3, 4];
    let p = v.as_mut_ptr();
    s::wmemmove(p.wrapping_add(1), p, 3);
    assert_eq!(v, [1, 1, 2, 3]);
}

#[test]
fn sym___wmemmove_chk() {
    let mut v = [1, 2, 3];
    let p = v.as_mut_ptr();
    s::__wmemmove_chk(p, p.wrapping_add(1), 2, 3);
    assert_eq!(v, [2, 3, 3]);
    aborts(|_| {
        let mut b = [1 as wchar_t, 2, 3];
        let p = b.as_mut_ptr();
        s::__wmemmove_chk(p, p.wrapping_add(1), 2, 1);
        0
    });
}

#[test]
fn sym_wmempcpy() {
    let mut d = [0 as wchar_t; 3];
    assert_eq!(off(s::wmempcpy(d.as_mut_ptr(), [5, 6].as_ptr(), 2), d.as_ptr()), 2);
}

#[test]
fn sym___wmempcpy_chk() {
    let mut d = [0 as wchar_t; 3];
    assert_eq!(off(s::__wmempcpy_chk(d.as_mut_ptr(), [5, 6].as_ptr(), 2, 3), d.as_ptr()), 2);
    aborts(|_| {
        let mut b = [0 as wchar_t; 1];
        s::__wmempcpy_chk(b.as_mut_ptr(), [5, 6].as_ptr(), 2, 1);
        0
    });
}

#[test]
fn sym_wmemset() {
    let mut d = [0 as wchar_t; 3];
    assert_eq!(s::wmemset(d.as_mut_ptr(), -1, 2), d.as_mut_ptr());
    assert_eq!(d, [-1, -1, 0]);
}

#[test]
fn sym___wmemset_chk() {
    let mut d = [0 as wchar_t; 2];
    s::__wmemset_chk(d.as_mut_ptr(), 4, 2, 2);
    assert_eq!(d, [4, 4]);
    aborts(|_| {
        let mut b = [0 as wchar_t; 2];
        s::__wmemset_chk(b.as_mut_ptr(), 4, 3, 2);
        0
    });
}

// opaque-ok: audited in GATE-SPEED-4/5: the direct calls here use their results and pass at opt-level 1 and 2 in both pass types; any case LLVM folds goes through selftest::opaque.
