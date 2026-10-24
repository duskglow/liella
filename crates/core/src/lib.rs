//! The clean internal library. It implements every functional contract of
//! `glibc-functional-spec.md` once, in the shape the *need* requires rather than the shape the C
//! interface happened to take (implementation-plan.md §2, §7).
//!
//! Crate-wide rules:
//! * `#![forbid(unsafe_code)]`. The kernel arrives as a generic `S: liella_kernel::Sys` value;
//!   the only unsafe code in the project is in `liella-boundary`, and the machine is reached
//!   through the C syscall layer (`csys/`).
//! * One error model: `Result<T, Error>`. No errno, no sentinels, no out-of-band channels.
//! * No hidden global state. Locale, time zone, environment, cursors and seeds are explicit
//!   values passed by the caller (the POSIX layer supplies the process-wide ones).
//! * Text is `&[u8]` with a known length; NUL termination appears only at the kernel boundary.
//! * All allocation is fallible (`mem::TryVec`, `mem::TryBox`); allocation failure is `ENOMEM`.
//! * The library never panics on any input. A panic is a bug and aborts.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::disallowed_methods))] // S8: fallible allocation only

extern crate alloc;

pub mod error;
pub mod mem;
pub mod env;
pub mod getopt;
pub mod argp;
pub mod diag;
pub mod bytes;
pub mod charmap;
pub mod ctype;
pub mod unicode;
pub mod text;
pub mod iconv;
pub mod locale;
pub mod catalog;
pub mod num;
pub mod random;
pub mod io;
pub mod stream;
pub mod printf;
pub mod scanf;
pub mod monetary;
pub mod fs;
pub mod search;
pub mod pattern;
pub mod net;
pub mod names;
pub mod users;
pub mod process;
pub mod term;
pub mod signal;
pub mod context;
pub mod thread;
pub mod time;
pub mod system;
pub mod ipc;
pub mod log;
pub mod dl;
pub mod prof;
pub mod config;
