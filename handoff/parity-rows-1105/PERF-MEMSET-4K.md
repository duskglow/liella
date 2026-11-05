# PERF-MEMSET-4K: memset 4 KiB aligned path

Id: PERF-MEMSET-4K
Kind: code
Priority: 56
Size: S
Host: keke
Stacks-on: -
Model: sonnet
Effort: medium
BUDGET: 40 tool calls / 80k

## Rows (instr/op Liella vs glibc: run label parity-rows-1105-counts / -counts2 on keke, 3 rounds, origin/next d10a6adc; time = Liella/glibc dynamic ns ratio of the same run; per-call profile: label parity-rows-1105-percall / -percall2, spec validation/percall/parity1105.c, instructions:u per call, deterministic)

| row | instr Liella | instr glibc | ratio | time |
|---|---:|---:|---:|---:|
| memset_4k (bench) | 278.1 | 246.0 | 1.13 | 1.03 |
| memset_4k (percall, call through a volatile pointer, aligned buffer) | 285.0 | 250.0 | 1.14 | - |
| memset_4k_off (bench, unaligned) | 13.2 | 13.3 | 0.99 | 1.00 |

## Hypothesis (from the per-call profile; no perf annotate, sampling on the VM loses samples)

A fixed gap of 35 instructions on the aligned 4 KiB call only; the unaligned row is at parity. That points at the aligned large-size path (the loop or rep stosb setup, the tail store, and a size-threshold test), 35 over a body that is ~250 in both. If the Liella path is rep stosb, instruction counts are not comparable to a vector loop (the brief for this round excludes the rep movsb copy rows); in that case check the 35 are not pure setup (alignment head, length rounding) and drop the row as not worth a story if time is at parity (it is 1.03). Decide from the percall plus wl_trace first.
First step of the story: re-run validation/wl_percall.sh --spec validation/percall/parity1105.c (and, for the call tree, validation/wl_trace.sh) and confirm before editing.

## The change

If the 35 instructions are removable setup in the aligned path, remove them, otherwise record memset_4k as accepted in the saturation register with the profile. At most 1.05x glibc in instructions. Behaviour unchanged (every path meets the spec; run-time dispatch for wider paths, test both). Plants for new or changed code; proof by `cargo xtask bench --compare --counters` before/after on the rows above, no row of the same bench slower.

## Rules

Standing rules of rules/thread_common.txt apply. No lab host names outside handoff/.

JOBS: gate, plants, counts (before and after), percall
