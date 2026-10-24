//! Thread bookkeeping (spec §22.1–§22.3, §22.6; D-22.1–D-22.11): the per-thread *descriptor*,
//! the layout of a thread's mapping, the join and cancellation state machines, and the
//! process-wide key table. The boundary creates threads (`liella_boundary::thread`); every
//! decision is here.
//!
//! **A thread's mapping** (D-22.9), from low to high addresses: the guard (no access), the
//! stack, the static TLS block, the thread block (the thread pointer), the descriptor, and the
//! key area. A caller-supplied stack (`pthread_attr_setstack`) gets only the part from the TLS
//! block up, in a mapping of its own.
//!
//! **The descriptor** is all atomics, so the creating thread, the thread itself, a joiner and
//! the internal signal's handler can share it. The thread block names it (word 0xa0); a thread
//! the host C library created (D-22.10) gets an *adopted* descriptor on first need, without
//! the join and stack parts.

#![allow(clippy::arithmetic_side_effects)] // offsets inside one mapping, key indexes below KEYS

use core::sync::atomic::{AtomicI32, AtomicU32, AtomicU64, AtomicUsize, Ordering::{AcqRel, Acquire, Relaxed, Release}};

use liella_kernel::uapi::Rseq;

use crate::tls::{TlsImage, BLOCK_SIZE};

/// The page size.
pub const PAGE: usize = 4096;
/// Keys a thread can hold values for (`PTHREAD_KEYS_MAX` stays 1024 in the ABI; more are
/// allowed, D-22.5).
pub const KEYS: usize = 4096;
/// Destructor rounds at thread exit (`PTHREAD_DESTRUCTOR_ITERATIONS`).
pub const DESTRUCTOR_ROUNDS: usize = 4;
/// Bytes of a thread's key area: a value and a sequence number per key.
pub const KEY_AREA: usize = KEYS * 16;
/// Bytes of a thread's result slots (D-H3), right after its key area: one per entry point that
/// returns a library-owned text per thread.
pub const SLOT_AREA: usize = SLOTS * SLOT_SIZE;
/// Result slots per thread.
pub const SLOTS: usize = 17;
/// Bytes of one result slot.
pub const SLOT_SIZE: usize = 32;
/// The result slot of `strerror` (§2.2).
pub const SLOT_STRERROR: usize = 0;
/// The result slot of `strsignal` (§20.8).
pub const SLOT_STRSIGNAL: usize = 1;
/// The `struct tm` that `gmtime` and `localtime` share (§23.5): two slots (56 bytes).
pub const SLOT_TM: usize = 2;
/// The text that `asctime` and `ctime` share (§23.5): 26 bytes.
pub const SLOT_ASCTIME: usize = 4;
/// The `struct tm` `getdate` returns (§23.7): two slots (56 bytes).
pub const SLOT_GETDATE: usize = 5;
/// The text `inet_ntoa` returns (§15.3): 16 bytes.
pub const SLOT_INET_NTOA: usize = 7;
/// The text `ether_ntoa` returns (§15.7): 18 bytes.
pub const SLOT_ETHER_NTOA: usize = 8;
/// The `struct ether_addr` `ether_aton` returns (§15.7): 6 bytes.
pub const SLOT_ETHER_ATON: usize = 9;
/// The thread's result area (D-C0.2; milestone 6, 2026-10-04): one word, the address of a
/// table the library allocates on first use (the address and size of each kind's block, each
/// block also allocated on first use) and frees, with the blocks, when the thread exits; so the
/// kinds are not limited by the slots. The records the non-`_r` database functions return (§16.3,
/// §16.5, §16.6, §17.3–17.6; D-17.3, D-16.2) and the mount-table functions (§13.13; D-13.15)
/// (`struct passwd`, `struct group`, `struct spwd`, `struct sgrp`, `struct utmp`, the login
/// name, `struct hostent`, `struct netent`, `struct servent`, `struct protoent`, `struct
/// rpcent`, `struct aliasent`, `struct mntent`, `struct fstab`), and the texts of `ecvt`,
/// `fcvt`, `qecvt`, `qfcvt`, `l64a`, `tmpnam(NULL)`, `inet_nsap_ntoa`, `rcmd`'s host name,
/// `ttyname`, `ptsname`, `getttyent`, `getusershell` and the resolver's `p_*`/`sym_*`/
/// `hostalias`/`loc_ntoa` (milestone 6, 2026-10-04).
/// Byte 8 of this slot is the thread's regex scratch busy flag (D-14.12; `users::scratch_claim`
/// in the boundary crate): a `regexec` claims it for its call, so a re-entrant one (a signal
/// handler) never shares the thread's regex blocks.
pub const SLOT_RECORDS: usize = 10;
/// The bytes of [`SLOT_RECORDS`] the result table uses: the table's address (word 0).
pub const RECORDS_AREA: usize = 8;

