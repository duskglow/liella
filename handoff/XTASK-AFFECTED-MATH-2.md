# XTASK-AFFECTED-MATH-2: the math fits and the tables generator map to the math set, not FULL

State: INCOMPLETE (jobs queued on mei, not collected). Branch `task/xtask-affected-math-2` (from origin/next c7c320a2),
code commit 50f0b63a, then this handoff commit. Ruling in force: #3204 (on top of #3188 / XTASK-AFFECTED-MATH).

## What changed (xtask/src/affected.rs, rule 8 of the module doc)
* `math_class`: `crates/math/fits/<name>.fit` (no subdirectory) -> `Class::MathFit`; exactly
  `crates/mathtools/src/tables.rs` -> `Class::MathTables`. Still FULL: the `.py` scripts under fits/, `fits/<sub>/`,
  a fit that is not in the tree (`Repo::fits`, read by `fit_files`: a removed fit, which `remez --check` cannot check),
  any other `tables.rs`.
* `Selection::math_coeffs` (factored from the MathCoeffs arm, same behaviour): the `coeffs` module as if changed, the
  math export set (`math_exports`) and `tables --check`.
* MathFit: `math_coeffs` plus `remez --check <fit>` (run from the root; `run_selection` splits on whitespace).
* MathTables: the file's ordinary mapping as a liella-mathtools source (`classify_path` + `own_suite`), `math_coeffs`
  (the generator writes every family) and `kernel-index --check` (also run inside `tables --check`; listed as the brief asks).
* Tests: new `math_fits_and_tables_generator_select_the_math_set_not_full` (direct and wide: not FULL, the commands,
  a superset of what coeffs/fast32_sin.rs selects, liella-mathtools unit tests, the generator's own mapping);
  `escalations`: the fits entry replaced by fast32_tables_gen.py, fits/sub/x.fit, a removed fit, mathtools/src/sub/tables.rs.

## Jobs (mei, role s3; all started detached, NOT COLLECTED at hand-back: queue load 51, 6 jobs ahead)
* `xam2-lint`: clippy -> ~/work/XTASK-AFFECTED-MATH-2/clippy.log, static -> static.log, then `cargo test -p xtask -- affected::`
  (its output in the jobrun log of xam2-lint).
* `xam2-replay`: `cargo xtask affected --replay records.txt --tier direct|wide` -> replay-direct.log / replay-wide.log.
* `xam2-gate`: `cargo xtask affected --base origin/next --run` -> xam2-gate.out.
* `xam2-plants`: `cargo xtask plants plants.txt` (10 plants, one per new mapping, msg: each) -> xam2-plants.out.
* NOT VERIFIED: compiles, clippy, static, unit tests, gate, plants, replay. Re-attach: `tail ~/work/XTASK-AFFECTED-MATH-2/xam2-*.out` on mei.

## Soundness replay (records: ~/work/XTASK-AFFECTED-MATH-2/records.txt on mei, from `git diff --name-only M^1 M`)
Last 10 libm merges on next. Fits / tables.rs touched by: libm-f32-erf-5 (7 fits), libm-f32-sinf-shrink-4 (2 fits),
libm-tables-per-family-2 (tables.rs). Red-gate failures: for the 7 stories also in XTASK-AFFECTED-MATH's replay, its
findings (no test failure outside the math set); for libm-hot-per-family-2, libm-f32-erf-5, libm-kernel-index no red
gate log with a FAILED line was found on mei/keke/shiki/kanon ~/work/<story>/ (FAILS empty). Per-story selections and
the miss count come from xam2-replay: NOT COLLECTED. Whether those three stories still go FULL for other files
(xtask/ main.rs, build.rs, ...) is in replay-direct.log.

## Next step
Collect xam2-lint, xam2-replay, xam2-gate, xam2-plants on mei. A red: fix, rerun only the failing job. p-tables-own may be
EQUIVALENT if the generator's own mapping adds nothing beyond the math set (then say so with the reason).
