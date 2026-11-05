# PERF-FNMATCH-BRACKET: fnmatch bracket expressions in single-byte locales

Id: PERF-FNMATCH-BRACKET
Kind: code
Priority: 55
Size: S
Host: keke
Stacks-on: -
Model: sonnet
Effort: medium
BUDGET: 40 tool calls / 80k

## Rows (instr/op Liella vs glibc: run label parity-rows-1105-counts / -counts2 on keke, 3 rounds, origin/next d10a6adc; time = Liella/glibc dynamic ns ratio of the same run; per-call profile: label parity-rows-1105-percall / -percall2, spec validation/percall/parity1105.c, instructions:u per call, deterministic)

| row | instr Liella | instr glibc | ratio | time |
|---|---:|---:|---:|---:|
| fnmatch_bracket (bench, C locale) | 953.0 | 888.8 | 1.07 | 1.09 |
| fnmatch_bracket (percall, C locale) | 999.0 | 891.0 | 1.12 | - |
| fnmatch_bracket (percall, en_US.UTF-8; for the record) | 1808.0 | 2231.2 | 0.81 | - |

## Hypothesis (from the per-call profile; no perf annotate, sampling on the VM loses samples)

In the C locale the pattern [a-m]*_[!0-9]*.[ch] costs 108 instructions more per call. The UTF-8 run shows Liella faster there, so the shared matcher is fine and the excess is in the single-byte path: most likely per-bracket-expression work (range test with class/collation checks that the C locale does not need) and the backtracking of the two stars re-scanning the same bytes. About 108 over 3 brackets and 2 stars is ~20 per element. Profile which of bracket parse and star backtrack dominates before choosing.
First step of the story: re-run validation/wl_percall.sh --spec validation/percall/parity1105.c (and, for the call tree, validation/wl_trace.sh) and confirm before editing.

## The change

In the single-byte locale path, test a bracket expression with a direct byte-range/negation loop (no collation lookup) and avoid re-parsing the pattern on star backtrack, so fnmatch_bracket is at most 1.05x glibc in instructions and 1.0x in time; the UTF-8 path stays as it is. Behaviour unchanged (every path meets the spec; run-time dispatch for wider paths, test both). Plants for new or changed code; proof by `cargo xtask bench --compare --counters` before/after on the rows above, no row of the same bench slower.

## Rules

Standing rules of rules/thread_common.txt apply. No lab host names outside handoff/.

JOBS: gate, plants, counts (before and after), percall