// ------------------------------------------------------------------------------ cancellation

/// Cancellation word: cancellation disabled (`PTHREAD_CANCEL_DISABLE`).
pub const CANCEL_DISABLED: u32 = 1;
/// Cancellation word: asynchronous type (`PTHREAD_CANCEL_ASYNCHRONOUS`).
pub const CANCEL_ASYNC: u32 = 2;
/// Cancellation word: a request arrived (`pthread_cancel`).
pub const CANCEL_REQUESTED: u32 = 4;
/// Cancellation word: the thread is acting on a request or exiting; no further action.
pub const CANCEL_ACTING: u32 = 8;
/// Cancellation word: a cancellation point's system call found the request due (the window
/// returned `-ECANCELED`, or the call was interrupted while it was due); the export acts on it
/// once its C result is formed (D-22.2). Not a bit the window tests.
pub const CANCEL_HIT: u32 = 16;
/// The bits the C layer's cancellation window tests: `(word & MASK) == REQUESTED` means
/// "act now" (machine/thread_start.S).
pub const CANCEL_WINDOW_MASK: u32 = CANCEL_DISABLED | CANCEL_REQUESTED | CANCEL_ACTING;

/// True if a thread with cancellation word `w` must act on a request at a cancellation point.
pub fn cancel_due(w: u32) -> bool {
    w & CANCEL_WINDOW_MASK == CANCEL_REQUESTED
}

// ------------------------------------------------------------------------------ joining

/// Join state: joinable, running or exited, not yet joined.
pub const JOINABLE: u32 = 0;
/// Join state: detached; its exit releases it.
pub const DETACHED: u32 = 1;
/// Join state: a joiner has claimed it (a second join or a detach is refused).
pub const JOINING: u32 = 2;

/// `pthread_detach`: `Some(true)` if the thread had exited already (the caller releases it
/// now), `Some(false)` if its exit will; `None` if it is not joinable.
pub fn detach(state: &AtomicU32, exited: bool) -> Option<bool> {
    state.compare_exchange(JOINABLE, DETACHED, AcqRel, Acquire).ok().map(|_| exited)
}

/// Claim a thread for joining: `false` if it is detached or already being joined.
pub fn claim_join(state: &AtomicU32) -> bool {
    state.compare_exchange(JOINABLE, JOINING, AcqRel, Acquire).is_ok()
}

// ------------------------------------------------------------------------------ the descriptor

