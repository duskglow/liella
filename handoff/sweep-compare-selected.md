# SWEEP-COMPARE-SELECTED (handoff)

State: branch task/sweep-compare-selected, code head 83a7d859 (stacks on TIMING-SWEEP-LIBM-1107, 4c0fec7c/cbb84c16).

Done:
- `sweep run` writes a `selected` column in sweep.tsv (yes: a selected ratio_host row; no: a counted row outside
  the selection, i.e. the libm calibration rows add_baseline/mxcsr_pair/mxcsr_wrapper_seq).
- `sweep compare` judges only `selected` yes rows (counts, spread, change, lists). Unselected rows get their own
  list ("Rows outside the selection") and are never listed as "not measured now". A table with no `selected`
  column has every row judged, as before, and the report prints a line saying so above the counts.
- The default `sweep run` selection now includes cbench_libm. With libm rows, `sweep run` writes and prints
  `<out>/libm-families.md` and `sweep compare` prints a per-class table of median time ratios per family and
  precision, using the TIMING-SWEEP-LIBM-1107 grouping (exp/log/pow, trig, hyperbolic, gamma, bessel, erf, roots/hypot,
  complex, special = rows named `<fn>_<case>`, other).
- docs/benchmarks.md section 6 (periodic sweep) is updated: the staging command now includes libm, and the column,
  compare and family-table text is new.
- Tests (xtask, no spec clause): compare_judges_only_the_selected_rows, libm_rows_are_grouped_by_family_and_precision,
  plus the existing sweep tests updated. 9/9 pass on mei (job scs-lint2, which also ran clippy and static: both clean).

Jobs (mei, role s1, head 83a7d859): gate `scs-gate` (affected --run --base origin/next), plants `scs-plants`
(handoff/SWEEP-COMPARE-SELECTED-plants.txt, 7 plants). Not collected at hand-back. Re-attach: `~/bin/jobwait scs-gate scs-plants`,
then `~/bin/test-summary ~/logs/scs-gate.log` and `tail ~/logs/scs-plants.log`.

Not verified: a real sweep with libm in the default selection (expected about 2 more minutes per host).
Decisions: none new beyond this note; family grouping matches the 1107 results file (row counts per family match).
Next step: collect scs-gate/scs-plants. On the next sweep, compare against the 1107 libm tables. Those tables have no
`selected` column, so expect the note line, and the calibration rows count only in the old table's judged numbers
when they are also judged in the new table.
