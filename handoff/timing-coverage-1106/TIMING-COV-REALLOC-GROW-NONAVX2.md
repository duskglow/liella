# TIMING-COV-REALLOC-GROW-NONAVX2: realloc that grows by doubling (16 B -> 4 KiB)

Id: TIMING-COV-REALLOC-GROW-NONAVX2
Kind: code
Priority: 55
Size: M
Host: -
Stacks-on: TIMING-IVB-MALLOC-CHAIN-2
Model: opus
Effort: high
BUDGET: 70 calls / 140k

## The change
Cut realloc's growing path (crates/boundary/src/posix/s03_memory.rs, realloc_general, and the realloc entry): the moving
case (new block, copy, free) and the grow-in-place case. Stack on TIMING-IVB-MALLOC-CHAIN-2 (queued_ci, same allocator
paths); PERF-REALLOC-USABLE-2/-3 (red) worked the stay-within-usable case: read their handoffs, do not redo it.

## Rows (TIMING-SWEEP-1105)
realloc_grow_x2: Ivy Bridge instr 1.107 / time 1.093; N100 0.982 / 0.962.

## Evidence (keke, next 730f416b, job timing-coverage-1106-percall; spec handoff/timing-coverage-1106/tc1106.c)
Per realloc (malloc + 8 reallocs + free per iteration, per = 8): Liella 287.8 (non-AVX2) / 252.9 (AVX2), glibc 199.5.
One traced realloc (non-AVX2, call 201): 201 instructions = realloc_general 130, realloc entry 59, memmove 12.
NOT VERIFIED: the probe ratio (1.44 / 1.27) is above the sweep's (1.11 IVB, 0.98 N100); the bench row's per-call base
includes the loop and the 16 B malloc; step 1 reconciles the probe with the row before any change.
Workloads: realloc 3.6M calls in 25 of 30 workloads.

## Hypothesis
59 instructions in the realloc entry before realloc_general is large for a path that always moves or grows: size-class
lookups done twice (old usable size, then the new class) and a general path taken for small classes.

## Steps
1. Reconcile: run the bench row's own shape (bench/c/cbench_malloc.c grow_x2) through wl_percall on both paths; trace
   realloc at sizes 32, 512 and 4096 (Liella only).
2. Remove the repeated class lookup; keep the in-place rules of PERF-REALLOC-USABLE-3.
3. Tests: realloc grow / shrink over every size class boundary, content preserved, malloc_usable_size invariants.
4. Plants: copy one byte short, free the old block before copying, grow in place past the usable size.

## JOBS
gate, plants, counts before / after (tc1106.c --cases realloc_grow_x2, both dispatch settings), timing
nico:timing-cov-realloc-grow-time (`ops timing-book nico <min> --story TIMING-COV-REALLOC-GROW-NONAVX2`), and the
cbench_malloc rows not slower on nozomi:timing-cov-realloc-grow-n100.

First line: COMPLETE:, PENDING JOBS:, INCOMPLETE: or DO NOT LAND:
