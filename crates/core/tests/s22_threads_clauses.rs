// Specified from spec §22 (phase 6); libthread_db (§22.10) is phase 14, the compat condition
// variable (§22.5) phase 15.
// Specify a test by replacing its stub with a real #[test] fn of the SAME NAME (see crates/spectest).
// Once a file contains specified tests, `cargo xtask stubs` stops rewriting it and only checks coverage.

//! Clause tests for spec §22 (67 clauses).

#![allow(clippy::unwrap_used, missing_docs)]

use spectest::clause;

/// Build and run C conformance programs against the products (`conformance/run.sh`).
fn conformance(names: &[&str]) {
    static RUNNER: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = RUNNER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let products = ["target/liella/libc.a", "target/liella/liella-crt1.o"];
    assert!(products.iter().all(|p| root.join(p).exists()), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let o = std::process::Command::new("sh").arg(root.join("conformance/run.sh")).args(names).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

/// §22 Notes: Since 2.34 all of `libpthread` is in `libc.so.6`. `libpthread.so.0` remains as an empty placeholder (⚑ COMPAT).
#[test]
fn s22_n_1() {
    // The products ship libpthread placeholders: -lpthread and DT_NEEDED libpthread.so.0 resolve.
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    conformance(&["s22_lifecycle"]);
    for p in ["target/liella/libpthread.so.0", "target/liella/libpthread.a"] {
        assert!(root.join(p).exists(), "{p} is built by `cargo xtask so`");
    }
    let o = std::process::Command::new("readelf").arg("-d").arg(root.join("target/liella/libpthread.so.0")).output().unwrap();
    let text = String::from_utf8_lossy(&o.stdout);
    assert!(text.contains("[libpthread.so.0]"), "its soname");
    assert!(!text.contains("NEEDED"), "empty: it needs nothing and defines nothing");
}
/// §22 Notes: Pthread functions return **error numbers** and do not set `errno` (C0.1). The exceptions are `sem_*`, which use `errno`.
#[test]
fn s22_n_2() {
    conformance(&["s22_more", "s22_sync"]);
}
/// §22 Notes: **Kernel (section-wide).** Thread synchronization rests on one primitive, the **futex** (`futex`/`futex_time64`), with operations `FUTEX_WAIT`, `FUTEX_WAKE`, `FUTEX_WAIT_BITSET` (absolute timeouts on 
#[test]
fn s22_n_3() {
    conformance(&["s22_sync", "s22_lifecycle"]);
}
/// §22.1 Contract: **`pthread_create(&t, attr, fn, arg)`** starts a new thread in the process running `fn(arg)`. The new thread inherits the creator's signal mask (unless set by attribute), its floating-point environmen
#[test]
fn s22_1_c_1() {
    conformance(&["s22_lifecycle", "s22_more"]);
}
/// §22.1 Contract: **`pthread_exit(v)`** terminates the calling thread. It runs cancellation cleanup handlers, then TLS destructors (§22.6), then makes `v` available to `join`. When the last thread exits, the process ex
#[test]
fn s22_1_c_2() {
    conformance(&["s22_cancel", "s22_last_exit"]);
}
/// §22.1 Contract: **`pthread_join(t, &v)`** waits for `t` to terminate and releases it. The `_np` forms try, time out (on `CLOCK_REALTIME`), or time out on a chosen clock.
#[test]
fn s22_1_c_3() {
    conformance(&["s22_lifecycle"]);
}
/// §22.1 Contract: **`pthread_detach(t)`** means `t`'s resources are released at termination without a join.
#[test]
fn s22_1_c_4() {
    conformance(&["s22_lifecycle"]);
}
/// §22.1 Contract: **`pthread_self`/`pthread_equal`** identify threads. `pthread_t` values of terminated and released threads may be reused.
#[test]
fn s22_1_c_5() {
    conformance(&["s22_lifecycle"]);
}
/// §22.1 Contract: **The C11 `thrd_*` functions** have the same semantics with C11 result codes (`thrd_success`, `thrd_nomem`, `thrd_timedout`, `thrd_busy`, `thrd_error`). `thrd_sleep` returns -1 on interruption (with t
#[test]
fn s22_1_c_6() {
    conformance(&["s22_lifecycle", "s22_sync"]);
}
/// §22.1 Contract: **`__libc_single_threaded`** (2.32) is a `char` that is non-zero while the process has never created a thread. Libraries use it to skip atomic operations.
#[test]
fn s22_1_c_7() {
    conformance(&["s22_lifecycle"]);
}
/// §22.1 Failure: `EAGAIN`: thread or memory limits.
#[test]
fn s22_1_f_1() {
    conformance(&["s22_more"]);
}
/// §22.1 Failure: `EINVAL`: a bad attribute.
#[test]
fn s22_1_f_2() {
    conformance(&["s22_lifecycle"]);
}
/// §22.1 Failure: `EPERM`: a scheduling policy that is not permitted.
#[test]
fn s22_1_f_3() {
    conformance(&["s22_more"]);
}
/// §22.1 Failure: `EDEADLK`: joining self or a join cycle.
#[test]
fn s22_1_f_4() {
    conformance(&["s22_lifecycle", "s22_more"]);
}
/// §22.1 Failure: `ESRCH`/`EINVAL`: the thread is not joinable. Joining a detached thread, or joining twice, is undefined behaviour.
#[test]
fn s22_1_f_5() {
    // Joining a detached thread is reported as EINVAL.
    conformance(&["s22_more"]);
}
/// §22.2 Contract: Build an attribute object that controls a new thread's:
#[test]
fn s22_2_c_0() {
    conformance(&["s22_lifecycle", "s22_more"]);
}
/// §22.2 Contract: detached state;
#[test]
fn s22_2_c_1() {
    conformance(&["s22_lifecycle"]);
}
/// §22.2 Contract: stack: size (at least `PTHREAD_STACK_MIN`, which since 2.34 is `sysconf(_SC_THREAD_STACK_MIN)` and not a constant), a caller-provided stack, and the guard size (rounded to pages; 0 disables the guard)
#[test]
fn s22_2_c_2() {
    conformance(&["s22_lifecycle"]);
}
/// §22.2 Contract: scheduling: policy and parameters, inherited or explicit. Only `PTHREAD_SCOPE_SYSTEM` is supported, and `PROCESS` gives `ENOTSUP`.
#[test]
fn s22_2_c_3() {
    conformance(&["s22_lifecycle", "s22_more"]);
}
/// §22.2 Contract: CPU affinity and initial signal mask. `pthread_getattr_np(t, &a)` returns the *actual* attributes of a running thread, including its stack bounds. For the main thread, these come from `/proc/self/maps
#[test]
fn s22_2_c_4() {
    conformance(&["s22_lifecycle", "s22_more"]);
}
/// §22.2 Guarantees: The `RLIMIT_STACK` soft limit at startup (8 MiB typically), or 2 MiB if that is unlimited.
#[test]
fn s22_2_g_0() {
    conformance(&["s22_more"]);
}
/// §22.3 Contract: Request that a target thread terminate.
#[test]
fn s22_3_c_0() {
    conformance(&["s22_cancel"]);
}
/// §22.3 Contract: In the **deferred** mode (the default), the target acts on the request at the next cancellation point (C0.2), or at `pthread_testcancel`.
#[test]
fn s22_3_c_1() {
    conformance(&["s22_cancel", "s22_cancel_points"]);
}
/// §22.3 Contract: In the **asynchronous** mode, the target acts at any instruction. Only async-cancel-safe code may run with this enabled.
#[test]
fn s22_3_c_2() {
    // Asynchronous requests act at the next safe point outside Liella code (owner ruling, D-22.11).
    conformance(&["s22_cancel"]);
}
/// §22.3 Contract: **Cancellation state** may be disabled, in which case requests stay pending.
#[test]
fn s22_3_c_3() {
    conformance(&["s22_cancel"]);
}
/// §22.3 Contract: **Acting on cancellation** runs the cleanup handlers in LIFO order and TLS destructors. In C++, it unwinds the stack with a forced unwind (`abi::__forced_unwind`). The thread then exits with `PTHREAD_
#[test]
fn s22_3_c_4() {
    // C cleanup handlers and TLS destructors; the C++ forced unwind (libgcc_s through dlopen) is interim until phase 14 (D-22.11).
    conformance(&["s22_cancel"]);
}
/// §22.3 Guarantees: If a cancellation point is cancelled *before* the underlying syscall had a side effect, the side effect did not happen. For `close`, the descriptor is still closed (⚑ KERN). For `read`/`write` cancell
#[test]
fn s22_3_g_0() {
    // Before or after the side effect, never between (s22_cancel_race).
    conformance(&["s22_cancel", "s22_more", "s22_cancel_race"]);
}
/// §22.4 Contract: Mutual exclusion. At most one thread owns a mutex.
#[test]
fn s22_4_c_0() {
    conformance(&["s22_sync"]);
}
/// §22.4 Contract: **Types:** `NORMAL` (self-deadlock on relock), `ERRORCHECK` (`EDEADLK` on relock, `EPERM` on unlock by a non-owner), `RECURSIVE` (a count), and `DEFAULT` (= `NORMAL`). `PTHREAD_MUTEX_ADAPTIVE_NP` spin
#[test]
fn s22_4_c_1() {
    conformance(&["s22_sync"]);
}
/// §22.4 Contract: **Protocols:** `NONE`, `INHERIT` (priority inheritance), `PROTECT` (priority ceiling).
#[test]
fn s22_4_c_2() {
    // PRIO_PROTECT ceilings round-trip; raising a thread to the ceiling needs a real-time policy (privileged).
    conformance(&["s22_sync", "s22_more"]);
}
/// §22.4 Contract: **`pshared`:** usable across processes when the mutex is placed in shared memory.
#[test]
fn s22_4_c_3() {
    conformance(&["s22_more", "s18_fork"]);
}
/// §22.4 Contract: **Robust:** if the owner dies, the next locker gets `EOWNERDEAD` and must call `pthread_mutex_consistent`. Otherwise, after unlocking, the mutex becomes permanently `ENOTRECOVERABLE`.
#[test]
fn s22_4_c_4() {
    conformance(&["s22_sync", "s18_fork"]);
}
/// §22.4 Contract: **Timeouts:** `timedlock` is absolute on `CLOCK_REALTIME`. `clocklock` (2.30) takes the clock: `CLOCK_MONOTONIC` or `CLOCK_REALTIME`.
#[test]
fn s22_4_c_5() {
    conformance(&["s22_sync"]);
}
/// §22.4 Contract: **Static initialization:** `PTHREAD_MUTEX_INITIALIZER` (all zero bytes) and the `_NP` initializers for the other types.
#[test]
fn s22_4_c_6() {
    conformance(&["s22_sync"]);
}
/// §22.4 Contract: **C11 `mtx_*`:** `mtx_plain`, `mtx_timed`, `mtx_recursive`.
#[test]
fn s22_4_c_7() {
    conformance(&["s22_sync"]);
}
/// §22.4 Failure: `EBUSY` (trylock), `ETIMEDOUT`, `EDEADLK`, `EPERM`, `EOWNERDEAD`, `ENOTRECOVERABLE`, `EINVAL`, `EAGAIN` (recursion count overflow).
#[test]
fn s22_4_f_0() {
    conformance(&["s22_sync"]);
}
/// §22.5 Contract: **Condition variables.** `wait(c, m)` atomically releases `m` and blocks until signalled, then reacquires `m`. `signal` wakes at least one waiter, and `broadcast` wakes all. Spurious wakeups are permi
#[test]
fn s22_5_c_1() {
    conformance(&["s22_sync"]);
}
/// §22.5 Contract: **Guarantee (glibc 2.25+):** a signal wakes only a waiter that blocked *before* the signal. Waiters are grouped to ensure this.
#[test]
fn s22_5_c_1_1() {
    conformance(&["s22_sync", "s22_order"]);
}
// Pending: phase 15 (compat versions).
clause!(s22_5_c_1_2, "22.5", "Contract", r##"⚑ COMPAT: `pthread_cond_*@GLIBC_2.2.5` implement the pre-2.3.2 condvar ABI for old binaries."##);
/// §22.5 Contract: **Read-write locks.** Many readers or one writer. The preference is set by `kind_np`: `PREFER_READER` (the default; ⚑ HIST: writer starvation is possible), `PREFER_WRITER_NONRECURSIVE`. A recursive re
#[test]
fn s22_5_c_2() {
    conformance(&["s22_sync"]);
}
/// §22.5 Contract: **Spin locks.** Busy-waiting mutual exclusion with no blocking. **Kernel:** none.
#[test]
fn s22_5_c_3() {
    conformance(&["s22_sync"]);
}
/// §22.5 Contract: **Barriers.** Block until `count` threads have arrived. Exactly one of them receives `PTHREAD_BARRIER_SERIAL_THREAD`.
#[test]
fn s22_5_c_4() {
    conformance(&["s22_sync"]);
}
/// §22.5 Contract: **`pthread_once(&o, fn)`/`call_once`.** Run `fn` exactly once across all threads. Others wait until it completes. If `fn` is cancelled, the once object is reset.
#[test]
fn s22_5_c_5() {
    conformance(&["s22_sync", "s22_cancel"]);
}
/// §22.6 Contract: **Keys** are process-wide identifiers. Each thread has its own `void *` value per key (initially NULL), and a key may have a destructor.
#[test]
fn s22_6_c_1() {
    conformance(&["s22_lifecycle"]);
}
/// §22.6 Contract: At thread exit, destructors run for non-NULL values, repeated up to `PTHREAD_DESTRUCTOR_ITERATIONS` (4) rounds while values remain.
#[test]
fn s22_6_c_2() {
    conformance(&["s22_lifecycle", "s22_cancel", "s22_order"]);
}
/// §22.6 Contract: Keys are limited to `PTHREAD_KEYS_MAX` (1024). ⚑ ARB.
#[test]
fn s22_6_c_3() {
    // More than PTHREAD_KEYS_MAX keys are allowed (D-22.5).
    conformance(&["s22_more"]);
}
/// §22.6 Contract: **Language-level TLS** (`__thread`, `_Thread_local`, `thread_local`) gives each thread its own instance of a variable. For dynamically loaded modules, the storage is allocated lazily through `__tls_ge
#[test]
fn s22_6_c_4() {
    // Static TLS here; dynamically loaded modules (__tls_get_addr) arrive with phase 14.
    conformance(&["s22_lifecycle"]);
}
/// §22.6 Guarantees: Static TLS for the executable and initially loaded libraries is at a fixed offset from the thread pointer.
#[test]
fn s22_6_g_0() {
    conformance(&["s22_more"]);
}
/// §22.7 Contract: A counting semaphore.
#[test]
fn s22_7_c_0() {
    conformance(&["s22_sync"]);
}
/// §22.7 Contract: `post` increments and wakes a waiter. It is **async-signal-safe**, the only synchronization primitive that is.
#[test]
fn s22_7_c_1() {
    conformance(&["s22_more"]);
}
/// §22.7 Contract: `wait` decrements, blocking while the count is 0. The timed forms take an absolute time.
#[test]
fn s22_7_c_2() {
    conformance(&["s22_sync"]);
}
/// §22.7 Contract: `getvalue` reports the count (glibc never reports a negative count for waiters).
#[test]
fn s22_7_c_3() {
    conformance(&["s22_sync"]);
}
/// §22.7 Contract: **Unnamed semaphores** (`sem_init`) are shared between threads or, with `pshared`, between processes.
#[test]
fn s22_7_c_4() {
    conformance(&["s22_sync", "s22_more"]);
}
/// §22.7 Contract: **Named semaphores** (`sem_open(name, O_CREAT|O_EXCL, mode, value)`) are files in `/dev/shm` named `sem.<name>`. Opening the same name twice in one process returns the same address.
#[test]
fn s22_7_c_5() {
    conformance(&["s22_sync"]);
}
/// §22.7 Failure: Returns -1 with `errno`: `EAGAIN` (trywait), `ETIMEDOUT`, `EINTR`, `EINVAL`, `EOVERFLOW` (`SEM_VALUE_MAX` = `INT_MAX`), `ENOENT`, `EEXIST`, `EACCES`.
#[test]
fn s22_7_f_0() {
    conformance(&["s22_sync", "s22_more"]);
}
/// §22.8 Contract: **Naming.** Set or get a thread name of at most 15 bytes plus a terminator (⚑ KERN: `TASK_COMM_LEN` is 16). Returns `ERANGE` if too long.
#[test]
fn s22_8_c_1() {
    conformance(&["s22_lifecycle"]);
}
/// §22.8 Contract: **Scheduling.** Per-thread scheduling policy and priority.
#[test]
fn s22_8_c_2() {
    conformance(&["s22_more"]);
}
/// §22.8 Contract: **`pthread_yield`** is `sched_yield`. ⚑ HIST: it is deprecated.
#[test]
fn s22_8_c_3() {
    assert_eq!(liella_boundary::posix::s22_threads::pthread_yield(), 0, "sched_yield");
}
/// §22.8 Contract: **Affinity.** A per-thread CPU mask.
#[test]
fn s22_8_c_4() {
    conformance(&["s22_more"]);
}
/// §22.8 Contract: **`pthread_getcpuclockid`** returns a clock ID that measures the thread's CPU time.
#[test]
fn s22_8_c_5() {
    conformance(&["s22_more"]);
}
/// §22.9 Contract: glibc registers an rseq area for each thread (2.35+, unless disabled with `glibc.pthread.rseq=0`) and publishes its offset from the thread pointer and its size, so that applications can use restartabl
#[test]
fn s22_9_c_0() {
    conformance(&["s22_lifecycle", "s22_more"]);
}
// Pending: phase 14 (libthread_db, spec §22.10).
clause!(s22_10_c_0, "22.10", "Contract", r##"A library that a *debugger* loads to interpret the thread structures of a target process. The debugger supplies memory and register access callbacks (`ps_pdread`, `ps_pdwrite`, `ps_lgetregs`, `ps_pglobal_lookup`, ...), and the library reports:"##);
// Pending: phase 14 (libthread_db).
clause!(s22_10_c_1, "22.10", "Contract", r##"the thread list;"##);
// Pending: phase 14 (libthread_db).
clause!(s22_10_c_2, "22.10", "Contract", r##"the LWP ↔ thread mapping;"##);
// Pending: phase 14 (libthread_db).
clause!(s22_10_c_3, "22.10", "Contract", r##"TLS addresses;"##);
// Pending: phase 14 (libthread_db).
clause!(s22_10_c_4, "22.10", "Contract", r##"TSD values;"##);
// Pending: phase 14 (libthread_db).
clause!(s22_10_c_5, "22.10", "Contract", r##"creation and death event breakpoints."##);
