//! `cargo xtask bench` memory benchmarks (phase 2 exit budgets; OD-C2 data). Prints one
//! `name<TAB>ns/op` line per measurement.
//!
//! The reference is the hardware copy bandwidth, measured as the host C library's `memcpy`
//! (which `copy_from_slice` calls in this non-product build). Each figure is the fastest of
//! many timed batches: the attainable speed, with scheduler noise removed.

#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::unwrap_used, clippy::indexing_slicing, missing_docs)]

use std::hint::black_box;
use std::time::Instant;

use liella_abi::ctypes::c_char;
use liella_boundary::posix::s04_strings as s;

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

fn main() {
    // The C layer's paths are chosen at startup in the products; here, explicitly.
    // `LIELLA_BENCH_NO_AVX2` (any value) measures the paths without AVX2 (PJ-7 before/after).
    if std::env::var_os("LIELLA_BENCH_NO_AVX2").is_some() {
        liella_boundary::cpu::init_with(&liella::config::Tunables { avx2: Some(false), ..Default::default() });
    } else {
        liella_boundary::cpu::init();
    }
    for &(n, tag) in &[(64usize, "64"), (4096, "4k"), (65536, "64k")] {
        let src = vec![0x5Au8; n];
        let mut dst = vec![0u8; n];
        let other = src.clone();
        let mut text = vec![b'x'; n + 1];
        text[n] = 0;
        let rows = [
            ("ref_memcpy", time(|| black_box(&mut dst[..]).copy_from_slice(black_box(&src[..])))),
            ("memcpy", time(|| {
                s::memcpy(black_box(dst.as_mut_ptr()).cast(), black_box(src.as_ptr()).cast(), n);
            })),
            ("memset", time(|| {
                s::memset(black_box(dst.as_mut_ptr()).cast(), 7, n);
            })),
            ("memcmp", time(|| {
                black_box(s::memcmp(black_box(src.as_ptr()).cast(), black_box(other.as_ptr()).cast(), n));
            })),
            ("memchr", time(|| {
                black_box(s::memchr(black_box(src.as_ptr()).cast(), 0, n));
            })),
            ("strlen", time(|| {
                black_box(s::strlen(black_box(text.as_ptr()).cast::<c_char>()));
            })),
            ("strnlen", time(|| {
                black_box(s::strnlen(black_box(text.as_ptr()).cast::<c_char>(), n + 1));
            })),
            ("rawmemchr", time(|| {
                black_box(s::rawmemchr(black_box(text.as_ptr()).cast(), 0));
            })),
        ];
        for (name, ns) in rows {
            println!("{name}_{tag}\t{ns:.2}");
        }
    }
    // Source and destination 64 bytes off a 4 KiB distance (no 4K aliasing between them):
    // OD-C2 data on how much of the gap is the aliasing glibc's ERMS/AVX2 paths absorb.
    let n = 4096;
    let src = vec![0x5Au8; n + 128];
    let mut dst = vec![0u8; n + 128];
    let s_off = &src[64..64 + n];
    println!("ref_memcpy_4k_off\t{:.2}", time(|| black_box(&mut dst[64..64 + n]).copy_from_slice(black_box(s_off))));
    println!("memcpy_4k_off\t{:.2}", time(|| {
        s::memcpy(black_box(dst[64..].as_mut_ptr()).cast(), black_box(s_off.as_ptr()).cast(), n);
    }));
    println!("memset_4k_off\t{:.2}", time(|| {
        s::memset(black_box(dst[64..].as_mut_ptr()).cast(), 7, n);
    }));
}
