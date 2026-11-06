# SWEEP-RUN-TOOL (timing-sweep runner and analyzer)

State: branch task/sweep-run-tool, base origin/next 6faf9548. Code: xtask/src/sweep.rs (new), xtask/src/main.rs
(dispatch, usage, module table). Docs: docs/benchmarks.md section 6, "The periodic timing sweep". Plants:
handoff/SWEEP-RUN-TOOL-plants.txt.

## What it does
- `./xtask-bin sweep run --out <dir> [--only a,b]` (staged tree, timing host) runs
  `bench --no-build --staged --counters --only <groups>` (default: every cbench_* group but cbench_libm, the same 17
  groups as 1106/1106E). It exits 0 when every selected ratio_host row has a median in this run's ratio_host.tsv,
  rounds.tsv and counters.tsv were written, and every line of the bench's `budget regressions` report (if rc != 0) is
  an `exceed` limit flag. Flagged rows go into `<dir>/sweep.tsv` (`flagged yes`) and the `sweep: COMPLETE:` line
  (`<dir>/summary.txt`). It exits nonzero (summary `sweep: INCOMPLETE: <why>`, no sweep.tsv) on a signal, a missing
  table, an unmeasured row, or any other bench failure. Stale tables (bench-out/ and <dir>) are deleted before the run.
- `cargo xtask sweep compare --new ivb=<a>/sweep.tsv --new n100=<b>/sweep.tsv (--old ivb=... | --old-table
  results/timing-sweep-1106e-table.md) --label 1106e` prints: counts per class (now and before), spread
  median/p90/max, change now/before median/p10/p90, lists (a)/(b)/(c), new rows, rows gone, deferred wordexp rows.
  These are the sections and thresholds of the 1106E analyze.py. Additions: the "rows gone" list, and the counts
  for the previous sweep use the rows measured now, as 1106E did.

## Sweep brief job (per timing host)
`./xtask-bin sweep run --out ~/work/<STORY>/out` in the unpacked stage, run through jobrun. Its rc is the job verdict.

## Decisions (local, recorded here; no D-row needed)
- A nonzero bench exit counts as "finished" only if its report lines all match `<bench>/<row>: ... exceed...`.
  Any other report line ("no counted instruction value", "counters table failed", ...) counts as unfinished.
- Rows without counters (about 40 per host in 1106E, e.g. cold_exec_*) are legitimate: instr/cycles show `-`.
  Only an empty counters.tsv fails the run.

## Open / next
- Next sweep brief: name the job above, then run `sweep compare --old-table results/timing-sweep-1106e-table.md`
  (that file is on task/timing-sweep-1106e, not on next) to get the report sections.
- Not tested on a real timing host. The brief asked for fixtures only. A real run is the next sweep.

## Not to redo
The design and the fixture tests. Gate and plants results are in handoff/reports/SWEEP-RUN-TOOL.json.
