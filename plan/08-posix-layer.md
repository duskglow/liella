<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

## 8. The POSIX translation layer

### 8.1 Purpose and location

The translation layer lives in `crates/boundary/src/posix`, one module per spec section. It
does exactly two things:

1. **Convert inputs** from C representations to internal values.
2. **Convert outputs** from internal results to C conventions.

All logic, error handling, edge-case management and computation live in the Rust library:
`liella`, `liella-legacy`, `liella-math` and `liella-rt`, with the safe half of the vocabulary in
`liella-abi`. **Any logic found in the translation layer is a design error.**

It lives in the boundary crate because it is C interop. Exporting symbols (category *export*)
and turning C pointers into Rust memory (category *slice*) are two of the four things only the
boundary may do. It is designed in parallel with each phase, so impedance mismatches surface
when the internal API is designed.

### 8.2 Rules

| # | Rule | Check |
|---|---|---|
| T1 | Each export is **one expression**: `ret::…( [conversions] → one library call with CSys )`. No `let`, `if`, `match`, loops, arithmetic, `?`, and no local or static state. | `cargo xtask purity` |
| T2 | **Closed vocabulary.** Inputs are converted by `crate::conv`, `crate::strz` (unsafe, category *slice*) and `liella_abi::{chk, callback, va}` (safe). Outputs are converted by `liella_abi::{ret, pack}`. `Result`/`Option` combinators compose them. | Review |
| T3 | **One library call.** Multi-step sequences live in `liella-legacy`. | Review |
| T4 | `unsafe` appears only as a single `unsafe { conv::… }` or `unsafe { strz::… }` block, each preceded by `// UNSAFE(slice) SAFETY: <which C contract guarantees the pointer>`. | `purity` and `unsafe-audit` |
| T5 | **Aliases share one function.** Different contracts (versions, dialects) are different translations. | Manifest review |
| T6 | Exports go only through `export!` (category *export*), with the version node from Appendix C.3. | Build; `unsafe-audit` |
| T7 | No translation decides *which* library function to call. Multiplexed C entry points are converted into typed values (`FcntlCmd`, `OptBuf`) consumed by one library function. | Review |
| T8 | **Fixed-size C objects are parameters of type `Option<&T>`/`Option<&mut T>`** (D-X.8). They need no conversion and no `unsafe`. Only variable-length memory uses category *slice*. | Review |
| T9 | **Variadic C entry points are C adapters** (`csys/src/adapter`), each forwarding to a fixed-arity Rust export that takes a `va_list` or the decoded fixed arguments (OD-C1). | `csys-audit` |

### 8.3 Conversion vocabulary

