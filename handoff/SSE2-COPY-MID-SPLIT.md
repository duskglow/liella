# SSE2-COPY-MID-SPLIT (handoff): SSE2 129-192 copy split, DROPPED (record only)

State: branch task/sse2-copy-mid-split-2 (SSE2-COPY-MID-SPLIT-2), origin/next merged in, the code reverted: fold.S,
crates/boundary/tests/small_copy.rs and bench/c/cbench_memref.c equal origin/next, so the product equals next. What lands
is the record: D-4.61 marked REJECTED (plan/07-decisions/s04-byte-arrays-and-strings.md) and a NEGATIVE row in
docs/saturation/03-memory-and-string-kernels.md (row after TIMING-IVB-MEMREF-ENTRY-2's fill-half NEGATIVE, whose flag now
points to it). Role keke:~/w/n2. Full report: handoff/SSE2-COPY-MID-SPLIT-2-report.md.

## Decisions in force
- Ruling #3608 (coordinator, 11-06): the split is dropped; memcpy_256 slower on both timing classes in both layouts
  fails #3421's / #3587's condition (the _256 rows must hold). D-4.61 REJECTED. D-4.55 (folded class gate, eight
  16-byte blocks per end for all of 129-256 on SSE2) stays as on next.

## Measurements (SSE2-COPY-MID-SPLIT, kept for the record; head / next paired ratio, whole cbench_memref, 4 x 5)
| row | Ivy L1 | Ivy L2 | N100 -AVX2 L1 | N100 -AVX2 L2 |
|---|---|---|---|---|
| memmove_144 / _192 / _256 | 0.828 / 0.853 / 1.000 | same | 0.851 / 0.845 / 1.000 | same |
| memcpy_144 | 0.934 | 1.044 | 1.000 | 0.923 |
| memcpy_192 | 0.826 | 0.914 | 1.000 | 0.923 |
| memcpy_256 | 1.155 | 1.045 | 1.077 | 1.077 |
L1 = 193-256 takes the branch, L2 = 193-256 falls through the compare. Instr (Skylake-SP, -AVX2): 129-192 51.4 -> 45.4,
193-256 51.4 -> 53.4; AVX2 level unchanged.

## Not to redo
The copy split in either layout; the fill split (TIMING-IVB-MEMREF-ENTRY-2). The code, the small_copy test
moves_of_128_to_193 and the cbench_memref 192 rows are at fb8480f9 / 464bc9ba on task/sse2-copy-mid-split for reuse;
handoff/SSE2-COPY-MID-SPLIT-plants.txt plants that reverted code (historical, do not run against next).

## Open
- Why memcpy_256 got slower was never profiled (the compare, or the 8-block body's new address, is suspected).
- INFORMATIONAL: the Ivy Bridge timing runs end rc 1 on BOTH trees because next's bcmp_eq_128 exceeds its Ivy Bridge
  cycle limit (cycles ratio 1.42-1.45 vs limit 1.383 in bench/host_cycle_limits.tsv; FAIL line in every run). This
  was the cause of sse2-copy-mid-split-ivb / -ivb2 rc 1; it is not this change (bcmp, and present on next).

## Next step
None for this story. Optional follow-up (not ordered): profile memcpy_256 on Alder Lake-N (cycles, uops, front-end
stalls) next vs layout 2 before any third attempt, keeping the 8-block body at next's address. Separately, the
bcmp_eq_128 Ivy Bridge cycle limit on next needs its own story.

## Plan rows to read
D-4.55, D-4.61 (s04), saturation section 3 rows for TIMING-IVB-MEMREF-ENTRY-2 and SSE2-COPY-MID-SPLIT.