/// The descriptor. Everything the thread runtime keeps per thread, beyond the thread block.
#[repr(C, align(64))]
pub struct Thread {
    /// The kernel's robust-list head (`set_robust_list`): the list (a self-pointing link when
    /// empty), the futex word's offset from a list entry, the entry being changed.
    pub robust: [AtomicUsize; 3],
    /// `DESC_*` flags.
    pub flags: AtomicU32,
    /// The join state (`JOINABLE`, `DETACHED`, `JOINING`).
    pub join: AtomicU32,
    /// The cancellation word (`CANCEL_*`).
    pub cancel: AtomicU32,
    /// The setxid request word (D-17.7): the request's generation this thread has served.
    pub setxid: AtomicU32,
    /// The start routine (a C function pointer).
    pub start: AtomicUsize,
    /// Its argument.
    pub arg: AtomicUsize,
    /// The thread's result (`pthread_exit`'s value, or the start routine's).
    pub result: AtomicUsize,
    /// The newest registered cleanup buffer (`__pthread_register_cancel`), 0 for none.
    pub cleanup_jmp: AtomicUsize,
    /// The newest old-style cleanup record (`_pthread_cleanup_push`), 0 for none.
    pub cleanup_old: AtomicUsize,
    /// The thread this one is joining (deadlock detection), 0 for none.
    pub joining: AtomicUsize,
    /// The whole mapping: base (0 for the initial and adopted threads).
    pub map_base: AtomicUsize,
    /// Length of the mapping.
    pub map_len: AtomicUsize,
    /// The stack: lowest usable address.
    pub stack_lo: AtomicUsize,
    /// Size of the stack.
    pub stack_size: AtomicUsize,
    /// Guard bytes below the stack.
    pub guard: AtomicUsize,
    /// The signal mask the thread starts with.
    pub sigmask: AtomicU64,
    /// The thread list's links (descriptor addresses).
    pub next: AtomicUsize,
    /// Previous in the thread list.
    pub prev: AtomicUsize,
    /// Scheduling the creator asked for: policy + 1 (0: inherit).
    pub sched_policy: AtomicI32,
    /// Priority.
    pub sched_prio: AtomicI32,
    /// The key area's address.
    pub keys: AtomicUsize,
    /// A start handshake word (the creator waits for the new thread's verdict when attributes
    /// ask for scheduling or affinity it must apply itself).
    pub started: AtomicU32,
    /// The verdict: 0 or an error number.
    pub start_error: AtomicU32,
    /// The per-thread result slots of D-H3 (`strsignal` and friends): an address, 0 until used.
    pub slots: AtomicUsize,
    /// The CPU affinity the creator asked for: address of a mask copied into the mapping (0:
    /// inherit).
    pub affinity: AtomicUsize,
    /// Size of the affinity mask.
    pub affinity_len: AtomicUsize,
    /// The thread pointer (`pthread_t`).
    pub tp: AtomicUsize,
    /// The reclaim token: whoever moves it from 0 to 1 recycles the mapping (the exiting
    /// thread of a detached thread, a detacher of an exited one, or the joiner).
    pub reclaim: AtomicU32,
    /// The restartable-sequences area (`struct rseq`, 32 bytes, 32-aligned; D-22.7).
    pub rseq: Rseq,
}

/// Descriptor flag: the runtime created this thread (it can be joined and its stack freed).
pub const DESC_CREATED: u32 = 1;
/// Descriptor flag: adopted for a thread the host C library created (D-22.10).
pub const DESC_ADOPTED: u32 = 2;
/// Descriptor flag: the stack was the caller's (`pthread_attr_setstack`).
pub const DESC_USER_STACK: u32 = 4;
/// Descriptor flag: the thread is a C11 thread (`thrd_create`: an `int` result).
pub const DESC_C11: u32 = 8;
/// Descriptor flag: the rseq area is registered with the kernel.
pub const DESC_RSEQ: u32 = 16;
/// Descriptor flag: the thread has exited (its exit sequence ran).
pub const DESC_EXITED: u32 = 32;

/// The descriptor's size.
pub const DESC_SIZE: usize = size_of::<Thread>();
/// `__rseq_size`: the registered size (the original 32-byte area).
pub const RSEQ_SIZE: u32 = 32;
/// The rseq area's offset in the descriptor.
pub const RSEQ_IN_DESC: usize = core::mem::offset_of!(Thread, rseq);

// The direct cancellation points (csys/src/machine/points.S, CANCEL-FAST-2) read the word at this
// offset from the descriptor.
const _: () = assert!(core::mem::offset_of!(Thread, cancel) == 32);
/// The robust head's offset of the futex word from a list entry: `__lock` (0) minus
/// `__list.__next` (32) in `pthread_mutex_t`.
pub const ROBUST_FUTEX_OFFSET: isize = -32;

const _: () = assert!(KEYS > 1024, "keys beyond PTHREAD_KEYS_MAX are allowed (D-22.5)");
const _: () = assert!(DESC_SIZE <= 512 && DESC_SIZE % 64 == 0 && RSEQ_IN_DESC % 32 == 0);

impl Thread {
    /// Prepare a fresh (zeroed) descriptor at `addr`: the robust list empty (its head points to
    /// itself) and the futex offset set.
    pub fn init_robust(&self, addr: usize) {
        self.robust[0].store(addr, Relaxed);
        self.robust[1].store(usize::from_ne_bytes(ROBUST_FUTEX_OFFSET.to_ne_bytes()), Relaxed);
        self.robust[2].store(0, Relaxed);
    }

    /// True if flag `f` is set.
    pub fn has(&self, f: u32) -> bool {
        self.flags.load(Acquire) & f != 0
    }

