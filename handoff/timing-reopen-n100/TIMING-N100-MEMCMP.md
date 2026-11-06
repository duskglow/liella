# TIMING-N100-MEMCMP: memcmp 256 and 4k are slower in time on the N100 with fewer instructions

Kind: code
Model: opus
BUDGET: 70 calls / 140k

## Rows (sweep N100: instr / cycles / time)
cbench_memref/memcmp_256: 0.913 / 1.059 / 1.061 (18.2 vs 17.2 cycles); memcmp_4k: 0.962 / 1.087 / 1.086 (280 vs 258). Ivy Bridge: 1.226 (covered by TIMING-IVB-MEMREF-ENTRY) and 0.976.

## Profile evidence (per call, Liella / glibc)
- memcmp_4k: br_misp_retired.all_branches 1.00 / 0.00; topdown_bad_speculation 122 / 0.1 slots (about 24 cycles = the whole 22-cycle gap). Memory scheduler 310 / 345 (Liella better). 98% of Liella samples in liella_machine_compare.
- memcmp_256: topdown_be_bound.mem_scheduler 15.5 / 5.0 slots (about 2 cycles, the gap); alloc_restrictions 1.0 / 3.0; front end 0.6 / 1.2. No mispredict.

## Hypothesis
4k: one branch per call is mispredicted every call, most likely the exit of the long-block loop (a trip count this core's predictor does not learn) or a size-class branch taken differently from the loop's history. 256: the .Lcmp_256 block (PERF-MEMCMP-256-AVX2, fold.S) issues its loads faster than the load scheduler accepts them on Gracemont.

## The change
Annotate the Liella samples to find the mispredicted branch (perf record -e br_misp_retired.all_branches:u on Liella only). For 4k, restructure the loop exit so the per-call misprediction goes (shorter trip count via a wider body, or a counted exit the predictor follows); for 256, reorder the block so loads and compares interleave. Instruction count must not rise on the 256 row (glibc 57.2 per call on the sweep).

## Profile method (TIMING-REOPEN-N100)
Sweep stage of next f2157504 (the TIMING-SWEEP-1105 products) on the booked N100 timing host (Alder Lake-N, Gracemont E-cores), perf 6.8, user mode, pinned to one CPU. perf stat -r 5, seven named-event sets, Liella static bench vs glibc host-dynamic bench (the sweep pairing). Per call = (row run minus a setup-only run with a non-matching CBENCH_ROWS) / 84000 calls; this reproduces the sweep per-call cycles within 1% (memcpy_144 14.18/13.12 vs sweep 14.3/13.3; memchr_64k 3565/3137 vs 3567/3160). Topdown events count slots (5 per cycle). glibc: aggregate counters only. Liella: cycle samples by symbol; perf annotate produced nothing (the timing host has no objdump), so instruction-level attribution is step 1 of each brief: copy rec.<row>.data and the staged bench binary from ~/work/TIMING-REOPEN-N100/out on the N100 timing host to a build host and annotate there (Liella only). Full per-call table: handoff/timing-reopen-n100/profile-n100-percall.txt.

## JOBS
- gate: `cargo xtask clippy`, `cargo xtask static`, then the direct-tier affected gate with `--base origin/next` (label TIMING-N100-MEMCMP-gate).
- plants: `cargo xtask plants` on the changed kernel, msg: regexes naming the expected failing test (label TIMING-N100-MEMCMP-plants).
- counts: `cargo xtask bench --compare --counters cbench_memref` before and after on an AVX2 build host and the Ivy Bridge-class path (labels TIMING-N100-MEMCMP-count-before / TIMING-N100-MEMCMP-count-after); regression gate: no row of the bench gains instructions.
- timing: booked N100 check of the rows above, Liella vs glibc, before and after (`<N100 host>:TIMING-N100-MEMCMP-n100`), and booked Ivy Bridge check of the same rows (`<IVB host>:TIMING-N100-MEMCMP-ivb`): an AVX2-path change must not slow the SSE2 class. Book with `ops timing-book <host> <minutes> --story TIMING-N100-MEMCMP`; cancel when done.
