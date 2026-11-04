# PERF-MEMCMPEQ-EARLY: bcmp / __memcmpeq / memcmp early-difference and tail rows

Id: PERF-MEMCMPEQ-EARLY
Kind: code
Priority: 53
Size: M
Host: keke
Stacks-on: -
Model: sonnet
Effort: medium
BUDGET: 40 tool calls / 75k

## Rows (instr/op Liella vs glibc, run label parity-rows-1104-counts2/-counts3 on keke, 3 rounds; time = Liella/glibc dynamic ns ratio on the same run)

| row | instr Liella | instr glibc | ratio | time |
|---|---:|---:|---:|---:|
| __memcmpeq_tail_32 | 31.4 | 20.4 | 1.54 | 0.60 |
| __memcmpeq_head_1k | 30.4 | 20.3 | 1.50 | 1.30 |
| memcmp_diff8_64 | 34.4 | 24.4 | 1.41 | 1.36 |
| __memcmpeq_head_128 | 28.4 | 20.4 | 1.39 | 1.41 |
| memcmp_diff8_96 / _127 | 33.4 / 33.5 | 24.4 | 1.37 | 1.27 / 1.08 |
| bcmp_tail_32 | 31.4 | 24.4 | 1.29 | 0.64 |
| bcmp_head_1k | 30.4 | 24.4 | 1.25 | 1.18 |
| bcmp_head_128 | 28.5 | 24.4 | 1.17 | 1.09 |
| memcmp_4k | 671.3 | 576.0 | 1.17 | 1.07 |

## Hypothesis

A difference found in the first vector costs 8 to 10 instructions more than glibc: the entry does extra size dispatch/alignment work before the first compare, and the exit computes the result with more steps. Make the first-block compare the first thing after a single size test and branch straight to the result on a nonzero mask. COORDINATE: the open story for memcmp_256 touches the same file; stack on it or take its head as base (do not take memcmp_256 itself).
(From the instruction counts per call only; no perf annotate was run in this planning story. First step of the story: count with validation/wl_percall.sh and annotate to confirm before editing.)

## The change

Reduce instructions:u per call on the rows above to at most 1.05x glibc, with no row of the same bench getting slower, behaviour unchanged (every path meets the spec; use run-time dispatch for wider paths, test both). Plants for the new code; proof by --compare --counters before/after.

## Rules

Standing rules of rules/thread_common.txt apply. No lab host names outside handoff/.

JOBS: gate, plants, counts (before and after), percall
