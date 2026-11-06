# TIMING-COV-MEMCHR-SHORT-NONAVX2: memchr at 16 bytes and memrchr with an early hit on the non-AVX2 class

Id: TIMING-COV-MEMCHR-SHORT-NONAVX2
Kind: code
Priority: 65
Size: S
Host: -
Stacks-on: TIMING-N100-SCAN-64K
Model: opus
Effort: high
BUDGET: 70 calls / 140k

## The change
Cut the non-AVX2 short path of liella_machine_memchr (csys/src/machine/scan.S; entry liella_machine_memchr_entry) for
n <= 32, and of memrchr for a hit in the first block (grep -n memrchr csys/src/machine/scan.S). Same results.
Stack on TIMING-N100-SCAN-64K (running, edits the same memchr kernel) and leave TIMING-IVB-MEMREF-ENTRY's memchr_64
entry change in place (do not redo it).

## Rows (TIMING-SWEEP-1105, Ivy Bridge, instr / time)
memchr_16 1.580 / 1.584, memchr_16_mix 1.338 / 1.171, memrchr_64_hit 1.107 / 1.276. N100: all faster.

## Evidence (keke, next 730f416b, job timing-coverage-1106-percall)
Non-AVX2 path, Liella / glibc instructions per call: memchr_16 50 / 31 (AVX2 path 29 / 30), memrchr_64_hit 33 / 30
(AVX2 29 / 29). Liella trace of one memchr_16 call (non-AVX2): 39 instructions, 36 in liella_machine_memchr, 3 in the
memchr entry. Workloads: memchr 6.7M calls in 12 of 30 workloads, memrchr 0.59M in 5.

## Hypothesis
The non-AVX2 build reaches a 16-byte miss through the general aligned loop (head, alignment, loop test) instead of one
unaligned 16-byte load with a length mask when the load stays within the page; the AVX2 build already does it in 29.

## Steps
1. wl_percall --trace memchr:200:200 under GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2 (Liella only) for memchr_16 and a
   16_mix-like offset set (bench/c/cbench_memref.c MROW) to locate the instructions.
2. Add or reorder the n <= 32 class in the non-AVX2 build (page-cross check kept).
3. Tests: memchr / memrchr every length 0-80 x every hit position x page-end placement, both dispatch paths.
4. Plants: off-by-one on the length mask, page-cross check removed, memrchr first-block hit returns the wrong match.

## JOBS
gate, plants, counts before / after (wl_percall --spec handoff/timing-coverage-1106/tc1106.c --cases memchr_16,memrchr_64_hit,
both dispatch settings), timing nico:timing-cov-memchr-short-time (`ops timing-book nico <min> --story TIMING-COV-MEMCHR-SHORT-NONAVX2`).

First line: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:
