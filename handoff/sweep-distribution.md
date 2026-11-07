# SWEEP-DISTRIBUTION (handoff)

State: branch task/sweep-distribution, code head 9d01eff39 (on origin/next 3bf4f6c7b, which already holds
SWEEP-COMPARE-SELECTED). Role keke:~/w/n1.

Done:
- `xtask sweep run` and `sweep compare` print a magnitude section per host (`distribution()` in xtask/src/sweep.rs).
  It has four parts. First, the row count per time-ratio bucket: <= 1.05, 1.05-2, 2-10, 10-100, > 100. The edges
  are upper-inclusive, so a ratio on an edge counts in the lower bucket. Second, the five slowest rows with their
  ratio. Third, a `RETHINK?` line naming every family whose median is above 10x. Fourth, a family table with the
  median per family and the mark.
- Families: a `cbench_libm` row is grouped by family and precision, the cells of the libm table
  (`cbench_libm trig long double`). Calibration names fall in `cbench_libm other`, but they are not selected, so
  `sweep run` and compare leave them out. Every other row is grouped by its bench. Deferred `wordexp` rows are left
  out.
- `sweep run` also writes `<out>/distribution.md` and deletes an old copy before the bench starts. In `sweep
  compare`, the section appears per class after the counts.
- Tests (xtask, no spec clause): time_ratio_buckets_have_exact_edges and
  distribution_counts_buckets_names_the_slowest_and_marks_families_above_10x. run_finishes_with_flagged_rows_and_writes_the_table
  now also checks distribution.md.
- docs/benchmarks.md section 6 and the module comment are updated.
- Plants: handoff/SWEEP-DISTRIBUTION-plants.txt (8 plants).

Jobs (keke, role n1, head 9d01eff39):
- sdist-plants: 7 CAUGHT, 0 SURVIVED. One WARN-NO-MSG: deferred-rows-counted did not compile, because its
  replacement dropped the leading `.`. This commit fixes the plant in the plants file.
- sdist-plants2: the fixed plant alone, from ~/work/SWEEP-DISTRIBUTION/plants-fix.txt (the role was busy with the
  gate, so the file could not be synced). It is QUEUED behind the gate.
- sdist-gate: `cargo xtask clippy && cargo xtask static && cargo xtask affected --run --base origin/next`. affected
  escalated to FULL because xtask/src/sweep.rs is part of the gate. It is RUNNING: workspace clippy and stubs were
  ok when I handed back.
Re-attach: `~/bin/jobwait sdist-gate sdist-plants2`, then `~/bin/test-summary ~/logs/sdist-gate.log` and
`grep ^plants: ~/logs/sdist-plants2.log | tail -3`. Superseded: sdist-lint
(first head 784e4fee7): clippy and static passed, and 2 tests failed on expectations. Commit 9d01eff39 fixed them.

Decisions (local, no maintainer ruling needed): the threshold is "above 10x", so a family median of exactly 10.00
is not marked, as the RUT rule words it. For libm, the family unit is the family x precision cell, not the whole
family across precisions: that is where the 1107 gaps sat (long double/complex). The whole-family medians are still
in the libm table.
Open: none. Not verified: output on a real sweep (the next sweep shows it).
Next step: none for this story; when the next sweep runs, check the RETHINK? line against the 1107 libm table.
