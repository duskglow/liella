# MATH-M6-F64-GAP-LOG-TABLES-2: port of the f64 gap chain and log2/log10 pre-scaled tables

State: branch task/math-m6-f64-gap-log-tables-2, code head cce65fca (handoff commit on top).

## What was done
- The original branch never reached the public tree, and neither did the chain it stacked on
  (BREAKDOWN, ASSEMBLY(-2), EXP-REDUCE(-2), ROUND-TEST(-2)); the public crates/math and crates/mathtools were still
  byte-identical to that chain's base. So the final state of crates/math and crates/mathtools was taken as a whole
  (no per-commit cherry-pick; the 26 commits were mostly merges and other stories).
- D-10.13 (plan/07-decisions/s10-mathematics.md) taken from the end of the chain; saturation register section 13
  (breakdown) and the DONE rows of 13.5 #1/#6/#7 applied on top of next's register, both sides of the one conflict kept.
  Lab host names were removed from the register text; the older per-story handoffs are carried in handoff/ unchanged.
- No conflict where next "does the same thing another way": next had none of this work.

## Results (keke, final tree identical to code head)
- Plants (math-m6-f64-gap-log-tables-2-plants, RAM copy, fresh dir): six one-line mutants (log2/log10 scale one ulp
  down, log2/log10 near-bound one ulp down, LOG10_2_E low part dropped, one LOG2_L entry one ulp) all CAUGHT by
  fast64::bounds; no WARN.
- Gate math-m6-f64-gap-log-tables-2-gate: `affected` escalated to FULL on the host (the synced role has no git base),
  so it runs clippy --workspace and the whole test suite; read its rc in jobs.tsv.
- Counts from the original run are in register 13.5 #7 (log2 268.6 -> 241.8, log10 275.9 -> 255.7 instr/call); not rerun.

## Open / not redone
- Register 13.5 #8 (FMA path) needs a ruling. Remaining log2/log10 gap is the shared reduction/polynomial (13.3).
- `cargo xtask tables --check` was not run separately here (the tables are generated; the plants and bounds test cover them).
