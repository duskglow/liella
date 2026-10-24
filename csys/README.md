# csys: the C syscall layer

The fourth artifact, alongside the spec (`glibc-functional-spec.md`), the Rust library
(`crates/`) and the POSIX translation layer (`crates/boundary/src/posix`). It is the only code
in the project that executes a system call or a machine instruction Rust cannot express safely.

## Responsibility

Thin wrappers, no logic, no error handling, no data transformation.

| Class | Contents | Status |
|---|---|---|
| **sys** | One wrapper per kernel system call of spec Appendix A.1: `long liella_sys_<name>(args...)`, returning the raw kernel result (a value, or `-errno`). | Approved (owner decision) |
| **machine** | Instruction-level primitives in assembly that Rust cannot express without `unsafe`: process entry, signal restorer, setjmp/longjmp register save and restore, context switch, thread start on a new stack, thread-pointer read, vDSO call by address. | Approved (owner decision) |
| **machine (additions)** | CPUID/XGETBV, MXCSR and x87 control-word access, PKRU read/write, the explicit-zeroing compiler barrier, calling code at an address (ELF constructors and entry points in `ld.so`). | Approved (OD-C1) |
| **adapter** | C calling-convention adapters: the variadic entry points of variadic exports (`printf(fmt, ...)` → `vprintf(fmt, ap)`), and `va_arg` accessors (`liella_va_i64(va_list *)`, ...). Stable Rust cannot define C-variadic functions or read a `va_list` without `unsafe`. | Approved (OD-C1) |

## Rules (checked by `cargo xtask csys-audit`)

* Every function body is a single statement: one system call, one assembly sequence, or one
  forwarding call. No `if`, `else`, `switch`, loops, `goto`, `?:`, and no `errno`.
* Freestanding: includes only `liella_csys.h` and the compiler-provided `<stddef.h>`,
  `<stdint.h>`, `<stdarg.h>`. No libc headers.
* Assembly (`.S`) only under `src/machine/`.
* Built by `crates/boundary/build.rs` with `-ffreestanding -fno-builtin -Wall -Wextra -Werror`.
* Exported names are prefixed `liella_`, except the variadic adapters, whose names are the
  exported glibc names (compiled under `LIELLA_EXPORT`, off in test builds so tests never
  interpose on the host libc).

## Tests

The C layer has no logic of its own to test. It is exercised through `liella-boundary`'s
`CSys` by the kernel-primitive suite (`crates/boundary/tests/appendix_a_primitives.rs`),
and `crates/boundary/tests/csys_link.rs` proves that it builds and links.
