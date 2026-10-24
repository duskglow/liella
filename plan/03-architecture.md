<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

## 3. Architecture

### 3.1 Runtime layering

```
                    C programs / existing glibc-linked binaries
                                   │  C ABI, symbol versions (Appendix C.3)
┌──────────────────────────────────▼──────────────────────────────────┐
│ liella-boundary::posix  POSIX translation layer: 3,943 exports, one   │
│   expression each: conv → call → ret        unsafe: export, slice    │
│ liella-boundary::{conv,strz,data,heap,export,va_source}               │
└──────────┬───────────────────────────────────────────────────┬──────┘
           │ calls the clean library with CSys                 │ GlobalAlloc
┌──────────▼──────────┐ ┌────────────────┐ ┌────────────────┐ ┌▼───────────────┐
│ liella-legacy        │ │ liella (core)   │ │ liella-math     │ │ liella-rt       │
│ quarantined HIST    │ │ clean API      │ │ generic floats │ │ startup, heap, │
│ semantics           │ │                │ │                │ │ sync, threads, │
└──────────┬──────────┘ └───────┬────────┘ └───────┬────────┘ │ signals        │
           │     all generic over S: Sys + Machine, all #![forbid(unsafe_code)]
┌──────────▼──────────────────────────────────────────────────────────────────┐
│ liella-kernel   the Sys and Machine traits, kernel types (Fd, KPath, Errno,  │
│                uapi structs), auxv/vDSO parsing over slices   [forbid]      │
│ liella-abi      C types, Appendix C layouts, ret, pack, va reader  [forbid]  │
└──────────┬──────────────────────────────────────────────────────────────────┘
           │ implemented by
┌──────────▼──────────────────────────────────────────────────────────────────┐
│ liella-boundary::sys (CSys) → liella-boundary::ffi        unsafe: extern      │
└──────────┬──────────────────────────────────────────────────────────────────┘
┌──────────▼──────────────────────────────────────────────────────────────────┐
│ csys/  C syscall layer: liella_sys_* wrappers, machine/*.S, adapters  (C/asm)│
└──────────┬──────────────────────────────────────────────────────────────────┘
           ▼  Linux kernel (syscall, vDSO, auxv, signal frames)
```

**The crate graph has no cycle.** At runtime `liella-boundary` is both the top (exports) and
the bottom (the `Sys` implementation), but in Cargo terms it depends on every library crate and
nothing depends on it. The clean crates reach the kernel only through the `Sys`/`Machine`
traits they are generic over (X5).

**`no_std` everywhere.** Rust `std` links a C library, and this project *is* the C library.
All library crates are `#![no_std]` + `alloc`. The global allocator is
`liella_boundary::heap` (category *allocator*), which delegates every decision to the safe
algorithm in `liella_rt::heap`. The panic handler is in `liella_rt::panic` (a safe attribute).

**Test builds never interpose.** Rust exports carry `#[unsafe(no_mangle)]` only under feature
`export`, and C adapters carry glibc names only under `LIELLA_EXPORT`. Only the products enable
them.

### 3.2 Where each unsafe category may appear

| Category | Files | Examples |
|---|---|---|
| extern | `boundary/src/ffi.rs`, `boundary/src/caller_alloc.rs` | `unsafe extern "C" { safe fn liella_sys_read(...); }`, the interposable `malloc` |
| export | `boundary/src/export.rs` (the `export!` macro), `boundary/src/data.rs` | `#[unsafe(no_mangle)]` on each export, data objects |
| slice | `boundary/src/conv.rs`, `boundary/src/strz.rs`, `boundary/src/heap.rs`, `boundary/src/tb.rs` (the thread block at the thread pointer, IP-8), `boundary/src/runtime.rs` (the kernel's initial stack, program headers and the initial thread's TLS, IP-9; products only), `boundary/src/posix/*` and `boundary/src/native/*` (single conversions only; the native C ABI, D-X.10) | `slice::from_raw_parts(ptr, len)` for a C caller's buffer, heap metadata regions |
| allocator | `boundary/src/heap.rs` | `unsafe impl GlobalAlloc` |
| native | `native/src/ffi.rs` only (the facade `liella-native`, a library for programs, D-X.10; its root is `#![deny(unsafe_code)]`, every other file of the crate `#![forbid(unsafe_code)]`) | `unsafe extern "C" { safe fn liella_fd_read(...); }` (pointer items private, wrapped by slice-taking functions), `unsafe impl GlobalAlloc` over `liella_heap_*`, the `entry!` macro's `#[unsafe(no_mangle)]` C `main`; for the Rust dylib (feature `dylib`), the exported `#[unsafe(no_mangle)]` string `LIELLA_RUST_ABI` and the one named section the audit accepts, `#[unsafe(link_section = ".note.liella.rustabi")]` (the Rust ABI version's note) |

`cargo xtask unsafe-audit` prints the count per category, so growth of the unsafe surface is
visible in every run. Phase 0 has one site (category *extern*: the link-proof declaration).

### 3.3 State ownership

| State | Owner | Examples |
|---|---|---|
| Kernel-held, process-wide | The kernel, never cached | cwd, umask, dispositions, limits, credentials (per thread) |
| Contract-visible library state | `liella-legacy` logic, storage in `liella-boundary::data` when exported | errno slot, `environ`, global locale, `getopt` data symbols, `*ent` cursors, generator seeds, `tzname`, printf registry, `_res`, `h_errno` |
| Clean library state | Explicit values in `liella` | `Locale`, `Zone`, `Env`, `Stream`, `Resolver`, `OptParser`, `Rng48`, ... |
| Runtime state | `liella-rt`, in memory the boundary converts to slices | thread blocks, allocator metadata, signal table, CPU features |

---
