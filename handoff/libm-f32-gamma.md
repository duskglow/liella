# LIBM-F32-GAMMA(-2): handoff (lgammaf, tgammaf double kernels)

State: branch `task/libm-f32-gamma-2` (continues `task/libm-f32-gamma`), merged with origin/next on 11-05;
code head `da21e569` (handoff commits on top). Role mei:~/w/s1. Verdict: INCOMPLETE (counts not run).

## Done
* LIBM-F32-GAMMA (3e302add): Remez targets `tgamma_2p5`, `lgamma1p_over_x`; fits `fits/fast32_tgamma.fit`
  (deg 14, 2^-50.17), `fits/fast32_lgamma.fit` (deg 11, 2^-49.77); `gamma_core` (≤ 55.1u), `ln64`,
  piecewise `lgamma_core` (refuses |y| < 2^-8 next to the negative zeros); `special::lgamma/tgamma` try the
  `float` fast path first (old bodies `lgamma_acc`/`tgamma_acc`). Proofs in the doc comments.
* -2 (6382a5f9): merged origin/next; kernels moved unchanged into `crates/math/src/fast32/gamma.rs` (one
  module per family); fast32.rs keeps only the `Fast` variants and eval arms. Sweeps use the shared tiered
  oracle (#2889): `oracle32::lgamma_est`/`tgamma_est` (double-double Stirling at w = x + n ≥ 16, 10 terms,
  remainder ≤ first omitted term; reflection for x < 0; integers → accurate path). New test
  `gamma_oracle_agrees_with_the_accurate_path` (decided answers = accurate path, 4 modes, on the sample).
  da21e569: `cargo xtask kernel-index` (KERNELS.md; static's tables-fresh check).
* The estimators assume `exp_log::log_dd` is within UE·(|ln v| + 1) (stated in `log_err`; only checked
  indirectly by the agreement test). Worth a direct bound test against mp if the oracle is reused.

## Jobs (mei)
* `libm-f32-gamma-2-check`: workspace clippy -D warnings + gamma tests: rc 0.
* `libm-f32-gamma-2-plants`: 6 plants (margin, tgamma coefficient, near-1 piece edge, near-zero edge,
  Stirling coefficient, oracle Stirling coefficient): 6 CAUGHT, 0 WARN.
* `libm-f32-gamma-2-gate`: superseded (static: KERNELS.md stale). `libm-f32-gamma-2-gate2`: static + direct tier
  on da21e569 (result in the report).
* `libm-f32-gamma-2-sweep-tgammaf` (head 6382a5f9; da21e569 differs only in KERNELS.md): PASS, 482 s test
  wall (584 s job incl. release build), 2626 s CPU. Fast path taken 1 683 198 064 of 4·2^32 mode-calls; oracle mp
  for 37 388 of them (2.2e-5).
* `libm-f32-gamma-2-sweep-lgammaf`: see the report (wall, CPU, mp rate, near-zero refusal count).

## Next step (in order)
1. Counts before/after: `cargo xtask bench --compare --counters cbench_libm`, rows tgammaf and lgammaf
   (base origin/next), as a jobrun job on the role, not concurrently with a gate. Target ≤ 1.05x.
2. If lgammaf stays above target: the near-zero refusals (|y| < 2^-8 around -2.457, -2.747, ...) cost a
   full mp call each; the lever is a double-double piece there.

## Decisions in force
Rulings #2760 (option A), #2784 (one story per family), #2783 (software sqrt; not used), #2889 (tiered oracle).
