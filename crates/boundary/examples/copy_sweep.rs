//! Copy-path measurements for the dispatch thresholds of `memcpy` (D-4.14, PJ-7): every copy
//! path of the boundary at a range of sizes and placements, with the host C library's `memcpy`
//! as the reference. Run on each timing machine:
//! `cargo run --release -p liella-boundary --example copy_sweep`.
//!
//! One line per size and placement: `n d sa ref fwd back erms avx2 memcpy memcpy_noavx2` in ns per
//! copy (fastest of the timed batches), where `d` is (dst - src) mod 4 KiB and `sa` the source
//! offset from a 4 KiB boundary; `-` where a path is unavailable.

#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::unwrap_used, clippy::indexing_slicing, clippy::cast_precision_loss, missing_docs)]

use std::hint::black_box;
use std::time::Instant;

use liella_boundary::posix::s04_strings as s;
use liella_boundary::selftest;

fn time(n: usize, mut f: impl FnMut()) -> f64 {
    let iters = (400_000 / n.max(64)).clamp(16, 2000) as u32;
    for _ in 0..iters {
        f();
    }
    let mut best = f64::MAX;
    for _ in 0..21 {
        let t = Instant::now();
        for _ in 0..iters {
            f();
        }
        best = best.min(t.elapsed().as_nanos() as f64 / f64::from(iters));
    }
    best
}

fn main() {
    liella_boundary::cpu::init();
    println!("# avx2 {} erms_threshold {}", liella_boundary::cpu::avx2(), liella_boundary::cpu::COPY_ERMS_MIN);
    const PAGE: usize = 4096;
    let max = 65_536;
    // Source pages at 0, destination pages at 2 * max: both 4 KiB-aligned bases.
    let mut buf = vec![0u8; 4 * max + 4 * PAGE];
    let base = buf.as_ptr().addr().wrapping_neg() & (PAGE - 1);
    for (i, b) in buf.iter_mut().enumerate() {
        *b = (i * 13 + 5) as u8;
    }
    let (lo, hi) = buf[base..].split_at_mut(2 * max);
    let placements: [(usize, usize); 13] =
        [(0, 0), (0, 16), (0, 64), (0, 100), (0, 511), (0, 1024), (0, 2047), (0, 2048), (0, 3072), (0, 4032), (0, 4080), (7, 0), (7, 16)];
    for n in [64usize, 128, 256, 512, 768, 1024, 1280, 2048, 4096, 8192, 16_384, 32_768, 65_536] {
        for &(sa, d) in &placements {
            let src = &lo[sa..sa + n];
            let dst = &mut hi[sa + d..sa + d + n];
            let r = time(n, || black_box(&mut *dst).copy_from_slice(black_box(src)));
            let fwd = time(n, || {
                selftest::copy_loops(black_box(&mut *dst), black_box(src), false);
            });
            let back = time(n, || {
                selftest::copy_loops(black_box(&mut *dst), black_box(src), true);
            });
            let erms = time(n, || {
                selftest::copy_erms(black_box(&mut *dst), black_box(src));
            });
            let avx2 = if selftest::copy_avx2(dst, src).is_some() {
                format!("{:.1}", time(n, || {
                    selftest::copy_avx2(black_box(&mut *dst), black_box(src));
                }))
            } else {
                "-".into()
            };
            let mc = time(n, || {
                s::memcpy(black_box(dst.as_mut_ptr()).cast(), black_box(src.as_ptr()).cast(), n);
            });
            liella_boundary::cpu::init_with(&liella::config::Tunables { avx2: Some(false), ..Default::default() });
            let mc_no = time(n, || {
                s::memcpy(black_box(dst.as_mut_ptr()).cast(), black_box(src.as_ptr()).cast(), n);
            });
            liella_boundary::cpu::init();
            assert_eq!(dst, src);
            println!("{n}\t{d}\t{sa}\t{r:.1}\t{fwd:.1}\t{back:.1}\t{erms:.1}\t{avx2}\t{mc:.1}\t{mc_no:.1}");
        }
    }
}
