# PERF-REALLOC-USABLE: realloc that stays within the usable size

Id: PERF-REALLOC-USABLE
Kind: code
Priority: 52
Size: S
Host: keke
Stacks-on: -
Model: sonnet
Effort: medium
BUDGET: 40 tool calls / 80k

## Rows (instr/op Liella vs glibc: run label parity-rows-1105-counts / -counts2 on keke, 3 rounds, origin/next d10a6adc; time = Liella/glibc dynamic ns ratio of the same run; per-call profile: label parity-rows-1105-percall / -percall2, spec validation/percall/parity1105.c, instructions:u per call, deterministic)

| row | instr Liella | instr glibc | ratio | time |
|---|---:|---:|---:|---:|
| realloc_within_usable (bench) | 216.2 | 195.1 | 1.11 | 1.33 |
| realloc_within_usable (percall: malloc 17, realloc to 24, store, free) | 203.0 | 188.0 | 1.08 | - |
| malloc_free_17 (percall control: malloc 17 + free) | 112.0 | 142.0 | 0.79 | - |

## Hypothesis (from the per-call profile; no perf annotate, sampling on the VM loses samples)

The control shows Liella malloc+free is 30 instructions *cheaper* than glibc for the same size, yet the realloc sequence is 15 dearer. So realloc itself costs about 91 instructions (203-112) in Liella against about 46 (188-142) in glibc: roughly 45 too many on the path where the request fits the existing usable size. Time is 1.33x, the worst time ratio in this round, so the path probably also has a dependent load chain or a branch-heavy size-class lookup that the instruction count understates. Target: the in-place fit check in realloc (usable-size lookup, class compare, early return) and any lock/tcache re-entry before it.
First step of the story: re-run validation/wl_percall.sh --spec validation/percall/parity1105.c (and, for the call tree, validation/wl_trace.sh) and confirm before editing.

## The change

Make realloc that fits in the existing chunk return after a short check (header read, class compare) without entering the general resize path, so realloc_within_usable is at most 1.05x glibc in instructions and at most 1.0x in time. Behaviour unchanged (every path meets the spec; run-time dispatch for wider paths, test both). Plants for new or changed code; proof by `cargo xtask bench --compare --counters` before/after on the rows above, no row of the same bench slower.

## Rules

Standing rules of rules/thread_common.txt apply. No lab host names outside handoff/.

JOBS: gate, plants, counts (before and after), percall
