//! Translation for spec §22: threads and synchronisation (phase 6).
//!
//! Each export converts its arguments and makes one call into `crate::pthread` (lifecycle,
//! attributes, keys, naming, scheduling), `crate::cancel` (cleanup buffers) or the
//! synchronisation objects. Pthread functions return error numbers and leave `errno` alone
//! (§22 notes).
//!
//! **Products only (`export! { runtime ... }`):** creating, joining, detaching and ending
//! threads, and the attribute object they take, are meaningful only in a process Liella
//! started (D-22.10). A test process the host C library started keeps the host's, so that the
//! host's own threads (the test harness's) are never handed Liella's.

use liella_abi::ctypes::{c_char, c_int, c_void};
use liella_abi::layout::{
    __pthread_unwind_buf_t, cnd_t, cpu_set_t, mtx_t, once_flag, pthread_attr_t, pthread_barrier_t, pthread_barrierattr_t, pthread_cond_t, pthread_condattr_t,
    pthread_key_t, pthread_mutex_t, pthread_mutexattr_t, pthread_once_t, pthread_rwlock_t, pthread_rwlockattr_t, pthread_spinlock_t, pthread_t, sem_t,
    sigset_t, struct__pthread_cleanup_buffer, struct_sched_param, thrd_t, tss_t,
};
use liella_kernel::uapi::Timespec;

use crate::cancel;
use crate::conv;
use crate::sync;
use crate::export;
use crate::fork;
use crate::pthread;
use crate::ret;
use crate::strz;

// ------------------------------------------------------------------------------ lifecycle (§22.1)

export! { runtime "GLIBC_2.34";
pub extern "C" fn pthread_create(thread: Option<&mut pthread_t>, attr: *const pthread_attr_t, start: usize, arg: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: attr is NULL or an initialised pthread_attr_t (§22.2).
    ret::errnum_store(thread, pthread::create(unsafe { conv::opt_obj64(attr) }, start, conv::addr(arg)))
}}

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn pthread_exit(value: *mut c_void) -> ! {
    pthread::exit(conv::addr(value))
}}

export! { runtime "GLIBC_2.34";
pub extern "C" fn pthread_join(thread: pthread_t, result: Option<&mut *mut c_void>) -> c_int {
    ret::errnum_if(result, pthread::join_block(thread))
}}

export! { runtime "GLIBC_2.34";
pub extern "C" fn pthread_tryjoin_np(thread: pthread_t, result: Option<&mut *mut c_void>) -> c_int {
    ret::errnum_if(result, pthread::join_try(thread))
}}

export! { runtime "GLIBC_2.34";
pub extern "C" fn pthread_timedjoin_np(thread: pthread_t, result: Option<&mut *mut c_void>, abstime: Option<&Timespec>) -> c_int {
    ret::errnum_if(result, pthread::join_timed(thread, abstime))
}}

export! { runtime "GLIBC_2.34";
pub extern "C" fn pthread_clockjoin_np(thread: pthread_t, result: Option<&mut *mut c_void>, clock: c_int, abstime: Option<&Timespec>) -> c_int {
    ret::errnum_if(result, pthread::join_clock(thread, clock, abstime))
}}

export! { runtime "GLIBC_2.34";
pub extern "C" fn pthread_detach(thread: pthread_t) -> c_int {
    ret::errnum(pthread::detach(thread))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pthread_self() -> pthread_t {
    pthread::self_id()
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pthread_equal(a: pthread_t, b: pthread_t) -> c_int {
    pthread::equal(a, b)
}}

// ------------------------------------------------------------------------------ C11 threads

export! { runtime "GLIBC_2.34";
pub extern "C" fn thrd_create(thr: Option<&mut thrd_t>, func: usize, arg: *mut c_void) -> c_int {
    pthread::thrd_create(thr, func, conv::addr(arg))
}}

export! { runtime "GLIBC_2.34";
pub extern "C" fn thrd_exit(res: c_int) -> ! {
    pthread::thrd_exit(res)
}}

export! { runtime "GLIBC_2.34";
pub extern "C" fn thrd_join(thr: thrd_t, res: Option<&mut c_int>) -> c_int {
    pthread::thrd_join(thr, res)
}}

export! { runtime "GLIBC_2.34";
pub extern "C" fn thrd_detach(thr: thrd_t) -> c_int {
    pthread::thrd_detach(thr)
}}

export! { "GLIBC_2.28";
pub extern "C" fn thrd_current() -> thrd_t {
    pthread::thrd_current()
}}

export! { "GLIBC_2.28";
pub extern "C" fn thrd_equal(a: thrd_t, b: thrd_t) -> c_int {
    pthread::thrd_equal(a, b)
}}

export! { "GLIBC_2.28";
pub extern "C" fn thrd_sleep(duration: Option<&Timespec>, remaining: Option<&mut Timespec>) -> c_int {
    pthread::thrd_sleep(duration, remaining)
}}

export! { "GLIBC_2.28";
pub extern "C" fn thrd_yield() {
    pthread::thrd_yield()
}}

// ------------------------------------------------------------------------------ attributes (§22.2)

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn pthread_attr_init(attr: *mut pthread_attr_t) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_attr_t (§22.2).
    ret::errnum(pthread::attr_init(unsafe { conv::obj64(attr) }))
}}

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn pthread_attr_destroy(attr: *mut pthread_attr_t) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    ret::errnum(pthread::attr_destroy(unsafe { conv::obj64(attr) }))
}}

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn pthread_attr_setdetachstate(attr: *mut pthread_attr_t, state: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    ret::errnum(pthread::attr_setdetachstate(unsafe { conv::obj64(attr) }, state))
}}

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn pthread_attr_getdetachstate(attr: *const pthread_attr_t, state: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    ret::errnum_store(state, pthread::attr_getdetachstate(unsafe { conv::obj64(attr) }))
}}

export! { runtime "GLIBC_2.34";
pub extern "C" fn pthread_attr_setstacksize(attr: *mut pthread_attr_t, size: usize) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    ret::errnum(pthread::attr_setstacksize(unsafe { conv::obj64(attr) }, size))
}}

export! { runtime "GLIBC_2.34";
pub extern "C" fn pthread_attr_getstacksize(attr: *const pthread_attr_t, size: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    ret::errnum_store(size, pthread::attr_getstacksize(unsafe { conv::obj64(attr) }))
}}

export! { runtime "GLIBC_2.34";
pub extern "C" fn pthread_attr_setstack(attr: *mut pthread_attr_t, addr: *mut c_void, size: usize) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    ret::errnum(pthread::attr_setstack(unsafe { conv::obj64(attr) }, conv::addr(addr), size))
}}

export! { runtime "GLIBC_2.34";
pub extern "C" fn pthread_attr_getstack(attr: *const pthread_attr_t, addr: Option<&mut *mut c_void>, size: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    pthread::attr_getstack_out(unsafe { conv::obj64(attr) }, addr, size)
}}

export! { runtime "GLIBC_2.34";
pub extern "C" fn pthread_attr_setstackaddr(attr: *mut pthread_attr_t, addr: *mut c_void) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    ret::errnum(pthread::attr_setstackaddr(unsafe { conv::obj64(attr) }, conv::addr(addr)))
}}

export! { runtime "GLIBC_2.34";
pub extern "C" fn pthread_attr_getstackaddr(attr: *const pthread_attr_t, addr: Option<&mut *mut c_void>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    ret::errnum_store(addr, pthread::attr_getstackaddr(unsafe { conv::obj64(attr) }))
}}

