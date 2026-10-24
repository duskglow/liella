// Specified in part from spec §29 (phase 15: s29_3_c_0, s29_4_c_3 by LEGACY-STUBS).
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §29 (8 clauses).
//!
//! §29.4's malloc clauses (LG-11) run through the exports of `liella-boundary`; the STREAMS and
//! removed-syscall clauses (LEGACY-STUBS) run through `liella_boundary::posix::s29_legacy`; the
//! RPC/XDR and NIS clauses are pending.

#![allow(clippy::unwrap_used, missing_docs)]
#![no_builtins] // calls to libc names (memset, memcmp) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
// opaque-ok: these tests exercise the exported malloc/free family itself (LG-11); the optimizer must not drop the calls, and the blocks are checked through the boundary's memcmp

use liella_abi::ctypes::c_void;
use liella_boundary::posix::s03_memory as m;
use liella_boundary::posix::s04_strings as st;
use core::ptr::{null, null_mut};

use liella::error::Errno;
use liella_boundary::errno;
use liella_boundary::posix::s29_legacy as l;
use spectest::clause;

/// -1 with `errno` `e`.
fn fails(r: i32, e: Errno) -> bool {
    r == -1 && errno::get() == e.get()
}

clause!(s29_1_c_1, "29.1", "Contract", r##"**XDR** serializes C data to and from the External Data Representation (RFC 4506): encode, decode and free, for primitive types, arrays, strings, unions, pointers, and record-marking streams."##);
clause!(s29_1_c_2, "29.1", "Contract", r##"**The RPC runtime** provides client handles over UDP, TCP and Unix sockets, server dispatch, portmapper registration, and authentication flavours (`AUTH_NONE`, `AUTH_UNIX`/`AUTH_SYS`, `AUTH_DES`)."##);
clause!(s29_2_c_0, "29.2", "Contract", r##"Client functions for Sun's Network Information Service (`yp_match`, `yp_first`, `yp_next`, `yp_all`, `yp_order`, `yp_master`, `yp_get_default_domain`, `yp_bind`, ...) and for NIS+."##);
#[test]
fn s29_3_c_0() {
    // As the glibc 2.39 probe (spec/behaviour/compat.md): the STREAMS calls fail with ENOSYS
    // whatever the arguments; isastream is 0 for an open descriptor (errno untouched) and EBADF
    // for any other.
    let f = std::fs::File::open("/dev/null").unwrap();
    let fd = std::os::fd::AsRawFd::as_raw_fd(&f);
    errno::set_raw(12345);
    assert_eq!((l::liella_compat_isastream(fd), errno::get()), (0, 12345), "an open descriptor");
    assert!(fails(l::liella_compat_isastream(-1), Errno::EBADF));
    assert!(fails(l::liella_compat_isastream(1 << 30), Errno::EBADF), "no such descriptor");
    let (mut flags, mut band) = (0, 0);
    for bad in [fd, -1] {
        assert!(fails(l::liella_compat_fattach(bad, c"/nonexistent".as_ptr()), Errno::ENOSYS));
        assert!(fails(l::liella_compat_getmsg(bad, null_mut(), null_mut(), &raw mut flags), Errno::ENOSYS));
        assert!(fails(l::liella_compat_putmsg(bad, null(), null(), 0), Errno::ENOSYS));
        assert!(fails(l::liella_compat_getpmsg(bad, null_mut(), null_mut(), &raw mut band, &raw mut flags), Errno::ENOSYS));
        assert!(fails(l::liella_compat_putpmsg(bad, null(), null(), 0, 0), Errno::ENOSYS));
    }
    assert!(fails(l::liella_compat_fattach(fd, null()), Errno::ENOSYS));
    assert!(fails(l::liella_compat_fdetach(c"/nonexistent".as_ptr()), Errno::ENOSYS));
    assert!(fails(l::liella_compat_fdetach(null()), Errno::ENOSYS));
}
clause!(s29_4_c_0, "29.4", "Contract", r##"Each keeps the contract it had when it was removed. Most are aliases of current functions, or stubs that fail with `ENOSYS`. Three are special:"##);
/// s29_4_c_1: `__libc_freeres()` releases libc's internal allocations so that leak checkers report
/// cleanly. Liella's `__libc_freeres` is inert (coordinator ruling #1317, differences row
/// `__libc_freeres`, compat flag COMPAT-11 registered): it releases nothing, so the call returns,
/// may be repeated, and leaves the library working (no state is torn down that a later call could
/// touch). A leak checker may report Liella's internal allocations as still reachable.
#[test]
fn s29_4_c_1() {
    let live = m::malloc(48);
    assert!(!live.is_null());
    st::memset(live, 0x33, 48);
    errno::set_raw(12345);
    m::__libc_freeres();
    m::__libc_freeres();
    assert_eq!(errno::get(), 12345, "an inert __libc_freeres leaves errno alone");
    // a block live across the call is untouched: nothing is released
    let want = [0x33u8; 48];
    assert_eq!(st::memcmp(live, want.as_ptr().cast::<c_void>(), 48), 0, "a live block survives __libc_freeres");
    let p = m::malloc(64);
    assert!(!p.is_null(), "the heap works after __libc_freeres");
    st::memset(p, 0x5a, 64);
    m::free(p);
    m::free(live);
    m::__libc_freeres();
    let q = m::__libc_malloc(8);
    assert!(!q.is_null());
    m::__libc_free(q);
}
/// s29_4_c_2: `__libc_malloc` and friends are aliases of `malloc` (the same heap and contracts)
/// that cannot be interposed: each is its own export that calls the heap directly, never through
/// the `malloc` name (`crates/boundary/src/posix/s03_memory.rs`), so a program's own `malloc`
/// does not reach them. Blocks cross freely between the two families.
#[test]
fn s29_4_c_2() {
    // __libc_malloc's block is an ordinary heap block: free() takes it.
    let p = m::__libc_malloc(40);
    assert!(!p.is_null());
    assert!(m::malloc_usable_size(p) >= 40);
    st::memset(p, 0x11, 40);
    m::free(p);
    // malloc's block goes back through __libc_free.
    let p = m::malloc(24);
    assert!(!p.is_null());
    m::__libc_free(p);
    // __libc_calloc zeroes; __libc_realloc keeps the contents.
    let zero = [0u8; 100];
    let p = m::__libc_calloc(10, 10);
    assert!(!p.is_null());
    assert_eq!(st::memcmp(p, zero.as_ptr().cast::<c_void>(), 100), 0, "__libc_calloc zeroes the block");
    st::memset(p, 0x7e, 100);
    let r = m::__libc_realloc(p, 4000);
    assert!(!r.is_null());
    let mark = [0x7eu8; 100];
    assert_eq!(st::memcmp(r, mark.as_ptr().cast::<c_void>(), 100), 0, "__libc_realloc keeps the contents");
    m::free(r);
    // The aligned forms: __libc_memalign, __libc_valloc and __libc_pvalloc (whole pages).
    let a = m::__libc_memalign(256, 10);
    assert!(!a.is_null() && a.addr() % 256 == 0);
    m::__libc_free(a);
    let v = m::__libc_valloc(10);
    assert!(!v.is_null() && v.addr() % 4096 == 0);
    m::free(v);
    let pv = m::__libc_pvalloc(10);
    assert!(!pv.is_null() && pv.addr() % 4096 == 0 && m::malloc_usable_size(pv) >= 4096);
    m::free(pv);
}
#[test]
fn s29_4_c_3() {
    // sysctl and __sysctl fail with ENOSYS and leave the buffers alone (the glibc 2.39 probe).
    let mut name = [1, 1];
    let mut buf = [7u8; 16];
    let mut len = buf.len();
    for call in [l::liella_compat_sysctl, l::liella_compat___sysctl] {
        assert!(fails(call(name.as_mut_ptr(), 2, buf.as_mut_ptr().cast(), &raw mut len, null_mut(), 0), Errno::ENOSYS));
        assert!(fails(call(null_mut(), 0, null_mut(), null_mut(), null_mut(), 0), Errno::ENOSYS));
    }
    assert_eq!((len, buf), (16, [7u8; 16]));
}
