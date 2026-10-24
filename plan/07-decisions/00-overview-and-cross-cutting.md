<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

## 7. Design decision register

Every ⚑ HIST, COMPAT, PATH and ARB annotation in the spec is either an opportunity for a
cleaner internal design, or a constraint the translation layer must reproduce at the
boundary. It is usually both. This register records the decision for each. ⚑ KERN items
are honoured unchanged, and are listed only where they shape a decision.

**Reading the tables.**

* *Internal* is the clean API in `liella`.
* *Boundary* is what the translation layer (plus `liella-legacy`) reproduces for C callers.
* A decision that changes behaviour observable through the C ABI says so explicitly, and has
  a test citing its ID.

### 7.1 Cross-cutting decisions (spec §30, patterns H1–H18)

| ID | Pattern | Decision |
|---|---|---|
| D-H1 | NUL-terminated text | **Internal:** text is `&[u8]` with a length, and wide text is `&[u32]`. NUL termination exists only in `kernel::KPath` and the `execve` argument builder (⚑ KERN). **Boundary:** `boundary::strz` reads C strings up to the terminator without over-reading (F1, OD-C2). Outputs append the terminator in `abi::ret`. |
| D-H2 | errno and five error conventions | **Internal:** one `Result<T, Error>` (X1). **Boundary:** one `abi::ret` function per convention (X2). Ambiguous sentinels stay ambiguous at the boundary, because the C ABI fixes them (F4). Internally they never occur. |
| D-H3 | Static buffers and hidden cursors (≈70 entry points) | **Internal:** results are owned values or writes into caller slices. Iteration state is a value the caller holds. **Boundary:** `legacy::result_slots` (per thread, P-scope) and `legacy::cursors` (per thread for the data-race-only cases, process-wide and locked for enumerators). `_r` forms are packers over the same call. Phase 8: the per-thread slots live after each thread's key area (`rt::thread::SLOT_AREA`, in every thread mapping, the initial area and adopted descriptors); `strerror` and `strsignal` use them, and fall back to the process-wide buffer only when no descriptor can be made. The `ecvt` family, `l64a` and `tmpnam(NULL)` stay process-wide (their contracts name a static buffer). |
| D-H4 | Ambient locale, zone, printf registry, resolver state | **Internal:** explicit `&Locale`, `&Zone`, `&Resolver` and registry parameters. **Boundary:** translations supply the current process or thread value from `liella-legacy`. The `_l` twins pass their argument instead. |
| D-H5 | Multiple delimiting conventions | **Internal:** one function per need. **Boundary:** `str*`, `strn*`, `strl*`, `mem*`, `wcs*` and `wmem*` differ only in which `conv` builds the input slice. |
| D-H6 | Fixed-size ABI structures | **Internal:** unbounded types (`SigSet` u64 bits, `CpuSet` dynamic, `FdSet`-free polling, resolver lists unbounded). **Boundary:** the exact ABI sizes are kept (`fd_set` 1024, `sigset_t` 128 bytes, `cpu_set_t` 1024, `_res` 3 and 6, `utmp` fields). Truncation at the boundary follows the contract. |
| D-H7 | Versioned superseded contracts | **Internal:** one current contract. **Boundary:** each old version is a separate export mapped to `legacy::versioned` (`memcpy@2.2.5` → move semantics; `fmemopen@2.2.5`; `glob@2.2.5`; `sched_setaffinity@2.3.3`; `pthread_cond_*@2.2.5`; `realpath@2.2.5`; the `scanf` dialects; `strerror_r` XSI/GNU; `basename` GNU/XPG; `sigpause` BSD/XSI; `signal` BSD/SysV; `__posix_getopt`). The version script comes from the manifest. |
| D-H8 | Dead subsystems | Everything in §29 lives in `liella-legacy` and phase 15. No core code depends on it. |
| D-H9 | POSIX process-wide semantics over per-thread kernel state (⚑ KERN) | **Internal:** both are offered explicitly: `cred::set_thread_*` (native, one syscall) and `cred::set_process_*` (broadcast). **Boundary:** POSIX names map to the process-wide form, so the contract holds. Cost finding F16. Scheduling calls act on the thread named, as the kernel does, and the documentation says so. |
| D-H10 | Variadic and untyped interfaces | **Internal:** typed operations only: `OpenOptions`, `FcntlCmd` enum, `Ioctl` typed requests for the terminal and socket subsets, `PrctlOp` enum, typed format arguments. **Boundary:** C variadic adapters (OD-C1) plus format-directed `va_list` decoding in `legacy` (F11). Untyped `ioctl`/`prctl`/`ptrace`/`syscall` pass through, because the kernel itself is untyped there (⚑ KERN). |
| D-H11 | Implicit configuration by files and environment | **Internal:** typed configuration values (`NssConfig`, `ResolverConfig`, `Zone`, `LocaleData`, `Tunables`) parsed by IP-21 and passed explicitly. **Boundary:** the default configuration is loaded lazily from the conventional paths, and reloaded on change where the contract requires it (resolv.conf, nsswitch.conf, TZ). **No `dlopen` for built-in functionality**: charsets, IDNA, the `files` and `dns` sources and the unwinder are built in. Only third-party NSS modules are loaded (P14). |
| D-H12 | Insecure designs | **Internal:** these are not offered: no unbounded writes, no name-only temp files, no shell-command APIs other than an explicit `Command`. **Boundary:** the contracts are implemented faithfully for ABI compatibility (`gets` as a compat symbol, `tmpnam`, `system`), with the defined-outcome rule S4. Link-time warnings are kept via `.gnu.warning` sections (phase 15). |
| D-H13 | Ancestral duplicates | One internal function. The duplicates are translation aliases, sometimes one function exported under two names. |
| D-H14 | Type-suffix explosion | Generic internals over `Float` and `Int` traits. Exports are per suffix (C3). |
| D-H15 | Retrofitted threading | **Internal:** thread-safe by construction. No `_unlocked` distinction internally: streams take `&mut Stream` under a lock held for a whole operation. **Boundary:** `flockfile` and the `*_unlocked` forms are honoured, because callers rely on the lock scope. |
| D-H16 | Implicit offsets | **Internal:** positioned I/O is the primitive (`read_at`/`write_at`), and cursor I/O is layered on top. **Boundary:** the kernel's shared file offset is kept (⚑ KERN). |
| D-H17 | Unwrapped kernel interfaces | `liella-kernel` wraps the whole modern syscall surface with typed APIs, whether or not POSIX exports them (D-24.6). |
| D-H18 | Signals for asynchrony | **Internal:** descriptor-based notification (`timerfd`, `signalfd`, `pidfd`, `eventfd`, io_uring). **Boundary:** signal-based contracts are reproduced where observable (`alarm`, itimers, `SIGEV_SIGNAL`). Signals are no longer used where the contract does not require them: `lckpwdf` and utmp lock timeouts (D-17.5, D-17.6), and `SIGEV_THREAD` timers (D-23.3). |
| D-T.1 | Directory order in tests | **Owner ruling 2026-10-04.** Results whose order follows directory order (`readdir`, unsorted `scandir`, `GLOB_NOSORT`, unsorted walks) are compared as sets, not as sequences; any order recorded in a behaviour document names the filesystem that produced it; an extra run on tmpfs is added only where order is itself the subject of a test. |

### 7.2 Per-section decisions

Each row gives the spec reference, the annotation it answers, and the decision.
"(observable)" marks a decision that deliberately changes behaviour a C caller could see.
Every such decision is stricter or more capable than the contract, never less.
