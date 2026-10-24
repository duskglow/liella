//! Equivalence tests for the legacy compatibility entry points (COLD-REWRITE-T-06):
//! `prng_global`, `prng_r`, `string_compat`, `ipc_compat`, `cred_compat`. They pin the
//! observable behaviour through the public interfaces, from the spec clauses named on each
//! test and the man pages; they were written and run against the code before its rewrite.
//!
//! Traces: s9_2_c_0, s9_3_c_0, s9_3_f_0, s9_4_c_6, s9_4_c_7, s4_11_c_1, s4_11_c_2, s17_1_c_0,
//! s17_1_g_0, s25_2_c_1.
#![allow(clippy::arithmetic_side_effects, clippy::unwrap_used, clippy::indexing_slicing, clippy::as_conversions, clippy::float_cmp, missing_docs)]

use std::sync::Mutex;

use liella::random::{AdditiveFeedback, ByteWords, Rng48, RNG48_A, RNG48_C, SIZES, TYPES};
use liella_abi::layout::{struct_drand48_data, struct_random_data};
use liella_boundary::posix::s25_ipc as x;
use liella_boundary::sys::CSys;
use liella_kernel::Errno;
use liella_legacy::ipc_compat::{self, CtlShape};
use liella_legacy::prng_global::{self, CMem};
use liella_legacy::{cred_compat, prng_r, string_compat};

static GLOBAL: Mutex<()> = Mutex::new(());

fn global() -> std::sync::MutexGuard<'static, ()> {
    GLOBAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Caller memory for state buffers: `(base address, bytes)` regions.
#[derive(Default)]
struct VecMem(Vec<(usize, Vec<u8>)>);

impl VecMem {
    fn add(&mut self, base: usize, len: usize) {
        self.0.push((base, vec![0xa5; len]));
    }
    fn get(&self, base: usize) -> &[u8] {
        &self.0.iter().find(|(b, _)| *b == base).unwrap().1
    }
}

impl CMem for &mut VecMem {
    fn bytes(&mut self, addr: usize, len: usize) -> Option<&mut [u8]> {
        let (b, v) = self.0.iter_mut().find(|(b, v)| addr >= *b && addr + len <= *b + v.len())?;
        Some(&mut v[addr - *b..addr - *b + len])
    }
}

struct NoMem;

impl CMem for NoMem {
    fn bytes(&mut self, _addr: usize, _len: usize) -> Option<&mut [u8]> {
        None
    }
}

const M48: u64 = (1 << 48) - 1;

fn lcg48(x: u64, a: u64, c: u64) -> u64 {
    a.wrapping_mul(x).wrapping_add(c) & M48
}

fn x48(w: [u16; 3]) -> u64 {
    u64::from(w[0]) | u64::from(w[1]) << 16 | u64::from(w[2]) << 32
}

/// The sequence the additive-feedback generator of type `t` gives for `seed` (the model).
fn model(size: usize, seed: u32, n: usize) -> Vec<u32> {
    let mut buf = vec![0u8; size];
    AdditiveFeedback::seed(&mut ByteWords(&mut buf), AdditiveFeedback::type_for(size).unwrap(), seed);
    (0..n).map(|_| AdditiveFeedback::next(&mut ByteWords(&mut buf))).collect()
}

// ---- prng_global: the 48-bit family ----

