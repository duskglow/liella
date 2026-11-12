# LIBM-ASINF-NODIV: asinf/acosf/atanf without the per-call division (#4924)

State: branch task/libm-asinf-nodiv, code head 1311439b3 (plus the record/handoff commit on top). Run 1 handed back
INCOMPLETE at the context checkpoint: code, tests, record and plants file are done; the timing verdict and
instruction counts are NOT done, and the gate, plants and exhaustive sweeps were still running at hand-back.

## What changed (D-10.103, amends D-10.82)
- asinf/acosf: |x| < 1/2 polynomial x + x·z·R(z) (new fit `crates/math/fits/fast32_asin.fit`, odd 1..23 on
  [-1/2, 1/2], 2^-51.9), Estrin with powers up to z^4 only (stays normal down to |x| = 2^-126 for acosf);
  |x| >= 1/2 half-angle form pi/2 - 2 asin sqrt((1-|x|)/2) (Cody & Waite ch. 10). One sqrtsd, zero divisions
  (was one sqrt + two divisions). acosf: pi/2 - asin x / 2h / pi - 2h.
- atanf |x| > 1: one division (was two): c = j/32 from a 160-byte const table `ATAN_RECIP_J` indexed by
  exponent + top 5 fraction bits (j = round(32/bin midpoint), j = 0 at |x| >= 32), u = (1 - x c)/(x + c),
  new fit `fast32_atan32.fit` (odd 1..9 on [-1/32, 1/32], 2^-62.1). |x| <= 1 unchanged (one division).
- Kernels split into `asin_y`/`acos_y`/`atan_y` (double result) for the new unit test
  `fast32::tests::inverse_trig_onepass_kernels_are_within_their_bound` (dense in 3/8 <= |x| < 3/4, every 509th
  elsewhere, atan [1, 64) every 61st and above). Sweep edges added in `s10_math_symbols.rs` (1/2, 32, 2).
- Product-build trap met and fixed: keeping sqrt's operand g alive made `sqrtsd` merge into a stale register
  (D-10.68 partial-register check in `cargo xtask so`); g is now formed twice (both exact) - commit 1311439b3.

Bounds (any direction): asinf 19.3u, acosf <= 15u, atanf>1 5.4u; INV_ONEPASS_BOUND 24 / DELTA 64 unchanged.
Checked on shiki before hand-back: `cargo xtask clippy` clean (job libm-asinf-nodiv-check1, earlier head with
identical math), new bound test + `math_expf_onepass_entries_match_the_envelope` pass, `cargo xtask so` clean on
1311439b3 (libm-asinf-nodiv-so).

## Jobs (shiki, role sh1)
- libm-asinf-nodiv-gate2: `cargo xtask static && cargo xtask affected --run --base origin/next` on 1311439b3.
- libm-asinf-nodiv-plants2: handoff/LIBM-ASINF-NODIV-plants.txt (5 plants: asin coefficient -> asinf sweep
  stride 3; atan32 coefficient, split up to 0.625, split down to 0.4375, table cut at 128 -> the bound test).
- libm-asinf-nodiv-sweeps: the three exhaustive one-pass sweeps (asinf, acosf, atanf; 2^32 x 4 directions),
  starts when gate2 ends. Re-attach: `ssh shiki '~/bin/jobwait libm-asinf-nodiv-gate2 libm-asinf-nodiv-plants2 libm-asinf-nodiv-sweeps'`.
- Superseded: libm-asinf-nodiv-plants (baseline `so` failed on the partial-register check), libm-asinf-nodiv-gate
  (killed: stale head).

## Gate2 result
RED only on two start-up page-fault bounds (hot_text_products: first localtime_r 21 > 20; localtime_r + mktime 21 > 20).
This change touches no start-up code: LAYOUT-JITTER (#4347). Not chased; origin/next not yet measured with the same count.

## Next step (continuation)
0. Measure the same two counts on origin/next (handoff/layout-stability-rethink.md tool) per #4347.
1. Read the three jobs above; fix anything red (plants WARN lines too).
2. Counts: `cargo xtask bench --compare --counters --runs 3 cbench_libm --rows asinf,acosf,atanf` on the role.
3. Timing (the verdict, ruling #4924): `cargo xtask bench --stage <tar> --only cbench_libm` on the role, book
   nico or kotori (Ivy Bridge) and nozomi (N100), run `./xtask-bin bench --no-build --compare cbench_libm --rows
   asinf,acosf,atanf --runs 5` under `~/bin/timing`. keke (Zen 1) is NOT bookable (`ops timing-book` refuses:
   "not timing-capable"): RULING NEEDED for the Zen 1 leg. Primed next for comparison: `--base` staging needs a
   git checkout, not the synced role; use a local git worktree of origin/next or the earlier booked figures.
4. Each function no slower anywhere; if one is slower, revert that part and record the negative row.

Plan rows: D-10.82, D-10.68, D-10.103 (records/decisions/D-10/).
