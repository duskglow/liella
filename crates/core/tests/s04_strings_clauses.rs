// Specified from glibc-functional-spec.md §4. Generated stubs were replaced by real tests of the
// SAME NAME (see crates/spectest); `cargo xtask stubs` now only checks coverage.

//! Clause tests for spec §4 (66 clauses).
//!
//! The clauses are tested on the functions that implement them: `liella::bytes` (the
//! length-delimited needs), `liella_legacy::cstring` and `string_compat` (the C-shaped
//! compositions). Clauses about the C interface itself (fortify aborts, `strdup`'s `ENOMEM`,
//! the tolerant `memcpy`) run through the exports of `liella-boundary`.
//!
//! Two clauses stay pending: §4 Notes 5 and 6 are design commentary with no observable
//! behaviour.

#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use core::cmp::Ordering;
use core::ptr::null_mut;

use liella::bytes::{self, ByteSet, Move, Tokens};
use liella::ctype;
use liella_abi::ctypes::c_char;
use liella_boundary::posix::s04_strings as x;
use liella_boundary::selftest::{self, ChildEnd};
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Rlimit, RLIMIT_AS, RLIM_INFINITY};
use liella_kernel::Errno;
use liella_legacy::cstring;
use liella_legacy::string_compat::{self, Cursor, TextAt};
use spectest::commentary;

const SIGABRT: i32 = 6;

/// Text with its terminator, as `str*` functions see C memory.
fn z(s: &str) -> Vec<u8> {
    s.bytes().chain([0]).collect()
}

/// The text of a terminated buffer.
fn text(b: &[u8]) -> &[u8] {
    &b[..b.iter().position(|&c| c == 0).unwrap_or(b.len())]
}

/// Memory for the address-based tokenizers: address `i + 1` is byte `i` of the buffer, 0 is
/// NULL.
struct Mem<'a>(&'a mut [u8]);

impl TextAt<u8> for &mut Mem<'_> {
    fn text_at(&mut self, addr: usize) -> Option<&mut [u8]> {
        let rest = self.0.get_mut(addr.checked_sub(1)?..)?;
        let n = rest.iter().position(|&c| c == 0)?;
        rest.get_mut(..n)
    }
}

struct Cell(usize);

impl Cursor for &mut Cell {
    fn get(&self) -> usize {
        self.0
    }
    fn set(&mut self, addr: usize) {
        self.0 = addr;
    }
}

fn tokens(s: &str, delims: &[u8]) -> Vec<String> {
    let mut buf = z(s);
    let mut mem = Mem(&mut buf);
    let mut cur = Cell(0);
    let mut out = Vec::new();
    let mut t = string_compat::strtok(1, ByteSet::new(delims).predicate(), &mut cur, &mut mem);
    while t != 0 {
        out.push(String::from_utf8(text(&mem.0[t - 1..]).to_vec()).unwrap());
        t = string_compat::strtok(0, ByteSet::new(delims).predicate(), &mut cur, &mut mem);
    }
    out
}

// ------------------------------------------------------------------------------ notes

#[test]
fn s4_n_1() {
    // Pure computations: every need is a function of its arguments alone, with no `Sys`.
    assert_eq!(bytes::find_seq(b"needle in hay", b"hay", core::convert::identity), Some(10));
    assert_eq!(bytes::compare_text(b"a", b"b"), Ordering::Less);
    assert_eq!(cstring::argz_count(b"a\0b\0"), 2);
}

#[test]
fn s4_n_2() {
    // MT-Safe: the same computation on many threads at once yields the same results.
    let results: Vec<usize> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..8).map(|_| sc.spawn(|| (0..1000).map(|i| bytes::span(format!("{i}aaa").as_bytes(), |b| b.is_ascii_digit())).sum::<usize>())).collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    assert!(results.windows(2).all(|w| w[0] == w[1]));
}

#[test]
fn s4_n_3() {
    // Overlap is undefined in C; here the move of an overlapping region is still exact
    // (D-4.2), the safe choice for undefined behaviour.
    let mut buf = *b"abcdef";
    assert_eq!(bytes::apply_move(Move::Within(&mut buf, 0, 2)), 4);
    assert_eq!(&buf, b"ababcd");
}

#[test]
fn s4_n_4() {
    // One need (compare) under every delimiting convention gives one answer.
    let (a, b) = (z("abcX"), z("abcY"));
    let mem = bytes::compare(&a[..3], &b[..3]);
    let strn = bytes::compare_text(&a[..3], &b[..3]);
    let str_ = bytes::compare_text(text(&a), text(&b));
    assert_eq!((mem, strn, str_), (Ordering::Equal, Ordering::Equal, Ordering::Less));
}

