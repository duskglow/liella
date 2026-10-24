# Architecture

This document explains how Liella is put together: where things are, which rules shape the
code, and why. It is written for experienced C, Rust and systems developers who are new to the
project. Every design claim names the plan row (`D-…`, `S…`, `P…`) or the file it comes from,
so you can check it and read the full reasoning. The plan itself starts at
[implementation-plan.md](../implementation-plan.md); its rows are found with
`grep -rn "^| D-12.18 " plan/`.

It describes what is on `main`. Where a decision has been made but its code is not on `main`
yet, the text says so.

## Contents

1. [Purpose and scope](#1-purpose-and-scope)
2. [The four artifacts](#2-the-four-artifacts)
3. [Crate layering](#3-crate-layering)
4. [Where unsafe code is allowed](#4-where-unsafe-code-is-allowed)
5. [How a C call flows](#5-how-a-c-call-flows)
6. [errno and the thread block](#6-errno-and-the-thread-block)
7. [The main subsystems](#7-the-main-subsystems)
8. [Hot kernels](#8-hot-kernels)
9. [How correctness is checked](#9-how-correctness-is-checked)
10. [How performance is checked](#10-how-performance-is-checked)
11. [The build](#11-the-build)
12. [Key design decisions](#12-key-design-decisions)
13. [Limitations, sized](#13-limitations-sized)
14. [Where to start reading](#14-where-to-start-reading)

## 1. Purpose and scope

Liella is a clean-room reimplementation of the GNU C library's ABI, version 2.39, for x86-64
Linux, written in Rust. A program compiled against the glibc 2.39 headers should link against
Liella's `libc.a` (and later `libc.so.6`) and behave as it does on glibc.

The single source of truth is [glibc-functional-spec.md](../glibc-functional-spec.md): a
functional description of every exported symbol, written for this project, with Appendix C
holding the ABI data. glibc's source code is never consulted, by people or by agents. Where the
spec leaves behaviour open, a black-box probe of the installed glibc settles it, and every
deliberate difference is recorded in
[docs/differences-from-glibc.md](differences-from-glibc.md)
([plan/04-test-methodology.md](../plan/04-test-methodology.md) §4.2).

"ABI-compatible" means three concrete things here:

| Aspect | What it means | Where it is recorded |
|---|---|---|
| Symbols and versions | Every exported name carries glibc's version node (`malloc@@GLIBC_2.2.5`). `cargo xtask so` generates the version script from `spec/abi/versions.tsv` and checks the result with `readelf`. | [spec/abi/versions.tsv](../spec/abi/versions.tsv), [docs/toolchain.md §3.2](toolchain.md) |
| Struct layouts | Every type a header exposes (`FILE`, `pthread_mutex_t`, `struct __locale_struct`, `jmp_buf`, ...) has glibc's size, alignment and field offsets. Each layout is a test. | `crates/abi` (`liella_abi::layout`), tests in `crates/abi/tests` |
| Conventions | `errno` through `__errno_location`, the thread pointer at `%fs` with the offsets compiled code reads (`%fs:0x28` canary, `%fs:0x30` pointer guard), the header-inline protocols (`getc`, `__ctype_b_loc`). | [crates/rt/src/tls.rs](../crates/rt/src/tls.rs), spec Appendix C.9 |

The scope is the whole exported surface: 3,943 exports
([plan/03-architecture.md](../plan/03-architecture.md) §3.1), including `libm`. The supported
environment is Linux 5.10 or later with `/proc` mounted; newer kernel features are detected at
run time, and without `/proc` a function must fail cleanly or use a documented fallback, never
give a wrong answer (OD-5 in [plan/09](../plan/09-open-decisions-and-review-log.md), D-X.12).

The goals, in priority order, are **safe** (no undefined behaviour reachable from the API,
consistent errors), **fast** (no overhead the contract does not need), **scalable** (one design
applied uniformly to every symbol) ([implementation-plan.md](../implementation-plan.md)).

**Status.** Phases 0 to 13 are done; the project is in the validation and optimization stage
that precedes phase 14 (dynamic linking) and phase 15 (legacy tier and binary compatibility)
([plan/06-phases/](../plan/06-phases/00-overview.md), `spec/schedule.tsv`). In practice that
means: statically linked programs run on Liella today; dynamically linked ones are built and
checked but cannot run until Liella's own `ld.so` exists (section 13).

## 2. The four artifacts

| Artifact | Path | What it is |
|---|---|---|
| The spec | `glibc-functional-spec.md`, `spec/` | The functional contract, the ABI data, the symbol manifest (`spec/symbols.tsv`), the clause list (`spec/clauses.tsv`), behaviour records from probes (`spec/behaviour/*.md`) |
| The C syscall layer | `csys/` | System-call wrappers and the few machine primitives Rust cannot express. No logic (rules K1-K6). |
| The Rust library | `crates/` | Everything else, in safe Rust except one crate |
| The POSIX translation layer | `crates/boundary/src/posix/` | One file per spec section (`s01_startup.rs` ... `s29_legacy.rs`); each export converts C conventions and makes one call |

## 3. Crate layering

### 3.1 The picture

```
           C programs (glibc 2.39 headers, existing object code)
                              |  C ABI, versioned symbols
+-----------------------------v-------------------------------------+
| liella-boundary   posix/ (exports), native/ (native C ABI),       |
|                   conv, strz, export, data, heap, tb, hot         |
|                   the only crate with unsafe Rust                 |
+------+--------------------+-----------------+---------------+-----+
       | calls, passing CSys as the kernel     |               | GlobalAlloc
+------v---------+ +--------v-------+ +-------v--------+ +----v----------+
| liella-legacy  | | liella (core)  | | liella-math    | | liella-rt     |
| historical     | | the clean API  | | generic floats | | startup, heap |
| semantics      | |                | |                | | threads, sync |
+------+---------+ +--------+-------+ +-------+--------+ | signals, TLS  |
       |                    |                 |          +----+----------+
       |  all generic over S: Sys + Machine, all #![forbid(unsafe_code)]
+------v--------------------v-----------------v---------------v-------+
| liella-kernel   the Sys and Machine traits, Fd, KPath, Errno, uapi |
| liella-abi      C types, Appendix C layouts, return conventions    |
| liella-mem      memcpy/memset/memcmp loops, #![no_builtins]         |
+------------------------------+--------------------------------------+
                               | implemented by
+------------------------------v--------------------------------------+
| liella-boundary::sys (CSys) -> liella-boundary::ffi                 |
+------------------------------+--------------------------------------+
+------------------------------v--------------------------------------+
| csys/  liella_sys_* wrappers, machine/*.S, variadic adapters (C/asm)|
+------------------------------+--------------------------------------+
                               v
               Linux kernel (syscall, vDSO, auxv, signal frames)
```

Adapted from [plan/03-architecture.md](../plan/03-architecture.md) §3.1.

### 3.2 What each crate owns

| Crate (package) | Path | Owns | Normal dependencies |
|---|---|---|---|
| `liella-kernel` | `crates/kernel` | The kernel as a value: the `Sys` and `Machine` traits, kernel types (`Fd`, `KPath`, `Errno`), uapi structs, auxv and vDSO parsing over slices, the `Faulty` fault injector. Makes no system call itself. | none |
| `liella-abi` | `crates/abi` | C types, the Appendix C layouts, the safe half of the conversion vocabulary (`ret`, `pack`, `chk`, the `va` reader) | kernel, core |
| `liella-mem` | `crates/mem` | The loops behind `memcpy`, `memmove`, `memset`, `memcmp`, `bcmp`, in a `#![no_builtins]` crate so they never compile back into calls to themselves (D-4.11) | none |
| `liella-rt` | `crates/rt` | Runtime logic: process start, threads, synchronisation primitives, the allocator algorithm, signal dispatch, contexts, FPU and CPU features, the thread block layout, the DWARF unwinder | kernel |
| `liella` (core) | `crates/core` | The clean library: every functional contract implemented once, in the shape the need requires (text, numbers, locales, streams, printf/scanf engines, time zones, regex, name services, ...) | kernel, mem, rt |
| `liella-legacy` | `crates/legacy` | Historical semantics the ABI obliges: hidden cursors, static result slots, process-global ambient state, superseded versioned contracts, mode-string parsing, `va_list` decoding (rule C5) | kernel, rt, core, abi |
| `liella-math` | `crates/math` | `libm`: one generic implementation per operation over binary32, binary64, x87 extended and binary128 (D-10.1) | rt |
| `liella-boundary` | `crates/boundary` | The C boundary: the POSIX exports, the native C ABI, the `CSys` implementation of `Sys`, the global allocator shim, exported data objects, the hot kernels | kernel, rt, abi, core, legacy, math |
| `liella-native` | `crates/native` | A facade for Rust programs that want the clean API without POSIX conventions (D-X.10, [docs/interfaces.md](interfaces.md)) | core, math, optionally rt, kernel |
| `liella-ld` | `crates/ld` | The dynamic linker (phase 14, not yet written) | kernel, rt |
| `liella-thread-db` | `crates/thread-db` | `libthread_db` for Liella's thread layout (phase 14) | kernel |
| `liella-mathtools` | `crates/mathtools` | Test and tool code only: the arbitrary-precision oracle, the Remez fitter, the glibc comparison (D-10.12). No product depends on it. | none |
| `spectest` | `crates/spectest` | Traceability macros: pending tests generated per clause and symbol | none |
| products | `crates/libc-so`, `crates/libm-so` | Separate workspaces that link `liella-boundary` with the export features into `libc.a`/`libc.so.6` and `libm.a`/`libm.so.6`, and name the global allocator and panic handler once | boundary |
| `xtask` | `xtask/` | Every build, check, generation and test command (section 11) | none |

The dependency columns are the `[dependencies]` sections of each `Cargo.toml`
(dev-dependencies excluded).

### 3.3 The rules about who may call whom

* **No cycle.** At run time `liella-boundary` is both the top (the exports) and the bottom (the
  `Sys` implementation), but in Cargo terms it depends on every library crate and nothing depends
  on it. The library crates reach the kernel only through the `Sys`/`Machine` traits they are
  generic over (rule X5, [plan/02](../plan/02-design-rules.md) §2.2).
* **Core never imports legacy** (rule C5). Historical behaviour (a static result buffer, a
  hidden cursor, a superseded version of a function) lives in `liella-legacy`, which calls core.
  Core stays free of it.
* **The translation layer contains no logic** (§8 of the plan, checked by `cargo xtask purity`).
  Each export in `posix/` is one expression: convert the arguments, call one library function,
  convert the result. Anything that needs a decision goes into core, legacy or rt.
* **Explicit context** (rule C2). Locale, time zone, environment, allocator domain, cursors,
  seeds and the kernel itself are parameters or values, never ambient state inside core.
* **`no_std` everywhere.** Rust's `std` links a C library, and this project is the C library.
  All library crates are `#![no_std]` with `alloc`; the global allocator is
  `liella_boundary::heap`, which delegates every decision to `liella_rt::heap`
  ([plan/03](../plan/03-architecture.md) §3.1).
* **Zero external dependencies**, by policy: no existing libc bindings or implementations may be
  used, even as a reference (workspace [Cargo.toml](../Cargo.toml), plan §1.2). The fuzz
  harnesses are a separate workspace with `libfuzzer-sys`, outside the product build.
* **Test builds never interpose.** Rust exports carry `#[unsafe(no_mangle)]` only under feature
  `export`, and C adapters carry glibc names only under `LIELLA_EXPORT`; only the products
  enable them, so a test binary runs on the host's C library unless a test asks otherwise.

### 3.4 Where state lives

| State | Owner | Examples |
|---|---|---|
| Kernel-held, process-wide | The kernel, never cached | cwd, umask, signal dispositions, limits, credentials |
| Contract-visible library state | `liella-legacy` logic, storage in `liella-boundary::data` when exported | `errno`, `environ`, the global locale, `getopt` data symbols, `*ent` cursors, `tzname`, `_res`, `h_errno` |
| Clean library state | Explicit values in `liella` | `Locale`, `Zone`, `Env`, `Stream`, `Resolver`, `OptParser` |
| Runtime state | `liella-rt`, in memory the boundary converts to slices | thread blocks, allocator metadata, the signal table, CPU features |

Source: [plan/03-architecture.md](../plan/03-architecture.md) §3.3. Rule C7 (P-scope) decides
per item whether hidden state stays process-global (where cross-thread use is legal) or becomes
per-thread (where cross-thread use was already a data race).

## 4. Where unsafe code is allowed

The policy is rules S1-S11 in [plan/02-design-rules.md](../plan/02-design-rules.md) §2.1.

* **S1.** Every crate except `liella-boundary` carries `#![forbid(unsafe_code)]`, which cannot
  be overridden. The one exception is `liella-native`, a library for programs, whose C-ABI
  declarations sit in one file under category *native*.
* **S2.** `liella-boundary` is `#![deny(unsafe_code)]`, and each site carries
  `#[allow(unsafe_code)]` after a comment `// UNSAFE(<category>): <why>` plus clippy's
  `SAFETY:` justification. There are six categories, each limited to named files
  ([plan/03](../plan/03-architecture.md) §3.2):

| Category | What it permits | Files |
|---|---|---|
| extern | declaring the C layer's functions (`unsafe extern "C"`, every item `safe`, pointer items private and wrapped by slice-taking functions, S4) | `ffi.rs`, `caller_alloc.rs` |
| export | `#[unsafe(no_mangle)]` on exports and data objects | `export.rs` (the `export!` macro), `data.rs` |
| slice | converting a C pointer and length into a Rust slice or reference | `conv.rs`, `strz.rs`, `heap.rs`, `tb.rs`, `runtime.rs`, single conversions in `posix/*` and `native/*` |
| allocator | `unsafe impl GlobalAlloc` | `heap.rs` |
| tb-field | one `%fs`-relative load of a field of the current thread's own block, by a compile-time checked offset; the only inline assembly in Rust (owner ruling 2026-10-06) | `tb.rs` |
| hot-kernel | unaligned SSE2 loads and stores for small copies, fills and compares over ranges the entry has already validated (owner ruling 2026-10-08, D-4.22) | `hot.rs` |

* **S3.** Everything else that needs the machine (system calls, process entry, the signal
  restorer, `setjmp`/`longjmp`, context switches, thread start, CPUID, the vDSO call) is in
  `csys/`, which contains no logic: every C function body is one statement (rule K2), checked by
  `cargo xtask csys-audit`.
* **S6.** Contract violations get defined outcomes where detection is O(1): a NULL where the
  contract has an error channel gives that error; where it has none, a diagnostic and an abort,
  not undefined behaviour. What cannot be detected from a C pointer (a buffer shorter than
  claimed) stays the caller's responsibility, as in any C library.
* **S7, S8.** The library never panics on any input (clippy `unwrap_used`, `indexing_slicing`,
  `arithmetic_side_effects` and friends), and allocation failure is `ENOMEM`, never an abort.

`cargo xtask unsafe-audit` enforces all of this on every test run: it rejects `unsafe` outside
the boundary, an unknown category, a category outside its files, a missing comment, and inline
assembly anywhere but the one `tb-field` site; it prints the count per category so growth is
visible. The audit is a token-level tripwire, not a proof (its own module comment says so,
[xtask/src/policy.rs](../xtask/src/policy.rs)); review is the primary control.

**What this means in practice.** The unsafe surface is small and listed: a reviewer can read
every unsafe site by grepping `UNSAFE(`. The categories exist because Rust offers no safe way to
do those operations (declare a foreign function, export a symbol, turn a C pointer into a
slice). Memory safety of the whole library therefore rests on (a) these sites, (b) the C layer
and its assembly, and (c) the caller passing valid buffers, which no C library can check.

## 5. How a C call flows

Take `read(2)` as an example
([crates/boundary/src/posix/s11_lowio.rs](../crates/boundary/src/posix/s11_lowio.rs)):

```rust
export! { "GLIBC_2.2.5";
pub extern "C" fn read(fd: c_int, buf: *mut c_void, n: size_t) -> ssize_t {
    // UNSAFE(slice) SAFETY: buf holds n bytes (§11.3).
    cancel::point(ret::ssize(unsafe { conv::buf_mut(buf, n) }.and_then(|b| CP.read(conv::fd(fd), b))))
}}
```

```
C caller
  | read(fd, buf, n)                         symbol read@@GLIBC_2.2.5
  v
export!  (boundary/src/export.rs)            no_mangle only with feature `export`
  | conv::buf_mut(buf, n)                    C pointer + length -> &mut [u8]  (category slice)
  | conv::fd(fd)                             c_int -> Fd
  v
library call  (here Sys::read directly;      safe Rust, generic over S: Sys
  most exports call into core/legacy/rt)
  v
CSys::read  (boundary/src/sys.rs)            the one implementation of Sys
  v
ffi::liella_sys_read  (boundary/src/ffi.rs)  `safe` extern declaration
  v
csys/src/sys/io.c                            one statement: the system call,
  |                                          returns value or -errno; never touches errno
  v
kernel
  ^ result travels back up:
ret::ssize(...)                              the C convention: -1 and errno on failure
cancel::point(...)                           a deferred cancellation acts here (D-22.2)
```

The conventions on the way back are fixed by rules X1-X5 ([plan/02](../plan/02-design-rules.md)
§2.2): one internal error type (`Errno` plus an internal `Canceled` marker); non-error outcomes
are typed results (`Parsed::NoDigits`, `Written::Truncated`); `errno` is written only on
failure, only by the translation layer, through one `liella_abi::ret` function per C convention;
`EINTR` is surfaced with no hidden retry unless a contract requires completion.

Cancellation (D-22.2) is an ordinary error: a cancellation point returns `Error::Canceled`,
which propagates through Rust frames like any error, and at the boundary `ret` hands it to
`rt::cancel::act()`, which runs the cleanup handlers and ends the thread. No Rust frame is ever
unwound through (S11).

Because every library function is generic over `S: Sys`, tests substitute
`liella_kernel::testing::Faulty`, which injects `EINTR`, `ENOSPC`, `EMFILE`, `ENOMEM` and short
transfers into chosen calls (X5). In the product, `CSys` is a zero-sized type and the generics
monomorphise away.

The memory handed to C callers (`strdup`, `getline`, `asprintf`) comes from the exported,
interposable `malloc`, so a program that interposes its own allocator gets its own blocks back;
library-internal memory uses the internal heap directly (D-3.1).

## 6. errno and the thread block

Each thread has a **thread block** at the thread pointer (`%fs`), defined in
[crates/rt/src/tls.rs](../crates/rt/src/tls.rs) (IP-8). It is 0x300 bytes, 64-byte aligned,
made of atomic words. The prefix up to 0x80 keeps the offsets compiled code expects (`%fs:0`
self pointer, `%fs:0x28` stack canary, `%fs:0x30` pointer guard). Liella's own fields sit in the
range glibc reserves as unused, so a block glibc's `ld.so` laid out is also a valid Liella block:

| Offset | Field | Purpose |
|---|---|---|
| 0x80 | `errno` | this thread's `errno`; `__errno_location` returns its address (D-2.1) |
| 0x84 | `tid` | kernel thread ID, cleared by the kernel at exit |
| 0x88 | `tls_dtors` | C++ `thread_local` destructor list |
| 0x90, 0x98 | `heap_cache`, `node` | the thread's allocator cache and NUMA node (D-3.10, D-3.11) |
| 0xa0 | `thread` | the thread descriptor (D-22.9) |
| 0xa8 | `locale` | the thread's `uselocale` choice, 0 for the global locale (D-7.8) |
| 0xb0-0xc0 | `ctype_b`, `ctype_toupper`, `ctype_tolower` | the pointers `__ctype_*_loc` return the addresses of, always current (D-5.5) |
| 0xc8 | `lconv` | the per-thread `struct lconv` of `localeconv` |
| 0x128 | `h_errno` | |
| 0x130 | `res` | the thread's resolver state (`__res_state()`) |
| 0x138 | `view` | the thread's current pre-decoded locale view (D-7.13) |
| 0x140 | `cache[..]` | one cache line of per-thread cache words; each owner validates what it reads (first owner: the time-zone period cache) |
| 0x180-0x300 | | reserved for later phases |

Hot paths read these words with **one `%fs`-relative load** through `tb::read`
([crates/boundary/src/tb.rs](../crates/boundary/src/tb.rs), category *tb-field*). Callers name
a field through a sealed marker type, never an offset. This exists because measurement showed
that reaching the thread block through a function call plus a dependent load cost about half of
`toupper`'s time (S2, *tb-field* evidence).

Two decisions build on it:

* **D-5.5.** The three ctype words always hold the active tables: `uselocale`, `setlocale` and
  thread creation store them, so `__ctype_b_loc` returns the word's address without re-resolving,
  and `isalpha` ... `toupper` read the word, range-check and index: one `%fs` load, one check,
  one table load.
* **D-7.13.** When a locale is interned, an immutable **LocaleView** is built with it (encoding,
  `MB_CUR_MAX`, radix, grouping, collation, ctype table addresses, wide tables) and never freed.
  The `view` word holds the calling thread's view, kept by the same writers as D-5.5, so
  `locale::view()` is one load and every locale-sensitive call (`mb*`, `printf`, `strto*`) reads
  pre-validated data. Cost: about 4.5 KB per interned locale and 8 bytes per thread. The
  per-category item views (LV-3) are not implemented yet; the row says so.

## 7. The main subsystems

**Memory allocator** (spec §3; [crates/rt/src/heap.rs](../crates/rt/src/heap.rs),
D-3.1, D-3.7, D-3.10 to D-3.13). The algorithm lives in safe Rust in `liella-rt` and manages
addresses only; the boundary supplies the slices and the copy and zero callbacks. Pages come
from `mmap`, never `brk`, so `brk`/`sbrk` cannot corrupt the heap (D-3.7). The heap has one
shard per NUMA node (up to 64), each with its own lock, free lists and counters on separate
cache lines, in 16 MiB aligned segments found through a lock-free registry (D-3.10). Each
thread owns a cache of slabs per size class: local allocation and free touch only words the
owner writes, with plain loads and stores and no atomic read-modify-write; a free from another
thread sets a bit in the slab's remote bitmap and queues the slab on the owner's lock-free
pending list (D-3.11). A thread's NUMA node is read on slow paths only, from rseq or the vDSO
`getcpu`, and memory is placed with `MPOL_PREFERRED`, never a hard binding, so a full node
degrades to remote memory rather than failing (D-3.12, rule M3). Caches of exited threads are
parked per node and reused whole.

**Buffered streams** (spec §12; [crates/core/src/stream.rs](../crates/core/src/stream.rs),
D-12.1, D-12.11 to D-12.14). One buffering engine works over a `Backing` (a descriptor, a fixed
or growing memory buffer, caller callbacks) and operates on a state over a caller-held buffer
slice, so the same engine serves the C `FILE`, whose buffer window must be visible to the
header-inline `getc`/`putc`, and a Rust-owned stream. The `printf` engine parses a format once
into literal runs and typed conversions (D-12.12); `va_list` decoding is legacy code, and `%n`
is honoured only where the contract requires it (D-12.4). The scanf engine (D-12.13) and the
wide-stream layer (D-12.14) were each written from the spec and black-box probes in a fresh
session.

**Locales** (spec §5-§7; `crates/core/src/locale/`, D-7.7, D-7.8, D-5.5, D-7.13). Locale data
is loaded once from the system's compiled locale files, interned, and shared read-only by every
thread; one lock serialises only first loads and changes of the global locale, and every lookup
is lock-free. Interned locales are never freed, which is what makes the lock-free reads and the
per-thread view pointers safe; an explicit reload (`liella_locale_invalidate`, D-X.13) builds a
new view and leaks the old one. The reads go through the thread block words described in
section 6.

**Regular expressions** (spec §14.9; [crates/core/src/pattern/regex.rs](../crates/core/src/pattern/regex.rs),
D-14.4, D-14.8 to D-14.11). A parser builds a tree under the `RE_*` syntax bits; the program is
a Thompson automaton serialised into the `regex_t`; the matcher runs all paths in parallel,
position by position, so matching without back-references is linear in the subject for a fixed
pattern. Subexpressions are found by a second run anchored on the overall match. Back-reference
patterns use bounded backtracking with `re_max_failures` as the budget. regcomp derives the
matcher's tables once (D-14.9), regexec uses a required-bytes prefilter (D-14.10) and allocates
nothing per call (D-14.11). A lazy DFA is on the optimization target list (rule P10, item 11);
it is not on `main`.

**Time zones** (spec §23.5-§23.6; [crates/core/src/time/zone.rs](../crates/core/src/time/zone.rs),
D-23.4 to D-23.10). A `Zone` is built from a TZif file (v1 to v4, RFC 8536) or a POSIX `TZ`
rule and answers both directions: instant to local time (`localtime`) and local time to instant
(`mktime`, which returns all candidate instants for ambiguous wall times). Zone data is loaded
once and pinned until explicitly invalidated (D-X.13). `mktime` allocates nothing per call (D-23.10), and
`localtime` keeps the current zone period in the thread block's cache words (`tb::TzPeriod0`).

**Name services and the integrated cache** (spec §16-§17; `crates/core/src/names/`,
D-16.10). The NSS engine implements the built-in `files` and `dns` sources, including a DNS
stub resolver and IDNA. In place of `nscd`, Liella has an integrated name-service cache:
answers of the built-in sources for hosts, passwd, group, services, protocols and networks,
keyed by the exact query and the lookup-steering environment, in a per-user shared region
(`/dev/shm/liella-nscache-<euid>`) whose every word is treated as untrusted input
([crates/core/src/names/cache.rs](../crates/core/src/names/cache.rs)). It exists for speed
only: a cached answer is at most about a second older than the last check of its files (or
within the DNS TTL), shadow and gshadow are never cached, and disabling the cache or calling
`liella_nscache_invalidate` makes the next lookup see every change. Everything that depends on
per-process state (interface-dependent sorting, `getaddrinfo` flags, a modified `_res`) runs
after the cache or bypasses it.

**Mathematics** (spec §10; `crates/math`, D-10.1 to D-10.14). One implementation per operation,
generic over the floating formats; formats that coincide are exported as aliases (D-10.1).
Internally a function returns `(value, Status)` with IEEE flags; `errno` is set only at the
boundary, per `math_errhandling` (D-10.2). Correctly rounded operations compute exactly and
round once (D-10.9); elementary functions use double-double arithmetic with a documented ULP
bound per function and format (D-10.3, D-10.13, [docs/libm-accuracy.tsv](libm-accuracy.tsv)).
The accuracy reference is an arbitrary-precision oracle written in this project
(`crates/mathtools`, D-10.12), and polynomial coefficients are fitted by its Remez tool, with
each fit regenerated and checked by `cargo xtask test`. The `libm` product is separate
(`crates/libm-so`, D-10.7).

**Process runtime and threads** (spec §1, §20-§22; `crates/rt`). Process entry is
`csys/src/machine/entry.S`, startup sequencing is `rt::start` (IP-9). Synchronisation objects
are built on atomics and `futex`, fit their ABI sizes, are valid when all-zero and hold no
pointers, so they work process-shared (D-22.1). Signal handlers enter through
`machine/restorer.S` and a dispatch table in rt (IP-16). `backtrace` uses Liella's own DWARF
call-frame unwinder (`rt::unwind`, D-2.7).

## 8. Hot kernels

The default is safe Rust. For a small number of very hot functions, measurement showed that
safe Rust kept the function measurably slower than glibc, and the owner allowed bounded
exceptions. The division of labour is the same in each case: **Rust validates once, at the
entry and outside any loop** (sizes, NULL, bounds, CPU state) and owns all policy; **a small
kernel does the tight loop** over the already-validated range, with no checks inside. The
kernel's contract (valid ranges in, nothing outside them touched) is stated in one comment and
enforced by the Rust caller.

| Kernel | Where | What | Decision |
|---|---|---|---|
| Terminator scans (`strlen`, `strnlen`, `rawmemchr`, every C-string conversion in `strz`) | `csys/src/machine/scan.S` | Aligned 32-byte (AVX2) or 16-byte (SSE2) loads that never cross a page; the path is chosen once from CPUID and XCR0 (`boundary::cpu`), SSE2 until then | D-4.14, D-4.24, D-4.21 |
| Small copies and fills (`memcpy`, `memmove`, `memset` and fortified forms, up to 128 bytes) | `crates/boundary/src/hot.rs` | 16-byte blocks from both ends, every source byte loaded before the first store (so overlap needs no test); SSE2 only, one kernel for every CPU | D-4.22 |
| Small compares (`memcmp`, `bcmp`, `__memcmpeq`, up to 64 bytes) | `crates/boundary/src/hot.rs` | the same treatment for compare | D-4.23 |
| Large copies | `csys/` machine class | ERMS (`rep movsb`) paths | D-4.14 |

**Checks that remain.** For the scanners, Rust checks in constant time that the reported byte is
the terminator and that a bounded result is within its bound. An answer outside its bound
aborts; a reported byte that changed under the call (another thread writing) is rescanned once
(D-4.21). Each kernel carries an oracle test, an exhaustive sweep (every alignment and length,
the terminator at every offset around an unmapped guard page) and planted-bug tests that prove a
wrong byte or length fails the suite (for example
[crates/boundary/tests/small_copy.rs](../crates/boundary/tests/small_copy.rs): sizes 0 to 256
by 32 x 32 alignments, overlap shifts, guard pages, three planted bugs).

**Sizing the risk.** These are the only places where a bug could read or write outside a
validated range without Rust catching it. They live in two files (`hot.rs`, 665 lines including its
contract comments and SSE2 intrinsics; `scan.S`, 1,486 lines of written-out assembly), handle only ranges the Rust entry has validated, and are covered by the
exhaustive tests above. A defect would most likely show up as a wrong result in those tests
rather than in the field. The rest of the library does not depend on them for its own memory
safety beyond what any call to `memcpy` implies.

**Decided but not on `main`.** Two later owner rulings (2026-10-08) are recorded in the project's
working notes and are being implemented on branches, not merged:

* *CPU-variant kernels*: a hot kernel may have per-CPU builds (for example SSE2 and AVX2)
  selected once at startup through one mechanism, with every variant tested by forcing it. Rule
  P4 still describes the older plan of whole-library `glibc-hwcaps` builds; neither exists on
  `main` (the scanners' internal AVX2/SSE2 choice above is the only CPU selection today).
* *NULL folding*: a hot small-size path may let a NULL pointer fault on page zero (SIGSEGV, as
  glibc observably does) instead of the explicit S6 abort, only with a startup guard that keeps
  the explicit checks when `vm.mmap_min_addr` is below 64 KiB or cannot be read. Such a path
  must be written in assembly, not Rust, because a NULL read is undefined behaviour in Rust.

## 9. How correctness is checked

The method is in [plan/04-test-methodology.md](../plan/04-test-methodology.md). Tests are
written from the spec before the code, and every test is traceable.

| Layer | What it checks | Where |
|---|---|---|
| Spec traceability | `cargo xtask stubs` generates one pending test per spec clause and per exported symbol; a pending test is `#[ignore]`d and fails if forced to run, so it is never mistaken for a pass. `cargo xtask trace` requires every test to name its clause (or say why it has none), and every row of the differences document and every behaviour record to name one too. | `crates/spectest`, `spec/clauses.tsv`, `xtask/src/trace.rs` |
| Unit and translation tests | The clean contract of core and math against `Faulty<CSys>`; each export's argument mapping, return convention and errno behaviour; each `Sys` method against the real kernel; every Appendix C layout and initializer | `crates/*/tests` |
| Interposed run | The workspace tests run a second time with Liella's exports interposed on the test processes, so the harness itself runs on Liella's `malloc` and `memcpy` (D-4.13) | `cargo xtask test` |
| C conformance | Self-checking C programs compiled against the installed glibc 2.39 headers, statically linked to Liella's `libc.a`, started by Liella's own runtime: real headers, calling convention, data symbols, process start and exit | [conformance/c/](../conformance/c), [conformance/run.sh](../conformance/run.sh) |
| Differential tests | `*_products.rs` tests build the same probe against the host glibc and statically against Liella, run both (some in private namespaces), and require identical output except recorded differences | `crates/core/tests/*_products.rs`, `crates/core/tests/oracle/` |
| Oracle diffs | Engines with an independent oracle (regex, libm against the arbitrary-precision reference) are compared against it | `crates/core/src/pattern/regex_oracle*.rs`, `crates/mathtools` |
| Fuzzing | libFuzzer harnesses for the parsers (printf and scanf formats, regex, glob, wordexp, locale and charmap files, `nsswitch.conf`, `resolv.conf`, DNS messages, the files databases, the nscache table), seeds in `fuzz/seeds`, crash triage rules | `fuzz/`, `cargo xtask fuzz`, plan §4.5 |
| Validation | GNU coreutils, bash and sed test suites run against Liella, failures triaged | [validation/](../validation) |
| Kernel range | Merge gates also run on a Linux 5.10 guest, the oldest supported kernel; tests that depend on newer kernel features detect them at run time and are counted when reduced or skipped (the guest is project infrastructure, not part of the repository) | supported environment: OD-5, D-X.12 |
| Static checks | `purity` (translation layer: conversions and one call), `unsafe-audit`, `csys-audit` (C layer: one statement per body, freestanding includes), `env-audit` (environment writes in tests that also start processes) | `xtask/src/policy.rs` |

Where the spec, a glibc probe and the official documentation (man pages, POSIX, the glibc
manual) disagree, a fixed procedure decides: the spec is corrected when both others contradict
it; a deliberate difference from glibc is recorded in
[docs/differences-from-glibc.md](differences-from-glibc.md); anything else goes to the owner.
The kernel's behaviour wins over all three. Probes are black-box programs; glibc's source is
never an oracle.

Results that follow directory order (`readdir`, unsorted `scandir`, `GLOB_NOSORT`) are compared
as sets, not sequences (D-T.1).

## 10. How performance is checked

`cargo xtask bench` builds the benchmarks in the shipped (release) profile and checks them
against [bench/budgets.tsv](../bench/budgets.tsv) (rule P6). The rows that matter most are
`ratio_host` rows: the same benchmark built once against Liella and once against the host's
glibc, run in the same session, reported as a ratio. Absolute nanoseconds are never compared
across machines; only the same-run ratio is (rule P10, item 13).

* **Parity is the upper bound.** A median ratio of 1.00 or less against glibc is the target
  for every row; a glibc benchmark proves the speed is achievable. A row between 1.00 and 1.05
  is re-run back to back before it counts either way. A row left above parity needs either a
  plan to reach it or a measured reason it cannot.
* **Per-CPU regression limits** in [bench/host_limits.tsv](../bench/host_limits.tsv), keyed by
  the CPU model string, guard against regressions (1.25 x the recorded median). They are guards,
  not targets. A CPU without an entry is measured and reported but not gated.
* **Detailed measurements** (instructions per operation, IPC, static versus dynamic glibc) are
  in [bench/parity-measure.tsv](../bench/parity-measure.tsv); the C benchmarks are in
  [bench/c/](../bench/c), the glibc reference harness in [benchmarks/](../benchmarks).
* **Optimization rules** (P7 to P10): safe first, then fast, then scalable; algorithmic
  changes before memory trades, helper threads or assembly; observable behaviour is frozen;
  caches are benchmarked with varying inputs so a repeated-call loop cannot flatter them.

Current numbers and how to read them: [docs/benchmarks.md](benchmarks.md).

## 11. The build

Everything goes through `cargo xtask` ([xtask/src/main.rs](../xtask/src/main.rs)). Build and
test inside Linux (or WSL); the library targets x86-64 Linux only.

| Command | What it does |
|---|---|
| `cargo xtask stubs` | Regenerate the pending tests and `spec/clauses.tsv` from the spec |
| `cargo xtask so` | Build the products into `target/liella/`: `libc.a`, `libc.so.6`, `liella-crt1.o`, the `libpthread` placeholders, the `libm` products |
| `cargo xtask test` | The full gate: static checks, generated-file checks, the products, the workspace tests twice (plain and interposed), the C conformance suite, the native examples, the Rust dylib, the symbol-gap report, the status table |
| `cargo xtask affected [--list]` | Only the tests a change can reach |
| `cargo xtask bench` | Benchmarks against budgets and per-CPU limits (release profile only) |
| `cargo xtask status [--schedule]` | Pending versus specified tests per section; with `--schedule`, pending work per owning phase |
| `cargo xtask fuzz <target> --time <min>` | Fuzzing |
| `purity`, `unsafe-audit`, `csys-audit`, `env-audit`, `trace` | The static checks, individually |
| `csys-gen`, `abi-gen`, `unicode-gen`, `locale-gen`, `native-gen`, `math-gen` | Generators; `--check` verifies the committed output is current |
| `native`, `rust-dylib` | Build and check the native Rust interface's examples and the Rust dylib |

**Two product profiles** ([xtask/src/so.rs](../xtask/src/so.rs),
[crates/libc-so/Cargo.toml](../crates/libc-so/Cargo.toml)):

| Profile | Settings | Used for |
|---|---|---|
| `fast` | thin LTO, 16 codegen units | `so`, `test`, `affected`: a one-line change relinks in 20-40 s instead of 77-95 s |
| `release` | fat LTO, one codegen unit, `panic = "abort"` | what ships, and every benchmark (`bench` refuses `fast` products); `LIELLA_FULL_PROFILE=1` selects it for the test paths |

The C layer is compiled by `crates/boundary/build.rs` with
`-ffreestanding -fno-builtin -Wall -Wextra -Werror`. Exact flags, toolchain versions, link
lines and the native and dylib builds are in [docs/toolchain.md](toolchain.md).

## 12. Key design decisions

The rows below shape the code most. Each links back to its full text in `plan/`.

| Row | Decision | Why |
|---|---|---|
| S1-S3 | `unsafe` only in `liella-boundary`, in named categories and files; the machine only in `csys/` | Memory safety by construction everywhere else, and an unsafe surface a reviewer can enumerate |
| X5 | The kernel is a value: every kernel-touching function is generic over `S: Sys` | Fault injection in tests (`Faulty`), and an acyclic crate graph |
| C5 | Historical semantics quarantined in `liella-legacy`; core never imports it | The clean API stays free of static buffers and hidden cursors |
| D-3.1 | Two allocation domains: memory handed to callers comes from the interposable `malloc` | Programs that interpose `malloc` must get their own blocks back |
| D-3.7 | The allocator never uses `brk` | `brk`/`sbrk` calls by the program cannot corrupt the heap |
| D-3.10, D-3.11, D-3.12 | Sharded NUMA-aware heap, thread-owned slabs, remote frees by bitmap and pending list, preferred (not bound) placement | Scales with cores and nodes without global locks on the hot path (rules M1-M5) |
| D-4.11, D-4.13, D-X.15 | Memory primitives in a `#![no_builtins]` crate; nothing wider than two words copied on the `memcpy` path; internal copies bind directly | In the product `memcpy` is Liella's own export, so it must never call itself |
| D-4.14, D-4.21 | Terminator scans in the C layer with constant-time postcondition checks in Rust | Speed at glibc's level while the Rust side keeps its safety argument |
| D-4.22, D-4.23 | Small copies, fills and compares in a validated register kernel | Measured: the safe path kept these rows above parity |
| D-5.5 | Per-thread ctype pointers always current | One load and one index per classification, and the ABI's own cache used as intended |
| D-7.8, D-7.13 | Locales interned, never freed, with an immutable pre-decoded view per locale | Lock-free reads, and locale-sensitive calls read pre-validated data |
| D-10.1, D-10.2 | One generic math implementation per operation; errno only at the boundary | One body instead of seven per function; the core stays pure |
| D-12.11 | The `FILE` object's ABI-visible prefix aliases the engine's buffer window | Header-inline `getc`/`putc` work against Liella's streams |
| D-14.4 | Regex by automaton, linear time without back-references | No catastrophic backtracking on ordinary patterns |
| D-16.10 | Integrated, per-user name-service cache replacing `nscd` | Speed with bounded staleness and no daemon; untrusted shared data is validated |
| D-22.1 | Sync objects fit their ABI size, valid when zero, pointer-free | Static initialisers and process-shared use work |
| D-22.2 | Cancellation is an explicit error, never unwinding | No Rust frame is unwound through (S11) |
| D-X.10 | A native C ABI and a versioned Rust dylib beside POSIX | Rust and other languages can use the library without C's conventions ([docs/interfaces.md](interfaces.md)) |
| D-X.13 | Locale and zone data pinned, reloaded only on explicit invalidation | Loaded once per process; old data stays valid for any thread holding it |

## 13. Limitations, sized

| Limitation | What it affects in practice |
|---|---|
| **No dynamic loader yet** (phase 14, `crates/ld` is a placeholder) | `libc.so.6` is built and its symbol table checked, but no dynamically linked program runs on Liella yet; only statically linked programs (`libc.a`) do. This is the largest gap today: most real-world binaries are dynamically linked ([docs/interfaces.md §6](interfaces.md), [docs/toolchain.md §8](toolchain.md)). |
| **Compatibility-only symbol versions** (phase 15) | `sym@OLD_VERSION` entries are not produced, so binaries linked against glibc versions older than a function's current version would not resolve them. Only relevant once the loader exists. |
| **x86-64 Linux only** | No other architecture or OS; the C layer's assembly and the Appendix C layouts are x86-64. Porting would mean a new `csys/src/machine/` and layout set. |
| **One CPU level** | Products are built for baseline x86-64. The string scanners pick AVX2 or SSE2 at run time; other code does not. This affects speed on AVX2 hosts for some rows, not correctness. |
| **Rust `std` programs** | Rust programs on Liella must be `#![no_std]` until `getcwd`, `realpath`, `dl_iterate_phdr` and `_dl_find_object` exist ([docs/toolchain.md §8](toolchain.md)). C programs are unaffected. |
| **Hot kernels** (section 8) | Listed for completeness: two files of SSE2 intrinsics and assembly over ranges Rust validates first, exhaustively tested. Unlikely to have practical impact. |

## 14. Where to start reading

* [docs/codemap/README.md](codemap/README.md): per spec section, the boundary file, the library
  modules, the clause tests, the conformance programs, the decision file and the behaviour
  records. Exported symbols: `grep -P '^strlen\t' docs/codemap/symbols.tsv`.
* [implementation-plan.md](../implementation-plan.md): the plan's index; read only the part
  you need.
* [docs/interfaces.md](interfaces.md): the four ways into the library.
* [docs/toolchain.md](toolchain.md): build details and what cannot work yet.
* [docs/differences-from-glibc.md](differences-from-glibc.md): every deliberate difference.
