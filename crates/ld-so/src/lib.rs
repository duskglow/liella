//! Product crate for `ld.so` (phase 14 DL-5, plan/ph14/design.md §0 and §1, R14-2, D-27.13).
//!
//! It links `liella-boundary` with feature `ld` (the start `liella_ld_start`, the self-relocator
//! and the loader's arch seam) and names what a `no_std` product must name once: the global
//! allocator (none yet: the loader does not allocate in DL-5) and the panic handler. `cargo xtask
//! so` links the archive with the `ld` variant of `machine/entry.S` into a shared object whose
//! entry point is `liella_machine_ld_entry`, then checks L14-1 over it.

#![no_std]
#![forbid(unsafe_code)]

pub use liella_boundary as _boundary;

#[global_allocator]
static ALLOCATOR: liella_boundary::ld::NoHeap = liella_boundary::ld::NoHeap;

#[panic_handler]
fn panic(info: &core::panic::PanicInfo<'_>) -> ! {
    liella_boundary::ld::panicked(info)
}