#[test]
fn s4_n_4_1() {
    // Byte count: data with embedded zeros is ordinary data.
    assert_eq!(bytes::find(b"a\0b", b'b'), Some(2));
}

#[test]
fn s4_n_4_2() {
    // NUL terminator: the text ends at the first zero.
    assert_eq!(text(b"ab\0cd\0"), b"ab");
}

#[test]
fn s4_n_4_3() {
    // Terminator or count, whichever comes first.
    let s = z("abcdef");
    assert_eq!(text(&s[..3]).len(), 3);
    assert_eq!(text(&s[..10.min(s.len())]).len(), 6);
}

#[test]
fn s4_n_4_4() {
    // A destination capacity: the copy is truncated to it and terminated.
    let mut d = [9u8; 4];
    assert_eq!(cstring::lcpy(b"abcdef", 4, &mut d[..]).unwrap(), 6);
    assert_eq!(&d, b"abc\0");
}

commentary!(s4_n_5, "4", "Notes", r##"The functional needs are fewer than 20: copy, move, fill, compare, search for a byte, search for a subsequence, search for any of a set, measure, concatenate, tokenize, duplicate. Each need is multiplied by three to six delimiting conventions, and again by the wide-character (`wcs*`/`wmem*`) versions. ⚑ COMMENTARY: implies that one need gives one answer under every delimiting convention; covered by `s4_n_4`."##);
commentary!(s4_n_6, "4", "Notes", r##"A design built on length-delimited **Text** needs one form per need. It needs NUL-terminated forms only at the kernel boundary (Path, argv). ⚑ COMMENTARY: nothing testable (an internal design rule, not observable through the C interface)."##);

// ------------------------------------------------------------------------------ 4.1 copy

#[test]
fn s4_1_c_0() {
    let mut d = [0u8; 3];
    assert_eq!(bytes::copy(&mut d, b"xyz"), 3);
    assert_eq!(&d, b"xyz");
    let mut w = [0i32; 2];
    bytes::copy(&mut w, &[0x1F600, -1]);
    assert_eq!(w, [0x1F600, -1], "wide characters copy as units");
}

#[test]
fn s4_1_c_1() {
    let mut d = [0u8; 4];
    assert_eq!(bytes::copy(&mut d, b"abcd"), 4);
    assert_eq!(&d, b"abcd");
    // COMPAT: both memcpy versions tolerate overlap here (D-4.2).
    let mut v = z("abcdef");
    let p = v.as_mut_ptr();
    x::memcpy(p.wrapping_add(1).cast(), p.cast(), 5);
    assert_eq!(text(&v), b"aabcde");
}

#[test]
fn s4_1_c_2() {
    let mut buf = *b"abcdef";
    bytes::apply_move(Move::Within(&mut buf, 2, 0));
    assert_eq!(&buf, b"cdefef");
}

#[test]
fn s4_1_c_3() {
    let mut d = [0 as c_char; 4];
    let r = x::mempcpy(d.as_mut_ptr().cast(), b"ab".as_ptr().cast(), 2);
    assert_eq!(r.cast::<c_char>(), d.as_mut_ptr().wrapping_add(2));
}

#[test]
fn s4_1_c_4() {
    let mut d = [0u8; 8];
    assert_eq!(bytes::copy_until(&mut d, b"ab:cd", b':'), Some(3), "past the copied c");
    assert_eq!(&d[..4], b"ab:\0");
    let mut e = [0u8; 2];
    assert_eq!(bytes::copy_until(&mut e, b"ab", b':'), None);
    assert_eq!(&e, b"ab");
}

#[test]
fn s4_1_c_5() {
    let mut v = z("abcd");
    let p = v.as_mut_ptr();
    x::bcopy(p.cast_const().cast(), p.wrapping_add(1).cast(), 3);
    assert_eq!(text(&v), b"aabc", "source first, destination second, overlap allowed");
}

#[test]
fn s4_1_c_6() {
    let mut d = [9u8; 5];
    assert_eq!(cstring::copy_z(b"abc", &mut d[..]).unwrap(), 3, "the stpcpy offset is the terminator");
    assert_eq!(&d, b"abc\0\x09");
}

#[test]
fn s4_1_c_7() {
    let mut d = [9u8; 5];
    assert_eq!(cstring::copy_n_padded(b"ab", 5, &mut d[..]).unwrap(), 2);
    assert_eq!(&d, b"ab\0\0\0");
    let mut e = [9u8; 3];
    cstring::copy_n_padded(b"abcdef", 3, &mut e[..]).unwrap();
    assert_eq!(&e, b"abc", "no terminator when s is n bytes or longer");
}

#[test]
fn s4_1_i_0() {
    let mut d: [u8; 0] = [];
    assert_eq!(cstring::copy_n_padded(b"abc", 0, &mut d[..]).unwrap(), 0, "n may be 0");
    assert_eq!(bytes::copy(&mut d, b""), 0);
}

#[test]
fn s4_1_g_0() {
    let mut d = [7u8; 8];
    cstring::copy_z(b"ab", &mut d[..]).unwrap();
    assert_eq!(&d[3..], &[7; 5], "nothing beyond the terminator is written");
    let mut e = [7u8; 8];
    cstring::copy_n_padded(b"ab", 4, &mut e[..]).unwrap();
    assert_eq!(&e[4..], &[7; 4], "padding stops at n");
}

#[test]
fn s4_1_f_0() {
    assert_eq!(cstring::copy_z(b"abcd", &mut [0u8; 4][..]), Err(Errno::ERANGE), "no room: refused, nothing written past the end");
    let end = selftest::in_child_quiet(|_| {
        let mut d = [0 as c_char; 2];
        x::__strcpy_chk(d.as_mut_ptr(), c"abc".as_ptr(), 2);
        0
    });
    assert_eq!(end.unwrap(), ChildEnd::Signaled(SIGABRT), "fortify: a controlled abort");
}

// ------------------------------------------------------------------------------ 4.2 fill

#[test]
fn s4_2_c_1() {
    let mut v = [0u8; 4];
    let p = v.as_mut_ptr().cast();
    x::memset(p, 0x141, 3);
    assert_eq!(v, [0x41, 0x41, 0x41, 0], "(unsigned char)c");
}

#[test]
fn s4_2_c_2() {
    let (mut a, mut b) = ([5u8; 4], [5u8; 4]);
    x::bzero(a.as_mut_ptr().cast(), 3);
    x::memset(b.as_mut_ptr().cast(), 0, 3);
    assert_eq!(a, b);
}

#[test]
fn s4_2_c_3() {
    let mut secret = *b"hunter2";
    bytes::wipe(&mut secret, |w| assert_eq!(w, [0; 7], "the barrier sees the zeroed bytes"));
    assert_eq!(secret, [0; 7]);
}

// ------------------------------------------------------------------------------ 4.3 compare

#[test]
fn s4_3_c_0() {
    let mut words: Vec<&[u8]> = vec![b"b", b"ab", b"a", b"", b"ba"];
    words.sort_by(|p, q| bytes::compare_text(p, q));
    assert_eq!(words, [&b""[..], b"a", b"ab", b"b", b"ba"]);
}

#[test]
fn s4_3_c_1() {
    assert_eq!(bytes::compare(&[0x80u8], &[0x7f]), Ordering::Greater, "unsigned char");
    assert_eq!(bytes::compare_text(b"ab", b"abc"), Ordering::Less, "a prefix is smaller");
    assert_eq!(bytes::compare_text(&b"abcX"[..3], &b"abcY"[..3]), Ordering::Equal, "strncmp bound");
}

#[test]
fn s4_3_c_2() {
    assert_ne!(x::bcmp(b"ab".as_ptr().cast(), b"ac".as_ptr().cast(), 2), 0);
    assert_eq!(x::bcmp(b"ab".as_ptr().cast(), b"ab".as_ptr().cast(), 2), 0);
}

#[test]
fn s4_3_c_3() {
    assert_ne!(x::__memcmpeq(b"ab".as_ptr().cast(), b"aB".as_ptr().cast(), 2), 0);
    assert_eq!(x::__memcmpeq(b"ab".as_ptr().cast(), b"ab".as_ptr().cast(), 2), 0);
}

#[test]
fn s4_3_c_4() {
    assert_eq!(bytes::compare_text_by(b"HeLLo", b"hello", ctype::lower_c), Ordering::Equal);
    assert_eq!(bytes::compare_text_by(b"_", b"a", ctype::lower_c), Ordering::Less, "'_' < 'a' after folding, though > 'A'");
    assert_eq!(bytes::compare_text_by(&[0xC9], &[0xE9], ctype::lower_c), Ordering::Less, "the C locale folds ASCII only (D-6.7)");
}

#[test]
fn s4_3_g_0() {
    let words: [&[u8]; 4] = [b"a", b"B", b"b", b"ab"];
    for p in words {
        for q in words {
            assert_eq!(bytes::compare_text(p, q), bytes::compare_text(q, p).reverse(), "antisymmetric");
            let (mut kp, mut kq) = ([0u8; 8], [0u8; 8]);
            cstring::xfrm(p, 8, &mut kp[..]).unwrap();
            cstring::xfrm(q, 8, &mut kq[..]).unwrap();
            assert_eq!(bytes::compare_text(text(&kp), text(&kq)), bytes::compare_text(p, q), "strxfrm agrees with strcoll");
        }
    }
}

#[test]
fn s4_3_f_0() {
    // In the C locale every byte string is valid, so strcoll sets no errno.
    liella_boundary::errno::set_raw(0);
    let r = x::strcoll(z("\u{ff}").as_ptr().cast(), z("a").as_ptr().cast());
    assert!(r > 0);
    assert_eq!(liella_boundary::errno::get(), 0);
}

// ------------------------------------------------------------------------------ 4.4 strxfrm

#[test]
fn s4_4_c_0() {
    let mut d = [9u8; 4];
    assert_eq!(cstring::xfrm(b"abc", 4, &mut d[..]).unwrap(), 3);
    assert_eq!(&d, b"abc\0");
    let mut short = [9u8; 3];
    assert_eq!(cstring::xfrm(b"abc", 3, &mut short[..]).unwrap(), 3, ">= n: incomplete");
    assert_eq!(cstring::xfrm(b"abc", 0, &mut [0u8; 0][..]).unwrap(), 3, "measuring call writes nothing");
}

#[test]
fn s4_4_c_0_one_pass_fill() {
    // Clauses: s4_4_c_0 (STRXFRM-EN: the one-pass key writes as the two-pass one did).
    fn put(k: &[u8]) -> impl Fn(&mut [u8]) -> Option<usize> + '_ {
        move |o: &mut [u8]| {
            let n = k.len().min(o.len());
            o.get_mut(..n)?.copy_from_slice(k.get(..n)?);
            Some(k.len())
        }
    }
    let lazy = |k: &'static [u8]| move || k.iter().copied();
    for len in [0usize, 1, 3, 255, 256, 257, 600] {
        let k: &'static [u8] = vec![b'k'; len].leak();
        for n in [0, len.saturating_sub(1), len, len + 1, len + 9] {
            let mut d = vec![9u8; n];
            assert_eq!(cstring::xfrm_fill(n, &mut d[..], put(k), lazy(k)).unwrap(), len, "len {len} n {n}");
            if len < n {
                assert_eq!(&d[..len], k, "len {len} n {n}: the key");
                assert_eq!(d[len], 0, "len {len} n {n}: terminated");
                assert!(d[len + 1..].iter().all(|&b| b == 9), "len {len} n {n}: nothing past the terminator");
            } else {
                assert!(d.iter().all(|&b| b == 9), "len {len} n {n}: an incomplete key writes nothing");
            }
            // No one-pass engine: the lazy key is measured and written.
            let mut e = vec![9u8; n];
            assert_eq!(cstring::xfrm_fill(n, &mut e[..], |_: &mut [u8]| None, lazy(k)).unwrap(), len);
            assert_eq!(d, e, "len {len} n {n}: both paths write the same");
        }
    }
}

// ------------------------------------------------------------------------------ 4.5 search a unit

#[test]
fn s4_5_c_1() {
    assert_eq!(bytes::find(&[1u8, 0xff, 0xff], 0xff), Some(1));
    assert_eq!(bytes::find(&[1u8, 2, 3][..2], 3), None, "only s[0..n)");
}

#[test]
fn s4_5_c_2() {
    assert_eq!(bytes::rfind(b"abab", b'a'), Some(2));
    assert_eq!(bytes::rfind(b"", b'a'), None);
}

#[test]
fn s4_5_c_3() {
    let v = z("abc");
    let r = x::rawmemchr(v.as_ptr().cast(), i32::from(b'c'));
    assert_eq!(r.cast::<u8>(), v.as_ptr().wrapping_add(2).cast_mut());
}

#[test]
fn s4_5_c_4() {
    assert_eq!(bytes::find_z(b"abc", b'b'), Some(1));
    assert_eq!(bytes::find_z(b"abc", 0), Some(3), "the terminator is searchable");
    assert_eq!(bytes::find_z(b"abc", b'z'), None);
}

#[test]
fn s4_5_c_5() {
    assert_eq!(bytes::find_z_or_end(b"abc", b'z'), 3);
    assert_eq!(bytes::find_z_or_end(b"abc", b'a'), 0);
}

#[test]
fn s4_5_c_6() {
    assert_eq!(bytes::rfind_z(b"a/b/c", b'/'), Some(3));
    assert_eq!(bytes::rfind_z(b"abc", 0), Some(3));
}

#[test]
fn s4_5_c_7() {
    let v = z("abca");
    assert_eq!(x::index(v.as_ptr().cast(), 97), x::strchr(v.as_ptr().cast(), 97));
    assert_eq!(x::rindex(v.as_ptr().cast(), 97), x::strrchr(v.as_ptr().cast(), 97));
}

// ------------------------------------------------------------------------------ 4.6 search a sequence

#[test]
fn s4_6_c_0() {
    assert_eq!(bytes::find_seq(b"abcabd", b"abd", core::convert::identity), Some(3));
    assert_eq!(bytes::find_seq(b"abc", b"", core::convert::identity), Some(0), "empty needle");
    assert_eq!(bytes::find_seq(b"ab", b"abc", core::convert::identity), None);
    assert_eq!(bytes::find_seq(b"xHeLLo", b"hello", ctype::lower_c), Some(1), "strcasestr folds");
}

#[test]
fn s4_6_g_0() {
    // The quadratic worst case (a^n against a^(n/2) b) would take ~10^11 steps here.
    let hay = vec![b'a'; 1 << 20];
    let mut needle = vec![b'a'; 1 << 19];
    needle.push(b'b');
    assert_eq!(bytes::find_seq(&hay, &needle, core::convert::identity), None);
    let mut hay2 = hay.clone();
    hay2.push(b'b');
    assert_eq!(bytes::find_seq(&hay2, &needle, core::convert::identity), Some((1 << 20) - (1 << 19)));
}

// ------------------------------------------------------------------------------ 4.7 sets

#[test]
fn s4_7_c_1() {
    assert_eq!(bytes::span(b"aabxa", ByteSet::new(b"ab").predicate()), 3);
}

#[test]
fn s4_7_c_2() {
    assert_eq!(bytes::span(b"abc:d", ByteSet::new(b":;").absent()), 3);
}

#[test]
fn s4_7_c_3() {
    assert_eq!(bytes::find_any(b"ab;c:", ByteSet::new(b":;").predicate()), Some(2));
    assert_eq!(bytes::find_any(b"abc", ByteSet::new(b"x").predicate()), None);
}

#[test]
fn s4_7_i_0() {
    assert_eq!(bytes::span(b"abcabc", ByteSet::new(b"cba").predicate()), bytes::span(b"abcabc", ByteSet::new(b"aabbcc").predicate()), "order and duplicates do not matter");
    assert!(!ByteSet::new(b"a\0b").contains(0), "0 is never a member");
}

// ------------------------------------------------------------------------------ 4.8 length

#[test]
fn s4_8_c_0() {
    assert_eq!(x::strlen(z("hello").as_ptr().cast()), 5);
    assert_eq!(x::strnlen(z("hello").as_ptr().cast(), 2), 2);
    let unterminated = [b'a'; 3];
    assert_eq!(x::strnlen(unterminated.as_ptr().cast(), 3), 3, "reads no more than n bytes");
}

// ------------------------------------------------------------------------------ 4.9 concatenate

#[test]
fn s4_9_c_1() {
    let mut d = [0u8; 8];
    d[..2].copy_from_slice(b"ab");
    cstring::cat_z(2, b"cd", &mut d[..]).unwrap();
    assert_eq!(text(&d), b"abcd");
}

#[test]
fn s4_9_c_2() {
    let mut d = [9u8; 8];
    d[..3].copy_from_slice(b"ab\0");
    cstring::cat_z(2, &b"cdef"[..2], &mut d[..]).unwrap();
    assert_eq!(&d[..5], b"abcd\0", "at most n of s, then a terminator");
}

#[test]
fn s4_9_c_3() {
    let mut d = [0u8; 6];
    d[..2].copy_from_slice(b"ab");
    assert_eq!(cstring::lcat(2, b"cdefg", 6, &mut d[..]).unwrap(), 7);
    assert_eq!(text(&d), b"abcde", "total kept at size - 1");
    let mut u = *b"xxx";
    assert_eq!(cstring::lcat(3, b"ab", 3, &mut u[..]).unwrap(), 5, "unterminated within size");
    assert_eq!(&u, b"xxx");
}

// ------------------------------------------------------------------------------ 4.10 duplicate

#[test]
fn s4_10_c_0() {
    let src = z("dup");
    let p = x::strdup(src.as_ptr().cast());
    assert_ne!(p.cast::<u8>(), src.as_ptr().cast_mut(), "a new copy");
    assert_eq!(x::strcmp(p, src.as_ptr().cast()), 0);
    let q = x::strndup(z("abcdef").as_ptr().cast(), 2);
    assert_eq!((x::strlen(q), x::strcmp(q, z("ab").as_ptr().cast())), (2, 0), "at most n bytes, terminated");
}

#[test]
fn s4_10_f_0() {
    // The address-space cap needs a process of its own, and the allocation must not run in a
    // forked copy of the multi-threaded harness (another thread may hold the allocator's lock
    // at the fork). So the test re-runs itself, alone, in a fresh process.
    if std::env::var_os("LIELLA_S4_10_F_0").is_some() {
        let mut big = vec![b'a'; 64 << 20];
        big.push(0);
        let statm = std::fs::read_to_string("/proc/self/statm").unwrap();
        let pages: u64 = statm.split_whitespace().next().unwrap().parse().unwrap();
        CSys.prlimit(0, RLIMIT_AS, Some(&Rlimit { cur: pages * 4096 + (16 << 20), max: RLIM_INFINITY })).unwrap();
        liella_boundary::errno::set_raw(0);
        let p = liella_boundary::selftest::opaque::strdup(big.as_ptr().cast());
        assert!(p.is_null(), "NULL");
        assert_eq!(liella_boundary::errno::get(), Errno::ENOMEM.get(), "ENOMEM");
        return;
    }
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["s4_10_f_0", "--exact", "--test-threads=1"])
        .env("LIELLA_S4_10_F_0", "1")
        .status()
        .unwrap();
    assert!(status.success(), "strdup without memory: NULL with ENOMEM");
}

