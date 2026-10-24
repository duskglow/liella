<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

### Phase 6: Threads, synchronisation, process-wide semantics

* **Scope:**
  * §22 (239 symbols: pthreads, C11 threads, semaphores, keys, cancellation, affinity,
    naming, rseq; `libthread_db` waits for P14);
  * §17.2, process-wide credential changes;
  * §18.2, `fork`, `_Fork`, `vfork`, `clone` and `atfork`;
  * §21, `setjmp`/`longjmp` and user contexts;
  * re-validation of the per-thread `errno` and result slots.
* **Clauses:** about 90.
* **Needs:** P5.
* **Builds:**
  * IP-17: the cancellation model (D-22.2).
  * Thread creation and exit: `rt::thread` over `machine/thread_start.S`.
  * The internal signal's multiplexed uses: cancel and setxid (D-20.1).
  * Robust lists, and rseq registration.
  * `machine/setjmp.S` and `machine/context.S` (register save and restore, pointer
    mangling, CET), with the mask handling and bookkeeping in `rt::context`.
* **Test surface:**
  * The contracts of every sync object: mutex types and protocols, robustness (kill the
    owner process for pshared robust mutexes), timed and clock variants on both clocks.
  * The condvar ordering guarantee of §22.5, barrier serial thread, and once-with-cancel
    reset.
  * Key destructor iterations (4 rounds).
  * Credential broadcast under 64 threads, including a partial-failure test that must abort
    (spec §17.2).
  * `fork` in a threaded process: `malloc` works in the child, and atfork order is honoured.
  * Cancellation at each cancellation point, both before and after the side effect.
  * `longjmp` from a signal handler out of a blocking call (F37).
* **Exit:** all §22 (minus thread_db), §17.2, §18.2 and §21 green, with stress tests run
  1,000 times without failure under `taskset` on 1 and N cores.
