# PERF-QSORT-COMPARISONS: why Liella's qsort makes ~4% more comparisons than glibc (COMPLETE, NEGATIVE)

State: branch `task/perf-qsort-comparisons`, cut from origin/next ff125cce. Role keke:~/w/n4. No library code changed: the
only commit adds the NEGATIVE update with the comparison-count table to `docs/saturation/19-qsort-element-sizes-and-fts-at-2-kib.md`
(section "Update: comparison counts per input pattern") plus this note and the report json.

## Decisions in force

* D-14.1 (stable, never fails) and D-14.18 (index merge sort, runs of 8 by binary insertion, one-comparison in-order test,
  descending detection): unchanged. No new D-row: nothing was decided except to keep the current strategy.

## Result

* The table (Liella / glibc comparator calls, 10 patterns x 7 sizes) is in section 19. Random: 1,830 / 1,720 at 256,
  9,316 / 8,958 at 1,024. Sorted, reversed and all-equal input cost Liella n - 1 (glibc about n/2 log2 n); organ pipe,
  sawtooth and 1%-swapped input are up to 3.6x cheaper on Liella; few-distinct keys (4 keys) cost Liella about 1-2% more.
* Cause, from a model (scratch `sim.py`) that reproduces every Liella cell exactly: at 1,024 elements about 236 extra
  comparisons come from the runs' predecessor test before the binary search (unbalanced search on random input) and 127
  from the in-order test before each merge. Removing both gives 8,953 = glibc's count. These two tests are exactly what
  makes ordered input cost n - 1.
* No comparison of the current sort is redundant, so every candidate that saves on random rises somewhere. Ten candidates
  (adaptive predecessor tests, tallies, pure binary insertion, conditional or back-off in-order tests, run lengths 5 to 16)
  were modelled on 150 cells; every one rises on at least one cell (table of candidates in section 19).

## Evidence and jobs (keke)

* pqc-percall-before: `validation/wl_percall.sh --spec validation/percall/qsort.c` on ff125cce, rc 0: e4_256 83012 /
  59665 (1.39), e64_256 87724 / 73273 (1.20), e4_1024 398584 / 284113 (1.40), e8_1024 396876 / 290558 (1.37),
  e64_1024 417268 / 337983 (1.23); comparator-only control 186910 both. The "MISMATCH" lines are the printed
  comparison counts (1,830 vs 1,720; 9,316 vs 8,958), different by design. After = before (no library change).
* glibc probe counts: keke:~/work/perf-qsort-comparisons/glibc.txt (run directly, a few ms). Liella probe: job pqc-counts
  (static against ~/wl/products/ff125cce.../libc.a, output liella.txt), rc 0: all 70 cells equal the model's, every
  sort sorted and stable.
* gate: pqc-gate (clippy, static, `cargo xtask affected` direct tier on the docs-only commit), started detached at
  hand-back; re-attach `ssh claude@keke '~/bin/jobwait pqc-gate'`.
* Plants: none. No library or test code changed, so there is nothing to plant.

## Open questions / rulings

* INFORMATIONAL: the best trade (predecessor test only after an insertion at the end of the run) gives random -1.9% at
  1,024 with rises of up to 14 comparisons in 20 of 150 cells. Recommendation: do not take it (it is a rise on real
  partly ordered input for ~2% of comparisons; the qsort gap is mostly instructions per comparison, 1.13 to 1.40x).

## Not to redo

The model and the 10 candidates above; the per-pattern table.

## Next step

None for the count. The qsort rows' gap is per comparison: the ping-pong merge over two buffers (section 19 "Not tried")
is the next idea, judged by percall counts with sorted input included.
