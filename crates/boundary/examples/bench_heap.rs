//! `cargo xtask bench` allocator benchmarks (phase 3; `benchmarks/progress.md`). Prints one
//! `name<TAB>ns/op` line per measurement.
//!
//! The reference is the host C library's allocator, reached through Rust's `Vec` (the global
//! allocator of this non-product build is the system `malloc`). `ref_*` rows allocate without
//! touching the memory, like the Liella rows. The threaded rows report wall time per
//! operation with every thread allocating and freeing at once: the cost of contention.

#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::unwrap_used, clippy::indexing_slicing, missing_docs)]

use std::hint::black_box;
use std::sync::Barrier;
use std::time::Instant;

use liella_boundary::posix::s03_memory as m;

fn time(mut f: impl FnMut()) -> f64 {
    for _ in 0..2000 {
        f();
    }
    let mut best = f64::MAX;
    for _ in 0..41 {
        let iters = 2000;
        let t = Instant::now();
        for _ in 0..iters {
            f();
        }
        best = best.min(t.elapsed().as_nanos() as f64 / f64::from(iters));
    }
    best
}

/// Wall time per malloc/free pair with `threads` threads each doing `per` pairs of `n` bytes,
/// best of five runs.
fn threaded(threads: usize, per: usize, n: usize, liella: bool) -> f64 {
    let mut best = f64::MAX;
    for _ in 0..5 {
        let start = Barrier::new(threads + 1);
        let t = std::thread::scope(|s| {
            for _ in 0..threads {
                s.spawn(|| {
                    start.wait();
                    for _ in 0..per {
                        if liella {
                            m::free(black_box(m::malloc(n)));
                        } else {
                            drop(black_box(Vec::<u8>::with_capacity(n)));
                        }
                    }
                });
            }
            start.wait();
            Instant::now()
        });
        best = best.min(t.elapsed().as_nanos() as f64 / (threads * per) as f64);
    }
    best
}

fn main() {
    for &(n, tag) in &[(64usize, "64"), (4096, "4k"), (65536, "64k"), (1 << 20, "1m")] {
        println!("ref_malloc_free_{tag}\t{:.2}", time(|| drop(black_box(Vec::<u8>::with_capacity(n)))));
        println!("malloc_free_{tag}\t{:.2}", time(|| m::free(black_box(m::malloc(n)))));
    }
    // A batch: 1000 live 64-byte blocks, then all freed (list and bitmap work, not one hot slot).
    println!("ref_batch_1000x64\t{:.2}", time(|| {
        let v: Vec<Vec<u8>> = (0..1000).map(|_| Vec::with_capacity(64)).collect();
        drop(black_box(v));
    }) / 1000.0);
    let mut ptrs = vec![std::ptr::null_mut(); 1000];
    println!("batch_1000x64\t{:.2}", time(|| {
        for p in &mut ptrs {
            *p = m::malloc(64);
        }
        for &p in black_box(&ptrs) {
            m::free(p);
        }
    }) / 1000.0);
    for t in [1usize, 4, 16] {
        println!("ref_threads{t}_64\t{:.2}", threaded(t, 200_000, 64, false));
        println!("threads{t}_64\t{:.2}", threaded(t, 200_000, 64, true));
    }
}
