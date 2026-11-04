//! The file-system walkers run out of memory (ENOMEM-FS-WALK).
//!
//! Each case runs in a re-executed child of the test binary. The child prepares everything it
//! needs, sets `RLIMIT_AS` to its current size plus a margin (calibrated from `/proc/self/statm`,
//! never a fixed number), then hoards the heap with `malloc` until `malloc(1)` fails. The
//! walker is then called at growing headroom: none at all, then 1, 2, 4, ... of the hoarded
//! blocks given back, then all of them. At EVERY stage the call must either
//!
//! * report its documented failure (`-1`/`NULL` with `ENOMEM`; `glob`: `GLOB_NOSPACE` with the
//!   structure empty), or
//! * succeed with the COMPLETE result (a truncated success is the bug these tests catch),
//!
//! and once the result is released, the heap's in-use bytes (`mallinfo2().uordblks`) must be what
//! they were before the call: no partial result stays allocated. With no headroom the call must
//! fail; with all of it, it must succeed. The child reports through its exit status only (no
//! allocation after the limit is set).
//!
//! The scratch tree is built by the test; names are compared in sorted order or as counts
//! (no file-system dependence). `nftw` is compared by its number of callbacks.
//!
//! Clauses: s13_3_f_0 (walks fail with -1 and `errno`), s14_8_f_0 (`GLOB_NOSPACE`), s13_1_f_0
//! (`getcwd`), s0_6_c0_1_1 (a failing function returns its sentinel and sets `errno`).
#![no_builtins] // memcpy/strlen reach Liella's exports, which read the C memory (no `unsafe` in tests)
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::cast_possible_truncation, clippy::cast_possible_wrap, missing_docs)]

use core::ptr::null_mut;
use std::ffi::{c_void, CString};
use std::sync::atomic::{AtomicUsize, Ordering};

use liella_abi::callback::GlobT;
use liella_abi::ctypes::{c_char, c_int};
use liella_abi::layout::{struct_dirent, FTSENT};
use liella_boundary::errno;
use liella_boundary::posix::{s03_memory, s04_strings as st, s13_filesystem as x, s14_search as g};
use liella_boundary::selftest::opaque;
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::{Rlimit, RLIMIT_AS, RLIM_INFINITY};
use liella_kernel::Errno;

const FILES: usize = 300;
const ENV_CASE: &str = "LIELLA_ENOMEM_FS_DIR";

/// Exit statuses of the child.
const OK: i32 = 0;
const NOT_FAILING_EMPTY: i32 = 10;
const NOT_SUCCEEDING_FULL: i32 = 11;
const LEAK: i32 = 12;
const BAD_FAILURE: i32 = 13;
const TRUNCATED: i32 = 14;
const NO_HOARD: i32 = 15;

enum Verdict {
    Complete,
    Failed,
    Truncated,
    BadFailure,
}

fn name(i: usize) -> String {
    format!("f{i:03}")
}

/// The scratch tree: `FILES` files `f000..`, and two others the filter drops.
fn build_tree(tag: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("liella-enomem-fs-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    for i in 0..FILES {
        std::fs::write(d.join(name(i)), b"").unwrap();
    }
    for n in ["g1", "g2"] {
        std::fs::write(d.join(n), b"").unwrap();
    }
    std::fs::canonicalize(&d).unwrap()
}

fn cstr(p: &std::path::Path) -> CString {
    CString::new(p.as_os_str().as_encoded_bytes()).unwrap()
}

/// Run `attempt` under the exhausted heap at growing headroom; the exit status of the child.
fn staged(mut attempt: impl FnMut() -> Verdict) -> i32 {
    let statm = std::fs::read_to_string("/proc/self/statm").unwrap();
    let pages: u64 = statm.split_whitespace().next().unwrap().parse().unwrap();
    let mut hoard: Vec<*mut c_void> = Vec::with_capacity(1 << 20);
    CSys.prlimit(0, RLIMIT_AS, Some(&Rlimit { cur: pages * 4096 + (32 << 20), max: RLIM_INFINITY })).unwrap();
    for size in [1usize << 20, 1 << 16, 1 << 12, 256, 16, 1] {
        loop {
            let p = opaque::malloc(size);
            if p.is_null() || hoard.len() == hoard.capacity() {
                break;
            }
            hoard.push(p);
        }
    }
    if hoard.is_empty() || !opaque::malloc(1).is_null() {
        eprintln!("DEBUG hoard {} cap {} pages {pages}", hoard.len(), hoard.capacity());
        return NO_HOARD;
    }
    let mut next_free = 0usize;
    let mut stage = 0usize;
    loop {
        eprintln!("DEBUG stage {stage} hoard {}", hoard.len());
        let before = s03_memory::mallinfo2().uordblks;
        let verdict = attempt();
        let after = s03_memory::mallinfo2().uordblks;
        let all_back = hoard.is_empty();
        if after != before {
            return LEAK;
        }
        match verdict {
            Verdict::Truncated => return TRUNCATED,
            Verdict::BadFailure => return BAD_FAILURE,
            Verdict::Failed if all_back => return NOT_SUCCEEDING_FULL,
            Verdict::Complete if stage == 0 => return NOT_FAILING_EMPTY,
            _ => {}
        }
        if all_back {
            return OK;
        }
        // Give back 1, 2, 4, ... blocks (the last hoarded, smallest first); at the end all of them.
        next_free = if next_free == 0 { 1 } else { next_free * 2 };
        for _ in 0..next_free {
            match hoard.pop() {
                Some(p) => opaque::free(p),
                None => break,
            }
        }
        stage += 1;
    }
}

