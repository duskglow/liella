# TIMING-COV-COMPARE-HEAD-SSE2 (handoff, run 1)

State: branch `task/timing-cov-compare-head-sse2`, code/doc head 2ea1f4dd (19eb8d65 fold.S change + 2ea1f4dd D-4.62
amendment and plants msg), cut from origin/next e0c6b146. Role mei:~/w/s3. Scratch mei:~/work/TIMING-COV-COMPARE-HEAD-SSE2
(stage.sh, run.sh, counts.sh, head.tar = next, base.tar = 34c98f61, new.tar = 19eb8d65), kotori/nozomi
~/work/TIMING-COV-COMPARE-HEAD-SSE2 (step 1 trees) and .../r2 (next vs new). Bookings 106/107 cancelled.
Decisions: D-4.62 (amended, "Equality entry order"), D-4.53 (classed pages, swap off by default, #3434), D-4.35.

## Step 1 (done): booked re-time, base 34c98f61 vs next e0c6b146 vs glibc, 9 interleaved rounds, medians, ns/call
Ivy Bridge class (kotori, job timing-cov-compare-head-sse2-ivb): next <= base on every row (memcmp_256 14.45 -> 12.85,
memcmp_128 8.03 -> 7.63, diff8_96/127 6.43 -> 6.03; memcmp_4k 143.7 -> 144.3, +0.4%, spread 0.05). Rows above 1.05
on next: meq head_128/1k 1.538, bcmp head_128/1k 1.462, meq/bcmp eq_128 1.263, eq_1k 1.066/1.054, bcmp_tail_128
1.098, memcmp_diff8_64/96/127 1.251, strncmp_diff8 1.417. N100 forced SSE2 (nozomi, job ...-n100): next = base;
above 1.05: meq/bcmp eq_1k 1.200, meq tail_1k 1.184, bcmp tail_1k 1.149, meq eq_128 1.132, meq head 1.094,
strncmp_diff8 1.080. N100 default: only memcmp_256 1.061. So step 2 went ahead.

## Step 2 (one lever built): SSE2 equality entry order (fold.S, liella_machine_equal)
Profile (code reading, SSE2, bcmp/meq above 64): .Leq_other jns -> .Leq_big (cmp_long, ja) -> .Leq_huge (cmp_mid,
je) -> .Leq_sse (cmp_sse, ja) -> head word -> .Leq_one: 5 taken branches, where the compare entry already sends SSE2
straight to its SSE2 class. Ivy Bridge timing fits about 1 ns per taken branch on this path (head_64, 1 taken
branch, 4.02 ns; head_128, 5 taken, 8.03 ns). Change: `.Leq_big` -> `.Leq_sse` directly; the AVX2 flag test moves
into `.Leq_sse_over` (AVX2 above cmp_long: +2 instructions; no cbench row is in that class: EROW sizes stop at 1k).
Timing next -> new (glibc), r2 jobs kotori:timing-cov-compare-head-sse2-ivb2, nozomi:timing-cov-compare-head-sse2-n100-2:
* Ivy Bridge: meq head_128/1k 8.03 -> 6.83 (5.22) 1.308; bcmp head 7.63 -> 6.43 (5.22) 1.232; eq_128 9.64 -> 8.43
  (7.63) 1.105; eq_1k 38.9 -> 36.2 (36.5) 0.99; tail_128 8.84 -> 7.63 (8.43/8.04) 0.90/0.95; tail_1k 0.98/0.96.
  memcmp_*, strncmp_*: unchanged (head/base 1.000, names64 0.999).
* N100 forced SSE2: meq head 5.02 -> 4.18 (4.59) 0.91; bcmp head 4.60 -> 3.76 (4.59) 0.82; eq_128 7.10 -> 6.27
  (6.27/7.10); eq_1k 32.6 -> 28.6 (27.2) 1.054; tail_1k 1.054/1.023. -11% to -18% on every equality row above 64.
* N100 default (AVX2): 47 rows, none faster; __memcmpeq_tail_128 and bcmp_eq_128 4.60 -> 4.61 (one timer step,
  spread 0) on code whose bytes did not move (.Leq_big_load is before the edit).
Counts (mei job timing-cov-compare-head-sse2-counts, `xtask-bin bench --no-build --staged --counters`): the AVX2
class is unchanged on all 96 rows (every difference is 0.1, run noise). The "noavx2" half did NOT apply the tunable
(its numbers equal the default ones), so the SSE2 instruction delta is NOT MEASURED: by reading the code it is -2 per
call on SSE2 equality calls of 65-2048 bytes. Next run: find how `bench --counters` forces the SSE2 class.

## Plants (handoff/TIMING-COV-COMPARE-HEAD-SSE2-plants.txt)
plants-1 (mei job timing-cov-compare-head-sse2-plants): eq-big-to-huge CAUGHT (Signaled(6)), eq-over-flag-dropped
EQUIVALENT (results and NULL outcomes identical, speed only), eq-over-avx2-skips-null WARN-NO-MSG: caught by
compares_match_a_byte_loop_* FAILED (the mutant skips the head words), not by the NULL test I had named; msg fixed
in 2ea1f4dd and re-run as timing-cov-compare-head-sse2-plants2.

## Not done / next step (in order)
1. SSE2 counts measured (see Counts). 2. Remaining SSE2 rows above 1.05 on Ivy Bridge: meq/bcmp head_128 1.31/1.23
(still 3 taken branches: jnc, jns, ja to .Leq_sse; next lever: an entry ladder whose >64 SSE2 path falls through,
which needs the classed swap (D-4.53, off by default, #3434) or a ruling to trade an AVX2 taken branch),
eq_128 1.105, memcmp_diff8 1.251 (8-64 and 65-128 compare classes: bswap'd head word then a second word and
bswap again), strncmp_diff8 1.417 (step 3, untouched). N100 forced SSE2: eq_1k/tail_1k 1.02-1.05 (long loop).
