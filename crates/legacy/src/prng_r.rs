//! The reentrant generator forms of spec §9.3–§9.4 over caller structures: `drand48_data`
//! (the 48-bit generator, bit-exact) and `random_data` (Liella's additive-feedback generator,
//! OD-13).
//!
//! These are the primitive forms; `prng_global` is the same arithmetic over library-owned
//! storage, and shares [`caller_words`] with this module.
//!
//! * A zero-initialised `drand48_data` takes the default parameters on first use (`__init`),
//!   as the C contract has it; its state starts at 0.
//! * `random_data.state` holds the address of the caller's state buffer itself (whose first
//!   word is the generator's header, `liella::random::AdditiveFeedback`): the generator's
//!   position lives in that buffer, so two descriptors over one buffer share a stream.
//!   `rand_type`, `rand_deg` and `rand_sep` describe the layout, and `fptr`/`rptr`/`end_ptr`
//!   bound it; the fields are opaque in the C contract (D-9.4).
//! * Caller memory that cannot be reached is `EFAULT`; a descriptor that describes no state
//!   is `EINVAL`.

use liella::error::{Errno, Result};
use liella::random::{AdditiveFeedback, ByteWords, Rng48, RNG48_A, RNG48_C, SIZES, TYPES};
use liella_abi::layout::{struct_drand48_data, struct_random_data};

use crate::prng_global::CMem;

// ---- the 48-bit generator over `drand48_data` ----

/// The generator `d` holds (defaults until the first call has initialised it).
fn rng(d: &struct_drand48_data) -> Rng48 {
    if d.__init == 0 {
        Rng48 { x: d.__x, a: RNG48_A, c: RNG48_C }
    } else {
        Rng48 { x: d.__x, a: d.__a, c: d.__c }
    }
}

fn put(d: &mut struct_drand48_data, r: Rng48) {
    d.__x = r.x;
    d.__a = r.a;
    d.__c = r.c;
    d.__init = 1;
}

/// Advance `d`'s own state.
fn next(d: &mut struct_drand48_data) -> u64 {
    let mut r = rng(d);
    let v = r.next();
    put(d, r);
    v
}

/// Advance the caller's `x` with `d`'s parameters; `d` keeps its own state.
fn step(x: &mut [u16; 3], d: &struct_drand48_data) -> u64 {
    rng(d).step(x)
}

/// `drand48_r`: 0.
pub fn drand48_r(d: &mut struct_drand48_data, out: &mut f64) -> i64 {
    *out = Rng48::to_double(next(d));
    0
}

/// `erand48_r`: advance `x` with `d`'s parameters; 0.
pub fn erand48_r(x: &mut [u16; 3], d: &mut struct_drand48_data, out: &mut f64) -> i64 {
    *out = Rng48::to_double(step(x, d));
    0
}

/// `lrand48_r`.
pub fn lrand48_r(d: &mut struct_drand48_data, out: &mut i64) -> i64 {
    *out = Rng48::to_nonneg(next(d));
    0
}

/// `nrand48_r`.
pub fn nrand48_r(x: &mut [u16; 3], d: &mut struct_drand48_data, out: &mut i64) -> i64 {
    *out = Rng48::to_nonneg(step(x, d));
    0
}

/// `mrand48_r`.
pub fn mrand48_r(d: &mut struct_drand48_data, out: &mut i64) -> i64 {
    *out = Rng48::to_signed(next(d));
    0
}

/// `jrand48_r`.
pub fn jrand48_r(x: &mut [u16; 3], d: &mut struct_drand48_data, out: &mut i64) -> i64 {
    *out = Rng48::to_signed(step(x, d));
    0
}

/// `srand48_r`.
pub fn srand48_r(seed: i64, d: &mut struct_drand48_data) -> i64 {
    let mut r = rng(d);
    r.srand48(seed);
    put(d, r);
    0
}

/// `seed48_r`: the previous state is kept in `__old_x`.
pub fn seed48_r(s: &[u16; 3], d: &mut struct_drand48_data) -> i64 {
    let mut r = rng(d);
    d.__old_x = r.seed48(*s);
    put(d, r);
    0
}