fn chk_9_4_c_global_family() {
    let _g = global();
    // srand48: X = seed << 16 | 0x330E, default a and c.
    prng_global::srand48(0x1234_5678);
    let mut x = 0x1234_5678u64 << 16 | 0x330e;
    for _ in 0..4 {
        x = lcg48(x, RNG48_A, u64::from(RNG48_C));
        assert_eq!(prng_global::lrand48(), (x >> 17) as i64);
    }
    // The three value shapes come from the same step.
    prng_global::srand48(-1);
    let x1 = lcg48(0xffff_ffffu64 << 16 | 0x330e, RNG48_A, u64::from(RNG48_C));
    assert_eq!(prng_global::mrand48(), i64::from((x1 >> 16) as u32 as i32));
    prng_global::srand48(77);
    let x1 = lcg48(77 << 16 | 0x330e, RNG48_A, u64::from(RNG48_C));
    assert_eq!(prng_global::drand48(), x1 as f64 / (1u64 << 48) as f64);
    // The caller-state forms advance the caller's state with the global parameters and leave
    // the global state alone.
    prng_global::srand48(5);
    let mut st = [1u16, 2, 3];
    let want = lcg48(x48(st), RNG48_A, u64::from(RNG48_C));
    assert_eq!(prng_global::nrand48(&mut st), (want >> 17) as i64);
    assert_eq!(x48(st), want);
    let want2 = lcg48(want, RNG48_A, u64::from(RNG48_C));
    assert_eq!(prng_global::jrand48(&mut st), i64::from((want2 >> 16) as u32 as i32));
    let want3 = lcg48(want2, RNG48_A, u64::from(RNG48_C));
    assert_eq!(prng_global::erand48(&mut st), want3 as f64 / (1u64 << 48) as f64);
    let g1 = lcg48(5 << 16 | 0x330e, RNG48_A, u64::from(RNG48_C));
    assert_eq!(prng_global::lrand48(), (g1 >> 17) as i64, "the global state was not advanced by the caller forms");
}

fn chk_9_4_c_global_seed48_lcong48() {
    let _g = global();
    // seed48 returns the previous state and installs the new one with default parameters.
    prng_global::srand48(9);
    let before = 9u64 << 16 | 0x330e;
    let old = prng_global::seed48([1, 2, 3]);
    assert_eq!(x48(old), before);
    let x1 = lcg48(x48([1, 2, 3]), RNG48_A, u64::from(RNG48_C));
    assert_eq!(prng_global::lrand48(), (x1 >> 17) as i64);
    // The C form returns the address of one static copy of the previous state.
    let a1 = prng_global::seed48_addr(&[7, 8, 9]);
    let a2 = prng_global::seed48_addr(&[4, 5, 6]);
    assert_ne!(a1, 0);
    assert_eq!(a1, a2, "one static copy");
    assert_eq!(prng_global::SEED48_OLD.iter().map(|w| w.load(std::sync::atomic::Ordering::Relaxed)).collect::<Vec<_>>(), [7, 8, 9]);
    // lcong48 installs state, multiplier and addend; later caller-state forms use them.
    prng_global::lcong48([1, 0, 0, 5, 0, 0, 3]);
    let x1 = lcg48(1, 5, 3);
    assert_eq!(prng_global::lrand48(), (x1 >> 17) as i64);
    let mut st = [0u16; 3];
    assert_eq!(x48({ prng_global::nrand48(&mut st); st }), 3, "caller state uses the installed a and c");
    prng_global::lcong48_from(&[1, 0, 0, 5, 0, 0, 3]);
    assert_eq!(prng_global::lrand48(), (x1 >> 17) as i64, "lcong48_from is lcong48");
    // srand48 restores the default parameters.
    prng_global::srand48(2);
    let x1 = lcg48(2 << 16 | 0x330e, RNG48_A, u64::from(RNG48_C));
    assert_eq!(prng_global::lrand48(), (x1 >> 17) as i64);
    prng_global::srand48(2);
    assert_eq!(prng_global::next48(), x1);
}

// ---- prng_global: random / rand / initstate / setstate ----

fn chk_9_2_c_global_default_state() {
    let _g = global();
    prng_global::srandom(42, NoMem);
    let want = model(128, 42, 6);
    let got: Vec<u32> = (0..6).map(|_| u32::try_from(prng_global::random(NoMem)).unwrap()).collect();
    assert_eq!(got, want, "the default state is the 128-byte type");
    prng_global::srandom(42, NoMem);
    let r: Vec<u32> = (0..6).map(|_| u32::try_from(prng_global::rand(NoMem)).unwrap()).collect();
    assert_eq!(r, want, "rand draws from the random() stream");
    prng_global::srandom(0, NoMem);
    let z: Vec<i64> = (0..3).map(|_| prng_global::random(NoMem)).collect();
    prng_global::srandom(0, NoMem);
    assert_eq!(z, (0..3).map(|_| prng_global::random(NoMem)).collect::<Vec<_>>(), "seed 0 is a seed like any other");
    assert_ne!(prng_global::default_state_addr(), 0);
}

