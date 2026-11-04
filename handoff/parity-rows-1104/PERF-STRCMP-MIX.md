# PERF-STRCMP-MIX: strcmp short and mixed rows

Id: PERF-STRCMP-MIX
Kind: code
Priority: 51
Size: S
Host: keke
Stacks-on: -
Model: sonnet
Effort: medium
BUDGET: 40 tool calls / 75k

## Rows (instr/op Liella vs glibc, run label parity-rows-1104-counts2/-counts3 on keke, 3 rounds; time = Liella/glibc dynamic ns ratio on the same run)

| row | instr Liella | instr glibc | ratio | time |
|---|---:|---:|---:|---:|
| strcmp_16_mix | 53.3 | 45.8 | 1.16 | 0.94 |
| strcmp_64_mix | 71.9 | 61.9 | 1.16 | 0.76 |
| strcmp_equal_16 | 32.0 | 28.0 | 1.14 | 1.00 |
| strcmp_names | 67.9 | 63.1 | 1.08 | 1.21 |

## Hypothesis

Ratios are about 4 to 10 instructions per call over. Times are at parity or better except strcmp_names (1.21), so the target is the entry/exit and page-cross check (one extra mask or compare per call) and, for strcmp_names, branch behaviour on the first differing byte. Time must not regress on the _mix rows, which are already faster than glibc.
(From the instruction counts per call only; no perf annotate was run in this planning story. First step of the story: count with validation/wl_percall.sh and annotate to confirm before editing.)

## The change

Reduce instructions:u per call on the rows above to at most 1.05x glibc, with no row of the same bench getting slower, behaviour unchanged (every path meets the spec; use run-time dispatch for wider paths, test both). Plants for the new code; proof by --compare --counters before/after.

## Rules

Standing rules of rules/thread_common.txt apply. No lab host names outside handoff/.

JOBS: gate, plants, counts (before and after), percall
