# LIBM-F32-ERF: handoff (erff, erfcf on the fast32 PATTERN)

State: branch `task/libm-f32-erf-3` (stacks on `task/libm-f32-erf-2`), role mei:~/w/s3. Code head: the commit
"Add a tiered test oracle and run the erf sweeps against it" (de666b73); the handoff commit follows it.
Verdict of -3: COMPLETE once its gate is green (see the report json for the job labels).

## What -3 did
* Read -2's jobs on keke: `libm-f32-erf-2-gate` rc 0 (10826 passed, 0 failed), `libm-f32-erf-2-plants` rc 0 (4
  CAUGHT, 1 WARN-NO-MSG: erfcx1-coeff is caught by the erf_core bound line, the regex named erfc_core; fixed in
  -3's plants file).
* Ruling #2889: `crates/math/src/oracle32.rs` (test-only, shared by every float family). `Est` = a double-double
  `v` with a proven absolute bound `err`, or "just below the float F" (erf beside ±1, erfc beside 2), or Unknown.
  `decide(est, mode)` rounds when no boundary of that mode lies within err (floats for directed, midpoints for
  nearest; only normal inexact results; exact cases never decided); `oracle(est, tally, accurate)` answers all
  four modes from one estimate and calls the mp path only for an undecided mode (counted in `Tally`).
  erf estimators: series (positive terms, tail bound) for |x| < 4; beside ±1 from 4 (erfc < e^-x²/(x√π));
  erfc = 1 + erf for x < 0 (beside 2 from -4), 1 - erf below 3, Laplace's continued fraction from 3 to 10
  (consecutive convergents bracket the value: their gap bounds the truncation). Bounds use U = 2^-100 per dd
  operation and 2^-98 for exp_dd: these per-operation figures are slack over dd's stated accuracy, NOT proven
  here; the end-to-end bound is checked against the 384-bit mp erf at 20k points
  (`erf_estimates_are_within_their_bounds`, which also rejects a bound looser than 2^-70).
* Sweeps now use the oracle (`agree_oracle`, `exhaustive_oracle`); `exhaustive_by` takes the chunk check.
* Tests added: `oracle_decides_only_clear_cells`, `erf_estimates_are_within_their_bounds`,
  `erf_oracle_agrees_with_the_accurate_path`. Unit run `libm-f32-erf-3-unit` (mei): 6 passed.

## Measured (mei, Skylake-SP class, load 35-47 during the runs)
* Sizing, LIELLA_SWEEP_STRIDE=64, both functions: 26 s wall, 157 s CPU; mp needed for 0 modes.
* Full sweep erfcf (`libm-f32-erf-3-sweep-erfcf`, 12 CPUs, test binary run from a RAM copy): rc 0, 643 s
  wall, 6432 s CPU, taken 7,405,707,468 of 2^34 calls, mp needed for 0 of 7,405,707,468 modes.
* Full sweep erff (`libm-f32-erf-3-sweep-erff`, 12 CPUs): see the report json / hand-back (was running).
* Counts (`libm-f32-erf-3-counts`, `cargo xtask bench --compare --counters cbench_libm`), instructions per
  call: erff 180.2 (before 228,016, the mp path; glibc 135.3), erfcf 191.6 (before not measured: the same mp
  path; glibc 138.7).

## Plants (`~/work/libm-f32-erf-3/plants2.txt` on mei; cmd `cargo test -p liella-math --release --lib -- erf oracle`)
Product: erfc-delta, erfcx1-coeff, piece-lo, piece-mid, erf-sat. Oracle: oracle-mid-side, oracle-sign-mode,
oracle-series-stop, oracle-beside-two, oracle-fraction-term. Run 1 (`libm-f32-erf-3-plants`, SUPERSEDED) had
oracle-fraction-depth (the fraction's doubling threshold), which SURVIVED: the starting depth 16 + 4000/a²
already brings the gap below 2^-96 on every tested point, so the threshold is a guard that only trades cost; it
was replaced by a plant on the fraction's term (run 2, `libm-f32-erf-3-plants-2`).

## Next step
None for the story. Optional: lower the fraction's starting depth if a later family needs the sweep faster
(erfcf's 643 s is mostly the fraction from 3 to 9.19); add estimators for the next families in oracle32.rs.

## Decisions in force
Rulings #2760, #2784, #2783, #2889. D-10.3: coefficients from the Remez tool.

## Not to redo
The trial degree search (degree 15 holds). The fallback design (subnormal erfc results on the accurate path).
The oracle and its bound test.
