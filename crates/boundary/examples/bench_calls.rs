//! Phase 5 benchmarks for `benchmarks/progress.md`: the cost of the thin kernel-surface
//! exports, against the same calls in the host glibc (`benchmarks/glibc_ref.c`). Prints one
//! `name<TAB>ns/op` line per measurement. Each figure is the fastest of 41 batches of 2000
//! calls after 2000 warm-up calls, the method of `bench_mem`.

#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::unwrap_used, missing_docs)]

use std::hint::black_box;
use std::time::Instant;

use liella_abi::layout::sigset_t;
use liella_boundary::posix::{s11_lowio as io, s13_filesystem as fs, s18_processes as pr, s20_signals as sg, s23_time as tm, s24_system as sy};
use liella_kernel::uapi::{Stat, Timespec, Timeval};

fn time(mut f: impl FnMut()) -> f64 {
    for _ in 0..2000 {
        f();
    }
    let mut best = f64::MAX;
    for _ in 0..41 {
        let t = Instant::now();
        for _ in 0..2000 {
            f();
        }
        best = best.min(t.elapsed().as_nanos() as f64 / 2000.0);
    }
    best
}

fn main() {
    let zero = io::liella_open3(c"/dev/zero".as_ptr(), 0, 0);
    let rows: [(&str, &mut dyn FnMut()); 11] = [
        ("getpid", &mut || {
            black_box(pr::getpid());
        }),
        ("getppid", &mut || {
            black_box(pr::getppid());
        }),
        ("clock_gettime_mono", &mut || {
            let mut ts = Timespec::default();
            black_box(tm::clock_gettime(1, Some(&mut ts)));
        }),
        ("time", &mut || {
            black_box(tm::time(None));
        }),
        ("gettimeofday", &mut || {
            let mut tv = Timeval::default();
            black_box(tm::gettimeofday(Some(&mut tv), None));
        }),
        ("sysconf_pagesize", &mut || {
            black_box(sy::sysconf(30));
        }),
        ("sysconf_nprocs_onln", &mut || {
            black_box(sy::sysconf(84));
        }),
        ("read_devzero_1", &mut || {
            let mut b = [0u8; 1];
            black_box(io::read(zero, b.as_mut_ptr().cast(), 1));
        }),
        ("sigprocmask_query", &mut || {
            let mut s = sigset_t { __val: [0; 16] };
            black_box(sg::sigprocmask(0, None, Some(&mut s)));
        }),
        ("stat_root", &mut || {
            let mut st = Stat::default();
            black_box(fs::stat(c"/".as_ptr(), Some(&mut st)));
        }),
        ("open_close_devnull", &mut || {
            let fd = io::liella_open3(c"/dev/null".as_ptr(), 0, 0);
            black_box(io::close(fd));
        }),
    ];
    for (name, f) in rows {
        println!("{name}\t{:.2}", time(f));
    }
}
