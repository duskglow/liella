//! Wildcards, globbing, regular expressions, word expansion (spec §14.7-14.10).
//!
//! Implementation phase: 12 (implementation-plan.md §6): [`fnmatch`] (§14.7), [`glob`] (§14.8),
//! [`regex`] (§14.9) and [`wordexp`] (§14.10). The bracket-expression and locale machinery is the
//! regular expressions' (`regex::text`), shared by all of them.

pub mod fnmatch;
pub mod glob;
pub mod regex;
pub mod wordexp;

/// The regular-expression engine as it stood before the optimization pass (main's working tree
/// on 2026-10-05: the T5 and T6 correctness fixes, none of the optimization commits), kept for
/// tests only as an oracle for the optimized engine (`regex_oracle_diff`); never in the products.
#[cfg(test)]
#[allow(dead_code, unreachable_pub, missing_docs)]
mod regex_oracle;
#[cfg(test)]
mod regex_oracle_diff;
