// Specified from glibc-functional-spec.md §9. Generated stubs were replaced by real tests of the
// SAME NAME (see crates/spectest); `cargo xtask stubs` now only checks coverage.

//! Clause tests for spec §9 (25 clauses).
//!
//! Kernel failures are injected with `liella_kernel::testing::Faulty`. Two clauses stay
//! pending: §9.1's cancellation-point guarantee (cancellation arrives in phase 6), and the
//! "identical across glibc versions" half of §9.3's guarantee is a recorded incompatibility
//! (OD-13, ERRATA E5); the determinism half is tested.

#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::float_cmp, missing_docs)]

use std::sync::Mutex;

use liella::random::{self, AdditiveFeedback, ByteWords, Rng48, RNG48_A, RNG48_C, SIZES};
use liella_boundary::sys::CSys;
use liella_kernel::sys::Sys;
use liella_kernel::testing::Faulty;
use liella_kernel::uapi::{GRND_INSECURE, GRND_NONBLOCK, GRND_RANDOM};
use liella_kernel::Errno;
use liella_legacy::prng_global;

/// Build and run C conformance programs against the products (`conformance/run.sh`).
fn conformance(names: &[&str]) {
    static RUNNER: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _g = RUNNER.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let products = ["target/liella/libc.a", "target/liella/liella-crt1.o"];
    assert!(products.iter().all(|p| root.join(p).exists()), "the products are missing: run `cargo xtask so` (`cargo xtask test` does)");
    let o = std::process::Command::new("sh").arg(root.join("conformance/run.sh")).args(names).output().unwrap();
    assert!(o.status.success(), "{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr));
}

static GLOBAL: Mutex<()> = Mutex::new(());

/// Memory for the global generator's caller buffers: none are used here.
struct NoMem;

impl prng_global::CMem for NoMem {
    fn bytes(&mut self, _addr: usize, _len: usize) -> Option<&mut [u8]> {
        None
    }
}

#[test]
fn s9_1_c_1() {
    let mut b = [0u8; 300];
    let n = CSys.getrandom(&mut b, 0).unwrap();
    assert!(n > 0 && n <= 300, "up to n bytes");
    let k = Faulty::new(CSys);
    k.fail("getrandom", Errno::EINTR, 0, 2);
    let mut c = [0u8; 8];
    random::fill(&k, &mut c).unwrap();
    assert_eq!(k.injected(), 2, "interrupted reads are retried by the fill loop");
}

#[test]
fn s9_1_c_1_1() {
    let mut b = [0u8; 8];
    assert_eq!(CSys.getrandom(&mut b, GRND_NONBLOCK), Ok(8), "the pool is initialised on a running system");
    assert!(random::pool_ready(&CSys));
    let k = Faulty::new(CSys);
    k.fail("getrandom", Errno::EAGAIN, 0, 1);
    assert!(!random::pool_ready(&k), "EAGAIN means not yet initialised");
}

#[test]
fn s9_1_c_1_2() {
    let mut b = [0u8; 8];
    assert!(CSys.getrandom(&mut b, GRND_RANDOM).is_ok());
}

#[test]
fn s9_1_c_1_3() {
    let mut b = [0u8; 8];
    match CSys.getrandom(&mut b, GRND_INSECURE) {
        Ok(n) => assert_eq!(n, 8),
        Err(e) => assert_eq!(e, Errno::EINVAL, "kernels before 5.6 reject the flag"),
    }
}

#[test]
fn s9_1_c_2() {
    let mut b = [0u8; 257];
    assert_eq!(random::entropy(&CSys, &mut b[..256]), Ok(()));
    assert_eq!(random::entropy(&CSys, &mut b), Err(Errno::EIO), "more than 256");
    let k = Faulty::new(CSys);
    k.fail("getrandom", Errno::ENOSYS, 0, 1);
    assert_eq!(random::entropy(&k, &mut b[..8]), Err(Errno::EIO), "any kernel failure is EIO");
}

#[test]
fn s9_1_c_3() {
    let mut ones = [0u32; 32];
    for _ in 0..2000 {
        let v = random::u32_random(&CSys).unwrap();
        for (i, c) in ones.iter_mut().enumerate() {
            *c += (v >> i) & 1;
        }
    }
    assert!(ones.iter().all(|&c| (800..1200).contains(&c)), "every bit about half the time: {ones:?}");
}

#[test]
fn s9_1_c_4() {
    let mut b = [0u8; 1 << 16];
    random::fill(&CSys, &mut b).unwrap();
    let zeros = b.iter().filter(|&&x| x == 0).count();
    assert!((100..420).contains(&zeros), "{zeros} zero bytes in 65536");
}

#[test]
fn s9_1_c_5() {
    assert_eq!((random::uniform(&CSys, 0), random::uniform(&CSys, 1)), (Some(0), Some(0)));
    let mut counts = [0u32; 3];
    for _ in 0..3000 {
        counts[random::uniform(&CSys, 3).unwrap() as usize] += 1;
    }
    assert!(counts.iter().all(|&c| (850..1150).contains(&c)), "{counts:?}");
    let top = u32::MAX / 2 + 2;
    for _ in 0..200 {
        assert!(random::uniform(&CSys, top).unwrap() < top);
    }
}

#[test]
fn s9_1_g_1() {
    let k = Faulty::new(CSys);
    k.fail("getrandom", Errno::ENOSYS, 0, 1);
    assert_eq!(random::u32_random(&k), None, "no value rather than a made-up one");
    let k = Faulty::new(CSys);
    k.fail("getrandom", Errno::EIO, 0, 1);
    assert_eq!(random::uniform(&k, 10), None);
}

#[test]
fn s9_1_g_2() {
    let k = Faulty::new(CSys);
    k.fail("getrandom", Errno::EIO, 1, 1);
    assert!(random::u32_random(&k).is_some(), "the first call reaches the kernel");
    assert!(random::u32_random(&k).is_none(), "and so does the second: nothing is buffered");
}

/// §9.1 Guarantees: `getrandom` is a cancellation point. `arc4random` is not.
#[test]
fn s9_1_g_3() {
    conformance(&["s22_cancel_points"]);
}

#[test]
fn s9_2_c_0() {
    // "The default seed is 1" is only observable before anything touches the generator, so
    // that half runs alone in a fresh process.
    if std::env::var_os("LIELLA_S9_2_C_0").is_some() {
        let first: Vec<i64> = (0..5).map(|_| prng_global::random(NoMem)).collect();
        prng_global::srandom(1, NoMem);
        let again: Vec<i64> = (0..5).map(|_| prng_global::random(NoMem)).collect();
        assert_eq!(first, again, "the default seed is 1");
        assert!(first.iter().all(|&v| (0..=0x7fff_ffff).contains(&v)), "in [0, RAND_MAX]");
        return;
    }
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["s9_2_c_0", "--exact", "--test-threads=1"])
        .env("LIELLA_S9_2_C_0", "1")
        .status()
        .unwrap();
    assert!(status.success(), "the default seed is 1 (fresh process)");
    let _g = GLOBAL.lock().unwrap();
    prng_global::srandom(7, NoMem);
    let a: Vec<i64> = (0..5).map(|_| prng_global::random(NoMem)).collect();
    prng_global::srandom(7, NoMem);
    let b: Vec<i64> = (0..5).map(|_| prng_global::random(NoMem)).collect();
    assert_eq!(a, b, "deterministic for a given seed");
    let mut s = 1u32;
    let r = prng_global::rand_r(&mut s);
    let mut t = 1u32;
    assert_eq!(prng_global::rand_r(&mut t), r, "rand_r keeps its state in the caller's variable");
    assert!(r >= 0);
}

