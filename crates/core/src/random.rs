//! Random numbers (spec §9; decisions D-9.1–D-9.3, OD-13).
//!
//! * Kernel randomness: [`fill`], [`entropy`], and the never-failing `arc4random` family
//!   ([`u32_random`], [`uniform`]), each taking the kernel as a value.
//! * [`Rng48`]: the POSIX 48-bit linear congruential generator, bit-exact (§9.4).
//! * [`AdditiveFeedback`]: Liella's documented additive lagged-Fibonacci generator behind
//!   `random`/`srandom`/`initstate`/`setstate` and `rand`. Per OD-13 its sequence is *not*
//!   glibc's (the spec does not specify one); its algorithm is fixed here so that sequences are
//!   reproducible across Liella versions.

#![allow(clippy::as_conversions, clippy::arithmetic_side_effects, clippy::indexing_slicing)] // modular generator arithmetic; table indices are reduced modulo the table size

use liella_kernel::errno::{Errno, KResult};
use liella_kernel::sys::Sys;
use liella_kernel::uapi::GRND_NONBLOCK;

/// Fill `buf` from the kernel CSPRNG, retrying short reads; `EINTR` is retried because the
/// callers' contracts never fail with it (`getentropy`, `arc4random`). Other errors propagate.
pub fn fill<S: Sys>(sys: &S, buf: &mut [u8]) -> KResult<()> {
    let mut done = 0;
    while done < buf.len() {
        match sys.getrandom(&mut buf[done..], 0) {
            Ok(0) => return Err(Errno::EIO), // a kernel that answers a non-empty request with nothing would spin us forever
            Ok(n) => done += n.min(buf.len() - done),
            Err(Errno::EINTR) => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// `getentropy`: exactly `buf.len() <= 256` bytes, or `EIO` (D-9.1: the 256-byte limit and
/// `EIO` are the stated glibc behaviour).
pub fn entropy<S: Sys>(sys: &S, buf: &mut [u8]) -> KResult<()> {
    if buf.len() > 256 {
        return Err(Errno::EIO);
    }
    fill(sys, buf).map_err(|_| Errno::EIO)
}

/// A uniformly distributed 32-bit value (`arc4random`). Each call takes its bytes from the
/// kernel, so the output is fork-safe (spec §9.1). `None` only if the kernel cannot supply
/// entropy, which the boundary turns into termination (the contract).
pub fn u32_random<S: Sys>(sys: &S) -> Option<u32> {
    let mut b = [0u8; 4];
    fill(sys, &mut b).ok()?;
    Some(u32::from_ne_bytes(b))
}

/// A uniform value in `[0, bound)` without modulo bias (`arc4random_uniform`); 0 for bounds 0
/// and 1. Multiply-shift: `x · bound` has the wanted value in its high word; the few low words
/// that would over-represent a value (`low < 2^32 mod bound`) are redrawn, so the common case
/// costs one multiply and no division.
pub fn uniform<S: Sys>(sys: &S, bound: u32) -> Option<u32> {
    if bound < 2 {
        return Some(0);
    }
    let mut m = u64::from(u32_random(sys)?) * u64::from(bound);
    if (m as u32) < bound {
        let floor = bound.wrapping_neg() % bound; // 2^32 mod bound
        while (m as u32) < floor {
            m = u64::from(u32_random(sys)?) * u64::from(bound);
        }
    }
    Some((m >> 32) as u32)
}

/// Whether the kernel pool is initialized (a non-blocking probe).
pub fn pool_ready<S: Sys>(sys: &S) -> bool {
    let mut b = [0u8; 1];
    sys.getrandom(&mut b, GRND_NONBLOCK) != Err(Errno::EAGAIN)
}

/// The default multiplier `a` (0x5DEECE66D) of §9.4.
pub const RNG48_A: u64 = 0x5_DEEC_E66D;
/// The default addend `c` (0xB).
pub const RNG48_C: u16 = 0xb;

/// The parameters and state of the 48-bit generator: `X(n+1) = (a·X(n) + c) mod 2^48`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rng48 {
    /// The 48-bit state as three 16-bit words, least significant first (the `xsubi` layout).
    pub x: [u16; 3],
    /// Multiplier (48 bits).
    pub a: u64,
    /// Addend.
    pub c: u16,
}

impl Rng48 {
    /// The state before any seeding call: X = 0x1234ABCD330E with default parameters, which
    /// is the state `srand48(0x1234ABCD)` produces (POSIX leaves the unseeded state open).
    pub const UNSEEDED: Rng48 = Rng48 { x: [0x330e, 0xabcd, 0x1234], a: RNG48_A, c: RNG48_C };

    /// Advance `state` with this generator's parameters and return the new 48-bit value.
    pub fn step(&self, state: &mut [u16; 3]) -> u64 {
        // Wrapping 64-bit arithmetic is exact modulo 2^48.
        let v = self.a.wrapping_mul(x48(*state)).wrapping_add(u64::from(self.c)) & MASK48;
        *state = split48(v);
        v
    }

    /// `drand48`/`erand48` from a 48-bit value: `X · 2^-48`, exact, in `[0.0, 1.0)`.
    pub fn to_double(v: u64) -> f64 {
        (v & MASK48) as f64 / (1u64 << 48) as f64
    }

    /// `lrand48`/`nrand48`: the high 31 bits, in `[0, 2^31)`.
    pub fn to_nonneg(v: u64) -> i64 {
        ((v & MASK48) >> 17) as i64
    }

    /// `mrand48`/`jrand48`: the high 32 bits as a signed value, in `[-2^31, 2^31)`.
    pub fn to_signed(v: u64) -> i64 {
        i64::from((v >> 16) as u32 as i32)
    }

    /// `srand48(seed)`: the high 32 state bits from the low 32 bits of `seed`, the low 16 bits
    /// 0x330E; default parameters.
    pub fn srand48(&mut self, seed: i64) {
        *self = Rng48 { x: split48((u64::from(seed as u32) << 16) | 0x330e), a: RNG48_A, c: RNG48_C };
    }

    /// `seed48(s)`: set the state, restore default parameters, return the previous state.
    pub fn seed48(&mut self, s: [u16; 3]) -> [u16; 3] {
        let old = self.x;
        *self = Rng48 { x: s, a: RNG48_A, c: RNG48_C };
        old
    }

    /// `lcong48(p)`: state `p[0..3]`, multiplier `p[3..6]`, addend `p[6]`.
    pub fn lcong48(&mut self, p: [u16; 7]) {
        *self = Rng48 { x: [p[0], p[1], p[2]], a: x48([p[3], p[4], p[5]]), c: p[6] };
    }

    /// Advance the generator's own state.
    #[allow(clippy::should_implement_trait)] // not an iterator: it has no end and takes no `Option`
    pub fn next(&mut self) -> u64 {
        let mut st = self.x;
        let v = self.step(&mut st);
        self.x = st;
        v
    }
}

const MASK48: u64 = (1 << 48) - 1;

/// Three state words (least significant first) as one 48-bit number.
fn x48(x: [u16; 3]) -> u64 {
    u64::from(x[0]) | u64::from(x[1]) << 16 | u64::from(x[2]) << 32
}

/// A 48-bit number as three state words.
fn split48(v: u64) -> [u16; 3] {
    [v as u16, (v >> 16) as u16, (v >> 32) as u16]
}

/// Word storage for the additive-feedback state (caller memory or the library default).
pub trait Words {
    /// Number of words.
    fn count(&self) -> usize;
    /// Word `i` (0 if out of range).
    fn word(&self, i: usize) -> u32;
    /// Set word `i` (ignored if out of range).
    fn set_word(&mut self, i: usize, v: u32);
}

impl Words for [u32] {
    fn count(&self) -> usize {
        self.len()
    }
    fn word(&self, i: usize) -> u32 {
        self.get(i).copied().unwrap_or(0)
    }
    fn set_word(&mut self, i: usize, v: u32) {
        if let Some(w) = self.get_mut(i) {
            *w = v;
        }
    }
}

/// Words over native-endian bytes (an `initstate` buffer).
pub struct ByteWords<'a>(pub &'a mut [u8]);

impl Words for ByteWords<'_> {
    fn count(&self) -> usize {
        self.0.len() / 4
    }
    fn word(&self, i: usize) -> u32 {
        match self.0.get(i * 4..i * 4 + 4) {
            Some(b) => u32::from_ne_bytes([b[0], b[1], b[2], b[3]]),
            None => 0,
        }
    }
    fn set_word(&mut self, i: usize, v: u32) {
        if let Some(b) = self.0.get_mut(i * 4..i * 4 + 4) {
            b.copy_from_slice(&v.to_ne_bytes());
        }
    }
}

