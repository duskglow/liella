<!-- Part of the Liella implementation plan; index and reading guide: implementation-plan.md -->

## 10. Risks

| Risk | Impact | Mitigation |
|---|---|---|
| Call overhead into the C layer (F44, F47) | Without LTO: +0.5 ns per thread-pointer read and one call per system call | Eliminated with LTO in release products (OD-C3). Pass the thread block explicitly in any case. |
| String scanning without over-reads (F1) | `strlen`-class throughput on long strings | OD-C2 with benchmark data. |
| Soundness argument for `safe` foreign declarations (F49) | A mistake in `ffi.rs` would be unsound | `ffi.rs` generated from the header (K5). The pointer-taking items are private. `ffi.rs` is reviewed in every phase. |
| Scale: 4,789 tests before implementation | Throughput | Generated stubs; property and exhaustive tests for families. |
| `ld.so` complexity | Phase 14 schedule | ELF reader from phase 1; test objects built by the system linker. |
| Foreign unwinding (OD-6) | Since 2026-10-21 C++ exceptions from search/sort/walk/`ftw`/`scandir` callbacks and `pthread_once` routines reach the caller as in glibc (S11, D-14.6); the reliance on unwinding through Rust frames without landing pads is checked on every product build (`cxx_throw_products`). `glob` callbacks still abort. | Spike in phase 2; `glob` undo open (D-14.6 gaps). |
| Security: the library is in every process | High | Fuzz every parser. S6 defined outcomes. `unsafe` confined to one audited crate and a logic-free C layer. |

---