export! { runtime "GLIBC_2.34";
pub extern "C" fn pthread_attr_setguardsize(attr: *mut pthread_attr_t, size: usize) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    ret::errnum(pthread::attr_setguardsize(unsafe { conv::obj64(attr) }, size))
}}

export! { runtime "GLIBC_2.34";
pub extern "C" fn pthread_attr_getguardsize(attr: *const pthread_attr_t, size: Option<&mut usize>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    ret::errnum_store(size, pthread::attr_getguardsize(unsafe { conv::obj64(attr) }))
}}

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn pthread_attr_setschedparam(attr: *mut pthread_attr_t, param: Option<&struct_sched_param>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    ret::errnum(pthread::attr_setschedparam(unsafe { conv::obj64(attr) }, param))
}}

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn pthread_attr_getschedparam(attr: *const pthread_attr_t, param: Option<&mut struct_sched_param>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    ret::errnum_store(param, pthread::attr_getschedparam(unsafe { conv::obj64(attr) }))
}}

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn pthread_attr_setschedpolicy(attr: *mut pthread_attr_t, policy: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    ret::errnum(pthread::attr_setschedpolicy(unsafe { conv::obj64(attr) }, policy))
}}

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn pthread_attr_getschedpolicy(attr: *const pthread_attr_t, policy: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    ret::errnum_store(policy, pthread::attr_getschedpolicy(unsafe { conv::obj64(attr) }))
}}

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn pthread_attr_setinheritsched(attr: *mut pthread_attr_t, inherit: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    ret::errnum(pthread::attr_setinheritsched(unsafe { conv::obj64(attr) }, inherit))
}}

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn pthread_attr_getinheritsched(attr: *const pthread_attr_t, inherit: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    ret::errnum_store(inherit, pthread::attr_getinheritsched(unsafe { conv::obj64(attr) }))
}}

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn pthread_attr_setscope(attr: *mut pthread_attr_t, scope: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    ret::errnum(pthread::attr_setscope(unsafe { conv::obj64(attr) }, scope))
}}

export! { runtime "GLIBC_2.2.5";
pub extern "C" fn pthread_attr_getscope(attr: *const pthread_attr_t, scope: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    ret::errnum_store(scope, pthread::attr_getscope(unsafe { conv::obj64(attr) }))
}}

export! { runtime "GLIBC_2.32";
pub extern "C" fn pthread_attr_setaffinity_np(attr: *mut pthread_attr_t, size: usize, set: *const cpu_set_t) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t; set to size bytes (§22.2).
    ret::errnum(pthread::attr_setaffinity(unsafe { conv::obj64(attr) }, unsafe { conv::buf(set.cast(), size) }))
}}

export! { runtime "GLIBC_2.34";
pub extern "C" fn pthread_attr_getaffinity_np(attr: *const pthread_attr_t, size: usize, set: *mut cpu_set_t) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t; set to size writable bytes.
    ret::errnum(pthread::attr_getaffinity(unsafe { conv::obj64(attr) }, unsafe { conv::buf_mut(set.cast(), size) }))
}}

export! { runtime "GLIBC_2.32";
pub extern "C" fn pthread_attr_setsigmask_np(attr: *mut pthread_attr_t, set: Option<&sigset_t>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    ret::errnum(pthread::attr_setsigmask(unsafe { conv::obj64(attr) }, set))
}}

export! { runtime "GLIBC_2.32";
pub extern "C" fn pthread_attr_getsigmask_np(attr: *const pthread_attr_t, set: Option<&mut sigset_t>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    pthread::attr_getsigmask(unsafe { conv::obj64(attr) }, set)
}}

export! { runtime "GLIBC_2.32";
pub extern "C" fn pthread_getattr_np(thread: pthread_t, attr: *mut pthread_attr_t) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_attr_t the caller later destroys (§22.2).
    ret::errnum(pthread::getattr_np(thread, unsafe { conv::obj64(attr) }))
}}

export! { runtime "GLIBC_2.34";
pub extern "C" fn pthread_getattr_default_np(attr: *mut pthread_attr_t) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_attr_t (§22.2).
    ret::errnum(pthread::getattr_default(unsafe { conv::obj64(attr) }))
}}