// ------------------------------------------------------------------------------ 4.11 tokenize

#[test]
fn s4_11_c_0() {
    let toks: Vec<_> = Tokens::new(b"a,b;;c", |c| c == b',' || c == b';', true).collect();
    assert_eq!(toks, [(0, 1), (2, 1), (5, 1)]);
}

#[test]
fn s4_11_c_1() {
    assert_eq!(tokens("::a:bc::d:", b":"), ["a", "bc", "d"], "leading delimiters skipped, empty tokens never returned");
    let mut s = *b"ab:cd";
    assert_eq!(string_compat::strtok_step(&mut s, |c| c == b':'), (Some(0), 3));
    assert_eq!(&s, b"ab\0cd", "the token is terminated in place");
    // The hidden process-global position, through the export.
    let mut v = z("x y");
    let d = z(" ");
    let first = x::strtok(v.as_mut_ptr().cast(), d.as_ptr().cast());
    let second = x::strtok(null_mut(), d.as_ptr().cast());
    assert_eq!((first.cast::<u8>(), second.cast::<u8>()), (v.as_mut_ptr(), v.as_mut_ptr().wrapping_add(2)));
}

#[test]
fn s4_11_c_2() {
    // Two independent cursors interleave without interfering.
    let (mut b1, mut b2) = (z("a b"), z("c d"));
    let (mut m1, mut m2) = (Mem(&mut b1), Mem(&mut b2));
    let (mut c1, mut c2) = (Cell(0), Cell(0));
    let sp = || ByteSet::new(b" ").predicate();
    assert_eq!(string_compat::strtok(1, sp(), &mut c1, &mut m1), 1);
    assert_eq!(string_compat::strtok(1, sp(), &mut c2, &mut m2), 1);
    assert_eq!(string_compat::strtok(0, sp(), &mut c1, &mut m1), 3);
    assert_eq!(string_compat::strtok(0, sp(), &mut c2, &mut m2), 3);
}

