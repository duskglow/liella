# TIMING-COV-MEMCHR-SHORT-NONAVX2 (handoff, run 2 = TIMING-COV-MEMCHR-SHORT-NONAVX2-2)

State: branch task/timing-cov-memchr-short-nonavx2-2, stacked on task/timing-n100-scan-64k-3 (3aac4b92). Code commits
3c5cb192 (16-byte SSE2 memchr), c2260bf9 (layout: setup/loop back at base offsets). Role kanon:~/w/kn1. Scratch
kanon/kotori:~/work/TIMING-COV-MEMCHR-SHORT-NONAVX2-2 (stage.sh, rows.sh, base.tar, head.tar = 3c5cb192 code,
head2.tar = c2260bf9, dis-*.txt disassembly of memchr from entry). Decisions: D-4.57 (s04), D-4.56 (0x231 pin).
Verdict: INCOMPLETE: one row (memchr_64_mix) still 4.5% slower than base on Ivy Bridge, same instructions (layout).

## Change (csys/src/machine/scan.S, SSE2 memchr only)
Run 1 (3c5cb192): entry tests `ecx <= 4032` first, `.Lmemchr_sse2` lost its page test, edge block broadcasts c,
`.Lmemchr_sse_head16` returns NULL for n <= 16 before folding blocks 1-3. SSE2 memchr_16 50 -> 33 instr (glibc 31).
Run 2 (c2260bf9): `.Lmemchr_sse_edge` (cold) moved into the int3 reserve before the 0x231 pin; a second
`.org entry + 0x3ba` puts `.Lmemchr_sse_setup` back at its base offset, so setup/body/g1/loop are at base offsets
(loop 0x480; run 1 had 0x490). No instruction changes.

## Timing (kotori, Ivy Bridge E3-1265L v2, AVX2 off, 45 alternations rotated base/head/glibc, rows.sh)
The CPU ran in two frequency modes (values cluster at x and 1.5625x); compare per mode, not the median.
ns per call, per mode (low / high): base, run-1 head, run-2 head (head2):
| row | base | head (3c5cb192) | head2 (c2260bf9) | glibc |
|---|---|---|---|---|
| memchr_16 | 7.63 / 11.92 | 6.83 / 10.67 (-10.5%) | 6.83 / 10.67 (-10.5%) | 4.82 / 7.54 |
| memchr_16_mix | 8.43 / 13.18 | 7.63 / 11.92 (-9.6%) | 7.63 / 11.92 (-9.6%) | ~7.2 / ~11.2 |
| memchr_64_mix | 8.83 / 13.80 | 9.23 / 14.43 (+4.5%) | 9.23 / 14.43 (+4.5%) | ~9.4 / ~15.1 |
| memchr_4k | 121.32 / 189.56 | 126.49 / 197.64 (+4.3%) | 117.28 / 183.25 (-3.3%) | 139.1 / 217.4 |
| memrchr_64_hit | 4.42 | 4.42 | 4.42 | 3.46 |
Spread within a mode is 0-0.02 ns, so +4.5% is real. Jobs kotori:timing-cov-memchr-short-nonavx2-2-time (base/head),
kotori:timing-cov-memchr-short-nonavx2-2-time2 (base/head2), both rc 0. Trial of the command on kanon first.

## Cause of the memchr_64_mix slowdown (evidence, not yet fixed)
Instructions on the n = 64 in-page path are unchanged (run 1 count). Disassembly (dis-base/head.txt, offsets from
liella_machine_memchr_entry): the in-page SSE2 region from 0x231 to the head-miss ret is 2 bytes earlier than base
(g1 group 0x2f0 -> 0x2ee, 0x38a ret -> 0x388): the n <= 16 test (+10 bytes) and the removed page test (-8, +-) net -2.
memchr_4k (setup + loop) recovered when the loop returned to 0x480, so the 64_mix loss is very likely this -2 shift.

## Next step
1. Restore the in-page region's base offsets after the head16 block: add 2 bytes where they are never executed
   (after an unconditional ret/jmp before `.Lmemchr_sse_head_miss` / g1, e.g. an `.org` to the base offset of the
   block after the first ret following head16), or shorten the n <= 16 test by 2 (jbe rel8 if .Lmemchr_none reach
   allows). Check with dis.sh that g1 is at 0x2f0 and the ret at 0x38a, recount memchr_16 (33), restage head3
   (stage.sh head3 with scan-head3.S), time base/head3/glibc on kotori/nico (rows.sh 45 base head3 glibc).
2. Then gate + plants (handoff/TIMING-COV-MEMCHR-SHORT-NONAVX2-plants.txt; edge plant regex still matches).
Note: this branch's xtask has no `bench --stage`; the stage.sh recipe (TIMING-N100-SCAN-64K-3's) was used, and the
full `--compare` (9 min per run) was replaced by the staged programs run with CBENCH_ROWS (rows.sh).
memrchr_64_hit: unchanged (no cut found in run 1; 1.277 of glibc).
Plan rows to read: D-4.56, D-4.57 (s04), D-4.45, D-4.48.
