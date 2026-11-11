# LIBM-DIRECTED-INPLACE: the double exp quick phase in the caller's rounding direction (CHECKPOINT)

State: branch task/libm-directed-inplace (cut from origin/next bb9130ed9), code head 856fcb002, all commits "WIP:".
Role shiki:~/w/sh2. Reserved decision id D-10.94 (record NOT written yet). Stopped at the context checkpoint
(~150k), 55 of 110 calls.

## Done (code)
- `liella_math::fast64::quick::exp_directed(x, h)` (both builds, inside the `quick!` macro; new macro arg `$cd`):
  exp's reduction with `t = ∘(∘(x·INV + h) + SHIFT)`, `h = −1/2` when the caller rounds up, `+1/2` down / toward
  zero, so `k` is round-to-nearest within 2^-34 in any direction and `r1` stays exact; the shared kernel; then a
  two-sided monotone test in the caller's direction: `e = zh·rho`, `lo = zh + (zl − e)`, accept iff
  `lo == zh + (zl + e)`, return `lo·2^m`. Sound in every direction with no midpoint argument.
- Proof: module doc of crates/math/src/fast64/quick.rs, section "Directed rounding" (each rounding one ulp; Fast2Sum
  error terms per Boldo, Graillat & Muller, ACM TOMS 44(1) 2017). Stated `RHO_DIR_SSE2 = 1.5·2^-61`,
  `RHO_DIR_FMA = 1.25·2^-67`. New test `directed_bounds` (quick/tests.rs) recomputes the terms: FMA 2^-66.766,
  SSE2 2^-60.504, both inside the stated values with the test's own roundings.
- `crate::hot::exp_directed(x, mode)` (FMA dispatch as exp_nearest); `mret::elem_fast_dir` = elem_fast whose
  non-default branch calls cold, out-of-line `elem_directed`: under `fpu::is_default_but_rounding(saved)` (existing
  predicate: traps masked, no FTZ/DAZ) the directed kernel runs in place on the one MXCSR read, else and on a decline
  `elem_switched` as before. The nearest path is unchanged instruction for instruction (same body as elem_fast).
- s10_math.tsv `exp d` row -> elem_fast_dir; s10_math_gen.rs regenerated with `cargo xtask math-gen` (exp, expf32x).
- expf: ALREADY runs in the caller's direction (`elem32_onepass`, D-10.63 / D-10.72; the 18x figure predates it).
  Not touched; its counts/timing still to be read in this story's measurement step.

## Results (shiki)
- libm-directed-inplace-gen2: math-gen + `cargo xtask clippy` + `cargo xtask static` rc 0 (on 856fcb002's tree minus
  the regenerated file, which it produced).
- libm-directed-inplace-tests: rc 0: directed_bounds; s10_math_symbols math_double_fast_entry* (the directed modes
  vs the accurate path, flags and errno, FTZ/DAZ and unmasked-trap words now exercise the new path); exp_quick.
- NOT DONE: counts (before/after/glibc), booked timing (Ivy Bridge, N100), 10^7 accuracy job, plants, gate,
  hot_text_products, decision record D-10.94.

## Next step (continuation, ~45 calls)
1. Tests in crates/boundary/tests/exp_quick.rs (both builds via `with_fma`, MXCSR set per call like
   s10_math_symbols' `around`): (a) hard family `x = ±2^-k`, k 31..=52 (SSE2) / 35..=52 (FMA), every directed mode:
   `selftest::exp_directed` must decline (e^x is within x²/2 of the double 1 ± 2^-k), and `m::exp` must equal
   `Real::exp(x, mode)`; (b) random sample vs `Real::exp(x, mode)`; (c) ignored accuracy job `EXP_DIR_N` (10^7)
   per directed mode and nearest through `m::exp`. Needs `selftest::exp_directed(x, mode)` -> `crate::hot::exp_directed`.
2. Update the comment of s10_math_symbols' `math_double_fast_entry_stays_out_of_place_in_a_non_default_mode`
   (exp now runs in place under a directed word without FTZ/DAZ/traps).
3. Plants file (handoff/LIBM-DIRECTED-INPLACE-plants.txt): nearest-test (`lo == zh + (zl + e)` -> nearest test
   `zh + zl * $c == zh`, return `zh * sc`) and fallback-skipped (`if lo == ... else { None }` -> `Some(lo * sc)`),
   both caught by test 1(a) decline asserts; mathret fallback skipped (`elem_switched(x, saved, f)` in
   elem_directed -> `elem_fast_cold::<0x3f, A, R>(x, saved, f)`), caught by 1(a)'s `m::exp` check.
4. `cargo xtask records new decision D-10.94` (method: Tang 1989 reduction/table, directed-rounding analysis above,
   Boldo-Graillat-Muller 2017), gate `affected --base origin/next`, hot_text_products, counts both builds
   (exp_upward/downward/towardzero, expf_*), booked timing nico/kotori + N100. Target exp/expf upward <= 2x glibc.
4. Name LIBM-DIRECTED-INPLACE-2 (log/logf, sin/sinf, pow): same recipe; the reduction's directed rounding of
   integer-producing SHIFT steps is the thing to check per function.
