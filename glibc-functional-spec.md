# GNU C Library (glibc): Functional Specification

*What glibc provides to programs, stated as contracts, independent of how glibc implements them.*

| | |
|---|---|
| Reference version | glibc 2.39 (tag `glibc-2.39`, sourceware.org), Linux, x86_64 ABI lists |
| Sources consulted | glibc source tree, the GNU C Library Reference Manual (`manual/*.texi`), Linux man-pages (sections 2 and 3, as installed on Ubuntu 24.04), exported ABI lists (`sysdeps/unix/sysv/linux/x86_64/64/*.abilist`), installed headers from `libc6-dev` |
| Scope | Every symbol exported by `libc.so.6`, `libm.so.6`, `ld-linux-x86-64.so.2`, and the auxiliary libraries (`libpthread`, `libdl`, `librt`, `libutil`, `libanl`, `libresolv`, `libnsl`, `libthread_db`, `libc_malloc_debug`, `libBrokenLocale`). Appendix B maps each of the ~3,900 distinct exported names to the section that specifies it. |
| Not in scope | Implementation strategy, internal data structures, performance tuning, the build system, utility programs (`ldconfig`, `iconv`, `locale`, `getent`, `nscd`), except where their behaviour is part of a caller-visible contract. |


## Contents

- [0. How to read this document](#0-how-to-read-this-document)
- [1. Program startup, termination and the program environment](#1-program-startup-termination-and-the-program-environment)
- [2. Error reporting and diagnostics](#2-error-reporting-and-diagnostics)
- [3. Memory allocation](#3-memory-allocation)
- [4. Byte arrays and strings](#4-byte-arrays-and-strings)
- [5. Character classification and case mapping](#5-character-classification-and-case-mapping)
- [6. Multibyte text, wide characters and character-set conversion](#6-multibyte-text-wide-characters-and-character-set-conversion)
- [7. Locales and message translation](#7-locales-and-message-translation)
- [8. Numeric conversion and integer arithmetic](#8-numeric-conversion-and-integer-arithmetic)
- [9. Random numbers](#9-random-numbers)
- [10. Mathematics (`libm`)](#10-mathematics-libm)
- [11. Low-level I/O on file descriptors](#11-low-level-io-on-file-descriptors)
- [12. Buffered streams (`<stdio.h>`)](#12-buffered-streams-stdioh)
- [13. File-system interface](#13-file-system-interface)
- [14. Searching, sorting and pattern matching](#14-searching-sorting-and-pattern-matching)
- [15. Sockets and network interfaces](#15-sockets-and-network-interfaces)
- [16. Name services and databases](#16-name-services-and-databases)
- [17. Users, groups, credentials and login accounting](#17-users-groups-credentials-and-login-accounting)
- [18. Processes](#18-processes)
- [19. Terminals, job control and pseudo-terminals](#19-terminals-job-control-and-pseudo-terminals)
- [20. Signals](#20-signals)
- [21. Non-local control transfer and execution contexts](#21-non-local-control-transfer-and-execution-contexts)
- [22. Threads and synchronization](#22-threads-and-synchronization)
- [23. Time, clocks and timers](#23-time-clocks-and-timers)
- [24. Resource limits, scheduling and system information](#24-resource-limits-scheduling-and-system-information)
- [25. Inter-process communication](#25-inter-process-communication)
- [26. System logging](#26-system-logging)
- [27. Dynamic linking and loading](#27-dynamic-linking-and-loading)
- [28. Profiling and instrumentation](#28-profiling-and-instrumentation)
- [29. Legacy and compatibility subsystems](#29-legacy-and-compatibility-subsystems)
- [30. Catalogue of historical accidents and path dependencies](#30-catalogue-of-historical-accidents-and-path-dependencies)
- [Appendix A. Kernel primitive catalogue (Linux x86_64)](#appendix-a-kernel-primitive-catalogue-linux-x8664)
- [Appendix B. Exported symbol index](#appendix-b-exported-symbol-index)
- [Appendix C. ABI data](#appendix-c-abi-data)
- [Appendix D. System data formats](#appendix-d-system-data-formats)

---

## 0. How to read this document

### 0.1 Purpose

This document specifies *what* glibc promises to callers so that an alternative implementation
can be designed against the contract rather than against the current code. It deliberately
separates three things that glibc's interface fuses together:

1. **The functional need.** What a caller actually needs (for example: "obtain the bytes of a
   file's contents starting at an offset").
2. **The kernel primitive.** What the Linux kernel ABI makes available to satisfy the need.
   The kernel ABI is treated as fixed and inviolable.
3. **The interface convention.** The particular shape the C interface gives the need (for
   example: a `FILE *` with a hidden position, a NUL-terminated name, a global `errno`).

Only (1) and (2) are requirements. (3) is documented so that it can be preserved where
compatibility demands it, and questioned everywhere else.

### 0.2 Entry template

Each function or function family is specified with these fields. Fields that would be empty are
omitted.

* **Contract.** What the operation does, stated as an observable effect.
* **Inputs.** What the caller supplies, and the constraints on it. Constraints that are
  *functionally necessary* are stated plainly. Constraints that are *conventional* are marked.
* **Guarantees.** What holds after a successful return.
* **Failure / edge cases.** How failure is reported, and the behaviour at boundaries.
* **Kernel.** The kernel primitives the contract ultimately requires. "None" means the contract
  is satisfiable in pure user-space computation. Where the need is met by a file or device the
  kernel exposes (`/proc`, `/sys`, `/dev`), that is listed too. Where the need is met by a
  *system configuration convention* that is not part of the kernel (for example `/etc/passwd`),
  it is listed under **Config** instead, because it is not inviolable.

### 0.3 Annotation markers

These are the high-value annotations the brief asks for. They are searchable.

| Marker | Meaning |
|---|---|
| **⚑ HIST** | The interface shape is a historical accident: it was a reasonable choice on a 1970s–1990s system and survives unchanged, but the functional need does not require it. |
| **⚑ COMPAT** | The shape is kept for binary or source compatibility (ABI symbol versions, POSIX/ISO text, other Unixes). It is not required by the functional need, but changing it breaks existing programs. |
| **⚑ PATH** | Path dependency: a later interface was forced into a shape by an earlier interface decision (for example, a `_r` variant added because the original returned a static buffer). |
| **⚑ ARB** | An input constraint that looks arbitrary (a fixed size, a magic limit, an undefined-behaviour trap) rather than functionally necessary. |
| **⚑ KERN** | The shape looks arbitrary but is in fact imposed by the fixed kernel ABI, so any implementation inherits it at the boundary. These are *not* free to change. |
| **⚑ COMMENTARY** | The item is design commentary, not behaviour a program can observe, so it has no test. The marker is followed on the same line by what testable behaviour the item implies and the clause that covers it (`covered by` and the clause ID), or by `nothing testable` and why. (Added 2026-10-05; `cargo xtask stubs` checks the form and counts the items.) |

### 0.4 Abstract types used in this specification

The C interface uses concrete representations. The specification states needs in terms of these
abstract types, and notes the concrete convention separately.

| Abstract type | Functional meaning | Current C convention | Notes |
|---|---|---|---|
| **Bytes** | A finite sequence of octets with a known length. | `(const void *, size_t)` pair. | Already length-delimited where glibc uses it (`mem*`). |
| **Text** | A finite sequence of octets interpreted as characters in some encoding, with a known extent. | NUL-terminated `char *` ("C string"). The terminating 0 byte is the only delimiter. | ⚑ HIST. NUL termination makes length an O(n) scan, forbids embedded 0 bytes, and is the root cause of a large family of overflow bugs and of the `n`/`l`/`_s`/`_chk` variant explosion. It is required only at the kernel boundary (see **Path**). |
| **Path** | A name the kernel resolves in its file-system namespace. | NUL-terminated `char *`. | ⚑ KERN. Every Linux syscall taking a path (`openat`, `execve`, `mkdirat`, ...) reads a NUL-terminated string from user memory. A Path therefore genuinely cannot contain a 0 byte, and an implementation must materialize a NUL-terminated copy at the syscall boundary. The limits `PATH_MAX` (4096, including the terminator) and `NAME_MAX` (255) are also kernel-imposed for a single syscall, but *not* for the file system as a whole (deeper trees are reachable through relative lookups). |
| **Text list** | An ordered sequence of Text values. | NULL-terminated array of NUL-terminated strings (`argv`, `envp`). | ⚑ KERN at `execve` (the kernel reads exactly this layout, and places it on the new process's stack). ⚑ HIST everywhere else. |
| **Wide text** | A sequence of Unicode code points (on Linux). | `wchar_t *`, NUL-terminated. `wchar_t` is 32 bits, UCS-4/UTF-32 on glibc. | ⚑ COMPAT. `wchar_t`'s width is platform-defined (16 bits on Windows), so portable code cannot rely on it being a full code point. |
| **Fd** | A kernel handle to an open file description. | `int`, non-negative. -1 is the "no handle" sentinel. | ⚑ KERN: the kernel issues small non-negative integers and guarantees the *lowest available* number for most allocating calls (POSIX requires this, and programs rely on it to redirect standard streams). |
| **Status** | Success or a specific failure reason. | Return value -1 (or `NULL`, `EOF`, ...) plus a thread-local `errno`; *or* a positive error number returned directly (pthreads, `posix_spawn`, `posix_fadvise`, `getaddrinfo` uses its own codes). | ⚑ HIST. The out-of-band `errno` channel is a legacy of single-threaded Unix. It is now a thread-local lvalue. See §0.6. |
| **Time instant** | A point on a clock's timeline. | `struct timespec {time_t tv_sec; long tv_nsec;}` with `0 ≤ tv_nsec < 1e9`. Older: `struct timeval` (µs), `time_t` (s). | ⚑ KERN for the syscall layout. Three resolutions coexist only for history. |
| **Object size** | A count of bytes or elements. | `size_t`. Signed results use `ssize_t`, reserving negative values for errors. | ⚑ HIST: `ssize_t` halves the representable range to reserve one sentinel. |

### 0.5 Conformance layers

glibc exposes several overlapping standards through feature-test macros (`_GNU_SOURCE`,
`_POSIX_C_SOURCE`, `_XOPEN_SOURCE`, `_DEFAULT_SOURCE`, `_ISOC11_SOURCE`, `_FILE_OFFSET_BITS`,
`_TIME_BITS`, `_FORTIFY_SOURCE`, ...). Feature macros select *declarations* and in a few cases
*semantics*: for example `strerror_r` returns `int` (XSI) or `char *` (GNU), and `basename`
modifies its argument or not, depending on the macro. ⚑ COMPAT: one name with two incompatible
contracts, chosen at compile time, exists only because GNU and XSI diverged before either
standardized. A new design should give the two contracts different names.

Standards referenced: ISO C (C89 through C23), POSIX.1-2008/2017/2024, XSI, BSD, SVID, and
GNU/Linux extensions. Each section notes the origin of an interface where it matters for
the annotations.

### 0.6 Cross-cutting conventions (stated once, apply everywhere)

**C0.1 Error reporting.** Unless an entry says otherwise:

* A failing function returns a sentinel (-1, `NULL`, `EOF`, `(size_t)-1`, `MAP_FAILED`, ...)
  and stores a positive error code in the calling thread's `errno`.
* `errno` is never set to 0 by a library function. A successful call may still change `errno`.
  ⚑ HIST: callers that need to detect errors from functions with no free sentinel (for example
  `strtol`, `readdir`, `getpwnam`, math functions) must zero `errno` before the call. An
  explicit status result would remove that burden.
* The error codes are the kernel's (`EPERM`=1 ... `EHWPOISON`=133 on x86_64). ⚑ KERN: the
  numbering is the kernel ABI. Library-originated failures reuse kernel codes with library
  meanings (for example `EINVAL`, `ENOMEM`, `EOVERFLOW`).
* Newer POSIX interfaces (threads, `posix_spawn`, `posix_fadvise`, `posix_fallocate`,
  `posix_memalign`, `getaddrinfo`, `pthread_*`, `timer_*` are mixed) return the error number
  directly and leave `errno` unspecified. ⚑ PATH: two error conventions coexist in one library.
  A caller must know which convention each function follows.

**C0.2 Threads and reentrancy.** Unless stated, functions are *MT-Safe* in the manual's
terminology (§1.2.2.1 of the manual defines MT-Safe, MT-Unsafe, AS-Safe, AC-Safe with
qualifiers such as `race`, `locale`, `env`, `sig`, `term`, `init`, `lock`, `corrupt`, `heap`,
`dlopen`, `plugin`, `i18n`, `timer`). The specification lists only departures:

* **Static result buffers.** Many classic functions return a pointer to storage owned by the
  library and overwritten by the next call (`strerror` historically, `asctime`, `ctime`,
  `gmtime`, `localtime`, `getpwnam`, `gethostbyname`, `inet_ntoa`, `ttyname`, `strtok`,
  `readdir`, `getlogin`, `ecvt`, `l64a`, `tmpnam(NULL)`, `ptsname`, ...). ⚑ HIST, ⚑ PATH:
  each of these grew a `_r` variant taking a caller buffer. The functional need is always
  "produce a result the caller owns", so in a new design the `_r` shape is the base contract.
* **Process-global mutable state that the contract exposes.** The current locale, the
  environment, the current directory, `umask`, signal dispositions, resource limits,
  the random seed of `rand`/`random`/`drand48`, the `getopt` cursor (`optind`, `optarg`),
  the `strtok` cursor, `h_errno`, `signgam`, the `tzname`/`timezone`/`daylight` variables,
  `environ`, `program_invocation_name`. Several of these are kernel-held (cwd, umask,
  signal dispositions, limits) and cannot be made per-thread. The others are library-held and
  are process-global only for history.
* **Async-signal safety.** Only functions listed as AS-Safe may be called from a signal handler
  that interrupted arbitrary code. POSIX enumerates the list. The functional reason is that
  the handler may run in the middle of a non-reentrant library operation on the same thread.
* **Cancellation points.** In a threaded program, the functions POSIX lists as cancellation
  points (blocking I/O, waits, sleeps, `open`, `close`, ...) may act on a pending deferred
  cancellation. glibc's cancellable wrappers are listed per section. ⚑ HIST: `close` being a
  cancellation point creates a well-known ambiguity about whether the descriptor was closed
  (on Linux it always is).

**C0.3 Restartable system calls.** Blocking primitives interrupted by a signal handler either
restart automatically (handler installed with `SA_RESTART`) or fail with `EINTR`. Some kernel
calls never restart (`select`, `poll`, `epoll_wait`, `nanosleep`, `sigsuspend`, socket calls with
timeouts, ...) and some restart with an adjusted timeout. glibc does not hide this, apart from
a few internal `TEMP_FAILURE_RETRY` loops. ⚑ KERN (restart semantics are kernel ABI),
⚑ HIST (`EINTR` as a caller-visible result is widely considered a design mistake, but the
kernel ABI returns it).

**C0.4 Large files and 64-bit time.** On x86_64, `off_t`, `ino_t`, `blkcnt_t`, `rlim_t`, and
`time_t` are already 64 bits. The `*64` symbols (`open64`, `stat64`, `lseek64`, `fopen64`,
`readdir64`, `getrlimit64`, ...) are exact aliases of the unsuffixed symbols. ⚑ COMPAT: they
exist because on 32-bit ABIs `_FILE_OFFSET_BITS=64` and `_TIME_BITS=64` select 64-bit variants
of hundreds of functions. The specification documents each function once. The 64-bit
variant is the contract, and the 32-bit variant is a compatibility shim that must fail with
`EOVERFLOW` when a value does not fit.

**C0.5 Fortification.** Symbols named `__<name>_chk` (`__memcpy_chk`, `__printf_chk`,
`__read_chk`, ...) implement the same contract as `<name>` with one extra input: the
compiler-known size of the destination object. If the operation would write beyond it, the
process terminates with a diagnostic (`__chk_fail`, "buffer overflow detected"). Format-string
variants also reject `%n` in writable memory. ⚑ PATH: fortification is needed because the
base interfaces do not take destination sizes. An interface that always carried a destination
extent would make the whole `_chk` family unnecessary.

**C0.6 Symbol versioning and compatibility symbols.** Every exported name carries a version
node (`GLIBC_2.2.5` is the x86_64 baseline). Some names have several versions with different
contracts (for example `memcpy@GLIBC_2.2.5` permits overlapping arguments, while
`memcpy@GLIBC_2.14` does not guarantee it; `realpath`, `sched_setaffinity`,
`pthread_cond_*`, `regexec`, `glob`, `posix_spawn`, `nftw`, `fmemopen`, `quick_exit` all
have older versioned contracts). Some names are exported only for old binaries and can no
longer be linked against (`gets`, `matherr`, `__malloc_hook`, `sys_errlist`, `pow10`,
`sigvec`, `ustat`, `stime`, `_sys_siglist`, the RPC/NIS stack moved to `libtirpc`/`libnsl`,
...). ⚑ COMPAT: an ABI-compatible replacement must keep every versioned contract that old
binaries were linked against. Appendix B lists these.

**C0.7 Memory ownership.** Unless stated, pointers the caller passes are borrowed only for the
call's duration. Pointers the library returns are either (a) caller-owned heap memory to be
released with `free`, (b) library-owned static storage (see C0.2), or (c) a view into
caller-supplied storage. Each entry states which. ⚑ HIST: the C interface does not encode
ownership in types, and several functions return different kinds from the same entry point
(`realpath` with and without a buffer, `getcwd(NULL, 0)`, `strerror_r` GNU form).

**C0.8 Kernel primitives beyond system calls.** The kernel ABI that glibc builds on includes more
than syscall numbers:

* The process entry state: stack layout (`argc`, `argv`, `envp`, auxiliary vector), initial
  register state.
* The **auxiliary vector** (`AT_PAGESZ`, `AT_HWCAP`/`AT_HWCAP2`, `AT_RANDOM`,
  `AT_SYSINFO_EHDR`, `AT_SECURE`, `AT_PHDR`, `AT_ENTRY`, `AT_BASE`, `AT_MINSIGSTKSZ`,
  `AT_EXECFN`, `AT_PLATFORM`, `AT_CLKTCK`, `AT_UID`/`AT_EUID`/`AT_GID`/`AT_EGID`).
* The **vDSO**: `clock_gettime`, `gettimeofday`, `time`, `getcpu`, `clock_getres`.
* The **signal frame** format and the `SA_RESTORER` / `rt_sigreturn` convention (x86_64 requires
  user space to supply the return trampoline).
* The **thread pointer** register (FS base on x86_64, set through `arch_prctl(ARCH_SET_FS)` or
  `clone(CLONE_SETTLS)`).
* **Shared user/kernel memory protocols**: futex words, robust-futex lists
  (`set_robust_list`), `clear_child_tid` (`set_tid_address` / `CLONE_CHILD_CLEARTID`), and
  the rseq area (`rseq`).
* **Pseudo-file systems**: `/proc` (`/proc/self/fd`, `/proc/self/maps`, `/proc/self/exe`,
  `/proc/stat`, `/proc/meminfo`, `/proc/sys/...`), `/sys` (`/sys/devices/system/cpu/...`),
  and `/dev` nodes (`/dev/ptmx`, `/dev/pts/*`, `/dev/null`, `/dev/tty`, `/dev/shm` for
  POSIX shared memory (a tmpfs mount convention), `/dev/log` (a syslog daemon socket, not
  kernel)).
* **Netlink** (`NETLINK_ROUTE`) for interface and address enumeration.

Appendix A catalogues all of these against the contracts that need them.

---

## 1. Program startup, termination and the program environment

### 1.1 Process entry and runtime initialization

**Contract.** Before `main` runs, the runtime turns the kernel's entry state into a C
execution environment:

* It passes `argc`, `argv`, and `envp`.
* It makes the environment available as `environ`.
* It initializes the thread pointer and TLS for the initial thread, and the stack-protector
  canary and pointer-guard values from `AT_RANDOM`.
* It initializes the standard streams `stdin`, `stdout` and `stderr`, and the "C" locale.
* It runs ELF constructors (`DT_PREINIT_ARRAY`, `DT_INIT`, `DT_INIT_ARRAY`) in dependency
  order.
* It registers destructors to run at normal exit.
* In secure-execution mode (`AT_SECURE` non-zero, which means setuid, setgid or
  file-capability binaries), it sanitizes the environment.

**Entry points.** `__libc_start_main` (ABI-only; the linker-supplied `_start` calls it),
`__libc_init_first` (compat), `__libc_stack_end` (data), `_dl_*` hooks in `ld.so`.

**Guarantees.**

* `argv[argc]` is `NULL`.
* `envp` is `NULL`-terminated.
* Descriptors 0, 1 and 2 are open when `main` runs. For secure-mode programs, glibc opens
  `/dev/null` on any of 0, 1, 2 that are closed. This prevents a privileged program's later
  `open` from receiving fd 1 or 2 and having its diagnostics written into a sensitive file.
* In secure mode, unsafe environment variables are removed (`LD_PRELOAD`, `LD_LIBRARY_PATH`,
  `GCONV_PATH`, `LOCPATH`, `MALLOC_*`, `TMPDIR`, `NLSPATH`, `HOSTALIASES`, `RES_OPTIONS`,
  `GLIBC_TUNABLES` with unsafe tunables, ...).

**Kernel.** `execve` stack layout (argc/argv/envp/auxv), auxv entries (`AT_RANDOM`,
`AT_SECURE`, `AT_PAGESZ`, `AT_HWCAP*`, `AT_SYSINFO_EHDR`, `AT_PHDR`, `AT_MINSIGSTKSZ`),
`arch_prctl(ARCH_SET_FS)`, `set_tid_address`, `set_robust_list`, `rseq`, `fcntl(F_GETFD)` (to
probe fds 0–2), `openat` (`/dev/null`), `brk`/`mmap` (initial heap and TLS), `mprotect`
(RELRO), `prctl` (for shadow stack on CET kernels).

⚑ HIST. The `argv`/`envp` shape (NULL-terminated arrays of NUL-terminated strings) is
⚑ KERN at the `execve` boundary. Inside the program the need is just "an ordered list of
Text arguments" and "a mapping from names to values". `envp` is passed as a list of
`NAME=VALUE` strings rather than a mapping, so a duplicate name has undefined meaning.

### 1.2 `exit`, `_exit`, `_Exit`, `quick_exit`, `abort`, `atexit`, `on_exit`, `at_quick_exit`, `__cxa_atexit`, `__cxa_finalize`, `__cxa_thread_atexit_impl`, `__cxa_at_quick_exit`

**Contract.**

* **`exit(status)`** runs, in reverse order of registration, all functions registered with
  `atexit`, `on_exit` and `__cxa_atexit`. Registrations made during exit processing also run.
  It then runs thread-local destructors for the calling thread, ELF destructors
  (`DT_FINI_ARRAY`) of loaded objects, flushes and closes all stdio streams, removes
  `tmpfile` files, and terminates the whole process with the status. It never returns.
* **`_exit`/`_Exit(status)`** terminate the process immediately. They skip handlers, stream
  flushing and destructors. Async-signal-safe.
* **`quick_exit(status)`** runs only the `at_quick_exit` handlers (LIFO), then `_Exit`.
* **`abort()`** terminates abnormally with `SIGABRT`:
  * If a handler catches `SIGABRT` and returns, `abort` resets the disposition to default and
    raises the signal again.
  * If the process still survives, it executes an architecture trap instruction and finally
    `_exit(127)`.
  * The raise is not blocked by the signal mask: `abort` unblocks `SIGABRT` first.
  * Streams are *not* flushed (POSIX.1-2008 permits but no longer requires flushing; glibc
    stopped flushing in 2.27).
* **`__cxa_atexit(fn, arg, dso)`** registers a destructor bound to a shared object.
  `__cxa_finalize(dso)` runs that object's destructors when it is unloaded (or all of them at
  exit when `dso` is NULL).
* **`__cxa_thread_atexit_impl(fn, obj, dso)`** registers a destructor for a C++
  `thread_local` object. It runs at thread exit, and it keeps `dso` loaded until then.

**Inputs.**

* **`status`** is an `int`. Only the low 8 bits reach a parent through `wait` (⚑ KERN:
  `exit_group` takes an int, but the wait-status encoding carries 8 bits).
* **`EXIT_SUCCESS`/`EXIT_FAILURE`** are 0 and 1.
* Registration limit: ISO C requires at least 32 `atexit` registrations. glibc has no fixed
  limit and allocates dynamically.

**Failure / edge cases.**

* `atexit`, `at_quick_exit` and `__cxa_atexit` return non-zero if memory cannot be obtained.
* Calling `exit` twice, or from an `atexit` handler, is undefined in ISO C. glibc detects
  reentry: later handlers are still run once.
* `exit` from a multi-threaded process terminates all threads without running their
  cleanup handlers.

**Kernel.** `exit_group` (whole process), `exit` (thread only, used by `pthread_exit` for the
last-thread path), `rt_sigaction`, `rt_sigprocmask`, `tgkill`/`kill` (abort), `write`/`close`
(stream flush).

⚑ HIST. `on_exit` (SunOS) duplicates `atexit` with a status and an argument pointer. The
functional need ("run a closure at exit") is met by one interface with a context pointer. The
`atexit` interface lacks one, and that is why `__cxa_atexit` had to be added for C++.
⚑ PATH. `quick_exit` exists because `exit` runs destructors that can deadlock in a
multi-threaded program. The need ("terminate now, but run a selected, safe subset of
handlers") is legitimate. Two independent handler lists are an artefact of adding it late.

### 1.3 Environment variables: `getenv`, `secure_getenv`, `setenv`, `unsetenv`, `putenv`, `clearenv`, `environ`, `__environ`

**Contract.** The process environment is a mutable mapping from names to values. It is
initialized from `execve`'s `envp` and passed to children by the `exec*` functions that do not
take an explicit environment.

* **`getenv(name)`** returns the value for `name`, or `NULL` if absent.
* **`secure_getenv(name)`** behaves like `getenv`, but returns `NULL` in secure-execution
  mode.
* **`setenv(name, value, overwrite)`** adds or (if `overwrite`) replaces an entry.
* **`unsetenv(name)`** removes *all* entries with that name.
* **`putenv(string)`** inserts the caller's `NAME=VALUE` string itself (not a copy). Later
  changes to that storage change the environment. A string with no `=` removes the name
  (glibc extension).
* **`clearenv()`** empties the environment.

**Inputs.**

* A name must be non-empty and must not contain `=`. `setenv` and `unsetenv` fail with
  `EINVAL` otherwise.
* Names and values are Text. They cannot contain the delimiter because the storage format is
  `NAME=VALUE\0` (⚑ KERN at `execve`, ⚑ HIST in memory).

**Guarantees.**

* `getenv`'s result stays valid until the entry is modified or removed.
* `setenv` copies its arguments.

**Failure / edge cases.**

* `ENOMEM`.
* `EINVAL` for a bad name.
* Duplicate names that came from `execve` are all removed by `unsetenv`. `getenv` returns
  the first.

**Threads.** MT-Unsafe `env`. Concurrent `getenv` and `setenv` are a data race.
⚑ HIST: `environ` is a bare global array that callers may read or replace directly, so the
library cannot synchronize access. The functional need, a process-wide name→value map
inherited across `exec`, would permit a lock-protected or copy-on-write structure. The
catch is that `environ` is ABI.

**Kernel.** None, except `execve`'s `envp`. `AT_SECURE` for `secure_getenv`.

⚑ HIST. `putenv` storing the caller's pointer is why the environment cannot be safely
reclaimed (the library cannot know which entries it owns). `setenv` therefore leaks
replaced values by design.

### 1.4 Program name: `program_invocation_name`, `program_invocation_short_name`, `__progname`, `__progname_full`, `getauxval(AT_EXECFN)`

**Contract.** Provides the `argv[0]` of the running program, both in full and as its final
path component, for diagnostics.

⚑ HIST. The value is whatever the parent passed as `argv[0]`. It is not a reliable identity.
The reliable identity is `/proc/self/exe` or `AT_EXECFN`.

### 1.5 Auxiliary vector: `getauxval`, `__getauxval`, `__getauxval2` (internal)

**Contract.** Returns the value of an auxiliary-vector entry the kernel supplied at `execve`.

**Inputs.** A key (`AT_*`, `unsigned long`).

**Guarantees.** Returns the value if present.

**Failure.** Returns 0 and sets `ENOENT` if absent. ⚑ ARB: 0 is also a legitimate value for
several keys, so the caller must inspect `errno`.

**Kernel.** The auxv block on the initial stack (also readable as `/proc/self/auxv`).

### 1.6 Command-line parsing: `getopt`, `getopt_long`, `getopt_long_only`, `optarg`, `optind`, `opterr`, `optopt`, `__posix_getopt`

**Contract.** Iteratively classifies the elements of an argument list into options, option
arguments and operands, according to a short-option specification string and (for the long
forms) a table of long-option descriptors.

**Inputs.**

* **The argument list** (`argc`, `argv`).
* **The short-option specification**, a Text in which each character is an option letter,
  `:` after it means a required argument, and `::` means an optional one. A leading `+`
  requests POSIX mode (stop at the first operand). A leading `-` returns operands in place as
  option code 1. A leading `:` suppresses diagnostics and distinguishes a missing argument
  (`:`) from an unknown option (`?`).
* **The long-option table** (`struct option {name, has_arg, flag, val}`), terminated by an
  all-zero entry.

**Guarantees.**

* Each call returns the next option's code, or -1 when options are exhausted.
* `optarg` points at the option's argument.
* `optind` is the index of the next element to process.
* By default (GNU mode), `getopt` **permutes** `argv` so that all operands end up at the
  end. Setting `POSIXLY_CORRECT` disables permutation.
* `--` ends option processing.

**Failure / edge cases.**

* An unknown option, or a missing required argument, returns `'?'` (or `':'`) and prints a
  diagnostic to `stderr` unless `opterr == 0`.
* A long option may be abbreviated to any unambiguous prefix. An ambiguous prefix is an
  error.

**Kernel.** None (diagnostics use `write` on fd 2 via stdio).

⚑ HIST. The parser's state (`optind`, `optarg`, `optopt`, `opterr`, and a hidden cursor
inside the current element) is process-global. Parsing two argument vectors, or re-parsing
one, requires resetting `optind = 0` (a GNU convention; POSIX says 1). The need is a
re-entrant iterator over a caller-owned parser state.
⚑ HIST. Mutating the caller's `argv` (permutation) is a surprising side effect, and is the
reason `argv` is declared `char *const []` yet modified.
⚑ COMPAT. `__posix_getopt` exists so that a POSIX-mode program gets non-permuting
behaviour through symbol redirection.

### 1.6a Suboption parsing: `getsubopt`

**Contract.** Parse a comma-separated option Text (`"rw,size=10"`) against a NULL-terminated
list of recognized names. Each call returns the index of the next suboption, or -1 if it is
unknown, and sets a pointer to its `=value` (or `NULL`); for an unknown suboption the pointer is
the whole `name[=value]` text. The input is modified in place
(commas and `=` are overwritten with terminators) and a cursor is advanced.

**Kernel.** None.

⚑ HIST. The parse is destructive, and there is no way to escape a comma.

### 1.7 Argument parsing framework: `argp_parse`, `argp_help`, `argp_state_help`, `argp_usage`, `argp_error`, `argp_failure`, `argp_program_version`, `argp_program_version_hook`, `argp_program_bug_address`, `argp_err_exit_status`, `_argp_unlock_xinl`

**Contract.** A declarative command-line parser. The caller supplies a tree of option tables
(`struct argp`, which may nest child parsers) and a callback. The library:

* parses the list and calls the callback for each option, each operand, and the
  initialization/finalization events;
* generates `--help`, `--usage` and `--version` output, formatted to the terminal width
  (`ARGP_HELP_FMT` tunes this);
* reports errors in the standard `program: message` format.

**Inputs.** Option descriptors (`name`, `key`, `arg`, `flags`, `doc`, `group`). Flags such
as `ARGP_NO_EXIT`, `ARGP_IN_ORDER`, `ARGP_NO_HELP`, `ARGP_PARSE_ARGV0` (honoured only together with `ARGP_NO_ERRS`), `ARGP_SILENT`.

**Guarantees.** On success returns 0 and optionally the index of the first unparsed
argument. By default the process exits with `argp_err_exit_status` (default `EX_USAGE`) on
a usage error.

**Failure.** An error code returned from the callback aborts the parse. `ARGP_ERR_UNKNOWN`
means "not mine; try child parsers".

**Kernel.** None beyond stream output. Help formatting reads `COLUMNS`.

⚑ HIST. Default process exit on error, and the global configuration variables
(`argp_program_version`, ...), make the framework hard to embed in a library. The need is
a parser that returns a structured result or error.

### 1.8 Tunables and runtime configuration

**Contract.** The `GLIBC_TUNABLES` environment variable (`name=value:name=value`) adjusts
library parameters at startup: malloc arena counts and thresholds, tcache sizes, thread stack
cache, CPU feature masks (`glibc.cpu.hwcaps`), `glibc.rtld.*`, `glibc.pthread.rseq`, and
others. Unknown or invalid tunables are ignored. In secure-execution mode, only tunables
marked safe are honoured.

**Programmatic interface.** None exported for general use (`__tunable_get_val` is internal
and exported only from `ld.so`).

⚑ HIST. Configuration by environment string, parsed before `main`, is the only channel
available before any user code runs. The legacy `MALLOC_*_` variables predate tunables and
are still honoured.

---

## 2. Error reporting and diagnostics

### 2.1 `errno`, `__errno_location`

**Contract.** A per-thread modifiable integer lvalue that holds the most recent error code set
by a library function on this thread. `errno` is a macro that expands to
`(*__errno_location())`.

**Guarantees.** Each thread has its own `errno`, and signal handlers see the interrupted
thread's `errno`. Portable handlers save and restore it.

**Kernel.** None. The kernel returns `-errno` in the syscall result register, and the library
transfers it to `errno`.

⚑ HIST. See C0.1. `errno` was a single global `int` in 1970s Unix. Its survival as an lvalue
macro forces every error-returning function to communicate through thread-local state rather
than through its return value, and makes "did it fail?" ambiguous for functions whose
sentinel is also a valid result.

### 2.2 Error text: `strerror`, `strerror_r` (XSI and GNU), `__xpg_strerror_r`, `strerror_l`, `strerrorname_np`, `strerrordesc_np`, `perror`, `sys_errlist`, `sys_nerr`, `_sys_errlist`, `_sys_nerr`

**Contract.** Map an error number to a message.

* **`strerror(e)`** returns a locale-translated message. The result is a pointer that stays
  valid until the next `strerror` call on the same thread (glibc uses a per-thread buffer
  only for unknown codes; known codes return a static string).
* **`strerror_l(e, loc)`** does the same using locale object `loc`.
* **`strerror_r(e, buf, n)`** has two contracts selected by feature macro (§0.5):
  * **XSI form (`__xpg_strerror_r`)** writes into `buf` and returns 0, `EINVAL` (unknown
    `e`) or `ERANGE` (buffer too small).
  * **GNU form** returns a `char *` that *may or may not* point into `buf` (it points at a
    static string for known codes).
* **`strerrorname_np(e)`** returns the symbolic name (`"EINVAL"`), or `NULL` for an
  unknown code.
* **`strerrordesc_np(e)`** returns the untranslated English description.
* **`perror(s)`** writes `s: <message>\n` (or just the message if `s` is NULL or empty) to
  `stderr`. It does not change the orientation of `stderr` if the stream is unoriented, and
  honours it if wide.

**Failure / edge cases.** Unknown codes yield `"Unknown error N"`.

**Kernel.** None (translation catalogs are files; see §7).

⚑ COMPAT. `sys_errlist[]`/`sys_nerr` are an exported array indexed by errno. They were
removed from the API in 2.32 (compat symbols only) because the array's size is baked into
binaries: adding an errno changes the ABI. The need (enumerating messages) is met by a
function.
⚑ HIST. The GNU and XSI `strerror_r` share one name with incompatible return types.

### 2.3 BSD and GNU diagnostic helpers: `err`, `errx`, `verr`, `verrx`, `warn`, `warnx`, `vwarn`, `vwarnx`, `error`, `error_at_line`, `error_message_count`, `error_one_per_line`, `error_print_progname`

**Contract.** Print `progname: <formatted message>[: <strerror(errno)>]\n` to `stderr`.

* `err*` then call `exit(status)`.
* `error(status, errnum, fmt, ...)` exits if `status != 0`. It includes the message for
  `errnum` if non-zero, flushes `stdout` first, and counts messages in
  `error_message_count`.
* `error_at_line` additionally prints `file:line:`. With `error_one_per_line`, it suppresses
  repeated messages for the same line.
* `error_print_progname`, if set, replaces the program-name prefix.

**Kernel.** None (stream I/O, then `exit_group`).

⚑ HIST. Two families (BSD `err` and GNU `error`) provide the same need with different
argument orders.

### 2.4 Assertions: `assert` (macro), `__assert_fail`, `__assert_perror_fail`, `__assert`, `assert_perror`

**Contract.** If the asserted expression is false and `NDEBUG` is not defined, print
`prog: file:line: function: Assertion 'expr' failed.` and call `abort`. `assert_perror(e)`
(GNU) does the same when `e != 0`, showing `strerror(e)`.

**Kernel.** `write` (fd 2), then `abort` (§1.2).

### 2.5 Fatal internal diagnostics: `__libc_fatal` (internal), `__chk_fail`, `__fortify_fail` (internal), `__stack_chk_fail`, `__stack_chk_guard` (not on x86_64)

**Contract.**

* **`__stack_chk_fail`** is called by compiler-inserted code when a stack canary has changed.
  It prints `*** stack smashing detected ***: terminated` and aborts.
* **`__chk_fail`** is called when a fortified function detects an overflow (C0.5). It prints
  `*** buffer overflow detected ***: terminated` and aborts.

**Guarantees.** No return. The diagnostic uses only async-signal-safe output, so it is
written even from a corrupted heap state.

**Kernel.** `write` (to `/dev/tty` or fd 2), `rt_sigprocmask`, `tgkill` (`SIGABRT`).

### 2.6 Message formatting for system administration: `fmtmsg`, `addseverity`

**Contract.** Formats a structured message from these fields and writes it to `stderr`, the
console, or both:

* **classification**: `MM_HARD`/`MM_SOFT`/`MM_FIRM`, `MM_APPL`/`MM_UTIL`/`MM_OPSYS`,
  `MM_RECOVER`/`MM_NRECOV`, `MM_PRINT`/`MM_CONSOLE`;
* **label**: `"xxx:yyy"`, a component and subcomponent of at most 10 and 14 characters;
* **severity**: `MM_HALT`, `MM_ERROR`, `MM_WARNING`, `MM_INFO`, or a severity added with
  `addseverity`;
* **text**, **action**, **tag**.

`MSGVERB` selects the fields to print and `SEV_LEVEL` defines extra severities.

**Failure.** Returns `MM_OK`, `MM_NOTOK`, `MM_NOMSG` (stderr failed) or `MM_NOCON` (console
failed).

**Kernel.** `write`, `openat("/dev/console")`.

⚑ HIST. SVID interface. Its label length limits (10 and 14 characters) are ⚑ ARB.

### 2.7 Execution tracing: `backtrace`, `backtrace_symbols`, `backtrace_symbols_fd`

**Contract.**

* **`backtrace(buf, n)`** stores up to `n` return addresses of the current call stack into
  `buf` and returns the count.
* **`backtrace_symbols(addrs, n)`** returns a single `malloc`ed block holding `n` Text
  descriptions (`object(symbol+offset) [address]`).
* **`backtrace_symbols_fd`** writes the same descriptions to a file descriptor, without
  allocating.

**Edge cases.**

* The first call may load the unwinder (`libgcc_s`) via `dlopen`, so it is not
  async-signal-safe unless primed.
* Frames without unwind information may be omitted.
* Symbol names are available only for dynamic symbols.

**Kernel.** `openat`/`mmap` (`dlopen` of the unwinder), `write` (`_fd` variant).

⚑ HIST. The result of `backtrace_symbols` is one allocation holding both the pointer array
and the strings. Callers must free only the outer pointer.

---

## 3. Memory allocation

### 3.1 General-purpose heap: `malloc`, `free`, `calloc`, `realloc`, `reallocarray`, `free_sized`\*, `aligned_alloc`, `memalign`, `posix_memalign`, `valloc`, `pvalloc`, `malloc_usable_size`, `cfree`

(\* `free_sized`/`free_aligned_sized` are C23 and are **not** in 2.39.)

**Contract.** Provide dynamically sized, suitably aligned blocks of writable memory whose
lifetime the caller controls.

* **`malloc(n)`** returns a block of at least `n` bytes, aligned for any fundamental type
  (`alignof(max_align_t)`, 16 on x86_64).
* **`calloc(k, n)`** returns a zero-filled block for `k` elements of `n` bytes. If `k*n`
  overflows `size_t`, it fails with `ENOMEM`.
* **`realloc(p, n)`** resizes block `p`, preserving `min(old, n)` bytes. It may move the
  block.
  * `realloc(NULL, n)` is `malloc(n)`.
  * `realloc(p, 0)` frees `p` and returns `NULL` in glibc. ⚑ HIST: implementations disagree
    (it may return a unique pointer), and C23 made the case undefined.
* **`reallocarray(p, k, n)`** is `realloc` with an overflow-checked product.
* **`free(p)`** releases a block. `free(NULL)` is a no-op. `free` preserves `errno`
  (2.33+).
* **Aligned forms:**
  * `aligned_alloc(a, n)` (C11) requires a power-of-two `a`. glibc accepts any `n`; C11
    required `n` to be a multiple of `a` until DR 460.
  * `posix_memalign(&p, a, n)` requires `a` to be a power of two and a multiple of
    `sizeof(void*)`. It returns the error number directly (`EINVAL`/`ENOMEM`) and leaves `*p`
    unmodified on failure.
  * `memalign(a, n)` (obsolete) rounds a non-power-of-two `a` up.
  * `valloc(n)` returns page-aligned memory.
  * `pvalloc(n)` returns page-aligned memory with `n` rounded up to a whole page.
* **`malloc_usable_size(p)`** returns the number of bytes actually usable in the block, which
  is at least the requested size.
* **`cfree`** is a compat alias for `free`.

**Inputs.** Sizes are `size_t`. Requests above `PTRDIFF_MAX` fail with `ENOMEM` (2.29+).
The functional reason is that pointer subtraction within an object must be representable.

**Guarantees.**

* Distinct live blocks never overlap.
* The contents of `malloc` and `realloc` growth are indeterminate. `calloc` contents are
  zero.
* A block's alignment is at least the fundamental alignment, or the requested alignment.
* Memory returned is not shared with any other live block, or with library-internal
  structures.

**Failure / edge cases.**

* Failure returns `NULL` with `ENOMEM` (except `posix_memalign`).
* `malloc(0)` returns a unique, freeable, non-NULL pointer in glibc. ⚑ ARB: the standard
  allows either this or NULL.
* Double `free`, `free` of a non-heap pointer, and use after free are undefined behaviour.
  glibc detects many of these and aborts (`free(): invalid pointer`, `double free detected in
  tcache 2`, `malloc(): corrupted top size`), but does not guarantee to.
* A `realloc` failure leaves the original block intact.

**Threads.** MT-Safe. A block may be freed by a different thread than the one that allocated
it. Allocation after `fork` in a multi-threaded parent is supported: glibc reinitializes the
allocator locks in the child, which is stronger than POSIX requires.

**Kernel.** `brk` (grow and shrink the main arena), `mmap` (anonymous; large blocks and
secondary arenas), `munmap`, `mremap` (large-block `realloc`), `mprotect` (arena growth),
`madvise(MADV_DONTNEED)` (returning free memory) and `madvise(MADV_HUGEPAGE)` (THP tunable),
`prctl(PR_SET_VMA)` (the 2.39 `decorate_maps` tunable). `/sys/kernel/mm/transparent_hugepage/*`
is read for THP tunables.

⚑ HIST. The size of a block is not given back to `free`. The allocator must record it
itself, which is a per-block overhead. C23 added `free_sized`, not yet in 2.39. The functional
need is "release the block I got", and the size is information the caller usually has.
⚑ PATH. Five aligned-allocation entry points exist, with three different argument-validity
rules and two error conventions, because each was added by a different standard body.

### 3.2 Allocator introspection and control: `mallopt`, `mallinfo`, `mallinfo2`, `malloc_stats`, `malloc_info`, `malloc_trim`, `M_*` constants

**Contract.**

* **`mallopt(param, value)`** adjusts allocator policy: `M_MMAP_THRESHOLD`, `M_MMAP_MAX`,
  `M_TRIM_THRESHOLD`, `M_TOP_PAD`, `M_ARENA_MAX`, `M_ARENA_TEST`, `M_PERTURB`,
  `M_CHECK_ACTION`, `M_MXFAST`. It returns 1 on success and 0 on error.
* **`mallinfo2()`** returns a structure of `size_t` statistics (arena bytes, free chunks,
  mmapped bytes, in-use bytes, releasable bytes).
* **`mallinfo()`** returns the same statistics as `int` fields.
* **`malloc_stats()`** prints per-arena statistics to `stderr`.
* **`malloc_info(options, fp)`** writes an XML description to `fp`. `options` must be 0;
  otherwise it fails with `EINVAL`. It returns 0, or -1 with `errno` set (Written 2026-10-04 (owner ruling, phase 12-13 report);
  malloc_info(3); the reference returns 22).
* **`malloc_trim(pad)`** returns free memory to the system, leaving `pad` bytes at the top of
  the main heap. It returns 1 if any memory was released.

⚑ COMPAT. `mallinfo` truncates counts above 2 GiB because its fields are `int`. `mallinfo2`
fixes this, and `mallinfo` is deprecated.
⚑ HIST. These interfaces expose the concepts of one particular implementation (arenas, fast
bins, a top chunk, an mmap threshold). A different allocator cannot give them their current
meanings. The implementation-neutral needs are: "report bytes in use / held / releasable",
and "release unused memory to the OS".

**Kernel.** As in §3.1, plus `madvise` for trim.

### 3.3 Allocator hooks and debugging: `__malloc_hook`, `__free_hook`, `__realloc_hook`, `__memalign_hook`, `__after_morecore_hook`, `__malloc_initialize_hook`, `__morecore`, `__default_morecore`, `malloc_get_state`, `malloc_set_state`, `mcheck`, `mcheck_pedantic`, `mcheck_check_all`, `mprobe`, `mtrace`, `muntrace`, `mallwatch`, `tr_break`

**Contract (current).**

* The hook variables are **compat-only** (2.34 removed their effect from `libc.so`). Old
  binaries can link to them, but they are inert in `libc.so.6`: assigning them does nothing.
  With `libc_malloc_debug.so` preloaded, only `__malloc_hook`, `__free_hook`, `__realloc_hook`,
  `__memalign_hook` and `__malloc_initialize_hook` act; `__after_morecore_hook` and
  `__morecore` stay inert (Written 2026-10-04 (owner ruling, phase 12-13 report), `spec/behaviour/mallocdbg.md`).
* **`mcheck(abortfn)`** (with `libc_malloc_debug.so`) enables consistency checking of every
  block. It must be called before the first allocation. `mprobe(p)` returns the status of one
  block (`MCHECK_OK`, `MCHECK_HEAD`, `MCHECK_TAIL`, `MCHECK_FREE`, `MCHECK_DISABLED`).
* **`mtrace()`** (with `libc_malloc_debug.so`) logs each allocation and free to the file named
  by `MALLOC_TRACE`. `muntrace()` stops logging.
* **`malloc_get_state`/`malloc_set_state`** are compat stubs (Emacs unexec). They now fail.

⚑ HIST. The hooks allowed interposing on the allocator without symbol interposition. They
were removed because the hook variables were writable function pointers inside libc, which
made them a well-known exploitation target. The functional need (replace or wrap the
allocator) is met by ELF symbol interposition of `malloc`/`free`/`calloc`/`realloc`, which
glibc supports and documents.

### 3.4 Stack allocation: `alloca` (macro/builtin), `__libc_alloca_cutoff` (internal)

**Contract.** Allocate `n` bytes in the caller's stack frame. The storage is released when the
function returns.

**Failure.** There is no failure indication. Exceeding the stack is undefined, and in
practice triggers `SIGSEGV` on the guard page.

**Kernel.** None directly. It depends on the kernel's stack growth and on the thread stack
guard page.

⚑ HIST. The unreported failure is an interface defect. The need, "scratch memory with
automatic lifetime", is met by C99 variable-length arrays, which have the same defect.

### 3.5 Obstacks: `_obstack_begin`, `_obstack_begin_1`, `_obstack_newchunk`, `_obstack_free`, `obstack_free`, `_obstack_memory_used`, `_obstack_allocated_p`, `obstack_alloc_failed_handler`, `obstack_exit_failure`, plus ~30 macros (`obstack_alloc`, `obstack_grow`, `obstack_finish`, `obstack_1grow`, `obstack_blank`, `obstack_object_size`, ...)

**Contract.** A stack-discipline arena. Objects are allocated in order and freed by releasing
an object together with everything allocated after it. The arena also supports *growing* an
object incrementally before finalizing it.

**Inputs.** Chunk-size hint, alignment mask, and caller-supplied chunk allocation and free
functions.

**Guarantees.**

* Finished objects never move.
* A growing object may move until it is finished.
* Alignment follows the obstack's alignment mask (default: fundamental alignment).

**Failure.** On allocation failure, `obstack_alloc_failed_handler` is called. Its default
prints a message and calls `exit(obstack_exit_failure)`. ⚑ HIST: failure by exiting from
inside a library, configured through a global handler.

**Kernel.** None (it uses the supplied allocator).

### 3.6 Page-level memory mapping: see §11.10 (`mmap`, `munmap`, `mprotect`, `mremap`, `madvise`, `mlock`, `msync`, `mincore`, `memfd_create`, `pkey_*`).

### 3.7 Program break: `brk`, `sbrk`, `__curbrk`

**Contract.** `brk(addr)` sets the end of the data segment. `sbrk(delta)` moves it by
`delta` and returns the old end.

**Failure.** `ENOMEM`.

**Kernel.** `brk` (the kernel syscall returns the new break rather than a status; glibc
converts that result to the POSIX contract).

⚑ HIST. The program break is a single process-wide contiguous region. It dates from before
virtual memory mapping existed. Using it concurrently with `malloc` is unsafe. It was removed
from POSIX in 2001, and only `mmap` is needed.

---

## 4. Byte arrays and strings

**Section-wide notes.**

* All functions in this section are pure computations. **Kernel: none.**
* All are MT-Safe and AS-Safe, except where stated (`strtok`, `strerror`, locale-dependent
  collation).
* Overlapping source and destination is undefined behaviour unless the entry says otherwise.
* ⚑ HIST (section-wide). Almost every function here exists in two or more forms whose only
  difference is *how the extent of an input is delimited*:
  * byte count (`mem*`);
  * NUL terminator (`str*`);
  * NUL terminator *or* a count, whichever comes first (`strn*`);
  * a destination capacity (`strl*`, `*_chk`).
* The functional needs are fewer than 20: copy, move, fill, compare, search for a byte,
  search for a subsequence, search for any of a set, measure, concatenate, tokenize,
  duplicate. Each need is multiplied by three to six delimiting conventions, and again by
  the wide-character (`wcs*`/`wmem*`) versions.
  ⚑ COMMENTARY: implies that one need gives one answer under every delimiting convention; covered by `s4_n_4`.
* A design built on length-delimited **Text** needs one form per need. It needs
  NUL-terminated forms only at the kernel boundary (Path, argv).
  ⚑ COMMENTARY: nothing testable (an internal design rule, not observable through the C interface).

### 4.1 Copy and move: `memcpy`, `memmove`, `mempcpy`, `__mempcpy`, `memccpy`, `bcopy`, `strcpy`, `stpcpy`, `__stpcpy`, `strncpy`, `stpncpy`, `__stpncpy`, `strlcpy`, `wmemcpy`, `wmemmove`, `wmempcpy`, `wcscpy`, `wcpcpy`, `wcsncpy`, `wcpncpy`, `wcslcpy`

**Contract.** Copy a sequence of bytes (or wide characters) from source to destination.

* **`memcpy(d, s, n)`** copies `n` bytes. Overlap is undefined. ⚑ COMPAT:
  `memcpy@GLIBC_2.2.5` tolerates overlap for old binaries; `@GLIBC_2.14` does not.
* **`memmove`** copies correctly under overlap.
* **`mempcpy`** returns `d + n` instead of `d`.
* **`memccpy(d, s, c, n)`** copies up to and including the first byte equal to `c`, at most
  `n` bytes. It returns a pointer past the copied `c`, or `NULL` if `c` was not found.
* **`bcopy(s, d, n)`** is `memmove` with the argument order reversed. ⚑ HIST (BSD); removed
  from POSIX in 2008.
* **`strcpy(d, s)`** copies `s` including its terminator. `stpcpy` returns a pointer to the
  copied terminator (useful for appending).
* **`strncpy(d, s, n)`** copies at most `n` bytes. If `s` is shorter, it pads `d` with zeros
  to exactly `n` bytes. If `s` is `n` bytes or longer, `d` is **not** terminated.
  ⚑ HIST: `strncpy` was designed for fixed-width, zero-padded fields (`utmp`, 14-byte
  directory entries of V7 Unix), not for bounded string copy. It is routinely misused as a
  bounded copy.
* **`stpncpy`** is `strncpy` returning `d + min(strlen(s), n)`.
* **`strlcpy(d, s, n)`** (2.38, from BSD) copies at most `n-1` bytes and always terminates if
  `n > 0`. It returns `strlen(s)`, so a result `≥ n` means truncation. ⚑ HIST: it reads all of
  `s` even when truncating.
* **Wide forms** are the same contracts over `wchar_t` units.

**Inputs.** Valid, non-overlapping regions of the stated sizes. For `str*`, the source must
be terminated. For `n` variants, `n` may be 0.

**Guarantees.** On return the destination holds the stated bytes. No other bytes are
written, except `strncpy`'s padding.

**Failure.** None reported. A destination overflow is undefined behaviour (C0.5 fortify
variants turn it into a controlled abort).

### 4.2 Fill: `memset`, `bzero`, `explicit_bzero`, `__explicit_bzero_chk`, `wmemset`

**Contract.**

* **`memset(d, c, n)`** sets `n` bytes to `(unsigned char)c`.
* **`bzero(d, n)`** equals `memset(d, 0, n)`. ⚑ HIST.
* **`explicit_bzero(d, n)`** zeroes the bytes, and the zeroing is guaranteed not to be
  optimized away by the compiler, even when the object is dead afterwards. This is the
  functional need behind clearing secrets. It is a contract with the compiler, not with the
  machine: copies in registers or on the stack are not covered.

### 4.3 Compare: `memcmp`, `bcmp`, `timingsafe_bcmp`\*, `strcmp`, `strncmp`, `strcasecmp`, `strncasecmp`, `strcasecmp_l`, `strncasecmp_l`, `strcoll`, `strcoll_l`, `__strcoll_l`, `strverscmp`, `wmemcmp`, `wcscmp`, `wcsncmp`, `wcscasecmp`, `wcsncasecmp`, `wcscasecmp_l`, `wcsncasecmp_l`, `wcscoll`, `wcscoll_l`

(\* not in glibc.)

**Contract.** Establish a total order between two sequences.

* **`memcmp`/`strcmp`/`strncmp`** compare lexicographically, treating bytes as
  `unsigned char`. The result's sign is the order; only the sign is specified.
* **`bcmp`** reports only equal or not equal. ⚑ HIST.
* **`__memcmpeq`** (2.35) has the equality-only contract of `bcmp`, under a reserved name. It
  exists so that compilers can emit calls to it when only equality matters.
* **`*casecmp`** compare after mapping each byte through the current locale's `tolower`.
  ⚑ HIST: byte-at-a-time case folding is wrong for multibyte encodings (UTF-8) and for
  language-specific folding (Turkish dotless i). The functional need, case-insensitive
  equality of text, requires a Unicode-aware fold, and the interface cannot express one.
* **`strcoll`/`wcscoll`** compare according to the collation rules of `LC_COLLATE`
  (multi-level ISO 14651 ordering). With the "C" locale this is `strcmp`.
* **`strverscmp`** (GNU) compares strings that contain embedded decimal numbers, so that
  "file9" < "file10". It treats leading zeros as fractional parts.
* **`_l` variants** take an explicit locale object instead of the thread's current locale.
  ⚑ PATH: `_l` variants were added throughout the library once per-thread and
  explicit locales arrived (§7). In a new design an explicit locale (or none) is the base
  contract.

**Guarantees.** The comparisons are consistent (a total preorder for a fixed locale).
`strcoll` agrees with the ordering of `strxfrm` outputs compared with `strcmp`.

**Failure.** `strcoll` may set `errno` (for example `EINVAL` for invalid multibyte input)
and still return a result. ⚑ ARB: there is no sentinel, so detection requires zeroing
`errno`.

### 4.4 Collation keys: `strxfrm`, `strxfrm_l`, `wcsxfrm`, `wcsxfrm_l`

**Contract.** Transform a string into a key such that `strcmp` on two keys gives the same
order as `strcoll` on the originals. Writes at most `n` bytes including the terminator and
returns the full key length (excluding the terminator). A result `≥ n` means the output is
incomplete, and the contents of the destination are then unspecified.

⚑ HIST. The "call twice, first to measure" protocol exists because the key length is not
predictable and the caller supplies the buffer.

### 4.5 Search for a byte or character: `memchr`, `rawmemchr`, `__rawmemchr`, `memrchr`, `strchr`, `strchrnul`, `strrchr`, `index`, `rindex`, `wmemchr`, `wcschr`, `wcschrnul`, `wcsrchr`

**Contract.**

* **`memchr(s, c, n)`** returns the first position in `s[0..n)` holding `(unsigned char)c`,
  or `NULL`.
* **`memrchr`** returns the last such position.
* **`rawmemchr(s, c)`** searches without a bound. The caller asserts that `c` occurs.
* **`strchr(s, c)`** returns the first occurrence of `c` in the terminated `s`. The
  terminator itself is searchable: `strchr(s, 0)` returns the end.
* **`strchrnul`** returns the end of the string instead of `NULL`.
* **`strrchr`** returns the last occurrence.
* **`index`/`rindex`** are BSD names for `strchr`/`strrchr`. ⚑ HIST.

⚑ ARB. These take `const char *` and return `char *`, discarding const. That is a C language
limitation, and C23 adds type-generic macros to fix it.

### 4.6 Search for a subsequence: `memmem`, `strstr`, `strcasestr`, `wcsstr`, `wcswcs`

**Contract.** Return the first position at which the needle occurs in the haystack, or
`NULL`. An empty needle matches at the start. `strcasestr` compares with locale case
folding.

**Guarantees (glibc).** Worst-case linear time (a two-way string-matching algorithm). Callers
may not rely on this; it is a quality-of-implementation property, not a contract.

⚑ HIST. `wcswcs` is an XPG4 name for `wcsstr`.

### 4.7 Span and set search: `strspn`, `strcspn`, `strpbrk`, `wcsspn`, `wcscspn`, `wcspbrk`, plus the compat inline helpers `__strspn_c1..3`, `__strcspn_c1..3`, `__strpbrk_c2..3`

**Contract.**

* **`strspn(s, set)`** returns the length of the longest prefix of `s` consisting only of
  bytes in `set`.
* **`strcspn`** returns the length of the longest prefix consisting only of bytes *not* in
  `set`.
* **`strpbrk`** returns a pointer to the first byte of `s` that is in `set`.

**Inputs.** `set` is a Text interpreted as a *set* of bytes. Order and duplicates do not
matter, and the byte 0 cannot be a member. ⚑ HIST: a set of bytes delimited by NUL.

⚑ COMPAT. The `__str*_c*` symbols are ABI stubs for inline expansions that old versions of
`<bits/string2.h>` emitted.

### 4.8 Length: `strlen`, `strnlen`, `__strnlen`, `wcslen`, `wcsnlen`

**Contract.** `strlen(s)` returns the number of bytes before the terminator. `strnlen(s, n)`
returns `min(strlen(s), n)` and reads no more than `n` bytes.

⚑ HIST. The length is recomputed by scanning because the representation does not store it.

### 4.9 Concatenate: `strcat`, `strncat`, `strlcat`, `wcscat`, `wcsncat`, `wcslcat`

**Contract.**

* **`strcat(d, s)`** appends `s` to the terminated string in `d`.
* **`strncat(d, s, n)`** appends at most `n` bytes of `s`, then a terminator. `n` bounds the
  *source*, not the destination. ⚑ ARB: this is a frequent source of overflow bugs.
* **`strlcat(d, s, size)`** appends while keeping the total length at most `size-1`, and
  returns `strlen(d_initial) + strlen(s)`. If `d` is not terminated within `size`, it returns
  `size + strlen(s)`.

### 4.10 Duplicate: `strdup`, `__strdup`, `strndup`, `__strndup`, `wcsdup`, `strdupa`/`strndupa` (macros, stack)

**Contract.** Return a new `malloc`ed copy (or, for the `a` forms, a stack copy) of the string,
or of at most `n` bytes of it, always terminated.

**Failure.** `NULL` with `ENOMEM`.

### 4.11 Tokenize: `strtok`, `strtok_r`, `__strtok_r`, `strsep`, `__strsep_g` and `__strsep_1c..3c` (compat), `__strtok_r_1c` (compat), `wcstok`

**Contract.** Split a mutable string into tokens separated by bytes from a delimiter set.

* **`strtok(s, delim)`** skips leading delimiters, terminates the token in place (writes 0),
  and remembers the position in hidden process-global state. It then returns successive
  tokens when called with `s == NULL`. Empty tokens are never returned.
* **`strtok_r`** keeps the position in a caller variable.
* **`strsep(&s, delim)`** (BSD) returns empty tokens, advancing `*s` past the delimiter, and
  sets `*s = NULL` after the last token.
* **`wcstok`** is `strtok_r` for wide strings (ISO C gave it three parameters).

⚑ HIST. `strtok`'s hidden global cursor makes it non-reentrant. ⚑ PATH: `strtok_r` was
added to fix that.
⚑ HIST. All forms destroy their input by writing terminators into it, because the only way to
yield a Text is to terminate it. A length-delimited design yields (start, length) views and
leaves the input intact.

### 4.12 Miscellaneous string utilities

* **`strfry(s)`** (GNU): randomly permutes the bytes of `s` in place. It is not suitable
  for cryptography. **Kernel:** `clock_gettime` (the seed; vDSO).
* **`memfrob(p, n)`** (GNU): XORs each byte with 42. It is its own inverse and provides no
  confidentiality. ⚑ HIST (a joke function kept for ABI).
* **`strsignal(sig)`**: see §20.8.
* **`basename(path)`** has two contracts selected by `<libgen.h>` (§0.5):
  * The **GNU** form (`<string.h>`) returns a pointer after the last `/`, so `"a/"` yields
    `""`, and never modifies the input.
  * The **XPG** form (`__xpg_basename`, via `<libgen.h>`) strips trailing slashes, may modify
    the input, and returns `"."` for an empty or NULL input.
* **`dirname(path)`** (`<libgen.h>`) returns the parent-directory component, modifying the
  input. `"/"` yields `"/"`, and `"a"` or empty yields `"."`.
  * ⚑ HIST: modifying the input is required because the interface returns a Text that must
    be terminated.
  * ⚑ ARB: `dirname` and `basename` are purely lexical and ignore symbolic links, so
    `dirname("a/..")` is `"a"`.
* **`ffs`, `ffsl`, `ffsll`**: see §8.9.
* **`swab(from, to, n)`**: copies `n` bytes, exchanging each adjacent pair (for byte-order
  conversion of 16-bit data). An odd final byte is dropped. ⚑ HIST.

### 4.13 Argz vectors: `argz_create`, `argz_create_sep`, `argz_count`, `argz_extract`, `argz_stringify`, `argz_add`, `argz_add_sep`, `argz_append`, `argz_delete`, `argz_insert`, `argz_next`, `argz_replace` (and `__argz_*` aliases)

**Contract.** Manage a *list of Text* stored as a single `malloc`ed buffer of concatenated
NUL-terminated strings, together with its total length. You can:

* create one from an `argv` array or a separator-delimited Text;
* count its entries;
* convert it back to an `argv`-style pointer array;
* join its entries with a separator;
* append, insert or delete entries;
* iterate over it;
* replace substrings in every entry.

**Failure.** Functions return `error_t` (`ENOMEM`), or 0 on success.

**Kernel.** None.

⚑ HIST. This is an ad hoc, length-delimited container representation that the rest of the
library does not use.

### 4.14 Envz vectors: `envz_entry`, `envz_get`, `envz_add`, `envz_merge`, `envz_remove`, `envz_strip`

**Contract.** Treat an argz vector as a `NAME=VALUE` mapping: look up, add, merge (with or
without overriding existing names), remove, and strip null entries. An entry without `=`
has a *null* value, which is distinct from an empty value.

**Kernel.** None.

---

## 5. Character classification and case mapping

### 5.1 Byte classification: `isalnum`, `isalpha`, `isblank`, `iscntrl`, `isdigit`, `isgraph`, `islower`, `isprint`, `ispunct`, `isspace`, `isupper`, `isxdigit`, `isascii`, `toascii`, `_l` variants of each, `__ctype_b_loc`, `__ctype_tolower_loc`, `__ctype_toupper_loc`, `__ctype_b`, `__ctype_tolower`, `__ctype_toupper`, `__ctype32_b`, `__ctype32_tolower`, `__ctype32_toupper`, `__ctype_get_mb_cur_max`, `__isalnum_l`... (the `__is*_l` aliases)

**Contract.** Report whether a single-byte character belongs to a class in the current
(`LC_CTYPE`) or given locale.

**Inputs.** An `int` that must be representable as `unsigned char`, or equal `EOF` (-1).
⚑ HIST, ⚑ ARB: passing a plain `char` holding a negative value (any byte ≥ 0x80 on x86,
where `char` is signed) is undefined behaviour. The domain includes `EOF` only so that the
result of `getc` can be passed unchecked. The functional need is "classify this byte".

`isctype(c, mask)`/`__isctype` (GNU) test an arbitrary combination of class bits (`_IS*`).

**Guarantees.** A non-zero result means membership. Class relations hold, for example
`isalnum ⇔ isalpha ∨ isdigit`. `isdigit` and `isxdigit` are locale-independent in practice
(always `0-9`, `a-f`, `A-F`). `isascii(c)` tests `0 ≤ c ≤ 127`, and `toascii` masks to 7 bits
(⚑ HIST, obsolete).

**Kernel.** None. Locale tables are loaded by `setlocale` (§7).

⚑ HIST. Single-byte classification is meaningless for multibyte encodings such as UTF-8,
where the class of byte 0xC3 is undefined. The functional need is classification of a
*character*, which §5.2 provides.
⚑ COMPAT. `__ctype_b_loc` and the other `*_loc` functions expose the per-thread table
pointers that the classification macros index directly. That makes the table layout (a
16-bit mask array indexed from -128 to 255) part of the ABI.

### 5.2 Wide-character classification: `iswalnum`, `iswalpha`, `iswblank`, `iswcntrl`, `iswdigit`, `iswgraph`, `iswlower`, `iswprint`, `iswpunct`, `iswspace`, `iswupper`, `iswxdigit`, `iswctype`, `wctype`, `_l` variants, `__iswctype`, `__wctype_l`

**Contract.** Classify a wide character (a Unicode code point on glibc) according to the
locale's character classes. `wctype(name)` looks up a class by name (standard or
locale-defined, for example `"jspace"`) and returns an opaque `wctype_t`. `iswctype(wc, t)`
tests membership. `WEOF` is a member of no class.

**Guarantees.** The classes follow the locale's definition, which for glibc's UTF-8 locales
is derived from the Unicode Character Database (Unicode 15.1.0 in 2.39).

**Failure.** `wctype` returns 0 for an unknown class.

**Kernel.** None.

### 5.3 Case mapping: `tolower`, `toupper`, `_tolower`, `_toupper`, `tolower_l`, `toupper_l`, `towlower`, `towupper`, `towctrans`, `wctrans`, `towlower_l`, `towupper_l`, `towctrans_l`, `wctrans_l`, `__towlower_l`, `__towupper_l`, `__towctrans`, `__towctrans_l`, `__wctrans_l`

**Contract.** Map one character to its lower- or uppercase counterpart in the locale. Characters
without a counterpart map to themselves. `wctrans(name)` looks up a named mapping
(`"tolower"`, `"toupper"`, or locale-defined, for example `"tojhira"`). `towctrans` applies it.

⚑ HIST. One-to-one mapping cannot express Unicode full case mapping, which is one-to-many
(`ß` → `SS`) and context-dependent (final sigma). `_tolower` and `_toupper` are SVID forms
whose behaviour is undefined for input that is not uppercase (respectively lowercase).

---

## 6. Multibyte text, wide characters and character-set conversion

**Section-wide notes.**

* **Kernel: none** for all conversion functions, except `iconv_open`, which loads conversion
  modules. That needs `openat`, `mmap` and `dlopen` of `gconv` modules, and reads
  **Config** `gconv-modules` / `gconv-modules.cache` under `GCONV_PATH` or the built-in
  directory.
* The *multibyte encoding* of the locale (`LC_CTYPE` codeset, usually UTF-8) is what these
  functions convert from and to. On glibc the *wide* representation is always UCS-4
  (`__STDC_ISO_10646__` is defined), so `wchar_t` holds exactly one Unicode code point.

### 6.1 Conversion state: `mbstate_t`, `mbsinit`

**Contract.** An opaque object holding the shift state and partial-character state of an
ongoing conversion. A zero-initialized object is the initial state. `mbsinit(ps)` reports
whether `ps` is in the initial state (true for `NULL`).

⚑ HIST. For stateless encodings such as UTF-8, the state exists only to carry a partial
character across calls. Its size is fixed in the ABI (8 bytes on glibc), which caps what a
future stateful encoding could carry.

### 6.2 Single-character conversion: `mblen`, `mbtowc`, `wctomb`, `mbrlen`, `__mbrlen`, `mbrtowc`, `__mbrtowc`, `wcrtomb`, `btowc`, `wctob`, `mbrtoc8`, `c8rtomb`, `mbrtoc16`, `c16rtomb`, `mbrtoc32`, `c32rtomb`

**Contract.**

* **`mbrtowc(&wc, s, n, ps)`** decodes one character from at most `n` bytes. It returns:
  * the number of bytes consumed;
  * 0 for the null character;
  * `(size_t)-2` for an incomplete but valid prefix (all `n` bytes consumed into `ps`);
  * `(size_t)-1` with `EILSEQ` for an invalid sequence.
* **`wcrtomb(s, wc, ps)`** encodes one character into at most `MB_CUR_MAX` bytes and returns
  the byte count, or `(size_t)-1`/`EILSEQ`.
* **`mbrlen`** is `mbrtowc` without storing the result.
* **The non-`r` forms (`mblen`, `mbtowc`, `wctomb`)** keep state in hidden static storage.
  ⚑ HIST, ⚑ PATH: that is why the `r` (restartable) forms exist.
* **`btowc(c)`/`wctob(wc)`** convert between a single byte and a wide character when the
  character is a single byte in the locale encoding. Otherwise they return `WEOF`/`EOF`.
* **`mbrtoc16`/`c16rtomb`** convert to and from UTF-16 code units. A character outside the
  BMP is delivered in two calls:
  * `mbrtoc16` returns `(size_t)-3` for the second (low) surrogate;
  * `c16rtomb` buffers a high surrogate in the state.
* **`mbrtoc32`/`c32rtomb`** convert to and from UTF-32 code points. **`mbrtoc8`/`c8rtomb`**
  (2.36, C23) convert to and from UTF-8 code units, one unit per call.

**Inputs.** `MB_CUR_MAX` (a runtime value, `__ctype_get_mb_cur_max()`) bounds the bytes
per character in the current locale. It is 6 for glibc UTF-8 (a historical maximum),
although valid UTF-8 needs at most 4. `MB_LEN_MAX` is the compile-time bound (16).

**Guarantees.** Output is written only for complete characters.

**Failure / edge cases.**

* Invalid or overlong UTF-8, surrogate code points, and values above `0x10FFFF` are
  rejected with `EILSEQ`.
* `s == NULL` resets the state; with an incomplete character pending it is an invalid sequence:
  `(size_t)-1` with `EILSEQ`.
* ⚑ HIST: `(size_t)-1`, `-2` and `-3` are in-band sentinel values in an unsigned return
  type.

### 6.3 String conversion: `mbstowcs`, `wcstombs`, `mbsrtowcs`, `wcsrtombs`, `mbsnrtowcs`, `wcsnrtombs`

**Contract.** Convert a whole terminated string. The `nr` forms limit the *input* length as
well as the output. They write at most `len` output units and return the number of units
written (excluding the terminator), or `(size_t)-1`/`EILSEQ`. If `dst` is `NULL`, they only
measure the output. The `r` forms advance `*src` to the point reached, or set it to `NULL`
if the terminator was converted.

⚑ HIST. "Call with NULL to measure, then call again" is the standard pattern whenever the
caller must allocate. A design that returns an owned result (or takes an allocator) makes the
double pass unnecessary.

### 6.4 General character-set conversion: `iconv_open`, `iconv`, `iconv_close`

**Contract.**

* **`iconv_open(to, from)`** creates a conversion descriptor between two named encodings. The
  names are case-insensitive, and aliases are accepted. Suffixes on `to` modify how
  unconvertible characters are handled:
  * `//TRANSLIT` approximates them;
  * `//IGNORE` drops them;
  * a combined form (2.32+ `//TRANSLIT//IGNORE`) does both.
* **`iconv(cd, &in, &inleft, &out, &outleft)`** converts as much input as possible, advancing
  the pointers and decrementing the counts. It returns the number of *irreversible*
  conversions performed.
* **`iconv(cd, NULL, NULL, &out, &outleft)`** emits the shift sequence that returns the
  output to the initial state.
* **`iconv(cd, NULL, NULL, NULL, NULL)`** resets the state.
* **`iconv_close`** frees the descriptor.

**Failure.**

* `iconv_open` returns `(iconv_t)-1` with `EINVAL` if the conversion is unsupported.
* `iconv` returns `(size_t)-1` with:
  * `E2BIG`: output space exhausted;
  * `EILSEQ`: invalid input, with `*in` left at the invalid sequence;
  * `EINVAL`: incomplete input at the end.
* With `//IGNORE`, `iconv` skips invalid input but still returns -1/`EILSEQ` at the end.
  ⚑ HIST: this makes "was anything lost?" hard to distinguish from "did it fail?".

**Guarantees.** The conversion is resumable across calls with no loss. glibc supplies
about 1,300 names or aliases for about 200 encodings (the `iconvdata` modules).

**Kernel.** `openat`, `read`, `mmap` (module cache), `dlopen` (modules). **Config:**
`GCONV_PATH` (ignored in secure mode).

⚑ HIST. A statically linked program cannot use `iconv` (or NSS, or locales) without the
shared modules. This makes "static" binaries silently depend on the installed glibc.

### 6.5 Display width: `wcwidth`, `wcswidth`

**Contract.** Return the number of terminal columns a wide character, or the first `n`
characters of a wide string, occupies:

* 0 for combining and zero-width characters, and for the null character;
* 2 for East Asian wide and fullwidth characters and most emoji;
* 1 otherwise;
* -1 if any character is non-printable.

**Kernel.** None.

⚑ HIST. The column width is a property of the terminal and its font, not of the character.
The locale's width table (derived from Unicode `EastAsianWidth`) is a best guess. Ambiguous-
width characters have no correct answer.

### 6.6 Wide-character string functions

The `wcs*`/`wmem*` equivalents of §4 are specified there. §12 covers the wide-oriented
stream functions, and §8.3 the wide numeric conversions.

---

## 7. Locales and message translation

### 7.1 Locale model

**Contract.** A *locale* is a named set of cultural conventions, divided into categories:

| Category | Governs |
|---|---|
| `LC_CTYPE` | character classes, case mapping, multibyte encoding |
| `LC_COLLATE` | string ordering |
| `LC_NUMERIC` | decimal point, thousands grouping (non-monetary) |
| `LC_MONETARY` | currency formatting |
| `LC_TIME` | day and month names, date formats |
| `LC_MESSAGES` | yes/no expressions, message catalog selection |
| `LC_PAPER`, `LC_NAME`, `LC_ADDRESS`, `LC_TELEPHONE`, `LC_MEASUREMENT`, `LC_IDENTIFICATION` | GNU extensions |

Locale-dependent functions consult the thread's current locale. That is the per-thread locale
set with `uselocale`, or else the process-global locale set with `setlocale`. `"C"` and
`"POSIX"` are built in. `"C.UTF-8"` has been built in since 2.35.

**Kernel.** None for the built-in locales. For other locales:

* `openat`, `fstat`, `mmap` of the compiled locale archive or of the per-category files;
* **Config:** `/usr/lib/locale/locale-archive`, `LOCPATH` (ignored in secure mode), and the
  `LANG`/`LC_ALL`/`LC_*` environment variables.

⚑ HIST. The locale was originally a single process-global setting (C89). Per-thread and
explicit locale objects were added later (POSIX.1-2008), which produced the `_l` twin of
every locale-sensitive function (C0.2). The functional need is "format or parse according to
conventions X", which is naturally an explicit parameter.
⚑ HIST. `LC_NUMERIC` affecting `printf("%f")` and `strtod` makes data file formats silently
depend on the user's locale. It is a well-known source of bugs.

### 7.2 `setlocale`

**Contract.** Query or set the global locale for one category or for all categories (`LC_ALL`).

* `setlocale(cat, NULL)` returns the current name.
* `setlocale(cat, "")` selects the locale from the environment: `LC_ALL`, then `LC_<cat>`,
  then `LANG`.
* The name returned for `LC_ALL` can be passed back to `setlocale` to restore a composite
  locale.
* A category whose current name is the name asked for keeps its data: it is not looked up
  again, so a changed `LOCPATH` or changed locale files take effect for it only after it has
  been set to another name (observed).

**Failure.** Returns `NULL` and leaves the locale unchanged if the name is unknown or the data
is unavailable.

**Threads.** MT-Unsafe (`const:locale env`). It changes the locale of all threads that have
not called `uselocale`. ⚑ HIST.

**Guarantees.** The returned string is library-owned, and valid until the next `setlocale`.

### 7.3 Locale objects: `newlocale`, `duplocale`, `freelocale`, `uselocale`, `__newlocale`, `__duplocale`, `__freelocale`, `__uselocale`

**Contract.**

* **`newlocale(mask, name, base)`** creates a locale object with the categories in `mask`
  taken from `name` and the rest from `base`. `base` is consumed: it may be reused or freed.
* **`duplocale`** copies an object.
* **`freelocale`** releases an object.
* **`uselocale(loc)`** sets the calling thread's locale and returns the previous one.
  `LC_GLOBAL_LOCALE` reverts to the global locale, and `(locale_t)0` only queries.

**Failure.** `newlocale` returns `(locale_t)0` with `EINVAL` (bad mask) or `ENOENT` (unknown
name). ⚑ ARB: freeing a locale that is still in use by a thread is undefined.

### 7.4 Locale information: `localeconv`, `nl_langinfo`, `nl_langinfo_l`, `__nl_langinfo_l`

**Contract.**

* **`localeconv()`** returns a `struct lconv` describing the numeric and monetary conventions:
  `decimal_point`, `thousands_sep`, `grouping`, `int_curr_symbol`, `currency_symbol`,
  `mon_*`, `positive_sign`, `negative_sign`, `*_frac_digits`, `p_cs_precedes`, and the
  others.
* **`nl_langinfo(item)`** returns one Text item. Items include `CODESET`, `D_T_FMT`,
  `D_FMT`, `T_FMT`, `DAY_1..7`, `ABDAY_*`, `MON_1..12`, `ABMON_*`, `ALTMON_*` (2.27+),
  `RADIXCHAR`, `THOUSEP`, `YESEXPR`, `NOEXPR`, `CRNCYSTR`, `ERA`, `ALT_DIGITS`, and GNU
  items for every category (`_NL_PAPER_HEIGHT`, `_NL_ADDRESS_POSTAL_FMT`, ...).

**Guarantees.** Pointers are library-owned. They stay valid until the locale changes.

⚑ HIST. `grouping` is a Text whose bytes are group sizes, where `CHAR_MAX` means "no further
grouping". Non-textual data is encoded in a string type.

### 7.5 Monetary formatting: `strfmon`, `strfmon_l`, `__strfmon_l`

**Contract.** Format one or more `double` or `long double` amounts according to a format
string (`%n` national, `%i` international), using `LC_MONETARY`. Flags control fill
character, grouping, sign style (`+`, `(`), and the suppression of the currency symbol. It
writes at most `max` bytes.

**Failure.** Returns -1 with `E2BIG` if the result does not fit.

⚑ HIST. `double` is unsuitable for currency. The functional need is formatting a decimal
amount.

### 7.6 Yes/no responses: `rpmatch`

**Contract.** Classify a response Text against the locale's `YESEXPR`/`NOEXPR` regular
expressions. Returns 1 (yes), 0 (no) or -1 (unrecognized).

**Kernel.** None.

### 7.7 Message catalogs, GNU gettext: `gettext`, `dgettext`, `dcgettext`, `ngettext`, `dngettext`, `dcngettext`, `textdomain`, `bindtextdomain`, `bind_textdomain_codeset`, `__dcgettext`, `__dgettext`, `_nl_default_dirname`, `_nl_domain_bindings`, `_nl_msg_cat_cntr`, `_libc_intl_domainname`

**Contract.** Translate a message, identified by its original (msgid) text, into the language
of `LC_MESSAGES` (or `LANGUAGE`, a colon-separated preference list).

* **`gettext(id)`** returns the translation, or `id` itself if there is none.
* **`ngettext(id, id_plural, n)`** selects a plural form using the catalog's plural
  expression.
* **Domains** name separate catalogs. `textdomain` sets the default domain.
  `bindtextdomain(domain, dir)` sets where catalogs are found (`dir/<lang>/<CATEGORY>/
  <domain>.mo`; `<CATEGORY>` is `LC_MESSAGES` except for `dcgettext`/`dcngettext` with another
  category; corrected 2026-10-04 under the three-way rule: the reference and bindtextdomain(3)
  agree, `spec/behaviour/catalogs.md`). `bind_textdomain_codeset` sets the output encoding.

* `LANGUAGE` is ignored when the `LC_MESSAGES` locale is `C`, `POSIX` or `C.UTF-8`, and a
  `C` or `POSIX` element in the list ends the search untranslated.
* `bindtextdomain` and `bind_textdomain_codeset` with a NULL or empty domain return NULL and
  leave `errno` unchanged (Written 2026-10-04 (owner ruling, phase 12-13 report), `spec/behaviour/catalogs.md`).

**Guarantees.** It never fails visibly: failure returns the untranslated text.
Returned translations are read-only and library-owned. `errno` is preserved.

**Kernel.** `openat`, `fstat`, `mmap` (`.mo` catalog files). **Config:** catalog
directories, `LANGUAGE`, `LC_ALL`, `LC_MESSAGES`, `LANG`, and `iconv` modules (for charset
conversion).

⚑ HIST. The source-language text is used as the lookup key. This couples translations to
exact English strings, but it is also why untranslated programs still work. The contract
is sound. `_nl_msg_cat_cntr` is an exported change counter that caching code in
`gettext.h` relies on (⚑ COMPAT).

### 7.8 X/Open message catalogs: `catopen`, `catgets`, `catclose`

**Contract.** Open a catalog by name (searching `NLSPATH` templates with `%N`, `%L`, `%l`,
`%t`, `%c` substitutions) and retrieve messages by numeric (set, message) identifiers,
returning a default Text if one is missing.

In an `NLSPATH` template, `%t` and `%c` keep the locale name's `@modifier`. The search stops
(`EINVAL`) at a candidate that is a directory or not a catalog, rather than trying the remaining
templates (Written 2026-10-04 (owner ruling, phase 12-13 report), `spec/behaviour/catalogs.md`).

**Failure.** `catopen` returns `(nl_catd)-1`; an empty name gives `EINVAL`. `catgets` returns
the default and sets `errno`.

**Kernel.** `openat`, `mmap`/`read`.

⚑ HIST. Numeric message identifiers require keeping catalogs and source in step by hand.
gettext superseded this interface.

---

## 8. Numeric conversion and integer arithmetic

**Section-wide notes.**

* **Kernel: none** throughout this section.
* Locale sensitivity (the radix character, thousands grouping with the GNU `'` flag, and
  locale digits with `I`) comes from `LC_NUMERIC`.

### 8.1 Text to integer: `strtol`, `strtoul`, `strtoll`, `strtoull`, `strtoq`, `strtouq`, `strtoimax`, `strtoumax`, `__strtol_internal` (and siblings, compat), `_l` variants, `__isoc23_strtol`, `__isoc23_strtoul`, `__isoc23_strtoll`, `__isoc23_strtoull`, `__isoc23_strtoimax`, `__isoc23_strtoumax` (and `_l`), `atoi`, `atol`, `atoll`

**Contract.** Parse the longest initial substring of a Text that has the form of an integer in
base `b`, and report where parsing stopped.

**Inputs.**

* **The text.** Leading white space (`isspace`) is skipped, and an optional sign is accepted.
* **The base** is 2..36, or 0 for automatic detection:
  * `0x`/`0X` → 16;
  * a leading `0` → 8;
  * `0b`/`0B` → 2 in C23 mode, via the `__isoc23_*` symbols (2.38+);
  * otherwise 10.
* **An optional end pointer** receiving the position after the last consumed character.

**Guarantees.**

* On success it returns the value and sets the end pointer.
* If no digits are consumed, it returns 0 and the end pointer equals the input.
* On overflow it returns `LONG_MAX`/`LONG_MIN`/`ULONG_MAX` (as appropriate) and sets
  `ERANGE`.
* For the unsigned forms a leading `-` is accepted, and the value is negated modulo 2^N.
  ⚑ HIST, ⚑ ARB: `strtoul("-1")` returns `ULONG_MAX` with no error, which is surprising
  and not functionally needed.

**Failure.** An invalid base gives `EINVAL`. ⚑ HIST: "no digits" is reported only through the
end pointer, and range errors only through `errno`. The caller must zero `errno` and compare
pointers. The need is a result that distinguishes {value, no number, out of range}.

**`atoi`, `atol`, `atoll`** are `strtol(s, NULL, 10)` with undefined behaviour on overflow and
no error reporting. ⚑ HIST.

⚑ PATH. The `__isoc23_*` entry points exist because C23 changed the meaning of base 0 (adding
`0b`) and the language mode selects which symbol a call binds to (C0.6).

### 8.2 Text to floating point: `strtod`, `strtof`, `strtold`, `strtof32`, `strtof64`, `strtof128`, `strtof32x`, `strtof64x`, `_l` variants, `__strtod_internal` (and siblings, compat), `atof`

**Contract.** Parse the longest initial substring that forms a floating constant, and return
the correctly rounded value in the current rounding mode. Accepted forms:

* decimal, with an optional exponent (`e`);
* hexadecimal (`0x...p±n`);
* `INF`/`INFINITY`;
* `NAN` and `NAN(n-char-sequence)`, where the payload sets the NaN's mantissa bits.

The radix character comes from the locale.

**Guarantees.** The result is correctly rounded (glibc uses exact arbitrary-precision
arithmetic), for any length of input.

**Failure.**

* Overflow returns `±HUGE_VAL` and sets `ERANGE`.
* Underflow returns the correctly rounded subnormal or zero and sets `ERANGE`.
* No conversion returns 0 with the end pointer equal to the input.

⚑ HIST. The locale-dependent radix makes `strtod` unusable for data interchange without
`_l` and a `"C"` locale object. The need, "parse a number in format F", should not depend
on ambient state.

### 8.3 Wide-character numeric conversion: `wcstol`, `wcstoul`, `wcstoll`, `wcstoull`, `wcstoq`, `wcstouq`, `wcstoimax`, `wcstoumax`, `wcstod`, `wcstof`, `wcstold`, `wcstof32/64/128/32x/64x`, `_l` variants, `__isoc23_wcsto*`, `__wcsto*_internal` (compat)

**Contract.** The same as §8.1 and §8.2, over wide text.

### 8.4 Floating point to text: `ecvt`, `fcvt`, `gcvt`, `qecvt`, `qfcvt`, `qgcvt`, `ecvt_r`, `fcvt_r`, `qecvt_r`, `qfcvt_r`, `strfromd`, `strfromf`, `strfroml`, `strfromf32/64/128/32x/64x`

**Contract.**

* **`strfromd(buf, n, fmt, x)`** (C23/TS 18661) formats `x` using a restricted `printf`
  conversion: `%[.prec]{a,A,e,E,f,F,g,G}`, with no flags, no width, and no locale grouping.
  It writes at most `n` bytes and returns the length the full result would have.
* **`ecvt(x, ndigit, &decpt, &sign)`** returns `ndigit` significant digits, `ndigit` reduced to a
  limit the precision gives (17 for `double`, 21 for the `long double` `q` forms). **`fcvt`**
  returns `ndigit` digits after the point, `ndigit` reduced the same way. Both return the digits only, with the decimal-point
  position and the sign reported separately.
* **`gcvt(x, ndigit, buf)`** formats like `%.*g`.
* **The `q` forms** take `long double`.
* **The `_r` forms** take a caller buffer.

**Guarantees.** Digit strings are exact (correctly rounded) representations.

⚑ HIST, ⚑ PATH. `ecvt` and `fcvt` return a static buffer, hence the `_r` forms. They were
removed from POSIX in 2008. The need, the shortest or a fixed-precision decimal
representation, is fully met by `strfrom*` or `snprintf`.

### 8.5 Integer to text

There is no dedicated integer-to-text function in glibc other than the `printf` family (§12.6)
and the radix-64 encoding of §8.10. ⚑ HIST: the absence is why programs use
`snprintf("%d")`, which drags in locale and formatting machinery for a pure conversion.

### 8.6 Integer arithmetic helpers: `abs`, `labs`, `llabs`, `imaxabs`, `div`, `ldiv`, `lldiv`, `imaxdiv`

**Contract.**

* **`abs(n)`** returns |n|. `abs(INT_MIN)` is undefined behaviour. ⚑ ARB: the result type
  cannot represent it, and C2y adds `uabs` for this.
* **`div(n, d)`** returns the quotient (truncated toward zero) and the remainder together.
  Division by zero, and `INT_MIN / -1`, are undefined.

### 8.7 Bit and byte utilities, C23 `<stdbit.h>` (2.39): `stdc_leading_zeros_{uc,us,ui,ul,ull}`, `stdc_leading_ones_*`, `stdc_trailing_zeros_*`, `stdc_trailing_ones_*`, `stdc_first_leading_zero_*`, `stdc_first_leading_one_*`, `stdc_first_trailing_zero_*`, `stdc_first_trailing_one_*`, `stdc_count_zeros_*`, `stdc_count_ones_*`, `stdc_has_single_bit_*`, `stdc_bit_width_*`, `stdc_bit_floor_*`, `stdc_bit_ceil_*` (70 functions)

**Contract.** Pure bit queries on unsigned integers of each standard width. The contracts
(for a value of width W bits):

* **Counting:** number of leading or trailing zero or one bits. The result is W for 0 when
  counting zeros.
* **Finding:** 1-based index of the first matching bit from the given end, or 0 if none.
* **`count_zeros`/`count_ones`:** population counts.
* **`has_single_bit`:** true if the value is a power of two.
* **`bit_width`:** 1 + floor(log2 x), or 0 for x = 0.
* **`bit_floor`:** the largest power of two ≤ x (0 for 0).
* **`bit_ceil`:** the smallest power of two ≥ x. The result is 0 if it is not
  representable. ⚑ ARB, as specified by C23.

⚑ HIST. There are five function copies per operation, one per C integer width, because C
functions are not generic. The header provides type-generic macros too.

### 8.8 Byte order: `htonl`, `htons`, `ntohl`, `ntohs`, `htobe16/32/64`, `htole16/32/64`, `be16toh`..., `le64toh` (macros), `bswap_16/32/64` (macros)

**Contract.** Convert between host byte order and big-endian (network) or little-endian order.
On x86_64, `hton*` and `ntoh*` are byte swaps. Only `htonl`, `htons`, `ntohl` and `ntohs`
are exported functions. The rest are macros.

⚑ HIST. The "network" naming reflects the 1983 socket API. The need is fixed-endianness
serialization.

### 8.9 Find first set: `ffs`, `ffsl`, `ffsll`

**Contract.** Return the 1-based index of the least-significant set bit, or 0 if none.

⚑ HIST. This duplicates `stdc_first_trailing_one`, and takes a *signed* integer.

### 8.10 Radix-64 conversion: `l64a`, `a64l`

**Contract.** Convert between a `long` (only the low 32 bits are used) and a string of up to 6
characters from `./0-9A-Za-z`, least-significant digit first.

⚑ HIST. The encoding is from crypt salts. `l64a` returns a static buffer.

### 8.11 Overflow-checked arithmetic

C23 `<stdckdint.h>` (`ckd_add`, `ckd_sub`, `ckd_mul`) is provided by the compiler, not by glibc.
glibc's own overflow checks (`reallocarray`, `calloc`) are internal.

---

## 9. Random numbers

### 9.1 Cryptographic-quality randomness: `getrandom`, `getentropy`, `arc4random`, `arc4random_buf`, `arc4random_uniform`

**Contract.**

* **`getrandom(buf, n, flags)`** fills `buf` with up to `n` bytes from the kernel CSPRNG and
  returns the count, which may be short for large `n` or when interrupted.
  * `GRND_NONBLOCK` fails with `EAGAIN` instead of blocking before the pool is initialized.
  * `GRND_RANDOM` draws from the blocking pool (legacy).
  * `GRND_INSECURE` (kernel 5.6+) never blocks.
* **`getentropy(buf, n)`** fills exactly `n ≤ 256` bytes, or fails with `EIO`. For
  `n > 256` it fails with `EIO` (POSIX 2024 requires `EINVAL`). ⚑ ARB: the 256-byte limit
  comes from OpenBSD and is not needed.
* **`arc4random()`** returns a uniformly distributed 32-bit value.
* **`arc4random_buf(buf, n)`** fills `n` bytes.
* **`arc4random_uniform(bound)`** returns a uniform value in `[0, bound)` without modulo bias
  (0 for bound 0 or 1).

**Guarantees.**

* The `arc4random` family never fails and never returns non-random data. If the kernel
  cannot supply entropy, the process terminates ("cannot get entropy for arc4random").
* In glibc 2.39 each call takes its bytes from `getrandom` (there is no user-space
  buffering), so output is fork-safe.
* `getrandom` is a cancellation point. `arc4random` is not.

**Kernel.** `getrandom` (syscall). When `getrandom` returns `ENOSYS` (pre-3.17 kernels),
`arc4random` falls back to `/dev/urandom` (`openat`, `read`) and uses `poll` on
`/dev/random` to wait for initialization.

⚑ HIST. The name "arc4" refers to the RC4 cipher, which is no longer used (OpenBSD compat).

### 9.2 ISO C generator: `rand`, `srand`, `rand_r`, `RAND_MAX`

**Contract.** `rand()` returns a pseudo-random integer in `[0, RAND_MAX]` (`RAND_MAX` =
2^31-1). The sequence is deterministic for a given `srand(seed)`, and the default seed is 1.
`rand_r(&seed)` keeps its state in a caller `unsigned int`.

**Threads.** `rand` is MT-Safe in glibc (it takes a lock), although ISO C does not require
this.

⚑ HIST. The global hidden state makes sequences non-reproducible in the presence of any other
user. `rand_r`'s 32-bit state is too small for good quality (it was removed from POSIX in
2024).

### 9.3 BSD generator: `random`, `srandom`, `initstate`, `setstate`, `random_r`, `srandom_r`, `initstate_r`, `setstate_r`

**Contract.** An additive-feedback generator with caller-selectable state size (8, 32, 64,
128 or 256 bytes). It returns values in `[0, 2^31-1]`. `initstate` supplies a state buffer
and returns the previous one. `setstate` switches buffers. The `_r` forms use a caller
`struct random_data`.

**Guarantees.** For a given seed and state size, the sequence is deterministic and identical
across glibc versions. Programs depend on this, so the algorithm is effectively a contract.

**Failure.** `initstate` with fewer than 8 bytes returns `NULL`/`EINVAL`.

### 9.4 SVID 48-bit generators: `drand48`, `erand48`, `lrand48`, `nrand48`, `mrand48`, `jrand48`, `srand48`, `seed48`, `lcong48`, and the `_r` forms (`drand48_r`, ..., `lcong48_r`)

**Contract.** A 48-bit linear congruential generator

X(n+1) = (a·X(n) + c) mod 2^48, with a = 0x5DEECE66D and c = 0xB.

The algorithm is **part of the contract**: POSIX specifies it exactly, and programs rely on
the exact sequence.

* `drand48` returns `[0.0, 1.0)`.
* `lrand48` returns `[0, 2^31)`.
* `mrand48` returns `[-2^31, 2^31)`.
* The `e/n/j` forms take a caller-supplied 3×16-bit state.
* `seed48` and `lcong48` set the state and the parameters. `seed48` returns a pointer to a
  static copy of the previous state.

**Kernel.** None.

⚑ HIST. The generator is statistically weak by modern standards and is kept only for
reproducibility.

---

## 10. Mathematics (`libm`)

### 10.0 Conventions for the whole section

**Kernel: none.** Every function in `libm` is a pure user-space computation on the
floating-point unit. The floating-point environment (rounding mode, exception flags, trap
enables) is per-thread CPU register state (MXCSR and the x87 control/status words), saved and
restored by the kernel across context switches and signals. Nothing in `libm` makes a system
call. `libmvec` (the SIMD vector variants `_ZGV*`) likewise needs no kernel support beyond the
CPU features reported in `AT_HWCAP`/CPUID.

**Type variants.** Each real function `f` exists for every supported floating type, and the
name suffix selects the type:

| Suffix | Type | x86_64 format |
|---|---|---|
| (none) | `double` | IEEE binary64 |
| `f` | `float` | binary32 |
| `l` | `long double` | x87 80-bit extended |
| `f32` | `_Float32` | binary32 (alias of `f`) |
| `f64` | `_Float64` | binary64 (alias of the unsuffixed form) |
| `f32x` | `_Float32x` | binary64 |
| `f64x` | `_Float64x` | x87 extended (alias of `l`) |
| `f128` | `_Float128` | IEEE binary128 (software) |

The contract is identical across types, apart from the range and precision of the type. This
specification documents each function once. ⚑ HIST: C's lack of generic functions produces
seven entry points per operation (≈1,000 of the ≈1,150 `libm` symbols). `<tgmath.h>`
supplies type-generic macros on top.

**Error reporting.** `math_errhandling` is `MATH_ERRNO | MATH_ERREXCEPT` on glibc. A domain
error sets `EDOM` and raises `FE_INVALID`, and a pole or range error sets `ERANGE` and raises
`FE_DIVBYZERO`/`FE_OVERFLOW`/`FE_UNDERFLOW`. A range error sets `ERANGE` when the result
overflows to ±∞ or underflows to ±0; a result rounded to the largest finite value (directed
rounding) or to a subnormal raises the flags only. Some functions report through the flags only
and never set `errno`: `lrint`, `llrint`, `lround`, `llround` (out of range), `fma` and the
narrowing `*fma` forms, `remquo`, `logb` (its pole at 0), `nextafter`/`nexttoward` from a zero
operand, and the underflow of `erf`. ⚑ HIST: setting `errno` from a pure function
defeats vectorization and constant folding, which is why compilers offer `-fno-math-errno`.
IEEE exception flags are the functionally sufficient channel. The historical SVID/XPG
`matherr` callback and `_LIB_VERSION` selection are compat-only (removed from the API in 2.27).

**Accuracy.** glibc documents measured maximum ULP errors per function, per type, per
architecture (manual "Errors in Math Functions"). It guarantees correct rounding only for
the IEEE-required operations: `sqrt`, `fma`, the narrowing arithmetic of §10.4, conversions,
`fmod`, `remainder`, and the rounding and classification functions. Other functions are
"faithful or better, with documented bounds". A reimplementation must at least match those
bounds, because programs (and the glibc test suite's `libm-test-ulps`) depend on them.

**Special values.** Behaviour on NaN, ±∞ and ±0 follows C Annex F and IEEE 754-2008/2019.
Quiet NaN inputs propagate without raising `FE_INVALID`. Signalling NaN inputs raise it.

**Finite-only entry points.** `__<name>_finite` (for example `__exp_finite`) were emitted by
`-ffinite-math-only` builds before 2.31. They remain as compat symbols with the same
contract as `<name>`, restricted to finite arguments.

### 10.1 Classification and comparison: `fpclassify` (macro), `__fpclassify`, `isnan`, `isinf`, `isfinite`/`finite`, `isnormal`, `signbit`/`__signbit`, `issignaling`/`__issignaling`, `iscanonical`/`__iscanonical`, `issubnormal`, `iszero`, `isgreater`..., `isunordered`, `iseqsig`/`__iseqsig`, `totalorder`, `totalordermag`, `__isinf`, `__isnan`, `__finite`, `isinff`, `isnanf`, `isinfl`, `isnanl` (and `__` forms)

**Contract.**

* Classify a value as NaN, infinite, zero, subnormal or normal. Also report its sign bit
  (including for ±0 and NaN), and whether a NaN is signalling or a value is canonical.
* The comparison macros compare without raising `FE_INVALID` for quiet NaN operands.
* `iseqsig` compares for equality and raises `FE_INVALID` for any NaN operand (sets `EDOM`).
* `totalorder(&x, &y)` implements the IEEE 754 total ordering, including NaN payloads and
  signs. It takes pointers (C23 changed this from values) so that signalling NaNs are not
  quieted. `totalordermag` does the same on magnitudes.

⚑ HIST. `isinf` returns ±1 in glibc (the sign) rather than a plain boolean. That is a GNU
extension, and some programs rely on it. `finite` is the BSD name for `isfinite`.

### 10.2 Exact and rounding operations

| Function(s) | Contract |
|---|---|
| `ceil`, `floor`, `trunc`, `round`, `roundeven` | Round to an integral value in the same format: up, down, toward zero, half away from zero, half to even. Exact; never raise `FE_INEXACT` (C23). |
| `rint`, `nearbyint` | Round in the current rounding mode. `rint` may raise `FE_INEXACT`, `nearbyint` does not. |
| `lrint`, `llrint`, `lround`, `llround` | Round and convert to `long`/`long long`. Out of range raises `FE_INVALID` and gives an unspecified value. |
| `fromfp`, `ufromfp`, `fromfpx`, `ufromfpx` (TS 18661-1) | Round in a *specified* rounding direction to a signed or unsigned integer of a *specified bit width*, returned as `intmax_t`/`uintmax_t` (the 2.39 interface; C23's floating-format return arrived later). The `x` forms raise `FE_INEXACT` for a non-integral argument. Out of range (also NaN, ∞, width 0): a domain error (`EDOM`, `FE_INVALID`) and an unspecified value. |
| `modf` | Split into integral and fractional parts, both with the argument's sign. |
| `frexp`, `ldexp`, `scalbn`, `scalbln` | Decompose into a mantissa in [0.5, 1) and an exponent, or multiply by 2^n exactly (with overflow and underflow as specified). |
| `ilogb`, `llogb`, `logb` | Unbiased exponent. `ilogb(0)` = `FP_ILOGB0` (INT_MIN), `ilogb(NaN)` = `FP_ILOGBNAN`, both with `FE_INVALID`. |
| `significand` | `scalb(x, -ilogb(x))`. ⚑ HIST (BSD). |
| `scalb` | Multiply by 2^y for floating `y`. ⚑ HIST: obsolete (XSI), only `double`/`float`/`long double`. |
| `nextafter`, `nexttoward`, `nextup`, `nextdown` | The adjacent representable value toward `y`, or toward ±∞. Overflow and underflow flagged. |
| `copysign`, `fabs` | Sign manipulation. Exact, never raise exceptions (even for sNaN). |
| `fmod`, `remainder`, `drem`, `remquo` | Exact remainders. `fmod` truncates, `remainder` rounds to nearest. `remquo` also returns ≥ 3 low bits of the quotient with its sign. `drem` is the BSD alias of `remainder` (⚑ HIST). |
| `fdim`, `fmax`, `fmin` | Positive difference; maximum and minimum treating a quiet NaN as missing data. |
| `fmaximum`, `fminimum`, `fmaximum_num`, `fminimum_num`, `fmaximum_mag`, `fminimum_mag`, `fmaximum_mag_num`, `fminimum_mag_num` (2.35, C23) | IEEE 754-2019 maximum and minimum: NaN-propagating, or number-preferring (`_num`), or by magnitude (`_mag`). −0 < +0. |
| `fmaxmag`, `fminmag` | TS 18661-1 magnitude max/min. ⚑ PATH: superseded by C23's `_mag_num`. |
| `canonicalize` | Produce the canonical encoding of a value. It matters only for formats with non-canonical encodings (x87 pseudo-denormals). A signalling NaN gives a quiet NaN and `FE_INVALID`. An encoding that represents no value (x87 unnormals, pseudo-infinities, pseudo-NaNs) leaves the destination unchanged and returns nonzero, raising nothing. |
| `getpayload`, `setpayload`, `setpayloadsig` | Read, or construct (quiet or signalling), a NaN payload. |
| `nan(tag)` | Produce a quiet NaN whose payload is derived from the Text `tag`. Equivalent to `strtod("NAN(tag)")`. |

### 10.3 Elementary transcendental functions

**Contract.** Each computes the named mathematical function, with the documented accuracy
(§10.0) and C Annex F special cases. Domain or range failures are reported as in §10.0.

| Family | Functions | Notable edge cases |
|---|---|---|
| Exponential | `exp`, `exp2`, `exp10`, `expm1`, `pow10` (compat alias of `exp10`, ⚑ HIST) | Overflow → `HUGE_VAL`/`ERANGE`. `expm1` is accurate near 0. |
| Logarithm | `log`, `log2`, `log10`, `log1p`, `logp1` (not in 2.39) | `log(0)` = −∞ (pole, `ERANGE`, `FE_DIVBYZERO`). `log(<0)` = NaN (`EDOM`). |
| Power and roots | `pow`, `sqrt`, `cbrt`, `hypot`, `rsqrt`/`compoundn`/`pown`/`powr`/`rootn` (not in 2.39) | `pow(x, ±0)` = 1 even for NaN x; `pow(1, y)` = 1; `pow(−∞, y)` by parity. `sqrt` is correctly rounded, and `sqrt(-0)` = −0. `hypot(±∞, NaN)` = +∞. |
| Trigonometric | `sin`, `cos`, `tan`, `sincos` (GNU: both at once), `asin`, `acos`, `atan`, `atan2`, `sinpi`/`cospi`/... (not in 2.39) | Arguments are reduced exactly for all finite inputs (Payne–Hanek), so results are meaningful for huge arguments. `sin(±∞)` = NaN (`EDOM`). `atan2` defines all signed-zero and infinity quadrants. |
| Hyperbolic | `sinh`, `cosh`, `tanh`, `asinh`, `acosh`, `atanh` | `atanh(±1)` = ±∞ (pole). `acosh(<1)` = NaN. |
| Error and gamma | `erf`, `erfc`, `tgamma`, `lgamma`, `lgamma_r`, `gamma` (alias of `lgamma`, ⚑ HIST), `signgam` / `__signgam` (data) | `lgamma` stores the sign of Γ(x) in the **global** `signgam`, so it is MT-Unsafe. ⚑ HIST, ⚑ PATH: `lgamma_r(x, &sign)` exists for that reason. `tgamma` at negative integers: NaN, `EDOM`. |
| Bessel | `j0`, `j1`, `jn`, `y0`, `y1`, `yn` | XSI. `yn(n, 0)` = −∞ (pole). Only `double`, `float`, `long double` and `_Float128`. ⚑ HIST: not in ISO C. |

### 10.4 Fused and narrowing arithmetic (TS 18661 / C23)

| Function(s) | Contract |
|---|---|
| `fma(x, y, z)` | (x × y) + z with a single rounding. |
| `fadd`, `fsub`, `fmul`, `fdiv`, `fsqrt`, `ffma`, and `d*`, `f32*`, `f32x*`, `f64*`, `f64x*` forms (for example `daddl`, `f32addf64`) | Perform the operation on wider-format arguments and round *once* to the narrower result format. Correctly rounded. ⚑ HIST: the name encodes both formats, giving about 150 symbols for 6 operations. |

### 10.5 Complex arithmetic (`<complex.h>`): `cabs`, `carg`, `cimag`, `creal`, `conj`, `cproj`, `cexp`, `clog`, `clog10`/`__clog10` (GNU), `cpow`, `csqrt`, `csin`, `ccos`, `ctan`, `casin`, `cacos`, `catan`, `csinh`, `ccosh`, `ctanh`, `casinh`, `cacosh`, `catanh`

**Contract.** The elementary functions of a complex argument with the principal branch cuts of
C Annex G. The special cases for infinite and NaN components are specified exhaustively in
Annex G, and glibc follows them. `cproj` projects onto the Riemann sphere. `cabs` is
`hypot(re, im)`, and `carg` is `atan2(im, re)`.

**Kernel.** None.

⚑ HIST. `cimag`, `creal` and `conj` are exported functions even though they are trivial
field accesses.

### 10.6 Floating-point environment (`<fenv.h>`): `feclearexcept`, `feraiseexcept`, `fetestexcept`, `fegetexceptflag`, `fesetexceptflag`, `fetestexceptflag`, `fesetexcept`, `fegetround`, `fesetround`, `fegetenv`, `fesetenv`, `feholdexcept`, `feupdateenv`, `fegetmode`, `fesetmode`, `feenableexcept`, `fedisableexcept`, `fegetexcept`, and the data `_FE_DFL_ENV`/`FE_DFL_ENV`, `FE_NOMASK_ENV`

**Contract.** Query and modify the calling thread's floating-point status (exception flags)
and control modes (rounding direction, trap enables):

* Clear, raise and test the flags `FE_INVALID`, `FE_DIVBYZERO`, `FE_OVERFLOW`,
  `FE_UNDERFLOW` and `FE_INEXACT`.
* Save and restore the flags as opaque objects (`fexcept_t`).
* Get and set the rounding direction: `FE_TONEAREST`, `FE_UPWARD`, `FE_DOWNWARD`,
  `FE_TOWARDZERO`.
* Save and restore the whole environment (`fenv_t`) or only the control modes (`femode_t`).
* `feholdexcept` saves the environment, clears the flags, and makes exceptions non-trapping.
* `feupdateenv` restores a saved environment and re-raises the saved exceptions.
* GNU extensions: `feenableexcept`, `fedisableexcept` and `fegetexcept` control
  **trapping**, where an exception delivers `SIGFPE`.

**Guarantees.** They return 0 on success, non-zero on failure. `fesetround` fails for an
unsupported mode. Exceptions: `fetestexcept` and `fetestexceptflag` return the set flags,
`fegetround` the mode, `feenableexcept` and `fedisableexcept` the previously enabled set (−1 on
failure), and `fegetexcept` the enabled set.

**Kernel.** None to change the state (it is CPU registers). Trapping exceptions are *delivered*
as `SIGFPE` by the kernel. The environment is per-thread and is inherited by new threads and
by `fork`.

⚑ HIST. The two FPUs on x86_64 (SSE MXCSR and x87) have separate control state. The
`fenv` functions must keep both in step, and a program that manipulates one directly
desynchronizes them.
⚑ COMPAT. `__fpu_control` and `__setfpucw` (`<fpu_control.h>`) set the *initial* x87
control word at startup. They are legacy.

---

## 11. Low-level I/O on file descriptors

**Section-wide notes.**

* Most functions in this section are **thin wrappers**. The contract *is* the kernel's, and
  glibc adds only:
  * the errno convention (C0.1);
  * thread cancellation (C0.2), for blocking calls;
  * 64-bit aliases (C0.4);
  * occasionally, emulation for older kernels.
* Such entries are marked **[thin]**. For them, "Kernel" names the syscall, and the contract
  is the kernel's documented behaviour (man-pages section 2). Only the caller-visible facts
  that a reimplementation must preserve are specified here.
* ⚑ KERN (section-wide): descriptor numbering, open-file-description sharing, the offset
  model, and the semantics of flags are all kernel ABI.

### 11.1 Opening and creating: `open`, `open64`, `openat`, `openat64`, `creat`, `creat64`, `__open_2`, `__openat_2` (fortify), `open_by_handle_at`, `name_to_handle_at` [thin]

**Contract.** Resolve a Path, relative to the current directory or to a directory fd
(`AT_FDCWD` for the current directory), and return a new fd referring to a new open file
description.

**Inputs.**

* **Access mode.** `O_RDONLY`, `O_WRONLY`, `O_RDWR` or `O_PATH`.
* **Creation flags.**
  * `O_CREAT`, `O_EXCL` and `O_TRUNC` create or truncate the file.
  * `O_NOCTTY` prevents acquiring a controlling terminal, and `O_NOFOLLOW` refuses a final
    symbolic link. `O_DIRECTORY` requires the Path to be a directory.
  * `O_CLOEXEC` closes the fd on exec, and `O_TMPFILE` creates an unnamed file.
* **Status flags.** `O_APPEND`, `O_NONBLOCK`, `O_SYNC`, `O_DSYNC`, `O_DIRECT`,
  `O_NOATIME`, `O_ASYNC`, `O_LARGEFILE`.
* **Mode.** Required iff `O_CREAT` or `O_TMPFILE`, and passed as a variadic argument.
  ⚑ HIST: `open` is variadic only because the mode was an optional third argument in
  V7. The fortify wrapper `__open_2` exists to catch a missing mode at compile time.

**Guarantees.**

* The lowest free descriptor number is returned (⚑ KERN).
* With `O_CREAT|O_EXCL` the create is atomic, even over most network file systems.
* The mode is masked by `umask`, or by the directory's default ACL.
* `creat(p, m)` = `open(p, O_CREAT|O_WRONLY|O_TRUNC, m)`. ⚑ HIST.

**Failure.** The kernel's errors: `EACCES`, `EEXIST`, `ENOENT`, `ENOTDIR`, `ELOOP`,
`EMFILE` (per-process limit, `RLIMIT_NOFILE`), `ENFILE`, `ENAMETOOLONG`, `EISDIR`, `EROFS`,
`ETXTBSY`, `EOVERFLOW`, `EINTR` (FIFOs, or blocking opens of devices), and others.

**Cancellation point.** Yes.

**Kernel.** `openat` (glibc implements `open` through `openat(AT_FDCWD, ...)`).
`open_by_handle_at` and `name_to_handle_at` operate on opaque kernel file handles (they need
`CAP_DAC_READ_SEARCH`).

⚑ KERN. `openat2` (resolve flags such as `RESOLVE_BENEATH` and `RESOLVE_NO_SYMLINKS`) has
no glibc wrapper in 2.39. Callers must use `syscall()`. The need, safe resolution confined to
a subtree, is real and is not exposed by glibc.

### 11.2 Closing: `close`, `close_range`, `closefrom`, `__close` [thin]

**Contract.** Release a descriptor. The underlying open file description is released when its
last descriptor closes. `close_range(lo, hi, flags)` closes (or, with `CLOSE_RANGE_CLOEXEC`,
marks close-on-exec) every descriptor in `[lo, hi]`. `closefrom(lo)` closes all descriptors
≥ lo and never fails.

**Guarantees (Linux).** After `close` returns, even with `EINTR` or `EIO`, the descriptor
is released. It must **not** be closed again, since the number may already have been reused.

**Failure.** `EBADF`, `EINTR`, `EIO` (for example, deferred write errors on NFS).

**Kernel.** `close`, `close_range`. `closefrom` falls back to iterating `/proc/self/fd`
(`openat`, `getdents64`) when `close_range` is unavailable, and aborts if even that fails.

⚑ HIST. Reporting deferred write errors through `close` is a historical accident of NFS. It
makes `close` a function whose error cannot be retried.

### 11.3 Reading and writing: `read`, `write`, `pread`, `pread64`, `pwrite`, `pwrite64`, `readv`, `writev`, `preadv`, `preadv64`, `pwritev`, `pwritev64`, `preadv2`, `pwritev2`, `preadv64v2`, `pwritev64v2`, `__read_chk`, `__pread_chk`, `__pread64_chk` [thin]

**Contract.**

* **`read(fd, buf, n)`** transfers up to `n` bytes from the file at the description's offset
  (or from the stream, for pipes, sockets and ttys) into `buf`, and advances the offset. It
  returns the count, which may be short: at end of file, for pipes and ttys, on signal
  interruption after partial transfer, and at the per-call cap. It returns 0 at end of file.
* **`write`** transfers up to `n` bytes from `buf`. It may be short (full device, pipe
  capacity, signal, `RLIMIT_FSIZE`).
* **`pread`/`pwrite`** use an explicit offset and do not move the file offset. ⚑ KERN:
  `pwrite` on an `O_APPEND` descriptor appends regardless of the offset (a Linux deviation
  from POSIX).
* **The `v` forms** perform scatter/gather across an array of `(base, len)` pairs, of at
  most `IOV_MAX` = 1024 entries. The transfer is atomic with respect to other writers on the
  same description, as for a single write.
* **The `v2` forms** add per-call flags: `RWF_HIPRI`, `RWF_DSYNC`, `RWF_SYNC`,
  `RWF_NOWAIT`, `RWF_APPEND`. An offset of -1 means "use the current offset".

**Inputs.**

* `n` up to `SSIZE_MAX`. ⚑ KERN: Linux transfers at most 0x7ffff000 bytes per call, so
  callers must loop.
* Writes to a pipe of at most `PIPE_BUF` (4096) bytes are atomic.

**Guarantees.** On success the byte count is exact. A regular-file `write` that returns `n`
has updated the page cache. Durability requires §11.8.

**Failure.**

* `EAGAIN`/`EWOULDBLOCK` for non-blocking I/O.
* `EINTR` if interrupted before any transfer.
* `EPIPE` plus `SIGPIPE` for a broken pipe or socket.
* `EFBIG`, `ENOSPC`, `EDQUOT`, `EIO`, `EISDIR`, `EINVAL` (bad alignment for `O_DIRECT`),
  `EBADF`, `EFAULT`, `EOVERFLOW`.

**Cancellation point.** Yes.

**Kernel.** `read`, `write`, `pread64`, `pwrite64`, `readv`, `writev`, `preadv`, `pwritev`,
`preadv2`, `pwritev2`. glibc emulates `preadv2`/`pwritev2` with `preadv`/`pwritev` when
flags are 0 and the kernel lacks them.

⚑ HIST. `ssize_t` returns reserve half the range for -1 (C0.4). `SIGPIPE` on a broken pipe is
a process-killing default that almost every network program disables (⚑ HIST, ⚑ KERN).

### 11.4 Positioning: `lseek`, `lseek64`, `llseek` (compat), `__lseek` [thin]

**Contract.** Set the description's offset relative to the start (`SEEK_SET`), the current
position (`SEEK_CUR`) or the end (`SEEK_END`). Also find the next data or hole region
(`SEEK_DATA`/`SEEK_HOLE`). Returns the resulting offset.

**Failure.**

* `ESPIPE` on pipes, sockets and FIFOs.
* `EINVAL` for a negative result or a bad `whence`.
* `ENXIO` for `SEEK_DATA`/`SEEK_HOLE` beyond the end of file.
* `EOVERFLOW`.

**Kernel.** `lseek`.

⚑ HIST. The shared, implicit file offset is a property of the open file description, so it is
shared across `dup`, `fork` and threads. This is a source of races, and it is why `pread` and
`pwrite` exist (⚑ PATH).

### 11.5 Descriptor duplication and control: `dup`, `dup2`, `dup3`, `__dup2`, `fcntl`, `fcntl64`, `__fcntl`, `getdtablesize` [thin]

**Contract.**

* **`dup(fd)`** returns the lowest free descriptor, sharing the description.
* **`dup2(old, new)`** makes `new` refer to `old`'s description, closing `new` first
  (atomically). If `old == new`, it validates `old` and returns it.
* **`dup3`** is like `dup2`, but accepts `O_CLOEXEC` and fails with `EINVAL` if
  `old == new`.
* **`fcntl(fd, cmd, ...)`** has these commands:
  * duplication: `F_DUPFD`, `F_DUPFD_CLOEXEC`;
  * descriptor flags: `F_GETFD`/`F_SETFD` (`FD_CLOEXEC`);
  * status flags: `F_GETFL`/`F_SETFL`;
  * locks: `F_GETLK`, `F_SETLK`, `F_SETLKW`, and the open-file-description locks
    `F_OFD_GETLK`, `F_OFD_SETLK`, `F_OFD_SETLKW`;
  * signal-driven I/O: `F_GETOWN`, `F_SETOWN`, `F_GETOWN_EX`, `F_SETOWN_EX`, `F_GETSIG`,
    `F_SETSIG`;
  * leases and notification: `F_SETLEASE`, `F_GETLEASE`, `F_NOTIFY`;
  * pipe size: `F_GETPIPE_SZ`, `F_SETPIPE_SZ`;
  * file seals: `F_ADD_SEALS`, `F_GET_SEALS`;
  * write-lifetime hints: `F_GET_RW_HINT`, `F_SET_RW_HINT`, `F_GET_FILE_RW_HINT`,
    `F_SET_FILE_RW_HINT`.
* **`getdtablesize()`** returns the `RLIMIT_NOFILE` soft limit. ⚑ HIST.

**Kernel.** `dup`, `dup2`/`dup3`, `fcntl`.

⚑ HIST. POSIX record locks (`F_SETLK`) are owned by the (process, inode) pair. Closing *any*
descriptor for the file releases all of the process's locks on it. This is a well-known
design defect, and the reason OFD locks were added (⚑ PATH).
⚑ HIST. `fcntl` is variadic, and its third argument's type depends on `cmd` (`int`,
`struct flock *`, `struct f_owner_ex *`, `uint64_t *`).

### 11.6 Device control: `ioctl` [thin]

**Contract.** Perform a device- or file-system-specific operation identified by a request
number, with one untyped argument.

**Kernel.** `ioctl`.

⚑ HIST. The request space is unstructured, and the argument type is implied by the request.
glibc cannot validate it. The terminal interface (§19) is the main caller-visible use that
glibc wraps with typed functions.

### 11.7 Multiplexing: `select`, `pselect`, `FD_SET`/`FD_CLR`/`FD_ISSET`/`FD_ZERO` (macros), `__fdelt_chk`, `poll`, `ppoll`, `__poll_chk`, `__ppoll_chk`, `epoll_create`, `epoll_create1`, `epoll_ctl`, `epoll_wait`, `epoll_pwait`, `epoll_pwait2` [thin]

**Contract.** Wait until one or more descriptors are ready (readable, writable, exceptional,
hang-up), until a timeout expires, or until a signal arrives.

* **`select`** uses three fixed-size bit sets, rewrites them on return, and takes a
  `struct timeval` timeout that Linux updates with the remaining time.
* **`pselect`** takes a `timespec` and atomically swaps the signal mask for the duration of
  the wait.
* **`poll`** takes an array of `{fd, events, revents}` and a millisecond timeout.
  **`ppoll`** takes a `timespec` and a signal mask.
* **epoll** is a kernel-held interest set:
  * `epoll_ctl` adds, modifies or deletes entries with edge- or level-triggered,
    `EPOLLONESHOT`, `EPOLLEXCLUSIVE` and `EPOLLWAKEUP` semantics;
  * `epoll_wait` returns ready entries with the caller's 64-bit cookie;
  * `epoll_pwait2` takes a `timespec`.
  * `epoll_create(size)` ignores `size` (it must be > 0). ⚑ HIST.

**Inputs.** For `select`, descriptors must be `< FD_SETSIZE` (1024). ⚑ ARB, ⚑ HIST:
`fd_set` is a fixed 1024-bit array baked into the ABI, and a larger descriptor overflows it
(fortify `__fdelt_chk` aborts). This is the main reason `select` is unusable in large
programs. `poll` and epoll have no such limit.

**Guarantees.** They return the number of ready descriptors, or 0 on timeout. A negative
`fd` in `poll` is ignored.

**Failure.** `EINTR`, `EBADF`, `EINVAL` (nfds > `RLIMIT_NOFILE`), `ENOMEM`.

**Cancellation point.** Yes.

**Kernel.** `pselect6`, `ppoll` (glibc implements `select` and `poll` through these),
`epoll_create1`, `epoll_ctl`, `epoll_pwait`, `epoll_pwait2`.

### 11.8 Durability and hints: `fsync`, `fdatasync`, `sync`, `syncfs`, `sync_file_range`, `posix_fadvise`, `posix_fadvise64`, `readahead`, `posix_fallocate`, `posix_fallocate64`, `fallocate`, `fallocate64`, `ftruncate`, `ftruncate64`, `truncate`, `truncate64` [thin]

**Contract.**

* **`fsync(fd)`** does not return until the file's data and metadata are on stable storage.
  **`fdatasync`** omits metadata that is not needed to retrieve the data.
* **`sync()`** schedules a flush of all file systems. On Linux it waits. **`syncfs(fd)`**
  flushes one file system.
* **`sync_file_range(fd, off, n, flags)`** controls writeback of a range. It gives **no**
  durability guarantee for metadata.
* **`posix_fadvise(fd, off, len, advice)`** declares the access pattern: `NORMAL`,
  `SEQUENTIAL`, `RANDOM`, `WILLNEED`, `DONTNEED`, `NOREUSE`. It returns the error number
  directly (C0.1).
* **`readahead`** is a Linux-specific prefetch.
* **`posix_fallocate`** guarantees that the space for the range is allocated, and extends the
  file if needed. It returns the error number directly. If the file system lacks `fallocate`,
  glibc **emulates** it by writing a byte into every block. ⚑ HIST: this emulation is slow,
  racy with concurrent writers, and cannot work on `O_APPEND` descriptors.
* **`fallocate(fd, mode, off, len)`** is the Linux call with modes such as
  `FALLOC_FL_PUNCH_HOLE`, `FALLOC_FL_COLLAPSE_RANGE`, `FALLOC_FL_ZERO_RANGE` and
  `FALLOC_FL_KEEP_SIZE`. It has no emulation.
* **`ftruncate`/`truncate`** set the file length. Extension reads as zeros.

**Kernel.** `fsync`, `fdatasync`, `sync`, `syncfs`, `sync_file_range`, `fadvise64`,
`readahead`, `fallocate`, `ftruncate`, `truncate`. The `posix_fallocate` emulation uses
`fstat`, `fstatfs` and `pwrite`.

### 11.9 In-kernel copying: `sendfile`, `sendfile64`, `copy_file_range`, `splice`, `tee`, `vmsplice` [thin]

**Contract.** Move data between descriptors without copying it through user memory.

* **`sendfile(out, in, &off, n)`**: `in` must be mmap-able. Since Linux 2.6.33, `out` may be
  any file.
* **`copy_file_range`** works between regular files, possibly as a reflink or a server-side
  copy.
* **`splice`** works when one end is a pipe. **`tee`** duplicates pipe contents. **`vmsplice`**
  maps user pages into a pipe.

**Guarantees.** Returns the count transferred, which may be short.

**Failure.** `EINVAL` or `EXDEV` for unsupported combinations. Callers must then fall back
to `read`/`write`. ⚑ HIST: glibc does not emulate `copy_file_range` (it did briefly, then
removed the emulation in 2.30).

**Kernel.** `sendfile`, `copy_file_range`, `splice`, `tee`, `vmsplice`.

### 11.10 Memory mapping and memory control: `mmap`, `mmap64`, `munmap`, `mprotect`, `pkey_mprotect`, `pkey_alloc`, `pkey_free`, `pkey_get`, `pkey_set`, `madvise`, `posix_madvise`, `process_madvise`, `process_mrelease`, `mremap`, `remap_file_pages`, `msync`, `mincore`, `mlock`, `mlock2`, `munlock`, `mlockall`, `munlockall`, `memfd_create`, `getpagesize`, `process_vm_readv`, `process_vm_writev` [thin]

**Contract.**

* **`mmap(addr, len, prot, flags, fd, off)`** creates a mapping of a file (`MAP_SHARED`,
  `MAP_PRIVATE`, `MAP_SHARED_VALIDATE`) or of anonymous memory (`MAP_ANONYMOUS`). Other
  flags:
  * placement: `MAP_FIXED`, `MAP_FIXED_NOREPLACE`;
  * population and residency: `MAP_POPULATE`, `MAP_LOCKED`, `MAP_NORESERVE`;
  * huge pages: `MAP_HUGETLB` with a size encoding;
  * stacks: `MAP_GROWSDOWN`, `MAP_STACK`;
  * persistent memory: `MAP_SYNC`.

  It returns the address, or `MAP_FAILED` (`(void*)-1`, ⚑ HIST: not `NULL`, because
  address 0 is mappable).
* **`munmap`** removes mappings in a range. **`mprotect`** changes access protections.
  **`pkey_mprotect`** also assigns a protection key.
* **`pkey_alloc`, `pkey_free`, `pkey_get`, `pkey_set`** manage memory protection keys.
  `pkey_get` and `pkey_set` read and write the per-thread PKRU register: user-space only, no
  syscall.
* **`madvise`** gives usage advice with Linux semantics. `MADV_DONTNEED` *discards*
  anonymous contents (a semantic effect, not advice). ⚑ HIST: advice with side effects.
* **`posix_madvise`** is the POSIX subset. It never discards data: glibc maps
  `POSIX_MADV_DONTNEED` to a no-op for that reason. It returns the error number directly.
* **`process_madvise`/`process_mrelease`** apply advice to, or reap, another process's memory
  through a pidfd.
* **`mremap`** grows, shrinks or moves a mapping. It is variadic, with a new-address argument
  for `MREMAP_FIXED`.
* **`remap_file_pages`** is deprecated and kernel-emulated.
* **`msync`** flushes a shared mapping (`MS_SYNC`, `MS_ASYNC`, `MS_INVALIDATE`). It is a
  cancellation point.
* **`mincore`** reports the residency of each page.
* **`mlock`, `mlock2` (`MLOCK_ONFAULT`), `munlock`, `mlockall(MCL_CURRENT|MCL_FUTURE|
  MCL_ONFAULT)`, `munlockall`** pin pages in RAM.
* **`memfd_create(name, flags)`** creates an anonymous file with optional sealing
  (`MFD_ALLOW_SEALING`) and huge pages (`MFD_HUGETLB`). New in the Linux 6.3 API:
  `MFD_EXEC`/`MFD_NOEXEC_SEAL`.
* **`getpagesize()`** returns the page size (from `AT_PAGESZ`). ⚑ HIST: `sysconf(_SC_PAGESIZE)`
  is the POSIX form.
* **`process_vm_readv`/`process_vm_writev`** copy between this and another process's address
  space (ptrace permission required).

**Inputs.** Addresses and offsets must be page-aligned (⚑ KERN). Lengths are rounded up to
pages.

**Failure.** `ENOMEM`, `EACCES`, `EINVAL`, `EAGAIN` (lock limits), `EPERM`, `EEXIST`
(`MAP_FIXED_NOREPLACE`), `ENODEV`, `EOVERFLOW`, `ETXTBSY`.

**Kernel.** `mmap`, `munmap`, `mprotect`, `pkey_mprotect`, `pkey_alloc`, `pkey_free`,
`madvise`, `process_madvise`, `process_mrelease`, `mremap`, `remap_file_pages`, `msync`,
`mincore`, `mlock`, `mlock2`, `munlock`, `mlockall`, `munlockall`, `memfd_create`,
`process_vm_readv`, `process_vm_writev`. Auxv `AT_PAGESZ`. The PKRU register (`rdpkru`/
`wrpkru` instructions).

### 11.11 Asynchronous I/O: `aio_read`, `aio_write`, `aio_fsync`, `aio_error`, `aio_return`, `aio_suspend`, `aio_cancel`, `lio_listio`, `aio_init` (and `*64` aliases; formerly `librt`)

**Contract.** Start a read, write or fsync that proceeds concurrently with the caller, and
observe its completion by polling (`aio_error` returns `EINPROGRESS` until done), by waiting
(`aio_suspend` with a timeout), or by notification (`sigevent`: a signal or a new thread).
`lio_listio` submits a batch (`LIO_WAIT`/`LIO_NOWAIT`). `aio_cancel` attempts to cancel
requests. `aio_return` retrieves the final status; an application calls it once per request,
and a further call returns the same status (Written 2026-10-04 (owner ruling, phase 12-13 report)). `aio_init` (GNU) tunes the
thread pool.

**Guarantees.** Requests on the same fd with the same priority complete in submission order
(glibc serializes them per fd). A request's `aiocb` must stay valid and unmodified until it
completes.

**Failure.** Submission fails with `EAGAIN` (resources), `EINVAL` (an invalid `aio_reqprio`,
`aio_fsync` operation, `lio_listio` mode or `aio_suspend` count, or no control block) or `EBADF`
(`aio_fsync` and `aio_cancel` on a descriptor that is not open). Every other error, including an
invalid offset, count or descriptor of a read or write, is the operation's own and is available
through `aio_error`. Corrected 2026-10-04 (three-way rule: the reference and POSIX both report
`EBADF` at submission, `spec/behaviour/aio.md`; an earlier text listed only `EAGAIN` and
`EINVAL`).

**Kernel.** glibc implements POSIX AIO **entirely in user space** with a pool of helper
threads that perform ordinary blocking `pread`/`pwrite`/`fsync`. So the requirements are
`clone`/`clone3` (threads), `futex`, `pread64`, `pwrite64`, `fsync`/`fdatasync`, and
`rt_sigqueueinfo`/`rt_tgsigqueueinfo` (notification). ⚑ HIST: the kernel's native
asynchronous interfaces (`io_submit`/`io_getevents` in Linux AIO, and `io_uring`) are **not**
used and have no glibc wrappers. The POSIX contract could be met with far fewer resources
through `io_uring`.

### 11.12 Event and notification descriptors: `eventfd`, `eventfd_read`, `eventfd_write`, `inotify_init`, `inotify_init1`, `inotify_add_watch`, `inotify_rm_watch`, `fanotify_init`, `fanotify_mark` [thin]

(`signalfd` is specified in §20, and `timerfd_*` in §23.)

**Contract.**

* **`eventfd(initval, flags)`** creates a descriptor holding a 64-bit counter. A read returns
  and resets it (or decrements it by 1 with `EFD_SEMAPHORE`), and a write adds to it.
  `eventfd_read` and `eventfd_write` are typed helpers that return 0 or -1.
* **inotify** reports file-system events (create, delete, modify, move, attrib, ...) on
  watched paths as a stream of variable-length records read from a descriptor.
* **fanotify** reports file-system access events with open file descriptors, optionally with
  permission decisions. It needs `CAP_SYS_ADMIN`.

**Kernel.** `eventfd2`, `inotify_init1`, `inotify_add_watch`, `inotify_rm_watch`,
`fanotify_init`, `fanotify_mark`.

### 11.13 Advisory whole-file and record locking: `flock`, `lockf`, `lockf64`

**Contract.**

* **`flock(fd, op)`** takes shared or exclusive whole-file locks (`LOCK_SH`, `LOCK_EX`,
  `LOCK_UN`, `LOCK_NB`). The lock is owned by the open file description.
* **`lockf(fd, cmd, len)`** locks, tests or unlocks an exclusive region from the current
  offset (`F_LOCK`, `F_TLOCK`, `F_ULOCK`, `F_TEST`). glibc implements it as `fcntl` POSIX
  record locks, so it inherits their per-process semantics (§11.5).

**Failure.** `EWOULDBLOCK`/`EAGAIN`, `EINTR`, `EDEADLK` (lockf), `ENOLCK`.

**Kernel.** `flock`, `fcntl(F_SETLK*)`.

⚑ HIST. Three lock families (`flock`, POSIX `fcntl`, OFD), with incompatible ownership
semantics and inconsistent interaction over NFS. The functional need is one advisory lock with
explicit ownership.

---

## 12. Buffered streams (`<stdio.h>`)

### 12.1 The stream model

**Contract.** A *stream* (`FILE *`) is a library object that provides:

* a **backing**: a file descriptor, a memory buffer, or user callbacks;
* a **user-space buffer** with one of three policies: fully buffered, line-buffered, or
  unbuffered;
* a **position**, and an **error indicator** and **end-of-file indicator**;
* an **orientation**: undecided, byte, or wide. It is fixed by the first I/O operation, or by
  `fwide`;
* for wide orientation, a **conversion state** that converts between wide characters and
  the locale's multibyte encoding at the buffer boundary;
* a **lock**, which makes each stdio call on the stream atomic with respect to other threads.

**Standard streams.** `stdin`, `stdout` and `stderr` are exported data objects, and so are
`_IO_2_1_stdin_` and the other `_IO_*` objects (⚑ COMPAT). `stdout` is line-buffered if it
refers to a terminal and fully buffered otherwise. `stderr` is unbuffered.

**Guarantees.**

* Output written to a stream reaches the backing when the buffer fills, when a newline is
  written to a line-buffered stream, at `fflush`, at `fclose`, at `exit`, or when input is
  requested from a line-buffered or unbuffered stream (which flushes all line-buffered output
  streams, per ISO C).
* Alternating input and output on an update stream requires an intervening `fflush`,
  a positioning call, or (for output after input) end of file. ⚑ HIST: this restriction
  reflects a single shared buffer, and is not needed functionally.

**Kernel.** For fd-backed streams: `read`, `write`, `lseek`, `close`, `fstat` (buffer size
from `st_blksize`, and tty detection through `ioctl(TCGETS)`), and `mmap` (read-only
streams opened with mode `m`). Memory and cookie streams need none.

⚑ HIST. The layout of `FILE` (`struct _IO_FILE`) is exposed in the headers. Macros and old
binaries access its buffer pointers directly (`_IO_read_ptr`, and so on). The layout is
therefore ABI and cannot change. The functional need is an opaque handle.

### 12.2 Opening and closing: `fopen`, `fopen64`, `fdopen`, `freopen`, `freopen64`, `fclose`, `fcloseall`, `fileno`, `fileno_unlocked`

**Contract.**

* **`fopen(path, mode)`** opens a Path and returns a stream. The mode is a Text:
  * a required prefix of `r`, `w`, `a`, `r+`, `w+` or `a+`;
  * an optional `b` (ignored on POSIX);
  * optional GNU and C11 modifiers: `x` (exclusive, `O_EXCL`), `e` (`O_CLOEXEC`), `m`
    (mmap for reading), `c` (no cancellation points), and `,ccs=CHARSET` (wide orientation
    with that encoding).

  Created files get mode `0666 & ~umask`.
* **`fdopen(fd, mode)`** wraps an existing descriptor. The mode must be compatible with the
  descriptor's access mode.
* **`freopen(path, mode, stream)`** closes `stream`'s backing and reopens it on a new Path
  while keeping the same `FILE *`. This is typically used to redirect a standard stream.
  A NULL `path` changes only the mode, and glibc supports this only as far as `fcntl`
  allows.
* **`fclose`** flushes, closes the backing, and frees the stream (even on error).
  **`fcloseall`** (GNU) closes all streams.
* **`fileno`** returns the backing descriptor, or -1/`EBADF` for streams without one.

**Failure.** `NULL` with the `errno` of the underlying `open`, `EINVAL` for a bad mode, or
`ENOMEM`. `fclose` returns `EOF` if the flush or the close failed. The stream is gone either
way.

**Kernel.** `openat`, `fcntl`, `close`, `lseek`, `dup3` (`freopen` preserves the descriptor
number).

⚑ HIST. The mode is a Text mini-language rather than a set of flags. The `a` mode maps to
`O_APPEND`, and after `a+` reads, POSIX's "writes always go to end" is enforced by the
kernel.

### 12.3 Memory and custom streams: `fmemopen`, `open_memstream`, `open_wmemstream`, `fopencookie`, `_IO_cookie_file` (compat)

**Contract.**

* **`fmemopen(buf, size, mode)`** gives stream access to a caller buffer of fixed size. If
  `buf` is NULL, the library allocates one, which is freed at close. In write modes, a NUL is
  maintained after the data when space permits.
* **`open_memstream(&ptr, &size)`** / **`open_wmemstream`** create a write-only stream to a
  growing buffer. `*ptr` and `*size` are updated at `fflush` and `fclose`. The caller frees
  `*ptr`.
* **`fopencookie(cookie, mode, {read, write, seek, close})`** (GNU) creates a stream whose
  backing is caller callbacks.

**Kernel.** None.

⚑ COMPAT. `fmemopen` has two versioned contracts (`@GLIBC_2.2.5` and `@GLIBC_2.22`). The
old one treats `size` as the data length in `r` mode and differs in NUL handling.

### 12.4 Buffering control: `setvbuf`, `setbuf`, `setbuffer`, `setlinebuf`, `fflush`, `fflush_unlocked`, `_flushlbf`, `__fpurge`, `fpurge`\*, `__flbf`, `__fbufsize`, `__fpending`, `__freading`, `__fwriting`, `__freadable`, `__fwritable`, `__fsetlocking`, `__flushlbf`

(\* not exported by glibc; `__fpurge` is.)

**Contract.**

* **`setvbuf(fp, buf, mode, size)`** selects `_IOFBF`, `_IOLBF` or `_IONBF`, and optionally a
  caller buffer. It must be called before any other operation on the stream.
* **`setbuf`**, **`setbuffer`** and **`setlinebuf`** are shorthand forms. ⚑ HIST.
* **`fflush(fp)`**:
  * For an output stream, it writes the buffered data to the backing.
  * For a seekable input stream, it discards the buffered input and repositions the backing
    to the logical position (POSIX 2008).
  * `fflush(NULL)` flushes all output streams.
* **`__fpurge`** discards buffered data without writing it.
* **The `<stdio_ext.h>` queries** report buffer size, pending bytes, and whether the stream is
  line-buffered, reading or writing, readable or writable.
* **`__fsetlocking(fp, FSETLOCKING_BYCALLER)`** disables the stream's internal locking.

**Failure.** `fflush` returns `EOF` and sets the error indicator.

### 12.5 Character and line I/O: `fgetc`, `getc`, `getchar`, `fputc`, `putc`, `putchar`, `ungetc`, `fgets`, `fputs`, `puts`, `gets` (compat), `getline`, `getdelim`, `__getdelim`, `getw`, `putw`, and `_unlocked` forms (`getc_unlocked`, `getchar_unlocked`, `fgetc_unlocked`, `putc_unlocked`, `putchar_unlocked`, `fputc_unlocked`, `fgets_unlocked`, `fputs_unlocked`, `fread_unlocked`, `fwrite_unlocked`, `feof_unlocked`, `ferror_unlocked`, `clearerr_unlocked`), `__uflow`, `__overflow`, `__underflow`, `__getdelim`, `__fgets_chk`, `__fgets_unlocked_chk`

**Contract.**

* **`fgetc`/`getc`/`getchar`** return the next byte as `unsigned char` converted to `int`, or
  `EOF` at end of file or on error (the indicators distinguish the two).
* **`fputc`/`putc`/`putchar`** write one byte and return it, or `EOF` on error.
* **`ungetc(c, fp)`** pushes back one byte, which the next read returns. At least one byte of
  pushback is guaranteed. It clears the end-of-file indicator. Pushback is discarded by a
  positioning call.
* **`fgets(buf, n, fp)`** reads at most `n-1` bytes, stopping after a newline, and
  terminates the result. It returns `buf`, or `NULL` if no bytes were read before end of file
  or an error.
* **`getline(&buf, &cap, fp)`** / **`getdelim(..., delim, ...)`** read a whole line (or
  delimited record), growing the caller's `malloc` buffer as needed. They return the number
  of bytes read *including the delimiter*, and embedded NUL bytes are preserved in the count.
  They return -1 at end of file or on error.
* **`fputs(s, fp)`** writes a Text without a newline. **`puts(s)`** writes it to `stdout`
  followed by a newline.
* **`gets`** reads a line with **no bound**. It was removed from ISO C11, and glibc exports it
  only for old binaries (⚑ HIST: an inherently unsafe interface).
* **`getw`/`putw`** read and write a native `int`. ⚑ HIST: they cannot distinguish the value
  `EOF` from end of file.
* **The `_unlocked` forms** have the same contracts without taking the stream lock. The
  caller must hold it (§12.9).
* **`__uflow`, `__underflow`, `__overflow`, `__wuflow`, `__wunderflow`, `__woverflow`** are the
  buffer refill and flush entry points called by old inline macros (⚑ COMPAT).

⚑ HIST. `fgets` cannot report how many bytes it read, so embedded NUL bytes silently truncate
the line. `getline` fixes this (⚑ PATH).
⚑ HIST. `getc` returns `int` so that one value can be the `EOF` sentinel. That is also the
reason for the ctype domain rule (§5.1).

### 12.5a Block I/O: `fread`, `fwrite`, `fread_unlocked`, `fwrite_unlocked`, `__fread_chk`, `__fread_unlocked_chk`

**Contract.**

* **`fread(buf, size, n, fp)`** reads up to `n` elements of `size` bytes and returns the
  number of *complete* elements read.
* **`fwrite`** writes up to `n` elements and returns the number of complete elements written.

A short count means end of file or an error, and `feof`/`ferror` tell which. If `size` or `n`
is 0, the result is 0 with no effect.

**Guarantees.** Bytes of a partially transferred final element are consumed or written, but
not counted. ⚑ ARB, ⚑ HIST: the element count hides how many bytes actually moved, so a
caller needing byte accuracy must pass `size = 1`. The functional need is a byte count.

**Kernel.** `read`, `write` (possibly bypassing the buffer for large transfers).

### 12.6 Formatted output: `printf`, `fprintf`, `dprintf`, `sprintf`, `snprintf`, `asprintf`, `__asprintf`, `vprintf`, `vfprintf`, `vdprintf`, `vsprintf`, `vsnprintf`, `vasprintf`, `obstack_printf`, `obstack_vprintf`, the wide forms `wprintf`, `fwprintf`, `swprintf`, `vwprintf`, `vfwprintf`, `vswprintf`, the fortified forms `__printf_chk`, `__fprintf_chk`, `__sprintf_chk`, `__snprintf_chk`, `__vsprintf_chk`, ..., `__obstack_printf_chk`, and the `__*_ieee128`/`__*ldbl*` (not on x86_64)

**Contract.** Render a format string with arguments into a destination:

| Destination | Functions |
|---|---|
| a stream | `fprintf` |
| `stdout` | `printf` |
| a descriptor | `dprintf` (no stream buffering) |
| a caller buffer | `sprintf` (unbounded), `snprintf` (bounded) |
| a fresh `malloc` buffer | `asprintf` |
| an obstack | `obstack_printf` |

The format language is ISO C with POSIX and GNU extensions:

* **Conversions:** `d i o u x X` (integers), `f F e E g G a A` (floating), `c s` (character
  and Text), `p` (pointer), `n` (store count), `%` (literal), `m` (GNU: `strerror(errno)`),
  `b`/`B` (C23 binary, 2.35+), `C`/`S` (XSI wide), `lc`/`ls` (wide).
* **Length modifiers:** `hh h l ll q L j z t`, and the C23 `wN`/`wfN` (2.38+).
* **Flags:** `- + space # 0`, `'` (locale thousands grouping), `I` (locale digits).
* **Field width and precision:** literal, or `*` from an argument.
* **Positional arguments:** `%n$`, `*m$` (POSIX).

**Guarantees.**

* Floating output is exact (correctly rounded) for any precision.
* The result is identical regardless of destination.
* Return values:
  * `snprintf` returns the length the full result *would* have had and writes at most
    `n-1` bytes plus a terminator.
  * `asprintf` returns the length, and `*strp` is set only on success.
  * The others return the number of bytes written.
* `%s` with a precision reads no more than the precision's bytes (the argument need not be
  terminated).
* `%s` with a NULL pointer prints `(null)` (GNU; undefined in ISO C).

**Failure.** A negative return, with `errno`:

* `EOVERFLOW` if the result exceeds `INT_MAX` (⚑ HIST: the return type is `int`, which
  caps formatted output at 2 GiB), or if a field width or precision written in the format
  exceeds `INT_MAX`; in that case nothing is written to a stream (owner ruling R3,
  2026-10-05);
* `ENOMEM`;
* `EILSEQ` (a wide character that cannot be converted);
* `EBADF` or the other stream errors.

In fortified builds, `%n` in a writable format string aborts. Mixing positional and
non-positional arguments is undefined.

**Kernel.** None for memory destinations. Stream or descriptor destinations use `write`.

⚑ HIST. The format string is an untyped mini-language interpreted at run time against
`va_list` arguments that have no type information. Type mismatches are undefined behaviour,
detectable only by compiler warnings. `%n` turns a format-string injection into an arbitrary
write. The functional need is typed formatting of values into text.
⚑ HIST. `sprintf` and `vsprintf` have no destination bound (C0.5).
⚑ HIST. Locale affects `%f` and `%'d` output (§7.1).

### 12.7 Formatted input: `scanf`, `fscanf`, `sscanf`, `vscanf`, `vfscanf`, `vsscanf`, `wscanf`, `fwscanf`, `swscanf`, `vwscanf`, `vfwscanf`, `vswscanf`, `__isoc99_*scanf` (all 12), `__isoc23_*scanf` (all 12)

**Contract.** Parse input according to a format of directives:

* white space matches any amount of white space;
* an ordinary character must match exactly;
* a conversion `%[*][width][m][len]conv` stores into successive pointer arguments. The
  conversions are `d i o u x X` (and C23 `b`), `a e f g`, `s`, `c`, `[set]`, `p`, `n` and `%`.
  `m` (POSIX 2008) allocates the destination for `s`, `c` and `[`, and `*` suppresses
  assignment.

The functions return the number of assignments made, or `EOF` if input failure occurred
before the first conversion.

**Guarantees.** Parsing stops at the first mismatch. The mismatching character is left
unread (at most one character of pushback, so a partial match such as `"1e"` for `%f`
consumes the prefix and fails). ⚑ ARB, ⚑ HIST: a single character of lookahead makes
parsing lossy.

**Failure.** Integer overflow in a conversion is undefined behaviour in ISO C. glibc stores a
wrapped or clamped value without an error. ⚑ HIST: `%d` cannot report range errors, so robust
programs avoid it and use `strtol`.

⚑ COMPAT, ⚑ PATH. There are three symbol sets for the same functions:

* the plain `scanf` (GNU: `%a` means "allocate", the pre-C99 meaning);
* `__isoc99_*` (C99: `%a` is a floating conversion);
* `__isoc23_*` (C23: `%i` and `%b` accept a `0b` prefix).

The header selects the set by language mode. The need behind all three is a single parser
with a specified grammar.

### 12.8 Positioning and status: `fseek`, `fseeko`, `fseeko64`, `ftell`, `ftello`, `ftello64`, `rewind`, `fgetpos`, `fgetpos64`, `fsetpos`, `fsetpos64`, `feof`, `ferror`, `clearerr`, `fwide`

**Contract.**

* **`fseek`/`fseeko`** set the position, discarding pushback and buffered input and flushing
  output. `fseeko` takes an `off_t`, while `fseek` takes a `long`. ⚑ HIST: `long` is 32 bits
  on 32-bit ABIs.
* **`ftell`/`ftello`** report the logical position.
* **`fgetpos`/`fsetpos`** save and restore the position together with the conversion state
  in an opaque `fpos_t`. This is required for stateful wide streams.
* **`rewind`** is `fseek(fp, 0, SEEK_SET)` plus `clearerr`, and reports no error. ⚑ HIST.
* **`feof`/`ferror`** test the indicators, and **`clearerr`** resets them.
* **`fwide(fp, mode)`** queries or sets the orientation (negative for byte, positive for
  wide, 0 to query). Once set, the orientation cannot change.

**Failure.** `ESPIPE`, `EINVAL`, `EOVERFLOW` (the position does not fit `long`).

### 12.9 Stream locking: `flockfile`, `ftrylockfile`, `funlockfile`

**Contract.** Acquire or release a stream's recursive lock, so that a sequence of operations
(typically `_unlocked` ones) executes atomically with respect to other threads. `ftrylockfile`
returns 0 on success and non-zero if the lock is held by another thread.

**Kernel.** `futex` (under contention).

### 12.10 Wide-character stream I/O: `fgetwc`, `getwc`, `getwchar`, `fputwc`, `putwc`, `putwchar`, `ungetwc`, `fgetws`, `fputws`, and `_unlocked` forms, `__fgetws_chk`, `__fgetws_unlocked_chk`

**Contract.** The same as §12.5 over wide characters. On a wide-oriented stream, each
wide character is converted to or from the multibyte encoding (the locale's, or the one given
with `ccs=`). `WEOF` is returned at end of file, on error, and on an encoding error (with
`EILSEQ` and the error indicator set).

⚑ HIST. Byte and wide orientation per stream, fixed at first use, is a C95 compromise. A
stream cannot mix `printf` and `wprintf`. The functional need is a byte stream plus an
optional decoder or encoder layer.

### 12.11 Customizing `printf`: `register_printf_function`, `register_printf_specifier`, `register_printf_modifier`, `register_printf_type`, `parse_printf_format`, `printf_size`, `printf_size_info`, `__printf_fp`

**Contract.**

* **`register_printf_specifier(spec, handler, arginfo)`** installs a process-wide handler for
  a new conversion letter. `register_printf_function` is the older form of the same.
* **`register_printf_modifier`** adds a length modifier (Text), and **`register_printf_type`**
  adds an argument type. **`parse_printf_format`** reports the argument types a format
  expects.
* **`printf_size`** is a ready-made handler that prints sizes with binary or decimal unit
  prefixes.
* **`__printf_fp`** is the internal floating-point formatter (⚑ COMPAT).

**Threads.** Registration is MT-Unsafe and global. ⚑ HIST: it changes the meaning of every
`printf` in the process, including in libraries. The functional need, formatting user-defined
types, is better met without global state.

### 12.12 Stream-related utilities: `ctermid`, `cuserid`, `tmpfile`, `tmpfile64`, `tmpnam`, `tmpnam_r`, `tempnam`, `P_tmpdir`, `L_tmpnam`, `TMP_MAX`

**Contract.**

* **`tmpfile()`** returns an update stream on a new file that is automatically removed at
  close or at process exit. glibc uses `O_TMPFILE` in `P_tmpdir` (`/tmp`) where supported;
  otherwise it creates the file and unlinks it immediately.
* **`tmpnam`/`tmpnam_r`/`tempnam`** return a Path that did not exist at the time of the
  call. ⚑ HIST: a time-of-check to time-of-use race makes them insecure (the linker warns).
  The need, "a new file nobody else can claim", is met by `mkstemp`/`tmpfile` (§13.9).
* **`ctermid(buf)`** returns the Path of the controlling terminal. It is always `"/dev/tty"`
  on Linux.
* **`cuserid(buf)`** returns a user name. ⚑ HIST: it was removed from POSIX because of
  ambiguity over whether it means the effective or the login user (glibc uses the effective
  UID).

**Kernel.** `openat` (`O_TMPFILE`, `O_EXCL`), `unlinkat`, `getrandom` with
`GRND_NONBLOCK` (name generation; falls back to `clock_gettime` bits), `geteuid`.

### 12.13 Legacy libio interface: `_IO_getc`, `_IO_putc`, `_IO_feof`, `_IO_ferror`, `_IO_flockfile`, `_IO_funlockfile`, `_IO_ftrylockfile`, `_IO_peekc_locked`, `_IO_padn`, `_IO_sgetn`, `_IO_seekoff`, `_IO_seekpos`, `_IO_vfscanf`, `_IO_vfprintf`, `_IO_2_1_stdin_`, `_IO_2_1_stdout_`, `_IO_2_1_stderr_`, `_IO_list_all`, `_IO_file_*`, `_IO_str_*`, `_IO_wfile_*`, `_IO_*_jumps`, `__uflow`, ...

**Contract.** Compat-only entry points. They were used by old inline macros and by the
libstdc++ `stdio_filebuf` that shared glibc's `FILE` implementation. Each has the contract of
the obvious public function. The vtable objects (`_IO_file_jumps`, ...) exist because
libstdc++ v2 subclassed `FILE`.

⚑ COMPAT. glibc must validate vtable pointers at every call (`_IO_vtable_check`) because a
writable `FILE` vtable pointer is a well-known exploitation target. This is a cost imposed
purely by the exposed layout.

---

## 13. File-system interface

**Section-wide notes.**

* Most operations are **[thin]** wrappers over the `*at` system calls. The non-`at` forms
  pass `AT_FDCWD`.
* ⚑ KERN. Path resolution, permission checks, symlink following, the `*at` directory-fd
  model, and inode metadata are all kernel ABI.
* ⚑ PATH. Every path function exists twice (`f(path)` and `fat(dirfd, path, flags)`),
  because the directory-relative form was added in 2006 to fix time-of-check to time-of-use
  races in the path-based form. The `*at` form is the base contract.

### 13.1 Working directory and root: `getcwd`, `__getcwd_chk`, `get_current_dir_name`, `getwd`, `__getwd_chk`, `chdir`, `fchdir`, `chroot`

**Contract.**

* **`getcwd(buf, size)`** stores the absolute Path of the current directory.
  * With `buf == NULL` (GNU), it allocates: `size` bytes, or exactly the needed size if
    `size` is 0.
  * If the current directory is unreachable from the root (for example, after `chroot` or a
    lazy unmount), Linux returns a path beginning `"(unreachable)"`. glibc then fails with
    `ENOENT`.
* **`get_current_dir_name()`** returns an allocated Path. It prefers `$PWD` if `$PWD` names
  the same directory, which preserves the symlinks the user navigated through.
* **`getwd(buf)`** assumes `buf` holds `PATH_MAX` bytes. ⚑ HIST: an unbounded interface,
  removed from POSIX.
* **`chdir`/`fchdir`** change the current directory. It is process-wide (⚑ KERN: cwd is per
  process, or shared per `CLONE_FS`).
* **`chroot(path)`** changes the root directory. It needs `CAP_SYS_CHROOT`, and does not
  change the cwd.

**Failure.** `ERANGE` (buffer too small), `ENOENT`, `EACCES`, `ENAMETOOLONG`.

**Kernel.** `getcwd`, `chdir`, `fchdir`, `chroot`, and `stat` (for `$PWD` validation).
glibc's generic `getcwd` fallback walks `..` with `openat`/`getdents64`/`fstatat`, for paths
longer than a page.

### 13.2 Directory reading: `opendir`, `fdopendir`, `readdir`, `readdir64`, `readdir_r`, `readdir64_r`, `rewinddir`, `seekdir`, `telldir`, `closedir`, `dirfd`, `getdirentries`, `getdirentries64`, `getdents64`

**Contract.**

* **`opendir(path)`**/**`fdopendir(fd)`** open a directory stream. `fdopendir` takes ownership
  of `fd`.
* **`readdir(d)`** returns the next entry (`struct dirent`: `d_ino`, `d_off`, `d_reclen`,
  `d_type`, `d_name`), or `NULL` at the end.
  * The entry storage belongs to the stream and is overwritten by the next call on the same
    stream. Different streams may be read concurrently.
  * `d_type` may be `DT_UNKNOWN`, in which case the caller must `stat` the entry.
* **`readdir_r`** copies the entry into a caller buffer. ⚑ HIST, ⚑ PATH: it is deprecated
  (2.24) because the caller cannot size `d_name` correctly for file systems with
  `NAME_MAX > 255`. The need that motivated it (thread safety) is met by `readdir`'s
  per-stream storage.
* **`telldir`/`seekdir`** save and restore a position. ⚑ KERN: positions are opaque 64-bit
  cookies from the file system (hash values on ext4). glibc maps them through a table
  because `long` may be 32 bits.
* **`rewinddir`** restarts the stream. **`closedir`** closes the stream and the fd.
  **`dirfd`** returns the stream's fd.
* **`getdents64(fd, buf, n)`** (2.30) is the raw syscall.
  **`getdirentries(fd, buf, n, &base)`** is a BSD wrapper. ⚑ HIST.

**Guarantees.** Every entry that exists for the whole duration of the scan is returned exactly
once. Entries created or removed during the scan may or may not appear. `.` and `..` are
included (except on some special file systems).

**Failure.** `readdir` returns `NULL` both at the end and on error. ⚑ HIST: the caller must
zero `errno` to distinguish them (C0.1). Errors are `EBADF`, `EOVERFLOW` (an entry does not
fit a 32-bit `ino_t`), and `ENOENT` (the directory was removed).

**Kernel.** `openat(O_DIRECTORY)`, `getdents64`, `lseek`, `close`.

⚑ HIST. `d_name` is declared as `char[256]` but may be longer on some file systems. The
declared array size is an ABI accident (⚑ ARB).

### 13.3 Directory scanning and tree walking: `scandir`, `scandir64`, `scandirat`, `scandirat64`, `alphasort`, `alphasort64`, `versionsort`, `versionsort64`, `ftw`, `ftw64`, `nftw`, `nftw64`, `fts_open`, `fts_read`, `fts_children`, `fts_set`, `fts_close`, `fts64_*`

**Contract.**

* **`scandir(dir, &list, filter, compar)`** reads all entries, keeps those for which
  `filter` returns non-zero, sorts them with `compar`, and returns an allocated array of
  allocated entries. It returns the count, or -1. **`alphasort`** compares names with
  `strcoll`, and **`versionsort`** with `strverscmp`.
* **`nftw(path, fn, nopenfd, flags)`** walks a tree, calling `fn` with the path, its `stat`,
  a type code, and a `FTW` record (level and basename offset):
  * type codes: `FTW_F`, `FTW_D`, `FTW_DNR`, `FTW_DP`, `FTW_NS`, `FTW_SL`, `FTW_SLN`;
  * flags: `FTW_PHYS` (do not follow symlinks), `FTW_MOUNT` (stay on one file system),
    `FTW_DEPTH` (post-order), `FTW_CHDIR`, and the GNU `FTW_ACTIONRETVAL` (`fn` controls
    skipping);
  * a non-zero return from `fn` stops the walk, and that value is returned.
* **`ftw`** is the older form, without flags. It always follows symlinks.
* **`fts_*`** (BSD) is an iterator-style walk. `fts_read` returns `FTSENT` records.
  `fts_set` requests skip, follow or again for the current entry, and `fts_children` lists a
  directory's children. It sorts with an optional comparator and reports cycles
  (`FTS_DC`).

**Inputs.** For `nftw`, `nopenfd` bounds simultaneously open directory descriptors. ⚑ ARB:
the walk depth is not limited by it, but the caller must pick the number.

**Failure.** -1 with `errno`. Entries that cannot be `stat`ed are reported as `FTW_NS`/`FTS_NS`.

**Kernel.** `openat`, `getdents64`, `newfstatat`, `fchdir`/`chdir` (`FTW_CHDIR`, fts
without `FTS_NOCHDIR`), `close`.

⚑ HIST. `nftw` and `fts` changing the process-wide cwd (by default for fts) is unsafe in
threaded programs. The need is a walk that uses `openat`-relative lookups, which fts can do
but does not by default.
⚑ COMPAT. `fts` was not LFS-safe until 2.34 added `fts64`.

### 13.4 Links, names and removal: `link`, `linkat`, `symlink`, `symlinkat`, `readlink`, `readlinkat`, `__readlink_chk`, `__readlinkat_chk`, `unlink`, `unlinkat`, `rmdir`, `remove`, `rename`, `renameat`, `renameat2` [thin]

**Contract.**

* **`link`** creates a new hard link (a name for the same inode).
* **`symlink(target, linkpath)`** creates a symbolic link whose content is `target`, which is
  not resolved at creation.
* **`readlink(path, buf, n)`** copies a link's content into `buf` **without** a terminator
  and returns its length. Truncation is silent: a result equal to `n` may mean truncation.
  ⚑ ARB, ⚑ KERN (the syscall behaves this way).
* **`unlink`** removes a non-directory name. **`rmdir`** removes an empty directory.
  **`remove`** (ISO C) does either.
* **`rename(old, new)`** atomically replaces `new` if it exists. **`renameat2`** (2.28) adds
  `RENAME_NOREPLACE`, `RENAME_EXCHANGE` and `RENAME_WHITEOUT`.

**Guarantees.** Rename within one file system is atomic: an observer sees either the old or the
new `new`, never neither.

**Failure.** `EXDEV` (rename or link across file systems), `EEXIST`, `ENOTEMPTY`, `EISDIR`,
`EBUSY`, `EMLINK`, `ELOOP`, `EPERM` (protected hardlinks or symlinks, sticky directories).

**Kernel.** `linkat`, `symlinkat`, `readlinkat`, `unlinkat`, `renameat2` (glibc implements
`rename` through `renameat2` or `renameat`).

### 13.5 Creating special files and directories: `mkdir`, `mkdirat`, `mkfifo`, `mkfifoat`, `mknod`, `mknodat`, `__xmknod`, `__xmknodat` (compat) [thin]

**Contract.** Create a directory, a FIFO, or a device, socket or regular node with the given
mode (masked by `umask`) and device number. `makedev`, `major` and `minor` (`gnu_dev_*`,
macros backed by inline functions) compose and decompose `dev_t`.

**Kernel.** `mkdirat`, `mknodat`.

⚑ COMPAT. `__xmknod` and the `__xstat` family (§13.6) carried a structure-version argument so
that `struct stat` could change layout. They became compat-only in 2.33.

### 13.6 File metadata: `stat`, `fstat`, `lstat`, `fstatat`, and `*64` forms, `statx`, `__xstat`, `__fxstat`, `__lxstat`, `__fxstatat` (and `64`, compat) [thin]

**Contract.** Return an inode's metadata:

* type and permissions (`st_mode`), `st_ino`, `st_dev`, `st_nlink`, `st_uid`, `st_gid`,
  `st_rdev`, `st_size`, `st_blksize`, `st_blocks`;
* `st_atim`, `st_mtim`, `st_ctim` (nanosecond `timespec`).

`fstatat` flags are `AT_SYMLINK_NOFOLLOW`, `AT_EMPTY_PATH` and `AT_NO_AUTOMOUNT`.

**`statx(dirfd, path, flags, mask, &stx)`** (2.28) requests specific fields and reports which
were filled (`stx_mask`). It adds birth time (`stx_btime`), attributes (`STATX_ATTR_*`),
mount ID, and direct-I/O alignment. It has sync control (`AT_STATX_SYNC_AS_STAT`,
`AT_STATX_FORCE_SYNC`, `AT_STATX_DONT_SYNC`).

**Failure.** `ENOENT`, `EACCES`, `ELOOP`, `ENOTDIR`, `EOVERFLOW` (a field does not fit a
32-bit type).

**Kernel.** `newfstatat` (`fstat`, `stat` and `lstat` all go through it on x86_64), `statx`
(glibc emulates it with `fstatat` on kernels older than 4.11).

⚑ KERN, ⚑ HIST. `struct stat`'s layout is per-architecture kernel ABI, with padding
reflecting 1990s decisions. `statx` is the kernel's extensible replacement. Birth time is
available only through `statx`.

### 13.7 Permissions, ownership and timestamps: `chmod`, `fchmod`, `fchmodat`, `lchmod`, `chown`, `fchown`, `lchown`, `fchownat`, `umask`, `access`, `faccessat`, `euidaccess`, `eaccess`, `utime`, `utimes`, `lutimes`, `futimes`, `futimesat`, `utimensat`, `futimens` [thin]

**Contract.**

* **The `chmod` family** changes permission bits. **`lchmod`** and
  **`fchmodat(..., AT_SYMLINK_NOFOLLOW)`** change a symlink itself. Since 2.32, glibc
  implements them via `O_PATH` + `/proc/self/fd`, or through `fchmodat2` on kernel 6.6+.
  Linux symlinks have no mode, so the result is `EOPNOTSUPP`.
* **The `chown` family** changes owner and group. -1 leaves a field unchanged.
* **`umask(mask)`** sets the process file-creation mask and returns the old one. It
  **cannot fail**. ⚑ HIST: there is no way to read the mask without setting it, except by
  setting it twice (racy) or by reading `/proc/self/status`.
* **`access(path, mode)`** checks `R_OK`, `W_OK`, `X_OK` or `F_OK` using the **real** UID and
  GID. **`faccessat(..., AT_EACCESS)`** and **`euidaccess`/`eaccess`** use the effective IDs.
  ⚑ HIST: `access` was designed for setuid programs to ask "could the invoking user do
  this?", which is inherently a time-of-check to time-of-use race.
* **`utimensat`/`futimens`** set access and modification times with nanosecond precision.
  `UTIME_NOW` and `UTIME_OMIT` are available per field.
* **`utime`** (seconds), **`utimes`**, **`lutimes`**, **`futimes`** (µs) and **`futimesat`**
  are older forms. ⚑ HIST.

**Kernel.** `fchmodat`, `fchmodat2`, `fchmod`, `fchownat`, `fchown`, `umask`, `faccessat`,
`faccessat2`, `utimensat`. `euidaccess` without `faccessat2` computes the answer from
`stat` and credentials (`getresuid`, `getgroups`).

### 13.8 Path canonicalization: `realpath`, `__realpath_chk`, `canonicalize_file_name`

**Contract.** Resolve a Path to an absolute Path without `.`, `..` or symbolic-link
components, naming the same file.

* **`realpath(path, resolved)`** writes to `resolved` (`PATH_MAX` bytes), or allocates the
  result if `resolved` is NULL.
* **`canonicalize_file_name`** always allocates.

**Failure.** `ENOENT` (any component is missing: the file must exist), `ELOOP`,
`ENAMETOOLONG`, `EACCES`. After any failure the contents of a caller buffer are unspecified
(POSIX leaves them undefined); callers must not rely on them. glibc writes the prefix that was
resolved, including the failing component, only for `ENOENT` and `EACCES`, and leaves the buffer
untouched for `ELOOP`, `ENOTDIR` and `ENAMETOOLONG` (`spec/behaviour/fstree.md`, the `realpath`
lines). Corrected 2026-10-03 (owner ruling): an earlier text said the buffer held the resolved
prefix after any failure.

**Kernel.** `readlinkat`/`readlink`, `getcwd`, `newfstatat`. ⚑ HIST: the lexical walk
performs one syscall per component. On Linux the same answer is obtainable in one step as
`readlink("/proc/self/fd/N")` after `open(O_PATH)`, but that differs for unreachable paths.
⚑ ARB: a caller buffer is assumed to be exactly `PATH_MAX` bytes, with no size parameter.

### 13.9 Temporary files and directories: `mkstemp`, `mkstemp64`, `mkostemp`, `mkostemp64`, `mkstemps`, `mkstemps64`, `mkostemps`, `mkostemps64`, `mkdtemp`, `mktemp`

**Contract.**

* **`mkstemp(template)`** replaces the trailing `XXXXXX` in a caller-writable Text with
  unique characters, creates the file with `O_CREAT|O_EXCL`, mode 0600, and returns the fd.
* **The `o` forms** accept extra `open` flags (`O_CLOEXEC`, `O_APPEND`, `O_SYNC`).
* **The `s` forms** allow a fixed suffix after the `X`s.
* **`mkdtemp`** creates a directory with mode 0700.
* **`mktemp`** only produces a name. ⚑ HIST: it is insecure, and the linker warns.

**Inputs.** The template must end in exactly six `X`s before the suffix. ⚑ ARB: six is a
historical constant. More `X`s would give more entropy, but only the last six are replaced.

**Failure.** `EINVAL` (bad template), `EEXIST` (about 238,000 attempts exhausted), and the
`open` errors.

**Kernel.** `openat(O_CREAT|O_EXCL)`, `mkdirat`, `getrandom` (falling back to `clock_gettime`).

### 13.10 File-system statistics: `statfs`, `fstatfs`, `statfs64`, `fstatfs64`, `statvfs`, `fstatvfs`, `statvfs64`, `fstatvfs64`, `ustat` (compat) [thin]

**Contract.**

* **`statfs`** returns the Linux structure: type magic, block size, blocks, free and available
  blocks, files, free files, fsid, name length, fragment size, and mount flags.
* **`statvfs`** is the POSIX form. glibc derives `f_flag` from `statfs.f_flags`, and since
  2.39 `struct statvfs` also has `f_type`.

**Kernel.** `statfs`, `fstatfs`. ⚑ HIST: before `f_flags` existed in `statfs`, glibc
populated the mount flags by scanning `/proc/mounts`. That code survives as a fallback.

### 13.11 Extended attributes: `setxattr`, `lsetxattr`, `fsetxattr`, `getxattr`, `lgetxattr`, `fgetxattr`, `listxattr`, `llistxattr`, `flistxattr`, `removexattr`, `lremovexattr`, `fremovexattr` [thin]

**Contract.** Set, get, list and remove named binary attributes on an inode. The namespaces are
`user.`, `trusted.`, `security.` and `system.`.

* `setxattr` flags: `XATTR_CREATE`, `XATTR_REPLACE`.
* `get` and `list` with `size` 0 return the needed size.
* `list` returns names as concatenated NUL-terminated Text.

**Failure.** `ENODATA`, `ERANGE`, `ENOTSUP`, `E2BIG`, `EDQUOT`.

**Kernel.** The twelve `*xattr` syscalls. ⚑ KERN: names are NUL-terminated and at most
`XATTR_NAME_MAX` (255) bytes. Values are at most `XATTR_SIZE_MAX` (64 KiB).

### 13.12 Mounting: `mount`, `umount`, `umount2`, `fsopen`, `fsconfig`, `fsmount`, `fspick`, `move_mount`, `open_tree`, `mount_setattr`, `pivot_root` (compat) [thin]

**Contract.**

* **`mount(src, target, fstype, flags, data)`** attaches, remounts, binds, moves, or changes
  propagation. `data` is a file-system-specific Text.
* **`umount2(target, flags)`**: `MNT_FORCE`, `MNT_DETACH`, `MNT_EXPIRE`, `UMOUNT_NOFOLLOW`.
* **The 2.36 fd-based mount API** (`fsopen`/`fsconfig`/`fsmount`/`move_mount`/`open_tree`/
  `fspick`/`mount_setattr`) configures a file-system context through a descriptor, which
  avoids the string-parsing `data` argument.

**Kernel.** The syscalls of the same names. They need `CAP_SYS_ADMIN` in the owning user
namespace.

⚑ HIST. `mount`'s untyped `data` string (parsed by each file system differently) is why the
new API exists.

### 13.13 Mount tables and fstab: `setmntent`, `getmntent`, `getmntent_r`, `__getmntent_r`, `addmntent`, `endmntent`, `hasmntopt`, `setfsent`, `getfsent`, `getfsspec`, `getfsfile`, `endfsent`

**Contract.**

* **`setmntent(file, mode)`** opens a mount table file: `/proc/self/mounts` (`_PATH_MOUNTED`
  is `/etc/mtab`, a symlink to it), or `/etc/fstab`.
* **`getmntent`** parses the next line into `struct mntent`: `fsname`, `dir`, `type`,
  `opts`, `freq`, `passno`. The escapes `\040`, `\011`, `\012` and `\134` (and `\\`) are
  decoded to space, tab, newline and backslash; every other backslash is kept. Corrected
  2026-10-03 (three-way rule: the reference library and getmntent(3) agree,
  `spec/behaviour/mntent.md`; the kernel's `/proc/self/mounts` writes only those four octal
  forms): an earlier text said octal escapes in general were decoded. `getmntent`
  returns static storage. ⚑ PATH: hence `getmntent_r`.
* **`addmntent`** appends an entry. **`hasmntopt(ent, opt)`** finds an option in `opts`.
* **The `fsent` functions** read `/etc/fstab` into `struct fstab`.

**Kernel.** `openat`/`read` on `/proc/self/mounts`. **Config:** `/etc/fstab`, `/etc/mtab`.

⚑ HIST. The mount table is text meant to be parsed. The kernel's structured interfaces
(`/proc/self/mountinfo`, `statmount`/`listmount` in Linux 6.8) are not wrapped.

### 13.14 Swap: `swapon`, `swapoff` [thin]

**Contract.** Enable or disable a swap device or file, with priority flags.
**Kernel.** `swapon`, `swapoff`.

### 13.15 Unsupported BSD file flags: `chflags`, `fchflags` (compat stubs)

**Contract.** None. They always fail with `ENOSYS`. ⚑ COMPAT.

---

## 14. Searching, sorting and pattern matching

### 14.1 Sorting: `qsort`, `qsort_r`

**Contract.** Reorder an array of `n` elements of `size` bytes into non-decreasing order
according to a caller comparison function. `qsort_r` passes an extra context argument to the
comparator (the GNU argument order, which differs from BSD's; ⚑ COMPAT).

**Inputs.** The comparator must implement a consistent total preorder. An inconsistent
comparator leaves the array in unspecified order. The 2.39 algorithms still stay within the
array in that case. Older releases' quicksort fallback could access memory outside the array
when given a non-transitive comparator (the Qualys report of January 2024).

**Guarantees.**

* The array is a permutation of the input, sorted.
* **Not stable.** ⚑ ARB, ⚑ HIST: glibc's historical mergesort *was* stable whenever it could
  allocate, and many programs silently depended on that. The contract never promised it.
* 2.39 uses a mergesort with a temporary buffer (on the stack for small inputs, otherwise
  `malloc`), and heapsort when allocation fails. It is O(n log n) worst case and never
  fails.

**Kernel.** None (`malloc` may use `mmap`/`brk`).

⚑ HIST. The comparator receives untyped pointers and returns an `int` whose sign is the
order. Element size is a runtime parameter, which prevents type checking and inlining.

### 14.2 Binary search: `bsearch`

**Contract.** Find *an* element equal to the key in a sorted array, or return `NULL`. If there
are several equal elements, which one is returned is unspecified.

**Kernel.** None.

### 14.3 Linear search: `lfind`, `lsearch`

**Contract.** Find the key in an unsorted array with `*nmemb` elements. `lsearch` appends the
key if it is absent and increments `*nmemb`. The caller must guarantee space for one more
element.

**Kernel.** None.

### 14.4 Binary search trees: `tsearch`, `tfind`, `tdelete`, `twalk`, `twalk_r`, `tdestroy`, `__tsearch`, `__tfind`, `__tdelete`, `__twalk`, `__twalk_r`, `__tdestroy`

**Contract.** A balanced ordered set of caller pointers keyed by a comparator. The root is a
caller `void *`.

* **`tsearch`** finds or inserts, returning a pointer to the node, whose first member is the
  key pointer.
* **`tfind`** finds.
* **`tdelete`** removes and returns the parent node (unspecified if the tree becomes empty;
  non-NULL on success).
* **`twalk`** visits nodes, reporting each in pre-, in- and post-order (`preorder`,
  `postorder`, `endorder`, `leaf`). ⚑ HIST: the SVID names are counter-intuitive.
  "postorder" means "in-order".
* **`twalk_r`** (2.30) passes a context argument.
* **`tdestroy`** (GNU) frees all nodes, calling a free function for each key.

**Guarantees.** O(log n) operations (glibc uses a red-black tree).

**Failure.** `tsearch` returns `NULL` if memory is exhausted.

**Kernel.** None.

⚑ PATH. `twalk` had no context argument, so callers used globals. `twalk_r` fixes this.

### 14.5 Hash tables: `hcreate`, `hsearch`, `hdestroy`, `hcreate_r`, `hsearch_r`, `hdestroy_r`

**Contract.** A fixed-capacity hash table mapping Text keys to `void *` data. `hsearch(item,
FIND|ENTER)` finds or inserts.

**Inputs.** The capacity is fixed at creation. glibc rounds it up to a prime.

**Failure.**

* `hsearch(ENTER)` returns `NULL` with `ENOMEM` when the table is full.
* The table cannot be resized, and entries cannot be deleted.
* `hdestroy` does not free keys.

⚑ HIST, ⚑ ARB. The non-`_r` forms use a **single process-global table**. That is why the `_r`
forms exist (⚑ PATH). The fixed capacity and missing deletion are not functional necessities.

### 14.6 Doubly linked queues: `insque`, `remque`

**Contract.** Insert an element after a given one, or remove an element, in a caller-managed
doubly linked list whose first two members are the forward and backward pointers. A NULL
predecessor starts a new linear list.

**Kernel.** None. ⚑ HIST.

### 14.7 Shell-style wildcard matching: `fnmatch`

**Contract.** Decide whether a Text matches a shell pattern.

* **Pattern syntax:** `*`, `?`, `[...]` bracket expressions (ranges, `[:class:]`,
  `[=equiv=]`, `[.coll.]`), and `\` escapes.
* **Flags:**
  * `FNM_NOESCAPE`;
  * `FNM_PATHNAME` (wildcards do not match `/`);
  * `FNM_PERIOD` (a leading `.` must be matched literally);
  * `FNM_LEADING_DIR`;
  * `FNM_CASEFOLD`;
  * `FNM_EXTMATCH` (ksh extended patterns `?(...)`, `*(...)`, `+(...)`, `@(...)`,
    `!(...)`).

**Guarantees.** Returns 0 for a match and `FNM_NOMATCH` otherwise. Multibyte-aware in the
current locale.

**Failure.** Returns another non-zero value on error (for example `ENOMEM`). Invalid multibyte
text in the pattern or the string is not an error: it is matched byte by byte, as an ordinary
character per byte, so that `glob` can match any file name. Corrected 2026-10-03 (owner ruling;
`spec/behaviour/patterns.md`: glibc matches such text byte by byte, POSIX and fnmatch(3) are
silent): an earlier text gave invalid multibyte text as an example of an error.

**Kernel.** None.

### 14.8 Path-name expansion: `glob`, `glob64`, `globfree`, `globfree64`, `glob_pattern_p`

**Contract.** Find all Paths matching a shell pattern, and return them in a
`glob_t {gl_pathc, gl_pathv, gl_offs}`.

* **Standard flags:** `GLOB_ERR`, `GLOB_MARK`, `GLOB_NOSORT`, `GLOB_DOOFFS`,
  `GLOB_NOCHECK`, `GLOB_APPEND`, `GLOB_NOESCAPE`.
* **GNU flags:**
  * `GLOB_PERIOD`, `GLOB_BRACE` (`{a,b}`), `GLOB_NOMAGIC`, `GLOB_TILDE`,
    `GLOB_TILDE_CHECK` (`~user` expansion);
  * `GLOB_ONLYDIR`;
  * `GLOB_ALTDIRFUNC`, with caller directory-access callbacks.
* Results are sorted with `strcoll` unless `GLOB_NOSORT`. An error callback may abort the
  walk.
* **`glob_pattern_p`** reports whether a pattern contains wildcards.

**Failure.** `GLOB_NOSPACE`, `GLOB_ABORTED`, `GLOB_NOMATCH`.

**Kernel.** `openat`, `getdents64`, `newfstatat`/`lstat`. **Config:** the passwd database via
NSS (for `~user`), and `HOME`.

⚑ COMPAT. `glob@GLIBC_2.2.5` versus `glob@GLIBC_2.27`: the newer version no longer follows
dangling symlinks when checking existence.

### 14.9 Regular expressions: `regcomp`, `regexec`, `regerror`, `regfree`, `re_compile_pattern`, `re_compile_fastmap`, `re_search`, `re_search_2`, `re_match`, `re_match_2`, `re_set_registers`, `re_set_syntax`, `re_syntax_options`, `re_comp`, `re_exec`, `re_max_failures`

**Contract.**

* **POSIX interface.**
  * `regcomp(&re, pattern, cflags)` compiles a basic or extended (`REG_EXTENDED`) regular
    expression. Other flags are `REG_ICASE`, `REG_NOSUB` and `REG_NEWLINE`.
  * `regexec(&re, text, nmatch, pmatch, eflags)` finds the leftmost-longest match and fills
    subexpression offsets (`regmatch_t`). `eflags` are `REG_NOTBOL`, `REG_NOTEOL` and the
    GNU `REG_STARTEND`, which lets `pmatch[0]` delimit the text so that it need not be
    terminated.
  * `regerror` formats an error code. `regfree` releases compiled storage.
* **GNU interface.** `re_compile_pattern`, `re_search` and `re_match` work on
  length-delimited buffers (including split buffers, `_2`) with a selectable syntax
  (`re_syntax_options`, `RE_SYNTAX_*`). They support back-references, and word and buffer
  anchors.
* **BSD interface.** `re_comp`/`re_exec` use one global compiled pattern. ⚑ HIST.

**Guarantees.** POSIX leftmost-longest semantics, and multibyte (locale) awareness. This
includes subexpression selection as POSIX XBD 9.1 defines it: consistent with the whole match
being the longest of the leftmost matches, each subpattern, from left to right, matches the
longest possible string (a null string being longer than no match), and `regexec` reports
subexpressions by its rules (the last match of a repeated group; -1 for a group that did not take
part, including a group nested in a repetition whose last iteration did not take it). Clarified
2026-10-03 (owner ruling, `spec/behaviour/regex-submatch-analysis.md`): glibc selects
subexpressions by alternation priority instead.

**Failure.** `regcomp` returns `REG_BADPAT`, `REG_ECOLLATE`, `REG_ECTYPE`, `REG_EESCAPE`,
`REG_ESUBREG`, `REG_EBRACK`, `REG_EPAREN`, `REG_EBRACE`, `REG_BADBR`, `REG_ERANGE`,
`REG_ESPACE` or `REG_BADRPT`. `regexec` returns `REG_NOMATCH` or `REG_ESPACE`.

**Kernel.** None.

⚑ HIST. Back-references make matching NP-hard. Exponential-time inputs are possible, and
`re_max_failures` is a global limit on backtracking.
⚑ HIST. The POSIX interface requires a terminated input (`REG_STARTEND` is the escape hatch),
so text containing NUL cannot be searched portably.

### 14.10 Shell word expansion: `wordexp`, `wordfree`

**Contract.** Perform POSIX shell word expansion on a Text:

* tilde, parameter and arithmetic expansion;
* command substitution (`$(...)`, backquotes) unless `WRDE_NOCMD`;
* field splitting on `IFS`;
* pathname expansion and quote removal.

The result is a list of words. Other flags are `WRDE_DOOFFS`, `WRDE_APPEND`, `WRDE_REUSE`,
`WRDE_SHOWERR` and `WRDE_UNDEF`.

**Failure.** `WRDE_BADCHAR` (an unquoted `|&;<>(){}` or newline), `WRDE_BADVAL`
(an undefined variable with `WRDE_UNDEF`), `WRDE_CMDSUB` (command substitution with
`WRDE_NOCMD`), `WRDE_NOSPACE`, `WRDE_SYNTAX`.

**Kernel.** Command substitution runs `/bin/sh -c` in a child: `pipe2`, `clone`/`fork`,
`execve`, `wait4`, `read`. Globbing uses §14.8.

⚑ HIST. Embedding a shell interpreter in libc (with a subprocess for `$(...)`) is a large and
security-sensitive surface for a function commonly used only for `~` and `$VAR` expansion.

---

## 15. Sockets and network interfaces

**Section-wide notes.**

* The socket calls are **[thin]**. ⚑ KERN: the address structures (`sockaddr_in`,
  `sockaddr_in6`, `sockaddr_un`, `sockaddr_ll`, `sockaddr_nl`, ...), the option levels and
  names, and the message-header layout are kernel ABI.
* ⚑ HIST. The generic `struct sockaddr` (a 2-byte family plus 14 bytes) is too small for most
  address families. That is why `struct sockaddr_storage` exists, and why every call passes an
  explicit length. A tagged union of address types would express the need directly.

### 15.1 Socket lifecycle: `socket`, `socketpair`, `bind`, `listen`, `accept`, `accept4`, `connect`, `shutdown`, `getsockname`, `getpeername`, `getsockopt`, `setsockopt`, `sockatmark`, `isfdtype` [thin]

**Contract.**

* **`socket(domain, type, protocol)`** creates an endpoint. The domains are `AF_INET`,
  `AF_INET6`, `AF_UNIX`, `AF_NETLINK`, `AF_PACKET`, and others. The types are
  `SOCK_STREAM`, `SOCK_DGRAM`, `SOCK_SEQPACKET` and `SOCK_RAW`, optionally combined with
  `SOCK_NONBLOCK` and `SOCK_CLOEXEC`.
* **`socketpair`** creates a connected pair. On Linux it is `AF_UNIX` only.
* **`bind`** assigns a local address, and **`listen(fd, backlog)`** marks the socket
  passive.
* **`accept`/`accept4`** return a new connected socket and, optionally, the peer address.
  `accept4` takes `SOCK_NONBLOCK|SOCK_CLOEXEC`.
* **`connect`** initiates a connection, or sets the default peer for datagram sockets. A
  non-blocking connect fails with `EINPROGRESS`, and completion is observed via
  `poll` + `SO_ERROR`.
* **`shutdown(fd, SHUT_RD|SHUT_WR|SHUT_RDWR)`** half-closes the connection.
* **`getsockname`/`getpeername`** return the local or peer address, truncated to the
  supplied length. The full length is reported back.
* **`getsockopt`/`setsockopt`** read and write options (`SOL_SOCKET`, `IPPROTO_TCP`,
  `IPPROTO_IP`, `IPPROTO_IPV6`, ...).
* **`sockatmark`** reports whether the socket is at the out-of-band mark. **`isfdtype(fd,
  S_IFSOCK)`** tests a descriptor's type.

**Failure.** The kernel errors, for example `EADDRINUSE`, `EADDRNOTAVAIL`, `ECONNREFUSED`,
`ETIMEDOUT`, `ENETUNREACH`, `EAFNOSUPPORT`, `EPROTONOSUPPORT`, `EMFILE`, `ENOBUFS`,
`EINPROGRESS`, `EALREADY`, `EISCONN`, `ENOTCONN`, `ENOTSOCK`, `EINTR`.

**Cancellation.** `accept`, `accept4` and `connect` are cancellation points.

**Kernel.** `socket`, `socketpair`, `bind`, `listen`, `accept4`, `connect`, `shutdown`,
`getsockname`, `getpeername`, `getsockopt`, `setsockopt`, `ioctl(SIOCATMARK)`, `fstat`.

⚑ HIST. The `socklen_t *` in-out length parameter conflates "capacity" with "actual size".
Truncation is silent: the caller must compare the lengths.

### 15.2 Data transfer: `send`, `recv`, `sendto`, `recvfrom`, `sendmsg`, `recvmsg`, `sendmmsg`, `recvmmsg`, `__recv_chk`, `__recvfrom_chk`, `__cmsg_nxthdr`, `CMSG_*` (macros) [thin]

**Contract.**

* **`send`/`recv`** are `write`/`read` with flags: `MSG_DONTWAIT`, `MSG_NOSIGNAL`,
  `MSG_PEEK`, `MSG_WAITALL`, `MSG_OOB`, `MSG_TRUNC`, `MSG_MORE`, `MSG_ZEROCOPY`, ...
* **`sendto`/`recvfrom`** also carry an address.
* **`sendmsg`/`recvmsg`** use a message header with scatter/gather buffers, an address, and
  **ancillary data** (control messages). Examples are `SCM_RIGHTS` (passing descriptors),
  `SCM_CREDENTIALS`, `IP_PKTINFO` and timestamps. The `CMSG_*` macros and `__cmsg_nxthdr`
  iterate over ancillary data.
* **`sendmmsg`/`recvmmsg`** transfer several messages per call. `recvmmsg` takes a timeout.

**Guarantees.** Datagram boundaries are preserved. A datagram longer than the buffer is
truncated, which is reported by `MSG_TRUNC` in `msg_flags`.

**Failure.** As §11.3, plus `ECONNRESET`, `EMSGSIZE`, `EDESTADDRREQ`, `ENOTCONN`.

**Cancellation.** Yes.

**Kernel.** `sendto`, `recvfrom` (glibc implements `send` and `recv` through them),
`sendmsg`, `recvmsg`, `sendmmsg`, `recvmmsg`.

⚑ HIST. The ancillary-data encoding requires alignment macros and manual buffer arithmetic.
It is the only way to pass file descriptors between processes.

### 15.3 Address conversion: `inet_pton`, `inet_ntop`, `inet_aton`, `inet_addr`, `inet_ntoa`, `inet_network`, `inet_makeaddr`, `inet_lnaof`, `inet_netof`, `inet_net_pton`, `inet_net_ntop`, `inet_nsap_addr`, `inet_nsap_ntoa`, `inet_neta`, `__inet_aton`...

**Contract.**

* **`inet_pton(af, text, dst)`** parses dotted-quad IPv4 (exactly four decimal parts) or
  IPv6 text (RFC 4291 forms, including embedded IPv4) into network byte order. It returns
  1 on success, 0 for invalid text, and -1 with `EAFNOSUPPORT`.
* **`inet_ntop(af, src, dst, size)`** formats an address, using RFC 5952 compressed form for
  IPv6. It fails with `ENOSPC` if the buffer is too small.
* **`inet_aton`** accepts the historical forms: 1 to 4 parts, each in octal, decimal or hex,
  so `"10.1"` means 10.0.0.1. ⚑ HIST: this lenient syntax has caused security bugs through
  inconsistent parsing between programs.
* **`inet_addr`** is `inet_aton` returning `INADDR_NONE` (0xFFFFFFFF) on error. ⚑ HIST,
  ⚑ ARB: that value is also the valid address 255.255.255.255.
* **`inet_ntoa`** formats into a static per-thread buffer. ⚑ HIST.
* **`inet_network`, `inet_makeaddr`, `inet_lnaof` and `inet_netof`** manipulate classful
  (A/B/C) network and host parts. ⚑ HIST: classful addressing has been obsolete since
  1993 (CIDR).
* **`inet_net_pton`/`inet_net_ntop`** parse and format CIDR prefixes. **`inet_nsap_*`**
  handle OSI NSAP addresses. ⚑ HIST.

**Constants exported as data.** `in6addr_any` (`::`) and `in6addr_loopback` (`::1`) are
`const struct in6_addr` objects. ⚑ HIST: they are data symbols rather than initializer
macros, because C89 could not express a structure constant in an expression.

**Kernel.** None.

### 15.4 Interface enumeration and naming: `if_nametoindex`, `if_indextoname`, `if_nameindex`, `if_freenameindex`, `getifaddrs`, `freeifaddrs`

**Contract.**

* **`if_nametoindex(name)`** returns the interface index, or 0 (with `ENODEV`) if there is no
  such interface. **`if_indextoname(i, buf)`** writes the name into a buffer of
  `IF_NAMESIZE` (16).
* **`if_nameindex()`** returns an allocated array of `{index, name}`, terminated by a
  zero entry.
* **`getifaddrs(&list)`** returns an allocated linked list of per-address records:
  name, flags (`IFF_*`), address, netmask, broadcast or destination address, and
  per-interface statistics (for `AF_PACKET` entries, `ifa_data` holds
  `rtnl_link_stats`).

**Kernel.** `socket(AF_NETLINK, SOCK_RAW, NETLINK_ROUTE)`, `sendto`/`recvmsg`
(`RTM_GETLINK`, `RTM_GETADDR`), `bind`, `getsockname`, and `ioctl(SIOCGIFINDEX, SIOCGIFNAME)`
on an `AF_INET`/`AF_UNIX` datagram socket.

⚑ KERN. `IF_NAMESIZE` = 16 is the kernel's `IFNAMSIZ`.

### 15.5 IPv6 option and routing-header helpers: `inet6_opt_init`, `inet6_opt_append`, `inet6_opt_finish`, `inet6_opt_set_val`, `inet6_opt_next`, `inet6_opt_find`, `inet6_opt_get_val`, `inet6_rth_space`, `inet6_rth_init`, `inet6_rth_add`, `inet6_rth_reverse`, `inet6_rth_segments`, `inet6_rth_getaddr`, `inet6_option_space`, `inet6_option_init`, `inet6_option_append`, `inet6_option_alloc`, `inet6_option_next`, `inet6_option_find`

**Contract.** Build and parse IPv6 hop-by-hop and destination options and type-0 routing
headers in caller buffers (RFC 3542). The `inet6_option_*` functions are the deprecated
RFC 2292 forms. ⚑ HIST.

**Kernel.** None.

### 15.6 Source-address and multicast filters: `getipv4sourcefilter`, `setipv4sourcefilter`, `getsourcefilter`, `setsourcefilter`

**Contract.** Get or set an include or exclude source list for a multicast group on an
interface (RFC 3678).

**Kernel.** `getsockopt`/`setsockopt` (`IP_MSFILTER`, `MCAST_MSFILTER`).

### 15.7 Ethernet addresses: `ether_aton`, `ether_aton_r`, `ether_ntoa`, `ether_ntoa_r`, `ether_hostton`, `ether_ntohost`, `ether_line`

**Contract.** Parse and format 48-bit MAC addresses (`xx:xx:xx:xx:xx:xx`). The non-`_r` forms
use static storage (⚑ PATH). `ether_hostton` and `ether_ntohost` map between host names and
MACs via the NSS `ethers` database. `ether_line` parses one line of `/etc/ethers`.

**Kernel.** None. **Config:** NSS `ethers` (`/etc/ethers`).

### 15.8 BSD remote execution: `rcmd`, `rcmd_af`, `rresvport`, `rresvport_af`, `ruserok`, `ruserok_af`, `iruserok`, `iruserok_af`, `rexec`, `rexec_af`, `rexecoptions`, `ruserpass` (compat), `__ivaliduser`, `__check_rhosts_file`, `__rcmd_errstr`, `bindresvport`

**Contract.**

* **`rcmd`** connects to an `rshd` from a privileged port (below 1024) and sends the user
  names and command, with an optional separate stderr channel.
* **`rresvport`/`bindresvport`** bind a privileged port.
* **`ruserok`/`iruserok`** check `~/.rhosts` and `/etc/hosts.equiv` trust.
* **`rexec`** authenticates with a password sent in clear text.

**Kernel.** `socket`, `bind`, `connect`, `listen`, `accept`, `read`, `write`, `poll`.
**Config:** `/etc/hosts.equiv`, `~/.rhosts`, `~/.netrc`.

⚑ HIST. The security model is address-based trust from privileged ports (1983). It is
obsolete and insecure, and kept only for ABI.

---

## 16. Name services and databases

### 16.1 The Name Service Switch (NSS): `nsswitch.conf` semantics, `__nss_configure_lookup`, `__nss_database_lookup` (compat), `__nss_next`, `__nss_hostname_digits_dots`, `__nss_*_lookup` (compat)

**Contract.** Every lookup in a system database is routed, per database, through an ordered
list of *sources* (services), with rules for what to do on each outcome. The databases are
`passwd`, `group`, `shadow`, `gshadow`, `hosts`, `networks`, `protocols`, `services`,
`rpc`, `ethers`, `netgroup`, `aliases`, `publickey` and `initgroups`.

* **Sources** are `files`, `dns`, `db`, `compat`, and plugin modules such as `systemd`, `sss`,
  `ldap` and `winbind`.
* **Outcome rules** are `[NOTFOUND=return]` style actions: `SUCCESS`, `NOTFOUND`, `UNAVAIL`
  and `TRYAGAIN`, each mapped to `return`, `continue` or `merge`.
* **`__nss_hostname_digits_dots(name, resbuf, buffer, buffer_size, buflen, result, status, af,
  h_errnop)`** (compat, no header declares it; NSS host modules call it) answers a host lookup
  for a numeric name without the switch: `int` result; `const char *name`, `struct hostent
  *resbuf`, `char **buffer`, `size_t *buffer_size`, `size_t buflen`, `struct hostent **result`,
  `enum nss_status *status`, `int af`, `int *h_errnop`. It returns 1 when `name` is a dotted
  numeric IPv4 text (only digits and dots, not ending in a dot, as `inet_aton` reads it:
  `1.2.3` is 1.2.0.3) or an IPv6 literal (hexadecimal digits, colons and dots), whether or not
  it can be returned, and 0 for any other name, writing nothing.
* The found entry is `resbuf`: `h_name` the name as given, no aliases, one address of family
  `af` (`AF_INET6`; any other value is `AF_INET`). A name of the other family, or one the
  parser rejects (`1.2.3.256`, five parts), is not found (`HOST_NOT_FOUND`).
* **The allocating form** (`buffer_size` not NULL): the strings and lists go in `*buffer`,
  enlarged with `realloc` to fit (`*buffer`, `*buffer_size` updated; done for any non-empty
  name, also one answered with 0); `*result` is `resbuf`, or
  NULL when not found; `*status` is not written. **The caller-buffer form** (`buffer_size`
  NULL): `buflen` bytes at `*buffer`; `*status` is `NSS_STATUS_SUCCESS` (1) or
  `NSS_STATUS_NOTFOUND` (0); `*result` is not written. `*h_errnop` is 0 or `HOST_NOT_FOUND`.
* **Failure.** A caller buffer too small: 1, `*status` `NSS_STATUS_TRYAGAIN` (-2), `*h_errnop`
  `NETDB_INTERNAL` (-1), `errno` `ERANGE`. No memory in the allocating form: 1, `*result` NULL,
  `NETDB_INTERNAL`, `ENOMEM`. A NULL `status` or `h_errnop` is not written (the reference
  dereferences it; S6). `errno` is not otherwise written. (Written 2026-10-05, owner ruling:
  observed black-box, `spec/behaviour/digits-dots.md`.)

The switch is configured by `/etc/nsswitch.conf`, and each source is a shared object
`libnss_<source>.so.2` loaded with `dlopen`. `__nss_configure_lookup(db, spec)` overrides the
configuration programmatically.

**Guarantees.**

* A lookup returns the first successful answer under the rules. `merge` concatenates group
  members across sources.
* The configuration is reloaded when the file changes (2.33+).
* `nscd`, if running and enabled, answers from its cache over a Unix socket first.

**Kernel.** `openat`, `read`, `fstat`, `mmap` (config, `files` sources, `dlopen`),
`socket(AF_UNIX)`, `connect`, `sendmsg`/`recvmsg` (nscd, including `SCM_RIGHTS` for its
shared-memory cache), `mmap` (nscd cache). **Config:** `/etc/nsswitch.conf`, the database
files, `/var/run/nscd/socket`.

⚑ HIST. Plugin loading through `dlopen` means that the name lookups of a statically linked
program still depend on the installed shared libraries. A lookup can also load arbitrary
code (the `plugin` safety qualifier on every NSS function).
⚑ HIST. The source-selection rules are a small configuration language, and its semantics are
defined only by glibc's implementation and manual.

### 16.2 Host and address resolution: `getaddrinfo`, `freeaddrinfo`, `gai_strerror`, `getnameinfo`, `getaddrinfo_a`, `gai_suspend`, `gai_error`, `gai_cancel` (`libanl`, merged into libc in 2.34)

**Contract.**

* **`getaddrinfo(node, service, hints, &res)`** translates a host name (or numeric address)
  and a service name (or port number) into a list of socket addresses suitable for `socket`
  plus `bind` or `connect`.
* **Hints** select the family (`AF_UNSPEC`, `AF_INET`, `AF_INET6`), the socket type and the
  protocol. Flags are:
  * `AI_PASSIVE` (wildcard address for `bind`);
  * `AI_CANONNAME`;
  * `AI_NUMERICHOST`, `AI_NUMERICSERV`;
  * `AI_V4MAPPED`, `AI_ALL`;
  * `AI_ADDRCONFIG` (return only families configured on the host);
  * `AI_IDN` (IDNA via `libidn2`, loaded with `dlopen`), `AI_CANONIDN`.
* The result list is **sorted by RFC 3484/6724 destination address selection**. That needs
  the source address for each candidate, which glibc learns by `connect`ing a UDP socket and
  calling `getsockname`. The policy table comes from `/etc/gai.conf`.
* **`getnameinfo(sa, len, host, hl, serv, sl, flags)`** does the reverse. Flags are
  `NI_NUMERICHOST`, `NI_NUMERICSERV`, `NI_NOFQDN`, `NI_NAMEREQD`, `NI_DGRAM` and `NI_IDN`.
* **`getaddrinfo_a(mode, list, n, sigevent)`** runs lookups asynchronously on helper threads.
  `gai_suspend`, `gai_error` and `gai_cancel` poll, wait and cancel.

**Failure.** Error codes are returned directly, not through `errno`: `EAI_AGAIN`,
`EAI_BADFLAGS`, `EAI_FAIL`, `EAI_FAMILY`, `EAI_MEMORY`, `EAI_NONAME`, `EAI_SERVICE`,
`EAI_SOCKTYPE`, `EAI_SYSTEM` (then `errno` is meaningful), `EAI_OVERFLOW`, `EAI_NODATA`,
`EAI_ADDRFAMILY`, `EAI_INPROGRESS`, `EAI_CANCELED`, `EAI_NOTCANCELED`, `EAI_ALLDONE`,
`EAI_INTR`, `EAI_IDN_ENCODE`. **`gai_strerror`** maps them to Text.

**Guarantees.** The result is allocated as a single list that must be freed with
`freeaddrinfo`. Each entry is ready to pass to `socket` and `connect`.

**Kernel.**

* NSS `hosts` (§16.1). The `dns` source uses §16.4.
* `socket(AF_NETLINK)` with `RTM_GETADDR` (for `AI_ADDRCONFIG`), and
  `socket(AF_INET/AF_INET6, SOCK_DGRAM)` + `connect` + `getsockname` (for sorting).
* Threads (`clone3`) for `getaddrinfo_a`.

**Config:** `/etc/hosts`, `/etc/gai.conf`, `/etc/resolv.conf`, `/etc/services`,
`/etc/host.conf`.

⚑ PATH. Error codes of a *third* kind (neither -1/`errno` nor a returned errno value), with
a separate string function.

### 16.3 Legacy host and network lookups: `gethostbyname`, `gethostbyname2`, `gethostbyaddr`, `gethostent`, `sethostent`, `endhostent`, their `_r` forms, `getipnodebyname`/`getipnodebyaddr`/`freehostent` (compat), `getnetbyname`, `getnetbyaddr`, `getnetent`, `setnetent`, `endnetent` and `_r` forms, `h_errno`, `__h_errno_location`, `herror`, `hstrerror`, `h_errlist`, `h_nerr` (compat), `_gethtbyname`... (compat)

**Contract.** Look up host entries (`struct hostent`: name, aliases, address type, length and
list) by name or address, or enumerate the hosts database. The same applies to networks
(`struct netent`).

**Failure.** `NULL`, with the error in **`h_errno`** (per-thread): `HOST_NOT_FOUND`,
`TRY_AGAIN`, `NO_RECOVERY`, `NO_DATA`. `herror` and `hstrerror` produce messages.

⚑ HIST. The result is in static storage (hence the `_r` forms, whose return conventions
differ from other `_r` functions: they return an errno value and also fill an `h_errno`
pointer). A second error channel, `h_errno`, exists alongside `errno`. The lookup is
IPv4-centric: `gethostbyname2` and then `getaddrinfo` were added for IPv6 (⚑ PATH).
Enumeration (`gethostent`) makes sense only for the `files` source.

### 16.4 DNS stub resolver: `res_init`, `res_ninit`, `res_nclose`, `res_query`, `res_nquery`, `res_search`, `res_nsearch`, `res_querydomain`, `res_nquerydomain`, `res_mkquery`, `res_nmkquery`, `res_send`, `res_nsend`, `dn_comp`, `dn_expand`, `dn_skipname`, `res_hnok`, `res_ownok`, `res_mailok`, `res_dnok`, `__res_state`, `_res` (data), `__res_*`, `ns_initparse`, `ns_parserr`, `ns_skiprr`, `ns_sprintrr`, `ns_name_*`, `ns_get16`, `ns_put32`, `ns_samename`, `ns_makecanon`, `ns_datetosecs`, `ns_format_ttl`, `ns_msg_getflag`, `ns_parse_ttl`, `ns_sprintrrf`, `ns_subdomain`, `__p_*`, `__fp_*`, `__sym_*`, `__b64_*`, `__hostalias`, `__loc_*`, `__dn_*`, `_getlong`, `_getshort`, `__putlong`, `__putshort`, `inet_neta` (`libresolv` and libc)

**Contract.** A DNS client that sends queries to the configured recursive servers and returns
raw DNS messages.

* **Initialization.** `res_ninit(&state)` reads the configuration:
  * nameservers (at most `MAXNS` = 3, ⚑ ARB);
  * the search list (every domain of `search` is used by the lookups, 2.26+; the state's
    `dnsrch` array shows at most the first `MAXDNSRCH` = 6, ⚑ ARB);
  * options: `ndots`, `timeout`, `attempts`, `rotate`, `edns0`, `single-request`,
    `single-request-reopen`, `use-vc`, `no-aaaa`, `trust-ad`, `no-reload`, `inet6`
    (obsolete);
  * the sortlist.
* **Querying.** `res_nquery(state, name, class, type, answer, anslen)` sends one query.
  `res_nsearch` applies the search list and `ndots` rules. `res_nsend` sends a prebuilt
  message.
* **Message handling.** `res_nmkquery` builds a query. `dn_comp`/`dn_expand` compress and
  expand domain names. The `ns_*` functions parse and print resource records.
* **Name checks.** `res_hnok` and the others check host-name syntax.

**Guarantees.**

* Queries are retried across servers with the configured timeouts.
* Responses are validated: the ID, source address and question must match.
* The AD bit is passed through only with `trust-ad`.
* The configuration is re-read automatically when `/etc/resolv.conf` changes (2.26+),
  unless `no-reload` is set.

**Failure.** -1 with `h_errno` (`HOST_NOT_FOUND`, `TRY_AGAIN`, `NO_DATA`, `NO_RECOVERY`).

**Kernel.** `socket(AF_INET/AF_INET6, SOCK_DGRAM|SOCK_STREAM)`, `connect`, `sendto`/
`sendmmsg`, `recvfrom`, `poll`, `close`, `newfstatat` (config change detection),
`clock_gettime`. **Config:** `/etc/resolv.conf`, `RES_OPTIONS`, `LOCALDOMAIN`,
`HOSTALIASES`.

⚑ HIST. The resolver state `_res` is a structure exported as data (per-thread, via
`__res_state()`). Its fixed-size arrays (3 servers, 6 search domains) are ABI, so the limits
cannot be raised for what the structure shows; the lookups themselves use the whole search list.

### 16.5 Services, protocols, RPC programs and netgroups: `getservbyname`, `getservbyport`, `getservent`, `setservent`, `endservent`, `getprotobyname`, `getprotobynumber`, `getprotoent`, `setprotoent`, `endprotoent`, `getrpcbyname`, `getrpcbynumber`, `getrpcent`, `setrpcent`, `endrpcent` (all with `_r` forms), `setnetgrent`, `getnetgrent`, `getnetgrent_r`, `endnetgrent`, `innetgr`

**Contract.** Look up a service (name ↔ port and protocol), a protocol (name ↔ number), or an
ONC RPC program (name ↔ number), or enumerate these databases. For **netgroups**, enumerate
the `(host, user, domain)` triples of a netgroup, or test membership (`innetgr`).

**Failure.** `NULL` on failure. The `_r` forms return an errno value, where `ERANGE` means
"buffer too small".

**Kernel.** Via NSS (§16.1). **Config:** `/etc/services`, `/etc/protocols`, `/etc/rpc`,
`/etc/netgroup`.

⚑ HIST. `getservbyport` takes the port in **network** byte order inside an `int`.

### 16.6 Mail aliases: `setaliasent`, `getaliasent`, `getaliasent_r`, `getaliasbyname`, `getaliasbyname_r`, `endaliasent`

**Contract.** Enumerate or look up entries of the NSS `aliases` database (`struct aliasent`:
name, members, local flag).

**Config:** `/etc/aliases`.

⚑ HIST. These exist to support MTAs.

---

## 17. Users, groups, credentials and login accounting

### 17.1 Querying credentials: `getuid`, `geteuid`, `getgid`, `getegid`, `getresuid`, `getresgid`, `getgroups`, `__getgroups_chk`, `group_member` [thin]

**Contract.** Return the calling process's real, effective and saved user and group IDs, and
its supplementary group list. `getgroups(0, NULL)` returns the count. `group_member(g)` (GNU)
tests whether `g` is the effective GID or a supplementary group.

**Guarantees.** These calls never fail, except `getgroups` with `EINVAL` when the buffer is
too small.

**Kernel.** `getuid`, `geteuid`, `getgid`, `getegid`, `getresuid`, `getresgid`, `getgroups`.

### 17.2 Changing credentials: `setuid`, `seteuid`, `setreuid`, `setresuid`, `setgid`, `setegid`, `setregid`, `setresgid`, `setgroups`, `setfsuid`, `setfsgid`, `initgroups`

**Contract.** Change the process's user or group identities, following the POSIX rules:

* A privileged process (one with `CAP_SETUID`/`CAP_SETGID`) may set any values.
* An unprivileged process may only switch among its real, effective and saved IDs.
* `setuid` with privilege sets all three.

Also:

* **`setgroups`** replaces the supplementary list. It requires privilege, and is denied in a
  user namespace when `/proc/self/setgroups` is `deny`.
* **`initgroups(user, g)`** sets the supplementary list to all groups that `user` belongs to,
  plus `g`.
* **`setfsuid`/`setfsgid`** set the file-system IDs used for access checks. They return the
  previous value and *cannot report failure*. ⚑ HIST.

**Guarantees.** **The change applies to every thread in the process atomically**, as POSIX
requires.

**Failure.** `EPERM`, `EINVAL`, `EAGAIN` (`RLIMIT_NPROC` on some kernels).

**Kernel.** `setuid`, `setresuid`, `setreuid`, `setgid`, `setresgid`, `setregid`,
`setgroups`, `setfsuid`, `setfsgid`. For multi-threaded processes, also `tgkill` and
`rt_sigqueueinfo` with the internal real-time signal **`SIGSETXID`** (`SIGRTMIN+1`), plus
`futex`.

⚑ KERN, ⚑ HIST (high value). **Linux credentials are per-thread**: the syscalls change only
the calling thread. POSIX requires process-wide semantics, so glibc must interrupt every other
thread with an internal signal and have each thread perform the same syscall. This is the
single largest mismatch between the POSIX contract and the kernel primitive. It is why glibc
reserves two real-time signals (`SIGCANCEL` and `SIGSETXID`), so `SIGRTMIN` is 34, not 32. It
also means a `setuid` can partially fail, which glibc treats as fatal. Any reimplementation
must replicate this broadcast, or define per-thread semantics explicitly.

### 17.3 The user and group databases: `getpwnam`, `getpwuid`, `getpwent`, `setpwent`, `endpwent`, `fgetpwent`, `putpwent`, `getpw`, `getpwnam_r`, `getpwuid_r`, `getpwent_r`, `fgetpwent_r`, `getgrnam`, `getgrgid`, `getgrent`, `setgrent`, `endgrent`, `fgetgrent`, `putgrent`, `getgrnam_r`, `getgrgid_r`, `getgrent_r`, `fgetgrent_r`, `getgrouplist`

**Contract.**

* **Look up a user record** (`struct passwd`: name, password placeholder, UID, GID, GECOS,
  home directory, shell) by name or by UID. Or enumerate all records, reading from NSS.
* **Look up a group record** (`struct group`: name, password, GID, member-name list) in the
  same way.
* **The `f` forms** parse records from a caller-supplied stream in `/etc/passwd` format.
  **`putpwent`/`putgrent`** write one.
* **`getgrouplist(user, g, groups, &n)`** lists the GIDs of all groups the user belongs to.
  If the array is too small, it returns -1 and sets `*n` to the needed size.
* **`getpw(uid, buf)`** writes a passwd line into an unbounded buffer. ⚑ HIST.

**Guarantees.** Results come from NSS (§16.1). The non-`_r` forms return static storage
(⚑ PATH: hence the `_r` forms).

**Failure.**

* **Non-`_r` forms** return `NULL`. "Not found" is *not* an error: `errno` is left unchanged,
  or set to 0, `ENOENT`, `ESRCH`, `EBADF` or `EPERM` depending on the source.
  ⚑ HIST: POSIX allows several values here, so "not found" and "failed" are hard to tell
  apart.
* **`_r` forms** return 0 with `*result == NULL` for not found, or an errno value (`ERANGE`
  means the caller buffer is too small; `sysconf(_SC_GETPW_R_SIZE_MAX)` gives only a hint).
  ⚑ HIST: the "guess a buffer size, retry on ERANGE" protocol.
* **End of an enumeration** (`getpwent`, `getgrent`, `getspent`, `getsgent`): the non-`_r` forms
  return `NULL` with `errno` unchanged; the `_r` forms return `ENOENT`, `errno` unchanged.

**Kernel.** As for §16.1. **Config:** `/etc/passwd`, `/etc/group`.

⚑ HIST. The enumeration interface (`setpwent`/`getpwent`/`endpwent`) holds a hidden
process-global cursor.

### 17.4 Shadow password and group databases: `getspnam`, `getspnam_r`, `getspent`, `getspent_r`, `setspent`, `endspent`, `fgetspent`, `fgetspent_r`, `sgetspent`, `sgetspent_r`, `putspent`, `lckpwdf`, `ulckpwdf`, `getsgnam`, `getsgnam_r`, `getsgent`, `getsgent_r`, `setsgent`, `endsgent`, `fgetsgent`, `fgetsgent_r`, `sgetsgent`, `sgetsgent_r`, `putsgent`

**Contract.** The same pattern as §17.3, for `struct spwd` (the password hash and aging fields:
last change, min, max, warn, inactive, expire) and `struct sgrp` (the group password, admins
and members). The `s` forms parse one Text line.

**`lckpwdf`/`ulckpwdf`** take and release an advisory lock on `/etc/.pwd.lock`, with a 15 s
timeout, to coordinate editors of the password files. It requires privilege.

**Kernel.** `openat`, `fcntl(F_SETLKW)`, `alarm`/`rt_sigaction` (the lock timeout).
**Config:** `/etc/shadow`, `/etc/gshadow`.

⚑ HIST. The lock timeout is implemented by a process-wide `SIGALRM` handler. ⚑ ARB: the
15-second value is fixed.

### 17.5 Login name and terminal session: `getlogin`, `getlogin_r`, `__getlogin_r_chk`, `setlogin` (not on Linux), `ttyslot`, `getpass`

**Contract.**

* **`getlogin()`** returns the name of the user logged in on the process's controlling
  session. glibc reads `/proc/self/loginuid` and maps it through the passwd database. If the
  audit login UID is unset, it falls back to the `utmp` entry for the controlling terminal.
* **`ttyslot()`** returns the index of the current terminal's entry in the terminal table.
  ⚑ HIST.
* **`getpass(prompt)`** opens `/dev/tty` (or uses stdin and stderr), disables echo, reads one
  line into a static buffer, and restores the terminal. ⚑ HIST: it is obsolete and the buffer
  is static. glibc puts no limit on the line's length (getpass(3), NOTES).

**Failure.** `getlogin` returns `NULL` with `ENXIO` or `ENOENT` when there is no login session.

**Kernel.** `openat("/proc/self/loginuid")`, `read`, `ttyname` (§19.3), and `ioctl`
(`TCGETS`/`TCSETS`) for `getpass`. **Config:** `utmp`, `passwd`.

### 17.5a Valid login shells: `getusershell`, `setusershell`, `endusershell`

**Contract.** Enumerate the shells listed in `/etc/shells`. If the file is absent, the list is
`/bin/sh` and `/bin/csh`. Programs such as `chsh` and `ftpd` use it to decide whether a user
has a "real" shell. The cursor is process-global.

**Kernel.** `openat`, `read`, `fstat`. **Config:** `/etc/shells`.

### 17.6 Login accounting: `setutent`, `getutent`, `getutent_r`, `getutid`, `getutid_r`, `getutline`, `getutline_r`, `pututline`, `endutent`, `utmpname`, `updwtmp`, `logwtmp`, `login`, `logout`, `login_tty` (§19), `setutxent`, `getutxent`, `getutxid`, `getutxline`, `pututxline`, `endutxent`, `utmpxname`, `updwtmpx`, `getutmp`, `getutmpx`, `__updwtmp`...

**Contract.** Read and write the login records database:

* `utmp`: current sessions, in `/var/run/utmp`;
* `wtmp`: login and logout history, in `/var/log/wtmp`;
* `btmp`: failed logins.

The records are `struct utmp`/`utmpx`: type (`USER_PROCESS`, `DEAD_PROCESS`, `LOGIN_PROCESS`,
`BOOT_TIME`, ...), PID, line, ID, user, host, exit status, session, time and address.

* **`getutid`/`getutline`** search by type and ID, or by line. **`pututline`** writes the
  record, replacing the matching one.
* **`login`/`logout`/`logwtmp`** (BSD, from `libutil`) are convenience wrappers.
* **`utmpname`** selects a different file.

**Guarantees.** Record updates are serialized with `fcntl` locks, with a 10-second timeout (⚑ ARB).

**Kernel.** `openat`, `read`, `write`, `lseek`, `fcntl(F_SETLKW)`, `alarm`/`setitimer` (the
lock timeout). **Config:** `/var/run/utmp`, `/var/log/wtmp`.

⚑ HIST, ⚑ ARB. Fixed-width fields: user 32 bytes, line 32, host 256, ID 4. On x86_64,
`ut_tv` uses **32-bit** seconds for compatibility with 32-bit binaries sharing the file
(`__WORDSIZE_TIME64_COMPAT32`), so these records overflow in 2038. The database is an
unstructured binary file of fixed-size records, which systemd-based systems have largely
replaced. `utmp` and `utmpx` are identical on glibc but duplicated for SVID/XPG compatibility.

---

## 18. Processes

### 18.1 Process identity: `getpid`, `getppid`, `gettid` [thin]

**Contract.** Return the calling process's ID (its thread-group ID), its parent's ID, or the
calling thread's kernel ID. These calls never fail.

**Kernel.** `getpid`, `getppid`, `gettid`. glibc stopped caching `getpid` in 2.25, and caches
the TID in the thread control block.

⚑ KERN. The PID of the parent changes when the parent exits (reparenting to init or to a
subreaper). `getppid` is a snapshot.

### 18.2 Process creation: `fork`, `_Fork`, `__fork`, `vfork`, `__vfork`, `clone`, `__clone`, `__register_atfork`, `pthread_atfork`

**Contract.**

* **`fork()`** creates a child process that is a copy of the caller. Copied: memory
  (copy-on-write), descriptors (sharing descriptions), the signal mask and dispositions, the
  cwd, the umask, limits, and so on. Returns: 0 in the child, the child's PID in the parent.
  * Only the calling thread exists in the child.
  * `pthread_atfork` handlers run: *prepare* in reverse registration order before the fork,
    and *parent* and *child* in registration order after it.
  * glibc makes its own internal state (malloc, stdio lists, dynamic-linker locks) consistent
    in the child.
* **`_Fork()`** (2.34, POSIX 2024) forks without running `atfork` handlers and without
  internal lock handling. It is async-signal-safe.
* **`vfork()`** creates a child that shares the parent's memory. The parent is suspended until
  the child calls `execve` or `_exit`. The child may do nothing else. ⚑ HIST: `vfork` is an
  optimization that predates copy-on-write, and it is dangerous to use correctly.
  `posix_spawn` is the safe interface to the same primitive.
* **`clone(fn, stack, flags, arg, [ptid, tls, ctid])`** is the Linux primitive. It creates a
  task that shares selected resources (`CLONE_VM`, `CLONE_FS`, `CLONE_FILES`,
  `CLONE_SIGHAND`, `CLONE_THREAD`, the namespaces, `CLONE_PIDFD`, `CLONE_SETTLS`, ...) and
  runs `fn(arg)` on the given stack. The child's return value from `fn` is its exit status.
  There is no public `clone3` wrapper in 2.39.

**Failure.** -1 with `EAGAIN` (`RLIMIT_NPROC`, or the kernel limit) or `ENOMEM`.

**Guarantees (fork in a threaded program).** The child may call only async-signal-safe
functions until it calls `exec`, per POSIX. glibc goes further: `malloc` works in the child.

**Kernel.** `clone` (with `SIGCHLD`, `CLONE_CHILD_SETTID`, `CLONE_CHILD_CLEARTID`),
`clone3`, `set_robust_list` (child), `madvise` / `MADV_WIPEONFORK` (not used in 2.39).

⚑ HIST. `fork` in a threaded process is fundamentally unsafe, because it copies locks held by
threads that no longer exist. `atfork` handlers are the patch (⚑ PATH). The functional needs
behind `fork` are usually "run another program" (`posix_spawn`) or "snapshot the process"
(rare).

### 18.3 Program execution: `execve`, `fexecve`, `execveat`, `execv`, `execvp`, `execvpe`, `execl`, `execle`, `execlp` [execve is thin]

**Contract.** Replace the process image with a new program, with the argument list and
environment given. The PID, descriptors not marked close-on-exec, the signal mask, ignored
signals (caught ones are reset to default), the cwd, limits and credentials are all
preserved. Set-user-ID and file capabilities are applied.

* **The `p` forms** search `$PATH` (the default is `/bin:/usr/bin`) for a name without `/`.
  If the file exists but is not a valid executable (`ENOEXEC`), they run it with `/bin/sh`.
  They continue past `EACCES` and report `EACCES` only if no later entry succeeds.
* **The `l` forms** take a variadic, NULL-terminated argument list.
* **The `e` forms** take an explicit environment. The other forms use `environ`.
* **`fexecve(fd, ...)`** executes an open file, via `execveat(fd, "", AT_EMPTY_PATH)`, or
  `/proc/self/fd/N` on older kernels.
* **`execveat`** (2.34) is a thin wrapper.

**Guarantees.** They do not return on success.

**Failure.** `E2BIG` (argv plus envp exceed the kernel limit, a quarter of `RLIMIT_STACK`, or
`MAX_ARG_STRLEN` = 32 pages per string), `EACCES`, `ENOENT`, `ENOEXEC`, `ETXTBSY`, `ENOMEM`,
`ELOOP`, `EPERM`.

**Kernel.** `execve`, `execveat`.

⚑ KERN. The argv and envp layout (§1.1).
⚑ HIST. Six exec variants encode three independent options in their names (`l` versus `v`,
`p`, `e`).

### 18.4 Spawning: `posix_spawn`, `posix_spawnp`, `pidfd_spawn`, `pidfd_spawnp`, `posix_spawnattr_init`, `posix_spawnattr_destroy`, `posix_spawnattr_{get,set}{flags,pgroup,sigmask,sigdefault,schedpolicy,schedparam}`, `posix_spawnattr_{get,set}cgroup_np`, `posix_spawn_file_actions_init`, `posix_spawn_file_actions_destroy`, `posix_spawn_file_actions_addopen`, `posix_spawn_file_actions_addclose`, `posix_spawn_file_actions_adddup2`, `posix_spawn_file_actions_addchdir_np`, `posix_spawn_file_actions_addfchdir_np`, `posix_spawn_file_actions_addclosefrom_np`, `posix_spawn_file_actions_addtcsetpgrp_np`

**Contract.** Create a child process running a new program, applying a list of descriptor
actions and attributes in the child before `exec`, and report the child's PID (or, for
`pidfd_spawn`, a pidfd).

* **File actions**, applied in order: open, close, dup2 (with `fd == newfd`, it clears
  `FD_CLOEXEC`), chdir, fchdir, closefrom, and tcsetpgrp.
* **Attribute flags:**
  * `POSIX_SPAWN_RESETIDS`: use the real IDs;
  * `POSIX_SPAWN_SETPGROUP`, `POSIX_SPAWN_SETSID`;
  * `POSIX_SPAWN_SETSIGMASK`, `POSIX_SPAWN_SETSIGDEF`: reset listed signals to default;
  * `POSIX_SPAWN_SETSCHEDULER`, `POSIX_SPAWN_SETSCHEDPARAM`;
  * `POSIX_SPAWN_USEVFORK` (ignored);
  * `POSIX_SPAWN_SETCGROUP` (2.39, Linux 5.7+).

**Guarantees.**

* Returns 0 on success, or an error number. **Errors that occur in the child before `exec`
  succeeds are reported to the parent as the return value**, not as a child exit status.
  glibc has guaranteed this since 2.24.
* Signal handlers do not run in the child between clone and exec: all signals are blocked,
  and caught signals are reset.
* It is safe in multi-threaded programs.

**Failure.** The error numbers of the failing action, `fork` or `exec`.

**Kernel.** `clone3` (`CLONE_VM|CLONE_VFORK|CLONE_CLEAR_SIGHAND`, `CLONE_INTO_CGROUP`,
`CLONE_PIDFD`), falling back to `clone(CLONE_VM|CLONE_VFORK)`. Then `mmap` (the child stack),
`rt_sigprocmask`, `rt_sigaction`, `openat`, `close`, `close_range`, `dup2`/`dup3`, `fcntl`,
`chdir`/`fchdir`, `ioctl(TIOCSPGRP)`, `setpgid`, `setsid`, `sched_setscheduler`/
`sched_setparam`, `setresuid`/`setresgid`, `execve`, `exit_group`.

⚑ PATH. The `_np` actions were added one at a time as programs found needs that the
fixed-function action list could not express. The underlying need is "run this setup
program in the child", which a restricted, pre-vetted action list approximates.

### 18.5 Process descriptors: `pidfd_open`, `pidfd_getfd`, `pidfd_send_signal`, `pidfd_getpid` [thin]

**Contract.**

* **`pidfd_open(pid, flags)`** returns a descriptor that refers stably to a process. It
  becomes readable (for `poll`) when the process exits. `flags` may be `PIDFD_NONBLOCK`.
* **`pidfd_getfd(pidfd, targetfd, 0)`** duplicates a descriptor out of another process.
* **`pidfd_send_signal`** sends a signal without PID-reuse races.
* **`pidfd_getpid(pidfd)`** (2.39) returns the PID, parsed from `/proc/self/fdinfo/N`.

**Failure.** `ESRCH` (the process is gone), `EPERM`, `EINVAL`.

**Kernel.** `pidfd_open`, `pidfd_getfd`, `pidfd_send_signal`, and `openat`/`read` on
`/proc/self/fdinfo`.

⚑ HIST. PIDs are reusable integers, which makes every PID-based operation racy. Pidfds are
the fix (⚑ PATH).

### 18.6 Waiting for children: `wait`, `waitpid`, `waitid`, `wait3`, `wait4`, `WIFEXITED`... (macros)

**Contract.** Wait for a state change of a child and collect its status.

* The state changes are: exited, killed by a signal, stopped, or continued (`WUNTRACED`,
  `WCONTINUED`).
* **`waitpid(pid, &status, options)`** selects by PID, process group (`-pgid` or 0), or any
  child (-1).
* **`waitid(idtype, id, &info, options)`** returns a `siginfo_t`; `si_status` carries 8 bits of exit status.
  `idtype` may be `P_PID`, `P_PGID`, `P_ALL` or `P_PIDFD`, and options include `WEXITED`,
  `WSTOPPED`, `WCONTINUED` and `WNOWAIT`.
* **`wait3`/`wait4`** also return the child's resource usage.

**Guarantees.** Status macros decode the value: `WEXITSTATUS` gives the 8-bit exit code,
`WTERMSIG` the signal, and `WCOREDUMP`, `WSTOPSIG` and `WIFCONTINUED` the rest. A reaped child
no longer exists as a zombie.

**Failure.** `ECHILD` (no matching children, or `SIGCHLD` is ignored so children are
auto-reaped), `EINTR`, `EINVAL`.

**Cancellation.** Yes.

**Kernel.** `wait4`, `waitid`.

⚑ HIST. Status is packed into an `int` whose layout is defined only by macros. It carries only
8 bits of exit status (§1.2). `waitid` reports the exit status in `si_status`, but the kernel
gives it only the same low 8 bits (`_exit(300)` reads back as 44); the library passes on what
the kernel gives. Corrected 2026-10-03 (owner) from black-box observation,
`spec/behaviour/process.md`: an earlier text said `waitid` exposed the full 32-bit value.

### 18.7 Running shell commands: `system`, `popen`, `pclose`

**Contract.**

* **`system(cmd)`** runs `/bin/sh -c cmd`. While it waits, it blocks `SIGCHLD` and ignores
  `SIGINT` and `SIGQUIT` in the caller. It returns the wait status, or -1 on failure. If the
  shell could not be executed, the status is exit code 127. `system(NULL)` reports whether a
  shell is available.
* **`popen(cmd, "r"|"w"|"re"|"we")`** runs the command with a pipe connected to its stdout
  or stdin, and returns a stream. **`pclose`** closes the stream, waits for the child, and
  returns its status.

**Threads.** `system` is MT-Safe in 2.39. Concurrent calls share the `SIGINT` and
`SIGQUIT` disposition changes, which are reference-counted.

**Kernel.** As for `posix_spawn` (§18.4, which glibc uses for both), `pipe2`, `wait4`,
`rt_sigaction`, `rt_sigprocmask`.

⚑ HIST. Passing a command line to a shell makes injection the default failure mode. The need
"run program P with arguments A" is met by `posix_spawn`.
⚑ HIST. `system`'s manipulation of process-wide signal dispositions is necessary for POSIX
semantics, but interferes with other threads.

### 18.8 Process groups and sessions: `getpgid`, `setpgid`, `getpgrp`, `setpgrp`, `__bsd_getpgrp` (compat), `getsid`, `setsid`, `tcgetpgrp`/`tcsetpgrp` (§19), `daemon` [thin except daemon]

**Contract.**

* **`setpgid(pid, pgid)`** moves a process into a new or existing group in the same session.
* **`setsid()`** creates a new session and group with the caller as leader, with no
  controlling terminal. It fails with `EPERM` if the caller is already a group leader.
* **`getsid`, `getpgid` and `getpgrp`** query.
* **`setpgrp()`** is `setpgid(0, 0)`. ⚑ HIST: the SVID and BSD versions differ, and
  `__bsd_getpgrp` is the compat symbol for the BSD form.
* **`daemon(nochdir, noclose)`** forks and the parent exits. The child calls `setsid`, changes
  to `/` unless `nochdir`, and redirects 0–2 to `/dev/null` unless `noclose`. It does
  **not** fork a second time, so the daemon may reacquire a controlling terminal. ⚑ HIST.

**Kernel.** `setpgid`, `setsid`, `getsid`, `getpgid`, and `clone`, `exit_group`, `chdir`,
`openat`, `dup2` for `daemon`.

### 18.9 Process attributes and namespaces: `prctl`, `personality`, `unshare`, `setns`, `arch_prctl`, `__arch_prctl`, `ptrace` [thin]

**Contract.**

* **`prctl(option, ...)`** gets or sets per-process or per-thread attributes: name, dumpable,
  no-new-privs, seccomp, parent-death signal, child subreaper, capability bounding set,
  memory tagging, and so on. It is variadic.
* **`personality(p)`** sets the execution domain flags (for example `ADDR_NO_RANDOMIZE`).
* **`unshare(flags)`** detaches the caller from shared namespaces or resources.
  **`setns(fd, nstype)`** joins a namespace.
* **`arch_prctl(code, addr)`** gets or sets x86 architecture state (FS and GS base, CPUID
  faulting, AMX permissions). ⚑ KERN.
* **`ptrace(request, pid, addr, data)`** traces a process. The variadic glibc prototype
  returns the peeked word for `PEEK*` requests, so a caller must zero `errno` to detect
  errors. ⚑ HIST.

**Kernel.** `prctl`, `personality`, `unshare`, `setns`, `arch_prctl`, `ptrace`.

---

## 19. Terminals, job control and pseudo-terminals

### 19.1 Terminal attributes: `tcgetattr`, `tcsetattr`, `cfgetispeed`, `cfgetospeed`, `cfsetispeed`, `cfsetospeed`, `cfsetspeed`, `cfmakeraw`, `tcsendbreak`, `tcdrain`, `tcflush`, `tcflow`, `tcgetsid`, `isatty`, `gtty`/`stty` (compat stubs)

**Contract.**

* **`tcgetattr`/`tcsetattr`** read and write a terminal's line-discipline settings
  (`struct termios`):
  * input, output, control and local mode flags;
  * control characters (`VINTR`, `VEOF`, `VMIN`, `VTIME`, ...);
  * line speeds.

  `tcsetattr` applies the settings immediately (`TCSANOW`), after output drains
  (`TCSADRAIN`), or after draining and flushing input (`TCSAFLUSH`).
* **The `cf*` functions** get and set speeds as `B*` constants.
* **`cfmakeraw`** configures raw mode.
* **`tcsendbreak`** sends a break. **`tcdrain`** waits for output to drain (a cancellation
  point). **`tcflush`** discards queued data. **`tcflow`** suspends or resumes output or
  input.
* **`tcgetsid`** returns the session that owns the terminal.
* **`isatty(fd)`** reports whether `fd` is a terminal (`ENOTTY` otherwise).

**Failure.** `ENOTTY`, `EINVAL`, `EINTR`, `EIO` (background process group, orphaned).

**Kernel.** `ioctl(TCGETS, TCSETS, TCSETSW, TCSETSF, TCSBRK, TCXONC, TCFLSH, TIOCGSID)`.

⚑ HIST, ⚑ KERN.

* `tcsetattr` "succeeds if *any* change was applied". POSIX requires callers to re-read the
  settings to verify them.
* glibc's `struct termios` differs from the kernel's `struct termios` (glibc's has larger
  `c_cc` and separate speed fields), so glibc translates between them.
* Speeds are symbolic `B*` codes rather than integers. Arbitrary rates need `BOTHER` and
  `TCSETS2`, which glibc does not expose in 2.39.

### 19.2 Controlling terminal and foreground group: `tcgetpgrp`, `tcsetpgrp`, `ctermid` (§12.12)

**Contract.**

* **`tcgetpgrp(fd)`** returns the foreground process group of the terminal.
* **`tcsetpgrp(fd, pgrp)`** makes `pgrp` the foreground group. When a background process
  calls it, it sends `SIGTTOU` unless that signal is blocked or ignored.

**Kernel.** `ioctl(TIOCGPGRP, TIOCSPGRP)`.

### 19.3 Terminal names: `ttyname`, `ttyname_r`, `__ttyname_r_chk`

**Contract.** Return the Path of the terminal device open on `fd`. The non-`_r` form uses a
static buffer (⚑ PATH).

**Failure.**

* `ENOTTY`: `fd` is not a terminal.
* `ENODEV`: the device exists, but not in this mount namespace's `/dev`, for example inside a
  container with a different `/dev/pts`.
* `ERANGE`: the buffer is too small.

**Kernel.** `readlink("/proc/self/fd/N")`, `fstat`, `newfstatat` (to validate the link
target), and `openat`/`getdents64` on `/dev/pts` and `/dev` as a fallback.

⚑ HIST. The kernel has no "name of this terminal" call. The name is reverse-engineered from
`/proc` and a directory scan.

### 19.4 Pseudo-terminals: `posix_openpt`, `getpt`, `grantpt`, `unlockpt`, `ptsname`, `ptsname_r`, `__ptsname_r_chk`, `openpty`, `forkpty`, `login_tty`

**Contract.**

* **`posix_openpt(flags)`** opens a new master side (`O_RDWR`, `O_NOCTTY`).
* **`grantpt(fd)`** sets ownership and permissions of the slave. On Linux with devpts this is
  already done by the kernel, so glibc only validates `fd`.
* **`unlockpt(fd)`** unlocks the slave so it can be opened.
* **`ptsname(fd)`** returns the slave's Path. It uses static storage (⚑ PATH).
* **`openpty(&master, &slave, name, termp, winp)`** creates a pair, opening both sides and
  optionally applying attributes and a window size.
* **`forkpty`** does `openpty` + `fork` + `login_tty` in the child.
* **`login_tty(fd)`** makes `fd` the controlling terminal of a new session and redirects
  descriptors 0–2 to it.

**Failure.** `EAGAIN`/`ENOENT` (no ptys available), `EINVAL`/`ENOTTY` (not a master).

**Kernel.** `openat("/dev/ptmx")`, `ioctl(TIOCGPTN, TIOCSPTLCK, TIOCGPTPEER, TIOCSCTTY,
TIOCSWINSZ)`, `setsid`, `dup2`, `clone`.

⚑ HIST. Four calls (`posix_openpt`, `grantpt`, `unlockpt`, `ptsname`) implement what is one
operation on Linux (`TIOCGPTPEER` opens the peer directly). The steps exist because older
Unixes needed a setuid helper (`pt_chown`, removed from glibc in 2.33).

### 19.5 Legacy terminal interfaces: `ttyent` (`getttyent`, `getttynam`, `setttyent`, `endttyent`), `sgtty` (`gtty`, `stty`), `revoke` (stub)

**Contract.** `getttyent` and the related functions read `/etc/ttys` (BSD). `gtty`, `stty`
and `revoke` are compat stubs that fail with `ENOSYS`. ⚑ COMPAT.

---

## 20. Signals

### 20.1 Signal model

**Contract.** A *signal* is an asynchronous notification identified by a number from 1 to 64
(`_NSIG` = 65 on Linux):

* standard signals 1–31;
* real-time signals `SIGRTMIN`..`SIGRTMAX`, which are 34–64 as visible to applications.

Each signal has, per process, a *disposition*: default, ignore, or a handler. Each thread has
a *mask* of blocked signals. Signals are either pending per thread or pending per process.
Standard signals do not queue: a second pending instance is merged. Real-time signals queue,
carry a value, and are delivered lowest-numbered first.

⚑ KERN. Numbering, default actions, queueing semantics, and the `siginfo_t` layout are kernel
ABI.

⚑ HIST (glibc-imposed). glibc reserves real-time signals 32 and 33 for internal use:
`SIGCANCEL` (cancellation, and `SIGTIMER` for `SIGEV_THREAD` timers) and `SIGSETXID` (§17.2).
`SIGRTMIN` is therefore a *function call* (`__libc_current_sigrtmin`), not a constant. glibc
refuses to let applications install handlers for, or block, those two signals.

**`__libc_current_sigrtmin`, `__libc_current_sigrtmax`, `__libc_allocate_rtsig`** return and
allocate real-time signal numbers.

### 20.2 Installing handlers: `sigaction`, `__sigaction`, `signal`, `bsd_signal`, `sysv_signal`, `ssignal`, `gsignal`, `sigvec` (compat), `siginterrupt`, `sigset`, `sighold`, `sigrelse`, `sigignore`

**Contract.**

* **`sigaction(sig, &act, &old)`** sets and gets the disposition. `act` holds the handler (or
  `SIG_DFL`/`SIG_IGN`), a mask added during the handler, and flags:
  * `SA_SIGINFO`: a three-argument handler receiving `siginfo_t` and `ucontext_t`;
  * `SA_RESTART`: C0.3;
  * `SA_NODEFER`, `SA_RESETHAND`;
  * `SA_ONSTACK`: use the alternate stack;
  * `SA_NOCLDSTOP`, `SA_NOCLDWAIT`: child-related behaviour.
* **`signal(sig, handler)`** in glibc has **BSD semantics**: the handler stays installed, the
  signal is blocked during the handler, and calls restart. `sysv_signal` gives the SVR4
  semantics: one-shot handler, no blocking, no restart. ⚑ HIST: the meaning of `signal`
  differs between Unix families. ISO C left it unspecified, which makes `signal` unportable.
* **`siginterrupt(sig, flag)`** toggles `SA_RESTART`.
* **`sigset`, `sighold`, `sigrelse`, `sigignore`** are SVR4 forms (removed from POSIX in
  2024). ⚑ HIST.
* **`ssignal`/`gsignal`** are SVID software signals (`gsignal` = `raise`). ⚑ HIST.
* **`sigvec`** is 4.2BSD, compat only.
* **`sigreturn`** is an exported stub that always fails with `ENOSYS`. The real return from a
  handler goes through the restorer described below.

**Failure.** `EINVAL`: an invalid signal, an attempt to catch or ignore `SIGKILL` or
`SIGSTOP`, or a glibc-internal signal.

**Kernel.** `rt_sigaction`. ⚑ KERN: on x86_64 the kernel **requires** `SA_RESTORER` and a
user-space trampoline that invokes `rt_sigreturn`, which glibc supplies (`__restore_rt`).
A replacement libc must supply one too.

### 20.3 Signal masks and sets: `sigprocmask`, `pthread_sigmask`, `sigemptyset`, `sigfillset`, `sigaddset`, `sigdelset`, `sigismember`, `sigisemptyset`, `sigandset`, `sigorset`, `__sigaddset`, `__sigdelset`, `__sigismember`, `sigblock`, `sigsetmask`, `siggetmask`, `sigpause`, `__sigpause`, `__xpg_sigpause`

**Contract.**

* **`pthread_sigmask(how, &set, &old)`** changes the calling thread's mask with
  `SIG_BLOCK`, `SIG_UNBLOCK` or `SIG_SETMASK`. It returns an error number.
  **`sigprocmask`** is the same with the `errno` convention.
* **The set operations** manipulate a `sigset_t`. `sigisemptyset`, `sigandset` and `sigorset`
  are GNU. The `__sig*` forms are inline helpers.
* **`sigblock`, `sigsetmask` and `siggetmask`** are BSD forms that use an `int` mask of the
  first 31 signals. ⚑ HIST.
* **`sigpause`** has two contracts selected by feature macros: BSD takes a mask, and XSI
  (`__xpg_sigpause`) takes a signal to remove from the mask. ⚑ HIST.

**Guarantees.** `SIGKILL` and `SIGSTOP` cannot be blocked, and are silently removed from the
set. So are glibc's internal signals.

**Kernel.** `rt_sigprocmask`.

⚑ ARB, ⚑ HIST. glibc's `sigset_t` is **1024 bits (128 bytes)**, while the kernel's is 64
bits. The glibc size was chosen in the 1990s to allow for growth that never happened. It is
now ABI and inflates `ucontext_t`, `jmp_buf` (`sigjmp_buf`) and `posix_spawnattr_t`.

### 20.4 Sending signals: `kill`, `killpg`, `raise`, `sigqueue`, `pthread_kill`, `pthread_sigqueue`, `tgkill`, `abort` (§1.2)

**Contract.**

* **`kill(pid, sig)`** sends to a process. `pid` 0 means the caller's group, -1 means all
  processes the caller may signal, and `-pgid` means a group. `sig` 0 only checks existence
  and permission.
* **`killpg(pgrp, sig)`** sends to a group.
* **`raise(sig)`** sends to the calling thread. The signal is delivered before `raise`
  returns, if unblocked.
* **`sigqueue(pid, sig, value)`** sends with an attached `sigval`. It queues, for real-time
  signals.
* **`pthread_kill(thread, sig)`** targets a thread in this process. **`pthread_sigqueue`**
  (GNU) does the same with a value.
* **`tgkill(tgid, tid, sig)`** (2.30) is a thin wrapper.

**Failure.** `EINVAL`, `EPERM`, `ESRCH`. `pthread_kill` on a thread that has exited but not
been joined returns 0 in 2.34+.

**Kernel.** `kill`, `tgkill`, `rt_sigqueueinfo`, `rt_tgsigqueueinfo`, `getpid`, `gettid`.
`raise` blocks all signals, reads the TID, calls `tgkill`, and restores the mask, so that it
targets the right thread even if a handler forks.

### 20.5 Waiting for signals: `pause`, `sigsuspend`, `sigwait`, `sigwaitinfo`, `sigtimedwait`, `sigpending`

**Contract.**

* **`pause()`** sleeps until a handler runs, then returns -1/`EINTR`.
* **`sigsuspend(&mask)`** atomically replaces the mask and waits for a handler to run, then
  restores the mask.
* **`sigwait(&set, &sig)`** synchronously accepts a pending signal from `set`, which should be
  blocked. It returns the signal number through `sig`, and an error number as its result.
* **`sigwaitinfo`/`sigtimedwait`** do the same with `siginfo_t` and an optional timeout
  (`EAGAIN` on timeout).
* **`sigpending(&set)`** reports the pending signals.

**Cancellation.** All the waiting functions are cancellation points.

**Kernel.** `rt_sigsuspend`, `rt_sigtimedwait`, `rt_sigpending`, `pause`
(`ppoll` on architectures without it).

### 20.6 Signal descriptors: `signalfd` [thin]

**Contract.** Create or modify a descriptor that returns pending signals from a mask as
`struct signalfd_siginfo` records when read. The signals must be blocked in order to be
received this way.

**Kernel.** `signalfd4`.

### 20.7 Alternate signal stack: `sigaltstack`, `sigstack` (compat), `MINSIGSTKSZ`, `SIGSTKSZ`

**Contract.** Define a per-thread memory region on which handlers flagged `SA_ONSTACK` run.
This allows handling stack overflow. `SS_DISABLE` disables it, and `SS_AUTODISARM`
(kernel 4.7+) disarms it while in use.

**Inputs.** The size must be at least `MINSIGSTKSZ`. ⚑ KERN, ⚑ HIST: since 2.34,
`MINSIGSTKSZ` and `SIGSTKSZ` are **not constants** under `_DYNAMIC_STACK_SIZE_SOURCE`/
`_GNU_SOURCE`. They call `sysconf(_SC_MINSIGSTKSZ)`, because AVX-512 and AMX signal frames
exceed the historical 2 KiB/8 KiB constants. The kernel reports the real minimum in
`AT_MINSIGSTKSZ`.

**Kernel.** `sigaltstack`, auxv `AT_MINSIGSTKSZ`.

### 20.8 Signal information: `siginfo_t`, `psiginfo`, `psignal`, `sigdescr_np`, `sigabbrev_np`, `strsignal`, `sys_siglist`, `_sys_siglist`, `sys_sigabbrev` (compat)

**Contract.**

* **`strsignal(sig)`** returns a translated description. The storage is per-thread for
  unknown signals.
* **`sigdescr_np`** returns the untranslated description, and **`sigabbrev_np`** returns the
  name without `SIG` (for example `"INT"`). Both were added in 2.32.
* **`psignal(sig, s)`** and **`psiginfo(info, s)`** print to `stderr`.

⚑ COMPAT. `sys_siglist` is the same arrays-in-ABI problem as `sys_errlist` (§2.2).

**Kernel.** None.

### 20.9 Handler execution environment

**Contract.** A handler runs on the interrupted thread, with the signal (and `sa_mask`)
blocked. It may call only async-signal-safe functions, per the POSIX list: `write`, `read`,
`_exit`, `sigaction`, `kill`, `raise`, `open`, `close`, `fork`/`_Fork`, `execve`, `waitpid`,
`sem_post`, `clock_gettime`, and others. It must save and restore `errno`. Returning from a
handler for a synchronous fault (`SIGSEGV`, `SIGBUS`, `SIGFPE`, `SIGILL`) that does not fix
the cause re-executes the faulting instruction.

⚑ HIST. The async-signal-safety restriction exists because signals interrupt arbitrary code.
The functional need behind most handlers ("notice that event X happened") is better met by
`signalfd` or `sigwait`, which convert signals into synchronous events.

---

## 21. Non-local control transfer and execution contexts

### 21.1 `setjmp`, `_setjmp`, `__sigsetjmp`, `sigsetjmp` (macro), `longjmp`, `_longjmp`, `siglongjmp`, `__longjmp_chk`

**Contract.**

* **`setjmp(env)`** saves the calling environment (callee-saved registers, stack pointer,
  program counter, and for `sigsetjmp(env, 1)` the signal mask). It returns 0 when called
  directly.
* **`longjmp(env, v)`** restores the environment so that `setjmp` appears to return `v` (1 if
  `v` is 0). The function that called `setjmp` must not have returned.
* **`_setjmp`/`_longjmp`** never save or restore the mask. **`setjmp`** on glibc does not save
  the mask either (BSD would). ⚑ HIST: the default differs by Unix family.
* **`__longjmp_chk`** is the fortified form. It aborts if the jump would move to a frame
  *below* the current stack pointer (an invalid jump), except for jumps between the alternate
  signal stack and the main stack.

**Inputs.** `setjmp` may appear only in restricted syntactic contexts (as a whole expression
statement, or in a comparison inside `if`/`while`/`switch`). Local non-`volatile` variables
modified after `setjmp` have indeterminate values after `longjmp`. ⚑ HIST: `setjmp` is a
function that "returns twice", which compilers must special-case.

**Guarantees.** On x86_64 the saved stack and return addresses are mangled with a per-process
secret (pointer guard). With CET, the shadow stack is unwound to match (via `incssp`).

**Kernel.** `rt_sigprocmask` (the mask save and restore), and CET shadow-stack
instructions. ⚑ ARB: `jmp_buf` embeds a full 128-byte `sigset_t` (§20.3).

### 21.2 User contexts: `getcontext`, `setcontext`, `makecontext`, `swapcontext`

**Contract.**

* **`getcontext(&uc)`** saves the full user context (registers, signal mask, stack
  description).
* **`setcontext(&uc)`** resumes it.
* **`makecontext(&uc, fn, argc, ...)`** modifies a context so that resuming it calls `fn`
  with `argc` integer arguments on the stack in `uc.uc_stack`. When `fn` returns, it resumes
  `uc.uc_link`, or exits the thread if that is NULL.
* **`swapcontext(&old, &new)`** saves into `old` and resumes `new`.

**Inputs.** Arguments to `makecontext` must be `int`-sized. ⚑ HIST: on 64-bit ABIs, passing
pointers is technically non-portable, although glibc supports 64-bit values on x86_64.

**Kernel.** `rt_sigprocmask` (the mask switch on every swap, which makes these calls slow),
and CET shadow-stack management (`map_shadow_stack`/`arch_prctl`).

⚑ HIST. These functions were removed from POSIX in 2008 because `makecontext`'s variadic
`int` signature cannot be made type-safe. They remain the only standard-library mechanism for
user-space coroutines. The functional need, switching between independent stacks, is
legitimate and is not otherwise met.

---

## 22. Threads and synchronization

**Section-wide notes.**

* Since 2.34 all of `libpthread` is in `libc.so.6`. `libpthread.so.0` remains as an empty
  placeholder (⚑ COMPAT).
* Pthread functions return **error numbers** and do not set `errno` (C0.1). The exceptions
  are `sem_*`, which use `errno`.
* **Kernel (section-wide).** Thread synchronization rests on one primitive, the **futex**
  (`futex`/`futex_time64`), with operations `FUTEX_WAIT`, `FUTEX_WAKE`,
  `FUTEX_WAIT_BITSET` (absolute timeouts on a chosen clock), `FUTEX_LOCK_PI`,
  `FUTEX_UNLOCK_PI`, `FUTEX_TRYLOCK_PI`, `FUTEX_LOCK_PI2`, `FUTEX_CMP_REQUEUE_PI` and
  `FUTEX_WAIT_REQUEUE_PI`. The uncontended paths are pure user-space atomic instructions. The
  futex syscall itself is not exported by glibc; callers use `syscall()`.

### 22.1 Thread lifecycle: `pthread_create`, `pthread_exit`, `pthread_join`, `pthread_tryjoin_np`, `pthread_timedjoin_np`, `pthread_clockjoin_np`, `pthread_detach`, `pthread_self`, `pthread_equal`, `thrd_create`, `thrd_exit`, `thrd_join`, `thrd_detach`, `thrd_current`, `thrd_equal`, `thrd_sleep`, `thrd_yield`, `__libc_single_threaded`

**Contract.**

* **`pthread_create(&t, attr, fn, arg)`** starts a new thread in the process running
  `fn(arg)`. The new thread inherits the creator's signal mask (unless set by attribute), its
  floating-point environment, and its CPU affinity. It starts with no pending signals and a
  fresh `errno` and TLS.
* **`pthread_exit(v)`** terminates the calling thread. It runs cancellation cleanup handlers,
  then TLS destructors (§22.6), then makes `v` available to `join`. When the last thread
  exits, the process exits with status 0.
* **`pthread_join(t, &v)`** waits for `t` to terminate and releases it. The `_np` forms try,
  time out (on `CLOCK_REALTIME`), or time out on a chosen clock.
* **`pthread_detach(t)`** means `t`'s resources are released at termination without a join.
* **`pthread_self`/`pthread_equal`** identify threads. `pthread_t` values of terminated and
  released threads may be reused.
* **The C11 `thrd_*` functions** have the same semantics with C11 result codes
  (`thrd_success`, `thrd_nomem`, `thrd_timedout`, `thrd_busy`, `thrd_error`). `thrd_sleep`
  returns -1 on interruption (with the remaining time) and another negative value on error.
* **`__libc_single_threaded`** (2.32) is a `char` that is non-zero while the process has
  never created a thread. Libraries use it to skip atomic operations.

**Failure.**

* `EAGAIN`: thread or memory limits.
* `EINVAL`: a bad attribute.
* `EPERM`: a scheduling policy that is not permitted.
* `EDEADLK`: joining self or a join cycle.
* `ESRCH`/`EINVAL`: the thread is not joinable.

Joining a detached thread, or joining twice, is undefined behaviour.

**Kernel.**

* `clone3` (falling back to `clone`) with `CLONE_VM|CLONE_FS|CLONE_FILES|CLONE_SIGHAND|
  CLONE_THREAD|CLONE_SYSVSEM|CLONE_SETTLS|CLONE_PARENT_SETTID|CLONE_CHILD_CLEARTID`.
* `mmap` + `mprotect` (the stack and its guard page; the stacks of exited threads are
  cached).
* `set_robust_list` and `rseq` (per new thread), `rt_sigprocmask`, `sched_setaffinity`/
  `sched_setscheduler`/`sched_setparam` (attributes), `futex` (join waits for the kernel to
  clear the TID word at exit via `CLONE_CHILD_CLEARTID`), `exit` (the thread-only exit
  syscall), `madvise(MADV_DONTNEED)` (releasing cached stacks).

⚑ HIST. `pthread_t` is an opaque handle that may be reused, so there is no race-free way to
check whether a thread still exists. The kernel TID is available via `gettid` or
`pthread_gettid_np` (the latter is not in 2.39).

### 22.2 Thread attributes: `pthread_attr_init`, `pthread_attr_destroy`, `pthread_attr_{get,set}detachstate`, `..._{get,set}stacksize`, `..._{get,set}stack`, `..._{get,set}stackaddr`, `..._{get,set}guardsize`, `..._{get,set}schedparam`, `..._{get,set}schedpolicy`, `..._{get,set}inheritsched`, `..._{get,set}scope`, `..._{get,set}affinity_np`, `..._{get,set}sigmask_np`, `pthread_getattr_np`, `pthread_getattr_default_np`, `pthread_setattr_default_np`, `pthread_getconcurrency`, `pthread_setconcurrency`

**Contract.** Build an attribute object that controls a new thread's:

* detached state;
* stack: size (at least `PTHREAD_STACK_MIN`, which since 2.34 is `sysconf(_SC_THREAD_STACK_MIN)`
  and not a constant), a caller-provided stack, and the guard size (rounded to pages; 0
  disables the guard);
* scheduling: policy and parameters, inherited or explicit. Only `PTHREAD_SCOPE_SYSTEM` is
  supported, and `PROCESS` gives `ENOTSUP`.
* CPU affinity and initial signal mask.

`pthread_getattr_np(t, &a)` returns the *actual* attributes of a running thread, including
its stack bounds. For the main thread, these come from `/proc/self/maps` and
`RLIMIT_STACK`. The `_default_np` functions set process-wide defaults.
`pthread_{get,set}concurrency` are no-ops kept for XSI. ⚑ HIST.

**Default stack size.** The `RLIMIT_STACK` soft limit at startup (8 MiB typically), or
2 MiB if that is unlimited.

**Kernel.** `getrlimit`, `openat`/`read` on `/proc/self/maps` (`getattr_np` on the main
thread), `sched_getaffinity`.

⚑ HIST. `pthread_attr_setstackaddr` is ambiguous about whether the address is the low or high
end of the stack, so it was deprecated in favour of `setstack` (⚑ PATH).

### 22.3 Cancellation: `pthread_cancel`, `pthread_setcancelstate`, `pthread_setcanceltype`, `pthread_testcancel`, `pthread_cleanup_push`/`_pop` (macros), `_pthread_cleanup_push`, `_pthread_cleanup_pop`, `_pthread_cleanup_push_defer`, `_pthread_cleanup_pop_restore`, `__pthread_register_cancel`, `__pthread_unregister_cancel`, `__pthread_register_cancel_defer`, `__pthread_unregister_cancel_restore`, `__pthread_unwind_next`, `__pthread_cleanup_routine`

**Contract.** Request that a target thread terminate.

* In the **deferred** mode (the default), the target acts on the request at the next
  cancellation point (C0.2), or at `pthread_testcancel`.
* In the **asynchronous** mode, the target acts at any instruction. Only
  async-cancel-safe code may run with this enabled.
* **Cancellation state** may be disabled, in which case requests stay pending.
* **Acting on cancellation** runs the cleanup handlers in LIFO order and TLS destructors.
  In C++, it unwinds the stack with a forced unwind (`abi::__forced_unwind`). The thread then
  exits with `PTHREAD_CANCELED`.

**Guarantees.** If a cancellation point is cancelled *before* the underlying syscall had a
side effect, the side effect did not happen. For `close`, the descriptor is still closed
(⚑ KERN). For `read`/`write` cancelled after partial transfer, the call returns normally and
is not cancelled.

**Kernel.** `tgkill` with `SIGCANCEL` (the internal signal). The cancellation points check a
per-thread flag on entry, and the signal handler checks whether the interrupted PC lies
inside a syscall wrapper window. It also uses the unwinder (`libgcc_s` via `dlopen`, which is
why cancellation can fail if `libgcc_s.so.1` is missing).

⚑ HIST (high value). Cancellation is a retrofit onto a library designed without it:

* it needs a special marked window around every blocking syscall;
* it needs a reserved signal;
* it needs a run-time dependency on the C++ unwinder;
* it has the famous ambiguity about side effects.

The functional need is "stop that thread cooperatively". Modern designs express it through
explicit cancellation tokens.

### 22.4 Mutexes: `pthread_mutex_init`, `pthread_mutex_destroy`, `pthread_mutex_lock`, `pthread_mutex_trylock`, `pthread_mutex_timedlock`, `pthread_mutex_clocklock`, `pthread_mutex_unlock`, `pthread_mutex_consistent`, `pthread_mutex_consistent_np`, `pthread_mutex_getprioceiling`, `pthread_mutex_setprioceiling`, `pthread_mutexattr_init`, `pthread_mutexattr_destroy`, `pthread_mutexattr_{get,set}type`, `..._{get,set}pshared`, `..._{get,set}protocol`, `..._{get,set}prioceiling`, `..._{get,set}robust`, `..._{get,set}robust_np`, `..._{get,set}kind_np` (compat), `mtx_init`, `mtx_lock`, `mtx_timedlock`, `mtx_trylock`, `mtx_unlock`, `mtx_destroy`

**Contract.** Mutual exclusion. At most one thread owns a mutex.

* **Types:** `NORMAL` (self-deadlock on relock), `ERRORCHECK` (`EDEADLK` on relock, `EPERM`
  on unlock by a non-owner), `RECURSIVE` (a count), and `DEFAULT` (= `NORMAL`).
  `PTHREAD_MUTEX_ADAPTIVE_NP` spins before blocking.
* **Protocols:** `NONE`, `INHERIT` (priority inheritance), `PROTECT` (priority ceiling).
* **`pshared`:** usable across processes when the mutex is placed in shared memory.
* **Robust:** if the owner dies, the next locker gets `EOWNERDEAD` and must call
  `pthread_mutex_consistent`. Otherwise, after unlocking, the mutex becomes permanently
  `ENOTRECOVERABLE`.
* **Timeouts:** `timedlock` is absolute on `CLOCK_REALTIME`. `clocklock` (2.30) takes the
  clock: `CLOCK_MONOTONIC` or `CLOCK_REALTIME`.
* **Static initialization:** `PTHREAD_MUTEX_INITIALIZER` (all zero bytes) and the
  `_NP` initializers for the other types.
* **C11 `mtx_*`:** `mtx_plain`, `mtx_timed`, `mtx_recursive`.

**Failure.** `EBUSY` (trylock), `ETIMEDOUT`, `EDEADLK`, `EPERM`, `EOWNERDEAD`,
`ENOTRECOVERABLE`, `EINVAL`, `EAGAIN` (recursion count overflow).

**Kernel.**

* `futex` (`WAIT`/`WAKE`, and the `LOCK_PI`/`UNLOCK_PI` operations for PI mutexes).
* `set_robust_list`: the kernel walks the per-thread robust list at thread death and marks
  held robust mutexes with `FUTEX_OWNER_DIED`.
* `sched_getparam` and `sched_setscheduler` (priority ceiling).

⚑ ARB. The object sizes (`pthread_mutex_t` 40 bytes, `pthread_cond_t` 48, `pthread_rwlock_t`
56, `pthread_barrier_t` 32, `sem_t` 32) and static-initializer bit patterns are ABI. Process-
shared use means their *layout* is shared between binaries built against different glibc
versions (⚑ COMPAT).

### 22.5 Condition variables, read-write locks, spin locks, barriers, once: `pthread_cond_*` (`init`, `destroy`, `wait`, `timedwait`, `clockwait`, `signal`, `broadcast`), `pthread_condattr_*` (`init`, `destroy`, `{get,set}pshared`, `{get,set}clock`), `pthread_rwlock_*` (`init`, `destroy`, `rdlock`, `tryrdlock`, `timedrdlock`, `clockrdlock`, `wrlock`, `trywrlock`, `timedwrlock`, `clockwrlock`, `unlock`), `pthread_rwlockattr_*` (`init`, `destroy`, `{get,set}pshared`, `{get,set}kind_np`), `pthread_spin_*` (`init`, `destroy`, `lock`, `trylock`, `unlock`), `pthread_barrier_*` (`init`, `destroy`, `wait`), `pthread_barrierattr_*`, `pthread_once`, `cnd_*`, `call_once`

**Contract.**

* **Condition variables.** `wait(c, m)` atomically releases `m` and blocks until signalled,
  then reacquires `m`. `signal` wakes at least one waiter, and `broadcast` wakes all. Spurious
  wakeups are permitted, so callers re-check their predicate. The timed variants use absolute
  timeouts. The clock is `CLOCK_REALTIME` by default, set by `condattr_setclock`, or given
  per call (`clockwait`).
  * **Guarantee (glibc 2.25+):** a signal wakes only a waiter that blocked *before* the
    signal. Waiters are grouped to ensure this.
  * ⚑ COMPAT: `pthread_cond_*@GLIBC_2.2.5` implement the pre-2.3.2 condvar ABI for old
    binaries.
* **Read-write locks.** Many readers or one writer. The preference is set by `kind_np`:
  `PREFER_READER` (the default; ⚑ HIST: writer starvation is possible),
  `PREFER_WRITER_NONRECURSIVE`. A recursive read lock by the same thread is permitted when
  preferring readers.
* **Spin locks.** Busy-waiting mutual exclusion with no blocking. **Kernel:** none.
* **Barriers.** Block until `count` threads have arrived. Exactly one of them receives
  `PTHREAD_BARRIER_SERIAL_THREAD`.
* **`pthread_once(&o, fn)`/`call_once`.** Run `fn` exactly once across all threads. Others
  wait until it completes. If `fn` is cancelled, the once object is reset.

**Kernel.** `futex` (with `FUTEX_WAIT_BITSET` for absolute timeouts on the chosen clock, and
`FUTEX_PRIVATE_FLAG` unless process-shared).

### 22.6 Thread-specific storage: `pthread_key_create`, `pthread_key_delete`, `pthread_getspecific`, `pthread_setspecific`, `tss_create`, `tss_delete`, `tss_get`, `tss_set`, the `__thread`/`thread_local` language feature, `__tls_get_addr`, `__libc_dlerror_result` (internal)

**Contract.**

* **Keys** are process-wide identifiers. Each thread has its own `void *` value per key
  (initially NULL), and a key may have a destructor.
* At thread exit, destructors run for non-NULL values, repeated up to
  `PTHREAD_DESTRUCTOR_ITERATIONS` (4) rounds while values remain.
* Keys are limited to `PTHREAD_KEYS_MAX` (1024). ⚑ ARB.
* **Language-level TLS** (`__thread`, `_Thread_local`, `thread_local`) gives each thread its
  own instance of a variable. For dynamically loaded modules, the storage is allocated lazily
  through `__tls_get_addr(module, offset)`.

**Guarantees.** Static TLS for the executable and initially loaded libraries is at a fixed
offset from the thread pointer.

**Kernel.** The thread pointer register (`arch_prctl(ARCH_SET_FS)` for the main thread, and
`CLONE_SETTLS` for others). `mmap` for dynamic TLS blocks.

⚑ HIST. The static TLS surplus (space reserved for `dlopen`ed libraries using the
initial-exec model) is a fixed size, tunable with `glibc.rtld.optional_static_tls`.
Exhausting it produces `cannot allocate memory in static TLS block`. That is a failure mode
that exists only because of how TLS models were layered.

### 22.7 Semaphores: `sem_init`, `sem_destroy`, `sem_wait`, `sem_trywait`, `sem_timedwait`, `sem_clockwait`, `sem_post`, `sem_getvalue`, `sem_open`, `sem_close`, `sem_unlink`

**Contract.** A counting semaphore.

* `post` increments and wakes a waiter. It is **async-signal-safe**, the only
  synchronization primitive that is.
* `wait` decrements, blocking while the count is 0. The timed forms take an absolute time.
* `getvalue` reports the count (glibc never reports a negative count for waiters).
* **Unnamed semaphores** (`sem_init`) are shared between threads or, with `pshared`, between
  processes.
* **Named semaphores** (`sem_open(name, O_CREAT|O_EXCL, mode, value)`) are files in
  `/dev/shm` named `sem.<name>`. Opening the same name twice in one process returns the same
  address.

**Failure.** Returns -1 with `errno`: `EAGAIN` (trywait), `ETIMEDOUT`, `EINTR`, `EINVAL`,
`EOVERFLOW` (`SEM_VALUE_MAX` = `INT_MAX`), `ENOENT`, `EEXIST`, `EACCES`.

**Kernel.** `futex`. For named semaphores: `openat` on `/dev/shm`, `mmap(MAP_SHARED)`,
`linkat` (atomic creation through a temporary file), `unlinkat`, `munmap`. **Config:** the
tmpfs mount at `/dev/shm` (⚑ HIST: a mount convention, not a kernel object).

### 22.8 Thread naming, scheduling and affinity: `pthread_setname_np`, `pthread_getname_np`, `pthread_setschedparam`, `pthread_getschedparam`, `pthread_setschedprio`, `pthread_yield`, `pthread_setaffinity_np`, `pthread_getaffinity_np`, `pthread_getcpuclockid`, `pthread_kill_other_threads_np` (compat no-op), `pthread_sigmask` (§20.3)

**Contract.**

* **Naming.** Set or get a thread name of at most 15 bytes plus a terminator (⚑ KERN:
  `TASK_COMM_LEN` is 16). Returns `ERANGE` if too long.
* **Scheduling.** Per-thread scheduling policy and priority.
* **`pthread_yield`** is `sched_yield`. ⚑ HIST: it is deprecated.
* **Affinity.** A per-thread CPU mask.
* **`pthread_getcpuclockid`** returns a clock ID that measures the thread's CPU time.

**Kernel.** `prctl(PR_SET_NAME)` (self), or `openat`/`write` on
`/proc/self/task/<tid>/comm` (another thread). `sched_setscheduler`, `sched_setparam`,
`sched_getparam`, `sched_getscheduler`, `sched_setaffinity`, `sched_getaffinity`,
`sched_yield`. The clock ID encodes the TID for `clock_gettime`.

### 22.9 Restartable sequences: `__rseq_offset`, `__rseq_size`, `__rseq_flags`

**Contract.** glibc registers an rseq area for each thread (2.35+, unless disabled with
`glibc.pthread.rseq=0`) and publishes its offset from the thread pointer and its size, so
that applications can use restartable sequences without registering their own area.
`sched_getcpu` reads the current CPU from this area without a syscall.

**Kernel.** `rseq`. ⚑ KERN: only one rseq registration per thread is allowed, which is why
the library, not the application, must own it.

### 22.10 Debugger support library: `libthread_db` (`td_init`, `td_ta_new`, `td_ta_delete`, `td_ta_map_lwp2thr`, `td_ta_thr_iter`, `td_ta_tsd_iter`, `td_thr_get_info`, `td_thr_getfpregs`, `td_thr_getgregs`, `td_thr_tls_get_addr`, `td_thr_tlsbase`, `td_thr_event_enable`, `td_ta_event_addr`, `td_ta_event_getmsg`, `td_symbol_list`, ...), `ps_*` callbacks

**Contract.** A library that a *debugger* loads to interpret the thread structures of a target
process. The debugger supplies memory and register access callbacks (`ps_pdread`,
`ps_pdwrite`, `ps_lgetregs`, `ps_pglobal_lookup`, ...), and the library reports:

* the thread list;
* the LWP ↔ thread mapping;
* TLS addresses;
* TSD values;
* creation and death event breakpoints.

**Kernel.** None of its own. It operates through the debugger's callbacks (which use
`ptrace` or core files).

⚑ HIST. This exists because the thread implementation's internal layout is private, yet
debuggers need to inspect it. It exposes the implementation through a versioned
symbol-description table (`nptl_version`, `_thread_db_*` symbols in `libc`). A replacement
libc must either provide an equivalent or be undebuggable by GDB's thread support.

---

## 23. Time, clocks and timers

### 23.1 Reading clocks: `clock_gettime`, `clock_getres`, `clock_getcpuclockid`, `time`, `gettimeofday`, `ftime` (compat), `clock`, `times`, `timespec_get`, `timespec_getres`

**Contract.** Return the current value of a clock.

* **`clock_gettime(id, &ts)`** reads one of these clocks:
  * `CLOCK_REALTIME`: settable wall-clock time since the Unix epoch;
  * `CLOCK_MONOTONIC`: not settable; it does not count suspend;
  * `CLOCK_BOOTTIME`: monotonic including suspend;
  * `CLOCK_MONOTONIC_RAW`: without NTP slewing;
  * `CLOCK_*_COARSE`: cheaper, lower resolution;
  * `CLOCK_TAI`;
  * `CLOCK_PROCESS_CPUTIME_ID`, `CLOCK_THREAD_CPUTIME_ID`;
  * dynamic clock IDs: those encoding a PID or TID, and those derived from a PTP clock
    descriptor.
* **`clock_getres`** returns the resolution. **`clock_getcpuclockid(pid)`** returns the CPU
  clock ID for a process.
* **`time(&t)`** returns seconds. **`gettimeofday(&tv, tz)`** returns microseconds; the `tz`
  argument is obsolete and is filled with zeros. **`ftime`** (compat, removed from the API in
  2.33) returns milliseconds. ⚑ HIST: four resolutions of the same need.
* **`timespec_get(&ts, TIME_UTC)`** (C11) is `CLOCK_REALTIME`. **`timespec_getres`** (C23) is
  its resolution. Only `TIME_UTC` is supported in 2.39; the optional C23 bases
  (`TIME_MONOTONIC`, ...) are not.
* **`clock()`** returns the process CPU time in `CLOCKS_PER_SEC` (= 1,000,000) units, or -1.
  ⚑ HIST: `clock_t` wraps. XSI fixes `CLOCKS_PER_SEC` regardless of the real resolution.
* **`times(&tms)`** returns user and system time for the process and for its waited-for
  children, in clock ticks (`sysconf(_SC_CLK_TCK)` = 100). It also returns an elapsed
  real-time value in ticks from an arbitrary origin. ⚑ HIST.

**Guarantees.** `CLOCK_MONOTONIC` never goes backwards. `CLOCK_REALTIME` may jump.
`tv_nsec` is always in `[0, 1e9)`.

**Failure.** `EINVAL` (unknown clock), `EOVERFLOW` (32-bit `time_t` ABIs).

**Kernel.** The **vDSO** (`__vdso_clock_gettime`, `__vdso_gettimeofday`, `__vdso_time`,
`__vdso_clock_getres`), with fallback to the syscalls `clock_gettime`, `clock_getres`,
`gettimeofday`, `time`; `times` uses the `times` syscall. vDSO location: `AT_SYSINFO_EHDR`.

⚑ KERN. Leap seconds: `CLOCK_REALTIME` is POSIX time, which repeats or smears leap seconds.
There is no leap-second-aware API apart from `CLOCK_TAI` combined with `adjtimex`.

### 23.2 Setting and adjusting clocks: `clock_settime`, `settimeofday`, `stime` (compat), `adjtime`, `adjtimex`, `__adjtimex`, `ntp_adjtime`, `ntp_gettime`, `ntp_gettimex`, `clock_adjtime`

**Contract.**

* **`clock_settime`/`settimeofday`** set a clock. They require `CAP_SYS_TIME`.
  `settimeofday` with a non-NULL `tz` fails with `EINVAL` in 2.31+ (except for the special
  `tz`-only call). ⚑ HIST.
* **`adjtime(&delta, &old)`** slews the clock gradually by `delta`.
* **`adjtimex`/`ntp_adjtime`/`clock_adjtime`** give full access to the kernel NTP state
  machine (offset, frequency, status, PLL/FLL, TAI offset).
* **`ntp_gettime`** reads the time together with the error estimate.

**Kernel.** `clock_settime`, `settimeofday`, `clock_adjtime` (glibc implements `adjtime` and
`adjtimex` with it).

### 23.3 Sleeping: `sleep`, `usleep`, `nanosleep`, `__nanosleep`, `clock_nanosleep`

**Contract.** Suspend the calling thread.

* **`nanosleep(&req, &rem)`** sleeps for a relative duration on `CLOCK_MONOTONIC`. If
  interrupted by a handler, it fails with `EINTR` and stores the remaining time.
* **`clock_nanosleep(id, flags, &req, &rem)`** sleeps on a chosen clock, relative or absolute
  (`TIMER_ABSTIME`, which avoids drift). It returns the error number directly.
* **`sleep(s)`** returns the unslept seconds (0 if complete). **`usleep(us)`** takes
  microseconds. ⚑ HIST: `usleep` was removed from POSIX, and it fails with `EINVAL` for
  `us ≥ 1,000,000` on some systems (not glibc).

**Cancellation.** Yes.

**Kernel.** `clock_nanosleep`.

⚑ HIST. `sleep` historically interacted with `SIGALRM` (it was implemented with `alarm`).
glibc does not do that, but POSIX still permits the interaction.

### 23.4 Timers: `alarm`, `ualarm`, `setitimer`, `getitimer`, `timer_create`, `timer_delete`, `timer_settime`, `timer_gettime`, `timer_getoverrun`, `timerfd_create`, `timerfd_settime`, `timerfd_gettime`

**Contract.**

* **`alarm(s)`** schedules a `SIGALRM` after `s` seconds, replacing any previous alarm. It
  returns the seconds that remained on the previous alarm. It cannot fail. **`ualarm`** does
  the same in microseconds. ⚑ HIST.
* **`setitimer(which, &new, &old)`** sets one of three per-process interval timers:
  `ITIMER_REAL` (sends `SIGALRM`), `ITIMER_VIRTUAL` (`SIGVTALRM`) or `ITIMER_PROF`
  (`SIGPROF`). Values are microseconds.
* **POSIX timers.** `timer_create(clock, &sigevent, &id)` creates any number of timers per
  process. Expiry is notified by:
  * a signal (`SIGEV_SIGNAL`), with `si_value`;
  * a signal to a specific thread (`SIGEV_THREAD_ID`);
  * a new thread (`SIGEV_THREAD`, which glibc implements with a helper thread that receives an
    internal signal);
  * nothing (`SIGEV_NONE`).

  `timer_settime` arms the timer, relative or absolute, with a nanosecond interval.
  `timer_getoverrun` reports expirations missed between signal deliveries.
* **`timerfd_*`** are timers that deliver expirations as a readable counter on a
  descriptor. `TFD_TIMER_CANCEL_ON_SET` notifies when the realtime clock is set.

**Failure.** `EINVAL`, `EAGAIN` (resource limits), `ENOMEM`.

**Kernel.** `alarm` (glibc uses `setitimer` where `alarm` is absent), `setitimer`,
`getitimer`, `timer_create`, `timer_delete`, `timer_settime`, `timer_gettime`,
`timer_getoverrun`, `timerfd_create`, `timerfd_settime`, `timerfd_gettime`. For
`SIGEV_THREAD`: `clone3` and the internal `SIGTIMER` (= `SIGCANCEL`).

⚑ HIST. The three itimers and `alarm` share state (`alarm` and `ITIMER_REAL` are the same
timer), so a library cannot use them without interfering with its caller.

### 23.5 Broken-down time: `gmtime`, `gmtime_r`, `localtime`, `localtime_r`, `mktime`, `timegm`, `timelocal`, `difftime`, `asctime`, `asctime_r`, `ctime`, `ctime_r`, `dysize`

**Contract.** Convert between `time_t` (seconds since the epoch, ignoring leap seconds) and
broken-down time (`struct tm`: second 0–60, minute, hour, day of month, month 0–11,
year − 1900, weekday, day of year, DST flag, plus the GNU fields `tm_gmtoff` and `tm_zone`).

* **`gmtime`** converts to UTC. **`localtime`** converts to the local time zone (§23.6).
* **`mktime(&tm)`** is the inverse of `localtime`. It normalizes out-of-range fields (for
  example day 32 → next month) and resolves `tm_isdst = -1` automatically.
* **`timegm`** (2.x, standardized in C23) is the inverse of `gmtime`. **`timelocal`** is an
  alias of `mktime`. ⚑ HIST.
* **`difftime(t1, t0)`** returns `t1 - t0` as `double`.
* **`asctime`/`ctime`** produce `"Sun Sep 16 01:03:52 1973\n"`.
* **`dysize(y)`** returns 365 or 366. ⚑ HIST.

**Guarantees.** The `_r` forms write to caller storage. The others return a pointer to a
single static `struct tm` or buffer, shared between `gmtime` and `localtime` and overwritten
by either (⚑ HIST, ⚑ PATH).

**Failure.**

* `EOVERFLOW`: the year does not fit an `int`.
* `mktime` returns -1 on error. ⚑ HIST, ⚑ ARB: -1 is also the valid time
  1969-12-31 23:59:59 UTC, so errors are ambiguous (C23 recommends checking `errno`).
* `asctime` has undefined behaviour (ISO C) for years outside 1000–9999. glibc formats years 0
  to 9999 (years below 1000 without padding) and returns `NULL` with `EOVERFLOW` for negative
  years and years above 9999 (⚑ ARB: a fixed 26-byte format). Corrected 2026-10-02 (owner) from
  black-box observation, `spec/behaviour/zones.md`.

**Kernel.** None, except that `localtime` may load the zone (§23.6).

⚑ HIST. The `tm_year` offset of 1900 and the 0-based month are 1970s conventions that
cause off-by-one bugs.

### 23.6 Time zones: `tzset`, `tzname`, `__tzname`, `timezone`, `__timezone`, `daylight`, `__daylight`

**Contract.** Establish the local time-zone rules from the `TZ` environment variable:

* a POSIX rule string (`EST5EDT,M3.2.0,M11.1.0`);
* `:path`, or a zone name, resolved as a file under `TZDIR`, default `/usr/share/zoneinfo`;
* if `TZ` is unset, the system default `/etc/localtime`.

The functions export the standard and daylight abbreviations (`tzname[2]`), the standard
offset in seconds *west* of UTC (`timezone`, ⚑ HIST: the sign convention is the opposite of
`tm_gmtoff`), and whether DST rules exist (`daylight`).

**Guarantees.** `localtime`, `mktime` and `strftime` call `tzset` implicitly (or behave as if
they did). glibc re-reads the zone file when `TZ` changes, or when `/etc/localtime` changes
and `TZ` is unset (checked by `stat`).

**Kernel.** `openat`, `read`, `fstat`, `newfstatat`. **Config:** TZif files, `TZ`, `TZDIR`,
`/etc/localtime`. ⚑ HIST: a time zone is not a kernel concept. The kernel's `timezone`
(`gettimeofday`'s `tz`) is vestigial.

⚑ HIST. The global variables `tzname`, `timezone` and `daylight` cannot express zones whose
offset has changed historically. `tm_gmtoff` and `tm_zone` per result are the functional
answer.

### 23.7 Formatting and parsing time: `strftime`, `strftime_l`, `__strftime_l`, `wcsftime`, `wcsftime_l`, `__wcsftime_l`, `strptime`, `strptime_l`, `getdate`, `getdate_r`, `getdate_err`

**Contract.**

* **`strftime(buf, max, fmt, &tm)`** formats broken-down time according to a directive
  language:
  * ISO C: `%a %A %b %B %c %d %H %I %j %m %M %p %S %U %w %W %x %X %y %Y %Z %%`;
  * POSIX and C99: `%C %D %e %F %g %G %h %n %r %R %t %T %u %V %z`;
  * `E` and `O` alternative modifiers (era, alternative digits);
  * GNU: `%k %l %P %s`, and the flags `_ - 0 ^ #` with width.

  Names come from `LC_TIME`. It returns the length, or **0 if the result does not fit**.
  ⚑ HIST, ⚑ ARB: 0 is also the correct length for an empty result, such as `"%p"` in some
  locales.
* **`strptime(s, fmt, &tm)`** parses according to the same directives. It fills only the
  fields it parses (glibc also computes `tm_wday`/`tm_yday` when possible), and returns a
  pointer past the consumed input, or `NULL`.
* **`getdate(s)`** parses `s` by trying each template in the file named by `DATEMSK`. It
  returns static storage, with errors in the global `getdate_err` (1–8). **`getdate_r`**
  returns the error. ⚑ HIST: a global error variable, and configuration through an
  environment-named file.

**Kernel.** None. `getdate` needs `openat`/`read` (the `DATEMSK` file) and `stat`.

---

## 24. Resource limits, scheduling and system information

### 24.1 Resource limits: `getrlimit`, `setrlimit`, `prlimit`, `getrlimit64`, `setrlimit64`, `prlimit64`, `ulimit`, `vlimit` (compat)

**Contract.** Get and set the soft and hard limits for a process resource:

* `RLIMIT_AS`, `CORE`, `CPU`, `DATA`, `FSIZE`, `LOCKS`, `MEMLOCK`, `MSGQUEUE`, `NICE`,
  `NOFILE`, `NPROC`, `RSS`, `RTPRIO`, `RTTIME`, `SIGPENDING`, `STACK`.
* `RLIM_INFINITY` means unlimited.
* A process may lower its hard limit, and may raise it only with `CAP_SYS_RESOURCE`.
* **`prlimit(pid, ...)`** operates on another process.
* **`ulimit(cmd, ...)`** is the SVID form: `UL_GETFSIZE` returns 512-byte blocks.
  ⚑ HIST.

**Kernel.** `prlimit64`.

### 24.2 Resource usage: `getrusage`, `vtimes` (compat)

**Contract.** Return the resources used by the process (`RUSAGE_SELF`), by its waited-for
children (`RUSAGE_CHILDREN`), or by the thread (`RUSAGE_THREAD`): user and system CPU time,
max RSS, page faults, block I/O, and voluntary and involuntary context switches. Several BSD
fields are always 0 on Linux. ⚑ KERN.

**Kernel.** `getrusage`.

### 24.3 Priorities and scheduling: `nice`, `getpriority`, `setpriority`, `sched_setscheduler`, `sched_getscheduler`, `sched_setparam`, `sched_getparam`, `sched_get_priority_min`, `sched_get_priority_max`, `sched_rr_get_interval`, `sched_yield`, `sched_setaffinity`, `sched_getaffinity`, `sched_getcpu`, `getcpu`, `CPU_*` macros, `__sched_cpucount`, `__sched_cpualloc`, `__sched_cpufree`

**Contract.**

* **`nice(inc)`** adds to the nice value and returns the new value. -1 is a valid result, so
  the caller must zero `errno`. ⚑ HIST.
* **`getpriority`/`setpriority(which, who, prio)`** apply to a process, group or user.
  ⚑ KERN: the syscall returns `20 - nice` to avoid negative returns, and glibc converts it
  back.
* **Scheduling policies:** `SCHED_OTHER`, `SCHED_BATCH`, `SCHED_IDLE`, `SCHED_FIFO`,
  `SCHED_RR`. `SCHED_DEADLINE` needs `sched_setattr`, which glibc does not wrap in 2.39.
  ⚑ KERN: the POSIX functions operate on a *thread* (the TID) on Linux, not on the
  process.
* **Affinity:** `cpu_set_t` masks, 1024 CPUs by default. Dynamically sized sets are available
  through `CPU_ALLOC`. ⚑ ARB: `cpu_set_t` is 1024 bits, and machines can have more CPUs.
  ⚑ COMPAT: `sched_setaffinity@GLIBC_2.3.3` versus the older form without a size argument.
* **`sched_getcpu()`** returns the current CPU. **`getcpu(&cpu, &node)`** (2.29) also returns
  the NUMA node.

**Kernel.** `getpriority`, `setpriority`, `sched_setscheduler`, `sched_getscheduler`,
`sched_setparam`, `sched_getparam`, `sched_get_priority_min`, `sched_get_priority_max`,
`sched_rr_get_interval`, `sched_yield`, `sched_setaffinity`, `sched_getaffinity`, the rseq
area or the vDSO `getcpu`, and `getcpu`.

### 24.4 System configuration queries: `sysconf`, `__sysconf`, `pathconf`, `fpathconf`, `confstr`, `__confstr_chk`, `getpagesize`, `get_nprocs`, `get_nprocs_conf`, `get_phys_pages`, `get_avphys_pages`, `getdtablesize`

**Contract.**

* **`sysconf(name)`** returns runtime values of system limits and options. Examples:
  * `_SC_ARG_MAX` (a quarter of `RLIMIT_STACK`, at least 128 KiB);
  * `_SC_CHILD_MAX`, `_SC_CLK_TCK`, `_SC_NGROUPS_MAX`, `_SC_OPEN_MAX`, `_SC_PAGESIZE`;
  * `_SC_NPROCESSORS_CONF`/`_ONLN`, `_SC_PHYS_PAGES`/`_AVPHYS_PAGES`;
  * `_SC_LEVEL1_DCACHE_*` (CPUID);
  * `_SC_MINSIGSTKSZ`, `_SC_SIGSTKSZ`, `_SC_THREAD_STACK_MIN`;
  * `_SC_GETPW_R_SIZE_MAX`;
  * about 250 POSIX option names.

  It returns -1 *without changing errno* for "no limit", and -1 with `EINVAL` for an unknown
  name. ⚑ HIST: three meanings share one return channel.
* **`pathconf(path, name)`/`fpathconf(fd, name)`** return file-system-dependent limits:
  `_PC_NAME_MAX`, `_PC_PATH_MAX`, `_PC_LINK_MAX` (derived from `statfs.f_type`),
  `_PC_PIPE_BUF`, `_PC_CHOWN_RESTRICTED`, `_PC_NO_TRUNC`, `_PC_FILESIZEBITS`,
  `_PC_REC_*`, `_PC_2_SYMLINKS`.
* **`confstr(name, buf, len)`** returns configuration Text: `_CS_PATH` (the default `PATH`),
  `_CS_GNU_LIBC_VERSION`, `_CS_GNU_LIBPTHREAD_VERSION`, `_CS_POSIX_V7_*_CFLAGS`. It returns
  the full length.
* **`get_nprocs()`** counts online CPUs, and **`get_nprocs_conf()`** counts configured CPUs.
  **`get_phys_pages`/`get_avphys_pages`** count memory pages.

**Kernel.**

* `get_nprocs`: `/sys/devices/system/cpu/online`, falling back to `/proc/stat`, then
  `sched_getaffinity`.
* `get_nprocs_conf`: `/sys/devices/system/cpu/` (`getdents64`), falling back to `/proc/stat`.
* Memory: `sysinfo`.
* Limits: `prlimit64`.
* `pathconf`: `statfs`, plus `/proc/sys/fs/pipe-max-size` and similar files.
* Cache sizes: CPUID.
* The auxv entries.

⚑ HIST. The CPU count comes from parsing text files. The count of CPUs *the process may use*
(`sched_getaffinity`) differs from both `_ONLN` and `_CONF`, and containers (cgroup CPU
quotas) are invisible to all three. The functional need, "how much parallelism is available
to me?", has no accurate answer in the current interface.

### 24.5 System identity: `uname`, `gethostname`, `sethostname`, `getdomainname`, `setdomainname`, `gethostid`, `sethostid`, `gnu_get_libc_version`, `gnu_get_libc_release`

**Contract.**

* **`uname(&u)`** returns the kernel name, node name, release, version, machine, and domain
  name (GNU).
* **`gethostname(buf, n)`** returns the node name. It fails with `ENAMETOOLONG` if truncated.
  ⚑ ARB: POSIX allows silent truncation.
* **`getdomainname`** returns the NIS domain name, not the DNS domain. ⚑ HIST: a common
  confusion.
* **`gethostid()`** returns a 32-bit host identifier: from `/etc/hostid` if present, otherwise
  derived from the host's IPv4 address. **`sethostid`** writes `/etc/hostid`. ⚑ HIST: the
  value is meaningless on modern systems.
* **`gnu_get_libc_version()`** returns `"2.39"`, and **`gnu_get_libc_release()`** returns
  `"stable"`.

**Kernel.** `uname` (glibc implements `gethostname` and `getdomainname` from it),
`sethostname`, `setdomainname`, `openat`/`read`/`write` (`/etc/hostid`), and the host
lookups of §16 for `gethostid`.

⚑ KERN. The `utsname` fields are 65 bytes each, a fixed kernel structure.

### 24.6 System-wide status and control: `sysinfo`, `getloadavg`, `reboot`, `klogctl`, `acct`, `quotactl`, `ioperm`, `iopl`, `syscall`, `vhangup`, `profil` (§28), `sysctl` (compat), `bdflush` (compat), `nfsservctl` (compat), `create_module`, `get_kernel_syms`, `query_module` (compat), `init_module`, `delete_module`, `uselib` (compat), `modify_ldt`

**Contract.**

* **`capget`/`capset`** read and write a thread's capability sets using the kernel's
  versioned header and data structures (`_LINUX_CAPABILITY_VERSION_3`). They are exported but
  not declared in any glibc header, and `libcap` is the intended interface. ⚑ HIST.
* **`sysinfo(&si)`** returns uptime, load averages, total, free, shared and buffer RAM, swap,
  process count, and a memory unit.
* **`getloadavg(a, n)`** returns up to three load averages, from `/proc/loadavg`.
* **`reboot(cmd)`**, **`klogctl`** (the kernel log buffer), **`acct`** (process accounting),
  **`quotactl`**, **`ioperm`/`iopl`** (x86 port I/O permission), **`vhangup`**,
  **`init_module`/`delete_module`** and **`modify_ldt`** are thin wrappers.
* **`syscall(nr, ...)`** invokes an arbitrary system call by number. It returns -1 with
  `errno` on error. This is the escape hatch for kernel interfaces glibc does not wrap
  (`futex`, `openat2`, `sched_setattr`, `io_uring_*`, `bpf`, `perf_event_open`, `clone3`,
  `landlock_*`, `seccomp`, ...).
* The compat entries are stubs failing with `ENOSYS`, or wrappers for syscalls that have been
  removed from the kernel.

**Kernel.** The syscalls of the same names. `openat`/`read` of `/proc/loadavg`.

⚑ HIST (high value). The boundary between "glibc wraps it" and "use `syscall()`" is set by
project history and by review bandwidth, not by a principle. Many important modern kernel
interfaces are reachable only through the untyped `syscall()`, without cancellation support
or 64-bit-time translation.

---

## 25. Inter-process communication

### 25.1 Pipes and FIFOs: `pipe`, `pipe2`, `mkfifo` (§13.5) [thin]

**Contract.** `pipe(fds)` creates a unidirectional byte channel: `fds[0]` reads and `fds[1]`
writes. `pipe2` also accepts `O_CLOEXEC`, `O_NONBLOCK` and `O_DIRECT` (packet mode).

**Guarantees.**

* Writes of at most `PIPE_BUF` (4096) bytes are atomic.
* A read on an empty pipe with no writers returns 0.
* A write with no readers raises `SIGPIPE` and fails with `EPIPE`.
* The capacity is adjustable with `F_SETPIPE_SZ`.

**Kernel.** `pipe2`.

### 25.2 System V message queues, semaphores and shared memory: `ftok`, `msgget`, `msgsnd`, `msgrcv`, `msgctl`, `semget`, `semop`, `semtimedop`, `semctl`, `shmget`, `shmat`, `shmdt`, `shmctl` [thin]

**Contract.**

* **Naming.** Kernel-persistent IPC objects named by a numeric key. `ftok(path, id)` derives
  a key from an inode number, a device number and an 8-bit ID. ⚑ HIST: collisions are
  possible, and the key space is 32 bits.
* **Message queues.** `msgsnd`/`msgrcv` send and receive typed messages. Receive selects by
  type (the first of that type; for a negative type, the first message of the lowest type up
  to its absolute value; or any), with `IPC_NOWAIT`, `MSG_NOERROR`,
  `MSG_EXCEPT` and `MSG_COPY`.
* **Semaphore sets.** `semop` performs an *atomic* array of operations on a set. `SEM_UNDO`
  reverses them at process exit. `semtimedop` adds a timeout. `semctl` is variadic, and its
  fourth argument is a `union semun` that **the caller must define**. ⚑ HIST.
* **Shared memory.** `shmget` creates a segment, and `shmat` attaches it at a kernel-chosen or
  requested address (`SHM_RND`, `SHM_RDONLY`, `SHM_REMAP`). `shmdt` detaches it.
* **Control.** The `*ctl` functions have `IPC_STAT`, `IPC_SET` and `IPC_RMID`, plus
  Linux-specific info commands.

**Failure.** `EACCES`, `EEXIST`, `ENOENT`, `EIDRM` (removed while waiting), `ENOSPC`
(system limits), `E2BIG`, `ENOMSG`, `EAGAIN`, `EINTR`, `ERANGE`.

**Kernel.** `msgget`, `msgsnd`, `msgrcv`, `msgctl`, `semget`, `semtimedop` (`semop` uses it),
`semctl`, `shmget`, `shmat`, `shmdt`, `shmctl`, and `newfstatat` for `ftok`.

⚑ HIST. System V IPC has its own namespace (numeric keys, not the file system), its own
permission model, and objects that persist after every user has exited. That is a leak risk
that POSIX IPC and file descriptors avoid. `SHMLBA` and the `__getshmlba` function
exist for architecture alignment rules (⚑ KERN).

### 25.3 POSIX message queues: `mq_open`, `mq_close`, `mq_unlink`, `mq_send`, `mq_timedsend`, `mq_receive`, `mq_timedreceive`, `mq_notify`, `mq_getattr`, `mq_setattr`, `__mq_open_2` (fortify) [thin]

**Contract.** Named priority queues of messages.

* A name is `"/name"`, with exactly one leading slash (⚑ KERN: the `mqueue` file-system
  namespace).
* A message has at most `mq_msgsize` bytes and a priority, and the highest priority is
  received first.
* `mq_notify` requests notification (a signal, or a new thread) when a message arrives on an
  empty queue, for a single registrant.
* Attributes: `mq_flags` (`O_NONBLOCK`), `mq_maxmsg`, `mq_msgsize`, `mq_curmsgs`.

**Failure.** `EAGAIN`, `EMSGSIZE`, `ETIMEDOUT`, `EBADF`, `EINVAL` (a name without the leading slash, or
attributes exceeding `/proc/sys/fs/mqueue/*`), `EACCES` (a further slash in the name, or
permission denied), `ENOENT` (the name `/` alone, or no such queue without `O_CREAT`),
`ENAMETOOLONG` (more than `NAME_MAX` bytes after the slash), `EBUSY` (`mq_notify` is already
registered, by any process including the caller: the kernel's rule). Corrected 2026-10-04
(three-way rule: the reference and mq_open(3) agree, `spec/behaviour/posixipc.md`; an
earlier text gave `EINVAL` for every bad name).

**Cancellation.** Send and receive are cancellation points.

**Kernel.** `mq_open`, `mq_unlink`, `mq_timedsend`, `mq_timedreceive`, `mq_notify`,
`mq_getsetattr`, `close`. For `SIGEV_THREAD` notification, glibc runs a helper thread that
receives on a netlink socket (`socket(AF_NETLINK, NETLINK_ROUTE)`, per the kernel's
`mq_notify` `SIGEV_THREAD` protocol) and spawns the notification thread.

### 25.4 POSIX shared memory: `shm_open`, `shm_unlink`

**Contract.** Create or open a named shared-memory object. Leading slashes in the name are
optional; after them the name is a single component, not `.` or `..`, with no further slash
(`EINVAL`; `ENOENT` for `shm_unlink`) and at most `NAME_MAX` bytes (`ENAMETOOLONG`). The result is
a descriptor to be sized with `ftruncate` and mapped with `mmap`. `shm_unlink` removes the name.
Written 2026-10-04 (owner ruling, phase 12-13 report), `spec/behaviour/posixipc.md`.

**Kernel.** `openat` and `unlinkat` in the tmpfs mounted at **`/dev/shm`**. ⚑ HIST: POSIX
shared memory on Linux is *not* a kernel object. It is a naming convention over a tmpfs
mount: it fails (`ENOENT`) only when `/dev/shm` does not exist, and any directory there
serves, tmpfs or not (Written 2026-10-04 (owner ruling, phase 12-13 report)). `memfd_create` is the kernel-native
anonymous equivalent.

---

## 26. System logging

### 26.1 `openlog`, `syslog`, `vsyslog`, `closelog`, `setlogmask`, `__syslog_chk`, `__vsyslog_chk`

**Contract.**

* **`openlog(ident, option, facility)`** sets a process-wide identifier prefix, options and a
  default facility. The options are:
  * `LOG_PID`, `LOG_CONS` (write to `/dev/console` on failure);
  * `LOG_NDELAY` (connect now), `LOG_ODELAY`;
  * `LOG_NOWAIT`;
  * `LOG_PERROR` (also write to stderr).
* **`syslog(priority, fmt, ...)`** formats a message (the `printf` language plus `%m`) and
  sends it to the system logger, prefixed with priority, timestamp and identifier, and with the
  PID when `LOG_PID` is set (corrected 2026-10-04, three-way rule: the reference and syslog(3)
  agree, `spec/behaviour/syslog.md`; an earlier text said the PID was always included).
* **`setlogmask(mask)`** filters priorities with `LOG_MASK`/`LOG_UPTO`.
* **`closelog`** closes the connection.

**Guarantees.**

* Messages below the mask are discarded without being formatted.
* If the connection fails, the message is retried once after reconnecting.
* With `LOG_CONS`, a message that still cannot be delivered is written to the console.
* It never reports failure to the caller.

**Kernel.** `socket(AF_UNIX, SOCK_DGRAM, then SOCK_STREAM)`, `connect`, `send`, `close`,
`openat`/`write` (`/dev/console`), `clock_gettime`, `getpid`. **Config:** `/dev/log`, a Unix
socket served by syslogd or journald, not by the kernel.

⚑ HIST.

* `ident` is stored **by pointer**, so the caller's string must outlive the logging session.
* The connection and settings are process-global: libraries calling `openlog` override the
  application's settings.
* The wire format is the unstructured RFC 3164-style text. Structured logging (RFC 5424,
  journald fields) is not expressible.
* The need, "emit a structured event to the system log", deserves an explicit handle.

---

## 27. Dynamic linking and loading

### 27.1 Loading and symbol lookup: `dlopen`, `dlmopen`, `dlclose`, `dlsym`, `dlvsym`, `dlerror`, `dladdr`, `dladdr1`, `dlinfo`, `_dl_find_object`, `dl_iterate_phdr`, `__libc_dlopen_mode`... (internal), `_dl_mcount_wrapper`, `_dl_mcount_wrapper_check`

**Contract.**

* **`dlopen(path, mode)`** loads an ELF shared object and its dependencies into the process
  and runs their constructors. It returns a handle, and repeated opens are reference-counted.
  * **Path search** applies when `path` contains no slash: `DT_RPATH`, `LD_LIBRARY_PATH`,
    `DT_RUNPATH`, `/etc/ld.so.cache`, then the default directories.
  * **Modes:** `RTLD_LAZY`/`RTLD_NOW` (binding time), `RTLD_GLOBAL`/`RTLD_LOCAL` (symbol
    visibility to later loads), `RTLD_NODELETE`, `RTLD_NOLOAD` (only return an existing
    handle), `RTLD_DEEPBIND` (prefer the object's own symbols).
  * **`dlopen(NULL)`** returns the global scope.
* **`dlmopen(lmid, ...)`** loads into a separate *link-map namespace* (`LM_ID_NEWLM`), giving
  an isolated copy of the libraries.
* **`dlsym(handle, name)`** returns the address of a symbol. The special handles are
  `RTLD_DEFAULT` (global lookup order) and `RTLD_NEXT` (the next occurrence after the caller's
  object, for interposition). **`dlvsym`** requests a specific symbol version.
* **`dlclose`** decrements the reference count. At zero, it runs destructors and unmaps the
  object, unless the object is `NODELETE` or has unique symbols or TLS in use.
* **`dlerror()`** returns a Text describing the last failure on this thread, and clears it.
  It returns `NULL` if there was no error. ⚑ HIST: this is yet another error channel.
  `dlsym` returning `NULL` is ambiguous for symbols whose value is 0, so callers must check
  `dlerror`.
* **`dladdr(addr, &info)`** / **`dladdr1`** map an address to its object and nearest symbol.
* **`dlinfo(handle, request, arg)`** queries a handle: its link map, origin, search paths,
  TLS module ID and block, and namespace.
* **`dl_iterate_phdr(cb, data)`** calls `cb` for each loaded object, with its load address and
  program headers. The loader lock is held during the callback.
* **`_dl_find_object(pc, &res)`** (2.35) finds an object and its `PT_GNU_EH_FRAME` for a
  program counter without taking locks. It exists for unwinders.

**Guarantees.** Symbol resolution follows ELF rules (global scope order, symbol versioning,
`STB_GNU_UNIQUE`, IFUNC resolvers run at bind time). Constructors run in dependency order.

**Failure.** `NULL` plus `dlerror` text: file not found, wrong ELF class, undefined symbol
(with `RTLD_NOW`), static TLS exhaustion, or a version mismatch.

**Kernel.** `openat`, `read`, `fstat`, `mmap` (segment mapping, `MAP_FIXED`/
`MAP_FIXED_NOREPLACE` inside a reservation), `mprotect` (RELRO, text relocation), `munmap`,
`close`, `madvise`, and `prctl`/`arch_prctl` (CET marking). **Config:**
`/etc/ld.so.cache`, `/etc/ld.so.preload`, `LD_LIBRARY_PATH`, `LD_PRELOAD`, `LD_BIND_NOW`,
`LD_DEBUG` (ignored in secure mode), and the glibc-hwcaps subdirectories.

⚑ HIST. `libdl.so.2` is an empty placeholder since 2.34 (⚑ COMPAT).
⚑ HIST. The global loader lock serializes `dlopen`, `dlclose`, `dl_iterate_phdr` and TLS
allocation, so a callback or constructor that calls back into the loader can deadlock.

### 27.2 Auditing and profiling interface (rtld-audit): `la_version`, `la_objsearch`, `la_objopen`, `la_preinit`, `la_symbind64`, `la_x86_64_gnu_pltenter`, `la_x86_64_gnu_pltexit`, `la_objclose`, `la_activity` (implemented *by* audit libraries); `LD_AUDIT`, `LD_PROFILE`, `_dl_mcount`

**Contract.** Libraries named in `LD_AUDIT` (or `DT_AUDIT`) receive callbacks from the dynamic
linker for library search, object load and unload, symbol binding (they can redirect
bindings), and PLT entry and exit.

**Kernel.** As for §27.1.

### 27.3 Thread-local storage runtime: `__tls_get_addr`, `___tls_get_addr` (i386), `_dl_tlsdesc_*` (internal)

**Contract.** Return the address of a TLS variable, given a module ID and an offset, for the
calling thread. The block is allocated on first access for modules loaded by `dlopen`.

**Failure.** None. Allocation failure aborts the process. ⚑ ARB: there is no error channel
for a lazy allocation.

**Kernel.** `mmap`/`brk` (through malloc).

### 27.4 ELF helper data and entry points: `_r_debug`, `_dl_debug_state`, `r_debug_extended` (debugger protocol), `__libc_enable_secure`, `_dl_argv`, `__libc_stack_end`, `_rtld_global`, `_rtld_global_ro` (GLIBC_PRIVATE)

**Contract.** `_r_debug` and `_dl_debug_state` implement the debugger rendezvous. The debugger
sets a breakpoint on `_dl_debug_state` and walks the `r_debug.r_map` list after each change.
Since 2.35 `r_version` is 2, which adds a linked list of namespaces.

⚑ HIST. The debugger protocol is a global data structure plus a breakpoint convention, which
has been extended in place for 30 years.

### 27.5 Static-link and CPU-feature support: `__x86_get_cpuid_feature_leaf`, `__x86_cpu_features` (via `<sys/platform/x86.h>`: `CPU_FEATURE_PRESENT`, `CPU_FEATURE_ACTIVE`), `__get_cpu_features` (old), IFUNC resolvers

**Contract.** Report CPU features as detected by the dynamic linker (CPUID plus OS support via
XCR0, masked by `glibc.cpu.hwcaps` tunables): "present" means the CPU supports the feature,
and "active" means it is also usable.

**Kernel.** CPUID/XGETBV instructions, auxv `AT_HWCAP`/`AT_HWCAP2`, and `arch_prctl`
(AMX permission requests).

---

## 28. Profiling and instrumentation

### 28.1 `gprof` support: `monstartup`, `_mcleanup`, `moncontrol`, `mcount`, `_mcount`, `__monstartup`, `__cyg_profile_func_enter`, `__cyg_profile_func_exit`, `__profile_frequency`, `profil`, `sprofil`, `gmon` output

**Contract.**

* **`monstartup(lowpc, highpc)`** allocates histogram and arc tables for a text range and
  starts PC sampling. `_mcleanup` writes `gmon.out` (or `$GMON_OUT_PREFIX.<pid>`) at exit.
* **`mcount`/`_mcount`** is called at every function entry of `-pg` code and records the
  call arc.
* **`profil(buf, size, offset, scale)`** samples the PC into a histogram on every
  `ITIMER_PROF` tick. **`sprofil`** takes multiple regions.
* **`__cyg_profile_func_enter/_exit`** are empty hooks called by
  `-finstrument-functions` code, for the application to interpose.

**Kernel.** `setitimer(ITIMER_PROF)`, `rt_sigaction(SIGPROF)`, `openat`, `write`
(`gmon.out`), `mmap` (tables).

⚑ HIST. PC-sampling profiling through signals has been superseded by `perf_event_open`, which
glibc does not wrap.

### 28.2 Memory-usage tracing and debugging (`libc_malloc_debug.so`, `libmemusage.so`)

See §3.3. `libmemusage.so` (`LD_PRELOAD`) records allocation statistics and writes a profile
at exit. ⚑ HIST.

### 28.3 Instrumentation entry points emitted by compilers: `__fentry__`, `__stack_chk_fail`, `__chk_fail`, `__*_chk` (C0.5), `__cxa_*` (§1.2), `__gcc_personality_v0` (not glibc)

**Contract.** `__fentry__` is the profiling counter call that `-pg -mfentry` code makes before the function prologue; it records the call arc as `mcount` does (§28.1). The others are
covered in their sections.

---

## 29. Legacy and compatibility subsystems

This section covers interfaces exported **only** for binary compatibility: they can no longer
be linked against with current headers. It also covers subsystems that are functional but
obsolete. A reimplementation that aims at ABI compatibility with existing binaries must
provide them. One that aims only at source compatibility may omit them.

### 29.1 ONC (Sun) RPC and XDR: `clnt_*`, `svc_*`, `xdr_*` (≈120 functions), `auth*`, `pmap_*`, `rpc_*`, `callrpc`, `registerrpc`, `get_myaddress`, `bindresvport`, `key_*`, `netname2user`, `user2netname`, `host2netname`, `getnetname`, `des_setparity`, `cbc_crypt`, `ecb_crypt`, `passwd2des`, `xencrypt`, `xdecrypt`, `getpublickey`, `getsecretkey`, `_rpc_dtablesize`, `svc_fdset`, `svc_pollfd`, `svc_max_pollfd`, `rpc_createerr`, `__rpc_thread_*`, `_null_auth`, `_authenticate`, `_seterr_reply`, `rtime`, `svcraw_create`, `clntraw_create`...

**Contract.**

* **XDR** serializes C data to and from the External Data Representation (RFC 4506): encode,
  decode and free, for primitive types, arrays, strings, unions, pointers, and
  record-marking streams.
* **The RPC runtime** provides client handles over UDP, TCP and Unix sockets, server
  dispatch, portmapper registration, and authentication flavours (`AUTH_NONE`,
  `AUTH_UNIX`/`AUTH_SYS`, `AUTH_DES`).

**Status in 2.39.** Compat only. It was deprecated in 2.26, when the headers became opt-in
through `--enable-obsolete-rpc`. That option was removed in 2.32, so new programs cannot be
built against glibc's RPC. The replacement is `libtirpc`. The symbols remain for old binaries.

**Kernel.** Sockets (§15), `poll`/`select`, `getpid`, `gettimeofday`.

⚑ HIST, ⚑ COMPAT. Process-global server state (`svc_fdset`, a 1024-bit `fd_set`), DES
encryption, and address-based trust.

### 29.2 NIS / YP client (`libnsl`): `yp_*`, `nis_*` (≈120 functions), `__yp_check`, `xdr_yp*`, `xdr_nis*`, `nis_sperrno`, `nis_print_*`, ...

**Contract.** Client functions for Sun's Network Information Service (`yp_match`, `yp_first`,
`yp_next`, `yp_all`, `yp_order`, `yp_master`, `yp_get_default_domain`, `yp_bind`, ...) and
for NIS+.

**Status.** `libnsl.so.1` is compat only since 2.32, when `--enable-obsolete-nsl` was removed.
Its headers are not installed, and the `nis`/`nisplus` NSS modules are no longer built. The
replacement is the separate `libnsl` 2.x package. The NIS+ client symbols remain as compat
symbols.

**Kernel.** Sockets via RPC.

### 29.3 STREAMS stubs: `getmsg`, `putmsg`, `getpmsg`, `putpmsg`, `isastream`, `fattach`, `fdetach`

**Contract.** None. These always fail with `ENOSYS`. `isastream` returns 0 for valid
descriptors. ⚑ COMPAT: they were added for XSI conformance. Linux never implemented
STREAMS.

### 29.4 Obsolete regular-expression and misc interfaces: `advance`, `step`, `loc1`, `loc2`, `locs` (`<regexp.h>`, removed 2.28), `re_comp`, `re_exec` (§14.9), `cuserid`, `getpass`, `getwd`, `gets`, `mktemp`, `tmpnam` (as noted in their sections), `ecvt`/`fcvt`/`gcvt`, `sigvec`, `sigstack`, `sstk`, `revoke`, `gtty`, `stty`, `ustat`, `stime`, `ftime`, `pow10`, `matherr`, `_LIB_VERSION`, `__malloc_hook` and friends, `malloc_get_state`, `pthread_kill_other_threads_np`, `pthread_mutexattr_*_np` aliases, `sys_errlist`, `sys_siglist`, `h_errlist`, `_sys_nerr`, `re_max_failures`, `nfsservctl`, `create_module`, `query_module`, `get_kernel_syms`, `uselib`, `bdflush`, `sysctl`, `__sysctl`, `vtimes`, `vlimit`, `chflags`, `fchflags`, `lchmod` (behaves as documented), `inet6_option_*`, `__xstat`/`__xmknod` family, `_IO_*` (§12.13), `__strtol_internal` family, `__*_c1..3` string inlines, `__ctype32_*`, `__libc_malloc` aliases, `__libc_freeres`

**Contract.** Each keeps the contract it had when it was removed. Most are aliases of current
functions, or stubs that fail with `ENOSYS`. Three are special:

* **`__libc_freeres()`** releases all of libc's internal allocations so that leak checkers
  (Valgrind) report cleanly.
* **`__libc_malloc` and friends** are aliases of `malloc` that cannot be interposed.
* **`sysctl`/`__sysctl`** fail with `ENOSYS`, because the kernel removed the syscall in 5.5.

⚑ COMPAT. The existence of this list is the strongest argument that a clean-contract
reimplementation should separate an "ABI shim" layer from the core library.

---

## 30. Catalogue of historical accidents and path dependencies

This section consolidates the ⚑ annotations into the design-level patterns behind them, ranked
roughly by how much of the interface they shape. For each pattern it gives the underlying
functional need, and whether the kernel ABI forces the current shape at the boundary.

| # | Pattern | Where it appears | Functional need | Kernel-forced? |
|---|---|---|---|---|
| H1 | **NUL-terminated strings as the universal text type** | §4 (all), §6, §12, §13, §14, §15, every Text parameter | A delimited byte sequence | **Only at the syscall boundary** (paths, `execve` argv/envp, xattr names, `mount` data, `sethostname` is length-based). Everywhere else it is a choice. It drives the `n`/`l`/`_chk` variant explosion, O(n) length, and embedded-NUL truncation (`fgets`, `regexec`, `strcoll`). |
| H2 | **`errno` as an out-of-band, thread-local error channel** | C0.1, nearly every function | A per-call result that is either a value or an error | No. The kernel returns `-errno` in the result register. Four further error conventions grew alongside it: returned error numbers (pthreads, `posix_*`), `h_errno` (§16.3), `EAI_*` (§16.2), and `dlerror` (§27.1). Ambiguous sentinels (`strtol`, `readdir`, `getpwnam`, `mktime` -1, `nice` -1, `getauxval` 0, `ptrace` PEEK, `sysconf` -1) force "zero errno before calling". |
| H3 | **Static result buffers and hidden global cursors, with `_r` twins** | `strtok`, `asctime`, `ctime`, `gmtime`, `localtime`, `getpw*`, `getgr*`, `getsp*`, `gethostby*`, `getservby*`, `inet_ntoa`, `ether_ntoa`, `ttyname`, `ptsname`, `ecvt`, `l64a`, `getmntent`, `strerror`, `getlogin`, `hsearch`, `getdate`, `re_comp`, `getopt`, `lgamma`/`signgam`, `mblen`/`mbtowc`, `rand`, `seed48`, the `*ent` enumerators | Return a caller-owned result, and keep iteration state in a caller-owned object | No. The `_r` form is always the real contract. About 70 entry points exist only because of this pattern. |
| H4 | **Process-global ambient state that affects pure computations** | Locale (`LC_NUMERIC` changing `printf`/`strtod`, `LC_CTYPE` changing `isalpha`, `strcoll`), time zone (`tzname`, `timezone`), `printf` registration, `re_syntax_options`, `_res` | An explicit parameter (format, locale, zone) | No. It led to the `_l` twin of about 80 functions (§7.1). |
| H5 | **Several delimiting conventions per operation** | `mem*`/`str*`/`strn*`/`strl*`, `wcs*`/`wmem*`, `fgets`/`getline`, `readlink` (no NUL) | One length-delimited form | Only `readlink`'s no-terminator behaviour is kernel-defined. |
| H6 | **Fixed-size ABI structures that cap functional limits** | `fd_set` (1024 fds), `sigset_t` (1024 bits vs the kernel's 64), `cpu_set_t` (1024 CPUs), `struct sockaddr` (14 bytes), `utmp` fields and 32-bit `ut_tv`, `_res` (3 servers, 6 search domains), `struct dirent` `d_name[256]`, `mbstate_t` (8 bytes), `jmp_buf` (embeds `sigset_t`), `FILE` layout, pthread object sizes, `mallinfo` `int` fields, `printf` `int` return | Size-agnostic handles | Partly. `sockaddr_*`, `IFNAMSIZ`, `TASK_COMM_LEN`, `utsname`, `NAME_MAX`/`PATH_MAX` per syscall, and `struct stat` are kernel ABI. `fd_set`, glibc's `sigset_t`, `cpu_set_t`, `utmp`, `_res`, `mbstate_t` and `FILE` are glibc's own choices, frozen by the ABI. |
| H7 | **Symbol versioning to preserve superseded contracts** | `memcpy@2.2.5` (overlap-safe), `pthread_cond_*@2.2.5`, `glob@2.2.5`, `fmemopen@2.2.5`, `sched_setaffinity@2.3.3`, `realpath@2.2.5`, `__isoc99_*scanf` vs `scanf` vs `__isoc23_*`, `strerror_r` XSI vs GNU, `basename` GNU vs XPG, `sigpause` BSD vs XSI, `signal` BSD vs SysV, `getopt` vs `__posix_getopt`, `qsort_r` argument order | One contract per name | No. A new library must decide per name whether to carry both. |
| H8 | **Dead or superseded subsystems kept for ABI** | §29 (RPC, NIS, STREAMS, `<regexp.h>`, malloc hooks, `sys_errlist`, removed syscalls), `libpthread`/`libdl`/`librt`/`libutil`/`libanl` placeholders | None | No. They argue for a separable compat shim. |
| H9 | **POSIX process-wide semantics over per-thread kernel primitives** | `setuid` family (§17.2: signal broadcast via `SIGSETXID`), `sched_*` operating on threads, `sigprocmask` in threads, `pthread_cancel` (§22.3, `SIGCANCEL`), `SIGEV_THREAD` timers and `mq_notify` (helper threads), `system` (process-wide signal dispositions), `fork` in threaded programs (§18.2) | The POSIX contract | **Yes, the mismatch is forced.** Linux credentials and scheduling are per-task. Any implementation of the POSIX contract must emulate process-wide scope. The emulation costs two reserved real-time signals and makes `SIGRTMIN` a runtime value. |
| H10 | **Variadic and untyped interfaces** | `printf`/`scanf` (run-time format language), `open` (optional mode), `fcntl`, `ioctl`, `prctl`, `ptrace`, `semctl` (caller-defined `union semun`), `mremap`, `execl*`, `makecontext`, `syscall` | Typed operations | `ioctl`, `fcntl`, `prctl` and `ptrace` are untyped *in the kernel*. `printf`/`scanf`/`open`/`execl`/`makecontext` are library choices. |
| H11 | **Configuration by text files and environment, loaded implicitly** | NSS (`/etc/nsswitch.conf` + `dlopen` of modules), resolver (`/etc/resolv.conf`), time zones (`TZ`, `/etc/localtime`), locales (`LANG`, archive), gconv, `/etc/gai.conf`, `/etc/hosts`, `/etc/shells`, `/etc/ttys`, `GLIBC_TUNABLES`, `MALLOC_*`, `LD_*`, `DATEMSK`, `NLSPATH`, `POSIXLY_CORRECT`, `MSGVERB` | Configurable policy | No. None of this is kernel. It also makes "static" binaries depend on installed shared objects (NSS, gconv). |
| H12 | **Interfaces that embed insecure designs** | `gets`, `sprintf`, `strcpy`/`strcat`, `tmpnam`/`mktemp`, `access` (check-then-use), `rcmd`/`ruserok` (address trust), `system`/`popen` (shell injection), `%n`, `getpass`, `crypt` (removed), `wordexp` (command substitution), `inet_aton` leniency, `select` fd overflow | The underlying need, done safely | No. |
| H13 | **Ancestral duplicates from different Unix families** | `bcopy`/`memmove`, `bzero`/`memset`, `index`/`strchr`, `rindex`/`strrchr`, `bcmp`, `ffs`, `drem`/`remainder`, `gamma`/`lgamma`, `finite`/`isfinite`, `timelocal`/`mktime`, `setbuf`/`setbuffer`/`setlinebuf`/`setvbuf`, `sigset`/`sigaction`, `err`/`error`, `utmp`/`utmpx`, `on_exit`/`atexit`, `getpgrp` BSD vs SysV, `wait3`/`wait4`, `ftw`/`nftw`/`fts`, `gethostby*`/`getaddrinfo`, `random`/`rand`/`drand48`/`arc4random` | One name per need | No. |
| H14 | **Type-suffix explosion for lack of generics** | `libm` (7 floating types × ~140 operations), narrowing arithmetic (≈150 symbols for 6 operations), `<stdbit.h>` (5 widths × 14), `strto*` (7 integer and 7 floating types), `abs`/`labs`/`llabs`/`imaxabs` | Generic numeric operations | No. It is a C-language limitation. |
| H15 | **Retrofitted threading** | `errno` as a macro, `_r` functions, `flockfile` plus `*_unlocked`, `__fsetlocking`, cancellation points (§22.3), `atfork` handlers, `__libc_single_threaded`, `rand` locking, `environ` races, `setlocale` vs `uselocale` | Thread-safe by construction | No. |
| H16 | **Offsets and positions as implicit shared state** | File offset shared across `dup`/`fork`/threads (§11.4), stdio position, directory position cookies (§13.2) | Explicit positions (`pread`/`pwrite`) | The shared offset is kernel ABI for the descriptor. Using it is a choice. |
| H17 | **Kernel interfaces not wrapped, or only through `syscall()`** | `futex`, `openat2`, `clone3`, `sched_setattr`, `io_uring_*`, `io_submit`, `perf_event_open`, `bpf`, `seccomp`, `landlock_*`, `statmount`/`listmount`, `memfd_secret`, `userfaultfd`, `pidfd` process-wide `waitid`, `TCSETS2` speeds, `getdents64` (wrapped only in 2.30), `gettid` (2.30) | Access to the full kernel ABI with the library's error, cancellation and time conventions | By definition, the need is kernel-defined. The gap is history and review bandwidth. |
| H18 | **Signals as the notification mechanism for everything asynchronous** | `alarm`/itimers, POSIX timers, AIO completion, `mq_notify`, `SIGCHLD`, `SIGPIPE` on broken pipes, `SIGIO`, `SIGPROF` profiling, `lckpwdf` and `utmp` lock timeouts via `SIGALRM` | Event delivery | The signals are kernel-defined. The kernel also offers descriptor-based alternatives (`timerfd`, `signalfd`, `pidfd`, `eventfd`, `io_uring`), which the older glibc interfaces predate and do not use. |

### 30.1 Constraints that *look* historical but are kernel-forced

A reimplementation cannot remove these, because the kernel ABI imposes them:

* Paths and `execve` arguments are NUL-terminated, and a single path is limited to `PATH_MAX`
  (4096) bytes.
* Descriptors are small non-negative integers, and allocation returns the lowest free one.
* `EINTR` and restart semantics.
* The x86_64 signal restorer (`SA_RESTORER` + `rt_sigreturn` trampoline).
* One rseq registration per thread.
* The `struct stat`, `sockaddr_*`, `siginfo_t`, `utsname` and `termios` (kernel form) layouts.
* The per-call I/O cap of 0x7ffff000 bytes.
* `readlink` not terminating its output.
* `TASK_COMM_LEN` (16) for thread names, `IFNAMSIZ` (16), and `XATTR_NAME_MAX` (255).
* Per-thread credentials (the kernel side of H9).
* The 8-bit exit status visible through `wait`.
* Page granularity for `mmap` and friends.
* `MAP_FAILED` rather than `NULL`, because address 0 is mappable.
* The `getpriority` return bias (20 - nice), handled inside the wrapper.

---

## Appendix A. Kernel primitive catalogue (Linux x86_64)

The minimum kernel surface that the functional contracts in this document require. It is
grouped by kind, and each entry lists the sections whose contracts depend on it. "Wrapper"
means glibc also exports a function that exposes the primitive nearly directly.

### A.1 System calls

| Area | System calls | Needed by |
|---|---|---|
| **Process lifecycle** | `execve`, `execveat`, `exit`, `exit_group`, `clone`, `clone3`, `wait4`, `waitid`, `getpid`, `getppid`, `gettid`, `set_tid_address` | §1, §18, §22 |
| **Process descriptors** | `pidfd_open`, `pidfd_getfd`, `pidfd_send_signal` | §18.5 |
| **Credentials** | `getuid`, `geteuid`, `getgid`, `getegid`, `getresuid`, `getresgid`, `setuid`, `setgid`, `setreuid`, `setregid`, `setresuid`, `setresgid`, `setfsuid`, `setfsgid`, `getgroups`, `setgroups`, `capget`, `capset` | §17, §13.7 |
| **Sessions and groups** | `setsid`, `getsid`, `setpgid`, `getpgid`, `getpgrp` | §18.8, §19 |
| **Signals** | `rt_sigaction`, `rt_sigprocmask`, `rt_sigreturn` (via the restorer), `rt_sigsuspend`, `rt_sigpending`, `rt_sigtimedwait`, `rt_sigqueueinfo`, `rt_tgsigqueueinfo`, `kill`, `tgkill`, `sigaltstack`, `signalfd4`, `pause` | §20, §1.2 (`abort`), §17.2 (`SIGSETXID`), §22.3 (`SIGCANCEL`), §23.4 |
| **Memory** | `brk`, `mmap`, `munmap`, `mprotect`, `mremap`, `madvise`, `msync`, `mincore`, `mlock`, `mlock2`, `munlock`, `mlockall`, `munlockall`, `remap_file_pages`, `memfd_create`, `pkey_alloc`, `pkey_free`, `pkey_mprotect`, `process_madvise`, `process_mrelease`, `process_vm_readv`, `process_vm_writev`, `map_shadow_stack` | §3, §11.10, §22.1, §27, §21.2 (CET) |
| **Thread support** | `futex` (`WAIT`, `WAKE`, `WAIT_BITSET`, `REQUEUE`/`CMP_REQUEUE`, `LOCK_PI`, `LOCK_PI2`, `UNLOCK_PI`, `TRYLOCK_PI`, `WAIT_REQUEUE_PI`, `CMP_REQUEUE_PI`), `set_robust_list`, `rseq`, `arch_prctl(ARCH_SET_FS)`, `sched_yield` | §22, §12.9 (stream locks), §3 (allocator locks), and every internal lock |
| **File descriptors** | `openat`, `open_by_handle_at`, `name_to_handle_at`, `close`, `close_range`, `dup`, `dup2`/`dup3`, `fcntl`, `ioctl`, `pipe2` | §11, §12, §13, §19, §25 |
| **Data transfer** | `read`, `write`, `pread64`, `pwrite64`, `readv`, `writev`, `preadv`, `pwritev`, `preadv2`, `pwritev2`, `lseek`, `sendfile`, `copy_file_range`, `splice`, `tee`, `vmsplice`, `readahead` | §11.3, §11.4, §11.9, §12 |
| **Multiplexing and events** | `pselect6`, `ppoll`, `epoll_create1`, `epoll_ctl`, `epoll_pwait`, `epoll_pwait2`, `eventfd2`, `inotify_init1`, `inotify_add_watch`, `inotify_rm_watch`, `fanotify_init`, `fanotify_mark` | §11.7, §11.12 |
| **Durability and space** | `fsync`, `fdatasync`, `sync`, `syncfs`, `sync_file_range`, `fadvise64`, `fallocate`, `ftruncate`, `truncate` | §11.8 |
| **Locks** | `flock`, `fcntl(F_SETLK/F_SETLKW/F_OFD_*)` | §11.5, §11.13, §17.4, §17.6 |
| **Namespace and metadata** | `newfstatat`, `fstat`, `statx`, `statfs`, `fstatfs`, `getdents64`, `getcwd`, `chdir`, `fchdir`, `chroot`, `mkdirat`, `mknodat`, `linkat`, `symlinkat`, `readlinkat`, `unlinkat`, `renameat2`, `fchmodat`, `fchmodat2`, `fchmod`, `fchownat`, `fchown`, `umask`, `faccessat`, `faccessat2`, `utimensat`, the twelve `*xattr` calls | §13, §12.2, §14.8, §27 |
| **Mounting** | `mount`, `umount2`, `pivot_root`, `fsopen`, `fsconfig`, `fsmount`, `fspick`, `move_mount`, `open_tree`, `mount_setattr`, `swapon`, `swapoff`, `quotactl` | §13.12, §13.14, §24.6 |
| **Sockets** | `socket`, `socketpair`, `bind`, `listen`, `accept4`, `connect`, `shutdown`, `getsockname`, `getpeername`, `getsockopt`, `setsockopt`, `sendto`, `recvfrom`, `sendmsg`, `recvmsg`, `sendmmsg`, `recvmmsg` | §15, §16 (DNS, nscd, netlink, gai sorting), §26, §25.3 |
| **Time** | `clock_gettime`, `clock_getres`, `clock_settime`, `clock_adjtime`, `clock_nanosleep`, `gettimeofday`, `settimeofday`, `time`, `times`, `setitimer`, `getitimer`, `alarm`, `timer_create`, `timer_settime`, `timer_gettime`, `timer_getoverrun`, `timer_delete`, `timerfd_create`, `timerfd_settime`, `timerfd_gettime` | §23, §9, §13.9, §26 |
| **Scheduling and resources** | `sched_setscheduler`, `sched_getscheduler`, `sched_setparam`, `sched_getparam`, `sched_get_priority_min`, `sched_get_priority_max`, `sched_rr_get_interval`, `sched_setaffinity`, `sched_getaffinity`, `getcpu`, `getpriority`, `setpriority`, `prlimit64`, `getrusage`, `sysinfo` | §24, §22.8 |
| **IPC** | `msgget`, `msgsnd`, `msgrcv`, `msgctl`, `semget`, `semtimedop`, `semctl`, `shmget`, `shmat`, `shmdt`, `shmctl`, `mq_open`, `mq_unlink`, `mq_timedsend`, `mq_timedreceive`, `mq_notify`, `mq_getsetattr` | §25 |
| **Randomness** | `getrandom` | §9.1, §13.9, §12.12 |
| **System identity and control** | `uname`, `sethostname`, `setdomainname`, `reboot`, `syslog` (`klogctl`), `acct`, `ioperm`, `iopl`, `modify_ldt`, `init_module`, `finit_module`, `delete_module`, `vhangup`, `personality`, `prctl`, `unshare`, `setns`, `ptrace` | §24.5, §24.6, §18.9 |

### A.2 Kernel interfaces other than system calls

| Interface | Needed by |
|---|---|
| **Initial process stack**: argc, argv, envp and the auxiliary vector | §1.1, §1.3, §1.5 |
| **Auxiliary vector entries**: `AT_PAGESZ`, `AT_RANDOM` (stack protector and pointer guard seeds), `AT_SECURE`, `AT_SYSINFO_EHDR` (vDSO), `AT_HWCAP`/`AT_HWCAP2`, `AT_MINSIGSTKSZ`, `AT_PHDR`/`AT_PHNUM`/`AT_ENTRY`/`AT_BASE`, `AT_EXECFN`, `AT_PLATFORM`, `AT_CLKTCK`, `AT_*UID`/`AT_*GID` | §1, §11.10, §20.7, §24.4, §27 |
| **vDSO**: `clock_gettime`, `clock_getres`, `gettimeofday`, `time`, `getcpu` | §23.1, §24.3 |
| **Signal frame and restorer**: the kernel-built `rt_sigframe`, `ucontext_t`/`siginfo_t` layout, and `SA_RESTORER` | §20, §21 |
| **Thread pointer register** (FS base) and TLS layout expectations | §22.6, §27.3 |
| **Shared memory protocols**: futex words, the robust list, `clear_child_tid`, and the rseq area | §22 |
| **CPU instructions requiring OS support**: XSAVE/XGETBV (state size for signal frames), RDPKRU/WRPKRU (protection keys), CET shadow stack (`incssp`, `rstorssp`), CPUID | §10.6, §11.10, §21, §27.5 |
| **`/proc`**: `/proc/self/fd/` (closefrom fallback, `ttyname`, `fexecve` fallback, `lchmod`), `/proc/self/fdinfo/` (`pidfd_getpid`), `/proc/self/maps` (`pthread_getattr_np`), `/proc/self/mounts` (mount tables, `statvfs` fallback), `/proc/self/loginuid` (`getlogin`), `/proc/self/task/<tid>/comm` (thread names), `/proc/stat` (CPU counts), `/proc/loadavg`, `/proc/sys/fs/pipe-max-size`, `/proc/sys/kernel/ngroups_max` | §11.2, §13, §17.5, §19.3, §22, §24 |
| **`/sys`**: `/sys/devices/system/cpu/online` and the `cpu*` directories, `/sys/kernel/mm/transparent_hugepage/*` | §24.4, §3.1 |
| **Device nodes**: `/dev/ptmx`, `/dev/pts/*`, `/dev/tty`, `/dev/null`, `/dev/console`, `/dev/urandom`/`/dev/random` (fallback) | §1.1, §9.1, §17.5, §19, §26 |
| **Netlink `NETLINK_ROUTE`**: `RTM_GETLINK`, `RTM_GETADDR`, and the `mq_notify` `SIGEV_THREAD` protocol | §15.4, §16.2, §25.3 |
| **Kernel error numbers** and the `-errno` return convention | C0.1 |

### A.3 Mount and system conventions that are *not* kernel ABI (not inviolable)

These are often mistaken for kernel interfaces. A new design is free to replace them, subject
to system integration:

* `/dev/shm` (a tmpfs mount used for POSIX shared memory and named semaphores);
* `/dev/log` (a syslog daemon's socket);
* `/etc/*` configuration: `nsswitch.conf`, `passwd`, `group`, `shadow`, `gshadow`, `hosts`,
  `resolv.conf`, `gai.conf`, `host.conf`, `services`, `protocols`, `rpc`, `networks`,
  `ethers`, `netgroup`, `aliases`, `shells`, `ttys`, `fstab`, `mtab`, `localtime`, `hostid`,
  `ld.so.cache`, `ld.so.preload`, `ld.so.conf`;
* `/usr/share/zoneinfo`, `/usr/lib/locale/locale-archive`, the gconv module directory, the
  message catalog directories;
* `/var/run/utmp`, `/var/log/wtmp`, `/var/run/nscd/socket`;
* `/bin/sh` (used by `system`, `popen`, `wordexp`, and the `execvp` `ENOEXEC` fallback).

---

## Appendix B. Exported symbol index

Every name exported by the glibc 2.39 x86_64 libraries, grouped by the section of this
document that specifies it. The data comes from `sysdeps/unix/sysv/linux/x86_64/64/*.abilist`,
and each name is mapped to the section of the installed header that declares it. The markers are:

* `@2.xx` gives the symbol version in which the name first appeared. Names without a marker date
  from the x86_64 baseline, `GLIBC_2.2.5`.
* `†` means that no installed header declares the name. It is internal, compat-only, or reached
  only through macros or the compiler.

Public `libm` names are folded into families: the base name plus its type-suffixed variants
(`f`, `l`, `f32`, `f64`, `f128`, `f32x`, `f64x`), with the count of exported names in the family
(§10.0). The folding is by suffix only, so a few non-variant names that happen to end in `f` or `l`
are folded too (for example `fdim` is counted under `fdim`).

Total distinct exported names: **3943**.

### B.1 §1. Program startup, termination, environment (48 names)

`abort`, `argp_err_exit_status`, `argp_error`, `argp_failure`, `argp_help`, `argp_parse`, `argp_program_bug_address`, `argp_program_version`, `argp_program_version_hook`†, `argp_state_help`, `argp_usage`, `clearenv`, `__cxa_at_quick_exit`†@2.10, `__cxa_atexit`†, `__cxa_finalize`†, `__cxa_thread_atexit_impl`†@2.18, `__environ`, `_environ`, `environ`, `_Exit`, `_exit`, `exit`, `__getauxval`@2.16, `getauxval`@2.16, `getenv`, `getopt`, `getopt_long`, `getopt_long_only`, `getsubopt`, `__libc_init_first`†, `__libc_stack_end`†, `__libc_start_main`†, `on_exit`, `optarg`, `opterr`, `optind`, `optopt`, `__posix_getopt`@2.10, `__progname_full`†, `__progname`†, `program_invocation_name`, `program_invocation_short_name`, `putenv`, `quick_exit`@2.10, `__secure_getenv`, `secure_getenv`@2.17, `setenv`, `unsetenv`

### B.2 §2. Error reporting and diagnostics (38 names)

`addseverity`, `__assert`, `__assert_fail`, `__assert_perror_fail`, `__backtrace`, `backtrace`, `__backtrace_symbols`, `backtrace_symbols`, `__backtrace_symbols_fd`, `backtrace_symbols_fd`, `__chk_fail`†@2.3.4, `err`, `__errno_location`, `error`, `error_at_line`, `error_message_count`, `error_one_per_line`, `error_print_progname`†, `errx`, `fmtmsg`, `__stack_chk_fail`†@2.4, `strerror`, `strerror_l`@2.6, `__strerror_r`, `strerror_r`, `strerrordesc_np`@2.32, `strerrorname_np`@2.32, `_sys_errlist`†, `sys_errlist`†, `_sys_nerr`†, `sys_nerr`†, `verr`, `verrx`, `vwarn`, `vwarnx`, `warn`, `warnx`, `__xpg_strerror_r`@2.3.4

### B.3 §3. Memory allocation (60 names)

`__after_morecore_hook`†, `aligned_alloc`@2.16, `brk`, `calloc`, `cfree`†, `__curbrk`†, `__default_morecore`†, `free`, `__free_hook`†, `__libc_calloc`†, `__libc_free`†, `__libc_freeres`†, `__libc_mallinfo`†, `__libc_malloc`†, `__libc_mallopt`†, `__libc_memalign`†, `__libc_pvalloc`†, `__libc_realloc`†, `__libc_valloc`†, `mallinfo`, `mallinfo2`@2.33, `malloc`, `malloc_get_state`†, `__malloc_hook`†, `malloc_info`@2.10, `__malloc_initialize_hook`†, `malloc_set_state`†, `malloc_stats`, `malloc_trim`, `malloc_usable_size`, `mallopt`, `mallwatch`†, `mcheck`, `mcheck_check_all`, `mcheck_pedantic`, `memalign`, `__memalign_hook`†, `__morecore`†, `mprobe`, `mtrace`, `muntrace`, `_obstack`, `obstack_alloc_failed_handler`†, `_obstack_allocated_p`†, `_obstack_begin`, `_obstack_begin_1`, `obstack_exit_failure`, `_obstack_free`, `obstack_free`, `_obstack_memory_used`, `_obstack_newchunk`, `posix_memalign`, `pvalloc`, `realloc`, `__realloc_hook`†, `reallocarray`@2.26, `__sbrk`, `sbrk`, `tr_break`†, `valloc`

### B.4 §4. Byte arrays and strings (169 names)

`argz_add`, `argz_add_sep`, `argz_append`, `__argz_count`, `argz_count`, `argz_create`, `argz_create_sep`, `argz_delete`, `argz_extract`, `argz_insert`, `__argz_next`, `argz_next`, `argz_replace`, `__argz_stringify`, `argz_stringify`, `basename`, `bcmp`, `bcopy`, `__bzero`, `bzero`, `dirname`, `envz_add`, `envz_entry`, `envz_get`, `envz_merge`, `envz_remove`, `envz_strip`, `__explicit_bzero_chk`@2.25, `explicit_bzero`@2.25, `index`, `memccpy`, `memchr`, `memcmp`, `__memcmpeq`@2.35, `memcpy`, `__memcpy_chk`@2.3.4, `memfrob`, `memmem`, `memmove`, `__memmove_chk`@2.3.4, `__mempcpy`, `mempcpy`, `__mempcpy_chk`@2.3.4, `__mempcpy_small`†, `memrchr`, `memset`, `__memset_chk`@2.3.4, `__rawmemchr`, `rawmemchr`, `rindex`, `__stpcpy`, `stpcpy`, `__stpcpy_chk`@2.3.4, `__stpcpy_small`†, `__stpncpy`, `stpncpy`, `__stpncpy_chk`@2.4, `__strcasecmp`, `strcasecmp`, `__strcasecmp_l`, `strcasecmp_l`@2.3, `__strcasestr`, `strcasestr`, `strcat`, `__strcat_chk`@2.3.4, `strchr`, `strchrnul`, `strcmp`, `strcoll`, `__strcoll_l`, `strcoll_l`@2.3, `strcpy`, `__strcpy_chk`@2.3.4, `__strcpy_small`†, `strcspn`, `__strcspn_c1`†, `__strcspn_c2`†, `__strcspn_c3`†, `__strdup`, `strdup`, `strfry`, `__strlcat_chk`@2.38, `strlcat`@2.38, `__strlcpy_chk`@2.38, `strlcpy`@2.38, `strlen`, `strncasecmp`, `__strncasecmp_l`, `strncasecmp_l`@2.3, `strncat`, `__strncat_chk`@2.3.4, `strncmp`, `strncpy`, `__strncpy_chk`@2.3.4, `__strndup`, `strndup`, `strnlen`, `strpbrk`, `__strpbrk_c2`†, `__strpbrk_c3`†, `strrchr`, `strsep`, `__strsep_1c`†, `__strsep_2c`†, `__strsep_3c`†, `__strsep_g`†, `strspn`, `__strspn_c1`†, `__strspn_c2`†, `__strspn_c3`†, `strstr`, `__strverscmp`, `strverscmp`, `strxfrm`, `__strxfrm_l`, `strxfrm_l`@2.3, `swab`, `wcpcpy`, `__wcpcpy_chk`@2.4, `wcpncpy`, `__wcpncpy_chk`@2.4, `wcscasecmp`, `__wcscasecmp_l`, `wcscasecmp_l`@2.3, `wcscat`, `__wcscat_chk`@2.4, `wcschr`, `wcschrnul`, `wcscmp`, `wcscoll`, `__wcscoll_l`, `wcscoll_l`@2.3, `wcscpy`, `__wcscpy_chk`@2.4, `wcscspn`, `wcsdup`, `__wcslcat_chk`@2.38, `wcslcat`@2.38, `__wcslcpy_chk`@2.38, `wcslcpy`@2.38, `wcslen`, `wcsncasecmp`, `__wcsncasecmp_l`, `wcsncasecmp_l`@2.3, `wcsncat`, `__wcsncat_chk`@2.4, `wcsncmp`, `wcsncpy`, `__wcsncpy_chk`@2.4, `wcsnlen`, `wcspbrk`, `wcsrchr`, `wcsspn`, `wcsstr`, `wcswcs`, `wcsxfrm`, `__wcsxfrm_l`, `wcsxfrm_l`@2.3, `wmemchr`, `wmemcmp`, `wmemcpy`, `__wmemcpy_chk`@2.4, `wmemmove`, `__wmemmove_chk`@2.4, `wmempcpy`, `__wmempcpy_chk`@2.4, `wmemset`, `__wmemset_chk`@2.4, `__xpg_basename`

### B.5 §5. Character classification (115 names)

`__ctype32_b`†, `__ctype32_tolower`†, `__ctype32_toupper`†, `__ctype_b`, `__ctype_b_loc`@2.3, `__ctype_tolower`, `__ctype_tolower_loc`@2.3, `__ctype_toupper`, `__ctype_toupper_loc`@2.3, `isalnum`, `__isalnum_l`, `isalnum_l`@2.3, `isalpha`, `__isalpha_l`, `isalpha_l`@2.3, `isascii`, `__isascii_l`, `isblank`, `__isblank_l`, `isblank_l`@2.3, `iscntrl`, `__iscntrl_l`, `iscntrl_l`@2.3, `__isctype`@2.3, `isctype`@2.3, `isdigit`, `__isdigit_l`, `isdigit_l`@2.3, `isgraph`, `__isgraph_l`, `isgraph_l`@2.3, `islower`, `__islower_l`, `islower_l`@2.3, `isprint`, `__isprint_l`, `isprint_l`@2.3, `ispunct`, `__ispunct_l`, `ispunct_l`@2.3, `isspace`, `__isspace_l`, `isspace_l`@2.3, `isupper`, `__isupper_l`, `isupper_l`@2.3, `iswalnum`, `__iswalnum_l`, `iswalnum_l`@2.3, `iswalpha`, `__iswalpha_l`, `iswalpha_l`@2.3, `iswblank`, `__iswblank_l`, `iswblank_l`@2.3, `iswcntrl`, `__iswcntrl_l`, `iswcntrl_l`@2.3, `__iswctype`, `iswctype`, `__iswctype_l`, `iswctype_l`@2.3, `iswdigit`, `__iswdigit_l`, `iswdigit_l`@2.3, `iswgraph`, `__iswgraph_l`, `iswgraph_l`@2.3, `iswlower`, `__iswlower_l`, `iswlower_l`@2.3, `iswprint`, `__iswprint_l`, `iswprint_l`@2.3, `iswpunct`, `__iswpunct_l`, `iswpunct_l`@2.3, `iswspace`, `__iswspace_l`, `iswspace_l`@2.3, `iswupper`, `__iswupper_l`, `iswupper_l`@2.3, `iswxdigit`, `__iswxdigit_l`, `iswxdigit_l`@2.3, `isxdigit`, `__isxdigit_l`, `isxdigit_l`@2.3, `toascii`, `__toascii_l`, `_tolower`, `tolower`, `__tolower_l`, `tolower_l`@2.3, `_toupper`, `toupper`, `__toupper_l`, `toupper_l`@2.3, `__towctrans`, `towctrans`, `__towctrans_l`, `towctrans_l`@2.3, `towlower`, `__towlower_l`, `towlower_l`@2.3, `towupper`, `__towupper_l`, `towupper_l`@2.3, `wctrans`, `__wctrans_l`, `wctrans_l`@2.3, `wctype`, `__wctype_l`, `wctype_l`@2.3

### B.6 §6. Multibyte, wide characters, conversion (35 names)

`btowc`, `c16rtomb`@2.16, `c32rtomb`@2.16, `c8rtomb`@2.36, `__ctype_get_mb_cur_max`, `iconv`, `iconv_close`, `iconv_open`, `mblen`, `__mbrlen`, `mbrlen`, `mbrtoc16`@2.16, `mbrtoc32`@2.16, `mbrtoc8`@2.36, `__mbrtowc`, `mbrtowc`, `mbsinit`, `mbsnrtowcs`, `__mbsnrtowcs_chk`@2.4, `mbsrtowcs`, `__mbsrtowcs_chk`@2.4, `mbstowcs`, `__mbstowcs_chk`@2.4, `mbtowc`, `wcrtomb`, `__wcrtomb_chk`@2.4, `wcsnrtombs`, `__wcsnrtombs_chk`@2.4, `wcsrtombs`, `__wcsrtombs_chk`@2.4, `wcswidth`, `wctob`, `wctomb`, `__wctomb_chk`@2.4, `wcwidth`

### B.7 §7. Locales and message translation (35 names)

`bind_textdomain_codeset`, `bindtextdomain`, `catclose`, `catgets`, `catopen`, `__dcgettext`, `dcgettext`, `dcngettext`, `__dgettext`, `dgettext`, `dngettext`, `__duplocale`, `duplocale`@2.3, `__freelocale`, `freelocale`@2.3, `gettext`, `_libc_intl_domainname`†, `localeconv`, `__newlocale`, `newlocale`@2.3, `ngettext`, `_nl_default_dirname`†, `_nl_domain_bindings`†, `nl_langinfo`, `__nl_langinfo_l`, `nl_langinfo_l`@2.3, `_nl_msg_cat_cntr`†, `rpmatch`, `setlocale`, `strfmon`, `__strfmon_l`, `strfmon_l`@2.3, `textdomain`, `__uselocale`@2.3, `uselocale`@2.3

### B.8 §8. Numeric conversion and integer arithmetic (219 names)

`a64l`, `abs`, `atof`, `atoi`, `atol`, `atoll`, `div`, `ecvt`, `ecvt_r`, `fcvt`, `fcvt_r`, `__ffs`, `ffs`, `ffsl`, `ffsll`, `gcvt`, `imaxabs`, `imaxdiv`, `__isoc23_strtoimax`@2.38, `__isoc23_strtol_l`@2.38, `__isoc23_strtol`@2.38, `__isoc23_strtoll_l`@2.38, `__isoc23_strtoll`@2.38, `__isoc23_strtoul_l`@2.38, `__isoc23_strtoul`@2.38, `__isoc23_strtoull_l`@2.38, `__isoc23_strtoull`@2.38, `__isoc23_strtoumax`@2.38, `__isoc23_wcstoimax`@2.38, `__isoc23_wcstol_l`@2.38, `__isoc23_wcstol`@2.38, `__isoc23_wcstoll_l`@2.38, `__isoc23_wcstoll`@2.38, `__isoc23_wcstoul_l`@2.38, `__isoc23_wcstoul`@2.38, `__isoc23_wcstoull_l`@2.38, `__isoc23_wcstoull`@2.38, `__isoc23_wcstoumax`@2.38, `l64a`, `labs`, `ldiv`, `llabs`, `lldiv`, `qecvt`, `qecvt_r`, `qfcvt`, `qfcvt_r`, `qgcvt`, `stdc_bit_ceil_uc`@2.39, `stdc_bit_ceil_ui`@2.39, `stdc_bit_ceil_ul`@2.39, `stdc_bit_ceil_ull`@2.39, `stdc_bit_ceil_us`@2.39, `stdc_bit_floor_uc`@2.39, `stdc_bit_floor_ui`@2.39, `stdc_bit_floor_ul`@2.39, `stdc_bit_floor_ull`@2.39, `stdc_bit_floor_us`@2.39, `stdc_bit_width_uc`@2.39, `stdc_bit_width_ui`@2.39, `stdc_bit_width_ul`@2.39, `stdc_bit_width_ull`@2.39, `stdc_bit_width_us`@2.39, `stdc_count_ones_uc`@2.39, `stdc_count_ones_ui`@2.39, `stdc_count_ones_ul`@2.39, `stdc_count_ones_ull`@2.39, `stdc_count_ones_us`@2.39, `stdc_count_zeros_uc`@2.39, `stdc_count_zeros_ui`@2.39, `stdc_count_zeros_ul`@2.39, `stdc_count_zeros_ull`@2.39, `stdc_count_zeros_us`@2.39, `stdc_first_leading_one_uc`@2.39, `stdc_first_leading_one_ui`@2.39, `stdc_first_leading_one_ul`@2.39, `stdc_first_leading_one_ull`@2.39, `stdc_first_leading_one_us`@2.39, `stdc_first_leading_zero_uc`@2.39, `stdc_first_leading_zero_ui`@2.39, `stdc_first_leading_zero_ul`@2.39, `stdc_first_leading_zero_ull`@2.39, `stdc_first_leading_zero_us`@2.39, `stdc_first_trailing_one_uc`@2.39, `stdc_first_trailing_one_ui`@2.39, `stdc_first_trailing_one_ul`@2.39, `stdc_first_trailing_one_ull`@2.39, `stdc_first_trailing_one_us`@2.39, `stdc_first_trailing_zero_uc`@2.39, `stdc_first_trailing_zero_ui`@2.39, `stdc_first_trailing_zero_ul`@2.39, `stdc_first_trailing_zero_ull`@2.39, `stdc_first_trailing_zero_us`@2.39, `stdc_has_single_bit_uc`@2.39, `stdc_has_single_bit_ui`@2.39, `stdc_has_single_bit_ul`@2.39, `stdc_has_single_bit_ull`@2.39, `stdc_has_single_bit_us`@2.39, `stdc_leading_ones_uc`@2.39, `stdc_leading_ones_ui`@2.39, `stdc_leading_ones_ul`@2.39, `stdc_leading_ones_ull`@2.39, `stdc_leading_ones_us`@2.39, `stdc_leading_zeros_uc`@2.39, `stdc_leading_zeros_ui`@2.39, `stdc_leading_zeros_ul`@2.39, `stdc_leading_zeros_ull`@2.39, `stdc_leading_zeros_us`@2.39, `stdc_trailing_ones_uc`@2.39, `stdc_trailing_ones_ui`@2.39, `stdc_trailing_ones_ul`@2.39, `stdc_trailing_ones_ull`@2.39, `stdc_trailing_ones_us`@2.39, `stdc_trailing_zeros_uc`@2.39, `stdc_trailing_zeros_ui`@2.39, `stdc_trailing_zeros_ul`@2.39, `stdc_trailing_zeros_ull`@2.39, `stdc_trailing_zeros_us`@2.39, `strfromd`@2.25, `strfromf128`@2.26, `strfromf32`@2.27, `strfromf32x`@2.27, `strfromf64`@2.27, `strfromf64x`@2.27, `strfromf`@2.25, `strfroml`@2.25, `strtod`, `__strtod_internal`†, `__strtod_l`, `strtod_l`@2.3, `strtof`, `__strtof128_internal`†@2.26, `strtof128_l`@2.26, `strtof128`@2.26, `strtof32_l`@2.27, `strtof32`@2.27, `strtof32x_l`@2.27, `strtof32x`@2.27, `strtof64_l`@2.27, `strtof64`@2.27, `strtof64x_l`@2.27, `strtof64x`@2.27, `__strtof_internal`†, `__strtof_l`, `strtof_l`@2.3, `strtoimax`, `strtok`, `__strtok_r`, `strtok_r`, `__strtok_r_1c`†, `strtol`, `__strtol_internal`†, `__strtol_l`, `strtol_l`@2.3, `strtold`, `__strtold_internal`†, `__strtold_l`, `strtold_l`@2.3, `strtoll`, `__strtoll_internal`†, `__strtoll_l`, `strtoll_l`@2.3.3, `strtoq`, `strtoul`, `__strtoul_internal`†, `__strtoul_l`, `strtoul_l`@2.3, `strtoull`, `__strtoull_internal`†, `__strtoull_l`, `strtoull_l`@2.3.3, `strtoumax`, `strtouq`, `wcstod`, `__wcstod_internal`†, `__wcstod_l`, `wcstod_l`@2.3, `wcstof`, `__wcstof128_internal`†@2.26, `wcstof128_l`@2.26, `wcstof128`@2.26, `wcstof32_l`@2.27, `wcstof32`@2.27, `wcstof32x_l`@2.27, `wcstof32x`@2.27, `wcstof64_l`@2.27, `wcstof64`@2.27, `wcstof64x_l`@2.27, `wcstof64x`@2.27, `__wcstof_internal`†, `__wcstof_l`, `wcstof_l`@2.3, `wcstoimax`, `wcstok`, `wcstol`, `__wcstol_internal`†, `__wcstol_l`, `wcstol_l`@2.3, `wcstold`, `__wcstold_internal`†, `__wcstold_l`, `wcstold_l`@2.3, `wcstoll`, `__wcstoll_internal`†, `__wcstoll_l`, `wcstoll_l`@2.3, `wcstombs`, `__wcstombs_chk`@2.4, `wcstoq`, `wcstoul`, `__wcstoul_internal`†, `__wcstoul_l`, `wcstoul_l`@2.3, `wcstoull`, `__wcstoull_internal`†, `__wcstoull_l`, `wcstoull_l`@2.3, `wcstoumax`, `wcstouq`

### B.9 §9. Random numbers (34 names)

`arc4random_buf`@2.36, `arc4random_uniform`@2.36, `arc4random`@2.36, `drand48`, `drand48_r`, `erand48`, `erand48_r`, `getentropy`@2.25, `getrandom`@2.25, `initstate`, `initstate_r`, `jrand48`, `jrand48_r`, `lcong48`, `lcong48_r`, `lrand48`, `lrand48_r`, `mrand48`, `mrand48_r`, `nrand48`, `nrand48_r`, `rand`, `rand_r`, `random`, `random_r`, `seed48`, `seed48_r`, `setstate`, `setstate_r`, `srand`, `srand48`, `srand48_r`, `srandom`, `srandom_r`

### B.10 §10. Mathematics (libm) (1163 names)

**Families (base name × exported names):** `acos`×8, `acosh`×8, `asin`×8, `asinh`×8, `atan`×8, `atan2`×8, `atanh`×8, `cabs`×8, `cacos`×8, `cacosh`×8, `canonicalize`×8, `carg`×8, `casin`×8, `casinh`×8, `catan`×8, `catanh`×8, `cbrt`×8, `ccos`×8, `ccosh`×8, `ceil`×8, `cexp`×8, `cimag`×8, `clog`×8, `clog10`×8, `conj`×8, `copysign`×8, `cos`×8, `cosh`×8, `cpow`×8, `cproj`×8, `creal`×8, `csin`×8, `csinh`×8, `csqrt`×8, `ctan`×8, `ctanh`×8, `dadd`×1, `ddiv`×1, `dfma`×1, `dmul`×1, `drem`×3, `dsqrt`×1, `dsub`×1, `erf`×8, `erfc`×8, `exp`×8, `exp10`×8, `exp2`×8, `expm1`×8, `f32add`×4, `f32div`×4, `f32fma`×4, `f32mul`×4, `f32sqrt`×4, `f32sub`×4, `f32xadd`×3, `f32xdiv`×3, `f32xfma`×3, `f32xmul`×3, `f32xsqrt`×3, `f32xsub`×3, `f64add`×2, `f64div`×2, `f64fma`×2, `f64mul`×2, `f64sqrt`×2, `f64sub`×2, `f64xadd`×1, `f64xdiv`×1, `f64xfma`×1, `f64xmul`×1, `f64xsqrt`×1, `f64xsub`×1, `fabs`×8, `fadd`×2, `fdim`×8, `fdiv`×2, `feclearexcept`×1, `fedisableexcept`×1, `feenableexcept`×1, `fegetenv`×1, `fegetexcept`×1, `fegetexceptflag`×1, `fegetmode`×1, `fegetround`×1, `feholdexcept`×1, `feraiseexcept`×1, `fesetenv`×1, `fesetexcept`×1, `fesetexceptflag`×1, `fesetmode`×1, `fesetround`×1, `fetestexcept`×1, `fetestexceptflag`×1, `feupdateenv`×1, `ffma`×2, `finite`×3, `floor`×8, `fma`×8, `fmax`×8, `fmaximum`×8, `fmaximum_mag`×8, `fmaximum_mag_num`×8, `fmaximum_num`×8, `fmaxmag`×8, `fmin`×8, `fminimum`×8, `fminimum_mag`×8, `fminimum_mag_num`×8, `fminimum_num`×8, `fminmag`×8, `fmod`×8, `fmul`×2, `frexp`×8, `fromfp`×8, `fromfpx`×8, `fsqrt`×2, `fsub`×2, `gamma`×3, `getpayload`×8, `hypot`×8, `ilogb`×8, `isinf`×3, `isnan`×3, `j0`×8, `j1`×8, `jn`×8, `ldexp`×8, `lgamma`×8, `lgamma_r`×1, `lgammaf128_r`×1, `lgammaf32_r`×1, `lgammaf32x_r`×1, `lgammaf64_r`×1, `lgammaf64x_r`×1, `lgammaf_r`×1, `lgammal_r`×1, `llogb`×8, `llrint`×8, `llround`×8, `log`×8, `log10`×8, `log1p`×8, `log2`×8, `logb`×8, `lrint`×8, `lround`×8, `matherr`×1, `modf`×8, `nan`×8, `nearbyint`×8, `nextafter`×8, `nextdown`×8, `nexttoward`×3, `nextup`×8, `pow`×8, `pow10`×3, `remainder`×8, `remquo`×8, `rint`×8, `round`×8, `roundeven`×8, `scalb`×3, `scalbln`×8, `scalbn`×8, `setpayload`×8, `setpayloadsig`×8, `signgam`×1, `significand`×3, `sin`×8, `sincos`×8, `sinh`×8, `sqrt`×8, `tan`×8, `tanh`×8, `tgamma`×8, `totalorder`×8, `totalordermag`×8, `trunc`×8, `ufromfp`×8, `ufromfpx`×8, `y0`×8, `y1`×8, `yn`×8

**Reserved-name entry points:** `__acos_finite`@2.15, `__acosf128_finite`@2.26, `__acosf_finite`@2.15, `__acosh_finite`@2.15, `__acoshf128_finite`@2.26, `__acoshf_finite`@2.15, `__acoshl_finite`@2.15, `__acosl_finite`@2.15, `__asin_finite`@2.15, `__asinf128_finite`@2.26, `__asinf_finite`@2.15, `__asinl_finite`@2.15, `__atan2_finite`@2.15, `__atan2f128_finite`@2.26, `__atan2f_finite`@2.15, `__atan2l_finite`@2.15, `__atanh_finite`@2.15, `__atanhf128_finite`@2.26, `__atanhf_finite`@2.15, `__atanhl_finite`@2.15, `__clog10`, `__clog10f`, `__clog10l`, `__cosh_finite`@2.15, `__coshf128_finite`@2.26, `__coshf_finite`@2.15, `__coshl_finite`@2.15, `__exp10_finite`@2.15, `__exp10f128_finite`@2.26, `__exp10f_finite`@2.15, `__exp10l_finite`@2.15, `__exp2_finite`@2.15, `__exp2f128_finite`@2.26, `__exp2f_finite`@2.15, `__exp2l_finite`@2.15, `__exp_finite`@2.15, `__expf128_finite`@2.26, `__expf_finite`@2.15, `__expl_finite`@2.15, `__finite`, `__finitef`, `__finitef128`@2.26, `__finitel`, `__fmod_finite`@2.15, `__fmodf128_finite`@2.26, `__fmodf_finite`@2.15, `__fmodl_finite`@2.15, `__fpclassify`, `__fpclassifyf`, `__fpclassifyf128`@2.26, `__fpclassifyl`, `__fpu_control`, `__gamma_r_finite`@2.15, `__gammaf128_r_finite`@2.26, `__gammaf_r_finite`@2.15, `__gammal_r_finite`@2.15, `__hypot_finite`@2.15, `__hypotf128_finite`@2.26, `__hypotf_finite`@2.15, `__hypotl_finite`@2.15, `__iscanonicall`@2.25, `__iseqsig`@2.25, `__iseqsigf128`@2.26, `__iseqsigf`@2.25, `__iseqsigl`@2.25, `__isinf`, `__isinff128`@2.26, `__isinff`†, `__isinfl`†, `__isnan`, `__isnanf128`†@2.26, `__isnanf`†, `__isnanl`†, `__issignaling`@2.18, `__issignalingf128`@2.26, `__issignalingf`@2.18, `__issignalingl`@2.18, `__j0_finite`@2.15, `__j0f128_finite`@2.26, `__j0f_finite`@2.15, `__j0l_finite`@2.15, `__j1_finite`@2.15, `__j1f128_finite`@2.26, `__j1f_finite`@2.15, `__j1l_finite`@2.15, `__jn_finite`@2.15, `__jnf128_finite`@2.26, `__jnf_finite`@2.15, `__jnl_finite`@2.15, `__lgamma_r_finite`@2.15, `__lgammaf128_r_finite`@2.26, `__lgammaf_r_finite`@2.15, `__lgammal_r_finite`@2.15, `_LIB_VERSION`, `__log10_finite`@2.15, `__log10f128_finite`@2.26, `__log10f_finite`@2.15, `__log10l_finite`@2.15, `__log2_finite`@2.15, `__log2f128_finite`@2.26, `__log2f_finite`@2.15, `__log2l_finite`@2.15, `__log_finite`@2.15, `__logf128_finite`@2.26, `__logf_finite`@2.15, `__logl_finite`@2.15, `__pow_finite`@2.15, `__powf128_finite`@2.26, `__powf_finite`@2.15, `__powl_finite`@2.15, `__remainder_finite`@2.15, `__remainderf128_finite`@2.26, `__remainderf_finite`@2.15, `__remainderl_finite`@2.15, `__scalb_finite`@2.15, `__scalbf_finite`@2.15, `__scalbl_finite`@2.15, `__signbit`, `__signbitf`, `__signbitf128`@2.26, `__signbitl`, `__signgam`@2.23, `__sinh_finite`@2.15, `__sinhf128_finite`@2.26, `__sinhf_finite`@2.15, `__sinhl_finite`@2.15, `__sqrt_finite`@2.15, `__sqrtf128_finite`@2.26, `__sqrtf_finite`@2.15, `__sqrtl_finite`@2.15, `__y0_finite`@2.15, `__y0f128_finite`@2.26, `__y0f_finite`@2.15, `__y0l_finite`@2.15, `__y1_finite`@2.15, `__y1f128_finite`@2.26, `__y1f_finite`@2.15, `__y1l_finite`@2.15, `__yn_finite`@2.15, `__ynf128_finite`@2.26, `__ynf_finite`@2.15, `__ynl_finite`@2.15

### B.11 §11. Low-level I/O on descriptors (141 names)

`aio_cancel`, `aio_cancel64`, `aio_error`, `aio_error64`, `aio_fsync`, `aio_fsync64`, `aio_init`, `aio_read`, `aio_read64`, `aio_return`, `aio_return64`, `aio_suspend`, `aio_suspend64`, `aio_write`, `aio_write64`, `__close`, `close`, `close_range`@2.34, `closefrom`@2.34, `copy_file_range`@2.27, `creat`, `creat64`, `dup`, `__dup2`, `dup2`, `dup3`@2.9, `epoll_create1`@2.9, `epoll_create`@2.3.2, `epoll_ctl`@2.3.2, `epoll_pwait2`@2.35, `epoll_pwait`@2.6, `epoll_wait`@2.3.2, `eventfd_read`@2.7, `eventfd_write`@2.7, `eventfd`@2.7, `fallocate64`@2.10, `fallocate`@2.10, `fanotify_init`@2.13, `fanotify_mark`@2.13, `__fcntl`, `fcntl`, `fcntl64`@2.28, `fdatasync`, `__fdelt_chk`@2.15, `__fdelt_warn`@2.15, `flock`, `fsync`, `inotify_add_watch`@2.4, `inotify_init1`@2.9, `inotify_init`@2.4, `inotify_rm_watch`@2.4, `ioctl`, `lio_listio`, `lio_listio64`, `llseek`†, `lockf`, `lockf64`, `__lseek`, `lseek`, `lseek64`, `madvise`, `memfd_create`@2.27, `mincore`, `mlock`, `mlock2`@2.27, `mlockall`, `mmap`, `mmap64`, `mprotect`, `mremap`, `msync`, `munlock`, `munlockall`, `munmap`, `name_to_handle_at`@2.14, `__open`, `open`, `__open64`, `open64`, `__open64_2`@2.7, `__open_2`@2.7, `open_by_handle_at`@2.14, `__openat64_2`@2.7, `openat64`@2.4, `__openat_2`@2.7, `openat`@2.4, `pkey_alloc`@2.27, `pkey_free`@2.27, `pkey_get`@2.27, `pkey_mprotect`@2.27, `pkey_set`@2.27, `__poll`, `poll`, `__poll_chk`@2.16, `posix_fadvise`, `posix_fadvise64`, `posix_fallocate`, `posix_fallocate64`, `posix_madvise`, `__ppoll_chk`@2.16, `ppoll`@2.4, `pread`, `__pread64`, `pread64`, `__pread64_chk`@2.4, `__pread_chk`@2.4, `preadv2`@2.26, `preadv64`@2.10, `preadv64v2`@2.26, `preadv`@2.10, `process_madvise`@2.36, `process_mrelease`@2.36, `process_vm_readv`@2.15, `process_vm_writev`@2.15, `pselect`, `pwrite`, `__pwrite64`, `pwrite64`, `pwritev2`@2.26, `pwritev64`@2.10, `pwritev64v2`@2.26, `pwritev`@2.10, `__read`, `read`, `__read_chk`@2.4, `readahead`@2.3, `readv`, `remap_file_pages`@2.3.3, `__select`, `select`, `sendfile`, `sendfile64`@2.3, `splice`@2.5, `sync`, `sync_file_range`@2.6, `syncfs`@2.14, `tee`@2.5, `vmsplice`@2.5, `__write`, `write`, `writev`

### B.12 §12. Buffered streams (340 names)

`__asprintf`, `asprintf`, `__asprintf_chk`@2.8, `clearerr`, `clearerr_unlocked`, `ctermid`, `cuserid`, `dprintf`, `__dprintf_chk`@2.8, `__fbufsize`, `fclose`, `fcloseall`, `fdopen`, `feof`, `feof_unlocked`, `ferror`, `ferror_unlocked`, `fflush`, `fflush_unlocked`, `fgetc`, `fgetc_unlocked`, `fgetpos`, `fgetpos64`, `fgets`, `__fgets_chk`@2.4, `fgets_unlocked`, `__fgets_unlocked_chk`@2.4, `fgetwc`, `fgetwc_unlocked`, `fgetws`, `__fgetws_chk`@2.4, `fgetws_unlocked`, `__fgetws_unlocked_chk`@2.4, `fileno`, `fileno_unlocked`, `__flbf`, `flockfile`, `_flushlbf`, `fmemopen`, `fopen`, `fopen64`, `fopencookie`, `__fpending`, `fprintf`, `__fprintf_chk`@2.3.4, `__fpurge`, `fputc`, `fputc_unlocked`, `fputs`, `fputs_unlocked`, `fputwc`, `fputwc_unlocked`, `fputws`, `fputws_unlocked`, `fread`, `__fread_chk`@2.7, `fread_unlocked`, `__fread_unlocked_chk`@2.7, `__freadable`, `__freading`, `freopen`, `freopen64`, `fscanf`, `fseek`, `fseeko`, `fseeko64`, `__fsetlocking`, `fsetpos`, `fsetpos64`, `ftell`, `ftello`, `ftello64`, `ftrylockfile`, `funlockfile`, `fwide`, `fwprintf`, `__fwprintf_chk`@2.4, `__fwritable`, `fwrite`, `fwrite_unlocked`, `__fwriting`, `fwscanf`, `getc`, `getc_unlocked`, `getchar`, `getchar_unlocked`, `__getdelim`, `getdelim`, `getline`, `gets`, `__gets_chk`@2.3.4, `getw`, `getwc`, `getwc_unlocked`, `getwchar`, `getwchar_unlocked`, `_IO_2_1_stderr_`, `_IO_2_1_stdin_`, `_IO_2_1_stdout_`, `_IO_adjust_column`, `_IO_adjust_wcolumn`, `_IO_default_doallocate`, `_IO_default_finish`, `_IO_default_pbackfail`, `_IO_default_uflow`, `_IO_default_xsgetn`, `_IO_default_xsputn`, `_IO_do_write`, `_IO_doallocbuf`, `_IO_fclose`, `_IO_fdopen`, `_IO_feof`, `_IO_ferror`, `_IO_fflush`, `_IO_fgetpos`, `_IO_fgetpos64`, `_IO_fgets`, `_IO_file_attach`, `_IO_file_close`, `_IO_file_close_it`, `_IO_file_doallocate`, `_IO_file_finish`, `_IO_file_fopen`, `_IO_file_init`, `_IO_file_jumps`, `_IO_file_open`, `_IO_file_overflow`, `_IO_file_read`, `_IO_file_seek`, `_IO_file_seekoff`, `_IO_file_setbuf`, `_IO_file_stat`, `_IO_file_sync`, `_IO_file_underflow`, `_IO_file_write`, `_IO_file_xsputn`, `_IO_flockfile`, `_IO_flush_all`, `_IO_flush_all_linebuffered`, `_IO_fopen`, `_IO_fprintf`, `_IO_fputs`, `_IO_fread`, `_IO_free_backup_area`, `_IO_free_wbackup_area`, `_IO_fsetpos`, `_IO_fsetpos64`, `_IO_ftell`, `_IO_ftrylockfile`, `_IO_funlockfile`, `_IO_fwrite`, `_IO_getc`, `_IO_getline`, `_IO_getline_info`, `_IO_gets`, `_IO_init`, `_IO_init_marker`, `_IO_init_wmarker`, `_IO_iter_begin`, `_IO_iter_end`, `_IO_iter_file`, `_IO_iter_next`, `_IO_least_wmarker`, `_IO_link_in`, `_IO_list_all`, `_IO_list_lock`, `_IO_list_resetlock`, `_IO_list_unlock`, `_IO_marker_delta`, `_IO_marker_difference`, `_IO_padn`, `_IO_peekc_locked`, `_IO_popen`, `_IO_printf`, `_IO_proc_close`, `_IO_proc_open`, `_IO_putc`, `_IO_puts`, `_IO_remove_marker`, `_IO_seekmark`, `_IO_seekoff`, `_IO_seekpos`, `_IO_seekwmark`, `_IO_setb`, `_IO_setbuffer`, `_IO_setvbuf`, `_IO_sgetn`, `_IO_sprintf`, `_IO_sputbackc`, `_IO_sputbackwc`, `_IO_sscanf`, `_IO_str_init_readonly`, `_IO_str_init_static`, `_IO_str_overflow`, `_IO_str_pbackfail`, `_IO_str_seekoff`, `_IO_str_underflow`, `_IO_sungetc`, `_IO_sungetwc`, `_IO_switch_to_get_mode`, `_IO_switch_to_main_wget_area`, `_IO_switch_to_wbackup_area`, `_IO_switch_to_wget_mode`, `_IO_un_link`, `_IO_ungetc`, `_IO_unsave_markers`, `_IO_unsave_wmarkers`, `_IO_vfprintf`, `_IO_vfscanf`, `_IO_vsprintf`, `_IO_wdefault_doallocate`, `_IO_wdefault_finish`, `_IO_wdefault_pbackfail`, `_IO_wdefault_uflow`, `_IO_wdefault_xsgetn`, `_IO_wdefault_xsputn`, `_IO_wdo_write`, `_IO_wdoallocbuf`, `_IO_wfile_jumps`, `_IO_wfile_overflow`, `_IO_wfile_seekoff`, `_IO_wfile_sync`, `_IO_wfile_underflow`, `_IO_wfile_xsputn`, `_IO_wmarker_delta`, `_IO_wsetb`, `__isoc23_fscanf`@2.38, `__isoc23_fwscanf`@2.38, `__isoc23_scanf`@2.38, `__isoc23_sscanf`@2.38, `__isoc23_swscanf`@2.38, `__isoc23_vfscanf`@2.38, `__isoc23_vfwscanf`@2.38, `__isoc23_vscanf`@2.38, `__isoc23_vsscanf`@2.38, `__isoc23_vswscanf`@2.38, `__isoc23_vwscanf`@2.38, `__isoc23_wscanf`@2.38, `__isoc99_fscanf`@2.7, `__isoc99_fwscanf`@2.7, `__isoc99_scanf`@2.7, `__isoc99_sscanf`@2.7, `__isoc99_swscanf`@2.7, `__isoc99_vfscanf`@2.7, `__isoc99_vfwscanf`@2.7, `__isoc99_vscanf`@2.7, `__isoc99_vsscanf`@2.7, `__isoc99_vswscanf`@2.7, `__isoc99_vwscanf`@2.7, `__isoc99_wscanf`@2.7, `obstack_printf`, `__obstack_printf_chk`@2.8, `obstack_vprintf`, `__obstack_vprintf_chk`@2.8, `open_memstream`, `open_wmemstream`@2.4, `__overflow`, `parse_printf_format`, `pclose`, `perror`, `popen`, `printf`, `__printf_chk`@2.3.4, `__printf_fp`†, `printf_size`, `printf_size_info`, `putc`, `putc_unlocked`, `putchar`, `putchar_unlocked`, `puts`, `putw`, `putwc`, `putwc_unlocked`, `putwchar`, `putwchar_unlocked`, `register_printf_function`, `register_printf_modifier`@2.10, `register_printf_specifier`@2.10, `register_printf_type`@2.10, `remove`, `rewind`, `scanf`, `setbuf`, `setbuffer`, `setlinebuf`, `setvbuf`, `snprintf`, `__snprintf_chk`@2.3.4, `sprintf`, `__sprintf_chk`@2.3.4, `sscanf`, `stderr`, `stdin`, `stdout`, `swprintf`, `__swprintf_chk`@2.4, `swscanf`, `tempnam`, `tmpfile`, `tmpfile64`, `tmpnam`, `tmpnam_r`, `__uflow`, `__underflow`†, `ungetc`, `ungetwc`, `vasprintf`, `__vasprintf_chk`@2.8, `vdprintf`, `__vdprintf_chk`@2.8, `vfprintf`, `__vfprintf_chk`@2.3.4, `__vfscanf`, `vfscanf`, `vfwprintf`, `__vfwprintf_chk`@2.4, `vfwscanf`, `vprintf`, `__vprintf_chk`@2.3.4, `vscanf`, `__vsnprintf`, `vsnprintf`, `__vsnprintf_chk`@2.3.4, `vsprintf`, `__vsprintf_chk`@2.3.4, `__vsscanf`, `vsscanf`, `vswprintf`, `__vswprintf_chk`@2.4, `vswscanf`, `vwprintf`, `__vwprintf_chk`@2.4, `vwscanf`, `__woverflow`†, `wprintf`, `__wprintf_chk`@2.4, `wscanf`, `__wuflow`†, `__wunderflow`†

### B.13 §13. File-system interface (167 names)

`access`, `addmntent`, `alphasort`, `alphasort64`, `canonicalize_file_name`, `chdir`, `chmod`, `chown`, `chroot`, `closedir`, `dirfd`, `eaccess`@2.4, `endfsent`, `__endmntent`, `endmntent`, `euidaccess`, `faccessat`@2.4, `fchdir`, `fchmod`, `fchmodat`@2.4, `fchown`, `fchownat`@2.4, `fdopendir`@2.4, `fgetxattr`@2.3, `flistxattr`@2.3, `fremovexattr`@2.3, `fsconfig`@2.36, `fsetxattr`@2.3, `fsmount`@2.36, `fsopen`@2.36, `fspick`@2.36, `fstat64`@2.33, `fstat`@2.33, `fstatat64`@2.33, `fstatat`@2.33, `fstatfs`, `fstatfs64`, `fstatvfs`, `fstatvfs64`, `ftruncate`, `ftruncate64`, `fts64_children`@2.23, `fts64_close`@2.23, `fts64_open`@2.23, `fts64_read`@2.23, `fts64_set`@2.23, `fts_children`, `fts_close`, `fts_open`, `fts_read`, `fts_set`, `ftw`, `ftw64`, `futimens`@2.6, `__fxstat64`†, `__fxstat`†, `__fxstatat64`†@2.4, `__fxstatat`†@2.4, `get_current_dir_name`, `getcwd`, `__getcwd_chk`@2.4, `getdents64`@2.30, `getdirentries`, `getdirentries64`, `getfsent`, `getfsfile`, `getfsspec`, `getmntent`, `__getmntent_r`, `getmntent_r`, `getwd`, `__getwd_chk`@2.4, `getxattr`@2.3, `gnu_dev_major`@2.3.3, `gnu_dev_makedev`@2.3.3, `gnu_dev_minor`@2.3.3, `hasmntopt`, `lchmod`@2.3.2, `lchown`, `lgetxattr`@2.3, `link`, `linkat`@2.4, `listxattr`@2.3, `llistxattr`@2.3, `lremovexattr`@2.3, `lsetxattr`@2.3, `lstat64`@2.33, `lstat`@2.33, `__lxstat64`†, `__lxstat`†, `mkdir`, `mkdirat`@2.4, `mkdtemp`, `mkfifo`, `mkfifoat`@2.4, `mknod`@2.33, `mknodat`@2.33, `mkostemp64`@2.7, `mkostemp`@2.7, `mkostemps64`@2.11, `mkostemps`@2.11, `mkstemp`, `mkstemp64`, `mkstemps64`@2.11, `mkstemps`@2.11, `mktemp`, `mount`, `mount_setattr`@2.36, `move_mount`@2.36, `nftw`, `nftw64`, `open_tree`@2.36, `opendir`, `pivot_root`†, `readdir`, `readdir64`, `readdir64_r`, `readdir_r`, `readlink`, `__readlink_chk`@2.4, `__readlinkat_chk`@2.5, `readlinkat`@2.4, `realpath`, `__realpath_chk`@2.4, `removexattr`@2.3, `rename`, `renameat2`@2.28, `renameat`@2.4, `revoke`, `rewinddir`, `rmdir`, `scandir`, `scandir64`, `scandirat64`@2.15, `scandirat`@2.15, `seekdir`, `setfsent`, `__setmntent`, `setmntent`, `setxattr`@2.3, `__statfs`, `statfs`, `statfs64`, `statvfs`, `statvfs64`, `statx`@2.28, `swapoff`, `swapon`, `symlink`, `symlinkat`@2.4, `telldir`, `truncate`, `truncate64`, `umask`, `umount`, `umount2`, `unlink`, `unlinkat`@2.4, `ustat`†, `utime`, `utimensat`@2.6, `versionsort`, `versionsort64`, `__xmknod`†, `__xmknodat`†@2.4, `__xstat64`†, `__xstat`†

### B.14 §14. Searching, sorting, pattern matching (45 names)

`bsearch`, `fnmatch`, `glob`, `glob64`, `glob_pattern_p`, `globfree`, `globfree64`, `hcreate`, `hcreate_r`, `hdestroy`, `hdestroy_r`, `hsearch`, `hsearch_r`, `insque`, `lfind`, `lsearch`, `qsort`, `qsort_r`@2.8, `re_comp`, `re_compile_fastmap`, `re_compile_pattern`, `re_exec`, `re_match`, `re_match_2`, `re_max_failures`†, `re_search`, `re_search_2`, `re_set_registers`, `re_set_syntax`, `re_syntax_options`, `regcomp`, `regerror`, `regexec`, `regfree`, `remque`, `stat64`@2.33, `stat`@2.33, `tdelete`, `tdestroy`, `tfind`, `tsearch`, `twalk`, `twalk_r`@2.30, `wordexp`, `wordfree`

### B.15 §15. Sockets and network interfaces (98 names)

`accept`, `accept4`@2.10, `bind`, `__check_rhosts_file`†, `__cmsg_nxthdr`, `__connect`, `connect`, `ether_aton`, `ether_aton_r`, `ether_hostton`, `ether_line`, `ether_ntoa`, `ether_ntoa_r`, `ether_ntohost`, `freeifaddrs`@2.3, `getifaddrs`@2.3, `getipv4sourcefilter`@2.3.4, `getpeername`, `getsockname`, `getsockopt`, `getsourcefilter`@2.3.4, `htonl`, `htons`, `if_freenameindex`, `if_indextoname`, `if_nameindex`, `if_nametoindex`, `in6addr_any`, `in6addr_loopback`, `inet6_opt_append`@2.5, `inet6_opt_find`@2.5, `inet6_opt_finish`@2.5, `inet6_opt_get_val`@2.5, `inet6_opt_init`@2.5, `inet6_opt_next`@2.5, `inet6_opt_set_val`@2.5, `inet6_option_alloc`@2.3.3, `inet6_option_append`@2.3.3, `inet6_option_find`@2.3.3, `inet6_option_init`@2.3.3, `inet6_option_next`@2.3.3, `inet6_option_space`@2.3.3, `inet6_rth_add`@2.5, `inet6_rth_getaddr`@2.5, `inet6_rth_init`@2.5, `inet6_rth_reverse`@2.5, `inet6_rth_segments`@2.5, `inet6_rth_space`@2.5, `inet_addr`, `inet_aton`, `inet_lnaof`, `inet_makeaddr`, `inet_net_ntop`, `inet_net_pton`, `inet_neta`, `inet_netof`, `inet_network`, `inet_nsap_addr`, `inet_nsap_ntoa`, `inet_ntoa`, `inet_ntop`, `inet_pton`, `iruserok`, `iruserok_af`, `isfdtype`, `__ivaliduser`†, `listen`, `ntohl`, `ntohs`, `rcmd`, `rcmd_af`, `__rcmd_errstr`†, `recv`, `__recv_chk`@2.4, `recvfrom`, `__recvfrom_chk`@2.4, `recvmmsg`@2.12, `recvmsg`, `rexec`, `rexec_af`, `rexecoptions`†, `rresvport`, `rresvport_af`, `ruserok`, `ruserok_af`, `ruserpass`†, `__send`, `send`, `sendmmsg`@2.14, `sendmsg`, `sendto`, `setipv4sourcefilter`@2.3.4, `setsockopt`, `setsourcefilter`@2.3.4, `shutdown`, `sockatmark`, `socket`, `socketpair`

### B.16 §16. Name services and databases (185 names)

`__b64_ntop`, `__b64_pton`, `__dn_comp`, `dn_comp`@2.34, `__dn_count_labels`, `__dn_expand`, `dn_expand`@2.34, `__dn_skipname`, `dn_skipname`@2.34, `endaliasent`, `endhostent`, `endnetent`, `endnetgrent`, `endprotoent`, `endrpcent`, `endservent`, `__fp_nquery`, `__fp_query`, `__fp_resstat`, `freeaddrinfo`, `gai_cancel`, `gai_error`, `gai_strerror`, `gai_suspend`, `getaddrinfo`, `getaddrinfo_a`, `getaliasbyname`, `getaliasbyname_r`, `getaliasent`, `getaliasent_r`, `gethostbyaddr`, `gethostbyaddr_r`, `gethostbyname`, `gethostbyname2`, `gethostbyname2_r`, `gethostbyname_r`, `gethostent`, `gethostent_r`, `_gethtbyaddr`†, `_gethtbyname2`†, `_gethtbyname`†, `_gethtent`†, `_getlong`, `getnameinfo`, `getnetbyaddr`, `getnetbyaddr_r`, `getnetbyname`, `getnetbyname_r`, `getnetent`, `getnetent_r`, `getnetgrent`, `getnetgrent_r`, `getprotobyname`, `getprotobyname_r`, `getprotobynumber`, `getprotobynumber_r`, `getprotoent`, `getprotoent_r`, `getrpcbyname`, `getrpcbyname_r`, `getrpcbynumber`, `getrpcbynumber_r`, `getrpcent`, `getrpcent_r`, `getservbyname`, `getservbyname_r`, `getservbyport`, `getservbyport_r`, `getservent`, `getservent_r`, `_getshort`, `h_errlist`†, `__h_errno_location`, `h_nerr`†, `herror`, `__hostalias`, `hstrerror`, `innetgr`, `__loc_aton`, `__loc_ntoa`, `ns_datetosecs`@2.9, `ns_format_ttl`@2.9, `ns_get16`@2.9, `ns_get32`@2.9, `ns_initparse`@2.9, `ns_makecanon`@2.9, `ns_msg_getflag`@2.9, `ns_name_compress`@2.9, `ns_name_ntol`@2.9, `ns_name_ntop`@2.9, `ns_name_pack`@2.9, `ns_name_pton`@2.9, `ns_name_rollback`@2.9, `ns_name_skip`@2.9, `ns_name_uncompress`@2.9, `ns_name_unpack`@2.9, `ns_parse_ttl`@2.9, `ns_parserr`@2.9, `ns_put16`@2.9, `ns_put32`@2.9, `ns_samedomain`@2.9, `ns_samename`@2.9, `ns_skiprr`@2.9, `ns_sprintrr`@2.9, `ns_sprintrrf`@2.9, `ns_subdomain`@2.9, `__nss_configure_lookup`, `__nss_database_lookup`†, `__nss_group_lookup`†, `__nss_hostname_digits_dots`†, `__nss_hosts_lookup`†, `__nss_next`†, `__nss_passwd_lookup`†, `__p_cdname`, `__p_cdnname`, `__p_class`, `__p_class_syms`, `__p_fqname`, `__p_fqnname`, `__p_option`, `__p_query`, `__p_rcode`@2.3.2, `__p_secstodate`, `__p_time`, `__p_type`, `__p_type_syms`, `__putlong`, `__putshort`, `_res`, `__res_close`, `__res_dnok`, `res_dnok`@2.34, `res_gethostbyaddr`, `res_gethostbyname`, `res_gethostbyname2`, `_res_hconf`, `__res_hnok`, `res_hnok`@2.34, `__res_hostalias`, `__res_init`, `__res_isourserver`, `__res_mailok`, `res_mailok`@2.34, `__res_mkquery`, `res_mkquery`@2.34, `__res_nameinquery`, `__res_nclose`, `__res_ninit`, `__res_nmkquery`, `res_nmkquery`@2.34, `__res_nquery`, `res_nquery`@2.34, `__res_nquerydomain`, `res_nquerydomain`@2.34, `__res_nsearch`, `res_nsearch`@2.34, `__res_nsend`, `res_nsend`@2.34, `_res_opcodes`, `__res_ownok`, `res_ownok`@2.34, `__res_queriesmatch`, `__res_query`, `res_query`@2.34, `__res_querydomain`, `res_querydomain`@2.34, `__res_randomid`, `__res_search`, `res_search`@2.34, `__res_send`, `res_send_setqhook`, `res_send_setrhook`, `res_send`@2.34, `__res_state`, `setaliasent`, `sethostent`, `_sethtent`†, `setnetent`, `setnetgrent`, `setprotoent`, `setrpcent`, `setservent`, `__sym_ntop`, `__sym_ntos`, `__sym_ston`

### B.17 §17. Users, groups, credentials, accounting (103 names)

`endgrent`, `endpwent`, `endsgent`@2.10, `endspent`, `endusershell`, `endutent`, `endutxent`, `fgetgrent`, `fgetgrent_r`, `fgetpwent`, `fgetpwent_r`, `fgetsgent_r`@2.10, `fgetsgent`@2.10, `fgetspent`, `fgetspent_r`, `getegid`, `geteuid`, `getgid`, `getgrent`, `getgrent_r`, `getgrgid`, `getgrgid_r`, `getgrnam`, `getgrnam_r`, `getgrouplist`, `getgroups`, `__getgroups_chk`@2.4, `getlogin`, `getlogin_r`, `__getlogin_r_chk`@2.4, `getpass`, `getpw`, `getpwent`, `getpwent_r`, `getpwnam`, `getpwnam_r`, `getpwuid`, `getpwuid_r`, `getresgid`, `getresuid`, `getsgent_r`@2.10, `getsgent`@2.10, `getsgnam_r`@2.10, `getsgnam`@2.10, `getspent`, `getspent_r`, `getspnam`, `getspnam_r`, `getuid`, `getusershell`, `getutent`, `getutent_r`, `getutid`, `getutid_r`, `getutline`, `getutline_r`, `getutmp`, `getutmpx`, `getutxent`, `getutxid`, `getutxline`, `group_member`, `initgroups`, `lckpwdf`, `login`, `login_tty`, `logout`, `logwtmp`, `putgrent`, `putpwent`, `putsgent`@2.10, `putspent`, `pututline`, `pututxline`, `setegid`, `seteuid`, `setfsgid`, `setfsuid`, `setgid`, `setgrent`, `setgroups`, `setlogin`, `setpwent`, `setregid`, `setresgid`, `setresuid`, `setreuid`, `setsgent`@2.10, `setspent`, `setuid`, `setusershell`, `setutent`, `setutxent`, `sgetsgent_r`@2.10, `sgetsgent`@2.10, `sgetspent`, `sgetspent_r`, `ttyslot`, `ulckpwdf`, `updwtmp`, `updwtmpx`, `utmpname`, `utmpxname`

### B.18 §18. Processes (77 names)

`__arch_prctl`†, `arch_prctl`†, `__bsd_getpgrp`†, `__clone`, `clone`, `daemon`, `execl`, `execle`, `execlp`, `execv`, `execve`, `execveat`@2.34, `execvp`, `execvpe`@2.11, `fexecve`, `__fork`, `fork`, `_Fork`@2.34, `__getpgid`, `getpgid`, `getpgrp`, `__getpid`, `getpid`, `getppid`, `getsid`, `gettid`@2.30, `personality`, `pidfd_getfd`@2.36, `pidfd_getpid`@2.39, `pidfd_open`@2.36, `pidfd_send_signal`@2.36, `pidfd_spawn`@2.39, `pidfd_spawnp`@2.39, `posix_spawn`, `posix_spawn_file_actions_addchdir_np`@2.29, `posix_spawn_file_actions_addclose`, `posix_spawn_file_actions_addclosefrom_np`@2.34, `posix_spawn_file_actions_adddup2`, `posix_spawn_file_actions_addfchdir_np`@2.29, `posix_spawn_file_actions_addopen`, `posix_spawn_file_actions_addtcsetpgrp_np`@2.35, `posix_spawn_file_actions_destroy`, `posix_spawn_file_actions_init`, `posix_spawnattr_destroy`, `posix_spawnattr_getcgroup_np`@2.39, `posix_spawnattr_getflags`, `posix_spawnattr_getpgroup`, `posix_spawnattr_getschedparam`, `posix_spawnattr_getschedpolicy`, `posix_spawnattr_getsigdefault`, `posix_spawnattr_getsigmask`, `posix_spawnattr_init`, `posix_spawnattr_setcgroup_np`@2.39, `posix_spawnattr_setflags`, `posix_spawnattr_setpgroup`, `posix_spawnattr_setschedparam`, `posix_spawnattr_setschedpolicy`, `posix_spawnattr_setsigdefault`, `posix_spawnattr_setsigmask`, `posix_spawnp`, `prctl`, `ptrace`, `__register_atfork`†@2.3.2, `__setpgid`, `setpgid`, `setpgrp`, `setsid`, `system`, `__vfork`, `vfork`, `__wait`, `wait`, `wait3`, `wait4`, `waitid`, `__waitpid`, `waitpid`

### B.19 §19. Terminals and pseudo-terminals (34 names)

`cfgetispeed`, `cfgetospeed`, `cfmakeraw`, `cfsetispeed`, `cfsetospeed`, `cfsetspeed`, `endttyent`, `forkpty`, `getpt`, `getttyent`, `getttynam`, `grantpt`, `gtty`, `isatty`, `openpty`, `posix_openpt`, `ptsname`, `ptsname_r`, `__ptsname_r_chk`@2.4, `setttyent`, `stty`, `tcdrain`, `tcflow`, `tcflush`, `tcgetattr`, `tcgetpgrp`, `tcgetsid`, `tcsendbreak`, `tcsetattr`, `tcsetpgrp`, `ttyname`, `ttyname_r`, `__ttyname_r_chk`@2.4, `unlockpt`

### B.20 §20. Signals (61 names)

`bsd_signal`, `gsignal`, `kill`, `killpg`, `__libc_allocate_rtsig`†, `__libc_current_sigrtmax`, `__libc_current_sigrtmin`, `pause`, `psiginfo`@2.10, `psignal`, `raise`, `sigabbrev_np`@2.32, `__sigaction`, `sigaction`, `__sigaddset`, `sigaddset`, `sigaltstack`, `sigandset`, `sigblock`, `__sigdelset`, `sigdelset`, `sigdescr_np`@2.32, `sigemptyset`, `sigfillset`, `siggetmask`, `sighold`, `sigignore`, `siginterrupt`, `sigisemptyset`, `__sigismember`, `sigismember`, `siglongjmp`, `signal`, `signalfd`@2.7, `sigorset`, `__sigpause`, `sigpause`, `sigpending`, `sigprocmask`, `sigqueue`, `sigrelse`, `sigreturn`, `sigset`, `__sigsetjmp`, `sigsetmask`, `sigstack`, `__sigsuspend`, `sigsuspend`, `sigtimedwait`, `sigvec`†, `sigwait`, `sigwaitinfo`, `ssignal`, `strsignal`, `sys_sigabbrev`†, `_sys_siglist`†, `sys_siglist`†, `__sysv_signal`, `sysv_signal`, `tgkill`@2.30, `__xpg_sigpause`†

### B.21 §21. Non-local control transfer and contexts (9 names)

`getcontext`, `_longjmp`, `longjmp`, `__longjmp_chk`@2.11, `makecontext`, `setcontext`, `_setjmp`, `setjmp`, `swapcontext`

### B.22 §22. Threads and synchronization (242 names)

`call_once`@2.28, `cnd_broadcast`@2.28, `cnd_destroy`@2.28, `cnd_init`@2.28, `cnd_signal`@2.28, `cnd_timedwait`@2.28, `cnd_wait`@2.28, `__libc_single_threaded`@2.32, `mtx_destroy`@2.28, `mtx_init`@2.28, `mtx_lock`@2.28, `mtx_timedlock`@2.28, `mtx_trylock`@2.28, `mtx_unlock`@2.28, `pthread_atfork`, `pthread_attr_destroy`, `pthread_attr_getaffinity_np`@2.3.3, `pthread_attr_getdetachstate`, `pthread_attr_getguardsize`, `pthread_attr_getinheritsched`, `pthread_attr_getschedparam`, `pthread_attr_getschedpolicy`, `pthread_attr_getscope`, `pthread_attr_getsigmask_np`@2.32, `pthread_attr_getstack`, `pthread_attr_getstackaddr`, `pthread_attr_getstacksize`, `pthread_attr_init`, `pthread_attr_setaffinity_np`@2.3.3, `pthread_attr_setdetachstate`, `pthread_attr_setguardsize`, `pthread_attr_setinheritsched`, `pthread_attr_setschedparam`, `pthread_attr_setschedpolicy`, `pthread_attr_setscope`, `pthread_attr_setsigmask_np`@2.32, `pthread_attr_setstack`, `pthread_attr_setstackaddr`, `pthread_attr_setstacksize`, `pthread_barrier_destroy`, `pthread_barrier_init`, `pthread_barrier_wait`, `pthread_barrierattr_destroy`, `pthread_barrierattr_getpshared`@2.3.3, `pthread_barrierattr_init`, `pthread_barrierattr_setpshared`, `pthread_cancel`, `_pthread_cleanup_pop`, `_pthread_cleanup_pop_restore`†, `_pthread_cleanup_push`, `_pthread_cleanup_push_defer`†, `__pthread_cleanup_routine`@2.3.3, `pthread_clockjoin_np`@2.31, `pthread_cond_broadcast`, `pthread_cond_clockwait`@2.30, `pthread_cond_destroy`, `pthread_cond_init`, `pthread_cond_signal`, `pthread_cond_timedwait`, `pthread_cond_wait`, `pthread_condattr_destroy`, `pthread_condattr_getclock`@2.3.3, `pthread_condattr_getpshared`, `pthread_condattr_init`, `pthread_condattr_setclock`@2.3.3, `pthread_condattr_setpshared`, `pthread_create`, `pthread_detach`, `pthread_equal`, `pthread_exit`, `pthread_getaffinity_np`@2.3.3, `pthread_getattr_default_np`@2.18, `pthread_getattr_np`, `pthread_getconcurrency`, `pthread_getcpuclockid`, `pthread_getname_np`@2.12, `pthread_getschedparam`, `__pthread_getspecific`, `pthread_getspecific`, `pthread_join`, `__pthread_key_create`, `pthread_key_create`, `pthread_key_delete`, `pthread_kill`, `pthread_kill_other_threads_np`†, `pthread_mutex_clocklock`@2.30, `pthread_mutex_consistent_np`†@2.4, `pthread_mutex_consistent`@2.12, `__pthread_mutex_destroy`, `pthread_mutex_destroy`, `pthread_mutex_getprioceiling`@2.4, `__pthread_mutex_init`, `pthread_mutex_init`, `__pthread_mutex_lock`, `pthread_mutex_lock`, `pthread_mutex_setprioceiling`@2.4, `pthread_mutex_timedlock`, `__pthread_mutex_trylock`, `pthread_mutex_trylock`, `__pthread_mutex_unlock`, `pthread_mutex_unlock`, `__pthread_mutexattr_destroy`, `pthread_mutexattr_destroy`, `pthread_mutexattr_getkind_np`†, `pthread_mutexattr_getprioceiling`@2.4, `pthread_mutexattr_getprotocol`@2.4, `pthread_mutexattr_getpshared`, `pthread_mutexattr_getrobust_np`†@2.4, `pthread_mutexattr_getrobust`@2.12, `pthread_mutexattr_gettype`, `__pthread_mutexattr_init`, `pthread_mutexattr_init`, `pthread_mutexattr_setkind_np`†, `pthread_mutexattr_setprioceiling`@2.4, `pthread_mutexattr_setprotocol`@2.4, `pthread_mutexattr_setpshared`, `pthread_mutexattr_setrobust_np`†@2.4, `pthread_mutexattr_setrobust`@2.12, `__pthread_mutexattr_settype`, `pthread_mutexattr_settype`, `__pthread_once`, `pthread_once`, `__pthread_register_cancel_defer`@2.3.3, `__pthread_register_cancel`@2.3.3, `pthread_rwlock_clockrdlock`@2.30, `pthread_rwlock_clockwrlock`@2.30, `__pthread_rwlock_destroy`, `pthread_rwlock_destroy`, `__pthread_rwlock_init`, `pthread_rwlock_init`, `__pthread_rwlock_rdlock`, `pthread_rwlock_rdlock`, `pthread_rwlock_timedrdlock`, `pthread_rwlock_timedwrlock`, `__pthread_rwlock_tryrdlock`, `pthread_rwlock_tryrdlock`, `__pthread_rwlock_trywrlock`, `pthread_rwlock_trywrlock`, `__pthread_rwlock_unlock`, `pthread_rwlock_unlock`, `__pthread_rwlock_wrlock`, `pthread_rwlock_wrlock`, `pthread_rwlockattr_destroy`, `pthread_rwlockattr_getkind_np`, `pthread_rwlockattr_getpshared`, `pthread_rwlockattr_init`, `pthread_rwlockattr_setkind_np`, `pthread_rwlockattr_setpshared`, `pthread_self`, `pthread_setaffinity_np`@2.3.3, `pthread_setattr_default_np`@2.18, `pthread_setcancelstate`, `pthread_setcanceltype`, `pthread_setconcurrency`, `pthread_setname_np`@2.12, `pthread_setschedparam`, `pthread_setschedprio`@2.3.4, `__pthread_setspecific`, `pthread_setspecific`, `pthread_sigmask`, `pthread_sigqueue`@2.11, `pthread_spin_destroy`, `pthread_spin_init`, `pthread_spin_lock`, `pthread_spin_trylock`, `pthread_spin_unlock`, `pthread_testcancel`, `pthread_timedjoin_np`@2.3.3, `pthread_tryjoin_np`@2.3.3, `__pthread_unregister_cancel_restore`@2.3.3, `__pthread_unregister_cancel`@2.3.3, `__pthread_unwind_next`@2.3.3, `pthread_yield`, `__rseq_flags`@2.35, `__rseq_offset`@2.35, `__rseq_size`@2.35, `__sched_yield`, `sched_yield`, `sem_clockwait`@2.30, `sem_close`, `sem_destroy`, `sem_getvalue`, `sem_init`, `sem_open`, `sem_post`, `sem_timedwait`, `sem_trywait`, `sem_unlink`, `sem_wait`, `__sysconf`, `td_init`, `td_log`, `td_symbol_list`, `td_ta_clear_event`, `td_ta_delete`, `td_ta_enable_stats`, `td_ta_event_addr`, `td_ta_event_getmsg`, `td_ta_get_nthreads`, `td_ta_get_ph`, `td_ta_get_stats`, `td_ta_map_id2thr`, `td_ta_map_lwp2thr`, `td_ta_new`, `td_ta_reset_stats`, `td_ta_set_event`, `td_ta_setconcurrency`, `td_ta_thr_iter`, `td_ta_tsd_iter`, `td_thr_clear_event`, `td_thr_dbresume`, `td_thr_dbsuspend`, `td_thr_event_enable`, `td_thr_event_getmsg`, `td_thr_get_info`, `td_thr_getfpregs`, `td_thr_getgregs`, `td_thr_getxregs`, `td_thr_getxregsize`, `td_thr_set_event`, `td_thr_setfpregs`, `td_thr_setgregs`, `td_thr_setprio`, `td_thr_setsigpending`, `td_thr_setxregs`, `td_thr_sigsetmask`, `td_thr_tls_get_addr`@2.3, `td_thr_tlsbase`@2.3.3, `td_thr_tsd`, `td_thr_validate`, `thrd_create`@2.28, `thrd_current`@2.28, `thrd_detach`@2.28, `thrd_equal`@2.28, `thrd_exit`@2.28, `thrd_join`@2.28, `thrd_sleep`@2.28, `thrd_yield`@2.28, `tss_create`@2.28, `tss_delete`@2.28, `tss_get`@2.28, `tss_set`@2.28

### B.23 §23. Time, clocks, timers (74 names)

`adjtime`, `__adjtimex`, `adjtimex`, `alarm`, `asctime`, `asctime_r`, `clock`, `clock_adjtime`@2.14, `clock_getcpuclockid`, `clock_getres`, `clock_gettime`, `clock_nanosleep`, `clock_settime`, `ctime`, `ctime_r`, `__daylight`, `daylight`, `difftime`, `dysize`, `ftime`, `futimes`@2.3, `futimesat`@2.4, `getdate`, `getdate_err`, `getdate_r`, `getitimer`, `__gettimeofday`, `gettimeofday`, `gmtime`, `__gmtime_r`, `gmtime_r`, `localtime`, `localtime_r`, `lutimes`@2.3, `mktime`, `nanosleep`, `__nanosleep`@2.2.6, `ntp_adjtime`, `ntp_gettime`†, `ntp_gettimex`@2.12, `setitimer`, `settimeofday`, `sleep`, `stime`†, `strftime`, `__strftime_l`@2.3, `strftime_l`@2.3, `strptime`, `strptime_l`@2.3.2, `time`, `timegm`, `timelocal`, `timer_create`, `timer_delete`, `timer_getoverrun`, `timer_gettime`, `timer_settime`, `timerfd_create`@2.8, `timerfd_gettime`@2.8, `timerfd_settime`@2.8, `times`, `timespec_get`@2.16, `timespec_getres`@2.34, `__timezone`, `timezone`, `__tzname`, `tzname`, `tzset`, `ualarm`, `usleep`, `utimes`, `wcsftime`, `__wcsftime_l`@2.3, `wcsftime_l`@2.3

### B.24 §24. Resources, scheduling, system information (80 names)

`acct`, `bdflush`†, `capget`†, `capset`†, `confstr`, `__confstr_chk`@2.4, `create_module`†, `delete_module`†, `fpathconf`, `get_avphys_pages`, `get_kernel_syms`†, `get_nprocs`, `get_nprocs_conf`, `get_phys_pages`, `getcpu`@2.29, `getdomainname`, `__getdomainname_chk`@2.4, `getdtablesize`, `gethostid`, `gethostname`, `__gethostname_chk`@2.4, `getloadavg`, `getpagesize`, `getpriority`, `getrlimit`, `getrlimit64`, `getrusage`, `gnu_get_libc_release`, `gnu_get_libc_version`, `init_module`†, `ioperm`, `iopl`, `klogctl`, `modify_ldt`†, `nfsservctl`†, `nice`, `pathconf`, `prlimit64`@2.13, `prlimit`@2.13, `query_module`†, `quotactl`, `reboot`, `__sched_cpualloc`@2.7, `__sched_cpucount`@2.6, `__sched_cpufree`@2.7, `__sched_get_priority_max`, `sched_get_priority_max`, `__sched_get_priority_min`, `sched_get_priority_min`, `sched_getaffinity`@2.3.3, `sched_getcpu`@2.6, `__sched_getparam`, `sched_getparam`, `__sched_getscheduler`, `sched_getscheduler`, `sched_rr_get_interval`, `sched_setaffinity`@2.3.3, `sched_setparam`, `__sched_setscheduler`, `sched_setscheduler`, `setdomainname`, `sethostid`, `sethostname`, `setns`@2.14, `setpriority`, `setrlimit`, `setrlimit64`, `sstk`†, `syscall`, `sysconf`, `__sysctl`†, `sysctl`†, `sysinfo`, `ulimit`, `uname`, `unshare`@2.4, `uselib`†, `vhangup`, `vlimit`, `vtimes`†

### B.25 §25. Inter-process communication (30 names)

`ftok`, `__getpagesize`, `mq_close`@2.3.4, `mq_getattr`@2.3.4, `mq_notify`@2.3.4, `__mq_open_2`@2.7, `mq_open`@2.3.4, `mq_receive`@2.3.4, `mq_send`@2.3.4, `mq_setattr`@2.3.4, `mq_timedreceive`@2.3.4, `mq_timedsend`@2.3.4, `mq_unlink`@2.3.4, `msgctl`, `msgget`, `msgrcv`, `msgsnd`, `__pipe`, `pipe`, `pipe2`@2.9, `semctl`, `semget`, `semop`, `semtimedop`@2.3.3, `shm_open`, `shm_unlink`, `shmat`, `shmctl`, `shmdt`, `shmget`

### B.26 §26. System logging (7 names)

`closelog`, `openlog`, `setlogmask`, `syslog`, `__syslog_chk`@2.4, `vsyslog`, `__vsyslog_chk`@2.4

### B.27 §27. Dynamic linking and loading (17 names)

`_dl_find_object`@2.35, `dl_iterate_phdr`, `_dl_mcount_wrapper_check`, `_dl_mcount_wrapper`†, `_dl_mcount`†, `dladdr`, `dladdr1`@2.3.3, `dlclose`, `dlerror`, `dlinfo`@2.3.3, `dlmopen`@2.3.4, `dlopen`, `dlsym`, `dlvsym`, `_r_debug`, `__tls_get_addr`†@2.3, `__x86_get_cpuid_feature_leaf`@2.33

### B.28 §28. Profiling and instrumentation (12 names)

`__cyg_profile_func_enter`†, `__cyg_profile_func_exit`†, `__fentry__`†@2.13, `_mcleanup`, `_mcount`†, `mcount`†, `moncontrol`†, `__monstartup`, `monstartup`, `profil`, `__profile_frequency`†, `sprofil`

### B.29 §29. Legacy and compatibility subsystems (305 names)

`advance`†, `authdes_create`, `authdes_getucred`, `authdes_pk_create`, `_authenticate`, `authnone_create`, `authunix_create`, `authunix_create_default`, `bindresvport`, `callrpc`, `cbc_crypt`, `chflags`†, `clnt_broadcast`, `clnt_create`, `clnt_pcreateerror`, `clnt_perrno`, `clnt_perror`, `clnt_spcreateerror`, `clnt_sperrno`, `clnt_sperror`, `clntraw_create`, `clnttcp_create`, `clntudp_bufcreate`, `clntudp_create`, `clntunix_create`, `des_setparity`, `ecb_crypt`, `fattach`†, `fchflags`†, `fdetach`†, `__free_fdresult`, `get_myaddress`, `getmsg`†, `getnetname`, `getpmsg`†, `getpublickey`, `getrpcport`, `getsecretkey`, `host2netname`, `isastream`†, `key_decryptsession`, `key_decryptsession_pk`, `__key_decryptsession_pk_LOCAL`†, `key_encryptsession`, `key_encryptsession_pk`, `__key_encryptsession_pk_LOCAL`†, `key_gendes`, `__key_gendes_LOCAL`†, `key_get_conv`, `key_secretkey_is_set`, `key_setnet`, `key_setsecret`, `__libanl_version_placeholder`†, `__libc_sa_len`†, `__libdl_version_placeholder`†, `__libpthread_version_placeholder`†, `__librt_version_placeholder`†, `__libutil_version_placeholder`†, `loc1`†, `loc2`†, `locs`†, `netname2host`, `netname2user`, `nis_add`, `nis_add_entry`, `nis_addmember`, `nis_checkpoint`, `nis_clone_directory`, `nis_clone_object`, `nis_clone_result`, `nis_creategroup`, `__nis_default_access`, `__nis_default_group`, `__nis_default_owner`, `__nis_default_ttl`, `nis_destroy_object`, `nis_destroygroup`, `nis_dir_cmp`, `nis_domain_of`, `nis_domain_of_r`, `__nis_finddirectory`, `nis_first_entry`, `nis_free_directory`, `nis_free_object`, `nis_free_request`, `nis_freenames`, `nis_freeresult`, `nis_freeservlist`, `nis_freetags`, `nis_getnames`, `nis_getservlist`, `__nis_hash`, `nis_ismember`, `nis_leaf_of`, `nis_leaf_of_r`, `nis_lerror`, `nis_list`, `nis_local_directory`, `nis_local_group`, `nis_local_host`, `nis_local_principal`, `nis_lookup`, `nis_mkdir`, `nis_modify`, `nis_modify_entry`, `nis_name_of`, `nis_name_of_r`, `nis_next_entry`, `nis_perror`, `nis_ping`, `nis_print_directory`, `nis_print_entry`, `nis_print_group`, `nis_print_group_entry`, `nis_print_link`, `nis_print_object`, `nis_print_result`, `nis_print_rights`, `nis_print_table`, `nis_read_obj`, `nis_remove`, `nis_remove_entry`, `nis_removemember`, `nis_rmdir`, `nis_servstate`, `nis_sperrno`, `nis_sperror`, `nis_sperror_r`, `nis_stats`, `nis_verifygroup`, `nis_write_obj`, `__nisbind_connect`, `__nisbind_create`, `__nisbind_destroy`, `__nisbind_next`, `_null_auth`, `passwd2des`, `pmap_getmaps`, `pmap_getport`, `pmap_rmtcall`, `pmap_set`, `pmap_unset`, `putmsg`†, `putpmsg`†, `readColdStartFile`, `registerrpc`, `rpc_createerr`, `_rpc_dtablesize`, `__rpc_thread_createerr`†, `__rpc_thread_svc_fdset`†, `__rpc_thread_svc_max_pollfd`†, `__rpc_thread_svc_pollfd`†, `rtime`†, `__rtld_version_placeholder`†@2.34, `_seterr_reply`, `step`, `svc_exit`, `svc_fdset`, `svc_getreq`, `svc_getreq_common`, `svc_getreq_poll`, `svc_getreqset`, `svc_max_pollfd`, `svc_pollfd`, `svc_register`, `svc_run`, `svc_sendreply`, `svc_unregister`, `svcauthdes_stats`, `svcerr_auth`, `svcerr_decode`, `svcerr_noproc`, `svcerr_noprog`, `svcerr_progvers`, `svcerr_systemerr`, `svcerr_weakauth`, `svcfd_create`, `svcraw_create`, `svctcp_create`, `svcudp_bufcreate`, `svcudp_create`, `svcudp_enablecache`, `svcunix_create`, `svcunixfd_create`, `user2netname`, `writeColdStartFile`, `xdecrypt`, `xdr_accepted_reply`, `xdr_array`, `xdr_authdes_cred`, `xdr_authdes_verf`, `xdr_authunix_parms`, `xdr_bool`, `xdr_bytes`, `xdr_callhdr`, `xdr_callmsg`, `xdr_cback_data`, `xdr_char`, `xdr_cryptkeyarg`, `xdr_cryptkeyarg2`, `xdr_cryptkeyres`, `xdr_des_block`, `xdr_domainname`, `xdr_double`, `xdr_enum`, `xdr_float`, `xdr_free`, `xdr_getcredres`, `xdr_hyper`, `xdr_int`, `xdr_int16_t`, `xdr_int32_t`, `xdr_int64_t`, `xdr_int8_t`, `xdr_key_netstarg`, `xdr_key_netstres`, `xdr_keybuf`, `xdr_keydat`, `xdr_keystatus`, `xdr_long`, `xdr_longlong_t`, `xdr_mapname`, `xdr_netnamestr`, `xdr_netobj`, `xdr_obj_p`, `xdr_opaque`, `xdr_opaque_auth`, `xdr_peername`, `xdr_pmap`, `xdr_pmaplist`, `xdr_pointer`, `xdr_quad_t`@2.3.4, `xdr_reference`, `xdr_rejected_reply`, `xdr_replymsg`, `xdr_rmtcall_args`, `xdr_rmtcallres`, `xdr_short`, `xdr_sizeof`, `xdr_string`, `xdr_u_char`, `xdr_u_hyper`, `xdr_u_int`, `xdr_u_long`, `xdr_u_longlong_t`, `xdr_u_quad_t`@2.3.4, `xdr_u_short`, `xdr_uint16_t`, `xdr_uint32_t`, `xdr_uint64_t`, `xdr_uint8_t`, `xdr_union`, `xdr_unixcred`, `xdr_valdat`, `xdr_vector`, `xdr_void`, `xdr_wrapstring`, `xdr_yp_buf`, `xdr_ypall`, `xdr_ypbind_binding`, `xdr_ypbind_resp`, `xdr_ypbind_resptype`, `xdr_ypbind_setdom`, `xdr_ypdelete_args`, `xdr_ypmap_parms`, `xdr_ypmaplist`, `xdr_yppush_status`, `xdr_yppushresp_xfr`, `xdr_ypreq_key`, `xdr_ypreq_nokey`, `xdr_ypreq_xfr`, `xdr_ypresp_all`, `xdr_ypresp_key_val`, `xdr_ypresp_maplist`, `xdr_ypresp_master`, `xdr_ypresp_order`, `xdr_ypresp_val`, `xdr_ypresp_xfr`, `xdr_ypstat`, `xdr_ypupdate_args`, `xdr_ypxfrstat`, `xdrmem_create`, `xdrrec_create`, `xdrrec_endofrecord`, `xdrrec_eof`, `xdrrec_skiprecord`, `xdrstdio_create`, `xencrypt`, `xprt_register`, `xprt_unregister`, `yp_all`, `yp_bind`, `__yp_check`, `yp_first`, `yp_get_default_domain`, `yp_maplist`, `yp_master`, `yp_match`, `yp_next`, `yp_order`, `yp_unbind`, `yp_update`, `ypbinderr_string`, `yperr_string`, `ypprot_err`

---

<!-- appendix-c:begin -->
## Appendix C. ABI data

*Normative. Added under open decision OD-1 (owner-approved): the public ABI data that binary
compatibility needs and that the body of this spec does not record.*

### C.0 Provenance and scope

| | |
|---|---|
| Reference system | ldd (Ubuntu GLIBC 2.39-0ubuntu8.9) 2.39 (x86_64) |
| Sources | (1) the installed **public headers**, read only through the compiler: a probe declares one object of each type, and layouts come from its debug information; sizes and alignments from `sizeof`/`_Alignof`; initializer bytes from compiled initializers; prototypes from `gcc -aux-info`; numeric macros by evaluation. (2) The **dynamic symbol tables** of the installed libraries (`readelf --dyn-syms`): version nodes, default versus compatibility binding, and the sizes of exported data objects. |
| Not used | glibc source code. No function bodies, code sizes or dispatch mechanisms are recorded: a function's size and whether it is an IFUNC are implementation details and are excluded. |
| Excluded | `GLIBC_PRIVATE` symbols (311 names): private interfaces between glibc's own libraries, which a replacement that ships its own `ld.so` and `libc.so.6` does not need to reproduce. |
| Reproduce | `python3 spec/tools/abi_extract.py && python3 spec/tools/appendix_c.py` on the reference system. |

### C.1 Machine-readable tables (normative)

The TSV files are part of this appendix. The tables below reproduce the parts most often needed; the files are complete.

| File | Contents | Rows |
|---|---|---|
| `spec/abi/versions.tsv` | library, symbol, version node, binding (`default` = `@@`, `compat` = `@`), type, scope, size (data objects only) | 4600 |
| `spec/abi/layouts.tsv` | per type: size and alignment, then each member's offset, type and bit-field position | 1372 (161 types) |
| `spec/abi/initializers.tsv` | static initializer bit patterns | 10 |
| `spec/abi/prototypes.tsv` | C declaration of every function the public headers declare, with its header | 3778 |
| `spec/abi/constants.tsv` | value of every integer-constant macro the public headers define (names not starting with `__`) | 6687 |

### C.2 Libraries and version nodes

| Library (soname) | Public version nodes |
|---|---|
| `libc.so.6` | GLIBC_2.2.5, GLIBC_2.2.6, GLIBC_2.3, GLIBC_2.3.2, GLIBC_2.3.3, GLIBC_2.3.4, GLIBC_2.4, GLIBC_2.5, GLIBC_2.6, GLIBC_2.7, GLIBC_2.8, GLIBC_2.9, GLIBC_2.10, GLIBC_2.11, GLIBC_2.12, GLIBC_2.13, GLIBC_2.14, GLIBC_2.15, GLIBC_2.16, GLIBC_2.17, GLIBC_2.18, GLIBC_2.22, GLIBC_2.23, GLIBC_2.24, GLIBC_2.25, GLIBC_2.26, GLIBC_2.27, GLIBC_2.28, GLIBC_2.29, GLIBC_2.30, GLIBC_2.31, GLIBC_2.32, GLIBC_2.33, GLIBC_2.34, GLIBC_2.35, GLIBC_2.36, GLIBC_2.38, GLIBC_2.39 |
| `libm.so.6` | GLIBC_2.2.5, GLIBC_2.15, GLIBC_2.18, GLIBC_2.23, GLIBC_2.24, GLIBC_2.25, GLIBC_2.26, GLIBC_2.27, GLIBC_2.28, GLIBC_2.29, GLIBC_2.31, GLIBC_2.32, GLIBC_2.35, GLIBC_2.38, GLIBC_2.39 |
| `libpthread.so.0` | GLIBC_2.2.5, GLIBC_2.2.6, GLIBC_2.3.2, GLIBC_2.3.3, GLIBC_2.3.4, GLIBC_2.4, GLIBC_2.11, GLIBC_2.12, GLIBC_2.18, GLIBC_2.28, GLIBC_2.30, GLIBC_2.31 |
| `libdl.so.2` | GLIBC_2.2.5, GLIBC_2.3.3, GLIBC_2.3.4 |
| `librt.so.1` | GLIBC_2.2.5, GLIBC_2.3.3, GLIBC_2.3.4, GLIBC_2.4, GLIBC_2.7 |
| `libutil.so.1` | GLIBC_2.2.5 |
| `libanl.so.1` | GLIBC_2.2.5 |
| `libresolv.so.2` | GLIBC_2.2.5, GLIBC_2.3.2, GLIBC_2.9 |
| `libnsl.so.1` | GLIBC_2.2.5 |
| `libthread_db.so.1` | GLIBC_2.2.5, GLIBC_2.3, GLIBC_2.3.3 |
| `libBrokenLocale.so.1` | GLIBC_2.2.5 |
| `libc_malloc_debug.so.0` | GLIBC_2.2.5, GLIBC_2.10, GLIBC_2.16, GLIBC_2.33 |
| `ld-linux-x86-64.so.2` | GLIBC_2.2.5, GLIBC_2.3, GLIBC_2.34, GLIBC_2.35 |

**Totals.** 3943 distinct public names (the same count as Appendix B). 262 names have more than
one version (C.3). 51 names are exported by more than one library (C.4).
537 names are **compatibility-only**: exported with `@` bindings only, so
old binaries can resolve them but new programs cannot link against them.

### C.3 Names with more than one version

Each row lists all public versions; **bold** is the default (`@@`) version, and the others are compatibility
versions with the older contracts that C0.6 describes. An implementation exports every listed version.

| Name | Library | Versions |
|---|---|---|
| `__isnanf128` | `libc.so.6` | GLIBC_2.26, **GLIBC_2.34** |
| `__libc_start_main` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `__libdl_version_placeholder` | `libdl.so.2` | GLIBC_2.2.5, GLIBC_2.3.3, GLIBC_2.3.4 |
| `__libpthread_version_placeholder` | `libpthread.so.0` | GLIBC_2.2.5, GLIBC_2.2.6, GLIBC_2.3.2, GLIBC_2.3.3, GLIBC_2.3.4, GLIBC_2.4, GLIBC_2.11, GLIBC_2.12, GLIBC_2.18, GLIBC_2.28, GLIBC_2.30, GLIBC_2.31 |
| `__librt_version_placeholder` | `librt.so.1` | GLIBC_2.2.5, GLIBC_2.3.3, GLIBC_2.3.4, GLIBC_2.4, GLIBC_2.7 |
| `__mq_open_2` | `libc.so.6` | GLIBC_2.7, **GLIBC_2.34** |
| `__pthread_cleanup_routine` | `libc.so.6` | GLIBC_2.3.3, **GLIBC_2.34** |
| `__pthread_key_create` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `__pthread_register_cancel` | `libc.so.6` | GLIBC_2.3.3, **GLIBC_2.34** |
| `__pthread_register_cancel_defer` | `libc.so.6` | GLIBC_2.3.3, **GLIBC_2.34** |
| `__pthread_unregister_cancel` | `libc.so.6` | GLIBC_2.3.3, **GLIBC_2.34** |
| `__pthread_unregister_cancel_restore` | `libc.so.6` | GLIBC_2.3.3, **GLIBC_2.34** |
| `__pthread_unwind_next` | `libc.so.6` | GLIBC_2.3.3, **GLIBC_2.34** |
| `_pthread_cleanup_pop` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `_pthread_cleanup_push` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `_sys_errlist` | `libc.so.6` | GLIBC_2.2.5, GLIBC_2.3, GLIBC_2.4, GLIBC_2.12 |
| `_sys_nerr` | `libc.so.6` | GLIBC_2.2.5, GLIBC_2.3, GLIBC_2.4, GLIBC_2.12 |
| `_sys_siglist` | `libc.so.6` | GLIBC_2.2.5, GLIBC_2.3.3 |
| `aio_cancel` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `aio_cancel64` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `aio_error` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `aio_error64` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `aio_fsync` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `aio_fsync64` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `aio_init` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `aio_read` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `aio_read64` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `aio_return` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `aio_return64` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `aio_suspend` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `aio_suspend64` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `aio_write` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `aio_write64` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `call_once` | `libc.so.6` | GLIBC_2.28, **GLIBC_2.34** |
| `clock_getcpuclockid` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.17** |
| `clock_getres` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.17** |
| `clock_gettime` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.17** |
| `clock_nanosleep` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.17** |
| `clock_settime` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.17** |
| `cnd_broadcast` | `libc.so.6` | GLIBC_2.28, **GLIBC_2.34** |
| `cnd_destroy` | `libc.so.6` | GLIBC_2.28, **GLIBC_2.34** |
| `cnd_init` | `libc.so.6` | GLIBC_2.28, **GLIBC_2.34** |
| `cnd_signal` | `libc.so.6` | GLIBC_2.28, **GLIBC_2.34** |
| `cnd_timedwait` | `libc.so.6` | GLIBC_2.28, **GLIBC_2.34** |
| `cnd_wait` | `libc.so.6` | GLIBC_2.28, **GLIBC_2.34** |
| `dladdr` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `dladdr1` | `libc.so.6` | GLIBC_2.3.3, **GLIBC_2.34** |
| `dlclose` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `dlerror` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `dlinfo` | `libc.so.6` | GLIBC_2.3.3, **GLIBC_2.34** |
| `dlmopen` | `libc.so.6` | GLIBC_2.3.4, **GLIBC_2.34** |
| `dlopen` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `dlsym` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `dlvsym` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `exp` | `libm.so.6` | GLIBC_2.2.5, **GLIBC_2.29** |
| `exp10` | `libm.so.6` | GLIBC_2.2.5, **GLIBC_2.39** |
| `exp10f` | `libm.so.6` | GLIBC_2.2.5, **GLIBC_2.32** |
| `exp2` | `libm.so.6` | GLIBC_2.2.5, **GLIBC_2.29** |
| `exp2f` | `libm.so.6` | GLIBC_2.2.5, **GLIBC_2.27** |
| `expf` | `libm.so.6` | GLIBC_2.2.5, **GLIBC_2.27** |
| `fmemopen` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.22** |
| `fmod` | `libm.so.6` | GLIBC_2.2.5, **GLIBC_2.38** |
| `fmodf` | `libm.so.6` | GLIBC_2.2.5, **GLIBC_2.38** |
| `forkpty` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `gai_cancel` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `gai_error` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `gai_suspend` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `getaddrinfo_a` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `glob` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.27** |
| `glob64` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.27** |
| `hypot` | `libm.so.6` | GLIBC_2.2.5, **GLIBC_2.35** |
| `hypotf` | `libm.so.6` | GLIBC_2.2.5, **GLIBC_2.35** |
| `lgamma` | `libm.so.6` | GLIBC_2.2.5, **GLIBC_2.23** |
| `lgammaf` | `libm.so.6` | GLIBC_2.2.5, **GLIBC_2.23** |
| `lgammal` | `libm.so.6` | GLIBC_2.2.5, **GLIBC_2.23** |
| `lio_listio` | `libc.so.6` | GLIBC_2.2.5, GLIBC_2.4, **GLIBC_2.34** |
| `lio_listio64` | `libc.so.6` | GLIBC_2.2.5, GLIBC_2.4, **GLIBC_2.34** |
| `log` | `libm.so.6` | GLIBC_2.2.5, **GLIBC_2.29** |
| `log2` | `libm.so.6` | GLIBC_2.2.5, **GLIBC_2.29** |
| `log2f` | `libm.so.6` | GLIBC_2.2.5, **GLIBC_2.27** |
| `logf` | `libm.so.6` | GLIBC_2.2.5, **GLIBC_2.27** |
| `login` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `login_tty` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `logout` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `logwtmp` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `memcpy` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.14** |
| `mq_close` | `libc.so.6` | GLIBC_2.3.4, **GLIBC_2.34** |
| `mq_getattr` | `libc.so.6` | GLIBC_2.3.4, **GLIBC_2.34** |
| `mq_notify` | `libc.so.6` | GLIBC_2.3.4, **GLIBC_2.34** |
| `mq_open` | `libc.so.6` | GLIBC_2.3.4, **GLIBC_2.34** |
| `mq_receive` | `libc.so.6` | GLIBC_2.3.4, **GLIBC_2.34** |
| `mq_send` | `libc.so.6` | GLIBC_2.3.4, **GLIBC_2.34** |
| `mq_setattr` | `libc.so.6` | GLIBC_2.3.4, **GLIBC_2.34** |
| `mq_timedreceive` | `libc.so.6` | GLIBC_2.3.4, **GLIBC_2.34** |
| `mq_timedsend` | `libc.so.6` | GLIBC_2.3.4, **GLIBC_2.34** |
| `mq_unlink` | `libc.so.6` | GLIBC_2.3.4, **GLIBC_2.34** |
| `mtx_destroy` | `libc.so.6` | GLIBC_2.28, **GLIBC_2.34** |
| `mtx_init` | `libc.so.6` | GLIBC_2.28, **GLIBC_2.34** |
| `mtx_lock` | `libc.so.6` | GLIBC_2.28, **GLIBC_2.34** |
| `mtx_timedlock` | `libc.so.6` | GLIBC_2.28, **GLIBC_2.34** |
| `mtx_trylock` | `libc.so.6` | GLIBC_2.28, **GLIBC_2.34** |
| `mtx_unlock` | `libc.so.6` | GLIBC_2.28, **GLIBC_2.34** |
| `nftw` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.3.3** |
| `nftw64` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.3.3** |
| `ns_name_compress` | `libc.so.6` | GLIBC_2.9, **GLIBC_2.34** |
| `ns_name_ntop` | `libc.so.6` | GLIBC_2.9, **GLIBC_2.34** |
| `ns_name_pack` | `libc.so.6` | GLIBC_2.9, **GLIBC_2.34** |
| `ns_name_pton` | `libc.so.6` | GLIBC_2.9, **GLIBC_2.34** |
| `ns_name_skip` | `libc.so.6` | GLIBC_2.9, **GLIBC_2.34** |
| `ns_name_uncompress` | `libc.so.6` | GLIBC_2.9, **GLIBC_2.34** |
| `ns_name_unpack` | `libc.so.6` | GLIBC_2.9, **GLIBC_2.34** |
| `openpty` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `posix_spawn` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.15** |
| `posix_spawnp` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.15** |
| `pow` | `libm.so.6` | GLIBC_2.2.5, **GLIBC_2.29** |
| `powf` | `libm.so.6` | GLIBC_2.2.5, **GLIBC_2.27** |
| `pthread_attr_getaffinity_np` | `libc.so.6` | GLIBC_2.3.3, GLIBC_2.3.4, **GLIBC_2.34** |
| `pthread_attr_getguardsize` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_attr_getstack` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_attr_getstackaddr` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_attr_getstacksize` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_attr_setaffinity_np` | `libc.so.6` | GLIBC_2.3.3, GLIBC_2.3.4, **GLIBC_2.32** |
| `pthread_attr_setguardsize` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_attr_setstack` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_attr_setstackaddr` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_attr_setstacksize` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_barrier_destroy` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_barrier_init` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_barrier_wait` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_barrierattr_destroy` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_barrierattr_getpshared` | `libc.so.6` | GLIBC_2.3.3, **GLIBC_2.34** |
| `pthread_barrierattr_init` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_barrierattr_setpshared` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_cancel` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_clockjoin_np` | `libc.so.6` | GLIBC_2.31, **GLIBC_2.34** |
| `pthread_cond_broadcast` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.3.2** |
| `pthread_cond_clockwait` | `libc.so.6` | GLIBC_2.30, **GLIBC_2.34** |
| `pthread_cond_destroy` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.3.2** |
| `pthread_cond_init` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.3.2** |
| `pthread_cond_signal` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.3.2** |
| `pthread_cond_timedwait` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.3.2** |
| `pthread_cond_wait` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.3.2** |
| `pthread_condattr_getclock` | `libc.so.6` | GLIBC_2.3.3, **GLIBC_2.34** |
| `pthread_condattr_getpshared` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_condattr_setclock` | `libc.so.6` | GLIBC_2.3.3, **GLIBC_2.34** |
| `pthread_condattr_setpshared` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_create` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_detach` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_getaffinity_np` | `libc.so.6` | GLIBC_2.3.3, GLIBC_2.3.4, **GLIBC_2.32** |
| `pthread_getattr_default_np` | `libc.so.6` | GLIBC_2.18, **GLIBC_2.34** |
| `pthread_getattr_np` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.32** |
| `pthread_getconcurrency` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_getcpuclockid` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_getname_np` | `libc.so.6` | GLIBC_2.12, **GLIBC_2.34** |
| `pthread_getspecific` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_join` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_key_create` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_key_delete` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_kill` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_mutex_clocklock` | `libc.so.6` | GLIBC_2.30, **GLIBC_2.34** |
| `pthread_mutex_consistent` | `libc.so.6` | GLIBC_2.12, **GLIBC_2.34** |
| `pthread_mutex_getprioceiling` | `libc.so.6` | GLIBC_2.4, **GLIBC_2.34** |
| `pthread_mutex_setprioceiling` | `libc.so.6` | GLIBC_2.4, **GLIBC_2.34** |
| `pthread_mutex_timedlock` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_mutex_trylock` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_mutexattr_destroy` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_mutexattr_getprioceiling` | `libc.so.6` | GLIBC_2.4, **GLIBC_2.34** |
| `pthread_mutexattr_getprotocol` | `libc.so.6` | GLIBC_2.4, **GLIBC_2.34** |
| `pthread_mutexattr_getpshared` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_mutexattr_getrobust` | `libc.so.6` | GLIBC_2.12, **GLIBC_2.34** |
| `pthread_mutexattr_gettype` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_mutexattr_init` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_mutexattr_setprioceiling` | `libc.so.6` | GLIBC_2.4, **GLIBC_2.34** |
| `pthread_mutexattr_setprotocol` | `libc.so.6` | GLIBC_2.4, **GLIBC_2.34** |
| `pthread_mutexattr_setpshared` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_mutexattr_setrobust` | `libc.so.6` | GLIBC_2.12, **GLIBC_2.34** |
| `pthread_mutexattr_settype` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_once` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_rwlock_clockrdlock` | `libc.so.6` | GLIBC_2.30, **GLIBC_2.34** |
| `pthread_rwlock_clockwrlock` | `libc.so.6` | GLIBC_2.30, **GLIBC_2.34** |
| `pthread_rwlock_destroy` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_rwlock_init` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_rwlock_rdlock` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_rwlock_timedrdlock` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_rwlock_timedwrlock` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_rwlock_tryrdlock` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_rwlock_trywrlock` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_rwlock_unlock` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_rwlock_wrlock` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_rwlockattr_destroy` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_rwlockattr_getkind_np` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_rwlockattr_getpshared` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_rwlockattr_init` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_rwlockattr_setkind_np` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_rwlockattr_setpshared` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_setaffinity_np` | `libc.so.6` | GLIBC_2.3.3, GLIBC_2.3.4, **GLIBC_2.34** |
| `pthread_setattr_default_np` | `libc.so.6` | GLIBC_2.18, **GLIBC_2.34** |
| `pthread_setconcurrency` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_setname_np` | `libc.so.6` | GLIBC_2.12, **GLIBC_2.34** |
| `pthread_setschedprio` | `libc.so.6` | GLIBC_2.3.4, **GLIBC_2.34** |
| `pthread_setspecific` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_sigmask` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.32** |
| `pthread_sigqueue` | `libc.so.6` | GLIBC_2.11, **GLIBC_2.34** |
| `pthread_spin_destroy` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_spin_init` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_spin_lock` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_spin_trylock` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_spin_unlock` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_testcancel` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `pthread_timedjoin_np` | `libc.so.6` | GLIBC_2.3.3, **GLIBC_2.34** |
| `pthread_tryjoin_np` | `libc.so.6` | GLIBC_2.3.3, **GLIBC_2.34** |
| `quick_exit` | `libc.so.6` | GLIBC_2.10, **GLIBC_2.24** |
| `realpath` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.3** |
| `regexec` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.3.4** |
| `sched_getaffinity` | `libc.so.6` | GLIBC_2.3.3, **GLIBC_2.3.4** |
| `sched_setaffinity` | `libc.so.6` | GLIBC_2.3.3, **GLIBC_2.3.4** |
| `sem_clockwait` | `libc.so.6` | GLIBC_2.30, **GLIBC_2.34** |
| `sem_close` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `sem_destroy` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `sem_getvalue` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `sem_init` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `sem_open` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `sem_post` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `sem_timedwait` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `sem_trywait` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `sem_unlink` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `sem_wait` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `shm_open` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `shm_unlink` | `libc.so.6` | GLIBC_2.2.5, **GLIBC_2.34** |
| `sys_errlist` | `libc.so.6` | GLIBC_2.2.5, GLIBC_2.3, GLIBC_2.4, GLIBC_2.12 |
| `sys_nerr` | `libc.so.6` | GLIBC_2.2.5, GLIBC_2.3, GLIBC_2.4, GLIBC_2.12 |
| `sys_sigabbrev` | `libc.so.6` | GLIBC_2.2.5, GLIBC_2.3.3 |
| `sys_siglist` | `libc.so.6` | GLIBC_2.2.5, GLIBC_2.3.3 |
| `thrd_create` | `libc.so.6` | GLIBC_2.28, **GLIBC_2.34** |
| `thrd_detach` | `libc.so.6` | GLIBC_2.28, **GLIBC_2.34** |
| `thrd_exit` | `libc.so.6` | GLIBC_2.28, **GLIBC_2.34** |
| `thrd_join` | `libc.so.6` | GLIBC_2.28, **GLIBC_2.34** |
| `timer_create` | `libc.so.6` | GLIBC_2.2.5, GLIBC_2.3.3, **GLIBC_2.34** |
| `timer_delete` | `libc.so.6` | GLIBC_2.2.5, GLIBC_2.3.3, **GLIBC_2.34** |
| `timer_getoverrun` | `libc.so.6` | GLIBC_2.2.5, GLIBC_2.3.3, **GLIBC_2.34** |
| `timer_gettime` | `libc.so.6` | GLIBC_2.2.5, GLIBC_2.3.3, **GLIBC_2.34** |
| `timer_settime` | `libc.so.6` | GLIBC_2.2.5, GLIBC_2.3.3, **GLIBC_2.34** |
| `totalorder` | `libm.so.6` | GLIBC_2.25, **GLIBC_2.31** |
| `totalorderf` | `libm.so.6` | GLIBC_2.25, **GLIBC_2.31** |
| `totalorderf128` | `libm.so.6` | GLIBC_2.26, **GLIBC_2.31** |
| `totalorderf32` | `libm.so.6` | GLIBC_2.27, **GLIBC_2.31** |
| `totalorderf32x` | `libm.so.6` | GLIBC_2.27, **GLIBC_2.31** |
| `totalorderf64` | `libm.so.6` | GLIBC_2.27, **GLIBC_2.31** |
| `totalorderf64x` | `libm.so.6` | GLIBC_2.27, **GLIBC_2.31** |
| `totalorderl` | `libm.so.6` | GLIBC_2.25, **GLIBC_2.31** |
| `totalordermag` | `libm.so.6` | GLIBC_2.25, **GLIBC_2.31** |
| `totalordermagf` | `libm.so.6` | GLIBC_2.25, **GLIBC_2.31** |
| `totalordermagf128` | `libm.so.6` | GLIBC_2.26, **GLIBC_2.31** |
| `totalordermagf32` | `libm.so.6` | GLIBC_2.27, **GLIBC_2.31** |
| `totalordermagf32x` | `libm.so.6` | GLIBC_2.27, **GLIBC_2.31** |
| `totalordermagf64` | `libm.so.6` | GLIBC_2.27, **GLIBC_2.31** |
| `totalordermagf64x` | `libm.so.6` | GLIBC_2.27, **GLIBC_2.31** |
| `totalordermagl` | `libm.so.6` | GLIBC_2.25, **GLIBC_2.31** |
| `tss_create` | `libc.so.6` | GLIBC_2.28, **GLIBC_2.34** |
| `tss_delete` | `libc.so.6` | GLIBC_2.28, **GLIBC_2.34** |
| `tss_get` | `libc.so.6` | GLIBC_2.28, **GLIBC_2.34** |
| `tss_set` | `libc.so.6` | GLIBC_2.28, **GLIBC_2.34** |

### C.4 Names exported by more than one library

These names are exported, with the versions in `versions.tsv`, by every library listed. Most are `libm`
functions that `libc.so.6` also provides for historical reasons.

| Name | Libraries |
|---|---|
| `__ctype_get_mb_cur_max` | `libBrokenLocale.so.1`, `libc.so.6` |
| `__finite` | `libc.so.6`, `libm.so.6` |
| `__finitef` | `libc.so.6`, `libm.so.6` |
| `__finitel` | `libc.so.6`, `libm.so.6` |
| `__free_hook` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `__malloc_hook` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `__memalign_hook` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `__realloc_hook` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `__signbit` | `libc.so.6`, `libm.so.6` |
| `__signbitf` | `libc.so.6`, `libm.so.6` |
| `__signbitl` | `libc.so.6`, `libm.so.6` |
| `aligned_alloc` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `calloc` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `copysign` | `libc.so.6`, `libm.so.6` |
| `copysignf` | `libc.so.6`, `libm.so.6` |
| `copysignl` | `libc.so.6`, `libm.so.6` |
| `finite` | `libc.so.6`, `libm.so.6` |
| `finitef` | `libc.so.6`, `libm.so.6` |
| `finitel` | `libc.so.6`, `libm.so.6` |
| `free` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `frexp` | `libc.so.6`, `libm.so.6` |
| `frexpf` | `libc.so.6`, `libm.so.6` |
| `frexpl` | `libc.so.6`, `libm.so.6` |
| `ldexp` | `libc.so.6`, `libm.so.6` |
| `ldexpf` | `libc.so.6`, `libm.so.6` |
| `ldexpl` | `libc.so.6`, `libm.so.6` |
| `mallinfo` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `mallinfo2` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `malloc` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `malloc_info` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `malloc_stats` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `malloc_trim` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `malloc_usable_size` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `mallopt` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `mcheck` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `mcheck_check_all` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `mcheck_pedantic` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `memalign` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `modf` | `libc.so.6`, `libm.so.6` |
| `modff` | `libc.so.6`, `libm.so.6` |
| `modfl` | `libc.so.6`, `libm.so.6` |
| `mprobe` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `mtrace` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `muntrace` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `posix_memalign` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `pvalloc` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `realloc` | `libc.so.6`, `libc_malloc_debug.so.0` |
| `scalbn` | `libc.so.6`, `libm.so.6` |
| `scalbnf` | `libc.so.6`, `libm.so.6` |
| `scalbnl` | `libc.so.6`, `libm.so.6` |
| `valloc` | `libc.so.6`, `libc_malloc_debug.so.0` |

### C.5 Exported data objects

Data symbols are ABI in two ways: their layout (C.7) and their **size**, which old binaries capture through
copy relocations. An implementation must export each object with exactly this size per version.

| Name | Library | Version | Binding | Size (bytes) |
|---|---|---|---|---|
| `_IO_2_1_stderr_` | `libc.so.6` | GLIBC_2.2.5 | default | 224 |
| `_IO_2_1_stdin_` | `libc.so.6` | GLIBC_2.2.5 | default | 224 |
| `_IO_2_1_stdout_` | `libc.so.6` | GLIBC_2.2.5 | default | 224 |
| `_IO_file_jumps` | `libc.so.6` | GLIBC_2.2.5 | default | 168 |
| `_IO_list_all` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `_IO_wfile_jumps` | `libc.so.6` | GLIBC_2.2.5 | default | 168 |
| `_LIB_VERSION` | `libm.so.6` | GLIBC_2.2.5 | compat | 4 |
| `__after_morecore_hook` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `__check_rhosts_file` | `libc.so.6` | GLIBC_2.2.5 | default | 4 |
| `__ctype32_b` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `__ctype32_tolower` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `__ctype32_toupper` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `__ctype_b` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `__ctype_tolower` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `__ctype_toupper` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `__curbrk` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `__daylight` | `libc.so.6` | GLIBC_2.2.5 | default | 4 |
| `__environ` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `__fpu_control` | `libc.so.6` | GLIBC_2.2.5 | default | 2 |
| `__free_hook` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `__free_hook` | `libc_malloc_debug.so.0` | GLIBC_2.2.5 | compat | 8 |
| `__key_decryptsession_pk_LOCAL` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `__key_encryptsession_pk_LOCAL` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `__key_gendes_LOCAL` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `__libc_single_threaded` | `libc.so.6` | GLIBC_2.32 | default | 1 |
| `__libc_stack_end` | `ld-linux-x86-64.so.2` | GLIBC_2.2.5 | default | 8 |
| `__malloc_hook` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `__malloc_hook` | `libc_malloc_debug.so.0` | GLIBC_2.2.5 | compat | 8 |
| `__malloc_initialize_hook` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `__memalign_hook` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `__memalign_hook` | `libc_malloc_debug.so.0` | GLIBC_2.2.5 | compat | 8 |
| `__morecore` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `__p_class_syms` | `libresolv.so.2` | GLIBC_2.2.5 | default | 168 |
| `__p_type_syms` | `libresolv.so.2` | GLIBC_2.2.5 | default | 1104 |
| `__progname` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `__progname_full` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `__rcmd_errstr` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `__realloc_hook` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `__realloc_hook` | `libc_malloc_debug.so.0` | GLIBC_2.2.5 | compat | 8 |
| `__rseq_flags` | `ld-linux-x86-64.so.2` | GLIBC_2.35 | default | 4 |
| `__rseq_offset` | `ld-linux-x86-64.so.2` | GLIBC_2.35 | default | 8 |
| `__rseq_size` | `ld-linux-x86-64.so.2` | GLIBC_2.35 | default | 4 |
| `__signgam` | `libm.so.6` | GLIBC_2.23 | default | 4 |
| `__timezone` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `__tzname` | `libc.so.6` | GLIBC_2.2.5 | default | 16 |
| `_environ` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `_libc_intl_domainname` | `libc.so.6` | GLIBC_2.2.5 | default | 5 |
| `_nl_default_dirname` | `libc.so.6` | GLIBC_2.2.5 | default | 18 |
| `_nl_domain_bindings` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `_nl_msg_cat_cntr` | `libc.so.6` | GLIBC_2.2.5 | default | 4 |
| `_null_auth` | `libc.so.6` | GLIBC_2.2.5 | compat | 24 |
| `_obstack` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `_r_debug` | `ld-linux-x86-64.so.2` | GLIBC_2.2.5 | default | 40 |
| `_res` | `libc.so.6` | GLIBC_2.2.5 | compat | 568 |
| `_res_hconf` | `libc.so.6` | GLIBC_2.2.5 | default | 72 |
| `_res_opcodes` | `libresolv.so.2` | GLIBC_2.2.5 | default | 128 |
| `_sys_errlist` | `libc.so.6` | GLIBC_2.2.5 | compat | 1000 |
| `_sys_errlist` | `libc.so.6` | GLIBC_2.3 | compat | 1008 |
| `_sys_errlist` | `libc.so.6` | GLIBC_2.4 | compat | 1056 |
| `_sys_errlist` | `libc.so.6` | GLIBC_2.12 | compat | 1080 |
| `_sys_nerr` | `libc.so.6` | GLIBC_2.2.5 | compat | 4 |
| `_sys_nerr` | `libc.so.6` | GLIBC_2.3 | compat | 4 |
| `_sys_nerr` | `libc.so.6` | GLIBC_2.4 | compat | 4 |
| `_sys_nerr` | `libc.so.6` | GLIBC_2.12 | compat | 4 |
| `_sys_siglist` | `libc.so.6` | GLIBC_2.2.5 | compat | 512 |
| `_sys_siglist` | `libc.so.6` | GLIBC_2.3.3 | compat | 520 |
| `argp_err_exit_status` | `libc.so.6` | GLIBC_2.2.5 | default | 4 |
| `argp_program_bug_address` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `argp_program_version` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `argp_program_version_hook` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `daylight` | `libc.so.6` | GLIBC_2.2.5 | default | 4 |
| `environ` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `error_message_count` | `libc.so.6` | GLIBC_2.2.5 | default | 4 |
| `error_one_per_line` | `libc.so.6` | GLIBC_2.2.5 | default | 4 |
| `error_print_progname` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `getdate_err` | `libc.so.6` | GLIBC_2.2.5 | default | 4 |
| `h_errlist` | `libc.so.6` | GLIBC_2.2.5 | default | 40 |
| `h_nerr` | `libc.so.6` | GLIBC_2.2.5 | default | 4 |
| `in6addr_any` | `libc.so.6` | GLIBC_2.2.5 | default | 16 |
| `in6addr_loopback` | `libc.so.6` | GLIBC_2.2.5 | default | 16 |
| `loc1` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `loc2` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `locs` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `mallwatch` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `obstack_alloc_failed_handler` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `obstack_exit_failure` | `libc.so.6` | GLIBC_2.2.5 | default | 4 |
| `optarg` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `opterr` | `libc.so.6` | GLIBC_2.2.5 | default | 4 |
| `optind` | `libc.so.6` | GLIBC_2.2.5 | default | 4 |
| `optopt` | `libc.so.6` | GLIBC_2.2.5 | default | 4 |
| `program_invocation_name` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `program_invocation_short_name` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `re_max_failures` | `libc.so.6` | GLIBC_2.2.5 | default | 4 |
| `re_syntax_options` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `rexecoptions` | `libc.so.6` | GLIBC_2.2.5 | default | 4 |
| `rpc_createerr` | `libc.so.6` | GLIBC_2.2.5 | compat | 32 |
| `signgam` | `libm.so.6` | GLIBC_2.2.5 | default | 4 |
| `stderr` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `stdin` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `stdout` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `svc_fdset` | `libc.so.6` | GLIBC_2.2.5 | compat | 128 |
| `svc_max_pollfd` | `libc.so.6` | GLIBC_2.2.5 | compat | 4 |
| `svc_pollfd` | `libc.so.6` | GLIBC_2.2.5 | compat | 8 |
| `svcauthdes_stats` | `libc.so.6` | GLIBC_2.2.5 | compat | 24 |
| `sys_errlist` | `libc.so.6` | GLIBC_2.2.5 | compat | 1000 |
| `sys_errlist` | `libc.so.6` | GLIBC_2.3 | compat | 1008 |
| `sys_errlist` | `libc.so.6` | GLIBC_2.4 | compat | 1056 |
| `sys_errlist` | `libc.so.6` | GLIBC_2.12 | compat | 1080 |
| `sys_nerr` | `libc.so.6` | GLIBC_2.2.5 | compat | 4 |
| `sys_nerr` | `libc.so.6` | GLIBC_2.3 | compat | 4 |
| `sys_nerr` | `libc.so.6` | GLIBC_2.4 | compat | 4 |
| `sys_nerr` | `libc.so.6` | GLIBC_2.12 | compat | 4 |
| `sys_sigabbrev` | `libc.so.6` | GLIBC_2.2.5 | compat | 512 |
| `sys_sigabbrev` | `libc.so.6` | GLIBC_2.3.3 | compat | 520 |
| `sys_siglist` | `libc.so.6` | GLIBC_2.2.5 | compat | 512 |
| `sys_siglist` | `libc.so.6` | GLIBC_2.3.3 | compat | 520 |
| `timezone` | `libc.so.6` | GLIBC_2.2.5 | default | 8 |
| `tzname` | `libc.so.6` | GLIBC_2.2.5 | default | 16 |

### C.6 Type sizes and alignments

| Type | Size | Align |
|---|---|---|
| `FILE` | 216 | 8 |
| `fpos_t` | 16 | 8 |
| `cookie_io_functions_t` | 32 | 8 |
| `va_list` | 24 | 8 |
| `max_align_t` | 32 | 16 |
| `div_t` | 8 | 4 |
| `ldiv_t` | 16 | 8 |
| `lldiv_t` | 16 | 8 |
| `imaxdiv_t` | 16 | 8 |
| `struct drand48_data` | 24 | 8 |
| `struct random_data` | 48 | 8 |
| `mbstate_t` | 8 | 4 |
| `struct __locale_struct` | 232 | 8 |
| `struct lconv` | 96 | 8 |
| `jmp_buf` | 200 | 8 |
| `sigjmp_buf` | 200 | 8 |
| `struct __jmp_buf_tag` | 200 | 8 |
| `sigset_t` | 128 | 8 |
| `siginfo_t` | 128 | 8 |
| `struct sigaction` | 152 | 8 |
| `stack_t` | 24 | 8 |
| `struct sigstack` | 16 | 8 |
| `ucontext_t` | 968 | 8 |
| `mcontext_t` | 256 | 8 |
| `union sigval` | 8 | 8 |
| `struct sigevent` | 64 | 8 |
| `pthread_t` | 8 | 8 |
| `pthread_attr_t` | 56 | 8 |
| `pthread_mutex_t` | 40 | 8 |
| `pthread_mutexattr_t` | 4 | 4 |
| `pthread_cond_t` | 48 | 8 |
| `pthread_condattr_t` | 4 | 4 |
| `pthread_rwlock_t` | 56 | 8 |
| `pthread_rwlockattr_t` | 8 | 8 |
| `pthread_spinlock_t` | 4 | 4 |
| `pthread_barrier_t` | 32 | 8 |
| `pthread_barrierattr_t` | 4 | 4 |
| `pthread_key_t` | 4 | 4 |
| `pthread_once_t` | 4 | 4 |
| `struct _pthread_cleanup_buffer` | 32 | 8 |
| `__pthread_unwind_buf_t` | 104 | 16 |
| `cpu_set_t` | 128 | 8 |
| `struct sched_param` | 4 | 4 |
| `sem_t` | 32 | 8 |
| `thrd_t` | 8 | 8 |
| `mtx_t` | 40 | 8 |
| `cnd_t` | 48 | 8 |
| `tss_t` | 4 | 4 |
| `once_flag` | 4 | 4 |
| `struct timespec` | 16 | 8 |
| `struct timeval` | 16 | 8 |
| `struct tm` | 56 | 8 |
| `struct itimerval` | 32 | 8 |
| `struct itimerspec` | 32 | 8 |
| `struct timezone` | 8 | 4 |
| `struct timex` | 208 | 8 |
| `struct ntptimeval` | 72 | 8 |
| `struct tms` | 32 | 8 |
| `struct timeb` | 16 | 8 |
| `struct rusage` | 144 | 8 |
| `struct rlimit` | 16 | 8 |
| `struct utsname` | 390 | 1 |
| `struct sysinfo` | 112 | 8 |
| `struct stat` | 144 | 8 |
| `struct statx` | 256 | 8 |
| `struct statfs` | 120 | 8 |
| `struct statvfs` | 112 | 8 |
| `struct dirent` | 280 | 8 |
| `struct dirent64` | 280 | 8 |
| `FTS` | 72 | 8 |
| `FTSENT` | 120 | 8 |
| `struct FTW` | 8 | 4 |
| `glob_t` | 72 | 8 |
| `regex_t` | 64 | 8 |
| `regmatch_t` | 8 | 4 |
| `struct re_registers` | 24 | 8 |
| `wordexp_t` | 24 | 8 |
| `ENTRY` | 16 | 8 |
| `struct hsearch_data` | 16 | 8 |
| `struct qelem` | 24 | 8 |
| `struct termios` | 60 | 4 |
| `struct winsize` | 8 | 2 |
| `struct utmp` | 384 | 4 |
| `struct utmpx` | 384 | 4 |
| `struct lastlog` | 292 | 4 |
| `struct exit_status` | 4 | 2 |
| `struct passwd` | 48 | 8 |
| `struct group` | 32 | 8 |
| `struct spwd` | 72 | 8 |
| `struct sgrp` | 32 | 8 |
| `struct hostent` | 32 | 8 |
| `struct netent` | 24 | 8 |
| `struct servent` | 32 | 8 |
| `struct protoent` | 24 | 8 |
| `struct rpcent` | 24 | 8 |
| `struct addrinfo` | 48 | 8 |
| `struct gaicb` | 56 | 8 |
| `struct aliasent` | 32 | 8 |
| `struct __res_state` | 568 | 8 |
| `ns_msg` | 80 | 8 |
| `ns_rr` | 1048 | 8 |
| `struct in_addr` | 4 | 4 |
| `struct in6_addr` | 16 | 4 |
| `struct sockaddr` | 16 | 2 |
| `struct sockaddr_storage` | 128 | 8 |
| `struct sockaddr_in` | 16 | 4 |
| `struct sockaddr_in6` | 28 | 4 |
| `struct sockaddr_un` | 110 | 2 |
| `struct ether_addr` | 6 | 1 |
| `struct ifaddrs` | 56 | 8 |
| `struct if_nameindex` | 16 | 8 |
| `struct ifreq` | 40 | 8 |
| `struct ip_mreq` | 8 | 4 |
| `struct group_source_req` | 264 | 8 |
| `struct ip6_mtuinfo` | 32 | 4 |
| `struct msghdr` | 56 | 8 |
| `struct cmsghdr` | 16 | 8 |
| `struct mmsghdr` | 64 | 8 |
| `struct iovec` | 16 | 8 |
| `struct linger` | 8 | 4 |
| `struct ucred` | 12 | 4 |
| `struct pollfd` | 8 | 4 |
| `struct epoll_event` | 12 | 1 |
| `fd_set` | 128 | 8 |
| `struct aiocb` | 168 | 8 |
| `struct aioinit` | 32 | 4 |
| `struct mq_attr` | 64 | 8 |
| `struct ipc_perm` | 48 | 8 |
| `struct msqid_ds` | 120 | 8 |
| `struct semid_ds` | 104 | 8 |
| `struct sembuf` | 6 | 2 |
| `struct shmid_ds` | 112 | 8 |
| `posix_spawnattr_t` | 336 | 8 |
| `posix_spawn_file_actions_t` | 80 | 8 |
| `Dl_info` | 32 | 8 |
| `struct dl_phdr_info` | 64 | 8 |
| `struct link_map` | 40 | 8 |
| `struct r_debug` | 40 | 8 |
| `struct dl_find_object` | 96 | 8 |
| `Dl_serinfo` | 32 | 8 |
| `struct mntent` | 40 | 8 |
| `struct fstab` | 48 | 8 |
| `struct ttyent` | 48 | 8 |
| `struct option` | 32 | 8 |
| `struct argp` | 56 | 8 |
| `struct argp_option` | 48 | 8 |
| `struct argp_state` | 96 | 8 |
| `struct argp_child` | 32 | 8 |
| `struct obstack` | 88 | 8 |
| `struct printf_info` | 20 | 4 |
| `struct mallinfo` | 40 | 4 |
| `struct mallinfo2` | 80 | 8 |
| `fenv_t` | 32 | 4 |
| `femode_t` | 8 | 4 |
| `fexcept_t` | 2 | 2 |
| `struct inotify_event` | 16 | 4 |
| `struct signalfd_siginfo` | 128 | 8 |
| `struct rseq` | 32 | 32 |
| `struct user_regs_struct` | 216 | 8 |
| `struct user_fpregs_struct` | 512 | 8 |
| `struct dqblk` | 72 | 8 |

### C.7 Member layouts of the types most exposed to callers

Offsets in bytes from the start of the object. Nested members are shown with dotted names. The complete
layouts of all types in C.6 are in `layouts.tsv`.

**`FILE`** (216 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `_flags` | `int` |
| 8 | `_IO_read_ptr` | `char *` |
| 16 | `_IO_read_end` | `char *` |
| 24 | `_IO_read_base` | `char *` |
| 32 | `_IO_write_base` | `char *` |
| 40 | `_IO_write_ptr` | `char *` |
| 48 | `_IO_write_end` | `char *` |
| 56 | `_IO_buf_base` | `char *` |
| 64 | `_IO_buf_end` | `char *` |
| 72 | `_IO_save_base` | `char *` |
| 80 | `_IO_backup_base` | `char *` |
| 88 | `_IO_save_end` | `char *` |
| 96 | `_markers` | `struct _IO_marker *` |
| 104 | `_chain` | `struct _IO_FILE *` |
| 112 | `_fileno` | `int` |
| 116 | `_flags2` | `int` |
| 120 | `_old_offset` | `__off_t` |
| 128 | `_cur_column` | `short unsigned int` |
| 130 | `_vtable_offset` | `signed char` |
| 131 | `_shortbuf` | `char[1]` |
| 136 | `_lock` | `_IO_lock_t *` |
| 144 | `_offset` | `__off64_t` |
| 152 | `_codecvt` | `struct _IO_codecvt *` |
| 160 | `_wide_data` | `struct _IO_wide_data *` |
| 168 | `_freeres_list` | `struct _IO_FILE *` |
| 176 | `_freeres_buf` | `void *` |
| 184 | `__pad5` | `size_t` |
| 192 | `_mode` | `int` |
| 196 | `_unused2` | `char[20]` |

**`mbstate_t`** (8 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `__count` | `int` |
| 4 | `__value` | `union <anon>` |
| 4 | `__value.__wch` | `unsigned int` |
| 4 | `__value.__wchb` | `char[4]` |

**`sigset_t`** (128 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `__val` | `long unsigned int[16]` |

**`struct __jmp_buf_tag`** (200 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `__jmpbuf` | `__jmp_buf` |
| 64 | `__mask_was_saved` | `int` |
| 72 | `__saved_mask` | `__sigset_t` |
| 72 | `__saved_mask.__val` | `long unsigned int[16]` |

**`__pthread_unwind_buf_t`** (104 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `__cancel_jmp_buf` | `struct __cancel_jmp_buf_tag[1]` |
| 72 | `__pad` | `void *[4]` |

**`struct sigaction`** (152 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `__sigaction_handler` | `union <anon>` |
| 0 | `__sigaction_handler.sa_handler` | `__sighandler_t` |
| 0 | `__sigaction_handler.sa_sigaction` | `fn(...) *` |
| 8 | `sa_mask` | `__sigset_t` |
| 8 | `sa_mask.__val` | `long unsigned int[16]` |
| 136 | `sa_flags` | `int` |
| 144 | `sa_restorer` | `fn(...) *` |

**`struct tm`** (56 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `tm_sec` | `int` |
| 4 | `tm_min` | `int` |
| 8 | `tm_hour` | `int` |
| 12 | `tm_mday` | `int` |
| 16 | `tm_mon` | `int` |
| 20 | `tm_year` | `int` |
| 24 | `tm_wday` | `int` |
| 28 | `tm_yday` | `int` |
| 32 | `tm_isdst` | `int` |
| 40 | `tm_gmtoff` | `long int` |
| 48 | `tm_zone` | `const char *` |

**`struct termios`** (60 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `c_iflag` | `tcflag_t` |
| 4 | `c_oflag` | `tcflag_t` |
| 8 | `c_cflag` | `tcflag_t` |
| 12 | `c_lflag` | `tcflag_t` |
| 16 | `c_line` | `cc_t` |
| 17 | `c_cc` | `cc_t[32]` |
| 52 | `c_ispeed` | `speed_t` |
| 56 | `c_ospeed` | `speed_t` |

**`struct dirent`** (280 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `d_ino` | `__ino_t` |
| 8 | `d_off` | `__off_t` |
| 16 | `d_reclen` | `short unsigned int` |
| 18 | `d_type` | `unsigned char` |
| 19 | `d_name` | `char[256]` |

**`struct utmp`** (384 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `ut_type` | `short int` |
| 4 | `ut_pid` | `pid_t` |
| 8 | `ut_line` | `char[32]` |
| 40 | `ut_id` | `char[4]` |
| 44 | `ut_user` | `char[32]` |
| 76 | `ut_host` | `char[256]` |
| 332 | `ut_exit` | `struct exit_status` |
| 332 | `ut_exit.e_termination` | `short int` |
| 334 | `ut_exit.e_exit` | `short int` |
| 336 | `ut_session` | `int32_t` |
| 340 | `ut_tv` | `struct <anon>` |
| 340 | `ut_tv.tv_sec` | `int32_t` |
| 344 | `ut_tv.tv_usec` | `int32_t` |
| 348 | `ut_addr_v6` | `int32_t[4]` |
| 364 | `__glibc_reserved` | `char[20]` |

**`struct passwd`** (48 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `pw_name` | `char *` |
| 8 | `pw_passwd` | `char *` |
| 16 | `pw_uid` | `__uid_t` |
| 20 | `pw_gid` | `__gid_t` |
| 24 | `pw_gecos` | `char *` |
| 32 | `pw_dir` | `char *` |
| 40 | `pw_shell` | `char *` |

**`struct group`** (32 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `gr_name` | `char *` |
| 8 | `gr_passwd` | `char *` |
| 16 | `gr_gid` | `__gid_t` |
| 24 | `gr_mem` | `char * *` |

**`struct hostent`** (32 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `h_name` | `char *` |
| 8 | `h_aliases` | `char * *` |
| 16 | `h_addrtype` | `int` |
| 20 | `h_length` | `int` |
| 24 | `h_addr_list` | `char * *` |

**`struct addrinfo`** (48 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `ai_flags` | `int` |
| 4 | `ai_family` | `int` |
| 8 | `ai_socktype` | `int` |
| 12 | `ai_protocol` | `int` |
| 16 | `ai_addrlen` | `socklen_t` |
| 24 | `ai_addr` | `struct sockaddr *` |
| 32 | `ai_canonname` | `char *` |
| 40 | `ai_next` | `struct addrinfo *` |

**`struct lconv`** (96 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `decimal_point` | `char *` |
| 8 | `thousands_sep` | `char *` |
| 16 | `grouping` | `char *` |
| 24 | `int_curr_symbol` | `char *` |
| 32 | `currency_symbol` | `char *` |
| 40 | `mon_decimal_point` | `char *` |
| 48 | `mon_thousands_sep` | `char *` |
| 56 | `mon_grouping` | `char *` |
| 64 | `positive_sign` | `char *` |
| 72 | `negative_sign` | `char *` |
| 80 | `int_frac_digits` | `char` |
| 81 | `frac_digits` | `char` |
| 82 | `p_cs_precedes` | `char` |
| 83 | `p_sep_by_space` | `char` |
| 84 | `n_cs_precedes` | `char` |
| 85 | `n_sep_by_space` | `char` |
| 86 | `p_sign_posn` | `char` |
| 87 | `n_sign_posn` | `char` |
| 88 | `int_p_cs_precedes` | `char` |
| 89 | `int_p_sep_by_space` | `char` |
| 90 | `int_n_cs_precedes` | `char` |
| 91 | `int_n_sep_by_space` | `char` |
| 92 | `int_p_sign_posn` | `char` |
| 93 | `int_n_sign_posn` | `char` |

**`struct __locale_struct`** (232 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `__locales` | `struct __locale_data *[13]` |
| 104 | `__ctype_b` | `const short unsigned int *` |
| 112 | `__ctype_tolower` | `const int *` |
| 120 | `__ctype_toupper` | `const int *` |
| 128 | `__names` | `const char *[13]` |

**`regex_t`** (64 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `buffer` | `struct re_dfa_t *` |
| 8 | `allocated` | `__re_long_size_t` |
| 16 | `used` | `__re_long_size_t` |
| 24 | `syntax` | `reg_syntax_t` |
| 32 | `fastmap` | `char *` |
| 40 | `translate` | `unsigned char *` |
| 48 | `re_nsub` | `size_t` |
| 56 | `can_be_null` | `unsigned int` (bits 1@0) |
| 56 | `regs_allocated` | `unsigned int` (bits 2@0) |
| 56 | `fastmap_accurate` | `unsigned int` (bits 1@0) |
| 56 | `no_sub` | `unsigned int` (bits 1@0) |
| 56 | `not_bol` | `unsigned int` (bits 1@0) |
| 56 | `not_eol` | `unsigned int` (bits 1@0) |
| 56 | `newline_anchor` | `unsigned int` (bits 1@0) |

**`glob_t`** (72 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `gl_pathc` | `size_t` |
| 8 | `gl_pathv` | `char * *` |
| 16 | `gl_offs` | `size_t` |
| 24 | `gl_flags` | `int` |
| 32 | `gl_closedir` | `fn(...) *` |
| 40 | `gl_readdir` | `fn(...) *` |
| 48 | `gl_opendir` | `fn(...) *` |
| 56 | `gl_lstat` | `fn(...) *` |
| 64 | `gl_stat` | `fn(...) *` |

**`cookie_io_functions_t`** (32 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `read` | `cookie_read_function_t *` |
| 8 | `write` | `cookie_write_function_t *` |
| 16 | `seek` | `cookie_seek_function_t *` |
| 24 | `close` | `cookie_close_function_t *` |

**`posix_spawnattr_t`** (336 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `__flags` | `short int` |
| 4 | `__pgrp` | `pid_t` |
| 8 | `__sd` | `sigset_t` |
| 8 | `__sd.__val` | `long unsigned int[16]` |
| 136 | `__ss` | `sigset_t` |
| 136 | `__ss.__val` | `long unsigned int[16]` |
| 264 | `__sp` | `struct sched_param` |
| 264 | `__sp.sched_priority` | `int` |
| 268 | `__policy` | `int` |
| 272 | `__cgroup` | `int` |
| 276 | `__pad` | `int[15]` |

**`posix_spawn_file_actions_t`** (80 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `__allocated` | `int` |
| 4 | `__used` | `int` |
| 8 | `__actions` | `struct __spawn_action *` |
| 16 | `__pad` | `int[16]` |

**`struct option`** (32 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `name` | `const char *` |
| 8 | `has_arg` | `int` |
| 16 | `flag` | `int *` |
| 24 | `val` | `int` |

**`struct epoll_event`** (12 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `events` | `uint32_t` |
| 4 | `data` | `epoll_data_t` |
| 4 | `data.ptr` | `void *` |
| 4 | `data.fd` | `int` |
| 4 | `data.u32` | `uint32_t` |
| 4 | `data.u64` | `uint64_t` |

**`struct dl_phdr_info`** (64 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `dlpi_addr` | `Elf64_Addr` |
| 8 | `dlpi_name` | `const char *` |
| 16 | `dlpi_phdr` | `const Elf64_Phdr *` |
| 24 | `dlpi_phnum` | `Elf64_Half` |
| 32 | `dlpi_adds` | `long long unsigned int` |
| 40 | `dlpi_subs` | `long long unsigned int` |
| 48 | `dlpi_tls_modid` | `size_t` |
| 56 | `dlpi_tls_data` | `void *` |

**`struct r_debug`** (40 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `r_version` | `int` |
| 8 | `r_map` | `struct link_map *` |
| 16 | `r_brk` | `Elf64_Addr` |
| 24 | `r_state` | `enum <anon>` |
| 32 | `r_ldbase` | `Elf64_Addr` |

**`struct link_map`** (40 bytes)

| Offset | Member | Type |
|---|---|---|
| 0 | `l_addr` | `Elf64_Addr` |
| 8 | `l_name` | `char *` |
| 16 | `l_ld` | `Elf64_Dyn *` |
| 24 | `l_next` | `struct link_map *` |
| 32 | `l_prev` | `struct link_map *` |

### C.8 Static initializers

Programs embed these bit patterns in their data. An object holding one of these patterns must behave as
initialised by the corresponding init function. Bytes are listed in memory order.

| Type | Initializer | Size | All zero | Bytes (hex) |
|---|---|---|---|---|
| `pthread_mutex_t` | `PTHREAD_MUTEX_INITIALIZER` | 40 | all-zero | `00000000000000000000000000000000000000000000000000000000000000000000000000000000` |
| `pthread_mutex_t` | `PTHREAD_RECURSIVE_MUTEX_INITIALIZER_NP` | 40 | nonzero | `00000000000000000000000000000000010000000000000000000000000000000000000000000000` |
| `pthread_mutex_t` | `PTHREAD_ERRORCHECK_MUTEX_INITIALIZER_NP` | 40 | nonzero | `00000000000000000000000000000000020000000000000000000000000000000000000000000000` |
| `pthread_mutex_t` | `PTHREAD_ADAPTIVE_MUTEX_INITIALIZER_NP` | 40 | nonzero | `00000000000000000000000000000000030000000000000000000000000000000000000000000000` |
| `pthread_rwlock_t` | `PTHREAD_RWLOCK_INITIALIZER` | 56 | all-zero | `0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000` |
| `pthread_rwlock_t` | `PTHREAD_RWLOCK_WRITER_NONRECURSIVE_INITIALIZER_NP` | 56 | nonzero | `0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000200000000000000` |
| `pthread_cond_t` | `PTHREAD_COND_INITIALIZER` | 48 | all-zero | `000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000` |
| `pthread_once_t` | `PTHREAD_ONCE_INIT` | 4 | all-zero | `00000000` |
| `once_flag` | `ONCE_FLAG_INIT` | 4 | all-zero | `00000000` |
| `mbstate_t` | `{0}` | 8 | all-zero | `0000000000000000` |

### C.9 Header-inline protocols

Public headers expand some interfaces into inline code or macros that compiled programs contain. A
replacement library must support these protocols exactly, because the code that relies on them is
inside the caller's binary.

| Interface | What compiled callers do | Obligation on the library |
|---|---|---|
| `getc_unlocked`, `fgetc_unlocked`, `getchar_unlocked` (`bits/types/struct_FILE.h`, `bits/stdio.h`) | `fp->_IO_read_ptr >= fp->_IO_read_end ? __uflow(fp) : *(unsigned char *)fp->_IO_read_ptr++` | Keep the read window `[_IO_read_ptr, _IO_read_end)` of every `FILE` valid at the offsets in C.7. `__uflow(fp)` refills and returns the next byte (advancing the window) or `EOF`. |
| `putc_unlocked`, `fputc_unlocked`, `putchar_unlocked` | `fp->_IO_write_ptr >= fp->_IO_write_end ? __overflow(fp, (unsigned char)c) : (unsigned char)(*fp->_IO_write_ptr++ = c)` | Keep the write window valid. A stream that must see every byte (unbuffered, line-buffered, wide, closed) sets `_IO_write_ptr == _IO_write_end` so that every byte reaches `__overflow`. |
| `feof_unlocked`, `ferror_unlocked` | test `fp->_flags & 0x0010` (`_IO_EOF_SEEN`) and `& 0x0020` (`_IO_ERR_SEEN`) | Maintain those two bits in `_flags`. `_IO_USER_LOCK` (`0x8000`) is also public. Other `_flags` bits are unconstrained. |
| `stdin`, `stdout`, `stderr` | read the exported `FILE *` variables | Export them as data (C.5) and read them on every implicit use (they are assignable). |
| `is*` classification (`ctype.h`, `__isctype`) | `(*__ctype_b_loc())[(int)c] & mask`, with masks `_ISupper` … `_ISalnum` = `_ISbit(0..11)`, where on little-endian `_ISbit(b) = b < 8 ? (1 << b) << 8 : (1 << b) >> 8` | `__ctype_b_loc()` returns a per-thread pointer to a pointer to an array of `unsigned short`, indexable from -128 to 255, for the thread's current locale. |
| `tolower`/`toupper` inline forms | `(*__ctype_tolower_loc())[c]`, `(*__ctype_toupper_loc())[c]` (`int32_t` tables, index -128..255) | As above, for the case tables. |
| `errno` | `(*__errno_location())` | Return the calling thread's errno cell, stable for the thread's lifetime. |
| `assert` | `__assert_fail(#expr, __FILE__, __LINE__, __func__)` | Signature per `prototypes.tsv`, no return. |
| `setjmp(env)` | `_setjmp(env)` (no mask save) | `setjmp` is a macro. `_setjmp` and `__sigsetjmp(env, savemask)` are the called entry points, with `jmp_buf` per C.7. |
| `sigsetjmp(env, save)` | `__sigsetjmp(env, save)` | As above. |
| `FD_SET`/`FD_CLR`/`FD_ISSET`/`FD_ZERO` | inline bit operations on `fd_set.fds_bits` (`long` words, bit `d % 64` of word `d / 64`); fortified builds call `__fdelt_chk(d)` | `fd_set` is 128 bytes (1024 bits). `__fdelt_chk` returns `d / 64` or aborts for `d` outside 0..1023. |
| `MB_CUR_MAX` | `__ctype_get_mb_cur_max()` | Per current locale. |
| `SIGRTMIN`/`SIGRTMAX` | `__libc_current_sigrtmin()` / `__libc_current_sigrtmax()` | Runtime values. |
| `PTHREAD_STACK_MIN`, `MINSIGSTKSZ`, `SIGSTKSZ` (with `_GNU_SOURCE` / `_DYNAMIC_STACK_SIZE_SOURCE`) | `sysconf(_SC_THREAD_STACK_MIN)`, `sysconf(_SC_MINSIGSTKSZ)`, `sysconf(_SC_SIGSTKSZ)` | Answer those `sysconf` names. |
| `pthread_cleanup_push`/`pop` (C, non-exception build) | allocate `__pthread_unwind_buf_t` on the caller's stack, call `__sigsetjmp` on its `__cancel_jmp_buf`, then `__pthread_register_cancel(&buf)`; on pop `__pthread_unregister_cancel(&buf)`; on cancellation the library `__pthread_unwind_next(&buf)`s through the chain | Layout of `__pthread_unwind_buf_t` per C.7. Cancellation must `longjmp` into each registered buffer in LIFO order. The `_defer`/`_restore` variants also switch the cancellation type. |
| `CMSG_NXTHDR` | `__cmsg_nxthdr(mhdr, cmsg)` | Signature per `prototypes.tsv`. |
| Fortified calls (`_FORTIFY_SOURCE`) | `__name_chk(args…, destlen)`: the compiler-known destination size is appended (for `read`: `__read_chk(fd, buf, nbytes, buflen)`); printf-family `_chk` forms insert a `flag` argument after the stream or buffer argument | Signatures per `prototypes.tsv`. Overflow calls `__chk_fail`. |
| Language-mode redirects | headers bind `scanf`-family calls to `__isoc99_*` (C99+) or `__isoc23_*` (C2X/C23) names, and `strtol`-family calls to `__isoc23_*` in C23 mode, through asm labels | Export all three name sets with their distinct contracts (§12.7, §8.1). |
| Math classification macros | `fpclassify`/`isinf`/`isnan`/`signbit`/`issignaling`/`iseqsig` may compile to builtins or call `__fpclassify*`, `__isinf*`, `__isnan*`, `__signbit*`, `__issignaling*`, `__iseqsig*` | Export those helpers (Appendix B.10). |

### C.10 Prototypes and constants

`prototypes.tsv` gives the declaration of 3778 functions from the public headers. Names that Appendix B
marks `†` have no public declaration; their calling convention is fixed by their callers (compiler-emitted
calls such as `__stack_chk_fail`, `__tls_get_addr`, `__cxa_*`, or header macros) and is recorded in C.9.
`constants.tsv` gives 6687 macro values. Constants that are kernel ABI (open flags, error numbers, signal
numbers, ioctl codes) come from the kernel UAPI; they appear here as the headers expose them, and must agree.

### C.11 Remaining gaps

* **libm accuracy bounds.** The headers do not record ULP bounds. Accuracy targets are set by the implementation
  plan's own tests (decision D-10.3).
* **System file formats** named by this spec but not described (locale archive, iconv modules, nscd wire
  protocol, message catalogs, `ld.so.cache`): format inspection is approved (owner decision on formats) and is
  performed in the phase that needs each format. Findings are added to this spec as Appendix D.
<!-- appendix-c:end -->

## Appendix D. System data formats

Formats of installed system files that the contracts depend on but do not describe (C.11).
They were established by format inspection (owner decision on formats; implementation-plan.md
D-7.2): reading files that the reference system's tools produce, checking each hypothesis
against the reference library's observable behaviour, and recording only what an
implementation needs to read the files. All integers are little-endian (x86_64). Further
formats are added in the phase that needs them.

### D.1 Compiled locales (phase 7)

**Sources of a named locale.** A locale is installed either as a directory holding one file
per category, or inside the locale archive. The contents of a category are the same bytes in
both forms.

* **Directory form:** `<dir>/<name>/LC_CTYPE`, `LC_NUMERIC`, `LC_TIME`, `LC_COLLATE`,
  `LC_MONETARY`, `LC_PAPER`, `LC_NAME`, `LC_ADDRESS`, `LC_TELEPHONE`, `LC_MEASUREMENT`,
  `LC_IDENTIFICATION`, and `LC_MESSAGES/SYS_LC_MESSAGES`. `<dir>` is each directory of
  `LOCPATH` (colon-separated, ignored in secure mode), or `/usr/lib/locale` when `LOCPATH` is
  unset.
* **Archive form:** `/usr/lib/locale/locale-archive` (D.1.6). The reference system used for
  this inspection has no archive, so the precedence between archive and directories could not
  be observed; Liella consults the archive only when `LOCPATH` is unset, and then before
  `/usr/lib/locale` directories (implementation-plan.md D-7.7).

**Name resolution** (observed). A name has the form
`language[_territory][.codeset][@modifier]`.

* `"C"` and `"POSIX"` (reported as `"C"`), and `"C.UTF-8"`/`"C.utf8"` (reported as given), are
  built in.
* The *normalized* codeset is the codeset with letters lowercased and every character that is
  not a letter or digit removed (`UTF-8` → `utf8`, `ISO-8859-1` → `iso88591`; `8859-1` →
  `88591`, which does not find `iso88591`).
* Candidates are generated from the most specific to the least, and components are only
  ever dropped, never added; the language is always kept. With the present components
  weighted modifier 4, territory 2, codeset 1, the candidates are tried in descending order
  of the kept components' total: the codeset is dropped first, then the territory, and the
  modifier last. A codeset is tried in its literal form before its normalized form. For
  example `aa_AA.UTF-8` finds a directory `aa_AA.utf8`, `ee_EE.UTF-8` finds `ee`, `bb_BB@x`
  finds `bb_BB`; `ii_II.UTF-8@m` prefers `ii_II@m` to `ii_II.utf8`, `jj_JJ.UTF-8` prefers
  `jj_JJ` to `jj.utf8`, `ll_LL@m` prefers `ll@m` to `ll_LL`, and `kk_KK.UTF-8` prefers
  `kk_KK.UTF-8` to `kk_KK.utf8`. `aa_AA` does not find `aa_AA.utf8`, and `cc_CC.utf8` does not
  find `cc_CC.UTF-8`.
* When the name carries a codeset, the found locale's own codeset (`LC_CTYPE` item
  `CODESET`), normalized, must equal it; otherwise the candidate is rejected.
* The name `setlocale` reports is the name as requested, even when a less specific candidate
  supplied the data.
* Names containing `/` are rejected.
* **Aliases.** A name listed in `/usr/share/locale/locale.alias` (lines `alias value`, `#`
  comments; matched without regard to case) is replaced by its value only when the name itself
  is not found (no candidate of it in the archive or the directories); the value's candidates
  are then searched the same way, and the reported name stays as requested (`french` →
  `fr_FR.ISO-8859-1`, reported `french`). Observed on the reference (strace, 2026-10-10):
  `en_US.UTF-8`, which the archive holds, opens only the archive and never `locale.alias`;
  `french` opens `locale.alias` (implementation-plan.md D-7.7, owner ruling 2026-10-11).

**Composite names.** When the categories differ, `setlocale(LC_ALL, NULL)` reports
`LC_CTYPE=…;LC_NUMERIC=…;LC_TIME=…;LC_COLLATE=…;LC_MONETARY=…;LC_MESSAGES=…;LC_PAPER=…;LC_NAME=…;LC_ADDRESS=…;LC_TELEPHONE=…;LC_MEASUREMENT=…;LC_IDENTIFICATION=…`
(this order), and that string is accepted back. A composite string that does not name all
twelve categories is rejected. When all categories agree, the single name is reported.

#### D.1.1 Category files

```
u32 magic                  per category (LC_CTYPE 0x20090720, LC_COLLATE 0x20051017, ...)
u32 n                      number of items
u32 offset[n]              byte offset of each item from the start of the file
...                        item data; item i ends where item i+1 begins (the last at end of file)
```

Item *i* of category *c* is the `nl_item` `(c << 16) | i` of `<langinfo.h>` (category numbers:
`LC_CTYPE` 0, `LC_NUMERIC` 1, `LC_TIME` 2, `LC_COLLATE` 3, `LC_MONETARY` 4, `LC_MESSAGES` 5,
`LC_PAPER` 7, `LC_NAME` 8, `LC_ADDRESS` 9, `LC_TELEPHONE` 10, `LC_MEASUREMENT` 11,
`LC_IDENTIFICATION` 12). An item's data is what `nl_langinfo` returns a pointer to: a
NUL-terminated string, a wide string (`wchar_t`, NUL-terminated), a `u32` word, or a byte,
according to the item's name. A file may hold more items than `<langinfo.h>` names (see
`LC_CTYPE` below).

#### D.1.2 Three-level tables

Wide-character tables (`LC_CTYPE` classes, maps and widths; `LC_COLLATE` `TABLEWC` and
`COLLSEQWC`) share one structure:

```
u32 shift1, bound, shift2, mask2, mask3
u32 level1[bound]          byte offsets (from the table start) of level-2 blocks, 0 = absent
level-2 blocks: u32[mask2 + 1]   byte offsets of level-3 blocks, 0 = absent
level-3 blocks
```

For a code point `wc`: `i1 = wc >> shift1` (absent if `i1 >= bound`); the level-2 entry is
`(wc >> shift2) & mask2`. In a **bit table** (classes) the level-3 block is `u32[mask3 + 1]`
words and the bit is `(word[(wc >> 5) & mask3] >> (wc & 31)) & 1`. In a **value table** the
level-3 block is indexed by `wc & mask3`: `i32` entries for maps and collation, bytes for
widths. An absent block means "no entry": not a member, map to itself, width −1 (as does
byte 0xff), collation entry 0.

#### D.1.3 `LC_CTYPE`

| Item | Data |
|---|---|
| `_NL_CTYPE_CLASS` | `u16[384]`: the class bits of bytes −128…255, exactly the array `__ctype_b_loc()` points into (at element 128) |
| `_NL_CTYPE_TOUPPER`, `_NL_CTYPE_TOLOWER` | `i32[384]`: the byte case maps for −128…255, exactly `__ctype_toupper_loc()`/`__ctype_tolower_loc()` (at element 128) |
| `_NL_CTYPE_CLASS32`, `_NL_CTYPE_TOUPPER32`, `_NL_CTYPE_TOLOWER32` | `u32[256]`: the same for bytes 0…255 in 32-bit form (`__ctype32_*`) |
| `_NL_CTYPE_CLASS_NAMES`, `_NL_CTYPE_MAP_NAMES` | NUL-separated names, ended by an empty name: the classes (`upper`, `lower`, `alpha`, `digit`, `xdigit`, `space`, `print`, `graph`, `blank`, `cntrl`, `punct`, `alnum`, then locale-defined such as `combining`) and maps (`toupper`, `tolower`, then locale-defined such as `totitle`) |
| `_NL_CTYPE_CLASS_OFFSET`, `_NL_CTYPE_MAP_OFFSET` | `u32`: the item index of the first class table and of the first map table. Class *k* (in name order) is a bit table at item `CLASS_OFFSET + k`; map *k* is a value table of `i32` deltas at item `MAP_OFFSET + k` (`towupper(wc) = wc + delta`) |
| `_NL_CTYPE_WIDTH` | a value table of bytes: the `wcwidth` of each code point; 0xff or absent = −1 |
| `_NL_CTYPE_MB_CUR_MAX` | `u32` |
| `CODESET` | the charset name |
| `_NL_CTYPE_INDIGITS*`, `_NL_CTYPE_OUTDIGIT*` | the digit strings, multibyte and wide |
| `_NL_CTYPE_TRANSLIT_TAB_SIZE` | `u32` *n* |
| `_NL_CTYPE_TRANSLIT_FROM_IDX`, `_TO_IDX` | `u32[n]`: word indices into the next tables |
| `_NL_CTYPE_TRANSLIT_FROM_TBL` | wide strings, NUL-terminated; entries are sorted (binary search) |
| `_NL_CTYPE_TRANSLIT_TO_TBL` | per entry, a list of alternative wide strings, each NUL-terminated, the list ended by an empty string; the first alternative may itself be empty (a combining mark that transliterates to nothing: `de_DE`'s U+0301 entry is two NULs) |
| `_NL_CTYPE_TRANSLIT_DEFAULT_MISSING` | the wide replacement used when no alternative converts (`?` in the reference locales) |

The class tables, the map tables and the width table were checked against the reference
library's `iswctype`, `towupper`, `towlower`, `towctrans(…, "totitle")` and `wcwidth` for all
1,114,112 code points (`en_US.UTF-8`), and the byte tables against `__ctype_b_loc()` and its
siblings in `en_US.ISO-8859-1`, `en_US.UTF-8` and `tr_TR.UTF-8`.

#### D.1.4 `LC_COLLATE`

| Item | Data |
|---|---|
| `_NL_COLLATE_NRULES` | `u32` *r*, the number of levels; 0 means code-point order (`strcoll` = `strcmp`; `C`, `C.UTF-8`) |
| `_NL_COLLATE_RULESETS` | bytes, *r* per ruleset: the rule of each level, bits FORWARD 1, BACKWARD 2, POSITION 4 |
| `_NL_COLLATE_TABLEMB` | `i32[256]`, indexed by a string's byte |
| `_NL_COLLATE_WEIGHTMB` | element weights: per level, a length byte then that many weight bytes |
| `_NL_COLLATE_EXTRAMB` | entry lists for bytes that begin longer elements |
| `_NL_COLLATE_INDIRECTMB` | `i32` entries reached through range entries |
| `_NL_COLLATE_TABLEWC` | a three-level value table of `i32`, by code point |
| `_NL_COLLATE_WEIGHTWC` | as `WEIGHTMB` in `u32` units: per level a count word, then the weights |
| `_NL_COLLATE_EXTRAWC`, `_NL_COLLATE_INDIRECTWC` | as the MB forms, in `i32` units |
| `_NL_COLLATE_COLLSEQMB`, `_NL_COLLATE_COLLSEQWC` | collation sequence values, which order the end points of bracket ranges in patterns: `COLLSEQMB` is `u8[256]` by byte; `COLLSEQWC` is a three-level value table (D.1.2) of `u32` by code point. In `en_US.UTF-8` every lowercase letter precedes every uppercase one (`a` 9488 … `z` 10856, `A` 10958 …) and `é` is 9747, which gives the observed `[a-z]`, `[A-z]` and `[b-é]` (phase 12 inspection) |
| `_NL_COLLATE_SYMB_*` | collating-symbol names (`[[.ch.]]`, phase 13) |

**Element lookup (bytes).** At position *p* of the string, `t = TABLEMB[s[p]]`.

* `t ≥ 0`: a one-byte element whose **entry** is `t`.
* `t < 0`: scan the list at byte offset `−t` of `EXTRAMB`. Each entry is `i32 i`, `u8 len`,
  then:
  * `i ≥ 0`: `u8 seq[len]`. It matches if the bytes after `s[p]` begin with `seq`; the element
    is `1 + len` bytes and its entry is `i`.
  * `i < 0`: `u8 lo[len]`, `u8 hi[len]`. It matches if the next `len` bytes, compared as a
    string, lie in `[lo, hi]`; the element is `1 + len` bytes and its entry is
    `INDIRECTMB[−i + k]`, where `k` is the offset of those bytes within the range
    (mixed radix: `k = k·(hi[j] − lo[j] + 1) + (b[j] − lo[j])` over `j`).

  The next entry begins at the following 4-byte boundary. The first matching entry wins (the
  tables list longer contractions first), and every list ends with an entry that always
  matches (`len` 0). Invalid and truncated byte sequences reach that entry too.
* An **entry** is `ruleset << 24 | offset`: the element's weights start at `offset` in
  `WEIGHTMB`, and its level rules are `RULESETS[ruleset · r + level]`.

**Element lookup (wide).** The same, with `TABLEWC` looked up by code point and `EXTRAWC`
entries of `i32 i`, `i32 len`, then `len` code points (or `lo[len]`, `hi[len]`), with no
padding; offsets are in `i32` units.

**Keys** (`strxfrm`, `wcsxfrm`; `strcoll`/`wcscoll` order strings as their keys compare). An
empty string has an empty key. Otherwise, for each level in turn:

1. The elements are taken in string order, except that each maximal run of consecutive
   elements whose rule at this level is BACKWARD is reversed.
2. Each element contributes its weights at this level. If its rule has POSITION, an element
   with weights is preceded by the unit `1 + s`, where `s` is the number of elements without
   weights at this level skipped since the last element with weights; elements without
   weights contribute nothing.
3. The level ends with the separator unit 1, unless it is a POSITION level that contributed
   nothing.

The key's final unit is then removed if it is a separator. Units are bytes for `strxfrm` and
`wchar_t` for `wcsxfrm`.

Checked against the reference library's `strxfrm` on 32,000 random strings in 16 locales
(`en_US`, `de_DE`, `cs_CZ`, `da_DK`, `fr_CA`, `hu_HU`, `es_ES`, `th_TH`, `vi_VN`, `tr_TR`,
`ja_JP` in UTF-8; `en_US`, `fr_FR`, `sv_SE` in ISO-8859-1; `ja_JP.EUC-JP`; `ko_KR.EUC-KR`),
including contractions, backward accents, ignorable characters and invalid bytes, and its
`wcsxfrm` on 18,000 strings in 9 locales: all keys identical. The reference model is
`spec/tools/collate_proto.py`.

#### D.1.5 Other categories

Their items are strings, string arrays, wide strings, bytes or `u32` words. For a **word item**
`nl_langinfo` returns the word's value converted to a pointer, not its address; for every other
item it returns the address of the item's data. The word items (observed: the reference library
returns each one's file value) are `_NL_CTYPE_MB_CUR_MAX`, `_NL_CTYPE_CLASS_OFFSET`,
`_NL_CTYPE_MAP_OFFSET`, `_NL_CTYPE_INDIGITS_MB_LEN`, `_NL_CTYPE_INDIGITS_WC_LEN`,
`_NL_CTYPE_OUTDIGIT0_WC`…`_NL_CTYPE_OUTDIGIT9_WC`, `_NL_CTYPE_TRANSLIT_TAB_SIZE`,
`_NL_CTYPE_TRANSLIT_DEFAULT_MISSING_LEN`, `_NL_CTYPE_TRANSLIT_IGNORE_LEN`,
`_NL_CTYPE_MAP_TO_NONASCII`, `_NL_CTYPE_NONASCII_CASE`, `_NL_NUMERIC_DECIMAL_POINT_WC`,
`_NL_NUMERIC_THOUSANDS_SEP_WC`, `_NL_TIME_ERA_NUM_ENTRIES`, `_NL_TIME_WEEK_1STDAY`,
`_NL_COLLATE_NRULES`, `_NL_COLLATE_SYMB_HASH_SIZEMB`, `_NL_MONETARY_UNO_VALID_FROM`,
`_NL_MONETARY_UNO_VALID_TO`, `_NL_MONETARY_DUO_VALID_FROM`, `_NL_MONETARY_DUO_VALID_TO`,
`_NL_MONETARY_DECIMAL_POINT_WC`, `_NL_MONETARY_THOUSANDS_SEP_WC`, `_NL_PAPER_HEIGHT`,
`_NL_PAPER_WIDTH` and `_NL_ADDRESS_COUNTRY_NUM` (35). In the reference library's built-in
locales only the low 32 bits of such a returned value are meaningful.

The built-in locales' item values are recorded in `spec/abi/langinfo.tsv`
(`spec/tools/langinfo_probe.py`): "C" and "C.UTF-8" share every value except `MB_CUR_MAX` (1
and 6), the codeset items (`ANSI_X3.4-1968` and `UTF-8`) and the transliteration table. `localeconv` reads its fields from the
`LC_NUMERIC` and `LC_MONETARY` items of the same names (`__DECIMAL_POINT`, `__GROUPING`,
`__INT_CURR_SYMBOL`, ..., `__INT_FRAC_DIGITS` as a byte).

#### D.1.6 The locale archive

```
u32 magic                  0xde020109
u32 serial
u32 namehash_offset, namehash_used, namehash_size
u32 string_offset,   string_used,   string_size
u32 locrectab_offset, locrectab_used, locrectab_size
u32 sumhash_offset,  sumhash_used,  sumhash_size
```

* **Name table:** `namehash_size` entries of `u32 hash, u32 name_offset, u32 locrec_offset`;
  an entry with `name_offset` 0 is unused. `name_offset` is the file offset of a
  NUL-terminated name, stored with its codeset normalized (`de_DE.utf8`). A reader may scan
  the entries instead of hashing.
* **Locale record** at `locrec_offset`: `u32 refs`, then 13 × (`u32 offset, u32 len`), one per
  category number 0…12. Slot 6 (`LC_ALL`) covers the whole record and is not a category.
  Each other slot locates that category's file image, byte-identical to the directory form
  (checked for every category of three archived locales).
* The sum-hash table (content checksums, used when adding locales) is not needed for reading.

### D.2 Message catalogs (phase 13)

Moved here on 2026-10-04 from `spec/behaviour/catalogs-format.md` (format inspection under plan §1.2; the behaviour document `spec/behaviour/catalogs.md` and its record check it). **Lookup in Liella (decision, 2026-10-04):** a `.mo` file is searched in its sorted original-string table by binary search; the hash table is not used, so no hash function is needed, every entry of a well-formed catalog is found, and a damaged hash table cannot make a lookup loop.

**How it was established** (plan §1.2, "Installed system data files": format inspection of files
that exist on the reference system or that its own tools generate; structure only). On the VM
(Ubuntu 24.04, glibc 2.39-0ubuntu8.9), small catalogs were generated with `msgfmt` from GNU
gettext-tools 0.21 (package `gettext` 0.21-14ubuntu2, installed with apt for this work; options
`--no-hash` and `--endianness=big` as well as the defaults) and with `gencat` from `libc-bin`
2.39-0ubuntu8.9, and dumped with `xxd`. The fixtures are the ones `catalogs_run.sh` builds
(`a.po`, `big.po`, `sd.po`; `a.msg`, `b.msg`, `c.msg`, `main.msg`). Each structural hypothesis was
then tested black-box: the probe writes catalogs of its own from the hypothesis (a `.mo` writer
following the GNU gettext manual, a `.cat` writer following D.2.2 below) and from deliberately
altered copies, and records what `gettext`, `ngettext`, `catopen` and `catgets` return. The
probe's `.cat` writer reproduces `gencat`'s output byte for byte for three catalogs (record lines
945-948). No code of any implementation was read. Integers are little-endian on x86_64 unless the
file says otherwise (`.mo` files carry their byte order in the magic number).

Documentation compared: the GNU gettext manual, chapter "The Format of GNU MO Files" (§11.3 in the
current online edition); POSIX.1-2017 `catopen`, `catgets`, `catclose`, XBD 8.2 (`NLSPATH`) and the
`gencat` utility (input format only: POSIX does not specify the catalog file).

#### D.2.1 GNU message objects (`.mo`) — §7.7, D-7.5

```
off  u32
  0  magic             0x950412de in the file's byte order (a file read with the other order
                       shows 0xde120495)
  4  revision          major << 16 | minor
  8  N                 number of strings (pairs)
 12  O                 offset of the original-string table: N x (u32 length, u32 offset)
 16  T                 offset of the translation table:     N x (u32 length, u32 offset)
 20  S                 number of hash-table entries (may be 0: no table)
 24  H                 offset of the hash table: S x u32
     -- revision with minor 1 (system-dependent strings; not in the manual's chapter):
 28  number of system-dependent segments
 32  offset of the segment table:  (u32 length, u32 offset) per segment, a NUL-terminated name
 36  number of system-dependent strings
 40  offset of the table of original system-dependent strings:   one u32 offset per string
 44  offset of the table of translated system-dependent strings: one u32 offset per string
```

* **Byte order.** Every word of the file is in the byte order of its magic number. `msgfmt`
  writes the build machine's order by default and big-endian with `--endianness=big` (`a-be.mo`
  starts `95 04 12 de`, `a.mo` starts `de 12 04 95`). The reference library reads both, and the
  probe's own big-endian file (record: `-- msgfmt a-be.mo`, `big-endian file (probe writer)`).
  A file whose magic is big-endian but whose other words are little-endian kills the process
  with `SIGSEGV` (an unstable case, left out of the record).
* **Revision.** `msgfmt` writes 0 for ordinary catalogs and 1 (major 0, minor 1) when a message
  uses a system-dependent format macro (`sd.mo`). The reference library uses a file of revision
  0 or 0x10000 (major 1, minor 0), and ignores (every lookup untranslated) a probe-written file of
  revision 1, 2, 0x20000 or 0xffffffff whose header has no minor-1 fields (record: `revision ...`
  lines). The manual says the major revision is 0 or 1 and the minor 0 or 1, that an unknown
  major revision must stop the reader, and that an unknown minor revision still allows reading
  part of the file. The record agrees for majors 0, 1 and 2; minor 2 and the field-less minor 1
  are rejected, which the manual neither requires nor forbids.
* **Tables.** A string is located by its descriptor (`length`, `offset` from the start of the
  file); the bytes at `offset + length` are a NUL that the length does not count. The original
  strings are sorted in increasing byte order (`msgfmt`: `""`, `file\0files`, `hello`,
  `menu\x04open` in `a.mo`), so the header entry is index 0. Translation *i* belongs to original
  *i*. Strings follow the hash table, originals first: `a.mo` has N 4, O 0x1c, T 0x3c, S 5,
  H 0x5c, strings from 0x70. `msgfmt --no-hash` gives S 0 and H at the end of the tables.
* **Context and plural keys** (as the manual says): a key with a context is `context \x04 msgid`;
  a plural entry's key is `msgid \0 msgid_plural` with the length covering both, and its
  translation is the forms `form0 \0 form1 \0 ...` with the length covering all of them.
  Only the singular part takes part in lookup: `ngettext("file", "other", 2)` finds the entry
  keyed `file\0files` (record: `ngettext(file, other, 2)`).
* **Hash table.** `S` u32 entries; an entry is 0 when empty, otherwise the index of a string plus
  1 (`a.mo`: `1 2 3 4 0` for N 4, S 5; `big.mo`: S 673 for N 501). The manual says conflicts are
  resolved by double hashing and that the hashing algorithm "is not documented here"; it was not
  derived. **What the reference library does with it** (record: `hash table altered`): when
  `S` is not 0 the table decides alone. A table of zeros makes all 500 lookups fail, with no
  fall-back to a search; a table whose every entry names a valid but wrong index never returns
  (the child is killed by its alarm); entries beyond N read outside the tables (unstable). When
  `S` is 0, the lookup is a search of the sorted originals: in a probe-written table whose keys
  are not sorted, only some keys are found (`hello` yes, `zz` and `file` no), which is what a
  binary search over the declared order gives. A table offset H beyond the file with S 5 did not
  matter in the record (`hash size 5, offset beyond the file`: all found); that it was not read
  is not established.
* **Header entry** (translation of `""`): lines `Name: value\n`. The reference library uses two
  pieces of it, found anywhere in the text: the codeset after `charset=` (also without a
  `Content-Type:` line, on a later line, with the codeset in lowercase (`utf-8`), and followed by
  a blank and more text:
  `charset=UTF-8 extra` works) and the plural rule after `Plural-Forms:` (also found when the
  name is lowercase, when it follows `X-Note: ` on the same line, and with `plural=` before
  `nplurals=`). Without `charset=` (no header entry, or a header without it) the translations
  are returned as stored, unconverted; with the placeholder `charset=CHARSET` or an unknown
  name like `NOPE`, every lookup in the catalog is untranslated (record: the `charset=...` and
  `no header entry` cases of the conversion section). The plural rule is described in
  catalogs.md.
* **Lengths are not used to bound the singular translation:** a translation descriptor with
  length 0 or 3 still returns `hallo` (the string up to its NUL), and an original's length of 4
  still matches `hello` and not `hell`. For a plural entry the length does bound the forms: a
  length of 5 over `Datei\0Dateien` makes `n=2` return `Datei`. A length beyond the end of the
  file is accepted. Offsets beyond the file are not checked (unstable cases).
* **Duplicate keys**: the first in table order wins (`hello` -> `first`), in a file without a hash
  table.
* **System-dependent strings** (revision 1; `sd.mo` from `msgid "count %<PRIu64> x"` with
  `#, c-format`): the ordinary tables hold only the header (N 1). The segment table names
  `PRIu64` (length 7 with its NUL). Each original/translated system-dependent string is a u32
  offset of a descriptor: `u32 offset` of a static text, then pairs `u32 segment length,
  u32 segment reference` ending with reference 0xffffffff. The static text is the concatenation
  of the segments (`count % x` = `count %` (7) + ` x` and its NUL (3)); a reference names an
  entry of the segment table whose expansion follows that segment. The reference library
  matches the msgid `count %lu x` (the expansion of `<PRIu64>` on x86_64 is `lu`) and returns
  `zahl %lu y`; `count %llu x`, `count %ld x`, `count %<PRIu64> x` and `count % x` are not found.
  The expansion of other macros (`PRId32`, `PRIxMAX`, `I` flags, ...) was not probed.
* **Files that are not catalogs** (record: `damaged .mo files`): a file shorter than the header,
  a wrong magic number, `N` 0, a directory, an unreadable file and a dangling link give
  untranslated results; `N` 2 hides the entries after index 1; a cut file serves what is left
  (bytes beyond the end of the file read as zeros within the last page: `half the file` gives
  `hello` -> `""`, and a translation that loses its NUL at the end of the file reads `ZZX`).

#### D.2.2 X/Open catalogs (`.cat`, written by `gencat`) — §7.8, D-7.6

```
off        u32
  0        magic        0x960408de (little-endian on x86_64)
  4        plane_size   P
  8        plane_depth  D
 12        native table: P*D slots of 3 x u32 (set + 1, msg, string offset); a slot with
           first word 0 is empty
 12+12PD   the same table in big-endian order
 12+24PD   string pool: NUL-terminated strings; string offset counts from here
```

* **Slots.** Message `msg` of set `set` is stored in a slot whose first word is `set + 1`
  (`b.cat`: set 10 is stored as 11; the default set `NL_SETD` 1 as 2). Its slot index in plane
  *p* (0-based) is `p * P + ((set + 1) * msg) mod P`; `gencat` uses the first plane where that
  slot is free (`a.cat`, P 5, D 2: set 1 message 1 collides with set 3 message 8 at index 2 and
  goes to index 7). The reference library checks every plane at that index: a message in plane
  2 is found although its plane-1 slot is empty (`message in the second plane`), and a message
  away from its index is not found (`slots 0 and 2 swapped`, `plane size 2`). A slot whose
  first word is the set number itself, not `set + 1`, is not found.
* **Which table is read.** On x86_64 the reference library reads the first (little-endian)
  table: zeroing it makes every message missing (`ENOMSG`), zeroing the second changes nothing.
  A file written in the big-endian order (magic, P and D byte-swapped, the big-endian table
  first) is rejected by `catopen` (-1, `errno` 0). The second table serves a reader of the other
  byte order; a little-endian reader can ignore it except for locating the string pool.
* **Validation by `catopen`** (record: `damaged .cat files`): fewer than 12 bytes or a directory
  give -1 with `EINVAL`; a wrong magic, a file shorter than its tables (12 bytes; 40 bytes) or a
  string offset beyond the file give -1 with `errno` 0. A string pool cut inside a string is
  accepted and the bytes past the end read as zeros (`one-on`, then `""`); a last string without
  its NUL reads on into the zero fill (`two-oneX`). A string offset inside another string returns
  that string's tail (`-one`). Plane size 0 kills the process with `SIGFPE` at `catopen` or
  `catgets` (the record does not separate them); plane depth 0 and sizes whose product overflows
  are unstable cases.
* **`gencat` output** (not needed to read a catalog): the string pool holds the sets in
  descending order and each set's messages in ascending order (`a.cat`: set 3's three strings,
  then set 1's); `gencat` chose P 5, D 2 for six messages, P 1, D 3 for three and P 1, D 1 for
  one. `gencat` stores the message text after its own escape processing; `catgets` returns the
  stored bytes unchanged (catalogs.md).

#### D.2.4 Catalog directories (gettext)

gettext's list of candidate directories for a locale name is not D.1's `setlocale` procedure
(2026-10-04, from `spec/behaviour/catalogs.md`, the "locale name variants" section of its record).
For `language[_territory][.codeset][@modifier]` the candidates drop, in order, the modifier last,
then the territory, then the codeset; a codeset is tried as written and then normalized
(lowercased, non-alphanumerics removed), and an all-digit normalized codeset gains the prefix
`iso` (`8859-1` → `iso88591`, where D.1 gives `88591`). An empty codeset counts as absent. Liella
never uses a candidate component that is empty, `.`, `..` or contains `/` (a decision, pending
the owner's confirmation as a security default).

#### D.2.3 Agreement with the documentation

* The `.mo` header, the two descriptor tables, the sorting, the NUL termination, the context
  separator `\x04` and the plural layout are exactly as the GNU gettext manual describes them.
* The manual's chapter does not describe the minor-1 header fields or the system-dependent
  string tables; their layout above is from inspection of `msgfmt`'s output and one black-box
  lookup.
* The hash function is undocumented by the manual and was not derived; with a hash table present
  the reference library does not fall back to a search, so an implementation that always
  searches answers identically for every well-formed `msgfmt` file and differently for a
  damaged hash table (decision for the coordinator; catalogs.md, "What the record does not
  settle").
* The `.cat` format is implementation-defined (POSIX specifies only `gencat`'s input). The
  layout above is the reference system's.

### D.3 The loader cache `ld.so.cache` (phase 14) — §27.1, D-27.3

**How it was established** (plan §1.2, format inspection; story DL-CACHE-FORMAT, 2026-10-23). On
keke (Ubuntu 24.04, glibc 2.39-0ubuntu8.9), `tools/probe/ldcache_run.sh` builds a private root
of tiny shared objects (`gcc -shared`, chosen sonames, soname links, `glibc-hwcaps/x86-64-v2`
and `x86-64-v3` subdirectories) and has `ldconfig -r <root> -X -i -c new|compat|old` write
caches for it; the files and keke's own `/etc/ld.so.cache` were read with `xxd`. The layout
below is the hypothesis that `tools/probe/ldcache_dump.py` implements. It parses every one of
these files strictly (all offsets bounds-checked) and writes new-format files of its own (its
own string layout, no shared tails), which `ldconfig -p` lists correctly. It was then checked
against the reference loader: `tools/probe/ldcache_loader.sh` bind-mounts each cache over
`/etc/ld.so.cache` inside an unprivileged user and mount namespace (`unshare -r -m`; the host's
`/etc` is never changed) and records which file `ld.so --list` resolves for a program with four
`DT_NEEDED` entries. Documentation compared: ldconfig(8) (man-pages 6.7), `ldconfig --help`. No
loader or ldconfig source was read. Integers are little-endian.

**Formats.** ldconfig(8): `-c` selects `old`, `new` or `compat`, and the default has been `new`
since glibc 2.32. keke's `/etc/ld.so.cache` is `new` (561 entries, all with flags `0x0303`, no
hwcaps extension). The reference loader reads all three (D.3.4).

#### D.3.1 New format (`glibc-ld.so.cache1.1`)

```
off  size
  0   20  magic "glibc-ld.so.cache" + version "1.1" (no NUL)
 20    4  nlibs        number of entries
 24    4  len_strings  length of the string table (bytes)
 28    1  flags        2 in every file seen (meaning not established; likely byte order)
 29    3  zero
 32    4  ext_offset   file offset of the extension directory; 0 = none
 36   12  zero (three u32)
 48       nlibs entries of 24 bytes:
            +0  i32 flags      library type and ABI (below)
            +4  u32 key        offset of the soname (NUL-terminated)
            +8  u32 value      offset of the path (NUL-terminated)
           +12  u32 osversion  0 in every file seen
           +16  u64 hwcap      bit 62 set: low 32 bits index the hwcaps-name array (D.3.2);
                               0 for a plain entry
 48 + 24*nlibs   the string table, len_strings bytes
```

* String offsets count from the start of the new header (the file start in a `new` file). They
  point into the string table, and strings share tails: a key is usually the tail of its own
  path (`/usr/lib/x86_64-linux-gnu/libnosoname.so` holds both). The string table is not sorted.
  A reader must check that each offset is in range and that its string ends inside the table.
* `key` is the library's `DT_SONAME`, or its file name when it has none. `value` is the
  scanned directory plus `/` plus the key, i.e. the soname link ldconfig makes, not the file
  the link points to. With `-X` (no links made) a cache names `libalpha.so.1` while only
  `libalpha.so.1.9` exists, and the loader then reports "cannot open shared object file".
* `flags`: the low byte is the type (`3` = ELF libc6), bits 8-15 the ABI. `ldconfig -p` labels
  (from hand-written entries): `0x0003` libc6 (no ABI, i.e. 32-bit), `0x0303` x86-64, `0x0803`
  x32, `0x0103`/`0x0403`/`0x0503`/`0x0703` "64bit", `0x0203` "512", `0x0603` N32,
  `0x0903` hard-float, `0x0a03` AArch64, `0x0b03` soft-float, `0x0c03` nan2008, `0x0d03`
  N32,nan2008; a type of 0, 1 or 2 is "unknown or unsupported flag". **Observed:** the x86-64
  loader skips `0x0003` and `0x0803` entries and takes the next entry with the same key.
* Padding: the extension directory starts at the end of the string table rounded up to 4
  (keke's `/etc/ld.so.cache` has `ext_offset` `0x89ec`, which is not 8-aligned).

#### D.3.2 Extensions

At `ext_offset`: `u32 magic 0xeaa42174`, `u32 count`, then `count` records of four u32
`{tag, flags, offset, size}` (flags 0 in every file seen). Seen tags:

* `0`: the generator, `size` bytes of text with no NUL (`ldconfig (Ubuntu GLIBC
  2.39-0ubuntu8.9) stable release version 2.39`; `ldconfig -p` prints it as "Cache generated
  by:").
* `1`: the glibc-hwcaps subdirectory names, `size/4` u32 string offsets (into the string table,
  counting like the entries' offsets). Present only when an entry has bit 62. An entry with
  index `i` lives in `<dir>/glibc-hwcaps/<name i>/`.

#### D.3.3 Order and lookup

* **Sort.** Entries are sorted descending by key under a comparison where a run of digits
  compares by numeric value and ranks above any non-digit, other bytes compare by value (case
  matters), and a key that is a prefix of another is smaller. Observed sequence:
  `lib10.so`, `lib9.so`, `libv.so.2`, `libv.so.1.10`, `libv.so.1.09`, `libv.so.1.9`,
  `libv.so.a`, `libnosoname.so`, …, `libab.so.1`, `libab.so`, `lib_x.so`, `libZed.so.1`,
  `libV.so.1`, `lib-x.so`. Under this comparison `1.09` and `1.9` are equal; their relative
  order was not settled.
* **Equal keys.** Entries with the same key keep ldconfig's directory order: the `-f`
  configuration directories in file order, then the trusted ones (here
  `/usr/lib/x86_64-linux-gnu`); hwcaps entries come before the plain entry of their directory.
* **Loader, observed** (`ld.so --list`, D.3 method):
  * Among entries with the same key it takes the first acceptable one in file order (swapping
    two plain entries swaps the result).
  * Key matching uses the comparison above, not byte equality: with both `libv.so.1.09` and
    `libv.so.1.9` in the cache, a request for `libv.so.1.9` loaded the `libv.so.1.09` entry's
    file.
  * hwcaps: the supported subdirectory with the highest priority wins, not the first in file
    order (`v2, v3, plain` and `v3, v2, plain` both load `x86-64-v3` on keke, which supports v3
    and not v4); `v2, plain` loads v2; `v2, v4, plain` loads v2. A plain entry placed before
    the hwcaps entries is taken. When the first hwcaps entry of the run is unsupported or
    unknown (`v4, v3, plain`; `v4, v2, plain`; `no-such-level, plain`) the plain entry is
    taken. In these probes that entry was also hwcaps index 0, so the two causes were not
    separated. ldconfig wrote v2 before v3, so its own caches did not trigger this.

#### D.3.4 Old and compat formats

* **Old** (`-c old`): `"ld.so-1.7.0\0"` (12 bytes), `u32 nlibs`, then `nlibs` entries of 12
  bytes `{i32 flags, u32 key, u32 value}`, then the strings. The offsets count from the end of
  the entries. There are no hwcap or osversion fields: hwcaps entries appear as plain entries,
  in the same order. **Observed:** the loader reads it and loaded the `x86-64-v2` file for
  `libgamma.so.10` (the first entry with that key).
* **Compat** (`-c compat`): an old part, then a complete new-format part. Its strings are shared:
  the old entries' offsets and the new part's string offsets all count from the start of the
  new header (`0x130` in the probe). The extension directory's `ext_offset` and its record
  offsets count from the start of the file. The hwcaps name offsets (tag 1) count from the new
  header. `ldconfig -p` prints those names as `""`, and the loader took the plain
  `libgamma.so.10` rather than a hwcaps entry. In the probe the old part listed 24 entries for
  23 libraries, the last one twice; the pure old file listed 23.

**Reader notes for DL-8** (recommendations; the decisions are DL-8's): read the `new` layout
(and the new part of a `compat` file), and check every offset and string end against the file
size before use (DL-0a covers corrupt caches). Matching keys byte for byte rather than under
the numeric comparison would never load a file under another soname, but it would differ from
the reference library only when two keys differ only in leading zeros. Take the hwcaps choice
from the highest-priority supported entry among those before the run's first plain entry.
