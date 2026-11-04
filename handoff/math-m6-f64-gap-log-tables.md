# MATH-M6-F64-GAP-LOG-TABLES (register 13.5 #7): log2/log10 from tables pre-scaled by 1/ln b - DONE

State: branch task/math-m6-f64-gap-log-tables, code head 26c5012f (docs/report commit on top); stacked on
task/math-m6-f64-gap-round-test-2 (queued for CI, not landed); origin/next merged first (1ac537c6, clean).
Role keke:~/w/n1, synced with --base origin/task/math-m6-f64-gap-log-tables at 1ac537c6 (the merge), so the direct
gate selects only this change.

## Change
- Generator (crates/mathtools/src/tables.rs, `cargo xtask tables`): `LOG2_L`, `LOG10_L` (-log_b c_i = LOG_L/ln b,
  double-double, 91 entries, 1,456 B each) and `LOG10_2_E` (log10 2: 42 bits + RN rest, within 2^-97).
  NOTE: the brief says `cargo xtask math-gen --check`; the tables generator is `cargo xtask tables [--check]`
  (math-gen generates the exports).
- fast64.rs: `log_series` (the kernel before assembly), `logb_assemble::<TEN>` (e·log_b 2 + (-log_b c) exact by
  Fast2Sum; only lh scaled by one exact two_prod with 1/ln b; log2 needs no e product), `logb_kernel`, `logb_eps` =
  2^-49·|T|·scale + 2^-70·|y.hi|, scale = least double >= 1/ln b (LOG2_SCALE/LOG10_SCALE). Bound restated term by
  term at `logb_assemble` (T costs 10u·ih·|T|; the rest < 2^-94.5·|e| + 2^-101·S, under 2^-80·|y|). Near test:
  RHO_LOG kept for |y.hi| >= LOG2_NEAR/LOG10_NEAR = least double >= 0.34·scale. Both log2/log10 entries (directed
  `log2`/`log10` and the nearest in-place ones) use it.
- Tests: `fast64::bounds::log_tables_cover_their_bounds` (tables vs Mp within 2^-104, normalization, LOG10_2_E,
  each constant the least double meeting its bound); the log2/log10 kernels added to the oracle test and to the
  near-constant ratio test. D-10.13 row restated; register 13.5 row 7 marked DONE with the counts.

## Results (keke)
- Workspace clippy -D warnings rc 0, cargo xtask static rc 0, liella-math fast64::bounds 7/7 (code head tree).
- Plants math-m6-f64-gap-log-tables-plants rc 0: CAUGHT log2/log10-scale-one-ulp-down, log2/log10-near-one-ulp-down
  ("below its bound"), log2/log10-table-one-ulp ("beyond 2^-104"), log10-e-kl-dropped ("above the bound"); no WARN.
- Counts math-m6-f64-gap-log-tables-counts rc 0 (base = fast64.rs + tables.rs of 1ac537c6 in a RAM copy), instr/call
  base -> after (glibc), default and -AVX2 the same: log2 268.6 -> 241.8 (63.1), log10 275.9 -> 255.7 (102.9);
  exp 194.4, exp2 214.4, log 236.5, pow 522.0 -> 521.5 unchanged. No row slower.
- Gate math-m6-f64-gap-log-tables-gate (direct tier) and sweep math-m6-f64-gap-log-tables-sweep (SLOT_SIZE=M,
  SLOT_MAX_MIN=480, detached): see the hand-back for their state.

## Next step / not to redo
Register 13.5 #8 (FMA path) needs a ruling first; the remaining log2/log10 gap (3.8x / 2.5x glibc) is the shared
reduction and polynomial (13.3), the same as log's. Plan rows: D-10.13; register 13.5 row 7.
