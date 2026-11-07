# COMPARE-HEAD-H2 / -H2-2 / -H2-3: head-first compare entry (memcmp / bcmp / __memcmpeq), checkpoint 3

State: branch `task/compare-head-h2-3` (base origin/next 5872ad47e). Commits: 09459dd58 H2 entries (D-4.66),
954b52553 plants, b737c441d S1 (0-15 fall through), **0577844f4 option (c) (this run)**, then handoff/plan commits.
Role mei:~/w/s1. **Verdict: INCOMPLETE, must not land as it stands: option (c) fixes the AVX2 equal rows on the N100
but makes 8-16 byte rows and mix16 slower than next on both timing hosts. Three entry-order tweaks in a row have
each traded one set of rows for another: recommend a step-back / rethink, not a fourth tweak (RULING NEEDED below).**

## What the code does now (fold.S, dispatch.rs; D-4.66 amended)
* Entry of both functions: `leaq -33(%rdx),%rax; cmpq cmp_avx_span(%rip),%rax; jb .L*_wide` (span word 2016 with
  folding + AVX2, else 0; it replaced `liella_machine_cmp_long`, whose test behind the head block is gone), then
  S1: `cmp cmp_head; jae/ja .L*_head`, fall-through 8-15/16 words, 0-7 `.L*_lt8`. Head path as in H2 (SSE2 33+
  and AVX2 > 2048 only now).

## Evidence (this run)
* lint on inherited head `compare-head-h2-3-lint0` rc 0; on 0577844f4 `compare-head-h2-3-lint1` (xtask clippy +
  static) rc 0. Gate `compare-head-h2-3-gate` (direct tier, mei) rc 0, peak 14G; static on the final head
  `compare-head-h2-3-static` rc 0.
* Plants `compare-head-h2-3-plants` (handoff/COMPARE-HEAD-H2-plants.txt, 15 plants incl. 6 new for the span
  test): 14 CAUGHT, 1 EQUIVALENT (span one high: 2049 takes the exact AVX2 big classes), 0 SURVIVED, 0 WARN.
* Harness correctness sweep `compare-head-h2-3-sweep-2` (Skylake-SP): 4,250,610 checks, 0 failures for glibc /
  next / (c) in the native and forced-SSE2 modes.
* Counts (`compare-head-h2-3-percall`, memcmp_bands, Skylake-SP, (c) - next per call): AVX2 eq_32 -2, eq_48/64
  -1, eq_96..256 -3; eq_8/16 +2 (bcmp, __memcmpeq), memcmp eq_16 +1. SSE2: eq_8/16 +2, eq_32 -2, 48..256 -4..+2.
* Timing (rethink harness, 7 interleaved rounds, medians; ratio (c)/next; one quantum ~0.42 ns N100, ~0.40 Ivy):
| row | N100 (AVX2) | N100 forced SSE2 | Ivy Bridge (SSE2) |
|---|---|---|---|
| eq_32 / eq_64 | 0.875 / 0.875-1.0 | - / 1.11 | 1.0 / 1.07 |
| eq_96, eq_128 / eq_256 | **0.917 / 0.93-0.95** | - | memcmp 1.10 / 1.05 |
| eq_8 / eq_16 | 1.00 / 1.00 | 1.0 | memcmp 1.11 / **1.22**; bcmp, meq eq_16 1.125 |
| head_8..64 (bcmp, meq) | **1.14-1.17** (1 quantum) | 1.17 | head_32/64 1.20 (already in S1) |
| memcmp head_64 | **1.29** (2 quanta) | - | 1.09 |
| mix16 | **1.08-1.16** | 1.135 | 1.02-1.05 |
| diff8/head 96-4096 | 0.80-0.89 (bcmp, meq) | - | 0.60 (as S1) |
Logs: `nozomi:compare-head-h2-3-nozomi`, `kotori:compare-head-h2-3-kotori`; analyse with
`python3 -I ~/work/COMPARE-HEAD-H2-3/t3/analyze.py ~/logs/<label>.log` on that host.

## Reading
* The 3 not-taken instructions of the span test cost a quantum on the 8-16 rows on both cores (Ivy: span 0, pure
  overhead); on the N100 the 8-64 "head" rows (early difference) lose the early exit's gain. memcmp head_64 on the
  N100 now pays the 32-byte block path where next had an 8-byte head probe.
* Ivy head_32/64 1.20 (bcmp, meq) already came with H2's head block (S1 run); it was not flagged before.
* No entry order with one shared body serves both: every runtime ISA test in front costs the small rows, and the
  head block in front costs the AVX2 mid rows.

## Next step (recommendation)
1. Step-back / RETHINK (rut rule): per-ISA entries chosen once at startup (the classed slot swap, D-4.53, which ruling
   #3841 kept off for this family) would drop the runtime span test: the SSE2 body = S1 entry, the AVX2 body = next's
   ladder for 33-2048 plus the head block only where it wins. Needs a ruling on #3841.
2. If (c) is kept instead: reverting 0577844f4 restores S1 (Ivy small rows at next, N100 AVX2 eq_64-128 1.17-1.375).

## Not to redo
H2, S1 and (c) are all timed on both hosts (tables here and in the -H2-2 report). Plants file covers all three.
