# TIMING-COV-STRSCAN-SHORT-IVB: short strchr/strchrnul/memrchr/memchr on Ivy Bridge at equal instructions

Id: TIMING-COV-STRSCAN-SHORT-IVB
Kind: code
Priority: 62
Size: M
Host: -
Model: opus
Effort: high
BUDGET: 80 calls / 160k
Stacks-on: none

## The change
Find and remove the Ivy Bridge cycle gap of the short scan kernels. Instruction counts equal glibc's (strchr,
strchrnul) or are lower (memchr_64, rawmemchr_4k): the cost is per instruction. TIMING-IVB-STRSCAN-DISPATCH-2 and
TIMING-COV-STRSCAN-FRONTEND-SSE2 are in the 1106E build and left these rows unchanged (strchr_16_hit 1.116 in both
sweeps): read their handoffs and docs/saturation/ (string kernels file) first and do not repeat their levers.

## Rows (1106E Ivy Bridge, instr / time; all faster on the N100)
strchr_16_hit 1.000 / 1.116; strchr_64_hit 1.000 / 1.135; strchr_64_miss 1.000 / 1.289; strchrnul_16_hit 1.000 /
1.110; strchrnul_64_hit 1.000 / 1.269; strchrnul_64_miss 1.000 / 1.194; memrchr_64_hit 1.107 / 1.277; memchr_16
1.001 / 1.251; memchr_64 0.780 / 1.117; rawmemchr_4k 0.908 / 1.078.

## Evidence (build host Skylake-SP, -AVX2, counters-mei.txt)
strchr_64_miss 56 vs 56 instructions, 15.3 vs 13.7 cycles; strchrnul_64_hit 53/53, 14.9 vs 13.2; strchr_16_hit
32/32, 8.2 vs 8.1; memrchr_64_hit 31 vs 28, 12.2 vs 9.2. memchr_16 1.045, memchr_64 0.785 and rawmemchr_4k 0.986
cycles do NOT reproduce off Ivy Bridge: the memchr part needs Ivy Bridge counters. Samples: strchr 40%, strchrnul
24%, memrchr 15% (+ memrchr_entry 1.4%).

## Hypothesis (needs Ivy Bridge counters, Liella side, on a booked timing host: perf stat only, no build there)
Equal instruction counts with more cycles on Ivy Bridge point at decode/uop-cache or port pressure in the
first-vector path (pcmpeqb/pmovmskb/bsf chain, or a taken-branch layout across a 32-byte boundary). Measure
idq/dsb and resource-stall events on Ivy Bridge for base vs a candidate; glibc: aggregate counters only.

## Tests and plants
machine_scanners / small scan suites (name the tests explicitly: the affected tier selects nothing for
csys/src/machine/*.S); .S plants anchored to their function label.

## Rows to time
cbench_hot strchr*/strchrnul*/memrchr*/strrchr*, cbench_memref memchr*/rawmemchr*, Ivy Bridge and the N100.
## JOBS
Gate: `cargo xtask clippy`, `cargo xtask static`, then the direct tier (`cargo xtask affected --run --base origin/next`)
through jobrun on your role. Plants: `cargo xtask plants <file>` on every changed function (msg: regex per plant).
Counts before/after on base and head (region counters or the probe, both sides printed). Timing: book an Ivy Bridge
and an N100 host (`ops timing-book`), stage once with `cargo xtask bench --stage`, run the rows below base vs head
vs glibc interleaved (>= 3 rounds), cancel the bookings when done. Keep only a change that makes no measured row slower.

First line: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:
