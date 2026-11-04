//! Test builds only (ALLOC-FAIL-INJECT, `selftest`; no spec clause: a test facility): the host
//! allocator with the calling thread's [`super::failinject`] plan in front of it. A test binary
//! declares [`FailAlloc`] as its `#[global_allocator]` to reach the internal heap's failure paths.
//!
//! This file is compiled only with the `selftest` feature (through `selftest.rs`), which no
//! product enables, so the shipped libraries compile no hook code at all (coordinator #2101).

extern crate std;

use core::alloc::{GlobalAlloc, Layout};
use std::alloc::System;

use super::failinject::{count_live, should_fail};

fn bytes(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// The host allocator with the calling thread's plan in front of it.
#[derive(Clone, Copy, Debug, Default)]
pub struct FailAlloc;

// UNSAFE(allocator): implementing `GlobalAlloc` is the allocator category itself.
// SAFETY: every block comes from `System` with the caller's layout and goes back to it unchanged;
// an injected failure is a null return, which the trait allows for any request.
#[allow(unsafe_code)]
unsafe impl GlobalAlloc for FailAlloc {
    // UNSAFE(allocator): the trait's required method.
    #[allow(unsafe_code)]
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if should_fail(layout.size()) {
            return core::ptr::null_mut();
        }
        // UNSAFE(allocator): passing the request on to the host allocator.
        // SAFETY: the caller's contract (a non-zero-size layout) is passed on unchanged.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            count_live(bytes(layout.size()));
        }
        p
    }

    // UNSAFE(allocator): the trait's required method.
    #[allow(unsafe_code)]
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // UNSAFE(allocator): returning the block to the host allocator.
        // SAFETY: `ptr` came from `System` (this allocator's only source) with `layout`.
        unsafe { System.dealloc(ptr, layout) };
        count_live(bytes(layout.size()).wrapping_neg());
    }

    // UNSAFE(allocator): the trait's provided method, overridden to keep `System`'s zeroing.
    #[allow(unsafe_code)]
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        if should_fail(layout.size()) {
            return core::ptr::null_mut();
        }
        // UNSAFE(allocator): passing the request on to the host allocator.
        // SAFETY: as `alloc`.
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            count_live(bytes(layout.size()));
        }
        p
    }

    // UNSAFE(allocator): the trait's provided method, overridden: a failed growth leaves the block.
    #[allow(unsafe_code)]
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        if new_size > layout.size() && should_fail(new_size) {
            return core::ptr::null_mut();
        }
        // UNSAFE(allocator): passing the request on to the host allocator.
        // SAFETY: `ptr` came from `System` with `layout`; the caller guarantees `new_size`.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            count_live(bytes(new_size).wrapping_sub(bytes(layout.size())));
        }
        p
    }
}