#[test]
fn s9_2_t_0() {
    let _g = GLOBAL.lock().unwrap();
    prng_global::srandom(5, NoMem);
    let single: Vec<i64> = (0..4000).map(|_| prng_global::random(NoMem)).collect();
    prng_global::srandom(5, NoMem);
    let mut all: Vec<i64> = std::thread::scope(|sc| {
        let hs: Vec<_> = (0..4).map(|_| sc.spawn(|| (0..1000).map(|_| prng_global::random(NoMem)).collect::<Vec<_>>())).collect();
        hs.into_iter().flat_map(|h| h.join().unwrap()).collect()
    });
    let mut want = single.clone();
    all.sort_unstable();
    want.sort_unstable();
    assert_eq!(all, want, "concurrent callers share one sequence without losing or repeating values");
}

#[test]
fn s9_3_c_0() {
    for (n, t) in [(8, 0), (31, 0), (32, 1), (64, 2), (128, 3), (255, 3), (256, 4), (1000, 4)] {
        assert_eq!(AdditiveFeedback::type_for(n), Some(t), "{n} bytes");
    }
    for &size in &SIZES {
        let mut buf = vec![0u8; size];
        AdditiveFeedback::seed(&mut ByteWords(&mut buf), AdditiveFeedback::type_for(size).unwrap(), 9);
        for _ in 0..1000 {
            assert!(AdditiveFeedback::next(&mut ByteWords(&mut buf)) <= 0x7fff_ffff);
        }
    }
}

