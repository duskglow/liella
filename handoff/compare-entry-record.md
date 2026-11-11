# COMPARE-ENTRY-RECORD: saturation record for COMPARE-ENTRY-RETHINK-2's negative result

State: branch `task/compare-entry-record`, cut from origin/next 28a903a01. Records only, no code.

## What was done
* `records/saturation/SR-88.toml` (section 3, reserved id SR-88): the short compare entry of `memcmp` / `bcmp` /
  `__memcmpeq` (rows `memcmp_diff8_64/96/127`, `__memcmpeq_head_128/1k`, plus `bcmp_head_*`, `*_eq_128`,
  `memcmp_256`). NEGATIVE, next kept. It records:
  * H2 (head block before dispatch), S1 (small sizes fall through), (c) (AVX2 span test first): each traded rows
    (#3977): a runtime ISA test in the shared entry costs the small rows one quantum; head block + test in front
    costs the N100 AVX2 33-128 rows.
  * Per-ISA entries S/A/B and the bound pointer P, with the rethink's ratio tables (Ivy Bridge memcmp S 0.576 of
    next; N100 S 1.25-1.73 on 33-128 rows; P 1.07-1.33 per call) and the Skylake-SP in-function counts.
  * Why each was rejected: classed swap off by owner ruling (#3994, +29.3 us / +65.5k kernel instr per static start
    on the N100); pointer one quantum per call; S alone regresses the N100; prototype small paths and long loop
    worse than next's.
* Numbers are taken only from `handoff/compare-entry-rethink-2.md` and its report json on next (the H2 stories'
  own handoffs are not on next and were not read; the brief named none of them).

## Decisions in force
D-4.53 (classed slots, M6 swap off by default), owner rulings #3977, #3994. No new decision.

## Not to redo
H2, S1, (c), a static bound pointer, or landing the SSE2 entry alone (see SR-88 `tried`).

## Open / next step
* Optional (backlog item 18): measure whether the 29.3 us static-start cost is paid once per process however many
  function families share the classed swap; if so, #3994's trade changes.
* What is left of the rows waits on dynamic IFUNC in the phase-14 loader (COMPARE-IFUNC-DYNAMIC).
* The train regenerates docs/saturation-register.md and docs/saturation/03-*.md; this branch does not touch them.

## Jobs
kanon `compare-entry-record-static2` (`cargo xtask records check && cargo xtask static` on the synced head).
