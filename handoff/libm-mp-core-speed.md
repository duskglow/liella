# LIBM-MP-CORE-SPEED handoff (updated by LIBM-MP-CORE-SPEED-2)

State: branch `task/libm-mp-core-speed-2` (stacks on `task/libm-mp-core-speed`), cut from origin/next b6353c7b.
Code commits: 5d68e879 (mp core), 6e4aa8cc (rounder), 4bbb53b9 (constant pin test). afe34f72: saturation section 20.
Role keke:~/w/n4 (the -1 run used n2). Scratch on keke: ~/work/libm-mp-core-speed-2/ (chain.sh, plants.txt).

## Done (LIBM-MP-CORE-SPEED, see saturation section 20)
1. `Mp::div_u64` without u128 software division (exact floor quotient, `div_u64_is_the_exact_floor_quotient`).
2. `pack`/`shr` shift-light (digest unchanged).
3. `MP_LN10`, `MP_LN_PI` generated constants in place of per-call `ln_atanh` series.
4. `elem::direct` double-double rounder (`direct_rounding_matches_the_wide_route`).

## The ln10 digest change (coordinator question, answered by -2)
`narrow_kernels_unchanged` was retaken (0x6e1d_732c_6aea_f08c) when `ln10` became `MP_LN10`. Settled: the new
constant is right and the old series value was the inexact one. Python `decimal` at 250 digits (pi by Machin, no
liella-mathtools code) gives ln 10 and ln pi whose 384-bit truncations equal `MP_LN10` / `MP_LN_PI` limb for limb;
the parts below the last bit are 0.173 / 0.226 units, so truncation = nearest (script:
~/w/.scratch/LIBM-MP-CORE-SPEED-2/ln10ref.py on the coordinator host). The old `ln_atanh(10)` lies within its proven
2^(e_ln+8-B) = 256 units; it differs in the low bits, hence the digest moved. Test
`mp::tests::ln10_and_ln_pi_constants_are_correctly_rounded` pins both constants and checks the series bound; the
accuracy job prints the measured series offsets (`ln_atanh(10) - MP_LN10 = ... units`).

## Why the -1 jobs did not finish
- `libm-mp-core-speed-accuracy` rc 124: `cargo test --release -p liella-math -- --include-ignored` also ran the three
  fast32 exhaustive f32 sweeps, which ran past the 150 min job limit under load (25 on 60 threads). It never reached
  the integration suites. The -2 accuracy job runs the release suites plus the two mp-path count jobs
  (`random_sample_retry_count`, `accuracy_suite_retry_count`), not the f32 sweeps. NOT RUN: the f32 sweeps; the f32 accurate path
  goes through `elem::direct`, covered by `direct_rounding_matches_the_wide_route` and the sampled fast32 tests.
- `libm-mp-core-speed-counts-after` rc 0 ran on n2 at 07:36 after the accuracy job ended; whose tree n2 held then is not
  proven, so its numbers (in section 20) are rechecked by `libm-mp-core-speed-2-counts-after` on n4.
- -1 gate: rc 0, escalated to FULL (generated tables changed): 10823 passed, 0 failed.

## Jobs of -2 (keke, run by ~/work/libm-mp-core-speed-2/chain.sh in this order: lint, gate, accuracy, counts-after; plants in parallel)
libm-mp-core-speed-2-lint, -gate, -accuracy, -counts-after, -plants (5 plants: reciprocal 1-off, pack drops a shift,
MP_LN10 limb, MP_LN_PI limb, rounder Down-mode sign). Read: `ssh keke '~/bin/jobwait <labels>'`.
Compare counts: `python3 -I cmp.py before.log <counts log>` (scratch; before = ~/work/libm-mp-core-speed/before.log, base b6353c7b).

## Next step
If all -2 jobs are rc 0 and counts-after matches section 20 (within 0.5%), the branch is ready to land. Otherwise fix
the named job. Possible later win (unmeasured): a 21-product `sqr`.