/// `lcong48_r`.
pub fn lcong48_r(p: &[u16; 7], d: &mut struct_drand48_data) -> i64 {
    let mut r = rng(d);
    r.lcong48(*p);
    put(d, r);
    0
}

// ---- the additive-feedback generator over `random_data` ----

/// The largest type's index; a header naming a type beyond it is read as this one.
const MAX_TYPE: usize = SIZES.len() - 1;

/// Run `f` on the generator state in caller memory at `addr`: the header word names the
/// type, which sizes the window. `f` also gets the type. `None` if the memory cannot be
/// reached. (Shared with `prng_global`, whose current state may be a caller buffer.)
pub(crate) fn caller_words<R>(mem: &mut dyn CMem, addr: usize, f: impl FnOnce(&mut ByteWords<'_>, usize) -> R) -> Option<R> {
    let head = mem.bytes(addr, 4)?;
    let t = usize::try_from(AdditiveFeedback::type_of(&ByteWords(head))).unwrap_or(MAX_TYPE).min(MAX_TYPE);
    let window = mem.bytes(addr, *SIZES.get(t)?)?;
    Some(f(&mut ByteWords(window), t))
}

/// Record in `rd` that the type-`t` state at `buf` is the generator it describes.
fn describe(rd: &mut struct_random_data, buf: usize, t: usize) {
    let (deg, sep) = TYPES.get(t).copied().unwrap_or((0, 0));
    let t32 = i32::try_from(t).unwrap_or(0);
    rd.state = buf;
    rd.rand_type = t32;
    rd.rand_deg = i32::try_from(deg).unwrap_or(0);
    rd.rand_sep = i32::try_from(sep).unwrap_or(0);
    // Front and rear pointers bound the table; the live indices are in the header word.
    rd.fptr = buf.wrapping_add(4);
    rd.rptr = buf.wrapping_add(4).wrapping_add(sep.wrapping_mul(4));
    rd.end_ptr = buf.wrapping_add(SIZES.get(t).copied().unwrap_or(0));
}

/// `random_r`: the next value of the generator `rd` describes; `EINVAL` if it describes none.
pub fn random_r(rd: &mut struct_random_data, out: &mut i32, mut mem: impl CMem) -> Result<()> {
    if rd.state == 0 {
        return Err(Errno::EINVAL);
    }
    let v = caller_words(&mut mem, rd.state, |w, _| AdditiveFeedback::next(w)).ok_or(Errno::EFAULT)?;
    *out = i32::try_from(v).unwrap_or(i32::MAX);
    Ok(())
}

/// `srandom_r`: reseed the generator `rd` describes.
pub fn srandom_r(seed: u32, rd: &mut struct_random_data, mut mem: impl CMem) -> Result<()> {
    if rd.state == 0 {
        return Err(Errno::EINVAL);
    }
    caller_words(&mut mem, rd.state, |w, t| AdditiveFeedback::seed(w, u32::try_from(t).unwrap_or(0), seed.max(1))).ok_or(Errno::EFAULT)
}

/// `initstate_r`: seed the `n`-byte buffer at `buf` and describe it in `rd`; `EINVAL` below
/// 8 bytes.
pub fn initstate_r(seed: u32, buf: usize, n: usize, rd: &mut struct_random_data, mut mem: impl CMem) -> Result<()> {
    let t = AdditiveFeedback::type_for(n).ok_or(Errno::EINVAL)?;
    let t = usize::try_from(t).map_err(|_| Errno::EINVAL)?;
    let window = mem.bytes(buf, *SIZES.get(t).ok_or(Errno::EINVAL)?).ok_or(Errno::EFAULT)?;
    AdditiveFeedback::seed(&mut ByteWords(window), u32::try_from(t).unwrap_or(0), seed.max(1));
    describe(rd, buf, t);
    Ok(())
}

/// `setstate_r`: describe the initialised buffer at `buf` in `rd`.
pub fn setstate_r(buf: usize, rd: &mut struct_random_data, mut mem: impl CMem) -> Result<()> {
    let head = mem.bytes(buf, 4).ok_or(Errno::EFAULT)?;
    let t = usize::try_from(AdditiveFeedback::type_of(&ByteWords(head))).unwrap_or(MAX_TYPE).min(MAX_TYPE);
    describe(rd, buf, t);
    Ok(())
}
