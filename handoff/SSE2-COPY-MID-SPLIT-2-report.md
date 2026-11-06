# SSE2-COPY-MID-SPLIT-2 report

Result: COMPLETE. Ruling #3608 applied: the SSE2 129-192 copy split is dropped and only the record lands.

## Commits (branch task/sse2-copy-mid-split-2)
- bc81bdbd: merge origin/next (the brief allowed this merge; it was clean).
- 60a4d773: revert the copy split. csys/src/machine/fold.S, crates/boundary/tests/small_copy.rs and
  bench/c/cbench_memref.c restored to origin/next; `git diff origin/next` now touches only docs, plan and handoff.
- b7dc73b8: D-4.61 marked REJECTED (keeps the measurements and points to the code at fb8480f9 / 464bc9ba on
  task/sse2-copy-mid-split); NEGATIVE register row in docs/saturation/03-memory-and-string-kernels.md after the
  fill-half row from TIMING-IVB-MEMREF-ENTRY-2, and that row's flag updated to point to it; handoff rewritten.
- the report commit (this file and handoff/reports/SSE2-COPY-MID-SPLIT-2.json).

## Jobs (keke, role n2)
- sse2-copy-mid-split-2-static: `cargo xtask static && cargo clippy --workspace --all-targets --all-features -- -D
  warnings`, rc 0 (every static check ok; clippy clean). Ran on b7dc73b8; the report commit after it changes only
  handoff/. The coordinator note said docs only: no gate, no plants, no timing.

## Earlier failed job (brief step 1)
sse2-copy-mid-split-ivb2 (kotori) ended rc 1 because of bench/host_cycle_limits.tsv on Ivy Bridge: next's own
cbench_memref/bcmp_eq_128 has cycles ratio 1.42-1.45 against its limit 1.383 (FAIL line) in every run of both the
next and the head tree ("=== run 4 next rc=1"). The cause is outside this change (bcmp, on next). With the split
reverted there is nothing in this story to fix. INFORMATIONAL: that limit needs its own story (re-calibrate or
fix bcmp_eq_128 on Ivy Bridge).

## Rulings
None needed.

## Refusals
None.

## Leftovers
Nothing running. handoff/SSE2-COPY-MID-SPLIT-plants.txt is historical: it plants the reverted code.
