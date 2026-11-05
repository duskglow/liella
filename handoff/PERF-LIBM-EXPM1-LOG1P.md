# PERF-LIBM-EXPM1-LOG1P: state (CHECKPOINT, not finished)

Branch task/perf-libm-expm1-log1p. Role shiki:~/w/sh2.

## Done
* `crates/math/src/fast64.rs`: in-place entries `expm1_nearest`, `log1p_nearest`, `exp10_nearest`,
  `cbrt_nearest` (double-double kernel + bound + constant rounding test `near` + exact test on a miss,
  accurate path when both decline), modelled on `exp_nearest`/`log_nearest`. Non-FMA build only.
  * expm1: |x| <= 0.34 table `EXP2M1_J64` + series (`e + p + e*p`); else `exp_kernel` and `v - 1` by 2Sum.
  * log1p: |x| <= 2^-7 series with r = x; else `1 + x = wh + wl`, `log_series(wh)` + `wl/wh`. Declines above 1e300
    (wl/wh would be subnormal: log1p(DBL_MAX) raised underflow in hardware, found by elem_products).
  * exp10: the existing fast64::exp10 arithmetic in place (exact cases 1..=22 declined).
  * cbrt: 3 Halley steps from a bit-trick guess, then a dd Newton correction from the exact residual; declines
    results of <= 18 significant bits (exact cubes) and |x| outside [2^-800, 2^800].
* `s10_math.tsv` rows split (f/l/q keep the envelope; d uses the new entries); `s10_math_gen.rs` regenerated.
* Unit tests `fast64::inplace::*` (600k+ operands per function, each fast answer bit-equal to the accurate path)
  pass; `cbrt_correction_stays_within_its_bound` checks the 2^-70 relative bound with double-double cubing.
* clippy + static green on the role; elem_products green after the log1p fix.

## Not done (remaining, ~25-30 calls)
* Plants (rounding test, range filter, dispatch) NOT run: no plants job exists. NOT READY on that count.
* The accuracy job (before/after identical, four rounding modes, specials) beyond the unit tests: the gate's
  `elem_products`/`special_cr` suites ran in the gate only.
* f32 rows (expm1f, log1pf, cbrtf, exp10f): untouched. `fast32::eval_nearest` refuses Log1p/Expm1/Cbrt ("flags on
  subnormal operands unproven"); wiring them in place is the next step for those rows.
* FMA build of the new kernels (`fused` module + `hot.rs` dispatch): not done; the expm1/log1p kernels would
  move into the `kernels!` macro. Needed to approach the 1.05x target (floor note: docs/saturation/16).
* exp10 still uses the dd `reduce` + `Dd::mul` (about 60 instructions); a Cody-Waite split of log10(2)/64 would cut it.
* docs/saturation row for rows that stay above 1.05x: write after the counts are read.

## Jobs on the final head (a7321398 + the log1p fix)
`perf-libm-expm1-log1p-gate-2`, `perf-libm-expm1-log1p-counts` (chain script ~/work/PERF-LIBM-EXPM1-LOG1P/chain.sh on shiki,
counts output ~/work/PERF-LIBM-EXPM1-LOG1P/counts.txt). Superseded: `perf-libm-expm1-log1p-gate` (red: log1p(DBL_MAX) underflow flag, fixed).

## Things not to redo
The tsv/gen flow: edit s10_math.tsv, run `cargo xtask math-gen` on the host, rsync s10_math_gen.rs back.
No cargo on the agent machine: build and test on the role.
