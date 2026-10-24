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
//! The other `behaviour` rows assigned to LG-2 are still missing (see the handoff).

use liella::error::{Errno, Result};

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
}
