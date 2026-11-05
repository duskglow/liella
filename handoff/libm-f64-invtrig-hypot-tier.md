# LIBM-F64-INVTRIG-HYPOT-TIER (run -4): run -3's red fixed (test reference), FULL gate running; kernel redesign not started (INCOMPLETE)

State: branch `task/libm-f64-invtrig-hypot-tier-4` (carries runs -1..-3), code head `80667ac8`, cut from origin/next
2c1e25b9 (next has moved on; no merge done); role shiki:~/w/sh1. Results stay correctly rounded (ruling #2760 A);
no differences row changes. Counts and per-phase split of the current kernels: docs/saturation/21-f64-invtrig-hypot-fast-tier.md.

## Done in run -4
* **Cause of gate3's red (acos(5e-324): entry 0x20, reference 0x30).** The product was right, the test reference was
  wrong. The reference was `mr::ret(trig::acos(..))`: it raises the status on top of whatever the raw double-double
  arithmetic raised in hardware, and acos's steps underflow (tiny inexact intermediate products) for a subnormal operand
  while the result, about pi/2, is not tiny. `elem` and `elem_fast` (the product entries) settle MXCSR and drop such
  hardware flags. The accurate path's `Status` was right all along (inexact only).
* **Three-way check.** Spec: §10.0 (s10_0_f_0) and C F.10: underflow is for a tiny result; pi/2 is not tiny. Black-box probe
  (shiki, glibc 2.39, `~/work/libm-f64-invtrig-hypot-tier-4/flags.c`; acos/asin/atan/atan2/hypot of ±5e-324, 1e-323,
  DBL_MIN, the largest subnormal, 2^-1000, 1e-300, 1e-200, 1e-160, 1e-150, 2^-27, all four directions): glibc acos raises
  **no** underflow (and no inexact: the D-10.14 row 342 difference, unchanged); Liella's acos raises 0x20 in all 44 cases,
  never underflow. asin/atan/atan2/hypot raise underflow only for tiny results on both sides. The other differences in the
  probe (directed-rounding bits, ERANGE for an underflow to zero) are the accurate path's existing correctly rounded
  behaviour under §10.0, not this story's.
* **A second defect in the same reference:** the raw `trig::*(x, mr::mode())` ran the double-double arithmetic under the
  caller's directed *hardware* rounding (unsupported: the envelope runs it in the default MXCSR). With each pair's operands
  swapped it gave other bits (atan2(-2.94e-264, 1.19e-240), upward).
* **Test** `math_double_invtrig_hypot_fast_tier_is_the_accurate_path` (9a07412a, 80667ac8): every entry against the
  envelope `mr::elem` (the pre-story entry), plus `annex_f`: underflow exactly for a tiny inexact result (DBL_MIN allowed:
  after-rounding tininess), overflow/invalid/no divide-by-zero, EDOM/ERANGE exactly as §10.0 gives them, decided from the
  result and operands alone. Added subnormal/tiny operands, both operand orders, and acos(±5e-324) in every direction.
  Inexact still comes from the envelope comparison (exactness is not decidable from the result alone).
* Plants (`cargo xtask plants`, 2/2 CAUGHT): `fallback-leaks-hw` (elem_fast_cold raises the status over hardware
  flags: run -3's class) and `exit-leaks-hw` (the in-place envelope does). No product code changed in this run.

## Jobs (shiki, ~/logs/<label>.log; files in ~/work/libm-f64-invtrig-hypot-tier-4/)
gate (FULL tier: `affected --base origin/next` escalates on the generated s10_math_gen.rs of run -1; running at
hand-back), plants2 (rc 0, 2/2). lint2 (clippy -D warnings, static, the test: rc 0). Superseded: plants (refused by
the tool: a `lit:` pattern held ` | `), lint (clippy: `panic!` in the test), t1/t2 (test iterations), so (build for the
probe).

## Exact next step
1. Read gate (jobwait on shiki). Red: fix only what it names, rerun that suite.
2. Kernel redesign (the coordinator's item 2, not started). Estimated savings per phase, from §21's split and the code
   (instructions per call; to be checked by wl_trace after each step, timing sweep gives the verdict):
   * **atan (kernel 210, glibc total 78.7).** Reduction: one `double` division for u = (t-c)/(1+t·c) with its error
     carried into the rounding test's bound instead of the double-double quotient + 5-op remainder: -30..-40.
     A j/256 table (|u| <= 1/512) so a degree-2 odd polynomial in `double` suffices instead of degree 4: -10..-15.
     |x| > 1: fold 1/x into the same single division (u = (1 - t·c)/(t + c) against atan(1/c)'s table entry) instead of a
     reciprocal + exact residual + dd subtraction from pi/2: -25..-35. Index clamp via an integer exponent test: -5.
     Expected ~100-120; the floor above glibc is the rounding test + settle (~15-20) that correct rounding needs.
   * **asin/acos (288/252 vs 75/80).** Drop asin = atan(a/sqrt(1-a²)): |x| <= 1/2 an odd polynomial in x² with a j/64
     table on x (asin) and acos = pi/2 - asin as one dd add; |x| > 1/2 asin = pi/2 - 2·asin(sqrt((1-|x|)/2)) with one
     `sqrtsd` and an FMA residual. Removes the dd sqrt, one or two dd quotients and the atan reduction: -150..-180.
   * **atan2 (224 vs 161.7):** shares the atan reduction with y/x as the single division: -60..-80 (near 1.05x).
   * **hypot (120 vs 63.8):** x²+y² as hi+lo by two FMAs, one `sqrtsd`, the correction (s - h²) by FMA decides
     rounding; no dd sqrt: -40..-55.
   Build atan first (largest shared win), prove the bound (plants near midpoints as in run -3), count, then the rest.
   Each new kernel needs the Dekker (no-FMA) path too, bit-identical (owner rule: hardware options, same result).

## Do not redo
* `_mm_sqrt_sd` in liella-math (E0133); out-of-line vs inline sqrt counts (§21); run -3's plants 1-6; the flags probe.

## Plan rows to read
D-10.13, D-10.14, D-10.22, D-10.25; original brief /home/claude/state/briefs/LIBM-F64-INVTRIG-HYPOT-TIER.md.
