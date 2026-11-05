# LIBM-F32-BESSEL: handoff (j0f, j1f, y0f, y1f fast path) — after LIBM-F32-BESSEL-3, INCOMPLETE (checkpoint)

State: branch `task/libm-f32-bessel-3` (LIBM-F32-ERF-5 base + merge of `task/libm-f32-bessel-2` + merge of
origin/next at 29d02b5c), head = the "Record the LIBM-F32-BESSEL-3 handoff" commit on top of `8d0b83c7`. Role mei:~/w/s1.
Report: handoff/reports/LIBM-F32-BESSEL-3.json.

## Decisions in force
* D-10.49 (ruling #2881): no table of zeros; absolute bound -> per-call margin; refusals near zeros go to the accurate
  path. The row now carries the measured sweep refusal rates (below).
* D-10.3 (Remez fits, unchanged); ruling #2889 (shared tiered oracle): Bessel adds `oracle32/bessel.rs` (`bessel_est`),
  no second oracle. The sweep driver (`agree_oracle`, `exhaustive_oracle`) moved from erf.rs into fast32/tests.rs,
  taking the estimator as a parameter; erf calls it with `erf_est`/`erfc_est`.
* One module per family: the Bessel kernels and their tests are in crates/math/src/fast32/bessel.rs.

## Done in -3 (verified unless marked)
* Merges: bessel-2 onto ERF-5 (conflicts: Fast enum/eval arms, eval_nearest list, remez `shift`+`centers`, D-10.49
  before D-10.50, coeffs modules); origin/next (coeffs modules, eval_nearest list: next dropped Sin/Cos/Tan).
* Estimator: series below 24 (Y: log_dd + γ + harmonic weights), Hankel 24..2^19 (dd reduction by π/4 in 3 parts,
  Taylor sin/cos), absolute bounds derived in the doc comments. Tests (green, job libm-f32-bessel-3-size-2):
  `bessel_oracle_is_within_its_bounds` (vs 384-bit values), `bessel_oracle_log_bound`, `bessel_oracle_agrees_with_the_accurate_path`.
* Sizing at LIELLA_SWEEP_STRIDE=64: j0f 4.6 s wall / 27 s CPU, j1f 5.0/29, y0f 2.6/15, y1f 2.9/17.
* Exhaustive sweeps (all rc 0, every encoding x 4 modes, mp tier needed for 0 modes):
  j0f 395 s wall / 2056 s CPU, taken 2617236912; j1f 426/2425, 2617235768; y0f 169/1002, 1308613912;
  y1f 554/1224, 1308615620. Refused in range [2^-20, 2^19): j0f 8784 of 2617245696 (3.4e-6), j1f 9928 (3.8e-6),
  y0f 8936 of 1308622848 (6.8e-6), y1f 7228 (5.5e-6). These ran on 29d02b5c; later commits change only test lints
  and docs, not kernels or estimator logic (the Range::contains rewrite keeps NaN -> Unknown).

## NOT VERIFIED at hand-back (jobs started detached on mei at the final code head 8d0b83c7)
* `libm-f32-bessel-3-gate-2`: clippy -D warnings, then xtask static, then the direct-tier gate (one job).
* `libm-f32-bessel-3-plants`: 5 plants (handoff/LIBM-F32-BESSEL-3-plants.txt).
* `libm-f32-bessel-3-counts-before-2` (origin/next export in ~/work/libm-f32-bessel-3/before) and
  `libm-f32-bessel-3-counts-after` (HEAD export in .../after): `cargo xtask bench --compare --counters --only cbench_libm`.
  Rows of interest: j0f, j1f, y0f, y1f (bench/c/cbench_libm.c 312-327).
Re-attach: `ssh claude@10.0.100.235 '~/bin/jobwait libm-f32-bessel-3-gate-2 libm-f32-bessel-3-plants libm-f32-bessel-3-counts-before-2 libm-f32-bessel-3-counts-after'`.

## Next step
Read those four jobs. Green gate + all plants CAUGHT -> record the before/after/glibc counts for the 4 rows (handoff,
saturation register libm section) and hand back COMPLETE. A red gate: fix, rerun only the failing suite.

## Not to redo
Fits, kernels, bound derivations (-1/-2); the oracle estimator and the four sweeps (-3); D-10.49 text.
