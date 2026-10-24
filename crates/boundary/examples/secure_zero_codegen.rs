//! The mechanical proof that `liella_secure_zero` is never optimised away (D-X.11), checked by
//! `cargo xtask native`: this program is built with the workspace's release profile (fat LTO,
//! one codegen unit, `opt-level = 3`), so `liella_secure_zero` and everything it calls in Rust
//! can be inlined into the functions below, and the executable is then disassembled.
//!
//! Each function derives a secret into a 4096-byte local buffer, folds it into a checksum (the
//! one value that leaves the function), then zeroes the buffer and returns. After the zeroing
//! the buffer is dead: no code can read it again, so an optimiser may remove a zeroing it can
//! see through.
//!
//! * `dead_fill` zeroes it with `fill(0)`, the control: the zeroing must be gone (no `memset`
//!   call, no `rep stos`), which shows that this build does remove dead zeroing.
//! * `dead_wipe_without_barrier` zeroes it with the clean library's `bytes::wipe` and a
//!   closure that does nothing: also gone, which shows that the barrier is what keeps the
//!   stores.
//! * `dead_secure_zero` calls `liella_secure_zero`: the 4096-byte zeroing must remain, and be
//!   followed by the call to the C layer's barrier `liella_machine_barrier`.

#![allow(missing_docs)]

use std::hint::black_box;

const N: usize = 4096;

/// A secret derived from `seed` (a linear congruential sequence's high bytes), built in place
/// (so that no zero initialisation precedes it).
fn derive(seed: u64) -> [u8; N] {
    let mut x = seed;
    core::array::from_fn(|_| {
        x = x.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        x.to_be_bytes()[0]
    })
}

/// A checksum of `buf`; `k` keeps the three functions distinct (identical code would be
/// folded into one function).
fn checksum(buf: &[u8], k: u64) -> u64 {
    buf.iter().fold(0u64, |a, &b| a.wrapping_mul(k).wrapping_add(u64::from(b)))
}

#[inline(never)]
fn dead_fill(seed: u64) -> u64 {
    let mut secret = derive(seed);
    let sum = checksum(&secret, 31);
    secret.fill(0);
    sum
}

#[inline(never)]
fn dead_wipe_without_barrier(seed: u64) -> u64 {
    let mut secret = derive(seed);
    let sum = checksum(&secret, 37);
    liella::bytes::wipe(&mut secret, |_| {});
    sum
}

#[inline(never)]
fn dead_secure_zero(seed: u64) -> u64 {
    let mut secret = derive(seed);
    let sum = checksum(&secret, 41);
    liella_boundary::native::secure::liella_secure_zero(secret.as_mut_ptr().cast(), secret.len());
    sum
}

fn main() {
    let seed = black_box(0x5eed_u64);
    println!("dead_fill\t{:016x}", dead_fill(seed));
    println!("dead_wipe_without_barrier\t{:016x}", dead_wipe_without_barrier(seed));
    println!("dead_secure_zero\t{:016x}", dead_secure_zero(seed));
}
