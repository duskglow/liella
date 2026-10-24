//! Runtime logic: startup, threads, synchronisation, the allocator algorithm, signal dispatch,
//! contexts, FPU and CPU features.
//!
//! Everything here is safe Rust (`#![forbid(unsafe_code)]`). The operations that need the machine
//! or raw memory are primitives of the C syscall layer (`csys/`), reached through the
//! `liella_kernel::{Sys, Machine}` traits that `liella-boundary` implements. Raw memory this
//! runtime must manage (heap metadata, thread blocks) arrives as Rust slices converted once, by the
//! boundary, under category *slice* of the unsafe policy (implementation-plan.md §2.1).

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::disallowed_methods))] // S8: fallible allocation only

extern crate alloc;

pub mod start;
pub mod tls;
pub mod futex;
pub mod sync;
pub mod heap;
pub mod thread;
pub mod pthread_sync;
pub mod signal;
pub mod context;
pub mod fpu;
pub mod cpu;
pub mod unwind;
pub mod fatal;
pub mod panic;
