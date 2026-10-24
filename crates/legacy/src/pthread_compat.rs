//! The C shapes of the thread interfaces (spec §22.1–§22.3, §22.6, §22.8): the attribute object,
//! C11 result codes, and the argument checks the contracts specify. The thread runtime itself is
//! `liella_rt::thread` and `liella_boundary::thread`.
//!
//! **`pthread_attr_t`** (56 bytes, ⚑ ARB: its contents are not ABI, only its size; programs
//! initialise it with `pthread_attr_init`) is viewed as seven words:
//! 0 flags and policy, 1 priority, 2 stack size (0: the default), 3 guard size, 4 the stack's
//! top address (`setstackaddr`'s address is the high end, as glibc reads it), 5 the signal
//! mask, 6 the CPU affinity copy (a heap block: its length, then the mask bytes; 0 for none).

use core::sync::atomic::{AtomicUsize, Ordering::Relaxed};

use liella::error::{Errno, Result};
use liella_kernel::uapi::{SCHED_FIFO, SCHED_OTHER, SCHED_RR};
use liella_rt::thread::{Attr, STACK_MIN};

/// Words of a `pthread_attr_t`.
pub const ATTR_WORDS: usize = 7;

const DETACHED: usize = 1;
const EXPLICIT: usize = 2;
const HAS_STACK: usize = 4;
const HAS_SIGMASK: usize = 8;
const HAS_GUARD: usize = 16;
const POLICY_SHIFT: u32 = 8;

/// `PTHREAD_CREATE_DETACHED`.
pub const CREATE_DETACHED: i32 = 1;
/// `PTHREAD_CREATE_JOINABLE`.
pub const CREATE_JOINABLE: i32 = 0;
/// `PTHREAD_EXPLICIT_SCHED`.
pub const EXPLICIT_SCHED: i32 = 1;
/// `PTHREAD_INHERIT_SCHED`.
pub const INHERIT_SCHED: i32 = 0;
/// `PTHREAD_SCOPE_SYSTEM`.
pub const SCOPE_SYSTEM: i32 = 0;
/// `PTHREAD_SCOPE_PROCESS`.
pub const SCOPE_PROCESS: i32 = 1;
/// `PTHREAD_ATTR_NO_SIGMASK_NP`.
pub const NO_SIGMASK_NP: i32 = -1;

/// The attribute words.
pub struct AttrWords<'a> {
    w: &'a [AtomicUsize],
}

fn word(w: &[AtomicUsize], i: usize) -> usize {
    w.get(i).map_or(0, |x| x.load(Relaxed))
}

fn set_word(w: &[AtomicUsize], i: usize, v: usize) {
    if let Some(x) = w.get(i) {
        x.store(v, Relaxed);
    }
}

