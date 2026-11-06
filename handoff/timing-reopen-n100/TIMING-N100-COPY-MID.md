# TIMING-N100-COPY-MID: memcpy 129..256 bytes is slower in time on the N100 at equal instructions

Kind: code
Model: opus
BUDGET: 70 calls / 140k

## Rows (sweep N100: instr / cycles / time)
cbench_memref/memcpy_144: 1.000 / 1.076 / 1.077 (14.3 vs 13.3 cycles); memcpy_256: 1.001 / 1.077 / 1.077. Control memcpy_128: 0.932 / 0.752 / 0.752 (Liella faster). Ivy Bridge: 1.010 and 0.730.

## Read first
PARITY-COPY-AVX2-3 landed on next AFTER the sweep base (handoff/PARITY-COPY-AVX2.md). Step 0: re-time these rows on next HEAD on the N100; if they are already <= 1.05, close this story with that measurement.

## Profile evidence (per call, Liella / glibc)
- cycles 14.18 / 13.12; instructions 38.13 / 38.10.
- topdown_be_bound.non_mem_scheduler 4.97 / 0.00 slots: about 1.0 cycle, the whole gap. Memory scheduler (5.07 / 5.12), front end (5.17 / 5.15) and alloc restrictions equal. No split loads, no mispredicts.
- Liella samples: 74% liella_machine_memmove.

## Hypothesis
The 129..256 class issues its vector work faster than the non-memory scheduler drains it on Gracemont (256-bit operations are executed as two 128-bit halves there; check this with the counters, do not assume): the reservation stations for the vector/ALU ports fill once per call. Same instruction count, different port pressure; the 128-byte path does not show it.

## The change
Annotate the 129..256 path of liella_machine_memmove (Liella samples). Try, on the AVX2 path only, shapes that lower non-memory-scheduler pressure at the same instruction count (e.g. interleave loads and stores so stores drain earlier, or 128-bit operations for the middle of the class on this core class if the dispatch can select it without a new runtime test). Keep the instruction count (the vector-vs-vector rule: rows are gated on instructions, verdict from timing; ruling #3046) and keep only a shape that is not slower on any measured AVX2 host.

## Profile method (TIMING-REOPEN-N100)
Sweep stage of next f2157504 (the TIMING-SWEEP-1105 products) on the booked N100 timing host (Alder Lake-N, Gracemont E-cores), perf 6.8, user mode, pinned to one CPU. perf stat -r 5, seven named-event sets, Liella static bench vs glibc host-dynamic bench (the sweep pairing). Per call = (row run minus a setup-only run with a non-matching CBENCH_ROWS) / 84000 calls; this reproduces the sweep per-call cycles within 1% (memcpy_144 14.18/13.12 vs sweep 14.3/13.3; memchr_64k 3565/3137 vs 3567/3160). Topdown events count slots (5 per cycle). glibc: aggregate counters only. Liella: cycle samples by symbol; perf annotate produced nothing (the timing host has no objdump), so instruction-level attribution is step 1 of each brief: copy rec.<row>.data and the staged bench binary from ~/work/TIMING-REOPEN-N100/out on the N100 timing host to a build host and annotate there (Liella only). Full per-call table: handoff/timing-reopen-n100/profile-n100-percall.txt.

## JOBS
- gate: `cargo xtask clippy`, `cargo xtask static`, then the direct-tier affected gate with `--base origin/next` (label TIMING-N100-COPY-MID-gate).
- plants: `cargo xtask plants` on the changed kernel, msg: regexes naming the expected failing test (label TIMING-N100-COPY-MID-plants).
- counts: `cargo xtask bench --compare --counters cbench_memref` before and after on an AVX2 build host and the Ivy Bridge-class path (labels TIMING-N100-COPY-MID-count-before / TIMING-N100-COPY-MID-count-after); regression gate: no row of the bench gains instructions.
- timing: booked N100 check of the rows above, Liella vs glibc, before and after (`<N100 host>:TIMING-N100-COPY-MID-n100`), and booked Ivy Bridge check of the same rows (`<IVB host>:TIMING-N100-COPY-MID-ivb`): an AVX2-path change must not slow the SSE2 class. Book with `ops timing-book <host> <minutes> --story TIMING-N100-COPY-MID`; cancel when done.
