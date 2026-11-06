# TIMING-COV-MEMCHR-SHORT-NONAVX2 (handoff, run 1)

State: branch task/timing-cov-memchr-short-nonavx2, stacked on task/timing-n100-scan-64k-3 (3aac4b92). Code commit
3c5cb192; later commits handoff/ only. Role keke:~/w/n2. Scratch keke:~/work/TIMING-COV-MEMCHR-SHORT-NONAVX2
(tc1106.c = handoff/timing-coverage-1106/tc1106.c of task/timing-coverage-1106, before.sh, after.sh, plants3.txt).
Verdict: INCOMPLETE: memchr_16 cut by instructions; the brief's timing job (nico) was not run (budget).
Decisions: D-NEW (plan/07-decisions/s04, placeholder after D-4.56: no id was reserved; the coordinator reserves it).

## Change (csys/src/machine/scan.S, SSE2 memchr only)
1. Slow entry `.Lmemchr_entry`: `cmpl $4032, %ecx; jbe .Lmemchr_sse2` first (short_lim is 4033 on AVX2, so this is
   the SSE2 level with [s, s + 64) in s's page), then the level test splits the AVX2 page-edge calls from the SSE2 page
   edge. `.Lmemchr_sse2` lost its own page test; `.Lmemchr_sse_edge` now broadcasts c itself.
2. `.Lmemchr_sse_head16`: `cmpq $16, %rdx; jbe .Lmemchr_none` before folding blocks 1-3.
Instruction effect per path: SSE2 in-page n <= 16 miss in block 0: -13 (fold + hit resolve skipped, -2 tests);
SSE2 in-page block-0 hit: -2; SSE2 in-page n > 16 past block 0: 0 (+2 n test, -2 merged test); SSE2 page edge: 0;
AVX2 page offset > 4032: +2 (one not-taken fused pair); AVX2 in-page: 0 (unchanged bytes before the jae).
The D-4.56 `.org` pin is kept: the SSE2 path still starts at entry + 0x231, but its bytes changed (SSE2 code after
0x231 is shorter by the page test, the edge block longer by the broadcast), so code after it moved.

## Profile (wl_trace, -AVX2, one call, Liella only)
memchr_16 before: 39 = entry 3 + kernel 36 (prefix 4, level 3, broadcast 3, page 2, block 0 5, fold 11, hit
resolve 3, h16_hit 3, none 2). After: 22.
memrchr_64_hit (n 6, hit at 5): 22 = entry 4 (n and NULL tests) + kernel 18 (end-page mask test 3, SSE2 broadcast 4,
page test 2, block 5, bound 4). No cut found: the level split needs two tests on one level while the AVX2 path keeps
one (one `test` cannot give a level-only flag: its result bits all depend on s + n - 1), the SSE2 broadcast is 4
instructions without SSSE3, and the bound test is needed for n < 16. NEGATIVE for this run; not changed.

## Counts (wl_percall, keke, tc1106.c; glibc / base 3aac4b92 / new 3c5cb192, instructions per call)
| case | dispatch | glibc | base | new |
|---|---|---|---|---|
| memchr_16 | default (AVX2) | 30.0 | 29.0 | 29.0 |
| memrchr_64_hit | default (AVX2) | 29.0 | 29.0 | 29.0 |
| memchr_16 | hwcaps=-AVX2 | 31.0 | 50.0 | 33.0 |
| memrchr_64_hit | hwcaps=-AVX2 | 30.0 | 33.0 | 33.0 |
Jobs: timing-cov-memchr-short-before (base, both settings, traces), timing-cov-memchr-short-after (rc 0).

## Tests and plants
New test `short_memchr_and_memrchr_answer_at_every_length_position_and_page_placement` (machine_scanners.rs;
s4_5_c_1, s4_5_c_2): lengths 0-80 x none and every single-match position x every offset in the 144 bytes before an
unmapped page and offsets 0-80 after one, both dispatch paths, bytes outside the region = the target.
Plants (handoff/TIMING-COV-MEMCHR-SHORT-NONAVX2-plants.txt, 5): plants2 = 5 failing (2 CAUGHT, 3 WARN-NO-MSG: the
memrchr plants abort in the selftest build's result check (SIGABRT) before the assert; edge-no-broadcast fails
bounded_scanners/every_scanner then SIGSEGVs). plants3 = the same with those messages named. The first plants job
was refused by the tool (line 4 not anchored to its function label).

## Next step
1. Timing (brief): book nico (`ops timing-book nico 60 --story TIMING-COV-MEMCHR-SHORT-NONAVX2 --reason timing`),
   stage base 3aac4b92 and head as in handoff/TIMING-N100-SCAN-64K.md "Staging recipe", time memchr_16, memchr_16_mix,
   memchr_64_mix, memchr_4k, memrchr_64_hit (Liella and glibc interleaved, -AVX2 on Ivy Bridge), label
   nico:timing-cov-memchr-short-time. The SSE2 layout moved after 0x231 (D-4.56 saw 14% from layout alone): if
   memchr_16_mix or memchr_64_mix is slower, try re-pinning the hot SSE2 blocks before reverting.
2. memrchr_64_hit: no instruction cut on SSE2 (above); if its time gap (1.276 at 1.107 instructions) matters, look at
   the taken `jz .Lmrc_entry` on the SSE2 path (layout), measured by timing only.
Plan rows to read: D-4.56, D-NEW (s04), D-4.45, D-4.48.
