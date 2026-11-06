# TIMING-IVB-MEMREF-ENTRY-2 (handoff; continues handoff/TIMING-IVB-MEMREF-ENTRY.md)

State: branch task/timing-ivb-memref-entry-2, cut from origin/next 9424819f (which already holds the memchr/rawmemchr
entry fix of TIMING-IVB-MEMREF-ENTRY). Role keke:~/w/n1. Code on the final head equals 9424819f: the split was
committed (b2b290de), timed, measured slower on memset_256 and reverted (new commit, no rewrite). Branch content:
this handoff, the NEGATIVE register row (docs/saturation/03-memory-and-string-kernels.md), and the rebased split
patch (handoff/TIMING-IVB-MEMREF-ENTRY-fold-split.patch, now applies to the folded 129-256 gate of NULLFOLD-COPY-MID-3).

## Ruling #3421 applied: the 129-192 split is DROPPED
Rebase: the old patch failed on fold.S comments only (the SSE2 bodies were unchanged); resolved by hand, the split sits
after `.Lmove_129_sse2_gate` and keeps the ORDER (all loads from s + 0 first). Plants on it: 3 of 3 CAUGHT
(keke:timing-ivb-memref-entry-2-plants, small_copy), now moot.
Counts (Zen 1 host, cbench_memref --compare --counters --tunables glibc.cpu.hwcaps=-AVX2, instr/call next -> split):
memmove_144 51.5 -> 45.5, memmove_256 51.5 -> 53.5, memset_144 39.5 -> 37.4, memset_256 39.5 -> 41.4.
Timing (whole cbench_memref, 3 staged trees interleaved, 4 alternations x 5 rounds, side order reversed every other
alternation; paired-round ratio median [min-max of per-alternation medians]; glibc drift 1.000 on every row):

| row | Ivy Bridge split/next | N100 (hwcaps=-AVX2) split/next |
|---|---|---|
| memmove_144 | 0.829 [0.829-0.829] (15.27 -> 12.66 ns; glibc 13.09) | 0.851 (11.18 -> 9.51; glibc 13.37) |
| memmove_256 | 1.000 | 1.000 |
| memset_144 | 0.999 (7.43 -> 7.42; glibc 6.12) | 0.998 |
| memset_256 | **1.117** (7.43 -> 8.30; glibc 7.43) | **1.089** (4.61 -> 5.02; glibc 5.85) |

The ruling's condition (both _256 rows hold, _144 rows improve) fails on memset_256 (both hosts) and memset_144
(no gain), so the split commit is reverted. The copy half alone met the condition on both hosts: see RULING NEEDED.

## Memchr entry fix (already landed in next): timing, before = next with fea51965's scan.S reversed
| row | Ivy Bridge next/before | N100 (hwcaps=-AVX2) next/before |
|---|---|---|
| memchr_16 | 1.001 (8.30 ns both; glibc 5.24) | **1.091** (4.60 -> 5.02; glibc 4.60) |
| memchr_64 | 0.900 (8.73 -> 7.86; glibc 7.42) | 0.917 |
| memchr_4k | 0.953 | 0.882 |
| rawmemchr_4k | **1.013** [1.013-1.145] (143.87 -> 145.71; glibc 133.73) | 0.913 |
Before tree: hunks 1-4 of the fix reversed cleanly; hunk 5 by hand (an `.org` pad added since sits before
`.Lmemchr_sse2`); its counts (memchr_16 46.4, memchr_64 42.4) match the pre-fix record.

## Jobs
keke: timing-ivb-memref-entry-2-stage-{split,after,before} (bench --stage + -AVX2 counts, rc 0),
timing-ivb-memref-entry-2-plants (split code, rc 0, moot), timing-ivb-memref-entry-2-gate (split head, stopped by me
when the split was dropped), timing-ivb-memref-entry-2-gate-final (final head). nico: timing-ivb-memref-entry-2-ivb2,
nozomi: timing-ivb-memref-entry-2-n100b (rc 0; ~/work/TIMING-IVB-MEMREF-ENTRY-2/analysis.txt on each). The first
timing labels (-ivb, -n100) ended rc 1 at once: `bench --rows` takes only rows with a bench/budgets.tsv line, and
memchr_16, memset_144 etc. have none; the runner now uses `--only cbench_memref` and the analyzer picks the rows.
Runner and analyzer: scratch copies on the timing hosts (~/work/TIMING-IVB-MEMREF-ENTRY-2/{run.sh,analyze.py}).

## RULING NEEDED
1. Copy-only split (memmove 129-192 six blocks per end, memset unchanged): memmove_144 -17% Ivy Bridge, -15% N100
   SSE2 level, memmove_256 flat on both, -6 instr. Recommend: a new story lands it with the fill half left out.
2. Landed memchr fix: rawmemchr_4k +1.3% on Ivy Bridge (same instruction count, one taken branch fewer) and
   memchr_16 +9% on N100 at the forced SSE2 level (real N100 runs the AVX2 path, unchanged). memchr_64 -8/-10% and
   memchr_4k -5/-12% gain. Recommend: keep it; a follow-up profiles the rawmemchr SSE2 loop placement on Ivy Bridge.

## Open (out of scope here)
memcmp_144, bcmp_tail_128, __memcmpeq_tail_8 on Ivy Bridge: no attribution yet (step 2 of the old handoff).

## Next step
Ruling 1 and 2 above; then profile memcmp/bcmp/__memcmpeq on Ivy Bridge (region counters, idq_uops_not_delivered,
resource_stalls). Do not redo: the counts and timing above, the rebase of the split.