impl<'a> AttrWords<'a> {
    /// The object's words (at least [`ATTR_WORDS`]).
    pub fn new(w: &'a [AtomicUsize]) -> AttrWords<'a> {
        AttrWords { w }
    }

    fn flags(&self) -> usize {
        word(self.w, 0)
    }

    fn set_flag(&self, f: usize, on: bool) {
        let v = self.flags();
        set_word(self.w, 0, if on { v | f } else { v & !f });
    }

    /// `pthread_attr_init`: the defaults (joinable, default stack and guard, inherited
    /// scheduling, `SCHED_OTHER` priority 0, no mask, no affinity).
    pub fn init(&self) {
        for i in 0..ATTR_WORDS {
            set_word(self.w, i, 0);
        }
    }

    /// The affinity copy's heap block (0 for none): `pthread_attr_destroy` frees it.
    pub fn affinity_block(&self) -> usize {
        word(self.w, 6)
    }

    /// Replace the affinity copy with `block` (0: none); returns the old block to free.
    pub fn swap_affinity(&self, block: usize) -> usize {
        let old = word(self.w, 6);
        set_word(self.w, 6, block);
        old
    }

    /// `pthread_attr_setdetachstate`.
    pub fn set_detachstate(&self, v: i32) -> Result<()> {
        match v {
            CREATE_DETACHED | CREATE_JOINABLE => {
                self.set_flag(DETACHED, v == CREATE_DETACHED);
                Ok(())
            }
            _ => Err(Errno::EINVAL),
        }
    }

    /// `pthread_attr_getdetachstate`.
    pub fn detachstate(&self) -> i32 {
        if self.flags() & DETACHED != 0 { CREATE_DETACHED } else { CREATE_JOINABLE }
    }

    /// `pthread_attr_setstacksize`: at least `PTHREAD_STACK_MIN`.
    pub fn set_stacksize(&self, n: usize) -> Result<()> {
        if n < STACK_MIN {
            return Err(Errno::EINVAL);
        }
        set_word(self.w, 2, n);
        Ok(())
    }

    /// `pthread_attr_getstacksize` (`default` when never set).
    pub fn stacksize(&self, default: usize) -> usize {
        match word(self.w, 2) {
            0 => default,
            n => n,
        }
    }

    /// `pthread_attr_setstack(addr, size)`: the stack is `[addr, addr + size)`.
    pub fn set_stack(&self, addr: usize, size: usize) -> Result<()> {
        if size < STACK_MIN {
            return Err(Errno::EINVAL);
        }
        let top = addr.checked_add(size).ok_or(Errno::EINVAL)?;
        set_word(self.w, 2, size);
        set_word(self.w, 4, top);
        self.set_flag(HAS_STACK, true);
        Ok(())
    }

    /// `pthread_attr_getstack`: (lowest address, size); (0, size) when none was set.
    pub fn stack(&self, default: usize) -> (usize, usize) {
        let size = self.stacksize(default);
        let top = word(self.w, 4);
        (top.saturating_sub(size), size)
    }

    /// `pthread_attr_setstackaddr(addr)`: `addr` is the stack's high end (⚑ PATH: glibc's
    /// reading of the deprecated call).
    pub fn set_stackaddr(&self, top: usize) {
        set_word(self.w, 4, top);
        self.set_flag(HAS_STACK, true);
    }

    /// `pthread_attr_getstackaddr`.
    pub fn stackaddr(&self) -> usize {
        word(self.w, 4)
    }

    /// `pthread_attr_setguardsize` (rounded to pages at creation; 0 disables the guard).
    pub fn set_guardsize(&self, n: usize) {
        set_word(self.w, 3, n);
        self.set_flag(HAS_GUARD, true);
    }

    /// `pthread_attr_getguardsize` (`default` when never set).
    pub fn guardsize(&self, default: usize) -> usize {
        if self.flags() & HAS_GUARD != 0 { word(self.w, 3) } else { default }
    }

    /// `pthread_attr_setschedpolicy`: `SCHED_OTHER`, `SCHED_FIFO` or `SCHED_RR`.
    pub fn set_policy(&self, p: i32) -> Result<()> {
        let ok = u32::try_from(p).is_ok_and(|p| p == SCHED_OTHER || p == SCHED_FIFO || p == SCHED_RR);
        if !ok {
            return Err(Errno::EINVAL);
        }
        let v = self.flags() & !(0xff << POLICY_SHIFT);
        set_word(self.w, 0, v | (usize::try_from(p).unwrap_or(0) << POLICY_SHIFT));
        Ok(())
    }

    /// `pthread_attr_getschedpolicy`.
    pub fn policy(&self) -> i32 {
        i32::try_from((self.flags() >> POLICY_SHIFT) & 0xff).unwrap_or(0)
    }

    /// `pthread_attr_setschedparam`: the priority must be within `[min, max]` for the
    /// attribute's policy (the caller asks the kernel for the range).
    pub fn set_priority(&self, prio: i32, range: (i32, i32)) -> Result<()> {
        if prio < range.0 || prio > range.1 {
            return Err(Errno::EINVAL);
        }
        set_word(self.w, 1, usize::from_ne_bytes(i64::from(prio).to_ne_bytes()));
        Ok(())
    }

    /// `pthread_attr_getschedparam`.
    pub fn priority(&self) -> i32 {
        i32::try_from(i64::from_ne_bytes(word(self.w, 1).to_ne_bytes())).unwrap_or(0)
    }

    /// `pthread_attr_setinheritsched`.
    pub fn set_inherit(&self, v: i32) -> Result<()> {
        match v {
            INHERIT_SCHED | EXPLICIT_SCHED => {
                self.set_flag(EXPLICIT, v == EXPLICIT_SCHED);
                Ok(())
            }
            _ => Err(Errno::EINVAL),
        }
    }

    /// `pthread_attr_getinheritsched`.
    pub fn inherit(&self) -> i32 {
        if self.flags() & EXPLICIT != 0 { EXPLICIT_SCHED } else { INHERIT_SCHED }
    }

    /// `pthread_attr_setsigmask_np`: `None` removes the mask.
    pub fn set_sigmask(&self, mask: Option<u64>) {
        set_word(self.w, 5, usize::try_from(mask.unwrap_or(0)).unwrap_or(0));
        self.set_flag(HAS_SIGMASK, mask.is_some());
    }

    /// `pthread_attr_getsigmask_np`: the mask, or `None` (`PTHREAD_ATTR_NO_SIGMASK_NP`).
    pub fn sigmask(&self) -> Option<u64> {
        (self.flags() & HAS_SIGMASK != 0).then(|| u64::try_from(word(self.w, 5)).unwrap_or(0))
    }

    /// The attributes for thread creation. `affinity` is the copy's mask (address and length),
    /// read by the caller from the heap block.
    pub fn decode(&self, affinity: Option<(usize, usize)>) -> Attr {
        let f = self.flags();
        Attr {
            detached: f & DETACHED != 0,
            stack_size: word(self.w, 2),
            guard: (f & HAS_GUARD != 0).then(|| word(self.w, 3)),
            stack: (f & HAS_STACK != 0).then(|| {
                let size = word(self.w, 2).max(STACK_MIN);
                let top = word(self.w, 4);
                (top.saturating_sub(size), size)
            }),
            sched: (f & EXPLICIT != 0).then(|| (self.policy(), self.priority())),
            sigmask: self.sigmask(),
            affinity,
        }
    }
}

/// `pthread_attr_setscope`: only `PTHREAD_SCOPE_SYSTEM` (§22.2).
pub fn check_scope(v: i32) -> Result<()> {
    match v {
        SCOPE_SYSTEM => Ok(()),
        SCOPE_PROCESS => Err(Errno::EOPNOTSUPP), // ENOTSUP, the same number on Linux
        _ => Err(Errno::EINVAL),
    }
}

// ------------------------------------------------------------------------------ C11

/// `thrd_success`.
pub const THRD_SUCCESS: i32 = 0;
/// `thrd_busy`.
pub const THRD_BUSY: i32 = 1;
/// `thrd_error`.
pub const THRD_ERROR: i32 = 2;
/// `thrd_nomem`.
pub const THRD_NOMEM: i32 = 3;
/// `thrd_timedout`.
pub const THRD_TIMEDOUT: i32 = 4;

/// A pthread error number as a C11 result code (§22.1, D-22.3).
pub fn c11_code(r: Result<()>) -> i32 {
    match r {
        Ok(()) => THRD_SUCCESS,
        Err(Errno::EBUSY) => THRD_BUSY,
        Err(Errno::ETIMEDOUT) => THRD_TIMEDOUT,
        Err(Errno::ENOMEM | Errno::EAGAIN) => THRD_NOMEM,
        Err(_) => THRD_ERROR,
    }
}

/// `thrd_create`'s code: `EAGAIN` from the kernel is `thrd_nomem`, as in glibc.
pub fn c11_create_code(r: Result<()>) -> i32 {
    c11_code(r)
}

/// `pthread_setname_np`/`getname_np`: names hold at most 15 bytes plus the terminator
/// (`TASK_COMM_LEN`, ⚑ KERN).
pub const NAME_MAX: usize = 15;

/// A thread name for the kernel: `ERANGE` if longer than 15 bytes.
pub fn comm(name: &[u8]) -> Result<[u8; 16]> {
    if name.len() > NAME_MAX {
        return Err(Errno::ERANGE);
    }
    let mut out = [0u8; 16];
    for (o, &b) in out.iter_mut().zip(name) {
        *o = b;
    }
    Ok(out)
}

// ------------------------------------------------------------------------------ main thread

/// The initial thread's stack for `pthread_getattr_np` (§22.2: from `/proc/self/maps` and
/// `RLIMIT_STACK`): the top is the end of the mapping holding `inside` (an address on the
/// stack); the size is the soft limit, but no more than the gap down to the mapping below,
/// in whole pages. `None` if no mapping holds `inside`.
pub fn main_stack(maps: &[u8], inside: usize, rlimit_cur: u64) -> Option<(usize, usize)> {
    let mut below_end = 0usize;
    for line in maps.split(|&b| b == b'\n') {
        let range = line.split(|&b| b == b' ').next()?;
        let mut parts = range.splitn(2, |&b| b == b'-');
        let (Some(lo), Some(hi)) = (parts.next().and_then(hex), parts.next().and_then(hex)) else { continue };
        if lo <= inside && inside < hi {
            let limit = usize::try_from(rlimit_cur).unwrap_or(usize::MAX);
            let size = limit.min(hi.saturating_sub(below_end)) & !(4096 - 1);
            return Some((hi, size));
        }
        if hi <= inside {
            below_end = hi;
        }
    }
    None
}

fn hex(t: &[u8]) -> Option<usize> {
    let mut v = 0usize;
    if t.is_empty() {
        return None;
    }
    for &c in t {
        let d = char::from(c).to_digit(16)?;
        v = v.checked_mul(16)?.checked_add(usize::try_from(d).ok()?)?;
    }
    Some(v)
}

/// `pthread_getcpuclockid`: the thread CPU-time clock of `tid` (⚑ KERN: `MAKE_THREAD_CPUCLOCK
/// (tid, CPUCLOCK_SCHED)`, i.e. `(~tid << 3) | 6`).
pub fn thread_cpuclock(tid: i32) -> i32 {
    (!tid).wrapping_shl(3) | 6
}

/// `/proc/self/task/<tid>/comm` as a NUL-terminated path.
#[allow(clippy::arithmetic_side_effects)] // counters below the 40-byte buffer: a TID has at most 10 digits
pub fn comm_path(tid: i32) -> [u8; 40] {
    let mut out = [0u8; 40];
    let head = b"/proc/self/task/";
    let mut n = 0;
    for &b in head {
        if let Some(o) = out.get_mut(n) {
            *o = b;
        }
        n += 1;
    }
    let mut digits = [0u8; 12];
    let mut d = 0;
    let mut v = tid.unsigned_abs();
    loop {
        if let Some(x) = digits.get_mut(d) {
            *x = b'0' + u8::try_from(v % 10).unwrap_or(0);
        }
        d += 1;
        v /= 10;
        if v == 0 {
            break;
        }
    }
    for i in (0..d).rev() {
        if let (Some(o), Some(&x)) = (out.get_mut(n), digits.get(i)) {
            *o = x;
        }
        n += 1;
    }
    for &b in b"/comm" {
        if let Some(o) = out.get_mut(n) {
            *o = b;
        }
        n += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;

    fn words() -> Vec<AtomicUsize> {
        (0..ATTR_WORDS).map(|_| AtomicUsize::new(0xdead)).collect()
    }

    #[test]
    fn attributes_round_trip_and_validate() {
        let w = words();
        let a = AttrWords::new(&w);
        a.init();
        assert_eq!(a.detachstate(), CREATE_JOINABLE);
        assert_eq!(a.stacksize(8 << 20), 8 << 20, "the default until set");
        assert_eq!(a.guardsize(4096), 4096);
        assert_eq!((a.policy(), a.priority(), a.inherit()), (0, 0, INHERIT_SCHED));
        assert_eq!(a.sigmask(), None);
        assert_eq!(a.set_detachstate(7), Err(Errno::EINVAL));
        a.set_detachstate(CREATE_DETACHED).unwrap();
        assert_eq!(a.detachstate(), CREATE_DETACHED);
        assert_eq!(a.set_stacksize(STACK_MIN - 1), Err(Errno::EINVAL));
        a.set_stacksize(1 << 20).unwrap();
        assert_eq!(a.stacksize(0), 1 << 20);
        a.set_guardsize(0);
        assert_eq!(a.guardsize(4096), 0, "0 disables the guard");
        assert_eq!(a.set_policy(3), Err(Errno::EINVAL), "SCHED_BATCH is not an attribute policy");
        a.set_policy(1).unwrap();
        assert_eq!(a.set_priority(100, (1, 99)), Err(Errno::EINVAL));
        a.set_priority(10, (1, 99)).unwrap();
        a.set_inherit(EXPLICIT_SCHED).unwrap();
        assert_eq!(a.set_inherit(5), Err(Errno::EINVAL));
        a.set_sigmask(Some(0b110));
        assert_eq!(a.sigmask(), Some(0b110));
        a.set_stack(0x10000, 0x20000).unwrap();
        assert_eq!(a.stack(0), (0x10000, 0x20000));
        assert_eq!(a.stackaddr(), 0x30000, "the high end");
        let d = a.decode(None);
        assert!(d.detached);
        assert_eq!(d.stack, Some((0x10000, 0x20000)));
        assert_eq!(d.sched, Some((1, 10)));
        assert_eq!(d.guard, Some(0));
        assert_eq!(d.sigmask, Some(0b110));
        assert_eq!(check_scope(SCOPE_PROCESS), Err(Errno::EOPNOTSUPP));
        assert_eq!(check_scope(9), Err(Errno::EINVAL));
    }

    #[test]
    fn the_main_stack_comes_from_maps_and_the_limit() {
        let maps = b"00400000-00452000 r-xp 00000000 08:02 173521 /usr/bin/x\n7ffd0000-7ffd2000 rw-p 00000000 00:00 0\n7ffde000-7ffff000 rw-p 00000000 00:00 0 [stack]\n";
        assert_eq!(main_stack(maps, 0x7fff_e000, 8 << 20), Some((0x7fff_f000, 0x7fff_f000 - 0x7ffd_2000)), "capped by the mapping below");
        assert_eq!(main_stack(maps, 0x7fff_e000, 64 << 10), Some((0x7fff_f000, 64 << 10)), "the soft limit");
        assert_eq!(main_stack(maps, 0x1000, 8 << 20), None);
        assert_eq!(thread_cpuclock(1234), (!1234 << 3) | 6);
        assert_eq!(&comm_path(4321)[..26], b"/proc/self/task/4321/comm\0");
    }

    #[test]
    fn c11_codes_and_names() {
        assert_eq!(c11_code(Ok(())), THRD_SUCCESS);
        assert_eq!(c11_code(Err(Errno::ETIMEDOUT)), THRD_TIMEDOUT);
        assert_eq!(c11_code(Err(Errno::EBUSY)), THRD_BUSY);
        assert_eq!(c11_code(Err(Errno::EAGAIN)), THRD_NOMEM);
        assert_eq!(c11_code(Err(Errno::EINVAL)), THRD_ERROR);
        assert_eq!(comm(b"0123456789abcdef"), Err(Errno::ERANGE));
        assert_eq!(&comm(b"worker").unwrap()[..7], b"worker\0");
    }
}
