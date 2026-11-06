# XTASK-BENCH-OUTPUTS

State: branch task/xtask-bench-outputs, base origin/next c101c4c6. Commits 4395443f (code), the fixture fix after
it, the docs/plants commit, then this handoff. Built and tested on keke role n3 (~/w/n3; logs in ~/w/xbo/).

## What changed
- `bench --counters` (xtask/src/bench.rs `counted_table` -> `count_rows` + `write_counters`): `counters.tsv` is
  rewritten with THIS run's rows only, also when a counted run fails part-way (the rows counted before the failure;
  header only when none), and `counters-<bench>.tsv` is written per bench the run started, with that bench's rows only.
- `bench --compare --counters` (it never wrote counters.tsv before; probably the source of the stale cbench_hot rows
  in TIMING-IVB-MEMREF-ENTRY): writes the same two files in the same 14 columns (Liella vs glibc dynamic; one counted
  run per side, so `spread` is `-` and `suspect` empty). counters.tsv is emptied at the start of the run.
- Columns unchanged, header still the first line (no comment line), so bench/limits_from_counters.py keeps working
  on either file.
- Staged trees (`bench-stage.manifest` at the root): every table (ratio_host, rounds, bench_times, decision,
  counters*, measure, *.compare) goes to `<staged root>/bench-out/` (`bench::out_dir`), never into target/liella.
  `bench --stage` leaves the build host's run tables (`bench::is_output`) out of the products. The hash refusal
  of a changed product is unchanged. Unstaged trees keep target/liella/bench/.
- docs/benchmarks.md: the limits_from_counters line and the staging section say where the tables go.
- Tests (no spec clause: developer tooling): `bench::tests::counters_files_hold_only_their_own_runs_rows`,
  `benchstage::tests::a_staged_tree_runs_twice_and_its_files_stay_as_unpacked`. Tests calling `run`/`compare` now
  hold `bench::RUN_LOCK` (the run flags STAGED/NO_BUILD/COMPARE_NO_BUILD are process-wide; parallel tests could flip
  them under each other); the existing staged test takes it too.

## Results
- clippy rc 0, static rc 0 (keke n3, ~/w/n3-xbo-clippy.log, ~/w/n3-xbo-static.log).
- keke:xbo-gate1 (direct tier = xtask's own suite; `affected --list --base origin/next` says FULL, escalated by the
  xtask/src change): `~/bin/gate 16G cargo test -p xtask --release`: 205 passed + 3 passed, 0 failed; peak 1.7G, rc 0.
- keke:xbo-plants1 (`cargo xtask plants handoff/XTASK-BENCH-OUTPUTS-plants.txt --dir ~/w/xbo/plants-1`): 6 CAUGHT,
  0 SURVIVED, 0 WARN-RC-ONLY, 0 WARN-NO-MSG; one WARN-NOT-RAM (keke has no /mnt/zram/claude; the copy was on nvme).
  Plants: staged-run-writes-into-products, stage-includes-old-run-tables, per-bench-file-mixes-benches,
  failed-count-keeps-old-rows, compare-writes-no-counters, compare-keeps-old-file-on-early-failure.
- No timing (tooling only). Nothing left running; keke ~/w/xbo/ keeps the logs and the plant copy (delete freely).

## Readers of the tables (for the coordinator; liella-ops is out of scope here)
- In the repo: bench/limits_from_counters.py (counters.tsv; format unchanged), bench/timing_history.py
  (decision.tsv), tools/prof/measure_summary.py (measure.tsv): unstaged paths unchanged.
- liella-ops: only state/PH14-OWNER-RULINGS.md names counters.tsv (text). Timing-host procedures that `scp`
  `out/counters.tsv` / rounds.tsv / bench_times.tsv from a staged tree (handoff/timing-sweep-1105.md style) must read
  `<dir>/bench-out/` now.

## Open
- INFORMATIONAL: no D-row added (the earlier XTASK-BENCH-STAGE handoff found `ops reserve` refuses a D-X id). Proposed
  text: "a staged tree's bench tables go to bench-out/, never into the staged products; every --counters run
  rewrites counters.tsv with its own rows and writes counters-<bench>.tsv".
- `cargo xtask affected --base origin/next` escalates xtask/src changes to FULL; the direct tests are xtask's own suite.

## Not to redo / next step
Code, tests, plants and docs are done. Next: coordinator review; update the timing-host procedures to read bench-out/.
