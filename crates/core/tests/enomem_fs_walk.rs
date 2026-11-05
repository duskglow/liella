//! The file-system walkers run out of memory (ENOMEM-FS-WALK-2, on the ALLOC-FAIL-INJECT hook).
//!
//! For each walker the test fails the 1st, 2nd, 3rd ... allocation the call makes (the hook,
//! `liella_boundary::selftest::failinject`, fails one chosen allocation of the calling thread's
//! global-allocator traffic, once), until a call finishes without reaching the planned failure.
//! At every injected failure the call must report its documented failure (`-1`/`NULL` with
//! `ENOMEM`; `glob`: `GLOB_NOSPACE` with the structure empty; `fts_read` ends the walk with
//! `ENOMEM`), never a truncated success, and the C heap's in-use bytes (`mallinfo2().uordblks`)
//! must be what they were before the call (no partial result stays allocated on the Liella heap), as must, after a reported failure, the hook's live-byte
//! count of the global allocator (`failinject::live_bytes`) and the caller-owned block count (`selftest::caller_blocks`). The call without
//! any injection must succeed with the COMPLETE result.
//!
//! The hook reaches only global-allocator allocations (`liella::mem` collections,
//! `try_reserve`), not the caller-owned `malloc` family, the C heap, `KernelPages` or `OLD_HEAP`
//! (coordinator #2101/#2102). A walker none of whose allocations is reachable has nothing to
//! inject: the test then prints "NOT TESTABLE YET: <walker>" and passes (an ENOMEM path on the C
//! heap needs a product-code hook, which needs an owner ruling).
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
use liella_boundary::selftest::failinject::{self, FailAlloc};
use liella_boundary::selftest;
use liella_kernel::Errno;

const FILES: usize = 300;

