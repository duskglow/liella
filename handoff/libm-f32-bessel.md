# LIBM-F32-BESSEL: handoff (j0f, j1f, y0f, y1f fast path) — after LIBM-F32-BESSEL-4, COMPLETE

State: branch `task/libm-f32-bessel-4` = `task/libm-f32-bessel-3` (fast-forward) + the lint fix + this record.
It stacks on LIBM-F32-ERF-5 (not on origin/next yet at 11-05 20:00; origin/next at d1a7f653 is already merged,
`git merge origin/next` was a no-op). Role kanon:~/w/kn1. Report: handoff/reports/LIBM-F32-BESSEL-4.json.

## Decisions in force
* D-10.49 (ruling #2881): no table of zeros; absolute bound -> per-call margin; refusals near zeros go to the accurate
  path; the row carries the measured sweep refusal rates.
* D-10.3 (Remez fits); ruling #2889 (shared tiered oracle): `oracle32/bessel.rs` (`bessel_est`), driver in
  fast32/tests.rs. One module per family: kernels and tests in crates/math/src/fast32/bessel.rs.

## Done in -4
* Cause of the -3 red (`libm-f32-bessel-3-gate-2` rc 101 on mei): clippy `approx_constant` on the hand-written
  `0.636_62` (2/π) in the series bound of oracle32/bessel.rs:125, the only error in the log. Fixed with
  `core::f64::consts::FRAC_2_PI` (commit "Use FRAC_2_PI ..."); the bound's 1.001 SLACK covers the f64 rounding.
* Diff since the sweep head 29d02b5c in crates/: that factor, `Range::contains` in `bessel_est` (NaN still
  Unknown), and two test-only lint rewrites (a NaN instead of a panic for a missing estimate; the zeros as f64
  literals cast to f32). The factor is 3.6e-7 relative smaller and still a valid bound, so a sweep can only decide
  the same operands or a few more, never a wrong one; the four sweeps of -3 stand.
* Counts (kanon, Broadwell; `cargo xtask bench --compare --counters --only cbench_libm`, origin/next tree vs head):
  j0f 121,749.6 -> 196.6 (glibc 278.4), j1f 121,833.7 -> 200.7 (283.0), y0f 472,877.0 -> 216.0 (294.9),
  y1f 472,875.7 -> 217.1 (296.9). No other row over +0.5% except mxcsr_wrapper_seq 10.2 -> 10.3 (+0.1/call, path
  not touched). erff/erfcf also drop (ERF-5 in the stack). Recorded in docs/saturation/23-f32-bessel-fast-path.md.
* Plants: `libm-f32-bessel-4-plants`, handoff/LIBM-F32-BESSEL-3-plants.txt, 5 CAUGHT, 0 WARN.

## Jobs (kanon) on the code head
`libm-f32-bessel-4-gate` (clippy, static, direct tier), `libm-f32-bessel-4-plants`,
`libm-f32-bessel-4-counts-before`, `libm-f32-bessel-4-counts-after`, and `libm-f32-bessel-4-static` (static on the
final head with the register section). All rc 0: the gate's `affected` escalated itself to FULL (generated
coeffs/tables from the ERF-5 stack in the diff) and `cargo xtask test` passed in 3138 s; counts 340 s each.
The code head is fbdc5de0; later commits are docs and handoff only (static green on them).

## Open
* Section number 23 in the saturation register may collide with another branch landing a section 23 (the check
  requires contiguous unique numbers): renumber at landing.
* Not timed (instructions below glibc's; the periodic timing sweep judges).

## Not to redo
Fits, kernels, bound derivations (-1/-2); the oracle estimator and the four sweeps (-3); D-10.49 text; counts (-4).
