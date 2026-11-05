# LIBM-F32-GAMMA: handoff (lgammaf, tgammaf double kernels)

State: branch `task/libm-f32-gamma` (stacks on LIBM-F32-DOUBLE-KERNELS-2), code head `3e302add`; this note and
the report are committed on top (handoff/ only). Role shiki:~/w/sh2. Verdict: INCOMPLETE (sweeps and counts not run).

## Done (3e302add)
* Remez tool (`crates/mathtools/src/remez.rs`): targets `tgamma_2p5` = Γ(5/2 + x) and `lgamma1p_over_x` =
  lnΓ(1 + x)/x (limit -γ at 0 by a symmetric difference at 3p bits).
* Fits (D-10.3): `fits/fast32_tgamma.fit` degree 14 on [-1/2, 1/2], 2^-50.17 after rounding (7.1u);
  `fits/fast32_lgamma.fit` degree 11 on [-1/8, 1/8], 2^-49.77 (9.4u). Degrees 15/12 gave 2^-52.3/2^-53.6 (unneeded).
* `fast32.rs`: `gamma_core` (Γ(t), t = x - n in [2, 3], times/divided by the exact factors; domain 2^-20 ≤ |x|,
  -42 < x < 36; ≤ 55.1u; DELTA 96), `ln64` (ln of a `double`, ≤ (1.45 + 2|y|)u absolute), `lgamma_core` piecewise
  (near 1: z·q(z) ≤ 12u; near 2: log1p_core + z·q(z) ≤ 41.6u; x ≥ 36 Stirling ≤ 29.1u; else ln|G|: ≤ 30.5 ulp for
  |y| ≥ 1, ≤ 7260 ulp (DELTA 11264) for 2^-8 ≤ |y| < 1, refused below: the negative-zero neighbourhoods).
  Proofs in the doc comments. `Fast::Tgamma`, `Fast::Lgamma`, `eval_tgamma`, `eval_lgamma` (signgam: x < 0 with an
  even integer part gives -1).
* `special.rs`: `lgamma`/`tgamma` try the `float` fast path first (`F::PREC == 24`); old bodies are
  `lgamma_acc`/`tgamma_acc` (the tests' reference).
* Tests (`fast32::tests`): `gamma_margins`, `gamma_cores_are_within_their_bounds` (vs the CR `double` path),
  `gamma_fast_paths_agree_on_a_sample` (value, flags, signgam; 4 modes), ignored
  `tgammaf_fast_path_agrees_exhaustively`, `lgammaf_fast_path_agrees_exhaustively`. Local run on the role: 3 passed.
* Workspace clippy (`--all-targets --all-features -D warnings`) and `cargo xtask static`: clean on 3e302add.

## Jobs (shiki)
* `libm-f32-gamma-gate2`: direct-tier gate, escalated to FULL by the tool (generated coefficient files), started
  detached. `libm-f32-gamma-gate` superseded (it was attached to an ssh session and stopped with kill-mine).
* `libm-f32-gamma-plants`: `cargo xtask plants ~/work/LIBM-F32-GAMMA/plants.txt --dir
  /dev/shm/claude/libm-f32-gamma/plants-1`: margin, a tgamma coefficient, the near-1 piece boundary, the
  near-zero edge (2^-8 -> 2^-16), a Stirling coefficient.

## Not done (next step, in order)
1. Collect `gate2` and `plants` (`~/bin/jobwait libm-f32-gamma-gate2 libm-f32-gamma-plants` on shiki); fix what they find.
2. Sweeps, one job each, in RAM (not concurrently with a build in sh2): size with `LIELLA_SWEEP_STRIDE=64` first.
   Estimate: each answers on about 1.5·10^9 encodings x 4 modes against the 384-bit path (tens of thousands of
   instructions per call), likely more than the 150-minute slot. Splitting the harness by range may be needed.
3. Counts before/after: `cargo xtask bench --compare --counters cbench_libm` rows tgammaf, lgammaf (base = origin/next).
   Expected: the fallback rate is small on [-10, 10] (refused: |y| < 2^-8 next to the negative zeros, about 1% of the row
   there, each costing a full mp call); if that holds the row above 1.05x, the next lever is a double-double piece
   near the zeros -2.457 and -2.747.

## Decisions in force
Rulings #2760 (option A), #2784 (one story per family), #2783 (software sqrt; not used here).