export! { runtime "GLIBC_2.34";
pub extern "C" fn pthread_setattr_default_np(attr: *const pthread_attr_t) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to an initialised pthread_attr_t (§22.2).
    ret::errnum(pthread::setattr_default(unsafe { conv::obj64(attr) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_getconcurrency() -> c_int {
    pthread::getconcurrency()
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_setconcurrency(level: c_int) -> c_int {
    ret::errnum(pthread::setconcurrency(level))
}}

// ------------------------------------------------------------------------------ cancellation buffers (§22.3)

export! { "GLIBC_2.34";
pub extern "C" fn __pthread_register_cancel(buf: *mut __pthread_unwind_buf_t) {
    // UNSAFE(slice) SAFETY: buf is the caller's live cleanup buffer (pthread_cleanup_push, §22.3).
    cancel::register(unsafe { conv::obj64(buf) })
}}

export! { "GLIBC_2.34";
pub extern "C" fn __pthread_unregister_cancel(buf: *mut __pthread_unwind_buf_t) {
    // UNSAFE(slice) SAFETY: buf is the caller's registered cleanup buffer (§22.3).
    cancel::unregister(unsafe { conv::obj64(buf) })
}}

export! { "GLIBC_2.34";
pub extern "C" fn __pthread_register_cancel_defer(buf: *mut __pthread_unwind_buf_t) {
    // UNSAFE(slice) SAFETY: buf is the caller's live cleanup buffer (§22.3).
    cancel::register_defer(unsafe { conv::obj64(buf) })
}}

export! { "GLIBC_2.34";
pub extern "C" fn __pthread_unregister_cancel_restore(buf: *mut __pthread_unwind_buf_t) {
    // UNSAFE(slice) SAFETY: buf is the caller's registered cleanup buffer (§22.3).
    cancel::unregister_restore(unsafe { conv::obj64(buf) })
}}

export! { runtime "GLIBC_2.34";
pub extern "C" fn __pthread_unwind_next(buf: *mut __pthread_unwind_buf_t) -> ! {
    // UNSAFE(slice) SAFETY: buf is the cleanup buffer whose handler is running (§22.3).
    cancel::unwind_next(unsafe { conv::obj64(buf) })
}}

export! { "GLIBC_2.34";
pub extern "C" fn _pthread_cleanup_push(buf: *mut struct__pthread_cleanup_buffer, routine: usize, arg: *mut c_void) {
    // UNSAFE(slice) SAFETY: buf is the caller's cleanup record for this frame (§22.3).
    cancel::push_old(unsafe { conv::obj64(buf) }, routine, conv::addr(arg))
}}

export! { "GLIBC_2.34";
pub extern "C" fn _pthread_cleanup_pop(buf: *mut struct__pthread_cleanup_buffer, execute: c_int) {
    // UNSAFE(slice) SAFETY: buf is the caller's registered cleanup record (§22.3).
    cancel::pop_old(unsafe { conv::obj64(buf) }, execute)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _pthread_cleanup_push_defer(buf: *mut struct__pthread_cleanup_buffer, routine: usize, arg: *mut c_void) {
    // UNSAFE(slice) SAFETY: buf is the caller's cleanup record for this frame (§22.3).
    cancel::push_old_defer(unsafe { conv::obj64(buf) }, routine, conv::addr(arg))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn _pthread_cleanup_pop_restore(buf: *mut struct__pthread_cleanup_buffer, execute: c_int) {
    // UNSAFE(slice) SAFETY: buf is the caller's registered cleanup record (§22.3).
    cancel::pop_old_restore(unsafe { conv::obj64(buf) }, execute)
}}

export! { "GLIBC_2.34";
pub extern "C" fn __pthread_cleanup_routine(frame: *mut [usize; 3]) {
    // UNSAFE(slice) SAFETY: frame is the caller's __pthread_cleanup_frame (§22.3).
    cancel::cleanup_routine(unsafe { conv::obj64(frame) })
}}

// ------------------------------------------------------------------------------ keys (§22.6)

export! { "GLIBC_2.34";
pub extern "C" fn pthread_key_create(key: Option<&mut pthread_key_t>, destructor: usize) -> c_int {
    ret::errnum_store(key, pthread::key_create(destructor))
}}

export! { "GLIBC_2.34";
pub extern "C" fn __pthread_key_create(key: Option<&mut pthread_key_t>, destructor: usize) -> c_int {
    ret::errnum_store(key, pthread::key_create(destructor))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_key_delete(key: pthread_key_t) -> c_int {
    ret::errnum(pthread::key_delete(key))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_getspecific(key: pthread_key_t) -> *mut c_void {
    pthread::getspecific(key)
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __pthread_getspecific(key: pthread_key_t) -> *mut c_void {
    pthread::getspecific(key)
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_setspecific(key: pthread_key_t, value: *const c_void) -> c_int {
    ret::errnum(pthread::setspecific(key, conv::addr(value)))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __pthread_setspecific(key: pthread_key_t, value: *const c_void) -> c_int {
    ret::errnum(pthread::setspecific(key, conv::addr(value)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn tss_create(key: Option<&mut tss_t>, destructor: usize) -> c_int {
    pthread::tss_create(key, destructor)
}}

export! { "GLIBC_2.34";
pub extern "C" fn tss_delete(key: tss_t) {
    pthread::tss_delete(key)
}}

export! { "GLIBC_2.34";
pub extern "C" fn tss_get(key: tss_t) -> *mut c_void {
    pthread::tss_get(key)
}}

export! { "GLIBC_2.34";
pub extern "C" fn tss_set(key: tss_t, value: *mut c_void) -> c_int {
    pthread::tss_set(key, conv::addr(value))
}}

// ------------------------------------------------------------------------------ naming, scheduling, affinity (§22.8)

export! { "GLIBC_2.34";
pub extern "C" fn pthread_setname_np(thread: pthread_t, name: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: name is a NUL-terminated string (§22.8).
    ret::errnum(pthread::setname(thread, unsafe { strz::strz(name) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_getname_np(thread: pthread_t, buf: *mut c_char, len: usize) -> c_int {
    // UNSAFE(slice) SAFETY: buf has len writable bytes (§22.8).
    ret::errnum(pthread::getname(thread, unsafe { conv::buf_mut(buf.cast(), len) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pthread_setschedparam(thread: pthread_t, policy: c_int, param: Option<&struct_sched_param>) -> c_int {
    ret::errnum(pthread::setschedparam(thread, policy, param))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pthread_getschedparam(thread: pthread_t, policy: Option<&mut c_int>, param: Option<&mut struct_sched_param>) -> c_int {
    pthread::getschedparam_out(thread, policy, param)
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_setschedprio(thread: pthread_t, prio: c_int) -> c_int {
    ret::errnum(pthread::setschedprio(thread, prio))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pthread_yield() -> c_int {
    pthread::yield_now()
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_setaffinity_np(thread: pthread_t, size: usize, set: *const cpu_set_t) -> c_int {
    // UNSAFE(slice) SAFETY: set has size readable bytes (§22.8).
    ret::errnum(pthread::setaffinity(thread, unsafe { conv::buf(set.cast(), size) }))
}}

export! { "GLIBC_2.32";
pub extern "C" fn pthread_getaffinity_np(thread: pthread_t, size: usize, set: *mut cpu_set_t) -> c_int {
    // UNSAFE(slice) SAFETY: set has size writable bytes (§22.8).
    ret::errnum(pthread::getaffinity(thread, unsafe { conv::buf_mut(set.cast(), size) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_getcpuclockid(thread: pthread_t, clock: Option<&mut c_int>) -> c_int {
    ret::errnum_store(clock, pthread::getcpuclockid(thread))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pthread_kill_other_threads_np() {
    pthread::no_op()
}}

// ------------------------------------------------------------------------------ mutexes (§22.4)

export! { "GLIBC_2.2.5";
pub extern "C" fn pthread_mutex_init(m: *mut pthread_mutex_t, attr: *const pthread_mutexattr_t) -> c_int {
    // UNSAFE(slice) SAFETY: m points to a pthread_mutex_t (initialised or a static initialiser, §22.4). attr is NULL or initialised.
    ret::errnum(sync::mutex_init(unsafe { conv::mutex(m) }, unsafe { conv::opt_obj32(attr) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __pthread_mutex_init(m: *mut pthread_mutex_t, attr: *const pthread_mutexattr_t) -> c_int {
    // UNSAFE(slice) SAFETY: m points to a pthread_mutex_t (initialised or a static initialiser, §22.4). attr is NULL or initialised.
    ret::errnum(sync::mutex_init(unsafe { conv::mutex(m) }, unsafe { conv::opt_obj32(attr) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pthread_mutex_destroy(m: *mut pthread_mutex_t) -> c_int {
    // UNSAFE(slice) SAFETY: m points to a pthread_mutex_t (initialised or a static initialiser, §22.4).
    ret::errnum(sync::mutex_destroy(unsafe { conv::mutex(m) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __pthread_mutex_destroy(m: *mut pthread_mutex_t) -> c_int {
    // UNSAFE(slice) SAFETY: m points to a pthread_mutex_t (initialised or a static initialiser, §22.4).
    ret::errnum(sync::mutex_destroy(unsafe { conv::mutex(m) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pthread_mutex_lock(m: *mut pthread_mutex_t) -> c_int {
    // UNSAFE(slice) SAFETY: m points to a pthread_mutex_t (initialised or a static initialiser, §22.4).
    ret::errnum(sync::mutex_lock_block(unsafe { conv::mutex(m) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __pthread_mutex_lock(m: *mut pthread_mutex_t) -> c_int {
    // UNSAFE(slice) SAFETY: m points to a pthread_mutex_t (initialised or a static initialiser, §22.4).
    ret::errnum(sync::mutex_lock_block(unsafe { conv::mutex(m) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_mutex_trylock(m: *mut pthread_mutex_t) -> c_int {
    // UNSAFE(slice) SAFETY: m points to a pthread_mutex_t (initialised or a static initialiser, §22.4).
    ret::errnum(sync::mutex_trylock(unsafe { conv::mutex(m) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __pthread_mutex_trylock(m: *mut pthread_mutex_t) -> c_int {
    // UNSAFE(slice) SAFETY: m points to a pthread_mutex_t (initialised or a static initialiser, §22.4).
    ret::errnum(sync::mutex_trylock(unsafe { conv::mutex(m) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_mutex_timedlock(m: *mut pthread_mutex_t, abstime: Option<&Timespec>) -> c_int {
    // UNSAFE(slice) SAFETY: m points to a pthread_mutex_t (initialised or a static initialiser, §22.4).
    ret::errnum(sync::mutex_timedlock(unsafe { conv::mutex(m) }, abstime))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_mutex_clocklock(m: *mut pthread_mutex_t, clock: c_int, abstime: Option<&Timespec>) -> c_int {
    // UNSAFE(slice) SAFETY: m points to a pthread_mutex_t (initialised or a static initialiser, §22.4).
    ret::errnum(sync::mutex_clocklock(unsafe { conv::mutex(m) }, clock, abstime))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pthread_mutex_unlock(m: *mut pthread_mutex_t) -> c_int {
    // UNSAFE(slice) SAFETY: m points to a pthread_mutex_t (initialised or a static initialiser, §22.4).
    ret::errnum(sync::mutex_unlock(unsafe { conv::mutex(m) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __pthread_mutex_unlock(m: *mut pthread_mutex_t) -> c_int {
    // UNSAFE(slice) SAFETY: m points to a pthread_mutex_t (initialised or a static initialiser, §22.4).
    ret::errnum(sync::mutex_unlock(unsafe { conv::mutex(m) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_mutex_consistent(m: *mut pthread_mutex_t) -> c_int {
    // UNSAFE(slice) SAFETY: m points to a pthread_mutex_t (initialised or a static initialiser, §22.4).
    ret::errnum(sync::mutex_consistent(unsafe { conv::mutex(m) }))
}}

export! { "GLIBC_2.4";
pub extern "C" fn pthread_mutex_consistent_np(m: *mut pthread_mutex_t) -> c_int {
    // UNSAFE(slice) SAFETY: m points to a pthread_mutex_t (initialised or a static initialiser, §22.4).
    ret::errnum(sync::mutex_consistent(unsafe { conv::mutex(m) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_mutex_getprioceiling(m: *const pthread_mutex_t, ceiling: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: m points to a pthread_mutex_t (initialised or a static initialiser, §22.4).
    ret::errnum_store(ceiling, sync::mutex_getprioceiling(unsafe { conv::mutex(m) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_mutex_setprioceiling(m: *mut pthread_mutex_t, ceiling: c_int, old: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: m points to a pthread_mutex_t (initialised or a static initialiser, §22.4).
    ret::errnum_if(old, sync::mutex_setprioceiling(unsafe { conv::mutex(m) }, ceiling))
}}

// ------------------------------------------------------------------------------ mutex attributes (§22.4)

export! { "GLIBC_2.34";
pub extern "C" fn pthread_mutexattr_init(attr: *mut pthread_mutexattr_t) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_mutexattr_t (§22.4).
    ret::errnum(sync::mutexattr_init(unsafe { conv::obj32(attr) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __pthread_mutexattr_init(attr: *mut pthread_mutexattr_t) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_mutexattr_t (§22.4).
    ret::errnum(sync::mutexattr_init(unsafe { conv::obj32(attr) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_mutexattr_destroy(attr: *mut pthread_mutexattr_t) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_mutexattr_t (§22.4).
    ret::errnum(sync::mutexattr_destroy(unsafe { conv::obj32(attr) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __pthread_mutexattr_destroy(attr: *mut pthread_mutexattr_t) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_mutexattr_t (§22.4).
    ret::errnum(sync::mutexattr_destroy(unsafe { conv::obj32(attr) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_mutexattr_settype(attr: *mut pthread_mutexattr_t, kind: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_mutexattr_t (§22.4).
    ret::errnum(sync::mutexattr_settype(unsafe { conv::obj32(attr) }, kind))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __pthread_mutexattr_settype(attr: *mut pthread_mutexattr_t, kind: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_mutexattr_t (§22.4).
    ret::errnum(sync::mutexattr_settype(unsafe { conv::obj32(attr) }, kind))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pthread_mutexattr_setkind_np(attr: *mut pthread_mutexattr_t, kind: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_mutexattr_t (§22.4).
    ret::errnum(sync::mutexattr_settype(unsafe { conv::obj32(attr) }, kind))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_mutexattr_gettype(attr: *const pthread_mutexattr_t, kind: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_mutexattr_t (§22.4).
    ret::errnum_store(kind, sync::mutexattr_gettype(unsafe { conv::obj32(attr) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pthread_mutexattr_getkind_np(attr: *const pthread_mutexattr_t, kind: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_mutexattr_t (§22.4).
    ret::errnum_store(kind, sync::mutexattr_gettype(unsafe { conv::obj32(attr) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_mutexattr_setpshared(attr: *mut pthread_mutexattr_t, pshared: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_mutexattr_t (§22.4).
    ret::errnum(sync::mutexattr_setpshared(unsafe { conv::obj32(attr) }, pshared))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_mutexattr_getpshared(attr: *const pthread_mutexattr_t, pshared: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_mutexattr_t (§22.4).
    ret::errnum_store(pshared, sync::mutexattr_getpshared(unsafe { conv::obj32(attr) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_mutexattr_setprotocol(attr: *mut pthread_mutexattr_t, protocol: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_mutexattr_t (§22.4).
    ret::errnum(sync::mutexattr_setprotocol(unsafe { conv::obj32(attr) }, protocol))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_mutexattr_getprotocol(attr: *const pthread_mutexattr_t, protocol: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_mutexattr_t (§22.4).
    ret::errnum_store(protocol, sync::mutexattr_getprotocol(unsafe { conv::obj32(attr) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_mutexattr_setprioceiling(attr: *mut pthread_mutexattr_t, ceiling: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_mutexattr_t (§22.4).
    ret::errnum(sync::mutexattr_setprioceiling(unsafe { conv::obj32(attr) }, ceiling))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_mutexattr_getprioceiling(attr: *const pthread_mutexattr_t, ceiling: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_mutexattr_t (§22.4).
    ret::errnum_store(ceiling, sync::mutexattr_getprioceiling(unsafe { conv::obj32(attr) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_mutexattr_setrobust(attr: *mut pthread_mutexattr_t, robust: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_mutexattr_t (§22.4).
    ret::errnum(sync::mutexattr_setrobust(unsafe { conv::obj32(attr) }, robust))
}}

export! { "GLIBC_2.4";
pub extern "C" fn pthread_mutexattr_setrobust_np(attr: *mut pthread_mutexattr_t, robust: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_mutexattr_t (§22.4).
    ret::errnum(sync::mutexattr_setrobust(unsafe { conv::obj32(attr) }, robust))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_mutexattr_getrobust(attr: *const pthread_mutexattr_t, robust: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_mutexattr_t (§22.4).
    ret::errnum_store(robust, sync::mutexattr_getrobust(unsafe { conv::obj32(attr) }))
}}

export! { "GLIBC_2.4";
pub extern "C" fn pthread_mutexattr_getrobust_np(attr: *const pthread_mutexattr_t, robust: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_mutexattr_t (§22.4).
    ret::errnum_store(robust, sync::mutexattr_getrobust(unsafe { conv::obj32(attr) }))
}}

// ------------------------------------------------------------------------------ condition variables (§22.5)

export! { "GLIBC_2.3.2";
pub extern "C" fn pthread_cond_init(c: *mut pthread_cond_t, attr: *const pthread_condattr_t) -> c_int {
    // UNSAFE(slice) SAFETY: c points to a pthread_cond_t (§22.5). attr is NULL or initialised.
    ret::errnum(sync::cond_init(unsafe { conv::obj32(c) }, unsafe { conv::opt_obj32(attr) }))
}}

export! { "GLIBC_2.3.2";
pub extern "C" fn pthread_cond_destroy(c: *mut pthread_cond_t) -> c_int {
    // UNSAFE(slice) SAFETY: c points to a pthread_cond_t (§22.5).
    ret::errnum(sync::cond_destroy(unsafe { conv::obj32(c) }))
}}

export! { "GLIBC_2.3.2";
pub extern "C" fn pthread_cond_wait(c: *mut pthread_cond_t, m: *mut pthread_mutex_t) -> c_int {
    // UNSAFE(slice) SAFETY: c points to a pthread_cond_t (§22.5). m points to a pthread_mutex_t the caller holds.
    ret::errnum(sync::cond_wait(unsafe { conv::obj32(c) }, unsafe { conv::mutex(m) }, None, None, false))
}}

export! { "GLIBC_2.3.2";
pub extern "C" fn pthread_cond_timedwait(c: *mut pthread_cond_t, m: *mut pthread_mutex_t, abstime: Option<&Timespec>) -> c_int {
    // UNSAFE(slice) SAFETY: c points to a pthread_cond_t (§22.5). m points to a pthread_mutex_t the caller holds.
    ret::errnum(sync::cond_wait(unsafe { conv::obj32(c) }, unsafe { conv::mutex(m) }, None, abstime, true))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_cond_clockwait(c: *mut pthread_cond_t, m: *mut pthread_mutex_t, clock: c_int, abstime: Option<&Timespec>) -> c_int {
    // UNSAFE(slice) SAFETY: c points to a pthread_cond_t (§22.5). m points to a pthread_mutex_t the caller holds.
    ret::errnum(sync::cond_wait(unsafe { conv::obj32(c) }, unsafe { conv::mutex(m) }, Some(clock), abstime, true))
}}

export! { "GLIBC_2.3.2";
pub extern "C" fn pthread_cond_signal(c: *mut pthread_cond_t) -> c_int {
    // UNSAFE(slice) SAFETY: c points to a pthread_cond_t (§22.5).
    ret::errnum(sync::cond_signal(unsafe { conv::obj32(c) }))
}}

export! { "GLIBC_2.3.2";
pub extern "C" fn pthread_cond_broadcast(c: *mut pthread_cond_t) -> c_int {
    // UNSAFE(slice) SAFETY: c points to a pthread_cond_t (§22.5).
    ret::errnum(sync::cond_broadcast(unsafe { conv::obj32(c) }))
}}

// The pre-2.3.2 condition variables, `pthread_cond_*@GLIBC_2.2.5` (s22_5_c_1_2; layout and
// mapping in sync.rs, "old condition variables"). Compat-only versions: exported under internal
// names, which the generated `.symver` stubs in `csys/src/machine/compat_aliases.S` (`cargo xtask
// compat-gen`, the `behaviour` rows of `spec/abi/compat-plan.tsv`) give the old version, in libc.so.6 only (as `stime`, D-27.4,
// R15-2); a program linked anew binds the 2.3.2 entries above.
crate::internal_entry! {
/// `pthread_cond_init@GLIBC_2.2.5`.
pub extern "C" fn liella_compat_pthread_cond_init(c: *mut u64, attr: *const pthread_condattr_t) -> c_int {
    // UNSAFE(slice) SAFETY: c points to an old pthread_cond_t (one word). attr is NULL or initialised.
    ret::errnum(sync::old_cond_init(unsafe { conv::obj64(c) }, unsafe { conv::opt_obj32(attr) }))
}}

crate::internal_entry! {
/// `pthread_cond_destroy@GLIBC_2.2.5`.
pub extern "C" fn liella_compat_pthread_cond_destroy(c: *mut u64) -> c_int {
    // UNSAFE(slice) SAFETY: c points to an old pthread_cond_t (one word).
    ret::errnum(sync::old_cond_destroy(unsafe { conv::obj64(c) }))
}}

crate::internal_entry! {
/// `pthread_cond_wait@GLIBC_2.2.5`.
pub extern "C" fn liella_compat_pthread_cond_wait(c: *mut u64, m: *mut pthread_mutex_t) -> c_int {
    // UNSAFE(slice) SAFETY: c points to an old pthread_cond_t. m points to a pthread_mutex_t the caller holds.
    ret::errnum(sync::old_cond_wait(unsafe { conv::obj64(c) }, unsafe { conv::mutex(m) }, None, false))
}}

crate::internal_entry! {
/// `pthread_cond_timedwait@GLIBC_2.2.5`.
pub extern "C" fn liella_compat_pthread_cond_timedwait(c: *mut u64, m: *mut pthread_mutex_t, abstime: Option<&Timespec>) -> c_int {
    // UNSAFE(slice) SAFETY: c points to an old pthread_cond_t. m points to a pthread_mutex_t the caller holds.
    ret::errnum(sync::old_cond_wait(unsafe { conv::obj64(c) }, unsafe { conv::mutex(m) }, abstime, true))
}}

crate::internal_entry! {
/// `pthread_cond_signal@GLIBC_2.2.5`.
pub extern "C" fn liella_compat_pthread_cond_signal(c: *mut u64) -> c_int {
    // UNSAFE(slice) SAFETY: c points to an old pthread_cond_t (one word).
    ret::errnum(sync::old_cond_signal(unsafe { conv::obj64(c) }, false))
}}

crate::internal_entry! {
/// `pthread_cond_broadcast@GLIBC_2.2.5`.
pub extern "C" fn liella_compat_pthread_cond_broadcast(c: *mut u64) -> c_int {
    // UNSAFE(slice) SAFETY: c points to an old pthread_cond_t (one word).
    ret::errnum(sync::old_cond_signal(unsafe { conv::obj64(c) }, true))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pthread_condattr_init(attr: *mut pthread_condattr_t) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_condattr_t (§22.5).
    ret::errnum(sync::condattr_init(unsafe { conv::obj32(attr) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pthread_condattr_destroy(attr: *mut pthread_condattr_t) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_condattr_t (§22.5).
    ret::errnum(sync::condattr_destroy(unsafe { conv::obj32(attr) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_condattr_setclock(attr: *mut pthread_condattr_t, clock: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_condattr_t (§22.5).
    ret::errnum(sync::condattr_setclock(unsafe { conv::obj32(attr) }, clock))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_condattr_getclock(attr: *const pthread_condattr_t, clock: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_condattr_t (§22.5).
    ret::errnum_store(clock, sync::condattr_getclock(unsafe { conv::obj32(attr) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_condattr_setpshared(attr: *mut pthread_condattr_t, pshared: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_condattr_t (§22.5).
    ret::errnum(sync::condattr_setpshared(unsafe { conv::obj32(attr) }, pshared))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_condattr_getpshared(attr: *const pthread_condattr_t, pshared: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_condattr_t (§22.5).
    ret::errnum_store(pshared, sync::condattr_getpshared(unsafe { conv::obj32(attr) }))
}}

// ------------------------------------------------------------------------------ read-write locks (§22.5)

export! { "GLIBC_2.34";
pub extern "C" fn pthread_rwlock_init(l: *mut pthread_rwlock_t, attr: *const pthread_rwlockattr_t) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_rwlock_t (§22.5). attr is NULL or initialised.
    ret::errnum(sync::rwlock_init(unsafe { conv::obj32(l) }, unsafe { conv::opt_obj32(attr) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __pthread_rwlock_init(l: *mut pthread_rwlock_t, attr: *const pthread_rwlockattr_t) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_rwlock_t (§22.5). attr is NULL or initialised.
    ret::errnum(sync::rwlock_init(unsafe { conv::obj32(l) }, unsafe { conv::opt_obj32(attr) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_rwlock_destroy(l: *mut pthread_rwlock_t) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_rwlock_t (§22.5).
    ret::errnum(sync::rwlock_destroy(unsafe { conv::obj32(l) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __pthread_rwlock_destroy(l: *mut pthread_rwlock_t) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_rwlock_t (§22.5).
    ret::errnum(sync::rwlock_destroy(unsafe { conv::obj32(l) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_rwlock_rdlock(l: *mut pthread_rwlock_t) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_rwlock_t (§22.5).
    ret::errnum(sync::rwlock_rdlock(unsafe { conv::obj32(l) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __pthread_rwlock_rdlock(l: *mut pthread_rwlock_t) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_rwlock_t (§22.5).
    ret::errnum(sync::rwlock_rdlock(unsafe { conv::obj32(l) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_rwlock_tryrdlock(l: *mut pthread_rwlock_t) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_rwlock_t (§22.5).
    ret::errnum(sync::rwlock_tryrdlock(unsafe { conv::obj32(l) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __pthread_rwlock_tryrdlock(l: *mut pthread_rwlock_t) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_rwlock_t (§22.5).
    ret::errnum(sync::rwlock_tryrdlock(unsafe { conv::obj32(l) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_rwlock_wrlock(l: *mut pthread_rwlock_t) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_rwlock_t (§22.5).
    ret::errnum(sync::rwlock_wrlock(unsafe { conv::obj32(l) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __pthread_rwlock_wrlock(l: *mut pthread_rwlock_t) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_rwlock_t (§22.5).
    ret::errnum(sync::rwlock_wrlock(unsafe { conv::obj32(l) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_rwlock_trywrlock(l: *mut pthread_rwlock_t) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_rwlock_t (§22.5).
    ret::errnum(sync::rwlock_trywrlock(unsafe { conv::obj32(l) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __pthread_rwlock_trywrlock(l: *mut pthread_rwlock_t) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_rwlock_t (§22.5).
    ret::errnum(sync::rwlock_trywrlock(unsafe { conv::obj32(l) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_rwlock_unlock(l: *mut pthread_rwlock_t) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_rwlock_t (§22.5).
    ret::errnum(sync::rwlock_unlock(unsafe { conv::obj32(l) }))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn __pthread_rwlock_unlock(l: *mut pthread_rwlock_t) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_rwlock_t (§22.5).
    ret::errnum(sync::rwlock_unlock(unsafe { conv::obj32(l) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_rwlock_timedrdlock(l: *mut pthread_rwlock_t, abstime: Option<&Timespec>) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_rwlock_t (§22.5).
    ret::errnum(sync::rwlock_timedrdlock(unsafe { conv::obj32(l) }, None, abstime))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_rwlock_clockrdlock(l: *mut pthread_rwlock_t, clock: c_int, abstime: Option<&Timespec>) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_rwlock_t (§22.5).
    ret::errnum(sync::rwlock_timedrdlock(unsafe { conv::obj32(l) }, Some(clock), abstime))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_rwlock_timedwrlock(l: *mut pthread_rwlock_t, abstime: Option<&Timespec>) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_rwlock_t (§22.5).
    ret::errnum(sync::rwlock_timedwrlock(unsafe { conv::obj32(l) }, None, abstime))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_rwlock_clockwrlock(l: *mut pthread_rwlock_t, clock: c_int, abstime: Option<&Timespec>) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_rwlock_t (§22.5).
    ret::errnum(sync::rwlock_timedwrlock(unsafe { conv::obj32(l) }, Some(clock), abstime))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_rwlockattr_init(attr: *mut pthread_rwlockattr_t) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_rwlockattr_t (§22.5).
    ret::errnum(sync::rwlockattr_init(unsafe { conv::obj32(attr) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_rwlockattr_destroy(attr: *mut pthread_rwlockattr_t) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_rwlockattr_t (§22.5).
    ret::errnum(sync::rwlockattr_destroy(unsafe { conv::obj32(attr) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_rwlockattr_setkind_np(attr: *mut pthread_rwlockattr_t, kind: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_rwlockattr_t (§22.5).
    ret::errnum(sync::rwlockattr_setkind(unsafe { conv::obj32(attr) }, kind))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_rwlockattr_getkind_np(attr: *const pthread_rwlockattr_t, kind: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_rwlockattr_t (§22.5).
    ret::errnum_store(kind, sync::rwlockattr_getkind(unsafe { conv::obj32(attr) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_rwlockattr_setpshared(attr: *mut pthread_rwlockattr_t, pshared: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_rwlockattr_t (§22.5).
    ret::errnum(sync::rwlockattr_setpshared(unsafe { conv::obj32(attr) }, pshared))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_rwlockattr_getpshared(attr: *const pthread_rwlockattr_t, pshared: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_rwlockattr_t (§22.5).
    ret::errnum_store(pshared, sync::rwlockattr_getpshared(unsafe { conv::obj32(attr) }))
}}

// ------------------------------------------------------------------------------ spin locks and barriers (§22.5)

export! { "GLIBC_2.34";
pub extern "C" fn pthread_spin_init(l: *mut pthread_spinlock_t, pshared: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_spinlock_t (§22.5).
    ret::errnum(sync::spin_init(unsafe { conv::obj32(l) }, pshared))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_spin_destroy(l: *mut pthread_spinlock_t) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_spinlock_t (§22.5).
    ret::errnum(sync::spin_destroy(unsafe { conv::obj32(l) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_spin_lock(l: *mut pthread_spinlock_t) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_spinlock_t (§22.5).
    ret::errnum(sync::spin_lock(unsafe { conv::obj32(l) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_spin_trylock(l: *mut pthread_spinlock_t) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_spinlock_t (§22.5).
    ret::errnum(sync::spin_trylock(unsafe { conv::obj32(l) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_spin_unlock(l: *mut pthread_spinlock_t) -> c_int {
    // UNSAFE(slice) SAFETY: l points to a pthread_spinlock_t (§22.5).
    ret::errnum(sync::spin_unlock(unsafe { conv::obj32(l) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_barrier_init(b: *mut pthread_barrier_t, attr: *const pthread_barrierattr_t, count: u32) -> c_int {
    // UNSAFE(slice) SAFETY: b points to a pthread_barrier_t (§22.5). attr is NULL or initialised.
    ret::errnum(sync::barrier_init(unsafe { conv::obj32(b) }, unsafe { conv::opt_obj32(attr) }, count))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_barrier_destroy(b: *mut pthread_barrier_t) -> c_int {
    // UNSAFE(slice) SAFETY: b points to a pthread_barrier_t (§22.5).
    ret::errnum(sync::barrier_destroy(unsafe { conv::obj32(b) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_barrier_wait(b: *mut pthread_barrier_t) -> c_int {
    // UNSAFE(slice) SAFETY: b points to a pthread_barrier_t (§22.5).
    sync::barrier_wait(unsafe { conv::obj32(b) })
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_barrierattr_init(attr: *mut pthread_barrierattr_t) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_barrierattr_t (§22.5).
    ret::errnum(sync::barrierattr_init(unsafe { conv::obj32(attr) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_barrierattr_destroy(attr: *mut pthread_barrierattr_t) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_barrierattr_t (§22.5).
    ret::errnum(sync::barrierattr_destroy(unsafe { conv::obj32(attr) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_barrierattr_setpshared(attr: *mut pthread_barrierattr_t, pshared: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_barrierattr_t (§22.5).
    ret::errnum(sync::barrierattr_setpshared(unsafe { conv::obj32(attr) }, pshared))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_barrierattr_getpshared(attr: *const pthread_barrierattr_t, pshared: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: attr points to a pthread_barrierattr_t (§22.5).
    ret::errnum_store(pshared, sync::barrierattr_getpshared(unsafe { conv::obj32(attr) }))
}}

// ------------------------------------------------------------------------------ once (§22.5)

// The exported entries are machine/once.S: the routine runs in a frame a C++ exception can
// unwind through (S11).

export! { asm "GLIBC_2.34";
pub extern "C" fn pthread_once(once: *mut pthread_once_t, init: usize) -> c_int {
    // UNSAFE(slice) SAFETY: once points to a pthread_once_t (§22.5).
    ret::errnum(sync::once(unsafe { conv::obj32(once) }, init))
}}

export! { asm "GLIBC_2.2.5";
pub extern "C" fn __pthread_once(once: *mut pthread_once_t, init: usize) -> c_int {
    // UNSAFE(slice) SAFETY: once points to a pthread_once_t (§22.5).
    ret::errnum(sync::once(unsafe { conv::obj32(once) }, init))
}}

export! { asm "GLIBC_2.34";
pub extern "C" fn call_once(flag: *mut once_flag, func: usize) {
    // UNSAFE(slice) SAFETY: flag points to a once_flag (§22.5).
    sync::call_once(unsafe { conv::obj32(flag) }, func)
}}

// The functions machine/once.S calls (hidden there; unmangled `liella_` names, never exported).

// UNSAFE(export): called by liella_machine_once (csys/src/machine/once.S) before the routine;
// permitted here by ruling #406 (xtask policy ONCE_HELPERS).
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
/// Before the routine (`sync::once_begin`).
pub extern "C" fn liella_once_begin(once: *mut pthread_once_t, init: usize, record: *mut [usize; 4]) -> c_int {
    // UNSAFE(slice) SAFETY: once is the C caller's pthread_once_t (§22.5).
    let w = unsafe { conv::obj32(once) };
    // UNSAFE(slice) SAFETY: record is the 4-word buffer in liella_machine_once's live frame.
    sync::once_begin(w, init, unsafe { conv::obj64(record) })
}

// UNSAFE(export): called by liella_machine_once (csys/src/machine/once.S) after the routine
// returned; permitted here by ruling #406 (xtask policy ONCE_HELPERS).
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
#[allow(clippy::not_unsafe_ptr_arg_deref)]
/// After the routine returned (`sync::once_end`).
pub extern "C" fn liella_once_end(once: *mut pthread_once_t, record: *mut [usize; 4]) {
    // UNSAFE(slice) SAFETY: once is the object liella_once_begin made running.
    let w = unsafe { conv::obj32(once) };
    // UNSAFE(slice) SAFETY: record is the buffer liella_once_begin registered in this frame.
    sync::once_end(w, unsafe { conv::obj64(record) })
}

// UNSAFE(export): the personality the unwinder calls for liella_machine_once's frame (named by
// csys/src/machine/once.S's call-frame information); permitted here by ruling #406.
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
/// The personality routine of liella_machine_once's frame (`sync::once_personality`).
pub extern "C" fn liella_once_personality(version: c_int, actions: c_int, _class: u64, _exception: usize, _context: usize) -> c_int {
    sync::once_personality(version, actions)
}

// The functions machine/guard.S calls (OD-6 revised: library code that calls C callbacks).

/// The state of one [`guarded`] call, in the caller's frame.
struct GuardFrame<'a, S, B, R> {
    state: &'a mut S,
    body: Option<B>,
    undo: fn(&mut S),
    out: Option<R>,
}

// UNSAFE(slice): machine/guard.S hands `guarded`'s frame back as an address; a reference to it
// needs a pointer dereference.
#[allow(unsafe_code)]
extern "C" fn guard_run<S, B: FnOnce(&mut S) -> R, R>(f: usize) -> i64 {
    // UNSAFE(slice) SAFETY: `f` is the address of the live `GuardFrame` that `guarded` passed to
    // liella_machine_guard, which calls this once while `guarded` waits for it.
    let f = unsafe { &mut *core::ptr::with_exposed_provenance_mut::<GuardFrame<'_, S, B, R>>(f) };
    if let Some(b) = f.body.take() {
        f.out = Some(b(&mut *f.state));
    }
    0
}

// UNSAFE(slice): as `guard_run` (the frame's address, from the guard record).
#[allow(unsafe_code)]
extern "C" fn guard_undo<S, B, R>(f: usize) {
    // UNSAFE(slice) SAFETY: `f` is the address of the `GuardFrame` whose body an unwind (or a cancellation)
    // is leaving: its memory is intact (the unwinder runs below it) and the body's frames,
    // which borrowed the state, are dead.
    let f = unsafe { &mut *core::ptr::with_exposed_provenance_mut::<GuardFrame<'_, S, B, R>>(f) };
    (f.undo)(&mut *f.state);
}

/// Run `body(state)` in machine/guard.S's frame (OD-6 revised, S11): a C++ exception (or a forced
/// unwind) thrown by a C callback the body calls reaches the C caller, as in glibc, and `undo`
/// runs on the way, during the unwinder's cleanup phase, to put `state` (what the body would
/// otherwise have cleaned up or completed) back in order. The body's Rust frames have no
/// landing pads (the products are `panic = "abort"`, the callbacks plain `extern "C"`), so the
/// unwinder passes them by their call-frame information alone (D-14.6).
#[allow(clippy::as_conversions)] // function pointers' addresses: the only spelling Rust has
pub(crate) fn guarded<S, R>(state: &mut S, body: impl FnOnce(&mut S) -> R, undo: fn(&mut S)) -> Option<R> {
    fn go<S, B: FnOnce(&mut S) -> R, R>(state: &mut S, body: B, undo: fn(&mut S)) -> Option<R> {
        let mut fr = GuardFrame { state, body: Some(body), undo, out: None };
        let run = guard_run::<S, B, R> as extern "C" fn(usize) -> i64 as usize;
        let und = guard_undo::<S, B, R> as extern "C" fn(usize) as usize;
        let _ = crate::ffi::liella_machine_guard(run, core::ptr::from_mut(&mut fr).expose_provenance(), und);
        fr.out
    }
    go(state, body, undo)
}


export! { local "liella_guard_begin";
/// Before the body (`cancel::guard_begin`).
pub extern "C" fn liella_guard_begin(record: *mut [usize; 4], undo: usize, ctx: usize) {
    // UNSAFE(slice) SAFETY: record is the 4-word buffer in liella_machine_guard's live frame.
    cancel::guard_begin(unsafe { conv::obj64(record) }, undo, ctx)
}}

export! { local "liella_guard_end";
/// After the body returned (`cancel::guard_end`).
pub extern "C" fn liella_guard_end(record: *mut [usize; 4]) {
    // UNSAFE(slice) SAFETY: record is the buffer liella_guard_begin registered in this frame.
    cancel::guard_end(unsafe { conv::obj64(record) })
}}

export! { local "liella_guard_personality";
/// The personality routine of liella_machine_guard's frame (`cancel::guard_personality`).
pub extern "C" fn liella_guard_personality(version: c_int, actions: c_int, _class: u64, _exception: usize, _context: usize) -> c_int {
    cancel::guard_personality(version, actions)
}}

// ------------------------------------------------------------------------------ semaphores (§22.7; errno convention)

export! { "GLIBC_2.34";
pub extern "C" fn sem_init(s: *mut sem_t, pshared: c_int, value: u32) -> c_int {
    // UNSAFE(slice) SAFETY: s points to a sem_t (§22.7).
    ret::zero(sync::sem_init(unsafe { conv::obj32(s) }, pshared, value))
}}

export! { "GLIBC_2.34";
pub extern "C" fn sem_destroy(s: *mut sem_t) -> c_int {
    // UNSAFE(slice) SAFETY: s points to a sem_t (§22.7).
    ret::zero(sync::sem_destroy(unsafe { conv::obj32(s) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn sem_post(s: *mut sem_t) -> c_int {
    // UNSAFE(slice) SAFETY: s points to a sem_t (§22.7).
    ret::zero(sync::sem_post(unsafe { conv::obj32(s) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn sem_wait(s: *mut sem_t) -> c_int {
    // UNSAFE(slice) SAFETY: s points to a sem_t (§22.7).
    ret::zero(sync::sem_wait(unsafe { conv::obj32(s) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn sem_trywait(s: *mut sem_t) -> c_int {
    // UNSAFE(slice) SAFETY: s points to a sem_t (§22.7).
    ret::zero(sync::sem_trywait(unsafe { conv::obj32(s) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn sem_timedwait(s: *mut sem_t, abstime: Option<&Timespec>) -> c_int {
    // UNSAFE(slice) SAFETY: s points to a sem_t (§22.7).
    ret::zero(sync::sem_timedwait(unsafe { conv::obj32(s) }, None, abstime))
}}

export! { "GLIBC_2.34";
pub extern "C" fn sem_clockwait(s: *mut sem_t, clock: c_int, abstime: Option<&Timespec>) -> c_int {
    // UNSAFE(slice) SAFETY: s points to a sem_t (§22.7).
    ret::zero(sync::sem_timedwait(unsafe { conv::obj32(s) }, Some(clock), abstime))
}}

export! { "GLIBC_2.34";
pub extern "C" fn sem_getvalue(s: *mut sem_t, value: Option<&mut c_int>) -> c_int {
    // UNSAFE(slice) SAFETY: s points to a sem_t (§22.7).
    ret::store(value, sync::sem_getvalue(unsafe { conv::obj32(s) }))
}}

export! { "LIELLA_PRIVATE";
pub extern "C" fn liella_sem_open4(name: *const c_char, oflag: c_int, mode: u32, value: u32) -> *mut sem_t {
    // UNSAFE(slice) SAFETY: name is a NUL-terminated string (§22.7).
    ret::null(sync::sem_open(unsafe { strz::strz(name) }, oflag, mode, value))
}}

export! { "GLIBC_2.34";
pub extern "C" fn sem_close(s: *mut sem_t) -> c_int {
    ret::zero(sync::sem_close(conv::addr(s)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn sem_unlink(name: *const c_char) -> c_int {
    // UNSAFE(slice) SAFETY: name is a NUL-terminated string (§22.7).
    ret::zero(sync::sem_unlink(unsafe { strz::strz(name) }))
}}

// ------------------------------------------------------------------------------ C11 mutexes and condition variables (§22.4, §22.5; D-22.3)

export! { "GLIBC_2.34";
pub extern "C" fn mtx_init(m: *mut mtx_t, kind: c_int) -> c_int {
    // UNSAFE(slice) SAFETY: m points to an mtx_t (§22.4).
    sync::mtx_init(unsafe { conv::mutex(m) }, kind)
}}

export! { "GLIBC_2.34";
pub extern "C" fn mtx_lock(m: *mut mtx_t) -> c_int {
    // UNSAFE(slice) SAFETY: m points to an initialised mtx_t (§22.4).
    sync::c11(sync::mutex_lock_block(unsafe { conv::mutex(m) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn mtx_trylock(m: *mut mtx_t) -> c_int {
    // UNSAFE(slice) SAFETY: m points to an initialised mtx_t (§22.4).
    sync::c11(sync::mutex_trylock(unsafe { conv::mutex(m) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn mtx_timedlock(m: *mut mtx_t, abstime: Option<&Timespec>) -> c_int {
    // UNSAFE(slice) SAFETY: m points to an initialised mtx_t (§22.4).
    sync::c11(sync::mutex_timedlock(unsafe { conv::mutex(m) }, abstime))
}}

export! { "GLIBC_2.34";
pub extern "C" fn mtx_unlock(m: *mut mtx_t) -> c_int {
    // UNSAFE(slice) SAFETY: m points to an initialised mtx_t (§22.4).
    sync::c11(sync::mutex_unlock(unsafe { conv::mutex(m) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn mtx_destroy(m: *mut mtx_t) {
    // UNSAFE(slice) SAFETY: m points to an mtx_t (§22.4).
    sync::mtx_destroy(unsafe { conv::mutex(m) })
}}

export! { "GLIBC_2.34";
pub extern "C" fn cnd_init(c: *mut cnd_t) -> c_int {
    // UNSAFE(slice) SAFETY: c points to a cnd_t (§22.5).
    sync::c11(sync::cond_init(unsafe { conv::obj32(c) }, Ok(None)))
}}

export! { "GLIBC_2.34";
pub extern "C" fn cnd_signal(c: *mut cnd_t) -> c_int {
    // UNSAFE(slice) SAFETY: c points to an initialised cnd_t (§22.5).
    sync::c11(sync::cond_signal(unsafe { conv::obj32(c) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn cnd_broadcast(c: *mut cnd_t) -> c_int {
    // UNSAFE(slice) SAFETY: c points to an initialised cnd_t (§22.5).
    sync::c11(sync::cond_broadcast(unsafe { conv::obj32(c) }))
}}

export! { "GLIBC_2.34";
pub extern "C" fn cnd_wait(c: *mut cnd_t, m: *mut mtx_t) -> c_int {
    // UNSAFE(slice) SAFETY: c points to an initialised cnd_t, m to an mtx_t the caller holds.
    sync::cnd_wait(unsafe { conv::obj32(c) }, unsafe { conv::mutex(m) }, None, false)
}}

export! { "GLIBC_2.34";
pub extern "C" fn cnd_timedwait(c: *mut cnd_t, m: *mut mtx_t, abstime: Option<&Timespec>) -> c_int {
    // UNSAFE(slice) SAFETY: c points to an initialised cnd_t, m to an mtx_t the caller holds.
    sync::cnd_wait(unsafe { conv::obj32(c) }, unsafe { conv::mutex(m) }, abstime, true)
}}

export! { "GLIBC_2.34";
pub extern "C" fn cnd_destroy(c: *mut cnd_t) {
    // UNSAFE(slice) SAFETY: c points to a cnd_t (§22.5).
    sync::cnd_destroy(unsafe { conv::obj32(c) })
}}

// ------------------------------------------------------------------------------ cancellation and signals (§22.3, §22.8)

export! { "GLIBC_2.34";
pub extern "C" fn pthread_cancel(thread: pthread_t) -> c_int {
    ret::errnum(pthread::cancel(thread))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pthread_setcancelstate(state: c_int, old: Option<&mut c_int>) -> c_int {
    ret::errnum_if(old, cancel::set_state(state))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pthread_setcanceltype(kind: c_int, old: Option<&mut c_int>) -> c_int {
    ret::errnum_if(old, cancel::set_type(kind))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_testcancel() {
    cancel::test_cancel()
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_kill(thread: pthread_t, sig: c_int) -> c_int {
    ret::errnum(pthread::kill(thread, sig))
}}

export! { "GLIBC_2.34";
pub extern "C" fn pthread_sigqueue(thread: pthread_t, sig: c_int, value: usize) -> c_int {
    ret::errnum(pthread::sigqueue(thread, sig, value))
}}

export! { "GLIBC_2.2.5";
pub extern "C" fn pthread_atfork(prepare: usize, parent: usize, child: usize) -> c_int {
    ret::errnum(fork::register(prepare, parent, child))
}}