    /// Take the reclaim token: true for exactly one caller.
    pub fn take_reclaim(&self) -> bool {
        self.reclaim.compare_exchange(0, 1, AcqRel, Acquire).is_ok()
    }
}

// ------------------------------------------------------------------------------ layout

/// Where a new thread's parts go inside its mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Layout {
    /// Bytes to map (a multiple of the page size).
    pub len: usize,
    /// The guard: bytes from the mapping's base (made inaccessible).
    pub guard: usize,
    /// The thread pointer, as an offset from the base.
    pub tp: usize,
    /// The TLS block's start, as an offset (it ends at `tp`).
    pub tls: usize,
    /// The descriptor's offset.
    pub desc: usize,
    /// The key area's offset.
    pub keys: usize,
    /// The initial stack pointer's offset: 16-aligned, below the TLS block.
    pub sp: usize,
}

impl Layout {
    /// A mapping with `stack` usable stack bytes above a `guard`-byte guard (both rounded up
    /// to pages), then TLS for `image`, the thread block, the descriptor and the key area.
    /// `None` on overflow or an invalid TLS segment.
    pub fn new(stack: usize, guard: usize, image: Option<&TlsImage>) -> Option<Layout> {
        let guard = guard.checked_next_multiple_of(PAGE)?;
        let stack = stack.max(PAGE).checked_next_multiple_of(PAGE)?;
        let (tls_size, align) = match image {
            Some(t) if t.align.max(1).is_power_of_two() && t.filesz <= t.memsz => (t.block_size()?, t.align.max(64)),
            Some(_) => return None,
            None => (0, 64),
        };
        // The thread pointer: above the stack and the TLS block, aligned for both.
        let tp = guard.checked_add(stack)?.checked_add(tls_size)?.checked_next_multiple_of(align)?;
        let desc = tp.checked_add(BLOCK_SIZE)?.checked_next_multiple_of(64)?;
        let keys = desc.checked_add(DESC_SIZE)?.checked_next_multiple_of(64)?;
        let len = keys.checked_add(KEY_AREA)?.checked_add(SLOT_AREA)?.checked_next_multiple_of(PAGE)?;
        let tls = tp.checked_sub(tls_size)?;
        // The stack ends below the TLS block, 16-aligned, with room for the two start words.
        let sp = (tls & !15).checked_sub(16)?;
        Some(Layout { len, guard, tp, tls, desc, keys, sp })
    }

    /// The parts for a caller-supplied stack `[lo, lo + size)`: TLS, block, descriptor and keys
    /// come from a separate mapping (offsets as for [`Layout::new`] with a minimal stack and no
    /// guard). Also returns the stack pointer: the stack's top, 16-aligned, less the two start
    /// words.
    pub fn for_user_stack(lo: usize, size: usize, image: Option<&TlsImage>) -> Option<(Layout, usize)> {
        let l = Layout::new(0, 0, image)?;
        let top = lo.checked_add(size)? & !15;
        Some((l, top.checked_sub(16)?))
    }
}

// ------------------------------------------------------------------------------ keys

/// The process-wide key table (§22.6): per key a sequence number (odd while the key exists)
/// and a destructor. Deleting a key makes the number even, so values set under the old key are
/// ignored if the slot is reused (each thread's entry records the number it was set under).
pub struct KeyTable {
    seq: [AtomicU32; KEYS],
    dtor: [AtomicUsize; KEYS],
}

impl Default for KeyTable {
    fn default() -> KeyTable {
        KeyTable::new()
    }
}

impl KeyTable {
    /// No keys (all zero).
    pub const fn new() -> KeyTable {
        KeyTable { seq: [const { AtomicU32::new(0) }; KEYS], dtor: [const { AtomicUsize::new(0) }; KEYS] }
    }

    /// `pthread_key_create`: a free key with destructor `dtor` (0 for none), or `None` when all
    /// are in use (`EAGAIN`).
    pub fn create(&self, dtor: usize) -> Option<usize> {
        for (k, (s, d)) in self.seq.iter().zip(&self.dtor).enumerate() {
            let v = s.load(Relaxed);
            if v % 2 == 0 && s.compare_exchange(v, v.wrapping_add(1), AcqRel, Relaxed).is_ok() {
                d.store(dtor, Release);
                return Some(k);
            }
        }
        None
    }