#[test]
fn s9_3_g_0() {
    // Deterministic for a seed and state size (the cross-version half is E5, OD-13).
    for &size in &SIZES {
        let t = AdditiveFeedback::type_for(size).unwrap();
        let run = || {
            let mut buf = vec![0u8; size];
            AdditiveFeedback::seed(&mut ByteWords(&mut buf), t, 1234);
            (0..20).map(|_| AdditiveFeedback::next(&mut ByteWords(&mut buf))).collect::<Vec<_>>()
        };
        assert_eq!(run(), run(), "{size} bytes");
    }
}

#[test]
fn s9_3_f_0() {
    assert_eq!(AdditiveFeedback::type_for(7), None);
    assert_eq!(prng_global::initstate(1, 0x1000, 7, NoMem), Err(Errno::EINVAL));
}

#[test]
fn s9_4_c_0() {
    let mut r = Rng48::UNSEEDED;
    let v = r.next();
    assert!(v < 1 << 48, "48 bits");
}

#[test]
fn s9_4_c_p1() {
    assert_eq!((RNG48_A, RNG48_C), (0x5DEECE66D, 0xB));
    let mut r = Rng48 { x: [0x330E, 0x0001, 0x0000], a: RNG48_A, c: RNG48_C };
    let x0 = 0x1_330Eu64;
    assert_eq!(r.next(), (RNG48_A.wrapping_mul(x0) + 0xB) & ((1 << 48) - 1));
}

#[test]
fn s9_4_c_p2() {
    // The POSIX sequence after srand48(0): fixed values, independent of any implementation.
    let mut r = Rng48::UNSEEDED;
    r.srand48(0);
    let vals: Vec<i64> = (0..3).map(|_| Rng48::to_nonneg(r.next())).collect();
    let mut x = 0x330Eu64;
    let want: Vec<i64> = (0..3).map(|_| {
        x = (0x5DEECE66Du64.wrapping_mul(x) + 0xB) & ((1 << 48) - 1);
        (x >> 17) as i64
    }).collect();
    assert_eq!(vals, want);
    assert_eq!(vals[0], 366_850_414, "the well-known first lrand48() after srand48(0)");
}

#[test]
fn s9_4_c_3() {
    assert_eq!(Rng48::to_double(0), 0.0);
    assert!(Rng48::to_double((1 << 48) - 1) < 1.0);
}

#[test]
fn s9_4_c_4() {
    assert_eq!((Rng48::to_nonneg(0), Rng48::to_nonneg((1 << 48) - 1)), (0, (1 << 31) - 1));
}

#[test]
fn s9_4_c_5() {
    assert_eq!((Rng48::to_signed(1 << 47), Rng48::to_signed((1 << 47) - 1)), (-(1 << 31), (1 << 31) - 1));
}

#[test]
fn s9_4_c_6() {
    let r = Rng48::UNSEEDED;
    let mut caller = [1u16, 0, 0];
    let v = r.step(&mut caller);
    assert_eq!(caller, [v as u16, (v >> 16) as u16, (v >> 32) as u16], "the caller's state advances");
    assert_eq!(r, Rng48::UNSEEDED, "the generator's own state does not");
}

#[test]
fn s9_4_c_7() {
    let mut r = Rng48::UNSEEDED;
    let old = r.seed48([1, 2, 3]);
    assert_eq!((old, r.x, r.a, r.c), (Rng48::UNSEEDED.x, [1, 2, 3], RNG48_A, RNG48_C));
    r.lcong48([4, 5, 6, 7, 0, 0, 9]);
    assert_eq!((r.x, r.a, r.c), ([4, 5, 6], 7, 9));
}
