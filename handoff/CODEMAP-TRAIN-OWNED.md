# CODEMAP-TRAIN-OWNED: docs/codemap is train-owned (static's codemap check fatal only on the train)

State: branch `task/codemap-train-owned`, cut from origin/next 9af6dff86. Code commit f7a1becd6; HEAD is the handoff
commit on top of it. Decision in force: D-X.66 (amends D-X.59). Records: `records/decisions/D-X/D-X.66.toml`. No view
was regenerated or committed.

## What changed
- `xtask/src/codemap.rs`: `run(root, check, in_place)` → `run_on(.., train)`. With `--check`, a stale map off the train
  prints `codemap: N files pending regeneration by the train` and returns Ok. Under `LIELLA_TRAIN=1` it is fatal:
  `<files> out of date: run cargo xtask codemap --in-place`. Without `--check`, generation writes
  `target/records-views/docs/codemap/{symbols.tsv,README.md}` (`records::out_root`/`OUT_OF_TREE`, the D-X.59 place) and
  leaves the tracked files alone. It writes in place only under `LIELLA_TRAIN=1` or `--in-place`
  (`records::in_place_env`). Errors from `generate` (an asm export without a label, a dead path in the hand-written
  README part) are still fatal everywhere.
- `xtask/src/main.rs`: wires up `--in-place`. Static calls `codemap::run(r, true, false)`. The usage line now shows
  `codemap [--check] [--in-place]`.
- Tests (no spec clause): the new `codemap::tests::train_owned_check_and_out_of_tree_generation` checks four things:
  the off-train check is advisory, the on-train check is fatal on a stale copy, off-train generation leaves the tracked
  files byte-identical and writes out of tree, and in-place regeneration is clean on the train. Updated:
  `removed_file_leaves_no_row_and_check_sees_it` (now on the train, in place).
- Plants: `handoff/CODEMAP-TRAIN-OWNED-plants.txt` has three plants: the off-train check made fatal again, the on-train
  check made advisory, and off-train generation writing in place.

## For the coordinator (liella-ops, not done here)
- Train generators: add `cargo xtask codemap --in-place`, or a plain `cargo xtask codemap` with `LIELLA_TRAIN=1` set.
  Run it after any generator that changes exports or tests, and before static.
- Train-owned paths: `docs/codemap/symbols.tsv` and `docs/codemap/README.md`. The README's hand-written part (from the
  first non-generated `## ` heading to the end) is still edited on story branches: generation keeps it verbatim from the
  tracked file. A merge conflict in the generated part of the README is resolved by regenerating; a conflict in the
  hand-written part needs a normal merge.
- An xtask from before this change does not know `--in-place` for codemap (it ignores it and writes in place anyway), so
  the train can switch before this lands.

## Results
See the report record `handoff/reports/CODEMAP-TRAIN-OWNED.json` and the job logs on keke (`~/logs/<label>.log`):
codemap-train-owned-clippy, -static, -gate, -plants, -proof (one sequential driver, `~/work/codemap-train-owned/drive.sh`;
the proof script is `~/work/codemap-train-owned/proof.sh`).

Results on f7a1becd6 (keke r1):
- clippy rc 0
- static rc 0 (`codemap: 0 files pending regeneration by the train`; `records fmt --check` and `records check` ok)
- proof rc 0 (all six steps passed: the stale copy is advisory off the train, including for a full static, and fatal
  on the train with rc 1; off-train generation leaves the tracked files untouched; in-place regeneration is clean)
- plants rc 0 (3 CAUGHT, 0 SURVIVED, 0 WARN)
- gate PENDING: `affected --run --base origin/next` escalated to FULL because xtask changed, and was still running at
  hand-back. Re-attach: `ssh claude@10.0.100.53 '~/bin/jobwait codemap-train-owned-gate'`.

## Notes
- I wrote D-X.66.toml by hand in the shape `records new` produces, because the worktree host has no cargo. The static job
  runs `records fmt --check` and `records check` to validate it.
- The generated README header still says "generated: `cargo xtask codemap`". I left it unchanged so this branch does not
  shift the generated text.

Next step: land; then the coordinator adds the generator and paths above to the train.
