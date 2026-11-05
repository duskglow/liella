# PERF-MEMSET-4K-3: parity rows (instructions:u), from the PARITY-ROWS-1106 round

Id: PERF-MEMSET-4K-3
Kind: code
Priority: 58
Size: S
Host: -
Stacks-on: -
Model: sonnet
Effort: medium
BUDGET: 70 tool calls / 140k

## Rows

Counter run: label parity-rows-1106-counts on a Broadwell-EP (Xeon E5-2699 v4) counter host, `cargo xtask bench --compare --counters --runs 3`, origin/next 412d6fc7; time = Liella/glibc dynamic ns ratio of the same run; per-call profile: label parity-rows-1106-percall2 (validation/wl_percall.sh, instructions:u per call, deterministic, perf annotate not used). Raw: bench-cbench_opt.out is opt.out, others.out in this directory.

| row | instr ratio | time ratio | percall (Liella / glibc) |
|---|---:|---:|---|
| memset_4k | 2.01 | 1.12 | 65 / 33 (1.97) |
| memset_64k | 1.64 | 1.02 | - |

## Hypothesis (from the per-call profile)

The 1105 story (PERF-MEMSET-4K and -2, tail jump to the large fill kernel above 256 bytes) landed; on this host the row is still 2x glibc in instructions (65 vs 33) and 1.12 in time. A 4 KiB fill is a handful of vector stores or one rep-stos, so 32 extra instructions are fixed dispatch and loop setup, not per-byte work. glibc's count of 33 means it takes a single short path for this size on this CPU class (Broadwell-EP, AVX2 and ERMS but no FSRM); Liella's path is likely the AVX2 loop with its head/tail stores. Open point to settle from the profile: whether the best path here is rep stosb (instruction count near 1, time decided by startup cost) or a four-store unrolled AVX2 loop. Counters decide: pick the lower instruction count that does not lose time on both a Broadwell and a Zen host. First step: percall --cases memset_4k with the dispatch level printed, and the same on a Zen host to see whether the row is host-specific (the 1105 round measured 1.13 on another host).

## The change

Make memset of 1 KiB to 64 KiB use the cheapest per-size path for the CPU class (run-time dispatch through the existing mechanism, both paths tested and bit-identical), so memset_4k and memset_64k are at most 1.05x glibc in instructions and at most 1.0x in time on both host classes. If the profile shows the remaining gap is below one cache line of setup, record it as accepted in the saturation register with the numbers. Behaviour unchanged (every path meets the spec; run-time dispatch for wider paths, test both). Plants for new or changed code; proof by `cargo xtask bench --compare --counters` before/after on the rows above, no row of the same bench slower.

## Rules

Standing rules of rules/thread_common.txt apply. No lab host names outside handoff/. Plants run through `cargo xtask plants` only.

JOBS: gate, plants (`cargo xtask plants`), counts (before and after), percall
