//! The kernel as a value (spec Appendix A, C0.8).
//!
//! This crate defines *what* the kernel provides, as the safe `Sys` and `Machine` traits and the
//! kernel's data types. It performs no system calls itself: the only implementation that reaches
//! the kernel is `liella_boundary::sys::CSys`, which calls the C syscall layer (`csys/`). Every
//! other crate receives the kernel as a generic parameter `S: Sys`, which is zero-cost after
//! monomorphisation and lets tests substitute a fault-injecting kernel (rule X5).
//!
//! `#![forbid(unsafe_code)]`. Syscall numbers, structure layouts and constants follow the Linux
//! UAPI (implementation-plan.md §1.2). Every `Sys` method is sound for every argument safe Rust
//! can build (decision D-X.9); `mem` holds the ownership types that make that possible.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::disallowed_methods))] // S8: fallible allocation only

pub mod errno;
pub mod fd;
pub mod path;
pub mod uapi;
pub mod mem;
pub mod ops;
pub mod sys;
pub mod machine;
pub mod auxv;
pub mod vdso;
pub mod testing;

pub use errno::{Errno, KResult};
pub use fd::Fd;
pub use machine::Machine;
pub use path::{KPathBuf, KStr, KStrBuf};
pub use sys::Sys;
