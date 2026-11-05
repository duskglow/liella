# LIBM-MP-CORE-SPEED handoff

State: branch `task/libm-mp-core-speed`, code commits 5d68e879 (mp core) and 6e4aa8cc (rounder), cut from origin/next b6353c7b.
Role keke:~/w/n2. Hand-back was at the budget checkpoint (70 calls): the jobs below were still running when it was written (plants had finished, rc 0).

## Done
1. `Mp::div_u64` (mp_core, every width): k < 2^32 divides by 32-bit half-limbs with native u64 division; k >= 2^32
   uses Moller-Granlund 2-by-1 division (IEEE TC 2011, Alg. 4) with one reciprocal per call (one u128 division
   per call, down from seven). Each step is exact, so the quotient is the same exact floor.
   Test `mp::tests::div_u64_is_the_exact_floor_quotient` checks it against 128-by-64 long division.
2. `pack`: one shift per limb, a plain copy when the top bit is already set (the old code did a div_euclid
   window per limb). `shr` got the same rewrite. add (no carry) takes the copy path; mul/sub shift.
3. `ln10` (384-bit) = generated `MP_LN10`; new `mp::ln_pi()` = `MP_LN_PI` for lgamma's reflection (special.rs).
   The 768/1536/3072-bit expansions keep the series (`ln10_value` per expansion site). Audit of mp, mpw, wide,
   special and complex_body: no other per-call constant derivation found.
4. `elem::finish` -> `elem::direct`: two_sum, one narrowing, sign of the remainder, in all four modes. Proof is in
   the doc comment. Falls back to `dd::round` for zero/non-finite/near-limit values and exact ties.
   Test `elem::tests::direct_rounding_matches_the_wide_route` compares it bit for bit (value and Status) with the
   256-bit route over ties, powers of two, binade edges and scaled results.

How bit-identity was shown: `narrow_kernels_unchanged` (digest of every 384-bit kernel) passed unchanged with
changes 1-2 and the series ln10 (RAM copy, keke). It was retaken (0x6e1d_732c_6aea_f08c) only for the new
ln10 constant. Unit run: 11/11 mp/mpw/elem/dd tests green. Clippy (workspace, all targets/features) and
`cargo xtask static` clean on the role at 6e4aa8cc.

## Jobs (keke, ~/logs/jobs.tsv; logs in ~/work/libm-mp-core-speed/)
- libm-mp-core-speed-counts-before: rc 0 (before-counters.tsv, before.log, base b6353c7b)
- libm-mp-core-speed-plants: rc 0 (04:12-04:17, none survived): 4 plants (`plants.txt`): reciprocal 1-off, pack skip that drops a shift, MP_LN10 limb,
  directed-mode sign in the rounder
- libm-mp-core-speed-gate (direct tier, --base origin/next), then -accuracy (`cargo test --release -p liella-math
  -- --include-ignored`), then -counts-after (after-counters.tsv), run in that order by chain.sh

## Not done / deviations from the brief
- Brief asked for four commits with counts each; there are two commits (changes 1-3 all touch mp.rs; non-interactive
  git cannot split hunks) and one before/after count pair. The per-change counts were not taken.
- The before/after comparison of the rows (erf*, lgamma*, tgamma*, j*/y*, *l, *f128, complex) was not read or
  summarised: NOT VERIFIED. The 2-3x / 10-20% expectation is unmeasured.
- `sqr` still runs the full 36-product mul (a symmetric 21-product square is a possible next win; bit-identical
  because the product is exact).

## Next step
Read the four jobs. If all are rc 0, compare before-counters.tsv with after-counters.tsv for the rows in the brief and
check no row got slower. Then add a saturation-register row for the mp core and the rounder.
