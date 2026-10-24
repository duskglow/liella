//! Product crate for `libm.a` and `libm.so.6` (spec §10, D-10.7).
//!
//! It links `liella-boundary` with feature `libm` only, so the math translation functions are
//! emitted under their C names and nothing of libc is: `errno` and the memory primitives are
//! reached through the C library the program links (`__errno_location`, `memcpy`). The math
//! code never allocates, and the panic handler aborts with a diagnostic as libc's does.
//!
//! `cargo xtask so` links the static library into one relocatable object, keeps only the math
//! symbols global (the rest becomes local, so it never collides with `libc.a`), weakens the ones
//! glibc exports from both libraries (`frexp`, `ldexp`, ...), and archives it as `libm.a`; then
//! links `libm.so.6` from it with the version script from `spec/abi/versions.tsv`.

#![no_std]
#![forbid(unsafe_code)]

pub use liella_boundary as _boundary;

#[global_allocator]
static ALLOCATOR: liella_boundary::heap::Liella = liella_boundary::heap::Liella;

#[panic_handler]
fn panic(_: &core::panic::PanicInfo<'_>) -> ! {
    liella_boundary::process::libm_panicked()
}
