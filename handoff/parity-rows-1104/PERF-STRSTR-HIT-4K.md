# PERF-STRSTR-HIT-4K: strstr on a long haystack

Id: PERF-STRSTR-HIT-4K
Kind: code
Priority: 54
Size: S
Host: keke
Stacks-on: -
Model: sonnet
Effort: medium
BUDGET: 40 tool calls / 75k

## Rows (instr/op Liella vs glibc, run label parity-rows-1104-counts2/-counts3 on keke, 3 rounds; time = Liella/glibc dynamic ns ratio on the same run)

| row | instr Liella | instr glibc | ratio | time |
|---|---:|---:|---:|---:|
| strstr_hit_4k | 1278.2 | 826.1 | 1.55 | 1.32 |
| strstr_hit_64 (same function, short) | 168.5 | 157.6 | 1.07 | 0.67 |

## Hypothesis

Short haystacks are at parity, so the per-byte scan loop is the gap: about 450 extra instructions over 4 KiB, roughly 0.11 per byte. Probably the candidate-filter loop advances a vector at a time with extra compares/mask handling per block (first+last byte filter not unrolled, or verifying every first-byte hit with a full compare). Check the hit rate of the first-byte filter and process 2 blocks per iteration.
(From the instruction counts per call only; no perf annotate was run in this planning story. First step of the story: count with validation/wl_percall.sh and annotate to confirm before editing.)

## The change

Reduce instructions:u per call on the rows above to at most 1.05x glibc, with no row of the same bench getting slower, behaviour unchanged (every path meets the spec; use run-time dispatch for wider paths, test both). Plants for the new code; proof by --compare --counters before/after.

## Rules

Standing rules of rules/thread_common.txt apply. No lab host names outside handoff/.

JOBS: gate, plants, counts (before and after), percall
