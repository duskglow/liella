# XTASK-BENCH-COMPARE-STAGED handoff

State: branch `task/xtask-bench-compare-staged` (from origin/next, stacks on XTASK-BENCH-OUTPUTS). Code commit
e74768be; this handoff, the report record and a docs correction are in the commit after it (see `git log -2`).
Decision in force: D-NEW in plan/07-decisions/x-implementation-decisions.md (placeholder id: the brief reserved none).

## Cause (found by inspection on the N100 timing host nozomi, read-only)
`bench --compare` and plain `bench` already used the same root. The tree in nozomi
~/work/COLDSTART-SECOND-LATENCY-3/tree came from a hand-repacked stage.tar (entries `./tree/...`, plus
probe.liella.after) that held `bench/`, `target/liella/`, `xtask/Cargo.toml` and `xtask-bin` but **no `Cargo.toml` and
no `bench-stage.manifest`**. So xtask's `root()` found no marker and fell back silently to the build-time
CARGO_MANIFEST_DIR (a path on the build host that does not exist on nozomi): `bench/c` was empty, hence "Valid names: .".
The plain command fails in that same tree too (`./xtask-bin bench --no-build --only zz` -> "bench/budgets.tsv: No such
file"); it worked in TIMING-SWEEP-1106/tree because that tree is a real `bench --stage` tarball (holds the manifest), where
`--compare`'s name check already listed the real names. The repack follows developer-flow.md's timing recipe ("stage
`target/liella`, `bench/`, `xtask/Cargo.toml` and the debug xtask binary as a tar", from handoff/timing-sweep-1105.md
"Stage"), which leaves out `Cargo.toml` and the manifest.

## Change
- `benchstage::find_root` (used by main's `root()`): the nearest directory at or above the working directory with
  `Cargo.toml` and `xtask/Cargo.toml`, or with the manifest; else the build-time root only if it is a root; else exit 2 with an
  error naming the directory and pointing to `bench --stage` (no more silent fallback to a path that does not exist).
- `bench --compare --no-build` verifies a staged tree against its manifest (`load_verified`, as plain `--no-build` does)
  and prints the `staged:` header.
- Unknown bench name: "Valid names (the *.c files in <dir>): ..." or "<dir> holds no *.c file".
- docs/benchmarks.md (staged section) updated.

## Results (kanon role kn1, Broadwell E5-2699 v4)
- kanon:xbcs-gate2: clippy, static, `~/bin/gate 16G cargo test -p xtask --release`: 206 + 3 passed, 0 failed, peak 1.7G,
  rc 0. Direct tier = xtask's own suite: `affected --list --tier direct --base origin/next` escalates xtask/src to FULL
  (precedent XTASK-BENCH-OUTPUTS xbo-gate1). SUPERSEDED: kanon:xbcs-gate1 (`affected --run`, escalated to FULL, stopped with
  kill-label by me); kanon:xbcs-check1 (red: the test tree sat under the workspace's target/, so the ancestor search
  found the workspace; moved to temp_dir); kanon:xbcs-check2 (green, before the final commit).
- kanon:xbcs-plants1: `cargo xtask plants handoff/XTASK-BENCH-COMPARE-STAGED-plants.txt`: 6 plants, 6 CAUGHT, 0 WARN.
  Plants: old fallback to a non-root build-time path, build-time root preferred over the staged tree, manifest not a root,
  compare skipping the hash check, list without directory, empty list message.
- kanon:xbcs-staged1: `bench --stage --only cbench_opt` on the role, unpacked to ~/work/XTASK-BENCH-COMPARE-STAGED/tree
  (outside any workspace), `./xtask-bin bench --compare --no-build --counters --runs 1 cbench_opt`: rc 0, staged header
  (67 products match), unknown name refused with the real list (also from bench/c/), and every second_* row has
  glibc dynamic, Liella and glibc static times (example second_bindtextdomain_gettext 648 vs 7849 ns, ratio 12.11). These are
  ONE round on an unbooked build host: a proof that it runs, NOT timing evidence.

## INFORMATIONAL
- The second_* rows print "-" for instr/op in `--compare --counters` (no counter rows for them); not changed here.
- Tests now: `benchstage::tests::compare_in_a_staged_tree_finds_its_root_and_benches` (new),
  `bench::tests::compare_rejects_unknown_bench_name_listing_valid_ones` (extended).
- Process note: I wrote my scratch edit scripts (python, in $S/work) through Bash heredocs that contained backslashes,
  against thread_common's Write-tool rule; the resulting diffs were checked and are correct. Later files used Write.

## RULING NEEDED
- The developer-flow.md timing recipe (and handoff/timing-sweep-1105.md "Stage") should say `cargo xtask bench --stage
  <out.tar> --only ...` and unpack the tarball whole; hand-picked trees now stop with a clear error instead of failing
  oddly. Recommendation: the coordinator updates the recipe (liella-ops is read-only for me).
- D-NEW needs its real id (next free D-X row, D-X.31 if none was taken since).

## Next step
Coordinator: assign the D-id, fix the recipe, re-run COLDSTART-SECOND-LATENCY-3's timing from a real `--stage` tarball
on the booked N100.