#[derive(Debug)]
enum Verdict {
    Complete,
    /// Every entry, but not in the comparator's order (fts after a sort allocation failure).
    Unsorted,
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

#[global_allocator]
static ALLOC: FailAlloc = FailAlloc;

/// Serialises the cases that change or read the process working directory.
static CWD: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Fail the 1st, 2nd, ... allocation of `attempt` in turn (see the file header); the verdict
/// message, empty when all is as documented.
fn sweep(walker: &str, mut attempt: impl FnMut() -> Verdict) -> String {
    let mut failures = 0u64;
    let mut absorbed = 0u64;
    // One uninjected run first: a walker may keep a bounded pool of its own state (fts keeps the
    // slot of a closed or failed stream on a free list for the next open). That pool is filled
    // here, so the leak checks below see only what a failed call leaves behind.
    let _ = attempt();
    // `uordblks` is process-wide. With the exports interposed (the suite's second pass) every
    // thread's Rust allocations are Liella heap blocks, so an allocation by libtest's main thread
    // (its bookkeeping right after it starts this test's thread) can land between `before` and
    // `after` and look like a leak of the call (ENOMEM-REALPATH-INTERMITTENT: +32 and +1024 bytes
    // at allocation 1 on a loaded 2-node host). A window that another thread's allocator call
    // overlapped is not judged: the same `n` is measured again (`disturbed`). The
    // per-thread checks need no such care.
    let mut disturbed = 0u64;
    let mut disturbed_moved = 0u64;
    let mut n = 0u64;
    while n < 20_000 {
        n += 1;
        let others_done = failinject::others_done();
        let before = s03_memory::mallinfo2().uordblks;
        let live_before = failinject::live_bytes();
        let caller_before = selftest::caller_blocks();
        let hit = failinject::injected();
        failinject::fail_nth(n);
        let verdict = attempt();
        failinject::off();
        let injected = failinject::injected() != hit;
        let after = s03_memory::mallinfo2().uordblks;
        if failinject::others_begun() != others_done {
            disturbed += 1;
            disturbed_moved += u64::from(after != before);
            if disturbed > 1000 {
                return format!("{walker}: allocation {n}: other threads used the allocator in 1000 windows, none measured");
            }
            n -= 1;
            continue;
        }
        let failed = matches!(verdict, Verdict::Failed);
        if failed && failinject::live_bytes() != live_before {
            return format!("{walker}: allocation {n}: a partial result stays allocated on the global allocator ({live_before} -> {})", failinject::live_bytes());
        }
        if failed && selftest::caller_blocks() != caller_before {
            return format!("{walker}: allocation {n}: a caller-owned block stays allocated ({caller_before} -> {})", selftest::caller_blocks());
        }
        // Only after a reported failure: a success hands the caller its result block, which with
        // the exports interposed is a block of the Liella heap that the caller owns.
        if failed && after != before {
            return format!("{walker}: allocation {n}: a partial result stays allocated ({before} -> {after})");
        }
        if !injected {
            // The walk made fewer than n allocations: it must have succeeded completely.
            if !matches!(verdict, Verdict::Complete) {
                return format!("{walker}: no injection at {n}, yet the result is not the complete success ({verdict:?})");
            }
            if failures == 0 {
                eprintln!("NOT TESTABLE YET: {walker} makes no global-allocator allocation (C heap only)");
            } else {
                eprintln!("{walker}: {failures} allocation failures injected, all reported as documented ({absorbed} absorbed with the complete result; {disturbed} windows measured again after another thread used the allocator, {disturbed_moved} of them with the C heap moved)");
            }
            return String::new();
        }
        match verdict {
            Verdict::Failed => failures += 1,
            Verdict::Unsorted => return format!("{walker}: allocation {n} failed: complete but out of order, reported as success"),
            // The failed allocation was absorbed (a fallback): the result is still the complete one.
            Verdict::Complete => absorbed += 1,
            Verdict::Truncated => return format!("{walker}: allocation {n} failed: truncated success"),
            Verdict::BadFailure => return format!("{walker}: allocation {n} failed: failure without the documented ENOMEM"),
        }
    }
    format!("{walker}: more than 20000 allocations")
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

// FS-ORDER: entries are sorted by an explicit alphasort comparator, so the check is independent of readdir order

fn case_scandir(dir: &std::path::Path) -> String {
    let d = cstr(dir);
    let want: Vec<CString> = (0..FILES).map(|i| CString::new(name(i)).unwrap()).collect();
    let cmp: liella_abi::callback::SlotCompareFn = by_name;
    sweep("scandir", || {
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
        // The list is kept: in a hosted test build its blocks are the host C library's, which
        // `opaque::free` rejects (as in failinject.rs).
        if ok { Verdict::Complete } else { Verdict::Truncated }
    })
}

fn case_glob(dir: &std::path::Path) -> String {
    let pat = CString::new(format!("{}/f*", dir.display())).unwrap();
    let prefix = format!("{}/", dir.display());
    let want: Vec<CString> = (0..FILES).map(|i| CString::new(format!("{prefix}{}", name(i))).unwrap()).collect();
    sweep("glob", || {
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
    // The children's `fts_path` is not set while sorting: compare the inline `fts_name`.
    let name = core::mem::offset_of!(FTSENT, fts_name);
    cmp_text(ea + name, eb + name)
}

// FS-ORDER: entries are sorted by an explicit path comparator and matched as a set (presence flags), not by readdir order

fn case_fts(dir: &std::path::Path) -> String {
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
    let mut present = vec![false; want.len()];
    sweep("fts", || {
        errno::set_raw(0);
        present.iter_mut().for_each(|b| *b = false);
        let mut dup = false;
        // FTS_PHYSICAL | FTS_NOCHDIR
        let t = x::fts_open(argv.as_ptr(), 0x10 | 0x04, Some(cmp));
        if t.is_null() {
            return failed_or(false, enomem());
        }
        let (mut seen, mut ok) = (0usize, true);
        let (failed, bad);
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
                // Membership (no allocation): the path is some wanted path, once.
                match want.iter().position(|w| is_text(path, w.to_bytes())) {
                    Some(k) if !present[k] => present[k] = true,
                    _ => dup = true,
                }
                seen += 1;
            }
        }
        x::fts_close(t);
        if bad {
            Verdict::BadFailure
        } else if failed {
            Verdict::Failed
        } else if seen == FILES + 3 && !dup && present.iter().all(|b| *b) {
            // Complete. An allocation failure while sorting a directory's entries leaves them
            // in directory order with the walk reporting success: the spec says nothing about it
            // (see the hand-back); `ok` is the ordered case.
            if ok { Verdict::Complete } else { Verdict::Unsorted }
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

// FS-ORDER: only the count of visited entries and the failure mode are checked, never their order

fn case_nftw(dir: &std::path::Path) -> String {
    let d = cstr(dir);
    sweep("nftw", || {
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

fn case_realpath(dir: &std::path::Path) -> String {
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let given = CString::new(format!("{}/./sub/../{}", dir.display(), name(7))).unwrap();
    let want = CString::new(format!("{}/{}", dir.display(), name(7))).unwrap();
    sweep("realpath", || {
        errno::set_raw(0);
        let p = x::realpath(given.as_ptr(), null_mut());
        if p.is_null() {
            return failed_or(false, enomem());
        }
        let ok = is_text(p as usize, want.to_bytes());
        // `p` is kept: a host C library block (see the scandir case).
        if ok { Verdict::Complete } else { Verdict::Truncated }
    })
}

fn case_canonicalize(dir: &std::path::Path) -> String {
    std::fs::create_dir_all(dir.join("sub")).unwrap();
    let given = CString::new(format!("{}/./sub/../{}", dir.display(), name(7))).unwrap();
    let want = CString::new(format!("{}/{}", dir.display(), name(7))).unwrap();
    sweep("canonicalize", || {
        errno::set_raw(0);
        let p = x::canonicalize_file_name(given.as_ptr());
        if p.is_null() {
            return failed_or(false, enomem());
        }
        let ok = is_text(p as usize, want.to_bytes());
        // `p` is kept: a host C library block (see the scandir case).
        if ok { Verdict::Complete } else { Verdict::Truncated }
    })
}

fn case_getcwd(dir: &std::path::Path) -> String {
    std::env::set_current_dir(dir).unwrap();
    let want = CString::new(dir.as_os_str().as_encoded_bytes()).unwrap();
    sweep("getcwd", || {
        errno::set_raw(0);
        let p = x::getcwd(null_mut(), 0);
        if p.is_null() {
            return failed_or(false, enomem());
        }
        let ok = is_text(p as usize, want.to_bytes());
        // `p` is kept: a host C library block (see the scandir case).
        if ok { Verdict::Complete } else { Verdict::Truncated }
    })
}

fn case_current_dir_name(dir: &std::path::Path) -> String {
    std::env::set_current_dir(dir).unwrap();
    let want = CString::new(dir.as_os_str().as_encoded_bytes()).unwrap();
    sweep("current_dir_name", || {
        errno::set_raw(0);
        let p = x::get_current_dir_name();
        if p.is_null() {
            return failed_or(false, enomem());
        }
        let ok = is_text(p as usize, want.to_bytes());
        // `p` is kept: a host C library block (see the scandir case).
        if ok { Verdict::Complete } else { Verdict::Truncated }
    })
}

/// Build the tree, run the case in this process; any message is a failure.
fn drive(test: &str, case: fn(&std::path::Path) -> String) {
    let _cwd = CWD.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = build_tree(test);
    let msg = case(&dir);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(msg.is_empty(), "{test}: {msg}");
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