/// Liella's additive-feedback generator (OD-13). Documented algorithm:
///
/// * **Types by state size** (bytes, including the header word): 8 → type 0 (a 31-bit LCG,
///   `x = x·1103515245 + 12345 mod 2^31`); 32 → lags (7, 3); 64 → (15, 1); 128 → (31, 3);
///   256 → (63, 1). A buffer of `n >= 8` bytes uses the largest type that fits.
/// * **Layout:** word 0 is the header: bits 0–2 the type, bits 8–15 the front index, bits
///   16–23 the rear index; words 1..=deg are the table.
/// * **Seeding:** the table is filled by SplitMix64 from the seed (low 32 bits of each
///   output), then `10·deg` outputs are discarded.
/// * **Output:** `t[f] += t[r]` (mod 2^32); the result is `t[f] >> 1` (31 bits); both indices
///   then advance modulo `deg`.
///
/// The lags are the exponents of primitive trinomials `x^deg + x^sep + 1` over GF(2), which is
/// what gives the recurrence its full period.
pub struct AdditiveFeedback;

/// (degree, separation) per type.
pub const TYPES: [(usize, usize); 5] = [(0, 0), (7, 3), (15, 1), (31, 3), (63, 1)];
/// Buffer bytes per type.
pub const SIZES: [usize; 5] = [8, 32, 64, 128, 256];

