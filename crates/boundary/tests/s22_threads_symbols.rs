// Specified from spec §22 (phase 6; `pthread_sigmask` in phase 5); the libthread_db entries (td_*) are phase 14.
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Translation-layer tests for the exported symbols of spec §22 (239 entries).
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! Thread creation, cancellation and everything that needs a thread descriptor are exports of
//! the products only (this host-started process keeps the host's threads): those symbols are
//! checked through the C conformance programs that call them, statically linked against
//! `libc.a`, and through their versioned entry in `libc.so.6`. The objects and attributes, which
//! touch only caller memory, are also called in process.
#![allow(non_snake_case)]
#![allow(clippy::unwrap_used, clippy::indexing_slicing, missing_docs)]
#![allow(unused_imports)]

use liella_boundary::errno;
use std::sync::atomic::{AtomicUsize, Ordering};
use liella_boundary::posix::s20_signals as sig;
use spectest::symbol;

use liella_abi::layout::{
    cnd_t, cpu_set_t, mtx_t, pthread_attr_t, pthread_barrier_t, pthread_barrierattr_t, pthread_cond_t, pthread_condattr_t, pthread_mutex_t,
    pthread_mutexattr_t, pthread_rwlock_t, pthread_rwlockattr_t, pthread_spinlock_t, pthread_t, sem_t, sigset_t, struct_sched_param, thrd_t,
};
use liella_boundary::posix::s22_threads as t;

mod products;
use products::{archived, compat_only, conformance, exported, loader_owned};

const EINVAL: i32 = 22;
const EBUSY: i32 = 16;
const EAGAIN: i32 = 11;
const ENOTSUP: i32 = 95;

// In-process translation checks for the functions that touch only caller memory: the objects
// and attributes need no thread descriptor, so they run in this (host-started) process.

fn attr() -> pthread_attr_t {
    let mut a = pthread_attr_t { __size: [0; 56] };
    assert_eq!(t::pthread_attr_init(&mut a), 0);
    a
}

fn attr_roundtrip() {
    let mut a = attr();
    let mut v = -1;
    assert_eq!(t::pthread_attr_getdetachstate(&a, Some(&mut v)), 0);
    assert_eq!(v, 0, "PTHREAD_CREATE_JOINABLE");
    assert_eq!(t::pthread_attr_setdetachstate(&mut a, 1), 0);
    t::pthread_attr_getdetachstate(&a, Some(&mut v));
    assert_eq!(v, 1);
    assert_eq!(t::pthread_attr_setdetachstate(&mut a, 7), EINVAL);
    assert_eq!(t::pthread_attr_setstacksize(&mut a, 1 << 20), 0);
    let mut sz = 0;
    t::pthread_attr_getstacksize(&a, Some(&mut sz));
    assert_eq!(sz, 1 << 20);
    assert_eq!(t::pthread_attr_setstacksize(&mut a, 100), EINVAL, "below PTHREAD_STACK_MIN");
    assert_eq!(t::pthread_attr_setguardsize(&mut a, 8192), 0);
    t::pthread_attr_getguardsize(&a, Some(&mut sz));
    assert_eq!(sz, 8192);
    assert_eq!(t::pthread_attr_setschedpolicy(&mut a, 1), 0, "SCHED_FIFO");
    t::pthread_attr_getschedpolicy(&a, Some(&mut v));
    assert_eq!(v, 1);
    assert_eq!(t::pthread_attr_setschedpolicy(&mut a, 9), EINVAL);
    let p = struct_sched_param { sched_priority: 3 };
    assert_eq!(t::pthread_attr_setschedparam(&mut a, Some(&p)), 0);
    let mut q = struct_sched_param { sched_priority: 0 };
    t::pthread_attr_getschedparam(&a, Some(&mut q));
    assert_eq!(q.sched_priority, 3);
    assert_eq!(t::pthread_attr_setinheritsched(&mut a, 1), 0, "PTHREAD_EXPLICIT_SCHED");
    t::pthread_attr_getinheritsched(&a, Some(&mut v));
    assert_eq!(v, 1);
    assert_eq!(t::pthread_attr_setinheritsched(&mut a, 5), EINVAL);
    assert_eq!(t::pthread_attr_setscope(&mut a, 0), 0, "PTHREAD_SCOPE_SYSTEM");
    t::pthread_attr_getscope(&a, Some(&mut v));
    assert_eq!(v, 0);
    assert_eq!(t::pthread_attr_setscope(&mut a, 1), ENOTSUP, "PTHREAD_SCOPE_PROCESS");
    let base = core::ptr::without_provenance_mut::<core::ffi::c_void>(0x10_0000);
    assert_eq!(t::pthread_attr_setstack(&mut a, base, 1 << 20), 0);
    let mut addr = core::ptr::null_mut();
    t::pthread_attr_getstack(&a, Some(&mut addr), Some(&mut sz));
    assert_eq!((addr, sz), (base, 1 << 20));
    assert_eq!(t::pthread_attr_setstackaddr(&mut a, base), 0);
    t::pthread_attr_getstackaddr(&a, Some(&mut addr));
    assert_eq!(addr, base, "the stack's top");
    let mut set = cpu_set_t { __bits: [0; 16] };
    set.__bits[0] = 0b101;
    assert_eq!(t::pthread_attr_setaffinity_np(&mut a, core::mem::size_of::<cpu_set_t>(), &set), 0);
    let mut got = cpu_set_t { __bits: [0; 16] };
    assert_eq!(t::pthread_attr_getaffinity_np(&a, core::mem::size_of::<cpu_set_t>(), &mut got), 0);
    assert_eq!(got.__bits, set.__bits);
    let mut m = sigset_t { __val: [0; 16] };
    assert_eq!(t::pthread_attr_getsigmask_np(&a, Some(&mut m)), -1, "PTHREAD_ATTR_NO_SIGMASK_NP");
    m.__val[0] = 1 << 9;
    assert_eq!(t::pthread_attr_setsigmask_np(&mut a, Some(&m)), 0);
    let mut back = sigset_t { __val: [0; 16] };
    assert_eq!(t::pthread_attr_getsigmask_np(&a, Some(&mut back)), 0);
    assert_eq!(back.__val[0] & (1 << 9), 1 << 9, "SIGUSR1");
    assert_eq!(t::pthread_attr_destroy(&mut a), 0);
}