fn chk_9_3_c_global_caller_state() {
    let _g = global();
    prng_global::srandom(1, NoMem);
    let default = prng_global::default_state_addr();
    let mut mem = VecMem::default();
    for (i, &size) in SIZES.iter().enumerate() {
        let base = 0x10_0000 * (i + 1);
        mem.add(base, size);
        let prev = prng_global::initstate(3 + i as u32, base, size, &mut mem).unwrap();
        assert!(prev == default || SIZES.iter().enumerate().any(|(j, _)| prev == 0x10_0000 * (j + 1)));
        let want = model(size, 3 + i as u32, 8);
        let got: Vec<u32> = (0..8).map(|_| u32::try_from(prng_global::random(&mut mem)).unwrap()).collect();
        assert_eq!(got, want, "{size} bytes");
        assert_ne!(mem.get(base), vec![0xa5; size].as_slice(), "the state lives in the caller's buffer");
    }
    // setstate returns the previous state's address and resumes the named buffer where it stopped.
    let base128 = 0x10_0000 * 4;
    let prev = prng_global::setstate(0x10_0000); // the 8-byte buffer
    assert_eq!(prev, 0x10_0000 * 5, "the last initstate buffer was current");
    prng_global::setstate(base128);
    let cont = model(128, 3 + 3, 20);
    let got: Vec<u32> = (0..8).map(|_| u32::try_from(prng_global::random(&mut mem)).unwrap()).collect();
    assert_eq!(got, cont[8..16], "continues from its saved indices");
    // Back to the default state.
    assert_eq!(prng_global::setstate(default), base128, "previous was the 128 buffer");
    prng_global::srandom(1, NoMem);
    // Failure: fewer than 8 bytes leaves the current state alone.
    assert_eq!(prng_global::initstate(1, 0x1000, 7, NoMem), Err(Errno::EINVAL));
    let r = prng_global::random(NoMem);
    prng_global::srandom(1, NoMem);
    assert_eq!(prng_global::random(NoMem), r);
}

fn chk_9_2_t_global_rand_r() {
    let mut a = 5u32;
    let mut b = 5u32;
    let v = prng_global::rand_r(&mut a);
    assert_eq!(v, i32::try_from(liella::random::rand_r(&mut b)).unwrap());
    assert_eq!(a, b);
}

// ---- prng_r ----