/// `true` when `errno` is `ENOMEM`.
fn enomem() -> bool {
    errno::get() == Errno::ENOMEM.get()
}

fn failed_or(ok: bool, got_enomem: bool) -> Verdict {
    if ok {
        Verdict::Complete
    } else if got_enomem {
        Verdict::Failed
    } else {
        Verdict::BadFailure
    }
}

/// The word at `p + off`, read with the library's `memcpy` (no allocation, no `unsafe`).
fn rd(p: usize, off: usize) -> usize {
    let mut b = [0u8; 8];
    st::memcpy(b.as_mut_ptr().cast(), (p + off) as *const c_void, 8);
    usize::from_ne_bytes(b)
}

/// The `i32` at `p + off`.
fn rd32(p: usize, off: usize) -> i32 {
    let mut b = [0u8; 4];
    st::memcpy(b.as_mut_ptr().cast(), (p + off) as *const c_void, 4);
    i32::from_ne_bytes(b)
}

/// The `u16` at `p + off`.
fn rd16(p: usize, off: usize) -> u16 {
    let mut b = [0u8; 2];
    st::memcpy(b.as_mut_ptr().cast(), (p + off) as *const c_void, 2);
    u16::from_ne_bytes(b)
}

/// Whether the C text at `p` is `want`.
fn is_text(p: usize, want: &[u8]) -> bool {
    p != 0 && st::strlen(p as *const c_char) == want.len() && st::memcmp(p as *const c_void, want.as_ptr().cast(), want.len()) == 0
}

/// Compare two C texts (bytewise).
fn cmp_text(a: usize, b: usize) -> c_int {
    let (la, lb) = (st::strlen(a as *const c_char), st::strlen(b as *const c_char));
    let c = st::memcmp(a as *const c_void, b as *const c_void, la.min(lb) + 1);
    c.signum()
}

const D_NAME: usize = core::mem::offset_of!(struct_dirent, d_name);

extern "C" fn keep_f(e: *const c_void) -> c_int {
    // A dirent from scandir; the name follows the header.
    let mut b = [0u8; 1];
    st::memcpy(b.as_mut_ptr().cast(), (e as usize + D_NAME) as *const c_void, 1);
    c_int::from(b[0] == b'f')
}

extern "C" fn by_name(a: *const *const c_void, b: *const *const c_void) -> c_int {
    x::alphasort(a, b)
}

fn case_scandir(dir: &std::path::Path) -> i32 {
    let d = cstr(dir);
    let want: Vec<CString> = (0..FILES).map(|i| CString::new(name(i)).unwrap()).collect();
    let cmp: liella_abi::callback::SlotCompareFn = by_name;
    staged(|| {
        let mut list: *mut *mut struct_dirent = null_mut();
        errno::set_raw(0);
        let n = x::scandir(d.as_ptr(), Some(&mut list), Some(keep_f), Some(cmp));
        if n < 0 {
            return failed_or(false, enomem());
        }
        let mut ok = n as usize == FILES && !list.is_null();
        if ok {
            for (i, w) in want.iter().enumerate() {
                let e = rd(list as usize, 8 * i);
                ok &= is_text(e + D_NAME, w.to_bytes());
            }
        }
        if !list.is_null() {
            for i in 0..n as usize {
                opaque::free(rd(list as usize, 8 * i) as *mut c_void);
            }
            opaque::free(list.cast());
        }
        if ok { Verdict::Complete } else { Verdict::Truncated }
    })
}

