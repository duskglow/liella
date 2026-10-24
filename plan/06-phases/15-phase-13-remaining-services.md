<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

### Phase 13: Remaining services

* **Scope:**
  * §26, syslog;
  * §25.3–25.4, POSIX message queues with `SIGEV_THREAD` notification, and POSIX shared
    memory;
  * §11.11, POSIX AIO (io_uring where available, D-11.8);
  * §7.7–7.8, gettext and catgets (the message-catalog format is inspected, and recorded
    in Appendix D);
  * §3.2–3.3, allocator introspection, and the debug facilities with `libc_malloc_debug`
    semantics.
* **Symbols:** about 90.
* **Clauses:** about 60.
* **Needs:** P12.
* **Exit:** in-scope clauses and symbols green.
* **Delivered** (exit met, with the deviations listed):
  * **Method (§1.4):** behaviour documents first (`syslog.md`, `posixipc.md`, `aio.md`,
    `catalogs.md`, `mallocdbg.md`, with their records; the probes ran in private namespaces with a
    `/dev/log`, `/dev/mqueue` and `/dev/shm` of their own, so the system's were never touched), the
    catalog formats moved into the spec as Appendix D.2, then fresh sessions per part, two at a
    time on separate areas.
  * **syslog (§26; D-26.2):** never blocks indefinitely (a 1 s bound per message where the
    reference blocks on a full receiver or a FIFO console), never changes `errno`.
  * **POSIX message queues and shared memory (§25.3–25.4; D-25.5):** `mq_notify` `SIGEV_THREAD`
    over netlink (D-25.3); errors where the reference aborts, blocks or opens `/dev` itself.
  * **gettext and catgets (§7.7–7.8; D-7.12):** catalogs checked whole before use (a damaged one
    is absent, never a crash), Plural-Forms never trapping, no stale translations, locale values
    never used as path components containing `/`, `.` or `..`.
  * **POSIX AIO (§11.11; D-11.11):** io_uring with a silent worker-pool fallback, both tested;
    the reference's crashes are completions or errors; the io_uring calls are in the C layer's
    system-call table.
  * **Allocator debugging (§3.2–3.3; D-3.4, D-3.5):** Liella's libc provides the
    `libc_malloc_debug` semantics itself (`mcheck`, `mtrace`, `M_PERTURB`), one relaxed load per
    call when off; the statistics count pending remote frees as free, so they no longer depend on
    timing.
  * **Spec corrections (three-way rule, each with its evidence):** §26.1 (the PID only with
    `LOG_PID`), §25.3 (the errors for bad queue names), §7.7 (the category's directory for
    `dcgettext`; the kernel line), §11.11 (`EBADF` at submission); Appendix D.2 (message catalogs,
    including D.2.4 on gettext's catalog directories).
  * **Tests:** every in-scope clause and symbol specified. Differential tests against the host
    glibc: `syslog_products`, `posixipc_products`, `catalogs_products` (plus the damaged catalogs
    outside the record), `aio_products` (five Liella variants: io_uring and both fallbacks, cache
    on and off), `mallocdbg_products`; conformance programs for `mcheck`, `mtrace` (including a
    set-group-ID run that must not create the trace file) and perturbation. The gate now leaves
    nothing in `/tmp`, `/dev/shm` or `/dev/mqueue` (15,897 leftover directories of earlier phases
    removed; test scratch under `CARGO_TARGET_TMPDIR` with drop guards). Races fixed: a fixture
    executed while another thread's fork held it open for writing (`ETXTBSY`), `s3_2_c_2`, and the
    allocator statistics.
  * **Budgets:** `benchmarks/progress.md` entry 11 and the review before phase 14 (107 budgets
    met). Fixed: gettext's alias rescan (7.9 → 2.8 µs), `mallinfo2`'s bitmap scan, the 64-byte
    `malloc`/`free` pair (an inlining regression from the debug check: 17.7 → 13.7 ns). Deferred
    with causes: gettext's per-lookup `stat` (D-7.12), `mallinfo2`/`malloc_info` slab scans, AIO
    list submission.
  * **Owner items** are in the phase-13 pause report.
