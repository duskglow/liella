# PERF-MEMCMP-TAILS: parity rows (instructions:u), from the PARITY-ROWS-1106 round

Id: PERF-MEMCMP-TAILS
Kind: code
Priority: 56
Size: M
Host: -
Stacks-on: -
Model: sonnet
Effort: medium
BUDGET: 70 tool calls / 140k

## Rows

Counter run: label parity-rows-1106-counts on a Broadwell-EP (Xeon E5-2699 v4) counter host, `cargo xtask bench --compare --counters --runs 3`, origin/next 412d6fc7; time = Liella/glibc dynamic ns ratio of the same run; per-call profile: label parity-rows-1106-percall2 (validation/wl_percall.sh, instructions:u per call, deterministic, perf annotate not used). Raw: bench-cbench_opt.out is opt.out, others.out in this directory.

| row | instr ratio | time ratio | percall (Liella / glibc) |
|---|---:|---:|---|
| memcmp_diff8_127 / _96 / _64 | 1.26 / 1.27 / 1.15 | 1.44 / 1.31 / 1.00 | - |
| __memcmpeq_tail_32 | 1.40 | 0.92 | - |
| __memcmpeq_head_128 / _1k | 1.27 / 1.36 | 0.93 / 0.26 | bcmp_hd_1024 33 / 27 (1.22) |
| bcmp_tail_32 / bcmp_head_1k | 1.19 / 1.15 | 0.86 / 0.18 | - |
| memcmp_4k / memcmp_64k | 1.16 / 1.07 | 1.00 / 0.89 | - |

## Hypothesis (from the per-call profile)

The 1105 memcmp story landed: bcmp_eq_* rows from the percall spec memcmp_bands are now at or below glibc (eq_8/12/16 0.79, eq_48/64 0.90, eq_2048 0.99), so the equal-buffer paths are done. What is left is the early-difference and tail paths: the first differing byte found in the 64-127 byte band (memcmp_diff8_*, time 1.31-1.44, the only rows here that are also slow in time) and the head/tail forms of the equality-only entry points (bcmp_hd_1024 33 vs 27, bcmp_eq_256 72 vs 61, bcmp_eq_32 34 vs 31). Likely cause: after a vector compare that finds a mismatch Liella extracts the byte offset and re-loads the two bytes through a longer sequence than needed, and for 65-127 bytes it runs the loop where overlapping 32-byte loads from both ends would do. First step: percall with memcmp_bands plus a diff8_127 case, and read the instructions between the mismatch mask and the return.

## The change

Shorten the mismatch-locating tail of memcmp/bcmp/__memcmpeq for 33-127 byte and 256 byte sizes (overlapping vector loads from both ends, direct offset to the result), so the rows above are at most 1.05x glibc in instructions and memcmp_diff8_* at most 1.0x in time. Results unchanged (sign and zero only for memcmp: exhaustive test over every size 0-300 and every first-difference position, both dispatch paths). Behaviour unchanged (every path meets the spec; run-time dispatch for wider paths, test both). Plants for new or changed code; proof by `cargo xtask bench --compare --counters` before/after on the rows above, no row of the same bench slower.

## Rules

Standing rules of rules/thread_common.txt apply. No lab host names outside handoff/. Plants run through `cargo xtask plants` only.

JOBS: gate, plants (`cargo xtask plants`), counts (before and after), percall
