# TIMING-COV-STRSCAN-SHORT-IVB (handoff)

State: branch task/timing-cov-strscan-short-ivb; code head a563b885 (csys/src/machine/scan.S), docs/handoff commit on top.
Role keke:~/w/n1. Decisions in force: D-4.45, D-4.59, D-NEW (this story; placeholder, coordinator assigns the id).

## Profile (Ivy Bridge, perf stat per row, Liella base / glibc, per 1e7 counted units; script run.sh in scratch)
strchr_16_hit: cycles 85.7 / 74.2, instructions 237.9 / 234.3, taken branches 29.6 / 22.4 (4 vs 3 per call),
DSB 4-uop cycles 34.9 / 57.4, idq_uops_not_delivered 98 / 3.5, resource stalls 0.9 / 11.3, no LCP, no split loads, MITE
under 2%. The gap at instruction parity is front-end delivery, and it tracks taken branches: Liella has one taken branch
per call more than the reference (the entry's level-dispatch `jae`), plus one per missed head block.
memrchr_64_hit: 1 split load per call on BOTH sides (bench buffer 64-aligned, the head reads [end-16, end)), so the split
is not the gap; the gap is 3 instructions (31 vs 28) and delivery (nd 93 vs 9).
memchr_64 / rawmemchr_4k: no front-end or stall signal in the aggregate counters (memchr_64 time 1.117, instr 0.784).

## Change (scan.S, SSE2 head of strchr and strchrnul)
Block 2 falls through into block 3 on a miss and jumps to an out-of-line tail (.Lschr_t32 / .Lschrnul_t32) on a hit;
blocks 0, 1, 3 keep their hit inline. Taken branches by deciding block 0/1/2/3/beyond: 0/1/3/2/3 (before 0/1/2/3/4).
Same instructions on every path. A pure linear chain cannot be never-worse than base; this trades a decision in bytes
32-47 (+1 taken branch, no bench row) for bytes 48-63 and every longer string (-1).
NEGATIVE (commit 6cb1a11b, replaced): blocks 1-3 all out of line: 64 rows 0.90-0.95 but strchr_16_miss 1.200 and
strchrnul_16_miss 1.076 Liella after/before.

## Timing (Ivy Bridge nico booking 112; N100 nozomi booking 111; staged trees base e0c6b146 vs head, 3 alternations x 5)
Ivy Bridge, final layout (Liella after/before, L/g time before -> after): strchr_64_hit 0.948 (1.136 -> 1.075),
strchr_64_miss 0.902 (1.289 -> 1.162), strchrnul_64_hit 0.895 (1.274 -> 1.140), strchrnul_64_miss 0.895 (1.194 ->
1.069), strchr_16_hit/miss and strchrnul_16_hit/miss 1.000/1.000/1.000/0.998; memchr/rawmemchr/memrchr rows 0.992-1.000
(untouched code). Counters: taken branches 50.6 -> 43.6 on the 64 rows, unchanged on the 16 rows.
N100: the first layout (6cb1a11b) left all 17 rows at 1.000 (AVX2 level does not run the SSE2 head); the final layout's
N100 run is job nozomi:timing-cov-strscan-short-ivb-nozomi2.
Target not reached: 16_hit rows stay 1.11-1.12 of the reference.

## Jobs
keke: timing-cov-strscan-short-ivb-plants2 (9/9 CAUGHT on a563b885 scan.S), timing-cov-strscan-short-ivb-gate3 (final head).
nico: timing-cov-strscan-short-ivb-nico2 (final layout), nozomi: timing-cov-strscan-short-ivb-nozomi2.
SUPERSEDED: -stage-base (bad args), -cmdtest (rows format), -gate (killed: head moved), -gate2 (killed: docs commit),
-plants (first layout), -nico / -nozomi (first layout: the NEGATIVE evidence above).

## Next step
1. The remaining 16-byte gap is the entry's taken level-dispatch branch (one per call, ~1.5 cycles on Ivy Bridge).
   Removing it needs load-time selection of the SSE2 entry (an IFUNC-style exported symbol) or an SSE2 fall-through
   that gives the AVX2 level a taken branch: a structural/ABI-adjacent choice, RULING NEEDED (recommend the load-time
   selection story, measured on both classes).
2. memrchr_64_hit: cut the 3 surplus instructions of the SSE2 head (entry tests + page test), not the split load.
3. memchr_64 (1.117) / rawmemchr_4k (1.078): no counter signal here; needs per-region counters.
Do not redo: all-out-of-line layout (negative), stub placement and compare form (D-4.59).
