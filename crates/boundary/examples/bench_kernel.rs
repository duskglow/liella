//! Kernel-boundary micro-benchmarks for `cargo xtask bench` (rule P6). Prints one line per
//! measurement, `name<TAB>ns_per_op`, for the budget check in `bench/budgets.tsv`.
//!
//! No benchmark framework (zero-dependency policy): each case runs a calibration pass, then
//! the median of nine timed batches is reported.

#![allow(clippy::unwrap_used, clippy::as_conversions, clippy::arithmetic_side_effects, clippy::indexing_slicing, missing_docs)]

use std::hint::black_box;
use std::time::Instant;

use liella_boundary::sys::CSys;
use liella_kernel::machine::Machine;
use liella_kernel::sys::Sys;
use liella_kernel::uapi::CLOCK_MONOTONIC;

const K: CSys = CSys;

fn measure(name: &str, mut f: impl FnMut()) {
    // Calibrate a batch to about 10 ms.
    let mut n = 1u64;
    loop {
        let t = Instant::now();
        for _ in 0..n {
            f();
        }
        if t.elapsed().as_millis() >= 10 || n >= 1 << 30 {
            break;
        }
        n *= 2;
    }
    let mut samples: Vec<f64> = (0..9)
        .map(|_| {
            let t = Instant::now();
            for _ in 0..n {
                f();
            }
            t.elapsed().as_nanos() as f64 / n as f64
        })
        .collect();
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap());
    println!("{name}\t{:.2}", samples[4]);
}

fn main() {
    measure("thread_pointer", || {
        black_box(K.thread_pointer());
    });
    measure("getppid", || {
        black_box(K.getppid());
    });
    measure("clock_gettime_vdso", || {
        black_box(K.clock_gettime(CLOCK_MONOTONIC).unwrap());
    });
    measure("clock_gettime_syscall", || {
        black_box(K.clock_gettime_syscall(CLOCK_MONOTONIC).unwrap());
    });
    measure("cpuid_leaf0", || {
        black_box(K.cpuid(0, 0));
    });
    let mut buf = [0u8; 64];
    let fd = K.openat(liella_kernel::Fd::CWD, liella_kernel::KPathBuf::new(b"/dev/zero").unwrap().as_kstr(), 0, 0).unwrap();
    measure("read_dev_zero_64", || {
        black_box(K.read(fd, &mut buf).unwrap());
    });
}
