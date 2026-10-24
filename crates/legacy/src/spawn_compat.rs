//! The C shapes of spec §18.4: `posix_spawnattr_t` (336 bytes) and
//! `posix_spawn_file_actions_t` (80 bytes), Appendix C.
//!
//! The attributes object is plain data: its documented members hold the values, with the
//! 1024-bit `sigset_t`s of §20.3 (only the first 64 signals mean anything, D-20.3; the
//! library's internal signal is never stored, D-20.7). The file-actions object holds the address
//! and length of a caller-heap block with the actions in `liella::process`'s encoding
//! (`__actions`, `__used`; `__allocated` is the block's size).

use liella::process::SpawnAttr;
use liella_abi::layout::sigset_t;

use crate::signal_compat::{c_set, kernel_set};

/// `posix_spawnattr_t` as a C program lays it out (Appendix C; the layout is tested).
#[derive(Clone, Copy)]
#[repr(C)]
pub struct CSpawnAttr {
    /// `__flags`.
    pub flags: i16,
    _pad0: i16,
    /// `__pgrp`.
    pub pgrp: i32,
    /// `__sd`: the signals `POSIX_SPAWN_SETSIGDEF` resets.
    pub sd: sigset_t,
    /// `__ss`: the mask `POSIX_SPAWN_SETSIGMASK` installs.
    pub ss: sigset_t,
    /// `__sp.sched_priority`.
    pub priority: i32,
    /// `__policy`.
    pub policy: i32,
    /// `__cgroup`.
    pub cgroup: i32,
    _pad: [i32; 15],
}

impl CSpawnAttr {
    /// A new object (`posix_spawnattr_init`): every value 0 and both sets empty.
    pub fn new() -> CSpawnAttr {
        let empty = sigset_t { __val: [0; 16] };
        CSpawnAttr { flags: 0, _pad0: 0, pgrp: 0, sd: empty, ss: empty, priority: 0, policy: 0, cgroup: 0, _pad: [0; 15] }
    }

    /// The values as the spawn takes them.
    pub fn values(&self) -> SpawnAttr {
        SpawnAttr {
            flags: u32::from(u16::from_ne_bytes(self.flags.to_ne_bytes())),
            pgroup: self.pgrp,
            sigdefault: kernel_set(&self.sd),
            sigmask: kernel_set(&self.ss),
            policy: u32::from_ne_bytes(self.policy.to_ne_bytes()),
            priority: self.priority,
            cgroup: self.cgroup,
        }
    }

    /// Store the flags (checked by `liella::process::spawn_flags`, so they fit in `short`).
    pub fn set_flags(&mut self, flags: u32) {
        self.flags = i16::from_ne_bytes(u16::try_from(flags & 0xffff).unwrap_or(0).to_ne_bytes());
    }

    /// A set as the C program reads it back (the internal signal hidden).
    pub fn c_set(set: &sigset_t) -> sigset_t {
        c_set(kernel_set(set))
    }
}

impl Default for CSpawnAttr {
    fn default() -> CSpawnAttr {
        CSpawnAttr::new()
    }
}

/// `posix_spawn_file_actions_t` as a C program lays it out (Appendix C; the layout is tested).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[repr(C)]
pub struct CFileActions {
    /// `__allocated`: the size of the block at `actions`.
    pub allocated: i32,
    /// `__used`: the bytes of encoded actions in it.
    pub used: i32,
    /// `__actions`: the block's address (0: no actions).
    pub actions: usize,
    _pad: [i32; 16],
}

impl CFileActions {
    /// An empty list (`posix_spawn_file_actions_init`).
    pub fn new() -> CFileActions {
        CFileActions::default()
    }

    /// The block's address and the length of its encoded actions.
    pub fn block(&self) -> (usize, usize) {
        (self.actions, usize::try_from(self.used).unwrap_or(0))
    }

    /// Record a new block holding `len` bytes of actions.
    pub fn set_block(&mut self, addr: usize, len: usize) {
        let n = i32::try_from(len).unwrap_or(i32::MAX);
        self.actions = addr;
        self.used = n;
        self.allocated = n;
    }
}
