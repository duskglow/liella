# SSE2-COPY-MID-SPLIT (handoff): memmove/memcpy 129-192 six blocks per end on the SSE2 level, copy half only

State: branch task/sse2-copy-mid-split, cut from origin/next e119b8bc. Code head 464bc9ba (the handoff commit sits on
top of it). Role mei:~/w/s2. Verdict: **DO NOT LAND**. memcpy_256 measured slower on both timing hosts in both
layouts tried (table below), so ruling #3587's condition ("the _256 rows hold") fails on a row the earlier
measurement did not time. The branch is kept unreverted as evidence for the ruling below.

## What the branch holds
- fb8480f9: copy half of the rebased split from handoff/TIMING-IVB-MEMREF-ENTRY-fold-split.patch (hunks 1-3 only;
  memset untouched). `.Lmove_129_sse2`: `cmpq $192` then six 16-byte blocks per end for 129-192, eight for 193-256.
  The D-4.55 gate is unchanged, every load precedes the first store and the first access is still the load at s + 0.
  It also adds the `small_copy` test moves_of_128_to_193_match_a_byte_loop_at_every_overlap_shift_in_each_build
  (every size 128-193 at every shift -(n+1)..=(n+1), all 7 movers, each build), the cbench_memref 192 size rows
  (memcpy/memset/memmove/memcmp_192; no budget lines), the plants file and decision row D-4.61.
- df14dbba: plant messages use `[0-9]`: the plants tool's message match does not take `\d` (WARN-NO-MSG otherwise).
- 464bc9ba: layout 2. 193-256 falls through the compare and 129-192 takes the jump (`jbe .Lmove_129_192_sse2`).
  This was tried because layout 1 measured memcpy_256 1.155 on Ivy Bridge. The branch-removal plant became
  equivalent, so it was replaced by an inverted-condition plant.

## Counts (mei, Skylake-SP; cbench_memref --compare --counters; instr/call next -> layout 1 -> layout 2)
SSE2 level (--tunables glibc.cpu.hwcaps=-AVX2): memmove_144 51.4 -> 45.4 -> 45.4; memcpy_144 51.3 -> 45.3 -> 45.4;
memmove_192, memcpy_192 51.4 -> 45.3/45.4; memmove_256 51.4 -> 53.4 -> 53.4 (+2: compare and branch);
memcpy_256 next not captured in the grep (the next tree runs the same 8-block body as memmove_256, 51.4 expected,
NOT VERIFIED) -> 53.4. AVX2 level: every row unchanged (35.3-35.4). cbench_startup ran in both stage-head jobs
(rc 0, its instruction limits checked by the run; startup_instr_c 2148 user instructions).

## Timing (whole cbench_memref, next vs head trees from `bench --stage`, 4 interleaved alternations x 5 rounds,
side order reversed every other alternation; paired head/next ratio median [per-alternation min-max]; glibc drift 1.000)

| row | Ivy L1 | Ivy L2 | N100 -AVX2 L1 | N100 -AVX2 L2 |
|---|---|---|---|---|
| memmove_144 | 0.828 | 0.828 | 0.851 | 0.851 |
| memmove_192 | 0.853 | 0.853 | 0.845 | 0.845 |
| memmove_256 | 1.000 | 1.000 | 1.000 | 1.000 |
| memcpy_144 | 0.934 | **1.044** | 1.000 | 0.923 |
| memcpy_192 | 0.826 | 0.914 [0.908-0.920] | 1.000 | 0.923 |
| memcpy_256 | **1.155** (7.31 -> 8.44 ns) | **1.045** (7.31 -> 7.64) | **1.077** (5.44 -> 5.86) | **1.077** |
| memset_144/192/256 | 1.000 | 1.000 | 1.000 | 1.000 |

The memmove rows reproduce TIMING-IVB-MEMREF-ENTRY-2 exactly. N100 memcpy_256 is 1.077 whichever way the branch goes,
so the cost is not only the taken branch (the compare, or where the 8-block body now sits, is suspected; not profiled).
Raw: kotori and nozomi ~/work/SSE2-COPY-MID-SPLIT/analysis-1.txt (L1), analysis.txt (L2).
Ivy runs end rc 1 on BOTH sides: bcmp_eq_128 fails its cycle limit in every next and head run (8 of 8). Not this change.

## Jobs
mei: sse2-copy-mid-split-stage-next (rc 0), -stage-head (L1, rc 0), -stage-head2 (L2, rc 0), -plants (L1: 5 CAUGHT,
2 of them WARN-NO-MSG from `\d`), -plants-2 and -gate-2 (L2 final code head, started 13:07Z). Superseded:
-gate (L1; stopped by me with kill-label when the code changed). kotori: sse2-copy-mid-split-ivb (L1), -ivb2 (L2) (rc 1:
bcmp_eq_128, see above). nozomi: -n100 (L1), -n100b (L2) (rc 0). Bookings 86 (kotori), 87 (nozomi) cancelled at the end.

## RULING NEEDED
1. Land the copy split or drop it. It gains memmove_144/192 15-17% on both hosts, but memcpy_256 (the statx size)
   is 4.5-15.5% slower on Ivy Bridge and 7.7% slower on the N100 SSE2 level in both layouts. Plain memcpy is more
   common than overlapping memmove. Recommend: do not land. Record a NEGATIVE saturation row (copy half; memcpy_256).
   Optional follow-up: profile memcpy_256 on the N100 (cycles, uops, idq_uops_not_delivered) for next vs L2, and try
   keeping the 8-block body at next's address (six-block body after it, `.p2align` on the jump target). Re-time
   the memcpy_* rows as well as memmove_*.
2. D-4.61 is written as if landed. If the split is dropped, the row should become REJECTED, like D-4.54.

## Do not redo
The counts and both timing runs above; the test and bench rows can be reused if the follow-up lands.

## Next step
Coordinator ruling 1. If dropped: revert fold.S on this branch (keep the test, the 192 bench rows and the plants
file with the D-row marked REJECTED), add the NEGATIVE register row, then gate.