#[test]
fn s4_11_c_3() {
    let mut buf = z("a::b");
    let mut mem = Mem(&mut buf);
    let mut cur = Cell(1);
    let mut got = Vec::new();
    loop {
        let t = string_compat::strsep(&mut cur, ByteSet::new(b":").predicate(), &mut mem);
        if t == 0 {
            break;
        }
        got.push(t);
    }
    assert_eq!(got, [1, 3, 4], "empty tokens are returned");
    assert_eq!(cur.0, 0, "the cursor becomes NULL after the last token");
}

#[test]
fn s4_11_c_4() {
    let mut v: Vec<i32> = "a.b".chars().map(|c| c as i32).chain([0]).collect();
    let d = [b'.' as i32, 0];
    let mut save = null_mut();
    let t1 = x::wcstok(v.as_mut_ptr(), d.as_ptr(), Some(&mut save));
    let t2 = x::wcstok(null_mut(), d.as_ptr(), Some(&mut save));
    assert_eq!((t1, t2), (v.as_mut_ptr(), v.as_mut_ptr().wrapping_add(2)));
    assert!(x::wcstok(null_mut(), d.as_ptr(), Some(&mut save)).is_null());
}

// ------------------------------------------------------------------------------ 4.13 argz

fn entries(argz: &[u8]) -> Vec<&[u8]> {
    cstring::argz_entries(argz).map(|(s, l)| &argz[s..s + l]).collect()
}

