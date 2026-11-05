# LIBM-F64-INVTRIG-HYPOT-TIER (run -5): atan/atan2/asin/acos kernel trimmed 2-8% by instructions; ERANGE row extended (COMPLETE, target not reached)

State: branch `task/libm-f64-invtrig-hypot-tier-5` (carries runs -1..-4), merged with origin/next (3176bd47), code
head 3c1200b4, role shiki:~/w/sh2. Results stay correctly rounded (ruling #2760 A). Counts and per-phase split:
docs/saturation/22-f64-invtrig-hypot-fast-tier.md (renumbered from 21: next took section 21).

## Done in run -5
* **Run -4's FULL gate:** green (32 suites ok, 10839 passed, 0 failed; conformance 117/0).
* **Merge origin/next:** conflicts in s10_math.tsv (kept this story's d-form asin/acos/atan/atan2 lines beside next's
  f-form fast hyperbolic lines), math lib.rs (both modules), saturation index (ours -> 22). `cargo xtask math-gen`
  regenerated s10_math_gen.rs: unchanged against the auto-merge.
* **#3017 differences row:** the double/float table had no underflow-to-zero errno row (row 357 is the long double
  table). Probe `~/work/libm-f64-invtrig-hypot-tier-5/erange.c` on shiki (glibc 2.39 vs Liella, ±2^-1074 / ±2^-149
  and twice that, all four directions): Liella sets ERANGE with underflow for sin, atan, atan2(x,1), tanh, asinh,
  log1p, expm1 and their float forms when the result goes to ±0; glibc mostly returns the smallest subnormal there
  (row 341's rounding difference), tanh/tanhf toward zero give −0 without ERANGE; atan2(x, 2) to nearest: ERANGE on
  both. New row after row 344 (D-10.14, factual).
* **Kernel (item 3), profiled first:** per-phase split of the FMA atan kernel from Liella's listing (in §22). Cut:
  `π/2 − atan(j/64)`, `π − atan(j/64)`, `π/2 + atan(j/64)` as const tables built from ATAN_J64 (`shifted`), so
  atan |x|>1, asin near 1 and the atan2 quadrants end in one table add with a sign (no `comb`); the table index read
  from the low bits of `64 th + 1.5·2^52`. |x| <= 1 bit-identical; bound argument in the module comment.
  Test: the table test also checks operands with 1/x near each node against mp::atan (both builds).
* Counts (Broadwell host, cbench_libm, instructions per call, before = merge commit): atan 236.8 -> 217.3,
  asin 325.1 -> 314.7, acos 292.9 -> 286.1, atan2 265.8 -> 248.8, hypot 153.4 = 153.4; no row slower. Target 1.05x
  not reached (atan 2.76x, atan2 1.54x).

## Jobs (shiki, ~/logs/<label>.log; files in ~/work/libm-f64-invtrig-hypot-tier-5/)
libm-f64-invtrig-hypot-tier-5-gate (escalated to FULL: 32 suites ok, 10851 passed, 0 failed; conformance 117/0), -plants (6/6 CAUGHT, no WARN), -counts (before/after),
-check (clippy -D warnings, static, invtrig tests: rc 0). Superseded: -prep (Liella link line lacked libm.a; the
math-gen and so parts were ok), -probe (evidence, rc 0).

## Exact next step
1. `settle` + sign is now the largest phase of atan (37 instructions): a cheaper rounding test (compare |y.lo|
   against a precomputed half-ulp minus EPS from the exponent bits, one integer path) is the next local cut.
2. Then the design in §22: one `double` quotient with its error in the rounding bound plus a finer table (j/256),
   asin/acos on their own reduction near 1 instead of atan of a quotient; hypot by FMA residual without a dd sqrt.
   Each needs the Dekker build too, bit-identical, and midpoint plants as in run -3.

## Do not redo
* `_mm_sqrt_sd` in liella-math (E0133); out-of-line vs inline sqrt counts (§22); run -3's plants 1-6; the flags
  probe of run -4; the ERANGE probe of run -5.

## Plan rows to read
D-10.13, D-10.14, D-10.22, D-10.25; original brief /home/claude/state/briefs/LIBM-F64-INVTRIG-HYPOT-TIER.md.
