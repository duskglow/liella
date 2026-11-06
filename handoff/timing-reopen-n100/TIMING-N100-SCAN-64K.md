# TIMING-N100-SCAN-64K: memchr / rawmemchr over 64 KiB are L2-latency bound on the N100

Kind: code
Model: opus
BUDGET: 70 calls / 140k

## Rows (sweep N100: instr / cycles / time)
cbench_memref/memchr_64k: 0.922 / 1.129 / 1.169 (3567 vs 3160 cycles); rawmemchr_64k: 0.909 / 1.058 / 1.060. Control memchr_4k (fits L1): 0.906 / 1.040 / 1.036. Ivy Bridge 1.006 / 1.012.

## Profile evidence (per call, Liella / glibc)
- memchr_64k: mem_bound_stalls.load_l2_hit 486 / 198 cycles (+288, two thirds of the gap); topdown_be_bound.non_mem_scheduler 7414 / 5241 slots; mem_scheduler_block.ld_buf 14 / 65 (Liella fills the load buffer far less: fewer loads in flight); one mispredict per call both sides; no split loads. 99% of samples in liella_machine_memchr_entry.
- rawmemchr_64k: load_l2_hit stalls 383 / 213; ld_buf 17 / 77.

## Hypothesis
64 KiB does not fit the 32 KiB L1D, so each call streams from L2. Liella keeps fewer L2 loads outstanding (lower memory-level parallelism: loads per iteration too few, or the next block's loads wait on the previous block's test), so L2 latency is exposed. The 4k row (L1 resident) is near parity, which fits.

## The change
Annotate the Liella loop. On the AVX2 long-buffer loop, raise the loads in flight: a wider body (e.g. four 32-byte loads OR-reduced before one test) or loads issued before the previous test resolves; measure a software prefetch distance only as a second option. Same for the rawmemchr long loop (scan.S). Instruction count must not rise past glibc on the 4k and 64k rows; the short rows (PARITY-SCAN-AVX2) must not move.

## Profile method (TIMING-REOPEN-N100)
Sweep stage of next f2157504 (the TIMING-SWEEP-1105 products) on the booked N100 timing host (Alder Lake-N, Gracemont E-cores), perf 6.8, user mode, pinned to one CPU. perf stat -r 5, seven named-event sets, Liella static bench vs glibc host-dynamic bench (the sweep pairing). Per call = (row run minus a setup-only run with a non-matching CBENCH_ROWS) / 84000 calls; this reproduces the sweep per-call cycles within 1% (memcpy_144 14.18/13.12 vs sweep 14.3/13.3; memchr_64k 3565/3137 vs 3567/3160). Topdown events count slots (5 per cycle). glibc: aggregate counters only. Liella: cycle samples by symbol; perf annotate produced nothing (the timing host has no objdump), so instruction-level attribution is step 1 of each brief: copy rec.<row>.data and the staged bench binary from ~/work/TIMING-REOPEN-N100/out on the N100 timing host to a build host and annotate there (Liella only). Full per-call table: handoff/timing-reopen-n100/profile-n100-percall.txt.

## JOBS
- gate: `cargo xtask clippy`, `cargo xtask static`, then the direct-tier affected gate with `--base origin/next` (label TIMING-N100-SCAN-64K-gate).
- plants: `cargo xtask plants` on the changed kernel, msg: regexes naming the expected failing test (label TIMING-N100-SCAN-64K-plants).
- counts: `cargo xtask bench --compare --counters cbench_memref` before and after on an AVX2 build host and the Ivy Bridge-class path (labels TIMING-N100-SCAN-64K-count-before / TIMING-N100-SCAN-64K-count-after); regression gate: no row of the bench gains instructions.
- timing: booked N100 check of the rows above, Liella vs glibc, before and after (`<N100 host>:TIMING-N100-SCAN-64K-n100`), and booked Ivy Bridge check of the same rows (`<IVB host>:TIMING-N100-SCAN-64K-ivb`): an AVX2-path change must not slow the SSE2 class. Book with `ops timing-book <host> <minutes> --story TIMING-N100-SCAN-64K`; cancel when done.
