//! Superseded versioned contracts (C0.6, H7; LG-2 `COMPAT-BEHAVIOUR`, R15-1).
//!
//! A `behaviour` row of `spec/abi/compat-plan.tsv` is an old version whose contract differs
//! from the default one. Each is a Rust entry `liella_compat_<name>` in the boundary crate that
//! the generated `csys/src/machine/compat_aliases.S` gives the old version (single `@`; never
//! the default), linked into `libc.so.6` only (R15-2). The rules that set an old contract apart
//! live here; the entries combine them with the default code. Black-box probes of glibc 2.39
//! settled each rule (handoff/compat-behaviour.md; glob: `spec/behaviour/patterns.md`, "glob compat"):
//!
//! * `memcpy@GLIBC_2.2.5`: overlap-safe, the `memmove` contract (no rule needed here; the entry
//!   calls the move kernel);
//! * `realpath@GLIBC_2.2.5`: [`realpath_2_2_5`], no allocation of the result;
//! * `glob@GLIBC_2.2.5` and `glob64@GLIBC_2.2.5`: [`GlobExistence`], with `GLOB_ALTDIRFUNC` the
//!   existence of a name is checked with the caller's `gl_stat`, not `gl_lstat`;
//! * `fmemopen@GLIBC_2.2.5`: the `old` buffer rules of `stdio_compat::mem_backing` (size 0 is
//!   `EINVAL`, reads run to the size, `a` places only the first position, `w` stores a NUL first).
//!
//! * `regexec@GLIBC_2.2.5`: [`regexec_2_2_5_eflags`], only `REG_NOTBOL` and `REG_NOTEOL` count
//!   (`REG_STARTEND` and unknown bits are ignored: the subject is the NUL-terminated string);
//! * `nftw@GLIBC_2.2.5` and `nftw64@GLIBC_2.2.5`: [`nftw_2_2_5_flags`], only `FTW_PHYS`,
//!   `FTW_MOUNT`, `FTW_CHDIR` and `FTW_DEPTH` count (`FTW_ACTIONRETVAL` and unknown bits are
//!   ignored, never `EINVAL`);
//! * `quick_exit@GLIBC_2.10`: the calling thread's C++ `thread_local` destructors run first,
//!   then the quick-exit entries (the entry calls the boundary's `quick_exit_2_10`).
//!
//! The other `behaviour` rows assigned to LG-2 (`posix_spawn@GLIBC_2.2.5`,
//! `posix_spawnp@GLIBC_2.2.5`) are still missing (see the handoff).

use liella::error::{Errno, Result};

use crate::fs_tree::{FTW_CHDIR, FTW_DEPTH, FTW_MOUNT, FTW_PHYS};

/// `REG_NOTBOL` and `REG_NOTEOL` (`<regex.h>`, x86-64 values).
const REG_NOTBOL_NOTEOL: i32 = 1 | 2;

/// `regexec@GLIBC_2.2.5`'s execution flags: the bits it honours of `eflags`. Probe-settled
/// (`spec/behaviour/compat.glibc.txt`, `regexec@GLIBC_2.2.5` lines): `REG_STARTEND` (4) is
/// not honoured (the subject runs to its NUL, the match is reported from the string start) and
/// an unknown bit (0x80) is not an error.
pub const fn regexec_2_2_5_eflags(eflags: i32) -> i32 {
    eflags & REG_NOTBOL_NOTEOL
}

/// `nftw@GLIBC_2.2.5`'s flags: the bits it honours of `flags`. Probe-settled
/// (`nftw@GLIBC_2.2.5` lines): `FTW_ACTIONRETVAL` is not honoured (a callback's value ends the
/// walk and is returned, as without it) and an unknown bit (0x20, 0x100, 0x40000000) is ignored
/// rather than `EINVAL`.
pub const fn nftw_2_2_5_flags(flags: i32) -> i32 {
    flags & (FTW_PHYS | FTW_MOUNT | FTW_CHDIR | FTW_DEPTH)
}

/// `realpath@GLIBC_2.2.5`'s own rule: a NULL `resolved` buffer is an error (`EINVAL`), checked
/// before anything else (a NULL path included); with a buffer the call is the default
/// `realpath`. `resolved_given` says whether the caller passed a buffer.
pub fn realpath_2_2_5(resolved_given: bool) -> Result<()> {
    if resolved_given { Ok(()) } else { Err(Errno::EINVAL) }
}

/// Which of the caller's `GLOB_ALTDIRFUNC` stat functions answers "does this name exist".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GlobExistence {
    /// `glob@@GLIBC_2.27` (the default): `gl_lstat`, so a dangling link exists.
    Lstat,
    /// `glob@GLIBC_2.2.5`: `gl_stat`, so a dangling link does not exist.
    Stat,
}

impl GlobExistence {
    /// The rule for the version: `old` is the `GLIBC_2.2.5` entry.
    pub const fn for_version(old: bool) -> Self {
        if old { Self::Stat } else { Self::Lstat }
    }

    /// Picks the function for an existence check from the pair (`lstat`, `stat`).
    pub fn pick<T>(self, lstat: T, stat: T) -> T {
        match self {
            Self::Lstat => lstat,
            Self::Stat => stat,
        }
    }

    /// Whether `GLOB_NOCHECK` on a plain name skips the existence check (the record's
    /// `glob-alt@2.2.5 [v/dang] NOCHECK|ALTDIRFUNC` line: neither stat function is called).
    pub const fn nocheck_skips_existence(self) -> bool {
        matches!(self, Self::Stat)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // spec/behaviour/compat-versioned.md "realpath@GLIBC_2.2.5": NULL resolved -> EINVAL.
    #[test]
    fn realpath_2_2_5_refuses_a_null_buffer() {
        assert_eq!(realpath_2_2_5(false), Err(Errno::EINVAL));
        assert_eq!(realpath_2_2_5(true), Ok(()));
    }

    // spec/behaviour/compat-versioned.md "glob@GLIBC_2.2.5": gl_stat for existence.
    #[test]
    fn glob_existence_follows_the_version() {
        assert_eq!(GlobExistence::for_version(true).pick("lstat", "stat"), "stat");
        assert_eq!(GlobExistence::for_version(false).pick("lstat", "stat"), "lstat");
    }

    // no spec clause: compat-plan.tsv behaviour row regexec@GLIBC_2.2.5 (LG-2; probe-settled).
    #[test]
    fn regexec_2_2_5_keeps_only_notbol_noteol() {
        assert_eq!(regexec_2_2_5_eflags(1 | 2 | 4 | 0x80), 3);
        assert_eq!(regexec_2_2_5_eflags(4), 0);
    }

    // no spec clause: compat-plan.tsv behaviour row nftw@GLIBC_2.2.5 (LG-2; probe-settled).
    #[test]
    fn nftw_2_2_5_drops_actionretval_and_unknown_bits() {
        assert_eq!(nftw_2_2_5_flags(1 | 2 | 4 | 8), 15);
        assert_eq!(nftw_2_2_5_flags(16 | 0x20 | 0x100 | 0x4000_0000 | 1), 1);
    }
}