fn chk_9_4_c_r_family() {
    let mut d: struct_drand48_data = zeroed_drand();
    let mut out = 0i64;
    // A zero-initialised buffer: state 0, default parameters.
    assert_eq!(prng_r::lrand48_r(&mut d, &mut out), 0);
    assert_eq!(out, (lcg48(0, RNG48_A, u64::from(RNG48_C)) >> 17) as i64);
    assert_eq!(prng_r::srand48_r(0x1111, &mut d), 0);
    let x1 = lcg48(0x1111 << 16 | 0x330e, RNG48_A, u64::from(RNG48_C));
    assert_eq!(prng_r::mrand48_r(&mut d, &mut out), 0);
    assert_eq!(out, i64::from((x1 >> 16) as u32 as i32));
    let mut f = 0f64;
    let x2 = lcg48(x1, RNG48_A, u64::from(RNG48_C));
    assert_eq!(prng_r::drand48_r(&mut d, &mut f), 0);
    assert_eq!(f, x2 as f64 / (1u64 << 48) as f64);
    // The caller-state forms use the buffer's parameters and leave its state.
    let mut st = [1u16, 0, 0];
    assert_eq!(prng_r::nrand48_r(&mut st, &mut d, &mut out), 0);
    assert_eq!(out, (lcg48(1, RNG48_A, u64::from(RNG48_C)) >> 17) as i64);
    assert_eq!(x48(st), lcg48(1, RNG48_A, u64::from(RNG48_C)));
    assert_eq!(prng_r::jrand48_r(&mut st, &mut d, &mut out), 0);
    assert_eq!(prng_r::erand48_r(&mut st, &mut d, &mut f), 0);
    assert_eq!(f, x48(st) as f64 / (1u64 << 48) as f64);
    let x3 = lcg48(x2, RNG48_A, u64::from(RNG48_C));
    assert_eq!(prng_r::lrand48_r(&mut d, &mut out), 0);
    assert_eq!(out, (x3 >> 17) as i64, "unchanged by the caller-state forms");
    // seed48_r keeps the previous state in __old_x and resets the parameters; lcong48_r sets them.
    assert_eq!(prng_r::seed48_r(&[1, 2, 3], &mut d), 0);
    assert_eq!(d.__old_x, [x3 as u16, (x3 >> 16) as u16, (x3 >> 32) as u16]);
    assert_eq!(d.__x, [1, 2, 3]);
    assert_eq!(prng_r::lcong48_r(&[1, 0, 0, 5, 0, 0, 3], &mut d), 0);
    assert_eq!(prng_r::lrand48_r(&mut d, &mut out), 0);
    assert_eq!(out, (lcg48(1, 5, 3) >> 17) as i64);
    assert_eq!(prng_r::srand48_r(4, &mut d), 0);
    assert_eq!(prng_r::lrand48_r(&mut d, &mut out), 0);
    assert_eq!(out, (lcg48(4 << 16 | 0x330e, RNG48_A, u64::from(RNG48_C)) >> 17) as i64, "srand48_r restores the defaults");
    let mut g = Rng48::UNSEEDED;
    g.srand48(4);
    assert_eq!(x48(g.x), 4 << 16 | 0x330e);
}

fn zeroed_drand() -> struct_drand48_data {
    struct_drand48_data { __x: [0; 3], __old_x: [0; 3], __c: 0, __init: 0, __a: 0 }
}

fn zeroed_random() -> struct_random_data {
    struct_random_data { fptr: 0, rptr: 0, state: 0, rand_type: 0, rand_deg: 0, rand_sep: 0, end_ptr: 0 }
}

fn chk_9_3_c_r_family() {
    let mut mem = VecMem::default();
    let mut out = 0i32;
    // No state described: EINVAL.
    let mut rd = zeroed_random();
    assert_eq!(prng_r::random_r(&mut rd, &mut out, &mut mem), Err(Errno::EINVAL));
    assert_eq!(prng_r::srandom_r(1, &mut rd, &mut mem), Err(Errno::EINVAL));
    // Too small a buffer.
    assert_eq!(prng_r::initstate_r(1, 0x2000, 7, &mut rd, &mut mem), Err(Errno::EINVAL));
    for (i, &size) in SIZES.iter().enumerate() {
        let base = 0x4_0000 * (i + 1);
        mem.add(base, size);
        let mut rd = zeroed_random();
        assert_eq!(prng_r::initstate_r(11, base, size, &mut rd, &mut mem), Ok(()));
        assert_eq!(rd.state, base);
        assert_eq!(rd.rand_type, i as i32);
        assert_eq!((rd.rand_deg as usize, rd.rand_sep as usize), TYPES[i]);
        let want = model(size, 11, 8);
        let got: Vec<u32> = (0..8).map(|_| { prng_r::random_r(&mut rd, &mut out, &mut mem).unwrap(); out as u32 }).collect();
        assert_eq!(got, want, "{size} bytes");
        // srandom_r reseeds the described generator.
        assert_eq!(prng_r::srandom_r(11, &mut rd, &mut mem), Ok(()));
        prng_r::random_r(&mut rd, &mut out, &mut mem).unwrap();
        assert_eq!(out as u32, want[0]);
        // A second descriptor over the same buffer continues the same stream (setstate_r).
        let mut rd2 = zeroed_random();
        assert_eq!(prng_r::setstate_r(base, &mut rd2, &mut mem), Ok(()));
        assert_eq!((rd2.state, rd2.rand_type, rd2.rand_deg, rd2.rand_sep), (rd.state, rd.rand_type, rd.rand_deg, rd.rand_sep));
        prng_r::random_r(&mut rd2, &mut out, &mut mem).unwrap();
        assert_eq!(out as u32, want[1], "the position lives in the buffer, not the descriptor");
    }
    // A buffer without a valid header type is refused.
    mem.add(0x40_0000, 128);
    // The header's type decides the layout; a type beyond the largest is the largest.
    for (t, want) in [(0u32, (0, 0, 0)), (1, (1, 7, 3)), (2, (2, 15, 1)), (3, (3, 31, 3)), (4, (4, 63, 1)), (5, (4, 63, 1)), (7, (4, 63, 1))] {
        mem.0.last_mut().unwrap().1[..4].copy_from_slice(&t.to_ne_bytes());
        let mut rd = zeroed_random();
        assert_eq!(prng_r::setstate_r(0x40_0000, &mut rd, &mut mem), Ok(()), "header type {t}");
        assert_eq!((rd.rand_type, rd.rand_deg, rd.rand_sep), want, "header type {t}");
    }
    // Unusable caller memory is reported, not ignored.
    let mut rd = zeroed_random();
    let r1 = prng_r::initstate_r(1, 0x9000_0000, 128, &mut rd, NoMem);
    let r2 = prng_r::setstate_r(0x9000_0000, &mut rd, NoMem);
    let r3 = prng_r::random_r(&mut rd, &mut out, NoMem);
    let r4 = prng_global::initstate(1, 0x9000_0000, 128, NoMem);
    assert_eq!((r1, r2, r3, r4), (Err(Errno::EFAULT), Err(Errno::EFAULT), Err(Errno::EINVAL), Err(Errno::EFAULT)));
}

