# MATH-M6-F64-GAP-ROUND-TEST (register 13.5 #5): code done, CHECKPOINT (context ceiling), NOT VERIFIED

State: branch task/math-m6-f64-gap-round-test, code head fe86cff1, stacked on task/math-m6-f64-gap-exp-reduce-2 (not landed;
merge of origin/next NOT done this run). Role keke:~/w/n3, synced with --base origin/task/math-m6-f64-gap-exp-reduce-2.

## Change (crates/math/src/fast64.rs)
- `near(y, c, m)`: w = y.lo*c; answer y.hi*2^m when y.hi + w == y.hi and w != 0 (w != 0 keeps the hardware inexact).
  Proof in its doc comment (H = distance to the midpoint on w's side, |y.hi| <= 2^54 H, c(1 - 2^54 rho) > 1 + u).
- Stated bounds: RHO_EXP 1.125*2^-70 (exp, exp2), RHO_POW 1.375*2^-70 (pow, only when dz <= DZ_NEAR = 2^-72 and
  -1021 <= m <= 1022), RHO_LOG 2^-69 (log/log2/log10, only when |ln x| >= 0.34; near 1 the ratio reaches 2^-64).
  C_EXP/C_POW/C_LOG = the least double with C(1 - 2^54 rho) > 1 + 2^-53 (test near_constants_cover_their_bounds
  checks exactly in Mp that C covers and C - 1ulp does not, and eps/|y.hi| <= rho on 20k operands).
- `finish`: a miss or an ineligible case falls to the exact `settle` test, so the accurate-path fallback rate is
  unchanged; expected miss rate about C - 1 (1.7e-5 exp, 2.1e-5 pow, 3.1e-5 logs). CAP proposed: miss rate <= 2^-12
  per function (a miss costs one settle, about 30 instructions: < 0.01 instr/call).
- Kernels return the series tail; eps (exp_eps/log_eps/pow_eps) is formed only on the settle path. pow split into
  pow_kernel. `#[doc(hidden)] pub fn nearest_stage(which, x, y)` reports which step answered, for the sweep.
- Accurate entries (exp(x, mode) ...) unchanged in behaviour.

## Results (keke n3)
- Workspace clippy -D warnings rc 0; cargo xtask static rc 0 (one commit before the last test-only edit);
  liella-math bounds tests 5/5 pass.
- Gate: math-m6-f64-gap-round-test-gate (Direct tier vs exp-reduce-2) STARTED detached, not read.

## Not done / next step (in order)
1. Read the gate; rerun `cargo xtask static` at the head.
2. Sweep: add the stage counts to math_double_fast_entry_sweep (crates/boundary/tests/s10_math_symbols.rs: count
   liella_math::fast64::nearest_stage codes 0/1/2/3 per function, print rates, assert code-1 rate <= 2^-12), start it
   detached (SLOT_SIZE=M).
3. Plants (RAM copy, cargo xtask so first): C_EXP/C_POW/C_LOG one ulp down; DZ_NEAR doubled; LOG_NEAR 0.25; drop
   `w != 0.0` (expect the envelope inexact test or EQUIVALENT); all with msg: near_constants_cover_their_bounds.
4. Counts: cargo xtask bench --compare --counters cbench_libm, default and --tunables glibc.cpu.hwcaps=-AVX2, base
   = exp-reduce-2 head (reuse ~/work/math-m6-f64-gap-exp-reduce-2/counts.sh pattern); rows exp exp2 log log2 log10 pow.
5. Register 13.5 row #5 with the saving; D-10.13 restatement (plan/07-decisions/s10-mathematics.md).
Plan rows: D-10.13; register 13.5.
