# LIBM-F64-EXPM1-HYPER-CBRT-TIER handoff (run 3, LIBM-F64-EXPM1-HYPER-CBRT-TIER-3)

Branch `task/libm-f64-expm1-hyper-cbrt-tier-3` (stacked on run 2's `task/libm-f64-expm1-hyper-cbrt-tier-2` d14fdcbe).
Code head e83bf52a (then this handoff/report commit). Role shiki:~/w/sh1 (Haswell-EP E5-2697 v3). Decisions in force:
D-10.13 (the accurate path is the reference), D-10.25 (FMA build through `dispatch::fma`), ruling #2760 A. Scope: expm1,
log1p, sinh, cosh, tanh, cbrt; asinh/acosh/atanh are a later story.

## State
* Product code is run 2's, unchanged (per-family layout `fast64/exp_log.rs`, `fast64/hyperbolic.rs`; `hot.rs`; `s10_math.tsv`
  `d` rows through `mret::elem_fast`). Proofs in each entry's doc comment.
* Run 3 added (9d462798, e83bf52a): `tier_declines_or_rounds_right_on_hard_cases` in `fast64/tier_tests.rs` with 18 hard
  cases (expm1 4, log1p 4, sinh 1, cosh 3, e^a/2 above 40 3, tanh 3); one bound plant per kernel path in
  `handoff/libm-f64-expm1-hyper-cbrt-tier-plants.txt`; saturation section 23
  (`docs/saturation/23-f64-expm1-log1p-hyperbolic-cbrt-fast-tier.md`) with the counts and the residual.
* How the hard cases were found: a scratch copy with every kernel-path bound set to `0.0` (one literal per site), a
  release example that compares the tier with `Real::*` (Nearest) over 24 threads of xorshift-uniform operands per range
  (10^8 per range, then 1.6·10^9 for the empty ones). Misround rate about 1 in 10^8 (expm1 [0.0157, 1]: 34 in 1.6e9;
  log1p 3 and 7; cosh 11; e^a/2 7; tanh 4; sinh [0.0157, 40] 1 in 1e8; cbrt 0 in 1e8). The scratch tools
  (`zero_bounds.py`, `hard_search.rs`, `search.sh`) are on shiki in `~/work/libm-f64-expm1-hyper-cbrt-tier-3/`, not in
  the repo.

## Results (run 3)
* Lint: `-lint-2` (workspace clippy all targets/features -D warnings, `cargo xtask static`, tier tests): rc 0.
* Plants `-plants-1` (`cargo xtask plants`, /mnt/zram dir): 10 plants, 10 CAUGHT (bound, log1p-bound, sinh-bound,
  cosh-bound, exp-half-bound, tanh-bound, folded-minus-one, one-plus-x-split, cbrt-seed, cbrt-exact-cube), 0 WARN.
* Gate `-gate` (`cargo xtask affected --run --base origin/next`): ESCALATES TO FULL because the branch changes
  `s10_math.tsv`/`s10_math_gen.rs` (generator input); run 2's gate-4 escalated the same way and never finished. Left
  running detached at hand-back (see the report for its status).
* Counts: product code unchanged since run 2's counts (keke `libm-f64-expm1-hyper-cbrt-tier-2-counts-after-5`, rc 0,
  Zen 1): expm1 156.4/86.4, log1p 204.5/83.5, cbrt 141.5/148.4, sinh 274.6/144.8, cosh 260.5/84.5, tanh 294.7/131.6
  (Liella/glibc instructions per call); before 928-1105 (cbrt 636). Target 1.05x reached for cbrt only.

## Open / next
1. Read the FULL gate result (`~/bin/jobwait libm-f64-expm1-hyper-cbrt-tier-3-gate` on shiki); fix what it finds.
2. Residual (expm1 1.8x, log1p 2.5x, sinh 1.9x, cosh 3.1x, tanh 2.2x): profile one call per row first
   (validation/wl_trace.sh), then the candidates in saturation section 23 (one table lookup for e^a and e^-a in
   `exp_pm`; `near` instead of `settle` with an explicit eps; log1p's division).
3. Not covered by a hard case: the series paths (|x| < 2^-6 or 2^-7) and cbrt's bound (no case in 10^8 operands; its
   real error is far smaller). The FMA build's hard cases are covered only by the bit-identity test in
   `boundary/tests/math_fma_paths.rs` (random operands), not by the hard-case list.
4. Directed rounding modes still run the accurate path only. asinh/acosh/atanh not started (run 1's design notes).

## Not to redo
The hard-case search (the list is in the test); the run-1 design notes (two kernel calls in `exp_pm`; cbrt's degree-3
second refinement) still hold. Accuracy contract unchanged: every answer is the accurate path's result bit for bit, so
docs/differences-from-glibc.md does not change.
