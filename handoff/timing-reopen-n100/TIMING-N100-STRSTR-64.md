# TIMING-N100-STRSTR-64: strstr over a 64-byte haystack is slower in time on the N100

Kind: code
Model: opus
BUDGET: 70 calls / 140k

## Rows (sweep N100: instr / cycles / time)
cbench_memref/strstr_miss_64: 0.804 / 1.073 / 1.102 (43.3 vs 40.4 cycles); strstr_hit_64: 1.353 / 1.444 / 1.437 (36.5 vs 25.3). Ivy Bridge 1.411 / 2.398. The 4k rows are much faster (0.27 / 0.64).

## Read first
handoff/strstr-short-dense-parity.md, handoff/perf-strstr-hit-4k.md, handoff/strstr-filter-land.md on next (the short-haystack history), so no tried shape is redone.

## Profile evidence (per call, Liella / glibc)
- miss_64: topdown_be_bound.non_mem_scheduler 55 / 0 slots; alloc_restrictions 5 / 1; front end 2.9 / 21.4 (Liella saves front-end slots and loses more in the scheduler). Split loads 1 / 2.
- hit_64: non_mem_scheduler 22.9 / 4.9; alloc_restrictions 3 / 1; 134 / 99 instructions.
- Liella samples (miss_64): 53% liella_machine_strpair, 26% the strstr entry (Rust), the rest the bench loop and kernel entry.

## Hypothesis
For 64-byte haystacks the pair-filter kernel's dependent vector chain (compare, combine, mask extract, bit scan) fills the non-memory scheduler on Gracemont, and a quarter of the time is fixed entry work (needle setup) that a 64-byte haystack does not amortise. hit_64 also has 35 extra instructions per call.

## The change
Annotate both Liella symbols. Cut the strstr entry's fixed work for short haystacks (needle length and first-pair setup once, no repeated length scans) and shorten the dependent chain in strpair's first block. hit_64 must lose instructions (target <= glibc 99.3 per call); miss_64 must not gain any. Keep the 4k rows' advantage.

## Profile method (TIMING-REOPEN-N100)
Sweep stage of next f2157504 (the TIMING-SWEEP-1105 products) on the booked N100 timing host (Alder Lake-N, Gracemont E-cores), perf 6.8, user mode, pinned to one CPU. perf stat -r 5, seven named-event sets, Liella static bench vs glibc host-dynamic bench (the sweep pairing). Per call = (row run minus a setup-only run with a non-matching CBENCH_ROWS) / 84000 calls; this reproduces the sweep per-call cycles within 1% (memcpy_144 14.18/13.12 vs sweep 14.3/13.3; memchr_64k 3565/3137 vs 3567/3160). Topdown events count slots (5 per cycle). glibc: aggregate counters only. Liella: cycle samples by symbol; perf annotate produced nothing (the timing host has no objdump), so instruction-level attribution is step 1 of each brief: copy rec.<row>.data and the staged bench binary from ~/work/TIMING-REOPEN-N100/out on the N100 timing host to a build host and annotate there (Liella only). Full per-call table: handoff/timing-reopen-n100/profile-n100-percall.txt.

## JOBS
- gate: `cargo xtask clippy`, `cargo xtask static`, then the direct-tier affected gate with `--base origin/next` (label TIMING-N100-STRSTR-64-gate).
- plants: `cargo xtask plants` on the changed kernel, msg: regexes naming the expected failing test (label TIMING-N100-STRSTR-64-plants).
- counts: `cargo xtask bench --compare --counters cbench_memref` before and after on an AVX2 build host and the Ivy Bridge-class path (labels TIMING-N100-STRSTR-64-count-before / TIMING-N100-STRSTR-64-count-after); regression gate: no row of the bench gains instructions.
- timing: booked N100 check of the rows above, Liella vs glibc, before and after (`<N100 host>:TIMING-N100-STRSTR-64-n100`), and booked Ivy Bridge check of the same rows (`<IVB host>:TIMING-N100-STRSTR-64-ivb`): an AVX2-path change must not slow the SSE2 class. Book with `ops timing-book <host> <minutes> --story TIMING-N100-STRSTR-64`; cancel when done.
