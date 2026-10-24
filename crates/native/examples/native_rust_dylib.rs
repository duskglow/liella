//! A program of the Rust dylib (D-X.10): linked against `libliella_rust.so.<abi>` (the crate
//! and the clean API, compiled once by the toolchain of Rust ABI version `<abi>`) and Liella's
//! `libc.so.6`, instead of compiling `liella-native` in. It runs the same steps as the other
//! examples, and one more: the Rust ABI version it was compiled against
//! (`liella_native::RUST_ABI`, a constant compiled into it) must be the one of the library it
//! runs with (`liella_native::LIELLA_RUST_ABI`, read from the dylib at run time).
//!
//! It defines no global allocator and no panic handler: the dylib defines both.
//!
//! `cargo xtask rust-dylib` builds it with `rustc --extern liella_native=...` (the commands are
//! in docs/toolchain.md) and checks with `readelf` that it needs `libliella_rust.so.<abi>` and
//! `libc.so.6`. It is not run yet: that needs Liella's dynamic loader (phase 14).

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

mod demo;

use alloc::format;
use alloc::string::String;

liella_native::entry!(start);

/// The version compiled in against the version loaded.
fn rust_abi() -> Result<String, String> {
    let loaded = liella_native::LIELLA_RUST_ABI.strip_suffix(b"\0").unwrap_or(&[]);
    let built = liella_native::RUST_ABI;
    if loaded == built.as_bytes() {
        Ok(format!("built against {built}, running with {built}"))
    } else {
        Err(format!("built against {built}, running with {}", String::from_utf8_lossy(loaded)))
    }
}

fn start() -> i32 {
    let [heap, secure, args, env, file, clock, tz, clean, locale, process, nscache, aio, getpass] = demo::STEPS;
    demo::run(&[("rust abi", rust_abi), heap, secure, args, env, file, clock, tz, clean, locale, process, nscache, aio, getpass])
}