fn mutexattr_roundtrip() {
    let mut a = pthread_mutexattr_t { __size: [0; 4] };
    assert_eq!(t::pthread_mutexattr_init(&mut a), 0);
    let mut v = -1;
    t::pthread_mutexattr_gettype(&a, Some(&mut v));
    assert_eq!(v, 0, "PTHREAD_MUTEX_NORMAL");
    assert_eq!(t::pthread_mutexattr_settype(&mut a, 1), 0, "RECURSIVE");
    t::pthread_mutexattr_getkind_np(&a, Some(&mut v));
    assert_eq!(v, 1);
    assert_eq!(t::pthread_mutexattr_setkind_np(&mut a, 2), 0, "ERRORCHECK");
    t::pthread_mutexattr_gettype(&a, Some(&mut v));
    assert_eq!(v, 2);
    assert_eq!(t::__pthread_mutexattr_settype(&mut a, 9), EINVAL);
    assert_eq!(t::pthread_mutexattr_setpshared(&mut a, 1), 0);
    t::pthread_mutexattr_getpshared(&a, Some(&mut v));
    assert_eq!(v, 1, "PTHREAD_PROCESS_SHARED");
    assert_eq!(t::pthread_mutexattr_setpshared(&mut a, 4), EINVAL);
    assert_eq!(t::pthread_mutexattr_setprotocol(&mut a, 2), 0, "PTHREAD_PRIO_PROTECT");
    t::pthread_mutexattr_getprotocol(&a, Some(&mut v));
    assert_eq!(v, 2);
    assert_eq!(t::pthread_mutexattr_setprotocol(&mut a, 7), EINVAL);
    assert_eq!(t::pthread_mutexattr_setprioceiling(&mut a, 10), 0);
    t::pthread_mutexattr_getprioceiling(&a, Some(&mut v));
    assert_eq!(v, 10);
    assert_eq!(t::pthread_mutexattr_setrobust(&mut a, 1), 0, "PTHREAD_MUTEX_ROBUST");
    t::pthread_mutexattr_getrobust_np(&a, Some(&mut v));
    assert_eq!(v, 1);
    assert_eq!(t::pthread_mutexattr_setrobust_np(&mut a, 0), 0);
    t::pthread_mutexattr_getrobust(&a, Some(&mut v));
    assert_eq!(v, 0);
    assert_eq!(t::pthread_mutexattr_setrobust(&mut a, 3), EINVAL);
    assert_eq!(t::pthread_mutexattr_destroy(&mut a), 0);
    assert_eq!(t::__pthread_mutexattr_init(&mut a), 0);
    assert_eq!(t::__pthread_mutexattr_destroy(&mut a), 0);
}

fn condattr_roundtrip() {
    let mut a = pthread_condattr_t { __size: [0; 4] };
    assert_eq!(t::pthread_condattr_init(&mut a), 0);
    let mut v = -1;
    t::pthread_condattr_getclock(&a, Some(&mut v));
    assert_eq!(v, 0, "CLOCK_REALTIME");
    assert_eq!(t::pthread_condattr_setclock(&mut a, 1), 0, "CLOCK_MONOTONIC");
    t::pthread_condattr_getclock(&a, Some(&mut v));
    assert_eq!(v, 1);
    assert_eq!(t::pthread_condattr_setclock(&mut a, 2), EINVAL, "a CPU-time clock");
    assert_eq!(t::pthread_condattr_setpshared(&mut a, 1), 0);
    t::pthread_condattr_getpshared(&a, Some(&mut v));
    assert_eq!(v, 1);
    assert_eq!(t::pthread_condattr_setpshared(&mut a, 3), EINVAL);
    assert_eq!(t::pthread_condattr_destroy(&mut a), 0);
}

fn rwlockattr_roundtrip() {
    let mut a = pthread_rwlockattr_t { __size: [0; 8] };
    assert_eq!(t::pthread_rwlockattr_init(&mut a), 0);
    let mut v = -1;
    t::pthread_rwlockattr_getkind_np(&a, Some(&mut v));
    assert_eq!(v, 0, "PTHREAD_RWLOCK_PREFER_READER_NP");
    assert_eq!(t::pthread_rwlockattr_setkind_np(&mut a, 2), 0, "PREFER_WRITER_NONRECURSIVE");
    t::pthread_rwlockattr_getkind_np(&a, Some(&mut v));
    assert_eq!(v, 2);
    assert_eq!(t::pthread_rwlockattr_setkind_np(&mut a, 9), EINVAL);
    assert_eq!(t::pthread_rwlockattr_setpshared(&mut a, 1), 0);
    t::pthread_rwlockattr_getpshared(&a, Some(&mut v));
    assert_eq!(v, 1);
    assert_eq!(t::pthread_rwlockattr_setpshared(&mut a, 2), EINVAL);
    assert_eq!(t::pthread_rwlockattr_destroy(&mut a), 0);
}

fn barrierattr_roundtrip() {
    let mut a = pthread_barrierattr_t { __size: [0; 4] };
    assert_eq!(t::pthread_barrierattr_init(&mut a), 0);
    let mut v = -1;
    t::pthread_barrierattr_getpshared(&a, Some(&mut v));
    assert_eq!(v, 0, "PTHREAD_PROCESS_PRIVATE");
    assert_eq!(t::pthread_barrierattr_setpshared(&mut a, 1), 0);
    t::pthread_barrierattr_getpshared(&a, Some(&mut v));
    assert_eq!(v, 1);
    assert_eq!(t::pthread_barrierattr_setpshared(&mut a, 2), EINVAL);
    assert_eq!(t::pthread_barrierattr_destroy(&mut a), 0);
}

fn spin_roundtrip() {
    let mut l = pthread_spinlock_t(7);
    assert_eq!(t::pthread_spin_init(&mut l, 0), 0);
    assert_eq!(l.0, 0, "unlocked");
    assert_eq!(t::pthread_spin_lock(&mut l), 0);
    assert_eq!(t::pthread_spin_trylock(&mut l), EBUSY);
    assert_eq!(t::pthread_spin_unlock(&mut l), 0);
    assert_eq!(t::pthread_spin_trylock(&mut l), 0);
    assert_eq!(t::pthread_spin_unlock(&mut l), 0);
    assert_eq!(t::pthread_spin_destroy(&mut l), 0);
}

fn sem_roundtrip() {
    let mut s = sem_t { __size: [0; 32] };
    assert_eq!(t::sem_init(&mut s, 0, 1), 0);
    let mut v = -1;
    assert_eq!(t::sem_getvalue(&mut s, Some(&mut v)), 0);
    assert_eq!(v, 1);
    assert_eq!(t::sem_trywait(&mut s), 0);
    liella_boundary::errno::set_raw(0);
    assert_eq!(t::sem_trywait(&mut s), -1);
    assert_eq!(liella_boundary::errno::get(), EAGAIN);
    assert_eq!(t::sem_post(&mut s), 0);
    t::sem_getvalue(&mut s, Some(&mut v));
    assert_eq!(v, 1);
    assert_eq!(t::sem_destroy(&mut s), 0);
    liella_boundary::errno::set_raw(0);
    assert_eq!(t::sem_init(&mut s, 0, u32::MAX), -1, "above SEM_VALUE_MAX");
    assert_eq!(liella_boundary::errno::get(), EINVAL);
}

