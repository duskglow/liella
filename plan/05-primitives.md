<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

## 5. Internal primitives catalogue

| ID | Primitive | Where | First needed (phase) | Notes |
|---|---|---|---|---|
| IP-1 | `Errno`, raw-result decoding, `Error`, `Result` | kernel, core | 1 | Numbering ⚑ KERN. |
| IP-2 | Syscall wrappers `liella_sys_*` and their `ffi` declarations (generated from the header, K5) | csys, boundary | 1 | The only `syscall` instructions in the tree. |
| IP-3 | `Fd`/`OwnedFd`, `KPath` (NUL-terminated in Rust-owned memory, PATH_MAX, no interior NUL) | kernel | 1 | ⚑ KERN H1 is honoured here. |
| IP-4 | auxv and vDSO parsing over slices; the vDSO call primitive | kernel, csys | 1 | |
| IP-5 | `RawMutex`, `RawOnce`, `RawRwLock`, `RawCondvar` on atomics + `Sys::futex`; valid when zero-initialised; pointer-free | rt | 3 | D-22.1. Fits pthread object sizes (Appendix C.6). |
| IP-6 | Page source (`Sys::mmap` only; never `brk`) | rt | 3 | D-3.7. |
| IP-7 | Heap algorithm over addresses (rt); `GlobalAlloc` shim (boundary, category *allocator*); `TryVec`/`TryBox`; the two allocation domains | rt, boundary, core | 3 | D-3.1, F23. |
| IP-8 | Thread blocks: per-thread state reached via `Machine::thread_pointer` and converted to a reference by the boundary | rt, csys, boundary | 4 | FS base ⚑ KERN. |
| IP-9 | Process entry (`machine/entry.S`) and startup sequencing (`rt::start`) | csys, rt | 4 | |
| IP-10 | errno slot, per-thread result slots, process cursors | legacy, boundary::data | 4 | P-scope (C7). |
| IP-11 | `Sys`/`Machine` traits plus the `Faulty` error injector | kernel | 1 | X5. |
| IP-12 | Conversion vocabulary: `boundary::{conv, strz}` (unsafe category *slice*) and `abi::{ret, pack, chk}` (safe) | boundary, abi | 1–2 | §8.3. |
| IP-13 | `export!` macro and exported data objects (atomics-only layouts) | boundary | 1, 4 | F6, F45. |
| IP-14 | Variadic adapters and `va_arg` accessors in C; the typed reader in `abi::va`; `VaSource` in the boundary | csys, abi, boundary | 5, 8 | F11 (resolved in C, OD-C1). |
| IP-15 | Callback adapters: C function pointers typed in export signatures are safe to call | abi | 2 | F12. |
| IP-16 | Signal restorer and handler entry (`machine/restorer.S`), dispatch table (rt) | csys, rt | 5 | ⚑ KERN SA_RESTORER. |
| IP-17 | Cancellation state and the `Canceled` propagation path | rt, core, boundary | 6 | D-22.2. |
| IP-18 | Exact binary↔decimal floating conversion engine | core | 2 | |
| IP-19 | Unicode tables generated from the UCD | core | 7 | |
| IP-20 | Locale data model and loader | core | 7 | Format inspection (Appendix D). |
| IP-21 | Internal line and record reader for configuration files | core | 4 | |
| IP-22 | Typed formatting and parsing engines | core | 8 | |
| IP-23 | Time-zone engine | core | 9 | |
| IP-24 | ELF64 reader over slices | kernel, ld | 1 (vDSO), 14 | |
| IP-25 | NSS engine | core, legacy | 10, 14 | |
| IP-26 | CPU feature decoding (`Machine::cpuid`) and hwcaps build variants | rt, csys, xtask | 2 | P4, OD-C1. |
| IP-27 | Backtrace unwinder (`rt::unwind`): `.eh_frame`/`.eh_frame_hdr` parsing, call-frame programs, DWARF expressions, signal frames (`S`), over read-only object segments the boundary exposes as `'static` slices (`conv::loaded`) and stack words read through `process_vm_readv` (checked reads, no faults) | rt, boundary, csys (restorer/entry call-frame information, link labels) | 8 | D-2.7. |

---
