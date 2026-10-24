//! Descriptors (spec §0.4 Fd: small non-negative integers, lowest free number allocated,
//! ⚑ KERN).

/// A file descriptor number. It is a plain number: holding one grants nothing, and the kernel
/// reports `EBADF` for numbers that are not open, so any value is sound to pass.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Fd(i32);

impl Fd {
    /// `AT_FDCWD`: "relative to the current directory" in `*at` calls.
    pub const CWD: Fd = Fd(-100);
    /// Standard input.
    pub const STDIN: Fd = Fd(0);
    /// Standard output.
    pub const STDOUT: Fd = Fd(1);
    /// Standard error.
    pub const STDERR: Fd = Fd(2);

    /// The descriptor with number `n` (any value; numbers that are not open fail with `EBADF`).
    pub const fn new(n: i32) -> Fd {
        Fd(n)
    }

    /// The number.
    pub const fn raw(self) -> i32 {
        self.0
    }
}