fn case_glob(dir: &std::path::Path) -> i32 {
    let pat = CString::new(format!("{}/f*", dir.display())).unwrap();
    let prefix = format!("{}/", dir.display());
    let want: Vec<CString> = (0..FILES).map(|i| CString::new(format!("{prefix}{}", name(i))).unwrap()).collect();
    staged(|| {
        let mut gl = GlobT { gl_pathc: 0, gl_pathv: 0, gl_offs: 0, gl_flags: 0, gl_closedir: None, gl_readdir: None, gl_opendir: None, gl_lstat: None, gl_stat: None };
        let r = g::glob(pat.as_ptr(), 0, None, Some(&mut gl));
        if r != 0 {
            // GLOB_NOSPACE is 1; the structure holds no result (nothing appended).
            return if r == 1 && gl.gl_pathc == 0 && gl.gl_pathv == 0 { Verdict::Failed } else { Verdict::BadFailure };
        }
        let mut ok = gl.gl_pathc as usize == FILES && gl.gl_pathv != 0;
        if ok {
            for (i, w) in want.iter().enumerate() {
                ok &= is_text(rd(gl.gl_pathv, 8 * i), w.to_bytes());
            }
        }
        g::globfree(Some(&mut gl));
        if ok { Verdict::Complete } else { Verdict::Truncated }
    })
}

const PATH: usize = core::mem::offset_of!(FTSENT, fts_path);

extern "C" fn by_path(a: *const *const c_void, b: *const *const c_void) -> c_int {
    // Two pointers to FTSENT pointers.
    let (ea, eb) = (rd(a as usize, 0), rd(b as usize, 0));
    cmp_text(rd(ea, PATH), rd(eb, PATH))
}

fn case_fts(dir: &std::path::Path) -> i32 {
    let d = cstr(dir);
    let argv: [*mut c_char; 2] = [d.as_ptr().cast_mut(), null_mut()];
    let mut want: Vec<CString> = vec![CString::new(dir.as_os_str().as_encoded_bytes()).unwrap()];
    for i in 0..FILES {
        want.push(CString::new(format!("{}/{}", dir.display(), name(i))).unwrap());
    }
    for n in ["g1", "g2"] {
        want.push(CString::new(format!("{}/{n}", dir.display())).unwrap());
    }
    let cmp: liella_abi::callback::SlotCompareFn = by_path;
    staged(|| {
        errno::set_raw(0);
        // FTS_PHYSICAL | FTS_NOCHDIR
        let t = x::fts_open(argv.as_ptr(), 0x10 | 0x04, Some(cmp));
        if t.is_null() {
            return failed_or(false, enomem());
        }
        let (mut seen, mut ok, mut failed, mut bad);
        (seen, ok, failed, bad) = (0usize, true, false, false);
        loop {
            errno::set_raw(0);
            let e = x::fts_read(t).cast::<FTSENT>();
            if e.is_null() {
                failed = errno::get() != 0;
                bad = failed && !enomem();
                break;
            }
            let ea = e as usize;
            let (info, path, ferr) = (rd16(ea, core::mem::offset_of!(FTSENT, fts_info)), rd(ea, PATH), rd32(ea, core::mem::offset_of!(FTSENT, fts_errno)));
            // FTS_D = 1 (pre-order), FTS_F = 8 (file), FTS_DP = 6 (post-order); FTS_DNR 4, FTS_ERR 7, FTS_NS 10 are failures.
            if matches!(info, 4 | 7 | 10) {
                failed = true;
                bad = ferr != Errno::ENOMEM.get();
                break;
            }
            if info != 6 {
                ok &= want.get(seen).is_some_and(|w| is_text(path, w.to_bytes()));
                seen += 1;
            }
        }
        x::fts_close(t);
        if bad {
            Verdict::BadFailure
        } else if failed {
            Verdict::Failed
        } else if ok && seen == FILES + 3 {
            Verdict::Complete
        } else {
            Verdict::Truncated
        }
    })
}

static CALLS: AtomicUsize = AtomicUsize::new(0);

extern "C" fn count_cb(_p: *const c_char, _st: *const c_void, _flag: c_int, _f: *mut c_void) -> c_int {
    CALLS.fetch_add(1, Ordering::Relaxed);
    0
}

fn case_nftw(dir: &std::path::Path) -> i32 {
    let d = cstr(dir);
    staged(|| {
        CALLS.store(0, Ordering::Relaxed);
        errno::set_raw(0);
        let r = x::nftw(d.as_ptr(), Some(count_cb), 10, 1);
        if r == -1 {
            return failed_or(false, enomem());
        }
        // The directory, the FILES files and the two others.
        if r == 0 && CALLS.load(Ordering::Relaxed) == FILES + 3 { Verdict::Complete } else { Verdict::Truncated }
    })
}

