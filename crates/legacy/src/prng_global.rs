//! The global generator states of `rand`/`random`/`drand48` (spec §9.2–§9.4; D-9.3, OD-13).
//!
//! * `random`/`srandom`/`initstate`/`setstate` and `rand`/`srand` share one current state:
//!   either the library's default 128-byte state or a caller buffer from `initstate` (whose
//!   address is kept here, its bytes reached through the `mem` callback the boundary
//!   supplies). The default starts as if seeded with 1, on first use.
//! * `drand48` and friends share one `Rng48`.
//!
//! Design: the reentrant forms of `prng_r` are the primitives; this module is the same
//! arithmetic over library-owned storage plus the "current state" choice. The crate forbids
//! `unsafe`, so the storage is atomics, and one spin lock makes each call (a read-modify-write
//! of several words) atomic as a whole; the atomics themselves are accessed `Relaxed` because
//! the lock orders them. `rand` stays MT-safe (spec §9.2). The lock is the only
//! synchronisation: no call blocks or allocates, so these are async-signal-safe to the extent
//! the C contract requires (`hold` lets `fork` and the AS-safety harness own the lock).

use core::sync::atomic::{AtomicBool, AtomicU16, AtomicU32, AtomicU64, AtomicUsize, Ordering::Relaxed};

use liella::error::{Errno, Result};
use liella::random::{AdditiveFeedback, ByteWords, Rng48, Words, RNG48_A, RNG48_C, SIZES};

use crate::lock::SpinLock;
use crate::prng_r::caller_words;

/// Caller memory, which the boundary implements by converting addresses (category *slice*).
pub trait CMem {
    /// The `len` bytes at `addr`, or `None` for an unusable address.
    fn bytes(&mut self, addr: usize, len: usize) -> Option<&mut [u8]>;
}

static LOCK: SpinLock = SpinLock::new();

/// Hold the global generator's lock while the guard lives (`fork`, the AS-safety harness).
pub fn hold() -> crate::lock::Guard<'static> {
    LOCK.lock()
}

/// The type of the default state (128 bytes, 32 words).
const DEFAULT_TYPE: u32 = 3;
static DEFAULT: [AtomicU32; 32] = [const { AtomicU32::new(0) }; 32];
static DEFAULT_SEEDED: AtomicBool = AtomicBool::new(false);
/// The current caller buffer (0: the default state).
static CURRENT: AtomicUsize = AtomicUsize::new(0);

struct AtomicWords<'a>(&'a [AtomicU32]);

impl Words for AtomicWords<'_> {
    fn count(&self) -> usize {
        self.0.len()
    }
    fn word(&self, i: usize) -> u32 {
        self.0.get(i).map_or(0, |w| w.load(Relaxed))
    }
    fn set_word(&mut self, i: usize, v: u32) {
        if let Some(w) = self.0.get(i) {
            w.store(v, Relaxed);
        }
    }
}

/// The address the C side sees for the default state (what `initstate`/`setstate` return
/// when the default was current).
pub fn default_state_addr() -> usize {
    DEFAULT.as_ptr().addr()
}

/// The current state's address as the C side sees it.
fn current_addr() -> usize {
    match CURRENT.load(Relaxed) {
        0 => default_state_addr(),
        a => a,
    }
}

/// Run `f` (with the state's type) on the current state, under the lock the caller holds.
/// `EFAULT` if the current state is a caller buffer that cannot be reached.
fn on_current<R>(mem: &mut dyn CMem, f: impl FnOnce(&mut dyn Words, u32) -> R) -> Result<R> {
    match CURRENT.load(Relaxed) {
        0 => {
            let mut w = AtomicWords(&DEFAULT);
            if !DEFAULT_SEEDED.swap(true, Relaxed) {
                AdditiveFeedback::seed(&mut w, DEFAULT_TYPE, 1);
            }
            Ok(f(&mut w, DEFAULT_TYPE))
        }
        a => caller_words(mem, a, |w, t| f(w, u32::try_from(t).unwrap_or(0))).ok_or(Errno::EFAULT),
    }
}

/// `random()`; 0 if the current state is an unreachable caller buffer.
pub fn random(mut mem: impl CMem) -> i64 {
    let _g = LOCK.lock();
    on_current(&mut mem, |w, _| AdditiveFeedback::next(w)).map_or(0, i64::from)
}

/// `rand()`: the same generator as `random` (OD-13).
pub fn rand(mem: impl CMem) -> i32 {
    i32::try_from(random(mem)).unwrap_or(0)
}

/// `rand_r`, as a C `int`: the caller's variable is the whole state, so this is the core
/// function.
pub fn rand_r(seed: &mut u32) -> i32 {
    i32::try_from(liella::random::rand_r(seed)).unwrap_or(0)
}

/// `srandom(seed)` / `srand(seed)`: reseed the current state.
pub fn srandom(seed: u32, mut mem: impl CMem) {
    // Seed 0 is seed 1 (glibc black-box, FTOK-HIGHBIT-2); the same holds for every seeding entry point here and in prng_r.
    let _g = LOCK.lock();
    // The default state is (re)seeded either way; an unreachable caller buffer stays as is.
    let _ = on_current(&mut mem, |w, t| AdditiveFeedback::seed(w, t, seed.max(1)));
}