// ---- string_compat ----

fn chk_4_11_c_steps() {
    let mut s = *b"::ab::cd";
    assert_eq!(string_compat::strtok_step(&mut s, |c| c == b':'), (Some(2), 5));
    assert_eq!(&s, b"::ab\0:cd");
    let mut t = *b"cd";
    assert_eq!(string_compat::strtok_step(&mut t, |c| c == b':'), (Some(0), 2));
    assert_eq!(string_compat::strtok_step(&mut [b':'; 3], |c| c == b':'), (None, 3));
    assert_eq!(string_compat::strtok_step::<u8>(&mut [], |c| c == b':'), (None, 0));
    let mut w = [i32::from(b'a'), i32::from(b','), i32::from(b'b')];
    assert_eq!(string_compat::strtok_step(&mut w, |c| c == i32::from(b',')), (Some(0), 2));
    assert_eq!(w, [i32::from(b'a'), 0, i32::from(b'b')]);
    let mut u = *b"a:b";
    assert_eq!(string_compat::strsep_step(&mut u, |c| c == b':'), Some(2));
    assert_eq!(&u, b"a\0b");
    let mut ab = *b"ab";
    assert_eq!(string_compat::strsep_step(&mut ab, |c| c == b':'), None);
    let mut e = *b":x";
    assert_eq!(string_compat::strsep_step(&mut e, |c| c == b':'), Some(1));
    assert_eq!(&e, b"\0x");
    assert_eq!(string_compat::strsep_step::<u8>(&mut [], |c| c == b':'), None);
}

fn chk_4_11_c_global_cursor() {
    use string_compat::Cursor;
    let _g = global();
    let mut c = string_compat::GlobalCursor;
    c.set(0x1234);
    assert_eq!(c.get(), 0x1234);
    assert_eq!(string_compat::STRTOK_CURSOR.load(std::sync::atomic::Ordering::Relaxed), 0x1234);
    c.set(0);
    assert_eq!(c.get(), 0);
}

// ---- ipc_compat ----

