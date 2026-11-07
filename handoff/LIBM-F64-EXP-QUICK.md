# LIBM-F64-EXP-QUICK (-2): binary64 exp quick phase (kernel, proof, tests; not wired)

State: branch `task/libm-f64-exp-quick-2`, code head 67b7caa22 (handoff/report commits on top). COMPLETE once the
gate and plants jobs below are green. Brief: `~/state/briefs/LIBM-F64-EXP-QUICK.md` (same as
`handoff/libm-rethink/LIBM-F64-EXP-QUICK.md`). Decisions in force: D-10.51 (two-phase design), D-10.54 (this kernel's
constants, bounds and measured accuracy), D-10.25/D-10.50 (two builds via `kernels!`/`fused`), D-10.3 (CR contract).

## What is in

* `crates/math/src/fast64/quick.rs`: `exp_quick` / `exp_quick_parts`, SSE2 build and `quick::fused` (FMA); the
  step-by-step bound in the module doc (SSE2 2^-61.503, stated 2^-61.476; FMA 2^-67.727, stated 2^-67.678).
  `exp_quick_parts`, `RHO_SSE2`, `RHO_FMA` are `pub` + `#[doc(hidden)]` for the boundary's tests only.
* Boundary access (unsafe stays in liella-boundary): `hot::exp_quick` / `exp_quick_parts` (`#[cfg(feature =
  "selftest")]`, dispatch on `dispatch::fma()`), `selftest::exp_quick` / `exp_quick_parts`.
* Tests (clause s10_0_f_0, §10.2 exp, D-10.3): SSE2 in `fast64/quick/tests.rs` (constants and bounds recomputed,
  error vs `Mp`, 10^7 operands, fallback 0.4-0.8%, hard cases, domain); FMA in `crates/boundary/tests/exp_quick.rs`
  (10^7 operands CR, FMA fallback < 0.05%, FMA/SSE2 bit identity on 2·10^5, FMA error vs `Mp` under `RHO_FMA`), plus
  two `#[ignore]` harnesses: `exp_quick_accuracy_job` (`EXP_QUICK_N`, default 10^8 per distribution and build) and
  `exp_quick_count_loop` (counts). The new test file is selected by the direct tier (it ran in the gate).
* Plants `handoff/LIBM-F64-EXP-QUICK-2-plants.txt`: C4 +1 ulp, C_SSE2 for rho = 2^-64, Estrin term on rh, table index
  +1, rl dropped, KBIAS 1022: 6 CAUGHT, 0 SURVIVED, 0 WARN (shiki `libm-f64-exp-quick-2-plants`, same quick.rs as the
  final head; rerun on the final head as `-plants3`, queued behind the gate; `-plants2` was refused by the copy check because it copied the role while the gate wrote conformance/build/: never start plants in a role while a gate runs there).

## Results (shiki, Haswell-EP, glibc 2.39)

* Accuracy job (`libm-f64-exp-quick-2-accuracy2`): 6·10^8 operands, **0 mismatches**. Fallback: uniform [-10, 10]
  SSE2 0.562%, FMA 0.0079%; uniform bits over [2^-60, 708) SSE2 0.526%, FMA 0.0072%; uniform [-745.2, 709.8] SSE2
  3.22%, FMA 2.68% (2.68% is the share outside the quick domain |x| >= 708). Max rel error: SSE2 2^-61.583 (bound
  2^-61.476); FMA printed by `exp_quick_fma_error_within_bound` (`--nocapture`), asserted under 2^-67.678.
* Counts (`libm-f64-exp-quick-2-counts2`), instructions per call incl. fallback, 4096 operands uniform [-10, 10]:
  **Liella quick FMA 72.0, SSE2 86.3** (targets 80 / 95: met) against glibc exp 61 (FMA) / 71
  (`GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA`). Liella's figure includes an out-of-line call to the `selftest` wrapper
  (cross-crate, not inlined, `Option<f64>` return) and the dispatch load; the wired entry will not have that wrapper,
  so WIRE must re-count. Not a timing claim: no timing is possible before wiring.
* Superseded: `-counts` (the loop compared its mode string every iteration, skewing the subtraction: FMA read 45),
  `-gate` (killed: head changed), `-unit`, `-accuracy`, `-lint0` (static red: stale KERNELS.md, fixed), `-lint1`, `-kidx`.

## Count recipe for an unwired kernel (proposed for rules/developer-flow)

1. An `#[ignore]` test with a loop: operands precomputed in a Vec, `black_box` per operand, the mode (kernel / none)
   decided ONCE before the loop, N from an env var; fallback to the real entry so the rate is included.
2. `cargo test --no-run --message-format=json` gives the binary; `perf stat -x, -o f -e instructions:u <bin> --ignored
   --exact <test> --test-threads=1` at N = 0 and N = 10^7, for the kernel and for `none`, min of 3 runs each.
3. Per call = ((I(N) − I(0)) − (Inone(N) − Inone(0))) / N. Reference: a C loop over the same operands at -O2
   -fno-builtin with the same subtraction, with and without `GLIBC_TUNABLES=glibc.cpu.hwcaps=-FMA`.
   Script: `~/work/LIBM-F64-EXP-QUICK-2/count.sh` + `glibc_exp_loop.c` on shiki (copies in the report).

## Next step

LIBM-F64-EXP-QUICK-WIRE: call `exp_quick` in front of `exp_nearest` in the C entry (both builds), re-count with
`cargo xtask bench --compare --counters` (cbench_libm exp), and book Ivy Bridge and the N100 for timing.

## Not to redo

The bound proof and tables (D-10.54); the 10^8 accuracy run; the plants. `KERNELS.md` holds line numbers of
quick.rs: any edit to quick.rs needs `cargo xtask kernel-index` or static's tables-fresh goes red.
