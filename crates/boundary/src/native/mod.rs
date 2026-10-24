//! The native C ABI (D-X.10): the stateful services of the library for programs that are not
//! written against POSIX's conventions, Rust programs first. It sits beside the POSIX
//! translation layer (`crate::posix`), reaches the same single instance of every piece of
//! process state, and follows the same discipline: each export is one expression that converts
//! its inputs, calls one function of the clean library or of the boundary's glue, and converts
//! the result. `cargo xtask purity` checks these bodies as it checks the POSIX layer's.
//!
//! # Conventions (stable; every later node follows them)
//!
//! * **Names.** `liella_<area>_<name>`, areas `heap`, `secure`, `env`, `fd`, `fs`, `clock`, `nscache`,
//!   `tz`, `locale`, `process`, `aio`, `getpass`. The C syscall layer's internal `liella_sys_*`, `liella_mach_*`,
//!   `liella_va_*` and the `LIELLA_PRIVATE` adapters' targets are not part of this interface:
//!   the interface is exactly the rows of `spec/abi/native.tsv`.
//! * **Versions.** Every symbol belongs to one version node, `LIELLA_NATIVE_1`,
//!   `LIELLA_NATIVE_2`, ... in `libc.so.6` (and is defined in `libc.a`). A published node never
//!   changes: no symbol is removed, retyped or given new semantics; additions go in a new node.
//! * **Types.** Fixed-width integers only (`int32_t`, `uint32_t`, `int64_t`, `uint64_t`), with
//!   `size_t` for lengths and indexes. Descriptors are `int32_t`; open flags, modes and
//!   `whence` are `uint32_t` with the kernel's values (constants in the header).
//! * **Byte slices** are a pointer and a length, never NUL-terminated. NULL with length 0 is
//!   the empty slice; NULL with a non-zero length is `-EFAULT`. A slice the kernel needs
//!   terminated (a path) must not contain a NUL (`-EINVAL`) and must be shorter than
//!   `PATH_MAX` (`-ENAMETOOLONG`); text stored for later (an environment entry) must not
//!   contain a NUL either (`-EINVAL`).
//! * **Results** are an `int64_t`: the value (never negative) on success, the kernel's error
//!   number negated on failure. `errno` is never read or written. A function with nothing to
//!   report returns 0.
//! * **Text output** goes into a caller buffer (pointer and length): the result is the full
//!   length of the text, and the text is copied only if it fits whole. A result greater than
//!   the buffer's length means "call again with this many bytes"; nothing partial is delivered.
//! * **Structures** are written through explicit out-parameters (`struct liella_* *`, NULL is
//!   `-EFAULT`), are `#[repr(C)]` with natural alignment and no implicit padding, and have their
//!   size, alignment and every field offset asserted in the header and in the layout tests.
//! * **Handles.** Library objects a later node exposes are opaque handles (`struct liella_x *`
//!   never dereferenced by the caller); v1 needs none.
//! * **The heap** is the one exception to the result convention: its functions mirror Rust's
//!   `GlobalAlloc` (an address, or NULL when no block can be had: exhaustion, or an alignment
//!   that is not a power of two). Misusing a block (an invalid or double free) is a diagnostic
//!   and abort, as for `free`.
//! * **No cancellation points.** The native calls never act on `pthread_cancel`.
//! * **Exit** (`liella_process_exit`) runs the C exit handlers and flushes the C streams, as
//!   `exit`.
//!
//! The table `spec/abi/native.tsv` lists every symbol with its node, C prototype and
//! documentation, every structure field with its offset, and every constant;
//! `cargo xtask native-gen` generates `include/liella/native.h` from it and, with `--check`,
//! fails if the header is stale or if the exports here differ from the table in name or
//! signature. `cargo xtask so` places the symbols in their nodes and checks the products.

pub mod consts;
pub mod types;
mod abi;

pub mod heap;
pub mod secure;
pub mod env;
pub mod fd;
pub mod fs;
pub mod clock;
pub mod tz;
pub mod locale;
pub mod process;
pub mod nscache;
pub mod aio;
pub mod getpass;
