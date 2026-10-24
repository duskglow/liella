// Specified from spec §3 and its Appendix B list. Generated stubs were replaced by real tests of
// the SAME NAME (see crates/spectest); `cargo xtask stubs` now only checks coverage.

//! Translation-layer tests for the exported symbols of spec §3 (60 entries).
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! Memory the library hands out is written and read through the exported `memset`/`memcpy`,
//! never with `unsafe` here. The heap is process-wide, so tests that compare its statistics hold
//! [`GLOBAL`]. Aborts and exits run in a re-executed copy of this binary (`LIELLA_S03_PROBE`):
//! forking a multi-threaded test process and then allocating could deadlock.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(non_snake_case)]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, missing_docs)]

use core::ptr::null_mut;
use core::sync::atomic::Ordering::Relaxed;
use std::os::unix::process::ExitStatusExt;
use std::sync::{Mutex, MutexGuard};

use liella_abi::ctypes::{c_int, c_void};
use liella_abi::layout::{struct_obstack, struct_obstack__temp};
use liella_boundary::data;
use liella_boundary::errno;
use liella_boundary::posix::s03_memory as s;
use liella_boundary::posix::s04_strings as st;
use liella_boundary::posix::s12_stdio as x;
use liella_boundary::selftest::{self, opaque, ChildEnd};
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;

const ENOMEM: i32 = 12;
const EINVAL: i32 = 22;
const ENOSYS: i32 = 38;
const PAGE: usize = 4096;
const PROBE: &str = "LIELLA_S03_PROBE";

static GLOBAL: Mutex<()> = Mutex::new(());

fn global() -> MutexGuard<'static, ()> {
    GLOBAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

fn fill(p: *mut c_void, byte: u8, n: usize) {
    st::memset(p, c_int::from(byte), n);
}

fn bytes(p: *const c_void, n: usize) -> Vec<u8> {
    let mut v = vec![0u8; n];
    st::memcpy(v.as_mut_ptr().cast(), p, n);
    v
}

fn addr(p: *const c_void) -> usize {
    p as usize
}

/// The probe this process was re-executed for, if any.
fn probe() -> Option<String> {
    std::env::var(PROBE).ok()
}

/// Re-execute this test binary running only `test` with the probe `name` set.
fn run_probe(test: &str, name: &str) -> std::process::Output {
    std::process::Command::new(std::env::current_exe().unwrap())
        .args([test, "--exact", "--test-threads=1", "--nocapture"])
        .env(PROBE, name)
        .output()
        .unwrap()
}

fn stderr_has(o: &std::process::Output, text: &str) -> bool {
    String::from_utf8_lossy(&o.stderr).contains(text)
}

