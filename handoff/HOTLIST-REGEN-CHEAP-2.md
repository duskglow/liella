# HOTLIST-REGEN-CHEAP-2: cheap regen of both profiles in 298 s, byte-identical to the full path; runner tested and planted

State: branch `task/hotlist-regen-cheap-2` (it carries HOTLIST-REGEN-CHEAP's commits; read handoff/HOTLIST-REGEN-CHEAP.md
for the earlier work), from origin/next 9b2a0c029. Code commit cfd8a3a3c, then this handoff, the plants file and the
updated pending record `records/pending/HOTLIST-REGEN-CHEAP-1.toml` (D-NEW, series D-X; the coordinator assigns the id).
Gate: `affected --base origin/next` escalates to FULL (xtask/src/main.rs, so.rs, hottext.rs). Per ruling #5092 it runs
as the named job `hrc2-gate-full` on shiki (~90 min), left running at hand-back.

## #5093: why the fast merge changes the release build's inputs (settled)
Mtimes are not the cause. The bytes change. I replayed the last run's fast merge (mei, tree 9b2a0c029, the saved
`target/product/regen/fast.trace` and `inputs/`) into a scratch dir (script ~/work/HOTLIST-REGEN-CHEAP-2/why.sh on mei):
- hot-text.list after the fast merge: **same key set, same profile columns** (45 `-`, 97 fast, 385 fast,release,
  41 release), but **not the same order**: 38 diff lines. The memcpy path (`liella_internal_memcpy`,
  `conv::internal_move`, `hot::mem::internal_large`, `liella_machine_copy_avx2`) and `stdio_compat::load` move.
- startup-text.keys: 4 keys reordered.
- Cause: merge.py puts the new trace's first-touch order first (by design, HOT-ORDER). The fast and release builds
  touch the memcpy path at different points, so the fast merge always reorders, and the release merge puts the
  release order back. The tracked lists hold the release order, so this repeats on every regen, steady state included.
- The build sees that order. The release build reads the lists (grouping, start-up read-only group). Also
  `LIELLA_BUILD_TAG` (so.rs `build_tag`) is an FNV hash of the **contents** of every file under crates/, csys/ and
  include/, which `rt`, `ld` and `boundary` embed through `option_env!`. So any byte change of a list recompiles those
  crates and their dependants. crates/libc-so has no build.rs, rustc reads no list, and xtask post-processes a fresh
  archive copy on every `so`, so mtimes trigger nothing.
- Consequence: the release build made ahead of the fast merge (the old `--both`) was never the one traced. It now
  builds once, after the fast merge (commit cfd8a3a3c). This is the old rebuild branch, so the outputs are unchanged.
- Write-only-on-change is implemented anyway (`runner.sh put_changed` for hot-text.list, startup-text.keys and
  probe-text.keys; startup_data.py compares before it replaces), and tested (equal bytes keep the file and its mtime).
- "Release products not rebuilt afterwards" **cannot hold** while the build tag hashes the lists: the last release
  build of a regen used the fast-reordered lists, so the next release `so` recompiles. Measured (hrc2-regen-cmp): after
  the regen, a plain `so` of each profile recompiled both raw archives (`target/product/<p>/libc.a` mtimes moved).
  On this tree that is expected for both profiles, because the regen changed probe-text.keys (1 line) and
  startup-data.keys (33 lines) against the stale tracked copies (#5094). In the steady state only release would
  recompile. The one-profile full path leaves fast stale in the same way. See RULING NEEDED below.

## Measured on shiki (Haswell E5-2697 v3, 26 threads, load 1.3 -> 2.6; job hrc2-regen-cmp, script ~/work/HOTLIST-REGEN-CHEAP-2/regen_cmp.sh)
`regen.sh . --both --allow-drop` on the role tree (cfd8a3a3c, lists = next's), **298 s wall (5.0 min)**:
build-fast 79, traces-fast 49, merge-fast 5, build-release 145, traces-release 15, merge-release 3, recheck 2 (s).
The old order measured 324 s on mei. Here the speculative build is gone and the fast traces now run on their own.
**All five outputs (hot-text.list, startup-text.keys, probe-text.keys, startup-data.keys, hot-text.absent) are
IDENTICAL (cmp) to the full path's outputs of the same tree measured on mei (hrc-regen-cmp, 1069 s).** That is
across hosts as well. The lists were restored afterwards.

## Tests and plants
- `xtask hottext::tests::regen_runner_runs_traces_together_and_keeps_their_order`: a fake tracer (HOTTEXT_GDB, its
  barrier directory written into the script, so `env -i` callers work too) waits until all 4 traces have started,
  with an iteration budget used only as a hang guard. It checks the tagged start order, each trace's own output and
  environment, that a failed trace fails the wait, and put_changed. The test names `tools/hottext/runner.sh` and
  `regen.sh`, so `affected` selects the xtask tests for a tools/hottext change (no affected.rs change needed).
- Plants (handoff/HOTLIST-REGEN-CHEAP-2-plants.txt, job hrc2-plants, `cargo xtask plants`): serial (subshell in the
  foreground) CAUGHT, drop-entry (the 2nd trace never reaches tagged, so its keys leave the list) CAUGHT, put-always
  CAUGHT. Deviation from the brief: "drop one list entry" is caught by the runner test, not by the 20+ minute
  full-vs-cheap comparison. Running that comparison as a plant needs two full regens in a cold copy.

## RULING NEEDED
- Build tag and lists: exclude the generated layout lists (crates/libc-so/hot-text.list, *.keys, hot-text.absent)
  from `build_tag`. They are linker-layout inputs that only xtask post-processing reads. A list-only change would then
  rerun the grouping without recompiling rt, ld, boundary and their dependants. The regen's release build after the
  fast merge (145 s here) would mostly go away, and a regen would leave no profile stale. Recommend: yes, if the tag
  only pairs products that share Rust code (the owner/maintainer decides what the tag identifies).

## Next step
HOTLIST-TRAIN-REGEN step 2: the train command can call `tools/hottext/regen.sh . --both --allow-drop` (about 5 min
for both profiles; the 15 min line holds). Then the #5094 stale tracked copy is refreshed by the train.
Plan rows: D-X.49, D-X.50, D-X.62, D-X.63.
Process note: one empty `python3 - <<'X'` heredoc was run by mistake (it did nothing). I report it as a rule
exception. Nothing of mine is running except hrc2-gate-full.