fn objects_roundtrip() {
    let mut m = pthread_mutex_t { __size: [0; 40] };
    assert_eq!(t::pthread_mutex_init(&mut m, core::ptr::null()), 0);
    assert_eq!(t::pthread_mutex_destroy(&mut m), 0);
    assert_eq!(t::__pthread_mutex_init(&mut m, core::ptr::null()), 0);
    assert_eq!(t::__pthread_mutex_destroy(&mut m), 0);
    let mut c = pthread_cond_t { __size: [0; 48] };
    assert_eq!(t::pthread_cond_init(&mut c, core::ptr::null()), 0);
    assert_eq!(t::pthread_cond_signal(&mut c), 0, "no waiters");
    assert_eq!(t::pthread_cond_broadcast(&mut c), 0);
    assert_eq!(t::pthread_cond_destroy(&mut c), 0);
    let mut rw = pthread_rwlock_t { __size: [0; 56] };
    assert_eq!(t::pthread_rwlock_init(&mut rw, core::ptr::null()), 0);
    assert_eq!(t::pthread_rwlock_destroy(&mut rw), 0);
    assert_eq!(t::__pthread_rwlock_init(&mut rw, core::ptr::null()), 0);
    assert_eq!(t::__pthread_rwlock_destroy(&mut rw), 0);
    let mut b = pthread_barrier_t { __size: [0; 32] };
    assert_eq!(t::pthread_barrier_init(&mut b, core::ptr::null(), 0), EINVAL, "a count of 0");
    assert_eq!(t::pthread_barrier_init(&mut b, core::ptr::null(), 2), 0);
    assert_eq!(t::pthread_barrier_destroy(&mut b), 0);
    let mut cc = cnd_t { __size: [0; 48] };
    assert_eq!(t::cnd_init(&mut cc), 0, "thrd_success");
    assert_eq!(t::cnd_signal(&mut cc), 0);
    assert_eq!(t::cnd_broadcast(&mut cc), 0);
    t::cnd_destroy(&mut cc);
    let mut mm = mtx_t { __size: [0; 40] };
    assert_eq!(t::mtx_init(&mut mm, 0), 0, "mtx_plain");
    t::mtx_destroy(&mut mm);
    assert_eq!(t::mtx_init(&mut mm, 1 | 2), 0, "mtx_recursive | mtx_timed");
    t::mtx_destroy(&mut mm);
    assert_eq!(t::mtx_init(&mut mm, 8), 2, "an unknown kind: thrd_error");
}

fn equal_check() {
    assert_ne!(t::pthread_equal(pthread_t(5), pthread_t(5)), 0);
    assert_eq!(t::pthread_equal(pthread_t(5), pthread_t(6)), 0);
    assert_ne!(t::thrd_equal(thrd_t(9), thrd_t(9)), 0);
    assert_eq!(t::thrd_equal(thrd_t(9), thrd_t(8)), 0);
}

fn concurrency_check() {
    static ONE: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = ONE.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    assert_eq!(t::pthread_setconcurrency(4), 0);
    assert_eq!(t::pthread_getconcurrency(), 4, "a hint, stored");
    assert_eq!(t::pthread_setconcurrency(-1), EINVAL);
    assert_eq!(t::pthread_setconcurrency(0), 0);
    assert_eq!(t::pthread_getconcurrency(), 0);
}

