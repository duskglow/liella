//! C types, glibc ABI layouts (spec Appendix C) and the safe half of the conversion vocabulary used
//! by the POSIX translation layer (implementation-plan.md §8.3).
//!
//! `#![forbid(unsafe_code)]`. Anything that touches raw C memory (pointer-to-slice conversions,
//! C strings, exported symbols, calls into C) is in `liella-boundary`; this crate defines the typed
//! values those conversions produce and the return conventions they feed.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::disallowed_methods))] // S8: fallible allocation only

extern crate alloc;

pub mod ctypes;
pub mod layout;
pub mod layout_init;
pub mod ret;
pub mod pack;
pub mod callback;
pub mod va;
pub mod chk;
