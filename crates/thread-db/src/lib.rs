//! Debugger support library (spec §22.10). Phase 14. It must describe *our* thread
//! layout, since debuggers read it through the td_* interface (finding F30).

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::disallowed_methods))] // S8: fallible allocation only

extern crate alloc;

pub mod agent;
pub mod thread;
pub mod proc_service;
