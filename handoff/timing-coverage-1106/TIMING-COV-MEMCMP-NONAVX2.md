# TIMING-COV-MEMCMP-NONAVX2: memcmp / bcmp / __memcmpeq early-difference and equal rows on the non-AVX2 class

Id: TIMING-COV-MEMCMP-NONAVX2
Kind: code
Priority: 70
Size: M
Host: -
Stacks-on: PARITY-MEMCMP-SMALL-EQ
Model: opus
Effort: high
BUDGET: 80 calls / 160k

## The change
Cut the instructions of liella_machine_compare and liella_machine_equal (csys/src/machine/fold.S) on the non-AVX2 builds
for: a first difference in the first 16 bytes of a long compare (head rows), a difference at byte 8 for n 64-127
(memcmp_diff8, the sort -C shape), and equal 64 B / 128 B / 1 KiB. The AVX2 path is PARITY-MEMCMP-SMALL-EQ's
(stack on it: same labels). Results unchanged: sign for memcmp, zero / non-zero for bcmp and __memcmpeq.

## Rows (TIMING-SWEEP-1105, Ivy Bridge, instr / time)
__memcmpeq_head_128 1.842 / 1.768, __memcmpeq_head_1k 1.845 / 1.768, bcmp_head_128 1.553 / 1.916, bcmp_head_1k 1.559 / 1.916,
memcmp_diff8_127 1.677 / 1.584, memcmp_diff8_96 1.672 / 1.584, memcmp_diff8_64 1.321 / 1.250, __memcmpeq_eq_64 1.375 / 1.187,
__memcmpeq_tail_64 1.370 / 1.267, bcmp_eq_64 1.340 / 1.200, __memcmpeq_eq_1k 1.212 / 1.239, bcmp_eq_1k 1.208 / 1.249,
bcmp_tail_64 1.120 / 1.056, __memcmpeq_eq_128 1.109 / 1.349, __memcmpeq_tail_128 1.109 / 1.419, __memcmpeq_tail_32 1.107 / 1.091,
bcmp_eq_128 1.077 / 1.444, memcmp_256 1.086 / 1.226. N100: all faster except memcmp_256 / 4k (TIMING-N100-MEMCMP).

## Evidence (keke, next 730f416b, job timing-coverage-1106-percall, handoff/timing-coverage-1106/)
Non-AVX2 path (GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2), Liella / glibc instructions per call: memcmp_diff8_96 45 / 26,
memcmp_diff8_64 36 / 26, bcmp_head_128 42 / 26, __memcmpeq head_1k 42 / 22, bcmp_eq_64 57 / 42, __memcmpeq tail_64 57 / 41,
bcmp_eq_1k 392 / 323, memcmp_256 118 / 107. These match the sweep's IVB instr ratios (1.36-1.91). The AVX2 path is
close (head_1k 27 / 21, diff8_96 32 / 25, others 0.95-1.08).
Workloads: memcmp is the most-called libc compare (46.8M calls, 13 of 30 workloads; WL-ENTRIES table on keke).

## Hypothesis
A head mismatch costs ~16 extra instructions whatever n is (42 at 128 and at 1 KiB), so the non-AVX2 entry for n > 64
does setup (alignment / loop set-up) before testing the first 16 bytes. diff8 at 96 (45) costs more than at 64 (36):
the 65-127 class tests its first block late. Test the first 16 bytes before any setup for n > 16, as the 33-64 class does.
Equal 64 B: 57 vs 42 suggests one more block test or a branchy tail than needed.

## Steps
1. grep -n the size classes in liella_machine_compare / _equal (non-AVX2 builds); wl_percall --trace memcmp:200:400
   for diff8_96 and bcmp head_128 under -AVX2 to see the path (Liella only).
2. Change the class entry order; keep AVX2 builds as PARITY-MEMCMP-SMALL-EQ leaves them.
3. Tests: the exhaustive size 0-300 x first-difference sweep in crates/boundary/tests (machine_scanners / small_copy) must
   run with the non-AVX2 builds forced (both dispatch paths); add a trace clause id.
4. Plants (cargo xtask plants, msg: regex per plant, .S plants anchored to the function label): drop the first-block test,
   swap the sign of the head result, skip the last partial block of the equal path.

## JOBS
gate (`cargo xtask affected --base origin/next` on the build host), plants (`cargo xtask plants`), counts before / after
(validation/wl_percall.sh --spec handoff/timing-coverage-1106/tc1106.c, with and without GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2;
fix that spec's bcmp / __memcmpeq check to `r != 0` first), and a booked timing check of these rows:
nico:timing-cov-memcmp-nonavx2-time (`ops timing-book nico <min> --story TIMING-COV-MEMCMP-NONAVX2`), plus the AVX2-class
rows not slower on nozomi:timing-cov-memcmp-nonavx2-n100.
Done when no row in the list is above 1.05x in instructions on the non-AVX2 path and no memref row is slower.

First line: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:
