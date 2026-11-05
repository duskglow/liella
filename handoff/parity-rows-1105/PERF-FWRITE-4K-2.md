# PERF-FWRITE-4K-2: fwrite of a 4096-byte block

Id: PERF-FWRITE-4K-2
Kind: code
Priority: 53
Size: S
Host: keke
Stacks-on: -
Model: sonnet
Effort: medium
BUDGET: 40 tool calls / 80k

## Rows (instr/op Liella vs glibc: run label parity-rows-1105-counts / -counts2 on keke, 3 rounds, origin/next d10a6adc; time = Liella/glibc dynamic ns ratio of the same run; per-call profile: label parity-rows-1105-percall / -percall2, spec validation/percall/parity1105.c, instructions:u per call, deterministic)

| row | instr Liella | instr glibc | ratio | time |
|---|---:|---:|---:|---:|
| fwrite_4k (bench) | 1193.1 | 1042.8 | 1.14 | 1.15 |
| fwrite_4k (percall, 4096 B into an 8192 B full buffer on /dev/null) | 815.0 | 644.0 | 1.27 | - |
| memcpy_4k_ref (percall control) | 440.0 | 425.0 | 1.04 | - |

## Hypothesis (from the per-call profile; no perf annotate, sampling on the VM loses samples)

The memcpy control is within 15 instructions, so the copy is not the gap. About 170 instructions per call (815-644, minus the 15) sit in the stream bookkeeping around it: lock/unlock of the stream, buffer-space arithmetic, and the flush decision when the 4 KiB block fills half the buffer. At the bench the gap is 150. Look for a repeated recomputation of the free space or two passes (flush, then copy) where one suffices; validation/percall/fwrite4k.c is the earlier spec for the same call (PERF-FWRITE-4K) and may already hold part of the answer: read its history first.
First step of the story: re-run validation/wl_percall.sh --spec validation/percall/parity1105.c (and, for the call tree, validation/wl_trace.sh) and confirm before editing.

## The change

Cut the fwrite bookkeeping for a block that is larger than a small threshold but smaller than the buffer, so fwrite_4k is at most 1.05x glibc in instructions and not slower in time. Behaviour unchanged (every path meets the spec; run-time dispatch for wider paths, test both). Plants for new or changed code; proof by `cargo xtask bench --compare --counters` before/after on the rows above, no row of the same bench slower.

## Rules

Standing rules of rules/thread_common.txt apply. No lab host names outside handoff/.

JOBS: gate, plants, counts (before and after), percall
