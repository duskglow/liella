// Specified from spec §9 and its Appendix B list. Generated stubs were replaced by real tests of
// the SAME NAME (see crates/spectest); `cargo xtask stubs` now only checks coverage.

//! Translation-layer tests for the exported symbols of spec §9 (34 entries).
//! Test names keep the exported C name verbatim (`sym_<name>`) for traceability.
//!
//! The global generators are process-wide, so every test that uses one holds [`GLOBAL`]. The
//! 48-bit family is checked against the recurrence of §9.4 computed here; `rand`/`random`
//! sequences are unspecified (OD-13), so their tests check range, determinism and the state
//! protocol.
#![no_builtins] // calls to libc names (labs, strlen, ...) reach Liella's export, not LLVM's folding (OPAQUE-AUDIT)
#![allow(non_snake_case)]
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::float_cmp, missing_docs)]

use core::ptr::null_mut;
use std::sync::{Mutex, MutexGuard};

use liella_abi::ctypes::c_char;
use liella_abi::layout::{struct_drand48_data, struct_random_data};
use liella_boundary::errno;
use liella_boundary::posix::s09_random as s;
use liella_boundary::selftest::{self, ChildEnd};

const EINVAL: i32 = 22;
const EIO: i32 = 5;
const SIGABRT: i32 = 6;
const A: u64 = 0x5_DEEC_E66D;
const MASK48: u64 = (1 << 48) - 1;

static GLOBAL: Mutex<()> = Mutex::new(());

fn global() -> MutexGuard<'static, ()> {
    GLOBAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// One step of the §9.4 recurrence with the default parameters.
fn step(x: u64) -> u64 {
    (A.wrapping_mul(x) + 0xB) & MASK48
}

fn x48(v: [u16; 3]) -> u64 {
    u64::from(v[0]) | (u64::from(v[1]) << 16) | (u64::from(v[2]) << 32)
}

fn d48() -> struct_drand48_data {
    struct_drand48_data { __x: [0; 3], __old_x: [0; 3], __c: 0, __init: 0, __a: 0 }
}

fn rdata() -> struct_random_data {
    struct_random_data { fptr: 0, rptr: 0, state: 0, rand_type: 0, rand_deg: 0, rand_sep: 0, end_ptr: 0 }
}

// ------------------------------------------------------------------------------ kernel randomness

#[test]
fn sym_getrandom() {
    let mut b = [0u8; 64];
    assert_eq!(s::getrandom(b.as_mut_ptr().cast(), 64, 0), 64);
    assert!(b.iter().any(|&x| x != 0));
    assert_eq!(s::getrandom(b.as_mut_ptr().cast(), 16, 1), 16, "GRND_NONBLOCK with an initialised pool");
    errno::set_raw(0);
    assert_eq!(s::getrandom(b.as_mut_ptr().cast(), 16, 0x100), -1);
    assert_eq!(errno::get(), EINVAL, "unknown flags");
}

#[test]
fn sym_getentropy() {
    let mut b = [0u8; 257];
    assert_eq!(s::getentropy(b.as_mut_ptr().cast(), 256), 0);
    errno::set_raw(0);
    assert_eq!(s::getentropy(b.as_mut_ptr().cast(), 257), -1);
    assert_eq!(errno::get(), EIO, "more than 256 bytes");
}

#[test]
fn sym_arc4random() {
    let v: Vec<u32> = (0..8).map(|_| s::arc4random()).collect();
    assert!(v.windows(2).any(|w| w[0] != w[1]), "not constant");
    assert_eq!(selftest::in_child_quiet(|_| {
        liella_boundary::ret::arc4::<u32>(None);
        0
    }).unwrap(), ChildEnd::Signaled(SIGABRT), "no entropy: the process terminates");
}

#[test]
fn sym_arc4random_buf() {
    let mut b = [0u8; 4096];
    s::arc4random_buf(b.as_mut_ptr().cast(), 4096);
    let zeros = b.iter().filter(|&&x| x == 0).count();
    assert!(zeros < 64, "filled ({zeros} zero bytes in 4096)");
    s::arc4random_buf(null_mut(), 0);
}

#[test]
fn sym_arc4random_uniform() {
    assert_eq!((s::arc4random_uniform(0), s::arc4random_uniform(1)), (0, 0));
    let mut seen = [false; 7];
    for _ in 0..500 {
        let v = s::arc4random_uniform(7);
        assert!(v < 7);
        seen[v as usize] = true;
    }
    assert!(seen.iter().all(|&b| b), "every residue occurs");
}

// ------------------------------------------------------------------------------ rand / random

