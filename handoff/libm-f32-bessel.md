# LIBM-F32-BESSEL: handoff (j0f, j1f, y0f, y1f fast path) — after LIBM-F32-BESSEL-2, INCOMPLETE

State: branch `task/libm-f32-bessel-2` (from `task/libm-f32-bessel`; its stack base `task/libm-f32-double-kernels-2`
is in origin/next), role mei:~/w/s1. Commits of -2: `ddf02202` (lower fit degrees), the coefficients + kernels +
tests commit, `f569ac6c` (D-10.49 row), then this handoff. Full report: handoff/reports/LIBM-F32-BESSEL-2.json.

## Decisions in force
* D-10.49 (ruling #2881, accepted): no table of zeros; absolute bound e -> per-call margin e·2^53/|y|; near a zero
  the rounding test refuses and the accurate path answers. Row in plan/07-decisions/s10-mathematics.md.
* D-10.3: coefficients from the Remez tool (`crates/math/fits/fast32_{j0,j1,y0,y1}{,_small,_reg}.fit`).
* Ruling #2889: the exhaustive sweep must use the shared tiered oracle (double-double + proven bound + rounding test
  per mode, mp only when undecided), NOT the 2.3e5-instruction mp path per call.

## Done in -2 (verified)
* Fits lowered: small/reg even 0..16 (odd 1..17 for y1_reg), J pieces 0..13, Y pieces kept 0..16 (Y's first piece
  [2,4) converges slowly next to the log singularity at 0: Y0 c=3 66.27, Y1 c=3 398.55 units of 2^-53; all other
  pieces <= 2.8). Remez job `libm-f32-bessel-2-remez` on mei: 127 s wall, 309 s CPU, all 8 fits.
* Kernels compile; per-function piece tables (J [[u64;14];11], Y [[u64;17];11]); `BES_PIECE0_E` = [128,128,192,512]
  for the first piece; the small-x Y bounds gained J's error times |L| (was missing).
* Tests in `fast32::tests` (all green, job `libm-f32-bessel-2-unit-3`, 22 s): `bessel_cores_are_within_their_bounds`
  (mp sums via `special::bessel01_dd`, worst error/bound j0 0.20, j1 0.24, y0 0.34, y1 0.78),
  `bessel_bounds_hold` (Horner γ·Σ|a_i|r^i + measured fit error <= constants; HANKEL = the recurrence bit for bit;
  a_22/24^22, a_23/24^23 < 2^-58 — note: ~2^-59.0 for nu = 1, so the old "< 2^-59" claim was too tight),
  `bessel_fast_path_agrees_with_the_accurate_path` (sampled operands both signs, 4 modes; fallback on a 2^16 grid
  over (0,64]: j0f 0, j1f 1, y0f 2, y1f 1). The Bessel functions are NOT in `ALL` (their accurate path would make
  the all-encodings stride test take ~15 min).
* Gate / plants / check jobs: see the report json (labels libm-f32-bessel-2-{check,gate,plants}).

## Not done (next steps, in order)
1. The tiered oracle (ruling #2889). LIBM-F32-ERF-2 had not started (no branch) when -2 ran; build it as a shared
   test helper or reuse ERF-2's if it landed. For Bessel the double-double stage is the hard part: suggestion: reuse
   the mp `bessel01` only when the dd stage is undecided, and for the dd stage evaluate the same piecewise forms in
   double-double (series for x < 2... or Hankel with dd sin/cos) with a proven bound; size with LIELLA_SWEEP_STRIDE=64.
2. Four `#[ignore]` exhaustive tests (one per function, via `exhaustive(&[Fast::J0])` etc.) on that oracle; one
   jobrun job each on a 7-CPU slot, well inside 150 min; record wall, CPU, mp-fallback rate, and the fast-path
   fallback rate near zeros -> add it to the D-10.49 row.
3. Counts before/after for cbench_libm j0f/j1f/y0f/y1f (`cargo xtask bench --compare --counters`, rows in
   bench/c/cbench_libm.c lines 312-327). "Before" = origin/next (accurate path only).

## Not to redo
Remez tool extension and the 8 fits (generated, D-10.3); the bound derivations in the code comments; D-10.49 text.
