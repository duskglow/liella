//! The POSIX translation layer (implementation-plan.md §8).
//!
//! Each exported symbol is one expression: convert the inputs (`crate::conv`, `crate::strz`,
//! `liella_abi`), call one function of `liella`, `liella-legacy` or `liella-math` with `CSys`,
//! convert the result (`liella_abi::ret`, `liella_abi::pack`). Branches, loops, arithmetic,
//! local state or error decisions here are design errors; `cargo xtask purity` rejects them.
//!
//! `unsafe` appears here only as the single `unsafe { conv::... }` or `unsafe { strz::... }`
//! block that converts a pointer received from C (category *slice*), each preceded by its
//! `// UNSAFE(slice): ...` comment, and in the `export!` expansion (category *export*).

pub mod s01_startup;
pub mod s02_diagnostics;
pub mod s03_memory;
pub mod s04_strings;
pub mod s05_ctype;
pub mod s06_multibyte;
pub mod s07_locale;
pub mod s08_numeric;
pub mod s09_random;
pub mod s10_math;
pub mod s10_math_gen;
pub mod s11_lowio;
pub mod s12_stdio;
pub mod s13_filesystem;
pub mod s14_search;
pub mod s15_sockets;
pub mod s16_nameservices;
pub mod s17_users;
pub mod s18_processes;
pub mod s19_terminals;
pub mod s20_signals;
pub mod s21_contexts;
pub mod s22_threads;
pub mod s23_time;
pub mod s24_system;
pub mod s25_ipc;
pub mod s26_syslog;
pub mod s27_dynlink;
pub mod s28_profiling;
pub mod s29_legacy;
pub mod s29_rpc_auth;
pub mod s29_rpc_clnt;
pub mod s29_rpc_svc;
pub mod s29_xdr_streams;