#[test]
fn sym_rand() {
    let _g = global();
    s::srand(42);
    let a: Vec<i32> = (0..20).map(|_| s::rand()).collect();
    s::srand(42);
    let b: Vec<i32> = (0..20).map(|_| s::rand()).collect();
    assert_eq!(a, b, "deterministic for a seed");
    assert!(a.iter().all(|&v| v >= 0), "in [0, RAND_MAX]");
}

#[test]
fn sym_srand() {
    let _g = global();
    s::srand(1);
    let one = s::rand();
    s::srand(2);
    assert_ne!(s::rand(), one);
}

#[test]
fn sym_rand_r() {
    let (mut a, mut b) = (7u32, 7u32);
    let x: Vec<i32> = (0..10).map(|_| s::rand_r(Some(&mut a))).collect();
    let y: Vec<i32> = (0..10).map(|_| s::rand_r(Some(&mut b))).collect();
    assert_eq!(x, y, "the state is the caller's");
    assert!(x.iter().all(|&v| v >= 0));
    assert_ne!(a, 7, "the seed advances");
}

#[test]
fn sym_random() {
    let _g = global();
    s::srandom(3);
    let a: Vec<i64> = (0..50).map(|_| s::random()).collect();
    assert!(a.iter().all(|&v| (0..1 << 31).contains(&v)));
    s::srandom(3);
    assert_eq!(s::random(), a[0]);
}

#[test]
fn sym_srandom() {
    let _g = global();
    s::srandom(9);
    let x = s::random();
    s::srandom(9);
    assert_eq!(s::random(), x);
}

#[test]
fn sym_srandom_zero_is_one() {
    // glibc black-box (FTOK-HIGHBIT): srandom(0) and srand(0) seed like seed 1.
    let _g = global();
    s::srandom(1);
    let one: Vec<_> = (0..4).map(|_| s::random()).collect();
    s::srandom(0);
    let zero: Vec<_> = (0..4).map(|_| s::random()).collect();
    assert_eq!(zero, one, "srandom(0) == srandom(1)");
    s::srand(0);
    let zero: Vec<_> = (0..4).map(|_| s::random()).collect();
    assert_eq!(zero, one, "srand(0) == srand(1)");
}

#[test]
fn sym_initstate() {
    let _g = global();
    let mut mine = Box::new([0u8; 64]);
    let p = mine.as_mut_ptr().cast::<c_char>();
    let prev = s::initstate(5, p, 64);
    assert!(!prev.is_null(), "the previous state");
    let a: Vec<i64> = (0..5).map(|_| s::random()).collect();
    s::initstate(5, p, 64);
    let b: Vec<i64> = (0..5).map(|_| s::random()).collect();
    assert_eq!(a, b, "deterministic for seed and size");
    s::setstate(prev);
    let mut tiny = [0u8; 7];
    errno::set_raw(0);
    assert!(s::initstate(1, tiny.as_mut_ptr().cast(), 7).is_null());
    assert_eq!(errno::get(), EINVAL, "fewer than 8 bytes");
}

#[test]
fn sym_setstate() {
    let _g = global();
    let mut one = Box::new([0u8; 128]);
    let mut two = Box::new([0u8; 32]);
    let def = s::initstate(1, one.as_mut_ptr().cast(), 128);
    let first = s::random();
    let back = s::initstate(1, two.as_mut_ptr().cast(), 32);
    assert_eq!(back, one.as_mut_ptr().cast(), "initstate returns the state it replaced");
    let _ = s::random();
    assert_eq!(s::setstate(one.as_mut_ptr().cast()), two.as_mut_ptr().cast());
    s::initstate(1, one.as_mut_ptr().cast(), 128);
    assert_eq!(s::random(), first, "switching back resumes that buffer");
    s::setstate(def);
}

#[test]
fn sym_random_r() {
    let mut state = [0u8; 32];
    let mut d = rdata();
    let mut out = 0i32;
    errno::set_raw(0);
    assert_eq!(s::random_r(Some(&mut d), Some(&mut out)), -1, "no state yet");
    assert_eq!(errno::get(), EINVAL);
    assert_eq!(s::initstate_r(11, state.as_mut_ptr().cast(), 32, Some(&mut d)), 0);
    assert_eq!(s::random_r(Some(&mut d), Some(&mut out)), 0);
    assert!(out >= 0);
}