| Where | Module | Converts |
|---|---|---|
| boundary (unsafe *slice*) | `conv` | `buf(ptr, n) → Result<&[u8]>`, `buf_mut`, `opt_buf(ptr, n) → OptBuf` (two-mode entry points), `array::<T>(ptr, n)` for POD element types from Appendix C, `path(ptr) → Result<KPath>` (via `strz`), `iov(ptr, n)`. NULL with n>0 is `EFAULT` or a fatal diagnostic, per S6. |
| boundary (unsafe *slice*) | `strz` | NUL-terminated C text into Rust views without reading past the terminator (F1, OD-C2); `strz_n(ptr, n)` for `strn*`; `wcsz(ptr)`. |
| boundary (safe) | `va_source` | `VaSource` over the C layer's `liella_va_*` accessors. |
| boundary (unsafe *export*) | `export`, `data` | the `export!` macro; data objects (F6, F45) |
| boundary (unsafe *extern*) | `caller_alloc` | caller-owned memory through the interposable `malloc` (F23) |
| abi (safe) | `ret` | `errno_neg1`, `errno_null`, `errnum`, `ssize`, `int_len`, `size_or_neg1`, `map_failed`, `eof`, `weof`, `found_ptr(base, Option<index>)` (a `wrapping_add` on the caller's pointer, which is safe), `eai`, `h_errno`, `dlerror`, `wait_status`, `elements(bytes, size)`, `mb_sentinels`, `parse_end` |
| abi (safe) | `pack` | owned results into caller memory that is already a slice or `&mut T`: the `_r` ERANGE protocol, `hostent`/`passwd`/`addrinfo` graphs, `dirent`, `tm`, `termios`, `lconv`, argz, `sockaddr` |
| abi (safe) | `chk`, `callback`, `va`, `layout`, `ctypes` | fortify comparisons; C function pointers (typed in the export signature, so safe to call); the typed `va_list` reader; the Appendix C types |

### 8.4 What translations look like

These show the shape a translation will take. They are not code in the scaffold.

```rust
// §11.3 read: one conversion (category slice), one call with the kernel as a value.
export! { "GLIBC_2.2.5";
pub extern "C" fn read(fd: c_int, buf: *mut c_void, n: size_t) -> ssize_t {
    // UNSAFE(slice) SAFETY: read's contract (§11.3) makes the caller guarantee n writable bytes at buf.
    ret::ssize(unsafe { conv::buf_mut(buf, n) }.and_then(|b| liella::io::read(&CSys, conv::fd(fd), b)))
}}

// §13.6 fstat: a fixed-size object needs no unsafe at all (T8).
export! { "GLIBC_2.33";
pub extern "C" fn fstat(fd: c_int, st: Option<&mut stat>) -> c_int {
    ret::errno_neg1(liella::fs::stat_fd(&CSys, conv::fd(fd)).and_then(|s| pack::stat(st, s)))
}}

// §14.1 qsort: the comparator is a typed C function pointer, safe to call; the array is one slice.
export! { "GLIBC_2.2.5";
pub extern "C" fn qsort(base: *mut c_void, n: size_t, size: size_t,
                        cmp: Option<extern "C" fn(*const c_void, *const c_void) -> c_int>) {
    // UNSAFE(slice) SAFETY: qsort's contract (§14.1) makes the caller guarantee n*size bytes at base.
    ret::unit(unsafe { conv::elements_mut(base, n, size) }.map(|a| liella::search::sort_stable(a, size, callback::cmp(cmp))))
}}

// §12.6 printf: the variadic entry is a C adapter (csys/src/adapter/variadic.c):
//     int printf(const char *fmt, ...) { va_list ap; va_start(ap, fmt); int r = vprintf(fmt, ap); va_end(ap); return r; }
export! { "GLIBC_2.2.5";
pub extern "C" fn vprintf(fmt: *const c_char, ap: *mut VaListTag) -> c_int {
    // UNSAFE(slice) SAFETY: printf's contract (§12.6) requires fmt to be a NUL-terminated string.
    ret::int_len(unsafe { strz::strz(fmt) }.and_then(|f| legacy::printf_va::print_to(&CSys, legacy::stdio_compat::stdout(), f, va_source::of(ap))))
}}
```

### 8.5 Findings: where translation is lossy or costs performance

*Lossy* means information or capability the internal API has but the C interface cannot carry,
or behaviour the C interface requires that the internal design would not choose. *Cost* is the
runtime price of translating. Findings F44–F50 are new with the C-layer architecture.

| ID | Where | Mismatch | Lossy? | Cost | Resolution |
|---|---|---|---|---|---|
| F1 | All `str*`/`wcs*` inputs | C text has no length, and **Rust forbids reading past the object end**. So the word-at-a-time over-read that makes C string scanning fast is unavailable, even in the boundary. | No | A byte-granular scan up to the terminator, then slice operations. Expected to be slower than glibc's assembly on long strings (to be measured in phase 2). | OD-C2: accept, or permit assembly string-scanning primitives in the C layer. |
| F2 | Variable-length pointer inputs | The caller's buffer extent is unknowable. | **Yes**: safety ends at the boundary | None | Irreducible for a C ABI. `_chk` forms recover the real bound (D-C0.5). S6 defines outcomes for detectable violations. |
| F3 | Every error return | `Result` becomes sentinel plus errno. | No | One TLS store on failure only | X3. |
| F4 | `strtol`, `readdir`, `getpwnam`, `mktime`, `nice`, `getauxval`, `sysconf`, `ptrace(PEEK)`, `inet_addr`, `strftime` | Distinct internal outcomes collapse into one C sentinel. | **Yes** | None | Fixed by the C ABI. The distinction exists internally. |
| F5 | ≈70 static-result functions | Need library-owned result storage. | No | Per-thread slot memory | `legacy::result_slots`. |
| F6 | Exported mutable data (`optind`, `environ`, `stdin`, `tzname`, `error_*`, `signgam`, `_res`, ...) | Callers read and write library state directly. | **Yes**: no synchronisation, no validation | Re-read on every use | `boundary::data` storage, authoritative for `legacy`. |
| F7 | `environ` | Callers may replace or edit the array. | **Yes**: O(n) lookup | O(n) `getenv` | `legacy::environ` scans the live array. |
| F8 | Locale-sensitive functions | The implicit current locale must be fetched. | No | One TLS read | |
| F9 | `wchar_t` | It can hold values that are not Unicode scalar values. | No | None | `u32` internally. |
| F10 | `FILE` | Header-inline code touches the buffer windows (Appendix C.9). | **Yes**: stream internals are ABI-visible | The window lives in memory C code writes, so the library reads it through the boundary each call. | `abi::layout::FILE` plus the `legacy::stdio_compat` window protocol. |
| F11 | Variadic exports (printf/scanf families, `open`, `fcntl`, `ioctl`, `prctl`, `execl*`, `syscall`, `semctl`, `mremap`, `ulimit`, `makecontext`, `mq_open`, `sem_open`, `error`, `err`, `syslog`, `argp_*`) | Rust cannot define C-variadic functions or read a `va_list` without `unsafe`. | No | One extra call (the C adapter forwards to the Rust export), plus one C call per `va_arg` read. | C adapters (OD-C1). Format- or command-directed decoding in `legacy`. |
| F12 | C callbacks called by the library | A foreign (C++) exception thrown from a callback unwinds through Rust frames. Since OD-6's revision (2026-10-21) it reaches the caller as in glibc for the search, sort, tree-walk, `ftw`/`nftw`, `scandir` and `pthread_once` callbacks (D-14.6, S11); `glob`'s callbacks still abort (no undo yet). | **Yes, for `glob`** | None when no exception is thrown | OD-6, D-14.6, S11. `pthread_once`/`call_once` routines run in machine/once.S; state held across other callbacks is undone by machine/guard.S. Cancellation never unwinds Rust frames (D-22.2). |
| F13 | `_r` functions and record results | Owned records serialised into C layouts, some with the `ERANGE` retry protocol. | No | A small copy per result | `abi::pack`. |
| F14 | Names with two contracts | Selected at compile time by macros, and by name at link time. | No | None | Separate exports (D-H7). |
| F15 | Symbol versions | 262 multi-version names, 545 compatibility-only (Appendix C.3). | No | None | Version script from `versions.tsv`; old contracts in `legacy::versioned`. |
| F16 | `setuid` family in threaded processes (⚑ KERN) | POSIX process scope over per-thread credentials. | No | O(threads) | D-17.1. |
| F17 | `SIGRTMIN` | The internal-signal reservation is visible to applications. | **Yes** (one real-time signal fewer) | None | One reservation (D-20.1, OD-8). |
| F18 | `select`/`fd_set` | 1,024-descriptor cap (Appendix C.6: 128 bytes). | **Yes** | None | Kept at the boundary. |
| F19 | `sigset_t` (128 bytes) vs the kernel (8) | Size conversion. | No | An 8-byte copy | |
| F20 | `timeval`/`time_t`/`clock_t`/ticks | Coarser resolutions. | **Yes** (precision) | One division | |
| F21 | `int` length results | `usize` internally. | **Yes** above `INT_MAX` | One compare | `ret::int_len`. |
| F22 | `pthread_*_t`, `sem_t` | ABI sizes and all-zero initializers (Appendix C.6/C.8; for example `PTHREAD_RECURSIVE_MUTEX_INITIALIZER_NP` sets byte 16 to 1). | No | None | D-22.1: primitives designed to these bit patterns. |
| F23 | Memory returned to callers | Must be freeable by a possibly interposed `free`. | No | One indirect call per caller-owned allocation | `boundary::caller_alloc`. |
| F24 | `vfork` | RESOLVED (VFORK-CLONE-VM, owner #410): `SYS_vfork` shares the memory, as glibc. Was a copy-on-write child: 27.6 ms per call at 1 GB (glibc 0.42 ms, sumire) and one minor fault per touched page afterwards (SPAWN-LARGE-PARENT). | No | None now | D-18.1. |
| F25 | Cancellation points | A per-thread state check at each one. | No | One load | D-22.2. |
| F26 | `fork` | Library locks must be handled around it. | No | Proportional to subsystems | atfork plumbing in `rt`. |
| F27 | NSS plugin modules | A reverse ABI into third-party modules. The module interface is inspected under the format decision (Appendix D, phase 14). | Depends on inspection | A `dlopen` per source | Phase 14. |
| F28 | iconv gconv modules | The module ABI is inspected under the format decision. Loading glibc's modules may still be impossible if they depend on glibc-private symbols (Appendix C excludes `GLIBC_PRIVATE`). | Likely **yes** | None | Built-in converters first (D-6.4). Decide after inspection in phase 7. |
| F29 | Locales | The archive format is inspected (Appendix D, phase 7). | No, once inspected | Archive `mmap` plus parsing | D-7.2. |
| F30 | `libthread_db` | Debuggers read our thread layout. | No, if shipped | None | Our own `libthread_db`. |
| F31 | Dynamically linked binaries | Need our `ld.so`. | No | None | Phase 14. |
| F32 | `_IO_*` vtable objects | Old libstdc++ subclassed `FILE`. | **Yes** (subclassing unsupported) | None | D-29.1. |
| F33 | `sys_errlist`/`sys_siglist` | The array extents are baked into old binaries. | No: sizes known (Appendix C.5: 1000, 1008, 1056 and 1080 bytes by version) | None | Data objects of exactly those sizes. |
| F34 | `mallinfo`/`malloc_info`/`mallopt` | They expose implementation concepts. | **Yes** (neutral values) | None | D-3.4. |
| F35 | `qsort` | Stable is chosen. | No | A buffer, or an in-place fallback | D-14.1. |
| F36 | `%n` in fortified printf | "Writable format" needs segment knowledge. | No | One lookup per `%n` | D-12.4. |
| F37 | `siglongjmp` out of a handler that interrupted library code | Skipping Rust frames with pending destructors. | No (POSIX already restricts this to AS-safe code) | None | D-21.1. |
| F38 | `utmp`/`wtmp` | 32-bit `ut_tv` (Y2038). | **Yes** | None | Layout kept (Appendix C.7, owner 2026-10-04); the seconds are unsigned, correct to 2106-02-07 06:28:15 UTC with pre-2038 bytes unchanged (owner ruling 2026-10-22, #415); a time outside `[0, 2^32)` is written as no time, `(0, 0)` (D-17.5). |
| F39 | Asynchronous cancellation | Cannot act inside library code. | **Yes** (latency) | None | D-22.2. |
| F40 | nscd | Protocol to be inspected. | Depends | Direct lookups meanwhile | D-16.5. |
| F41 | `getopt` | Permutes the caller's `argv`. | No | O(n) | `legacy::getopt`. |
| F42 | ctype inline macros | Index `*__ctype_b_loc()` directly (Appendix C.9). | **Yes** (table layout frozen) | None | D-5.3. |
| F43 | `stdin`/`stdout`/`stderr` are assignable | Re-read on each use. | No | One load | F6. |
| F44 | **Every kernel operation** | It goes Rust → `ffi` → C wrapper → `syscall`, not an inline `syscall` instruction. | No | One direct call, plus a wrapper that the compiler reduces to a few instructions. Removable with cross-language LTO (OD-C3). | Measure in phase 0. With LTO the wrappers inline into Rust. |
| F45 | Exported data objects | Must be expressed without `static mut` or `unsafe impl Sync`: `#[repr(C)]` structs of atomics with the Appendix C.5 sizes. C code accesses them non-atomically. | No (same memory) | None; relaxed atomics compile to plain loads and stores on x86_64 | `boundary::data`. The mixed atomic and non-atomic access is the same data race the C contract already has. |
| F46 | Runtime CPU dispatch | Calling `#[target_feature]` code from generic code needs `unsafe`. | No | Static binaries run the baseline build. Dynamic ones get per-level builds through hwcaps. | P4. |
| F47 | Thread-local state | Reached through `Machine::thread_pointer` (a C primitive), then converted to a reference (category *slice*) per access. No `#[thread_local]` (unstable). | No | One C call per TLS access, unless inlined by LTO (OD-C3) | Cache the thread-block reference per call chain, and pass it explicitly (C2). |
| F48 | Heap metadata | The safe heap algorithm handles addresses as integers. Metadata regions reach it as slices converted once per region. | No | None | IP-7. |
| F49 | Soundness of `safe` foreign declarations with pointer parameters | Encapsulation argument (private declarations, slice-typed wrappers) rather than a compiler guarantee. | No | None | S4; review of `ffi.rs` in every phase. |
| F50 | Typed C function-pointer parameters (`Option<extern "C" fn>`) | Calling them is safe Rust, because the export signature carries the trust (category *export*). A caller passing a mistyped pointer is a C contract violation. | No | None | T8; D-X.8. |
| F51 | `__pthread_unwind_buf_t` | glibc gives the typedef `__attribute__((aligned))`: 104 bytes, alignment 16. A Rust type's size must be a multiple of its alignment. | No (the object is C-allocated) | None | Liella declares it 8-aligned; the Appendix C test checks the size and documents the exception. Objects of this type are only reached through pointers from C (`pthread_cleanup_push` frames). |
| F52 | Kernel calls that write through "input" pointers | `pselect6` and `ppoll` write the remaining time back into the timeout, as does `recvmmsg`; `capset` writes the preferred version into the header. A `const` C prototype would hide this. | No | None | `Sys` takes `&mut` for each (D-X.9 refinement c); found by reading the kernel's ABI while typing the syscall table. |
| F53 | `semctl(GETALL/SETALL)` | The array length is the set's `sem_nsems`, which the call does not take. | No | One extra `IPC_STAT` call | `CSys` reads `sem_nsems` first and refuses a short array. Residual: a set deleted and its ID reused between the two calls (IDs carry a sequence number, so this needs about 32,768 recycles). The phase-1 tests caught a wrong `sem_nsems` word index here before it shipped. |
| F54 | Raw fork in a multithreaded process | Without `atfork` handlers the child inherits locks other threads held. | No | None | `selftest` children do not allocate. The real `fork()` (phase 6) must reset the allocator and stdio locks in the child, as the spec requires (§18.1). |
| F55 | Kernel version and configuration dependence | On this suite's reference kernel (WSL2 5.15): `fchmodat2`, `map_shadow_stack`, `statmount`, `listmount` are `ENOSYS`; `ioperm`/`iopl` are `ENOSYS` (built without `CONFIG_X86_IOPL_IOPERM`); `RWF_NOWAIT` on pipes is `EOPNOTSUPP`. | No | A fallback per call | Tests accept exactly these outcomes and say so. Library code must fall back (`fchmodat` with `AT_SYMLINK_NOFOLLOW` through `O_PATH` and `/proc/self/fd`, per spec §13) where the newer call is missing. |
| F56 | Floating-point environment changes | `Machine::set_mxcsr` and `set_x87_control` change modes that LLVM assumes are the default (round to nearest, no traps); compiled Rust float code does not observe them reliably. | No | None | The `fenv` functions (track M) must perform mode-dependent operations through the C layer or in code built to respect the dynamic environment, not through ordinary Rust float arithmetic. |
| F57 | Order of kernel checks | Several calls check privilege before arguments (`reboot`, `syslog`, `setgroups`, `quotactl` resolves the device first). Which error a bad call returns is kernel ABI. | No | None | Translations must not pre-validate in a different order, or errno would differ from glibc's. |

**Summary.** The architecture change adds two O(1) costs:

* one call into the C layer per kernel operation (F44);
* one per TLS access (F47).

Cross-language LTO can remove both (OD-C3). It also makes one cost visible that was
previously hidden in `unsafe` code: C string scanning without over-reads (F1, OD-C2). The rest
of the findings are unchanged.

### 8.6 Enforcement

* `cargo xtask purity` checks the translation layer: keywords, operators, `?`, the
  `unsafe { conv:: | strz:: }` shape, and the `UNSAFE(slice)` comment on each such block.
* `cargo xtask unsafe-audit` checks:
  * `#![forbid(unsafe_code)]` in every crate except the boundary, and no `unsafe` anywhere
    else, including tests and build scripts;
  * `#![deny(unsafe_code)]` in the boundary;
  * that every site is one of the four categories, in a permitted file, documented with
    `UNSAFE(<category>)`.

  It prints per-category counts. Verified: it rejects an undocumented boundary site, an
  undocumented `#[allow(unsafe_code)]`, and `unsafe` in `liella`. The compiler also rejects
  the last.
* `cargo xtask csys-audit` checks the C layer: single-statement bodies, no control flow or
  `errno`, the freestanding includes, and assembly only under `machine/`.
* The review checklist for each translation PR:
  1. Manifest row and versions (Appendix C.3).
  2. Translation test specified.
  3. New vocabulary is tested in `crates/abi/tests` (safe) or `crates/boundary/tests`
     (unsafe *slice*).
  4. The findings table is updated.

---
