//! The process heap (`liella_heap_*`): the one behind `malloc`, as Rust's global allocator.
//!
//! ```text
//! #[global_allocator]
//! static HEAP: liella_native::heap::Allocator = liella_native::heap::Allocator;
//! ```
//!
//! Every Rust allocation of the program then comes from the C library's heap: the program's
//! Rust code, its C code and the C library allocate from one heap, under one set of limits and
//! misuse checks.

#![forbid(unsafe_code)]

pub use crate::ffi::Allocator;

/// The alignment every heap block has at least (`LIELLA_HEAP_MIN_ALIGN`). Blocks with at
/// most this alignment are resized by the heap itself; larger ones are moved.
pub const MIN_ALIGN: usize = crate::ffi::HEAP_MIN_ALIGN;
