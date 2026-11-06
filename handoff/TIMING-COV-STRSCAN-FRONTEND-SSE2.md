# TIMING-COV-STRSCAN-FRONTEND-SSE2 (handoff)

State: branch task/timing-cov-strscan-frontend-sse2, code head 8a5f4ed5 (csys/src/machine/scan.S, D-4.59 row,
plants file); this handoff commit follows. Role shiki:~/w/sh2 (Haswell-EP E5-2697 v3). Decisions in force: D-4.21,
D-4.33, D-4.35, D-4.39, D-4.45, D-4.59 (new).

## Profile (step 1): the hypothesis is refuted
Per-row Liella and glibc counters (perf stat on target/liella/bench/cbench_hot and .host-static, CBENCH_ROWS=<row>,
GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2; script ~/work/timing-cov-strscan-frontend-sse2/perf.sh; values per 1e7
iterations = 8 calls, divide by 8 for per call). idq.mite_uops is 0.25-1.5 against idq.dsb_uops 200-450 on all seven
rows, for both libraries: the legacy decoder does NOT feed the SSE2 heads; no window falls out of the uop cache.
The gap is delivery: idq_uops_not_delivered Liella/glibc, before: strchrnul_16_hit 51.7/22.7, strchr_64_miss
65.8/24.8, strncmp_eq16 114.6/36.6, with Liella's DSB uops equal or fewer. Cycles (before): strchr_16_hit 67.2 vs 70.2
(Liella faster), strchrnul_16_hit 67.2 vs 59.5, strncmp_eq16 133/131 vs 133/124 (host noise ~5-10%).

## Change (scan.S)
1. strchr / strchrnul: the AVX2-level page-end/NULL stubs moved before the 32-aligned SSE2 head (jae 2 bytes, not 6).
   Measured: no change (strchrnul_16_hit cycles 67.2 -> 68.1 -> 67.8, nd 51.7 -> 52.3); the 32-byte-window model
   that predicted a gain is wrong for this row.
2. strncmp: a vector hit before n falls through into .Lsnc_byte; an equal NUL returns in place. eq16 nd 114.6 -> 86.5,
   cycles 133/131 -> 123/123 (glibc 123-124): parity on this host. Instruction counts unchanged (67.0 vs 66.0).
3. strchrnul SSE2 head blocks use strchr's compare/compare/or form (same instruction count, one step shorter chain).
   Measured neutral (67.8 vs 67.2 cycles): the strchr-vs-strchrnul gap is NOT that chain.
No row slower by cycles; instructions identical on all rows (compare: strchr 32/32, 56/56; strchrnul 29/29, 53/53).

## Jobs (shiki)
Final head 8a5f4ed5: timing-cov-strscan-frontend-sse2-counts-sse2-2 rc 0 (compare + perf "after2"),
timing-cov-strscan-frontend-sse2-plants-2 rc 0 (8/8 CAUGHT, no WARN), -gate-2 and -counts-default-2 queued after them.
SUPERSEDED: -counts-before-sse2 (base, evidence), -counts-sse2 and -plants (head 551d76ac), -counts-default and
-gate (head 551d76ac, killed when the head moved).

## Not done / open
- Ivy Bridge and N100 timing NOT run: this tree's xtask has no `bench --stage` (the developer-flow timing recipe
  needs it), so bookings 84 (kotori) and 85 (nozomi) were cancelled. The periodic sweep (or a continuation once
  --stage is on next) times the seven rows.
- strchrnul_16_hit stays 1 cycle/call above glibc on Haswell with equal instructions and uops and front end not the
  cause by these counters (nd is 2x but changes in it did not move cycles). Next: perf record cycles on Liella only
  (the strchrnul row vs strchr row, same shape) and try the hit tail (bsf -> lea -> ret) vs strchr's; or the
  `pxor %xmm3` zero placement. Do not redo: rel8/stub placement, compare-chain form (both measured neutral).
- strncmp_diff8 (1.44 instr, 1.50 time here) is untouched and remains the largest strncmp gap.
