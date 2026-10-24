<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

### Phase 5: Thin kernel surfaces

* **Scope:**
  * §11 low-level I/O except AIO (about 125 symbols);
  * §13.4–13.7 and §13.10–13.15, the thin file-system calls;
  * §15.1–15.2, socket calls;
  * §17.1, credential queries;
  * §18.1, process identity;
  * §20.1–20.9, signals except the cancel and setxid interplay;
  * §23.1–23.3, clocks and sleeping (vDSO);
  * §24.1–24.4, limits, scheduling and `sysconf`;
  * §25.1–25.2, pipes and SysV IPC;
  * `syscall()`.
* **Symbols:** about 520.
* **Clauses:** about 330.
* **Needs:** P4.
* **Builds:**
  * IP-16: `machine/restorer.S` and the rt signal table.
  * The C-layer variadic adapters (IP-14, OD-C1) for `open`, `openat`, `fcntl`, `ioctl`,
    `prctl`, `semctl`, `mremap`, `syscall`, `ulimit`, each forwarding to a fixed-arity Rust
    export.
  * The `select`/`fd_set` and `sigset_t` converters.
* **Test surface:**
  * For each thin wrapper: the contract's guarantees and every errno it lists that is
    reachable in tests.
  * Cancellation-point markers, which are tested in P6.
  * Signal delivery: restorer correctness, `SA_RESTART` versus `EINTR`, `raise` targeting
    the calling thread, and mask edge cases (`SIGKILL`/`SIGSTOP` silently removed, internal
    signal hidden).
  * `sysconf` returning -1 without errno for "no limit" (D-24.1).
* **Exit:** all in-scope clauses and symbols green. AS-safety list verified: no locks and no
  allocation in listed functions, checked by a test harness that calls each from a signal
  handler interrupting a lock holder.
* **Delivered** (exit met, with the deviations listed):
  * **Exports:** 418 glibc symbols: §11 except AIO, §13.4–13.7 and §13.10–13.15, §15.1–15.2,
    §17.1, §18.1, §20.1–20.9, §23.1–23.3, §24.1–24.4, §25.1–25.2 and `syscall`. Each is a
    single-expression translation (purity gate). What glibc adds to a kernel call lives in
    new `liella-legacy` modules: `io_compat`, `fs_compat`, `net_compat`, `cred_compat`,
    `ipc_compat`, `clock_compat`, `sys_compat` (with the generated `sysconf_table`) and
    `signal_compat`. The computed configuration answers are in `liella::system`, the
    device-number encoding in `liella::fs`.
  * **C layer:** variadic adapters for `open`, `openat`, `fcntl`, `ioctl`, `mremap`, `semctl`,
    `ulimit` and `syscall` (D-11.10). The system-call table gains `ustat` and a generic
    `syscall` row (kernel name `*`: the number is the first argument, generated like every
    other wrapper, K5).
  * **IP-16:** the restorer of phase 1 serves every handler; the rt table is interpreted as
    D-20.6.
  * **Spec data:** ERRATA E10 (the constants extractor skipped whole chunks and recorded
    addresses; `constants.tsv` regenerated, 726 constants added, no meaningful value changed).
    A new probe records the reference glibc's `sysconf`, `confstr` and `pathconf` answers
    (`spec/abi/sysconf.tsv`, D-24.6).
  * **AS-safety (exit criterion):** `crates/core/tests/as_safety.rs`. A child holds every lock
    of the library (`selftest::with_all_locks`, over the new `Heap::lock_all`, which `fork`
    will use too) while a signal handler calls the 135 async-signal-safe functions phases 1–5
    implement; all complete. A negative control (`malloc` from the handler) blocks on the held
    heap lock, the alarm ends the child, and the harness names the call.
  * **Tests:** 418 symbol tests and 251 clause tests. `sysconf`, `confstr` and
    `pathconf`/`fpathconf` are compared with the host glibc name by name; `psignal` and
    `psiginfo` output byte for byte. Fallbacks run against injected kernel failures
    (`Faulty`). Four C conformance programs exercise the variadic entry points through real
    varargs. Process-wide state (dispositions, masks, limits, the umask, descriptor
    numbering) is changed only in re-executed children. Full gate: 4,344 tests over both
    passes, 0 failed; 18 of 18 conformance programs; all 14 bench budgets met; clippy, purity,
    unsafe-audit, csys-audit and csys-gen clean.
  * **Pending clauses:** 13 in scope. Eight cancellation clauses and `pthread_kill` (s20_4_c_5)
    wait for phase 6; §13.13's four move to phase 12 (D-13.10).
  * **Found and fixed:**
    * Phase 1's `CSys::getpriority` reached C callers biased (20 − nice); D-24.8.
    * A process Liella did not start re-read `/proc/self/auxv` on every lookup
      (`sysconf(_SC_PAGESIZE)` at 792 ns); it is now read once (4.19 ns, glibc 4.17).
    * `time()` went through `clock_gettime` at 5× glibc; D-23.8.
    * The sysfs CPU-list parser rejected the trailing newline every real file has, so the
      processor count silently used its `/proc/stat` fallback (and `ngroups_max` its default).
    * `faccessat`'s emulation allocated; D-17.6.
    * Test-suite races: tests that closed descriptor numbers twice, closed a range, or used a
      number after closing it could reach a concurrently running test's descriptors (and did:
      a closed pipe under `std::process::Command`). Such tests now use private numbers or
      re-executed children; the conformance children inherit descriptors, so their
      lowest-number checks compare reuse instead. A group signal reached the test harness
      itself; such tests now run in a process group of their own.
  * **Deviations from the plan as written:**
    1. **§13.13** moves to phase 12 (D-13.10): its functions are `FILE *` interfaces.
    2. **`prctl`'s adapter** moves to phase 11 with §18.9; the plan listed it among phase 5's
       adapters.
    3. **Symbols listed with other sections, done here because their contracts are in scope:**
       `ftruncate`/`truncate` (§11.8 contract, §13 symbols), `remove` (§13.4 contract, §12
       symbol), `chflags`/`fchflags` (§13.15, §29 symbols), `pthread_sigmask` (§20.3, §22
       symbol).
    4. **D-13.9** refined to the spec's glibc fallback (owner to confirm).
    5. **D-24.7:** cache geometry from the deterministic CPUID leaves (owner to confirm).
    6. **D-20.6:** the rt "dispatch table" holds only the library's own handler (owner to
       confirm).
    7. **Interim, until later phases:** `strsignal`'s unknown-signal buffer is process-wide
       until phase 6 gives threads their slots (D-H3); `psignal`/`psiginfo` write to
       descriptor 2 until the streams (phase 8); compat-only symbols (`ustat`, `stime`,
       `ftime`, `sigstack`, `sigvec`, the `sys_siglist` objects, the `__x*` stat family) carry
       a default version until phase 15 adds compat versions.
    8. **D-20.1** is observable as specified: applications see `SIGRTMIN` = 33, where the spec's
       text (glibc) says 34. The clause test asserts 33 and names the decision.
  * **Multi-core and NUMA (§2.6 register):** nothing in phase 5 adds shared state on a hot
    path. The process-wide states the C contracts require (`siginterrupt`'s set, the
    real-time signal allocation, the auxiliary-vector cache, `strsignal`'s interim buffer) are
    atomics without locks; the AS-safety harness checks that the async-signal-safe functions
    take no lock at all. `Heap::lock_all` gives phase 6's `fork` the whole-heap quiescence it
    needs. `get_nprocs` keeps glibc's sysfs contract; the accurate parallelism of D-24.2
    (affinity and cgroup quota) is phase 6's, with the thread runtime that uses it.
