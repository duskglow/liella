//! The boundary: the only crate in the project that may contain `unsafe` Rust, and only in
//! four categories (implementation-plan.md §2.1):
//!
//! | Category    | What                                                        | Where           |
//! |-------------|-------------------------------------------------------------|-----------------|
//! | `extern`    | declaring the C syscall layer's functions                   | `ffi`, `caller_alloc` |
//! | `export`    | exporting C symbols (`#[unsafe(no_mangle)]`)                | `export`, `data`, `posix`, `native` |
//! | `slice`     | converting C pointers (and lengths) into Rust slices/refs   | `conv`, `strz`, `heap`, `posix`, `native` |
//! | `allocator` | `unsafe impl GlobalAlloc`                                   | `heap`          |
//!
//! The crate is `#![deny(unsafe_code)]`. Every permitted site carries `#[allow(unsafe_code)]`
//! directly after a comment `// UNSAFE(<category>): <why this cannot be avoided>`.
//! `cargo xtask unsafe-audit` rejects any other form, any other category, and any `unsafe`
//! outside this crate. The C syscall layer itself lives in `csys/` and is compiled by
//! `build.rs`.
//!
//! At runtime this crate is both ends of the stack: C callers enter through `posix` (or, for
//! programs not written against POSIX's conventions, through the native C ABI, `native`,
//! D-X.10), the clean
//! library runs generic over `liella_kernel::Sys`, and `sys::CSys` reaches the kernel through
//! `ffi`. In Cargo terms it depends on everything and nothing depends on it, so there is no
//! cycle.

#![cfg_attr(not(test), no_std)]
#![deny(unsafe_code)]
// The products define and declare the C runtime's own symbols (`malloc`, `free`, `exit`, ...):
// the declarations add only a `safe` qualifier and the definitions are the C signatures, which
// Rust 1.99's `suspicious_runtime_symbol_definitions` flags. Older toolchains do not know it.
#![allow(unknown_lints, suspicious_runtime_symbol_definitions)]
#![cfg_attr(not(test), deny(clippy::disallowed_methods))] // S8: fallible allocation only

extern crate alloc;

pub mod ffi;
mod ffi_sys;
pub mod sys;
mod nr;
mod raw;
#[cfg(feature = "selftest")]
pub mod selftest;
pub mod conv;
pub mod hot;
mod dispatch;
pub mod errno;
pub mod ret;
pub mod mathret;
pub mod strz;
pub mod heap;
pub mod cpu;
mod diag;
pub mod tb;
pub mod process;
pub mod startup_names;
pub mod thread;
pub mod pthread;
pub mod cancel;
pub mod context;
pub mod sync;
pub mod setxid;
pub mod nscache;
pub mod locale;
pub mod regex;
pub mod patterns;
pub mod iconv;
pub mod catalog;
pub mod stdio;
pub mod dirtree;
pub mod printf;
pub mod scanf;
pub mod report;
pub mod options;
pub mod argp;
pub mod fmtmsg;
pub mod backtrace;
pub mod monetary;
pub mod fork;
pub mod spawn;
pub mod timer;
pub mod zone;
pub mod netaddr;
pub mod users;
pub mod mntent;
pub mod terminal;
pub mod netdb;
pub mod resolv;
pub mod gai_async;
pub mod timefmt;
pub mod mqueue;
pub mod syslog;
pub mod aio;
mod stacks;
#[cfg(feature = "runtime")]
pub mod runtime;
pub mod export;
pub mod data;
pub mod caller_alloc;
pub mod va_source;
pub mod posix;
pub mod native;
