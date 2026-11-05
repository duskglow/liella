# XTASK-AFFECTED-MATH: libm wiring and coefficient files map to the math suites, not FULL

State: branch `task/xtask-affected-math` (from origin/next 5ac80194), commits e5e7407c (mapping), a test commit, this handoff commit.
Ruling in force: #3188 (developer gate may be targeted for libm wiring; the landing gate in CI stays FULL).

## What changed (xtask/src/affected.rs, rule 8 of the module doc)
* `math_class`: exactly `crates/boundary/src/posix/s10_math.tsv`, `.../s10_math_gen.rs`, `csys/src/adapter/math_*.c`
  -> `Class::MathWiring`; `crates/math/src/coeffs/<family>.rs` (one component) -> `Class::MathCoeffs`.
  Checked before the csys/ and generated-source escalations. Still FULL: `crates/math/fits/`, every other `*_gen.rs`,
  `tables.rs` outside coeffs/ (e.g. `crates/mathtools/src/tables.rs`), subdirectories of coeffs/ or adapter/, any non-.c
  adapter file, other csys C files (`libm_init.c`), build.rs.
* `Selection::math_exports`: `s10_math_gen` as if changed (narrow_direct / narrow by tier), every `liella-math` target,
  every target whose own source names `s10_math` or `libm` or carries section 10, the unit tests and clippy of
  liella-boundary and liella-math, conformance programs of section 10 or calling an entry of s10_math_gen / s10_math.
* Wiring adds `cargo xtask math-gen --check`; coefficients add the `coeffs` module as if changed plus
  `cargo xtask tables --check`. `run_selection` now splits an xtask command on whitespace (flags).
* Tests: `math_wiring_and_coefficients_select_the_math_suites_not_full` (direct and wide tiers, 5 files),
  `escalations` (s10_math entries removed; fits, coeffs/sub, adapter .h, other adapter .c, other _gen.rs added).

## Results (keke, role n2)
* clippy rc 0, static rc 0, xtask affected:: tests 46 passed (job xam-lint2).
* Direct gate (`cargo xtask affected --run`, job xam-gate): `affected: ok`.
* Plants (`cargo xtask plants`): xam-plants 6 plants, 4 CAUGHT, 2 SURVIVED (the loops were also reached through the
  s10_math_gen narrowing for the 7 named targets); test strengthened (every liella-math target and every target naming
  s10_math/libm asserted), xam-plants2 re-ran both survivors plus wiring-drop-exports: 3/3 CAUGHT, 0 WARN.
* After the test-only second commit: clippy clean (inside xam-plants2), gate xam-gate2 `affected: ok`; static was run
  on the first commit only (rc 0) - the second commit changes only a #[cfg(test)] body.

## Soundness on history (replay, job xam-lint2; records in the story's scratch, `--replay` over both tiers: 0 misses)
Last 10 libm merges on next; red gates found on keke/mei/shiki/kanon logs; CI reds were all process reds (conflicts,
register precheck, incomplete hand-back), no test failures.
| story | new direct selection | test failures in its red gates |
|---|---|---|
| libm-f32-sinf-shrink-4 | FULL (math fits) | none |
| libm-f64-explogpow-tier | 70t/17c (was FULL) | none |
| libm-tables-per-family-2 | FULL (mathtools tables.rs, xtask main.rs) | none |
| libm-f64-invtrig-hypot-tier-5 | 231t/117c (was FULL) | none |
| libm-f32-hyperbolic-4 | 70t/17c (was FULL) | arch-audit static only (runs in every selection) |
| libm-f64-fast-kernels-3 | FULL (boundary build.rs, adapter/libm_init.c) | none |
| libm-f32-hyperbolic-3 | 41t/14c (was FULL) | none |
| libm-special-fast-tier-2 | FULL (mathtools tables.rs) | boundary::s10_math_symbols, math::s10_math_clauses, math::special_products: all inside the math set (asserted by the unit test) |
| libm-f32-hyperbolic-2 | 35t/0c | none |
| libm-wide-dd-tier-2 | 230t/117c | none |
No failure falls outside the new selection.

## Open / informational
* The xtask command split (`math-gen --check`) is not reached by the unit tests; it runs only in `--run` of a math
  selection. NOT VERIFIED by a run of a tsv change (no libm change on this branch).
* Next candidates (not done, need a ruling): `crates/mathtools/src/tables.rs` (the generator) -> math set + `tables
  --check`; `crates/math/fits/*.fit` -> `remez --check` + math set. Both kept FULL per the brief.

## Next step
Coordinator: CI. Not to redo: the history replay (records reproducible from `git diff --name-only <merge>^1 <merge>`).