    /// `pthread_key_delete`: false if `k` is not a live key (`EINVAL`).
    pub fn delete(&self, k: usize) -> bool {
        let Some(s) = self.seq.get(k) else { return false };
        let v = s.load(Acquire);
        v % 2 == 1 && s.compare_exchange(v, v.wrapping_add(1), AcqRel, Relaxed).is_ok()
    }

    /// The live key `k`'s sequence number, or `None`.
    pub fn live(&self, k: usize) -> Option<u32> {
        self.seq.get(k).map(|s| s.load(Acquire)).filter(|v| v % 2 == 1)
    }

    /// The live key `k`'s destructor (0 for none).
    pub fn dtor(&self, k: usize) -> usize {
        self.dtor.get(k).map_or(0, |d| d.load(Acquire))
    }

    /// One past the highest key ever created (bounds the exit-time scan).
    pub fn span(&self) -> usize {
        self.seq.iter().rposition(|s| s.load(Relaxed) != 0).map_or(0, |i| i + 1)
    }
}

/// A thread's key area: per key the value and the sequence number it was set under.
pub struct Specific<'a> {
    words: &'a [AtomicUsize],
}

impl<'a> Specific<'a> {
    /// The area over `words` (`2 * KEYS` words).
    pub fn new(words: &'a [AtomicUsize]) -> Specific<'a> {
        Specific { words }
    }

    /// `pthread_getspecific`: the value set under the key's current life, else 0.
    pub fn get(&self, table: &KeyTable, k: usize) -> usize {
        match (table.live(k), self.words.get(2 * k), self.words.get(2 * k + 1)) {
            (Some(seq), Some(v), Some(s)) if s.load(Relaxed) == usize::try_from(seq).unwrap_or(0) => v.load(Relaxed),
            _ => 0,
        }
    }

    /// `pthread_setspecific`: false if `k` is not a live key (`EINVAL`).
    pub fn set(&self, table: &KeyTable, k: usize, value: usize) -> bool {
        match (table.live(k), self.words.get(2 * k), self.words.get(2 * k + 1)) {
            (Some(seq), Some(v), Some(s)) => {
                v.store(value, Relaxed);
                s.store(usize::try_from(seq).unwrap_or(0), Relaxed);
                true
            }
            _ => false,
        }
    }

    /// Thread exit (§22.6): up to `DESTRUCTOR_ROUNDS` rounds, each calling `call(dtor, value)`
    /// for every live key whose value is non-NULL and has a destructor, after clearing the
    /// value; stops early when a round finds nothing.
    pub fn run_destructors(&self, table: &KeyTable, mut call: impl FnMut(usize, usize)) {
        for _ in 0..DESTRUCTOR_ROUNDS {
            let mut any = false;
            for k in 0..table.span() {
                let v = self.get(table, k);
                let d = table.dtor(k);
                if v != 0 && d != 0 {
                    let _ = self.set(table, k, 0);
                    any = true;
                    call(d, v);
                }
            }
            if !any {
                return;
            }
        }
    }
}

// ------------------------------------------------------------------------------ attributes

/// A new thread's attributes, decoded from a `pthread_attr_t`.
#[derive(Clone, Copy, Debug, Default)]
pub struct Attr {
    /// Start detached.
    pub detached: bool,
    /// Stack bytes (0: the default).
    pub stack_size: usize,
    /// Guard bytes (`None`: the default).
    pub guard: Option<usize>,
    /// A caller-supplied stack: lowest address and size.
    pub stack: Option<(usize, usize)>,
    /// Explicit scheduling: policy and priority.
    pub sched: Option<(i32, i32)>,
    /// The initial signal mask (`None`: the creator's).
    pub sigmask: Option<u64>,
    /// The CPU affinity: address and size of the caller's mask (`None`: inherit).
    pub affinity: Option<(usize, usize)>,
}


/// `PTHREAD_STACK_MIN` as `sysconf(_SC_THREAD_STACK_MIN)` answers on x86-64 (16 KiB).
pub const STACK_MIN: usize = 16384;

/// The default stack size (§22.2): the `RLIMIT_STACK` soft limit at startup, 2 MiB if that is
/// unlimited (or unknown), at least the minimum.
pub fn default_stack(rlimit_cur: Option<u64>) -> usize {
    const TWO_MIB: usize = 2 << 20;
    match rlimit_cur.and_then(|v| usize::try_from(v).ok()) {
        Some(v) if v < (1 << 40) => v.max(STACK_MIN).next_multiple_of(PAGE),
        _ => TWO_MIB,
    }
}

/// Whether a thread's mapping is marked `MADV_NOHUGEPAGE` (D-22.9), from `uname`'s release
/// (`"5.15.167.4-microsoft-standard-WSL2"`). Before 6.7 the kernel lets a `MAP_STACK` mapping
/// take transparent huge pages, and one that merged with an adjacent mapping can then make
/// that mapping's untouched pages resident; 6.7 made `MAP_STACK` imply `VM_NOHUGEPAGE`. A
/// release that does not parse counts as old.
pub fn stack_needs_nohugepage(release: &[u8]) -> bool {
    fn leading(s: &[u8]) -> Option<u32> {
        let mut digits = s.iter().take_while(|b| b.is_ascii_digit()).peekable();
        digits.peek()?;
        digits.try_fold(0u32, |n, &d| n.checked_mul(10)?.checked_add(char::from(d).to_digit(10)?))
    }
    let mut parts = release.split(|&b| b == b'.');
    match (parts.next().and_then(leading), parts.next().and_then(leading)) {
        (Some(major), Some(minor)) => (major, minor) < (6, 7),
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::arithmetic_side_effects, clippy::indexing_slicing)]
    use super::*;