fn shapes_check() {
    const IPC_RMID: i32 = 0;
    const IPC_SET: i32 = 1;
    const IPC_STAT: i32 = 2;
    const IPC_INFO: i32 = 3;
    use CtlShape::{Ds, Info, None as No, Val, Values};
    let sem = [(IPC_RMID, No), (IPC_SET, Ds), (IPC_STAT, Ds), (IPC_INFO, Info(40)), (11, No), (12, No), (13, Values), (14, No), (15, No), (16, Val), (17, Values), (18, Ds), (19, Info(40)), (20, Ds)];
    for (cmd, want) in sem {
        assert_eq!(ipc_compat::sem_shape(cmd), Some(want), "semctl {cmd}");
        assert_eq!(ipc_compat::sem_shape(cmd | ipc_compat::IPC_64), Some(want), "semctl {cmd}|IPC_64");
    }
    let msg = [(IPC_RMID, No), (IPC_SET, Ds), (IPC_STAT, Ds), (IPC_INFO, Info(32)), (11, Ds), (12, Info(32)), (13, Ds)];
    for (cmd, want) in msg {
        assert_eq!(ipc_compat::msg_shape(cmd), Some(want), "msgctl {cmd}");
        assert_eq!(ipc_compat::msg_shape(cmd | ipc_compat::IPC_64), Some(want), "msgctl {cmd}|IPC_64");
    }
    let shm = [(IPC_RMID, No), (IPC_SET, Ds), (IPC_STAT, Ds), (IPC_INFO, Info(72)), (11, No), (12, No), (13, Ds), (14, Info(48)), (15, Ds)];
    for (cmd, want) in shm {
        assert_eq!(ipc_compat::shm_shape(cmd), Some(want), "shmctl {cmd}");
        assert_eq!(ipc_compat::shm_shape(cmd | ipc_compat::IPC_64), Some(want), "shmctl {cmd}|IPC_64");
    }
    for bad in [-1, 99, 21, 0x7f] {
        assert_eq!(ipc_compat::sem_shape(bad), None, "semctl {bad}");
        assert_eq!(ipc_compat::msg_shape(bad), None, "msgctl {bad}");
        assert_eq!(ipc_compat::shm_shape(bad), None, "shmctl {bad}");
    }
    assert_eq!(ipc_compat::msg_bytes(0), Ok(8));
    assert_eq!(ipc_compat::msg_bytes(100), Ok(108));
    assert!(ipc_compat::msg_bytes(usize::MAX).is_err());
}

#[test]
fn s25_2_c_1() {
    shapes_check();
    ctl_check();
    ftok_check();
}

fn ftok_check() {
    use std::os::unix::fs::MetadataExt;
    let exe = std::env::current_exe().unwrap();
    let md = std::fs::metadata(&exe).unwrap();
    let c = std::ffi::CString::new(exe.to_str().unwrap()).unwrap();
    let path = liella_kernel::path::KStr::from_nul_terminated(c.as_bytes_with_nul()).unwrap();
    for id in [0, 1, 0x7f, 0xff, 0x1ab, -1] {
        let want = (md.ino() & 0xffff) as i32 | ((md.dev() & 0xff) as i32) << 16 | (id & 0xff) << 24;
        assert_eq!(ipc_compat::ftok(&CSys, path, id), Ok(want), "id {id}");
    }
}

