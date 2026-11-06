# LIBM-SPECIAL-GAMMA-TIER handoff (run 3: LIBM-SPECIAL-GAMMA-TIER-3, merge only)

State: branch `task/libm-special-gamma-tier-3` (runs 1-2 on `task/libm-special-gamma-tier-2`, then this run's merge),
HEAD in handoff/reports/LIBM-SPECIAL-GAMMA-TIER-3.json. Role keke:~/w/n2. Pattern: handoff/libm-special-fast-tier.md
(erf/erfc). Accuracy contract unchanged (correct rounding, 384-bit fallback, ruling #2760 option A): no differences
row. Full report: handoff/libm-special-gamma-tier-report.md.

## Design (module docs of crates/math/src/fast_gamma.rs and crates/mathtools/src/gamma_tables.rs carry the proof)
* Generator `gamma_tables.rs` (`cargo xtask tables`) writes the gamma family's own file `crates/math/src/coeffs/gamma.rs`
  (closed after `erf.rs` in `mathtools/src/tables.rs`; `coeffs.rs` declares `gamma` and re-exports it through
  `coeffs::tables`). Tables: `LG_T` (ln Gamma on [2^-5, 16), 144 slots, bound 2^-75.22 relative), `LG_Z1`/`LG_Z2`.
* Runtime `fast_gamma.rs`: ln|Gamma| = L - ln|D|, Gamma = e^L/D for 2^-60 <= |x| < 16. Wired in
  `special::lgamma/tgamma` for doubles; old bodies `*_accurate` (fallback). No boundary wrappers: the gamma hot
  module (`crates/boundary/src/hot/gamma.rs`) stays as next has it.
* Near lgamma's negative zeros: refusal to the 384-bit path by design (absolute bound).

## Done in run 3
Merged origin/next c7c320a2 (per-family tables, per-family hot modules). Conflicts: `coeffs/tables.rs` (took next's
deletion), `math/src/lib.rs` (both module lines), `mathtools/src/tables.rs` (gamma emitted into `gamma.rs`).
Regenerated with `cargo xtask tables` on the role (job libm-special-gamma-tier-3-tables): only `coeffs/gamma.rs` new,
the other coeffs files byte-identical; `crates/math/KERNELS.md` regenerated (indexes fast_gamma.rs). Kernel code
unchanged since the verified head e104133f: `git diff e104133f HEAD -- crates/math/src/fast_gamma.rs
crates/math/tests/fast_gamma.rs` is empty; gamma_tables.rs changed one doc line (output path). Plants file
handoff/plants/libm-special-gamma-tier-3.plants (table-entry plant now on coeffs/gamma.rs). Gate, plants, counts:
see the report.

## Open / next step
1. Counts vs the 1.05x target (see the report). Next: profile one call per phase (entry `mret::lgamma`, split,
   Taylor row, log_dd/exp_dd, settle) against glibc's total before any redesign.
2. Out of scope so far: x >= 16 (Stirling), x <= -16 (still the 384-bit path), |x| < 2^-60, float rows.
3. A double-double step at the negative zeros would cut the near-zero refusals (up to ~44% within 2^-48 relative of
   a zero), but the uniform-range rate is 5 in 10^5; not worth it unless a workload sits on the zeros.

## Things not to redo
* Runs 1-2's design and generator assertions hold (all slot bounds <= 2^-75.22); the merge to per-family files.
* The emitted doc line on the LG_* tables says "(module `fast_special`)" (shared `emit_table` text); cosmetic.