#[test]
fn s4_13_c_0() {
    let v = cstring::argz_from_list([&b"a"[..], b"bc"].into_iter()).unwrap();
    assert_eq!(v, b"a\0bc\0", "one buffer of terminated strings; its length is the total");
}

#[test]
fn s4_13_c_1() {
    assert_eq!(cstring::argz_from_list([&b"x"[..], b""].into_iter()).unwrap(), b"x\0\0");
    assert_eq!(cstring::argz_from_sep(b":a::b:", b':').unwrap(), b"a\0b\0");
}

#[test]
fn s4_13_c_2() {
    assert_eq!(cstring::argz_count(b"a\0\0b\0"), 3);
    assert_eq!(cstring::argz_count(b""), 0);
}

#[test]
fn s4_13_c_3() {
    let v = b"a\0bc\0";
    let mut out = [99usize; 3];
    cstring::argz_extract(v, 1000, &mut out[..]).unwrap();
    assert_eq!(out, [1000, 1002, 0], "entry addresses, then NULL");
}

#[test]
fn s4_13_c_4() {
    let mut v = *b"a\0b\0c\0";
    cstring::argz_stringify(&mut v, b',');
    assert_eq!(&v, b"a,b,c\0");
}

#[test]
fn s4_13_c_5() {
    let v = cstring::argz_add(b"a\0", b"c").unwrap();
    let v = cstring::argz_insert(&v, 2, b"b").unwrap();
    assert_eq!(entries(&v), [&b"a"[..], b"b", b"c"]);
    let v = cstring::argz_delete(&v, 2).unwrap();
    assert_eq!(entries(&v), [&b"a"[..], b"c"]);
}

