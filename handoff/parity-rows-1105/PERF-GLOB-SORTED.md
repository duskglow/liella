# PERF-GLOB-SORTED: glob with sorted results

Id: PERF-GLOB-SORTED
Kind: code
Priority: 57
Size: S
Host: keke
Stacks-on: -
Model: sonnet
Effort: medium
BUDGET: 40 tool calls / 80k

## Rows (instr/op Liella vs glibc: run label parity-rows-1105-counts / -counts2 on keke, 3 rounds, origin/next d10a6adc; time = Liella/glibc dynamic ns ratio of the same run; per-call profile: label parity-rows-1105-percall / -percall2, spec validation/percall/parity1105.c, instructions:u per call, deterministic)

| row | instr Liella | instr glibc | ratio | time |
|---|---:|---:|---:|---:|
| glob_1000_sorted (bench) | 3668776.5 | 3443234.3 | 1.07 | 0.87 |
| glob_sorted_38 (percall, dir/*, C locale) | 85734.8 | 68437.0 | 1.25 | - |

## Hypothesis (from the per-call profile; no perf annotate, sampling on the VM loses samples)

Per match Liella is about 430 instructions over (17300 over about 40 matches in the percall; 225000 over 1000 in the bench, 225 per match): a per-entry cost plus the sort. Time is already 0.87, so the instructions are probably a different sort/compare algorithm (more compares, fewer cache misses). Profile compare count versus per-entry string copy before touching it; do not make the time worse. Note the overlap with PERF-NFTW-ENTRY if the shared piece is readdir/path building.
First step of the story: re-run validation/wl_percall.sh --spec validation/percall/parity1105.c (and, for the call tree, validation/wl_trace.sh) and confirm before editing.

## The change

Reduce the per-match instruction cost of glob with sorting (strcoll in the C locale is a byte compare) to at most 1.05x glibc in instructions, keeping time at or below the current 0.87. Behaviour unchanged (every path meets the spec; run-time dispatch for wider paths, test both). Plants for new or changed code; proof by `cargo xtask bench --compare --counters` before/after on the rows above, no row of the same bench slower.

## Rules

Standing rules of rules/thread_common.txt apply. No lab host names outside handoff/.

JOBS: gate, plants, counts (before and after), percall
