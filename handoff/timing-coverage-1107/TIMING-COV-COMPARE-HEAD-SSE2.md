# TIMING-COV-COMPARE-HEAD-SSE2: memcmp/bcmp/__memcmpeq head and diff8 rows and strncmp_diff8 on the SSE2 class

Id: TIMING-COV-COMPARE-HEAD-SSE2
Kind: code
Priority: 64
Size: M
Host: -
Model: opus
Effort: high
BUDGET: 80 calls / 160k
Stacks-on: none

## The change
Step 1 (required, first): the booked re-time TIMING-COV-MEMCMP-NONAVX2-4 never ran (its handoff, "Not done" 1):
base 34c98f61 vs origin/next vs glibc on Ivy Bridge and the N100 forced SSE2 (GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2).
If every row below is <= 1.05, stop: COMPLETE with the table. Step 2: the lever that handoff names and did not
build: a separately dispatched SSE2 entry for memcmp/bcmp/__memcmpeq without the AVX2 carry gates of the shared
entry ladder (its profile: entry ladder 5 + jns 1 + cmp_long 2 + cmp_sse 2 + probe 6 + bsf/re-read/order 7, ~23 in
function vs glibc ~15; est. -8 instructions on every SSE2 row). Step 3: strncmp's diff8 path on SSE2.
Read D-4.62 (plan/07-decisions/s04) and D-4.35 first.

## Rows (1106E Ivy Bridge, instr / time; all faster or parity on the N100 except memcmp_256)
__memcmpeq_head_128/1k 1.282 / 1.538; bcmp_head_128/1k 1.08 / 1.462; memcmp_diff8_64/96/127 1.317-1.400 /
1.251-1.334; __memcmpeq_eq_128 0.955 / 1.263; bcmp_eq_128 0.926 / 1.260; bcmp_tail_128 0.826 / 1.101; memcmp_128
0.895 / 1.052; memcmp_256 1.010 / 1.162; __memcmpeq_eq_1k, bcmp_eq_1k 1.065-1.077 (spread 0.30-0.39);
cbench_hot/strncmp_diff8 1.436 / 1.417.

## Evidence (build host, -AVX2, region counters per call, counters-mei.txt; tree includes MEMCMP-NONAVX2-4)
__memcmpeq_head_128 25.4 vs 21.3 instructions, 14.0 vs 8.4 cycles; bcmp_head_128 25.3 vs 25.4, 12.5 vs 8.6;
memcmp_diff8_127 31.3 vs 25.3, 11.6 vs 9.4; memcmp_diff8_64 33.3 vs 25.5, 9.5 vs 9.9; __memcmpeq_eq_128 59.3 vs
64.3, 19.5 vs 20.6; memcmp_256 103.7 vs 106.5, 28.5 vs 28.9. strncmp_diff8 56 vs 39 instructions, 17.1 vs 12.2 cycles;
Liella samples: liella_machine_strncmp 75%. The memcmp group's sample count was too small to rank symbols
(`resolve_fold_slow` appeared: check whether a NULL-fold resolve runs inside the timed loop before blaming it).

## Hypothesis
Head rows (difference in the first 16 bytes) pay the shared ladder; equal-instruction bcmp_head at 1.45x cycles
points to a dependent chain (bsf -> re-read -> order) rather than count. Measure cycles per variant on the SSE2
class before choosing.

## Tests and plants
small_compare, the memcmp/bcmp/strncmp correctness sweeps (lengths 0-2048, every offset, NULL fold classes);
plants on the new entry (dispatch to the wrong entry, a dropped tail, a wrong sign) with msg: regexes.

## Rows to time
Every cbench_memref memcmp/bcmp/__memcmpeq row and cbench_hot strncmp_* on Ivy Bridge, the N100 default and the
N100 forced SSE2; no AVX2 row may get slower.
## JOBS
Gate: `cargo xtask clippy`, `cargo xtask static`, then the direct tier (`cargo xtask affected --run --base origin/next`)
through jobrun on your role. Plants: `cargo xtask plants <file>` on every changed function (msg: regex per plant).
Counts before/after on base and head (region counters or the probe, both sides printed). Timing: book an Ivy Bridge
and an N100 host (`ops timing-book`), stage once with `cargo xtask bench --stage`, run the rows below base vs head
vs glibc interleaved (>= 3 rounds), cancel the bookings when done. Keep only a change that makes no measured row slower.

First line: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:
