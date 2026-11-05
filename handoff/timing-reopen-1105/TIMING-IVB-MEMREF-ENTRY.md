# TIMING-IVB-MEMREF-ENTRY: small/mid memcmp, __memcmpeq, bcmp, memchr, memmove, memset on Ivy Bridge: a fixed per-call cycle cost

Id: TIMING-IVB-MEMREF-ENTRY
Kind: code
Priority: 75
Size: M
Host: -
Stacks-on: TIMING-IVB-STRSCAN-DISPATCH
Model: opus
Effort: high
BUDGET: 80 calls / 140k

## Rows (TIMING-SWEEP-1105, Ivy Bridge, cbench_memref; instr / cycles / time)
__memcmpeq_head_8 0.790 / 1.113 / 1.101; __memcmpeq_tail_8 0.783 / 1.197 / 1.221; bcmp_tail_128 0.960 / 1.179 / 1.173;
memchr_64 0.780 / 1.173 / 1.177; memcmp_128 0.910 / 1.094 / 1.103; memcmp_144 0.988 / 1.128 / 1.172;
memmove_144 0.831 / 1.168 / 1.167; memset_144 0.866 / 1.201 / 1.214; rawmemchr_4k 0.908 / 1.070 / 1.076.
N100: these rows are faster than glibc (other N100 memref rows are listed in the README for a later round).

## Profile evidence (TIMING-REOPEN-1105, booked Ivy Bridge host)
memref rows run 41 x 2000 calls per process, so whole-process perf stat is start-up dominated (not usable).
Region counters (CBENCH_COUNTERS=1, measured loop only, per call; instr / cycles / uops, Liella vs glibc):
memchr_64 42.7 / 21.2 / 40.8 vs 54.6 / 18.2 / 45.6 (+3.0 cycles); __memcmpeq_tail_8 18.7 / 12.1 / 17.8 vs
23.4 / 9.6 / 22.4 (+2.5); memmove_144 53.8 / 36.1 / 58.8 vs 64.7 / 31.4 / 59.9 (+4.7); memset_144 39.7 / 18.5 / 45.8 vs
45.7 / 15.4 / 41.6 (+3.1); memcmp_144 80.0 / 29.1 / 75.1 vs 80.6 / 25.0 / 70.6 (+4.1); bcmp_tail_128 71.8 / 29.0 / 67.9
vs 74.6 / 24.1 / 65.5 (+4.9). Branch misses and L1D misses ~0.01 per call on both sides.
A roughly constant 2.5-5 extra cycles per call at 8 to 144 bytes, with fewer instructions and uops, points at the
entry/exit (class dispatch, first-block setup), not at the loops. Stall attribution by event sampling inside the
function was attempted (two runs, labels timing-reopen-1105-prof2/-prof3) and produced no samples: NOT VERIFIED.

## Hypothesis
The same entry shape as TIMING-IVB-STRSCAN-DISPATCH (dispatch-mask load plus taken branches to the SSE2-class body),
costing front-end bubbles and load latency in front of the first compare. To be confirmed by step 1.

## The change
1. Attribute the stall: sample on the Ivy Bridge host with `perf record -e <event>:u -c <period>` over a loop of
   25 runs and count samples whose symbol is the Liella kernel (perf script -F event,ip,sym; check the output is
   non-empty before aggregating), or build an isolated per-row loop probe (as PERF-MALLOC-IVB-CYCLES did) and use perf stat.
2. Apply the dispatch fix from TIMING-IVB-STRSCAN-DISPATCH to these entries (stacked on it) or, if the evidence
   differs, the fix the profile names. AVX2 hosts must not get slower.

## JOBS
gate (clippy, static, affected --base origin/next); plants via `cargo xtask plants` (anchored .S plants, tests named
explicitly: liella-boundary small_copy / machine_scanners); instruction counts before/after (cbench_memref,
`--compare --counters`, AVX2 build host and `--tunables glibc.cpu.hwcaps=-AVX2`) as the regression gate; booked timing
check of these rows on the Ivy Bridge timing host (`ops timing-book nico <min> --story <ID>`) and the N100.