#[test]
fn sym_random_r_seed_zero_is_one() {
    // glibc black-box (FTOK-HIGHBIT-2): initstate_r / srandom_r with seed 0 behave as seed 1.
    let run = |seed: u32| {
        let mut state = [0u8; 128];
        let mut d = rdata();
        assert_eq!(s::initstate_r(seed, state.as_mut_ptr().cast(), 128, Some(&mut d)), 0);
        let mut v = Vec::new();
        for _ in 0..4 {
            let mut o = 0i32;
            s::random_r(Some(&mut d), Some(&mut o));
            v.push(o);
        }
        assert_eq!(s::srandom_r(seed, Some(&mut d)), 0);
        for _ in 0..4 {
            let mut o = 0i32;
            s::random_r(Some(&mut d), Some(&mut o));
            v.push(o);
        }
        v
    };
    assert_eq!(run(0), run(1));
    assert_ne!(run(2), run(1));
}

#[test]
fn sym_initstate_seed_zero_is_one() {
    let _g = global();
    let mut mine = Box::new([0u8; 128]);
    let p = mine.as_mut_ptr().cast::<c_char>();
    let prev = s::initstate(0, p, 128);
    let x: Vec<i64> = (0..4).map(|_| s::random()).collect();
    s::initstate(1, p, 128);
    let y: Vec<i64> = (0..4).map(|_| s::random()).collect();
    s::setstate(prev);
    assert_eq!(x, y);
}

#[test]
fn sym_srandom_r() {
    let mut state = [0u8; 64];
    let mut d = rdata();
    s::initstate_r(1, state.as_mut_ptr().cast(), 64, Some(&mut d));
    let mut a = 0;
    s::random_r(Some(&mut d), Some(&mut a));
    assert_eq!(s::srandom_r(1, Some(&mut d)), 0);
    let mut b = 0;
    s::random_r(Some(&mut d), Some(&mut b));
    assert_eq!(a, b, "reseeding restarts the sequence");
}

#[test]
fn sym_initstate_r() {
    let mut state = [0u8; 256];
    let mut d = rdata();
    assert_eq!(s::initstate_r(2, state.as_mut_ptr().cast(), 256, Some(&mut d)), 0);
    assert_eq!((d.rand_type, d.rand_deg, d.rand_sep), (4, 63, 1), "256 bytes: the largest type");
    let mut tiny = [0u8; 4];
    errno::set_raw(0);
    assert_eq!(s::initstate_r(2, tiny.as_mut_ptr().cast(), 4, Some(&mut d)), -1);
    assert_eq!(errno::get(), EINVAL);
}

#[test]
fn sym_setstate_r() {
    let mut s1 = [0u8; 32];
    let mut d = rdata();
    s::initstate_r(4, s1.as_mut_ptr().cast(), 32, Some(&mut d));
    let mut other = rdata();
    assert_eq!(s::setstate_r(s1.as_mut_ptr().cast(), Some(&mut other)), 0);
    let (mut a, mut b) = (0, 0);
    s::random_r(Some(&mut other), Some(&mut a));
    s::srandom_r(4, Some(&mut d));
    s::random_r(Some(&mut d), Some(&mut b));
    assert_eq!(a, b, "the buffer carries the generator");
    assert_eq!(other.rand_type, 1);
}

// ------------------------------------------------------------------------------ 48-bit family

#[test]
fn sym_srand48() {
    let _g = global();
    s::srand48(0x1234_5678);
    let x0 = (0x1234_5678u64 << 16) | 0x330E;
    assert_eq!(s::lrand48(), (step(x0) >> 17) as i64, "X = seed << 16 | 0x330E");
}

#[test]
fn sym_drand48() {
    let _g = global();
    s::srand48(1);
    let x1 = step((1 << 16) | 0x330E);
    assert_eq!(s::drand48(), x1 as f64 / (1u64 << 48) as f64);
    let v = s::drand48();
    assert!((0.0..1.0).contains(&v));
}

#[test]
fn sym_lrand48() {
    let _g = global();
    s::srand48(-1);
    let x0 = (0xFFFF_FFFFu64 << 16) | 0x330E;
    let x1 = step(x0);
    assert_eq!(s::lrand48(), (x1 >> 17) as i64);
    assert_eq!(s::lrand48(), (step(x1) >> 17) as i64);
}

#[test]
fn sym_mrand48() {
    let _g = global();
    s::srand48(77);
    let x1 = step((77 << 16) | 0x330E);
    assert_eq!(s::mrand48(), i64::from((x1 >> 16) as u32 as i32));
}

#[test]
fn sym_erand48() {
    let _g = global();
    s::srand48(0);
    let mut x = [1u16, 2, 3];
    let want = step(x48([1, 2, 3]));
    assert_eq!(s::erand48(Some(&mut x)), want as f64 / (1u64 << 48) as f64);
    assert_eq!(x48(x), want, "the caller's state advances");
}