#[test]
fn s4_13_c_6() {
    let v = b"a\0bc\0";
    assert_eq!(cstring::argz_next(v, None), Some(0));
    assert_eq!(cstring::argz_next(v, Some(0)), Some(2));
    assert_eq!(cstring::argz_next(v, Some(2)), None);
    assert_eq!(cstring::argz_next(b"", None), None);
}

#[test]
fn s4_13_c_7() {
    let (v, n) = cstring::argz_replace(b"aXa\0X\0", b"X", b"yz").unwrap();
    assert_eq!((v.as_slice(), n), (&b"ayza\0yz\0"[..], 2));
}

#[test]
fn s4_13_f_0() {
    let mut z_ = null_mut();
    let mut n = 0;
    assert_eq!(x::argz_create_sep(z("a b").as_ptr().cast(), i32::from(b' '), Some(&mut z_), Some(&mut n)), 0, "0 on success");
    assert_eq!(x::argz_insert(Some(&mut z_), Some(&mut n), z("elsewhere").as_mut_ptr().cast(), z("q").as_ptr().cast()), Errno::EINVAL.get(), "an error_t, not -1");
}

// ------------------------------------------------------------------------------ 4.14 envz

#[test]
fn s4_14_c_0() {
    let e = b"A=1\0N\0E=\0";
    assert_eq!(cstring::envz_get(e, b"A"), Some(2));
    assert_eq!(cstring::envz_get(e, b"N"), None, "null value");
    assert_eq!(cstring::envz_get(e, b"E"), Some(8), "empty value, distinct from null");
    assert_eq!(cstring::envz_entry(e, b"N"), Some(4));
    let added = cstring::envz_add(e, b"A", Some(b"2")).unwrap();
    assert_eq!(entries(&added), [&b"N"[..], b"E=", b"A=2"]);
    let kept = cstring::envz_merge(e, b"A=9\0B=1\0", false).unwrap();
    assert_eq!(entries(&kept), [&b"A=1"[..], b"N", b"E=", b"B=1"]);
    let over = cstring::envz_merge(e, b"A=9\0", true).unwrap();
    assert_eq!(cstring::envz_get(&over, b"A").map(|i| over[i]), Some(b'9'));
    assert_eq!(entries(&cstring::envz_remove(e, b"E").unwrap()), [&b"A=1"[..], b"N"]);
    assert_eq!(entries(&cstring::envz_strip(e).unwrap()), [&b"A=1"[..], b"E="]);
}