fn case_realpath(dir: &std::path::Path) -> i32 {
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let given = CString::new(format!("{}/./sub/../{}", dir.display(), name(7))).unwrap();
    let want = CString::new(format!("{}/{}", dir.display(), name(7))).unwrap();
    staged(|| {
        errno::set_raw(0);
        let p = x::realpath(given.as_ptr(), null_mut());
        if p.is_null() {
            return failed_or(false, enomem());
        }
        let ok = is_text(p as usize, want.to_bytes());
        opaque::free(p.cast());
        if ok { Verdict::Complete } else { Verdict::Truncated }
    })
}

fn case_canonicalize(dir: &std::path::Path) -> i32 {
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let given = CString::new(format!("{}/./sub/../{}", dir.display(), name(7))).unwrap();
    let want = CString::new(format!("{}/{}", dir.display(), name(7))).unwrap();
    staged(|| {
        errno::set_raw(0);
        let p = x::canonicalize_file_name(given.as_ptr());
        if p.is_null() {
            return failed_or(false, enomem());
        }
        let ok = is_text(p as usize, want.to_bytes());
        opaque::free(p.cast());
        if ok { Verdict::Complete } else { Verdict::Truncated }
    })
}

fn case_getcwd(dir: &std::path::Path) -> i32 {
    std::env::set_current_dir(dir).unwrap();
    let want = CString::new(dir.as_os_str().as_encoded_bytes()).unwrap();
    staged(|| {
        errno::set_raw(0);
        let p = x::getcwd(null_mut(), 0);
        eprintln!("DEBUG getcwd p={p:?} errno={} used={}", errno::get(), s03_memory::mallinfo2().uordblks);
        if p.is_null() {
            return failed_or(false, enomem());
        }
        let ok = is_text(p as usize, want.to_bytes());
        opaque::free(p.cast());
        if ok { Verdict::Complete } else { Verdict::Truncated }
    })
}

fn case_current_dir_name(dir: &std::path::Path) -> i32 {
    std::env::set_current_dir(dir).unwrap();
    let want = CString::new(dir.as_os_str().as_encoded_bytes()).unwrap();
    staged(|| {
        errno::set_raw(0);
        let p = x::get_current_dir_name();
        if p.is_null() {
            return failed_or(false, enomem());
        }
        let ok = is_text(p as usize, want.to_bytes());
        opaque::free(p.cast());
        if ok { Verdict::Complete } else { Verdict::Truncated }
    })
}

/// The parent: build the tree, run the case in a child of this binary; the child's status is the verdict.
fn drive(test: &str, case: fn(&std::path::Path) -> i32) {
    if let Ok(dir) = std::env::var(ENV_CASE) {
        let code = case(std::path::Path::new(&dir));
        std::process::exit(code);
    }
    let dir = build_tree(test);
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact", "--test-threads=1", "--nocapture"])
        .env(ENV_CASE, &dir)
        .env_remove("PWD")
        .status()
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(status.code(), Some(OK), "{test}: child status (10 not failing with no memory, 11 not succeeding with all of it, 12 a partial result stays allocated, 13 failure without ENOMEM/GLOB_NOSPACE, 14 truncated success, 15 heap not exhausted)");
}

// s13_3_f_0, s0_6_c0_1_1
#[test]
fn enomem_scandir_filter_alphasort() {
    drive("enomem_scandir_filter_alphasort", case_scandir);
}

// s14_8_f_0
#[test]
fn enomem_glob_many_matches() {
    drive("enomem_glob_many_matches", case_glob);
}

// s13_3_f_0
#[test]
fn enomem_fts_walk() {
    drive("enomem_fts_walk", case_fts);
}

// s13_3_f_0
#[test]
fn enomem_nftw_walk() {
    drive("enomem_nftw_walk", case_nftw);
}

// s0_6_c0_1_1
#[test]
fn enomem_realpath_null() {
    drive("enomem_realpath_null", case_realpath);
}

// s0_6_c0_1_1
#[test]
fn enomem_canonicalize_file_name() {
    drive("enomem_canonicalize_file_name", case_canonicalize);
}

// s13_1_f_0, s0_6_c0_1_1
#[test]
fn enomem_getcwd_null() {
    drive("enomem_getcwd_null", case_getcwd);
}

// s0_6_c0_1_1
#[test]
fn enomem_get_current_dir_name() {
    drive("enomem_get_current_dir_name", case_current_dir_name);
}