* **Budgets (owner 2026-10-01, rule P6):** added to `bench/budgets.tsv` at this phase's exit,
  each as a ratio to the host glibc measured in the same run (`bench_heap`'s `ref_*` rows):
  * one thread, 64-byte `malloc`/`free`: at most 1.5× glibc;
  * 16 threads at once, 64-byte pairs (wall time per pair): at most 2× glibc;
  * 1 MiB `malloc`/`free`: at most 2× glibc;
  * `gettid`: a load from the thread block, no system call.
* **6a (owner 2026-10-01, OD-C2 revised):** `memchr` and `memcmp` as C-layer assembly, with
  the D-4.14 treatment. Expected: ≤ 1.1× glibc at 4 KiB and 64 KiB.
* **Design first (owner 2026-10-01):** the per-thread cache, NUMA topology and thread
  runtime decisions are written below before their implementation: D-3.11 (per-thread caches),
  D-3.12 (topology), D-3.13 (large-block reuse), D-17.7 (setxid), D-18.6 (`fork`), D-21.3
  (shadow stacks), D-22.9 (thread runtime), D-22.10 (threads Liella did not create) and
  D-22.11 (cancellation mechanics). (Decision numbers follow spec sections; the plan's earlier
  "D-6.x" would have collided with §6's iconv decisions.)

* **Delivered** (exit met, with the deviations listed):
  * **Exports:** the §22 thread library except `libthread_db` (199 symbols with
    `pthread_sigmask`: POSIX threads, C11 threads, semaphores, keys, cancellation, affinity,
    naming, rseq), §21 (9), the §17.2 setters (11), and §18.2's `fork`, `_Fork`, `vfork`,
    `clone` and their aliases (8), plus `__sigsetjmp` and `siglongjmp` (§20 symbols, §21
    contracts). Thread creation, exit, joining and cancellation are products-only exports
    (`export! { runtime ... }`): a test process keeps the host's threads (D-22.10).
    `libpthread.so.0` and `libpthread.a` ship as empty placeholders, so `-lpthread` and old
    `DT_NEEDED` entries resolve (§22 notes).
  * **Allocator (6, D-3.11 to D-3.13):** per-thread caches of thread-owned slabs, remote frees
    through a second bitmap, caches parked and recycled whole per node, the node from rseq or
    the vDSO's `getcpu`; large blocks reused per thread with a lock-free large table.
  * **Thread runtime (D-22.9):** one mapping per thread (guard, stack, static TLS, thread
    block, descriptor, key area); `pthread_t` is the thread pointer; stacks cached up to
    40 MiB; robust list and rseq registered per thread; `gettid` and `pthread_self` are loads.
  * **Cancellation (D-22.2, D-22.11):** a cancellation point is an explicit `Err(Canceled)`;
    blocking calls go through the C-layer window primitive, whose cancelled exit the internal
    signal's handler selects only while the system call has had no effect. 63 exports are
    cancellation points (the `CP` kernel). Asynchronous cancellation acts at the next safe
    point (owner ruling; deviation below).
  * **Process-wide semantics:** the setxid broadcast (D-17.7) replays every credential change
    on every thread and ends the process if one cannot follow; `fork` runs the atfork handlers
    and takes the library's locks in a fixed order (atfork, thread list, stack recycling,
    setxid, heap, environment) and resets the child (D-18.6).
  * **C layer:** `machine/thread_start.S`, `machine/setjmp.S` (pointer mangling),
    `machine/context.S`, `machine/scan.S` (6a: `memchr`, `memcmp`, D-4.17), and the variadic
    adapters for `makecontext` and `sem_open`.
  * **Tests:** 228 symbol tests and 97 clause tests in this phase. Seventeen new C
    conformance programs: lifecycle, sync objects (robust pshared mutexes with the owner
    process killed, condvar ordering, barrier serial thread, once reset by cancellation, PP
    ceilings), cancellation (19 points cancelled before the call's effect, and a race test
    showing a request lands before or after the side effect, never between, in both orders),
    ordering (a condition-variable signal wakes only an earlier waiter; four destructor rounds),
    the last thread's exit, `setjmp`/contexts/`__longjmp_chk` (and `siglongjmp` out of a
    handler interrupting a blocking `read`, F37), the credential broadcast under
    64 threads and the abort when a thread cannot follow, `fork` with atfork order and
    `malloc` in the child, the alias names, and a coverage program that calls every symbol no
    scenario reaches (including the glibc-internal names, through their symbol names). Symbol
    tests check each export's `libc.so.6` version against `versions.tsv`, run the programs
    that reference it (from `nm -u` of their objects), and call the descriptor-free objects
    and attributes in process. Heap: `crates/boundary/tests/heap_threads.rs` and the rt
    tests. Full gate: 5,044 tests over both passes, 0 failed; 35 of 35 conformance programs;
    all 21 bench budgets met; clippy, purity, unsafe-audit, csys-audit, csys-gen and abi-gen
    clean.
  * **Stress (exit criterion):** `conformance/stress.sh`, 1,000 runs of each phase-6 program
    on one core (`taskset -c 0`) and on all 32 logical CPUs: 17 programs, 34,000 runs, none
    failed (WSL2 kernel 5.15).
  * **Budgets (rule P6):** `bench/budgets.tsv` gains the owner's four: 64-byte pair
    ≤ 1.5× glibc, 16 threads ≤ 2×, 1 MiB ≤ 2×, `gettid` a load (≤ 5 ns and ≤ 0.1× a system
    call). `cargo xtask bench` now also runs C benchmarks against the products
    (`bench/c/cbench_*.c`), since the thread block exists only in a process Liella started.
    Results: `benchmarks/progress.md` entry 4.
  * **Found and fixed:**
    * Transparent huge pages: before Linux 6.7 a `MAP_STACK` mapping can take a huge page once
      it merges with an adjacent anonymous mapping, making the neighbour's untouched pages
      resident. Thread mappings are now marked `MADV_NOHUGEPAGE` before first use on such
      kernels (D-22.9; found through a flaky `mincore` clause test, fixed in 23f159c and
      0d7877a).
    * Two test-suite races in phase 5's tests, exposed by this phase's longer conformance runs:
      a mapping test unmapped its whole range after punching a hole, which a concurrent test's
      `mmap` could have filled; a `flock` test's lock was held by concurrently spawned
      processes, through an inherited descriptor (it lacked close-on-exec) or through the copy
      of the descriptor table a child holds until its exec. Its descriptors are now
      close-on-exec and its release check waits for the release.
    * `conformance/run.sh` rewrote programs in place, so a concurrent run of the same program
      failed with `ETXTBSY`; it now links under a temporary name and renames.
  * **Deviations from the plan as written:**
    1. **Asynchronous cancellation** takes effect only at safe points outside Liella code
       (owner ruling 2026-10-01, D-22.11). glibc acts at once.
    2. **Dynamically linked programs** cannot create threads until phase 14 (`EAGAIN`; owner
       ruling 2026-10-01, D-22.10). Statically linked programs have full threads.
    3. **`setjmp` (the symbol) does not save the mask,** as the spec says; the host glibc's
       does (D-21.4, for the owner).
    4. **Pending clauses:** `libthread_db` (§22.10, 6 clauses) and the C++ forced unwind of
       §22.3 wait for phase 14; the compat condition variable (§22.5) for phase 15;
       `initgroups` (§17.2) for phase 10. `pthread_yield`, `pthread_kill_other_threads_np`
       and the libpthread-era `__pthread_*` aliases are compat-only in 2.34+: in `libc.a`, and
       in `libc.so.6` with phase 15's compat versions.
  * **Multi-core and NUMA (§2.6 register):** the heap's per-thread caches, NUMA placement and
    large-block reuse are D-3.11 to D-3.13 (register row 6): no lock or atomic read-modify-write
    on a thread's own allocations, the node read only on slow paths, 16 threads at 1.17× glibc
    (from 632×). The thread runtime touches the shared thread list only at create, exit, setxid
    and `fork`; everything per call (`errno`, the TID, `pthread_self`, keys, the heap cache) is
    reached through the thread pointer. Cancellation and setxid share one internal signal.
    Placement is tested only through simulated topologies: this machine has one node (a two-node
    machine is offered for the phase 6/7 report).
