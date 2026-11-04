# MATH-F64-FMA-PATH handoff (run 4 = MATH-F64-FMA-PATH-4)

Branch task/math-f64-fma-path-4 (= run 3's branch + merge of origin/next 432f1ef7). Role keke:~/w/r5.
Code head 4f8d85fe (merge commit; the handoff/report commit follows it). Decisions in force: **D-10.23**
(plan/07-decisions/s10-mathematics.md; was D-10.22 on run 3's branch, renumbered because next's
MATH-M6-SQRT row took D-10.22), ruling #2261 (UNSAFE(hot-kernel) widened).

## Run 4
- CI conflict resolved: MATH-M6-F64-GAP-LOG-TABLES-3 on next rewrote fast64's kernels (Fast2Sum assembly,
  `reduce_ln2`/`reduce_exp`/`reduce_pow`, `log_series` + `log_assemble`, the `near`/`finish` rounding test,
  `exp_kernel` returning the tail). Next's arithmetic kept unchanged; `kernels!` now wraps only the functions with
  exact products: `exp_kernel` (rh², th·ph), `log_kernel`, `log_series` (m·c, rh²). Macro form
  `kernels!(<attrs for all>; <inline attr for log_series>; <prod>)` because `#[inline(always)]` cannot sit on a
  `#[target_feature]` fn. `mul`/`mul_f64` macro copies dropped (unused now); `reduce` (exp10 only) is a plain fn.
  `fused::{exp,log}_nearest` mirror `exp_near`/`log_near` (`finish` with `C_EXP` / `log_c`).
- Jobs on keke, chain `~/work/math-f64-fma-path-4/verify.sh` (out: verify.out): `math-f64-fma-path-4-build1`
  (math-gen must leave s10_math_gen.rs unchanged, xtask clippy, xtask static, math_fma_paths, liella-math lib),
  `math-f64-fma-path-4-gate1` (direct, --base origin/next), `math-f64-fma-path-4-plants1` (RAM copy
  /dev/shm/claude/math-f64-fma-path-4/plants-1; fmadd-sign, fused-exp-scale (new: m+1 in fused exp), fused-log-scale
  (re-anchored on `finish`), select-ignores-arg; exits 1 on a survivor). Results were not in at hand-back.

## Open
- Code comments still cite **D-10.22** for the FMA build (dd.rs:68, fast64.rs x3, config.rs:49, cpu.rs:220,
  dispatch.rs:33, hot.rs:2502, selftest.rs:689/700, tests/math_fma_paths.rs:1): change them to D-10.23 (out of
  budget this run; pure comments). Row order in the decisions file is D-10.21, D-10.23, D-10.22: swap.

## Next step (exact)
1. Read the three jobs above in keke ~/logs/jobs.tsv; on a red, fix the cause and rerun only that job.
2. D-10.22 -> D-10.23 in the comments listed above.
3. Counts (original brief step 0, not started): `cargo xtask bench --compare --counters cbench_libm` for exp/log,
   default and with `--tunables glibc.cpu.hwcaps=-FMA`; update saturation register 13.5 #8; keep only if no row slower.
4. Rest of the family (exp2, exp10, log2, log10, pow) via the same macro + selectors in hot.rs + tsv + math-gen.

## Not to redo
The macro scope fix, the decision row, the category widening, run 3's lint/static fixes, the merge resolution.
