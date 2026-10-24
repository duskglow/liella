//! Historical semantics that the exported ABI obliges us to reproduce but that the clean
//! design rejects: hidden cursors, static result slots, process-global ambient state, superseded
//! versioned contracts, and compatibility-only subsystems (spec H3, H4, H7, H8, §29).
//!
//! Why a separate crate: the POSIX translation layer must contain no logic
//! (implementation-plan.md §8), and `liella` (core) must carry no historical artefacts. This crate
//! is where such logic is allowed to live, visibly quarantined. `liella` never depends on it.
//!
//! `#![forbid(unsafe_code)]`.

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::disallowed_methods))] // S8: fallible allocation only

extern crate alloc;

pub mod lock;
pub mod errno_slot;
pub mod result_slots;
pub mod cursors;
pub mod env_table;
pub mod environ;
pub mod exit_handlers;
pub mod io_compat;
pub mod fs_compat;
pub mod fs_tree;
pub mod fts;
pub mod ipc_compat;
pub mod net_compat;
pub mod inet_compat;
pub mod rcmd;
pub mod interim_db;
pub mod nss_files;
pub mod gai;
pub mod accounting;
pub mod cred_compat;
pub mod clock_compat;
pub mod sys_compat;
pub mod sysconf_table;
pub mod argp;
pub mod global_locale;
pub mod intl;
pub mod recheck;
pub mod string_compat;
pub mod cstring;
pub mod ctype_tables;
pub mod mb_state;
pub mod mb_compat;
pub mod num_compat;
pub mod prng_global;
pub mod prng_r;
pub mod stdio_compat;
pub mod printf_va;
pub mod scanf_va;
pub mod printf_registry;
pub mod time_compat;
pub mod getdate;
pub mod netdb_compat;
pub mod resolv_compat;
pub mod nss;
pub mod nscache;
pub mod signal_compat;
pub mod spawn_compat;
pub mod versioned;
pub mod malloc_compat;
pub mod malloc_debug;
pub mod pthread_compat;
pub mod obstack;
pub mod search_global;
pub mod search_c;
pub mod regex_compat;
pub mod des;
pub mod sunrpc;
pub mod sunrpc_clnt;
pub mod sunrpc_rec;
pub mod sunrpc_svc;
pub mod nis;
pub mod stubs;
pub mod term_compat;
pub mod log_compat;