#[test]
fn sym_call_once() {
    exported("call_once");
    conformance(&["s22_sync", "s22_once_unwind"]);
}
#[test]
fn sym_cnd_broadcast() {
    objects_roundtrip();
    exported("cnd_broadcast");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_cnd_destroy() {
    objects_roundtrip();
    exported("cnd_destroy");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_cnd_init() {
    objects_roundtrip();
    exported("cnd_init");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_cnd_signal() {
    objects_roundtrip();
    exported("cnd_signal");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_cnd_timedwait() {
    exported("cnd_timedwait");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_cnd_wait() {
    exported("cnd_wait");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym___libc_single_threaded() {
    exported("__libc_single_threaded");
    conformance(&["s22_lifecycle"]);
}
#[test]
fn sym_mtx_destroy() {
    objects_roundtrip();
    exported("mtx_destroy");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_mtx_init() {
    objects_roundtrip();
    exported("mtx_init");
    conformance(&["s22_sync"]);
}
#[test]
fn sym_mtx_lock() {
    exported("mtx_lock");
    conformance(&["s22_sync"]);
}
#[test]
fn sym_mtx_timedlock() {
    exported("mtx_timedlock");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_mtx_trylock() {
    exported("mtx_trylock");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_mtx_unlock() {
    exported("mtx_unlock");
    conformance(&["s22_sync"]);
}
#[test]
fn sym_pthread_atfork() {
    compat_only("pthread_atfork");
    conformance(&["s18_fork"]);
}
#[test]
fn sym_pthread_attr_destroy() {
    attr_roundtrip();
    exported("pthread_attr_destroy");
    conformance(&["s22_lifecycle", "s22_more"]);
}
#[test]
fn sym_pthread_attr_getaffinity_np() {
    attr_roundtrip();
    exported("pthread_attr_getaffinity_np");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_attr_getdetachstate() {
    attr_roundtrip();
    exported("pthread_attr_getdetachstate");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_attr_getguardsize() {
    attr_roundtrip();
    exported("pthread_attr_getguardsize");
    conformance(&["s22_lifecycle"]);
}
#[test]
fn sym_pthread_attr_getinheritsched() {
    attr_roundtrip();
    exported("pthread_attr_getinheritsched");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_attr_getschedparam() {
    attr_roundtrip();
    exported("pthread_attr_getschedparam");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_attr_getschedpolicy() {
    attr_roundtrip();
    exported("pthread_attr_getschedpolicy");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_attr_getscope() {
    attr_roundtrip();
    exported("pthread_attr_getscope");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_attr_getsigmask_np() {
    attr_roundtrip();
    exported("pthread_attr_getsigmask_np");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_attr_getstack() {
    attr_roundtrip();
    exported("pthread_attr_getstack");
    conformance(&["s22_lifecycle"]);
}
#[test]
fn sym_pthread_attr_getstackaddr() {
    attr_roundtrip();
    exported("pthread_attr_getstackaddr");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_attr_getstacksize() {
    attr_roundtrip();
    exported("pthread_attr_getstacksize");
    conformance(&["s22_lifecycle", "s22_more"]);
}
#[test]
fn sym_pthread_attr_init() {
    attr_roundtrip();
    exported("pthread_attr_init");
    conformance(&["s22_lifecycle", "s22_more"]);
}
#[test]
fn sym_pthread_attr_setaffinity_np() {
    attr_roundtrip();
    exported("pthread_attr_setaffinity_np");
    conformance(&["s22_more"]);
}
#[test]
fn sym_pthread_attr_setdetachstate() {
    attr_roundtrip();
    exported("pthread_attr_setdetachstate");
    conformance(&["s22_more"]);
}
#[test]
fn sym_pthread_attr_setguardsize() {
    attr_roundtrip();
    exported("pthread_attr_setguardsize");
    conformance(&["s22_lifecycle"]);
}
#[test]
fn sym_pthread_attr_setinheritsched() {
    attr_roundtrip();
    exported("pthread_attr_setinheritsched");
    conformance(&["s22_more"]);
}
#[test]
fn sym_pthread_attr_setschedparam() {
    attr_roundtrip();
    exported("pthread_attr_setschedparam");
    conformance(&["s22_more"]);
}
#[test]
fn sym_pthread_attr_setschedpolicy() {
    attr_roundtrip();
    exported("pthread_attr_setschedpolicy");
    conformance(&["s22_more"]);
}
#[test]
fn sym_pthread_attr_setscope() {
    attr_roundtrip();
    exported("pthread_attr_setscope");
    conformance(&["s22_lifecycle"]);
}
#[test]
fn sym_pthread_attr_setsigmask_np() {
    attr_roundtrip();
    exported("pthread_attr_setsigmask_np");
    conformance(&["s22_more"]);
}
#[test]
fn sym_pthread_attr_setstack() {
    attr_roundtrip();
    exported("pthread_attr_setstack");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_attr_setstackaddr() {
    attr_roundtrip();
    exported("pthread_attr_setstackaddr");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_attr_setstacksize() {
    attr_roundtrip();
    exported("pthread_attr_setstacksize");
    conformance(&["s22_lifecycle", "s22_more"]);
}
#[test]
fn sym_pthread_barrier_destroy() {
    objects_roundtrip();
    exported("pthread_barrier_destroy");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_barrier_init() {
    objects_roundtrip();
    exported("pthread_barrier_init");
    conformance(&["s17_setxid", "s17_setxid_ids", "s22_sync"]);
}
#[test]
fn sym_pthread_barrier_wait() {
    exported("pthread_barrier_wait");
    conformance(&["s17_setxid", "s17_setxid_ids", "s22_sync"]);
}
#[test]
fn sym_pthread_barrierattr_destroy() {
    barrierattr_roundtrip();
    exported("pthread_barrierattr_destroy");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_barrierattr_getpshared() {
    barrierattr_roundtrip();
    exported("pthread_barrierattr_getpshared");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_barrierattr_init() {
    barrierattr_roundtrip();
    exported("pthread_barrierattr_init");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_barrierattr_setpshared() {
    barrierattr_roundtrip();
    exported("pthread_barrierattr_setpshared");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_cancel() {
    exported("pthread_cancel");
    conformance(&["s22_cancel", "s22_cancel_points", "s22_more", "s22_cancel_fast"]);
}
#[test]
fn sym__pthread_cleanup_pop() {
    exported("_pthread_cleanup_pop");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym__pthread_cleanup_pop_restore() {
    compat_only("_pthread_cleanup_pop_restore");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym__pthread_cleanup_push() {
    exported("_pthread_cleanup_push");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym__pthread_cleanup_push_defer() {
    compat_only("_pthread_cleanup_push_defer");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym___pthread_cleanup_routine() {
    exported("__pthread_cleanup_routine");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_clockjoin_np() {
    exported("pthread_clockjoin_np");
    conformance(&["s22_lifecycle"]);
}
#[test]
fn sym_pthread_cond_broadcast() {
    objects_roundtrip();
    exported("pthread_cond_broadcast");
    conformance(&["s22_sync"]);
}
#[test]
fn sym_pthread_cond_clockwait() {
    exported("pthread_cond_clockwait");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_cond_destroy() {
    objects_roundtrip();
    exported("pthread_cond_destroy");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_cond_init() {
    objects_roundtrip();
    exported("pthread_cond_init");
    conformance(&["s22_sync"]);
}
/// s22_5_c_1_2: the old (pre-2.3.2) condition variable is one word, zeroed by `init`, filled in on
/// first use and zero again after `destroy`; `init` refuses a shared or monotonic attribute after
/// zeroing (tools/probe/compat/compat_condvar.c). No thread waits here (waits need the products).
#[test]
fn sym_liella_compat_pthread_cond_objects() {
    let mut w: u64 = 0xa5a5_a5a5_a5a5_a5a5;
    assert_eq!(t::liella_compat_pthread_cond_init(&mut w, core::ptr::null()), 0);
    assert_eq!(w, 0, "init zeroes the word");
    assert_eq!(t::liella_compat_pthread_cond_signal(&mut w), 0);
    assert_ne!(w, 0, "first use installs the condition variable");
    let first = w;
    assert_eq!(t::liella_compat_pthread_cond_broadcast(&mut w), 0);
    assert_eq!(w, first, "later uses keep it");
    assert_eq!(t::liella_compat_pthread_cond_destroy(&mut w), 0);
    assert_eq!(w, 0, "destroy detaches it");
    assert_eq!(t::liella_compat_pthread_cond_destroy(&mut w), 0, "destroy of an unused object");
    let mut a = pthread_condattr_t { __size: [0; 4] };
    assert_eq!(t::pthread_condattr_init(&mut a), 0);
    assert_eq!(t::pthread_condattr_setpshared(&mut a, 1), 0);
    w = u64::MAX;
    assert_eq!(t::liella_compat_pthread_cond_init(&mut w, &a), 22, "process-shared refused (EINVAL)");
    assert_eq!(w, 0, "zeroed before the refusal");
    assert_eq!(t::pthread_condattr_init(&mut a), 0);
    assert_eq!(t::pthread_condattr_setclock(&mut a, 1), 0);
    assert_eq!(t::liella_compat_pthread_cond_init(&mut w, &a), 22, "CLOCK_MONOTONIC refused (EINVAL)");
    assert_eq!(t::liella_compat_pthread_cond_signal(core::ptr::null_mut()), 22, "NULL: EINVAL");
}
/// s22_5_c_1_2: the old condition variable across Liella's threads, through the products: signal
/// hands each of 20000 items to one of 4 consumers, broadcast releases 4 waiters for each of 2000
/// generations, timedwait returns 0 when signalled before its deadline and ETIMEDOUT past it, and
/// destroy zeroes the word. A lost wakeup hangs the program until its alarm fails it (LG-3, #1167).
#[test]
fn sym_liella_compat_pthread_cond_threads() {
    conformance(&["s22_compat_condvar"]);
}
#[test]
fn sym_pthread_cond_signal() {
    objects_roundtrip();
    exported("pthread_cond_signal");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_cond_timedwait() {
    exported("pthread_cond_timedwait");
    conformance(&["s22_sync"]);
}
#[test]
fn sym_pthread_cond_wait() {
    exported("pthread_cond_wait");
    conformance(&["s22_cancel", "s22_sync"]);
}
#[test]
fn sym_pthread_condattr_destroy() {
    condattr_roundtrip();
    exported("pthread_condattr_destroy");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_condattr_getclock() {
    condattr_roundtrip();
    exported("pthread_condattr_getclock");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_condattr_getpshared() {
    condattr_roundtrip();
    exported("pthread_condattr_getpshared");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_condattr_init() {
    condattr_roundtrip();
    exported("pthread_condattr_init");
    conformance(&["s22_sync"]);
}
#[test]
fn sym_pthread_condattr_setclock() {
    condattr_roundtrip();
    exported("pthread_condattr_setclock");
    conformance(&["s22_sync"]);
}
#[test]
fn sym_pthread_condattr_setpshared() {
    condattr_roundtrip();
    exported("pthread_condattr_setpshared");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_create() {
    exported("pthread_create");
    conformance(&["s17_setxid", "s17_setxid_ids", "s18_fork"]);
}
#[test]
fn sym_pthread_detach() {
    exported("pthread_detach");
    conformance(&["s22_lifecycle", "s22_more"]);
}
#[test]
fn sym_pthread_equal() {
    equal_check();
    exported("pthread_equal");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_exit() {
    exported("pthread_exit");
    conformance(&["s22_cancel", "s22_last_exit"]);
}
#[test]
fn sym_pthread_getaffinity_np() {
    exported("pthread_getaffinity_np");
    conformance(&["s22_more"]);
}
#[test]
fn sym_pthread_getattr_default_np() {
    exported("pthread_getattr_default_np");
    conformance(&["s22_more"]);
}
#[test]
fn sym_pthread_getattr_np() {
    exported("pthread_getattr_np");
    conformance(&["s22_lifecycle"]);
}
#[test]
fn sym_pthread_getconcurrency() {
    concurrency_check();
    exported("pthread_getconcurrency");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_getcpuclockid() {
    exported("pthread_getcpuclockid");
    conformance(&["s22_more"]);
}
#[test]
fn sym_pthread_getname_np() {
    exported("pthread_getname_np");
    conformance(&["s22_lifecycle"]);
}
#[test]
fn sym_pthread_getschedparam() {
    exported("pthread_getschedparam");
    conformance(&["s22_more"]);
}
#[test]
fn sym___pthread_getspecific() {
    compat_only("__pthread_getspecific");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_getspecific() {
    exported("pthread_getspecific");
    conformance(&["s22_lifecycle"]);
}
#[test]
fn sym_pthread_join() {
    exported("pthread_join");
    conformance(&["s17_setxid", "s17_setxid_ids", "s18_fork"]);
}
#[test]
fn sym___pthread_key_create() {
    exported("__pthread_key_create");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_key_create() {
    exported("pthread_key_create");
    conformance(&["s22_cancel", "s22_lifecycle", "s22_more"]);
}
#[test]
fn sym_pthread_key_delete() {
    exported("pthread_key_delete");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_kill() {
    exported("pthread_kill");
    conformance(&["s22_cancel"]);
}
#[test]
fn sym_pthread_kill_other_threads_np() {
    compat_only("pthread_kill_other_threads_np");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_mutex_clocklock() {
    exported("pthread_mutex_clocklock");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_mutex_consistent_np() {
    compat_only("pthread_mutex_consistent_np");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_mutex_consistent() {
    exported("pthread_mutex_consistent");
    conformance(&["s18_fork", "s22_sync"]);
}
#[test]
fn sym___pthread_mutex_destroy() {
    objects_roundtrip();
    compat_only("__pthread_mutex_destroy");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_mutex_destroy() {
    objects_roundtrip();
    exported("pthread_mutex_destroy");
    conformance(&["s22_sync"]);
}
#[test]
fn sym_pthread_mutex_getprioceiling() {
    exported("pthread_mutex_getprioceiling");
    conformance(&["s22_more"]);
}
#[test]
fn sym___pthread_mutex_init() {
    objects_roundtrip();
    compat_only("__pthread_mutex_init");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_mutex_init() {
    objects_roundtrip();
    exported("pthread_mutex_init");
    conformance(&["s18_fork", "s22_more", "s22_sync"]);
}
#[test]
fn sym___pthread_mutex_lock() {
    compat_only("__pthread_mutex_lock");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_mutex_lock() {
    exported("pthread_mutex_lock");
    conformance(&["s18_fork", "s22_cancel", "s22_more"]);
}
#[test]
fn sym_pthread_mutex_setprioceiling() {
    exported("pthread_mutex_setprioceiling");
    conformance(&["s22_more"]);
}
#[test]
fn sym_pthread_mutex_timedlock() {
    exported("pthread_mutex_timedlock");
    conformance(&["s22_sync"]);
}
#[test]
fn sym___pthread_mutex_trylock() {
    compat_only("__pthread_mutex_trylock");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_mutex_trylock() {
    exported("pthread_mutex_trylock");
    conformance(&["s22_cancel", "s22_sync"]);
}
#[test]
fn sym___pthread_mutex_unlock() {
    compat_only("__pthread_mutex_unlock");
    conformance(&["s22_symbols"]);
}
#[test]
fn mutex_unlock_checks_the_owner_of_checked_types() {
    // Clauses: s22_4_c_0 (TINY-HOT: the inlined unlock fast path keeps the owner checks). An
    // error-checking or recursive mutex held by this thread: another thread's unlock is EPERM and
    // leaves it held; an unlocked error-checking mutex's unlock is EPERM; a recursive mutex held
    // twice stays held after one unlock.
    const EPERM: i32 = 1;
    for ty in [2, 1] {
        let mut a = pthread_mutexattr_t { __size: [0; 4] };
        assert_eq!(t::pthread_mutexattr_init(&mut a), 0);
        assert_eq!(t::pthread_mutexattr_settype(&mut a, ty), 0);
        let mut m = pthread_mutex_t { __size: [0; 40] };
        assert_eq!(t::pthread_mutex_init(&mut m, &a), 0);
        let mp: *mut pthread_mutex_t = &mut m;
        if ty == 2 {
            assert_eq!(t::pthread_mutex_unlock(mp), EPERM, "unlocking an unlocked error-checking mutex");
        }
        assert_eq!(t::pthread_mutex_lock(mp), 0);
        let addr = mp.expose_provenance();
        let other = std::thread::spawn(move || t::pthread_mutex_unlock(core::ptr::with_exposed_provenance_mut(addr))).join().unwrap();
        assert_eq!(other, EPERM, "type {ty}: unlock by a thread that does not own it");
        // Tried from another thread: the owner's own trylock of a recursive mutex succeeds (POSIX).
        let still = std::thread::spawn(move || t::pthread_mutex_trylock(core::ptr::with_exposed_provenance_mut(addr))).join().unwrap();
        assert_eq!(still, EBUSY, "type {ty}: still held after the other thread's unlock");
        if ty == 1 {
            assert_eq!(t::pthread_mutex_lock(mp), 0, "recursive relock");
            assert_eq!(t::pthread_mutex_unlock(mp), 0);
            let held = std::thread::spawn(move || t::pthread_mutex_trylock(core::ptr::with_exposed_provenance_mut(addr))).join().unwrap();
            assert_eq!(held, EBUSY, "a recursive mutex locked twice is held after one unlock");
        }
        assert_eq!(t::pthread_mutex_unlock(mp), 0);
        let free = std::thread::spawn(move || {
            let p = core::ptr::with_exposed_provenance_mut(addr);
            (t::pthread_mutex_trylock(p), t::pthread_mutex_unlock(p))
        })
        .join()
        .unwrap();
        assert_eq!(free, (0, 0), "type {ty}: released by the owner's last unlock");
        assert_eq!(t::pthread_mutex_destroy(mp), 0);
    }
}

/// Runs `f` on a helper thread; a hang (a lost wake-up leaves a waiter asleep for good) fails the
/// test instead of stalling the run. 120 s is a hang detector, not a speed bound.
fn within_hang_limit(what: &str, f: impl FnOnce() + Send + 'static) {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        f();
        let _ = tx.send(());
    });
    assert!(rx.recv_timeout(std::time::Duration::from_secs(120)).is_ok(), "{what}: no progress (a waiter was never woken?)");
}

#[test]
fn mutex_kinds_exclude_under_contention() {
    // Clauses: s22_4_c_0 (MUTEX-FAST: the inlined fast paths). Eight threads each take every kind
    // of plain mutex 4000 times (the recursive one two levels deep, through the nested fast path)
    // and bump a counter with a separate load and store: any lost update (no exclusion) or lost
    // wake-up (a release that does not wake a sleeper) shows as a wrong total or a hang.
    for ty in [0, 1, 2, 3] {
        within_hang_limit(&format!("type {ty}"), move || {
            let mut a = pthread_mutexattr_t { __size: [0; 4] };
            assert_eq!(t::pthread_mutexattr_init(&mut a), 0);
            assert_eq!(t::pthread_mutexattr_settype(&mut a, ty), 0);
            let m: &'static mut pthread_mutex_t = Box::leak(Box::new(pthread_mutex_t { __size: [0; 40] }));
            assert_eq!(t::pthread_mutex_init(m, &a), 0);
            let addr = core::ptr::from_mut(m).expose_provenance();
            let counter: &'static AtomicUsize = Box::leak(Box::new(AtomicUsize::new(0)));
            let (threads, rounds) = (8, 4000);
            let hs: Vec<_> = (0..threads)
                .map(|_| {
                    std::thread::spawn(move || {
                        let p: *mut pthread_mutex_t = core::ptr::with_exposed_provenance_mut(addr);
                        for i in 0..rounds {
                            assert_eq!(t::pthread_mutex_lock(p), 0);
                            let nested = ty == 1 && i % 2 == 0;
                            if nested {
                                assert_eq!(t::pthread_mutex_lock(p), 0);
                            }
                            let v = counter.load(Ordering::Relaxed);
                            for _ in 0..(i % 7) {
                                core::hint::spin_loop();
                            }
                            counter.store(v + 1, Ordering::Relaxed);
                            if nested {
                                assert_eq!(t::pthread_mutex_unlock(p), 0);
                            }
                            assert_eq!(t::pthread_mutex_unlock(p), 0);
                        }
                    })
                })
                .collect();
            for h in hs {
                h.join().unwrap();
            }
            assert_eq!(counter.load(Ordering::Relaxed), threads * rounds, "type {ty}: updates lost");
            assert_eq!(t::pthread_mutex_destroy(m), 0, "type {ty}: left free");
        });
    }
}

#[test]
fn mutex_kind_rules_on_the_fast_paths() {
    // Clauses: s22_4_c_0 (MUTEX-FAST). Relock by the owner: EDEADLK (error-checking), one more level
    // (recursive, trylock too); trylock of a held mutex: EBUSY; a recursive mutex is released only
    // by its last unlock; an error-checking one unlocked twice: EPERM.
    const EPERM: i32 = 1;
    const EDEADLK: i32 = 35;
    let mk = |ty: i32| {
        let mut a = pthread_mutexattr_t { __size: [0; 4] };
        assert_eq!(t::pthread_mutexattr_init(&mut a), 0);
        assert_eq!(t::pthread_mutexattr_settype(&mut a, ty), 0);
        let m: &'static mut pthread_mutex_t = Box::leak(Box::new(pthread_mutex_t { __size: [0; 40] }));
        assert_eq!(t::pthread_mutex_init(m, &a), 0);
        core::ptr::from_mut(m)
    };
    let other_try = |p: *mut pthread_mutex_t| {
        let a = p.expose_provenance();
        std::thread::spawn(move || {
            let p = core::ptr::with_exposed_provenance_mut(a);
            let r = t::pthread_mutex_trylock(p);
            if r == 0 {
                assert_eq!(t::pthread_mutex_unlock(p), 0);
            }
            r
        })
        .join()
        .unwrap()
    };
    let e = mk(2);
    assert_eq!(t::pthread_mutex_lock(e), 0);
    assert_eq!(t::pthread_mutex_lock(e), EDEADLK);
    assert_eq!(t::pthread_mutex_trylock(e), EBUSY);
    assert_eq!(other_try(e), EBUSY);
    assert_eq!(t::pthread_mutex_unlock(e), 0);
    assert_eq!(t::pthread_mutex_unlock(e), EPERM);
    assert_eq!(other_try(e), 0);
    let r = mk(1);
    assert_eq!(t::pthread_mutex_lock(r), 0);
    assert_eq!(t::pthread_mutex_lock(r), 0);
    assert_eq!(t::pthread_mutex_trylock(r), 0);
    for left in [2, 1] {
        assert_eq!(t::pthread_mutex_unlock(r), 0);
        assert_eq!(other_try(r), EBUSY, "{left} level(s) still held");
    }
    assert_eq!(t::pthread_mutex_unlock(r), 0);
    assert_eq!(t::pthread_mutex_unlock(r), EPERM);
    assert_eq!(other_try(r), 0);
    let n = mk(0);
    assert_eq!(t::pthread_mutex_trylock(n), 0);
    assert_eq!(t::pthread_mutex_trylock(n), EBUSY);
    assert_eq!(other_try(n), EBUSY);
    assert_eq!(t::pthread_mutex_unlock(n), 0);
    assert_eq!(other_try(n), 0);
}

#[test]
fn sym_pthread_mutex_unlock() {
    exported("pthread_mutex_unlock");
    conformance(&["s18_fork", "s22_cancel", "s22_more"]);
}
#[test]
fn sym___pthread_mutexattr_destroy() {
    mutexattr_roundtrip();
    compat_only("__pthread_mutexattr_destroy");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_mutexattr_destroy() {
    mutexattr_roundtrip();
    exported("pthread_mutexattr_destroy");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_mutexattr_getkind_np() {
    mutexattr_roundtrip();
    compat_only("pthread_mutexattr_getkind_np");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_mutexattr_getprioceiling() {
    mutexattr_roundtrip();
    exported("pthread_mutexattr_getprioceiling");
    conformance(&["s22_more"]);
}
#[test]
fn sym_pthread_mutexattr_getprotocol() {
    mutexattr_roundtrip();
    exported("pthread_mutexattr_getprotocol");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_mutexattr_getpshared() {
    mutexattr_roundtrip();
    exported("pthread_mutexattr_getpshared");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_mutexattr_getrobust_np() {
    mutexattr_roundtrip();
    compat_only("pthread_mutexattr_getrobust_np");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_mutexattr_getrobust() {
    mutexattr_roundtrip();
    exported("pthread_mutexattr_getrobust");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_mutexattr_gettype() {
    mutexattr_roundtrip();
    exported("pthread_mutexattr_gettype");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym___pthread_mutexattr_init() {
    mutexattr_roundtrip();
    compat_only("__pthread_mutexattr_init");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_mutexattr_init() {
    mutexattr_roundtrip();
    exported("pthread_mutexattr_init");
    conformance(&["s18_fork", "s22_more", "s22_sync"]);
}
#[test]
fn sym_pthread_mutexattr_setkind_np() {
    mutexattr_roundtrip();
    compat_only("pthread_mutexattr_setkind_np");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_mutexattr_setprioceiling() {
    mutexattr_roundtrip();
    exported("pthread_mutexattr_setprioceiling");
    conformance(&["s22_more"]);
}
#[test]
fn sym_pthread_mutexattr_setprotocol() {
    mutexattr_roundtrip();
    exported("pthread_mutexattr_setprotocol");
    conformance(&["s22_more", "s22_sync"]);
}
#[test]
fn sym_pthread_mutexattr_setpshared() {
    mutexattr_roundtrip();
    exported("pthread_mutexattr_setpshared");
    conformance(&["s18_fork", "s22_more"]);
}
#[test]
fn sym_pthread_mutexattr_setrobust_np() {
    mutexattr_roundtrip();
    compat_only("pthread_mutexattr_setrobust_np");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_mutexattr_setrobust() {
    mutexattr_roundtrip();
    exported("pthread_mutexattr_setrobust");
    conformance(&["s18_fork", "s22_sync"]);
}
#[test]
fn sym___pthread_mutexattr_settype() {
    mutexattr_roundtrip();
    compat_only("__pthread_mutexattr_settype");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_mutexattr_settype() {
    mutexattr_roundtrip();
    exported("pthread_mutexattr_settype");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym___pthread_once() {
    compat_only("__pthread_once");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_once() {
    exported("pthread_once");
    conformance(&["s22_cancel", "s22_sync", "s22_once_unwind", "s22_once_cxx"]);
}
#[test]
fn sym___pthread_register_cancel_defer() {
    exported("__pthread_register_cancel_defer");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym___pthread_register_cancel() {
    exported("__pthread_register_cancel");
    conformance(&["s22_cancel"]);
}
#[test]
fn sym_pthread_rwlock_clockrdlock() {
    exported("pthread_rwlock_clockrdlock");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_rwlock_clockwrlock() {
    exported("pthread_rwlock_clockwrlock");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym___pthread_rwlock_destroy() {
    objects_roundtrip();
    compat_only("__pthread_rwlock_destroy");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_rwlock_destroy() {
    objects_roundtrip();
    exported("pthread_rwlock_destroy");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym___pthread_rwlock_init() {
    objects_roundtrip();
    compat_only("__pthread_rwlock_init");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_rwlock_init() {
    objects_roundtrip();
    exported("pthread_rwlock_init");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym___pthread_rwlock_rdlock() {
    compat_only("__pthread_rwlock_rdlock");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_rwlock_rdlock() {
    exported("pthread_rwlock_rdlock");
    conformance(&["s22_sync"]);
}
#[test]
fn sym_pthread_rwlock_timedrdlock() {
    exported("pthread_rwlock_timedrdlock");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_rwlock_timedwrlock() {
    exported("pthread_rwlock_timedwrlock");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym___pthread_rwlock_tryrdlock() {
    compat_only("__pthread_rwlock_tryrdlock");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_rwlock_tryrdlock() {
    exported("pthread_rwlock_tryrdlock");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym___pthread_rwlock_trywrlock() {
    compat_only("__pthread_rwlock_trywrlock");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_rwlock_trywrlock() {
    exported("pthread_rwlock_trywrlock");
    conformance(&["s22_sync"]);
}
#[test]
fn sym___pthread_rwlock_unlock() {
    compat_only("__pthread_rwlock_unlock");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_rwlock_unlock() {
    exported("pthread_rwlock_unlock");
    conformance(&["s22_sync"]);
}
#[test]
fn sym___pthread_rwlock_wrlock() {
    compat_only("__pthread_rwlock_wrlock");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_rwlock_wrlock() {
    exported("pthread_rwlock_wrlock");
    conformance(&["s22_sync"]);
}
#[test]
fn sym_pthread_rwlockattr_destroy() {
    rwlockattr_roundtrip();
    exported("pthread_rwlockattr_destroy");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_rwlockattr_getkind_np() {
    rwlockattr_roundtrip();
    exported("pthread_rwlockattr_getkind_np");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_rwlockattr_getpshared() {
    rwlockattr_roundtrip();
    exported("pthread_rwlockattr_getpshared");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_rwlockattr_init() {
    rwlockattr_roundtrip();
    exported("pthread_rwlockattr_init");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_rwlockattr_setkind_np() {
    rwlockattr_roundtrip();
    exported("pthread_rwlockattr_setkind_np");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_rwlockattr_setpshared() {
    rwlockattr_roundtrip();
    exported("pthread_rwlockattr_setpshared");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_self() {
    exported("pthread_self");
    conformance(&["s22_cancel_points", "s22_lifecycle", "s22_more"]);
}
#[test]
fn sym_pthread_setaffinity_np() {
    exported("pthread_setaffinity_np");
    conformance(&["s22_more"]);
}
#[test]
fn sym_pthread_setattr_default_np() {
    exported("pthread_setattr_default_np");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_setcancelstate() {
    exported("pthread_setcancelstate");
    conformance(&["s22_cancel", "s22_cancel_points", "s22_more"]);
}
#[test]
fn sym_pthread_setcanceltype() {
    exported("pthread_setcanceltype");
    conformance(&["s22_cancel"]);
}
#[test]
fn sym_pthread_setconcurrency() {
    concurrency_check();
    exported("pthread_setconcurrency");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_setname_np() {
    exported("pthread_setname_np");
    conformance(&["s22_lifecycle"]);
}
#[test]
fn sym_pthread_setschedparam() {
    exported("pthread_setschedparam");
    conformance(&["s22_more"]);
}
#[test]
fn sym_pthread_setschedprio() {
    exported("pthread_setschedprio");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym___pthread_setspecific() {
    compat_only("__pthread_setspecific");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_setspecific() {
    exported("pthread_setspecific");
    conformance(&["s22_cancel", "s22_lifecycle"]);
}
#[test]
fn sym_pthread_sigmask() {
    // In a fresh thread: the mask is per thread.
    std::thread::spawn(|| {
        let mut set = liella_abi::layout::sigset_t { __val: [0; 16] };
        sig::sigaddset(Some(&mut set), 10);
        let mut old = liella_abi::layout::sigset_t { __val: [9; 16] };
        assert_eq!(sig::pthread_sigmask(0, Some(&set), Some(&mut old)), 0);
        let mut now = liella_abi::layout::sigset_t { __val: [0; 16] };
        sig::pthread_sigmask(0, None, Some(&mut now));
        assert_eq!(sig::sigismember(Some(&now), 10), 1);
        errno::set_raw(0);
        assert_eq!(sig::pthread_sigmask(9, Some(&set), None), 22, "the error number, returned");
        assert_eq!(errno::get(), 0, "errno untouched");
    })
    .join()
    .unwrap();
}
#[test]
fn sym_pthread_sigqueue() {
    exported("pthread_sigqueue");
    conformance(&["s22_cancel"]);
}
#[test]
fn sym_pthread_spin_destroy() {
    spin_roundtrip();
    exported("pthread_spin_destroy");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_pthread_spin_init() {
    spin_roundtrip();
    exported("pthread_spin_init");
    conformance(&["s22_sync"]);
}
#[test]
fn sym_pthread_spin_lock() {
    spin_roundtrip();
    exported("pthread_spin_lock");
    conformance(&["s22_sync"]);
}
#[test]
fn sym_pthread_spin_trylock() {
    spin_roundtrip();
    exported("pthread_spin_trylock");
    conformance(&["s22_sync"]);
}
#[test]
fn sym_pthread_spin_unlock() {
    spin_roundtrip();
    exported("pthread_spin_unlock");
    conformance(&["s22_sync"]);
}
#[test]
fn sym_pthread_testcancel() {
    exported("pthread_testcancel");
    conformance(&["s22_cancel"]);
}
#[test]
fn sym_pthread_timedjoin_np() {
    exported("pthread_timedjoin_np");
    conformance(&["s22_lifecycle"]);
}
#[test]
fn sym_pthread_tryjoin_np() {
    exported("pthread_tryjoin_np");
    conformance(&["s22_lifecycle"]);
}
#[test]
fn sym___pthread_unregister_cancel_restore() {
    exported("__pthread_unregister_cancel_restore");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym___pthread_unregister_cancel() {
    exported("__pthread_unregister_cancel");
    conformance(&["s22_cancel"]);
}
#[test]
fn sym___pthread_unwind_next() {
    exported("__pthread_unwind_next");
    conformance(&["s22_cancel"]);
}
#[test]
fn sym_pthread_yield() {
    compat_only("pthread_yield");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym___rseq_flags() {
    loader_owned("__rseq_flags");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym___rseq_offset() {
    loader_owned("__rseq_offset");
    conformance(&["s22_lifecycle", "s22_more"]);
}
#[test]
fn sym___rseq_size() {
    loader_owned("__rseq_size");
    conformance(&["s22_lifecycle", "s22_more"]);
}
#[test]
fn sym_sem_clockwait() {
    exported("sem_clockwait");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_sem_close() {
    exported("sem_close");
    conformance(&["s22_sync"]);
}
#[test]
fn sym_sem_destroy() {
    sem_roundtrip();
    exported("sem_destroy");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_sem_getvalue() {
    sem_roundtrip();
    exported("sem_getvalue");
    conformance(&["s22_sync"]);
}
#[test]
fn sym_sem_init() {
    sem_roundtrip();
    exported("sem_init");
    conformance(&["s22_cancel", "s22_more", "s22_sync"]);
}
#[test]
fn sym_sem_open() {
    exported("sem_open");
    conformance(&["s22_sync"]);
}
#[test]
fn sym_sem_post() {
    sem_roundtrip();
    exported("sem_post");
    conformance(&["s22_more", "s22_sync"]);
}
#[test]
fn sym_sem_timedwait() {
    exported("sem_timedwait");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_sem_trywait() {
    sem_roundtrip();
    exported("sem_trywait");
    conformance(&["s22_more", "s22_sync"]);
}
#[test]
fn sym_sem_unlink() {
    exported("sem_unlink");
    conformance(&["s22_sync"]);
}
#[test]
fn sym_sem_wait() {
    exported("sem_wait");
    conformance(&["s22_cancel", "s22_more", "s22_sync"]);
}
symbol!(sym_td_init, "td_init", "22", "2.2.5", r##""##);
symbol!(sym_td_log, "td_log", "22", "2.2.5", r##""##);
symbol!(sym_td_symbol_list, "td_symbol_list", "22", "2.2.5", r##""##);
symbol!(sym_td_ta_clear_event, "td_ta_clear_event", "22", "2.2.5", r##""##);
symbol!(sym_td_ta_delete, "td_ta_delete", "22", "2.2.5", r##""##);
symbol!(sym_td_ta_enable_stats, "td_ta_enable_stats", "22", "2.2.5", r##""##);
symbol!(sym_td_ta_event_addr, "td_ta_event_addr", "22", "2.2.5", r##""##);
symbol!(sym_td_ta_event_getmsg, "td_ta_event_getmsg", "22", "2.2.5", r##""##);
symbol!(sym_td_ta_get_nthreads, "td_ta_get_nthreads", "22", "2.2.5", r##""##);
symbol!(sym_td_ta_get_ph, "td_ta_get_ph", "22", "2.2.5", r##""##);
symbol!(sym_td_ta_get_stats, "td_ta_get_stats", "22", "2.2.5", r##""##);
symbol!(sym_td_ta_map_id2thr, "td_ta_map_id2thr", "22", "2.2.5", r##""##);
symbol!(sym_td_ta_map_lwp2thr, "td_ta_map_lwp2thr", "22", "2.2.5", r##""##);
symbol!(sym_td_ta_new, "td_ta_new", "22", "2.2.5", r##""##);
symbol!(sym_td_ta_reset_stats, "td_ta_reset_stats", "22", "2.2.5", r##""##);
symbol!(sym_td_ta_set_event, "td_ta_set_event", "22", "2.2.5", r##""##);
symbol!(sym_td_ta_setconcurrency, "td_ta_setconcurrency", "22", "2.2.5", r##""##);
symbol!(sym_td_ta_thr_iter, "td_ta_thr_iter", "22", "2.2.5", r##""##);
symbol!(sym_td_ta_tsd_iter, "td_ta_tsd_iter", "22", "2.2.5", r##""##);
symbol!(sym_td_thr_clear_event, "td_thr_clear_event", "22", "2.2.5", r##""##);
symbol!(sym_td_thr_dbresume, "td_thr_dbresume", "22", "2.2.5", r##""##);
symbol!(sym_td_thr_dbsuspend, "td_thr_dbsuspend", "22", "2.2.5", r##""##);
symbol!(sym_td_thr_event_enable, "td_thr_event_enable", "22", "2.2.5", r##""##);
symbol!(sym_td_thr_event_getmsg, "td_thr_event_getmsg", "22", "2.2.5", r##""##);
symbol!(sym_td_thr_get_info, "td_thr_get_info", "22", "2.2.5", r##""##);
symbol!(sym_td_thr_getfpregs, "td_thr_getfpregs", "22", "2.2.5", r##""##);
symbol!(sym_td_thr_getgregs, "td_thr_getgregs", "22", "2.2.5", r##""##);
symbol!(sym_td_thr_getxregs, "td_thr_getxregs", "22", "2.2.5", r##""##);
symbol!(sym_td_thr_getxregsize, "td_thr_getxregsize", "22", "2.2.5", r##""##);
symbol!(sym_td_thr_set_event, "td_thr_set_event", "22", "2.2.5", r##""##);
symbol!(sym_td_thr_setfpregs, "td_thr_setfpregs", "22", "2.2.5", r##""##);
symbol!(sym_td_thr_setgregs, "td_thr_setgregs", "22", "2.2.5", r##""##);
symbol!(sym_td_thr_setprio, "td_thr_setprio", "22", "2.2.5", r##""##);
symbol!(sym_td_thr_setsigpending, "td_thr_setsigpending", "22", "2.2.5", r##""##);
symbol!(sym_td_thr_setxregs, "td_thr_setxregs", "22", "2.2.5", r##""##);
symbol!(sym_td_thr_sigsetmask, "td_thr_sigsetmask", "22", "2.2.5", r##""##);
symbol!(sym_td_thr_tls_get_addr, "td_thr_tls_get_addr", "22", "2.3", r##""##);
symbol!(sym_td_thr_tlsbase, "td_thr_tlsbase", "22", "2.3.3", r##""##);
symbol!(sym_td_thr_tsd, "td_thr_tsd", "22", "2.2.5", r##""##);
symbol!(sym_td_thr_validate, "td_thr_validate", "22", "2.2.5", r##""##);
#[test]
fn sym_thrd_create() {
    exported("thrd_create");
    conformance(&["s22_lifecycle", "s22_sync"]);
}
#[test]
fn sym_thrd_current() {
    exported("thrd_current");
    conformance(&["s22_lifecycle"]);
}
#[test]
fn sym_thrd_detach() {
    exported("thrd_detach");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_thrd_equal() {
    equal_check();
    exported("thrd_equal");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_thrd_exit() {
    exported("thrd_exit");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_thrd_join() {
    exported("thrd_join");
    conformance(&["s22_lifecycle", "s22_sync"]);
}
#[test]
fn sym_thrd_sleep() {
    exported("thrd_sleep");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_thrd_yield() {
    exported("thrd_yield");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_tss_create() {
    exported("tss_create");
    conformance(&["s22_sync"]);
}
#[test]
fn sym_tss_delete() {
    exported("tss_delete");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_tss_get() {
    exported("tss_get");
    conformance(&["s22_symbols"]);
}
#[test]
fn sym_tss_set() {
    exported("tss_set");
    conformance(&["s22_sync"]);
}