    fn zeroed() -> Thread {
        Thread {
            robust: Default::default(),
            flags: AtomicU32::new(0),
            join: AtomicU32::new(0),
            cancel: AtomicU32::new(0),
            setxid: AtomicU32::new(0),
            start: AtomicUsize::new(0),
            arg: AtomicUsize::new(0),
            result: AtomicUsize::new(0),
            cleanup_jmp: AtomicUsize::new(0),
            cleanup_old: AtomicUsize::new(0),
            joining: AtomicUsize::new(0),
            map_base: AtomicUsize::new(0),
            map_len: AtomicUsize::new(0),
            stack_lo: AtomicUsize::new(0),
            stack_size: AtomicUsize::new(0),
            guard: AtomicUsize::new(0),
            sigmask: AtomicU64::new(0),
            next: AtomicUsize::new(0),
            prev: AtomicUsize::new(0),
            sched_policy: AtomicI32::new(0),
            sched_prio: AtomicI32::new(0),
            keys: AtomicUsize::new(0),
            started: AtomicU32::new(0),
            start_error: AtomicU32::new(0),
            slots: AtomicUsize::new(0),
            affinity: AtomicUsize::new(0),
            affinity_len: AtomicUsize::new(0),
            tp: AtomicUsize::new(0),
            reclaim: AtomicU32::new(0),
            rseq: Rseq::new(),
        }
    }

    #[test]
    fn the_layout_stacks_guard_stack_tls_block_descriptor_and_keys() {
        let img = TlsImage { addr: 0, filesz: 8, memsz: 200, align: 32 };
        let l = Layout::new(64 << 10, PAGE, Some(&img)).unwrap();
        assert_eq!(l.guard, PAGE);
        assert_eq!(l.tp % 64, 0);
        assert_eq!(l.tp - l.tls, 224, "variant II: round_up(memsz, align) below the thread pointer");
        assert!(l.sp < l.tls && l.sp % 16 == 0 && l.sp + 16 <= l.tls);
        assert!(l.sp >= l.guard + (64 << 10) - 64, "the whole stack is usable");
        assert!(l.desc >= l.tp + BLOCK_SIZE && l.desc % 64 == 0);
        assert!(l.keys >= l.desc + DESC_SIZE);
        assert!(l.len >= l.keys + KEY_AREA + SLOT_AREA && l.len % PAGE == 0);
        let none = Layout::new(0, 0, None).unwrap();
        assert_eq!(none.guard, 0);
        assert!(none.sp > 0 && none.sp % 16 == 0, "a minimum stack of a page");
        assert_eq!(Layout::new(usize::MAX - 10, PAGE, None), None);
        assert_eq!(Layout::new(PAGE, 0, Some(&TlsImage { addr: 0, filesz: 0, memsz: 1, align: 3 })), None);
        let (u, sp) = Layout::for_user_stack(0x10_0000, 0x8008, None).unwrap();
        assert_eq!(sp, 0x10_8000 - 16);
        assert_eq!(u.guard, 0);
    }

