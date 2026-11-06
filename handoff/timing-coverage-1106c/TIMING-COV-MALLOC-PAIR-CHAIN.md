# TIMING-COV-MALLOC-PAIR-CHAIN: shorten the carried malloc/free index chain on the SSE2 class

Id: TIMING-COV-MALLOC-PAIR-CHAIN
Kind: code
Priority: 70
Size: M
Host: -
Model: opus
Effort: high
BUDGET: 80 calls / 160k
Reserve: D-3

## The change

Cut the cycles of the malloc+free pair on Ivy Bridge without the latest-block fast path (refused twice, owner
#2826 and #3210; docs/saturation/04-allocator.md, read both NEGATIVE rows first). The register's cause row
(PERF-MALLOC-IVB-CYCLES) puts the gap on a chain carried from pair to pair through the bitmap word, with two `imul`s
on it: malloc's `i * CLASSES[c]` (crates/rt/src/heap.rs, alloc_hint / Block addr) and free's
`q = (off * magic) >> 40` (free fast path, `FREE_VIEW[c]`). The check `q * size == off` is a predicted branch, off the
chain.

## Rows (TIMING-SWEEP-1106, Ivy Bridge; instr / time)
malloc_free_16/32/64/128/256/1024 0.830 / 1.112-1.181; realloc_null_24 0.857 / 1.075; realloc_within_usable
0.995 / 1.183. N100: all faster (0.72-0.92).

## Evidence (handoff/timing-coverage-1106c/README.md section 3, Haswell-EP build host, hwcaps=-AVX2)
Region counters per call: malloc_free_* 122.1 instr / 47.1-51.3 cycles vs glibc 147.1 / 38.4-38.7;
realloc_within_usable 64.9 vs 51.3 cycles; realloc_null_24 54.7 vs 45.9. Aggregate resource_stalls.any 9x glibc's.
The gap reproduces on the Haswell-EP build host, so develop there with region counters
(`CBENCH_ROWS=<row> CBENCH_COUNTERS=1 GLIBC_TUNABLES=glibc.cpu.hwcaps=-AVX2 target/liella/bench/cbench_malloc`,
pinned) and book Ivy Bridge only for the final check.

## Hypothesis (NOT VERIFIED)
Every row above is a power-of-two class (16..1024; 24 rounds to 32). For those classes both multiplies can be
shifts (`i << log2`, `off >> log2` plus a low-bits test), taking about 4 cycles (2 x imul latency 3 vs shift 1) off
a chain of ~6-7 cycles of gap. Non-power-of-two classes keep the magic multiply. Ways to do it: a per-class shift
in FREE_VIEW (the descriptor word at heap.rs:148 and :1190 already packs class, magic and size: check its free bits
before using it) with a class-kind branch, or a branch-free form. Measure the branch on a mixed-class workload (heap_mix rows, cbench_malloc)
so the common case does not pay a mispredict.

## Steps
1. Read docs/saturation/04-allocator.md rows 5-7 and handoff/PERF-MALLOC-IVB-CYCLES.md; check the shift idea is not
   already a recorded negative (if it is, hand back DO NOT LAND with the row).
2. Prototype on the role host; region counters before/after for malloc_free_16/64/1024, realloc rows, heap_mix_real_2t,
   at default and hwcaps=-AVX2. Keep only if no row is slower.
3. D-3 row for the index computation (reserved). Saturation register row either way.

## Tests / plants
Spec clause §3 malloc/free/realloc (alignment, usable size, reuse). Plants via `cargo xtask plants`: shift off by
one for one class; low-bits test dropped (interior pointer freed as a block start must still be rejected by the
existing checks); non-power-of-two class sent down the shift path. msg:<regex> naming the failing test.

## JOBS
gate (clippy, static, affected --base origin/next) on the role; plants; counts `bench --compare --counters
cbench_malloc` default and hwcaps=-AVX2; booked timing check of the eight rows on an Ivy Bridge timing host
(`ops timing-book <host> <min> --story TIMING-COV-MALLOC-PAIR-CHAIN`) and the N100 (must stay faster). Name timing
jobs `<host>:<label>`.

First line: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:
