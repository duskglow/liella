<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

### Phase 4: Process runtime

* **Scope:**
  * §1.1–1.5, program startup, exit and environment (48 symbols);
  * §2.1, §2.4 and §2.5, errno, assertions and fatal diagnostics (with stream-free output);
  * §1.8, the tunables parser (the policy's input format only);
  * `__libc_start_main`, `__cxa_*`, `__stack_chk_fail`, and the `__chk_fail` family.
* **Clauses:** about 80.
* **Needs:** P3.
* **Builds:**
  * IP-8 (thread blocks), and IP-9: `csys/src/machine/entry.S` plus `rt::start`.
  * IP-10 (`errno` slot, result slots).
  * The IP-13 data objects (`environ`, `__environ`, `program_invocation_name`, ...), with
    the sizes of Appendix C.5.
  * IP-21 (config reader), `rt::fatal`, `rt::panic`.
  * The **product build** (`cargo xtask so`): `libc.a`, `libc.so.6`, and the start object
    containing `csys` entry. The version script is generated from Appendix C.3
    (`spec/abi/versions.tsv`).
* **Test surface:**
  * `exit` ordering across the three registration kinds, handlers registering handlers,
    re-entry, and `quick_exit` isolation.
  * `abort` with a caught, returning `SIGABRT` handler.
  * Secure-mode sanitising of the environment and of fds 0–2.
  * `getenv`/`setenv`/`unsetenv`/`putenv` aliasing semantics (D-1.3), and duplicate
    `execve` entries.
  * `getauxval` absent key (D-1.5).
* **The C conformance suite goes live:** `conformance/run.sh` links real C programs from
  here on, and every later phase adds C tests for header-inline behaviour.
* **Exit:** a static C "hello" built with `-nostdlib` against our `crt1` and `libc.a` runs.
  All §1.1–1.5 and §2.1/2.4/2.5 green.
* **Delivered** (exit met, with the deviations listed):
  * **4a, the performance floor (P7, OD-C2):** the C layer's scanners (`strlen`, `strnlen`,
    `rawmemchr`, and every byte-text conversion in `strz`) and the ERMS copies and fills
    (D-4.14). Every phase-2 budget now passes.
  * **Exports:** 29 of §1's 48 symbols and 6 of §2's, each a single-expression translation.
    * §1: `__libc_start_main`, `__libc_init_first`, `exit`, `_exit`, `_Exit`, `quick_exit`,
      `abort`, `on_exit`, the `__cxa_*` registrations, the environment family, `getauxval`,
      and the data objects `environ`/`__environ`/`_environ`, the program names and
      `__libc_stack_end`.
    * §2: `__errno_location`, the `__assert*` family, `__stack_chk_fail`, `__chk_fail`.
    * The static library also has `atexit` and `at_quick_exit`, which glibc ships in
      `libc_nonshared.a`.
    * §1.6–1.7 (`getopt`, `argp`) are phase 8, as the plan schedules them.
  * **Runtime (IP-8, IP-9, IP-13):**
    * `csys/src/machine/entry.S` is the `_start` of `liella-crt1.o`. `boundary::runtime` carries
      out `liella_rt::start`'s sequence: initial-thread TLS and canary for static programs,
      secure-mode sanitising, environment and names, tunables and CPU paths, destructor
      registration, constructors, `main`, `exit`.
    * `rt::tls::ThreadBlock` is the thread block (D-4.15). `legacy::exit_handlers` and
      `legacy::environ` are the C shapes (D-1.2, D-1.3, D-1.9). `liella::config` holds the
      tunables and the IP-21 line reader (D-1.8). `rt::fatal` and `rt::panic` are the
      async-signal-safe diagnostics.
  * **Products (`cargo xtask so`):** `target/liella/libc.a`, `libc.so.6` (647 symbols with
    default versions from Appendix C.3, soname `libc.so.6`) and `liella-crt1.o` (D-4.16).
  * **C conformance suite live:** 15 programs, each started by Liella's own runtime. The runner
    takes directives for arguments, an exact environment (repeated names kept, through
    `execve`), the expected status or signal, expected stderr, and secure mode (a setgid copy,
    so `AT_SECURE` is genuinely set). `cargo xtask test` builds the products first.
  * **Tests:**
    * 29 + 6 symbol tests and 46 + 6 clause tests for §1–§2.
    * The runtime logic is tested where it lives: TLS placement, guards, program headers,
      secure-mode names, tunables, exit registries over a C-call recorder, the environment over
      a model of C memory.
    * Exits, aborts and secure-mode descriptors run in re-executed children.
    * Full gate: 1,499 tests in each of the two passes, 0 failed; 15 of 15 conformance programs;
      all 14 bench budgets met.
  * **Found and fixed:** a `libc.so.6` under `target/release` replaced the host C library for
    every build script Cargo ran, because Cargo puts that directory on `LD_LIBRARY_PATH`. The
    products now go to `target/liella` (D-4.16). The phase-0 conformance template expected the
    wrong bytes after `memmove`; it had never run.
  * **Pending clauses:** 22. They are §1.1's streams and C locale (phase 8), abort's no-flush
    clause (streams, phase 8), `exit` with several threads (phase 6), and §1.6–1.7 (phase 8).
  * **Deviations from the plan as written:**
    1. **`__assert_perror_fail`** shows the error's symbolic name (`ENOENT`) until §2.2's
       messages arrive (phase 8).
    2. **Static PIE** executables are refused with a diagnostic: they must relocate themselves
       before any address in static data is used (phase 14, with `ld.so`).
    3. **`libc.so.6`** carries default versions only. The 530 compat versions need `.symver`
       aliases (phase 15). It cannot be loaded by the host's `ld.so`, which expects glibc's
       private interfaces; Liella's own `ld.so` is phase 14.
    4. **D-1.3:** values that `setenv` replaces are left allocated, as the contract allows,
       rather than tracked for release by `clearenv`. `clearenv` releases only the array.
    5. **Tunables:** only those that select the C layer's paths apply (D-1.8); other names are
       ignored, which the contract permits.
    6. **The unsafe placement table (§3.2)** gains `boundary/src/tb.rs` and
       `boundary/src/runtime.rs` for category *slice*: the thread block and the initial stack
       are the conversions IP-8 and IP-9 assign to the boundary.
    7. **A machine primitive, `liella_machine_trap` (`hlt`),** is added for `abort`'s last step,
       which spec §1.2 requires.
    8. **OD-C2 (c) did not deliver.** The x86-64-v3 build moves `memchr` only from 146 to 116 ns
       at 4 KiB (glibc: 21) and leaves `memcmp` at about 69 (glibc: 28). Two reshapings of the
       safe loops as vector reductions measured 20 times slower and were reverted. Under P7
       these would move to C-layer scanners, which needs the owner's decision because OD-C2 (c)
       keeps them in Rust.
  * **Multi-core and NUMA (§2.6 register):** the thread block reserves the per-thread allocator
    cache and NUMA node words (0x90, 0x98) that phase 6 fills. Nothing in phase 4 adds shared
    state on a hot path. The exit registries and the environment lock serialise operations
    that run once per process or that the contract makes MT-Unsafe.
