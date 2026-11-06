# LIBM-F32-GAMMA(-2, -3): handoff (lgammaf, tgammaf double kernels)

State: branch `task/libm-f32-gamma-3` (continues `-2`, `-1`; based on origin/next as merged 11-05). Code head
`233ba694`; handoff commits on top. Role kanon:~/w/kn1. Verdict: COMPLETE (lgammaf count target not reached; see next).

## Done
* -1 (3e302add): Remez fits `fits/fast32_tgamma.fit` (deg 14, 2^-50.17), `fits/fast32_lgamma.fit` (deg 11,
  2^-49.77); `gamma_core` (≤ 55.1u), `ln64`, piecewise `lgamma_core` (refuses |y| < 2^-8 next to the negative
  zeros); `special::lgamma/tgamma` try the `float` fast path first. Proofs in the doc comments.
* -2 (6382a5f9, da21e569): kernels in `crates/math/src/fast32/gamma.rs`; oracle32 `lgamma_est`/`tgamma_est`
  (ruling #2889's tiered oracle); KERNELS.md regenerated.
* -3: **why the exhaustive lgammaf sweep was killed at 9000 s** (mei, rc 124). `gamma_sweep_profile` (new,
  ignored diagnostic: the oracle check per sign/exponent slice, sampled or in full, with taken calls, mp modes,
  near-zero refusals and wall time per slice) named the range: every slice from 2^23 up to overflow had 100 % of
  its modes on mp (and the integers from 2^13 up a growing share), ~80 µs per operand, ~0.9·10^9 operands. Cause:
  `lgamma_est` used tgamma's refusal `gamma_unknown` (every integer, since Γ(n) is exact, and |x| ≥ 2^23), while
  lgammaf's fast path takes all of x ≥ 36. The near-zero refusals were not the cost (they skip the oracle).
  Fix (233ba694): `lgamma_unknown` refuses only ±0, ±∞, NaN, x < −64, the poles and lnΓ(1) = lnΓ(2) = 0; lnΓ(n)
  for n ≥ 3 is transcendental (never a float or midpoint) and Stirling's bound holds for all x > 0, so the
  accuracy proof is unchanged. ln p is skipped when p = 1 exactly (n = 0). New test
  `lgamma_oracle_decides_integers_and_large_operands`.
  Profile (stride 4099, all slices): 16.53 s summed before, 1.10 s after; the only slice left with mp is 248
  (x ≈ 2.7e36–5.3e36: results ≥ 3e38, which `oracle32::decide` refuses near FLT_MAX), ~3 % of the sweep's CPU.

## Jobs (kanon unless named)
* `libm-f32-gamma-3-profile-before` / `-profile-after`: the slice tables above (diagnostics).
* `libm-f32-gamma-3-check`: gamma and oracle32 unit tests on 233ba694: 7 passed.
* `libm-f32-gamma-3-sweep-lgammaf` (233ba694): PASS, 554.7 s test wall, 3883 s CPU at load 8–25 on 35 threads
  (tgammaf on mei: 482 s, 2626 s CPU). Fast path taken 5 598 331 376 of 4·2^32 mode-calls; oracle mp for
  6 035 592 (1.08e-3); near-zero refusals 45 474 operands. The tgammaf sweep was not rerun (tgamma_est unchanged).
* `libm-f32-gamma-3-plants`: 5 CAUGHT, 1 SURVIVED (`lp-shortcut-value` at 1e-9 ≈ 1/2000 ulp: below the sample
  tests' resolution, only the exhaustive sweep sees it). `libm-f32-gamma-3-plants2`: the same file with that plant
  at 1e-6 (≈ 0.5 ulp): 6 CAUGHT, 0 SURVIVED, 0 WARN.
* `libm-f32-gamma-3-counts-before` (origin/next e39b9799 copy in RAM) / `-counts-after` (233ba694),
  `cargo xtask bench --compare --counters --runs 1 cbench_libm`, instructions per call (glibc 2.39 in brackets):
  tgammaf 461 689.5 → 230.2 (476.5): 0.48x; lgammaf 429 599.0 → 2 240.0 (277.2): 8.08x.
* `libm-f32-gamma-3-gate` (2b1392b1; later commits are handoff-only): workspace clippy -D warnings and `cargo xtask static`
  passed; `affected --run` escalated to FULL (the .fit files count as generated sources) and was still running at hand-back.

## Next step
1. lgammaf is at 8.08x of glibc by instructions (target ≤ 1.05x). Probable surplus (NOT VERIFIED; profile first,
   validation/wl_percall.sh): the bench range [-10, 10] includes the near-zero refusals (|y| < 2^-8 around
   −2.457, −2.747, −3.14, ...) and |x| < 2^-20, each a full mp call (~400k instructions); 0.5 % of operands
   would explain it. The lever is a double-double piece (or a wider kernel) beside the negative zeros.
2. Optional: allow `decide` up to just below FLT_MAX to drop slice 248's mp (~3 % of sweep CPU).

## Decisions in force
Rulings #2760 (option A), #2784 (one story per family), #2783 (software sqrt; not used), #2889 (tiered oracle).
