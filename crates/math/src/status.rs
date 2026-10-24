//! What an operation reports besides its value (D-10.2): the IEEE exception flags it raises and
//! the error the C interface stores in `errno`. Internal functions return `(value, Status)`; the
//! translation layer raises the flags (`liella_rt::fpu::raise`) and sets `errno`, the only place
//! math touches either.

pub use liella_rt::fpu::{Flags, Rounding};

/// The `errno` side of a result (spec §10.0).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum MathError {
    /// None.
    #[default]
    None,
    /// A domain error (`EDOM`).
    Domain,
    /// A pole or range error (`ERANGE`).
    Range,
}

/// Flags plus the `errno` report, and a hint for the envelope: `hw`, the MXCSR flag bits the
/// computation itself may have set in hardware (D-10.13, #1360). The in-place envelope exit skips
/// its MXCSR write when the caller's saved word already holds every bit of `flags` and `hw`, since
/// the write would then store what MXCSR already holds. `hw` is conservative: every constructor
/// gives [`Status::HW_ANY`] (all six bits) and only a path that proves a smaller set narrows it
/// ([`Status::hw`]). It is not part of the report, so equality ignores it.
#[derive(Clone, Copy, Debug)]
pub struct Status {
    /// Exception flags to raise.
    pub flags: Flags,
    /// The error to report through `errno`.
    pub error: MathError,
    /// The MXCSR flag bits (positions 0-5, the denormal-operand bit 1 included) the computation
    /// may have raised in hardware; a superset is always correct.
    pub hw: Flags,
}

impl PartialEq for Status {
    fn eq(&self, o: &Status) -> bool {
        self.flags == o.flags && self.error == o.error
    }
}

impl Eq for Status {}

impl Default for Status {
    fn default() -> Status {
        Status::OK
    }
}

const RANGE_FLAGS: Flags = Flags(Flags::OVERFLOW.0 | Flags::UNDERFLOW.0);

impl Status {
    /// Every MXCSR flag bit, the denormal-operand bit included: what any computation may raise.
    pub const HW_ANY: Flags = Flags(0x3f);
    /// MXCSR's denormal-operand flag (bit 1; no C `FE_` flag): set by an SSE operation on a
    /// subnormal operand when DAZ is off.
    pub const HW_DENORMAL: Flags = Flags(0x02);
    /// Nothing to report.
    pub const OK: Status = Status { flags: Flags::NONE, error: MathError::None, hw: Status::HW_ANY };
    /// `FE_INVALID` and `EDOM`.
    pub const DOMAIN: Status = Status { flags: Flags::INVALID, error: MathError::Domain, hw: Status::HW_ANY };
    /// `FE_INVALID` without `errno`.
    pub const INVALID: Status = Status { flags: Flags::INVALID, error: MathError::None, hw: Status::HW_ANY };
    /// `FE_DIVBYZERO` and `ERANGE` (a pole).
    pub const POLE: Status = Status { flags: Flags::DIVBYZERO, error: MathError::Range, hw: Status::HW_ANY };

    /// Only flags.
    pub const fn flags(flags: Flags) -> Status {
        Status { flags, error: MathError::None, hw: Status::HW_ANY }
    }

    /// The same report, stating that the computation raised at most `hw` in hardware: only for a
    /// path whose every operation is accounted for (fast32's kernels).
    #[must_use]
    pub const fn hw(self, hw: Flags) -> Status {
        Status { flags: self.flags, error: self.error, hw }
    }

    /// The same flags, reporting `error`.
    #[must_use]
    pub const fn with(self, error: MathError) -> Status {
        Status { flags: self.flags, error, hw: self.hw }
    }

    /// `ERANGE` if the flags include overflow or underflow.
    #[must_use]
    pub const fn range_errno(self) -> Status {
        if self.flags.any(RANGE_FLAGS) { self.with(MathError::Range) } else { self }
    }

    /// `ERANGE` if the flags include overflow (an underflow keeps `errno`).
    #[must_use]
    pub const fn overflow_errno(self) -> Status {
        if self.flags.any(Flags::OVERFLOW) { self.with(MathError::Range) } else { self }
    }

    /// The same report without `errno`.
    #[must_use]
    pub const fn no_errno(self) -> Status {
        self.with(MathError::None)
    }

    /// Both reports: the flags united, the first error kept.
    #[must_use]
    pub const fn and(self, o: Status) -> Status {
        let error = if matches!(self.error, MathError::None) { o.error } else { self.error };
        Status { flags: self.flags.union(o.flags), error, hw: self.hw.union(o.hw) }
    }
}
