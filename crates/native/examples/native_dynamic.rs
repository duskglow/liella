//! The dynamic path of the native Rust interface (D-X.10): a program linked against Liella's
//! `libc.so.6`, reaching the stateful services only through the `LIELLA_NATIVE_1` symbols,
//! with the stateless clean API compiled in. It runs the same steps as the static example.
//!
//! `cargo xtask native` builds it (feature `dynamic`) and checks with `readelf` that it needs
//! `libc.so.6` and references the native symbols at `LIELLA_NATIVE_1`. It is not run yet: that
//! needs Liella's dynamic loader (phase 14), and the host's loader must never load Liella's
//! `libc.so.6`.

#![no_std]
#![no_main]
#![forbid(unsafe_code)]

extern crate alloc;

mod demo;

// With feature `dylib` (all features, under clippy) the library defines both.
#[cfg(not(feature = "dylib"))]
#[global_allocator]
static HEAP: liella_native::heap::Allocator = liella_native::heap::Allocator;

liella_native::entry!(start);

fn start() -> i32 {
    demo::run(&demo::STEPS)
}

#[cfg(not(feature = "dylib"))]
#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    demo::panicked(info)
}
