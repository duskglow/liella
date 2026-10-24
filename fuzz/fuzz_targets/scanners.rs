//! The C layer's string scanners (D-4.14, P7; PJ-8): `strcmp` (the one-pass compare), `strlen`,
//! `strnlen` (with the short-string prologue) and `getenv` (the one-pass `environ` scan), against
//! plain Rust references, with the strings placed at the end of a page whose next page is
//! unmapped (a read past a terminator faults) and at a fuzzed offset inside a page.
//!
//! Input: two bytes (the offsets of the second copies), then `a 0xFF b`; `a` and `b` may hold
//! NULs (each C string ends at its first). For `getenv`, `a` split at `\n` is the environment
//! and `b` (up to its first NUL) the name.
#![no_main]
#![allow(clippy::disallowed_methods)] // a test tool: S8 (fallible allocation) governs the library crates only

use core::cmp::Ordering;
use core::sync::atomic::Ordering::{Acquire, Release};
use std::sync::OnceLock;

use libfuzzer_sys::fuzz_target;
use liella_abi::ctypes::{c_char, c_void};
use liella_boundary::posix::{s01_startup as env, s04_strings as s, s11_lowio as lowio};
use liella_fuzz::Input;

const PAGE: usize = 4096;

/// Two readable pages, each followed by an unmapped one: their base addresses.
fn pages() -> (usize, usize) {
    *PAGES.get_or_init(|| {
        let map = || {
            let p = lowio::mmap(core::ptr::null_mut(), 2 * PAGE, 3, 0x22, -1, 0) as usize;
            assert!(p != usize::MAX && p != 0, "mmap");
            assert_eq!(lowio::mprotect((p + PAGE) as *mut c_void, PAGE, 0), 0, "mprotect");
            p
        };
        (map(), map())
    })
}
static PAGES: OnceLock<(usize, usize)> = OnceLock::new();

/// The C string of `t`: up to its first NUL.
fn cstr(t: &[u8]) -> &[u8] {
    t.split(|&c| c == 0).next().unwrap_or(&[])
}

/// Copy `t` (cut to fit, then terminated) so that its terminator is the last byte of the page
/// at `base`; the string's address.
fn at_end(base: usize, t: &[u8]) -> usize {
    let t = &t[..t.len().min(PAGE - 1)];
    let p = base + PAGE - 1 - t.len();
    s::memcpy(p as *mut c_void, t.as_ptr().cast(), t.len());
    s::memset((base + PAGE - 1) as *mut c_void, 0, 1);
    p
}

fn sign(o: Ordering) -> i32 {
    o as i32
}

fuzz_target!(|data: &[u8]| {
    let mut inp = Input::new(data);
    let (oa, ob) = (usize::from(inp.byte()), usize::from(inp.byte()));
    let rest = inp.rest();
    let (a, b) = match rest.iter().position(|&c| c == 0xFF) {
        Some(i) => (&rest[..i], &rest[i + 1..]),
        None => (rest, &[][..]),
    };
    let (ca, cb) = (cstr(a), cstr(b));
    let (pa_base, pb_base) = pages();
    let (pa, pb) = (at_end(pa_base, a), at_end(pb_base, b));
    let (ca, cb) = (&ca[..ca.len().min(PAGE - 1)], &cb[..cb.len().min(PAGE - 1)]);
    let want = sign(ca.cmp(cb));
    assert_eq!(s::strcmp(pa as *const c_char, pb as *const c_char).signum(), want, "strcmp at page ends");
    assert_eq!(s::strlen(pa as *const c_char), ca.len(), "strlen at a page end");
    for max in [0, 1, ob % 40, ca.len(), ca.len() + 1] {
        assert_eq!(s::strnlen(pa as *const c_char, max), ca.len().min(max), "strnlen at a page end, max {max}");
    }
    // The same strings at fuzzed offsets in ordinary buffers.
    let mut va = vec![b'#'; oa + a.len() + 1];
    va[oa..oa + a.len()].copy_from_slice(a);
    va[oa + a.len()] = 0;
    let mut vb = vec![b'#'; ob + b.len() + 1];
    vb[ob..ob + b.len()].copy_from_slice(b);
    vb[ob + b.len()] = 0;
    let (qa, qb) = (va[oa..].as_ptr().cast::<c_char>(), vb[ob..].as_ptr().cast::<c_char>());
    assert_eq!(s::strcmp(qa, qb).signum(), sign(cstr(a).cmp(cstr(b))), "strcmp at offsets {oa}/{ob}");
    assert_eq!(s::strlen(qa), cstr(a).len(), "strlen at offset {oa}");
    // getenv over the environment `a` (split at newlines), name `b`, against the definition.
    let entries: Vec<Vec<u8>> = cstr(a).split(|&c| c == b'\n').map(|e| [e, &[0][..]].concat()).collect();
    let arr: Vec<*const c_char> = entries.iter().map(|e| e.as_ptr().cast()).chain([core::ptr::null()]).collect();
    let name = cstr(b);
    let want = entries.iter().find_map(|e| {
        let e = &e[..e.len() - 1];
        e.strip_prefix(name).and_then(|r| r.strip_prefix(b"=")).map(|v| v.to_vec())
    });
    let saved = liella_boundary::ffi::environ.load(Acquire);
    liella_boundary::ffi::environ.store(arr.as_ptr().expose_provenance(), Release);
    let got = env::getenv(qb);
    let got = (!got.is_null()).then(|| {
        let n = s::strlen(got);
        let mut v = vec![0u8; n];
        s::memcpy(v.as_mut_ptr().cast(), got.cast(), n);
        v
    });
    liella_boundary::ffi::environ.store(saved, Release);
    assert_eq!(got, want, "getenv");
});
