# PERF-LIBM-TRIG (checkpoint hand-back)

State: branch task/perf-libm-trig, code head 50b70a90 (in-place round-to-nearest entries for sin, cos, tan, double and float).

## Done
- `fast32::{sin,cos,tan}_nearest` (eval_nearest rows for Sin/Cos/Tan) and `trig::{sin,cos,tan}_nearest` (f64): the same fast kernels, run in place under the default MXCSR word
  through `elem32_fast` / `elem_fast` (the exp/log pattern) instead of the full `mret::elem` envelope. Refuses every operand the accurate entry handles first
  (zero, NaN, inf, |x| < 2^-30 (f32: 2^-26), |x| >= 2^20 (f32: 2^19), subnormals), so where it answers the value and flags are the accurate path's. Inexact is raised in hardware
  (f64: the returned `y.hi + y.lo` is an inexact addition for the refused-if-zero `y.lo`).
- s10_math.tsv rows split (f / d / lq), s10_math_gen.rs regenerated with `cargo xtask math-gen`.
- Tests: trig_nearest_entries_match_the_accurate_path, trig_nearest_rounding_test_edges (trig.rs); trig_nearest_entries_agree_with_the_accurate_entry and the ignored
  exhaustive trig_nearest_entries_agree_exhaustively (fast32.rs); math_trig_in_place_entry_matches_the_envelope (boundary s10_math_symbols.rs).

## NOT done (checkpoint: context ceiling reached, not budget of calls)
- Hypothesis (2) f64 kernel (cheaper reduction/eval/settle, FMA dispatch) and (3) directed-mode fast path (sin_upward, sinf_upward) and sin_huge: untouched.
- The 1.05x target is NOT shown. The counts of the in-place step are in the job logs: keke ~/work/perf-libm-trig/counts-before.log and counts-after.log.
- No docs/saturation row yet for rows above 1.05x (needs the counts and the per-phase floor).

## Jobs (keke, role n3; scripts in ~/work/perf-libm-trig/)
perf-libm-trig-counts0 (before = RAM copy of origin/next, after = role), perf-libm-trig-plants, NOT started: the gate (script ~/work/perf-libm-trig/gate.sh: clippy, static, affected; run it with jobrun label perf-libm-trig-gate after counts-after.log shows rc=, since the counts build in the role). The new unit tests passed on keke before the last two commits (4 passed); the boundary test math_trig_in_place_entry_matches_the_envelope has NOT been run yet.

## Next step
Read counts-after.log; if sin/cos f64 still > 1.05x, do the f64 kernel work (reduction for |x| < pi/4 without double-double, Estrin polynomial, FMA dispatch with bit-identical
results) and the directed-mode fast path; then add docs/saturation/20 for rows that stay above 1.05x.
