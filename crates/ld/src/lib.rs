//! Dynamic linking (spec §27). Phase 14. Also provides symbol-version resolution for the
//! compatibility contracts of C0.6.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::disallowed_methods))] // S8: fallible allocation only

extern crate alloc;

pub mod elf;
pub mod map;
pub mod search;
pub mod reloc;
pub mod symbols;
pub mod tls;
pub mod namespaces;
pub mod audit;
pub mod debug;
pub mod api;