// ------------------------------------------------------------------------------ clauses after a flag line (2026-10-04)

fn cz(s: &str) -> Vec<c_char> {
    s.bytes().map(|b| b as c_char).chain(core::iter::once(0)).collect()
}

#[test]
fn s4_1_c_8() {
    // stpncpy is strncpy returning d + min(strlen(s), n).
    let s = cz("abc");
    let mut d = [b'x' as c_char; 6];
    let r = x::stpncpy(d.as_mut_ptr(), s.as_ptr(), 5);
    assert_eq!(r, d.as_mut_ptr().wrapping_add(3), "short source: at the first padding NUL");
    assert_eq!(d.map(|c| c as u8), *b"abc\0\0x", "padded to n");
    let r = x::stpncpy(d.as_mut_ptr(), s.as_ptr(), 2);
    assert_eq!(r, d.as_mut_ptr().wrapping_add(2), "truncated: d + n, no terminator");
}

#[test]
fn s4_1_c_9() {
    // strlcpy copies at most n-1 bytes, terminates when n > 0, returns strlen(s).
    let s = cz("abcdef");
    let mut d = [b'x' as c_char; 8];
    assert_eq!(x::strlcpy(d.as_mut_ptr(), s.as_ptr(), 4), 6, "a result >= n means truncation");
    assert_eq!(&d[..4].iter().map(|&c| c as u8).collect::<Vec<_>>(), b"abc\0");
    assert_eq!(x::strlcpy(d.as_mut_ptr(), s.as_ptr(), 0), 6, "n 0: nothing written");
    assert_eq!(d[0] as u8, b'a');
    assert_eq!(x::strlcpy(d.as_mut_ptr(), s.as_ptr(), 8), 6);
    assert_eq!(&d[..7].iter().map(|&c| c as u8).collect::<Vec<_>>(), b"abcdef\0");
}