/// One SplitMix64 step.
fn splitmix(s: &mut u64) -> u64 {
    *s = s.wrapping_add(0x9e37_79b9_7f4a_7c15);
    let mut z = *s;
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

/// The decoded header: type, degree, separation, front and rear indices. Indices a damaged
/// header holds beyond the table are folded into it, and a type beyond the last is the last.
struct Header {
    t: usize,
    deg: usize,
    front: usize,
    rear: usize,
}

impl AdditiveFeedback {
    /// The type a buffer of `n` bytes holds, or `None` below 8 bytes (`EINVAL`).
    pub fn type_for(n: usize) -> Option<u32> {
        SIZES.iter().rposition(|&s| s <= n).map(|t| t as u32)
    }

    fn load<W: Words + ?Sized>(w: &W) -> Header {
        let h = w.word(0);
        let t = ((h & 7) as usize).min(TYPES.len() - 1);
        let deg = TYPES[t].0;
        let fold = |i: u32| if deg == 0 { 0 } else { i as usize % deg };
        Header { t, deg, front: fold((h >> 8) & 0xff), rear: fold((h >> 16) & 0xff) }
    }

    fn store<W: Words + ?Sized>(w: &mut W, t: usize, front: usize, rear: usize) {
        w.set_word(0, t as u32 | (front as u32) << 8 | (rear as u32) << 16);
    }

    /// The type recorded in an initialized state.
    pub fn type_of<W: Words + ?Sized>(w: &W) -> u32 {
        Self::load(w).t as u32
    }

    /// Initialize `w` as type `t` from `seed`.
    pub fn seed<W: Words + ?Sized>(w: &mut W, t: u32, seed: u32) {
        let t = (t as usize).min(TYPES.len() - 1);
        let (deg, sep) = TYPES[t];
        if deg == 0 {
            Self::store(w, 0, 0, 0);
            w.set_word(1, seed & 0x7fff_ffff);
            return;
        }
        let mut s = u64::from(seed);
        for i in 0..deg {
            w.set_word(1 + i, splitmix(&mut s) as u32);
        }
        Self::store(w, t, sep, 0);
        for _ in 0..10 * deg {
            Self::next(w);
        }
    }

    /// The next value in `[0, 2^31)`.
    #[allow(clippy::should_implement_trait)] // an associated function over a state, not an iterator
    pub fn next<W: Words + ?Sized>(w: &mut W) -> u32 {
        let Header { t, deg, front, rear } = Self::load(w);
        if deg == 0 {
            let x = w.word(1).wrapping_mul(1_103_515_245).wrapping_add(12345) & 0x7fff_ffff;
            w.set_word(1, x);
            return x;
        }
        let v = w.word(1 + front).wrapping_add(w.word(1 + rear));
        w.set_word(1 + front, v);
        Self::store(w, t, (front + 1) % deg, (rear + 1) % deg);
        v >> 1
    }
}

/// `rand_r`: Liella's documented 32-bit-state generator (OD-13): three steps of
/// `s = s·1103515245 + 12345 (mod 2^32)` contribute 11, 10 and 10 bits (bits 16 and up) of
/// output.
pub fn rand_r(seed: &mut u32) -> u32 {
    let mut s = *seed;
    let mut out = 0u32;
    for bits in [11u32, 10, 10] {
        s = s.wrapping_mul(1_103_515_245).wrapping_add(12345);
        out = (out << bits) | ((s >> 16) & ((1 << bits) - 1));
    }
    *seed = s;
    out & 0x7fff_ffff
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::indexing_slicing, clippy::arithmetic_side_effects, clippy::as_conversions, clippy::byte_char_slices)]
mod tests {
    use super::*;

    #[test]
    fn rng48_follows_the_posix_recurrence() {
        let mut r = Rng48::UNSEEDED;
        r.srand48(0);
        assert_eq!(x48(r.x), 0x330e);
        let v1 = r.next();
        assert_eq!(v1, (RNG48_A * 0x330e + 0xb) & ((1 << 48) - 1));
        assert!(Rng48::to_double(v1) < 1.0);
        assert_eq!(Rng48::to_nonneg((1u64 << 48) - 1), (1 << 31) - 1);
        assert_eq!(Rng48::to_signed(1u64 << 47), -(1 << 31));
    }

    #[test]
    fn additive_feedback_is_deterministic_and_resumable() {
        for size in SIZES {
            let t = AdditiveFeedback::type_for(size).unwrap();
            let mut a = vec![0u32; size / 4];
            AdditiveFeedback::seed(a.as_mut_slice(), t, 42);
            let mut b = a.clone();
            let seq_a: Vec<u32> = (0..100).map(|_| AdditiveFeedback::next(a.as_mut_slice())).collect();
            let seq_b: Vec<u32> = (0..100).map(|_| AdditiveFeedback::next(b.as_mut_slice())).collect();
            assert_eq!(seq_a, seq_b);
            assert!(seq_a.iter().all(|&v| v < 1 << 31));
        }
        assert_eq!(AdditiveFeedback::type_for(7), None);
        assert_eq!(AdditiveFeedback::type_for(300), Some(4));
    }
}
