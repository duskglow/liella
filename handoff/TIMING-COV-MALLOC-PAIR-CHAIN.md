# TIMING-COV-MALLOC-PAIR-CHAIN: shift instead of imul for power-of-two classes (NEGATIVE, reverted)

State: branch task/timing-cov-malloc-pair-chain, base next eef6b2dd. Net change on the branch: one new test, its plants
file, decision row D-3.18, a NEGATIVE register row in docs/saturation/04-allocator.md. The allocator code is unchanged
(the change was built, measured slower and reverted in the branch's own commits).

## What was tried
`FREE_VIEW` gained a fourth word, `log2(size)` for a power-of-two class and 0 otherwise. Malloc's block offset
`i * CLASSES[c]` (alloc_small, alloc_group, alloc_hint) became `i << log2` for those classes, and free's
`(off * magic) >> 40` (free_hint, fit) became `off >> log2`, each behind a class-kind branch. The start check
`q * size == off && q < n` stayed the same for both kinds. Codegen (objdump of Liella's libc.so.6) showed real
branches, no `cmov`: free folds the kind test into one `bt` on a constant class mask. Each side gains
`movzbl` (the shift-table load), `bt`+`jcc`, a `mov` and a `shl/shr %cl`.

## Measurement (shiki = Haswell-EP E5-2697 v3, where the Ivy Bridge gap reproduces; job shiki:tcmpc-rc-shiki)
cbench_malloc with CBENCH_COUNTERS=1, `taskset -c 5`, base stage and new stage (both `cargo xtask bench --stage` on
keke) plus glibc dynamic, interleaved per round, default and hwcaps=-AVX2. 3 rounds default and 2 at -AVX2 were
taken, then the job was stopped because the verdict was clear. Medians, cycles per call (base / new / glibc):

| row | default | -AVX2 | instr base / new / glibc |
|---|---|---|---|
| malloc_free_16 | 47.18 / 48.62 / 38.39 | 47.12 / 48.62 / 39.38 | 122.1 / 131.1 / 147.1 |
| malloc_free_64 | 47.04 / 48.72 / 38.34 | 47.02 / 48.73 / 38.38 | 122.1 / 131.1 / 147.1 |
| malloc_free_1024 | 47.16 / 48.60 / 38.33 | 47.14 / 48.61 / 38.33 | 122.1 / 131.1 / 147.1 |
| realloc_within_usable | 64.80 / 65.23 / 51.21 | 64.74 / 66.23 / 51.27 | 194.1 / 207.1 / 195.1 |
| realloc_null_24 | 54.87 / 56.33 / 45.87 | 54.81 / 63.77 / 45.95 | 149.1 / 159.1 / 174.1 |
| heap_mix_real | 89.32 / 95.43 / 137.39 | 89.61 / 97.90 / 137.59 | 147.6 / 151.3 / 203.4 |

Every row is slower and no ranges overlap (the full ranges are in the register row). heap_mix_real_2t prints no counter line (threaded).
Why: the SSE2 build has no BMI2, so a shift by `cl` is 3 uops, and the shift-table load and the class test add 4 to 5
instructions per side. Together they cost more than the two 3-cycle `imul`s the change takes off the chain. The brief's
model (2 x 3-cycle imul to 1-cycle shift) did not count the variable-count shift's uops or the added loads.

Not done, and why: the Ivy Bridge and N100 timing and the coordinator's du / gawk-regex timing (ruling #3540).
The change was already slower on the development host and is reverted, so the branch has nothing to time. wl_count
(keke:tcmpc-wlc) exited 2 because the base was not primed on keke; with the code reverted it is moot.

## Tests and plants (final head)
New test `every_class_places_blocks_by_index_and_finds_them_again` (crates/rt/tests/heap_and_sync.rs; s3_1_c_1,
s3_1_c_3, s3_1_c_5; D-3.18, D-3.16). For every class it checks: block i at index times size, alignment, usable size,
interior addresses (ALIGN, size/2, size-ALIGN) refused, an in-place resize within the class, free once, double free
refused, block 0 reused. It passed on the shift code too (keke:tcmpc-gate, 1166 passed).
Plants (handoff/TIMING-COV-MALLOC-PAIR-CHAIN-plants.txt, keke:tcmpc-plants2): 3 CAUGHT, 0 SURVIVED, 0 WARN.
They are free_hint's start check dropped, locate_in's start check dropped, and alloc_hint's stride doubled. The brief's
shift-specific plants were written for the shift code, but that run was refused (the plants file was not on the role
yet: "cannot read handoff/...plants.txt"). The shift code was then reverted, so they were rewritten against the kept code.
Gate on the final code (keke:tcmpc-gate2): clippy, static, affected --base origin/next: 50 passed, 0 failed, peak 1.2G.

## Do not redo
The shift form for power-of-two classes, with a per-class shift table and a branch (D-3.18). A form with no per-class load
and no branch was not found: the classes are 1, 3, 5 or 7 times a power of two, so a uniform shift still needs a multiply.

## Next step
None on this row from the index computation. The chain's other parts (the forwarded bitmap-word and hint loads) are
the remaining levers, and the latest-block path that skipped them was refused twice (#2826, #3210).
Plan rows: D-3.16, D-3.17, D-3.18; register docs/saturation/04-allocator.md rows "cycles on Ivy Bridge" and the two NEGATIVE rows.

Files outside the repo: ~/w/.scratch/TIMING-COV-MALLOC-PAIR-CHAIN/ (rc.sh, summ.py, rc.log); shiki
~/work/TIMING-COV-MALLOC-PAIR-CHAIN/ (base and new stages); keke ~/work/TIMING-COV-MALLOC-PAIR-CHAIN/ (base.tar, new.tar).
