//! Mathematics (spec §10). One implementation per operation, generic over the floating
//! formats of spec §10.0, instead of seven hand-written copies (decision D-10.1). Exceptions are
//! reported through IEEE flags and a typed status; errno is set only by the translation layer
//! (decision D-10.2).
//!
//! Every function takes its operands in a format `F: Float` (binary32, binary64, x87 extended,
//! binary128), the rounding direction where the result depends on it (an explicit argument,
//! rule C2), and returns `(value, Status)`. The library never reads or writes the floating-point
//! environment itself; `fenv` (over `liella-rt::fpu`) is for its callers.
//!
//! `#![forbid(unsafe_code)]`. The FPU control registers are reached through `liella-rt::fpu`.
//! Nothing here allocates: the `libm` product is linked without a heap of its own (D-10.7).

#![cfg_attr(not(test), no_std)]
#![forbid(unsafe_code)]
#![cfg_attr(not(test), deny(clippy::disallowed_methods))] // S8: fallible allocation only

pub mod float;
pub mod status;
pub mod fenv;
pub mod arith;
pub mod classify;
pub mod round;
pub mod manip;
pub mod rem;
pub mod minmax;
pub mod fused;
pub mod dd;
pub mod elem;
pub mod exp_log;
pub mod fast32;
pub mod fast64;
pub mod pow;
pub mod trig;
pub mod hyper;
pub mod mp;
pub mod mpw;
pub mod special;
pub mod wide;
pub mod real;
pub mod complex;
pub mod ulp;
pub mod stats;
mod coeffs;

pub use float::{Float, F128, F80};
pub use status::{Flags, MathError, Rounding, Status};