    #[test]
    fn robust_heads_point_to_themselves() {
        let t = zeroed();
        t.init_robust(0x1000);
        assert_eq!(t.robust[0].load(Relaxed), 0x1000);
        assert_eq!(isize::from_ne_bytes(t.robust[1].load(Relaxed).to_ne_bytes()), -32);
        assert_eq!(RSEQ_IN_DESC % 32, 0);
        assert!(t.take_reclaim());
        assert!(!t.take_reclaim(), "one reclaimer");
    }

    #[test]
    fn cancellation_is_due_only_when_requested_enabled_and_not_acting() {
        assert!(cancel_due(CANCEL_REQUESTED));
        assert!(cancel_due(CANCEL_REQUESTED | CANCEL_ASYNC));
        assert!(!cancel_due(CANCEL_REQUESTED | CANCEL_DISABLED));
        assert!(!cancel_due(CANCEL_REQUESTED | CANCEL_ACTING));
        assert!(!cancel_due(0));
        assert_eq!(CANCEL_WINDOW_MASK, 13, "the constant machine/thread_start.S tests");
    }

    #[test]
    fn joins_and_detaches_claim_once() {
        let s = AtomicU32::new(JOINABLE);
        assert!(claim_join(&s));
        assert!(!claim_join(&s), "a second join is refused");
        assert_eq!(detach(&s, false), None, "so is a detach of a joined thread");
        let d = AtomicU32::new(JOINABLE);
        assert_eq!(detach(&d, true), Some(true));
        assert!(!claim_join(&d));
    }

    #[test]
    fn keys_hold_values_per_life_and_run_destructors_in_rounds() {
        let t = KeyTable::new();
        let a = t.create(0x111).unwrap();
        let b = t.create(0).unwrap();
        assert_ne!(a, b);
        let words: Vec<AtomicUsize> = (0..2 * KEYS).map(|_| AtomicUsize::new(0)).collect();
        let sp = Specific::new(&words);
        assert_eq!(sp.get(&t, a), 0, "initially NULL");
        assert!(sp.set(&t, a, 7));
        assert!(sp.set(&t, b, 9));
        assert_eq!((sp.get(&t, a), sp.get(&t, b)), (7, 9));
        assert!(t.delete(b));
        assert!(!t.delete(b), "deleting twice is EINVAL");
        assert_eq!(sp.get(&t, b), 0);
        assert!(!sp.set(&t, b, 1));
        let b2 = t.create(0).unwrap();
        assert_eq!(b2, b, "the slot is reused");
        assert_eq!(sp.get(&t, b2), 0, "a value set under the deleted key is not visible");
        // A destructor that sets the value again runs in further rounds, at most four.
        let mut calls = 0;
        sp.run_destructors(&t, |d, v| {
            assert_eq!((d, v), (0x111, 7));
            calls += 1;
            let _ = sp.set(&t, a, 7);
        });
        assert_eq!(calls, DESTRUCTOR_ROUNDS);
        assert_eq!(sp.get(&t, a), 7, "the last round's value stays");
        let mut more = 0;
        let _ = sp.set(&t, a, 0);
        sp.run_destructors(&t, |_, _| more += 1);
        assert_eq!(more, 0, "NULL values get no call");

    }

    #[test]
    fn the_default_stack_follows_rlimit_stack() {
        assert_eq!(default_stack(Some(8 << 20)), 8 << 20);
        assert_eq!(default_stack(None), 2 << 20);
        assert_eq!(default_stack(Some(u64::MAX)), 2 << 20, "unlimited");
        assert_eq!(default_stack(Some(100)), STACK_MIN);
    }

    #[test]
    fn stacks_avoid_huge_pages_before_6_7() {
        assert!(stack_needs_nohugepage(b"5.15.167.4-microsoft-standard-WSL2"));
        assert!(stack_needs_nohugepage(b"6.6.30"));
        assert!(!stack_needs_nohugepage(b"6.7.0-rc1"), "6.7 marks MAP_STACK itself");
        assert!(!stack_needs_nohugepage(b"6.12.1-arch1-1"));
        assert!(!stack_needs_nohugepage(b"10.0"));
        assert!(stack_needs_nohugepage(b""), "unknown: old");
        assert!(stack_needs_nohugepage(b"x.y"));
    }
}
