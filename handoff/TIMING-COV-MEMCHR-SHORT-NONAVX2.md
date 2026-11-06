# TIMING-COV-MEMCHR-SHORT-NONAVX2 (handoff, run 3 = TIMING-COV-MEMCHR-SHORT-NONAVX2-3)

State: branch task/timing-cov-memchr-short-nonavx2-3 (on -2's branch, stacked on task/timing-n100-scan-64k-3,
3aac4b92). Code commits 3c5cb192 (16-byte SSE2 memchr), c2260bf9 (setup/loop at base offsets), c2d802ee (run 3:
head16 pad). Role shiki:~/w/sh1. Scratch shiki/kotori:~/work/TIMING-COV-MEMCHR-SHORT-NONAVX2-3 (stage.sh,
stage-diag.sh, rows3.sh, rowsd.sh, base/head3/diag tars, dis-base/head3/diag.txt). Decisions: D-4.57 (s04), D-4.56.
Verdict: INCOMPLETE, RULING NEEDED: memchr_64_mix stays 4.5% slower than base on Ivy Bridge (still faster than glibc).

## Run 3
1. Pad (c2d802ee): `.org liella_machine_memchr_entry + 0x25e, 0xcc` after block 0's ret (2 int3, never executed).
   Disassembly: head3 == base byte for byte at 0x268-0x38a (fold 0x268, g1 0x2f0, ret 0x38a) and 0x3ba-0x5a0
   (setup, loop 0x480, .Lmemchr_none). Only differences left: entry dispatch, block 0 (0x23e vs base 0x24a), cold
   edge block. No executed instruction changes (the only change is a jz target 2 bytes later).
2. Timing, kotori (Ivy Bridge, AVX2 off), job kotori:timing-cov-memchr-short-nonavx2-3-time3, 45 alternations
   base/head3/glibc rotated, ns per call per frequency mode (low / high):
   | row | base | head3 | glibc |
   |---|---|---|---|
   | memchr_16 | 7.63 / 11.92 | 6.83 / 10.67 (-10.5%) | 4.82 / 7.54 |
   | memchr_16_mix | 8.43 / 13.18 | 7.63 / 11.92 (-9.6%) | ~7.2 / ~11.3 |
   | memchr_64_mix | 8.83 / 13.80 | 9.23 / 14.43 (+4.5%) | ~9.4 / ~14.7 |
   | memchr_4k | 121.32 / 189.56 | 117.28 / 183.25 (-3.3%) | ~137 / ~214 |
   | memrchr_64_hit | 4.42 | 4.42 | 3.47 |
   Identical to head2: the 2 bytes do NOT explain memchr_64_mix.
3. Diagnostic (not committed, coordinator: no third layout blind): "diag" = head3 with `cmpq $16 / jbe` replaced by
   one 10-byte nop (same offsets, same uop count, branch gone). Job kotori:timing-cov-memchr-short-nonavx2-3-diag,
   30 alternations base/head3/diag/glibc (all high mode): memchr_64_mix base 13.80, head3 14.43, diag 14.43;
   memchr_16 base 11.92, head3 10.67, diag 12.55. So the n <= 16 branch costs nothing on 64_mix, and every in-page
   call pays one quantum (+0.63 ns high mode, about one cycle) in the part of the change before it: the entry's
   merged level+page test (`cmpl $4032, %ecx; jbe .Lmemchr_sse2` first, then the level load) and/or block 0's new
   position (0x23e, 12 bytes earlier). memchr_16's early exit wins back two quanta, memchr_4k gains from elsewhere.
4. Gate shiki:timing-cov-memchr-short-nonavx2-3-gate on 62d6b9dc (code = c2d802ee; later commits are handoff/ only):
   10 suites ok, 10223 passed, 0 failed; conformance 117/117; rc 0; peak 15G. Clippy (-D warnings) and static clean.
   Plants shiki:timing-cov-memchr-short-nonavx2-3-plants3 (5 plants + head16-pad-removed `equivalent:`).
   -2's four jobs (kanon gate/plants, kotori time/time2) were all rc 0.

## RULING NEEDED
Accept memchr_64_mix +4.5% on Ivy Bridge AVX2-off (still ~2% faster than glibc there) for memchr_16 -10.5%,
memchr_16_mix -9.6%, memchr_4k -3.3%. Recommendation: accept and land. Otherwise run 4 tests the two remaining
candidates separately (one variant with the base entry order and the base page test inside .Lmemchr_sse2, one with
block 0 back at 0x24a by padding before .Lmemchr_sse2, which moves the 0x231 pin: D-4.56).

## Next step
Ruling. If rejected: the two isolating variants above with stage-diag.sh/rowsd.sh (pattern: same offsets elsewhere,
one difference each), on the less busy Ivy Bridge host. memrchr_64_hit: unchanged (1.277 of glibc), no cut found.
Plan rows to read: D-4.56, D-4.57 (s04), D-4.45, D-4.48.
