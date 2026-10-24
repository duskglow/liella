//! The single error type of the library (decision D-X.2): a kernel-numbered `Errno`.

pub use liella_kernel::errno::{Errno, KResult};

/// Result of a library operation.
pub type Result<T> = KResult<T>;