#[test]
fn s4_1_c_10() {
    // The wide forms are the same contracts over wchar_t units.
    let s: Vec<i32> = "abc".chars().map(|c| c as i32).chain(core::iter::once(0)).collect();
    let mut d = [7i32; 6];
    let r = x::wcpncpy(d.as_mut_ptr().cast(), s.as_ptr().cast(), 5);
    assert_eq!(r, d.as_mut_ptr().wrapping_add(3).cast());
    assert_eq!(d, [97, 98, 99, 0, 0, 7]);
    let mut w = [7i32; 3];
    assert_eq!(x::wcslcpy(w.as_mut_ptr().cast(), s.as_ptr().cast(), 3), 3);
    assert_eq!(w, [97, 98, 0]);
}

#[test]
fn s4_3_c_5() {
    // strcoll/wcscoll: in the C locale, strcmp's order (wcscmp's for wide).
    let (a, b) = (cz("B"), cz("a"));
    assert!(x::strcoll(a.as_ptr(), b.as_ptr()) < 0, "'B' < 'a' by byte");
    assert_eq!(x::strcoll(a.as_ptr(), a.as_ptr()), 0);
    let wa: Vec<i32> = vec![66, 0];
    let wb: Vec<i32> = vec![97, 0];
    assert!(x::wcscoll(wa.as_ptr().cast(), wb.as_ptr().cast()) < 0);
}

#[test]
fn s4_3_c_6() {
    // strverscmp: "file9" < "file10"; leading zeros are fractional parts.
    let v = |a: &str, b: &str| x::strverscmp(cz(a).as_ptr(), cz(b).as_ptr()).signum();
    assert_eq!(v("file9", "file10"), -1);
    assert_eq!(v("a1", "a1"), 0);
    assert_eq!(v("000", "00"), -1, "more leading zeros: a smaller fraction");
    assert_eq!(v("009", "01"), -1);
}

#[test]
fn s4_3_c_7() {
    // The _l variants take an explicit locale: strcoll_l with the C locale is strcmp's order
    // whatever the thread's locale.
    use liella_boundary::posix::s07_locale as lx;
    let c = lx::newlocale(0x1fbf, cz("C").as_ptr(), core::ptr::null_mut());
    assert!(!c.is_null());
    assert!(x::strcoll_l(cz("B").as_ptr(), cz("a").as_ptr(), c) < 0);
    lx::freelocale(c);
}