#[test]
fn sym_nrand48() {
    let _g = global();
    s::srand48(0);
    let mut x = [9u16, 9, 9];
    let want = step(x48([9, 9, 9]));
    assert_eq!(s::nrand48(Some(&mut x)), (want >> 17) as i64);
}

#[test]
fn sym_jrand48() {
    let _g = global();
    s::srand48(0);
    let mut x = [0xFFFFu16, 0xFFFF, 0xFFFF];
    let want = step(MASK48);
    assert_eq!(s::jrand48(Some(&mut x)), i64::from((want >> 16) as u32 as i32));
}

#[test]
fn sym_seed48() {
    let _g = global();
    let a = [1u16, 2, 3];
    s::seed48(Some(&a));
    let b = [4u16, 5, 6];
    let old = s::seed48(Some(&b));
    let mut copy = [0u16; 3];
    liella_boundary::posix::s04_strings::memcpy(copy.as_mut_ptr().cast(), old.cast_const().cast(), 6);
    assert_eq!(copy, [1, 2, 3], "a static copy of the previous state");
    assert_eq!(s::lrand48(), (step(x48([4, 5, 6])) >> 17) as i64);
}

#[test]
fn sym_lcong48() {
    let _g = global();
    let p = [1u16, 0, 0, 3, 0, 0, 5];
    s::lcong48(Some(&p));
    assert_eq!(s::lrand48(), ((3 + 5) >> 17) as i64, "X = 1, a = 3, c = 5: a·X + c");
    assert_eq!(s::lrand48(), (((3 * 8 + 5) & MASK48) >> 17) as i64);
    s::srand48(0);
    let x1 = step(0x330E);
    assert_eq!(s::lrand48(), (x1 >> 17) as i64, "srand48 restores the default parameters");
}

#[test]
fn sym_drand48_r() {
    let mut d = d48();
    let mut out = 0.0;
    assert_eq!(s::drand48_r(Some(&mut d), Some(&mut out)), 0);
    assert_eq!(out, step(0) as f64 / (1u64 << 48) as f64, "a zeroed buffer starts at 0 with the default parameters");
}

#[test]
fn sym_erand48_r() {
    let mut d = d48();
    let mut x = [5u16, 0, 0];
    let mut out = 0.0;
    assert_eq!(s::erand48_r(Some(&mut x), Some(&mut d), Some(&mut out)), 0);
    assert_eq!(x48(x), step(5));
}

#[test]
fn sym_lrand48_r() {
    let mut d = d48();
    s::srand48_r(3, Some(&mut d));
    let mut out = 0;
    assert_eq!(s::lrand48_r(Some(&mut d), Some(&mut out)), 0);
    assert_eq!(out, (step((3 << 16) | 0x330E) >> 17) as i64);
}

#[test]
fn sym_nrand48_r() {
    let mut d = d48();
    let mut x = [0u16, 1, 0];
    let mut out = 0;
    s::nrand48_r(Some(&mut x), Some(&mut d), Some(&mut out));
    assert_eq!(out, (step(1 << 16) >> 17) as i64);
}

#[test]
fn sym_mrand48_r() {
    let mut d = d48();
    let mut out = 0;
    s::mrand48_r(Some(&mut d), Some(&mut out));
    assert_eq!(out, i64::from((step(0) >> 16) as u32 as i32));
}

#[test]
fn sym_jrand48_r() {
    let mut d = d48();
    let mut x = [2u16, 0, 0];
    let mut out = 0;
    s::jrand48_r(Some(&mut x), Some(&mut d), Some(&mut out));
    assert_eq!(out, i64::from((step(2) >> 16) as u32 as i32));
}

#[test]
fn sym_srand48_r() {
    let mut d = d48();
    assert_eq!(s::srand48_r(0x10, Some(&mut d)), 0);
    assert_eq!((x48(d.__x), d.__a, d.__c, d.__init), ((0x10 << 16) | 0x330E, A, 0xB, 1));
}

#[test]
fn sym_seed48_r() {
    let mut d = d48();
    s::srand48_r(1, Some(&mut d));
    let seed = [7u16, 8, 9];
    assert_eq!(s::seed48_r(Some(&seed), Some(&mut d)), 0);
    assert_eq!((d.__x, x48(d.__old_x)), ([7, 8, 9], (1 << 16) | 0x330E));
}

#[test]
fn sym_lcong48_r() {
    let mut d = d48();
    let p = [1u16, 0, 0, 2, 0, 0, 3];
    assert_eq!(s::lcong48_r(Some(&p), Some(&mut d)), 0);
    let mut out = 0;
    s::lrand48_r(Some(&mut d), Some(&mut out));
    assert_eq!((d.__a, d.__c, x48(d.__x)), (2, 3, 5), "the caller's parameters");
}