fn ctl_check() {
    // semctl: GETALL/SETALL need the set's size; msgctl/shmctl STAT decode into the kernel structures.
    const IPC_RMID: i32 = 0;
    const IPC_STAT: i32 = 2;
    const GETALL: i32 = 13;
    const SETALL: i32 = 17;
    let id = x::semget(0, 3, 0o600);
    assert!(id >= 0);
    let mut vals = [0u16; 3];
    assert_eq!(ipc_compat::nsems(&CSys, id), Ok(3));
    assert_eq!(ipc_compat::semctl(&CSys, id, 0, SETALL, ipc_compat::SemArg::Values(&mut [4, 5, 6])), Ok(0));
    assert_eq!(ipc_compat::semctl(&CSys, id, 0, GETALL, ipc_compat::SemArg::Values(&mut vals)), Ok(0));
    assert_eq!(vals, [4, 5, 6]);
    let mut ds = liella_kernel::uapi::SemidDs::default();
    assert_eq!(ipc_compat::semctl(&CSys, id, 0, IPC_STAT, ipc_compat::SemArg::Ds(&mut ds)), Ok(0));
    assert_eq!(ds.nsems(), 3);
    assert_eq!(ipc_compat::semctl(&CSys, id, 0, IPC_RMID, ipc_compat::SemArg::None), Ok(0));
    assert!(ipc_compat::nsems(&CSys, id).is_err(), "removed");
    assert_eq!(ipc_compat::semctl(&CSys, id, 0, 99, ipc_compat::SemArg::None), Err(Errno::EINVAL));

    let q = x::msgget(0, 0o600);
    assert!(q >= 0);
    let mut mds = liella_kernel::uapi::MsqidDs::default();
    assert_eq!(ipc_compat::msgctl(&CSys, q, IPC_STAT, ipc_compat::CtlArg::Ds(&mut mds)), Ok(0));
    assert_eq!(ipc_compat::msgctl(&CSys, q, 3, ipc_compat::CtlArg::Info(&mut [0u8; 32])).map(|_| ()), Ok(()));
    assert_eq!(ipc_compat::msgctl(&CSys, q, IPC_RMID, ipc_compat::CtlArg::None), Ok(0));
    let m = x::shmget(0, 4096, 0o600);
    assert!(m >= 0);
    let mut sds = liella_kernel::uapi::ShmidDs::default();
    assert_eq!(ipc_compat::shmctl(&CSys, m, IPC_STAT, ipc_compat::CtlArg::Ds(&mut sds)), Ok(0));
    assert_eq!(sds.segsz(), 4096);
    assert_eq!(ipc_compat::shmctl(&CSys, m, IPC_RMID, ipc_compat::CtlArg::None), Ok(0));
}

// ---- cred_compat ----

#[test]
fn s17_1_c_0() {
    let n = cred_compat::getgroups(&CSys, 0, &mut []).unwrap();
    let mut list = vec![0u32; n];
    assert_eq!(cred_compat::getgroups(&CSys, i32::try_from(n).unwrap(), &mut list), Ok(n));
    let egid = liella_boundary::posix::s17_users::getegid();
    assert_eq!(cred_compat::getgroups(&CSys, -1, &mut list), Err(Errno::EINVAL));
    if n > 0 {
        assert_eq!(cred_compat::getgroups(&CSys, i32::try_from(n - 1).unwrap(), &mut list[..n - 1]), Err(Errno::EINVAL));
    }
    // A list longer than the stack buffer takes the /proc scan; same answers.
    if n > 1 {
        for &g in &list {
            assert_eq!(cred_compat::in_group_buffered::<_, 1>(&CSys, g, u32::MAX - 1), Ok(true), "scan finds {g}");
        }
        assert_eq!(cred_compat::in_group_buffered::<_, 1>(&CSys, 4_000_000_001, 4_000_000_002), Ok(false));
    }
    for &g in &list {
        assert_eq!(cred_compat::in_group(&CSys, g, u32::MAX - 1), Ok(true), "supplementary {g}");
        assert_eq!(cred_compat::group_member(&CSys, g), Ok(true));
    }
    assert_eq!(cred_compat::in_group(&CSys, 4_000_000_001, 4_000_000_002), Ok(false));
    assert_eq!(cred_compat::in_group(&CSys, 4_000_000_002, 4_000_000_002), Ok(true), "the primary group counts");
    assert_eq!(cred_compat::group_member(&CSys, egid), Ok(true), "the effective GID counts");
    assert_eq!(cred_compat::group_member(&CSys, 4_000_000_001), Ok(false));
}

#[test]
fn s9_4_c_0() {
    chk_9_4_c_global_family();
    chk_9_4_c_global_seed48_lcong48();
    chk_9_4_c_r_family();
}

#[test]
fn s9_3_c_0() {
    chk_9_2_c_global_default_state();
    chk_9_3_c_global_caller_state();
    chk_9_2_t_global_rand_r();
    chk_9_3_c_r_family();
}

#[test]
fn s4_11_c_1() {
    chk_4_11_c_steps();
    chk_4_11_c_global_cursor();
}
