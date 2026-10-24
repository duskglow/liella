# MATH-M6-F64-GAP-ROUND-TEST(-2) (register 13.5 #5): plants green; counts, sweep, FULL gate running detached

State: branch task/math-m6-f64-gap-round-test-2, head = the commit carrying this file (code head 9a89612c), stacked on
task/math-m6-f64-gap-exp-reduce-2 (queued for CI, not landed); origin/next merged (clean, 3bc1db7e). Role keke:~/w/n1,
synced with --base origin/task/math-m6-f64-gap-exp-reduce-2 (the gate escalated to FULL because of the next merge).

## Change (crates/math/src/fast64.rs, run 1; unchanged in run 2)
- `near(y, c, m)`: w = y.lo*c; answer y.hi*2^m when y.hi + w == y.hi and w != 0. RHO_EXP 1.125*2^-70, RHO_POW
  1.375*2^-70 (dz <= DZ_NEAR = 2^-72), RHO_LOG 2^-69 (|ln x| >= 0.34); C_* = least double with C(1 - 2^54 rho) > 1 + 2^-53.
- A miss goes to the EXACT `settle` test (coordinator #2071), never the accurate path: a miss costs time, not correctness.
## Run 2
- New bounds test `fast64::bounds::near_miss_goes_to_the_exact_test` (a constructed miss must settle to 1 with st 1).
- `math_double_fast_entry_sweep` now counts `nearest_stage` per function and asserts the miss rate (miss / (near+miss))
  <= 2^-12 = 244 ppm per function, and that near answers at all. Cap: coordinator #2071; a miss costs one settle (~30 instr),
  so the cap bounds the cost below 0.01 instr/call. Expected about C - 1: 17 ppm exp, 21 ppm pow, 31 ppm logs.
## Results (keke n1, head 9a89612c)
- Workspace clippy -D warnings rc 0; cargo xtask static rc 0; liella-math fast64::bounds 6/6.
- Plants math-m6-f64-gap-round-test-2-plants rc 0: CAUGHT c-exp/c-pow/c-log-one-ulp-down ("below its bound"),
  CAUGHT miss-to-accurate ("near miss not settled"); no WARN lines.
- Sweep math-m6-f64-gap-round-test-2-sweep (SLOT_SIZE=M, SLOT_MAX_MIN=480): chunk 135/200, 0 mismatches at hand-back;
  the stage lines (miss ppm per function) print at the end of ~/logs/math-m6-f64-gap-round-test-2-sweep.log.
- Counts math-m6-f64-gap-round-test-2-counts (base = exp-reduce-2's fast64.rs in a copy of this head; after; default and
  -AVX2): QUEUED for a slot at hand-back (35 min), not read.
- Gate math-m6-f64-gap-round-test-2-gate: FULL (escalated by the origin/next merge), running.
SUPERSEDED: math-m6-f64-gap-round-test-gate (run 1, older head, rc 0).
## Not done / next step
1. Read the counts log: instructions:u base/after/glibc for exp exp2 log log2 log10 pow, default and -AVX2; any slower row = a
   trade-off to report. 2. Write register 13.5 row 5 "DONE (...)" with the saving and the sweep's miss ppm; add the near-test
   restatement to D-10.13 (plan/07-decisions/s10-mathematics.md). Both need step 1's numbers, so they are not written yet.
Plan rows: D-10.13; register 13.5 row 5.
