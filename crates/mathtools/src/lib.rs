//! The `libm` test oracle and tooling (decision D-10.3).
//!
//! * [`mp`]: arbitrary-precision binary floating-point arithmetic written in this project, the
//!   reference every accuracy measurement and test of `liella-math` is checked against.
//! * [`remez`] and the `remez` binary: minimax (Remez exchange) fitting of the polynomial
//!   coefficients that `liella-math` uses, with each derivation recorded in the generated file.
//! * [`cmp`] and the `mathcmp` binary: the glibc comparison and accuracy measurement.
//! * [`cplx`]: the complex functions' oracle (both parts, from the real oracles).
//!
//! Test and tool code only: no library crate or product depends on this crate.
//!
//! # Provenance (clean room)
//!
//! Everything here is implemented from the mathematics and the published literature, without
//! reference to the source code of any other arithmetic or mathematical library:
//!
//! * integer arithmetic: schoolbook multiplication, Knuth's Algorithm D for division
//!   (*The Art of Computer Programming*, vol. 2, 4.3.1), Newton's iteration for integer square
//!   and cube roots;
//! * constants: Machin's formula for pi and `ln 2 = 2 atanh(1/3)`; the tests check them
//!   against the Gauss-Legendre (AGM) iteration and the series `ln 2 = sum 1/(k 2^k)`;
//! * elementary functions: Taylor series with argument reduction and halving/doubling
//!   identities, as in Brent and Zimmermann, *Modern Computer Arithmetic*, ch. 4, and
//!   Muller, *Elementary Functions*; trigonometric reduction by a run-time multiple-precision
//!   2/pi carried to as many bits as the argument needs (no table);
//! * correct rounding: Ziv's strategy (Ziv, 1991);
//! * minimax fitting: the Remez exchange algorithm as described in numerical-analysis texts
//!   (equioscillation theorem, Chebyshev initial reference, multi-point exchange).

#![forbid(unsafe_code)]
// Big-number limb arithmetic on bounded indices and widths, as in the formats code.
#![allow(clippy::arithmetic_side_effects, clippy::as_conversions, clippy::indexing_slicing)]

pub mod cmp;
pub mod cplx;
pub mod mp;
pub mod remez;
pub mod tables;
