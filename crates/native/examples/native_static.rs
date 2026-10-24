//! The static path of the native Rust interface (D-X.10): a program linked like a product,
//! statically, with `liella-crt1.o` and Liella's `libc.a` and no host C library, using the
//! heap as its global allocator, the environment, a file round trip, the clocks, the time
//! zone, the locale and the process's arguments, ID and exit, and printing what it did.
//!
//! `cargo xtask native` builds it (feature `static`) and runs it; the module documentation of
//! xtask/src/native_examples.rs gives the exact commands. It is `#![no_std]` and `#![no_main]` until
//! Rust's `std` links against `libc.a` (the symbols it still lacks are listed there).

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

mod demo;

use alloc::format;
use alloc::string::String;

// With feature `dylib` (all features, under clippy) the library defines both.
#[cfg(not(feature = "dylib"))]
#[global_allocator]
static HEAP: liella_native::heap::Allocator = liella_native::heap::Allocator;

liella_native::entry!(start);

/// The public types of liella-kernel, re-exported with feature `static` only.
fn kernel_types() -> Result<String, String> {
    let e = liella_native::kernel::Errno::ENOENT;
    if liella_native::Errno::ENOENT == e { Ok(format!("{e} is the facade's Errno")) } else { Err(String::from("two Errno types")) }
}

fn start() -> i32 {
    let [heap, secure, args, env, file, clock, tz, clean, locale, process, nscache, aio, getpass] = demo::STEPS;
    demo::run(&[heap, secure, args, env, file, clock, tz, clean, ("kernel types", kernel_types), locale, process, nscache, aio, getpass])
}

#[cfg(not(feature = "dylib"))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    demo::panicked(info)
}
