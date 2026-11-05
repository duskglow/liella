# LIBM-F32-ERF: handoff (erff, erfcf on the fast32 PATTERN)

State: branch `task/libm-f32-erf-4` (stacks on `-3`, `-2`, `-1`; merged with origin/next 4c33d4c8), role keke:~/w/n2.
Code head: fafbde3c "Merge origin/next again (LIBM-F32-HYPERBOLIC-3)"; the handoff commit follows it.
Verdict of -4: COMPLETE once the jobs below are rc 0 (they were still running at hand-back).

## What -4 did (coordinator note 11-05: merge next, one module per family, one sweep oracle)
* Merged origin/next twice (hyperbolic family, then LIBM-F32-HYPERBOLIC-3 landed mid-run). Both sides kept: the
  `Fast` enum, `eval`, the `eval_nearest` refusal list (erff/erfcf still refused there: no in-place entry is
  proven for them), the `fast_fn!` wrappers, the test tables (ALL/NAMES = 23; erff/erfcf last, CHEAP = 21 so they
  stay out of `agree`'s wide strides), `accurate`, and the bench-range match (21 = erff, 22 = erfcf).
* Layout: the erf kernels (bounds, deltas, erfcx pieces, erf_small, erfc_big, erf_core, erfc_core) and all erf
  tests (oracle sweeps, oracle agreement, stride and bound tests) moved to `crates/math/src/fast32/erf.rs`.
  fast32.rs keeps the shared core and dispatch with `mod erf; use erf::{erf_core, erfc_core, ERFC_DELTA,
  ERF_DELTA};`. Shared test helpers `accurate`, `agree_in`, `exhaustive_by`, `MODES` are `pub(super)` in
  fast32's test module so family modules reuse them. Next family: put its kernels in `fast32/<family>.rs` the same way.
* One sweep oracle: next carries no second float sweep helper. Hyperbolic sweeps use the shared
  `exhaustive`/`exhaustive_by` against their (cheap) accurate path; erf sweeps use `exhaustive_by` with the
  tiered oracle (`crates/math/src/oracle32.rs`, ruling #2889). `wide_dd.rs`'s `sweep` is a random tier check
  of the wide (long double) route, not a float sweep: left alone.
* No kernel code changed in either family. Workspace clippy -D warnings and `cargo xtask static` clean on keke.

## Jobs on the final head (keke)
`libm-f32-erf-4-gate-2` (affected --base origin/next; escalates to FULL: generated coeff sources in the diff),
`libm-f32-erf-4-plants-2` (`~/work/libm-f32-erf-4/plants.txt`, 11 plants: -3's ten with the kernel plants'
path now fast32/erf.rs, plus dispatch-erfc on fast32.rs's eval; dir /dev/shm/claude/libm-f32-erf-4/plants-2),
`libm-f32-erf-4-sweeps-2` (both exhaustive oracle sweeps, 16 CPUs, RAM copy; expect mp needed for 4 erff modes,
0 erfcf, as in -3), `libm-f32-erf-4-counts` (`cargo xtask bench --compare --counters cbench_libm`, RAM copy;
compare erff 180.2 / erfcf 191.6 from -3 and the hyperbolic rows in handoff/libm-f32-hyperbolic.md).
Superseded (older merge head, killed by me when next moved): `libm-f32-erf-4-gate` (143), `-plants` (143),
`-sweeps` (124). Results were not read before hand-back: the coordinator collects them.

## Next step
Read the four jobs; if counts match -3 and the hyperbolic table, nothing changed. Otherwise none for the story.
Optional: lower the fraction's starting depth in oracle32.rs if a sweep must run faster.

## Decisions in force
Rulings #2760, #2784, #2783, #2889. D-10.3: coefficients from the Remez tool.

## Not to redo
The trial degree search (degree 15 holds). The fallback design (subnormal erfc results on the accurate path).
The oracle and its bound test. The module split (done here).