/// Run `body` alone in a re-executed copy of this binary (one test, one thread), so that heap
/// statistics see no other test's allocations. In the copy, `body` runs directly.
fn isolated(test: &str, body: impl FnOnce()) {
    if probe().as_deref() == Some(test) {
        body();
        return;
    }
    let o = run_probe(test, test);
    assert!(o.status.success(), "{test} in isolation: {}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

// ------------------------------------------------------------------------------ allocation

#[test]
fn sym_malloc() {
    let p = opaque::malloc(100);
    assert!(!p.is_null() && addr(p) % 16 == 0, "aligned for any fundamental type");
    assert!(s::malloc_usable_size(p) >= 100);
    fill(p, 0xAB, 100);
    assert_eq!(bytes(p, 100), vec![0xAB; 100]);
    let z1 = opaque::malloc(0);
    let z2 = opaque::malloc(0);
    assert!(!z1.is_null() && z1 != z2, "malloc(0) is a unique pointer (D-3.2)");
    opaque::free(z1);
    opaque::free(z2);
    let big = opaque::malloc(3 << 20);
    assert!(!big.is_null());
    fill(big, 1, 3 << 20);
    opaque::free(big);
    opaque::free(p);
    errno::set_raw(0);
    assert!(opaque::malloc(usize::MAX - 64).is_null());
    assert_eq!(errno::get(), ENOMEM);
}

#[test]
fn sym___libc_malloc() {
    let p = s::__libc_malloc(24);
    assert!(!p.is_null());
    s::__libc_free(p);
}

#[test]
fn sym_free() {
    match probe().as_deref() {
        Some("double") => {
            let q = opaque::malloc(40);
            opaque::free(q);
            liella_boundary::selftest::expect_crash();
            opaque::free(q);
            return;
        }
        Some("invalid") => {
            let q = opaque::malloc(40);
            liella_boundary::selftest::expect_crash();
            opaque::free(q.cast::<u8>().wrapping_add(8).cast());
            return;
        }
        _ => {}
    }
    opaque::free(null_mut());
    let p = opaque::malloc(40);
    errno::set_raw(77);
    opaque::free(p);
    assert_eq!(errno::get(), 77, "free never changes errno");
    let o = run_probe("sym_free", "double");
    assert_eq!(o.status.signal(), Some(6), "a double free aborts (S4)");
    assert!(stderr_has(&o, "free(): double free detected"));
    let o = run_probe("sym_free", "invalid");
    assert_eq!(o.status.signal(), Some(6));
    assert!(stderr_has(&o, "free(): invalid pointer"));
}

#[test]
fn sym___libc_free() {
    s::__libc_free(null_mut());
    s::__libc_free(opaque::malloc(1));
}

#[test]
fn sym_cfree() {
    s::cfree(null_mut());
    s::cfree(opaque::calloc(2, 3));
}

#[test]
fn sym_calloc() {
    for n in [1usize, 100, 5000, 70_000, 2 << 20] {
        let p = opaque::malloc(n);
        fill(p, 0xFF, n);
        opaque::free(p);
        let z = opaque::calloc(1, n);
        assert_eq!(bytes(z, n), vec![0; n], "{n} bytes zeroed even when reused");
        opaque::free(z);
    }
    errno::set_raw(0);
    assert!(opaque::calloc(usize::MAX / 2, 3).is_null(), "an overflowing product");
    assert_eq!(errno::get(), ENOMEM);
    let z = opaque::calloc(0, 9);
    assert!(!z.is_null());
    opaque::free(z);
}

#[test]
fn sym___libc_calloc() {
    let z = s::__libc_calloc(4, 8);
    assert_eq!(bytes(z, 32), vec![0; 32]);
    opaque::free(z);
}

#[test]
fn sym_realloc() {
    if probe().as_deref() == Some("stale") {
        let x = opaque::malloc(8);
        opaque::free(x);
        liella_boundary::selftest::expect_crash();
        opaque::realloc(x, 16);
        return;
    }
    let p = opaque::realloc(null_mut(), 10);
    assert!(!p.is_null(), "NULL is malloc");
    st::memcpy(p, b"0123456789".as_ptr().cast(), 10);
    let q = opaque::realloc(p, 100_000);
    assert_eq!(bytes(q, 10), b"0123456789", "contents survive growth");
    let r = opaque::realloc(q, 4);
    assert_eq!(bytes(r, 4), b"0123", "and shrinking");
    let big = opaque::realloc(r, 4 << 20);
    assert_eq!(bytes(big, 4), b"0123");
    let big2 = opaque::realloc(big, 9 << 20);
    assert_eq!(bytes(big2, 4), b"0123", "a large block grows by remapping");
    errno::set_raw(0);
    assert!(opaque::realloc(big2, usize::MAX - 64).is_null());
    assert_eq!(errno::get(), ENOMEM);
    assert_eq!(bytes(big2, 4), b"0123", "a failed realloc leaves the block intact");
    errno::set_raw(5);
    assert!(opaque::realloc(big2, 0).is_null(), "size 0 frees (D-3.2)");
    assert_eq!(errno::get(), 5, "without an error");
    let o = run_probe("sym_realloc", "stale");
    assert_eq!(o.status.signal(), Some(6));
    assert!(stderr_has(&o, "realloc(): double free detected"));
}

#[test]
fn sym___libc_realloc() {
    let p = s::__libc_realloc(null_mut(), 3);
    let q = s::__libc_realloc(p, 300);
    assert!(!q.is_null());
    opaque::free(q);
}

#[test]
fn sym_reallocarray() {
    let p = s::reallocarray(null_mut(), 10, 4);
    assert!(s::malloc_usable_size(p) >= 40);
    errno::set_raw(0);
    assert!(s::reallocarray(p, usize::MAX / 2, 3).is_null());
    assert_eq!(errno::get(), ENOMEM, "an overflowing product leaves the block intact");
    opaque::free(p);
}

// ------------------------------------------------------------------------------ alignment

#[test]
fn sym_aligned_alloc() {
    for a in [1usize, 8, 16, 64, 4096, 1 << 16, 1 << 21] {
        let p = opaque::aligned_alloc(a, 100);
        assert!(!p.is_null() && addr(p) % a == 0, "alignment {a}");
        opaque::free(p);
    }
    let p = opaque::aligned_alloc(64, 7);
    assert!(!p.is_null(), "any size is accepted (C17)");
    opaque::free(p);
    errno::set_raw(0);
    assert!(opaque::aligned_alloc(24, 48).is_null());
    assert_eq!(errno::get(), EINVAL, "not a power of two");
}

#[test]
fn sym_memalign() {
    let p = s::memalign(256, 10);
    assert_eq!(addr(p) % 256, 0);
    let q = s::memalign(24, 10);
    assert_eq!(addr(q) % 32, 0, "rounded up to a power of two");
    opaque::free(p);
    opaque::free(q);
}

#[test]
fn sym___libc_memalign() {
    let p = s::__libc_memalign(128, 1);
    assert_eq!(addr(p) % 128, 0);
    opaque::free(p);
}

#[test]
fn sym_posix_memalign() {
    let mut out: *mut c_void = null_mut();
    assert_eq!(opaque::posix_memalign(Some(&mut out), 64, 100), 0);
    assert!(!out.is_null() && addr(out) % 64 == 0);
    opaque::free(out);
    let sentinel = 0x1234usize as *mut c_void;
    let mut out = sentinel;
    errno::set_raw(0);
    assert_eq!(opaque::posix_memalign(Some(&mut out), 4, 10), EINVAL, "not a multiple of sizeof(void *)");
    assert_eq!(opaque::posix_memalign(Some(&mut out), 48, 10), EINVAL, "not a power of two");
    assert_eq!(opaque::posix_memalign(Some(&mut out), 64, usize::MAX - 64), ENOMEM);
    assert_eq!(out, sentinel, "untouched on failure");
    assert_eq!(errno::get(), 0, "the error is returned, errno unchanged");
}

#[test]
fn sym_valloc() {
    let p = s::valloc(10);
    assert_eq!(addr(p) % PAGE, 0);
    opaque::free(p);
}

#[test]
fn sym___libc_valloc() {
    let p = s::__libc_valloc(PAGE + 1);
    assert_eq!(addr(p) % PAGE, 0);
    opaque::free(p);
}

#[test]
fn sym_pvalloc() {
    let p = s::pvalloc(10);
    assert_eq!(addr(p) % PAGE, 0);
    assert!(s::malloc_usable_size(p) >= PAGE, "rounded to whole pages");
    let z = s::pvalloc(0);
    assert!(s::malloc_usable_size(z) >= PAGE, "at least one page");
    opaque::free(p);
    opaque::free(z);
}

#[test]
fn sym___libc_pvalloc() {
    let p = s::__libc_pvalloc(PAGE + 1);
    assert!(s::malloc_usable_size(p) >= 2 * PAGE);
    opaque::free(p);
}

// ------------------------------------------------------------------------------ introspection

#[test]
fn sym_malloc_usable_size() {
    assert_eq!(s::malloc_usable_size(null_mut()), 0);
    for n in [1usize, 17, 1000, 65_536, 65_537, 1 << 20] {
        let p = opaque::malloc(n);
        let u = s::malloc_usable_size(p);
        assert!(u >= n);
        fill(p, 7, u);
        opaque::free(p);
    }
}

#[test]
fn sym_malloc_trim() {
    isolated("sym_malloc_trim", || {
        let ps: Vec<_> = (0..200).map(|_| opaque::malloc(20_000)).collect();
        for p in ps {
            opaque::free(p);
        }
        assert_eq!(s::malloc_trim(0), 1, "empty slabs went back to the kernel");
        let r = s::malloc_trim(0);
        assert!(r == 0 || r == 1);
    });
}

#[test]
fn sym_mallopt() {
    for p in [-1, -2, -3, -6, -8] {
        assert_eq!(s::mallopt(p, 1), 1, "parameter {p} accepted (D-3.4)");
    }
    assert_eq!(s::mallopt(-4, 1), 1, "M_MMAP_MAX accepted and ignored (D-3.4)");
    assert_eq!(s::mallopt(12345, 1), 1, "an unknown parameter is accepted, as glibc does");
    assert_eq!(s::mallopt(1, 161), 0, "M_MXFAST above 160 is refused");
}

#[test]
fn sym___libc_mallopt() {
    assert_eq!(s::__libc_mallopt(-1, 1 << 20), 1);
}

#[test]
fn sym_mallinfo2() {
    isolated("sym_mallinfo2", || {
        let a = s::mallinfo2();
        let p = opaque::malloc(5 << 20);
        let b = s::mallinfo2();
        assert_eq!(b.hblks, a.hblks + 1, "one more large block");
        assert!(b.hblkhd >= a.hblkhd + (5 << 20));
        assert!(b.uordblks >= a.uordblks + (5 << 20), "bytes in use");
        assert_eq!((b.ordblks, b.smblks, b.usmblks, b.fsmblks), (0, 0, 0, 0), "arena-specific fields are 0");
        opaque::free(p);
        let c = s::mallinfo2();
        // The process is not quiescent (the harness thread and the interposed runtime may allocate a
        // few bytes between two calls), so "no longer in use" is checked as: the 5 MiB block left the
        // in-use count, and what remains differs from the start by far less than the block.
        const NOISE: u64 = 64 << 10;
        assert!(c.uordblks.abs_diff(a.uordblks) <= NOISE, "no longer in use: {} vs {}", c.uordblks, a.uordblks);
        assert!(b.uordblks.saturating_sub(c.uordblks) + NOISE >= 5 << 20, "the freed block left the in-use count");
        // The thread keeps the block for reuse (D-3.13): still mapped until a trim.
        assert_eq!(c.hblks, a.hblks + 1);
        assert_eq!(s::malloc_trim(0), 1);
        let d = s::mallinfo2();
        assert_eq!(d.hblks, a.hblks);
        assert!(d.uordblks.abs_diff(a.uordblks) <= NOISE, "in use after trim: {} vs {}", d.uordblks, a.uordblks);
    });
}

#[test]
fn sym_mallinfo() {
    isolated("sym_mallinfo", || {
        let m = s::mallinfo();
        let m2 = s::mallinfo2();
        assert_eq!(i64::from(m.uordblks), i64::try_from(m2.uordblks).unwrap().min(i64::from(i32::MAX)));
        assert!(m.hblks >= 0 && m.arena >= 0, "saturated, never negative");
    });
}

#[test]
fn sym___libc_mallinfo() {
    isolated("sym___libc_mallinfo", || {
        assert_eq!(s::__libc_mallinfo().ordblks, 0);
    });
}

#[test]
fn sym_malloc_stats() {
    if probe().as_deref() == Some("stats") {
        let p = opaque::malloc(1 << 20);
        s::malloc_stats();
        opaque::free(p);
        return;
    }
    let o = run_probe("sym_malloc_stats", "stats");
    assert!(o.status.success());
    for line in ["Total (incl. mmap):", "system bytes", "in use bytes", "max mmap regions"] {
        assert!(stderr_has(&o, line), "{line}");
    }
}

#[test]
fn sym_malloc_info() {
    let mut buf = vec![0u8; 8192];
    let fp = x::fmemopen(buf.as_mut_ptr().cast(), buf.len(), c"w".as_ptr());
    assert!(!fp.is_null());
    let p = opaque::malloc(1 << 20);
    assert_eq!(s::malloc_info(0, fp), 0);
    assert_eq!(x::fflush(fp), 0);
    let text = String::from_utf8_lossy(&buf[..buf.iter().position(|&b| b == 0).unwrap()]).into_owned();
    assert!(text.starts_with("<malloc version=\"1\">\n") && text.ends_with("</malloc>\n"), "{text}");
    let mmap = text.lines().find(|l| l.starts_with("<total type=\"mmap\"")).unwrap();
    assert!(!mmap.contains("count=\"0\""), "the 1 MiB block is a mapping: {mmap}");
    assert_eq!(text.lines().filter(|l| l.starts_with("<heap ")).count(), 1, "one heap (D-3.4)");
    errno::set_raw(0);
    assert_eq!(s::malloc_info(-1, fp), -1);
    assert_eq!(errno::get(), EINVAL, "malloc_info(3): EINVAL for options other than 0");
    assert_eq!(x::fclose(fp), 0);
    opaque::free(p);
}

#[test]
fn sym___libc_freeres() {
    // inert (ruling #1317): errno is left alone
    errno::set_raw(12345);
    s::__libc_freeres();
    assert_eq!(errno::get(), 12345, "__libc_freeres leaves errno alone");
    opaque::free(opaque::malloc(1));
}

#[test]
fn sym_malloc_get_state() {
    errno::set_raw(0);
    assert!(s::malloc_get_state().is_null());
    assert_eq!(errno::get(), ENOSYS, "a compat stub that fails (D-3.5)");
}

#[test]
fn sym_malloc_set_state() {
    errno::set_raw(0);
    assert_eq!(s::malloc_set_state(null_mut()), -1);
    assert_eq!(errno::get(), ENOSYS);
}

#[test]
fn sym___default_morecore() {
    // Compat-only (`__default_morecore@GLIBC_2.2.5`, LG-11): the translation under its internal name.
    assert!(s::liella_compat___default_morecore(4096).is_null(), "the heap never uses the break (D-3.7)");
    assert!(s::liella_compat___default_morecore(0).is_null());
}

// ------------------------------------------------------------------------------ debugging

extern "C" fn no_abort(_: c_int) {}

#[test]
fn sym_mcheck() {
    assert_eq!(s::mcheck(Some(no_abort)), -1, "after the process's first allocation: too late, checking stays off");
    assert_eq!(s::mcheck(None), -1);
}

#[test]
fn sym_mcheck_pedantic() {
    assert_eq!(s::mcheck_pedantic(None), -1);
}

#[test]
fn sym_mcheck_check_all() {
    s::mcheck_check_all();
}

#[test]
fn sym_mprobe() {
    let p = opaque::malloc(8);
    assert_eq!(s::mprobe(p), -1, "MCHECK_DISABLED");
    opaque::free(p);
}

#[test]
fn sym_mtrace() {
    s::mtrace();
    opaque::free(opaque::malloc(3));
    s::muntrace();
}

#[test]
fn sym_muntrace() {
    s::muntrace();
}

#[test]
fn sym_tr_break() {
    // Compat-only (`tr_break@GLIBC_2.2.5`, LG-11): a no-op mtrace breakpoint.
    errno::set_raw(12345);
    s::liella_compat_tr_break();
    assert_eq!(errno::get(), 12345, "tr_break does nothing, errno included");
}

// ------------------------------------------------------------------------------ inert data

fn inert(cell: &core::sync::atomic::AtomicUsize) {
    let _g = global();
    assert_eq!(cell.load(Relaxed), 0, "initially NULL");
    cell.store(0xDEAD_0000, Relaxed);
    let p = opaque::malloc(64);
    opaque::free(opaque::realloc(p, 128));
    opaque::free(s::memalign(64, 8));
    cell.store(0, Relaxed);
}

#[test]
fn sym___malloc_hook() {
    inert(&data::__malloc_hook);
}

#[test]
fn sym___free_hook() {
    inert(&data::__free_hook);
}

#[test]
fn sym___realloc_hook() {
    inert(&data::__realloc_hook);
}

#[test]
fn sym___memalign_hook() {
    inert(&data::__memalign_hook);
}

#[test]
fn sym___after_morecore_hook() {
    inert(&data::__after_morecore_hook);
}

#[test]
fn sym___malloc_initialize_hook() {
    inert(&data::__malloc_initialize_hook);
}

#[test]
fn sym___morecore() {
    inert(&data::__morecore);
}

#[test]
fn sym_mallwatch() {
    inert(&data::mallwatch);
}

#[test]
fn sym__obstack() {
    inert(&data::_obstack);
}

// ------------------------------------------------------------------------------ program break

fn break_moves(_: &CSys) -> i32 {
    let old = s::sbrk(0) as usize;
    if s::sbrk(8192) as usize != old || s::sbrk(0) as usize != old + 8192 {
        return 1;
    }
    if data::__curbrk.load(Relaxed) != old + 8192 {
        return 2;
    }
    if s::sbrk(-8192) as usize != old + 8192 || s::sbrk(0) as usize != old {
        return 3;
    }
    if s::brk((old + 4096) as *mut c_void) != 0 || s::sbrk(0) as usize != old + 4096 {
        return 4;
    }
    errno::set_raw(0);
    if s::sbrk(isize::MAX) as isize != -1 || errno::get() != ENOMEM {
        return 5;
    }
    errno::set_raw(0);
    if s::brk(usize::MAX as *mut c_void) != -1 || errno::get() != ENOMEM {
        return 6;
    }
    0
}

fn break_moves_dunder(_: &CSys) -> i32 {
    let old = s::__sbrk(0) as usize;
    i32::from(s::__sbrk(4096) as usize != old || s::__sbrk(0) as usize != old + 4096)
}

fn break_recorded(_: &CSys) -> i32 {
    i32::from(s::sbrk(0) as usize != data::__curbrk.load(Relaxed))
}

// The break moves in a forked child only: the child allocates nothing, and in the test process
// the break belongs to the harness's own allocator.
#[test]
fn sym_sbrk() {
    assert_eq!(selftest::in_child(break_moves).unwrap(), ChildEnd::Exited(0));
}

#[test]
fn sym___sbrk() {
    assert_eq!(selftest::in_child(break_moves_dunder).unwrap(), ChildEnd::Exited(0));
}

#[test]
fn sym_brk() {
    assert_eq!(selftest::in_child(break_moves).unwrap(), ChildEnd::Exited(0));
}

#[test]
fn sym___curbrk() {
    assert_eq!(selftest::in_child(break_recorded).unwrap(), ChildEnd::Exited(0), "sbrk records the break");
}

// ------------------------------------------------------------------------------ obstacks

extern "C" fn chunk_malloc(n: usize) -> *mut c_void {
    opaque::malloc(n)
}

extern "C" fn chunk_free(p: *mut c_void) {
    opaque::free(p);
}

extern "C" fn chunk_malloc_arg(arg: *mut c_void, n: usize) -> *mut c_void {
    if arg as usize != 0xA5A5 {
        CSys.exit_group(70);
    }
    opaque::malloc(n)
}

extern "C" fn chunk_free_arg(arg: *mut c_void, p: *mut c_void) {
    if arg as usize != 0xA5A5 {
        CSys.exit_group(71);
    }
    opaque::free(p);
}

extern "C" fn chunk_fail(_: usize) -> *mut c_void {
    null_mut()
}

fn obstack() -> struct_obstack {
    struct_obstack {
        chunk_size: 0,
        chunk: 0,
        object_base: 0,
        next_free: 0,
        chunk_limit: 0,
        temp: struct_obstack__temp { tempint: 0 },
        alignment_mask: 0,
        chunkfun: 0,
        freefun: 0,
        extra_arg: 0,
        use_extra_arg: [0],
    }
}

fn begin(h: &mut struct_obstack, size: c_int) {
    assert_eq!(s::_obstack_begin(Some(h), size, 0, chunk_malloc as *const () as usize, chunk_free as *const () as usize), 1);
}

/// `obstack_grow(h, data, n)` as the `<obstack.h>` macro does it.
fn grow(h: &mut struct_obstack, data: &[u8]) {
    if h.chunk_limit - h.next_free < data.len() {
        s::_obstack_newchunk(Some(h), data.len() as c_int);
    }
    st::memcpy(h.next_free as *mut c_void, data.as_ptr().cast(), data.len());
    h.next_free += data.len();
}

/// `obstack_finish(h)` as the macro does it.
fn finish(h: &mut struct_obstack) -> usize {
    let m = h.alignment_mask as usize;
    let obj = h.object_base;
    h.next_free = ((h.next_free + m) & !m).min(h.chunk_limit);
    h.object_base = h.next_free;
    obj
}

#[test]
fn sym__obstack_begin() {
    let mut h = obstack();
    begin(&mut h, 0);
    assert_eq!(h.chunk_size, 4064, "0 selects the default chunk size");
    assert_eq!(h.alignment_mask, 15, "and the default alignment");
    assert_eq!(h.object_base % 16, 0);
    assert_eq!(h.object_base, h.next_free, "an empty object");
    assert!(h.chunk_limit > h.next_free);
    s::obstack_free(Some(&mut h), null_mut());
    let mut h = obstack();
    assert_eq!(s::_obstack_begin(Some(&mut h), 200, 64, chunk_malloc as *const () as usize, chunk_free as *const () as usize), 1);
    assert_eq!((h.chunk_size, h.alignment_mask, h.object_base % 64), (200, 63, 0));
    s::obstack_free(Some(&mut h), null_mut());
}

#[test]
fn sym__obstack_begin_1() {
    let mut h = obstack();
    assert_eq!(s::_obstack_begin_1(Some(&mut h), 64, 0, chunk_malloc_arg as *const () as usize, chunk_free_arg as *const () as usize, 0xA5A5 as *mut c_void), 1);
    assert_eq!(h.use_extra_arg[0] & 1, 1);
    grow(&mut h, &[9; 500]);
    s::obstack_free(Some(&mut h), null_mut());
    assert_eq!(h.chunk, 0);
}

#[test]
fn sym__obstack_newchunk() {
    let mut h = obstack();
    begin(&mut h, 64);
    let first = h.chunk;
    grow(&mut h, b"hello ");
    grow(&mut h, &[b'x'; 300]);
    assert_ne!(h.chunk, first, "a new chunk");
    let obj = finish(&mut h);
    let got = bytes(obj as *const c_void, 306);
    assert_eq!(&got[..6], b"hello ", "the growing object moved with its contents");
    assert!(got[6..].iter().all(|&b| b == b'x'));
    s::obstack_free(Some(&mut h), null_mut());
}

#[test]
fn sym__obstack_allocated_p() {
    let mut h = obstack();
    begin(&mut h, 64);
    grow(&mut h, b"abc");
    let a = finish(&mut h);
    grow(&mut h, &[1; 400]);
    let b = finish(&mut h);
    assert_eq!(s::_obstack_allocated_p(Some(&mut h), a as *mut c_void), 1);
    assert_eq!(s::_obstack_allocated_p(Some(&mut h), b as *mut c_void), 1);
    let other = opaque::malloc(8);
    assert_eq!(s::_obstack_allocated_p(Some(&mut h), other), 0);
    opaque::free(other);
    s::obstack_free(Some(&mut h), null_mut());
}

#[test]
fn sym_obstack_free() {
    if probe().as_deref() == Some("foreign") {
        let mut h = obstack();
        begin(&mut h, 64);
        liella_boundary::selftest::expect_crash();
        s::obstack_free(Some(&mut h), 0x10 as *mut c_void);
        return;
    }
    let mut h = obstack();
    begin(&mut h, 64);
    grow(&mut h, b"keep");
    let keep = finish(&mut h);
    grow(&mut h, &[2; 1000]);
    let _dropped = finish(&mut h);
    s::obstack_free(Some(&mut h), keep as *mut c_void);
    assert_eq!((h.object_base, h.next_free), (keep, keep), "back to the freed object");
    assert_eq!(s::_obstack_allocated_p(Some(&mut h), keep as *mut c_void), 1, "its chunk survives");
    s::obstack_free(Some(&mut h), null_mut());
    let o = run_probe("sym_obstack_free", "foreign");
    assert_eq!(o.status.signal(), Some(6), "an object in no chunk aborts");
}

#[test]
fn sym__obstack_free() {
    let mut h = obstack();
    begin(&mut h, 0);
    grow(&mut h, b"z");
    s::_obstack_free(Some(&mut h), null_mut());
    assert_eq!(h.chunk, 0);
}

#[test]
fn sym__obstack_memory_used() {
    let mut h = obstack();
    begin(&mut h, 256);
    assert_eq!(s::_obstack_memory_used(Some(&mut h)), 256);
    grow(&mut h, &[0; 600]);
    assert!(s::_obstack_memory_used(Some(&mut h)) > 600);
    s::obstack_free(Some(&mut h), null_mut());
}

extern "C" fn exit_9() {
    CSys.exit_group(9);
}

#[test]
fn sym_obstack_alloc_failed_handler() {
    match probe().as_deref() {
        Some("handler") => {
            data::obstack_alloc_failed_handler.store(exit_9 as *mut (), Relaxed);
            let mut h = obstack();
            s::_obstack_begin(Some(&mut h), 0, 0, chunk_fail as *const () as usize, chunk_free as *const () as usize);
            return;
        }
        Some("default") => {
            let mut h = obstack();
            s::_obstack_begin(Some(&mut h), 0, 0, chunk_fail as *const () as usize, chunk_free as *const () as usize);
            return;
        }
        _ => {}
    }
    let o = run_probe("sym_obstack_alloc_failed_handler", "default");
    assert_eq!(o.status.code(), Some(1), "the default handler exits with obstack_exit_failure");
    assert!(stderr_has(&o, "memory exhausted"));
    let o = run_probe("sym_obstack_alloc_failed_handler", "handler");
    assert_eq!(o.status.code(), Some(9), "a replaced handler runs");
}

#[test]
fn sym_obstack_exit_failure() {
    if probe().as_deref() == Some("status") {
        data::obstack_exit_failure.store(42, Relaxed);
        let mut h = obstack();
        begin(&mut h, 64);
        h.chunkfun = chunk_fail as *const () as usize;
        // An object that fills the chunk: _obstack_newchunk cannot get another.
        h.next_free = h.chunk_limit;
        s::_obstack_newchunk(Some(&mut h), 10);
        return;
    }
    assert_eq!(data::obstack_exit_failure.load(Relaxed), 1);
    let o = run_probe("sym_obstack_exit_failure", "status");
    assert_eq!(o.status.code(), Some(42));
}

// ------------------------------------------------------------------------------ page source

/// Free bytes on NUMA node `n` (`/sys/devices/system/node/nodeN/meminfo`); `None` when unreadable.
fn node_free_bytes(n: u32) -> Option<usize> {
    let text = std::fs::read_to_string(format!("/sys/devices/system/node/node{n}/meminfo")).ok()?;
    let line = text.lines().find(|l| l.contains("MemFree:"))?;
    let kb: usize = line.split_whitespace().rev().nth(1)?.parse().ok()?;
    Some(kb * 1024)
}

#[test]
fn kernel_pages_align_and_prefer_a_node() {
    // Trace: no spec clause (the heap's kernel page source); D-3.10, M3.
    use liella_boundary::heap::KernelPages;
    use liella_rt::heap::{Pages, SEG};
    let pages = KernelPages;
    let a = pages.map(SEG, SEG, Some(0)).unwrap();
    assert_eq!(a % SEG, 0, "aligned as asked (segments are found by address, D-3.10)");
    fill(a as *mut c_void, 7, SEG);
    // Liella controls the policy, not the placement. Kernel doc (admin-guide/mm/numa_memory_policy):
    // MPOL_PREFERRED "attempts to allocate pages from the preferred node, but will fall back to
    // other nodes" - the kernel also weighs zone watermarks, reclaim and page-cache state, so a page
    // can land elsewhere with free memory on node 0 (FLAKE-NUMA-NODE, seen twice). Asserted: the mode
    // and mask via get_mempolicy. The placement is only printed as a diagnostic.
    match selftest::policy_of(a) {
        Ok((mode, mask)) => {
            assert_eq!(mode, liella_kernel::uapi::MPOL_PREFERRED as i32, "the preferred-node policy was applied");
            assert_eq!(mask & 1, 1, "and it names node 0");
            eprintln!(
                "diagnostic: page on node {:?}, node 0 MemFree {:?}",
                selftest::node_of(a).ok(),
                node_free_bytes(0)
            );
        }
        Err(e) => assert_eq!(e, liella_kernel::Errno::ENOSYS, "only a kernel without CONFIG_NUMA may refuse"),
    }
    let b = pages.map(3 * PAGE, PAGE, None).unwrap();
    assert_eq!(bytes(b as *const c_void, 3 * PAGE), vec![0; 3 * PAGE], "fresh mappings read as zero");
    pages.unmap(a, SEG);
    pages.unmap(b, 3 * PAGE);
}