/// `initstate(seed, buf, n)`: seed `buf` (type by size) and make it current; returns the
/// previous state's address, or `EINVAL` if `n < 8`.
pub fn initstate(seed: u32, buf: usize, n: usize, mut mem: impl CMem) -> Result<usize> {
    let t = usize::try_from(AdditiveFeedback::type_for(n).ok_or(Errno::EINVAL)?).map_err(|_| Errno::EINVAL)?;
    let _g = LOCK.lock();
    let window = mem.bytes(buf, *SIZES.get(t).ok_or(Errno::EINVAL)?).ok_or(Errno::EFAULT)?;
    AdditiveFeedback::seed(&mut ByteWords(window), u32::try_from(t).unwrap_or(0), seed.max(1));
    let prev = current_addr();
    CURRENT.store(buf, Relaxed);
    Ok(prev)
}

/// `setstate(buf)`: make an initialized buffer current; returns the previous state's address.
pub fn setstate(buf: usize) -> usize {
    let _g = LOCK.lock();
    let prev = current_addr();
    CURRENT.store(if buf == default_state_addr() { 0 } else { buf }, Relaxed);
    prev
}

// ---- the 48-bit family ----

static X48: AtomicU64 = AtomicU64::new(0x1234_abcd_330e);
static A48: AtomicU64 = AtomicU64::new(RNG48_A);
static C48: AtomicU16 = AtomicU16::new(RNG48_C);
/// The static copy of the previous state that `seed48` returns.
pub static SEED48_OLD: [AtomicU16; 3] = [const { AtomicU16::new(0) }; 3];

/// The global generator (call with the lock held).
fn load() -> Rng48 {
    let [b0, b1, b2, b3, b4, b5, ..] = X48.load(Relaxed).to_le_bytes();
    Rng48 { x: [u16::from_le_bytes([b0, b1]), u16::from_le_bytes([b2, b3]), u16::from_le_bytes([b4, b5])], a: A48.load(Relaxed), c: C48.load(Relaxed) }
}

fn store(r: Rng48) {
    X48.store(u64::from(r.x[0]) | u64::from(r.x[1]) << 16 | u64::from(r.x[2]) << 32, Relaxed);
    A48.store(r.a, Relaxed);
    C48.store(r.c, Relaxed);
}

/// Advance the global generator and return its new 48-bit value.
pub fn next48() -> u64 {
    let _g = LOCK.lock();
    let mut r = load();
    let v = r.next();
    store(r);
    v
}

/// Advance a caller state with the global parameters (`erand48`, `nrand48`, `jrand48`).
pub fn step48(xsubi: &mut [u16; 3]) -> u64 {
    let _g = LOCK.lock();
    load().step(xsubi)
}

/// `srand48`.
pub fn srand48(seed: i64) {
    let _g = LOCK.lock();
    let mut r = load();
    r.srand48(seed);
    store(r);
}

/// Install `s` with the default parameters and keep the previous state in [`SEED48_OLD`].
fn seed48_locked(s: [u16; 3]) -> [u16; 3] {
    let mut r = load();
    let old = r.seed48(s);
    store(r);
    for (slot, w) in SEED48_OLD.iter().zip(old) {
        slot.store(w, Relaxed);
    }
    old
}

/// `seed48` as C sees it: seed from `s` and return the address of the static copy of the
/// previous state.
pub fn seed48_addr(s: &[u16; 3]) -> usize {
    let _g = LOCK.lock();
    seed48_locked(*s);
    SEED48_OLD.as_ptr().addr()
}

/// `drand48`.
pub fn drand48() -> f64 {
    Rng48::to_double(next48())
}

/// `erand48`.
pub fn erand48(x: &mut [u16; 3]) -> f64 {
    Rng48::to_double(step48(x))
}

/// `lrand48`.
pub fn lrand48() -> i64 {
    Rng48::to_nonneg(next48())
}

/// `nrand48`.
pub fn nrand48(x: &mut [u16; 3]) -> i64 {
    Rng48::to_nonneg(step48(x))
}

/// `mrand48`.
pub fn mrand48() -> i64 {
    Rng48::to_signed(next48())
}

/// `jrand48`.
pub fn jrand48(x: &mut [u16; 3]) -> i64 {
    Rng48::to_signed(step48(x))
}

/// `lcong48` from the caller's seven words.
pub fn lcong48_from(p: &[u16; 7]) {
    lcong48(*p);
}

/// `seed48`: returns the previous state, also kept in [`SEED48_OLD`].
pub fn seed48(s: [u16; 3]) -> [u16; 3] {
    let _g = LOCK.lock();
    seed48_locked(s)
}

/// `lcong48`.
pub fn lcong48(p: [u16; 7]) {
    let _g = LOCK.lock();
    let mut r = load();
    r.lcong48(p);
    store(r);
}
