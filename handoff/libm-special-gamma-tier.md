# LIBM-SPECIAL-GAMMA-TIER handoff (run 2: LIBM-SPECIAL-GAMMA-TIER-2)

State: branch `task/libm-special-gamma-tier-2` (run 1's WIP commits 18ad79de/68657bc9, then run 2), merged with
origin/next 4c33d4c8; HEAD in handoff/reports/LIBM-SPECIAL-GAMMA-TIER-2.json. Role keke:~/w/n3. Pattern:
handoff/libm-special-fast-tier.md (erf/erfc). Accuracy contract unchanged (correct rounding, 384-bit fallback, ruling
#2760 option A): no differences row. Full report: handoff/libm-special-gamma-tier-report.md.

## Design (module docs of crates/math/src/fast_gamma.rs and crates/mathtools/src/gamma_tables.rs carry the proof)
* Generator `gamma_tables.rs` (`cargo xtask tables`): ln Gamma Taylor rows from Hurwitz zeta; `LG_T` (ln Gamma on
  [2^-5, 16), 144 slots, bound 2^-75.22 relative), `LG_Z1`/`LG_Z2` (ln Gamma(z+t)/t at the zeros z = 1, 2).
* Runtime `fast_gamma.rs`: ln|Gamma| = L - ln|D|, Gamma = e^L/D for 2^-60 <= |x| < 16 (shift by -floor(x) for
  x <= -1/2, exact products). Wired in `special::lgamma/tgamma` for doubles; old bodies `*_accurate` (fallback).
* Near lgamma's negative zeros: REFUSAL to the 384-bit path (the bound stays absolute, from |L| and |ln D|, so
  `settle` refuses the cancelled cases); no double-double step. Measured rates in the report.

## Done in run 2
Tables regenerated and committed; first compile clean (clippy, static); unit tests green unchanged; new test at the
28 negative zeros in (-16, -2) (bisection on the oracle) in all four modes; kernel test covers them; fallback rate
measured per distance band; plants file handoff/plants/libm-special-gamma-tier-2.plants; gate, plants, counts.

## Open / next step
0. FIRST: merge origin/next (c6e5db9d or later). It moved after this run's merge: LIBM-TABLES-PER-FAMILY splits
   the generated tables into one file per family (`crates/math/src/coeffs/<family>.rs`, `Out::close(file, doc)`,
   `generate() -> Vec<(file, src)>`, `coeffs.rs` declares and re-exports them). Trial merge (aborted): conflicts in
   `crates/math/src/coeffs/tables.rs` (deleted upstream: take the deletion) and `crates/mathtools/src/tables.rs`
   (take next's version, then add `crate::gamma_tables::emit(&mut o.src); o.close("gamma.rs", "Taylor tables of the
   lgamma/tgamma fast tier");` after the erf close); `special_tables.rs` auto-merged (check the pub(crate) items and
   the target argument still compile). Declare `gamma` in `coeffs.rs` like `erf`, run `cargo xtask tables` on the
   role, commit the generated `coeffs/gamma.rs`, change the plants file's table-entry path to it, then rerun the
   fast_gamma tests, clippy, static, gate (escalates to FULL by rule: generated sources), plants (new dir plants-2).
1. Counts after vs the 1.05x target: see the report. If short: profile one call per phase (entry `mret::elem`, split,
   Taylor row, log_dd/exp_dd, settle) against glibc's total before any redesign (brief rule "profile before you design").
2. Out of scope so far: x >= 16 (Stirling), x <= -16 (still the 384-bit path), |x| < 2^-60, float rows.
3. A double-double step at the negative zeros would cut the near-zero refusals (up to ~44% within 2^-48 relative of
   a zero) but the uniform-range rate is 5 in 10^5; not worth it unless a workload sits on the zeros.

## Things not to redo
* Run 1's design and the generator assertions hold (tables built, all slot bounds <= 2^-75.22).
* The emitted doc line on the LG_* tables says "(module `fast_special`)" (shared `emit_table` text); cosmetic.
